//! Duolo 在本机使用的目录。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// 环境变量，用来覆盖默认的数据目录（测试和多实例时使用）。
pub const HOME_ENV: &str = "DUOLO_HOME";

/// Duolo 的数据目录布局：
///
/// ```text
/// ~/.duolo/
///   config.toml   用户配置
///   state.json    项目和任务
///   wt/           每个任务的 git worktree
/// ```
#[derive(Debug, Clone)]
pub struct Paths {
    root: PathBuf,
}

impl Paths {
    /// 按 `DUOLO_HOME` 或 `~/.duolo` 定位数据目录。
    pub fn discover() -> Result<Self> {
        if let Some(dir) = std::env::var_os(HOME_ENV) {
            return Ok(Self::at(PathBuf::from(dir)));
        }
        let home = dirs::home_dir().context("无法确定用户主目录")?;
        Ok(Self::at(home.join(".duolo")))
    }

    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn config_file(&self) -> PathBuf {
        self.root.join("config.toml")
    }

    pub fn state_file(&self) -> PathBuf {
        self.root.join("state.json")
    }

    pub fn worktrees(&self) -> PathBuf {
        self.root.join("wt")
    }

    pub fn ensure(&self) -> Result<()> {
        std::fs::create_dir_all(&self.root).with_context(|| format!("无法创建数据目录 {}", self.root.display()))
    }
}
