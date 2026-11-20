//! 画在右侧上方的弹出内容：新建任务、添加项目、帮助。

use std::time::Duration;

use ratatui::layout::{Position, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use super::theme;
use crate::app::{App, Mode};
use crate::line_input::LineInput;
use crate::voice::Phase;

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    match &app.mode {
        Mode::NewTask { input, project } => {
            let name = app.engine.state().projects.get(*project).map(|p| p.name.as_str()).unwrap_or("");
            let mut title =
                vec![Span::styled("新任务", theme::strong()), Span::styled(format!(" · {name}"), theme::muted())];
            title.extend(voice_status(app));
            let hint = "↵ 创建   Tab 换项目   ⌃Space 说话   Esc 取消";
            input_box(frame, area, Line::from(title), input, hint);
        }
        Mode::AddProject { input } => {
            let title = Line::from(Span::styled("添加项目", theme::strong()));
            input_box(frame, area, title, input, "输入 git 仓库路径   ↵ 添加   Esc 取消");
        }
        Mode::Help => help(frame, area),
        _ => {}
    }
}

fn voice_status(app: &App) -> Vec<Span<'static>> {
    match app.voice.phase() {
        Phase::Recording { .. } => {
            let secs = app.voice.recording_elapsed().unwrap_or(Duration::ZERO).as_secs();
            vec![Span::raw("   "), Span::raw(format!("● {}:{:02}", secs / 60, secs % 60))]
        }
        Phase::Processing { .. } => vec![Span::styled("   整理中…", theme::dim())],
        _ => Vec::new(),
    }
}

/// 贴在右侧底部的输入框：标题、输入行、提示。
fn input_box(frame: &mut Frame, area: Rect, title: Line<'static>, input: &LineInput, hint: &str) {
    let height = 5.min(area.height);
    let rect = Rect::new(area.x, area.y + area.height - height, area.width, height);
    frame.render_widget(Clear, rect);
    let block = Block::new().borders(Borders::TOP).border_style(theme::faint());
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let mut title_spans = vec![Span::raw(" ")];
    title_spans.extend(title.spans);
    let lines = vec![
        Line::from(title_spans),
        Line::from(vec![Span::styled(" › ", theme::muted()), Span::raw(input.text().to_string())]),
        Line::default(),
        Line::styled(format!(" {hint}"), theme::dim()),
    ];
    frame.render_widget(Paragraph::new(lines), inner);

    let x = inner.x + 3 + input.before_cursor().width() as u16;
    let y = inner.y + 1;
    if x < inner.x + inner.width && y < inner.y + inner.height {
        frame.set_cursor_position(Position::new(x, y));
    }
}

fn help(frame: &mut Frame, area: Rect) {
    const ITEMS: &[(&str, &str)] = &[
        ("⌃] j / k", "切换任务"),
        ("⌃] h / l", "切换同一任务里的 Agent"),
        ("⌃] 1-9", "跳到第几个任务"),
        ("⌃] n", "新任务"),
        ("⌃] r", "启动审阅 Agent（只读）"),
        ("⌃] s", "启动或恢复当前 Agent"),
        ("⌃] x", "关闭任务"),
        ("⌃] a", "添加项目"),
        ("⌃] b", "显示或隐藏侧边栏"),
        ("⌃] q", "退出"),
        ("⌃] ⌃]", "把 ⌃] 发给 Agent"),
        ("⌃Space", "语音输入；再按一次结束"),
        ("⇧PgUp / PgDn", "查看历史输出"),
    ];
    let width = 52.min(area.width);
    let height = (ITEMS.len() as u16 + 4).min(area.height);
    let rect = Rect::new(area.x + (area.width - width) / 2, area.y + (area.height - height) / 2, width, height);
    frame.render_widget(Clear, rect);
    let block = Block::bordered().border_style(theme::faint()).title(Span::styled(" 快捷键 ", theme::muted()));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let mut lines: Vec<Line> = ITEMS
        .iter()
        .map(|(k, v)| Line::from(vec![Span::styled(format!(" {k:<14}"), theme::muted()), Span::raw(*v)]))
        .collect();
    lines.push(Line::default());
    lines.push(Line::styled(" 任意键关闭", theme::dim()));
    frame.render_widget(Paragraph::new(lines), inner);
}
