use std::path::Path;

use crate::paired_host::path_select::{
    confirm_attach_origin, resolve_attach_base_origin, AttachPath, CandidatePath,
};
use crate::remote::attach_client::{attach_opaque_session, load_or_generate_client_attach_identity};
use crate::remote::attach_crypto::{SecureStream, WebSocketByteStream};

pub type RelayAttachedStream =
    SecureStream<WebSocketByteStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>>;

pub fn build_host_candidates(
    host: &crate::paired_host::inventory::HostView,
    device_token: Option<&str>,
) -> Vec<CandidatePath> {
    let mut candidates = Vec::new();

    candidates.push(CandidatePath::with_expected_machine_id(
        AttachPath::Relay,
        &host.relay_origin,
        device_token.map(str::to_owned),
        Some(host.machine_id.clone()),
    ));

    if host.ssh_host_id.is_some() {
        candidates.push(CandidatePath::with_expected_machine_id(
            AttachPath::SshForward,
            format!(
                "http://127.0.0.1:{}",
                crate::remote::discovery::tailscale::DEFAULT_FERRYX_PORT
            ),
            device_token.map(str::to_owned),
            Some(host.machine_id.clone()),
        ));
    }

    candidates
}

/// Attaches to a paired session using candidate paths, confirming the chosen origin on success.
///
/// Note: The caller must call `release_attach_origin(session_id)` when the session stream ends.
pub async fn attach_paired_session_with_candidates(
    relay_origin: &str,
    session_id: &str,
    machine_id: &str,
    machine_attach_public_key: &str,
    enrollment_epoch: &str,
    identity_dir: &Path,
    candidates: &[CandidatePath],
    http_client: Option<&reqwest::Client>,
) -> anyhow::Result<RelayAttachedStream> {
    let identity = load_or_generate_client_attach_identity(identity_dir)
        .map_err(|error| anyhow::anyhow!(error))?;

    let default_client;
    let client = match http_client {
        Some(c) => c,
        None => {
            default_client = reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(3))
                .timeout(std::time::Duration::from_secs(3))
                .build()
                .unwrap_or_default();
            &default_client
        }
    };

    let chosen_origin = resolve_attach_base_origin(
        session_id,
        candidates,
        relay_origin,
        client,
    )
    .await;

    let ws_origin = if chosen_origin.starts_with("http://") {
        chosen_origin.replacen("http://", "ws://", 1)
    } else if chosen_origin.starts_with("https://") {
        chosen_origin.replacen("https://", "wss://", 1)
    } else {
        chosen_origin.to_string()
    };

    let trimmed = ws_origin.trim_end_matches('/');
    let url = if trimmed.contains("/tunnel/opaque") {
        trimmed.to_string()
    } else if candidates
        .iter()
        .any(|c| c.base_origin == chosen_origin && c.path != AttachPath::Relay)
    {
        format!("{trimmed}/api/v1/attach")
    } else {
        format!("{trimmed}/tunnel/opaque/{session_id}")
    };

    let (stream, _) = tokio_tungstenite::connect_async(url).await?;
    let attached = attach_opaque_session(
        stream,
        &identity,
        machine_attach_public_key,
        machine_id,
        session_id,
        enrollment_epoch,
    )
    .await
    .map_err(|error| anyhow::anyhow!(error.to_string()))?;

    confirm_attach_origin(session_id, &chosen_origin);

    Ok(attached)
}

pub async fn attach_relay_session(
    relay_origin: &str,
    session_id: &str,
    machine_id: &str,
    machine_attach_public_key: &str,
    enrollment_epoch: &str,
    identity_dir: &Path,
) -> anyhow::Result<RelayAttachedStream> {
    attach_paired_session_with_candidates(
        relay_origin,
        session_id,
        machine_id,
        machine_attach_public_key,
        enrollment_epoch,
        identity_dir,
        &[],
        None,
    )
    .await
}
