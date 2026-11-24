//! 按住说话的快捷键，Duolo 在后台时也有效。
//!
//! - macOS：按住 Fn。用系统级的按键监听（CGEventTap）感知 Fn 的按下和松开，需要"辅助功能"权限，
//!   还没有权限时后台每隔几秒重试一次，用户在系统设置里打开后自动生效。
//! - Windows / Linux：没有能单独监听的 Fn 键，用全局快捷键 Ctrl+Shift+Space（按住说话，轻按开始 / 结束）。
//!   Linux 上全局快捷键只在 X11 下有效。
//!
//! 事件通过 `fn-key` 发给界面，内容是：
//! - `down` / `up`：Fn 按下、松开；
//! - `space`：录音中按住 Fn 时按了空格（锁定录音，松手继续录），这个空格不会传给其他程序；
//! - `chord`：按住 Fn 时按了别的键（例如 Fn+↑ 翻页），这次按 Fn 不是要说话。

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// 没有权限时重试的间隔。
const RETRY_INTERVAL: Duration = Duration::from_secs(3);

static ACTIVE: AtomicBool = AtomicBool::new(false);
/// 正在录音。只有录音中 Fn+空格才会被拦下，其他时候按键原样传给系统。
static RECORDING: AtomicBool = AtomicBool::new(false);
static STARTED: AtomicBool = AtomicBool::new(false);

/// Fn 说话是否可用，以及不可用的原因。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FnKeyStatus {
    /// 监听已经生效。
    pub active: bool,
    /// 已有辅助功能权限。
    pub trusted: bool,
    /// 系统设置里"按下 🌐 键时"不是"不执行任何操作"，按 Fn 会同时弹出表情或切换输入法。
    pub conflict: bool,
}

pub fn status() -> FnKeyStatus {
    FnKeyStatus { active: ACTIVE.load(Ordering::SeqCst), trusted: platform::trusted(false), conflict: fn_conflict() }
}

/// 录音开始和结束时调用。
pub fn set_recording(recording: bool) {
    RECORDING.store(recording, Ordering::SeqCst);
}

/// 弹出系统的辅助功能授权提示。
pub fn request_permission() {
    platform::trusted(true);
}

/// 启动监听线程（只启动一次）。
pub fn start(app: AppHandle) {
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    thread::spawn(move || loop {
        // 正常情况下 run 不会返回；返回说明还没有权限，过一会儿再试。
        platform::run(&app);
        thread::sleep(RETRY_INTERVAL);
    });
}

/// `defaults read com.apple.HIToolbox AppleFnUsageType`：0 表示按 🌐 键不执行任何操作。
/// 没有这一项时是系统默认行为（弹出表情或切换输入法），也算冲突。只有 macOS 有这个问题。
fn fn_conflict() -> bool {
    if !cfg!(target_os = "macos") {
        return false;
    }
    let out = std::process::Command::new("defaults").args(["read", "com.apple.HIToolbox", "AppleFnUsageType"]).output();
    match out {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).trim() != "0",
        _ => cfg!(target_os = "macos"),
    }
}

fn emit(app: &AppHandle, what: &str) {
    let _ = app.emit("fn-key", what);
}

