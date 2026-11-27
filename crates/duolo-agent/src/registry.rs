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
builtin!("claude", "codex", "opencode", "pi", "grok");

/// 复制很便宜，可以带到别的线程用。
#[derive(Debug, Clone, Default)]
pub struct Registry {
    plugins: Vec<Arc<Plugin>>,
    /// 内置的终端（用户自己的 shell）。能按 `shell` 找到、启动，但不是 AI，不在 [`Registry::iter`] 里：
    /// 不出现在选 Agent、环境检查、审阅这些地方。
    shell: Option<Arc<Plugin>>,
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
        // 官方安装脚本放命令的目录不一定在 PATH 里（脚本不一定改 shell 配置），也去那里找。
        duolo_core::process::add_search_dirs(found.iter().flat_map(|p| p.bin_dirs()));
        (found, problems)
    }

    pub(crate) fn builtin() -> Self {
        let mut registry = Self { shell: Some(Arc::new(Plugin::new(Manifest::shell(), None))), ..Self::default() };
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
        // config.toml 里的 `[agents.shell]` 改的是内置的终端（例如换一个 shell），它照旧不算 Agent。
        if plugin.id() == AgentKind::SHELL {
            self.shell = Some(plugin);
            return;
        }
        match self.plugins.iter_mut().find(|p| p.id() == plugin.id()) {
            Some(slot) => *slot = plugin,
            None => self.plugins.push(plugin),
        }
    }

    fn find(&self, id: &str) -> Option<&Arc<Plugin>> {
        if id == AgentKind::SHELL {
            return self.shell.as_ref();
        }
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
        if kind.is_shell() {
            return duolo_core::t("agent.shell");
        }
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

    /// 内置插件，按登记的顺序。
    const BUILTIN: [&str; 5] = ["claude", "codex", "opencode", "pi", "grok"];

    const AIDER: &str = "schema = 1\nid = \"aider\"\nname = \"Aider\"\ncommand = \"aider\"\n";

    /// 内置的终端能按 `shell` 找到、启动，但不算 Agent；config.toml 能换它的命令。
    #[cfg(unix)]
    #[test]
    fn the_shell_is_found_but_not_listed() {
        let dir = tempfile::tempdir().unwrap();
        let (registry, _) = Registry::load(&Config::default(), &dir.path().join("none"));
        assert!(!ids(&registry).contains(&"shell"));
        let shell = registry.get(&AgentKind::shell()).unwrap();
        let start = |prompt| crate::Start {
            role: duolo_core::AgentRole::Writer,
            resume: false,
            session: None,
            hooks: None,
            prompt,
            instructions: None,
        };
        // 总是一个能打字的 shell，命令等它准备好再打进去。
        let launch = shell.launch(start(Some("pnpm dev")));
        assert_eq!(
            (launch.args.as_slice(), launch.paste.as_deref()),
            (["-l".to_string()].as_slice(), Some("pnpm dev"))
        );
        assert_eq!(shell.launch(start(None)).args, ["-l"]);

        let (registry, _) =
            Registry::load(&config("[agents.shell]\ncommand = \"/bin/bash\"\n"), &dir.path().join("none"));
        assert!(!ids(&registry).contains(&"shell"));
        assert_eq!(registry.get(&AgentKind::shell()).unwrap().command(), "/bin/bash");
    }

    #[test]
    fn builtin_plugins_come_first() {
        let dir = tempfile::tempdir().unwrap();
        let (registry, problems) = Registry::load(&Config::default(), &dir.path().join("none"));
        assert_eq!(ids(&registry), BUILTIN);
        assert!(problems.is_empty());
    }

    #[test]
    fn user_plugins_and_config_are_added() {
        let dir = tempfile::tempdir().unwrap();
        install(dir.path(), "aider", AIDER);
        std::fs::create_dir(dir.path().join("not-a-plugin")).unwrap();
        let config = config("[agents.gemini]\ncommand = \"gemini\"\n[agents.aider]\nargs = [\"--dark\"]\n");
        let (registry, problems) = Registry::load(&config, dir.path());
        assert_eq!(ids(&registry), [&BUILTIN[..], &["aider", "gemini"]].concat());
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
        assert_eq!(ids(&registry), [&BUILTIN[..], &["aider"]].concat());
        assert_eq!(problems.len(), 2);
        assert!(problems.iter().all(|p| p.code() == "agentPluginInvalid"), "{problems:?}");
    }

    #[test]
    fn user_plugin_can_replace_a_builtin_one() {
        let dir = tempfile::tempdir().unwrap();
        install(dir.path(), "codex", "schema = 1\nid = \"codex\"\nname = \"My Codex\"\ncommand = \"my-codex\"\n");
        let (registry, _) = Registry::load(&Config::default(), dir.path());
        assert_eq!(ids(&registry), BUILTIN, "换掉内置的，不是多一个");
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
