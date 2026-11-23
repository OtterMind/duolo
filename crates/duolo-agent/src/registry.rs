//! 可用的 Agent：内置的、用户装在 `~/.duolo/agents/` 的，以及只在 config.toml 里写了一段的。

use std::path::Path;
use std::sync::Arc;

use anyhow::{ensure, Context, Result};
use duolo_core::process::OneShot;
use duolo_core::{AgentKind, Config, UserError};

use crate::manifest::Manifest;
use crate::plugin::Plugin;

/// 内置插件。界面按这个顺序列出；起名、整理这样的小事用第一个装了的，所以快的在前面。
macro_rules! builtin {
    ($($id:literal),* $(,)?) => {
        const BUILTIN: &[(&str, &str, &str)] = &[$((
            $id,
            include_str!(concat!("../agents/", $id, "/agent.toml")),
            include_str!(concat!("../agents/", $id, "/icon.svg")),
        )),*];
    };
}
builtin!("claude", "codex");

/// 复制很便宜，可以带到别的线程用。
#[derive(Debug, Clone, Default)]
pub struct Registry {
    plugins: Vec<Arc<Plugin>>,
}

impl Registry {
    /// 内置插件，加上 `dir`（`~/.duolo/agents`）里的（同名的换掉内置的），再套用 `config` 里的修改。
    /// 写错的插件跳过，第二项是要告诉用户的问题。
    ///
    /// 只从用户自己的目录读，不从项目里读：插件能指定要运行的命令，打开别人的仓库不能就运行它的命令。
    pub fn load(config: &Config, dir: &Path) -> (Self, Vec<UserError>) {
        let mut problems = Vec::new();
        let mut found = Self::builtin();
        for path in plugin_dirs(dir) {
            match read(&path) {
                Ok(plugin) => found.put(plugin),
                Err(err) => problems.push(UserError::AgentPluginInvalid {
                    path: path.join("agent.toml").display().to_string(),
                    detail: format!("{err:#}").trim().to_string(),
                }),
            }
        }
        for (id, changes) in &config.agents {
            let base = match found.find(id) {
                Some(plugin) => plugin.as_ref().clone(),
                None => Plugin::new(Manifest::plain(id), None),
            };
            found.put(base.with(changes));
        }
        (found, problems)
    }

    pub(crate) fn builtin() -> Self {
        let mut registry = Self::default();
        for (id, manifest, icon) in BUILTIN {
            match Manifest::parse(manifest) {
                Ok(manifest) => registry.put(Plugin::new(manifest, Some((*icon).to_string()))),
                // 内置的写错了测试会发现，见 tests/conformance.rs。
                Err(err) => debug_assert!(false, "built-in agent {id}: {err:#}"),
            }
        }
        registry
    }

    fn put(&mut self, plugin: Plugin) {
        let plugin = Arc::new(plugin);
        match self.plugins.iter_mut().find(|p| p.id() == plugin.id()) {
            Some(slot) => *slot = plugin,
            None => self.plugins.push(plugin),
        }
    }

