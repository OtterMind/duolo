//! 任务的事件流和远程命令（手机端的基础，见 docs/architecture.md「手机端」）。
//!
//! 事件来自三处：Agent 自己写的会话记录（对话和工具调用）、终端画面（状态、等你选的提示）、
//! 引擎自己（任务建好、改名、审阅、改动统计）。界面定期调用 [`Engine::poll_events`] 取新事件。
//! 别的设备能对这台电脑做的事（`Command`）统一由 [`crate::ops::execute`] 执行。

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Result;
use duolo_core::{AgentKey, AgentRole, EventKind, Speaker, TaskEvent, TaskId, UserError};

use crate::engine::Engine;
use crate::events::EventLog;
use crate::prompt::{is_choosing, parse_prompt, Prompt};
use crate::status::AgentStatus;
use crate::transcript::{Entry, Flavor, Watcher};

/// 状态要稳定这么久才记成事件。
const STATUS_SETTLE: Duration = Duration::from_millis(1500);

/// 已经记下来、Agent 正在等的提示。
#[derive(Debug)]
struct OpenPrompt {
    /// 提示的内容，变了就是另一条提示。
    id: String,
    /// 记成事件时的编号；没记成（写文件失败）时为空。
    seq: Option<u64>,
    options: usize,
}

/// 事件流用到的状态，挂在 [`Engine`] 上。
#[derive(Debug)]
pub(crate) struct LiveState {
    pub(crate) log: EventLog,
    /// 上次 poll 之后新记的事件。
    fresh: Vec<TaskEvent>,
    watchers: HashMap<AgentKey, Watcher>,
    /// 上次记下的状态和提示，变了才记新事件。
    status: HashMap<AgentKey, String>,
    /// 刚变成的状态和变的时间：稳定一会儿才记，避免界面重绘时"运行中 / 空闲"来回跳。
    settling: HashMap<AgentKey, (String, Instant)>,
    prompt: HashMap<AgentKey, OpenPrompt>,
    diff: HashMap<TaskId, duolo_git::DiffStat>,
    /// Agent 还没准备好时收到的消息，准备好后再发。
    pending: HashMap<AgentKey, Vec<String>>,
    /// Agent 会话记录所在的家目录。
    home: PathBuf,
}

impl LiveState {
    pub(crate) fn new(events_dir: PathBuf) -> Self {
        Self {
            log: EventLog::new(events_dir),
            fresh: Vec::new(),
            watchers: HashMap::new(),
            status: HashMap::new(),
            settling: HashMap::new(),
            prompt: HashMap::new(),
            diff: HashMap::new(),
            pending: HashMap::new(),
            home: duolo_core::process::home_dir(),
        }
    }
}

impl Engine {
    /// 收集新事件并记下来，返回这次新增的（界面每次刷新时调用）。
    pub fn poll_events(&mut self) -> Vec<TaskEvent> {
        self.poll_transcripts();
        self.poll_agent_states();
        self.poll_diffs();
        std::mem::take(&mut self.live.fresh)
    }

    /// 任务第 `after` 条之后的事件（断线重连时补齐用）。
    pub fn events_since(&self, task: TaskId, after: u64) -> Result<Vec<TaskEvent>> {
        self.live.log.since(task, after)
    }

    /// 记一条事件，返回它的编号。写文件失败时只丢这一条，不影响正在做的事。
    pub(crate) fn record(&mut self, task: TaskId, kind: EventKind) -> Option<u64> {
        let event = self.live.log.append(task, kind).ok()?;
        let seq = event.seq;
        self.live.fresh.push(event);
        Some(seq)
    }

    /// Agent 启动后开始跟着它的会话记录。
    pub(crate) fn watch_transcript(&mut self, key: AgentKey) {
        let Some(task) = self.state.task(key.task) else { return };
        let Some(flavor) = task.agent(key.agent).and_then(|a| Flavor::of(&a.kind.0)) else { return };
        let watcher = Watcher::new(flavor, &task.worktree, std::time::SystemTime::now(), self.live.home.clone());
        self.live.watchers.insert(key, watcher);
    }

    /// 读 Agent 会话记录的时候用哪个家目录（测试里换成临时目录）。
    pub fn set_agent_home(&mut self, home: PathBuf) {
        self.live.home = home;
    }

