//! `duolo` 命令行入口。

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use duolo_core::text::{format_elapsed, now_secs};
use duolo_core::{i18n, t, tf, AgentKind, Paths, ProjectId, Settings, TaskId};
use duolo_engine::{check_environment, Engine, TaskOptions};

#[derive(Debug, Parser)]
#[command(
    name = "duolo",
    version,
    args_conflicts_with_subcommands = true,
    disable_help_flag = true,
    disable_version_flag = true,
    disable_help_subcommand = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,
    path: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    Tui {
        path: Option<PathBuf>,
    },
    Add {
        path: PathBuf,
    },
    New {
        #[arg(required = true)]
        prompt: Vec<String>,
        #[arg(short, long)]
        project: Option<String>,
        #[arg(short, long)]
        writer: Option<String>,
        #[arg(short, long)]
        reviewer: Option<String>,
    },
    Switch {
        task: u64,
        #[arg(short, long)]
        writer: Option<String>,
        #[arg(short, long)]
        reviewer: Option<String>,
    },
    Ls,
    Doctor,
}

fn main() -> ExitCode {
    // Agent 运行钩子时用的就是这个程序（见 duolo_core::hook）：写下事件就退出。
    if std::env::args_os().nth(1).is_some_and(|arg| arg == duolo_core::hook::HOOK_ARG) {
        duolo_core::hook::run();
        return ExitCode::SUCCESS;
    }
    // 命令行和终端界面跟随用户在 Duolo 里选的语言，没选过时跟随系统语言。
    if let Ok(paths) = Paths::discover() {
        i18n::apply_preference(&Settings::load(&paths.settings_file()).unwrap_or_default().language);
    }
    let parsed = localized_command().try_get_matches_from(args()).and_then(|m| Cli::from_arg_matches(&m));
    let cli = match parsed {
        Ok(cli) => cli,
        Err(err) => exit_with(err),
    };
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            match err.downcast_ref::<Plain>() {
                Some(Plain(message)) => eprintln!("duolo: {message}"),
                None => eprintln!("duolo: {}", duolo_core::error::describe(&err)),
            }
            ExitCode::FAILURE
        }
    }
}

/// 已经翻译好、原样给用户看的错误（不是程序出了问题）。
#[derive(Debug)]
struct Plain(String);

impl std::fmt::Display for Plain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Plain {}

/// 命令行参数。需求可以是列表（`duolo new "- …"`），前面加个空格，不让 clap 当成选项。
fn args() -> impl Iterator<Item = OsString> {
    std::env::args_os().map(|arg| match arg.to_str() {
        Some(text) if text.starts_with("- ") => format!(" {text}").into(),
        _ => arg,
    })
}

/// 命令行参数有误：clap 自己的报错只有英文，这里换成当前语言。帮助和版本照常输出。
fn exit_with(err: clap::Error) -> ! {
    use clap::error::{ContextKind, ContextValue, ErrorKind};
    if matches!(err.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayVersion) {
        err.exit();
    }
    let context = |kind| match err.get(kind) {
        Some(ContextValue::String(s)) => s.clone(),
        Some(ContextValue::Strings(list)) => list.join(", "),
        _ => String::new(),
    };
    let message = match err.kind() {
        ErrorKind::MissingRequiredArgument | ErrorKind::MissingSubcommand => {
            tf("cli.missingArgs", &[("arg", &context(ContextKind::InvalidArg))])
        }
        _ => {
            let arg = [ContextKind::InvalidArg, ContextKind::InvalidSubcommand, ContextKind::InvalidValue]
                .into_iter()
                .map(context)
                .find(|s| !s.is_empty())
                .unwrap_or_default();
            tf("cli.badArgs", &[("arg", &arg)])
        }
    };
    eprintln!("duolo: {message}");
    std::process::exit(2);
}

