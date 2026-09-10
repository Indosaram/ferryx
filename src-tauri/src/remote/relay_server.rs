//! ferryx-relay: the public relay server that brokers WebSocket tunnels
//! between a desktop daemon (behind NAT/firewall) and a remote client.
//!
//! Three endpoints cooperate to build a session:
//!
//! - `GET /tunnel/control`            - the desktop daemon holds this socket
//!   open for the lifetime of the process. The relay uses it to push
//!   incoming-session notifications so the daemon knows when to open a data
//!   channel.
//! - `GET /tunnel/data/:session_id`   - the desktop daemon opens one of these
//!   per session, in response to a control-channel notification.
//! - `GET /tunnel/client/:session_id` - the remote client opens one of these
//!   to attach to the session. Once both the data and client sockets are
//!   present the relay proxies frames between them, bidirectionally, until
//!   either side disconnects.
//!
//! Authentication is a single bearer credential - the desktop daemon's
//! Machine Token - checked on the control channel. Data and client
//! channels are bound to a `session_id` that is only ever handed out over
//! the (already authenticated) control channel, so they do not need to
//! re-present the Machine Token. An authenticated control socket can send
//! `{"type":"AllocateSession"}` to receive a fresh UUID in an
//! `IncomingSessionNotice`. Trusted in-process callers can also issue via
//! `notify_incoming_session`. Unknown IDs are rejected before upgrade;
//! each issued ID accepts exactly one data half and one client half.
//! Pending and active sessions share a 100-entry budget. Pairing expires
//! after 30 seconds; sends time out after 30 seconds and sessions after one hour.

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path as AxumPath, State,
    },
    http::{HeaderMap, StatusCode},
    response::Response,
    routing::get,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;

/// How long a session may sit waiting for its data or client half before
/// the relay gives up and evicts it from the registry.
const SESSION_PAIRING_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_PENDING_SESSIONS: usize = 100;
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(30);
const SESSION_TRANSFER_TIMEOUT: Duration = Duration::from_secs(3600);
const MAX_MESSAGE_SIZE: usize = 64 * 1024;

/// Interval on which the background reaper sweeps the session registry for
/// entries that have exceeded [`SESSION_PAIRING_TIMEOUT`] without pairing.
const SESSION_SWEEP_INTERVAL: Duration = Duration::from_secs(10);

/// Notification pushed down the control channel when a client asks to open
/// a new session against this daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncomingSessionNotice {
    pub session_id: String,
}

/// Which of the two tunnel halves a pending registry entry holds.
#[derive(Clone, Copy, PartialEq, Eq)]
enum HalfKind {
    Data,
    Client,
}

/// An issued session, retained through reservation and active transfer.
/// `notify` hands the second socket to the first connection's task.
struct WaitingHalf {
    generation: u64,
    created_at: Instant,
    kind: Option<HalfKind>,
    notify: Option<oneshot::Sender<WebSocket>>,
    active: bool,
}

struct ControlChannel {
    generation: u64,
    tx: mpsc::Sender<IncomingSessionNotice>,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum ControlRequest {
    AllocateSession,
}

/// Also cleans up failed upgrades and cancelled connection tasks.
struct SessionGuard {
    armed: bool,
    state: RelayState,
    session_id: String,
    generation: u64,
}

impl Drop for SessionGuard {
    fn drop(&mut self) {
        if self.armed {
            self.state.remove_pending(&self.session_id, self.generation);
        }
    }
}

/// Shared relay state: the set of authorized Machine Tokens, the live
/// control channels (one per connected daemon), and the registry of
/// sessions awaiting pairing.
#[derive(Clone)]
pub struct RelayState {
    inner: Arc<RelayInner>,
}

struct RelayInner {
    machine_tokens: Vec<String>,
    control_channels: Mutex<HashMap<String, ControlChannel>>,
    next_generation: AtomicU64,
    pending_sessions: Mutex<HashMap<String, WaitingHalf>>,
}

/// Outcome of offering a socket to the pairing registry.
enum PairingOutcome {
    /// The opposite half is reserved; send it the socket after upgrade.
    HandOff(oneshot::Sender<WebSocket>),
    /// The first half is reserved and awaits the counterpart after upgrade.
    Waiting { rx: oneshot::Receiver<WebSocket> },
}

impl RelayState {
    pub fn new(machine_tokens: Vec<String>) -> Self {
        Self {
            inner: Arc::new(RelayInner {
                machine_tokens,
                next_generation: AtomicU64::new(1),
                control_channels: Mutex::new(HashMap::new()),
                pending_sessions: Mutex::new(HashMap::new()),
            }),
        }
    }

