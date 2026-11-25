//! 一次会话的内容：各个协议把收到的消息换成这里的几种变化（一段话写了一点、在想、工具开始或结束），
//! 拼成对话（你说的、Agent 说的、工具调用），再加上正在写的那段话。

use std::path::Path;

use serde_json::Value;

use crate::{Draft, Entry};

#[derive(Debug, Default)]
pub(crate) struct Conversation {
    /// 任务目录：工具的路径在它下面时去掉这一段。
    cwd: String,
    /// 重放以前的对话时为真：这时你说的话也要记下来；平时你说的话由发送的一方记。
    pub replaying: bool,
    /// 正在拼的一段话。
    pending: Option<Pending>,
    tools: Vec<ToolCall>,
    thinking: bool,
    done: Vec<Entry>,
}

#[derive(Debug)]
struct Pending {
    user: bool,
    id: Option<String>,
    text: String,
}

/// 一次工具调用。`kind` 是 ACP 的工具种类，没有时用 `title` 的第一个词当工具名（见 [`tool_name`]）。
#[derive(Debug, Default)]
pub(crate) struct ToolCall {
    id: String,
    pub kind: String,
    pub title: String,
    /// 参数：从里面找路径、命令（见 [`target_of`]）。
    pub input: Value,
    pub location: Option<String>,
    /// `completed`、`failed` 时记下来。
    pub status: String,
    recorded: bool,
}

impl Conversation {
    pub fn new(cwd: &Path) -> Self {
        Self { cwd: cwd.display().to_string(), ..Self::default() }
    }

    /// 一段话又写了一点。`id` 不同就是另一段；没有 `id` 时接在同一个人正在说的话后面。
    pub fn text(&mut self, user: bool, id: Option<&str>, chunk: &str) {
        self.thinking = false;
        let same = self.pending.as_ref().is_some_and(|p| p.user == user && (id.is_none() || p.id.as_deref() == id));
        if !same {
            self.flush();
            self.pending = Some(Pending { user, id: id.map(str::to_string), text: String::new() });
        }
        if let Some(pending) = &mut self.pending {
            pending.text.push_str(chunk);
        }
    }

    /// Agent 的一段话写完了，`text` 是全文：正在拼的就是这段时以全文为准。
    pub fn said(&mut self, id: Option<&str>, text: &str) {
        let same = self.pending.as_ref().is_some_and(|p| !p.user && id.is_some() && p.id.as_deref() == id);
        if !same {
            self.flush();
        }
        self.pending = Some(Pending { user: false, id: id.map(str::to_string), text: text.to_string() });
        self.flush();
    }

    pub fn think(&mut self) {
        self.flush();
        self.thinking = true;
    }

    /// 工具调用开始或者有变化：`change` 只改有的字段。结束（完成、失败）时记下来。
    pub fn tool(&mut self, id: &str, change: impl FnOnce(&mut ToolCall)) {
        self.flush();
        self.thinking = false;
        let index = match self.tools.iter().position(|t| t.id == id) {
            Some(i) => i,
            None => {
                self.tools.push(ToolCall { id: id.to_string(), ..ToolCall::default() });
                self.tools.len() - 1
            }
        };
        change(&mut self.tools[index]);
        if matches!(self.tools[index].status.as_str(), "completed" | "failed") {
            self.record_tool(index);
        }
    }

    /// 这个工具用什么、对什么（命令、文件）；没见过时为 None。
    pub fn describe_tool(&self, id: &str) -> Option<(String, String)> {
        self.tools.iter().find(|t| t.id == id).map(|t| self.describe(t))
    }

    /// 一轮结束（回复了、会话重放完了）：写到一半的都算写完。
    pub fn finish_turn(&mut self) {
        self.flush();
        self.thinking = false;
        for i in 0..self.tools.len() {
            self.record_tool(i);
        }
        self.tools.clear();
    }

    /// 写完的对话，取一次就清空。
    pub fn take(&mut self) -> Vec<Entry> {
        std::mem::take(&mut self.done)
    }

