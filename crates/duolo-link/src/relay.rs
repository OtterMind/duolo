//! 后台线程：连着服务器的转发 WebSocket（`/v1/connect`，协议见 server/src/relay.ts），断了自动重连。

use std::io::ErrorKind;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use duolo_core::wire::{DesktopMessage, PhoneMessage};
use serde::Deserialize;
use tungstenite::client::IntoClientRequest;
use tungstenite::protocol::frame::coding::CloseCode;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Connector, Message, WebSocket};

use crate::api::{api_error, ApiError};
use crate::desk::{self, Verdict};
use crate::{now_ms, Link, Status};

type Socket = WebSocket<MaybeTlsStream<TcpStream>>;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
/// 读一次最多等多久，之后看看有没有要发的。
const POLL: Duration = Duration::from_millis(200);
/// 这么久没动静就 ping 一下，免得中间的代理断开空闲连接。
const PING_EVERY: Duration = Duration::from_secs(25);
/// 重连的等待：从 2 秒开始，每次翻倍，最多 1 分钟。
const BACKOFF_MIN: Duration = Duration::from_secs(2);
const BACKOFF_MAX: Duration = Duration::from_secs(60);
/// 服务器关连接的原因：没登录或设备没登记（server/src/app.ts）。
const CLOSE_UNLINKED: u16 = 4401;

pub(crate) fn spawn(link: Link) {
    thread::Builder::new()
        .name("duolo-link".into())
        .spawn(move || {
            let mut backoff = BACKOFF_MIN;
            loop {
                link.set_status(Status::Connecting);
                let started = Instant::now();
                let err = match session(&link) {
                    Ok(()) => anyhow!("the server closed the connection"),
                    Err(err) => err,
                };
                link.shared().out = None;
                link.shared().online.clear();
                eprintln!("link: {err:#}");
                link.set_status(Status::Offline { error: error_code(&err).into() });
                // 连上过一阵子再断的，从头开始等。
                if started.elapsed() > BACKOFF_MAX {
                    backoff = BACKOFF_MIN;
                }
                thread::sleep(backoff);
                backoff = (backoff * 2).min(BACKOFF_MAX);
            }
        })
        .expect("can start the link thread");
}

/// 给界面看的错误码。
fn error_code(err: &anyhow::Error) -> &'static str {
    match api_error(err) {
        Some(ApiError::Unauthorized) => "linkSignedOut",
        Some(_) => "linkRefused",
        None => "linkOffline",
    }
}

