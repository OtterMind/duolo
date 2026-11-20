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
    /// 每种 Agent 怎么启动。
    pub agents: BTreeMap<String, AgentProfile>,
    pub voice: VoiceConfig,
    /// 发给审阅 Agent 的提示词，`{base}` 会替换成基准分支，`{language}` 会替换成界面语言。
    pub review_prompt: String,
    /// 后台审阅用的命令：在任务的 worktree 里运行，提示词作为最后一个参数，结论从 stdout 读取。
    pub review_command: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        let mut agents = BTreeMap::new();
        agents.insert(
            "claude".into(),
            AgentProfile {
                command: "claude".into(),
                args: Vec::new(),
                resume_args: vec!["--continue".into()],
                review_args: Vec::new(),
            },
        );
        agents.insert(
            "codex".into(),
            AgentProfile {
                command: "codex".into(),
                args: Vec::new(),
                resume_args: vec!["resume".into(), "--last".into()],
                review_args: vec!["--sandbox".into(), "read-only".into()],
            },
        );
        Self {
            writer: AgentKind::claude(),
            reviewer: AgentKind::codex(),
            agents,
            voice: VoiceConfig::default(),
            review_prompt: DEFAULT_REVIEW_PROMPT.into(),
            review_command: ["codex", "exec", "--sandbox", "read-only", "--skip-git-repo-check", "--color", "never"]
                .map(String::from)
                .to_vec(),
        }
    }
}

/// 默认的审阅提示词。`{base}` 换成基准分支，`{language}` 换成界面语言（英文名，例如 `Japanese`）。
pub const DEFAULT_REVIEW_PROMPT: &str =
    "Review every change in this git worktree relative to the {base} branch, including uncommitted changes \
(you can use `git diff {base}`). You are a read-only reviewer: do not modify any files.\n\
Write your review in {language}.\n\
On the first line, summarize the result in one short sentence, e.g. \"2 critical issues, 3 suggestions\" or \"No issues found\".\n\
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

    pub fn profile(&self, kind: &AgentKind) -> Option<&AgentProfile> {
        self.agents.get(&kind.0)
    }

    /// 填好基准分支和回答语言的审阅提示词。
    pub fn review_prompt_for(&self, base: &str, language: &str) -> String {
        self.review_prompt.replace("{base}", base).replace("{language}", language)
    }
}

/// 如何启动某种 Agent 的 CLI。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct AgentProfile {
    /// 可执行文件，例如 `claude`。
    pub command: String,
    /// 每次启动都带上的参数。
    pub args: Vec<String>,
    /// 恢复上一次会话时追加的参数，例如 `--continue`。
    pub resume_args: Vec<String>,
    /// 作为审阅者启动时追加的参数，例如只读沙箱。
    pub review_args: Vec<String>,
}

/// 语音输入配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct VoiceConfig {
    /// 录音并转写的命令。Duolo 启动它开始录音，结束时发送 SIGINT，
    /// 命令应在收到 SIGINT 后停止录音、把识别出的文字打印到 stdout 并退出。
    /// 为空表示未配置。
    pub command: Vec<String>,
    /// 是否把识别结果整理成一条清晰的指令。
    pub organize: bool,
    /// 整理用的命令，整理提示词作为最后一个参数传入，结果从 stdout 读取。
    pub organizer: Vec<String>,
    /// 整理最长等待秒数，超时就直接使用原话。
    pub organize_timeout_secs: u64,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            command: Vec::new(),
            organize: true,
            organizer: vec!["claude".into(), "-p".into(), "--model".into(), "haiku".into()],
            organize_timeout_secs: 30,
        }
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
        assert_eq!(cfg.writer, AgentKind::codex());
        assert_eq!(cfg.reviewer, AgentKind::codex());
        assert!(cfg.voice.is_configured());
        assert!(cfg.voice.organize);
        assert!(cfg.profile(&AgentKind::claude()).is_some());
    }

    #[test]
    fn review_prompt_mentions_base() {
        let cfg = Config::default();
        let prompt = cfg.review_prompt_for("main", "Japanese");
        assert!(prompt.contains("git diff main"));
        assert!(prompt.contains("Write your review in Japanese."));
    }

    #[test]
    fn missing_file_is_default() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config::load(&dir.path().join("none.toml")).unwrap();
        assert_eq!(cfg.writer, AgentKind::claude());
    }
}
