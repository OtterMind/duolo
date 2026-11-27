//! 电脑这边怎么回应手机（约定见 docs/mobile.md「电脑端要遵守的」）。不碰网络，方便测试。

use duolo_core::wire::{DesktopMessage, Overview, Packet, PhoneMessage};
use duolo_core::{Command, CommandResult, EventKind, TaskEvent, TaskId};

use crate::saved::{DoneCommand, Saved};

/// 电脑上的数据和能做的事，由桌面应用提供。
pub trait Host: Send + Sync + 'static {
    /// 列表页的全部内容（`rev` 由连接自己填）。
    fn overview(&self) -> Overview;
    /// 任务第 `after` 条之后的事件。
    fn events_since(&self, task: TaskId, after: u64) -> Vec<TaskEvent>;
    /// 最近一次审阅：结束时间（Unix 毫秒）和全文。
    fn review(&self, task: TaskId) -> Option<(u64, String)>;
    /// 执行一条命令（可能要几秒，在后台线程里调用）。
    fn execute(&self, command: Command) -> CommandResult;
}

/// 一次最多发多少条事件。
pub const PAGE: usize = 200;
/// 一条消息加密前最多多大。服务器的上限是加密后 256 KB，base64 会大三分之一。
pub const MAX_PLAIN: usize = 180 * 1024;
/// 单条事件里的文字太长时截到这么长。
const MAX_TEXT: usize = 16 * 1024;

pub fn encode(message: DesktopMessage) -> Vec<u8> {
    serde_json::to_vec(&Packet::new(message)).expect("messages serialize")
}

/// 解出手机发来的消息。不认识的 `type`、更新的版本返回空（忽略，不报错）。
pub fn decode(plaintext: &[u8]) -> Option<PhoneMessage> {
    let packet: Packet<PhoneMessage> = serde_json::from_slice(plaintext).ok()?;
    (packet.v == duolo_core::wire::VERSION).then_some(packet.message)
}

/// 任务的事件分成几条消息：每条最多 [`PAGE`] 条事件、不超过 [`MAX_PLAIN`]。
/// 只发第一页：手机看 `latest` 没到就接着要。
pub fn events_page(task: TaskId, events: Vec<TaskEvent>, latest: u64) -> DesktopMessage {
    let mut events: Vec<TaskEvent> = events.into_iter().take(PAGE).map(shorten).collect();
    loop {
        let message = DesktopMessage::Events { task, events: events.clone(), latest };
        if events.len() <= 1 || encode(message.clone()).len() <= MAX_PLAIN {
            return message;
        }
        events.truncate(events.len() / 2);
    }
}

/// 太长的对话截短，免得一条事件就超过消息的上限。
fn shorten(mut event: TaskEvent) -> TaskEvent {
    if let EventKind::Message { text, .. } = &mut event.kind {
        if text.len() > MAX_TEXT {
            let mut end = MAX_TEXT;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
            text.push('…');
        }
    }
    event
}

/// 回应 `fetch`：事件的第一页，有审阅结论时再带上全文。
pub fn fetch(host: &dyn Host, task: TaskId, after: u64) -> Vec<DesktopMessage> {
    let events = host.events_since(task, after);
    let latest = events.last().map_or(after, |e| e.seq);
    let mut out = vec![events_page(task, events, latest)];
    if let Some((finished_at, output)) = host.review(task) {
        let mut output = output;
        if output.len() > MAX_PLAIN / 2 {
            let mut end = MAX_PLAIN / 2;
            while !output.is_char_boundary(end) {
                end -= 1;
            }
            output.truncate(end);
        }
        out.push(DesktopMessage::Review { task, finished_at, output });
    }
    out
}

/// 一条命令现在该怎么处理。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// 执行过了，回上次的结果。
    Done(CommandResult),
    /// 不执行，直接回这个结果。
    Refuse(CommandResult),
    Run,
}

