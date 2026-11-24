//! Duolo 桌面应用的后端。
//!
//! 界面在仓库的 `app/` 目录（React + xterm.js）。这里持有 [`Engine`]，把它的能力作为命令提供给界面，
//! 并在后台定时生成 [`view::Snapshot`]，内容变化时推送给界面。Agent 的原始输出通过
//! `attach` 命令的 Channel 直接推给 xterm.js。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod fnkey;
mod platform;
#[cfg(unix)]
mod shell_env;
mod view;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use duolo_core::{Paths, UserError};
use duolo_engine::Engine;
use duolo_voice::Recorder;
use serde::Serialize;
#[cfg(target_os = "macos")]
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Emitter, Manager, RunEvent, WindowEvent};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_window_state::StateFlags;

/// 界面多久没回应退出请求，就当作它坏了。
const QUIT_REPLY: Duration = Duration::from_secs(3);
/// 强制退出前最多等 Agent 停多久。
const FORCE_QUIT_AFTER: Duration = Duration::from_secs(8);
/// 状态推送的间隔。
const SNAPSHOT_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug)]
pub struct AppState {
    engine: Mutex<Engine>,
    recorder: Mutex<Option<Recorder>>,
    /// 等界面来取的"打开项目"请求。
    open_request: Mutex<Option<String>>,
    /// 界面收到了这次退出请求。
    quit_seen: AtomicBool,
    /// 正在用系统弹窗问要不要退出。
    asking_quit: AtomicBool,
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
    pub(crate) fn internal(message: String) -> Self {
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
    // Agent 运行钩子时用的就是这个程序（见 duolo_core::hook）：写下事件就退出，什么界面都不开。
    if std::env::args_os().nth(1).is_some_and(|arg| arg == duolo_core::hook::HOOK_ARG) {
        return duolo_core::hook::run();
    }
    #[cfg(unix)]
    shell_env::load();
    let mut engine = match Paths::discover().and_then(Engine::open) {
        Ok(engine) => engine,
        Err(err) => return cannot_start(&err),
    };
    if let Ok(exe) = std::env::current_exe() {
        engine.set_hook_program(exe);
    }
    let state = AppState {
        engine: Mutex::new(engine),
        recorder: Mutex::new(None),
        open_request: Mutex::new(path_arg(std::env::args().skip(1), None).inspect(|p| eprintln!("open request: {p}"))),
        quit_seen: AtomicBool::new(false),
        asking_quit: AtomicBool::new(false),
    };

    let builder = tauri::Builder::default()
        // 必须最先注册：第二次启动（例如 `duolo <目录>`）时把参数交给已运行的实例。
        .plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
            if let Some(path) = path_arg(args.into_iter().skip(1), Some(PathBuf::from(cwd))) {
                eprintln!("open request from another instance: {path}");
                let _ = app.emit("open-project", path);
            }
            show_main_window(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        // 记住窗口的大小、位置和是否最大化，下次照原样打开（Warp、Orca 都这样）。
        // 不记"是否可见"：macOS 上关窗口只是隐藏，记下来下次启动窗口就出不来了。
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(StateFlags::SIZE | StateFlags::POSITION | StateFlags::MAXIMIZED)
                .build(),
        )
        .manage(state);
    // 原生菜单只在 macOS 上用（菜单在屏幕顶部）。Windows / Linux 上菜单会出现在窗口里，
    // 而且它的 Ctrl 快捷键会抢走终端的按键，所以那边的快捷键由界面自己处理（见 app/src/lib/platform.ts）。
    #[cfg(target_os = "macos")]
    let builder = builder
        // 界面加载后会按用户的语言调用 apply_locale 重建菜单。
        .menu(|app| build_menu(app, "en"))
        .on_menu_event(|app, event| match event.id().as_ref().strip_prefix("duolo:") {
            Some("quit") => request_quit(app),
            Some(name) => {
                let _ = app.emit("menu", name);
            }
            None => {}
        });
    let app = builder
        .setup(|app| {
            first_launch_size(app);
            spawn_snapshot_loop(app.handle().clone());
            fnkey::start(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if cfg!(target_os = "macos") {
                    // macOS：关闭窗口只是隐藏，Agent 继续在后台运行，从 Dock 可以再打开；⌘Q 才真正退出。
                    let _ = window.hide();
                } else {
                    // Windows / Linux 没有 Dock，隐藏了就找不回来：按"退出"处理，界面会先确认。
                    request_quit(window.app_handle());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::snapshot,
            commands::startup_notices,
            commands::environment,
            commands::take_open_request,
            commands::add_project,
            commands::create_task,
            commands::agent_plugins,
            commands::agent_defaults,
            commands::task_events,
            commands::execute,
            commands::rename_task,
            commands::branches,
            commands::discover_repos,
            commands::clone_project,
            commands::projects_home,
            commands::create_project_folder,
            commands::project_repos,
            commands::add_repo,
            commands::suggest_branch,
            commands::rename_branch,
            commands::name_task,
            commands::close_task,
            commands::start_agent,
            commands::start_reviewer,
            commands::switch_writer,
            commands::set_reviewer,
            commands::start_review,
            commands::cancel_review,
            commands::attach,
            commands::write,
            commands::newline,
            commands::send_text,
            commands::resize,
            commands::diff_stat,
            commands::reveal,
            commands::open_terminal,
            commands::voice_start,
            commands::voice_cancel,
            commands::voice_status,
            commands::get_voice_settings,
            commands::set_voice_settings,
            commands::fn_key_status,
            commands::request_fn_key_permission,
            commands::open_system_settings,
            commands::voice_finish,
            commands::log,
            commands::quit,
            commands::quit_seen,
            commands::get_language,
            commands::system_languages,
            commands::set_language,
            commands::apply_locale,
        ])
        .build(context())
        .expect("cannot start Duolo");

    app.run(|app, event| match event {
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => show_main_window(app),
        RunEvent::Exit => app.state::<AppState>().engine().shutdown(),
        _ => {}
    });
}

/// `generate_context!` 会把界面文件和 Info.plist 嵌进程序，只能展开一次。
fn context() -> tauri::Context {
    tauri::generate_context!()
}

/// 第一次打开（还没记过窗口大小）时铺满屏幕的可用区域（不含菜单栏、Dock、任务栏），和 Orca 一样；
/// 以后按上次关闭时的大小打开。屏幕信息拿不到时保持配置里的 1280×820（和 Warp 的默认差不多）。
fn first_launch_size(app: &tauri::App) {
    let saved = app.path().app_config_dir().map(|dir| dir.join(tauri_plugin_window_state::DEFAULT_FILENAME));
    if saved.is_ok_and(|file| file.exists()) {
        return;
    }
    let Some(window) = app.get_webview_window("main") else { return };
    let Ok(Some(monitor)) = window.primary_monitor() else { return };
    let area = monitor.work_area();
    let _ = window.set_size(area.size);
    let _ = window.set_position(area.position);
}

/// 打不开数据目录（没有权限、磁盘只读……）：弹窗说明原因再退出，不然双击图标会毫无反应。
fn cannot_start(err: &anyhow::Error) {
    eprintln!("duolo: {err:#}");
    duolo_core::i18n::apply_preference(duolo_core::settings::LANGUAGE_AUTO);
    let message = UserError::CannotStart { detail: format!("{err:#}") }.localized();
    let mut context = context();
    // 不开主窗口：没有数据，界面什么也显示不了。
    context.config_mut().app.windows.clear();
    let shown = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            let handle = app.handle().clone();
            let dialog = app.dialog().message(message).title("Duolo").kind(MessageDialogKind::Error);
            dialog.show(move |_| handle.exit(1));
            Ok(())
        })
        .run(context);
    if shown.is_err() {
        std::process::exit(1);
    }
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

/// 请界面处理退出（有 Agent 在运行时先确认）。界面没有回应（崩溃、卡死）时改用系统弹窗问，不然窗口会关不掉。
fn request_quit(app: &AppHandle) {
    let app = app.clone();
    app.state::<AppState>().quit_seen.store(false, Ordering::Relaxed);
    let _ = app.emit("menu", "quit");
    thread::spawn(move || {
        thread::sleep(QUIT_REPLY);
        let state = app.state::<AppState>();
        if state.quit_seen.load(Ordering::Relaxed) || state.asking_quit.swap(true, Ordering::Relaxed) {
            return;
        }
        let buttons =
            MessageDialogButtons::OkCancelCustom(duolo_core::t("confirm.quitAction"), duolo_core::t("common.cancel"));
        let dialog = app.dialog().message(duolo_core::t("quit.notResponding")).title("Duolo").buttons(buttons);
        let handle = app.clone();
        dialog.kind(MessageDialogKind::Warning).show(move |yes| {
            handle.state::<AppState>().asking_quit.store(false, Ordering::Relaxed);
            if yes {
                force_quit(handle);
            }
        });
    });
}

/// 尽量先停下 Agent 再退出；后端也卡住时不再等。
fn force_quit(app: AppHandle) {
    thread::spawn(|| {
        thread::sleep(FORCE_QUIT_AFTER);
        std::process::exit(1);
    });
    thread::spawn(move || {
        app.state::<AppState>().engine().shutdown();
        app.exit(0);
    });
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
            let (snapshot, events) = {
                let mut engine = state.engine();
                // `duolo new`、终端界面可能刚改过任务。
                engine.sync();
                engine.retry_failed_resumes();
                engine.poll_reviews();
                let events = engine.poll_events();
                (view::snapshot(&mut engine), events)
            };
            // 任务的事件流（对话、工具调用、状态、等你选……），以后手机端也用它。
            if !events.is_empty() {
                let _ = app.emit("task-events", &events);
            }
            if last.as_ref() != Some(&snapshot) {
                let _ = app.emit("snapshot", &snapshot);
                last = Some(snapshot);
            }
            thread::sleep(SNAPSHOT_INTERVAL);
        }
    });
}

