//! JSONL client for an mhc `--mode rpc --multi-session` host over its stdin/stdout.
//!
//! mhc's own Rust client speaks Unix sockets only, so this is a separate, portable client.
//! One reader task routes every record: responses by request id, session records by their
//! `sessionId` routing handle, work-tool requests to their own channel. One writer task owns
//! the engine's stdin, so queuing a line (an abort, a cancel) never waits on another caller.
//! Malformed or oversized lines from the engine are counted and skipped; they never end the
//! transport or reach a caller.
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot, watch};

/// Extension RPC event a conversation session's typed work tool emits.
pub const WORK_REQUEST_EVENT: &str = "supervisor.work_request";
/// Extension request that answers one work request.
pub const WORK_REPLY_REQUEST: &str = "supervisor.work_reply";

pub type BoxedReader = Box<dyn AsyncRead + Send + Unpin>;
pub type BoxedWriter = Box<dyn AsyncWrite + Send + Unpin>;

/// Failure of one request on the transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestError {
    /// The transport ended; the reason is the engine side's.
    Gone(String),
    /// No response within the deadline. The outcome is unknown; the command is not resent.
    Timeout,
    /// The engine answered `success: false`.
    Refused {
        command: String,
        error: String,
        code: Option<String>,
    },
}

#[derive(Default)]
struct Routes {
    pending: HashMap<String, oneshot::Sender<Value>>,
    sessions: HashMap<String, mpsc::UnboundedSender<Value>>,
    /// Routing handles the engine reported closed, with the reason it gave.
    closed: HashMap<String, String>,
    work: Option<mpsc::UnboundedSender<Value>>,
}

struct Shared {
    /// `None` once the transport is gone; every waiter is released by dropping its sender.
    routes: Mutex<Option<Routes>>,
    malformed: AtomicU64,
    gone: watch::Sender<Option<String>>,
}

impl Shared {
    fn routes(&self) -> std::sync::MutexGuard<'_, Option<Routes>> {
        self.routes.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn mark_gone(&self, reason: &str) {
        let taken = self.routes().take();
        drop(taken);
        self.gone.send_if_modified(|current| {
            if current.is_some() {
                return false;
            }
            *current = Some(reason.to_owned());
            true
        });
    }

    fn malformed(&self, why: &str, bytes: usize) {
        self.malformed.fetch_add(1, Ordering::Relaxed);
        // The line itself may carry secrets; only its size is logged.
        tracing::warn!(why, bytes, "mhc supervisor engine sent a malformed line; it was skipped");
    }

    fn dispatch(&self, value: Value) {
        let Some(kind) = value.get("type").and_then(Value::as_str).map(str::to_owned) else {
            self.malformed("record without a type", value.to_string().len());
            return;
        };
        let mut guard = self.routes();
        let Some(routes) = guard.as_mut() else {
            return;
        };
        if kind == "response" {
            if let Some(sender) = value
                .get("id")
                .and_then(Value::as_str)
                .and_then(|id| routes.pending.remove(id))
            {
                let _ = sender.send(value);
            }
            return;
        }
        if kind == "extension_event"
            && value.get("name").and_then(Value::as_str) == Some(WORK_REQUEST_EVENT)
        {
            if let Some(work) = &routes.work {
                let _ = work.send(value);
            }
            return;
        }
        let Some(handle) = value.get("sessionId").and_then(Value::as_str).map(str::to_owned) else {
            tracing::debug!(kind = %kind, "mhc supervisor host record");
            return;
        };
        if kind == "session_closed" {
            let reason = value
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or("unspecified")
                .to_owned();
            routes.closed.insert(handle.clone(), reason);
        }
        if let Some(sender) = routes.sessions.get(&handle) {
            let _ = sender.send(value);
        }
    }
}

/// One stdio connection to the engine.
pub struct StdioClient {
    shared: Arc<Shared>,
    /// `None` after [`Self::close`]: the writer then shuts the engine's stdin.
    outbound: Mutex<Option<mpsc::UnboundedSender<String>>>,
    next_id: AtomicU64,
}

