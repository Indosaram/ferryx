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

use crate::remote::attach_crypto::{AttachError, AttachResponder, SecureStream};
use crate::remote::session_transport::proxy_secure_to_gateway;

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
    let mut stream = AxumWebSocketStream::new(socket);
    let accepted = match AttachResponder::new(&attach, &machine_id, "direct", &enrollment_epoch) {
        Ok(responder) => {
            let authorize = Arc::clone(&deps.authorize);
            // The handshake borrows the socket so a refusal can still close it below.
            responder
                .accept(&mut stream, move |key| authorize(key))
                .await
                .map(|(secure, _device_key)| secure.transport)
        }
        Err(error) => Err(error),
    };
    match accepted {
        Ok(transport) => {
            let secure = SecureStream {
                inner: stream,
                transport,
            };
            let _ = proxy_secure_to_gateway(secure, gateway).await;
        }
        // The peer left mid-handshake; there is nobody to tell.
        Err(AttachError::StreamClosed) => {}
        Err(_) => close_auth_failed(stream.socket).await,
    }
}

/// Ends a socket whose first-frame authentication failed with 1008 `AUTH_FAILED`, so the
/// client can tell a refused credential from a network drop.
async fn close_auth_failed(mut socket: axum::extract::ws::WebSocket) {
    let frame = axum::extract::ws::CloseFrame {
        code: axum::extract::ws::close_code::POLICY,
        reason: crate::remote::relay_server::AUTH_FAILED.into(),
    };
    let sent = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        socket.send(axum::extract::ws::Message::Close(Some(frame))),
    )
    .await;
    if !matches!(sent, Ok(Ok(()))) {
        tracing::debug!("direct attach auth-failure close failed or timed out");
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
    use tokio::io::AsyncReadExt;

    /// Both tests load or create the canonical machine and attach identities, and the
    /// machine identity has no file lock: two tests creating it at once on a fresh machine get
    /// different machine ids, so one handshake's prologue no longer matches.
    static CANONICAL_IDENTITY: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[tokio::test]
    async fn a_lan_attach_reaches_the_local_gateway_decrypted() {
        let _identity = CANONICAL_IDENTITY.lock().await;
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
            authorize: Arc::new(move |key| key == client_public),
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

        secure.send_frame(b"sentinel").await.expect("send sentinel");

        let seen = tokio::time::timeout(std::time::Duration::from_secs(5), gateway_seen)
            .await
            .expect("gateway received bytes in time");
        assert_eq!(seen.expect("gateway read"), b"sentinel".to_vec());
    }

    /// Reads frames until the close frame and returns its code and reason.
    async fn close_of<S>(ws: &mut tokio_tungstenite::WebSocketStream<S>) -> (u16, String)
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    {
        use futures_util::StreamExt as _;
        use tokio_tungstenite::tungstenite::Message;
        loop {
            let message = tokio::time::timeout(std::time::Duration::from_secs(5), ws.next())
                .await
                .expect("the attach socket closes in time")
                .expect("a close frame, not a bare end")
                .expect("no transport error");
            match message {
                Message::Close(Some(frame)) => {
                    return (u16::from(frame.code), frame.reason.to_string())
                }
                Message::Close(None) => panic!("close without a code"),
                _ => continue,
            }
        }
    }

    #[tokio::test]
    async fn a_refused_first_attach_frame_closes_with_policy_auth_failed() {
        use futures_util::SinkExt as _;
        use tokio_tungstenite::tungstenite::Message;

        let _identity = CANONICAL_IDENTITY.lock().await;
        let attach = crate::remote::attach_identity::load_or_generate_canonical_attach_identity()
            .expect("machine attach identity");
        let machine_public = STANDARD.decode(&attach.public_key).unwrap();
        let machine_id = crate::remote::auth::canonical_identity_dir()
            .ok()
            .and_then(|dir| crate::remote::auth::load_or_generate_machine_identity(&dir).ok())
            .map(|identity| identity.machine_id)
            .expect("machine identity");
        let enrollment_epoch = crate::account::enroll_client::load_enrollment_record()
            .map(|record| record.enrollment_epoch)
            .unwrap_or_default();

        // The gateway is never reached: authentication fails first.
        let gateway_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let deps = Arc::new(AttachRouterDeps {
            gateway_addr: gateway_listener.local_addr().unwrap().to_string(),
            authorize: Arc::new(|_| false),
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, attach_router(deps)).await.unwrap();
        });
        let url = format!("ws://{addr}/api/v1/attach");

        // A well-formed Noise IK first message from a key no device of this machine owns.
        let params: snow::params::NoiseParams =
            "Noise_IK_25519_ChaChaPoly_BLAKE2s".parse().unwrap();
        let prologue = format!(
            "{}:{machine_id}:direct:{enrollment_epoch}",
            crate::remote::attach_crypto::ATTACH_PROLOGUE_PREFIX
        );
        let mut initiator = snow::Builder::new(params)
            .local_private_key(&[7u8; 32])
            .remote_public_key(&machine_public)
            .prologue(prologue.as_bytes())
            .build_initiator()
            .unwrap();
        let mut first = vec![0u8; 1024];
        let written = initiator.write_message(&[], &mut first).unwrap();
        let mut framed = (written as u32).to_be_bytes().to_vec();
        framed.extend_from_slice(&first[..written]);

        // A garbage first frame is refused the same way.
        let mut garbage = 8u32.to_be_bytes().to_vec();
        garbage.extend_from_slice(b"not-ikv1");

        for first_frame in [framed, garbage] {
            let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
            ws.send(Message::Binary(first_frame.into())).await.unwrap();
            assert_eq!(close_of(&mut ws).await, (1008, "AUTH_FAILED".to_string()));
        }
    }
}
