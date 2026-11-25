use duolo_pty::{AgentState, Exit};

/// 界面上展示的 Agent 状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStatus {
    /// 还没有进程（新任务尚未启动，或重新打开 Duolo 后尚未恢复）。
    NotStarted,
    Running,
    NeedsInput,
    Idle,
    Exited(Exit),
}

impl From<AgentState> for AgentStatus {
    fn from(state: AgentState) -> Self {
        match state {
            AgentState::Running => Self::Running,
            AgentState::NeedsInput => Self::NeedsInput,
            AgentState::Idle => Self::Idle,
            AgentState::Exited(exit) => Self::Exited(exit),
        }
    }
}

impl From<duolo_chat::Status> for AgentStatus {
    fn from(status: duolo_chat::Status) -> Self {
        match status {
            duolo_chat::Status::Starting | duolo_chat::Status::Working => Self::Running,
            duolo_chat::Status::Asking => Self::NeedsInput,
            duolo_chat::Status::Idle => Self::Idle,
            duolo_chat::Status::Exited { code, success } => Self::Exited(Exit { code, success }),
        }
    }
}

impl AgentStatus {
    /// 当前语言的状态名。
    pub fn label(self) -> String {
        duolo_core::t(match self {
            Self::NotStarted => "status.not_started",
            Self::Running => "status.running",
            Self::NeedsInput => "status.needs_input",
            Self::Idle => "status.idle",
            Self::Exited(exit) if exit.success => "status.exited",
            Self::Exited(_) => "status.failed",
        })
    }

    /// 状态码，和桌面界面、手机端的状态一一对应。
    pub fn code(self) -> &'static str {
        match self {
            Self::NotStarted => "not_started",
            Self::Running => "running",
            Self::NeedsInput => "needs_input",
            Self::Idle => "idle",
            Self::Exited(exit) if exit.success => "exited",
            Self::Exited(_) => "failed",
        }
    }

    pub fn is_live(self) -> bool {
        matches!(self, Self::Running | Self::NeedsInput | Self::Idle)
    }
}

/// 侧边栏底部的汇总。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Summary {
    pub running: usize,
    pub needs_input: usize,
}
