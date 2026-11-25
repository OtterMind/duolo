//! 项目、任务和 Agent 的数据模型。
//!
//! 这些类型会被序列化到 `state.json`，以后也会作为守护进程与客户端之间协议的基础，
//! 所以字段只增不改，新增字段要带 `#[serde(default)]`。

use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 项目 id，取仓库目录名，冲突时追加序号。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProjectId(pub String);

impl fmt::Display for ProjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// 任务 id，全局递增。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TaskId(pub u64);

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 一个任务里某个 Agent 的全局标识。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AgentKey {
    pub task: TaskId,
    pub agent: u32,
}

/// 一个 git 仓库。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: ProjectId,
    pub name: String,
    pub root: PathBuf,
}

/// 用哪个 Agent 插件（见 `duolo-agent`）。名字也是配置里 `[agents.<name>]` 的名字。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AgentKind(pub String);

impl AgentKind {
    /// 找不到对应的插件时显示的名字：首字母大写。
    pub fn fallback_name(&self) -> String {
        let mut chars = self.0.chars();
        match chars.next() {
            Some(first) => first.to_uppercase().chain(chars).collect(),
            None => String::new(),
        }
    }
}

/// Agent 在任务里的分工。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRole {
    /// 写代码。
    Writer,
    /// 只读审阅。
    Reviewer,
}

impl AgentRole {
    /// 当前语言的"<agent> 写 / 审"。
    pub fn label(self, agent: &str) -> String {
        match self {
            Self::Writer => crate::tf("agentRole.writer", &[("agent", &agent)]),
            Self::Reviewer => crate::tf("agentRole.reviewer", &[("agent", &agent)]),
        }
    }
}

/// Agent 在哪里运行。同一次会话可以来回切换。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Surface {
    /// 在终端里运行它自己的命令行界面。
    #[default]
    Terminal,
    /// 通过它的结构化协议对话，界面由 Duolo 画（插件要有 `[chat]`）。
    Chat,
}

/// 任务里的一个 Agent。运行时状态（进程、屏幕）不在这里，由调度引擎持有。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSpec {
    pub id: u32,
    pub kind: AgentKind,
    pub role: AgentRole,
    /// 是否已经启动过。第一次启动带上需求，之后用恢复会话的参数启动。
    #[serde(default)]
    pub launched: bool,
    /// Agent 自己的会话编号（从它的会话记录里读到的），恢复时按它找回这一次会话。
    #[serde(default)]
    pub session: Option<String>,
    /// 第一次启动时交给它的话；没有时用任务的需求。中途接手的 Agent 用它拿到交接说明。
    #[serde(default)]
    pub prompt: Option<String>,
    /// 中途被换下来的时候：之后换回它时，告诉它这期间发生了什么。
    #[serde(default)]
    pub left: Option<Departure>,
    /// 下次恢复会话后先发给它的话（换回来时，这期间发生了什么）。
    #[serde(default)]
    pub resume_note: Option<String>,
    /// 在终端里还是聊天模式下运行。旧数据里没有：终端。
    #[serde(default)]
    pub surface: Surface,
}

impl AgentSpec {
    pub fn new(id: u32, kind: AgentKind, role: AgentRole) -> Self {
        Self {
            id,
            kind,
            role,
            launched: false,
            session: None,
            prompt: None,
            left: None,
            resume_note: None,
            surface: Surface::Terminal,
        }
    }
}

/// Agent 中途被换下来的时候。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Departure {
    /// Unix 秒。
    pub at: u64,
    /// 当时任务事件的最后一条（之后的对话是它不知道的）。
    pub seq: u64,
    /// 当时代码的样子（包括没提交的改动），见 `duolo_git::snapshot_commit`；没记下来时为 None。
    #[serde(default)]
    pub commit: Option<String>,
}

/// 一个任务：一段需求、一个独立的 worktree，以及在里面工作的若干 Agent。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub project: ProjectId,
    pub title: String,
    pub prompt: String,
    pub branch: String,
    /// 创建任务时项目所在的分支，审阅和合并以它为基准。
    pub base: String,
    pub worktree: PathBuf,
    /// Unix 秒。
    pub created_at: u64,
    pub agents: Vec<AgentSpec>,
    /// 任务改的是项目里的哪个 git 仓库（主仓库的路径，worktree 从它创建）。
    /// 旧数据里没有：项目本身就是仓库。
    #[serde(default)]
    pub repo: Option<PathBuf>,
    /// 审阅用的 Agent。旧数据里没有，按配置里的默认审阅者。
    #[serde(default)]
    pub reviewer: Option<AgentKind>,
    /// 最近一次后台审阅的结果。
    #[serde(default)]
    pub review: Option<Review>,
    /// 中途换下来的 Agent。事件里的 Agent 编号还指着它们，编号也不再分给新的。
    #[serde(default)]
    pub retired: Vec<AgentSpec>,
}

/// 一次后台审阅的结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Review {
    /// Unix 秒。
    pub finished_at: u64,
    /// 审阅命令是否正常结束。失败时 `output` 是错误说明。
    pub ok: bool,
    pub output: String,
}

/// 审阅结论的判断，来自结论第一行的标记（见 `DEFAULT_REVIEW_PROMPT`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// `[PASS]`：没有必须修改的问题。
    Pass,
    /// `[ISSUES n]`：有 n 个必须修改的问题。
    Issues(u32),
    /// 没有标记（自定义提示词或模型没有照做）。
    Unknown,
}

