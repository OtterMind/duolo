//! 根据需求给新分支起名：调用一个小模型命令（见 [`crate::Engine::namer`]），
//! 把结果整理成合法的分支名。没装、超时或结果不能用时返回 None，调用方用默认名字。

use std::time::Duration;

use duolo_core::process::{output_within, OneShot, Prompted};
use duolo_core::text::clean_branch_name;

/// 起名最长等多久。在后台做，慢的模型要十几秒；再慢就不如直接用默认名字。
const NAMING_TIMEOUT: Duration = Duration::from_secs(25);

const NAMING_PROMPT: &str = "Suggest a git branch name for the task below. \
Use the form <type>/<short-kebab-case-description> where <type> is one of feat, fix, refactor, docs, test, chore. \
Use English words only, at most 5 words after the slash. Reply with the branch name only, nothing else.\n\nTask:\n";

/// 阻塞调用，请在后台线程里执行。
pub fn suggest_branch(namer: &OneShot, prompt: &str) -> Option<String> {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return None;
    }
    let namer = Prompted::new(namer, Some(&format!("{NAMING_PROMPT}{prompt}")))?;
    let (status, out) = output_within(namer, NAMING_TIMEOUT).ok()?;
    status.success().then(|| clean_branch_name(&out))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(script: &str) -> OneShot {
        ["sh", "-c", script, "sh"].map(String::from).to_vec().into()
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
        assert_eq!(suggest_branch(&OneShot::default(), "x"), None);
        assert_eq!(suggest_branch(&sh("echo feat/x"), "  "), None);
        assert_eq!(suggest_branch(&vec!["definitely-not-a-command".to_string()].into(), "x"), None);
    }
}
