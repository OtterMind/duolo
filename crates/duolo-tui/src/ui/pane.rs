//! 右侧：首页输入框，或当前任务（标题、Agent 标签、Agent 终端）；最下面一行是提示栏。

use std::time::Duration;

use duolo_core::{t, tf, Surface};
use duolo_engine::AgentStatus;
use duolo_pty::Size;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Margin, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use tui_term::widget::{Cursor, PseudoTerminal};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::sidebar::hint_line;
use super::theme;
use crate::app::{App, Focus, Mode};
use crate::voice::Phase;

/// 启动后多久内画面为空时显示"正在启动"。
const STARTING_GRACE: Duration = Duration::from_secs(8);

pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    app.pane_area = area;
    let [main, bar] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
    if app.is_home() {
        render_home(frame, app, main);
    } else {
        render_task(frame, app, main);
    }
    render_bar(frame, app, bar);
}

// ---- 首页 ----

fn render_home(frame: &mut Frame, app: &mut App, area: Rect) {
    let area = area.inner(Margin { horizontal: 2, vertical: 0 });
    let [content, input] = Layout::vertical([Constraint::Min(1), Constraint::Length(5)]).areas(area);

    let adding = app.home.adding_project;
    let (config, agents) = (app.engine.config(), app.engine.agents());
    let mut lines = if adding {
        vec![
            Line::styled(t("tui.addProjectTitle"), theme::strong()),
            Line::default(),
            Line::styled(t("tui.addProjectBody1"), theme::dim()),
            Line::styled(t("tui.addProjectBody2"), theme::dim()),
        ]
    } else {
        vec![
            Line::styled(t("tui.homeTitle"), theme::strong()),
            Line::default(),
            Line::styled(
                tf(
                    "tui.homeBody",
                    &[("writer", &agents.name(&config.writer)), ("reviewer", &agents.name(&config.reviewer))],
                ),
                theme::dim(),
            ),
        ]
    };
    lines.push(Line::default());
    lines.push(env_line(app));
    let top = content.y + content.height.saturating_sub(lines.len() as u16) / 2;
    let text_area = Rect::new(content.x, top, content.width, (lines.len() as u16).min(content.height));
    frame.render_widget(Paragraph::new(lines), text_area);

    render_home_input(frame, app, input);
}

fn env_line(app: &App) -> Line<'static> {
    let Some(checks) = app.env_checks() else {
        return Line::styled(t("tui.checkingEnv"), theme::faint());
    };
    let mut spans = Vec::new();
    for check in checks {
        if !spans.is_empty() {
            spans.push(Span::raw("    "));
        }
        let voice = check.kind == duolo_engine::CheckKind::Voice;
        let (icon, style) = match (check.ok, voice) {
            (true, _) => ("✓", theme::dim()),
            (false, true) => ("○", theme::faint()),
            (false, false) => ("✕", theme::need()),
        };
        spans.push(Span::styled(format!("{icon} {}", check.name), style));
        if !check.detail.is_empty() {
            spans.push(Span::styled(format!(" {}", check.detail), theme::faint()));
        }
    }
    Line::from(spans)
}

fn render_home_input(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::new().borders(Borders::TOP).border_style(theme::faint());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let [title, input, _, hints] = Layout::vertical([Constraint::Length(1); 4]).areas(inner);

    // 第一行：输入框是做什么的、在哪个项目；录音状态在最右。
    let mut spans = if app.home.adding_project {
        vec![Span::styled(t("tui.addProjectLabel"), theme::muted())]
    } else {
        let project = app.home_project_name().unwrap_or("").to_string();
        let mut s = vec![Span::styled(tf("tui.newTaskIn", &[("project", &project)]), theme::muted())];
        if app.engine.state().projects.len() > 1 {
            s.push(Span::styled(format!("  ⇥ {}", t("tui.hint.switch")), theme::faint()));
        }
        s
    };
    if let Some(status) = voice_status(app) {
        spans.push(Span::raw("   "));
        spans.push(status);
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), title);

    // 第二行：输入内容或占位提示。
    let text = app.home.input.text();
    let line = if text.is_empty() {
        let placeholder = if app.home.adding_project { t("tui.repoPlaceholder") } else { t("tui.promptPlaceholder") };
        Line::from(vec![Span::styled("› ", theme::muted()), Span::styled(placeholder, theme::faint())])
    } else {
        Line::from(vec![Span::styled("› ", theme::muted()), Span::raw(text.to_string())])
    };
    frame.render_widget(Paragraph::new(line), input);
    if app.focus == Focus::Main && app.mode == Mode::Normal {
        let x = input.x + 2 + app.home.input.before_cursor().width() as u16;
        if x < input.x + input.width {
            frame.set_cursor_position(Position::new(x, input.y));
        }
    }

    // 第四行：按键提示。
    let enter = if app.home.adding_project { t("tui.hint.add") } else { t("tui.hint.createStart") };
    let (speak, back, sidebar) = (t("tui.hint.speak"), t("tui.hint.back"), t("tui.hint.sidebar"));
    let mut items: Vec<(&str, &str)> = vec![("↵", &enter), ("⌃Space", &speak)];
    if app.has_previous_task() || (app.home.adding_project && !app.engine.state().projects.is_empty()) {
        items.push(("Esc", &back));
    }
    items.push(("⌃]", &sidebar));
    frame.render_widget(Paragraph::new(hint_line(&items)), hints);
}

