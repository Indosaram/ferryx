//! The account attach client: every new attach path runs one handshake here before any gateway
//! plaintext exists, so anything in between - the relay above all - carries ciphertext only.

use std::path::Path;

use base64::{engine::general_purpose::STANDARD, Engine as _};

use super::attach_crypto::{AttachError, AttachInitiator, SecureStream, WebSocketByteStream};
use super::attach_identity::AttachIdentity;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AttachSession {
    pub session_id: String,
    pub machine_id: String,
    pub machine_attach_public_key: String,
    pub enrollment_epoch: String,
    pub relay_origin: String,
}

pub fn load_or_generate_client_attach_identity(base_dir: &Path) -> Result<AttachIdentity, String> {    super::attach_identity::load_or_generate_attach_identity(base_dir)
}

/// Completes the attach handshake on an already-open tunnel socket and returns the only stream
/// that carries this session's bytes in the clear.
pub async fn attach_opaque_session<S>(
    stream: tokio_tungstenite::WebSocketStream<S>,
    identity: &AttachIdentity,
    machine_attach_public_key: &str,
    machine_id: &str,
    session_id: &str,
    enrollment_epoch: &str,
) -> Result<SecureStream<WebSocketByteStream<S>>, AttachError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let private = STANDARD
        .decode(&identity.private_key)
        .map_err(|error| AttachError::HandshakeFailed(error.to_string()))?;
    let private: [u8; 32] = private
        .try_into()
        .map_err(|_| AttachError::HandshakeFailed("client attach key length".into()))?;
    let initiator = AttachInitiator::new(
        &private,
        machine_attach_public_key,
        machine_id,
        session_id,
        enrollment_epoch,
    )?;
    let (secure, _) = initiator.connect(WebSocketByteStream::new(stream)).await?;
    Ok(secure)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_client_attach_identity_round_trips_through_its_directory() {
        let dir = tempfile::tempdir().unwrap();
        let first = load_or_generate_client_attach_identity(dir.path()).expect("generate");
        let second = load_or_generate_client_attach_identity(dir.path()).expect("load");
        assert_eq!(first.public_key, second.public_key);
        assert_eq!(first.private_key, second.private_key);
        assert_eq!(STANDARD.decode(&first.private_key).unwrap().len(), 32);
    }
}
