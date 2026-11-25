//! 界面调用的命令。每个命令只做参数转换，具体逻辑都在 `duolo-engine` 里。

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use duolo_core::{AgentKey, AgentKind, Command, CommandResult, ProjectId, TaskEvent, TaskId};
use duolo_core::{Settings, Surface, UserError, VoiceConfig};
use duolo_engine::{check_environment, ops, Size, TaskOptions};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Manager, State};

use crate::view::{self, CheckView, DiffView, PluginView, Snapshot};
use crate::{AppState, Res};

/// 在后台线程里做费时间的事。
async fn blocking<T: Send + 'static>(
    app: AppHandle,
    work: impl FnOnce(&AppState) -> anyhow::Result<T> + Send + 'static,
) -> Res<T> {
    Ok(tauri::async_runtime::spawn_blocking(move || work(&app.state::<AppState>())).await??)
}

fn key(task: u64, agent: u32) -> AgentKey {
    AgentKey { task: TaskId(task), agent }
}

#[tauri::command]
pub fn snapshot(state: State<'_, AppState>) -> Snapshot {
    view::snapshot(&mut state.engine())
}

/// 启动时发现的问题（数据文件损坏后恢复了、配置写错了），界面提示一次。
#[tauri::command]
pub fn startup_notices(state: State<'_, AppState>) -> Vec<crate::Error> {
    state.engine().take_notices().into_iter().map(Into::into).collect()
}

/// 检查 git、Agent CLI 和语音。会运行几个 `--version`，放到后台线程执行。
#[tauri::command]
pub async fn environment(state: State<'_, AppState>) -> Res<Vec<CheckView>> {
    let (mut config, agents) = {
        let engine = state.engine();
        (engine.config().clone(), engine.agents().clone())
    };
    config.voice = voice_config(&state);
    let checks = tauri::async_runtime::spawn_blocking(move || check_environment(&config, &agents)).await?;
    Ok(checks.into_iter().map(Into::into).collect())
}

/// 启动时或通过 `duolo <目录>` 请求打开的项目，取一次后清空。
#[tauri::command]
pub fn take_open_request(state: State<'_, AppState>) -> Option<String> {
    state.open_request.lock().unwrap_or_else(|p| p.into_inner()).take()
}

#[tauri::command]
pub async fn add_project(state: State<'_, AppState>, path: String) -> Res<String> {
    Ok(state.engine().add_project(&PathBuf::from(path))?.0)
}

/// 新建任务。没传的选项按配置默认：`repo` 为项目里的第一个仓库，`base` 为仓库的当前分支，
/// `branch` 为 `duolo/<编号>-<需求>`，`writer` / `reviewer` 为配置里的 Agent。
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn create_task(
    app: AppHandle,
    project: String,
    prompt: String,
    repo: Option<String>,
    base: Option<String>,
    branch: Option<String>,
    writer: Option<String>,
    reviewer: Option<String>,
) -> Res<u64> {
    let opts = TaskOptions {
        repo: repo.map(PathBuf::from),
        base,
        branch,
        writer: writer.map(AgentKind),
        reviewer: reviewer.map(AgentKind),
    };
    blocking(app, move |state| ops::create_task(&state.engine, &ProjectId(project), &prompt, &opts))
        .await
        .map(|id| id.0)
}

/// 任务第 `after` 条之后的事件（`after` 为 0 时是全部）。新事件通过 `task-events` 推送。
#[tauri::command]
pub fn task_events(state: State<'_, AppState>, task: u64, after: u64) -> Res<Vec<TaskEvent>> {
    Ok(state.engine().events_since(TaskId(task), after)?)
}

/// 执行一条命令（和手机端发来的命令同一套格式）。
#[tauri::command]
pub async fn execute(app: AppHandle, command: Command) -> Res<CommandResult> {
    blocking(app, move |state| Ok(ops::execute(&state.engine, command))).await
}

/// 可用的 Agent 插件：名字和图标。
#[tauri::command]
pub fn agent_plugins(state: State<'_, AppState>) -> Vec<PluginView> {
    state.engine().agents().iter().map(Into::into).collect()
}

/// 配置里默认写代码和审阅的 Agent。
#[tauri::command]
pub fn agent_defaults(state: State<'_, AppState>) -> AgentDefaults {
    let engine = state.engine();
    let config = engine.config();
    AgentDefaults { writer: config.writer.0.clone(), reviewer: config.reviewer.0.clone() }
}

