use super::*;
use crate::account::billing::entitlement::GRACE_SECS;
use crate::account::mailer::FileMailer;
use crate::account::origin::DeploymentMode;
use crate::account::store::{now_secs, SessionRecord};
use axum::response::{IntoResponse, Response};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::{Signer, SigningKey};
use parking_lot::Mutex;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;
use tokio::sync::{oneshot, Notify};

const PRO_MONTHLY_VARIANT: &str = "11";
const PRO_ANNUAL_VARIANT: &str = "12";
const TEAM_MONTHLY_VARIANT: &str = "13";
const TEAM_ANNUAL_VARIANT: &str = "14";
const TEAM_HOSTPACK_VARIANT: &str = "15";
const STORE_ID: &str = "1";
const WEBHOOK_SECRET: &str = "test-webhook-secret";
const ITEM_ID: &str = "900";
const T1: &str = "2026-09-30T10:00:01.000000Z";
const T2: &str = "2026-09-30T12:00:00.000000Z";
const T3: &str = "2026-09-30T13:00:00.000000Z";

#[derive(Default)]
struct StubData {
    subscriptions: BTreeMap<String, serde_json::Value>,
    item_patches: Vec<(String, u32)>,
    item_patch_attempts: Vec<(String, u32)>,
    subscription_gets: Vec<String>,
    checkouts: Vec<serde_json::Value>,
    fail_patch: bool,
    fail_second_patch: bool,
    fail_get: bool,
    patch_gate_tx: Option<oneshot::Sender<()>>,
    patch_gate_rx: Option<Arc<Notify>>,
}

#[derive(Clone)]
struct LsStub {
    data: Arc<Mutex<StubData>>,
}

async fn stub_get_subscription(
    State(stub): State<LsStub>,
    AxumPath(subscription_id): AxumPath<String>,
) -> Response {
    let mut data = stub.data.lock();
    data.subscription_gets.push(subscription_id.clone());
    if data.fail_get {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"errors": [{"detail": "provider get outage"}]})),
        )
            .into_response();
    }
    match data.subscriptions.get(&subscription_id) {
        Some(value) => (StatusCode::OK, Json(value.clone())).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({"errors": [{"detail": "subscription not found"}]})),
        )
            .into_response(),
    }
}

async fn stub_patch_item(
    State(stub): State<LsStub>,
    AxumPath(item_id): AxumPath<String>,
    body: Bytes,
) -> Response {
    let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
    let quantity = parsed["data"]["attributes"]["quantity"]
        .as_u64()
        .unwrap_or(0) as u32;

    let (gate_tx, gate_rx) = {
        let mut data = stub.data.lock();
        (data.patch_gate_tx.take(), data.patch_gate_rx.clone())
    };
    if let Some(tx) = gate_tx {
        let _ = tx.send(());
    }
    if let Some(rx) = gate_rx {
        rx.notified().await;
    }

    let mut data = stub.data.lock();
    data.item_patch_attempts.push((item_id.clone(), quantity));
    let is_second = data.item_patch_attempts.len() > 1;
    if data.fail_patch || (data.fail_second_patch && is_second) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "errors": [{ "detail": "quantity update failed" }] })),
        )
            .into_response();
    }
    data.item_patches.push((item_id.clone(), quantity));
    for value in data.subscriptions.values_mut() {
        let item = &mut value["data"]["attributes"]["first_subscription_item"];
        if item["id"].as_str() == Some(item_id.as_str()) {
            item["quantity"] = json!(quantity);
        }
    }
    (
        StatusCode::OK,
        Json(json!({
            "data": {
                "type": "subscription-items",
                "id": item_id,
                "attributes": { "quantity": quantity }
            }
        })),
    )
        .into_response()
}

async fn stub_create_checkout(State(stub): State<LsStub>, body: Bytes) -> Response {
    let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
    stub.data.lock().checkouts.push(parsed);
    (
        StatusCode::OK,
        Json(json!({
            "data": {
                "type": "checkouts",
                "id": "1",
                "attributes": { "url": "https://store.lemonsqueezy.com/checkout/buy/stub" }
            }
        })),
    )
        .into_response()
}

struct TestServer {
    state: Arc<AccountState>,
    stub: LsStub,
    url: String,
    data_dir: std::path::PathBuf,
    mail_dir: std::path::PathBuf,
    _tmp: tempfile::TempDir,
}

async fn spawn_ls_stub(fixtures: Vec<serde_json::Value>) -> (String, LsStub) {
    let mut data = StubData::default();
    for fixture in fixtures {
        let id = fixture["data"]["id"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        data.subscriptions.insert(id, fixture);
    }
    let stub = LsStub {
        data: Arc::new(Mutex::new(data)),
    };
    let router = Router::new()
        .route("/subscriptions/{id}", get(stub_get_subscription))
        .route(
            "/subscription-items/{id}",
            axum::routing::patch(stub_patch_item),
        )
        .route("/checkouts", post(stub_create_checkout))
        .with_state(stub.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    (format!("http://{addr}"), stub)
}

fn test_ls_config(api_base_url: &str) -> LemonSqueezyConfig {
    LemonSqueezyConfig {
        api_key: "test-api-key".to_string(),
        store_id: STORE_ID.to_string(),
        webhook_secret: WEBHOOK_SECRET.to_string(),
        variant_pro_monthly: PRO_MONTHLY_VARIANT.to_string(),
        variant_pro_annual: PRO_ANNUAL_VARIANT.to_string(),
        variant_team_monthly: TEAM_MONTHLY_VARIANT.to_string(),
        variant_team_annual: TEAM_ANNUAL_VARIANT.to_string(),
        variant_team_hostpack_annual: TEAM_HOSTPACK_VARIANT.to_string(),
        api_base_url: Some(api_base_url.to_string()),
        expect_test_mode: true,
    }
}

fn unconfigured_ls_config() -> LemonSqueezyConfig {
    LemonSqueezyConfig {
        api_key: String::new(),
        store_id: String::new(),
        webhook_secret: String::new(),
        variant_pro_monthly: String::new(),
        variant_pro_annual: String::new(),
        variant_team_monthly: String::new(),
        variant_team_annual: String::new(),
        variant_team_hostpack_annual: String::new(),
        api_base_url: None,
        expect_test_mode: false,
    }
}

async fn spawn_billing_server(
    mode: DeploymentMode,
    ls: LemonSqueezyConfig,
    stub: LsStub,
) -> TestServer {
    let tmp = tempfile::tempdir().unwrap();
    let mail_dir = tmp.path().join("mail");
    let mailer = Arc::new(FileMailer::with_dir(mail_dir.clone()));
    let state = Arc::new(
        AccountState::new(tmp.path(), "https://relay.test", mailer).with_deployment_mode(mode),
    );
    let router = billing_routes(ls).with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    TestServer {
        state: state.clone(),
        stub,
        url: format!("http://{addr}"),
        data_dir: state.data_dir.clone(),
        mail_dir,
        _tmp: tmp,
    }
}

async fn start_commercial_server() -> TestServer {
    let (api_base, stub) = spawn_ls_stub(Vec::new()).await;
    spawn_billing_server(DeploymentMode::Commercial, test_ls_config(&api_base), stub).await
}

fn seed_store(data_dir: &Path, seed: impl FnOnce(&mut AccountStore)) {
    let mut store = AccountStore::load(data_dir).expect("load store");
    seed(&mut store);
    store.save(data_dir).expect("save store");
}

fn read_store(data_dir: &Path) -> AccountStore {
    AccountStore::load(data_dir).expect("load store")
}

fn seed_user(store: &mut AccountStore, user_id: &str, email: &str) -> String {
    store.users.insert(
        user_id.to_string(),
        UserRecord {
            user_id: user_id.to_string(),
            email: email.to_string(),
            created_at: 1,
        },
    );
    let token = format!("session-{user_id}");
    store.sessions.insert(
        token_hash(&token),
        SessionRecord {
            user_id: user_id.to_string(),
            expires_at: now_secs() + 3_600,
        },
    );
    token
}

fn seed_machine(store: &mut AccountStore, owner_user_id: &str, machine_id: &str, public_key: &str) {
    let machine_record_id = format!("rec-{machine_id}");
    store.machines.insert(
        machine_record_id.clone(),
        MachineRecord {
            machine_record_id,
            owner_user_id: owner_user_id.to_string(),
            machine_id: machine_id.to_string(),
            display_name: machine_id.to_string(),
            public_key: public_key.to_string(),
            attach_public_key: String::new(),
            relay_origin: "https://relay.test".to_string(),
            platform: "test".to_string(),
            enrollment_epoch: 1,
            enrolled_at: 1,
            last_seen_at: 0,
        },
    );
}

#[allow(clippy::too_many_arguments)]
fn seed_subscription(
    store: &mut AccountStore,
    subscription_id: &str,
    owner_user_id: &str,
    org_id: Option<&str>,
    plan_key: &str,
    kind: &str,
    seats: u32,
    host_packs: u32,
    status: &str,
    ls_updated_at: u64,
) {
    store.subscriptions.insert(
        subscription_id.to_string(),
        SubscriptionRecord {
            subscription_id: subscription_id.to_string(),
            owner_user_id: owner_user_id.to_string(),
            org_id: org_id.map(str::to_string),
            plan_key: plan_key.to_string(),
            seats,
            host_packs,
            kind: kind.to_string(),
            status: status.to_string(),
            ends_at: None,
            ls_customer_id: Some("1".to_string()),
            ls_updated_at,
            manage_url: Some("https://store.lemonsqueezy.com/billing".to_string()),
        },
    );
}

fn seed_org(store: &mut AccountStore, org_id: &str, owner_user_id: &str, members: &[(&str, &str)]) {
    store.orgs.insert(
        org_id.to_string(),
        OrgRecord {
            org_id: org_id.to_string(),
            owner_user_id: owner_user_id.to_string(),
            name: "team".to_string(),
            created_at: 1,
        },
    );
    for (user_id, role) in members {
        store.org_members.insert(
            member_key(org_id, user_id),
            OrgMemberRecord {
                org_id: org_id.to_string(),
                user_id: user_id.to_string(),
                role: role.to_string(),
                joined_at: 1,
            },
        );
    }
}

fn subscription_fixture(
    subscription_id: &str,
    variant_id: &str,
    quantity: u32,
    status: &str,
    updated_at: &str,
) -> serde_json::Value {
    json!({
        "data": {
            "type": "subscriptions",
            "id": subscription_id,
            "attributes": {
                "store_id": STORE_ID,
                "customer_id": "1",
                "variant_id": variant_id,
                "status": status,
                "first_subscription_item": { "id": ITEM_ID, "quantity": quantity },
                "ends_at": null,
                "updated_at": updated_at,
                "test_mode": true,
                "urls": { "customer_portal": "https://store.lemonsqueezy.com/billing" }
            }
        }
    })
}

fn subscription_event_body(
    event_name: &str,
    subscription_id: &str,
    variant_id: &str,
    quantity: u32,
    status: &str,
    updated_at: &str,
    custom_data: serde_json::Value,
) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "meta": { "event_name": event_name, "custom_data": custom_data },
        "data": {
            "type": "subscriptions",
            "id": subscription_id,
            "attributes": {
                "store_id": STORE_ID,
                "customer_id": "1",
                "variant_id": variant_id,
                "status": status,
                "first_subscription_item": { "id": ITEM_ID, "quantity": quantity },
                "ends_at": null,
                "updated_at": updated_at,
                "test_mode": true,
                "urls": { "customer_portal": "https://store.lemonsqueezy.com/billing" }
            }
        }
    }))
    .unwrap()
}

fn invoice_event_body(event_name: &str, subscription_id: &str, invoice_id: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "meta": { "event_name": event_name },
        "data": {
            "type": "subscription-invoices",
            "id": invoice_id,
            "attributes": {
                "store_id": STORE_ID,
                "customer_id": "1",
                "subscription_id": subscription_id,
                "updated_at": T3,
                "test_mode": true
            }
        }
    }))
    .unwrap()
}

