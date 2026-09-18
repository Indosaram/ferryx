use crate::browser::manager::BrowserManager;
use crate::browser::model::*;
use crate::browser::security::BrowserError;
use crate::browser::snapshot_source::{
    BrowserSnapshotSource, SnapshotFormat, SnapshotOptions, UnsupportedSnapshotSource,
};
use super::remote_bridge_protocol::MAX_FRAME_PAYLOAD_BYTES;
use super::remote_driver::*;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;

// Binding budgets from §8.1 override:
pub const MAX_VIEWERS_PER_BROWSER: usize = 2;
pub const MAX_CONCURRENT_CAPTURED_BROWSERS: usize = 1;
pub const MAX_GLOBAL_DRIVERS: usize = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteServiceError {
    QuotaExceeded(&'static str),
    StaleLease,
    StaleInstance,
    StaleGeneration,
    StaleSnapshot,
    SnapshotMapMismatch,
    BrowserNotFound(String),
    Unsupported(&'static str),
    Internal(String),
    DesktopReclaimed,
}

impl std::fmt::Display for RemoteServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::QuotaExceeded(msg) => write!(f, "quota exceeded: {}", msg),
            Self::StaleLease => write!(f, "stale driver lease"),
            Self::StaleInstance => write!(f, "stale browser instance"),
            Self::StaleGeneration => write!(f, "stale document generation"),
            Self::StaleSnapshot => write!(f, "stale snapshot reference"),
            Self::SnapshotMapMismatch => write!(f, "snapshot target map revision mismatch"),
            Self::BrowserNotFound(id) => write!(f, "browser session not found: {}", id),
            Self::Unsupported(msg) => write!(f, "unsupported: {}", msg),
            Self::Internal(msg) => write!(f, "internal error: {}", msg),
            Self::DesktopReclaimed => write!(f, "control reclaimed by desktop owner"),
        }
    }
}

impl std::error::Error for RemoteServiceError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegotiatedCaptureProfile {
    pub format: SnapshotFormat,
    pub quality: u8,
    pub interval_ms: u64,
    pub max_edge: u32,
}