/// `--help` 里的说明按当前语言显示。
fn localized_command() -> clap::Command {
    let cmd = Cli::command()
        .about(t("cli.about"))
        .mut_arg("path", |a| a.help(t("cli.help.path")))
        .mut_subcommand("tui", |c| c.about(t("cli.help.tui")).mut_arg("path", |a| a.help(t("cli.help.path"))))
        .mut_subcommand("add", |c| c.about(t("cli.help.add")))
        .mut_subcommand("new", |c| {
            c.about(t("cli.help.new"))
                .mut_arg("prompt", |a| a.help(t("cli.help.prompt")))
                .mut_arg("project", |a| a.help(t("cli.help.project")))
                .mut_arg("writer", |a| a.help(t("cli.help.writer")))
                .mut_arg("reviewer", |a| a.help(t("cli.help.reviewer")))
        })
        .mut_subcommand("switch", |c| {
            c.about(t("cli.help.switch"))
                .mut_arg("task", |a| a.help(t("cli.help.task")))
                .mut_arg("writer", |a| a.help(t("cli.help.switchWriter")))
                .mut_arg("reviewer", |a| a.help(t("cli.help.switchReviewer")))
        })
        .mut_subcommand("ls", |c| c.about(t("cli.help.ls")))
        .mut_subcommand("doctor", |c| c.about(t("cli.help.doctor")))
        .arg(clap::Arg::new("version").short('V').long("version").action(clap::ArgAction::Version))
        .mut_arg("version", |a| a.help(t("cli.help.version")));
    localize_chrome(cmd)
}

/// clap 自带的标题（用法、命令、参数、选项）和 `-h` 的说明换成当前语言。子命令一并处理。
fn localize_chrome(mut cmd: clap::Command) -> clap::Command {
    let ids: Vec<(clap::Id, bool)> = cmd.get_arguments().map(|a| (a.get_id().clone(), a.is_positional())).collect();
    for (id, positional) in ids {
        let heading = if positional { t("cli.arguments") } else { t("cli.options") };
        cmd = cmd.mut_arg(id, |a| a.help_heading(heading));
    }
    let help = clap::Arg::new("help").short('h').long("help").action(clap::ArgAction::Help).help(t("cli.help.help"));
    cmd = cmd
        .arg(help.help_heading(t("cli.options")))
        .subcommand_help_heading(t("cli.commands"))
        .help_template(format!("{{about-with-newline}}\n{}: {{usage}}\n\n{{all-args}}{{after-help}}", t("cli.usage")));
    let names: Vec<String> = cmd.get_subcommands().map(|c| c.get_name().to_string()).collect();
    for name in names {
        cmd = cmd.mut_subcommand(name, localize_chrome);
    }
    cmd
}

/// 终端界面用的引擎：Agent 的钩子交给这个程序（`duolo __hook`）。
fn engine_with_hooks(paths: Paths) -> Result<Engine> {
    let mut engine = Engine::open(paths)?;
    if let Ok(exe) = std::env::current_exe() {
        engine.set_hook_program(exe);
    }
    Ok(engine)
}

