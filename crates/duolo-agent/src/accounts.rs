//! 一种 Agent 的多个账号（参考 Orca）：每个账号是一个文件夹，启动时用插件声明的环境变量（`[account].home_env`，
//! 例如 `CLAUDE_CONFIG_DIR`）指过去。只用文件夹，不读、不复制里面的令牌。
//!
//! 账号从哪里来：默认的（不设这个变量）；在 Duolo 里添加的（`~/.duolo/accounts/<插件 id>/<编号>`，
//! 对话记录等链接到默认文件夹，换了账号还能接着以前的对话）；别的软件里登录好的（`[account].import`）。

use std::path::{Path, PathBuf};

use crate::manifest::AppSpec;

/// 一个账号。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    /// 它的文件夹；默认账号为 None。
    pub dir: Option<PathBuf>,
    pub source: AccountSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountSource {
    Default,
    /// 在 Duolo 里添加的。
    Duolo,
    /// 别的软件里登录好的（软件名）。
    App(String),
}

impl Account {
    pub fn default_account() -> Self {
        Self { dir: None, source: AccountSource::Default }
    }

    /// 标识：文件夹的路径，默认账号是空字符串（`settings.json` 里记的就是这个）。
    pub fn id(&self) -> String {
        self.dir.as_ref().map(|d| d.display().to_string()).unwrap_or_default()
    }
}

/// 别的软件里的账号文件夹：路径里的一段 `*` 是每个账号一个的文件夹，按名字排序。
pub(crate) fn imported(spec: &AppSpec, expand: impl Fn(&str) -> PathBuf) -> Vec<Account> {
    let mut found = Vec::new();
    for pattern in spec.paths() {
        let Some((parent, rest)) = pattern.split_once('*') else {
            let dir = expand(pattern);
            if dir.is_dir() {
                found.push(dir);
            }
            continue;
        };
        let parent = expand(parent);
        let rest = rest.trim_start_matches(['/', '\\']);
        let Ok(entries) = std::fs::read_dir(&parent) else { continue };
        let mut dirs: Vec<PathBuf> =
            entries.flatten().map(|e| e.path().join(rest)).filter(|dir| dir.is_dir()).collect();
        dirs.sort();
        found.extend(dirs);
    }
    found.into_iter().map(|dir| Account { dir: Some(dir), source: AccountSource::App(spec.name.clone()) }).collect()
}

/// 在 Duolo 里添加的账号：`root` 下每个文件夹一个，按名字（添加的先后）排序。
pub(crate) fn own(root: &Path) -> Vec<Account> {
    let Ok(entries) = std::fs::read_dir(root) else { return Vec::new() };
    let mut dirs: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    dirs.into_iter().map(|dir| Account { dir: Some(dir), source: AccountSource::Duolo }).collect()
}

/// 准备新账号的文件夹：`share` 里的（对话记录等）链接到默认文件夹 `home` 里的同名文件，换了账号还能接着以前的对话。
/// 默认文件夹里还没有的不链接（它会在新账号的文件夹里自己建）。Windows 上建链接要额外的权限，不链接。
pub(crate) fn prepare(dir: &Path, home: &Path, share: &[String]) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    for name in share {
        let (target, link) = (home.join(name), dir.join(name));
        if target.exists() && std::fs::symlink_metadata(&link).is_err() {
            std::os::unix::fs::symlink(&target, &link)?;
        }
    }
    #[cfg(not(unix))]
    let _ = (home, share);
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn finds_accounts_and_shares_history() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        for id in ["b", "a"] {
            std::fs::create_dir_all(root.join("app/profiles").join(id).join("home")).unwrap();
        }
        std::fs::create_dir_all(root.join("app/profiles/broken")).unwrap();
        let spec = AppSpec {
            name: "App".into(),
            macos: vec!["{root}/app/profiles/*/home".into()],
            linux: vec!["{root}/app/profiles/*/home".into()],
            windows: Vec::new(),
        };
        let expand = |p: &str| PathBuf::from(p.replace("{root}", &root.display().to_string()));
        let found = imported(&spec, expand);
        let dirs: Vec<_> = found.iter().map(|a| a.dir.clone().unwrap()).collect();
        assert_eq!(dirs, [root.join("app/profiles/a/home"), root.join("app/profiles/b/home")]);
        assert_eq!(found[0].source, AccountSource::App("App".into()));

        let home = root.join("home");
        std::fs::create_dir_all(home.join("projects")).unwrap();
        let dir = root.join("own/1");
        prepare(&dir, &home, &["projects".into(), "missing".into()]).unwrap();
        assert_eq!(std::fs::read_link(dir.join("projects")).unwrap(), home.join("projects"));
        assert!(!dir.join("missing").exists());
        // 再准备一次不出错。
        prepare(&dir, &home, &["projects".into()]).unwrap();
        assert_eq!(own(&root.join("own")), [Account { dir: Some(dir), source: AccountSource::Duolo }]);
    }
}