impl Default for NegotiatedCaptureProfile {
    fn default() -> Self {
        Self {
            format: SnapshotFormat::Jpeg { quality: 70 },
            quality: 70,
            interval_ms: 80,
            max_edge: 2048,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ViewerInfo {
    pub subscription_id: String,
    pub device_id: String,
    pub viewer_instance_id: String,
    pub stream_id: u32,
    pub unacked_seq: Option<u32>,
    pub pending_frame: Option<Vec<u8>>,
    pub joined_at: Instant,
    pub paused: bool,
    pub stalled: bool,
}

pub struct BrowserRemoteService {
    manager: BrowserManager,
    driver_broker: Arc<RemoteDriverBroker>,
    subscribers_per_browser: Arc<parking_lot::Mutex<HashMap<String, HashMap<String, ViewerInfo>>>>,
    producer_active: Arc<parking_lot::Mutex<HashMap<String, bool>>>,
    producer_handles: Arc<parking_lot::Mutex<HashMap<String, tokio::task::AbortHandle>>>,
    active_stream_ids: Arc<parking_lot::Mutex<HashMap<String, u32>>>,
    negotiated_profiles: Arc<parking_lot::Mutex<HashMap<String, NegotiatedCaptureProfile>>>,
    captures_in_progress: Arc<AtomicUsize>,
    stream_counter: Arc<AtomicU64>,
    desktop_epoch: Arc<AtomicU64>,
    service_epoch: u64,
    frame_broadcaster: Arc<parking_lot::Mutex<HashMap<String, tokio::sync::broadcast::Sender<Vec<u8>>>>>,
    snapshot_source: Arc<parking_lot::RwLock<Arc<dyn BrowserSnapshotSource>>>,
    native_capture_semaphore: Arc<tokio::sync::Semaphore>,
    quarantined_permits: Arc<parking_lot::Mutex<HashMap<String, crate::browser::snapshot_source::SnapshotCallbackCoordinator>>>,
}

impl BrowserRemoteService {
    pub fn new(manager: BrowserManager, driver_broker: Arc<RemoteDriverBroker>) -> Self {
        Self {
            manager,
            driver_broker,
            subscribers_per_browser: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            producer_active: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            producer_handles: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            active_stream_ids: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            negotiated_profiles: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            captures_in_progress: Arc::new(AtomicUsize::new(0)),
            stream_counter: Arc::new(AtomicU64::new(1)),
            desktop_epoch: Arc::new(AtomicU64::new(1)),
            service_epoch: 1,
            frame_broadcaster: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            snapshot_source: Arc::new(parking_lot::RwLock::new(Arc::new(UnsupportedSnapshotSource))),
            native_capture_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
            quarantined_permits: Arc::new(parking_lot::Mutex::new(HashMap::new())),
        }
    }

    pub fn with_snapshot_source(
        manager: BrowserManager,
        driver_broker: Arc<RemoteDriverBroker>,
        snapshot_source: Arc<dyn BrowserSnapshotSource>,
    ) -> Self {
        Self {
            manager,
            driver_broker,
            subscribers_per_browser: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            producer_active: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            producer_handles: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            active_stream_ids: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            negotiated_profiles: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            captures_in_progress: Arc::new(AtomicUsize::new(0)),
            stream_counter: Arc::new(AtomicU64::new(1)),
            desktop_epoch: Arc::new(AtomicU64::new(1)),
            service_epoch: 1,
            frame_broadcaster: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            snapshot_source: Arc::new(parking_lot::RwLock::new(snapshot_source)),
            native_capture_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
            quarantined_permits: Arc::new(parking_lot::Mutex::new(HashMap::new())),
        }
    }

    pub fn set_snapshot_source(&self, snapshot_source: Arc<dyn BrowserSnapshotSource>) {
        *self.snapshot_source.write() = snapshot_source;
    }

    pub fn frame_broadcaster(
        &self,
    ) -> &Arc<parking_lot::Mutex<HashMap<String, tokio::sync::broadcast::Sender<Vec<u8>>>>> {
        &self.frame_broadcaster
    }

    pub fn active_stream_id(&self, browser_id: &str) -> Option<u32> {
        let active_streams = self.active_stream_ids.lock();
        active_streams.get(browser_id).copied()
    }

    pub fn native_capture_semaphore(&self) -> &Arc<tokio::sync::Semaphore> {
        &self.native_capture_semaphore
    }

    pub fn is_native_capture_active(&self) -> bool {
        self.native_capture_semaphore.available_permits() == 0
    }

    pub fn release_quarantine(&self, browser_id: &str) {
        let coord_opt = self.quarantined_permits.lock().remove(browser_id);
        if let Some(coord) = coord_opt {
            let _ = coord.complete(Err(crate::browser::security::BrowserError::Internal(
                "webview closed or quarantine reclaimed".into(),
            )
            .into()));
        }
    }

    pub fn subscribe_frames(&self, browser_id: &str) -> tokio::sync::broadcast::Receiver<Vec<u8>> {
        let mut broadcasters = self.frame_broadcaster.lock();
        let tx = broadcasters
            .entry(browser_id.to_string())
            .or_insert_with(|| tokio::sync::broadcast::channel(32).0);
        tx.subscribe()
    }

    pub fn driver_broker(&self) -> &Arc<RemoteDriverBroker> {
        &self.driver_broker
    }

    pub fn service_epoch(&self) -> u64 {
        self.service_epoch
    }

    pub fn desktop_epoch(&self) -> u64 {
        self.desktop_epoch.load(Ordering::SeqCst)
    }

    pub fn increment_desktop_epoch(&self) -> u64 {
        self.desktop_epoch.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn is_snapshot_supported(&self) -> bool {
        self.snapshot_source.read().is_supported()
    }

    pub fn snapshot_source(&self) -> Arc<dyn BrowserSnapshotSource> {
        self.snapshot_source.read().clone()
    }

    pub fn set_viewer_paused(&self, browser_id: &str, subscription_id: &str, paused: bool) {
        let mut subs = self.subscribers_per_browser.lock();
        if let Some(browser_subs) = subs.get_mut(browser_id) {
            if let Some(viewer) = browser_subs.get_mut(subscription_id) {
                viewer.paused = paused;
            }
        }
    }

    pub fn set_viewer_stalled(&self, browser_id: &str, subscription_id: &str, stalled: bool) {
        let mut subs = self.subscribers_per_browser.lock();
        if let Some(browser_subs) = subs.get_mut(browser_id) {
            if let Some(viewer) = browser_subs.get_mut(subscription_id) {
                viewer.stalled = stalled;
            }
        }
    }

    /// Subscribes a viewer to a browser's shared screencast producer.
    /// Enforces budget: max 2 viewers per browser, max 1 concurrently-captured browser.
    pub fn subscribe(
        &self,
        browser_id: &str,
        device_id: &str,
        viewer_instance_id: &str,
    ) -> Result<String, RemoteServiceError> {
        self.subscribe_with_profile(browser_id, device_id, viewer_instance_id, None)
            .map(|(sub_id, _)| sub_id)
    }

    pub fn subscribe_with_profile(
        &self,
        browser_id: &str,
        device_id: &str,
        viewer_instance_id: &str,
        requested_profile: Option<NegotiatedCaptureProfile>,
    ) -> Result<(String, NegotiatedCaptureProfile), RemoteServiceError> {
        // Ensure browser exists and is visible
        if !self.manager.is_visible(browser_id).map_err(|e| RemoteServiceError::BrowserNotFound(e.to_string()))? {
            return Err(RemoteServiceError::Unsupported("browser is not visible; screencast requires visible session"));
        }

        let mut subs_map = self.subscribers_per_browser.lock();
        let mut active_map = self.producer_active.lock();

        let browser_subs = subs_map.entry(browser_id.to_string()).or_default();
        let was_empty = browser_subs.is_empty();

        // Check viewer limit per browser (max 2)
        if browser_subs.len() >= MAX_VIEWERS_PER_BROWSER {
            return Err(RemoteServiceError::QuotaExceeded(
                "max 2 viewers per browser reached",
            ));
        }

        // Check concurrent captured browser limit (max 1)
        let is_currently_captured = active_map.get(browser_id).copied().unwrap_or(false);
        if !is_currently_captured {
            let active_browser_count = active_map.values().filter(|&&active| active).count();
            if active_browser_count >= MAX_CONCURRENT_CAPTURED_BROWSERS {
                return Err(RemoteServiceError::QuotaExceeded(
                    "max 1 concurrently captured browser reached",
                ));
            }
            // Activate producer for this browser
            active_map.insert(browser_id.to_string(), true);
            self.captures_in_progress.fetch_add(1, Ordering::SeqCst);
        }

        let subscription_id = format!("sub_{}", Uuid::new_v4());
        let stream_id = {
            let mut active_streams = self.active_stream_ids.lock();
            *active_streams
                .entry(browser_id.to_string())
                .or_insert_with(|| self.stream_counter.fetch_add(1, Ordering::SeqCst) as u32)
        };

        let negotiated = if was_empty {
            let prof = requested_profile.unwrap_or_default();
            self.negotiated_profiles
                .lock()
                .insert(browser_id.to_string(), prof.clone());
            prof
        } else {
            self.negotiated_profiles
                .lock()
                .get(browser_id)
                .cloned()
                .unwrap_or_default()
        };

        browser_subs.insert(
            subscription_id.clone(),
            ViewerInfo {
                subscription_id: subscription_id.clone(),
                device_id: device_id.to_string(),
                viewer_instance_id: viewer_instance_id.to_string(),
                stream_id,
                unacked_seq: None,
                pending_frame: None,
                joined_at: Instant::now(),
                paused: false,
                stalled: false,
            },
        );

        drop(active_map);
        drop(subs_map);

        if was_empty {
            self.spawn_capture_producer(browser_id.to_string(), stream_id, negotiated.clone());
        }

        Ok((subscription_id, negotiated))
    }

    /// Unsubscribes a viewer.
    /// If subscriber count reaches 0, producer is stopped immediately (0 subscribers -> 0 captures).
    pub fn unsubscribe(&self, browser_id: &str, subscription_id: &str) -> bool {
        let mut subs_map = self.subscribers_per_browser.lock();
        let mut active_map = self.producer_active.lock();

        let mut removed = false;
        if let Some(browser_subs) = subs_map.get_mut(browser_id) {
            if browser_subs.remove(subscription_id).is_some() {
                removed = true;
                // Also release any driver lease owned by this subscription
                self.driver_broker.release_by_subscription(subscription_id);
            }

            // If 0 subscribers remain, stop producer immediately
            if browser_subs.is_empty() {
                if let Some(active) = active_map.get_mut(browser_id) {
                    if *active {
                        *active = false;
                        self.captures_in_progress.fetch_sub(1, Ordering::SeqCst);
                    }
                }
                self.active_stream_ids.lock().remove(browser_id);
                self.negotiated_profiles.lock().remove(browser_id);
                if let Some(handle) = self.producer_handles.lock().remove(browser_id) {
                    handle.abort();
                }
            }
        }

        removed
    }

    pub fn is_producer_active(&self, browser_id: &str) -> bool {
        let active_map = self.producer_active.lock();
        active_map.get(browser_id).copied().unwrap_or(false)
    }

    pub fn active_capture_count(&self) -> usize {
        self.captures_in_progress.load(Ordering::SeqCst)
    }

    pub fn subscriber_count(&self, browser_id: &str) -> usize {
        let subs_map = self.subscribers_per_browser.lock();
        subs_map.get(browser_id).map(|s| s.len()).unwrap_or(0)
    }

    fn spawn_capture_producer(&self, browser_id: String, stream_id: u32, profile: NegotiatedCaptureProfile) {
        if let Some(existing) = self.producer_handles.lock().remove(&browser_id) {
            existing.abort();
        }

        let handle = match tokio::runtime::Handle::try_current() {
            Ok(h) => h,
            Err(_) => return, // No Tokio runtime active (e.g. synchronous unit test), skip spawning capture loop
        };

        let tx = {
            let mut broadcasters = self.frame_broadcaster.lock();
            broadcasters
                .entry(browser_id.clone())
                .or_insert_with(|| tokio::sync::broadcast::channel(32).0)
                .clone()
        };

        let subscribers_map = Arc::clone(&self.subscribers_per_browser);
        let producer_active = Arc::clone(&self.producer_active);
        let producer_handles = Arc::clone(&self.producer_handles);
        let active_stream_ids = Arc::clone(&self.active_stream_ids);
        let captures_in_progress = Arc::clone(&self.captures_in_progress);
        let snapshot_source_holder = Arc::clone(&self.snapshot_source);
        let native_capture_semaphore = Arc::clone(&self.native_capture_semaphore);
        let quarantined_permits = Arc::clone(&self.quarantined_permits);
        let manager = self.manager.clone();
        let desktop_epoch = Arc::clone(&self.desktop_epoch);
        let service_epoch = self.service_epoch;
        let browser_id_clone = browser_id.clone();

        let join_handle = handle.spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(
                profile.interval_ms.clamp(50, 5000),
            ));
            let mut seq: u32 = 0;

            loop {
                interval.tick().await;

                // While active subscribers > 0
                let (has_active_subscribers, has_eligible_subscribers) = {
                    let subs = subscribers_map.lock();
                    if let Some(s) = subs.get(&browser_id_clone) {
                        let active = !s.is_empty();
                        let eligible = s.values().any(|v| !v.paused && !v.stalled);
                        (active, eligible)
                    } else {
                        (false, false)
                    }
                };

                if !has_active_subscribers {
                    let mut active_map = producer_active.lock();
                    if let Some(active) = active_map.get_mut(&browser_id_clone) {
                        if *active {
                            *active = false;
                            captures_in_progress.fetch_sub(1, Ordering::SeqCst);
                        }
                    }
                    active_stream_ids.lock().remove(&browser_id_clone);
                    producer_handles.lock().remove(&browser_id_clone);
                    break;
                }

                if !has_eligible_subscribers {
                    // Capture only for eligible consumers/credit: skip capture tick when all viewers are paused or stalled
                    continue;
                }

                let state = match manager.get_state(&browser_id_clone) {
                    Ok(s) => s,
                    Err(_) => break,
                };

                if !state.visible {
                    continue;
                }

                let (bounds, zoom, viewport_rev) = manager
                    .get_geometry(&browser_id_clone)
                    .unwrap_or((None, 1.0, 1));
                let capture_rect = bounds.unwrap_or(LogicalRect {
                    x: 0.0,
                    y: 0.0,
                    width: 1024.0,
                    height: 768.0,
                });

                let (raw_w, raw_h) = if capture_rect.width > 0.0 && capture_rect.height > 0.0 {
                    (
                        (capture_rect.width * zoom.max(1.0)).round() as u32,
                        (capture_rect.height * zoom.max(1.0)).round() as u32,
                    )
                } else {
                    (1024, 768)
                };
                let (clamped_w, clamped_h) =
                    crate::browser::snapshot_source::clamp_capture_dimensions(
                        raw_w.min(profile.max_edge),
                        raw_h.min(profile.max_edge),
                    );
                let snapshot_options = match profile.format {
                    SnapshotFormat::Png => SnapshotOptions::png().with_bounds(clamped_w, clamped_h),
                    SnapshotFormat::Jpeg { quality } => {
                        SnapshotOptions::jpeg(quality).with_bounds(clamped_w, clamped_h)
                    }
                };

                // Sample immutable ticket before acquisition (R5-9):
                // (instance_id, service_epoch, desktop_epoch, generation, viewport_revision)
                let ticket_instance_id = match manager.get_instance_id(&browser_id_clone) {
                    Ok(id) => id,
                    Err(_) => break,
                };
                let ticket_service_epoch = service_epoch;
                let ticket_desktop_epoch = desktop_epoch.load(Ordering::SeqCst);
                let ticket_generation = state.generation;
                let ticket_viewport_rev = viewport_rev;

                // Acquire shared native-capture permit
                let permit = match native_capture_semaphore.clone().try_acquire_owned() {
                    Ok(p) => p,
                    Err(_) => {
                        // Shared native-capture permit is already held by an in-flight capture or quarantined timeout.
                        // Skip tick so overlapping captures cannot run across producer restarts or iterations.
                        continue;
                    }
                };

                let (coordinator, _rx) =
                    crate::browser::snapshot_source::SnapshotCallbackCoordinator::new();
                let permit_slot = Arc::new(parking_lot::Mutex::new(Some(permit)));
                let ps = permit_slot.clone();
                let qp = Arc::clone(&quarantined_permits);
                let b_id = browser_id_clone.clone();
                coordinator.set_permit_releaser(move || {
                    let mut guard = ps.lock();
                    if let Some(p) = guard.take() {
                        drop(p);
                    }
                    qp.lock().remove(&b_id);
                });

                let source = snapshot_source_holder.read().clone();
                let snapshot_res = source
                    .take_snapshot_coordinated(&state.webview_label, snapshot_options, coordinator.clone())
                    .await;

                let is_timeout = match &snapshot_res {
                    Err(e) => {
                        e.message.contains("timed out")
                            || e.code == crate::ipc::IpcErrorCode::BrowserWaitTimeout
                    }
                    _ => false,
                };
                if is_timeout {
                    // Retain coordinator and permit in quarantined_permits until callback completion!
                    quarantined_permits
                        .lock()
                        .insert(browser_id_clone.clone(), coordinator);
                } else {
                    let mut guard = permit_slot.lock();
                    if let Some(p) = guard.take() {
                        drop(p);
                    }
                }

                let snapshot = match snapshot_res {
                    Ok(s) => s,
                    Err(_) => continue,
                };

                // Revalidate current state immediately before publication (R4-7 Part C, R5-9):
                // Ticket was sampled before the await; require exact match on (instance_id, service_epoch, desktop_epoch, generation, viewport_revision)
                let current_state = match manager.get_state(&browser_id_clone) {
                    Ok(s) => s,
                    Err(_) => break,
                };

                if !current_state.visible {
                    // Visibility loss halts publication
                    continue;
                }

                let current_instance_id = manager.get_instance_id(&browser_id_clone).unwrap_or_default();
                let current_desktop_epoch = desktop_epoch.load(Ordering::SeqCst);
                let (_, _, current_viewport_rev) = manager
                    .get_geometry(&browser_id_clone)
                    .unwrap_or((None, 1.0, 1));

                if current_instance_id != ticket_instance_id
                    || current_state.generation != ticket_generation
                    || current_desktop_epoch != ticket_desktop_epoch
                    || current_viewport_rev != ticket_viewport_rev
                {
                    // Ticket mismatch! Drop frame sampled before instance/epoch/generation/viewport change;
                    // advance seq to emit a resync sequence gap
                    seq = seq.wrapping_add(1);
                    continue;
                }

                seq = seq.wrapping_add(1);

                let format_byte = match snapshot.format {
                    SnapshotFormat::Jpeg { .. } => super::remote_bridge_protocol::FRAME_FORMAT_JPEG,
                    SnapshotFormat::Png => super::remote_bridge_protocol::FRAME_FORMAT_PNG,
                };

                let metadata = crate::remote::browser_protocol::BrowserFrameMetadata {
                    offset_top: 0.0,
                    page_scale_factor: zoom,
                    device_width: capture_rect.width,
                    device_height: capture_rect.height,
                    image_width: snapshot.width,
                    image_height: snapshot.height,
                    scroll_offset_x: 0.0,
                    scroll_offset_y: 0.0,
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs_f64())
                        .unwrap_or(0.0),
                    stream_id,
                    browser_instance_id: ticket_instance_id,
                    browser_service_epoch: ticket_service_epoch.to_string(),
                    desktop_epoch: ticket_desktop_epoch.to_string(),
                    document_generation: ticket_generation.to_string(),
                    viewport_revision: ticket_viewport_rev.to_string(),
                    capture_rect: crate::remote::browser_protocol::BrowserCaptureRect {
                        x: capture_rect.x,
                        y: capture_rect.y,
                        width: capture_rect.width,
                        height: capture_rect.height,
                    },
                    geometry_source: "wkSnapshot".to_string(),
                };

                if let Ok(frame_bytes) = super::remote_bridge_protocol::encode_frame(
                    format_byte,
                    seq,
                    &metadata,
                    &snapshot.bytes,
                ) {
                    let _ = tx.send(frame_bytes);
                }
            }
        });

        self.producer_handles
            .lock()
            .insert(browser_id, join_handle.abort_handle());
    }

