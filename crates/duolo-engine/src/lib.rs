//! 调度引擎：管理项目、任务、worktree 和 Agent 进程的生命周期。
//!
//! 引擎不依赖任何界面。现在由 TUI 直接持有；以后拆出守护进程 `duolod` 时，
//! 这里的公开方法就是守护进程对外提供的接口，终端、手机和网页客户端都通过它操作。

mod engine;
mod env;
mod status;

pub use duolo_git::DiffStat;
pub use duolo_pty::Size;
pub use engine::Engine;
pub use env::{check_environment, Check};
pub use status::{AgentStatus, Summary};
