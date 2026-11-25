//! 用录下来的真实收发跑一遍聊天模式：测试程序自己当假 Agent（见 `duolo_chat::testing`）。

use std::path::Path;
use std::time::{Duration, Instant};

use duolo_agent::Registry;
use duolo_chat::testing::{serve_if_asked, FIXTURE_ENV};
use duolo_chat::{Chat, Entry, Protocol, Spec, Status, Update};
use duolo_core::{AgentKind, Config};

/// 被当成假 Agent 运行时，照着录下来的回答；平时什么也不做。
#[test]
fn fake_agent() {
    serve_if_asked();
}

fn chat(fixture: &str, spec: Spec) -> Chat {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(fixture);
    let exe = std::env::current_exe().unwrap();
    Chat::spawn(Spec {
        program: exe.display().to_string(),
        args: vec!["fake_agent".into(), "--exact".into(), "--nocapture".into(), "--test-threads=1".into()],
        env: vec![(FIXTURE_ENV.into(), fixture.display().to_string())],
        cwd: Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf(),
        ..spec
    })
    .unwrap()
}

/// 一直取更新，直到 `done`。
fn until(chat: &mut Chat, what: &str, mut done: impl FnMut(&mut Chat, &[Update]) -> bool) -> Vec<Update> {
    let deadline = Instant::now() + Duration::from_secs(180);
    let mut all = Vec::new();
    loop {
        all.extend(chat.take_updates());
        if done(chat, &all) {
            return all;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}: {all:#?} {:?}", chat.status());
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn first_prompt_runs_and_permission_is_answered() {
    let mut c = chat("permission.txt", Spec { prompt: Some("Run the tests".into()), ..Spec::default() });
    let mut updates = until(&mut c, "the question", |c, _| c.status() == Status::Asking);
    let ask = c.ask().unwrap();
    assert_eq!((ask.tool.as_str(), ask.target.as_str()), ("Bash", "pnpm test"));
    assert_eq!(ask.options, ["Allow once", "Reject"]);
    assert!(!c.answer(ask.id + 1, 0), "别的问题不答");
    assert!(!c.answer(ask.id, 5), "没有这个选项");
    assert!(c.answer(ask.id, 0));
    updates.extend(until(&mut c, "the reply", |c, u| c.status() == Status::Idle && u.len() >= 2));
    assert_eq!(
        updates,
        vec![
            Update::Session("s-1".into()),
            Update::Entry(Entry::User("Run the tests".into())),
            Update::Entry(Entry::Tool { tool: "Bash".into(), target: "pnpm test".into() }),
            Update::Entry(Entry::Agent("You chose once.".into())),
        ]
    );
    c.send("again");
    let updates = until(&mut c, "the second turn", |c, u| c.status() == Status::Idle && u.len() >= 2);
    assert_eq!(updates[1], Update::Entry(Entry::Agent("Second turn.".into())));
    c.stop();
    assert!(matches!(c.status(), Status::Exited { .. }));
}

#[test]
fn earlier_session_is_loaded_with_its_history() {
    let mut c = chat(
        "opencode-1.19.6.txt",
        Spec { session: Some("ses_1".into()), prompt: Some("unused".into()), ..Spec::default() },
    );
    let updates = until(&mut c, "the history", |c, u| c.status() == Status::Idle && u.len() >= 2);
    let Update::History(history) = &updates[0] else { panic!("{updates:?}") };
    assert_eq!(history.len(), 4, "{history:?}");
    assert!(matches!(&history[0], Entry::User(text) if text.starts_with("Create a file hello.txt")));
    assert_eq!(updates[1], Update::Session("ses_1".into()));
    assert_eq!(updates.len(), 2, "接回的会话不再发第一句话");
}

#[test]
fn opencode_turn_matches_the_recording() {
    let mut c = chat("opencode-1.19.6.txt", Spec { prompt: Some("Create a file".into()), ..Spec::default() });
    let updates = until(&mut c, "the turn", |c, u| c.status() == Status::Idle && u.len() >= 5);
    assert_eq!(updates[0], Update::Session("ses_def92169effeTIBq4PNH9LCzj9".into()));
    assert_eq!(updates[4], Update::Entry(Entry::Agent("`ls` lists `hello.txt` and `README.md`.".into())));
}

#[test]
fn claude_turn_with_permission() {
    let spec = Spec { protocol: Protocol::Claude, prompt: Some("Create a file".into()), ..Spec::default() };
    let mut c = chat("claude-2.1.311.txt", spec);
    let mut updates = until(&mut c, "the question", |c, _| c.status() == Status::Asking);
    let ask = c.ask().unwrap();
    assert!(ask.tool == "Write" && ask.target.ends_with("hello.txt"), "{ask:?}");
    assert_eq!(ask.options.first().map(String::as_str), Some("Allow"));
    assert!(c.answer(ask.id, 0));
    updates.extend(until(&mut c, "the reply", |c, u| {
        c.status() == Status::Idle && u.iter().any(|u| matches!(u, Update::Entry(Entry::Agent(_))))
    }));
    // 新开的会话，编号要等它收到第一句话才说。
    assert_eq!(updates[0], Update::Entry(Entry::User("Create a file".into())));
    assert_eq!(updates[1], Update::Session("8cf00cda-209b-4645-9d91-5721f9f251b1".into()));
    assert!(matches!(&updates[2], Update::Entry(Entry::Tool { tool, .. }) if tool == "Write"), "{updates:#?}");
    assert!(
        matches!(updates.last(), Some(Update::Entry(Entry::Agent(text))) if text.contains("README.md")),
        "{updates:#?}"
    );
}

#[test]
fn codex_turn_and_resumed_thread() {
    let spec = Spec { protocol: Protocol::Codex, prompt: Some("Create a file".into()), ..Spec::default() };
    let mut c = chat("codex-0.169.1.txt", spec);
    let mut updates = until(&mut c, "the question", |c, _| c.status() == Status::Asking);
    let ask = c.ask().unwrap();
    assert_eq!((ask.tool.as_str(), ask.target.as_str()), ("Bash", "printf %s hi > hello.txt"));
    assert_eq!(ask.options.len(), 3);
    assert!(c.answer(ask.id, 0));
    updates.extend(until(&mut c, "the reply", |c, u| {
        c.status() == Status::Idle && u.iter().any(|u| matches!(u, Update::Entry(Entry::Agent(_))))
    }));
    let thread = "01a212e4-ec52-77d3-a85f-f991647c34b2";
    assert_eq!(updates[0], Update::Session(thread.into()));
    assert!(
        matches!(updates.last(), Some(Update::Entry(Entry::Agent(text))) if text.contains("README.md")),
        "{updates:#?}"
    );
    c.stop();

    let spec = Spec { protocol: Protocol::Codex, session: Some(thread.into()), ..Spec::default() };
    let mut c = chat("codex-0.169.1.txt", spec);
    let updates = until(&mut c, "the history", |c, u| c.status() == Status::Idle && u.len() >= 2);
    let Update::History(history) = &updates[0] else { panic!("{updates:?}") };
    assert!(matches!(&history[0], Entry::User(text) if text.starts_with("Create a file hello.txt")), "{history:?}");
    assert_eq!(updates[1], Update::Session(thread.into()));
}

#[test]
fn grok_turn_with_permission() {
    let mut c = chat("grok-1.0.92.txt", Spec { prompt: Some("Create a file".into()), ..Spec::default() });
    let mut updates = until(&mut c, "the question", |c, _| c.status() == Status::Asking);
    let ask = c.ask().unwrap();
    assert!(ask.tool == "Edit" && ask.target.ends_with("hello.txt"), "{ask:?}");
    assert_eq!(ask.options.len(), 3);
    assert!(c.answer(ask.id, 0));
    updates.extend(until(&mut c, "the reply", |c, u| {
        c.status() == Status::Idle && u.iter().any(|u| matches!(u, Update::Entry(Entry::Agent(_))))
    }));
    assert_eq!(updates[0], Update::Session("01a21310-839f-7a13-b0ab-d4c5e3e5af96".into()));
    assert!(updates.contains(&Update::Entry(Entry::Tool { tool: "Bash".into(), target: "ls".into() })), "{updates:#?}");
}

#[test]
fn failed_start_reports_why() {
    assert!(Chat::spawn(Spec { program: "duolo-no-such-agent".into(), ..Spec::default() }).is_err());
    // 录下来的只有握手，开会话时假 Agent 回 "not recorded"，就像没登录时 Agent 拒绝一样。
    let mut c = chat("handshake-only.txt", Spec::default());
    until(&mut c, "the failure", |c, _| matches!(c.status(), Status::Exited { success: false, .. }));
    assert_eq!(c.error().as_deref(), Some("not recorded"));
}

/// 用真的 Agent 跑一遍：开会话、干活（要授权时都允许）、退出后接回同一个会话再问一句。
/// 要装好、登录好对应的 CLI，手动运行，例如：
/// `cargo test -p duolo-chat --test chat real_claude -- --ignored`
/// 同时设 `DUOLO_CHAT_RECORD=<文件>` 就把收发录下来，整理后放进 `tests/fixtures/` 当测试数据。
fn real_agent(agent: &str) {
    let dir = std::env::temp_dir().join(format!("duolo-chat-{agent}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("README.md"), "# Demo\n").unwrap();
    let (registry, _) = Registry::load(&Config::default(), &dir.join("no-plugins"));
    let launch = registry.get(&AgentKind(agent.into())).unwrap().chat().unwrap();
    let real = |session: Option<String>, prompt: Option<&str>| {
        Chat::spawn(Spec {
            protocol: Protocol::from_name(&launch.protocol).unwrap(),
            program: launch.program.clone(),
            args: launch.args.clone(),
            resume_session_args: launch.resume_session_args.clone(),
            resume_args: launch.resume_args.clone(),
            env: launch.env.clone(),
            env_remove: registry.env_remove(),
            cwd: dir.clone(),
            session,
            prompt: prompt.map(str::to_string),
            ..Spec::default()
        })
        .unwrap()
    };
    let replied = |c: &mut Chat, u: &[Update]| {
        if let Some(ask) = c.ask() {
            c.answer(ask.id, 0);
        }
        c.status() == Status::Idle && u.iter().any(|u| matches!(u, Update::Entry(Entry::Agent(_))))
    };
    let mut c = real(
        None,
        Some("Create a file hello.txt containing 'hi', then run `ls` and tell me the result in one sentence."),
    );
    let updates = until(&mut c, "the first turn", replied);
    let session = updates.iter().find_map(|u| match u {
        Update::Session(id) => Some(id.clone()),
        _ => None,
    });
    let session = session.unwrap_or_else(|| panic!("{updates:#?}"));
    assert!(std::fs::read_to_string(dir.join("hello.txt")).unwrap().contains("hi"), "{updates:#?}");
    assert!(updates.iter().any(|u| matches!(u, Update::Entry(Entry::Tool { .. }))), "{updates:#?}");
    c.stop();

    let mut c = real(Some(session.clone()), None);
    c.send("What file did you create? Answer with only the file name.");
    let updates = until(&mut c, "the second turn", replied);
    assert!(updates.contains(&Update::Session(session)), "{updates:#?}");
    assert!(
        updates.iter().any(|u| matches!(u, Update::Entry(Entry::Agent(text)) if text.contains("hello.txt"))),
        "{updates:#?}"
    );
    c.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
#[ignore = "needs opencode and a model"]
fn real_opencode() {
    real_agent("opencode");
}

#[test]
#[ignore = "needs a logged-in claude"]
fn real_claude() {
    real_agent("claude");
}

#[test]
#[ignore = "needs a logged-in codex"]
fn real_codex() {
    real_agent("codex");
}

#[test]
#[ignore = "needs a logged-in grok"]
fn real_grok() {
    real_agent("grok");
}

#[test]
#[ignore = "needs a logged-in agy"]
fn real_antigravity() {
    real_agent("antigravity");
}
