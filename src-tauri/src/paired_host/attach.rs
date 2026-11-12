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

pub struct AttachCandidates<'a> {
    pub paths: &'a [CandidatePath],
    pub http_client: Option<&'a reqwest::Client>,
}

/// Typed failure for an attach race in which every candidate was rejected because it named a
/// machine other than the one being attached to. Never a panic, and never a silent fallback to
/// a plain relay attach: the caller must not read this as an ordinary transport hiccup.
pub const ATTACH_IDENTITY_MISMATCH: &str = "ATTACH_IDENTITY_MISMATCH";

/// Whether a candidate can be the machine this attach targets.
///
/// Identity is checked here, inside the attempt, so a rejected candidate is torn down while
/// every other candidate - the relay included - is still live and still able to win. A
/// candidate that carries no identity claim keeps the historical behaviour: the Noise static
/// key pin and the prologue-bound machine id remain the only checks.
fn candidate_names_expected_machine(candidate: &CandidatePath, machine_id: &str) -> bool {
    match candidate.expected_machine_id.as_deref() {
        None => true,
        Some(expected) => expected == machine_id,
    }
}

#[cfg(test)]
pub(super) fn candidate_names_expected_machine_for_test(
    candidate: &CandidatePath,
    machine_id: &str,
) -> bool {
    candidate_names_expected_machine(candidate, machine_id)
}

/// The candidates the degraded (non-racing) attempt may still dial. Filtering here is what makes
/// the identity gate hold on both paths: without it the fallback would dial `candidates.first()`
/// and quietly re-admit a candidate the gate had just refused.
fn eligible_candidates(candidates: &[CandidatePath], machine_id: &str) -> Vec<CandidatePath> {
    candidates
        .iter()
        .filter(|candidate| candidate_names_expected_machine(candidate, machine_id))
        .cloned()
        .collect()
}

pub async fn attach_paired_session_with_candidates(
    relay_origin: &str,
    session_id: &str,
    machine_id: &str,
    machine_attach_public_key: &str,
    enrollment_epoch: &str,
    identity_dir: &Path,
    options: AttachCandidates<'_>,
) -> anyhow::Result<RelayAttachedStream> {
    let AttachCandidates { paths: candidates, http_client: _http_client } = options;
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
        let rejected = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let rejections = Arc::clone(&rejected);

        let selected = select_and_reuse_channel(
            candidates,
            PATH_PROBE_DEADLINE,
            move |candidate| {
                let id_clone = Arc::clone(&identity_arc);
                let pk = pub_key.clone();
                let m = mid.clone();
                let s = sid.clone();
                let e = epoch.clone();
                let rejections = Arc::clone(&rejections);
                async move {
                    if !candidate_names_expected_machine(&candidate, &m) {
                        rejections.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        return Err(());
                    }
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
        if rejected.load(std::sync::atomic::Ordering::SeqCst) == candidates.len() {
            return Err(anyhow::anyhow!(ATTACH_IDENTITY_MISMATCH));
        }
    }

    // The degraded attempt must not resurrect a candidate the identity gate just refused - that
    // would make D3 enforceable only on the fast path. `&[]` still resolves to the relay, so a
    // host that publishes nothing behaves exactly as it did before candidates existed.
    let eligible = eligible_candidates(candidates, machine_id);
    let chosen_origin = resolve_attach_base_origin(session_id, &eligible, relay_origin);
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
        AttachCandidates { paths: &[], http_client: None },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claiming(path: AttachPath, origin: &str) -> CandidatePath {
        CandidatePath::with_expected_machine_id(path, origin, None, Some("machine-a".into()))
    }

    #[test]
    fn the_degraded_path_drops_a_candidate_naming_another_machine() {
        let candidates = vec![
            claiming(AttachPath::Direct, "https://wrong.test"),
            CandidatePath::new(AttachPath::Relay, "https://relay.test", None),
        ];
        let eligible = eligible_candidates(&candidates, "machine-b");
        assert_eq!(eligible.len(), 1, "only the relay may survive the identity gate");
        assert_eq!(eligible[0].base_origin, "https://relay.test");
        assert_eq!(
            eligible_candidates(&candidates, "machine-a").len(),
            2,
            "the machine that was actually published stays reachable"
        );
    }

    #[test]
    fn an_empty_candidate_list_stays_empty_so_the_relay_is_unconditional() {
        assert!(eligible_candidates(&[], "machine-a").is_empty());
    }

    fn naming_another_machine(path: AttachPath, origin: &str) -> CandidatePath {
        CandidatePath::with_expected_machine_id(
            path,
            origin,
            None,
            Some("other-machine".to_string()),
        )
    }

    /// Every candidate naming a different machine must fail as one typed error - never a panic,
    /// and never a silent relay fallback. The caller has to be able to tell "this is not the host
    /// I expected" from "the network was unlucky", because only the first is a security event.
    #[tokio::test]
    async fn every_candidate_naming_another_machine_is_one_typed_error() {
        let identity = tempfile::tempdir().expect("temp identity dir");
        let candidates = [
            naming_another_machine(AttachPath::Lan, "http://127.0.0.1:9"),
            naming_another_machine(AttachPath::Relay, "http://127.0.0.1:9"),
        ];
        let result = attach_paired_session_with_candidates(
            "http://127.0.0.1:9",
            "session-every-candidate-mismatched",
            "machine-a",
            "unused-attach-public-key",
            "epoch-1",
            identity.path(),
            AttachCandidates {
                paths: &candidates,
                http_client: None,
            },
        )
        .await;
        // Matched rather than `expect_err`, which would require the attached stream to be Debug
        // for a failure path that never produces one.
        let error = match result {
            Ok(_) => panic!("no candidate names machine-a, so nothing may attach"),
            Err(error) => error,
        };
        assert_eq!(error.to_string(), ATTACH_IDENTITY_MISMATCH);
    }
}