/// 应用菜单（只在 macOS 上用），文案按 `locale` 显示。
/// 自定义项的 id 以 `duolo:` 开头，点击后转发给界面处理，快捷键也在这里注册。
#[cfg(target_os = "macos")]
fn build_menu(app: &AppHandle, locale: &str) -> tauri::Result<Menu<tauri::Wry>> {
    let t = |key: &str| duolo_core::i18n::translate(locale, key);
    let item =
        |id: &str, label: &str, accel: &str| MenuItem::with_id(app, format!("duolo:{id}"), label, true, Some(accel));
    let app_menu = Submenu::with_items(
        app,
        "Duolo",
        true,
        &[
            &PredefinedMenuItem::about(app, Some(t("menu.about").as_str()), None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, Some(t("menu.hide").as_str()))?,
            &PredefinedMenuItem::hide_others(app, Some(t("menu.hideOthers").as_str()))?,
            &PredefinedMenuItem::separator(app)?,
            &item("quit", &t("menu.quit"), "CmdOrCtrl+Q")?,
        ],
    )?;
    let task_menu = Submenu::with_items(
        app,
        t("menu.task"),
        true,
        &[
            &item("new-task", &t("menu.newTask"), "CmdOrCtrl+N")?,
            &item("add-project", &t("menu.addProject"), "CmdOrCtrl+O")?,
            &PredefinedMenuItem::separator(app)?,
            &item("review", &t("menu.review"), "CmdOrCtrl+Shift+R")?,
            &item("prev-task", &t("menu.prevTask"), "CmdOrCtrl+Alt+Up")?,
            &item("next-task", &t("menu.nextTask"), "CmdOrCtrl+Alt+Down")?,
            &PredefinedMenuItem::separator(app)?,
            &item("reveal", &t("menu.reveal"), "CmdOrCtrl+Shift+F")?,
            &item("close-task", &t("menu.closeTask"), "CmdOrCtrl+Shift+W")?,
        ],
    )?;
    let edit_menu = Submenu::with_items(
        app,
        t("menu.edit"),
        true,
        &[
            &PredefinedMenuItem::undo(app, Some(t("menu.undo").as_str()))?,
            &PredefinedMenuItem::redo(app, Some(t("menu.redo").as_str()))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, Some(t("menu.cut").as_str()))?,
            &PredefinedMenuItem::copy(app, Some(t("menu.copy").as_str()))?,
            &PredefinedMenuItem::paste(app, Some(t("menu.paste").as_str()))?,
            &PredefinedMenuItem::select_all(app, Some(t("menu.selectAll").as_str()))?,
            &PredefinedMenuItem::separator(app)?,
            &item("voice", &t("menu.voice"), "CmdOrCtrl+Shift+Space")?,
        ],
    )?;
    let window_menu = Submenu::with_items(
        app,
        t("menu.window"),
        true,
        &[
            &PredefinedMenuItem::minimize(app, Some(t("menu.minimize").as_str()))?,
            &PredefinedMenuItem::fullscreen(app, Some(t("menu.fullscreen").as_str()))?,
            &PredefinedMenuItem::close_window(app, Some(t("menu.closeWindow").as_str()))?,
        ],
    )?;
    Menu::with_items(app, &[&app_menu, &task_menu, &edit_menu, &window_menu])
}

/// 切换界面语言：重建菜单，审阅结论也改用这种语言。
pub fn apply_locale(app: &AppHandle, locale: &str) -> tauri::Result<()> {
    duolo_core::i18n::set_locale(locale);
    #[cfg(target_os = "macos")]
    app.set_menu(build_menu(app, locale)?)?;
    let state = app.state::<AppState>();
    state.engine().set_reply_language(duolo_core::i18n::language_name(locale));
    Ok(())
}
