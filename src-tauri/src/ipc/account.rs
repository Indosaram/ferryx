use std::time::Duration;

use serde::Serialize;
use serde_json::json;

use super::error::{IpcError, IpcErrorCode};
use super::run_blocking;
use crate::account::enroll_client::{self, AccountEnrollmentRecord};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountEnrollmentStatus {
    pub enrolled: bool,
    pub account_origin: Option<String>,
    pub enrolled_at: Option<u64>,
}

impl From<Option<AccountEnrollmentRecord>> for AccountEnrollmentStatus {
    fn from(record: Option<AccountEnrollmentRecord>) -> Self {
        match record {
            Some(record) => Self {
                enrolled: true,
                account_origin: Some(record.account_origin),
                enrolled_at: Some(record.enrolled_at),
            },
            None => Self {
                enrolled: false,
                account_origin: None,
                enrolled_at: None,
            },
        }
    }
}

#[tauri::command]
pub async fn cmd_account_enrollment_status() -> Result<AccountEnrollmentStatus, IpcError> {
    run_blocking(|| Ok(enroll_client::load_enrollment_record().into())).await
}

#[tauri::command]
pub async fn cmd_account_enroll_this_machine(
    origin: String,
    enrollment_code: String,
) -> Result<AccountEnrollmentStatus, IpcError> {
    let origin = crate::account::origin::normalize_account_origin(&origin)
        .map_err(|error| IpcError::new(IpcErrorCode::InvalidArgument, error.to_string()))?;
    let code = enrollment_code.trim().to_string();
    if code.is_empty() {
        return Err(IpcError::new(
            IpcErrorCode::InvalidArgument,
            "enrollment code is required",
        ));
    }
    // enroll_machine does sync identity-file I/O, so it must not run on a reactor thread.
    let handle = tokio::runtime::Handle::current();
    run_blocking(move || {
        handle
            .block_on(enroll_client::enroll_machine(&origin, &code))
            .map(|record| Some(record).into())
            .map_err(|error| {
                IpcError::new(IpcErrorCode::from_code_str(&error.code), error.message)
            })
    })
    .await
}

/// Raw answer of one account-service call made on behalf of the desktop webview. The UI reads
/// `body` exactly as it reads a `fetch` response, including the `{code, message, details}`
/// envelope of a non-2xx status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountHttpResponse {
    pub status: u16,
    pub body: String,
}

const ACCOUNT_API_PREFIX: &str = "/api/account/v1/";
const ATTACH_SESSION_PATH: &str = "/api/v1/attach/session";
const ACCOUNT_HTTP_DEFAULT_TIMEOUT_MS: u64 = 15_000;
const ACCOUNT_HTTP_MAX_TIMEOUT_MS: u64 = 60_000;
const ACCOUNT_HTTP_MAX_REQUEST_BYTES: usize = 1024 * 1024;
const ACCOUNT_HTTP_MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug)]
struct AccountHttpRequest {
    origin: String,
    method: reqwest::Method,
    path: String,
    bearer: Option<String>,
    body: Option<String>,
    timeout: Duration,
}

fn account_error(code: &str, message: impl Into<String>, details: serde_json::Value) -> IpcError {
    IpcError::new(IpcErrorCode::from_code_str(code), message).with_details(details)
}

/// Only the account API and the attach-session allocation are reachable, and only through a
/// path that the URL parser keeps verbatim (no dot segments, query, fragment or empty segment).
fn is_account_api_path(origin: &str, path: &str) -> bool {
    let route = path == ATTACH_SESSION_PATH
        || (path.starts_with(ACCOUNT_API_PREFIX) && path.len() > ACCOUNT_API_PREFIX.len());
    let charset = path
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"-._~%/".contains(&byte));
    if !route || !charset || path.contains("//") {
        return false;
    }
    url::Url::parse(&format!("{origin}{path}"))
        .is_ok_and(|url| url.path() == path && url.query().is_none() && url.fragment().is_none())
}