#[derive(Debug, serde::Serialize)]
pub struct AgentDefaults {
    writer: String,
    reviewer: String,
}

/// 根据需求让 AI 给新分支起名；没装、超时或结果不能用时返回空，界面用默认名字。
#[tauri::command]
pub async fn suggest_branch(state: State<'_, AppState>, prompt: String) -> Res<Option<String>> {
    let Some(namer) = state.engine().namer() else { return Ok(None) };
    Ok(tauri::async_runtime::spawn_blocking(move || duolo_engine::suggest_branch(&namer, &prompt)).await?)
}

/// AI 起好名字后，换掉任务自动起的分支名。
#[tauri::command]
pub async fn rename_branch(state: State<'_, AppState>, task: u64, branch: String) -> Res<()> {
    Ok(state.engine().rename_branch(TaskId(task), &branch)?)
}

#[tauri::command]
pub async fn rename_task(state: State<'_, AppState>, task: u64, title: String) -> Res<()> {
    Ok(state.engine().rename_task(TaskId(task), &title)?)
}

/// 项目里的 git 仓库。`name` 是相对项目文件夹的路径，项目本身就是仓库时为空。
#[tauri::command]
pub async fn project_repos(state: State<'_, AppState>, project: String) -> Res<Vec<RepoView>> {
    let root = state
        .engine()
        .state()
        .project(&ProjectId(project))
        .map(|p| p.root.clone())
        .ok_or(UserError::ProjectNotFound)?;
    let folder = root.clone();
    let repos = tauri::async_runtime::spawn_blocking(move || duolo_engine::repos_in(&folder)).await?;
    Ok(repos
        .into_iter()
        .map(|path| RepoView {
            name: path.strip_prefix(&root).map(|p| p.display().to_string()).unwrap_or_default(),
            path: path.display().to_string(),
        })
        .collect())
}

/// 在项目里建一个 git 仓库：`name` 为空时把项目文件夹本身初始化为仓库，否则新建子文件夹。返回仓库路径。
#[tauri::command]
pub async fn add_repo(app: AppHandle, project: String, name: Option<String>) -> Res<String> {
    blocking(app, move |state| ops::add_repo(&state.engine, &ProjectId(project), name.as_deref()))
        .await
        .map(|dir| dir.display().to_string())
}

/// 仓库的本地分支，当前分支在最前面。
#[tauri::command]
pub async fn branches(repo: String) -> Res<Vec<String>> {
    Ok(tauri::async_runtime::spawn_blocking(move || duolo_git::branches(&PathBuf::from(repo))).await??)
}

/// 这台 Mac 上常见代码目录里、还没有添加为项目的 git 仓库。
#[tauri::command]
pub async fn discover_repos(state: State<'_, AppState>) -> Res<Vec<RepoView>> {
    let added: Vec<_> = state.engine().state().projects.iter().filter_map(|p| duolo_git::file_id(&p.root)).collect();
    let roots = code_dirs();
    let found = tauri::async_runtime::spawn_blocking(move || duolo_git::find_repos(&roots, 40)).await?;
    Ok(found
        .into_iter()
        .filter(|path| duolo_git::file_id(path).is_none_or(|id| !added.contains(&id)))
        .map(|path| RepoView {
            name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            path: path.display().to_string(),
        })
        .collect())
}

#[derive(Debug, serde::Serialize)]
pub struct RepoView {
    name: String,
    path: String,
}

/// 放代码的常见目录，存在的才返回。Duolo 自己的目录排在最前面。
fn code_dirs() -> Vec<PathBuf> {
    let Some(home) = home() else { return Vec::new() };
    CODE_DIRS.iter().map(|d| home.join(d)).filter(|d| d.is_dir()).collect()
}

const CODE_DIRS: &[&str] = &[
    PROJECTS_DIR,
    "projects",
    "Projects",
    "code",
    "Code",
    "src",
    "dev",
    "Developer",
    "workspace",
    "work",
    "repos",
    "github",
    "git",
];

/// Duolo 新建文件夹和克隆仓库的默认位置：`~/DuoloProjects`，不和用户自己的 `~/projects` 混在一起。
const PROJECTS_DIR: &str = "DuoloProjects";

fn home() -> Option<PathBuf> {
    Some(duolo_core::process::home_dir()).filter(|p| !p.as_os_str().is_empty())
}

fn projects_dir() -> PathBuf {
    home().unwrap_or_default().join(PROJECTS_DIR)
}

