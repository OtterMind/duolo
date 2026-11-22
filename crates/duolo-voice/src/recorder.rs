//! 录音会话：启动配置的录音命令，结束时通知它停止（macOS / Linux 发 SIGINT，Windows 往标准输入写一行 `stop`），
//! 再读取转写结果。
//!
//! 命令的输出约定：以 `~ ` 开头的行是识别中的临时结果（可选，界面用来实时显示）；
//! 以 `^ ` 开头的行是当前音量（0~1，可选）；
//! 以 `= ` 开头的行是最终结果；没有 `= ` 行时，其余所有输出合起来就是结果（兼容简单的脚本）。

use std::io::{BufRead, BufReader};
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use duolo_core::proctree::ProcessTree;
use duolo_core::{UserError, VoiceConfig};

/// 收到 SIGINT 后，留给命令完成转写的时间。
const TRANSCRIBE_TIMEOUT: Duration = Duration::from_secs(60);
/// 结束命令时，最长等它消失多久。
const KILL_WAIT: Duration = Duration::from_secs(2);
/// 退出码 2 表示没有麦克风或语音识别权限。
const EXIT_NO_PERMISSION: i32 = 2;

/// 读线程收集到的输出。
#[derive(Debug, Default)]
struct Transcript {
    partial: String,
    level: Option<f32>,
    final_text: Option<String>,
    plain: Vec<String>,
}

#[derive(Debug)]
pub struct Recorder {
    child: Child,
    tree: ProcessTree,
    started: Instant,
    transcript: Arc<Mutex<Transcript>>,
    reader: Option<JoinHandle<()>>,
}

impl Recorder {
    pub fn start(cfg: &VoiceConfig) -> Result<Self> {
        let (program, args) = cfg.command.split_first().ok_or(UserError::VoiceNotConfigured)?;
        let mut cmd = duolo_core::process::command(program);
        cmd.args(args)
            // Windows 没有 SIGINT，结束录音时往标准输入写一行 `stop`（见 interrupt）。
            .stdin(if cfg!(unix) { Stdio::null() } else { Stdio::piped() })
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let (mut child, tree) = duolo_core::process::spawn_tree(&mut cmd)
            .map_err(|_| UserError::CommandNotFound { program: program.clone() })?;
        let stdout = child.stdout.take().context("voice command has no stdout")?;
        let transcript = Arc::new(Mutex::new(Transcript::default()));
        let sink = Arc::clone(&transcript);
        let reader = thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let mut t = sink.lock().unwrap_or_else(|p| p.into_inner());
                if let Some(level) = line.strip_prefix("^ ") {
                    t.level = level.trim().parse().ok();
                } else if let Some(text) = line.strip_prefix("~ ") {
                    t.partial = text.trim().to_string();
                } else if let Some(text) = line.strip_prefix("= ") {
                    t.final_text = Some(text.trim().to_string());
                } else if !line.trim().is_empty() {
                    t.plain.push(line.trim().to_string());
                }
            }
        });
        Ok(Self { child, tree, started: Instant::now(), transcript, reader: Some(reader) })
    }

    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// 目前听到的内容（命令支持实时结果时才有）。
    pub fn partial(&self) -> String {
        self.transcript.lock().unwrap_or_else(|p| p.into_inner()).partial.clone()
    }

    /// 最近的音量（0~1）。命令不报告音量时为 None。
    pub fn level(&self) -> Option<f32> {
        self.transcript.lock().unwrap_or_else(|p| p.into_inner()).level
    }

    /// 结束录音并等待转写结果。阻塞，请在后台线程调用。
    pub fn finish(mut self) -> Result<String> {
        interrupt(&mut self.child)?;
        let deadline = Instant::now() + TRANSCRIBE_TIMEOUT;
        let status = loop {
            if let Some(status) = self.child.try_wait()? {
                break status;
            }
            if Instant::now() >= deadline {
                self.kill();
                bail!("voice command did not finish within {} seconds", TRANSCRIBE_TIMEOUT.as_secs());
            }
            thread::sleep(Duration::from_millis(30));
        };
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        if status.code() == Some(EXIT_NO_PERMISSION) {
            return Err(UserError::VoicePermission.into());
        }
        if !status.success() {
            bail!("voice command exited with {status}");
        }
        let t = self.transcript.lock().unwrap_or_else(|p| p.into_inner());
        let text = t.final_text.clone().unwrap_or_else(|| t.plain.join(" "));
        let text = if text.trim().is_empty() { t.partial.clone() } else { text };
        if text.trim().is_empty() {
            return Err(UserError::NothingRecognized.into());
        }
        Ok(text.trim().to_string())
    }

    /// 放弃这次录音。
    pub fn cancel(mut self) {
        self.kill();
    }

    fn kill(&mut self) {
        self.tree.kill(None, KILL_WAIT);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// macOS、Linux：给识别命令发 SIGINT。
#[cfg(unix)]
#[allow(unsafe_code)]
fn interrupt(child: &mut Child) -> Result<()> {
    let pid = libc::pid_t::try_from(child.id()).context("process id out of range")?;
    // SAFETY: 只是向自己启动的子进程发送信号，不涉及内存访问。
    let rc = unsafe { libc::kill(pid, libc::SIGINT) };
    if rc != 0 {
        bail!("cannot stop recording: {}", std::io::Error::last_os_error());
    }
    Ok(())
}

/// Windows：往识别命令的标准输入写一行 `stop`，再关掉标准输入。
#[cfg(not(unix))]
#[allow(clippy::unnecessary_wraps)]
fn interrupt(child: &mut Child) -> Result<()> {
    use std::io::Write;
    let mut stdin = child.stdin.take().context("voice command has no stdin")?;
    // 写不进去说明它已经自己退出了（例如没有麦克风）：退出码会说明原因。
    let _ = stdin.write_all(b"stop\n");
    drop(stdin);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 模拟录音命令：先执行 `before`，收到停止信号（Windows 上是标准输入的 `stop`）后执行 `then`。
    fn cfg(before: &str, then: &str) -> VoiceConfig {
        let script = if cfg!(unix) {
            format!("{before} trap '{then}' INT; while true; do sleep 0.05; done")
        } else {
            format!("{before} read -r line; {then}")
        };
        VoiceConfig { command: vec!["sh".into(), "-c".into(), script], ..VoiceConfig::default() }
    }

    #[test]
    fn transcript_after_interrupt() {
        let rec = Recorder::start(&cfg("", "echo hello world; exit 0")).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(rec.finish().unwrap(), "hello world");
    }

    #[test]
    fn partial_then_final() {
        let rec = Recorder::start(&cfg("echo '^ 0.40'; echo '~ hello';", "echo \"= hello world\"; exit 0")).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while rec.partial().is_empty() {
            assert!(Instant::now() < deadline, "no partial result");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(rec.partial(), "hello");
        assert_eq!(rec.level(), Some(0.4));
        assert_eq!(rec.finish().unwrap(), "hello world");
    }

    #[test]
    fn permission_denied() {
        // 停止时才发现没有权限；以及一启动就因为没有麦克风退出了（Windows 的识别脚本是这样）。
        for before in ["", if cfg!(windows) { "exit 2;" } else { "" }] {
            let rec = Recorder::start(&cfg(before, "exit 2")).unwrap();
            std::thread::sleep(Duration::from_millis(500));
            let err = rec.finish().unwrap_err();
            assert_eq!(err.downcast_ref::<UserError>(), Some(&UserError::VoicePermission));
        }
    }

    #[test]
    fn unconfigured() {
        assert!(Recorder::start(&VoiceConfig::default()).is_err());
    }
}
