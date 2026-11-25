//! 测试用的假 Agent：照着录下来的真实收发（`tests/fixtures/*.txt`）回答。
//!
//! 录的格式：每行 `>> ` 开头是 Duolo 发的，`<< ` 开头是 Agent 发的。收到某个方法的请求时，
//! 把录下来的、这个方法的请求之后到它的回复为止 Agent 发的行原样发出去（回复的编号换成这次请求的）。
//! 中间有 Agent 发的请求（问是否允许）时，等 Duolo 回答了再往下，后面行里的 `{answer}` 换成选的选项。
//!
//! 测试进程自己当假 Agent：设好环境变量 [`FIXTURE_ENV`]，再运行测试程序里调用 [`serve_if_asked`] 的那个测试。

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, Write};

use serde_json::Value;

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

/// 每个方法录下来的回答，按收到的先后用。
fn scripts(fixture: &str) -> HashMap<String, VecDeque<Vec<Value>>> {
    let mut scripts: HashMap<String, VecDeque<Vec<Value>>> = HashMap::new();
    let mut open: Vec<(Value, String, Vec<Value>)> = Vec::new();
    for line in fixture.lines() {
        if let Some(sent) = line.strip_prefix(">> ") {
            let message: Value = serde_json::from_str(sent).expect("fixture line");
            if let (Some(method), Some(id)) = (message["method"].as_str(), message.get("id")) {
                open.push((id.clone(), method.to_string(), Vec::new()));
            }
        } else if let Some(said) = line.strip_prefix("<< ") {
            let message: Value = serde_json::from_str(said).expect("fixture line");
            let reply_to =
                open.iter().position(|(id, _, _)| message.get("method").is_none() && message.get("id") == Some(id));
            if let Some((_, _, lines)) = open.last_mut() {
                lines.push(message);
            }
            if let Some(i) = reply_to {
                let (_, method, lines) = open.remove(i);
                scripts.entry(method).or_default().push_back(lines);
            }
        }
    }
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
        let Ok(request) = serde_json::from_str::<Value>(&line) else { continue };
        let (Some(method), Some(id)) = (request["method"].as_str(), request.get("id")) else { continue };
        let Some(script) = scripts.get_mut(method).and_then(VecDeque::pop_front) else {
            send(
                &serde_json::json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "not recorded"}}),
            );
            continue;
        };
        let mut answer = String::new();
        for mut message in script {
            if message.get("method").is_none() && message.get("id").is_some() {
                message["id"] = id.clone();
            }
            let text = message.to_string().replace("{answer}", &answer);
            let message: Value = serde_json::from_str(&text).expect("still JSON");
            send(&message);
            // Agent 问了问题：等回答。
            if let (Some(_), Some(asked)) = (message.get("method"), message.get("id")) {
                for line in lines.by_ref() {
                    let Ok(reply) = serde_json::from_str::<Value>(&line) else { continue };
                    if reply.get("id") == Some(asked) && reply.get("method").is_none() {
                        let outcome = &reply["result"]["outcome"];
                        answer = outcome["optionId"]
                            .as_str()
                            .or(outcome["outcome"].as_str())
                            .unwrap_or_default()
                            .to_string();
                        break;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let input = concat!(
            r#"{"jsonrpc":"2.0","id":7,"method":"initialize","params":{}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":8,"method":"session/prompt","params":{}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":"p","result":{"outcome":{"outcome":"selected","optionId":"always"}}}"#,
            "\n"
        );
        let mut out = Vec::new();
        serve(fixture, input.as_bytes(), &mut out);
        let out: Vec<Value> =
            String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(out[0]["id"], 7, "回复的编号换成这次请求的");
        assert_eq!(out[1]["method"], "session/request_permission");
        assert_eq!(out[2]["params"]["text"], "picked always");
        assert_eq!(out[3]["id"], 8);
    }
}
