//! 任务里另开的终端：用户自己的 shell（见 `duolo_agent::Manifest::shell`），工作目录是任务的 worktree，
//! 不用跳到系统的终端软件。它不是 AI：不写需求、不参与交接，任务关掉时一起结束。
//!
//! 写代码的也可以就是终端（新建任务时选「终端」）：任务的需求就是要运行的命令，运行完退出码就是结果；
//! 没写命令时是一个能打字的 shell。这种任务和别的任务一样能并排看、交给 Agent、关闭。

use anyhow::Result;
use duolo_core::{AgentKey, AgentKind, AgentRole, AgentSpec, TaskId, UserError};

use crate::engine::Engine;

impl Engine {
    /// 在任务里开一个终端并启动它。返回它的编号。
    pub fn open_shell(&mut self, id: TaskId) -> Result<AgentKey> {
        self.check_open(id)?;
        let agent = self.update(|state| {
            let task = state.task_mut(id).ok_or(UserError::TaskNotFound)?;
            let agent = task.next_agent_id();
            task.agents.push(AgentSpec::new(agent, AgentKind::shell(), AgentRole::Shell));
            Ok(agent)
        })?;
        let key = AgentKey { task: id, agent };
        self.start_agent(key)?;
        Ok(key)
    }

    /// 关掉任务里另开的终端：结束它的 shell，从任务里去掉。记在换下来的 Agent 里，事件里的编号还能对上，
    /// 编号也不再分给新的。
    pub fn close_shell(&mut self, key: AgentKey) -> Result<()> {
        self.stop_agent(key);
        self.update(|state| {
            let task = state.task_mut(key.task).ok_or(UserError::TaskNotFound)?;
            let at = task
                .agents
                .iter()
                .position(|a| a.id == key.agent && a.role == AgentRole::Shell)
                .ok_or(UserError::AgentNotRunning)?;
            let closed = task.agents.remove(at);
            task.retired.push(closed);
            Ok(())
        })
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::path::Path;
    use std::time::{Duration, Instant};

    use duolo_core::Paths;

    use super::*;
    use crate::engine::TaskOptions;
    use crate::status::AgentStatus;

    fn git(dir: &Path, args: &[&str]) {
        let out = std::process::Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    }

    /// 用 /bin/sh 当用户的 shell，不读测试机器上用户自己的配置。
    fn engine() -> (tempfile::TempDir, Engine, duolo_core::ProjectId) {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("shop");
        std::fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "init"]);
        let home = dir.path().join("home");
        std::fs::create_dir(&home).unwrap();
        std::fs::write(home.join("config.toml"), "[agents.shell]\ncommand = \"/bin/sh\"\n").unwrap();
        let mut engine = Engine::open(Paths::at(&home)).unwrap();
        let project = engine.add_project(&repo).unwrap();
        (dir, engine, project)
    }

    fn wait(what: &str, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// 终端任务：需求就是命令，在任务的 worktree 里运行，运行完退出码就是结果。
    #[test]
    fn a_shell_task_runs_its_command_in_the_worktree() {
        let (_dir, mut engine, project) = engine();
        let opts = TaskOptions { writer: Some(AgentKind::shell()), ..Default::default() };
        let id = engine.create_task_with(&project, "pwd > where.txt && exit 3", &opts).unwrap();
        let worktree = engine.state().task(id).unwrap().worktree.clone();
        let key = engine.writer_key(id).unwrap();
        engine.start_agent(key).unwrap();
        wait("the command to finish", || matches!(engine.agent_status(key), AgentStatus::Exited(_)));
        let AgentStatus::Exited(exit) = engine.agent_status(key) else { unreachable!() };
        assert!(!exit.success, "退出码 3 是失败");
        let written = std::fs::read_to_string(worktree.join("where.txt")).unwrap();
        assert_eq!(Path::new(written.trim()).canonicalize().unwrap(), worktree.canonicalize().unwrap());

        // 再启动一次：重新运行这条命令，而不是打开一个空的 shell。
        std::fs::remove_file(worktree.join("where.txt")).unwrap();
        engine.start_agent(key).unwrap();
        wait("the command to run again", || worktree.join("where.txt").exists());
        engine.shutdown();
    }

    /// 任务里另开的终端：在 worktree 里能打字，关掉后从任务里去掉，写代码的 Agent 不受影响。
    #[test]
    fn a_task_can_open_and_close_its_own_terminals() {
        let (_dir, mut engine, project) = engine();
        let opts = TaskOptions { writer: Some(AgentKind::shell()), ..Default::default() };
        let id = engine.create_task_with(&project, "", &opts).unwrap();
        let worktree = engine.state().task(id).unwrap().worktree.clone();
        let writer = engine.writer_key(id).unwrap();

        let shell = engine.open_shell(id).unwrap();
        assert_ne!(shell, writer);
        assert!(engine.agent_status(shell).is_live());
        engine.write(shell, b"echo hello > typed.txt\r").unwrap();
        wait("the typed command", || worktree.join("typed.txt").exists());

        engine.close_shell(shell).unwrap();
        let task = engine.state().task(id).unwrap();
        assert_eq!(task.agents.iter().map(|a| a.id).collect::<Vec<_>>(), [writer.agent]);
        assert!(task.retired.iter().any(|a| a.id == shell.agent));
        assert_eq!(engine.agent_status(shell), AgentStatus::NotStarted);
        assert_ne!(engine.open_shell(id).unwrap(), shell, "编号不再用");
        engine.shutdown();
    }
}