/// Validates one webview request before any byte leaves the machine. An invalid origin is a
/// typed `INVALID_ACCOUNT_ORIGIN` error; nothing falls back to a default origin.
fn account_http_request(
    origin: &str,
    method: &str,
    path: &str,
    bearer: Option<String>,
    body: Option<String>,
    timeout_ms: Option<u64>,
) -> Result<AccountHttpRequest, IpcError> {
    let normalized =
        crate::account::origin::normalize_account_origin(origin.trim()).map_err(|error| {
            account_error(
                "INVALID_ACCOUNT_ORIGIN",
                format!(
                    "account origin {origin:?} cannot be used: {}",
                    error.message
                ),
                json!({ "origin": origin, "reason": error.code }),
            )
        })?;
    let method = match method {
        "GET" => reqwest::Method::GET,
        "POST" => reqwest::Method::POST,
        other => {
            return Err(account_error(
                "INVALID_ARGUMENT",
                format!("account request method {other:?} is not allowed"),
                json!({ "method": other }),
            ))
        }
    };
    if !is_account_api_path(&normalized, path) {
        return Err(account_error(
            "INVALID_ARGUMENT",
            format!("{path:?} is not an account API path"),
            json!({ "path": path }),
        ));
    }
    let bearer = bearer.filter(|token| !token.is_empty());
    if bearer
        .as_deref()
        .is_some_and(|token| !token.bytes().all(|byte| byte.is_ascii_graphic()))
    {
        // The token itself never enters the error.
        return Err(account_error(
            "INVALID_ARGUMENT",
            "account session token is malformed",
            json!({ "path": path }),
        ));
    }
    if let Some(body) = &body {
        if method == reqwest::Method::GET {
            return Err(account_error(
                "INVALID_ARGUMENT",
                "an account GET request carries no body",
                json!({ "path": path }),
            ));
        }
        if body.len() > ACCOUNT_HTTP_MAX_REQUEST_BYTES {
            return Err(account_error(
                "PAYLOAD_TOO_LARGE",
                "account request body is too large",
                json!({ "path": path, "limitBytes": ACCOUNT_HTTP_MAX_REQUEST_BYTES }),
            ));
        }
    }
    let timeout_ms = timeout_ms
        .unwrap_or(ACCOUNT_HTTP_DEFAULT_TIMEOUT_MS)
        .clamp(1, ACCOUNT_HTTP_MAX_TIMEOUT_MS);
    Ok(AccountHttpRequest {
        origin: normalized,
        method,
        path: path.to_string(),
        bearer,
        body,
        timeout: Duration::from_millis(timeout_ms),
    })
}

fn account_transport_error(origin: &str, timeout: Duration, error: reqwest::Error) -> IpcError {
    let timeout_ms = u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX);
    if error.is_timeout() {
        return account_error(
            "REQUEST_TIMEOUT",
            format!("account service at {origin} did not answer within {timeout_ms} ms"),
            json!({ "origin": origin, "timeoutMs": timeout_ms }),
        );
    }
    let error = error.without_url();
    let cause = std::error::Error::source(&error)
        .map(|source| format!(": {source}"))
        .unwrap_or_default();
    account_error(
        "ACCOUNT_UNREACHABLE",
        format!("account service at {origin} is unreachable: {error}{cause}"),
        json!({ "origin": origin }),
    )
}