    /// 正在写的话、在想、在用的工具。
    pub fn draft(&self) -> Draft {
        let text = self.pending.as_ref().filter(|p| !p.user).map(|p| p.text.clone()).unwrap_or_default();
        let tool = self.tools.iter().rev().find(|t| !t.recorded).map(|t| self.describe(t));
        Draft { text, thinking: self.thinking, tool }
    }

    /// 拼好的一段话放进写完的对话。
    fn flush(&mut self) {
        let Some(pending) = self.pending.take() else { return };
        let text = pending.text.trim();
        if text.is_empty() {
            return;
        }
        if !pending.user {
            self.done.push(Entry::Agent(text.to_string()));
        } else if self.replaying {
            self.done.push(Entry::User(text.to_string()));
        }
    }

    fn record_tool(&mut self, index: usize) {
        if self.tools[index].recorded {
            return;
        }
        self.tools[index].recorded = true;
        let (tool, target) = self.describe(&self.tools[index]);
        self.done.push(Entry::Tool { tool, target });
    }

    /// 工具名（和别的 Agent 的会话记录用同一套：Read、Edit、Bash…）和它的对象（相对任务目录的路径、命令）。
    fn describe(&self, tool: &ToolCall) -> (String, String) {
        let target = target_of(&tool.input).or_else(|| tool.location.clone()).unwrap_or_else(|| {
            if tool.kind.is_empty() {
                String::new()
            } else {
                tool.title.clone()
            }
        });
        (tool_name(&tool.kind, &tool.title), self.relative(&short(&target)))
    }

    fn relative(&self, path: &str) -> String {
        let inside = path.strip_prefix(&self.cwd).and_then(|rest| rest.strip_prefix(['/', '\\']));
        inside.filter(|rest| !rest.is_empty()).unwrap_or(path).to_string()
    }
}

/// ACP 的工具种类换成会话记录里常见的名字；`other` 和没写的用它自己的标题。
pub(crate) fn tool_name(kind: &str, title: &str) -> String {
    let name = match kind {
        "read" => "Read",
        "edit" => "Edit",
        "delete" => "Delete",
        "move" => "Move",
        "search" => "Search",
        "execute" => "Bash",
        "fetch" => "Fetch",
        "think" => "Think",
        _ => title.split_whitespace().next().unwrap_or("Tool"),
    };
    name.to_string()
}

/// 工具参数里的对象：路径、搜索的内容、网址、命令。
fn target_of(input: &Value) -> Option<String> {
    for key in ["file_path", "filePath", "path", "notebook_path", "pattern", "url", "query"] {
        if let Some(s) = input[key].as_str().filter(|s| !s.is_empty()) {
            return Some(s.to_string());
        }
    }
    match &input["command"] {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Array(parts) => Some(parts.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" ")),
        _ => None,
    }
}

/// 第一行，最多 160 个字符。
fn short(s: &str) -> String {
    let line = s.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    duolo_core::text::truncate_chars(line, 160)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn finished_text_replaces_what_was_streamed() {
        let mut c = Conversation::new(Path::new("/work/demo"));
        c.text(false, Some("m1"), "Hel");
        assert_eq!(c.draft().text, "Hel");
        c.said(Some("m1"), "Hello there");
        c.said(Some("m2"), "Second");
        assert_eq!(c.take(), vec![Entry::Agent("Hello there".into()), Entry::Agent("Second".into())]);
    }

    #[test]
    fn tools_are_recorded_once_when_done() {
        let mut c = Conversation::new(Path::new("/work/demo"));
        c.tool("t1", |t| {
            t.title = "Read".into();
            t.input = json!({"file_path": "/work/demo/src/main.rs"});
        });
        assert_eq!(c.draft().tool, Some(("Read".into(), "src/main.rs".into())));
        c.tool("t1", |t| t.status = "completed".into());
        c.tool("t1", |t| t.status = "completed".into());
        c.finish_turn();
        assert_eq!(c.take(), vec![Entry::Tool { tool: "Read".into(), target: "src/main.rs".into() }]);
    }
}