/// 新建文件夹和克隆仓库默认放在哪里（界面里显示给用户）。
#[tauri::command]
pub fn projects_home() -> String {
    projects_dir().display().to_string()
}

/// 新建一个文件夹作为项目（初始化为 git 仓库）。`parent` 为空时放在 `~/DuoloProjects`。
#[tauri::command]
pub async fn create_project_folder(state: State<'_, AppState>, name: String, parent: Option<String>) -> Res<String> {
    let parent = parent.map(PathBuf::from).unwrap_or_else(projects_dir);
    Ok(state.engine().create_project_folder(&parent, &name)?.0)
}

/// 克隆仓库到 `~/DuoloProjects`，并加为项目。
#[tauri::command]
pub async fn clone_project(state: State<'_, AppState>, url: String) -> Res<String> {
    let url = url.trim().to_string();
    let name = duolo_git::repo_name_from_url(&url)
        .filter(|_| url.contains('/') || url.contains(':'))
        .ok_or_else(|| UserError::InvalidCloneUrl { url: url.clone() })?;
    let dest = projects_dir().join(&name);
    if dest.exists() {
        return Err(UserError::PathExists { path: dest.display().to_string() }.into());
    }
    let target = dest.clone();
    tauri::async_runtime::spawn_blocking(move || duolo_git::clone(&url, &target)).await??;
    Ok(state.engine().add_project(&dest)?.0)
}

/// 用户在 CLI 里第一次输入的内容作为任务的需求，补上标题；`branch` 是 AI 起的分支名（可以为空）。
#[tauri::command]
pub async fn name_task(state: State<'_, AppState>, task: u64, prompt: String, branch: Option<String>) -> Res<()> {
    Ok(state.engine().name_task(TaskId(task), &prompt, branch.as_deref())?)
}

