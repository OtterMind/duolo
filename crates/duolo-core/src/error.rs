//! 需要让用户看懂并处理的错误。
//!
//! 每个变体对应界面语言包里的一条 `error.<code>` 文案，参数原样交给界面填进去，
//! 所以界面能用任何语言显示；`Display` 给出英文，用于日志和命令行。
//! 其他内部错误直接用 `anyhow`，界面显示通用文案并附上英文详情。
//!
//! 新增变体时：在这里加上 `code`，并在 `app/src/i18n/locales/*.ts` 的 `error.*` 里补上每种语言的文案。

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
    #[error("voice input is not configured (voice.command)")]
    VoiceNotConfigured,
    #[error("microphone or speech recognition permission was denied")]
    VoicePermission,
    #[error("no speech was recognized")]
    NothingRecognized,
    #[error("not recording")]
    NotRecording,
    #[error("review command is not configured (review_command)")]
    ReviewNotConfigured,
    #[error("the agent is not running")]
    AgentNotRunning,
    #[error("task not found")]
    TaskNotFound,
    #[error("project not found")]
    ProjectNotFound,
    #[error("no agent named \"{kind}\" in config")]
    UnknownAgent { kind: String },
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
            Self::NotRecording => "notRecording",
            Self::ReviewNotConfigured => "reviewNotConfigured",
            Self::AgentNotRunning => "agentNotRunning",
            Self::TaskNotFound => "taskNotFound",
            Self::ProjectNotFound => "projectNotFound",
            Self::UnknownAgent { .. } => "unknownAgent",
        }
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
            _ => {}
        }
        params
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_and_params() {
        let err = UserError::NotGitRepo { path: "/tmp/x".into() };
        assert_eq!(err.code(), "notGitRepo");
        assert_eq!(err.params()["path"], "/tmp/x");
        assert_eq!(err.to_string(), "/tmp/x is not inside a git repository");
    }
}
