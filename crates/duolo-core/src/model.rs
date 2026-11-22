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

/// 用哪个 CLI 来跑 Agent。名字对应配置里的 `[agents.<name>]`。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AgentKind(pub String);

impl AgentKind {
    pub fn claude() -> Self {
        Self("claude".into())
    }

    pub fn codex() -> Self {
        Self("codex".into())
    }

    /// 界面上显示的名字。
    pub fn display_name(&self) -> String {
        match self.0.as_str() {
            "claude" => "Claude".into(),
            "codex" => "Codex".into(),
            other => {
                let mut chars = other.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().chain(chars).collect(),
                    None => String::new(),
                }
            }
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

/// 任务里的一个 Agent。运行时状态（进程、屏幕）不在这里，由调度引擎持有。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSpec {
    pub id: u32,
    pub kind: AgentKind,
    pub role: AgentRole,
    /// 是否已经启动过。第一次启动带上需求，之后用恢复会话的参数启动。
    #[serde(default)]
    pub launched: bool,
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
    fn first_line(&self) -> &str {
        self.output.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("")
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
        self.agents.iter().map(|a| a.id + 1).max().unwrap_or(0)
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
    fn display_names() {
        assert_eq!(AgentKind::claude().display_name(), "Claude");
        assert_eq!(AgentKind("aider".into()).display_name(), "Aider");
    }

    #[test]
    fn review_verdict() {
        let review = |output: &str| Review { finished_at: 0, ok: true, output: output.into() };
        assert_eq!(review("[PASS] No issues found\n").verdict(), Verdict::Pass);
        assert_eq!(review("[PASS] No issues found").summary(), "No issues found");
        let r = review("\n[ISSUES 2] 2 critical issues, 3 suggestions\n- a");
        assert_eq!(r.verdict(), Verdict::Issues(2));
        assert_eq!(r.summary(), "2 critical issues, 3 suggestions");
        assert_eq!(review("Looks good").verdict(), Verdict::Unknown);
        assert_eq!(review("Looks good").summary(), "Looks good");
        let failed = Review { finished_at: 0, ok: false, output: "[PASS] x".into() };
        assert_eq!(failed.verdict(), Verdict::Unknown);
    }
}
