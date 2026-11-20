//! 把键盘和粘贴事件翻译成 App 上的操作。
//!
//! 规则：
//! - 当前 Agent 在运行时，按键原样交给它；Duolo 只截获 `Ctrl+]`（前缀键）和 `Ctrl+Space`（语音）；
//! - 没有运行中的 Agent 时，单键直接是 Duolo 的操作（n 新任务、a 添加项目、q 退出……）；
//! - 弹出的输入框、确认框打开时，按键都交给它们。

use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::app::{App, Mode};
use crate::keys;
use crate::voice::VoiceTarget;

pub fn handle(app: &mut App, event: Event) {
    match event {
        Event::Key(key) if key.kind != KeyEventKind::Release => on_key(app, key),
        Event::Paste(text) => on_paste(app, &text),
        _ => {}
    }
}

fn on_key(app: &mut App, key: KeyEvent) {
    match std::mem::replace(&mut app.mode, Mode::Normal) {
        Mode::Normal => normal(app, key),
        Mode::Prefix => prefix(app, key),
        Mode::NewTask { mut input, mut project } => {
            if keys::is_voice(&key) {
                app.mode = Mode::NewTask { input, project };
                app.toggle_voice(VoiceTarget::NewTask);
                return;
            }
            match key.code {
                KeyCode::Esc => app.voice.cancel(),
                KeyCode::Enter if !input.is_empty() => {
                    app.submit_new_task(input.text().to_string(), project);
                }
                KeyCode::Tab => {
                    let count = app.engine.state().projects.len().max(1);
                    project = (project + 1) % count;
                    app.mode = Mode::NewTask { input, project };
                }
                _ => {
                    edit_line(&mut input, &key);
                    app.mode = Mode::NewTask { input, project };
                }
            }
        }
        Mode::AddProject { mut input } => match key.code {
            KeyCode::Esc => {}
            KeyCode::Enter if !input.is_empty() => app.submit_add_project(input.text().to_string()),
            _ => {
                edit_line(&mut input, &key);
                app.mode = Mode::AddProject { input };
            }
        },
        Mode::ConfirmClose(id) => {
            if matches!(key.code, KeyCode::Char('y' | 'Y')) {
                app.close_task(id);
            }
        }
        Mode::ConfirmQuit => {
            if matches!(key.code, KeyCode::Char('y' | 'Y')) {
                app.quit_now();
            }
        }
        Mode::Help => {}
    }
}

/// 普通模式：有运行中的 Agent 时转发按键，否则按 Duolo 的单键操作处理。
fn normal(app: &mut App, key: KeyEvent) {
    if keys::is_prefix(&key) {
        app.mode = Mode::Prefix;
        return;
    }

    let Some(agent) = app.focused_live_key() else {
        idle_shortcuts(app, key);
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
    // 录音中按 Esc 取消录音。
    if app.voice.recording_elapsed().is_some() && key.code == KeyCode::Esc {
        app.voice.cancel();
        return;
    }

    // Shift+PageUp/PageDown 查看回滚，不发给 Agent。
    if key.modifiers.contains(KeyModifiers::SHIFT) && matches!(key.code, KeyCode::PageUp | KeyCode::PageDown) {
        if let Some(session) = app.engine.session(agent) {
            let rows = usize::from(session.size().rows.saturating_sub(2)).max(1);
            let offset = session.scrollback();
            let next = if key.code == KeyCode::PageUp { offset + rows } else { offset.saturating_sub(rows) };
            session.set_scrollback(next);
        }
        return;
    }

    let Some(session) = app.engine.session_mut(agent) else { return };
    if session.scrollback() > 0 {
        session.set_scrollback(0);
    }
    let app_cursor = session.with_screen(|s| s.application_cursor());
    if let Some(bytes) = keys::encode(&key, app_cursor) {
        if let Err(err) = session.write(&bytes) {
            app.error(format!("{err:#}"));
        }
    }
}

/// 没有运行中的 Agent 时的单键操作。
fn idle_shortcuts(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Enter => app.start_selected_agent(),
        KeyCode::Char('n') => app.open_new_task(),
        KeyCode::Char('a') => app.mode = Mode::AddProject { input: Default::default() },
        KeyCode::Char('r') => app.start_reviewer(),
        KeyCode::Char('x') => {
            if let Some(task) = app.selected_task() {
                app.mode = Mode::ConfirmClose(task.id);
            }
        }
        KeyCode::Char('j') | KeyCode::Down => app.move_task(1),
        KeyCode::Char('k') | KeyCode::Up => app.move_task(-1),
        KeyCode::Char('l') | KeyCode::Right => app.move_agent(1),
        KeyCode::Char('h') | KeyCode::Left => app.move_agent(-1),
        KeyCode::Char('b') => app.sidebar_visible = !app.sidebar_visible,
        KeyCode::Char('?') => app.mode = Mode::Help,
        KeyCode::Char('q') => app.request_quit(),
        _ => {}
    }
}

/// `Ctrl+]` 之后的一个键。
fn prefix(app: &mut App, key: KeyEvent) {
    if keys::is_prefix(&key) {
        // 连按两次：把 Ctrl+] 本身发给 Agent。
        if let Some(agent) = app.focused_live_key() {
            if let Some(session) = app.engine.session_mut(agent) {
                let _ = session.write(&[0x1d]);
            }
        }
        return;
    }
    match key.code {
        KeyCode::Char(c @ '1'..='9') => app.select_nth_task(c as usize - '1' as usize),
        KeyCode::Char('s') => app.start_selected_agent(),
        KeyCode::Esc => {}
        _ => idle_shortcuts(app, key),
    }
}

fn on_paste(app: &mut App, text: &str) {
    match &mut app.mode {
        Mode::NewTask { input, .. } | Mode::AddProject { input } => input.insert_str(text),
        Mode::Normal => {
            if let Some(agent) = app.focused_live_key() {
                if let Some(session) = app.engine.session_mut(agent) {
                    let _ = session.paste(text);
                }
            }
        }
        _ => {}
    }
}

fn edit_line(input: &mut crate::line_input::LineInput, key: &KeyEvent) {
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
