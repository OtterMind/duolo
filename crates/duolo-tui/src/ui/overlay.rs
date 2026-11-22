//! 浮在最上层的帮助。

use duolo_core::t;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use super::theme;
use crate::app::{App, Mode};

/// 帮助的每一行：分组、按键、作用（当前语言）。
fn help_rows() -> Vec<(String, String, String)> {
    let row = |group: &str, keys: String, what: &str| {
        (if group.is_empty() { String::new() } else { t(group) }, keys, t(what))
    };
    vec![
        row("tui.help.anywhere", "⌃]".into(), "tui.help.switchFocus"),
        row("", "⌃Space".into(), "tui.help.voice"),
        row("tui.help.sidebar", "↑↓  ↵".into(), "tui.help.selectOpen"),
        row("", "n  a".into(), "tui.help.newAdd"),
        row("", "r  x".into(), "tui.help.reviewClose"),
        row("", "1-9".into(), "tui.help.jump"),
        row("", "b  q".into(), "tui.help.hideQuit"),
        row("tui.help.home", "↵  ⇥".into(), "tui.help.createSwitch"),
        row("tui.help.agent", t("tui.help.anyKey"), "tui.help.direct"),
        row("", format!("⇧PgUp  {}", t("tui.help.wheel")), "tui.help.scroll"),
        row("tui.help.voiceText", "↵  Tab  Esc".into(), "tui.help.voiceKeys"),
        row("tui.help.mouse", t("tui.help.click"), "tui.help.mouseOpen"),
        row("", format!("⇧ {}", t("tui.help.drag")), "tui.help.selectText"),
    ]
}

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    if app.mode != Mode::Help {
        return;
    }
    let help = help_rows();
    // 分组和按键两列按最长的内容对齐，各语言长度不同。
    let group_width = help.iter().map(|(g, _, _)| g.width()).max().unwrap_or(0) + 2;
    let keys_width = help.iter().map(|(_, k, _)| k.width()).max().unwrap_or(0) + 2;
    let what_width = help.iter().map(|(_, _, w)| w.width()).max().unwrap_or(0);
    let width = ((group_width + keys_width + what_width + 4) as u16).min(area.width);
    let height = (help.len() as u16 + 4).min(area.height);
    let rect = Rect::new(area.x + (area.width - width) / 2, area.y + (area.height - height) / 2, width, height);
    // 整行清空：底下的中文字符占两格，只清弹窗范围会压住左边框。
    frame.render_widget(Clear, Rect::new(area.x, rect.y, area.width, rect.height));
    let block = Block::bordered()
        .border_style(theme::faint())
        .title(Span::styled(format!(" {} ", t("tui.help.title")), theme::muted()));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let mut lines: Vec<Line> = help
        .into_iter()
        .map(|(group, keys, what)| {
            Line::from(vec![
                Span::styled(format!(" {}", pad(&group, group_width)), theme::faint()),
                Span::styled(pad(&keys, keys_width), theme::muted()),
                Span::raw(what),
            ])
        })
        .collect();
    lines.push(Line::default());
    lines.push(Line::styled(format!(" {}", t("tui.help.close")), theme::faint()));
    frame.render_widget(Paragraph::new(lines), inner);
}

/// 按显示宽度补空格（中文占两格）。
fn pad(text: &str, width: usize) -> String {
    format!("{text}{}", " ".repeat(width.saturating_sub(text.width())))
}
