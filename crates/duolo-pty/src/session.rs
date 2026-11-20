use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

use crate::detect::{classify, AgentState};

/// 保留的回滚行数。
const SCROLLBACK_LINES: usize = 5_000;
/// 状态识别时查看屏幕底部的行数。
const DETECT_TAIL_LINES: usize = 14;

/// 终端尺寸（字符格）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub rows: u16,
    pub cols: u16,
}

impl Size {
    pub fn new(rows: u16, cols: u16) -> Self {
        Self { rows: rows.max(2), cols: cols.max(10) }
    }

    fn to_pty(self) -> PtySize {
        PtySize { rows: self.rows, cols: self.cols, pixel_width: 0, pixel_height: 0 }
    }
}

/// 有新输出时的回调，界面层用它触发重绘。
pub type OutputHook = Arc<dyn Fn() + Send + Sync>;

/// 启动一个会话所需的信息。
#[derive(Clone)]
pub struct SessionSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
    pub size: Size,
    pub on_output: Option<OutputHook>,
}

impl std::fmt::Debug for SessionSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionSpec")
            .field("program", &self.program)
            .field("args", &self.args)
            .field("cwd", &self.cwd)
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

/// 进程退出信息。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exit {
    pub code: u32,
    pub success: bool,
}

/// 最近一次输出的时间，供状态识别使用。
#[derive(Debug)]
struct Activity {
    origin: Instant,
    last_output_ms: AtomicU64,
}

impl Activity {
    fn touch(&self) {
        let ms = u64::try_from(self.origin.elapsed().as_millis()).unwrap_or(u64::MAX);
        self.last_output_ms.store(ms, Ordering::Relaxed);
    }

    fn quiet_for(&self) -> Duration {
        let last = Duration::from_millis(self.last_output_ms.load(Ordering::Relaxed));
        self.origin.elapsed().saturating_sub(last)
    }
}

/// 一个运行中的 Agent CLI。
pub struct Session {
    terminal: Arc<Mutex<vt100::Parser>>,
    activity: Arc<Activity>,
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    started: Instant,
    size: Size,
    exit: Option<Exit>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("pid", &self.child.process_id())
            .field("size", &self.size)
            .field("exit", &self.exit)
            .finish_non_exhaustive()
    }
}

impl Session {
    pub fn spawn(spec: SessionSpec) -> Result<Self> {
        let pair = native_pty_system().openpty(spec.size.to_pty()).context("无法创建伪终端")?;

        let mut cmd = CommandBuilder::new(&spec.program);
        cmd.args(&spec.args);
        cmd.cwd(&spec.cwd);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        for (key, value) in &spec.env {
            cmd.env(key, value);
        }
        let child = pair
            .slave
            .spawn_command(cmd)
            .with_context(|| format!("无法启动 {}，请确认已安装并在 PATH 中", spec.program))?;
        // 子进程已持有从端，父进程这边关闭，进程退出时读端才能收到 EOF。
        drop(pair.slave);

        let terminal = Arc::new(Mutex::new(vt100::Parser::new(spec.size.rows, spec.size.cols, SCROLLBACK_LINES)));
        let activity = Arc::new(Activity { origin: Instant::now(), last_output_ms: AtomicU64::new(0) });
        let reader = pair.master.try_clone_reader().context("无法读取伪终端")?;
        let writer = pair.master.take_writer().context("无法写入伪终端")?;
        spawn_reader(reader, Arc::clone(&terminal), Arc::clone(&activity), spec.on_output);

        Ok(Self {
            terminal,
            activity,
            writer,
            master: pair.master,
            child,
            started: Instant::now(),
            size: spec.size,
            exit: None,
        })
    }

    /// 把字节原样写给 Agent，例如按键编码。
    pub fn write(&mut self, bytes: &[u8]) -> Result<()> {
        self.writer.write_all(bytes)?;
        self.writer.flush()?;
        Ok(())
    }

    /// 粘贴一段文字。Agent 开启了括号粘贴模式时会加上标记，避免换行被当成回车。
    pub fn paste(&mut self, text: &str) -> Result<()> {
        let bracketed = self.screen().bracketed_paste();
        let mut bytes = Vec::with_capacity(text.len() + 12);
        if bracketed {
            bytes.extend_from_slice(b"\x1b[200~");
        }
        bytes.extend_from_slice(text.as_bytes());
        if bracketed {
            bytes.extend_from_slice(b"\x1b[201~");
        }
        self.write(&bytes)
    }

    pub fn resize(&mut self, size: Size) -> Result<()> {
        if size == self.size {
            return Ok(());
        }
        self.master.resize(size.to_pty()).context("无法调整终端尺寸")?;
        self.screen().set_size(size.rows, size.cols);
        self.size = size;
        Ok(())
    }

