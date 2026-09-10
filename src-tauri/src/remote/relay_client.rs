//! Desktop daemon reverse tunnel client.
//!
//! Counterpart to [`crate::remote::relay_server`]: this is the piece that
//! runs *on the desktop daemon*. It holds a long-lived outbound WebSocket
//! connection to a `ferryx-relay` instance's `/tunnel/control` endpoint,
//! authenticated with the daemon's Machine Token. Whenever the relay
//! notifies it of an incoming session (a remote client wants to attach),
//! it opens a second WebSocket to `/tunnel/data/:session_id` and proxies
//! frames between that socket and the local loopback gateway that Ferryx
//! serves at the address supplied by the gateway runtime.
//!
//! The control connection is expected to stay up for the lifetime of the
//! daemon process; if it drops (relay restart, network blip, ...) the
//! client reconnects automatically with exponential backoff.

use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::AUTHORIZATION;
use tokio_tungstenite::tungstenite::Message;

/// The local gateway that Ferryx's remote server listens on. Data channel
/// traffic is proxied to/from this address.
const LOCAL_GATEWAY_ADDR: &str = "127.0.0.1:43821";

/// Initial delay before the first reconnect attempt after a dropped or
/// failed control connection.
const INITIAL_BACKOFF: Duration = Duration::from_millis(500);

/// Ceiling on the reconnect backoff delay.
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Notification pushed down the control channel when a client asks to open
/// a new session against this daemon. Mirrors
/// [`crate::remote::relay_server::IncomingSessionNotice`] - kept as a
/// separate type here (rather than a shared import) so this module only
/// depends on the wire shape, not the server's internal types.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct SessionRequest {
    session_id: String,
}

/// Reverse tunnel client run by the desktop daemon.
///
/// Construct with [`RelayClient::new`] and drive with [`RelayClient::run`],
/// typically spawned onto its own long-lived task. `run` never returns
/// under normal operation - it reconnects on any control-channel failure
/// and only exits if the task itself is dropped/aborted.
#[derive(Clone)]
pub struct RelayClient {
    /// Base HTTP(S) URL of the relay, e.g. `https://relay.example.com`.
    /// Scheme is upgraded to `ws`/`wss` when building tunnel URLs.
    relay_url: String,
    /// This daemon's Machine Token, presented as a bearer credential on
    /// the control channel.
    machine_token: String,
    /// Local loopback gateway address that data channels are proxied to.
    gateway_addr: String,
}

impl RelayClient {
    /// Creates a client targeting `relay_url` (e.g.
    /// `https://relay.example.com` or `wss://relay.example.com`) using
    /// `machine_token` for authentication. Data channels are proxied to
    /// the local gateway on [`LOCAL_GATEWAY_ADDR`].
    pub fn new(relay_url: impl Into<String>, machine_token: impl Into<String>) -> Self {
        Self::with_gateway(relay_url, machine_token, LOCAL_GATEWAY_ADDR)
    }

    /// Creates a client proxying to the gateway's actual bound address,
    /// including when the gateway requests an OS-assigned port.
    pub fn with_gateway(
        relay_url: impl Into<String>,
        machine_token: impl Into<String>,
        gateway_addr: impl Into<String>,
    ) -> Self {
        Self {
            relay_url: relay_url.into(),
            machine_token: machine_token.into(),
            gateway_addr: gateway_addr.into(),
        }
    }

    fn control_url(&self) -> String {
        format!("{}/tunnel/control", to_ws_base(&self.relay_url))
    }

    fn data_url(&self, session_id: &str) -> String {
        format!(
            "{}/tunnel/data/{}",
            to_ws_base(&self.relay_url),
            session_id
        )
    }

    /// Runs the reverse tunnel client forever: connects the control
    /// channel, handles incoming session notifications until the
    /// connection drops, then reconnects with exponential backoff and
    /// repeats. Intended to be spawned onto a dedicated task.
    pub async fn run(&self) {
        let mut backoff = INITIAL_BACKOFF;
        loop {
            match self.run_control_session().await {
                Ok(()) => {
                    // Clean shutdown (control socket closed normally).
                    // Still reconnect - the daemon is expected to stay
                    // tunneled for as long as it runs - but reset backoff
                    // since the prior session was healthy.
                    tracing::info!("relay control channel closed; reconnecting");
                    backoff = INITIAL_BACKOFF;
                }
                Err(err) => {
                    tracing::warn!(
                        "relay control channel error: {err}; retrying in {:.1}s",
                        backoff.as_secs_f64()
                    );
                }
            }

            tokio::time::sleep(backoff).await;
            backoff = std::cmp::min(backoff * 2, MAX_BACKOFF);
        }
    }