fn sign_webhook(body: &[u8]) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    let mut mac = Hmac::<Sha256>::new_from_slice(WEBHOOK_SECRET.as_bytes()).expect("hmac key");
    mac.update(body);
    mac.finalize()
        .into_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn machine_signing_key() -> (SigningKey, String) {
    let key = SigningKey::from_bytes(&[3u8; 32]);
    let public_key = STANDARD.encode(key.verifying_key().as_bytes());
    (key, public_key)
}

fn machine_signature(key: &SigningKey, machine_id: &str, ts: u64) -> String {
    STANDARD.encode(key.sign(format!("{machine_id}:{ts}").as_bytes()).to_bytes())
}

async fn response_json(response: reqwest::Response) -> serde_json::Value {
    response.json().await.unwrap_or(serde_json::Value::Null)
}

async fn post_webhook(
    server: &TestServer,
    client: &reqwest::Client,
    body: &[u8],
    signature: &str,
) -> (u16, serde_json::Value) {
    let response = client
        .post(format!("{}/api/account/v1/billing/webhook", server.url))
        .header(WEBHOOK_SIGNATURE_HEADER, signature)
        .body(body.to_vec())
        .send()
        .await
        .expect("webhook request");
    let status = response.status().as_u16();
    (status, response_json(response).await)
}

async fn post_signed_webhook(
    server: &TestServer,
    client: &reqwest::Client,
    body: &[u8],
) -> (u16, serde_json::Value) {
    let signature = sign_webhook(body);
    post_webhook(server, client, body, &signature).await
}

async fn get_entitlement(
    server: &TestServer,
    client: &reqwest::Client,
    token: Option<&str>,
) -> (u16, serde_json::Value) {
    let request = client.get(format!("{}/api/account/v1/billing/entitlement", server.url));
    let request = match token {
        Some(token) => request.bearer_auth(token),
        None => request,
    };
    let response = request.send().await.expect("entitlement request");
    let status = response.status().as_u16();
    (status, response_json(response).await)
}

async fn post_lease(
    server: &TestServer,
    client: &reqwest::Client,
    machine_id: &str,
    ts: u64,
    signature: &str,
) -> (u16, serde_json::Value) {
    let response = client
        .post(format!("{}/api/account/v1/billing/lease", server.url))
        .json(&json!({ "machineId": machine_id, "ts": ts, "sig": signature }))
        .send()
        .await
        .expect("lease request");
    let status = response.status().as_u16();
    (status, response_json(response).await)
}

fn post_json(
    client: &reqwest::Client,
    url: String,
    token: &str,
    body: serde_json::Value,
) -> reqwest::RequestBuilder {
    client.post(url).bearer_auth(token).json(&body)
}

fn snapshot_mail_paths(mail_dir: &Path) -> std::collections::BTreeSet<std::path::PathBuf> {
    if !mail_dir.exists() {
        return std::collections::BTreeSet::new();
    }
    let mut paths = std::collections::BTreeSet::new();
    for entry in std::fs::read_dir(mail_dir).expect("read mail dir") {
        let entry = entry.expect("mail dir entry");
        paths.insert(entry.path());
    }
    paths
}

fn parse_token_from_mail_body(body: &str) -> String {
    body.split("token=")
        .nth(1)
        .expect("invite link carries the token")
        .split_whitespace()
        .next()
        .expect("invite token value")
        .to_string()
}

fn invite_token_from_new_mail(
    mail_dir: &Path,
    before: &std::collections::BTreeSet<std::path::PathBuf>,
) -> String {
    let current = snapshot_mail_paths(mail_dir);
    let new_paths: Vec<_> = current.difference(before).cloned().collect();
    assert_eq!(
        new_paths.len(),
        1,
        "operation must create exactly one new mail file without deleting or hiding"
    );
    let body = std::fs::read_to_string(&new_paths[0]).expect("new invite mail content");
    parse_token_from_mail_body(&body)
}

fn login_code_from_mail(mail_dir: &Path) -> String {
    let entries: Vec<_> = std::fs::read_dir(mail_dir)
        .expect("mail dir")
        .filter_map(|entry| entry.ok())
        .collect();
    assert_eq!(entries.len(), 1, "login request sends exactly one mail");
    let body = std::fs::read_to_string(entries[0].path()).expect("magic link content");
    body.split("code=")
        .nth(1)
        .expect("magic link carries the login code")
        .split_whitespace()
        .next()
        .expect("login code value")
        .to_string()
}

#[test]
fn ignored_outcome_never_downgrades_an_applied_row() {
    let mut store = AccountStore::default();
    record_payment_event(
        &mut store,
        "applied-key",
        "subscription_updated",
        10,
        "applied",
    );
    let wrote = record_outcome_unless_applied(
        &mut store,
        "applied-key",
        "subscription_updated",
        20,
        "ignored:stale",
    );
    assert!(
        !wrote,
        "a concurrent applied outcome wins over a later ignore"
    );
    assert_eq!(store.payment_events["applied-key"].outcome, "applied");
    assert_eq!(store.payment_events["applied-key"].applied_at, Some(10));

    let wrote = record_outcome_unless_applied(
        &mut store,
        "fresh-key",
        "subscription_updated",
        20,
        "ignored:stale",
    );
    assert!(wrote);
    assert_eq!(store.payment_events["fresh-key"].outcome, "ignored:stale");

    record_payment_event(
        &mut store,
        "rejected-key",
        "subscription_updated",
        10,
        "rejected:signature",
    );
    let wrote = record_outcome_unless_applied(
        &mut store,
        "rejected-key",
        "subscription_updated",
        20,
        "ignored:owner_mismatch",
    );
    assert!(wrote, "a rejection is not an authenticated outcome");
    assert_eq!(
        store.payment_events["rejected-key"].outcome,
        "ignored:owner_mismatch"
    );
}

#[test]
fn billing_clock_offset_is_saturating() {
    assert_eq!(apply_clock_offset(1_000, -400), 600);
    assert_eq!(apply_clock_offset(1_000, 400), 1_400);
    assert_eq!(apply_clock_offset(1_000, 0), 1_000);
    assert_eq!(apply_clock_offset(10, -400), 0);
}

#[test]
fn duplicate_membership_rows_count_machines_once() {
    let mut store = AccountStore::default();
    seed_user(&mut store, "u1", "owner@test.local");
    seed_user(&mut store, "u2", "member@test.local");
    seed_org(&mut store, "org-1", "u1", &[("u1", ROLE_OWNER)]);
    for (index, role) in [ROLE_MEMBER, ROLE_ADMIN].into_iter().enumerate() {
        store.org_members.insert(
            format!("duplicate-{index}"),
            OrgMemberRecord {
                org_id: "org-1".to_string(),
                user_id: "u2".to_string(),
                role: role.to_string(),
                joined_at: 1,
            },
        );
    }
    seed_machine(&mut store, "u1", "m-owner", "owner-key");
    seed_machine(&mut store, "u2", "m-member", "member-key");

    let owner_scope = owner_scope_for_user(&store, "u1");
    assert_eq!(owner_scope.org_id.as_deref(), Some("org-1"));
    assert_eq!(
        owner_scope.user_ids,
        vec!["u1".to_string(), "u2".to_string()],
        "a member with two rows is one pool member"
    );
    assert_eq!(machines_used(&store, &owner_scope), 2);

    let member_scope = owner_scope_for_user(&store, "u2");
    assert_eq!(member_scope.org_id.as_deref(), Some("org-1"));
    assert_eq!(
        machines_used(&store, &member_scope),
        2,
        "each machine is counted once even when a membership row is duplicated"
    );
}

#[tokio::test]
async fn duplicate_webhook_applies_once() {
    let server = start_commercial_server().await;
    let client = reqwest::Client::new();
    seed_store(&server.data_dir, |store| {
        seed_user(store, "u1", "u1@test.local");
    });

    let body = subscription_event_body(
        "subscription_created",
        "555",
        PRO_MONTHLY_VARIANT,
        1,
        "active",
        T1,
        json!({ "user_id": "u1" }),
    );

    let (status, payload) = post_signed_webhook(&server, &client, &body).await;
    assert_eq!(status, 200);
    assert_eq!(payload["status"], "applied");

    let store = read_store(&server.data_dir);
    let record = store.subscriptions.get("555").expect("subscription stored");
    assert_eq!(record.plan_key, "pro_monthly");
    assert_eq!(record.kind, KIND_BASE);
    assert_eq!(record.host_packs, 0);
    assert_eq!(record.seats, 0);
    assert_eq!(record.status, "active");
    assert_eq!(record.owner_user_id, "u1");
    assert!(record.org_id.is_none());
    assert_eq!(store.payment_events.len(), 1);
    let event_key_hash = event_key(&body);
    assert_eq!(
        store
            .payment_events
            .get(&event_key_hash)
            .map(|e| e.outcome.as_str()),
        Some("applied")
    );

    let applied_updated_at = record.ls_updated_at;
    seed_store(&server.data_dir, |store| {
        let record = store.subscriptions.get_mut("555").expect("subscription");
        record.host_packs = 7;
        record.ls_updated_at = applied_updated_at + 10_000;
    });

    let (replay_status, replay_payload) = post_signed_webhook(&server, &client, &body).await;
    assert_eq!(replay_status, 200);
    assert_eq!(replay_payload["status"], "duplicate");

    let store = read_store(&server.data_dir);
    let record = store
        .subscriptions
        .get("555")
        .expect("subscription still stored");
    assert_eq!(record.host_packs, 7, "a replay must not re-apply the event");
    assert_eq!(record.ls_updated_at, applied_updated_at + 10_000);
    assert_eq!(
        store.payment_events.len(),
        1,
        "a replay records no second event"
    );
}

#[tokio::test]
async fn tampered_signature_is_rejected() {
    let server = start_commercial_server().await;
    let client = reqwest::Client::new();
    seed_store(&server.data_dir, |store| {
        seed_user(store, "u1", "u1@test.local");
    });

    let body = subscription_event_body(
        "subscription_created",
        "556",
        PRO_MONTHLY_VARIANT,
        1,
        "active",
        T1,
        json!({ "user_id": "u1" }),
    );
    let mut signature = sign_webhook(&body);
    signature.replace_range(0..1, if signature.starts_with('a') { "b" } else { "a" });

    let (status, payload) = post_webhook(&server, &client, &body, &signature).await;
    assert_eq!(status, 401);
    assert_eq!(payload["code"], "WEBHOOK_SIGNATURE_INVALID");

    let store = read_store(&server.data_dir);
    assert!(
        store.subscriptions.is_empty(),
        "a rejected body never applies"
    );
    let key = event_key(&body);
    assert_eq!(
        store.payment_events.get(&key).map(|e| e.outcome.as_str()),
        Some("rejected:signature")
    );

    let garbage = b"{\"not\": \"a lemon squeezy webhook\"}";
    let (garbage_status, _) = post_webhook(&server, &client, garbage, "deadbeef").await;
    assert_eq!(garbage_status, 401);
    let store = read_store(&server.data_dir);
    assert_eq!(
        store.payment_events.len(),
        1,
        "only payloads shaped like a Lemon Squeezy webhook are recorded"
    );
}

#[tokio::test]
async fn rejected_signature_does_not_block_a_later_signed_delivery() {
    let server = start_commercial_server().await;
    let client = reqwest::Client::new();
    seed_store(&server.data_dir, |store| {
        seed_user(store, "u1", "u1@test.local");
    });

    let body = subscription_event_body(
        "subscription_created",
        "558",
        PRO_MONTHLY_VARIANT,
        1,
        "active",
        T1,
        json!({ "user_id": "u1" }),
    );
    let key = event_key(&body);
    let mut forged = sign_webhook(&body);
    forged.replace_range(0..1, if forged.starts_with('a') { "b" } else { "a" });

    let (forged_status, _) = post_webhook(&server, &client, &body, &forged).await;
    assert_eq!(forged_status, 401);
    let store = read_store(&server.data_dir);
    assert_eq!(
        store
            .payment_events
            .get(&key)
            .map(|event| event.outcome.as_str()),
        Some("rejected:signature")
    );
    assert!(store.subscriptions.is_empty());

    let (applied_status, applied) = post_signed_webhook(&server, &client, &body).await;
    assert_eq!(applied_status, 200);
    assert_eq!(applied["status"], "applied");
    let store = read_store(&server.data_dir);
    let record = store.payment_events.get(&key).expect("ledger row");
    assert_eq!(
        record.outcome, "applied",
        "the rejected row transitions inside the authenticated transaction"
    );
    assert!(record.applied_at.is_some());
    assert_eq!(store.subscriptions.len(), 1);
    assert_eq!(store.subscriptions["558"].plan_key, "pro_monthly");
    let applied_subscription = store.subscriptions["558"].clone();

    let (duplicate_status, duplicate) = post_signed_webhook(&server, &client, &body).await;
    assert_eq!(duplicate_status, 200);
    assert_eq!(duplicate["status"], "duplicate");

    let (downgrade_status, _) = post_webhook(&server, &client, &body, &forged).await;
    assert_eq!(downgrade_status, 401);
    let store = read_store(&server.data_dir);
    assert_eq!(
        store
            .payment_events
            .get(&key)
            .map(|event| event.outcome.as_str()),
        Some("applied"),
        "an unauthenticated retry can never downgrade an applied outcome"
    );
    assert_eq!(
        store.subscriptions.get("558").cloned(),
        Some(applied_subscription),
        "a forged retry cannot mutate applied subscription state"
    );

    let (after_status, after) = post_signed_webhook(&server, &client, &body).await;
    assert_eq!(after_status, 200);
    assert_eq!(after["status"], "duplicate");
}

