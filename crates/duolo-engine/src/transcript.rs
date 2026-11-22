//! 读 Agent 自己写的会话记录，变成结构化的对话和工具调用（手机上看的是这些，而不是终端画面）。
//!
//! - Claude Code：`~/.claude/projects/<工作目录，非字母数字换成 ->/<会话>.jsonl`
//! - Codex：`~/.codex/sessions/<年>/<月>/<日>/rollout-*.jsonl`，第一行记着工作目录；
//!   后台审阅用的 `codex exec` 也会写，它的 `source` 是 `exec`，跳过。
//!
//! 只读文件末尾新写的部分；只要 Agent 这次启动之后的记录（恢复会话时不重复以前的内容）。

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;

/// 会话记录里的一条。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    User(String),
    Agent(String),
    Tool { tool: String, target: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    Claude,
    Codex,
}

impl Flavor {
    pub fn of(kind: &str) -> Option<Self> {
        match kind {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            _ => None,
        }
    }
}

/// 跟着一个 Agent 的会话记录。
#[derive(Debug)]
pub struct Watcher {
    flavor: Flavor,
    worktree: PathBuf,
    home: PathBuf,
    /// Agent 启动的时间（UTC，`YYYY-MM-DDTHH:MM:SS`），早于它的记录不要。
    since: String,
    since_time: SystemTime,
    file: Option<PathBuf>,
    offset: u64,
    /// 还没写完的最后一行。
    partial: String,
    /// 上次找会话文件的时间。
    located_at: Option<SystemTime>,
}

/// 多久找一次新的会话文件（Agent 刚启动时文件还没建好，或者换了会话）。
const LOCATE_EVERY: Duration = Duration::from_secs(2);

impl Watcher {
    pub fn new(flavor: Flavor, worktree: &Path, started: SystemTime, home: PathBuf) -> Self {
        // 留一点余量：会话文件里的时间和这里的时钟可能差一点。
        let since_time = started.checked_sub(Duration::from_secs(2)).unwrap_or(started);
        Self {
            flavor,
            worktree: worktree.to_path_buf(),
            home,
            since: utc_stamp(since_time),
            since_time,
            file: None,
            offset: 0,
            partial: String::new(),
            located_at: None,
        }
    }

    /// 新写进会话记录的内容。
    pub fn poll(&mut self) -> Vec<Entry> {
        let due = self.located_at.is_none_or(|t| t.elapsed().unwrap_or_default() >= LOCATE_EVERY);
        if due {
            self.located_at = Some(SystemTime::now());
            let found = match self.flavor {
                Flavor::Claude => self.locate_claude(),
                Flavor::Codex => self.locate_codex(),
            };
            if found.is_some() && found != self.file {
                self.file = found;
                self.offset = 0;
                self.partial.clear();
            }
        }
        let Some(file) = &self.file else { return Vec::new() };
        let Ok(mut f) = File::open(file) else { return Vec::new() };
        if f.seek(SeekFrom::Start(self.offset)).is_err() {
            return Vec::new();
        }
        let mut buf = String::new();
        let Ok(read) = f.read_to_string(&mut buf) else { return Vec::new() };
        self.offset += read as u64;
        self.partial.push_str(&buf);
        let Some(end) = self.partial.rfind('\n') else { return Vec::new() };
        let complete: String = self.partial.drain(..=end).collect();
        complete
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter(|v| {
                v.get("timestamp").and_then(Value::as_str).is_some_and(|t| t.get(..19).unwrap_or(t) >= &self.since[..])
            })
            .flat_map(|v| match self.flavor {
                Flavor::Claude => parse_claude(&v),
                Flavor::Codex => parse_codex(&v),
            })
            .collect()
    }

    fn locate_claude(&self) -> Option<PathBuf> {
        let dir = self.home.join(".claude/projects").join(claude_dir_name(&self.worktree));
        newest(
            fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "jsonl")),
        )
    }

    fn locate_codex(&self) -> Option<PathBuf> {
        let root = self.home.join(".codex/sessions");
        // 最近几天的目录就够了：会话是 Agent 这次启动后才建的。
        let mut days = Vec::new();
        for year in sorted_dirs(&root).into_iter().rev().take(2) {
            for month in sorted_dirs(&year).into_iter().rev().take(2) {
                days.extend(sorted_dirs(&month).into_iter().rev().take(3));
            }
        }
        let candidates = days
            .iter()
            .filter_map(|d| fs::read_dir(d).ok())
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
            .filter(|p| modified(p).is_some_and(|m| m >= self.since_time))
            .filter(|p| codex_session_matches(p, &self.worktree));
        newest(candidates)
    }
}

/// Claude Code 存会话的目录名：工作目录里不是字母、数字、`-` 的字符都换成 `-`。
pub fn claude_dir_name(dir: &Path) -> String {
    dir.display().to_string().chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' }).collect()
}

fn codex_session_matches(path: &Path, worktree: &Path) -> bool {
    let Ok(mut f) = File::open(path) else { return false };
    let mut head = vec![0; 16 * 1024];
    let n = f.read(&mut head).unwrap_or(0);
    let head = String::from_utf8_lossy(&head[..n]);
    let Some(first) = head.lines().next() else { return false };
    let Ok(meta) = serde_json::from_str::<Value>(first) else { return false };
    let payload = &meta["payload"];
    payload["cwd"].as_str().is_some_and(|cwd| Path::new(cwd) == worktree) && payload["source"].as_str() != Some("exec")
}

