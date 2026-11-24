//! 用 Duolo 服务器识别：录麦克风，16 kHz PCM 通过 WebSocket 发给服务器，服务器转给识别服务，
//! 把识别中和最终的文字发回来。协议见 `server/src/voice.ts`。
//!
//! 开始时等服务器说"可以"（`ready`）才算开始成功；服务器不可用、没额度、连不上时返回错误，
//! 调用方改用本机的识别。

use std::io::ErrorKind;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use duolo_core::UserError;
use serde::Deserialize;
use tungstenite::client::IntoClientRequest;
use tungstenite::protocol::frame::coding::CloseCode;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Connector, Message, WebSocket};

use crate::mic::{Chunk, Mic};

/// 连服务器、等它说"可以"最长多久；超过就改用本机识别，免得用户等。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
/// 说完后等最终结果最长多久（服务器自己最多等 15 秒）。
const FINAL_TIMEOUT: Duration = Duration::from_secs(20);
/// 收发循环里每次读消息最多等多久。
const POLL: Duration = Duration::from_millis(20);
/// 服务器用这个关闭码表示登录凭据无效。
const CLOSE_UNAUTHORIZED: u16 = 4401;

/// 开始一次识别要的东西。
#[derive(Debug, Clone, Default)]
pub struct CloudRequest {
    /// 服务器地址（`https://…`），识别接口是 `/v1/voice`。
    pub server: String,
    pub token: String,
    /// 热词：文件名、函数名等（重要的在前）。
    pub hotwords: Vec<String>,
    /// 对话上下文（新的在前）。
    pub context: Vec<String>,
    /// 让服务器把结果整理成指令。
    pub organize: bool,
}

/// 识别结果：原话，以及服务器整理过的（要求整理且整理出来了时）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transcript {
    pub raw: String,
    pub organized: Option<String>,
}

/// 服务器拒绝了这次识别。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloudError {
    /// 登录凭据无效：重新登录再试。
    Unauthorized,
    /// 服务器给的错误码（`voiceUnavailable`、`voiceQuotaExceeded`……）。
    Refused(String),
}

impl std::fmt::Display for CloudError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unauthorized => write!(f, "the voice server rejected the sign-in"),
            Self::Refused(code) => write!(f, "the voice server refused: {code}"),
        }
    }
}

impl std::error::Error for CloudError {}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerMessage {
    Ready,
    Partial {
        text: String,
    },
    Final {
        text: String,
        #[serde(default)]
        organized: Option<String>,
    },
    Error {
        error: String,
    },
}

/// 识别中的状态，收发线程写，界面轮询读。
#[derive(Debug, Default)]
struct Shared {
    partial: String,
    level: Option<f32>,
    /// 收发线程结束时的结果。
    result: Option<Result<Transcript, String>>,
}

