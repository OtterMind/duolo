//! 一键安装、升级 Agent，登录账号：在一个单独的终端里运行插件声明的命令（`[install]`、`[account]`）。
//!
//! 安装命令是一行 shell 脚本（官方的 `curl … | sh`、`irm … | iex`），交给用户的 shell（Windows 上是 PowerShell）；
//! 登录直接运行 Agent 自己的命令，它会打开浏览器，有时要用户把授权码粘贴回终端。
//! 做法参考 T3 Code：在终端里运行，结束后界面重新检查环境。

use anyhow::Result;
use duolo_agent::{Account, Registry};
use duolo_core::{process, AgentKind, UserError};
use duolo_pty::{OutputSink, Rules, Session, SessionSpec, Size};

/// 做哪一步。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupStep {
    Install,
    /// 太旧时用它自己的升级命令：照原来的安装方式（npm、Homebrew、安装脚本）升级，不会再装一份（参考 Orca）。
    Update,
    Login,
}

/// 在终端里开始安装、升级或登录，输出交给 `sink`。
pub fn start_setup(
    agents: &Registry,
    kind: &AgentKind,
    step: SetupStep,
    account: Option<&Account>,
    size: Size,
    sink: OutputSink,
) -> Result<Session> {
    let plugin = agents.get(kind)?;
    let missing = || UserError::CommandNotFound { program: plugin.command().into() };
    let (program, args, env) = match step {
        SetupStep::Install => {
            let (program, args) = shell(plugin.install_runnable().ok_or_else(missing)?);
            (program, args, Vec::new())
        }
        SetupStep::Update => {
            let argv = plugin.update_command(plugin.version().as_deref()).ok_or_else(missing)?;
            (argv[0].clone(), argv[1..].to_vec(), plugin.start_env())
        }
        // 带着启动时的环境变量：登录到和平时运行时同一处，也不趁登录自动升级（Orca 的 loginEnv）。
        SetupStep::Login => {
            let args = plugin.login_args().ok_or_else(missing)?;
            (plugin.command().to_string(), args.to_vec(), plugin.start_env())
        }
    };
    // 登录某个账号：在它的文件夹里登录（见 `accounts.rs`）。
    let env = env.into_iter().chain(account.map(|a| plugin.account_vars(a)).unwrap_or_default()).collect();
    let session = Session::spawn(SessionSpec {
        program,
        args,
        cwd: process::home_dir(),
        env,
        env_remove: agents.env_remove(),
        size,
        rules: Rules::default(),
        on_output: None,
    })?;
    session.attach(sink);
    Ok(session)
}

/// 现在登录了没有：登录时浏览器授权完，有的 CLI（Windows 上的 Codex）不会自己退出，界面靠这个判断已经好了。
/// 插件不知道怎么查、查不出来时为 None。要一小会儿，不要拿着引擎的锁调用。
pub fn signed_in(agents: &Registry, kind: &AgentKind) -> Option<bool> {
    let status = agents.get(kind).ok()?.account_status()?;
    crate::env::signed_in(duolo_core::process::OneShot { env_remove: agents.env_remove(), ..status })
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
        assert!(
            start_setup(&agents, &AgentKind("gone".into()), SetupStep::Login, None, size, Box::new(|_| {})).is_err()
        );
    }
}
