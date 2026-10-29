//! Router-level tests for install keys and machine approval requests (plan: Tests > L-key).

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use super::origin::DeploymentMode;
use super::peer_record::test_support::{http, identity};
use super::service::{router, AccountState};
use super::store::{now_secs, random_token, token_hash, MachineRecord, SessionRecord, UserRecord};
use crate::remote::account_protocol::AccountEnrollRequest;
use crate::remote::auth::MachineIdentity;

const LIMIT: Duration = Duration::from_secs(10);
const ORIGIN: &str = "https://relay.test";

struct Fixture {
    url: String,
    state: Arc<AccountState>,
    server: tokio::task::JoinHandle<()>,
    _tmp: tempfile::TempDir,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

async fn spawn(mode: DeploymentMode) -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let mailer = Arc::new(crate::account::mailer::FileMailer::with_dir(tmp.path().join("mail")));
    let state = Arc::new(AccountState::new(tmp.path(), ORIGIN, mailer).with_deployment_mode(mode));
    let app = router(Arc::clone(&state));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Fixture {
        url: format!("http://{addr}"),
        state,
        server,
        _tmp: tmp,
    }
}

/// Seeds a user with a live session and returns its bearer token.
fn seed_user(fx: &Fixture, user_id: &str, email: &str) -> String {
    let token = random_token();
    let (user_id, email, hash) = (user_id.to_string(), email.to_string(), token_hash(&token));
    fx.state
        .mutate(|store| {
            store.users.insert(
                user_id.clone(),
                UserRecord {
                    user_id: user_id.clone(),
                    email,
                    created_at: 1,
                },
            );
            store.sessions.insert(
                hash,
                SessionRecord {
                    user_id,
                    expires_at: now_secs() + 3600,
                },
            );
            Ok(())
        })
        .unwrap();
    token
}

fn seed_machine(fx: &Fixture, owner: &str, machine_id: &str, public_key: &str) {
    let (owner, machine_id, public_key) =
        (owner.to_string(), machine_id.to_string(), public_key.to_string());
    fx.state
        .mutate(|store| {
            store.machines.insert(
                format!("rec-{machine_id}"),
                MachineRecord {
                    machine_record_id: format!("rec-{machine_id}"),
                    owner_user_id: owner,
                    machine_id: machine_id.clone(),
                    display_name: machine_id,
                    public_key: public_key.clone(),
                    attach_public_key: public_key,
                    relay_origin: ORIGIN.to_string(),
                    platform: "linux".to_string(),
                    enrollment_epoch: 1,
                    enrolled_at: 1,
                    last_seen_at: 0,
                },
            );
            Ok(())
        })
        .unwrap();
}

