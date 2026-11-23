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
    /// 图标的颜色（插件的 `icon_color`）；没有时用文字的颜色。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
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
    /// 列表上显示的状态，由电脑算好（[`TaskState::of`]），手机不用自己推导。
    #[serde(default)]
    pub state: TaskState,
    /// 改的是项目里的哪个仓库（相对项目文件夹的路径）；项目本身就是仓库时为空。
    #[serde(default)]
    pub repo: String,
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

/// 任务的状态：写代码的 Agent 的状态和审阅结论合在一起。电脑和手机的列表都按它显示，
/// 规则只有这一份；样例在 mobile/contract/behavior.json。文案是 `state.<code>`。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    /// 等你确认。
    NeedsYou,
    /// 审出了必须改的问题。
    Issues,
    ReviewFailed,
    /// Agent 异常退出。
    Crashed,
    Writing,
    Reviewing,
    Passed,
    /// 审完了，但结论里没有判断标记。
    Reviewed,
    /// 写完了，等你下一句。
    Written,
    Ended,
    #[default]
    NotStarted,
}

/// 状态归哪一组：要你处理、进行中、完成。手机按组排序和筛选。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StateGroup {
    Attention,
    Active,
    Done,
}

/// 推导状态要用的审阅结果。时间都是 Unix 毫秒。
#[derive(Debug, Clone, Copy)]
pub struct ReviewFacts<'a> {
    pub ok: bool,
    /// `pass`、`issues` 或 `unknown`。
    pub verdict: &'a str,
    pub finished_at: u64,
}

impl TaskState {
    /// `status` 是写代码的 Agent 的状态码，`since` 是它进入这个状态的时间（不知道时为 0）。
    pub fn of(status: &str, reviewing: bool, review: Option<ReviewFacts<'_>>, since: u64) -> Self {
        match status {
            "needs_input" => return Self::NeedsYou,
            _ if reviewing => return Self::Reviewing,
            "running" => return Self::Writing,
            "failed" => return Self::Crashed,
            _ => {}
        }
        // 审阅之后又接着做了：以前的结论不算数。
        if let Some(review) = review.filter(|r| !(status == "idle" && since > r.finished_at)) {
            return match (review.ok, review.verdict) {
                (false, _) => Self::ReviewFailed,
                (true, "issues") => Self::Issues,
                (true, "pass") => Self::Passed,
                _ => Self::Reviewed,
            };
        }
        match status {
            "idle" => Self::Written,
            "exited" => Self::Ended,
            _ => Self::NotStarted,
        }
    }

    pub fn group(self) -> StateGroup {
        match self {
            Self::NeedsYou | Self::Issues | Self::ReviewFailed | Self::Crashed => StateGroup::Attention,
            Self::Writing | Self::Reviewing => StateGroup::Active,
            Self::Passed | Self::Reviewed | Self::Written | Self::Ended | Self::NotStarted => StateGroup::Done,
        }
    }
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

    /// 列表状态的规则，手机端的测试读同一份样例。
    #[test]
    fn task_states_match_the_contract() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mobile/contract/behavior.json");
        let contract: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        for case in contract["rows"].as_array().unwrap() {
            let task = &case["task"];
            let review = task.get("review").map(|r| ReviewFacts {
                ok: r["ok"].as_bool().unwrap(),
                verdict: r["verdict"].as_str().unwrap(),
                finished_at: r["finishedAt"].as_u64().unwrap(),
            });
            let state = TaskState::of(
                task["status"].as_str().unwrap(),
                task.get("reviewingSince").is_some(),
                review,
                task["since"].as_u64().unwrap(),
            );
            assert_eq!(serde_json::to_value(state).unwrap(), case["state"], "{}", case["name"]);
            assert_eq!(serde_json::to_value(state.group()).unwrap(), case["group"], "{}", case["name"]);
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
