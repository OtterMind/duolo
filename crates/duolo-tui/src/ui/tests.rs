//! 画面测试：把各个界面画到内存里，检查关键内容，也方便用 `--nocapture` 直接查看画面。

use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use duolo_core::Paths;
use duolo_engine::Engine;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;

use crate::app::{App, Focus};
use crate::input;

struct Fixture {
    _dir: tempfile::TempDir,
    repo: std::path::PathBuf,
    app: App,
}

fn git(dir: &Path, args: &[&str]) {
    assert!(Command::new("git").arg("-C").arg(dir).args(args).status().unwrap().success());
}

/// 一个临时仓库 + 临时数据目录，Agent 用 `sh -c cat` 冒充。
fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("shop");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "init"]);
    let home = dir.path().join("home");
    std::fs::create_dir(&home).unwrap();
    std::fs::write(
        home.join("config.toml"),
        "[agents.claude]\ncommand = \"sh\"\nargs = [\"-c\", \"printf '> '; cat\", \"agent\"]\n\
         [agents.codex]\ncommand = \"sh\"\nargs = [\"-c\", \"printf 'review> '; cat\", \"agent\"]\n",
    )
    .unwrap();
    let engine = Engine::open(Paths::at(&home)).unwrap();
    let app = App::new(engine, &repo);
    Fixture { _dir: dir, repo, app }
}

fn draw(app: &mut App) -> String {
    let mut terminal = Terminal::new(TestBackend::new(110, 24)).unwrap();
    terminal.draw(|f| super::render(f, app)).unwrap();
    let buf = terminal.backend().buffer().clone();
    let mut out = String::new();
    for y in 0..buf.area.height {
        let mut line = String::new();
        let mut x = 0;
        while x < buf.area.width {
            let symbol = buf[(x, y)].symbol();
            line.push_str(symbol);
            // 中文等宽字符占两格，跳过它后面的占位格。
            x += (unicode_width::UnicodeWidthStr::width(symbol) as u16).max(1);
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

fn press(app: &mut App, code: KeyCode) {
    input::handle(app, Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
}

fn ctrl(app: &mut App, c: char) {
    input::handle(app, Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)));
}

fn type_text(app: &mut App, text: &str) {
    for c in text.chars() {
        press(app, KeyCode::Char(c));
    }
}

fn wait_for(app: &mut App, mut f: impl FnMut(&str) -> bool) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let screen = draw(app);
        if f(&screen) || Instant::now() > deadline {
            return screen;
        }
        thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn first_launch_lands_on_home_input() {
    let mut fx = fixture();
    let screen = wait_for(&mut fx.app, |s| !s.contains("正在检查环境"));
    println!("{screen}");
    assert!(fx.app.is_home());
    assert!(screen.contains("+ 新任务"));
    assert!(screen.contains("今天让 Agent 做点什么"));
    assert!(screen.contains("新任务 · shop"));
    assert!(screen.contains("描述你想做的事"));
    assert!(screen.contains("+ 添加项目"));
}

#[test]
fn typing_on_home_creates_and_opens_task() {
    let mut fx = fixture();
    type_text(&mut fx.app, "Add order export");
    assert!(draw(&mut fx.app).contains("› Add order export"));
    press(&mut fx.app, KeyCode::Enter);
    let screen = wait_for(&mut fx.app, |s| s.contains("> "));
    println!("{screen}");
    assert!(!fx.app.is_home());
    assert_eq!(fx.app.focus, Focus::Main);
    assert!(screen.contains("Add order export  │"));
    assert!(screen.contains("Claude 写"));
    assert!(screen.contains("duolo/1-add-order-export"));
    assert!(fx.repo.parent().unwrap().join("home/wt/shop/1-add-order-export").exists());
}

#[test]
fn sidebar_is_keyboard_operable() {
    let mut fx = fixture();
    type_text(&mut fx.app, "first task");
    press(&mut fx.app, KeyCode::Enter);

    // ⌃] 进入侧边栏，n 回到首页新建任务。
    ctrl(&mut fx.app, ']');
    assert_eq!(fx.app.focus, Focus::Sidebar);
    let screen = draw(&mut fx.app);
    println!("{screen}");
    assert!(screen.contains("↑↓ 选择"));
    press(&mut fx.app, KeyCode::Char('n'));
    assert!(fx.app.is_home());
    assert_eq!(fx.app.focus, Focus::Main);

    // 回到侧边栏，向下选到任务并打开。
    ctrl(&mut fx.app, ']');
    press(&mut fx.app, KeyCode::Down);
    press(&mut fx.app, KeyCode::Enter);
    assert!(!fx.app.is_home());

    // r 启动审阅，x 关闭任务需要确认。
    ctrl(&mut fx.app, ']');
    press(&mut fx.app, KeyCode::Char('r'));
    let screen = wait_for(&mut fx.app, |s| s.contains("review>"));
    println!("{screen}");
    assert!(screen.contains("Codex 审"));
    ctrl(&mut fx.app, ']');
    press(&mut fx.app, KeyCode::Char('x'));
    assert!(draw(&mut fx.app).contains("关闭「first task」"));
    press(&mut fx.app, KeyCode::Char('y'));
    assert!(fx.app.is_home(), "关闭最后一个任务后回到首页");
}

#[test]
fn add_project_from_home() {
    let mut fx = fixture();
    ctrl(&mut fx.app, ']');
    press(&mut fx.app, KeyCode::Char('a'));
    let screen = draw(&mut fx.app);
    println!("{screen}");
    assert!(screen.contains("添加一个项目"));
    type_text(&mut fx.app, "/definitely/not/a/repo");
    press(&mut fx.app, KeyCode::Enter);
    assert!(draw(&mut fx.app).contains("not inside a git repository"));
    press(&mut fx.app, KeyCode::Esc);
    assert!(!fx.app.home.adding_project);
}

#[test]
fn help_overlay() {
    let mut fx = fixture();
    ctrl(&mut fx.app, ']');
    press(&mut fx.app, KeyCode::Char('?'));
    let screen = draw(&mut fx.app);
    println!("{screen}");
    assert!(screen.contains("快捷键"));
    press(&mut fx.app, KeyCode::Char('z'));
    assert!(!draw(&mut fx.app).contains("快捷键"));
}
