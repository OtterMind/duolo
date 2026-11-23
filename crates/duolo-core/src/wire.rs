//! 电脑和手机之间的消息（端到端加密之前的明文）。约定见 docs/mobile.md。
//!
//! - 字段只增不改；收到不认识的 `type` 时忽略，不报错，所以新旧版本可以互通。
//! - 服务器只转发密文，这里的内容它看不到。
//! - `mobile/contract/*.json` 是三端（Rust、Swift、Kotlin）共用的样例，改格式要同时改样例。

use serde::{Deserialize, Serialize};

use crate::event::{Command, CommandResult, TaskEvent};
use crate::model::TaskId;

/// 消息格式的版本。不兼容的改动才加一。
pub const VERSION: u32 = 1;

/// "回答"和"打断"只对当下有意义：比这更早发出的不执行（毫秒）。留了两边时钟不准的余量。
pub const LIVE_COMMAND_TTL_MS: u64 = 5 * 60 * 1000;

/// 其他命令最多晚到多久（毫秒），和服务器离线信箱的保存时间一致。
/// 电脑记住执行过的命令编号要比这更久，这样同一条命令不会执行两次。
pub const COMMAND_TTL_MS: u64 = 7 * 24 * 60 * 60 * 1000;

/// 一条消息和它的格式版本。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Packet<T> {
    pub v: u32,
    #[serde(flatten)]
    pub message: T,
}

impl<T> Packet<T> {
    pub fn new(message: T) -> Self {
        Self { v: VERSION, message }
    }
}

/// 手机发给电脑的。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum PhoneMessage {
    /// 要一份最新的总览（打开应用、重新连上时）。
    Sync,
    /// 要任务第 `after` 条之后的事件（打开任务、发现中间缺了几条时）。
    Fetch { task: TaskId, after: u64 },
    /// 执行一条命令。`id` 由手机生成，同一个编号只执行一次；`at` 是发出的时间（Unix 毫秒）。
    Command { id: String, at: u64, command: Command },
}

/// 电脑发给手机的。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum DesktopMessage {
    /// 总览，整份替换手机上的。
    Overview(Overview),
    /// 任务的事件，按编号连续。`latest` 是电脑上最后一条的编号，没到就继续要。
    Events { task: TaskId, events: Vec<TaskEvent>, latest: u64 },
    /// 最近一次审阅的全文（Markdown）。
    Review { task: TaskId, finished_at: u64, output: String },
    /// 命令的结果。
    Result { id: String, result: CommandResult },
}

/// 列表页需要的全部内容。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    /// 每次变化加一；比手机上的旧就忽略（消息可能乱序到达）。应用重启后用当前时间接着往上走。
    pub rev: u64,
    #[serde(default)]
    pub projects: Vec<ProjectOverview>,
    #[serde(default)]
    pub tasks: Vec<TaskOverview>,
    /// 能用来写和审的 Agent。
    #[serde(default)]
    pub agents: Vec<AgentOption>,
    /// 默认谁写、谁审（Agent 的种类）。
    #[serde(default)]
    pub writer: String,
    #[serde(default)]
    pub reviewer: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOption {
    pub kind: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectOverview {
    pub id: String,
    pub name: String,
    /// 新任务默认从哪个分支开始。
    #[serde(default)]
    pub branch: String,
    #[serde(default)]
    pub branches: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskOverview {
    pub id: TaskId,
    pub project: String,
    pub title: String,
    /// 还没有需求的空白任务，界面按当前语言显示为"新任务"。
    #[serde(default)]
    pub untitled: bool,
    #[serde(default)]
    pub branch: String,
    /// 写代码的 Agent 的状态码，和 [`crate::EventKind::Status`] 一致。
    pub status: String,
    pub writer: AgentOption,
    pub reviewer: AgentOption,
    /// 最近一次有动静的时间（Unix 毫秒），列表按它排。
    pub updated_at: u64,
    /// 现在这个状态从什么时候开始（Unix 毫秒）。
    #[serde(default)]
    pub since: u64,
    /// 最后一句对话，列表第二行显示。
    #[serde(default)]
    pub last: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<PromptOverview>,
    /// 正在审阅时的开始时间（Unix 毫秒）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewing_since: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<ReviewOverview>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff: Option<DiffOverview>,
    /// 最后一条事件的编号。
    #[serde(default)]
    pub seq: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptOverview {
    /// 这条提示的事件编号，回答时带回来。
    pub seq: u64,
    pub question: String,
    #[serde(default)]
    pub detail: String,
    pub options: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewOverview {
    /// Unix 毫秒。
    pub finished_at: u64,
    pub ok: bool,
    /// `pass`、`issues` 或 `unknown`。
    pub verdict: String,
    #[serde(default)]
    pub issues: u32,
    #[serde(default)]
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffOverview {
    pub files: u32,
    pub insertions: u32,
    pub deletions: u32,
}

impl Command {
    /// 到达时（`now`，Unix 毫秒）是不是已经太晚，不该再执行。
    pub fn is_expired(&self, at: u64, now: u64) -> bool {
        let ttl = match self {
            Self::Answer { .. } | Self::Interrupt { .. } => LIVE_COMMAND_TTL_MS,
            _ => COMMAND_TTL_MS,
        };
        now.saturating_sub(at) > ttl
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    fn contract() -> Value {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mobile/contract/wire.json");
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    /// 三端共用的样例：读进来再写出去，内容不变。
    #[test]
    fn contract_round_trips() {
        let contract = contract();
        for case in contract["phone"].as_array().unwrap() {
            let packet: Packet<PhoneMessage> = serde_json::from_value(case["json"].clone()).unwrap();
            assert_eq!(packet.v, VERSION);
            assert_eq!(serde_json::to_value(&packet).unwrap(), case["json"], "{}", case["name"]);
        }
        for case in contract["desktop"].as_array().unwrap() {
            let packet: Packet<DesktopMessage> = serde_json::from_value(case["json"].clone()).unwrap();
            assert_eq!(serde_json::to_value(&packet).unwrap(), case["json"], "{}", case["name"]);
        }
    }

    #[test]
    fn live_commands_expire_quickly() {
        let answer = Command::Answer { task: TaskId(1), option: 0, prompt: Some(3) };
        assert!(!answer.is_expired(1_000, 1_000 + LIVE_COMMAND_TTL_MS));
        assert!(answer.is_expired(1_000, 1_001 + LIVE_COMMAND_TTL_MS));
        let message = Command::SendMessage { task: TaskId(1), text: "hi".into() };
        assert!(!message.is_expired(1_000, 1_001 + LIVE_COMMAND_TTL_MS));
        assert!(message.is_expired(0, COMMAND_TTL_MS + 1));
        assert!(!message.is_expired(5_000, 1_000), "手机的时钟快一点不算过期");
    }
}
