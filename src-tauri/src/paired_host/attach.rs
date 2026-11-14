use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crate::paired_host::path_select::{
    confirm_attach_origin, resolve_attach_base_origin, select_and_reuse_channel, AttachPath,
    CandidatePath, GLOBAL_SESSION_ATTACH_ORIGINS, PATH_PROBE_DEADLINE,
};
use crate::remote::attach_client::{attach_opaque_session, load_or_generate_client_attach_identity};
use crate::remote::attach_crypto::{SecureStream, WebSocketByteStream};
use crate::remote::attach_identity::AttachIdentity;

pub type RelayAttachedStream =
    SecureStream<WebSocketByteStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>>;

/// The machine's direct attach endpoint (`remote::attach_router`) binds its Noise prologue to this
/// fixed session id; the relay binds the prologue to the allocated opaque session id instead. A
/// direct candidate dialled with the relay id never completes the handshake, so the path silently
/// lost every race to the relay.
pub const DIRECT_ATTACH_SESSION_ID: &str = "direct";

fn prologue_session_id<'a>(candidate: &CandidatePath, session_id: &'a str) -> &'a str {
    if candidate.path == AttachPath::Relay {
        session_id
    } else {
        DIRECT_ATTACH_SESSION_ID
    }
}

/// How long one pinned origin may take.
///
/// The pin must obey the same rule as the candidate pool: a route with a declared budget uses it,
/// and a bare direct route uses the probe deadline. Sharing the rule is what keeps a stalled pin
/// from holding an attach that the pool would have cut off and retried over the relay.
pub(crate) fn pin_budget(candidates: &[CandidatePath], origin: &str) -> Duration {
    candidates
        .iter()
        .find(|candidate| candidate.base_origin == origin)
        .and_then(|candidate| candidate.probe_budget)
        .unwrap_or(PATH_PROBE_DEADLINE)
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

pub struct AttachCandidates<'a> {
    pub paths: &'a [CandidatePath],
    pub http_client: Option<&'a reqwest::Client>,
}

/// Dial one pinned origin and complete the Noise handshake on it.
///
/// Split out of the attach entry point so the pin attempt can be wrapped in the same budget the
/// candidate pool would give that route; a direct route that accepts the socket and then stalls in
/// the handshake must not hang an attach that the relay could still serve.
async fn pin_attach(
    url: String,
    identity: &AttachIdentity,
    machine_attach_public_key: &str,
    machine_id: &str,
    prologue_session: &str,
    enrollment_epoch: &str,
) -> anyhow::Result<RelayAttachedStream> {
    let (stream, _) = tokio_tungstenite::connect_async(url)
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    attach_opaque_session(
        stream,
        identity,
        machine_attach_public_key,
        machine_id,
        prologue_session,
        enrollment_epoch,
    )
    .await
    .map_err(|error| anyhow::anyhow!(error.to_string()))
}

/// Candidates that may be dialled for `machine_id`.
///
/// A candidate that names a *different* machine is dropped rather than dialled: a stale paired
/// record must never attach this client to another host. Dropping keeps the relay fallback live,
/// which is what an unavailable direct route already does.
pub(crate) fn eligible_candidates(candidates: &[CandidatePath], machine_id: &str) -> Vec<CandidatePath> {
    candidates
        .iter()
        .filter(|candidate| {
            candidate
                .expected_machine_id
                .as_deref()
                .is_none_or(|expected| expected == machine_id)
        })
        .cloned()
        .collect()
}

/// Drop a pin that the current inventory no longer offers.
///
/// `confirm_attach_origin` records only when the session has no origin yet, so leaving a stale
/// record in place would swallow the origin that actually won this attempt and every later attach
/// would keep dialling a route the inventory has already stopped offering.
fn discard_ineligible_pin(session_id: &str) {
    GLOBAL_SESSION_ATTACH_ORIGINS.remove(session_id);
}

