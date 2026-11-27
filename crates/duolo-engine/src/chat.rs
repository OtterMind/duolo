//! 聊天模式下运行的 Agent（见 `duolo-chat`）：启动、把对话记成事件、在终端和聊天之间切换。
//!
//! 和终端里是同一次会话：会话编号一样，切换时结束现在的进程，换个方式接着这次会话启动。
//! 对话直接来自协议，不读会话记录；你说的话由发送时记下。

use std::collections::HashMap;

use anyhow::{Context, Result};
use duolo_chat::{Chat, Draft, Entry, Protocol, Spec, Update};
use duolo_core::{AgentKey, AgentKind, AgentRole, EventKind, Settings, Speaker, Surface, TaskId, UserError};

use crate::engine::Engine;

impl Engine {
    /// 这个 Agent 要在聊天模式下运行：选了聊天、是写代码的、插件支持。插件不再支持时照旧在终端里运行。
    pub(crate) fn runs_in_chat(&self, key: AgentKey) -> bool {
        let Some(agent) = self.state.task(key.task).and_then(|t| t.agent(key.agent)) else { return false };
        agent.surface == Surface::Chat && agent.role == AgentRole::Writer && self.supports_chat(&agent.kind)
    }

    /// 这种 Agent 能不能在聊天模式下运行。
    pub fn supports_chat(&self, kind: &AgentKind) -> bool {
        self.agents.get(kind).is_ok_and(|p| p.chat().is_some())
    }

    /// 新的写代码的 Agent 用什么方式运行：想要聊天但它不支持时用终端。
    pub(crate) fn surface_for(&self, kind: &AgentKind, wanted: Surface) -> Surface {
        match wanted {
            Surface::Chat if self.supports_chat(kind) => Surface::Chat,
            _ => Surface::Terminal,
        }
    }

    /// 写代码的 Agent 现在用什么方式运行。
    pub fn surface_of(&self, key: AgentKey) -> Surface {
        if self.runs_in_chat(key) {
            Surface::Chat
        } else {
            Surface::Terminal
        }
    }

    /// 换个方式运行写代码的 Agent：结束现在的进程，接着同一次会话在新的方式下启动。记下这次的选择，新任务照着用。
    pub fn set_surface(&mut self, task: TaskId, surface: Surface) -> Result<()> {
        let key = self.writer_key(task)?;
        let kind = self.state.task(task).and_then(|t| t.agent(key.agent)).map(|a| a.kind.clone()).context("agent")?;
        if surface == Surface::Chat && !self.supports_chat(&kind) {
            return Err(UserError::ChatUnsupported.into());
        }
        self.remember_surface(surface);
        if self.surface_of(key) == surface {
            return self.start_agent(key);
        }
        self.update(|state| {
            if let Some(agent) = state.task_mut(task).and_then(|t| t.agent_mut(key.agent)) {
                agent.surface = surface;
            }
            Ok(())
        })?;
        // 丢掉就会在后台结束；还没发出去的话交给新的进程。
        let queued = self.live.pending.remove(&key);
        self.sessions.remove(&key);
        self.chats.remove(&key);
        self.forget_runtime(key);
        self.start_agent(key)?;
        for text in queued.into_iter().flatten() {
            self.queue_message(key, text);
        }
        Ok(())
    }

    /// 记下用户选的运行方式，新任务照着用。
    pub(crate) fn remember_surface(&mut self, surface: Surface) {
        let file = self.paths.settings_file();
        if let Ok(mut settings) = Settings::load(&file) {
            settings.surface = Some(surface);
            let _ = settings.save(&file);
        }
        self.last_surface = Some(surface);
    }

    /// 设置里选的新任务默认运行方式。
    pub fn set_default_surface(&mut self, surface: Surface) {
        self.remember_surface(surface);
    }

    /// 新任务默认用什么方式运行（上次选的）。
    pub fn default_surface(&self) -> Surface {
        self.last_surface.unwrap_or_default()
    }

