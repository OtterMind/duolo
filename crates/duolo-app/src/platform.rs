//! 各个系统不一样的地方：在文件管理器 / 终端里打开目录、打开系统设置、给 Agent 用的 `duolo` 命令。

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

/// 登记到 Duolo 服务器上的平台名（手机上按它显示图标）。
pub const PLATFORM: &str = if cfg!(target_os = "macos") {
    "mac"
} else if cfg!(windows) {
    "windows"
} else {
    "linux"
};

/// 这台电脑的名字（手机上显示）：macOS 上是"电脑名称"，别的系统是主机名。
pub fn device_name() -> String {
    let output = if cfg!(target_os = "macos") {
        duolo_core::process::command("scutil").args(["--get", "ComputerName"]).output()
    } else {
        duolo_core::process::command("hostname").output()
    };
    let name =
        output.ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    name.filter(|n| !n.is_empty()).unwrap_or_else(|| "Duolo".into())
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

/// 剪贴板里的文件或图片，变成要粘贴给 Agent 的路径：复制的文件给它的路径，复制的图片先存成临时 PNG。
/// 都没有（只是文字）时为空，照常粘贴文字。Agent（Claude Code、Codex）认得出图片路径，会当成附件。
///
/// 不让 Agent 自己读剪贴板：在访达里复制文件时，剪贴板里附带的图像是文件图标，它读到的就是图标。
pub fn clipboard_paths() -> io::Result<Vec<String>> {
    let dir = std::env::temp_dir().join("duolo-paste");
    std::fs::create_dir_all(&dir)?;
    let millis = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
    let image = dir.join(format!("{millis}.png"));
    let image = image.to_string_lossy().to_string();
    let output = if cfg!(target_os = "macos") {
        Command::new("osascript").args(["-l", "JavaScript", "-e", CLIPBOARD_SCRIPT, &image]).output()?
    } else if cfg!(windows) {
        let script = format!(
            "Add-Type -AssemblyName System.Windows.Forms, System.Drawing; \
             $f = [Windows.Forms.Clipboard]::GetFileDropList(); \
             if ($f.Count -gt 0) {{ $f }} elseif ([Windows.Forms.Clipboard]::ContainsImage()) {{ \
             [Windows.Forms.Clipboard]::GetImage().Save('{}', [Drawing.Imaging.ImageFormat]::Png); '{}' }}",
            image.replace('\'', "''"),
            image.replace('\'', "''"),
        );
        duolo_core::process::command("powershell")
            .args(["-NoProfile", "-NonInteractive", "-STA", "-Command", &script])
            .output()?
    } else {
        // Linux 先只粘贴文字。
        return Ok(Vec::new());
    };
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(text.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect())
}

/// 见 clipboard.js。osascript 用 `-e` 运行时，后面的参数就是 `run(argv)` 的 argv。
const CLIPBOARD_SCRIPT: &str = include_str!("clipboard.js");

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

/// 在 `dir` 里放一个叫 `duolo` 的小脚本，带上 `__cli` 运行桌面应用自己（`exe`）：Agent 的 PATH 里有这个目录，
/// 在对话里就能用 `duolo automation add` 之类的命令。每次启动都重写，应用换了位置也跟得上。
pub fn install_cli(dir: &Path, exe: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let exe = exe.display().to_string();
    if cfg!(windows) {
        // 批处理里 % 要写成 %%。
        let script = format!("@\"{}\" {} %*\r\n@exit /b %ERRORLEVEL%\r\n", exe.replace('%', "%%"), duolo_cli::CLI_ARG);
        return write_if_changed(&dir.join("duolo.cmd"), &script);
    }
    // 单引号里的单引号写成 '\''。
    let quoted = format!("'{}'", exe.replace('\'', "'\\''"));
    let script =
        format!("#!/bin/sh\n# Duolo 的命令行，由桌面应用生成。\nexec {quoted} {} \"$@\"\n", duolo_cli::CLI_ARG);
    let file = dir.join("duolo");
    write_if_changed(&file, &script)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

fn write_if_changed(file: &Path, text: &str) -> io::Result<()> {
    if std::fs::read_to_string(file).is_ok_and(|old| old == text) {
        return Ok(());
    }
    std::fs::write(file, text)
}

/// 当命令行用时接上启动它的终端：Windows 的正式版是窗口程序，不接的话输出看不到。其他系统什么也不做。
pub fn attach_console() {
    #[cfg(windows)]
    // SAFETY: 只是请求接上父进程的控制台，失败（本来就有、或父进程没有）时什么也不变。
    unsafe {
        windows_sys::Win32::System::Console::AttachConsole(windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS);
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

#[cfg(all(test, unix))]
mod cli_tests {
    use super::install_cli;

    /// 脚本带上 `__cli` 和原来的参数运行应用；路径里有空格、单引号也不出错。
    #[test]
    fn the_script_runs_the_app_as_cli() {
        let dir = tempfile::tempdir().unwrap();
        let app_dir = dir.path().join("It's an app");
        std::fs::create_dir(&app_dir).unwrap();
        let exe = app_dir.join("Duolo");
        std::fs::write(&exe, "#!/bin/sh\nprintf '%s|' \"$@\"\n").unwrap();
        let bin = dir.path().join("bin");
        for _ in 0..2 {
            install_cli(&bin, &exe).unwrap();
        }
        std::process::Command::new("chmod").arg("+x").arg(&exe).status().unwrap();
        let out = std::process::Command::new(bin.join("duolo")).args(["automation", "add", "a b"]).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout), "__cli|automation|add|a b|");
    }
}
