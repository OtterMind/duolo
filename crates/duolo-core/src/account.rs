//! 在 Duolo 服务器上的登录凭据，保存在 `~/.duolo/account.json`（只有自己能读）。
//!
//! 没登录过时，第一次用到服务器（例如语音识别）会自动领一个匿名身份；以后用邮箱登录时换成正式账号。

use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::store::{load_json, save_json};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Account {
    /// 凭据属于哪个服务器；换了服务器就作废。
    pub server: String,
    /// `Authorization: Bearer` 用的令牌。
    pub token: String,
    /// 匿名身份（没有用邮箱登录）。
    pub anonymous: bool,
}

impl Account {
    /// 读取；文件损坏时当作没登录（重新领一个就好）。
    pub fn load(path: &Path) -> Self {
        load_json(path).unwrap_or_default()
    }

    /// 这个服务器上的令牌。
    pub fn token_for(&self, server: &str) -> Option<&str> {
        (self.server == server && !self.token.is_empty()).then_some(self.token.as_str())
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        save_json(path, self)?;
        // 令牌等同于密码：只让自己读写。
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_belongs_to_its_server() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("account.json");
        assert_eq!(Account::load(&path), Account::default());
        let account = Account { server: "https://a.example".into(), token: "t".into(), anonymous: true };
        account.save(&path).unwrap();
        let loaded = Account::load(&path);
        assert_eq!(loaded.token_for("https://a.example"), Some("t"));
        assert_eq!(loaded.token_for("https://b.example"), None);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
    }
}
