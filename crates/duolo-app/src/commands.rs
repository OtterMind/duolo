//! 界面调用的命令。每个命令只做参数转换，具体逻辑都在 `duolo-engine` 里。

use std::path::PathBuf;

use anyhow::anyhow;
use duolo_core::{AgentKey, ProjectId, TaskId};
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
    let config = state.engine().config().clone();
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

#[tauri::command]
pub fn voice_start(state: State<'_, AppState>) -> Res<()> {
    let config = state.engine().config().voice.clone();
    let recorder = Recorder::start(&config)?;
    if let Some(old) = state.recorder.lock().unwrap_or_else(|p| p.into_inner()).replace(recorder) {
        old.cancel();
    }
    Ok(())
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
    let recorder =
        state.recorder.lock().unwrap_or_else(|p| p.into_inner()).take().ok_or_else(|| anyhow!("没有在录音"))?;
    let config = state.engine().config().voice.clone();
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
