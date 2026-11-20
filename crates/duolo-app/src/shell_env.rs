//! 从 Dock 或 Finder 启动的应用只有系统默认的 PATH（/usr/bin:/bin…），找不到装在
//! Homebrew、~/.local/bin 等位置的 claude 和 codex。启动时从用户的登录 shell 读取 PATH。

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const MARKER: &str = "__DUOLO_PATH__";
const TIMEOUT: Duration = Duration::from_secs(5);

/// 用登录 shell 的 PATH 替换当前进程的 PATH。读取失败时保持不变。
pub fn load_login_path() {
    if let Some(path) = login_path() {
        std::env::set_var("PATH", path);
    }
}

fn login_path() -> Option<String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    // 交互式登录 shell 才会读 .zshrc；输出前后加标记，避开 shell 启动时打印的其他内容。
    let mut child = Command::new(shell)
        .args(["-ilc", &format!("printf '{MARKER}%s{MARKER}' \"$PATH\"")])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + TIMEOUT;
    while child.try_wait().ok()?.is_none() {
        if Instant::now() > deadline {
            let _ = child.kill();
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = child.wait_with_output().ok()?;
    parse(&String::from_utf8_lossy(&output.stdout))
}

fn parse(stdout: &str) -> Option<String> {
    let path = stdout.split(MARKER).nth(1)?.trim();
    (!path.is_empty()).then(|| path.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_shell_noise() {
        let out = format!("welcome!\n{MARKER}/opt/homebrew/bin:/usr/bin{MARKER}");
        assert_eq!(parse(&out).as_deref(), Some("/opt/homebrew/bin:/usr/bin"));
        assert_eq!(parse("nothing"), None);
    }
}
