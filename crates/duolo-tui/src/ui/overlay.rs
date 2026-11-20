//! 浮在最上层的帮助。

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use super::theme;
use crate::app::{App, Mode};

const HELP: &[(&str, &str, &str)] = &[
    ("任意位置", "⌃]", "在侧边栏和右侧之间切换"),
    ("", "⌃Space", "语音输入，再按一次结束"),
    ("侧边栏", "↑↓  ↵", "选择  打开"),
    ("", "n  a", "新任务  添加项目"),
    ("", "r  x", "启动审阅  关闭任务"),
    ("", "1-9", "跳到第几个任务"),
    ("", "b  q", "隐藏侧边栏  退出"),
    ("首页", "↵  ⇥", "创建任务  切换项目"),
    ("Agent", "任意键", "直接发给 Agent"),
    ("", "⇧PgUp  滚轮", "查看历史输出"),
    ("语音文字", "↵  Tab  Esc", "发送  只填入  还原"),
    ("鼠标", "点击", "打开侧边栏里的任务或 Agent"),
    ("", "⇧ 拖动", "选择文字（iTerm 为 ⌥ 拖动）"),
];

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    if app.mode != Mode::Help {
        return;
    }
    let width = 60.min(area.width);
    let height = (HELP.len() as u16 + 4).min(area.height);
    let rect = Rect::new(area.x + (area.width - width) / 2, area.y + (area.height - height) / 2, width, height);
    // 整行清空：底下的中文字符占两格，只清弹窗范围会压住左边框。
    frame.render_widget(Clear, Rect::new(area.x, rect.y, area.width, rect.height));
    let block = Block::bordered().border_style(theme::faint()).title(Span::styled(" 快捷键 ", theme::muted()));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let mut lines: Vec<Line> = HELP
        .iter()
        .map(|(group, keys, what)| {
            Line::from(vec![
                Span::styled(format!(" {}", pad(group, 9)), theme::faint()),
                Span::styled(pad(keys, 14), theme::muted()),
                Span::raw(*what),
            ])
        })
        .collect();
    lines.push(Line::default());
    lines.push(Line::styled(" 任意键关闭", theme::faint()));
    frame.render_widget(Paragraph::new(lines), inner);
}

/// 按显示宽度补空格（中文占两格）。
fn pad(text: &str, width: usize) -> String {
    format!("{text}{}", " ".repeat(width.saturating_sub(text.width())))
}
