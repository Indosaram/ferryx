//! Phone push: VAPID keys, subscriptions persisted per paired device, and the sender.
//!
//! The payload names the agent, its worktree and the state. Terminal content never enters it.
//! Sending goes through the async reqwest client; key and store disk I/O runs on blocking
//! workers.

use crate::agent_control::AgentControlState;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

/// Stored next to the auth store.
pub const PUSH_SUBSCRIPTIONS_FILE: &str = "push-subscriptions.json";
pub const PUSH_VAPID_FILE: &str = "push-vapid.json";
/// A turn at least this long counts as long for `FinishedMode::LongTurns`.
pub const LONG_TURN: Duration = Duration::from_secs(60);
const SEND_TIMEOUT: Duration = Duration::from_secs(15);
/// Seconds a push service keeps an undelivered message.
const PUSH_TTL_SECS: u32 = 60 * 60;
/// VAPID `sub` contact claim. Apple's push service rejects a JWT without one.
const VAPID_SUBJECT: &str = "https://ferryx.app";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PushSubscriptionKeys {
    pub p256dh: String,
    pub auth: String,
}

/// The push payload, the shape the phone's service worker reads. `agent_id` is the backend
/// session id (the notification tag and the session opened on tap). Nothing from the terminal
/// screen is carried: only the agent name, the worktree name and the state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStateNotification {
    pub agent_id: String,
    pub agent_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    pub state: String, // "waiting_input", "finished" or "test"
    pub title: String,
    pub body: String,
}

pub fn format_agent_push_payload(notif: &AgentStateNotification) -> Result<String, String> {
    serde_json::to_string(notif).map_err(|e| e.to_string())
}

/// When a finished turn is pushed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FinishedMode {
    Off,
    #[default]
    LongTurns,
    EveryTurn,
}

impl FinishedMode {
    pub fn allows(self, turn: Duration) -> bool {
        match self {
            FinishedMode::Off => false,
            FinishedMode::LongTurns => turn >= LONG_TURN,
            FinishedMode::EveryTurn => true,
        }
    }
}

fn default_true() -> bool {
    true
}

/// Body of `POST /api/push/subscribe`: the browser subscription plus the alert preferences.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PushSubscribeRequest {
    pub endpoint: String,
    pub keys: PushSubscriptionKeys,
    #[serde(default = "default_true")]
    pub phone_needs_input: bool,
    #[serde(default)]
    pub phone_finished_mode: FinishedMode,
}

