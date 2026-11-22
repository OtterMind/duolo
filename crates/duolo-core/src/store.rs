//! `state.json` 等 JSON 文件的读写。
//!
//! - 写入先写临时文件、落盘，再改名，中途崩溃或断电也不会留下半个文件；
//! - `state.json` 每次保存都另存一份 `state.json.bak`，读到损坏的文件时用它恢复；
//! - 桌面应用、终端界面和 `duolo new` 会同时用同一份 `state.json`：修改前用 [`Store::lock`] 独占，
//!   在锁里重新读一遍再改（见 `Engine::update`），不会互相覆盖。

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use anyhow::{bail, Context, Result};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::model::State;

/// 等别的进程放开锁的最长时间。正常情况下只占用几毫秒。
const LOCK_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct Store {
    path: PathBuf,
}

/// 独占 `state.json`，丢弃时放开。
#[derive(Debug)]
pub struct StoreLock(#[allow(dead_code)] Option<File>);

/// 打开时读到的内容。
#[derive(Debug)]
pub struct Loaded {
    pub state: State,
    /// `state.json` 损坏时，损坏的文件挪到了哪里。
    pub damaged: Option<PathBuf>,
    /// 损坏后用备份恢复了（否则是从空的开始）。
    pub restored: bool,
    /// 本来就没有 `state.json`。
    pub missing: bool,
}

/// `state.json` 的版本，变了说明别的进程改过。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp(SystemTime, u64);

enum Read {
    Missing,
    Damaged,
    Ok(State),
}

impl Store {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    fn sibling(&self, suffix: &str) -> PathBuf {
        let mut name = self.path.file_name().unwrap_or_default().to_os_string();
        name.push(suffix);
        self.path.with_file_name(name)
    }

    /// 独占 `state.json`，别的进程正在改时等它改完。
    pub fn lock(&self) -> Result<StoreLock> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let path = self.sibling(".lock");
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .with_context(|| format!("cannot open {}", path.display()))?;
        let deadline = Instant::now() + LOCK_TIMEOUT;
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(StoreLock(Some(file))),
                Err(TryLockError::WouldBlock) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                Err(TryLockError::WouldBlock) => bail!("{} is locked by another process", path.display()),
                // 有的文件系统（例如某些网络盘）不支持文件锁，只能不加锁。
                Err(TryLockError::Error(err)) if err.kind() == io::ErrorKind::Unsupported => {
                    return Ok(StoreLock(None));
                }
                Err(TryLockError::Error(err)) => {
                    return Err(err).with_context(|| format!("cannot lock {}", path.display()));
                }
            }
        }
    }

    /// 启动时读取。文件损坏时把它挪到一边，改用备份；备份也不能用就从空的开始。要先 [`Store::lock`]。
    pub fn load(&self) -> Result<Loaded> {
        self.remove_stale_temps();
        match self.read(&self.path)? {
            Read::Ok(state) => Ok(Loaded { state, damaged: None, restored: false, missing: false }),
            Read::Missing => Ok(Loaded { state: State::default(), damaged: None, restored: false, missing: true }),
            Read::Damaged => {
                let backup = match self.read(&self.sibling(".bak")) {
                    Ok(Read::Ok(state)) => Some(state),
                    _ => None,
                };
                let damaged = self.set_aside()?;
                let restored = backup.is_some();
                let state = backup.unwrap_or_default();
                self.save(&state)?;
                Ok(Loaded { state, damaged: Some(damaged), restored, missing: false })
            }
        }
    }

    /// 修改前重新读一遍，拿到别的进程刚写的内容。要先 [`Store::lock`]。
    /// 文件没了或损坏时返回 None，调用方接着用内存里的（损坏的文件挪到一边，不覆盖它）。
    pub fn reload(&self) -> Result<Option<State>> {
        match self.read(&self.path)? {
            Read::Ok(state) => Ok(Some(state)),
            Read::Missing => Ok(None),
            Read::Damaged => {
                self.set_aside()?;
                Ok(None)
            }
        }
    }

    /// 只看一眼最新内容，不动文件，也不用加锁。读不到时返回 None。
    pub fn peek(&self) -> Option<State> {
        match self.read(&self.path) {
            Ok(Read::Ok(state)) => Some(state),
            _ => None,
        }
    }

    pub fn stamp(&self) -> Option<Stamp> {
        let meta = std::fs::metadata(&self.path).ok()?;
        Some(Stamp(meta.modified().ok()?, meta.len()))
    }

    /// 保存，并更新备份。要先 [`Store::lock`]。
    pub fn save(&self, state: &State) -> Result<()> {
        save_json(&self.path, state)?;
        // 主文件已经安全落盘，备份写失败（例如磁盘刚好满了）不影响这次保存。
        let _ = save_json(&self.sibling(".bak"), state);
        Ok(())
    }

    /// 旧版本没有备份：启动时先补一份，不等下次保存。
    pub fn ensure_backup(&self, state: &State) {
        let backup = self.sibling(".bak");
        if !backup.exists() {
            let _ = save_json(&backup, state);
        }
    }

    fn read(&self, path: &Path) -> Result<Read> {
        match std::fs::read(path) {
            Ok(bytes) => Ok(parse(&bytes).map_or(Read::Damaged, Read::Ok)),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(Read::Missing),
            Err(err) => Err(err).with_context(|| format!("cannot read {}", path.display())),
        }
    }

    /// 写到一半被强制结束的进程留下的临时文件。刚写的不动，可能正要改名。
    fn remove_stale_temps(&self) {
        let Some(dir) = self.path.parent() else { return };
        let prefix = self.path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let old = entry
                .metadata()
                .and_then(|m| m.modified())
                .is_ok_and(|at| at.elapsed().unwrap_or_default() > Duration::from_secs(60));
            if old && name.starts_with(&prefix) && name.ends_with(".tmp") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    /// 把损坏的 `state.json` 改名留着，返回新的位置。
    fn set_aside(&self) -> Result<PathBuf> {
        let to = self.sibling(&format!(".damaged-{}", crate::text::now_secs()));
        replace(&self.path, &to).with_context(|| format!("cannot move {} aside", self.path.display()))?;
        Ok(to)
    }
}

