//! 需要让用户看懂并处理的错误。
//!
//! 每个变体对应界面语言包里的一条 `error.<code>` 文案，参数原样交给界面填进去，
//! 所以界面能用任何语言显示；`Display` 给出英文，用于日志和命令行。
//! 其他内部错误直接用 `anyhow`，界面显示通用文案并附上英文详情。
//!
//! 新增变体时：在这里加上 `code`，并在 `locales/*.json` 的 `error.*` 里补上每种语言的文案。

use std::collections::BTreeMap;

/// 用户可处理的错误。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UserError {
    #[error("{path} is not inside a git repository")]
    NotGitRepo { path: String },
    #[error("git is not installed or not on PATH")]
    GitMissing,
    #[error("git {command} failed: {detail}")]
    GitFailed { command: String, detail: String },
    #[error("cannot start {program}; make sure it is installed and on PATH")]
    CommandNotFound { program: String },
    #[error("voice input needs a Duolo server (server)")]
    VoiceNotConfigured,
    #[error("microphone permission was denied")]
    VoicePermission,
    #[error("no speech was recognized")]
    NothingRecognized,
    #[error("speech recognition failed")]
    VoiceFailed,
    #[error("the server has no speech recognition right now")]
    VoiceUnavailable,
    #[error("today's voice input quota is used up")]
    VoiceQuotaExceeded,
    #[error("cannot reach the Duolo server")]
    VoiceOffline,
    #[error("not recording")]
    NotRecording,
    #[error("review command is not configured (review_command)")]
    ReviewNotConfigured,
    #[error("the agent is not running")]
    AgentNotRunning,
    #[error("this agent cannot run in chat mode")]
    ChatUnsupported,
    #[error("the account is gone")]
    AccountNotFound,
    #[error("the agent is no longer waiting for this answer")]
    PromptChanged,
    #[error("the command arrived too late")]
    CommandExpired,
    #[error("task not found")]
    TaskNotFound,
    #[error("the task is being closed")]
    TaskClosing,
    #[error("project not found")]
    ProjectNotFound,
    #[error("the project still has tasks")]
    ProjectHasTasks,
    #[error("cancelled")]
    Cancelled,
    #[error("no agent named \"{kind}\" in config")]
    UnknownAgent { kind: String },
    #[error("{path} already exists")]
    PathExists { path: String },
    #[error("\"{url}\" is not a git URL")]
    InvalidCloneUrl { url: String },
    #[error("{path} is not a folder")]
    NotAFolder { path: String },
    #[error("this project has no git repository yet")]
    NoRepository,
    #[error("\"{name}\" is not a valid branch name")]
    InvalidBranch { name: String },
    #[error("branch \"{name}\" already exists")]
    BranchExists { name: String },
    #[error("{path} is a separate git repository inside the task and cannot be saved automatically")]
    NestedRepo { path: String },
    #[error("files in {path} are still being changed, so the task was kept")]
    StillChanging { path: String },
    #[error("{path} is being used by another program, so the task was kept")]
    FolderInUse { path: String },
    #[error("could not save uncommitted changes, so the task was kept: {detail}")]
    SaveChangesFailed { detail: String },
    #[error("state.json was damaged and has been restored from its backup; the damaged file is at {path}")]
    StateRestored { path: String },
    #[error("state.json was damaged and has no usable backup; the damaged file is at {path}")]
    StateReset { path: String },
    #[error("{path} is not valid, so defaults are used: {detail}")]
    ConfigInvalid { path: String, detail: String },
    #[error("the agent plugin {path} is not valid and was skipped: {detail}")]
    AgentPluginInvalid { path: String, detail: String },
    #[error("Duolo cannot start: {detail}")]
    CannotStart { detail: String },
}