#[tokio::test]
async fn out_of_order_update_is_ignored() {
    let server = start_commercial_server().await;
    let client = reqwest::Client::new();
    let mut token = String::new();
    seed_store(&server.data_dir, |store| {
        token = seed_user(store, "u1", "u1@test.local");
    });

    let newer = subscription_event_body(
        "subscription_updated",
        "557",
        PRO_MONTHLY_VARIANT,
        4,
        "active",
        T2,
        json!({ "user_id": "u1" }),
    );
    let (status, payload) = post_signed_webhook(&server, &client, &newer).await;
    assert_eq!(status, 200);
    assert_eq!(payload["status"], "applied");

    let older = subscription_event_body(
        "subscription_updated",
        "557",
        PRO_MONTHLY_VARIANT,
        10,
        "active",
        T1,
        json!({ "user_id": "u1" }),
    );
    let (stale_status, stale_payload) = post_signed_webhook(&server, &client, &older).await;
    assert_eq!(stale_status, 200);
    assert_eq!(stale_payload["status"], "ignored");

    let store = read_store(&server.data_dir);
    let record = store.subscriptions.get("557").expect("subscription");
    assert_eq!(
        record.host_packs, 3,
        "the stale event must not overwrite state"
    );
    assert_eq!(
        store
            .payment_events
            .get(&event_key(&older))
            .map(|event| event.outcome.as_str()),
        Some("ignored:stale")
    );

    let (entitlement_status, entitlement) = get_entitlement(&server, &client, Some(&token)).await;
    assert_eq!(entitlement_status, 200);
    assert_eq!(entitlement["plan"], "pro_monthly");
    assert_eq!(entitlement["machineLimit"], 25);
    assert_eq!(entitlement["hostPacks"], 3);
}

#[tokio::test]
async fn team_pool_counts_member_machines() {
    let server = start_commercial_server().await;
    let client = reqwest::Client::new();
    let mut member_token = String::new();
    seed_store(&server.data_dir, |store| {
        let _owner_token = seed_user(store, "u1", "owner@test.local");
        member_token = seed_user(store, "u2", "member@test.local");
        seed_org(
            store,
            "org-1",
            "u1",
            &[("u1", ROLE_OWNER), ("u2", ROLE_MEMBER)],
        );
        seed_subscription(
            store,
            "600",
            "u1",
            Some("org-1"),
            "team_monthly",
            KIND_BASE,
            2,
            0,
            "active",
            5,
        );
        seed_machine(store, "u1", "m-owner", "owner-key");
        seed_machine(store, "u2", "m-member-1", "member-key-1");
        seed_machine(store, "u2", "m-member-2", "member-key-2");
    });

    let (status, entitlement) = get_entitlement(&server, &client, Some(&member_token)).await;
    assert_eq!(status, 200);
    assert_eq!(entitlement["plan"], "team_monthly");
    assert_eq!(entitlement["status"], "ok");
    assert_eq!(entitlement["machineLimit"], 20);
    assert_eq!(entitlement["machinesUsed"], 3);
    assert_eq!(entitlement["seats"], 2);
    assert_eq!(entitlement["orgId"], "org-1");
    assert_eq!(entitlement["role"], ROLE_MEMBER);
    assert!(entitlement["graceEndsAt"].is_null());

    seed_store(&server.data_dir, |store| {
        for index in 0..18 {
            seed_machine(
                store,
                "u2",
                &format!("m-member-extra-{index}"),
                &format!("member-key-extra-{index}"),
            );
        }
    });

    let (over_status, over) = get_entitlement(&server, &client, Some(&member_token)).await;
    assert_eq!(over_status, 200);
    assert_eq!(over["status"], "over_limit");
    assert_eq!(over["machinesUsed"], 21);
    assert_eq!(over["machineLimit"], 20);
    let grace_ends_at = over["graceEndsAt"].as_u64().expect("grace deadline");
    assert!(grace_ends_at > billing_now());
    assert!(grace_ends_at <= billing_now() + GRACE_SECS + 5);

    let (repeat_status, repeat) = get_entitlement(&server, &client, Some(&member_token)).await;
    assert_eq!(repeat_status, 200);
    assert_eq!(
        repeat["graceEndsAt"].as_u64(),
        Some(grace_ends_at),
        "an unchanged violation keeps the original grace start"
    );
}

#[tokio::test]
async fn team_hostpack_subscription_adds_to_pool() {
    let server = start_commercial_server().await;
    let client = reqwest::Client::new();
    let mut owner_token = String::new();
    seed_store(&server.data_dir, |store| {
        owner_token = seed_user(store, "u1", "owner@test.local");
        seed_org(store, "org-1", "u1", &[("u1", ROLE_OWNER)]);
        seed_subscription(
            store,
            "601",
            "u1",
            Some("org-1"),
            "team_monthly",
            KIND_BASE,
            2,
            0,
            "active",
            5,
        );
    });

    let body = subscription_event_body(
        "subscription_created",
        "602",
        TEAM_HOSTPACK_VARIANT,
        2,
        "active",
        T1,
        json!({ "user_id": "u1", "org_id": "org-1" }),
    );
    let (status, payload) = post_signed_webhook(&server, &client, &body).await;
    assert_eq!(status, 200);
    assert_eq!(payload["status"], "applied");

    let store = read_store(&server.data_dir);
    let record = store.subscriptions.get("602").expect("host pack stored");
    assert_eq!(record.kind, KIND_HOSTPACK);
    assert_eq!(record.plan_key, "team_annual");
    assert_eq!(record.host_packs, 2);
    assert_eq!(record.seats, 0);
    assert_eq!(record.org_id.as_deref(), Some("org-1"));

    let (entitlement_status, entitlement) =
        get_entitlement(&server, &client, Some(&owner_token)).await;
    assert_eq!(entitlement_status, 200);
    assert_eq!(entitlement["machineLimit"], 30);
    assert_eq!(entitlement["hostPacks"], 2);
    assert_eq!(entitlement["seats"], 2);
}

#[tokio::test]
async fn invoice_event_fetches_authoritative_subscription() {
    let (api_base, stub) = spawn_ls_stub(vec![subscription_fixture(
        "700",
        PRO_MONTHLY_VARIANT,
        3,
        "active",
        T2,
    )])
    .await;
    let server =
        spawn_billing_server(DeploymentMode::Commercial, test_ls_config(&api_base), stub).await;
    let client = reqwest::Client::new();
    let mut token = String::new();
    seed_store(&server.data_dir, |store| {
        token = seed_user(store, "u1", "u1@test.local");
        seed_subscription(
            store,
            "700",
            "u1",
            None,
            "pro_monthly",
            KIND_BASE,
            0,
            0,
            "active",
            5,
        );
    });

    let body = invoice_event_body("subscription_payment_success", "700", "8001");
    let (status, payload) = post_signed_webhook(&server, &client, &body).await;
    assert_eq!(status, 200);
    assert_eq!(payload["status"], "applied");

    let store = read_store(&server.data_dir);
    let record = store.subscriptions.get("700").expect("subscription");
    assert_eq!(
        record.host_packs, 2,
        "authoritative quantity 3 means one paid pack"
    );
    assert_eq!(
        record.manage_url.as_deref(),
        Some("https://store.lemonsqueezy.com/billing")
    );

    let (entitlement_status, entitlement) = get_entitlement(&server, &client, Some(&token)).await;
    assert_eq!(entitlement_status, 200);
    assert_eq!(entitlement["machineLimit"], 20);
    assert_eq!(entitlement["machinesUsed"], 0);
}

#[tokio::test]
async fn quantity_route_updates_ls_and_entitlement() {
    let (api_base, stub) = spawn_ls_stub(vec![subscription_fixture(
        "701",
        PRO_MONTHLY_VARIANT,
        1,
        "active",
        T1,
    )])
    .await;
    let server =
        spawn_billing_server(DeploymentMode::Commercial, test_ls_config(&api_base), stub).await;
    let client = reqwest::Client::new();
    let mut token = String::new();
    seed_store(&server.data_dir, |store| {
        token = seed_user(store, "u1", "u1@test.local");
        seed_subscription(
            store,
            "701",
            "u1",
            None,
            "pro_monthly",
            KIND_BASE,
            0,
            0,
            "active",
            5,
        );
    });

    let response = post_json(
        &client,
        format!("{}/api/account/v1/billing/quantity", server.url),
        &token,
        json!({ "hostPacks": 3 }),
    )
    .send()
    .await
    .expect("quantity request");
    assert_eq!(response.status().as_u16(), 200);
    let body = response_json(response).await;
    assert_eq!(body["plan"], "pro_monthly");
    assert_eq!(
        body["machineLimit"].as_u64(),
        Some(25),
        "capacity follows the provider acknowledgement immediately"
    );
    assert_eq!(body["hostPacks"].as_u64(), Some(3));

    {
        let stub_state = server.stub.data.lock();
        assert_eq!(
            stub_state.item_patches,
            vec![(ITEM_ID.to_string(), 4)],
            "one PATCH with the graduated quantity for three packs"
        );
        assert_eq!(
            stub_state.subscription_gets,
            vec!["701".to_string(), "701".to_string()],
            "one read to resolve the item and one read after the update"
        );
    }

    let store = read_store(&server.data_dir);
    let record = store.subscriptions.get("701").expect("subscription");
    assert_eq!(record.host_packs, 3);
    assert_eq!(record.seats, 0);
    assert_eq!(record.plan_key, "pro_monthly");
    assert_eq!(
        record.ls_updated_at,
        crate::account::billing::lemonsqueezy::parse_timestamp(T1).expect("fixture timestamp")
    );

    let applied = subscription_event_body(
        "subscription_updated",
        "701",
        PRO_MONTHLY_VARIANT,
        4,
        "active",
        T2,
        json!({ "user_id": "u1" }),
    );
    let (webhook_status, webhook_payload) = post_signed_webhook(&server, &client, &applied).await;
    assert_eq!(webhook_status, 200);
    assert_eq!(webhook_payload["status"], "applied");

    let (status, entitlement) = get_entitlement(&server, &client, Some(&token)).await;
    assert_eq!(status, 200);
    assert_eq!(entitlement["machineLimit"].as_u64(), Some(25));
    assert_eq!(entitlement["hostPacks"].as_u64(), Some(3));
}

