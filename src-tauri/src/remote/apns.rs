//! Native iOS push (APNs) for the Ferryx companion app.
//!
//! The Safari home-screen PWA can use Web Push, but an app installed from the App Store or
//! TestFlight runs in WKWebView, which has no `PushManager` at all, so `pushSubscription.ts`
//! reports `unsupported` there. The only way an installed app can be alerted is APNs, which is
//! what this module sends.
//!
//! A **token** (`.p8`) key is the only supported credential: it authenticates with an ES256 JWT
//! whose `kid` is the key id and whose `iss` is the ten-character team id. A certificate
//! (`.p12`) is per-app and expires, so it is deliberately not supported.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// APNs refuses a provider token older than one hour. Refresh well inside that.
const TOKEN_LIFETIME: Duration = Duration::from_secs(45 * 60);
const APNS_TIMEOUT: Duration = Duration::from_secs(15);
/// A build signed with a development profile gets a token only the sandbox host accepts, so the
/// host must follow the app's signing, never a default.
const APNS_PRODUCTION_HOST: &str = "https://api.push.apple.com";
const APNS_SANDBOX_HOST: &str = "https://api.sandbox.push.apple.com";
/// Apple keeps an undelivered notification this long at most.
const APNS_EXPIRY_SECS: u32 = 60 * 60;

#[derive(Debug, thiserror::Error)]
pub enum ApnsError {
    #[error("APNs is not configured: {0}")]
    NotConfigured(String),
    #[error("APNs credentials are invalid: {0}")]
    Credentials(String),
    #[error("APNs request failed: {0}")]
    Transport(String),
}

/// Configured by `FERRYX_APNS_KEY_FILE` (path to the `.p8`), `FERRYX_APNS_KEY_ID` and
/// `FERRYX_APNS_TEAM_ID`. All absent means APNs is off; a partial set is an error, because
/// silently sending nothing is worse than refusing.
#[derive(Clone)]
pub struct ApnsCredentials {
    key_pem: String,
    key_id: String,
    team_id: String,
}

impl std::fmt::Debug for ApnsCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The private key must never reach a log line.
        f.debug_struct("ApnsCredentials")
            .field("key_id", &self.key_id)
            .field("team_id", &self.team_id)
            .finish_non_exhaustive()
    }
}

impl ApnsCredentials {
    /// Reads the three variables. `Ok(None)` when none is set.
    pub fn from_env() -> Result<Option<Self>, ApnsError> {
        let file = std::env::var("FERRYX_APNS_KEY_FILE").ok().filter(|v| !v.trim().is_empty());
        let key_id = std::env::var("FERRYX_APNS_KEY_ID").ok().filter(|v| !v.trim().is_empty());
        let team_id = std::env::var("FERRYX_APNS_TEAM_ID").ok().filter(|v| !v.trim().is_empty());
        match (file, key_id, team_id) {
            (None, None, None) => Ok(None),
            (Some(file), Some(key_id), Some(team_id)) => {
                Self::from_parts(PathBuf::from(file), key_id, team_id).map(Some)
            }
            _ => Err(ApnsError::NotConfigured(
                "FERRYX_APNS_KEY_FILE, FERRYX_APNS_KEY_ID and FERRYX_APNS_TEAM_ID must be set together".into(),
            )),
        }
    }

    /// Test-only: credentials from a PEM already in memory, so a test needs no `.p8` on disk.
    #[cfg(test)]
    pub(crate) fn for_test(key_pem: String, key_id: &str, team_id: &str) -> Self {
        Self { key_pem, key_id: key_id.to_string(), team_id: team_id.to_string() }
    }

