//! Codex：`<数据目录>/sessions/<年>/<月>/<日>/rollout-*.jsonl`，第一行记着工作目录；
//! 后台审阅用的 `codex exec` 也会写，它的 `source` 是 `exec`，跳过。

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::Value;

use super::{first_lines, recent_sessions, short, sorted_dirs, tool_target, user_text, Entry, Format};

#[derive(Debug)]
pub(super) struct Codex;

impl Format for Codex {
    fn locate(&self, data: &Path, worktree: &Path, since: SystemTime) -> Vec<PathBuf> {
        // 最近几天的目录就够了：会话是 Agent 这次启动后才建的。
        let mut days = Vec::new();
        for year in sorted_dirs(&data.join("sessions")).into_iter().rev().take(2) {
            for month in sorted_dirs(&year).into_iter().rev().take(2) {
                days.extend(sorted_dirs(&month).into_iter().rev().take(3));
            }
        }
        let sessions = days.iter().flat_map(|day| recent_sessions(day, since));
        sessions.filter(|p| matches(p, worktree)).collect()
    }

    /// 恢复的会话可能是很多天以前建的：文件名以编号结尾。
    fn find(&self, data: &Path, _worktree: &Path, id: &str) -> Option<PathBuf> {
        let suffix = format!("-{id}.jsonl");
        for year in sorted_dirs(&data.join("sessions")).into_iter().rev() {
            for month in sorted_dirs(&year).into_iter().rev() {
                for day in sorted_dirs(&month).into_iter().rev() {
                    let found = std::fs::read_dir(&day)
                        .into_iter()
                        .flatten()
                        .flatten()
                        .map(|e| e.path())
                        .find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().ends_with(&suffix)));
                    if found.is_some() {
                        return found;
                    }
                }
            }
        }
        None
    }

    /// 第一行的 `payload.id`。
    fn session_id(&self, path: &Path) -> Option<String> {
        let first: Value = serde_json::from_str(&first_lines(path).next()?).ok()?;
        first["payload"]["id"].as_str().map(str::to_string)
    }

    fn parse(&self, v: &Value) -> Vec<Entry> {
        if v["type"] != "response_item" {
            return Vec::new();
        }
        let p = &v["payload"];
        match p["type"].as_str() {
            Some("message") => {
                let text: String = p["content"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|i| i["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                match p["role"].as_str() {
                    Some("user") => user_text(&text).into_iter().collect(),
                    Some("assistant") if !text.trim().is_empty() => vec![Entry::Agent(text.trim().into())],
                    _ => Vec::new(),
                }
            }
            Some("function_call") => {
                let args: Value =
                    p["arguments"].as_str().and_then(|a| serde_json::from_str(a).ok()).unwrap_or(Value::Null);
                vec![Entry::Tool { tool: p["name"].as_str().unwrap_or("tool").into(), target: tool_target(&args) }]
            }
            Some("custom_tool_call") => vec![Entry::Tool {
                tool: p["name"].as_str().unwrap_or("tool").into(),
                target: short(p["input"].as_str().unwrap_or_default()),
            }],
            Some("local_shell_call") => vec![Entry::Tool { tool: "shell".into(), target: tool_target(&p["action"]) }],
            _ => Vec::new(),
        }
    }
}

/// 第一行说它在 `worktree` 里，而且不是后台运行的。
fn matches(path: &Path, worktree: &Path) -> bool {
    let Some(first) = first_lines(path).next() else { return false };
    let Ok(meta) = serde_json::from_str::<Value>(&first) else { return false };
    let payload = &meta["payload"];
    payload["cwd"].as_str().is_some_and(|cwd| Path::new(cwd) == worktree) && payload["source"].as_str() != Some("exec")
}