#[tokio::test]
async fn team_quantity_validates_before_the_first_patch() {
    let (api_base, stub) = spawn_ls_stub(vec![
        subscription_fixture("710", TEAM_MONTHLY_VARIANT, 2, "active", T1),
        subscription_fixture("711", TEAM_HOSTPACK_VARIANT, 1, "active", T1),
    ])
    .await;
    let server =
        spawn_billing_server(DeploymentMode::Commercial, test_ls_config(&api_base), stub).await;
    let client = reqwest::Client::new();
    let mut owner_token = String::new();
    seed_store(&server.data_dir, |store| {
        owner_token = seed_user(store, "u1", "owner@test.local");
        seed_org(store, "org-1", "u1", &[("u1", ROLE_OWNER)]);
        seed_subscription(
            store,
            "710",
            "u1",
            Some("org-1"),
            "team_monthly",
            KIND_BASE,
            2,
            0,
            "active",
            5,
        );
        seed_subscription(
            store,
            "711",
            "u1",
            Some("org-1"),
            "team_annual",
            KIND_HOSTPACK,
            0,
            1,
            "active",
            5,
        );
    });
    {
        let mut data = server.stub.data.lock();
        let fixture = data
            .subscriptions
            .get_mut("711")
            .expect("host pack fixture");
        fixture["data"]["attributes"]["first_subscription_item"]["id"] = json!("901");
    }

    let invalid = post_json(
        &client,
        format!("{}/api/account/v1/billing/quantity", server.url),
        &owner_token,
        json!({ "seats": 3, "hostPacks": 100_001 }),
    )
    .send()
    .await
    .expect("invalid quantity request");
    assert_eq!(invalid.status().as_u16(), 502);
    assert_eq!(
        response_json(invalid).await["code"],
        "BILLING_PROVIDER_ERROR"
    );
    assert!(
        server.stub.data.lock().item_patches.is_empty(),
        "no provider call may happen before every requested quantity is valid"
    );

    let applied = post_json(
        &client,
        format!("{}/api/account/v1/billing/quantity", server.url),
        &owner_token,
        json!({ "seats": 3, "hostPacks": 2 }),
    )
    .send()
    .await
    .expect("quantity request");
    assert_eq!(applied.status().as_u16(), 200);
    let body = response_json(applied).await;
    assert_eq!(body["plan"], "team_monthly");
    assert_eq!(body["seats"].as_u64(), Some(3));
    assert_eq!(body["hostPacks"].as_u64(), Some(2));
    assert_eq!(body["machineLimit"].as_u64(), Some(40));
    assert_eq!(
        server.stub.data.lock().item_patches,
        vec![(ITEM_ID.to_string(), 3), ("901".to_string(), 2)]
    );

    let store = read_store(&server.data_dir);
    assert_eq!(store.subscriptions["710"].seats, 3);
    assert_eq!(store.subscriptions["711"].host_packs, 2);
}

#[tokio::test]
async fn quantity_provider_failure_surfaces_and_changes_nothing() {
    let (api_base, stub) = spawn_ls_stub(vec![subscription_fixture(
        "702",
        PRO_MONTHLY_VARIANT,
        1,
        "active",
        T1,
    )])
    .await;
    let server =
        spawn_billing_server(DeploymentMode::Commercial, test_ls_config(&api_base), stub).await;
    let client = reqwest::Client::new();
    let mut token = String::new();
    seed_store(&server.data_dir, |store| {
        token = seed_user(store, "u1", "u1@test.local");
        seed_subscription(
            store,
            "702",
            "u1",
            None,
            "pro_monthly",
            KIND_BASE,
            0,
            0,
            "active",
            5,
        );
    });
    server.stub.data.lock().fail_patch = true;

    let response = post_json(
        &client,
        format!("{}/api/account/v1/billing/quantity", server.url),
        &token,
        json!({ "hostPacks": 3 }),
    )
    .send()
    .await
    .expect("quantity request");
    assert_eq!(response.status().as_u16(), 502);
    assert_eq!(
        response_json(response).await["code"],
        "BILLING_PROVIDER_ERROR"
    );

    let store = read_store(&server.data_dir);
    let record = store.subscriptions.get("702").expect("subscription");
    assert_eq!(
        record.host_packs, 0,
        "a failed provider call mutates nothing"
    );
    assert_eq!(record.ls_updated_at, 5);

    let (status, entitlement) = get_entitlement(&server, &client, Some(&token)).await;
    assert_eq!(status, 200);
    assert_eq!(entitlement["machineLimit"].as_u64(), Some(10));
}

#[tokio::test]
async fn checkout_returns_provider_url() {
    let server = start_commercial_server().await;
    let client = reqwest::Client::new();
    let mut token = String::new();
    seed_store(&server.data_dir, |store| {
        token = seed_user(store, "u1", "u1@test.local");
    });

    let response = post_json(
        &client,
        format!("{}/api/account/v1/billing/checkout", server.url),
        &token,
        json!({ "plan": "pro_monthly", "hostPacks": 2 }),
    )
    .send()
    .await
    .expect("checkout request");
    assert_eq!(response.status().as_u16(), 200);
    let body = response_json(response).await;
    assert_eq!(
        body["url"],
        "https://store.lemonsqueezy.com/checkout/buy/stub"
    );

    let checkouts = server.stub.data.lock().checkouts.clone();
    assert_eq!(checkouts.len(), 1);
    let attributes = &checkouts[0]["data"]["attributes"];
    assert_eq!(attributes["checkout_data"]["custom"]["user_id"], "u1");
    assert!(
        attributes["checkout_data"]["custom"]
            .get("org_id")
            .is_none(),
        "a Pro checkout stays on the personal pool"
    );
    assert_eq!(
        attributes["checkout_data"]["variant_quantities"][0]["quantity"],
        3
    );
}

#[tokio::test]
async fn team_checkout_creates_org_and_requires_owner_role() {
    let server = start_commercial_server().await;
    let client = reqwest::Client::new();
    let mut owner_token = String::new();
    let mut member_token = String::new();
    seed_store(&server.data_dir, |store| {
        owner_token = seed_user(store, "u1", "owner@test.local");
        member_token = seed_user(store, "u2", "member@test.local");
    });

    let response = post_json(
        &client,
        format!("{}/api/account/v1/billing/checkout", server.url),
        &owner_token,
        json!({ "plan": "team_monthly", "seats": 3 }),
    )
    .send()
    .await
    .expect("team checkout request");
    assert_eq!(response.status().as_u16(), 200);

    let store = read_store(&server.data_dir);
    let org = store.orgs.values().next().expect("team org created");
    assert_eq!(org.owner_user_id, "u1");
    let membership = store
        .org_members
        .get(&member_key(&org.org_id, "u1"))
        .expect("owner membership");
    assert_eq!(membership.role, ROLE_OWNER);
    let org_id = org.org_id.clone();

    let checkouts = server.stub.data.lock().checkouts.clone();
    assert_eq!(
        checkouts[0]["data"]["attributes"]["checkout_data"]["custom"]["org_id"],
        org_id.as_str()
    );
    assert_eq!(
        checkouts[0]["data"]["attributes"]["checkout_data"]["variant_quantities"][0]["quantity"],
        3
    );

    seed_store(&server.data_dir, |store| {
        store.org_members.insert(
            member_key(&org_id, "u2"),
            OrgMemberRecord {
                org_id: org_id.clone(),
                user_id: "u2".to_string(),
                role: ROLE_MEMBER.to_string(),
                joined_at: 1,
            },
        );
    });

    let forbidden = post_json(
        &client,
        format!("{}/api/account/v1/billing/checkout", server.url),
        &member_token,
        json!({ "plan": "team_monthly", "seats": 3 }),
    )
    .send()
    .await
    .expect("member team checkout");
    assert_eq!(forbidden.status().as_u16(), 403);
    assert_eq!(response_json(forbidden).await["code"], "ORG_ROLE_REQUIRED");

    let hostpack = post_json(
        &client,
        format!("{}/api/account/v1/billing/checkout", server.url),
        &owner_token,
        json!({ "plan": "team_hostpack_annual", "hostPacks": 1 }),
    )
    .send()
    .await
    .expect("host pack checkout");
    assert_eq!(hostpack.status().as_u16(), 409);
    assert_eq!(
        response_json(hostpack).await["code"],
        "TEAM_SUBSCRIPTION_REQUIRED"
    );
}

#[tokio::test]
async fn free_entitlement_works_without_ls_config() {
    let server = spawn_billing_server(
        DeploymentMode::Commercial,
        unconfigured_ls_config(),
        LsStub {
            data: Arc::new(Mutex::new(StubData::default())),
        },
    )
    .await;
    let client = reqwest::Client::new();
    let (signing_key, public_key) = machine_signing_key();
    let mut token = String::new();
    seed_store(&server.data_dir, |store| {
        token = seed_user(store, "u1", "u1@test.local");
        seed_machine(store, "u1", "machine-1", &public_key);
    });

    let (status, entitlement) = get_entitlement(&server, &client, Some(&token)).await;
    assert_eq!(status, 200);
    assert_eq!(entitlement["plan"], "free");
    assert_eq!(entitlement["status"], "ok");
    assert_eq!(entitlement["machineLimit"], 1);
    assert_eq!(entitlement["machinesUsed"], 1);
    assert!(entitlement["seats"].is_null());
    assert!(entitlement["manageUrl"].is_null());

    let ts = billing_now();
    let signature = machine_signature(&signing_key, "machine-1", ts);
    let (lease_status, lease_body) =
        post_lease(&server, &client, "machine-1", ts, &signature).await;
    assert_eq!(
        lease_status, 200,
        "a healthy Free host still gets its lease"
    );
    assert!(lease_body["lease"]
        .as_str()
        .is_some_and(|lease| !lease.is_empty()));

    let checkout = post_json(
        &client,
        format!("{}/api/account/v1/billing/checkout", server.url),
        &token,
        json!({ "plan": "pro_monthly" }),
    )
    .send()
    .await
    .expect("checkout request");
    assert_eq!(checkout.status().as_u16(), 503);
    assert_eq!(
        response_json(checkout).await["code"],
        "BILLING_UNCONFIGURED"
    );

    let quantity = post_json(
        &client,
        format!("{}/api/account/v1/billing/quantity", server.url),
        &token,
        json!({ "hostPacks": 1 }),
    )
    .send()
    .await
    .expect("quantity request");
    assert_eq!(quantity.status().as_u16(), 503);
}

#[tokio::test]
async fn lease_refused_when_stopped() {
    let server = start_commercial_server().await;
    let client = reqwest::Client::new();
    let (signing_key, public_key) = machine_signing_key();
    let grace_started_at = billing_now() - GRACE_SECS - 30;
    seed_store(&server.data_dir, |store| {
        seed_user(store, "u1", "u1@test.local");
        seed_machine(store, "u1", "machine-1", &public_key);
        seed_machine(store, "u1", "machine-2", "other-public-key");
        store.billing_states.insert(
            "u1".to_string(),
            BillingStateRecord {
                owner_key: "u1".to_string(),
                grace_started_at: Some(grace_started_at),
                stopped_at: None,
                last_notice: None,
            },
        );
    });

    let ts = billing_now();
    let signature = machine_signature(&signing_key, "machine-1", ts);
    let (status, payload) = post_lease(&server, &client, "machine-1", ts, &signature).await;
    assert_eq!(status, 402);
    assert_eq!(payload["code"], "REMOTE_SUSPENDED");
    assert!(payload["lease"].is_null(), "a stopped owner gets no lease");
    assert_eq!(payload["details"]["plan"], "free");
    assert_eq!(payload["details"]["status"], "stopped");
    assert_eq!(
        payload["details"]["graceEndsAt"].as_u64(),
        Some(grace_started_at + GRACE_SECS)
    );
    assert!(payload["details"]["stoppedAt"].as_u64().is_some());

    let store = read_store(&server.data_dir);
    let record = store.billing_states.get("u1").expect("billing state");
    assert!(record.stopped_at.is_some());
    assert_eq!(record.grace_started_at, Some(grace_started_at));
}

#[tokio::test]
async fn lease_expiry_is_capped_by_grace_end() {
    let server = start_commercial_server().await;
    let client = reqwest::Client::new();
    let (signing_key, public_key) = machine_signing_key();
    let grace_started_at = billing_now() - GRACE_SECS + 3_600;
    seed_store(&server.data_dir, |store| {
        seed_user(store, "u1", "u1@test.local");
        seed_machine(store, "u1", "machine-1", &public_key);
        seed_machine(store, "u1", "machine-2", "other-public-key");
        store.billing_states.insert(
            "u1".to_string(),
            BillingStateRecord {
                owner_key: "u1".to_string(),
                grace_started_at: Some(grace_started_at),
                stopped_at: None,
                last_notice: None,
            },
        );
    });

    let ts = billing_now();
    let signature = machine_signature(&signing_key, "machine-1", ts);
    let (status, payload) = post_lease(&server, &client, "machine-1", ts, &signature).await;
    assert_eq!(status, 200);
    let expires_at = payload["expiresAt"].as_u64().expect("expiry");
    assert_eq!(
        expires_at,
        grace_started_at + GRACE_SECS,
        "a lease inside the grace window expires at the suspension deadline"
    );
    assert!(expires_at < billing_now() + lease::LEASE_TTL_SECS);

    let lease = payload["lease"].as_str().expect("lease");
    let claims = lease::verify_lease(&server.state.account_public_key(), "machine-1", lease)
        .expect("the account signature verifies");
    assert_eq!(claims.expires_at, expires_at);
    assert_eq!(claims.owner_key, "u1");
}

