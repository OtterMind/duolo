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
    /// 语音识别的语言：`auto` 跟随界面语言，或者语言代码。
    pub voice_language: String,
    /// 说完后是否整理成指令；没有设置过时用 `config.toml` 里的 `voice.organize`。
    pub voice_organize: Option<bool>,
}

impl Default for Settings {
    fn default() -> Self {
        Self { language: LANGUAGE_AUTO.into(), voice_language: LANGUAGE_AUTO.into(), voice_organize: None }
    }
}

impl Settings {
    /// 读取；文件损坏时按默认值（只是偏好，下次修改时会重新写好）。
    pub fn load(path: &Path) -> Result<Self> {
        match load_json(path) {
            Err(err) if err.chain().any(|e| e.is::<serde_json::Error>()) => Ok(Self::default()),
            result => result,
        }
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
        Settings { language: "ja".into(), ..Settings::default() }.save(&path).unwrap();
        let loaded = Settings::load(&path).unwrap();
        assert_eq!(loaded.language, "ja");
        assert_eq!(loaded.voice_language, LANGUAGE_AUTO);
        assert_eq!(loaded.voice_organize, None);
    }

    #[test]
    fn damaged_file_is_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "{\"language\":").unwrap();
        assert_eq!(Settings::load(&path).unwrap(), Settings::default());
    }
}
