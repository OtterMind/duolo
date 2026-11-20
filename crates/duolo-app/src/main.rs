//! Duolo 桌面应用的后端。
//!
//! 界面在仓库的 `app/` 目录（React + xterm.js）。这里持有 [`Engine`]，把它的能力作为命令提供给界面，
//! 并在后台定时生成 [`view::Snapshot`]，内容变化时推送给界面。Agent 的原始输出通过
//! `attach` 命令的 Channel 直接推给 xterm.js。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod shell_env;
mod view;

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use duolo_core::Paths;
use duolo_engine::Engine;
use duolo_voice::Recorder;
use serde::Serialize;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Emitter, Manager, RunEvent, WindowEvent};

/// 状态推送的间隔。
const SNAPSHOT_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug)]
pub struct AppState {
    engine: Mutex<Engine>,
    recorder: Mutex<Option<Recorder>>,
    /// 等界面来取的"打开项目"请求。
    open_request: Mutex<Option<String>>,
}

impl AppState {
    fn engine(&self) -> MutexGuard<'_, Engine> {
        self.engine.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// 命令的错误：转成一句给用户看的话。
#[derive(Debug)]
pub struct Error(String);

pub type Res<T> = Result<T, Error>;

impl Serialize for Error {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl From<anyhow::Error> for Error {
    fn from(err: anyhow::Error) -> Self {
        Self(format!("{err:#}"))
    }
}

impl From<tauri::Error> for Error {
    fn from(err: tauri::Error) -> Self {
        Self(err.to_string())
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Self(err.to_string())
    }
}

fn main() {
    shell_env::load_login_path();
    let engine = match Paths::discover().and_then(Engine::open) {
        Ok(engine) => engine,
        Err(err) => {
            eprintln!("duolo: {err:#}");
            std::process::exit(1);
        }
    };
    let state = AppState {
        engine: Mutex::new(engine),
        recorder: Mutex::new(None),
        open_request: Mutex::new(path_arg(std::env::args().skip(1), None)),
    };

    let app = tauri::Builder::default()
        // 必须最先注册：第二次启动（例如 `duolo <目录>`）时把参数交给已运行的实例。
        .plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
            if let Some(path) = path_arg(args.into_iter().skip(1), Some(PathBuf::from(cwd))) {
                let _ = app.emit("open-project", path);
            }
            show_main_window(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .menu(build_menu)
        .on_menu_event(|app, event| {
            if let Some(name) = event.id().as_ref().strip_prefix("duolo:") {
                let _ = app.emit("menu", name);
            }
        })
        .setup(|app| {
            spawn_snapshot_loop(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            // 关闭窗口只是隐藏，Agent 继续在后台运行；从 Dock 可以再打开。⌘Q 才真正退出。
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::snapshot,
            commands::environment,
            commands::take_open_request,
            commands::add_project,
            commands::create_task,
            commands::close_task,
            commands::start_agent,
            commands::start_reviewer,
            commands::attach,
            commands::write,
            commands::send_text,
            commands::resize,
            commands::diff_stat,
            commands::reveal,
            commands::voice_start,
            commands::voice_cancel,
            commands::voice_finish,
            commands::log,
            commands::quit,
        ])
        .build(tauri::generate_context!())
        .expect("无法启动 Duolo");

    app.run(|app, event| match event {
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => show_main_window(app),
        RunEvent::Exit => app.state::<AppState>().engine().shutdown(),
        _ => {}
    });
}

/// 命令行参数里的目录（相对路径按 `cwd` 解析）。
fn path_arg(mut args: impl Iterator<Item = String>, cwd: Option<PathBuf>) -> Option<String> {
    let arg = args.find(|a| !a.starts_with('-'))?;
    let path = PathBuf::from(arg);
    let path = match cwd {
        Some(cwd) if path.is_relative() => cwd.join(path),
        _ => path,
    };
    Some(path.display().to_string())
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// 后台定时生成状态快照，变化时推送给界面。
fn spawn_snapshot_loop(app: AppHandle) {
    thread::spawn(move || {
        let mut last = None;
        loop {
            let state = app.state::<AppState>();
            let snapshot = {
                let mut engine = state.engine();
                engine.retry_failed_resumes();
                view::snapshot(&mut engine)
            };
            if last.as_ref() != Some(&snapshot) {
                let _ = app.emit("snapshot", &snapshot);
                last = Some(snapshot);
            }
            thread::sleep(SNAPSHOT_INTERVAL);
        }
    });
}

/// 应用菜单。自定义项的 id 以 `duolo:` 开头，点击后转发给界面处理，快捷键也在这里注册。
fn build_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let item =
        |id: &str, text: &str, accel: &str| MenuItem::with_id(app, format!("duolo:{id}"), text, true, Some(accel));
    let app_menu = Submenu::with_items(
        app,
        "Duolo",
        true,
        &[
            &PredefinedMenuItem::about(app, Some("关于 Duolo"), None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, Some("隐藏 Duolo"))?,
            &PredefinedMenuItem::hide_others(app, Some("隐藏其他"))?,
            &PredefinedMenuItem::separator(app)?,
            &item("quit", "退出 Duolo", "CmdOrCtrl+Q")?,
        ],
    )?;
    let task_menu = Submenu::with_items(
        app,
        "任务",
        true,
        &[
            &item("new-task", "新任务", "CmdOrCtrl+N")?,
            &item("add-project", "添加项目…", "CmdOrCtrl+O")?,
            &PredefinedMenuItem::separator(app)?,
            &item("review", "让 Codex 审阅", "CmdOrCtrl+Shift+R")?,
            &item("next-agent", "切换 Agent", "Ctrl+Tab")?,
            &item("prev-task", "上一个任务", "CmdOrCtrl+Alt+Up")?,
            &item("next-task", "下一个任务", "CmdOrCtrl+Alt+Down")?,
            &PredefinedMenuItem::separator(app)?,
            &item("reveal", "在 Finder 中显示", "CmdOrCtrl+Shift+F")?,
            &item("close-task", "关闭任务…", "CmdOrCtrl+Shift+W")?,
        ],
    )?;
    let edit_menu = Submenu::with_items(
        app,
        "编辑",
        true,
        &[
            &PredefinedMenuItem::undo(app, Some("撤销"))?,
            &PredefinedMenuItem::redo(app, Some("重做"))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, Some("剪切"))?,
            &PredefinedMenuItem::copy(app, Some("拷贝"))?,
            &PredefinedMenuItem::paste(app, Some("粘贴"))?,
            &PredefinedMenuItem::select_all(app, Some("全选"))?,
            &PredefinedMenuItem::separator(app)?,
            &item("voice", "语音输入", "Ctrl+Space")?,
        ],
    )?;
    let window_menu = Submenu::with_items(
        app,
        "窗口",
        true,
        &[
            &PredefinedMenuItem::minimize(app, Some("最小化"))?,
            &PredefinedMenuItem::fullscreen(app, Some("全屏"))?,
            &PredefinedMenuItem::close_window(app, Some("关闭窗口"))?,
        ],
    )?;
    Menu::with_items(app, &[&app_menu, &task_menu, &edit_menu, &window_menu])
}
