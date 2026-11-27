//! 电脑和手机之间的连接（约定见 docs/mobile.md，服务器见 server/）。
//!
//! - 这台电脑在服务器上有一个身份（和语音识别共用 `~/.duolo/account.json`，没有就领一个匿名的）
//!   和一台登记过的设备（X25519 公钥）。
//! - 配对：生成配对码，连同电脑的公钥和一个随机密码做成二维码；手机扫码后用配对码登录同一个账号，
//!   第一条消息用这个密码加密，电脑逐个试还没用掉的密码认出它。
//! - 后台线程连着服务器的 WebSocket（断了自动重连），收手机的消息、回结果、推新的总览和事件。
//!   只推给在线的手机：离线的上线后自己会来要。
//!
//! 和引擎之间只通过 [`Host`]：桌面应用提供总览、事件和执行命令。

mod api;
pub mod crypto;
mod desk;
mod relay;
mod saved;

use std::collections::{HashMap, HashSet};
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use duolo_core::wire::{DesktopMessage, Overview};
use duolo_core::{Account, Paths, TaskEvent};
use serde::Serialize;

pub use api::ApiError;
pub use desk::Host;

use crate::api::{api_error, Api};
use crate::crypto::{Channel, KeyPair};
use crate::saved::{OpenPairing, PhoneRecord, Saved};

/// 配对码在服务器上的有效期（和 server/src/app.ts 一致）。
const PAIRING_TTL_MS: u64 = 10 * 60 * 1000;

/// 这台电脑的信息，登记到服务器上。
#[derive(Debug, Clone)]
pub struct Options {
    pub paths: Paths,
    /// 服务器地址；空的表示不连。
    pub server: String,
    /// 设备名（例如"张三的 MacBook Pro"），手机上显示。
    pub name: String,
    /// `mac`、`windows` 或 `linux`。
    pub platform: String,
}

/// 连接现在怎么样，设置里显示。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum Status {
    /// 没有配置服务器。
    Off,
    Connecting,
    Online,
    /// 连不上，过一会儿再试。`error` 是错误码（`error.*` 文案）。
    Offline {
        error: String,
    },
}

/// 配好对的一台手机。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhoneView {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub online: bool,
    /// Unix 毫秒。
    pub paired_at: u64,
}

/// 给手机扫的配对二维码。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pairing {
    /// `duolo://pair?...`
    pub link: String,
    /// 二维码（SVG）。
    pub svg: String,
    /// Unix 毫秒。
    pub expires_at: u64,
}

/// 和服务器、手机之间的连接。克隆的都是同一个。
#[derive(Clone)]
pub struct Link {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for Link {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Link").field("server", &self.inner.options.server).finish_non_exhaustive()
    }
}

struct Inner {
    options: Options,
    host: Arc<dyn Host>,
    shared: Mutex<Shared>,
}

struct Shared {
    saved: Saved,
    key: KeyPair,
    /// 和每台手机之间的密钥（从保存的公钥和密码算出来）。
    channels: HashMap<String, Channel>,
    token: Option<String>,
    online: HashSet<String>,
    status: Status,
    /// 连着服务器时，往 WebSocket 里写的队列。
    out: Option<mpsc::Sender<String>>,
    /// 最近一次发出的总览（不算 `rev`），变了才加 `rev`。
    overview: Option<Overview>,
    rev: u64,
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

impl Link {
    /// 读保存的身份，后台连服务器。没有配置服务器时什么也不做。
    pub fn start(options: Options, host: Arc<dyn Host>) -> Self {
        let file = options.paths.link_file();
        let mut saved = Saved::load(&file);
        let server = options.server.trim_end_matches('/').to_string();
        // 换了服务器：之前的设备和手机都在旧服务器上。
        if saved.server != server {
            saved = Saved { server: server.clone(), key: saved.key, ..Saved::default() };
        }
        let key = KeyPair::from_base64(&saved.key).unwrap_or_else(|_| KeyPair::generate());
        saved.key = key.secret_base64();
        saved.prune(now_ms());
        let channels = channels_of(&key, &saved.phones);
        let status = if server.is_empty() { Status::Off } else { Status::Connecting };
        let shared = Shared {
            saved,
            key,
            channels,
            token: None,
            online: HashSet::new(),
            status,
            out: None,
            overview: None,
            rev: now_ms(),
        };
        let link = Self {
            inner: Arc::new(Inner { options: Options { server, ..options }, host, shared: Mutex::new(shared) }),
        };
        if let Err(err) = link.save() {
            eprintln!("link: cannot save: {err:#}");
        }
        if !link.inner.options.server.is_empty() {
            relay::spawn(link.clone());
        }
        link
    }

