//! 语音输入状态机。
//!
//! ```text
//! 空闲 --⌃Space--> 录音中 --⌃Space--> 处理中（转写 + 整理，后台线程）--> 待确认
//!  ^                                                                     |
//!  +------------------------- ↵ 发送 / Tab 填入 / Esc 放弃 ---------------+
//! ```
//!
//! 待确认的文字由界面画在 Agent 输入行的光标处（类似输入法的候选字），
//! 确认之前不会写进 Agent，所以边说边改不会弄乱 Agent 的输入框。

use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use std::time::Duration;

use anyhow::Result;
use duolo_core::{AgentKey, VoiceConfig};
use duolo_voice::{organize, OrganizeContext, Recorder};

/// 语音结果要送到哪里。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceTarget {
    /// 某个 Agent 的输入行。
    Agent(AgentKey),
    /// 首页的输入框。
    Home,
}

#[derive(Debug)]
pub enum Phase {
    Idle,
    Recording {
        recorder: Recorder,
        target: VoiceTarget,
        prefix: String,
    },
    Processing {
        prefix: String,
    },
    /// 等待确认。`raw` 是原话，`text` 是当前显示的内容（整理后的或用户改过的）。
    Pending {
        target: VoiceTarget,
        raw: String,
        text: String,
    },
}

/// 后台线程送回的结果。
#[derive(Debug)]
enum Outcome {
    Ready { target: VoiceTarget, prefix: String, raw: String, organized: Option<String> },
    Failed { target: VoiceTarget, prefix: String, error: String },
}

/// 一次后台处理完成后，交给 App 的结果。
#[derive(Debug)]
pub enum Delivery {
    /// 送进首页输入框的文字。
    HomeText(String),
    /// 出错提示。
    Error(String),
}

#[derive(Debug)]
pub struct Voice {
    phase: Phase,
    tx: Sender<Outcome>,
    rx: Receiver<Outcome>,
}

impl Default for Voice {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self { phase: Phase::Idle, tx, rx }
    }
}

impl Voice {
    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    pub fn is_idle(&self) -> bool {
        matches!(self.phase, Phase::Idle)
    }

    /// 某个 Agent 是否有等待确认的语音文字。
    pub fn pending_for(&self, key: AgentKey) -> Option<&str> {
        match &self.phase {
            Phase::Pending { target: VoiceTarget::Agent(k), text, .. } if *k == key => Some(text),
            _ => None,
        }
    }

    /// 开始录音；正在待确认时，新说的内容接在后面。
    pub fn start(&mut self, cfg: &VoiceConfig, target: VoiceTarget) -> Result<()> {
        let prefix = match std::mem::replace(&mut self.phase, Phase::Idle) {
            Phase::Pending { text, .. } => text,
            other => {
                self.phase = other;
                if !self.is_idle() {
                    return Ok(());
                }
                String::new()
            }
        };
        let recorder = Recorder::start(cfg)?;
        self.phase = Phase::Recording { recorder, target, prefix };
        Ok(())
    }

    /// 结束录音，转写和整理在后台进行。
    pub fn stop(&mut self, cfg: &VoiceConfig, ctx: OrganizeContext) {
        let Phase::Recording { recorder, target, prefix } = std::mem::replace(&mut self.phase, Phase::Idle) else {
            return;
        };
        self.phase = Phase::Processing { prefix: prefix.clone() };
        let cfg = cfg.clone();
        let tx = self.tx.clone();
        thread::spawn(move || {
            let outcome = match recorder.finish() {
                Ok(raw) => {
                    let organized = cfg.organize.then(|| organize(&cfg, &raw, &ctx).ok()).flatten();
                    Outcome::Ready { target, prefix, raw, organized }
                }
                Err(err) => Outcome::Failed { target, prefix, error: format!("{err:#}") },
            };
            let _ = tx.send(outcome);
        });
    }

    /// 取消：录音中则丢弃录音，待确认则丢弃文字。
    pub fn cancel(&mut self) {
        if let Phase::Recording { recorder, .. } = std::mem::replace(&mut self.phase, Phase::Idle) {
            recorder.cancel();
        }
    }

    /// 待确认时按 Esc：先还原成原话，再按一次才放弃。
    pub fn revert_or_cancel(&mut self) {
        if let Phase::Pending { raw, text, .. } = &mut self.phase {
            if text != raw {
                *text = raw.clone();
                return;
            }
        }
        self.cancel();
    }

    /// 取走待确认的文字（发送或填入时调用）。
    pub fn take_pending(&mut self) -> Option<String> {
        match std::mem::replace(&mut self.phase, Phase::Idle) {
            Phase::Pending { text, .. } => Some(text),
            other => {
                self.phase = other;
                None
            }
        }
    }

    /// 对待确认的文字做简单编辑。
    pub fn edit_pending(&mut self, f: impl FnOnce(&mut String)) {
        if let Phase::Pending { text, .. } = &mut self.phase {
            f(text);
        }
    }

    pub fn recording_elapsed(&self) -> Option<Duration> {
        match &self.phase {
            Phase::Recording { recorder, .. } => Some(recorder.elapsed()),
            _ => None,
        }
    }

    /// 收取后台结果。送进 Agent 的结果进入待确认状态；其他结果交给调用方。
    pub fn poll(&mut self) -> Option<Delivery> {
        let outcome = self.rx.try_recv().ok()?;
        match outcome {
            Outcome::Ready { target, prefix, raw, organized } => {
                let raw = join(&prefix, &raw);
                let text = organized.map_or_else(|| raw.clone(), |o| join(&prefix, &o));
                match target {
                    VoiceTarget::Home => {
                        self.phase = Phase::Idle;
                        Some(Delivery::HomeText(text))
                    }
                    VoiceTarget::Agent(_) => {
                        self.phase = Phase::Pending { target, raw, text };
                        None
                    }
                }
            }
            Outcome::Failed { target, prefix, error } => {
                self.phase = if prefix.is_empty() {
                    Phase::Idle
                } else {
                    Phase::Pending { target, raw: prefix.clone(), text: prefix }
                };
                Some(Delivery::Error(error))
            }
        }
    }
}

fn join(prefix: &str, more: &str) -> String {
    if prefix.is_empty() {
        more.to_string()
    } else {
        format!("{prefix}{more}")
    }
}