// 系统按键监听和辅助功能权限只有 C 接口，这里是整个项目里唯一需要 unsafe 的地方。
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod platform {
    use super::*;
    use core_foundation::base::TCFType;
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
    use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
    use core_foundation::string::CFString;
    use core_graphics::event::{
        CGEventField, CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
        CGEventType, CallbackResult, EventField,
    };
    use std::sync::atomic::AtomicPtr;

    /// Fn 键（kVK_Function）。
    const KEY_FN: i64 = 63;
    /// 空格（kVK_Space）。
    const KEY_SPACE: i64 = 49;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
    }
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventTapEnable(tap: *mut std::ffi::c_void, enable: bool);
    }

    /// 当前的监听端口，被系统停用（超时）时用来重新启用。
    static PORT: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());

    pub fn trusted(prompt: bool) -> bool {
        let key = CFString::new("AXTrustedCheckOptionPrompt");
        let value = if prompt { CFBoolean::true_value() } else { CFBoolean::false_value() };
        let options = CFDictionary::from_CFType_pairs(&[(key.as_CFType(), value.as_CFType())]);
        unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef()) }
    }

    const KEYCODE: CGEventField = EventField::KEYBOARD_EVENT_KEYCODE;
    /// 已经说过在等授权了。
    static WAITING: AtomicBool = AtomicBool::new(false);

    pub fn run(app: &AppHandle) {
        if !trusted(false) {
            // 只在第一次说，免得每 3 秒刷一行。
            if !WAITING.swap(true, Ordering::SeqCst) {
                eprintln!("fn key: waiting for the Accessibility permission");
            }
            return;
        }
        let held = AtomicBool::new(false);
        let app = app.clone();
        let tap = CGEventTap::new(
            CGEventTapLocation::Session,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::Default,
            vec![CGEventType::FlagsChanged, CGEventType::KeyDown],
            move |_proxy, kind, event| {
                match kind {
                    CGEventType::FlagsChanged if event.get_integer_value_field(KEYCODE) == KEY_FN => {
                        let down = event.get_flags().contains(CGEventFlags::CGEventFlagSecondaryFn);
                        eprintln!("fn key: {}", if down { "down" } else { "up" });
                        if held.swap(down, Ordering::SeqCst) != down {
                            emit(&app, if down { "down" } else { "up" });
                        }
                    }
                    CGEventType::KeyDown if held.load(Ordering::SeqCst) => {
                        if event.get_integer_value_field(KEYCODE) == KEY_SPACE && RECORDING.load(Ordering::SeqCst) {
                            emit(&app, "space");
                            return CallbackResult::Drop;
                        }
                        emit(&app, "chord");
                    }
                    CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput => {
                        let port = PORT.load(Ordering::SeqCst);
                        if !port.is_null() {
                            unsafe { CGEventTapEnable(port, true) };
                        }
                    }
                    _ => {}
                }
                CallbackResult::Keep
            },
        );
        let Ok(tap) = tap else {
            eprintln!("fn key: cannot listen to the keyboard");
            return;
        };
        let Ok(source) = tap.mach_port().create_runloop_source(0) else { return };
        PORT.store(tap.mach_port().as_concrete_TypeRef() as *mut _, Ordering::SeqCst);
        CFRunLoop::get_current().add_source(&source, unsafe { kCFRunLoopCommonModes });
        tap.enable();
        ACTIVE.store(true, Ordering::SeqCst);
        eprintln!("fn key: listening");
        CFRunLoop::run_current();
        ACTIVE.store(false, Ordering::SeqCst);
        PORT.store(std::ptr::null_mut(), Ordering::SeqCst);
    }
}

/// Windows / Linux：全局快捷键 Ctrl+Shift+Space，按下发 `down`，松开发 `up`。
#[cfg(not(target_os = "macos"))]
mod platform {
    use super::*;
    use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

    /// 不需要额外的系统权限。
    pub fn trusted(_prompt: bool) -> bool {
        true
    }

    pub fn run(app: &AppHandle) {
        let shortcut = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Space);
        let plugin = tauri_plugin_global_shortcut::Builder::new()
            .with_handler(move |app, pressed, event| {
                if *pressed == shortcut {
                    emit(app, if event.state() == ShortcutState::Pressed { "down" } else { "up" });
                }
            })
            .build();
        if app.plugin(plugin).is_ok() && app.global_shortcut().register(shortcut).is_ok() {
            ACTIVE.store(true, Ordering::SeqCst);
        }
        // 注册失败（快捷键被别的程序占用）也不重试：麦克风按钮照样能用。
        loop {
            thread::park();
        }
    }
}
