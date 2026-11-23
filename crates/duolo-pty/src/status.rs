//! 程序自己报告的状态：OSC 7501（Program Status Protocol，
//! <https://www.superlogical.com/rex/docs/build/program-status>）。
//!
//! 程序往终端输出里写 `ESC ] 7501 ; state=working:app=claude-code ESC \`，说自己在做什么；
//! 写 `ESC ] 7501 ; ? ESC \` 问终端支不支持，终端回答之后它才开始报告（Claude Code 是这样）。
//! 比从画面上认字可靠：程序改界面不影响它。Claude Code 2.1.295、Codex 0.163 起都会报告。

/// 序列的开头。
const PREFIX: &[u8] = b"\x1b]7501;";
/// 规定整个序列最长 4096 字节。
const MAX_BODY: usize = 4096;

/// 回答程序的询问：支持哪些状态和"在等什么"。
pub(crate) const QUERY_REPLY: &[u8] =
    b"\x1b]7501;?:states=idle,working,done,blocked,error:kinds=permission,question,auth\x1b\\";

/// 程序报告的状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reported {
    Idle,
    Working,
    /// 做完了一件事，在等下一条指令。
    Done,
    /// 在等用户：确认、回答问题、登录。
    Blocked,
    /// 这件事出错了，在等下一条指令。
    Error,
}

/// 输出里的一条消息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Message {
    /// 问终端支不支持。
    Query,
    /// 程序本身（不是它下面的子任务）的新状态；`None` 是撤回之前的报告。
    Report(Option<Reported>),
}

/// 从一段段输出里找出消息。序列可能被拆在两段输出里。
#[derive(Debug, Default)]
pub(crate) struct Scanner {
    /// 上一段末尾没结束的序列（或者可能是序列开头的几个字节）。
    pending: Vec<u8>,
}

impl Scanner {
    pub(crate) fn scan(&mut self, bytes: &[u8]) -> Vec<Message> {
        if self.pending.is_empty() && !bytes.contains(&0x1b) {
            return Vec::new();
        }
        let mut data = std::mem::take(&mut self.pending);
        data.extend_from_slice(bytes);
        let mut found = Vec::new();
        let mut from = 0;
        while let Some(at) = find(&data[from..], PREFIX).map(|i| from + i) {
            let body_start = at + PREFIX.len();
            match terminator(&data[body_start..]) {
                Some(Ok((len, end))) => {
                    found.extend(parse(&data[body_start..body_start + len]));
                    from = body_start + end;
                }
                // 没按规定结束就开始了别的序列：这一条作废。
                Some(Err(end)) => from = body_start + end,
                None => {
                    // 还没结束：留到下一段，太长的不要。
                    if data.len() - at <= MAX_BODY {
                        self.pending = data[at..].to_vec();
                    }
                    return found;
                }
            }
        }
        // 末尾可能是序列开头的一部分。
        let rest = &data[from..];
        if let Some(keep) = (1..PREFIX.len()).rev().find(|&n| rest.ends_with(&PREFIX[..n])) {
            self.pending = rest[rest.len() - keep..].to_vec();
        }
        found
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// 序列在哪里结束：`Ok((内容长度, 结束符之后的位置))`，结束符是 BEL 或 `ESC \`；
/// 中间开始了别的控制序列时 `Err(那个位置)`；还没结束时 None。
fn terminator(body: &[u8]) -> Option<Result<(usize, usize), usize>> {
    let at = body.iter().position(|&b| b == 0x07 || b == 0x1b)?;
    match body[at] {
        0x07 => Some(Ok((at, at + 1))),
        _ if body.get(at + 1) == Some(&b'\\') => Some(Ok((at, at + 2))),
        // ESC 后面还没到：等下一段。
        _ if at + 1 == body.len() => None,
        _ => Some(Err(at)),
    }
}

/// `state=working:app=claude-code:kind=permission`；有 `id` 的是子任务的，不管。
fn parse(body: &[u8]) -> Option<Message> {
    let body = std::str::from_utf8(body).ok()?.trim();
    if body == "?" {
        return Some(Message::Query);
    }
    let mut state = None;
    for field in body.split(':') {
        let (key, value) = field.split_once('=')?;
        match key {
            "id" => return None,
            "state" => state = Some(value),
            _ => {}
        }
    }
    let reported = match state? {
        "idle" => Some(Reported::Idle),
        "working" => Some(Reported::Working),
        "done" => Some(Reported::Done),
        "blocked" => Some(Reported::Blocked),
        "error" => Some(Reported::Error),
        "clear" => None,
        _ => return None,
    };
    Some(Message::Report(reported))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(chunks: &[&[u8]]) -> Vec<Message> {
        let mut scanner = Scanner::default();
        chunks.iter().flat_map(|c| scanner.scan(c)).collect()
    }

    #[test]
    fn reads_reports_and_queries() {
        let out = scan(&[b"hello \x1b]7501;?\x1b\\ text \x1b]7501;state=working:app=claude-code\x07 more"]);
        assert_eq!(out, [Message::Query, Message::Report(Some(Reported::Working))]);
        assert_eq!(
            scan(&[b"\x1b]7501;state=blocked:kind=permission:app=codex\x1b\\"]),
            [Message::Report(Some(Reported::Blocked))]
        );
        assert_eq!(scan(&[b"\x1b]7501;state=clear\x1b\\"]), [Message::Report(None)]);
    }

    #[test]
    fn sequences_split_across_reads() {
        let whole = b"ab\x1b]7501;state=idle:app=claude-code\x1b\\cd";
        for cut in 1..whole.len() {
            let (a, b) = whole.split_at(cut);
            assert_eq!(scan(&[a, b]), [Message::Report(Some(Reported::Idle))], "cut at {cut}");
        }
    }

    #[test]
    fn ignores_what_is_not_ours() {
        assert!(scan(&[b"\x1b]0;\xe2\x9c\xb3 Claude Code\x07", b"\x1b]7502;state=idle\x07"]).is_empty());
        assert!(scan(&[b"\x1b]7501;id=task/1:state=working\x1b\\"]).is_empty(), "子任务的状态不是它自己的");
        assert!(scan(&[b"\x1b]7501;state=sleeping\x1b\\"]).is_empty());
        // 没结束就开始了别的序列：作废，后面的照样认。
        assert_eq!(
            scan(&[b"\x1b]7501;state=idle\x1b[0m\x1b]7501;state=done\x07"]),
            [Message::Report(Some(Reported::Done))]
        );
    }
}
