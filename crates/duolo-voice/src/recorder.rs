//! 录音会话：启动配置的录音命令，结束时发送 SIGINT 并读取转写结果。

use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use duolo_core::VoiceConfig;

use crate::process::wait_output;

/// 收到 SIGINT 后，留给命令完成转写的时间。
const TRANSCRIBE_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug)]
pub struct Recorder {
    child: Child,
    started: Instant,
}

impl Recorder {
    pub fn start(cfg: &VoiceConfig) -> Result<Self> {
        let (program, args) = cfg.command.split_first().context("还没有配置语音命令（voice.command）")?;
        let child = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("无法启动语音命令 {program}"))?;
        Ok(Self { child, started: Instant::now() })
    }

    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// 结束录音并等待转写结果。阻塞，请在后台线程调用。
    pub fn finish(self) -> Result<String> {
        interrupt(&self.child)?;
        let text = wait_output(self.child, TRANSCRIBE_TIMEOUT)?;
        if text.is_empty() {
            bail!("没有识别到内容");
        }
        Ok(text)
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
    let pid = libc::pid_t::try_from(child.id()).context("进程号超出范围")?;
    // SAFETY: 只是向自己启动的子进程发送信号，不涉及内存访问。
    let rc = unsafe { libc::kill(pid, libc::SIGINT) };
    if rc != 0 {
        bail!("无法结束录音：{}", std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(unix))]
fn interrupt(_child: &Child) -> Result<()> {
    bail!("当前平台暂不支持语音输入")
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
    fn unconfigured() {
        assert!(Recorder::start(&VoiceConfig::default()).is_err());
    }
}