    /// Latest-only frame admission logic.
    /// Returns which subscribers should receive this frame immediately.
    pub fn admit_frame(
        &self,
        browser_id: &str,
        seq: u32,
        frame_bytes: Vec<u8>,
    ) -> Result<Vec<String>, RemoteServiceError> {
        // Permanent drop if oversized
        if frame_bytes.len() > MAX_FRAME_PAYLOAD_BYTES {
            return Ok(Vec::new()); // dropped permanently, no infinite retries
        }

        let mut subs_map = self.subscribers_per_browser.lock();
        let browser_subs = match subs_map.get_mut(browser_id) {
            Some(s) => s,
            None => return Ok(Vec::new()),
        };

        let mut immediate_subscribers = Vec::new();
        for (sub_id, viewer) in browser_subs.iter_mut() {
            if viewer.unacked_seq.is_none() {
                // Viewer is ready for immediate frame
                viewer.unacked_seq = Some(seq);
                immediate_subscribers.push(sub_id.clone());
            } else {
                // Viewer is busy with an in-flight unacknowledged frame.
                // Replace pending_frame with the latest frame only (drop older pending frames).
                viewer.pending_frame = Some(frame_bytes.clone());
            }
        }

        Ok(immediate_subscribers)
    }

    /// Handles a frame ACK from a viewer.
    /// If a newer pending frame exists, returns it for immediate dispatch.
    pub fn acknowledge_frame(
        &self,
        browser_id: &str,
        subscription_id: &str,
        seq: u32,
    ) -> Option<Vec<u8>> {
        let mut subs_map = self.subscribers_per_browser.lock();
        let browser_subs = subs_map.get_mut(browser_id)?;
        let viewer = browser_subs.get_mut(subscription_id)?;

        if viewer.unacked_seq == Some(seq) {
            viewer.unacked_seq = None;
            // Pop the pending latest frame if one was queued
            if let Some(pending) = viewer.pending_frame.take() {
                // Set pending seq placeholder (represented as 0 or next seq in caller)
                return Some(pending);
            }
        }

        None
    }

