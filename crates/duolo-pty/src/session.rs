use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use duolo_core::proctree::{Members, ProcessTree};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

use crate::detect::{classify, AgentState};

/// 保留的回滚行数。
const SCROLLBACK_LINES: usize = 5_000;
/// 状态识别时查看屏幕底部的行数。
const DETECT_TAIL_LINES: usize = 14;

/// 请 Agent 自己退出后等多久（它要保存会话），还不退出就强制结束。
const GRACE: Duration = Duration::from_millis(800);
/// 强制结束后最长等多久。
const STOP_TIMEOUT: Duration = Duration::from_secs(2);

/// 不支持括号粘贴的 Agent，启动多久之后才当作准备好了。
const PLAIN_READY_AFTER: Duration = Duration::from_secs(1);

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

/// 保留的原始输出字节数，新的界面接入时用来重放。超出后改用当前画面重放。
const HISTORY_BYTES: usize = 8 * 1024 * 1024;

/// 有新输出时的回调，界面层用它触发重绘。
pub type OutputHook = Arc<dyn Fn() + Send + Sync>;

/// 原始输出的订阅者。桌面端把字节原样交给 xterm.js 渲染。
pub type OutputSink = Box<dyn FnMut(&[u8]) + Send>;

/// 原始输出的历史和订阅者。读线程先更新解析器再更新这里，两把锁的顺序固定。
#[derive(Default)]
struct Tap {
    history: Vec<u8>,
    /// 历史被截断过，重放时改用解析器里的当前画面。
    truncated: bool,
    sink: Option<OutputSink>,
}

impl Tap {
    fn push(&mut self, bytes: &[u8]) {
        if self.history.len() + bytes.len() > HISTORY_BYTES {
            self.history.clear();
            self.truncated = true;
        }
        if !self.truncated {
            self.history.extend_from_slice(bytes);
        }
        if let Some(sink) = &mut self.sink {
            sink(bytes);
        }
    }
}

/// 启动一个会话所需的信息。
#[derive(Clone)]
pub struct SessionSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
    /// 不传给子进程的环境变量。
    pub env_remove: Vec<String>,
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
    tap: Arc<Mutex<Tap>>,
    activity: Arc<Activity>,
    /// 要写给 Agent 的内容，由写线程写（见 [`spawn_writer`]）。
    writer: Sender<Vec<u8>>,
    master: Box<dyn MasterPty + Send>,
    /// 停止后为 None。
    child: Option<Box<dyn Child + Send + Sync>>,
    tree: ProcessTree,
    pid: Option<u32>,
    started: Instant,
    size: Size,
    exit: Option<Exit>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("pid", &self.pid)
            .field("size", &self.size)
            .field("exit", &self.exit)
            .finish_non_exhaustive()
    }
}

impl Session {
    pub fn spawn(spec: SessionSpec) -> Result<Self> {
        let pair = native_pty_system().openpty(spec.size.to_pty()).context("cannot open a pseudo terminal")?;

        let program = duolo_core::process::resolve(&spec.program);
        let mut cmd = if duolo_core::process::is_batch_file(&program) {
            // Windows 的批处理文件（npm 装的 claude.cmd）要交给 cmd.exe。portable-pty 会按普通程序的规则给参数加引号，
            // cmd.exe 不认，所以把整条命令放进环境变量，让 cmd 自己展开（见 process::batch_command_line）。
            let line = duolo_core::process::batch_command_line(program.as_os_str(), &spec.args)
                .context("arguments for a batch file cannot contain line breaks")?;
            let mut cmd = CommandBuilder::new("cmd.exe");
            cmd.args(["/d", "/c", "%DUOLO_LAUNCH%"]);
            cmd.env("DUOLO_LAUNCH", line);
            cmd
        } else {
            let mut cmd = CommandBuilder::new(program);
            cmd.args(&spec.args);
            cmd
        };
        cmd.cwd(&spec.cwd);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        for key in &spec.env_remove {
            cmd.env_remove(key);
        }
        for (key, value) in &spec.env {
            cmd.env(key, value);
        }
        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|_| duolo_core::UserError::CommandNotFound { program: spec.program.clone() })?;
        // 子进程已持有从端，父进程这边关闭，进程退出时读端才能收到 EOF。
        drop(pair.slave);
        let pid = child.process_id();
        let tree = process_tree(child.as_ref());

