//! 界面调用的命令。每个命令只做参数转换，具体逻辑都在 `duolo-engine` 里。

use std::path::PathBuf;

use duolo_core::{AgentKey, ProjectId, TaskId};
use duolo_core::{Settings, UserError, VoiceConfig};
use duolo_engine::{check_environment, Size};
use duolo_voice::{OrganizeContext, Recorder};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, State};

use crate::view::{self, CheckView, DiffView, Snapshot};
use crate::{AppState, Res};

fn key(task: u64, agent: u32) -> AgentKey {
    AgentKey { task: TaskId(task), agent }
}

#[tauri::command]
pub fn snapshot(state: State<'_, AppState>) -> Snapshot {
    view::snapshot(&mut state.engine())
}

/// 检查 git、Agent CLI 和语音。会运行几个 `--version`，放到后台线程执行。
#[tauri::command]
pub async fn environment(state: State<'_, AppState>) -> Res<Vec<CheckView>> {
    let mut config = state.engine().config().clone();
    config.voice = voice_config(&state);
    let checks = tauri::async_runtime::spawn_blocking(move || check_environment(&config)).await?;
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

#[tauri::command]
pub async fn create_task(state: State<'_, AppState>, project: String, prompt: String) -> Res<u64> {
    Ok(state.engine().create_task(&ProjectId(project), &prompt)?.0)
}

/// 用户在 CLI 里第一次输入的内容作为任务的需求，补上标题和分支名。
#[tauri::command]
pub async fn name_task(state: State<'_, AppState>, task: u64, prompt: String) -> Res<()> {
    Ok(state.engine().name_task(TaskId(task), &prompt)?)
}

#[tauri::command]
pub async fn close_task(state: State<'_, AppState>, task: u64) -> Res<()> {
    Ok(state.engine().close_task(TaskId(task))?)
}

#[tauri::command]
pub async fn start_agent(state: State<'_, AppState>, task: u64, agent: u32) -> Res<()> {
    Ok(state.engine().start_agent(key(task, agent))?)
}

#[tauri::command]
pub async fn start_reviewer(state: State<'_, AppState>, task: u64) -> Res<u32> {
    Ok(state.engine().start_reviewer(TaskId(task))?.agent)
}

/// 在后台审阅任务的改动。
#[tauri::command]
pub async fn start_review(state: State<'_, AppState>, task: u64) -> Res<()> {
    Ok(state.engine().start_review(TaskId(task))?)
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

#[tauri::command]
pub fn send_text(state: State<'_, AppState>, task: u64, agent: u32, text: String, submit: bool) -> Res<()> {
    Ok(state.engine().paste(key(task, agent), &text, submit)?)
}

/// 所有 Agent 共用一个窗格尺寸。
#[tauri::command]
pub fn resize(state: State<'_, AppState>, rows: u16, cols: u16) {
    state.engine().set_pane_size(Size::new(rows, cols));
}

#[tauri::command]
pub async fn diff_stat(state: State<'_, AppState>, task: u64) -> Res<Option<DiffView>> {
    Ok(state.engine().diff_stat(TaskId(task)).map(Into::into))
}

/// 在 Finder 或终端里打开目录。
#[tauri::command]
pub fn reveal(path: String, app: Option<String>) -> Res<()> {
    let mut cmd = std::process::Command::new("open");
    if let Some(app) = app {
        cmd.arg("-a").arg(app);
    }
    cmd.arg(path).spawn()?;
    Ok(())
}

// ---- 语音 ----

/// 随应用打包的语音识别程序（macOS Speech 框架），见 `dictate/main.swift`。
fn bundled_dictate() -> Option<String> {
    let path = std::env::current_exe().ok()?.parent()?.join("duolo-dictate");
    let usable = std::fs::metadata(&path).is_ok_and(|m| m.len() > 0);
    usable.then(|| path.display().to_string())
}

/// 实际使用的语音配置：用户没有配置 `voice.command` 时，用内置的识别程序，识别语言跟随界面语言。
fn voice_config(state: &AppState) -> VoiceConfig {
    let mut config = state.engine().config().voice.clone();
    if !config.is_configured() {
        if let Some(dictate) = bundled_dictate() {
            let locale = state.locale.lock().unwrap_or_else(|p| p.into_inner()).clone();
            config.command = vec![dictate, "--locale".into(), locale];
        }
    }
    config
}

#[tauri::command]
pub fn voice_start(state: State<'_, AppState>) -> Res<()> {
    let config = voice_config(&state);
    let recorder = Recorder::start(&config)?;
    if let Some(old) = state.recorder.lock().unwrap_or_else(|p| p.into_inner()).replace(recorder) {
        old.cancel();
    }
    Ok(())
}

/// 录音中目前听到的内容。
#[tauri::command]
pub fn voice_partial(state: State<'_, AppState>) -> String {
    state.recorder.lock().unwrap_or_else(|p| p.into_inner()).as_ref().map(Recorder::partial).unwrap_or_default()
}

#[tauri::command]
pub fn voice_cancel(state: State<'_, AppState>) {
    if let Some(recorder) = state.recorder.lock().unwrap_or_else(|p| p.into_inner()).take() {
        recorder.cancel();
    }
}

/// 结束录音，返回原话和整理后的指令（未开启整理或整理失败时为 None）。
#[tauri::command]
pub async fn voice_finish(
    state: State<'_, AppState>,
    project: Option<String>,
    task: Option<String>,
) -> Res<VoiceResult> {
    let recorder = state.recorder.lock().unwrap_or_else(|p| p.into_inner()).take().ok_or(UserError::NotRecording)?;
    let config = voice_config(&state);
    tauri::async_runtime::spawn_blocking(move || {
        let raw = recorder.finish()?;
        let organized = if config.organize {
            duolo_voice::organize(&config, &raw, &OrganizeContext { project, task }).ok().filter(|t| *t != raw)
        } else {
            None
        };
        Ok(VoiceResult { raw, organized })
    })
    .await?
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

/// 结束所有 Agent 并退出。
#[tauri::command]
pub fn quit(app: AppHandle, state: State<'_, AppState>) {
    state.engine().shutdown();
    app.exit(0);
}