    fn find(&self, id: &str) -> Option<&Arc<Plugin>> {
        self.plugins.iter().find(|p| p.id() == id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Plugin> {
        self.plugins.iter().map(Arc::as_ref)
    }

    pub fn get(&self, kind: &AgentKind) -> Result<&Plugin, UserError> {
        self.find(&kind.0).map(Arc::as_ref).ok_or_else(|| UserError::UnknownAgent { kind: kind.0.clone() })
    }

    /// 界面上显示的名字；插件已经不在了（任务是以前建的）时用它的 id。
    pub fn name(&self, kind: &AgentKind) -> String {
        self.find(&kind.0).map_or_else(|| kind.fallback_name(), |p| p.name().to_string())
    }

    /// 启动任何 Agent 时都不传下去的环境变量。
    pub fn env_remove(&self) -> Vec<String> {
        self.iter().flat_map(|p| p.env_remove().iter().cloned()).collect()
    }

    /// 后台审阅用的命令。
    pub fn review(&self, kind: &AgentKind) -> Result<OneShot> {
        let command = self.get(kind)?.review().ok_or(UserError::ReviewNotConfigured)?;
        Ok(self.clean(command))
    }

    /// 做小事用的命令：第一个装了的。和谁写代码无关，要的是快，内置的按快慢排。
    pub fn quick(&self) -> Option<OneShot> {
        let command = self.iter().filter(|p| p.is_installed()).find_map(Plugin::quick)?;
        Some(self.clean(command))
    }

    /// 去掉不该传给 Agent 的环境变量。
    pub fn clean(&self, mut command: OneShot) -> OneShot {
        command.env_remove = self.env_remove();
        command
    }
}

/// `dir` 里有 `agent.toml` 的文件夹，按名字排序。
fn plugin_dirs(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut dirs: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("agent.toml").is_file())
        .collect();
    dirs.sort();
    dirs
}

fn read(dir: &Path) -> Result<Plugin> {
    let text = std::fs::read_to_string(dir.join("agent.toml")).context("cannot read agent.toml")?;
    let manifest = Manifest::parse(&text)?;
    let name = dir.file_name().unwrap_or_default().to_string_lossy();
    ensure!(manifest.id == name, "id is {:?} but the folder is {name:?}", manifest.id);
    Ok(Plugin::new(manifest, std::fs::read_to_string(dir.join("icon.svg")).ok()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(text: &str) -> Config {
        toml::from_str(text).unwrap()
    }

    fn install(dir: &Path, name: &str, manifest: &str) {
        std::fs::create_dir_all(dir.join(name)).unwrap();
        std::fs::write(dir.join(name).join("agent.toml"), manifest).unwrap();
    }

    fn ids(registry: &Registry) -> Vec<&str> {
        registry.iter().map(Plugin::id).collect()
    }

    const AIDER: &str = "schema = 1\nid = \"aider\"\nname = \"Aider\"\ncommand = \"aider\"\n";

    #[test]
    fn builtin_plugins_come_first() {
        let dir = tempfile::tempdir().unwrap();
        let (registry, problems) = Registry::load(&Config::default(), &dir.path().join("none"));
        assert_eq!(ids(&registry), ["claude", "codex"]);
        assert!(problems.is_empty());
    }

    #[test]
    fn user_plugins_and_config_are_added() {
        let dir = tempfile::tempdir().unwrap();
        install(dir.path(), "aider", AIDER);
        std::fs::create_dir(dir.path().join("not-a-plugin")).unwrap();
        let config = config("[agents.gemini]\ncommand = \"gemini\"\n[agents.aider]\nargs = [\"--dark\"]\n");
        let (registry, problems) = Registry::load(&config, dir.path());
        assert_eq!(ids(&registry), ["claude", "codex", "aider", "gemini"]);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(registry.name(&AgentKind("gemini".into())), "Gemini");
        assert_eq!(registry.name(&AgentKind("removed-one".into())), "Removed-one");
        assert!(matches!(registry.get(&AgentKind("nope".into())), Err(UserError::UnknownAgent { .. })));
    }

    #[test]
    fn broken_plugins_are_skipped_and_reported() {
        let dir = tempfile::tempdir().unwrap();
        install(dir.path(), "aider", AIDER);
        install(dir.path(), "broken", "schema = 1\nid = ");
        install(dir.path(), "other-name", AIDER);
        let (registry, problems) = Registry::load(&Config::default(), dir.path());
        assert_eq!(ids(&registry), ["claude", "codex", "aider"]);
        assert_eq!(problems.len(), 2);
        assert!(problems.iter().all(|p| p.code() == "agentPluginInvalid"), "{problems:?}");
    }

    #[test]
    fn user_plugin_can_replace_a_builtin_one() {
        let dir = tempfile::tempdir().unwrap();
        install(dir.path(), "codex", "schema = 1\nid = \"codex\"\nname = \"My Codex\"\ncommand = \"my-codex\"\n");
        let (registry, _) = Registry::load(&Config::default(), dir.path());
        assert_eq!(ids(&registry), ["claude", "codex"]);
        assert_eq!(registry.name(&AgentKind("codex".into())), "My Codex");
    }

    #[test]
    fn small_jobs_use_an_installed_agent() {
        let dir = tempfile::tempdir().unwrap();
        // `sh` 一定装了，其他两个换成不存在的命令。
        let config = config(
            "[agents.claude]\ncommand = \"duolo-no-such-command\"\n[agents.codex]\ncommand = \"sh\"\n\
             [agents.plain]\ncommand = \"sh\"\n",
        );
        let (registry, _) = Registry::load(&config, dir.path());
        let quick = registry.quick().unwrap();
        assert_eq!(quick.argv[0], "sh", "没装的、做不了小事的都跳过");
        assert!(quick.env_remove.iter().any(|v| v == "CLAUDECODE"));
    }
}
