//! 运行环境检查：git、Agent CLI 和语音是否可用。首页和 `duolo doctor` 共用。

use std::io::ErrorKind;
use std::thread;
use std::time::Duration;

use duolo_agent::Registry;
use duolo_core::process::{output_within, OneShot, Prompted};
use duolo_core::{t, tf, AgentKind, Config};

/// `--version` 最长等多久。
const VERSION_TIMEOUT: Duration = Duration::from_secs(10);

/// 检查的是什么。界面按它选择对应语言的文案。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckKind {
    Git,
    Agent,
    Voice,
}

/// 一项检查的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub kind: CheckKind,
    /// Agent 的种类，其他检查为空。
    pub agent: String,
    /// 显示名，例如 `git`、Agent 的名字。
    pub name: String,
    pub ok: bool,
    /// 检查的命令。
    pub command: String,
    /// 版本号，取不到时为空。
    pub version: String,
    /// 当前语言的说明（终端界面和 `duolo doctor` 使用）：版本号或问题。
    pub detail: String,
    /// 没安装时的安装命令（Agent 才有）。
    pub install: String,
}

/// 检查 git、每一种 Agent（默认的写代码和审阅 Agent 排在前面）、语音。
/// 会同时调用各个命令的 `--version`，一般一秒内，最长 [`VERSION_TIMEOUT`]，界面里请在后台线程调用。
pub fn check_environment(config: &Config, agents: &Registry) -> Vec<Check> {
    let mut kinds: Vec<AgentKind> = vec![config.writer.clone(), config.reviewer.clone()];
    kinds.extend(agents.iter().map(|plugin| plugin.kind()));
    let mut seen = std::collections::HashSet::new();
    kinds.retain(|k| seen.insert(k.clone()));
    let mut checks: Vec<Check> = thread::scope(|scope| {
        let git = scope.spawn(|| command_check(CheckKind::Git, "git", "git"));
        let found: Vec<_> = kinds.iter().map(|kind| scope.spawn(|| agent_check(agents, kind))).collect();
        std::iter::once(git).chain(found).filter_map(|check| check.join().ok()).collect()
    });
    checks.push(Check {
        kind: CheckKind::Voice,
        agent: String::new(),
        install: String::new(),
        name: t("env.voice"),
        ok: config.voice.is_configured(),
        command: config.voice.command.first().cloned().unwrap_or_default(),
        version: String::new(),
        detail: if config.voice.is_configured() { t("env.configured") } else { t("env.notConfigured") },
    });
    checks
}

fn agent_check(agents: &Registry, kind: &AgentKind) -> Check {
    match agents.get(kind) {
        Ok(plugin) => Check {
            agent: kind.0.clone(),
            install: plugin.install().to_string(),
            ..command_check(CheckKind::Agent, plugin.name(), plugin.command())
        },
        Err(_) => Check {
            kind: CheckKind::Agent,
            agent: kind.0.clone(),
            name: agents.name(kind),
            ok: false,
            command: kind.0.clone(),
            version: String::new(),
            detail: t("env.notInConfig"),
            install: String::new(),
        },
    }
}

fn command_check(kind: CheckKind, name: &str, command: &str) -> Check {
    check_within(kind, name, OneShot::from(vec![command.to_string(), "--version".into()]), VERSION_TIMEOUT)
}

fn check_within(kind: CheckKind, name: &str, command: OneShot, timeout: Duration) -> Check {
    let base = Check {
        kind,
        agent: String::new(),
        install: String::new(),
        name: name.into(),
        ok: false,
        command: command.program().into(),
        version: String::new(),
        detail: tf("env.missing", &[("command", &command.program())]),
    };
    let Some(prompted) = Prompted::new(&command, None) else { return base };
    match output_within(prompted, timeout) {
        Ok((status, out)) if status.success() => {
            let version = version_of(&out).unwrap_or_default();
            Check { ok: true, detail: version.clone(), version, ..base }
        }
        // 装了，只是版本号半天出不来（例如正在检查更新）。
        Err(err) if err.kind() == ErrorKind::TimedOut => Check { ok: true, detail: String::new(), ..base },
        _ => base,
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
        let check = command_check(CheckKind::Agent, "x", "duolo-no-such-command");
        assert!(!check.ok);
    }

    #[test]
    fn slow_command_does_not_hold_up_the_rest() {
        let slow = ["sh", "-c", "sleep 30"].map(String::from).to_vec();
        let started = std::time::Instant::now();
        let check = check_within(CheckKind::Agent, "x", slow.into(), Duration::from_millis(300));
        assert!(check.ok && check.version.is_empty(), "装了，只是没等到版本号");
        assert!(started.elapsed() < Duration::from_secs(8));
    }

    #[test]
    fn every_agent_is_checked() {
        let config: Config = toml::from_str(
            "writer = \"gone\"\nreviewer = \"codex\"\n[agents.claude]\ncommand = \"duolo-no-such-command\"\n\
             [agents.codex]\ncommand = \"git\"\n",
        )
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let (agents, _) = Registry::load(&config, dir.path());
        let checks = check_environment(&config, &agents);
        let found: Vec<_> = checks.iter().map(|c| (c.agent.as_str(), c.ok)).collect();
        assert_eq!(found, [("", true), ("gone", false), ("codex", true), ("claude", false), ("", false)]);
    }
}