    fn shared(&self) -> MutexGuard<'_, Shared> {
        self.inner.shared.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn save(&self) -> Result<()> {
        let saved = self.shared().saved.clone();
        saved.save(&self.inner.options.paths.link_file())
    }

    fn api(&self) -> Api {
        Api::new(&self.inner.options.server)
    }

    pub fn status(&self) -> Status {
        self.shared().status.clone()
    }

    fn set_status(&self, status: Status) {
        self.shared().status = status;
    }

    /// 有在线的手机：值得算总览推过去。
    pub fn wants_updates(&self) -> bool {
        let shared = self.shared();
        shared.out.is_some() && shared.saved.phones.iter().any(|p| shared.online.contains(&p.id))
    }

    /// 电脑上有了变化：新的总览和事件推给在线的手机。
    pub fn publish(&self, overview: Overview, events: &[TaskEvent]) {
        let changed = self.stamp(overview);
        let mut by_task: Vec<(duolo_core::TaskId, Vec<TaskEvent>)> = Vec::new();
        for event in events {
            match by_task.iter_mut().find(|(task, _)| *task == event.task) {
                Some((_, list)) => list.push(event.clone()),
                None => by_task.push((event.task, vec![event.clone()])),
            }
        }
        let online: Vec<String> = {
            let shared = self.shared();
            shared.saved.phones.iter().filter(|p| shared.online.contains(&p.id)).map(|p| p.id.clone()).collect()
        };
        for phone in online {
            for (task, list) in &by_task {
                let latest = list.last().map_or(0, |e| e.seq);
                self.send_to(&phone, desk::events_page(*task, list.clone(), latest));
            }
            if let Some(overview) = &changed {
                self.send_to(&phone, DesktopMessage::Overview(overview.clone()));
            }
        }
    }

    /// 给总览填上 `rev`：和上次发的不一样才加（重启后从当前时间接着往上走）。变了时返回它。
    fn stamp(&self, mut overview: Overview) -> Option<Overview> {
        let mut shared = self.shared();
        overview.rev = 0;
        if shared.overview.as_ref() == Some(&overview) {
            return None;
        }
        shared.rev = (shared.rev + 1).max(now_ms());
        shared.overview = Some(overview.clone());
        overview.rev = shared.rev;
        Some(overview)
    }

    /// 现在的总览（手机来要时）。
    fn current_overview(&self) -> Overview {
        let overview = self.inner.host.overview();
        if let Some(stamped) = self.stamp(overview.clone()) {
            return stamped;
        }
        Overview { rev: self.shared().rev, ..overview }
    }

    /// 加密后放进发送队列。没连上时丢掉：手机上线后自己会要。
    pub(crate) fn send_to(&self, phone: &str, message: DesktopMessage) {
        let shared = self.shared();
        let (Some(out), Some(channel)) = (&shared.out, shared.channels.get(phone)) else { return };
        let id = uuid::Uuid::new_v4().to_string();
        let aad = crypto::header(&id, &shared.saved.device, phone);
        let body = crypto::seal(&channel.to_phone, &desk::encode(message), &aad);
        let frame = serde_json::json!({ "type": "send", "to": phone, "id": id, "body": body });
        let _ = out.send(frame.to_string());
    }