/// 字段都有默认值，`{}`、`[]` 也能解析成空的状态，但我们写的文件一定有 `next_task_id`：没有就是坏了。
fn parse(bytes: &[u8]) -> Option<State> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    value.get("next_task_id")?;
    serde_json::from_value(value).ok()
}

/// 读取 JSON 文件；文件不存在时返回默认值。
pub fn load_json<T: DeserializeOwned + Default>(path: &Path) -> Result<T> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).with_context(|| format!("file is corrupted: {}", path.display())),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(T::default()),
        Err(err) => Err(err).with_context(|| format!("cannot read {}", path.display())),
    }
}

/// 原子地写入 JSON 文件。
pub fn save_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // 临时文件名带上进程号，两个进程同时写也不会写到同一个临时文件里。
    let mut tmp = path.file_name().unwrap_or_default().to_os_string();
    tmp.push(format!(".{}.tmp", std::process::id()));
    let tmp = path.with_file_name(tmp);
    let bytes = serde_json::to_vec_pretty(value)?;
    let written = File::create(&tmp).and_then(|mut file| {
        file.write_all(&bytes)?;
        // 先落盘再改名：否则断电后可能留下一个空文件。
        file.sync_all()
    });
    if let Err(err) = written.and_then(|()| replace(&tmp, path)) {
        let _ = std::fs::remove_file(&tmp);
        return Err(err).with_context(|| format!("cannot save {}", path.display()));
    }
    Ok(())
}