/// 连上一次，直到断开。
fn session(link: &Link) -> Result<()> {
    let (token, device) = link.ready()?;
    let mut socket = connect(&link.inner.options.server, &token)?;
    set_read_timeout(&socket, Some(POLL))?;
    let (tx, rx) = mpsc::channel::<String>();
    link.shared().out = Some(tx);
    link.set_status(Status::Online);
    let mut last_sent = Instant::now();
    loop {
        match socket.read() {
            Ok(Message::Text(text)) => {
                if let Ok(frame) = serde_json::from_str::<Frame>(&text) {
                    handle(link, &device, frame, &mut socket)?;
                }
            }
            Ok(Message::Close(frame)) => {
                if frame.as_ref().is_some_and(|f| f.code == CloseCode::from(CLOSE_UNLINKED)) {
                    // 令牌失效或设备没登记：下次连之前重新领、重新登记。
                    let reason = frame.map(|f| f.reason.to_string()).unwrap_or_default();
                    let mut shared = link.shared();
                    if reason == "unauthorized" {
                        shared.token = None;
                        drop(shared);
                        link.sign_in_again()?;
                    } else {
                        shared.saved.device.clear();
                    }
                    bail!("the server unlinked this computer ({reason})");
                }
                bail!("the server closed the connection");
            }
            Ok(_) => {}
            Err(err) if timed_out(&err) => {}
            Err(err) => return Err(err).context("connection lost"),
        }
        while let Ok(frame) = rx.try_recv() {
            socket.send(Message::text(frame))?;
            last_sent = Instant::now();
        }
        if last_sent.elapsed() > PING_EVERY {
            socket.send(Message::text(r#"{"type":"ping"}"#))?;
            last_sent = Instant::now();
        }
    }
}

/// 服务器发来的（只认用得到的字段）。
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Frame {
    Hello {
        peers: Vec<Peer>,
    },
    Presence {
        device: String,
        online: bool,
    },
    Message {
        id: String,
        from: String,
        body: String,
    },
    Error {
        code: String,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
struct Peer {
    id: String,
    name: String,
    online: bool,
}

fn handle(link: &Link, device: &str, frame: Frame, socket: &mut Socket) -> Result<()> {
    match frame {
        Frame::Hello { peers } => {
            let mut shared = link.shared();
            // 服务器上已经没有的手机（在手机上退出了、在别处解除了）忘掉。
            let before = shared.saved.phones.len();
            shared.saved.phones.retain(|p| peers.iter().any(|peer| peer.id == p.id));
            let removed = before != shared.saved.phones.len();
            for phone in &mut shared.saved.phones {
                if let Some(peer) = peers.iter().find(|peer| peer.id == phone.id) {
                    phone.name = peer.name.clone();
                }
            }
            let phones: Vec<String> = shared.saved.phones.iter().map(|p| p.id.clone()).collect();
            shared.channels.retain(|id, _| phones.contains(id));
            shared.online = peers.iter().filter(|p| p.online).map(|p| p.id.clone()).collect();
            drop(shared);
            if removed {
                link.save()?;
            }
        }
        Frame::Presence { device, online } => {
            let mut shared = link.shared();
            if online {
                shared.online.insert(device);
            } else {
                shared.online.remove(&device);
            }
        }
        Frame::Message { id, from, body } => {
            // 解不开的（不认识的设备、坏了的消息）也确认掉，不然服务器一直补发。
            if let Some(plaintext) = link.open_from(&from, &id, &body) {
                if let Some(message) = desk::decode(&plaintext) {
                    on_phone_message(link, &from, message);
                }
            } else {
                eprintln!("link: ignored a message from {from} that cannot be decrypted");
            }
            socket.send(Message::text(serde_json::json!({ "type": "ack", "ids": [id] }).to_string()))?;
        }
        Frame::Error { code } => eprintln!("link: server error {code} (device {device})"),
        Frame::Other => {}
    }
    Ok(())
}

fn on_phone_message(link: &Link, phone: &str, message: PhoneMessage) {
    match message {
        PhoneMessage::Sync => link.send_to(phone, DesktopMessage::Overview(link.current_overview())),
        PhoneMessage::Fetch { task, after } => {
            for reply in desk::fetch(link.inner.host.as_ref(), task, after) {
                link.send_to(phone, reply);
            }
        }
        PhoneMessage::Command { id, at, command } => {
            let verdict = desk::check(&link.shared().saved, &id, at, &command, now_ms());
            match verdict {
                Verdict::Done(result) | Verdict::Refuse(result) => {
                    link.send_to(phone, DesktopMessage::Result { id, result });
                }
                Verdict::Run => {
                    // 命令可能要几秒（建任务、启动 Agent），不挡住收发。
                    let link = link.clone();
                    let phone = phone.to_string();
                    let host = Arc::clone(&link.inner.host);
                    thread::spawn(move || {
                        let result = host.execute(command);
                        desk::remember(&mut link.shared().saved, &id, now_ms(), &result);
                        if let Err(err) = link.save() {
                            eprintln!("link: cannot save the command result: {err:#}");
                        }
                        link.send_to(&phone, DesktopMessage::Result { id, result });
                    });
                }
            }
        }
    }
}

/// `https://host` → `wss://host/v1/connect`。
fn connect_url(server: &str) -> Result<String> {
    let server = server.trim_end_matches('/');
    if let Some(rest) = server.strip_prefix("https://") {
        Ok(format!("wss://{rest}/v1/connect"))
    } else if let Some(rest) = server.strip_prefix("http://") {
        Ok(format!("ws://{rest}/v1/connect"))
    } else {
        bail!("invalid server address: {server}")
    }
}

fn connect(server: &str, token: &str) -> Result<Socket> {
    let mut req = connect_url(server)?.into_client_request()?;
    req.headers_mut().insert("authorization", format!("Bearer {token}").parse()?);
    let uri = req.uri().clone();
    let host = uri.host().context("server address has no host")?;
    let port = uri.port_u16().unwrap_or(if uri.scheme_str() == Some("wss") { 443 } else { 80 });
    // 逐个地址试：`localhost` 常常先解析成 IPv6，而服务器可能只听 IPv4。
    let mut last = None;
    let mut connected = None;
    for addr in (host, port).to_socket_addrs()? {
        match TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) {
            Ok(stream) => {
                connected = Some(stream);
                break;
            }
            Err(err) => last = Some(err),
        }
    }
    let stream = match (connected, last) {
        (Some(stream), _) => stream,
        (None, Some(err)) => return Err(err.into()),
        (None, None) => bail!("cannot resolve the server address"),
    };
    stream.set_read_timeout(Some(CONNECT_TIMEOUT))?;
    stream.set_write_timeout(Some(CONNECT_TIMEOUT))?;
    stream.set_nodelay(true)?;
    let (socket, _) = tungstenite::client_tls_with_config(req, stream, None, Some(Connector::Rustls(tls_config())))
        .map_err(|e| anyhow!("cannot connect to the server: {e}"))?;
    Ok(socket)
}

/// 用 ring 和内置的根证书，不依赖进程里装了哪个加密实现。
fn tls_config() -> Arc<rustls::ClientConfig> {
    let roots = rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("ring supports the default TLS versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
    Arc::new(config)
}

fn set_read_timeout(socket: &Socket, timeout: Option<Duration>) -> Result<()> {
    let tcp = match socket.get_ref() {
        MaybeTlsStream::Plain(s) => s,
        MaybeTlsStream::Rustls(s) => &s.sock,
        _ => bail!("unexpected stream"),
    };
    tcp.set_read_timeout(timeout)?;
    Ok(())
}

/// 读超时：Unix 上是 WouldBlock，Windows 上是 TimedOut。
fn timed_out(err: &tungstenite::Error) -> bool {
    matches!(err, tungstenite::Error::Io(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_the_connect_url() {
        assert_eq!(connect_url("https://duolo.io/").unwrap(), "wss://duolo.io/v1/connect");
        assert_eq!(connect_url("http://localhost:8787").unwrap(), "ws://localhost:8787/v1/connect");
        assert!(connect_url("duolo.io").is_err());
    }
}