#[derive(Debug)]
pub struct Recorder {
    started: Instant,
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

type Socket = WebSocket<MaybeTlsStream<TcpStream>>;

impl Recorder {
    /// 连上服务器、等它说"可以"，再打开麦克风。阻塞最多几秒，请在后台线程调用。
    pub fn start(request: &CloudRequest) -> Result<Self> {
        let mut socket = connect(request)?;
        let hello = serde_json::json!({
            "type": "start",
            "hotwords": request.hotwords,
            "context": request.context,
            "organize": request.organize,
        });
        socket.send(Message::text(hello.to_string()))?;
        wait_ready(&mut socket)?;
        set_poll(&socket, Some(POLL))?;
        let (mic, chunks) = Mic::start()?;

        let shared = Arc::new(Mutex::new(Shared::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let cancel = Arc::new(AtomicBool::new(false));
        let worker = {
            let (shared, stop, cancel) = (Arc::clone(&shared), Arc::clone(&stop), Arc::clone(&cancel));
            thread::spawn(move || {
                let result = pump(&mut socket, mic, &chunks, &shared, &stop, &cancel);
                let _ = socket.close(None);
                let _ = socket.flush();
                lock(&shared).result = Some(result.map_err(|e| format!("{e:#}")));
            })
        };
        Ok(Self { started: Instant::now(), shared, stop, cancel, worker: Some(worker) })
    }

    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    pub fn partial(&self) -> String {
        lock(&self.shared).partial.clone()
    }

    pub fn level(&self) -> Option<f32> {
        lock(&self.shared).level
    }

    /// 结束录音，等最终结果。阻塞，请在后台线程调用。
    pub fn finish(mut self) -> Result<Transcript> {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        let shared = lock(&self.shared);
        let transcript = match shared.result.clone() {
            Some(Ok(transcript)) => transcript,
            // 断了但已经听到一些：用听到的。
            _ if !shared.partial.trim().is_empty() => Transcript { raw: shared.partial.clone(), organized: None },
            Some(Err(err)) => {
                eprintln!("voice: {err}");
                return Err(UserError::VoiceFailed.into());
            }
            None => return Err(UserError::VoiceFailed.into()),
        };
        let raw = transcript.raw.trim().to_string();
        if raw.is_empty() {
            return Err(UserError::NothingRecognized.into());
        }
        let organized = transcript.organized.map(|o| o.trim().to_string()).filter(|o| !o.is_empty() && *o != raw);
        Ok(Transcript { raw, organized })
    }

    /// 放弃这次录音。
    pub fn cancel(mut self) {
        self.cancel.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}

fn lock(shared: &Mutex<Shared>) -> std::sync::MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(|p| p.into_inner())
}

/// `https://host/path` → `wss://host/path/v1/voice`（http 对应 ws）。
fn voice_url(server: &str) -> Result<String> {
    let server = server.trim_end_matches('/');
    let url = if let Some(rest) = server.strip_prefix("https://") {
        format!("wss://{rest}/v1/voice")
    } else if let Some(rest) = server.strip_prefix("http://") {
        format!("ws://{rest}/v1/voice")
    } else {
        bail!("invalid server address: {server}");
    };
    Ok(url)
}

fn connect(request: &CloudRequest) -> Result<Socket> {
    let mut req = voice_url(&request.server)?.into_client_request()?;
    req.headers_mut().insert("authorization", format!("Bearer {}", request.token).parse()?);
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
        .map_err(|e| anyhow!("cannot connect to the voice server: {e}"))?;
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

fn set_poll(socket: &Socket, timeout: Option<Duration>) -> Result<()> {
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

fn closed_unauthorized(message: &Message) -> bool {
    matches!(message, Message::Close(Some(frame)) if frame.code == CloseCode::from(CLOSE_UNAUTHORIZED))
}

fn wait_ready(socket: &mut Socket) -> Result<()> {
    loop {
        let message = socket.read().map_err(|e| {
            if timed_out(&e) {
                anyhow!("the voice server did not answer in time")
            } else {
                anyhow!("voice server: {e}")
            }
        })?;
        if closed_unauthorized(&message) {
            return Err(CloudError::Unauthorized.into());
        }
        let Message::Text(text) = message else { continue };
        match serde_json::from_str::<ServerMessage>(&text) {
            Ok(ServerMessage::Ready) => return Ok(()),
            Ok(ServerMessage::Error { error }) => return Err(CloudError::Refused(error).into()),
            _ => {}
        }
    }
}

/// 收发循环：把麦克风的声音发出去，收识别结果；说完后发 stop 等最终结果。
fn pump(
    socket: &mut Socket,
    mic: Mic,
    chunks: &Receiver<Chunk>,
    shared: &Mutex<Shared>,
    stop: &AtomicBool,
    cancel: &AtomicBool,
) -> Result<Transcript> {
    let mut mic = Some(mic);
    let mut deadline = None;
    // 诊断：发了多少声音、最大音量（全是 0 说明系统给的是静音，多半是没有麦克风权限）。
    let mut sent = 0usize;
    let mut peak = 0f32;
    loop {
        if cancel.load(Ordering::SeqCst) {
            bail!("cancelled");
        }
        // 先把攒下的声音发完。
        loop {
            match chunks.recv_timeout(Duration::ZERO) {
                Ok(chunk) => {
                    sent += chunk.pcm.len();
                    peak = peak.max(chunk.level);
                    lock(shared).level = Some(chunk.level);
                    let bytes: Vec<u8> = chunk.pcm.iter().flat_map(|s| s.to_le_bytes()).collect();
                    socket.send(Message::binary(bytes))?;
                }
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => {
                    // 麦克风停了（设备拔掉）：当作说完了。
                    stop.store(true, Ordering::SeqCst);
                    break;
                }
            }
        }
        if stop.load(Ordering::SeqCst) && deadline.is_none() {
            // 关麦克风会把最后一块声音交出来，发完再说结束。
            drop(mic.take());
            for chunk in chunks.try_iter() {
                let bytes: Vec<u8> = chunk.pcm.iter().flat_map(|s| s.to_le_bytes()).collect();
                socket.send(Message::binary(bytes))?;
            }
            socket.send(Message::text(r#"{"type":"stop"}"#))?;
            #[allow(clippy::cast_precision_loss)]
            let seconds = sent as f32 / crate::mic::RATE as f32;
            eprintln!("voice: sent {seconds:.1} s of audio, peak level {peak:.2}");
            deadline = Some(Instant::now() + FINAL_TIMEOUT);
        }
        if deadline.is_some_and(|d| Instant::now() >= d) {
            bail!("no final result from the voice server");
        }
        match socket.read() {
            Ok(Message::Text(text)) => match serde_json::from_str::<ServerMessage>(&text) {
                Ok(ServerMessage::Partial { text }) => lock(shared).partial = text,
                Ok(ServerMessage::Final { text, organized }) => return Ok(Transcript { raw: text, organized }),
                Ok(ServerMessage::Error { error }) => return Err(CloudError::Refused(error).into()),
                _ => {}
            },
            Ok(Message::Close(_)) => bail!("the voice server closed the connection"),
            Ok(_) => {}
            Err(e) if timed_out(&e) => {}
            Err(e) => return Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_url_follows_the_scheme() {
        assert_eq!(voice_url("https://duolo.example/").unwrap(), "wss://duolo.example/v1/voice");
        assert_eq!(voice_url("http://localhost:8787").unwrap(), "ws://localhost:8787/v1/voice");
        assert!(voice_url("duolo.example").is_err());
    }

    #[test]
    fn parses_server_messages() {
        let m: ServerMessage = serde_json::from_str(r#"{"type":"partial","text":"hi"}"#).unwrap();
        assert!(matches!(m, ServerMessage::Partial { text } if text == "hi"));
        let m: ServerMessage = serde_json::from_str(r#"{"type":"final","text":"a","organized":"b"}"#).unwrap();
        assert!(
            matches!(m, ServerMessage::Final { text, organized } if text == "a" && organized.as_deref() == Some("b"))
        );
        let m: ServerMessage = serde_json::from_str(r#"{"type":"final","text":"a"}"#).unwrap();
        assert!(matches!(m, ServerMessage::Final { organized: None, .. }));
        let m: ServerMessage = serde_json::from_str(r#"{"type":"error","error":"voiceUnavailable"}"#).unwrap();
        assert!(matches!(m, ServerMessage::Error { error } if error == "voiceUnavailable"));
    }

    /// 没有服务器在听的端口：很快失败，调用方好改用本机识别。
    #[test]
    fn unreachable_server_fails_fast() {
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let started = Instant::now();
        let request = CloudRequest { server: format!("http://127.0.0.1:{port}"), ..CloudRequest::default() };
        assert!(Recorder::start(&request).is_err());
        assert!(started.elapsed() < CONNECT_TIMEOUT);
    }

    /// 服务器说没有识别服务：返回它的错误码。
    #[test]
    fn refused_by_server() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut ws = tungstenite::accept(stream).unwrap();
            ws.send(Message::text(r#"{"type":"error","error":"voiceUnavailable"}"#)).unwrap();
            let _ = ws.close(None);
            while ws.read().is_ok() {}
        });
        let request =
            CloudRequest { server: format!("http://127.0.0.1:{port}"), token: "t".into(), ..CloudRequest::default() };
        let err = Recorder::start(&request).err().unwrap();
        assert_eq!(err.downcast_ref::<CloudError>(), Some(&CloudError::Refused("voiceUnavailable".into())));
        server.join().unwrap();
    }

    #[test]
    fn unauthorized_close() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut ws = tungstenite::accept(stream).unwrap();
            let frame = tungstenite::protocol::CloseFrame {
                code: CloseCode::from(CLOSE_UNAUTHORIZED),
                reason: "unauthorized".into(),
            };
            ws.close(Some(frame)).unwrap();
            while ws.read().is_ok() {}
        });
        let request =
            CloudRequest { server: format!("http://127.0.0.1:{port}"), token: "bad".into(), ..CloudRequest::default() };
        let err = Recorder::start(&request).err().unwrap();
        assert_eq!(err.downcast_ref::<CloudError>(), Some(&CloudError::Unauthorized));
        server.join().unwrap();
    }
}