    fn validate_machine_token(&self, token: &str) -> bool {
        if self.inner.machine_tokens.is_empty() {
            // No configured tokens means the relay has nothing to check
            // against; reject explicitly rather than silently accepting
            // anything.
            return false;
        }
        self.inner
            .machine_tokens
            .iter()
            .any(|candidate| candidate == token)
    }

    fn register_control_channel(
        &self,
        machine_token: String,
    ) -> (u64, mpsc::Receiver<IncomingSessionNotice>) {
        let (tx, rx) = mpsc::channel(MAX_PENDING_SESSIONS);
        let generation = self.inner.next_generation.fetch_add(1, Ordering::Relaxed);
        self.inner
            .control_channels
            .lock()
            .insert(machine_token, ControlChannel { generation, tx });
        (generation, rx)
    }

    fn unregister_control_channel(&self, machine_token: &str, generation: u64) {
        let mut channels = self.inner.control_channels.lock();
        if channels
            .get(machine_token)
            .is_some_and(|channel| channel.generation == generation)
        {
            channels.remove(machine_token);
        }
    }

    /// Trusted in-process issuance API. The caller must supply an opaque,
    /// unpredictable ID and a configured token with a live control channel.
    /// Registry insertion and notification are atomic with respect to admission.
    pub fn notify_incoming_session(&self, machine_token: &str, session_id: &str) -> bool {
        self.issue_session(machine_token, session_id, None)
    }

    fn issue_session(
        &self,
        machine_token: &str,
        session_id: &str,
        generation: Option<u64>,
    ) -> bool {
        if !self.validate_machine_token(machine_token) {
            return false;
        }
        let channels = self.inner.control_channels.lock();
        let Some(channel) = channels.get(machine_token) else {
            return false;
        };
        if generation.is_some_and(|id| id != channel.generation) {
            return false;
        }
        let mut sessions = self.inner.pending_sessions.lock();
        if sessions.len() >= MAX_PENDING_SESSIONS || sessions.contains_key(session_id) {
            return false;
        }
        if channel
            .tx
            .try_send(IncomingSessionNotice {
                session_id: session_id.into(),
            })
            .is_err()
        {
            return false;
        }
        sessions.insert(
            session_id.into(),
            WaitingHalf {
                generation: self.inner.next_generation.fetch_add(1, Ordering::Relaxed),
                created_at: Instant::now(),
                kind: None,
                notify: None,
                active: false,
            },
        );
        true
    }

    /// Reserve before upgrade so concurrent duplicates receive HTTP 409.
    fn reserve_half(
        &self,
        session_id: &str,
        kind: HalfKind,
    ) -> Result<(u64, PairingOutcome), StatusCode> {
        let mut sessions = self.inner.pending_sessions.lock();
        let waiting = sessions.get_mut(session_id).ok_or(StatusCode::NOT_FOUND)?;
        if waiting.active || waiting.kind == Some(kind) {
            return Err(StatusCode::CONFLICT);
        }
        if waiting.created_at.elapsed() >= SESSION_PAIRING_TIMEOUT {
            return Err(StatusCode::GONE);
        }
        let outcome = if let Some(tx) = waiting.notify.take() {
            waiting.active = true;
            PairingOutcome::HandOff(tx)
        } else {
            let (tx, rx) = oneshot::channel();
            waiting.kind = Some(kind);
            waiting.notify = Some(tx);
            PairingOutcome::Waiting { rx }
        };
        Ok((waiting.generation, outcome))
    }