/// One line from the engine: a complete line, or the byte count of a line over the limit.
enum Line {
    Text(Vec<u8>),
    Oversized(usize),
}

/// Reads one `\n`-terminated line, never buffering more than `max` bytes of it.
async fn next_line<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    max: usize,
) -> std::io::Result<Option<Line>> {
    let mut line = Vec::new();
    let mut overflow = 0usize;
    loop {
        let (found, used) = {
            let available = reader.fill_buf().await?;
            if available.is_empty() {
                return Ok(match (line.is_empty(), overflow) {
                    (true, 0) => None,
                    (_, 0) => Some(Line::Text(line)),
                    (_, bytes) => Some(Line::Oversized(bytes)),
                });
            }
            let newline = available.iter().position(|byte| *byte == b'\n');
            let take = newline.unwrap_or(available.len());
            if overflow == 0 && line.len() + take <= max {
                line.extend_from_slice(&available[..take]);
            } else {
                overflow += line.len() + take;
                line.clear();
            }
            (newline.is_some(), newline.map_or(available.len(), |index| index + 1))
        };
        reader.consume(used);
        if found {
            return Ok(Some(if overflow == 0 {
                Line::Text(line)
            } else {
                Line::Oversized(overflow)
            }));
        }
    }
}

impl StdioClient {
    /// Starts the reader and writer tasks. The returned receiver yields every typed work-tool
    /// request record the engine forwards.
    pub fn start(
        reader: BoxedReader,
        writer: BoxedWriter,
        max_line_bytes: usize,
    ) -> (Arc<Self>, mpsc::UnboundedReceiver<Value>) {
        let (work_tx, work_rx) = mpsc::unbounded_channel();
        let (gone, _) = watch::channel(None);
        let shared = Arc::new(Shared {
            routes: Mutex::new(Some(Routes {
                work: Some(work_tx),
                ..Routes::default()
            })),
            malformed: AtomicU64::new(0),
            gone,
        });
        let (outbound, mut lines) = mpsc::unbounded_channel::<String>();
        let writer_shared = shared.clone();
        tokio::spawn(async move {
            let mut writer = writer;
            while let Some(line) = lines.recv().await {
                let written = match writer.write_all(line.as_bytes()).await {
                    Ok(()) => writer.flush().await,
                    Err(error) => Err(error),
                };
                if let Err(error) = written {
                    writer_shared.mark_gone(&format!("engine stdin closed: {error}"));
                    return;
                }
            }
            // Every client handle is gone: closing stdin ends the engine's stdio host.
            let _ = writer.shutdown().await;
        });
        let reader_shared = shared.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(reader);
            loop {
                match next_line(&mut reader, max_line_bytes).await {
                    Ok(None) => {
                        reader_shared.mark_gone("engine stdout ended");
                        return;
                    }
                    Err(error) => {
                        reader_shared.mark_gone(&format!("engine stdout failed: {error}"));
                        return;
                    }
                    Ok(Some(Line::Oversized(bytes))) => {
                        reader_shared.malformed("line over the size limit", bytes)
                    }
                    Ok(Some(Line::Text(bytes))) => {
                        let text = bytes.strip_suffix(b"\r").unwrap_or(bytes.as_slice());
                        if text.iter().all(u8::is_ascii_whitespace) {
                            continue;
                        }
                        match serde_json::from_slice::<Value>(text) {
                            Ok(value) if value.is_object() => reader_shared.dispatch(value),
                            Ok(_) => reader_shared.malformed("record is not an object", text.len()),
                            Err(_) => reader_shared.malformed("record is not JSON", text.len()),
                        }
                    }
                }
            }
        });
        (
            Arc::new(Self {
                shared,
                outbound: Mutex::new(Some(outbound)),
                next_id: AtomicU64::new(0),
            }),
            work_rx,
        )
    }

    pub fn gone_reason(&self) -> Option<String> {
        self.shared.gone.borrow().clone()
    }

    pub fn is_gone(&self) -> bool {
        self.shared.gone.borrow().is_some()
    }

    /// Stops sending: queued lines are still written, then the engine's stdin is shut, which
    /// ends its stdio host. The transport is gone once the engine closes its stdout.
    pub fn close(&self) {
        let taken = self
            .outbound
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        drop(taken);
    }

    /// Resolves once the transport is gone.
    pub async fn closed(&self) {
        let mut gone = self.shared.gone.subscribe();
        let _ = gone.wait_for(Option::is_some).await;
    }

    /// Malformed or oversized lines skipped so far.
    pub fn malformed_lines(&self) -> u64 {
        self.shared.malformed.load(Ordering::Relaxed)
    }

    fn gone_error(&self) -> RequestError {
        RequestError::Gone(
            self.gone_reason()
                .unwrap_or_else(|| "engine transport ended".to_owned()),
        )
    }

    /// Queues one command line without waiting for a response. serde_json escapes control
    /// characters, so a command is always exactly one line whatever text it carries.
    pub fn send(&self, command: &Value) -> Result<(), RequestError> {
        if self.is_gone() {
            return Err(self.gone_error());
        }
        let mut line = command.to_string();
        line.push('\n');
        let outbound = self.outbound.lock().unwrap_or_else(PoisonError::into_inner);
        match outbound.as_ref() {
            Some(sender) => sender.send(line).map_err(|_| self.gone_error()),
            None => Err(RequestError::Gone("the supervisor closed the engine transport".to_owned())),
        }
    }

    /// Sends one command with a fresh id and waits up to `deadline` for its response.
    /// Returns the response `data` (null when absent).
    pub async fn request(&self, command: Value, deadline: Duration) -> Result<Value, RequestError> {
        let mut command = command;
        let id = format!("fx-{}", self.next_id.fetch_add(1, Ordering::Relaxed) + 1);
        command["id"] = json!(id);
        let kind = command
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let (sender, receiver) = oneshot::channel();
        {
            let mut guard = self.shared.routes();
            let Some(routes) = guard.as_mut() else {
                return Err(self.gone_error());
            };
            routes.pending.insert(id.clone(), sender);
        }
        if let Err(error) = self.send(&command) {
            self.forget_pending(&id);
            return Err(error);
        }
        match tokio::time::timeout(deadline, receiver).await {
            Err(_) => {
                self.forget_pending(&id);
                Err(RequestError::Timeout)
            }
            Ok(Err(_)) => Err(self.gone_error()),
            Ok(Ok(response)) => {
                if response.get("success") == Some(&Value::Bool(true)) {
                    Ok(response.get("data").cloned().unwrap_or(Value::Null))
                } else {
                    Err(RequestError::Refused {
                        command: kind,
                        error: response
                            .get("error")
                            .and_then(Value::as_str)
                            .unwrap_or("refused without a message")
                            .to_owned(),
                        code: response
                            .get("errorCode")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                    })
                }
            }
        }
    }

    fn forget_pending(&self, id: &str) {
        if let Some(routes) = self.shared.routes().as_mut() {
            routes.pending.remove(id);
        }
    }

    /// Routes the records of one session handle to the returned receiver until
    /// [`Self::unsubscribe`]. Subscribe before triggering anything on the session.
    pub fn subscribe(&self, handle: &str) -> Result<mpsc::UnboundedReceiver<Value>, RequestError> {
        let (sender, receiver) = mpsc::unbounded_channel();
        let mut guard = self.shared.routes();
        let Some(routes) = guard.as_mut() else {
            return Err(self.gone_error());
        };
        routes.sessions.insert(handle.to_owned(), sender);
        Ok(receiver)
    }

    pub fn unsubscribe(&self, handle: &str) {
        if let Some(routes) = self.shared.routes().as_mut() {
            routes.sessions.remove(handle);
        }
    }

    /// The reason the engine gave when it closed `handle`, if it did. Any reason is accepted,
    /// including ones this client does not know (`host_shutdown`, `idle_evicted`, ...).
    pub fn closed_reason(&self, handle: &str) -> Option<String> {
        self.shared
            .routes()
            .as_ref()
            .and_then(|routes| routes.closed.get(handle).cloned())
    }

    pub fn forget_closed(&self, handle: &str) {
        if let Some(routes) = self.shared.routes().as_mut() {
            routes.closed.remove(handle);
        }
    }
}
