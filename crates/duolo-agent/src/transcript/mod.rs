//! 读 Agent 自己写的会话记录，变成结构化的对话和工具调用（手机上看的是这些，而不是终端画面）。
//!
//! 每种格式实现 [`Format`]，插件的声明按名字引用。只读文件末尾新写的部分；
//! 只要 Agent 这次启动之后的记录（恢复会话时不重复以前的内容）。

mod claude;
mod codex;

use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;

/// 会话记录里的一条。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    User(String),
    Agent(String),
    Tool { tool: String, target: String },
}

/// 一种会话记录的格式：一行一个 JSON 对象，`timestamp` 是 UTC 时间。
pub(crate) trait Format: std::fmt::Debug + Send + Sync {
    /// 在 `worktree` 里工作的会话写在哪个文件：`data` 是 Agent 的数据目录，`since` 之前就没再写过的不算。
    /// 后台运行一次的（审阅、起名）不算。
    fn locate(&self, data: &Path, worktree: &Path, since: SystemTime) -> Option<PathBuf>;

    /// 一行记录里的内容，不关心的返回空。
    fn parse(&self, line: &Value) -> Vec<Entry>;
}

/// 按名字找格式。
pub(crate) fn format(name: &str) -> Option<&'static dyn Format> {
    match name {
        "claude" => Some(&claude::Claude),
        "codex" => Some(&codex::Codex),
        _ => None,
    }
}

/// 去哪里找 Agent 的数据目录。
#[derive(Debug, Clone)]
pub enum Home {
    /// 用户的家目录，Agent 自己的环境变量优先。
    User,
    /// 指定的目录（测试用），不看环境变量。
    At(PathBuf),
}

impl Home {
    pub(crate) fn data_dir(&self, default: &str, env: Option<&str>) -> PathBuf {
        match self {
            Self::User => env
                .and_then(std::env::var_os)
                .filter(|dir| !dir.is_empty())
                .map_or_else(|| duolo_core::process::home_dir().join(default), PathBuf::from),
            Self::At(home) => home.join(default),
        }
    }
}

/// 在后台跟着一个 Agent 的会话记录，丢掉它就停。找文件、读文件都不占用调用方的时间。
#[derive(Debug)]
pub struct Watch {
    entries: Receiver<Entry>,
    stop: Arc<AtomicBool>,
}

/// 多久看一次有没有新内容。
const POLL_EVERY: Duration = Duration::from_millis(250);
/// 多久找一次新的会话文件（Agent 刚启动时文件还没建好，或者换了会话）。
const LOCATE_EVERY: Duration = Duration::from_secs(2);

/// 认会话时最多读文件开头多少。
const HEAD_BYTES: u64 = 1024 * 1024;

impl Watch {
    pub(crate) fn start(format: &'static dyn Format, worktree: &Path, data: PathBuf) -> Self {
        let (sender, entries) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let mut reader = Reader::new(format, worktree, data, SystemTime::now());
        let stopped = Arc::clone(&stop);
        let spawned = thread::Builder::new().name("duolo-transcript".into()).spawn(move || {
            while !stopped.load(Ordering::Relaxed) {
                for entry in reader.poll() {
                    if sender.send(entry).is_err() {
                        return;
                    }
                }
                thread::sleep(POLL_EVERY);
            }
        });
        // 线程都建不了时只是没有对话记录，不影响 Agent。
        drop(spawned);
        Self { entries, stop }
    }

