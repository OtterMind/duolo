//! 界面状态和所有用户操作。这里不涉及绘制，绘制见 `ui`。

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use duolo_core::{t, tf, AgentKey, ProjectId, Task, TaskId};
use duolo_engine::{check_environment, Check, Engine};
use duolo_pty::Size;
use duolo_voice::OrganizeContext;
use ratatui::layout::Rect;

use crate::line_input::LineInput;
use crate::voice::{Delivery, Voice, VoiceTarget};

/// 提示消息显示多久。
const FLASH_TTL: Duration = Duration::from_secs(4);
/// 整理事件流的间隔，和桌面应用的快照间隔一样。
const EVENTS_INTERVAL: Duration = Duration::from_millis(250);

/// 需要用户确认或阅读的弹出状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    ConfirmClose(TaskId),
    ConfirmQuit,
    Help,
}

/// 键盘焦点在哪一侧。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    /// 右侧：首页输入框，或当前 Agent。
    Main,
    /// 左侧边栏：方向键选择，回车打开。
    Sidebar,
}

/// 侧边栏里可以选中的一行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    NewTask,
    Task(TaskId),
    Agent(AgentKey),
    AddProject,
}

/// 首页输入框：平时用来新建任务，`adding_project` 时用来添加项目。
#[derive(Debug, Default)]
pub struct Home {
    pub input: LineInput,
    pub project: usize,
    pub adding_project: bool,
}

#[derive(Debug)]
pub struct Flash {
    pub text: String,
    pub is_error: bool,
    at: Instant,
}

#[derive(Debug)]
pub struct App {
    pub engine: Engine,
    pub mode: Mode,
    pub focus: Focus,
    /// 侧边栏获得焦点时的光标。
    pub cursor: Item,
    pub voice: Voice,
    pub home: Home,
    pub sidebar_visible: bool,
    /// 上一帧侧边栏每一行的位置，用于鼠标点击。
    pub sidebar_hits: Vec<(u16, Item)>,
    pub sidebar_area: Rect,
    pub pane_area: Rect,
    selected_task: Option<TaskId>,
    last_task: Option<TaskId>,
    selected_agent: u32,
    flash: Option<Flash>,
    quit: bool,
    env: Arc<Mutex<Option<Vec<Check>>>>,
    /// 上次整理事件流的时间（识别状态、给刚启动的 Agent 发排队的消息）。
    last_events: Instant,
}

impl App {
    pub fn new(mut engine: Engine, cwd: &Path) -> Self {
        // 在 git 仓库里启动时，自动把它加入项目。不在仓库里不算错误。
        let current = engine.add_project(cwd).ok();
        let project =
            current.as_ref().and_then(|id| engine.state().projects.iter().position(|p| &p.id == id)).unwrap_or(0);

        // 环境检查要调用各个命令，放到后台，不拖慢启动。
        let env = Arc::new(Mutex::new(None));
        {
            let env = Arc::clone(&env);
            let config = engine.config().clone();
            thread::spawn(move || {
                let checks = check_environment(&config);
                *env.lock().unwrap_or_else(|p| p.into_inner()) = Some(checks);
            });
        }

        let mut app = Self {
            engine,
            mode: Mode::Normal,
            focus: Focus::Main,
            cursor: Item::NewTask,
            voice: Voice::default(),
            home: Home { project, ..Home::default() },
            sidebar_visible: true,
            sidebar_hits: Vec::new(),
            sidebar_area: Rect::default(),
            pane_area: Rect::default(),
            selected_task: None,
            last_task: None,
            selected_agent: 0,
            flash: None,
            quit: false,
            env,
            last_events: Instant::now(),
        };
        app.start_pending_agents();
        // 有任务时打开第一个（优先等你确认的），没有任务时停在首页。
        let first = app.ordered_tasks().into_iter().next();
        if let Some(id) = first {
            app.open_task(id);
        }
        let notices: Vec<String> = app.engine.take_notices().iter().map(|n| n.localized()).collect();
        if !notices.is_empty() {
            app.error(notices.join(" "));
        }
        app
    }

    // ---- 查询 ----

    pub fn should_quit(&self) -> bool {
        self.quit
    }

    pub fn is_home(&self) -> bool {
        self.selected_task().is_none()
    }