fn parse_claude(v: &Value) -> Vec<Entry> {
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

fn parse_codex(v: &Value) -> Vec<Entry> {
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
            let args: Value = p["arguments"].as_str().and_then(|a| serde_json::from_str(a).ok()).unwrap_or(Value::Null);
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

/// 用户自己说的话；Agent 自动加的内容（`<system-reminder>`、命令输出、AGENTS.md 说明）不算。
fn user_text(text: &str) -> Option<Entry> {
    let text = text.trim();
    let generated = text.is_empty() || text.starts_with('<') || text.starts_with("# AGENTS.md");
    (!generated).then(|| Entry::User(text.into()))
}

/// 工具调用的对象：文件路径、命令、搜索词，取第一个有的。
fn tool_target(input: &Value) -> String {
    for key in ["file_path", "path", "notebook_path", "pattern", "url", "query", "description"] {
        if let Some(s) = input[key].as_str() {
            return short(s);
        }
    }
    match &input["command"] {
        Value::String(s) => short(s),
        Value::Array(parts) => short(&parts.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" ")),
        _ => String::new(),
    }
}

/// 第一行，最多 160 个字符。
fn short(s: &str) -> String {
    let line = s.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    duolo_core::text::truncate_chars(line, 160)
}

fn newest(paths: impl Iterator<Item = PathBuf>) -> Option<PathBuf> {
    paths.filter_map(|p| modified(&p).map(|m| (m, p))).max_by_key(|(m, _)| *m).map(|(_, p)| p)
}

fn modified(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn sorted_dirs(dir: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> =
        fs::read_dir(dir).map(|r| r.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect()).unwrap_or_default();
    dirs.sort();
    dirs
}

/// UTC 时间写成 `YYYY-MM-DDTHH:MM:SS`，和会话记录里时间戳的前 19 位同样格式，可以直接比大小。
pub fn utc_stamp(time: SystemTime) -> String {
    let secs = time.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // 公历换算（Howard Hinnant 的 civil_from_days）。
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}", rem / 3600, rem % 3600 / 60, rem % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn utc_stamps() {
        assert_eq!(utc_stamp(UNIX_EPOCH), "1970-01-01T00:00:00");
        assert_eq!(utc_stamp(UNIX_EPOCH + Duration::from_secs(1_795_251_266)), "2026-11-21T08:54:26");
    }

    #[test]
    fn claude_dir_names() {
        assert_eq!(claude_dir_name(Path::new("/Users/a/.duolo/wt/shop/5-hi")), "-Users-a--duolo-wt-shop-5-hi");
    }

    #[test]
    fn reads_new_claude_entries_only() {
        let home = tempfile::tempdir().unwrap();
        let worktree = Path::new("/w/shop/1-x");
        let dir = home.path().join(".claude/projects").join(claude_dir_name(worktree));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("s.jsonl");
        let mut f = File::create(&file).unwrap();
        // 早于这次启动的旧记录不要。
        writeln!(f, r#"{{"type":"user","timestamp":"2000-01-01T00:00:00Z","message":{{"content":"old"}}}}"#).unwrap();
        let now = utc_stamp(SystemTime::now() + Duration::from_secs(5));
        writeln!(f, r#"{{"type":"user","timestamp":"{now}Z","message":{{"content":"fix the login loop"}}}}"#).unwrap();
        writeln!(
            f,
            r#"{{"type":"user","timestamp":"{now}Z","message":{{"content":"<system-reminder>x</system-reminder>"}}}}"#
        )
        .unwrap();
        let assistant = format!(
            r#"{{"type":"assistant","timestamp":"{now}Z","message":{{"content":[{{"type":"thinking"}},{{"type":"text","text":"Looking."}},{{"type":"tool_use","name":"Edit","input":{{"file_path":"src/guard.ts"}}}}]}}}}"#
        );
        // 最后一行还没写完。
        write!(f, "{}", &assistant[..30]).unwrap();
        f.flush().unwrap();

        let mut w = Watcher::new(Flavor::Claude, worktree, SystemTime::now(), home.path().to_path_buf());
        assert_eq!(w.poll(), vec![Entry::User("fix the login loop".into())]);
        writeln!(f, "{}", &assistant[30..]).unwrap();
        assert_eq!(
            w.poll(),
            vec![Entry::Agent("Looking.".into()), Entry::Tool { tool: "Edit".into(), target: "src/guard.ts".into() }]
        );
        assert!(w.poll().is_empty());
    }

    #[test]
    fn codex_skips_background_reviews() {
        let home = tempfile::tempdir().unwrap();
        let worktree = Path::new("/w/shop/2-y");
        let day = home.path().join(".codex/sessions/2026/11/21");
        fs::create_dir_all(&day).unwrap();
        let now = utc_stamp(SystemTime::now() + Duration::from_secs(5));
        let session = |name: &str, source: &str, text: &str| {
            let mut f = File::create(day.join(name)).unwrap();
            writeln!(f, r#"{{"timestamp":"{now}Z","type":"session_meta","payload":{{"cwd":"/w/shop/2-y","source":"{source}"}}}}"#).unwrap();
            writeln!(
                f,
                r#"{{"timestamp":"{now}Z","type":"response_item","payload":{{"type":"message","role":"assistant","content":[{{"type":"output_text","text":"{text}"}}]}}}}"#
            )
            .unwrap();
            writeln!(
                f,
                r#"{{"timestamp":"{now}Z","type":"response_item","payload":{{"type":"function_call","name":"shell","arguments":"{{\"command\":[\"pnpm\",\"test\"]}}"}}}}"#
            )
            .unwrap();
        };
        session("rollout-a.jsonl", "exec", "review");
        session("rollout-b.jsonl", "cli", "working");
        let mut w = Watcher::new(Flavor::Codex, worktree, SystemTime::now(), home.path().to_path_buf());
        assert_eq!(
            w.poll(),
            vec![Entry::Agent("working".into()), Entry::Tool { tool: "shell".into(), target: "pnpm test".into() }]
        );
    }
}
