//! 任务中途换 Agent：换写代码的，或者换审阅的。
//!
//! 换写代码的 Agent 时，原来的停下，新的在同一个 worktree 里接着做：它拿到一份交接说明，
//! 里面有需求、最近的对话，并让它先看 git 里已经改了什么。原来那个记在 `Task::retired` 里，
//! 事件里它的编号还能对上，也记下它离开时代码的样子（[`Departure`]）。
//!
//! 换回一天之内换下来的同一种 Agent 时，接回它原来的那次会话（它记得自己做过的事），
//! 恢复后先告诉它这期间的对话和改动；接不回来（会话没了）就带着交接说明重新开始。
//!
//! 也可以分出一个新任务（[`Engine::fork_task`]）：照着原任务现在的代码新建分支和 worktree，
//! 另一个 Agent 带着同样的交接说明在里面接着做，原来的任务不停，两边并行。

use std::path::PathBuf;

use anyhow::Result;
use duolo_core::text::{derive_title, now_secs, slug, truncate_chars};
use duolo_core::{
    AgentKey, AgentKind, AgentRole, AgentSpec, Departure, EventKind, Speaker, Task, TaskEvent, TaskId, UserError,
};

use crate::engine::{Engine, NewTask};

/// 交接说明里最多带几条最近的对话。
const HANDOFF_MESSAGES: usize = 20;
/// 每条对话最多带多少字。
const HANDOFF_MESSAGE_CHARS: usize = 1500;
/// 带上的对话一共最多多少字（太长的命令行在 Windows 上会启动失败）。
const HANDOFF_CHARS: usize = 10_000;
/// 换下来多久以内换回来，接回它原来的会话（再久的话它记得的东西多半已经过时了）。
const REVIVE_WITHIN_SECS: u64 = 24 * 3600;
/// 记下离开时代码样子的提交说明。
pub(crate) const SNAPSHOT_MESSAGE: &str = "Duolo: work when the agent was switched";
/// 分出任务时记下原任务代码样子的提交说明。
pub(crate) const FORK_MESSAGE: &str = "Duolo: work when the task was forked";

impl Engine {
    /// 换写代码的 Agent：停下原来的，让 `kind` 在同一个 worktree 里接着做。返回接手的 Agent。
    /// 已经是它时什么也不做。会运行 git 记下现在代码的样子，大仓库要一会儿；
    /// 共用一个加锁的引擎时用 [`crate::ops::switch_writer`]，不拿着锁做这件事。
    ///
    /// 只是改状态：原来的 Agent 在哪个进程里跑，那个进程同步状态时停掉它（见 `adopt`），所以命令行也能换。
    /// 原来的在这个进程里跑时，接手的马上启动；否则等打开任务时启动。
    pub fn switch_writer(&mut self, id: TaskId, kind: AgentKind) -> Result<AgentKey> {
        self.switch_writer_with(id, kind, "")
    }

    /// 和 [`Engine::switch_writer`] 一样，再带上用户要接手的 Agent 做的事（`request`，可以为空）。
    pub fn switch_writer_with(&mut self, id: TaskId, kind: AgentKind, request: &str) -> Result<AgentKey> {
        let snapshot = match self.prepare_switch(id, &kind)? {
            Some(worktree) => duolo_git::snapshot_commit(&worktree, SNAPSHOT_MESSAGE).ok(),
            None => return self.writer_key(id),
        };
        self.switch_writer_at(id, kind, snapshot, request)
    }

    /// 换人前的检查。返回要记下样子的 worktree；已经是它时为 None。
    pub(crate) fn prepare_switch(&self, id: TaskId, kind: &AgentKind) -> Result<Option<PathBuf>> {
        self.check_open(id)?;
        let plugin = self.agents().get(kind)?;
        if !plugin.is_installed() {
            return Err(UserError::CommandNotFound { program: plugin.command().into() }.into());
        }
        let old = self.writer_key(id)?;
        let task = self.state.task(id).ok_or(UserError::TaskNotFound)?;
        let current = task.agent(old.agent).ok_or(UserError::AgentNotRunning)?;
        Ok((current.kind != *kind).then(|| task.worktree.clone()))
    }

