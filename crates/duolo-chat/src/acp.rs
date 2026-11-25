//! ACP（Agent Client Protocol，<https://agentclientprotocol.com>）的会话内容：把 `session/update` 一条条拼成
//! 对话（你说的、Agent 说的、工具调用），再加上正在写的那段话。只认用得到的字段，不认识的更新忽略：
//! Agent 升级加了新的更新类型也不会出错。

use std::path::Path;

use serde_json::Value;

use crate::{Draft, Entry};

/// 一次会话的内容。
#[derive(Debug, Default)]
pub(crate) struct Conversation {
    /// 任务目录：工具的路径在它下面时去掉这一段。
    cwd: String,
    /// 重放以前的对话（`session/load`）时为真：这时你说的话也要记下来；平时你说的话由发送的一方记。
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

#[derive(Debug, Default)]
struct ToolCall {
    id: String,
    kind: String,
    title: String,
    input: Value,
    location: Option<String>,
    status: String,
    recorded: bool,
}

impl Conversation {
    pub fn new(cwd: &Path) -> Self {
        Self { cwd: cwd.display().to_string(), ..Self::default() }
    }

    /// `session/update` 的 `update`。
    pub fn update(&mut self, update: &Value) {
        match update["sessionUpdate"].as_str().unwrap_or_default() {
            "agent_message_chunk" => self.chunk(false, update),
            "user_message_chunk" => self.chunk(true, update),
            "agent_thought_chunk" => {
                self.flush();
                self.thinking = true;
            }
            "tool_call" | "tool_call_update" => {
                self.flush();
                self.thinking = false;
                self.tool(update);
            }
            _ => {}
        }
    }

    /// 一轮结束（`session/prompt` 回复了、会话重放完了）：写到一半的都算写完。
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

    /// 请求授权的那个工具：用什么、对什么（命令、文件）。
    pub fn permission(&mut self, tool_call: &Value) -> (String, String) {
        self.flush();
        self.tool(tool_call);
        let id = tool_call["toolCallId"].as_str().unwrap_or_default();
        match self.tools.iter().find(|t| t.id == id) {
            Some(tool) => self.describe(tool),
            None => (tool_name("", tool_call["title"].as_str().unwrap_or_default()), String::new()),
        }
    }

    fn chunk(&mut self, user: bool, update: &Value) {
        let content = &update["content"];
        if content["type"].as_str() != Some("text") {
            return;
        }
        let text = content["text"].as_str().unwrap_or_default();
        let id = update["messageId"].as_str().map(str::to_string);
        self.thinking = false;
        let same = self.pending.as_ref().is_some_and(|p| p.user == user && (p.id == id || id.is_none()));
        if !same {
            self.flush();
            self.pending = Some(Pending { user, id, text: String::new() });
        }
        if let Some(pending) = &mut self.pending {
            pending.text.push_str(text);
        }
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

    /// `tool_call` 或 `tool_call_update`：只有出现的字段才换掉原来的。结束（完成、失败）时记下来。
    fn tool(&mut self, update: &Value) {
        let Some(id) = update["toolCallId"].as_str() else { return };
        let index = match self.tools.iter().position(|t| t.id == id) {
            Some(i) => i,
            None => {
                self.tools.push(ToolCall { id: id.to_string(), ..ToolCall::default() });
                self.tools.len() - 1
            }
        };
        let tool = &mut self.tools[index];
        let text = |key: &str| update[key].as_str().filter(|s| !s.is_empty()).map(str::to_string);
        if let Some(kind) = text("kind") {
            tool.kind = kind;
        }
        if let Some(title) = text("title") {
            tool.title = title;
        }
        if let Some(status) = text("status") {
            tool.status = status;
        }
        if update["rawInput"].as_object().is_some_and(|input| !input.is_empty()) {
            tool.input = update["rawInput"].clone();
        }
        if let Some(path) = update["locations"].get(0).and_then(|l| l["path"].as_str()) {
            tool.location = Some(path.to_string());
        }
        if matches!(tool.status.as_str(), "completed" | "failed") {
            self.record_tool(index);
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
fn tool_name(kind: &str, title: &str) -> String {
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

    /// 录下来的真实会话里 Agent 发来的 `session/update`。
    fn updates(fixture: &str) -> Vec<Value> {
        fixture
            .lines()
            .filter_map(|l| l.strip_prefix("<< "))
            .map(|l| serde_json::from_str::<Value>(l).unwrap())
            .filter(|m| m["method"] == "session/update")
            .map(|m| m["params"]["update"].clone())
            .collect()
    }

    const OPENCODE: &str = include_str!("../tests/fixtures/opencode-1.19.6.txt");

    #[test]
    fn real_session_becomes_a_conversation() {
        let all = updates(OPENCODE);
        // 前一半是这一轮，后一半是 session/load 重放的同一段对话。
        let replay_from = all.iter().position(|u| u["sessionUpdate"] == "user_message_chunk").unwrap();
        let mut live = Conversation::new(Path::new("/work/demo"));
        let mut drafts = Vec::new();
        for update in &all[..replay_from] {
            live.update(update);
            drafts.push(live.draft());
        }
        live.finish_turn();
        let turn = live.take();
        assert_eq!(
            turn,
            vec![
                Entry::Tool { tool: "Edit".into(), target: "hello.txt".into() },
                Entry::Tool { tool: "Bash".into(), target: "ls".into() },
                Entry::Agent("`ls` lists `hello.txt` and `README.md`.".into()),
            ]
        );
        assert!(drafts.iter().any(|d| d.thinking), "先在想");
        assert!(drafts.iter().any(|d| d.tool == Some(("Bash".into(), "ls".into()))), "在跑命令");
        assert!(drafts.iter().any(|d| d.text == "`ls` lists `"), "话是一点点写出来的");

        let mut replay = Conversation::new(Path::new("/work/demo"));
        replay.replaying = true;
        for update in &all[replay_from..] {
            replay.update(update);
        }
        replay.finish_turn();
        let history = replay.take();
        assert_eq!(
            history[0],
            Entry::User(
                "Create a file hello.txt containing 'hi', then run `ls` and tell me the result in one sentence.".into()
            )
        );
        assert_eq!(history[1..], turn[..], "重放出来的和当时记下的一样，才能去重");
    }

    #[test]
    fn user_words_are_not_recorded_twice() {
        let mut c = Conversation::default();
        c.update(&json!({"sessionUpdate": "user_message_chunk", "content": {"type": "text", "text": "hi"}}));
        c.update(&json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "Hello"}}));
        c.update(&json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": " there"}}));
        c.update(&json!({"sessionUpdate": "plan", "entries": []}));
        c.update(&json!({"sessionUpdate": "something_new"}));
        c.finish_turn();
        assert_eq!(c.take(), vec![Entry::Agent("Hello there".into())], "发出去的话由发送的一方记");
    }

    #[test]
    fn permission_names_the_command() {
        let mut c = Conversation::new(Path::new("/work/demo"));
        let call = json!({"toolCallId": "t1", "title": "pnpm test", "kind": "execute", "rawInput": {"command": "pnpm test\n--watch"}});
        assert_eq!(c.permission(&call), ("Bash".into(), "pnpm test".into()));
        let unknown = json!({"toolCallId": "t2", "title": "mcp lookup"});
        assert_eq!(c.permission(&unknown), ("mcp".into(), String::new()));
    }
}
