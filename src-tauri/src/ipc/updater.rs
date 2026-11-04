use crate::ipc::error::{IpcError, IpcErrorCode};
use crate::ipc::run_blocking;
use base64::Engine as _;
use serde::Serialize;
use std::sync::Mutex;
use std::time::Duration;
use tauri::ipc::Channel;
use tauri::Url;
use tauri::{AppHandle, Runtime, State};
use tauri::utils::{config::BundleType, platform::bundle_type};
use tauri_plugin_updater::{Update, Updater, UpdaterExt};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Host platform of the running binary, as a plain value so the ownership decision below can be
/// unit tested for every platform from any platform.
enum UpdateHost {
    Windows,
    MacOs,
    Linux,
}

impl UpdateHost {
    fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Linux
        }
    }
}

/// Pure decision behind [`updater_managed_externally`]: whether another channel owns update
/// delivery for this install.
///
/// A Linux distribution package is owned by the package manager: `tauri-plugin-updater` routes a
/// `Deb`/`Rpm` bundle to `dpkg`/`rpm`, which reject the raw AppImage Ferryx publishes, so an
/// in-app update there can never succeed. An AppImage is replaced in place, and an unpackaged
/// build reports no bundle type and keeps self-updating. Windows Store ownership is a
/// package-identity fact rather than a bundle type (`BundleType::Msi` is the self-updating MSI
/// installer), so the bundle decision never claims a Windows install.
fn managed_externally_for(host: UpdateHost, bundle: Option<BundleType>) -> bool {
    match host {
        UpdateHost::Linux => matches!(bundle, Some(BundleType::Deb | BundleType::Rpm)),
        UpdateHost::Windows | UpdateHost::MacOs => false,
    }
}

#[cfg(windows)]
fn has_package_identity() -> bool {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentPackageFullName(
            packageFullNameLength: *mut u32,
            packageFullName: *mut u16,
        ) -> i32;
    }
    let mut len: u32 = 0;
    let rc = unsafe { GetCurrentPackageFullName(&mut len, std::ptr::null_mut()) };
    rc == 122 || rc == 0
}

/// Store (MSIX) installs live under the write-protected `WindowsApps` tree where the Store
/// owns update delivery; self-updating those installs would fail or duplicate the app. The
/// bundle term is always false here and keeps the whole decision in one place.
#[cfg(windows)]
pub fn updater_managed_externally() -> bool {
    managed_externally_for(UpdateHost::current(), bundle_type())
        || has_package_identity()
        || std::env::var_os("FERRYX_STORE_BUILD").is_some()
        || std::env::current_exe()
            .map(|exe| {
                let p = exe.to_string_lossy().to_lowercase();
                p.contains(r"\windowsapps\")
                    || p.contains("windowsapps")
                    || exe.parent().map(|d| d.join("msix.marker").exists()).unwrap_or(false)
            })
            .unwrap_or(false)
}

/// macOS self-updates from the `.app`. On Linux only the AppImage is replaced in place by the
/// in-app updater; a distribution package is updated by `dpkg`/`rpm` instead.
#[cfg(not(windows))]
pub fn updater_managed_externally() -> bool {
    managed_externally_for(UpdateHost::current(), bundle_type())
}

#[tauri::command]
pub async fn cmd_updater_managed_externally() -> Result<bool, IpcError> {
    Ok(updater_managed_externally())
}

/// Windows ships through two channels with different update ownership: MSIX installs are
/// updated by the Microsoft Store, NSIS installs by the in-app updater. macOS and the Linux
/// AppImage self-update; a Linux distribution package keeps the same `native` channel while
/// `updater_managed_externally` hides the in-app update card.
pub fn distribution_channel() -> &'static str {
    if cfg!(windows) {
        if updater_managed_externally() {
            "store"
        } else {
            "installer"
        }
    } else {
        "native"
    }
}

#[tauri::command]
pub async fn cmd_distribution_channel() -> Result<&'static str, IpcError> {
    Ok(distribution_channel())
}

/// No-progress window: a download that delivers no bytes for this long is abandoned. It is not a
/// total-request timeout, so a slow download that keeps delivering bytes never trips it.
const DEFAULT_STALL_WINDOW: Duration = Duration::from_secs(60);

const UPDATER_MANAGED_EXTERNALLY: &str = "UPDATER_MANAGED_EXTERNALLY";
const UPDATER_VERSION_MISMATCH: &str = "UPDATER_VERSION_MISMATCH";
const UPDATER_NO_PENDING_INSTALL: &str = "UPDATER_NO_PENDING_INSTALL";
/// The signed manifest sets `halt: true`: the release operator paused every update.
const UPDATE_HALTED: &str = "UPDATE_HALTED";
/// The signed manifest offers a version below its own `minimumVersion` floor.
const UPDATE_BELOW_FLOOR: &str = "UPDATE_BELOW_FLOOR";
/// `latest.json.minisig` is missing, malformed or does not verify with the updater key, or the
/// manifest the update check used differs from the verified one.
const UPDATE_MANIFEST_UNVERIFIED: &str = "UPDATE_MANIFEST_UNVERIFIED";

/// The release publishes the minisign signature of every manifest next to it under this suffix.
const MANIFEST_SIGNATURE_SUFFIX: &str = ".minisig";
const MANIFEST_FETCH_TIMEOUT: Duration = Duration::from_secs(30);

/// Progress stream delivered to the frontend through the command's channel.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "event", content = "data", rename_all = "camelCase")]
pub enum UpdaterDownloadEvent {
    #[serde(rename_all = "camelCase")]
    Started { content_length: Option<u64> },
    #[serde(rename_all = "camelCase")]
    Progress { chunk_length: usize },
    Finished,
    Cancelled,
    Stalled,
    Error { message: String },
}

/// What the download task reports while it runs.
#[derive(Debug)]
enum DownloadSignal {
    Chunk {
        len: usize,
        content_length: Option<u64>,
    },
    /// The body is fully received; only signature verification remains, so the no-progress
    /// watchdog is disarmed.
    BodyComplete,
    Done(Result<Vec<u8>, String>),
}

#[derive(Debug, PartialEq)]
enum DownloadOutcome {
    Completed(Vec<u8>),
    Cancelled,
    Stalled,
    Failed(String),
}