/// A pinned origin is reused only while it is still one of this machine's eligible routes.
fn pinned_is_eligible(candidates: &[CandidatePath], origin: &str) -> bool {
    candidates.iter().any(|c| c.base_origin == origin)
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
    let mut candidates = eligible_candidates(candidates, machine_id);
    // A direct route is a probe; the relay is a cold handshake. Give the relay candidate the full
    // establishment budget so a stale direct route cannot stop an attach that the relay would serve.
    for candidate in candidates.iter_mut() {
        if candidate.path == AttachPath::Relay {
            candidate.probe_budget = Some(crate::paired_host::secure_http::ESTABLISH_TIMEOUT);
        }
    }
    let candidates = candidates.as_slice();
    let identity = load_or_generate_client_attach_identity(identity_dir)
        .map_err(|error| anyhow::anyhow!(error))?;

    if let Some(existing) = GLOBAL_SESSION_ATTACH_ORIGINS.get(session_id) {
        let is_relay = !candidates
            .iter()
            .any(|c| c.base_origin == existing && c.path != AttachPath::Relay);
        // A pinned origin from an EARLIER allocation is reused only when it is one of this
        // machine's eligible routes. Otherwise a reused session id would be dialled at a route the
        // current inventory no longer offers, instead of the routes (or relay) that do work now.
        if !pinned_is_eligible(candidates, &existing) {
            tracing::debug!(
                session_id,
                origin = %existing,
                "Discarding a pinned attach origin that is no longer an eligible route"
            );
            discard_ineligible_pin(session_id);
        } else {
            let url = attach_ws_url(&existing, session_id, is_relay);
            let prologue_session = if is_relay { session_id } else { DIRECT_ATTACH_SESSION_ID };
            // A pinned origin is a CANDIDATE, not a verdict: eligibility is a static inventory
            // fact, while reachability is decided at dial time. Returning the failure here made a
            // stale-but-still-listed direct route fatal - the relay candidate that would have
            // answered was never tried. So a failed pin falls through to the candidate pool below;
            // only a successful pin short-circuits. The pin is also bounded by the SAME budget the
            // pool would give that route, otherwise a direct route that accepts the socket and then
            // never completes the handshake would hang the attach here, where the pool's probe
            // deadline used to cut it off and let the relay win.
            let pin_deadline = pin_budget(candidates, &existing);
            let pinned = tokio::time::timeout(
                pin_deadline,
                pin_attach(
                    url,
                    &identity,
                    machine_attach_public_key,
                    machine_id,
                    prologue_session,
                    enrollment_epoch,
                ),
            )
            .await;
            match pinned {
                Ok(Ok(attached)) => return Ok(attached),
                Ok(Err(error)) => tracing::debug!(
                    session_id,
                    origin = %existing,
                    %error,
                    "Pinned attach origin answered but failed to attach; trying the candidate pool"
                ),
                Err(_) => tracing::debug!(
                    session_id,
                    origin = %existing,
                    "Pinned attach origin did not answer within its budget; trying the candidate pool"
                ),
            }
            // The pin is stale, so the route allocation it named must not be reused for the
            // candidate pool: a later attach would pin the same dead origin again.
            GLOBAL_SESSION_ATTACH_ORIGINS.remove(session_id);
        }
    }

    if !candidates.is_empty() {
        let identity_arc = Arc::new(identity.clone());
        let pub_key = machine_attach_public_key.to_string();
        let mid = machine_id.to_string();
        let sid = session_id.to_string();
        let epoch = enrollment_epoch.to_string();
        let rejected = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let rejections = Arc::clone(&rejected);

        // The direct attempt is a PROBE: it either answers quickly or it does not, and a slow
        // direct dial says nothing about the machine. The relay can only be reached through a cold
        // handshake that legitimately takes longer than that probe, so it keeps the full
        // establishment budget. Racing every candidate under one short probe deadline made a host
        // with a stale direct route fail to attach where the relay-only path used to succeed.
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
                    let prologue_session = prologue_session_id(&candidate, &s).to_string();
                    let start = std::time::Instant::now();
                    let (stream, _) = tokio_tungstenite::connect_async(url)
                        .await
                        .map_err(|_| ())?;
                    let attached = attach_opaque_session(
                        stream,
                        &id_clone,
                        &pk,
                        &m,
                        &prologue_session,
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

        // Every candidate was tried and none attached. Falling through would dial a route this
        // attempt already consumed: a relay half the relay server now refuses (it rejects an
        // active or reserved half), or the same direct endpoint that just failed. So the failure is
        // reported as-is. In particular the relay must NOT be re-dialled when the pool contained
        // it, and a direct-only pool must not re-dial its dead direct route either - the
        // relay-only path below is reached only when this whole block did not run.
        return Err(anyhow::anyhow!(
            "no attach candidate reached machine {machine_id}"
        ));
    }

    let chosen_origin =
        resolve_attach_base_origin(session_id, candidates, machine_id, relay_origin);
    let is_relay = !candidates
        .iter()
        .any(|c| c.base_origin == chosen_origin && c.path != AttachPath::Relay);
    let url = attach_ws_url(&chosen_origin, session_id, is_relay);
    let prologue_session = if is_relay { session_id } else { DIRECT_ATTACH_SESSION_ID };

    let (stream, _) = tokio_tungstenite::connect_async(url).await?;
    let attached = attach_opaque_session(
        stream,
        &identity,
        machine_attach_public_key,
        machine_id,
        prologue_session,
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
    attach_relay_session_with_candidates(
        relay_origin,
        session_id,
        machine_id,
        machine_attach_public_key,
        enrollment_epoch,
        identity_dir,
        &[],
    )
    .await
}

/// Relay attach that may also race a verified direct/LAN route to the same machine.
///
/// `paths` is the candidate set for this host; an empty set is exactly the relay-only attach
/// that `attach_relay_session` performs, so a host with no live direct route is unaffected.
pub async fn attach_relay_session_with_candidates(
    relay_origin: &str,
    session_id: &str,
    machine_id: &str,
    machine_attach_public_key: &str,
    enrollment_epoch: &str,
    identity_dir: &Path,
    paths: &[CandidatePath],
) -> anyhow::Result<RelayAttachedStream> {
    attach_paired_session_with_candidates(
        relay_origin,
        session_id,
        machine_id,
        machine_attach_public_key,
        enrollment_epoch,
        identity_dir,
        AttachCandidates { paths, http_client: None },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paired_host::path_select::release_attach_origin;

    fn candidate(path: AttachPath, origin: &str) -> CandidatePath {
        CandidatePath::new(path, origin, Some("token".into()))
    }

    /// The machine's direct endpoint binds its Noise prologue to `direct`; the relay binds it to the
    /// allocated opaque session. Using the relay id on a direct socket fails the handshake, which is
    /// how every direct attach used to lose to the relay.
    #[test]
    fn a_direct_candidate_uses_the_direct_prologue_session_id() {
        assert_eq!(
            prologue_session_id(&candidate(AttachPath::Direct, "http://127.0.0.1:43821"), "opaque-1"),
            DIRECT_ATTACH_SESSION_ID
        );
        assert_eq!(
            prologue_session_id(&candidate(AttachPath::Relay, "https://relay.example.com"), "opaque-1"),
            "opaque-1"
        );
    }

    /// A pinned origin must not outlive the budget the candidate pool would give the same route.
    /// Otherwise a direct route that accepts the socket and then stalls in the handshake holds the
    /// whole attach, where the pool would have cut it off and let the relay answer.
    #[test]
    fn a_pinned_origin_uses_the_budget_its_route_declares() {
        let relay = "https://relay.example.com";
        let mut candidates = vec![
            candidate(AttachPath::Direct, "http://127.0.0.1:43821"),
            candidate(AttachPath::Relay, relay),
        ];
        candidates[1].probe_budget = Some(crate::paired_host::secure_http::ESTABLISH_TIMEOUT);

        // A route with a declared budget uses it, so the relay keeps its establishment budget.
        assert_eq!(
            pin_budget(&candidates, relay),
            crate::paired_host::secure_http::ESTABLISH_TIMEOUT
        );
        // A bare direct route falls back to the shared probe deadline rather than waiting forever.
        assert_eq!(
            pin_budget(&candidates, "http://127.0.0.1:43821"),
            PATH_PROBE_DEADLINE
        );
        // An origin that is no longer among the candidates still gets a bounded attempt.
        assert_eq!(pin_budget(&candidates, "http://127.0.0.1:1"), PATH_PROBE_DEADLINE);
    }

    #[test]
    fn a_direct_candidate_dials_the_machine_attach_endpoint() {
        assert_eq!(
            attach_ws_url("http://127.0.0.1:43821", "opaque-1", false),
            "ws://127.0.0.1:43821/api/v1/attach"
        );
        assert_eq!(
            attach_ws_url("https://relay.example.com", "opaque-1", true),
            "wss://relay.example.com/tunnel/opaque/opaque-1"
        );
    }

    /// An exhausted candidate pool must report the failure instead of re-dialling a route this
    /// attempt already consumed. `resolve_attach_base_origin` falls back to the first eligible
    /// route for the machine when the pool has no relay candidate, so letting a non-empty pool
    /// fall through would re-dial the very direct endpoint that just failed (and re-dial a relay
    /// half the relay server now refuses). The relay-only path is reachable only from an EMPTY
    /// pool, where that resolver returns the caller's relay origin.
    #[test]
    fn an_exhausted_pool_does_not_resolve_to_a_route_it_already_tried() {
        let relay = "https://relay.example.com";
        let direct = "http://127.0.0.1:43821";
        let session = format!("pool-exhausted-{}", uuid::Uuid::new_v4());
        release_attach_origin(&session);
        // A direct-only pool: the resolver hands back the same direct route, which is exactly why
        // the caller must not fall through to it.
        assert_eq!(
            resolve_attach_base_origin(&session, &[candidate(AttachPath::Direct, direct)], "m1", relay),
            direct
        );
        // Only an empty pool resolves to the relay origin, so falling through there dials the one
        // route that was never attempted.
        assert_eq!(resolve_attach_base_origin(&session, &[], "m1", relay), relay);
        release_attach_origin(&session);
    }

    /// A pin that is no longer one of the machine's eligible routes must be dropped, not merely
    /// logged. `confirm_attach_origin` records only when absent, so a surviving stale pin would
    /// swallow the origin that actually won the race and every later attach would keep dialling the
    /// route the inventory no longer offers.
    #[test]
    fn an_ineligible_pin_is_removed_instead_of_being_kept_for_the_next_attach() {
        let stale = "http://127.0.0.1:43821";
        let fresh = "https://relay.example.com";
        let session = format!("ineligible-pin-{}", uuid::Uuid::new_v4());
        release_attach_origin(&session);

        // An earlier allocation pinned the session to a route the current inventory does not list.
        assert_eq!(confirm_attach_origin(&session, stale), stale);
        // The route the inventory offers now, which the stale pin does not name.
        let candidates = [candidate(AttachPath::Relay, fresh)];
        assert!(!pinned_is_eligible(&candidates, stale));

        discard_ineligible_pin(&session);

        // The record is gone, so the winning route is the one that gets recorded instead of being
        // swallowed by the stale pin.
        assert_eq!(GLOBAL_SESSION_ATTACH_ORIGINS.get(&session), None);
        assert_eq!(confirm_attach_origin(&session, fresh), fresh);
        release_attach_origin(&session);
    }
}
