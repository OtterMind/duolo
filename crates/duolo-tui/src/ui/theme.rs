//! 配色：中性灰为主，唯一的彩色是"需要你"的琥珀色。
//! 背景不着色，跟随用户终端的主题。

use std::time::{SystemTime, UNIX_EPOCH};

use duolo_engine::AgentStatus;
use ratatui::style::{Color, Modifier, Style};

pub const MUTED: Color = Color::Rgb(160, 160, 160);
pub const DIM: Color = Color::Rgb(112, 112, 112);
pub const FAINT: Color = Color::Rgb(70, 70, 70);
pub const NEED: Color = Color::Rgb(227, 164, 60);
pub const ADD: Color = Color::Rgb(124, 197, 138);
pub const DEL: Color = Color::Rgb(224, 122, 114);
/// 右侧正在显示的那一行。
pub const SELECTED_BG: Color = Color::Rgb(38, 38, 38);
/// 侧边栏有焦点时，光标所在的行。
pub const CURSOR_BG: Color = Color::Rgb(60, 60, 60);
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

/// 按键提示里的键名。
pub fn key() -> Style {
    Style::new().fg(MUTED)
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
