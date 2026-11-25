//! 用录下来的真实收发跑一遍聊天模式：测试程序自己当假 Agent（见 `duolo_chat::testing`）。

use std::path::Path;
use std::time::{Duration, Instant};

use duolo_chat::testing::{serve_if_asked, FIXTURE_ENV};
use duolo_chat::{Chat, Entry, Spec, Status, Update};

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
fn failed_start_reports_why() {
    assert!(Chat::spawn(Spec { program: "duolo-no-such-agent".into(), ..Spec::default() }).is_err());
    // 录下来的只有握手，开会话时假 Agent 回 "not recorded"，就像没登录时 Agent 拒绝一样。
    let mut c = chat("handshake-only.txt", Spec::default());
    until(&mut c, "the failure", |c, _| matches!(c.status(), Status::Exited { success: false, .. }));
    assert_eq!(c.error().as_deref(), Some("not recorded"));
}

/// 真的 OpenCode：开会话、干活、退出后接回同一个会话再说一句。要装好 `opencode`、能连模型，手动运行：
/// `cargo test -p duolo-chat --test acp real_opencode -- --ignored`
#[test]
#[ignore = "needs opencode and a model"]
fn real_opencode() {
    let dir = std::env::temp_dir().join(format!("duolo-acp-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let real = |session: Option<String>, prompt: &str| {
        Chat::spawn(Spec {
            program: "opencode".into(),
            args: vec!["acp".into()],
            env: vec![("OPENCODE_DISABLE_AUTOUPDATE".into(), "true".into())],
            cwd: dir.clone(),
            session,
            prompt: Some(prompt.into()),
            ..Spec::default()
        })
        .unwrap()
    };
    let mut c = real(None, "Create a file named note.txt containing the word duolo. Reply in one short sentence.");
    let updates = until(&mut c, "the first turn", |c, u| {
        c.status() == Status::Idle && u.iter().any(|u| matches!(u, Update::Entry(Entry::Agent(_))))
    });
    let Update::Session(session) = updates[0].clone() else { panic!("{updates:?}") };
    assert!(std::fs::read_to_string(dir.join("note.txt")).unwrap().contains("duolo"));
    c.stop();

    let mut c = real(Some(session.clone()), "unused");
    c.send("What file did you create? Answer with the file name only.");
    let updates = until(&mut c, "the second turn", |c, u| {
        c.status() == Status::Idle && u.iter().filter(|u| matches!(u, Update::Entry(Entry::Agent(_)))).count() >= 1
    });
    assert!(matches!(&updates[0], Update::History(h) if !h.is_empty()), "{updates:?}");
    assert_eq!(updates[1], Update::Session(session));
    assert!(updates.iter().any(|u| matches!(u, Update::Entry(Entry::Agent(text)) if text.contains("note.txt"))), "{updates:#?}");
    c.stop();
    let _ = std::fs::remove_dir_all(&dir);
}
