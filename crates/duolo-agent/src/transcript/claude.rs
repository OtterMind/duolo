//! Claude Code：`<数据目录>/projects/<工作目录，非字母数字换成 ->/<会话>.jsonl`。

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::Value;

use super::{first_lines, recent_sessions, tool_target, user_text, Entry, Format};

#[derive(Debug)]
pub(super) struct Claude;

impl Format for Claude {
    fn locate(&self, data: &Path, worktree: &Path, since: SystemTime) -> Vec<PathBuf> {
        let sessions = recent_sessions(&data.join("projects").join(dir_name(worktree)), since);
        sessions.into_iter().filter(|p| !is_headless(p)).collect()
    }

    /// 文件名就是会话编号。
    fn session_id(&self, path: &Path) -> Option<String> {
        path.file_stem().map(|s| s.to_string_lossy().into_owned())
    }

    fn find(&self, data: &Path, worktree: &Path, id: &str) -> Option<PathBuf> {
        let path = data.join("projects").join(dir_name(worktree)).join(format!("{id}.jsonl"));
        path.is_file().then_some(path)
    }

    fn parse(&self, v: &Value) -> Vec<Entry> {
        if v["isMeta"].as_bool() == Some(true) || v["isSidechain"].as_bool() == Some(true) {
            return Vec::new();
        }
        let content = &v["message"]["content"];
        match v["type"].as_str() {
            Some("user") => match content {
                Value::String(text) => user_text(text).into_iter().collect(),
                Value::Array(items) => items
                    .iter()
                    .filter(|i| i["type"] == "text")
                    .filter_map(|i| i["text"].as_str().and_then(user_text))
                    .collect(),
                _ => Vec::new(),
            },
            Some("assistant") => content
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|item| match item["type"].as_str() {
                    Some("text") => {
                        item["text"].as_str().map(str::trim).filter(|t| !t.is_empty()).map(|t| Entry::Agent(t.into()))
                    }
                    Some("tool_use") => Some(Entry::Tool {
                        tool: item["name"].as_str().unwrap_or("tool").into(),
                        target: tool_target(&item["input"]),
                    }),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        }
    }
}

/// Claude Code 存会话的目录名：工作目录里不是字母、数字、`-` 的字符都换成 `-`。
pub(super) fn dir_name(dir: &Path) -> String {
    dir.display().to_string().chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' }).collect()
}

/// `claude -p`（后台审阅）的会话：和终端里的写在同一个目录，`entrypoint` 是 `sdk-cli`。
fn is_headless(path: &Path) -> bool {
    first_lines(path)
        .filter_map(|line| serde_json::from_str::<Value>(&line).ok())
        .find_map(|v| v["entrypoint"].as_str().map(|e| e.starts_with("sdk")))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dir_names() {
        assert_eq!(dir_name(Path::new("/Users/a/.duolo/wt/shop/5-hi")), "-Users-a--duolo-wt-shop-5-hi");
    }
}