async fn send(
    fx: &Fixture,
    method: reqwest::Method,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (u16, Value) {
    let mut builder = http().request(method, format!("{}{path}", fx.url));
    if let Some(token) = token {
        builder = builder.bearer_auth(token);
    }
    if let Some(body) = body {
        builder = builder.json(&body);
    }
    let response = tokio::time::timeout(LIMIT, builder.send())
        .await
        .expect("bounded request")
        .unwrap();
    let status = response.status().as_u16();
    (status, response.json().await.unwrap_or(Value::Null))
}

async fn post(fx: &Fixture, path: &str, token: Option<&str>, body: Value) -> (u16, Value) {
    send(fx, reqwest::Method::POST, path, token, Some(body)).await
}

async fn get(fx: &Fixture, path: &str, token: &str) -> (u16, Value) {
    send(fx, reqwest::Method::GET, path, Some(token), None).await
}

async fn issue_key(fx: &Fixture, token: &str) -> (u16, Value) {
    post(fx, "/api/account/v1/install-keys", Some(token), json!({ "label": "ci box" })).await
}

/// Full enroll over the wire: challenge, signed request, enroll.
async fn enroll(fx: &Fixture, machine: &MachineIdentity, code: &str) -> (u16, Value) {
    let (status, challenge) = post(
        fx,
        "/api/account/v1/machines/enroll/challenge",
        None,
        json!({ "machineId": machine.machine_id }),
    )
    .await;
    assert_eq!(status, 200, "{challenge}");
    let nonce = challenge["nonce"].as_str().unwrap().to_string();
    let now = now_secs();
    let code_hash = crate::remote::auth::enrollment_code_hash(code);
    let signature =
        crate::remote::auth::sign_account_enrollment(machine, ORIGIN, &code_hash, &nonce, now)
            .unwrap();
    let request = AccountEnrollRequest {
        api_version: 1,
        enrollment_code: code.to_string(),
        machine_id: machine.machine_id.clone(),
        display_name: machine.display_name.clone(),
        public_key: machine.public_key.clone(),
        attach_public_key: machine.public_key.clone(),
        platform: crate::remote::machine_protocol::Platform::Linux,
        app_version: "2026.1023.1".into(),
        nonce,
        timestamp: now,
        signature,
    };
    post(
        fx,
        "/api/account/v1/machines/enroll",
        None,
        serde_json::to_value(&request).unwrap(),
    )
    .await
}

fn install_key_row(fx: &Fixture, key: &str) -> Option<crate::account::store::EnrollmentCodeRecord> {
    let store = fx.state.load().unwrap();
    store
        .enrollment_codes
        .get(&crate::remote::auth::enrollment_code_hash(key))
        .cloned()
}

async fn create_request(fx: &Fixture, machine: &MachineIdentity, email: Option<&str>) -> Value {
    let mut body = json!({
        "machineId": machine.machine_id,
        "publicKey": machine.public_key,
        "displayName": "noye",
        "platform": "linux",
        "appVersion": "2026.1023.1",
    });
    if let Some(email) = email {
        body["email"] = json!(email);
    }
    let response = tokio::time::timeout(
        LIMIT,
        http()
            .post(format!("{}/api/account/v1/machine-requests", fx.url))
            .header("cf-connecting-ip", "198.51.100.7")
            .header("x-forwarded-for", "203.0.113.9")
            .json(&body)
            .send(),
    )
    .await
    .expect("bounded request")
    .unwrap();
    assert_eq!(response.status().as_u16(), 200);
    response.json().await.unwrap()
}

async fn lookup(fx: &Fixture, token: &str, user_code: &str) -> (u16, Value) {
    post(
        fx,
        "/api/account/v1/machine-requests/lookup",
        Some(token),
        json!({ "userCode": user_code }),
    )
    .await
}

async fn decide(fx: &Fixture, token: &str, request_id: &str, user_code: &str, verb: &str) -> (u16, Value) {
    post(
        fx,
        &format!("/api/account/v1/machine-requests/{request_id}/{verb}"),
        Some(token),
        json!({ "userCode": user_code }),
    )
    .await
}

async fn poll(fx: &Fixture, device_code: &str) -> (u16, Value) {
    post(
        fx,
        "/api/account/v1/machine-requests/poll",
        None,
        json!({ "deviceCode": device_code }),
    )
    .await
}

/// Clears the poll pacing marker so a test can poll again without waiting out the interval.
fn reset_poll_pacing(fx: &Fixture) {
    fx.state
        .mutate(|store| {
            for request in store.machine_requests.values_mut() {
                request.last_polled_at = None;
            }
            Ok(())
        })
        .unwrap();
}

#[tokio::test]
async fn install_key_issue_then_enroll_succeeds_and_records_consumption() {
    let fx = spawn(DeploymentMode::SelfHost).await;
    let token = seed_user(&fx, "user-a", "a@test.local");
    let (status, issued) = issue_key(&fx, &token).await;
    assert_eq!(status, 200, "{issued}");
    let key = issued["key"].as_str().unwrap().to_string();
    assert!(key.starts_with("fxk_") && key.len() == 68, "{key}");
    assert!(issued["keyId"].as_str().unwrap().starts_with("ik_"));
    assert_eq!(
        issued["installCommand"]["sh"],
        format!("curl -fsSL '{ORIGIN}/install.sh' | FERRYX_ORIGIN='{ORIGIN}' sh -s -- --key {key}")
    );
    assert!(issued["installCommand"]["ps1"].as_str().unwrap().contains(&key));
    let expires_at = issued["expiresAt"].as_u64().unwrap();
    assert!(expires_at > now_secs() + 3500 && expires_at <= now_secs() + 3600);

    let row = install_key_row(&fx, &key).expect("stored hashed");
    assert_eq!(row.kind, "install_key");
    assert!(row.consumed_at.is_none());

    let machine = identity("mach-key-1", 41);
    let (status, body) = enroll(&fx, &machine, &key).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["accountId"], "user-a");

    let row = install_key_row(&fx, &key).expect("consumed key is kept for audit");
    assert!(row.consumed_at.is_some());
    assert_eq!(row.consumed_machine_id.as_deref(), Some("mach-key-1"));
}

