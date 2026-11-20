//! 右侧：首页输入框，或当前任务（标题、Agent 标签、Agent 终端）；最下面一行是提示栏。

use std::time::Duration;

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

const VOICE_HINT: &str = "⌃Space 说话";
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
    let config = app.engine.config();
    let mut lines = if adding {
        vec![
            Line::styled("添加一个项目", theme::strong()),
            Line::default(),
            Line::styled("Duolo 在 git 仓库里工作。在下面输入仓库路径，", theme::dim()),
            Line::styled("或者直接在仓库目录里运行 duolo。", theme::dim()),
        ]
    } else {
        vec![
            Line::styled("今天让 Agent 做点什么？", theme::strong()),
            Line::default(),
            Line::styled(
                format!(
                    "Duolo 会新开一个 worktree，让 {} 写代码，需要时让 {} 审阅。",
                    config.writer.display_name(),
                    config.reviewer.display_name()
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
        return Line::styled("正在检查环境…", theme::faint());
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
        vec![Span::styled("添加项目", theme::muted())]
    } else {
        let mut s = vec![
            Span::styled("新任务 · ", theme::muted()),
            Span::styled(app.home_project_name().unwrap_or("").to_string(), theme::muted()),
        ];
        if app.engine.state().projects.len() > 1 {
            s.push(Span::styled("  ⇥ 切换", theme::faint()));
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
        let placeholder = if app.home.adding_project {
            "仓库路径，例如 ~/code/shop"
        } else {
            "描述你想做的事，例如：给订单列表加分页和筛选"
        };
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
    let mut items: Vec<(&str, &str)> =
        vec![("↵", if app.home.adding_project { "添加" } else { "创建并启动" }), ("⌃Space", "说话")];
    if app.has_previous_task() || (app.home.adding_project && !app.engine.state().projects.is_empty()) {
        items.push(("Esc", "返回"));
    }
    items.push(("⌃]", "侧边栏"));
    frame.render_widget(Paragraph::new(hint_line(&items)), hints);
}

fn voice_status(app: &App) -> Option<Span<'static>> {
    match app.voice.phase() {
        Phase::Recording { .. } => {
            let secs = app.voice.recording_elapsed().unwrap_or(Duration::ZERO).as_secs();
            Some(Span::raw(format!("● 正在听 {}:{:02}", secs / 60, secs % 60)))
        }
        Phase::Processing { .. } => Some(Span::styled("整理中…", theme::dim())),
        _ => None,
    }
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
        spans.push(Span::styled(format!(" {} · {}", agent.kind.display_name(), agent.role.label()), label_style));
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
    let name = agent.kind.display_name();

    if !status.is_live() {
        let line = match status {
            AgentStatus::Exited(exit) if exit.success => format!("{name} 已结束 · 按 ↵ 恢复会话"),
            AgentStatus::Exited(exit) => format!("{name} 异常退出（退出码 {}）· 按 ↵ 重新启动", exit.code),
            _ if agent.launched => format!("按 ↵ 恢复 {name} 上次的会话"),
            _ => format!("按 ↵ 启动 {name}"),
        };
        centered(frame, area, Line::styled(line, theme::muted()));
        return;
    }

    let focused = app.focus == Focus::Main && app.mode == Mode::Normal;
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
        centered(frame, area, Line::styled(format!("正在启动 {name}…"), theme::dim()));
        return;
    }
    let scrolled = session.scrollback() > 0;
    if scrolled {
        draw_right(
            frame.buffer_mut(),
            area,
            area.y,
            " 正在查看历史输出 · 任意键回到底部 ",
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
                draw_right(frame.buffer_mut(), area, at_cursor.y, "↵ 发送  Esc 还原", theme::dim());
            }
        }
        Phase::Recording { prefix, .. } => {
            let end = draw_wrapped(frame.buffer_mut(), area, at_cursor, prefix, Style::new().bg(theme::PREEDIT_BG));
            let secs = app.voice.recording_elapsed().unwrap_or(Duration::ZERO).as_secs();
            draw_right(
                frame.buffer_mut(),
                area,
                end.y,
                &format!("● 正在听 {}:{:02}", secs / 60, secs % 60),
                Style::new(),
            );
        }
        Phase::Processing { prefix, .. } => {
            let end = draw_wrapped(frame.buffer_mut(), area, at_cursor, prefix, Style::new().bg(theme::PREEDIT_BG));
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

// ---- 提示栏 ----

fn render_bar(frame: &mut Frame, app: &mut App, area: Rect) {
    let area = area.inner(Margin { horizontal: 1, vertical: 0 });
    let line = match app.mode {
        Mode::ConfirmClose(id) => {
            let title = app.engine.state().task(id).map(|t| t.title.clone()).unwrap_or_default();
            Line::from(vec![
                Span::styled(format!("关闭「{title}」？会结束它的 Agent 并删除 worktree，分支保留。"), theme::need()),
                Span::styled("  y 确认  其他键取消", theme::muted()),
            ])
        }
        Mode::ConfirmQuit => Line::from(vec![
            Span::styled("还有 Agent 在运行，退出会结束它们（之后可以恢复会话）。", theme::need()),
            Span::styled("  y 退出  其他键取消", theme::muted()),
        ]),
        _ => match app.flash() {
            Some(flash) => {
                Line::styled(flash.text.clone(), if flash.is_error { theme::need() } else { theme::muted() })
            }
            None if app.is_home() => Line::default(),
            None => {
                let status = app.selected_key().map(|k| app.engine.agent_status(k));
                let mut items = vec![("⌃]", "侧边栏")];
                if status == Some(AgentStatus::NeedsInput) {
                    items.insert(0, ("●", "Agent 在等你确认，直接在上面选择"));
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
