//! 任务中途换 Agent：换写代码的，或者换审阅的。
//!
//! 换写代码的 Agent 时，原来的停下，新的在同一个 worktree 里接着做：它拿到一份交接说明，
//! 里面有需求、最近的对话，并让它先看 git 里已经改了什么。原来那个记在 `Task::retired` 里，
//! 事件里它的编号还能对上；之后再换回同一种 Agent 也是新开一次会话，带着新的交接说明。

use anyhow::Result;
use duolo_core::text::truncate_chars;
use duolo_core::{AgentKey, AgentKind, AgentRole, AgentSpec, EventKind, Speaker, Task, TaskId, UserError};

use crate::engine::Engine;

/// 交接说明里最多带几条最近的对话。
const HANDOFF_MESSAGES: usize = 20;
/// 每条对话最多带多少字。
const HANDOFF_MESSAGE_CHARS: usize = 1500;
/// 带上的对话一共最多多少字（太长的命令行在 Windows 上会启动失败）。
const HANDOFF_CHARS: usize = 10_000;

impl Engine {
    /// 换写代码的 Agent：停下原来的，让 `kind` 在同一个 worktree 里接着做。返回新的 Agent。
    /// 已经是它时什么也不做。
    pub fn switch_writer(&mut self, id: TaskId, kind: AgentKind) -> Result<AgentKey> {
        self.check_open(id)?;
        let plugin = self.agents().get(&kind)?;
        if !plugin.is_installed() {
            return Err(UserError::CommandNotFound { program: plugin.command().into() }.into());
        }
        let old = self.writer_key(id)?;
        let task = self.state.task(id).ok_or(UserError::TaskNotFound)?;
        let current = task.agent(old.agent).ok_or(UserError::AgentNotRunning)?;
        if current.kind == kind {
            return Ok(old);
        }
        let handoff = self.handoff(task, current);

        let new_id = self.update(|state| {
            let t = state.task_mut(id).ok_or(UserError::TaskNotFound)?;
            let at = t.agents.iter().position(|a| a.id == old.agent).ok_or(UserError::AgentNotRunning)?;
            let next =
                AgentSpec { prompt: handoff, ..AgentSpec::new(t.next_agent_id(), kind.clone(), AgentRole::Writer) };
            let retired = std::mem::replace(&mut t.agents[at], next);
            let new_id = t.agents[at].id;
            t.retired.push(retired);
            Ok(new_id)
        })?;
        let key = AgentKey { task: id, agent: new_id };
        self.stop_agent(old);
        self.resuming.remove(&old);
        self.session_ids.remove(&old);
        self.hand_over(old, key);
        self.record(id, EventKind::WriterChanged { from: old.agent, to: new_id, kind: kind.0 });
        self.start_agent(key)?;
        Ok(key)
    }

    /// 换审阅的 Agent，下一次审阅起生效。正在进行的审阅不受影响。
    pub fn set_reviewer(&mut self, id: TaskId, kind: AgentKind) -> Result<()> {
        // 不能审阅的（插件没写审阅命令）不让选；config.toml 里整条换掉了审阅命令时谁都可以。
        if self.config().review_command.is_empty() {
            self.agents().review(&kind)?;
        } else {
            self.agents().get(&kind)?;
        }
        if self.reviewer_of(id)? == kind {
            return Ok(());
        }
        self.update(|state| {
            state.task_mut(id).ok_or(UserError::TaskNotFound)?.reviewer = Some(kind.clone());
            Ok(())
        })?;
        self.record(id, EventKind::ReviewerChanged { reviewer: kind.0 });
        Ok(())
    }

    /// 给接手的 Agent 的交接说明。任务还是空白的（没有需求、也没说过话）时为 None：直接开始。
    fn handoff(&self, task: &Task, from: &AgentSpec) -> Option<String> {
        let name = |agent: u32| {
            let spec = task.agents.iter().chain(&task.retired).find(|a| a.id == agent);
            spec.map_or_else(|| "Agent".to_string(), |a| self.agents().name(&a.kind))
        };
        let events = self.events_since(task.id, 0).unwrap_or_default();
        let mut lines: Vec<String> = events
            .into_iter()
            .filter_map(|e| match e.kind {
                EventKind::Message { speaker, agent, text } => {
                    let who = match speaker {
                        Speaker::User => "User".to_string(),
                        Speaker::Agent => name(agent),
                    };
                    Some(format!("[{who}] {}", truncate_chars(text.trim(), HANDOFF_MESSAGE_CHARS)))
                }
                _ => None,
            })
            .collect();
        lines = lines.split_off(lines.len().saturating_sub(HANDOFF_MESSAGES));
        while lines.iter().map(|l| l.chars().count()).sum::<usize>() > HANDOFF_CHARS && !lines.is_empty() {
            lines.remove(0);
        }
        if task.prompt.trim().is_empty() && lines.is_empty() {
            return None;
        }
        let commit = duolo_git::fork_point(&task.worktree, &task.base).unwrap_or_else(|_| task.base.clone());
        Some(handoff_text(&name(from.id), task, &lines, &commit))
    }
}

