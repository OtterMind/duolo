//! 请求、回复、通知：一行一条 JSON，走 Agent 的标准输入输出。两种写法见 [`Framing`]。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde_json::{json, Value};

/// 对方回的错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RpcError {
    pub code: i64,
    pub message: String,
}

/// 等回复的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Failure {
    /// 进程没了（或者已经要结束了）。
    Closed,
    TimedOut,
    Rejected(RpcError),
}

type Reply = Result<Value, RpcError>;

/// 对方发来的、不是回复的消息。
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Incoming {
    Request { id: Value, method: String, params: Value },
    Notification { method: String, params: Value },
}

/// 消息怎么写。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Framing {
    /// JSON-RPC 2.0（ACP、Codex 的 app-server）。
    JsonRpc,
    /// Claude 的 stream-json：请求是 `control_request`（`request.subtype` 是方法），回复是 `control_response`；
    /// 别的消息按 `type` 当成通知。
    Control,
}

/// 往 Agent 发消息、把回复交给等着的人。可以在多个线程里用。
#[derive(Debug, Clone)]
pub(crate) struct Rpc {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    framing: Framing,
    /// 交给写线程的行；关掉后为 None（写线程随之结束，Agent 的标准输入关闭）。
    out: Mutex<Option<Sender<String>>>,
    next: AtomicU64,
    waiting: Mutex<HashMap<u64, Sender<Reply>>>,
}

impl Rpc {
    pub fn new(out: Sender<String>, framing: Framing) -> Self {
        Self {
            inner: Arc::new(Inner {
                framing,
                out: Mutex::new(Some(out)),
                next: AtomicU64::new(1),
                waiting: Mutex::default(),
            }),
        }
    }

    /// 发一个请求，回复从返回的通道里取（用 [`wait`]）。
    pub fn request(&self, method: &str, params: Value) -> Receiver<Reply> {
        let id = self.inner.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        lock(&self.inner.waiting).insert(id, tx);
        let message = match self.inner.framing {
            Framing::JsonRpc => json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
            Framing::Control => {
                let mut request = if params.is_object() { params } else { json!({}) };
                request["subtype"] = json!(method);
                json!({"type": "control_request", "request_id": format!("{CONTROL_ID}{id}"), "request": request})
            }
        };
        if !self.send(message) {
            lock(&self.inner.waiting).remove(&id);
        }
        rx
    }

    /// 发通知（只用于 JSON-RPC）；`params` 为 null 时不带。
    pub fn notify(&self, method: &str, params: Value) {
        let mut message = json!({"jsonrpc": "2.0", "method": method});
        if !params.is_null() {
            message["params"] = params;
        }
        self.send(message);
    }

    pub fn respond(&self, id: Value, result: Value) {
        self.send(match self.inner.framing {
            Framing::JsonRpc => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Framing::Control => json!({
                "type": "control_response",
                "response": {"subtype": "success", "request_id": id, "response": result},
            }),
        });
    }

    pub fn respond_error(&self, id: Value, code: i64, message: &str) {
        self.send(match self.inner.framing {
            Framing::JsonRpc => json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}),
            Framing::Control => json!({
                "type": "control_response",
                "response": {"subtype": "error", "request_id": id, "error": message},
            }),
        });
    }

    /// 读到的一行：回复交给等着的人，请求和通知返回给调用方；不是 JSON 的行（日志）忽略。
    pub fn dispatch(&self, line: &str) -> Option<Incoming> {
        let message: Value = serde_json::from_str(line.trim()).ok()?;
        match self.inner.framing {
            Framing::JsonRpc => self.dispatch_json_rpc(message),
            Framing::Control => self.dispatch_control(message),
        }
    }

    fn dispatch_json_rpc(&self, message: Value) -> Option<Incoming> {
        let method = message["method"].as_str().map(str::to_string);
        match (method, message.get("id")) {
            (Some(method), Some(id)) if !id.is_null() => {
                Some(Incoming::Request { id: id.clone(), method, params: message["params"].clone() })
            }
            (Some(method), _) => Some(Incoming::Notification { method, params: message["params"].clone() }),
            (None, Some(id)) => {
                let reply = match message.get("error") {
                    Some(error) if !error.is_null() => Err(RpcError {
                        code: error["code"].as_i64().unwrap_or(0),
                        message: error["message"].as_str().unwrap_or_default().to_string(),
                    }),
                    _ => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
                };
                self.reply(id.as_u64()?, reply);
                None
            }
            (None, None) => None,
        }
    }

    fn dispatch_control(&self, mut message: Value) -> Option<Incoming> {
        match message["type"].as_str()? {
            "control_request" => Some(Incoming::Request {
                id: message["request_id"].clone(),
                method: message["request"]["subtype"].as_str().unwrap_or_default().to_string(),
                params: message["request"].take(),
            }),
            "control_response" => {
                let response = &message["response"];
                let id = response["request_id"].as_str()?.strip_prefix(CONTROL_ID)?.parse().ok()?;
                let reply = match response["subtype"].as_str() {
                    Some("error") => {
                        Err(RpcError { code: 0, message: response["error"].as_str().unwrap_or_default().to_string() })
                    }
                    _ => Ok(response["response"].clone()),
                };
                self.reply(id, reply);
                None
            }
            kind => Some(Incoming::Notification { method: kind.to_string(), params: message }),
        }
    }

    fn reply(&self, id: u64, reply: Reply) {
        if let Some(waiter) = lock(&self.inner.waiting).remove(&id) {
            let _ = waiter.send(reply);
        }
    }

    /// 不再发东西（关掉 Agent 的标准输入），等回复的都不用等了。
    pub fn close(&self) {
        lock(&self.inner.out).take();
        lock(&self.inner.waiting).clear();
    }

    /// 原样发一条消息（不是请求、回复、通知的，例如 Claude 的 `user` 消息）。
    pub fn send(&self, message: Value) -> bool {
        lock(&self.inner.out).as_ref().is_some_and(|out| out.send(message.to_string()).is_ok())
    }
}

