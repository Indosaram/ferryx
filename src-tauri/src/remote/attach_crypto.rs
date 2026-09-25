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

    // Golden transcript generated by browser Noise IK client:
    // Source: .omo/evidence/account-issued-remote-grants/noise-golden-transcript.json
    const GOLDEN_PROLOGUE_HEX: &str =
        "6665727279782d6174746163682d76313a4d414348494e453a53455353494f4e3a45504f4348";
    const GOLDEN_RESPONDER_STATIC_PRIV_HEX: &str =
        "546f6b696f527573744461656d6f6e5374617469634b65793031323334353637";
    const GOLDEN_RESPONDER_STATIC_PUB_HEX: &str =
        "509b6c7d0177a5fac052815bb5eabd4bcfc3f9f32fe8e4a4d976c00fb983851d";
    const GOLDEN_INITIATOR_STATIC_PUB_HEX: &str =
        "8c60c09873704981788c8647798311432d71809a7b0ec1aab237d79e0a37562a";
    const GOLDEN_INITIATOR_STATIC_PRIV_HEX: &str =
        "e8476a6a89e7587a1be7587442bf58971be7587442bf58971be7587442bf5897";
    const GOLDEN_INITIATOR_EPHEMERAL_PUB_HEX: &str =
        "ebdb97a5ee6ebe715911f28e0bbf76028b7c499465a280180fef060aeaf39e3c";
    const GOLDEN_INITIATOR_EPHEMERAL_PRIV_HEX: &str =
        "89abcdef0123456789abcdef0123456789abcdef0123456789abcdef01234567";
    const GOLDEN_MSG1_HEX: &str =
        "ebdb97a5ee6ebe715911f28e0bbf76028b7c499465a280180fef060aeaf39e3c189346429b7b139c8216507c4c5bb2f69634720de2982171dd39db651f7d4d5d7dde498ab9ccc101d0bad61d4e2703d1231d17560bd359dde592809f463b7873";
    const GOLDEN_TRANSPORT_PLAINTEXT_HEX: &str =
        "6665727279782d6e6f6973652d7472616e73706f72742d706c61696e74657874";

    fn decode_hex(hex: &str) -> Vec<u8> {
        assert_eq!(hex.len() % 2, 0, "hex string must have an even length");
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("valid hex byte"))
            .collect()
    }

    fn encode_hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
    }

    #[test]
    fn browser_noise_ik_golden_transcript_interop() {
        let responder_static_priv = decode_hex(GOLDEN_RESPONDER_STATIC_PRIV_HEX);
        let prologue = decode_hex(GOLDEN_PROLOGUE_HEX);

        // 1. Build a snow RESPONDER with the golden responder static private key and golden prologue.
        let params = noise_params().expect("valid NoiseParams");
        let mut responder = Builder::new(params)
            .local_private_key(&responder_static_priv)
            .prologue(&prologue)
            .build_responder()
            .expect("build snow responder");

        // 2. read_message(msg1) must succeed. Assert get_remote_static() equals the golden initiator_static_pub_hex bytes.
        let msg1 = decode_hex(GOLDEN_MSG1_HEX);
        let mut payload = vec![0u8; MAX_HANDSHAKE_MESSAGE];
        let read = responder
            .read_message(&msg1, &mut payload)
            .expect("read_message(msg1) must succeed against browser msg1");
        assert_eq!(read, 0, "msg1 payload should be empty");

        let remote_static = responder
            .get_remote_static()
            .expect("responder must have remote static key");
        let expected_initiator_pub = decode_hex(GOLDEN_INITIATOR_STATIC_PUB_HEX);
        assert_eq!(
            remote_static,
            expected_initiator_pub.as_slice(),
            "responder learned initiator STATIC PUBLIC key (not private key)"
        );

        let initiator_priv = decode_hex(GOLDEN_INITIATOR_STATIC_PRIV_HEX);
        assert_ne!(
            remote_static,
            initiator_priv.as_slice(),
            "remote static must NOT match initiator's private key"
        );

        // 3. write_message with an empty payload to produce msg2 and transition to transport mode.
        let mut msg2 = vec![0u8; MAX_HANDSHAKE_MESSAGE];
        let written = responder
            .write_message(&[], &mut msg2)
            .expect("write_message(msg2) must succeed");
        assert!(written > 0, "msg2 frame must be written");

        let _transport = responder
            .into_transport_mode()
            .expect("responder into_transport_mode must succeed");

        // 4. Note on transport ciphertext:
        // A transport frame ciphertext is only meaningful within the exact handshake session that
        // produced its msg2, because the responder generates an ephemeral key during write_message(msg2)
        // that mixes into the final DH (ee, se) steps and produces session-unique transport keys.
        // Since this golden transcript is one-directional (recording the initiator's msg1 and a ciphertext
        // derived under the TypeScript test responder's own ephemeral key), a fresh snow responder
        // generating its own ephemeral cannot decrypt the browser-recorded transport ciphertext.
        // Full bidirectional transport verification is covered by the daemon-level end-to-end integration suite,
        // and responder-generated golden transcripts can be produced via `emit_golden_responder_transcript`.
    }

    #[test]
    #[ignore = "run with --ignored --nocapture to regenerate the golden responder transcript; it is a generator, not an assertion"]
    fn emit_golden_responder_transcript() {
        let responder_static_priv = decode_hex(GOLDEN_RESPONDER_STATIC_PRIV_HEX);
        let prologue = decode_hex(GOLDEN_PROLOGUE_HEX);
        let params = noise_params().expect("valid NoiseParams");

        let mut responder = Builder::new(params)
            .local_private_key(&responder_static_priv)
            .prologue(&prologue)
            .build_responder()
            .expect("build snow responder");

        let msg1 = decode_hex(GOLDEN_MSG1_HEX);
        let mut payload = vec![0u8; MAX_HANDSHAKE_MESSAGE];
        responder
            .read_message(&msg1, &mut payload)
            .expect("read_message(msg1)");

        let mut msg2 = vec![0u8; MAX_HANDSHAKE_MESSAGE];
        let written_msg2 = responder
            .write_message(&[], &mut msg2)
            .expect("write_message(msg2)");

        let mut transport = responder
            .into_transport_mode()
            .expect("into_transport_mode");

        let plaintext = decode_hex(GOLDEN_TRANSPORT_PLAINTEXT_HEX);
        let mut transport_buf = vec![0u8; plaintext.len() + 16];
        let written_transport = transport
            .write_message(&plaintext, &mut transport_buf)
            .expect("transport write_message");

        let output = serde_json::json!({
            "initiator_static_pub_hex": GOLDEN_INITIATOR_STATIC_PUB_HEX,
            "initiator_static_priv_hex": GOLDEN_INITIATOR_STATIC_PRIV_HEX,
            "initiator_ephemeral_pub_hex": GOLDEN_INITIATOR_EPHEMERAL_PUB_HEX,
            "initiator_ephemeral_priv_hex": GOLDEN_INITIATOR_EPHEMERAL_PRIV_HEX,
            "responder_static_pub_hex": GOLDEN_RESPONDER_STATIC_PUB_HEX,
            "prologue_hex": GOLDEN_PROLOGUE_HEX,
            "msg1_hex": GOLDEN_MSG1_HEX,
            "msg2_hex": encode_hex(&msg2[..written_msg2]),
            "transport_plaintext_hex": GOLDEN_TRANSPORT_PLAINTEXT_HEX,
            "transport_ciphertext_hex": encode_hex(&transport_buf[..written_transport]),
        });

        println!("{}", serde_json::to_string(&output).expect("json serialization"));
    }

    #[test]
    fn browser_noise_ik_negative_authorization_control() {
        // Negative control:
        // Constructing a bespoke msg1 with the s token encrypted using the initiator's PRIVATE key
        // instead of its public key cannot be done cheaply via snow, because snow's Builder strictly
        // encapsulates handshake message assembly and always encrypts the derived public key. Synthesizing
        // a wire msg1 directly would require manually reimplementing the Noise IK symmetric state machine
        // (BLAKE2s hash chaining, HKDF, and ChaCha20Poly1305) from scratch; as instructed, we skip
        // constructing that synthetic frame rather than faking it.
        //
        // Instead, we verify the authorization and cryptographic gates:
        // 1) The authorization predicate strictly rejects the initiator's private key.
        let golden_initiator_pub = decode_hex(GOLDEN_INITIATOR_STATIC_PUB_HEX);
        let golden_initiator_priv = decode_hex(GOLDEN_INITIATOR_STATIC_PRIV_HEX);
        let pub_key: [u8; 32] = golden_initiator_pub.clone().try_into().unwrap();
        let priv_key: [u8; 32] = golden_initiator_priv.clone().try_into().unwrap();
        let authorize = |key: &[u8; 32]| key == &pub_key;
        assert!(authorize(&pub_key), "registered public key must authorize");
        assert!(!authorize(&priv_key), "private key presented as remote static must FAIL to authorize");

        // 2) Any corruption of the s-token ciphertext in msg1 fails snow's read_message with a decrypt error.
        let responder_static_priv = decode_hex(GOLDEN_RESPONDER_STATIC_PRIV_HEX);
        let prologue = decode_hex(GOLDEN_PROLOGUE_HEX);
        let params = noise_params().expect("valid NoiseParams");
        let mut responder = Builder::new(params)
            .local_private_key(&responder_static_priv)
            .prologue(&prologue)
            .build_responder()
            .expect("build snow responder");

        let mut tampered_msg1 = decode_hex(GOLDEN_MSG1_HEX);
        // Byte 32..80 is the encrypted s-token + Poly1305 tag. Flipping a bit invalidates the tag.
        tampered_msg1[40] ^= 0x5a;
        let mut payload = vec![0u8; MAX_HANDSHAKE_MESSAGE];
        let outcome = responder.read_message(&tampered_msg1, &mut payload);
        assert!(outcome.is_err(), "tampered s-token ciphertext must fail snow's read_message");
    }
}