/// Body of `POST /api/push/preferences`. Absent fields keep their stored value.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushPreferences {
    #[serde(default)]
    pub phone_needs_input: Option<bool>,
    #[serde(default)]
    pub phone_finished_mode: Option<FinishedMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StoredSubscription {
    pub device_id: String,
    pub endpoint: String,
    pub keys: PushSubscriptionKeys,
    pub needs_input: bool,
    pub finished_mode: FinishedMode,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct PersistedPushStore {
    #[serde(default)]
    subscriptions: Vec<StoredSubscription>,
}

#[derive(Debug, thiserror::Error)]
pub enum PushStoreError {
    #[error("endpoint is not an accepted push service URL")]
    InvalidEndpoint,
    #[error("endpoint belongs to another device")]
    ForeignEndpoint,
    #[error("push store write failed: {0}")]
    Storage(#[from] std::io::Error),
}

/// Subscriptions keyed by endpoint, each owned by one paired device id.
#[derive(Debug, Default)]
pub struct PushStore {
    subscriptions: Mutex<HashMap<String, StoredSubscription>>,
    path: Option<PathBuf>,
    allow_loopback: std::sync::atomic::AtomicBool,
}

impl PushStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads the store at `path`; an unreadable or corrupt file starts empty and is logged.
    pub fn load(path: Option<PathBuf>) -> Self {
        let persisted = path.as_deref().and_then(|path| match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice::<PersistedPushStore>(&bytes)
                .map_err(|error| tracing::warn!("ignoring corrupt push store: {error}"))
                .ok(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                tracing::warn!("push store unreadable: {error}");
                None
            }
        });
        let subscriptions = persisted
            .map(|store| store.subscriptions)
            .unwrap_or_default()
            .into_iter()
            .map(|sub| (sub.endpoint.clone(), sub))
            .collect();
        Self {
            subscriptions: Mutex::new(subscriptions),
            path,
            allow_loopback: std::sync::atomic::AtomicBool::new(loopback_endpoints_allowed()),
        }
    }

    /// Lets tests subscribe a plain-http loopback endpoint: a local mock push service.
    #[cfg(test)]
    pub(crate) fn allow_loopback_endpoints(&self) {
        self.allow_loopback
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    fn persist(&self, subs: &HashMap<String, StoredSubscription>) -> std::io::Result<()> {
        let Some(path) = self.path.as_deref() else {
            return Ok(());
        };
        let mut subscriptions: Vec<_> = subs.values().cloned().collect();
        subscriptions.sort_by(|a, b| a.endpoint.cmp(&b.endpoint));
        crate::remote::auth::write_private_json(path, &PersistedPushStore { subscriptions })
    }

    /// Mutates under the lock and persists; a failed write rolls the change back.
    fn mutate<T>(
        &self,
        change: impl FnOnce(&mut HashMap<String, StoredSubscription>) -> Result<T, PushStoreError>,
    ) -> Result<T, PushStoreError> {
        let mut subs = self.subscriptions.lock();
        let before = subs.clone();
        let result = change(&mut subs)?;
        if *subs != before {
            if let Err(error) = self.persist(&subs) {
                *subs = before;
                return Err(error.into());
            }
        }
        Ok(result)
    }

    pub fn subscribe(&self, device_id: &str, request: PushSubscribeRequest) -> Result<(), PushStoreError> {
        validate_endpoint(
            &request.endpoint,
            self.allow_loopback.load(std::sync::atomic::Ordering::SeqCst),
        )?;
        self.mutate(|subs| {
            if subs
                .get(&request.endpoint)
                .is_some_and(|stored| stored.device_id != device_id)
            {
                return Err(PushStoreError::ForeignEndpoint);
            }
            subs.insert(
                request.endpoint.clone(),
                StoredSubscription {
                    device_id: device_id.to_string(),
                    endpoint: request.endpoint,
                    keys: request.keys,
                    needs_input: request.phone_needs_input,
                    finished_mode: request.phone_finished_mode,
                },
            );
            Ok(())
        })
    }

    pub fn unsubscribe(&self, device_id: &str, endpoint: &str) -> Result<(), PushStoreError> {
        self.mutate(|subs| {
            if subs.get(endpoint).is_some_and(|stored| stored.device_id != device_id) {
                return Err(PushStoreError::ForeignEndpoint);
            }
            subs.remove(endpoint);
            Ok(())
        })
    }

    /// Applies preferences to every subscription the device owns; returns how many it has.
    pub fn set_preferences(&self, device_id: &str, prefs: &PushPreferences) -> Result<usize, PushStoreError> {
        self.mutate(|subs| {
            let mut count = 0;
            for stored in subs.values_mut().filter(|stored| stored.device_id == device_id) {
                if let Some(needs_input) = prefs.phone_needs_input {
                    stored.needs_input = needs_input;
                }
                if let Some(mode) = prefs.phone_finished_mode {
                    stored.finished_mode = mode;
                }
                count += 1;
            }
            Ok(count)
        })
    }

    pub fn revoke_device(&self, device_id: &str) -> Result<(), PushStoreError> {
        self.mutate(|subs| {
            subs.retain(|_, stored| stored.device_id != device_id);
            Ok(())
        })
    }

    /// Drops every subscription whose device `is_live` rejects (unpaired, revoked or expired).
    pub fn retain_devices(&self, is_live: impl Fn(&str) -> bool) -> Result<(), PushStoreError> {
        self.mutate(|subs| {
            subs.retain(|_, stored| is_live(&stored.device_id));
            Ok(())
        })
    }

    /// Drops a subscription the push service reported gone (404/410).
    pub fn remove_gone(&self, endpoint: &str) -> Result<(), PushStoreError> {
        self.mutate(|subs| {
            subs.remove(endpoint);
            Ok(())
        })
    }

    pub fn list_subscriptions(&self) -> Vec<StoredSubscription> {
        let mut subs: Vec<_> = self.subscriptions.lock().values().cloned().collect();
        subs.sort_by(|a, b| a.endpoint.cmp(&b.endpoint));
        subs
    }

    pub fn for_device(&self, device_id: &str) -> Vec<StoredSubscription> {
        self.list_subscriptions()
            .into_iter()
            .filter(|stored| stored.device_id == device_id)
            .collect()
    }
}

/// `FERRYX_PUSH_ALLOW_LOOPBACK_ENDPOINT=1`, honored only in debug builds.
fn loopback_endpoints_allowed() -> bool {
    let set = std::env::var("FERRYX_PUSH_ALLOW_LOOPBACK_ENDPOINT")
        .is_ok_and(|value| value == "1" || value.eq_ignore_ascii_case("true"));
    crate::remote::security_settings::insecure_env_effective(set, cfg!(debug_assertions), || false)
}

/// A push endpoint must be an https URL on a known browser push service, with no credentials,
/// fragment or custom port, so a paired device cannot point the daemon at internal services.
pub fn validate_endpoint(endpoint: &str, allow_loopback: bool) -> Result<url::Url, PushStoreError> {
    let url = url::Url::parse(endpoint).map_err(|_| PushStoreError::InvalidEndpoint)?;
    if endpoint.len() > 2048 || !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
        return Err(PushStoreError::InvalidEndpoint);
    }
    let loopback_mock = allow_loopback
        && url.scheme() == "http"
        && matches!(url.host(), Some(url::Host::Ipv4(ip)) if ip.is_loopback());
    if loopback_mock {
        return Ok(url);
    }
    let known_service = match url.host() {
        Some(url::Host::Domain(host)) => {
            let host = host.to_ascii_lowercase();
            PUSH_SERVICE_HOSTS
                .iter()
                .any(|service| host == *service || host.ends_with(&format!(".{service}")))
        }
        _ => false,
    };
    if url.scheme() != "https" || !known_service || url.port().is_some_and(|port| port != 443) {
        return Err(PushStoreError::InvalidEndpoint);
    }
    Ok(url)
}

