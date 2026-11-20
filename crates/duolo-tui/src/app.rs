//! 界面状态和所有用户操作。这里不涉及绘制，绘制见 `ui`。

use std::path::Path;
use std::time::{Duration, Instant};

use duolo_core::{AgentKey, ProjectId, Task, TaskId};
use duolo_engine::Engine;
use duolo_pty::Size;
use duolo_voice::OrganizeContext;

use crate::line_input::LineInput;
use crate::voice::{Delivery, Voice, VoiceTarget};

/// 提示消息显示多久。
const FLASH_TTL: Duration = Duration::from_secs(4);

/// 当前的交互模式。
#[derive(Debug)]
pub enum Mode {
    /// 按键交给当前 Agent（没有 Agent 时由 Duolo 处理）。
    Normal,
    /// 按了 `Ctrl+]`，等待下一个键。
    Prefix,
    NewTask {
        input: LineInput,
        project: usize,
    },
    AddProject {
        input: LineInput,
    },
    ConfirmClose(TaskId),
    ConfirmQuit,
    Help,
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
    pub voice: Voice,
    pub sidebar_visible: bool,
    selected_task: Option<TaskId>,
    selected_agent: u32,
    flash: Option<Flash>,
    quit: bool,
}

impl App {
    pub fn new(mut engine: Engine, cwd: &Path) -> Self {
        // 在 git 仓库里启动时，自动把它加入项目。不在仓库里不算错误。
        let current = engine.add_project(cwd).ok();
        let selected_task = current
            .as_ref()
            .and_then(|p| engine.state().tasks_of(p).next().map(|t| t.id))
            .or_else(|| engine.state().tasks.first().map(|t| t.id));
        let mut app = Self {
            engine,
            mode: Mode::Normal,
            voice: Voice::default(),
            sidebar_visible: true,
            selected_task,
            selected_agent: 0,
            flash: None,
            quit: false,
        };
        app.start_pending_agents();
        app
    }

    // ---- 查询 ----