/// Mutation target: deleting the `consumed_at` check in `enroll` lets the second enroll
/// through (same owner re-enroll), which turns this test RED.
#[tokio::test]
async fn install_key_reuse_is_409_install_key_used() {
    let fx = spawn(DeploymentMode::SelfHost).await;
    let token = seed_user(&fx, "user-a", "a@test.local");
    let (_, issued) = issue_key(&fx, &token).await;
    let key = issued["key"].as_str().unwrap().to_string();
    let machine = identity("mach-reuse", 42);
    let (status, body) = enroll(&fx, &machine, &key).await;
    assert_eq!(status, 200, "{body}");

    let (status, body) = enroll(&fx, &machine, &key).await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["code"], "INSTALL_KEY_USED");
    let other = identity("mach-reuse-2", 43);
    let (status, body) = enroll(&fx, &other, &key).await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["code"], "INSTALL_KEY_USED");
}

#[tokio::test]
async fn expired_install_key_is_401_and_deleted() {
    let fx = spawn(DeploymentMode::SelfHost).await;
    let token = seed_user(&fx, "user-a", "a@test.local");
    let (_, issued) = issue_key(&fx, &token).await;
    let key = issued["key"].as_str().unwrap().to_string();
    let hash = crate::remote::auth::enrollment_code_hash(&key);
    fx.state
        .mutate(|store| {
            store.enrollment_codes.get_mut(&hash).unwrap().expires_at = now_secs() - 1;
            Ok(())
        })
        .unwrap();

    let (status, body) = enroll(&fx, &identity("mach-exp", 44), &key).await;
    assert_eq!(status, 401, "{body}");
    assert_eq!(body["code"], "INSTALL_KEY_EXPIRED");
    assert!(install_key_row(&fx, &key).is_none(), "expired key row is deleted");

    let (status, body) = enroll(&fx, &identity("mach-unknown", 45), "fxk_unknown").await;
    assert_eq!(status, 401, "{body}");
    assert_eq!(body["code"], "ENROLL_CODE_INVALID");
}

#[tokio::test]
async fn install_key_enrolls_into_issuer_and_refuses_machine_owned_by_another_account() {
    let fx = spawn(DeploymentMode::SelfHost).await;
    let token_a = seed_user(&fx, "user-a", "a@test.local");
    seed_user(&fx, "user-b", "b@test.local");
    let owned_by_b = identity("mach-of-b", 46);
    seed_machine(&fx, "user-b", &owned_by_b.machine_id, &owned_by_b.public_key);

    let (_, issued) = issue_key(&fx, &token_a).await;
    let key = issued["key"].as_str().unwrap().to_string();
    let (status, body) = enroll(&fx, &owned_by_b, &key).await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["code"], "ACCOUNT_MACHINE_CLAIMED");
    assert!(
        install_key_row(&fx, &key).unwrap().consumed_at.is_none(),
        "a refused enroll does not consume the key"
    );

    let fresh = identity("mach-of-a", 47);
    let (status, body) = enroll(&fx, &fresh, &key).await;
    assert_eq!(status, 200, "{body}");
    let store = fx.state.load().unwrap();
    let machine = store
        .machines
        .values()
        .find(|m| m.machine_id == "mach-of-a")
        .unwrap();
    assert_eq!(machine.owner_user_id, "user-a");
}

