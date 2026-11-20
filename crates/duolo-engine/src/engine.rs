use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use duolo_core::text::{derive_title, now_secs, slug};
use duolo_core::{
    AgentKey, AgentKind, AgentRole, AgentSpec, Config, Paths, Project, ProjectId, Review, State, Store, Task, TaskId,
    UserError,
};
use duolo_git::DiffStat;
use duolo_pty::{OutputHook, OutputSink, Session, SessionSpec, Size};

use crate::review::ReviewJob;
use crate::status::{AgentStatus, Summary};

/// 改动统计的缓存时间，避免每帧都调用 git。
const DIFF_TTL: Duration = Duration::from_secs(3);
/// Claude Code 给自己子进程设置的会话标记。从 Claude Code 里启动 Duolo 时会被继承下来，
/// 让 Agent 以为自己是子会话（例如不保存对话记录，之后就无法恢复），启动 Agent 前去掉。
const INHERITED_SESSION_VARS: &[&str] = &[
    "CLAUDECODE",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_SESSION_ATTENDED",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
    "CLAUDE_CODE_EXECPATH",
    "CLAUDE_PID",
    "CLAUDE_EFFORT",
];
/// 恢复会话后在这段时间内异常退出，就认为没有可以恢复的会话。
const RESUME_GRACE: Duration = Duration::from_secs(5);

pub struct Engine {
    paths: Paths,
    config: Config,
    store: Store,
    state: State,
    sessions: HashMap<AgentKey, Session>,
    /// 每次启动进程都分配一个新编号，界面据此判断要不要重新订阅输出。
    session_ids: HashMap<AgentKey, u64>,
    next_session_id: u64,
    /// 以"恢复会话"方式启动、还在观察期内的 Agent。
    resuming: HashSet<AgentKey>,
    reviews: HashMap<TaskId, ReviewJob>,
    /// 审阅结论使用的语言（英文名），跟随界面语言。
    reply_language: String,
    pane: Size,
    on_output: Option<OutputHook>,
    diffs: HashMap<TaskId, (Instant, Option<DiffStat>)>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("root", &self.paths.root())
            .field("projects", &self.state.projects.len())
            .field("tasks", &self.state.tasks.len())
            .field("live_sessions", &self.sessions.len())
            .finish_non_exhaustive()
    }
}

impl Engine {
    /// 读取配置和状态。
    pub fn open(paths: Paths) -> Result<Self> {
        paths.ensure()?;
        let config = Config::load(&paths.config_file())?;
        let store = Store::new(paths.state_file());
        let state = store.load()?;
        Ok(Self {
            paths,
            config,
            store,
            state,
            sessions: HashMap::new(),
            session_ids: HashMap::new(),
            next_session_id: 1,
            resuming: HashSet::new(),
            reviews: HashMap::new(),
            reply_language: duolo_core::config::DEFAULT_REPLY_LANGUAGE.to_string(),
            pane: Size::new(24, 80),
            on_output: None,
            diffs: HashMap::new(),
        })
    }

    pub fn paths(&self) -> &Paths {
        &self.paths
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    /// 设置审阅结论使用的语言（英文名，例如 `Simplified Chinese`）。界面切换语言时调用。
    pub fn set_reply_language(&mut self, language: &str) {
        self.reply_language = language.to_string();
    }

    /// Agent 有新输出时调用，界面层用来触发重绘。
    pub fn set_output_hook(&mut self, hook: OutputHook) {
        self.on_output = Some(hook);
    }

    /// 设置 Agent 窗格的尺寸，并同步给所有运行中的会话。
    pub fn set_pane_size(&mut self, size: Size) {
        self.pane = size;
        for session in self.sessions.values_mut() {
            let _ = session.resize(size);
        }
    }

    // ---- 项目 ----

    /// 添加 `path` 所在的仓库为项目；已添加过时直接返回已有项目。
    pub fn add_project(&mut self, path: &Path) -> Result<ProjectId> {
        let root = duolo_git::repo_root(path)?;
        if let Some(existing) = self.state.projects.iter().find(|p| p.root == root) {
            return Ok(existing.id.clone());
        }
        let name = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "project".into());
        let id = self.state.unique_project_id(&name);
        self.state.projects.push(Project { id: id.clone(), name, root });
        self.save()?;
        Ok(id)
    }

    // ---- 任务 ----

