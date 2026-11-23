//! 用户配置 `~/.duolo/config.toml`。
//!
//! 所有字段都有默认值，配置文件不存在或只写了一部分也能正常工作。

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::model::AgentKind;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// 默认写代码的 Agent。
    pub writer: AgentKind,
    /// 默认审阅的 Agent。
    pub reviewer: AgentKind,
    /// 对 Agent 插件的修改，或者没有插件、只在这里写一段的 Agent。
    pub agents: BTreeMap<String, AgentOverride>,
    pub voice: VoiceConfig,
    /// 发给审阅 Agent 的提示词，`{base}` 会替换成基准分支，`{commit}` 替换成从它分出来的提交，`{language}` 会替换成界面语言。
    pub review_prompt: String,
    /// 后台审阅用的命令，不管审阅的是哪个 Agent。一般不用设置。
    pub review_command: Vec<String>,
    /// 根据需求给新分支起名的命令，提示词作为最后一个参数，结果从 stdout 读取。
    /// 没写时用装了的 Agent；写成空的表示不用 AI 起名。
    pub namer: Option<Vec<String>>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            writer: AgentKind(DEFAULT_WRITER.into()),
            reviewer: AgentKind(DEFAULT_REVIEWER.into()),
            agents: BTreeMap::new(),
            voice: VoiceConfig::default(),
            review_prompt: DEFAULT_REVIEW_PROMPT.into(),
            review_command: Vec::new(),
            namer: None,
        }
    }
}

/// 没有配置时谁写、谁审。
const DEFAULT_WRITER: &str = "claude";
const DEFAULT_REVIEWER: &str = "codex";

/// 默认的审阅提示词。`{base}` 换成基准分支，`{commit}` 换成从它分出来的那个提交，`{language}` 换成界面语言（英文名，例如 `Japanese`）。
pub const DEFAULT_REVIEW_PROMPT: &str =
    "Review every change made in this git worktree since it branched off {base} at commit {commit}. \
Run `git diff {commit}` for committed and uncommitted changes, and `git status --short --untracked-files=all` to find \
new files that are not in git yet (lines starting with `??`), then read those files in full. \
Commits made on {base} after that point are not part of this work. \
You are a read-only reviewer: do not modify any files.\n\
Write your review in {language}.\n\
On the first line, write `[PASS]` if nothing must be fixed, or `[ISSUES n]` where n is the number of critical issues, \
followed by a one-sentence summary, e.g. \"[ISSUES 2] 2 critical issues, 3 suggestions\" or \"[PASS] No issues found\".\n\
Then list issues grouped as Critical / Suggestion / Style. For each, give the file and line number and how to fix it.";

/// 没有指定界面语言时，审阅结论使用的语言。
pub const DEFAULT_REPLY_LANGUAGE: &str = "English";

impl Config {
    /// 读取配置；文件不存在时返回默认值。
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).with_context(|| format!("invalid config file: {}", path.display())),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => Err(err).with_context(|| format!("cannot read config file {}", path.display())),
        }
    }

    /// 填好基准分支、分出来的提交和回答语言的审阅提示词。
    pub fn review_prompt_for(&self, base: &str, commit: &str, language: &str) -> String {
        self.review_prompt.replace("{base}", base).replace("{commit}", commit).replace("{language}", language)
    }
}

/// config.toml 里的 `[agents.<id>]`：在插件的声明上修改，没写的沿用插件的。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentOverride {
    /// 可执行文件，例如 `claude` 或完整路径。
    pub command: Option<String>,
    /// 每次启动都带上的参数。
    pub args: Option<Vec<String>>,
    /// 恢复上一次会话时追加的参数，例如 `--continue`。
    pub resume_args: Option<Vec<String>>,
    /// 作为审阅者启动时追加的参数，例如只读沙箱。
    pub review_args: Option<Vec<String>>,
    /// 后台审阅用的整条命令：在任务的 worktree 里只读运行，提示词作为最后一个参数，结论从 stdout 读取。
    pub review_command: Option<Vec<String>>,
    /// 没安装时提示用户运行的安装命令。
    pub install: Option<String>,
}

/// 语音输入配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct VoiceConfig {
    /// 录音并转写的命令。Duolo 启动它开始录音，结束时通知它停止：macOS / Linux 发送 SIGINT，
    /// Windows 往标准输入写一行 `stop`。命令收到后停止录音、把识别出的文字打印到 stdout 并退出。
    /// 为空表示未配置。
    pub command: Vec<String>,
    /// 是否把识别结果整理成一条清晰的指令。
    pub organize: bool,
    /// 整理用的命令，整理提示词作为最后一个参数传入，结果从 stdout 读取。没写时用装了的 Agent。
    pub organizer: Option<Vec<String>>,
    /// 整理最长等待秒数，超时就直接使用原话。
    pub organize_timeout_secs: u64,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self { command: Vec::new(), organize: true, organizer: None, organize_timeout_secs: 30 }
    }
}

impl VoiceConfig {
    pub fn is_configured(&self) -> bool {
        !self.command.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_config_keeps_defaults() {
        let cfg: Config = toml::from_str("writer = \"codex\"\n[voice]\ncommand = [\"rec.sh\"]\n").unwrap();
        assert_eq!((cfg.writer.0.as_str(), cfg.reviewer.0.as_str()), ("codex", "codex"));
        assert!(cfg.voice.is_configured());
        assert!(cfg.voice.organize);
        assert!(cfg.namer.is_none() && cfg.voice.organizer.is_none());
    }

    #[test]
    fn empty_is_different_from_missing() {
        let cfg: Config = toml::from_str("namer = []\n[agents.claude]\nresume_args = []\n").unwrap();
        assert_eq!(cfg.namer, Some(Vec::new()));
        let claude = &cfg.agents["claude"];
        assert_eq!(claude.resume_args, Some(Vec::new()));
        assert!(claude.command.is_none() && claude.args.is_none());
    }

    #[test]
    fn review_prompt_mentions_base() {
        let cfg = Config::default();
        let prompt = cfg.review_prompt_for("main", "abc123", "Japanese");
        assert!(prompt.contains("branched off main at commit abc123"));
        assert!(prompt.contains("git diff abc123"));
        assert!(prompt.contains("Write your review in Japanese."));
    }

    #[test]
    fn missing_file_is_default() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config::load(&dir.path().join("none.toml")).unwrap();
        assert_eq!(cfg.writer.0, DEFAULT_WRITER);
    }
}