    fn remove_pending(&self, session_id: &str, generation: u64) {
        let mut sessions = self.inner.pending_sessions.lock();
        if sessions
            .get(session_id)
            .is_some_and(|entry| entry.generation == generation)
        {
            sessions.remove(session_id);
        }
    }

    /// Sweeps the pending-session registry, dropping any entry that has
    /// been waiting longer than [`SESSION_PAIRING_TIMEOUT`]. Dropping the
    /// entry drops its `notify` sender, which causes the waiting task's
    /// `rx.await` to resolve to an error so it can close its socket.
    fn sweep_expired_sessions(&self) {
        let mut sessions = self.inner.pending_sessions.lock();
        sessions.retain(|_, waiting| {
            waiting.active || waiting.created_at.elapsed() < SESSION_PAIRING_TIMEOUT
        });
    }
}

/// Builds the Axum router exposing the three tunnel endpoints.
pub fn relay_router(state: RelayState) -> Router {
    Router::new()
        .route("/tunnel/control", get(control_handler))
        .route("/tunnel/data/{session_id}", get(data_handler))
        .route("/tunnel/client/{session_id}", get(client_handler))
        .with_state(state)
}

/// Spawns the background task that periodically evicts sessions which
/// never paired within [`SESSION_PAIRING_TIMEOUT`].
pub fn spawn_session_reaper(state: RelayState) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(SESSION_SWEEP_INTERVAL).await;
            state.sweep_expired_sessions();
        }
    });
}

fn extract_bearer_token(headers: &HeaderMap, query_token: Option<&str>) -> Option<String> {
    if let Some(value) = headers.get(axum::http::header::AUTHORIZATION) {
        if let Ok(text) = value.to_str() {
            if let Some(token) = text.strip_prefix("Bearer ") {
                return Some(token.to_string());
            }
        }
    }
    query_token.map(|s| s.to_string())
}

#[derive(Debug, Deserialize)]
struct AuthQuery {
    token: Option<String>,
}

/// `GET /tunnel/control` - the desktop daemon's long-lived control channel.
///
/// The daemon authenticates with its Machine Token (via `Authorization:
/// Bearer <token>` header or `?token=` query parameter). Once upgraded, the
/// relay forwards [`IncomingSessionNotice`] messages to the daemon as JSON
/// text frames for as long as the socket remains open.
async fn control_handler(
    ws: WebSocketUpgrade,
    axum::extract::Query(query): axum::extract::Query<AuthQuery>,
    headers: HeaderMap,
    State(state): State<RelayState>,
) -> Result<Response, (StatusCode, String)> {
    let token = extract_bearer_token(&headers, query.token.as_deref())
        .ok_or((StatusCode::UNAUTHORIZED, "Missing Machine Token".into()))?;
    if !state.validate_machine_token(&token) {
        return Err((StatusCode::UNAUTHORIZED, "Invalid Machine Token".into()));
    }

    Ok(ws
        .max_message_size(MAX_MESSAGE_SIZE)
        .max_frame_size(MAX_MESSAGE_SIZE)
        .on_upgrade(move |socket| handle_control_socket(socket, state, token)))
}

async fn handle_control_socket(mut socket: WebSocket, state: RelayState, machine_token: String) {
    let (generation, mut notices) = state.register_control_channel(machine_token.clone());
    loop {
        tokio::select! {
            notice = notices.recv() => {
                let Some(notice) = notice else { break };
                let Ok(payload) = serde_json::to_string(&notice) else { continue };
                if !matches!(timeout(TRANSFER_TIMEOUT, socket.send(Message::Text(payload.into()))).await, Ok(Ok(()))) {
                    tracing::warn!("relay control send failed or timed out");
                    break;
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<ControlRequest>(&text) {
                            Ok(ControlRequest::AllocateSession) => {
                                let id = uuid::Uuid::new_v4().to_string();
                                if !state.issue_session(&machine_token, &id, Some(generation)) {
                                    tracing::warn!("relay session allocation rejected");
                                    break;
                                }
                            }
                            Err(error) => tracing::warn!(%error, "invalid relay control request"),
                        }
                    }
                    Some(Ok(_)) => {}
                    Some(Err(error)) => {
                        tracing::warn!(%error, "relay control receive failed");
                        break;
                    }
                }
            }
        }
    }
    state.unregister_control_channel(&machine_token, generation);
}