    /// Connects the control channel and services it until it closes or
    /// errors. Each incoming [`SessionRequest`] spawns an independent task
    /// that opens a data channel and proxies it against the local gateway,
    /// so a slow or stuck session cannot block subsequent notifications.
    async fn run_control_session(&self) -> anyhow::Result<()> {
        let mut request = self.control_url().into_client_request()?;
        request.headers_mut().insert(
            AUTHORIZATION,
            format!("Bearer {}", self.machine_token).parse()?,
        );

        let (socket, _response) = tokio_tungstenite::connect_async(request).await?;
        tracing::info!("relay control channel connected");
        let (mut write, mut read) = socket.split();
        // Dropping the control future also aborts its data channels.
        let mut sessions = tokio::task::JoinSet::new();

        loop {
            let message = tokio::select! {
                message = read.next() => message,
                result = sessions.join_next(), if !sessions.is_empty() => {
                    if let Some(Err(err)) = result {
                        tracing::warn!("relay data channel task failed: {err}");
                    }
                    continue;
                }
            };
            match message {
                Some(Ok(Message::Text(text))) => {
                    match serde_json::from_str::<SessionRequest>(&text) {
                        Ok(SessionRequest { session_id }) => {
                            let client = self.clone();
                            sessions.spawn(async move {
                                if let Err(err) = client.handle_session(&session_id).await {
                                    tracing::warn!(
                                        "relay data channel for session {session_id} failed: {err}"
                                    );
                                }
                            });
                        }
                        Err(err) => {
                            tracing::warn!("relay control channel sent unrecognized payload: {err}");
                        }
                    }
                }
                Some(Ok(Message::Ping(payload))) => {
                    write.send(Message::Pong(payload)).await?;
                }
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(_)) => {
                    // Binary/Pong/Frame - control channel has no other
                    // traffic; ignore.
                }
                Some(Err(err)) => return Err(err.into()),
            }
        }

        Ok(())
    }

    /// Opens a data channel for `session_id` against the relay and proxies
    /// frames bidirectionally between it and the local loopback gateway
    /// until either side disconnects.
    async fn handle_session(&self, session_id: &str) -> anyhow::Result<()> {
        let (ws, _response) = tokio_tungstenite::connect_async(self.data_url(session_id)).await?;
        let gateway = TcpStream::connect(&self.gateway_addr).await?;
        proxy_ws_to_tcp(ws, gateway).await
    }
}

/// Rewrites an `http(s)://` (or already-`ws(s)://`) URL to its `ws(s)://`
/// equivalent, trimming any trailing slash so path segments can be
/// appended directly.
fn to_ws_base(url: &str) -> String {
    let rewritten = if let Some(rest) = url.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = url.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        url.to_string()
    };
    rewritten.trim_end_matches('/').to_string()
}