    /// 新建任务：创建分支和 worktree，登记写代码的 Agent。Agent 在 [`Engine::start_agent`] 时才启动。
    ///
    /// `prompt` 可以为空：Agent 直接打开等用户在它的输入框里输入，第一次输入后用
    /// [`Engine::name_task`] 补上标题和分支名。
    pub fn create_task(&mut self, project: &ProjectId, prompt: &str) -> Result<TaskId> {
        let prompt = prompt.trim();
        let project = self.state.project(project).cloned().ok_or(UserError::ProjectNotFound)?;
        let id = self.state.allocate_task_id();
        let title = derive_title(prompt);
        let dir_name = format!("{}-{}", id, slug(&title));
        let branch = format!("duolo/{dir_name}");
        let worktree = self.paths.worktrees().join(&project.id.0).join(&dir_name);
        let base = duolo_git::current_branch(&project.root)?;
        duolo_git::add_worktree(&project.root, &worktree, &branch, &base)?;

        let writer = AgentSpec { id: 0, kind: self.config.writer.clone(), role: AgentRole::Writer, launched: false };
        self.state.tasks.push(Task {
            id,
            project: project.id,
            title,
            prompt: prompt.to_string(),
            branch,
            base,
            worktree,
            created_at: now_secs(),
            agents: vec![writer],
            review: None,
        });
        self.save()?;
        Ok(id)
    }

    /// 给还没有需求的任务补上需求：更新标题，并把分支重命名为对应的名字（失败时保留原分支名）。
    /// 已经有需求的任务不变。
    pub fn name_task(&mut self, id: TaskId, prompt: &str) -> Result<()> {
        let prompt = prompt.trim();
        let task = self.state.task_mut(id).ok_or(UserError::TaskNotFound)?;
        if prompt.is_empty() || !task.prompt.is_empty() {
            return Ok(());
        }
        task.prompt = prompt.to_string();
        task.title = derive_title(prompt);
        let branch = format!("duolo/{}-{}", id, slug(&task.title));
        if branch != task.branch && duolo_git::rename_branch(&task.worktree, &branch).is_ok() {
            task.branch = branch;
        }
        self.save()
    }

    /// 结束任务的所有 Agent，删除 worktree（分支保留），并从列表中移除。
    pub fn close_task(&mut self, id: TaskId) -> Result<()> {
        let task = self.state.task(id).cloned().ok_or(UserError::TaskNotFound)?;
        self.sessions.retain(|key, _| key.task != id);
        self.reviews.remove(&id);
        self.diffs.remove(&id);
        if task.worktree.exists() {
            let root = self.state.project(&task.project).map(|p| p.root.clone());
            match root {
                Some(root) => duolo_git::remove_worktree(&root, &task.worktree, true)?,
                None => std::fs::remove_dir_all(&task.worktree)?,
            }
        }
        self.state.tasks.retain(|t| t.id != id);
        self.save()
    }

    /// 给任务加上审阅 Agent 并启动；已有审阅者时直接启动它。
    pub fn start_reviewer(&mut self, task: TaskId) -> Result<AgentKey> {
        let reviewer_kind = self.config.reviewer.clone();
        let t = self.state.task_mut(task).ok_or(UserError::TaskNotFound)?;
        let agent_id = match t.agent_with_role(AgentRole::Reviewer) {
            Some(existing) => existing.id,
            None => {
                let id = t.next_agent_id();
                t.agents.push(AgentSpec { id, kind: reviewer_kind, role: AgentRole::Reviewer, launched: false });
                id
            }
        };
        let key = AgentKey { task, agent: agent_id };
        self.start_agent(key)?;
        Ok(key)
    }

    // ---- 后台审阅 ----

    /// 在后台审阅任务相对基准分支的全部改动。已经在审阅时什么也不做。
    pub fn start_review(&mut self, id: TaskId) -> Result<()> {
        if self.reviews.contains_key(&id) {
            return Ok(());
        }
        let task = self.state.task(id).ok_or(UserError::TaskNotFound)?;
        let prompt = self.config.review_prompt_for(&task.base, &self.reply_language);
        let job =
            ReviewJob::spawn(&self.config.review_command, &prompt, task.worktree.clone(), INHERITED_SESSION_VARS)?;
        self.reviews.insert(id, job);
        Ok(())
    }

    pub fn cancel_review(&mut self, id: TaskId) {
        self.reviews.remove(&id);
    }

    /// 正在审阅时返回开始时间（Unix 秒）。
    pub fn review_started_at(&self, id: TaskId) -> Option<u64> {
        self.reviews.get(&id).map(ReviewJob::started_at)
    }

