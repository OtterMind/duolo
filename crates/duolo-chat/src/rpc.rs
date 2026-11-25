//! JSON-RPC 2.0：一行一条消息，走 Agent 的标准输入输出。

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

/// 往 Agent 发消息、把回复交给等着的人。可以在多个线程里用。
#[derive(Debug, Clone)]
pub(crate) struct Rpc {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    /// 交给写线程的行；关掉后为 None（写线程随之结束，Agent 的标准输入关闭）。
    out: Mutex<Option<Sender<String>>>,
    next: AtomicU64,
    waiting: Mutex<HashMap<u64, Sender<Reply>>>,
}

impl Rpc {
    pub fn new(out: Sender<String>) -> Self {
        Self {
            inner: Arc::new(Inner { out: Mutex::new(Some(out)), next: AtomicU64::new(1), waiting: Mutex::default() }),
        }
    }

    /// 发一个请求，回复从返回的通道里取（用 [`wait`]）。
    pub fn request(&self, method: &str, params: Value) -> Receiver<Reply> {
        let id = self.inner.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        lock(&self.inner.waiting).insert(id, tx);
        if !self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})) {
            lock(&self.inner.waiting).remove(&id);
        }
        rx
    }

    pub fn notify(&self, method: &str, params: Value) {
        self.send(json!({"jsonrpc": "2.0", "method": method, "params": params}));
    }

    pub fn respond(&self, id: Value, result: Value) {
        self.send(json!({"jsonrpc": "2.0", "id": id, "result": result}));
    }

    pub fn respond_error(&self, id: Value, code: i64, message: &str) {
        self.send(json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}));
    }

    /// 读到的一行：回复交给等着的人，请求和通知返回给调用方；不是 JSON-RPC 的行（日志）忽略。
    pub fn dispatch(&self, line: &str) -> Option<Incoming> {
        let message: Value = serde_json::from_str(line.trim()).ok()?;
        let method = message["method"].as_str().map(str::to_string);
        match (method, message.get("id")) {
            (Some(method), Some(id)) if !id.is_null() => {
                Some(Incoming::Request { id: id.clone(), method, params: message["params"].clone() })
            }
            (Some(method), _) => Some(Incoming::Notification { method, params: message["params"].clone() }),
            (None, Some(id)) => {
                let waiter = id.as_u64().and_then(|id| lock(&self.inner.waiting).remove(&id))?;
                let reply = match message.get("error") {
                    Some(error) if !error.is_null() => Err(RpcError {
                        code: error["code"].as_i64().unwrap_or(0),
                        message: error["message"].as_str().unwrap_or_default().to_string(),
                    }),
                    _ => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
                };
                let _ = waiter.send(reply);
                None
            }
            (None, None) => None,
        }
    }

    /// 不再发东西（关掉 Agent 的标准输入），等回复的都不用等了。
    pub fn close(&self) {
        lock(&self.inner.out).take();
        lock(&self.inner.waiting).clear();
    }

    fn send(&self, message: Value) -> bool {
        lock(&self.inner.out).as_ref().is_some_and(|out| out.send(message.to_string()).is_ok())
    }
}

/// 等回复；`timeout` 为 None 时一直等（Agent 干活要多久没有上限）。
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
        let rpc = Rpc::new(out);
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
        let rpc = Rpc::new(out);
        let ask = r#"{"jsonrpc":"2.0","id":"p1","method":"session/request_permission","params":{"a":1}}"#;
        assert!(
            matches!(rpc.dispatch(ask), Some(Incoming::Request { method, .. }) if method == "session/request_permission")
        );
        let update = r#"{"jsonrpc":"2.0","method":"session/update","params":{}}"#;
        assert!(matches!(rpc.dispatch(update), Some(Incoming::Notification { .. })));
        assert_eq!(rpc.dispatch("Starting server..."), None, "日志行忽略");
    }

    #[test]
    fn closing_releases_waiters() {
        let (out, _lines) = mpsc::channel();
        let rpc = Rpc::new(out);
        let rx = rpc.request("session/prompt", json!({}));
        rpc.close();
        assert_eq!(wait(&rx, None), Err(Failure::Closed));
        assert_eq!(wait(&rpc.request("x", json!({})), None), Err(Failure::Closed), "关掉之后发的也不等");
    }
}
