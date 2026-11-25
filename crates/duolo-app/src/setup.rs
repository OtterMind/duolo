//! 一键安装、升级 Agent，登录账号（见 `duolo_engine::start_setup`）：画面推给界面显示，用户可以在里面输入（粘贴授权码）。
//! 同一时间只运行一个，结束时发 `setup-exit` 事件（带着是第几次，界面只认自己开始的那次），界面重新检查环境。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use duolo_core::AgentKind;
use duolo_engine::{start_setup, Session, SetupStep, Size};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Emitter, State};

use crate::{AppState, Res};

/// 结束时通知界面的事件：`{ "run": 第几次, "success": bool }`。
const EXIT_EVENT: &str = "setup-exit";

type Running = Arc<Mutex<Option<(u64, Session)>>>;

/// 正在运行的安装或登录，和它是第几次（换成新的一次后，旧的那次的等待线程就不管了）。
#[derive(Debug, Default)]
pub struct Setup {
    running: Running,
    next: AtomicU64,
}

fn lock(running: &Running) -> MutexGuard<'_, Option<(u64, Session)>> {
    running.lock().unwrap_or_else(|p| p.into_inner())
}

/// 开始安装（`step` 为 `install`）、升级（`update`）或登录（`login`）`agent`，返回这是第几次。上一次没结束的先结束。
#[tauri::command]
pub fn setup_start(
    app: AppHandle,
    state: State<'_, AppState>,
    agent: String,
    step: String,
    rows: u16,
    cols: u16,
    on_data: Channel<InvokeResponseBody>,
) -> Res<u64> {
    let step = match step.as_str() {
        "install" => SetupStep::Install,
        "update" => SetupStep::Update,
        _ => SetupStep::Login,
    };
    let sink = Box::new(move |bytes: &[u8]| {
        let _ = on_data.send(InvokeResponseBody::Raw(bytes.to_vec()));
    });
    let size = Size::new(rows.max(4), cols.max(20));
    let session = start_setup(state.engine().agents(), &AgentKind(agent), step, size, sink)?;
    let setup = &state.setup;
    let id = setup.next.fetch_add(1, Ordering::Relaxed);
    if let Some((_, mut old)) = lock(&setup.running).replace((id, session)) {
        old.stop();
    }
    let running = Arc::clone(&setup.running);
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(250));
        let mut guard = lock(&running);
        let Some((current, session)) = guard.as_mut() else { return };
        if *current != id {
            return;
        }
        let Some(exit) = session.poll_exit() else { continue };
        guard.take();
        drop(guard);
        // 装好后重新读一遍登录 shell 的 PATH：安装脚本可能把自己的目录加进了 shell 配置。
        #[cfg(unix)]
        if step != SetupStep::Login && exit.success {
            crate::shell_env::refresh_path();
        }
        let _ = app.emit(EXIT_EVENT, serde_json::json!({ "run": id, "success": exit.success }));
        return;
    });
    Ok(id)
}

/// 现在登录了没有（登录过程中界面隔一会儿问一次）。不知道时为 null。
#[tauri::command]
pub async fn setup_signed_in(state: State<'_, AppState>, agent: String) -> Res<Option<bool>> {
    let agents = state.engine().agents().clone();
    Ok(tauri::async_runtime::spawn_blocking(move || duolo_engine::signed_in(&agents, &AgentKind(agent))).await?)
}

/// 用户在终端里输入的（粘贴授权码、按回车）。
#[tauri::command]
pub fn setup_write(state: State<'_, AppState>, data: String) -> Res<()> {
    if let Some((_, session)) = lock(&state.setup.running).as_mut() {
        session.write(data.as_bytes())?;
    }
    Ok(())
}

#[tauri::command]
pub fn setup_resize(state: State<'_, AppState>, rows: u16, cols: u16) -> Res<()> {
    if let Some((_, session)) = lock(&state.setup.running).as_mut() {
        session.resize(Size::new(rows.max(4), cols.max(20)))?;
    }
    Ok(())
}

/// 取消：结束整棵进程树。
#[tauri::command]
pub fn setup_stop(state: State<'_, AppState>) {
    if let Some((_, mut session)) = lock(&state.setup.running).take() {
        session.stop();
    }
}