    /// 换人。`snapshot` 是现在代码的样子（[`duolo_git::snapshot_commit`]），换回原来的 Agent 时告诉它改了什么。
    /// `request` 是用户要接手的 Agent 做的事，跟在交接说明后面。
    pub(crate) fn switch_writer_at(
        &mut self,
        id: TaskId,
        kind: AgentKind,
        snapshot: Option<String>,
        request: &str,
    ) -> Result<AgentKey> {
        if self.prepare_switch(id, &kind)?.is_none() {
            return self.writer_key(id);
        }
        let old = self.writer_key(id)?;
        let was_running = self.sessions.contains_key(&old) || self.chats.contains_key(&old);
        let task = self.state.task(id).ok_or(UserError::TaskNotFound)?;
        let current = task.agent(old.agent).ok_or(UserError::AgentNotRunning)?;
        let handoff = with_request(self.handoff(task, current), request);
        let revived = self.revivable(task, &kind);
        let note = revived.and_then(|r| r.left.as_ref()).map(|left| self.welcome_back(task, left));
        let note = with_request(note, request);
        let revived = revived.map(|r| r.id);
        // 接手的照旧在聊天模式下运行，它不支持时在终端里。
        let surface = self.surface_for(&kind, current.surface);
        let departure = Departure { at: now_secs(), seq: self.live.log.last_seq(id), commit: snapshot };

        let new_id = self.update(|state| {
            let t = state.task_mut(id).ok_or(UserError::TaskNotFound)?;
            let at = t.agents.iter().position(|a| a.id == old.agent).ok_or(UserError::AgentNotRunning)?;
            let next = match revived.and_then(|r| t.retired.iter().position(|a| a.id == r)) {
                // 接回原来的会话；接不回来时带着交接说明重新开始。
                Some(i) => AgentSpec { prompt: handoff, left: None, resume_note: note, ..t.retired.remove(i) },
                None => {
                    AgentSpec { prompt: handoff, ..AgentSpec::new(t.next_agent_id(), kind.clone(), AgentRole::Writer) }
                }
            };
            let next = AgentSpec { surface, ..next };
            let mut leaving = std::mem::replace(&mut t.agents[at], next);
            leaving.left = Some(departure);
            leaving.resume_note = None;
            t.retired.push(leaving);
            Ok(t.agents[at].id)
        })?;
        let key = AgentKey { task: id, agent: new_id };
        self.record(id, EventKind::WriterChanged { from: old.agent, to: new_id, kind: kind.0 });
        if was_running {
            self.start_agent(key)?;
        }
        Ok(key)
    }

    /// 现在换过去能接回原来会话的 Agent（界面据此说明换过去会怎样）。
    pub fn resumable_writers(&self, id: TaskId) -> Vec<AgentKind> {
        let Some(task) = self.state.task(id) else { return Vec::new() };
        let mut kinds: Vec<AgentKind> = Vec::new();
        for kind in task.retired.iter().map(|a| &a.kind) {
            if !kinds.contains(kind) && self.revivable(task, kind).is_some() {
                kinds.push(kind.clone());
            }
        }
        kinds
    }