    fn poll_transcripts(&mut self) {
        let mut found = Vec::new();
        for (key, watcher) in &mut self.live.watchers {
            for entry in watcher.poll() {
                found.push((*key, entry));
            }
        }
        for (key, entry) in found {
            let kind = match entry {
                Entry::User(text) => EventKind::Message { speaker: Speaker::User, agent: key.agent, text },
                Entry::Agent(text) => EventKind::Message { speaker: Speaker::Agent, agent: key.agent, text },
                Entry::Tool { tool, target } => EventKind::Tool { agent: key.agent, tool, target },
            };
            self.record(key.task, kind);
        }
    }

    /// 状态变化、等你选的提示，以及 Agent 准备好之后补发排队的消息。
    fn poll_agent_states(&mut self) {
        let keys: Vec<AgentKey> = self.sessions.keys().copied().collect();
        for key in keys {
            let status = self.agent_status(key);
            let code = status.code();
            if self.live.status.get(&key).map(String::as_str) == Some(code) {
                self.live.settling.remove(&key);
            } else {
                let since = match self.live.settling.get(&key) {
                    Some((pending, since)) if pending == code => *since,
                    _ => {
                        self.live.settling.insert(key, (code.to_string(), Instant::now()));
                        Instant::now()
                    }
                };
                // 等你确认、退出这些马上记；运行中 / 空闲要稳定一会儿。
                let urgent = matches!(status, AgentStatus::NeedsInput | AgentStatus::Exited(_));
                if urgent || since.elapsed() >= STATUS_SETTLE {
                    self.live.settling.remove(&key);
                    self.live.status.insert(key, code.to_string());
                    self.record(key.task, EventKind::Status { agent: key.agent, status: code.into() });
                }
            }
            if status == AgentStatus::NeedsInput {
                if let Some((id, prompt)) = self.prompt_on_screen(key) {
                    if self.live.prompt.get(&key).map(|p| &p.id) != Some(&id) {
                        let options = prompt.options.len();
                        let kind = EventKind::Prompt {
                            agent: key.agent,
                            question: prompt.question,
                            detail: prompt.detail,
                            options: prompt.options,
                        };
                        let seq = self.record(key.task, kind);
                        self.live.prompt.insert(key, OpenPrompt { id, seq, options });
                    }
                }
            } else {
                self.live.prompt.remove(&key);
            }
            // 刚启动的 Agent 准备好输入框之后，把排队的消息发过去。
            if status == AgentStatus::Idle && self.live.pending.contains_key(&key) && self.can_take_message(key) {
                if let Some(queued) = self.live.pending.remove(&key) {
                    for text in queued {
                        let _ = self.paste(key, &text, true);
                    }
                }
            }
        }
    }

    /// 画面上现在等着用户选的提示，和区分它的标识。
    fn prompt_on_screen(&self, key: AgentKey) -> Option<(String, Prompt)> {
        let prompt = self.sessions.get(&key)?.with_screen(|screen| parse_prompt(&screen.contents()))?;
        let id = format!("{}\n{}\n{}", prompt.question, prompt.detail, prompt.options.join("\n"));
        Some((id, prompt))
    }

    fn poll_diffs(&mut self) {
        let tasks: Vec<TaskId> = self.sessions.keys().map(|k| k.task).collect();
        for task in tasks {
            let Some(stat) = self.diff_stat(task) else { continue };
            if self.live.diff.get(&task) != Some(&stat) {
                self.live.diff.insert(task, stat);
                let kind =
                    EventKind::Diff { files: stat.files, insertions: stat.insertions, deletions: stat.deletions };
                self.record(task, kind);
            }
        }
    }

    // ---- 发消息 ----

    /// 画面上已经有准备好的输入框，而且没有在等用户选什么（第一次启动时选主题、确认信任目录……）。
    pub(crate) fn can_take_message(&self, key: AgentKey) -> bool {
        self.sessions
            .get(&key)
            .is_some_and(|s| s.is_ready_for_input() && !s.with_screen(|screen| is_choosing(&screen.contents())))
    }

