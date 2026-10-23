//! MachineClient hooks for the direct path: pick the request base, report direct
//! transport loss, and start the background negotiator once trust is known.

use url::Url;

use super::client::ClientError;
use super::direct_connect::{ensure_direct, is_running, DirectAttempt};
use super::direct_route::GLOBAL_DIRECT_ROUTES;
use super::direct_trust::{trusted_key, DirectTrustStore};
use super::inventory::{CredentialLease, HostView};

/// Base URL for one request/stream plus the direct epoch it used, if any.
/// Callers must reuse one base for a ticket and the socket it authorizes.
pub(crate) struct RequestBase {
    pub url: Url,
    pub direct_epoch: Option<u64>,
}

pub(crate) fn request_base(host: &HostView) -> Result<RequestBase, ClientError> {
    let invalid = || ClientError::local("INVALID_REQUEST");
    if let Some((origin, epoch)) = GLOBAL_DIRECT_ROUTES.direct_for(&host.host_id) {
        if let Ok(url) = Url::parse(&origin) {
            return Ok(RequestBase { url, direct_epoch: Some(epoch) });
        }
    }
    Ok(RequestBase {
        url: Url::parse(&crate::remote::relay_client::live_relay_url(&host.host_id))
            .map_err(|_| invalid())?,
        direct_epoch: None,
    })
}

/// A request over the direct adapter failed at the transport level: fall back.
pub(crate) fn report_direct_loss(host: &HostView, epoch: u64) {
    GLOBAL_DIRECT_ROUTES.mark_disconnected(&host.host_id, epoch, std::time::Instant::now());
}

/// Transport-level failures (not host answers) that justify leaving the direct path.
pub(crate) fn is_transport_failure(error: &ClientError) -> bool {
    error.machine_error.is_none() && matches!(error.code.as_str(), "HOST_UNAVAILABLE" | "TIMEOUT")
}

/// Starts direct negotiation in the background after the relay confirmed the
/// host's machine identity. Without a locally provisioned host key nothing
/// starts and every request stays on the relay. Never blocks the caller.
pub(crate) fn kick(http: &reqwest::Client, host: &HostView, lease: &CredentialLease) {
    let generation = lease.generation().0;
    if is_running(&host.host_id, generation) {
        return;
    }
    // Subscribe before any await so a revocation during trust loading is seen.
    let mut cancelled = lease.cancellation();
    let Ok(bearer) = lease.token().map(str::to_owned) else {
        return;
    };
    let http = http.clone();
    let host = host.clone();
    tokio::spawn(async move {
        let Some(attempt) = load_attempt(&host, bearer).await else {
            return;
        };
        if *cancelled.borrow_and_update() {
            return;
        }
        let cancelled = async move {
            // borrow_and_update first: a flag already set before subscribe counts.
            while !*cancelled.borrow_and_update() {
                if cancelled.changed().await.is_err() {
                    return;
                }
            }
        };
        ensure_direct(&GLOBAL_DIRECT_ROUTES, http, attempt, generation, cancelled);
    });
}

async fn load_attempt(host: &HostView, bearer: String) -> Option<DirectAttempt> {
    let dir = crate::remote::auth::canonical_identity_dir().ok()?;
    let http = super::direct_autotrust::account_http();
    let account_key = if super::direct_trust::auto_trust_enabled() {
        crate::remote::direct_lease::ensure_account_public_key(Some(&dir), &http)
            .await
            .ok()
    } else {
        None
    };
    load_attempt_in(dir, host, bearer, account_key.as_deref().map(|key| (&http, key))).await
}

