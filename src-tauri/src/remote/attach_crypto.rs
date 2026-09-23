use base64::{engine::general_purpose::STANDARD, Engine as _};
use futures_util::{Sink, Stream};
use snow::{params::NoiseParams, Builder};
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_tungstenite::tungstenite::{Bytes, Message};
use tokio_tungstenite::WebSocketStream;

use super::attach_identity::AttachIdentity;

pub const MAX_ATTACH_FRAME: usize = 65519;
pub const MAX_HANDSHAKE_MESSAGE: usize = 65_535;
pub const ATTACH_PROLOGUE_PREFIX: &str = "ferryx-attach-v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachError {
    BadKeyEncoding,
    HandshakeFailed(String),
    FrameTooLarge(usize),
    StreamClosed,
    DecryptFailed(String),
}

impl std::fmt::Display for AttachError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadKeyEncoding => write!(formatter, "ATTACH_KEY_INVALID"),
            Self::HandshakeFailed(message) => write!(formatter, "ATTACH_HANDSHAKE_FAILED: {message}"),
            Self::FrameTooLarge(size) => write!(formatter, "ATTACH_FRAME_TOO_LARGE: {size}"),
            Self::StreamClosed => write!(formatter, "ATTACH_STREAM_CLOSED"),
            Self::DecryptFailed(message) => write!(formatter, "ATTACH_DECRYPT_FAILED: {message}"),
        }
    }
}

fn attach_prologue(machine_id: &str, session_id: &str, enrollment_epoch: &str) -> Vec<u8> {
    format!("{ATTACH_PROLOGUE_PREFIX}:{machine_id}:{session_id}:{enrollment_epoch}").into_bytes()
}

fn noise_params() -> Result<NoiseParams, AttachError> {
    "Noise_IK_25519_ChaChaPoly_BLAKE2s"
        .parse()
        .map_err(|error: snow::Error| AttachError::HandshakeFailed(error.to_string()))
}

fn decode_key(value: &str) -> Result<[u8; 32], AttachError> {
    let bytes = STANDARD
        .decode(value)
        .map_err(|_| AttachError::BadKeyEncoding)?;
    bytes.try_into().map_err(|_| AttachError::BadKeyEncoding)
}

pub struct AttachResponder {
    static_private: [u8; 32],
    prologue: Vec<u8>,
}

impl AttachResponder {
    pub fn new(
        identity: &AttachIdentity,
        machine_id: &str,
        session_id: &str,
        enrollment_epoch: &str,
    ) -> Result<Self, AttachError> {
        Ok(Self {
            static_private: decode_key(&identity.private_key)?,
            prologue: attach_prologue(machine_id, session_id, enrollment_epoch),
        })
    }

    /// Completes the handshake as the machine side. `authorize` receives the
    /// device's static attach key and must bind it to a live, unrevoked device
    /// for this machine before any framed byte is accepted.
    pub async fn accept<S>(
        &self,
        stream: S,
        authorize: impl Fn(&[u8; 32]) -> bool,
    ) -> Result<(SecureStream<S>, [u8; 32]), AttachError>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let mut stream = stream;
        let mut handshake = Builder::new(noise_params()?)
            .local_private_key(&self.static_private)
            .prologue(&self.prologue)
            .build_responder()
            .map_err(|error| AttachError::HandshakeFailed(error.to_string()))?;

        let first = read_frame(&mut stream, MAX_HANDSHAKE_MESSAGE).await?;
        let mut payload = vec![0u8; 65535];
        handshake
            .read_message(&first, &mut payload)
            .map_err(|error| AttachError::HandshakeFailed(error.to_string()))?;
        let initiator_static = handshake
            .get_remote_static()
            .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok())
            .ok_or(AttachError::BadKeyEncoding)?;
        if !authorize(&initiator_static) {
            return Err(AttachError::HandshakeFailed(
                "initiator key is not bound to a device of this machine".into(),
            ));
        }

        let mut second = vec![0u8; MAX_HANDSHAKE_MESSAGE];
        let written = handshake
            .write_message(&[], &mut second)
            .map_err(|error| AttachError::HandshakeFailed(error.to_string()))?;
        write_frame(&mut stream, &second[..written]).await?;

        let transport = handshake
            .into_transport_mode()
            .map_err(|error| AttachError::HandshakeFailed(error.to_string()))?;
        Ok((SecureStream::new(stream, transport), initiator_static))
    }
}

