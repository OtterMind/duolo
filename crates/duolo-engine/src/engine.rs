use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use duolo_agent::{Plugin, Registry, Start};
use duolo_chat::Chat;
use duolo_core::config::{CHANGES_IN_FILE, CHANGES_IN_GIT};
use duolo_core::hook::HOOK_DIR_ENV;
use duolo_core::process::OneShot;
use duolo_core::text::{derive_title, now_secs, slug};
use duolo_core::{
    AgentKey, AgentKind, AgentRole, AgentSpec, Config, EventKind, Paths, Project, ProjectId, Review, Settings, Stamp,
    State, Store, Surface, Task, TaskId, UserError, Verdict,
};
use duolo_git::DiffStat;
use duolo_pty::{OutputHook, OutputSink, Rules, Session, SessionSpec, Size};

use crate::diffs::Diffs;
use crate::live::LiveState;
use crate::review::ReviewJob;
use crate::status::{AgentStatus, Summary};

/// 关闭任务时 worktree 改成的名字：`.closing-<Unix 秒>-<任务编号>`。
const CLOSING_PREFIX: &str = ".closing-";
/// 恢复会话后在这段时间内（或者还没画出输入框时）异常退出，就认为没有可以恢复的会话。
const RESUME_GRACE: Duration = Duration::from_secs(5);

/// 新建任务时的选择，没设置的按配置默认。
#[derive(Debug, Clone, Default)]
pub struct TaskOptions {
    /// 改项目里的哪个仓库，默认是项目本身（项目就是仓库时）或项目里的第一个仓库。
    pub repo: Option<PathBuf>,
    /// 基于哪个分支，默认是仓库的当前分支。
    pub base: Option<String>,
    /// 新分支的名字，默认是 `duolo/<编号>-<需求>`。
    pub branch: Option<String>,
    /// 写代码的 Agent。
    pub writer: Option<AgentKind>,
    /// 审阅的 Agent。
    pub reviewer: Option<AgentKind>,
    /// 写代码的 Agent 在终端里还是聊天模式下运行，默认和上次一样。选了的记下来，下次照着用。
    pub surface: Option<Surface>,
}

/// 关闭任务的结果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Closed {
    /// 没提交的改动保存到了哪个分支；没有要保存的为 None。
    pub saved_to: Option<String>,
    /// 工作目录已经不是 git worktree，没有删除，留在这里。
    pub left_at: Option<PathBuf>,
}

/// 准备好、还没建 worktree 的新任务，见 [`Engine::prepare_task`]。
#[derive(Debug)]
pub struct NewTask {
    pub(crate) task: Task,
    pub(crate) repo: PathBuf,
    /// 分出的任务：照着这个 worktree 现在的样子建（见 [`Engine::prepare_fork`]）。
    pub(crate) like: Option<PathBuf>,
}

impl NewTask {
    /// 建分支和 worktree。
    pub fn check_out(&self) -> Result<()> {
        match &self.like {
            Some(from) => {
                let snapshot = duolo_git::snapshot_commit(from, crate::switch::FORK_MESSAGE)?;
                duolo_git::add_worktree_like(&self.repo, &self.task.worktree, &self.task.branch, &snapshot)
            }
            None => duolo_git::add_worktree(&self.repo, &self.task.worktree, &self.task.branch, &self.task.base),
        }
    }
}

/// 正在关闭的任务，见 [`Engine::begin_close`]。
#[derive(Debug)]
pub struct Closing {
    task: Task,
    repo: Option<PathBuf>,
    sessions: Vec<Session>,
    chats: Vec<Chat>,
    review: Option<ReviewJob>,
}

impl Closing {
    /// 停下 Agent，保存没提交的改动，删除 worktree。
    pub fn run(&mut self) -> Result<Closed> {
        // Agent 都停下来之后再保存，免得保存之后又写进来新的改动。
        stop_all(std::mem::take(&mut self.sessions), std::mem::take(&mut self.chats), self.review.take());
        let task = &self.task;
        let mut closed = Closed::default();
        if task.worktree.exists() {
            match &self.repo {
                Some(repo) if duolo_git::is_worktree(&task.worktree) => {
                    let fallback = format!("duolo/{}-saved", task.id);
                    let message = duolo_core::t("task.commitMessage");
                    let saved = duolo_git::save_worktree(&task.worktree, &task.base, &fallback, &message)
                        .map_err(save_error)?;
                    let late = remove_worktree(task, repo, &fallback, &message)?;
                    closed.saved_to = late.or(saved);
                }
                // 主仓库没了，或者 `.git` 被删了：没法保存里面的改动，目录留给用户自己处理。
                _ => closed.left_at = Some(task.worktree.clone()),
            }
        } else if let Some(repo) = &self.repo {
            // 目录已经不在（例如上次关到一半应用退出了）：登记还在的话一并删掉。
            let _ = duolo_git::remove_worktree(repo, &task.worktree, true);
        }
        Ok(closed)
    }
}

/// 同时停下这些 Agent 和审阅，等它们的进程都消失。
fn stop_all(sessions: Vec<Session>, chats: Vec<Chat>, reviews: impl IntoIterator<Item = ReviewJob>) {
    std::thread::scope(|scope| {
        for mut session in sessions {
            scope.spawn(move || session.stop());
        }
        for mut chat in chats {
            scope.spawn(move || chat.stop());
        }
        for review in reviews {
            scope.spawn(move || review.stop());
        }
    });
}

/// 把 worktree 改名让开，删掉它的登记，再在后台删除文件（`node_modules` 这类目录删起来很慢）。改名是一步完成的：
/// 文件被占用时（Windows）直接失败，不会出现删了一半的目录。
/// 改名的同时可能刚好又写进来一点，之后再保存一次，返回保存到了哪个分支。
fn remove_worktree(task: &Task, repo: &Path, fallback: &str, message: &str) -> Result<Option<String>> {
    let worktree = &task.worktree;
    // 放在同一层目录里：子模块记着到主仓库的相对路径，层数变了就打不开了。
    let moved = worktree.with_file_name(format!("{CLOSING_PREFIX}{}-{}", now_secs(), task.id));
    rename_when_free(worktree, &moved).map_err(|err| {
        if duolo_core::process::is_busy(&err) {
            return UserError::FolderInUse { path: worktree.display().to_string() }.into();
        }
        anyhow::Error::new(err).context(format!("cannot remove {}", worktree.display()))
    })?;
    let removed = duolo_git::save_moved_worktree(&moved, fallback, message)
        .map_err(save_error)
        .and_then(|late| duolo_git::remove_worktree(repo, worktree, true).map(|()| late));
    match removed {
        Ok(late) => {
            std::thread::spawn(move || std::fs::remove_dir_all(moved));
            Ok(late)
        }
        Err(err) => {
            // 又有没法保存的内容，或者登记删不掉（例如用 `git worktree lock` 锁住了）：改回原名，当作没关。
            rename_when_free(&moved, worktree)
                .with_context(|| format!("cannot move {} back to {}", moved.display(), worktree.display()))?;
            Err(err)
        }
    }
}

/// 改名。Windows 上刚结束的进程可能还占着目录里的文件，等一会儿再试。
fn rename_when_free(from: &Path, to: &Path) -> std::io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match std::fs::rename(from, to) {
            Err(err) if duolo_core::process::is_busy(&err) && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(100));
            }
            result => return result,
        }
    }
}