#[tokio::test]
async fn outstanding_install_key_limit_is_429() {
    let fx = spawn(DeploymentMode::SelfHost).await;
    let token = seed_user(&fx, "user-a", "a@test.local");
    for _ in 0..10 {
        let (status, body) = issue_key(&fx, &token).await;
        assert_eq!(status, 200, "{body}");
    }
    let (status, body) = issue_key(&fx, &token).await;
    assert_eq!(status, 429, "{body}");
    assert_eq!(body["code"], "INSTALL_KEY_LIMIT");

    // Revoking one frees a slot.
    let (_, list) = get(&fx, "/api/account/v1/install-keys", &token).await;
    let key_id = list[0]["keyId"].as_str().unwrap().to_string();
    let (status, _) = send(
        &fx,
        reqwest::Method::POST,
        &format!("/api/account/v1/install-keys/{key_id}/revoke"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, 204);
    let (status, body) = issue_key(&fx, &token).await;
    assert_eq!(status, 200, "{body}");
}

#[tokio::test]
async fn install_key_list_never_returns_the_key_and_requires_auth() {
    let fx = spawn(DeploymentMode::SelfHost).await;
    let token = seed_user(&fx, "user-a", "a@test.local");
    let other = seed_user(&fx, "user-b", "b@test.local");
    let (_, issued) = issue_key(&fx, &token).await;
    let key = issued["key"].as_str().unwrap().to_string();
    let hash = crate::remote::auth::enrollment_code_hash(&key);

    let (status, list) = get(&fx, "/api/account/v1/install-keys", &token).await;
    assert_eq!(status, 200, "{list}");
    let raw = list.to_string();
    assert!(!raw.contains(&key), "list must not leak the key: {raw}");
    assert!(!raw.contains(&hash), "list must not leak the key hash: {raw}");
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["keyId"], issued["keyId"]);
    assert_eq!(list[0]["label"], "ci box");
    assert!(list[0].get("key").is_none());

    let (_, foreign) = get(&fx, "/api/account/v1/install-keys", &other).await;
    assert_eq!(foreign.as_array().unwrap().len(), 0, "keys are per user");

    let (status, body) =
        send(&fx, reqwest::Method::GET, "/api/account/v1/install-keys", None, None).await;
    assert_eq!(status, 401, "{body}");
    let (status, body) = post(&fx, "/api/account/v1/install-keys", None, json!({})).await;
    assert_eq!(status, 401, "{body}");
}

#[tokio::test]
async fn billing_plan_limit_refuses_issue_early_and_does_not_consume_on_enroll() {
    let fx = spawn(DeploymentMode::Commercial).await;
    let token = seed_user(&fx, "user-a", "a@test.local");
    let (status, issued) = issue_key(&fx, &token).await;
    assert_eq!(status, 200, "{issued}");
    let key = issued["key"].as_str().unwrap().to_string();

    // Every machine slot of the free plan is now taken.
    for index in 0..crate::account::billing::entitlement::FREE_HOSTS {
        let existing = identity(&format!("mach-existing-{index}"), 200 + index as u8);
        seed_machine(&fx, "user-a", &existing.machine_id, &existing.public_key);
    }

    let (status, body) = issue_key(&fx, &token).await;
    assert_eq!(status, 402, "{body}");
    assert_eq!(body["code"], "PLAN_LIMIT_REACHED");

    let (status, body) = enroll(&fx, &identity("mach-second", 49), &key).await;
    assert_eq!(status, 402, "{body}");
    assert_eq!(body["code"], "PLAN_LIMIT_REACHED");
    let row = install_key_row(&fx, &key).expect("refused key is kept");
    assert!(row.consumed_at.is_none(), "plan-limit refusal does not consume the key");
}

