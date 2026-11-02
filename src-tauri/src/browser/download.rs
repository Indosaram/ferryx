use crate::browser::{validate_url, BrowserError};
use futures_util::StreamExt;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
#[cfg(feature = "desktop")]
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncWriteExt;
use tokio::sync::{broadcast, Notify};
use uuid::Uuid;

pub const BROWSER_DOWNLOAD_UPDATED_EVENT: &str = "browser_download_updated";
const BROADCAST_CHANNEL_CAPACITY: usize = 256;
const PROGRESS_BYTE_THRESHOLD: u64 = 1024 * 1024; // 1 MiB
const PROGRESS_TIME_THRESHOLD: Duration = Duration::from_millis(250);
/// Maximum number of finished (Completed/Failed/Cancelled) records kept in the registry.
const FINISHED_RECORD_CAP: usize = 200;
const DOWNLOAD_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// Maximum idle time between two reads of the response (headers or body).
const DOWNLOAD_READ_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DownloadStatus {
    Pending,
    InProgress,
    /// Held by the engine; it can be resumed (native engine downloads only).
    Paused,
    Completed,
    Failed,
    Cancelled,
}

impl DownloadStatus {
    fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

/// Why a download failed, for the shelf to show something actionable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DownloadFailureKind {
    DiskFull,
    AccessDenied,
    Network,
    Server,
    Other,
}