    /// Records a new DOM automation snapshot, issuing a fresh snapshot_id and incrementing map_revision.
    pub fn record_snapshot(
        &self,
        browser_id: &str,
        generation: u64,
        targets: Vec<BrowserAutomationTarget>,
    ) -> Result<(String, u64), RemoteServiceError> {
        self.manager
            .record_remote_snapshot(browser_id, generation, targets)
            .map_err(|e| match e {
                BrowserError::AutomationSnapshotStale => RemoteServiceError::StaleGeneration,
                BrowserError::NotFound(id) => RemoteServiceError::BrowserNotFound(id),
                other => RemoteServiceError::Internal(other.to_string()),
            })
    }

    /// Verifies reference-based click/fill target using separate snapshot_id and immutable map_revision.
    pub fn verify_snapshot_ref(
        &self,
        browser_id: &str,
        snapshot_id: &str,
        map_revision: u64,
        reference: &str,
    ) -> Result<String, RemoteServiceError> {
        self.manager
            .verify_remote_target(browser_id, snapshot_id, map_revision, reference)
            .map_err(|e| match e {
                BrowserError::AutomationSnapshotStale => RemoteServiceError::SnapshotMapMismatch,
                BrowserError::AutomationTargetNotFound(_) => RemoteServiceError::StaleSnapshot,
                BrowserError::NotFound(id) => RemoteServiceError::BrowserNotFound(id),
                other => RemoteServiceError::Internal(other.to_string()),
            })
    }