/// Supervises one download: races the signal stream against the cancel token and a no-progress
/// watchdog. The watchdog is armed before the first signal, so a server that never answers the
/// request (no headers, no bytes) stalls exactly like one that stops mid-body, and every chunk
/// pushes the deadline out again.
async fn supervise_download(
    mut signals: mpsc::UnboundedReceiver<DownloadSignal>,
    cancel: CancellationToken,
    stall_window: Duration,
    mut on_event: impl FnMut(UpdaterDownloadEvent),
) -> DownloadOutcome {
    let mut deadline = tokio::time::Instant::now() + stall_window;
    let mut watchdog_armed = true;
    let mut started = false;
    loop {
        let stall = async {
            if watchdog_armed {
                tokio::time::sleep_until(deadline).await
            } else {
                std::future::pending::<()>().await
            }
        };
        tokio::select! {
            biased;
            _ = cancel.cancelled() => return DownloadOutcome::Cancelled,
            signal = signals.recv() => match signal {
                Some(DownloadSignal::Chunk { len, content_length }) => {
                    if !started {
                        started = true;
                        on_event(UpdaterDownloadEvent::Started { content_length });
                    }
                    on_event(UpdaterDownloadEvent::Progress { chunk_length: len });
                    deadline = tokio::time::Instant::now() + stall_window;
                }
                Some(DownloadSignal::BodyComplete) => watchdog_armed = false,
                Some(DownloadSignal::Done(Ok(bytes))) => return DownloadOutcome::Completed(bytes),
                Some(DownloadSignal::Done(Err(message))) => return DownloadOutcome::Failed(message),
                None => {
                    return DownloadOutcome::Failed("update download ended without a result".into())
                }
            },
            _ = stall => return DownloadOutcome::Stalled,
        }
    }
}

/// Download bookkeeping kept in managed state: the cancel token of the running download (tagged
/// with a generation so a superseded download cannot claim the slot) and the verified bytes
/// waiting for install. Generic over the pending payload so the state machine is testable
/// without a real [`Update`].
struct DownloadSlot<P> {
    inner: Mutex<SlotInner<P>>,
}

struct SlotInner<P> {
    generation: u64,
    running: Option<(u64, CancellationToken)>,
    pending: Option<P>,
}

struct DownloadTicket {
    generation: u64,
    token: CancellationToken,
}

impl<P> Default for DownloadSlot<P> {
    fn default() -> Self {
        Self {
            inner: Mutex::new(SlotInner {
                generation: 0,
                running: None,
                pending: None,
            }),
        }
    }
}

impl<P> DownloadSlot<P> {
    fn lock(&self) -> std::sync::MutexGuard<'_, SlotInner<P>> {
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Starts a download, dropping any earlier pending bytes. Refuses a second concurrent one.
    fn begin(&self) -> Result<DownloadTicket, IpcError> {
        let mut inner = self.lock();
        if inner.running.is_some() {
            return Err(IpcError::new(
                IpcErrorCode::RequestConflict,
                "an update download is already running",
            ));
        }
        inner.pending = None;
        inner.generation += 1;
        let token = CancellationToken::new();
        inner.running = Some((inner.generation, token.clone()));
        Ok(DownloadTicket {
            generation: inner.generation,
            token,
        })
    }

    /// Ends the download that owns `ticket`. Bytes are kept only for a completed download whose
    /// token was never cancelled, so a cancel that races completion still leaves nothing to
    /// install. Returns whether the payload was stored.
    fn finish(&self, ticket: &DownloadTicket, payload: Option<P>) -> bool {
        let mut inner = self.lock();
        let owns_slot = inner
            .running
            .as_ref()
            .is_some_and(|(generation, _)| *generation == ticket.generation);
        if owns_slot {
            inner.running = None;
        }
        match payload {
            Some(payload) if owns_slot && !ticket.token.is_cancelled() => {
                inner.pending = Some(payload);
                true
            }
            _ => false,
        }
    }

    /// Cancels the running download (if any) and discards pending bytes.
    fn cancel(&self) {
        let mut inner = self.lock();
        if let Some((_, token)) = inner.running.take() {
            token.cancel();
        }
        inner.pending = None;
    }

    /// Hands the verified bytes to install exactly once.
    fn take_pending(&self) -> Option<P> {
        self.lock().pending.take()
    }
}

/// Managed state for the in-app updater download/install commands.
#[derive(Default)]
pub struct UpdaterDownloadState {
    slot: DownloadSlot<(Update, Vec<u8>)>,
}

/// Runs `touch` (the only path that reaches the updater plugin) only when the in-app updater
/// owns this install. `app.updater()` panics when the plugin was never registered, which is the
/// case for Store/deb/rpm installs.
fn gate_updater<T>(managed_externally: bool, touch: impl FnOnce() -> T) -> Result<T, IpcError> {
    if managed_externally {
        return Err(IpcError::new(
            IpcErrorCode::Custom(UPDATER_MANAGED_EXTERNALLY.into()),
            "updates for this install are delivered by another channel",
        ));
    }
    Ok(touch())
}

/// The UI asked to download the version it showed the user; a fresh check that offers anything
/// else (or nothing) is refused rather than silently downloading a different build.
fn select_expected_update<T>(
    expected: &str,
    offered: Option<T>,
    version_of: impl Fn(&T) -> &str,
) -> Result<T, IpcError> {
    match offered {
        Some(update) if version_of(&update) == expected => Ok(update),
        Some(update) => Err(IpcError::new(
            IpcErrorCode::Custom(UPDATER_VERSION_MISMATCH.into()),
            format!(
                "update check now offers {}, not {expected}",
                version_of(&update)
            ),
        )
        .with_details(serde_json::json!({ "expected": expected, "offered": version_of(&update) }))),
        None => Err(IpcError::new(
            IpcErrorCode::NotFound,
            format!("no update is available (expected {expected})"),
        )),
    }
}

