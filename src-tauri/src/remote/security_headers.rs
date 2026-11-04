//! Response hardening shared by the gateway and the relay web surfaces.
//!
//! Two middlewares live here:
//!
//! - [`security_headers`] stamps the web client (responses of `serve_static_from`, marked
//!   with [`WebClientDocument`]) with a Content-Security-Policy that forbids framing and
//!   foreign scripts, plus `X-Frame-Options`, `Referrer-Policy` and `X-Content-Type-Options`.
//!   Any other HTML (server-rendered account pages with their own inline scripts) gets
//!   `frame-ancestors 'none'` and `X-Frame-Options` only, so it cannot be framed either.
//!   Every other response, notably API JSON, gets `nosniff` and `Referrer-Policy` only.
//!   [`with_hsts`] adds `Strict-Transport-Security` on a surface that serves TLS (the relay
//!   with `--tls-*`); the plain-http gateway never sends it.
//! - [`websocket_origin_guard`] refuses a WebSocket upgrade whose `Origin` is not on the
//!   surface's allowlist with 403 before any routing decision or handler runs. A missing
//!   `Origin` is a native client (daemon, CLI, desktop shell) and is allowed.
//!
//! No cookies exist on these surfaces (the bearer lives in localStorage), so no CSRF token
//! is involved.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderValue, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// Origins of the Tauri desktop shell (macOS/Linux and Windows webviews).
pub const DESKTOP_APP_ORIGINS: [&str; 3] = [
    "tauri://localhost",
    "http://tauri.localhost",
    "https://tauri.localhost",
];

pub const X_FRAME_OPTIONS_VALUE: &str = "DENY";
pub const REFERRER_POLICY_VALUE: &str = "no-referrer";
pub const HSTS_VALUE: &str = "max-age=31536000";
/// CSP for HTML that is not the web client: no framing, nothing else restricted.
pub const FRAME_ONLY_CSP: &str = "frame-ancestors 'none'";

/// Response extension marking the web client (SPA document and its assets), which gets
/// the full [`content_security_policy`].
#[derive(Clone, Copy, Debug)]
pub struct WebClientDocument;

/// Machine-readable body of a refused cross-site upgrade.
pub const ORIGIN_NOT_ALLOWED: &str = "ORIGIN_NOT_ALLOWED";

/// Decides, per request, which extra origins the page may connect to and which
/// WebSocket `Origin`s are admitted.
pub trait WebSurfacePolicy: Send + Sync + 'static {
    /// Origins (scheme://host[:port]) the served page may open HTTP and WebSocket
    /// connections to besides its own origin.
    fn connect_origins(&self) -> Vec<String>;
    /// Whether a WebSocket upgrade carrying `origin` is admitted. `headers` are the
    /// request headers, so a surface can admit its own `Host`.
    fn websocket_origin_allowed(&self, origin: &str, headers: &HeaderMap) -> bool;
}

/// Normalizes a configured URL (`https://`, `wss://`, `http://`, `ws://`) to its web
/// origin, mapping WebSocket schemes to their HTTP equivalents.
pub fn web_origin(url: &str) -> Option<String> {
    let mut parsed = url::Url::parse(url.trim()).ok()?;
    let scheme = match parsed.scheme() {
        "ws" => "http",
        "wss" => "https",
        other => other,
    }
    .to_string();
    if scheme != parsed.scheme() {
        parsed.set_scheme(&scheme).ok()?;
    }
    let origin = parsed.origin();
    origin.is_tuple().then(|| origin.ascii_serialization())
}

/// Whether `origin` is the request's own origin (`http(s)://<Host>`).
pub fn is_same_host_origin(origin: &str, headers: &HeaderMap) -> bool {
    headers
        .get(header::HOST)
        .and_then(|host| host.to_str().ok())
        .is_some_and(|host| {
            origin == format!("http://{host}") || origin == format!("https://{host}")
        })
}

/// Whether `origin` is one of the desktop shell's webview origins.
pub fn is_desktop_app_origin(origin: &str) -> bool {
    DESKTOP_APP_ORIGINS.contains(&origin)
}

