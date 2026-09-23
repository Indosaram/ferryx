use std::path::Path;

use crate::remote::attach_client::{attach_opaque_session, load_or_generate_client_attach_identity};
use crate::remote::attach_crypto::{SecureStream, WebSocketByteStream};

pub type RelayAttachedStream =
    SecureStream<WebSocketByteStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>>;

pub async fn attach_relay_session(
    relay_origin: &str,
    session_id: &str,
    machine_id: &str,
    machine_attach_public_key: &str,
    enrollment_epoch: &str,
    identity_dir: &Path,
) -> anyhow::Result<RelayAttachedStream> {
    let identity = load_or_generate_client_attach_identity(identity_dir)
        .map_err(|error| anyhow::anyhow!(error))?;
    let ws_origin = if relay_origin.starts_with("http://") {
        relay_origin.replacen("http://", "ws://", 1)
    } else if relay_origin.starts_with("https://") {
        relay_origin.replacen("https://", "wss://", 1)
    } else {
        relay_origin.to_string()
    };
    let url = format!(
        "{}/tunnel/opaque/{session_id}",
        ws_origin.trim_end_matches('/')
    );
    let (stream, _) = tokio_tungstenite::connect_async(url).await?;
    attach_opaque_session(
        stream,
        &identity,
        machine_attach_public_key,
        machine_id,
        session_id,
        enrollment_epoch,
    )
    .await
    .map_err(|error| anyhow::anyhow!(error.to_string()))
}