    /// 给任务写代码的 Agent 发一句话并发送。Agent 没在运行时接着上次的会话启动，准备好后再发。
    /// 空白任务的第一句话同时成为它的需求和标题。
    pub fn send_message(&mut self, task: TaskId, text: &str) -> Result<()> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(());
        }
        let key = self.writer_key(task)?;
        let t = self.state.task(task).ok_or(UserError::TaskNotFound)?;
        // 还没启动过的空白任务：这句话成为它的需求，随启动交给 Agent，不用再发一遍。
        let becomes_prompt = t.prompt.is_empty() && t.agent(key.agent).is_some_and(|a| !a.launched);
        self.name_task(task, text, None)?;
        let running = self.sessions.get_mut(&key).is_some_and(|s| s.poll_exit().is_none());
        if running {
            self.paste(key, text, true)
        } else {
            self.start_agent(key)?;
            if !becomes_prompt {
                self.queue_message(key, text.to_string());
            }
            Ok(())
        }
    }

    /// 排队的消息里去掉和 `text` 一样的。
    pub(crate) fn drop_queued(&mut self, key: AgentKey, text: &str) {
        if let Some(queue) = self.live.pending.get_mut(&key) {
            queue.retain(|queued| queued != text);
            if queue.is_empty() {
                self.live.pending.remove(&key);
            }
        }
    }

    /// 刚启动的 Agent 准备好之后再发这句话（并回车）。
    pub(crate) fn queue_message(&mut self, key: AgentKey, text: String) {
        self.live.pending.entry(key).or_default().push(text);
    }

    /// 回答 Agent 正在等的选择（`option` 从 0 开始），等于在终端里按对应的数字键。
    /// `prompt` 有值时只回答那一条提示，Agent 已经不在等它时报 `promptChanged`。
    pub fn answer(&mut self, task: TaskId, option: usize, prompt: Option<u64>) -> Result<()> {
        let key = self.writer_key(task)?;
        if let Some(seq) = prompt {
            // 画面可能刚变，上次轮询记下的还没更新：和现在画面上的比。
            let waiting = self.agent_status(key) == AgentStatus::NeedsInput;
            let on_screen = self.prompt_on_screen(key).map(|(id, _)| id);
            let matches = self.live.prompt.get(&key).is_some_and(|p| {
                p.seq == Some(seq) && option < p.options && on_screen.as_deref() == Some(p.id.as_str())
            });
            if !waiting || !matches {
                return Err(UserError::PromptChanged.into());
            }
        }
        self.write(key, (option + 1).to_string().as_bytes())
    }

    /// 打断 Agent 正在做的事（终端里按 Esc）。
    pub fn interrupt(&mut self, task: TaskId) -> Result<()> {
        let key = self.writer_key(task)?;
        self.write(key, b"\x1b")
    }

    pub(crate) fn writer_key(&self, task: TaskId) -> Result<AgentKey> {
        let t = self.state.task(task).ok_or(UserError::TaskNotFound)?;
        let agent = t.agent_with_role(AgentRole::Writer).ok_or(UserError::AgentNotRunning)?;
        Ok(t.key(agent.id))
    }

    /// 任务关闭后不再跟着它的 Agent。
    pub(crate) fn forget_task(&mut self, task: TaskId) {
        self.live.watchers.retain(|k, _| k.task != task);
        self.live.status.retain(|k, _| k.task != task);
        self.live.settling.retain(|k, _| k.task != task);
        self.live.prompt.retain(|k, _| k.task != task);
        self.live.pending.retain(|k, _| k.task != task);
        self.live.diff.remove(&task);
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use std::sync::Mutex;

    use duolo_core::{Command, CommandResult, Paths, ProjectId};

    use super::*;

    fn git(dir: &std::path::Path, args: &[&str]) {
        let out = std::process::Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    }

    /// 一个仓库，写代码的 Agent 是 `script`（sh 脚本）。
    fn engine(script: &str) -> (tempfile::TempDir, Engine, ProjectId) {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("shop");
        std::fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "init"]);
        let home = dir.path().join("home");
        std::fs::create_dir(&home).unwrap();
        let config = format!("[agents.claude]\ncommand = \"sh\"\nargs = [\"-c\", {script:?}, \"agent\"]\n");
        std::fs::write(home.join("config.toml"), config).unwrap();
        let mut engine = Engine::open(Paths::at(&home)).unwrap();
        engine.set_agent_home(dir.path().join("user-home"));
        let project = engine.add_project(&repo).unwrap();
        (dir, engine, project)
    }

    fn wait_for(engine: &mut Engine, what: &str, mut done: impl FnMut(&[TaskEvent]) -> bool) -> Vec<TaskEvent> {
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut all = Vec::new();
        loop {
            all.extend(engine.poll_events());
            if done(&all) {
                return all;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}: {all:#?}");
            thread::sleep(Duration::from_millis(100));
        }
    }

    /// 通过 `ops` 执行命令，和桌面应用一样。
    fn execute(engine: &Mutex<Engine>, command: Command) -> CommandResult {
        crate::ops::execute(engine, command)
    }

    fn screen(engine: &Engine, key: AgentKey) -> String {
        engine.session(key).map(|s| s.with_screen(|s| s.contents())).unwrap_or_default()
    }

    /// 轮询到画面上出现 `text`。
    fn wait_screen(engine: &mut Engine, key: AgentKey, text: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !screen(engine, key).contains(text) {
            engine.poll_events();
            assert!(Instant::now() < deadline, "no {text:?} on screen: {}", screen(engine, key));
            thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn prompt_shows_up_and_can_be_answered() {
        // 假 Agent：显示一个确认框，读到 1 就打印 picked-1。只用 POSIX sh 的写法（Debian 的 sh 是 dash）。
        let script = "printf 'Bash command\\n\\n  pnpm test\\n\\nDo you want to proceed?\\n> 1. Yes\\n  2. No\\n'; \
                      stty -icanon; c=$(dd bs=1 count=1 2>/dev/null); echo; echo picked-$c; sleep 5";
        let (_dir, engine, project) = engine(script);
        let engine = Mutex::new(engine);
        let result = execute(
            &engine,
            Command::CreateTask {
                project: project.0.clone(),
                prompt: "Run the tests".into(),
                repo: None,
                base: None,
                branch: None,
                writer: None,
                reviewer: None,
            },
        );
        assert!(result.ok, "{result:?}");
        let task = result.task.unwrap();

        let events = wait_for(&mut engine.lock().unwrap(), "prompt", |all| {
            all.iter().any(|e| matches!(e.kind, EventKind::Prompt { .. }))
        });
        assert!(matches!(events[0].kind, EventKind::TaskCreated { .. }));
        let prompt = events.iter().find_map(|e| match &e.kind {
            EventKind::Prompt { question, detail, options, .. } => {
                Some((question.clone(), detail.clone(), options.clone()))
            }
            _ => None,
        });
        assert_eq!(
            prompt,
            Some(("Do you want to proceed?".into(), "pnpm test".into(), vec!["Yes".into(), "No".into()]))
        );
        assert!(events.windows(2).all(|w| w[1].seq == w[0].seq + 1), "编号连续");

        // 回答的不是现在这条提示，或者没有这个选项：不执行。
        let seq = events.iter().rev().find(|e| matches!(e.kind, EventKind::Prompt { .. })).unwrap().seq;
        for (option, prompt) in [(0, seq + 100), (7, seq)] {
            let late = execute(&engine, Command::Answer { task, option, prompt: Some(prompt) });
            assert_eq!(late.error.as_deref(), Some("promptChanged"));
        }
        assert!(execute(&engine, Command::Answer { task, option: 0, prompt: Some(seq) }).ok);
        let mut guard = engine.lock().unwrap();
        let key = guard.writer_key(task).unwrap();
        wait_screen(&mut guard, key, "picked-1");
        // 补齐：从头要能拿到同样的事件。
        let replay = guard.events_since(task, 0).unwrap();
        assert_eq!(replay[0].seq, 1);
        assert!(replay.len() >= events.len());
        drop(guard);

        assert!(execute(&engine, Command::CloseTask { task }).ok);
        assert!(engine.lock().unwrap().state().task(task).is_none());
    }

    #[test]
    fn errors_come_back_as_codes() {
        let (_dir, engine, _) = engine("cat");
        let result = execute(&Mutex::new(engine), Command::SendMessage { task: TaskId(99), text: "hi".into() });
        assert_eq!(result, CommandResult { ok: false, task: None, error: Some("taskNotFound".into()) });
    }

    /// 假 Agent：和 Claude Code 一样，先空白一阵（此时的输入会丢），画出输入框后才打开括号粘贴、开始读。
    /// 打印启动时带的需求（`arg:…`）和收到的每一行（`got:…`）。
    const SLOW_START: &str = "sleep 2.2; stty -echo; perl -e 'use POSIX; tcflush(0, TCIFLUSH)'; \
                              printf 'arg:%s\\n\\033[?2004h> ' \"$1\"; while read -r line; do echo \"got:$line\"; done";

    #[test]
    fn message_waits_until_the_agent_is_ready() {
        let (_dir, mut engine, project) = engine(SLOW_START);
        let task = engine.create_task(&project, "Export orders as CSV").unwrap();
        let key = engine.writer_key(task).unwrap();
        engine.start_agent(key).unwrap();
        engine.stop_agent(key);
        // 已经启动过：这句话要等它恢复、准备好之后再发。
        engine.send_message(task, "and Excel").unwrap();
        wait_screen(&mut engine, key, "and Excel");
        thread::sleep(Duration::from_millis(300));
        assert_eq!(screen(&engine, key).matches("and Excel").count(), 1, "{}", screen(&engine, key));
    }

    #[test]
    fn first_message_of_a_blank_task_is_sent_once() {
        let (_dir, mut engine, project) = engine(SLOW_START);
        let task = engine.create_task(&project, "").unwrap();
        engine.send_message(task, "Export orders as CSV").unwrap();
        assert_eq!(engine.state().task(task).unwrap().title, "Export orders as CSV", "空白任务的第一句话成为标题");
        let key = engine.writer_key(task).unwrap();
        wait_screen(&mut engine, key, "arg:Export orders as CSV");
        let until = Instant::now() + Duration::from_secs(3);
        while Instant::now() < until {
            engine.poll_events();
            thread::sleep(Duration::from_millis(100));
        }
        assert!(!screen(&engine, key).contains("got:"), "随启动带过去了，不能再发一遍：{}", screen(&engine, key));
    }

    /// Windows 上 npm 装的 Claude 是批处理文件，带不了换行：需求等它准备好之后粘贴过去。
    #[cfg(windows)]
    #[test]
    fn batch_file_agent_gets_a_multi_line_prompt() {
        let (dir, mut engine, project) = engine("cat");
        let agent = dir.path().join("agent.sh");
        std::fs::write(&agent, "sleep 1; echo \"args:$#\"; while read -r line; do echo \"got:$line\"; done\n").unwrap();
        let script = dir.path().join("claude.cmd");
        std::fs::write(&script, "@sh \"%~dp0agent.sh\" %*\r\n").unwrap();
        let config = format!("[agents.claude]\ncommand = {:?}\n", script.display().to_string());
        std::fs::write(dir.path().join("home/config.toml"), config).unwrap();
        drop(engine);
        engine = Engine::open(Paths::at(dir.path().join("home"))).unwrap();
        let task = engine.create_task(&project, "Export orders\nas CSV & \"Excel\" 100%").unwrap();
        let key = engine.writer_key(task).unwrap();
        engine.start_agent(key).unwrap();
        wait_screen(&mut engine, key, "args:0");
        wait_screen(&mut engine, key, "got:Export orders");
        wait_screen(&mut engine, key, "got:as CSV & \"Excel\" 100%");
        engine.close_task(task).unwrap();
    }

    #[test]
    fn message_does_not_answer_a_startup_dialog() {
        // 假 Agent：第一次启动先让用户选主题，选完才出现输入框。
        let script = "printf '\\033[?2004hChoose a theme\\n❯ 1. Dark\\n  2. Light\\n'; read -r pick; \
                      printf '\\033[2J\\033[Hpicked:%s\\n> ' \"$pick\"; while read -r line; do echo \"got:$line\"; done";
        let (_dir, mut engine, project) = engine(script);
        let task = engine.create_task(&project, "Export orders as CSV").unwrap();
        let key = engine.writer_key(task).unwrap();
        engine.start_agent(key).unwrap();
        engine.queue_message(key, "hello".into());
        let until = Instant::now() + Duration::from_secs(3);
        while Instant::now() < until {
            engine.poll_events();
            thread::sleep(Duration::from_millis(100));
        }
        assert!(!screen(&engine, key).contains("picked"), "不能替用户选：{}", screen(&engine, key));
        engine.write(key, b"1\r").unwrap();
        wait_screen(&mut engine, key, "got:");
        assert!(screen(&engine, key).contains("picked:1"));
    }
}
