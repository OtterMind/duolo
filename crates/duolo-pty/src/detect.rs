//! 从屏幕内容和输出节奏判断 Agent 的状态。
//!
//! 这是第一版的启发式规则：确认请求按关键字识别，"运行中"按最近是否有输出识别。
//! 以后接入 hooks 和会话日志后，这里会作为兜底规则保留。

use std::time::Duration;

use crate::session::Exit;

/// 超过这个时长没有输出，就认为 Agent 停下来了。
const QUIET_THRESHOLD: Duration = Duration::from_millis(1_500);

/// 出现这些文字说明 Agent 在等用户确认（统一按小写比较）。
/// Claude Code 和 Codex 的确认界面都会给出 "1. Yes" 开头的选项列表。
const APPROVAL_MARKERS: &[&str] =
    &["1. yes", "do you want to proceed", "allow command", "would you like to run the following command"];

/// 出现这些文字说明 Agent 正在工作，即使暂时没有新输出。
const BUSY_MARKERS: &[&str] = &["esc to interrupt", "esc to cancel"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentState {
    /// 正在工作。
    Running,
    /// 在等用户确认。
    NeedsInput,
    /// 空闲，等待下一条指令。
    Idle,
    Exited(Exit),
}

impl AgentState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "运行中",
            Self::NeedsInput => "等你确认",
            Self::Idle => "等待输入",
            Self::Exited(exit) if exit.success => "已结束",
            Self::Exited(_) => "异常退出",
        }
    }
}

/// `tail` 是屏幕底部的文字，`quiet_for` 是距离最近一次输出的时长。
pub fn classify(tail: &str, quiet_for: Duration) -> AgentState {
    let lower = tail.to_lowercase();
    if APPROVAL_MARKERS.iter().any(|m| lower.contains(m)) {
        return AgentState::NeedsInput;
    }
    if BUSY_MARKERS.iter().any(|m| lower.contains(m)) || quiet_for < QUIET_THRESHOLD {
        return AgentState::Running;
    }
    AgentState::Idle
}

#[cfg(test)]
mod tests {
    use super::*;

    const QUIET: Duration = Duration::from_secs(10);
    const BUSY: Duration = Duration::from_millis(100);

    #[test]
    fn approval_prompt_wins() {
        let tail = "Allow command?\n  pnpm add csv-stringify\n› 1. Yes";
        assert_eq!(classify(tail, BUSY), AgentState::NeedsInput);
        let tail = "Do you want to proceed?\n❯ 1. Yes\n  2. No";
        assert_eq!(classify(tail, QUIET), AgentState::NeedsInput);
    }

    #[test]
    fn spinner_means_running() {
        assert_eq!(classify("✻ Thinking… (esc to interrupt)", QUIET), AgentState::Running);
        assert_eq!(classify("> ", BUSY), AgentState::Running);
    }

    #[test]
    fn quiet_prompt_is_idle() {
        assert_eq!(classify("> \n? for shortcuts", QUIET), AgentState::Idle);
    }
}
