//! 要花好几秒的操作：建 worktree、关任务。界面和后台循环共用一个加锁的 [`Engine`]，
//! 拿着锁做这些事，所有任务的按键和画面都会跟着卡住，所以分成几步，只在改状态时拿锁。

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use anyhow::Result;
use duolo_core::{AgentKey, AgentKind, Command, CommandResult, ProjectId, TaskId, UserError};

use crate::engine::{Closed, Engine, TaskOptions};

fn lock(engine: &Mutex<Engine>) -> MutexGuard<'_, Engine> {
    engine.lock().unwrap_or_else(|p| p.into_inner())
}

/// 新建任务，见 [`Engine::create_task_with`]。
pub fn create_task(engine: &Mutex<Engine>, project: &ProjectId, prompt: &str, opts: &TaskOptions) -> Result<TaskId> {
    let new = lock(engine).prepare_task(project, prompt, opts)?;
    new.check_out()?;
    lock(engine).finish_task(new)
}

/// 关闭任务，见 [`Engine::close_task`]。
pub fn close_task(engine: &Mutex<Engine>, id: TaskId) -> Result<Closed> {
    let mut closing = lock(engine).begin_close(id)?;
    let result = closing.run();
    lock(engine).finish_close(id, result)
}

/// 换写代码的 Agent，见 [`Engine::switch_writer`]。记下代码样子的 git 不拿着锁做。
pub fn switch_writer(engine: &Mutex<Engine>, id: TaskId, kind: AgentKind) -> Result<AgentKey> {
    let Some(worktree) = lock(engine).prepare_switch(id, &kind)? else { return lock(engine).writer_key(id) };
    let snapshot = duolo_git::snapshot_commit(&worktree, crate::switch::SNAPSHOT_MESSAGE).ok();
    lock(engine).switch_writer_at(id, kind, snapshot)
}

/// 在项目里建一个仓库，见 [`Engine::add_repo`]。
pub fn add_repo(engine: &Mutex<Engine>, project: &ProjectId, name: Option<&str>) -> Result<PathBuf> {
    let dir = lock(engine).new_repo_dir(project, name)?;
    // 已有的文件夹可能很大，第一次提交要一会儿。
    duolo_git::init_repo(&dir)?;
    Ok(dir)
}

