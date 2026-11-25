//! Agent 插件：每种编程 Agent（Claude Code、Codex……）怎么启动、恢复、审阅，画面怎么认，会话记录怎么读。
//!
//! 一个插件是一份声明（`agent.toml`）加一个图标，内置的在本 crate 的 `agents/`，用户自己的在 `~/.duolo/agents/`。
//! 声明表达不了的部分（读会话记录）在这里写成代码，由声明按名字引用。
//! 别的 crate 只通过 [`Registry`] 和 [`Plugin`] 使用，不出现任何产品的名字。写法见 docs/agents.md。

mod hooks;
mod manifest;
mod plugin;
mod registry;
mod transcript;
mod trust;

pub use hooks::HookEvent;
pub use manifest::{Keys, Manifest, PromptVia, Screen, CHAT_PROTOCOLS};
pub use plugin::{ChatLaunch, Launch, Plugin, Start};
pub use registry::Registry;
pub use transcript::{Entry, Home, Sessions, Watch};
