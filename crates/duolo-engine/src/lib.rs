//! 调度引擎：管理项目、任务、worktree 和 Agent 进程的生命周期。
//!
//! 引擎不依赖任何界面。现在由 TUI 直接持有；以后拆出守护进程 `duolod` 时，
//! 这里的公开方法就是守护进程对外提供的接口，终端、手机和网页客户端都通过它操作。

mod accounts;
pub mod automation;
mod chat;
#[cfg(test)]
mod conformance;
mod diffs;
mod engine;
mod env;
mod events;
mod live;
mod naming;
pub mod ops;
mod prompt;
mod review;
mod setup;
mod shells;
mod status;
mod switch;
mod updates;

pub use accounts::{account_status, AccountStatus};
pub use automation::AutomationDraft;
pub use duolo_agent::{Account, AccountSource, Plugin, Registry};
pub use duolo_chat::Draft;
pub use duolo_git::DiffStat;
pub use duolo_pty::{Session, Size};
pub use engine::{repos_in, Closed, Closing, Engine, NewTask, TaskOptions};
pub use env::{check_environment, Check, CheckKind};
pub use live::Digest;
pub use naming::suggest_branch;
pub use setup::{signed_in, start_setup, SetupStep};
pub use status::{AgentStatus, Summary};
pub use updates::{agent_versions, duolo_update, Release};
