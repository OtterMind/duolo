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
    /// Duolo 服务器的地址（语音识别、手机配对和转发）。写成空的表示不用服务器：语音输入和手机都用不了。
    pub server: String,
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
            server: DEFAULT_SERVER.into(),
        }
    }
}

/// 默认的 Duolo 服务器（语音识别、手机配对和转发）。
pub const DEFAULT_SERVER: &str = "https://duolo.io";

/// 没有配置时谁写、谁审。
const DEFAULT_WRITER: &str = "claude";
const DEFAULT_REVIEWER: &str = "codex";

/// 默认的审阅提示词。`{base}` 换成基准分支，`{commit}` 换成从它分出来的那个提交，`{language}` 换成界面语言（英文名，例如 `Japanese`），
/// `{changes}` 换成去哪里看改动（[`CHANGES_IN_FILE`] 或 [`CHANGES_IN_GIT`]）。
pub const DEFAULT_REVIEW_PROMPT: &str =
    "Review every change made in this task since it branched off {base} at commit {commit}. {changes} \
Commits made on {base} after that point are not part of this work. \
You are a read-only reviewer: do not modify any files.\n\
Write your review in {language}.\n\
On the first line, write `[PASS]` if nothing must be fixed, or `[ISSUES n]` where n is the number of critical issues, \
followed by a one-sentence summary, e.g. \"[ISSUES 2] 2 critical issues, 3 suggestions\" or \"[PASS] No issues found\".\n\
Then list issues grouped as Critical / Suggestion / Style. For each, give the file and line number and how to fix it.";

/// 后台审阅时，Duolo 把改动写进副本里的这个文件（审阅者不一定能运行命令）。
pub const REVIEW_FILE: &str = "DUOLO_REVIEW.md";

/// 后台审阅：改动在 [`REVIEW_FILE`] 里。
pub const CHANGES_IN_FILE: &str = "The commits, the changed files and the full diff are in the file DUOLO_REVIEW.md \
in the current folder: read it first, then read the changed files where you need more context.";

/// 在终端里审阅：自己用 git 看。
pub const CHANGES_IN_GIT: &str = "Run `git diff {commit}` for committed and uncommitted changes, and \
`git status --short --untracked-files=all` to find new files that are not in git yet (lines starting with `??`), \
then read those files in full.";

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

    /// 填好基准分支、分出来的提交、去哪里看改动和回答语言的审阅提示词。
    /// 自己写的提示词里没有 `{changes}` 时，把它加在最后：审阅者得知道改动在哪里。
    pub fn review_prompt_for(&self, base: &str, commit: &str, language: &str, changes: &str) -> String {
        let prompt = if self.review_prompt.contains("{changes}") {
            self.review_prompt.clone()
        } else {
            format!("{}\n{{changes}}", self.review_prompt)
        };
        prompt
            .replace("{changes}", changes)
            .replace("{base}", base)
            .replace("{commit}", commit)
            .replace("{language}", language)
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
    /// 按编号恢复某一次会话的参数，`{session}` 换成会话编号，例如 `["--resume", "{session}"]`。
    pub resume_session_args: Option<Vec<String>>,
    /// 新开会话时指定会话编号的参数，`{session}` 换成 Duolo 生成的 UUID，例如 `["--session-id", "{session}"]`。
    pub session_id_args: Option<Vec<String>>,
    /// 作为审阅者启动时追加的参数，例如只读沙箱。
    pub review_args: Option<Vec<String>>,
    /// 后台审阅用的整条命令：在任务的 worktree 里只读运行，提示词作为最后一个参数，结论从 stdout 读取。
    pub review_command: Option<Vec<String>>,
    /// 没安装时提示用户运行的安装命令。
    pub install: Option<String>,
}

/// 语音输入配置。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct VoiceConfig {
    /// 是否让服务器把识别结果整理成一条清晰的指令。默认不整理：识别结果已经带标点，整理要多等一会儿。
    pub organize: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_config_keeps_defaults() {
        // 以前的 voice.command（本机识别命令）已经不用了，留在配置里也能读。
        let cfg: Config = toml::from_str("writer = \"codex\"\n[voice]\ncommand = [\"rec.sh\"]\n").unwrap();
        assert_eq!((cfg.writer.0.as_str(), cfg.reviewer.0.as_str()), ("codex", "codex"));
        assert!(!cfg.voice.organize);
        assert!(cfg.namer.is_none());
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
        let prompt = cfg.review_prompt_for("main", "abc123", "Japanese", CHANGES_IN_GIT);
        assert!(prompt.contains("branched off main at commit abc123"));
        assert!(prompt.contains("git diff abc123"));
        assert!(prompt.contains("Write your review in Japanese."));
        let prompt = cfg.review_prompt_for("main", "abc123", "Japanese", CHANGES_IN_FILE);
        assert!(prompt.contains(REVIEW_FILE) && !prompt.contains("git diff"));
        // 自己写的提示词没提改动在哪里：补在最后。
        let custom = Config { review_prompt: "Be strict about {base}.".into(), ..Config::default() };
        assert_eq!(
            custom.review_prompt_for("main", "c1", "English", CHANGES_IN_GIT).lines().last().unwrap(),
            CHANGES_IN_GIT.replace("{commit}", "c1").lines().last().unwrap()
        );
    }

    #[test]
    fn missing_file_is_default() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config::load(&dir.path().join("none.toml")).unwrap();
        assert_eq!(cfg.writer.0, DEFAULT_WRITER);
    }
}
