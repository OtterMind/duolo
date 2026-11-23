//! 插件的声明 `agent.toml`。

use std::path::{Component, Path};

use anyhow::{anyhow, bail, ensure, Result};
use serde::Deserialize;

use crate::transcript;

/// 这个版本认识的声明格式。
pub const SCHEMA: u32 = 1;

/// `resume_session_args` 里换成会话编号的占位符。
pub(crate) const SESSION: &str = "{session}";
/// `hooks.args` 里换成设置文件路径的占位符。
pub(crate) const SETTINGS: &str = "{settings}";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// 声明格式的版本。
    pub schema: u32,
    /// 唯一的名字，保存在任务里，也是 config.toml 里 `[agents.<id>]` 的名字。
    pub id: String,
    /// 界面上显示的名字。
    pub name: String,
    /// 可执行文件，例如 `claude`。
    pub command: String,
    /// 图标的颜色；没有时跟随文字颜色。
    #[serde(default)]
    pub icon_color: Option<String>,
    /// 最低能用的版本（`--version` 打印的）。更旧的不能选，环境检查里提示升级；读不出版本时照样用。
    #[serde(default)]
    pub minimum_version: Option<String>,
    #[serde(default)]
    pub install: Install,
    #[serde(default)]
    pub start: StartSpec,
    /// 后台审阅：在任务的 worktree 里只读运行，结论从 stdout 读取。没有时不能用来审阅。
    #[serde(default)]
    pub review: Option<HeadlessSpec>,
    /// 起分支名、整理口述这样的小事：快、便宜，不读写文件。
    #[serde(default)]
    pub quick: Option<HeadlessSpec>,
    #[serde(default)]
    pub screen: Screen,
    #[serde(default)]
    pub keys: Keys,
    #[serde(default)]
    pub transcript: Option<TranscriptSpec>,
    #[serde(default)]
    pub trust: Option<TrustSpec>,
    #[serde(default)]
    pub hooks: Option<HooksSpec>,
    #[serde(default)]
    pub session_list: Option<SessionListSpec>,
    #[serde(default)]
    pub env: EnvSpec,
}

/// 没安装时提示用户运行的命令，各系统不同时分开写。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Install {
    pub default: String,
    pub macos: Option<String>,
    pub windows: Option<String>,
    pub linux: Option<String>,
}

impl Install {
    /// 当前系统的安装命令。
    pub fn command(&self) -> &str {
        let own = if cfg!(target_os = "macos") {
            &self.macos
        } else if cfg!(windows) {
            &self.windows
        } else {
            &self.linux
        };
        own.as_deref().unwrap_or(&self.default)
    }
}

/// 第一句话怎么交给它。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PromptVia {
    /// 最后一个参数。
    #[default]
    Argument,
    /// `prompt_flag` 后面，例如 `--prompt=<第一句话>`。
    Flag,
    /// 等它准备好之后粘贴进输入框。
    Paste,
}

/// 在终端里启动。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StartSpec {
    /// 每次启动都带上的参数。config.toml 里的 `args` 会换掉它们。
    pub args: Vec<String>,
    /// 每次启动都带上、config.toml 换不掉的参数：关掉启动时的升级提示这类，换掉了会出问题的。
    pub always_args: Vec<String>,
    pub prompt: PromptVia,
    /// `prompt = "flag"` 时的参数，和第一句话拼成一个参数。
    pub prompt_flag: String,
    /// 放在第一句话前面的参数（一般是 `--`），这样它以 `-` 开头也不会被当成选项。
    pub separator: Option<String>,
    /// 恢复这个目录里上一次会话的参数。
    pub resume_args: Vec<String>,
    /// 按编号恢复某一次会话的参数，`{session}` 换成会话编号（从会话记录里读到的）。
    /// 没写或者还不知道编号时用 `resume_args`。
    pub resume_session_args: Vec<String>,
    /// 新开会话时指定会话编号的参数，`{session}` 换成 Duolo 生成的 UUID。写了就不用从会话记录里认编号。
    pub session_id_args: Vec<String>,
    /// 作为审阅者启动时追加的参数，例如只读沙箱。
    pub review_args: Vec<String>,
    /// 启动时设置的环境变量，例如关掉启动时的升级提示（在那个界面按回车会选"现在升级"）。
    pub env: std::collections::BTreeMap<String, String>,
}

/// 运行一次就退出的命令。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HeadlessSpec {
    pub args: Vec<String>,
    /// 放在提示词前面的参数，同 [`StartSpec::separator`]。
    pub separator: Option<String>,
    /// 提示词改从标准输入给时追加的参数（Windows 的批处理文件带不了换行）。
    pub stdin_args: Vec<String>,
    /// 只留下允许的功能，其余开着的都关掉。程序以后新加的功能也就默认关着。
    pub features: Option<FeaturesSpec>,
    /// 运行时设置的环境变量（例如临时定义一个只读的 agent）。
    pub env: std::collections::BTreeMap<String, String>,
}