fn voice_status(app: &App) -> Option<Span<'static>> {
    match app.voice.phase() {
        Phase::Recording { .. } => {
            let secs = app.voice.recording_elapsed().unwrap_or(Duration::ZERO).as_secs();
            Some(Span::raw(listening(secs)))
        }
        Phase::Processing { .. } => Some(Span::styled(t("voice.processing"), theme::dim())),
        _ => None,
    }
}

/// "● 正在听 0:07"。
fn listening(secs: u64) -> String {
    tf("tui.listening", &[("time", &format!("{}:{:02}", secs / 60, secs % 60))])
}

// ---- 任务 ----

fn render_task(frame: &mut Frame, app: &mut App, area: Rect) {
    let [header, body] = Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).areas(area);
    let term = body.inner(Margin { horizontal: 1, vertical: 0 });
    app.set_pane_size(Size::new(term.height, term.width));
    render_header(frame, app, header);
    render_agent(frame, app, term);
}

/// 第一行：任务标题 │ Agent 标签 …… 分支和改动；第二行留白。
fn render_header(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(task) = app.selected_task().cloned() else { return };
    let row = Rect::new(area.x, area.y, area.width, 1);
    let selected = app.selected_key();

    let mut spans =
        vec![Span::raw(" "), Span::styled(task.title.clone(), theme::strong()), Span::styled("  │  ", theme::faint())];
    for agent in &task.agents {
        let key = task.key(agent.id);
        let (icon, icon_style) = theme::status_icon(app.engine.agent_status(key));
        let active = Some(key) == selected;
        let label_style = if active { Style::new().add_modifier(Modifier::BOLD) } else { theme::dim() };
        spans.push(Span::styled(icon, icon_style));
        spans.push(Span::styled(format!(" {}", agent.role.label(&app.engine.agents().name(&agent.kind))), label_style));
        spans.push(Span::raw("    "));
    }

    let mut right = vec![Span::styled(task.branch.clone(), theme::faint())];
    if let Some(stat) = app.engine.diff_stat(task.id).filter(|s| !s.is_empty()) {
        right.push(Span::styled(format!("  +{}", stat.insertions), Style::new().fg(theme::ADD)));
        right.push(Span::styled(format!(" −{}", stat.deletions), Style::new().fg(theme::DEL)));
    }
    right.push(Span::raw(" "));
    let right_width: u16 = right.iter().map(|s| s.content.width() as u16).sum();
    let [left_area, right_area] = Layout::horizontal([Constraint::Min(1), Constraint::Length(right_width)]).areas(row);
    frame.render_widget(Paragraph::new(Line::from(spans)), left_area);
    frame.render_widget(Paragraph::new(Line::from(right)), right_area);
}

