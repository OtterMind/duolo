//! 任务的事件流和远程命令：电脑端界面、以后的手机端和云端同步都用这套格式。
//!
//! 事件按任务编号，从 1 开始递增（`seq`），客户端断线重连时只需要说"我有到第几条"，补发之后的即可。
//! 事件里只放数据（文件路径、命令、状态码），不放给人看的句子：客户端按自己的语言显示。

use serde::{Deserialize, Serialize};

use crate::model::TaskId;

/// 任务里发生的一件事。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskEvent {
    pub task: TaskId,
    /// 任务内从 1 开始递增。
    pub seq: u64,
    /// Unix 毫秒。
    pub at: u64,
    #[serde(flatten)]
    pub kind: EventKind,
}

/// 说话的一方。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Speaker {
    User,
    Agent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventKind {
    /// 任务建好了。
    TaskCreated {
        title: String,
        branch: String,
    },
    /// 任务有了标题或换了分支名。
    TaskRenamed {
        title: String,
        branch: String,
    },
    TaskClosed,
    /// 一条对话：你说的话，或者 Agent 的回复。
    Message {
        speaker: Speaker,
        agent: u32,
        text: String,
    },
    /// Agent 调用了一个工具。`tool` 是工具名（Read、Edit、Bash…），`target` 是文件路径或命令。
    Tool {
        agent: u32,
        tool: String,
        target: String,
    },
    /// Agent 的状态变了：`not_started`、`running`、`needs_input`、`idle`、`exited`、`failed`。
    Status {
        agent: u32,
        status: String,
    },
    /// Agent 在等你选：问题、说明（例如要运行的命令）和选项（例如 允许 / 拒绝）。
    Prompt {
        agent: u32,
        question: String,
        detail: String,
        options: Vec<String>,
    },
    /// 后台审阅开始。
    ReviewStarted {
        reviewer: String,
    },
    /// 后台审阅结束：`verdict` 是 `pass`、`issues` 或 `unknown`，审阅失败时 `ok` 为假。
    ReviewFinished {
        ok: bool,
        verdict: String,
        issues: u32,
        summary: String,
    },
    /// 改动统计变了。
    Diff {
        files: u32,
        insertions: u32,
        deletions: u32,
    },
}

/// 从别的设备（手机）发给电脑的命令。电脑执行后回一个 [`CommandResult`]。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    /// 新建任务。没给的选项按电脑上的默认值。
    CreateTask {
        project: String,
        prompt: String,
        #[serde(default)]
        repo: Option<String>,
        #[serde(default)]
        base: Option<String>,
        #[serde(default)]
        branch: Option<String>,
        #[serde(default)]
        writer: Option<String>,
        #[serde(default)]
        reviewer: Option<String>,
    },
    /// 给任务的 Agent 发一句话（Agent 没在运行时先接着上次的会话启动）。
    SendMessage {
        task: TaskId,
        text: String,
    },
    /// 回答 Agent 正在等的选择，`option` 从 0 开始。
    Answer {
        task: TaskId,
        option: usize,
    },
    /// 打断 Agent 正在做的事（相当于在终端里按 Esc）。
    Interrupt {
        task: TaskId,
    },
    StartReview {
        task: TaskId,
    },
    CloseTask {
        task: TaskId,
    },
}

/// 命令的结果。失败时 `error` 是错误码（和桌面界面的 `error.*` 文案对应）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandResult {
    pub ok: bool,
    /// 新建任务时是新任务的编号。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<TaskId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_format() {
        let event = TaskEvent {
            task: TaskId(3),
            seq: 7,
            at: 1,
            kind: EventKind::Tool { agent: 0, tool: "Edit".into(), target: "src/guard.ts".into() },
        };
        let json = serde_json::to_string(&event).unwrap();
        assert_eq!(json, r#"{"task":3,"seq":7,"at":1,"type":"tool","agent":0,"tool":"Edit","target":"src/guard.ts"}"#);
        assert_eq!(serde_json::from_str::<TaskEvent>(&json).unwrap(), event);

        let cmd: Command = serde_json::from_str(r#"{"type":"answer","task":3,"option":0}"#).unwrap();
        assert_eq!(cmd, Command::Answer { task: TaskId(3), option: 0 });
        let cmd: Command = serde_json::from_str(r#"{"type":"create_task","project":"shop","prompt":"x"}"#).unwrap();
        assert!(matches!(cmd, Command::CreateTask { repo: None, .. }));
    }
}
