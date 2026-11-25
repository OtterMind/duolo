//! Codex 的 app-server（`codex app-server`，VS Code 插件用的同一套）：JSON-RPC。会话叫 thread，编号和
//! `codex resume` 用的一样；一轮（turn）用 `turn/start` 开始、`turn/completed` 通知结束；
//! 运行命令、改文件前的授权是它发来的请求。只认用得到的字段，不认识的通知忽略。

use duolo_core::t;
use serde_json::{json, Value};

use crate::conversation::Conversation;
use crate::rpc::{wait, Failure, Incoming, RpcError};
use crate::{Driver, Entry, Link, Spec, SETUP_TIMEOUT};

pub(crate) struct Codex;

impl Driver for Codex {
    fn incoming(&self, link: &Link, message: Incoming) {
        match message {
            Incoming::Notification { method, params } => {
                // 子 Agent 的会话不算在对话里。
                if let (Some(thread), Some(ours)) = (params["threadId"].as_str(), link.session()) {
                    if thread != ours {
                        return;
                    }
                }
                match method.as_str() {
                    "turn/started" => link.set_turn(params["turn"]["id"].as_str().map(str::to_string)),
                    "turn/completed" => {
                        let turn = &params["turn"];
                        let result = match turn["status"].as_str() {
                            Some("failed") => Err(turn["error"]["message"].as_str().unwrap_or("failed").to_string()),
                            _ => Ok(()),
                        };
                        link.end_turn(result);
                    }
                    _ => link.conversation(|c| notified(c, &method, &params)),
                }
            }
            Incoming::Request { id, method, params } => {
                let what = match method.as_str() {
                    "item/commandExecution/requestApproval" => link.conversation(|c| {
                        let item = params["itemId"].as_str().unwrap_or("approval");
                        c.tool(item, |tool| {
                            tool.kind = "execute".into();
                            tool.input = json!({"command": command_of(&params)});
                        });
                        c.describe_tool(item).unwrap_or_default()
                    }),
                    "item/fileChange/requestApproval" => link.conversation(|c| {
                        let item = params["itemId"].as_str().unwrap_or_default();
                        c.describe_tool(item).unwrap_or_else(|| ("Edit".into(), String::new()))
                    }),
                    _ => return link.rpc.respond_error(id, -32601, "method not supported"),
                };
                let options = vec![
                    (t("chat.allow"), json!({"decision": "accept"})),
                    (t("chat.allowSession"), json!({"decision": "acceptForSession"})),
                    (t("chat.deny"), json!({"decision": "decline"})),
                ];
                link.ask(id, what, options, json!({"decision": "cancel"}));
            }
        }
    }

    fn open(&self, link: &Link, spec: &Spec) -> Result<(), Failure> {
        let init = json!({"clientInfo": {"name": "duolo", "title": "Duolo", "version": env!("CARGO_PKG_VERSION")}});
        wait(&link.rpc.request("initialize", init), Some(SETUP_TIMEOUT))?;
        link.rpc.notify("initialized", Value::Null);
        let cwd = spec.cwd.display().to_string();
        let earlier = match &spec.session {
            Some(id) => Some(id.clone()),
            None if spec.resume_latest => {
                let list = json!({"cwd": cwd, "limit": 1, "sortKey": "updated_at"});
                let found = wait(&link.rpc.request("thread/list", list), Some(SETUP_TIMEOUT)).ok();
                found.and_then(|r| r["data"][0]["id"].as_str().map(str::to_string))
            }
            None => None,
        };
        if let Some(id) = earlier {
            let resume = json!({"threadId": id, "cwd": cwd});
            if let Ok(resumed) = wait(&link.rpc.request("thread/resume", resume), Some(SETUP_TIMEOUT)) {
                link.history(history(&resumed["thread"]));
                link.set_session(resumed["thread"]["id"].as_str().unwrap_or(&id));
                return Ok(());
            }
        }
        let started = wait(&link.rpc.request("thread/start", json!({"cwd": cwd})), Some(SETUP_TIMEOUT))?;
        let id = started["thread"]["id"]
            .as_str()
            .ok_or_else(|| Failure::Rejected(RpcError { code: 0, message: "no thread id".into() }))?;
        link.set_session(id);
        link.first_prompt(spec);
        Ok(())
    }

    fn prompt(&self, link: &Link, text: &str) -> Result<(), String> {
        let input = json!([{"type": "text", "text": text, "text_elements": []}]);
        let start = json!({"threadId": link.session(), "input": input});
        match wait(&link.rpc.request("turn/start", start), Some(SETUP_TIMEOUT)) {
            Ok(started) => link.set_turn(started["turn"]["id"].as_str().map(str::to_string)),
            Err(Failure::Rejected(err)) => return Err(err.message),
            Err(_) => return Ok(()),
        }
        link.wait_turn()
    }

    fn interrupt(&self, link: &Link) {
        if let (Some(thread), Some(turn)) = (link.session(), link.turn()) {
            drop(link.rpc.request("turn/interrupt", json!({"threadId": thread, "turnId": turn})));
        }
    }
}

/// 一条通知：一段话写了一点、在想、一项（话、命令、改文件…）开始或结束。
fn notified(c: &mut Conversation, method: &str, params: &Value) {
    match method {
        "item/agentMessage/delta" => {
            c.text(false, params["itemId"].as_str(), params["delta"].as_str().unwrap_or_default());
        }
        "item/reasoning/summaryTextDelta" | "item/reasoning/textDelta" => c.think(),
        "item/started" => item(c, &params["item"], false),
        "item/completed" => item(c, &params["item"], true),
        _ => {}
    }
}

