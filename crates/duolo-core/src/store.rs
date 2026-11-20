//! `state.json` 等 JSON 文件的读写。写入先写临时文件再改名，中途崩溃也不会留下半个文件。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::model::State;

#[derive(Debug, Clone)]
pub struct Store {
    path: PathBuf,
}

impl Store {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn load(&self) -> Result<State> {
        load_json(&self.path)
    }

    pub fn save(&self, state: &State) -> Result<()> {
        save_json(&self.path, state)
    }
}

/// 读取 JSON 文件；文件不存在时返回默认值。
pub fn load_json<T: DeserializeOwned + Default>(path: &Path) -> Result<T> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).with_context(|| format!("file is corrupted: {}", path.display())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(err) => Err(err).with_context(|| format!("cannot read {}", path.display())),
    }
}

/// 原子地写入 JSON 文件。
pub fn save_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(value)?;
    std::fs::write(&tmp, bytes).with_context(|| format!("cannot write {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("cannot save {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Project, ProjectId};

    #[test]
    fn round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().join("state.json"));
        assert!(store.load().unwrap().projects.is_empty());

        let mut state = State::default();
        state.projects.push(Project { id: ProjectId("shop".into()), name: "shop".into(), root: "/r".into() });
        state.allocate_task_id();
        store.save(&state).unwrap();

        let loaded = store.load().unwrap();
        assert_eq!(loaded.projects.len(), 1);
        assert_eq!(loaded.next_task_id, 2);
    }
}