/// 交接说明。给 Agent 看的，用英文；用户的话原样带上，Agent 会照着用户的语言回答。
fn handoff_text(from: &str, task: &Task, conversation: &[String], commit: &str) -> String {
    let mut text = format!(
        "You are taking over a coding task from another agent ({from}). Its work so far is in this git worktree.\n"
    );
    if !task.prompt.trim().is_empty() {
        text.push_str(&format!("\nThe task:\n{}\n", task.prompt.trim()));
    }
    if !conversation.is_empty() {
        text.push_str("\nThe latest conversation with the user, oldest first:\n\n");
        text.push_str(&conversation.join("\n\n"));
        text.push('\n');
    }
    text.push_str(&format!(
        "\nRun `git status` and `git diff {commit}` to see everything changed since the task branched off {base} \
at commit {commit}. Continue from where the work is; do not start over. If it already looks finished, \
summarize the current state briefly and ask the user what to do next.",
        base = task.base
    ));
    text
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::{Duration, Instant};

    use duolo_core::{Paths, ProjectId};

    use super::*;
    use crate::status::AgentStatus;

    fn git(dir: &Path, args: &[&str]) {
        let out = std::process::Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    }

    /// 两种 Agent 都是 sh：把第一句话（最后一个参数）写进 worktree 里的文件，然后等着。
    /// 还有一种只在配置里写了的 `toy`，没有审阅命令。
    fn engine() -> (tempfile::TempDir, Engine, ProjectId) {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("shop");
        std::fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "init"]);
        let home = dir.path().join("home");
        std::fs::create_dir(&home).unwrap();
        let agent = |name: &str| {
            let script = format!("last=; for a; do last=$a; done; printf '%s' \"$last\" > started-{name}; cat");
            format!("[agents.{name}]\ncommand = \"sh\"\nargs = [\"-c\", {script:?}, \"agent\"]\n")
        };
        let config = format!("{}{}[agents.toy]\ncommand = \"sh\"\n", agent("claude"), agent("codex"));
        std::fs::write(home.join("config.toml"), config).unwrap();
        let mut engine = Engine::open(Paths::at(&home)).unwrap();
        engine.set_agent_home(dir.path().join("user-home"));
        let project = engine.add_project(&repo).unwrap();
        (dir, engine, project)
    }

    fn read_when_written(path: &Path) -> String {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(text) = std::fs::read_to_string(path) {
                if !text.is_empty() {
                    return text;
                }
            }
            assert!(Instant::now() < deadline, "{} was not written", path.display());
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn another_agent_takes_over_with_a_handoff() {
        let (_dir, mut engine, project) = engine();
        let id = engine.create_task(&project, "Export orders as CSV").unwrap();
        let first = engine.writer_key(id).unwrap();
        engine.start_agent(first).unwrap();
        let worktree = engine.state().task(id).unwrap().worktree.clone();
        assert_eq!(read_when_written(&worktree.join("started-claude")), "Export orders as CSV");
        engine.record(id, EventKind::Message { speaker: Speaker::User, agent: 0, text: "Use semicolons".into() });

        let next = engine.switch_writer(id, AgentKind("codex".into())).unwrap();
        assert_ne!(next, first);
        assert_eq!(engine.agent_status(first), AgentStatus::NotStarted, "原来的停了");
        let handoff = read_when_written(&worktree.join("started-codex"));
        assert!(handoff.contains("from another agent (Claude)"), "{handoff}");
        assert!(handoff.contains("Export orders as CSV") && handoff.contains("[User] Use semicolons"), "{handoff}");
        assert!(handoff.contains("git diff "), "{handoff}");

        let task = engine.state().task(id).unwrap();
        assert_eq!((task.agents.len(), task.agents[0].kind.0.as_str()), (1, "codex"));
        assert_eq!(task.retired.len(), 1);
        assert_eq!(task.retired[0].id, first.agent, "事件里的编号还能对上");
        assert_eq!(engine.writer_key(id).unwrap(), next);
        // 换回原来的那种：新编号，不重复用。
        let again = engine.switch_writer(id, AgentKind("claude".into())).unwrap();
        assert!(again.agent > next.agent);
        assert_eq!(engine.switch_writer(id, AgentKind("claude".into())).unwrap(), again, "已经是它了");
        engine.shutdown();
    }

    #[test]
    fn blank_task_starts_the_new_agent_blank() {
        let (_dir, mut engine, project) = engine();
        let id = engine.create_task(&project, "").unwrap();
        let key = engine.switch_writer(id, AgentKind("codex".into())).unwrap();
        let spec = engine.state().task(id).unwrap().agent(key.agent).unwrap().clone();
        assert!(spec.prompt.is_none());
        engine.shutdown();
    }

    #[test]
    fn cannot_switch_to_an_agent_that_is_missing() {
        let (_dir, mut engine, project) = engine();
        let id = engine.create_task(&project, "x").unwrap();
        let err = engine.switch_writer(id, AgentKind("nope".into())).unwrap_err();
        assert!(matches!(err.downcast_ref::<UserError>(), Some(UserError::UnknownAgent { .. })), "{err:#}");
        assert_eq!(engine.state().task(id).unwrap().agents[0].kind.0, "claude", "没换");
    }

    #[test]
    fn reviewer_can_be_changed() {
        let (_dir, mut engine, project) = engine();
        let id = engine.create_task(&project, "x").unwrap();
        engine.set_reviewer(id, AgentKind("claude".into())).unwrap();
        assert_eq!(engine.reviewer_of(id).unwrap().0, "claude");
        let err = engine.set_reviewer(id, AgentKind("toy".into())).unwrap_err();
        assert!(matches!(err.downcast_ref::<UserError>(), Some(UserError::ReviewNotConfigured)), "{err:#}");
        assert_eq!(engine.reviewer_of(id).unwrap().0, "claude");
    }
}