impl DownloadFailureKind {
    /// Classifies a local write error; a full disk is the one the user can act on.
    pub fn of_io(error: &std::io::Error) -> Self {
        match error.kind() {
            std::io::ErrorKind::StorageFull | std::io::ErrorKind::QuotaExceeded => Self::DiskFull,
            std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem => {
                Self::AccessDenied
            }
            _ => Self::Other,
        }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadRecord {
    pub id: String,
    pub url: String,
    pub file_path: String,
    pub status: DownloadStatus,
    pub total_bytes: Option<u64>,
    pub received_bytes: u64,
    pub error: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    /// Whether Cancel reaches the download (a system-webview download cannot be stopped).
    #[serde(default = "default_true")]
    pub can_cancel: bool,
    /// Whether the engine can pause and resume it.
    #[serde(default)]
    pub can_pause: bool,
    #[serde(default)]
    pub failure: Option<DownloadFailureKind>,
}

fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProgressEmissionState {
    last_emitted_bytes: u64,
    last_emitted_percent: Option<u64>,
    last_emitted_at: Instant,
}

/// Per-download cancellation: the flag is the durable state, the notify wakes a task that is
/// blocked in `send()` or `stream.next()`.
struct CancelHandle {
    flag: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

/// Steers a download the engine itself performs (Chromium, or a system webview).
pub trait DownloadControl: Send + Sync {
    fn cancel(&self);
    fn pause(&self);
    fn resume(&self);
}

pub struct DownloadRegistry {
    records: RwLock<HashMap<String, DownloadRecord>>,
    cancellations: RwLock<HashMap<String, CancelHandle>>,
    controls: RwLock<HashMap<String, Arc<dyn DownloadControl>>>,
    progress_states: RwLock<HashMap<String, ProgressEmissionState>>,
    /// Ids of finished records, oldest first; bounded to `FINISHED_RECORD_CAP`.
    finished_order: RwLock<VecDeque<String>>,
    broadcaster: broadcast::Sender<DownloadRecord>,
}

impl Default for DownloadRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl DownloadRegistry {
    pub fn new() -> Self {
        let (broadcaster, _) = broadcast::channel(BROADCAST_CHANNEL_CAPACITY);
        Self {
            records: RwLock::new(HashMap::new()),
            cancellations: RwLock::new(HashMap::new()),
            controls: RwLock::new(HashMap::new()),
            progress_states: RwLock::new(HashMap::new()),
            finished_order: RwLock::new(VecDeque::new()),
            broadcaster,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<DownloadRecord> {
        self.broadcaster.subscribe()
    }

    fn notify(&self, record: DownloadRecord) {
        // If there are no active receivers, SendError is returned; dropping it is correct
        // because subscribers can come and go (e.g. before the Tauri listener forwards,
        // or when no window/frontend client is actively listening).
        let _ = self.broadcaster.send(record);
    }

    pub fn register(&self, id: String, url: String, file_path: String) -> Arc<AtomicBool> {
        self.register_cancellable(id, url, file_path).0
    }

    fn register_cancellable(
        &self,
        id: String,
        url: String,
        file_path: String,
    ) -> (Arc<AtomicBool>, Arc<Notify>) {
        let now = current_time_ms();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_notify = Arc::new(Notify::new());
        self.cancellations.write().insert(
            id.clone(),
            CancelHandle {
                flag: Arc::clone(&cancel_flag),
                notify: Arc::clone(&cancel_notify),
            },
        );
        // A re-registered id is live again; it must not be evicted as a finished record.
        self.finished_order.write().retain(|finished| finished != &id);
        let record = DownloadRecord {
            id: id.clone(),
            url,
            file_path,
            status: DownloadStatus::Pending,
            total_bytes: None,
            received_bytes: 0,
            error: None,
            created_at_ms: now,
            updated_at_ms: now,
            can_cancel: true,
            can_pause: false,
            failure: None,
        };
        self.insert_live(record);
        (cancel_flag, cancel_notify)
    }

    fn insert_live(&self, record: DownloadRecord) {
        let id = record.id.clone();
        self.records.write().insert(id.clone(), record.clone());
        self.progress_states.write().insert(
            id,
            ProgressEmissionState {
                last_emitted_bytes: 0,
                last_emitted_percent: None,
                last_emitted_at: Instant::now(),
            },
        );
        self.notify(record);
    }

    /// Registers a download the engine performs. With a `control`, Cancel (and Pause/Resume
    /// when `can_pause`) reach the engine; without one the record only reports progress.
    pub fn register_native(
        &self,
        id: String,
        url: String,
        file_path: String,
        total_bytes: Option<u64>,
        control: Option<Arc<dyn DownloadControl>>,
        can_pause: bool,
    ) {
        let now = current_time_ms();
        self.finished_order.write().retain(|finished| finished != &id);
        let can_cancel = control.is_some();
        match control {
            Some(control) => {
                self.controls.write().insert(id.clone(), control);
            }
            None => {
                self.controls.write().remove(&id);
            }
        }
        self.insert_live(DownloadRecord {
            id,
            url,
            file_path,
            status: DownloadStatus::InProgress,
            total_bytes,
            received_bytes: 0,
            error: None,
            created_at_ms: now,
            updated_at_ms: now,
            can_cancel,
            can_pause: can_pause && can_cancel,
            failure: None,
        });
    }

    /// Engine-reported progress: unlike [`Self::update_progress`] it also carries the total
    /// and the destination the engine settled on.
    pub fn update_native(&self, id: &str, received_bytes: u64, total_bytes: Option<u64>, file_path: Option<&str>) {
        {
            let mut records = self.records.write();
            if let Some(record) = records.get_mut(id).filter(|r| !r.status.is_terminal()) {
                if total_bytes.is_some() {
                    record.total_bytes = total_bytes;
                }
                if let Some(path) = file_path.filter(|path| !path.is_empty()) {
                    record.file_path = path.to_string();
                }
            }
        }
        self.update_progress(id, received_bytes);
    }

    /// Hands a live native download the engine control that arrived after it was registered
    /// (a system webview exposes its download object only once the download started). Cancel,
    /// and Pause/Resume when `can_pause`, then reach the engine. False when the record is gone
    /// or finished.
    pub fn attach_control(&self, id: &str, control: Arc<dyn DownloadControl>, can_pause: bool) -> bool {
        let updated = {
            let mut records = self.records.write();
            match records.get_mut(id) {
                Some(record) if !record.status.is_terminal() => {
                    self.controls.write().insert(id.to_string(), control);
                    record.can_cancel = true;
                    record.can_pause = can_pause;
                    record.updated_at_ms = current_time_ms();
                    Some(record.clone())
                }
                _ => None,
            }
        };
        match updated {
            Some(record) => {
                self.notify(record);
                true
            }
            None => false,
        }
    }

    /// Moves a live record between InProgress and Paused; emits only on a change.
    pub fn set_paused(&self, id: &str, paused: bool) {
        let target = if paused { DownloadStatus::Paused } else { DownloadStatus::InProgress };
        let updated = {
            let mut records = self.records.write();
            match records.get_mut(id) {
                Some(record) if !record.status.is_terminal() && record.status != target => {
                    record.status = target;
                    record.updated_at_ms = current_time_ms();
                    Some(record.clone())
                }
                _ => None,
            }
        };
        if let Some(record) = updated {
            self.notify(record);
        }
    }

    /// Asks the engine to pause or resume; the record changes when the engine reports it.
    /// False when the download has no engine control or cannot pause.
    pub fn request_pause(&self, id: &str, pause: bool) -> bool {
        let pausable = self
            .records
            .read()
            .get(id)
            .is_some_and(|record| record.can_pause && !record.status.is_terminal());
        let control = self.controls.read().get(id).cloned();
        match control {
            Some(control) if pausable => {
                if pause {
                    control.pause();
                } else {
                    control.resume();
                }
                true
            }
            _ => false,
        }
    }

    pub fn set_failed_kind(&self, id: &str, kind: DownloadFailureKind, error: String) {
        {
            let mut records = self.records.write();
            if let Some(record) = records.get_mut(id).filter(|r| !r.status.is_terminal()) {
                record.failure = Some(kind);
            }
        }
        self.set_failed(id, error);
    }

    pub fn set_started(&self, id: &str, total_bytes: Option<u64>) {
        self.set_started_at(id, total_bytes, Instant::now());
    }

    fn set_started_at(&self, id: &str, total_bytes: Option<u64>, now: Instant) {
        let updated_record = {
            let mut records = self.records.write();
            if let Some(record) = records.get_mut(id).filter(|r| !r.status.is_terminal()) {
                record.status = DownloadStatus::InProgress;
                record.total_bytes = total_bytes;
                record.updated_at_ms = current_time_ms();
                Some(record.clone())
            } else {
                None
            }
        };

        if let Some(record) = updated_record {
            {
                let mut pstates = self.progress_states.write();
                if let Some(pstate) = pstates.get_mut(id) {
                    pstate.last_emitted_bytes = record.received_bytes;
                    pstate.last_emitted_percent = record
                        .total_bytes
                        .filter(|&total| total > 0)
                        .map(|total| (record.received_bytes * 100) / total);
                    pstate.last_emitted_at = now;
                }
            }
            self.notify(record);
        }
    }

    pub fn update_progress(&self, id: &str, received_bytes: u64) {
        self.update_progress_at(id, received_bytes, Instant::now());
    }

    fn update_progress_at(&self, id: &str, received_bytes: u64, now: Instant) {
        let maybe_to_notify = {
            let mut records = self.records.write();
            let mut pstates = self.progress_states.write();

            if let (Some(record), Some(pstate)) = (
                records.get_mut(id).filter(|r| !r.status.is_terminal()),
                pstates.get_mut(id),
            ) {
                record.received_bytes = received_bytes;
                record.updated_at_ms = current_time_ms();

                let byte_delta = received_bytes.saturating_sub(pstate.last_emitted_bytes);
                let time_elapsed = now.duration_since(pstate.last_emitted_at);

                let current_percent = record
                    .total_bytes
                    .filter(|&total| total > 0)
                    .map(|total| (received_bytes * 100) / total);

                let percent_changed = match (pstate.last_emitted_percent, current_percent) {
                    (Some(last_p), Some(curr_p)) => curr_p > last_p,
                    (None, Some(_)) => true,
                    _ => false,
                };

                // Bounded emission: integer percentage change, >= 1 MiB boundary, OR >= 250ms elapsed
                let should_emit = percent_changed
                    || byte_delta >= PROGRESS_BYTE_THRESHOLD
                    || time_elapsed >= PROGRESS_TIME_THRESHOLD;

                if should_emit {
                    pstate.last_emitted_bytes = received_bytes;
                    pstate.last_emitted_percent = current_percent;
                    pstate.last_emitted_at = now;
                    Some(record.clone())
                } else {
                    None
                }
            } else {
                None
            }
        };

        if let Some(record) = maybe_to_notify {
            self.notify(record);
        }
    }

    /// Moves a live record into a terminal state exactly once. Later terminal transitions
    /// are no-ops, so the first outcome (e.g. a user cancel) is never overwritten.
    fn finish(&self, id: &str, status: DownloadStatus, error: Option<String>) {
        let updated_record = {
            let mut records = self.records.write();
            match records.get_mut(id) {
                Some(record) if !record.status.is_terminal() => {
                    record.status = status;
                    if error.is_some() {
                        record.error = error;
                    }
                    record.updated_at_ms = current_time_ms();
                    Some(record.clone())
                }
                _ => None,
            }
        };
        let Some(record) = updated_record else {
            return;
        };
        self.cancellations.write().remove(id);
        self.controls.write().remove(id);
        self.progress_states.write().remove(id);
        self.record_finished(id);
        self.notify(record);
    }

    /// Remembers a finished id and evicts the oldest finished records beyond the cap.
    fn record_finished(&self, id: &str) {
        let evicted: Vec<String> = {
            let mut order = self.finished_order.write();
            order.push_back(id.to_string());
            let excess = order.len().saturating_sub(FINISHED_RECORD_CAP);
            order.drain(..excess).collect()
        };
        if evicted.is_empty() {
            return;
        }
        let mut records = self.records.write();
        for old_id in evicted {
            records.remove(&old_id);
        }
    }

    pub fn set_completed(&self, id: &str) {
        self.finish(id, DownloadStatus::Completed, None);
    }

    /// The destination rename succeeded: the file is committed, so the record becomes
    /// Completed even when a cancel landed between the rename and this call. Once a record
    /// is Completed it is terminal, so later cancels are no-ops.
    pub fn set_committed(&self, id: &str) {
        let raced_cancel = {
            let mut records = self.records.write();
            match records.get_mut(id) {
                Some(record) if record.status == DownloadStatus::Cancelled => {
                    record.status = DownloadStatus::Completed;
                    record.error = None;
                    record.updated_at_ms = current_time_ms();
                    Some(record.clone())
                }
                _ => None,
            }
        };
        match raced_cancel {
            // finish() already ran the terminal bookkeeping for the cancel.
            Some(record) => self.notify(record),
            None => self.set_completed(id),
        }
    }

    pub fn set_failed(&self, id: &str, error: String) {
        self.finish(id, DownloadStatus::Failed, Some(error));
    }

    /// The engine reports a native download as failed. Engines report the stop a pause causes
    /// as a failure, so a Paused record stays Paused; a cancelled one stays Cancelled.
    pub fn set_engine_failed(&self, id: &str, error: String) {
        let paused = self.records.read().get(id).is_some_and(|record| record.status == DownloadStatus::Paused);
        if !paused {
            self.set_failed(id, error);
        }
    }

    pub fn set_cancelled(&self, id: &str) {
        self.finish(
            id,
            DownloadStatus::Cancelled,
            Some("download cancelled".to_string()),
        );
    }

    pub fn cancel(&self, id: &str) -> bool {
        let control = self.controls.read().get(id).cloned();
        if let Some(control) = control {
            control.cancel();
            self.set_cancelled(id);
            return true;
        }
        let found = {
            let map = self.cancellations.read();
            match map.get(id) {
                Some(handle) => {
                    handle.flag.store(true, Ordering::SeqCst);
                    // notify_one stores a permit, so a task that is between two awaits
                    // still observes the cancel at its next select.
                    handle.notify.notify_one();
                    true
                }
                None => false,
            }
        };
        if found {
            self.set_cancelled(id);
        }
        found
    }

    pub fn list(&self) -> Vec<DownloadRecord> {
        let mut list: Vec<DownloadRecord> = self.records.read().values().cloned().collect();
        list.sort_by_key(|record| std::cmp::Reverse(record.created_at_ms));
        list
    }

    pub fn get(&self, id: &str) -> Option<DownloadRecord> {
        self.records.read().get(id).cloned()
    }
}

static DOWNLOAD_REGISTRY: LazyLock<DownloadRegistry> = LazyLock::new(DownloadRegistry::new);

pub fn subscribe_download_updates() -> broadcast::Receiver<DownloadRecord> {
    DOWNLOAD_REGISTRY.subscribe()
}

#[cfg(feature = "desktop")]
pub fn register_download_event_forwarder<R: tauri::Runtime>(app: &AppHandle<R>) {
    let app_handle = app.clone();
    let mut rx = subscribe_download_updates();
    tauri::async_runtime::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(record) => {
                    if let Err(err) = app_handle.emit(BROWSER_DOWNLOAD_UPDATED_EVENT, &record) {
                        tracing::warn!(
                            "failed to emit browser download update for {}: {err}",
                            record.id
                        );
                    }
                }
                Err(broadcast::error::RecvError::Lagged(missed)) => {
                    tracing::debug!(
                        missed,
                        "download update broadcast receiver lagged; continuing"
                    );
                }
                Err(broadcast::error::RecvError::Closed) => {
                    tracing::debug!("download update broadcast channel closed; stopping forwarder");
                    break;
                }
            }
        }
    });
}

pub fn list_downloads() -> Vec<DownloadRecord> {
    DOWNLOAD_REGISTRY.list()
}

pub fn get_download(id: &str) -> Option<DownloadRecord> {
    DOWNLOAD_REGISTRY.get(id)
}

pub fn cancel_download(id: &str) -> bool {
    DOWNLOAD_REGISTRY.cancel(id)
}

pub fn pause_download(id: &str, pause: bool) -> bool {
    DOWNLOAD_REGISTRY.request_pause(id, pause)
}

/// The process-wide registry the shelf reads; native engine downloads report here too.
pub fn registry() -> &'static DownloadRegistry {
    &DOWNLOAD_REGISTRY
}