    pub fn has_previous_task(&self) -> bool {
        self.last_task.is_some_and(|id| self.engine.state().task(id).is_some())
    }

    pub fn env_checks(&self) -> Option<Vec<Check>> {
        self.env.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    pub fn selected_task(&self) -> Option<&Task> {
        self.selected_task.and_then(|id| self.engine.state().task(id))
    }

    pub fn selected_key(&self) -> Option<AgentKey> {
        let task = self.selected_task()?;
        task.agent(self.selected_agent).map(|a| task.key(a.id))
    }

    /// 当前 Agent 是否有在运行的进程（决定按键是否转发给它）。
    pub fn focused_live_key(&mut self) -> Option<AgentKey> {
        let key = self.selected_key()?;
        self.engine.agent_status(key).is_live().then_some(key)
    }

    pub fn flash(&self) -> Option<&Flash> {
        self.flash.as_ref().filter(|f| f.at.elapsed() < FLASH_TTL)
    }

    /// 侧边栏顺序下的所有任务。
    pub fn ordered_tasks(&self) -> Vec<TaskId> {
        let state = self.engine.state();
        state.projects.iter().flat_map(|p| state.tasks_of(&p.id).map(|t| t.id)).collect()
    }

    /// 侧边栏里所有可选中的行，从上到下。
    pub fn items(&self) -> Vec<Item> {
        let state = self.engine.state();
        let mut items = vec![Item::NewTask];
        for project in &state.projects {
            for task in state.tasks_of(&project.id) {
                items.push(Item::Task(task.id));
                items.extend(task.agents.iter().map(|a| Item::Agent(task.key(a.id))));
            }
        }
        items.push(Item::AddProject);
        items
    }

    /// 右侧当前显示的内容对应侧边栏的哪一行。
    pub fn current_item(&self) -> Item {
        match self.selected_key() {
            Some(key) => Item::Agent(key),
            None if self.home.adding_project => Item::AddProject,
            None => Item::NewTask,
        }
    }

    // ---- 提示 ----

    pub fn info(&mut self, text: impl Into<String>) {
        self.flash = Some(Flash { text: text.into(), is_error: false, at: Instant::now() });
    }

    pub fn error(&mut self, err: impl std::fmt::Display) {
        self.flash = Some(Flash { text: err.to_string(), is_error: true, at: Instant::now() });
    }

    // ---- 焦点和选择 ----

    pub fn focus_sidebar(&mut self) {
        self.sidebar_visible = true;
        self.focus = Focus::Sidebar;
        self.cursor = self.current_item();
    }

    pub fn focus_main(&mut self) {
        self.focus = Focus::Main;
    }

    /// 在侧边栏里上下移动；移到任务或 Agent 上时右侧同步预览。
    pub fn move_cursor(&mut self, delta: isize) {
        let items = self.items();
        let current = items.iter().position(|i| *i == self.cursor).unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, items.len() as isize - 1) as usize;
        self.cursor = items[next];
        self.preview(self.cursor);
    }

    fn preview(&mut self, item: Item) {
        match item {
            Item::NewTask => self.show_home(false),
            Item::AddProject => self.show_home(true),
            Item::Task(id) => self.select_task(id),
            Item::Agent(key) => {
                self.select_task(key.task);
                self.selected_agent = key.agent;
            }
        }
    }

    /// 打开侧边栏里的一行：预览它，把焦点交给右侧，需要时启动 Agent。
    pub fn activate(&mut self, item: Item) {
        self.cursor = item;
        self.preview(item);
        self.focus_main();
        if matches!(item, Item::Task(_) | Item::Agent(_)) {
            self.ensure_selected_agent_running();
        }
    }

    pub fn open_task(&mut self, id: TaskId) {
        self.activate(Item::Task(id));
    }

    fn select_task(&mut self, id: TaskId) {
        if self.selected_task != Some(id) {
            self.selected_task = Some(id);
            self.selected_agent = 0;
        }
        self.home.adding_project = false;
    }

    pub fn select_nth_task(&mut self, n: usize) {
        if let Some(id) = self.ordered_tasks().get(n).copied() {
            self.open_task(id);
        }
    }

    // ---- 首页 ----

