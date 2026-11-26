//! 检查更新：Duolo 自己（GitHub 上发布的版本）和各个 Agent（npm 上的最新版本）。
//! 要联网，最长几秒，不要拿着引擎的锁调用。

use std::time::Duration;

use anyhow::{Context, Result};
use duolo_agent::Registry;
use duolo_core::text::is_older;
use duolo_core::AgentKind;

/// Duolo 最新发布的版本。
const RELEASES: &str = "https://api.github.com/repos/OtterMind/duolo/releases/latest";
/// npm 上包的最新版本：`{registry}/{包名}/latest`。
const NPM: &str = "https://registry.npmjs.org";
const TIMEOUT: Duration = Duration::from_secs(8);

/// 发布了的版本。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    /// 下载页。
    pub url: String,
}

/// Duolo 有没有比 `current` 新的版本。还没发布过任何版本时是 `Ok(None)`；网络不通时是 `Err`。
pub fn duolo_update(current: &str) -> Result<Option<Release>> {
    #[derive(serde::Deserialize)]
    struct Latest {
        tag_name: String,
        html_url: String,
    }
    let latest: Latest = match agent().get(RELEASES).header("Accept", "application/vnd.github+json").call() {
        Ok(mut response) => response.body_mut().read_json().context("unexpected release response")?,
        Err(ureq::Error::StatusCode(404)) => return Ok(None),
        Err(err) => return Err(err).context("cannot check for updates"),
    };
    let version = latest.tag_name.trim_start_matches('v').to_string();
    Ok(is_older(current, &version).then_some(Release { version, url: latest.html_url }))
}

/// 各个 Agent 在 npm 上的最新版本（插件写了 `[install].npm` 的才查），同时查。查不到的不在结果里。
pub fn agent_versions(agents: &Registry) -> Vec<(AgentKind, String)> {
    let packages: Vec<(AgentKind, String)> =
        agents.iter().filter_map(|p| Some((p.kind(), p.npm_package()?.to_string()))).collect();
    std::thread::scope(|scope| {
        let jobs: Vec<_> =
            packages.iter().map(|(kind, package)| (kind, scope.spawn(move || npm_latest(package)))).collect();
        jobs.into_iter().filter_map(|(kind, job)| Some((kind.clone(), job.join().ok()?.ok()?))).collect()
    })
}

fn npm_latest(package: &str) -> Result<String> {
    #[derive(serde::Deserialize)]
    struct Latest {
        version: String,
    }
    let latest: Latest = agent().get(&format!("{NPM}/{package}/latest")).call()?.body_mut().read_json()?;
    Ok(latest.version)
}

fn agent() -> ureq::Agent {
    // GitHub 的接口要求带 User-Agent。
    ureq::Agent::config_builder().timeout_global(Some(TIMEOUT)).user_agent("duolo").build().into()
}
