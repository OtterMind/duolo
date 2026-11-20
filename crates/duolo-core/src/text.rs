//! 文本小工具：任务标题、分支 slug、时长显示。

use std::time::{SystemTime, UNIX_EPOCH};

const TITLE_MAX_CHARS: usize = 28;
const SLUG_MAX_CHARS: usize = 24;

/// 从需求原文取第一行作为任务标题，过长时截断。
pub fn derive_title(prompt: &str) -> String {
    let line = prompt.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("新任务");
    truncate_chars(line, TITLE_MAX_CHARS)
}

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
    fn titles() {
        assert_eq!(derive_title("\n  接入 Stripe 订阅\n细节"), "接入 Stripe 订阅");
        assert_eq!(derive_title(""), "新任务");
        assert_eq!(truncate_chars("一二三四五", 3), "一二…");
    }

    #[test]
    fn slugs() {
        assert_eq!(slug("Add Stripe subscriptions!"), "add-stripe-subscriptions");
        assert_eq!(slug("接入 Stripe 订阅"), "stripe");
        assert_eq!(slug("订单导出"), "task");
    }

    #[test]
    fn elapsed() {
        assert_eq!(format_elapsed(5), "5s");
        assert_eq!(format_elapsed(125), "2m");
        assert_eq!(format_elapsed(7200), "2h");
        assert_eq!(format_elapsed(200_000), "2d");
    }
}
