//! 聊天模式：Agent 不在终端里运行，Duolo 通过它的结构化协议对话，界面自己画。
//!
//! 现在用的是 ACP（Agent Client Protocol）：JSON-RPC 走标准输入输出，OpenCode、Grok、Antigravity 等都支持。
//! 以后 Codex 的 app-server、Claude 的 stream-json 也放在这里，对外都是同一个 [`Chat`]。
//!
//! 线程：写线程（往标准输入写，Agent 不读也不会卡住调用方）、读线程（回复交给等着的人，更新拼成对话）、
//! 读错误输出的线程（留最后几行，启动失败时给用户看），以及按顺序走流程的线程（握手、开会话、一句句发）。
//! 调用方只取结果，不等 Agent。

mod acp;
mod rpc;
pub mod testing;

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use duolo_core::process;
use duolo_core::proctree::ProcessTree;
use serde_json::{json, Value};

use acp::Conversation;
use rpc::{lock, wait, Failure, Incoming, Rpc};

/// 握手、开会话最长等多久（第一次启动可能要装插件、连模型服务）。
const SETUP_TIMEOUT: Duration = Duration::from_secs(120);
/// 结束时先关掉标准输入，等它自己退出多久。
const GRACE: Duration = Duration::from_secs(1);
/// 强制结束后最多等进程消失多久。
const STOP_TIMEOUT: Duration = Duration::from_secs(2);
/// 错误输出留最后几行。
const STDERR_LINES: usize = 20;

/// 怎么启动。
#[derive(Debug, Clone, Default)]
pub struct Spec {
    pub program: String,
    pub args: Vec<String>,
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
    shared: Arc<Shared>,
    rpc: Rpc,
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
    request: Value,
    option_ids: Vec<String>,
}

