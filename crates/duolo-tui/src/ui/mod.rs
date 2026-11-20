//! 根据 App 状态绘制画面。
//!
//! 布局：左侧边栏 | 右侧（首页输入框，或当前任务的 Agent 终端）。帮助浮层画在最上面。

mod overlay;
mod pane;
mod sidebar;
mod theme;

#[cfg(test)]
mod tests;

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::Frame;

use crate::app::App;

/// 侧边栏宽度（字符格）。
const SIDEBAR_WIDTH: u16 = 36;

pub fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let main = if app.sidebar_visible && area.width > SIDEBAR_WIDTH + 40 {
        let [side, main] = Layout::horizontal([Constraint::Length(SIDEBAR_WIDTH), Constraint::Min(1)]).areas(area);
        sidebar::render(frame, app, side);
        main
    } else {
        app.sidebar_area = Rect::default();
        app.sidebar_hits.clear();
        area
    };
    pane::render(frame, app, main);
    overlay::render(frame, app, main);
}
