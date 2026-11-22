//! 启动外部命令（git、claude、codex、语音识别…）时，按当前系统的规则找到可执行文件。
//!
//! Windows 上 npm 装的 `claude`、`codex` 实际是 `claude.cmd`，直接 `Command::new("claude")` 只会找 `.exe`，
//! 会报找不到命令。这里用 `which` 按 PATH 和 PATHEXT 查找，找不到时原样交给系统（报错由调用方处理）。

use std::ffi::OsStr;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use crate::proctree::ProcessTree;

/// 可执行文件的完整路径；找不到时返回原来的名字。
pub fn resolve(program: impl AsRef<OsStr>) -> PathBuf {
    let program = program.as_ref();
    which::which(program).unwrap_or_else(|_| PathBuf::from(program))
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

/// 运行 `argv` 并把 `prompt` 交给它的命令。提示词一般是最后一个参数。
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
    pub fn new(argv: &[String], prompt: Option<&str>) -> Option<Self> {
        let (program, args) = argv.split_first()?;
        Some(Self::build(program, args, prompt, is_batch_file(resolve(program))))
    }

    fn build(program: &str, args: &[String], prompt: Option<&str>, batch: bool) -> Self {
        let mut cmd = command(program);
        cmd.args(args);
        let mut stdin = None;
        match prompt {
            Some(prompt) if batch => stdin = Some(prompt.to_string()),
            Some(prompt) => {
                cmd.arg(prompt);
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
    fn prompt_goes_to_stdin_for_batch_files() {
        let run = |batch: bool| {
            // 打印参数个数和标准输入。提示词里有空格，免得 Windows 上 Git 自带的 sh 按换行拆开参数。
            let args = ["-c", "echo $#; cat", "sh"].map(String::from);
            let mut prompted = Prompted::build("sh", &args, Some("two words\nand lines"), batch);
            prompted.cmd.stdout(Stdio::piped());
            let (child, _tree) = prompted.spawn().unwrap();
            String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap()
        };
        assert_eq!(run(false), "1\n");
        assert_eq!(run(true), "0\ntwo words\nand lines");
        assert!(Prompted::new(&[], Some("x")).is_none());
    }

    #[test]
    fn finds_programs_on_path() {
        let git = resolve("git");
        assert!(git.is_absolute(), "{}", git.display());
        assert_eq!(resolve("definitely-not-a-command"), PathBuf::from("definitely-not-a-command"));
    }
}
