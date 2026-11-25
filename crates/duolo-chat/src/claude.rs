//! Claude Code 的 stream-json（Agent SDK 用的同一套）：启动参数是
//! `-p --input-format stream-json --output-format stream-json --verbose --include-partial-messages
//! --permission-prompt-tool stdio`。你说的话是一条 `user` 消息，一轮以 `result` 结束；
//! 请求授权是 `can_use_tool` 这个 control 请求。接回会话靠启动参数（`--resume`），以前的对话不重放：
//! 在终端里时已经从会话记录里记下了。只认用得到的字段，不认识的消息忽略。

use std::sync::atomic::{AtomicBool, Ordering};

use duolo_core::t;
use serde_json::{json, Value};

use crate::conversation::Conversation;
use crate::rpc::{wait, Failure, Incoming};
use crate::{Driver, Link, Spec, SETUP_TIMEOUT};

#[derive(Default)]
pub(crate) struct Claude {
    /// 用户打断了这一轮：结束时报的错不算错。
    interrupted: AtomicBool,
}

impl Driver for Claude {
    fn incoming(&self, link: &Link, message: Incoming) {
        match message {
            Incoming::Notification { method, params } => {
                if let Some(id) = params["session_id"].as_str() {
                    link.set_session(id);
                }
                // 子任务（Task 工具）里的消息不算在对话里。
                if !params["parent_tool_use_id"].is_null() {
                    return;
                }
                match method.as_str() {
                    "result" => link.end_turn(self.outcome(&params)),
                    _ => link.conversation(|c| said(c, &method, &params)),
                }
            }
            Incoming::Request { id, method, params } if method == "can_use_tool" => {
                let what = link.conversation(|c| permission(c, &params));
                let input = params["input"].clone();
                let allow = json!({"behavior": "allow", "updatedInput": input});
                let mut options = vec![(t("chat.allow"), allow.clone())];
                if params["permission_suggestions"].as_array().is_some_and(|s| !s.is_empty()) {
                    let mut always = allow;
                    always["updatedPermissions"] = params["permission_suggestions"].clone();
                    options.push((t("chat.allowAlways"), always));
                }
                let deny =
                    json!({"behavior": "deny", "message": "The user doesn't want to proceed with this tool use."});
                options.push((t("chat.deny"), deny.clone()));
                let mut cancel = deny;
                cancel["interrupt"] = json!(true);
                link.ask(id, what, options, cancel);
            }
            Incoming::Request { id, .. } => link.rpc.respond_error(id, 0, "not supported"),
        }
    }

    fn open(&self, link: &Link, spec: &Spec) -> Result<(), Failure> {
        wait(&link.rpc.request("initialize", json!({})), Some(SETUP_TIMEOUT))?;
        match &spec.session {
            // 接回的会话：启动参数里已经带上了。
            Some(id) => link.set_session(id),
            // 接着最近的一次：编号等它说了才知道。
            None if spec.resume_latest => {}
            None => link.first_prompt(spec),
        }
        Ok(())
    }

    fn prompt(&self, link: &Link, text: &str) -> Result<(), String> {
        self.interrupted.store(false, Ordering::Relaxed);
        link.rpc.send(json!({
            "type": "user",
            "message": {"role": "user", "content": [{"type": "text", "text": text}]},
            "parent_tool_use_id": null,
            "session_id": link.session().unwrap_or_default(),
        }));
        link.wait_turn()
    }

    fn interrupt(&self, link: &Link) {
        self.interrupted.store(true, Ordering::Relaxed);
        // 不等回复：这一轮结束时会有 `result`。
        drop(link.rpc.request("interrupt", json!({})));
    }
}