/// Browser push services. An allowlist, not "any public name": a name can resolve to an
/// internal address, and the daemon must never be a request proxy for a paired device.
const PUSH_SERVICE_HOSTS: &[&str] = &[
    "fcm.googleapis.com",
    "push.apple.com",
    "push.services.mozilla.com",
    "notify.windows.com",
];

/// The VAPID private key, raw P-256 scalar in unpadded base64url, the form web-push expects.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VapidKeys {
    private_key: String,
    public_key: String,
}

impl VapidKeys {
    pub fn generate() -> Self {
        use base64::Engine;
        use jwt_simple::prelude::*;
        let pair = ES256KeyPair::generate();
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        Self {
            private_key: b64.encode(pair.to_bytes()),
            public_key: b64.encode(pair.public_key().public_key().to_bytes_uncompressed()),
        }
    }

    /// Reads the key pair at `path`, or creates it there with owner-only permissions.
    pub fn load_or_create(path: Option<&std::path::Path>) -> std::io::Result<Self> {
        let Some(path) = path else {
            return Ok(Self::generate());
        };
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let keys = Self::generate();
                crate::remote::auth::write_private_json(path, &keys)?;
                Ok(keys)
            }
            Err(error) => Err(error),
        }
    }

    /// Unpadded base64url uncompressed point: the `applicationServerKey` a browser subscribes with.
    pub fn public_key(&self) -> &str {
        &self.public_key
    }
}

/// How one send ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendOutcome {
    Delivered,
    /// 404 or 410: the subscription is dead and has been deleted.
    Gone,
    Failed(String),
}

/// Keys, subscriptions and the async HTTP client.
pub struct PushService {
    pub store: PushStore,
    vapid_path: Option<PathBuf>,
    vapid: OnceLock<VapidKeys>,
    client: reqwest::Client,
}

