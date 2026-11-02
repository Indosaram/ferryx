//! Identity providers refuse sign-in inside embedded system webviews: Google with
//! "disallowed_useragent" / "This browser or app may not be secure", Apple, Microsoft and Okta
//! with an "unsupported browser" page. A tab that lands on such a page is offered a handoff
//! that keeps the user in Ferryx: the same sign-in in a Chromium engine tab of the same profile
//! (installing the engine first when needed), or on macOS the system web-authentication
//! session whose callback loads in the original tab. Opening another browser is only offered
//! after the user declines those.

use crate::browser::cookies::ImportedCookie;
use crate::browser::engine_install::EngineStatus;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::LazyLock;
use url::Url;

const GOOGLE_ACCOUNTS_HOST: &str = "accounts.google.com";
const APPLE_HOSTS: &[&str] = &["appleid.apple.com", "idmsa.apple.com"];
const MICROSOFT_HOSTS: &[&str] = &["login.microsoftonline.com", "login.live.com"];
const OKTA_HOST_SUFFIXES: &[&str] = &[".okta.com", ".oktapreview.com"];
/// Path fragments of Google's rejection pages.
const BLOCK_PATH_MARKERS: &[&str] = &["/signin/rejected", "/v3/signin/rejected"];
/// Query markers Google puts on the OAuth error page for embedded webviews.
const BLOCK_QUERY_MARKERS: &[&str] = &["disallowed_useragent", "disallowed-useragent"];
/// Titles of Google's rejection page (English UI).
const BLOCK_TITLE_MARKERS: &[&str] = &[
    "this browser or app may not be secure",
    "couldn't sign you in",
    "couldn\u{2019}t sign you in",
];
/// Titles of the Apple, Microsoft and Okta "unsupported browser" pages (English UI).
const UNSUPPORTED_BROWSER_TITLE_MARKERS: &[&str] = &[
    "browser is not supported",
    "unsupported browser",
    "browser not supported",
    "update your browser",
];
/// Where a Google handoff restarts when the blocked page names no `continue` target.
const DEFAULT_SIGN_IN_URL: &str = "https://accounts.google.com/";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SignInProvider {
    Google,
    Apple,
    Microsoft,
    Okta,
}

/// What the tab's page is, for the handoff bar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SignInBlock {
    /// The provider blocked sign-in in this webview; `retry_url` restarts it elsewhere.
    #[serde(rename_all = "camelCase")]
    Blocked {
        provider: SignInProvider,
        retry_url: String,
    },
    NotBlocked,
}

/// The identity provider an https URL belongs to, by exact host.
fn provider_for(url: &Url) -> Option<SignInProvider> {
    if url.scheme() != "https" {
        return None;
    }
    let host = url.host_str()?.to_ascii_lowercase();
    if host == GOOGLE_ACCOUNTS_HOST {
        Some(SignInProvider::Google)
    } else if APPLE_HOSTS.contains(&host.as_str()) {
        Some(SignInProvider::Apple)
    } else if MICROSOFT_HOSTS.contains(&host.as_str()) {
        Some(SignInProvider::Microsoft)
    } else if OKTA_HOST_SUFFIXES.iter().any(|suffix| host.ends_with(suffix)) {
        Some(SignInProvider::Okta)
    } else {
        None
    }
}

/// Classifies a tab's address and title as an identity provider's embedded-webview block.
pub fn detect_sign_in_block(url: &str, title: Option<&str>) -> SignInBlock {
    let Ok(parsed) = Url::parse(url) else {
        return SignInBlock::NotBlocked;
    };
    classify(&parsed, title, &provider_for)
}

/// Detection with the host check passed in, so a test can point it at a local fixture.
fn classify(
    parsed: &Url,
    title: Option<&str>,
    provider_of: &dyn Fn(&Url) -> Option<SignInProvider>,
) -> SignInBlock {
    let Some(provider) = provider_of(parsed) else {
        return SignInBlock::NotBlocked;
    };
    let title = title.unwrap_or_default().to_lowercase();
    let blocked = match provider {
        SignInProvider::Google => google_blocked(parsed, &title),
        SignInProvider::Apple | SignInProvider::Microsoft | SignInProvider::Okta => {
            UNSUPPORTED_BROWSER_TITLE_MARKERS
                .iter()
                .any(|marker| title.contains(marker))
        }
    };
    if !blocked {
        return SignInBlock::NotBlocked;
    }
    let retry_url = match provider {
        SignInProvider::Google => google_retry_url(parsed),
        // The blocked page is the sign-in page itself.
        SignInProvider::Apple | SignInProvider::Microsoft | SignInProvider::Okta => {
            parsed.to_string()
        }
    };
    SignInBlock::Blocked {
        provider,
        retry_url,
    }
}