#[tokio::test]
async fn lease_cap_holds_at_the_grace_boundary() {
    let server = start_commercial_server().await;
    let client = reqwest::Client::new();
    let (signing_key, public_key) = machine_signing_key();
    seed_store(&server.data_dir, |store| {
        seed_user(store, "u1", "u1@test.local");
        seed_machine(store, "u1", "machine-1", &public_key);
        seed_machine(store, "u1", "machine-2", "other-public-key");
    });

    let set_grace = |store: &mut AccountStore, started: u64| {
        store.billing_states.insert(
            "u1".to_string(),
            BillingStateRecord {
                owner_key: "u1".to_string(),
                grace_started_at: Some(started),
                stopped_at: None,
                last_notice: None,
            },
        );
    };
    let stored_state = |server: &TestServer| {
        read_store(&server.data_dir)
            .billing_states
            .get("u1")
            .cloned()
            .expect("billing state")
    };

    // Deadline further out than the TTL: the 24 hour allowance is the binding limit.
    let ttl_grace_start = billing_now();
    seed_store(&server.data_dir, |store| set_grace(store, ttl_grace_start));
    let ts = billing_now();
    let signature = machine_signature(&signing_key, "machine-1", ts);
    let (ttl_status, ttl_body) = post_lease(&server, &client, "machine-1", ts, &signature).await;
    assert_eq!(ttl_status, 200);
    let ttl_expiry = ttl_body["expiresAt"].as_u64().expect("expiry");
    let now = billing_now();
    assert!(ttl_expiry >= now + lease::LEASE_TTL_SECS - 5);
    assert!(ttl_expiry <= now + lease::LEASE_TTL_SECS + 5);
    assert!(ttl_expiry < ttl_grace_start + GRACE_SECS);
    assert_eq!(
        stored_state(&server).grace_started_at,
        Some(ttl_grace_start)
    );

    // Deadline five minutes out: the cap binds and lands exactly on the deadline.
    let capped_grace_start = billing_now() - GRACE_SECS + 300;
    seed_store(&server.data_dir, |store| {
        set_grace(store, capped_grace_start)
    });
    let ts = billing_now();
    let signature = machine_signature(&signing_key, "machine-1", ts);
    let (capped_status, capped_body) =
        post_lease(&server, &client, "machine-1", ts, &signature).await;
    assert_eq!(capped_status, 200);
    let capped_expiry = capped_body["expiresAt"].as_u64().expect("expiry");
    assert_eq!(
        capped_expiry,
        capped_grace_start + GRACE_SECS,
        "the lease ends exactly at the suspension deadline, never 24 hours later"
    );
    assert!(capped_expiry < billing_now() + lease::LEASE_TTL_SECS);
    assert_eq!(
        stored_state(&server).grace_started_at,
        Some(capped_grace_start),
        "a lease lookup keeps the original grace start"
    );
    let capped_lease = capped_body["lease"].as_str().expect("lease");
    let claims = lease::verify_lease(
        &server.state.account_public_key(),
        "machine-1",
        capped_lease,
    )
    .expect("the signed lease verifies");
    assert_eq!(claims.expires_at, capped_grace_start + GRACE_SECS);

    // The exact deadline instant is already stopped: no lease at all, and grace is not reset.
    let boundary_grace_start = billing_now() - GRACE_SECS;
    seed_store(&server.data_dir, |store| {
        set_grace(store, boundary_grace_start)
    });
    let ts = billing_now();
    let signature = machine_signature(&signing_key, "machine-1", ts);
    let (boundary_status, boundary_body) =
        post_lease(&server, &client, "machine-1", ts, &signature).await;
    assert_eq!(boundary_status, 402);
    assert_eq!(boundary_body["code"], "REMOTE_SUSPENDED");
    assert!(boundary_body["lease"].is_null());
    assert_eq!(
        boundary_body["details"]["graceEndsAt"].as_u64(),
        Some(boundary_grace_start + GRACE_SECS)
    );
    let boundary_state = stored_state(&server);
    assert_eq!(boundary_state.grace_started_at, Some(boundary_grace_start));
    assert!(
        boundary_state.stopped_at.is_some(),
        "the deadline instant is recorded as stopped"
    );
}

#[tokio::test]
async fn lease_signature_and_freshness_are_enforced() {
    let server = start_commercial_server().await;
    let client = reqwest::Client::new();
    let (signing_key, public_key) = machine_signing_key();
    seed_store(&server.data_dir, |store| {
        seed_user(store, "u1", "u1@test.local");
        seed_machine(store, "u1", "machine-1", &public_key);
    });

    let ts = billing_now();
    let valid = machine_signature(&signing_key, "machine-1", ts);
    let (unknown_status, unknown) =
        post_lease(&server, &client, "machine-unknown", ts, &valid).await;
    assert_eq!(unknown_status, 404);
    assert_eq!(unknown["code"], "NOT_FOUND");

    let (invalid_status, invalid) =
        post_lease(&server, &client, "machine-1", ts, "not-a-signature").await;
    assert_eq!(invalid_status, 403);
    assert_eq!(invalid["code"], "MACHINE_SIGNATURE_INVALID");

    let stale_ts = ts - LEASE_CLOCK_SKEW_SECS - 1;
    let stale_signature = machine_signature(&signing_key, "machine-1", stale_ts);
    let (stale_status, stale) =
        post_lease(&server, &client, "machine-1", stale_ts, &stale_signature).await;
    assert_eq!(stale_status, 403);
    assert_eq!(stale["code"], "MACHINE_SIGNATURE_STALE");

    let other_key = SigningKey::from_bytes(&[4u8; 32]);
    let foreign = machine_signature(&other_key, "machine-1", ts);
    let (foreign_status, foreign_payload) =
        post_lease(&server, &client, "machine-1", ts, &foreign).await;
    assert_eq!(foreign_status, 403);
    assert_eq!(foreign_payload["code"], "MACHINE_SIGNATURE_INVALID");

    let (healthy_status, healthy) = post_lease(&server, &client, "machine-1", ts, &valid).await;
    assert_eq!(healthy_status, 200);
    let expires_at = healthy["expiresAt"].as_u64().expect("expiry");
    let now = billing_now();
    assert!(expires_at >= now + lease::LEASE_TTL_SECS - 5);
    assert!(expires_at <= now + lease::LEASE_TTL_SECS + 5);
}

#[tokio::test]
async fn error_body_carries_details() {
    let server = start_commercial_server().await;
    let client = reqwest::Client::new();
    let (signing_key, public_key) = machine_signing_key();
    let grace_started_at = billing_now() - GRACE_SECS - 30;
    seed_store(&server.data_dir, |store| {
        seed_user(store, "u1", "u1@test.local");
        seed_machine(store, "u1", "machine-1", &public_key);
        seed_machine(store, "u1", "machine-2", "other-public-key");
        store.billing_states.insert(
            "u1".to_string(),
            BillingStateRecord {
                owner_key: "u1".to_string(),
                grace_started_at: Some(grace_started_at),
                stopped_at: None,
                last_notice: None,
            },
        );
    });

    let ts = billing_now();
    let signature = machine_signature(&signing_key, "machine-1", ts);
    let (status, payload) = post_lease(&server, &client, "machine-1", ts, &signature).await;
    assert_eq!(status, 402);
    let details = payload["details"].as_object().expect("structured details");
    for key in ["plan", "status", "graceEndsAt", "stoppedAt"] {
        assert!(details.contains_key(key), "details carries {key}");
    }

    let (unauthorized_status, unauthorized) = get_entitlement(&server, &client, None).await;
    assert_eq!(unauthorized_status, 401);
    assert_eq!(unauthorized["code"], "UNAUTHORIZED");
    assert!(
        unauthorized
            .as_object()
            .is_some_and(|body| !body.contains_key("details")),
        "an error without details omits the field entirely"
    );
}

#[tokio::test]
async fn selfhost_hides_billing_routes() {
    let selfhost_tmp = tempfile::tempdir().unwrap();
    let selfhost_state = Arc::new(
        AccountState::new(
            selfhost_tmp.path(),
            "https://relay.test",
            Arc::new(FileMailer::with_dir(selfhost_tmp.path().join("mail"))),
        )
        .with_deployment_mode(DeploymentMode::SelfHost),
    );
    let selfhost = serve_router(crate::account::service::router(selfhost_state.clone())).await;

    let commercial_tmp = tempfile::tempdir().unwrap();
    let commercial_state = Arc::new(
        AccountState::new(
            commercial_tmp.path(),
            "https://relay.test",
            Arc::new(FileMailer::with_dir(commercial_tmp.path().join("mail"))),
        )
        .with_deployment_mode(DeploymentMode::Commercial),
    );
    let commercial = serve_router(crate::account::service::router(commercial_state.clone())).await;

    let client = reqwest::Client::new();
    let health = client
        .get(format!("{selfhost}/api/account/v1/health"))
        .send()
        .await
        .expect("health request");
    assert_eq!(health.status().as_u16(), 200);

    let selfhost_gets = [
        "/api/account/v1/billing/entitlement",
        "/api/account/v1/org/members",
    ];
    for path in selfhost_gets {
        let response = client
            .get(format!("{selfhost}{path}"))
            .send()
            .await
            .expect("selfhost request");
        assert_eq!(
            response.status().as_u16(),
            404,
            "{path} stays hidden on selfhost"
        );
    }
    let selfhost_posts = [
        "/api/account/v1/billing/lease",
        "/api/account/v1/billing/webhook",
        "/api/account/v1/org/invite",
        "/api/account/v1/org/accept",
    ];
    for path in selfhost_posts {
        let response = client
            .post(format!("{selfhost}{path}"))
            .json(&json!({}))
            .send()
            .await
            .expect("selfhost request");
        assert_eq!(
            response.status().as_u16(),
            404,
            "{path} stays hidden on selfhost"
        );
    }

    let (commercial_status, commercial_body) = {
        let response = client
            .get(format!("{commercial}/api/account/v1/billing/entitlement"))
            .send()
            .await
            .expect("commercial request");
        let status = response.status().as_u16();
        (status, response_json(response).await)
    };
    assert_eq!(
        commercial_status, 401,
        "the same route is registered in commercial mode"
    );
    assert_eq!(commercial_body["code"], "UNAUTHORIZED");
}

async fn serve_router(router: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    format!("http://{addr}")
}

struct OrgFixture {
    owner_token: String,
    member_token: String,
    member_user_id: String,
    org_id: String,
}

fn seed_org_fixture(server: &TestServer) -> OrgFixture {
    let mut fixture = OrgFixture {
        owner_token: String::new(),
        member_token: String::new(),
        member_user_id: "u2".to_string(),
        org_id: "org-1".to_string(),
    };
    seed_store(&server.data_dir, |store| {
        fixture.owner_token = seed_user(store, "u1", "owner@test.local");
        fixture.member_token = seed_user(store, "u2", "member@test.local");
        seed_org(store, &fixture.org_id, "u1", &[("u1", ROLE_OWNER)]);
        seed_subscription(
            store,
            "610",
            "u1",
            Some(&fixture.org_id),
            "team_monthly",
            KIND_BASE,
            2,
            0,
            "active",
            5,
        );
    });
    fixture
}

async fn invite_member(
    server: &TestServer,
    client: &reqwest::Client,
    owner_token: &str,
    email: &str,
) -> String {
    let before = snapshot_mail_paths(&server.mail_dir);
    let response = post_json(
        client,
        format!("{}/api/account/v1/org/invite", server.url),
        owner_token,
        json!({ "email": email }),
    )
    .send()
    .await
    .expect("invite request");
    assert_eq!(response.status().as_u16(), 200);
    invite_token_from_new_mail(&server.mail_dir, &before)
}

