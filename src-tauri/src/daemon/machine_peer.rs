//! Machine gateway negotiation is explicit; pre-contract owners fail closed.
use super::{LegacyPeer, LegacyStream};
use crate::daemon::protocol::{DaemonRequest, DaemonResponse, DAEMON_PROTOCOL_VERSION};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use std::time::Duration;

impl LegacyPeer {
    pub(crate) async fn close_machine_http(&self, id: &str, token: &str, body: &[u8]) -> Result<(u16, Vec<u8>), String> {
        use tokio::io::AsyncReadExt;
        tokio::time::timeout(Duration::from_secs(40), async {
            let mut stream = self.machine_gateway().await?;
            let mut url = reqwest::Url::parse("http://localhost/").map_err(|_| "INVALID_REQUEST")?;
            url.path_segments_mut().map_err(|_| "INVALID_REQUEST")?.extend(["api", "v1", "sessions", id]);
            let authorization = axum::http::HeaderValue::from_str(&format!("Bearer {token}")).map_err(|_| "UNAUTHORIZED")?;
            let head = format!("DELETE {} HTTP/1.1\r\nHost: localhost\r\nAuthorization: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", url.path(), authorization.to_str().map_err(|_| "UNAUTHORIZED")?, body.len());
            stream.write_all(head.as_bytes()).await.map_err(|_| "HOST_UNAVAILABLE")?;
            stream.write_all(body).await.map_err(|_| "HOST_UNAVAILABLE")?;
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            (&mut reader).take(8193).read_line(&mut line).await.map_err(|_| "HOST_UNAVAILABLE")?;
            if line.len() > 8192 { return Err("HOST_UNAVAILABLE".into()); }
            let status = line.split_whitespace().nth(1).ok_or("HOST_UNAVAILABLE")?.parse::<u16>().map_err(|_| "HOST_UNAVAILABLE")?;
            let mut length = None;
            let mut header_bytes = line.len();
            loop {
                line.clear();
                (&mut reader).take(8193).read_line(&mut line).await.map_err(|_| "HOST_UNAVAILABLE")?;
                header_bytes += line.len();
                if header_bytes > 16384 { return Err("HOST_UNAVAILABLE".into()); }
                if line == "\r\n" { break; }
                if line.is_empty() || line.len() > 8192 { return Err("HOST_UNAVAILABLE".into()); }
                if let Some((name, value)) = line.split_once(':') {
                    if name.eq_ignore_ascii_case("content-length") { length = Some(value.trim().parse::<usize>().map_err(|_| "HOST_UNAVAILABLE")?); }
                }
            }
            let length = match (status, length) {
                (204, None) => 0,
                (_, Some(length)) => length,
                (_, None) => return Err("HOST_UNAVAILABLE".into()),
            };
            if length > 64 * 1024 { return Err("HOST_UNAVAILABLE".into()); }
            let mut bytes = vec![0; length];
            reader.read_exact(&mut bytes).await.map_err(|_| "HOST_UNAVAILABLE")?;
            Ok((status, bytes))
        }).await.map_err(|_| "TIMEOUT".to_owned())?
    }

    pub(crate) async fn machine_gateway(&self) -> Result<LegacyStream, String> {
        tokio::time::timeout(Duration::from_secs(10), async {
            let mut stream = self.connect_stream().await.map_err(|_| "HOST_UNAVAILABLE")?;
            for request in [DaemonRequest::Handshake { version: DAEMON_PROTOCOL_VERSION }, DaemonRequest::MachineGateway] {
                let mut bytes = serde_json::to_vec(&request).map_err(|error| error.to_string())?;
                bytes.push(b'\n');
                stream.write_all(&bytes).await.map_err(|_| "HOST_UNAVAILABLE")?;
                let mut line = String::new();
                // Capacity one leaves every HTTP/WS byte in the underlying stream.
                BufReader::with_capacity(1, &mut stream).read_line(&mut line).await.map_err(|_| "HOST_UNAVAILABLE")?;
                let response = serde_json::from_str::<DaemonResponse>(&line).map_err(|_| "MACHINE_OWNER_UNSUPPORTED")?;
                if !matches!((&request, response),
                    (DaemonRequest::Handshake { .. }, DaemonResponse::HandshakeOk { .. }) |
                    (DaemonRequest::MachineGateway, DaemonResponse::MachineGatewayOk)) {
                    return Err("MACHINE_OWNER_UNSUPPORTED".into());
                }
            }
            Ok(stream)
        }).await.map_err(|_| "TIMEOUT".to_owned())?
    }
}
