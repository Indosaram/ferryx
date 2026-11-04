//! Wizard sync router coverage; all requests have a bounded completion deadline.
use std::sync::Arc;
use std::time::Duration;
use serde_json::{json, Value};
use super::service::{router, AccountState};
use super::store::{now_secs, token_hash, SessionRecord, UserRecord};

struct Fixture {
    url: String,
    state: Arc<AccountState>,
    server: tokio::task::JoinHandle<()>,
    _dir: tempfile::TempDir,
}

impl Drop for Fixture {
    fn drop(&mut self) { self.server.abort(); }
}

async fn spawn() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let mailer = Arc::new(super::mailer::FileMailer::with_dir(dir.path().join("mail")));
    let state = Arc::new(AccountState::new(dir.path(), "http://localhost", mailer));
    state.mutate(|store| {
        for id in ["alice", "bob"] {
            store.users.insert(id.into(), UserRecord {
                user_id: id.into(), email: format!("{id}@test.local"), created_at: 1,
            });
            store.sessions.insert(token_hash(id), SessionRecord {
                user_id: id.into(), expires_at: now_secs() + 3600,
            });
        }
        Ok(())
    }).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let app = router(Arc::clone(&state));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
    Fixture { url, state, server, _dir: dir }
}

async fn send(fx: &Fixture, method: &str, path: &str, token: Option<&str>, body: Option<Value>) -> (u16, Value) {
    let mut req = super::peer_record::test_support::http()
        .request(method.parse().unwrap(), format!("{}{path}", fx.url));
    if let Some(token) = token { req = req.bearer_auth(token); }
    if let Some(body) = body { req = req.json(&body); }
    tokio::time::timeout(Duration::from_secs(10), async move {
        let res = req.send().await.unwrap();
        (res.status().as_u16(), res.json().await.unwrap())
    }).await.expect("bounded HTTP response")
}

const PATH: &str = "/api/v1/account/onboarding";

#[tokio::test]
async fn auth_required_and_machine_steps_rejected() {
    let fx = spawn().await;
    for method in ["GET", "PUT"] {
        assert_eq!(send(&fx, method, PATH, None, None).await.0, 401);
        assert_eq!(send(&fx, method, PATH, Some("invalid"), None).await.0, 401);
    }
    for step in ["enroll", "cli", "skills", "project", "agents", "import"] {
        let (status, error) = send(&fx, "PUT", PATH, Some("alice"),
            Some(json!({"completedSteps":[step],"dismissed":false}))).await;
        assert_eq!(status, 400);
        assert_eq!(error["code"], "MACHINE_STEP_NOT_SYNCABLE");
    }
    assert_eq!(send(&fx, "PUT", PATH, Some("alice"),
        Some(json!({"completedSteps":["/secret/path"],"dismissed":false}))).await.0, 400);
}

#[tokio::test]
async fn round_trip_union_or_newest_ordering_and_account_isolation() {
    let fx = spawn().await;
    let put = |steps: Value, dismissed, updated| json!({"completedSteps":steps,"dismissed":dismissed,"updatedAt":updated});
    assert_eq!(send(&fx, "PUT", PATH, Some("alice"), Some(put(json!(["intro", "intro"]), true, 10))).await.0, 200);
    let (status, merged) = send(&fx, "PUT", PATH, Some("alice"), Some(put(json!(["features"]), false, 20))).await;
    assert_eq!(status, 200);
    assert_eq!(merged, put(json!(["features", "intro"]), true, 20));
    let (_, stale) = send(&fx, "PUT", PATH, Some("alice"), Some(put(json!(["phone"]), false, 5))).await;
    assert_eq!(stale, put(json!(["features", "intro", "phone"]), true, 20));
    assert_eq!(send(&fx, "GET", PATH, Some("alice"), None).await, (200, stale.clone()));
    assert_eq!(send(&fx, "GET", "/api/account/v1/onboarding", Some("alice"), None).await.1, stale);
    assert_eq!(send(&fx, "GET", PATH, Some("bob"), None).await.1["completedSteps"], json!([]));
    // Reload from SQLite, not a process-local cache (restart/handover durability).
    assert_eq!(fx.state.load().unwrap().onboarding["alice"].completed_steps, ["features", "intro", "phone"]);
}

#[tokio::test]
async fn deletion_removes_progress_and_put_budget_is_per_user() {
    let fx = spawn().await;
    let body = json!({"completedSteps":["intro"],"dismissed":false});
    for _ in 0..60 {
        assert_eq!(send(&fx, "PUT", PATH, Some("alice"), Some(body.clone())).await.0, 200);
    }
    assert_eq!(send(&fx, "PUT", PATH, Some("alice"), Some(body.clone())).await.0, 429);
    assert_eq!(send(&fx, "PUT", PATH, Some("bob"), Some(body)).await.0, 200);
    assert_eq!(send(&fx, "POST", "/api/account/v1/account/delete", Some("alice"),
        Some(json!({"confirmEmail":"alice@test.local"}))).await.0, 200);
    let store = fx.state.load().unwrap();
    assert!(!store.onboarding.contains_key("alice"));
    assert!(store.onboarding.contains_key("bob"));
}