#[tokio::test]
async fn org_invite_accept_adds_member_to_pool() {
    let server = start_commercial_server().await;
    let client = reqwest::Client::new();
    let fixture = seed_org_fixture(&server);

    let token = invite_member(&server, &client, &fixture.owner_token, "member@test.local").await;

    let accept = post_json(
        &client,
        format!("{}/api/account/v1/org/accept", server.url),
        &fixture.member_token,
        json!({ "token": token }),
    )
    .send()
    .await
    .expect("accept request");
    assert_eq!(accept.status().as_u16(), 200);
    let accepted = response_json(accept).await;
    assert_eq!(accepted["orgId"], fixture.org_id.as_str());
    assert_eq!(accepted["role"], ROLE_MEMBER);

    let members = client
        .get(format!("{}/api/account/v1/org/members", server.url))
        .bearer_auth(&fixture.owner_token)
        .send()
        .await
        .expect("members request");
    assert_eq!(members.status().as_u16(), 200);
    let members = response_json(members).await;
    assert_eq!(members["orgId"], fixture.org_id.as_str());
    assert_eq!(members["role"], ROLE_OWNER);
    let listed = members["members"].as_array().expect("members array");
    assert_eq!(listed.len(), 2);
    assert!(listed.iter().any(|member| {
        member["userId"] == fixture.member_user_id.as_str()
            && member["email"] == "member@test.local"
            && member["role"] == ROLE_MEMBER
    }));

    seed_store(&server.data_dir, |store| {
        seed_machine(store, "u2", "m-member", "member-key");
    });
    let (status, entitlement) =
        get_entitlement(&server, &client, Some(&fixture.member_token)).await;
    assert_eq!(status, 200);
    assert_eq!(entitlement["orgId"], fixture.org_id.as_str());
    assert_eq!(entitlement["machinesUsed"], 1);
    assert_eq!(entitlement["machineLimit"], 20);
}

#[tokio::test]
async fn org_invite_replay_is_rejected() {
    let server = start_commercial_server().await;
    let client = reqwest::Client::new();
    let fixture = seed_org_fixture(&server);
    let token = invite_member(&server, &client, &fixture.owner_token, "member@test.local").await;

    let first = post_json(
        &client,
        format!("{}/api/account/v1/org/accept", server.url),
        &fixture.member_token,
        json!({ "token": token }),
    )
    .send()
    .await
    .expect("accept request");
    assert_eq!(first.status().as_u16(), 200);

    let replay = post_json(
        &client,
        format!("{}/api/account/v1/org/accept", server.url),
        &fixture.member_token,
        json!({ "token": token }),
    )
    .send()
    .await
    .expect("replay request");
    assert_eq!(replay.status().as_u16(), 400);
    assert_eq!(response_json(replay).await["code"], "ORG_INVITE_INVALID");

    let store = read_store(&server.data_dir);
    assert_eq!(
        store.org_members.len(),
        2,
        "the replay adds no second membership"
    );
}

#[tokio::test]
async fn org_invite_email_mismatch_is_rejected() {
    let server = start_commercial_server().await;
    let client = reqwest::Client::new();
    let fixture = seed_org_fixture(&server);
    let mut outsider_token = String::new();
    seed_store(&server.data_dir, |store| {
        outsider_token = seed_user(store, "u3", "outsider@test.local");
    });
    let token = invite_member(&server, &client, &fixture.owner_token, "member@test.local").await;

    let mismatch = post_json(
        &client,
        format!("{}/api/account/v1/org/accept", server.url),
        &outsider_token,
        json!({ "token": token }),
    )
    .send()
    .await
    .expect("mismatched accept");
    assert_eq!(mismatch.status().as_u16(), 403);
    assert_eq!(
        response_json(mismatch).await["code"],
        "ORG_INVITE_EMAIL_MISMATCH"
    );

    let accepted = post_json(
        &client,
        format!("{}/api/account/v1/org/accept", server.url),
        &fixture.member_token,
        json!({ "token": token }),
    )
    .send()
    .await
    .expect("accepted after mismatch");
    assert_eq!(
        accepted.status().as_u16(),
        200,
        "a mismatched session must not consume the invite"
    );
}

#[tokio::test]
async fn org_owner_cannot_remove_self() {
    let server = start_commercial_server().await;
    let client = reqwest::Client::new();
    let fixture = seed_org_fixture(&server);

    let removed = client
        .post(format!(
            "{}/api/account/v1/org/members/u1/remove",
            server.url
        ))
        .bearer_auth(&fixture.owner_token)
        .send()
        .await
        .expect("self removal");
    assert_eq!(removed.status().as_u16(), 400);
    assert_eq!(
        response_json(removed).await["code"],
        "ORG_OWNER_CANNOT_REMOVE_SELF"
    );

    let store = read_store(&server.data_dir);
    assert!(store
        .org_members
        .contains_key(&member_key(&fixture.org_id, "u1")));
}

#[tokio::test]
async fn org_remove_returns_machines_to_individual_pool() {
    let server = start_commercial_server().await;
    let client = reqwest::Client::new();
    let fixture = seed_org_fixture(&server);
    seed_store(&server.data_dir, |store| {
        store.org_members.insert(
            member_key(&fixture.org_id, "u2"),
            OrgMemberRecord {
                org_id: fixture.org_id.clone(),
                user_id: "u2".to_string(),
                role: ROLE_MEMBER.to_string(),
                joined_at: 1,
            },
        );
        seed_machine(store, "u1", "m-owner", "owner-key");
        seed_machine(store, "u2", "m-member", "member-key");
    });

    let (pooled_status, pooled) =
        get_entitlement(&server, &client, Some(&fixture.owner_token)).await;
    assert_eq!(pooled_status, 200);
    assert_eq!(pooled["machinesUsed"], 2);

    let removed = client
        .post(format!(
            "{}/api/account/v1/org/members/u2/remove",
            server.url
        ))
        .bearer_auth(&fixture.owner_token)
        .send()
        .await
        .expect("removal request");
    assert_eq!(removed.status().as_u16(), 200);
    let removed_body = response_json(removed).await;
    assert_eq!(removed_body["removedUserId"], "u2");

    let store = read_store(&server.data_dir);
    assert!(
        !store
            .org_members
            .contains_key(&member_key(&fixture.org_id, "u2")),
        "the membership row is gone"
    );
    assert!(
        store
            .machines
            .values()
            .any(|machine| machine.machine_id == "m-member"),
        "the member machine record is preserved"
    );

    let (pooled_status, pooled) =
        get_entitlement(&server, &client, Some(&fixture.owner_token)).await;
    assert_eq!(pooled_status, 200);
    assert_eq!(
        pooled["machinesUsed"], 1,
        "the removed member's machines left the pool"
    );

    let (member_status, member) =
        get_entitlement(&server, &client, Some(&fixture.member_token)).await;
    assert_eq!(member_status, 200);
    assert!(member["orgId"].is_null());
    assert_eq!(member["plan"], "free");
    assert_eq!(member["machineLimit"], 1);
    assert_eq!(member["machinesUsed"], 1);
    assert_eq!(member["status"], "ok");
}

#[tokio::test]
async fn org_requires_owner_or_admin_role() {
    let server = start_commercial_server().await;
    let client = reqwest::Client::new();
    let fixture = seed_org_fixture(&server);
    let mut admin_token = String::new();
    seed_store(&server.data_dir, |store| {
        admin_token = seed_user(store, "u4", "admin@test.local");
        store.org_members.insert(
            member_key(&fixture.org_id, "u2"),
            OrgMemberRecord {
                org_id: fixture.org_id.clone(),
                user_id: "u2".to_string(),
                role: ROLE_MEMBER.to_string(),
                joined_at: 1,
            },
        );
        store.org_members.insert(
            member_key(&fixture.org_id, "u4"),
            OrgMemberRecord {
                org_id: fixture.org_id.clone(),
                user_id: "u4".to_string(),
                role: ROLE_ADMIN.to_string(),
                joined_at: 1,
            },
        );
    });

    let invite = post_json(
        &client,
        format!("{}/api/account/v1/org/invite", server.url),
        &fixture.member_token,
        json!({ "email": "someone@test.local" }),
    )
    .send()
    .await
    .expect("invite request");
    assert_eq!(invite.status().as_u16(), 403);
    assert_eq!(response_json(invite).await["code"], "ORG_ROLE_REQUIRED");

    let remove = client
        .post(format!(
            "{}/api/account/v1/org/members/u4/remove",
            server.url
        ))
        .bearer_auth(&fixture.member_token)
        .send()
        .await
        .expect("remove request");
    assert_eq!(remove.status().as_u16(), 403);
    assert_eq!(response_json(remove).await["code"], "ORG_ROLE_REQUIRED");

    let quantity = post_json(
        &client,
        format!("{}/api/account/v1/billing/quantity", server.url),
        &fixture.member_token,
        json!({ "seats": 3 }),
    )
    .send()
    .await
    .expect("quantity request");
    assert_eq!(quantity.status().as_u16(), 403);
    assert_eq!(response_json(quantity).await["code"], "ORG_ROLE_REQUIRED");

    let admin_invite = post_json(
        &client,
        format!("{}/api/account/v1/org/invite", server.url),
        &admin_token,
        json!({ "email": "invited@test.local" }),
    )
    .send()
    .await
    .expect("admin invite request");
    assert_eq!(
        admin_invite.status().as_u16(),
        200,
        "an admin may invite even though a plain member may not"
    );
}

