use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use duolo_core::text::{derive_title, now_secs, slug};
use duolo_core::{
    AgentKey, AgentKind, AgentRole, AgentSpec, Config, Paths, Project, ProjectId, State, Store, Task, TaskId,
};
use duolo_git::DiffStat;
use duolo_pty::{OutputHook, Session, SessionSpec, Size};

use crate::status::{AgentStatus, Summary};

/// 改动统计的缓存时间，避免每帧都调用 git。
const DIFF_TTL: Duration = Duration::from_secs(3);

pub struct Engine {
    paths: Paths,
    config: Config,
    store: Store,
    state: State,
    sessions: HashMap<AgentKey, Session>,
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
    pub fn create_task(&mut self, project: &ProjectId, prompt: &str) -> Result<TaskId> {
        let prompt = prompt.trim();
        if prompt.is_empty() {
            bail!("需求不能为空");
        }
        let project = self.state.project(project).cloned().context("项目不存在")?;
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
        });
        self.save()?;
        Ok(id)
    }

    /// 结束任务的所有 Agent，删除 worktree（分支保留），并从列表中移除。
    pub fn close_task(&mut self, id: TaskId) -> Result<()> {
        let task = self.state.task(id).cloned().context("任务不存在")?;
        self.sessions.retain(|key, _| key.task != id);
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
        let t = self.state.task_mut(task).context("任务不存在")?;
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

    // ---- Agent 进程 ----

    /// 启动 Agent。第一次启动会带上需求，之后用恢复会话的方式启动。已在运行时什么也不做。
    pub fn start_agent(&mut self, key: AgentKey) -> Result<()> {
        if let Some(session) = self.sessions.get_mut(&key) {
            if session.poll_exit().is_none() {
                return Ok(());
            }
        }
        let spec = self.session_spec(key)?;
        let session = Session::spawn(spec)?;
        self.sessions.insert(key, session);
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
    }

    fn session_spec(&self, key: AgentKey) -> Result<SessionSpec> {
        let task = self.state.task(key.task).context("任务不存在")?;
        let agent = task.agent(key.agent).context("Agent 不存在")?;
        let profile = self.profile(&agent.kind)?;

        let mut args = profile.args.clone();
        if agent.role == AgentRole::Reviewer {
            args.extend(profile.review_args.iter().cloned());
        }
        if agent.launched {
            args.extend(profile.resume_args.iter().cloned());
        } else {
            args.push(match agent.role {
                AgentRole::Writer => task.prompt.clone(),
                AgentRole::Reviewer => self.config.review_prompt_for(&task.base),
            });
        }

        Ok(SessionSpec {
            program: profile.command.clone(),
            args,
            cwd: task.worktree.clone(),
            env: vec![("DUOLO_TASK_ID".into(), task.id.to_string())],
            size: self.pane,
            on_output: self.on_output.clone(),
        })
    }

    fn profile(&self, kind: &AgentKind) -> Result<&duolo_core::AgentProfile> {
        self.config.profile(kind).with_context(|| format!("配置里没有 Agent「{}」", kind.0))
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
    fn state_survives_reopen() {
        let (dir, mut engine, project) = engine_with_repo();
        let id = engine.create_task(&project, "订单导出 CSV").unwrap();
        drop(engine);
        let engine = Engine::open(Paths::at(dir.path().join("home"))).unwrap();
        assert_eq!(engine.state().task(id).unwrap().title, "订单导出 CSV");
        assert!(engine.state().task(id).unwrap().branch == "duolo/1-csv");
    }

    #[test]
    fn empty_prompt_rejected() {
        let (_dir, mut engine, project) = engine_with_repo();
        assert!(engine.create_task(&project, "   ").is_err());
    }
}
