//! 识别前给识别服务的提示：热词（文件名、代码里的名字）和最近的对话。
//! 用户说的话常夹着代码里的英文名字，单靠声音很难认对；告诉识别服务这个任务里在聊什么，准确率会高很多。

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
    let mut hotwords = Vec::new();
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
    // 最近碰过的文件和 Agent 提到的名字，新的在前。
    for event in source.events.iter().rev() {
        match &event.kind {
            EventKind::Tool { target, .. } | EventKind::Approval { target, .. } if looks_like_path(target) => {
                if let Some(name) = Path::new(target).file_name().and_then(|n| n.to_str()) {
                    push(name);
                }
            }
            EventKind::Message { speaker: Speaker::Agent, text, .. } => {
                for code in text.split('`').skip(1).step_by(2) {
                    push(code);
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
            event(EventKind::Message { speaker: Speaker::User, agent: 1, text: "export orders".into() }),
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
            events: &events,
        });
        assert_eq!(h.hotwords, ["exportCsv", "app/src/orders.ts", "old.rs", "export-csv", "shop"]);
        assert_eq!(h.context[0], "Run tests? (Yes / No)");
        assert!(h.context[1].starts_with("I added"));
        assert_eq!(h.context[2], "export orders");
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
}
