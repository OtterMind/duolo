//! 聊天模式：Agent 不在终端里运行，Duolo 通过它的结构化协议对话，界面自己画。
//!
//! 支持的协议见 [`Protocol`]：都是一行一条 JSON，走标准输入输出，对外都是同一个 [`Chat`]。
//! 每种协议只管把收到的消息换成对话的变化（见 `conversation`），握手、开会话、发一句话。
//!
//! 线程：写线程（往标准输入写，Agent 不读也不会卡住调用方）、读线程（回复交给等着的人，别的交给协议）、
//! 读错误输出的线程（留最后几行，启动失败时给用户看），以及按顺序走流程的线程（握手、开会话、一句句发）。
//! 调用方只取结果，不等 Agent。

mod acp;
mod claude;
mod codex;
mod conversation;
mod rpc;
pub mod testing;

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use duolo_core::process;
use duolo_core::proctree::ProcessTree;
use serde_json::Value;

use conversation::Conversation;
use rpc::{lock, Failure, Framing, Incoming, Rpc};

/// 握手、开会话最长等多久（第一次启动可能要装插件、连模型服务）。
const SETUP_TIMEOUT: Duration = Duration::from_secs(120);
/// 结束时先关掉标准输入，等它自己退出多久。
const GRACE: Duration = Duration::from_secs(1);
/// 强制结束后最多等进程消失多久。
const STOP_TIMEOUT: Duration = Duration::from_secs(2);
/// 错误输出留最后几行。
const STDERR_LINES: usize = 20;
/// 设了这个环境变量（一个文件路径）时，把和 Agent 的收发都记到这个文件里，用来录测试数据。
pub const RECORD_ENV: &str = "DUOLO_CHAT_RECORD";

/// 和 Agent 说话用的协议。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Protocol {
    /// ACP（Agent Client Protocol，<https://agentclientprotocol.com>）。
    #[default]
    Acp,
    /// Claude Code 的 stream-json（`-p --input-format stream-json --output-format stream-json`）。
    Claude,
    /// Codex 的 app-server。
    Codex,
}

impl Protocol {
    /// 插件声明里的名字。
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "acp" => Some(Self::Acp),
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            _ => None,
        }
    }

    fn driver(self) -> Arc<dyn Driver> {
        match self {
            Self::Acp => Arc::new(acp::Acp),
            Self::Claude => Arc::<claude::Claude>::default(),
            Self::Codex => Arc::new(codex::Codex),
        }
    }

    fn framing(self) -> Framing {
        match self {
            Self::Claude => Framing::Control,
            Self::Acp | Self::Codex => Framing::JsonRpc,
        }
    }
}

/// 怎么启动。
#[derive(Debug, Clone, Default)]
pub struct Spec {
    pub protocol: Protocol,
    pub program: String,
    pub args: Vec<String>,
    /// 接回会话靠启动参数的协议（Claude）：接回 `session` 时加上的参数，`{session}` 换成会话编号。
    pub resume_session_args: Vec<String>,
    /// 同上，接回这个目录里最近的一次时加上的参数。
    pub resume_args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub env_remove: Vec<String>,
    pub cwd: PathBuf,
    /// 接着这次会话；接不回来时新开。
    pub session: Option<String>,
    /// 不知道会话编号时，接回这个目录里最近的一次（之前在终端里开的会话）。
    pub resume_latest: bool,
    /// 新开会话时第一句话（需求或交接说明）。接回原来的会话时不发。
    pub prompt: Option<String>,
}

/// 对话里的一条。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    User(String),
    Agent(String),
    Tool { tool: String, target: String },
}

/// 从上次取过之后发生的事。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Update {
    /// 这次用的会话编号：新开的，或者接回的。
    Session(String),
    /// 接回会话时它重放的以前的对话（之前可能已经记过，由调用方去重）。
    History(Vec<Entry>),
    Entry(Entry),
    /// 这一轮因为出错停了：协议里的错误类型（例如额度用完了）。
    Failed(String),
}

/// 现在在做什么。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// 在握手、开会话。
    Starting,
    Working,
    /// 在等用户回答（是否允许）。
    Asking,
    Idle,
    Exited {
        code: u32,
        success: bool,
    },
}

