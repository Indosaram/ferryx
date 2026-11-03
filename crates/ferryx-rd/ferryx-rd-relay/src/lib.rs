use std::{
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        ConnectInfo, Path, State,
    },
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use dashmap::DashMap;
use futures_util::{SinkExt, StreamExt};
use ferryx_rd_proto::{
    decode_relay_frame, relay_host_id_ok, relay_token_matches, RelayControl,
    RELAY_FRAME_HEADER_LEN, RELAY_MAX_PAYLOAD_LEN, RELAY_MAX_SESSIONS_PER_HOST,
};
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

const DEFAULT_ACCEPT_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_IDLE: Duration = Duration::from_secs(60);
const DEFAULT_HEARTBEAT: Duration = Duration::from_secs(30);
const DEFAULT_SESSION_LIMIT: Duration = Duration::from_secs(24 * 60 * 60);
const DEFAULT_CONNECTS_PER_MINUTE: u32 = 100;

#[derive(Debug, Error)]
pub enum RelayError {
    #[error("RELAY_AUTH_SECRET is missing or empty")]
    MissingSecret,
}

#[derive(Clone)]
pub struct RelayConfig {
    pub auth_secret: Vec<u8>,
    pub max_sessions_per_host: usize,
    pub max_payload_len: usize,
    pub idle_timeout: Duration,
    pub heartbeat: Duration,
    pub max_session_duration: Duration,
    pub connects_per_minute: u32,
    pub accept_timeout: Duration,
}

impl RelayConfig {
    pub fn from_secret(secret: Vec<u8>) -> Self {
        Self {
            auth_secret: secret,
            max_sessions_per_host: RELAY_MAX_SESSIONS_PER_HOST,
            max_payload_len: RELAY_MAX_PAYLOAD_LEN,
            idle_timeout: DEFAULT_IDLE,
            heartbeat: DEFAULT_HEARTBEAT,
            max_session_duration: DEFAULT_SESSION_LIMIT,
            connects_per_minute: DEFAULT_CONNECTS_PER_MINUTE,
            accept_timeout: DEFAULT_ACCEPT_TIMEOUT,
        }
    }
}

pub fn config_from_secret(secret: Option<&str>) -> Result<RelayConfig, RelayError> {
    match secret {
        Some(value) if !value.is_empty() => Ok(RelayConfig::from_secret(value.as_bytes().to_vec())),
        _ => Err(RelayError::MissingSecret),
    }
}

pub fn config_from_env() -> Result<RelayConfig, RelayError> {
    config_from_secret(std::env::var("RELAY_AUTH_SECRET").ok().as_deref())
}

#[derive(Clone)]
struct AppState {
    inner: Arc<StateInner>,
}

struct StateInner {
    config: RelayConfig,
    hosts: DashMap<String, mpsc::UnboundedSender<Message>>,
    sessions: DashMap<String, Session>,
    rates: DashMap<IpAddr, Vec<Instant>>,
}

struct Session {
    host_id: String,
    client_tx: mpsc::UnboundedSender<Message>,
    accept_tx: Mutex<Option<oneshot::Sender<()>>>,
    created: Instant,
}

impl AppState {
    fn new(config: RelayConfig) -> Self {
        Self {
            inner: Arc::new(StateInner {
                config,
                hosts: DashMap::new(),
                sessions: DashMap::new(),
                rates: DashMap::new(),
            }),
        }
    }

    fn allow_connect(&self, ip: IpAddr) -> bool {
        let mut hits = self.inner.rates.entry(ip).or_default();
        let now = Instant::now();
        hits.retain(|stamp| now.saturating_duration_since(*stamp) < Duration::from_secs(60));
        if hits.len() as u32 >= self.inner.config.connects_per_minute {
            return false;
        }
        hits.push(now);
        true
    }

    fn session_count(&self, host_id: &str) -> usize {
        self.inner
            .sessions
            .iter()
            .filter(|entry| entry.host_id == host_id)
            .count()
    }

    fn drop_host(&self, host_id: &str) {
        self.inner.hosts.remove(host_id);
        let doomed: Vec<String> = self
            .inner
            .sessions
            .iter()
            .filter(|entry| entry.host_id == host_id)
            .map(|entry| entry.key().clone())
            .collect();
        for id in doomed {
            self.inner.sessions.remove(&id);
        }
    }
}

