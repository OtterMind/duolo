//! 后台审阅：在任务的 worktree 里以只读方式运行审阅命令（默认 `codex exec`），读取它的结论。
//!
//! 审阅不占用界面上的 Agent 窗口，结束后结论保存在任务上，可以一键交给写代码的 Agent 修改。

use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use duolo_core::process::Prompted;
use duolo_core::proctree::ProcessTree;
use duolo_core::UserError;

/// 审阅最长运行时间，超过就结束它。
const TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// 结束审阅命令时，最长等它消失多久。
const KILL_WAIT: Duration = Duration::from_secs(2);

#[derive(Debug)]
struct Running {
    child: Child,
    tree: ProcessTree,
}

/// 一个正在运行的审阅。
#[derive(Debug)]
pub(crate) struct ReviewJob {
    /// Unix 秒。
    started_at: u64,
    /// 还在运行的命令，结束后为 None。
    running: Arc<Mutex<Option<Running>>>,
    result: Arc<Mutex<Option<Result<String, String>>>>,
}

impl ReviewJob {
    pub(crate) fn spawn(command: &[String], prompt: &str, cwd: PathBuf, env_remove: &[&str]) -> Result<Self> {
        let mut review = Prompted::new(command, Some(prompt)).ok_or(UserError::ReviewNotConfigured)?;
        review.cmd.current_dir(cwd).stdout(Stdio::piped()).stderr(Stdio::piped());
        for key in env_remove {
            review.cmd.env_remove(key);
        }
        let (mut child, tree) =
            review.spawn().map_err(|_| UserError::CommandNotFound { program: command[0].clone() })?;
        let mut stdout = child.stdout.take().context("review command has no stdout")?;
        let mut stderr = child.stderr.take().context("review command has no stderr")?;

        let running = Arc::new(Mutex::new(Some(Running { child, tree })));
        let result = Arc::new(Mutex::new(None));
        let job = Self {
            started_at: duolo_core::text::now_secs(),
            running: Arc::clone(&running),
            result: Arc::clone(&result),
        };

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
                let mut slot = running.lock().unwrap_or_else(|p| p.into_inner());
                // 被取消了。
                let Some(current) = slot.as_mut() else { break None };
                match current.child.try_wait() {
                    Ok(Some(status)) => {
                        // 它留下的后台进程可能还占着输出管道，不结束就一直读不完。
                        if let Some(done) = slot.take() {
                            done.tree.kill(None, KILL_WAIT);
                        }
                        break Some(status);
                    }
                    Ok(None) if started.elapsed() > TIMEOUT => {
                        stop(slot.take());
                        break None;
                    }
                    Ok(None) => {}
                    Err(_) => break None,
                }
                drop(slot);
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

    /// 结束审阅，等它的进程都消失（退出应用时用）。
    pub(crate) fn stop(self) {
        let running = self.running.lock().unwrap_or_else(|p| p.into_inner()).take();
        stop(running);
    }

    pub(crate) fn cancel(&self) {
        let running = self.running.lock().unwrap_or_else(|p| p.into_inner()).take();
        // 等它消失要一会儿，不让调用方（拿着引擎的锁）等。
        if running.is_some() {
            thread::spawn(move || stop(running));
        }
    }
}

impl Drop for ReviewJob {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// 结束审阅命令和它启动的所有进程。
fn stop(running: Option<Running>) {
    if let Some(Running { mut child, tree }) = running {
        tree.kill(None, KILL_WAIT);
        let _ = child.kill();
        let _ = child.wait();
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
