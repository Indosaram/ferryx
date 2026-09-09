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
//! re-present the Machine Token.

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
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;

/// How long a session may sit waiting for its data or client half before
/// the relay gives up and evicts it from the registry.
const SESSION_PAIRING_TIMEOUT: Duration = Duration::from_secs(30);

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

/// A half that arrived at the registry before its counterpart, waiting to
/// be paired. `notify` is fired with the *counterpart's* socket once one
/// arrives, so the waiting task can resume and start proxying.
struct WaitingHalf {
    kind: HalfKind,
    created_at: Instant,
    notify: oneshot::Sender<WebSocket>,
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
    control_channels: Mutex<HashMap<String, mpsc::UnboundedSender<IncomingSessionNotice>>>,
    pending_sessions: Mutex<HashMap<String, WaitingHalf>>,
}

/// Outcome of offering a socket to the pairing registry.
enum PairingOutcome {
    /// A counterpart of the opposite kind was already waiting; it has been
    /// handed this call's socket and will drive the proxy itself. This
    /// call is done.
    HandedOff,
    /// No differently-kinded counterpart was waiting (or it was gone).
    /// This call's socket is now the registered waiting half; the caller
    /// keeps driving it and awaits `rx` for the counterpart's socket once
    /// one arrives.
    Waiting {
        own_socket: WebSocket,
        rx: oneshot::Receiver<WebSocket>,
    },
}