    /// Validates all guards before executing a mutation command:
    /// lease validity, browser instance ID, desktop epoch, and document generation.
    pub fn execute_command_guard(
        &self,
        browser_id: &str,
        lease_epoch: u64,
        device_id: &str,
        connection_id: &str,
        instance_id: &str,
        desktop_epoch: u64,
        generation: u64,
    ) -> Result<(), RemoteServiceError> {
        // 1. Lease guard
        self.driver_broker
            .validate_lease(browser_id, lease_epoch, device_id, connection_id)
            .map_err(|e| match e {
                RemoteDriverError::DesktopReclaimed => RemoteServiceError::DesktopReclaimed,
                _ => RemoteServiceError::StaleLease,
            })?;

        // 2. Desktop epoch guard
        if desktop_epoch != self.desktop_epoch() {
            return Err(RemoteServiceError::StaleLease);
        }

        // 3. Instance ID guard
        let actual_instance = self
            .manager
            .get_instance_id(browser_id)
            .map_err(|e| RemoteServiceError::BrowserNotFound(e.to_string()))?;
        if actual_instance != instance_id {
            return Err(RemoteServiceError::StaleInstance);
        }

        // 4. Document generation guard
        let state = self
            .manager
            .get_state(browser_id)
            .map_err(|e| RemoteServiceError::BrowserNotFound(e.to_string()))?;
        if state.generation != generation {
            return Err(RemoteServiceError::StaleGeneration);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_service() -> (BrowserRemoteService, String) {
        let manager = BrowserManager::new();
        let state = manager
            .register_session(CreateBrowserRequest {
                browser_id: Some("test-b1".into()),
                workspace_id: Some("ws-1".into()),
                worktree_path: None,
                url: "https://example.com".into(),
                profile: None,
                zoom_factor: None,
                bounds: Some(LogicalRect {
                    x: 0.0,
                    y: 0.0,
                    width: 800.0,
                    height: 600.0,
                }),
                visible: Some(true),
            })
            .unwrap();

        let broker = Arc::new(RemoteDriverBroker::new());
        let service = BrowserRemoteService::new(manager, broker);
        (service, state.browser_id)
    }

    #[test]
    fn test_producer_pauses_on_zero_subscribers() {
        let (service, b1) = create_test_service();

        assert!(!service.is_producer_active(&b1));
        assert_eq!(service.active_capture_count(), 0);

        // Subscribe viewer 1 -> producer starts
        let sub1 = service.subscribe(&b1, "dev1", "v1").unwrap();
        assert!(service.is_producer_active(&b1));
        assert_eq!(service.active_capture_count(), 1);

        // Subscribe viewer 2 -> shared producer remains active
        let sub2 = service.subscribe(&b1, "dev2", "v2").unwrap();
        assert!(service.is_producer_active(&b1));
        assert_eq!(service.active_capture_count(), 1);

        // Viewer 1 leaves -> producer still active because viewer 2 remains
        assert!(service.unsubscribe(&b1, &sub1));
        assert!(service.is_producer_active(&b1));
        assert_eq!(service.active_capture_count(), 1);

        // Viewer 2 leaves -> 0 subscribers: producer PAUSES/STOPS immediately!
        assert!(service.unsubscribe(&b1, &sub2));
        assert!(!service.is_producer_active(&b1));
        assert_eq!(service.active_capture_count(), 0);
    }

    #[test]
    fn test_budget_concurrency_limits() {
        let (service, b1) = create_test_service();

        // 1. Max 2 viewers per browser (§8.1 binding override)
        let sub1 = service.subscribe(&b1, "dev1", "v1").unwrap();
        let sub2 = service.subscribe(&b1, "dev2", "v2").unwrap();
        let sub3_err = service.subscribe(&b1, "dev3", "v3");
        assert!(matches!(sub3_err, Err(RemoteServiceError::QuotaExceeded(_))));

        // 2. Max 1 concurrently captured browser (§8.1 binding override)
        // Create second browser session
        service
            .manager
            .register_session(CreateBrowserRequest {
                browser_id: Some("test-b2".into()),
                workspace_id: Some("ws-1".into()),
                worktree_path: None,
                url: "https://example.com/2".into(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .unwrap();

        // Attempting to subscribe to second browser while first browser is active is rejected
        let b2_sub_err = service.subscribe("test-b2", "dev4", "v4");
        assert!(matches!(b2_sub_err, Err(RemoteServiceError::QuotaExceeded(_))));

        // Once first browser is unsubscribed, second browser can be captured
        service.unsubscribe(&b1, &sub1);
        service.unsubscribe(&b1, &sub2);

        let b2_sub_ok = service.subscribe("test-b2", "dev4", "v4");
        assert!(b2_sub_ok.is_ok());
    }

    #[test]
    fn test_stale_rejections_and_identity() {
        let (service, b1) = create_test_service();
        let sub = service.subscribe(&b1, "dev1", "v1").unwrap();

        // Claim driver lease
        let lease = service
            .driver_broker()
            .claim("dev1", "conn1", &sub, &b1, true)
            .unwrap();

        let instance_id = service.manager.get_instance_id(&b1).unwrap();
        let desktop_epoch = service.desktop_epoch();
        let generation = service.manager.get_state(&b1).unwrap().generation;

        // 1. Valid command guard passes
        assert!(service
            .execute_command_guard(
                &b1,
                lease.lease_epoch,
                "dev1",
                "conn1",
                &instance_id,
                desktop_epoch,
                generation
            )
            .is_ok());

        // 2. Stale lease epoch rejected
        assert!(matches!(
            service.execute_command_guard(
                &b1,
                lease.lease_epoch + 1,
                "dev1",
                "conn1",
                &instance_id,
                desktop_epoch,
                generation
            ),
            Err(RemoteServiceError::StaleLease)
        ));

        // 3. Stale instance ID rejected
        assert!(matches!(
            service.execute_command_guard(
                &b1,
                lease.lease_epoch,
                "dev1",
                "conn1",
                "wrong-instance-id",
                desktop_epoch,
                generation
            ),
            Err(RemoteServiceError::StaleInstance)
        ));

        // 4. Stale desktop epoch rejected
        assert!(matches!(
            service.execute_command_guard(
                &b1,
                lease.lease_epoch,
                "dev1",
                "conn1",
                &instance_id,
                desktop_epoch + 1,
                generation
            ),
            Err(RemoteServiceError::StaleLease)
        ));

        // 5. Stale document generation rejected
        assert!(matches!(
            service.execute_command_guard(
                &b1,
                lease.lease_epoch,
                "dev1",
                "conn1",
                &instance_id,
                desktop_epoch,
                generation + 1
            ),
            Err(RemoteServiceError::StaleGeneration)
        ));
    }

    #[test]
    fn test_snapshot_map_revision_check() {
        let (service, b1) = create_test_service();
        let state = service.manager.get_state(&b1).unwrap();

        let targets1 = vec![BrowserAutomationTarget {
            reference: "btn-submit".into(),
            selector: "#submit".into(),
        }];

        let (snap1_id, rev1) = service
            .record_snapshot(&b1, state.generation, targets1)
            .unwrap();

        // Valid reference resolution
        let sel = service.verify_snapshot_ref(&b1, &snap1_id, rev1, "btn-submit").unwrap();
        assert_eq!(sel, "#submit");

        // Stale snapshot reference or wrong map revision rejected
        assert!(matches!(
            service.verify_snapshot_ref(&b1, "old-snap-id", rev1, "btn-submit"),
            Err(RemoteServiceError::SnapshotMapMismatch)
        ));

        assert!(matches!(
            service.verify_snapshot_ref(&b1, &snap1_id, rev1 + 1, "btn-submit"),
            Err(RemoteServiceError::SnapshotMapMismatch)
        ));

        // New snapshot in same document generation replaces target map and increments revision
        let targets2 = vec![BrowserAutomationTarget {
            reference: "btn-next".into(),
            selector: "#next".into(),
        }];
        let (snap2_id, rev2) = service
            .record_snapshot(&b1, state.generation, targets2)
            .unwrap();
        assert_ne!(snap1_id, snap2_id);
        assert_eq!(rev2, rev1 + 1);

        // Old snap1_id is now rejected
        assert!(matches!(
            service.verify_snapshot_ref(&b1, &snap1_id, rev1, "btn-submit"),
            Err(RemoteServiceError::SnapshotMapMismatch)
        ));

        // New snap2_id is accepted
        let sel2 = service.verify_snapshot_ref(&b1, &snap2_id, rev2, "btn-next").unwrap();
        assert_eq!(sel2, "#next");
    }

    #[test]
    fn test_latest_only_frame_admission() {
        let (service, b1) = create_test_service();
        let sub1 = service.subscribe(&b1, "dev1", "v1").unwrap();

        // 1. Frame 1 admitted immediately
        let frame1 = vec![1, 2, 3];
        let immediate1 = service.admit_frame(&b1, 1, frame1).unwrap();
        assert_eq!(immediate1, vec![sub1.clone()]);

        // 2. While frame 1 is unacknowledged, frame 2 arrives -> queued in pending_frame (latest)
        let frame2 = vec![4, 5, 6];
        let immediate2 = service.admit_frame(&b1, 2, frame2.clone()).unwrap();
        assert!(immediate2.is_empty(), "busy viewer does not get immediate delivery");

        // 3. Frame 3 arrives before ACK -> replaces frame 2 in pending_frame (frame 2 dropped!)
        let frame3 = vec![7, 8, 9];
        let immediate3 = service.admit_frame(&b1, 3, frame3.clone()).unwrap();
        assert!(immediate3.is_empty());

        // 4. Viewer ACKs frame 1 -> gets frame 3 (latest only, frame 2 was dropped)
        let next_frame = service.acknowledge_frame(&b1, &sub1, 1);
        assert_eq!(next_frame, Some(frame3));

        // 5. Oversized frame (> 2MiB) is permanently dropped
        let huge_frame = vec![0u8; MAX_FRAME_PAYLOAD_BYTES + 1];
        let admitted_huge = service.admit_frame(&b1, 4, huge_frame).unwrap();
        assert!(admitted_huge.is_empty());
    }

    #[tokio::test]
    async fn test_r5_9_producer_ticket_drops_frame_on_instance_replacement() {
        struct SingleCoordinatedSource {
            started_tx: tokio::sync::mpsc::Sender<()>,
            proceed_rx: tokio::sync::Mutex<tokio::sync::mpsc::Receiver<()>>,
        }

        impl crate::browser::snapshot_source::BrowserSnapshotSource for SingleCoordinatedSource {
            fn is_supported(&self) -> bool { true }
            fn capture_snapshot<'a>(
                &'a self,
                webview_label: &'a str,
                options: crate::browser::snapshot_source::SnapshotOptions,
            ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<crate::browser::snapshot_source::BrowserSnapshot, crate::ipc::IpcError>> + Send + 'a>> {
                let (coordinator, _) = crate::browser::snapshot_source::SnapshotCallbackCoordinator::new();
                self.take_snapshot_coordinated(webview_label, options, coordinator)
            }
            fn take_snapshot_coordinated<'a>(
                &'a self,
                _webview_label: &'a str,
                _options: crate::browser::snapshot_source::SnapshotOptions,
                _coordinator: crate::browser::snapshot_source::SnapshotCallbackCoordinator,
            ) -> crate::remote::browser_backend::BoxFuture<'a, Result<crate::browser::snapshot_source::BrowserSnapshot, crate::ipc::IpcError>> {
                Box::pin(async move {
                    let _ = self.started_tx.send(()).await;
                    let _ = self.proceed_rx.lock().await.recv().await;
                    Ok(crate::browser::snapshot_source::BrowserSnapshot::new(
                        crate::browser::snapshot_source::sample_valid_jpeg_bytes(),
                        crate::browser::snapshot_source::SnapshotFormat::Jpeg { quality: 80 },
                        800,
                        600,
                    ))
                })
            }
        }

        let (service, b1) = create_test_service();
        let (started_tx, mut started_rx) = tokio::sync::mpsc::channel(1);
        let (proceed_tx, proceed_rx) = tokio::sync::mpsc::channel(1);

        let source = Arc::new(SingleCoordinatedSource {
            started_tx,
            proceed_rx: tokio::sync::Mutex::new(proceed_rx),
        });
        service.set_snapshot_source(source);

        let mut frame_rx = service.subscribe_frames(&b1);
        let old_instance_id = service.manager.get_instance_id(&b1).unwrap();
        let sub = service.subscribe(&b1, "dev-ticket", "v-ticket").unwrap();

        // Await deterministic signal that capture tick 1 has sampled ticket and entered await
        tokio::time::timeout(std::time::Duration::from_secs(2), started_rx.recv())
            .await
            .expect("Capture must start")
            .expect("started channel open");

        // While tick 1 is inside capture await: close and recreate browser with SAME ID
        service.manager.remove_session(&b1);
        service
            .manager
            .register_session(CreateBrowserRequest {
                browser_id: Some(b1.clone()),
                workspace_id: Some("ws-1".into()),
                worktree_path: None,
                url: "https://example.com/recreated".into(),
                profile: None,
                zoom_factor: None,
                bounds: Some(LogicalRect {
                    x: 0.0,
                    y: 0.0,
                    width: 800.0,
                    height: 600.0,
                }),
                visible: Some(true),
            })
            .unwrap();

        let new_instance_id = service.manager.get_instance_id(&b1).unwrap();
        assert_ne!(old_instance_id, new_instance_id, "Recreated session has new instance_id");
        assert_eq!(
            service.manager.get_state(&b1).unwrap().generation,
            1,
            "Recreated session restarts at generation 1"
        );

        // Now allow tick 1 to complete its capture await
        proceed_tx.send(()).await.unwrap();

        // Immediate unsubscribe to ensure tick 2 does not produce a frame
        // (we are specifically testing that tick 1's old frame is dropped!)
        let recv_frame = tokio::time::timeout(std::time::Duration::from_millis(100), frame_rx.recv()).await;
        // In tick 1, ticket mismatch occurred: instance_id changed! Frame MUST have been dropped!
        if let Ok(Ok(frame_bytes)) = recv_frame {
            let (_, metadata, _) = crate::browser::remote_bridge_protocol::decode_frame(&frame_bytes).unwrap();
            panic!(
                "Old capture must not be published! Received frame with browser_instance_id={}, ticket was {}",
                metadata.browser_instance_id, old_instance_id
            );
        }

        service.unsubscribe(&b1, &sub);
    }
}