    fn show_home(&mut self, adding_project: bool) {
        if let Some(id) = self.selected_task.take() {
            self.last_task = Some(id);
        }
        let adding = adding_project || self.engine.state().projects.is_empty();
        if adding != self.home.adding_project {
            self.home.input = LineInput::default();
        }
        self.home.adding_project = adding;
    }

    /// 回到首页输入框新建任务。
    pub fn new_task(&mut self) {
        self.activate(Item::NewTask);
    }

    pub fn add_project(&mut self) {
        self.activate(Item::AddProject);
    }

    /// 首页按 Esc：添加项目时退回新建任务，否则回到上一个任务。
    pub fn leave_home(&mut self) {
        if self.home.adding_project && !self.engine.state().projects.is_empty() {
            self.show_home(false);
        } else if let Some(id) = self.last_task.filter(|id| self.engine.state().task(*id).is_some()) {
            self.open_task(id);
        }
    }

    pub fn cycle_home_project(&mut self) {
        let count = self.engine.state().projects.len();
        if count > 1 {
            self.home.project = (self.home.project + 1) % count;
        }
    }

    pub fn home_project_name(&self) -> Option<&str> {
        self.engine.state().projects.get(self.home.project).map(|p| p.name.as_str())
    }

    pub fn submit_home(&mut self) {
        let text = self.home.input.text().trim().to_string();
        if text.is_empty() {
            return;
        }
        if self.home.adding_project {
            match self.engine.add_project(Path::new(&expand_home(&text))) {
                Ok(id) => {
                    self.home.project = self.engine.state().projects.iter().position(|p| p.id == id).unwrap_or(0);
                    self.home.adding_project = false;
                    self.home.input = LineInput::default();
                    self.info(tf("tui.projectAdded", &[("id", &id)]));
                }
                Err(err) => self.error(duolo_core::error::describe(&err)),
            }
            return;
        }
        let Some(project) = self.engine.state().projects.get(self.home.project).map(|p| p.id.clone()) else {
            return;
        };
        match self.create_and_start(&project, &text) {
            Ok(id) => {
                self.home.input = LineInput::default();
                self.open_task(id);
            }
            Err(err) => self.error(duolo_core::error::describe(&err)),
        }
    }

    fn create_and_start(&mut self, project: &ProjectId, prompt: &str) -> anyhow::Result<TaskId> {
        let id = self.engine.create_task(project, prompt)?;
        let key = self.engine.state().task(id).map(|t| t.key(0)).expect("task was just created");
        self.engine.start_agent(key)?;
        Ok(id)
    }

    // ---- 任务和 Agent ----

    /// 选中的 Agent 没在运行时启动它（第一次启动或恢复会话）。
    pub fn ensure_selected_agent_running(&mut self) {
        let Some(key) = self.selected_key() else { return };
        if self.engine.agent_status(key).is_live() {
            return;
        }
        if let Err(err) = self.engine.start_agent(key) {
            self.error(duolo_core::error::describe(&err));
        }
    }

    /// 侧边栏光标所在的任务；不在任务上时用右侧当前的任务。
    pub fn task_under_cursor(&self) -> Option<TaskId> {
        match (self.focus, self.cursor) {
            (Focus::Sidebar, Item::Task(id)) => Some(id),
            (Focus::Sidebar, Item::Agent(key)) => Some(key.task),
            _ => self.selected_task().map(|t| t.id),
        }
    }

    pub fn start_reviewer(&mut self) {
        let Some(task) = self.task_under_cursor() else {
            self.info(t("tui.selectTaskFirst"));
            return;
        };
        match self.engine.start_reviewer(task) {
            Ok(key) => {
                self.activate(Item::Agent(key));
                self.info(t("tui.reviewStarted"));
            }
            Err(err) => self.error(duolo_core::error::describe(&err)),
        }
    }

    pub fn request_close(&mut self) {
        match self.task_under_cursor() {
            Some(id) => self.mode = Mode::ConfirmClose(id),
            None => self.info(t("tui.selectTaskFirst")),
        }
    }

