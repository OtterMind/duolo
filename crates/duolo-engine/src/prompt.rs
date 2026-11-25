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
//! 选项用数字键选，所以回答时把选项的编号发给 Agent 就行。
//!
//! 也认单选框写法（Grok）：`1 (○) Always allow`、`3 (●) Yes, proceed`，编号可以不连续，按画面上的编号按键；
//! 没有编号的选项选不了，不算。

/// 认出的提示。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub question: String,
    /// 问题上面的说明，例如要运行的命令或要改的文件。
    pub detail: String,
    pub options: Vec<String>,
    /// 选每个选项要按的键（画面上的编号）。聊天模式下的问题按编号回答，没有键。
    pub keys: Vec<String>,
}

/// 从画面文字里找最下面的一组带编号的选项：至少两个，从 1 开始；`1.` 写法要连续，单选框写法只要递增。
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
    let options: Vec<Choice> = lines[first..=last].iter().filter_map(|l| option_of(l)).collect();
    let radio = options.iter().all(|o| o.radio);
    let numbered = options.first().is_some_and(|o| o.n == 1)
        && options.windows(2).all(|w| if radio { w[1].n > w[0].n } else { w[1].n == w[0].n + 1 });
    if options.len() < 2 || !numbered {
        return None;
    }
    // 按键说明（`←/→ widen scope`、`↑/↓ Navigate`）不是问题。
    let above: Vec<&str> =
        lines[..first].iter().map(String::as_str).map(|l| if is_key_hint(l) { "" } else { l }).collect();
    let q = above.iter().rposition(|l| !l.is_empty())?;
    let question = above[q].to_string();
    // 问题上面的一段是说明：跳过紧挨着问题的空行，取到再往上的空行为止。
    let end = above[..q].iter().rposition(|l| !l.is_empty()).map_or(0, |i| i + 1);
    let start = above[..end].iter().rposition(|l| l.is_empty()).map_or(0, |i| i + 1);
    let detail = above[start..end].join("\n");
    let keys = options.iter().map(|o| o.n.to_string()).collect();
    Some(Prompt { question, detail, options: options.into_iter().map(|o| o.text).collect(), keys })
}

/// 一个带编号的选项。
struct Choice {
    n: usize,
    text: String,
    /// 单选框写法（`1 (○) …`）。
    radio: bool,
}

fn is_key_hint(line: &str) -> bool {
    line.contains("←/→") || line.contains("↑/↓")
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

/// `❯ 1. Yes` / `2. No` / `3 (●) Yes, proceed` → 编号和文字。
fn option_of(line: &str) -> Option<Choice> {
    let line = line.trim_start_matches(['❯', '>', '›', ' ']);
    let digits = line.find(|c: char| !c.is_ascii_digit())?;
    let n: usize = line[..digits].parse().ok()?;
    let rest = &line[digits..];
    let (text, radio) = match rest.strip_prefix(". ") {
        Some(text) => (text, false),
        None => {
            let rest = rest.strip_prefix(' ')?.trim_start();
            let mark = ["(○)", "(●)", "(◉)", "( )", "(x)"].iter().find(|m| rest.starts_with(**m))?;
            (&rest[mark.len()..], true)
        }
    };
    let text = text.trim();
    (n > 0 && n < 10 && !text.is_empty()).then(|| Choice { n, text: text.to_string(), radio })
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
        assert_eq!(p.keys, ["1", "2", "3"]);
    }

    #[test]
    fn radio_options_keep_their_numbers() {
        let screen = "\
  ┃  ›  Fetch HTTP status line from example.com
  ┃  │ curl -sI https://example.com | head -1
  ┃  ←/→ widen/narrow scope · e edit pattern
  ┃
  ┃  1 (○) Always allow: curl -sI
  ┃  3 (●) Yes, proceed
  ┃  4 (○) No, reject (type to add feedback)
  ┃  5 (○) Never allow: curl -sI
  ┃    (○) Yes, and don't ask again for anything (always-approve mode)
  ┃
  1/5:select  │  Tab:next option
";
        let p = parse_prompt(screen).unwrap();
        assert_eq!(p.question, "curl -sI https://example.com | head -1");
        assert_eq!(p.detail, "›  Fetch HTTP status line from example.com");
        assert_eq!(p.options[1], "Yes, proceed");
        assert_eq!(p.keys, ["1", "3", "4", "5"], "按画面上的编号，没有编号的那个选不了");
        assert!(parse_prompt("1. a\n3. b\n").is_none(), "1. 写法要连续");
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
