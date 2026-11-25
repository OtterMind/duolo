//! Grok：`<数据目录>/sessions/<URL 编码的工作目录>/<会话编号>/chat_history.jsonl`（见它自带的 docs/user-guide/17-sessions.md）。
//! 目录名太长（超过 255 字节）时换成简称加哈希，原路径写在目录里的 `.cwd` 文件里。
//! `chat_history.jsonl` 一行一条发给模型的消息：你说的话包在 `<user_query>` 里，Agent 的回复带着 `tool_calls`。
//! 每行没有时间，靠文件的读取位置区分新旧（见 [`Format::dated`]）。
//! 后台运行（`grok -p`，审阅、起名）也会留会话，`summary.json` 里 `session_kind` 是 `headless`，跳过。

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::Value;

use super::{modified, tool_target, Entry, Format};

const FILE: &str = "chat_history.jsonl";

#[derive(Debug)]
pub(super) struct Grok;

impl Format for Grok {
    fn locate(&self, data: &Path, worktree: &Path, since: SystemTime) -> Vec<PathBuf> {
        let sessions = groups(data, worktree).into_iter().flat_map(|group| subdirs(&group));
        sessions
            .filter(|dir| !headless(dir))
            .map(|dir| dir.join(FILE))
            .filter(|p| modified(p).is_some_and(|m| m >= since))
            .collect()
    }

    /// 会话目录的名字。
    fn session_id(&self, path: &Path) -> Option<String> {
        path.parent()?.file_name().map(|n| n.to_string_lossy().into_owned())
    }

    fn find(&self, data: &Path, worktree: &Path, id: &str) -> Option<PathBuf> {
        groups(data, worktree).into_iter().map(|group| group.join(id).join(FILE)).find(|p| p.is_file())
    }

    fn parse(&self, v: &Value) -> Vec<Entry> {
        match v["type"].as_str() {
            Some("user") => texts(&v["content"]).iter().filter_map(|t| user_query(t)).map(Entry::User).collect(),
            Some("assistant") => {
                let said = v["content"].as_str().map(str::trim).filter(|t| !t.is_empty());
                let tools = v["tool_calls"].as_array().into_iter().flatten().map(|call| {
                    let args: Value =
                        call["arguments"].as_str().and_then(|a| serde_json::from_str(a).ok()).unwrap_or_default();
                    Entry::Tool {
                        tool: tool_name(call["name"].as_str().unwrap_or("tool")),
                        target: tool_target(&args_of(args)),
                    }
                });
                said.map(|t| Entry::Agent(t.into())).into_iter().chain(tools).collect()
            }
            _ => Vec::new(),
        }
    }

    fn dated(&self) -> bool {
        false
    }
}

/// 存这个目录的会话的那一组：目录名解码后就是工作目录，或者 `.cwd` 里写着它。
fn groups(data: &Path, worktree: &Path) -> Vec<PathBuf> {
    let wanted = worktree.display().to_string();
    subdirs(&data.join("sessions"))
        .into_iter()
        .filter(|dir| {
            let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            decode(&name).as_deref() == Some(wanted.as_str())
                || std::fs::read_to_string(dir.join(".cwd")).is_ok_and(|cwd| cwd.trim() == wanted)
        })
        .collect()
}

/// 后台运行留下的会话。
fn headless(session: &Path) -> bool {
    let summary = std::fs::read_to_string(session.join("summary.json")).unwrap_or_default();
    serde_json::from_str::<Value>(&summary).is_ok_and(|s| s["session_kind"] == "headless")
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let entries = std::fs::read_dir(dir).into_iter().flatten().flatten();
    entries.map(|e| e.path()).filter(|p| p.is_dir()).collect()
}

