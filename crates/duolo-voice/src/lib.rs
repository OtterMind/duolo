//! 语音输入：录音识别和需求整理。
//!
//! 识别用 Duolo 服务器：录麦克风发给服务器，带上任务里的热词和对话，中英混说、代码里的名字认得准；
//! 要的话服务器顺便把原话整理成一条指令。
//! 这些都是阻塞调用，调用方应在后台线程里执行。

mod cloud;
mod hints;
mod mic;
mod session;

pub use cloud::{Recorder, Transcript};
pub use hints::{hints, HintSource, Hints};
pub use session::start;
