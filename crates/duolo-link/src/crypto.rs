//! 电脑和手机之间的端到端加密，三端一样（样例在 mobile/contract/crypto.json）：
//!
//! - 每台设备一对 X25519 密钥，公钥登记在服务器上。
//! - 配对时电脑在二维码里多给一个随机的配对密码（`secret`），服务器看不到二维码，所以拿到公钥也算不出密钥。
//! - 两个方向各一把密钥：HKDF-SHA256（盐是配对密码，输入是 X25519 共享密钥，
//!   info 是 `duolo/v1 desktop->phone` 或 `duolo/v1 phone->desktop` 加上电脑公钥、手机公钥）。
//! - 每条消息 ChaCha20-Poly1305，随机 12 字节 nonce，附加数据是 `duolo/v1\n<消息编号>\n<发送方>\n<接收方>`，
//!   服务器改了收发方或编号就解不开。密文是 `1 | nonce | 密文 | 标签`，再 base64。

use anyhow::{bail, Context, Result};
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine as _;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use hkdf::Hkdf;
use rand_core::{OsRng, RngCore};
use sha2::Sha256;
use x25519_dalek::{PublicKey, StaticSecret};

/// 密文的格式版本（第一个字节）。
const FORMAT: u8 = 1;
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;

/// 这台电脑的密钥对。
#[derive(Clone)]
pub struct KeyPair {
    secret: StaticSecret,
    public: PublicKey,
}

impl std::fmt::Debug for KeyPair {
    // 不打印私钥。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyPair").field("public", &self.public_base64()).finish_non_exhaustive()
    }
}

impl KeyPair {
    pub fn generate() -> Self {
        Self::from_secret(StaticSecret::random_from_rng(OsRng))
    }

    /// 从保存的私钥（base64）恢复。
    pub fn from_base64(secret: &str) -> Result<Self> {
        Ok(Self::from_secret(StaticSecret::from(key32(secret)?)))
    }

    fn from_secret(secret: StaticSecret) -> Self {
        let public = PublicKey::from(&secret);
        Self { secret, public }
    }

    pub fn secret_base64(&self) -> String {
        STANDARD.encode(self.secret.to_bytes())
    }

    pub fn public_bytes(&self) -> [u8; 32] {
        self.public.to_bytes()
    }

    /// 登记在服务器上的公钥（标准 base64）。
    pub fn public_base64(&self) -> String {
        STANDARD.encode(self.public.to_bytes())
    }

    /// 和一台手机之间的两把密钥。`phone_key` 是手机的公钥，`secret` 是配对时二维码里的密码。
    pub fn channel(&self, phone_key: &[u8; 32], secret: &[u8]) -> Channel {
        let shared = self.secret.diffie_hellman(&PublicKey::from(*phone_key));
        let derive = |direction: &str| {
            let mut info = format!("duolo/v1 {direction}").into_bytes();
            info.extend_from_slice(&self.public.to_bytes());
            info.extend_from_slice(phone_key);
            let mut key = [0u8; 32];
            Hkdf::<Sha256>::new(Some(secret), shared.as_bytes())
                .expand(&info, &mut key)
                .expect("32 bytes is a valid HKDF-SHA256 length");
            key
        };
        Channel { to_phone: derive("desktop->phone"), from_phone: derive("phone->desktop") }
    }
}

/// 和一台手机之间两个方向的密钥。
#[derive(Clone, PartialEq, Eq)]
pub struct Channel {
    pub to_phone: [u8; 32],
    pub from_phone: [u8; 32],
}

impl std::fmt::Debug for Channel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Channel { .. }")
    }
}

/// 一条消息的附加数据：绑定编号、发送方和接收方。
pub fn header(id: &str, from: &str, to: &str) -> Vec<u8> {
    format!("duolo/v1\n{id}\n{from}\n{to}").into_bytes()
}

/// 加密，返回 base64。
pub fn seal(key: &[u8; 32], plaintext: &[u8], aad: &[u8]) -> String {
    let mut nonce = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce);
    seal_with(key, &nonce, plaintext, aad)
}

