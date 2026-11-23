//! 钩子（为什么这样做、和 Orca 的做法比见 docs/agent-compat.md）：每种 Agent 的写法实现 [`Format`]，插件的声明按名字引用。
//!
//! - 装：每次启动时把一份设置文件交给它（Claude Code 的 `--settings <文件>`），不改用户的配置。
//! - 收：钩子命令是 Duolo 自己的程序（`duolo_core::hook`），事件写成文件，引擎来读。
//! - 版本：旧版遇到不认识的事件会拒绝整个设置文件，所以每个事件都写明最早支持的版本，读不出版本时不装。

use serde_json::{json, Value};

use crate::transcript::tool_target;

/// 从钩子里得到的、Duolo 用得上的事。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookEvent {
    /// 会话开始（新开、恢复、清空后重新开始都算）：它的会话编号，比从会话记录里认的准。
    Session(String),
    /// 请求授权：用什么工具、对什么（文件路径、命令）。
    Approval { tool: String, target: String },
}

pub(crate) trait Format: std::fmt::Debug + Send + Sync {
    /// 交给它的设置文件内容：这些事件发生时运行 `command`。
    fn settings(&self, command: &str, events: &[&str]) -> String;
    /// 一个事件里 Duolo 用得上的；不关心的返回 None。
    fn parse(&self, event: &Value) -> Option<HookEvent>;
}

pub(crate) fn format(name: &str) -> Option<&'static dyn Format> {
    match name {
        "claude" => Some(&Claude),
        _ => None,
    }
}

/// Claude Code：`{"hooks": {"<事件>": [{"hooks": [{"type": "command", "command": …}]}]}}`，
/// 事件从标准输入给，字段名是 `hook_event_name`、`session_id`、`tool_name`、`tool_input`。
#[derive(Debug)]
struct Claude;

impl Format for Claude {
    fn settings(&self, command: &str, events: &[&str]) -> String {
        let hook = json!([{ "hooks": [{ "type": "command", "command": command, "timeout": 10 }] }]);
        let hooks: serde_json::Map<String, Value> = events.iter().map(|e| (e.to_string(), hook.clone())).collect();
        json!({ "hooks": hooks }).to_string()
    }

    fn parse(&self, event: &Value) -> Option<HookEvent> {
        match event["hook_event_name"].as_str()? {
            "SessionStart" => event["session_id"].as_str().map(|id| HookEvent::Session(id.to_string())),
            "PermissionRequest" => Some(HookEvent::Approval {
                tool: event["tool_name"].as_str().unwrap_or("tool").to_string(),
                target: tool_target(&event["tool_input"]),
            }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_settings_and_events() {
        let settings: Value =
            serde_json::from_str(&Claude.settings("\"/a b/duolo\" __hook", &["SessionStart"])).unwrap();
        assert_eq!(settings["hooks"]["SessionStart"][0]["hooks"][0]["command"], "\"/a b/duolo\" __hook");
        assert_eq!(settings["hooks"].as_object().unwrap().len(), 1);

        let start = json!({"hook_event_name": "SessionStart", "session_id": "abc", "source": "clear"});
        assert_eq!(Claude.parse(&start), Some(HookEvent::Session("abc".into())));
        let ask = json!({"hook_event_name": "PermissionRequest", "tool_name": "Bash", "tool_input": {"command": "pnpm test\nmore", "description": "Run tests"}});
        assert_eq!(Claude.parse(&ask), Some(HookEvent::Approval { tool: "Bash".into(), target: "pnpm test".into() }));
        assert_eq!(Claude.parse(&json!({"hook_event_name": "Stop"})), None);
    }
}