    /// 在聊天模式下启动。`fresh` 为真时不接回会话。
    pub(crate) fn spawn_chat(&mut self, key: AgentKey, fresh: bool) -> Result<()> {
        self.use_account(key);
        let task = self.state.task(key.task).ok_or(UserError::TaskNotFound)?;
        let agent = task.agent(key.agent).context("agent not found")?.clone();
        let plugin = self.agents.get(&agent.kind)?;
        let mut launch = plugin.chat().ok_or(UserError::ChatUnsupported)?;
        if let Some(text) = self.instructions(task.id).filter(|_| agent.role == AgentRole::Writer) {
            launch.args.extend(plugin.instructions_args(&text));
        }
        let resumed = !fresh && agent.launched;
        // 第一句话只在新开会话时发；接不回原来的会话时也要用，所以总是带上。
        let prompt = agent.prompt.clone().or_else(|| Some(task.prompt.clone()).filter(|p| !p.is_empty()));
        let protocol = Protocol::from_name(&launch.protocol).ok_or(UserError::ChatUnsupported)?;
        let spec = Spec {
            protocol,
            program: launch.program,
            args: launch.args,
            resume_session_args: launch.resume_session_args,
            resume_args: launch.resume_args,
            env: [("DUOLO_TASK_ID".to_string(), task.id.to_string())]
                .into_iter()
                .chain(self.cli_path_var())
                .chain(launch.env)
                .chain(self.account_vars(key))
                .collect(),
            env_remove: self.agents.env_remove(),
            cwd: task.worktree.clone(),
            session: agent.session.clone().filter(|_| resumed),
            // 在终端里开过、还没认出编号的会话：接回这个目录里最近的一次。
            resume_latest: resumed && agent.session.is_none(),
            prompt: prompt.clone(),
        };
        self.trust_folder(key);
        let chat = Chat::spawn(spec)?;
        self.sessions.remove(&key);
        self.live.watchers.remove(&key);
        self.forget_runtime(key);
        if let Some(prompt) = prompt.filter(|_| !resumed) {
            self.drop_queued(key, &prompt);
        }
        let queued = self.live.pending.remove(&key).unwrap_or_default();
        if let Some(note) = agent.resume_note.clone().filter(|_| resumed) {
            chat.send(&note);
        }
        for text in queued {
            chat.send(&text);
        }
        self.chats.insert(key, chat);
        if !agent.launched || agent.resume_note.is_some() || !resumed {
            self.update(|state| {
                if let Some(agent) = state.task_mut(key.task).and_then(|t| t.agent_mut(key.agent)) {
                    agent.launched = true;
                    agent.resume_note = None;
                    if !resumed {
                        agent.session = None;
                    }
                }
                Ok(())
            })?;
        }
        Ok(())
    }

    /// 聊天模式下的 Agent 说的话、用的工具记成事件，记下会话编号。
    pub(crate) fn poll_chats(&mut self) {
        let found: Vec<(AgentKey, Vec<Update>)> =
            self.chats.iter().map(|(key, chat)| (*key, chat.take_updates())).filter(|(_, u)| !u.is_empty()).collect();
        for (key, updates) in found {
            for update in updates {
                match update {
                    Update::Session(id) => self.set_session(key, id),
                    Update::History(entries) => self.record_history(key, entries),
                    Update::Entry(entry) => {
                        self.record(key.task, event_of(key, entry));
                    }
                    Update::Failed(error) => self.agent_failed(key, &error),
                }
            }
        }
    }

    /// 接回会话时重放的以前的对话：已经记过的（在终端里读会话记录记下的、这次之前在聊天模式下记的）跳过，
    /// 没记过的补上。
    fn record_history(&mut self, key: AgentKey, entries: Vec<Entry>) {
        let mut known: HashMap<EventKind, usize> = HashMap::new();
        for event in self.live.log.since(key.task, 0).unwrap_or_default() {
            *known.entry(event.kind).or_default() += 1;
        }
        for entry in entries {
            let kind = event_of(key, entry);
            match known.get_mut(&kind) {
                Some(n) if *n > 0 => *n -= 1,
                _ => {
                    self.record(key.task, kind);
                }
            }
        }
    }

    /// 正在写的话、在想、在用的工具；不在聊天模式下运行时为 None。
    pub fn chat_draft(&self, key: AgentKey) -> Option<Draft> {
        self.chats.get(&key).map(Chat::draft)
    }

    /// 聊天模式下出错的原因（Agent 自己说的）。
    pub fn chat_error(&mut self, key: AgentKey) -> Option<String> {
        self.chats.get_mut(&key).and_then(Chat::error)
    }