fn seal_with(key: &[u8; 32], nonce: &[u8; NONCE_LEN], plaintext: &[u8], aad: &[u8]) -> String {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let sealed = cipher
        .encrypt(Nonce::from_slice(nonce), Payload { msg: plaintext, aad })
        .expect("encrypting in memory does not fail");
    let mut out = Vec::with_capacity(1 + NONCE_LEN + sealed.len());
    out.push(FORMAT);
    out.extend_from_slice(nonce);
    out.extend_from_slice(&sealed);
    STANDARD.encode(out)
}

/// 解密 base64 的密文。密钥不对、内容被改过、格式不认识时报错。
pub fn open(key: &[u8; 32], body: &str, aad: &[u8]) -> Result<Vec<u8>> {
    let data = STANDARD.decode(body.trim()).context("not base64")?;
    if data.len() < 1 + NONCE_LEN + TAG_LEN {
        bail!("message too short");
    }
    if data[0] != FORMAT {
        bail!("unknown message format {}", data[0]);
    }
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    cipher
        .decrypt(Nonce::from_slice(&data[1..=NONCE_LEN]), Payload { msg: &data[1 + NONCE_LEN..], aad })
        .map_err(|_| anyhow::anyhow!("cannot decrypt the message"))
}

/// 随机的配对密码（32 字节）。
pub fn random_secret() -> [u8; 32] {
    let mut secret = [0u8; 32];
    OsRng.fill_bytes(&mut secret);
    secret
}

/// 二维码里用的 base64url（不带填充）。
pub fn base64url(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn base64(bytes: &[u8]) -> String {
    STANDARD.encode(bytes)
}

pub fn from_base64(text: &str) -> Result<Vec<u8>> {
    STANDARD.decode(text.trim()).context("not base64")
}

/// 32 字节的密钥（标准 base64）。
pub fn key32(text: &str) -> Result<[u8; 32]> {
    from_base64(text)?.try_into().map_err(|_| anyhow::anyhow!("a key must be 32 bytes"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 三端共用的样例。
    fn contract() -> serde_json::Value {
        let text = include_str!("../../../mobile/contract/crypto.json");
        serde_json::from_str(text).unwrap()
    }

    fn s(v: &serde_json::Value) -> &str {
        v.as_str().unwrap()
    }

    #[test]
    fn derives_the_same_keys_as_the_phones() {
        let c = contract();
        let desktop = KeyPair::from_base64(s(&c["desktop"]["privateKey"])).unwrap();
        assert_eq!(desktop.public_base64(), s(&c["desktop"]["publicKey"]));
        let phone = key32(s(&c["phone"]["publicKey"])).unwrap();
        let channel = desktop.channel(&phone, &from_base64(s(&c["secret"])).unwrap());
        assert_eq!(base64(&channel.to_phone), s(&c["keys"]["desktopToPhone"]));
        assert_eq!(base64(&channel.from_phone), s(&c["keys"]["phoneToDesktop"]));
    }

    #[test]
    fn seals_and_opens_the_contract_messages() {
        let c = contract();
        for m in c["messages"].as_array().unwrap() {
            let key = key32(s(&c["keys"][s(&m["direction"])])).unwrap();
            let aad = header(s(&m["id"]), s(&m["from"]), s(&m["to"]));
            let opened = open(&key, s(&m["body"]), &aad).unwrap();
            assert_eq!(String::from_utf8(opened).unwrap(), s(&m["plaintext"]));
            let nonce: [u8; NONCE_LEN] = from_base64(s(&m["nonce"])).unwrap().try_into().unwrap();
            assert_eq!(seal_with(&key, &nonce, s(&m["plaintext"]).as_bytes(), &aad), s(&m["body"]));
        }
    }

    #[test]
    fn rejects_tampered_messages() {
        let key = [7u8; 32];
        let body = seal(&key, b"hello", &header("1", "a", "b"));
        assert!(open(&key, &body, &header("1", "a", "b")).is_ok());
        // 服务器改了接收方、编号，或者用了别的密钥：解不开。
        assert!(open(&key, &body, &header("1", "a", "c")).is_err());
        assert!(open(&key, &body, &header("2", "a", "b")).is_err());
        assert!(open(&[8u8; 32], &body, &header("1", "a", "b")).is_err());
    }
}