    /// 收集已经结束的审阅，保存结论。界面定期调用，返回有新结论的任务。
    pub fn poll_reviews(&mut self) -> Vec<TaskId> {
        let mut done = Vec::new();
        for (&id, job) in &self.reviews {
            if let Some(result) = job.take_result() {
                let (ok, output) = match result {
                    Ok(output) => (true, output),
                    Err(err) => (false, err),
                };
                if let Some(task) = self.state.task_mut(id) {
                    task.review = Some(Review { finished_at: now_secs(), ok, output });
                }
                done.push(id);
            }
        }
        for id in &done {
            self.reviews.remove(id);
        }
        if !done.is_empty() {
            let _ = self.save();
        }
        done
    }

    // ---- Agent 进程 ----

    /// 启动 Agent。第一次启动会带上需求，之后用恢复会话的方式启动。已在运行时什么也不做。
    pub fn start_agent(&mut self, key: AgentKey) -> Result<()> {
        if let Some(session) = self.sessions.get_mut(&key) {
            if session.poll_exit().is_none() {
                return Ok(());
            }
        }
        self.spawn(key, false)
    }

    /// 恢复会话很快就异常退出时（通常是上次还没产生对话就被关掉了），改为带着需求重新开始。
    /// 界面定期调用，返回重新开始的 Agent。
    pub fn retry_failed_resumes(&mut self) -> Vec<AgentKey> {
        let mut retry = Vec::new();
        let keys: Vec<AgentKey> = self.resuming.iter().copied().collect();
        for key in keys {
            let Some(session) = self.sessions.get_mut(&key) else {
                self.resuming.remove(&key);
                continue;
            };
            match session.poll_exit() {
                Some(exit) if !exit.success => retry.push(key),
                Some(_) => {
                    self.resuming.remove(&key);
                }
                None if session.started().elapsed() >= RESUME_GRACE => {
                    self.resuming.remove(&key);
                }
                None => {}
            }
        }
        retry.retain(|&key| self.spawn(key, true).is_ok());
        retry
    }

    /// 当前进程的编号；没有进程时为 None。
    pub fn session_id(&self, key: AgentKey) -> Option<u64> {
        self.sessions.contains_key(&key).then(|| self.session_ids.get(&key).copied()).flatten()
    }

    /// 启动进程。`fresh` 为真时不恢复会话，重新带上需求。
    fn spawn(&mut self, key: AgentKey, fresh: bool) -> Result<()> {
        let spec = self.session_spec(key, fresh)?;
        let resumed = !fresh && self.state.task(key.task).and_then(|t| t.agent(key.agent)).is_some_and(|a| a.launched);
        let session = Session::spawn(spec)?;
        self.sessions.insert(key, session);
        self.session_ids.insert(key, self.next_session_id);
        self.next_session_id += 1;
        if resumed {
            self.resuming.insert(key);
        } else {
            self.resuming.remove(&key);
        }
        if let Some(agent) = self.state.task_mut(key.task).and_then(|t| t.agent_mut(key.agent)) {
            if !agent.launched {
                agent.launched = true;
                self.save()?;
            }
        }
        Ok(())
    }

    pub fn stop_agent(&mut self, key: AgentKey) {
        self.sessions.remove(&key);
    }

    /// 订阅 Agent 的原始输出，先重放已有输出。Agent 没有运行时报错。
    pub fn attach(&self, key: AgentKey, sink: OutputSink) -> Result<()> {
        self.sessions.get(&key).ok_or(UserError::AgentNotRunning)?.attach(sink);
        Ok(())
    }

    /// 把按键或文字原样写给 Agent。
    pub fn write(&mut self, key: AgentKey, bytes: &[u8]) -> Result<()> {
        self.sessions.get_mut(&key).ok_or(UserError::AgentNotRunning)?.write(bytes)
    }

    /// 粘贴一段文字（Agent 开启括号粘贴时带上标记），`submit` 为真时再按一次回车。
    pub fn paste(&mut self, key: AgentKey, text: &str, submit: bool) -> Result<()> {
        let session = self.sessions.get_mut(&key).ok_or(UserError::AgentNotRunning)?;
        session.paste(text)?;
        if submit {
            session.write(b"\r")?;
        }
        Ok(())
    }

    pub fn session(&self, key: AgentKey) -> Option<&Session> {
        self.sessions.get(&key)
    }

    pub fn session_mut(&mut self, key: AgentKey) -> Option<&mut Session> {
        self.sessions.get_mut(&key)
    }

    pub fn agent_status(&mut self, key: AgentKey) -> AgentStatus {
        match self.sessions.get_mut(&key) {
            Some(session) => session.state().into(),
            None => AgentStatus::NotStarted,
        }
    }