    /// Reads the key at `path`. Call from a blocking worker: it touches disk.
    pub fn from_parts(path: PathBuf, key_id: String, team_id: String) -> Result<Self, ApnsError> {
        let key_pem = std::fs::read_to_string(&path).map_err(|error| {
            ApnsError::NotConfigured(format!("APNs key {} is unreadable: {error}", path.display()))
        })?;
        if !key_pem.contains("PRIVATE KEY") {
            return Err(ApnsError::Credentials("the APNs key file is not a PKCS#8 private key".into()));
        }
        // Both ids are ten alphanumeric characters; Apple answers anything else with an
        // uninformative 403, so reject it locally.
        for (label, value) in [("key id", &key_id), ("team id", &team_id)] {
            if value.len() != 10 || !value.chars().all(|c| c.is_ascii_alphanumeric()) {
                return Err(ApnsError::Credentials(format!(
                    "the APNs {label} must be ten alphanumeric characters"
                )));
            }
        }
        Ok(Self { key_pem, key_id, team_id })
    }

    /// The ES256 provider token: `{"alg":"ES256","kid":<key id>}` and
    /// `{"iss":<team id>,"iat":<now>}`, signed with the `.p8`.
    pub fn provider_token(&self, now: u64) -> Result<String, ApnsError> {
        use jwt_simple::algorithms::ES256KeyPair;
        use jwt_simple::claims::{JWTClaims, NoCustomClaims};
        use jwt_simple::prelude::*;
        let key = ES256KeyPair::from_pem(&self.key_pem)
            .map_err(|error| ApnsError::Credentials(format!("the APNs key was rejected: {error}")))?;
        // `jwt-simple` takes the key id on the key and writes it as the header's `kid`.
        let key = key.with_key_id(&self.key_id);
        let claims = JWTClaims::<NoCustomClaims> {
            // `coarsetime::Duration::from(u64)` stores raw fixed-point bits, so `as_secs`
            // would return 0 and APNs would reject the token; `from_secs` scales properly.
            issued_at: Some(UnixTimeStamp::from_secs(now)),
            expires_at: None,
            invalid_before: None,
            issuer: Some(self.team_id.clone()),
            subject: None,
            audiences: None,
            jwt_id: None,
            nonce: None,
            custom: NoCustomClaims {},
        };
        key.sign(claims)
            .map_err(|error| ApnsError::Credentials(format!("the APNs token was not signed: {error}")))
    }
}

/// The provider token, refreshed before it expires.
pub struct ApnsTokenCache {
    credentials: ApnsCredentials,
    token: tokio::sync::Mutex<Option<(String, SystemTime)>>,
}

impl ApnsTokenCache {
    pub fn new(credentials: ApnsCredentials) -> Self {
        Self { credentials, token: tokio::sync::Mutex::new(None) }
    }

    /// A token valid for at least the next request, reusing the cached one while it is young.
    pub async fn token(&self) -> Result<String, ApnsError> {
        let mut slot = self.token.lock().await;
        let now = SystemTime::now();
        if let Some((token, issued)) = slot.as_ref() {
            if now.duration_since(*issued).unwrap_or(TOKEN_LIFETIME) < TOKEN_LIFETIME {
                return Ok(token.clone());
            }
        }
        let seconds = now.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let token = self.credentials.provider_token(seconds)?;
        *slot = Some((token.clone(), now));
        Ok(token)
    }

    /// Drops the cached token so the next request signs a fresh one.
    ///
    /// APNs answers `403 ExpiredProviderToken` when it rejects a token this cache still considers
    /// fresh (a skewed clock, or a key that was rotated out from under it). Without this the cache
    /// keeps handing out the rejected token, so one 403 silences push for the rest of the
    /// 45-minute window.
    pub async fn invalidate(&self) {
        *self.token.lock().await = None;
    }
}

/// Where the app's signing put it: a development build's token is only valid on the sandbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ApnsEnvironment {
    #[default]
    Production,
    Sandbox,
}

impl ApnsEnvironment {
    fn host(self) -> &'static str {
        match self {
            Self::Production => APNS_PRODUCTION_HOST,
            Self::Sandbox => APNS_SANDBOX_HOST,
        }
    }
}

