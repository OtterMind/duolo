//! ACP（Agent Client Protocol，<https://agentclientprotocol.com>）：JSON-RPC，OpenCode、Grok、Antigravity 等都支持。
//! 只认用得到的字段，不认识的更新忽略：Agent 升级加了新的更新类型也不会出错。

use std::path::Path;

use serde_json::{json, Value};

use crate::conversation::{tool_name, Conversation};
use crate::rpc::{wait, Failure, Incoming, RpcError};
use crate::{Driver, Link, Spec, SETUP_TIMEOUT};

pub(crate) struct Acp;

impl Driver for Acp {
    fn incoming(&self, link: &Link, message: Incoming) {
        match message {
            Incoming::Notification { method, params } if method == "session/update" => {
                link.conversation(|c| update(c, &params["update"]));
            }
            Incoming::Request { id, method, params } if method == "session/request_permission" => {
                let what = link.conversation(|c| permission(c, &params["toolCall"]));
                let options = params["options"].as_array().cloned().unwrap_or_default();
                let options = options
                    .iter()
                    .map(|o| {
                        let name = o["name"].as_str().unwrap_or_default().to_string();
                        (name, json!({"outcome": {"outcome": "selected", "optionId": o["optionId"]}}))
                    })
                    .collect();
                link.ask(id, what, options, json!({"outcome": {"outcome": "cancelled"}}));
            }
            // 没说过支持的（读写文件、开终端）：告诉它不支持，它会自己做。
            Incoming::Request { id, .. } => link.rpc.respond_error(id, -32601, "method not supported"),
            Incoming::Notification { .. } => {}
        }
    }

    fn open(&self, link: &Link, spec: &Spec) -> Result<(), Failure> {
        let init = json!({
            "protocolVersion": 1,
            "clientCapabilities": {"fs": {"readTextFile": false, "writeTextFile": false}, "terminal": false},
            "clientInfo": {"name": "duolo", "version": env!("CARGO_PKG_VERSION")},
        });
        let caps = wait(&link.rpc.request("initialize", init), Some(SETUP_TIMEOUT))?["agentCapabilities"].clone();
        let cwd = spec.cwd.display().to_string();
        let earlier = match &spec.session {
            Some(id) => Some(id.clone()),
            None if spec.resume_latest && caps["sessionCapabilities"].get("list").is_some() => {
                latest_session(link, &spec.cwd)
            }
            None => None,
        };
        if let Some(id) = earlier.filter(|_| caps["loadSession"].as_bool() == Some(true)) {
            link.begin_history();
            let load = json!({"sessionId": id, "cwd": cwd, "mcpServers": []});
            let loaded = wait(&link.rpc.request("session/load", load), Some(SETUP_TIMEOUT)).is_ok();
            link.end_history(loaded);
            if loaded {
                link.set_session(&id);
                return Ok(());
            }
        }
        let created =
            wait(&link.rpc.request("session/new", json!({"cwd": cwd, "mcpServers": []})), Some(SETUP_TIMEOUT))?;
        let id = created["sessionId"]
            .as_str()
            .ok_or_else(|| Failure::Rejected(RpcError { code: 0, message: "no sessionId".into() }))?;
        link.set_session(id);
        link.first_prompt(spec);
        Ok(())
    }

    fn prompt(&self, link: &Link, text: &str) -> Result<(), String> {
        let prompt = json!({"sessionId": link.session(), "prompt": [{"type": "text", "text": text}]});
        match wait(&link.rpc.request("session/prompt", prompt), None) {
            Err(Failure::Rejected(err)) => Err(err.message),
            _ => Ok(()),
        }
    }

    fn interrupt(&self, link: &Link) {
        if let Some(session) = link.session() {
            link.rpc.notify("session/cancel", json!({"sessionId": session}));
        }
    }
}

/// 这个目录里最近的一次会话。
fn latest_session(link: &Link, cwd: &Path) -> Option<String> {
    let request = link.rpc.request("session/list", json!({"cwd": cwd.display().to_string()}));
    let list = wait(&request, Some(SETUP_TIMEOUT)).ok()?;
    let mut sessions: Vec<&Value> = list["sessions"].as_array()?.iter().collect();
    sessions.sort_by(|a, b| b["updatedAt"].as_str().cmp(&a["updatedAt"].as_str()));
    sessions.first()?["sessionId"].as_str().map(str::to_string)
}

/// `session/update` 的 `update`。你说的话只在重放以前的对话时记（见 `Conversation::replaying`）。
fn update(c: &mut Conversation, update: &Value) {
    match update["sessionUpdate"].as_str().unwrap_or_default() {
        kind @ ("agent_message_chunk" | "user_message_chunk") => {
            let content = &update["content"];
            if content["type"].as_str() == Some("text") {
                let text = content["text"].as_str().unwrap_or_default();
                c.text(kind == "user_message_chunk", update["messageId"].as_str(), text);
            }
        }
        "agent_thought_chunk" => c.think(),
        "tool_call" | "tool_call_update" => tool(c, update),
        _ => {}
    }
}