/// 同一个编号只执行一次；太晚到的不执行；回答必须带上它回答的是哪条提示。
pub fn check(saved: &Saved, id: &str, at: u64, command: &Command, now: u64) -> Verdict {
    if let Some(result) = saved.result_of(id) {
        return Verdict::Done(result.clone());
    }
    let refuse = |code: &str| Verdict::Refuse(CommandResult { ok: false, task: None, error: Some(code.into()) });
    if command.is_expired(at, now) {
        return refuse("commandExpired");
    }
    if matches!(command, Command::Answer { prompt: None, .. }) {
        return refuse("promptChanged");
    }
    Verdict::Run
}

/// 记下执行的结果。
pub fn remember(saved: &mut Saved, id: &str, now: u64, result: &CommandResult) {
    saved.done.retain(|d| d.id != id);
    saved.done.push(DoneCommand { id: id.to_string(), at: now, result: result.clone() });
}

#[cfg(test)]
mod tests {
    use duolo_core::Speaker;

    use super::*;

    fn message(task: u64, seq: u64, text: &str) -> TaskEvent {
        TaskEvent {
            task: TaskId(task),
            seq,
            at: seq,
            kind: EventKind::Message { speaker: Speaker::Agent, agent: 0, text: text.into() },
        }
    }

    #[test]
    fn decodes_phone_messages_and_ignores_unknown_ones() {
        assert_eq!(decode(br#"{"v":1,"type":"sync"}"#), Some(PhoneMessage::Sync));
        assert_eq!(
            decode(br#"{"v":1,"type":"fetch","task":3,"after":10}"#),
            Some(PhoneMessage::Fetch { task: TaskId(3), after: 10 })
        );
        assert_eq!(decode(br#"{"v":1,"type":"dance"}"#), None);
        assert_eq!(decode(br#"{"v":2,"type":"sync"}"#), None);
        assert_eq!(decode(b"not json"), None);
    }

    #[test]
    fn pages_events_by_count_and_size() {
        let many: Vec<_> = (1..=450).map(|n| message(1, n, "hi")).collect();
        let DesktopMessage::Events { events, latest, .. } = events_page(TaskId(1), many, 450) else { panic!() };
        assert_eq!((events.len(), events[0].seq, latest), (PAGE, 1, 450));

        let big = "x".repeat(10_000);
        let heavy: Vec<_> = (1..=100).map(|n| message(1, n, &big)).collect();
        let page = events_page(TaskId(1), heavy, 100);
        assert!(encode(page.clone()).len() <= MAX_PLAIN);
        let DesktopMessage::Events { events, .. } = page else { panic!() };
        assert!(!events.is_empty() && events.len() < 100);

        // 一条就超长的截短。
        let huge = "字".repeat(MAX_TEXT);
        let DesktopMessage::Events { events, .. } = events_page(TaskId(1), vec![message(1, 1, &huge)], 1) else {
            panic!()
        };
        let EventKind::Message { text, .. } = &events[0].kind else { panic!() };
        assert!(text.len() <= MAX_TEXT + '…'.len_utf8() && text.ends_with('…'));
    }

    #[test]
    fn runs_each_command_once_and_refuses_late_or_blind_answers() {
        let mut saved = Saved::default();
        let send = Command::SendMessage { task: TaskId(1), text: "go".into() };
        assert_eq!(check(&saved, "c1", 1_000, &send, 2_000), Verdict::Run);
        let ok = CommandResult { ok: true, task: None, error: None };
        remember(&mut saved, "c1", 2_000, &ok);
        assert_eq!(check(&saved, "c1", 1_000, &send, 3_000), Verdict::Done(ok));

        let answer = Command::Answer { task: TaskId(1), option: 0, prompt: Some(4) };
        let late = duolo_core::wire::LIVE_COMMAND_TTL_MS + 10;
        let Verdict::Refuse(result) = check(&saved, "c2", 0, &answer, late) else { panic!() };
        assert_eq!(result.error.as_deref(), Some("commandExpired"));
        let blind = Command::Answer { task: TaskId(1), option: 0, prompt: None };
        let Verdict::Refuse(result) = check(&saved, "c3", 0, &blind, 1) else { panic!() };
        assert_eq!(result.error.as_deref(), Some("promptChanged"));
    }
}