#[tokio::test]
async fn login_flow_session_serves_billing_entitlement() {
    let tmp = tempfile::tempdir().unwrap();
    let mail_dir = tmp.path().join("mail");
    let state = Arc::new(
        AccountState::new(
            tmp.path(),
            "https://relay.test",
            Arc::new(FileMailer::with_dir(mail_dir.clone())),
        )
        .with_deployment_mode(DeploymentMode::Commercial),
    );
    let url = serve_router(crate::account::service::router(state.clone())).await;
    let client = reqwest::Client::new();

    let request = client
        .post(format!("{url}/api/account/v1/login/request"))
        .json(&json!({ "email": "fresh@test.local" }))
        .send()
        .await
        .expect("login request");
    assert_eq!(request.status().as_u16(), 202);

    let consume = client
        .post(format!("{url}/api/account/v1/login/consume"))
        .json(&json!({ "code": login_code_from_mail(&mail_dir) }))
        .send()
        .await
        .expect("login consume");
    assert_eq!(consume.status().as_u16(), 200);
    let session = response_json(consume).await;
    let token = session["token"]
        .as_str()
        .expect("session token")
        .to_string();
    assert_eq!(session["email"], "fresh@test.local");

    let entitlement_response = client
        .get(format!("{url}/api/account/v1/billing/entitlement"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("entitlement request");
    assert_eq!(entitlement_response.status().as_u16(), 200);
    let entitlement = response_json(entitlement_response).await;
    assert_eq!(entitlement["plan"], "free");
    assert_eq!(entitlement["machineLimit"].as_u64(), Some(1));
    assert_eq!(entitlement["machinesUsed"].as_u64(), Some(0));
    assert_eq!(entitlement["status"], "ok");
    assert!(entitlement["orgId"].is_null());
    assert!(entitlement["role"].is_null());

    let anonymous = client
        .get(format!("{url}/api/account/v1/billing/entitlement"))
        .send()
        .await
        .expect("anonymous entitlement request");
    assert_eq!(
        anonymous.status().as_u16(),
        401,
        "the billing surface has no anonymous path"
    );
}

#[tokio::test]
async fn team_cancelled_paid_through_base_aggregates_delinquent_pack() {
    let server = start_commercial_server().await;
    let client = reqwest::Client::new();
    let fixture = seed_org_fixture(&server);
    let now = billing_now();

    seed_store(&server.data_dir, |store| {
        // Base team plan is cancelled but ends in the future (paid-through/active)
        store.subscriptions.insert(
            "sub-team-base".to_string(),
            SubscriptionRecord {
                subscription_id: "sub-team-base".to_string(),
                owner_user_id: "u1".to_string(),
                org_id: Some(fixture.org_id.clone()),
                plan_key: "team_annual".to_string(),
                seats: 2,
                host_packs: 0,
                kind: KIND_BASE.to_string(),
                status: "cancelled".to_string(),
                ends_at: Some(now + 86400 * 30),
                ls_customer_id: Some("cust-1".to_string()),
                ls_updated_at: 10,
                manage_url: None,
            },
        );
        // Annual host pack is past_due (delinquent)
        store.subscriptions.insert(
            "sub-team-pack".to_string(),
            SubscriptionRecord {
                subscription_id: "sub-team-pack".to_string(),
                owner_user_id: "u1".to_string(),
                org_id: Some(fixture.org_id.clone()),
                plan_key: "team_annual".to_string(),
                seats: 0,
                host_packs: 2,
                kind: KIND_HOSTPACK.to_string(),
                status: "past_due".to_string(),
                ends_at: Some(now + 86400 * 30),
                ls_customer_id: Some("cust-1".to_string()),
                ls_updated_at: 20,
                manage_url: None,
            },
        );
    });

    let (status, ent) = get_entitlement(&server, &client, Some(&fixture.owner_token)).await;
    assert_eq!(status, 200);
    assert_eq!(ent["plan"], "team_annual");
    assert_eq!(
        ent["status"], "past_due",
        "delinquent pack turns cancelled-paid-through team pool into past_due"
    );
    assert!(!ent["graceEndsAt"].is_null());
}

#[tokio::test]
async fn org_accept_page_serves_html() {
    let server = start_commercial_server().await;
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/org/accept?token=sample-token-123", server.url))
        .send()
        .await
        .expect("org accept page request");
    assert_eq!(resp.status().as_u16(), 200);
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|val| val.to_str().ok())
        .unwrap_or_default();
    assert!(
        content_type.contains("text/html"),
        "expected text/html content-type header, got: {content_type}"
    );
    let text = resp.text().await.expect("page html text");
    // Machine-consumed storage and auth keys aligned with accountSession.ts contract
    assert!(text.contains("ferryx_remote_token_account"));
    assert!(text.contains("ferryx.account.tokenOrigin"));
    assert!(text.contains("ferryx:account-session"));
    assert!(!text.contains("ferryx.account.token'"));
}
#[tokio::test]
async fn quantity_reconciliation_rejects_demoted_owner_during_provider_io_and_skips_second_patch() {
    let (api_base, stub) = spawn_ls_stub(vec![
        subscription_fixture("710", TEAM_MONTHLY_VARIANT, 2, "active", T1),
        subscription_fixture("711", TEAM_HOSTPACK_VARIANT, 1, "active", T1),
    ])
    .await;
    let server =
        spawn_billing_server(DeploymentMode::Commercial, test_ls_config(&api_base), stub).await;
    let client = reqwest::Client::new();
    let mut owner_token = String::new();
    seed_store(&server.data_dir, |store| {
        owner_token = seed_user(store, "u1", "owner@test.local");
        seed_org(store, "org-1", "u1", &[("u1", ROLE_OWNER)]);
        seed_subscription(
            store,
            "710",
            "u1",
            Some("org-1"),
            "team_monthly",
            KIND_BASE,
            2,
            0,
            "active",
            5,
        );
        seed_subscription(
            store,
            "711",
            "u1",
            Some("org-1"),
            "team_annual",
            KIND_HOSTPACK,
            0,
            1,
            "active",
            5,
        );
    });
    {
        let mut data = server.stub.data.lock();
        let fixture = data
            .subscriptions
            .get_mut("711")
            .expect("host pack fixture");
        fixture["data"]["attributes"]["first_subscription_item"]["id"] = json!("901");
    }

    // Set up exact oneshot/Notify prearmed provider gate
    let (gate_tx, gate_rx) = oneshot::channel();
    let gate_notify = Arc::new(Notify::new());
    {
        let mut data = server.stub.data.lock();
        data.patch_gate_tx = Some(gate_tx);
        data.patch_gate_rx = Some(gate_notify.clone());
    }

    let server_url = server.url.clone();
    let owner_tok = owner_token.clone();
    let req_client = client.clone();
    let task = tokio::spawn(async move {
        post_json(
            &req_client,
            format!("{}/api/account/v1/billing/quantity", server_url),
            &owner_tok,
            json!({ "seats": 3, "hostPacks": 2 }),
        )
        .send()
        .await
        .expect("quantity request")
    });

    // Wait for the first PATCH to arrive at provider stub
    tokio::time::timeout(std::time::Duration::from_secs(5), gate_rx)
        .await
        .expect("timed out waiting for first patch provider gate")
        .expect("gate_rx receive");

    // Concurrently demote owner to ROLE_MEMBER while provider I/O is pending
    seed_store(&server.data_dir, |store| {
        store.org_members.insert(
            member_key("org-1", "u1"),
            OrgMemberRecord {
                org_id: "org-1".to_string(),
                user_id: "u1".to_string(),
                role: ROLE_MEMBER.to_string(),
                joined_at: 1,
            },
        );
    });

    // Release provider gate
    gate_notify.notify_one();

    let resp = tokio::time::timeout(std::time::Duration::from_secs(5), task)
        .await
        .expect("timed out waiting for quantity response")
        .expect("join handle");

    assert_eq!(resp.status().as_u16(), 403);
    let body = response_json(resp).await;
    assert_eq!(body["code"], "ORG_ROLE_REQUIRED");

    // Assert that exactly one patch occurred and the second PATCH was never attempted
    let attempts = server.stub.data.lock().item_patch_attempts.clone();
    assert_eq!(
        attempts,
        vec![(ITEM_ID.to_string(), 3)],
        "second axis PATCH must never be attempted after authorization demotion"
    );
    let patches = server.stub.data.lock().item_patches.clone();
    assert_eq!(
        patches,
        vec![(ITEM_ID.to_string(), 3)],
        "only the first patch was completed"
    );
}

#[tokio::test]
async fn quantity_partial_second_patch_failure_reconciles_first_axis() {
    let (api_base, stub) = spawn_ls_stub(vec![
        subscription_fixture("710", TEAM_MONTHLY_VARIANT, 2, "active", T1),
        subscription_fixture("711", TEAM_HOSTPACK_VARIANT, 1, "active", T1),
    ])
    .await;
    let server =
        spawn_billing_server(DeploymentMode::Commercial, test_ls_config(&api_base), stub).await;
    let client = reqwest::Client::new();
    let mut owner_token = String::new();
    seed_store(&server.data_dir, |store| {
        owner_token = seed_user(store, "u1", "owner@test.local");
        seed_org(store, "org-1", "u1", &[("u1", ROLE_OWNER)]);
        seed_subscription(
            store,
            "710",
            "u1",
            Some("org-1"),
            "team_monthly",
            KIND_BASE,
            2,
            0,
            "active",
            5,
        );
        seed_subscription(
            store,
            "711",
            "u1",
            Some("org-1"),
            "team_annual",
            KIND_HOSTPACK,
            0,
            1,
            "active",
            5,
        );
    });
    {
        let mut data = server.stub.data.lock();
        let fixture = data
            .subscriptions
            .get_mut("711")
            .expect("host pack fixture");
        fixture["data"]["attributes"]["first_subscription_item"]["id"] = json!("901");
        data.fail_second_patch = true;
    }

    let resp = post_json(
        &client,
        format!("{}/api/account/v1/billing/quantity", server.url),
        &owner_token,
        json!({ "seats": 4, "hostPacks": 3 }),
    )
    .send()
    .await
    .expect("quantity request");

    // Must surface typed provider error for the failed second axis
    assert_eq!(resp.status().as_u16(), 502);
    let body = response_json(resp).await;
    assert_eq!(body["code"], "BILLING_PROVIDER_ERROR");

    // Verify both patch attempts occurred with exact IDs and quantities
    let attempts = server.stub.data.lock().item_patch_attempts.clone();
    assert_eq!(
        attempts,
        vec![(ITEM_ID.to_string(), 4), ("901".to_string(), 3)],
        "both axes were attempted with exact IDs and quantities"
    );

    // Verify item_patches only recorded the first successful patch
    let patches = server.stub.data.lock().item_patches.clone();
    assert_eq!(
        patches,
        vec![(ITEM_ID.to_string(), 4)],
        "first patch succeeded before second failed"
    );

    // Verify partial reconciliation: first axis (seats) was reconciled locally despite second failure
    let store = read_store(&server.data_dir);
    assert_eq!(
        store.subscriptions["710"].seats, 4,
        "first axis acknowledged by provider must be reconciled"
    );
    assert_eq!(
        store.subscriptions["711"].host_packs, 1,
        "second axis failed at provider and must remain unchanged"
    );
}

#[tokio::test]
async fn duplicate_invoice_webhook_during_provider_outage_returns_duplicate_without_get() {
    let (api_base, stub) = spawn_ls_stub(vec![subscription_fixture(
        "700",
        PRO_MONTHLY_VARIANT,
        3,
        "active",
        T2,
    )])
    .await;
    let server =
        spawn_billing_server(DeploymentMode::Commercial, test_ls_config(&api_base), stub).await;
    let client = reqwest::Client::new();
    seed_store(&server.data_dir, |store| {
        seed_user(store, "u1", "u1@test.local");
        seed_subscription(
            store,
            "700",
            "u1",
            None,
            "pro_monthly",
            KIND_BASE,
            0,
            0,
            "active",
            5,
        );
    });

    let body = invoice_event_body("subscription_payment_success", "700", "8001");
    let (status, payload) = post_signed_webhook(&server, &client, &body).await;
    assert_eq!(status, 200);
    assert_eq!(payload["status"], "applied");

    let initial_gets = server.stub.data.lock().subscription_gets.len();
    assert_eq!(initial_gets, 1, "first invoice delivery fetched subscription details");

    // Simulate provider outage on GET endpoint
    server.stub.data.lock().fail_get = true;

    // Replay duplicate invoice webhook
    let (replay_status, replay_payload) = post_signed_webhook(&server, &client, &body).await;
    assert_eq!(replay_status, 200);
    assert_eq!(replay_payload["status"], "duplicate");

    // Assert that zero additional GET requests were made to the provider
    let subsequent_gets = server.stub.data.lock().subscription_gets.len();
    assert_eq!(
        subsequent_gets, initial_gets,
        "replay of duplicate invoice must short-circuit before provider GET"
    );
}

#[tokio::test]
async fn duplicate_invite_token_rejected_after_member_removal() {
    let server = start_commercial_server().await;
    let client = reqwest::Client::new();
    let fixture = seed_org_fixture(&server);

    // 1. Issue first invite for member@test.local
    let token1 = invite_member(&server, &client, &fixture.owner_token, "member@test.local").await;

    // 2. Issue second invite for same email in same org.
    // Production replacement semantics invalidate/replace earlier pending invites for the same org/email.
    let token2 = {
        let before = snapshot_mail_paths(&server.mail_dir);
        let resp = post_json(
            &client,
            format!("{}/api/account/v1/org/invite", server.url),
            &fixture.owner_token,
            json!({ "email": "member@test.local" }),
        )
        .send()
        .await
        .expect("second invite request");
        assert_eq!(resp.status().as_u16(), 200);
        invite_token_from_new_mail(&server.mail_dir, &before)
    };

    // The first token was replaced and must now be rejected
    let accept_replaced = post_json(
        &client,
        format!("{}/api/account/v1/org/accept", server.url),
        &fixture.member_token,
        json!({ "token": token1 }),
    )
    .send()
    .await
    .expect("accept replaced invite");
    assert_eq!(accept_replaced.status().as_u16(), 400);
    assert_eq!(
        response_json(accept_replaced).await["code"],
        "ORG_INVITE_INVALID"
    );

    // Accept second (active) invite -> succeeds
    let accept2 = post_json(
        &client,
        format!("{}/api/account/v1/org/accept", server.url),
        &fixture.member_token,
        json!({ "token": token2 }),
    )
    .send()
    .await
    .expect("accept second invite");
    assert_eq!(accept2.status().as_u16(), 200);

    // Verify member joined
    {
        let store = read_store(&server.data_dir);
        assert!(store
            .org_members
            .contains_key(&member_key(&fixture.org_id, &fixture.member_user_id)));
    }

    // Now seed a leftover/duplicate pending invite record directly (simulating a concurrent or older uncleaned invite)
    let leftover_token = "leftover-token-xyz".to_string();
    seed_store(&server.data_dir, |store| {
        store.org_invites.insert(
            token_hash(&leftover_token),
            OrgInviteRecord {
                token_hash: token_hash(&leftover_token),
                org_id: fixture.org_id.clone(),
                email: "member@test.local".to_string(),
                expires_at: now_secs() + 3600,
            },
        );
    });

    // Owner removes the member
    let remove = client
        .post(format!(
            "{}/api/account/v1/org/members/{}/remove",
            server.url, fixture.member_user_id
        ))
        .bearer_auth(&fixture.owner_token)
        .send()
        .await
        .expect("remove member request");
    assert_eq!(remove.status().as_u16(), 200);

    // The leftover invite token for that member's email must be revoked by org_remove_member
    let accept_leftover = post_json(
        &client,
        format!("{}/api/account/v1/org/accept", server.url),
        &fixture.member_token,
        json!({ "token": leftover_token }),
    )
    .send()
    .await
    .expect("accept leftover invite");
    assert_eq!(accept_leftover.status().as_u16(), 400);
    let body = response_json(accept_leftover).await;
    assert_eq!(body["code"], "ORG_INVITE_INVALID");

    // Store contains only owner, no members and no remaining invites for this user
    let store = read_store(&server.data_dir);
    assert_eq!(store.org_members.len(), 1);
    assert!(!store
        .org_members
        .contains_key(&member_key(&fixture.org_id, &fixture.member_user_id)));
    assert!(!store
        .org_invites
        .contains_key(&token_hash(&leftover_token)));
}

#[tokio::test]
async fn org_accept_preserves_existing_owner_or_admin_role_and_recomputes_grace() {
    let server = start_commercial_server().await;
    let client = reqwest::Client::new();
    let fixture = seed_org_fixture(&server);

    // 1. Issue invite for admin@test.local while user u4 is NOT a member yet.
    // HTTP POST /api/account/v1/org/invite succeeds 200.
    let token = invite_member(&server, &client, &fixture.owner_token, "admin@test.local").await;

    // 2. Before accepting, user u4 is created and promoted to ROLE_ADMIN in the org
    // (e.g. via prior administrative join or concurrent write), and machines are seeded
    // such that adding u4's machines causes the org pool to exceed its machineLimit (20).
    let mut admin_token = String::new();
    seed_store(&server.data_dir, |store| {
        admin_token = seed_user(store, "u4", "admin@test.local");
        store.org_members.insert(
            member_key(&fixture.org_id, "u4"),
            OrgMemberRecord {
                org_id: fixture.org_id.clone(),
                user_id: "u4".to_string(),
                role: ROLE_ADMIN.to_string(),
                joined_at: 1,
            },
        );
        // Seed 19 machines for owner u1, and 2 machines for admin u4 (total = 21 > 20 limit)
        for i in 0..19 {
            seed_machine(store, "u1", &format!("m-owner-{i}"), &format!("key-owner-{i}"));
        }
        seed_machine(store, "u4", "m-admin-0", "key-admin-0");
        seed_machine(store, "u4", "m-admin-1", "key-admin-1");
    });

    let before_request = billing_now();
    let accept = post_json(
        &client,
        format!("{}/api/account/v1/org/accept", server.url),
        &admin_token,
        json!({ "token": token }),
    )
    .send()
    .await
    .expect("accept request");
    let after_response = billing_now();
    assert_eq!(accept.status().as_u16(), 200);
    let accepted = response_json(accept).await;
    assert_eq!(accepted["orgId"], fixture.org_id.as_str());
    assert_eq!(
        accepted["role"], ROLE_ADMIN,
        "existing admin role must be preserved on accept"
    );

    let store = read_store(&server.data_dir);
    let member = store
        .org_members
        .get(&member_key(&fixture.org_id, "u4"))
        .expect("admin member record");
    assert_eq!(
        member.role, ROLE_ADMIN,
        "stored role must remain admin, not downgraded to member"
    );
    // Verify token was consumed
    assert!(
        !store.org_invites.contains_key(&token_hash(&token)),
        "invite token must be consumed"
    );

    // Verify pool grace state was recomputed and persisted with exact transition values
    let billing_state = store
        .billing_states
        .get(&format!("org:{}", fixture.org_id))
        .expect("org pool billing state must be persisted during accept");
    let grace_started = billing_state
        .grace_started_at
        .expect("grace_started_at must be populated when pool machinesUsed (21) exceeds limit (20)");
    assert!(
        grace_started >= before_request && grace_started <= after_response,
        "grace_started_at must fall inclusively between request start and response end: {before_request} <= {grace_started} <= {after_response}"
    );

    // Entitlement query verifies over_limit and active grace deadline
    let (status, ent) = get_entitlement(&server, &client, Some(&admin_token)).await;
    assert_eq!(status, 200);
    assert_eq!(ent["status"], "over_limit");
    assert_eq!(ent["machinesUsed"], 21);
    assert_eq!(ent["machineLimit"], 20);
    assert_eq!(ent["role"], ROLE_ADMIN);
    assert_eq!(ent["graceEndsAt"], grace_started + GRACE_SECS);
}

#[tokio::test]
async fn concurrent_valid_webhooks_serialize_as_applied_and_duplicate() {
    let server = start_commercial_server().await;
    let client = reqwest::Client::new();
    seed_store(&server.data_dir, |store| {
        seed_user(store, "u1", "u1@test.local");
    });

    let body = subscription_event_body(
        "subscription_created",
        "559",
        PRO_MONTHLY_VARIANT,
        1,
        "active",
        T1,
        json!({ "user_id": "u1" }),
    );

    let client1 = client.clone();
    let client2 = client.clone();
    let body1 = body.clone();
    let body2 = body.clone();
    let server_url1 = server.url.clone();
    let server_url2 = server.url.clone();
    let sig1 = sign_webhook(&body1);
    let sig2 = sign_webhook(&body2);

    let t1 = tokio::spawn(async move {
        client1
            .post(format!("{server_url1}/api/account/v1/billing/webhook"))
            .header(WEBHOOK_SIGNATURE_HEADER, sig1)
            .body(body1)
            .send()
            .await
            .expect("webhook send 1")
    });

    let t2 = tokio::spawn(async move {
        client2
            .post(format!("{server_url2}/api/account/v1/billing/webhook"))
            .header(WEBHOOK_SIGNATURE_HEADER, sig2)
            .body(body2)
            .send()
            .await
            .expect("webhook send 2")
    });

    let (r1, r2) = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        async { tokio::try_join!(t1, t2) },
    )
    .await
    .expect("timed out waiting for concurrent webhooks")
    .expect("both tasks join");
    assert_eq!(r1.status().as_u16(), 200);
    assert_eq!(r2.status().as_u16(), 200);

    let p1 = response_json(r1).await;
    let p2 = response_json(r2).await;

    let statuses = vec![p1["status"].as_str().unwrap(), p2["status"].as_str().unwrap()];
    assert!(
        statuses.contains(&"applied") && statuses.contains(&"duplicate"),
        "concurrent valid webhook deliveries must result in exactly one applied and one duplicate: {:?}",
        statuses
    );

    let store = read_store(&server.data_dir);
    assert_eq!(store.subscriptions.len(), 1);
    assert_eq!(store.subscriptions["559"].plan_key, "pro_monthly");
    assert_eq!(store.payment_events.len(), 1);
    let event = store.payment_events.values().next().unwrap();
    assert_eq!(event.outcome, "applied");
}

