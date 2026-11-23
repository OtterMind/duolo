//! 启动外部命令（git、claude、codex、语音识别…）时，按当前系统的规则找到可执行文件。
//!
//! Windows 上 npm 装的 `claude`、`codex` 实际是 `claude.cmd`，直接 `Command::new("claude")` 只会找 `.exe`，
//! 会报找不到命令。这里用 `which` 按 PATH 和 PATHEXT 查找，找不到时原样交给系统（报错由调用方处理）。

use std::ffi::OsStr;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use crate::proctree::ProcessTree;

/// 结束命令时，最长等它的进程消失多久。
const KILL_WAIT: Duration = Duration::from_secs(2);

/// 可执行文件的完整路径；找不到时为 None。
pub fn find(program: impl AsRef<OsStr>) -> Option<PathBuf> {
    which::which(program).ok()
}

/// 可执行文件的完整路径；找不到时返回原来的名字。
pub fn resolve(program: impl AsRef<OsStr>) -> PathBuf {
    let program = program.as_ref();
    find(program).unwrap_or_else(|| PathBuf::from(program))
}

/// 和 `Command::new` 一样，但先按当前系统的规则找到可执行文件；Windows 上不弹出控制台窗口。
pub fn command(program: impl AsRef<OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(resolve(program));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // 桌面应用自己没有控制台，不加这个标志每运行一次 git 都会闪一个黑窗口。
        cmd.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    }
    cmd
}

/// 运行一次就退出的命令：给它一段提示词，结果从 stdout 读。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OneShot {
    pub argv: Vec<String>,
    /// 放在提示词前面的参数（一般是 `--`），见 [`prompt_args`]。
    pub separator: Option<String>,
    /// 提示词改从标准输入给时追加的参数。
    pub stdin_args: Vec<String>,
    /// 不传给它的环境变量。
    pub env_remove: Vec<String>,
    /// 运行前关掉不在允许名单里的功能（见 [`FeatureGate`]）。
    pub features: Option<FeatureGate>,
}

/// 只留下允许的功能：运行前先问程序开着哪些功能，不在 `allow` 里的都用 `disable` 关掉。
/// 程序以后新加的功能（例如连到云端的工具）也就默认关着，不用每次升级都补名单。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FeatureGate {
    /// 列出功能的参数（跟在程序后面）。输出每行 `<功能名> … <true|false>`，最后一列是开没开。
    pub list: Vec<String>,
    pub allow: Vec<String>,
    /// 关掉一个功能的参数，`{feature}` 换成功能名。
    pub disable: Vec<String>,
}

impl FeatureGate {
    /// 要关掉的功能的参数。列不出来时为空（还有命令本身写死的限制）。
    pub fn disable_args(&self, program: &str) -> Vec<String> {
        enabled_features(program, &self.list)
            .into_iter()
            .filter(|name| !self.allow.contains(name))
            .flat_map(|name| self.disable.iter().map(move |arg| arg.replace("{feature}", &name)))
            .collect()
    }
}

/// 程序开着的功能，按程序文件和修改时间记住（程序自己升级后重新问）。
fn enabled_features(program: &str, list: &[String]) -> Vec<String> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    /// 按（程序的真实路径，修改时间）记住的结果。
    type Cache = Mutex<HashMap<(PathBuf, Option<std::time::SystemTime>), Vec<String>>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let path = resolve(program);
    let real = std::fs::canonicalize(&path).unwrap_or(path);
    let key = (real.clone(), std::fs::metadata(&real).and_then(|m| m.modified()).ok());
    let cache = CACHE.get_or_init(Mutex::default);
    if let Some(found) = cache.lock().unwrap_or_else(|p| p.into_inner()).get(&key) {
        return found.clone();
    }
    let mut cmd = command(program);
    cmd.args(list).stdin(Stdio::null()).stderr(Stdio::null());
    let Ok(output) = cmd.output() else { return Vec::new() };
    if !output.status.success() {
        return Vec::new();
    }
    let found = parse_enabled(&String::from_utf8_lossy(&output.stdout));
    cache.lock().unwrap_or_else(|p| p.into_inner()).insert(key, found.clone());
    found
}

