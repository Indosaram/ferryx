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

#[derive(Clone, Debug)]
pub struct ViewerInfo {
    pub subscription_id: String,
    pub device_id: String,
    pub viewer_instance_id: String,
    pub stream_id: u64,
    pub unacked_seq: Option<u32>,
    pub pending_frame: Option<Vec<u8>>,
    pub joined_at: Instant,
}

pub struct BrowserRemoteService {
    manager: BrowserManager,
    driver_broker: Arc<RemoteDriverBroker>,
    subscribers_per_browser: Arc<parking_lot::Mutex<HashMap<String, HashMap<String, ViewerInfo>>>>,
    producer_active: Arc<parking_lot::Mutex<HashMap<String, bool>>>,
    captures_in_progress: Arc<AtomicUsize>,
    stream_counter: Arc<AtomicU64>,
    desktop_epoch: Arc<AtomicU64>,
    service_epoch: u64,
    frame_broadcaster: Arc<parking_lot::Mutex<HashMap<String, tokio::sync::broadcast::Sender<Vec<u8>>>>>,
    snapshot_source: Arc<parking_lot::RwLock<Arc<dyn BrowserSnapshotSource>>>,
}

impl BrowserRemoteService {
    pub fn new(manager: BrowserManager, driver_broker: Arc<RemoteDriverBroker>) -> Self {
        Self {
            manager,
            driver_broker,
            subscribers_per_browser: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            producer_active: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            captures_in_progress: Arc::new(AtomicUsize::new(0)),
            stream_counter: Arc::new(AtomicU64::new(1)),
            desktop_epoch: Arc::new(AtomicU64::new(1)),
            service_epoch: 1,
            frame_broadcaster: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            snapshot_source: Arc::new(parking_lot::RwLock::new(Arc::new(UnsupportedSnapshotSource))),
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
            captures_in_progress: Arc::new(AtomicUsize::new(0)),
            stream_counter: Arc::new(AtomicU64::new(1)),
            desktop_epoch: Arc::new(AtomicU64::new(1)),
            service_epoch: 1,
            frame_broadcaster: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            snapshot_source: Arc::new(parking_lot::RwLock::new(snapshot_source)),
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

    /// Subscribes a viewer to a browser's shared screencast producer.
    /// Enforces budget: max 2 viewers per browser, max 1 concurrently-captured browser.
    pub fn subscribe(
        &self,
        browser_id: &str,
        device_id: &str,
        viewer_instance_id: &str,
    ) -> Result<String, RemoteServiceError> {
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
        let stream_id = self.stream_counter.fetch_add(1, Ordering::SeqCst);

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
            },
        );

        drop(active_map);
        drop(subs_map);

        if was_empty {
            self.spawn_capture_producer(browser_id.to_string());
        }

        Ok(subscription_id)
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

    fn spawn_capture_producer(&self, browser_id: String) {
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
        let captures_in_progress = Arc::clone(&self.captures_in_progress);
        let snapshot_source_holder = Arc::clone(&self.snapshot_source);
        let manager = self.manager.clone();
        let desktop_epoch = Arc::clone(&self.desktop_epoch);
        let service_epoch = self.service_epoch;

        handle.spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(80)); // Maintain ~10-15 FPS (12.5 FPS)
            let mut seq: u32 = 0;

            loop {
                interval.tick().await;

                // While active subscribers > 0
                let has_active_subscribers = {
                    let subs = subscribers_map.lock();
                    subs.get(&browser_id).map(|s| !s.is_empty()).unwrap_or(false)
                };

                if !has_active_subscribers {
                    let mut active_map = producer_active.lock();
                    if let Some(active) = active_map.get_mut(&browser_id) {
                        if *active {
                            *active = false;
                            captures_in_progress.fetch_sub(1, Ordering::SeqCst);
                        }
                    }
                    break;
                }

                let state = match manager.get_state(&browser_id) {
                    Ok(s) => s,
                    Err(_) => break,
                };

                if !state.visible {
                    continue;
                }

                let source = snapshot_source_holder.read().clone();
                let snapshot_res = source
                    .take_snapshot(&state.webview_label, SnapshotOptions::jpeg(70))
                    .await;

                let snapshot = match snapshot_res {
                    Ok(s) => s,
                    Err(_) => continue,
                };

                seq = seq.wrapping_add(1);

                let (bounds, zoom, viewport_rev) = manager
                    .get_geometry(&browser_id)
                    .unwrap_or((None, 1.0, 1));
                let capture_rect = bounds.unwrap_or(LogicalRect {
                    x: 0.0,
                    y: 0.0,
                    width: snapshot.width as f64,
                    height: snapshot.height as f64,
                });

                let format_byte = match snapshot.format {
                    SnapshotFormat::Jpeg { .. } => super::remote_bridge_protocol::FRAME_FORMAT_JPEG,
                    SnapshotFormat::Png => super::remote_bridge_protocol::FRAME_FORMAT_PNG,
                };

                let metadata = super::remote_bridge_protocol::RemoteFrameMetadata {
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
                    stream_id: 1,
                    browser_instance_id: manager.get_instance_id(&browser_id).unwrap_or_default(),
                    browser_service_epoch: service_epoch.to_string(),
                    desktop_epoch: desktop_epoch.load(Ordering::SeqCst).to_string(),
                    document_generation: state.generation.to_string(),
                    viewport_revision: viewport_rev,
                    capture_rect,
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
}
