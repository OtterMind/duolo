//! 从屏幕内容和输出节奏判断 Agent 的状态。
//!
//! 这是第一版的启发式规则：确认请求按关键字识别，"running"按最近是否有输出识别。
//! 以后接入 hooks 和会话日志后，这里会作为兜底规则保留。

use std::time::Duration;

use crate::session::Exit;

/// 超过这个时长没有输出，就认为 Agent 停下来了。
const QUIET_THRESHOLD: Duration = Duration::from_millis(1_500);

/// 怎么从画面认出状态，由启动会话的一方按 Agent 给出。统一按小写比较，这里的文字也要是小写。
#[derive(Debug, Clone, Default)]
pub struct Rules {
    /// 出现这些文字说明 Agent 在等用户确认。
    pub approval: Vec<String>,
    /// 出现这些文字说明 Agent 正在工作，即使暂时没有新输出。
    pub busy: Vec<String>,
}

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

/// `tail` 是屏幕底部的文字，`quiet_for` 是距离最近一次输出的时长。
pub fn classify(rules: &Rules, tail: &str, quiet_for: Duration) -> AgentState {
    let lower = tail.to_lowercase();
    if rules.approval.iter().any(|m| lower.contains(m)) {
        return AgentState::NeedsInput;
    }
    if rules.busy.iter().any(|m| lower.contains(m)) || quiet_for < QUIET_THRESHOLD {
        return AgentState::Running;
    }
    AgentState::Idle
}

#[cfg(test)]
mod tests {
    use super::*;

    const QUIET: Duration = Duration::from_secs(10);
    const BUSY: Duration = Duration::from_millis(100);

    fn classify(tail: &str, quiet_for: Duration) -> AgentState {
        let rules = Rules { approval: vec!["1. yes".into()], busy: vec!["esc to interrupt".into()] };
        super::classify(&rules, tail, quiet_for)
    }

    #[test]
    fn approval_prompt_wins() {
        assert_eq!(classify("Allow command?\n› 1. Yes", BUSY), AgentState::NeedsInput);
        assert_eq!(classify("Proceed?\n❯ 1. YES\n  2. No\nesc to interrupt", QUIET), AgentState::NeedsInput);
    }

    #[test]
    fn spinner_means_running() {
        assert_eq!(classify("✻ Thinking… (Esc to interrupt)", QUIET), AgentState::Running);
        assert_eq!(classify("> ", BUSY), AgentState::Running);
    }

    #[test]
    fn quiet_prompt_is_idle() {
        assert_eq!(classify("> \n? for shortcuts", QUIET), AgentState::Idle);
    }
}