/// `<功能名> … <true|false>` 的行里开着的功能名。
fn parse_enabled(listing: &str) -> Vec<String> {
    let name_ok =
        |name: &str| !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    listing
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let name = words.next()?;
            (words.last() == Some("true") && name_ok(name)).then(|| name.to_string())
        })
        .collect()
}

impl From<Vec<String>> for OneShot {
    fn from(argv: Vec<String>) -> Self {
        Self { argv, ..Self::default() }
    }
}

impl OneShot {
    pub fn program(&self) -> &str {
        self.argv.first().map_or("", String::as_str)
    }
}

/// 提示词作为参数时怎么写，免得以 `-` 开头（例如列表）时被当成选项：
/// 前面放 `separator`；命令不支持的话在最前面加一个空格，意思不变。
pub fn prompt_args(separator: Option<&str>, prompt: &str) -> Vec<String> {
    match separator {
        Some(separator) => vec![separator.to_string(), prompt.to_string()],
        None if prompt.starts_with('-') => vec![format!(" {prompt}")],
        None => vec![prompt.to_string()],
    }
}

/// 把 `prompt` 交给 [`OneShot`] 的命令。提示词一般是最后一个参数。
///
/// Windows 上 npm 装的 `claude.cmd`、`codex.cmd` 是批处理文件，参数里不能有换行（Rust 直接拒绝），
/// 提示词改从标准输入给：`claude -p` 和 `codex exec` 没有提示词参数时都从标准输入读。
#[derive(Debug)]
pub struct Prompted {
    pub cmd: Command,
    /// 要写进标准输入的提示词。
    stdin: Option<String>,
}

impl Prompted {
    /// `argv` 为空时返回 None。
    pub fn new(command: &OneShot, prompt: Option<&str>) -> Option<Self> {
        let program = command.argv.first()?;
        Some(Self::build(command, prompt, is_batch_file(resolve(program))))
    }

    fn build(one: &OneShot, prompt: Option<&str>, batch: bool) -> Self {
        let mut cmd = command(one.program());
        cmd.args(one.argv.iter().skip(1));
        if let Some(gate) = &one.features {
            cmd.args(gate.disable_args(one.program()));
        }
        for key in &one.env_remove {
            cmd.env_remove(key);
        }
        let mut stdin = None;
        match prompt {
            Some(prompt) if batch => {
                cmd.args(&one.stdin_args);
                stdin = Some(prompt.to_string());
            }
            Some(prompt) => {
                cmd.args(prompt_args(one.separator.as_deref(), prompt));
            }
            None => {}
        }
        cmd.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() });
        Self { cmd, stdin }
    }

    /// 启动，连同它之后启动的子进程一起管理（见 [`spawn_tree`]）。
    pub fn spawn(mut self) -> io::Result<(Child, ProcessTree)> {
        let (mut child, tree) = spawn_tree(&mut self.cmd)?;
        if let (Some(text), Some(mut pipe)) = (self.stdin, child.stdin.take()) {
            // 提示词可能比管道的缓冲区大，另开线程写，免得和读输出的一方互相等。
            std::thread::spawn(move || pipe.write_all(text.as_bytes()));
        }
        Ok((child, tree))
    }
}

/// 启动命令，返回它的进程树：结束时用 [`ProcessTree::kill`]，它启动的子进程也一起结束。
/// 只结束它自己是不够的：npm 装的 `codex` 是一层 node，真正干活的进程会留下来继续跑。
pub fn spawn_tree(cmd: &mut Command) -> io::Result<(Child, ProcessTree)> {
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(cmd, 0);
    let child = cmd.spawn()?;
    let tree = ProcessTree::of(&child);
    Ok((child, tree))
}

/// 运行 `command`（和它启动的子进程），读它的 stdout；超过 `timeout` 就全部结束，返回 `TimedOut`。
pub fn output_within(mut command: Prompted, timeout: Duration) -> io::Result<(ExitStatus, String)> {
    command.cmd.stdout(Stdio::piped()).stderr(Stdio::null());
    let (mut child, tree) = command.spawn()?;
    let mut stdout = child.stdout.take().ok_or(io::ErrorKind::BrokenPipe)?;
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = stdout.read_to_end(&mut out);
        out
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait()? {
            Some(status) => break Some(status),
            None if Instant::now() >= deadline => break None,
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    // 正常结束的，它留下的后台进程也可能还占着输出管道，不结束就一直读不完。
    tree.kill(None, KILL_WAIT);
    let Some(status) = status else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(io::ErrorKind::TimedOut.into());
    };
    let out = reader.join().unwrap_or_default();
    Ok((status, String::from_utf8_lossy(&out).into_owned()))
}

