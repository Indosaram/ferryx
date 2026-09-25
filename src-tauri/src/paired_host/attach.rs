use std::path::Path;
use std::sync::Arc;

use crate::paired_host::path_select::{
    confirm_attach_origin, resolve_attach_base_origin, select_and_reuse_channel, AttachPath,
    CandidatePath, GLOBAL_SESSION_ATTACH_ORIGINS, PATH_PROBE_DEADLINE,
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

fn attach_ws_url(base_origin: &str, session_id: &str, is_relay: bool) -> String {
    let ws_origin = if base_origin.starts_with("http://") {
        base_origin.replacen("http://", "ws://", 1)
    } else if base_origin.starts_with("https://") {
        base_origin.replacen("https://", "wss://", 1)
    } else {
        base_origin.to_string()
    };

    let trimmed = ws_origin.trim_end_matches('/');
    if trimmed.contains("/tunnel/opaque") {
        trimmed.to_string()
    } else if !is_relay {
        format!("{trimmed}/api/v1/attach")
    } else {
        format!("{trimmed}/tunnel/opaque/{session_id}")
    }
}

pub async fn attach_paired_session_with_candidates(
    relay_origin: &str,
    session_id: &str,
    machine_id: &str,
    machine_attach_public_key: &str,
    enrollment_epoch: &str,
    identity_dir: &Path,
    candidates: &[CandidatePath],
    _http_client: Option<&reqwest::Client>,
) -> anyhow::Result<RelayAttachedStream> {
    let identity = load_or_generate_client_attach_identity(identity_dir)
        .map_err(|error| anyhow::anyhow!(error))?;

    if let Some(existing) = GLOBAL_SESSION_ATTACH_ORIGINS.get(session_id) {
        let is_relay = !candidates
            .iter()
            .any(|c| c.base_origin == existing && c.path != AttachPath::Relay);
        let url = attach_ws_url(&existing, session_id, is_relay);
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

        return Ok(attached);
    }

    if !candidates.is_empty() {
        let identity_arc = Arc::new(identity.clone());
        let pub_key = machine_attach_public_key.to_string();
        let mid = machine_id.to_string();
        let sid = session_id.to_string();
        let epoch = enrollment_epoch.to_string();

        let selected = select_and_reuse_channel(
            candidates,
            PATH_PROBE_DEADLINE,
            move |candidate| {
                let id_clone = Arc::clone(&identity_arc);
                let pk = pub_key.clone();
                let m = mid.clone();
                let s = sid.clone();
                let e = epoch.clone();
                async move {
                    let is_relay = candidate.path == AttachPath::Relay;
                    let url = attach_ws_url(&candidate.base_origin, &s, is_relay);
                    let start = std::time::Instant::now();
                    let (stream, _) = tokio_tungstenite::connect_async(url)
                        .await
                        .map_err(|_| ())?;
                    let attached = attach_opaque_session(
                        stream,
                        &id_clone,
                        &pk,
                        &m,
                        &s,
                        &e,
                    )
                    .await
                    .map_err(|_| ())?;
                    Ok((attached, start.elapsed()))
                }
            },
        )
        .await;

        if let Some(winner) = selected {
            confirm_attach_origin(session_id, &winner.base_origin);
            return Ok(winner.channel);
        }
    }

    let chosen_origin = resolve_attach_base_origin(session_id, candidates, relay_origin);
    let is_relay = !candidates
        .iter()
        .any(|c| c.base_origin == chosen_origin && c.path != AttachPath::Relay);
    let url = attach_ws_url(&chosen_origin, session_id, is_relay);

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

pub async fn attach_host_session(
    host: &crate::paired_host::inventory::HostView,
    device_token: Option<&str>,
    session_id: &str,
    machine_attach_public_key: &str,
    enrollment_epoch: &str,
    identity_dir: &Path,
) -> anyhow::Result<RelayAttachedStream> {
    let candidates = build_host_candidates(host, device_token);
    attach_paired_session_with_candidates(
        &host.relay_origin,
        session_id,
        &host.machine_id,
        machine_attach_public_key,
        enrollment_epoch,
        identity_dir,
        &candidates,
        None,
    )
    .await
}