impl UserError {
    /// 语言包里的键名后缀：`error.<code>`。
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotGitRepo { .. } => "notGitRepo",
            Self::GitMissing => "gitMissing",
            Self::GitFailed { .. } => "gitFailed",
            Self::CommandNotFound { .. } => "commandNotFound",
            Self::VoiceNotConfigured => "voiceNotConfigured",
            Self::VoicePermission => "voicePermission",
            Self::NothingRecognized => "nothingRecognized",
            Self::VoiceFailed => "voiceFailed",
            Self::VoiceUnavailable => "voiceUnavailable",
            Self::VoiceQuotaExceeded => "voiceQuotaExceeded",
            Self::VoiceOffline => "voiceOffline",
            Self::NotRecording => "notRecording",
            Self::ReviewNotConfigured => "reviewNotConfigured",
            Self::AgentNotRunning => "agentNotRunning",
            Self::ChatUnsupported => "chatUnsupported",
            Self::AccountNotFound => "accountNotFound",
            Self::PromptChanged => "promptChanged",
            Self::CommandExpired => "commandExpired",
            Self::TaskNotFound => "taskNotFound",
            Self::TaskClosing => "taskClosing",
            Self::ProjectNotFound => "projectNotFound",
            Self::ProjectHasTasks => "projectHasTasks",
            Self::Cancelled => "cancelled",
            Self::UnknownAgent { .. } => "unknownAgent",
            Self::PathExists { .. } => "pathExists",
            Self::InvalidCloneUrl { .. } => "invalidCloneUrl",
            Self::NotAFolder { .. } => "notAFolder",
            Self::NoRepository => "noRepository",
            Self::InvalidBranch { .. } => "invalidBranch",
            Self::BranchExists { .. } => "branchExists",
            Self::NestedRepo { .. } => "nestedRepo",
            Self::StillChanging { .. } => "stillChanging",
            Self::FolderInUse { .. } => "folderInUse",
            Self::SaveChangesFailed { .. } => "saveChangesFailed",
            Self::StateRestored { .. } => "stateRestored",
            Self::StateReset { .. } => "stateReset",
            Self::ConfigInvalid { .. } => "configInvalid",
            Self::AgentPluginInvalid { .. } => "agentPluginInvalid",
            Self::CannotStart { .. } => "cannotStart",
        }
    }

    /// 当前语言的说明（命令行和终端界面使用；桌面界面用错误码自己翻译）。
    pub fn localized(&self) -> String {
        let params = self.params();
        let vars: Vec<(&str, &dyn std::fmt::Display)> =
            params.iter().map(|(k, v)| (*k, v as &dyn std::fmt::Display)).collect();
        crate::i18n::tf(&format!("error.{}", self.code()), &vars)
    }

    /// 填进文案的参数。
    pub fn params(&self) -> BTreeMap<&'static str, String> {
        let mut params = BTreeMap::new();
        match self {
            Self::NotGitRepo { path } => {
                params.insert("path", path.clone());
            }
            Self::GitFailed { command, detail } => {
                params.insert("command", command.clone());
                params.insert("detail", detail.clone());
            }
            Self::CommandNotFound { program } => {
                params.insert("program", program.clone());
            }
            Self::UnknownAgent { kind } => {
                params.insert("kind", kind.clone());
            }
            Self::PathExists { path }
            | Self::NestedRepo { path }
            | Self::StillChanging { path }
            | Self::FolderInUse { path }
            | Self::StateRestored { path }
            | Self::StateReset { path } => {
                params.insert("path", path.clone());
            }
            Self::SaveChangesFailed { detail } | Self::CannotStart { detail } => {
                params.insert("detail", detail.clone());
            }
            Self::ConfigInvalid { path, detail } | Self::AgentPluginInvalid { path, detail } => {
                params.insert("path", path.clone());
                params.insert("detail", detail.clone());
            }
            Self::InvalidCloneUrl { url } => {
                params.insert("url", url.clone());
            }
            Self::NotAFolder { path } => {
                params.insert("path", path.clone());
            }
            Self::InvalidBranch { name } | Self::BranchExists { name } => {
                params.insert("name", name.clone());
            }
            _ => {}
        }
        params
    }
}

/// 给用户看的错误说明（命令行和终端界面使用）：用户能处理的错误按当前语言显示，
/// 其他内部错误套上通用文案，附英文详情。
pub fn describe(err: &anyhow::Error) -> String {
    match err.chain().find_map(|e| e.downcast_ref::<UserError>()) {
        Some(user) => user.localized(),
        None => crate::i18n::tf("error.internal", &[("message", &format!("{err:#}"))]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_code_has_text_in_every_language() {
        let all = [
            UserError::NotGitRepo { path: "/x".into() },
            UserError::GitMissing,
            UserError::GitFailed { command: "c".into(), detail: "d".into() },
            UserError::CommandNotFound { program: "p".into() },
            UserError::VoiceNotConfigured,
            UserError::VoicePermission,
            UserError::NothingRecognized,
            UserError::VoiceFailed,
            UserError::VoiceUnavailable,
            UserError::VoiceQuotaExceeded,
            UserError::VoiceOffline,
            UserError::NotRecording,
            UserError::ReviewNotConfigured,
            UserError::AgentNotRunning,
            UserError::ChatUnsupported,
            UserError::AccountNotFound,
            UserError::TaskNotFound,
            UserError::TaskClosing,
            UserError::ProjectNotFound,
            UserError::ProjectHasTasks,
            UserError::Cancelled,
            UserError::UnknownAgent { kind: "k".into() },
            UserError::PathExists { path: "/x".into() },
            UserError::InvalidCloneUrl { url: "u".into() },
            UserError::NotAFolder { path: "/x".into() },
            UserError::NoRepository,
            UserError::InvalidBranch { name: "n".into() },
            UserError::BranchExists { name: "n".into() },
            UserError::NestedRepo { path: "/x".into() },
            UserError::StillChanging { path: "/x".into() },
            UserError::FolderInUse { path: "/x".into() },
            UserError::SaveChangesFailed { detail: "d".into() },
            UserError::StateRestored { path: "/x".into() },
            UserError::StateReset { path: "/x".into() },
            UserError::ConfigInvalid { path: "/x".into(), detail: "d".into() },
            UserError::AgentPluginInvalid { path: "/x".into(), detail: "d".into() },
            UserError::CannotStart { detail: "d".into() },
        ];
        for err in all {
            let key = format!("error.{}", err.code());
            for locale in crate::i18n::locales() {
                let text = crate::i18n::translate(locale, &key);
                assert_ne!(text, key, "{locale} has no text for {key}");
                for name in err.params().keys() {
                    assert!(text.contains(&format!("{{{name}}}")), "{locale} {key} lacks {{{name}}}");
                }
            }
        }
    }

    #[test]
    fn code_and_params() {
        let err = UserError::NotGitRepo { path: "/tmp/x".into() };
        assert_eq!(err.code(), "notGitRepo");
        assert_eq!(err.params()["path"], "/tmp/x");
        assert_eq!(err.to_string(), "/tmp/x is not inside a git repository");
    }
}