impl Chat {
    /// 启动。马上返回，握手、开会话在后台做。
    pub fn spawn(spec: Spec) -> Result<Self> {
        let mut cmd = process::command(&spec.program);
        cmd.args(&spec.args)
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
        let rpc = Rpc::new(out);
        let shared = Arc::new(Shared::default());
        lock(&shared.inner).conversation = Conversation::new(&spec.cwd);

        thread::Builder::new().name("duolo-chat-writer".into()).spawn(move || {
            let mut stdin = stdin;
            for line in lines {
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
        let (reader_shared, reader_rpc) = (shared.clone(), rpc.clone());
        thread::Builder::new()
            .name("duolo-chat-reader".into())
            .spawn(move || read(stdout, &reader_shared, &reader_rpc))?;
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
        let (driver_shared, driver_rpc) = (shared.clone(), rpc.clone());
        thread::Builder::new()
            .name("duolo-chat-driver".into())
            .spawn(move || drive(&driver_shared, &driver_rpc, &spec))?;

        Ok(Self { shared, rpc, child: Some(child), tree, exit: None, started: Instant::now() })
    }

    /// 发一句话。正在干活时排在后面，这一轮结束再发。
    pub fn send(&self, text: &str) {
        lock(&self.shared.inner).queue.push_back(text.to_string());
        self.shared.wake.notify_all();
    }

    /// 打断：停下这一轮，排着的话也不发了，在问的问题当作取消。
    pub fn cancel(&self) {
        let mut inner = lock(&self.shared.inner);
        inner.queue.clear();
        if let Some(pending) = inner.ask.take() {
            self.rpc.respond(pending.request, json!({"outcome": {"outcome": "cancelled"}}));
        }
        if let (true, Some(session)) = (inner.working, inner.session.clone()) {
            self.rpc.notify("session/cancel", json!({"sessionId": session}));
        }
    }

    /// 回答问题 `ask`（选第 `option` 个，从 0 开始）。已经不在问这个了返回 false。
    pub fn answer(&self, ask: u64, option: usize) -> bool {
        let mut inner = lock(&self.shared.inner);
        let fits = inner.ask.as_ref().is_some_and(|p| p.ask.id == ask && option < p.option_ids.len());
        if !fits {
            return false;
        }
        let pending = inner.ask.take().expect("checked above");
        let id = pending.option_ids[option].clone();
        self.rpc.respond(pending.request, json!({"outcome": {"outcome": "selected", "optionId": id}}));
        true
    }

    /// 在等用户回答的问题。
    pub fn ask(&self) -> Option<Ask> {
        lock(&self.shared.inner).ask.as_ref().map(|p| p.ask.clone())
    }

    /// 上次取过之后发生的事，取一次就清空。
    pub fn take_updates(&self) -> Vec<Update> {
        std::mem::take(&mut lock(&self.shared.inner).updates)
    }

    pub fn draft(&self) -> Draft {
        lock(&self.shared.inner).conversation.draft()
    }

    /// 出错的原因（Agent 自己说的，不翻译）：启动失败、一轮对话失败，或者退出前最后的错误输出。
    pub fn error(&mut self) -> Option<String> {
        let failed = matches!(self.status(), Status::Exited { success: false, .. });
        let inner = lock(&self.shared.inner);
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
        let inner = lock(&self.shared.inner);
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
        self.rpc.close();
        let Some(mut child) = self.child.take() else { return };
        stop_process(&mut child, &self.tree, self.exit.is_some());
        if let Ok(Some(status)) = child.try_wait() {
            let code = status.code().map_or(1, |c| u32::try_from(c).unwrap_or(1));
            self.exit.get_or_insert((code, status.success()));
        }
    }

    fn stop_in_background(&mut self) {
        self.rpc.close();
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

/// 读线程：回复交给等着的人，更新拼成对话，问题记下来等用户回答。
fn read(stdout: impl std::io::Read, shared: &Shared, rpc: &Rpc) {
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        match rpc.dispatch(&line) {
            Some(Incoming::Notification { method, params }) if method == "session/update" => {
                let mut inner = lock(&shared.inner);
                inner.conversation.update(&params["update"]);
                collect(&mut inner);
            }
            Some(Incoming::Request { id, method, params }) if method == "session/request_permission" => {
                let mut inner = lock(&shared.inner);
                let (tool, target) = inner.conversation.permission(&params["toolCall"]);
                collect(&mut inner);
                let options = params["options"].as_array().cloned().unwrap_or_default();
                let names = options.iter().map(|o| o["name"].as_str().unwrap_or_default().to_string()).collect();
                let option_ids =
                    options.iter().map(|o| o["optionId"].as_str().unwrap_or_default().to_string()).collect();
                inner.next_ask += 1;
                let ask = Ask { id: inner.next_ask, tool, target, options: names };
                if let Some(old) = inner.ask.replace(PendingAsk { ask, request: id, option_ids }) {
                    rpc.respond(old.request, json!({"outcome": {"outcome": "cancelled"}}));
                }
            }
            // 没说过支持的（读写文件、开终端）：告诉它不支持，它会自己做。
            Some(Incoming::Request { id, .. }) => rpc.respond_error(id, -32601, "method not supported"),
            Some(Incoming::Notification { .. }) | None => {}
        }
    }
    let mut inner = lock(&shared.inner);
    inner.closed = true;
    inner.ask = None;
    drop(inner);
    rpc.close();
    shared.wake.notify_all();
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
fn drive(shared: &Shared, rpc: &Rpc, spec: &Spec) {
    let Some(session) = open_session(shared, rpc, spec) else { return };
    {
        let mut inner = lock(&shared.inner);
        inner.session = Some(session.clone());
        inner.updates.push(Update::Session(session.clone()));
        inner.ready = true;
    }
    loop {
        let text = {
            let mut inner = lock(&shared.inner);
            loop {
                if inner.closed {
                    return;
                }
                if let Some(text) = inner.queue.pop_front() {
                    break text;
                }
                inner = shared.wake.wait(inner).unwrap_or_else(|p| p.into_inner());
            }
        };
        {
            let mut inner = lock(&shared.inner);
            inner.working = true;
            inner.error = None;
            inner.updates.push(Update::Entry(Entry::User(text.clone())));
        }
        let prompt = json!({"sessionId": session, "prompt": [{"type": "text", "text": text}]});
        let reply = wait(&rpc.request("session/prompt", prompt), None);
        let mut inner = lock(&shared.inner);
        inner.conversation.finish_turn();
        collect(&mut inner);
        inner.working = false;
        if let Err(Failure::Rejected(err)) = reply {
            inner.error = Some(err.message);
        }
    }
}

/// 握手，接回原来的会话（接不回来就新开，新开的先发第一句话），返回会话编号。失败时记下原因，返回 None。
fn open_session(shared: &Shared, rpc: &Rpc, spec: &Spec) -> Option<String> {
    let fail = |failure: Failure| -> Option<String> {
        let mut inner = lock(&shared.inner);
        if !inner.closed {
            inner.failed = Some(match failure {
                Failure::Rejected(err) => err.message,
                Failure::TimedOut => "timed out".into(),
                Failure::Closed => String::new(),
            });
        }
        None
    };
    let init = json!({
        "protocolVersion": 1,
        "clientCapabilities": {"fs": {"readTextFile": false, "writeTextFile": false}, "terminal": false},
        "clientInfo": {"name": "duolo", "version": env!("CARGO_PKG_VERSION")},
    });
    let caps = match wait(&rpc.request("initialize", init), Some(SETUP_TIMEOUT)) {
        Ok(caps) => caps["agentCapabilities"].clone(),
        Err(failure) => return fail(failure),
    };
    let cwd = spec.cwd.display().to_string();
    let earlier = match &spec.session {
        Some(id) => Some(id.clone()),
        None if spec.resume_latest && caps["sessionCapabilities"].get("list").is_some() => {
            latest_session(rpc, &spec.cwd)
        }
        None => None,
    };
    if let Some(id) = earlier.filter(|_| caps["loadSession"].as_bool() == Some(true)) {
        {
            let mut inner = lock(&shared.inner);
            inner.history = Some(Vec::new());
            inner.conversation.replaying = true;
        }
        let load = json!({"sessionId": id, "cwd": cwd, "mcpServers": []});
        let loaded = wait(&rpc.request("session/load", load), Some(SETUP_TIMEOUT));
        let mut inner = lock(&shared.inner);
        inner.conversation.finish_turn();
        collect(&mut inner);
        inner.conversation.replaying = false;
        let history = inner.history.take().unwrap_or_default();
        if loaded.is_ok() {
            inner.updates.push(Update::History(history));
            return Some(id);
        }
    }
    let created = wait(&rpc.request("session/new", json!({"cwd": cwd, "mcpServers": []})), Some(SETUP_TIMEOUT));
    let id = match created.map(|r| r["sessionId"].as_str().map(str::to_string)) {
        Ok(Some(id)) => id,
        Ok(None) => return fail(Failure::Rejected(rpc::RpcError { code: 0, message: "no sessionId".into() })),
        Err(failure) => return fail(failure),
    };
    if let Some(prompt) = spec.prompt.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
        lock(&shared.inner).queue.push_front(prompt.to_string());
    }
    Some(id)
}

/// 这个目录里最近的一次会话。
fn latest_session(rpc: &Rpc, cwd: &Path) -> Option<String> {
    let list =
        wait(&rpc.request("session/list", json!({"cwd": cwd.display().to_string()})), Some(SETUP_TIMEOUT)).ok()?;
    let mut sessions: Vec<&Value> = list["sessions"].as_array()?.iter().collect();
    sessions.sort_by(|a, b| b["updatedAt"].as_str().cmp(&a["updatedAt"].as_str()));
    sessions.first()?["sessionId"].as_str().map(str::to_string)
}
