//! 任务事件的记录：每个任务一个只追加的文件 `~/.duolo/events/<任务编号>.jsonl`，一行一个事件。
//! 应用重启后接着原来的编号往下记；客户端用 [`EventLog::since`] 补齐断线期间的事件。

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use duolo_core::{EventKind, TaskEvent, TaskId};

#[derive(Debug)]
pub struct EventLog {
    dir: PathBuf,
    /// 每个任务下一条事件的编号（读过文件之后才有）。
    next_seq: HashMap<TaskId, u64>,
}

impl EventLog {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir, next_seq: HashMap::new() }
    }

    fn file(&self, task: TaskId) -> PathBuf {
        self.dir.join(format!("{}.jsonl", task.0))
    }

    /// 记一条事件，返回带编号的事件。
    pub fn append(&mut self, task: TaskId, kind: EventKind) -> Result<TaskEvent> {
        let seq = match self.next_seq.get(&task) {
            Some(seq) => *seq,
            None => self.read(task)?.last().map_or(1, |e| e.seq + 1),
        };
        let event = TaskEvent { task, seq, at: now_millis(), kind };
        fs::create_dir_all(&self.dir).with_context(|| format!("cannot create {}", self.dir.display()))?;
        let mut file = OpenOptions::new().create(true).append(true).open(self.file(task))?;
        writeln!(file, "{}", serde_json::to_string(&event)?)?;
        self.next_seq.insert(task, seq + 1);
        Ok(event)
    }

    /// 第 `after` 条之后的事件（`after` 为 0 时是全部）。
    pub fn since(&self, task: TaskId, after: u64) -> Result<Vec<TaskEvent>> {
        Ok(self.read(task)?.into_iter().filter(|e| e.seq > after).collect())
    }

    /// 删掉已经不存在的任务的事件（关闭了的任务，应用启动时清理）。
    pub fn prune(&mut self, keep: &[TaskId]) {
        let Ok(entries) = fs::read_dir(&self.dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            let id = path.file_stem().and_then(|s| s.to_str()).and_then(|s| s.parse().ok()).map(TaskId);
            if id.is_some_and(|id| !keep.contains(&id)) {
                let _ = fs::remove_file(path);
            }
        }
    }

    fn read(&self, task: TaskId) -> Result<Vec<TaskEvent>> {
        let Ok(file) = fs::File::open(self.file(task)) else { return Ok(Vec::new()) };
        // 写到一半的最后一行（应用被强制退出时）跳过。
        Ok(BufReader::new(file).lines().map_while(Result::ok).filter_map(|l| serde_json::from_str(&l).ok()).collect())
    }
}

pub fn now_millis() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use duolo_core::Speaker;

    #[test]
    fn numbers_events_and_survives_restart() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = EventLog::new(dir.path().to_path_buf());
        let task = TaskId(4);
        let msg = |text: &str| EventKind::Message { speaker: Speaker::User, agent: 0, text: text.into() };
        assert_eq!(log.append(task, msg("a")).unwrap().seq, 1);
        assert_eq!(log.append(task, msg("b")).unwrap().seq, 2);
        assert_eq!(log.append(TaskId(5), msg("other")).unwrap().seq, 1);

        let mut log = EventLog::new(dir.path().to_path_buf());
        assert_eq!(log.append(task, msg("c")).unwrap().seq, 3, "重启后接着编号");
        let after_one: Vec<u64> = log.since(task, 1).unwrap().iter().map(|e| e.seq).collect();
        assert_eq!(after_one, [2, 3]);

        log.prune(&[task]);
        assert!(log.since(TaskId(5), 0).unwrap().is_empty(), "不存在的任务的事件被清理");
        assert_eq!(log.since(task, 0).unwrap().len(), 3, "还在的任务不动");
    }
}