/// `GET /tunnel/data/:session_id` - the desktop daemon's data channel for a
/// specific session, opened after receiving a control-channel notification.
async fn data_handler(
    ws: WebSocketUpgrade,
    AxumPath(session_id): AxumPath<String>,
    State(state): State<RelayState>,
) -> Result<Response, StatusCode> {
    upgrade_half(ws, state, session_id, HalfKind::Data)
}

/// `GET /tunnel/client/:session_id` - the remote client's channel for a
/// specific session.
async fn client_handler(
    ws: WebSocketUpgrade,
    AxumPath(session_id): AxumPath<String>,
    State(state): State<RelayState>,
) -> Result<Response, StatusCode> {
    upgrade_half(ws, state, session_id, HalfKind::Client)
}

fn upgrade_half(
    ws: WebSocketUpgrade,
    state: RelayState,
    session_id: String,
    kind: HalfKind,
) -> Result<Response, StatusCode> {
    let (generation, outcome) = state.reserve_half(&session_id, kind)?;
    let guard = SessionGuard {
        armed: true,
        state,
        session_id,
        generation,
    };
    Ok(ws
        .max_message_size(MAX_MESSAGE_SIZE)
        .max_frame_size(MAX_MESSAGE_SIZE)
        .on_upgrade(move |socket| handle_half_socket(socket, guard, outcome)))
}

/// Registers `socket` as one half of `session_id`'s pairing. If the
/// opposite half is already waiting, hands `socket` to it directly and
/// returns immediately (the other task drives the proxy). Otherwise waits
/// (bounded by [`SESSION_PAIRING_TIMEOUT`]) for the opposite half to show
/// up, then proxies frames bidirectionally between the two sockets until
/// either side disconnects.
async fn handle_half_socket(
    mut socket: WebSocket,
    mut guard: SessionGuard,
    outcome: PairingOutcome,
) {
    match outcome {
        PairingOutcome::HandOff(tx) => {
            if tx.send(socket).is_ok() {
                // The waiting task owns cleanup after a successful transfer.
                guard.armed = false;
            } else {
                tracing::warn!("relay counterpart disconnected before handoff");
            }
        }
        PairingOutcome::Waiting { mut rx } => {
            let transfer = async {
                let mut buffered = Vec::new();
                let mut bytes = 0;
                let pairing = async {
                    loop {
                        tokio::select! {
                            peer = &mut rx => return peer.map_err(anyhow::Error::from),
                            incoming = socket.recv() => {
                                match incoming {
                                    Some(Ok(Message::Close(_))) | None => anyhow::bail!("pending peer disconnected"),
                                    Some(Err(error)) => return Err(error.into()),
                                    Some(Ok(message)) => {
                                        bytes += match &message {
                                            Message::Text(text) => text.len(),
                                            Message::Binary(data) | Message::Ping(data) | Message::Pong(data) => data.len(),
                                            Message::Close(_) => 0,
                                        };
                                        if bytes > MAX_MESSAGE_SIZE || buffered.len() >= 100 {
                                            anyhow::bail!("pending frame buffer limit exceeded");
                                        }
                                        buffered.push(message);
                                    }
                                }
                            }
                        }
                    }
                };
                let mut peer = timeout(SESSION_PAIRING_TIMEOUT, pairing).await??;
                for message in buffered {
                    timeout(TRANSFER_TIMEOUT, peer.send(message)).await??;
                }
                proxy_sockets(socket, peer).await
            };
            match timeout(SESSION_TRANSFER_TIMEOUT, transfer).await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => tracing::warn!(%error, "relay session failed"),
                Err(error) => tracing::warn!(%error, "relay session lifetime exceeded"),
            }
            drop(guard);
        }
    }
}

