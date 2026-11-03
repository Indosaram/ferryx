use std::{
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{Mutex, Semaphore},
    task::JoinSet,
};

use crate::agent_input::{
    convert_agent_action_to_events, encode_nv12_screenshot, AgentAction, FrameMetadata,
    InputStateTracker, ScreenInfo, ScreenshotFormat,
};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const WORK_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_CONNECTIONS: usize = 32;
const MAX_WORK: u32 = 4;

struct ConnectionWork {
    input_closed: AtomicBool,
    disconnect: Mutex<()>,
    screenshots: Arc<Semaphore>,
    jobs: Arc<Semaphore>,
}

impl ConnectionWork {
    fn new() -> Self {
        Self {
            input_closed: AtomicBool::new(false),
            disconnect: Mutex::new(()),
            screenshots: Arc::new(Semaphore::new(1)),
            jobs: Arc::new(Semaphore::new(MAX_WORK as usize)),
        }
    }

    async fn blocking<T: Send + 'static>(
        &self,
        job: impl FnOnce() -> T + Send + 'static,
    ) -> std::io::Result<T> {
        tokio::time::timeout(WORK_TIMEOUT, async {
            let permit = self
                .jobs
                .clone()
                .acquire_owned()
                .await
                .map_err(std::io::Error::other)?;
            tokio::task::spawn_blocking(move || {
                // Cancellation of the HTTP waiter cannot release a running job's permit.
                let _permit = permit;
                job()
            })
            .await
            .map_err(std::io::Error::other)
        })
        .await
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::TimedOut))?
    }
}

#[derive(Debug, Clone)]
pub struct FrameSnapshot {
    pub width: u32,
    pub height: u32,
    pub buffer: Arc<Vec<u8>>,
    pub metadata: Option<FrameMetadata>,
}

pub trait AgentServerBackend: Send + Sync + 'static {
    fn send_input_event(&self, event: ferryx_rd_proto::InputEvent) -> Result<(), String>;
    fn get_screen_info(&self) -> ScreenInfo;
    fn get_latest_frame_nv12(&self) -> Option<(u32, u32, Arc<Vec<u8>>)>;
    fn get_latest_frame_metadata(&self) -> Option<FrameMetadata> {
        None
    }
    fn get_latest_frame_snapshot(&self) -> Option<FrameSnapshot> {
        let (width, height, buffer) = self.get_latest_frame_nv12()?;
        let metadata = self.get_latest_frame_metadata();
        Some(FrameSnapshot {
            width,
            height,
            buffer,
            metadata,
        })
    }

    /// Whether the backend owns a remote session it can gracefully stop via
    /// [`AgentServerBackend::try_disconnect_session`]. Generic screenshot/input
    /// backends default to false and must never fake a disconnect.
    fn supports_session_disconnect(&self) -> bool {
        false
    }

    /// Gracefully stop the backend-owned remote session through its existing
    /// lifecycle. Called at most once per server, only after the disconnecting
    /// response write was attempted and the HTTP listener closed, and only when
    /// [`Self::supports_session_disconnect`] returns true.
    /// Synchronous calls must return; the server can time out their waiters but
    /// cannot interrupt a blocked backend method.
    fn try_disconnect_session(&self) -> Result<(), String> {
        Err("session disconnect unsupported by this backend".to_string())
    }
}

pub struct AgentServer {
    listener: Option<TcpListener>,
    addr: SocketAddr,
    backend: Arc<dyn AgentServerBackend>,
    tracker: Arc<Mutex<InputStateTracker>>,
    current_pos: Arc<Mutex<(f32, f32)>>,
    done_tx: Arc<tokio::sync::watch::Sender<bool>>,
    auth_token: Option<String>,
    #[cfg(test)]
    accepted: Option<tokio::sync::mpsc::Sender<()>>,
}

impl AgentServer {
    pub fn new(addr: SocketAddr, backend: Arc<dyn AgentServerBackend>) -> Self {
        Self {
            listener: None,
            addr,
            backend,
            tracker: Arc::new(Mutex::new(InputStateTracker::default())),
            current_pos: Arc::new(Mutex::new((0.5, 0.5))),
            done_tx: Arc::new(tokio::sync::watch::channel(false).0),
            auth_token: None,
            #[cfg(test)]
            accepted: None,
        }
    }

    pub fn with_auth_token(mut self, token: impl Into<String>) -> Self {
        self.auth_token = Some(token.into());
        self
    }

    pub fn set_auth_token(&mut self, token: impl Into<String>) {
        self.auth_token = Some(token.into());
    }

    pub async fn bind(
        addr: SocketAddr,
        backend: Arc<dyn AgentServerBackend>,
    ) -> std::io::Result<(Self, SocketAddr)> {
        let listener = TcpListener::bind(addr).await?;
        let local_addr = listener.local_addr()?;
        Ok((
            Self {
                listener: Some(listener),
                addr: local_addr,
                backend,
                tracker: Arc::new(Mutex::new(InputStateTracker::default())),
                current_pos: Arc::new(Mutex::new((0.5, 0.5))),
                done_tx: Arc::new(tokio::sync::watch::channel(false).0),
                auth_token: None,
                #[cfg(test)]
                accepted: None,
            },
            local_addr,
        ))
    }

    pub async fn run(
        mut self,
        mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
    ) -> std::io::Result<()> {
        let listener = match self.listener.take() {
            Some(l) => l,
            None => TcpListener::bind(self.addr).await?,
        };
        let backend = self.backend;
        let tracker = self.tracker;
        let current_pos = self.current_pos;
        let done_tx = self.done_tx;
        let mut done_rx = done_tx.subscribe();
        let mut session_disconnect_requested = false;
        let work = Arc::new(ConnectionWork::new());
        let mut connections = JoinSet::new();

        // Spawn watchdog timer task (500ms interval)
        let watchdog_tracker = tracker.clone();
        let watchdog_backend = backend.clone();
        let watchdog_pos = current_pos.clone();
        connections.spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(500));
            loop {
                interval.tick().await;
                let pos = *watchdog_pos.lock().await;
                if let Ok(mut t) =
                    tokio::time::timeout(Duration::from_millis(100), watchdog_tracker.lock()).await
                {
                    if t.is_timed_out() {
                        // Detection is non-destructive: the error-aware release keeps the held
                        // state recorded when transmission fails so the next tick retries.
                        let (_sent, error) = release_inputs(&*watchdog_backend, &mut t, pos);
                        if let Some(error) = error {
                            tracing::warn!(%error, "watchdog input release failed; retrying");
                            t.mark_release_failed();
                        }
                    }
                }
            }
        });

        loop {
            if *done_rx.borrow() {
                session_disconnect_requested = true;
                break;
            }
            if *shutdown_rx.borrow() || shutdown_rx.has_changed().is_err() {
                break;
            }
            tokio::select! {
                biased;
                _ = done_rx.changed() => {},
                _ = shutdown_rx.changed() => {},
                joined = connections.join_next(), if !connections.is_empty() => {
                    log_connection_result(joined);
                }
                accept_res = listener.accept() => {
                    match accept_res {
                        Ok((stream, _peer_addr)) => {
                            if connections.len() < MAX_CONNECTIONS {
                                connections.spawn(handle_connection(stream, backend.clone(),
                                    tracker.clone(), current_pos.clone(), done_tx.clone(), work.clone(), self.auth_token.clone()));
                            } else {
                                // No rejection tasks or queue: silent peers cannot grow work.
                                drop(stream);
                            }
                            #[cfg(test)]
                            if let Some(accepted) = &self.accepted {
                                accepted.try_send(()).unwrap();
                            }
                        }
                        Err(err) => {
                            tracing::warn!("Agent server accept error: {err}");
                        }
                    }
                }
            }
        }

        work.input_closed.store(true, Ordering::SeqCst);
        drop(listener);
        connections.abort_all();
        while let Some(joined) = connections.join_next().await {
            log_connection_result(Some(joined));
        }
        // Blocking operations cannot be forcibly cancelled. Drain their permits
        // before backend stop, or return an error rather than claiming a join.
        let drained =
            tokio::time::timeout(WORK_TIMEOUT, work.jobs.clone().acquire_many_owned(MAX_WORK))
                .await
                .map_err(|_| std::io::Error::from(std::io::ErrorKind::TimedOut))?
                .map_err(std::io::Error::other)?;
        drop(drained);
        if session_disconnect_requested {
            work.blocking(move || backend.try_disconnect_session())
                .await?
                .map_err(std::io::Error::other)?;
        } else {
            // Every shutdown path must release held input, not just the disconnect branch.
            let mut t = tracker.lock().await;
            if !t.is_empty() {
                let pos = *current_pos.lock().await;
                let (_sent, error) = release_inputs(&*backend, &mut t, pos);
                if let Some(error) = error {
                    tracing::warn!(%error, "shutdown input release failed");
                }
            }
        }
        Ok(())
    }
}

fn log_connection_result(joined: Option<Result<std::io::Result<()>, tokio::task::JoinError>>) {
    match joined {
        Some(Ok(Err(error))) => tracing::warn!(%error, "Agent HTTP connection failed"),
        Some(Err(error)) if !error.is_cancelled() => {
            tracing::error!(%error, "Agent HTTP task failed")
        }
        Some(Ok(Ok(()))) | Some(Err(_)) | None => {}
    }
}

fn check_auth_header(auth_token: Option<&str>, headers_text: &str) -> bool {
    if let Some(expected) = auth_token {
        for line in headers_text.lines().skip(1) {
            let (name, value) = match line.split_once(':') {
                Some((n, v)) => (n, v.trim()),
                None => continue,
            };
            if name.eq_ignore_ascii_case("Authorization") {
                if let Some(token) = value.strip_prefix("Bearer ") {
                    if token == expected {
                        return true;
                    }
                }
            }
            if name.eq_ignore_ascii_case("X-Maho-Token") && value == expected {
                return true;
            }
        }
        false
    } else {
        true
    }
}

