//! 配色：中性灰为主，唯一的彩色是"需要你"的琥珀色。
//! 背景不着色，跟随用户终端的主题。

use std::time::{SystemTime, UNIX_EPOCH};

use duolo_engine::AgentStatus;
use ratatui::style::{Color, Modifier, Style};

pub const MUTED: Color = Color::Rgb(150, 150, 150);
pub const DIM: Color = Color::Rgb(105, 105, 105);
pub const FAINT: Color = Color::Rgb(60, 60, 60);
pub const NEED: Color = Color::Rgb(227, 164, 60);
pub const ADD: Color = Color::Rgb(124, 197, 138);
pub const DEL: Color = Color::Rgb(224, 122, 114);
pub const SELECTED_BG: Color = Color::Rgb(42, 42, 42);
pub const PREEDIT_BG: Color = Color::Rgb(48, 48, 48);

pub fn muted() -> Style {
    Style::new().fg(MUTED)
}

pub fn dim() -> Style {
    Style::new().fg(DIM)
}

pub fn faint() -> Style {
    Style::new().fg(FAINT)
}

pub fn strong() -> Style {
    Style::new().add_modifier(Modifier::BOLD)
}

pub fn need() -> Style {
    Style::new().fg(NEED)
}

const SPINNER: [&str; 4] = ["◐", "◓", "◑", "◒"];

/// 状态图标：转圈表示运行中，琥珀点表示等你，空心圈表示等待，对勾表示结束。
pub fn status_icon(status: AgentStatus) -> (&'static str, Style) {
    match status {
        AgentStatus::Running => (spinner_frame(), muted()),
        AgentStatus::NeedsInput => ("●", need()),
        AgentStatus::Idle => ("○", dim()),
        AgentStatus::NotStarted => ("·", dim()),
        AgentStatus::Exited(exit) if exit.success => ("✓", dim()),
        AgentStatus::Exited(_) => ("✕", dim()),
    }
}

fn spinner_frame() -> &'static str {
    let ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    SPINNER[(ms / 250) as usize % SPINNER.len()]
}
