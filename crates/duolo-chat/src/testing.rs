//! 测试用的假 Agent：照着录下来的真实收发（`tests/fixtures/*.txt`，用 [`crate::RECORD_ENV`] 录）回答。
//!
//! 录的格式：每行 `>> ` 开头是 Duolo 发的，`<< ` 开头是 Agent 发的。收到 Duolo 的一条消息时，把录下来的、
//! 同一种消息之后到 Duolo 发下一条之前 Agent 发的行原样发出去（回复的编号换成这次请求的）。
//! 中间有 Agent 发的请求（问是否允许）时，等 Duolo 回答了再往下，后面行里的 `{answer}` 换成选的选项。
//! 两种写法都认：JSON-RPC（ACP、Codex）和 Claude 的 stream-json（`control_request` / `control_response`）。
//!
//! 测试进程自己当假 Agent：设好环境变量 [`FIXTURE_ENV`]，再运行测试程序里调用 [`serve_if_asked`] 的那个测试。

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, Write};

use serde_json::{json, Value};

/// 录下来的收发文件的路径。
pub const FIXTURE_ENV: &str = "DUOLO_FAKE_ACP";

/// 设了 [`FIXTURE_ENV`] 时照着它回答，直到标准输入关闭；没设时什么也不做。
pub fn serve_if_asked() {
    let Some(path) = std::env::var_os(FIXTURE_ENV) else { return };
    let text = std::fs::read_to_string(path).expect("fixture");
    // 测试框架先打印了 `test 名字 ... ` 没换行，先换一行，别让第一条消息接在后面。
    println!();
    serve(&text, std::io::stdin().lock(), &mut std::io::stdout());
}

/// Duolo 发的一条消息是哪一种：JSON-RPC 的方法，control 请求的 `subtype`，或者别的消息的 `type`。
/// 回答 Agent 的问题的那种返回 None。
fn kind(message: &Value) -> Option<String> {
    if message["type"] == "control_request" {
        return message["request"]["subtype"].as_str().map(str::to_string);
    }
    if is_answer(message) {
        return None;
    }
    message["method"].as_str().or(message["type"].as_str()).map(str::to_string)
}

fn is_answer(message: &Value) -> bool {
    message["type"] == "control_response" || (message.get("method").is_none() && message.get("id").is_some())
}

/// 请求的编号（通知、普通消息没有）。
fn request_id(message: &Value) -> Option<&Value> {
    if message["type"] == "control_request" {
        return message.get("request_id");
    }
    message.get("method").and(message.get("id"))
}

/// 对 `id` 这个请求的回复。
fn replies_to(message: &Value, id: &Value) -> bool {
    if message["type"] == "control_response" {
        return &message["response"]["request_id"] == id;
    }
    message.get("method").is_none() && message.get("id") == Some(id)
}

/// 回复改成回给 `id`。
fn reply_to(message: &mut Value, id: &Value) {
    if message["type"] == "control_response" {
        message["response"]["request_id"] = id.clone();
    } else {
        message["id"] = id.clone();
    }
}

/// 一段录下来的回答：当时请求的编号，和 Agent 发的行。
type Script = (Option<Value>, Vec<Value>);

/// 每种消息录下来的回答，按收到的先后用。
fn scripts(fixture: &str) -> HashMap<String, VecDeque<Script>> {
    let mut scripts: HashMap<String, VecDeque<Script>> = HashMap::new();
    let mut current: Option<(String, Script)> = None;
    let mut close = |current: &mut Option<(String, Script)>| {
        if let Some((kind, script)) = current.take() {
            scripts.entry(kind).or_default().push_back(script);
        }
    };
    for line in fixture.lines() {
        if let Some(sent) = line.strip_prefix(">> ") {
            let message: Value = serde_json::from_str(sent).expect("fixture line");
            if let Some(kind) = kind(&message) {
                close(&mut current);
                current = Some((kind, (request_id(&message).cloned(), Vec::new())));
            }
        } else if let Some(said) = line.strip_prefix("<< ") {
            if let Ok(message) = serde_json::from_str::<Value>(said) {
                if let Some((_, (_, lines))) = &mut current {
                    lines.push(message);
                }
            }
        }
    }
    close(&mut current);
    scripts
}

