//! The seam between account grant issuance and daemon admission.
//!
//! Two halves of this feature already have coverage that never touches: the account service
//! tests prove an issued grant reaches the relay sealed and signed, and the machine socket
//! tests prove a Machine-scope device token is admitted. Neither proves the claim this feature
//! actually makes — that a grant the account service issued for scope `machine` produces a
//! credential this daemon admits to a machine-only terminal, while a `mirror` grant produces
//! one it refuses.
//!
//! These tests close that seam without a relay and without SSH: the real `issue_grant` handler
//! mints the sealed offer, which is opened and applied with the real `apply_grant_offer`, and
//! the resulting pairing token is exchanged for a device token that is then presented to the
//! real terminal route. The grant scope is the only variable.

use crate::remote::account_protocol::{
    AccountGrantOfferEnvelope, AccountGrantRequest, AccountGrantScope,
};
use crate::remote::server::machine_input_fixture::Fixture;
use crate::remote::{DeviceAccessScope, DevicePermission};
use axum::body::Bytes;
use axum::http::HeaderMap;
use axum::routing::post;
use axum::{Json, Router};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use futures_util::FutureExt;
use std::time::Duration;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Error};

const DEADLINE: Duration = Duration::from_secs(15);

async fn stub_relay() -> String {
    let router = Router::new().route(
        "/api/v1/attach/grant",
        post(|| async {
            (
                axum::http::StatusCode::OK,
                Json(serde_json::json!({
                    "accepted": true,
                    "delivered": { "grantId": "g1", "status": "ready" }
                })),
            )
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind stub relay");
    let origin = format!("http://{}", listener.local_addr().expect("relay addr"));
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    origin
}

/// An account store holding exactly one user, one session and one enrolled machine, built with
/// the same records and indexes the real enrollment path writes.
fn seed_account(
    state: &crate::account::service::AccountState,
    attach_public_key: &str,
    relay_origin: &str,
) -> (String, crate::account::store::MachineRecord) {
    use crate::account::store::{
        now_secs, random_token, token_hash, MachineRecord, SessionRecord, UserRecord,
    };

    let now = now_secs();
    let user_id = uuid::Uuid::new_v4().to_string();
    let session_token = random_token();
    let machine = MachineRecord {
        machine_record_id: uuid::Uuid::new_v4().to_string(),
        owner_user_id: user_id.clone(),
        machine_id: format!("test-machine-{user_id}"),
        display_name: "Grant Seam Machine".to_string(),
        public_key: STANDARD.encode([1u8; 32]),
        attach_public_key: attach_public_key.to_string(),
        relay_origin: relay_origin.to_string(),
        platform: "macos".to_string(),
        enrollment_epoch: 1,
        enrolled_at: now,
        last_seen_at: now,
    };
    let seeded = machine.clone();
    state
        .mutate(|store| {
            store.users.insert(
                user_id.clone(),
                UserRecord {
                    user_id: user_id.clone(),
                    email: format!("seam-{user_id}@example.com"),
                    created_at: now,
                },
            );
            store.sessions.insert(
                token_hash(&session_token),
                SessionRecord {
                    user_id: user_id.clone(),
                    expires_at: now + 3600,
                },
            );
            store
                .machines
                .insert(seeded.machine_record_id.clone(), seeded);
            Ok(())
        })
        .expect("seed account store");
    (session_token, machine)
}

/// Issue one real grant for `scope` and return the pairing token the account service minted,
/// after the sealed offer has been opened and applied by the daemon's own sink.
pub(crate) async fn issue_and_apply(auth: &crate::remote::AuthManager, scope: AccountGrantScope) -> String {
    use crate::account::service::{issue_grant, AccountState};

    let relay_origin = stub_relay().await;
    let attach_secret = x25519_dalek::StaticSecret::random_from_rng(rand::rngs::OsRng);
    let attach_public = x25519_dalek::PublicKey::from(&attach_secret);
    let attach = crate::remote::attach_identity::AttachIdentity {
        public_key: STANDARD.encode(attach_public.as_bytes()),
        private_key: STANDARD.encode(attach_secret.to_bytes()),
    };

    let tmp = tempfile::tempdir().expect("account temp dir");
    let state = std::sync::Arc::new(AccountState::new(
        tmp.path(),
        "https://account.test",
        std::sync::Arc::new(crate::account::mailer::FileMailer::with_dir(
            tmp.path().join("mail"),
        )),
    ));
    let (session_token, machine) = seed_account(&state, &attach.public_key, &relay_origin);

    let request = AccountGrantRequest {
        machine_record_id: machine.machine_record_id.clone(),
        enrollment_epoch: machine.enrollment_epoch.to_string(),
        device_label: "Seam phone".to_string(),
        installation_id: "seam-install".to_string(),
        grant_scope: scope,
        attach_public_key: attach.public_key.clone(),
        requester_machine: None,
    };
    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::AUTHORIZATION,
        format!("Bearer {session_token}")
            .parse()
            .expect("authorization header"),
    );
    let response = issue_grant(
        axum::extract::State(state.clone()),
        axum::extract::Path(machine.machine_record_id.clone()),
        headers,
        Bytes::from(serde_json::to_vec(&request).expect("grant request json")),
    )
    .await
    .expect("grant issuance succeeds");
    let sealed_offer = response
        .0
        .sealed_offer
        .clone()
        .expect("a machine-scope grant must carry a sealed offer");
    let machine_id = machine.machine_id.clone();
    let enrollment_epoch = machine.enrollment_epoch.to_string();
    assert_eq!(response.0.machine_id, machine_id);

    let envelope = AccountGrantOfferEnvelope {
        machine_id: machine_id.clone(),
        enrollment_epoch: enrollment_epoch.clone(),
        sealed: sealed_offer.sealed,
    };
    let now = crate::account::store::now_secs();
    let account_vouched = false;
    let ack = crate::account::offer_sink::apply_grant_offer(
        auth,
        &attach,
        &machine_id,
        &enrollment_epoch,
        &envelope,
        account_vouched,
        now,
    )
    .expect("the daemon applies the sealed grant");
    assert_eq!(ack.status, "ready");
    assert_eq!(ack.grant_id, response.0.grant_id);
    response.0.pairing_token
}

/// One machine-only session id and daemon epoch, created through the fixture's own control
/// token so the session is exactly what the machine API produces.
async fn machine_session(fixture: &Fixture) -> (String, String) {
    let session = fixture.create().await;
    (
        session["target"]["sessionId"]
            .as_str()
            .expect("session id")
            .to_string(),
        session["target"]["daemonEpoch"]
            .as_str()
            .expect("daemon epoch")
            .to_string(),
    )
}

async fn dial(fixture: &Fixture, token: &str, session_id: &str, epoch: &str) -> Result<(), Error> {
    let mut request = format!(
        "{}/api/v1/terminal/{session_id}?daemonEpoch={epoch}",
        fixture.base.replace("http:", "ws:")
    )
    .into_client_request()
    .expect("websocket request");
    request.headers_mut().insert(
        "authorization",
        format!("Bearer {token}").parse().expect("bearer header"),
    );
    tokio::time::timeout(DEADLINE, tokio_tungstenite::connect_async(request))
        .await
        .expect("bounded terminal handshake")
        .map(|_| ())
}

#[tokio::test]
async fn machine_grant_yields_a_token_the_daemon_admits_and_mirror_grant_does_not() {
    let fixture = Fixture::new().await;
    let outcome = std::panic::AssertUnwindSafe(async {
        let (session_id, epoch) = machine_session(&fixture).await;

        // Machine scope: the grant must become a Control/Machine credential the daemon admits.
        let machine_token = issue_and_apply(&fixture.state.auth_manager, AccountGrantScope::Machine).await;
        let (machine_device_token, machine_device) = fixture
            .state
            .auth_manager
            .exchange_pairing_code(&machine_token, "Seam phone")
            .expect("a machine-scope pairing token is exchangeable");
        assert_eq!(
            machine_device.permission,
            DevicePermission::Control,
            "the sink must map a machine grant to Control, the only permission machine routes accept"
        );
        assert_eq!(
            machine_device.access_scope,
            DeviceAccessScope::Machine,
            "the scope must reach the device record, because the terminal route dispatches on it"
        );
        assert_eq!(
            fixture
                .state
                .auth_manager
                .validate_token(&machine_device_token)
                .expect("the issued token validates")
                .access_scope,
            DeviceAccessScope::Machine
        );
        dial(&fixture, &machine_device_token, &session_id, &epoch)
            .await
            .expect("a machine-scope grant must be admitted to a machine-only terminal");

        // Mirror scope: the same issuance path, a different declaration. The credential is
        // still Control, so the refusal proves the scope is what gates admission.
        let mirror_token = issue_and_apply(&fixture.state.auth_manager, AccountGrantScope::Mirror).await;
        let (mirror_device_token, mirror_device) = fixture
            .state
            .auth_manager
            .exchange_pairing_code(&mirror_token, "Seam mirror")
            .expect("a mirror-scope pairing token is exchangeable");
        assert_eq!(mirror_device.permission, DevicePermission::Control);
        assert_eq!(
            mirror_device.access_scope,
            DeviceAccessScope::Mirror,
            "a mirror grant must not be promoted to machine scope"
        );
        let Err(Error::Http(refusal)) = dial(&fixture, &mirror_device_token, &session_id, &epoch).await
        else {
            panic!("a mirror credential must be refused on a machine-only session");
        };
        assert_eq!(refusal.status().as_u16(), 403);
        let body = String::from_utf8_lossy(refusal.body().as_deref().unwrap_or_default()).into_owned();
        assert_eq!(
            body.lines().next(),
            Some("MACHINE_ACCESS_REQUIRED"),
            "a mirror credential must be refused for scope, not for a missing session: {body:?}"
        );

        // Both credentials coexist: applying the second grant did not overwrite the first, so a
        // phone pairing in either scope cannot disturb an already-admitted machine device.
        fixture
            .state
            .auth_manager
            .validate_token(&machine_device_token)
            .expect("the machine-scope token survives the mirror grant");
        dial(&fixture, &machine_device_token, &session_id, &epoch)
            .await
            .expect("the machine-scope credential is still admitted after a mirror grant");
    })
    .catch_unwind()
    .await;
    fixture.cleanup().await;
    match outcome {
        Ok(()) => {}
        Err(panic) => std::panic::resume_unwind(panic),
    }
}
