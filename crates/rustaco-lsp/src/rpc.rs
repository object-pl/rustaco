//! JSON-RPC 2.0 over a byte stream with LSP's `Content-Length` framing, and the client
//! that matches responses to requests.

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

/// Writes one framed message.
pub fn write_message(out: &mut dyn Write, message: &Value) -> io::Result<()> {
    let body = message.to_string();
    write!(out, "Content-Length: {}\r\n\r\n{}", body.len(), body)?;
    out.flush()
}

/// Reads one framed message; None at the end of the stream.
pub fn read_message(input: &mut dyn BufRead) -> io::Result<Option<Value>> {
    let mut length = None;
    loop {
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some(v) = line.strip_prefix("Content-Length:") {
            length = v.trim().parse::<usize>().ok();
        }
    }
    let length = length.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "message without Content-Length"))?;
    let mut body = vec![0; length];
    input.read_exact(&mut body)?;
    serde_json::from_slice(&body).map(Some).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// An error answer from the server, or a failure to get one.
#[derive(Clone, Debug, PartialEq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.message, self.code)
    }
}

impl std::error::Error for RpcError {}

impl RpcError {
    fn local(message: impl Into<String>) -> Self {
        Self { code: -32803, message: message.into() }
    }
}

/// A message the server sent on its own: a notification (diagnostics, log) or a request
/// (`workspace/applyEdit`, `workspace/configuration`) that needs [`Connection::respond`].
#[derive(Clone, Debug, PartialEq)]
pub enum ServerMessage {
    Notification { method: String, params: Value },
    Request { id: Value, method: String, params: Value },
}

type Pending = Arc<Mutex<HashMap<i64, Sender<Result<Value, RpcError>>>>>;

/// One JSON-RPC connection: requests get their answers back by id, everything else the
/// server sends arrives on [`Connection::messages`].
pub struct Connection {
    writer: Mutex<Box<dyn Write + Send>>,
    next_id: AtomicI64,
    pending: Pending,
    messages: Receiver<ServerMessage>,
}

impl Connection {
    /// Starts reading `input` on a thread.
    pub fn new(input: impl Read + Send + 'static, output: impl Write + Send + 'static) -> Self {
        let pending: Pending = Arc::default();
        let (tx, messages) = channel();
        let reader_pending = pending.clone();
        std::thread::Builder::new()
            .name("rustaco-lsp-reader".into())
            .spawn(move || read_loop(BufReader::new(input), reader_pending, tx))
            .expect("spawn the reader thread");
        Self { writer: Mutex::new(Box::new(output)), next_id: AtomicI64::new(1), pending, messages }
    }

    fn send(&self, message: &Value) -> Result<(), RpcError> {
        let mut writer = self.writer.lock().map_err(|_| RpcError::local("writer poisoned"))?;
        write_message(&mut **writer, message).map_err(|e| RpcError::local(e.to_string()))
    }

    /// Sends a request; the answer arrives on the returned channel.
    pub fn request_async(&self, method: &str, params: Value) -> Receiver<Result<Value, RpcError>> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = channel();
        if let Ok(mut pending) = self.pending.lock() {
            pending.insert(id, tx.clone());
        }
        if let Err(e) = self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })) {
            let _ = tx.send(Err(e));
        }
        rx
    }

    /// Sends a request and waits up to `timeout` for its answer.
    pub fn request(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, RpcError> {
        match self.request_async(method, params).recv_timeout(timeout) {
            Ok(r) => r,
            Err(RecvTimeoutError::Timeout) => Err(RpcError::local(format!("{method}: no answer in {timeout:?}"))),
            Err(RecvTimeoutError::Disconnected) => Err(RpcError::local("the server closed the connection")),
        }
    }

    pub fn notify(&self, method: &str, params: Value) -> Result<(), RpcError> {
        self.send(&json!({ "jsonrpc": "2.0", "method": method, "params": params }))
    }

    /// Answers a request the server sent.
    pub fn respond(&self, id: Value, result: Value) -> Result<(), RpcError> {
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "result": result }))
    }

    /// Notifications and requests from the server, in arrival order.
    pub fn messages(&self) -> &Receiver<ServerMessage> {
        &self.messages
    }
}

fn read_loop(mut input: impl BufRead, pending: Pending, messages: Sender<ServerMessage>) {
    while let Ok(Some(message)) = read_message(&mut input) {
        let method = message.get("method").and_then(Value::as_str).map(str::to_owned);
        let id = message.get("id").cloned();
        match (method, id) {
            (Some(method), Some(id)) => {
                let params = message.get("params").cloned().unwrap_or(Value::Null);
                let _ = messages.send(ServerMessage::Request { id, method, params });
            }
            (Some(method), None) => {
                let params = message.get("params").cloned().unwrap_or(Value::Null);
                let _ = messages.send(ServerMessage::Notification { method, params });
            }
            (None, Some(id)) => {
                let Some(id) = id.as_i64() else { continue };
                let Some(tx) = pending.lock().ok().and_then(|mut p| p.remove(&id)) else { continue };
                let result = match message.get("error") {
                    Some(e) => Err(RpcError {
                        code: e.get("code").and_then(Value::as_i64).unwrap_or(0),
                        message: e.get("message").and_then(Value::as_str).unwrap_or("").to_owned(),
                    }),
                    None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
                };
                let _ = tx.send(result);
            }
            (None, None) => {}
        }
    }
    // The stream ended: every waiting request fails.
    if let Ok(mut pending) = pending.lock() {
        for (_, tx) in pending.drain() {
            let _ = tx.send(Err(RpcError::local("the server closed the connection")));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framing_round_trips() {
        let mut buf = Vec::new();
        write_message(&mut buf, &json!({"a": "é"})).unwrap();
        write_message(&mut buf, &json!([1, 2])).unwrap();
        let mut input = io::Cursor::new(buf);
        assert_eq!(read_message(&mut input).unwrap(), Some(json!({"a": "é"})));
        assert_eq!(read_message(&mut input).unwrap(), Some(json!([1, 2])));
        assert_eq!(read_message(&mut input).unwrap(), None);
    }
}