impl RelayState {
    pub fn new(machine_tokens: Vec<String>) -> Self {
        Self {
            inner: Arc::new(RelayInner {
                machine_tokens,
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
    ) -> mpsc::UnboundedReceiver<IncomingSessionNotice> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.inner
            .control_channels
            .lock()
            .insert(machine_token, tx);
        rx
    }

    fn unregister_control_channel(&self, machine_token: &str) {
        self.inner.control_channels.lock().remove(machine_token);
    }

    /// Notifies the connected daemon holding `machine_token`'s control
    /// channel that a new session has been requested. Returns `true` if a
    /// control channel accepted the notification.
    #[allow(dead_code)]
    pub fn notify_incoming_session(&self, machine_token: &str, session_id: &str) -> bool {
        let channels = self.inner.control_channels.lock();
        match channels.get(machine_token) {
            Some(tx) => tx
                .send(IncomingSessionNotice {
                    session_id: session_id.to_string(),
                })
                .is_ok(),
            None => false,
        }
    }

    /// Offers `socket` as the given `kind` of half for `session_id`.
    ///
    /// If a differently-kinded half is already registered and waiting,
    /// this removes it from the registry, hands it `socket` directly (it
    /// will resume and proxy), and returns [`PairingOutcome::HandedOff`].
    ///
    /// Otherwise this call's own socket becomes the new waiting half. The
    /// caller must keep driving `socket` itself and wait on the returned
    /// receiver for the eventual counterpart.
    fn offer_half(&self, session_id: &str, kind: HalfKind, socket: WebSocket) -> PairingOutcome {
        let mut sessions = self.inner.pending_sessions.lock();

        let mut socket = socket;
        if let Some(existing) = sessions.get(session_id) {
            if existing.kind != kind {
                let waiting = sessions.remove(session_id).expect("just matched above");
                match waiting.notify.send(socket) {
                    Ok(()) => return PairingOutcome::HandedOff,
                    Err(returned_socket) => {
                        // The waiting task's receiver was already dropped
                        // (e.g. it timed out right as we tried to hand
                        // off); recover our socket and fall through to
                        // register as the new waiting half instead.
                        socket = returned_socket;
                    }
                }
            }
        }

        let (tx, rx) = oneshot::channel();
        sessions.insert(
            session_id.to_string(),
            WaitingHalf {
                kind,
                created_at: Instant::now(),
                notify: tx,
            },
        );
        PairingOutcome::Waiting {
            own_socket: socket,
            rx,
        }
    }

    fn remove_pending(&self, session_id: &str) {
        self.inner.pending_sessions.lock().remove(session_id);
    }

    /// Sweeps the pending-session registry, dropping any entry that has
    /// been waiting longer than [`SESSION_PAIRING_TIMEOUT`]. Dropping the
    /// entry drops its `notify` sender, which causes the waiting task's
    /// `rx.await` to resolve to an error so it can close its socket.
    fn sweep_expired_sessions(&self) {
        let mut sessions = self.inner.pending_sessions.lock();
        sessions.retain(|_, waiting| waiting.created_at.elapsed() < SESSION_PAIRING_TIMEOUT);
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

    Ok(ws.on_upgrade(move |socket| handle_control_socket(socket, state, token)))
}

async fn handle_control_socket(mut socket: WebSocket, state: RelayState, machine_token: String) {
    let mut notices = state.register_control_channel(machine_token.clone());
    loop {
        tokio::select! {
            notice = notices.recv() => {
                let Some(notice) = notice else { break };
                let Ok(payload) = serde_json::to_string(&notice) else { continue };
                if socket.send(Message::Text(payload.into())).await.is_err() {
                    break;
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {
                        // Control channel only carries relay-to-daemon
                        // notifications; any inbound frame (e.g. a
                        // keepalive ping payload) is simply ignored.
                    }
                    Some(Err(_)) => break,
                }
            }
        }
    }
    state.unregister_control_channel(&machine_token);
}

/// `GET /tunnel/data/:session_id` - the desktop daemon's data channel for a
/// specific session, opened after receiving a control-channel notification.
async fn data_handler(
    ws: WebSocketUpgrade,
    AxumPath(session_id): AxumPath<String>,
    State(state): State<RelayState>,
) -> Response {
    ws.on_upgrade(move |socket| handle_half_socket(socket, state, session_id, HalfKind::Data))
}

/// `GET /tunnel/client/:session_id` - the remote client's channel for a
/// specific session.
async fn client_handler(
    ws: WebSocketUpgrade,
    AxumPath(session_id): AxumPath<String>,
    State(state): State<RelayState>,
) -> Response {
    ws.on_upgrade(move |socket| handle_half_socket(socket, state, session_id, HalfKind::Client))
}

/// Registers `socket` as one half of `session_id`'s pairing. If the
/// opposite half is already waiting, hands `socket` to it directly and
/// returns immediately (the other task drives the proxy). Otherwise waits
/// (bounded by [`SESSION_PAIRING_TIMEOUT`]) for the opposite half to show
/// up, then proxies frames bidirectionally between the two sockets until
/// either side disconnects.
async fn handle_half_socket(
    socket: WebSocket,
    state: RelayState,
    session_id: String,
    kind: HalfKind,
) {
    match state.offer_half(&session_id, kind, socket) {
        PairingOutcome::HandedOff => {
            // The counterpart that was already waiting received our
            // socket over its own oneshot and will drive the proxy;
            // nothing left to do here.
        }
        PairingOutcome::Waiting { own_socket, rx } => {
            match timeout(SESSION_PAIRING_TIMEOUT, rx).await {
                Ok(Ok(peer_socket)) => {
                    proxy_sockets(own_socket, peer_socket).await;
                }
                Ok(Err(_)) | Err(_) => {
                    // Either the registry entry was reaped (sender
                    // dropped) or we hit our own bound waiting for a
                    // counterpart that never arrived. Either way, make
                    // sure the (now stale) registry entry is gone and
                    // close our socket.
                    state.remove_pending(&session_id);
                    let _ = own_socket;
                }
            }
        }
    }
}

/// Proxies WebSocket frames bidirectionally between `a` and `b` until
/// either side closes or errors.
async fn proxy_sockets(a: WebSocket, b: WebSocket) {
    let (mut a_tx, mut a_rx) = a.split();
    let (mut b_tx, mut b_rx) = b.split();

    let a_to_b = async {
        while let Some(Ok(msg)) = a_rx.next().await {
            let is_close = matches!(msg, Message::Close(_));
            if b_tx.send(msg).await.is_err() || is_close {
                break;
            }
        }
        let _ = b_tx.close().await;
    };
    let b_to_a = async {
        while let Some(Ok(msg)) = b_rx.next().await {
            let is_close = matches!(msg, Message::Close(_));
            if a_tx.send(msg).await.is_err() || is_close {
                break;
            }
        }
        let _ = a_tx.close().await;
    };

    tokio::join!(a_to_b, b_to_a);
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::{SinkExt as _, StreamExt as _};
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
        let session_id = "integration-test-session";

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
    }
}