pub fn router(config: RelayConfig) -> Router {
    let state = AppState::new(config);
    Router::new()
        .route("/health", get(health))
        .route("/register", get(register))
        .route("/connect/{host_id}", get(connect_route))
        .with_state(state)
}

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok" }))
}

async fn register(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.max_message_size(state.inner.config.max_payload_len + RELAY_FRAME_HEADER_LEN + 64)
        .on_upgrade(move |socket| host_socket(socket, state))
}

async fn connect_route(
    ws: WebSocketUpgrade,
    Path(host_id): Path<String>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let limit = state.inner.config.max_payload_len + RELAY_FRAME_HEADER_LEN + 64;
    ws.max_message_size(limit)
        .on_upgrade(move |socket| client_socket(socket, state, host_id, peer.ip()))
}

fn text_message(control: &RelayControl) -> Result<Message, serde_json::Error> {
    Ok(Message::Text(serde_json::to_string(control)?.into()))
}

fn error_message(code: &str, message: &str) -> Message {
    let control = RelayControl::Error {
        code: code.to_owned(),
        message: message.to_owned(),
    };
    text_message(&control).unwrap_or(Message::Close(None))
}

async fn host_socket(mut socket: WebSocket, state: AppState) {
    let first = match socket.recv().await {
        Some(Ok(Message::Text(text))) => text.to_string(),
        _ => {
            let _ = socket
                .send(error_message("unauthorized", "expected register"))
                .await;
            return;
        }
    };
    let Ok(RelayControl::Register { host_id, token }) = serde_json::from_str(&first) else {
        let _ = socket
            .send(error_message("unauthorized", "expected register"))
            .await;
        return;
    };
    if !relay_host_id_ok(&host_id)
        || !relay_token_matches(&state.inner.config.auth_secret, &host_id, &token)
    {
        let _ = socket
            .send(error_message("unauthorized", "invalid token"))
            .await;
        return;
    }
    let registered = match text_message(&RelayControl::Registered {
        host_id: host_id.clone(),
    }) {
        Ok(message) => message,
        Err(_) => return,
    };
    if socket.send(registered).await.is_err() {
        return;
    }

    let (tx, mut rx) = mpsc::unbounded_channel();
    state.drop_host(&host_id);
    state.inner.hosts.insert(host_id.clone(), tx.clone());
    tracing::info!(%host_id, "relay: host registered");
    let (mut sink, mut stream) = socket.split();
    let writer = tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            if sink.send(message).await.is_err() {
                break;
            }
        }
    });

    let idle = state.inner.config.idle_timeout;
    let mut last = tokio::time::Instant::now();
    let mut ping = tokio::time::interval(state.inner.config.heartbeat);
    ping.tick().await;
    loop {
        tokio::select! {
            incoming = stream.next() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        last = tokio::time::Instant::now();
                        handle_host_text(&state, &host_id, text.as_str(), &tx);
                    }
                    Some(Ok(Message::Binary(bytes))) => {
                        last = tokio::time::Instant::now();
                        if !forward_from_host(&state, &host_id, bytes.as_ref(), &tx) {
                            break;
                        }
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        last = tokio::time::Instant::now();
                        let _ = tx.send(Message::Pong(payload));
                    }
                    Some(Ok(Message::Pong(_))) => last = tokio::time::Instant::now(),
                    _ => break,
                }
            }
            _ = ping.tick() => {
                if let Ok(message) = text_message(&RelayControl::Ping) {
                    let _ = tx.send(message);
                }
            }
            _ = tokio::time::sleep_until(last + idle) => break,
        }
    }
    state.drop_host(&host_id);
    drop(tx);
    let _ = writer.await;
}

fn handle_host_text(
    state: &AppState,
    host_id: &str,
    text: &str,
    tx: &mpsc::UnboundedSender<Message>,
) {
    let Ok(control) = serde_json::from_str::<RelayControl>(text) else {
        return;
    };
    match control {
        RelayControl::Pong | RelayControl::Ping => {}
        RelayControl::Accept { session_id } => {
            let sender = {
                let Some(session) = state.inner.sessions.get(&session_id) else {
                    return;
                };
                if session.host_id != host_id {
                    return;
                }
                session
                    .accept_tx
                    .lock()
                    .ok()
                    .and_then(|mut guard| guard.take())
            };
            if let Some(sender) = sender {
                let _ = sender.send(());
            }
        }
        _ => {
            let _ = tx.send(error_message("bad_frame", "unexpected host control"));
        }
    }
}