fn stall_window() -> Duration {
    #[cfg(debug_assertions)]
    if let Some(secs) = std::env::var("FERRYX_UPDATER_STALL_SECS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
    {
        return Duration::from_secs(secs);
    }
    DEFAULT_STALL_WINDOW
}

fn manifest_unverified(message: impl Into<String>) -> IpcError {
    IpcError::new(
        IpcErrorCode::Custom(UPDATE_MANIFEST_UNVERIFIED.into()),
        message,
    )
}

/// Decodes the updater public key in the form `plugins.updater.pubkey` stores it: base64 of the
/// two-line `minisign.pub` text.
fn decode_updater_pubkey(wrapped: &str) -> Result<minisign_verify::PublicKey, IpcError> {
    let text = base64::engine::general_purpose::STANDARD
        .decode(wrapped.trim())
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .ok_or_else(|| IpcError::internal("the updater public key is not base64 text"))?;
    minisign_verify::PublicKey::decode(&text)
        .map_err(|error| IpcError::internal(format!("the updater public key is invalid: {error}")))
}

/// Accepts the plain four-line `minisign -S` output and the base64-wrapped form `tauri signer`
/// writes.
fn decode_manifest_signature(raw: &[u8]) -> Result<minisign_verify::Signature, IpcError> {
    let text = std::str::from_utf8(raw)
        .map_err(|_| manifest_unverified("latest.json.minisig is not text"))?
        .trim();
    let text = if text.starts_with("untrusted comment:") {
        text.to_owned()
    } else {
        base64::engine::general_purpose::STANDARD
            .decode(text)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .ok_or_else(|| manifest_unverified("latest.json.minisig is not a minisign signature"))?
    };
    minisign_verify::Signature::decode(&text).map_err(|error| {
        manifest_unverified(format!("latest.json.minisig is malformed: {error}"))
    })
}

/// Verifies the exact manifest bytes against the updater key and parses them. Legacy
/// (non-prehashed) signatures are refused.
fn verify_manifest(
    manifest: &[u8],
    signature: &[u8],
    pubkey: &minisign_verify::PublicKey,
) -> Result<serde_json::Value, IpcError> {
    let signature = decode_manifest_signature(signature)?;
    pubkey.verify(manifest, &signature, false).map_err(|error| {
        manifest_unverified(format!(
            "latest.json does not verify with the updater key: {error}"
        ))
    })?;
    serde_json::from_slice(manifest)
        .map_err(|error| manifest_unverified(format!("signed latest.json is not JSON: {error}")))
}

/// Same leniency as the updater plugin's manifest parser: an optional leading `v`.
fn parse_manifest_version(raw: &str) -> Option<semver::Version> {
    let raw = raw.trim();
    semver::Version::parse(raw.strip_prefix('v').unwrap_or(raw)).ok()
}

/// Release controls carried by a verified manifest.
#[derive(Debug, PartialEq)]
struct ManifestPolicy {
    version: semver::Version,
    minimum_version: Option<semver::Version>,
    halt: bool,
    halt_reason: Option<String>,
}

/// Reads the policy fields. A malformed field fails closed: a signed manifest whose controls
/// cannot be read is not trusted to offer anything.
fn manifest_policy(manifest: &serde_json::Value) -> Result<ManifestPolicy, IpcError> {
    use serde_json::Value;
    let version = manifest
        .get("version")
        .or_else(|| manifest.get("name"))
        .and_then(Value::as_str)
        .and_then(parse_manifest_version)
        .ok_or_else(|| manifest_unverified("signed latest.json has no valid version"))?;
    let minimum_version = match manifest.get("minimumVersion") {
        None | Some(Value::Null) => None,
        Some(Value::String(raw)) => Some(parse_manifest_version(raw).ok_or_else(|| {
            manifest_unverified(format!("signed latest.json has an invalid minimumVersion {raw:?}"))
        })?),
        Some(other) => {
            return Err(manifest_unverified(format!(
                "signed latest.json has a non-string minimumVersion {other}"
            )))
        }
    };
    let halt = match manifest.get("halt") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(halt)) => *halt,
        Some(other) => {
            return Err(manifest_unverified(format!(
                "signed latest.json has a non-boolean halt {other}"
            )))
        }
    };
    let halt_reason = manifest
        .get("haltReason")
        .and_then(Value::as_str)
        .map(str::to_owned);
    Ok(ManifestPolicy {
        version,
        minimum_version,
        halt,
        halt_reason,
    })
}

/// The halt switch wins over everything; then the offered version must reach the floor.
fn enforce_manifest_policy(policy: &ManifestPolicy) -> Result<(), IpcError> {
    if policy.halt {
        let reason = policy.halt_reason.as_deref().unwrap_or("");
        return Err(IpcError::new(
            IpcErrorCode::Custom(UPDATE_HALTED.into()),
            if reason.is_empty() {
                "updates are paused by the release operator".to_owned()
            } else {
                format!("updates are paused by the release operator: {reason}")
            },
        )
        .with_details(serde_json::json!({ "haltReason": policy.halt_reason })));
    }
    if let Some(floor) = &policy.minimum_version {
        if policy.version < *floor {
            return Err(IpcError::new(
                IpcErrorCode::Custom(UPDATE_BELOW_FLOOR.into()),
                format!(
                    "the offered version {} is below the minimum version {floor}",
                    policy.version
                ),
            )
            .with_details(serde_json::json!({
                "offered": policy.version.to_string(),
                "minimumVersion": floor.to_string(),
            })));
        }
    }
    Ok(())
}

fn manifest_signature_url(endpoint: &Url) -> Url {
    let mut url = endpoint.clone();
    let path = format!("{}{MANIFEST_SIGNATURE_SUFFIX}", endpoint.path());
    url.set_path(&path);
    url
}

async fn fetch_bytes(client: &reqwest::Client, url: Url) -> Result<Vec<u8>, String> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("HTTP {status}"));
    }
    let body = response.bytes().await.map_err(|error| error.to_string())?;
    Ok(body.to_vec())
}

/// Fetches the first manifest an endpoint serves (the order the updater plugin tries them) and
/// its `.minisig`, and returns the verified JSON. A manifest without a valid signature is never
/// skipped in favour of the next endpoint.
async fn fetch_verified_manifest(
    endpoints: &[Url],
    pubkey: &minisign_verify::PublicKey,
) -> Result<serde_json::Value, IpcError> {
    let client = reqwest::Client::builder()
        .timeout(MANIFEST_FETCH_TIMEOUT)
        .build()
        .map_err(IpcError::internal)?;
    let mut failures = Vec::new();
    for endpoint in endpoints {
        let manifest = match fetch_bytes(&client, endpoint.clone()).await {
            Ok(manifest) => manifest,
            Err(error) => {
                failures.push(format!("{endpoint}: {error}"));
                continue;
            }
        };
        let signature_url = manifest_signature_url(endpoint);
        let signature = fetch_bytes(&client, signature_url.clone())
            .await
            .map_err(|error| {
                manifest_unverified(format!("no manifest signature at {signature_url}: {error}"))
            })?;
        return verify_manifest(&manifest, &signature, pubkey);
    }
    Err(IpcError::internal(format!(
        "no update manifest endpoint answered: {}",
        failures.join("; ")
    )))
}