/// A registered iOS device: the APNs device token, never a Web Push endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApnsSubscription {
    pub device_id: String,
    /// Hex, as APNs prints it; the path component of the request.
    pub token: String,
    #[serde(default)]
    pub environment: ApnsEnvironment,
    #[serde(default = "default_true")]
    pub needs_input: bool,
}

fn default_true() -> bool {
    true
}

impl ApnsSubscription {
    /// A device token is 32 bytes of hex. Anything else is a client bug, not a delivery failure.
    pub fn valid_token(token: &str) -> bool {
        token.len() == 64 && token.chars().all(|c| c.is_ascii_hexdigit())
    }
}

/// How one APNs send ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApnsOutcome {
    Delivered,
    /// 410, or 400 `BadDeviceToken`/`Unregistered`: the token is dead and must be dropped.
    Gone,
    Failed(String),
}

/// `url` is the same deep link the Web Push payload carries, so one client path serves both.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApnsPayload {
    pub aps: Aps,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// The backend session id, so a tap can target the exact card.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Aps {
    pub alert: ApsAlert,
    pub sound: String,
    /// Collapse several decisions for one session into the newest.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApsAlert {
    pub title: String,
    pub body: String,
}

impl ApnsPayload {
    pub fn decision(title: &str, body: &str, url: Option<String>, agent_id: Option<String>) -> Self {
        Self {
            aps: Aps {
                alert: ApsAlert { title: title.to_string(), body: body.to_string() },
                sound: "default".to_string(),
                thread_id: agent_id.clone(),
            },
            url,
            agent_id,
        }
    }

    pub fn to_json(&self) -> Result<String, ApnsError> {
        serde_json::to_string(self).map_err(|error| ApnsError::Transport(error.to_string()))
    }
}

/// Sends to APNs through the shared client. One instance per gateway.
pub struct ApnsSender {
    tokens: ApnsTokenCache,
    client: reqwest::Client,
    /// The app's bundle id, which APNs requires as `apns-topic`.
    topic: String,
    /// Test-only: a local mock stands in for Apple, so a test can prove the send path without
    /// reaching the network. Production always posts to [`ApnsEnvironment::host`].
    #[cfg(test)]
    base_url: Option<String>,
}

impl ApnsSender {
    pub fn new(credentials: ApnsCredentials, topic: String, client: reqwest::Client) -> Self {
        Self {
            tokens: ApnsTokenCache::new(credentials),
            client,
            topic,
            #[cfg(test)]
            base_url: None,
        }
    }

    /// Test-only: a sender that posts to a local mock instead of Apple's host.
    #[cfg(test)]
    pub(crate) fn for_test(
        credentials: ApnsCredentials,
        topic: String,
        client: reqwest::Client,
        base_url: String,
    ) -> Self {
        Self {
            tokens: ApnsTokenCache::new(credentials),
            client,
            topic,
            base_url: Some(base_url),
        }
    }

    /// Where this send goes: Apple's host for the app's environment, or the test mock.
    fn host_for(&self, environment: ApnsEnvironment) -> &str {
        #[cfg(test)]
        if let Some(base) = self.base_url.as_deref() {
            return base;
        }
        environment.host()
    }

