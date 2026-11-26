//! 把键盘、鼠标和粘贴事件翻译成 App 上的操作。
//!
//! 规则：
//! - `⌃]` 在右侧和侧边栏之间切换焦点；
//! - 侧边栏：↑↓ 选择、↵ 打开，单键执行操作（n 新任务、r 审阅、x 关闭……）；
//! - 右侧首页：直接打字新建任务；
//! - 右侧 Agent 在运行时，按键原样交给它，Duolo 只截获 `⌃]` 和 `⌃Space`（语音）；
//! - 鼠标点击侧边栏打开对应的行，在 Agent 窗格滚动查看历史输出。

use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::layout::Position;

use crate::app::{App, Focus, HandoffKind, Mode};
use crate::keys;
use crate::line_input::LineInput;
use crate::voice::VoiceTarget;

/// 滚轮每格滚动的行数。
const WHEEL_LINES: isize = 3;

pub fn handle(app: &mut App, event: Event) {
    match event {
        Event::Key(key) if key.kind != KeyEventKind::Release => on_key(app, key),
        Event::Paste(text) => on_paste(app, &text),
        Event::Mouse(mouse) => {
            let at = Position::new(mouse.column, mouse.row);
            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => on_click(app, at),
                MouseEventKind::ScrollUp if app.pane_area.contains(at) => app.scroll(WHEEL_LINES),
                MouseEventKind::ScrollDown if app.pane_area.contains(at) => app.scroll(-WHEEL_LINES),
                _ => {}
            }
        }
        _ => {}
    }
}

fn on_key(app: &mut App, key: KeyEvent) {
    match app.mode {
        Mode::ConfirmClose(id) => {
            app.mode = Mode::Normal;
            if matches!(key.code, KeyCode::Char('y' | 'Y') | KeyCode::Enter) {
                app.close_task(id);
            }
        }
        Mode::ConfirmQuit => {
            app.mode = Mode::Normal;
            if matches!(key.code, KeyCode::Char('y' | 'Y') | KeyCode::Enter) {
                app.quit_now();
            }
        }
        Mode::Help => app.mode = Mode::Normal,
        Mode::Handoff => handoff(app, key),
        Mode::Normal => {
            if keys::is_prefix(&key) {
                match app.focus {
                    Focus::Main => app.focus_sidebar(),
                    Focus::Sidebar => app.focus_main(),
                }
                return;
            }
            match app.focus {
                Focus::Sidebar => sidebar(app, key),
                Focus::Main if app.is_home() => home(app, key),
                Focus::Main => agent(app, key),
            }
        }
    }
}

fn sidebar(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => app.move_cursor(-1),
        KeyCode::Down | KeyCode::Char('j') => app.move_cursor(1),
        KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => app.activate(app.cursor),
        KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') => app.focus_main(),
        KeyCode::Char('n') => app.new_task(),
        KeyCode::Char('a') => app.add_project(),
        KeyCode::Char('r') => app.start_reviewer(),
        KeyCode::Char('t') => app.open_handoff(),
        KeyCode::Char('x') => app.request_close(),
        KeyCode::Char(c @ '1'..='9') => app.select_nth_task(c as usize - '1' as usize),
        KeyCode::Char('b') => {
            app.sidebar_visible = false;
            app.focus_main();
        }
        KeyCode::Char('?') => app.mode = Mode::Help,
        KeyCode::Char('q') => app.request_quit(),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => app.request_quit(),
        _ => {}
    }
}

/// 首页输入框。
/// 「交给另一个 Agent」表单：←→ 谁来，↑↓ 做什么，↵ 交出去，Esc 取消，其余按键写补充的一句话（审阅时不用写）。
fn handoff(app: &mut App, key: KeyEvent) {
    let Some(form) = app.handoff.as_mut() else {
        app.mode = Mode::Normal;
        return;
    };
    let step = |i: usize, len: usize, back: bool| if back { (i + len - 1) % len } else { (i + 1) % len };
    match key.code {
        KeyCode::Esc => app.close_handoff(),
        KeyCode::Enter => app.submit_handoff(),
        KeyCode::Left | KeyCode::Right => form.agent = step(form.agent, form.agents.len(), key.code == KeyCode::Left),
        KeyCode::Up | KeyCode::Down => {
            let all = HandoffKind::ALL;
            let at = all.iter().position(|k| *k == form.kind).unwrap_or(0);
            form.kind = all[step(at, all.len(), key.code == KeyCode::Up)];
        }
        _ if form.kind != HandoffKind::Review => edit_line(&mut form.request, &key),
        _ => {}
    }
}

