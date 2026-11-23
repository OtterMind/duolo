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
///   agents/       用户自己装的 Agent 插件
///   wt/           每个任务的 git worktree
///   review/       审阅时的临时副本，审阅完就删
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
        let home = dirs::home_dir().context("cannot determine the home directory")?;
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

    /// 界面里修改的偏好设置（语言等）。用户手写的配置在 `config.toml`。
    pub fn settings_file(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    pub fn state_file(&self) -> PathBuf {
        self.root.join("state.json")
    }

    /// 用户自己装的 Agent 插件：`agents/<id>/agent.toml`。
    pub fn agents(&self) -> PathBuf {
        self.root.join("agents")
    }

    pub fn worktrees(&self) -> PathBuf {
        self.root.join("wt")
    }

    /// 审阅时的临时副本：`review/<开始时间>-<任务>-<序号>`。
    pub fn reviews(&self) -> PathBuf {
        self.root.join("review")
    }

    /// 每个任务的事件流：`events/<任务编号>.jsonl`，一行一个事件。
    pub fn events(&self) -> PathBuf {
        self.root.join("events")
    }

    pub fn ensure(&self) -> Result<()> {
        std::fs::create_dir_all(&self.root)
            .with_context(|| format!("cannot create data directory {}", self.root.display()))
    }
}