/// Windows 上 npm 装的命令（`claude.cmd`、`codex.cmd`）是批处理文件，不能直接作为程序启动，
/// 要交给 `cmd.exe /c`。其他系统上永远是 false。
pub fn is_batch_file(program: impl AsRef<OsStr>) -> bool {
    cfg!(windows) && is_batch_path(program.as_ref())
}

fn is_batch_path(program: &OsStr) -> bool {
    std::path::Path::new(program)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat"))
}

/// 用 cmd.exe 运行批处理文件的命令行。调用方把它放进环境变量 `X`，再运行 `cmd.exe /d /c %X%`：
/// cmd 展开变量后才解析，值里的 `%` 不会再被展开，`&`、`|` 等在引号里也只是普通字符。
/// 每个参数加引号，`"` 写成 `""`。参数里有换行时返回 None：cmd.exe 的命令行没法带上换行。
pub fn batch_command_line(script: &OsStr, args: &[String]) -> Option<String> {
    let mut line = String::new();
    for (i, arg) in std::iter::once(script.to_string_lossy().into_owned()).chain(args.iter().cloned()).enumerate() {
        if arg.contains(['\r', '\n']) {
            return None;
        }
        if i > 0 {
            line.push(' ');
        }
        line.push('"');
        let escaped = arg.replace('"', "\"\"");
        // 结尾的反斜杠会把后面的引号转义掉，要成对写。
        let trailing = escaped.len() - escaped.trim_end_matches('\\').len();
        line.push_str(&escaped);
        line.push_str(&"\\".repeat(trailing));
        line.push('"');
    }
    Some(line)
}

/// 文件或目录正被别的进程占用（Windows 上才会这样）：稍等再试可能就好了。
pub fn is_busy(err: &io::Error) -> bool {
    // 5 拒绝访问、32 共享冲突、33 锁定冲突；后两个 Rust 没有归到 PermissionDenied。
    cfg!(windows) && matches!(err.raw_os_error(), Some(5 | 32 | 33))
}

