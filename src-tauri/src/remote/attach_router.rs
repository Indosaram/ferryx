use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use axum::{
    extract::{State, WebSocketUpgrade},
    response::Response,
    routing::get,
    Router,
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use futures_util::{Sink as _, Stream as _};

use crate::remote::session_transport::{
    establish_session, proxy_secure_to_gateway, SessionTransport,
};

pub type AttachAuthorizer = Arc<dyn Fn(&[u8]) -> bool + Send + Sync>;

pub struct AttachRouterDeps {
    pub gateway_addr: String,
    pub authorize: AttachAuthorizer,
}

pub fn attach_router(deps: Arc<AttachRouterDeps>) -> Router {
    Router::new()
        .route("/api/v1/attach", get(attach_ws))
        .with_state(deps)
}

async fn attach_ws(ws: WebSocketUpgrade, State(deps): State<Arc<AttachRouterDeps>>) -> Response {
    ws.on_upgrade(move |socket| serve_direct_attach(socket, deps))
}

async fn serve_direct_attach(
    socket: axum::extract::ws::WebSocket,
    deps: Arc<AttachRouterDeps>,
) {
    let Ok(attach) =
        crate::remote::attach_identity::load_or_generate_canonical_attach_identity()
    else {
        return;
    };
    let machine_id = crate::remote::auth::canonical_identity_dir()
        .ok()
        .and_then(|dir| crate::remote::auth::load_or_generate_machine_identity(&dir).ok())
        .map(|identity| identity.machine_id)
        .unwrap_or_default();
    let enrollment_epoch = crate::account::enroll_client::load_enrollment_record()
        .map(|record| record.enrollment_epoch)
        .unwrap_or_default();
    let Ok(gateway) = tokio::net::TcpStream::connect(&deps.gateway_addr).await else {
        return;
    };
    let stream = AxumWebSocketStream::new(socket);
    let opened = establish_session(
        stream,
        true,
        Some(&attach),
        &machine_id,
        "direct",
        &enrollment_epoch,
        {
            let authorize = Arc::clone(&deps.authorize);
            move |key| authorize(key)
        },
    )
    .await;
    if let Ok(SessionTransport::Attached(secure)) = opened {
        let _ = proxy_secure_to_gateway(secure, gateway).await;
    }
}

/// Bridges axum's websocket into the byte-stream shape the shared attach handshake needs.
struct AxumWebSocketStream {
    socket: axum::extract::ws::WebSocket,
    read_buffer: Vec<u8>,
    eof: bool,
}

impl AxumWebSocketStream {
    fn new(socket: axum::extract::ws::WebSocket) -> Self {
        Self {
            socket,
            read_buffer: Vec::new(),
            eof: false,
        }
    }
}

impl AsyncRead for AxumWebSocketStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        loop {
            if !self.read_buffer.is_empty() {
                let take = buf.remaining().min(self.read_buffer.len());
                let data: Vec<u8> = self.read_buffer.drain(..take).collect();
                buf.put_slice(&data);
                return Poll::Ready(Ok(()));
            }
            if self.eof {
                return Poll::Ready(Ok(()));
            }
            match Pin::new(&mut self.socket).poll_next(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(None) => {
                    self.eof = true;
                    return Poll::Ready(Ok(()));
                }
                Poll::Ready(Some(Err(error))) => {
                    return Poll::Ready(Err(std::io::Error::other(error)))
                }
                Poll::Ready(Some(Ok(message))) => match message {
                    axum::extract::ws::Message::Binary(data) => {
                        self.read_buffer.extend_from_slice(&data);
                    }
                    axum::extract::ws::Message::Text(text) => {
                        self.read_buffer.extend_from_slice(text.as_bytes());
                    }
                    axum::extract::ws::Message::Close(_) => {
                        self.eof = true;
                        return Poll::Ready(Ok(()));
                    }
                    _ => {}
                },
            }
        }
    }
}

impl AsyncWrite for AxumWebSocketStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match Pin::new(&mut self.socket).poll_ready(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(error)) => Poll::Ready(Err(std::io::Error::other(error))),
            Poll::Ready(Ok(())) => {
                match Pin::new(&mut self.socket)
                    .start_send(axum::extract::ws::Message::Binary(data.to_vec().into()))
                {
                    Ok(()) => Poll::Ready(Ok(data.len())),
                    Err(error) => Poll::Ready(Err(std::io::Error::other(error))),
                }
            }
        }
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.socket)
            .poll_flush(cx)
            .map_err(std::io::Error::other)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        let _ = Pin::new(&mut self.socket)
            .start_send(axum::extract::ws::Message::Close(None));
        Pin::new(&mut self.socket)
            .poll_flush(cx)
            .map_err(std::io::Error::other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::attach_client::{
        attach_opaque_session, load_or_generate_client_attach_identity,
    };
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn a_lan_attach_reaches_the_local_gateway_decrypted() {
        let client_dir = tempfile::tempdir().unwrap();
        let client_identity =
            load_or_generate_client_attach_identity(client_dir.path()).expect("client identity");
        let client_public = STANDARD
            .decode(&client_identity.public_key)
            .expect("client key bytes");
        let attach = crate::remote::attach_identity::load_or_generate_canonical_attach_identity()
            .expect("machine attach identity");
        let machine_public = attach.public_key.clone();
        let machine_id = crate::remote::auth::canonical_identity_dir()
            .ok()
            .and_then(|dir| crate::remote::auth::load_or_generate_machine_identity(&dir).ok())
            .map(|identity| identity.machine_id)
            .expect("machine identity");
        let enrollment_epoch = crate::account::enroll_client::load_enrollment_record()
            .map(|record| record.enrollment_epoch)
            .unwrap_or_default();

        let gateway_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let gateway_addr = gateway_listener.local_addr().unwrap().to_string();
        let (seen_tx, gateway_seen) = tokio::sync::oneshot::channel::<Vec<u8>>();
        tokio::spawn(async move {
            let (mut socket, _) = gateway_listener.accept().await.unwrap();
            let mut received = vec![0u8; 8];
            socket.read_exact(&mut received).await.unwrap();
            let _ = seen_tx.send(received);
        });

        let deps = Arc::new(AttachRouterDeps {
            gateway_addr: gateway_addr.clone(),
            authorize: Arc::new(move |key| key == &client_public),
        });
        let app = attach_router(deps);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = listener.local_addr().unwrap().to_string();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let url = format!("ws://{origin}/api/v1/attach");
        let (ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();
        let mut secure = attach_opaque_session(
            ws,
            &client_identity,
            &machine_public,
            &machine_id,
            "direct",
            &enrollment_epoch,
        )
        .await
        .expect("direct attach handshake");

        use tokio::io::AsyncWriteExt as _;
        secure.send_frame(b"sentinel").await.expect("send sentinel");

        let seen = tokio::time::timeout(std::time::Duration::from_secs(5), gateway_seen)
            .await
            .expect("gateway received bytes in time");
        assert_eq!(seen.expect("gateway read"), b"sentinel".to_vec());
    }
}