    /// 配好对的手机。
    pub fn phones(&self) -> Vec<PhoneView> {
        let shared = self.shared();
        shared
            .saved
            .phones
            .iter()
            .map(|p| PhoneView {
                id: p.id.clone(),
                name: p.name.clone(),
                platform: p.platform.clone(),
                online: shared.online.contains(&p.id),
                paired_at: p.paired_at,
            })
            .collect()
    }

    /// 生成一个配对二维码（要连服务器，请在后台线程调用）。
    pub fn pair(&self) -> Result<Pairing> {
        let (token, _) = self.ready()?;
        let code = self.with_token_retry(&token, |api, token| api.create_pairing(token))?;
        let secret = crypto::random_secret();
        let expires_at = now_ms() + PAIRING_TTL_MS;
        let public = {
            let mut shared = self.shared();
            shared.saved.prune(now_ms());
            shared.saved.pairings.push(OpenPairing { secret: crypto::base64(&secret), expires_at });
            shared.key.public_bytes()
        };
        self.save()?;
        let link = pairing_link(&self.inner.options.server, &code, &public, &secret);
        Ok(Pairing { svg: qr_svg(&link)?, link, expires_at })
    }

    /// 解除一台手机：服务器上删掉它（它的登录随之失效），本机也忘掉。
    pub fn unpair(&self, id: &str) -> Result<()> {
        let (token, _) = self.ready()?;
        self.with_token_retry(&token, |api, token| api.remove_device(token, id))?;
        {
            let mut shared = self.shared();
            shared.saved.phones.retain(|p| p.id != id);
            shared.channels.remove(id);
            shared.online.remove(id);
        }
        self.save()
    }

    /// 确保有令牌、这台电脑登记过，返回令牌和设备编号。
    fn ready(&self) -> Result<(String, String)> {
        let token = self.token()?;
        let device = self.shared().saved.device.clone();
        if !device.is_empty() {
            return Ok((token, device));
        }
        let device = self.register(&token)?;
        Ok((token, device))
    }

    /// 在服务器上的令牌：和语音识别共用，没有就领一个匿名的。
    fn token(&self) -> Result<String> {
        if let Some(token) = self.shared().token.clone() {
            return Ok(token);
        }
        let file = self.inner.options.paths.account_file();
        let server = &self.inner.options.server;
        let token = match Account::load(&file).token_for(server) {
            Some(token) => token.to_string(),
            None => self.sign_in_again()?,
        };
        self.shared().token = Some(token.clone());
        Ok(token)
    }

    /// 领一个新的匿名身份。是新的账号了：之前的设备和手机都不算数。
    fn sign_in_again(&self) -> Result<String> {
        let file = self.inner.options.paths.account_file();
        let server = self.inner.options.server.clone();
        let saved = Account::load(&file);
        if saved.token_for(&server).is_some() && !saved.anonymous {
            // 用邮箱登录的账号不悄悄换掉，让用户重新登录。
            anyhow::bail!(ApiError::Unauthorized);
        }
        let token = self.api().sign_in_anonymous().context("cannot sign in to the Duolo server")?;
        Account { server, token: token.clone(), anonymous: true }.save(&file)?;
        {
            let mut shared = self.shared();
            shared.token = Some(token.clone());
            shared.saved.device.clear();
            shared.saved.phones.clear();
            shared.saved.pairings.clear();
            shared.channels.clear();
        }
        self.save()?;
        Ok(token)
    }

    /// 登记这台电脑，记下设备编号。
    fn register(&self, token: &str) -> Result<String> {
        let public = self.shared().key.public_base64();
        let o = &self.inner.options;
        let device = self.api().register(token, &o.name, &o.platform, &public)?;
        self.shared().saved.device = device.clone();
        self.save()?;
        Ok(device)
    }

    /// 调一个要登录的接口；令牌失效时重新领一次、没登记时登记一次，再试。
    fn with_token_retry<T>(&self, token: &str, call: impl Fn(&Api, &str) -> Result<T>) -> Result<T> {
        let api = self.api();
        match call(&api, token) {
            Err(err) if api_error(&err) == Some(&ApiError::Unauthorized) => {
                self.shared().token = None;
                let token = self.sign_in_again()?;
                self.register(&token)?;
                call(&api, &token)
            }
            Err(err) if api_error(&err) == Some(&ApiError::NotRegistered) => {
                self.register(token)?;
                call(&api, token)
            }
            result => result,
        }
    }

