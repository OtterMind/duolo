//! 连接要记住的东西，存在 `~/.duolo/link.json`（只有自己能读：里面有私钥和配对密码）。

use std::path::Path;

use anyhow::Result;
use duolo_core::store::{load_json, save_json};
use duolo_core::CommandResult;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Saved {
    /// 下面的设备和手机属于哪个服务器；换了服务器就重新登记。
    pub server: String,
    /// 这台电脑的私钥（base64）。公钥登记在服务器上，手机用它加密。
    pub key: String,
    /// 在服务器上的设备编号。
    pub device: String,
    /// 配对好的手机。
    pub phones: Vec<PhoneRecord>,
    /// 给出去、还没有手机用上的配对密码。
    pub pairings: Vec<OpenPairing>,
    /// 执行过的命令：同一个编号只执行一次，再来就回同样的结果。
    pub done: Vec<DoneCommand>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PhoneRecord {
    /// 手机在服务器上的设备编号。
    pub id: String,
    pub name: String,
    pub platform: String,
    /// 手机的公钥（base64）。
    pub key: String,
    /// 配对时的密码（base64），和两边的公钥一起算出密钥。
    pub secret: String,
    /// Unix 毫秒。
    pub paired_at: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct OpenPairing {
    pub secret: String,
    /// Unix 毫秒，过了这个时间服务器上的配对码也失效了。
    pub expires_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DoneCommand {
    pub id: String,
    /// 执行的时间（Unix 毫秒）。
    pub at: u64,
    pub result: CommandResult,
}

/// 执行过的命令编号至少记这么久（比服务器离线信箱的 7 天多一天）。
pub const DONE_TTL_MS: u64 = 8 * 24 * 60 * 60 * 1000;

impl Saved {
    /// 读取；文件损坏时从头来（重新配对就好）。
    pub fn load(path: &Path) -> Self {
        load_json(path).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        save_json(path, self)?;
        // 有私钥和配对密码：只让自己读写。
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    /// 去掉过期的配对密码和太旧的命令记录。
    pub fn prune(&mut self, now: u64) {
        self.pairings.retain(|p| p.expires_at > now);
        self.done.retain(|d| now.saturating_sub(d.at) <= DONE_TTL_MS);
    }

    pub fn result_of(&self, id: &str) -> Option<&CommandResult> {
        self.done.iter().find(|d| d.id == id).map(|d| &d.result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_privately_and_prunes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("link.json");
        assert_eq!(Saved::load(&path), Saved::default());
        let mut saved = Saved {
            pairings: vec![
                OpenPairing { secret: "old".into(), expires_at: DONE_TTL_MS + 100 },
                OpenPairing { secret: "new".into(), expires_at: DONE_TTL_MS + 300 },
            ],
            done: vec![
                DoneCommand { id: "a".into(), at: 0, result: CommandResult::default() },
                DoneCommand { id: "b".into(), at: DONE_TTL_MS, result: CommandResult::default() },
            ],
            ..Saved::default()
        };
        saved.prune(DONE_TTL_MS + 200);
        assert_eq!(saved.pairings.len(), 1);
        assert_eq!(saved.result_of("b"), Some(&CommandResult::default()));
        assert_eq!(saved.result_of("a"), None);
        saved.save(&path).unwrap();
        assert_eq!(Saved::load(&path), saved);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
    }
}