#[tokio::test]
async fn machine_request_lookup_approve_poll_once_then_enroll() {
    let fx = spawn(DeploymentMode::SelfHost).await;
    let token = seed_user(&fx, "user-a", "a@test.local");
    let machine = identity("mach-approve", 50);
    let created = create_request(&fx, &machine, Some("A@Test.Local")).await;
    let user_code = created["userCode"].as_str().unwrap().to_string();
    let device_code = created["deviceCode"].as_str().unwrap().to_string();
    assert_eq!(created["verificationUri"], format!("{ORIGIN}/approve"));
    assert_eq!(
        created["verificationUriComplete"],
        format!("{ORIGIN}/approve?code={user_code}")
    );
    assert_eq!(created["expiresIn"], 900);
    assert_eq!(created["interval"], 3);
    assert!(crate::account::machine_requests::normalize_user_code(&user_code).as_deref()
        == Some(user_code.as_str()));

    let (status, pending) = get(&fx, "/api/account/v1/machine-requests", &token).await;
    assert_eq!(status, 200, "{pending}");
    assert_eq!(pending.as_array().unwrap().len(), 1, "email hint routes the request");

    let (status, view) = lookup(&fx, &token, &user_code.to_lowercase()).await;
    assert_eq!(status, 200, "{view}");
    assert_eq!(view["machineId"], "mach-approve");
    assert_eq!(view["displayName"], "noye");
    assert_eq!(view["platform"], "linux");
    assert_eq!(view["appVersion"], "2026.1023.1");
    assert_eq!(view["requesterIp"], "198.51.100.7");
    assert!(!view.to_string().contains(&device_code));
    let request_id = view["requestId"].as_str().unwrap().to_string();

    let (status, body) = decide(&fx, &token, &request_id, &user_code, "approve").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["machineId"], "mach-approve");

    let (status, polled) = poll(&fx, &device_code).await;
    assert_eq!(status, 200, "{polled}");
    assert_eq!(polled["status"], "approved");
    let code = polled["enrollmentCode"].as_str().unwrap().to_string();

    reset_poll_pacing(&fx);
    let (status, again) = poll(&fx, &device_code).await;
    assert_eq!(status, 400, "{again}");
    assert!(again.get("enrollmentCode").is_none(), "the code is delivered once");
    let store = fx.state.load().unwrap();
    assert!(
        store.machine_requests.values().all(|r| r.enrollment_code.is_none()),
        "the plaintext code is cleared after delivery"
    );
    assert!(
        !store.enrollment_codes.contains_key(&code),
        "the enrollment code is stored hashed"
    );

    let (status, body) = enroll(&fx, &machine, &code).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["accountId"], "user-a");
}

