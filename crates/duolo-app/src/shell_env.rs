//! 从 Dock、Finder 或桌面图标启动的应用没有用户在 shell 里设置的环境变量：PATH 只有系统默认的（/usr/bin:/bin…），
//! 找不到装在 Homebrew、~/.local/bin 等位置的 claude 和 codex；代理、API 密钥也没有，Agent 会连不上网。
//! 启动时从用户的登录 shell 读一遍。

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::Read;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

/// 开头和结尾的标记，由 shell 里的 printf 用两段拼出来：有的 shell（dash）把上一条命令的最后一个参数放进
/// 环境变量 `_`，标记要是整个写在参数里，就会在环境变量中间提前出现。
const MARKER: &str = "__DUOLO_ENV_";
const BEGIN: &str = "__DUOLO_ENV_BEGIN";
const END: &str = "__DUOLO_ENV_END";
const TIMEOUT: Duration = Duration::from_secs(5);

/// 描述那个 shell 自己的变量，不带进来。
const SHELL_ONLY: &[&str] = &[
    "_",
    "PWD",
    "OLDPWD",
    "SHLVL",
    "SHELL",
    "TERM",
    "TERM_PROGRAM",
    "TERM_PROGRAM_VERSION",
    "TERM_SESSION_ID",
    "TMPDIR",
];

/// 补上用户在 shell 里设置的环境变量：PATH 换成登录 shell 的，其他的只补当前没有的。读取失败时保持不变。
/// 要在启动其他线程之前调用。
pub fn load() {
    let current: HashMap<OsString, OsString> = std::env::vars_os().collect();
    for (name, value) in additions(&current, login_env().unwrap_or_default()) {
        std::env::set_var(name, value);
    }
    // 从 Dock 启动时没有语言设置，命令行程序会按 ASCII 处理中文：和系统自带的终端一样用 UTF-8。
    if ["LC_ALL", "LC_CTYPE", "LANG"].iter().all(|name| std::env::var_os(name).is_none_or(|v| v.is_empty())) {
        std::env::set_var("LANG", if cfg!(target_os = "macos") { "en_US.UTF-8" } else { "C.UTF-8" });
    }
}

fn additions(
    current: &HashMap<OsString, OsString>,
    login: HashMap<OsString, OsString>,
) -> impl Iterator<Item = (OsString, OsString)> + '_ {
    login.into_iter().filter(|(name, _)| {
        let shell_only = SHELL_ONLY.iter().any(|s| name.as_os_str() == *s);
        !shell_only && (name == "PATH" || !current.contains_key(name))
    })
}

fn login_env() -> Option<HashMap<OsString, OsString>> {
    let fallback = if cfg!(target_os = "macos") { "/bin/zsh" } else { "/bin/sh" };
    let shell = std::env::var("SHELL").unwrap_or_else(|_| fallback.into());
    // 交互式登录 shell 才会读 .zshrc；输出前后加标记，避开 shell 启动时打印的其他内容。
    // 变量的值里可能有换行，用 \0 分隔。
    let script = format!("printf {MARKER}%s BEGIN; /usr/bin/env -0; printf {MARKER}%s END");
    // 不放进单独的进程组：从终端启动时，交互式 shell 在后台进程组里会被系统暂停。
    let mut child = Command::new(shell)
        .args(["-ilc", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let (sender, receiver) = mpsc::channel();
    // 边运行边读，读到结尾的标记就够了：.zshrc 里启动的后台程序可能一直占着输出管道，等不到它关闭。
    std::thread::spawn(move || {
        let (mut out, mut buf) = (Vec::new(), [0; 8192]);
        while let Ok(n @ 1..) = stdout.read(&mut buf) {
            out.extend_from_slice(&buf[..n]);
            if markers(&out).is_some() {
                break;
            }
        }
        let _ = sender.send(out);
    });
    let out = receiver.recv_timeout(TIMEOUT).ok();
    let _ = child.kill();
    let _ = child.wait();
    parse(&out?)
}

/// 两个标记之间的范围。
fn markers(out: &[u8]) -> Option<std::ops::Range<usize>> {
    let find = |from: usize, marker: &str| {
        out[from..].windows(marker.len()).position(|w| w == marker.as_bytes()).map(|i| from + i)
    };
    let start = find(0, BEGIN)? + BEGIN.len();
    Some(start..find(start, END)?)
}

fn parse(stdout: &[u8]) -> Option<HashMap<OsString, OsString>> {
    let vars: HashMap<OsString, OsString> = stdout[markers(stdout)?]
        .split(|b| *b == 0)
        .filter_map(|entry| {
            let eq = entry.iter().position(|b| *b == b'=').filter(|i| *i > 0)?;
            let (name, value) = (&entry[..eq], &entry[eq + 1..]);
            Some((OsString::from_vec(name.to_vec()), OsString::from_vec(value.to_vec())))
        })
        .collect();
    // PATH 一定有；没有说明读到的不是环境变量。
    vars.get(std::ffi::OsStr::from_bytes(b"PATH")).is_some_and(|p| !p.is_empty()).then_some(vars)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_variables_and_ignores_shell_noise() {
        let out = format!(
            "welcome!\n{BEGIN}PATH=/opt/homebrew/bin:/usr/bin\0HTTPS_PROXY=http://127.0.0.1:7890\0\
             MULTI=a\nb=c\0_=BEGIN\0{END}bye"
        );
        let vars = parse(out.as_bytes()).unwrap();
        assert_eq!(vars.len(), 4);
        assert_eq!(vars[&OsString::from("PATH")], "/opt/homebrew/bin:/usr/bin");
        assert_eq!(vars[&OsString::from("MULTI")], "a\nb=c");
        assert!(parse(b"nothing").is_none());
        assert!(parse(format!("{BEGIN}oops{END}").as_bytes()).is_none());
    }

    #[test]
    fn keeps_what_is_already_set() {
        let map = |pairs: &[(&str, &str)]| pairs.iter().map(|(k, v)| (k.into(), v.into())).collect();
        let current: HashMap<OsString, OsString> = map(&[("PATH", "/usr/bin"), ("HTTPS_PROXY", "mine")]);
        let login = map(&[("PATH", "/opt/bin"), ("HTTPS_PROXY", "rc"), ("ANTHROPIC_API_KEY", "k"), ("SHLVL", "2")]);
        let added: HashMap<OsString, OsString> = additions(&current, login).collect();
        assert_eq!(added, map(&[("PATH", "/opt/bin"), ("ANTHROPIC_API_KEY", "k")]));
    }

    #[test]
    fn reads_a_real_shell() {
        std::env::set_var("DUOLO_TEST_VALUE", "two\nlines");
        let vars = login_env().unwrap();
        assert_eq!(vars[&OsString::from("DUOLO_TEST_VALUE")], "two\nlines");
    }
}
