//! 抓 Agent 真实的终端画面，做插件的 `fixtures/`（approval.txt、busy.txt、idle.txt……）。
//!
//! `cargo run -p duolo-pty --example capture -- <脚本> <输出目录> <工作目录> <程序> [参数…]`
//!
//! 脚本一行一条：
//! - `until <文字> <秒>`：等画面上出现这段文字（不分大小写），超时就报错退出
//! - `wait <秒>`：等一会儿
//! - `send <文字>`：往终端里写，`\r`、`\e`、`\n` 换成回车、Esc、换行
//! - `dump <文件名>`：把现在的画面写进输出目录，同时打印出来，还有它自己报告的状态（OSC 7501）
//!
//! 画面是 40 行 120 列，和 Duolo 默认的终端差不多大。

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use duolo_pty::{Rules, Session, SessionSpec, Size};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [script, out, cwd, program, rest @ ..] = args.as_slice() else {
        bail!("usage: capture <script> <out-dir> <cwd> <program> [args...]");
    };
    let script = std::fs::read_to_string(script).context("script")?;
    let out = PathBuf::from(out);
    std::fs::create_dir_all(&out)?;
    let mut session = Session::spawn(SessionSpec {
        program: program.clone(),
        args: rest.to_vec(),
        cwd: PathBuf::from(cwd),
        env: Vec::new(),
        env_remove: Vec::new(),
        size: Size::new(40, 120),
        rules: Rules::default(),
        on_output: None,
    })?;
    let screen = |s: &Session| s.with_screen(|screen| screen.contents());
    for line in script.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let (command, arg) = line.split_once(' ').unwrap_or((line, ""));
        match command {
            "until" => {
                let (text, secs) = arg.rsplit_once(' ').context("until <text> <secs>")?;
                let deadline = Instant::now() + Duration::from_secs_f64(secs.parse()?);
                while !screen(&session).to_lowercase().contains(&text.to_lowercase()) {
                    if Instant::now() > deadline {
                        eprintln!("{}", screen(&session));
                        bail!("timed out waiting for {text:?}");
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
            "wait" => std::thread::sleep(Duration::from_secs_f64(arg.parse()?)),
            "send" => {
                let bytes = arg.replace("\\r", "\r").replace("\\e", "\x1b").replace("\\n", "\n");
                session.write(bytes.as_bytes())?;
            }
            "dump" => {
                let text = screen(&session);
                std::fs::write(out.join(arg), format!("{}\n", text.trim_end()))?;
                println!("==== {arg} (reported: {:?})\n{text}", session.reported());
            }
            other => bail!("unknown command {other:?}"),
        }
    }
    session.stop();
    Ok(())
}
