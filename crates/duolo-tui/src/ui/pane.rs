//! 右侧：Agent 标签、嵌入的 Agent 终端、底栏，以及画在输入行上的语音文字。

use std::time::Duration;

use duolo_engine::AgentStatus;
use duolo_pty::Size;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use tui_term::widget::{Cursor, PseudoTerminal};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::theme;
use crate::app::{App, Mode};
use crate::voice::Phase;

const VOICE_HINT: &str = "⌃Space 说话";

pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    let [tabs, body, bar] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(1), Constraint::Length(1)]).areas(area);
    let term = body.inner(ratatui::layout::Margin { horizontal: 1, vertical: 0 });

    app.set_pane_size(Size::new(term.height, term.width));
    render_tabs(frame, app, tabs);
    render_body(frame, app, term);
    render_bar(frame, app, bar);
}

fn render_tabs(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(task) = app.selected_task().cloned() else { return };
    let selected = app.selected_key();
    let mut spans = vec![Span::raw(" ")];
    for agent in &task.agents {
        let key = task.key(agent.id);
        let (icon, icon_style) = theme::status_icon(app.engine.agent_status(key));
        let label_style = if Some(key) == selected { theme::strong() } else { theme::dim() };
        spans.push(Span::styled(icon, icon_style));
        spans.push(Span::styled(format!(" {} · {}   ", agent.kind.display_name(), agent.role.label()), label_style));
    }

    let mut right = vec![Span::styled(task.branch.clone(), theme::dim())];
    if let Some(stat) = app.engine.diff_stat(task.id).filter(|s| !s.is_empty()) {
        right.push(Span::styled(format!("  +{}", stat.insertions), Style::new().fg(theme::ADD)));
        right.push(Span::styled(format!(" −{}", stat.deletions), Style::new().fg(theme::DEL)));
    }
    right.push(Span::raw(" "));
    let right_width: u16 = right.iter().map(|s| s.content.width() as u16).sum();
    let [left_area, right_area] = Layout::horizontal([Constraint::Min(1), Constraint::Length(right_width)]).areas(area);
    frame.render_widget(Paragraph::new(Line::from(spans)), left_area);
    frame.render_widget(Paragraph::new(Line::from(right)), right_area);
}

fn render_body(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(task) = app.selected_task().cloned() else {
        let lines = if app.engine.state().projects.is_empty() {
            vec![
                Line::styled("还没有项目", theme::muted()),
                Line::styled("在 git 仓库里运行 duolo，或按 a 添加项目", theme::dim()),
            ]
        } else {
            vec![
                Line::styled("还没有任务", theme::muted()),
                Line::styled("按 n 新建任务：描述需求，Duolo 会开一个 worktree 并启动 Agent", theme::dim()),
            ]
        };
        frame.render_widget(Paragraph::new(lines), centered_text_area(area, 2));
        return;
    };
    let Some(key) = app.selected_key() else { return };
    let status = app.engine.agent_status(key);
    let agent = task.agent(key.agent).cloned().expect("选中的 Agent 存在");

    if !status.is_live() {
        let name = agent.kind.display_name();
        let line = match status {
            AgentStatus::Exited(exit) => format!("{name} 已结束（退出码 {}）· 按 Enter 恢复会话", exit.code),
            _ if agent.launched => format!("按 Enter 恢复 {name} 上次的会话"),
            _ => format!("按 Enter 启动 {name}"),
        };
        frame.render_widget(Paragraph::new(Line::styled(line, theme::muted())), centered_text_area(area, 1));
        return;
    }

    let focused = matches!(app.mode, Mode::Normal);
    let Some(session) = app.engine.session(key) else { return };
    let (cursor, cursor_hidden, row_tail_empty) = session.with_screen(|screen| {
        let mut cursor = Cursor::default();
        cursor.hide();
        frame.render_widget(PseudoTerminal::new(screen).cursor(cursor), area);
        let (row, col) = screen.cursor_position();
        let tail_empty = screen
            .rows(col, screen.size().1.saturating_sub(col))
            .nth(usize::from(row))
            .is_some_and(|t| t.trim().is_empty());
        ((row, col), screen.hide_cursor(), tail_empty)
    });
    let scrolled = session.scrollback() > 0;
    let at_cursor = Position::new(area.x + cursor.1, area.y + cursor.0);
    if !area.contains(at_cursor) || scrolled {
        return;
    }

    let buf = frame.buffer_mut();
    match app.voice.phase() {
        Phase::Pending { .. } => {
            if let Some(text) = app.voice.pending_for(key) {
                let end = draw_wrapped(buf, area, at_cursor, text, Style::new().bg(theme::PREEDIT_BG));
                if focused {
                    frame.set_cursor_position(end);
                }
                draw_right(frame.buffer_mut(), area, at_cursor.y, "↵ 发送  Esc 还原", theme::dim());
            }
        }
        Phase::Recording { prefix, .. } => {
            let end = draw_wrapped(buf, area, at_cursor, prefix, Style::new().bg(theme::PREEDIT_BG));
            let secs = app.voice.recording_elapsed().unwrap_or(Duration::ZERO).as_secs();
            draw_right(frame.buffer_mut(), area, end.y, &format!("● {}:{:02}", secs / 60, secs % 60), Style::new());
        }
        Phase::Processing { prefix, .. } => {
            let end = draw_wrapped(buf, area, at_cursor, prefix, Style::new().bg(theme::PREEDIT_BG));
            draw_right(frame.buffer_mut(), area, end.y, "整理中…", theme::dim());
        }
        Phase::Idle => {
            if focused && !cursor_hidden {
                frame.set_cursor_position(at_cursor);
            }
            if focused && status == AgentStatus::Idle && row_tail_empty {
                draw_right(frame.buffer_mut(), area, at_cursor.y, VOICE_HINT, theme::faint());
            }
        }
    }
}

