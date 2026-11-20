//! 根据 App 状态绘制画面。
//!
//! 布局：左侧边栏 | 右侧（Agent 标签、Agent 终端、底栏）。弹出的输入框和帮助画在右侧上方。

mod overlay;
mod pane;
mod sidebar;
mod theme;

use ratatui::layout::{Constraint, Layout};
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
        area
    };
    pane::render(frame, app, main);
    overlay::render(frame, app, main);
}
