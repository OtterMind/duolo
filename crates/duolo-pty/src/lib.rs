//! 在伪终端里运行 Agent CLI，并维护它的屏幕状态。
//!
//! 每个 [`Session`] 持有一个 PTY 和一个 VT 解析器：后台线程读取输出喂给解析器，
//! 终端界面通过 [`Session::with_screen`] 读取当前画面来渲染，桌面界面通过 [`Session::attach`]
//! 订阅原始输出交给 xterm.js；按键都通过 [`Session::write`] 写回。
//! 这个 crate 不依赖任何 UI 库，以后搬进守护进程时可以原样复用。

mod detect;
mod session;
mod status;

pub use detect::{classify, AgentState, Rules};
pub use session::{Exit, OutputHook, OutputSink, Session, SessionSpec, Size};
pub use status::Reported;
pub use vt100::Screen;