#[tokio::test]
async fn team_member_entitlement_redacts_customer_portal_manage_url() {
    let server = start_commercial_server().await;
    let client = reqwest::Client::new();
    let fixture = seed_org_fixture(&server);
    let now = billing_now();

    // Deterministically seed matching membership for u2 as ROLE_MEMBER and the team subscription on fixture.org_id
    seed_store(&server.data_dir, |store| {
        store.org_members.insert(
            member_key(&fixture.org_id, &fixture.member_user_id),
            OrgMemberRecord {
                org_id: fixture.org_id.clone(),
                user_id: fixture.member_user_id.clone(),
                role: ROLE_MEMBER.to_string(),
                joined_at: 1,
            },
        );
        store.subscriptions.insert(
            "sub-team".to_string(),
            SubscriptionRecord {
                subscription_id: "sub-team".to_string(),
                owner_user_id: "u1".to_string(),
                org_id: Some(fixture.org_id.clone()),
                plan_key: "team_monthly".to_string(),
                seats: 2,
                host_packs: 0,
                kind: KIND_BASE.to_string(),
                status: "active".to_string(),
                ends_at: Some(now + 86400 * 30),
                ls_customer_id: Some("cust-1".to_string()),
                ls_updated_at: 10,
                manage_url: Some("https://billing.provider.test/portal/secret-bearer-url".to_string()),
            },
        );
    });

    // Owner sees manageUrl
    let (owner_status, owner_ent) = get_entitlement(&server, &client, Some(&fixture.owner_token)).await;
    assert_eq!(owner_status, 200);
    assert_eq!(owner_ent["role"], ROLE_OWNER);
    assert_eq!(
        owner_ent["manageUrl"],
        "https://billing.provider.test/portal/secret-bearer-url",
        "owner must see the customer portal manageUrl"
    );

    // Plain team member must have manageUrl redacted
    let (member_status, member_ent) = get_entitlement(&server, &client, Some(&fixture.member_token)).await;
    assert_eq!(member_status, 200);
    assert_eq!(member_ent["role"], ROLE_MEMBER);
    assert_eq!(member_ent["orgId"], fixture.org_id.as_str());
    assert!(
        member_ent["manageUrl"].is_null(),
        "plain team member entitlement response must not disclose customer portal bearer URL"
    );
}

#[tokio::test]
async fn debug_clock_advance_still_valid_real_ts_returns_payment_required_stopped() {
    let server = start_commercial_server().await;
    let (signing_key, public_key) = machine_signing_key();

    let real_now = crate::account::store::now_secs();
    seed_store(&server.data_dir, |store| {
        seed_user(store, "u1", "u1@test.local");
        seed_machine(store, "u1", "machine-1", &public_key);
        seed_machine(store, "u1", "machine-2", "other-public-key");
        store.billing_states.insert(
            "u1".to_string(),
            BillingStateRecord {
                owner_key: "u1".to_string(),
                grace_started_at: Some(real_now),
                stopped_at: None,
                last_notice: None,
            },
        );
    });

    let ts = real_now;
    let signature = machine_signature(&signing_key, "machine-1", ts);
    let request = LeaseBody {
        machine_id: "machine-1".to_string(),
        ts,
        sig: signature,
    };

    // 1. Positive case: real auth_now accepts signature within ±60s window.
    // Billing evaluation uses advanced billing_now (+8 days > GRACE_SECS 7 days).
    // Must return 402 REMOTE_SUSPENDED with details.status == "stopped", NOT 403 MACHINE_SIGNATURE_STALE.
    let advanced_billing_now = real_now + 8 * 86400;
    let result = billing_lease_core(&server.state, &request, real_now, advanced_billing_now).await;
    let err = result.expect_err("stopped account must fail lease");
    assert_eq!(err.status, axum::http::StatusCode::PAYMENT_REQUIRED);
    assert_eq!(err.code, "REMOTE_SUSPENDED");
    let details = err.details.as_ref().expect("details object");
    assert_eq!(details["status"], "stopped");

    // 2. Negative case: ts outside ±60s of auth_now still fails with 403 MACHINE_SIGNATURE_STALE
    // even when billing clock is advanced.
    let stale_ts = real_now - LEASE_CLOCK_SKEW_SECS - 5;
    let stale_signature = machine_signature(&signing_key, "machine-1", stale_ts);
    let stale_request = LeaseBody {
        machine_id: "machine-1".to_string(),
        ts: stale_ts,
        sig: stale_signature,
    };
    let stale_result = billing_lease_core(&server.state, &stale_request, real_now, advanced_billing_now).await;
    let stale_err = stale_result.expect_err("stale ts must be rejected");
    assert_eq!(stale_err.status, axum::http::StatusCode::FORBIDDEN);
    assert_eq!(stale_err.code, "MACHINE_SIGNATURE_STALE");
}
