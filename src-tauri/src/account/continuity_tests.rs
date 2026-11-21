use super::*;
use crate::account::service::router;
use crate::account::store::{random_token, token_hash, SessionRecord, UserRecord};
use serde_json::{json, Value};
use std::time::Duration;

struct Fixture {
    dir: tempfile::TempDir,
    state: Arc<AccountState>,
    url: String,
    server: tokio::task::JoinHandle<()>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

impl Fixture {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mailer = Arc::new(crate::account::mailer::FileMailer::with_dir(dir.path().join("mail")));
        let state = Arc::new(AccountState::new(dir.path(), "https://relay.test", mailer));
        let app = router(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self { dir, state, url, server }
    }

    fn user(&self, id: &str) -> String {
        let token = random_token();
        self.state.mutate(|store| {
            store.users.insert(id.into(), UserRecord {
                user_id: id.into(), email: format!("{id}@test.local"), created_at: 1,
            });
            store.sessions.insert(token_hash(&token), SessionRecord {
                user_id: id.into(), expires_at: now_secs() + 3600,
            });
            Ok(())
        }).unwrap();
        token
    }

    async fn request(&self, token: Option<&str>, body: Option<Value>) -> (u16, Value) {
        tokio::time::timeout(Duration::from_secs(10), async {
            let client = reqwest::Client::builder().no_proxy().build().unwrap();
            let url = format!("{}/api/account/v1/continuity", self.url);
            let mut request = if let Some(body) = body {
                client.put(url).json(&body)
            } else {
                client.get(url)
            };
            if let Some(token) = token {
                request = request.bearer_auth(token);
            }
            let response = request.send().await.unwrap();
            let status = response.status().as_u16();
            // The request size limit answers before the JSON handler. Successful reads,
            // including an empty pointer, must still return valid JSON.
            let value = if status == 413 { Value::Null } else { response.json().await.unwrap() };
            (status, value)
        }).await.expect("bounded HTTP request")
    }
}

fn selection(revision: i64, session: &str) -> Value {
    json!({ "expectedRevision": revision, "machineId": "machine-a", "workspaceId": "workspace-a",
        "worktreeSlug": "main", "sessionId": session })
}

fn clear(revision: i64) -> Value {
    json!({ "expectedRevision": revision, "machineId": null, "workspaceId": null,
        "worktreeSlug": null, "sessionId": null })
}

#[tokio::test]
async fn cas_conflict_tombstone_and_account_isolation_over_http() {
    let fx = Fixture::new().await;
    let a = fx.user("account-a");
    let b = fx.user("account-b");
    assert_eq!(fx.request(None, None).await.0, 401);
    assert_eq!(fx.request(None, Some(selection(0, "s"))).await.0, 401);
    assert_eq!(fx.request(Some("invalid"), None).await.0, 401);
    assert_eq!(fx.request(Some(&a), None).await, (200, Value::Null));
    let (status, first) = fx.request(Some(&a), Some(selection(0, "s1"))).await;
    assert_eq!(status, 200, "{first}");
    assert_eq!(first["revision"], 1);
    assert_eq!(first["sessionId"], "s1");
    assert!(first["updatedAt"].as_u64().is_some());
    let (status, conflict) = fx.request(Some(&a), Some(selection(0, "stale"))).await;
    assert_eq!(status, 409, "{conflict}");
    assert_eq!(conflict["code"], "CONTINUITY_CONFLICT");
    assert_eq!(conflict["details"]["current"], first);
    assert_eq!(fx.request(Some(&a), None).await, (200, first.clone()));
    assert_eq!(fx.request(Some(&b), None).await, (200, Value::Null));
    let (status, other) = fx.request(Some(&b), Some(selection(0, "other"))).await;
    assert_eq!(status, 200, "{other}");
    assert_eq!(other["revision"], 1);
    assert_eq!(fx.request(Some(&a), None).await.1, first);
    let (status, tombstone) = fx.request(Some(&a), Some(clear(1))).await;
    assert_eq!(status, 200, "{tombstone}");
    assert_eq!(tombstone["revision"], 2);
    for field in ["machineId", "workspaceId", "worktreeSlug", "sessionId"] {
        assert_eq!(tombstone[field], Value::Null);
    }
    assert_eq!(fx.request(Some(&a), None).await.1, tombstone);
    for stale in [0, 1] {
        let (status, conflict) = fx.request(Some(&a), Some(selection(stale, "stale"))).await;
        assert_eq!(status, 409);
        assert_eq!(conflict["details"]["current"], tombstone);
    }
    assert_eq!(fx.request(Some(&a), Some(selection(2, "s2"))).await.1["revision"], 3);
    assert_eq!(fx.request(Some(&b), None).await.1, other);
}

#[tokio::test]
async fn root_workspace_selection_round_trips_and_clears_with_monotonic_revision() {
    let fx = Fixture::new().await;
    let token = fx.user("root-workspace");
    let mut body = selection(0, "root-session");
    body["worktreeSlug"] = Value::Null;
    let (status, pointer) = fx.request(Some(&token), Some(body)).await;
    assert_eq!(status, 200, "{pointer}");
    assert_eq!(pointer["machineId"], "machine-a");
    assert_eq!(pointer["workspaceId"], "workspace-a");
    assert_eq!(pointer["sessionId"], "root-session");
    assert_eq!(pointer["worktreeSlug"], Value::Null);
    assert_eq!(pointer["revision"], 1);
    assert_eq!(fx.request(Some(&token), None).await, (200, pointer));
    let (status, tombstone) = fx.request(Some(&token), Some(clear(1))).await;
    assert_eq!(status, 200, "{tombstone}");
    assert_eq!(tombstone["revision"], 2);
    assert_eq!(tombstone["machineId"], Value::Null);
}

#[tokio::test]
async fn clear_before_first_publish_still_fences_revision_zero() {
    let fx = Fixture::new().await;
    let a = fx.user("a");
    let (status, pointer) = fx.request(Some(&a), Some(clear(0))).await;
    assert_eq!(status, 200);
    assert_eq!(pointer["revision"], 1);
    assert_eq!(fx.request(Some(&a), Some(selection(0, "stale"))).await.0, 409);
}

#[tokio::test]
async fn invalid_targets_and_revision_do_not_mutate() {
    let fx = Fixture::new().await;
    let token = fx.user("a");
    let mut partial = selection(0, "s");
    partial["machineId"] = Value::Null;
    let mut empty = selection(0, "s");
    empty["workspaceId"] = json!(" ");
    let mut unknown = selection(0, "s");
    unknown["userId"] = json!("other");
    let mut empty_slug = selection(0, "s");
    empty_slug["worktreeSlug"] = json!(" ");
    let mut oversized_slug = selection(0, "s");
    oversized_slug["worktreeSlug"] = json!("x".repeat(4097));
    let invalid = serde_json::from_value::<PutContinuity>(oversized_slug.clone()).unwrap();
    assert_eq!(invalid.validate().unwrap_err().code, "INVALID_CONTINUITY");
    // Four fields may each be 4096 bytes, so the route's own limit lets this reach the handler:
    // the field rule, not the router-wide 4 KiB default, decides validity.
    let (status, refusal) = fx.request(Some(&token), Some(oversized_slug)).await;
    assert_eq!(status, 400, "{refusal}");
    assert_eq!(refusal["code"], "INVALID_CONTINUITY", "{refusal}");
    // A field as large as the whole cap cannot fit in a body capped there, so the route's own
    // limit - not the field rule - is still what answers past it.
    let mut over_limit = selection(0, "s");
    over_limit["worktreeSlug"] = json!("x".repeat(MAX_BODY_BYTES));
    assert_eq!(fx.request(Some(&token), Some(over_limit)).await.0, 413);
    assert_eq!(fx.request(Some(&token), None).await, (200, Value::Null));
    for body in [partial, empty, empty_slug, selection(-1, "s"), json!({}), unknown] {
        assert_eq!(fx.request(Some(&token), Some(body)).await.0, 400);
        assert_eq!(fx.request(Some(&token), None).await, (200, Value::Null));
    }
}

#[tokio::test]
async fn simultaneous_publish_has_exactly_one_winner() {
    let fx = Fixture::new().await;
    let token = fx.user("a");
    let (a, b) = tokio::join!(
        fx.request(Some(&token), Some(selection(0, "a"))),
        fx.request(Some(&token), Some(selection(0, "b"))),
    );
    let (winner, loser) = if a.0 == 200 { (a, b) } else { (b, a) };
    assert_eq!(winner.0, 200);
    assert_eq!(loser.0, 409);
    assert_eq!(loser.1["details"]["current"], winner.1);
    assert_eq!(fx.request(Some(&token), None).await.1, winner.1);
}

#[tokio::test]
async fn standby_without_table_reads_null_and_refuses_writes_then_promotes() {
    let fx = Fixture::new().await;
    let token = fx.user("a");
    {
        let conn = store_sqlite::open_sqlite_connection(fx.dir.path()).unwrap();
        conn.execute_batch("DROP TABLE continuity_pointers; PRAGMA user_version = 3;").unwrap();
    }
    let marker = fx.dir.path().join(store_sqlite::STANDBY_MARKER_FILENAME);
    std::fs::write(&marker, b"").unwrap();
    assert_eq!(fx.request(Some(&token), None).await, (200, Value::Null));
    let (status, body) = fx.request(Some(&token), Some(selection(0, "s"))).await;
    assert_eq!(status, 503);
    assert_eq!(body["code"], "ACCOUNT_STORE_READ_ONLY");
    {
        let conn = store_sqlite::open_sqlite_connection(fx.dir.path()).unwrap();
        assert!(!store_sqlite::codec::table_exists(&conn, "continuity_pointers").unwrap());
        assert_eq!(conn.query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0)).unwrap(), 3);
    }
    std::fs::remove_file(&marker).unwrap();
    assert_eq!(fx.request(Some(&token), Some(selection(0, "s"))).await.0, 200);
    std::fs::write(&marker, b"").unwrap();
    assert_eq!(fx.request(Some(&token), None).await.1["revision"], 1);
}