    pub fn should_quit(&self) -> bool {
        self.quit
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

    /// 新建任务时默认的项目：当前任务所在的项目，否则第一个。
    fn default_project_index(&self) -> usize {
        let projects = &self.engine.state().projects;
        self.selected_task().and_then(|t| projects.iter().position(|p| p.id == t.project)).unwrap_or(0)
    }

    // ---- 提示 ----

    pub fn info(&mut self, text: impl Into<String>) {
        self.flash = Some(Flash { text: text.into(), is_error: false, at: Instant::now() });
    }

    pub fn error(&mut self, err: impl std::fmt::Display) {
        self.flash = Some(Flash { text: err.to_string(), is_error: true, at: Instant::now() });
    }

    // ---- 选择 ----

    pub fn select_task(&mut self, id: TaskId) {
        if self.selected_task != Some(id) {
            self.selected_task = Some(id);
            self.selected_agent = 0;
        }
    }

    pub fn move_task(&mut self, delta: isize) {
        let tasks = self.ordered_tasks();
        if tasks.is_empty() {
            return;
        }
        let current = self.selected_task.and_then(|id| tasks.iter().position(|t| *t == id)).unwrap_or(0);
        let len = tasks.len() as isize;
        let next = (current as isize + delta).rem_euclid(len) as usize;
        self.select_task(tasks[next]);
    }

    pub fn select_nth_task(&mut self, n: usize) {
        if let Some(id) = self.ordered_tasks().get(n) {
            self.select_task(*id);
        }
    }

    pub fn move_agent(&mut self, delta: isize) {
        let Some(task) = self.selected_task() else { return };
        let ids: Vec<u32> = task.agents.iter().map(|a| a.id).collect();
        let current = ids.iter().position(|id| *id == self.selected_agent).unwrap_or(0);
        let len = ids.len() as isize;
        self.selected_agent = ids[(current as isize + delta).rem_euclid(len) as usize];
    }

    // ---- 任务和 Agent ----

    pub fn open_new_task(&mut self) {
        if self.engine.state().projects.is_empty() {
            self.mode = Mode::AddProject { input: LineInput::default() };
            self.info("先添加一个项目（git 仓库路径）");
            return;
        }
        self.mode = Mode::NewTask { input: LineInput::default(), project: self.default_project_index() };
    }

    pub fn submit_new_task(&mut self, prompt: String, project: usize) {
        let Some(project_id) = self.engine.state().projects.get(project).map(|p| p.id.clone()) else {
            return;
        };
        self.mode = Mode::Normal;
        match self.create_and_start(&project_id, &prompt) {
            Ok(id) => self.select_task(id),
            Err(err) => self.error(format!("{err:#}")),
        }
    }

    fn create_and_start(&mut self, project: &ProjectId, prompt: &str) -> anyhow::Result<TaskId> {
        let id = self.engine.create_task(project, prompt)?;
        let key = self.engine.state().task(id).map(|t| t.key(0)).expect("刚创建的任务");
        self.engine.start_agent(key)?;
        Ok(id)
    }

    pub fn submit_add_project(&mut self, path: String) {
        let path = expand_home(path.trim());
        self.mode = Mode::Normal;
        match self.engine.add_project(Path::new(&path)) {
            Ok(id) => self.info(format!("已添加项目 {id}")),
            Err(err) => self.error(format!("{err:#}")),
        }
    }

    pub fn start_selected_agent(&mut self) {
        if let Some(key) = self.selected_key() {
            if let Err(err) = self.engine.start_agent(key) {
                self.error(format!("{err:#}"));
            }
        }
    }

    pub fn start_reviewer(&mut self) {
        let Some(task) = self.selected_task().map(|t| t.id) else { return };
        match self.engine.start_reviewer(task) {
            Ok(key) => {
                self.selected_agent = key.agent;
                self.info("已启动审阅（只读）");
            }
            Err(err) => self.error(format!("{err:#}")),
        }
    }

    pub fn close_task(&mut self, id: TaskId) {
        self.mode = Mode::Normal;
        let next = {
            let tasks = self.ordered_tasks();
            let pos = tasks.iter().position(|t| *t == id);
            pos.and_then(|i| tasks.get(i + 1).or_else(|| i.checked_sub(1).and_then(|j| tasks.get(j)))).copied()
        };
        match self.engine.close_task(id) {
            Ok(()) => {
                self.selected_task = next;
                self.selected_agent = 0;
                self.info("任务已关闭，分支保留");
            }
            Err(err) => self.error(format!("{err:#}")),
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
                self.error(format!("{err:#}"));
            }
        }
    }

    /// 窗格尺寸变化时同步给所有会话。
    pub fn set_pane_size(&mut self, size: Size) {
        self.engine.set_pane_size(size);
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
            self.error("还没有配置语音命令：在 ~/.duolo/config.toml 里设置 voice.command，见 README");
            return;
        }
        if let Err(err) = self.voice.start(&cfg, target) {
            self.error(format!("{err:#}"));
        }
    }

    fn organize_context(&self) -> OrganizeContext {
        let task = self.selected_task();
        OrganizeContext {
            project: task.and_then(|t| self.engine.state().project(&t.project)).map(|p| p.name.clone()),
            task: task.map(|t| t.title.clone()),
        }
    }

    /// 确认语音文字：写进 Agent 的输入行，`submit` 为真时同时回车发送。
    pub fn commit_voice(&mut self, key: AgentKey, submit: bool) {
        let Some(text) = self.voice.take_pending() else { return };
        let Some(session) = self.engine.session_mut(key) else { return };
        let result = session.paste(&text).and_then(|()| if submit { session.write(b"\r") } else { Ok(()) });
        if let Err(err) = result {
            self.error(format!("{err:#}"));
        }
    }

    /// 处理后台任务的结果。返回是否需要重绘。
    pub fn poll_background(&mut self) -> bool {
        let Some(delivery) = self.voice.poll() else {
            return self.voice.recording_elapsed().is_some();
        };
        match delivery {
            Delivery::NewTaskText(text) => {
                if let Mode::NewTask { input, .. } = &mut self.mode {
                    input.insert_str(&text);
                }
            }
            Delivery::Error(err) => self.error(format!("语音：{err}")),
        }
        true
    }
}

fn expand_home(path: &str) -> String {
    match (path.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => Path::new(&home).join(rest).to_string_lossy().into_owned(),
        _ => path.to_string(),
    }
}
