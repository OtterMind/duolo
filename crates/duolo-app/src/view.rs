//! 发给界面的数据。界面只读这些结构，不直接接触领域模型，以后手机端和网页端沿用同一份格式。

use duolo_core::wire::{ReviewFacts, TaskState};
use duolo_core::{AgentRole, State, Task, Verdict};
use duolo_engine::{AgentStatus, Check, CheckKind, DiffStat, Engine, Plugin};
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
    /// 还没有需求、也没改过名的空白任务，界面按当前语言显示为"新任务"。
    pub untitled: bool,
    pub branch: String,
    pub base: String,
    pub worktree: String,
    pub created_at: u64,
    pub agents: Vec<AgentView>,
    /// 审阅用的 Agent。
    pub reviewer: ReviewerView,
    /// 改的是项目里的哪个仓库（相对项目文件夹的路径）；项目本身就是仓库时为空。
    pub repo: String,
    /// 正在后台审阅时的开始时间（Unix 秒）。
    pub reviewing_since: Option<u64>,
    pub review: Option<ReviewView>,
    /// 列表上显示的状态（写代码的 Agent 和审阅合在一起），和手机端同一套规则。
    pub state: TaskState,
    /// 换回来时能接着原来会话的 Agent（插件 id）。
    pub resumable: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewerView {
    pub kind: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewView {
    pub finished_at: u64,
    pub ok: bool,
    /// `pass`、`issues` 或 `unknown`（没有判断标记）。
    pub verdict: &'static str,
    /// 必须修改的问题数（`verdict` 为 `issues` 时）。
    pub issues: u32,
    pub summary: String,
    pub output: String,
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

/// 一种 Agent 的名字和图标。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginView {
    pub id: String,
    pub name: String,
    /// 单色 SVG，界面当作遮罩用；没有时显示首字母。
    pub icon: Option<String>,
    /// 图标的颜色，没有时跟随文字颜色。
    pub color: Option<String>,
    /// 能不能用来审阅。
    pub reviews: bool,
}

impl From<&Plugin> for PluginView {
    fn from(plugin: &Plugin) -> Self {
        Self {
            id: plugin.id().into(),
            name: plugin.name().into(),
            icon: plugin.icon().map(Into::into),
            color: plugin.icon_color().map(Into::into),
            reviews: plugin.review().is_some(),
        }
    }
}

/// 环境检查结果。文案由界面按 `kind` 翻译。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckView {
    /// `git`、`agent` 或 `voice`。
    pub kind: &'static str,
    /// Agent 的种类，其他检查为空。
    pub agent: String,
    /// 没安装时的安装命令（Agent 才有）。
    pub install: String,
    pub name: String,
    pub ok: bool,
    pub command: String,
    pub version: String,
    /// 装的版本太旧时，最低要求的版本。
    pub minimum: String,
}

impl From<Check> for CheckView {
    fn from(check: Check) -> Self {
        let kind = match check.kind {
            CheckKind::Git => "git",
            CheckKind::Agent => "agent",
            CheckKind::Voice => "voice",
        };
        Self {
            kind,
            agent: check.agent,
            install: check.install,
            name: check.name,
            ok: check.ok,
            command: check.command,
            version: check.version,
            minimum: check.minimum,
        }
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
    let closing: Vec<_> = state.tasks.iter().map(|t| t.id).filter(|id| engine.is_closing(*id)).collect();
    let projects = state
        .projects
        .iter()
        .map(|project| ProjectView {
            id: project.id.0.clone(),
            name: project.name.clone(),
            root: project.root.display().to_string(),
            // 正在关闭的任务先不显示（停 Agent、保存改动要几秒）；没关成会再出现。
            tasks: state
                .tasks_of(&project.id)
                .filter(|task| !closing.contains(&task.id))
                .map(|task| task_view(engine, task, &project.root))
                .collect(),
        })
        .collect();
    let summary = engine.summary();
    Snapshot { projects, running: summary.running, needs_input: summary.needs_input }
}

fn task_view(engine: &mut Engine, task: &Task, project_root: &std::path::Path) -> TaskView {
    let agents = task
        .agents
        .iter()
        .map(|agent| AgentView {
            id: agent.id,
            kind: agent.kind.0.clone(),
            name: engine.agents().name(&agent.kind),
            role: match agent.role {
                AgentRole::Writer => Role::Writer,
                AgentRole::Reviewer => Role::Reviewer,
            },
            status: engine.agent_status(task.key(agent.id)).into(),
            session: engine.session_id(task.key(agent.id)),
        })
        .collect();
    let state = {
        let writer = task.agent_with_role(AgentRole::Writer).map(|a| task.key(a.id));
        let status = writer.map_or(AgentStatus::NotStarted, |key| engine.agent_status(key));
        let since = writer.and_then(|key| engine.status_since(key)).unwrap_or(0);
        let verdict = task.review.as_ref().map(|r| (r.ok, r.verdict()));
        let review = task.review.as_ref().zip(verdict).map(|(r, (ok, verdict))| ReviewFacts {
            ok,
            verdict: verdict_code(verdict),
            finished_at: r.finished_at * 1000,
        });
        TaskState::of(status.code(), engine.review_started_at(task.id).is_some(), review, since)
    };
    TaskView {
        id: task.id.0,
        title: task.title.clone(),
        prompt: task.prompt.clone(),
        untitled: task.prompt.is_empty() && duolo_core::text::is_default_title(&task.title),
        branch: task.branch.clone(),
        base: task.base.clone(),
        worktree: task.worktree.display().to_string(),
        created_at: task.created_at,
        agents,
        reviewer: {
            let kind = engine.reviewer_of(task.id).unwrap_or_else(|_| engine.config().reviewer.clone());
            ReviewerView { name: engine.agents().name(&kind), kind: kind.0 }
        },
        repo: task
            .repo
            .as_deref()
            .and_then(|repo| repo.strip_prefix(project_root).ok())
            .map(|rel| rel.display().to_string())
            .unwrap_or_default(),
        reviewing_since: engine.review_started_at(task.id),
        state,
        resumable: engine.resumable_writers(task.id).into_iter().map(|kind| kind.0).collect(),
        review: task.review.as_ref().map(|r| ReviewView {
            finished_at: r.finished_at,
            ok: r.ok,
            verdict: verdict_code(r.verdict()),
            issues: match r.verdict() {
                Verdict::Issues(n) => n,
                _ => 0,
            },
            summary: r.summary().to_string(),
            output: r.output.clone(),
        }),
    }
}

fn verdict_code(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Pass => "pass",
        Verdict::Issues(_) => "issues",
        Verdict::Unknown => "unknown",
    }
}