async fn read_request<R: tokio::io::AsyncRead + Unpin>(stream: &mut R) -> std::io::Result<Vec<u8>> {
    let deadline = tokio::time::Instant::now() + REQUEST_TIMEOUT;
    let mut buf = vec![0u8; 65536];
    let mut used = 0;
    let mut total = None;
    loop {
        if let Some(total) = total {
            if used >= total {
                buf.truncate(total);
                return Ok(buf);
            }
        }
        if used == buf.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "request too large",
            ));
        }
        let end = total.unwrap_or(buf.len());
        let n = tokio::time::timeout_at(deadline, stream.read(&mut buf[used..end]))
            .await
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::TimedOut))??;
        if n == 0 {
            if used == 0 {
                return Ok(Vec::new());
            }
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "incomplete request",
            ));
        }
        used += n;
        if total.is_none() {
            let delimiter = buf[..used]
                .windows(4)
                .position(|w| w == b"\r\n\r\n")
                .map(|i| (i, 4))
                .or_else(|| {
                    buf[..used]
                        .windows(2)
                        .position(|w| w == b"\n\n")
                        .map(|i| (i, 2))
                });
            if let Some((header_end, delimiter_len)) = delimiter {
                let headers = std::str::from_utf8(&buf[..header_end])
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
                let mut content_length = None;
                for line in headers.lines().skip(1) {
                    let (name, value) = line.split_once(':').ok_or_else(|| {
                        std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid header")
                    })?;
                    if name.eq_ignore_ascii_case("transfer-encoding") {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "unsupported transfer encoding",
                        ));
                    }
                    if name.eq_ignore_ascii_case("content-length") {
                        let value = value.trim();
                        if content_length.is_some()
                            || value.is_empty()
                            || !value.bytes().all(|b| b.is_ascii_digit())
                        {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                "invalid content length",
                            ));
                        }
                        content_length = Some(value.parse::<usize>().map_err(|e| {
                            std::io::Error::new(std::io::ErrorKind::InvalidInput, e)
                        })?);
                    }
                }
                let body_start = header_end + delimiter_len;
                let length = content_length.unwrap_or(0);
                if length > buf.len() - body_start {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "request too large",
                    ));
                }
                total = Some(body_start + length);
            }
        }
    }
}

async fn handle_connection(
    mut stream: TcpStream,
    backend: Arc<dyn AgentServerBackend>,
    tracker: Arc<Mutex<InputStateTracker>>,
    current_pos: Arc<Mutex<(f32, f32)>>,
    done_tx: Arc<tokio::sync::watch::Sender<bool>>,
    work: Arc<ConnectionWork>,
    auth_token: Option<String>,
) -> std::io::Result<()> {
    let buf = match read_request(&mut stream).await {
        Ok(buf) => buf,
        Err(err) => {
            let (status, text) = match err.kind() {
                std::io::ErrorKind::InvalidInput => (413, "Payload Too Large"),
                std::io::ErrorKind::TimedOut => (408, "Request Timeout"),
                std::io::ErrorKind::InvalidData | std::io::ErrorKind::UnexpectedEof => {
                    (400, "Bad Request")
                }
                _ => return Err(err),
            };
            let json =
                serde_json::to_vec(&serde_json::json!({"ok": false, "error": err.to_string()}))?;
            return send_response(&mut stream, status, text, "application/json", &json).await;
        }
    };
    let n = buf.len();
    if n == 0 {
        return Ok(());
    }

    let request_str = String::from_utf8_lossy(&buf[..n]);
    let mut lines = request_str.lines();
    let request_line = match lines.next() {
        Some(line) => line,
        None => return Ok(()),
    };

    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        return send_response(
            &mut stream,
            400,
            "Bad Request",
            "text/plain",
            b"Malformed request",
        )
        .await;
    }

    let method = parts[0];
    let path = parts[1];

    let header_end = request_str
        .find("\r\n\r\n")
        .or_else(|| request_str.find("\n\n"));

    let headers_text = match header_end {
        Some(idx) => &request_str[..idx],
        None => &request_str,
    };
    let body = match header_end {
        Some(idx) => {
            let offset = if request_str[idx..].starts_with("\r\n\r\n") {
                idx + 4
            } else {
                idx + 2
            };
            &buf[offset..n]
        }
        None => &[],
    };

    macro_rules! require_auth {
        () => {
            if !check_auth_header(auth_token.as_deref(), headers_text) {
                return send_response(
                    &mut stream,
                    401,
                    "Unauthorized",
                    "application/json",
                    b"{\"ok\":false,\"error\":\"unauthorized\"}",
                )
                .await;
            }
        };
    }

    match (method, path) {
        ("GET", "/api/v1/health") | ("GET", "/health") => {
            send_response(
                &mut stream,
                200,
                "OK",
                "application/json",
                b"{\"status\":\"ok\"}",
            )
            .await
        }
        ("GET", "/api/v1/screen/info") => {
            require_auth!();
            let info = work.blocking(move || backend.get_screen_info()).await?;
            let json = serde_json::to_vec(&info)?;
            send_response(&mut stream, 200, "OK", "application/json", &json).await
        }
        ("GET", path) if path.starts_with("/api/v1/screen/screenshot") => {
            require_auth!();
            let permit = match work.screenshots.clone().try_acquire_owned() {
                Ok(permit) => permit,
                Err(_) => {
                    return send_response(
                        &mut stream,
                        429,
                        "Too Many Requests",
                        "application/json",
                        br#"{"ok":false,"error":"screenshot busy"}"#,
                    )
                    .await
                }
            };
            let format = if path.contains("format=jpeg") {
                ScreenshotFormat::Jpeg
            } else {
                ScreenshotFormat::Png
            };

            let (status, text, resp) =
                work.blocking(move || {
                    let _permit = permit;
                    match backend.get_latest_frame_snapshot() {
                    Some(snapshot) => {
                        match encode_nv12_screenshot(snapshot.width, snapshot.height, &snapshot.buffer, format) {
                            Ok(base64_data) => {
                                let mut json_resp = serde_json::json!({
                                    "ok": true, "format": format, "width": snapshot.width,
                                    "height": snapshot.height, "base64": base64_data,
                                });
                                if let Some(meta) = snapshot.metadata {
                                    json_resp["frame_id"] = serde_json::json!(meta.frame_id);
                                    json_resp["timestamp_ms"] = serde_json::json!(meta.timestamp_ms);
                                    json_resp["age_ms"] = serde_json::json!(meta.age_ms);
                                }
                                (200, "OK", json_resp)
                            },
                            Err(err) => (
                                500,
                                "Internal Error",
                                serde_json::json!({"ok": false, "error": err}),
                            ),
                        }
                    }
                    None => (
                        404,
                        "Not Found",
                        serde_json::json!({"ok": false, "error": "no active frame received yet"}),
                    ),
                    }
                }).await?;
            let json = serde_json::to_vec(&resp)?;
            send_response(&mut stream, status, text, "application/json", &json).await
        }
        ("GET", path) if path.starts_with("/api/v1/screen/wait_change") => {
            require_auth!();
            // Parse query parameters
            let query = path.split('?').nth(1).unwrap_or("");
            let mut last_frame_id: Option<u64> = None;
            let mut timeout_ms: u64 = 5000;

            for param in query.split('&') {
                if let Some((key, value)) = param.split_once('=') {
                    match key {
                        "last_frame_id" => {
                            if let Ok(id) = value.parse::<u64>() {
                                last_frame_id = Some(id);
                            }
                        }
                        "timeout_ms" => {
                            if let Ok(ms) = value.parse::<u64>() {
                                timeout_ms = ms.min(30000);
                            }
                        }
                        _ => {}
                    }
                }
            }

            let start = tokio::time::Instant::now();
            let poll_interval = Duration::from_millis(25);
            let timeout_dur = Duration::from_millis(timeout_ms);
            let resp = loop {
                if let Some(meta) = backend.get_latest_frame_metadata() {
                    // Check if frame has changed
                    if last_frame_id.is_none() || meta.frame_id != last_frame_id.unwrap() {
                        break serde_json::json!({
                            "ok": true,
                            "changed": true,
                            "frame_id": meta.frame_id,
                            "timestamp_ms": meta.timestamp_ms,
                            "age_ms": meta.age_ms,
                        });
                    }
                }

                if start.elapsed() >= timeout_dur {
                    break serde_json::json!({
                        "ok": true,
                        "changed": false,
                        "frame_id": last_frame_id.unwrap_or(0),
                    });
                }

                tokio::time::sleep(poll_interval).await;
            };
            let json = serde_json::to_vec(&resp)?;
            send_response(&mut stream, 200, "OK", "application/json", &json).await
        }
        ("POST", "/api/v1/input/action") | ("POST", "/api/v1/input/batch") => {
            require_auth!();
            if work.input_closed.load(Ordering::SeqCst) {
                return reject_input(&mut stream).await;
            }
            let actions = if path == "/api/v1/input/action" {
                serde_json::from_slice::<AgentAction>(body).map(|action| vec![action])
            } else {
                serde_json::from_slice::<Vec<AgentAction>>(body)
            };
            let actions = match actions {
                Ok(actions) => actions,
                Err(err) => {
                    let kind = if path == "/api/v1/input/action" {
                        "payload"
                    } else {
                        "batch"
                    };
                    let resp = serde_json::json!({"ok": false, "error": format!("invalid JSON {kind}: {err}")});
                    let json = serde_json::to_vec(&resp)?;
                    return send_response(
                        &mut stream,
                        400,
                        "Bad Request",
                        "application/json",
                        &json,
                    )
                    .await;
                }
            };

            let mut t = tokio::time::timeout(WORK_TIMEOUT, tracker.lock_owned())
                .await
                .map_err(|_| std::io::Error::from(std::io::ErrorKind::TimedOut))?;
            let mut cp = current_pos.lock_owned().await;
            if work.input_closed.load(Ordering::SeqCst) {
                return reject_input(&mut stream).await;
            }
            // Move both guards into the job: conversion and all sends are one ordered transaction,
            // even if the connection task is cancelled while the blocking dispatch is running.
            let (status, text, resp) = work
                .blocking(move || {
                    use crate::agent_input::MouseButton;
                    use ferryx_rd_proto::InputEventType;
                    let screen_info = backend.get_screen_info();
                    let mut total_events = 0;
                    // Downs already accepted by the backend, so a later failure can be rolled back.
                    let mut held_buttons: std::collections::HashSet<MouseButton> =
                        std::collections::HashSet::new();
                    let mut held_keys: std::collections::HashMap<u16, ferryx_rd_proto::Modifiers> =
                        std::collections::HashMap::new();
                    for action in &actions {
                        let events = match convert_agent_action_to_events(
                            action,
                            &mut t,
                            &mut cp,
                            screen_info.width as f32,
                            screen_info.height as f32,
                        ) {
                            Ok(events) => events,
                            Err(err) => {
                                return (
                                    400,
                                    "Bad Request",
                                    serde_json::json!({"ok": false, "error": err.to_string()}),
                                )
                            }
                        };
                        for event in events {
                            if let Err(err) = backend.send_input_event(event) {
                                // Re-record what the backend actually holds, then release it all
                                // (matching ups plus a trailing Reset) before answering.
                                for button in held_buttons {
                                    t.record_button_down(button);
                                }
                                for (key_code, modifiers) in held_keys {
                                    t.record_key_down(key_code, modifiers);
                                }
                                let (_released, cleanup) = release_inputs(&*backend, &mut t, *cp);
                                let error = match cleanup {
                                    Some(cleanup) => format!("{err}; cleanup: {cleanup}"),
                                    None => err,
                                };
                                return (
                                    500,
                                    "Internal Error",
                                    serde_json::json!({"ok": false, "error": error}),
                                );
                            }
                            match event.event_type {
                                InputEventType::LeftMouseDown => {
                                    held_buttons.insert(MouseButton::Left);
                                }
                                InputEventType::RightMouseDown => {
                                    held_buttons.insert(MouseButton::Right);
                                }
                                InputEventType::MiddleMouseDown => {
                                    held_buttons.insert(MouseButton::Middle);
                                }
                                InputEventType::LeftMouseUp => {
                                    held_buttons.remove(&MouseButton::Left);
                                }
                                InputEventType::RightMouseUp => {
                                    held_buttons.remove(&MouseButton::Right);
                                }
                                InputEventType::MiddleMouseUp => {
                                    held_buttons.remove(&MouseButton::Middle);
                                }
                                InputEventType::KeyDown => {
                                    held_keys.insert(event.key_code, event.modifiers);
                                }
                                InputEventType::KeyUp => {
                                    held_keys.remove(&event.key_code);
                                }
                                InputEventType::Reset => {
                                    held_buttons.clear();
                                    held_keys.clear();
                                }
                                _ => {}
                            }
                            total_events += 1;
                        }
                    }
                    (
                        200,
                        "OK",
                        serde_json::json!({"ok": true, "events_sent": total_events}),
                    )
                })
                .await?;
            let json = serde_json::to_vec(&resp)?;
            send_response(&mut stream, status, text, "application/json", &json).await
        }
        ("POST", "/api/v1/input/reset") => {
            require_auth!();
            if work.input_closed.load(Ordering::SeqCst) {
                return reject_input(&mut stream).await;
            }
            let mut t = tokio::time::timeout(WORK_TIMEOUT, tracker.lock_owned())
                .await
                .map_err(|_| std::io::Error::from(std::io::ErrorKind::TimedOut))?;
            let cp = current_pos.lock_owned().await;
            if work.input_closed.load(Ordering::SeqCst) {
                return reject_input(&mut stream).await;
            }
            let (count, error) = work
                .blocking(move || release_inputs(&*backend, &mut t, *cp))
                .await?;
            let (status, text) = if error.is_some() {
                (500, "Internal Error")
            } else {
                (200, "OK")
            };
            let mut resp = serde_json::json!({ "ok": error.is_none(), "reset_events_sent": count });
            if let Some(error) = error {
                resp["error"] = error.into();
            }
            let json = serde_json::to_vec(&resp)?;
            send_response(&mut stream, status, text, "application/json", &json).await
        }
        ("POST", "/api/v1/session/disconnect") => {
            require_auth!();
            handle_session_disconnect(&mut stream, backend, tracker, current_pos, done_tx, work)
                .await
        }
        _ => {
            send_response(
                &mut stream,
                404,
                "Not Found",
                "application/json",
                b"{\"error\":\"not found\"}",
            )
            .await
        }
    }
}