/// 执行别的设备发来的命令。
pub fn execute(engine: &Mutex<Engine>, command: Command) -> CommandResult {
    let result = match command {
        Command::CreateTask { project, prompt, repo, base, branch, writer, reviewer } => {
            let opts = TaskOptions {
                repo: repo.map(PathBuf::from),
                base,
                branch,
                writer: writer.map(AgentKind),
                reviewer: reviewer.map(AgentKind),
                surface: None,
            };
            create_task(engine, &ProjectId(project), &prompt, &opts).and_then(|id| {
                // 手机上建的任务没人在电脑前点开，直接启动 Agent。
                let mut engine = lock(engine);
                let key = engine.writer_key(id)?;
                engine.start_agent(key)?;
                Ok(Some(id))
            })
        }
        Command::SendMessage { task, text } => lock(engine).send_message(task, &text).map(|()| None),
        Command::Answer { task, option, prompt } => lock(engine).answer(task, option, prompt).map(|()| None),
        Command::Interrupt { task } => lock(engine).interrupt(task).map(|()| None),
        Command::StartReview { task } => lock(engine).start_review(task).map(|()| None),
        Command::SwitchWriter { task, agent } => {
            // 和新建任务一样：没人在电脑前点开，直接启动接手的 Agent。
            switch_writer(engine, task, AgentKind(agent)).and_then(|key| lock(engine).start_agent(key)).map(|()| None)
        }
        Command::SetReviewer { task, agent } => lock(engine).set_reviewer(task, AgentKind(agent)).map(|()| None),
        Command::CloseTask { task } => close_task(engine, task).map(|_| None),
    };
    match result {
        Ok(task) => CommandResult { ok: true, task, error: None },
        Err(err) => {
            let code = err.chain().find_map(|e| e.downcast_ref::<UserError>()).map_or("internal", UserError::code);
            CommandResult { ok: false, task: None, error: Some(code.to_string()) }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;
    use std::time::{Duration, Instant};

    use duolo_core::Paths;

    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let out = std::process::Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    }

    /// 做 `work` 的同时不停地拿锁，返回最久等了多久。
    fn longest_wait(engine: &Mutex<Engine>, work: impl FnOnce()) -> Duration {
        let done = AtomicBool::new(false);
        thread::scope(|scope| {
            let probe = scope.spawn(|| {
                let mut longest = Duration::ZERO;
                while !done.load(Ordering::Relaxed) {
                    let asked = Instant::now();
                    lock(engine).poll_events();
                    longest = longest.max(asked.elapsed());
                    thread::sleep(Duration::from_millis(5));
                }
                longest
            });
            // `work` 里断言失败时也要让探测线程停下来，不然测试会一直卡住。
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work));
            done.store(true, Ordering::Relaxed);
            let longest = probe.join().unwrap();
            result.unwrap_or_else(|panic| std::panic::resume_unwind(panic));
            longest
        })
    }

    #[test]
    fn slow_git_does_not_hold_the_engine() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("shop");
        std::fs::create_dir(&repo).unwrap();
        std::fs::write(repo.join("a.txt"), "a\n").unwrap();
        duolo_git::init_repo(&repo).unwrap();
        // 让检出、统计改动、保存都要一秒多：大仓库就是这样。
        std::fs::write(repo.join(".git/slow.sh"), "#!/bin/sh\nsleep 1.2\ncat\n").unwrap();
        std::fs::create_dir_all(repo.join(".git/info")).unwrap();
        std::fs::write(repo.join(".git/info/attributes"), "*.txt filter=slow\n").unwrap();
        let script = format!("sh {}", repo.join(".git/slow.sh").display());
        for side in ["clean", "smudge"] {
            git(&repo, &["config", &format!("filter.slow.{side}"), &script]);
        }
        let home = dir.path().join("home");
        std::fs::create_dir(&home).unwrap();
        let config = "[agents.claude]\ncommand = \"sh\"\nargs = [\"-c\", \"cat\", \"agent\"]\n";
        std::fs::write(home.join("config.toml"), config).unwrap();
        let mut opened = Engine::open(Paths::at(&home)).unwrap();
        let project = opened.add_project(&repo).unwrap();
        let engine = Mutex::new(opened);

        let mut task = None;
        let started = Instant::now();
        let waited = longest_wait(&engine, || {
            task = Some(create_task(&engine, &project, "Export orders", &TaskOptions::default()).unwrap());
        });
        assert!(started.elapsed() > Duration::from_secs(1), "检出应该是慢的");
        assert!(waited < Duration::from_millis(500), "建任务时等了 {waited:?}");

        let task = task.unwrap();
        let worktree = lock(&engine).state().task(task).unwrap().worktree.clone();
        std::fs::write(worktree.join("a.txt"), "changed\n").unwrap();
        {
            let mut engine = lock(&engine);
            let key = engine.writer_key(task).unwrap();
            engine.start_agent(key).unwrap();
        }
        let waited = longest_wait(&engine, || thread::sleep(Duration::from_secs(3)));
        assert!(waited < Duration::from_millis(500), "统计改动时等了 {waited:?}");

        let mut closed = None;
        let waited = longest_wait(&engine, || {
            thread::scope(|scope| {
                let closing = scope.spawn(|| close_task(&engine, task));
                thread::sleep(Duration::from_millis(300));
                // 关到一半：不能再启动它的 Agent，也不能再关一次。
                let mut guard = lock(&engine);
                assert!(guard.is_closing(task));
                let key = guard.writer_key(task).unwrap();
                for err in [guard.start_agent(key).unwrap_err(), guard.begin_close(task).unwrap_err()] {
                    assert!(matches!(err.downcast_ref(), Some(UserError::TaskClosing)), "{err:#}");
                }
                drop(guard);
                closed = Some(closing.join().unwrap().unwrap());
            });
        });
        assert!(waited < Duration::from_millis(500), "关任务时等了 {waited:?}");
        assert!(closed.unwrap().saved_to.is_some());
        assert!(!lock(&engine).is_closing(task) && !worktree.exists());
    }
}