fn render_agent(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(key) = app.selected_key() else { return };
    let status = app.engine.agent_status(key);
    let Some(agent) = app.selected_task().and_then(|t| t.agent(key.agent)).cloned() else { return };
    let name = app.engine.agents().name(&agent.kind);

    if !status.is_live() {
        let line = match status {
            AgentStatus::Exited(exit) if exit.success => tf("tui.agentEnded", &[("agent", &name)]),
            AgentStatus::Exited(exit) => tf("tui.agentCrashed", &[("agent", &name), ("code", &exit.code)]),
            _ if agent.launched => tf("tui.resumeHint", &[("agent", &name)]),
            _ => tf("tui.startHint", &[("agent", &name)]),
        };
        centered(frame, area, Line::styled(line, theme::muted()));
        return;
    }

    let focused = app.focus == Focus::Main && app.mode == Mode::Normal;
    // 聊天模式下没有终端画面，对话在桌面应用里看。
    if app.engine.surface_of(key) == Surface::Chat {
        centered(frame, area, Line::styled(tf("tui.chatMode", &[("agent", &name)]), theme::muted()));
        return;
    }
    let Some(session) = app.engine.session(key) else { return };
    let just_started = session.started().elapsed() < STARTING_GRACE;
    let (cursor, cursor_hidden, row_tail_empty, blank) = session.with_screen(|screen| {
        let mut cursor = Cursor::default();
        cursor.hide();
        frame.render_widget(PseudoTerminal::new(screen).cursor(cursor), area);
        let (row, col) = screen.cursor_position();
        let tail_empty = screen
            .rows(col, screen.size().1.saturating_sub(col))
            .nth(usize::from(row))
            .is_some_and(|t| t.trim().is_empty());
        ((row, col), screen.hide_cursor(), tail_empty, screen.contents().trim().is_empty())
    });
    if blank && just_started {
        centered(frame, area, Line::styled(tf("task.starting", &[("agent", &name)]), theme::dim()));
        return;
    }
    let scrolled = session.scrollback() > 0;
    if scrolled {
        draw_right(
            frame.buffer_mut(),
            area,
            area.y,
            &format!(" {} ", t("tui.scrolledBack")),
            Style::new().bg(theme::CURSOR_BG),
        );
        return;
    }
    let at_cursor = Position::new(area.x + cursor.1, area.y + cursor.0);
    if !area.contains(at_cursor) {
        return;
    }

    match app.voice.phase() {
        Phase::Pending { .. } => {
            if let Some(text) = app.voice.pending_for(key) {
                let end = draw_wrapped(frame.buffer_mut(), area, at_cursor, text, Style::new().bg(theme::PREEDIT_BG));
                if focused {
                    frame.set_cursor_position(end);
                }
                let hint = format!("↵ {}  Esc {}", t("tui.hint.send"), t("tui.hint.discard"));
                draw_right(frame.buffer_mut(), area, at_cursor.y, &hint, theme::dim());
            }
        }
        Phase::Recording { prefix, .. } => {
            let end = draw_wrapped(frame.buffer_mut(), area, at_cursor, prefix, Style::new().bg(theme::PREEDIT_BG));
            let secs = app.voice.recording_elapsed().unwrap_or(Duration::ZERO).as_secs();
            draw_right(frame.buffer_mut(), area, end.y, &listening(secs), Style::new());
        }
        Phase::Processing { prefix, .. } => {
            let end = draw_wrapped(frame.buffer_mut(), area, at_cursor, prefix, Style::new().bg(theme::PREEDIT_BG));
            draw_right(frame.buffer_mut(), area, end.y, &t("voice.processing"), theme::dim());
        }
        Phase::Idle => {
            if focused && !cursor_hidden {
                frame.set_cursor_position(at_cursor);
            }
            if focused && status == AgentStatus::Idle && row_tail_empty {
                let hint = format!("⌃Space {}", t("tui.hint.speak"));
                draw_right(frame.buffer_mut(), area, at_cursor.y, &hint, theme::faint());
            }
        }
    }
}

// ---- 提示栏 ----

fn render_bar(frame: &mut Frame, app: &mut App, area: Rect) {
    let area = area.inner(Margin { horizontal: 1, vertical: 0 });
    let line = match app.mode {
        Mode::ConfirmClose(id) => {
            let title = app.engine.state().task(id).map(|t| t.title.clone()).unwrap_or_default();
            Line::from(vec![
                Span::styled(tf("tui.confirmClose", &[("title", &title)]), theme::need()),
                Span::styled(format!("  y {}  {}", t("tui.hint.confirm"), t("tui.hint.otherCancels")), theme::muted()),
            ])
        }
        Mode::ConfirmQuit => Line::from(vec![
            Span::styled(t("tui.confirmQuit"), theme::need()),
            Span::styled(format!("  y {}  {}", t("tui.hint.quit"), t("tui.hint.otherCancels")), theme::muted()),
        ]),
        _ => match app.flash() {
            Some(flash) => {
                Line::styled(flash.text.clone(), if flash.is_error { theme::need() } else { theme::muted() })
            }
            None if app.is_home() => Line::default(),
            None => {
                let status = app.selected_key().map(|k| app.engine.agent_status(k));
                let (sidebar, needs_you) = (t("tui.hint.sidebar"), t("tui.needsYou"));
                let mut items = vec![("⌃]", sidebar.as_str())];
                if status == Some(AgentStatus::NeedsInput) {
                    items.insert(0, ("●", needs_you.as_str()));
                }
                hint_line(&items)
            }
        },
    };
    frame.render_widget(Paragraph::new(line), area);
}

// ---- 绘制工具 ----

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

fn centered(frame: &mut Frame, area: Rect, line: Line<'static>) {
    let top = area.y + area.height / 2;
    frame.render_widget(Paragraph::new(line), Rect::new(area.x + 2, top, area.width.saturating_sub(4), 1));
}
