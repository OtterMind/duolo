//! `state.json` 的读写。写入先写临时文件再改名，中途崩溃也不会留下半个文件。

use std::path::PathBuf;

use anyhow::{Context, Result};

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
        match std::fs::read(&self.path) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).with_context(|| format!("状态文件损坏：{}", self.path.display()))
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(State::default()),
            Err(err) => Err(err).with_context(|| format!("无法读取状态文件 {}", self.path.display())),
        }
    }

    pub fn save(&self, state: &State) -> Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(state)?;
        std::fs::write(&tmp, bytes).with_context(|| format!("无法写入 {}", tmp.display()))?;
        std::fs::rename(&tmp, &self.path).with_context(|| format!("无法保存 {}", self.path.display()))
    }
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