/// Proxies WebSocket frames bidirectionally between `a` and `b` until
/// either side closes or errors.
async fn proxy_sockets(a: WebSocket, b: WebSocket) -> anyhow::Result<()> {
    let (mut a_tx, mut a_rx) = a.split();
    let (mut b_tx, mut b_rx) = b.split();

    let a_to_b = async {
        while let Some(msg) = a_rx.next().await {
            let msg = msg?;
            let is_close = matches!(msg, Message::Close(_));
            timeout(TRANSFER_TIMEOUT, b_tx.send(msg)).await??;
            if is_close {
                break;
            }
        }
        Ok::<(), anyhow::Error>(())
    };
    let b_to_a = async {
        while let Some(msg) = b_rx.next().await {
            let msg = msg?;
            let is_close = matches!(msg, Message::Close(_));
            timeout(TRANSFER_TIMEOUT, a_tx.send(msg)).await??;
            if is_close {
                break;
            }
        }
        Ok::<(), anyhow::Error>(())
    };

    tokio::select! {
        result = a_to_b => result,
        result = b_to_a => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::Message as TMessage;

    /// Starts an in-process relay server bound to an ephemeral loopback
    /// port and returns its base `ws://` URL along with a handle that
    /// keeps the server task alive for the duration of the test.
    async fn spawn_test_relay() -> (String, tokio::task::JoinHandle<()>) {
        let state = RelayState::new(vec!["test-machine-token".to_string()]);
        let router = relay_router(state);

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ephemeral port");
        let addr = listener.local_addr().expect("local addr");

        let handle = tokio::spawn(async move {
            axum::serve(listener, router)
                .await
                .expect("relay server exited unexpectedly");
        });

        (format!("ws://{addr}"), handle)
    }

    /// End-to-end integration test: starts a real relay server on an
    /// ephemeral port, connects a simulated "data" channel (standing in
    /// for the desktop daemon's side of the tunnel) and a "client"
    /// channel to the same session id, and verifies that frames sent from
    /// either side are proxied through to the other - i.e. the relay's
    /// reverse-tunnel multiplexing bridges the two independently-opened
    /// WebSocket connections into one bidirectional pipe.
    #[tokio::test]
    async fn test_relay_reverse_tunnel_multiplex() {
        let (base_url, _server) = spawn_test_relay().await;
        let (mut control, _) = tokio_tungstenite::connect_async(format!(
            "{base_url}/tunnel/control?token=test-machine-token"
        ))
        .await
        .unwrap();
        control
            .send(TMessage::Text(r#"{"type":"AllocateSession"}"#.into()))
            .await
            .unwrap();
        let notice = timeout(Duration::from_secs(5), control.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let notice: IncomingSessionNotice =
            serde_json::from_str(notice.to_text().unwrap()).unwrap();
        let session_id = notice.session_id;

        let data_url = format!("{base_url}/tunnel/data/{session_id}");
        let client_url = format!("{base_url}/tunnel/client/{session_id}");

        // Open the "data" half first (simulating the desktop daemon
        // opening its data channel in response to a control-channel
        // notification). It will sit in the pending registry until the
        // client half connects.
        let (mut data_socket, _) = tokio_tungstenite::connect_async(&data_url)
            .await
            .expect("data channel should connect");

        // Now open the "client" half for the same session id. This should
        // be paired with the waiting data half and the relay should start
        // proxying frames between them.
        let (mut client_socket, _) = tokio_tungstenite::connect_async(&client_url)
            .await
            .expect("client channel should connect");

        for url in [&data_url, &client_url] {
            let error = tokio_tungstenite::connect_async(url).await.unwrap_err();
            assert!(
                matches!(error, tokio_tungstenite::tungstenite::Error::Http(response)
                if response.status() == StatusCode::CONFLICT)
            );
        }

        // client -> data
        client_socket
            .send(TMessage::Text("hello from client".into()))
            .await
            .expect("client send should succeed");

        let received = tokio::time::timeout(Duration::from_secs(5), data_socket.next())
            .await
            .expect("data side should receive client frame before timeout")
            .expect("data socket stream should not end")
            .expect("data socket frame should not error");
        match received {
            TMessage::Text(text) => assert_eq!(text, "hello from client"),
            other => panic!("expected text frame relayed from client, got {other:?}"),
        }

        // data -> client (bidirectional echo path back the other way)
        data_socket
            .send(TMessage::Text("hello from data".into()))
            .await
            .expect("data send should succeed");

        let received = tokio::time::timeout(Duration::from_secs(5), client_socket.next())
            .await
            .expect("client side should receive data frame before timeout")
            .expect("client socket stream should not end")
            .expect("client socket frame should not error");
        match received {
            TMessage::Text(text) => assert_eq!(text, "hello from data"),
            other => panic!("expected text frame relayed from data channel, got {other:?}"),
        }

        // Round-trip a second message each way to confirm the bridge
        // stays open for more than a single exchange (i.e. it is a
        // genuine bidirectional proxy, not a one-shot handoff).
        client_socket
            .send(TMessage::Text("second client message".into()))
            .await
            .expect("second client send should succeed");
        let received = tokio::time::timeout(Duration::from_secs(5), data_socket.next())
            .await
            .expect("data side should receive second client frame before timeout")
            .expect("data socket stream should not end")
            .expect("data socket frame should not error");
        match received {
            TMessage::Text(text) => assert_eq!(text, "second client message"),
            other => panic!("expected second text frame relayed from client, got {other:?}"),
        }

        let _ = client_socket.close(None).await;
        let _ = data_socket.close(None).await;
        _server.abort();
    }

    #[tokio::test]
    async fn test_relay_unissued_session_rejected() {
        let (base, server) = spawn_test_relay().await;
        for half in ["data", "client"] {
            let error = tokio_tungstenite::connect_async(format!("{base}/tunnel/{half}/unissued"))
                .await
                .unwrap_err();
            assert!(
                matches!(error, tokio_tungstenite::tungstenite::Error::Http(response)
                if response.status() == StatusCode::NOT_FOUND)
            );
        }
        server.abort();
    }

    #[test]
    fn test_relay_generation_safe_cleanup_and_limits() {
        let state = RelayState::new(vec!["tok".into()]);
        assert!(!state.notify_incoming_session("tok", "missing-control"));
        let (old, _old_rx) = state.register_control_channel("tok".into());
        let (new, mut rx) = state.register_control_channel("tok".into());
        state.unregister_control_channel("tok", old);
        assert!(!state.issue_session("tok", "stale", Some(old)));
        assert!(state.notify_incoming_session("tok", "session"));
        rx.try_recv().unwrap();
        let (generation, waiting) = state.reserve_half("session", HalfKind::Data).unwrap();
        assert!(matches!(
            state.reserve_half("session", HalfKind::Data),
            Err(StatusCode::CONFLICT)
        ));
        state.remove_pending("session", generation);
        drop(waiting);
        assert!(state.notify_incoming_session("tok", "session"));
        rx.try_recv().unwrap();
        state.remove_pending("session", generation);
        let (_, waiting) = state.reserve_half("session", HalfKind::Client).unwrap();
        let (_, handoff) = state.reserve_half("session", HalfKind::Data).unwrap();
        assert!(matches!(
            state.reserve_half("session", HalfKind::Client),
            Err(StatusCode::CONFLICT)
        ));
        for n in 1..MAX_PENDING_SESSIONS {
            assert!(state.notify_incoming_session("tok", &format!("session-{n}")));
            rx.try_recv().unwrap();
        }
        assert!(!state.notify_incoming_session("tok", "overflow"));
        state.unregister_control_channel("tok", new);
        assert!(!state.notify_incoming_session("tok", "no-control"));
        drop((waiting, handoff));
    }
}