/// Proxies frames bidirectionally between a relay data-channel WebSocket
/// and a local TCP connection (the loopback gateway) until either side
/// closes or errors.
async fn proxy_ws_to_tcp(
    ws: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    tcp: TcpStream,
) -> anyhow::Result<()> {
    let (mut ws_tx, mut ws_rx) = ws.split();
    let (mut tcp_rx, mut tcp_tx) = tcp.into_split();

    let ws_to_tcp = async {
        loop {
            match ws_rx.next().await {
                Some(Ok(Message::Binary(data))) => {
                    if tcp_tx.write_all(&data).await.is_err() {
                        break;
                    }
                }
                Some(Ok(Message::Text(text))) => {
                    if tcp_tx.write_all(text.as_bytes()).await.is_err() {
                        break;
                    }
                }
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(_)) => continue,
                Some(Err(_)) => break,
            }
        }
        let _ = tcp_tx.shutdown().await;
    };

    let tcp_to_ws = async {
        let mut buf = [0u8; 8192];
        loop {
            match tcp_rx.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if ws_tx.send(Message::Binary(buf[..n].to_vec().into())).await.is_err() {
                        break;
                    }
                }
            }
        }
        let _ = ws_tx.send(Message::Close(None)).await;
    };

    tokio::join!(ws_to_tcp, tcp_to_ws);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_ws_base_rewrites_https_and_trims_trailing_slash() {
        assert_eq!(to_ws_base("https://relay.example.com/"), "wss://relay.example.com");
        assert_eq!(to_ws_base("http://localhost:8787"), "ws://localhost:8787");
        assert_eq!(to_ws_base("wss://relay.example.com"), "wss://relay.example.com");
    }

    #[test]
    fn control_and_data_urls_are_built_from_relay_url() {
        let client = RelayClient::new("https://relay.example.com", "tok");
        assert_eq!(
            client.control_url(),
            "wss://relay.example.com/tunnel/control"
        );
        assert_eq!(
            client.data_url("abc123"),
            "wss://relay.example.com/tunnel/data/abc123"
        );
    }

    #[test]
    fn with_gateway_preserves_address_and_credentials() {
        let client = RelayClient::with_gateway("http://localhost:8787/", "machine", "127.0.0.1:54321");
        assert_eq!(client.gateway_addr, "127.0.0.1:54321");
        assert_eq!(client.machine_token, "machine");
        assert_eq!(client.control_url(), "ws://localhost:8787/tunnel/control");
        assert_eq!(client.data_url("session"), "ws://localhost:8787/tunnel/data/session");
        assert_eq!(RelayClient::new("http://localhost", "tok").gateway_addr, LOCAL_GATEWAY_ADDR);
    }

    #[tokio::test]
    async fn proxies_frames_between_data_channel_and_local_gateway() {
        use tokio::net::TcpListener;

        // Fake local gateway: echoes back whatever it receives, prefixed.
        let gateway_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let gateway_addr = gateway_listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut sock, _) = gateway_listener.accept().await.unwrap();
            let mut buf = [0u8; 5];
            sock.read_exact(&mut buf).await.unwrap();
            let mut reply = b"echo:".to_vec();
            reply.extend_from_slice(&buf);
            sock.write_all(&reply).await.unwrap();
        });

        // A relay fixture sends the exact session notification after upgrade,
        // then exposes the data socket through a signal, not a timing delay.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let relay_addr = listener.local_addr().unwrap();
        let (data_tx, data_rx) = tokio::sync::oneshot::channel();
        let relay_task = tokio::spawn(async move {
            let (control, _) = listener.accept().await.unwrap();
            let mut control = tokio_tungstenite::accept_async(control).await.unwrap();
            control.send(Message::Text(r#"{"session_id":"sess-1"}"#.into())).await.unwrap();
            let (data, _) = listener.accept().await.unwrap();
            let data = tokio_tungstenite::accept_async(data).await.unwrap();
            data_tx.send(data).unwrap();
            while let Some(message) = control.next().await {
                if message.is_err() { break; }
            }
        });

        let relay_url = format!("http://{relay_addr}");
        let client = RelayClient::with_gateway(&relay_url, "tok", gateway_addr.to_string());

        // Drive the daemon's control channel (and any sessions it spawns)
        // in the background.
        let client_task = tokio::spawn(async move {
            let _ = client.run_control_session().await;
        });

        let mut client_ws = tokio::time::timeout(Duration::from_secs(5), data_rx)
            .await.expect("data connection timed out").unwrap();

        client_ws
            .send(Message::Binary(b"hello".to_vec().into()))
            .await
            .unwrap();

        let reply = tokio::time::timeout(Duration::from_secs(5), client_ws.next())
            .await
            .expect("timed out waiting for proxied reply")
            .expect("stream ended without a reply")
            .expect("websocket error");

        match reply {
            Message::Binary(data) => assert_eq!(data.as_ref(), b"echo:hello"),
            other => panic!("expected binary reply, got {other:?}"),
        }

        client_task.abort();
        assert!(client_task.await.unwrap_err().is_cancelled());
        relay_task.abort();
    }
}