async fn reject_input(stream: &mut TcpStream) -> std::io::Result<()> {
    send_response(
        stream,
        409,
        "Conflict",
        "application/json",
        br#"{"ok":false,"error":"session is disconnecting"}"#,
    )
    .await
}

fn release_inputs(
    backend: &dyn AgentServerBackend,
    tracker: &mut InputStateTracker,
    pos: (f32, f32),
) -> (usize, Option<String>) {
    use crate::agent_input::MouseButton;
    use ferryx_rd_proto::InputEventType;
    let events = tracker.release_all(pos.0, pos.1);
    let mut sent = 0;
    let mut error = None;
    let mut reset_sent = false;
    // Attempt every release including Reset even if an earlier send failed.
    for event in &events {
        match backend.send_input_event(*event) {
            Ok(()) => {
                sent += 1;
                reset_sent |= event.event_type == InputEventType::Reset;
            }
            Err(err) => {
                error.get_or_insert(err);
            }
        }
    }
    if !reset_sent {
        // Keep a conservative record for retry when the remote state is unknown.
        for event in events {
            match event.event_type {
                InputEventType::LeftMouseUp => tracker.record_button_down(MouseButton::Left),
                InputEventType::RightMouseUp => tracker.record_button_down(MouseButton::Right),
                InputEventType::MiddleMouseUp => tracker.record_button_down(MouseButton::Middle),
                InputEventType::KeyUp => tracker.record_key_down(event.key_code, event.modifiers),
                _ => {}
            }
        }
    }
    (sent, error)
}

async fn handle_session_disconnect(
    stream: &mut TcpStream,
    backend: Arc<dyn AgentServerBackend>,
    tracker: Arc<Mutex<InputStateTracker>>,
    current_pos: Arc<Mutex<(f32, f32)>>,
    done_tx: Arc<tokio::sync::watch::Sender<bool>>,
    work: Arc<ConnectionWork>,
) -> std::io::Result<()> {
    if !backend.supports_session_disconnect() {
        let json = serde_json::to_vec(&serde_json::json!({
            "ok": false, "disconnected": false,
            "error": "session disconnect unsupported by this backend",
        }))?;
        return send_response(stream, 501, "Not Implemented", "application/json", &json).await;
    }
    let Ok(_disconnect) = work.disconnect.try_lock() else {
        return reject_input(stream).await;
    };
    if *done_tx.borrow() {
        return reject_input(stream).await;
    }
    // Gate before waiting for the input transaction. Already-running dispatch
    // finishes first; queued requests recheck the gate after acquiring tracker.
    // A failed release leaves input closed but permits an explicit disconnect retry.
    work.input_closed.store(true, Ordering::SeqCst);
    let mut t = tokio::time::timeout(WORK_TIMEOUT, tracker.lock_owned())
        .await
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::TimedOut))?;
    let cp = current_pos.lock_owned().await;
    let (released_inputs, error) = work
        .blocking(move || release_inputs(&*backend, &mut t, *cp))
        .await?;
    let mut resp = serde_json::json!({
        "ok": error.is_none(), "disconnected": error.is_none(), "released_inputs": released_inputs,
    });
    if let Some(error) = error {
        resp["error"] = error.into();
        send_response(
            stream,
            500,
            "Internal Error",
            "application/json",
            &serde_json::to_vec(&resp)?,
        )
        .await?;
        done_tx.send_replace(true);
        return Ok(());
    }
    // Teardown follows the bounded response attempt even if the client vanished.
    // No new input can appear between release, response, and backend stop.
    let response = send_response(
        stream,
        200,
        "OK",
        "application/json",
        &serde_json::to_vec(&resp)?,
    )
    .await;
    done_tx.send_replace(true);
    response
}

