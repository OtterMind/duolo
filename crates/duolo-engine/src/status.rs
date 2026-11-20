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

impl AgentStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::NotStarted => "未启动",
            Self::Running => "运行中",
            Self::NeedsInput => "等你确认",
            Self::Idle => "等待输入",
            Self::Exited(exit) if exit.success => "已结束",
            Self::Exited(_) => "异常退出",
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
