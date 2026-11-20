//! 侧边栏：项目 → 任务 → Agent，每个 Agent 一行状态。

use duolo_core::text::{format_elapsed, now_secs, truncate_chars};
use duolo_core::TaskId;
use duolo_engine::AgentStatus;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use super::theme;
use crate::app::App;

pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    let block = Block::new().borders(Borders::RIGHT).border_style(theme::faint());
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [body, footer] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)])
        .areas(inner.inner(ratatui::layout::Margin { horizontal: 1, vertical: 0 }));
    let width = usize::from(body.width);

    let mut lines = vec![Line::from(Span::styled("duolo", theme::strong())), Line::default()];
    lines.extend(project_lines(app, width));
    frame.render_widget(Paragraph::new(lines), body);

    let summary = app.engine.summary();
    let mut spans = vec![Span::styled(format!("{} 个运行中", summary.running), theme::dim())];
    if summary.needs_input > 0 {
        spans.push(Span::styled(" · ", theme::dim()));
        spans.push(Span::styled(format!("{} 个等你", summary.needs_input), theme::need()));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), footer);
}

fn project_lines(app: &mut App, width: usize) -> Vec<Line<'static>> {
    let state = app.engine.state().clone();
    if state.projects.is_empty() {
        return vec![
            Line::styled("还没有项目", theme::muted()),
            Line::styled("在 git 仓库里运行 duolo，", theme::dim()),
            Line::styled("或按 a 添加", theme::dim()),
        ];
    }

    let selected = app.selected_task().map(|t| t.id);
    let now = now_secs();
    let mut lines = Vec::new();
    for project in &state.projects {
        let tasks: Vec<_> = state.tasks_of(&project.id).collect();
        lines.push(row(
            vec![Span::styled(format!("▾ {}", project.name), theme::muted())],
            Span::styled(format!("{} 个任务", tasks.len()), theme::dim()),
            width,
            Style::new(),
        ));
        if tasks.is_empty() {
            lines.push(Line::styled("    按 n 新建任务", theme::dim()));
        }
        for task in tasks {
            let statuses: Vec<(u32, AgentStatus)> =
                task.agents.iter().map(|a| (a.id, app.engine.agent_status(task.key(a.id)))).collect();
            lines.push(task_line(task.id, &task.title, task_status(&statuses), now - task.created_at, selected, width));
            for (agent, (id, status)) in task.agents.iter().zip(&statuses) {
                let elapsed = app.engine.session(task.key(*id)).map(|s| s.started().elapsed().as_secs());
                lines.push(agent_line(&agent.kind.display_name(), agent.role.label(), *status, elapsed, width));
            }
        }
        lines.push(Line::default());
    }
    lines
}

fn task_line(
    id: TaskId,
    title: &str,
    status: AgentStatus,
    age: u64,
    selected: Option<TaskId>,
    width: usize,
) -> Line<'static> {
    let (icon, icon_style) = theme::status_icon(status);
    let is_selected = selected == Some(id);
    let title_style = if is_selected { theme::strong() } else { Style::new() };
    let bg = if is_selected { Style::new().bg(theme::SELECTED_BG) } else { Style::new() };
    row(
        vec![
            Span::raw("  "),
            Span::styled(icon, icon_style),
            Span::raw(" "),
            Span::styled(truncate_chars(title, 18), title_style),
        ],
        Span::styled(format_elapsed(age), theme::dim()),
        width,
        bg,
    )
}

fn agent_line(name: &str, role: &str, status: AgentStatus, elapsed: Option<u64>, width: usize) -> Line<'static> {
    let (icon, icon_style) = theme::status_icon(status);
    let state_style = if status == AgentStatus::NeedsInput { theme::need() } else { theme::dim() };
    row(
        vec![
            Span::raw("      "),
            Span::styled(icon, icon_style),
            Span::raw(" "),
            Span::styled(name.to_string(), theme::muted()),
            Span::styled(format!(" {role} · "), theme::dim()),
            Span::styled(status.label(), state_style),
        ],
        Span::styled(elapsed.map(format_elapsed).unwrap_or_default(), theme::dim()),
        width,
        Style::new(),
    )
}

/// 任务的汇总状态：有一个等你就是等你，否则有一个在跑就是运行中。
fn task_status(agents: &[(u32, AgentStatus)]) -> AgentStatus {
    let any = |f: fn(&AgentStatus) -> bool| agents.iter().any(|(_, s)| f(s));
    if any(|s| *s == AgentStatus::NeedsInput) {
        AgentStatus::NeedsInput
    } else if any(|s| *s == AgentStatus::Running) {
        AgentStatus::Running
    } else if any(|s| *s == AgentStatus::Idle) {
        AgentStatus::Idle
    } else {
        agents.first().map_or(AgentStatus::NotStarted, |(_, s)| *s)
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