fn forward_from_host(
    state: &AppState,
    host_id: &str,
    bytes: &[u8],
    host_tx: &mpsc::UnboundedSender<Message>,
) -> bool {
    let frame = match decode_relay_frame(bytes) {
        Ok(frame) => frame,
        Err(_) => {
            let _ = host_tx.send(error_message("bad_frame", "invalid frame"));
            return false;
        }
    };
    if frame.payload.len() > state.inner.config.max_payload_len {
        let _ = host_tx.send(error_message("bad_frame", "payload too large"));
        return false;
    }
    let Ok(id) = Uuid::from_slice(&frame.session_id) else {
        return true;
    };
    let key = id.to_string();
    let client_tx = {
        let Some(session) = state.inner.sessions.get(&key) else {
            return true;
        };
        if session.host_id != host_id {
            return true;
        }
        if Instant::now().saturating_duration_since(session.created)
            > state.inner.config.max_session_duration
        {
            drop(session);
            state.inner.sessions.remove(&key);
            let _ = host_tx.send(error_message("session_expired", "session expired"));
            return true;
        }
        session.client_tx.clone()
    };
    tracing::debug!(host_id, %key, len = bytes.len(), "relay: host uplink bytes");
    let _ = client_tx.send(Message::Binary(bytes.to_vec().into()));
    true
}

async fn client_socket(mut socket: WebSocket, state: AppState, host_id: String, ip: IpAddr) {
    if !state.allow_connect(ip) {
        let _ = socket
            .send(error_message("rate_limited", "too many connects"))
            .await;
        return;
    }
    if !state.inner.hosts.contains_key(&host_id) {
        let _ = socket
            .send(error_message("not_found", "host is not registered"))
            .await;
        return;
    }
    let first = match socket.recv().await {
        Some(Ok(Message::Text(text))) => text.to_string(),
        _ => {
            let _ = socket
                .send(error_message("unauthorized", "expected connect"))
                .await;
            return;
        }
    };
    let Ok(RelayControl::Connect {
        host_id: claimed,
        token,
    }) = serde_json::from_str(&first)
    else {
        let _ = socket
            .send(error_message("unauthorized", "expected connect"))
            .await;
        return;
    };
    if claimed != host_id || !relay_token_matches(&state.inner.config.auth_secret, &host_id, &token)
    {
        let _ = socket
            .send(error_message("unauthorized", "invalid token"))
            .await;
        return;
    }
    if state.session_count(&host_id) >= state.inner.config.max_sessions_per_host {
        let _ = socket
            .send(error_message("session_limit", "too many sessions"))
            .await;
        return;
    }
    let session_uuid = Uuid::new_v4();
    let session_id = session_uuid.to_string();
    let (accept_tx, accept_rx) = oneshot::channel();
    let (client_tx, mut client_rx) = mpsc::unbounded_channel();
    state.inner.sessions.insert(
        session_id.clone(),
        Session {
            host_id: host_id.clone(),
            client_tx,
            accept_tx: Mutex::new(Some(accept_tx)),
            created: Instant::now(),
        },
    );
    let incoming = match text_message(&RelayControl::Incoming {
        session_id: session_id.clone(),
    }) {
        Ok(message) => message,
        Err(_) => {
            state.inner.sessions.remove(&session_id);
            return;
        }
    };
    let host_tx = state.inner.hosts.get(&host_id).map(|entry| entry.clone());
    let Some(host_tx) = host_tx else {
        state.inner.sessions.remove(&session_id);
        let _ = socket
            .send(error_message("not_found", "host is not registered"))
            .await;
        return;
    };
    if host_tx.send(incoming).is_err() {
        state.inner.sessions.remove(&session_id);
        let _ = socket
            .send(error_message("not_found", "host is not registered"))
            .await;
        return;
    }
    tracing::info!(%host_id, %session_id, "relay: incoming sent to host");
    let accepted = tokio::time::timeout(state.inner.config.accept_timeout, accept_rx).await;
    if accepted.is_err() || accepted.ok().and_then(Result::ok).is_none() {
        state.inner.sessions.remove(&session_id);
        let _ = socket
            .send(error_message("not_found", "host did not accept"))
            .await;
        return;
    }
    let connected = match text_message(&RelayControl::Connected) {
        Ok(message) => message,
        Err(_) => {
            state.inner.sessions.remove(&session_id);
            return;
        }
    };
    tracing::info!(%host_id, %session_id, "relay: session connected");
    if socket.send(connected).await.is_err() {
        state.inner.sessions.remove(&session_id);
        return;
    }

    let (mut sink, mut stream) = socket.split();
    let session_bytes = *session_uuid.as_bytes();
    let max_payload = state.inner.config.max_payload_len;
    let mut last = tokio::time::Instant::now();
    loop {
        tokio::select! {
            incoming = stream.next() => {
                match incoming {
                    Some(Ok(Message::Binary(bytes))) => {
                        last = tokio::time::Instant::now();
                        tracing::debug!(%session_id, len = bytes.len(), "relay: client bytes");
                        if !forward_from_client(&state, &host_id, &session_id, &session_bytes, bytes.as_ref(), max_payload, &host_tx) {
                            tracing::warn!(%session_id, "relay: closing client ws after bad frame");
                            let _ = sink.send(error_message("bad_frame", "invalid frame")).await;
                            break;
                        }
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        let _ = sink.send(Message::Pong(payload)).await;
                    }
                    Some(Ok(Message::Pong(_))) | Some(Ok(Message::Text(_))) => {}
                    _ => break,
                }
            }
            outgoing = client_rx.recv() => {
                match outgoing {
                    Some(message) => {
                        if sink.send(message).await.is_err() {
                            tracing::warn!(%session_id, "relay: client sink send failed");
                            break;
                        }
                    }
                    None => {
                        tracing::warn!(%session_id, "relay: client_rx closed");
                        break;
                    }
                }
            }
            _ = tokio::time::sleep_until(last + state.inner.config.idle_timeout) => {
                tracing::warn!(%session_id, "relay: client ws idle timeout");
                break;
            }
        }
    }
    tracing::info!(%session_id, "relay: client ws loop exited");
    state.inner.sessions.remove(&session_id);
}

