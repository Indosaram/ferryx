use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use super::attach_crypto::{AttachError, AttachResponder, SecureStream};
use super::attach_identity::AttachIdentity;

/// What a relay data session turned out to be: the raw stream (legacy HTTP proxy
/// traffic) or a stream that already completed the attach handshake.
pub enum SessionTransport<S> {
    Plain(S),
    Attached(SecureStream<S>),
}

impl<S: AsyncRead + AsyncWrite + Unpin> SessionTransport<S> {
    pub fn is_attached(&self) -> bool {
        matches!(self, Self::Attached(_))
    }
}

/// Decides how a data session is carried. `opaque` comes from the control notice that
/// announced the session; `authorize` receives the initiator's static attach key and is
/// the caller's policy for whether that device may attach to this machine.
pub async fn establish_session<S, F>(
    stream: S,
    opaque: bool,
    attach: Option<&AttachIdentity>,
    machine_id: &str,
    session_id: &str,
    enrollment_epoch: &str,
    authorize: F,
) -> Result<SessionTransport<S>, AttachError>
where
    S: AsyncRead + AsyncWrite + Unpin,
    F: Fn(&[u8; 32]) -> bool + Send,
{
    if !opaque {
        return Ok(SessionTransport::Plain(stream));
    }
    let attach = attach.ok_or(AttachError::BadKeyEncoding)?;
    let responder = AttachResponder::new(attach, machine_id, session_id, enrollment_epoch)?;
    let (secure, device_key) = responder.accept(stream, authorize).await?;
    let _ = device_key;
    Ok(SessionTransport::Attached(secure))
}

