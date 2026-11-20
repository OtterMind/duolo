//! `duolo` 命令行入口。

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use duolo_core::text::{format_elapsed, now_secs};
use duolo_core::{Paths, ProjectId};
use duolo_engine::{check_environment, Engine};

#[derive(Debug, Parser)]
#[command(
    name = "duolo",
    version,
    about = "本地优先的 AI 开发搭档：并行运行 Claude Code 和 Codex，一个写、一个审",
    args_conflicts_with_subcommands = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,
    /// 打开 Duolo；目录是 git 仓库时自动加入项目（默认当前目录）
    path: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// 在终端里打开工作台（适合 SSH 远程使用）
    Tui { path: Option<PathBuf> },
    /// 添加项目（git 仓库）
    Add { path: PathBuf },
    /// 新建任务；下次打开工作台时自动启动 Agent
    New {
        /// 需求描述
        #[arg(required = true)]
        prompt: Vec<String>,
        /// 项目 id，默认是当前目录所在的仓库
        #[arg(short, long)]
        project: Option<String>,
    },
    /// 列出所有任务
    Ls,
    /// 检查运行环境
    Doctor,
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("duolo: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    let paths = Paths::discover()?;
    let cwd = std::env::current_dir().context("cannot read the current directory")?;
    match cli.command {
        None => {
            let dir = cli.path.unwrap_or(cwd);
            if open_app(&dir)? {
                return Ok(());
            }
            eprintln!("没有找到 Duolo.app，先在终端里打开。安装桌面应用见 README。");
            duolo_tui::run(Engine::open(paths)?, &dir)
        }
        Some(Cmd::Tui { path }) => duolo_tui::run(Engine::open(paths)?, &path.unwrap_or(cwd)),
        Some(Cmd::Add { path }) => {
            let mut engine = Engine::open(paths)?;
            let id = engine.add_project(&path)?;
            println!("已添加项目 {id}");
            Ok(())
        }
        Some(Cmd::New { prompt, project }) => {
            let mut engine = Engine::open(paths)?;
            let project = match project {
                Some(id) => {
                    let id = ProjectId(id);
                    if engine.state().project(&id).is_none() {
                        bail!("没有项目 {id}，可用 duolo ls 查看");
                    }
                    id
                }
                None => engine.add_project(&cwd)?,
            };
            let id = engine.create_task(&project, &prompt.join(" "))?;
            let task = engine.state().task(id).expect("刚创建的任务");
            println!("已创建任务 #{id}「{}」", task.title);
            println!("  分支 {}", task.branch);
            println!("  运行 duolo 打开 Duolo，点开任务后 Agent 会自动启动");
            Ok(())
        }
        Some(Cmd::Ls) => {
            let engine = Engine::open(paths)?;
            let state = engine.state();
            if state.tasks.is_empty() {
                println!("还没有任务。用 duolo new \"需求\" 或在工作台里按 n 新建。");
            }
            let now = now_secs();
            for project in &state.projects {
                println!("{}  {}", project.id, project.root.display());
                for task in state.tasks_of(&project.id) {
                    let agents: Vec<String> =
                        task.agents.iter().map(|a| format!("{}{}", a.kind.display_name(), a.role.label())).collect();
                    println!(
                        "  #{:<4} {:<30} {:<8} {}  {}",
                        task.id.0,
                        task.title,
                        format_elapsed(now.saturating_sub(task.created_at)),
                        agents.join(" · "),
                        task.branch
                    );
                }
            }
            Ok(())
        }
        Some(Cmd::Doctor) => doctor(&paths),
    }
}

/// 桌面应用的 Bundle ID，与 crates/duolo-app/tauri.conf.json 一致。
const APP_BUNDLE_ID: &str = "ai.ottermind.duolo";

/// 打开桌面应用；`dir` 是 git 仓库时交给应用加入项目。没有安装应用时返回 false。
fn open_app(dir: &std::path::Path) -> Result<bool> {
    if !cfg!(target_os = "macos") {
        return Ok(false);
    }
    let mut cmd = std::process::Command::new("open");
    match duolo_git::repo_root(dir) {
        // -n 启动新实例，新实例会把目录转交给已经在运行的 Duolo 后退出。
        Ok(root) => cmd.args(["-n", "-b", APP_BUNDLE_ID, "--args"]).arg(root),
        Err(_) => cmd.args(["-b", APP_BUNDLE_ID]),
    };
    let status = cmd.stderr(std::process::Stdio::null()).status().context("cannot run open")?;
    Ok(status.success())
}

fn doctor(paths: &Paths) -> Result<()> {
    let engine = Engine::open(paths.clone())?;
    let config = engine.config();
    println!("数据目录  {}", paths.root().display());
    for check in check_environment(config) {
        println!("{} {:<8} {}", if check.ok { "✓" } else { "✕" }, check.name, check.detail);
    }
    println!("写代码    {}", config.writer.display_name());
    println!("审阅      {}", config.reviewer.display_name());
    if !config.voice.is_configured() {
        println!("语音命令在 {} 的 voice.command 里设置，见 README", paths.config_file().display());
    }
    Ok(())
}
