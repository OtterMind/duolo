//! 钩子：Agent 在会话开始、请求授权这些时候运行的命令（为什么这样做见 docs/agent-compat.md）。
//!
//! Duolo 自己的程序带上 [`HOOK_ARG`] 就是这个命令：把 Agent 交给它的事件（标准输入里的一个 JSON）
//! 原样写成一个文件，放进环境变量 [`HOOK_DIR_ENV`] 指的目录，引擎去那里读、读完删掉。
//! 不起服务、不用令牌和 curl，Windows 上一样能用；Duolo 没在运行时，事件留在目录里，下次再读。

use std::io::Read;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// 当钩子运行时程序的第一个参数。
pub const HOOK_ARG: &str = "__hook";
/// 事件写到哪个目录。Duolo 启动 Agent 时设置，钩子进程从 Agent 那里继承。
pub const HOOK_DIR_ENV: &str = "DUOLO_HOOK_DIR";
/// 一个事件最大多少（写文件的工具会把整个文件内容带进来，Duolo 用不着那么多）。
const MAX_EVENT: u64 = 1024 * 1024;

/// 当钩子运行：读标准输入，写一个事件文件。出什么问题都直接结束，不能挡住 Agent。
pub fn run() {
    let Some(dir) = std::env::var_os(HOOK_DIR_ENV).filter(|d| !d.is_empty()) else { return };
    let mut input = String::new();
    if std::io::stdin().take(MAX_EVENT).read_to_string(&mut input).is_err() {
        return;
    }
    let _ = write_event(Path::new(&dir), &input);
}

/// 写成 `<纳秒>-<进程号>.json`：按名字排序就是先后顺序。先写临时文件再改名，读的一方不会读到半个。
fn write_event(dir: &Path, input: &str) -> std::io::Result<()> {
    // 只收 JSON 对象；写成一行，免得 Agent 给的格式里有多余的换行。
    let Ok(value) = serde_json::from_str::<serde_json::Value>(input) else { return Ok(()) };
    if !value.is_object() {
        return Ok(());
    }
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let name = format!("{nanos:024}-{}", std::process::id());
    let temp = dir.join(format!(".{name}.tmp"));
    std::fs::write(&temp, value.to_string())?;
    std::fs::rename(&temp, dir.join(format!("{name}.json")))
}

/// 目录里还没读的事件，按先后顺序，读完就删掉。读不懂的也删掉。
pub fn take_events(dir: &Path) -> Vec<serde_json::Value> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut files: Vec<_> =
        entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect();
    files.sort();
    files
        .into_iter()
        .filter_map(|path| {
            let text = std::fs::read_to_string(&path).ok();
            let _ = std::fs::remove_file(&path);
            serde_json::from_str(&text?).ok()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_come_back_in_order_once() {
        let dir = tempfile::tempdir().unwrap();
        write_event(dir.path(), "{\"hook_event_name\":\"SessionStart\",\n \"session_id\":\"a\"}\n").unwrap();
        write_event(dir.path(), "{\"hook_event_name\":\"Stop\"}").unwrap();
        write_event(dir.path(), "not json").unwrap();
        write_event(dir.path(), "[1]").unwrap();
        let events = take_events(dir.path());
        let names: Vec<_> = events.iter().map(|e| e["hook_event_name"].as_str().unwrap()).collect();
        assert_eq!(names, ["SessionStart", "Stop"]);
        assert!(take_events(dir.path()).is_empty(), "读过的删掉了");
        assert!(take_events(&dir.path().join("missing")).is_empty());
    }
}
