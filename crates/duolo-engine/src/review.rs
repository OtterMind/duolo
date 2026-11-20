//! 后台审阅：在任务的 worktree 里以只读方式运行审阅命令（默认 `codex exec`），读取它的结论。
//!
//! 审阅不占用界面上的 Agent 窗口，结束后结论保存在任务上，可以一键交给写代码的 Agent 修改。

use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use duolo_core::UserError;

/// 审阅最长运行时间，超过就结束它。
const TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// 一个正在运行的审阅。
#[derive(Debug)]
pub(crate) struct ReviewJob {
    /// Unix 秒。
    started_at: u64,
    child: Arc<Mutex<Child>>,
    result: Arc<Mutex<Option<Result<String, String>>>>,
}

impl ReviewJob {
    pub(crate) fn spawn(command: &[String], prompt: &str, cwd: PathBuf, env_remove: &[&str]) -> Result<Self> {
        let (program, args) = command.split_first().ok_or(UserError::ReviewNotConfigured)?;
        let mut cmd = Command::new(program);
        cmd.args(args).arg(prompt).current_dir(cwd).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        for key in env_remove {
            cmd.env_remove(key);
        }
        let mut child = cmd.spawn().map_err(|_| UserError::CommandNotFound { program: program.clone() })?;
        let mut stdout = child.stdout.take().context("review command has no stdout")?;
        let mut stderr = child.stderr.take().context("review command has no stderr")?;

        let child = Arc::new(Mutex::new(child));
        let result = Arc::new(Mutex::new(None));
        let job =
            Self { started_at: duolo_core::text::now_secs(), child: Arc::clone(&child), result: Arc::clone(&result) };

        thread::Builder::new().name("duolo-review".into()).spawn(move || {
            let out_reader = thread::spawn(move || {
                let mut buf = String::new();
                let _ = stdout.read_to_string(&mut buf);
                buf
            });
            let err_reader = thread::spawn(move || {
                let mut buf = String::new();
                let _ = stderr.read_to_string(&mut buf);
                buf
            });
            let started = Instant::now();
            let status = loop {
                let mut child = child.lock().unwrap_or_else(|p| p.into_inner());
                match child.try_wait() {
                    Ok(Some(status)) => break Some(status),
                    Ok(None) if started.elapsed() > TIMEOUT => {
                        let _ = child.kill();
                    }
                    Ok(None) => {}
                    Err(_) => break None,
                }
                drop(child);
                thread::sleep(Duration::from_millis(100));
            };
            let out = out_reader.join().unwrap_or_default();
            let err = err_reader.join().unwrap_or_default();
            let outcome = match status {
                Some(status) if status.success() && !out.trim().is_empty() => Ok(out.trim().to_string()),
                Some(status) if status.success() => Err("the review command produced no output".to_string()),
                _ => Err(failure_text(&err)),
            };
            *result.lock().unwrap_or_else(|p| p.into_inner()) = Some(outcome);
        })?;
        Ok(job)
    }

    pub(crate) fn started_at(&self) -> u64 {
        self.started_at
    }

    /// 结束后返回结论或错误说明；还在运行时返回 None。
    pub(crate) fn take_result(&self) -> Option<Result<String, String>> {
        self.result.lock().unwrap_or_else(|p| p.into_inner()).take()
    }

    pub(crate) fn cancel(&self) {
        let _ = self.child.lock().unwrap_or_else(|p| p.into_inner()).kill();
    }
}

impl Drop for ReviewJob {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// 失败时取 stderr 最后几行作为说明。
fn failure_text(stderr: &str) -> String {
    let tail: Vec<&str> = stderr.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let tail = tail[tail.len().saturating_sub(4)..].join("\n");
    if tail.is_empty() {
        "the review did not finish".to_string()
    } else {
        format!("the review did not finish:\n{tail}")
    }
}