/// [`load_attempt`] against an explicit identity dir. `auto_trust` carries the account HTTP
/// client and the operator-pinned account public key; `None` disables account auto-pinning.
///
/// On a trust miss, a client enrolled with an account fetches the host's account-signed peer
/// record from the account origin it enrolled with, verifies it, pins it and continues. Every
/// failure (not enrolled, no pin, network, bad record) is logged and leaves the route on the relay.
pub(crate) async fn load_attempt_in(
    dir: std::path::PathBuf,
    host: &HostView,
    bearer: String,
    auto_trust: Option<(&reqwest::Client, &str)>,
) -> Option<DirectAttempt> {
    let pinned = trusted_key(DirectTrustStore::at(&dir), host.machine_id.clone()).await;
    if pinned.is_none() && auto_trust.is_none() {
        return None;
    }
    let identity_dir = dir.clone();
    let identity = crate::ipc::run_blocking(move || {
        crate::remote::auth::load_or_generate_machine_identity(&identity_dir)
            .map_err(crate::ipc::IpcError::internal)
    })
    .await
    .ok()?;
    let host_key = match pinned {
        Some(key) => key,
        None => {
            let (http, account_public_key) = auto_trust?;
            let enrollment_dir = dir.clone();
            let enrollment = crate::ipc::run_blocking(move || {
                Ok(crate::remote::direct_lease::load_host_enrollment(Some(&enrollment_dir)))
            })
            .await
            .ok()
            .flatten()?;
            let attestor = super::direct_autotrust::Attestor {
                http,
                account_origin: &enrollment.account_origin,
                own: &identity,
                account_public_key,
            };
            match super::direct_autotrust::attest_and_pin(
                DirectTrustStore::at(&dir),
                &attestor,
                &host.machine_id,
                None,
                crate::remote::direct_lease::unix_now_secs(),
            )
            .await
            {
                Ok(key) => {
                    tracing::info!(host = %host.machine_id, "direct trust pinned from account record");
                    key
                }
                Err(error) => {
                    tracing::info!(host = %host.machine_id, %error, "no account direct trust; staying on relay");
                    return None;
                }
            }
        }
    };
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let seed: [u8; 32] = STANDARD.decode(&identity.private_key).ok()?.try_into().ok()?;
    Some(DirectAttempt {
        host_id: host.host_id.clone(),
        base_url: host.host_id.clone(),
        bearer,
        own_machine_id: identity.machine_id,
        host_machine_id: host.machine_id.clone(),
        signing_key: ed25519_dalek::SigningKey::from_bytes(&seed),
        host_key,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::peer_record::{sign_peer_record, test_support::*};
    use crate::account::enroll_client::{AccountEnrollmentRecord, ACCOUNT_ENROLLMENT_FILE};
    use crate::paired_host::inventory::{AuthStatus, GrantScope};
    use crate::scoped_contracts::Epoch;
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use std::time::Duration;

    const LIMIT: Duration = Duration::from_secs(15);

    fn host_view(machine_id: &str) -> HostView {
        HostView {
            host_id: "host-view-1".into(),
            relay_origin: "https://relay.test".into(),
            machine_id: machine_id.into(),
            display_label: "host".into(),
            grant_scope: GrantScope::Machine,
            generation: Epoch(1),
            auth_status: AuthStatus::Paired,
            online: true,
            ssh_host_id: None,
            device_id: None,
            unavailable_reason: None,
        }
    }

    /// A client data dir with an identity and an account enrollment pointing at `origin`.
    fn client_dir(origin: &str) -> (tempfile::TempDir, crate::remote::auth::MachineIdentity) {
        let dir = tempfile::tempdir().unwrap();
        let identity = crate::remote::auth::load_or_generate_machine_identity(dir.path()).unwrap();
        let record = AccountEnrollmentRecord {
            account_id: "acct-1".into(),
            machine_record_id: format!("rec-{}", identity.machine_id),
            account_origin: origin.into(),
            relay_origin: "https://relay.test".into(),
            enrollment_epoch: "1".into(),
            enrolled_at: 1,
        };
        std::fs::write(
            dir.path().join(ACCOUNT_ENROLLMENT_FILE),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        (dir, identity)
    }

    async fn attempt(
        dir: &std::path::Path,
        host: &HostView,
        account_key: Option<&str>,
    ) -> Option<DirectAttempt> {
        let http = http();
        tokio::time::timeout(
            LIMIT,
            load_attempt_in(dir.to_path_buf(), host, "bearer".into(), account_key.map(|k| (&http, k))),
        )
        .await
        .expect("bounded attempt")
    }

    #[tokio::test]
    async fn client_valid_account_record_pins_host_and_attempt_proceeds() {
        let host = identity("host-m", 31);
        // Enrollment origin is only known after the account starts; the identity must exist first.
        let probe = tempfile::tempdir().unwrap();
        let client = crate::remote::auth::load_or_generate_machine_identity(probe.path()).unwrap();
        let fx = spawn_account(&[
            ("user-1", &client.machine_id, &client.public_key),
            ("user-1", "host-m", &host.public_key),
        ])
        .await;
        let (dir, _) = client_dir(&fx.origin);
        std::fs::copy(probe.path().join("identity.json"), dir.path().join("identity.json")).unwrap();

        let store = DirectTrustStore::at(dir.path());
        assert_eq!(store.trusted_key("host-m").unwrap(), None);
        let attempt = attempt(dir.path(), &host_view("host-m"), Some(&fx.account_public_key))
            .await
            .expect("an account-attested host key lets the direct attempt proceed");
        let expected = super::super::direct_trust::parse_verifying_key(&host.public_key).unwrap();
        assert_eq!(attempt.host_key, expected);
        assert_eq!(attempt.own_machine_id, client.machine_id);
        assert_eq!(store.trusted_key("host-m").unwrap(), Some(expected), "the key is pinned");
    }

    #[tokio::test]
    async fn client_forged_or_unsigned_record_leaves_no_pin() {
        let host = identity("host-m", 31);
        let account = ed25519_dalek::SigningKey::from_bytes(&[41; 32]);
        let account_public = STANDARD.encode(account.verifying_key().to_bytes());
        let forger = ed25519_dalek::SigningKey::from_bytes(&[42; 32]);
        let now = crate::remote::direct_lease::unix_now_secs();
        let forged =
            sign_peer_record(&forger, "host-m", &host.public_key, "user-1", 1, now + 600).unwrap();
        let wrong_machine =
            sign_peer_record(&account, "host-other", &host.public_key, "user-1", 1, now + 600).unwrap();
        for record in [forged, "unsigned-garbage".to_string(), wrong_machine] {
            let (origin, stub) = spawn_record_stub(record).await;
            let (dir, _) = client_dir(&origin);
            assert!(attempt(dir.path(), &host_view("host-m"), Some(&account_public)).await.is_none());
            assert_eq!(DirectTrustStore::at(dir.path()).trusted_key("host-m").unwrap(), None);
            stub.abort();
        }
    }

    #[tokio::test]
    async fn client_without_enrollment_or_opt_out_stays_on_relay() {
        let host = identity("host-m", 31);
        let account = ed25519_dalek::SigningKey::from_bytes(&[41; 32]);
        let account_public = STANDARD.encode(account.verifying_key().to_bytes());
        let now = crate::remote::direct_lease::unix_now_secs();
        let record = sign_peer_record(&account, "host-m", &host.public_key, "u", 1, now + 600).unwrap();
        let (origin, stub) = spawn_record_stub(record).await;
        // Opt-out (no account key passed): nothing is fetched or pinned.
        let (dir, _) = client_dir(&origin);
        assert!(attempt(dir.path(), &host_view("host-m"), None).await.is_none());
        assert_eq!(DirectTrustStore::at(dir.path()).trusted_key("host-m").unwrap(), None);
        // Not account-enrolled: skipped silently.
        let bare = tempfile::tempdir().unwrap();
        assert!(attempt(bare.path(), &host_view("host-m"), Some(&account_public)).await.is_none());
        assert_eq!(DirectTrustStore::at(bare.path()).trusted_key("host-m").unwrap(), None);
        stub.abort();
    }
}