async fn send_response(
    stream: &mut TcpStream,
    status_code: u16,
    status_text: &str,
    content_type: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let response_header = format!(
        "HTTP/1.1 {status_code} {status_text}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    tokio::time::timeout(REQUEST_TIMEOUT, async {
        stream.write_all(response_header.as_bytes()).await?;
        stream.write_all(body).await?;
        stream.flush().await
    })
    .await
    .map_err(|_| std::io::Error::from(std::io::ErrorKind::TimedOut))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn reader_enforces_size_and_framing_boundaries() {
        let header = "POST / HTTP/1.1\r\nContent-Length: 65494\r\n\r\n";
        let length = 65536 - header.len();
        let mut exact = format!("POST / HTTP/1.1\r\nContent-Length: {length}\r\n\r\n").into_bytes();
        exact.resize(65536, b'x');
        assert_eq!(
            read_request(&mut exact.as_slice()).await.unwrap().len(),
            65536
        );
        for (request, kind) in [
            (
                b"POST / HTTP/1.1\r\nContent-Length: 65536\r\n\r\n".to_vec(),
                std::io::ErrorKind::InvalidInput,
            ),
            (vec![b'x'; 65536], std::io::ErrorKind::InvalidInput),
            (
                b"POST / HTTP/1.1\r\nContent-Length: 2\r\n\r\nx".to_vec(),
                std::io::ErrorKind::UnexpectedEof,
            ),
            (
                b"POST / HTTP/1.1\r\nContent-Length: -1\r\n\r\n".to_vec(),
                std::io::ErrorKind::InvalidData,
            ),
            (
                b"POST / HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 1\r\n\r\nx".to_vec(),
                std::io::ErrorKind::InvalidData,
            ),
            (
                b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec(),
                std::io::ErrorKind::InvalidData,
            ),
        ] {
            assert_eq!(
                read_request(&mut request.as_slice())
                    .await
                    .unwrap_err()
                    .kind(),
                kind
            );
        }
    }

    #[tokio::test]
    async fn oversized_http_request_returns_413() {
        let backend = Arc::new(MockBackend {
            sent_count: AtomicUsize::new(0),
        });
        let (server, addr) = AgentServer::bind("127.0.0.1:0".parse().unwrap(), backend.clone())
            .await
            .unwrap();
        let (shutdown, rx) = tokio::sync::watch::channel(false);
        let task = tokio::spawn(server.run(rx));
        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(b"POST /api/v1/input/action HTTP/1.1\r\nContent-Length: 65536\r\n\r\n")
            .await
            .unwrap();
        let mut response = Vec::new();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            stream.read_to_end(&mut response),
        )
        .await;
        shutdown.send(true).unwrap();
        task.await.unwrap().unwrap();
        result.unwrap().unwrap();
        assert!(response.starts_with(b"HTTP/1.1 413 "));
        assert_eq!(backend.sent_count.load(Ordering::SeqCst), 0);
    }

    struct FragmentedRequest {
        chunks: std::collections::VecDeque<Vec<u8>>,
    }

    impl tokio::io::AsyncRead for FragmentedRequest {
        fn poll_read(
            mut self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            if let Some(mut chunk) = self.chunks.pop_front() {
                let n = chunk.len().min(buf.remaining());
                buf.put_slice(&chunk[..n]);
                if n < chunk.len() {
                    self.chunks.push_front(chunk.split_off(n));
                }
            }
            std::task::Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn reader_accumulates_fragmented_headers_and_body() {
        let body = br#"{"action":"mouse_down","button":"left"}"#;
        let header = format!(
            "POST /api/v1/input/action HTTP/1.1\r\nContent-Length: {}\r\n\r",
            body.len()
        );
        let fragments = [header.as_bytes(), b"\n", &body[..20], &body[20..]];
        let expected = fragments.concat();
        let mut reader = FragmentedRequest {
            chunks: fragments.into_iter().map(<[u8]>::to_vec).collect(),
        };
        let request = read_request(&mut reader).await.unwrap();
        assert_eq!(request, expected);
        assert!(reader.chunks.is_empty());
    }

    struct GatedBackend {
        screenshot: bool,
        events: std::sync::Mutex<Vec<ferryx_rd_proto::InputEvent>>,
        entered: std::sync::Mutex<Option<std::sync::mpsc::Sender<()>>>,
        release: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
    }

    impl GatedBackend {
        fn block_once(&self) {
            if let Some(entered) = self.entered.lock().unwrap().take() {
                entered.send(()).unwrap();
                self.release.lock().unwrap().recv().unwrap();
            }
        }
    }

    impl AgentServerBackend for GatedBackend {
        fn send_input_event(&self, event: ferryx_rd_proto::InputEvent) -> Result<(), String> {
            if !self.screenshot {
                self.block_once();
            }
            self.events.lock().unwrap().push(event);
            Ok(())
        }

        fn get_screen_info(&self) -> ScreenInfo {
            MockBackend {
                sent_count: AtomicUsize::new(0),
            }
            .get_screen_info()
        }

        fn get_latest_frame_nv12(&self) -> Option<(u32, u32, Arc<Vec<u8>>)> {
            if self.screenshot {
                self.block_once();
            }
            Some((2, 2, Arc::new(vec![128; 6])))
        }
    }

    struct HttpFixture {
        addr: SocketAddr,
        tracker: Arc<Mutex<InputStateTracker>>,
        release: Option<std::sync::mpsc::Sender<()>>,
        shutdown: tokio::sync::watch::Sender<bool>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl HttpFixture {
        fn start(
            backend: Arc<dyn AgentServerBackend>,
            release: std::sync::mpsc::Sender<()>,
        ) -> Self {
            let (ready_tx, ready_rx) = std::sync::mpsc::channel();
            let (shutdown, shutdown_rx) = tokio::sync::watch::channel(false);
            let thread = std::thread::spawn(move || {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap()
                    .block_on(async move {
                        let (server, addr) =
                            AgentServer::bind("127.0.0.1:0".parse().unwrap(), backend)
                                .await
                                .unwrap();
                        ready_tx.send((addr, server.tracker.clone())).unwrap();
                        server.run(shutdown_rx).await.unwrap();
                    });
            });
            let (addr, tracker) = ready_rx
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
            Self {
                addr,
                tracker,
                release: Some(release),
                shutdown,
                thread: Some(thread),
            }
        }

        fn release(&mut self) {
            if let Some(release) = self.release.take() {
                release.send(()).unwrap();
            }
        }
    }

    impl Drop for HttpFixture {
        fn drop(&mut self) {
            self.release();
            let _ = self.shutdown.send(true);
            if let Some(thread) = self.thread.take() {
                thread.join().unwrap();
            }
            eprintln!("HTTP fixture cleanup: gate released, shutdown sent, runtime thread joined");
        }
    }

    fn socket_request(addr: SocketAddr, request: &str) -> std::net::TcpStream {
        use std::io::Write;
        let mut stream = std::net::TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(3)))
            .unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        stream
    }

    fn read_json(stream: &mut std::net::TcpStream) -> std::io::Result<serde_json::Value> {
        use std::io::Read;
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes)?;
        let start = bytes.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
        Ok(serde_json::from_slice(&bytes[start..]).unwrap())
    }

    fn health_during_blocked_backend(screenshot: bool, path: &str, body: &str) {
        // Given a real socket server on its own current-thread runtime and a gated sync backend.
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let backend = Arc::new(GatedBackend {
            events: std::sync::Mutex::new(Vec::new()),
            screenshot,
            entered: std::sync::Mutex::new(Some(entered_tx)),
            release: std::sync::Mutex::new(release_rx),
        });
        let mut fixture = HttpFixture::start(backend, release_tx);
        let method = if screenshot { "GET" } else { "POST" };
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let mut blocked = socket_request(fixture.addr, &request);
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        // When the backend cannot complete until the test explicitly releases it.
        let mut health = socket_request(
            fixture.addr,
            "GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n",
        );
        let response_before_release = read_json(&mut health);
        // Cleanup happens before the RED assertion, including draining the blocked request.
        fixture.release();
        let completed = read_json(&mut blocked).unwrap();
        assert_eq!(completed["ok"], true);
        drop(fixture);
        // Then health completes while the synchronous backend is still blocked.
        assert_eq!(
            response_before_release.expect("health must respond before backend release")["status"],
            "ok"
        );
    }

    #[test]
    fn health_responds_when_screenshot_backend_is_blocked() {
        health_during_blocked_backend(true, "/api/v1/screen/screenshot?format=png", "");
    }

    #[test]
    fn health_responds_when_input_action_backend_is_blocked() {
        health_during_blocked_backend(
            false,
            "/api/v1/input/action",
            r#"{"action":"mouse_down","button":"left"}"#,
        );
    }

    #[test]
    fn health_responds_when_input_batch_backend_is_blocked() {
        health_during_blocked_backend(
            false,
            "/api/v1/input/batch",
            r#"[{"action":"mouse_down","button":"left"},{"action":"mouse_up","button":"left"}]"#,
        );
    }

    #[test]
    fn health_responds_when_input_reset_backend_is_blocked() {
        health_during_blocked_backend(false, "/api/v1/input/reset", "");
    }

    #[test]
    fn concurrent_action_and_reset_cannot_interleave_a_blocked_batch() {
        use ferryx_rd_proto::InputEventType::{
            LeftMouseDown, LeftMouseUp, Reset, RightMouseDown, RightMouseUp,
        };
        // Given a batch stopped inside its first synchronous send.
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let backend = Arc::new(GatedBackend {
            screenshot: false,
            events: std::sync::Mutex::new(Vec::new()),
            entered: std::sync::Mutex::new(Some(entered_tx)),
            release: std::sync::Mutex::new(release_rx),
        });
        let mut fixture = HttpFixture::start(backend.clone(), release_tx);
        let post = |path: &str, body: &str| {
            format!("POST /api/v1/input/{path} HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{body}", body.len())
        };
        let mut batch = socket_request(
            fixture.addr,
            &post(
                "batch",
                r#"[{"action":"mouse_down","button":"left"},{"action":"mouse_up","button":"left"}]"#,
            ),
        );
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        // The shared tracker must remain exclusively owned through dispatch, not just conversion.
        let transaction_held = fixture.tracker.try_lock().is_err();
        // When action and reset arrive concurrently with that blocked batch.
        let mut action = socket_request(
            fixture.addr,
            &post("action", r#"{"action":"mouse_down","button":"right"}"#),
        );
        let mut reset = socket_request(fixture.addr, &post("reset", ""));
        fixture.release();
        assert_eq!(read_json(&mut batch).unwrap()["events_sent"], 2);
        assert_eq!(read_json(&mut action).unwrap()["events_sent"], 1);
        let reset_response = read_json(&mut reset).unwrap();
        // Then the batch is contiguous and reset reflects whichever complete transaction preceded it.
        assert!(transaction_held);
        let events: Vec<_> = backend
            .events
            .lock()
            .unwrap()
            .iter()
            .map(|e| e.event_type)
            .collect();
        assert_eq!(&events[..2], &[LeftMouseDown, LeftMouseUp]);
        match events[2..] {
            [RightMouseDown, RightMouseUp, Reset] => {
                assert_eq!(reset_response["reset_events_sent"], 2);
                assert!(fixture.tracker.try_lock().unwrap().is_empty());
            }
            [Reset, RightMouseDown] => {
                assert_eq!(reset_response["reset_events_sent"], 1);
                assert!(!fixture.tracker.try_lock().unwrap().is_empty());
            }
            ref other => panic!("interleaved input transactions: {other:?}"),
        }
    }

    struct MockBackend {
        sent_count: AtomicUsize,
    }

    impl AgentServerBackend for MockBackend {
        fn send_input_event(&self, _event: ferryx_rd_proto::InputEvent) -> Result<(), String> {
            self.sent_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn get_screen_info(&self) -> ScreenInfo {
            ScreenInfo {
                width: 1920,
                height: 1080,
                scale: 1.0,
                logical_width: Some(1920),
                logical_height: Some(1080),
                monitors: vec![],
                connected_host: "test-host".to_string(),
            }
        }

        fn get_latest_frame_nv12(&self) -> Option<(u32, u32, Arc<Vec<u8>>)> {
            let w = 64u32;
            let h = 64u32;
            let len = (w * h * 3 / 2) as usize;
            Some((w, h, Arc::new(vec![128u8; len])))
        }
    }

    #[tokio::test]
    async fn server_handles_http_requests() {
        use tokio::io::AsyncReadExt;

        let backend = Arc::new(MockBackend {
            sent_count: AtomicUsize::new(0),
        });

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let (server, addr) = AgentServer::bind("127.0.0.1:0".parse().unwrap(), backend.clone())
            .await
            .unwrap();

        let server_handle = tokio::spawn(async move {
            server.run(shutdown_rx).await.unwrap();
        });

        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(b"GET /api/v1/health HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();

        let mut resp_bytes = Vec::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            stream.read_to_end(&mut resp_bytes),
        )
        .await
        .expect("health read timeout")
        .unwrap();

        let delimiter = b"\r\n\r\n";
        let header_end = resp_bytes
            .windows(delimiter.len())
            .position(|w| w == delimiter)
            .expect("malformed HTTP response: missing header delimiter");
        let header_str = std::str::from_utf8(&resp_bytes[..header_end]).unwrap();
        assert!(header_str.starts_with("HTTP/1.1 200 OK"));

        let body_bytes = &resp_bytes[header_end + delimiter.len()..];
        let body_val: serde_json::Value = serde_json::from_slice(body_bytes).unwrap();
        assert_eq!(body_val["status"], "ok");

        let mut stream = TcpStream::connect(addr).await.unwrap();
        let action_json = b"{\"action\":\"click\",\"x\":100.0,\"y\":200.0,\"button\":\"left\",\"count\":1,\"normalized\":false}";
        let req = format!(
            "POST /api/v1/input/action HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n",
            action_json.len()
        );
        stream.write_all(req.as_bytes()).await.unwrap();
        stream.write_all(action_json).await.unwrap();

        let mut resp_bytes = Vec::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            stream.read_to_end(&mut resp_bytes),
        )
        .await
        .expect("action read timeout")
        .unwrap();

        let header_end = resp_bytes
            .windows(delimiter.len())
            .position(|w| w == delimiter)
            .expect("malformed HTTP response: missing header delimiter");
        let header_str = std::str::from_utf8(&resp_bytes[..header_end]).unwrap();
        assert!(header_str.starts_with("HTTP/1.1 200 OK"));

        let body_bytes = &resp_bytes[header_end + delimiter.len()..];
        let body_val: serde_json::Value = serde_json::from_slice(body_bytes).unwrap();
        assert_eq!(body_val["ok"], true);
        assert_eq!(backend.sent_count.load(Ordering::SeqCst), 3);

        let _ = shutdown_tx.send(true);
        let _ = server_handle.await;
    }

    struct DisconnectBackend {
        supports: bool,
        disconnect_requested: std::sync::atomic::AtomicBool,
        events: std::sync::Mutex<Vec<ferryx_rd_proto::InputEvent>>,
    }

    impl AgentServerBackend for DisconnectBackend {
        fn send_input_event(&self, event: ferryx_rd_proto::InputEvent) -> Result<(), String> {
            self.events.lock().unwrap().push(event);
            Ok(())
        }

        fn get_screen_info(&self) -> ScreenInfo {
            MockBackend {
                sent_count: AtomicUsize::new(0),
            }
            .get_screen_info()
        }

        fn get_latest_frame_nv12(&self) -> Option<(u32, u32, Arc<Vec<u8>>)> {
            None
        }

        fn supports_session_disconnect(&self) -> bool {
            self.supports
        }

        fn try_disconnect_session(&self) -> Result<(), String> {
            self.disconnect_requested.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    fn disconnect_backend(supports: bool) -> Arc<DisconnectBackend> {
        Arc::new(DisconnectBackend {
            supports,
            disconnect_requested: std::sync::atomic::AtomicBool::new(false),
            events: std::sync::Mutex::new(Vec::new()),
        })
    }

    async fn http_roundtrip(addr: SocketAddr, request: &str) -> (String, serde_json::Value) {
        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut bytes = Vec::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            stream.read_to_end(&mut bytes),
        )
        .await
        .expect("response deadline")
        .unwrap();
        let header_end = bytes
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .expect("response header delimiter");
        let line_end = bytes.iter().position(|&b| b == b'\n').unwrap();
        let status = String::from_utf8_lossy(&bytes[..line_end - 1]).into_owned();
        let body: serde_json::Value = serde_json::from_slice(&bytes[header_end + 4..]).unwrap();
        (status, body)
    }

    async fn disconnect_roundtrip(addr: SocketAddr) -> (String, serde_json::Value) {
        http_roundtrip(
            addr,
            "POST /api/v1/session/disconnect HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n",
        )
        .await
    }

    #[tokio::test]
    async fn disconnect_answers_then_stops_worker_listener_and_backend_session() {
        // Given: a serving agent server whose backend owns a disconnectible session.
        let backend = disconnect_backend(true);
        let (server, addr) = AgentServer::bind("127.0.0.1:0".parse().unwrap(), backend.clone())
            .await
            .unwrap();
        let (shutdown, rx) = tokio::sync::watch::channel(false);
        let worker = tokio::spawn(server.run(rx));
        // When: the disconnect request is sent and its full response observed.
        let (status, body) = disconnect_roundtrip(addr).await;
        // Then: success is reported before any teardown, with nothing held
        // (the reset transaction always emits a trailing Reset event).
        assert_eq!(status, "HTTP/1.1 200 OK");
        assert_eq!(body["ok"], true);
        assert_eq!(body["disconnected"], true);
        assert_eq!(body["released_inputs"], 1);
        // The owned worker ends itself and the listener port closes.
        tokio::time::timeout(std::time::Duration::from_secs(5), worker)
            .await
            .expect("worker must stop after disconnect")
            .unwrap()
            .unwrap();
        assert!(
            TcpStream::connect(addr).await.is_err(),
            "listener must be closed after disconnect"
        );
        // Only after the worker ended is the backend session stop triggered.
        assert!(backend.disconnect_requested.load(Ordering::SeqCst));
        let _ = shutdown.send(true);
    }

    #[tokio::test]
    async fn disconnect_on_unsupported_backend_refuses_without_stopping() {
        // Given: a serving agent server whose backend cannot stop a session.
        let backend = disconnect_backend(false);
        let (server, addr) = AgentServer::bind("127.0.0.1:0".parse().unwrap(), backend.clone())
            .await
            .unwrap();
        let (shutdown, rx) = tokio::sync::watch::channel(false);
        let worker = tokio::spawn(server.run(rx));
        // When: disconnect is requested.
        let (status, body) = disconnect_roundtrip(addr).await;
        // Then: no disconnected claim is made and the worker keeps serving.
        assert_eq!(status, "HTTP/1.1 501 Not Implemented");
        assert_eq!(body["ok"], false);
        assert_eq!(body["disconnected"], false);
        assert!(body["error"].is_string());
        assert!(!backend.disconnect_requested.load(Ordering::SeqCst));
        let (health_status, health_body) = http_roundtrip(
            addr,
            "GET /api/v1/health HTTP/1.1\r\nHost: localhost\r\n\r\n",
        )
        .await;
        assert_eq!(health_status, "HTTP/1.1 200 OK");
        assert_eq!(health_body["status"], "ok");
        shutdown.send(true).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), worker)
            .await
            .expect("worker must stop after shutdown")
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn disconnect_releases_held_input_with_reset_transaction_before_answering() {
        // Given: a held left mouse button on the shared input tracker.
        let backend = disconnect_backend(true);
        let (server, addr) = AgentServer::bind("127.0.0.1:0".parse().unwrap(), backend.clone())
            .await
            .unwrap();
        let tracker = server.tracker.clone();
        let (shutdown, rx) = tokio::sync::watch::channel(false);
        let worker = tokio::spawn(server.run(rx));
        let down_body = r#"{"action":"mouse_down","button":"left"}"#;
        let (_, down_response) = http_roundtrip(
            addr,
            &format!(
                "POST /api/v1/input/action HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{down_body}",
                down_body.len()
            ),
        )
        .await;
        assert_eq!(down_response["events_sent"], 1);
        // When: disconnect arrives.
        let (status, body) = disconnect_roundtrip(addr).await;
        // Then: the held button was released before the success answer.
        assert_eq!(status, "HTTP/1.1 200 OK");
        assert_eq!(body["disconnected"], true);
        assert_eq!(body["released_inputs"], 2);
        assert!(tracker.try_lock().unwrap().is_empty());
        let events: Vec<_> = backend
            .events
            .lock()
            .unwrap()
            .iter()
            .map(|e| e.event_type)
            .collect();
        assert_eq!(
            events,
            vec![
                ferryx_rd_proto::InputEventType::LeftMouseDown,
                ferryx_rd_proto::InputEventType::LeftMouseUp,
                ferryx_rd_proto::InputEventType::Reset,
            ]
        );
        tokio::time::timeout(std::time::Duration::from_secs(5), worker)
            .await
            .expect("worker must stop after disconnect")
            .unwrap()
            .unwrap();
        assert!(backend.disconnect_requested.load(Ordering::SeqCst));
        let _ = shutdown.send(true);
    }

    struct GatedStopBackend {
        events: std::sync::Mutex<Vec<ferryx_rd_proto::InputEvent>>,
        stop_started: std::sync::Mutex<Option<std::sync::mpsc::Sender<()>>>,
        release: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
        disconnect_started: std::sync::atomic::AtomicBool,
    }

    impl AgentServerBackend for GatedStopBackend {
        fn send_input_event(&self, event: ferryx_rd_proto::InputEvent) -> Result<(), String> {
            self.events.lock().unwrap().push(event);
            Ok(())
        }

        fn get_screen_info(&self) -> ScreenInfo {
            MockBackend {
                sent_count: AtomicUsize::new(0),
            }
            .get_screen_info()
        }

        fn get_latest_frame_nv12(&self) -> Option<(u32, u32, Arc<Vec<u8>>)> {
            None
        }

        fn supports_session_disconnect(&self) -> bool {
            true
        }

        fn try_disconnect_session(&self) -> Result<(), String> {
            self.disconnect_started.store(true, Ordering::SeqCst);
            if let Some(started) = self.stop_started.lock().unwrap().take() {
                let _ = started.send(());
            }
            // Park until the observing test confirms the response arrived; a
            // regression that stops before answering would time out here.
            self.release
                .lock()
                .unwrap()
                .recv_timeout(std::time::Duration::from_secs(5))
                .map_err(|err| format!("disconnect stop gate: {err}"))
        }
    }

    #[test]
    fn disconnect_response_fully_arrives_before_backend_stop_unblocks() {
        // Given: a real socket server whose stop trigger parks until released.
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let backend = Arc::new(GatedStopBackend {
            events: std::sync::Mutex::new(Vec::new()),
            stop_started: std::sync::Mutex::new(Some(started_tx)),
            release: std::sync::Mutex::new(release_rx),
            disconnect_started: std::sync::atomic::AtomicBool::new(false),
        });
        let mut fixture = HttpFixture::start(backend.clone(), release_tx);
        let mut stream = socket_request(
            fixture.addr,
            "POST /api/v1/session/disconnect HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n",
        );
        // When: the response is read to EOF, so it was fully written first.
        let response = read_json(&mut stream).unwrap();
        assert_eq!(response["ok"], true);
        assert_eq!(response["disconnected"], true);
        assert_eq!(response["released_inputs"], 1);
        started_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        assert!(
            std::net::TcpStream::connect(fixture.addr).is_err(),
            "listener must close before backend teardown starts"
        );
        // Then: releasing the gate lets the stop complete and the worker join.
        fixture.release();
        fixture.thread.take().unwrap().join().unwrap();
        assert!(backend.disconnect_started.load(Ordering::SeqCst));
        assert!(std::net::TcpStream::connect(fixture.addr).is_err());
        drop(fixture);
    }

    struct FailingReleaseBackend;
    impl AgentServerBackend for FailingReleaseBackend {
        fn send_input_event(&self, _: ferryx_rd_proto::InputEvent) -> Result<(), String> {
            Err("hardware injection failed".into())
        }
        fn get_screen_info(&self) -> ScreenInfo {
            MockBackend {
                sent_count: AtomicUsize::new(0),
            }
            .get_screen_info()
        }
        fn get_latest_frame_nv12(&self) -> Option<(u32, u32, Arc<Vec<u8>>)> {
            None
        }
        fn supports_session_disconnect(&self) -> bool {
            true
        }
        fn try_disconnect_session(&self) -> Result<(), String> {
            Ok(())
        }
    }

    async fn release_failure_response(path: &str, count_field: &str) {
        let (server, addr) = AgentServer::bind(
            "127.0.0.1:0".parse().unwrap(),
            Arc::new(FailingReleaseBackend),
        )
        .await
        .unwrap();
        let (shutdown, rx) = tokio::sync::watch::channel(false);
        let worker = tokio::spawn(server.run(rx));
        let (status, body) = http_roundtrip(
            addr,
            &format!("POST /api/v1/{path} HTTP/1.1\r\nContent-Length: 0\r\n\r\n"),
        )
        .await;
        let _ = shutdown.send(true);
        worker.await.unwrap().unwrap();
        assert_eq!(status, "HTTP/1.1 500 Internal Error", "{path}");
        assert_eq!(body["ok"], false);
        assert_eq!(body[count_field], 0);
        if path == "session/disconnect" {
            assert_eq!(body["disconnected"], false);
        }
    }

    #[tokio::test]
    async fn disconnect_release_failure_never_reports_success() {
        release_failure_response("session/disconnect", "released_inputs").await;
    }

    #[tokio::test]
    async fn reset_release_failure_never_reports_success() {
        release_failure_response("input/reset", "reset_events_sent").await;
    }

    struct FailOnBackend {
        fail_on: ferryx_rd_proto::InputEventType,
        attempted: std::sync::Mutex<Vec<ferryx_rd_proto::InputEventType>>,
    }
    impl AgentServerBackend for FailOnBackend {
        fn send_input_event(&self, event: ferryx_rd_proto::InputEvent) -> Result<(), String> {
            self.attempted.lock().unwrap().push(event.event_type);
            if event.event_type == self.fail_on {
                return Err("scripted transmission failure".into());
            }
            Ok(())
        }
        fn get_screen_info(&self) -> ScreenInfo {
            MockBackend {
                sent_count: AtomicUsize::new(0),
            }
            .get_screen_info()
        }
        fn get_latest_frame_nv12(&self) -> Option<(u32, u32, Arc<Vec<u8>>)> {
            None
        }
    }

    #[tokio::test]
    async fn failed_dispatch_releases_already_sent_downs() {
        use ferryx_rd_proto::InputEventType::{LeftMouseDown, LeftMouseUp, Reset};
        // Given a backend that accepts the down of a batch but rejects its up.
        let backend = Arc::new(FailOnBackend {
            fail_on: LeftMouseUp,
            attempted: std::sync::Mutex::new(Vec::new()),
        });
        let (server, addr) = AgentServer::bind("127.0.0.1:0".parse().unwrap(), backend.clone())
            .await
            .unwrap();
        let tracker = server.tracker.clone();
        let (shutdown, rx) = tokio::sync::watch::channel(false);
        let worker = tokio::spawn(server.run(rx));
        let body =
            r#"[{"action":"mouse_down","button":"left"},{"action":"mouse_up","button":"left"}]"#;
        // When the batch is dispatched.
        let (status, response) = http_roundtrip(
            addr,
            &format!(
                "POST /api/v1/input/batch HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            ),
        )
        .await;
        let _ = shutdown.send(true);
        worker.await.unwrap().unwrap();
        // Then the failure is reported and the accepted down was released with a trailing Reset.
        assert_eq!(status, "HTTP/1.1 500 Internal Error");
        assert_eq!(response["ok"], false);
        let attempted = backend.attempted.lock().unwrap().clone();
        assert_eq!(attempted.first(), Some(&LeftMouseDown));
        assert_eq!(attempted.last(), Some(&Reset));
        assert!(
            attempted.iter().filter(|e| **e == LeftMouseUp).count() >= 2,
            "the accepted down must be released again after the failure: {attempted:?}"
        );
        assert!(tracker.try_lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn external_shutdown_releases_held_input() {
        use ferryx_rd_proto::InputEventType::{LeftMouseDown, LeftMouseUp, Reset};
        // Given a held left button and an external shutdown (no disconnect request).
        let backend = disconnect_backend(false);
        let (server, addr) = AgentServer::bind("127.0.0.1:0".parse().unwrap(), backend.clone())
            .await
            .unwrap();
        let tracker = server.tracker.clone();
        let (shutdown, rx) = tokio::sync::watch::channel(false);
        let worker = tokio::spawn(server.run(rx));
        let body = r#"{"action":"mouse_down","button":"left"}"#;
        let (_, down) = http_roundtrip(
            addr,
            &format!(
                "POST /api/v1/input/action HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            ),
        )
        .await;
        assert_eq!(down["events_sent"], 1);
        // When the server is shut down externally.
        shutdown.send(true).unwrap();
        worker.await.unwrap().unwrap();
        // Then the held button was released before returning.
        let events: Vec<_> = backend
            .events
            .lock()
            .unwrap()
            .iter()
            .map(|e| e.event_type)
            .collect();
        assert_eq!(events, vec![LeftMouseDown, LeftMouseUp, Reset]);
        assert!(tracker.try_lock().unwrap().is_empty());
    }

    struct DisconnectGateBackend {
        entered: std::sync::Mutex<Option<std::sync::mpsc::Sender<()>>>,
        release: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
        events: std::sync::Mutex<Vec<ferryx_rd_proto::InputEvent>>,
    }
    impl AgentServerBackend for DisconnectGateBackend {
        fn send_input_event(&self, event: ferryx_rd_proto::InputEvent) -> Result<(), String> {
            if let Some(entered) = self.entered.lock().unwrap().take() {
                entered.send(()).unwrap();
                self.release.lock().unwrap().recv().unwrap();
            }
            self.events.lock().unwrap().push(event);
            Ok(())
        }
        fn get_screen_info(&self) -> ScreenInfo {
            MockBackend {
                sent_count: AtomicUsize::new(0),
            }
            .get_screen_info()
        }
        fn get_latest_frame_nv12(&self) -> Option<(u32, u32, Arc<Vec<u8>>)> {
            None
        }
        fn supports_session_disconnect(&self) -> bool {
            true
        }
        fn try_disconnect_session(&self) -> Result<(), String> {
            Ok(())
        }
    }

    #[test]
    fn disconnect_gates_new_input_before_release() {
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let backend = Arc::new(DisconnectGateBackend {
            entered: std::sync::Mutex::new(Some(entered_tx)),
            release: std::sync::Mutex::new(release_rx),
            events: std::sync::Mutex::new(Vec::new()),
        });
        let mut fixture = HttpFixture::start(backend.clone(), release_tx);
        let mut disconnect = socket_request(
            fixture.addr,
            "POST /api/v1/session/disconnect HTTP/1.1\r\nContent-Length: 0\r\n\r\n",
        );
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        let body = r#"{"action":"mouse_down","button":"left"}"#;
        let mut action = socket_request(
            fixture.addr,
            &format!(
                "POST /api/v1/input/action HTTP/1.1\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            ),
        );
        let rejected = read_json(&mut action);
        fixture.release();
        let disconnected = read_json(&mut disconnect).unwrap();
        drop(fixture);
        assert_eq!(
            rejected.expect("input must reject before release")["ok"],
            false
        );
        assert_eq!(disconnected["ok"], true);
        assert_eq!(backend.events.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn unauthorized_request_returns_401() {
        let backend = Arc::new(MockBackend {
            sent_count: AtomicUsize::new(0),
        });
        let (mut server, addr) = AgentServer::bind("127.0.0.1:0".parse().unwrap(), backend.clone())
            .await
            .unwrap();
        server.set_auth_token("secret-token");
        let (shutdown, rx) = tokio::sync::watch::channel(false);
        let task = tokio::spawn(server.run(rx));
        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(b"GET /api/v1/screen/info HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();
        let mut resp_bytes = Vec::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            stream.read_to_end(&mut resp_bytes),
        )
        .await
        .expect("read timeout")
        .unwrap();
        let delimiter = b"\r\n\r\n";
        let header_end = resp_bytes
            .windows(delimiter.len())
            .position(|w| w == delimiter)
            .expect("header delimiter");
        let header_str = std::str::from_utf8(&resp_bytes[..header_end]).unwrap();
        assert!(header_str.starts_with("HTTP/1.1 401 Unauthorized"));
        let body_bytes = &resp_bytes[header_end + delimiter.len()..];
        let body_val: serde_json::Value = serde_json::from_slice(body_bytes).unwrap();
        assert_eq!(body_val["ok"], false);
        assert_eq!(body_val["error"], "unauthorized");
        shutdown.send(true).unwrap();
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn authorized_request_with_bearer_token_returns_200() {
        let backend = Arc::new(MockBackend {
            sent_count: AtomicUsize::new(0),
        });
        let (mut server, addr) = AgentServer::bind("127.0.0.1:0".parse().unwrap(), backend.clone())
            .await
            .unwrap();
        server.set_auth_token("secret-token");
        let (shutdown, rx) = tokio::sync::watch::channel(false);
        let task = tokio::spawn(server.run(rx));
        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(b"GET /api/v1/screen/info HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer secret-token\r\n\r\n")
            .await
            .unwrap();
        let mut resp_bytes = Vec::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            stream.read_to_end(&mut resp_bytes),
        )
        .await
        .expect("read timeout")
        .unwrap();
        let delimiter = b"\r\n\r\n";
        let header_end = resp_bytes
            .windows(delimiter.len())
            .position(|w| w == delimiter)
            .expect("header delimiter");
        let header_str = std::str::from_utf8(&resp_bytes[..header_end]).unwrap();
        assert!(
            header_str.starts_with("HTTP/1.1 200 OK"),
            "expected 200 OK, got: {}",
            header_str
        );
        let body_bytes = &resp_bytes[header_end + delimiter.len()..];
        let body_val: serde_json::Value = serde_json::from_slice(body_bytes).unwrap();
        assert_eq!(body_val["width"], 1920);
        shutdown.send(true).unwrap();
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn authorized_request_with_x_maho_token_returns_200() {
        let backend = Arc::new(MockBackend {
            sent_count: AtomicUsize::new(0),
        });
        let (mut server, addr) = AgentServer::bind("127.0.0.1:0".parse().unwrap(), backend.clone())
            .await
            .unwrap();
        server.set_auth_token("secret-token");
        let (shutdown, rx) = tokio::sync::watch::channel(false);
        let task = tokio::spawn(server.run(rx));
        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(b"GET /api/v1/screen/info HTTP/1.1\r\nHost: localhost\r\nX-Maho-Token: secret-token\r\n\r\n")
            .await
            .unwrap();
        let mut resp_bytes = Vec::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            stream.read_to_end(&mut resp_bytes),
        )
        .await
        .expect("read timeout")
        .unwrap();
        let delimiter = b"\r\n\r\n";
        let header_end = resp_bytes
            .windows(delimiter.len())
            .position(|w| w == delimiter)
            .expect("header delimiter");
        let header_str = std::str::from_utf8(&resp_bytes[..header_end]).unwrap();
        assert!(
            header_str.starts_with("HTTP/1.1 200 OK"),
            "expected 200 OK, got: {}",
            header_str
        );
        let body_bytes = &resp_bytes[header_end + delimiter.len()..];
        let body_val: serde_json::Value = serde_json::from_slice(body_bytes).unwrap();
        assert_eq!(body_val["width"], 1920);
        shutdown.send(true).unwrap();
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn health_endpoint_requires_no_auth() {
        let backend = Arc::new(MockBackend {
            sent_count: AtomicUsize::new(0),
        });
        let (mut server, addr) = AgentServer::bind("127.0.0.1:0".parse().unwrap(), backend.clone())
            .await
            .unwrap();
        server.set_auth_token("secret-token");
        let (shutdown, rx) = tokio::sync::watch::channel(false);
        let task = tokio::spawn(server.run(rx));
        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(b"GET /api/v1/health HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();
        let mut resp_bytes = Vec::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            stream.read_to_end(&mut resp_bytes),
        )
        .await
        .expect("read timeout")
        .unwrap();
        let delimiter = b"\r\n\r\n";
        let header_end = resp_bytes
            .windows(delimiter.len())
            .position(|w| w == delimiter)
            .expect("header delimiter");
        let header_str = std::str::from_utf8(&resp_bytes[..header_end]).unwrap();
        assert!(header_str.starts_with("HTTP/1.1 200 OK"));
        let body_bytes = &resp_bytes[header_end + delimiter.len()..];
        let body_val: serde_json::Value = serde_json::from_slice(body_bytes).unwrap();
        assert_eq!(body_val["status"], "ok");
        shutdown.send(true).unwrap();
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn response_headers_omit_access_control_allow_origin_wildcard() {
        let backend = Arc::new(MockBackend {
            sent_count: AtomicUsize::new(0),
        });
        let (server, addr) = AgentServer::bind("127.0.0.1:0".parse().unwrap(), backend.clone())
            .await
            .unwrap();
        let (shutdown, rx) = tokio::sync::watch::channel(false);
        let task = tokio::spawn(server.run(rx));
        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(b"GET /api/v1/health HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();
        let mut resp_bytes = Vec::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            stream.read_to_end(&mut resp_bytes),
        )
        .await
        .expect("read timeout")
        .unwrap();
        let delimiter = b"\r\n\r\n";
        let header_end = resp_bytes
            .windows(delimiter.len())
            .position(|w| w == delimiter)
            .expect("header delimiter");
        let header_str = std::str::from_utf8(&resp_bytes[..header_end]).unwrap();
        assert!(
            !header_str.contains("Access-Control-Allow-Origin"),
            "CORS wildcard must not be present"
        );
        shutdown.send(true).unwrap();
        task.await.unwrap().unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn idle_header_and_body_reader_deadlines() {
        use std::{future::Future, task::Poll, time::Duration};
        for partial in [
            &b""[..],
            &b"POST / HTTP/1.1\r\nContent-Length: 2\r\n\r\nx"[..],
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let mut client = TcpStream::connect(listener.local_addr().unwrap())
                .await
                .unwrap();
            let (mut server, _) = listener.accept().await.unwrap();
            client.write_all(partial).await.unwrap();
            if !partial.is_empty() {
                server.readable().await.unwrap();
            }
            let request = read_request(&mut server);
            tokio::pin!(request);
            // Register the actual reader before moving virtual time, not an accept/read guess.
            std::future::poll_fn(|cx| {
                assert!(request.as_mut().poll(cx).is_pending());
                Poll::Ready(())
            })
            .await;
            tokio::time::advance(Duration::from_secs(6)).await;
            let result = tokio::time::timeout(Duration::from_secs(1), request).await;
            assert_eq!(
                result
                    .expect("idle request must expire")
                    .unwrap_err()
                    .kind(),
                std::io::ErrorKind::TimedOut
            );
        }
    }

    #[test]
    fn saturated_screenshot_rejects_without_starting_another_job() {
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let backend = Arc::new(GatedBackend {
            screenshot: true,
            events: std::sync::Mutex::new(Vec::new()),
            entered: std::sync::Mutex::new(Some(entered_tx)),
            release: std::sync::Mutex::new(release_rx),
        });
        let mut fixture = HttpFixture::start(backend, release_tx);
        let request = "GET /api/v1/screen/screenshot HTTP/1.1\r\n\r\n";
        let mut first = socket_request(fixture.addr, request);
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        let mut second = socket_request(fixture.addr, request);
        let rejected = read_json(&mut second);
        fixture.release();
        assert_eq!(read_json(&mut first).unwrap()["ok"], true);
        drop(fixture);
        assert_eq!(
            rejected.expect("screenshot must reject before backend release")["ok"],
            false
        );
    }

    #[tokio::test]
    async fn idle_connections_are_capped() {
        let backend = Arc::new(MockBackend {
            sent_count: AtomicUsize::new(0),
        });
        let (mut server, addr) = AgentServer::bind("127.0.0.1:0".parse().unwrap(), backend)
            .await
            .unwrap();
        let (accepted_tx, mut accepted_rx) = tokio::sync::mpsc::channel(1);
        server.accepted = Some(accepted_tx);
        let (shutdown, rx) = tokio::sync::watch::channel(false);
        let worker = tokio::spawn(server.run(rx));
        let mut clients = Vec::new();
        for _ in 0..33 {
            clients.push(TcpStream::connect(addr).await.unwrap());
            tokio::time::timeout(std::time::Duration::from_secs(5), accepted_rx.recv())
                .await
                .unwrap()
                .unwrap();
        }
        let mut response = Vec::new();
        let overflow = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            clients.last_mut().unwrap().read_to_end(&mut response),
        )
        .await;
        shutdown.send(true).unwrap();
        worker.await.unwrap().unwrap();
        assert_eq!(
            overflow.expect("33rd idle connection must close").unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn shutdown_closes_owned_idle_connections() {
        let backend = Arc::new(MockBackend {
            sent_count: AtomicUsize::new(0),
        });
        let (mut server, addr) = AgentServer::bind("127.0.0.1:0".parse().unwrap(), backend)
            .await
            .unwrap();
        let (accepted_tx, mut accepted_rx) = tokio::sync::mpsc::channel(1);
        server.accepted = Some(accepted_tx);
        let (shutdown, rx) = tokio::sync::watch::channel(false);
        let worker = tokio::spawn(server.run(rx));
        let mut client = TcpStream::connect(addr).await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), accepted_rx.recv())
            .await
            .unwrap()
            .unwrap();
        shutdown.send(true).unwrap();
        worker.await.unwrap().unwrap();
        let mut response = Vec::new();
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(1),
                client.read_to_end(&mut response)
            )
            .await
            .expect("owned socket must close with server")
            .unwrap(),
            0
        );
    }

    struct MetadataBackend {
        sent_count: AtomicUsize,
        frame_metadata: std::sync::Mutex<Option<FrameMetadata>>,
    }

    impl AgentServerBackend for MetadataBackend {
        fn send_input_event(&self, _event: ferryx_rd_proto::InputEvent) -> Result<(), String> {
            self.sent_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn get_screen_info(&self) -> ScreenInfo {
            MockBackend {
                sent_count: AtomicUsize::new(0),
            }
            .get_screen_info()
        }

        fn get_latest_frame_nv12(&self) -> Option<(u32, u32, Arc<Vec<u8>>)> {
            let w = 64u32;
            let h = 64u32;
            let len = (w * h * 3 / 2) as usize;
            Some((w, h, Arc::new(vec![128u8; len])))
        }

        fn get_latest_frame_metadata(&self) -> Option<FrameMetadata> {
            *self.frame_metadata.lock().unwrap()
        }
    }

    #[tokio::test]
    async fn screenshot_response_includes_frame_metadata_when_present() {
        let backend = Arc::new(MetadataBackend {
            sent_count: AtomicUsize::new(0),
            frame_metadata: std::sync::Mutex::new(Some(FrameMetadata {
                frame_id: 42,
                timestamp_ms: 1000,
                age_ms: 50,
            })),
        });
        let (server, addr) = AgentServer::bind("127.0.0.1:0".parse().unwrap(), backend)
            .await
            .unwrap();
        let (shutdown, rx) = tokio::sync::watch::channel(false);
        let task = tokio::spawn(server.run(rx));

        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(b"GET /api/v1/screen/screenshot HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();

        let mut resp_bytes = Vec::new();
        tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut resp_bytes))
            .await
            .expect("screenshot read timeout")
            .unwrap();

        let header_end = resp_bytes
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .unwrap();
        let body_bytes = &resp_bytes[header_end + 4..];
        let body_val: serde_json::Value = serde_json::from_slice(body_bytes).unwrap();
        assert_eq!(body_val["frame_id"], 42);
        assert_eq!(body_val["timestamp_ms"], 1000);
        assert_eq!(body_val["age_ms"], 50);

        shutdown.send(true).unwrap();
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn wait_change_returns_immediately_when_frame_changed() {
        let backend = Arc::new(MetadataBackend {
            sent_count: AtomicUsize::new(0),
            frame_metadata: std::sync::Mutex::new(Some(FrameMetadata {
                frame_id: 100,
                timestamp_ms: 2000,
                age_ms: 10,
            })),
        });
        let (server, addr) = AgentServer::bind("127.0.0.1:0".parse().unwrap(), backend)
            .await
            .unwrap();
        let (shutdown, rx) = tokio::sync::watch::channel(false);
        let task = tokio::spawn(server.run(rx));

        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(b"GET /api/v1/screen/wait_change?last_frame_id=99&timeout_ms=5000 HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();

        let mut resp_bytes = Vec::new();
        tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut resp_bytes))
            .await
            .expect("wait_change read timeout")
            .unwrap();

        let header_end = resp_bytes
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .unwrap();
        let body_bytes = &resp_bytes[header_end + 4..];
        let body_val: serde_json::Value = serde_json::from_slice(body_bytes).unwrap();
        assert_eq!(body_val["ok"], true);
        assert_eq!(body_val["changed"], true);
        assert_eq!(body_val["frame_id"], 100);

        shutdown.send(true).unwrap();
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn wait_change_returns_timeout_when_no_change() {
        let backend = Arc::new(MetadataBackend {
            sent_count: AtomicUsize::new(0),
            frame_metadata: std::sync::Mutex::new(Some(FrameMetadata {
                frame_id: 50,
                timestamp_ms: 1500,
                age_ms: 20,
            })),
        });
        let (server, addr) = AgentServer::bind("127.0.0.1:0".parse().unwrap(), backend)
            .await
            .unwrap();
        let (shutdown, rx) = tokio::sync::watch::channel(false);
        let task = tokio::spawn(server.run(rx));

        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(b"GET /api/v1/screen/wait_change?last_frame_id=50&timeout_ms=100 HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();

        let mut resp_bytes = Vec::new();
        tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut resp_bytes))
            .await
            .expect("wait_change read timeout")
            .unwrap();

        let header_end = resp_bytes
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .unwrap();
        let body_bytes = &resp_bytes[header_end + 4..];
        let body_val: serde_json::Value = serde_json::from_slice(body_bytes).unwrap();
        assert_eq!(body_val["ok"], true);
        assert_eq!(body_val["changed"], false);
        assert_eq!(body_val["frame_id"], 50);

        shutdown.send(true).unwrap();
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn wait_change_requires_authentication() {
        let backend = Arc::new(MetadataBackend {
            sent_count: AtomicUsize::new(0),
            frame_metadata: std::sync::Mutex::new(Some(FrameMetadata {
                frame_id: 60,
                timestamp_ms: 1800,
                age_ms: 30,
            })),
        });
        let (mut server, addr) = AgentServer::bind("127.0.0.1:0".parse().unwrap(), backend)
            .await
            .unwrap();
        server.set_auth_token("secret-token");
        let (shutdown, rx) = tokio::sync::watch::channel(false);
        let task = tokio::spawn(server.run(rx));

        // Test without auth header should be rejected
        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(b"GET /api/v1/screen/wait_change?last_frame_id=50&timeout_ms=100 HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();

        let mut resp_bytes = Vec::new();
        tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut resp_bytes))
            .await
            .expect("read timeout")
            .unwrap();

        let header_line = String::from_utf8_lossy(
            &resp_bytes[..resp_bytes.iter().position(|&b| b == b'\n').unwrap()],
        );
        assert!(
            header_line.contains("401"),
            "should return 401 Unauthorized without auth"
        );

        // Test with valid auth header should succeed
        let mut stream2 = TcpStream::connect(addr).await.unwrap();
        stream2
            .write_all(b"GET /api/v1/screen/wait_change?last_frame_id=50&timeout_ms=100 HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer secret-token\r\n\r\n")
            .await
            .unwrap();

        let mut resp_bytes2 = Vec::new();
        tokio::time::timeout(
            Duration::from_secs(5),
            stream2.read_to_end(&mut resp_bytes2),
        )
        .await
        .expect("read timeout")
        .unwrap();

        let header_line2 = String::from_utf8_lossy(
            &resp_bytes2[..resp_bytes2.iter().position(|&b| b == b'\n').unwrap()],
        );
        assert!(
            header_line2.contains("200"),
            "should return 200 OK with valid auth"
        );

        shutdown.send(true).unwrap();
        task.await.unwrap().unwrap();
    }
}
