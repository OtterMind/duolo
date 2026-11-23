//! 从 Agent 的终端画面里认出"等你选"的提示：问题、上下文（例如要运行的命令）和带编号的选项。
//!
//! Claude Code 的确认框大致是：
//!
//! ```text
//! │ Bash command                         │
//! │   pnpm test --filter orders          │
//! │ Do you want to proceed?              │
//! │ ❯ 1. Yes                             │
//! │   2. Yes, and don't ask again …      │
//! │   3. No, and tell Claude what to do  │
//! ```
//!
//! 选项用数字键选，所以回答时把选项序号发给 Agent 就行。

/// 认出的提示。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub question: String,
    /// 问题上面的说明，例如要运行的命令或要改的文件。
    pub detail: String,
    pub options: Vec<String>,
}

/// 从画面文字里找最下面的一组带编号的选项（至少两个，从 1 开始连续）。
pub fn parse_prompt(screen: &str) -> Option<Prompt> {
    let lines: Vec<String> = screen.lines().map(clean).collect();
    let last = lines.iter().rposition(|l| option_of(l).is_some())?;
    // 往上找连续的选项，中间允许折行的续行。
    let mut first = last;
    let mut i = last;
    while i > 0 {
        i -= 1;
        if option_of(&lines[i]).is_some() {
            first = i;
        } else if lines[i].is_empty() || i + 1 < first {
            break;
        }
    }
    let options: Vec<(usize, String)> = lines[first..=last].iter().filter_map(|l| option_of(l)).collect();
    let numbered_from_one = options.iter().enumerate().all(|(i, (n, _))| *n == i + 1);
    if options.len() < 2 || !numbered_from_one {
        return None;
    }
    let above: Vec<&str> = lines[..first].iter().map(String::as_str).collect();
    let q = above.iter().rposition(|l| !l.is_empty())?;
    let question = above[q].to_string();
    // 问题上面的一段是说明：跳过紧挨着问题的空行，取到再往上的空行为止。
    let end = above[..q].iter().rposition(|l| !l.is_empty()).map_or(0, |i| i + 1);
    let start = above[..end].iter().rposition(|l| l.is_empty()).map_or(0, |i| i + 1);
    let detail = above[start..end].join("\n");
    Some(Prompt { question, detail, options: options.into_iter().map(|(_, text)| text).collect() })
}

/// 画面上有一个正等着用户选的列表（光标停在某个选项上），例如第一次启动时选主题、确认信任目录。
/// 和 [`parse_prompt`] 不同，对话里普通的编号列表不算。`markers` 是这个 Agent 自己的写法（小写）。
pub fn is_choosing(screen: &str, markers: &[String]) -> bool {
    screen.lines().map(clean).any(|line| {
        let lower = line.to_lowercase();
        (line.starts_with(['❯', '›']) && option_of(&line).is_some()) || markers.iter().any(|m| lower.contains(m))
    })
}

/// 去掉框线和首尾空白。
fn clean(line: &str) -> String {
    line.trim_matches(|c: char| c.is_whitespace() || "│┃╭╮╰╯─━┌┐└┘|".contains(c)).to_string()
}

/// `❯ 1. Yes` / `2. No` → (编号, 文字)。
fn option_of(line: &str) -> Option<(usize, String)> {
    let line = line.trim_start_matches(['❯', '>', '›', ' ']);
    let dot = line.find(". ")?;
    let n: usize = line[..dot].parse().ok()?;
    let text = line[dot + 2..].trim();
    (n > 0 && n < 10 && !text.is_empty()).then(|| (n, text.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_permission_box() {
        let screen = "\
⏺ I'll run the tests.

╭──────────────────────────────────────────╮
│ Bash command                             │
│                                          │
│   pnpm test --filter orders              │
│   Run order tests                        │
│                                          │
│ Do you want to proceed?                  │
│ ❯ 1. Yes                                 │
│   2. Yes, and don't ask again for pnpm   │
│   3. No, and tell Claude what to do      │
│      differently (esc)                   │
╰──────────────────────────────────────────╯
";
        let p = parse_prompt(screen).unwrap();
        assert_eq!(p.question, "Do you want to proceed?");
        assert_eq!(p.detail, "pnpm test --filter orders\nRun order tests");
        assert_eq!(p.options, ["Yes", "Yes, and don't ask again for pnpm", "No, and tell Claude what to do"]);
    }

    #[test]
    fn choosing_is_not_a_plain_list() {
        let markers = ["enter to confirm".to_string()];
        assert!(is_choosing("Theme\n❯ 1. Dark\n  2. Light", &[]));
        assert!(is_choosing("❯ No, exit\n  Yes\n Enter to confirm", &markers));
        assert!(!is_choosing("❯ No, exit\n  Yes\n Enter to confirm", &[]));
        assert!(!is_choosing("Steps:\n1. Read\n2. Write\n> ", &markers));
    }

    #[test]
    fn plain_list_is_not_a_prompt() {
        assert!(parse_prompt("Steps:\n1. Read the code\n").is_none());
        assert!(parse_prompt("2. second\n3. third\n").is_none(), "编号要从 1 开始");
        assert!(parse_prompt("nothing here").is_none());
    }
}
