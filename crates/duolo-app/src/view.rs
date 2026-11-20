//! 发给界面的数据。界面只读这些结构，不直接接触领域模型，以后手机端和网页端沿用同一份格式。

use duolo_core::{AgentRole, State, Task};
use duolo_engine::{AgentStatus, Check, DiffStat, Engine};
use serde::Serialize;

/// 侧边栏和首页需要的全部状态。后台定时生成，内容变化时推送给界面。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub projects: Vec<ProjectView>,
    pub running: usize,
    pub needs_input: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectView {
    pub id: String,
    pub name: String,
    pub root: String,
    pub tasks: Vec<TaskView>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskView {
    pub id: u64,
    pub title: String,
    pub prompt: String,
    pub branch: String,
    pub base: String,
    pub worktree: String,
    pub created_at: u64,
    pub agents: Vec<AgentView>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentView {
    pub id: u32,
    pub kind: String,
    pub name: String,
    pub role: Role,
    pub status: Status,
    /// 当前进程的编号，变化时界面要重新订阅输出。
    pub session: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Writer,
    Reviewer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    NotStarted,
    Running,
    NeedsInput,
    Idle,
    Exited,
    Failed,
}

impl From<AgentStatus> for Status {
    fn from(status: AgentStatus) -> Self {
        match status {
            AgentStatus::NotStarted => Self::NotStarted,
            AgentStatus::Running => Self::Running,
            AgentStatus::NeedsInput => Self::NeedsInput,
            AgentStatus::Idle => Self::Idle,
            AgentStatus::Exited(exit) if exit.success => Self::Exited,
            AgentStatus::Exited(_) => Self::Failed,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckView {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

impl From<Check> for CheckView {
    fn from(check: Check) -> Self {
        Self { name: check.name, ok: check.ok, detail: check.detail }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffView {
    pub files: u32,
    pub insertions: u32,
    pub deletions: u32,
}

impl From<DiffStat> for DiffView {
    fn from(stat: DiffStat) -> Self {
        Self { files: stat.files, insertions: stat.insertions, deletions: stat.deletions }
    }
}

pub fn snapshot(engine: &mut Engine) -> Snapshot {
    let state: State = engine.state().clone();
    let projects = state
        .projects
        .iter()
        .map(|project| ProjectView {
            id: project.id.0.clone(),
            name: project.name.clone(),
            root: project.root.display().to_string(),
            tasks: state.tasks_of(&project.id).map(|task| task_view(engine, task)).collect(),
        })
        .collect();
    let summary = engine.summary();
    Snapshot { projects, running: summary.running, needs_input: summary.needs_input }
}

fn task_view(engine: &mut Engine, task: &Task) -> TaskView {
    let agents = task
        .agents
        .iter()
        .map(|agent| AgentView {
            id: agent.id,
            kind: agent.kind.0.clone(),
            name: agent.kind.display_name(),
            role: match agent.role {
                AgentRole::Writer => Role::Writer,
                AgentRole::Reviewer => Role::Reviewer,
            },
            status: engine.agent_status(task.key(agent.id)).into(),
            session: engine.session_id(task.key(agent.id)),
        })
        .collect();
    TaskView {
        id: task.id.0,
        title: task.title.clone(),
        prompt: task.prompt.clone(),
        branch: task.branch.clone(),
        base: task.base.clone(),
        worktree: task.worktree.display().to_string(),
        created_at: task.created_at,
        agents,
    }
}
