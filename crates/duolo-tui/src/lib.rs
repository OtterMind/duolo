//! Duolo 的终端界面。
//!
//! 结构：
//! - [`app`]：界面状态和所有操作，不涉及绘制；
//! - [`input`]：把键盘事件翻译成操作；
//! - [`keys`]：把按键编码成发给 Agent 的字节；
//! - [`voice`]：语音输入的状态机，耗时步骤在后台线程执行；
//! - [`ui`]：根据状态绘制画面。

mod app;
mod input;
mod keys;
mod line_input;
mod ui;
mod voice;

use std::io::stdout;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use duolo_engine::Engine;
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
};
use ratatui::crossterm::execute;

use crate::app::App;

/// 没有新事件时，最长多久重绘一次（用于刷新转圈图标和用时）。
const IDLE_REDRAW: Duration = Duration::from_millis(250);
/// 等待键盘事件的间隔。
const POLL_INTERVAL: Duration = Duration::from_millis(30);

/// 打开工作台，直到用户退出。`cwd` 是 git 仓库时会自动加入项目。
pub fn run(mut engine: Engine, cwd: &Path) -> Result<()> {
    // 终端界面目前只有中文，审阅结论也用中文。
    engine.set_reply_language("Simplified Chinese");
    let dirty = Arc::new(AtomicBool::new(true));
    {
        let dirty = Arc::clone(&dirty);
        engine.set_output_hook(Arc::new(move || dirty.store(true, Ordering::Relaxed)));
    }
    let mut app = App::new(engine, cwd);

    let mut terminal = ratatui::init();
    execute!(stdout(), EnableBracketedPaste, EnableMouseCapture)?;
    let result = event_loop(&mut terminal, &mut app, &dirty);
    let _ = execute!(stdout(), DisableBracketedPaste, DisableMouseCapture);
    ratatui::restore();
    app.shutdown();
    result
}

fn event_loop(terminal: &mut ratatui::DefaultTerminal, app: &mut App, dirty: &AtomicBool) -> Result<()> {
    let mut last_draw = Instant::now() - IDLE_REDRAW;
    loop {
        let changed = app.poll_background();
        if changed || dirty.swap(false, Ordering::Relaxed) || last_draw.elapsed() >= IDLE_REDRAW {
            terminal.draw(|frame| ui::render(frame, app))?;
            last_draw = Instant::now();
        }
        if app.should_quit() {
            return Ok(());
        }
        if event::poll(POLL_INTERVAL)? {
            // 一次处理完排队的事件，再统一重绘，粘贴大段文字时不会逐字刷新。
            loop {
                input::handle(app, event::read()?);
                if !event::poll(Duration::ZERO)? {
                    break;
                }
            }
            dirty.store(true, Ordering::Relaxed);
        }
    }
}