    /// 认出发消息的手机并解开：认识的用它的密钥；第一次来的，用还没用掉的配对密码逐个试。
    fn open_from(&self, from: &str, id: &str, body: &str) -> Option<Vec<u8>> {
        let (aad, known) = {
            let shared = self.shared();
            (crypto::header(id, from, &shared.saved.device), shared.channels.get(from).cloned())
        };
        if let Some(channel) = known {
            return crypto::open(&channel.from_phone, body, &aad).ok();
        }
        if self.shared().saved.pairings.is_empty() {
            return None;
        }
        // 新手机：从服务器拿它的公钥。
        let token = self.token().ok()?;
        let (_, devices) = self.with_token_retry(&token, |api, token| api.devices(token)).ok()?;
        let device = devices.into_iter().find(|d| d.id == from)?;
        let phone_key = crypto::key32(&device.public_key).ok()?;
        let mut shared = self.shared();
        let now = now_ms();
        let found = shared.saved.pairings.iter().filter(|p| p.expires_at > now).find_map(|pairing| {
            let secret = crypto::from_base64(&pairing.secret).ok()?;
            let channel = shared.key.channel(&phone_key, &secret);
            let plaintext = crypto::open(&channel.from_phone, body, &aad).ok()?;
            Some((pairing.secret.clone(), channel, plaintext))
        });
        let (secret, channel, plaintext) = found?;
        shared.saved.pairings.retain(|p| p.secret != secret);
        shared.saved.phones.retain(|p| p.id != from);
        shared.saved.phones.push(PhoneRecord {
            id: device.id.clone(),
            name: device.name,
            platform: device.platform,
            key: device.public_key,
            secret,
            paired_at: now,
        });
        shared.channels.insert(device.id.clone(), channel);
        shared.online.insert(device.id);
        drop(shared);
        if let Err(err) = self.save() {
            eprintln!("link: cannot save the new phone: {err:#}");
        }
        Some(plaintext)
    }
}

fn channels_of(key: &KeyPair, phones: &[PhoneRecord]) -> HashMap<String, Channel> {
    phones
        .iter()
        .filter_map(|p| {
            let phone = crypto::key32(&p.key).ok()?;
            let secret = crypto::from_base64(&p.secret).ok()?;
            Some((p.id.clone(), key.channel(&phone, &secret)))
        })
        .collect()
}

/// 配对链接，格式见 mobile/contract/behavior.json 的 `pairing`。
fn pairing_link(server: &str, code: &str, public: &[u8; 32], secret: &[u8]) -> String {
    format!(
        "duolo://pair?v=1&server={}&code={}&key={}&secret={}",
        percent_encode(server),
        percent_encode(code),
        crypto::base64url(public),
        crypto::base64url(secret)
    )
}

fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn qr_svg(text: &str) -> Result<String> {
    use qrcode::render::svg;
    let code = qrcode::QrCode::new(text.as_bytes()).context("cannot make the QR code")?;
    Ok(code.render::<svg::Color<'_>>().min_dimensions(220, 220).quiet_zone(true).build())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_the_pairing_link_like_the_contract() {
        let c: serde_json::Value =
            serde_json::from_str(include_str!("../../../mobile/contract/behavior.json")).unwrap();
        let sample = &c["pairing"]["valid"][0];
        let text = |key: &str| sample[key].as_str().unwrap().to_string();
        let public = crypto::key32(&text("key")).unwrap();
        let secret = crypto::from_base64(&text("secret")).unwrap();
        assert_eq!(pairing_link(&text("server"), &text("code"), &public, &secret), text("link"));
    }

    #[test]
    fn renders_a_qr_code() {
        let svg = qr_svg("duolo://pair?v=1").unwrap();
        assert!(svg.starts_with("<?xml") && svg.contains("<svg"));
    }
}
