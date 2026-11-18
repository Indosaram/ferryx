//! Phone push: VAPID keys, subscriptions persisted per paired device, and the sender.
//!
//! The payload names the agent, its worktree and the state. Terminal content never enters it.
//! Push is for decision cards only (plan D13): a new supervisor escalation, deep-linked to its
//! card, never progress, retries or finished sessions.
//! Sending goes through the async reqwest client; key and store disk I/O runs on blocking
//! workers.

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
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
    pub state: String, // "decision" or "test"
    pub title: String,
    pub body: String,
    /// Where a tap goes (a path on the phone's own origin). Absent for the test alert.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
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

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PushApnsSubscribeRequest {
    pub token: String,
    #[serde(default = "default_true")]
    pub phone_needs_input: bool,
    #[serde(default)]
    pub sandbox: bool,
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
    /// iOS devices registered through APNs, keyed by device token.
    #[serde(default)]
    apns: Vec<crate::remote::apns::ApnsSubscription>,
    /// Session refs the owner muted: they stay in the panel and never push.
    #[serde(default)]
    muted_sessions: BTreeSet<String>,
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
    /// iOS tokens, keyed by the token itself so one device cannot register twice.
    apns: Mutex<HashMap<String, crate::remote::apns::ApnsSubscription>>,
    /// Locked only while `subscriptions` is held or alone, never the other way round.
    muted: Mutex<BTreeSet<String>>,
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
        let (stored, apns, muted) = persisted
            .map(|store| (store.subscriptions, store.apns, store.muted_sessions))
            .unwrap_or_default();
        let subscriptions = stored.into_iter().map(|sub| (sub.endpoint.clone(), sub)).collect();
        let apns = apns.into_iter().map(|sub| (sub.token.clone(), sub)).collect();
        Self {
            subscriptions: Mutex::new(subscriptions),
            apns: Mutex::new(apns),
            muted: Mutex::new(muted),
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

    fn persist(
        &self,
        subs: &HashMap<String, StoredSubscription>,
        apns: &HashMap<String, crate::remote::apns::ApnsSubscription>,
        muted: &BTreeSet<String>,
    ) -> std::io::Result<()> {
        let Some(path) = self.path.as_deref() else {
            return Ok(());
        };
        let mut subscriptions: Vec<_> = subs.values().cloned().collect();
        subscriptions.sort_by(|a, b| a.endpoint.cmp(&b.endpoint));
        let mut apns: Vec<_> = apns.values().cloned().collect();
        apns.sort_by(|a, b| a.token.cmp(&b.token));
        crate::remote::auth::write_private_json(
            path,
            &PersistedPushStore { subscriptions, apns, muted_sessions: muted.clone() },
        )
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
            if let Err(error) = self.persist(&subs, &self.apns.lock(), &self.muted.lock()) {
                *subs = before;
                return Err(error.into());
            }
        }
        Ok(result)
    }

    /// Mutes or unmutes one session and persists; a failed write rolls the change back.
    /// Returns whether the set changed.
    pub fn set_session_muted(&self, session_ref: &str, muted: bool) -> Result<bool, PushStoreError> {
        let subs = self.subscriptions.lock();
        let mut set = self.muted.lock();
        let changed = if muted { set.insert(session_ref.to_string()) } else { set.remove(session_ref) };
        if changed {
            if let Err(error) = self.persist(&subs, &self.apns.lock(), &set) {
                if muted {
                    set.remove(session_ref);
                } else {
                    set.insert(session_ref.to_string());
                }
                return Err(error.into());
            }
        }
        Ok(changed)
    }

    pub fn is_session_muted(&self, session_ref: &str) -> bool {
        self.muted.lock().contains(session_ref)
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

    /// Applies preferences to every subscription the device owns, Web Push and APNs; returns how
    /// many it has. One write: the two maps are what one owner action changes.
    pub fn set_preferences(&self, device_id: &str, prefs: &PushPreferences) -> Result<usize, PushStoreError> {
        self.mutate_both(|subs, apns| {
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
            // An iOS install has no Web Push subscription, so its only row is the APNs token.
            // APNs carries no finished-turn mode, so only the needs-input switch applies.
            for stored in apns.values_mut().filter(|stored| stored.device_id == device_id) {
                if let Some(needs_input) = prefs.phone_needs_input {
                    stored.needs_input = needs_input;
                }
                count += 1;
            }
            Ok(count)
        })
    }

    /// Drops every Web Push subscription the device owns, after the device itself was revoked.
    pub fn revoke_device(&self, device_id: &str) -> Result<(), PushStoreError> {
        self.mutate(|subs| {
            subs.retain(|_, stored| stored.device_id != device_id);
            Ok(())
        })
    }

    /// Drops every APNs token the device owns, after the device itself was revoked. Apple has no
    /// push endpoint to delete, so this row is the only record that the install is gone.
    pub fn revoke_apns_device(&self, device_id: &str) -> Result<(), PushStoreError> {
        self.mutate_apns(|apns| {
            apns.retain(|_, stored| stored.device_id != device_id);
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

    /// Registers an APNs token for `device_id`. Re-registering the same token refreshes it
    /// instead of duplicating, and a token owned by another device is refused: a phone must not
    /// be able to steal another phone's alerts.
    ///
    /// Every path here takes `subscriptions` before `apns`; taking `apns` first would invert that
    /// order and deadlock against a concurrent Web Push write, which is what
    /// `concurrent_web_push_and_apns_writes_do_not_deadlock` guards. [`Self::mutate_both`] holds
    /// the order in one place so a new caller cannot pick a different one.
    fn mutate_apns<T>(
        &self,
        change: impl FnOnce(&mut HashMap<String, crate::remote::apns::ApnsSubscription>) -> Result<T, PushStoreError>,
    ) -> Result<T, PushStoreError> {
        self.mutate_both(|_, apns| change(apns))
    }

    /// Mutates the Web Push and APNs maps together, in the same lock order as [`Self::mutate`],
    /// and persists both in one write. Used where one owner action changes both transports; a
    /// failed write rolls both maps back, so the file never describes half the change.
    fn mutate_both<T>(
        &self,
        change: impl FnOnce(
            &mut HashMap<String, StoredSubscription>,
            &mut HashMap<String, crate::remote::apns::ApnsSubscription>,
        ) -> Result<T, PushStoreError>,
    ) -> Result<T, PushStoreError> {
        let mut subs = self.subscriptions.lock();
        let mut apns = self.apns.lock();
        let subs_before = subs.clone();
        let apns_before = apns.clone();
        let result = change(&mut subs, &mut apns)?;
        if *subs != subs_before || *apns != apns_before {
            if let Err(error) = self.persist(&subs, &apns, &self.muted.lock()) {
                *subs = subs_before;
                *apns = apns_before;
                return Err(error.into());
            }
        }
        Ok(result)
    }

    pub fn subscribe_apns(
        &self,
        device_id: &str,
        token: &str,
        environment: crate::remote::apns::ApnsEnvironment,
        needs_input: bool,
    ) -> Result<(), PushStoreError> {
        if !crate::remote::apns::ApnsSubscription::valid_token(token) {
            return Err(PushStoreError::InvalidEndpoint);
        }
        self.mutate_apns(|apns| {
            if apns.get(token).is_some_and(|stored| stored.device_id != device_id) {
                return Err(PushStoreError::ForeignEndpoint);
            }
            apns.insert(
                token.to_string(),
                crate::remote::apns::ApnsSubscription {
                    device_id: device_id.to_string(),
                    token: token.to_string(),
                    environment,
                    needs_input,
                },
            );
            Ok(())
        })
    }

    /// Drops an APNs token. Only its owner may remove it.
    pub fn unsubscribe_apns(&self, device_id: &str, token: &str) -> Result<(), PushStoreError> {
        self.mutate_apns(|apns| {
            if apns.get(token).is_some_and(|stored| stored.device_id != device_id) {
                return Err(PushStoreError::ForeignEndpoint);
            }
            apns.remove(token);
            Ok(())
        })
    }

    pub fn list_apns(&self) -> Vec<crate::remote::apns::ApnsSubscription> {
        let mut subs: Vec<_> = self.apns.lock().values().cloned().collect();
        subs.sort_by(|a, b| a.token.cmp(&b.token));
        subs
    }

    pub fn apns_for_device(&self, device_id: &str) -> Vec<crate::remote::apns::ApnsSubscription> {
        self.list_apns()
            .into_iter()
            .filter(|stored| stored.device_id == device_id)
            .collect()
    }

    /// Drops a token APNs reported as unregistered.
    pub fn remove_gone_apns(&self, token: &str) -> Result<(), PushStoreError> {
        self.mutate_apns(|apns| {
            apns.remove(token);
            Ok(())
        })
    }

    /// Drops every APNs token whose device `is_live` rejects.
    pub fn retain_apns_devices(&self, is_live: impl Fn(&str) -> bool) -> Result<(), PushStoreError> {
        self.mutate_apns(|apns| {
            apns.retain(|_, stored| is_live(&stored.device_id));
            Ok(())
        })
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
    /// `None` means APNs is unconfigured; the flag stops a repeated env probe.
    apns: OnceLock<Option<crate::remote::apns::SharedApns>>,
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
        Self { store, vapid_path, vapid: OnceLock::new(), apns: OnceLock::new(), client }
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

    /// The APNs sender for `FERRYX_APNS_*`, probed once. `None` leaves web push as the only
    /// route, which is what every build without an Apple key does. Reading the configured `.p8`
    /// touches disk, so the probe runs on a blocking worker; the result is cached either way.
    pub async fn apns(self: &Arc<Self>) -> Option<crate::remote::apns::SharedApns> {
        if let Some(cached) = self.apns.get() {
            return cached.clone();
        }
        let service = Arc::clone(self);
        let probed = match crate::ipc::run_blocking(move || Ok(service.probe_apns())).await {
            Ok(sender) => sender,
            Err(error) => {
                tracing::warn!("APNs probe failed: {}", error.message);
                None
            }
        };
        self.apns.get_or_init(|| probed).clone()
    }

    /// The synchronous half of [`Self::apns`]: `FERRYX_APNS_*` plus the key file. Blocking only.
    fn probe_apns(&self) -> Option<crate::remote::apns::SharedApns> {
        match crate::remote::apns::ApnsSender::from_env(self.client.clone()) {
            Ok(sender) => sender.map(Arc::new),
            Err(error) => {
                tracing::warn!("APNs stays disabled: {error}");
                None
            }
        }
    }

    /// Test-only: installs the sender the env probe would have built, so a test can point APNs
    /// at a local mock instead of Apple. Must run before the first [`Self::apns`] call.
    #[cfg(test)]
    pub(crate) fn install_apns_for_test(&self, sender: crate::remote::apns::SharedApns) {
        let _ = self.apns.set(Some(sender));
    }

    /// Test-only: pins the probe to "not configured". Without it, a test asserting the
    /// unconfigured path would depend on the ambient `FERRYX_APNS_*` of the machine it runs on.
    #[cfg(test)]
    pub(crate) fn install_no_apns_for_test(&self) {
        let _ = self.apns.set(None);
    }

    /// Posts one APNs alert, dropping the token when Apple says it is dead.
    pub async fn send_apns(
        self: &Arc<Self>,
        sender: &crate::remote::apns::SharedApns,
        sub: &crate::remote::apns::ApnsSubscription,
        notif: &AgentStateNotification,
    ) -> SendOutcome {
        let payload = crate::remote::apns::ApnsPayload::decision(
            &notif.title,
            &notif.body,
            notif.url.clone(),
            Some(notif.agent_id.clone()).filter(|id| !id.is_empty()),
        );
        let outcome = sender.send(sub, &payload).await;
        if outcome == crate::remote::apns::ApnsOutcome::Gone {
            let service = Arc::clone(self);
            let token = sub.token.clone();
            let removed = crate::ipc::run_blocking(move || {
                service.store.remove_gone_apns(&token).map_err(crate::ipc::IpcError::internal)
            })
            .await;
            if let Err(error) = removed {
                tracing::warn!("dropping a dead APNs token failed: {}", error.message);
            }
            return SendOutcome::Gone;
        }
        match outcome {
            crate::remote::apns::ApnsOutcome::Delivered => SendOutcome::Delivered,
            crate::remote::apns::ApnsOutcome::Gone => SendOutcome::Gone,
            crate::remote::apns::ApnsOutcome::Failed(error) => SendOutcome::Failed(error),
        }
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

/// The only phone alert: a new decision card for the owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushKind {
    Decision,
}

impl PushKind {
    pub fn wanted_by(self, sub: &StoredSubscription) -> bool {
        match self {
            PushKind::Decision => sub.needs_input,
        }
    }
}

/// The notification for decision cards: one names its agent and worktree and opens its card;
/// several held back by the batch window collapse into one that opens the panel.
pub fn decision_notification(refs: &[String], agent: &str, worktree: Option<&str>) -> AgentStateNotification {
    let (agent_id, agent_name, worktree, title, body, url) = match refs {
        [only] => (
            only.clone(),
            agent.to_string(),
            worktree.map(str::to_string),
            "Decision needed".to_string(),
            match worktree {
                Some(worktree) => format!("{agent} in {worktree}"),
                None => agent.to_string(),
            },
            ferryx_hub::supervisor::push_policy::decision_deep_link(only),
        ),
        _ => (
            String::new(),
            "Ferryx".to_string(),
            None,
            "Decisions needed".to_string(),
            format!("{} sessions need a decision", refs.len()),
            ferryx_hub::supervisor::push_policy::PANEL_DEEP_LINK.to_string(),
        ),
    };
    AgentStateNotification {
        agent_id,
        agent_name,
        worktree,
        state: "decision".to_string(),
        title,
        body,
        url: Some(url),
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