/// Agent 在等用户回答的问题。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    /// 这个 [`Chat`] 里递增，回答时带上，免得答到后来的问题上。
    pub id: u64,
    /// 用什么工具、对什么（命令、文件），和 `Entry::Tool` 一样。
    pub tool: String,
    pub target: String,
    /// 选项（Agent 给的名字）。
    pub options: Vec<String>,
}

/// 正在写的话、在想、在用的工具。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Draft {
    pub text: String,
    pub thinking: bool,
    pub tool: Option<(String, String)>,
}

/// 一个在聊天模式下运行的 Agent。丢掉就结束它（在后台）。
pub struct Chat {
    link: Arc<Link>,
    driver: Arc<dyn Driver>,
    child: Option<Child>,
    tree: ProcessTree,
    exit: Option<(u32, bool)>,
    started: Instant,
}

impl std::fmt::Debug for Chat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Chat").field("exit", &self.exit).finish_non_exhaustive()
    }
}

#[derive(Debug, Default)]
struct Shared {
    inner: Mutex<Inner>,
    wake: Condvar,
}

#[derive(Debug, Default)]
struct Inner {
    conversation: Conversation,
    updates: Vec<Update>,
    /// 接回会话时重放的对话，重放完一起交出去。
    history: Option<Vec<Entry>>,
    queue: VecDeque<String>,
    ask: Option<PendingAsk>,
    next_ask: u64,
    session: Option<String>,
    ready: bool,
    working: bool,
    /// 协议自己的这一轮的编号（打断时用）。
    turn: Option<String>,
    /// 这一轮结束了（协议用通知告诉结束时）：成功，或者 Agent 说的原因。
    turn_end: Option<Result<(), String>>,
    /// 读不到输出了（进程退出或者关了标准输出）。
    closed: bool,
    /// 握手、开会话失败：Agent 说的原因。
    failed: Option<String>,
    /// 最近一次出错（一轮对话失败）时 Agent 说的原因。
    error: Option<String>,
    stderr: VecDeque<String>,
}

#[derive(Debug)]
struct PendingAsk {
    ask: Ask,
    /// 要回复的请求。
    request: Value,
    /// 每个选项回给 Agent 的内容。
    answers: Vec<Value>,
    /// 打断时回给 Agent 的内容。
    cancel: Value,
}