/// Sibling temp file (`<path>.<download-id>.part`) the body streams into before the
/// final rename; the id keeps concurrent downloads to one destination apart.
fn part_path_for(path: &Path, download_id: &str) -> PathBuf {
    let mut part = path.as_os_str().to_owned();
    part.push(format!(".{download_id}.part"));
    PathBuf::from(part)
}

async fn remove_part_file(part_path: &Path) {
    if let Err(e) = tokio::fs::remove_file(part_path).await {
        if e.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(
                "failed to clean up partial download file {}: {e}",
                part_path.display()
            );
        }
    }
}

/// Ends a download as cancelled: removes the temp file and records the cancel.
async fn abort_cancelled(id: &str, part_path: Option<&Path>) -> BrowserError {
    if let Some(part) = part_path {
        remove_part_file(part).await;
    }
    DOWNLOAD_REGISTRY.set_cancelled(id);
    BrowserError::DownloadFailed("download cancelled".into())
}

/// Ends a download as failed: removes the temp file and records the error.
async fn abort_failed(id: &str, part_path: Option<&Path>, err_msg: String) -> BrowserError {
    if let Some(part) = part_path {
        remove_part_file(part).await;
    }
    DOWNLOAD_REGISTRY.set_failed(id, err_msg.clone());
    BrowserError::DownloadFailed(err_msg)
}

/// Ends a download as failed by a local write error, keeping a full disk recognisable.
async fn abort_io(id: &str, part_path: Option<&Path>, error: std::io::Error) -> BrowserError {
    if let Some(part) = part_path {
        remove_part_file(part).await;
    }
    let message = error.to_string();
    DOWNLOAD_REGISTRY.set_failed_kind(id, DownloadFailureKind::of_io(&error), message.clone());
    BrowserError::DownloadFailed(message)
}

/// Ids become part of the `<path>.<id>.part` file name, so they must stay one plain file-name
/// fragment: a separator or `..` would move the temp file out of the destination directory.
fn is_valid_download_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b':'))
}

/// Rejects a destination that cannot name a file, and an unusable caller-chosen id, before any
/// network request or registry entry exists.
fn validate_download_target(path: &Path, download_id: Option<&str>) -> Result<(), BrowserError> {
    if path.file_name().is_none() {
        return Err(BrowserError::DownloadFailed(
            "download destination must name a file".into(),
        ));
    }
    if let Some(id) = download_id {
        if !is_valid_download_id(id) {
            return Err(BrowserError::DownloadFailed(
                "download id must be 1-128 characters of letters, digits, '-', '_' or ':'".into(),
            ));
        }
    }
    Ok(())
}

