//! 识别前给识别服务的提示：热词（文件名、代码里的名字）和最近的对话。
//! 用户说的话常夹着代码里的英文名字，单靠声音很难认对；告诉识别服务这个任务里在聊什么，准确率会高很多。
//! 识别完再按热词把拆开说的名字拼回去（"use state" → useState），见 [`fix_names`]。

use std::path::Path;

use duolo_core::{EventKind, Speaker, TaskEvent};

/// 最多给多少个热词（服务器还会再截）。
const MAX_HOTWORDS: usize = 40;
/// 最多给多少段对话。
const MAX_CONTEXT: usize = 6;
/// 每段对话最多多少字（取结尾：最后几句通常是在问你的）。
const MAX_CONTEXT_CHARS: usize = 300;
/// 热词最长多少字。
const MAX_WORD_CHARS: usize = 40;

/// 当前在做什么，用来生成提示。
#[derive(Debug, Default)]
pub struct HintSource<'a> {
    pub project: Option<&'a str>,
    pub task_title: Option<&'a str>,
    pub branch: Option<&'a str>,
    /// 用户在设置里加的词（产品名、库名……），排在最前面。
    pub words: &'a [String],
    /// 任务的事件，旧的在前。
    pub events: &'a [TaskEvent],
}

/// 热词（重要的在前）和对话上下文（新的在前）。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Hints {
    pub hotwords: Vec<String>,
    pub context: Vec<String>,
}

pub fn hints(source: &HintSource<'_>) -> Hints {
    // 用户加的词可以带空格（"Claude Code"），不按代码名字的规矩筛。
    let mut hotwords = clean_words(source.words);
    let mut push = |word: &str| {
        let word = word.trim().trim_matches(|c: char| matches!(c, '"' | '\'' | ',' | '.' | ':' | ';' | '(' | ')'));
        let ok = !word.is_empty()
            && word.chars().count() <= MAX_WORD_CHARS
            && !word.contains(char::is_whitespace)
            && word.chars().any(char::is_alphabetic);
        if ok && hotwords.len() < MAX_HOTWORDS && !hotwords.iter().any(|w: &String| w == word) {
            hotwords.push(word.to_string());
        }
    };
    // 最近碰过的文件、对话里提到的名字，新的在前。
    for event in source.events.iter().rev() {
        match &event.kind {
            EventKind::Tool { target, .. } | EventKind::Approval { target, .. } if looks_like_path(target) => {
                if let Some(name) = Path::new(target).file_name().and_then(|n| n.to_str()) {
                    push(name);
                }
            }
            EventKind::Message { speaker, text, .. } => {
                // Agent 用反引号标出来的最可靠，先放；再是没加反引号但一看就是代码的（双方说的都算）。
                if *speaker == Speaker::Agent {
                    for code in text.split('`').skip(1).step_by(2) {
                        push(code);
                    }
                }
                for word in code_names(text) {
                    push(word);
                }
            }
            _ => {}
        }
    }
    for word in [source.branch, source.project].into_iter().flatten() {
        push(word);
    }

    let mut context = Vec::new();
    for event in source.events.iter().rev() {
        let text = match &event.kind {
            EventKind::Prompt { question, options, .. } if options.is_empty() => question.clone(),
            EventKind::Prompt { question, options, .. } => format!("{question} ({})", options.join(" / ")),
            EventKind::Message { text, .. } => text.clone(),
            _ => continue,
        };
        let text = tail(text.trim(), MAX_CONTEXT_CHARS);
        if !text.is_empty() && !context.contains(&text) {
            context.push(text);
        }
        if context.len() >= MAX_CONTEXT {
            break;
        }
    }
    if let Some(title) = source.task_title.filter(|t| !t.trim().is_empty()) {
        context.push(title.trim().to_string());
    }
    Hints { hotwords, context }
}

