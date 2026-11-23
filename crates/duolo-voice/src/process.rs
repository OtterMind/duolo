//! 运行外部命令并读取 stdout，支持超时。

use std::io::ErrorKind;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use duolo_core::process::{output_within, OneShot, Prompted};
use duolo_core::UserError;

/// 运行 `command`（可额外带一段提示词），等待输出。
pub(crate) fn run(command: &OneShot, prompt: Option<&str>, timeout: Duration) -> Result<String> {
    let prompted = Prompted::new(command, prompt).context("empty command")?;
    let (status, out) = match output_within(prompted, timeout) {
        Ok(done) => done,
        Err(err) if err.kind() == ErrorKind::TimedOut => {
            bail!("command did not finish within {} seconds", timeout.as_secs())
        }
        Err(_) => return Err(UserError::CommandNotFound { program: command.program().into() }.into()),
    };
    if !status.success() {
        bail!("command exited with {status}");
    }
    Ok(out.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(words: &[&str]) -> OneShot {
        words.iter().map(|w| w.to_string()).collect::<Vec<_>>().into()
    }

    #[test]
    fn captures_stdout() {
        let out = run(&argv(&["echo", "hi"]), Some("there"), Duration::from_secs(5)).unwrap();
        assert_eq!(out, "hi there");
    }

    #[test]
    fn times_out() {
        let err = run(&argv(&["sleep", "5"]), None, Duration::from_millis(200)).unwrap_err();
        assert!(err.to_string().contains("did not finish"));
    }
}