    pub fn size(&self) -> Size {
        self.size
    }

    /// 在锁内读取当前画面。
    pub fn with_screen<R>(&self, f: impl FnOnce(&vt100::Screen) -> R) -> R {
        let parser = self.lock();
        f(parser.screen())
    }

    /// 查看回滚：`offset` 为离开底部的行数，0 表示回到最新画面。
    pub fn set_scrollback(&self, offset: usize) {
        self.screen().set_scrollback(offset);
    }

    pub fn scrollback(&self) -> usize {
        self.lock().screen().scrollback()
    }

    pub fn started(&self) -> Instant {
        self.started
    }

    pub fn pid(&self) -> Option<u32> {
        self.child.process_id()
    }

    /// 检查进程是否已退出，退出后结果会被缓存。
    pub fn poll_exit(&mut self) -> Option<Exit> {
        if self.exit.is_none() {
            if let Ok(Some(status)) = self.child.try_wait() {
                self.exit = Some(Exit { code: status.exit_code(), success: status.success() });
            }
        }
        self.exit
    }

    /// 根据退出状态、最近输出和屏幕底部内容判断 Agent 正在做什么。
    pub fn state(&mut self) -> AgentState {
        if let Some(exit) = self.poll_exit() {
            return AgentState::Exited(exit);
        }
        let tail = self.with_screen(|screen| tail_text(screen, DETECT_TAIL_LINES));
        classify(&tail, self.activity.quiet_for())
    }

    pub fn kill(&mut self) {
        if self.poll_exit().is_none() {
            let _ = self.child.kill();
        }
    }

    fn lock(&self) -> MutexGuard<'_, vt100::Parser> {
        // 解析器只在读线程和这里被访问，中毒时继续使用里面的状态即可。
        self.terminal.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn screen(&self) -> ScreenGuard<'_> {
        ScreenGuard(self.lock())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.kill();
    }
}

/// 让 `self.screen().xxx()` 可以直接调用可变的屏幕方法。
struct ScreenGuard<'a>(MutexGuard<'a, vt100::Parser>);

impl std::ops::Deref for ScreenGuard<'_> {
    type Target = vt100::Screen;
    fn deref(&self) -> &Self::Target {
        self.0.screen()
    }
}

impl std::ops::DerefMut for ScreenGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.screen_mut()
    }
}

fn spawn_reader(
    mut reader: Box<dyn Read + Send>,
    terminal: Arc<Mutex<vt100::Parser>>,
    activity: Arc<Activity>,
    on_output: Option<OutputHook>,
) {
    thread::Builder::new()
        .name("duolo-pty-reader".into())
        .spawn(move || {
            let mut buf = [0u8; 16 * 1024];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        terminal.lock().unwrap_or_else(|p| p.into_inner()).process(&buf[..n]);
                        activity.touch();
                        if let Some(hook) = &on_output {
                            hook();
                        }
                    }
                }
            }
            if let Some(hook) = &on_output {
                hook();
            }
        })
        .expect("无法创建读线程");
}

/// 屏幕底部 `lines` 行非空文本。
fn tail_text(screen: &vt100::Screen, lines: usize) -> String {
    let (_, cols) = screen.size();
    let rows: Vec<String> = screen.rows(0, cols).collect();
    let mut tail: Vec<&str> = rows.iter().rev().map(|r| r.trim_end()).filter(|r| !r.is_empty()).take(lines).collect();
    tail.reverse();
    tail.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(program: &str, args: &[&str]) -> SessionSpec {
        SessionSpec {
            program: program.into(),
            args: args.iter().map(|s| s.to_string()).collect(),
            cwd: std::env::temp_dir(),
            env: Vec::new(),
            size: Size::new(10, 40),
            on_output: None,
        }
    }

    fn wait_until(mut f: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if f() {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        panic!("等待超时");
    }

    #[test]
    fn renders_output_and_exits() {
        let mut s = Session::spawn(spec("sh", &["-c", "printf 'hello\\nworld'"])).unwrap();
        wait_until(|| s.poll_exit().is_some());
        wait_until(|| s.with_screen(|scr| scr.contents().contains("world")));
        assert!(matches!(s.state(), AgentState::Exited(Exit { success: true, .. })));
    }

    #[test]
    fn echoes_input_and_resizes() {
        let mut s = Session::spawn(spec("cat", &[])).unwrap();
        s.write(b"duolo\r").unwrap();
        wait_until(|| s.with_screen(|scr| scr.contents().contains("duolo")));
        s.resize(Size::new(20, 60)).unwrap();
        assert_eq!(s.with_screen(|scr| scr.size()), (20, 60));
        s.kill();
        wait_until(|| s.poll_exit().is_some());
    }
}