/// The `connect-src` sources for one web origin: the HTTP origin and its WebSocket twin.
/// Never a bare `ws:`/`wss:` scheme source, and never an origin carrying characters that
/// could end the source list or the directive (`;`, `,`, quotes, whitespace).
fn connect_sources(origin: &str) -> Option<[String; 2]> {
    let origin = web_origin(origin)?;
    if !origin
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '/' | '[' | ']'))
    {
        return None;
    }
    let socket = if let Some(rest) = origin.strip_prefix("https://") {
        format!("wss://{rest}")
    } else {
        format!("ws://{}", origin.strip_prefix("http://")?)
    };
    Some([origin, socket])
}

/// Builds the document Content-Security-Policy for a page that may also talk to
/// `connect_origins`.
pub fn content_security_policy<I, S>(connect_origins: I) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut connect = vec!["'self'".to_string()];
    for origin in connect_origins {
        for source in connect_sources(origin.as_ref()).into_iter().flatten() {
            if !connect.contains(&source) {
                connect.push(source);
            }
        }
    }
    format!(
        "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
         img-src 'self' data: blob:; connect-src {}; frame-ancestors 'none'; \
         base-uri 'none'; form-action 'self'",
        connect.join(" ")
    )
}

fn is_html(headers: &HeaderMap) -> bool {
    headers.get_all(header::CONTENT_TYPE).iter().any(|value| {
        value
            .to_str()
            .is_ok_and(|value| value.trim().to_ascii_lowercase().starts_with("text/html"))
    })
}

/// Applies the response headers for one response. HTML documents get `csp` (or
/// [`FRAME_ONLY_CSP`] when `None`) and `X-Frame-Options`; every response gets `nosniff` and
/// `Referrer-Policy`. Headers a handler already set (for example the relay's
/// `default-src 'none'` on proxied API responses) are kept.
pub fn apply_security_headers(headers: &mut HeaderMap, csp: Option<&str>) {
    headers
        .entry(header::X_CONTENT_TYPE_OPTIONS)
        .or_insert(HeaderValue::from_static("nosniff"));
    headers
        .entry(header::REFERRER_POLICY)
        .or_insert(HeaderValue::from_static(REFERRER_POLICY_VALUE));
    if !is_html(headers) {
        return;
    }
    let value = match csp.and_then(|csp| HeaderValue::from_str(csp).ok()) {
        Some(value) => value,
        None => HeaderValue::from_static(FRAME_ONLY_CSP),
    };
    headers.entry(header::CONTENT_SECURITY_POLICY).or_insert(value);
    headers.insert(
        header::X_FRAME_OPTIONS,
        HeaderValue::from_static(X_FRAME_OPTIONS_VALUE),
    );
}

/// Response middleware: see the module docs.
pub async fn security_headers(
    State(policy): State<Arc<dyn WebSurfacePolicy>>,
    request: Request<Body>,
    next: Next,
) -> Response {
    // The page's own host, as both schemes: CSP Level 3 lets `'self'` cover same-host
    // WebSockets, but older WebKit does not, and the phone shell is often an old Safari.
    let own_host = request
        .headers()
        .get(header::HOST)
        .and_then(|host| host.to_str().ok())
        .map(|host| [format!("http://{host}"), format!("https://{host}")]);
    let mut response = next.run(request).await;
    if response.status() == StatusCode::SWITCHING_PROTOCOLS {
        return response;
    }
    let web_client = response.extensions().get::<WebClientDocument>().is_some();
    let csp = (web_client && is_html(response.headers())).then(|| {
        content_security_policy(
            own_host
                .into_iter()
                .flatten()
                .chain(policy.connect_origins()),
        )
    });
    apply_security_headers(response.headers_mut(), csp.as_deref());
    response
}

/// Response middleware adding `Strict-Transport-Security`; see [`with_hsts`].
pub async fn strict_transport_security(request: Request<Body>, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .entry(header::STRICT_TRANSPORT_SECURITY)
        .or_insert(HeaderValue::from_static(HSTS_VALUE));
    response
}

/// Adds HSTS to every response of a surface that is served over TLS only.
pub fn with_hsts<S>(router: axum::Router<S>) -> axum::Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router.layer(axum::middleware::from_fn(strict_transport_security))
}

/// Whether the request asks for a WebSocket upgrade.
pub fn is_websocket_upgrade(headers: &HeaderMap) -> bool {
    headers
        .get(header::UPGRADE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("websocket"))
}