/// Stable update manifest; the same URL as `plugins.updater.endpoints` in tauri.conf.json.
pub const STABLE_UPDATE_ENDPOINT: &str =
    "https://github.com/Indosaram/ferryx/releases/latest/download/latest.json";
/// Beta update manifest, uploaded next to `latest.json` on the latest release.
pub const BETA_UPDATE_ENDPOINT: &str =
    "https://github.com/Indosaram/ferryx/releases/latest/download/latest-beta.json";

/// Endpoints for the update channel the user picked in Settings. Stable (the default, also for
/// any unknown value) keeps the configured endpoints. Beta asks `latest-beta.json` first and falls
/// back to the stable manifest, which the plugin tries next when the beta one is missing.
fn channel_endpoints(channel: Option<&str>) -> Option<[&'static str; 2]> {
    match channel {
        Some("beta") => Some([BETA_UPDATE_ENDPOINT, STABLE_UPDATE_ENDPOINT]),
        _ => None,
    }
}

/// Debug builds may point the updater at a test manifest; release builds never read the variable.
fn debug_endpoint_override() -> Option<String> {
    #[cfg(debug_assertions)]
    if let Ok(endpoint) = std::env::var("FERRYX_UPDATER_ENDPOINT") {
        return Some(endpoint);
    }
    None
}

/// Public key and endpoints of `plugins.updater` in tauri.conf.json.
fn updater_plugin_config<R: Runtime>(app: &AppHandle<R>) -> Result<(String, Vec<Url>), IpcError> {
    let config = app
        .config()
        .plugins
        .0
        .get("updater")
        .ok_or_else(|| IpcError::internal("plugins.updater is missing from the app config"))?;
    let pubkey = config
        .get("pubkey")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| IpcError::internal("plugins.updater.pubkey is missing"))?
        .to_owned();
    let endpoints = config
        .get("endpoints")
        .and_then(serde_json::Value::as_array)
        .map(|endpoints| {
            endpoints
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::parse::<Url>)
                .collect::<Result<Vec<Url>, _>>()
        })
        .transpose()
        .map_err(IpcError::internal)?
        .unwrap_or_default();
    Ok((pubkey, endpoints))
}

/// Endpoints for this check: the debug override, else the channel's, else the configured ones.
fn resolve_endpoints(
    configured: Vec<Url>,
    update_channel: Option<&str>,
    debug_override: Option<&str>,
) -> Result<Vec<Url>, IpcError> {
    if let Some(endpoint) = debug_override {
        let url = endpoint.trim().parse().map_err(|error| {
            IpcError::new(
                IpcErrorCode::InvalidArgument,
                format!("FERRYX_UPDATER_ENDPOINT is not a URL: {error}"),
            )
        })?;
        return Ok(vec![url]);
    }
    if let Some(endpoints) = channel_endpoints(update_channel) {
        return endpoints
            .iter()
            .map(|endpoint| endpoint.parse())
            .collect::<Result<Vec<_>, _>>()
            .map_err(IpcError::internal);
    }
    Ok(configured)
}

fn build_updater<R: Runtime>(app: &AppHandle<R>, endpoints: Vec<Url>) -> Result<Updater, IpcError> {
    app.updater_builder()
        .endpoints(endpoints)
        .map_err(IpcError::internal)?
        .build()
        .map_err(IpcError::internal)
}

/// The update check behind both commands. An externally managed install stops at the gate
/// before any request. Otherwise the signed manifest is verified and its halt switch and
/// minimum-version floor enforced before the plugin check runs; the plugin keeps its default
/// comparator, so an older manifest never offers a downgrade. The plugin's own manifest fetch
/// must equal the verified one, so a swap between the two requests is refused.
async fn policy_checked_update<R: Runtime>(
    app: &AppHandle<R>,
    managed_externally: bool,
    update_channel: Option<&str>,
    debug_override: Option<&str>,
) -> Result<Option<Update>, IpcError> {
    gate_updater(managed_externally, || ())?;
    let (pubkey, configured) = updater_plugin_config(app)?;
    let endpoints = resolve_endpoints(configured, update_channel, debug_override)?;
    let pubkey = decode_updater_pubkey(&pubkey)?;
    let manifest = fetch_verified_manifest(&endpoints, &pubkey).await?;
    enforce_manifest_policy(&manifest_policy(&manifest)?)?;
    let offered = build_updater(app, endpoints)?
        .check()
        .await
        .map_err(IpcError::internal)?;
    match offered {
        Some(update) if update.raw_json != manifest => Err(manifest_unverified(
            "the update check received a manifest that differs from the signed latest.json",
        )),
        offered => Ok(offered),
    }
}

/// Update offered by [`cmd_updater_check`], in the shape the UI shows to the user.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdaterOffer {
    pub version: String,
    pub current_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

impl From<&Update> for UpdaterOffer {
    fn from(update: &Update) -> Self {
        Self {
            version: update.version.clone(),
            current_version: update.current_version.clone(),
            // Same rendering as the plugin's own `check` command.
            date: update.date.map(|date| date.to_string()),
            body: update.body.clone(),
        }
    }
}

/// Availability check through the same [`policy_checked_update`] as [`cmd_updater_download`], so
/// the signed-manifest policy and the debug-only `FERRYX_UPDATER_ENDPOINT` override cover the
/// offer as well as the download. Refusals carry `UPDATE_HALTED`, `UPDATE_BELOW_FLOOR` or
/// `UPDATE_MANIFEST_UNVERIFIED`.
#[tauri::command]
pub async fn cmd_updater_check<R: Runtime>(
    app: AppHandle<R>,
    update_channel: Option<String>,
) -> Result<Option<UpdaterOffer>, IpcError> {
    let offered = policy_checked_update(
        &app,
        updater_managed_externally(),
        update_channel.as_deref(),
        debug_endpoint_override().as_deref(),
    )
    .await?;
    Ok(offered.as_ref().map(UpdaterOffer::from))
}

