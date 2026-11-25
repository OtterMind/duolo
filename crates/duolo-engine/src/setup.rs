//! 一键安装 Agent、登录账号：在一个单独的终端里运行插件声明的安装命令（`[install]`）或登录命令（`[account]`）。
//!
//! 安装命令是一行 shell 脚本（官方的 `curl … | sh`、`irm … | iex`），交给用户的 shell（Windows 上是 PowerShell）；
//! 登录直接运行 Agent 自己的命令，它会打开浏览器，有时要用户把授权码粘贴回终端。
//! 做法参考 T3 Code：在终端里运行，结束后界面重新检查环境。

use anyhow::Result;
use duolo_agent::Registry;
use duolo_core::{process, AgentKind, UserError};
use duolo_pty::{OutputSink, Rules, Session, SessionSpec, Size};

/// 做哪一步。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupStep {
    Install,
    Login,
}

/// 在终端里开始安装或登录，输出交给 `sink`。
pub fn start_setup(
    agents: &Registry,
    kind: &AgentKind,
    step: SetupStep,
    size: Size,
    sink: OutputSink,
) -> Result<Session> {
    let plugin = agents.get(kind)?;
    let (program, args) = match step {
        SetupStep::Install => shell(plugin.install()),
        SetupStep::Login => {
            let args = plugin.login_args().ok_or(UserError::CommandNotFound { program: plugin.command().into() })?;
            (plugin.command().to_string(), args.to_vec())
        }
    };
    let session = Session::spawn(SessionSpec {
        program,
        args,
        cwd: process::home_dir(),
        env: Vec::new(),
        env_remove: agents.env_remove(),
        size,
        rules: Rules::default(),
        on_output: None,
    })?;
    session.attach(sink);
    Ok(session)
}

/// 运行一行安装脚本的程序和参数。
fn shell(script: &str) -> (String, Vec<String>) {
    if cfg!(windows) {
        let args = ["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", script];
        return ("powershell".into(), args.map(String::from).to_vec());
    }
    let fallback = if cfg!(target_os = "macos") { "/bin/zsh" } else { "/bin/sh" };
    let shell = std::env::var("SHELL").unwrap_or_else(|_| fallback.into());
    (shell, vec!["-lc".into(), script.into()])
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn install_runs_in_the_users_shell() {
        let dir = tempfile::tempdir().unwrap();
        let (agents, _) = Registry::load(&duolo_core::Config::default(), dir.path());
        let (program, args) = shell("echo hi");
        assert!(program.ends_with("sh"), "{program}");
        assert_eq!(args, ["-lc", "echo hi"]);
        // 没有这个插件时什么也不运行。
        let size = Size::new(10, 40);
        assert!(start_setup(&agents, &AgentKind("gone".into()), SetupStep::Login, size, Box::new(|_| {})).is_err());
    }
}
