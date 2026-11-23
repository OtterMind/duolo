//! 各个系统不一样的地方：内置语音识别、在文件管理器 / 终端里打开目录、打开系统设置。

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// 应用的资源目录（Windows 上的语音识别脚本放在这里），启动时设置。
pub static RESOURCES: OnceLock<PathBuf> = OnceLock::new();

/// 随应用安装的语音识别命令，识别语言是 `locale`。没有内置识别时返回 None（Linux：用户自己配置 voice.command）。
pub fn bundled_dictate(locale: &str) -> Option<Vec<String>> {
    if cfg!(target_os = "macos") {
        // macOS：Speech 框架，见 dictate/main.swift，作为 externalBin 和主程序放在一起。
        let path = std::env::current_exe().ok()?.parent()?.join("duolo-dictate");
        let usable = std::fs::metadata(&path).is_ok_and(|m| m.len() > 0);
        return usable.then(|| vec![path.display().to_string(), "--locale".into(), locale.into()]);
    }
    if cfg!(windows) {
        // Windows：System.Speech 听写，见 dictate/dictate.ps1，作为资源随应用安装。
        let script = RESOURCES.get()?.join("dictate").join("dictate.ps1");
        return script.exists().then(|| {
            ["powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]
                .map(String::from)
                .into_iter()
                .chain([script.display().to_string(), "-locale".into(), locale.into()])
                .collect()
        });
    }
    None
}

/// 在系统的文件管理器里打开目录。
pub fn reveal(path: &Path) -> io::Result<()> {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    // Windows 的资源管理器不认 `C:/Users/…`（git 给出的写法），会打开"文档"。
    let path: PathBuf = path.components().collect();
    Command::new(program).arg(path).spawn().map(drop)
}

/// 在终端里打开目录：macOS 用"终端"，Windows 优先 Windows Terminal，Linux 依次试常见的终端。
pub fn open_terminal(path: &Path) -> io::Result<()> {
    if cfg!(target_os = "macos") {
        return Command::new("open").args(["-a", "Terminal"]).arg(path).spawn().map(drop);
    }
    if cfg!(windows) {
        if duolo_core::process::command("wt").arg("-d").arg(path).spawn().is_ok() {
            return Ok(());
        }
        return duolo_core::process::command("cmd").args(["/C", "start", "cmd"]).current_dir(path).spawn().map(drop);
    }
    let dir = path.display().to_string();
    let candidates: [(&str, Vec<String>); 4] = [
        ("x-terminal-emulator", vec![]),
        ("gnome-terminal", vec![format!("--working-directory={dir}")]),
        ("konsole", vec!["--workdir".into(), dir.clone()]),
        ("xterm", vec![]),
    ];
    for (program, args) in candidates {
        if which::which(program).is_ok() {
            return Command::new(program).args(args).current_dir(path).env("PWD", path).spawn().map(drop);
        }
    }
    Err(io::Error::new(io::ErrorKind::NotFound, "no terminal emulator found"))
}

/// 打开系统设置的对应页面（只有 macOS 有这些页面：麦克风、辅助功能、键盘等）。
pub fn open_settings(pane: &str) -> io::Result<()> {
    let url = match pane {
        "keyboard" => "x-apple.systempreferences:com.apple.Keyboard-Settings.extension",
        "accessibility" => "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
        "microphone" => "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone",
        "speech" => "x-apple.systempreferences:com.apple.preference.security?Privacy_SpeechRecognition",
        "sound" => "x-apple.systempreferences:com.apple.Sound-Settings.extension",
        _ => return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("unknown settings pane {pane}"))),
    };
    if cfg!(target_os = "macos") {
        Command::new("open").arg(url).spawn().map(drop)
    } else if cfg!(windows) {
        // Windows 的对应页面：麦克风隐私设置、声音设置。
        let page = if pane == "sound" { "ms-settings:sound" } else { "ms-settings:privacy-microphone" };
        duolo_core::process::command("cmd").args(["/C", "start", "", page]).spawn().map(drop)
    } else {
        Err(io::Error::new(io::ErrorKind::Unsupported, "system settings are not available"))
    }
}