    /// 换了进程：上一个进程的状态、提示不算数了。
    fn forget_runtime(&mut self, key: AgentKey) {
        self.resuming.remove(&key);
        self.session_ids.remove(&key);
        self.unsettle(key);
        self.live.prompt.remove(&key);
    }
}

fn event_of(key: AgentKey, entry: Entry) -> EventKind {
    match entry {
        Entry::User(text) => EventKind::Message { speaker: Speaker::User, agent: key.agent, text },
        Entry::Agent(text) => EventKind::Message { speaker: Speaker::Agent, agent: key.agent, text },
        Entry::Tool { tool, target } => EventKind::Tool { agent: key.agent, tool, target },
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use duolo_core::{Command, Paths, TaskEvent};

    use super::*;

    /// 被当成假 Agent 运行时，照着录下来的收发回答（见 `duolo_chat::testing`）；平时什么也不做。
    #[test]
    fn fake_agent() {
        duolo_chat::testing::serve_if_asked();
    }

    fn git(dir: &Path, args: &[&str]) {
        let out = std::process::Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    }

    /// 一个仓库，加一个支持聊天模式的插件 `fake`：命令是这个测试程序自己，照着 `fixture` 回答。
    fn engine(fixture: &str) -> (tempfile::TempDir, Engine, duolo_core::ProjectId) {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("shop");
        std::fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "init"]);
        let plugin = dir.path().join("home/agents/fake");
        std::fs::create_dir_all(&plugin).unwrap();
        let exe = std::env::current_exe().unwrap();
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../duolo-chat/tests/fixtures").join(fixture);
        let manifest = format!(
            "schema = 1\nid = \"fake\"\nname = \"Fake\"\ncommand = {:?}\n\
             [chat]\nprotocol = \"acp\"\nargs = [\"chat::tests::fake_agent\", \"--exact\", \"--nocapture\", \"--test-threads=1\"]\n\
             env = {{ {} = {:?} }}\n",
            exe.display().to_string(),
            duolo_chat::testing::FIXTURE_ENV,
            fixture.display().to_string(),
        );
        std::fs::write(plugin.join("agent.toml"), manifest).unwrap();
        let mut engine = Engine::open(Paths::at(dir.path().join("home"))).unwrap();
        engine.set_agent_home(dir.path().join("user-home"));
        let project = engine.add_project(&repo).unwrap();
        (dir, engine, project)
    }

    fn wait_for(engine: &Mutex<Engine>, what: &str, mut done: impl FnMut(&[TaskEvent]) -> bool) -> Vec<TaskEvent> {
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut all = Vec::new();
        loop {
            all.extend(engine.lock().unwrap().poll_events());
            if done(&all) {
                return all;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}: {all:#?}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn message(events: &[TaskEvent], speaker: Speaker, text: &str) -> bool {
        events
            .iter()
            .any(|e| matches!(&e.kind, EventKind::Message { speaker: s, text: t, .. } if *s == speaker && t == text))
    }

    #[test]
    fn chat_mode_end_to_end() {
        let (_dir, mut engine, project) = engine("permission.txt");
        let fake = AgentKind("fake".into());
        let task = engine.create_task_with(
            &project,
            "Run the tests",
            &crate::TaskOptions { writer: Some(fake.clone()), ..Default::default() },
        );
        let task = task.unwrap();
        let key = engine.writer_key(task).unwrap();
        assert_eq!(engine.surface_of(key), Surface::Terminal, "没选过时在终端里");
        engine.set_surface(task, Surface::Chat).unwrap();
        assert_eq!(engine.surface_of(key), Surface::Chat);
        assert!(engine.session(key).is_none(), "不开终端");
        let engine = Mutex::new(engine);

        // 第一句话是需求；它要跑命令，先问。
        let events =
            wait_for(&engine, "the question", |e| e.iter().any(|e| matches!(e.kind, EventKind::Prompt { .. })));
        assert!(message(&events, Speaker::User, "Run the tests"));
        let (seq, prompt) = events
            .iter()
            .find_map(|e| match &e.kind {
                EventKind::Prompt { question, detail, options, .. } => {
                    Some((e.seq, (question.clone(), detail.clone(), options.clone())))
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(prompt, ("Bash".into(), "pnpm test".into(), vec!["Allow once".to_string(), "Reject".to_string()]));
        assert_eq!(engine.lock().unwrap().agent_status(key), crate::AgentStatus::NeedsInput);
        let stale = crate::ops::execute(&engine, Command::Answer { task, option: 0, prompt: Some(seq + 100) });
        assert_eq!(stale.error.as_deref(), Some("promptChanged"), "不是在问的那条不答");
        let answered = crate::ops::execute(&engine, Command::Answer { task, option: 0, prompt: Some(seq) });
        assert!(answered.ok, "{answered:?}");

        let events = wait_for(&engine, "the reply", |e| message(e, Speaker::Agent, "You chose once."));
        assert!(events.iter().any(
            |e| matches!(&e.kind, EventKind::Tool { tool, target, .. } if tool == "Bash" && target == "pnpm test")
        ));
        let session = |engine: &Mutex<Engine>| {
            engine.lock().unwrap().state().task(task).unwrap().agent(key.agent).unwrap().session.clone()
        };
        assert_eq!(session(&engine).as_deref(), Some("s-1"), "记下会话编号，下次接回");

        // 手机发来的消息也走同一条路。
        let sent = crate::ops::execute(&engine, Command::SendMessage { task, text: "again".into() });
        assert!(sent.ok, "{sent:?}");
        wait_for(&engine, "the second turn", |e| {
            message(e, Speaker::User, "again") && message(e, Speaker::Agent, "Second turn.")
        });
        let status = wait_for(&engine, "idle", |e| {
            e.iter().any(|e| matches!(&e.kind, EventKind::Status { status, .. } if status == "idle"))
        });
        assert!(!status.is_empty());

        let mut engine = engine.into_inner().unwrap();
        engine.shutdown();
        assert!(!engine.has_live_sessions());
    }

    #[test]
    fn only_agents_with_chat_can_switch() {
        let (_dir, mut engine, project) = engine("permission.txt");
        let opts = crate::TaskOptions { writer: Some(AgentKind("pi".into())), ..Default::default() };
        let task = engine.create_task_with(&project, "x", &opts).unwrap();
        let err = engine.set_surface(task, Surface::Chat).unwrap_err();
        assert_eq!(err.downcast_ref(), Some(&UserError::ChatUnsupported));
        assert!(engine.supports_chat(&AgentKind("fake".into())));
        assert!(!engine.supports_chat(&AgentKind("pi".into())));
    }

    #[test]
    fn new_task_runs_where_chosen_and_remembers_it() {
        let (_dir, mut engine, project) = engine("permission.txt");
        let fake = Some(AgentKind("fake".into()));
        let chat = crate::TaskOptions { writer: fake.clone(), surface: Some(Surface::Chat), ..Default::default() };
        let first = engine.create_task_with(&project, "x", &chat).unwrap();
        assert_eq!(engine.surface_of(engine.writer_key(first).unwrap()), Surface::Chat);
        assert_eq!(engine.default_surface(), Surface::Chat, "选了的下次照着用");
        let again = crate::TaskOptions { writer: fake, ..Default::default() };
        let second = engine.create_task_with(&project, "y", &again).unwrap();
        assert_eq!(engine.surface_of(engine.writer_key(second).unwrap()), Surface::Chat);
        // 不支持聊天的照旧在终端里运行。
        let pi = crate::TaskOptions { writer: Some(AgentKind("pi".into())), ..Default::default() };
        let third = engine.create_task_with(&project, "z", &pi).unwrap();
        assert_eq!(engine.surface_of(engine.writer_key(third).unwrap()), Surface::Terminal);
    }

    #[test]
    fn replayed_history_is_recorded_once() {
        let (_dir, mut engine, project) = engine("permission.txt");
        let opts = crate::TaskOptions { writer: Some(AgentKind("fake".into())), ..Default::default() };
        let task = engine.create_task_with(&project, "x", &opts).unwrap();
        let key = engine.writer_key(task).unwrap();
        engine.record(task, EventKind::Message { speaker: Speaker::User, agent: key.agent, text: "hi".into() });
        let history = vec![
            Entry::User("hi".into()),
            Entry::Agent("hello".into()),
            Entry::Tool { tool: "Read".into(), target: "a.ts".into() },
        ];
        engine.record_history(key, history.clone());
        engine.record_history(key, history);
        let events = engine.events_since(task, 0).unwrap();
        let messages =
            events.iter().filter(|e| matches!(e.kind, EventKind::Message { .. } | EventKind::Tool { .. })).count();
        assert_eq!(messages, 3, "终端里记过的、上次接回时补过的都不再记");
    }
}