/// 整理用户加的词：去掉空的、重复的、太长的，最多留 [`MAX_HOTWORDS`] 个。可以用逗号、顿号、换行分开写在一起。
pub fn clean_words(words: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for word in words.iter().flat_map(|w| w.split([',', '，', '、', ';', '；', '\n'])) {
        let word = word.trim();
        if !word.is_empty() && word.chars().count() <= MAX_WORD_CHARS && !out.iter().any(|w| w == word) {
            out.push(word.to_string());
        }
    }
    out.truncate(MAX_HOTWORDS);
    out
}

/// 文字里看着像代码的名字：`exportCsv`、`OrderService`、`snake_case`、`orders.ts`、`duolo_core::t`、`src/app`。
/// 普通的英文单词不算（识别服务本来就认得）。
fn code_names(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | ':')))
        .map(|w| w.trim_matches(|c: char| matches!(c, '-' | '.' | '/' | ':')))
        .filter(|w| is_code_name(w))
}

fn is_code_name(word: &str) -> bool {
    if word.len() < 3 || word.contains("://") || !word.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
        return false;
    }
    let bytes = word.as_bytes();
    let camel = bytes.windows(2).any(|w| w[0].is_ascii_lowercase() && w[1].is_ascii_uppercase());
    let joined = bytes.windows(3).any(|w| {
        matches!(w[1], b'_' | b'.' | b'/' | b':') && w[0].is_ascii_alphanumeric() && w[2].is_ascii_alphanumeric()
    });
    camel || joined || word.contains("::")
}

/// 按热词改正识别结果里拆开说、或大小写不对的名字："加个 use state" → "加个 useState"，"export CSV" → "exportCsv"。
/// 只改由几段拼成的名字（单个词识别服务自己会认），几段之间只能隔着空格、`_`、`-`、`.`、`/`。
pub(crate) fn fix_names(text: &str, hotwords: &[String]) -> String {
    // 名字压成小写字母数字作比较；长的先比，免得 "order" 抢了 "orderService"。
    let mut names: Vec<(String, &str, usize)> = hotwords
        .iter()
        .filter_map(|w| {
            let key: String = w.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_lowercase()).collect();
            let parts = word_parts(w);
            (parts >= 2 && key.len() >= 5).then_some((key, w.as_str(), parts))
        })
        .collect();
    if names.is_empty() {
        return text.to_string();
    }
    names.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
    // 文字里的英文、数字段（字节位置）。
    let mut runs = Vec::new();
    let mut start = None;
    for (i, c) in text.char_indices().chain([(text.len(), ' ')]) {
        match (c.is_ascii_alphanumeric(), start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                runs.push((s, i));
                start = None;
            }
            _ => {}
        }
    }
    let gap_ok =
        |from: usize, to: usize| text[from..to].chars().all(|c| c == ' ' || matches!(c, '_' | '-' | '.' | '/'));
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut i = 0;
    'runs: while i < runs.len() {
        // 前一个字符是 `_` 说明这段是别的名字的一部分。
        if runs[i].0 > 0 && text[..runs[i].0].ends_with('_') {
            i += 1;
            continue;
        }
        for (key, name, parts) in &names {
            let mut joined = String::new();
            let first = i;
            for j in first..runs.len().min(first + parts + 1) {
                if j > first && !gap_ok(runs[j - 1].1, runs[j].0) {
                    break;
                }
                joined.extend(text[runs[j].0..runs[j].1].chars().map(|c| c.to_ascii_lowercase()));
                if joined.len() > key.len() || !key.starts_with(joined.as_str()) {
                    break;
                }
                let (from, to) = (runs[first].0, runs[j].1);
                if joined == *key && !text[to..].starts_with('_') {
                    if &text[from..to] != *name {
                        out.push_str(&text[copied..from]);
                        out.push_str(name);
                        copied = to;
                    }
                    i = j + 1;
                    continue 'runs;
                }
            }
        }
        i += 1;
    }
    out.push_str(&text[copied..]);
    out
}