pub async fn download_url_with_cookies(
    url: &str,
    path: &Path,
    cookies: Option<&str>,
    download_id: Option<&str>,
) -> Result<String, BrowserError> {
    let url = validate_url(url)?;
    if url == "about:blank" {
        return Err(BrowserError::DownloadFailed(
            "about:blank cannot be downloaded".into(),
        ));
    }
    validate_download_target(path, download_id)?;

    let id = download_id
        .map(|s| s.to_string())
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    let path_str = path.to_string_lossy().to_string();
    let (cancel_flag, cancel_notify) =
        DOWNLOAD_REGISTRY.register_cancellable(id.clone(), url.clone(), path_str);
    let is_cancelled = || cancel_flag.load(Ordering::SeqCst);

    let client = match reqwest::Client::builder()
        .connect_timeout(DOWNLOAD_CONNECT_TIMEOUT)
        .read_timeout(DOWNLOAD_READ_TIMEOUT)
        .build()
    {
        Ok(client) => client,
        Err(e) => {
            let err_msg = format!("failed to build reqwest client: {e}");
            return Err(abort_failed(&id, None, err_msg).await);
        }
    };

    let mut req = client.get(&url);
    if let Some(cookie_str) = cookies {
        if !cookie_str.trim().is_empty() {
            req = req.header(reqwest::header::COOKIE, cookie_str.trim());
        }
    }

    if is_cancelled() {
        return Err(abort_cancelled(&id, None).await);
    }
    let send_result = tokio::select! {
        _ = cancel_notify.notified() => None,
        res = req.send() => Some(res),
    };
    let response = match send_result {
        None => return Err(abort_cancelled(&id, None).await),
        Some(Ok(res)) => res,
        Some(Err(err)) => return Err(abort_failed(&id, None, err.to_string()).await),
    };

    if !response.status().is_success() {
        let err_msg = format!("server returned HTTP {}", response.status());
        return Err(abort_failed(&id, None, err_msg).await);
    }

    if is_cancelled() {
        return Err(abort_cancelled(&id, None).await);
    }
    let total_bytes = response.content_length();
    DOWNLOAD_REGISTRY.set_started(&id, total_bytes);

    if let Some(parent) = path.parent() {
        if let Err(e) = tokio::fs::create_dir_all(parent).await {
            return Err(abort_io(&id, None, e).await);
        }
    }

    // Stream into a sibling temp file so a failed or cancelled download never truncates
    // an existing file at the destination; the rename below is the only commit point.
    let part_path = part_path_for(path, &id);
    let mut file = match tokio::fs::File::create(&part_path).await {
        Ok(f) => f,
        Err(e) => {
            return Err(abort_io(&id, Some(&part_path), e).await);
        }
    };

    let mut stream = response.bytes_stream();
    let mut received: u64 = 0;

    loop {
        if is_cancelled() {
            drop(file);
            return Err(abort_cancelled(&id, Some(&part_path)).await);
        }
        let next = tokio::select! {
            _ = cancel_notify.notified() => {
                drop(file);
                return Err(abort_cancelled(&id, Some(&part_path)).await);
            }
            next = stream.next() => next,
        };
        let chunk = match next {
            None => break,
            Some(Ok(c)) => c,
            Some(Err(e)) => {
                drop(file);
                return Err(abort_failed(&id, Some(&part_path), e.to_string()).await);
            }
        };

        received += chunk.len() as u64;
        if let Err(e) = file.write_all(&chunk).await {
            drop(file);
            return Err(abort_io(&id, Some(&part_path), e).await);
        }
        DOWNLOAD_REGISTRY.update_progress(&id, received);
    }

    if let Err(e) = file.flush().await {
        drop(file);
        return Err(abort_io(&id, Some(&part_path), e).await);
    }
    drop(file);

    // A cancel that raced the last chunk (or an empty body) must not be reported as Completed.
    if is_cancelled() {
        return Err(abort_cancelled(&id, Some(&part_path)).await);
    }
    if let Err(e) = tokio::fs::rename(&part_path, path).await {
        return Err(abort_io(&id, Some(&part_path), e).await);
    }

    DOWNLOAD_REGISTRY.set_committed(&id);
    Ok(id)
}