/// Checks for the update the UI expects and downloads it under a cancel token and a no-progress
/// watchdog. The outcome arrives on `on_event`; verified bytes stay in managed state for
/// [`cmd_updater_install`].
#[tauri::command]
pub async fn cmd_updater_download<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, UpdaterDownloadState>,
    expected_version: String,
    on_event: Channel<UpdaterDownloadEvent>,
    update_channel: Option<String>,
) -> Result<(), IpcError> {
    let offered = policy_checked_update(
        &app,
        updater_managed_externally(),
        update_channel.as_deref(),
        debug_endpoint_override().as_deref(),
    )
    .await?;
    let update = select_expected_update(&expected_version, offered, |update: &Update| {
        update.version.as_str()
    })?;

    let ticket = state.slot.begin()?;
    let (tx, rx) = mpsc::unbounded_channel();
    let download = {
        let update = update.clone();
        tauri::async_runtime::spawn(async move {
            let chunk_tx = tx.clone();
            let finish_tx = tx.clone();
            let result = update
                .download(
                    move |len, content_length| {
                        let _ = chunk_tx.send(DownloadSignal::Chunk {
                            len,
                            content_length,
                        });
                    },
                    move || {
                        let _ = finish_tx.send(DownloadSignal::BodyComplete);
                    },
                )
                .await
                .map_err(|error| error.to_string());
            let _ = tx.send(DownloadSignal::Done(result));
        })
    };

    let outcome = supervise_download(rx, ticket.token.clone(), stall_window(), |event| {
        let _ = on_event.send(event);
    })
    .await;
    // Dropping the in-flight request is what actually stops the transfer.
    download.abort();

    let event = match outcome {
        DownloadOutcome::Completed(bytes) => {
            if state.slot.finish(&ticket, Some((update, bytes))) {
                UpdaterDownloadEvent::Finished
            } else {
                UpdaterDownloadEvent::Cancelled
            }
        }
        DownloadOutcome::Cancelled => {
            state.slot.finish(&ticket, None);
            UpdaterDownloadEvent::Cancelled
        }
        DownloadOutcome::Stalled => {
            state.slot.finish(&ticket, None);
            UpdaterDownloadEvent::Stalled
        }
        DownloadOutcome::Failed(message) => {
            state.slot.finish(&ticket, None);
            UpdaterDownloadEvent::Error { message }
        }
    };
    let _ = on_event.send(event);
    Ok(())
}

/// Cancels the running download and discards any downloaded bytes.
#[tauri::command]
pub async fn cmd_updater_cancel(state: State<'_, UpdaterDownloadState>) -> Result<(), IpcError> {
    gate_updater(updater_managed_externally(), || state.slot.cancel())
}

