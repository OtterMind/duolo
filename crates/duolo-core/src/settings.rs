//! 界面里修改的偏好设置，保存在 `~/.duolo/settings.json`。
//!
//! 和 `config.toml` 分开：`config.toml` 由用户手写，程序只读不写；这里的内容由界面修改。

use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::store::{load_json, save_json};

/// 跟随系统语言。
pub const LANGUAGE_AUTO: &str = "auto";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// 界面语言：`auto` 跟随系统，或者语言代码（`en`、`zh-CN`、`ja` 等，见 `app/src/i18n`）。
    pub language: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self { language: LANGUAGE_AUTO.into() }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Result<Self> {
        load_json(path)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        save_json(path, self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_auto_and_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        assert_eq!(Settings::load(&path).unwrap().language, LANGUAGE_AUTO);
        Settings { language: "ja".into() }.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap().language, "ja");
    }
}
