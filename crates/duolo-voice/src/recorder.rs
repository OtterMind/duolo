//! 录音会话：启动配置的录音命令，结束时发送 SIGINT 并读取转写结果。
//!
//! 命令的输出约定：以 `~ ` 开头的行是识别中的临时结果（可选，界面用来实时显示）；
//! 以 `= ` 开头的行是最终结果；没有 `= ` 行时，其余所有输出合起来就是结果（兼容简单的脚本）。

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use duolo_core::{UserError, VoiceConfig};

/// 收到 SIGINT 后，留给命令完成转写的时间。
const TRANSCRIBE_TIMEOUT: Duration = Duration::from_secs(60);
/// 退出码 2 表示没有麦克风或语音识别权限。
const EXIT_NO_PERMISSION: i32 = 2;

/// 读线程收集到的输出。
#[derive(Debug, Default)]
struct Transcript {
    partial: String,
    final_text: Option<String>,
    plain: Vec<String>,
}

#[derive(Debug)]
pub struct Recorder {
    child: Child,
    started: Instant,
    transcript: Arc<Mutex<Transcript>>,
    reader: Option<JoinHandle<()>>,
}

impl Recorder {
    pub fn start(cfg: &VoiceConfig) -> Result<Self> {
        let (program, args) = cfg.command.split_first().ok_or(UserError::VoiceNotConfigured)?;
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| UserError::CommandNotFound { program: program.clone() })?;
        let stdout = child.stdout.take().context("voice command has no stdout")?;
        let transcript = Arc::new(Mutex::new(Transcript::default()));
        let sink = Arc::clone(&transcript);
        let reader = thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let mut t = sink.lock().unwrap_or_else(|p| p.into_inner());
                if let Some(text) = line.strip_prefix("~ ") {
                    t.partial = text.trim().to_string();
                } else if let Some(text) = line.strip_prefix("= ") {
                    t.final_text = Some(text.trim().to_string());
                } else if !line.trim().is_empty() {
                    t.plain.push(line.trim().to_string());
                }
            }
        });
        Ok(Self { child, started: Instant::now(), transcript, reader: Some(reader) })
    }

    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// 目前听到的内容（命令支持实时结果时才有）。
    pub fn partial(&self) -> String {
        self.transcript.lock().unwrap_or_else(|p| p.into_inner()).partial.clone()
    }

    /// 结束录音并等待转写结果。阻塞，请在后台线程调用。
    pub fn finish(mut self) -> Result<String> {
        interrupt(&self.child)?;
        let deadline = Instant::now() + TRANSCRIBE_TIMEOUT;
        let status = loop {
            if let Some(status) = self.child.try_wait()? {
                break status;
            }
            if Instant::now() >= deadline {
                let _ = self.child.kill();
                let _ = self.child.wait();
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
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn interrupt(child: &Child) -> Result<()> {
    let pid = libc::pid_t::try_from(child.id()).context("process id out of range")?;
    // SAFETY: 只是向自己启动的子进程发送信号，不涉及内存访问。
    let rc = unsafe { libc::kill(pid, libc::SIGINT) };
    if rc != 0 {
        bail!("cannot stop recording: {}", std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(unix))]
fn interrupt(_child: &Child) -> Result<()> {
    bail!("voice input is not supported on this platform")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(script: &str) -> VoiceConfig {
        VoiceConfig { command: vec!["sh".into(), "-c".into(), script.into()], ..VoiceConfig::default() }
    }

    #[test]
    fn transcript_after_interrupt() {
        // 模拟录音命令：一直等待，收到 SIGINT 后输出文字。
        let rec = Recorder::start(&cfg("trap 'echo 你好 世界; exit 0' INT; while true; do sleep 0.05; done")).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(rec.finish().unwrap(), "你好 世界");
    }

    #[test]
    fn partial_then_final() {
        let script = "echo '~ 你好'; trap 'echo \"= 你好 世界\"; exit 0' INT; while true; do sleep 0.05; done";
        let rec = Recorder::start(&cfg(script)).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while rec.partial().is_empty() {
            assert!(Instant::now() < deadline, "没有收到临时结果");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(rec.partial(), "你好");
        assert_eq!(rec.finish().unwrap(), "你好 世界");
    }

    #[test]
    fn permission_denied() {
        let rec = Recorder::start(&cfg("trap 'exit 2' INT; while true; do sleep 0.05; done")).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        let err = rec.finish().unwrap_err();
        assert_eq!(err.downcast_ref::<UserError>(), Some(&UserError::VoicePermission));
    }

    #[test]
    fn unconfigured() {
        assert!(Recorder::start(&VoiceConfig::default()).is_err());
    }
}