pub async fn download_url_to_path(url: &str, path: &Path) -> Result<(), BrowserError> {
    download_url_with_cookies(url, path, None, None).await.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_download_registry_lifecycle() {
        let registry = DownloadRegistry::new();
        let cancel_flag = registry.register(
            "dl-1".into(),
            "https://example.com/file.zip".into(),
            "/tmp/file.zip".into(),
        );

        assert!(!cancel_flag.load(Ordering::SeqCst));
        let rec = registry.get("dl-1").expect("must exist");
        assert_eq!(rec.status, DownloadStatus::Pending);
        assert_eq!(rec.received_bytes, 0);

        registry.set_started("dl-1", Some(1024));
        let rec = registry.get("dl-1").unwrap();
        assert_eq!(rec.status, DownloadStatus::InProgress);
        assert_eq!(rec.total_bytes, Some(1024));

        registry.update_progress("dl-1", 512);
        let rec = registry.get("dl-1").unwrap();
        assert_eq!(rec.received_bytes, 512);

        registry.set_completed("dl-1");
        let rec = registry.get("dl-1").unwrap();
        assert_eq!(rec.status, DownloadStatus::Completed);

        let list = registry.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "dl-1");
    }

    #[test]
    fn test_download_registry_cancellation() {
        let registry = DownloadRegistry::new();
        let cancel_flag = registry.register(
            "dl-2".into(),
            "https://example.com/data.tar".into(),
            "/tmp/data.tar".into(),
        );

        assert!(!cancel_flag.load(Ordering::SeqCst));
        let cancelled = registry.cancel("dl-2");
        assert!(cancelled);
        assert!(cancel_flag.load(Ordering::SeqCst));

        let rec = registry.get("dl-2").unwrap();
        assert_eq!(rec.status, DownloadStatus::Cancelled);
        assert_eq!(rec.error.as_deref(), Some("download cancelled"));
    }

    #[test]
    fn test_download_registry_failure() {
        let registry = DownloadRegistry::new();
        registry.register(
            "dl-3".into(),
            "https://example.com/notfound.txt".into(),
            "/tmp/notfound.txt".into(),
        );

        registry.set_failed("dl-3", "server returned HTTP 404".into());
        let rec = registry.get("dl-3").unwrap();
        assert_eq!(rec.status, DownloadStatus::Failed);
        assert_eq!(rec.error.as_deref(), Some("server returned HTTP 404"));
    }

    #[test]
    fn test_download_serde_roundtrip() {
        let rec = DownloadRecord {
            id: "dl-99".into(),
            url: "https://example.com/a.pdf".into(),
            file_path: "/tmp/a.pdf".into(),
            status: DownloadStatus::InProgress,
            total_bytes: Some(2048),
            received_bytes: 1024,
            error: None,
            created_at_ms: 1000,
            updated_at_ms: 1050,
            can_cancel: true,
            can_pause: false,
            failure: None,
        };

        let json = serde_json::to_string(&rec).unwrap();
        assert!(json.contains("\"status\":\"inProgress\""));
        assert!(json.contains("\"totalBytes\":2048"));
        let parsed: DownloadRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, rec);
    }

    #[tokio::test]
    async fn test_download_url_rejects_about_blank() {
        let result = download_url_to_path("about:blank", Path::new("/tmp/test_blank")).await;
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            BrowserError::DownloadFailed("about:blank cannot be downloaded".into())
        );
    }

    /// A destination without a file name, or an id that could escape the destination
    /// directory through the `.part` name, is refused before any request is made.
    #[tokio::test]
    async fn test_download_rejects_unusable_destination_and_unsafe_id() {
        let url = "https://example.invalid/file.bin";
        for path in [Path::new(""), Path::new("/"), Path::new("/tmp/..")] {
            let result = download_url_with_cookies(url, path, None, None).await;
            assert_eq!(
                result,
                Err(BrowserError::DownloadFailed(
                    "download destination must name a file".into()
                )),
                "{path:?}"
            );
        }
        for id in ["../escape", "a/b", "a\\b", "", "dl id"] {
            let result =
                download_url_with_cookies(url, Path::new("/tmp/ok.bin"), None, Some(id)).await;
            assert!(
                matches!(&result, Err(BrowserError::DownloadFailed(m)) if m.contains("download id")),
                "{id:?}: {result:?}"
            );
        }
        assert!(get_download("../escape").is_none(), "no record for a refused id");
    }

    #[tokio::test]
    async fn test_download_registry_lifecycle_broadcast_events() {
        let registry = DownloadRegistry::new();
        // Exact subscription BEFORE any mutation
        let mut rx = registry.subscribe();

        let _cancel_flag = registry.register(
            "dl-event-1".into(),
            "https://example.com/file.bin".into(),
            "/tmp/file.bin".into(),
        );

        let event1 = tokio::time::timeout(Duration::from_millis(500), rx.recv())
            .await
            .expect("timeout waiting for register event")
            .expect("recv error");
        assert_eq!(event1.id, "dl-event-1");
        assert_eq!(event1.status, DownloadStatus::Pending);

        registry.set_started("dl-event-1", Some(100));
        let event2 = tokio::time::timeout(Duration::from_millis(500), rx.recv())
            .await
            .expect("timeout waiting for started event")
            .expect("recv error");
        assert_eq!(event2.id, "dl-event-1");
        assert_eq!(event2.status, DownloadStatus::InProgress);
        assert_eq!(event2.total_bytes, Some(100));

        registry.set_completed("dl-event-1");
        let event3 = tokio::time::timeout(Duration::from_millis(500), rx.recv())
            .await
            .expect("timeout waiting for completed event")
            .expect("recv error");
        assert_eq!(event3.id, "dl-event-1");
        assert_eq!(event3.status, DownloadStatus::Completed);
    }

    #[tokio::test]
    async fn test_download_registry_bounded_progress_thresholds() {
        let registry = DownloadRegistry::new();
        // Exact subscription BEFORE mutation
        let mut rx = registry.subscribe();
        // Frozen clock: every call uses the same instant, so the 250ms time threshold can
        // never fire and only the percent/byte thresholds are exercised.
        let frozen = Instant::now();

        let _cancel_flag = registry.register(
            "dl-progress-1".into(),
            "https://example.com/large.iso".into(),
            "/tmp/large.iso".into(),
        );
        let _ = rx.recv().await.unwrap(); // register

        registry.set_started_at("dl-progress-1", Some(100_000_000), frozen); // 100 MB -> 1% is 1,000,000 bytes
        let _ = rx.recv().await.unwrap(); // started

        // Update with small chunk: 100 bytes (0% change, < 1 MiB, < 250ms) -> should NOT emit
        registry.update_progress_at("dl-progress-1", 100, frozen);
        let try_res = rx.try_recv();
        assert!(
            try_res.is_err(),
            "small sub-threshold progress chunk must not emit"
        );

        // Update crossing 1% (1,000,000 bytes) -> should emit
        registry.update_progress_at("dl-progress-1", 1_000_000, frozen);
        let event = tokio::time::timeout(Duration::from_millis(500), rx.recv())
            .await
            .expect("timeout waiting for progress event on percent threshold")
            .expect("recv error");
        assert_eq!(event.received_bytes, 1_000_000);

        // Sub-threshold again: 1,000,100 bytes (same percent, delta 100 bytes) -> should NOT emit
        registry.update_progress_at("dl-progress-1", 1_000_100, frozen);
        assert!(rx.try_recv().is_err());

        // Update crossing 1 MiB delta (1,000,100 + 1,048,576) without totalBytes (e.g. None)
        registry.register(
            "dl-progress-unknown-len".into(),
            "https://example.com/stream".into(),
            "/tmp/stream".into(),
        );
        let _ = rx.recv().await.unwrap();
        registry.set_started_at("dl-progress-unknown-len", None, frozen);
        let _ = rx.recv().await.unwrap();

        // 500 KiB -> no emit
        registry.update_progress_at("dl-progress-unknown-len", 512 * 1024, frozen);
        assert!(rx.try_recv().is_err());

        // 1.5 MiB -> crosses 1 MiB boundary -> emit
        registry.update_progress_at("dl-progress-unknown-len", 1536 * 1024, frozen);
        let event_mib = tokio::time::timeout(Duration::from_millis(500), rx.recv())
            .await
            .expect("timeout waiting for 1MiB progress event")
            .expect("recv error");
        assert_eq!(event_mib.id, "dl-progress-unknown-len");
        assert_eq!(event_mib.received_bytes, 1536 * 1024);
    }

    #[tokio::test]
    async fn test_download_registry_cancellation_emits_cancelled_event() {
        let registry = DownloadRegistry::new();
        let mut rx = registry.subscribe();

        let _cancel_flag = registry.register(
            "dl-cancel-1".into(),
            "https://example.com/archive.tar".into(),
            "/tmp/archive.tar".into(),
        );
        let _ = rx.recv().await.unwrap();

        registry.set_started("dl-cancel-1", Some(50_000));
        let _ = rx.recv().await.unwrap();

        let cancelled = registry.cancel("dl-cancel-1");
        assert!(cancelled);

        let event = tokio::time::timeout(Duration::from_millis(500), rx.recv())
            .await
            .expect("timeout waiting for cancel event")
            .expect("recv error");
        assert_eq!(event.id, "dl-cancel-1");
        assert_eq!(event.status, DownloadStatus::Cancelled);
        assert_eq!(event.error.as_deref(), Some("download cancelled"));
    }

    #[tokio::test]
    async fn test_download_registry_failure_emits_failed_event() {
        let registry = DownloadRegistry::new();
        let mut rx = registry.subscribe();

        registry.register(
            "dl-fail-1".into(),
            "https://example.com/fail.zip".into(),
            "/tmp/fail.zip".into(),
        );
        let _ = rx.recv().await.unwrap();

        registry.set_failed("dl-fail-1", "connection reset by peer".into());
        let event = tokio::time::timeout(Duration::from_millis(500), rx.recv())
            .await
            .expect("timeout waiting for failed event")
            .expect("recv error");
        assert_eq!(event.id, "dl-fail-1");
        assert_eq!(event.status, DownloadStatus::Failed);
        assert_eq!(event.error.as_deref(), Some("connection reset by peer"));
    }

    #[test]
    fn test_download_registry_no_deadlock_on_rapid_transitions() {
        let registry = DownloadRegistry::new();
        let mut rx = registry.subscribe();

        let _flag = registry.register("dl-deadlock".into(), "https://a.com".into(), "/tmp/a".into());
        registry.set_started("dl-deadlock", Some(100));
        registry.update_progress("dl-deadlock", 50);
        assert!(registry.cancel("dl-deadlock"));

        // All transitions should be drained without locking or hanging
        let mut count = 0;
        while let Ok(_rec) = rx.try_recv() {
            count += 1;
        }
        assert!(count >= 3);
    }

    // ---- Regression tests for the 2026-10-13 browser audit (A-CORE-3/4/5/6/11) ----

    use tokio::io::AsyncReadExt;
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;

    const SIGNAL_TIMEOUT: Duration = Duration::from_secs(10);

    enum StubBehavior {
        /// Read the request and never write a response.
        NeverRespond,
        /// Send headers announcing a large body, then never write any body byte.
        StallInBody,
        /// Send headers announcing a large body, write a few bytes, then close the socket.
        DropMidBody,
    }

    /// In-process HTTP stub. Returns the URL and a receiver fired once the request has been
    /// read (and, for body behaviors, once the partial body has been written).
    async fn spawn_http_stub(behavior: StubBehavior) -> (String, oneshot::Receiver<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind stub");
        let addr = listener.local_addr().expect("stub addr");
        let (ready_tx, ready_rx) = oneshot::channel();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut buf = vec![0u8; 4096];
            let mut request = Vec::new();
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = socket.read(&mut buf).await.expect("read request");
                if n == 0 {
                    return;
                }
                request.extend_from_slice(&buf[..n]);
            }
            match behavior {
                StubBehavior::NeverRespond => {
                    let _ = ready_tx.send(());
                    std::future::pending::<()>().await;
                }
                StubBehavior::StallInBody => {
                    socket
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 100000\r\nContent-Type: application/octet-stream\r\n\r\n",
                        )
                        .await
                        .expect("write response headers");
                    socket.flush().await.expect("flush response headers");
                    let _ = ready_tx.send(());
                    std::future::pending::<()>().await;
                }
                StubBehavior::DropMidBody => {
                    socket
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 100000\r\nContent-Type: application/octet-stream\r\n\r\npartial-body",
                        )
                        .await
                        .expect("write partial response");
                    socket.flush().await.expect("flush partial response");
                    let _ = ready_tx.send(());
                    drop(socket);
                }
            }
        });
        (format!("http://{addr}/file.bin"), ready_rx)
    }

    fn part_path_of(path: &Path, id: &str) -> std::path::PathBuf {
        let mut s = path.as_os_str().to_owned();
        s.push(format!(".{id}.part"));
        std::path::PathBuf::from(s)
    }

    /// m4: each download streams into its own `<path>.<id>.part`, so two downloads to
    /// one destination never share (and truncate) a temp file, nor clobber `<path>.part`.
    #[test]
    fn part_path_is_unique_per_download() {
        let dest = Path::new("/tmp/ferryx-dl/file.bin");
        let a = part_path_for(dest, "dl-a");
        let b = part_path_for(dest, "dl-b");
        assert_ne!(a, b);
        assert_eq!(a, part_path_of(dest, "dl-a"));
        assert_eq!(a.parent(), dest.parent());
        assert_ne!(a, Path::new("/tmp/ferryx-dl/file.bin.part"));
    }

    /// A-CORE-3: once cancelled, later lifecycle calls must not flip the status or emit.
    #[test]
    fn test_download_registry_terminal_state_is_sticky_after_cancel() {
        let registry = DownloadRegistry::new();
        registry.register(
            "dl-terminal".into(),
            "https://example.com/t.bin".into(),
            "/tmp/t.bin".into(),
        );
        assert!(registry.cancel("dl-terminal"));
        let mut rx = registry.subscribe();

        registry.set_started("dl-terminal", Some(10));
        registry.update_progress("dl-terminal", 10);
        registry.set_completed("dl-terminal");
        registry.set_failed("dl-terminal", "connection reset".into());

        let rec = registry.get("dl-terminal").expect("record");
        assert_eq!(rec.status, DownloadStatus::Cancelled);
        assert_eq!(rec.error.as_deref(), Some("download cancelled"));
        assert_eq!(rec.received_bytes, 0);
        assert!(
            rx.try_recv().is_err(),
            "no event may be emitted after a terminal state"
        );

        // Completed is terminal too: a late failure must not overwrite it.
        registry.register(
            "dl-terminal-2".into(),
            "https://example.com/t2.bin".into(),
            "/tmp/t2.bin".into(),
        );
        registry.set_completed("dl-terminal-2");
        registry.set_failed("dl-terminal-2", "late".into());
        registry.set_started("dl-terminal-2", None);
        assert_eq!(
            registry.get("dl-terminal-2").unwrap().status,
            DownloadStatus::Completed
        );
    }

    /// n3: the rename is the commit point. A cancel that lands after the rename but before the
    /// record is completed must not leave a Cancelled record beside a complete destination file.
    #[test]
    fn test_download_commit_wins_over_cancel_that_raced_the_rename() {
        let registry = DownloadRegistry::new();
        registry.register(
            "dl-commit-race".into(),
            "https://example.com/c.bin".into(),
            "/tmp/c.bin".into(),
        );
        registry.set_started("dl-commit-race", Some(4));
        registry.update_progress("dl-commit-race", 4);
        // The task has renamed the part file into place; the cancel lands now.
        assert!(registry.cancel("dl-commit-race"));
        assert_eq!(
            registry.get("dl-commit-race").unwrap().status,
            DownloadStatus::Cancelled
        );
        let mut rx = registry.subscribe();

        registry.set_committed("dl-commit-race");

        let rec = registry.get("dl-commit-race").expect("record");
        assert_eq!(rec.status, DownloadStatus::Completed);
        assert_eq!(rec.error, None);
        let event = rx.try_recv().expect("the committed outcome must be published");
        assert_eq!(event.status, DownloadStatus::Completed);
        assert!(rx.try_recv().is_err(), "exactly one event");
        // A committed download is no longer cancellable.
        assert!(!registry.cancel("dl-commit-race"));
        assert_eq!(
            registry.get("dl-commit-race").unwrap().status,
            DownloadStatus::Completed
        );
        // Committing is a plain completion when no cancel raced in.
        registry.register(
            "dl-commit-plain".into(),
            "https://example.com/p.bin".into(),
            "/tmp/p.bin".into(),
        );
        registry.set_committed("dl-commit-plain");
        assert_eq!(
            registry.get("dl-commit-plain").unwrap().status,
            DownloadStatus::Completed
        );
    }

    /// A-CORE-4: a download that dies mid-body must leave the existing destination intact
    /// and must not leave a partial file behind.
    #[tokio::test]
    async fn test_download_failure_preserves_existing_destination() {
        let dir = tempfile::tempdir().expect("tempdir");
        let dest = dir.path().join("existing.bin");
        std::fs::write(&dest, b"original contents").expect("seed destination");

        let (url, ready) = spawn_http_stub(StubBehavior::DropMidBody).await;
        let result = tokio::time::timeout(
            SIGNAL_TIMEOUT,
            download_url_with_cookies(&url, &dest, None, Some("dl-acore4-drop")),
        )
        .await
        .expect("download must finish after the server drops the connection");
        tokio::time::timeout(SIGNAL_TIMEOUT, ready)
            .await
            .expect("stub ready signal")
            .expect("stub ready sender");

        assert!(result.is_err(), "truncated body must fail: {result:?}");
        assert_eq!(
            std::fs::read(&dest).expect("destination still readable"),
            b"original contents"
        );
        assert!(!part_path_of(&dest, "dl-acore4-drop").exists(), "no .part file may remain");
        assert_eq!(
            get_download("dl-acore4-drop").unwrap().status,
            DownloadStatus::Failed
        );
    }

    async fn assert_cancel_unblocks(behavior: StubBehavior, id: &'static str) {
        let dir = tempfile::tempdir().expect("tempdir");
        let dest = dir.path().join("stalled.bin");
        let wait_for_body = matches!(behavior, StubBehavior::StallInBody);
        let mut updates = subscribe_download_updates();
        let (url, ready) = spawn_http_stub(behavior).await;

        let dest_for_task = dest.clone();
        let task = tokio::spawn(async move {
            download_url_with_cookies(&url, &dest_for_task, None, Some(id)).await
        });
        tokio::time::timeout(SIGNAL_TIMEOUT, ready)
            .await
            .expect("stub must receive the request")
            .expect("stub ready sender");
        if wait_for_body {
            // The task is past send() once it publishes InProgress; with no body byte ever
            // written it is now parked in stream.next().
            tokio::time::timeout(SIGNAL_TIMEOUT, async {
                loop {
                    match updates.recv().await {
                        Ok(rec) if rec.id == id && rec.status == DownloadStatus::InProgress => break,
                        Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(broadcast::error::RecvError::Closed) => panic!("updates closed"),
                    }
                }
            })
            .await
            .expect("download must reach InProgress");
        }

        assert!(cancel_download(id), "download must be registered and cancellable");
        let result = tokio::time::timeout(SIGNAL_TIMEOUT, task)
            .await
            .expect("cancel must wake a stalled download task")
            .expect("download task join");
        assert_eq!(
            result,
            Err(BrowserError::DownloadFailed("download cancelled".into()))
        );
        assert_eq!(get_download(id).unwrap().status, DownloadStatus::Cancelled);
        assert!(!dest.exists(), "cancelled download must not create the destination");
        assert!(!part_path_of(&dest, id).exists(), "no .part file may remain");
    }

    /// A-CORE-5: cancel must wake a download stalled in send() (server never responds).
    #[tokio::test]
    async fn test_download_cancel_wakes_stalled_send() {
        assert_cancel_unblocks(StubBehavior::NeverRespond, "dl-acore5-send").await;
    }

    /// A-CORE-5: cancel must wake a download stalled mid-body (stream.next() never yields).
    #[tokio::test]
    async fn test_download_cancel_wakes_stalled_body() {
        assert_cancel_unblocks(StubBehavior::StallInBody, "dl-acore5-body").await;
    }

    /// A-CORE-6: finished records are bounded; live records are never evicted.
    #[test]
    fn test_download_registry_evicts_oldest_finished_beyond_cap() {
        let registry = DownloadRegistry::new();
        registry.register("live".into(), "https://example.com/l".into(), "/tmp/l".into());
        registry.set_started("live", None);

        let total = FINISHED_RECORD_CAP + 25;
        for i in 0..total {
            let id = format!("done-{i}");
            registry.register(id.clone(), "https://example.com/d".into(), "/tmp/d".into());
            if i % 2 == 0 {
                registry.set_completed(&id);
            } else {
                registry.set_failed(&id, "boom".into());
            }
        }

        let list = registry.list();
        let finished = list
            .iter()
            .filter(|r| r.status != DownloadStatus::InProgress)
            .count();
        assert_eq!(finished, FINISHED_RECORD_CAP);
        assert!(registry.get("live").is_some(), "live record must survive");
        assert!(registry.get("done-0").is_none(), "oldest finished must be evicted");
        assert!(registry.get(&format!("done-{}", 24)).is_none());
        assert!(registry.get(&format!("done-{}", 25)).is_some());
        assert!(registry.get(&format!("done-{}", total - 1)).is_some());
    }

    /// A-CORE-11: the time threshold is driven by an injected Instant, not the wall clock.
    #[test]
    fn test_download_registry_time_threshold_uses_injected_clock() {
        let registry = DownloadRegistry::new();
        let mut rx = registry.subscribe();
        let base = Instant::now();
        registry.register("dl-clock".into(), "https://example.com/c".into(), "/tmp/c".into());
        registry.set_started_at("dl-clock", None, base);
        while rx.try_recv().is_ok() {}

        // Far in the "past" relative to any real clock skew: 249ms after start -> no emit.
        registry.update_progress_at("dl-clock", 10, base + Duration::from_millis(249));
        assert!(rx.try_recv().is_err(), "below the time threshold must not emit");

        // Exactly at the threshold -> emit, regardless of how little real time has passed.
        registry.update_progress_at("dl-clock", 20, base + Duration::from_millis(250));
        let ev = rx.try_recv().expect("time threshold must emit");
        assert_eq!(ev.received_bytes, 20);

        // Next window restarts at the emission instant.
        registry.update_progress_at("dl-clock", 30, base + Duration::from_millis(499));
        assert!(rx.try_recv().is_err());
        registry.update_progress_at("dl-clock", 40, base + Duration::from_millis(500));
        assert_eq!(rx.try_recv().expect("second window").received_bytes, 40);
    }

    #[derive(Default)]
    struct RecordingControl {
        calls: parking_lot::Mutex<Vec<&'static str>>,
    }

    impl DownloadControl for RecordingControl {
        fn cancel(&self) {
            self.calls.lock().push("cancel");
        }
        fn pause(&self) {
            self.calls.lock().push("pause");
        }
        fn resume(&self) {
            self.calls.lock().push("resume");
        }
    }

    /// An engine download: pause/resume reach the engine, the record follows what the engine
    /// reports, and cancel goes to the engine and is terminal.
    #[test]
    fn native_download_pause_resume_and_cancel_reach_the_engine() {
        let registry = DownloadRegistry::new();
        let control = Arc::new(RecordingControl::default());
        registry.register_native(
            "cef:1:7".into(),
            "https://a.test/big.iso".into(),
            "/tmp/big.iso".into(),
            Some(1000),
            Some(control.clone()),
            true,
        );
        let record = registry.get("cef:1:7").unwrap();
        assert_eq!(record.status, DownloadStatus::InProgress);
        assert!(record.can_cancel && record.can_pause);

        assert!(registry.request_pause("cef:1:7", true));
        assert_eq!(registry.get("cef:1:7").unwrap().status, DownloadStatus::InProgress);
        registry.set_paused("cef:1:7", true);
        assert_eq!(registry.get("cef:1:7").unwrap().status, DownloadStatus::Paused);
        registry.update_native("cef:1:7", 500, Some(1000), Some("/tmp/big (1).iso"));
        let record = registry.get("cef:1:7").unwrap();
        assert_eq!((record.received_bytes, record.file_path.as_str()), (500, "/tmp/big (1).iso"));

        assert!(registry.request_pause("cef:1:7", false));
        registry.set_paused("cef:1:7", false);
        assert_eq!(registry.get("cef:1:7").unwrap().status, DownloadStatus::InProgress);

        assert!(registry.cancel("cef:1:7"));
        assert_eq!(registry.get("cef:1:7").unwrap().status, DownloadStatus::Cancelled);
        assert!(!registry.request_pause("cef:1:7", true), "a finished download cannot pause");
        registry.set_paused("cef:1:7", true);
        assert_eq!(registry.get("cef:1:7").unwrap().status, DownloadStatus::Cancelled);
        assert_eq!(*control.calls.lock(), vec!["pause", "resume", "cancel"]);
    }

    /// A system-webview download cannot be steered: no cancel, no pause, and cancel is refused.
    #[test]
    fn native_download_without_a_control_only_reports() {
        let registry = DownloadRegistry::new();
        registry.register_native(
            "wv:1".into(),
            "https://a.test/f".into(),
            "/tmp/f".into(),
            None,
            None,
            true,
        );
        let record = registry.get("wv:1").unwrap();
        assert!(!record.can_cancel && !record.can_pause);
        assert!(!registry.request_pause("wv:1", true));
        assert!(!registry.cancel("wv:1"));
        registry.set_committed("wv:1");
        assert_eq!(registry.get("wv:1").unwrap().status, DownloadStatus::Completed);
    }

    /// Registers a system-webview download the way the webview hook does: first without a
    /// control, then with the one the engine hands over.
    fn webview_download_with_control(registry: &DownloadRegistry, id: &str, can_pause: bool) -> Arc<RecordingControl> {
        registry.register_native(id.into(), "https://a.test/f".into(), "/tmp/f".into(), None, None, false);
        let control = Arc::new(RecordingControl::default());
        assert!(registry.attach_control(id, control.clone(), can_pause));
        control
    }

    #[test]
    fn attach_control_turns_on_the_controls_and_routes_them_to_the_engine() {
        let registry = DownloadRegistry::new();
        let mut updates = registry.subscribe();
        let control = webview_download_with_control(&registry, "wv:attach", true);
        let record = registry.get("wv:attach").unwrap();
        assert!(record.can_cancel && record.can_pause);
        let mut last = None;
        while let Ok(update) = updates.try_recv() {
            last = Some(update);
        }
        assert!(last.is_some_and(|update| update.can_cancel && update.can_pause), "the shelf learns about the controls");

        assert!(registry.request_pause("wv:attach", true));
        assert!(registry.request_pause("wv:attach", false));
        assert!(registry.cancel("wv:attach"));
        assert_eq!(*control.calls.lock(), vec!["pause", "resume", "cancel"]);
        assert!(!registry.attach_control("wv:attach", control, true), "a finished download takes no control");
    }

    #[test]
    fn a_pause_the_engine_reports_as_a_failure_stays_paused() {
        let registry = DownloadRegistry::new();
        webview_download_with_control(&registry, "wv:pause", true);
        assert!(registry.request_pause("wv:pause", true));
        registry.set_paused("wv:pause", true);
        registry.set_engine_failed("wv:pause", "download failed".into());
        let record = registry.get("wv:pause").unwrap();
        assert_eq!(record.status, DownloadStatus::Paused);
        assert!(record.error.is_none());
    }

    #[test]
    fn resuming_returns_a_paused_download_to_in_progress() {
        let registry = DownloadRegistry::new();
        let control = webview_download_with_control(&registry, "wv:resume", true);
        registry.set_paused("wv:resume", true);
        assert!(registry.request_pause("wv:resume", false));
        registry.set_paused("wv:resume", false);
        assert_eq!(registry.get("wv:resume").unwrap().status, DownloadStatus::InProgress);
        assert_eq!(*control.calls.lock(), vec!["resume"]);
        registry.set_engine_failed("wv:resume", "network".into());
        assert_eq!(registry.get("wv:resume").unwrap().status, DownloadStatus::Failed, "a running download can fail");
    }

    #[test]
    fn a_cancel_stays_cancelled_when_the_engine_then_reports_a_failure() {
        let registry = DownloadRegistry::new();
        webview_download_with_control(&registry, "wv:cancel", true);
        assert!(registry.cancel("wv:cancel"));
        registry.set_engine_failed("wv:cancel", "download failed".into());
        assert_eq!(registry.get("wv:cancel").unwrap().status, DownloadStatus::Cancelled);
    }

    #[test]
    fn a_cancel_only_control_refuses_pause() {
        let registry = DownloadRegistry::new();
        let control = webview_download_with_control(&registry, "wv:cancel-only", false);
        let record = registry.get("wv:cancel-only").unwrap();
        assert!(record.can_cancel && !record.can_pause);
        assert!(!registry.request_pause("wv:cancel-only", true));
        assert!(control.calls.lock().is_empty());
        assert!(registry.cancel("wv:cancel-only"));
        assert_eq!(*control.calls.lock(), vec!["cancel"]);
    }

    #[test]
    fn a_full_disk_is_reported_as_disk_full() {
        let registry = DownloadRegistry::new();
        registry.register("dl-full".into(), "https://a.test/f".into(), "/tmp/f".into());
        let error = std::io::Error::from(std::io::ErrorKind::StorageFull);
        registry.set_failed_kind("dl-full", DownloadFailureKind::of_io(&error), error.to_string());
        let record = registry.get("dl-full").unwrap();
        assert_eq!(record.status, DownloadStatus::Failed);
        assert_eq!(record.failure, Some(DownloadFailureKind::DiskFull));
        assert_eq!(
            DownloadFailureKind::of_io(&std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
            DownloadFailureKind::AccessDenied
        );
        let json = serde_json::to_string(&record).unwrap();
        assert!(json.contains("\"failure\":\"diskFull\""));
    }
}
