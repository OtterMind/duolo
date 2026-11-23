//! 插件的声明 `agent.toml`。

use std::path::{Component, Path};

use anyhow::{anyhow, bail, ensure, Result};
use serde::Deserialize;

use crate::transcript;

/// 这个版本认识的声明格式。
pub const SCHEMA: u32 = 1;

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
    /// 每次启动都带上的参数。
    pub args: Vec<String>,
    pub prompt: PromptVia,
    /// `prompt = "flag"` 时的参数，和第一句话拼成一个参数。
    pub prompt_flag: String,
    /// 放在第一句话前面的参数（一般是 `--`），这样它以 `-` 开头也不会被当成选项。
    pub separator: Option<String>,
    /// 恢复这个目录里上一次会话的参数。
    pub resume_args: Vec<String>,
    /// 作为审阅者启动时追加的参数，例如只读沙箱。
    pub review_args: Vec<String>,
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
            install: Install::default(),
            start: StartSpec::default(),
            review: None,
            quick: None,
            screen: Screen::default(),
            keys: Keys::default(),
            transcript: None,
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
        fails(&format!("{MINIMAL}icon_color = \"red;}}\"\n"), "icon_color");
        fails(&format!("{MINIMAL}[transcript]\nformat = \"nope\"\nhome = \".x\"\n"), "transcript format");
        fails(&format!("{MINIMAL}[transcript]\nformat = \"claude\"\nhome = \"../x\"\n"), "home directory");
        // 更新的格式：说需要新版本，而不是说有不认识的字段。
        fails(&format!("{}future = 1\n", MINIMAL.replace("schema = 1", "schema = 2")), "newer version");
    }
}
