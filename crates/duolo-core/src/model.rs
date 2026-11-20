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
    pub fn label(self) -> &'static str {
        match self {
            Self::Writer => "写",
            Self::Reviewer => "审",
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
        (2..).map(|n| format!("{base}-{n}")).find(|id| !taken(id)).map(ProjectId).expect("序号总能找到空位")
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
}