    /// The sender for `FERRYX_APNS_*`, or `None` when APNs is unconfigured. Reads the key file,
    /// so call it from a blocking worker.
    pub fn from_env(client: reqwest::Client) -> Result<Option<Self>, ApnsError> {
        let Some(credentials) = ApnsCredentials::from_env()? else {
            return Ok(None);
        };
        let topic = std::env::var("FERRYX_APNS_TOPIC")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "com.ferryx.mobile".to_string());
        Ok(Some(Self::new(credentials, topic, client)))
    }

    /// Posts one notification, mapping Apple's refusals onto the store's lifecycle.
    pub async fn send(&self, sub: &ApnsSubscription, payload: &ApnsPayload) -> ApnsOutcome {
        if !ApnsSubscription::valid_token(&sub.token) {
            return ApnsOutcome::Gone;
        }
        let token = match self.tokens.token().await {
            Ok(token) => token,
            Err(error) => return ApnsOutcome::Failed(error.to_string()),
        };
        let body = match payload.to_json() {
            Ok(body) => body,
            Err(error) => return ApnsOutcome::Failed(error.to_string()),
        };
        let url = format!("{}/3/device/{}", self.host_for(sub.environment), sub.token);
        let request = self
            .client
            .post(&url)
            .timeout(APNS_TIMEOUT)
            .header("authorization", format!("bearer {token}"))
            .header("apns-topic", &self.topic)
            .header("apns-push-type", "alert")
            .header("apns-priority", "10")
            .header("apns-expiration", apns_expiration())
            .header("content-type", "application/json")
            .body(body);
        match request.send().await {
            Ok(response) => {
                let status = response.status().as_u16();
                if (200..300).contains(&status) {
                    // The delivery receipt: Apple's `apns-id` ties this send to its own logs.
                    let apns_id = response
                        .headers()
                        .get("apns-id")
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or("");
                    tracing::info!(
                        status,
                        apns_id,
                        token = %redact_token(&sub.token),
                        "APNs accepted the push"
                    );
                    return ApnsOutcome::Delivered;
                }
                let reason = response
                    .json::<ApnsErrorBody>()
                    .await
                    .map(|body| body.reason)
                    .unwrap_or_else(|_| String::new());
                // 410 Unregistered and 400 BadDeviceToken both mean the token is dead, never "retry".
                if status == 410 || reason == "Unregistered" || reason == "BadDeviceToken" {
                    ApnsOutcome::Gone
                } else if reason == "ExpiredProviderToken" || reason == "InvalidProviderToken" {
                    // The provider token itself was rejected, so drop it: otherwise this cache keeps
                    // serving the rejected token and every send fails until it ages out.
                    self.tokens.invalidate().await;
                    ApnsOutcome::Failed(format!("APNs rejected the provider token: {reason}"))
                } else {
                    ApnsOutcome::Failed(if reason.is_empty() {
                        format!("APNs answered {status}")
                    } else {
                        format!("APNs answered {status}: {reason}")
                    })
                }
            }
            Err(error) => ApnsOutcome::Failed(send_error_message(error)),
        }
    }
}

/// The log line for a failed send. `reqwest::Error`'s `Display` appends `for url (...)`, and this
/// request's url ends in the device token, so the url is stripped before the error is formatted.
fn send_error_message(error: reqwest::Error) -> String {
    format!("APNs request failed: {}", error.without_url())
}

#[derive(Debug, Deserialize)]
struct ApnsErrorBody {
    reason: String,
}

/// A device token as it may appear in a log: the first and last four characters only.
pub fn redact_token(token: &str) -> String {
    let chars: Vec<char> = token.chars().collect();
    if chars.len() <= 8 {
        return "*".repeat(chars.len());
    }
    let head: String = chars[..4].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}...{tail}")
}

fn apns_expiration() -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    (now + u64::from(APNS_EXPIRY_SECS)).to_string()
}

/// Shared handle: the gateway holds one, and fan-out clones the `Arc`.
pub type SharedApns = Arc<ApnsSender>;

#[cfg(test)]
mod tests {
    use super::*;

    const TEAM: &str = "5DUM8WPB4C";
    const KEY_ID: &str = "HLC2K69Y8G";

    /// A throwaway P-256 key in PKCS#8 PEM, the shape Apple's `.p8` has.
    fn test_key_pem() -> String {
        use jwt_simple::algorithms::ES256KeyPair;
        ES256KeyPair::generate().to_pem().unwrap()
    }

    fn credentials() -> ApnsCredentials {
        ApnsCredentials { key_pem: test_key_pem(), key_id: KEY_ID.into(), team_id: TEAM.into() }
    }