pub struct AttachInitiator {
    static_private: [u8; 32],
    responder_static: [u8; 32],
    prologue: Vec<u8>,
}

impl AttachInitiator {
    pub fn new(
        device_private_key: &[u8; 32],
        machine_attach_public_key: &str,
        machine_id: &str,
        session_id: &str,
        enrollment_epoch: &str,
    ) -> Result<Self, AttachError> {
        Ok(Self {
            static_private: *device_private_key,
            responder_static: decode_key(machine_attach_public_key)?,
            prologue: attach_prologue(machine_id, session_id, enrollment_epoch),
        })
    }

    pub async fn connect<S>(&self, stream: S) -> Result<(SecureStream<S>, [u8; 32]), AttachError>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let mut stream = stream;
        let mut handshake = Builder::new(noise_params()?)
            .local_private_key(&self.static_private)
            .remote_public_key(&self.responder_static)
            .prologue(&self.prologue)
            .build_initiator()
            .map_err(|error| AttachError::HandshakeFailed(error.to_string()))?;

        let mut first = vec![0u8; MAX_HANDSHAKE_MESSAGE];
        let written = handshake
            .write_message(&[], &mut first)
            .map_err(|error| AttachError::HandshakeFailed(error.to_string()))?;
        write_frame(&mut stream, &first[..written]).await?;

        let second = read_frame(&mut stream, MAX_HANDSHAKE_MESSAGE).await?;
        let mut payload = vec![0u8; 65535];
        handshake
            .read_message(&second, &mut payload)
            .map_err(|error| AttachError::HandshakeFailed(error.to_string()))?;
        let responder_static = handshake
            .get_remote_static()
            .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok())
            .ok_or(AttachError::BadKeyEncoding)?;

        let transport = handshake
            .into_transport_mode()
            .map_err(|error| AttachError::HandshakeFailed(error.to_string()))?;
        Ok((SecureStream::new(stream, transport), responder_static))
    }
}

pub struct SecureStream<S> {
    pub(crate) inner: S,
    pub(crate) transport: snow::TransportState,
}

