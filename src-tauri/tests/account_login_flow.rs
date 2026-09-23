use std::sync::Arc;

use ferryx_lib::account::mailer::{FileMailer, Mailer, MailerError};
use ferryx_lib::account::service::{router, AccountState};
use ferryx_lib::account::store::{now_secs, token_hash, AccountStore, LoginCodeRecord};
use serde_json::{json, Value};

struct FailingMailer;

impl Mailer for FailingMailer {
    fn send_magic_link(&self, _to: &str, _url: &str) -> Result<(), MailerError> {
        Err(MailerError::mail_failed("smtp refused the message"))
    }
}

struct Server {
    base: String,
    handle: tokio::task::JoinHandle<()>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn spawn(state: AccountState) -> Server {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let handle = tokio::spawn(async move {
        let _ = ferryx_lib::account::service::serve(listener, Arc::new(state)).await;
    });
    Server {
        base: format!("http://{addr}"),
        handle,
    }
}

async fn post(base: &str, path: &str, body: Value, bearer: Option<&str>) -> (u16, Value) {
    let client = reqwest::Client::new();
    let mut request = client.post(format!("{base}{path}")).json(&body);
    if let Some(token) = bearer {
        request = request.bearer_auth(token);
    }
    let response = request.send().await.expect("request");
    let status = response.status().as_u16();
    let text = response.text().await.expect("body");
    let value = if text.is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&text).unwrap_or(Value::String(text))
    };
    (status, value)
}

fn read_magic_link(mail_dir: &std::path::Path) -> String {
    let entry = std::fs::read_dir(mail_dir)
        .expect("mail dir")
        .filter_map(Result::ok)
        .find(|entry| entry.path().is_file())
        .expect("a mail file");
    std::fs::read_to_string(entry.path()).expect("mail body")
}

fn code_from(text: &str) -> String {
    text.split("code=")
        .nth(1)
        .expect("code param")
        .trim()
        .to_string()
}

#[tokio::test]
async fn account_login_flow() {
    let data_dir = tempfile::tempdir().expect("data");
    let mail_dir = tempfile::tempdir().expect("mail");
    let state = AccountState::new(
        data_dir.path(),
        "http://127.0.0.1:9",
        Arc::new(FileMailer::with_dir(mail_dir.path())),
    );
    let server = spawn(state).await;

    let (status, _) = post(
        &server.base,
        "/api/account/v1/login/request",
        json!({ "email": "a@b.co" }),
        None,
    )
    .await;
    assert_eq!(status, 202, "known address is accepted");

    let code = code_from(&read_magic_link(mail_dir.path()));
    let (status, body) = post(
        &server.base,
        "/api/account/v1/login/consume",
        json!({ "code": code }),
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let token = body["token"].as_str().expect("token").to_string();
    assert_eq!(body["email"], "a@b.co");
    assert!(body.get("password").is_none());

    let store = AccountStore::load(data_dir.path()).expect("store");
    assert!(
        !std::fs::read_to_string(data_dir.path().join("account-store.json"))
            .expect("raw store")
            .contains(&token),
        "the session bearer must never be persisted in the clear"
    );
    assert_eq!(store.users.len(), 1);

    let (status, body) = post(
        &server.base,
        "/api/account/v1/login/consume",
        json!({ "code": code }),
        None,
    )
    .await;
    assert_eq!(status, 401, "{body}");
    assert_eq!(body["code"], "LOGIN_CODE_USED");

    let (status, _) = post(
        &server.base,
        "/api/account/v1/login/request",
        json!({ "email": "nobody@b.co" }),
        None,
    )
    .await;
    assert_eq!(status, 202, "an unknown address looks identical");
    let store = AccountStore::load(data_dir.path()).expect("store");
    assert_eq!(store.users.len(), 1, "request alone creates no account");

    let (status, body) = post(
        &server.base,
        "/api/account/v1/logout",
        json!({}),
        Some(&token),
    )
    .await;
    assert_eq!(status, 204, "{body}");
    let (status, body) = post(
        &server.base,
        "/api/account/v1/enrollment-codes",
        json!({}),
        Some(&token),
    )
    .await;
    assert_eq!(status, 401, "a revoked session cannot issue enrollment codes: {body}");
}

#[tokio::test]
async fn account_login_expired_code_is_rejected() {
    let data_dir = tempfile::tempdir().expect("data");
    let mail_dir = tempfile::tempdir().expect("mail");
    let expired = "0123456789abcdef0123456789abcdef";
    let mut store = AccountStore::load(data_dir.path()).expect("store");
    store.login_codes.insert(
        token_hash(expired),
        LoginCodeRecord {
            email: "a@b.co".into(),
            expires_at: now_secs() - 1,
        },
    );
    store.save(data_dir.path()).expect("save");

    let state = AccountState::new(
        data_dir.path(),
        "http://127.0.0.1:9",
        Arc::new(FileMailer::with_dir(mail_dir.path())),
    );
    let server = spawn(state).await;
    let (status, body) = post(
        &server.base,
        "/api/account/v1/login/consume",
        json!({ "code": expired }),
        None,
    )
    .await;
    assert_eq!(status, 401, "{body}");
    assert_eq!(body["code"], "LOGIN_CODE_EXPIRED");
}

#[tokio::test]
async fn account_login_mail_failure_is_reported_for_every_address() {
    let data_dir = tempfile::tempdir().expect("data");
    let state = AccountState::new(
        data_dir.path(),
        "http://127.0.0.1:9",
        Arc::new(FailingMailer),
    );
    let server = spawn(state).await;
    for email in ["a@b.co", "nobody@b.co"] {
        let (status, body) = post(
            &server.base,
            "/api/account/v1/login/request",
            json!({ "email": email }),
            None,
        )
        .await;
        assert_eq!(status, 503, "{body}");
        assert_eq!(body["code"], "MAIL_FAILED");
    }
    let store = AccountStore::load(data_dir.path()).expect("store");
    assert!(
        store.login_codes.is_empty(),
        "a failed delivery must not leave a live login code"
    );
}