impl Chat {
    /// 启动。马上返回，握手、开会话在后台做。
    pub fn spawn(spec: Spec) -> Result<Self> {
        let mut args = spec.args.clone();
        match &spec.session {
            Some(id) => args.extend(spec.resume_session_args.iter().map(|a| a.replace("{session}", id))),
            None if spec.resume_latest => args.extend(spec.resume_args.iter().cloned()),
            None => {}
        }
        let mut cmd = process::command(&spec.program);
        cmd.args(&args)
            .current_dir(&spec.cwd)
            // 有的程序（例如 OpenCode）按 PWD 找项目，见 `Prompted::in_dir`。
            .env("PWD", &spec.cwd)
            .envs(spec.env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for name in &spec.env_remove {
            cmd.env_remove(name);
        }
        let (mut child, tree) =
            process::spawn_tree(&mut cmd).with_context(|| format!("cannot start {}", spec.program))?;
        let stdin = child.stdin.take().context("no stdin")?;
        let stdout = child.stdout.take().context("no stdout")?;
        let stderr = child.stderr.take().context("no stderr")?;

        let (out, lines) = mpsc::channel::<String>();
        let rpc = Rpc::new(out, spec.protocol.framing());
        let shared = Arc::new(Shared::default());
        lock(&shared.inner).conversation = Conversation::new(&spec.cwd);
        let link = Arc::new(Link { shared: shared.clone(), rpc });
        let driver = spec.protocol.driver();
        let record = Recorder::open();

        let writer_record = record.clone();
        thread::Builder::new().name("duolo-chat-writer".into()).spawn(move || {
            let mut stdin = stdin;
            for line in lines {
                if let Some(record) = &writer_record {
                    record.line(">> ", &line);
                }
                if stdin
                    .write_all(line.as_bytes())
                    .and_then(|()| stdin.write_all(b"\n"))
                    .and_then(|()| stdin.flush())
                    .is_err()
                {
                    break;
                }
            }
        })?;
        let (reader_link, reader_driver) = (link.clone(), driver.clone());
        thread::Builder::new()
            .name("duolo-chat-reader".into())
            .spawn(move || read(stdout, &reader_link, reader_driver.as_ref(), record.as_deref()))?;
        let stderr_shared = shared.clone();
        thread::Builder::new().name("duolo-chat-stderr".into()).spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                let mut inner = lock(&stderr_shared.inner);
                inner.stderr.push_back(line);
                if inner.stderr.len() > STDERR_LINES {
                    inner.stderr.pop_front();
                }
            }
        })?;
        let (driver_link, driver_driver) = (link.clone(), driver.clone());
        thread::Builder::new()
            .name("duolo-chat-driver".into())
            .spawn(move || drive(&driver_link, driver_driver.as_ref(), &spec))?;

        Ok(Self { link, driver, child: Some(child), tree, exit: None, started: Instant::now() })
    }

    /// 发一句话。正在干活时排在后面，这一轮结束再发。
    pub fn send(&self, text: &str) {
        self.link.inner().queue.push_back(text.to_string());
        self.link.shared.wake.notify_all();
    }

    /// 打断：停下这一轮，排着的话也不发了，在问的问题当作取消。
    pub fn cancel(&self) {
        let mut inner = self.link.inner();
        inner.queue.clear();
        if let Some(pending) = inner.ask.take() {
            self.link.rpc.respond(pending.request, pending.cancel);
        }
        let working = inner.working;
        drop(inner);
        if working {
            self.driver.interrupt(&self.link);
        }
    }

    /// 回答问题 `ask`（选第 `option` 个，从 0 开始）。已经不在问这个了返回 false。
    pub fn answer(&self, ask: u64, option: usize) -> bool {
        let mut inner = self.link.inner();
        let fits = inner.ask.as_ref().is_some_and(|p| p.ask.id == ask && option < p.answers.len());
        if !fits {
            return false;
        }
        let mut pending = inner.ask.take().expect("checked above");
        self.link.rpc.respond(pending.request, pending.answers.swap_remove(option));
        true
    }

    /// 在等用户回答的问题。
    pub fn ask(&self) -> Option<Ask> {
        self.link.inner().ask.as_ref().map(|p| p.ask.clone())
    }

    /// 上次取过之后发生的事，取一次就清空。
    pub fn take_updates(&self) -> Vec<Update> {
        std::mem::take(&mut self.link.inner().updates)
    }

    pub fn draft(&self) -> Draft {
        self.link.inner().conversation.draft()
    }

    /// 出错的原因（Agent 自己说的，不翻译）：启动失败、一轮对话失败，或者退出前最后的错误输出。
    pub fn error(&mut self) -> Option<String> {
        let failed = matches!(self.status(), Status::Exited { success: false, .. });
        let inner = self.link.inner();
        let tail = || {
            let tail: Vec<&str> = inner.stderr.iter().map(String::as_str).filter(|l| !l.trim().is_empty()).collect();
            (!tail.is_empty()).then(|| tail[tail.len().saturating_sub(5)..].join("\n"))
        };
        inner.failed.clone().or_else(|| inner.error.clone()).or_else(|| failed.then(tail).flatten())
    }

    pub fn status(&mut self) -> Status {
        if let Some((code, success)) = self.poll_exit() {
            return Status::Exited { code, success };
        }
        let inner = self.link.inner();
        if inner.failed.is_some() {
            drop(inner);
            // 握手、开会话失败：这个进程没用了。
            self.stop_in_background();
            return Status::Exited { code: 1, success: false };
        }
        if inner.ask.is_some() {
            Status::Asking
        } else if !inner.ready {
            Status::Starting
        } else if inner.working || !inner.queue.is_empty() {
            Status::Working
        } else {
            Status::Idle
        }
    }

    pub fn started(&self) -> Instant {
        self.started
    }

    /// 进程退出了没有（退出码、是否正常）。
    pub fn poll_exit(&mut self) -> Option<(u32, bool)> {
        if self.exit.is_none() {
            if let Some(Ok(Some(status))) = self.child.as_mut().map(Child::try_wait) {
                let code = status.code().map_or(1, |c| u32::try_from(c).unwrap_or(1));
                self.exit = Some((code, status.success()));
            }
        }
        self.exit
    }

    /// 结束它和它启动的所有进程，等它们真的消失。会等一会儿，不要拿着锁调用。
    pub fn stop(&mut self) {
        self.link.rpc.close();
        let Some(mut child) = self.child.take() else { return };
        stop_process(&mut child, &self.tree, self.exit.is_some());
        if let Ok(Some(status)) = child.try_wait() {
            let code = status.code().map_or(1, |c| u32::try_from(c).unwrap_or(1));
            self.exit.get_or_insert((code, status.success()));
        }
    }

    fn stop_in_background(&mut self) {
        self.link.rpc.close();
        let Some(mut child) = self.child.take() else { return };
        let tree = std::mem::replace(&mut self.tree, ProcessTree::empty());
        let exited = self.exit.is_some();
        self.exit.get_or_insert((1, false));
        thread::spawn(move || stop_process(&mut child, &tree, exited));
    }
}

