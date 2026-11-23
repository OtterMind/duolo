//! 侧边栏：新任务入口、项目 → 任务 → Agent，以及添加项目。既显示状态，也是操作栏。

use duolo_core::text::{format_elapsed, now_secs, truncate_chars};
use duolo_core::{t, tf};
use duolo_engine::AgentStatus;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use super::theme;
use crate::app::{App, Focus, Item};

pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    app.sidebar_area = area;
    let block = Block::new().borders(Borders::RIGHT).border_style(theme::faint());
    let inner = block.inner(area).inner(Margin { horizontal: 1, vertical: 0 });
    frame.render_widget(block, area);

    let [body, footer] = Layout::vertical([Constraint::Min(1), Constraint::Length(2)]).areas(inner);
    let width = usize::from(body.width);
    let rows = build_rows(app, width);

    app.sidebar_hits.clear();
    let mut lines = Vec::with_capacity(rows.len());
    for (i, (line, item)) in rows.into_iter().enumerate() {
        let y = body.y + i as u16;
        if y >= body.y + body.height {
            break;
        }
        if let Some(item) = item {
            app.sidebar_hits.push((y, item));
        }
        lines.push(line);
    }
    frame.render_widget(Paragraph::new(lines), body);
    frame.render_widget(Paragraph::new(footer_lines(app)), footer);
}

/// 每一行的内容，以及它对应的可选项（标题、空行为 None）。
fn build_rows(app: &mut App, width: usize) -> Vec<(Line<'static>, Option<Item>)> {
    let header = row(
        vec![Span::styled("duolo", theme::strong())],
        Span::styled(concat!("v", env!("CARGO_PKG_VERSION")), theme::faint()),
        width,
        Style::new(),
    );
    let mut rows = vec![
        (header, None),
        (Line::default(), None),
        item_row(app, Item::NewTask, vec![Span::raw(t("tui.newTask"))], Span::raw(""), width),
        (Line::default(), None),
    ];

    let state = app.engine.state().clone();
    let now = now_secs();
    for project in &state.projects {
        let tasks: Vec<_> = state.tasks_of(&project.id).collect();
        rows.push((
            row(
                vec![Span::styled(project.name.clone(), theme::muted())],
                Span::styled(if tasks.is_empty() { String::new() } else { tasks.len().to_string() }, theme::faint()),
                width,
                Style::new(),
            ),
            None,
        ));
        if tasks.is_empty() {
            rows.push((Line::styled(format!("  {}", t("sidebar.noTasks")), theme::faint()), None));
        }
        for task in tasks {
            let statuses: Vec<AgentStatus> =
                task.agents.iter().map(|a| app.engine.agent_status(task.key(a.id))).collect();
            let (icon, icon_style) = theme::status_icon(task_status(&statuses));
            rows.push(item_row(
                app,
                Item::Task(task.id),
                vec![
                    Span::raw("  "),
                    Span::styled(icon, icon_style),
                    Span::raw(" "),
                    Span::raw(truncate_chars(&task.title, 20)),
                ],
                Span::styled(format_elapsed(now.saturating_sub(task.created_at)), theme::faint()),
                width,
            ));
            for (agent, status) in task.agents.iter().zip(statuses) {
                let key = task.key(agent.id);
                let (icon, icon_style) = theme::status_icon(status);
                let state_style = if status == AgentStatus::NeedsInput { theme::need() } else { theme::dim() };
                let elapsed = app.engine.session(key).map(|s| format_elapsed(s.started().elapsed().as_secs()));
                rows.push(item_row(
                    app,
                    Item::Agent(key),
                    vec![
                        Span::raw("     "),
                        Span::styled(icon, icon_style),
                        Span::raw(" "),
                        Span::styled(agent.role.label(&app.engine.agents().name(&agent.kind)), theme::muted()),
                        Span::styled(" · ", theme::dim()),
                        Span::styled(status.label(), state_style),
                    ],
                    Span::styled(elapsed.unwrap_or_default(), theme::faint()),
                    width,
                ));
            }
        }
        rows.push((Line::default(), None));
    }
    if state.projects.is_empty() {
        rows.push((Line::styled(t("sidebar.noProjects"), theme::dim()), None));
        rows.push((Line::default(), None));
    }
    rows.push(item_row(
        app,
        Item::AddProject,
        vec![Span::styled(t("tui.addProject"), theme::dim())],
        Span::raw(""),
        width,
    ));
    rows
}