/// Request middleware: refuses a cross-site WebSocket upgrade with 403 before routing.
pub async fn websocket_origin_guard(
    State(policy): State<Arc<dyn WebSurfacePolicy>>,
    request: Request<Body>,
    next: Next,
) -> Response {
    if is_websocket_upgrade(request.headers()) {
        if let Some(origin) = request.headers().get(header::ORIGIN) {
            let allowed = origin
                .to_str()
                .is_ok_and(|origin| policy.websocket_origin_allowed(origin, request.headers()));
            if !allowed {
                return (StatusCode::FORBIDDEN, ORIGIN_NOT_ALLOWED).into_response();
            }
        }
    }
    next.run(request).await
}

/// Wraps `router` with both middlewares. Apply it after every sub-router is merged:
/// `Router::layer` only wraps the routes that exist when it is called. The origin guard
/// is the outer layer, so a refused upgrade never reaches routing.
pub fn harden<S>(router: axum::Router<S>, policy: Arc<dyn WebSurfacePolicy>) -> axum::Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router
        .layer(axum::middleware::from_fn_with_state(
            Arc::clone(&policy),
            security_headers,
        ))
        .layer(axum::middleware::from_fn_with_state(
            policy,
            websocket_origin_guard,
        ))
}

#[cfg(test)]
mod tests {
    use super::tests_support::*;
    use super::*;
    use axum::routing::get;
    use std::net::SocketAddr;

    struct Fixed;
    impl WebSurfacePolicy for Fixed {
        fn connect_origins(&self) -> Vec<String> {
            vec!["https://relay.example".into()]
        }
        fn websocket_origin_allowed(&self, origin: &str, headers: &HeaderMap) -> bool {
            origin == "https://relay.example" || is_same_host_origin(origin, headers)
        }
    }

    async fn serve() -> (SocketAddr, tokio::task::JoinHandle<()>) {
        let router = axum::Router::new()
            .route(
                "/api/v1/x",
                get(|| async { axum::Json(serde_json::json!({ "ok": true })) }),
            )
            .route("/ws", get(|| async { "upgrade target" }))
            .route(
                "/account-page",
                get(|| async { axum::response::Html("<script>inline()</script>") }),
            )
            .fallback(|| async {
                let mut response = axum::response::Html("<!doctype html>").into_response();
                response.extensions_mut().insert(WebClientDocument);
                response
            });
        let app = with_hsts(harden(router, Arc::new(Fixed)));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (addr, server)
    }

    #[test]
    fn csp_lists_relay_https_and_wss_and_never_a_bare_scheme() {
        let csp = content_security_policy(["https://relay.ferryx.dev/", "wss://relay.ferryx.dev"]);
        assert_eq!(
            csp,
            "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
             img-src 'self' data: blob:; connect-src 'self' https://relay.ferryx.dev \
             wss://relay.ferryx.dev; frame-ancestors 'none'; base-uri 'none'; form-action 'self'"
        );
        for token in csp.split([' ', ';']) {
            assert!(token != "ws:" && token != "wss:", "bare scheme source in {csp}");
        }
        // Garbage origins contribute nothing rather than a broken source; an origin that
        // would smuggle a directive (a `Host: a;script-src` header) is dropped.
        assert_eq!(
            content_security_policy([
                "not a url",
                "javascript:alert(1)",
                "https://a b",
                "http://a;script-src",
                "http://a,b",
            ]),
            content_security_policy(Vec::<String>::new())
        );
        let plain = content_security_policy(["http://10.0.0.2:8787"]);
        assert!(plain.contains("connect-src 'self' http://10.0.0.2:8787 ws://10.0.0.2:8787;"));
    }

    #[tokio::test]
    async fn html_gets_every_document_header() {
        let (addr, server) = serve().await;
        let response = http().get(format!("http://{addr}/")).send().await.unwrap();
        assert_eq!(response.status(), 200);
        let csp = header_str(&response, "content-security-policy").expect("csp");
        assert!(csp.contains("frame-ancestors 'none'"), "{csp}");
        assert!(csp.contains("script-src 'self';"), "{csp}");
        assert!(
            csp.contains(&format!(
                "connect-src 'self' http://{addr} ws://{addr} https://{addr} wss://{addr} \
                 https://relay.example wss://relay.example;"
            )),
            "{csp}"
        );
        assert_eq!(header_str(&response, "x-frame-options"), Some("DENY"));
        assert_eq!(header_str(&response, "referrer-policy"), Some("no-referrer"));
        assert_eq!(header_str(&response, "x-content-type-options"), Some("nosniff"));
        assert_eq!(
            header_str(&response, "strict-transport-security"),
            Some("max-age=31536000")
        );
        server.abort();
    }

