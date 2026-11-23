//! 任务的事件流和远程命令（手机端的基础，见 docs/architecture.md「手机端」）。
//!
//! 事件来自三处：Agent 自己写的会话记录（对话和工具调用）、终端画面（状态、等你选的提示）、
//! 引擎自己（任务建好、改名、审阅、改动统计）。界面定期调用 [`Engine::poll_events`] 取新事件。
//! 别的设备能对这台电脑做的事（`Command`）统一由 [`crate::ops::execute`] 执行。

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Result;
use duolo_agent::{Entry, Home, Sessions, Watch};
use duolo_core::{AgentKey, AgentRole, EventKind, Speaker, State, TaskEvent, TaskId, UserError};

use crate::engine::Engine;
use crate::events::EventLog;
use crate::prompt::{is_choosing, parse_prompt};
use crate::status::AgentStatus;

/// 状态要稳定这么久才记成事件。
const STATUS_SETTLE: Duration = Duration::from_millis(1500);

/// 事件流用到的状态，挂在 [`Engine`] 上。
#[derive(Debug)]
pub(crate) struct LiveState {
    pub(crate) log: EventLog,
    /// 上次 poll 之后新记的事件。
    fresh: Vec<TaskEvent>,
    watchers: HashMap<AgentKey, Watch>,
    /// 上次记下的状态和提示，变了才记新事件。
    status: HashMap<AgentKey, String>,
    /// 刚变成的状态和变的时间：稳定一会儿才记，避免界面重绘时"运行中 / 空闲"来回跳。
    settling: HashMap<AgentKey, (String, Instant)>,
    prompt: HashMap<AgentKey, String>,
    diff: HashMap<TaskId, duolo_git::DiffStat>,
    /// Agent 还没准备好时收到的消息，准备好后再发。
    pending: HashMap<AgentKey, Vec<String>>,
    /// 这次启动之后已经画出过输入框的 Agent。
    settled: HashSet<AgentKey>,
    /// 去哪里找 Agent 的会话记录。
    home: Home,
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
            settled: HashSet::new(),
            home: Home::User,
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

    /// 记一条事件。写文件失败时只丢这一条，不影响正在做的事。
    pub(crate) fn record(&mut self, task: TaskId, kind: EventKind) {
        if let Ok(event) = self.live.log.append(task, kind) {
            self.live.fresh.push(event);
        }
    }

    /// Agent 启动后开始跟着它的会话记录。`resumed` 时接着读它原来的那次会话。
    pub(crate) fn watch_transcript(&mut self, key: AgentKey, resumed: bool) {
        let Some(task) = self.state.task(key.task) else { return };
        let own = task.agent(key.agent).and_then(|a| a.session.clone()).filter(|_| resumed);
        let others =
            task.agents.iter().chain(&task.retired).filter(|a| a.id != key.agent).filter_map(|a| a.session.clone());
        let sessions = Sessions { own, others: others.collect() };
        match self.plugin_of(key).ok().and_then(|plugin| plugin.watch(&task.worktree, &self.live.home, sessions)) {
            Some(watch) => self.live.watchers.insert(key, watch),
            None => self.live.watchers.remove(&key),
        };
    }

    /// 读 Agent 会话记录的时候用哪个家目录（测试里换成临时目录）。
    pub fn set_agent_home(&mut self, home: PathBuf) {
        self.live.home = Home::At(home);
    }

    fn poll_transcripts(&mut self) {
        let mut found = Vec::new();
        let mut sessions = Vec::new();
        for (key, watch) in &self.live.watchers {
            found.extend(watch.take().into_iter().map(|entry| (*key, entry)));
            if let Some(id) = watch.session() {
                sessions.push((*key, id));
            }
        }
        self.remember_sessions(sessions);
        for (key, entry) in found {
            let kind = match entry {
                Entry::User(text) => EventKind::Message { speaker: Speaker::User, agent: key.agent, text },
                Entry::Agent(text) => EventKind::Message { speaker: Speaker::Agent, agent: key.agent, text },
                Entry::Tool { tool, target } => EventKind::Tool { agent: key.agent, tool, target },
            };
            self.record(key.task, kind);
        }
    }

