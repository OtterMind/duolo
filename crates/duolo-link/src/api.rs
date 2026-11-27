//! 调服务器的 HTTP 接口（server/src/app.ts）：登录、登记设备、配对码、设备列表。

use std::time::Duration;

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::json;

/// 一次请求最长等多久。
const TIMEOUT: Duration = Duration::from_secs(10);

/// 服务器拒绝时的原因，调用方据此决定重新登录还是重新登记。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiError {
    /// 令牌无效（过期、服务器换了库）。
    Unauthorized,
    /// 令牌有效，但这次登录还没有登记设备。
    NotRegistered,
    /// 其他状态码。
    Status(u16),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unauthorized => f.write_str("the server rejected the sign-in"),
            Self::NotRegistered => f.write_str("this computer is not registered on the server"),
            Self::Status(code) => write!(f, "the server answered {code}"),
        }
    }
}

impl std::error::Error for ApiError {}

#[derive(Debug, Clone)]
pub struct Api {
    server: String,
    agent: ureq::Agent,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    pub name: String,
    pub platform: String,
    #[serde(default)]
    pub online: bool,
    #[serde(default)]
    pub public_key: String,
}

impl Api {
    pub fn new(server: &str) -> Self {
        let agent =
            ureq::Agent::config_builder().timeout_global(Some(TIMEOUT)).http_status_as_error(false).build().into();
        Self { server: server.trim_end_matches('/').to_string(), agent }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.server)
    }

    /// 领一个匿名身份，返回令牌。
    pub fn sign_in_anonymous(&self) -> Result<String> {
        #[derive(Deserialize)]
        struct SignedIn {
            token: String,
        }
        let mut res = self.agent.post(self.url("/v1/auth/sign-in/anonymous")).send_json(json!({}))?;
        check(res.status().as_u16())?;
        let signed: SignedIn = res.body_mut().read_json().context("unexpected sign-in response")?;
        Ok(signed.token)
    }

    /// 登记这台电脑（同一个登录会话再登记就是更新），返回设备编号。
    pub fn register(&self, token: &str, name: &str, platform: &str, public_key: &str) -> Result<String> {
        #[derive(Deserialize)]
        struct Registered {
            device: Device,
        }
        let mut res = self
            .agent
            .post(self.url("/v1/devices"))
            .header("authorization", bearer(token))
            .send_json(json!({ "name": name, "platform": platform, "publicKey": public_key }))?;
        check(res.status().as_u16())?;
        let registered: Registered = res.body_mut().read_json().context("unexpected response")?;
        Ok(registered.device.id)
    }

    /// 账号下的所有设备（带公钥），和这台电脑自己的编号。
    pub fn devices(&self, token: &str) -> Result<(String, Vec<Device>)> {
        #[derive(Deserialize)]
        struct Devices {
            me: String,
            devices: Vec<Device>,
        }
        let mut res = self.agent.get(self.url("/v1/devices")).header("authorization", bearer(token)).call()?;
        check(res.status().as_u16())?;
        let list: Devices = res.body_mut().read_json().context("unexpected response")?;
        Ok((list.me, list.devices))
    }

    /// 生成配对码（十分钟内有效，只能用一次）。
    pub fn create_pairing(&self, token: &str) -> Result<String> {
        #[derive(Deserialize)]
        struct Pairing {
            code: String,
        }
        let mut res = self.agent.post(self.url("/v1/pairings")).header("authorization", bearer(token)).send_empty()?;
        check(res.status().as_u16())?;
        let pairing: Pairing = res.body_mut().read_json().context("unexpected response")?;
        Ok(pairing.code)
    }

    /// 解除一台设备：它的登录随之失效。
    pub fn remove_device(&self, token: &str, id: &str) -> Result<()> {
        let res =
            self.agent.delete(self.url(&format!("/v1/devices/{id}"))).header("authorization", bearer(token)).call()?;
        match res.status().as_u16() {
            // 已经不在了也算成功。
            404 => Ok(()),
            code => check(code),
        }
    }
}

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

fn check(status: u16) -> Result<()> {
    match status {
        200..=299 => Ok(()),
        401 => Err(ApiError::Unauthorized.into()),
        409 => Err(ApiError::NotRegistered.into()),
        code => Err(ApiError::Status(code).into()),
    }
}

/// 出错是不是因为服务器拒绝了（而不是连不上）。
pub fn api_error(err: &anyhow::Error) -> Option<&ApiError> {
    err.downcast_ref::<ApiError>()
}
