//! 会话存在数据库里的 Agent：用它自己的命令列出会话（插件的 `[session_list]`），认出这次的会话编号。
//! 公开的命令比直接读它的数据库稳：数据库是它内部的东西，升级就可能变。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use duolo_core::process::{output_within, OneShot, Prompted};
use serde_json::Value;

use super::{is_session_id, Sessions};
use crate::manifest::SessionListSpec;

/// 还没认出来时多久问一次；认出来以后（用户可能新开了会话）多久问一次。
const UNTIL_FOUND: Duration = Duration::from_secs(3);
const AFTER_FOUND: Duration = Duration::from_secs(15);
const TIMEOUT: Duration = Duration::from_secs(10);

/// 在后台一直问，直到 `stop`。认出的编号写进 `session`。
pub(crate) fn run(
    argv: Vec<String>,
    spec: SessionListSpec,
    worktree: PathBuf,
    known: Sessions,
    others: Arc<Mutex<Vec<String>>>,
    session: Arc<Mutex<Option<String>>>,
    stop: Arc<AtomicBool>,
) {
    let started = now_ms().saturating_sub(2_000);
    let dirs = [Some(worktree.clone()), worktree.canonicalize().ok()];
    while !stop.load(Ordering::Relaxed) {
        let mut cmd = match Prompted::new(&OneShot::from(argv.clone()), None) {
            Some(cmd) => cmd,
            None => return,
        };
        cmd.in_dir(&worktree);
        let listed = output_within(cmd, TIMEOUT).ok().filter(|(status, _)| status.success()).map(|(_, out)| out);
        let others = others.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let found = listed.as_deref().and_then(|out| pick(out, &spec, &dirs, started, known.own.as_deref(), &others));
        let wait = if found.is_some() { AFTER_FOUND } else { UNTIL_FOUND };
        if found.is_some() {
            *session.lock().unwrap_or_else(|p| p.into_inner()) = found;
        }
        // 分小段睡，丢掉 Watch 时很快就停。
        let mut slept = Duration::ZERO;
        while slept < wait && !stop.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(200));
            slept += Duration::from_millis(200);
        }
    }
}

/// 在这个目录里、这次启动之后建的（或者正在恢复的那个）、不是别的 Agent 的会话里，最新的一个。
fn pick(
    listing: &str,
    spec: &SessionListSpec,
    dirs: &[Option<PathBuf>],
    since_ms: u64,
    own: Option<&str>,
    others: &[String],
) -> Option<String> {
    let items: Vec<Value> = serde_json::from_str(listing).ok()?;
    items
        .iter()
        .filter_map(|item| {
            let id = item[&spec.id].as_str()?;
            let dir = Path::new(item[&spec.directory].as_str()?);
            let created = item[&spec.created].as_u64().unwrap_or(0);
            let here = dirs.iter().flatten().any(|d| d == dir);
            let ours = Some(id) == own || created >= since_ms;
            (here && ours && is_session_id(id) && !others.iter().any(|o| o == id)).then(|| (created, id.to_string()))
        })
        .max_by_key(|(created, _)| *created)
        .map(|(_, id)| id)
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> SessionListSpec {
        SessionListSpec { args: Vec::new(), id: "id".into(), directory: "directory".into(), created: "created".into() }
    }

    #[test]
    fn picks_this_folders_new_session() {
        let listing = r#"[
            {"id": "ses_old", "directory": "/w/1-x", "created": 100},
            {"id": "ses_other_task", "directory": "/w/2-y", "created": 5000},
            {"id": "ses_new", "directory": "/w/1-x", "created": 3000},
            {"id": "ses_newest_but_taken", "directory": "/w/1-x", "created": 4000}
        ]"#;
        let dirs = [Some(PathBuf::from("/w/1-x")), None];
        let taken = vec!["ses_newest_but_taken".to_string()];
        assert_eq!(pick(listing, &spec(), &dirs, 1000, None, &taken).as_deref(), Some("ses_new"));
        // 恢复的那个是很早以前建的，也认。
        assert_eq!(pick(listing, &spec(), &dirs, 10_000, Some("ses_old"), &[]).as_deref(), Some("ses_old"));
        assert_eq!(pick(listing, &spec(), &dirs, 10_000, None, &[]), None, "这次启动之后还没建");
        assert_eq!(pick("not json", &spec(), &dirs, 0, None, &[]), None);
    }
}