/// Installs the bytes kept by a finished [`cmd_updater_download`]. Not cancellable. On Windows
/// the plugin launches the installer and exits the process, so this never returns there.
#[tauri::command]
pub async fn cmd_updater_install(state: State<'_, UpdaterDownloadState>) -> Result<(), IpcError> {
    let pending = gate_updater(updater_managed_externally(), || state.slot.take_pending())?;
    let (update, bytes) = pending.ok_or_else(|| {
        IpcError::new(
            IpcErrorCode::Custom(UPDATER_NO_PENDING_INSTALL.into()),
            "no downloaded update is waiting to be installed",
        )
    })?;
    run_blocking(move || update.install(bytes).map_err(IpcError::internal)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channel_endpoints() {
        assert_eq!(channel_endpoints(None), None);
        assert_eq!(channel_endpoints(Some("stable")), None);
        assert_eq!(channel_endpoints(Some("nightly")), None);
        assert_eq!(
            channel_endpoints(Some("beta")),
            Some([BETA_UPDATE_ENDPOINT, STABLE_UPDATE_ENDPOINT])
        );
        for endpoint in [BETA_UPDATE_ENDPOINT, STABLE_UPDATE_ENDPOINT] {
            assert!(endpoint.parse::<tauri::Url>().is_ok(), "{endpoint}");
        }
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
        assert_eq!(
            config["plugins"]["updater"]["endpoints"][0].as_str(),
            Some(STABLE_UPDATE_ENDPOINT)
        );
    }

    #[test]
    fn test_distribution_channel_non_windows() {
        if !cfg!(windows) {
            assert_eq!(distribution_channel(), "native");
            // A test binary is never packaged as a distribution package, so it keeps the
            // in-app updater.
            assert!(!updater_managed_externally());
        }
    }

    #[test]
    fn test_managed_externally_for_linux_packages() {
        // dpkg/rpm own these installs: the plugin routes the bundle type to the package
        // manager, which rejects the raw AppImage Ferryx publishes.
        assert!(managed_externally_for(
            UpdateHost::Linux,
            Some(BundleType::Deb)
        ));
        assert!(managed_externally_for(
            UpdateHost::Linux,
            Some(BundleType::Rpm)
        ));
        // The AppImage is replaced in place, and an unpackaged build has no channel but itself.
        assert!(!managed_externally_for(
            UpdateHost::Linux,
            Some(BundleType::AppImage)
        ));
        assert!(!managed_externally_for(UpdateHost::Linux, None));
        // Windows Store ownership is a package-identity fact, not a bundle type; macOS
        // self-updates from the `.app`.
        assert!(!managed_externally_for(
            UpdateHost::Windows,
            Some(BundleType::Msi)
        ));
        assert!(!managed_externally_for(
            UpdateHost::Windows,
            Some(BundleType::Nsis)
        ));
        assert!(!managed_externally_for(
            UpdateHost::MacOs,
            Some(BundleType::App)
        ));
    }

    #[test]
    fn test_distribution_channel_windows() {
        if cfg!(windows) {
            std::env::set_var("FERRYX_STORE_BUILD", "1");
            assert_eq!(distribution_channel(), "store");
            assert!(updater_managed_externally());
            std::env::remove_var("FERRYX_STORE_BUILD");
        }
    }

    const WINDOW: Duration = Duration::from_secs(60);

    fn chunk(len: usize) -> DownloadSignal {
        DownloadSignal::Chunk {
            len,
            content_length: Some(1_000),
        }
    }

    /// Runs the supervisor on its own task so the test can drive the signal stream and the
    /// paused clock; returns the sender, the cancel token, the join handle and the event log.
    type SupervisorFixture = (
        mpsc::UnboundedSender<DownloadSignal>,
        CancellationToken,
        tokio::task::JoinHandle<DownloadOutcome>,
        std::sync::Arc<Mutex<Vec<UpdaterDownloadEvent>>>,
    );

    fn spawn_supervisor() -> SupervisorFixture {
        let (tx, rx) = mpsc::unbounded_channel();
        let cancel = CancellationToken::new();
        let events = std::sync::Arc::new(Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&events);
        let handle = tokio::spawn(supervise_download(rx, cancel.clone(), WINDOW, move |event| {
            sink.lock().unwrap().push(event)
        }));
        (tx, cancel, handle, events)
    }

    #[tokio::test(start_paused = true)]
    async fn cancel_stops_before_install() {
        let slot: DownloadSlot<Vec<u8>> = DownloadSlot::default();
        let ticket = slot.begin().unwrap();
        let (tx, rx) = mpsc::unbounded_channel();
        let supervisor = tokio::spawn(supervise_download(
            rx,
            ticket.token.clone(),
            WINDOW,
            |_| {},
        ));
        tx.send(chunk(100)).unwrap();
        tokio::task::yield_now().await;
        slot.cancel();
        assert_eq!(supervisor.await.unwrap(), DownloadOutcome::Cancelled);
        // The body completing after the cancel still never reaches install.
        assert!(!slot.finish(&ticket, Some(vec![1, 2, 3])));
        assert_eq!(slot.take_pending(), None);
    }

    #[tokio::test(start_paused = true)]
    async fn stall_fires_after_no_progress_window() {
        let (tx, _cancel, handle, events) = spawn_supervisor();
        tx.send(chunk(10)).unwrap();
        tokio::task::yield_now().await;
        tokio::time::advance(WINDOW - Duration::from_millis(1)).await;
        assert!(!handle.is_finished(), "stalled before the full window elapsed");
        tokio::time::advance(Duration::from_millis(2)).await;
        assert_eq!(handle.await.unwrap(), DownloadOutcome::Stalled);
        assert_eq!(
            events.lock().unwrap().as_slice(),
            &[
                UpdaterDownloadEvent::Started {
                    content_length: Some(1_000)
                },
                UpdaterDownloadEvent::Progress { chunk_length: 10 },
            ]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn slow_progress_never_stalls() {
        let (tx, _cancel, handle, _events) = spawn_supervisor();
        // Ten minutes of one byte every 59 s: far past any total timeout, never idle a full window.
        for _ in 0..10 {
            tokio::time::advance(WINDOW - Duration::from_secs(1)).await;
            tx.send(chunk(1)).unwrap();
            tokio::task::yield_now().await;
            assert!(!handle.is_finished());
        }
        tx.send(DownloadSignal::BodyComplete).unwrap();
        // Signature verification may take longer than the window once the body is in.
        tokio::time::advance(WINDOW * 3).await;
        assert!(!handle.is_finished());
        tx.send(DownloadSignal::Done(Ok(vec![9; 10]))).unwrap();
        assert_eq!(handle.await.unwrap(), DownloadOutcome::Completed(vec![9; 10]));
    }

    #[tokio::test(start_paused = true)]
    async fn completed_download_yields_bytes() {
        let slot: DownloadSlot<Vec<u8>> = DownloadSlot::default();
        let ticket = slot.begin().unwrap();
        let (tx, rx) = mpsc::unbounded_channel();
        let events = std::sync::Arc::new(Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&events);
        let supervisor = tokio::spawn(supervise_download(
            rx,
            ticket.token.clone(),
            WINDOW,
            move |event| sink.lock().unwrap().push(event),
        ));
        tx.send(chunk(3)).unwrap();
        tx.send(DownloadSignal::BodyComplete).unwrap();
        tx.send(DownloadSignal::Done(Ok(vec![1, 2, 3]))).unwrap();
        let DownloadOutcome::Completed(bytes) = supervisor.await.unwrap() else {
            panic!("download did not complete");
        };
        assert!(slot.finish(&ticket, Some(bytes)));
        assert_eq!(slot.take_pending(), Some(vec![1, 2, 3]));
        // Install consumes the bytes exactly once.
        assert_eq!(slot.take_pending(), None);
        assert_eq!(events.lock().unwrap().len(), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn stall_fires_when_headers_never_arrive() {
        let (_tx, _cancel, handle, events) = spawn_supervisor();
        // No signal at all: the request never produced headers or bytes.
        tokio::time::advance(WINDOW + Duration::from_millis(1)).await;
        assert_eq!(handle.await.unwrap(), DownloadOutcome::Stalled);
        assert!(events.lock().unwrap().is_empty());
    }

    #[test]
    fn version_mismatch_is_refused() {
        let pick = |offered: Option<&'static str>| {
            select_expected_update("2026.1014.1", offered, |version: &&str| version)
        };
        assert_eq!(pick(Some("2026.1014.1")).unwrap(), "2026.1014.1");
        let mismatch = pick(Some("2026.1015.1")).unwrap_err();
        assert_eq!(
            mismatch.code,
            IpcErrorCode::Custom(UPDATER_VERSION_MISMATCH.into())
        );
        assert_eq!(pick(None).unwrap_err().code, IpcErrorCode::NotFound);
    }

    #[test]
    fn managed_externally_returns_typed_error_without_touching_plugin() {
        let touched = std::cell::Cell::new(false);
        let error = gate_updater(true, || touched.set(true)).unwrap_err();
        assert!(!touched.get(), "the plugin path ran on an externally managed install");
        let wire = serde_json::to_value(&error).unwrap();
        assert_eq!(wire["code"], UPDATER_MANAGED_EXTERNALLY);
        assert!(gate_updater(false, || touched.set(true)).is_ok());
        assert!(touched.get());
    }

    /// Minisign keypair generated inside the test; the release key is never used.
    struct TestKey {
        signing: ed25519_dalek::SigningKey,
        key_id: [u8; 8],
    }

    impl TestKey {
        fn new(seed: u8) -> Self {
            Self {
                signing: ed25519_dalek::SigningKey::from_bytes(&[seed; 32]),
                key_id: [seed, 1, 2, 3, 4, 5, 6, 7],
            }
        }

        /// `plugins.updater.pubkey` form: base64 of the two-line minisign.pub text.
        fn tauri_pubkey(&self) -> String {
            use base64::Engine as _;
            let b64 = base64::engine::general_purpose::STANDARD;
            let mut raw = b"Ed".to_vec();
            raw.extend_from_slice(&self.key_id);
            raw.extend_from_slice(self.signing.verifying_key().as_bytes());
            b64.encode(format!(
                "untrusted comment: minisign public key\n{}\n",
                b64.encode(raw)
            ))
        }

        /// Prehashed (`ED`) minisign signature, as `minisign -S` writes it.
        fn sign(&self, data: &[u8]) -> String {
            use base64::Engine as _;
            use blake2::Digest as _;
            use ed25519_dalek::Signer as _;
            let b64 = base64::engine::general_purpose::STANDARD;
            let digest = blake2::Blake2b512::digest(data);
            let payload = self.signing.sign(digest.as_slice()).to_bytes();
            let comment = "timestamp:0\tfile:latest.json";
            let mut global = payload.to_vec();
            global.extend_from_slice(comment.as_bytes());
            let global = self.signing.sign(&global).to_bytes();
            let mut line = b"ED".to_vec();
            line.extend_from_slice(&self.key_id);
            line.extend_from_slice(&payload);
            format!(
                "untrusted comment: signature from test key\n{}\ntrusted comment: {comment}\n{}\n",
                b64.encode(line),
                b64.encode(global)
            )
        }
    }

    fn test_manifest(version: &str, extra: serde_json::Value) -> Vec<u8> {
        let mut platforms = serde_json::Map::new();
        for target in [
            "darwin-aarch64",
            "darwin-x86_64",
            "linux-x86_64",
            "linux-aarch64",
            "windows-x86_64",
            "windows-aarch64",
        ] {
            platforms.insert(
                target.into(),
                serde_json::json!({
                    "signature": "unused-by-check",
                    "url": "https://example.invalid/Ferryx.app.tar.gz",
                }),
            );
        }
        let mut manifest = serde_json::json!({
            "version": version,
            "notes": format!("Ferryx {version}"),
            "pub_date": "2026-11-04T00:00:00.000Z",
            "platforms": platforms,
        });
        if let serde_json::Value::Object(extra) = extra {
            manifest.as_object_mut().unwrap().extend(extra);
        }
        serde_json::to_vec_pretty(&manifest).unwrap()
    }

    /// What the local endpoint serves. `alternate` replaces the manifest from the second request
    /// on, to model a swap between the verified fetch and the plugin's own fetch.
    #[derive(Clone, Default)]
    struct Served {
        manifest: Vec<u8>,
        signature: Option<Vec<u8>>,
        alternate: Option<Vec<u8>>,
    }

    struct ManifestServer {
        base: String,
        hits: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        task: tokio::task::JoinHandle<()>,
    }

    async fn serve_manifest(served: Served) -> ManifestServer {
        use axum::http::StatusCode;
        use axum::response::IntoResponse;
        use std::sync::atomic::{AtomicUsize, Ordering};
        let hits = std::sync::Arc::new(AtomicUsize::new(0));
        let manifest_hits = std::sync::Arc::new(AtomicUsize::new(0));
        let router = axum::Router::new()
            .route(
                "/latest.json",
                axum::routing::get({
                    let served = served.clone();
                    let hits = hits.clone();
                    move || {
                        hits.fetch_add(1, Ordering::SeqCst);
                        let nth = manifest_hits.fetch_add(1, Ordering::SeqCst);
                        let body = match (&served.alternate, nth) {
                            (Some(alternate), 1..) => alternate.clone(),
                            _ => served.manifest.clone(),
                        };
                        async move { body }
                    }
                }),
            )
            .route(
                "/latest.json.minisig",
                axum::routing::get({
                    let hits = hits.clone();
                    move || {
                        hits.fetch_add(1, Ordering::SeqCst);
                        let response = match served.signature.clone() {
                            Some(signature) => signature.into_response(),
                            None => StatusCode::NOT_FOUND.into_response(),
                        };
                        async move { response }
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        ManifestServer { base, hits, task }
    }

    /// Mock app with the updater plugin, the test key as `plugins.updater.pubkey` and the given
    /// current version.
    fn mock_updater_app(key: &TestKey, current_version: &str) -> tauri::App<tauri::test::MockRuntime> {
        let mut context = tauri::test::mock_context(tauri::test::noop_assets());
        context.config_mut().plugins.0.insert(
            "updater".into(),
            serde_json::json!({
                "endpoints": ["https://example.invalid/latest.json"],
                "pubkey": key.tauri_pubkey(),
            }),
        );
        context.package_info_mut().version = current_version.parse().unwrap();
        tauri::test::mock_builder()
            .plugin(tauri_plugin_updater::Builder::new().build())
            .build(context)
            .unwrap()
    }

    /// Runs [`policy_checked_update`] against a local endpoint serving `served` and returns the
    /// offered version or the error code.
    fn check_against(
        key: &TestKey,
        current_version: &str,
        served: Served,
        managed_externally: bool,
    ) -> (Result<Option<String>, String>, usize) {
        let app = mock_updater_app(key, current_version);
        let handle = app.handle().clone();
        tauri::async_runtime::block_on(async move {
            let server = serve_manifest(served).await;
            let endpoint = format!("{}/latest.json", server.base);
            let result =
                policy_checked_update(&handle, managed_externally, None, Some(endpoint.as_str()))
                    .await
                    .map(|offered| offered.map(|update| update.version))
                    .map_err(|error| serde_json::to_value(&error).unwrap()["code"].to_string());
            server.task.abort();
            (result, server.hits.load(std::sync::atomic::Ordering::SeqCst))
        })
    }

    fn signed(key: &TestKey, manifest: Vec<u8>) -> Served {
        Served {
            signature: Some(key.sign(&manifest).into_bytes()),
            manifest,
            alternate: None,
        }
    }

    fn code(name: &str) -> Result<Option<String>, String> {
        Err(format!("\"{name}\""))
    }

    #[test]
    fn signed_manifest_offers_newer_version() {
        let key = TestKey::new(7);
        let served = signed(&key, test_manifest("2.1.0", serde_json::json!({ "minimumVersion": "2.0.0" })));
        let (result, _) = check_against(&key, "2.0.0", served, false);
        assert_eq!(result, Ok(Some("2.1.0".into())));
    }

    #[test]
    fn halt_switch_refuses_update() {
        let key = TestKey::new(7);
        let served = signed(
            &key,
            test_manifest("2.1.0", serde_json::json!({ "halt": true, "haltReason": "bad build" })),
        );
        let (result, _) = check_against(&key, "2.0.0", served, false);
        println!("halt manifest -> {result:?}");
        assert_eq!(result, code(UPDATE_HALTED));
    }

    #[test]
    fn offer_below_minimum_version_is_refused() {
        let key = TestKey::new(7);
        let served = signed(&key, test_manifest("2.1.0", serde_json::json!({ "minimumVersion": "2.2.0" })));
        let (result, _) = check_against(&key, "2.0.0", served, false);
        println!("below-floor manifest -> {result:?}");
        assert_eq!(result, code(UPDATE_BELOW_FLOOR));
    }

    #[test]
    fn bad_or_missing_signature_is_unverified() {
        let key = TestKey::new(7);
        let manifest = test_manifest("2.1.0", serde_json::json!({}));

        // One modified byte after signing.
        let mut tampered = signed(&key, manifest.clone());
        let last = tampered.manifest.len() - 2;
        tampered.manifest[last] ^= 0x01;
        let (result, _) = check_against(&key, "2.0.0", tampered, false);
        println!("tampered manifest -> {result:?}");
        assert_eq!(result, code(UPDATE_MANIFEST_UNVERIFIED));

        // Signed by another key.
        let other = TestKey::new(9);
        let (result, _) = check_against(&key, "2.0.0", signed(&other, manifest.clone()), false);
        assert_eq!(result, code(UPDATE_MANIFEST_UNVERIFIED));

        // Unsigned: no .minisig published.
        let unsigned = Served {
            manifest: manifest.clone(),
            ..Served::default()
        };
        let (result, _) = check_against(&key, "2.0.0", unsigned, false);
        assert_eq!(result, code(UPDATE_MANIFEST_UNVERIFIED));

        // Garbage signature body.
        let garbage = Served {
            manifest,
            signature: Some(b"not a signature".to_vec()),
            alternate: None,
        };
        let (result, _) = check_against(&key, "2.0.0", garbage, false);
        assert_eq!(result, code(UPDATE_MANIFEST_UNVERIFIED));
    }

    #[test]
    fn manifest_swapped_after_verification_is_unverified() {
        let key = TestKey::new(7);
        let mut served = signed(&key, test_manifest("2.1.0", serde_json::json!({})));
        served.alternate = Some(test_manifest("2.9.0", serde_json::json!({})));
        let (result, _) = check_against(&key, "2.0.0", served, false);
        assert_eq!(result, code(UPDATE_MANIFEST_UNVERIFIED));
    }

    #[test]
    fn older_signed_manifest_never_downgrades() {
        let key = TestKey::new(7);
        let served = signed(&key, test_manifest("1.0.0", serde_json::json!({})));
        let (result, _) = check_against(&key, "2.0.0", served, false);
        println!("1.0.0 manifest to a 2.0.0 client -> {result:?}");
        assert_eq!(result, Ok(None));
    }

    #[test]
    fn externally_managed_install_never_fetches_the_manifest() {
        let key = TestKey::new(7);
        let served = signed(&key, test_manifest("2.1.0", serde_json::json!({ "halt": true })));
        let (result, hits) = check_against(&key, "2.0.0", served, true);
        assert_eq!(result, code(UPDATER_MANAGED_EXTERNALLY));
        assert_eq!(hits, 0, "an externally managed install contacted the update endpoint");
    }

    #[test]
    fn manifest_policy_rejects_malformed_fields() {
        let policy = |extra: serde_json::Value| {
            let manifest: serde_json::Value =
                serde_json::from_slice(&test_manifest("2.1.0", extra)).unwrap();
            manifest_policy(&manifest).map_err(|error| error.code)
        };
        let unverified = IpcErrorCode::Custom(UPDATE_MANIFEST_UNVERIFIED.into());
        assert_eq!(policy(serde_json::json!({ "halt": "yes" })).unwrap_err(), unverified);
        assert_eq!(policy(serde_json::json!({ "minimumVersion": 2 })).unwrap_err(), unverified);
        assert_eq!(
            policy(serde_json::json!({ "minimumVersion": "not-semver" })).unwrap_err(),
            unverified
        );
        let parsed = policy(serde_json::json!({ "minimumVersion": "v2.0.0", "halt": null })).unwrap();
        assert_eq!(parsed.minimum_version, Some(semver::Version::new(2, 0, 0)));
        assert!(!parsed.halt);
        assert_eq!(
            manifest_policy(&serde_json::json!({ "version": "nope" })).unwrap_err().code,
            unverified
        );
        // The floor is inclusive: offering exactly the minimum version is allowed.
        let at_floor = policy(serde_json::json!({ "minimumVersion": "2.1.0" })).unwrap();
        assert!(enforce_manifest_policy(&at_floor).is_ok());
    }

    #[test]
    fn committed_updater_pubkey_decodes() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
        let pubkey = config["plugins"]["updater"]["pubkey"].as_str().unwrap();
        assert!(decode_updater_pubkey(pubkey).is_ok());
    }

    #[test]
    fn signature_url_appends_minisig_suffix() {
        let url: Url = STABLE_UPDATE_ENDPOINT.parse().unwrap();
        assert_eq!(
            manifest_signature_url(&url).as_str(),
            "https://github.com/Indosaram/ferryx/releases/latest/download/latest.json.minisig"
        );
    }

    /// QA entry: the real `cmd_updater_check` command with the debug `FERRYX_UPDATER_ENDPOINT`
    /// override, printing each reason code. The only test that sets the variable.
    #[test]
    fn qa_cmd_updater_check_reports_reason_codes() {
        let key = TestKey::new(7);
        let scenarios = [
            ("halt", "2.0.0", signed(&key, test_manifest("2.1.0", serde_json::json!({ "halt": true })))),
            (
                "floor",
                "2.0.0",
                signed(&key, test_manifest("2.1.0", serde_json::json!({ "minimumVersion": "3.0.0" }))),
            ),
            ("unsigned", "2.0.0", Served { manifest: test_manifest("2.1.0", serde_json::json!({})), ..Served::default() }),
            ("downgrade", "2.0.0", signed(&key, test_manifest("1.0.0", serde_json::json!({})))),
        ];
        let mut seen = Vec::new();
        for (name, current, served) in scenarios {
            let app = mock_updater_app(&key, current);
            let handle = app.handle().clone();
            let outcome = tauri::async_runtime::block_on(async move {
                let server = serve_manifest(served).await;
                std::env::set_var("FERRYX_UPDATER_ENDPOINT", format!("{}/latest.json", server.base));
                let result = cmd_updater_check(handle, None).await;
                std::env::remove_var("FERRYX_UPDATER_ENDPOINT");
                server.task.abort();
                match result {
                    Ok(offer) => format!("offer={:?}", offer.map(|offer| offer.version)),
                    Err(error) => format!("code={}", serde_json::to_value(&error).unwrap()["code"]),
                }
            });
            println!("QA cmd_updater_check {name}: {outcome}");
            seen.push(outcome);
        }
        assert_eq!(
            seen,
            [
                format!("code=\"{UPDATE_HALTED}\""),
                format!("code=\"{UPDATE_BELOW_FLOOR}\""),
                format!("code=\"{UPDATE_MANIFEST_UNVERIFIED}\""),
                "offer=None".to_owned(),
            ]
        );
    }
}