impl Drop for Chat {
    fn drop(&mut self) {
        self.stop_in_background();
    }
}

fn stop_process(child: &mut Child, tree: &ProcessTree, exited: bool) {
    let members = tree.members();
    if !exited {
        // 标准输入已经关了，大多数 Agent 会自己退出。
        wait_exit(child, GRACE);
        tree.hang_up();
    }
    tree.kill(Some(members), STOP_TIMEOUT);
    if !wait_exit(child, Duration::ZERO) {
        let _ = child.kill();
        wait_exit(child, STOP_TIMEOUT);
    }
}

fn wait_exit(child: &mut Child, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            _ if Instant::now() >= deadline => return false,
            _ => thread::sleep(Duration::from_millis(10)),
        }
    }
}

/// 读线程：回复交给等着的人，别的交给协议处理。
fn read(stdout: impl std::io::Read, link: &Link, driver: &dyn Driver, record: Option<&Recorder>) {
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        if let Some(record) = record {
            record.line("<< ", &line);
        }
        if let Some(message) = link.rpc.dispatch(&line) {
            driver.incoming(link, message);
        }
    }
    let mut inner = link.inner();
    inner.closed = true;
    inner.ask = None;
    drop(inner);
    link.rpc.close();
    link.shared.wake.notify_all();
}

/// 写完的对话交出去；在重放以前的对话时先攒着。
fn collect(inner: &mut Inner) {
    let done = inner.conversation.take();
    match &mut inner.history {
        Some(history) => history.extend(done),
        None => inner.updates.extend(done.into_iter().map(Update::Entry)),
    }
}

/// 按顺序走流程：握手、接回或新开会话，然后一句句发。
fn drive(link: &Link, driver: &dyn Driver, spec: &Spec) {
    if let Err(failure) = driver.open(link, spec) {
        link.fail(failure);
        return;
    }
    link.inner().ready = true;
    loop {
        let text = {
            let mut inner = link.inner();
            loop {
                if inner.closed {
                    return;
                }
                if let Some(text) = inner.queue.pop_front() {
                    break text;
                }
                inner = link.shared.wake.wait(inner).unwrap_or_else(|p| p.into_inner());
            }
        };
        {
            let mut inner = link.inner();
            inner.working = true;
            inner.error = None;
            inner.turn_end = None;
            inner.updates.push(Update::Entry(Entry::User(text.clone())));
        }
        let result = driver.prompt(link, &text);
        let mut inner = link.inner();
        inner.conversation.finish_turn();
        collect(&mut inner);
        inner.working = false;
        inner.turn = None;
        if let Err(reason) = result {
            inner.error = Some(reason);
        }
    }
}

/// 一种协议：怎么握手开会话、怎么发一句话、收到的消息怎么处理。
trait Driver: Send + Sync {
    /// 读线程收到的请求、通知（回复已经交给等着的人了）。
    fn incoming(&self, link: &Link, message: Incoming);
    /// 握手，接回原来的会话（用 [`Link::set_session`] 报会话编号，[`Link::history`] 交以前的对话），
    /// 或者新开（用 [`Link::first_prompt`] 排上第一句话）。
    fn open(&self, link: &Link, spec: &Spec) -> Result<(), Failure>;
    /// 发一句话，等这一轮结束。出错时返回 Agent 说的原因。
    fn prompt(&self, link: &Link, text: &str) -> Result<(), String>;
    /// 打断正在干的这一轮。
    fn interrupt(&self, link: &Link);
}

/// 协议和共享的状态之间：读线程、流程线程、调用方都通过它改状态。
struct Link {
    shared: Arc<Shared>,
    rpc: Rpc,
}