    #[tokio::test]
    async fn other_html_cannot_be_framed_but_keeps_its_inline_scripts() {
        let (addr, server) = serve().await;
        let response = http()
            .get(format!("http://{addr}/account-page"))
            .send()
            .await
            .unwrap();
        assert_eq!(
            header_str(&response, "content-security-policy"),
            Some("frame-ancestors 'none'")
        );
        assert_eq!(header_str(&response, "x-frame-options"), Some("DENY"));
        assert_eq!(header_str(&response, "referrer-policy"), Some("no-referrer"));
        server.abort();
    }

    #[tokio::test]
    async fn api_json_gets_nosniff_and_referrer_policy_only() {
        let (addr, server) = serve().await;
        let response = http()
            .get(format!("http://{addr}/api/v1/x"))
            .send()
            .await
            .unwrap();
        assert_eq!(header_str(&response, "x-content-type-options"), Some("nosniff"));
        assert_eq!(header_str(&response, "referrer-policy"), Some("no-referrer"));
        assert!(response.headers().get("content-security-policy").is_none());
        assert!(response.headers().get("x-frame-options").is_none());
        server.abort();
    }

    #[tokio::test]
    async fn hsts_is_only_added_by_the_tls_layer() {
        let router = axum::Router::new().route("/", get(|| async { "plain" }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, harden(router, Arc::new(Fixed))).await.unwrap();
        });
        let response = http().get(format!("http://{addr}/")).send().await.unwrap();
        assert!(response.headers().get("strict-transport-security").is_none());
        server.abort();
    }

    #[test]
    fn handler_headers_are_kept() {
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain"));
        headers.insert(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static("default-src 'none'"),
        );
        apply_security_headers(&mut headers, Some("default-src 'self'"));
        assert!(headers.get(header::X_FRAME_OPTIONS).is_none());
        assert_eq!(headers[header::CONTENT_SECURITY_POLICY], "default-src 'none'");
        assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    }

    #[tokio::test]
    async fn foreign_origin_upgrade_is_refused_before_routing() {
        let (addr, server) = serve().await;
        for path in ["/ws", "/no/such/route"] {
            assert_eq!(
                upgrade_status(addr, path, Some("https://evil.example")).await,
                403,
                "{path}"
            );
        }
        // An opaque Origin is refused as well.
        assert_eq!(upgrade_status(addr, "/ws", Some("null")).await, 403);
        server.abort();
    }

    #[tokio::test]
    async fn native_and_allowlisted_upgrades_pass_the_guard() {
        let (addr, server) = serve().await;
        let same_host = format!("http://{addr}");
        for origin in [None, Some("https://relay.example"), Some(same_host.as_str())] {
            // The route is not a WebSocket handler, so a passed guard answers 200.
            assert_eq!(upgrade_status(addr, "/ws", origin).await, 200, "{origin:?}");
        }
        // A plain request with a foreign Origin is not an upgrade; CORS governs it.
        let response = http()
            .get(format!("http://{addr}/api/v1/x"))
            .header("Origin", "https://evil.example")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        server.abort();
    }
}

/// The gateway router (`create_remote_router`) carries the same hardening. These live here
/// so the security filter `remote::security_headers` runs them with the module tests.
#[cfg(test)]
mod gateway_tests {
    use super::tests_support::*;
    use crate::remote::{create_remote_router, RemoteGatewayState};
    use crate::terminal::TerminalService;
    use crate::worktree::WorkspaceRegistry;
    use std::sync::Arc;

