//! 运行环境检查：git、Agent CLI 和语音是否可用。首页和 `duolo doctor` 共用。

use std::process::{Command, Stdio};

use duolo_core::{AgentKind, Config};

/// 一项检查的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub name: String,
    pub ok: bool,
    /// 版本号或说明。
    pub detail: String,
}

/// 依次检查 git、写代码和审阅用的 Agent、语音。会调用各个命令的 `--version`，耗时约一秒，
/// 界面里请在后台线程调用。
pub fn check_environment(config: &Config) -> Vec<Check> {
    let mut checks = vec![command_check("git", "git")];
    let mut kinds: Vec<&AgentKind> = vec![&config.writer];
    if config.reviewer != config.writer {
        kinds.push(&config.reviewer);
    }
    for kind in kinds {
        checks.push(match config.profile(kind) {
            Some(profile) => command_check(&kind.display_name(), &profile.command),
            None => Check { name: kind.display_name(), ok: false, detail: "配置里没有".into() },
        });
    }
    checks.push(Check {
        name: "语音".into(),
        ok: config.voice.is_configured(),
        detail: if config.voice.is_configured() { "已配置".into() } else { "未配置".into() },
    });
    checks
}

fn command_check(name: &str, command: &str) -> Check {
    let output = Command::new(command).arg("--version").stdin(Stdio::null()).stderr(Stdio::null()).output();
    match output {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            Check { name: name.into(), ok: true, detail: version_of(&text).unwrap_or_default() }
        }
        _ => Check { name: name.into(), ok: false, detail: format!("没有找到 {command}") },
    }
}

/// 从 `--version` 的输出里取出版本号，例如 `codex-cli 0.169.1` → `0.169.1`。
fn version_of(text: &str) -> Option<String> {
    text.lines()
        .next()?
        .split_whitespace()
        .find(|w| w.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .map(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(version_of("codex-cli 0.169.1\n").as_deref(), Some("0.169.1"));
        assert_eq!(version_of("2.1.311 (Claude Code)").as_deref(), Some("2.1.311"));
        assert_eq!(version_of("git version 2.50.1 (Apple Git-155)").as_deref(), Some("2.50.1"));
        assert_eq!(version_of("no digits"), None);
    }

    #[test]
    fn missing_command() {
        let check = command_check("x", "duolo-no-such-command");
        assert!(!check.ok);
    }
}