    #[test]
    fn provider_token_carries_the_key_id_and_team_and_verifies() {
        use jwt_simple::algorithms::ES256PublicKey;
        use jwt_simple::prelude::*;

        let credentials = credentials();
        let token = credentials.provider_token(1_700_000_000).unwrap();
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3, "a JWT has three parts");

        let header: serde_json::Value =
            serde_json::from_slice(&base64_url_decode(parts[0])).unwrap();
        assert_eq!(header["alg"], "ES256");
        assert_eq!(header["kid"], KEY_ID, "APNs finds the key by kid");

        let claims: serde_json::Value =
            serde_json::from_slice(&base64_url_decode(parts[1])).unwrap();
        assert_eq!(claims["iss"], TEAM, "APNs checks iss against the team");
        assert_eq!(claims["iat"], 1_700_000_000u64);

        // A token that parses but does not verify is exactly what APNs answers 403 to.
        let pair = ES256KeyPair::from_pem(&credentials.key_pem).unwrap();
        let public = ES256PublicKey::from_bytes(&pair.public_key().to_bytes()).unwrap();
        let verified = public.verify_token::<NoCustomClaims>(&token, None);
        assert!(verified.is_ok(), "the provider token must verify: {verified:?}");
    }

    #[test]
    fn malformed_ids_are_refused_before_any_request() {
        let path = std::env::temp_dir().join(format!("ferryx-apns-{}.p8", std::process::id()));
        std::fs::write(&path, test_key_pem()).unwrap();
        for (key_id, team) in [
            ("short".to_string(), TEAM.to_string()),
            (KEY_ID.to_string(), "not-ten!!".to_string()),
            (String::new(), TEAM.to_string()),
            (KEY_ID.to_string(), String::new()),
        ] {
            let error = ApnsCredentials::from_parts(path.clone(), key_id, team).unwrap_err();
            assert!(error.to_string().contains("ten alphanumeric"), "{error}");
        }
        assert!(ApnsCredentials::from_parts(path.clone(), KEY_ID.into(), TEAM.into()).is_ok());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_key_file_without_pem_is_refused() {
        let path = std::env::temp_dir().join(format!("ferryx-apns-bad-{}.p8", std::process::id()));
        std::fs::write(&path, "not a key").unwrap();
        let error = ApnsCredentials::from_parts(path.clone(), KEY_ID.into(), TEAM.into()).unwrap_err();
        assert!(error.to_string().contains("PKCS#8"), "{error}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn only_a_32_byte_hex_token_is_accepted() {
        assert!(ApnsSubscription::valid_token(&"ab".repeat(32)));
        assert!(!ApnsSubscription::valid_token(""));
        assert!(!ApnsSubscription::valid_token(&"ab".repeat(31)));
        assert!(!ApnsSubscription::valid_token(&"zz".repeat(32)));
    }

    #[test]
    fn the_payload_matches_the_shape_the_app_reads() {
        let payload = ApnsPayload::decision(
            "Decision needed",
            "claude in ferryx/pane",
            Some("/#/supervisor?session=s1".into()),
            Some("s1".into()),
        );
        let json: serde_json::Value = serde_json::from_str(&payload.to_json().unwrap()).unwrap();
        assert_eq!(json["aps"]["alert"]["title"], "Decision needed");
        assert_eq!(json["aps"]["alert"]["body"], "claude in ferryx/pane");
        assert_eq!(json["aps"]["sound"], "default");
        assert_eq!(json["url"], "/#/supervisor?session=s1");
        assert_eq!(json["agent_id"], "s1");
    }

    #[test]
    fn a_logged_token_keeps_only_its_first_and_last_four_characters() {
        let token = format!("abcd{}wxyz", "0".repeat(56));
        let redacted = redact_token(&token);
        assert_eq!(redacted, "abcd...wxyz");
        assert!(!redacted.contains(&token[4..60]), "the middle of the token never reaches a log");
        // A value too short to redact by halves is masked whole rather than printed.
        assert_eq!(redact_token("abcd1234"), "********");
        assert_eq!(redact_token(""), "");
    }

    #[tokio::test]
    async fn a_failed_send_message_does_not_carry_the_device_token() {
        let token = "ab".repeat(32);
        // A scheme reqwest refuses fails inside the client, before any connection is attempted, so
        // this test does no I/O and does not depend on timing or on a port being closed.
        let url = format!("ftp://127.0.0.1/3/device/{token}");
        let client = reqwest::Client::new();
        let error = client
            .post(&url)
            .send()
            .await
            .expect_err("reqwest only speaks http and https");

        // The premise: formatting the error as-is does leak the token. If a reqwest upgrade ever
        // stops naming the url this assert fails, so the guard below cannot silently go vacuous.
        let raw = format!("APNs request failed: {error}");
        let message = send_error_message(error);
        assert!(raw.contains(&token), "reqwest names the request url: {raw}");
        assert!(!message.contains(&token), "a send error must not name the token: {message}");
    }

    #[test]
    fn sandbox_and_production_are_different_hosts() {
        assert_ne!(ApnsEnvironment::Production.host(), ApnsEnvironment::Sandbox.host());
        assert!(ApnsEnvironment::Sandbox.host().contains("sandbox"));
        // A development build's token is only valid on the sandbox; the default must be
        // production so an unset field never silently misroutes a release build's token.
        assert_eq!(ApnsEnvironment::default(), ApnsEnvironment::Production);
    }

    #[tokio::test]
    async fn a_cached_token_is_not_re_signed() {
        // `iat` is whole seconds, so two signatures in the same second would compare equal and a
        // plain equality check could never fail. Seed the slot with a token that is not a JWT:
        // reuse must hand that exact string back, while a re-sign would mint a real one.
        let cache = ApnsTokenCache::new(credentials());
        {
            let mut slot = cache.token.lock().await;
            *slot = Some(("cached-token".to_string(), SystemTime::now()));
        }
        assert_eq!(cache.token().await.unwrap(), "cached-token");
    }

    #[tokio::test]
    async fn an_expired_cached_token_is_re_signed() {
        let cache = ApnsTokenCache::new(credentials());
        {
            let mut slot = cache.token.lock().await;
            let stale = SystemTime::now() - TOKEN_LIFETIME - Duration::from_secs(1);
            *slot = Some(("stale-token".to_string(), stale));
        }
        let fresh = cache.token().await.unwrap();
        assert_ne!(fresh, "stale-token", "an expired token must be replaced");
        assert_eq!(fresh.split('.').count(), 3, "the replacement is a signed JWT");
    }

    // APNs rejects a token it will not honour with `ExpiredProviderToken`. The cache still thinks
    // that token is young, so without invalidation every later send reuses the rejected one and
    // push stays dead for the rest of the window. This is the test that fails if `invalidate` is
    // reverted, which is the whole reason it exists.
    #[tokio::test]
    async fn invalidating_a_rejected_token_forces_a_re_sign() {
        let cache = ApnsTokenCache::new(credentials());
        {
            let mut slot = cache.token.lock().await;
            // Fresh enough to be reused: only invalidation can displace it.
            *slot = Some(("rejected-token".to_string(), SystemTime::now()));
        }
        assert_eq!(cache.token().await.unwrap(), "rejected-token", "it starts out cached");

        cache.invalidate().await;

        let fresh = cache.token().await.unwrap();
        assert_ne!(fresh, "rejected-token", "a rejected token must not be served again");
        assert_eq!(fresh.split('.').count(), 3, "the replacement is a signed JWT");
    }

    fn base64_url_decode(input: &str) -> Vec<u8> {
        use base64::Engine;
        base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(input).unwrap()
    }
}