    /// 上次之后新写进会话记录的内容，马上返回。
    pub fn take(&self) -> Vec<Entry> {
        self.entries.try_iter().collect()
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[derive(Debug)]
struct Reader {
    format: &'static dyn Format,
    worktree: PathBuf,
    data: PathBuf,
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

impl Reader {
    fn new(format: &'static dyn Format, worktree: &Path, data: PathBuf, started: SystemTime) -> Self {
        // 留一点余量：会话文件里的时间和这里的时钟可能差一点。
        let since_time = started.checked_sub(Duration::from_secs(2)).unwrap_or(started);
        Self {
            format,
            worktree: worktree.to_path_buf(),
            data,
            since: utc_stamp(since_time),
            since_time,
            file: None,
            offset: 0,
            partial: String::new(),
            located_at: None,
        }
    }

    /// 新写进会话记录的内容。
    fn poll(&mut self) -> Vec<Entry> {
        let due = self.located_at.is_none_or(|t| t.elapsed().unwrap_or_default() >= LOCATE_EVERY);
        if due {
            self.located_at = Some(SystemTime::now());
            let found = self.format.locate(&self.data, &self.worktree, self.since_time);
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
            .flat_map(|v| self.format.parse(&v))
            .collect()
    }
}

/// 文件开头的若干行，用来认出是哪种会话。Codex 的第一行就有近 30 KB。
fn first_lines(path: &Path) -> impl Iterator<Item = String> {
    let file = File::open(path).ok().map(|f| BufReader::new(f.take(HEAD_BYTES)));
    file.into_iter().flat_map(|f| f.lines().map_while(Result::ok))
}

/// `dir` 里 `since` 之后写过的 `.jsonl` 文件。
fn recent_sessions(dir: &Path, since: SystemTime) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
        .filter(|p| modified(p).is_some_and(|m| m >= since))
        .collect()
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
fn utc_stamp(time: SystemTime) -> String {
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

    fn reader(format: &str, worktree: &str, data: &Path) -> Reader {
        Reader::new(super::format(format).unwrap(), Path::new(worktree), data.to_path_buf(), SystemTime::now())
    }

    fn soon() -> String {
        utc_stamp(SystemTime::now() + Duration::from_secs(5))
    }

    #[test]
    fn utc_stamps() {
        assert_eq!(utc_stamp(UNIX_EPOCH), "1970-01-01T00:00:00");
        assert_eq!(utc_stamp(UNIX_EPOCH + Duration::from_secs(1_795_251_266)), "2026-11-21T08:54:26");
    }

    #[test]
    fn data_dir_follows_the_environment_variable() {
        // PATH 一定有值，借来当作 Agent 的变量。
        let path = PathBuf::from(std::env::var_os("PATH").unwrap());
        assert_eq!(Home::User.data_dir(".x", Some("PATH")), path);
        assert!(Home::User.data_dir(".x", Some("DUOLO_NO_SUCH_VAR")).ends_with(".x"));
        assert_eq!(Home::At("/h".into()).data_dir(".x", Some("PATH")), Path::new("/h/.x"));
    }

    #[test]
    fn reads_new_claude_entries_only() {
        let data = tempfile::tempdir().unwrap();
        let dir = data.path().join("projects").join(claude::dir_name(Path::new("/w/shop/1-x")));
        fs::create_dir_all(&dir).unwrap();
        let mut f = File::create(dir.join("s.jsonl")).unwrap();
        // 早于这次启动的旧记录不要。
        writeln!(f, r#"{{"type":"user","timestamp":"2000-01-01T00:00:00Z","message":{{"content":"old"}}}}"#).unwrap();
        let now = soon();
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

        let mut r = reader("claude", "/w/shop/1-x", data.path());
        assert_eq!(r.poll(), vec![Entry::User("fix the login loop".into())]);
        writeln!(f, "{}", &assistant[30..]).unwrap();
        assert_eq!(
            r.poll(),
            vec![Entry::Agent("Looking.".into()), Entry::Tool { tool: "Edit".into(), target: "src/guard.ts".into() }]
        );
        assert!(r.poll().is_empty());
    }

    #[test]
    fn claude_skips_background_runs() {
        let data = tempfile::tempdir().unwrap();
        let dir = data.path().join("projects").join(claude::dir_name(Path::new("/w/shop/1-x")));
        fs::create_dir_all(&dir).unwrap();
        let now = soon();
        let session = |name: &str, entrypoint: &str, text: &str| {
            let mut f = File::create(dir.join(name)).unwrap();
            writeln!(f, r#"{{"type":"queue-operation","timestamp":"{now}Z"}}"#).unwrap();
            writeln!(
                f,
                r#"{{"type":"user","entrypoint":"{entrypoint}","timestamp":"{now}Z","message":{{"content":"{text}"}}}}"#
            )
            .unwrap();
        };
        session("writer.jsonl", "cli", "Export orders");
        // 审阅比写代码的晚开始，文件更新。
        thread::sleep(Duration::from_millis(20));
        session("review.jsonl", "sdk-cli", "Review every change");
        let mut r = reader("claude", "/w/shop/1-x", data.path());
        assert_eq!(r.poll(), vec![Entry::User("Export orders".into())]);
    }

    #[test]
    fn codex_skips_background_runs() {
        let data = tempfile::tempdir().unwrap();
        let day = data.path().join("sessions/2026/11/21");
        fs::create_dir_all(&day).unwrap();
        let now = soon();
        let session = |name: &str, source: &str, text: &str| {
            let mut f = File::create(day.join(name)).unwrap();
            // 第一行里有完整的系统提示词，很长。
            let long = "x".repeat(30 * 1024);
            writeln!(f, r#"{{"timestamp":"{now}Z","type":"session_meta","payload":{{"cwd":"/w/shop/2-y","source":"{source}","base_instructions":"{long}"}}}}"#).unwrap();
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
        let mut r = reader("codex", "/w/shop/2-y", data.path());
        assert_eq!(
            r.poll(),
            vec![Entry::Agent("working".into()), Entry::Tool { tool: "shell".into(), target: "pnpm test".into() }]
        );
    }

    #[test]
    fn watching_stops_when_dropped() {
        let data = tempfile::tempdir().unwrap();
        let watch = Watch::start(super::format("claude").unwrap(), Path::new("/w/x"), data.path().to_path_buf());
        let stop = Arc::clone(&watch.stop);
        assert!(watch.take().is_empty());
        drop(watch);
        assert!(stop.load(Ordering::Relaxed));
        // 线程退出后只剩这一份。
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while Arc::strong_count(&stop) > 1 {
            assert!(std::time::Instant::now() < deadline, "线程没有停");
            thread::sleep(Duration::from_millis(20));
        }
    }
}
