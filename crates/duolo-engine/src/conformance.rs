//! 每个内置的 Agent 插件都要通过的检查。加新插件时在它的 `fixtures/` 里放上真实的画面和会话记录，这里不用改
//! （新的会话记录格式除外：在 [`session_file`] 里写明放在哪里）。

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use duolo_agent::{Entry, Home, Plugin, Registry, Start};
use duolo_core::process::Prompted;
use duolo_core::{AgentRole, Config};
use duolo_pty::{classify, AgentState, Rules};

use crate::prompt::{is_choosing, parse_prompt};

const QUIET: Duration = Duration::from_secs(10);

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn builtin() -> Registry {
    Registry::load(&Config::default(), Path::new("no-such-dir")).0
}

fn fixture(plugin: &Plugin, name: &str) -> String {
    let path = repo().join("crates/duolo-agent/agents").join(plugin.id()).join("fixtures").join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display())).replace("\r\n", "\n")
}

fn state(plugin: &Plugin, name: &str) -> AgentState {
    let screen = plugin.screen();
    let rules = Rules { approval: screen.approval.clone(), busy: screen.busy.clone() };
    classify(&rules, &fixture(plugin, name), QUIET)
}

#[test]
fn every_folder_is_a_builtin_plugin() {
    let mut folders: Vec<String> = std::fs::read_dir(repo().join("crates/duolo-agent/agents"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    let registry = builtin();
    let mut ids: Vec<String> = registry.iter().map(|p| p.id().to_string()).collect();
    folders.sort();
    ids.sort();
    assert_eq!(folders, ids);
}

#[test]
fn has_what_the_interface_shows() {
    for p in builtin().iter() {
        assert!(!p.install().is_empty(), "{} 没有安装命令", p.id());
        let icon = p.icon().unwrap();
        assert!(icon.starts_with("<svg") && icon.contains("viewBox") && icon.contains("xmlns"), "{} 的图标", p.id());
        assert!(!icon.contains("<script"), "{} 的图标", p.id());
        assert!(!p.keys().interrupt.is_empty() && !p.keys().newline.is_empty(), "{} 的按键", p.id());
        let screen = p.screen();
        for word in screen.approval.iter().chain(&screen.busy).chain(&screen.choosing) {
            assert_eq!(*word, word.to_lowercase(), "{}：画面文字按小写比较", p.id());
            assert!(!word.trim().is_empty(), "{}：空的会匹配所有画面", p.id());
        }
    }
}

#[test]
fn prompt_arrives_whole_and_is_never_an_option() {
    let prompts = ["fix it", "- fix the login\n- add tests", "--help", "@notes do this", "导出订单 \"CSV\" & 100%"];
    for p in builtin().iter() {
        for prompt in prompts {
            let launch = p.launch(Start { role: AgentRole::Writer, resume: false, prompt: Some(prompt) });
            match launch.paste {
                Some(paste) => assert_eq!(paste, prompt),
                None => {
                    let last = launch.args.last().unwrap();
                    assert!(last.ends_with(prompt), "{}: {:?}", p.id(), launch.args);
                    let guarded = launch.args.len() > 1 && launch.args[launch.args.len() - 2] == "--";
                    assert!(guarded || !last.starts_with('-') || last.len() > prompt.len(), "{}: {last:?}", p.id());
                }
            }
        }
        let blank = p.launch(Start { role: AgentRole::Writer, resume: false, prompt: None });
        assert!(blank.paste.is_none() && !blank.args.iter().any(|a| a == "--"), "{}: {:?}", p.id(), blank.args);
        let resumed = p.launch(Start { role: AgentRole::Writer, resume: true, prompt: None });
        assert_ne!(resumed.args, blank.args, "{} 不能恢复会话", p.id());
    }
}

#[test]
fn recognizes_its_own_screens() {
    for p in builtin().iter() {
        let id = p.id();
        let choosing = |name: &str| is_choosing(&fixture(p, name), &p.screen().choosing);
        assert_eq!(state(p, "approval.txt"), AgentState::NeedsInput, "{id}");
        let prompt = parse_prompt(&fixture(p, "approval.txt")).unwrap_or_else(|| panic!("{id}：认不出确认框的选项"));
        assert!(prompt.options.len() >= 2 && !prompt.question.is_empty(), "{id}: {prompt:?}");
        assert!(choosing("approval.txt") && choosing("dialog.txt"), "{id}：这时不能发消息");
        assert_eq!(state(p, "dialog.txt"), AgentState::NeedsInput, "{id}");
        assert_eq!(state(p, "busy.txt"), AgentState::Running, "{id}");
        assert_eq!(state(p, "idle.txt"), AgentState::Idle, "{id}");
        assert!(!choosing("idle.txt") && parse_prompt(&fixture(p, "idle.txt")).is_none(), "{id}");
    }
}

/// 后台命令的最后一个选项可能会吞掉后面的参数（`--allowedTools <tools...>`），和提示词之间要隔开。
#[test]
fn background_prompts_are_kept_apart_from_options() {
    for p in builtin().iter() {
        for command in [p.review(), p.quick()].into_iter().flatten() {
            assert!(command.separator.is_some(), "{}: {:?}", p.id(), command.argv);
            assert_eq!(command.argv[0], p.command());
        }
    }
}

/// `fixtures/session.jsonl` 要放在哪里，相对 Agent 的家目录。
fn session_file(id: &str, worktree: &str) -> PathBuf {
    match id {
        "claude" => Path::new(".claude/projects").join(worktree.replace('/', "-")).join("s.jsonl"),
        "codex" => PathBuf::from(".codex/sessions/2026/11/22/rollout-s.jsonl"),
        other => panic!("没写明 {other} 的会话记录放在哪里"),
    }
}

#[test]
fn reads_its_own_conversation() {
    // 2100 年，一定晚于启动的时间；和真实的一样是 UTC。
    let now = "2100-01-01T00:00:00.000Z";
    assert!(SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() < 4_102_444_800);
    let worktree = "/w/shop/1-export";
    for p in builtin().iter() {
        let home = tempfile::tempdir().unwrap();
        let file = home.path().join(session_file(p.id(), worktree));
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let text = fixture(p, "session.jsonl").replace("{now}", now).replace("{worktree}", worktree);
        std::fs::write(&file, text).unwrap();

        let watch = p.watch(Path::new(worktree), &Home::At(home.path().to_path_buf())).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut entries = Vec::new();
        while entries.len() < 3 {
            assert!(Instant::now() < deadline, "{}: {entries:?}", p.id());
            entries.extend(watch.take());
            std::thread::sleep(Duration::from_millis(50));
        }
        let expected = [
            Entry::User("Export orders".into()),
            Entry::Agent("On it.".into()),
            Entry::Tool { tool: "Edit".into(), target: "src/orders.ts".into() },
        ];
        assert_eq!(entries, expected, "{}", p.id());
    }
}

/// 除了插件自己和默认配置，代码里不出现产品的名字：要按 Agent 区分的事写进插件的声明。
#[test]
fn only_plugins_know_product_names() {
    let allowed = ["duolo-agent", "duolo-core/src/config.rs", "duolo-engine/src/conformance.rs"];
    let mut sources = Vec::new();
    let mut dirs = vec![repo().join("crates"), repo().join("app/src")];
    while let Some(dir) = dirs.pop() {
        for path in std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()) {
            if path.is_dir() && !path.ends_with("target") && !path.ends_with("gen") {
                dirs.push(path);
            } else if path.extension().is_some_and(|e| e == "rs" || e == "ts" || e == "tsx") {
                sources.push(path);
            }
        }
    }
    assert!(sources.len() > 60);
    for path in sources {
        let name = path.display().to_string().replace('\\', "/");
        if allowed.iter().any(|a| name.contains(a)) {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let code: Vec<&str> = text
            .split("#[cfg(test)]")
            .next()
            .unwrap()
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect();
        let code = code.join("\n");
        for plugin in builtin().iter() {
            let quoted = format!("\"{}\"", plugin.id());
            assert!(!code.contains(&quoted), "{name} 里写了 {quoted}");
        }
    }
}

/// 本机装了的 Agent：真的运行一次，确认参数都认得（没登录，所以只会停在登录上，不花额度）。
/// `cargo test -p duolo-engine real_commands -- --ignored --nocapture`
#[test]
#[ignore = "要装好各个 Agent"]
fn real_commands_accept_our_arguments() {
    let home = tempfile::tempdir().unwrap();
    for p in builtin().iter().filter(|p| p.is_installed()) {
        for (what, command) in [("review", p.review()), ("quick", p.quick())] {
            let Some(command) = command else { continue };
            let mut prompted = Prompted::new(&command, Some("- list\n- of things")).unwrap();
            prompted.cmd.current_dir(home.path());
            for var in ["HOME", "USERPROFILE", "CLAUDE_CONFIG_DIR", "CODEX_HOME"] {
                prompted.cmd.env(var, home.path());
            }
            let log = home.path().join("log");
            let file = std::fs::File::create(&log).unwrap();
            prompted.cmd.stdout(file.try_clone().unwrap()).stderr(file);
            let (mut child, tree) = prompted.spawn().unwrap();
            let deadline = Instant::now() + Duration::from_secs(8);
            while child.try_wait().unwrap().is_none() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(100));
            }
            tree.kill(None, Duration::from_secs(2));
            let _ = child.kill();
            let out = std::fs::read_to_string(&log).unwrap_or_default();
            println!("== {} {what}\n{out}", p.id());
            for sign in ["unknown option", "unexpected argument", "invalid value", "Input must be provided", "Usage:"] {
                assert!(!out.contains(sign), "{} {what}: {out}", p.id());
            }
            assert!(!out.trim().is_empty(), "{} {what} 什么都没说", p.id());
        }
    }
}