#[tokio::test]
async fn foreign_approver_with_email_hint_is_403() {
    let fx = spawn(DeploymentMode::SelfHost).await;
    seed_user(&fx, "user-a", "a@test.local");
    let intruder = seed_user(&fx, "user-b", "b@test.local");
    let created = create_request(&fx, &identity("mach-hint", 51), Some("a@test.local")).await;
    let user_code = created["userCode"].as_str().unwrap().to_string();

    let (status, body) = lookup(&fx, &intruder, &user_code).await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(body["code"], "MACHINE_REQUEST_FOREIGN_ACCOUNT");

    let request_id = fx.state.load().unwrap().machine_requests.keys().next().unwrap().clone();
    let (status, body) = decide(&fx, &intruder, &request_id, &user_code, "approve").await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(body["code"], "MACHINE_REQUEST_FOREIGN_ACCOUNT");

    let (_, pending) = get(&fx, "/api/account/v1/machine-requests", &intruder).await;
    assert_eq!(pending.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn decided_machine_request_is_409() {
    let fx = spawn(DeploymentMode::SelfHost).await;
    let token = seed_user(&fx, "user-a", "a@test.local");
    let created = create_request(&fx, &identity("mach-decided", 52), None).await;
    let user_code = created["userCode"].as_str().unwrap().to_string();
    let (_, view) = lookup(&fx, &token, &user_code).await;
    let request_id = view["requestId"].as_str().unwrap().to_string();

    let (status, _) = decide(&fx, &token, &request_id, &user_code, "deny").await;
    assert_eq!(status, 204);
    for verb in ["approve", "deny"] {
        let (status, body) = decide(&fx, &token, &request_id, &user_code, verb).await;
        assert_eq!(status, 409, "{verb}: {body}");
        assert_eq!(body["code"], "MACHINE_REQUEST_DECIDED");
    }
}

#[tokio::test]
async fn approve_refuses_machine_owned_by_another_account() {
    let fx = spawn(DeploymentMode::SelfHost).await;
    let token = seed_user(&fx, "user-a", "a@test.local");
    seed_user(&fx, "user-b", "b@test.local");
    let machine = identity("mach-claimed", 53);
    seed_machine(&fx, "user-b", &machine.machine_id, &machine.public_key);
    let created = create_request(&fx, &machine, None).await;
    let user_code = created["userCode"].as_str().unwrap().to_string();
    let (_, view) = lookup(&fx, &token, &user_code).await;
    let request_id = view["requestId"].as_str().unwrap().to_string();
    let (status, body) = decide(&fx, &token, &request_id, &user_code, "approve").await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["code"], "ACCOUNT_MACHINE_CLAIMED");
}

#[tokio::test]
async fn bound_code_with_another_machine_or_key_is_401_mismatch() {
    let fx = spawn(DeploymentMode::SelfHost).await;
    let token = seed_user(&fx, "user-a", "a@test.local");
    let machine = identity("mach-bound", 54);
    let created = create_request(&fx, &machine, None).await;
    let user_code = created["userCode"].as_str().unwrap().to_string();
    let device_code = created["deviceCode"].as_str().unwrap().to_string();
    let (_, view) = lookup(&fx, &token, &user_code).await;
    let request_id = view["requestId"].as_str().unwrap().to_string();
    decide(&fx, &token, &request_id, &user_code, "approve").await;
    let (_, polled) = poll(&fx, &device_code).await;
    let code = polled["enrollmentCode"].as_str().unwrap().to_string();

    let (status, body) = enroll(&fx, &identity("mach-other", 55), &code).await;
    assert_eq!(status, 401, "{body}");
    assert_eq!(body["code"], "ENROLL_CODE_MACHINE_MISMATCH");

    // Same machine id, different key.
    let (status, body) = enroll(&fx, &identity("mach-bound", 56), &code).await;
    assert_eq!(status, 401, "{body}");
    assert_eq!(body["code"], "ENROLL_CODE_MACHINE_MISMATCH");

    // The refusals did not burn the code: the approved machine still enrolls.
    let (status, body) = enroll(&fx, &machine, &code).await;
    assert_eq!(status, 200, "{body}");
}

#[tokio::test]
async fn denied_machine_request_polls_denied() {
    let fx = spawn(DeploymentMode::SelfHost).await;
    let token = seed_user(&fx, "user-a", "a@test.local");
    let created = create_request(&fx, &identity("mach-deny", 57), None).await;
    let user_code = created["userCode"].as_str().unwrap().to_string();
    let device_code = created["deviceCode"].as_str().unwrap().to_string();
    let (_, view) = lookup(&fx, &token, &user_code).await;
    let request_id = view["requestId"].as_str().unwrap().to_string();
    let (status, _) = decide(&fx, &token, &request_id, &user_code, "deny").await;
    assert_eq!(status, 204);

    let (status, polled) = poll(&fx, &device_code).await;
    assert_eq!(status, 200, "{polled}");
    assert_eq!(polled["status"], "denied");
    assert!(polled.get("enrollmentCode").is_none());
}

#[tokio::test]
async fn polling_faster_than_interval_is_slow_down_and_expiry_is_typed() {
    let fx = spawn(DeploymentMode::SelfHost).await;
    let created = create_request(&fx, &identity("mach-slow", 58), None).await;
    let device_code = created["deviceCode"].as_str().unwrap().to_string();

    let (status, first) = poll(&fx, &device_code).await;
    assert_eq!(status, 200, "{first}");
    assert_eq!(first["status"], "pending");
    let (status, second) = poll(&fx, &device_code).await;
    assert_eq!(status, 400, "{second}");
    assert_eq!(second["code"], "slow_down");

    fx.state
        .mutate(|store| {
            for request in store.machine_requests.values_mut() {
                request.expires_at = now_secs() - 1;
            }
            Ok(())
        })
        .unwrap();
    let (status, body) = poll(&fx, &device_code).await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["code"], "MACHINE_REQUEST_EXPIRED");
}