fn serve(fixture: &str, input: impl BufRead, output: &mut impl Write) {
    let mut scripts = scripts(fixture);
    let mut lines = input.lines().map_while(Result::ok);
    let mut send = |message: &Value| {
        let _ = writeln!(output, "{message}");
        let _ = output.flush();
    };
    while let Some(line) = lines.next() {
        let Ok(received) = serde_json::from_str::<Value>(&line) else { continue };
        let Some(kind) = kind(&received) else { continue };
        let id = request_id(&received).cloned();
        let Some((recorded, script)) = scripts.get_mut(&kind).and_then(VecDeque::pop_front) else {
            if let Some(id) = id {
                send(&not_recorded(&received, id));
            }
            continue;
        };
        let mut answer = String::new();
        for mut message in script {
            if let (Some(recorded), Some(id)) = (&recorded, &id) {
                if replies_to(&message, recorded) {
                    reply_to(&mut message, id);
                }
            }
            let text = message.to_string().replace("{answer}", &answer);
            let message: Value = serde_json::from_str(&text).expect("still JSON");
            send(&message);
            // Agent 问了问题：等回答。
            if let Some(asked) = request_id(&message) {
                for line in lines.by_ref() {
                    let Ok(reply) = serde_json::from_str::<Value>(&line) else { continue };
                    if replies_to(&reply, asked) {
                        answer = chosen(&reply);
                        break;
                    }
                }
            }
        }
    }
}

/// 没录下来的请求：像 Agent 拒绝一样回错。
fn not_recorded(request: &Value, id: Value) -> Value {
    if request["type"] == "control_request" {
        json!({"type": "control_response", "response": {"subtype": "error", "request_id": id, "error": "not recorded"}})
    } else {
        json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "not recorded"}})
    }
}

/// 回答里选的是什么：ACP 的选项编号、Codex 的 decision、Claude 的 behavior。
fn chosen(reply: &Value) -> String {
    let outcome = &reply["result"]["outcome"];
    [
        &outcome["optionId"],
        &outcome["outcome"],
        &reply["result"]["decision"],
        &reply["response"]["response"]["behavior"],
    ]
    .into_iter()
    .find_map(Value::as_str)
    .unwrap_or_default()
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(fixture: &str, input: &[&str]) -> Vec<Value> {
        let mut out = Vec::new();
        serve(fixture, input.join("\n").as_bytes(), &mut out);
        String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
    }

    #[test]
    fn replays_by_method_and_waits_for_answers() {
        let fixture = r#">> {"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}
<< {"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1}}
>> {"jsonrpc":"2.0","id":2,"method":"session/prompt","params":{}}
<< {"jsonrpc":"2.0","id":"p","method":"session/request_permission","params":{}}
>> {"jsonrpc":"2.0","id":"p","result":{"outcome":{"outcome":"selected","optionId":"once"}}}
<< {"jsonrpc":"2.0","method":"session/update","params":{"text":"picked {answer}"}}
<< {"jsonrpc":"2.0","id":2,"result":{"stopReason":"end_turn"}}
"#;
        let out = run(
            fixture,
            &[
                r#"{"jsonrpc":"2.0","id":7,"method":"initialize","params":{}}"#,
                r#"{"jsonrpc":"2.0","id":8,"method":"session/prompt","params":{}}"#,
                r#"{"jsonrpc":"2.0","id":"p","result":{"outcome":{"outcome":"selected","optionId":"always"}}}"#,
            ],
        );
        assert_eq!(out[0]["id"], 7, "回复的编号换成这次请求的");
        assert_eq!(out[1]["method"], "session/request_permission");
        assert_eq!(out[2]["params"]["text"], "picked always");
        assert_eq!(out[3]["id"], 8);
    }

    #[test]
    fn replays_control_messages() {
        let fixture = r#">> {"type":"control_request","request_id":"duolo-1","request":{"subtype":"initialize"}}
<< {"type":"control_response","response":{"subtype":"success","request_id":"duolo-1","response":{}}}
>> {"type":"user","message":{"content":"hi"}}
<< {"type":"control_request","request_id":"q","request":{"subtype":"can_use_tool"}}
>> {"type":"control_response","response":{"subtype":"success","request_id":"q","response":{"behavior":"allow"}}}
<< {"type":"result","result":"{answer}"}
"#;
        let out = run(
            fixture,
            &[
                r#"{"type":"control_request","request_id":"duolo-5","request":{"subtype":"initialize"}}"#,
                r#"{"type":"user","message":{"content":"hello"}}"#,
                r#"{"type":"control_response","response":{"subtype":"success","request_id":"q","response":{"behavior":"deny"}}}"#,
            ],
        );
        assert_eq!(out[0]["response"]["request_id"], "duolo-5");
        assert_eq!(out[1]["request"]["subtype"], "can_use_tool");
        assert_eq!(out[2]["result"], "deny");
    }
}
