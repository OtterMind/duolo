//! 开始一次语音输入：拿到在 Duolo 服务器上的身份（没有就领一个匿名的），带上任务的提示连上服务器。

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use duolo_core::{Account, Paths, UserError};

use crate::cloud::{CloudError, CloudRequest, Recorder};
use crate::hints::Hints;

/// 领匿名身份的请求最长等多久。
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(5);

/// 开始录音。连服务器要等一会儿，请在后台线程调用。
/// 服务器没配置、连不上、拒绝时返回对应的 [`UserError`]。
pub fn start(paths: &Paths, server: &str, hints: Hints) -> Result<Recorder> {
    if server.trim().is_empty() {
        return Err(UserError::VoiceNotConfigured.into());
    }
    let mut request = CloudRequest {
        server: server.to_string(),
        token: String::new(),
        hotwords: hints.hotwords,
        context: hints.context,
    };
    let file = paths.account_file();
    let saved = Account::load(&file);
    let result = match saved.token_for(server) {
        Some(token) => {
            request.token = token.to_string();
            match Recorder::start(&request) {
                // 凭据过期或服务器换了库：匿名的重新领一个再试一次。
                Err(err) if is_unauthorized(&err) && saved.anonymous => sign_in(&file, server).and_then(|token| {
                    request.token = token;
                    Recorder::start(&request)
                }),
                result => result,
            }
        }
        None => sign_in(&file, server).and_then(|token| {
            request.token = token;
            Recorder::start(&request)
        }),
    };
    result.map_err(user_error)
}

fn is_unauthorized(err: &anyhow::Error) -> bool {
    err.downcast_ref::<CloudError>() == Some(&CloudError::Unauthorized)
}

/// 把开始时的错误换成用户看得懂的。
fn user_error(err: anyhow::Error) -> anyhow::Error {
    if err.downcast_ref::<UserError>().is_some() {
        return err;
    }
    eprintln!("voice: {err:#}");
    let user = match err.downcast_ref::<CloudError>() {
        Some(CloudError::Refused(code)) if code == "voiceUnavailable" => UserError::VoiceUnavailable,
        Some(CloudError::Refused(code)) if code == "voiceQuotaExceeded" => UserError::VoiceQuotaExceeded,
        Some(_) => UserError::VoiceFailed,
        None => UserError::VoiceOffline,
    };
    user.into()
}

/// 领一个匿名身份，存起来。
fn sign_in(file: &Path, server: &str) -> Result<String> {
    #[derive(serde::Deserialize)]
    struct SignedIn {
        token: String,
    }
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(SIGN_IN_TIMEOUT)).build().into();
    let url = format!("{}/v1/auth/sign-in/anonymous", server.trim_end_matches('/'));
    let signed: SignedIn = agent
        .post(&url)
        .send_json(serde_json::json!({}))
        .context("cannot sign in to the Duolo server")?
        .body_mut()
        .read_json()
        .context("unexpected sign-in response")?;
    Account { server: server.to_string(), token: signed.token.clone(), anonymous: true }.save(file)?;
    Ok(signed.token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needs_a_server() {
        let dir = tempfile::tempdir().unwrap();
        let err = start(&Paths::at(dir.path()), "", Hints::default()).unwrap_err();
        assert_eq!(err.downcast_ref::<UserError>(), Some(&UserError::VoiceNotConfigured));
    }

    #[test]
    fn unreachable_server_is_offline() {
        let dir = tempfile::tempdir().unwrap();
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let err = start(&Paths::at(dir.path()), &format!("http://127.0.0.1:{port}"), Hints::default()).unwrap_err();
        assert_eq!(err.downcast_ref::<UserError>(), Some(&UserError::VoiceOffline));
    }
}