#[tokio::test]
async fn machine_request_create_validates_and_rate_limits_per_ip() {
    let fx = spawn(DeploymentMode::SelfHost).await;
    let (status, body) = post(&fx, "/api/account/v1/machine-requests", None, json!({})).await;
    assert_eq!(status, 400, "{body}");
    let (status, body) = post(
        &fx,
        "/api/account/v1/machine-requests",
        None,
        json!({
            "machineId": "m",
            "publicKey": "not-a-key",
            "displayName": "d",
            "platform": "linux",
            "appVersion": "1",
        }),
    )
    .await;
    assert_eq!(status, 400, "{body}");

    for index in 0..10u8 {
        create_request(&fx, &identity(&format!("mach-rate-{index}"), 60 + index), None).await;
    }
    let machine = identity("mach-rate-x", 90);
    let response = http()
        .post(format!("{}/api/account/v1/machine-requests", fx.url))
        .header("cf-connecting-ip", "198.51.100.7")
        .json(&json!({
            "machineId": machine.machine_id,
            "publicKey": machine.public_key,
            "displayName": "x",
            "platform": "linux",
            "appVersion": "1",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 429);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["code"], "RATE_LIMITED");
}

#[tokio::test]
async fn lookup_is_rate_limited_per_user() {
    let fx = spawn(DeploymentMode::SelfHost).await;
    let token = seed_user(&fx, "user-a", "a@test.local");
    for _ in 0..20 {
        let (status, body) = lookup(&fx, &token, "ABCD-EFGH").await;
        assert_eq!(status, 404, "{body}");
    }
    let (status, body) = lookup(&fx, &token, "ABCD-EFGH").await;
    assert_eq!(status, 429, "{body}");
    assert_eq!(body["code"], "RATE_LIMITED");
}

#[test]
fn onboarding_records_round_trip_through_sqlite() {
    use crate::account::store::{AccountStore, EnrollmentCodeRecord, MachineRequestRecord};
    let dir = tempfile::tempdir().unwrap();
    let mut store = AccountStore::default();
    store.users.insert(
        "u1".into(),
        UserRecord {
            user_id: "u1".into(),
            email: "u1@test.local".into(),
            created_at: 1,
        },
    );
    let key = EnrollmentCodeRecord {
        user_id: "u1".into(),
        account_origin: ORIGIN.into(),
        expires_at: now_secs() + 600,
        kind: "install_key".into(),
        key_id: Some("ik_1".into()),
        label: Some("label".into()),
        created_at: 5,
        consumed_at: Some(6),
        consumed_machine_id: Some("m1".into()),
        bound_machine_id: None,
        bound_public_key: None,
    };
    let bound = EnrollmentCodeRecord {
        user_id: "u1".into(),
        account_origin: ORIGIN.into(),
        expires_at: now_secs() + 600,
        bound_machine_id: Some("m2".into()),
        bound_public_key: Some("pk".into()),
        ..Default::default()
    };
    let request = MachineRequestRecord {
        request_id: "mreq_1".into(),
        device_code_hash: "dch".into(),
        user_code: "ABCD-EFGH".into(),
        machine_id: "m2".into(),
        public_key: "pk".into(),
        display_name: "box".into(),
        platform: "linux".into(),
        app_version: "1".into(),
        email_hint: Some("u1@test.local".into()),
        requester_ip: "198.51.100.7".into(),
        created_at: 7,
        expires_at: now_secs() + 900,
        status: "approved".into(),
        decided_by: Some("u1".into()),
        decided_at: Some(8),
        enrollment_code: Some("plain".into()),
        last_polled_at: Some(9),
    };
    store.enrollment_codes.insert("key-hash".into(), key.clone());
    store.enrollment_codes.insert("bound-hash".into(), bound.clone());
    store.machine_requests.insert("mreq_1".into(), request.clone());
    store.save(dir.path()).unwrap();

    let loaded = AccountStore::load(dir.path()).unwrap();
    assert_eq!(loaded.enrollment_codes.get("key-hash"), Some(&key));
    assert_eq!(loaded.enrollment_codes.get("bound-hash"), Some(&bound));
    assert_eq!(loaded.enrollment_codes.get("bound-hash").unwrap().kind, "code");
    assert_eq!(loaded.machine_requests.get("mreq_1"), Some(&request));
}