fn google_blocked(parsed: &Url, lowercase_title: &str) -> bool {
    let path = parsed.path().to_ascii_lowercase();
    let query = parsed.query().unwrap_or_default().to_ascii_lowercase();
    BLOCK_PATH_MARKERS
        .iter()
        .any(|marker| path.starts_with(marker))
        || BLOCK_QUERY_MARKERS
            .iter()
            .any(|marker| query.contains(marker))
        || BLOCK_TITLE_MARKERS
            .iter()
            .any(|marker| lowercase_title.contains(marker))
}

/// The `continue` target when it is an https URL, else Google's account sign-in page.
fn google_retry_url(blocked: &Url) -> String {
    blocked
        .query_pairs()
        .find(|(key, _)| key == "continue")
        .and_then(|(_, value)| Url::parse(&value).ok())
        .filter(|target| target.scheme() == "https")
        .map(|target| target.to_string())
        .unwrap_or_else(|| DEFAULT_SIGN_IN_URL.to_string())
}

/// Callback the OS auth session waits for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AuthCallback {
    /// redirect_uri with a custom scheme (e.g. com.example.app:/cb)
    Scheme { scheme: String },
    /// https redirect_uri
    Https { host: String, path: String },
}

/// An OAuth authorize request the tab made, and the callback it redirects to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthFlow {
    pub authorize_url: String,
    pub callback: AuthCallback,
}

/// Newest first: the blocked URL, then `history` (oldest first, as a tab stores it) from newest
/// to oldest. The first https URL on a known identity-provider host whose query has client_id,
/// redirect_uri and response_type wins.
pub fn oauth_flow(blocked_url: &str, history: &[String]) -> Option<OAuthFlow> {
    find_oauth_flow(blocked_url, history, &provider_for)
}

fn find_oauth_flow(
    blocked_url: &str,
    history: &[String],
    provider_of: &dyn Fn(&Url) -> Option<SignInProvider>,
) -> Option<OAuthFlow> {
    std::iter::once(blocked_url)
        .chain(history.iter().rev().map(String::as_str))
        .find_map(|candidate| authorize_request(candidate, provider_of))
}

fn authorize_request(
    candidate: &str,
    provider_of: &dyn Fn(&Url) -> Option<SignInProvider>,
) -> Option<OAuthFlow> {
    let parsed = Url::parse(candidate).ok()?;
    provider_of(&parsed)?;
    let param = |name: &str| {
        parsed
            .query_pairs()
            .find(|(key, value)| key == name && !value.is_empty())
            .map(|(_, value)| value.into_owned())
    };
    param("client_id")?;
    param("response_type")?;
    let callback = callback_for(&param("redirect_uri")?)?;
    Some(OAuthFlow {
        authorize_url: candidate.to_string(),
        callback,
    })
}

/// The callback a redirect_uri names: an https host and path, or a custom scheme. Plain http
/// (e.g. a loopback redirect) is not something an OS auth session can wait for.
fn callback_for(redirect_uri: &str) -> Option<AuthCallback> {
    let parsed = Url::parse(redirect_uri).ok()?;
    match parsed.scheme() {
        "https" => {
            let host = parsed.host_str().filter(|host| !host.is_empty())?;
            Some(AuthCallback::Https {
                host: host.to_string(),
                path: parsed.path().to_string(),
            })
        }
        "http" => None,
        scheme => Some(AuthCallback::Scheme {
            scheme: scheme.to_string(),
        }),
    }
}

/// Whether a Chromium tab can take the sign-in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EngineAvailability {
    Ready,
    NotInstalled,
    /// Installed but this session cannot run it (e.g. a Wayland session).
    Unusable,
    Unsupported,
}