    /// 可以接回原来会话的、换下来的同一种 Agent：最近一次换下来的，一天以内，记得会话编号，插件能按编号恢复。
    fn revivable<'a>(&self, task: &'a Task, kind: &AgentKind) -> Option<&'a AgentSpec> {
        let plugin = self.agents().get(kind).ok()?;
        let last = task.retired.iter().rev().find(|a| a.kind == *kind && a.role == AgentRole::Writer)?;
        let recent = last.left.as_ref().is_some_and(|l| now_secs().saturating_sub(l.at) <= REVIVE_WITHIN_SECS);
        (recent && last.session.is_some() && last.launched && plugin.resumes_session()).then_some(last)
    }

    /// 分出新任务：照着任务 `id` 现在的代码（包括没提交的改动）新建分支和 worktree，让 `kind` 带着交接说明和
    /// `request`（可以为空）在里面接着做。原来的任务照旧运行。返回新任务；Agent 在打开它时才启动。
    /// 会运行 git，大仓库要一会儿；共用一个加锁的引擎时用 [`crate::ops::fork_task`]。
    pub fn fork_task(&mut self, id: TaskId, kind: AgentKind, request: &str) -> Result<TaskId> {
        let new = self.prepare_fork(id, kind, request)?;
        new.check_out()?;
        self.finish_task(new)
    }

    /// 分出新任务的第一步：检查、定好名字和交接说明、占住编号。之后和新建任务一样
    /// 用 [`NewTask::check_out`] 建 worktree，[`Engine::finish_task`] 登记。
    pub fn prepare_fork(&mut self, id: TaskId, kind: AgentKind, request: &str) -> Result<NewTask> {
        self.check_open(id)?;
        let plugin = self.agents().get(&kind)?;
        if !plugin.is_installed() {
            return Err(UserError::CommandNotFound { program: plugin.command().into() }.into());
        }
        let parent = self.state.task(id).cloned().ok_or(UserError::TaskNotFound)?;
        let from = self.writer_key(id)?;
        let current = parent.agent(from.agent).ok_or(UserError::AgentNotRunning)?;
        let handoff = self.handoff(&parent, current).map(|text| format!("{text}\n\n{FORK_NOTE}"));
        let prompt = with_request(handoff, request);
        let surface = self.surface_for(&kind, current.surface);
        let repo = match &parent.repo {
            Some(repo) => repo.clone(),
            None => self.state.project(&parent.project).ok_or(UserError::ProjectNotFound)?.root.clone(),
        };
        let request = request.trim();
        let title = if request.is_empty() { parent.title.clone() } else { derive_title(request) };
        let new_id = self.update(|state| Ok(state.allocate_task_id()))?;
        let branch = duolo_git::unique_branch(&repo, &parent.branch);
        let worktree = self.paths().worktrees().join(&parent.project.0).join(format!("{new_id}-{}", slug(&title)));
        let writer = AgentSpec { prompt, surface, ..AgentSpec::new(0, kind, AgentRole::Writer) };
        let task = Task {
            id: new_id,
            project: parent.project,
            title,
            prompt: parent.prompt,
            branch,
            base: parent.base,
            worktree,
            created_at: now_secs(),
            agents: vec![writer],
            repo: Some(repo.clone()),
            reviewer: parent.reviewer,
            review: None,
            retired: Vec::new(),
            forked_from: Some(id),
        };
        Ok(NewTask { task, repo, like: Some(parent.worktree) })
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
        let events = self.events_since(task.id, 0).unwrap_or_default();
        let lines = self.conversation(task, events);
        if task.prompt.trim().is_empty() && lines.is_empty() {
            return None;
        }
        let commit = self.fork_of(task);
        Some(handoff_text(&self.name_of(task, from.id), task, &lines, &commit))
    }

    /// 换回来时先告诉它的：它离开之后的对话和改动。
    fn welcome_back(&self, task: &Task, left: &Departure) -> String {
        let events = self.events_since(task.id, left.seq).unwrap_or_default();
        welcome_text(&self.conversation(task, events), left.commit.as_deref())
    }

    /// 事件里的对话，每条前面写上是谁说的；只留最近的、不太长的。
    fn conversation(&self, task: &Task, events: Vec<TaskEvent>) -> Vec<String> {
        let mut lines: Vec<String> = events
            .into_iter()
            .filter_map(|e| match e.kind {
                EventKind::Message { speaker, agent, text } => {
                    let who = match speaker {
                        Speaker::User => "User".to_string(),
                        Speaker::Agent => self.name_of(task, agent),
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
        lines
    }

    /// 任务里某个 Agent（包括换下来的）的名字。
    fn name_of(&self, task: &Task, agent: u32) -> String {
        let spec = task.agents.iter().chain(&task.retired).find(|a| a.id == agent);
        spec.map_or_else(|| "Agent".to_string(), |a| self.agents().name(&a.kind))
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

/// 分出的任务里，交接说明后面再说一句：这是一份副本，原来的 Agent 还在原任务里做。
const FORK_NOTE: &str = "This worktree is a separate copy on its own branch, made for you. The other agent keeps \
working on the original task, so do not wait for it or coordinate with it.";

/// 交接说明后面带上用户要它做的事。没有交接说明时，就是这句话本身。
fn with_request(text: Option<String>, request: &str) -> Option<String> {
    let request = request.trim();
    match (text, request.is_empty()) {
        (text, true) => text,
        (Some(text), false) => Some(format!("{text}\n\nWhat the user wants you to do now:\n{request}")),
        (None, false) => Some(request.to_string()),
    }
}

/// 换回来的 Agent 恢复会话后先收到的话。
fn welcome_text(conversation: &[String], commit: Option<&str>) -> String {
    let mut text = "You are back on this task. While you were away, other agents worked on it.\n".to_string();
    if !conversation.is_empty() {
        text.push_str("\nThe conversation since you left, oldest first:\n\n");
        text.push_str(&conversation.join("\n\n"));
        text.push('\n');
    }
    text.push_str(&match commit {
        Some(commit) => format!(
            "\nRun `git diff {commit}` to see how the files changed since you left (commit {commit} is how they looked then), \
and `git status --short --untracked-files=all` to find new files that are not in git yet. "
        ),
        None => "\nRun `git status` and `git diff` to see the current state of the files. ".to_string(),
    });
    text.push_str("Files may differ from what you remember, so read them again before editing. Then continue.");
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

    /// 等文件写完（有内容，而且一会儿不再变）。
    fn read_when_written(path: &Path) -> String {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut last = String::new();
        loop {
            let text = std::fs::read_to_string(path).unwrap_or_default();
            if !text.is_empty() && text == last {
                return text;
            }
            last = text;
            assert!(Instant::now() < deadline, "{} was not written", path.display());
            std::thread::sleep(Duration::from_millis(200));
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
        assert_eq!(engine.switch_writer(id, AgentKind("codex".into())).unwrap(), next, "已经是它了");
        engine.shutdown();
    }

    /// 接力时带上一句话：跟在交接说明后面。
    #[test]
    fn a_request_follows_the_handoff() {
        let (_dir, mut engine, project) = engine();
        let id = engine.create_task(&project, "Export orders as CSV").unwrap();
        let first = engine.writer_key(id).unwrap();
        engine.start_agent(first).unwrap();
        let worktree = engine.state().task(id).unwrap().worktree.clone();
        read_when_written(&worktree.join("started-claude"));

        engine.switch_writer_with(id, AgentKind("codex".into()), "  Add a total row ").unwrap();
        let handoff = read_when_written(&worktree.join("started-codex"));
        assert!(handoff.starts_with("You are taking over"), "{handoff}");
        assert!(handoff.ends_with("What the user wants you to do now:\nAdd a total row"), "{handoff}");
        engine.shutdown();
    }

    /// 分出新任务：新的 worktree 和原来的代码一样（没提交的改动照旧没提交），另一个 Agent 带着交接说明
    /// 在里面接着做；原来的任务不受影响。
    #[test]
    fn a_fork_continues_from_a_copy_of_the_work() {
        let (_dir, mut engine, project) = engine();
        let id = engine.create_task(&project, "Export orders as CSV").unwrap();
        let first = engine.writer_key(id).unwrap();
        engine.start_agent(first).unwrap();
        let parent = engine.state().task(id).unwrap().clone();
        read_when_written(&parent.worktree.join("started-claude"));
        std::fs::write(parent.worktree.join("tracked.txt"), "v1\n").unwrap();
        git(&parent.worktree, &["add", "tracked.txt"]);
        git(&parent.worktree, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "v1"]);
        std::fs::write(parent.worktree.join("tracked.txt"), "v2\n").unwrap();
        std::fs::write(parent.worktree.join("new.txt"), "new\n").unwrap();

        let fork = engine.fork_task(id, AgentKind("codex".into()), "Use cursor paging").unwrap();
        let task = engine.state().task(fork).unwrap().clone();
        assert_eq!(task.forked_from, Some(id));
        assert_eq!(task.branch, format!("{}-2", parent.branch));
        assert_eq!((task.base.as_str(), task.prompt.as_str()), (parent.base.as_str(), "Export orders as CSV"));
        assert_eq!(task.title, "Use cursor paging");
        assert_ne!(task.worktree, parent.worktree);
        assert_eq!(std::fs::read_to_string(task.worktree.join("tracked.txt")).unwrap(), "v2\n");
        assert_eq!(std::fs::read_to_string(task.worktree.join("new.txt")).unwrap(), "new\n");
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(&task.worktree)
            .args(["status", "--porcelain", "--", "tracked.txt", "new.txt"])
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&status.stdout), " M tracked.txt\n?? new.txt\n", "改动照旧没提交");
        assert_eq!(engine.agent_status(first), AgentStatus::Running, "原来的任务照旧运行");

        engine.start_agent(engine.writer_key(fork).unwrap()).unwrap();
        let handoff = read_when_written(&task.worktree.join("started-codex"));
        assert!(handoff.contains("from another agent (Claude)") && handoff.contains("separate copy"), "{handoff}");
        assert!(handoff.ends_with("What the user wants you to do now:\nUse cursor paging"), "{handoff}");
        engine.shutdown();
    }

    /// 很快又换回原来的：接回它原来的会话，恢复后先告诉它这期间的对话和改动。
    #[test]
    fn switching_back_resumes_the_earlier_session() {
        let (_dir, mut engine, project) = engine();
        let id = engine.create_task(&project, "Export orders as CSV").unwrap();
        let first = engine.writer_key(id).unwrap();
        engine.start_agent(first).unwrap();
        let session = engine.state().task(id).unwrap().agents[0].session.clone().unwrap();
        let worktree = engine.state().task(id).unwrap().worktree.clone();
        read_when_written(&worktree.join("started-claude"));
        std::fs::remove_file(worktree.join("started-claude")).unwrap();

        let codex = engine.switch_writer(id, AgentKind("codex".into())).unwrap();
        assert_eq!(engine.resumable_writers(id), [AgentKind("claude".into())], "界面据此说明会接回原来的会话");
        let left = engine.state().task(id).unwrap().retired[0].left.clone().unwrap();
        assert!(left.commit.is_some());
        engine.record(
            id,
            EventKind::Message { speaker: Speaker::User, agent: codex.agent, text: "Add a total row".into() },
        );

        let back = engine.switch_writer(id, AgentKind("claude".into())).unwrap();
        assert_eq!(back, first, "还是原来那个");
        assert_eq!(read_when_written(&worktree.join("started-claude")), session, "--resume <它的会话>");
        let task = engine.state().task(id).unwrap();
        assert_eq!(task.retired.iter().map(|a| a.id).collect::<Vec<_>>(), [codex.agent]);
        assert!(task.agents[0].resume_note.is_none() && task.agents[0].left.is_none());
        let note = engine.live.pending.get(&back).and_then(|q| q.first()).cloned().unwrap();
        assert!(note.starts_with("You are back on this task"), "{note}");
        assert!(note.contains("[User] Add a total row") && !note.contains("Export orders"), "只说它不知道的：{note}");
        assert!(note.contains(&format!("git diff {}", left.commit.as_deref().unwrap())), "{note}");
        assert!(note.contains("git status"), "新加的文件 git diff 看不到：{note}");
        engine.shutdown();
    }

    /// 换下来太久了，原来的会话多半已经过时：重新开始。
    #[test]
    fn switching_back_much_later_starts_fresh() {
        let (_dir, mut engine, project) = engine();
        let id = engine.create_task(&project, "Export orders as CSV").unwrap();
        let first = engine.writer_key(id).unwrap();
        engine.start_agent(first).unwrap();
        engine.switch_writer(id, AgentKind("codex".into())).unwrap();
        engine
            .update(|state| {
                state.task_mut(id).unwrap().retired[0].left.as_mut().unwrap().at = 0;
                Ok(())
            })
            .unwrap();
        let back = engine.switch_writer(id, AgentKind("claude".into())).unwrap();
        assert!(back.agent > first.agent, "新开一个");
        engine.shutdown();
    }

    /// 命令行（另一个进程）换人：只改状态；跑着原来那个 Agent 的进程同步时停掉它，新的等打开任务时再启动。
    #[test]
    fn another_process_can_switch() {
        let (dir, mut app, project) = engine();
        let id = app.create_task(&project, "Export orders as CSV").unwrap();
        let first = app.writer_key(id).unwrap();
        app.start_agent(first).unwrap();
        app.queue_message(first, "also add a header".into());

        let mut cli = Engine::open(Paths::at(dir.path().join("home"))).unwrap();
        let next = cli.switch_writer(id, AgentKind("codex".into())).unwrap();
        assert_eq!(cli.agent_status(next), AgentStatus::NotStarted, "命令行不替它启动");

        app.sync();
        assert_eq!(app.agent_status(first), AgentStatus::NotStarted, "原来的停了");
        assert_eq!(app.writer_key(id).unwrap(), next);
        assert_eq!(app.live.pending.get(&next).map(Vec::len), Some(1), "没发出去的消息交给接手的");
        app.start_agent(next).unwrap();
        let worktree = app.state().task(id).unwrap().worktree.clone();
        assert!(read_when_written(&worktree.join("started-codex")).contains("from another agent (Claude)"));
        app.shutdown();
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
