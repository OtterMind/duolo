//! 文本小工具：任务标题、分支 slug、时长显示。

use std::time::{SystemTime, UNIX_EPOCH};

const TITLE_MAX_CHARS: usize = 28;
const SLUG_MAX_CHARS: usize = 24;

/// 从需求原文取第一行作为任务标题，过长时截断。
pub fn derive_title(prompt: &str) -> String {
    let line = prompt.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("New task");
    truncate_chars(line, TITLE_MAX_CHARS)
}

/// 没有需求的任务的默认标题。早期版本写进数据的是中文"新任务"，也算默认标题。
pub fn is_default_title(title: &str) -> bool {
    title == derive_title("") || title == LEGACY_DEFAULT_TITLE
}

/// 早期版本的默认标题，只用来识别旧数据。
const LEGACY_DEFAULT_TITLE: &str = "\u{65b0}\u{4efb}\u{52a1}";

/// 按字符数截断，截断时加省略号。
pub fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// 生成分支名用的 ASCII slug。中文标题没有可用字符时返回 `task`。
pub fn slug(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
        if out.len() >= SLUG_MAX_CHARS {
            break;
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "task".into()
    } else {
        out
    }
}

/// 把模型给的分支名整理成合法、简短的 git 分支名：取第一行，去掉引号和反引号，
/// 只留小写字母、数字、`/`、`-`、`.`；整理后没有字母数字时返回 None。
pub fn clean_branch_name(raw: &str) -> Option<String> {
    let line = raw.lines().map(str::trim).find(|l| !l.is_empty())?;
    let line = line.trim_matches(|c| matches!(c, '`' | '"' | '\'' | '*'));
    let mut out = String::new();
    for ch in line.chars() {
        let ch = ch.to_ascii_lowercase();
        if ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.') {
            out.push(ch);
        } else if !out.ends_with('-') && !out.ends_with('/') {
            out.push('-');
        }
        if out.len() >= BRANCH_MAX_CHARS {
            break;
        }
    }
    let out = out.trim_matches(|c| matches!(c, '-' | '/' | '.')).replace("..", ".");
    out.chars().any(|c| c.is_ascii_alphanumeric()).then_some(out)
}

const BRANCH_MAX_CHARS: usize = 50;

/// 从 `--version` 的输出里取出版本号，例如 `codex-cli 0.169.1` → `0.169.1`、`2.1.311 (Claude Code)` → `2.1.311`。
pub fn version_in(output: &str) -> Option<String> {
    output
        .lines()
        .next()?
        .split_whitespace()
        .find(|w| w.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .map(Into::into)
}

/// `2.1.311`、`v0.169.1`、`1.2.3-beta.1` 的数字部分；不像版本号时为 None。
pub fn parse_version(text: &str) -> Option<Vec<u64>> {
    let core = text.trim().trim_start_matches('v').split(['-', '+']).next()?;
    let parts: Option<Vec<u64>> = core.split('.').map(|p| p.parse().ok()).collect();
    parts.filter(|p| !p.is_empty())
}

/// `version` 比 `minimum` 旧。读不懂的版本号不算旧（照样让用）。
pub fn is_older(version: &str, minimum: &str) -> bool {
    match (parse_version(version), parse_version(minimum)) {
        (Some(v), Some(m)) => v < m,
        _ => false,
    }
}

pub fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// 把秒数显示成 `12s`、`5m`、`3h`、`2d`。
pub fn format_elapsed(secs: u64) -> String {
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3599 => format!("{}m", secs / 60),
        3600..=86_399 => format!("{}h", secs / 3600),
        _ => format!("{}d", secs / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(is_older("0.135.9", "0.136.0") && !is_older("0.169.1", "0.136.0"));
        assert!(is_older("2.1.9", "2.1.10"), "按数字比，不按字符串");
        assert!(!is_older("v2.1.311", "2.1.311") && !is_older("2.1.311-beta.1", "2.1.300"));
        assert!(!is_older("unknown", "1.0.0"), "读不懂的不拦");
        assert_eq!(parse_version("1.x"), None);
    }

    #[test]
    fn titles() {
        assert_eq!(derive_title("\n  接入 Stripe 订阅\n细节"), "接入 Stripe 订阅");
        assert_eq!(derive_title(""), "New task");
        assert!(is_default_title("New task"));
        assert!(is_default_title(LEGACY_DEFAULT_TITLE));
        assert!(!is_default_title("Export CSV"));
        assert_eq!(truncate_chars("一二三四五", 3), "一二…");
    }

    #[test]
    fn slugs() {
        assert_eq!(slug("Add Stripe subscriptions!"), "add-stripe-subscriptions");
        assert_eq!(slug("接入 Stripe 订阅"), "stripe");
        assert_eq!(slug("订单导出"), "task");
    }

    #[test]
    fn branch_names_from_model_output() {
        assert_eq!(clean_branch_name("`feat/order-csv-export`\n").as_deref(), Some("feat/order-csv-export"));
        assert_eq!(clean_branch_name("Feat/Order CSV Export").as_deref(), Some("feat/order-csv-export"));
        assert_eq!(clean_branch_name("\"fix/login..loop\"").as_deref(), Some("fix/login.loop"));
        assert_eq!(clean_branch_name("  ").as_deref(), None);
        assert_eq!(clean_branch_name("订单").as_deref(), None);
    }

    #[test]
    fn elapsed() {
        assert_eq!(format_elapsed(5), "5s");
        assert_eq!(format_elapsed(125), "2m");
        assert_eq!(format_elapsed(7200), "2h");
        assert_eq!(format_elapsed(200_000), "2d");
    }
}
