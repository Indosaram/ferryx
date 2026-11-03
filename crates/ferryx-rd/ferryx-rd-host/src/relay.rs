use std::{collections::HashMap, net::SocketAddr, time::Duration};

use futures_util::{SinkExt, StreamExt};
use ferryx_rd_proto::{
    decode_relay_frame, encode_relay_frame, relay_host_id_ok, relay_token, RelayControl,
    RELAY_CHANNEL_TCP, RELAY_CHANNEL_UDP,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpStream, UdpSocket},
    sync::mpsc,
};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct HostRelayConfig {
    pub url: String,
    pub host_id: String,
    pub secret: Vec<u8>,
    pub local_tcp: SocketAddr,
    pub local_udp: SocketAddr,
}

pub fn register_url(base: &str) -> String {
    let trimmed = base.trim().trim_end_matches('/');
    let with_scheme = if let Some(rest) = trimmed.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = trimmed.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        trimmed.to_owned()
    };
    if with_scheme.ends_with("/register") {
        with_scheme
    } else {
        format!("{with_scheme}/register")
    }
}

pub fn sanitize_host_id(name: &str) -> String {
    let mut out: String = name
        .bytes()
        .filter(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        .map(|byte| byte as char)
        .collect();
    if out.is_empty() {
        out.push_str("host");
    }
    out.truncate(128);
    out
}

pub fn next_backoff(current: Duration) -> Duration {
    (current.saturating_mul(2))
        .min(Duration::from_secs(30))
        .max(Duration::from_secs(1))
}

pub struct HostRelayHandle {
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for HostRelayHandle {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Owned registration: dropping the handle stops registration, retries and
/// all forwarding tasks on its private runtime.
pub fn spawn_host_relay(config: HostRelayConfig) -> std::io::Result<HostRelayHandle> {
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let thread = std::thread::Builder::new()
        .name("ferryx-rd-host-relay".to_owned())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    tracing::error!(%error, "relay runtime failed");
                    return;
                }
            };
            runtime.block_on(async move {
                tokio::select! {
                    _ = stopped => {},
                    _ = maintain(config) => {},
                }
            });
        })?;
    Ok(HostRelayHandle {
        stop: Some(stop),
        thread: Some(thread),
    })
}

pub async fn maintain(config: HostRelayConfig) {
    if !relay_host_id_ok(&config.host_id) {
        tracing::error!(host_id = %config.host_id, "relay host id is not valid");
        return;
    }
    let mut delay = Duration::from_secs(1);
    loop {
        match serve_registration(&config).await {
            Ok(()) => delay = Duration::from_secs(1),
            Err(error) => {
                tracing::warn!(%error, "relay registration ended");
            }
        }
        tokio::time::sleep(delay).await;
        delay = next_backoff(delay);
    }
}

async fn serve_registration(config: &HostRelayConfig) -> Result<(), String> {
    let url = register_url(&config.url);
    let (mut socket, _) = connect_async(&url)
        .await
        .map_err(|error| error.to_string())?;
    let token = relay_token(&config.secret, &config.host_id);
    let register = serde_json::to_string(&RelayControl::Register {
        host_id: config.host_id.clone(),
        token,
    })
    .map_err(|error| error.to_string())?;
    socket
        .send(Message::Text(register.into()))
        .await
        .map_err(|error| error.to_string())?;
    let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<Message>();
    let mut sessions: HashMap<String, SessionLink> = HashMap::new();
    loop {
        tokio::select! {
            incoming = socket.next() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        if let Err(error) = handle_text(config, &text, &outbound_tx, &mut sessions).await {
                            tracing::warn!(%error, "relay control failed");
                        }
                    }
                    Some(Ok(Message::Binary(bytes))) => {
                        if let Err(error) = write_downlink(&bytes, &sessions).await {
                            tracing::debug!(%error, "relay downlink dropped");
                        }
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        let _ = outbound_tx.send(Message::Pong(payload));
                    }
                    Some(Ok(Message::Pong(_))) => {}
                    Some(Ok(Message::Close(_))) | None => return Err("relay socket closed".to_owned()),
                    Some(Err(error)) => return Err(error.to_string()),
                    Some(Ok(_)) => {}
                }
            }
            outgoing = outbound_rx.recv() => {
                match outgoing {
                    Some(message) => {
                        socket.send(message).await.map_err(|error| error.to_string())?;
                    }
                    None => return Err("relay writer closed".to_owned()),
                }
            }
        }
    }
}

enum Downlink {
    Tcp(Vec<u8>),
    Udp(Vec<u8>),
}

struct SessionLink {
    downlink: mpsc::UnboundedSender<Downlink>,
}