    pub fn close_task(&mut self, id: TaskId) {
        self.mode = Mode::Normal;
        let tasks = self.ordered_tasks();
        let next = tasks
            .iter()
            .position(|t| *t == id)
            .and_then(|i| tasks.get(i + 1).or_else(|| i.checked_sub(1).and_then(|j| tasks.get(j))).copied());
        match self.engine.close_task(id) {
            Ok(closed) => {
                if self.last_task == Some(id) {
                    self.last_task = None;
                }
                match next {
                    Some(next) => self.open_task(next),
                    None => self.new_task(),
                }
                self.info(match (closed.saved_to, closed.left_at) {
                    (Some(branch), _) => tf("task.savedTo", &[("branch", &branch)]),
                    (None, Some(path)) => tf("task.leftAt", &[("path", &path.display())]),
                    (None, None) => t("tui.taskClosed"),
                });
            }
            Err(err) => self.error(duolo_core::error::describe(&err)),
        }
    }

    pub fn request_quit(&mut self) {
        if self.engine.has_live_sessions() {
            self.mode = Mode::ConfirmQuit;
        } else {
            self.quit = true;
        }
    }

    pub fn quit_now(&mut self) {
        self.quit = true;
    }

    pub fn shutdown(&mut self) {
        self.voice.cancel();
        self.engine.shutdown();
    }

    /// 通过 `duolo new` 创建、还没启动过的 Agent，在打开工作台时自动启动。
    fn start_pending_agents(&mut self) {
        let pending: Vec<AgentKey> = self
            .engine
            .state()
            .tasks
            .iter()
            .flat_map(|t| t.agents.iter().filter(|a| !a.launched).map(move |a| t.key(a.id)))
            .collect();
        for key in pending {
            if let Err(err) = self.engine.start_agent(key) {
                self.error(duolo_core::error::describe(&err));
            }
        }
    }

    /// 窗格尺寸变化时同步给所有会话。
    pub fn set_pane_size(&mut self, size: Size) {
        self.engine.set_pane_size(size);
    }

    /// 查看历史输出：正数向上翻。
    pub fn scroll(&mut self, lines: isize) {
        let Some(key) = self.selected_key() else { return };
        let Some(session) = self.engine.session(key) else { return };
        let offset = session.scrollback() as isize;
        session.set_scrollback((offset + lines).max(0) as usize);
    }

    // ---- 语音 ----

    pub fn toggle_voice(&mut self, target: VoiceTarget) {
        let cfg = self.engine.config().voice.clone();
        if self.voice.recording_elapsed().is_some() {
            let ctx = self.organize_context();
            self.voice.stop(&cfg, ctx);
            return;
        }
        if !cfg.is_configured() {
            self.error(t("error.voiceNotConfigured"));
            return;
        }
        if let Err(err) = self.voice.start(&cfg, target) {
            self.error(duolo_core::error::describe(&err));
        }
    }

    fn organize_context(&self) -> OrganizeContext {
        let task = self.selected_task();
        OrganizeContext {
            project: task
                .and_then(|t| self.engine.state().project(&t.project))
                .map(|p| p.name.clone())
                .or_else(|| self.home_project_name().map(Into::into)),
            task: task.map(|t| t.title.clone()),
        }
    }

    /// 确认语音文字：写进 Agent 的输入行，`submit` 为真时同时回车发送。
    pub fn commit_voice(&mut self, key: AgentKey, submit: bool) {
        let Some(text) = self.voice.take_pending() else { return };
        let Some(session) = self.engine.session_mut(key) else { return };
        let result = session.paste(&text).and_then(|()| if submit { session.write(b"\r") } else { Ok(()) });
        if let Err(err) = result {
            self.error(duolo_core::error::describe(&err));
        }
    }

    /// 处理后台任务的结果。返回是否需要重绘。
    pub fn poll_background(&mut self) -> bool {
        if self.last_events.elapsed() >= EVENTS_INTERVAL {
            self.last_events = Instant::now();
            self.engine.sync();
            self.engine.poll_events();
        }
        if !self.engine.retry_failed_resumes().is_empty() {
            return true;
        }
        let Some(delivery) = self.voice.poll() else {
            return self.voice.recording_elapsed().is_some();
        };
        match delivery {
            Delivery::HomeText(text) => self.home.input.insert_str(&text),
            Delivery::Error(err) => self.error(tf("tui.voiceError", &[("error", &err)])),
        }
        true
    }
}

fn expand_home(path: &str) -> String {
    match path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
        Some(rest) => duolo_core::process::home_dir().join(rest).to_string_lossy().into_owned(),
        None => path.to_string(),
    }
}