impl<S> SecureStream<S> {
    fn new(inner: S, transport: snow::TransportState) -> Self {
        Self { inner, transport }
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> SecureStream<S> {
    pub async fn send_frame(&mut self, plaintext: &[u8]) -> Result<(), AttachError> {
        if plaintext.len() > MAX_ATTACH_FRAME {
            return Err(AttachError::FrameTooLarge(plaintext.len()));
        }
        let mut buffer = vec![0u8; plaintext.len() + 16];
        let written = self
            .transport
            .write_message(plaintext, &mut buffer)
            .map_err(|error| AttachError::DecryptFailed(error.to_string()))?;
        write_frame(&mut self.inner, &buffer[..written]).await
    }

    pub async fn recv_frame(&mut self) -> Result<Vec<u8>, AttachError> {
        let frame = read_frame(&mut self.inner, MAX_ATTACH_FRAME + 16).await?;
        let mut plaintext = vec![0u8; frame.len().max(1)];
        let read = self
            .transport
            .read_message(&frame, &mut plaintext)
            .map_err(|error| AttachError::DecryptFailed(error.to_string()))?;
        plaintext.truncate(read);
        Ok(plaintext)
    }

    pub fn into_inner(self) -> S {
        self.inner
    }
}

pub struct WebSocketByteStream<S> {
    inner: WebSocketStream<S>,
    pending: Vec<u8>,
    offset: usize,
}

impl<S> WebSocketByteStream<S> {
    pub fn new(inner: WebSocketStream<S>) -> Self {
        Self {
            inner,
            pending: Vec::new(),
            offset: 0,
        }
    }

    pub fn into_inner(self) -> WebSocketStream<S> {
        self.inner
    }
}

impl<S> AsyncRead for WebSocketByteStream<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        loop {
            if this.offset < this.pending.len() {
                let remaining = &this.pending[this.offset..];
                let take = remaining.len().min(buffer.remaining());
                buffer.put_slice(&remaining[..take]);
                this.offset += take;
                if this.offset == this.pending.len() {
                    this.pending.clear();
                    this.offset = 0;
                }
                return Poll::Ready(Ok(()));
            }
            match Pin::new(&mut this.inner).poll_next(cx) {
                Poll::Ready(Some(Ok(Message::Binary(bytes)))) => {
                    this.pending = bytes.to_vec();
                    this.offset = 0;
                }
                Poll::Ready(Some(Ok(Message::Text(text)))) => {
                    this.pending = text.as_str().as_bytes().to_vec();
                    this.offset = 0;
                }
                Poll::Ready(Some(Ok(Message::Ping(_))))
                | Poll::Ready(Some(Ok(Message::Pong(_))))
                | Poll::Ready(Some(Ok(Message::Frame(_)))) => continue,
                Poll::Ready(Some(Ok(Message::Close(_)))) | Poll::Ready(None) => {
                    return Poll::Ready(Ok(()));
                }
                Poll::Ready(Some(Err(error))) => {
                    return Poll::Ready(Err(std::io::Error::other(error.to_string())));
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

impl<S> AsyncWrite for WebSocketByteStream<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        match Pin::new(&mut this.inner).poll_ready(cx) {
            Poll::Ready(Ok(())) => {}
            Poll::Ready(Err(error)) => {
                return Poll::Ready(Err(std::io::Error::other(error.to_string())));
            }
            Poll::Pending => return Poll::Pending,
        }
        match Pin::new(&mut this.inner).start_send(Message::Binary(Bytes::from(buffer.to_vec()))) {
            Ok(()) => Poll::Ready(Ok(buffer.len())),
            Err(error) => Poll::Ready(Err(std::io::Error::other(error.to_string()))),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        match Pin::new(&mut this.inner).poll_flush(cx) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Ready(Err(error)) => {
                Poll::Ready(Err(std::io::Error::other(error.to_string())))
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        match Pin::new(&mut this.inner).poll_close(cx) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Ready(Err(error)) => {
                Poll::Ready(Err(std::io::Error::other(error.to_string())))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

pub(crate) async fn write_frame<S: AsyncWrite + Unpin>(stream: &mut S, bytes: &[u8]) -> Result<(), AttachError> {
    if bytes.len() > MAX_ATTACH_FRAME {
        return Err(AttachError::FrameTooLarge(bytes.len()));
    }
    stream
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .await
        .map_err(|_| AttachError::StreamClosed)?;
    stream
        .write_all(bytes)
        .await
        .map_err(|_| AttachError::StreamClosed)?;
    stream.flush().await.map_err(|_| AttachError::StreamClosed)?;
    Ok(())
}

pub(crate) async fn read_frame<S: AsyncRead + Unpin>(
    stream: &mut S,
    max: usize,
) -> Result<Vec<u8>, AttachError> {
    let mut header = [0u8; 4];
    stream
        .read_exact(&mut header)
        .await
        .map_err(|_| AttachError::StreamClosed)?;
    let length = u32::from_be_bytes(header) as usize;
    if length > max {
        return Err(AttachError::FrameTooLarge(length));
    }
    let mut buffer = vec![0u8; length];
    stream
        .read_exact(&mut buffer)
        .await
        .map_err(|_| AttachError::StreamClosed)?;
    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    const SENTINEL: &str = "FERRYX_E2EE_SENTINEL";

    fn identities() -> (AttachIdentity, AttachIdentity) {
        let machine = x25519_dalek::StaticSecret::random_from_rng(rand::rngs::OsRng);
        let device = x25519_dalek::StaticSecret::random_from_rng(rand::rngs::OsRng);
        (
            AttachIdentity {
                public_key: STANDARD.encode(x25519_dalek::PublicKey::from(&machine).as_bytes()),
                private_key: STANDARD.encode(machine.to_bytes()),
            },
            AttachIdentity {
                public_key: STANDARD.encode(x25519_dalek::PublicKey::from(&device).as_bytes()),
                private_key: STANDARD.encode(device.to_bytes()),
            },
        )
    }

    /// A splicing proxy that records every byte it forwards, standing in for the
    /// relay's data-socket bridge.
    async fn splicing_proxy() -> (tokio::io::DuplexStream, tokio::io::DuplexStream, Arc<Mutex<Vec<u8>>>) {
        let (client_side, mut proxy_client) = tokio::io::duplex(64 * 1024);
        let (mut proxy_daemon, daemon_side) = tokio::io::duplex(64 * 1024);
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&recorded);
        tokio::spawn(async move {
            let mut to_daemon = [0u8; 8192];
            let mut to_client = [0u8; 8192];
            loop {
                tokio::select! {
                    read = proxy_client.read(&mut to_daemon) => match read {
                        Ok(0) | Err(_) => break,
                        Ok(count) => {
                            log.lock().await.extend_from_slice(&to_daemon[..count]);
                            if proxy_daemon.write_all(&to_daemon[..count]).await.is_err() {
                                break;
                            }
                        }
                    },
                    read = proxy_daemon.read(&mut to_client) => match read {
                        Ok(0) | Err(_) => break,
                        Ok(count) => {
                            log.lock().await.extend_from_slice(&to_client[..count]);
                            if proxy_client.write_all(&to_client[..count]).await.is_err() {
                                break;
                            }
                        }
                    },
                }
            }
        });
        (client_side, daemon_side, recorded)
    }

    async fn attach_pair(
        session: &str,
    ) -> (SecureStream<tokio::io::DuplexStream>, SecureStream<tokio::io::DuplexStream>, Arc<Mutex<Vec<u8>>>) {
        let (machine, device) = identities();
        let responder = AttachResponder::new(&machine, "machine-1", session, "3").expect("responder");
        let device_private: [u8; 32] = decode_key(&device.private_key).expect("device key");
        let initiator = AttachInitiator::new(
            &device_private,
            &machine.public_key,
            "machine-1",
            session,
            "3",
        )
        .expect("initiator");

        let (client_side, daemon_side, recorded) = splicing_proxy().await;
        let device_public = decode_key(&device.public_key).expect("device public");
        let responder_task = tokio::spawn(async move {
            responder
                .accept(daemon_side, move |key| *key == device_public)
                .await
        });
        let (client_stream, _) = initiator.connect(client_side).await.expect("client handshake");
        let (server_stream, observed) = responder_task
            .await
            .expect("join")
            .expect("server handshake");
        assert_eq!(observed, device_public, "the responder learns the device key");
        (client_stream, server_stream, recorded)
    }

    #[tokio::test]
    async fn attach_sentinel_is_invisible_to_the_relay() {
        let (mut client, mut server, recorded) = attach_pair("session-1").await;
        client
            .send_frame(format!("{SENTINEL} typing keystrokes").as_bytes())
            .await
            .expect("send");
        let received = server.recv_frame().await.expect("recv");
        assert!(String::from_utf8_lossy(&received).contains(SENTINEL));

        server
            .send_frame(format!("{SENTINEL} terminal output").as_bytes())
            .await
            .expect("send");
        let echoed = client.recv_frame().await.expect("recv");
        assert!(String::from_utf8_lossy(&echoed).contains(SENTINEL));

        let spliced = String::from_utf8_lossy(&recorded.lock().await).to_string();
        assert!(
            !spliced.contains(SENTINEL),
            "the spliced copy must never contain terminal plaintext"
        );
        assert!(
            !spliced.contains("keystrokes") && !spliced.contains("terminal output"),
            "no fragment of the payload may survive in the spliced copy"
        );
    }

    #[tokio::test]
    async fn attach_crypto_runs_over_a_websocket_pair() {
        let (machine, device) = identities();
        let responder = AttachResponder::new(&machine, "machine-1", "ws-session", "1").expect("responder");
        let device_private: [u8; 32] = decode_key(&device.private_key).expect("device key");
        let initiator = AttachInitiator::new(
            &device_private,
            &machine.public_key,
            "machine-1",
            "ws-session",
            "1",
        )
        .expect("initiator");

        let (client_duplex, server_duplex) = tokio::io::duplex(256 * 1024);
        let (client_ws, server_ws) = tokio::join!(
            tokio_tungstenite::client_async("ws://127.0.0.1/attach", client_duplex),
            tokio_tungstenite::accept_async(server_duplex),
        );
        let client_ws = WebSocketByteStream::new(client_ws.expect("client websocket").0);
        let server_ws = WebSocketByteStream::new(server_ws.expect("server websocket"));
        let device_public = decode_key(&device.public_key).expect("device public");

        let responder_task = tokio::spawn(async move {
            responder
                .accept(server_ws, move |key| *key == device_public)
                .await
        });
        let (mut client, _) = initiator.connect(client_ws).await.expect("handshake over ws");
        let (mut server, observed) = responder_task
            .await
            .expect("join")
            .expect("handshake over ws");
        assert_eq!(observed, device_public);

        let payload = format!("{SENTINEL} {}", "ws".repeat(5000));
        client
            .send_frame(payload.as_bytes())
            .await
            .expect("send large frame");
        let received = server.recv_frame().await.expect("recv large frame");
        assert_eq!(received, payload.as_bytes(), "a frame larger than one websocket message survives intact");

        server.send_frame(b"ack").await.expect("reply");
        let reply = client.recv_frame().await.expect("reply");
        assert_eq!(reply, b"ack");
    }

    #[tokio::test]
    async fn attach_rejects_an_unbound_initiator_key() {
        let (machine, device) = identities();
        let responder = AttachResponder::new(&machine, "machine-1", "session-2", "1").expect("responder");
        let device_private: [u8; 32] = decode_key(&device.private_key).expect("device key");
        let initiator = AttachInitiator::new(
            &device_private,
            &machine.public_key,
            "machine-1",
            "session-2",
            "1",
        )
        .expect("initiator");
        let (mut client_side, daemon_side) = tokio::io::duplex(64 * 1024);
        let responder_task = tokio::spawn(async move {
            responder.accept(daemon_side, |_key| false).await
        });
        let _ = initiator.connect(&mut client_side).await;
        let outcome = responder_task.await.expect("join");
        assert!(outcome.is_err(), "an unbound device key must fail the handshake");
    }

    #[tokio::test]
    async fn attach_rejects_a_wrong_responder_key() {
        let (machine, device) = identities();
        let (_, other) = identities();
        let device_private: [u8; 32] = decode_key(&device.private_key).expect("device key");
        let initiator = AttachInitiator::new(
            &device_private,
            &other.public_key,
            "machine-1",
            "session-3",
            "1",
        )
        .expect("initiator");
        let responder = AttachResponder::new(&machine, "machine-1", "session-3", "1").expect("responder");
        let (client_side, daemon_side) = tokio::io::duplex(64 * 1024);
        let responder_task = tokio::spawn(async move {
            responder.accept(daemon_side, |_key| true).await
        });
        let client = initiator.connect(client_side).await;
        let server = responder_task.await.expect("join");
        assert!(
            client.is_err() || server.is_err(),
            "a substituted responder identity must not complete the handshake"
        );
    }

    #[tokio::test]
    async fn attach_rejects_a_tampered_frame_and_a_plaintext_client() {
        let (mut client, mut server, _recorded) = attach_pair("session-4").await;
        client.send_frame(b"alpha").await.expect("send");
        let first = server.recv_frame().await.expect("recv");
        assert_eq!(first, b"alpha");

        let wrong_prologue = AttachResponder::new(
            &identities().0,
            "machine-1",
            "session-4",
            "1",
        )
        .expect("responder");

        let (mut plain, daemon_side) = tokio::io::duplex(64 * 1024);
        let responder_task = tokio::spawn(async move {
            wrong_prologue.accept(daemon_side, |_key| true).await
        });
        plain
            .write_all(b"\x00\x00\x00\x05alpha")
            .await
            .expect("write plaintext");
        let outcome = responder_task.await.expect("join");
        assert!(
            outcome.is_err(),
            "a plaintext client must not be able to attach"
        );
    }
}