fn render_bar(frame: &mut Frame, app: &mut App, area: Rect) {
    let left = if let Some(flash) = app.flash() {
        let style = if flash.is_error { theme::need() } else { theme::muted() };
        Line::from(Span::styled(format!(" {}", flash.text), style))
    } else {
        match &app.mode {
            Mode::ConfirmClose(_) => Line::styled(
                " 关闭这个任务？会结束它的 Agent 并删除 worktree，分支保留。y 确认  其他键取消",
                theme::need(),
            ),
            Mode::ConfirmQuit => Line::styled(" 还有 Agent 在运行，退出会结束它们。y 退出  其他键取消", theme::need()),
            Mode::Prefix => Line::styled(
                " j/k 切换任务  h/l 切换 Agent  n 新任务  r 启动审阅  x 关闭任务  b 侧边栏  ? 帮助  q 退出",
                theme::muted(),
            ),
            _ => match app.selected_task() {
                Some(task) => Line::styled(format!(" {}", task.worktree.display()), theme::faint()),
                None => Line::default(),
            },
        }
    };
    frame.render_widget(Paragraph::new(left), area);
    if matches!(app.mode, Mode::Normal) {
        draw_right(frame.buffer_mut(), area, area.y, "⌃] 菜单 ", theme::dim());
    }
}

/// 从 `start` 开始写文字，到行尾自动换到下一行。返回文字结束处的位置。
fn draw_wrapped(buf: &mut Buffer, area: Rect, start: Position, text: &str, style: Style) -> Position {
    let mut pos = start;
    let right = area.x + area.width;
    let bottom = area.y + area.height;
    for ch in text.chars() {
        let w = ch.width().unwrap_or(0) as u16;
        if w == 0 {
            continue;
        }
        if pos.x + w > right {
            pos = Position::new(area.x, pos.y + 1);
            if pos.y >= bottom {
                break;
            }
        }
        buf.set_stringn(pos.x, pos.y, ch.to_string(), usize::from(w), style.add_modifier(Modifier::UNDERLINED));
        pos.x += w;
    }
    pos
}

/// 在某一行的最右侧写一段短文字。
fn draw_right(buf: &mut Buffer, area: Rect, y: u16, text: &str, style: Style) {
    let w = text.width() as u16;
    if w + 2 > area.width || y < area.y || y >= area.y + area.height {
        return;
    }
    buf.set_stringn(area.x + area.width - w - 1, y, text, usize::from(w), style);
}

fn centered_text_area(area: Rect, lines: u16) -> Rect {
    let top = area.y + area.height.saturating_sub(lines) / 2;
    Rect::new(area.x + 2, top, area.width.saturating_sub(4), lines.min(area.height))
}