        let terminal = Arc::new(Mutex::new(vt100::Parser::new(spec.size.rows, spec.size.cols, SCROLLBACK_LINES)));
        let activity = Arc::new(Activity { origin: Instant::now(), last_output_ms: AtomicU64::new(0) });
        let tap = Arc::new(Mutex::new(Tap::default()));
        let reader = pair.master.try_clone_reader().context("cannot read from the pseudo terminal")?;
        let writer = pair.master.take_writer().context("cannot write to the pseudo terminal")?;
        let writer = spawn_writer(writer);
        spawn_reader(
            reader,
            writer.clone(),
            Arc::clone(&terminal),
            Arc::clone(&tap),
            Arc::clone(&activity),
            spec.on_output,
        );

        Ok(Self {
            terminal,
            tap,
            activity,
            writer,
            master: pair.master,
            child: Some(child),
            tree,
            pid,
            started: Instant::now(),
            size: spec.size,
            exit: None,
        })
    }

    /// 把字节原样写给 Agent，例如按键编码。马上返回，Agent 暂时不读也不会卡住调用方。
    pub fn write(&mut self, bytes: &[u8]) -> Result<()> {
        self.writer.send(bytes.to_vec()).context("the agent has exited")
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
        self.master.resize(size.to_pty()).context("cannot resize the pseudo terminal")?;
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

    /// 订阅原始输出：先把目前为止的输出交给 `sink` 重放，之后的新输出实时推送。
    /// 同一时间只有一个订阅者，新的订阅会替换旧的。
    pub fn attach(&self, mut sink: OutputSink) {
        let parser = self.lock();
        let mut tap = self.tap.lock().unwrap_or_else(|p| p.into_inner());
        if tap.truncated {
            sink(&parser.screen().state_formatted());
        } else {
            sink(&tap.history);
        }
        tap.sink = Some(sink);
    }

    pub fn detach(&self) {
        self.tap.lock().unwrap_or_else(|p| p.into_inner()).sink = None;
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
        self.pid
    }

    /// 检查进程是否已退出，退出后结果会被缓存。
    pub fn poll_exit(&mut self) -> Option<Exit> {
        if self.exit.is_none() {
            if let Some(Ok(Some(status))) = self.child.as_mut().map(|c| c.try_wait()) {
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

    /// 画面上有内容、而且已经可以接收粘贴了。Claude Code、Codex 准备好输入框时会打开括号粘贴模式；
    /// 不支持这个模式的 Agent 只看画面是不是空的。
    pub fn is_ready_for_input(&self) -> bool {
        let (bracketed, blank) = self.with_screen(|s| (s.bracketed_paste(), s.contents().trim().is_empty()));
        !blank && (bracketed || self.started.elapsed() >= PLAIN_READY_AFTER)
    }

    /// 结束 Agent 和它启动的所有进程，等它们真的消失。会等一会儿，不要拿着锁调用。
    pub fn stop(&mut self) {
        let Some(mut child) = self.child.take() else { return };
        let members = self.tree.members();
        stop_process(child.as_mut(), &self.tree, members, self.exit.is_some());
        if let Ok(Some(status)) = child.try_wait() {
            self.exit.get_or_insert(Exit { code: status.exit_code(), success: status.success() });
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
        // 没有先调用 stop：在后台结束，不让调用方等。自己退出的就不管了（它可能特意留下了开发服务器）。
        if self.poll_exit().is_some() {
            return;
        }
        let Some(mut child) = self.child.take() else { return };
        let tree = std::mem::replace(&mut self.tree, ProcessTree::empty());
        thread::spawn(move || stop_process(child.as_mut(), &tree, tree.members(), false));
    }
}

fn stop_process(child: &mut (dyn Child + Send + Sync), tree: &ProcessTree, members: Members, exited: bool) {
    if !exited {
        tree.hang_up();
        wait_exit(child, GRACE);
    }
    tree.kill(Some(members), STOP_TIMEOUT);
    if !exited && !wait_exit(child, Duration::ZERO) {
        // 进程树没能结束它（Windows 上建不了作业对象）。
        let _ = child.kill();
        wait_exit(child, STOP_TIMEOUT);
    }
}

/// 最多等 `timeout`，返回退出了没有。
fn wait_exit(child: &mut (dyn Child + Send + Sync), timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            _ if Instant::now() >= deadline => return false,
            _ => thread::sleep(Duration::from_millis(10)),
        }
    }
}

#[cfg(unix)]
fn process_tree(child: &(dyn Child + Send + Sync)) -> ProcessTree {
    child.process_id().map_or_else(ProcessTree::empty, ProcessTree::of_pid)
}

#[cfg(windows)]
fn process_tree(child: &(dyn Child + Send + Sync)) -> ProcessTree {
    child.as_raw_handle().map_or_else(ProcessTree::empty, ProcessTree::of_handle)
}

/// 写线程：Agent 不读输入时，往终端里写会一直阻塞（例如粘贴一大段文字）。写失败（Agent 退出了）时线程结束。
fn spawn_writer(mut writer: Box<dyn Write + Send>) -> Sender<Vec<u8>> {
    let (sender, receiver) = mpsc::channel::<Vec<u8>>();
    thread::Builder::new()
        .name("duolo-pty-writer".into())
        .spawn(move || {
            for bytes in receiver {
                if writer.write_all(&bytes).and_then(|()| writer.flush()).is_err() {
                    break;
                }
            }
        })
        .expect("cannot spawn writer thread");
    sender
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
    writer: Sender<Vec<u8>>,
    terminal: Arc<Mutex<vt100::Parser>>,
    tap: Arc<Mutex<Tap>>,
    activity: Arc<Activity>,
    on_output: Option<OutputHook>,
) {
    thread::Builder::new()
        .name("duolo-pty-reader".into())
        .spawn(move || {
            let mut buf = [0u8; 16 * 1024];
            // Windows 的伪终端（ConPTY）一启动先问光标在哪，得到回答之前什么都不输出。没有界面订阅的任务没人回答，
            // 所以由我们回答，也不再交给界面，免得它又回答一次、变成发给 Agent 的输入。
            let mut asks_cursor = cfg!(windows);
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let mut bytes = std::borrow::Cow::Borrowed(&buf[..n]);
                        if asks_cursor {
                            if let Some(rest) = without_cursor_query(&bytes) {
                                asks_cursor = false;
                                let _ = writer.send(b"\x1b[1;1R".to_vec());
                                bytes = std::borrow::Cow::Owned(rest);
                            }
                        }
                        let mut parser = terminal.lock().unwrap_or_else(|p| p.into_inner());
                        parser.process(&bytes);
                        tap.lock().unwrap_or_else(|p| p.into_inner()).push(&bytes);
                        drop(parser);
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
        .expect("cannot spawn reader thread");
}

/// 去掉第一个"光标在哪"的询问（`ESC [ 6 n`）；没有时返回 None。
fn without_cursor_query(bytes: &[u8]) -> Option<Vec<u8>> {
    const QUERY: &[u8] = b"\x1b[6n";
    let at = bytes.windows(QUERY.len()).position(|w| w == QUERY)?;
    Some([&bytes[..at], &bytes[at + QUERY.len()..]].concat())
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
            env_remove: Vec::new(),
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
        panic!("timed out");
    }

    #[test]
    fn removes_the_cursor_query() {
        assert_eq!(without_cursor_query(b"\x1b[6n").unwrap(), b"");
        assert_eq!(without_cursor_query(b"\x1b[?25l\x1b[6nhi").unwrap(), b"\x1b[?25lhi");
        assert!(without_cursor_query(b"hello \x1b[6m").is_none());
    }

    #[test]
    fn renders_output_and_exits() {
        let mut s = Session::spawn(spec("sh", &["-c", "printf 'hello\\nworld'"])).unwrap();
        wait_until(|| s.poll_exit().is_some());
        wait_until(|| s.with_screen(|scr| scr.contents().contains("world")));
        assert!(matches!(s.state(), AgentState::Exited(Exit { success: true, .. })));
    }

    #[test]
    fn attach_replays_then_streams() {
        let mut s = Session::spawn(spec("cat", &[])).unwrap();
        s.write(b"one\r").unwrap();
        wait_until(|| s.with_screen(|scr| scr.contents().contains("one")));

        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink_seen = Arc::clone(&seen);
        s.attach(Box::new(move |bytes| sink_seen.lock().unwrap().extend_from_slice(bytes)));
        s.write(b"two\r").unwrap();
        wait_until(|| String::from_utf8_lossy(&seen.lock().unwrap()).contains("two"));
        let text = String::from_utf8_lossy(&seen.lock().unwrap()).into_owned();
        assert!(text.find("one").unwrap() < text.find("two").unwrap(), "{text:?}");
        s.stop();
    }

    #[test]
    fn echoes_input_and_resizes() {
        let mut s = Session::spawn(spec("cat", &[])).unwrap();
        s.write(b"duolo\r").unwrap();
        wait_until(|| s.with_screen(|scr| scr.contents().contains("duolo")));
        s.resize(Size::new(20, 60)).unwrap();
        assert_eq!(s.with_screen(|scr| scr.size()), (20, 60));
        s.stop();
        assert!(s.poll_exit().is_some());
    }

    #[test]
    fn writing_never_blocks() {
        // 一直不读输入的 Agent：粘贴很长的文字也要马上返回。
        let mut s = Session::spawn(spec("sleep", &["30"])).unwrap();
        let started = Instant::now();
        s.paste(&"x".repeat(4 * 1024 * 1024)).unwrap();
        s.write(b"\r").unwrap();
        assert!(started.elapsed() < Duration::from_secs(1));
        s.stop();
        assert!(s.poll_exit().is_some());
    }

    #[test]
    #[cfg(unix)]
    fn stop_ends_background_processes() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("log");
        // 后台进程忽略挂断信号，另开一个会话，一直写文件。
        let script = format!(
            "perl -e 'use POSIX; if (fork == 0) {{ POSIX::setsid(); $SIG{{HUP}} = q(IGNORE); \
             while (1) {{ open F, q(>>), q({}); print F qq(x); close F; select(undef, undef, undef, 0.02) }} }}; sleep 60'; true",
            log.display()
        );
        let mut s = Session::spawn(spec("sh", &["-c", &script])).unwrap();
        wait_until(|| std::fs::metadata(&log).is_ok_and(|m| m.len() > 3));
        s.stop();
        let size = || std::fs::metadata(&log).unwrap().len();
        let before = size();
        thread::sleep(Duration::from_millis(300));
        assert_eq!(size(), before, "后台进程也要结束");
    }

    #[test]
    fn ready_only_after_something_is_drawn() {
        let mut s = Session::spawn(spec("sh", &["-c", "sleep 0.4; printf '\\033[?2004hhello'; cat"])).unwrap();
        assert!(!s.is_ready_for_input(), "画面还是空的");
        wait_until(|| s.is_ready_for_input());
        assert!(s.started().elapsed() >= Duration::from_millis(300));
        s.stop();
    }
}