/// 一项开始（`done` 为假）或结束。
fn item(c: &mut Conversation, item: &Value, done: bool) {
    let id = item["id"].as_str().unwrap_or_default();
    let status = || match item["status"].as_str() {
        Some("failed" | "declined") => "failed",
        _ => "completed",
    };
    match item["type"].as_str().unwrap_or_default() {
        "agentMessage" if done => c.said(Some(id), item["text"].as_str().unwrap_or_default()),
        "reasoning" if !done => c.think(),
        "commandExecution" => c.tool(id, |tool| {
            tool.kind = "execute".into();
            tool.input = json!({"command": command_of(item)});
            if done {
                tool.status = status().into();
            }
        }),
        "fileChange" => c.tool(id, |tool| {
            tool.kind = "edit".into();
            tool.location = item["changes"][0]["path"].as_str().map(str::to_string);
            if done {
                tool.status = status().into();
            }
        }),
        "webSearch" => c.tool(id, |tool| {
            tool.kind = "search".into();
            tool.input = json!({"query": item["query"]});
            if done {
                tool.status = "completed".into();
            }
        }),
        "mcpToolCall" | "dynamicToolCall" => c.tool(id, |tool| {
            tool.title = item["tool"].as_str().unwrap_or("tool").to_string();
            tool.input = item["arguments"].clone();
            if done {
                tool.status = status().into();
            }
        }),
        _ => {}
    }
}

/// 要运行的命令：它自己拆出来的那条；拆不出来时去掉外面包的 `zsh -lc "…"`。
fn command_of(v: &Value) -> String {
    if let [action] = v["commandActions"].as_array().map(Vec::as_slice).unwrap_or_default() {
        if let Some(command) = action["command"].as_str().filter(|c| !c.is_empty()) {
            return command.to_string();
        }
    }
    let command = v["command"].as_str().unwrap_or_default();
    let inner = [" -lc ", " -c "].iter().find_map(|flag| {
        let (shell, rest) = command.split_once(flag)?;
        shell.ends_with("sh").then_some(rest)
    });
    match inner {
        Some(rest) => rest.trim().trim_matches(|c| c == '"' || c == '\'').to_string(),
        None => command.to_string(),
    }
}

/// 接回的会话里以前说过的话。工具调用不算：在终端里记下的名字和这里的不一样，没法去重。
fn history(thread: &Value) -> Vec<Entry> {
    let items = thread["turns"].as_array().into_iter().flatten().flat_map(|turn| turn["items"].as_array()).flatten();
    items
        .filter_map(|item| {
            let entry = match item["type"].as_str()? {
                "userMessage" => {
                    let parts = item["content"].as_array()?.iter().filter_map(|p| p["text"].as_str());
                    Entry::User(parts.collect::<Vec<_>>().join("\n").trim().to_string())
                }
                "agentMessage" => Entry::Agent(item["text"].as_str()?.trim().to_string()),
                _ => return None,
            };
            match &entry {
                Entry::User(s) | Entry::Agent(s) if s.is_empty() => None,
                _ => Some(entry),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    const CODEX: &str = include_str!("../tests/fixtures/codex-0.169.1.txt");

    fn messages() -> Vec<Value> {
        CODEX.lines().filter_map(|l| l.strip_prefix("<< ")).map(|l| serde_json::from_str(l).unwrap()).collect()
    }

    #[test]
    fn real_session_becomes_a_conversation() {
        let mut c = Conversation::new(Path::new("/work/demo"));
        let mut drafts = Vec::new();
        for message in messages().iter().take_while(|m| m["method"] != "turn/completed") {
            if let Some(method) = message["method"].as_str() {
                notified(&mut c, method, &message["params"]);
            }
            drafts.push(c.draft());
        }
        c.finish_turn();
        let entries = c.take();
        assert!(matches!(&entries[0], Entry::Agent(text) if text.contains("hello.txt")), "{entries:#?}");
        assert_eq!(
            entries[1..3],
            [
                Entry::Tool { tool: "Bash".into(), target: "printf %s hi > hello.txt".into() },
                Entry::Tool { tool: "Bash".into(), target: "ls".into() },
            ]
        );
        assert!(matches!(&entries[3..], [Entry::Agent(text)] if text.contains("README.md")), "{entries:#?}");
        assert!(drafts.iter().any(|d| d.tool.as_ref().is_some_and(|(t, _)| t == "Bash")), "在跑命令");
    }

    #[test]
    fn resumed_thread_brings_its_history() {
        let resumed = messages().into_iter().find(|m| m["result"]["thread"]["turns"][0].is_object()).unwrap();
        let history = history(&resumed["result"]["thread"]);
        assert!(matches!(&history[0], Entry::User(text) if text.starts_with("Create a file hello.txt")), "{history:?}");
        assert!(matches!(history.last(), Some(Entry::Agent(text)) if text.contains("README.md")), "{history:?}");
    }

    #[test]
    fn commands_lose_their_shell_wrapper() {
        assert_eq!(command_of(&json!({"command": "/bin/zsh -lc \"cargo test\""})), "cargo test");
        assert_eq!(command_of(&json!({"command": "ls -la"})), "ls -la");
        let parsed = json!({"command": "/bin/zsh -lc ls", "commandActions": [{"type": "listFiles", "command": "ls"}]});
        assert_eq!(command_of(&parsed), "ls");
    }
}
