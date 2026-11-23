//! pi：`<数据目录>/sessions/--<工作目录>--/<时间>_<会话编号>.jsonl`（工作目录去掉开头的分隔符，`/`、`\`、`:` 换成 `-`）。
//! 第一行是会话头 `{"type":"session","id":…,"cwd":…}`，之后每行一个 `{"type":"message","message":{"role":…}}`。
//! 格式见 pi 自带的 docs/session-format.md。后台运行时用 `--no-session`，不留文件。

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::Value;

use super::{first_lines, recent_sessions, tool_target, user_text, Entry, Format};

#[derive(Debug)]
pub(super) struct Pi;

impl Format for Pi {
    fn locate(&self, data: &Path, worktree: &Path, since: SystemTime) -> Vec<PathBuf> {
        recent_sessions(&data.join("sessions").join(dir_name(worktree)), since)
    }

    /// 会话头里的 `id`。
    fn session_id(&self, path: &Path) -> Option<String> {
        let head: Value = serde_json::from_str(&first_lines(path).next()?).ok()?;
        (head["type"] == "session").then(|| head["id"].as_str().map(str::to_string)).flatten()
    }

    fn find(&self, data: &Path, worktree: &Path, id: &str) -> Option<PathBuf> {
        let suffix = format!("_{id}.jsonl");
        std::fs::read_dir(data.join("sessions").join(dir_name(worktree)))
            .ok()?
            .flatten()
            .map(|e| e.path())
            .find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().ends_with(&suffix)))
    }

    fn parse(&self, v: &Value) -> Vec<Entry> {
        if v["type"] != "message" {
            return Vec::new();
        }
        let message = &v["message"];
        let content = &message["content"];
        match message["role"].as_str() {
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
                    Some("toolCall") => Some(Entry::Tool {
                        tool: item["name"].as_str().unwrap_or("tool").into(),
                        target: tool_target(&item["arguments"]),
                    }),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        }
    }
}

/// pi 存会话的目录名：`--<去掉开头分隔符、/ \ : 换成 - 的工作目录>--`。
pub(super) fn dir_name(dir: &Path) -> String {
    let path = dir.display().to_string();
    let trimmed = path.trim_start_matches(['/', '\\']);
    format!("--{}--", trimmed.replace(['/', '\\', ':'], "-"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dir_names() {
        assert_eq!(dir_name(Path::new("/Users/a/.duolo/wt/shop/5-hi")), "--Users-a-.duolo-wt-shop-5-hi--");
        assert_eq!(dir_name(Path::new(r"C:\code\shop")), "--C--code-shop--");
    }
}
