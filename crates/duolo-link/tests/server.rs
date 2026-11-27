//! 和真的服务器走一遍：电脑连上、生成配对码；模拟手机领取配对码、连上转发，加密发 `sync` 和一条命令，收到总览和结果。
//!
//! 要一台本机的服务器（server/，`docker compose up -d` 之后 `pnpm dev`），地址放在 `DUOLO_TEST_SERVER`：
//!   DUOLO_TEST_SERVER=http://127.0.0.1:8787 cargo test -p duolo-link --test server -- --ignored

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine as _;
use duolo_core::wire::{DesktopMessage, Overview, Packet, ProjectOverview};
use duolo_core::{Command, CommandResult, Paths, TaskEvent, TaskId};
use duolo_link::{crypto, Host, Link, Options, Status};
use hkdf::Hkdf;
use serde_json::{json, Value};
use sha2::Sha256;
use tungstenite::Message;
use x25519_dalek::{PublicKey, StaticSecret};

#[derive(Default)]
struct FakeHost {
    commands: Mutex<Vec<Command>>,
}

impl Host for FakeHost {
    fn overview(&self) -> Overview {
        let project =
            ProjectOverview { id: "shop".into(), name: "shop".into(), branch: "main".into(), branches: vec![] };
        Overview { projects: vec![project], writer: "writer".into(), reviewer: "reviewer".into(), ..Overview::default() }
    }
    fn events_since(&self, _task: TaskId, _after: u64) -> Vec<TaskEvent> {
        Vec::new()
    }
    fn review(&self, _task: TaskId) -> Option<(u64, String)> {
        None
    }
    fn execute(&self, command: Command) -> CommandResult {
        self.commands.lock().unwrap().push(command);
        CommandResult { ok: true, task: Some(TaskId(7)), error: None }
    }
}

fn query<'a>(link: &'a str, name: &str) -> &'a str {
    link.split(['?', '&']).find_map(|kv| kv.strip_prefix(&format!("{name}="))).unwrap()
}

fn b64url(text: &str) -> Vec<u8> {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(text).unwrap()
}

/// 手机这边的两把密钥（和电脑算的一样：info 里先电脑公钥、后手机公钥）。
fn phone_keys(phone: &StaticSecret, desktop: &[u8; 32], secret: &[u8]) -> ([u8; 32], [u8; 32]) {
    let shared = phone.diffie_hellman(&PublicKey::from(*desktop));
    let phone_public = PublicKey::from(phone).to_bytes();
    let derive = |direction: &str| {
        let mut info = format!("duolo/v1 {direction}").into_bytes();
        info.extend_from_slice(desktop);
        info.extend_from_slice(&phone_public);
        let mut key = [0u8; 32];
        Hkdf::<Sha256>::new(Some(secret), shared.as_bytes()).expand(&info, &mut key).unwrap();
        key
    };
    (derive("phone->desktop"), derive("desktop->phone"))
}