impl PushService {
    /// `dir` is the auth store directory; `None` keeps everything in memory.
    pub fn new(dir: Option<&std::path::Path>) -> Self {
        Self::with_store(
            PushStore::load(dir.map(|dir| dir.join(PUSH_SUBSCRIPTIONS_FILE))),
            dir.map(|dir| dir.join(PUSH_VAPID_FILE)),
        )
    }

    pub fn with_store(store: PushStore, vapid_path: Option<PathBuf>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(SEND_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap_or_else(|error| {
                tracing::warn!("push client builder failed, using defaults: {error}");
                reqwest::Client::new()
            });
        Self { store, vapid_path, vapid: OnceLock::new(), client }
    }

    /// The key pair, created on first use. Call from a blocking worker: it may touch disk.
    pub fn vapid_blocking(&self) -> std::io::Result<VapidKeys> {
        if let Some(keys) = self.vapid.get() {
            return Ok(keys.clone());
        }
        let keys = VapidKeys::load_or_create(self.vapid_path.as_deref())?;
        Ok(self.vapid.get_or_init(|| keys).clone())
    }

    pub async fn vapid(self: &Arc<Self>) -> Result<VapidKeys, String> {
        if let Some(keys) = self.vapid.get() {
            return Ok(keys.clone());
        }
        let service = Arc::clone(self);
        crate::ipc::run_blocking(move || {
            service
                .vapid_blocking()
                .map_err(|error| crate::ipc::IpcError::internal(format!("VAPID key unavailable: {error}")))
        })
        .await
        .map_err(|error| error.message)
    }

    /// Encrypts and posts one message, deleting the subscription on 404/410.
    pub async fn send(self: &Arc<Self>, sub: &StoredSubscription, notif: &AgentStateNotification) -> SendOutcome {
        let outcome = match self.vapid().await {
            Ok(vapid) => match encrypt_request(&vapid, sub, notif) {
                Ok(request) => self.post(request).await,
                Err(error) => SendOutcome::Failed(error),
            },
            Err(error) => SendOutcome::Failed(error),
        };
        if outcome == SendOutcome::Gone {
            let service = Arc::clone(self);
            let endpoint = sub.endpoint.clone();
            let removed = crate::ipc::run_blocking(move || {
                service
                    .store
                    .remove_gone(&endpoint)
                    .map_err(crate::ipc::IpcError::internal)
            })
            .await;
            if let Err(error) = removed {
                tracing::warn!("dropping a gone push subscription failed: {}", error.message);
            }
        }
        outcome
    }

    async fn post(&self, request: EncryptedRequest) -> SendOutcome {
        let mut builder = self.client.post(request.endpoint);
        for (name, value) in request.headers {
            builder = builder.header(name, value);
        }
        match builder.body(request.body).send().await {
            Ok(response) => match response.status().as_u16() {
                200..=299 => SendOutcome::Delivered,
                404 | 410 => SendOutcome::Gone,
                status => SendOutcome::Failed(format!("push service answered {status}")),
            },
            Err(error) => SendOutcome::Failed(format!("push request failed: {error}")),
        }
    }
}

/// The request web-push built, detached from its `http` crate version.
struct EncryptedRequest {
    endpoint: String,
    headers: Vec<(String, Vec<u8>)>,
    body: Vec<u8>,
}

/// The aes128gcm-encrypted, VAPID-signed request web-push builds for this subscription.
fn encrypt_request(
    vapid: &VapidKeys,
    sub: &StoredSubscription,
    notif: &AgentStateNotification,
) -> Result<EncryptedRequest, String> {
    let info = web_push::SubscriptionInfo::new(
        sub.endpoint.as_str(),
        sub.keys.p256dh.as_str(),
        sub.keys.auth.as_str(),
    );
    let payload = format_agent_push_payload(notif)?;
    let signature = web_push::VapidSignatureBuilder::from_base64(&vapid.private_key, &info)
        .and_then(|mut builder| {
            builder.add_claim("sub", VAPID_SUBJECT);
            builder.build()
        })
        .map_err(|error| format!("VAPID signature failed: {error}"))?;
    let mut message = web_push::WebPushMessageBuilder::new(&info);
    message.set_ttl(PUSH_TTL_SECS);
    message.set_vapid_signature(signature);
    message.set_payload(web_push::ContentEncoding::Aes128Gcm, payload.as_bytes());
    let message = message
        .build()
        .map_err(|error| format!("push encryption failed: {error}"))?;
    let (parts, body) = web_push::request_builder::build_request::<Vec<u8>>(message).into_parts();
    Ok(EncryptedRequest {
        endpoint: parts.uri.to_string(),
        headers: parts
            .headers
            .iter()
            // reqwest sets the length of the body it sends itself.
            .filter(|(name, _)| name.as_str() != "content-length")
            .map(|(name, value)| (name.as_str().to_string(), value.as_bytes().to_vec()))
            .collect(),
        body,
    })
}

/// A phone alert an agent transition earns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushKind {
    NeedsInput,
    /// A turn ended; the duration is how long the agent worked.
    Finished(Duration),
}

