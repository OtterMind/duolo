//! 语音输入：录音转写和需求整理。
//!
//! 识别引擎不内置，而是通过配置的外部命令接入（例如 sox + whisper.cpp、SenseVoice），
//! 方便按平台和隐私要求替换。整理同样通过命令完成，默认用 `claude -p` 的快速模型。
//!
//! 两个步骤都是阻塞调用，调用方应在后台线程里执行。

mod organizer;
mod process;
mod recorder;

pub use organizer::{organize, organize_prompt, OrganizeContext};
pub use recorder::Recorder;