/// 打开引擎，启动时发现的问题写到 stderr。
fn open_engine(paths: Paths) -> Result<Engine> {
    let mut engine = Engine::open(paths)?;
    for notice in engine.take_notices() {
        eprintln!("duolo: {}", notice.localized());
    }
    Ok(engine)
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
            eprintln!("{}", t("cli.appNotFound"));
            duolo_tui::run(engine_with_hooks(paths)?, &dir)
        }
        Some(Cmd::Tui { path }) => duolo_tui::run(engine_with_hooks(paths)?, &path.unwrap_or(cwd)),
        Some(Cmd::Add { path }) => {
            let mut engine = open_engine(paths)?;
            let id = engine.add_project(&path)?;
            println!("{}", tf("cli.projectAdded", &[("id", &id)]));
            Ok(())
        }
        Some(Cmd::New { prompt, project, writer, reviewer }) => {
            let mut engine = open_engine(paths)?;
            let project = match project {
                Some(id) => {
                    let id = ProjectId(id);
                    if engine.state().project(&id).is_none() {
                        bail!(Plain(tf("cli.noSuchProject", &[("id", &id)])));
                    }
                    id
                }
                None => engine.add_project(&cwd)?,
            };
            let opts =
                TaskOptions { writer: writer.map(AgentKind), reviewer: reviewer.map(AgentKind), ..Default::default() };
            let id = engine.create_task_with(&project, &prompt.join(" "), &opts)?;
            let task = engine.state().task(id).expect("task was just created");
            println!("{}", tf("cli.taskCreated", &[("id", &id), ("title", &task.title)]));
            println!("  {}", tf("cli.taskBranch", &[("branch", &task.branch)]));
            println!("  {}", t("cli.taskHint"));
            Ok(())
        }
        Some(Cmd::Switch { task, writer, reviewer }) => {
            if writer.is_none() && reviewer.is_none() {
                bail!(Plain(t("cli.switchWhat")));
            }
            let mut engine = open_engine(paths)?;
            let id = TaskId(task);
            // 只改状态：Agent 在桌面应用里跑的话，由它停掉原来的、在打开任务时启动新的。
            if let Some(kind) = writer.map(AgentKind) {
                engine.switch_writer(id, kind.clone())?;
                println!("{}", tf("cli.writerSwitched", &[("id", &id), ("agent", &engine.agents().name(&kind))]));
            }
            if let Some(kind) = reviewer.map(AgentKind) {
                engine.set_reviewer(id, kind.clone())?;
                println!("{}", tf("cli.reviewerSwitched", &[("id", &id), ("agent", &engine.agents().name(&kind))]));
            }
            Ok(())
        }
        Some(Cmd::Ls) => {
            let engine = open_engine(paths)?;
            let state = engine.state();
            if state.tasks.is_empty() {
                println!("{}", t("cli.noTasks"));
            }
            let now = now_secs();
            for project in &state.projects {
                println!("{}  {}", project.id, project.root.display());
                for task in state.tasks_of(&project.id) {
                    let agents: Vec<String> =
                        task.agents.iter().map(|a| a.role.label(&engine.agents().name(&a.kind))).collect();
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
        // Windows / Linux：直接启动应用程序；已经在运行时，新实例会把目录转交给它后退出。
        let Some(app) = installed_app() else { return Ok(false) };
        let mut cmd = std::process::Command::new(app);
        if let Ok(root) = duolo_git::repo_root(dir) {
            cmd.arg(root);
        }
        cmd.stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .context("cannot start Duolo")?;
        return Ok(true);
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

/// Windows / Linux 上装好的桌面应用：Windows 安装程序放在 `%LOCALAPPDATA%\Duolo`（只给当前用户装）
/// 或 `%ProgramFiles%\Duolo`；Linux 的 deb 包装在 PATH 上。
fn installed_app() -> Option<std::path::PathBuf> {
    if cfg!(windows) {
        return ["LOCALAPPDATA", "ProgramFiles"]
            .into_iter()
            .filter_map(std::env::var_os)
            .map(|dir| std::path::PathBuf::from(dir).join("Duolo").join("Duolo.exe"))
            .find(|exe| exe.exists());
    }
    which::which("Duolo").ok()
}

fn doctor(paths: &Paths) -> Result<()> {
    let engine = open_engine(paths.clone())?;
    let (config, agents) = (engine.config(), engine.agents());
    println!("{}{}", pad(&t("cli.doctor.data"), 12), paths.root().display());
    for check in check_environment(config, agents) {
        println!("{} {}{}", if check.ok { "✓" } else { "✕" }, pad(&check.name, 10), check.detail);
    }
    println!("{}{}", pad(&t("cli.doctor.writer"), 12), agents.name(&config.writer));
    println!("{}{}", pad(&t("cli.doctor.reviewer"), 12), agents.name(&config.reviewer));
    if !config.voice.is_configured() {
        println!("{}", tf("cli.doctor.voiceHint", &[("path", &paths.config_file().display())]));
    }
    Ok(())
}

/// 按显示宽度补空格（中日韩文字占两格）。
fn pad(text: &str, width: usize) -> String {
    use unicode_width::UnicodeWidthStr;
    format!("{text}{}", " ".repeat(width.saturating_sub(text.width()).max(1)))
}