    async fn gateway() -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
        let state = Arc::new(RemoteGatewayState::new(
            Arc::new(TerminalService::default()),
            WorkspaceRegistry::new(),
        ));
        state.config.write().relay_url = Some("wss://relay.selfhost.example/".to_string());
        *state.cors_saved_host_origins.write() = vec!["https://saved.example".to_string()];
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, create_remote_router(state)).await.unwrap();
        });
        (addr, server)
    }

    #[tokio::test]
    async fn gateway_refuses_cross_site_websocket_upgrades_with_403() {
        let (addr, server) = gateway().await;
        for path in [
            "/api/v1/events?ticket=bad",
            "/api/v1/terminal/session?ticket=bad",
            "/api/v1/browser/b1?ticket=bad",
            "/api/v1/workspace/dag?ticket=bad",
            "/api/v1/no-such-socket",
        ] {
            for origin in ["https://evil.example", "null", "https://relay.ferryx.dev.evil.example"] {
                assert_eq!(
                    upgrade_status(addr, path, Some(origin)).await,
                    403,
                    "{origin} upgrading {path}"
                );
            }
        }
        let own = format!("http://{addr}");
        for origin in [
            None,
            Some(own.as_str()),
            Some(crate::remote::state::DEFAULT_RELAY_URL),
            Some("https://relay.selfhost.example"),
            Some("https://saved.example"),
            Some("tauri://localhost"),
            Some("http://tauri.localhost"),
            Some("https://tauri.localhost"),
        ] {
            // Past the guard the handler's own credential check answers 401.
            assert_eq!(
                upgrade_status(addr, "/api/v1/events?ticket=bad", origin).await,
                401,
                "{origin:?} must pass the origin guard"
            );
        }
        server.abort();
    }

    #[tokio::test]
    async fn gateway_documents_and_api_responses_carry_security_headers() {
        let (addr, server) = gateway().await;
        let response = http().get(format!("http://{addr}/")).send().await.unwrap();
        let content_type = header_str(&response, "content-type").unwrap_or_default();
        assert!(content_type.starts_with("text/html"), "{content_type}");
        let csp = header_str(&response, "content-security-policy").expect("csp");
        assert!(csp.contains("frame-ancestors 'none'"), "{csp}");
        assert!(
            csp.contains("https://relay.ferryx.dev wss://relay.ferryx.dev"),
            "the default relay is connectable: {csp}"
        );
        assert!(
            csp.contains("https://relay.selfhost.example wss://relay.selfhost.example"),
            "the configured relay is connectable: {csp}"
        );
        assert_eq!(header_str(&response, "x-frame-options"), Some("DENY"));
        assert_eq!(header_str(&response, "referrer-policy"), Some("no-referrer"));
        assert_eq!(header_str(&response, "x-content-type-options"), Some("nosniff"));
        assert!(response.headers().get("strict-transport-security").is_none());

        let response = http()
            .get(format!("http://{addr}/api/v1/health"))
            .send()
            .await
            .unwrap();
        assert_eq!(header_str(&response, "x-content-type-options"), Some("nosniff"));
        assert_eq!(header_str(&response, "referrer-policy"), Some("no-referrer"));
        assert!(response.headers().get("x-frame-options").is_none());
        assert!(response.headers().get("content-security-policy").is_none());
        server.abort();
    }
}

#[cfg(test)]
mod tests_support {
    use std::net::SocketAddr;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    pub fn http() -> reqwest::Client {
        reqwest::Client::builder().no_proxy().build().unwrap()
    }

    pub fn header_str<'a>(response: &'a reqwest::Response, name: &str) -> Option<&'a str> {
        response.headers().get(name).and_then(|value| value.to_str().ok())
    }

    /// Sends a raw WebSocket upgrade request and returns the status code of the reply.
    pub async fn upgrade_status(addr: SocketAddr, path: &str, origin: Option<&str>) -> u16 {
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        let origin = origin
            .map(|origin| format!("Origin: {origin}\r\n"))
            .unwrap_or_default();
        let request = format!(
            "GET {path} HTTP/1.1\r\nHost: {addr}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
             Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n{origin}\r\n"
        );
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut buf = [0u8; 512];
        let n = tokio::time::timeout(std::time::Duration::from_secs(5), stream.read(&mut buf))
            .await
            .expect("the server answers the upgrade in time")
            .unwrap();
        String::from_utf8_lossy(&buf[..n])
            .split_whitespace()
            .nth(1)
            .and_then(|code| code.parse().ok())
            .unwrap_or(0)
    }
}
