//! Duolo 的领域模型、配置和本地持久化。
//!
//! 这个 crate 不依赖终端、进程或 UI，只描述"有哪些项目、任务和 Agent"以及它们如何保存。
//! 调度引擎、TUI，以及以后的守护进程和手机端协议都建立在这里的类型之上。

pub mod account;
pub mod config;
pub mod error;
pub mod event;
pub mod hook;
pub mod i18n;
pub mod model;
pub mod paths;
pub mod process;
pub mod proctree;
pub mod settings;
pub mod store;
pub mod text;
pub mod wire;

pub use account::Account;
pub use config::{AgentOverride, Config, VoiceConfig};
pub use error::UserError;
pub use event::{Command, CommandResult, EventKind, Speaker, TaskEvent};
pub use i18n::{t, tf};
pub use model::{
    AgentKey, AgentKind, AgentRole, AgentSpec, Automation, AutomationId, AutomationRun, Departure, Project, ProjectId,
    Review, RunOutcome, RunTrigger, State, Surface, Task, TaskId, Verdict,
};
pub use paths::Paths;
pub use settings::Settings;
pub use store::{Loaded, Stamp, Store};