/// 可选中的一行：侧边栏光标所在时高亮，右侧正在显示时浅色高亮。
fn item_row(
    app: &App,
    item: Item,
    left: Vec<Span<'static>>,
    right: Span<'static>,
    width: usize,
) -> (Line<'static>, Option<Item>) {
    let is_cursor = app.focus == Focus::Sidebar && app.cursor == item;
    let current = app.current_item();
    let is_current = current == item || matches!((item, current), (Item::Task(t), Item::Agent(k)) if k.task == t);
    let style = if is_cursor {
        Style::new().bg(theme::CURSOR_BG).add_modifier(Modifier::BOLD)
    } else if is_current {
        Style::new().bg(theme::SELECTED_BG)
    } else {
        Style::new()
    };
    (row(left, right, width, style), Some(item))
}

fn footer_lines(app: &mut App) -> Vec<Line<'static>> {
    if app.focus == Focus::Sidebar {
        return vec![
            hint_line(&[("↑↓", &t("tui.hint.select")), ("↵", &t("tui.hint.open")), ("Esc", &t("tui.hint.back"))]),
            hint_line(&[
                ("n", &t("tui.hint.new")),
                ("r", &t("tui.hint.review")),
                ("x", &t("tui.hint.close")),
                ("?", &t("tui.hint.help")),
                ("q", &t("tui.hint.quit")),
            ]),
        ];
    }
    let summary = app.engine.summary();
    let mut spans = Vec::new();
    if summary.running + summary.needs_input == 0 {
        spans.push(Span::styled(t("sidebar.noneRunning"), theme::faint()));
    } else {
        spans.push(Span::styled(tf("sidebar.running", &[("count", &summary.running)]), theme::dim()));
        if summary.needs_input > 0 {
            spans.push(Span::styled(" · ", theme::dim()));
            spans.push(Span::styled(tf("sidebar.needsYou", &[("count", &summary.needs_input)]), theme::need()));
        }
    }
    vec![Line::from(spans), hint_line(&[("⌃]", &t("tui.hint.sidebar"))])]
}

pub fn hint_line(items: &[(&str, &str)]) -> Line<'static> {
    let mut spans = Vec::new();
    for (i, (key, label)) in items.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(key.to_string(), theme::key()));
        spans.push(Span::styled(format!(" {label}"), theme::faint()));
    }
    Line::from(spans)
}

/// 任务的汇总状态：有一个等你就是等你，否则有一个在跑就是运行中。
fn task_status(agents: &[AgentStatus]) -> AgentStatus {
    if agents.contains(&AgentStatus::NeedsInput) {
        AgentStatus::NeedsInput
    } else if agents.contains(&AgentStatus::Running) {
        AgentStatus::Running
    } else if agents.contains(&AgentStatus::Idle) {
        AgentStatus::Idle
    } else {
        agents.first().copied().unwrap_or(AgentStatus::NotStarted)
    }
}

/// 左边内容 + 右对齐的一段文字，填满整行宽度。
fn row(left: Vec<Span<'static>>, right: Span<'static>, width: usize, style: Style) -> Line<'static> {
    let used: usize = left.iter().map(|s| s.content.width()).sum::<usize>() + right.content.width();
    let mut spans = left;
    spans.push(Span::raw(" ".repeat(width.saturating_sub(used))));
    spans.push(right);
    Line::from(spans).style(style)
}