/// 等回复；`timeout` 为 None 时一直等（Agent 干活要多久没有上限）。
/// 我们发的 control 请求的编号前缀（Agent 自己的请求编号是别的样子）。
const CONTROL_ID: &str = "duolo-";

pub(crate) fn wait(rx: &Receiver<Reply>, timeout: Option<Duration>) -> Result<Value, Failure> {
    let reply = match timeout {
        Some(timeout) => rx.recv_timeout(timeout).map_err(|err| match err {
            RecvTimeoutError::Timeout => Failure::TimedOut,
            RecvTimeoutError::Disconnected => Failure::Closed,
        })?,
        None => rx.recv().map_err(|_| Failure::Closed)?,
    };
    reply.map_err(Failure::Rejected)
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replies_reach_the_caller() {
        let (out, lines) = mpsc::channel();
        let rpc = Rpc::new(out, Framing::JsonRpc);
        let rx = rpc.request("initialize", json!({}));
        let sent: Value = serde_json::from_str(&lines.recv().unwrap()).unwrap();
        assert_eq!((sent["id"].as_u64(), sent["method"].as_str()), (Some(1), Some("initialize")));
        assert_eq!(rpc.dispatch(r#"{"jsonrpc":"2.0","id":1,"result":{"ok":true}}"#), None);
        assert_eq!(wait(&rx, Some(Duration::from_secs(1))).unwrap()["ok"], true);

        let rx = rpc.request("session/new", json!({}));
        rpc.dispatch(r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32000,"message":"Authentication required"}}"#);
        let err = wait(&rx, None).unwrap_err();
        assert_eq!(err, Failure::Rejected(RpcError { code: -32000, message: "Authentication required".into() }));
    }

    #[test]
    fn requests_and_notifications_are_returned() {
        let (out, _lines) = mpsc::channel();
        let rpc = Rpc::new(out, Framing::JsonRpc);
        let ask = r#"{"jsonrpc":"2.0","id":"p1","method":"session/request_permission","params":{"a":1}}"#;
        assert!(
            matches!(rpc.dispatch(ask), Some(Incoming::Request { method, .. }) if method == "session/request_permission")
        );
        let update = r#"{"jsonrpc":"2.0","method":"session/update","params":{}}"#;
        assert!(matches!(rpc.dispatch(update), Some(Incoming::Notification { .. })));
        assert_eq!(rpc.dispatch("Starting server..."), None, "日志行忽略");
    }

    #[test]
    fn control_messages_are_requests_and_replies() {
        let (out, lines) = mpsc::channel();
        let rpc = Rpc::new(out, Framing::Control);
        let rx = rpc.request("initialize", json!({}));
        let sent: Value = serde_json::from_str(&lines.recv().unwrap()).unwrap();
        assert_eq!(
            sent,
            json!({"type": "control_request", "request_id": "duolo-1", "request": {"subtype": "initialize"}})
        );
        let reply = r#"{"type":"control_response","response":{"subtype":"success","request_id":"duolo-1","response":{"ok":1}}}"#;
        assert_eq!(rpc.dispatch(reply), None);
        assert_eq!(wait(&rx, Some(Duration::from_secs(1))).unwrap()["ok"], 1);

        let ask =
            r#"{"type":"control_request","request_id":"abc","request":{"subtype":"can_use_tool","tool_name":"Bash"}}"#;
        let Some(Incoming::Request { id, method, params }) = rpc.dispatch(ask) else { panic!() };
        assert_eq!((method.as_str(), &params["tool_name"]), ("can_use_tool", &json!("Bash")));
        rpc.respond(id, json!({"behavior": "allow"}));
        let sent: Value = serde_json::from_str(&lines.recv().unwrap()).unwrap();
        assert_eq!(
            sent["response"],
            json!({"subtype": "success", "request_id": "abc", "response": {"behavior": "allow"}})
        );

        let said = r#"{"type":"assistant","message":{}}"#;
        assert!(matches!(rpc.dispatch(said), Some(Incoming::Notification { method, .. }) if method == "assistant"));
    }

    #[test]
    fn closing_releases_waiters() {
        let (out, _lines) = mpsc::channel();
        let rpc = Rpc::new(out, Framing::JsonRpc);
        let rx = rpc.request("session/prompt", json!({}));
        rpc.close();
        assert_eq!(wait(&rx, None), Err(Failure::Closed));
        assert_eq!(wait(&rpc.request("x", json!({})), None), Err(Failure::Closed), "关掉之后发的也不等");
    }
}