impl Link {
    fn inner(&self) -> MutexGuard<'_, Inner> {
        lock(&self.shared.inner)
    }

    /// 改对话；写完的交出去。
    fn conversation<R>(&self, change: impl FnOnce(&mut Conversation) -> R) -> R {
        let mut inner = self.inner();
        let result = change(&mut inner.conversation);
        collect(&mut inner);
        result
    }

    fn session(&self) -> Option<String> {
        self.inner().session.clone()
    }

    /// 协议报的错误类型。
    fn failed(&self, error: &str) {
        self.inner().updates.push(Update::Failed(error.to_string()));
    }

    /// 这次用的会话编号；变了才报。
    fn set_session(&self, id: &str) {
        let mut inner = self.inner();
        if !id.is_empty() && inner.session.as_deref() != Some(id) {
            inner.session = Some(id.to_string());
            inner.updates.push(Update::Session(id.to_string()));
        }
    }

    /// 新开的会话：第一句话排在最前面。
    fn first_prompt(&self, spec: &Spec) {
        if let Some(prompt) = spec.prompt.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
            self.inner().queue.push_front(prompt.to_string());
        }
    }

    /// 开始重放以前的对话：这期间写完的对话先攒着，你说的话也记下。
    fn begin_history(&self) {
        let mut inner = self.inner();
        inner.history = Some(Vec::new());
        inner.conversation.replaying = true;
    }

    /// 重放完了；`keep` 为假时（没接回来）丢掉。
    fn end_history(&self, keep: bool) {
        let mut inner = self.inner();
        inner.conversation.finish_turn();
        collect(&mut inner);
        inner.conversation.replaying = false;
        let history = inner.history.take().unwrap_or_default();
        if keep {
            inner.updates.push(Update::History(history));
        }
    }

    /// 以前的对话（协议直接给出全部时用）。
    fn history(&self, entries: Vec<Entry>) {
        self.inner().updates.push(Update::History(entries));
    }

    /// Agent 问用户：每个选项的名字和选它时回给 Agent 的内容；`cancel` 是打断时回的。
    fn ask(&self, request: Value, (tool, target): (String, String), options: Vec<(String, Value)>, cancel: Value) {
        let mut inner = self.inner();
        inner.next_ask += 1;
        let (names, answers) = options.into_iter().unzip();
        let ask = Ask { id: inner.next_ask, tool, target, options: names };
        if let Some(old) = inner.ask.replace(PendingAsk { ask, request, answers, cancel }) {
            self.rpc.respond(old.request, old.cancel);
        }
    }

    /// 协议自己的这一轮的编号（打断时用）。
    fn set_turn(&self, id: Option<String>) {
        self.inner().turn = id;
    }

    fn turn(&self) -> Option<String> {
        self.inner().turn.clone()
    }

    /// 这一轮结束了（协议用通知告诉的时候）。
    fn end_turn(&self, result: Result<(), String>) {
        self.inner().turn_end = Some(result);
        self.shared.wake.notify_all();
    }

    /// 等 [`Link::end_turn`]；进程没了也算结束。
    fn wait_turn(&self) -> Result<(), String> {
        let mut inner = self.inner();
        loop {
            if let Some(result) = inner.turn_end.take() {
                return result;
            }
            if inner.closed {
                return Ok(());
            }
            inner = self.shared.wake.wait(inner).unwrap_or_else(|p| p.into_inner());
        }
    }

    /// 握手、开会话失败。
    fn fail(&self, failure: Failure) {
        let mut inner = self.inner();
        if !inner.closed {
            inner.failed = Some(match failure {
                Failure::Rejected(err) => err.message,
                Failure::TimedOut => "timed out".into(),
                Failure::Closed => String::new(),
            });
        }
    }
}

/// 设了 [`RECORD_ENV`] 时把收发记下来（`>> ` 是发的、`<< ` 是收的），用来做测试数据。
struct Recorder(Mutex<std::fs::File>);

impl Recorder {
    fn open() -> Option<Arc<Self>> {
        let path = std::env::var_os(RECORD_ENV)?;
        let file = std::fs::OpenOptions::new().create(true).append(true).open(path).ok()?;
        Some(Arc::new(Self(Mutex::new(file))))
    }

    fn line(&self, prefix: &str, line: &str) {
        let _ = writeln!(lock(&self.0), "{prefix}{line}");
    }
}