impl Claude {
    /// `result`：这一轮成功了没有。
    fn outcome(&self, result: &Value) -> Result<(), String> {
        if result["is_error"].as_bool() != Some(true) || self.interrupted.load(Ordering::Relaxed) {
            return Ok(());
        }
        let errors: Vec<&str> = result["errors"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        let reason = result["result"].as_str().map(str::to_string).filter(|r| !r.is_empty());
        Err(reason
            .or_else(|| (!errors.is_empty()).then(|| errors.join("\n")))
            .unwrap_or_else(|| result["subtype"].as_str().unwrap_or("error").to_string()))
    }
}

/// 一条消息：流式的片段（`stream_event`）、写完的消息（`assistant`）、工具的结果（`user`）。
fn said(c: &mut Conversation, kind: &str, message: &Value) {
    match kind {
        "stream_event" => {
            let event = &message["event"];
            let id = message["api_message_id"].as_str();
            match event["type"].as_str() {
                Some("content_block_start") => {
                    let block = &event["content_block"];
                    match block["type"].as_str() {
                        Some("thinking" | "redacted_thinking") => c.think(),
                        Some("tool_use") => tool_use(c, block),
                        _ => {}
                    }
                }
                Some("content_block_delta") if event["delta"]["type"] == "text_delta" => {
                    c.text(false, id, event["delta"]["text"].as_str().unwrap_or_default());
                }
                _ => {}
            }
        }
        "assistant" => {
            let id = message["message"]["id"].as_str();
            for block in message["message"]["content"].as_array().into_iter().flatten() {
                match block["type"].as_str() {
                    Some("text") => c.said(id, block["text"].as_str().unwrap_or_default()),
                    Some("tool_use") => tool_use(c, block),
                    _ => {}
                }
            }
        }
        "user" => {
            for block in message["message"]["content"].as_array().into_iter().flatten() {
                if let (Some("tool_result"), Some(id)) = (block["type"].as_str(), block["tool_use_id"].as_str()) {
                    let failed = block["is_error"].as_bool() == Some(true);
                    c.tool(id, |tool| tool.status = if failed { "failed" } else { "completed" }.into());
                }
            }
        }
        _ => {}
    }
}

/// 开始用一个工具：名字就是会话记录里的名字（Read、Edit、Bash…）。参数写完时才有。
fn tool_use(c: &mut Conversation, block: &Value) {
    let Some(id) = block["id"].as_str() else { return };
    c.tool(id, |tool| {
        if let Some(name) = block["name"].as_str() {
            tool.title = name.to_string();
        }
        if block["input"].as_object().is_some_and(|input| !input.is_empty()) {
            tool.input = block["input"].clone();
        }
    });
}

/// 请求授权的那个工具：用什么、对什么。
fn permission(c: &mut Conversation, request: &Value) -> (String, String) {
    let id = request["tool_use_id"].as_str().unwrap_or("permission");
    tool_use(c, &json!({"id": id, "name": request["tool_name"], "input": request["input"]}));
    c.describe_tool(id).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::Entry;

    const CLAUDE: &str = include_str!("../tests/fixtures/claude-2.1.311.txt");

    #[test]
    fn real_session_becomes_a_conversation() {
        let mut c = Conversation::new(Path::new("/work/demo"));
        let mut drafts = Vec::new();
        for line in CLAUDE.lines().filter_map(|l| l.strip_prefix("<< ")) {
            let message: Value = serde_json::from_str(line).unwrap();
            if message["type"] == "control_request" {
                assert_eq!(permission(&mut c, &message["request"]), ("Write".into(), "hello.txt".into()));
            } else if message["parent_tool_use_id"].is_null() {
                said(&mut c, message["type"].as_str().unwrap(), &message);
            }
            drafts.push(c.draft());
        }
        c.finish_turn();
        let entries = c.take();
        assert_eq!(
            entries[..2],
            [
                Entry::Tool { tool: "Write".into(), target: "hello.txt".into() },
                Entry::Tool { tool: "Bash".into(), target: "ls".into() },
            ]
        );
        assert!(
            matches!(&entries[2..], [Entry::Agent(text)] if text.contains("README.md")),
            "流式的片段和写完的消息只记一次：{entries:?}"
        );
        assert!(drafts.iter().any(|d| d.tool == Some(("Bash".into(), "ls".into()))), "在跑命令");
        assert!(drafts.iter().any(|d| !d.text.is_empty() && !d.text.contains("README.md")), "话是一点点写出来的");
    }

    #[test]
    fn errors_say_why_unless_interrupted() {
        let claude = Claude::default();
        let failed = json!({"type": "result", "subtype": "error_during_execution", "is_error": true, "errors": ["API Error: 529"]});
        assert_eq!(claude.outcome(&failed), Err("API Error: 529".into()));
        assert_eq!(claude.outcome(&json!({"is_error": false, "result": "fine"})), Ok(()));
        claude.interrupted.store(true, Ordering::Relaxed);
        assert_eq!(claude.outcome(&failed), Ok(()), "用户打断的不算错");
    }
}