/// Sends one validated request with the shared account client. Redirects are returned, never
/// followed, so the bearer cannot be replayed to another host. The network I/O is async, so it
/// never parks a runtime worker.
async fn send_account_http_request(
    request: AccountHttpRequest,
) -> Result<AccountHttpResponse, IpcError> {
    let AccountHttpRequest {
        origin,
        method,
        path,
        bearer,
        body,
        timeout,
    } = request;
    let client = crate::account::enroll_client::account_http_client_builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| IpcError::internal(format!("account HTTP client unavailable: {error}")))?;
    let mut builder = client.request(method, format!("{origin}{path}"));
    if let Some(bearer) = &bearer {
        builder = builder.bearer_auth(bearer);
    }
    if let Some(body) = body {
        builder = builder
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body);
    }
    let mut response = builder
        .send()
        .await
        .map_err(|error| account_transport_error(&origin, timeout, error))?;
    let status = response.status().as_u16();
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| account_transport_error(&origin, timeout, error))?
    {
        if bytes.len() + chunk.len() > ACCOUNT_HTTP_MAX_RESPONSE_BYTES {
            return Err(account_error(
                "ACCOUNT_BAD_RESPONSE",
                format!("account service at {origin} sent a response larger than {ACCOUNT_HTTP_MAX_RESPONSE_BYTES} bytes"),
                json!({ "origin": origin, "limitBytes": ACCOUNT_HTTP_MAX_RESPONSE_BYTES }),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(AccountHttpResponse {
        status,
        body: String::from_utf8_lossy(&bytes).into_owned(),
    })
}

/// Account-service HTTP for the desktop webview: login, machines, grants, install keys,
/// approvals and billing go through Rust so a self-hosted account origin works without
/// widening the webview CSP.
#[tauri::command]
pub async fn cmd_account_http_request(
    origin: String,
    method: String,
    path: String,
    bearer: Option<String>,
    body: Option<String>,
    timeout_ms: Option<u64>,
) -> Result<AccountHttpResponse, IpcError> {
    let request = account_http_request(&origin, &method, &path, bearer, body, timeout_ms)?;
    send_account_http_request(request).await
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use axum::http::{HeaderMap, StatusCode};
    use axum::response::{IntoResponse, Redirect};
    use axum::routing::{get, post};

    use super::*;
    use crate::account::mailer::{Mailer, MailerError};
    use crate::account::service::{router, AccountState};

    fn code(error: &IpcError) -> String {
        error.code.to_string()
    }

    async fn call(
        origin: &str,
        method: &str,
        path: &str,
        bearer: Option<&str>,
        body: Option<serde_json::Value>,
        timeout_ms: Option<u64>,
    ) -> Result<AccountHttpResponse, IpcError> {
        cmd_account_http_request(
            origin.to_string(),
            method.to_string(),
            path.to_string(),
            bearer.map(str::to_string),
            body.map(|body| body.to_string()),
            timeout_ms,
        )
        .await
    }

    async fn serve(app: axum::Router) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{addr}"), server)
    }

    #[derive(Default)]
    struct CapturingMailer {
        links: Mutex<Vec<String>>,
    }

    impl Mailer for CapturingMailer {
        fn send_magic_link(&self, _to: &str, url: &str) -> Result<(), MailerError> {
            self.links.lock().expect("links").push(url.to_string());
            Ok(())
        }
    }

    #[test]
    fn invalid_origin_is_a_typed_error_not_a_default() {
        for origin in [
            "",
            "not a url",
            "http://account.selfhost.test",
            "https://user:secret@account.selfhost.test",
            "https://account.selfhost.test/base",
            "https://account.selfhost.test/?q=1",
        ] {
            let error =
                account_http_request(origin, "GET", "/api/account/v1/health", None, None, None)
                    .expect_err("an invalid origin must be refused");
            assert_eq!(code(&error), "INVALID_ACCOUNT_ORIGIN", "origin {origin:?}");
            assert_eq!(
                error.details.as_ref().and_then(|d| d["origin"].as_str()),
                Some(origin)
            );
        }
        let request = account_http_request(
            "https://account.selfhost.test/",
            "GET",
            "/api/account/v1/health",
            None,
            None,
            None,
        )
        .expect("a self-hosted https origin is accepted as is");
        assert_eq!(request.origin, "https://account.selfhost.test");
        assert_eq!(
            request.timeout,
            Duration::from_millis(ACCOUNT_HTTP_DEFAULT_TIMEOUT_MS)
        );
    }

    #[test]
    fn only_account_api_paths_are_reachable() {
        let origin = "https://account.selfhost.test";
        for path in [
            "/api/account/v1/machines",
            "/api/account/v1/machines/rec%201/grants",
            "/api/account/v1/billing/entitlement",
            "/api/v1/attach/session",
        ] {
            assert!(
                account_http_request(origin, "GET", path, None, None, None).is_ok(),
                "{path} must be allowed"
            );
        }
        for path in [
            "",
            "api/account/v1/machines",
            "/api/account/v1/",
            "/api/v1/pair/exchange",
            "/api/account/v1/../../v1/pair/exchange",
            "/api/account/v1/%2e%2e/%2E%2E/v1/pair/exchange",
            "/api/account/v1/./machines",
            "/api/account/v1//machines",
            "/api/account/v1/machines?all=1",
            "/api/account/v1/machines#x",
            "/api/account/v1/machines@evil.test",
        ] {
            let error = account_http_request(origin, "GET", path, None, None, None)
                .expect_err("a non-account path must be refused");
            assert_eq!(code(&error), "INVALID_ARGUMENT", "path {path:?}");
        }
    }

    #[test]
    fn method_body_and_token_shape_are_checked_without_echoing_the_token() {
        let origin = "https://account.selfhost.test";
        let path = "/api/account/v1/machines";
        for method in ["PUT", "DELETE", "get", "OPTIONS"] {
            let error = account_http_request(origin, method, path, None, None, None)
                .expect_err("only GET and POST are allowed");
            assert_eq!(code(&error), "INVALID_ARGUMENT");
        }
        let error = account_http_request(origin, "GET", path, None, Some("{}".into()), None)
            .expect_err("GET carries no body");
        assert_eq!(code(&error), "INVALID_ARGUMENT");
        let error = account_http_request(
            origin,
            "GET",
            path,
            Some("secret-token\r\nX-Injected: 1".into()),
            None,
            None,
        )
        .expect_err("a token with control bytes is refused");
        assert_eq!(code(&error), "INVALID_ARGUMENT");
        let rendered = serde_json::to_string(&error).expect("serialize");
        assert!(
            !rendered.contains("secret-token"),
            "the token must not leak: {rendered}"
        );
        let request = account_http_request(
            origin,
            "POST",
            path,
            Some(String::new()),
            None,
            Some(10_000_000),
        )
        .expect("an empty bearer means no bearer");
        assert_eq!(request.bearer, None);
        assert_eq!(
            request.timeout,
            Duration::from_millis(ACCOUNT_HTTP_MAX_TIMEOUT_MS)
        );
    }

    /// QA: a self-hosted account service on a non-relay origin (the real account router on
    /// loopback) completes the desktop sign-in and machine listing through the IPC command.
    #[tokio::test]
    async fn self_hosted_account_service_sign_in_runs_through_ipc() {
        let tmp = tempfile::tempdir().expect("temp");
        let mailer = Arc::new(CapturingMailer::default());
        let state = Arc::new(AccountState::new(
            tmp.path(),
            "https://account.selfhost.test",
            mailer.clone(),
        ));
        let (origin, server) = serve(router(state)).await;

        let health = call(&origin, "GET", "/api/account/v1/health", None, None, None)
            .await
            .expect("health");
        assert_eq!(health.status, 200);

        let requested = call(
            &origin,
            "POST",
            "/api/account/v1/login/request",
            None,
            Some(json!({ "email": "owner@selfhost.test" })),
            None,
        )
        .await
        .expect("login request");
        assert_eq!(
            requested.status,
            StatusCode::ACCEPTED.as_u16(),
            "{}",
            requested.body
        );
        let requested: serde_json::Value = serde_json::from_str(&requested.body).expect("json");
        assert!(requested["loginHandle"]
            .as_str()
            .is_some_and(|handle| !handle.is_empty()));

        let link = mailer
            .links
            .lock()
            .expect("links")
            .pop()
            .expect("magic link sent");
        let login_code = url::Url::parse(&link)
            .expect("link")
            .query_pairs()
            .find(|(key, _)| key == "code")
            .map(|(_, value)| value.into_owned())
            .expect("code in link");
        let consumed = call(
            &origin,
            "POST",
            "/api/account/v1/login/consume",
            None,
            Some(json!({ "code": login_code })),
            None,
        )
        .await
        .expect("login consume");
        assert_eq!(consumed.status, 200, "{}", consumed.body);
        let consumed: serde_json::Value = serde_json::from_str(&consumed.body).expect("json");
        let token = consumed["token"]
            .as_str()
            .expect("session token")
            .to_string();

        let machines = call(
            &origin,
            "GET",
            "/api/account/v1/machines",
            Some(token.as_str()),
            None,
            None,
        )
        .await
        .expect("machines");
        assert_eq!(machines.status, 200);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&machines.body).expect("json"),
            json!([])
        );

        let unauthorized = call(&origin, "GET", "/api/account/v1/machines", None, None, None)
            .await
            .expect("an HTTP error status is a response, not a transport error");
        assert_eq!(unauthorized.status, 401);
        let envelope: serde_json::Value = serde_json::from_str(&unauthorized.body).expect("json");
        assert_eq!(
            envelope["code"], "UNAUTHORIZED",
            "the structured envelope reaches the UI"
        );
        server.abort();
    }

    #[tokio::test]
    async fn bearer_is_sent_but_redirects_are_not_followed() {
        let followed = Arc::new(AtomicUsize::new(0));
        let target_hits = Arc::clone(&followed);
        let app = axum::Router::new()
            .route(
                "/api/account/v1/echo",
                post(|headers: HeaderMap, body: String| async move {
                    let auth = headers
                        .get("authorization")
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or_default()
                        .to_string();
                    let content_type = headers
                        .get("content-type")
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or_default()
                        .to_string();
                    axum::Json(json!({ "auth": auth, "contentType": content_type, "body": body }))
                }),
            )
            .route(
                "/api/account/v1/moved",
                get(|| async { Redirect::temporary("/api/account/v1/target") }),
            )
            .route(
                "/api/account/v1/target",
                get(move || {
                    target_hits.fetch_add(1, Ordering::SeqCst);
                    async { StatusCode::OK }
                }),
            );
        let (origin, server) = serve(app).await;

        let echoed = call(
            &origin,
            "POST",
            "/api/account/v1/echo",
            Some("tok-1"),
            Some(json!({ "email": "a@b.test" })),
            None,
        )
        .await
        .expect("echo");
        let echoed: serde_json::Value = serde_json::from_str(&echoed.body).expect("json");
        assert_eq!(echoed["auth"], "Bearer tok-1");
        assert_eq!(echoed["contentType"], "application/json");
        assert_eq!(echoed["body"], r#"{"email":"a@b.test"}"#);

        let moved = call(
            &origin,
            "GET",
            "/api/account/v1/moved",
            Some("tok-1"),
            None,
            None,
        )
        .await
        .expect("redirect answer");
        assert_eq!(moved.status, 307);
        assert_eq!(
            followed.load(Ordering::SeqCst),
            0,
            "a redirect must not be followed"
        );
        server.abort();
    }

    #[tokio::test]
    async fn unreachable_origin_is_a_typed_error_naming_the_origin() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let origin = format!("http://{}", listener.local_addr().expect("addr"));
        drop(listener);
        let error = call(
            &origin,
            "GET",
            "/api/account/v1/health",
            Some("tok-secret"),
            None,
            None,
        )
        .await
        .expect_err("nothing listens");
        assert_eq!(code(&error), "ACCOUNT_UNREACHABLE");
        assert_eq!(
            error.details.as_ref().and_then(|d| d["origin"].as_str()),
            Some(origin.as_str())
        );
        assert!(error.message.contains(&origin), "{}", error.message);
        assert!(!error.message.contains("tok-secret"));
    }

    #[tokio::test]
    async fn slow_and_oversized_answers_are_typed_errors() {
        let app = axum::Router::new()
            .route(
                "/api/account/v1/hang",
                get(|| async {
                    std::future::pending::<()>().await;
                    StatusCode::OK
                }),
            )
            .route(
                "/api/account/v1/huge",
                get(|| async {
                    "x".repeat(ACCOUNT_HTTP_MAX_RESPONSE_BYTES + 1)
                        .into_response()
                }),
            );
        let (origin, server) = serve(app).await;

        let error = call(
            &origin,
            "GET",
            "/api/account/v1/hang",
            None,
            None,
            Some(200),
        )
        .await
        .expect_err("the request times out");
        assert_eq!(code(&error), "REQUEST_TIMEOUT");
        assert_eq!(
            error.details.as_ref().and_then(|d| d["timeoutMs"].as_u64()),
            Some(200)
        );

        let error = call(&origin, "GET", "/api/account/v1/huge", None, None, None)
            .await
            .expect_err("the response is too large");
        assert_eq!(code(&error), "ACCOUNT_BAD_RESPONSE");
        server.abort();
    }
}