/// 见 `duolo_core::process::FeatureGate`。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeaturesSpec {
    /// 列出功能的参数，跟在命令后面。输出每行 `<功能名> … <true|false>`。
    pub list: Vec<String>,
    #[serde(default)]
    pub allow: Vec<String>,
    /// 关掉一个功能的参数，`{feature}` 换成功能名。
    pub disable: Vec<String>,
}

/// 怎么从画面认出它的状态，统一按小写比较。
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Screen {
    /// 画面底部出现这些文字：在等用户确认。
    pub approval: Vec<String>,
    /// 画面底部出现这些文字：正在工作，即使暂时没有新输出。
    pub busy: Vec<String>,
    /// 出现这些文字：停在一个要用户选的列表上，这时不能发消息。带编号、有光标的列表不用写，都认得。
    pub choosing: Vec<String>,
}

impl Default for Screen {
    /// 没有声明时按最常见的写法认。
    fn default() -> Self {
        let words = |list: &[&str]| list.iter().map(|s| s.to_string()).collect();
        Self {
            approval: words(&["1. yes", "do you want to proceed", "enter to confirm"]),
            busy: words(&["esc to interrupt", "esc to cancel"]),
            choosing: words(&["enter to confirm"]),
        }
    }
}

/// 发给它的按键。
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Keys {
    /// 打断正在做的事。
    pub interrupt: String,
    /// 在输入框里换行。
    pub newline: String,
}

impl Default for Keys {
    fn default() -> Self {
        Self { interrupt: "\x1b".into(), newline: "\x1b\r".into() }
    }
}

/// 它自己写的会话记录在哪里、是什么格式。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptSpec {
    /// 用哪段代码读，见 `transcript` 模块。
    pub format: String,
    /// 数据目录，相对用户的家目录。
    pub home: String,
    /// 设置了这个环境变量时，用它作为数据目录。
    #[serde(default)]
    pub home_env: Option<String>,
}

/// 会话不是一个个文件（例如存在数据库里）时，用它自己的命令列出会话，认出这次的会话编号。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionListSpec {
    /// 列出会话的参数，跟在命令后面，在任务的目录里运行；输出一个 JSON 数组。
    pub args: Vec<String>,
    /// 每一项里会话编号、工作目录、创建时间（Unix 毫秒）的字段名。
    pub id: String,
    pub directory: String,
    pub created: String,
}

/// 钩子：它在会话开始、请求授权这些时候运行 Duolo 给的命令（见 `hooks` 模块）。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HooksSpec {
    /// 用哪段代码拼设置、读事件，见 `hooks` 模块。
    pub format: String,
    /// 把设置文件交给它的参数，`{settings}` 换成文件路径。
    pub args: Vec<String>,
    /// 要的事件和最早支持它的版本。装的版本更旧、或者读不出版本时，不装这个事件。
    pub events: std::collections::BTreeMap<String, String>,
}