impl EngineAvailability {
    pub fn from_status(status: &EngineStatus) -> Self {
        match status {
            EngineStatus::Installed { inactive: None, .. } => Self::Ready,
            EngineStatus::Installed {
                inactive: Some(_), ..
            } => Self::Unusable,
            EngineStatus::NotInstalled => Self::NotInstalled,
            EngineStatus::Unsupported => Self::Unsupported,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HostPlatform {
    Macos,
    Windows,
    Linux,
}

/// The running OS; any OS other than macOS and Windows counts as Linux.
pub fn host_platform() -> HostPlatform {
    if cfg!(target_os = "macos") {
        HostPlatform::Macos
    } else if cfg!(target_os = "windows") {
        HostPlatform::Windows
    } else {
        HostPlatform::Linux
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum HandoffAction {
    ChromiumTab {
        url: String,
    },
    InstallEngine {
        url: String,
    },
    SystemAuthSession {
        authorize_url: String,
        callback: AuthCallback,
    },
    ExternalBrowser {
        url: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoffPlan {
    pub provider: SignInProvider,
    pub engine: EngineAvailability,
    /// The offered action.
    pub primary: HandoffAction,
    /// Other in-app actions, shown next to the primary and tried in order when it fails.
    pub alternatives: Vec<HandoffAction>,
    /// Shown only after the user declines the in-app actions (a secondary menu).
    pub on_decline: Vec<HandoffAction>,
}

/// The decision table. `auth_session` is whether the OS auth session can run `flow` (false off
/// macOS). In-app actions come first; another browser is only offered once they are declined,
/// or as the only action when no in-app path exists.
pub fn handoff_plan(
    provider: SignInProvider,
    retry_url: &str,
    engine: EngineAvailability,
    platform: HostPlatform,
    flow: Option<&OAuthFlow>,
    auth_session: bool,
) -> HandoffPlan {
    let mut in_app = Vec::new();
    match engine {
        EngineAvailability::Ready => in_app.push(HandoffAction::ChromiumTab {
            url: retry_url.to_string(),
        }),
        EngineAvailability::NotInstalled => in_app.push(HandoffAction::InstallEngine {
            url: retry_url.to_string(),
        }),
        EngineAvailability::Unusable | EngineAvailability::Unsupported => {}
    }
    if let Some(flow) = flow.filter(|_| platform == HostPlatform::Macos && auth_session) {
        in_app.push(HandoffAction::SystemAuthSession {
            authorize_url: flow.authorize_url.clone(),
            callback: flow.callback.clone(),
        });
    }
    let external = HandoffAction::ExternalBrowser {
        url: retry_url.to_string(),
    };
    let mut in_app = in_app.into_iter();
    match in_app.next() {
        Some(primary) => HandoffPlan {
            provider,
            engine,
            primary,
            alternatives: in_app.collect(),
            on_decline: vec![external],
        },
        None => HandoffPlan {
            provider,
            engine,
            primary: external,
            alternatives: Vec::new(),
            on_decline: Vec::new(),
        },
    }
}

/// Converts a CDP `Network.getCookies` result into importable cookies. A session cookie
/// (`expires` -1) has no expiry; entries without a name are skipped.
pub fn cdp_cookies(result: &serde_json::Value) -> Vec<ImportedCookie> {
    let Some(cookies) = result.get("cookies").and_then(serde_json::Value::as_array) else {
        return Vec::new();
    };
    cookies
        .iter()
        .filter_map(|cookie| {
            let text = |key: &str| cookie.get(key).and_then(serde_json::Value::as_str);
            let flag = |key: &str| {
                cookie
                    .get(key)
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false)
            };
            let name = text("name").filter(|name| !name.is_empty())?.to_string();
            let session = flag("session");
            let expires_unix = cookie
                .get("expires")
                .and_then(serde_json::Value::as_f64)
                .filter(|expires| !session && expires.is_finite() && *expires > 0.0)
                .map(|expires| expires.floor() as i64);
            let same_site = text("sameSite").and_then(|value| match value {
                "Strict" => Some("strict".to_string()),
                "Lax" => Some("lax".to_string()),
                "None" => Some("none".to_string()),
                _ => None,
            });
            Some(ImportedCookie {
                name,
                value: text("value").unwrap_or_default().to_string(),
                domain: text("domain")
                    .filter(|domain| !domain.is_empty())
                    .map(str::to_string),
                path: text("path")
                    .filter(|path| path.starts_with('/'))
                    .unwrap_or("/")
                    .to_string(),
                secure: flag("secure"),
                http_only: flag("httpOnly"),
                expires_unix,
                same_site,
            })
        })
        .collect()
}

/// A Chromium tab opened for a sign-in, waiting to finish it so its cookies can be copied into
/// the profile's system-webview tabs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingHandoff {
    pub chromium_browser_id: String,
    pub profile_id: String,
    pub target_url: String,
    pub idp_host: String,
}

struct TrackedHandoff {
    handoff: PendingHandoff,
    /// Whether the tab has shown the identity provider's page; it only counts as finished
    /// once it leaves that page.
    seen_idp: bool,
}

static PENDING: LazyLock<Mutex<HashMap<String, TrackedHandoff>>> =
    LazyLock::new(Default::default);

fn is_idp_host(url: &Url, idp_host: &str) -> bool {
    url.host_str()
        .is_some_and(|host| host.eq_ignore_ascii_case(idp_host))
}

/// Remembers a handoff tab until its sign-in finishes. A tab that starts on the identity
/// provider's page counts as having seen it.
pub fn track(handoff: PendingHandoff) {
    let seen_idp = Url::parse(&handoff.target_url)
        .is_ok_and(|target| is_idp_host(&target, &handoff.idp_host));
    PENDING.lock().insert(
        handoff.chromium_browser_id.clone(),
        TrackedHandoff { handoff, seen_idp },
    );
}

/// The handoff of `tab` once its address leaves the identity provider for an http(s) page,
/// returned once.
pub fn take_completed(tab: &str, url: &str) -> Option<PendingHandoff> {
    let mut pending = PENDING.lock();
    let tracked = pending.get_mut(tab)?;
    let parsed = Url::parse(url).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    if is_idp_host(&parsed, &tracked.handoff.idp_host) {
        tracked.seen_idp = true;
        return None;
    }
    if !tracked.seen_idp {
        return None;
    }
    pending.remove(tab).map(|tracked| tracked.handoff)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENGINES: [EngineAvailability; 4] = [
        EngineAvailability::Ready,
        EngineAvailability::NotInstalled,
        EngineAvailability::Unusable,
        EngineAvailability::Unsupported,
    ];
    const PLATFORMS: [HostPlatform; 3] =
        [HostPlatform::Macos, HostPlatform::Windows, HostPlatform::Linux];
    const PROVIDERS: [SignInProvider; 4] = [
        SignInProvider::Google,
        SignInProvider::Apple,
        SignInProvider::Microsoft,
        SignInProvider::Okta,
    ];
    const RETRY: &str = "https://accounts.google.com/";

    fn blocked(url: &str, title: Option<&str>) -> Option<(SignInProvider, String)> {
        match detect_sign_in_block(url, title) {
            SignInBlock::Blocked {
                provider,
                retry_url,
            } => Some((provider, retry_url)),
            SignInBlock::NotBlocked => None,
        }
    }

    fn google_retry(url: &str, title: Option<&str>) -> Option<String> {
        blocked(url, title).map(|(provider, retry_url)| {
            assert_eq!(provider, SignInProvider::Google);
            retry_url
        })
    }

    fn sample_flow() -> OAuthFlow {
        OAuthFlow {
            authorize_url: "https://accounts.google.com/o/oauth2/v2/auth?client_id=c&redirect_uri=com.example.app%3A%2Fcb&response_type=code".into(),
            callback: AuthCallback::Scheme {
                scheme: "com.example.app".into(),
            },
        }
    }

    #[test]
    fn detects_the_rejected_page_and_keeps_its_continue_target() {
        let url = "https://accounts.google.com/v3/signin/rejected?continue=https%3A%2F%2Fmail.google.com%2Fmail%2F&flowName=GlifWebSignIn";
        assert_eq!(
            google_retry(url, None).as_deref(),
            Some("https://mail.google.com/mail/")
        );
    }

    #[test]
    fn detects_the_disallowed_useragent_oauth_error() {
        let url = "https://accounts.google.com/signin/oauth/error?authError=Cg9kaXNhbGxvd2VkX3VzZXJhZ2VudA&client_id=x&error=disallowed_useragent";
        assert_eq!(
            google_retry(url, None).as_deref(),
            Some("https://accounts.google.com/")
        );
    }

    #[test]
    fn detects_the_block_by_title() {
        let url =
            "https://accounts.google.com/v3/signin/identifier?continue=https://drive.google.com/";
        assert_eq!(
            google_retry(
                url,
                Some("Couldn\u{2019}t sign you in - This browser or app may not be secure")
            )
            .as_deref(),
            Some("https://drive.google.com/")
        );
    }

    #[test]
    fn ignores_normal_sign_in_other_hosts_and_other_oauth_errors() {
        assert_eq!(
            blocked(
                "https://accounts.google.com/v3/signin/identifier",
                Some("Sign in - Google Accounts")
            ),
            None
        );
        assert_eq!(
            blocked(
                "https://evil.example/signin/rejected?error=disallowed_useragent",
                None
            ),
            None
        );
        assert_eq!(
            blocked("http://accounts.google.com/signin/rejected", None),
            None
        );
        assert_eq!(
            blocked(
                "https://accounts.google.com/signin/oauth/error?error=invalid_client",
                None
            ),
            None
        );
        assert_eq!(blocked("not a url", None), None);
    }

    #[test]
    fn a_non_https_continue_target_falls_back_to_the_sign_in_page() {
        let url = "https://accounts.google.com/signin/rejected?continue=javascript:alert(1)";
        assert_eq!(
            google_retry(url, None).as_deref(),
            Some("https://accounts.google.com/")
        );
    }

    #[test]
    fn detects_apple_microsoft_and_okta_unsupported_browser_pages() {
        let cases = [
            (
                "https://appleid.apple.com/auth/authorize?client_id=x",
                "Your browser is not supported",
                SignInProvider::Apple,
            ),
            (
                "https://idmsa.apple.com/appleauth/auth/signin",
                "Unsupported Browser",
                SignInProvider::Apple,
            ),
            (
                "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
                "Browser not supported",
                SignInProvider::Microsoft,
            ),
            (
                "https://login.live.com/login.srf",
                "Please update your browser",
                SignInProvider::Microsoft,
            ),
            (
                "https://acme.okta.com/oauth2/v1/authorize",
                "Unsupported browser",
                SignInProvider::Okta,
            ),
            (
                "https://acme.oktapreview.com/login",
                "This browser is not supported",
                SignInProvider::Okta,
            ),
        ];
        for (url, title, provider) in cases {
            assert_eq!(
                blocked(url, Some(title)),
                Some((provider, Url::parse(url).expect("case url").to_string())),
                "{url}"
            );
        }
    }

    #[test]
    fn ignores_normal_pages_non_https_and_look_alike_hosts_of_other_providers() {
        assert_eq!(
            blocked("https://appleid.apple.com/sign-in", Some("Sign in - Apple")),
            None
        );
        assert_eq!(blocked("https://appleid.apple.com/sign-in", None), None);
        assert_eq!(
            blocked("http://appleid.apple.com/", Some("Unsupported browser")),
            None
        );
        assert_eq!(
            blocked(
                "https://appleid.apple.com.evil.test/",
                Some("Unsupported browser")
            ),
            None
        );
        assert_eq!(
            blocked("https://evil-okta.com/", Some("Unsupported browser")),
            None
        );
        assert_eq!(
            blocked("https://okta.com.evil.test/", Some("Unsupported browser")),
            None
        );
        assert_eq!(
            blocked(
                "https://login.microsoftonline.com.evil.test/",
                Some("Browser not supported")
            ),
            None
        );
    }

    #[test]
    fn serializes_for_the_handoff_bar() {
        assert_eq!(
            serde_json::to_value(SignInBlock::Blocked {
                provider: SignInProvider::Google,
                retry_url: "https://a/".into()
            })
            .expect("block json"),
            serde_json::json!({ "kind": "blocked", "provider": "google", "retryUrl": "https://a/" })
        );
        assert_eq!(
            serde_json::to_value(SignInBlock::NotBlocked).expect("not blocked json"),
            serde_json::json!({ "kind": "notBlocked" })
        );
        let plan = handoff_plan(
            SignInProvider::Apple,
            "https://appleid.apple.com/",
            EngineAvailability::NotInstalled,
            HostPlatform::Macos,
            Some(&sample_flow()),
            true,
        );
        assert_eq!(
            serde_json::to_value(plan).expect("plan json"),
            serde_json::json!({
                "provider": "apple",
                "engine": "notInstalled",
                "primary": { "kind": "installEngine", "url": "https://appleid.apple.com/" },
                "alternatives": [{
                    "kind": "systemAuthSession",
                    "authorizeUrl": sample_flow().authorize_url,
                    "callback": { "kind": "scheme", "scheme": "com.example.app" }
                }],
                "onDecline": [{ "kind": "externalBrowser", "url": "https://appleid.apple.com/" }]
            })
        );
        assert_eq!(
            serde_json::to_value(AuthCallback::Https {
                host: "app.test".into(),
                path: "/cb".into()
            })
            .expect("callback json"),
            serde_json::json!({ "kind": "https", "host": "app.test", "path": "/cb" })
        );
    }

    #[test]
    fn oauth_flow_picks_the_newest_authorize_request() {
        let history = [
            "https://example.com/".to_string(),
            "https://accounts.google.com/o/oauth2/v2/auth?client_id=old&redirect_uri=https%3A%2F%2Fold.test%2Fcb&response_type=code".to_string(),
            "https://accounts.google.com/o/oauth2/v2/auth?client_id=new&redirect_uri=com.example.app%3A%2Foauth&response_type=code".to_string(),
            "https://accounts.google.com/v3/signin/identifier".to_string(),
        ];
        let flow = oauth_flow(
            "https://accounts.google.com/signin/oauth/error?error=disallowed_useragent",
            &history,
        )
        .expect("flow");
        assert_eq!(flow.authorize_url, history[2]);
        assert_eq!(
            flow.callback,
            AuthCallback::Scheme {
                scheme: "com.example.app".into()
            }
        );
    }

    #[test]
    fn oauth_flow_prefers_the_blocked_url_and_reads_https_callbacks() {
        let blocked_url = "https://login.microsoftonline.com/common/oauth2/v2.0/authorize?client_id=c&redirect_uri=https%3A%2F%2Fapp.test%2Fauth%2Fcallback&response_type=code";
        let history = [
            "https://accounts.google.com/o/oauth2/v2/auth?client_id=x&redirect_uri=com.x%3A%2Fcb&response_type=code".to_string(),
        ];
        let flow = oauth_flow(blocked_url, &history).expect("flow");
        assert_eq!(flow.authorize_url, blocked_url);
        assert_eq!(
            flow.callback,
            AuthCallback::Https {
                host: "app.test".into(),
                path: "/auth/callback".into()
            }
        );
    }

    #[test]
    fn oauth_flow_ignores_other_hosts_and_incomplete_requests() {
        let history = [
            "https://evil.test/authorize?client_id=c&redirect_uri=https%3A%2F%2Fa.test%2Fcb&response_type=code".to_string(),
            "http://accounts.google.com/o/oauth2/v2/auth?client_id=c&redirect_uri=https%3A%2F%2Fa.test%2Fcb&response_type=code".to_string(),
            "https://accounts.google.com/o/oauth2/v2/auth?client_id=c&response_type=code".to_string(),
            "https://accounts.google.com/o/oauth2/v2/auth?client_id=c&redirect_uri=https%3A%2F%2Fa.test%2Fcb".to_string(),
            "https://accounts.google.com/o/oauth2/v2/auth?client_id=c&redirect_uri=http%3A%2F%2F127.0.0.1%3A9000%2Fcb&response_type=code".to_string(),
        ];
        assert_eq!(oauth_flow("https://accounts.google.com/", &history), None);
        assert_eq!(oauth_flow("not a url", &[]), None);
    }

    #[test]
    fn engine_availability_follows_the_install_status() {
        let installed = |inactive: Option<&str>| EngineStatus::Installed {
            version: "1".into(),
            previous: None,
            inactive: inactive.map(str::to_string),
        };
        assert_eq!(
            EngineAvailability::from_status(&installed(None)),
            EngineAvailability::Ready
        );
        assert_eq!(
            EngineAvailability::from_status(&installed(Some("wayland-session"))),
            EngineAvailability::Unusable
        );
        assert_eq!(
            EngineAvailability::from_status(&EngineStatus::NotInstalled),
            EngineAvailability::NotInstalled
        );
        assert_eq!(
            EngineAvailability::from_status(&EngineStatus::Unsupported),
            EngineAvailability::Unsupported
        );
    }

    /// The decision table written out independently of `handoff_plan`.
    fn expected_plan(
        engine: EngineAvailability,
        platform: HostPlatform,
        flow: Option<&OAuthFlow>,
        auth_session: bool,
    ) -> (HandoffAction, Vec<HandoffAction>, Vec<HandoffAction>) {
        let external = HandoffAction::ExternalBrowser { url: RETRY.into() };
        let system = match (platform, auth_session, flow) {
            (HostPlatform::Macos, true, Some(flow)) => Some(HandoffAction::SystemAuthSession {
                authorize_url: flow.authorize_url.clone(),
                callback: flow.callback.clone(),
            }),
            _ => None,
        };
        let engine_action = match engine {
            EngineAvailability::Ready => Some(HandoffAction::ChromiumTab { url: RETRY.into() }),
            EngineAvailability::NotInstalled => {
                Some(HandoffAction::InstallEngine { url: RETRY.into() })
            }
            EngineAvailability::Unusable | EngineAvailability::Unsupported => None,
        };
        match (engine_action, system) {
            (Some(engine_action), Some(system)) => (engine_action, vec![system], vec![external]),
            (Some(engine_action), None) => (engine_action, vec![], vec![external]),
            (None, Some(system)) => (system, vec![], vec![external]),
            (None, None) => (external, vec![], vec![]),
        }
    }

    #[test]
    fn the_decision_table_holds_for_every_combination() {
        let sample = sample_flow();
        for engine in ENGINES {
            for platform in PLATFORMS {
                for provider in PROVIDERS {
                    for flow in [None, Some(&sample)] {
                        for auth_session in [false, true] {
                            let plan =
                                handoff_plan(provider, RETRY, engine, platform, flow, auth_session);
                            let (primary, alternatives, on_decline) =
                                expected_plan(engine, platform, flow, auth_session);
                            let case = format!(
                                "{engine:?} {platform:?} {provider:?} flow={} auth={auth_session}",
                                flow.is_some()
                            );
                            assert_eq!(plan.provider, provider, "{case}");
                            assert_eq!(plan.engine, engine, "{case}");
                            assert_eq!(plan.primary, primary, "{case}");
                            assert_eq!(plan.alternatives, alternatives, "{case}");
                            assert_eq!(plan.on_decline, on_decline, "{case}");
                            let is_external =
                                |action: &HandoffAction| matches!(action, HandoffAction::ExternalBrowser { .. });
                            if !is_external(&plan.primary) {
                                assert!(!plan.alternatives.iter().any(is_external), "{case}");
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn decision_table_spot_checks() {
        let flow = sample_flow();
        let plan = handoff_plan(
            SignInProvider::Google,
            RETRY,
            EngineAvailability::NotInstalled,
            HostPlatform::Macos,
            Some(&flow),
            true,
        );
        assert_eq!(plan.primary, HandoffAction::InstallEngine { url: RETRY.into() });
        assert_eq!(
            plan.alternatives,
            vec![HandoffAction::SystemAuthSession {
                authorize_url: flow.authorize_url.clone(),
                callback: flow.callback.clone(),
            }]
        );

        let plan = handoff_plan(
            SignInProvider::Microsoft,
            RETRY,
            EngineAvailability::Unusable,
            HostPlatform::Linux,
            Some(&flow),
            true,
        );
        assert_eq!(plan.primary, HandoffAction::ExternalBrowser { url: RETRY.into() });
        assert!(plan.alternatives.is_empty());
        assert!(plan.on_decline.is_empty());

        let plan = handoff_plan(
            SignInProvider::Okta,
            RETRY,
            EngineAvailability::NotInstalled,
            HostPlatform::Windows,
            None,
            false,
        );
        assert_eq!(plan.primary, HandoffAction::InstallEngine { url: RETRY.into() });
        assert_eq!(
            plan.on_decline,
            vec![HandoffAction::ExternalBrowser { url: RETRY.into() }]
        );

        let plan = handoff_plan(
            SignInProvider::Apple,
            RETRY,
            EngineAvailability::Unsupported,
            HostPlatform::Macos,
            Some(&flow),
            true,
        );
        assert!(matches!(plan.primary, HandoffAction::SystemAuthSession { .. }));
        assert!(plan.alternatives.is_empty());
        assert_eq!(
            plan.on_decline,
            vec![HandoffAction::ExternalBrowser { url: RETRY.into() }]
        );
    }

    #[test]
    fn host_platform_matches_the_build_target() {
        let expected = if cfg!(target_os = "macos") {
            HostPlatform::Macos
        } else if cfg!(target_os = "windows") {
            HostPlatform::Windows
        } else {
            HostPlatform::Linux
        };
        assert_eq!(host_platform(), expected);
    }

    #[test]
    fn cdp_cookies_convert_to_imported_cookies() {
        let result = serde_json::json!({
            "cookies": [
                {
                    "name": "SID", "value": "abc", "domain": ".google.com", "path": "/",
                    "expires": 1_900_000_000.5, "size": 6, "httpOnly": true, "secure": true,
                    "session": false, "sameSite": "Lax"
                },
                {
                    "name": "session", "value": "x", "domain": "app.test", "path": "/app",
                    "expires": -1, "httpOnly": false, "secure": true, "session": true,
                    "sameSite": "None"
                },
                {
                    "name": "strict", "value": "y", "domain": "app.test", "path": "",
                    "expires": 1_900_000_000, "sameSite": "Strict"
                },
                { "name": "", "value": "nameless", "domain": "app.test" },
                { "value": "no name", "domain": "app.test" }
            ]
        });
        let cookies = cdp_cookies(&result);
        assert_eq!(
            cookies,
            vec![
                ImportedCookie {
                    name: "SID".into(),
                    value: "abc".into(),
                    domain: Some(".google.com".into()),
                    path: "/".into(),
                    secure: true,
                    http_only: true,
                    expires_unix: Some(1_900_000_000),
                    same_site: Some("lax".into()),
                },
                ImportedCookie {
                    name: "session".into(),
                    value: "x".into(),
                    domain: Some("app.test".into()),
                    path: "/app".into(),
                    secure: true,
                    http_only: false,
                    expires_unix: None,
                    same_site: Some("none".into()),
                },
                ImportedCookie {
                    name: "strict".into(),
                    value: "y".into(),
                    domain: Some("app.test".into()),
                    path: "/".into(),
                    secure: false,
                    http_only: false,
                    expires_unix: Some(1_900_000_000),
                    same_site: Some("strict".into()),
                },
            ]
        );
        for cookie in cookies {
            assert!(crate::browser::cookies::cookie_from_imported(cookie).is_ok());
        }
        assert!(cdp_cookies(&serde_json::json!({})).is_empty());
    }

    fn pending(tab: &str, target_url: &str) -> PendingHandoff {
        PendingHandoff {
            chromium_browser_id: tab.into(),
            profile_id: "default".into(),
            target_url: target_url.into(),
            idp_host: "accounts.google.com".into(),
        }
    }

    #[test]
    fn a_tracked_handoff_completes_once_when_it_leaves_the_identity_provider() {
        let tab = "signin:test-leaves-idp";
        track(pending(tab, "https://accounts.google.com/v3/signin/identifier"));
        assert_eq!(
            take_completed(tab, "https://accounts.google.com/v3/signin/challenge"),
            None
        );
        assert_eq!(take_completed(tab, "about:blank"), None);
        assert_eq!(
            take_completed(tab, "https://mail.google.com/mail/"),
            Some(pending(tab, "https://accounts.google.com/v3/signin/identifier"))
        );
        assert_eq!(take_completed(tab, "https://mail.google.com/mail/"), None);
        assert_eq!(take_completed("signin:never-tracked", "https://a.test/"), None);
    }

    #[test]
    fn a_handoff_that_starts_off_the_identity_provider_waits_for_it() {
        let tab = "signin:test-waits-for-idp";
        track(pending(tab, "https://mail.google.com/mail/"));
        assert_eq!(take_completed(tab, "https://mail.google.com/mail/"), None);
        assert_eq!(
            take_completed(tab, "https://accounts.google.com/v3/signin/identifier"),
            None
        );
        assert!(take_completed(tab, "https://mail.google.com/mail/u/0/").is_some());
        assert_eq!(take_completed(tab, "https://mail.google.com/mail/u/0/"), None);
    }

    fn title_of(html: &str) -> Option<String> {
        let start = html.find("<title>")? + "<title>".len();
        let end = html[start..].find("</title>")? + start;
        Some(html[start..end].to_string())
    }

    /// A local identity provider whose authorize endpoint redirects an embedded webview to the
    /// disallowed_useragent error page, walked the way a tab follows it.
    #[tokio::test]
    async fn a_fixture_identity_provider_block_yields_in_app_handoffs() {
        use axum::http::{header, StatusCode};
        let router = axum::Router::new()
            .route(
                "/o/oauth2/v2/auth",
                axum::routing::get(|| async {
                    (
                        StatusCode::FOUND,
                        [(
                            header::LOCATION,
                            "/signin/oauth/error?authError=x&error=disallowed_useragent",
                        )],
                    )
                }),
            )
            .route(
                "/signin/oauth/error",
                axum::routing::get(|| async {
                    axum::response::Html(
                        "<html><head><title>Couldn't sign you in</title></head><body></body></html>",
                    )
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fixture");
        let address = listener.local_addr().expect("fixture address");
        let server = tokio::spawn(async move { axum::serve(listener, router).await });

        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .expect("client");
        let authorize_url = format!(
            "http://{address}/o/oauth2/v2/auth?client_id=fixture-client&redirect_uri=https%3A%2F%2Fapp.test%2Fcb&response_type=code"
        );
        let mut chain = Vec::new();
        let mut next = authorize_url.clone();
        let mut title = None;
        for _ in 0..5 {
            chain.push(next.clone());
            let response = client.get(&next).send().await.expect("fixture response");
            if response.status().is_redirection() {
                let location = response
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .expect("redirect location")
                    .to_string();
                next = Url::parse(&next)
                    .and_then(|base| base.join(&location))
                    .expect("redirect target")
                    .to_string();
                continue;
            }
            title = title_of(&response.text().await.expect("fixture body"));
            break;
        }
        server.abort();

        assert_eq!(chain.len(), 2);
        assert_eq!(title.as_deref(), Some("Couldn't sign you in"));
        let port = address.port();
        let fixture = move |url: &Url| {
            (url.host_str() == Some("127.0.0.1") && url.port() == Some(port))
                .then_some(SignInProvider::Google)
        };
        let blocked_url = chain.last().expect("blocked page").clone();
        let history = chain[..chain.len() - 1].to_vec();
        let block = classify(
            &Url::parse(&blocked_url).expect("blocked url"),
            title.as_deref(),
            &fixture,
        );
        let (provider, retry_url) = match block {
            SignInBlock::Blocked {
                provider,
                retry_url,
            } => (provider, retry_url),
            SignInBlock::NotBlocked => panic!("the fixture block was not detected"),
        };
        assert_eq!(provider, SignInProvider::Google);
        assert_eq!(retry_url, DEFAULT_SIGN_IN_URL);
        // The real host check refuses the fixture's http origin.
        assert_eq!(
            detect_sign_in_block(&blocked_url, title.as_deref()),
            SignInBlock::NotBlocked
        );

        let flow = find_oauth_flow(&blocked_url, &history, &fixture).expect("authorize request");
        assert_eq!(flow.authorize_url, authorize_url);
        assert_eq!(
            flow.callback,
            AuthCallback::Https {
                host: "app.test".into(),
                path: "/cb".into()
            }
        );

        let mac_plan = handoff_plan(
            provider,
            &retry_url,
            EngineAvailability::NotInstalled,
            HostPlatform::Macos,
            Some(&flow),
            true,
        );
        assert_eq!(
            mac_plan.primary,
            HandoffAction::InstallEngine {
                url: retry_url.clone()
            }
        );
        assert_eq!(
            mac_plan.alternatives,
            vec![HandoffAction::SystemAuthSession {
                authorize_url: authorize_url.clone(),
                callback: flow.callback.clone(),
            }]
        );
        let windows_plan = handoff_plan(
            provider,
            &retry_url,
            EngineAvailability::Ready,
            HostPlatform::Windows,
            Some(&flow),
            false,
        );
        assert_eq!(
            windows_plan.primary,
            HandoffAction::ChromiumTab {
                url: retry_url.clone()
            }
        );
        assert!(windows_plan.alternatives.is_empty());

        // A cancelled auth session leaves the tab as it was: asking again gives the same plan,
        // and no handoff is tracked for the tab.
        let again = handoff_plan(
            provider,
            &retry_url,
            EngineAvailability::NotInstalled,
            HostPlatform::Macos,
            find_oauth_flow(&blocked_url, &history, &fixture).as_ref(),
            true,
        );
        assert_eq!(again, mac_plan);
        assert_eq!(take_completed("fixture-tab", "https://app.test/cb"), None);
    }
}