    pub fn summary(&mut self) -> Summary {
        let mut summary = Summary::default();
        for session in self.sessions.values_mut() {
            match AgentStatus::from(session.state()) {
                AgentStatus::NeedsInput => summary.needs_input += 1,
                AgentStatus::Running => summary.running += 1,
                _ => {}
            }
        }
        summary
    }

    pub fn has_live_sessions(&mut self) -> bool {
        self.sessions.values_mut().any(|s| s.poll_exit().is_none())
    }

    /// 任务相对基准分支的改动统计，带短时缓存。
    pub fn diff_stat(&mut self, task: TaskId) -> Option<DiffStat> {
        if let Some((at, stat)) = self.diffs.get(&task) {
            if at.elapsed() < DIFF_TTL {
                return *stat;
            }
        }
        let stat = self.state.task(task).and_then(|t| duolo_git::diff_stat(&t.worktree, &t.base).ok());
        self.diffs.insert(task, (Instant::now(), stat));
        stat
    }

    /// 结束所有 Agent 进程。
    pub fn shutdown(&mut self) {
        self.sessions.clear();
        self.reviews.clear();
    }

    fn session_spec(&self, key: AgentKey, fresh: bool) -> Result<SessionSpec> {
        let task = self.state.task(key.task).ok_or(UserError::TaskNotFound)?;
        let agent = task.agent(key.agent).context("agent not found")?;
        let profile = self.profile(&agent.kind)?;

        let mut args = profile.args.clone();
        if agent.role == AgentRole::Reviewer {
            args.extend(profile.review_args.iter().cloned());
        }
        if agent.launched && !fresh {
            args.extend(profile.resume_args.iter().cloned());
        } else {
            match agent.role {
                AgentRole::Writer if task.prompt.is_empty() => {}
                AgentRole::Writer => args.push(task.prompt.clone()),
                AgentRole::Reviewer => args.push(self.config.review_prompt_for(&task.base, &self.reply_language)),
            }
        }

        Ok(SessionSpec {
            program: profile.command.clone(),
            args,
            cwd: task.worktree.clone(),
            env: vec![("DUOLO_TASK_ID".into(), task.id.to_string())],
            env_remove: INHERITED_SESSION_VARS.iter().map(|v| v.to_string()).collect(),
            size: self.pane,
            on_output: self.on_output.clone(),
        })
    }

    fn profile(&self, kind: &AgentKind) -> Result<&duolo_core::AgentProfile> {
        self.config.profile(kind).ok_or_else(|| UserError::UnknownAgent { kind: kind.0.clone() }.into())
    }

    fn save(&self) -> Result<()> {
        self.store.save(&self.state)
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::thread;

    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let ok = Command::new("git").arg("-C").arg(dir).args(args).status().unwrap().success();
        assert!(ok, "git {args:?}");
    }