    /// 记下 Agent 的会话编号，下次按编号恢复。同一个任务里别的 Agent 已经认领的编号不抢，
    /// 并告诉同一个任务里的其他 Agent：这个会话不是它们的。
    fn remember_sessions(&mut self, mut found: Vec<(AgentKey, String)>) {
        let claimed = |state: &State, key: AgentKey, id: &String| {
            state.task(key.task).is_some_and(|t| {
                t.agents.iter().chain(&t.retired).any(|a| a.id != key.agent && a.session.as_ref() == Some(id))
            })
        };
        let known = |state: &State, key: AgentKey| state.task(key.task).and_then(|t| t.agent(key.agent)).cloned();
        // 已经有会话的 Agent 又认出一个新的：同一个任务里还有正在运行、还没认出自己会话的 Agent 时先不认，
        // 那多半是它的。
        let unsure = |state: &State, key: AgentKey| {
            state.task(key.task).is_some_and(|t| {
                t.agents
                    .iter()
                    .any(|a| a.id != key.agent && a.session.is_none() && self.sessions.contains_key(&t.key(a.id)))
            })
        };
        found.retain(|(key, id)| {
            let Some(agent) = known(&self.state, *key) else { return false };
            agent.session.as_ref() != Some(id)
                && !claimed(&self.state, *key, id)
                && (agent.session.is_none() || !unsure(&self.state, *key))
        });
        // 两个 Agent 同时认出同一个会话：给还没有会话的那个（另一个是看到了别人新建的文件）。
        found.sort_by_key(|(key, _)| known(&self.state, *key).is_some_and(|a| a.session.is_some()));
        let mut taken = HashSet::new();
        found.retain(|(key, id)| taken.insert((key.task, id.clone())));
        if found.is_empty() {
            return;
        }
        // 没存上只是下次恢复最近的一次会话。
        let _ = self.update(|state| {
            for (key, id) in &found {
                if let Some(agent) = state.task_mut(key.task).and_then(|t| t.agent_mut(key.agent)) {
                    agent.session = Some(id.clone());
                }
            }
            Ok(())
        });
        let tasks: HashSet<TaskId> = found.iter().map(|(key, _)| key.task).collect();
        for (key, watch) in &self.live.watchers {
            let Some(task) = self.state.task(key.task).filter(|t| tasks.contains(&t.id)) else { continue };
            let others = task.agents.iter().chain(&task.retired).filter(|a| a.id != key.agent);
            watch.set_others(others.filter_map(|a| a.session.clone()).collect());
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
                let prompt =
                    self.sessions.get(&key).and_then(|s| s.with_screen(|screen| parse_prompt(&screen.contents())));
                if let Some(prompt) = prompt {
                    let id = format!("{}\n{}\n{}", prompt.question, prompt.detail, prompt.options.join("\n"));
                    if self.live.prompt.get(&key) != Some(&id) {
                        self.live.prompt.insert(key, id);
                        let kind = EventKind::Prompt {
                            agent: key.agent,
                            question: prompt.question,
                            detail: prompt.detail,
                            options: prompt.options,
                        };
                        self.record(key.task, kind);
                    }
                }
            } else {
                self.live.prompt.remove(&key);
            }
            if status == AgentStatus::Idle && self.can_take_message(key) {
                self.live.settled.insert(key);
            }
            // 准备好输入框、没有在等用户选什么之后，把排队的消息发过去。
            if status == AgentStatus::Idle && self.live.pending.contains_key(&key) && self.accepts_message(key) {
                if let Some(queued) = self.live.pending.remove(&key) {
                    for text in queued {
                        let _ = self.paste(key, &text, true);
                    }
                }
            }
        }
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
        let Ok(plugin) = self.plugin_of(key) else { return false };
        let choosing = &plugin.screen().choosing;
        self.sessions.get(&key).is_some_and(|s| {
            s.is_ready_for_input() && !s.with_screen(|screen| is_choosing(&screen.contents(), choosing))
        })
    }

    /// 现在可以发消息。启动时要看仔细（[`Engine::can_take_message`]）；画出过输入框之后，画面上的编号列表
    /// 可能只是用户自己说过的话，只看它是不是在等用户确认。
    fn accepts_message(&mut self, key: AgentKey) -> bool {
        if self.live.settled.contains(&key) {
            return !matches!(self.agent_status(key), AgentStatus::NeedsInput);
        }
        self.can_take_message(key)
    }

    /// 替用户发一句话（粘贴并回车）。Agent 停在要选的列表上（信任目录、是否升级、是否允许）时，
    /// 回车会变成替用户选，所以先排着，等用户选完再发。
    pub fn submit(&mut self, key: AgentKey, text: &str) -> Result<()> {
        if self.session_mut(key).ok_or(UserError::AgentNotRunning)?.poll_exit().is_some() {
            return Err(UserError::AgentNotRunning.into());
        }
        if self.accepts_message(key) {
            return self.paste(key, text, true);
        }
        self.queue_message(key, text.to_string());
        Ok(())
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
            self.submit(key, text)
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

    /// Agent 新启动了：重新看它准备好没有。
    pub(crate) fn unsettle(&mut self, key: AgentKey) {
        self.live.settled.remove(&key);
    }

    /// 刚启动的 Agent 准备好之后再发这句话（并回车）。
    pub(crate) fn queue_message(&mut self, key: AgentKey, text: String) {
        self.live.pending.entry(key).or_default().push(text);
    }

    /// 回答 Agent 正在等的选择（`option` 从 0 开始），等于在终端里按对应的数字键。
    pub fn answer(&mut self, task: TaskId, option: usize) -> Result<()> {
        let key = self.writer_key(task)?;
        self.write(key, (option + 1).to_string().as_bytes())
    }

    /// 打断 Agent 正在做的事。
    pub fn interrupt(&mut self, task: TaskId) -> Result<()> {
        let key = self.writer_key(task)?;
        let interrupt = self.plugin_of(key)?.keys().interrupt.clone();
        self.write(key, interrupt.as_bytes())
    }

    /// 在 Agent 的输入框里换行。
    pub fn newline(&mut self, key: AgentKey) -> Result<()> {
        let newline = self.plugin_of(key)?.keys().newline.clone();
        self.write(key, newline.as_bytes())
    }

    pub(crate) fn writer_key(&self, task: TaskId) -> Result<AgentKey> {
        let t = self.state.task(task).ok_or(UserError::TaskNotFound)?;
        let agent = t.agent_with_role(AgentRole::Writer).ok_or(UserError::AgentNotRunning)?;
        Ok(t.key(agent.id))
    }

    /// 换下来的 Agent：不再跟着它，没发出去的消息交给 `successor`。
    pub(crate) fn hand_over(&mut self, key: AgentKey, successor: AgentKey) {
        self.live.watchers.remove(&key);
        self.live.status.remove(&key);
        self.live.settling.remove(&key);
        self.live.prompt.remove(&key);
        self.live.settled.remove(&key);
        if let Some(queued) = self.live.pending.remove(&key) {
            self.live.pending.entry(successor).or_default().extend(queued);
        }
    }

    /// 任务关闭后不再跟着它的 Agent。
    pub(crate) fn forget_task(&mut self, task: TaskId) {
        self.live.watchers.retain(|k, _| k.task != task);
        self.live.status.retain(|k, _| k.task != task);
        self.live.settling.retain(|k, _| k.task != task);
        self.live.prompt.retain(|k, _| k.task != task);
        self.live.pending.retain(|k, _| k.task != task);
        self.live.settled.retain(|k| k.task != task);
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

        assert!(execute(&engine, Command::Answer { task, option: 0 }).ok);
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
                              for prompt; do :; done; printf 'arg:%s\\n\\033[?2004h> ' \"$prompt\"; while read -r line; do echo \"got:$line\"; done";

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
    fn message_does_not_answer_a_later_dialog() {
        // 假 Agent：第一句话之后弹出确认框（默认选中的是危险的那个），选完才接着读消息。
        let script = "stty -echo; printf '\\033[?2004h> '; read -r line; \
                      printf '\\033[2J\\033[HDo you want to proceed?\\n❯ 1. Yes, delete\\n  2. No\\n'; read -r pick; \
                      printf '\\033[2J\\033[Hpicked:%s\\n> ' \"$pick\"; while read -r line; do echo \"got:$line\"; done";
        let (_dir, mut engine, project) = engine(script);
        let task = engine.create_task(&project, "Export orders as CSV").unwrap();
        let key = engine.writer_key(task).unwrap();
        engine.start_agent(key).unwrap();
        wait_screen(&mut engine, key, ">");
        wait_for(&mut engine, "idle", |all| {
            all.iter().any(|e| matches!(&e.kind, EventKind::Status { status, .. } if status == "idle"))
        });
        // 用户自己说的话里有编号，不该被当成在选。
        engine.submit(key, "1. go").unwrap();
        wait_screen(&mut engine, key, "Yes, delete");
        engine.send_message(task, "second").unwrap();
        engine.submit(key, "third").unwrap();
        let until = Instant::now() + Duration::from_secs(3);
        while Instant::now() < until {
            engine.poll_events();
            thread::sleep(Duration::from_millis(100));
        }
        assert!(screen(&engine, key).contains("Yes, delete"), "不能替用户选：{}", screen(&engine, key));
        engine.write(key, b"2\r").unwrap();
        wait_screen(&mut engine, key, "third");
        let shown = screen(&engine, key);
        assert!(shown.contains("picked:2") && shown.contains("second"), "{shown}");
    }

    #[test]
    fn own_words_do_not_look_like_a_dialog() {
        // 假 Agent：像 Claude Code 一样把用户说的话带着 ❯ 显示出来。
        let script = "stty -echo; printf '\\033[?2004h> '; read -r first; \
                      printf '\\n❯ 1. %s\\n  2. and tests\\n> ' \"$first\"; while read -r line; do echo \"got:$line\"; done";
        let (_dir, mut engine, project) = engine(script);
        let task = engine.create_task(&project, "Export orders as CSV").unwrap();
        let key = engine.writer_key(task).unwrap();
        engine.start_agent(key).unwrap();
        wait_for(&mut engine, "idle", |all| {
            all.iter().any(|e| matches!(&e.kind, EventKind::Status { status, .. } if status == "idle"))
        });
        engine.submit(key, "export").unwrap();
        wait_screen(&mut engine, key, "2. and tests");
        engine.submit(key, "next").unwrap();
        wait_screen(&mut engine, key, "got:");
        assert!(screen(&engine, key).contains("next"), "{}", screen(&engine, key));
    }

    /// 用户装在 `~/.duolo/agents/` 的插件：和内置的一样用，行为都按它的声明来。
    #[cfg(unix)]
    #[test]
    fn user_plugin_works_end_to_end() {
        let (dir, engine, project) = engine("cat");
        drop(engine);
        let plugin = dir.path().join("home/agents/toy");
        std::fs::create_dir_all(&plugin).unwrap();
        let agent = dir.path().join("toy.sh");
        // 打印参数，然后把收到的字节按十六进制打印出来。`review` 时打印结论就退出。
        let script = "if [ \"$1\" = review ]; then echo \"[PASS] $# args\"; exit; fi; \
                      stty raw -echo; printf 'args:%s\\r\\n' \"$*\"; printf '\\033[?2004hPICK ONE\\r\\n'; \
                      exec perl -e '$|=1; while (sysread(STDIN,$c,1)) { printf \"%02x \", ord $c }'\n";
        std::fs::write(&agent, script).unwrap();
        let manifest = format!(
            "schema = 1\nid = \"toy\"\nname = \"Toy\"\ncommand = \"sh\"\n\
             [start]\nargs = [{agent:?}]\nprompt = \"flag\"\nprompt_flag = \"--say=\"\nresume_args = [\"--again\"]\n\
             [review]\nargs = [{agent:?}, \"review\"]\nseparator = \"--\"\n\
             [screen]\napproval = [\"pick one\"]\nbusy = []\nchoosing = []\n\
             [keys]\ninterrupt = \"\\u0003\"\nnewline = \"\\n\"\n",
        );
        std::fs::write(plugin.join("agent.toml"), manifest).unwrap();
        std::fs::create_dir(dir.path().join("home/agents/bad")).unwrap();
        std::fs::write(dir.path().join("home/agents/bad/agent.toml"), "schema = 1\n").unwrap();

        let mut engine = Engine::open(Paths::at(dir.path().join("home"))).unwrap();
        let notices = engine.take_notices();
        assert!(
            matches!(&notices[..], [UserError::AgentPluginInvalid { path, .. }] if path.contains("bad")),
            "{notices:?}"
        );

        let toy = duolo_core::AgentKind("toy".into());
        let opts = crate::TaskOptions { writer: Some(toy.clone()), reviewer: Some(toy), ..Default::default() };
        let task = engine.create_task_with(&project, "- list", &opts).unwrap();
        let key = engine.writer_key(task).unwrap();
        engine.start_agent(key).unwrap();
        wait_screen(&mut engine, key, "args:--say=- list");
        wait_screen(&mut engine, key, "PICK ONE");
        assert_eq!(engine.agent_status(key), AgentStatus::NeedsInput, "按它自己的写法认状态");
        engine.interrupt(task).unwrap();
        wait_screen(&mut engine, key, "03");
        engine.newline(key).unwrap();
        wait_screen(&mut engine, key, "0a");

        engine.stop_agent(key);
        engine.start_agent(key).unwrap();
        wait_screen(&mut engine, key, "args:--again");

        engine.start_review(task).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while engine.poll_reviews().is_empty() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(50));
        }
        let review = engine.state().task(task).unwrap().review.clone().unwrap();
        assert_eq!((review.ok, review.output.as_str()), (true, "[PASS] 3 args"), "review -- <提示词>");
        engine.close_task(task).unwrap();
    }

    /// 读到会话编号后记下来，下次按编号恢复（同一个目录里可能还有别的 Agent 的会话）。
    #[test]
    fn resumes_the_session_it_read() {
        let (dir, mut engine, project) = engine("echo \"args:$*\"; cat");
        let task = engine.create_task(&project, "Export orders").unwrap();
        let key = engine.writer_key(task).unwrap();
        engine.start_agent(key).unwrap();
        wait_screen(&mut engine, key, "args:-- Export orders");

        let worktree = engine.state().task(task).unwrap().worktree.display().to_string();
        let folder: String =
            worktree.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' }).collect();
        let sessions = dir.path().join("user-home/.claude/projects").join(folder);
        std::fs::create_dir_all(&sessions).unwrap();
        let line = r#"{"type":"user","timestamp":"2100-01-01T00:00:00Z","message":{"content":"Export orders"}}"#;
        std::fs::write(sessions.join("5b4c8a2e-0000-4000-8000-000000000001.jsonl"), format!("{line}\n")).unwrap();
        let deadline = Instant::now() + Duration::from_secs(8);
        while engine.state().task(task).unwrap().agents[0].session.is_none() {
            assert!(Instant::now() < deadline, "没有记下会话编号");
            engine.poll_events();
            thread::sleep(Duration::from_millis(50));
        }

        engine.stop_agent(key);
        engine.start_agent(key).unwrap();
        wait_screen(&mut engine, key, "args:--resume 5b4c8a2e-0000-4000-8000-000000000001");
        engine.shutdown();
    }

    /// 同一个 worktree 里两个同种的 Agent（终端里审阅的那个后启动）：各认各的会话。
    #[test]
    fn agents_in_one_folder_keep_their_own_sessions() {
        let (dir, mut engine, project) = engine("cat");
        let claude = duolo_core::AgentKind("claude".into());
        let opts = crate::TaskOptions { writer: Some(claude.clone()), reviewer: Some(claude), ..Default::default() };
        let task = engine.create_task_with(&project, "Export orders", &opts).unwrap();
        let writer = engine.writer_key(task).unwrap();
        engine.start_agent(writer).unwrap();
        let worktree = engine.state().task(task).unwrap().worktree.display().to_string();
        let folder: String =
            worktree.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' }).collect();
        let sessions = dir.path().join("user-home/.claude/projects").join(folder);
        std::fs::create_dir_all(&sessions).unwrap();
        let line = r#"{"type":"user","timestamp":"2100-01-01T00:00:00Z","message":{"content":"hi"}}"#;
        let session_of = |engine: &Engine, key: AgentKey| {
            engine.state().task(key.task).unwrap().agent(key.agent).unwrap().session.clone()
        };
        let wait = |engine: &mut Engine, key: AgentKey, want: &str| {
            let deadline = Instant::now() + Duration::from_secs(8);
            while session_of(engine, key).as_deref() != Some(want) {
                assert!(Instant::now() < deadline, "{key:?}: {:?}", session_of(engine, key));
                engine.poll_events();
                thread::sleep(Duration::from_millis(50));
            }
        };
        std::fs::write(sessions.join("aaaa-1.jsonl"), format!("{line}\n")).unwrap();
        wait(&mut engine, writer, "aaaa-1");

        let reviewer = engine.start_reviewer(task).unwrap();
        thread::sleep(Duration::from_millis(50));
        std::fs::write(sessions.join("bbbb-2.jsonl"), format!("{line}\n")).unwrap();
        wait(&mut engine, reviewer, "bbbb-2");
        // 写代码的那个也会看到这个更新的文件，但它不是它的。
        for _ in 0..60 {
            engine.poll_events();
            thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(session_of(&engine, writer).as_deref(), Some("aaaa-1"));
        engine.shutdown();
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
