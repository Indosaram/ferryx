//! Router-level tests for `POST /api/account/v1/account/delete`.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use crate::account::peer_record::test_support::http;
use crate::account::service::{router, AccountState};
use crate::account::store::{
    now_secs, random_token, token_hash, GrantRecord, LoginCodeRecord, MachineRecord,
    SessionRecord, SubscriptionRecord, UserRecord,
};

const LIMIT: Duration = Duration::from_secs(10);
const DELETE: &str = "/api/account/v1/account/delete";
const EMAIL: &str = "owner@example.com";
const USER: &str = "user-owner";

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

async fn spawn() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let mailer = Arc::new(crate::account::mailer::FileMailer::with_dir(tmp.path().join("mail")));
    let state = Arc::new(AccountState::new(tmp.path(), "https://relay.test", mailer));
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

/// Seeds `USER` with two live sessions, one machine and one grant on it; returns both tokens.
fn seed_owner(fx: &Fixture) -> (String, String) {
    let (first, second) = (random_token(), random_token());
    let now = now_secs();
    fx.state
        .mutate(|store| {
            store.users.insert(
                USER.into(),
                UserRecord {
                    user_id: USER.into(),
                    email: EMAIL.into(),
                    created_at: 1,
                },
            );
            for token in [&first, &second] {
                store.sessions.insert(
                    token_hash(token),
                    SessionRecord {
                        user_id: USER.into(),
                        expires_at: now + 3600,
                    },
                );
            }
            store.machines.insert(
                "rec-m1".into(),
                MachineRecord {
                    machine_record_id: "rec-m1".into(),
                    owner_user_id: USER.into(),
                    machine_id: "m1".into(),
                    display_name: "m1".into(),
                    public_key: "pk".into(),
                    attach_public_key: "apk".into(),
                    relay_origin: "https://relay.test".into(),
                    platform: "linux".into(),
                    enrollment_epoch: 1,
                    enrolled_at: 1,
                    last_seen_at: 0,
                },
            );
            store.grants.insert(
                "g1".into(),
                GrantRecord {
                    grant_id: "g1".into(),
                    machine_record_id: "rec-m1".into(),
                    owner_user_id: USER.into(),
                    pairing_token_hash: "h".into(),
                    grant_scope: "full".into(),
                    device_attach_public_key: "d".into(),
                    installation_id: "i".into(),
                    issued_at: now,
                    expires_at: now + 600,
                },
            );
            Ok(())
        })
        .unwrap();
    (first, second)
}

fn seed_subscription(fx: &Fixture, status: &str, ends_at: Option<u64>) {
    let status = status.to_string();
    fx.state
        .mutate(|store| {
            store.subscriptions.insert(
                "sub-1".into(),
                SubscriptionRecord {
                    subscription_id: "sub-1".into(),
                    owner_user_id: USER.into(),
                    org_id: None,
                    plan_key: "pro_monthly".into(),
                    seats: 1,
                    host_packs: 0,
                    kind: "base".into(),
                    status,
                    ends_at,
                    ls_customer_id: None,
                    ls_updated_at: 1,
                    manage_url: None,
                },
            );
            Ok(())
        })
        .unwrap();
}