async fn handle_text(
    config: &HostRelayConfig,
    text: &str,
    outbound: &mpsc::UnboundedSender<Message>,
    sessions: &mut HashMap<String, SessionLink>,
) -> Result<(), String> {
    let control: RelayControl = serde_json::from_str(text).map_err(|error| error.to_string())?;
    match control {
        RelayControl::Incoming { session_id } => {
            let session = Uuid::parse_str(&session_id).map_err(|error| error.to_string())?;
            let tcp = TcpStream::connect(config.local_tcp)
                .await
                .map_err(|error| error.to_string())?;
            tcp.set_nodelay(true).map_err(|error| error.to_string())?;
            let (mut read_half, mut write_half) = tcp.into_split();
            let udp = UdpSocket::bind("127.0.0.1:0")
                .await
                .map_err(|error| error.to_string())?;
            udp.connect(config.local_udp)
                .await
                .map_err(|error| error.to_string())?;
            let (downlink, mut downlink_rx) = mpsc::unbounded_channel();
            sessions.insert(session_id.clone(), SessionLink { downlink });
            let accept = serde_json::to_string(&RelayControl::Accept {
                session_id: session_id.clone(),
            })
            .map_err(|error| error.to_string())?;
            outbound
                .send(Message::Text(accept.into()))
                .map_err(|error| error.to_string())?;
            let uplink = outbound.clone();
            tokio::spawn(async move {
                let mut tcp_buf = vec![0_u8; 16 * 1024];
                let mut udp_buf = vec![0_u8; 64 * 1024];
                loop {
                    tokio::select! {
                        message = downlink_rx.recv() => {
                            match message {
                                Some(Downlink::Tcp(bytes)) => {
                                    if write_half.write_all(&bytes).await.is_err() {
                                        break;
                                    }
                                }
                                Some(Downlink::Udp(bytes)) => {
                                    if udp.send(&bytes).await.is_err() {
                                        break;
                                    }
                                }
                                None => break,
                            }
                        }
                        read = read_half.read(&mut tcp_buf) => {
                            match read {
                                Ok(0) | Err(_) => break,
                                Ok(size) => {
                                    if let Ok(frame) = encode_relay_frame(session.as_bytes(), RELAY_CHANNEL_TCP, &tcp_buf[..size]) {
                                        if uplink.send(Message::Binary(frame.into())).is_err() {
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                        read = udp.recv(&mut udp_buf) => {
                            match read {
                                Ok(size) => {
                                    if let Ok(frame) = encode_relay_frame(session.as_bytes(), RELAY_CHANNEL_UDP, &udp_buf[..size]) {
                                        if uplink.send(Message::Binary(frame.into())).is_err() {
                                            break;
                                        }
                                    }
                                }
                                Err(_) => break,
                            }
                        }
                    }
                }
            });
            Ok(())
        }
        RelayControl::Ping => {
            let pong =
                serde_json::to_string(&RelayControl::Pong).map_err(|error| error.to_string())?;
            outbound
                .send(Message::Text(pong.into()))
                .map_err(|error| error.to_string())?;
            Ok(())
        }
        _ => Ok(()),
    }
}

async fn write_downlink(
    bytes: &[u8],
    sessions: &HashMap<String, SessionLink>,
) -> Result<(), String> {
    let frame = decode_relay_frame(bytes).map_err(|error| error.to_string())?;
    let id = Uuid::from_slice(&frame.session_id).map_err(|error| error.to_string())?;
    let Some(link) = sessions.get(&id.to_string()) else {
        return Ok(());
    };
    let message = match frame.channel {
        RELAY_CHANNEL_TCP => Downlink::Tcp(frame.payload),
        RELAY_CHANNEL_UDP => Downlink::Udp(frame.payload),
        _ => return Err("bad channel".to_owned()),
    };
    link.downlink
        .send(message)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_url_rewrites_https_and_appends_path() {
        assert_eq!(
            register_url("https://relay.example"),
            "wss://relay.example/register"
        );
        assert_eq!(
            register_url("ws://127.0.0.1:9/register"),
            "ws://127.0.0.1:9/register"
        );
    }

    #[test]
    fn sanitize_host_id_drops_illegal_chars() {
        assert_eq!(sanitize_host_id("My Host!"), "MyHost");
        assert_eq!(sanitize_host_id(""), "host");
        assert!(relay_host_id_ok(&sanitize_host_id(&"a".repeat(200))));
    }

    #[tokio::test]
    async fn dropping_registration_handle_closes_relay_socket() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (registered_tx, registered_rx) = tokio::sync::oneshot::channel();
        let peer = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();
            let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
                panic!("expected registration");
            };
            assert!(matches!(
                serde_json::from_str::<RelayControl>(&text).unwrap(),
                RelayControl::Register { .. }
            ));
            registered_tx.send(()).unwrap();
            // Runtime shutdown may end TCP without a WebSocket close frame.
            loop {
                match socket.next().await {
                    None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                    Some(Ok(Message::Text(text))) => assert!(matches!(
                        serde_json::from_str::<RelayControl>(&text).unwrap(),
                        RelayControl::Ping
                    )),
                    other => panic!("unexpected relay frame: {other:?}"),
                }
            }
        });
        let handle = spawn_host_relay(HostRelayConfig {
            url: format!("ws://{address}"),
            host_id: "owned-host".into(),
            secret: b"secret".to_vec(),
            local_tcp: address,
            local_udp: address,
        })
        .unwrap();
        tokio::time::timeout(Duration::from_secs(5), registered_rx)
            .await
            .unwrap()
            .unwrap();
        tokio::task::spawn_blocking(move || drop(handle))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), peer)
            .await
            .unwrap()
            .unwrap();
    }

    #[test]
    fn backoff_caps_at_thirty_seconds() {
        assert_eq!(next_backoff(Duration::from_secs(1)), Duration::from_secs(2));
        assert_eq!(
            next_backoff(Duration::from_secs(20)),
            Duration::from_secs(30)
        );
    }

    #[tokio::test]
    async fn host_relay_writes_tcp_and_udp_payloads_to_local_sockets() {
        use std::net::SocketAddr;

        use futures_util::{SinkExt, StreamExt};
        use ferryx_rd_proto::{relay_token, RelayControl, RELAY_CHANNEL_TCP, RELAY_CHANNEL_UDP};
        use tokio::io::AsyncReadExt;
        use tokio::net::{TcpListener, UdpSocket};
        use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};

        let relay_listener = TcpListener::bind("127.0.0.1:0").await.expect("relay bind");
        let relay_addr = relay_listener.local_addr().expect("relay addr");
        let app = ferryx_rd_relay::router(ferryx_rd_relay::RelayConfig::from_secret(
            b"test-secret".to_vec(),
        ))
        .into_make_service_with_connect_info::<SocketAddr>();
        tokio::spawn(async move {
            let _ = axum::serve(relay_listener, app).await;
        });

        let tcp_listener = TcpListener::bind("127.0.0.1:0").await.expect("tcp bind");
        let local_tcp = tcp_listener.local_addr().expect("tcp addr");
        let udp = UdpSocket::bind("127.0.0.1:0").await.expect("udp bind");
        let local_udp = udp.local_addr().expect("udp addr");
        let host_config = HostRelayConfig {
            url: format!("ws://{relay_addr}"),
            host_id: "host-1".to_owned(),
            secret: b"test-secret".to_vec(),
            local_tcp,
            local_udp,
        };
        tokio::spawn(async move {
            let _ = serve_registration(&host_config).await;
        });

        let token = relay_token(b"test-secret", "host-1");
        let mut client = None;
        for _ in 0..50 {
            let Ok((mut stream, _)) =
                connect_async(format!("ws://{relay_addr}/connect/host-1")).await
            else {
                tokio::time::sleep(Duration::from_millis(20)).await;
                continue;
            };
            if stream
                .send(WsMessage::Text(
                    serde_json::to_string(&RelayControl::Connect {
                        host_id: "host-1".into(),
                        token: token.clone(),
                    })
                    .expect("connect json")
                    .into(),
                ))
                .await
                .is_err()
            {
                continue;
            }
            let accepted = tokio::time::timeout(Duration::from_secs(1), async {
                loop {
                    match stream.next().await {
                        Some(Ok(WsMessage::Text(text))) => {
                            let Ok(control) = serde_json::from_str::<RelayControl>(text.as_str())
                            else {
                                continue;
                            };
                            if matches!(control, RelayControl::Connected) {
                                return true;
                            }
                            if matches!(control, RelayControl::Error { .. }) {
                                return false;
                            }
                        }
                        Some(Ok(WsMessage::Ping(payload))) => {
                            if stream.send(WsMessage::Pong(payload)).await.is_err() {
                                return false;
                            }
                        }
                        _ => return false,
                    }
                }
            })
            .await;
            if matches!(accepted, Ok(true)) {
                client = Some(stream);
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let mut client = client.expect("host did not accept");

        let (mut accepted, _) = tokio::time::timeout(Duration::from_secs(2), tcp_listener.accept())
            .await
            .expect("accept timeout")
            .expect("accept");
        let tcp_payload = b"tcp-via-host-relay";
        let udp_payload = b"udp-via-host-relay";
        let mut tcp_frame = vec![0_u8; 17 + tcp_payload.len()];
        tcp_frame[16] = RELAY_CHANNEL_TCP;
        tcp_frame[17..].copy_from_slice(tcp_payload);
        let mut udp_frame = vec![0_u8; 17 + udp_payload.len()];
        udp_frame[16] = RELAY_CHANNEL_UDP;
        udp_frame[17..].copy_from_slice(udp_payload);
        client
            .send(WsMessage::Binary(tcp_frame.into()))
            .await
            .expect("send tcp");
        client
            .send(WsMessage::Binary(udp_frame.into()))
            .await
            .expect("send udp");

        let mut got_tcp = vec![0_u8; tcp_payload.len()];
        tokio::time::timeout(Duration::from_secs(2), accepted.read_exact(&mut got_tcp))
            .await
            .expect("tcp timeout")
            .expect("tcp read");
        assert_eq!(got_tcp, tcp_payload);
        let mut got_udp = vec![0_u8; udp_payload.len()];
        let (size, _) = tokio::time::timeout(Duration::from_secs(2), udp.recv_from(&mut got_udp))
            .await
            .expect("udp timeout")
            .expect("udp recv");
        assert_eq!(&got_udp[..size], udp_payload);
    }
}