/// `tool_call` 或 `tool_call_update`：只有出现的字段才换掉原来的。
fn tool(c: &mut Conversation, update: &Value) {
    let Some(id) = update["toolCallId"].as_str() else { return };
    c.tool(id, |tool| {
        let text = |key: &str| update[key].as_str().filter(|s| !s.is_empty()).map(str::to_string);
        if let Some(kind) = text("kind") {
            tool.kind = kind;
        }
        if let Some(title) = text("title") {
            tool.title = title;
        }
        if let Some(status) = text("status") {
            tool.status = status;
        }
        if update["rawInput"].as_object().is_some_and(|input| !input.is_empty()) {
            tool.input = update["rawInput"].clone();
        }
        if let Some(path) = update["locations"].get(0).and_then(|l| l["path"].as_str()) {
            tool.location = Some(path.to_string());
        }
    });
}

/// 请求授权的那个工具：用什么、对什么（命令、文件）。
fn permission(c: &mut Conversation, tool_call: &Value) -> (String, String) {
    tool(c, tool_call);
    let id = tool_call["toolCallId"].as_str().unwrap_or_default();
    c.describe_tool(id)
        .unwrap_or_else(|| (tool_name("", tool_call["title"].as_str().unwrap_or_default()), String::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Entry;

    /// 录下来的真实会话里 Agent 发来的 `session/update`。
    fn updates(fixture: &str) -> Vec<Value> {
        fixture
            .lines()
            .filter_map(|l| l.strip_prefix("<< "))
            .map(|l| serde_json::from_str::<Value>(l).unwrap())
            .filter(|m| m["method"] == "session/update")
            .map(|m| m["params"]["update"].clone())
            .collect()
    }

    const OPENCODE: &str = include_str!("../tests/fixtures/opencode-1.19.6.txt");

    #[test]
    fn real_session_becomes_a_conversation() {
        let all = updates(OPENCODE);
        // 前一半是这一轮，后一半是 session/load 重放的同一段对话。
        let replay_from = all.iter().position(|u| u["sessionUpdate"] == "user_message_chunk").unwrap();
        let mut live = Conversation::new(Path::new("/work/demo"));
        let mut drafts = Vec::new();
        for u in &all[..replay_from] {
            update(&mut live, u);
            drafts.push(live.draft());
        }
        live.finish_turn();
        let turn = live.take();
        assert_eq!(
            turn,
            vec![
                Entry::Tool { tool: "Edit".into(), target: "hello.txt".into() },
                Entry::Tool { tool: "Bash".into(), target: "ls".into() },
                Entry::Agent("`ls` lists `hello.txt` and `README.md`.".into()),
            ]
        );
        assert!(drafts.iter().any(|d| d.thinking), "先在想");
        assert!(drafts.iter().any(|d| d.tool == Some(("Bash".into(), "ls".into()))), "在跑命令");
        assert!(drafts.iter().any(|d| d.text == "`ls` lists `"), "话是一点点写出来的");

        let mut replay = Conversation::new(Path::new("/work/demo"));
        replay.replaying = true;
        for u in &all[replay_from..] {
            update(&mut replay, u);
        }
        replay.finish_turn();
        let history = replay.take();
        assert_eq!(
            history[0],
            Entry::User(
                "Create a file hello.txt containing 'hi', then run `ls` and tell me the result in one sentence.".into()
            )
        );
        assert_eq!(history[1..], turn[..], "重放出来的和当时记下的一样，才能去重");
    }

    #[test]
    fn user_words_are_not_recorded_twice() {
        let mut c = Conversation::default();
        update(&mut c, &json!({"sessionUpdate": "user_message_chunk", "content": {"type": "text", "text": "hi"}}));
        update(&mut c, &json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "Hello"}}));
        update(&mut c, &json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": " there"}}));
        update(&mut c, &json!({"sessionUpdate": "plan", "entries": []}));
        update(&mut c, &json!({"sessionUpdate": "something_new"}));
        c.finish_turn();
        assert_eq!(c.take(), vec![Entry::Agent("Hello there".into())], "发出去的话由发送的一方记");
    }

    #[test]
    fn permission_names_the_command() {
        let mut c = Conversation::new(Path::new("/work/demo"));
        let call = json!({"toolCallId": "t1", "title": "pnpm test", "kind": "execute", "rawInput": {"command": "pnpm test\n--watch"}});
        assert_eq!(permission(&mut c, &call), ("Bash".into(), "pnpm test".into()));
        let unknown = json!({"toolCallId": "t2", "title": "mcp lookup"});
        assert_eq!(permission(&mut c, &unknown), ("mcp".into(), String::new()));
    }
}