async fn send(fx: &Fixture, method: reqwest::Method, path: &str, token: Option<&str>, body: Option<Value>) -> (u16, Value) {
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

async fn delete(fx: &Fixture, token: Option<&str>, confirm: &str) -> (u16, Value) {
    send(fx, reqwest::Method::POST, DELETE, token, Some(json!({ "confirmEmail": confirm }))).await
}

async fn list_machines(fx: &Fixture, token: &str) -> u16 {
    send(fx, reqwest::Method::GET, "/api/account/v1/machines", Some(token), None).await.0
}

#[tokio::test]
async fn delete_account_succeeds_and_revokes_every_session() {
    let fx = spawn().await;
    let (first, second) = seed_owner(&fx);

    let (status, body) = delete(&fx, Some(&first), "  Owner@Example.COM ").await;

    assert_eq!((status, body), (200, json!({ "deleted": true })));
    assert_eq!(list_machines(&fx, &first).await, 401);
    assert_eq!(list_machines(&fx, &second).await, 401);
}

#[tokio::test]
async fn delete_account_replay_after_deletion_is_401() {
    let fx = spawn().await;
    let (token, _) = seed_owner(&fx);
    assert_eq!(delete(&fx, Some(&token), EMAIL).await.0, 200);

    let (status, body) = delete(&fx, Some(&token), EMAIL).await;

    assert_eq!(status, 401);
    assert_eq!(body["code"], "UNAUTHORIZED");
}

#[tokio::test]
async fn delete_account_removes_machines_grants_and_user_row() {
    let fx = spawn().await;
    let (token, _) = seed_owner(&fx);

    assert_eq!(delete(&fx, Some(&token), EMAIL).await.0, 200);

    let store = fx.state.load().unwrap();
    assert!(store.users.is_empty());
    assert!(store.sessions.is_empty());
    assert!(store.machines.is_empty());
    assert!(store.grants.is_empty());
}

#[tokio::test]
async fn delete_account_lets_the_email_sign_up_as_a_new_account() {
    let fx = spawn().await;
    let (token, _) = seed_owner(&fx);
    assert_eq!(delete(&fx, Some(&token), EMAIL).await.0, 200);
    let code = random_token();
    let code_hash = token_hash(&code);
    fx.state
        .mutate(|store| {
            store.login_codes.insert(
                code_hash,
                LoginCodeRecord {
                    email: EMAIL.into(),
                    expires_at: now_secs() + 600,
                    consumed_at: None,
                    login_handle_hash: None,
                },
            );
            Ok(())
        })
        .unwrap();

    let (status, body) = send(
        &fx,
        reqwest::Method::POST,
        "/api/account/v1/login/consume",
        None,
        Some(json!({ "code": code })),
    )
    .await;

    assert_eq!(status, 200, "{body}");
    assert_eq!(body["email"], EMAIL);
    assert_ne!(body["accountId"], USER);
    assert_eq!(list_machines(&fx, body["token"].as_str().unwrap()).await, 200);
}

#[tokio::test]
async fn delete_account_with_mismatched_email_is_400_and_keeps_the_account() {
    let fx = spawn().await;
    let (token, _) = seed_owner(&fx);

    let (status, body) = delete(&fx, Some(&token), "someone@example.com").await;

    assert_eq!(status, 400);
    assert_eq!(body["code"], "CONFIRMATION_MISMATCH");
    assert_eq!(list_machines(&fx, &token).await, 200);
}

#[tokio::test]
async fn delete_account_without_or_with_unknown_token_is_401() {
    let fx = spawn().await;
    seed_owner(&fx);

    assert_eq!(delete(&fx, None, EMAIL).await.0, 401);
    assert_eq!(delete(&fx, Some("not-a-session"), EMAIL).await.0, 401);
    assert_eq!(fx.state.load().unwrap().users.len(), 1);
}

#[tokio::test]
async fn delete_account_with_active_subscription_is_409_and_keeps_the_account() {
    let fx = spawn().await;
    let (token, _) = seed_owner(&fx);
    seed_subscription(&fx, "active", None);

    let (status, body) = delete(&fx, Some(&token), EMAIL).await;

    assert_eq!(status, 409);
    assert_eq!(body["code"], "SUBSCRIPTION_ACTIVE");
    assert_eq!(list_machines(&fx, &token).await, 200);
}

#[tokio::test]
async fn delete_account_with_expired_subscription_succeeds_and_drops_it() {
    let fx = spawn().await;
    let (token, _) = seed_owner(&fx);
    seed_subscription(&fx, "expired", Some(1));

    assert_eq!(delete(&fx, Some(&token), EMAIL).await.0, 200);

    assert!(fx.state.load().unwrap().subscriptions.is_empty());
}