/// URL 解码（`%2F` → `/`）；不是合法的 UTF-8 时为 None。
fn decode(name: &str) -> Option<String> {
    let bytes = name.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// 消息内容里的文字：一个字符串，或者 `[{"type": "text", "text": …}]`。
fn texts(content: &Value) -> Vec<String> {
    match content {
        Value::String(s) => vec![s.clone()],
        Value::Array(items) => items.iter().filter_map(|i| i["text"].as_str().map(str::to_string)).collect(),
        _ => Vec::new(),
    }
}

/// 用户自己说的话：`<user_query>` 里的。别的（`<user_info>`、`<system-reminder>`）是它自己加的。
fn user_query(text: &str) -> Option<String> {
    let start = text.find("<user_query>")? + "<user_query>".len();
    let end = text[start..].find("</user_query>").map_or(text.len(), |i| start + i);
    Some(text[start..end].trim().to_string()).filter(|t| !t.is_empty())
}

/// 它的工具名换成会话记录里常见的说法（和聊天模式下 ACP 报的一样），别的照原样。
fn tool_name(name: &str) -> String {
    let common = match name {
        "run_terminal_command" => "Bash",
        "read_file" => "Read",
        "write" | "search_replace" => "Edit",
        "grep" => "Search",
        "web_fetch" => "Fetch",
        other => other,
    };
    common.to_string()
}

/// 它的参数名换成 [`tool_target`] 认得的（`target_file` → `file_path`）。
fn args_of(mut args: Value) -> Value {
    for (from, to) in [("target_file", "file_path"), ("target_directory", "path")] {
        if let Some(v) = args.get(from).cloned() {
            args[to] = v;
        }
    }
    args
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn messages_and_tools() {
        let user = json!({"type": "user", "content": [{"type": "text", "text": "<user_query>\nRun the tests\n</user_query>"}]});
        assert_eq!(Grok.parse(&user), [Entry::User("Run the tests".into())]);
        let reminder = json!({"type": "user", "content": [{"type": "text", "text": "<system-reminder>\nskills\n</system-reminder>"}]});
        assert!(Grok.parse(&reminder).is_empty());
        let reply = json!({"type": "assistant", "content": "On it.", "tool_calls": [
            {"id": "c1", "name": "read_file", "arguments": "{\"target_file\":\"src/a.ts\"}"},
            {"id": "c2", "name": "run_terminal_command", "arguments": "{\"command\":\"pnpm test\",\"description\":\"Run\"}"},
        ]});
        assert_eq!(
            Grok.parse(&reply),
            [
                Entry::Agent("On it.".into()),
                Entry::Tool { tool: "Read".into(), target: "src/a.ts".into() },
                Entry::Tool { tool: "Bash".into(), target: "pnpm test".into() },
            ]
        );
        assert!(Grok.parse(&json!({"type": "reasoning"})).is_empty());
    }

    #[test]
    fn groups_by_encoded_or_recorded_directory() {
        let data = tempfile::tempdir().unwrap();
        let sessions = data.path().join("sessions");
        std::fs::create_dir_all(sessions.join("%2Fw%2Fshop%2F1-export/s1")).unwrap();
        std::fs::create_dir_all(sessions.join("long-slug-1234/s2")).unwrap();
        std::fs::write(sessions.join("long-slug-1234/.cwd"), "/w/very/long\n").unwrap();
        std::fs::write(sessions.join("%2Fw%2Fshop%2F1-export/s1").join(FILE), "").unwrap();
        std::fs::create_dir_all(sessions.join("%2Fw%2Fshop%2F1-export/bg")).unwrap();
        std::fs::write(sessions.join("%2Fw%2Fshop%2F1-export/bg").join(FILE), "").unwrap();
        std::fs::write(sessions.join("%2Fw%2Fshop%2F1-export/bg/summary.json"), r#"{"session_kind":"headless"}"#)
            .unwrap();
        let since = SystemTime::UNIX_EPOCH;
        let found = Grok.locate(data.path(), Path::new("/w/shop/1-export"), since);
        assert_eq!(found.len(), 1, "后台运行的不算：{found:?}");
        assert_eq!(groups(data.path(), Path::new("/w/shop/1-export")).len(), 1);
        assert_eq!(groups(data.path(), Path::new("/w/very/long")).len(), 1);
        let found = Grok.find(data.path(), Path::new("/w/shop/1-export"), "s1").unwrap();
        assert_eq!(Grok.session_id(&found).as_deref(), Some("s1"));
        assert_eq!(decode("%E4%BB%BB%E5%8A%A1"), Some("任务".into()));
    }
}
