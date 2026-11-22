//! 根据需求给新分支起名：调用配置里的小模型命令（默认 `claude -p --model haiku`），
//! 把结果整理成合法的分支名。没装、超时或结果不能用时返回 None，调用方用默认名字。

use std::io::Read;
use std::process::Stdio;
use std::thread;
use std::time::{Duration, Instant};

use duolo_core::process::Prompted;
use duolo_core::text::clean_branch_name;

/// 起名最长等多久。界面在用户停止输入后调用，太慢就不如直接用默认名字。
const NAMING_TIMEOUT: Duration = Duration::from_secs(12);

/// 超时后结束它，最长等它消失多久。
const KILL_WAIT: Duration = Duration::from_secs(2);

const NAMING_PROMPT: &str = "Suggest a git branch name for the task below. \
Use the form <type>/<short-kebab-case-description> where <type> is one of feat, fix, refactor, docs, test, chore. \
Use English words only, at most 5 words after the slash. Reply with the branch name only, nothing else.\n\nTask:\n";

/// 阻塞调用，请在后台线程里执行。
pub fn suggest_branch(namer: &[String], prompt: &str) -> Option<String> {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return None;
    }
    let mut namer = Prompted::new(namer, Some(&format!("{NAMING_PROMPT}{prompt}")))?;
    namer.cmd.stdout(Stdio::piped()).stderr(Stdio::null());
    let (mut child, tree) = namer.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = thread::spawn(move || {
        let mut out = String::new();
        let _ = stdout.read_to_string(&mut out);
        out
    });
    let deadline = Instant::now() + NAMING_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) | Err(_) => return None,
            Ok(None) if Instant::now() >= deadline => {
                tree.kill(None, KILL_WAIT);
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => thread::sleep(Duration::from_millis(50)),
        }
    }
    clean_branch_name(&reader.join().ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(script: &str) -> Vec<String> {
        vec!["sh".into(), "-c".into(), script.into(), "sh".into()]
    }

    #[test]
    fn uses_model_output() {
        assert_eq!(
            suggest_branch(&sh("echo '`feat/Order CSV export`'"), "x").as_deref(),
            Some("feat/order-csv-export")
        );
    }

    #[test]
    fn falls_back_to_none() {
        assert_eq!(suggest_branch(&sh("exit 1"), "x"), None);
        assert_eq!(suggest_branch(&[], "x"), None);
        assert_eq!(suggest_branch(&sh("echo feat/x"), "  "), None);
        assert_eq!(suggest_branch(&["definitely-not-a-command".into()], "x"), None);
    }
}