/// 名字由几段组成：`useState` 2、`export_csv_v2` 3、`orders.ts` 2。
fn word_parts(word: &str) -> usize {
    let mut parts = 0;
    let mut prev: Option<char> = None;
    for c in word.chars() {
        let starts = match prev {
            _ if !c.is_ascii_alphanumeric() => false,
            None => true,
            Some(p) if !p.is_ascii_alphanumeric() => true,
            Some(p) => p.is_ascii_lowercase() && c.is_ascii_uppercase(),
        };
        if starts {
            parts += 1;
        }
        prev = Some(c);
    }
    parts
}

/// 工具的目标是文件路径（不是一条命令）。
fn looks_like_path(target: &str) -> bool {
    !target.is_empty()
        && !target.contains(char::is_whitespace)
        && (target.contains('/') || target.contains('\\') || target.contains('.'))
}

/// 最后 `max` 个字。
fn tail(text: &str, max: usize) -> String {
    let count = text.chars().count();
    text.chars().skip(count.saturating_sub(max)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use duolo_core::TaskId;

    fn event(kind: EventKind) -> TaskEvent {
        TaskEvent { task: TaskId(1), seq: 0, at: 0, kind }
    }

    #[test]
    fn files_and_code_names_newest_first() {
        let events = [
            event(EventKind::Tool { agent: 1, tool: "Read".into(), target: "src/old.rs".into() }),
            event(EventKind::Tool { agent: 1, tool: "Bash".into(), target: "cargo test --workspace".into() }),
            event(EventKind::Message { speaker: Speaker::User, agent: 1, text: "export orders from OrderList".into() }),
            event(EventKind::Message {
                speaker: Speaker::Agent,
                agent: 1,
                text: "I added `exportCsv` to `app/src/orders.ts`. Should I also handle `a b`?".into(),
            }),
            event(EventKind::Prompt {
                agent: 1,
                question: "Run tests?".into(),
                detail: String::new(),
                options: vec!["Yes".into(), "No".into()],
            }),
        ];
        let h = hints(&HintSource {
            project: Some("shop"),
            task_title: Some("Export orders"),
            branch: Some("export-csv"),
            words: &["Tauri".to_string()],
            events: &events,
        });
        assert_eq!(
            h.hotwords,
            ["Tauri", "exportCsv", "app/src/orders.ts", "OrderList", "old.rs", "export-csv", "shop"]
        );
        assert_eq!(h.context[0], "Run tests? (Yes / No)");
        assert!(h.context[1].starts_with("I added"));
        assert_eq!(h.context[2], "export orders from OrderList");
        assert_eq!(h.context.last().unwrap(), "Export orders");
    }

    #[test]
    fn long_messages_keep_the_end() {
        let long = format!("{}Which one?", "x".repeat(1000));
        let events = [event(EventKind::Message { speaker: Speaker::Agent, agent: 1, text: long })];
        let h = hints(&HintSource { events: &events, ..HintSource::default() });
        assert_eq!(h.context[0].chars().count(), MAX_CONTEXT_CHARS);
        assert!(h.context[0].ends_with("Which one?"));
    }

    #[test]
    fn code_names_in_plain_text() {
        let names: Vec<_> =
            code_names("Fix exportCsv in orders.ts (see duolo_core::t, https://a.com/x). Then run it.").collect();
        assert_eq!(names, ["exportCsv", "orders.ts", "duolo_core::t"]);
    }

    #[test]
    fn joins_names_said_in_pieces() {
        let words = ["useState".to_string(), "exportCsv".to_string(), "orders.ts".to_string(), "Tauri".to_string()];
        assert_eq!(fix_names("加一个 use state 的 hook", &words), "加一个 useState 的 hook");
        assert_eq!(fix_names("把 Export CSV 改到 orders ts 里", &words), "把 exportCsv 改到 orders.ts 里");
        assert_eq!(fix_names("usestate and use_state_x", &words), "useState and use_state_x");
        // 只说了一段、已经对了、单个词的，不动。
        assert_eq!(fix_names("use it, useState, tauri", &words), "use it, useState, tauri");
    }
}
