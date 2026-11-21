use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use super::service::{router, AccountState};
use super::store::{now_secs, random_token, token_hash, SessionRecord, UserRecord};

struct Fixture {
    _dir: tempfile::TempDir,
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
        let mailer = Arc::new(super::mailer::FileMailer::with_dir(dir.path().join("mail")));
        let state = Arc::new(AccountState::new(dir.path(), "https://relay.test", mailer));
        let app = router(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self { _dir: dir, state, url, server }
    }

    fn user(&self, id: &str, expires_at: u64) -> String {
        let token = random_token();
        self.state.mutate(|store| {
            store.users.insert(id.into(), UserRecord {
                user_id: id.into(), email: format!("{id}@test.local"), created_at: 1,
            });
            store.sessions.insert(token_hash(&token), SessionRecord {
                user_id: id.into(), expires_at,
            });
            Ok(())
        }).unwrap();
        token
    }

    async fn me(&self, authorization: Option<&str>, query: &str) -> (u16, Value) {
        tokio::time::timeout(Duration::from_secs(10), async {
            let client = reqwest::Client::builder().no_proxy().build().unwrap();
            let mut request = client.get(format!("{}/api/account/v1/me{query}", self.url));
            if let Some(authorization) = authorization {
                request = request.header(reqwest::header::AUTHORIZATION, authorization);
            }
            let response = request.send().await.unwrap();
            (response.status().as_u16(), response.json().await.unwrap())
        }).await.expect("bounded identity request")
    }
}

#[tokio::test]
async fn identity_refuses_missing_malformed_unknown_and_expired_bearers() {
    let fx = Fixture::new().await;
    let expired = format!("Bearer {}", fx.user("expired", now_secs() - 1));
    for authorization in [None, Some("Basic invalid"), Some("Bearer "), Some("Bearer unknown"), Some(expired.as_str())] {
        let (status, body) = fx.me(authorization, "").await;
        assert_eq!(status, 401, "{body}");
        assert_eq!(body["code"], "UNAUTHORIZED");
        assert!(body.get("accountId").is_none());
    }
}

#[tokio::test]
async fn identity_comes_only_from_the_authenticated_account() {
    let fx = Fixture::new().await;
    let a = format!("Bearer {}", fx.user("canonical-a", now_secs() + 3600));
    let b = format!("Bearer {}", fx.user("canonical-b", now_secs() + 3600));
    for (authorization, query, account_id) in [
        (a.as_str(), "", "canonical-a"),
        (b.as_str(), "", "canonical-b"),
        (a.as_str(), "?accountId=canonical-b&user_id=canonical-b", "canonical-a"),
    ] {
        assert_eq!(fx.me(Some(authorization), query).await,
            (200, json!({ "accountId": account_id })));
    }
}
