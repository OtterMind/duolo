//! Duolo 的领域模型、配置和本地持久化。
//!
//! 这个 crate 不依赖终端、进程或 UI，只描述"有哪些项目、任务和 Agent"以及它们如何保存。
//! 调度引擎、TUI，以及以后的守护进程和手机端协议都建立在这里的类型之上。

pub mod config;
pub mod model;
pub mod paths;
pub mod store;
pub mod text;

pub use config::{AgentProfile, Config, VoiceConfig};
pub use model::{AgentKey, AgentKind, AgentRole, AgentSpec, Project, ProjectId, State, Task, TaskId};
pub use paths::Paths;
pub use store::Store;
