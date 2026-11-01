use std::{net::SocketAddr, path::PathBuf, time::Duration};

use futures_util::{SinkExt, StreamExt};
use ferryx_rd_proto::{
    decode_relay_frame, encode_relay_frame, relay_token, RelayControl, RELAY_CHANNEL_TCP,
    RELAY_CHANNEL_UDP,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream, UdpSocket},
    sync::mpsc,
};
use tokio_tungstenite::{connect_async, tungstenite::Message};

/// Reads the relay auth secret from RELAY_AUTH_SECRET or ~/.maho-relay-secret.
pub fn load_auth_secret() -> Option<String> {
    if let Ok(secret) = std::env::var("RELAY_AUTH_SECRET") {
        if !secret.is_empty() {
            return Some(secret);
        }
    }
    let path = std::env::var("HOME").ok()?;
    let file = PathBuf::from(path).join(".maho-relay-secret");
    std::fs::read_to_string(file)
        .ok()
        .map(|content| content.trim().to_owned())
        .filter(|secret| !secret.is_empty())
}

#[derive(Debug)]
pub struct ClientRelayBridge {
    pub local_tcp: SocketAddr,
    pub local_udp: SocketAddr,
}

pub fn connect_url(base: &str, host_id: &str) -> String {
    let trimmed = base.trim().trim_end_matches('/');
    let with_scheme = if let Some(rest) = trimmed.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = trimmed.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        trimmed.to_owned()
    };
    let root = with_scheme
        .trim_end_matches("/register")
        .trim_end_matches("/connect");
    format!("{root}/connect/{host_id}")
}

pub async fn start_client_bridge(
    relay_base: &str,
    host_id: &str,
    secret: &[u8],
) -> Result<ClientRelayBridge, String> {
    let tcp_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|error| error.to_string())?;
    let local_tcp = tcp_listener
        .local_addr()
        .map_err(|error| error.to_string())?;
    let udp = UdpSocket::bind("127.0.0.1:0")
        .await
        .map_err(|error| error.to_string())?;
    let local_udp = udp.local_addr().map_err(|error| error.to_string())?;
    let url = connect_url(relay_base, host_id);
    let (mut socket, _) = connect_async(&url)
        .await
        .map_err(|error| error.to_string())?;
    let token = relay_token(secret, host_id);
    let connect = serde_json::to_string(&RelayControl::Connect {
        host_id: host_id.to_owned(),
        token,
    })
    .map_err(|error| error.to_string())?;
    socket
        .send(Message::Text(connect.into()))
        .await
        .map_err(|error| error.to_string())?;
    wait_connected(&mut socket).await?;
    let (tcp_uplink, mut tcp_uplink_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let (udp_uplink, mut udp_uplink_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let mut udp_peer: Option<SocketAddr> = None;
    tokio::spawn(async move {
        let accepted = tokio::time::timeout(Duration::from_secs(10), tcp_listener.accept()).await;
        let Ok(Ok((stream, _))) = accepted else {
            return;
        };
        let (mut read_half, mut write_half) = stream.into_split();
        let mut tcp_buf = vec![0_u8; 16 * 1024];
        let mut udp_buf = vec![0_u8; 64 * 1024];
        let nil = [0_u8; 16];
        loop {
            tokio::select! {
                incoming = socket.next() => {
                    match incoming {
                        Some(Ok(Message::Binary(bytes))) => {
                            let Ok(frame) = decode_relay_frame(bytes.as_ref()) else { continue };
                            if frame.channel == RELAY_CHANNEL_TCP {
                                if write_half.write_all(&frame.payload).await.is_err() {
                                    break;
                                }
                            } else if frame.channel == RELAY_CHANNEL_UDP {
                                let Some(peer) = udp_peer else { continue };
                                if udp.send_to(&frame.payload, peer).await.is_err() {
                                    break;
                                }
                            }
                        }
                        Some(Ok(Message::Ping(payload))) => {
                            if socket.send(Message::Pong(payload)).await.is_err() {
                                break;
                            }
                        }
                        Some(Ok(_)) => {}
                        _ => break,
                    }
                }
                payload = tcp_uplink_rx.recv() => {
                    let Some(payload) = payload else { break };
                    let Ok(frame) = encode_relay_frame(&nil, RELAY_CHANNEL_TCP, &payload) else { break };
                    if socket.send(Message::Binary(frame.into())).await.is_err() {
                        break;
                    }
                }
                payload = udp_uplink_rx.recv() => {
                    let Some(payload) = payload else { break };
                    let Ok(frame) = encode_relay_frame(&nil, RELAY_CHANNEL_UDP, &payload) else { break };
                    if socket.send(Message::Binary(frame.into())).await.is_err() {
                        break;
                    }
                }
                read = read_half.read(&mut tcp_buf) => {
                    match read {
                        Ok(0) | Err(_) => break,
                        Ok(size) => {
                            if tcp_uplink.send(tcp_buf[..size].to_vec()).is_err() {
                                break;
                            }
                        }
                    }
                }
                read = udp.recv_from(&mut udp_buf) => {
                    match read {
                        Err(_) => break,
                        Ok((size, peer)) => {
                            udp_peer = Some(peer);
                            if udp_uplink.send(udp_buf[..size].to_vec()).is_err() {
                                break;
                            }
                        }
                    }
                }
            }
        }
    });
    Ok(ClientRelayBridge {
        local_tcp,
        local_udp,
    })
}

async fn wait_connected(
    socket: &mut tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>,
) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err("relay connect timed out".to_owned());
        }
        let message = tokio::time::timeout(remaining, socket.next())
            .await
            .map_err(|_| "relay connect timed out".to_owned())?
            .ok_or_else(|| "relay socket closed".to_owned())?
            .map_err(|error| error.to_string())?;
        match message {
            Message::Text(text) => {
                let control: RelayControl =
                    serde_json::from_str(text.as_str()).map_err(|error| error.to_string())?;
                match control {
                    RelayControl::Connected => return Ok(()),
                    RelayControl::Error { message, .. } => return Err(message),
                    _ => {}
                }
            }
            Message::Ping(payload) => {
                socket
                    .send(Message::Pong(payload))
                    .await
                    .map_err(|error| error.to_string())?;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connect_url_uses_connect_path() {
        assert_eq!(
            connect_url("https://relay.example", "host-1"),
            "wss://relay.example/connect/host-1"
        );
        assert_eq!(
            connect_url("ws://127.0.0.1:9/register", "host-1"),
            "ws://127.0.0.1:9/connect/host-1"
        );
    }
}