fn forward_from_client(
    state: &AppState,
    host_id: &str,
    session_id: &str,
    session_bytes: &[u8; 16],
    bytes: &[u8],
    max_payload: usize,
    host_tx: &mpsc::UnboundedSender<Message>,
) -> bool {
    let Ok(frame) = decode_relay_frame(bytes) else {
        return false;
    };
    if frame.payload.len() > max_payload {
        return false;
    }
    let stamped;
    let bytes = if frame.session_id == [0_u8; 16] {
        stamped = {
            let mut owned = bytes.to_vec();
            owned[..16].copy_from_slice(session_bytes);
            owned
        };
        stamped.as_slice()
    } else if frame.session_id != *session_bytes {
        return false;
    } else {
        bytes
    };
    if !state.inner.sessions.contains_key(session_id) {
        return false;
    }
    let Some(live_host) = state.inner.hosts.get(host_id) else {
        return false;
    };
    if !mpsc::UnboundedSender::same_channel(&live_host, host_tx) {
        return false;
    }
    drop(live_host);
    host_tx.send(Message::Binary(bytes.to_vec().into())).is_ok()
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use axum::extract::connect_info::IntoMakeServiceWithConnectInfo;
    use futures_util::{SinkExt, StreamExt};
    use ferryx_rd_proto::{
        encode_relay_frame, relay_token, RelayControl, RELAY_CHANNEL_TCP, RELAY_CHANNEL_UDP,
        RELAY_MAX_PAYLOAD_LEN,
    };
    use tokio::net::TcpListener;
    use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};
    use uuid::Uuid;

    use super::*;

    fn test_config() -> RelayConfig {
        RelayConfig::from_secret(b"test-secret".to_vec())
    }

    async fn spawn_server(config: RelayConfig) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let app: IntoMakeServiceWithConnectInfo<Router, SocketAddr> =
            router(config).into_make_service_with_connect_info();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        addr
    }

    async fn connect(
        addr: SocketAddr,
        path: &str,
    ) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>
    {
        let (stream, _) = connect_async(format!("ws://{addr}{path}"))
            .await
            .expect("connect");
        stream
    }

    async fn next_text<S>(stream: &mut S) -> String
    where
        S: StreamExt<Item = Result<WsMessage, tokio_tungstenite::tungstenite::Error>>
            + SinkExt<WsMessage>
            + Unpin,
    {
        loop {
            match stream.next().await.expect("frame").expect("ws") {
                WsMessage::Text(text) => return text.to_string(),
                WsMessage::Ping(payload) => {
                    if stream.send(WsMessage::Pong(payload)).await.is_err() {
                        panic!("pong");
                    }
                }
                other => panic!("unexpected {other:?}"),
            }
        }
    }

    fn frame(session: Uuid, channel: u8, payload: &[u8]) -> Vec<u8> {
        encode_relay_frame(session.as_bytes(), channel, payload).expect("frame")
    }

    #[tokio::test]
    async fn bridge_forwards_tcp_and_udp_both_directions() {
        let addr = spawn_server(test_config()).await;
        let token = relay_token(b"test-secret", "host-1");
        let mut host = connect(addr, "/register").await;
        host.send(WsMessage::Text(
            serde_json::to_string(&RelayControl::Register {
                host_id: "host-1".into(),
                token: token.clone(),
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
        let registered: RelayControl = serde_json::from_str(&next_text(&mut host).await).unwrap();
        assert!(matches!(registered, RelayControl::Registered { .. }));

        let mut client = connect(addr, "/connect/host-1").await;
        client
            .send(WsMessage::Text(
                serde_json::to_string(&RelayControl::Connect {
                    host_id: "host-1".into(),
                    token,
                })
                .unwrap()
                .into(),
            ))
            .await
            .unwrap();
        let incoming: RelayControl = serde_json::from_str(&next_text(&mut host).await).unwrap();
        let RelayControl::Incoming { session_id } = incoming else {
            panic!("expected incoming");
        };
        let session = Uuid::parse_str(&session_id).unwrap();
        host.send(WsMessage::Text(
            serde_json::to_string(&RelayControl::Accept {
                session_id: session_id.clone(),
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
        let connected: RelayControl = serde_json::from_str(&next_text(&mut client).await).unwrap();
        assert!(matches!(connected, RelayControl::Connected));

        let tcp = b"tcp-from-client";
        let udp = b"udp-from-client";
        client
            .send(WsMessage::Binary(
                frame(session, RELAY_CHANNEL_TCP, tcp).into(),
            ))
            .await
            .unwrap();
        client
            .send(WsMessage::Binary(
                frame(session, RELAY_CHANNEL_UDP, udp).into(),
            ))
            .await
            .unwrap();
        let got_tcp = next_binary(&mut host).await;
        let got_udp = next_binary(&mut host).await;
        assert_eq!(decode_relay_frame(&got_tcp).unwrap().payload, tcp);
        assert_eq!(decode_relay_frame(&got_udp).unwrap().payload, udp);

        let back_tcp = b"tcp-from-host";
        let back_udp = b"udp-from-host";
        host.send(WsMessage::Binary(
            frame(session, RELAY_CHANNEL_TCP, back_tcp).into(),
        ))
        .await
        .unwrap();
        host.send(WsMessage::Binary(
            frame(session, RELAY_CHANNEL_UDP, back_udp).into(),
        ))
        .await
        .unwrap();
        let got_back_tcp = next_binary(&mut client).await;
        let got_back_udp = next_binary(&mut client).await;
        assert_eq!(decode_relay_frame(&got_back_tcp).unwrap().payload, back_tcp);
        assert_eq!(decode_relay_frame(&got_back_udp).unwrap().payload, back_udp);
    }

    async fn next_binary<S>(stream: &mut S) -> Vec<u8>
    where
        S: StreamExt<Item = Result<WsMessage, tokio_tungstenite::tungstenite::Error>>
            + SinkExt<WsMessage>
            + Unpin,
    {
        loop {
            match stream.next().await.expect("frame").expect("ws") {
                WsMessage::Binary(bytes) => return bytes.to_vec(),
                WsMessage::Ping(payload) => {
                    if stream.send(WsMessage::Pong(payload)).await.is_err() {
                        panic!("pong");
                    }
                }
                WsMessage::Text(_) => {}
                other => panic!("unexpected {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn register_rejects_wrong_hmac() {
        let addr = spawn_server(test_config()).await;
        let mut host = connect(addr, "/register").await;
        host.send(WsMessage::Text(
            serde_json::to_string(&RelayControl::Register {
                host_id: "host-1".into(),
                token: "00".repeat(32),
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
        let error: RelayControl = serde_json::from_str(&next_text(&mut host).await).unwrap();
        match error {
            RelayControl::Error { code, .. } => assert_eq!(code, "unauthorized"),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn fifth_session_is_rejected() {
        let addr = spawn_server(test_config()).await;
        let token = relay_token(b"test-secret", "host-1");
        let mut host = connect(addr, "/register").await;
        host.send(WsMessage::Text(
            serde_json::to_string(&RelayControl::Register {
                host_id: "host-1".into(),
                token: token.clone(),
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
        let _ = next_text(&mut host).await;
        let mut clients = Vec::new();
        for _ in 0..4 {
            let mut client = connect(addr, "/connect/host-1").await;
            client
                .send(WsMessage::Text(
                    serde_json::to_string(&RelayControl::Connect {
                        host_id: "host-1".into(),
                        token: token.clone(),
                    })
                    .unwrap()
                    .into(),
                ))
                .await
                .unwrap();
            let incoming: RelayControl = serde_json::from_str(&next_text(&mut host).await).unwrap();
            let RelayControl::Incoming { session_id } = incoming else {
                panic!("expected incoming");
            };
            host.send(WsMessage::Text(
                serde_json::to_string(&RelayControl::Accept { session_id })
                    .unwrap()
                    .into(),
            ))
            .await
            .unwrap();
            let connected: RelayControl =
                serde_json::from_str(&next_text(&mut client).await).unwrap();
            assert!(matches!(connected, RelayControl::Connected));
            clients.push(client);
        }
        let mut extra = connect(addr, "/connect/host-1").await;
        extra
            .send(WsMessage::Text(
                serde_json::to_string(&RelayControl::Connect {
                    host_id: "host-1".into(),
                    token,
                })
                .unwrap()
                .into(),
            ))
            .await
            .unwrap();
        let error: RelayControl = serde_json::from_str(&next_text(&mut extra).await).unwrap();
        match error {
            RelayControl::Error { code, .. } => assert_eq!(code, "session_limit"),
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(clients.len(), 4);
    }

    #[tokio::test]
    async fn payload_over_2mib_is_rejected() {
        let addr = spawn_server(test_config()).await;
        let token = relay_token(b"test-secret", "host-1");
        let mut host = connect(addr, "/register").await;
        host.send(WsMessage::Text(
            serde_json::to_string(&RelayControl::Register {
                host_id: "host-1".into(),
                token: token.clone(),
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
        let _ = next_text(&mut host).await;
        let mut client = connect(addr, "/connect/host-1").await;
        client
            .send(WsMessage::Text(
                serde_json::to_string(&RelayControl::Connect {
                    host_id: "host-1".into(),
                    token,
                })
                .unwrap()
                .into(),
            ))
            .await
            .unwrap();
        let incoming: RelayControl = serde_json::from_str(&next_text(&mut host).await).unwrap();
        let RelayControl::Incoming { session_id } = incoming else {
            panic!("expected incoming");
        };
        let session = Uuid::parse_str(&session_id).unwrap();
        host.send(WsMessage::Text(
            serde_json::to_string(&RelayControl::Accept { session_id })
                .unwrap()
                .into(),
        ))
        .await
        .unwrap();
        let _ = next_text(&mut client).await;
        let payload = vec![7u8; RELAY_MAX_PAYLOAD_LEN + 1];
        let mut raw = session.as_bytes().to_vec();
        raw.push(RELAY_CHANNEL_TCP);
        raw.extend_from_slice(&payload);
        client.send(WsMessage::Binary(raw.into())).await.unwrap();
        let error: RelayControl = serde_json::from_str(&next_text(&mut client).await).unwrap();
        match error {
            RelayControl::Error { code, .. } => assert_eq!(code, "bad_frame"),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn empty_relay_auth_secret_fails_config() {
        assert!(config_from_secret(None).is_err());
        assert!(config_from_secret(Some("")).is_err());
        assert!(config_from_env().is_err() || std::env::var("RELAY_AUTH_SECRET").is_ok());
        assert!(config_from_secret(Some("present")).is_ok());
    }
}