/// Carries a completed attach session to the local gateway: frames arriving from the
/// peer are written to `gateway`, and bytes read from `gateway` are sent back as frames.
/// Nothing is interpreted on the way through, so the gateway only ever sees plaintext.
pub async fn proxy_secure_to_gateway<S, T>(
    secure: SecureStream<S>,
    gateway: T,
) -> Result<(), AttachError>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (mut stream_rx, mut stream_tx) = tokio::io::split(secure.inner);
    let (mut gateway_rx, mut gateway_tx) = tokio::io::split(gateway);
    let transport = std::sync::Arc::new(std::sync::Mutex::new(secure.transport));

    let t_rx = transport.clone();
    let to_gateway = async move {
        loop {
            let frame = crate::remote::attach_crypto::read_frame(
                &mut stream_rx,
                crate::remote::attach_crypto::MAX_ATTACH_FRAME + 16,
            )
            .await?;
            let mut plaintext = vec![0u8; frame.len().max(1)];
            let read = {
                let mut t = t_rx.lock().unwrap();
                t.read_message(&frame, &mut plaintext)
                    .map_err(|e| AttachError::DecryptFailed(e.to_string()))?
            };
            plaintext.truncate(read);
            gateway_tx
                .write_all(&plaintext)
                .await
                .map_err(|_| AttachError::StreamClosed)?;
        }
        #[allow(unreachable_code)]
        Ok::<(), AttachError>(())
    };

    let t_tx = transport;
    let from_gateway = async move {
        let mut buffer = vec![0u8; 16 * 1024];
        loop {
            let n = gateway_rx
                .read(&mut buffer)
                .await
                .map_err(|_| AttachError::StreamClosed)?;
            if n == 0 {
                break;
            }
            let plaintext = &buffer[..n];
            let mut cipher = vec![0u8; plaintext.len() + 16];
            let written = {
                let mut t = t_tx.lock().unwrap();
                t.write_message(plaintext, &mut cipher)
                    .map_err(|e| AttachError::DecryptFailed(e.to_string()))?
            };
            crate::remote::attach_crypto::write_frame(&mut stream_tx, &cipher[..written]).await?;
        }
        Ok::<(), AttachError>(())
    };

    tokio::select! {
        res1 = to_gateway => res1,
        res2 = from_gateway => res2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::attach_crypto::AttachInitiator;
    use base64::{engine::general_purpose::STANDARD, Engine as _};

    fn attach_identity() -> AttachIdentity {
        let secret = x25519_dalek::StaticSecret::random_from_rng(rand::rngs::OsRng);
        AttachIdentity {
            public_key: STANDARD.encode(x25519_dalek::PublicKey::from(&secret).as_bytes()),
            private_key: STANDARD.encode(secret.to_bytes()),
        }
    }

    #[tokio::test]
    async fn a_legacy_session_stays_plain() {
        let (client, server) = tokio::io::duplex(4096);
        let transport = establish_session(server, false, None, "machine-1", "s1", "1", |_| true)
            .await
            .expect("plain session");
        assert!(!transport.is_attached());
        drop(client);
    }

    #[tokio::test]
    async fn an_opaque_session_only_carries_decrypted_bytes() {
        let machine = attach_identity();
        let device_secret = x25519_dalek::StaticSecret::random_from_rng(rand::rngs::OsRng);
        let device_public = STANDARD.encode(x25519_dalek::PublicKey::from(&device_secret).as_bytes());
        let device_private = device_secret.to_bytes();

        let (client, server) = tokio::io::duplex(64 * 1024);
        let responder_task = tokio::spawn({
            let machine = machine.clone();
            async move {
                establish_session(server, true, Some(&machine), "machine-1", "s1", "2", {
                    let expected = STANDARD.decode(&device_public).expect("device key");
                    move |key: &[u8; 32]| key.as_slice() == expected.as_slice()
                })
                .await
            }
        });

        let initiator = AttachInitiator::new(&device_private, &machine.public_key, "machine-1", "s1", "2")
            .expect("initiator");
        let (mut secure, _) = initiator.connect(client).await.expect("handshake");
        secure.send_frame(b"GET / HTTP/1.1\r\n").await.expect("send");

        let transport = responder_task.await.expect("join").expect("opaque session");
        assert!(transport.is_attached());
        let SessionTransport::Attached(mut secure_server) = transport else {
            panic!("expected an attached transport");
        };
        let received = secure_server.recv_frame().await.expect("recv");
        assert_eq!(received, b"GET / HTTP/1.1\r\n");
    }

    #[tokio::test]
    async fn an_opaque_session_refuses_an_unknown_device() {
        let machine = attach_identity();
        let stranger_secret = x25519_dalek::StaticSecret::random_from_rng(rand::rngs::OsRng);
        let stranger_private = stranger_secret.to_bytes();

        let (client, server) = tokio::io::duplex(64 * 1024);
        let responder_task = tokio::spawn({
            let machine = machine.clone();
            async move {
                establish_session(server, true, Some(&machine), "machine-1", "s1", "2", |_| false).await
            }
        });

        let initiator = AttachInitiator::new(&stranger_private, &machine.public_key, "machine-1", "s1", "2")
            .expect("initiator");
        let outcome = initiator.connect(client).await;
        let responder = responder_task.await.expect("join");
        assert!(outcome.is_err() || responder.is_err(), "an unknown device must not attach");
    }

    #[tokio::test]
    async fn a_proxied_session_moves_plaintext_both_ways() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let machine = attach_identity();
        let device_secret = x25519_dalek::StaticSecret::random_from_rng(rand::rngs::OsRng);
        let device_private = device_secret.to_bytes();

        let (client, server) = tokio::io::duplex(64 * 1024);
        let responder_task = tokio::spawn({
            let machine = machine.clone();
            async move {
                establish_session(server, true, Some(&machine), "machine-1", "s1", "2", |_| true).await
            }
        });
        let initiator = AttachInitiator::new(&device_private, &machine.public_key, "machine-1", "s1", "2")
            .expect("initiator");
        let (mut secure_client, _) = initiator.connect(client).await.expect("handshake");

        let transport = responder_task.await.expect("join").expect("session");
        let SessionTransport::Attached(secure_server) = transport else {
            panic!("expected an attached transport");
        };
        let (mut gateway_peer, gateway_local) = tokio::io::duplex(64 * 1024);
        let proxy = tokio::spawn(async move { proxy_secure_to_gateway(secure_server, gateway_local).await });

        secure_client
            .send_frame(b"GET / HTTP/1.1\r\n")
            .await
            .expect("client frame");
        let mut from_peer = Vec::new();
        let mut buffer = [0u8; 64];
        while from_peer.len() < 16 {
            let read = tokio::time::timeout(Duration::from_secs(5), gateway_peer.read(&mut buffer))
                .await
                .expect("gateway read timed out")
                .expect("gateway read");
            assert!(read > 0, "gateway closed early");
            from_peer.extend_from_slice(&buffer[..read]);
        }
        assert_eq!(&from_peer[..16], b"GET / HTTP/1.1\r\n");

        gateway_peer
            .write_all(b"HTTP/1.1 200 OK\r\n")
            .await
            .expect("gateway write");
        let reply = tokio::time::timeout(Duration::from_secs(5), secure_client.recv_frame())
            .await
            .expect("client read timed out")
            .expect("client read");
        assert_eq!(reply, b"HTTP/1.1 200 OK\r\n");

        proxy.abort();
    }

    #[tokio::test]
    async fn an_opaque_session_without_an_attach_key_fails() {
        let (client, server) = tokio::io::duplex(4096);
        let outcome = establish_session(server, true, None, "machine-1", "s1", "2", |_| true).await;
        assert!(outcome.is_err());
        drop(client);
    }
}