/// 改名并覆盖。Windows 上目标刚好被杀毒软件或别的进程打开时会短暂失败，稍等再试。
fn replace(from: &Path, to: &Path) -> io::Result<()> {
    let mut tries = 10;
    loop {
        tries -= 1;
        match std::fs::rename(from, to) {
            Err(err) if tries > 0 && crate::process::is_busy(&err) => {
                thread::sleep(Duration::from_millis(20));
            }
            result => return result,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Project, ProjectId};

    fn store_with_project() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().join("state.json"));
        let mut state = State::default();
        state.projects.push(Project { id: ProjectId("shop".into()), name: "shop".into(), root: "/r".into() });
        state.allocate_task_id();
        store.save(&state).unwrap();
        (dir, store)
    }

    #[test]
    fn round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let empty = Store::new(dir.path().join("state.json")).load().unwrap();
        assert!(empty.missing && empty.state.projects.is_empty());

        let (_dir, store) = store_with_project();
        let loaded = store.load().unwrap();
        assert!(!loaded.missing && loaded.damaged.is_none());
        assert_eq!(loaded.state.projects.len(), 1);
        assert_eq!(loaded.state.next_task_id, 2);
    }

    #[test]
    fn damaged_file_is_restored_from_backup() {
        let (_dir, store) = store_with_project();
        // 断电后常见的样子：空文件，或者只写了一半。
        for broken in ["", "{\"projects\":[", "{}", "[]", "null"] {
            std::fs::write(&store.path, broken).unwrap();
            let loaded = store.load().unwrap();
            assert!(loaded.restored);
            assert_eq!(loaded.state.projects.len(), 1);
            assert_eq!(std::fs::read_to_string(loaded.damaged.unwrap()).unwrap(), broken, "损坏的文件留着");
            assert!(store.load().unwrap().damaged.is_none(), "恢复后的文件是好的");
        }
    }

    #[test]
    fn damaged_file_without_backup_starts_empty() {
        let (_dir, store) = store_with_project();
        std::fs::write(&store.path, "oops").unwrap();
        std::fs::write(store.sibling(".bak"), "oops").unwrap();
        let loaded = store.load().unwrap();
        assert!(!loaded.restored && loaded.state.projects.is_empty());
        assert!(loaded.damaged.unwrap().exists());
    }

    #[test]
    fn reload_never_overwrites_what_it_cannot_read() {
        let (dir, store) = store_with_project();
        std::fs::write(&store.path, "oops").unwrap();
        assert!(store.reload().unwrap().is_none(), "接着用内存里的");
        let kept = std::fs::read_dir(dir.path()).unwrap().flatten().any(|e| {
            e.file_name().to_string_lossy().contains(".damaged-") && std::fs::read(e.path()).unwrap() == b"oops"
        });
        assert!(kept, "损坏的文件留着");
        assert!(store.reload().unwrap().is_none() && store.peek().is_none());
    }

    #[test]
    fn removes_stale_temp_files_only() {
        let (dir, store) = store_with_project();
        let (old, fresh) = (dir.path().join("state.json.1.tmp"), dir.path().join("state.json.2.tmp"));
        for path in [&old, &fresh] {
            std::fs::write(path, "half").unwrap();
        }
        let hour_ago = SystemTime::now() - Duration::from_secs(3600);
        File::options().write(true).open(&old).unwrap().set_modified(hour_ago).unwrap();
        store.load().unwrap();
        assert!(!old.exists() && fresh.exists());
    }

    #[test]
    fn lock_is_exclusive() {
        let (_dir, store) = store_with_project();
        let first = store.lock().unwrap();
        let other = store.clone();
        let waiting = thread::spawn(move || {
            let started = Instant::now();
            other.lock().unwrap();
            started.elapsed()
        });
        thread::sleep(Duration::from_millis(200));
        drop(first);
        assert!(waiting.join().unwrap() >= Duration::from_millis(100), "要等前一个放开");
    }
}