/// 关闭任务，返回没提交的改动保存到了哪里。
#[tauri::command]
pub async fn close_task(app: AppHandle, task: u64) -> Res<ClosedView> {
    let closed = blocking(app, move |state| ops::close_task(&state.engine, TaskId(task))).await?;
    Ok(ClosedView { saved_to: closed.saved_to, left_at: closed.left_at.map(|p| p.display().to_string()) })
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClosedView {
    saved_to: Option<String>,
    left_at: Option<String>,
}

#[tauri::command]
pub async fn start_agent(state: State<'_, AppState>, task: u64, agent: u32) -> Res<()> {
    Ok(state.engine().start_agent(key(task, agent))?)
}

/// 换个方式运行写代码的 Agent：`terminal` 或 `chat`，接着同一个会话。
#[tauri::command]
pub async fn set_surface(state: State<'_, AppState>, task: u64, surface: String) -> Res<()> {
    let surface = if surface == "chat" { Surface::Chat } else { Surface::Terminal };
    Ok(state.engine().set_surface(TaskId(task), surface)?)
}

#[tauri::command]
pub async fn start_reviewer(state: State<'_, AppState>, task: u64) -> Res<u32> {
    Ok(state.engine().start_reviewer(TaskId(task))?.agent)
}

/// 换写代码的 Agent：原来的停下，`kind` 在同一个 worktree 里接着做。返回接手的 Agent。
#[tauri::command]
pub async fn switch_writer(app: AppHandle, task: u64, kind: String) -> Res<u32> {
    blocking(app, move |state| ops::switch_writer(&state.engine, TaskId(task), AgentKind(kind)))
        .await
        .map(|key| key.agent)
}

/// 换审阅的 Agent，下一次审阅起生效。
#[tauri::command]
pub async fn set_reviewer(state: State<'_, AppState>, task: u64, kind: String) -> Res<()> {
    Ok(state.engine().set_reviewer(TaskId(task), AgentKind(kind))?)
}

/// 在后台审阅任务的改动。`reviewer` 是这次叫谁来审（会记住），不给就用上次叫的。
#[tauri::command]
pub async fn start_review(state: State<'_, AppState>, task: u64, reviewer: Option<String>) -> Res<()> {
    let mut engine = state.engine();
    match reviewer {
        Some(kind) => engine.review_with(TaskId(task), AgentKind(kind))?,
        None => engine.start_review(TaskId(task))?,
    }
    Ok(())
}

#[tauri::command]
pub fn cancel_review(state: State<'_, AppState>, task: u64) {
    state.engine().cancel_review(TaskId(task));
}

/// 订阅 Agent 的原始输出：先重放已有输出，之后实时推送。
#[tauri::command]
pub fn attach(state: State<'_, AppState>, task: u64, agent: u32, on_data: Channel<InvokeResponseBody>) -> Res<()> {
    let sink = Box::new(move |bytes: &[u8]| {
        let _ = on_data.send(InvokeResponseBody::Raw(bytes.to_vec()));
    });
    Ok(state.engine().attach(key(task, agent), sink)?)
}

#[tauri::command]
pub fn write(state: State<'_, AppState>, task: u64, agent: u32, data: String) -> Res<()> {
    Ok(state.engine().write(key(task, agent), data.as_bytes())?)
}

/// 在 Agent 的输入框里换行（⇧↵）。
#[tauri::command]
pub fn newline(state: State<'_, AppState>, task: u64, agent: u32) -> Res<()> {
    Ok(state.engine().newline(key(task, agent))?)
}

#[tauri::command]
pub fn send_text(state: State<'_, AppState>, task: u64, agent: u32, text: String, submit: bool) -> Res<()> {
    let (mut engine, key) = (state.engine(), key(task, agent));
    Ok(if submit { engine.submit(key, &text) } else { engine.paste(key, &text, false) }?)
}

/// 所有 Agent 共用一个窗格尺寸。
#[tauri::command]
pub fn resize(state: State<'_, AppState>, rows: u16, cols: u16) {
    state.engine().set_pane_size(Size::new(rows, cols));
}

#[tauri::command]
pub async fn diff_stat(app: AppHandle, task: u64) -> Res<Option<DiffView>> {
    blocking(app, move |state| {
        // 第一次要等后台算完；太久就先不显示，界面过几秒会再问。
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let (stat, pending) = {
                let mut engine = state.engine();
                (engine.diff_stat(TaskId(task)), engine.diff_pending(TaskId(task)))
            };
            if !pending || Instant::now() >= deadline {
                return Ok(stat.map(Into::into));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    })
    .await
}

/// 在系统的文件管理器里打开目录。
#[tauri::command]
pub fn reveal(path: String) -> Res<()> {
    Ok(crate::platform::reveal(&PathBuf::from(path))?)
}

/// 在终端里打开目录。
#[tauri::command]
pub fn open_terminal(path: String) -> Res<()> {
    Ok(crate::platform::open_terminal(&PathBuf::from(path))?)
}

// ---- 语音 ----

/// 实际使用的语音配置：是否整理按界面里的设置。
fn voice_config(state: &AppState) -> VoiceConfig {
    let (mut config, settings_path) = {
        let engine = state.engine();
        (engine.config().voice.clone(), engine.paths().settings_file())
    };
    let settings = Settings::load(&settings_path).unwrap_or_default();
    if let Some(organize) = settings.voice_organize {
        config.organize = organize;
    }
    config
}

/// 界面里的语音设置。
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct VoiceSettings {
    organize: bool,
}

#[tauri::command]
pub fn get_voice_settings(state: State<'_, AppState>) -> Res<VoiceSettings> {
    let (organize, path) = {
        let engine = state.engine();
        (engine.config().voice.organize, engine.paths().settings_file())
    };
    let settings = Settings::load(&path)?;
    Ok(VoiceSettings { organize: settings.voice_organize.unwrap_or(organize) })
}

#[tauri::command]
pub fn set_voice_settings(state: State<'_, AppState>, voice: VoiceSettings) -> Res<()> {
    let path = state.engine().paths().settings_file();
    let mut settings = Settings::load(&path)?;
    settings.voice_organize = Some(voice.organize);
    Ok(settings.save(&path)?)
}

/// 按住 Fn 说话是否可用。
#[tauri::command]
pub fn fn_key_status() -> crate::fnkey::FnKeyStatus {
    crate::fnkey::status()
}

/// 弹出系统的辅助功能授权提示。
#[tauri::command]
pub fn request_fn_key_permission() {
    crate::fnkey::request_permission();
}

/// 打开系统设置的对应页面（麦克风、辅助功能、键盘…）。
#[tauri::command]
pub fn open_system_settings(pane: String) -> Res<()> {
    Ok(crate::platform::open_settings(&pane)?)
}

/// 开始录音：连上 Duolo 服务器，带上 `task` 的热词和最近的对话（新建任务时只有项目名）。
#[tauri::command]
pub async fn voice_start(app: AppHandle, task: Option<u64>, project: Option<String>) -> Res<()> {
    eprintln!("voice: start (task {task:?})");
    blocking(app, move |state| {
        let (paths, server, hints) = {
            let engine = state.engine();
            let task = task.and_then(|id| engine.state().task(TaskId(id)));
            let events = task.and_then(|t| engine.events_since(t.id, 0).ok()).unwrap_or_default();
            let project_name = task.and_then(|t| engine.state().project(&t.project)).map(|p| p.name.as_str());
            let hints = duolo_voice::hints(&duolo_voice::HintSource {
                project: project_name.or(project.as_deref()),
                task_title: task.map(|t| t.title.as_str()),
                branch: task.map(|t| t.branch.as_str()),
                events: &events,
            });
            (engine.paths().clone(), engine.config().server.clone(), hints)
        };
        let organize = voice_config(state).organize;
        let recorder = duolo_voice::start(&paths, &server, hints, organize)
            .inspect_err(|e| eprintln!("voice: cannot start: {e:#}"))?;
        if let Some(old) = state.recorder.lock().unwrap_or_else(|p| p.into_inner()).replace(recorder) {
            old.cancel();
        }
        crate::fnkey::set_recording(true);
        Ok(())
    })
    .await
}

/// 录音中目前听到的内容和音量。
#[tauri::command]
pub fn voice_status(state: State<'_, AppState>) -> VoiceStatus {
    let recorder = state.recorder.lock().unwrap_or_else(|p| p.into_inner());
    match recorder.as_ref() {
        Some(r) => VoiceStatus { text: r.partial(), level: r.level() },
        None => VoiceStatus { text: String::new(), level: None },
    }
}

#[derive(Debug, serde::Serialize)]
pub struct VoiceStatus {
    text: String,
    /// 0~1；还没收到声音时为空。
    level: Option<f32>,
}

#[tauri::command]
pub fn voice_cancel(state: State<'_, AppState>) {
    crate::fnkey::set_recording(false);
    if let Some(recorder) = state.recorder.lock().unwrap_or_else(|p| p.into_inner()).take() {
        recorder.cancel();
    }
}

/// 结束录音，返回原话和服务器整理后的指令（没开整理或整理失败时为 None）。
#[tauri::command]
pub async fn voice_finish(state: State<'_, AppState>) -> Res<VoiceResult> {
    crate::fnkey::set_recording(false);
    let recorder = state.recorder.lock().unwrap_or_else(|p| p.into_inner()).take().ok_or(UserError::NotRecording)?;
    let transcript = tauri::async_runtime::spawn_blocking(move || recorder.finish()).await??;
    Ok(VoiceResult { raw: transcript.raw, organized: transcript.organized })
}

#[derive(Debug, serde::Serialize)]
pub struct VoiceResult {
    raw: String,
    organized: Option<String>,
}

// ---- 语言 ----

/// 用户选择的界面语言：`auto` 或语言代码。
#[tauri::command]
pub fn get_language(state: State<'_, AppState>) -> Res<String> {
    let path = state.engine().paths().settings_file();
    Ok(Settings::load(&path)?.language)
}

#[tauri::command]
pub fn set_language(state: State<'_, AppState>, language: String) -> Res<()> {
    let path = state.engine().paths().settings_file();
    let mut settings = Settings::load(&path)?;
    settings.language = language;
    Ok(settings.save(&path)?)
}

/// 系统的首选语言列表（如 `zh-Hans-CN`）。网页引擎里的 navigator.languages 不一定反映系统设置，以这里为准。
#[tauri::command]
pub fn system_languages() -> Vec<String> {
    sys_locale::get_locales().collect()
}

/// 界面确定了实际使用的语言后调用：菜单和审阅结论跟着切换。
#[tauri::command]
pub fn apply_locale(app: AppHandle, locale: String) -> Res<()> {
    Ok(crate::apply_locale(&app, &locale)?)
}

/// 界面里的报错写到 stderr，方便排查。
#[tauri::command]
pub fn log(message: String) {
    eprintln!("[ui] {message}");
}

/// 界面收到了退出请求，会自己处理（见 `request_quit`）。
#[tauri::command]
pub fn quit_seen(state: State<'_, AppState>) {
    state.quit_seen.store(true, Ordering::Relaxed);
}

/// 结束所有 Agent 并退出。
#[tauri::command]
pub async fn quit(app: AppHandle) {
    let handle = app.clone();
    // 要等 Agent 的进程都消失，不占着界面线程。
    let _ = blocking(app, |state| {
        state.engine().shutdown();
        Ok(())
    })
    .await;
    handle.exit(0);
}