impl PushKind {
    pub fn wanted_by(self, sub: &StoredSubscription) -> bool {
        match self {
            PushKind::NeedsInput => sub.needs_input,
            PushKind::Finished(turn) => sub.finished_mode.allows(turn),
        }
    }

    pub fn notification(self, session_id: &str, agent: &str, worktree: Option<&str>) -> AgentStateNotification {
        let (state, title) = match self {
            PushKind::NeedsInput => ("waiting_input", "Needs input"),
            PushKind::Finished(_) => ("finished", "Finished"),
        };
        let body = match worktree {
            Some(worktree) => format!("{agent} in {worktree}"),
            None => agent.to_string(),
        };
        AgentStateNotification {
            agent_id: session_id.to_string(),
            agent_name: agent.to_string(),
            worktree: worktree.map(str::to_string),
            state: state.to_string(),
            title: title.to_string(),
            body,
        }
    }
}

/// Sessions remembered at once. A forgotten idle session only loses a turn that has not begun.
const TRACKED_SESSIONS: usize = 1024;

#[derive(Debug, Clone, Copy)]
struct TurnState {
    state: AgentControlState,
    turn_started: Option<std::time::Instant>,
}

/// Turns agent-state observations into alerts: entering `Blocked` is "needs input"; `Idle` after
/// `Working` is a finished turn, timed from the first `Working` of that turn.
#[derive(Debug, Default)]
pub struct TurnTracker {
    sessions: HashMap<String, TurnState>,
}

impl TurnTracker {
    pub fn observe(
        &mut self,
        session_id: &str,
        state: AgentControlState,
        now: std::time::Instant,
    ) -> Option<PushKind> {
        if !self.sessions.contains_key(session_id) && self.sessions.len() >= TRACKED_SESSIONS {
            self.sessions.retain(|_, turn| turn.turn_started.is_some());
        }
        let entry = self.sessions.entry(session_id.to_string()).or_insert(TurnState {
            state: AgentControlState::Unknown,
            turn_started: None,
        });
        let previous = entry.state;
        // Unknown is a gap in the reading, not a state change: keep the last known state so
        // Blocked -> Unknown -> Blocked stays one wait.
        if state != AgentControlState::Unknown {
            entry.state = state;
        }
        match state {
            AgentControlState::Working => {
                entry.turn_started.get_or_insert(now);
                None
            }
            AgentControlState::Blocked => (previous != AgentControlState::Blocked).then_some(PushKind::NeedsInput),
            AgentControlState::Idle => entry
                .turn_started
                .take()
                .map(|started| PushKind::Finished(now.saturating_duration_since(started))),
            AgentControlState::Unknown => None,
        }
    }
}

/// How long a desktop "focused" report stays believed. A GUI that stops reporting (crashed,
/// detached, asleep) must not silence the phone forever.
pub const DESKTOP_FOCUS_STALE: Duration = Duration::from_secs(120);

/// Whether the last desktop window-focus report still suppresses phone pushes at `now`.
pub fn desktop_focus_suppresses(report: Option<(bool, std::time::Instant)>, now: std::time::Instant) -> bool {
    report.is_some_and(|(focused, at)| focused && now.saturating_duration_since(at) < DESKTOP_FOCUS_STALE)
}

#[cfg(test)]
#[path = "push_tests.rs"]
mod tests;