/// 它把"信任这个文件夹"记在哪里，Duolo 启动它之前替用户记下（见 `trust` 模块）。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustSpec {
    /// 用哪段代码写，见 `trust` 模块。
    pub format: String,
    /// 配置文件，相对用户的家目录。
    pub file: String,
    /// 设置了这个环境变量时，配置文件是它下面的 `env_file`。
    #[serde(default)]
    pub env: Option<String>,
    #[serde(default)]
    pub env_file: Option<String>,
    /// 信任哪个目录：`worktree` 是任务的目录（关任务时删掉这一项），`repo` 是主仓库。
    pub scope: TrustScope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrustScope {
    Worktree,
    Repo,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EnvSpec {
    /// 启动任何 Agent 时都不传下去的环境变量（它给自己的子进程设置的标记）。
    pub remove: Vec<String>,
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Self> {
        // 先看版本：格式升级后多出来的字段不该报成"不认识的字段"。
        #[derive(Deserialize)]
        struct Version {
            schema: u32,
        }
        let Version { schema } = toml::from_str(text).map_err(|err| brief(text, &err))?;
        ensure!(schema <= SCHEMA, "schema {schema} needs a newer version of Duolo");
        let manifest: Self = toml::from_str(text).map_err(|err| brief(text, &err))?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// 没有声明、只在 config.toml 里写了 `[agents.<id>]` 的 Agent。
    pub fn plain(id: &str) -> Self {
        Self {
            schema: SCHEMA,
            id: id.into(),
            name: duolo_core::AgentKind(id.into()).fallback_name(),
            command: id.into(),
            icon_color: None,
            minimum_version: None,
            install: Install::default(),
            start: StartSpec::default(),
            review: None,
            quick: None,
            screen: Screen::default(),
            keys: Keys::default(),
            transcript: None,
            trust: None,
            hooks: None,
            session_list: None,
            env: EnvSpec::default(),
        }
    }

    fn validate(&self) -> Result<()> {
        let id_ok = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_';
        ensure!(!self.id.is_empty() && self.id.chars().all(id_ok), "invalid id {:?}: use a-z, 0-9, - and _", self.id);
        ensure!(!self.name.trim().is_empty(), "name is empty");
        ensure!(!self.command.trim().is_empty(), "command is empty");
        if self.start.prompt == PromptVia::Flag {
            ensure!(!self.start.prompt_flag.is_empty(), "prompt = \"flag\" needs prompt_flag");
        }
        for (name, args) in
            [("resume_session_args", &self.start.resume_session_args), ("session_id_args", &self.start.session_id_args)]
        {
            ensure!(args.is_empty() || args.iter().any(|a| a.contains(SESSION)), "{name} must contain {SESSION}");
        }
        for features in [&self.review, &self.quick].into_iter().flatten().filter_map(|h| h.features.as_ref()) {
            ensure!(!features.list.is_empty(), "features.list is empty");
            ensure!(
                features.disable.iter().any(|a| a.contains("{feature}")),
                "features.disable must contain {{feature}}"
            );
        }
        if let Some(minimum) = &self.minimum_version {
            ensure!(duolo_core::text::parse_version(minimum).is_some(), "minimum_version must look like 1.2.3");
        }
        if let Some(hooks) = &self.hooks {
            ensure!(crate::hooks::format(&hooks.format).is_some(), "unknown hooks format {:?}", hooks.format);
            ensure!(hooks.args.iter().any(|a| a.contains(SETTINGS)), "hooks.args must contain {SETTINGS}");
            for (event, since) in &hooks.events {
                ensure!(
                    duolo_core::text::parse_version(since).is_some(),
                    "hooks.events.{event} must be a version like 1.2.3"
                );
            }
        }
        if let Some(trust) = &self.trust {
            ensure!(crate::trust::store(&trust.format).is_some(), "unknown trust format {:?}", trust.format);
            let inside = |file: &str| {
                !file.is_empty() && Path::new(file).components().all(|c| matches!(c, Component::Normal(_)))
            };
            ensure!(inside(&trust.file), "trust file must be a path inside the home directory");
            ensure!(trust.env_file.as_deref().is_none_or(inside), "trust env_file must be a relative path");
        }
        if let Some(color) = &self.icon_color {
            let hex = color.strip_prefix('#').unwrap_or_default();
            let ok = matches!(hex.len(), 3 | 6) && hex.chars().all(|c| c.is_ascii_hexdigit());
            ensure!(ok, "icon_color must look like #d97757");
        }
        if let Some(transcript) = &self.transcript {
            if transcript::format(&transcript.format).is_none() {
                bail!("unknown transcript format {:?}", transcript.format);
            }
            let inside = Path::new(&transcript.home).components().all(|c| matches!(c, Component::Normal(_)));
            ensure!(inside && !transcript.home.is_empty(), "transcript home must be a path inside the home directory");
        }
        Ok(())
    }
}

/// 哪一行错了、错在哪里，一行说完（toml 自己的写法带着几行代码，弹窗里对不齐）。
fn brief(text: &str, err: &toml::de::Error) -> anyhow::Error {
    match err.span().and_then(|span| text.get(..span.start)) {
        Some(before) => anyhow!("line {}: {}", before.matches('\n').count() + 1, err.message()),
        None => anyhow!("{}", err.message()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = "schema = 1\nid = \"aider\"\nname = \"Aider\"\ncommand = \"aider\"\n";

    #[test]
    fn minimal_manifest_uses_common_defaults() {
        let m = Manifest::parse(MINIMAL).unwrap();
        assert_eq!(m.start.prompt, PromptVia::Argument);
        assert_eq!(m.keys.interrupt, "\x1b");
        assert!(m.screen.approval.iter().any(|w| w == "1. yes"));
        assert!(m.review.is_none() && m.transcript.is_none());
    }

    #[test]
    fn mistakes_are_reported() {
        let fails = |text: &str, want: &str| {
            let err = format!("{:#}", Manifest::parse(text).unwrap_err());
            assert!(err.contains(want), "{err:?} lacks {want:?}");
        };
        fails(&format!("{MINIMAL}comand = \"x\"\n"), "line 5: unknown field `comand`");
        fails(&format!("{MINIMAL}[start]\nresume = []\n"), "resume");
        fails(&MINIMAL.replace("id = \"aider\"", "id = \"../aider\""), "invalid id");
        fails(&MINIMAL.replace("command = \"aider\"", "command = \" \""), "command is empty");
        fails(&format!("{MINIMAL}[start]\nprompt = \"flag\"\n"), "prompt_flag");
        fails(&format!("{MINIMAL}[start]\nresume_session_args = [\"-r\"]\n"), "must contain {session}");
        fails(&format!("{MINIMAL}[start]\nsession_id_args = [\"--id\"]\n"), "session_id_args must contain");
        fails(&format!("{MINIMAL}icon_color = \"red;}}\"\n"), "icon_color");
        fails(&format!("{MINIMAL}[transcript]\nformat = \"nope\"\nhome = \".x\"\n"), "transcript format");
        fails(&format!("{MINIMAL}[transcript]\nformat = \"claude\"\nhome = \"../x\"\n"), "home directory");
        // 更新的格式：说需要新版本，而不是说有不认识的字段。
        fails(&format!("{}future = 1\n", MINIMAL.replace("schema = 1", "schema = 2")), "newer version");
    }
}