#[test]
#[ignore = "needs a local server in DUOLO_TEST_SERVER"]
fn pairs_and_talks_through_a_real_server() {
    let server = std::env::var("DUOLO_TEST_SERVER").expect("set DUOLO_TEST_SERVER");
    let dir = tempfile::tempdir().unwrap();
    let host = Arc::new(FakeHost::default());
    let options = Options {
        paths: Paths::at(dir.path()),
        server: server.clone(),
        name: "Test Mac".into(),
        platform: "mac".into(),
    };
    let link = Link::start(options, host.clone());
    let deadline = Instant::now() + Duration::from_secs(20);
    while link.status() != Status::Online {
        assert!(Instant::now() < deadline, "desktop did not connect: {:?}", link.status());
        std::thread::sleep(Duration::from_millis(100));
    }

    // 手机扫码：领取配对码，登记成同一个账号下的设备。
    let pairing = link.pair().unwrap();
    assert!(pairing.svg.contains("<svg"));
    let code = query(&pairing.link, "code");
    let desktop_key: [u8; 32] = b64url(query(&pairing.link, "key")).try_into().unwrap();
    let secret = b64url(query(&pairing.link, "secret"));
    let phone = StaticSecret::random_from_rng(rand_core::OsRng);
    let phone_public = crypto::base64(&PublicKey::from(&phone).to_bytes());
    let claim: Value = ureq::post(format!("{server}/v1/pairings/claim"))
        .send_json(
            json!({ "code": code, "device": { "name": "Test iPhone", "platform": "ios", "publicKey": phone_public } }),
        )
        .unwrap()
        .body_mut()
        .read_json()
        .unwrap();
    let token = claim["token"].as_str().unwrap();
    let me = claim["device"]["id"].as_str().unwrap().to_string();
    let desktop = claim["desktop"]["id"].as_str().unwrap().to_string();
    assert_eq!(claim["desktop"]["publicKey"].as_str().unwrap(), crypto::base64(&desktop_key));
    let (to_desktop, from_desktop) = phone_keys(&phone, &desktop_key, &secret);

    let ws_url = server.replacen("http", "ws", 1) + "/v1/connect?token=" + token;
    let (mut socket, _) = tungstenite::connect(ws_url).unwrap();
    if let tungstenite::stream::MaybeTlsStream::Plain(tcp) = socket.get_ref() {
        tcp.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    }
    // 服务器验证完登录才开始收消息，先等它的 hello（手机也是这样）。
    loop {
        let Message::Text(text) = socket.read().unwrap() else { continue };
        if serde_json::from_str::<Value>(&text).unwrap()["type"] == "hello" {
            break;
        }
    }
    let send = |socket: &mut tungstenite::WebSocket<_>, n: u32, plaintext: Value| {
        let id = format!("phone-{n}");
        let body = crypto::seal(&to_desktop, plaintext.to_string().as_bytes(), &crypto::header(&id, &me, &desktop));
        socket
            .send(Message::text(json!({ "type": "send", "to": desktop, "id": id, "body": body }).to_string()))
            .unwrap();
    };
    send(&mut socket, 1, json!({ "v": 1, "type": "sync" }));
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
    let command = json!({ "type": "send_message", "task": 3, "text": "hello from the phone" });
    send(&mut socket, 2, json!({ "v": 1, "type": "command", "id": "c-1", "at": now, "command": command }));
    // 同一条命令再来一次：只执行一次，回同样的结果。
    send(&mut socket, 3, json!({ "v": 1, "type": "command", "id": "c-1", "at": now, "command": command }));

    let mut overview = None;
    let mut results = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(20);
    while (overview.is_none() || results.len() < 2) && Instant::now() < deadline {
        let Message::Text(text) = socket.read().unwrap() else { continue };
        let frame: Value = serde_json::from_str(&text).unwrap();
        if frame["type"] != "message" {
            continue;
        }
        let id = frame["id"].as_str().unwrap();
        let plaintext =
            crypto::open(&from_desktop, frame["body"].as_str().unwrap(), &crypto::header(id, &desktop, &me)).unwrap();
        socket.send(Message::text(json!({ "type": "ack", "ids": [id] }).to_string())).unwrap();
        match serde_json::from_slice::<Packet<DesktopMessage>>(&plaintext).unwrap().message {
            DesktopMessage::Overview(o) => overview = Some(o),
            DesktopMessage::Result { id, result } => results.push((id, result)),
            _ => {}
        }
    }
    let overview = overview.expect("got an overview");
    assert_eq!(overview.projects[0].id, "shop");
    assert!(overview.rev > 0);
    assert_eq!(results.len(), 2, "got both results");
    for (id, result) in &results {
        assert_eq!(id, "c-1");
        assert_eq!(result.task, Some(TaskId(7)));
    }
    assert_eq!(host.commands.lock().unwrap().len(), 1, "the command ran once");
    assert_eq!(link.phones().len(), 1);
    assert_eq!(link.phones()[0].name, "Test iPhone");

    // 解除：服务器上删掉，手机的登录随之失效。
    link.unpair(&me).unwrap();
    assert!(link.phones().is_empty());
}