#[tokio::test]
async fn whole_store_save_and_logout_preserve_pointer_but_account_deletion_cascades() {
    let fx = Fixture::new().await;
    let token = fx.user("a");
    let (_, pointer) = fx.request(Some(&token), Some(selection(0, "s"))).await;
    fx.state.mutate(|store| {
        store.users.get_mut("a").unwrap().email = "changed@test.local".into();
        Ok(())
    }).unwrap();
    assert_eq!(fx.request(Some(&token), None).await.1, pointer);
    let logout = tokio::time::timeout(Duration::from_secs(10), async {
        reqwest::Client::builder().no_proxy().build().unwrap()
            .post(format!("{}/api/account/v1/logout", fx.url))
            .bearer_auth(&token).send().await.unwrap()
    }).await.expect("bounded logout request");
    assert_eq!(logout.status().as_u16(), 204);
    let conn = store_sqlite::open_sqlite_connection(fx.dir.path()).unwrap();
    assert_eq!(current(&conn, "a").unwrap().unwrap().revision, 1);
    drop(conn);
    assert_eq!(fx.request(Some(&token), None).await.0, 401);
    fx.state.mutate(|store| {
        store.users.remove("a");
        Ok(())
    }).unwrap();
    let conn = store_sqlite::open_sqlite_connection(fx.dir.path()).unwrap();
    assert_eq!(current(&conn, "a").unwrap(), None);
}
