//! 运行外部命令并读取 stdout，支持超时。

use std::io::Read;
use std::process::{Child, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use duolo_core::process::Prompted;
use duolo_core::proctree::ProcessTree;
use duolo_core::UserError;

/// 超时后结束命令，最长等它消失多久。
const KILL_WAIT: Duration = Duration::from_secs(2);

/// 等待已启动的子进程结束并返回 stdout；超过 `timeout` 时结束它（和它的子进程）并报错。
fn wait_output(mut child: Child, tree: &ProcessTree, timeout: Duration) -> Result<String> {
    let mut stdout = child.stdout.take().context("child process has no stdout")?;
    let reader = thread::spawn(move || {
        let mut buf = String::new();
        let _ = stdout.read_to_string(&mut buf);
        buf
    });

    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            tree.kill(None, KILL_WAIT);
            let _ = child.kill();
            let _ = child.wait();
            bail!("command did not finish within {} seconds", timeout.as_secs());
        }
        thread::sleep(Duration::from_millis(50));
    };

    let out = reader.join().unwrap_or_default();
    if !status.success() {
        bail!("command exited with {status}");
    }
    Ok(out.trim().to_string())
}

/// 运行 `argv`（可额外带一段提示词），等待输出。
pub(crate) fn run(argv: &[String], prompt: Option<&str>, timeout: Duration) -> Result<String> {
    let mut command = Prompted::new(argv, prompt).context("empty command")?;
    command.cmd.stdout(Stdio::piped()).stderr(Stdio::null());
    let (child, tree) = command.spawn().map_err(|_| UserError::CommandNotFound { program: argv[0].clone() })?;
    wait_output(child, &tree, timeout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_stdout() {
        let out = run(&["echo".into(), "hi".into()], Some("there"), Duration::from_secs(5)).unwrap();
        assert_eq!(out, "hi there");
    }

    #[test]
    fn times_out() {
        let err = run(&["sleep".into(), "5".into()], None, Duration::from_millis(200)).unwrap_err();
        assert!(err.to_string().contains("did not finish"));
    }
}