fn home(app: &mut App, key: KeyEvent) {
    if keys::is_voice(&key) {
        app.toggle_voice(VoiceTarget::Home);
        return;
    }
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Enter => app.submit_home(),
        KeyCode::Tab if !app.home.adding_project => app.cycle_home_project(),
        KeyCode::Esc => {
            if app.voice.recording_elapsed().is_some() {
                app.voice.cancel();
            } else {
                app.leave_home();
            }
        }
        KeyCode::Char('c') if ctrl => {
            if app.home.input.is_empty() {
                app.request_quit();
            } else {
                app.home.input = LineInput::default();
            }
        }
        // 输入框为空时，上下键直接进入侧边栏选任务。
        KeyCode::Up | KeyCode::Down if app.home.input.is_empty() => {
            app.focus_sidebar();
            app.move_cursor(if key.code == KeyCode::Up { -1 } else { 1 });
        }
        _ => edit_line(&mut app.home.input, &key),
    }
}

/// 右侧是某个 Agent。
fn agent(app: &mut App, key: KeyEvent) {
    let Some(agent) = app.focused_live_key() else {
        // Agent 没在运行：回车启动或恢复，其余按键不起作用。
        if key.code == KeyCode::Enter {
            app.ensure_selected_agent_running();
        }
        return;
    };

    if keys::is_voice(&key) {
        app.toggle_voice(VoiceTarget::Agent(agent));
        return;
    }

    // 有待确认的语音文字时，按键先作用于它。
    if app.voice.pending_for(agent).is_some() {
        match key.code {
            KeyCode::Enter => app.commit_voice(agent, true),
            KeyCode::Tab => app.commit_voice(agent, false),
            KeyCode::Esc => app.voice.revert_or_cancel(),
            KeyCode::Backspace => app.voice.edit_pending(|t| {
                t.pop();
            }),
            KeyCode::Char(c) if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
                app.voice.edit_pending(|t| t.push(c));
            }
            _ => {}
        }
        return;
    }
    if app.voice.recording_elapsed().is_some() && key.code == KeyCode::Esc {
        app.voice.cancel();
        return;
    }

    // Shift+PageUp/PageDown 查看历史输出，不发给 Agent。
    if key.modifiers.contains(KeyModifiers::SHIFT) && matches!(key.code, KeyCode::PageUp | KeyCode::PageDown) {
        let page = app.engine.session(agent).map_or(10, |s| (s.size().rows.saturating_sub(2) as isize).max(1));
        app.scroll(if key.code == KeyCode::PageUp { page } else { -page });
        return;
    }

    let Some(session) = app.engine.session_mut(agent) else { return };
    if session.scrollback() > 0 {
        session.set_scrollback(0);
    }
    let app_cursor = session.with_screen(|s| s.application_cursor());
    if let Some(bytes) = keys::encode(&key, app_cursor) {
        if let Err(err) = session.write(&bytes) {
            app.error(duolo_core::error::describe(&err));
        }
    }
}

fn on_click(app: &mut App, at: Position) {
    if app.mode != Mode::Normal {
        return;
    }
    if app.sidebar_area.contains(at) {
        if let Some(item) = app.sidebar_hits.iter().find(|(y, _)| *y == at.y).map(|(_, item)| *item) {
            app.activate(item);
        }
    } else if app.pane_area.contains(at) {
        app.focus_main();
    }
}

fn on_paste(app: &mut App, text: &str) {
    if app.mode != Mode::Normal || app.focus != Focus::Main {
        return;
    }
    if app.is_home() {
        app.home.input.insert_str(text);
    } else if let Some(agent) = app.focused_live_key() {
        if let Some(session) = app.engine.session_mut(agent) {
            let _ = session.paste(text);
        }
    }
}

fn edit_line(input: &mut LineInput, key: &KeyEvent) {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Char('u') if ctrl => input.clear_before_cursor(),
        KeyCode::Char('a') if ctrl => input.home(),
        KeyCode::Char('e') if ctrl => input.end(),
        KeyCode::Char(c) if !ctrl => input.insert(c),
        KeyCode::Backspace => input.backspace(),
        KeyCode::Delete => input.delete(),
        KeyCode::Left => input.left(),
        KeyCode::Right => input.right(),
        KeyCode::Home => input.home(),
        KeyCode::End => input.end(),
        _ => {}
    }
}