pub struct Engine {
    pub(crate) paths: Paths,
    config: Config,
    pub(crate) agents: Registry,
    store: Store,
    /// 内存里的状态。只通过 [`Engine::update`] 修改，别的进程改了由 [`Engine::sync`] 读进来。
    pub(crate) state: State,
    /// 内存里的状态对应 `state.json` 的哪个版本。
    stamp: Option<Stamp>,
    /// 启动时发现、要告诉用户的问题（数据文件损坏、配置或插件写错了）。
    notices: Vec<UserError>,
    pub(crate) sessions: HashMap<AgentKey, Session>,
    /// 在聊天模式下运行的 Agent（见 `chat.rs`）。一个 Agent 只会在 `sessions`、`chats` 之一里。
    pub(crate) chats: HashMap<AgentKey, Chat>,
    /// 每次启动进程都分配一个新编号，界面据此判断要不要重新订阅输出。
    pub(crate) session_ids: HashMap<AgentKey, u64>,
    next_session_id: u64,
    /// 以"恢复会话"方式启动、还在观察期内的 Agent。
    pub(crate) resuming: HashSet<AgentKey>,
    /// 由 Duolo 定了会话编号、还在观察期内的 Agent。
    assigning: HashSet<AgentKey>,
    /// 钩子命令用的程序（Duolo 自己，见 `duolo_core::hook`）；没设置时不装钩子。
    hook_program: Option<PathBuf>,
    /// 这次运行里发现不认得"指定会话编号"选项的 Agent（版本太旧，或者以后改了名字），之后不再给它定编号。
    no_session_ids: HashSet<AgentKind>,
    reviews: HashMap<TaskId, ReviewJob>,
    /// 审阅结论使用的语言（英文名），跟随界面语言。
    reply_language: String,
    /// 上次叫来审阅的 Agent（`settings.json` 的 `reviewer`）。
    last_reviewer: Option<AgentKind>,
    /// 上次选的运行方式（`settings.json` 的 `surface`），新任务照着用。
    pub(crate) last_surface: Option<Surface>,
    pane: Size,
    /// 每个 Agent 终端自己的尺寸：分屏时两格宽度不同，不能共用一个。没有时用 `pane`。
    sizes: HashMap<AgentKey, Size>,
    /// 多个账号，见 `accounts.rs`。
    pub(crate) accounts: crate::accounts::Accounts,
    on_output: Option<OutputHook>,
    diffs: Diffs,
    /// 正在关闭的任务：不再启动它的 Agent。
    closing: HashSet<TaskId>,
    /// 事件流和远程命令用到的状态，见 `live.rs`。
    pub(crate) live: LiveState,
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
        let mut notices = Vec::new();
        // 配置写错了不拦着启动：先用默认配置，告诉用户哪里错了。文件是用户手写的，不动它。
        let config = Config::load(&paths.config_file()).unwrap_or_else(|err| {
            let path = paths.config_file().display().to_string();
            notices.push(UserError::ConfigInvalid { path, detail: err.root_cause().to_string().trim().to_string() });
            Config::default()
        });
        let (agents, problems) = Registry::load(&config, &paths.agents());
        notices.extend(problems);
        let store = Store::new(paths.state_file());
        let mut live = LiveState::new(paths.events());
        let (state, stamp) = {
            let _lock = store.lock()?;
            let loaded = store.load()?;
            let mut state = loaded.state;
            if let Some(damaged) = &loaded.damaged {
                let path = damaged.display().to_string();
                if loaded.restored {
                    notices.push(UserError::StateRestored { path });
                } else {
                    // 从空的开始，但编号不能和磁盘上留着的 worktree 撞上。
                    state.next_task_id = state.next_task_id.max(highest_task_id(&paths) + 1);
                    store.save(&state)?;
                    notices.push(UserError::StateReset { path });
                }
            } else if !loaded.missing {
                store.ensure_backup(&state);
                live.log.prune(&state.tasks.iter().map(|t| t.id).collect::<Vec<_>>());
            }
            (state, store.stamp())
        };
        remove_leftovers(&paths.worktrees());
        crate::review::remove_leftovers(&paths.reviews());
        let settings = Settings::load(&paths.settings_file()).unwrap_or_default();
        let last_reviewer = settings.reviewer.clone().map(AgentKind);
        let last_surface = settings.surface;
        Ok(Self {
            paths,
            config,
            agents,
            store,
            state,
            stamp,
            notices,
            sessions: HashMap::new(),
            chats: HashMap::new(),
            session_ids: HashMap::new(),
            next_session_id: 1,
            resuming: HashSet::new(),
            assigning: HashSet::new(),
            hook_program: None,
            no_session_ids: HashSet::new(),
            reviews: HashMap::new(),
            reply_language: duolo_core::config::DEFAULT_REPLY_LANGUAGE.to_string(),
            last_reviewer,
            last_surface,
            pane: Size::new(24, 80),
            sizes: HashMap::new(),
            accounts: crate::accounts::Accounts::new(&settings),
            on_output: None,
            diffs: Diffs::default(),
            closing: HashSet::new(),
            live,
        })
    }

    pub fn paths(&self) -> &Paths {
        &self.paths
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// 可用的 Agent 插件。
    pub fn agents(&self) -> &Registry {
        &self.agents
    }

    /// 给新分支起名用的命令；不用 AI 起名、没有能用的 Agent 时为 None。
    pub fn namer(&self) -> Option<OneShot> {
        self.small_job(&self.config.namer)
    }

    fn small_job(&self, configured: &Option<Vec<String>>) -> Option<OneShot> {
        match configured {
            Some(argv) if argv.is_empty() => None,
            Some(argv) => Some(self.agents.clean(argv.clone().into())),
            None => self.agents.quick(),
        }
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    /// 启动时发现、要告诉用户的问题，取一次就清空。
    pub fn take_notices(&mut self) -> Vec<UserError> {
        std::mem::take(&mut self.notices)
    }

    /// 修改状态并保存。桌面应用、终端界面和 `duolo new` 可能同时在运行，所以独占 `state.json`，
    /// 在最新的内容上改；`change` 报错或保存失败时什么都不变。
    /// `change` 里只改状态，不做 git 操作之类费时间的事，也不再调用会修改状态的方法。
    pub(crate) fn update<R>(&mut self, change: impl FnOnce(&mut State) -> Result<R>) -> Result<R> {
        let lock = self.store.lock()?;
        let mut state = self.store.reload()?.unwrap_or_else(|| self.state.clone());
        let result = change(&mut state)?;
        self.store.save(&state)?;
        let stamp = self.store.stamp();
        drop(lock);
        self.adopt(state, stamp);
        Ok(result)
    }

    /// 读进别的进程做的修改。界面定期调用；没有变化时只看一眼文件时间。
    pub fn sync(&mut self) {
        let stamp = self.store.stamp();
        if stamp.is_none() || stamp == self.stamp {
            return;
        }
        if let Some(state) = self.store.peek() {
            self.adopt(state, stamp);
        }
    }

    /// 换成新的状态，已经不在的任务的进程和缓存一并清掉。
    /// 状态是准的：别的进程（命令行、手机）换下了这里正在跑的 Agent，就在这里停掉它，没发出去的消息交给接手的。
    fn adopt(&mut self, state: State, stamp: Option<Stamp>) {
        let old = std::mem::replace(&mut self.state, state);
        self.stamp = stamp;
        for id in old.tasks.iter().map(|t| t.id).filter(|id| self.state.task(*id).is_none()).collect::<Vec<_>>() {
            self.sessions.retain(|key, _| key.task != id);
            self.chats.retain(|key, _| key.task != id);
            self.sizes.retain(|key, _| key.task != id);
            self.reviews.remove(&id);
            self.diffs.remove(id);
            self.forget_task(id);
        }
        let gone: Vec<AgentKey> = self
            .sessions
            .keys()
            .chain(self.chats.keys())
            .filter(|key| self.state.task(key.task).is_some_and(|t| t.agent(key.agent).is_none()))
            .copied()
            .collect();
        for key in gone {
            self.sessions.remove(&key);
            self.chats.remove(&key);
            self.resuming.remove(&key);
            self.session_ids.remove(&key);
            let successor = self.writer_key(key.task).ok();
            self.hand_over(key, successor);
        }
    }

    /// 设置审阅结论使用的语言（英文名，例如 `Simplified Chinese`）。界面切换语言时调用。
    pub fn set_reply_language(&mut self, language: &str) {
        self.reply_language = language.to_string();
    }

    /// Agent 有新输出时调用，界面层用来触发重绘。
    pub fn set_output_hook(&mut self, hook: OutputHook) {
        self.on_output = Some(hook);
    }

    /// 设置 Agent 窗格的尺寸，并同步给所有运行中的会话（只有一个窗格的终端界面用）。
    pub fn set_pane_size(&mut self, size: Size) {
        self.pane = size;
        self.sizes.clear();
        for session in self.sessions.values_mut() {
            let _ = session.resize(size);
        }
    }

    /// 设置一个 Agent 终端的尺寸（桌面端每格各自调整，分屏时两格宽度不同）。也是之后新开的终端的默认尺寸。
    pub fn set_agent_size(&mut self, key: AgentKey, size: Size) {
        self.pane = size;
        self.sizes.insert(key, size);
        if let Some(session) = self.sessions.get_mut(&key) {
            let _ = session.resize(size);
        }
    }

    // ---- 项目 ----

    /// 把 `path` 加为项目；已添加过时直接返回已有项目。
    ///
    /// 项目就是一个工作文件夹：可以是一个 git 仓库（在仓库里的子目录时取仓库根目录），
    /// 也可以是装了好几个仓库的普通文件夹，甚至是空文件夹（之后在里面建仓库）。
    pub fn add_project(&mut self, path: &Path) -> Result<ProjectId> {
        if !path.is_dir() {
            return Err(UserError::NotAFolder { path: path.display().to_string() }.into());
        }
        let root = duolo_git::repo_root(path).unwrap_or_else(|_| path.to_path_buf());
        let name = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "project".into());
        self.update(|state| {
            if let Some(existing) = state.projects.iter().find(|p| p.root == root) {
                return Ok(existing.id.clone());
            }
            let id = state.unique_project_id(&name);
            state.projects.push(Project { id: id.clone(), name, root });
            Ok(id)
        })
    }

    /// 在 `parent` 下新建文件夹 `name`，初始化为 git 仓库，并加为项目。
    pub fn create_project_folder(&mut self, parent: &Path, name: &str) -> Result<ProjectId> {
        let dir = new_folder(parent, name)?;
        duolo_git::init_repo(&dir)?;
        self.add_project(&dir)
    }

    /// 项目里的 git 仓库：项目本身是仓库时就是它自己，否则是项目文件夹里（往下两层）的仓库。
    pub fn repos_of(&self, project: &ProjectId) -> Result<Vec<PathBuf>> {
        Ok(repos_in(&self.state.project(project).ok_or(UserError::ProjectNotFound)?.root))
    }

    /// 在项目里建一个 git 仓库：`name` 为空时把项目文件夹本身初始化为仓库，否则新建子文件夹 `name`。
    /// 返回仓库路径。
    pub fn add_repo(&mut self, project: &ProjectId, name: Option<&str>) -> Result<PathBuf> {
        let dir = self.new_repo_dir(project, name)?;
        duolo_git::init_repo(&dir)?;
        Ok(dir)
    }

    /// [`Engine::add_repo`] 要初始化的文件夹。
    pub(crate) fn new_repo_dir(&self, project: &ProjectId, name: Option<&str>) -> Result<PathBuf> {
        let root = self.state.project(project).ok_or(UserError::ProjectNotFound)?.root.clone();
        match name.map(str::trim).filter(|n| !n.is_empty()) {
            Some(name) => new_folder(&root, name),
            None => Ok(root),
        }
    }

    // ---- 任务 ----

    /// 新建任务：创建分支和 worktree，登记写代码的 Agent。Agent 在 [`Engine::start_agent`] 时才启动。
    ///
    /// `prompt` 可以为空：Agent 直接打开等用户在它的输入框里输入，第一次输入后用
    /// [`Engine::name_task`] 补上标题和分支名。
    pub fn create_task(&mut self, project: &ProjectId, prompt: &str) -> Result<TaskId> {
        self.create_task_with(project, prompt, &TaskOptions::default())
    }

    /// 和 [`Engine::create_task`] 一样，但可以指定基准分支、写代码和审阅的 Agent；没指定的按配置默认。
    pub fn create_task_with(&mut self, project: &ProjectId, prompt: &str, opts: &TaskOptions) -> Result<TaskId> {
        let new = self.prepare_task(project, prompt, opts)?;
        new.check_out()?;
        self.finish_task(new)
    }

    /// 新建任务的第一步：检查选择、定好名字、占住编号。之后用 [`NewTask::check_out`] 建 worktree（大仓库要好几秒，
    /// 不用拿着引擎），再用 [`Engine::finish_task`] 登记。
    pub fn prepare_task(&mut self, project: &ProjectId, prompt: &str, opts: &TaskOptions) -> Result<NewTask> {
        let prompt = prompt.trim();
        let writer = opts.writer.clone().unwrap_or_else(|| self.config.writer.clone());
        // 审阅者只在明确选了时记在任务上；没选的用时再定（上次叫的、配置的默认），见 `reviewer_of`。
        let reviewer = opts.reviewer.clone();
        for kind in std::iter::once(&writer).chain(reviewer.as_ref()) {
            self.agents.get(kind)?;
        }
        let repo = match &opts.repo {
            Some(repo) => repo.clone(),
            None => self.repos_of(project)?.into_iter().next().ok_or(UserError::NoRepository)?,
        };
        let project = self.state.project(project).cloned().ok_or(UserError::ProjectNotFound)?;
        if !repo.starts_with(&project.root) || !duolo_git::is_repo_root(&repo) {
            return Err(UserError::NoRepository.into());
        }
        // 用户自己定的名字：不合法或已存在都直接告诉用户。
        let chosen = opts.branch.as_deref().map(str::trim).filter(|b| !b.is_empty());
        if let Some(name) = chosen {
            if !duolo_git::is_valid_branch_name(&repo, name) {
                return Err(UserError::InvalidBranch { name: name.into() }.into());
            }
            if duolo_git::branch_exists(&repo, name) {
                return Err(UserError::BranchExists { name: name.into() }.into());
            }
        }
        let base = match opts.base.as_deref().map(str::trim).filter(|b| !b.is_empty()) {
            Some(base) => base.to_string(),
            None => duolo_git::current_branch(&repo)?,
        };
        // 先占住编号再建 worktree，别的进程不会拿到同一个。
        let id = self.update(|state| Ok(state.allocate_task_id()))?;
        let title = derive_title(prompt);
        let branch = match chosen {
            Some(name) => name.to_string(),
            None => duolo_git::unique_branch(&repo, &format!("duolo/{}-{}", id, slug(&title))),
        };
        // 工作目录名：自己定了分支名时取分支名的最后一段，否则按需求。
        let label = match chosen {
            Some(_) => slug(branch.rsplit('/').next().unwrap_or(&branch)),
            None => slug(&title),
        };
        let worktree = self.paths.worktrees().join(&project.id.0).join(format!("{id}-{label}"));
        let mut writer = AgentSpec::new(0, writer, AgentRole::Writer);
        if let Some(surface) = opts.surface {
            self.remember_surface(surface);
        }
        writer.surface = self.surface_for(&writer.kind, self.last_surface.unwrap_or_default());
        let task = Task {
            id,
            project: project.id,
            title,
            prompt: prompt.to_string(),
            branch,
            base,
            worktree,
            created_at: now_secs(),
            agents: vec![writer],
            repo: Some(repo.clone()),
            reviewer,
            review: None,
            retired: Vec::new(),
            forked_from: None,
        };
        Ok(NewTask { task, repo, like: None })
    }

    /// 新建任务的最后一步：登记。
    pub fn finish_task(&mut self, new: NewTask) -> Result<TaskId> {
        let NewTask { task, repo, .. } = new;
        let (id, worktree) = (task.id, task.worktree.clone());
        let added = self.update(|state| {
            state.tasks.push(task);
            Ok(())
        });
        if let Err(err) = added {
            // 没能记下来（例如磁盘满了）：刚建的 worktree 里还什么都没有，收拾掉。
            let _ = duolo_git::remove_worktree(&repo, &worktree, true);
            return Err(err);
        }
        self.record_renamed(id, true);
        Ok(id)
    }

    /// 给还没有需求的任务补上需求：更新标题，并把自动起的分支名换成 `branch`（没给时按需求生成；
    /// 重名时加序号，失败时保留原分支名）。已经有需求的任务不变。
    pub fn name_task(&mut self, id: TaskId, prompt: &str, branch: Option<&str>) -> Result<()> {
        let prompt = prompt.trim();
        let task = self.state.task(id).ok_or(UserError::TaskNotFound)?;
        if prompt.is_empty() || !task.prompt.is_empty() {
            return Ok(());
        }
        let title = derive_title(prompt);
        let wanted = branch
            .map(str::trim)
            .filter(|b| duolo_git::is_valid_branch_name(&task.worktree, b))
            .map(String::from)
            .unwrap_or_else(|| format!("duolo/{}-{}", id, slug(&title)));
        let mut renamed = None;
        if wanted != task.branch {
            let branch = duolo_git::unique_branch(&task.worktree, &wanted);
            if duolo_git::rename_branch(&task.worktree, &branch).is_ok() {
                renamed = Some(branch);
            }
        }
        self.update(|state| {
            let task = state.task_mut(id).ok_or(UserError::TaskNotFound)?;
            task.prompt = prompt.to_string();
            task.title = title;
            task.branch = renamed.unwrap_or_else(|| task.branch.clone());
            Ok(())
        })?;
        self.record_renamed(id, false);
        Ok(())
    }

    /// 换掉任务自动起的分支名（`duolo/` 开头的）。用户自己定过的分支名不动；新名字不合法时忽略，重名时加序号。
    pub fn rename_branch(&mut self, id: TaskId, wanted: &str) -> Result<()> {
        let task = self.state.task(id).ok_or(UserError::TaskNotFound)?;
        let wanted = wanted.trim();
        if !task.branch.starts_with("duolo/") || wanted == task.branch {
            return Ok(());
        }
        if !duolo_git::is_valid_branch_name(&task.worktree, wanted) {
            return Err(UserError::InvalidBranch { name: wanted.into() }.into());
        }
        let branch = duolo_git::unique_branch(&task.worktree, wanted);
        duolo_git::rename_branch(&task.worktree, &branch)?;
        self.update(|state| {
            state.task_mut(id).ok_or(UserError::TaskNotFound)?.branch = branch;
            Ok(())
        })?;
        self.record_renamed(id, false);
        Ok(())
    }

    /// 修改任务标题。分支名不变，避免影响正在运行的 Agent。
    pub fn rename_task(&mut self, id: TaskId, title: &str) -> Result<()> {
        let title = title.trim();
        let task = self.state.task(id).ok_or(UserError::TaskNotFound)?;
        if title.is_empty() || task.title == title {
            return Ok(());
        }
        self.update(|state| {
            state.task_mut(id).ok_or(UserError::TaskNotFound)?.title = title.to_string();
            Ok(())
        })?;
        self.record_renamed(id, false);
        Ok(())
    }

    /// 记一条"任务建好了 / 改名了"的事件，带上当前的标题和分支名。
    pub(crate) fn record_renamed(&mut self, id: TaskId, created: bool) {
        let Some(task) = self.state.task(id) else { return };
        let (title, branch) = (task.title.clone(), task.branch.clone());
        let kind =
            if created { EventKind::TaskCreated { title, branch } } else { EventKind::TaskRenamed { title, branch } };
        self.record(id, kind);
    }

    /// 结束任务的所有 Agent，删除 worktree，并从列表中移除。分支保留；没提交的改动先提交到分支上，
    /// 没法完整保存时报错，任务和 worktree 都留着。
    pub fn close_task(&mut self, id: TaskId) -> Result<Closed> {
        let mut closing = self.begin_close(id)?;
        let result = closing.run();
        self.finish_close(id, result)
    }

    /// 关闭任务的第一步：取走它的 Agent 和审阅。之后用 [`Closing::run`] 停止、保存、删除（要好几秒，不用拿着引擎），
    /// 再用 [`Engine::finish_close`] 登记。中间这段时间任务的 Agent 不会再启动。
    pub fn begin_close(&mut self, id: TaskId) -> Result<Closing> {
        let task = self.state.task(id).cloned().ok_or(UserError::TaskNotFound)?;
        if !self.closing.insert(id) {
            return Err(UserError::TaskClosing.into());
        }
        let repo = task.repo.clone().or_else(|| self.state.project(&task.project).map(|p| p.root.clone()));
        let sessions = self.sessions.extract_if(|key, _| key.task == id).map(|(_, session)| session).collect();
        let chats = self.chats.extract_if(|key, _| key.task == id).map(|(_, chat)| chat).collect();
        Ok(Closing { task, repo, sessions, chats, review: self.reviews.remove(&id) })
    }

    /// 关闭任务的最后一步：`result` 是 [`Closing::run`] 的结果，失败时任务照旧。
    pub fn finish_close(&mut self, id: TaskId, result: Result<Closed>) -> Result<Closed> {
        self.closing.remove(&id);
        let closed = result?;
        // 任务的目录没了，替它记下的信任也删掉（每种 Agent 都试一下：中途可能换过人）。钩子的事件也不要了。
        if let Some(task) = self.state.task(id) {
            for plugin in self.agents.iter() {
                let _ = plugin.forget_trust(&self.live.home, &task.worktree);
            }
            // 用别的账号运行过的，信任记在那个账号的文件夹里。
            for agent in task.agents.iter().chain(&task.retired) {
                if let Ok(plugin) = self.agents.get(&agent.kind) {
                    let _ = plugin.forget_trust(&self.home_of(task.key(agent.id)), &task.worktree);
                }
            }
            for agent in task.agents.iter().chain(&task.retired) {
                let dir = self.hook_dir(task.key(agent.id));
                let _ = std::fs::remove_dir_all(&dir);
                let _ = std::fs::remove_file(dir.with_extension("settings.json"));
            }
        }
        self.update(|state| {
            state.tasks.retain(|t| t.id != id);
            Ok(())
        })?;
        self.record(id, EventKind::TaskClosed);
        Ok(closed)
    }

    /// 任务正在关闭。
    pub fn is_closing(&self, id: TaskId) -> bool {
        self.closing.contains(&id)
    }

    /// 给任务加上审阅 Agent 并启动；已有审阅者时直接启动它。
    pub fn start_reviewer(&mut self, task: TaskId) -> Result<AgentKey> {
        let reviewer_kind = self.reviewer_of(task)?;
        let agent_id = self.update(|state| {
            let t = state.task_mut(task).ok_or(UserError::TaskNotFound)?;
            if let Some(existing) = t.agent_with_role(AgentRole::Reviewer) {
                return Ok(existing.id);
            }
            let id = t.next_agent_id();
            t.agents.push(AgentSpec::new(id, reviewer_kind, AgentRole::Reviewer));
            Ok(id)
        })?;
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
        self.check_open(id)?;
        let reviewer = self.reviewer_of(id)?;
        let task = self.state.task(id).ok_or(UserError::TaskNotFound)?;
        let fork = self.fork_of(task);
        let prompt = self.config.review_prompt_for(&task.base, &fork, &self.reply_language, CHANGES_IN_FILE);
        // config.toml 里写了 review_command 时用它，否则用审阅 Agent 自己的只读审阅命令。
        let command = match &self.config.review_command {
            custom if !custom.is_empty() => self.agents.clean(custom.clone().into()),
            _ => self.agents.review(&reviewer)?,
        };
        let copy = crate::review::copy_dir(&self.paths.reviews(), id.0);
        let job = ReviewJob::spawn(&command, &prompt, task.worktree.clone(), copy, fork)?;
        self.reviews.insert(id, job);
        self.record(id, EventKind::ReviewStarted { reviewer: reviewer.0 });
        Ok(())
    }

    /// 任务的审阅 Agent：这个任务上次叫的，没有就是上次在任何任务里叫的，再没有按配置默认。
    pub fn reviewer_of(&self, id: TaskId) -> Result<AgentKind> {
        let task = self.state.task(id).ok_or(UserError::TaskNotFound)?;
        Ok(task.reviewer.clone().or_else(|| self.last_reviewer.clone()).unwrap_or_else(|| self.config.reviewer.clone()))
    }

    /// 叫 `kind` 来审阅任务的改动，并记下这次的选择（这个任务和以后的新任务都先用它）。
    pub fn review_with(&mut self, id: TaskId, kind: AgentKind) -> Result<()> {
        self.set_reviewer(id, kind.clone())?;
        if self.last_reviewer.as_ref() != Some(&kind) {
            let path = self.paths.settings_file();
            let mut settings = Settings::load(&path)?;
            settings.reviewer = Some(kind.0.clone());
            settings.save(&path)?;
            self.last_reviewer = Some(kind);
        }
        self.start_review(id)
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
        let mut results = Vec::new();
        for (&id, job) in &self.reviews {
            if let Some(result) = job.take_result() {
                let (ok, output) = match result {
                    Ok(output) => (true, output),
                    Err(err) => (false, err),
                };
                results.push((id, Review { finished_at: now_secs(), ok, output }));
            }
        }
        if results.is_empty() {
            return Vec::new();
        }
        let put = |state: &mut State| {
            for (id, review) in &results {
                if let Some(task) = state.task_mut(*id) {
                    task.review = Some(review.clone());
                }
            }
        };
        let saved = self.update(|state| {
            put(state);
            Ok(())
        });
        if saved.is_err() {
            // 没存上（例如磁盘满了）：至少这次运行里能看到结论。
            put(&mut self.state);
        }
        let done: Vec<TaskId> = results.into_iter().map(|(id, _)| id).collect();
        for id in &done {
            self.reviews.remove(id);
            let Some(review) = self.state.task(*id).and_then(|t| t.review.clone()) else { continue };
            let (verdict, issues) = match review.verdict() {
                Verdict::Pass => ("pass", 0),
                Verdict::Issues(n) => ("issues", n),
                Verdict::Unknown => ("unknown", 0),
            };
            let kind = EventKind::ReviewFinished {
                ok: review.ok,
                verdict: verdict.into(),
                issues,
                summary: review.summary().to_string(),
            };
            self.record(*id, kind);
        }
        done
    }

    // ---- Agent 进程 ----

    /// 启动 Agent。第一次启动会带上需求，之后用恢复会话的方式启动。已在运行时什么也不做。
    pub fn start_agent(&mut self, key: AgentKey) -> Result<()> {
        if self.is_running(key) {
            return Ok(());
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
                // 停在启动时的对话框上（信任目录等）不算恢复成功：选完之后才会真的去恢复。
                None if session.started().elapsed() >= RESUME_GRACE && self.can_take_message(key) => {
                    self.resuming.remove(&key);
                }
                None => {}
            }
        }
        // 定了会话编号却马上出错退出、而且说的是那个选项：它不认得，不再给它定编号，重新开始。
        let keys: Vec<AgentKey> = self.assigning.iter().copied().collect();
        for key in keys {
            let Some(session) = self.sessions.get_mut(&key) else {
                self.assigning.remove(&key);
                continue;
            };
            match session.poll_exit() {
                Some(exit) if !exit.success => {
                    self.assigning.remove(&key);
                    let Ok(plugin) = self.plugin_of(key) else { continue };
                    let option = plugin.session_id_option().unwrap_or_default().to_string();
                    let said =
                        self.sessions.get(&key).map(|s| s.with_screen(|screen| screen.contents())).unwrap_or_default();
                    if !option.is_empty() && said.contains(&option) {
                        let kind = plugin.kind();
                        self.no_session_ids.insert(kind);
                        retry.push(key);
                    }
                }
                Some(_) => {
                    self.assigning.remove(&key);
                }
                None if session.started().elapsed() >= RESUME_GRACE => {
                    self.assigning.remove(&key);
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

    /// 正在关闭的任务不能再启动东西。
    pub(crate) fn check_open(&self, id: TaskId) -> Result<()> {
        if self.closing.contains(&id) {
            return Err(UserError::TaskClosing.into());
        }
        Ok(())
    }

    /// 任务从基准分支分出来的提交（算不出来时用基准分支的名字）。
    pub(crate) fn fork_of(&self, task: &Task) -> String {
        duolo_git::fork_point(&task.worktree, &task.base).unwrap_or_else(|_| task.base.clone())
    }

    /// 启动进程。`fresh` 为真时不恢复会话，重新带上需求。
    pub(crate) fn spawn(&mut self, key: AgentKey, fresh: bool) -> Result<()> {
        self.check_open(key.task)?;
        if self.runs_in_chat(key) {
            return self.spawn_chat(key, fresh);
        }
        self.chats.remove(&key);
        // 启动过的接着上次的会话；恢复不了的（不知道会话编号，插件又不能"恢复最近一次"）带着需求重新开始。
        let spec = self.state.task(key.task).and_then(|t| t.agent(key.agent)).cloned();
        let resumable =
            spec.as_ref().is_some_and(|a| self.plugin_of(key).is_ok_and(|p| p.can_resume(a.session.as_deref())));
        let resumed = !fresh && resumable && spec.as_ref().is_some_and(|a| a.launched);
        let fresh = !resumed;
        // 新开会话时能定编号的，由 Duolo 定：一开始就知道它的会话记录是哪个，不用猜。
        let kind = self.state.task(key.task).and_then(|t| t.agent(key.agent)).map(|a| a.kind.clone());
        let can_assign =
            self.plugin_of(key)?.assigns_session() && !kind.is_some_and(|k| self.no_session_ids.contains(&k));
        let assigned = (!resumed && can_assign).then(|| uuid::Uuid::new_v4().to_string());
        self.use_account(key);
        let (spec, paste_later) = self.session_spec(key, fresh, assigned.as_deref())?;
        self.trust_folder(key);
        let session = Session::spawn(spec)?;
        self.sessions.insert(key, session);
        if !resumed {
            // 需求已经随启动交给 Agent 了，排队的消息里有同样的话就不再发一遍。
            let prompt = self.state.task(key.task).map(|t| t.prompt.clone()).unwrap_or_default();
            self.drop_queued(key, &prompt);
        }
        if let Some(text) = paste_later {
            self.queue_message(key, text);
        }
        self.unsettle(key);
        self.session_ids.insert(key, self.next_session_id);
        self.next_session_id += 1;
        if resumed {
            self.resuming.insert(key);
        } else {
            self.resuming.remove(&key);
        }
        if assigned.is_some() {
            self.assigning.insert(key);
        } else {
            self.assigning.remove(&key);
        }
        // 换回来的 Agent 接回了原来的会话：准备好之后先告诉它这期间发生了什么。重新开始的话交接说明里都有了。
        let spec = self.state.task(key.task).and_then(|t| t.agent(key.agent));
        if let Some(note) = spec.and_then(|a| a.resume_note.clone()).filter(|_| resumed) {
            self.queue_message(key, note);
        }
        // 重新开始的不再是以前那次会话：记下定好的编号，定不了的等读到新的会话记录再记。
        let stale = self
            .state
            .task(key.task)
            .and_then(|t| t.agent(key.agent))
            .is_some_and(|a| !a.launched || (!resumed && a.session != assigned) || a.resume_note.is_some());
        if stale {
            self.update(|state| {
                if let Some(agent) = state.task_mut(key.task).and_then(|t| t.agent_mut(key.agent)) {
                    agent.launched = true;
                    agent.resume_note = None;
                    if !resumed {
                        agent.session.clone_from(&assigned);
                    }
                }
                Ok(())
            })?;
        }
        // 定好编号的新会话要从头读，不能当成恢复的（恢复的会跳过已有的内容）。
        self.watch_transcript(key, resumed);
        if assigned.is_some() {
            self.share_sessions(key.task);
        }
        Ok(())
    }

    pub fn stop_agent(&mut self, key: AgentKey) {
        self.sessions.remove(&key);
        self.chats.remove(&key);
    }

    /// 有进程在跑（终端或者聊天模式）。
    pub(crate) fn is_running(&mut self, key: AgentKey) -> bool {
        match (self.sessions.get_mut(&key), self.chats.get_mut(&key)) {
            (Some(session), _) => session.poll_exit().is_none(),
            (_, Some(chat)) => !matches!(chat.status(), duolo_chat::Status::Exited { .. }),
            _ => false,
        }
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
        if let Some(chat) = self.chats.get_mut(&key) {
            return chat.status().into();
        }
        match self.sessions.get_mut(&key) {
            Some(session) => session.state().into(),
            None => AgentStatus::NotStarted,
        }
    }

    pub fn summary(&mut self) -> Summary {
        let mut summary = Summary::default();
        let terminal = self.sessions.values_mut().map(|s| AgentStatus::from(s.state()));
        let chat = self.chats.values_mut().map(|c| AgentStatus::from(c.status()));
        for status in terminal.collect::<Vec<_>>().into_iter().chain(chat) {
            match status {
                AgentStatus::NeedsInput => summary.needs_input += 1,
                AgentStatus::Running => summary.running += 1,
                _ => {}
            }
        }
        summary
    }

    pub fn has_live_sessions(&mut self) -> bool {
        self.sessions.values_mut().any(|s| s.poll_exit().is_none())
            || self.chats.values_mut().any(|c| c.poll_exit().is_none())
    }

    /// 任务相对基准分支的改动统计。在后台算，这里马上返回最近一次的结果；第一次还没算完时是 None，
    /// 见 [`Engine::diff_pending`]。
    pub fn diff_stat(&mut self, task: TaskId) -> Option<DiffStat> {
        let t = self.state.task(task)?;
        self.diffs.get(task, &t.worktree, &t.base)
    }

    pub fn diff_pending(&self, task: TaskId) -> bool {
        self.diffs.pending(task)
    }

    /// 结束所有 Agent 和审阅，等它们的进程都消失。
    pub fn shutdown(&mut self) {
        let sessions = self.sessions.drain().map(|(_, session)| session).collect();
        let chats = self.chats.drain().map(|(_, chat)| chat).collect();
        stop_all(sessions, chats, self.reviews.drain().map(|(_, review)| review).collect::<Vec<_>>());
    }

    /// 启动 Agent 的命令。第二项是要等 Agent 准备好后再粘贴过去的第一句话（见下面的说明），一般是 None。
    /// Agent 的钩子交给哪个程序：桌面应用、命令行启动时传自己（`std::env::current_exe`）。
    pub fn set_hook_program(&mut self, program: PathBuf) {
        self.hook_program = Some(program);
    }

    /// Agent 的钩子事件写在哪个目录。
    pub(crate) fn hook_dir(&self, key: AgentKey) -> PathBuf {
        self.paths.hooks().join(format!("{}-{}", key.task, key.agent))
    }

    /// 准备好钩子：清掉上次留下的事件，写好设置文件，返回它的路径。不装钩子时为 None。
    fn prepare_hooks(&self, key: AgentKey, plugin: &Plugin) -> Option<String> {
        let program = self.hook_program.as_ref()?;
        let command = format!("\"{}\" {}", program.display(), duolo_core::hook::HOOK_ARG);
        let settings = plugin.hook_settings(&command, plugin.version().as_deref())?;
        let dir = self.hook_dir(key);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).ok()?;
        let file = dir.with_extension("settings.json");
        std::fs::write(&file, settings).ok()?;
        Some(file.display().to_string())
    }

    /// `assigned` 是新开会话时 Duolo 给它定的会话编号。
    fn session_spec(
        &self,
        key: AgentKey,
        fresh: bool,
        assigned: Option<&str>,
    ) -> Result<(SessionSpec, Option<String>)> {
        let task = self.state.task(key.task).ok_or(UserError::TaskNotFound)?;
        let agent = task.agent(key.agent).context("agent not found")?;
        let plugin = self.agents.get(&agent.kind)?;
        let resume = agent.launched && !fresh;
        let prompt = match agent.role {
            _ if resume => None,
            _ if agent.prompt.is_some() => agent.prompt.clone(),
            AgentRole::Writer => Some(task.prompt.clone()).filter(|p| !p.is_empty()),
            AgentRole::Shell => None,
            AgentRole::Reviewer => Some(self.config.review_prompt_for(
                &task.base,
                &self.fork_of(task),
                &self.reply_language,
                CHANGES_IN_GIT,
            )),
        };
        let session = if resume { agent.session.as_deref() } else { assigned };
        let hooks = self.prepare_hooks(key, plugin);
        let start = Start { role: agent.role, resume, session, hooks: hooks.as_deref(), prompt: prompt.as_deref() };
        let launch = plugin.launch(start);
        let screen = plugin.screen();
        let spec = SessionSpec {
            program: launch.program,
            args: launch.args,
            cwd: task.worktree.clone(),
            env: [("DUOLO_TASK_ID".to_string(), task.id.to_string())]
                .into_iter()
                .chain(hooks.map(|_| (HOOK_DIR_ENV.to_string(), self.hook_dir(key).display().to_string())))
                .chain(launch.env)
                .chain(self.account_vars(key))
                .collect(),
            env_remove: self.agents.env_remove(),
            size: self.sizes.get(&key).copied().unwrap_or(self.pane),
            rules: Rules { approval: screen.approval.clone(), busy: screen.busy.clone() },
            on_output: self.on_output.clone(),
        };
        Ok((spec, launch.paste))
    }

    /// 启动前替用户信任任务的目录，Agent 就不会先停在"信任这个文件夹吗"上。用户添加项目就是信任它。
    /// 记不下来时它照样会问，不影响启动。
    pub(crate) fn trust_folder(&self, key: AgentKey) {
        let (Some(task), Ok(plugin)) = (self.state.task(key.task), self.plugin_of(key)) else { return };
        let project = self.state.project(&task.project).map(|p| p.root.clone());
        let Some(repo) = task.repo.clone().or(project) else { return };
        let _ = plugin.trust(&self.home_of(key), &task.worktree, &repo);
    }

    /// 任务里某个 Agent 的插件。
    pub(crate) fn plugin_of(&self, key: AgentKey) -> Result<&Plugin> {
        let task = self.state.task(key.task).ok_or(UserError::TaskNotFound)?;
        let agent = task.agent(key.agent).context("agent not found")?;
        Ok(self.agents.get(&agent.kind)?)
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// 项目文件夹 `root` 里的 git 仓库，见 [`Engine::repos_of`]。文件夹很大时要找一会儿。
pub fn repos_in(root: &Path) -> Vec<PathBuf> {
    if duolo_git::is_repo_root(root) {
        return vec![root.to_path_buf()];
    }
    let mut repos = duolo_git::find_repos(&[root.to_path_buf()], 200);
    repos.sort();
    repos
}

/// 在 `parent` 下新建文件夹 `name`（父目录不存在时一起建）。已存在时报错。
fn new_folder(parent: &Path, name: &str) -> Result<PathBuf> {
    let name = name.trim();
    if name.is_empty() || name.contains('/') || name.starts_with('.') {
        return Err(UserError::NotAFolder { path: name.to_string() }.into());
    }
    let dir = parent.join(name);
    if dir.exists() {
        return Err(UserError::PathExists { path: dir.display().to_string() }.into());
    }
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// 保存不了没提交的改动：用户能处理的原因原样返回，其他的说明任务没有关闭。
fn save_error(err: anyhow::Error) -> anyhow::Error {
    match err.downcast_ref::<UserError>() {
        Some(UserError::NestedRepo { .. } | UserError::StillChanging { .. }) => err,
        _ => UserError::SaveChangesFailed { detail: format!("{err:#}") }.into(),
    }
}

/// 磁盘上的 worktree 和事件记录用过的最大任务编号。
fn highest_task_id(paths: &Paths) -> u64 {
    let names = |dir: PathBuf| {
        std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned())
    };
    // `wt/<项目>/<编号>-<需求>` 和 `events/<编号>.jsonl`。
    names(paths.worktrees())
        .flat_map(|project| names(paths.worktrees().join(project)))
        .chain(names(paths.events()))
        .filter_map(|name| name.split(['-', '.']).next()?.parse().ok())
        .max()
        .unwrap_or(0)
}

/// 删掉上次没删完的 worktree（在后台）。刚改名的不动：别的进程可能还要把它改回去。
fn remove_leftovers(worktrees: &Path) {
    let dirs = |dir: &Path| std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path());
    let stale: Vec<PathBuf> = dirs(worktrees)
        .flat_map(|project| dirs(&project).collect::<Vec<_>>())
        .filter(|path| {
            let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
            let closed_at = name.strip_prefix(CLOSING_PREFIX).and_then(|rest| rest.split('-').next()?.parse().ok());
            closed_at.is_some_and(|at: u64| now_secs().saturating_sub(at) > 60)
        })
        .collect();
    if !stale.is_empty() {
        std::thread::spawn(move || {
            for dir in stale {
                let _ = std::fs::remove_dir_all(dir);
            }
        });
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

        let id = engine.create_task(&project, "Add Stripe subscriptions\nmonthly and yearly").unwrap();
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
        assert_eq!(engine.start_reviewer(id).unwrap(), reviewer, "reviewer is not added twice");

        // 等待 cat 进入空闲。
        thread::sleep(Duration::from_millis(1_700));
        assert_eq!(engine.agent_status(writer), AgentStatus::Idle);

        engine.close_task(id).unwrap();
        assert!(!task.worktree.exists());
        assert!(engine.state().tasks.is_empty());
        assert_eq!(engine.agent_status(writer), AgentStatus::NotStarted);
    }

    fn git_out(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    #[test]
    fn closing_keeps_uncommitted_work_on_the_branch() {
        let (_dir, mut engine, project) = engine_with_repo();
        let id = engine.create_task(&project, "Export orders as CSV").unwrap();
        let task = engine.state().task(id).unwrap().clone();
        engine.start_agent(task.key(0)).unwrap();
        std::fs::write(task.worktree.join("export.rs"), "fn main() {}\n").unwrap();

        let closed = engine.close_task(id).unwrap();
        assert_eq!(closed, Closed { saved_to: Some(task.branch.clone()), left_at: None });
        assert!(!task.worktree.exists());
        let root = engine.state().project(&project).unwrap().root.clone();
        assert_eq!(git_out(&root, &["show", &format!("{}:export.rs", task.branch)]), "fn main() {}");
        assert_eq!(git_out(&root, &["worktree", "list", "--porcelain"]).matches("worktree ").count(), 1);
        // 文件在后台删除。
        let deadline = Instant::now() + Duration::from_secs(5);
        while std::fs::read_dir(task.worktree.parent().unwrap()).unwrap().next().is_some() {
            assert!(Instant::now() < deadline, "files are not deleted");
            thread::sleep(Duration::from_millis(20));
        }

        // 没有改动的任务不产生提交。
        let id = engine.create_task(&project, "Nothing yet").unwrap();
        assert_eq!(engine.close_task(id).unwrap(), Closed::default());
    }

    #[test]
    fn closing_fails_when_work_cannot_be_saved() {
        let (_dir, mut engine, project) = engine_with_repo();
        let id = engine.create_task(&project, "Vendor a library").unwrap();
        let task = engine.state().task(id).unwrap().clone();
        let nested = task.worktree.join("lib");
        std::fs::create_dir(&nested).unwrap();
        duolo_git::init_repo(&nested).unwrap();
        let err = engine.close_task(id).unwrap_err();
        assert!(matches!(err.downcast_ref(), Some(UserError::NestedRepo { .. })), "{err:#}");
        assert!(nested.exists() && engine.state().task(id).is_some(), "任务和目录都留着");

        // 锁住的 worktree 删不掉登记：目录要挪回原处。
        std::fs::remove_dir_all(&nested).unwrap();
        let root = engine.state().project(&project).unwrap().root.clone();
        git(&root, &["worktree", "lock", task.worktree.to_str().unwrap()]);
        engine.close_task(id).unwrap_err();
        assert!(task.worktree.join(".git").exists() && engine.state().task(id).is_some());

        // 主仓库没了：没法保存，目录留给用户。
        std::fs::rename(&root, root.with_file_name("moved")).unwrap();
        std::fs::write(task.worktree.join("a.txt"), "a").unwrap();
        let closed = engine.close_task(id).unwrap();
        assert_eq!(closed.left_at.as_deref(), Some(task.worktree.as_path()));
        assert!(task.worktree.join("a.txt").exists() && engine.state().task(id).is_none());
    }

    #[test]
    fn two_processes_do_not_overwrite_each_other() {
        let (dir, mut app, project) = engine_with_repo();
        let mut cli = Engine::open(Paths::at(dir.path().join("home"))).unwrap();
        let from_cli = cli.create_task(&project, "From the CLI").unwrap();
        let from_app = app.create_task(&project, "From the app").unwrap();
        assert_ne!(from_cli, from_app);
        cli.sync();
        for engine in [&app, &cli] {
            let titles: Vec<&str> = engine.state().tasks.iter().map(|t| t.title.as_str()).collect();
            assert_eq!(titles, ["From the CLI", "From the app"]);
        }

        // 一边关掉的任务，另一边不能再写回来，进程也要结束。
        let key = app.state().task(from_app).unwrap().key(0);
        app.start_agent(key).unwrap();
        if cfg!(windows) {
            // Windows 上目录被另一个进程的 Agent 占着，关不掉：说明原因，任务留着。
            let err = cli.close_task(from_app).unwrap_err();
            assert!(matches!(err.downcast_ref(), Some(UserError::FolderInUse { .. })), "{err:#}");
            assert!(cli.state().task(from_app).is_some());
            stop_all(app.sessions.drain().map(|(_, session)| session).collect(), Vec::new(), None);
        }
        cli.close_task(from_app).unwrap();
        app.sync();
        assert!(app.state().task(from_app).is_none() && app.session(key).is_none());
        app.rename_task(from_cli, "Renamed").unwrap();
        cli.sync();
        assert_eq!(cli.state().tasks.len(), 1);
        assert_eq!(cli.state().tasks[0].title, "Renamed");
    }

    #[test]
    fn opens_with_damaged_files() {
        let (dir, mut engine, project) = engine_with_repo();
        let id = engine.create_task(&project, "Export orders as CSV").unwrap();
        assert!(engine.take_notices().is_empty());
        drop(engine);
        let home = dir.path().join("home");

        std::fs::write(home.join("state.json"), "").unwrap();
        std::fs::write(home.join("config.toml"), "writer = [").unwrap();
        let mut engine = Engine::open(Paths::at(&home)).unwrap();
        assert_eq!(engine.state().task(id).unwrap().title, "Export orders as CSV");
        let codes: Vec<&str> = engine.take_notices().iter().map(UserError::code).collect();
        assert_eq!(codes, ["configInvalid", "stateRestored"]);
        assert!(engine.take_notices().is_empty(), "只提示一次");
        assert_eq!(std::fs::read_to_string(home.join("config.toml")).unwrap(), "writer = [", "用户的文件不动");
        drop(engine);

        // 连备份也坏了：从空的开始，但不动 worktree 和事件记录，编号也不撞上。
        std::fs::write(home.join("state.json"), "oops").unwrap();
        std::fs::write(home.join("state.json.bak"), "oops").unwrap();
        let mut engine = Engine::open(Paths::at(&home)).unwrap();
        assert!(engine.state().tasks.is_empty());
        assert_eq!(engine.take_notices().last().unwrap().code(), "stateReset");
        assert!(home.join("events/1.jsonl").exists());
        let project = engine.add_project(&dir.path().join("shop")).unwrap();
        assert_eq!(engine.create_task(&project, "Next").unwrap(), TaskId(id.0 + 1));
    }

    #[test]
    fn failed_resume_starts_fresh() {
        let (dir, mut engine, project) = engine_with_repo();
        let id = engine.create_task(&project, "Export orders as CSV").unwrap();
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
    fn failed_resume_after_a_startup_dialog_starts_fresh() {
        let (dir, mut engine, project) = engine_with_repo();
        let id = engine.create_task(&project, "Export orders as CSV").unwrap();
        let key = engine.state().task(id).unwrap().key(0);
        engine.start_agent(key).unwrap();
        drop(engine);

        // 恢复时先问是否信任目录，用户过了一阵才选，选完才发现没有会话可恢复。
        let home = dir.path().join("home");
        std::fs::write(
            home.join("config.toml"),
            "[agents.claude]\ncommand = \"sh\"\nargs = [\"-c\", \"test \\\"$0\\\" = resume && { \
             echo 'Enter to confirm'; read -r x; exit 1; }; cat\"]\nresume_args = [\"resume\"]\n",
        )
        .unwrap();
        let mut engine = Engine::open(Paths::at(&home)).unwrap();
        engine.start_agent(key).unwrap();
        let until = Instant::now() + RESUME_GRACE + Duration::from_millis(500);
        while Instant::now() < until {
            assert!(engine.retry_failed_resumes().is_empty());
            thread::sleep(Duration::from_millis(100));
        }
        engine.write(key, b"\r").unwrap();
        thread::sleep(Duration::from_millis(300));
        assert_eq!(engine.retry_failed_resumes(), vec![key]);
        thread::sleep(Duration::from_millis(300));
        assert!(engine.agent_status(key).is_live());
    }

    #[test]
    fn small_jobs_follow_the_config() {
        let open = |config: &str| {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join("config.toml"), config).unwrap();
            let engine = Engine::open(Paths::at(dir.path())).unwrap();
            engine.namer().map(|c| c.argv)
        };
        // 别的内置 Agent 本机装没装都不影响结果。
        let fake = "[agents.claude]\ncommand = \"git\"\n[agents.codex]\ncommand = \"duolo-no-such-command\"\n\
                    [agents.opencode]\ncommand = \"duolo-no-such-command\"\n[agents.pi]\ncommand = \"duolo-no-such-command\"\n\
                    [agents.grok]\ncommand = \"duolo-no-such-command\"\n";
        assert_eq!(open(fake).unwrap()[0], "git", "没写时用装了的 Agent");
        assert_eq!(open(&format!("namer = [\"x\", \"y\"]\n{fake}")).unwrap(), ["x", "y"]);
        assert_eq!(open(&format!("namer = []\n{fake}")), None, "写成空的就是不用");
        let none = fake.replace("\"git\"", "\"duolo-no-such-command\"");
        assert_eq!(open(&none), None);
    }

    #[test]
    fn background_review_saves_conclusion() {
        let (dir, mut engine, project) = engine_with_repo();
        let id = engine.create_task(&project, "Export orders as CSV").unwrap();
        drop(engine);
        let home = dir.path().join("home");
        // 冒充审阅命令：输出结论，并确认收到了带基准分支的提示词。
        std::fs::write(
            home.join("config.toml"),
            "review_command = [\"sh\", \"-c\", \"echo No issues found; echo \\\"$0\\\" | grep -q main\"]\n",
        )
        .unwrap();
        let mut engine = Engine::open(Paths::at(&home)).unwrap();
        engine.start_review(id).unwrap();
        assert!(engine.review_started_at(id).is_some());
        let deadline = Instant::now() + Duration::from_secs(5);
        while engine.poll_reviews().is_empty() {
            assert!(Instant::now() < deadline, "review timed out");
            thread::sleep(Duration::from_millis(50));
        }
        let review = engine.state().task(id).unwrap().review.clone().unwrap();
        assert!(review.ok, "{}", review.output);
        assert_eq!(review.summary(), "No issues found");
        assert!(engine.review_started_at(id).is_none());
    }

    /// 叫谁来审阅随时选：记在任务上，也成为以后新任务的默认，重开引擎也记得。
    #[cfg(unix)]
    #[test]
    fn review_with_remembers_the_choice() {
        let (dir, mut engine, project) = engine_with_repo();
        let first = engine.create_task(&project, "Export orders as CSV").unwrap();
        drop(engine);
        let home = dir.path().join("home");
        // 换掉了审阅命令时谁都可以审；结论马上出来。
        std::fs::write(
            home.join("config.toml"),
            "reviewer = \"codex\"\nreview_command = [\"sh\", \"-c\", \"echo ok\"]\n",
        )
        .unwrap();
        let mut engine = Engine::open(Paths::at(&home)).unwrap();
        assert_eq!(engine.reviewer_of(first).unwrap().0, "codex");
        engine.review_with(first, AgentKind("claude".into())).unwrap();
        assert!(engine.review_started_at(first).is_some());
        assert_eq!(engine.reviewer_of(first).unwrap().0, "claude");
        let second = engine.create_task(&project, "Import orders").unwrap();
        assert_eq!(engine.reviewer_of(second).unwrap().0, "claude", "新任务先用上次选的");
        engine.cancel_review(first);
        drop(engine);
        let engine = Engine::open(Paths::at(&home)).unwrap();
        assert_eq!(engine.reviewer_of(second).unwrap().0, "claude", "重开也记得");
    }

    /// 审阅在一份副本里：看得到没提交的改动，写了文件也不影响任务的 worktree，结束后副本删掉。
    #[cfg(unix)]
    #[test]
    fn review_runs_in_a_copy() {
        let (dir, mut engine, project) = engine_with_repo();
        let id = engine.create_task(&project, "Export orders as CSV").unwrap();
        let worktree = engine.state().task(id).unwrap().worktree.clone();
        std::fs::write(worktree.join("orders.csv"), "id;total\n").unwrap();
        drop(engine);
        let home = dir.path().join("home");
        // 改动写在 DUOLO_REVIEW.md 里，它不算副本里的改动。
        let script = "grep -q '+id;total' DUOLO_REVIEW.md && test -z \"$(git status --porcelain)\" && \
                      cat orders.csv && echo scribbled > orders.csv && touch junk && pwd";
        std::fs::write(home.join("config.toml"), format!("review_command = [\"sh\", \"-c\", {script:?}]\n")).unwrap();
        let mut engine = Engine::open(Paths::at(&home)).unwrap();
        engine.start_review(id).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while engine.poll_reviews().is_empty() {
            assert!(Instant::now() < deadline, "review timed out");
            thread::sleep(Duration::from_millis(50));
        }
        let review = engine.state().task(id).unwrap().review.clone().unwrap();
        assert!(review.ok, "{}", review.output);
        assert!(review.output.starts_with("id;total"), "{}", review.output);
        // 结论里的路径换回任务的 worktree。
        assert!(review.output.ends_with(&*worktree.to_string_lossy()), "{}", review.output);
        assert_eq!(std::fs::read_to_string(worktree.join("orders.csv")).unwrap(), "id;total\n");
        assert!(!worktree.join("junk").exists());
        let left: Vec<_> = std::fs::read_dir(home.join("review")).unwrap().collect();
        assert!(left.is_empty(), "{left:?}");
    }

    #[test]
    fn cancelled_review_leaves_no_copy() {
        let (dir, mut engine, project) = engine_with_repo();
        let id = engine.create_task(&project, "Export orders as CSV").unwrap();
        drop(engine);
        let home = dir.path().join("home");
        std::fs::write(home.join("config.toml"), "review_command = [\"sh\", \"-c\", \"sleep 30\"]\n").unwrap();
        let mut engine = Engine::open(Paths::at(&home)).unwrap();
        engine.start_review(id).unwrap();
        thread::sleep(Duration::from_millis(300));
        engine.cancel_review(id);
        let deadline = Instant::now() + Duration::from_secs(10);
        while std::fs::read_dir(home.join("review")).is_ok_and(|mut d| d.next().is_some()) {
            assert!(Instant::now() < deadline, "副本没有删掉");
            thread::sleep(Duration::from_millis(50));
        }
        let repo = engine.state().projects[0].root.clone();
        let list = std::process::Command::new("git").arg("-C").arg(&repo).args(["worktree", "list"]).output().unwrap();
        assert!(!String::from_utf8_lossy(&list.stdout).contains("review"), "登记也删了");
    }

    #[test]
    fn state_survives_reopen() {
        let (dir, mut engine, project) = engine_with_repo();
        let id = engine.create_task(&project, "Export orders as CSV").unwrap();
        drop(engine);
        let engine = Engine::open(Paths::at(dir.path().join("home"))).unwrap();
        assert_eq!(engine.state().task(id).unwrap().title, "Export orders as CSV");
        assert!(engine.state().task(id).unwrap().branch == "duolo/1-export-orders-as-csv");
    }

    #[test]
    fn untitled_task_gets_named_by_first_prompt() {
        let (_dir, mut engine, project) = engine_with_repo();
        let id = engine.create_task(&project, "   ").unwrap();
        let task = engine.state().task(id).unwrap().clone();
        assert_eq!(task.title, "New task");
        assert_eq!(task.branch, "duolo/1-new-task");

        engine.name_task(id, "Add Stripe subscriptions", None).unwrap();
        let task = engine.state().task(id).unwrap().clone();
        assert_eq!(task.title, "Add Stripe subscriptions");
        assert_eq!(task.branch, "duolo/1-add-stripe-subscriptions");
        assert_eq!(duolo_git::current_branch(&task.worktree).unwrap(), task.branch);

        engine.name_task(id, "Something else", None).unwrap();
        assert_eq!(
            engine.state().task(id).unwrap().title,
            "Add Stripe subscriptions",
            "only the first prompt names the task"
        );
    }

    #[test]
    fn task_uses_chosen_writer_and_reviewer() {
        let (dir, mut engine, project) = engine_with_repo();
        let opts = TaskOptions {
            writer: Some(AgentKind("codex".into())),
            reviewer: Some(AgentKind("claude".into())),
            ..TaskOptions::default()
        };
        let id = engine.create_task_with(&project, "Export orders as CSV", &opts).unwrap();
        assert_eq!(engine.state().task(id).unwrap().agents[0].kind.0, "codex");
        assert_eq!(engine.reviewer_of(id).unwrap().0, "claude");

        // 审阅用的是审阅 Agent 自己的审阅命令。
        let home = dir.path().join("home");
        std::fs::write(
            home.join("config.toml"),
            "[agents.claude]\ncommand = \"sh\"\nreview_command = [\"sh\", \"-c\", \"echo '[PASS] reviewed by claude'\"]\n",
        )
        .unwrap();
        let mut engine = Engine::open(Paths::at(&home)).unwrap();
        engine.start_review(id).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while engine.poll_reviews().is_empty() {
            assert!(Instant::now() < deadline, "review timed out");
            thread::sleep(Duration::from_millis(50));
        }
        let review = engine.state().task(id).unwrap().review.clone().unwrap();
        assert_eq!(review.summary(), "reviewed by claude");

        let unknown = TaskOptions { writer: Some(AgentKind("aider".into())), ..TaskOptions::default() };
        assert!(engine.create_task_with(&project, "x", &unknown).is_err());
    }

    #[test]
    fn folder_project_with_several_repos() {
        let (dir, mut engine, _) = engine_with_repo();
        // 一个普通文件夹，里面有两个仓库。
        let folder = dir.path().join("work");
        let project = engine.create_project_folder(&folder, "shop").unwrap_or_else(|e| panic!("{e:#}"));
        let root = engine.state().project(&project).unwrap().root.clone();
        assert!(duolo_git::is_repo_root(&root), "新建的文件夹项目本身就是仓库");

        let plain = dir.path().join("plain");
        std::fs::create_dir(&plain).unwrap();
        let project = engine.add_project(&plain).unwrap();
        assert!(engine.repos_of(&project).unwrap().is_empty());
        assert!(engine.create_task(&project, "x").is_err(), "没有仓库时不能新建任务");
        let api = engine.add_repo(&project, Some("api")).unwrap();
        let web = engine.add_repo(&project, Some("web")).unwrap();
        assert_eq!(engine.repos_of(&project).unwrap(), vec![api.clone(), web.clone()]);

        let opts =
            TaskOptions { repo: Some(web.clone()), branch: Some("feat/csv-export".into()), ..TaskOptions::default() };
        let id = engine.create_task_with(&project, "Export orders as CSV", &opts).unwrap();
        let task = engine.state().task(id).unwrap().clone();
        assert_eq!(task.branch, "feat/csv-export");
        assert_eq!(task.repo.as_deref(), Some(web.as_path()));
        assert!(task.worktree.ends_with("plain/1-csv-export"), "{}", task.worktree.display());

        // 同名分支、非法分支名都直接报错。
        let err = engine.create_task_with(&project, "again", &opts).unwrap_err();
        assert!(matches!(err.downcast_ref(), Some(UserError::BranchExists { .. })));
        let bad = TaskOptions { branch: Some("bad name".into()), ..opts.clone() };
        let err = engine.create_task_with(&project, "again", &bad).unwrap_err();
        assert!(matches!(err.downcast_ref(), Some(UserError::InvalidBranch { .. })));

        engine.close_task(id).unwrap();
        assert!(!task.worktree.exists());
        assert!(duolo_git::branch_exists(&web, "feat/csv-export"), "关闭任务保留分支");
    }

    #[test]
    fn blank_task_takes_suggested_branch() {
        let (_dir, mut engine, project) = engine_with_repo();
        let id = engine.create_task(&project, "").unwrap();
        engine.name_task(id, "Export orders", Some("feat/order-export")).unwrap();
        assert_eq!(engine.state().task(id).unwrap().branch, "feat/order-export");

        // 后来才起好的名字：只换自动起的分支名，自己定过的不动。
        let id = engine.create_task(&project, "Import orders").unwrap();
        engine.rename_branch(id, "feat/order-import").unwrap();
        assert_eq!(engine.state().task(id).unwrap().branch, "feat/order-import");
        engine.rename_branch(id, "feat/other").unwrap();
        assert_eq!(engine.state().task(id).unwrap().branch, "feat/order-import");
    }

    #[test]
    fn task_from_other_branch_and_rename() {
        let (_dir, mut engine, project) = engine_with_repo();
        let root = engine.state().project(&project).unwrap().root.clone();
        let out = std::process::Command::new("git").arg("-C").arg(&root).args(["branch", "dev"]).output().unwrap();
        assert!(out.status.success());
        let opts = TaskOptions { base: Some("dev".into()), ..TaskOptions::default() };
        let id = engine.create_task_with(&project, "Export orders as CSV", &opts).unwrap();
        assert_eq!(engine.state().task(id).unwrap().base, "dev");

        engine.rename_task(id, "  Export CSV and Excel ").unwrap();
        let task = engine.state().task(id).unwrap();
        assert_eq!(task.title, "Export CSV and Excel");
        assert_eq!(task.branch, "duolo/1-export-orders-as-csv", "renaming keeps the branch");
    }
}