/// 用户的家目录（macOS、Linux 是 `$HOME`，Windows 是 `%USERPROFILE%`）。
pub fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_batch_command_lines() {
        assert!(is_batch_path(OsStr::new(r"C:\npm\claude.CMD")));
        assert!(!is_batch_path(OsStr::new(r"C:\bin\codex.exe")));
        let line = batch_command_line(OsStr::new(r"C:\npm\codex.cmd"), &["resume".into(), r#"50% "done" \"#.into()]);
        assert_eq!(line.as_deref(), Some(r#""C:\npm\codex.cmd" "resume" "50% ""done"" \\""#));
        assert_eq!(batch_command_line(OsStr::new("a.cmd"), &["two\nlines".into()]), None);
    }

    #[test]
    fn only_allowed_features_stay_on() {
        let listing = "apps                 stable             true\nshell_tool  stable  true\n\
                       removed_one          removed            false\nName Stage Enabled\n";
        assert_eq!(parse_enabled(listing), ["apps", "shell_tool"]);
        // 假程序：`features list` 时列出功能，其他时候打印收到的参数。
        let dir = tempfile::tempdir().unwrap();
        let prog = dir.path().join("agent.sh");
        std::fs::write(&prog, "if [ \"$1\" = features ]; then printf 'apps x true\\nshell_tool x true\\nold x false\\n'; else echo \"$*\"; fi\n").unwrap();
        let gate = FeatureGate {
            list: vec![prog.display().to_string(), "features".into(), "list".into()],
            allow: vec!["shell_tool".into()],
            disable: vec!["-c".into(), "features.{feature}=false".into()],
        };
        let one = OneShot {
            argv: vec!["sh".into(), "-c".into(), "echo \"$*\"".into(), "sh".into(), "exec".into()],
            features: Some(gate),
            ..Default::default()
        };
        let mut prompted = Prompted::build(&one, Some("go"), false);
        prompted.cmd.stdout(Stdio::piped());
        let (child, _tree) = prompted.spawn().unwrap();
        let out = String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap();
        assert_eq!(out.trim(), "exec -c features.apps=false go", "新功能默认关掉，允许的留着");
    }

    #[test]
    fn prompt_goes_to_stdin_for_batch_files() {
        let run = |batch: bool| {
            // 打印参数个数和标准输入。提示词里有空格，免得 Windows 上 Git 自带的 sh 按换行拆开参数。
            let argv = ["sh", "-c", "echo $#; cat", "sh"].map(String::from).to_vec();
            let mut prompted = Prompted::build(&argv.into(), Some("two words\nand lines"), batch);
            prompted.cmd.stdout(Stdio::piped());
            let (child, _tree) = prompted.spawn().unwrap();
            String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap()
        };
        assert_eq!(run(false), "1\n");
        assert_eq!(run(true), "0\ntwo words\nand lines");
        assert!(Prompted::new(&OneShot::default(), Some("x")).is_none());
    }

    #[test]
    fn prompt_is_not_read_as_an_option() {
        let run = |one: &OneShot, batch: bool| {
            let mut prompted = Prompted::build(one, Some("- two words"), batch);
            prompted.cmd.stdout(Stdio::piped());
            let (child, _tree) = prompted.spawn().unwrap();
            String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap()
        };
        let argv = ["sh", "-c", "for a; do echo \"[$a]\"; done; cat", "sh"].map(String::from).to_vec();
        let mut one = OneShot::from(argv);
        assert_eq!(run(&one, false), "[ - two words]\n");
        one.separator = Some("--".into());
        one.stdin_args = vec!["-".into()];
        assert_eq!(run(&one, false), "[--]\n[- two words]\n");
        assert_eq!(run(&one, true), "[-]\n- two words", "从标准输入给的时候不用分隔");
    }

    #[test]
    fn output_is_limited_in_time() {
        let sh = |script: &str| {
            let argv = ["sh", "-c", script].map(String::from).to_vec();
            output_within(Prompted::new(&argv.into(), None).unwrap(), Duration::from_secs(1))
        };
        let (status, out) = sh("echo hi; exit 3").unwrap();
        assert_eq!((status.code(), out.trim()), (Some(3), "hi"));
        let started = Instant::now();
        assert_eq!(sh("sleep 30").unwrap_err().kind(), io::ErrorKind::TimedOut);
        // 命令结束了，它留下的后台进程还占着输出管道。
        assert_eq!(sh("sleep 30 & echo done").unwrap().1.trim(), "done");
        assert!(started.elapsed() < Duration::from_secs(10), "{:?}", started.elapsed());
    }

    /// 真实的批处理文件：带换行和特殊字符的提示词要原样到它手里。
    #[cfg(windows)]
    #[test]
    fn batch_file_receives_a_multi_line_prompt() {
        let dir = tempfile::tempdir().unwrap();
        // 路径里有空格，像 C:\Program Files 那样。
        let bin = dir.path().join("my tools");
        std::fs::create_dir(&bin).unwrap();
        let script = bin.join("codex.cmd");
        std::fs::write(&script, "@echo off\r\necho args=%*\r\nfindstr \"^\"\r\n").unwrap();
        let prompt = "first line\nsecond & \"quoted\" 100% %PATH% ^ > <\nlast";
        let argv = [script.display().to_string(), "exec".into(), "--color".into(), "never".into()];
        let mut prompted = Prompted::new(&argv.to_vec().into(), Some(prompt)).unwrap();
        prompted.cmd.stdout(Stdio::piped());
        let (child, _tree) = prompted.spawn().unwrap();
        let out = String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap().replace('\r', "");
        assert!(out.contains("args=exec --color never") && out.contains(prompt), "{out}");
    }

    #[test]
    fn finds_programs_on_path() {
        let git = resolve("git");
        assert!(git.is_absolute(), "{}", git.display());
        assert_eq!(resolve("definitely-not-a-command"), PathBuf::from("definitely-not-a-command"));
    }
}
