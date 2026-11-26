//! 各个系统不一样的地方：在文件管理器 / 终端里打开目录、打开系统设置。

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

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

/// 用默认浏览器打开网址。只打开 http / https：终端里的输出是程序写的，不能借它打开本机的文件或别的程序。
pub fn open_url(url: &str) -> io::Result<()> {
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "only http and https links are opened"));
    }
    if cfg!(target_os = "macos") {
        return Command::new("open").arg(url).spawn().map(drop);
    }
    if cfg!(windows) {
        // 不经过 cmd：网址里的 & 会被它当成分隔符。
        return duolo_core::process::command("rundll32").arg("url.dll,FileProtocolHandler").arg(url).spawn().map(drop);
    }
    Command::new("xdg-open").arg(url).spawn().map(drop)
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

#[cfg(test)]
mod url_tests {
    use super::open_url;

    /// 终端输出里的链接只打开网页，不打开本机文件或别的程序。
    #[test]
    fn only_web_links_are_opened() {
        for url in ["file:///etc/passwd", "javascript:alert(1)", "ssh://host", "/Applications/Calculator.app"] {
            assert_eq!(open_url(url).unwrap_err().kind(), std::io::ErrorKind::InvalidInput, "{url}");
        }
    }
}
