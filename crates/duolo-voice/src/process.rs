//! 运行外部命令并读取 stdout，支持超时。

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

/// 等待已启动的子进程结束并返回 stdout；超过 `timeout` 时结束它并报错。
pub(crate) fn wait_output(mut child: Child, timeout: Duration) -> Result<String> {
    let mut stdout = child.stdout.take().context("子进程没有 stdout")?;
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
            let _ = child.kill();
            let _ = child.wait();
            bail!("命令超过 {} 秒没有结束", timeout.as_secs());
        }
        thread::sleep(Duration::from_millis(50));
    };

    let out = reader.join().unwrap_or_default();
    if !status.success() {
        bail!("命令退出码 {status}");
    }
    Ok(out.trim().to_string())
}

/// 运行 `argv`（可额外追加一个参数），等待输出。
pub(crate) fn run(argv: &[String], extra_arg: Option<&str>, timeout: Duration) -> Result<String> {
    let (program, args) = argv.split_first().context("命令为空")?;
    let mut cmd = Command::new(program);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    if let Some(arg) = extra_arg {
        cmd.arg(arg);
    }
    let child = cmd.spawn().with_context(|| format!("无法启动 {program}"))?;
    wait_output(child, timeout)
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
        assert!(err.to_string().contains("没有结束"));
    }
}