    /// 用 `sh -c cat` 冒充 Agent，测试不依赖真实的 claude / codex。
    fn engine_with_repo() -> (tempfile::TempDir, Engine, ProjectId) {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("shop");
        std::fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "init"]);

        let home = dir.path().join("home");
        std::fs::create_dir(&home).unwrap();
        std::fs::write(
            home.join("config.toml"),
            // `sh -c cat agent <需求>`：多出的参数被 sh 当作位置参数忽略，cat 一直等待输入。
            "[agents.claude]\ncommand = \"sh\"\nargs = [\"-c\", \"cat\", \"agent\"]\n\
             [agents.codex]\ncommand = \"sh\"\nargs = [\"-c\", \"cat\", \"agent\"]\n",
        )
        .unwrap();
        let mut engine = Engine::open(Paths::at(&home)).unwrap();
        let project = engine.add_project(&repo).unwrap();
        (dir, engine, project)
    }

    #[test]
    fn task_lifecycle() {
        let (_dir, mut engine, project) = engine_with_repo();
        let root = engine.state().projects[0].root.clone();
        assert_eq!(engine.add_project(&root).unwrap(), project);

        let id = engine.create_task(&project, "Add Stripe subscriptions\n月付和年付").unwrap();
        let task = engine.state().task(id).unwrap().clone();
        assert_eq!(task.title, "Add Stripe subscriptions");
        assert_eq!(task.branch, "duolo/1-add-stripe-subscriptions");
        assert_eq!(task.base, "main");
        assert!(task.worktree.exists());

        let writer = task.key(0);
        assert_eq!(engine.agent_status(writer), AgentStatus::NotStarted);
        engine.start_agent(writer).unwrap();
        assert!(engine.agent_status(writer).is_live());
        assert!(engine.state().task(id).unwrap().agents[0].launched);

        let reviewer = engine.start_reviewer(id).unwrap();
        assert_eq!(reviewer.agent, 1);
        assert_eq!(engine.start_reviewer(id).unwrap(), reviewer, "不会重复添加审阅者");

        // 等待 cat 进入空闲。
        thread::sleep(Duration::from_millis(1_700));
        assert_eq!(engine.agent_status(writer), AgentStatus::Idle);

        engine.close_task(id).unwrap();
        assert!(!task.worktree.exists());
        assert!(engine.state().tasks.is_empty());
        assert_eq!(engine.agent_status(writer), AgentStatus::NotStarted);
    }

    #[test]
    fn failed_resume_starts_fresh() {
        let (dir, mut engine, project) = engine_with_repo();
        let id = engine.create_task(&project, "订单导出 CSV").unwrap();
        let key = engine.state().task(id).unwrap().key(0);
        engine.start_agent(key).unwrap();
        drop(engine);

        // 恢复参数让进程立刻失败；重新开始时没有这个参数，进程正常运行。
        let home = dir.path().join("home");
        std::fs::write(
            home.join("config.toml"),
            "[agents.claude]\ncommand = \"sh\"\nargs = [\"-c\", \"test \\\"$0\\\" = resume && exit 3; cat\"]\n\
             resume_args = [\"resume\"]\n",
        )
        .unwrap();
        let mut engine = Engine::open(Paths::at(&home)).unwrap();
        engine.start_agent(key).unwrap();
        let first = engine.session_id(key).unwrap();
        thread::sleep(Duration::from_millis(300));
        assert_eq!(engine.retry_failed_resumes(), vec![key]);
        assert!(engine.session_id(key).unwrap() > first);
        thread::sleep(Duration::from_millis(300));
        assert!(engine.agent_status(key).is_live());
        assert!(engine.retry_failed_resumes().is_empty());
    }

    #[test]
    fn background_review_saves_conclusion() {
        let (dir, mut engine, project) = engine_with_repo();
        let id = engine.create_task(&project, "订单导出 CSV").unwrap();
        drop(engine);
        let home = dir.path().join("home");
        // 冒充审阅命令：输出结论，并确认收到了带基准分支的提示词。
        std::fs::write(
            home.join("config.toml"),
            "review_command = [\"sh\", \"-c\", \"echo 没有发现问题; echo \\\"$0\\\" | grep -q main\"]\n",
        )
        .unwrap();
        let mut engine = Engine::open(Paths::at(&home)).unwrap();
        engine.start_review(id).unwrap();
        assert!(engine.review_started_at(id).is_some());
        let deadline = Instant::now() + Duration::from_secs(5);
        while engine.poll_reviews().is_empty() {
            assert!(Instant::now() < deadline, "审阅超时");
            thread::sleep(Duration::from_millis(50));
        }
        let review = engine.state().task(id).unwrap().review.clone().unwrap();
        assert!(review.ok, "{}", review.output);
        assert_eq!(review.summary(), "没有发现问题");
        assert!(engine.review_started_at(id).is_none());
    }

    #[test]
    fn state_survives_reopen() {
        let (dir, mut engine, project) = engine_with_repo();
        let id = engine.create_task(&project, "订单导出 CSV").unwrap();
        drop(engine);
        let engine = Engine::open(Paths::at(dir.path().join("home"))).unwrap();
        assert_eq!(engine.state().task(id).unwrap().title, "订单导出 CSV");
        assert!(engine.state().task(id).unwrap().branch == "duolo/1-csv");
    }

    #[test]
    fn untitled_task_gets_named_by_first_prompt() {
        let (_dir, mut engine, project) = engine_with_repo();
        let id = engine.create_task(&project, "   ").unwrap();
        let task = engine.state().task(id).unwrap().clone();
        assert_eq!(task.title, "新任务");
        assert_eq!(task.branch, "duolo/1-task");

        engine.name_task(id, "Add Stripe subscriptions").unwrap();
        let task = engine.state().task(id).unwrap().clone();
        assert_eq!(task.title, "Add Stripe subscriptions");
        assert_eq!(task.branch, "duolo/1-add-stripe-subscriptions");
        assert_eq!(duolo_git::current_branch(&task.worktree).unwrap(), task.branch);

        engine.name_task(id, "另一个需求").unwrap();
        assert_eq!(engine.state().task(id).unwrap().title, "Add Stripe subscriptions", "只在第一次命名");
    }
}
