//! 后台审阅：把任务现在的改动检出到一份临时副本，在副本里以只读方式运行审阅 Agent 的命令，读取它的结论。
//!
//! 审阅命令本身是只读的（插件声明的沙箱、工具白名单），副本再兜一层：用户自己的设置放开了权限时，
//! 审阅者改了什么也只改在副本里，写代码的 Agent 和用户看到的 worktree 不受影响；审阅期间写代码的 Agent
//! 继续改，审阅看到的也是开始时的样子。审阅不占用界面上的 Agent 窗口，结束后结论保存在任务上，
//! 可以一键交给写代码的 Agent 修改。

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use duolo_core::process::{self, OneShot, Prompted};
use duolo_core::proctree::ProcessTree;
use duolo_core::{t, tf, UserError};

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
    /// 还在运行的命令，还没启动或者已经结束时为 None。
    running: Arc<Mutex<Option<Running>>>,
    /// 取消了：还在准备副本时取消的，准备好也不再启动。
    cancelled: Arc<AtomicBool>,
    result: Arc<Mutex<Option<Result<String, String>>>>,
}

impl ReviewJob {
    /// 在后台把 `worktree` 检出到 `copy`，在那里运行审阅命令，结束后删掉 `copy`。
    pub(crate) fn spawn(command: &OneShot, prompt: &str, worktree: PathBuf, copy: PathBuf) -> Result<Self> {
        let mut review = Prompted::new(command, Some(prompt)).ok_or(UserError::ReviewNotConfigured)?;
        // 先说找不到命令，免得白做一份副本。
        if process::find(command.program()).is_none() {
            return Err(UserError::CommandNotFound { program: command.program().into() }.into());
        }
        review.cmd.current_dir(&copy).stdout(Stdio::piped()).stderr(Stdio::piped());
        let running = Arc::new(Mutex::new(None));
        let cancelled = Arc::new(AtomicBool::new(false));
        let result = Arc::new(Mutex::new(None));
        let job = Self {
            started_at: duolo_core::text::now_secs(),
            running: Arc::clone(&running),
            cancelled: Arc::clone(&cancelled),
            result: Arc::clone(&result),
        };
        thread::Builder::new().name("duolo-review".into()).spawn(move || {
            let outcome = match duolo_git::snapshot(&worktree, &copy) {
                Ok(()) => run(review, &running, &cancelled).map(|out| own_paths(out, &copy, &worktree)),
                Err(err) => Err(tf("review.copyFailed", &[("detail", &format!("{err:#}"))])),
            };
            // 删不掉的下次启动时再删（remove_leftovers）。
            let _ = duolo_git::remove_snapshot(&copy);
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
        self.cancelled.store(true, Ordering::SeqCst);
        let running = self.running.lock().unwrap_or_else(|p| p.into_inner()).take();
        stop(running);
    }

    pub(crate) fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
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

/// 在准备好的副本里运行审阅命令，等它结束，返回结论或错误说明。
fn run(review: Prompted, running: &Mutex<Option<Running>>, cancelled: &AtomicBool) -> Result<String, String> {
    let mut slot = running.lock().unwrap_or_else(|p| p.into_inner());
    if cancelled.load(Ordering::SeqCst) {
        return Err(t("review.didNotFinish"));
    }
    let (mut child, tree) = review.spawn().map_err(|err| failure_text(&err.to_string()))?;
    let (Some(mut stdout), Some(mut stderr)) = (child.stdout.take(), child.stderr.take()) else {
        stop(Some(Running { child, tree }));
        return Err(t("review.didNotFinish"));
    };
    *slot = Some(Running { child, tree });
    drop(slot);

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
    match status {
        Some(status) if status.success() && !out.trim().is_empty() => Ok(out.trim().to_string()),
        Some(status) if status.success() => Err(t("review.noOutput")),
        _ => Err(failure_text(&err)),
    }
}

/// 结论里提到的副本路径换回任务的 worktree（有的 Agent 写的是解析过链接的真实路径）。
fn own_paths(text: String, copy: &Path, worktree: &Path) -> String {
    let mut text = text;
    let real = copy.canonicalize().ok().filter(|real| real != copy);
    for from in real.iter().map(PathBuf::as_path).chain([copy]) {
        text = text.replace(&*from.to_string_lossy(), &worktree.to_string_lossy());
    }
    text
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
        t("review.didNotFinish")
    } else {
        format!("{}\n{tail}", t("review.didNotFinish"))
    }
}

/// 审阅用的临时副本放在哪里：`<目录>/<开始时间>-<任务>-<序号>`，名字里的时间用来清理上次没删掉的。
pub(crate) fn copy_dir(dir: &Path, task: u64) -> PathBuf {
    let now = duolo_core::text::now_secs();
    (1..).map(|n| dir.join(format!("{now}-{task}-{n}"))).find(|p| !p.exists()).expect("a free name always exists")
}

/// 删掉上次没删掉的副本（应用中途退出时会留下）。还可能在用的（比审阅最长时间还新的）不动：别的进程可能正在审阅。
pub(crate) fn remove_leftovers(dir: &Path) {
    let stale: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .filter(|p| {
            let name = p.file_name().unwrap_or_default().to_string_lossy().into_owned();
            let started: Option<u64> = name.split('-').next().and_then(|s| s.parse().ok());
            started.is_some_and(|at| duolo_core::text::now_secs().saturating_sub(at) > TIMEOUT.as_secs() + 600)
        })
        .collect();
    if !stale.is_empty() {
        thread::spawn(move || {
            for copy in stale {
                let _ = duolo_git::remove_snapshot(&copy);
            }
        });
    }
}