impl Review {
    /// 带判断标记的那一行；有的 Agent 在结论前先说一句"我先看看"，所以不只看第一行。没有标记时取第一行。
    fn first_line(&self) -> &str {
        let mut lines = self.output.lines().map(str::trim).filter(|l| !l.is_empty());
        let first = lines.clone().next().unwrap_or("");
        let marked = |l: &&str| {
            let upper = l.to_ascii_uppercase();
            upper.starts_with("[PASS]") || upper.starts_with("[ISSUES")
        };
        lines.find(marked).unwrap_or(first)
    }

    /// 第一行开头的标记和剩下的摘要。
    fn parse(&self) -> (Verdict, &str) {
        let line = self.first_line();
        let Some(rest) = line.strip_prefix('[') else { return (Verdict::Unknown, line) };
        let Some((tag, summary)) = rest.split_once(']') else { return (Verdict::Unknown, line) };
        let tag = tag.trim().to_ascii_uppercase();
        let verdict = if tag == "PASS" {
            Verdict::Pass
        } else if let Some(n) = tag.strip_prefix("ISSUES") {
            Verdict::Issues(n.trim().parse().unwrap_or(1))
        } else {
            return (Verdict::Unknown, line);
        };
        (verdict, summary.trim())
    }

    /// 结论的第一行（去掉判断标记），用作摘要。
    pub fn summary(&self) -> &str {
        self.parse().1
    }

    pub fn verdict(&self) -> Verdict {
        if self.ok {
            self.parse().0
        } else {
            Verdict::Unknown
        }
    }
}

impl Task {
    pub fn agent(&self, id: u32) -> Option<&AgentSpec> {
        self.agents.iter().find(|a| a.id == id)
    }

    pub fn agent_mut(&mut self, id: u32) -> Option<&mut AgentSpec> {
        self.agents.iter_mut().find(|a| a.id == id)
    }

    pub fn agent_with_role(&self, role: AgentRole) -> Option<&AgentSpec> {
        self.agents.iter().find(|a| a.role == role)
    }

    pub fn next_agent_id(&self) -> u32 {
        self.agents.iter().chain(&self.retired).map(|a| a.id + 1).max().unwrap_or(0)
    }

    pub fn key(&self, agent: u32) -> AgentKey {
        AgentKey { task: self.id, agent }
    }
}

/// 持久化到 `state.json` 的全部内容。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    #[serde(default = "State::first_task_id")]
    pub next_task_id: u64,
    #[serde(default)]
    pub projects: Vec<Project>,
    #[serde(default)]
    pub tasks: Vec<Task>,
}

impl Default for State {
    fn default() -> Self {
        Self { next_task_id: Self::first_task_id(), projects: Vec::new(), tasks: Vec::new() }
    }
}

impl State {
    fn first_task_id() -> u64 {
        1
    }

    pub fn project(&self, id: &ProjectId) -> Option<&Project> {
        self.projects.iter().find(|p| &p.id == id)
    }

    pub fn task(&self, id: TaskId) -> Option<&Task> {
        self.tasks.iter().find(|t| t.id == id)
    }

    pub fn task_mut(&mut self, id: TaskId) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|t| t.id == id)
    }

    pub fn tasks_of<'a>(&'a self, project: &'a ProjectId) -> impl Iterator<Item = &'a Task> + 'a {
        self.tasks.iter().filter(move |t| &t.project == project)
    }

    pub fn allocate_task_id(&mut self) -> TaskId {
        let id = TaskId(self.next_task_id);
        self.next_task_id += 1;
        id
    }

    /// 为仓库目录名生成一个不冲突的项目 id。
    pub fn unique_project_id(&self, base: &str) -> ProjectId {
        let base = if base.is_empty() { "project" } else { base };
        let taken = |id: &str| self.projects.iter().any(|p| p.id.0 == id);
        if !taken(base) {
            return ProjectId(base.to_string());
        }
        (2..).map(|n| format!("{base}-{n}")).find(|id| !taken(id)).map(ProjectId).expect("a free suffix always exists")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_ids_do_not_collide() {
        let mut state = State::default();
        state.projects.push(Project { id: ProjectId("shop".into()), name: "shop".into(), root: "/a/shop".into() });
        assert_eq!(state.unique_project_id("shop").0, "shop-2");
        assert_eq!(state.unique_project_id("api").0, "api");
    }

    #[test]
    fn task_ids_increase() {
        let mut state = State::default();
        assert_eq!(state.allocate_task_id(), TaskId(1));
        assert_eq!(state.allocate_task_id(), TaskId(2));
    }

    #[test]
    fn fallback_names() {
        assert_eq!(AgentKind("aider".into()).fallback_name(), "Aider");
        assert_eq!(AgentKind(String::new()).fallback_name(), "");
    }

    #[test]
    fn review_verdict() {
        let review = |output: &str| Review { finished_at: 0, ok: true, output: output.into() };
        assert_eq!(review("[PASS] No issues found\n").verdict(), Verdict::Pass);
        assert_eq!(review("[PASS] No issues found").summary(), "No issues found");
        let r = review("\n[ISSUES 2] 2 critical issues, 3 suggestions\n- a");
        assert_eq!(r.verdict(), Verdict::Issues(2));
        assert_eq!(r.summary(), "2 critical issues, 3 suggestions");
        let chatty = review("I'll start by reading the review file.\n[ISSUES 1] 1 critical issue\n- a");
        assert_eq!((chatty.verdict(), chatty.summary()), (Verdict::Issues(1), "1 critical issue"), "前面多说了一句");
        assert_eq!(review("Looks good").verdict(), Verdict::Unknown);
        assert_eq!(review("Looks good").summary(), "Looks good");
        let failed = Review { finished_at: 0, ok: false, output: "[PASS] x".into() };
        assert_eq!(failed.verdict(), Verdict::Unknown);
    }
}
