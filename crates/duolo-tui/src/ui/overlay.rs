//! 浮在最上层的帮助和「交给另一个 Agent」表单。

use duolo_core::{t, tf, AgentRole};
use ratatui::layout::{Position, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use super::theme;
use crate::app::{App, HandoffForm, HandoffKind, Mode};

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
        row("", "t".into(), "tui.help.handoff"),
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
    if app.mode == Mode::Handoff {
        if let Some(form) = &app.handoff {
            render_handoff(frame, app, form, area);
        }
        return;
    }
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

/// 「交给另一个 Agent」：谁来、做什么、补充一句，下面说明会发生什么。
fn render_handoff(frame: &mut Frame, app: &App, form: &HandoffForm, area: Rect) {
    let (kind, name) = &form.agents[form.agent];
    let writer = app.engine.state().task(form.task).and_then(|t| t.agent_with_role(AgentRole::Writer));
    let writer_name = writer.map(|w| app.engine.agents().name(&w.kind)).unwrap_or_default();
    let writing = writer.is_some_and(|w| &w.kind == kind);
    let cant_review = app.engine.config().review_command.is_empty() && app.engine.agents().review(kind).is_err();
    let foot = match form.kind {
        HandoffKind::Review if cant_review => tf("handoff.cantReview", &[("agent", name)]),
        HandoffKind::Review => tf("handoff.reviewFoot", &[("agent", name)]),
        HandoffKind::Relay if writing => tf("handoff.alreadyWriting", &[("agent", name)]),
        HandoffKind::Relay => tf("handoff.relayFoot", &[("from", &writer_name), ("to", name)]),
        HandoffKind::Fork => tf("handoff.forkFoot", &[("from", &writer_name)]),
    };
    let labels = [t("handoff.who"), t("handoff.what"), t("handoff.request")];
    let label_width = labels.iter().map(|l| l.width()).max().unwrap_or(0) + 2;
    let mut lines = vec![
        Line::from(vec![
            Span::styled(format!(" {}", pad(&labels[0], label_width)), theme::faint()),
            Span::raw(format!("‹ {name} ›")),
        ]),
        Line::from(vec![
            Span::styled(format!(" {}", pad(&labels[1], label_width)), theme::faint()),
            Span::raw(format!("‹ {} ›", form.kind.label())),
        ]),
    ];
    let asks = form.kind != HandoffKind::Review;
    if asks {
        let text = form.request.text();
        lines.push(Line::from(vec![
            Span::styled(format!(" {}", pad(&labels[2], label_width)), theme::faint()),
            if text.is_empty() {
                Span::styled(t("handoff.requestPlaceholder"), theme::faint())
            } else {
                Span::raw(text.to_string())
            },
        ]));
    }
    lines.push(Line::default());
    lines.push(Line::styled(format!(" {foot}"), theme::muted()));
    lines.push(Line::default());
    lines.push(Line::styled(format!(" {}", t("tui.handoff.keys")), theme::faint()));

    let width = (lines.iter().map(|l| l.width()).max().unwrap_or(0) as u16 + 4).clamp(40, area.width.max(1));
    let height = (lines.len() as u16 + 2).min(area.height);
    let rect = Rect::new(area.x + (area.width - width) / 2, area.y + (area.height - height) / 2, width, height);
    frame.render_widget(Clear, Rect::new(area.x, rect.y, area.width, rect.height));
    let block = Block::bordered()
        .border_style(theme::faint())
        .title(Span::styled(format!(" {} ", t("handoff.title")), theme::muted()));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    frame.render_widget(Paragraph::new(lines), inner);
    if asks {
        let x = inner.x + 1 + label_width as u16 + form.request.before_cursor().width() as u16;
        if x < inner.x + inner.width && inner.height > 2 {
            frame.set_cursor_position(Position::new(x, inner.y + 2));
        }
    }
}

/// 按显示宽度补空格（中文占两格）。
fn pad(text: &str, width: usize) -> String {
    format!("{text}{}", " ".repeat(width.saturating_sub(text.width())))
}
