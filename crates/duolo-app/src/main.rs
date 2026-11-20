//! Duolo 桌面应用的后端。
//!
//! 界面在仓库的 `app/` 目录（React + xterm.js）。这里持有 [`Engine`]，把它的能力作为命令提供给界面，
//! 并在后台定时生成 [`view::Snapshot`]，内容变化时推送给界面。Agent 的原始输出通过
//! `attach` 命令的 Channel 直接推给 xterm.js。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod locale;
mod shell_env;
mod view;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use duolo_core::{Paths, UserError};
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
    /// 界面当前使用的语言代码，内置语音识别按它选择识别语言。
    locale: Mutex<String>,
}

impl AppState {
    fn engine(&self) -> MutexGuard<'_, Engine> {
        self.engine.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// 命令的错误。用户可处理的错误带上错误码和参数，由界面按当前语言显示；
/// 其他错误的错误码是 `internal`，界面显示通用文案并附上英文详情。
#[derive(Debug, Serialize)]
pub struct Error {
    code: &'static str,
    params: BTreeMap<&'static str, String>,
    message: String,
}

pub type Res<T> = Result<T, Error>;

impl Error {
    fn internal(message: String) -> Self {
        Self { code: "internal", params: BTreeMap::new(), message }
    }
}

impl From<UserError> for Error {
    fn from(err: UserError) -> Self {
        Self { code: err.code(), params: err.params(), message: err.to_string() }
    }
}

impl From<anyhow::Error> for Error {
    fn from(err: anyhow::Error) -> Self {
        match err.chain().find_map(|e| e.downcast_ref::<UserError>()) {
            Some(user) => Self { message: format!("{err:#}"), ..user.clone().into() },
            None => Self::internal(format!("{err:#}")),
        }
    }
}

impl From<tauri::Error> for Error {
    fn from(err: tauri::Error) -> Self {
        Self::internal(err.to_string())
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Self::internal(err.to_string())
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
        locale: Mutex::new("en".into()),
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
        // 界面加载后会按用户的语言调用 apply_locale 重建菜单。
        .menu(|app| build_menu(app, "en"))
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
            commands::name_task,
            commands::close_task,
            commands::start_agent,
            commands::start_reviewer,
            commands::start_review,
            commands::cancel_review,
            commands::attach,
            commands::write,
            commands::send_text,
            commands::resize,
            commands::diff_stat,
            commands::reveal,
            commands::voice_start,
            commands::voice_cancel,
            commands::voice_partial,
            commands::voice_finish,
            commands::log,
            commands::quit,
            commands::get_language,
            commands::system_languages,
            commands::set_language,
            commands::apply_locale,
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
                engine.poll_reviews();
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

/// 应用菜单，文案按 `locale` 显示。自定义项的 id 以 `duolo:` 开头，点击后转发给界面处理，快捷键也在这里注册。
fn build_menu(app: &AppHandle, locale: &str) -> tauri::Result<Menu<tauri::Wry>> {
    let text = locale::menu_text(locale);
    let item =
        |id: &str, label: &str, accel: &str| MenuItem::with_id(app, format!("duolo:{id}"), label, true, Some(accel));
    let app_menu = Submenu::with_items(
        app,
        "Duolo",
        true,
        &[
            &PredefinedMenuItem::about(app, Some(text.about), None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, Some(text.hide))?,
            &PredefinedMenuItem::hide_others(app, Some(text.hide_others))?,
            &PredefinedMenuItem::separator(app)?,
            &item("quit", text.quit, "CmdOrCtrl+Q")?,
        ],
    )?;
    let task_menu = Submenu::with_items(
        app,
        text.task,
        true,
        &[
            &item("new-task", text.new_task, "CmdOrCtrl+N")?,
            &item("add-project", text.add_project, "CmdOrCtrl+O")?,
            &PredefinedMenuItem::separator(app)?,
            &item("review", text.review, "CmdOrCtrl+Shift+R")?,
            &item("prev-task", text.prev_task, "CmdOrCtrl+Alt+Up")?,
            &item("next-task", text.next_task, "CmdOrCtrl+Alt+Down")?,
            &PredefinedMenuItem::separator(app)?,
            &item("reveal", text.reveal, "CmdOrCtrl+Shift+F")?,
            &item("close-task", text.close_task, "CmdOrCtrl+Shift+W")?,
        ],
    )?;
    let edit_menu = Submenu::with_items(
        app,
        text.edit,
        true,
        &[
            &PredefinedMenuItem::undo(app, Some(text.undo))?,
            &PredefinedMenuItem::redo(app, Some(text.redo))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, Some(text.cut))?,
            &PredefinedMenuItem::copy(app, Some(text.copy))?,
            &PredefinedMenuItem::paste(app, Some(text.paste))?,
            &PredefinedMenuItem::select_all(app, Some(text.select_all))?,
            &PredefinedMenuItem::separator(app)?,
            &item("voice", text.voice, "CmdOrCtrl+Shift+Space")?,
        ],
    )?;
    let window_menu = Submenu::with_items(
        app,
        text.window,
        true,
        &[
            &PredefinedMenuItem::minimize(app, Some(text.minimize))?,
            &PredefinedMenuItem::fullscreen(app, Some(text.fullscreen))?,
            &PredefinedMenuItem::close_window(app, Some(text.close_window))?,
        ],
    )?;
    Menu::with_items(app, &[&app_menu, &task_menu, &edit_menu, &window_menu])
}

/// 切换界面语言：重建菜单，审阅结论也改用这种语言。
pub fn apply_locale(app: &AppHandle, locale: &str) -> tauri::Result<()> {
    app.set_menu(build_menu(app, locale)?)?;
    let state = app.state::<AppState>();
    state.engine().set_reply_language(locale::language_name(locale));
    *state.locale.lock().unwrap_or_else(|p| p.into_inner()) = locale.to_string();
    Ok(())
}
