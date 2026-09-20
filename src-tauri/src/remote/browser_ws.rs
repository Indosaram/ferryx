//! Remote Browser WebSocket Per-Connection Lifetime, Actors & Dispatcher
//! Authoritative Spec: docs/plans/REMOTE_BROWSER_SCREENCAST_PLAN_2026-09-17.md (§4.3, §4.4, §5, §6.2)

use crate::browser::model::LogicalRect;
use crate::browser::remote_input::{map_point_mainframe, validate_point_timing_and_viewport};
use crate::remote::auth::DevicePermission;
use crate::remote::browser_admission::{
    AdmissionController, SubscriberQueue, CONTROL_QUEUE_MAX_COUNT,
};
use crate::remote::browser_backend::{
    BrowserCommandContext, RemoteBrowserBackend, RemoteBrowserError,
};
use crate::remote::browser_protocol::{
    is_decimal_u64_string, BrowserFrameMetadata, ClientMessage, ServerMessage, HEADER_BYTE_LENGTH,
    MAX_METADATA_BYTES,
};
use crate::remote::browser_security::{
    require_permission, sanitize_public_string, sanitize_url, RequestDeduplicator, MAX_FILL_BYTES,
    MAX_REQUEST_WIRE_BYTES, MAX_SCRIPT_BYTES,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

/// Number of most recently sent frames retained for point-click fencing (§4.5, R4-8).
pub const MAX_SENT_FRAME_RECORDS: usize = 16;

/// A frame this connection actually put on the wire. Point clicks may only
/// reference one of these records, never client-asserted geometry.
#[derive(Debug, Clone)]
pub struct SentFrameRecord {
    pub stream_id: u32,
    pub seq: u32,
    pub sent_at: Instant,
    pub metadata: BrowserFrameMetadata,
}

/// Authoritative desktop sharing status published to the GUI indicator (§6.1, R4-5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SharingDriverStatus {
    Idle,
    Viewing,
    Driving,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharingState {
    pub is_sharing: bool,
    pub active_sessions_count: usize,
    pub driver_status: SharingDriverStatus,
    pub driver_device_id: Option<String>,
}

#[derive(Default)]
struct SharingInner {
    /// connection_id -> device_id for every admitted viewer socket
    viewers: HashMap<String, String>,
    /// (connection_id, device_id) of the current remote driver
    driver: Option<(String, String)>,
    last_published: Option<SharingState>,
}

impl SharingInner {
    fn snapshot(&self) -> SharingState {
        let driver_device_id = self.driver.as_ref().map(|(_, dev)| dev.clone());
        let driver_status = if driver_device_id.is_some() {
            SharingDriverStatus::Driving
        } else if self.viewers.is_empty() {
            SharingDriverStatus::Idle
        } else {
            SharingDriverStatus::Viewing
        };
        SharingState {
            is_sharing: !self.viewers.is_empty(),
            active_sessions_count: self.viewers.len(),
            driver_status,
            driver_device_id,
        }
    }
}

/// Publishes authoritative remote-browser sharing state so the desktop indicator
/// reflects live daemon admission and revocation instead of guessing (R4-5).
pub struct SharingRegistry {
    inner: Mutex<SharingInner>,
    tx: tokio::sync::broadcast::Sender<SharingState>,
    listener: parking_lot::RwLock<Option<SharingStateListener>>,
}

pub type SharingStateListener =
    Arc<dyn Fn(&crate::browser::remote_bridge_protocol::BrowserSharingState) + Send + Sync>;

impl From<&SharingState> for crate::browser::remote_bridge_protocol::BrowserSharingState {
    fn from(s: &SharingState) -> Self {
        let driver_status = match s.driver_status {
            SharingDriverStatus::Idle => {
                crate::browser::remote_bridge_protocol::BrowserSharingDriverStatus::Idle
            }
            SharingDriverStatus::Viewing => {
                crate::browser::remote_bridge_protocol::BrowserSharingDriverStatus::Viewing
            }
            SharingDriverStatus::Driving => {
                crate::browser::remote_bridge_protocol::BrowserSharingDriverStatus::Driving
            }
        };
        crate::browser::remote_bridge_protocol::BrowserSharingState::new(
            s.is_sharing,
            s.active_sessions_count as u32,
            driver_status,
            s.driver_device_id.clone(),
        )
    }
}

impl SharingRegistry {
    pub fn new() -> Self {
        let (tx, _) = tokio::sync::broadcast::channel(64);
        Self {
            inner: Mutex::new(SharingInner::default()),
            tx,
            listener: parking_lot::RwLock::new(None),
        }
    }

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<SharingState> {
        self.tx.subscribe()
    }

    pub fn current(&self) -> SharingState {
        self.inner.lock().snapshot()
    }

    pub fn set_listener(&self, listener: SharingStateListener) {
        *self.listener.write() = Some(listener);
    }

    pub fn spawn_tauri_forwarder<F>(&self, emit: F) -> tokio::task::JoinHandle<()>
    where
        F: Fn(crate::browser::remote_bridge_protocol::BrowserSharingState) + Send + Sync + 'static,
    {
        let mut rx = self.subscribe();
        tokio::spawn(async move {
            while let Ok(state) = rx.recv().await {
                emit(crate::browser::remote_bridge_protocol::BrowserSharingState::from(&state));
            }
        })
    }

    fn publish(&self) {
        let mut guard = self.inner.lock();
        let state = guard.snapshot();
        if guard.last_published.as_ref() == Some(&state) {
            return;
        }
        guard.last_published = Some(state.clone());
        drop(guard);
        let _ = self.tx.send(state.clone());
        if let Some(listener) = self.listener.read().as_ref() {
            let dto = crate::browser::remote_bridge_protocol::BrowserSharingState::from(&state);
            listener(&dto);
        }
    }

    pub fn viewer_admitted(&self, connection_id: &str, device_id: &str) {
        self.inner
            .lock()
            .viewers
            .insert(connection_id.to_string(), device_id.to_string());
        self.publish();
    }

    pub fn viewer_removed(&self, connection_id: &str) {
        {
            let mut guard = self.inner.lock();
            guard.viewers.remove(connection_id);
            if guard
                .driver
                .as_ref()
                .is_some_and(|(conn, _)| conn == connection_id)
            {
                guard.driver = None;
            }
        }
        self.publish();
    }

    pub fn driver_claimed(&self, connection_id: &str, device_id: &str) {
        self.inner.lock().driver = Some((connection_id.to_string(), device_id.to_string()));
        self.publish();
    }

    pub fn driver_released(&self, connection_id: &str) {
        {
            let mut guard = self.inner.lock();
            if guard
                .driver
                .as_ref()
                .is_some_and(|(conn, _)| conn == connection_id)
            {
                guard.driver = None;
            }
        }
        self.publish();
    }
}

impl Default for SharingRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Reads the metadata block of an outbound browser frame without re-validating
/// the image payload. Returns `None` for buffers that are not complete frames.
pub fn parse_frame_metadata(bytes: &[u8]) -> Option<BrowserFrameMetadata> {
    if bytes.len() < HEADER_BYTE_LENGTH {
        return None;
    }
    let metadata_len = u32::from_le_bytes(bytes[8..12].try_into().ok()?) as usize;
    if metadata_len == 0 || metadata_len > MAX_METADATA_BYTES {
        return None;
    }
    let end = HEADER_BYTE_LENGTH.checked_add(metadata_len)?;
    if bytes.len() < end {
        return None;
    }
    serde_json::from_slice(&bytes[HEADER_BYTE_LENGTH..end]).ok()
}

pub fn sanitize_json_value(val: serde_json::Value) -> serde_json::Value {
    match val {
        serde_json::Value::String(s) => serde_json::Value::String(sanitize_public_string(&s)),
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(sanitize_json_value).collect())
        }
        serde_json::Value::Object(map) => {
            let mut new_map = serde_json::Map::with_capacity(map.len());
            for (k, v) in map {
                new_map.insert(sanitize_public_string(&k), sanitize_json_value(v));
            }
            serde_json::Value::Object(new_map)
        }
        other => other,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WsConnectionState {
    Opening,
    Ready,
    Streaming,
    Paused,
    Closing,
    Closed,
}

pub struct BrowserWsSession {
    pub connection_id: String,
    pub device_id: String,
    pub browser_id: String,
    pub permission: DevicePermission,
    pub state: WsConnectionState,
    pub is_driver: bool,
    pub lease_epoch: Option<u64>,
    pub subscription_id: Option<String>,
    pub stream_id: u32,
    pub queue: Option<SubscriberQueue>,
    pub backend_subscription_id: Option<String>,
    pub dedup: Arc<RequestDeduplicator>,
    pub eval_semaphore: Arc<Semaphore>,
    pub command_semaphore: Arc<Semaphore>,
    pub cancel_token: CancellationToken,
    pub last_heartbeat: Instant,
    pub pending_promoted_frame: Option<Vec<u8>>,
    /// Highest frame sequence this viewer confirmed as presented.
    pub last_acked_seq: Option<u32>,
    /// Bounded ledger of frames actually sent on this socket (R4-8 click fence).
    pub sent_frames: VecDeque<SentFrameRecord>,
    /// Authoritative sharing publisher for the desktop indicator (R4-5).
    pub sharing: Option<Arc<SharingRegistry>>,
    /// Real identity negotiated at subscription time (R4-7).
    pub negotiated_identity: Option<crate::remote::browser_backend::BrowserSubscribeIdentity>,
    /// Tracked spawned tasks to cancel on teardown (R6-4).
    pub tracked_tasks: Arc<parking_lot::Mutex<Vec<tokio::task::JoinHandle<()>>>>,
    /// Scope validator checking if session's desktop scope is still active (R6-4).
    pub scope_validator: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
}

/// Serializer map ensuring fill commands per (browser_id, lease_epoch, target) are queued
/// sequentially and never interleave on one target (R6-11).
static FILL_SERIALIZERS: std::sync::LazyLock<
    parking_lot::Mutex<
        std::collections::HashMap<(String, u64, String), Arc<tokio::sync::Mutex<()>>>,
    >,
> = std::sync::LazyLock::new(|| parking_lot::Mutex::new(std::collections::HashMap::new()));

fn get_fill_serializer(browser_id: &str, epoch: u64, target: &str) -> Arc<tokio::sync::Mutex<()>> {
    let mut map = FILL_SERIALIZERS.lock();
    if map.len() > 256 {
        map.retain(|_, v| Arc::strong_count(v) > 1);
    }
    map.entry((browser_id.to_string(), epoch, target.to_string()))
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}

fn browser_error(
    request_id: Option<String>,
    code: impl Into<String>,
    message: impl AsRef<str>,
    retryable: bool,
    retry_after_ms: Option<u64>,
) -> ServerMessage {
    ServerMessage::BrowserError {
        request_id,
        code: code.into(),
        message: sanitize_public_string(message.as_ref()),
        retryable,
        retry_after_ms,
    }
}

pub fn map_remote_browser_error(err: &RemoteBrowserError) -> (&'static str, bool, Option<u64>) {
    let msg = match err {
        RemoteBrowserError::ExecutionFailed(m)
        | RemoteBrowserError::InvalidRequest(m)
        | RemoteBrowserError::Unavailable(m)
        | RemoteBrowserError::Forbidden(m)
        | RemoteBrowserError::NotFound(m) => m.as_str(),
        RemoteBrowserError::WaitTimeout => "",
    };

    if msg.contains("BROWSER_STALE_FRAME") || msg.contains("stale frame") {
        return ("BROWSER_STALE_FRAME", false, None);
    }
    if msg.contains("BROWSER_TARGET_NOT_FOUND")
        || msg.contains("element not found")
        || msg.contains("no element at coordinates")
        || msg.contains("no active element")
        || msg.contains("target element not found")
        || msg.contains("target resolution failed")
        || msg.contains("missing click target")
    {
        return ("BROWSER_TARGET_NOT_FOUND", false, None);
    }

    match err {
        RemoteBrowserError::WaitTimeout => ("BROWSER_TIMEOUT", true, Some(1000)),
        RemoteBrowserError::Forbidden(_) => ("BROWSER_FORBIDDEN", false, None),
        RemoteBrowserError::Unavailable(_) => ("BROWSER_UNAVAILABLE", true, Some(2000)),
        RemoteBrowserError::NotFound(_) => ("BROWSER_NOT_FOUND", false, None),
        RemoteBrowserError::InvalidRequest(_) => ("BROWSER_INVALID_REQUEST", false, None),
        RemoteBrowserError::ExecutionFailed(_) => ("BROWSER_EXECUTION_FAILED", false, None),
    }
}

/// Accepts either a u64 decimal string or a non-negative integer and returns the
/// canonical decimal-string form required on the public wire (R4-9).
pub fn normalize_decimal_u64(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) => {
            let trimmed = s.trim();
            is_decimal_u64_string(trimmed).then(|| trimmed.to_string())
        }
        serde_json::Value::Number(n) => n.as_u64().map(|v| v.to_string()),
        _ => None,
    }
}

/// Converts a backend snapshot payload into the public reference catalogue.
/// Counts-only payloads are rejected: a click/fill reference cannot be mapped
/// from a number (R4-9).
pub fn snapshot_catalogue_from_backend(
    value: &serde_json::Value,
) -> Result<
    (
        String,
        String,
        Option<serde_json::Value>,
        Vec<serde_json::Value>,
    ),
    String,
> {
    let obj = value
        .as_object()
        .ok_or_else(|| "snapshot response is not an object".to_string())?;

    let snapshot_id = obj
        .get("snapshotId")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| "snapshot response is missing snapshotId".to_string())?
        .to_string();

    let map_revision = obj
        .get("mapRevision")
        .and_then(normalize_decimal_u64)
        .ok_or_else(|| "snapshot response mapRevision is not a u64 decimal".to_string())?;

    let raw_elements = obj
        .get("elements")
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            "snapshot response has no element reference catalogue (element counts are not a catalogue)"
                .to_string()
        })?;

    let mut elements = Vec::with_capacity(raw_elements.len());
    for element in raw_elements {
        let element_obj = element
            .as_object()
            .ok_or_else(|| "snapshot element is not an object".to_string())?;
        let reference = element_obj
            .get("ref")
            .or_else(|| element_obj.get("reference"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| "snapshot element is missing a public reference".to_string())?
            .to_string();

        let mut public = serde_json::Map::new();
        public.insert("ref".into(), serde_json::Value::String(reference));
        for (key, val) in element_obj {
            // `selector` is desktop-internal DOM detail and never crosses the public wire.
            if key == "ref" || key == "reference" || key == "selector" {
                continue;
            }
            public.insert(key.clone(), sanitize_json_value(val.clone()));
        }
        elements.push(serde_json::Value::Object(public));
    }

    let root = obj.get("root").cloned().map(sanitize_json_value);
    Ok((snapshot_id, map_revision, root, elements))
}

impl BrowserWsSession {
    pub fn new(
        connection_id: String,
        device_id: String,
        browser_id: String,
        permission: DevicePermission,
        now: Instant,
    ) -> Self {
        Self {
            connection_id,
            device_id,
            browser_id,
            permission,
            state: WsConnectionState::Opening,
            is_driver: false,
            lease_epoch: None,
            subscription_id: None,
            stream_id: 1,
            queue: None,
            backend_subscription_id: None,
            dedup: Arc::new(RequestDeduplicator::new()),
            eval_semaphore: Arc::new(Semaphore::new(1)),
            command_semaphore: Arc::new(Semaphore::new(CONTROL_QUEUE_MAX_COUNT)),
            cancel_token: CancellationToken::new(),
            last_heartbeat: now,
            pending_promoted_frame: None,
            last_acked_seq: None,
            sent_frames: VecDeque::new(),
            sharing: None,
            negotiated_identity: None,
            tracked_tasks: Arc::new(parking_lot::Mutex::new(Vec::new())),
            scope_validator: None,
        }
    }

    /// Binds this socket to the authoritative sharing publisher (R4-5).
    pub fn with_sharing_registry(mut self, registry: Arc<SharingRegistry>) -> Self {
        self.sharing = Some(registry);
        self
    }

    /// Binds an active scope validation check (R6-4).
    pub fn with_scope_validator(mut self, validator: Arc<dyn Fn() -> bool + Send + Sync>) -> Self {
        self.scope_validator = Some(validator);
        self
    }

    /// Revalidates that the session scope is still valid and not cancelled (R6-4).
    pub fn is_scope_valid(&self) -> bool {
        if self.cancel_token.is_cancelled() {
            return false;
        }
        if let Some(ref val) = self.scope_validator {
            val()
        } else {
            true
        }
    }

    /// Tracks a spawned task so it can be aborted on session teardown (R6-4).
    pub fn track_task(&self, handle: tokio::task::JoinHandle<()>) {
        let mut tasks = self.tracked_tasks.lock();
        tasks.retain(|h| !h.is_finished());
        tasks.push(handle);
    }

    /// Aborts all in-flight spawned command tasks immediately (R6-4).
    pub fn abort_all_command_tasks(&self) {
        let mut tasks = self.tracked_tasks.lock();
        for handle in tasks.drain(..) {
            handle.abort();
        }
    }

    fn record_sent_frame(&mut self, frame_bytes: &[u8], seq: u32, now: Instant) {
        let Some(metadata) = parse_frame_metadata(frame_bytes) else {
            return;
        };
        self.sent_frames.push_back(SentFrameRecord {
            stream_id: metadata.stream_id,
            seq,
            sent_at: now,
            metadata,
        });
        while self.sent_frames.len() > MAX_SENT_FRAME_RECORDS {
            self.sent_frames.pop_front();
        }
    }

    /// Drops stranded ACK credit so a resumed or restarted stream can deliver a
    /// fresh frame instead of waiting forever on a frame the viewer never saw (R4-10).
    fn reset_frame_credit(&mut self) {
        if let Some(queue) = self.queue.as_mut() {
            queue.unacked_seq = None;
            queue.unacked_sent_at = None;
            queue.pending_frame = None;
        }
        self.pending_promoted_frame = None;
        self.sent_frames.clear();
        self.last_acked_seq = None;
    }

    /// Marks the remote driver lease as revoked and republishes sharing state (R4-5).
    pub fn mark_driver_revoked(&mut self, _reason: &str) -> Option<u64> {
        let epoch = self.lease_epoch.take();
        self.is_driver = false;
        if let Some(registry) = self.sharing.as_ref() {
            registry.driver_released(&self.connection_id);
        }
        epoch
    }

    /// Resolves and fences a point click against the frames this socket actually
    /// sent, then rewrites the params with the real displayed geometry (R4-8).
    ///
    /// Returns `Err((code, message))` for every unfenceable click; reference-based
    /// clicks are passed through untouched.
    fn fence_point_click(
        &self,
        params: Option<serde_json::Value>,
    ) -> Result<Option<serde_json::Value>, (&'static str, String)> {
        let Some(serde_json::Value::Object(mut map)) = params else {
            return Err((
                "BROWSER_INVALID_REQUEST",
                "click requires either a snapshot reference or fenced point coordinates".into(),
            ));
        };

        // Reference clicks are fenced by the snapshot contract, not the frame ledger.
        if map
            .get("reference")
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.trim().is_empty())
            || map
                .get("selector")
                .and_then(|v| v.as_str())
                .is_some_and(|s| !s.trim().is_empty())
        {
            return Ok(Some(serde_json::Value::Object(map)));
        }

        let (Some(u), Some(v)) = (
            map.get("u").and_then(|v| v.as_f64()),
            map.get("v").and_then(|v| v.as_f64()),
        ) else {
            return Err((
                "BROWSER_INVALID_REQUEST",
                "click requires either a snapshot reference or (u, v) coordinates".into(),
            ));
        };

        // Displayed-frame identity is REQUIRED for point clicks; it is never optional.
        let stream_id = map
            .get("streamId")
            .and_then(|v| v.as_u64())
            .and_then(|v| u32::try_from(v).ok());
        let seq = map
            .get("sequenceNumber")
            .or_else(|| map.get("seq"))
            .and_then(|v| v.as_u64())
            .and_then(|v| u32::try_from(v).ok());
        let doc_gen = map
            .get("documentGeneration")
            .and_then(normalize_decimal_u64);
        let viewport_rev = map.get("viewportRevision").and_then(normalize_decimal_u64);
        let instance_id = map
            .get("browserInstanceId")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let (Some(stream_id), Some(seq), Some(doc_gen), Some(viewport_rev), Some(instance_id)) =
            (stream_id, seq, doc_gen, viewport_rev, instance_id)
        else {
            return Err((
                "BROWSER_INVALID_REQUEST",
                "point click requires displayed-frame metadata (streamId, sequenceNumber, documentGeneration, viewportRevision, browserInstanceId)"
                    .into(),
            ));
        };

        // A click may only reference a frame this connection actually sent.
        let record = self
            .sent_frames
            .iter()
            .rev()
            .find(|rec| rec.stream_id == stream_id && rec.seq == seq)
            .ok_or((
                "BROWSER_STALE_FRAME",
                "point click references a frame that was never sent on this stream".to_string(),
            ))?;

        // Frames older than the last frame the viewer acknowledged are superseded.
        if self.last_acked_seq.is_some_and(|acked| seq < acked) {
            return Err((
                "BROWSER_STALE_FRAME",
                "point click references a frame older than the last acknowledged frame".to_string(),
            ));
        }

        if record.metadata.browser_instance_id != instance_id
            || record.metadata.document_generation != doc_gen
            || record.metadata.viewport_revision != viewport_rev
        {
            return Err((
                "BROWSER_STALE_FRAME",
                "point click identity does not match the referenced sent frame".to_string(),
            ));
        }

        let frame_gen = record
            .metadata
            .document_generation
            .parse::<u64>()
            .map_err(|_| {
                (
                    "BROWSER_STALE_FRAME",
                    "sent frame has an invalid document generation".to_string(),
                )
            })?;
        let frame_vp = record
            .metadata
            .viewport_revision
            .parse::<u64>()
            .map_err(|_| {
                (
                    "BROWSER_STALE_FRAME",
                    "sent frame has an invalid viewport revision".to_string(),
                )
            })?;

        // Age fence: clicks on frames older than MAX_FRAME_AGE are rejected.
        validate_point_timing_and_viewport(
            record.sent_at,
            frame_vp,
            frame_vp,
            frame_gen,
            frame_gen,
        )
        .map_err(|e| ("BROWSER_STALE_FRAME", e.to_string()))?;

        // Real displayed geometry from the sent frame, never a guessed viewport.
        let rect = LogicalRect {
            x: record.metadata.capture_rect.x,
            y: record.metadata.capture_rect.y,
            width: record.metadata.capture_rect.width,
            height: record.metadata.capture_rect.height,
        };
        let point = map_point_mainframe(u, v, &rect, false, false, false)
            .map_err(|e| ("BROWSER_INVALID_REQUEST", e.to_string()))?;

        map.insert(
            "captureRect".into(),
            serde_json::json!({
                "x": rect.x,
                "y": rect.y,
                "width": rect.width,
                "height": rect.height,
            }),
        );
        map.insert("x".into(), serde_json::json!(point.x));
        map.insert("y".into(), serde_json::json!(point.y));
        map.insert(
            "geometrySource".into(),
            serde_json::Value::String(record.metadata.geometry_source.clone()),
        );
        map.insert("streamId".into(), serde_json::json!(stream_id));
        map.insert("sequenceNumber".into(), serde_json::json!(seq));
        map.insert(
            "documentGeneration".into(),
            serde_json::Value::String(doc_gen),
        );
        map.insert(
            "viewportRevision".into(),
            serde_json::Value::String(viewport_rev),
        );

        Ok(Some(serde_json::Value::Object(map)))
    }

    pub fn enqueue_frame(&mut self, frame_bytes: Vec<u8>, now: Instant) -> Option<Vec<u8>> {
        if self.state == WsConnectionState::Paused {
            return None;
        }
        let queue = self.queue.as_mut()?;
        let seq = if frame_bytes.len() >= 8 {
            u32::from_le_bytes(frame_bytes[4..8].try_into().unwrap_or([0; 4]))
        } else {
            0
        };
        match queue.enqueue_frame(seq, frame_bytes.clone(), now) {
            crate::remote::browser_admission::AdmissionOutcome::Admit => {
                self.record_sent_frame(&frame_bytes, seq, now);
                Some(frame_bytes)
            }
            _ => None,
        }
    }

    pub async fn dispatch_raw_text(
        &mut self,
        text: &str,
        backend: &Arc<dyn RemoteBrowserBackend>,
        admission: &AdmissionController,
        out_tx: &tokio::sync::mpsc::Sender<ServerMessage>,
        now: Instant,
    ) -> Result<(), String> {
        if text.len() > MAX_REQUEST_WIRE_BYTES {
            let err = browser_error(
                None,
                "BROWSER_INVALID_REQUEST",
                "Raw WebSocket message exceeds wire size limit of 64 KiB",
                false,
                None,
            );
            let _ = out_tx.send(err).await;
            return Ok(());
        }

        match serde_json::from_str::<ClientMessage>(text) {
            Ok(client_msg) => {
                self.dispatch_client_message(client_msg, backend, admission, out_tx, now)
                    .await
            }
            Err(e) => {
                let err = browser_error(
                    None,
                    "BROWSER_INVALID_REQUEST",
                    format!("Invalid JSON request: {}", e),
                    false,
                    None,
                );
                let _ = out_tx.send(err).await;
                Ok(())
            }
        }
    }

    pub async fn handle_raw_text(
        &mut self,
        text: &str,
        backend: &Arc<dyn RemoteBrowserBackend>,
        admission: &AdmissionController,
        now: Instant,
    ) -> Result<Option<ServerMessage>, String> {
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        self.dispatch_raw_text(text, backend, admission, &tx, now)
            .await?;
        Ok(rx.recv().await)
    }

    pub fn handle_client_binary(&mut self, _bytes: &[u8]) -> Result<(), String> {
        // §4.2: Client to server binary frames are strictly rejected
        Err(sanitize_public_string(
            "Client-to-server binary WebSocket frames are rejected",
        ))
    }

    pub async fn dispatch_client_message(
        &mut self,
        msg: ClientMessage,
        backend: &Arc<dyn RemoteBrowserBackend>,
        admission: &AdmissionController,
        out_tx: &tokio::sync::mpsc::Sender<ServerMessage>,
        now: Instant,
    ) -> Result<(), String> {
        let limiter = admission.get_device_limiter(&self.device_id, now);

        match msg {
            ClientMessage::BrowserSubscribe {
                request_id,
                viewer_instance_id,
                options,
            } => {
                let sub_id = format!("sub-{}", self.connection_id);
                if let Err(e) = admission.try_subscribe(
                    &self.browser_id,
                    &sub_id,
                    &self.device_id,
                    &viewer_instance_id,
                ) {
                    let err = browser_error(
                        Some(request_id),
                        "BROWSER_SUBSCRIPTION_FAILED",
                        format!("Subscription failed: {e}"),
                        false,
                        None,
                    );
                    let _ = out_tx.send(err).await;
                    return Ok(());
                }

                // Wire subscription ownership and reconcile stream identity with backend
                let (negotiated_options, identity) = match tokio::select! {
                    _ = self.cancel_token.cancelled() => return Ok(()),
                    res = backend.subscribe_viewer(
                        &self.browser_id,
                        &self.device_id,
                        &viewer_instance_id,
                        Some(options.clone()),
                    ) => res,
                } {
                    Ok((backend_sub, stream_id, negotiated, ident)) => {
                        self.backend_subscription_id = Some(backend_sub.clone());
                        self.stream_id = stream_id;
                        self.negotiated_identity = Some(ident.clone());
                        // R6-9: Register service subscription mapping immediately
                        admission.register_service_subscription(&sub_id, &backend_sub);
                        (negotiated, ident)
                    }
                    Err(e) => {
                        admission.unsubscribe(&self.browser_id, &sub_id);
                        let err = browser_error(
                            Some(request_id),
                            "BROWSER_SUBSCRIPTION_FAILED",
                            format!("Backend subscription failed: {e}"),
                            false,
                            None,
                        );
                        let _ = out_tx.send(err).await;
                        return Ok(());
                    }
                };

                // Post-await scope revalidation (R6-4)
                if !self.is_scope_valid() {
                    self.cancel_token.cancel();
                    return Ok(());
                }

                let queue = SubscriberQueue::new(sub_id.clone(), self.stream_id);
                self.queue = Some(queue);
                self.subscription_id = Some(sub_id.clone());
                self.state = WsConnectionState::Streaming;
                self.last_acked_seq = None;
                self.sent_frames.clear();
                if let Some(registry) = self.sharing.as_ref() {
                    registry.viewer_admitted(&self.connection_id, &self.device_id);
                }

                let resp = ServerMessage::BrowserSubscribed {
                    request_id,
                    subscription_id: sub_id,
                    stream_id: self.stream_id,
                    browser_id: self.browser_id.clone(),
                    browser_instance_id: identity.browser_instance_id,
                    browser_service_epoch: identity.browser_service_epoch,
                    desktop_epoch: identity.desktop_epoch,
                    document_generation: identity.document_generation,
                    options: negotiated_options,
                };
                let _ = out_tx.send(resp).await;
                Ok(())
            }

            ClientMessage::BrowserFrameAck { stream_id, seq } => {
                if self.state != WsConnectionState::Paused {
                    if stream_id == self.stream_id {
                        self.last_acked_seq = Some(match self.last_acked_seq {
                            Some(prev) if prev > seq => prev,
                            _ => seq,
                        });
                        if let Some(sub_id) = &self.subscription_id {
                            admission.set_viewer_stalled(&self.browser_id, sub_id, false);
                        }
                    }
                    if let Some(queue) = self.queue.as_mut() {
                        if let Some((promoted_seq, promoted_bytes)) =
                            queue.acknowledge_frame(stream_id, seq, now)
                        {
                            self.record_sent_frame(&promoted_bytes, promoted_seq, now);
                            self.pending_promoted_frame = Some(promoted_bytes);
                        }
                    }
                }
                Ok(())
            }

            ClientMessage::BrowserHeartbeat {
                request_id,
                lease_epoch,
                subscription_id,
            } => {
                self.last_heartbeat = now;
                if let Some(ref epoch_str) = lease_epoch {
                    let sub_id = self.subscription_id.as_deref().unwrap_or("");
                    let epoch = epoch_str.parse::<u64>().unwrap_or(0);
                    let sub_matches = subscription_id.as_deref() == Some(sub_id);
                    let epoch_matches = self.lease_epoch == Some(epoch);
                    if !self.is_driver
                        || !sub_matches
                        || !epoch_matches
                        || admission
                            .broker
                            .refresh_lease(
                                &self.device_id,
                                &self.connection_id,
                                sub_id,
                                &self.browser_id,
                                epoch,
                                now,
                            )
                            .is_err()
                    {
                        let err = browser_error(
                            request_id,
                            "BROWSER_INVALID_REQUEST",
                            "Driver lease 5-tuple mismatch or lease expired",
                            false,
                            None,
                        );
                        let _ = out_tx.send(err).await;
                        return Ok(());
                    }

                    // Synchronize lease renewal with the backend service broker (R5-8):
                    let hb_res = tokio::select! {
                        _ = self.cancel_token.cancelled() => return Ok(()),
                        r = backend.heartbeat_driver(&self.device_id, &self.connection_id, sub_id, epoch) => r,
                    };
                    if let Err(e) = hb_res {
                        let err = browser_error(
                            request_id,
                            "BROWSER_INVALID_REQUEST",
                            format!("Backend driver lease renewal failed: {e}"),
                            false,
                            None,
                        );
                        let _ = out_tx.send(err).await;
                        return Ok(());
                    }
                }

                // Authoritative document generation refresh on heartbeat (R5-7):
                if let Some(ref mut ident) = self.negotiated_identity {
                    let scope = crate::remote::browser_backend::DesktopScope {
                        workspace_id: "".into(),
                        worktree_slug: "".into(),
                    };
                    let st_res = tokio::select! {
                        _ = self.cancel_token.cancelled() => return Ok(()),
                        r = backend.get_state(&self.browser_id, &scope) => r,
                    };
                    if let Ok(state) = st_res {
                        ident.document_generation = state.document_generation;
                    }
                }

                // Post-await scope revalidation (R6-4)
                if !self.is_scope_valid() {
                    self.cancel_token.cancel();
                    return Ok(());
                }

                let resp = ServerMessage::BrowserPong {
                    request_id,
                    timestamp: Some(now.elapsed().as_secs_f64()),
                };
                let _ = out_tx.send(resp).await;
                Ok(())
            }

            ClientMessage::BrowserDriverClaim {
                request_id,
                subscription_id,
                browser_id,
            } => {
                if let Err(e) = require_permission(self.permission, DevicePermission::Control) {
                    let err = browser_error(
                        Some(request_id),
                        "BROWSER_FORBIDDEN",
                        e.to_string(),
                        false,
                        None,
                    );
                    let _ = out_tx.send(err).await;
                    return Ok(());
                }

                // P1-04: Reject BrowserDriverClaim if client has not subscribed yet
                // or if subscription_id / browser_id does not match
                if self.subscription_id.is_none()
                    || self.subscription_id.as_deref() != Some(&subscription_id)
                    || browser_id != self.browser_id
                {
                    let err = browser_error(
                        Some(request_id),
                        "BROWSER_INVALID_REQUEST",
                        "Subscription binding mismatch or not subscribed",
                        false,
                        None,
                    );
                    let _ = out_tx.send(err).await;
                    return Ok(());
                }

                if !limiter.check_claim(now) {
                    let err = browser_error(
                        Some(request_id),
                        "BROWSER_RATE_LIMITED",
                        "Driver claim rate limit exceeded",
                        true,
                        Some(500),
                    );
                    let _ = out_tx.send(err).await;
                    return Ok(());
                }

                match admission.broker.claim_driver(
                    &self.device_id,
                    &self.connection_id,
                    &subscription_id,
                    &browser_id,
                    now,
                ) {
                    Ok(lease) => {
                        let claim_res = tokio::select! {
                            _ = self.cancel_token.cancelled() => return Ok(()),
                            r = backend.claim_driver(
                                &browser_id,
                                &self.device_id,
                                &self.connection_id,
                                &subscription_id,
                                lease.lease_epoch,
                            ) => r,
                        };
                        match claim_res {
                            Ok(()) => {
                                // Post-await scope revalidation (R6-4)
                                if !self.is_scope_valid() {
                                    self.cancel_token.cancel();
                                    return Ok(());
                                }
                                self.is_driver = true;
                                self.lease_epoch = Some(lease.lease_epoch);
                                if let Some(registry) = self.sharing.as_ref() {
                                    registry.driver_claimed(&self.connection_id, &self.device_id);
                                }
                                let resp = ServerMessage::BrowserDriverClaimed {
                                    request_id,
                                    lease_epoch: lease.lease_epoch.to_string(),
                                    expires_at: lease.expires_at.elapsed().as_secs_f64(),
                                };
                                let _ = out_tx.send(resp).await;
                                Ok(())
                            }
                            Err(e) => {
                                let _ = admission.broker.release_driver(
                                    &self.device_id,
                                    &self.connection_id,
                                    &subscription_id,
                                    &browser_id,
                                    lease.lease_epoch,
                                );
                                let err = browser_error(
                                    Some(request_id),
                                    match e {
                                        crate::remote::browser_backend::RemoteBrowserError::Forbidden(_) => "BROWSER_FORBIDDEN",
                                        crate::remote::browser_backend::RemoteBrowserError::NotFound(_) => "BROWSER_NOT_FOUND",
                                        _ => "BROWSER_DRIVER_BUSY",
                                    },
                                    format!("Backend driver claim failed: {e}"),
                                    false,
                                    None,
                                );
                                let _ = out_tx.send(err).await;
                                Ok(())
                            }
                        }
                    }
                    Err(code) => {
                        let err = browser_error(
                            Some(request_id),
                            code,
                            "Another remote driver is active",
                            true,
                            Some(1000),
                        );
                        let _ = out_tx.send(err).await;
                        Ok(())
                    }
                }
            }

            ClientMessage::BrowserDriverRelease {
                request_id,
                lease_epoch,
            } => {
                let epoch = lease_epoch.parse::<u64>().unwrap_or(0);
                let sub_id = self.subscription_id.as_deref().unwrap_or("");
                let epoch_matches = self.lease_epoch == Some(epoch);
                if !self.is_driver
                    || !epoch_matches
                    || !admission.broker.release_driver(
                        &self.device_id,
                        &self.connection_id,
                        sub_id,
                        &self.browser_id,
                        epoch,
                    )
                {
                    let err = browser_error(
                        Some(request_id),
                        "BROWSER_INVALID_REQUEST",
                        "Driver lease 5-tuple mismatch or not active driver",
                        false,
                        None,
                    );
                    let _ = out_tx.send(err).await;
                    return Ok(());
                }
                self.is_driver = false;
                self.lease_epoch = None;
                let _ = tokio::select! {
                    _ = self.cancel_token.cancelled() => return Ok(()),
                    r = backend.release_driver(sub_id, epoch) => r,
                };
                if !self.is_scope_valid() {
                    self.cancel_token.cancel();
                    return Ok(());
                }
                if let Some(registry) = self.sharing.as_ref() {
                    registry.driver_released(&self.connection_id);
                }
                let resp = ServerMessage::BrowserDriverReleased {
                    request_id,
                    lease_epoch: Some(lease_epoch),
                };
                let _ = out_tx.send(resp).await;
                Ok(())
            }

            ClientMessage::BrowserCommand {
                request_id,
                request_seq,
                browser_id,
                lease_epoch,
                browser_instance_id,
                desktop_epoch,
                document_generation,
                command,
                params,
            } => {
                // P1-04: Reject BrowserCommand if browser_id != self.browser_id or self.subscription_id.is_none()
                if browser_id != self.browser_id || self.subscription_id.is_none() {
                    let err = browser_error(
                        Some(request_id),
                        "BROWSER_INVALID_REQUEST",
                        "Invalid browser or subscription binding",
                        false,
                        None,
                    );
                    let _ = out_tx.send(err).await;
                    return Ok(());
                }

                // R4-7 Part B: Command identity fencing against negotiated session identity
                let Some(ref negotiated) = self.negotiated_identity else {
                    let err = browser_error(
                        Some(request_id),
                        "BROWSER_INVALID_REQUEST",
                        "Command received without active negotiated subscription",
                        false,
                        None,
                    );
                    let _ = out_tx.send(err).await;
                    return Ok(());
                };

                if browser_instance_id.trim().is_empty()
                    || browser_instance_id != negotiated.browser_instance_id
                {
                    let err = browser_error(
                        Some(request_id),
                        "BROWSER_STALE_IDENTITY",
                        format!(
                            "Browser instance ID mismatch: expected {}, got {}",
                            negotiated.browser_instance_id, browser_instance_id
                        ),
                        false,
                        None,
                    );
                    let _ = out_tx.send(err).await;
                    return Ok(());
                }

                if desktop_epoch.trim().is_empty() || desktop_epoch != negotiated.desktop_epoch {
                    let err = browser_error(
                        Some(request_id),
                        "BROWSER_STALE_IDENTITY",
                        format!(
                            "Desktop epoch mismatch: expected {}, got {}",
                            negotiated.desktop_epoch, desktop_epoch
                        ),
                        false,
                        None,
                    );
                    let _ = out_tx.send(err).await;
                    return Ok(());
                }

                if document_generation.trim().is_empty()
                    || document_generation != negotiated.document_generation
                {
                    let err = browser_error(
                        Some(request_id),
                        "BROWSER_STALE_IDENTITY",
                        format!(
                            "Document generation mismatch: expected {}, got {}",
                            negotiated.document_generation, document_generation
                        ),
                        false,
                        None,
                    );
                    let _ = out_tx.send(err).await;
                    return Ok(());
                }

                // P1-06: Boundary size limits on incoming commands:
                // command name <= 64 KiB, script in params <= 32 KiB, fill text <= 16 KiB
                if command.len() > MAX_REQUEST_WIRE_BYTES {
                    let err = browser_error(
                        Some(request_id),
                        "BROWSER_INVALID_REQUEST",
                        "Command name exceeds limit",
                        false,
                        None,
                    );
                    let _ = out_tx.send(err).await;
                    return Ok(());
                }

                if let Some(ref p) = params {
                    if let Some(script) = p.get("script").and_then(|v| v.as_str()) {
                        if script.len() > MAX_SCRIPT_BYTES {
                            let err = browser_error(
                                Some(request_id),
                                "BROWSER_INVALID_REQUEST",
                                "Script exceeds maximum size of 32 KiB",
                                false,
                                None,
                            );
                            let _ = out_tx.send(err).await;
                            return Ok(());
                        }
                    }
                    if let Some(fill_val) = p
                        .get("text")
                        .or_else(|| p.get("value"))
                        .and_then(|v| v.as_str())
                    {
                        if fill_val.len() > MAX_FILL_BYTES {
                            let err = browser_error(
                                Some(request_id),
                                "BROWSER_INVALID_REQUEST",
                                "Fill text exceeds maximum size of 16 KiB",
                                false,
                                None,
                            );
                            let _ = out_tx.send(err).await;
                            return Ok(());
                        }
                    }
                }

                if !limiter.check_command(now) {
                    let err = browser_error(
                        Some(request_id),
                        "BROWSER_RATE_LIMITED",
                        "Command rate limit exceeded",
                        true,
                        Some(250),
                    );
                    let _ = out_tx.send(err).await;
                    return Ok(());
                }

                if let Err(e) = require_permission(self.permission, DevicePermission::Control) {
                    let err = browser_error(
                        Some(request_id),
                        "BROWSER_FORBIDDEN",
                        e.to_string(),
                        false,
                        None,
                    );
                    let _ = out_tx.send(err).await;
                    return Ok(());
                }

                let epoch = lease_epoch.parse::<u64>().unwrap_or(0);
                let sub_id = self.subscription_id.as_deref().unwrap_or("");
                // P1-04: verify the complete (device_id, connection_id, subscription_id, browser_id, epoch) tuple
                if !admission.broker.is_active_driver(
                    &self.device_id,
                    &self.connection_id,
                    sub_id,
                    &self.browser_id,
                    epoch,
                    now,
                ) {
                    let err = browser_error(
                        Some(request_id),
                        "BROWSER_DRIVER_BUSY",
                        "Not currently active driver",
                        false,
                        None,
                    );
                    let _ = out_tx.send(err).await;
                    return Ok(());
                }

                let seq_num = request_seq.parse::<u64>().unwrap_or(0);
                match self.dedup.check_or_record(seq_num, now) {
                    Ok(Some(cached)) => {
                        let val: Option<serde_json::Value> = serde_json::from_slice(&cached).ok();
                        let resp = ServerMessage::BrowserResult {
                            request_id,
                            result: val,
                        };
                        let _ = out_tx.send(resp).await;
                        return Ok(());
                    }
                    Ok(None) => {}
                    Err(e) => {
                        let err = browser_error(
                            Some(request_id),
                            "BROWSER_INVALID_REQUEST",
                            e.to_string(),
                            false,
                            None,
                        );
                        let _ = out_tx.send(err).await;
                        return Ok(());
                    }
                }

                // If command is navigate, validate URL scheme
                if command == "navigate" {
                    if let Some(ref p) = params {
                        if let Some(url_str) = p.get("url").and_then(|v| v.as_str()) {
                            if let Err(e) = sanitize_url(url_str) {
                                let err = browser_error(
                                    Some(request_id),
                                    "BROWSER_INVALID_REQUEST",
                                    e.to_string(),
                                    false,
                                    None,
                                );
                                let _ = out_tx.send(err).await;
                                return Ok(());
                            }
                        }
                    }
                }

                let is_eval = command == "eval";
                if is_eval && !limiter.check_eval(now) {
                    let err = browser_error(
                        Some(request_id),
                        "BROWSER_RATE_LIMITED",
                        "Eval rate limit exceeded",
                        true,
                        Some(1000),
                    );
                    let _ = out_tx.send(err).await;
                    return Ok(());
                }

                // R4-8: point clicks are fenced against frames this socket actually sent
                // and rewritten with the real displayed geometry before execution.
                let params = if command == "click" {
                    match self.fence_point_click(params) {
                        Ok(p) => p,
                        Err((code, message)) => {
                            let err = browser_error(Some(request_id), code, message, false, None);
                            let _ = out_tx.send(err).await;
                            return Ok(());
                        }
                    }
                } else {
                    params
                };

                let cmd_permit = match self.command_semaphore.clone().try_acquire_owned() {
                    Ok(p) => p,
                    Err(_) => {
                        let err = browser_error(
                            Some(request_id),
                            "BROWSER_RATE_LIMITED",
                            "Command queue limit exceeded",
                            true,
                            Some(250),
                        );
                        let _ = out_tx.send(err).await;
                        return Ok(());
                    }
                };

                // R5-13: IME fills supersession tracking per (browser_id, lease_epoch, target)
                let maybe_fill_rev = if command == "fill" {
                    params.as_ref().and_then(|p| {
                        p.get("revision")
                            .or_else(|| p.get("imeRevision"))
                            .and_then(|v| {
                                v.as_u64()
                                    .or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok()))
                            })
                    })
                } else {
                    None
                };
                let fill_target = if command == "fill" {
                    params
                        .as_ref()
                        .and_then(|p| p.get("reference").or_else(|| p.get("selector")))
                        .and_then(|v| v.as_str())
                        .unwrap_or("active")
                        .to_string()
                } else {
                    String::new()
                };

                if let Some(rev) = maybe_fill_rev {
                    let mut tracker = crate::browser::remote_input::IME_SUPERSESSION_TRACKER.lock();
                    let entry = tracker
                        .entry((browser_id.clone(), epoch, fill_target.clone()))
                        .or_insert(0);
                    if rev > *entry {
                        *entry = rev;
                    }
                }

                let is_fill = command == "fill";
                let fill_serializer = if is_fill {
                    Some(get_fill_serializer(&browser_id, epoch, &fill_target))
                } else {
                    None
                };
                let scope_validator = self.scope_validator.clone();

                // P1-07: TypeSafe-approved spawned_command_tasks pattern:
                // Spawn command execution onto an async task with results returned to out_tx.
                // Reader loop is NOT blocked!
                let ctx = BrowserCommandContext {
                    browser_id: browser_id.clone(),
                    command,
                    params,
                    document_generation: Some(document_generation),
                    browser_instance_id: Some(browser_instance_id),
                    desktop_epoch: Some(desktop_epoch),
                    lease_epoch: Some(lease_epoch),
                    device_id: Some(self.device_id.clone()),
                    connection_id: Some(self.connection_id.clone()),
                };
                let backend = Arc::clone(backend);
                let out_tx = out_tx.clone();
                let dedup = Arc::clone(&self.dedup);
                let eval_semaphore = Arc::clone(&self.eval_semaphore);
                let cancel_token = self.cancel_token.clone();
                let broker = Arc::clone(&admission.broker);
                let device_id = self.device_id.clone();
                let connection_id = self.connection_id.clone();
                let sub_id_str = sub_id.to_string();
                let browser_id_str = self.browser_id.clone();

                let handle = tokio::spawn(async move {
                    let _held_cmd_permit = cmd_permit;

                    if cancel_token.is_cancelled() {
                        return;
                    }
                    if let Some(ref val) = scope_validator {
                        if !val() {
                            cancel_token.cancel();
                            return;
                        }
                    }

                    let eval_permit = if is_eval {
                        let p = tokio::select! {
                            _ = cancel_token.cancelled() => return,
                            res = eval_semaphore.acquire() => {
                                match res {
                                    Ok(permit) => permit,
                                    Err(_) => {
                                        let err_reply = browser_error(
                                            Some(request_id),
                                            "BROWSER_EXECUTION_FAILED",
                                            "Eval dispatcher closed",
                                            false,
                                            None,
                                        );
                                        let _ = out_tx.send(err_reply).await;
                                        return;
                                    }
                                }
                            }
                        };
                        Some(p)
                    } else {
                        None
                    };

                    // R6-11: Acquire per-target fill serializer lock to serialize concurrent fills
                    let _held_fill_lock = if let Some(ref lock) = fill_serializer {
                        let guard = tokio::select! {
                            _ = cancel_token.cancelled() => {
                                drop(eval_permit);
                                return;
                            }
                            g = lock.lock() => g,
                        };
                        Some(guard)
                    } else {
                        None
                    };

                    // Execution-time lease revalidation: RE-VALIDATE that the driver lease is still valid
                    // and has not expired or been revoked before executing! If lease expired while waiting
                    // for permit or lock, abort with BROWSER_FORBIDDEN.
                    let now_exec = Instant::now();
                    if !broker.is_active_driver(
                        &device_id,
                        &connection_id,
                        &sub_id_str,
                        &browser_id_str,
                        epoch,
                        now_exec,
                    ) {
                        drop(_held_fill_lock);
                        drop(eval_permit);
                        let err_reply = browser_error(
                            Some(request_id),
                            "BROWSER_FORBIDDEN",
                            "Driver lease expired or revoked before execution",
                            false,
                            None,
                        );
                        let _ = out_tx.send(err_reply).await;
                        return;
                    }

                    // R5-13, R6-11: Enforce supersession at the mutation boundary (under the fill lock)
                    if let Some(rev) = maybe_fill_rev {
                        let is_superseded = {
                            let tracker =
                                crate::browser::remote_input::IME_SUPERSESSION_TRACKER.lock();
                            tracker
                                .get(&(browser_id_str.clone(), epoch, fill_target.clone()))
                                .copied()
                                .map(|max_rev| max_rev > rev)
                                .unwrap_or(false)
                        };
                        if is_superseded {
                            drop(_held_fill_lock);
                            drop(eval_permit);
                            let reply = ServerMessage::BrowserResult {
                                request_id,
                                result: Some(
                                    serde_json::json!({ "filled": true, "superseded": true }),
                                ),
                            };
                            let _ = out_tx.send(reply).await;
                            return;
                        }
                    }

                    let res = tokio::select! {
                        _ = cancel_token.cancelled() => {
                            drop(_held_fill_lock);
                            drop(eval_permit);
                            return;
                        }
                        r = backend.execute_command(ctx) => {
                            drop(_held_fill_lock);
                            drop(eval_permit);
                            r
                        }
                    };

                    // R6-4: Post-await scope revalidation before publishing
                    if cancel_token.is_cancelled() {
                        return;
                    }
                    if let Some(ref val) = scope_validator {
                        if !val() {
                            cancel_token.cancel();
                            return;
                        }
                    }

                    let reply = match res {
                        Ok(result) => {
                            let json_val = result.value.map(sanitize_json_value);
                            if let Some(ref v) = json_val {
                                if let Ok(bytes) = serde_json::to_vec(v) {
                                    dedup.record_result(seq_num, bytes, Instant::now());
                                }
                            }
                            ServerMessage::BrowserResult {
                                request_id,
                                result: json_val,
                            }
                        }
                        Err(e) => {
                            // P2-03, R6-6: Preserve typed error codes from RemoteBrowserError
                            let (code, retryable, retry_after_ms) = map_remote_browser_error(&e);
                            browser_error(
                                Some(request_id),
                                code,
                                e.to_string(),
                                retryable,
                                retry_after_ms,
                            )
                        }
                    };
                    let _ = out_tx.send(reply).await;
                });
                self.track_task(handle);

                Ok(())
            }

            ClientMessage::BrowserUnsubscribe {
                request_id,
                subscription_id,
            } => {
                if let Some(backend_sub) = self.backend_subscription_id.take() {
                    let _ = backend
                        .unsubscribe_viewer(&self.browser_id, &backend_sub)
                        .await;
                }
                self.teardown(admission);
                let resp = ServerMessage::BrowserUnsubscribed {
                    request_id,
                    subscription_id,
                };
                let _ = out_tx.send(resp).await;
                Ok(())
            }

            ClientMessage::BrowserSnapshot {
                request_id,
                browser_id,
            } => {
                // R4-9: snapshots are View-accessible; they read page structure only.
                if browser_id != self.browser_id || self.subscription_id.is_none() {
                    let err = browser_error(
                        Some(request_id),
                        "BROWSER_INVALID_REQUEST",
                        "Invalid browser or subscription binding",
                        false,
                        None,
                    );
                    let _ = out_tx.send(err).await;
                    return Ok(());
                }

                if !limiter.check_command(now) {
                    let err = browser_error(
                        Some(request_id),
                        "BROWSER_RATE_LIMITED",
                        "Command rate limit exceeded",
                        true,
                        Some(250),
                    );
                    let _ = out_tx.send(err).await;
                    return Ok(());
                }

                // R6-4: Scope-cancellable snapshot backend call
                let res = tokio::select! {
                    _ = self.cancel_token.cancelled() => return Ok(()),
                    r = backend
                        .execute_command(BrowserCommandContext {
                            browser_id: self.browser_id.clone(),
                            command: "snapshot".into(),
                            params: None,
                            document_generation: None,
                            ..Default::default()
                        }) => r,
                };

                // R6-4: Revalidate scope after await before publishing
                if !self.is_scope_valid() {
                    self.cancel_token.cancel();
                    return Ok(());
                }

                let reply = match res {
                    Ok(result) => {
                        match result.value.as_ref().map(snapshot_catalogue_from_backend) {
                            Some(Ok((snapshot_id, map_revision, root, elements))) => {
                                ServerMessage::BrowserSnapshot {
                                    request_id,
                                    snapshot_id,
                                    map_revision,
                                    root,
                                    elements,
                                }
                            }
                            Some(Err(e)) => browser_error(
                                Some(request_id),
                                "BROWSER_EXECUTION_FAILED",
                                e,
                                false,
                                None,
                            ),
                            None => browser_error(
                                Some(request_id),
                                "BROWSER_EXECUTION_FAILED",
                                "snapshot returned no result",
                                false,
                                None,
                            ),
                        }
                    }
                    Err(e) => {
                        let (code, retryable, retry_after_ms) = map_remote_browser_error(&e);
                        browser_error(
                            Some(request_id),
                            code,
                            e.to_string(),
                            retryable,
                            retry_after_ms,
                        )
                    }
                };
                let _ = out_tx.send(reply).await;
                Ok(())
            }

            ClientMessage::BrowserPause {
                browser_id,
                stream_id,
            } => {
                // R4-10: pause/resume are bound to (browserId, streamId) exactly as sent.
                if browser_id != self.browser_id || stream_id != self.stream_id {
                    let err = browser_error(
                        None,
                        "BROWSER_INVALID_REQUEST",
                        "Pause does not match the active browser/stream binding",
                        false,
                        None,
                    );
                    let _ = out_tx.send(err).await;
                    return Ok(());
                }
                self.state = WsConnectionState::Paused;
                if let Some(sub_id) = &self.subscription_id {
                    admission.set_viewer_paused(&self.browser_id, sub_id, true);
                }
                Ok(())
            }

            ClientMessage::BrowserResume {
                browser_id,
                stream_id,
            } => {
                if browser_id != self.browser_id || stream_id != self.stream_id {
                    let err = browser_error(
                        None,
                        "BROWSER_INVALID_REQUEST",
                        "Resume does not match the active browser/stream binding",
                        false,
                        None,
                    );
                    let _ = out_tx.send(err).await;
                    return Ok(());
                }
                if let Some(sub_id) = self.subscription_id.clone() {
                    // A frame dropped while paused would otherwise stay unacked forever and
                    // stall every later frame; drop stale credit and take a fresh frame.
                    self.reset_frame_credit();
                    self.state = WsConnectionState::Streaming;
                    admission.set_viewer_paused(&self.browser_id, &sub_id, false);
                }
                Ok(())
            }
        }
    }

    pub async fn handle_client_message(
        &mut self,
        msg: ClientMessage,
        backend: &Arc<dyn RemoteBrowserBackend>,
        admission: &AdmissionController,
        now: Instant,
    ) -> Result<Option<ServerMessage>, String> {
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        self.dispatch_client_message(msg, backend, admission, &tx, now)
            .await?;
        Ok(rx.recv().await)
    }

    pub fn teardown(&mut self, admission: &AdmissionController) {
        self.cancel_token.cancel();
        self.abort_all_command_tasks();
        let sub_id = self.subscription_id.take();
        if let Some(ref sub) = sub_id {
            admission.unsubscribe(&self.browser_id, sub);
        }
        if let Some(epoch) = self.lease_epoch.take() {
            let s_id = sub_id.as_deref().unwrap_or("");
            admission.broker.release_driver(
                &self.device_id,
                &self.connection_id,
                s_id,
                &self.browser_id,
                epoch,
            );
        }
        if let Some(queue) = self.queue.as_mut() {
            queue.close();
        }
        self.is_driver = false;
        self.sent_frames.clear();
        self.last_acked_seq = None;
        self.state = WsConnectionState::Closed;
        if let Some(registry) = self.sharing.as_ref() {
            registry.viewer_removed(&self.connection_id);
        }
    }

    pub async fn teardown_with_backend(
        &mut self,
        admission: &AdmissionController,
        backend: &Arc<dyn RemoteBrowserBackend>,
    ) {
        // R6-4: Cancel outstanding operations BEFORE bounded backend cleanup
        self.cancel_token.cancel();
        self.abort_all_command_tasks();

        const CLEANUP_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(500);
        let backend_cleanup = async {
            if let Some(backend_sub) = self.backend_subscription_id.take() {
                let _ = backend
                    .unsubscribe_viewer(&self.browser_id, &backend_sub)
                    .await;
                if let Some(epoch) = self.lease_epoch {
                    let sub_id = self.subscription_id.clone().unwrap_or_default();
                    let _ = backend.release_driver(&sub_id, epoch).await;
                }
            }
        };
        let _ = tokio::time::timeout(CLEANUP_TIMEOUT, backend_cleanup).await;

        self.teardown(admission);
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::remote::browser_backend::{BrowserCommandResult, InProcessTestBackend};
    use crate::remote::browser_protocol::BrowserSubscribeOptions;

    #[test]
    fn test_ws_rejects_client_to_server_binary() {
        let mut session = BrowserWsSession::new(
            "c1".into(),
            "d1".into(),
            "b1".into(),
            DevicePermission::Control,
            Instant::now(),
        );
        let dummy_binary = vec![0x62, 1, 1, 1];
        let res = session.handle_client_binary(&dummy_binary);
        assert!(res.is_err(), "Client binary must be rejected");
    }

    #[tokio::test]
    async fn test_ws_lifecycle_subscribe_claim_command_teardown() {
        let now = Instant::now();
        let mut session = BrowserWsSession::new(
            "c1".into(),
            "d1".into(),
            "b1".into(),
            DevicePermission::Control,
            now,
        );
        let backend: Arc<dyn RemoteBrowserBackend> = Arc::new(InProcessTestBackend::new());
        let admission = AdmissionController::new();

        // Subscribe
        let sub_msg = ClientMessage::BrowserSubscribe {
            request_id: "r1".into(),
            viewer_instance_id: "v1".into(),
            options: BrowserSubscribeOptions {
                format: crate::remote::browser_protocol::BrowserImageFormat::Jpeg,
                quality: Some(70),
                interval_ms: Some(250),
                max_edge: Some(1280),
            },
        };
        let resp = session
            .handle_client_message(sub_msg, &backend, &admission, now)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(resp, ServerMessage::BrowserSubscribed { .. }));
        assert_eq!(session.state, WsConnectionState::Streaming);

        // Driver claim
        let claim_msg = ClientMessage::BrowserDriverClaim {
            request_id: "r2".into(),
            subscription_id: session.subscription_id.clone().unwrap(),
            browser_id: "b1".into(),
        };
        let resp2 = session
            .handle_client_message(claim_msg, &backend, &admission, now)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(resp2, ServerMessage::BrowserDriverClaimed { .. }));
        assert!(session.is_driver);

        // Execute command
        let cmd_msg = ClientMessage::BrowserCommand {
            request_id: "r3".into(),
            request_seq: "1".into(),
            browser_id: "b1".into(),
            lease_epoch: session.lease_epoch.unwrap().to_string(),
            browser_instance_id: "bi1".into(),
            desktop_epoch: "1".into(),
            document_generation: "1".into(),
            command: "getState".into(),
            params: None,
        };
        let resp3 = session
            .handle_client_message(cmd_msg, &backend, &admission, now)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(resp3, ServerMessage::BrowserResult { .. }));

        // Teardown
        session.teardown(&admission);
        assert_eq!(session.state, WsConnectionState::Closed);
        assert!(!session.is_driver);
    }

    #[tokio::test]
    async fn test_ws_bounded_eval_dispatcher_does_not_block_heartbeat_or_release() {
        let now = Instant::now();
        let mut session = BrowserWsSession::new(
            "c3".into(),
            "d3".into(),
            "b1".into(),
            DevicePermission::Control,
            now,
        );
        let backend: Arc<dyn RemoteBrowserBackend> = Arc::new(InProcessTestBackend::new());
        let admission = AdmissionController::new();

        // Subscribe & claim driver
        let sub_msg = ClientMessage::BrowserSubscribe {
            request_id: "r1".into(),
            viewer_instance_id: "v1".into(),
            options: BrowserSubscribeOptions {
                format: crate::remote::browser_protocol::BrowserImageFormat::Jpeg,
                quality: None,
                interval_ms: None,
                max_edge: None,
            },
        };
        session
            .handle_client_message(sub_msg, &backend, &admission, now)
            .await
            .unwrap();

        let claim_msg = ClientMessage::BrowserDriverClaim {
            request_id: "r2".into(),
            subscription_id: session.subscription_id.clone().unwrap(),
            browser_id: "b1".into(),
        };
        session
            .handle_client_message(claim_msg, &backend, &admission, now)
            .await
            .unwrap();
        assert!(session.is_driver);

        // Heartbeat is handled immediately even if eval permit is acquired
        let _permit = session
            .eval_semaphore
            .clone()
            .acquire_owned()
            .await
            .unwrap();

        let hb_msg = ClientMessage::BrowserHeartbeat {
            request_id: Some("hb1".into()),
            lease_epoch: session.lease_epoch.map(|e| e.to_string()),
            subscription_id: session.subscription_id.clone(),
        };
        let hb_resp = session
            .handle_client_message(hb_msg, &backend, &admission, now)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(hb_resp, ServerMessage::BrowserPong { .. }));

        // Driver release is also handled immediately without blocking on eval permit!
        let release_msg = ClientMessage::BrowserDriverRelease {
            request_id: "rel1".into(),
            lease_epoch: session.lease_epoch.unwrap().to_string(),
        };
        let rel_resp = session
            .handle_client_message(release_msg, &backend, &admission, now)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            rel_resp,
            ServerMessage::BrowserDriverReleased { .. }
        ));
        assert!(!session.is_driver);
    }

    #[tokio::test]
    async fn test_ws_view_only_permission_cannot_claim_or_command() {
        let now = Instant::now();
        let mut session = BrowserWsSession::new(
            "c2".into(),
            "d2".into(),
            "b1".into(),
            DevicePermission::View, // View only!
            now,
        );
        let backend: Arc<dyn RemoteBrowserBackend> = Arc::new(InProcessTestBackend::new());
        let admission = AdmissionController::new();

        let claim_msg = ClientMessage::BrowserDriverClaim {
            request_id: "r1".into(),
            subscription_id: "sub-c2".into(),
            browser_id: "b1".into(),
        };
        let resp = session
            .handle_client_message(claim_msg, &backend, &admission, now)
            .await
            .unwrap()
            .unwrap();
        match resp {
            ServerMessage::BrowserError { code, .. } => {
                assert_eq!(code, "BROWSER_FORBIDDEN");
            }
            _ => panic!("Expected BROWSER_FORBIDDEN error for View-only device"),
        }
    }

    #[tokio::test]
    async fn test_ws_tuple_binding_enforcement() {
        let now = Instant::now();
        let mut session = BrowserWsSession::new(
            "c-tuple".into(),
            "d-tuple".into(),
            "b1".into(),
            DevicePermission::Control,
            now,
        );
        let backend: Arc<dyn RemoteBrowserBackend> = Arc::new(InProcessTestBackend::new());
        let admission = AdmissionController::new();

        // 1. Claim before subscribe -> rejected with BROWSER_INVALID_REQUEST
        let claim_unsub = ClientMessage::BrowserDriverClaim {
            request_id: "r-unsub".into(),
            subscription_id: "sub-fake".into(),
            browser_id: "b1".into(),
        };
        let resp = session
            .handle_client_message(claim_unsub, &backend, &admission, now)
            .await
            .unwrap()
            .unwrap();
        match resp {
            ServerMessage::BrowserError { code, message, .. } => {
                assert_eq!(code, "BROWSER_INVALID_REQUEST");
                assert!(message.contains("Subscription binding mismatch or not subscribed"));
            }
            _ => panic!("Expected BROWSER_INVALID_REQUEST"),
        }

        // Subscribe properly
        let sub_msg = ClientMessage::BrowserSubscribe {
            request_id: "r-sub".into(),
            viewer_instance_id: "v1".into(),
            options: BrowserSubscribeOptions {
                format: crate::remote::browser_protocol::BrowserImageFormat::Jpeg,
                quality: None,
                interval_ms: None,
                max_edge: None,
            },
        };
        session
            .handle_client_message(sub_msg, &backend, &admission, now)
            .await
            .unwrap();

        // 2. Claim with mismatched subscription_id -> rejected
        let claim_bad_sub = ClientMessage::BrowserDriverClaim {
            request_id: "r-badsub".into(),
            subscription_id: "wrong-sub".into(),
            browser_id: "b1".into(),
        };
        let resp2 = session
            .handle_client_message(claim_bad_sub, &backend, &admission, now)
            .await
            .unwrap()
            .unwrap();
        match resp2 {
            ServerMessage::BrowserError { code, .. } => assert_eq!(code, "BROWSER_INVALID_REQUEST"),
            _ => panic!("Expected BROWSER_INVALID_REQUEST"),
        }

        // 3. Claim with mismatched browser_id -> rejected
        let claim_bad_b = ClientMessage::BrowserDriverClaim {
            request_id: "r-badb".into(),
            subscription_id: session.subscription_id.clone().unwrap(),
            browser_id: "b2".into(),
        };
        let resp3 = session
            .handle_client_message(claim_bad_b, &backend, &admission, now)
            .await
            .unwrap()
            .unwrap();
        match resp3 {
            ServerMessage::BrowserError { code, .. } => assert_eq!(code, "BROWSER_INVALID_REQUEST"),
            _ => panic!("Expected BROWSER_INVALID_REQUEST"),
        }

        // 4. Command with mismatched browser_id -> rejected
        let cmd_bad_b = ClientMessage::BrowserCommand {
            request_id: "r-cmdbadb".into(),
            request_seq: "1".into(),
            browser_id: "b2".into(),
            lease_epoch: "1".into(),
            browser_instance_id: "bi1".into(),
            desktop_epoch: "1".into(),
            document_generation: "1".into(),
            command: "getState".into(),
            params: None,
        };
        let resp4 = session
            .handle_client_message(cmd_bad_b, &backend, &admission, now)
            .await
            .unwrap()
            .unwrap();
        match resp4 {
            ServerMessage::BrowserError { code, message, .. } => {
                assert_eq!(code, "BROWSER_INVALID_REQUEST");
                assert!(message.contains("Invalid browser or subscription binding"));
            }
            _ => panic!("Expected BROWSER_INVALID_REQUEST"),
        }
    }

    #[tokio::test]
    async fn test_ws_payload_limits_and_sanitization() {
        let now = Instant::now();
        let mut session = BrowserWsSession::new(
            "c-limits".into(),
            "d-limits".into(),
            "b1".into(),
            DevicePermission::Control,
            now,
        );
        let backend: Arc<dyn RemoteBrowserBackend> = Arc::new(InProcessTestBackend::new());
        let admission = AdmissionController::new();

        // Subscribe & claim driver
        let sub_msg = ClientMessage::BrowserSubscribe {
            request_id: "r1".into(),
            viewer_instance_id: "v1".into(),
            options: BrowserSubscribeOptions {
                format: crate::remote::browser_protocol::BrowserImageFormat::Jpeg,
                quality: None,
                interval_ms: None,
                max_edge: None,
            },
        };
        session
            .handle_client_message(sub_msg, &backend, &admission, now)
            .await
            .unwrap();

        let claim_msg = ClientMessage::BrowserDriverClaim {
            request_id: "r2".into(),
            subscription_id: session.subscription_id.clone().unwrap(),
            browser_id: "b1".into(),
        };
        session
            .handle_client_message(claim_msg, &backend, &admission, now)
            .await
            .unwrap();

        let epoch = session.lease_epoch.unwrap().to_string();

        // 1. Oversized command name > 64 KiB
        let oversized_command = "a".repeat(65 * 1024);
        let cmd_oversized = ClientMessage::BrowserCommand {
            request_id: "r-large-cmd".into(),
            request_seq: "1".into(),
            browser_id: "b1".into(),
            lease_epoch: epoch.clone(),
            browser_instance_id: "bi1".into(),
            desktop_epoch: "1".into(),
            document_generation: "1".into(),
            command: oversized_command,
            params: None,
        };
        let resp = session
            .handle_client_message(cmd_oversized, &backend, &admission, now)
            .await
            .unwrap()
            .unwrap();
        match resp {
            ServerMessage::BrowserError { code, message, .. } => {
                assert_eq!(code, "BROWSER_INVALID_REQUEST");
                assert!(message.contains("Command name exceeds limit"));
            }
            _ => panic!("Expected BROWSER_INVALID_REQUEST"),
        }

        // 2. Oversized script in params > 32 KiB
        let oversized_script = "x".repeat(33 * 1024);
        let cmd_oversized_script = ClientMessage::BrowserCommand {
            request_id: "r-large-script".into(),
            request_seq: "2".into(),
            browser_id: "b1".into(),
            lease_epoch: epoch.clone(),
            browser_instance_id: "bi1".into(),
            desktop_epoch: "1".into(),
            document_generation: "1".into(),
            command: "eval".into(),
            params: Some(serde_json::json!({ "script": oversized_script })),
        };
        let resp_script = session
            .handle_client_message(cmd_oversized_script, &backend, &admission, now)
            .await
            .unwrap()
            .unwrap();
        match resp_script {
            ServerMessage::BrowserError { code, message, .. } => {
                assert_eq!(code, "BROWSER_INVALID_REQUEST");
                assert!(message.contains("Script exceeds maximum size of 32 KiB"));
            }
            _ => panic!("Expected BROWSER_INVALID_REQUEST"),
        }

        // 3. Oversized fill text > 16 KiB
        let oversized_fill = "y".repeat(17 * 1024);
        let cmd_oversized_fill = ClientMessage::BrowserCommand {
            request_id: "r-large-fill".into(),
            request_seq: "3".into(),
            browser_id: "b1".into(),
            lease_epoch: epoch,
            browser_instance_id: "bi1".into(),
            desktop_epoch: "1".into(),
            document_generation: "1".into(),
            command: "fill".into(),
            params: Some(serde_json::json!({ "value": oversized_fill })),
        };
        let resp_fill = session
            .handle_client_message(cmd_oversized_fill, &backend, &admission, now)
            .await
            .unwrap()
            .unwrap();
        match resp_fill {
            ServerMessage::BrowserError { code, message, .. } => {
                assert_eq!(code, "BROWSER_INVALID_REQUEST");
                assert!(message.contains("Fill text exceeds maximum size of 16 KiB"));
            }
            _ => panic!("Expected BROWSER_INVALID_REQUEST"),
        }
    }

    struct ErrorBackend {
        err: RemoteBrowserError,
    }
    impl RemoteBrowserBackend for ErrorBackend {
        fn list_sessions<'a>(
            &'a self,
            _scope: &'a crate::remote::browser_backend::DesktopScope,
        ) -> futures_util::future::BoxFuture<
            'a,
            Result<
                Vec<crate::remote::browser_backend::RemoteBrowserSessionSummary>,
                RemoteBrowserError,
            >,
        > {
            Box::pin(async move { Ok(vec![]) })
        }
        fn identify_session<'a>(
            &'a self,
            _scope: &'a crate::remote::browser_backend::DesktopScope,
        ) -> futures_util::future::BoxFuture<
            'a,
            Result<
                Option<crate::remote::browser_backend::RemoteBrowserSessionSummary>,
                RemoteBrowserError,
            >,
        > {
            Box::pin(async move { Ok(None) })
        }
        fn get_state<'a>(
            &'a self,
            _browser_id: &'a str,
            _scope: &'a crate::remote::browser_backend::DesktopScope,
        ) -> futures_util::future::BoxFuture<
            'a,
            Result<crate::remote::browser_backend::BrowserRemoteState, RemoteBrowserError>,
        > {
            Box::pin(async move { Err(self.err.clone()) })
        }
        fn execute_command(
            &self,
            _ctx: BrowserCommandContext,
        ) -> futures_util::future::BoxFuture<'_, Result<BrowserCommandResult, RemoteBrowserError>>
        {
            Box::pin(async move { Err(self.err.clone()) })
        }
        fn capabilities(
            &self,
        ) -> futures_util::future::BoxFuture<'_, crate::remote::browser_backend::BrowserCapabilities>
        {
            Box::pin(async move {
                crate::remote::browser_backend::BrowserCapabilities {
                    browser_available: false,
                    supported_formats: vec![],
                    supported_commands: vec![],
                    max_edge: 2048,
                    max_fps: 8,
                }
            })
        }
    }

    #[tokio::test]
    async fn test_ws_typed_error_preservation() {
        let now = Instant::now();

        for (backend_err, expected_code) in [
            (RemoteBrowserError::WaitTimeout, "BROWSER_TIMEOUT"),
            (
                RemoteBrowserError::Forbidden("Permission denied /Users/alice".into()),
                "BROWSER_FORBIDDEN",
            ),
            (
                RemoteBrowserError::Unavailable("GUI process exited".into()),
                "BROWSER_UNAVAILABLE",
            ),
        ] {
            let admission = AdmissionController::new();
            let mut session = BrowserWsSession::new(
                "c-err".into(),
                "d-err".into(),
                "b1".into(),
                DevicePermission::Control,
                now,
            );
            let backend: Arc<dyn RemoteBrowserBackend> =
                Arc::new(ErrorBackend { err: backend_err });

            let sub_msg = ClientMessage::BrowserSubscribe {
                request_id: "r1".into(),
                viewer_instance_id: "v1".into(),
                options: BrowserSubscribeOptions {
                    format: crate::remote::browser_protocol::BrowserImageFormat::Jpeg,
                    quality: None,
                    interval_ms: None,
                    max_edge: None,
                },
            };
            session
                .handle_client_message(sub_msg, &backend, &admission, now)
                .await
                .unwrap();

            let claim_msg = ClientMessage::BrowserDriverClaim {
                request_id: "r2".into(),
                subscription_id: session.subscription_id.clone().unwrap(),
                browser_id: "b1".into(),
            };
            session
                .handle_client_message(claim_msg, &backend, &admission, now)
                .await
                .unwrap();

            let cmd = ClientMessage::BrowserCommand {
                request_id: "r3".into(),
                request_seq: "1".into(),
                browser_id: "b1".into(),
                lease_epoch: session.lease_epoch.unwrap().to_string(),
                browser_instance_id: "bi1".into(),
                desktop_epoch: "1".into(),
                document_generation: "1".into(),
                command: "wait".into(),
                params: None,
            };
            let resp = session
                .handle_client_message(cmd, &backend, &admission, now)
                .await
                .unwrap()
                .unwrap();
            match resp {
                ServerMessage::BrowserError { code, message, .. } => {
                    assert_eq!(code, expected_code, "Preserve typed error code");
                    // Path redaction verification
                    assert!(!message.contains("/Users/"), "Paths must be redacted");
                }
                _ => panic!("Expected BrowserError with code {}", expected_code),
            }
        }
    }

    #[tokio::test]
    async fn test_r6_6_ws_typed_error_preservation_target_not_found_and_stale_frame() {
        let now = Instant::now();

        let cases = [
            (
                RemoteBrowserError::ExecutionFailed("element not found".into()),
                "BROWSER_TARGET_NOT_FOUND",
            ),
            (
                RemoteBrowserError::ExecutionFailed("no element at coordinates".into()),
                "BROWSER_TARGET_NOT_FOUND",
            ),
            (
                RemoteBrowserError::ExecutionFailed(
                    "BROWSER_TARGET_NOT_FOUND: querySelector null".into(),
                ),
                "BROWSER_TARGET_NOT_FOUND",
            ),
            (
                RemoteBrowserError::InvalidRequest(
                    "target resolution failed: invalid reference".into(),
                ),
                "BROWSER_TARGET_NOT_FOUND",
            ),
            (
                RemoteBrowserError::ExecutionFailed(
                    "BROWSER_STALE_FRAME: viewport revision changed since frame capture".into(),
                ),
                "BROWSER_STALE_FRAME",
            ),
        ];

        for (backend_err, expected_code) in cases {
            let admission = AdmissionController::new();
            let mut session = BrowserWsSession::new(
                "c-err".into(),
                "d-err".into(),
                "b1".into(),
                DevicePermission::Control,
                now,
            );
            let backend: Arc<dyn RemoteBrowserBackend> =
                Arc::new(ErrorBackend { err: backend_err });

            let sub_msg = ClientMessage::BrowserSubscribe {
                request_id: "r1".into(),
                viewer_instance_id: "v1".into(),
                options: BrowserSubscribeOptions {
                    format: crate::remote::browser_protocol::BrowserImageFormat::Jpeg,
                    quality: None,
                    interval_ms: None,
                    max_edge: None,
                },
            };
            session
                .handle_client_message(sub_msg, &backend, &admission, now)
                .await
                .unwrap();

            let claim_msg = ClientMessage::BrowserDriverClaim {
                request_id: "r2".into(),
                subscription_id: session.subscription_id.clone().unwrap(),
                browser_id: "b1".into(),
            };
            session
                .handle_client_message(claim_msg, &backend, &admission, now)
                .await
                .unwrap();

            let cmd = ClientMessage::BrowserCommand {
                request_id: "r3".into(),
                request_seq: "1".into(),
                browser_id: "b1".into(),
                lease_epoch: session.lease_epoch.unwrap().to_string(),
                browser_instance_id: "bi1".into(),
                desktop_epoch: "1".into(),
                document_generation: "1".into(),
                command: "click".into(),
                params: Some(serde_json::json!({ "reference": "target-1" })),
            };
            let resp = session
                .handle_client_message(cmd, &backend, &admission, now)
                .await
                .unwrap()
                .unwrap();
            match resp {
                ServerMessage::BrowserError { code, .. } => {
                    assert_eq!(
                        code, expected_code,
                        "Expected error code {} for error, got {}",
                        expected_code, code
                    );
                }
                _ => panic!("Expected BrowserError with code {}", expected_code),
            }
        }
    }

    #[tokio::test]
    async fn test_r6_11_concurrent_fills_are_serialized_per_target() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

        struct FillTestBackend {
            in_flight: AtomicUsize,
            had_concurrent: AtomicBool,
        }
        impl RemoteBrowserBackend for FillTestBackend {
            fn list_sessions<'a>(
                &'a self,
                _scope: &'a crate::remote::browser_backend::DesktopScope,
            ) -> futures_util::future::BoxFuture<
                'a,
                Result<
                    Vec<crate::remote::browser_backend::RemoteBrowserSessionSummary>,
                    RemoteBrowserError,
                >,
            > {
                Box::pin(async move { Ok(vec![]) })
            }
            fn identify_session<'a>(
                &'a self,
                _scope: &'a crate::remote::browser_backend::DesktopScope,
            ) -> futures_util::future::BoxFuture<
                'a,
                Result<
                    Option<crate::remote::browser_backend::RemoteBrowserSessionSummary>,
                    RemoteBrowserError,
                >,
            > {
                Box::pin(async move { Ok(None) })
            }
            fn get_state<'a>(
                &'a self,
                _browser_id: &'a str,
                _scope: &'a crate::remote::browser_backend::DesktopScope,
            ) -> futures_util::future::BoxFuture<
                'a,
                Result<crate::remote::browser_backend::BrowserRemoteState, RemoteBrowserError>,
            > {
                Box::pin(async move { Err(RemoteBrowserError::NotFound("b1".into())) })
            }
            fn execute_command(
                &self,
                ctx: BrowserCommandContext,
            ) -> futures_util::future::BoxFuture<'_, Result<BrowserCommandResult, RemoteBrowserError>>
            {
                Box::pin(async move {
                    if ctx.command == "fill" {
                        let prev = self.in_flight.fetch_add(1, Ordering::SeqCst);
                        if prev > 0 {
                            self.had_concurrent.store(true, Ordering::SeqCst);
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                        self.in_flight.fetch_sub(1, Ordering::SeqCst);
                    }
                    Ok(BrowserCommandResult {
                        success: true,
                        value: Some(serde_json::json!({ "filled": true })),
                    })
                })
            }
            fn capabilities(
                &self,
            ) -> futures_util::future::BoxFuture<
                '_,
                crate::remote::browser_backend::BrowserCapabilities,
            > {
                Box::pin(async move {
                    crate::remote::browser_backend::BrowserCapabilities {
                        browser_available: true,
                        supported_formats: vec![],
                        supported_commands: vec!["fill".into()],
                        max_edge: 2048,
                        max_fps: 8,
                    }
                })
            }
        }

        let now = Instant::now();
        let admission = AdmissionController::new();
        let mut session = BrowserWsSession::new(
            "c-fill".into(),
            "d-fill".into(),
            "b-fill-1".into(),
            DevicePermission::Control,
            now,
        );
        let backend = Arc::new(FillTestBackend {
            in_flight: AtomicUsize::new(0),
            had_concurrent: AtomicBool::new(false),
        });
        let dyn_backend: Arc<dyn RemoteBrowserBackend> = backend.clone();

        // Subscribe & claim
        let sub_msg = ClientMessage::BrowserSubscribe {
            request_id: "r1".into(),
            viewer_instance_id: "v1".into(),
            options: BrowserSubscribeOptions::default(),
        };
        session
            .handle_client_message(sub_msg, &dyn_backend, &admission, now)
            .await
            .unwrap();

        let claim_msg = ClientMessage::BrowserDriverClaim {
            request_id: "r2".into(),
            subscription_id: session.subscription_id.clone().unwrap(),
            browser_id: "b-fill-1".into(),
        };
        session
            .handle_client_message(claim_msg, &dyn_backend, &admission, now)
            .await
            .unwrap();

        let (out_tx, mut out_rx) = tokio::sync::mpsc::channel(16);

        // Send two fill commands concurrently for the same target
        let cmd1 = ClientMessage::BrowserCommand {
            request_id: "f1".into(),
            request_seq: "1".into(),
            browser_id: "b-fill-1".into(),
            lease_epoch: session.lease_epoch.unwrap().to_string(),
            browser_instance_id: "bi1".into(),
            desktop_epoch: "1".into(),
            document_generation: "1".into(),
            command: "fill".into(),
            params: Some(serde_json::json!({ "selector": "#input", "value": "a" })),
        };
        let cmd2 = ClientMessage::BrowserCommand {
            request_id: "f2".into(),
            request_seq: "2".into(),
            browser_id: "b-fill-1".into(),
            lease_epoch: session.lease_epoch.unwrap().to_string(),
            browser_instance_id: "bi1".into(),
            desktop_epoch: "1".into(),
            document_generation: "1".into(),
            command: "fill".into(),
            params: Some(serde_json::json!({ "selector": "#input", "value": "b" })),
        };

        session
            .dispatch_client_message(cmd1, &dyn_backend, &admission, &out_tx, now)
            .await
            .unwrap();
        session
            .dispatch_client_message(cmd2, &dyn_backend, &admission, &out_tx, now)
            .await
            .unwrap();

        // Wait for both results
        let r1 = out_rx.recv().await.unwrap();
        let r2 = out_rx.recv().await.unwrap();
        assert!(matches!(r1, ServerMessage::BrowserResult { .. }));
        assert!(matches!(r2, ServerMessage::BrowserResult { .. }));

        assert!(
            !backend.had_concurrent.load(Ordering::SeqCst),
            "Fills on the same target must NEVER execute concurrently (must be serialized)"
        );
    }

    #[tokio::test]
    async fn test_r6_11_fill_supersession_at_mutation_boundary_under_serializer() {
        struct ControlledBackend {
            first_started: tokio::sync::Notify,
            first_continue: tokio::sync::Notify,
            executed_commands: parking_lot::Mutex<Vec<String>>,
        }
        impl RemoteBrowserBackend for ControlledBackend {
            fn list_sessions<'a>(
                &'a self,
                _scope: &'a crate::remote::browser_backend::DesktopScope,
            ) -> futures_util::future::BoxFuture<
                'a,
                Result<
                    Vec<crate::remote::browser_backend::RemoteBrowserSessionSummary>,
                    RemoteBrowserError,
                >,
            > {
                Box::pin(async move { Ok(vec![]) })
            }
            fn identify_session<'a>(
                &'a self,
                _scope: &'a crate::remote::browser_backend::DesktopScope,
            ) -> futures_util::future::BoxFuture<
                'a,
                Result<
                    Option<crate::remote::browser_backend::RemoteBrowserSessionSummary>,
                    RemoteBrowserError,
                >,
            > {
                Box::pin(async move { Ok(None) })
            }
            fn get_state<'a>(
                &'a self,
                _browser_id: &'a str,
                _scope: &'a crate::remote::browser_backend::DesktopScope,
            ) -> futures_util::future::BoxFuture<
                'a,
                Result<crate::remote::browser_backend::BrowserRemoteState, RemoteBrowserError>,
            > {
                Box::pin(async move { Err(RemoteBrowserError::NotFound("b1".into())) })
            }
            fn execute_command(
                &self,
                ctx: BrowserCommandContext,
            ) -> futures_util::future::BoxFuture<'_, Result<BrowserCommandResult, RemoteBrowserError>>
            {
                Box::pin(async move {
                    let val = ctx
                        .params
                        .as_ref()
                        .and_then(|p| p.get("value"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    if val == "first" {
                        self.executed_commands.lock().push(val.clone());
                        self.first_started.notify_one();
                        self.first_continue.notified().await;
                    } else {
                        self.executed_commands.lock().push(val.clone());
                    }
                    Ok(BrowserCommandResult {
                        success: true,
                        value: Some(serde_json::json!({ "filled": true })),
                    })
                })
            }
            fn capabilities(
                &self,
            ) -> futures_util::future::BoxFuture<
                '_,
                crate::remote::browser_backend::BrowserCapabilities,
            > {
                Box::pin(async move {
                    crate::remote::browser_backend::BrowserCapabilities {
                        browser_available: true,
                        supported_formats: vec![],
                        supported_commands: vec!["fill".into()],
                        max_edge: 2048,
                        max_fps: 8,
                    }
                })
            }
        }

        let now = Instant::now();
        let admission = AdmissionController::new();
        let mut session = BrowserWsSession::new(
            "c-super".into(),
            "d-super".into(),
            "b-super-1".into(),
            DevicePermission::Control,
            now,
        );
        let backend = Arc::new(ControlledBackend {
            first_started: tokio::sync::Notify::new(),
            first_continue: tokio::sync::Notify::new(),
            executed_commands: parking_lot::Mutex::new(Vec::new()),
        });
        let dyn_backend: Arc<dyn RemoteBrowserBackend> = backend.clone();

        // Subscribe & claim
        let sub_msg = ClientMessage::BrowserSubscribe {
            request_id: "r1".into(),
            viewer_instance_id: "v1".into(),
            options: BrowserSubscribeOptions::default(),
        };
        session
            .handle_client_message(sub_msg, &dyn_backend, &admission, now)
            .await
            .unwrap();

        let claim_msg = ClientMessage::BrowserDriverClaim {
            request_id: "r2".into(),
            subscription_id: session.subscription_id.clone().unwrap(),
            browser_id: "b-super-1".into(),
        };
        session
            .handle_client_message(claim_msg, &dyn_backend, &admission, now)
            .await
            .unwrap();

        let (out_tx, mut out_rx) = tokio::sync::mpsc::channel(16);

        // 1. Send first fill with rev 1 ("first")
        let cmd1 = ClientMessage::BrowserCommand {
            request_id: "f1".into(),
            request_seq: "1".into(),
            browser_id: "b-super-1".into(),
            lease_epoch: session.lease_epoch.unwrap().to_string(),
            browser_instance_id: "bi1".into(),
            desktop_epoch: "1".into(),
            document_generation: "1".into(),
            command: "fill".into(),
            params: Some(
                serde_json::json!({ "selector": "#target", "value": "first", "revision": 1 }),
            ),
        };
        session
            .dispatch_client_message(cmd1, &dyn_backend, &admission, &out_tx, now)
            .await
            .unwrap();

        // Wait until first fill begins executing
        backend.first_started.notified().await;

        // 2. Queue fill with rev 1 ("stale") while first is executing
        // Then queue fill with rev 2 ("newer")
        // "newer" advances tracker to 2.
        let cmd_stale = ClientMessage::BrowserCommand {
            request_id: "f-stale".into(),
            request_seq: "2".into(),
            browser_id: "b-super-1".into(),
            lease_epoch: session.lease_epoch.unwrap().to_string(),
            browser_instance_id: "bi1".into(),
            desktop_epoch: "1".into(),
            document_generation: "1".into(),
            command: "fill".into(),
            params: Some(
                serde_json::json!({ "selector": "#target", "value": "stale", "revision": 1 }),
            ),
        };
        let cmd_newer = ClientMessage::BrowserCommand {
            request_id: "f-newer".into(),
            request_seq: "3".into(),
            browser_id: "b-super-1".into(),
            lease_epoch: session.lease_epoch.unwrap().to_string(),
            browser_instance_id: "bi1".into(),
            desktop_epoch: "1".into(),
            document_generation: "1".into(),
            command: "fill".into(),
            params: Some(
                serde_json::json!({ "selector": "#target", "value": "newer", "revision": 2 }),
            ),
        };

        session
            .dispatch_client_message(cmd_stale, &dyn_backend, &admission, &out_tx, now)
            .await
            .unwrap();
        session
            .dispatch_client_message(cmd_newer, &dyn_backend, &admission, &out_tx, now)
            .await
            .unwrap();

        // Release first command to finish
        backend.first_continue.notify_one();

        // Collect all 3 responses
        let mut results = Vec::new();
        for _ in 0..3 {
            results.push(out_rx.recv().await.unwrap());
        }

        // Verify "stale" was superseded at the mutation boundary under the lock:
        let stale_resp = results
            .iter()
            .find(|m| match m {
                ServerMessage::BrowserResult { request_id, .. } => request_id == "f-stale",
                _ => false,
            })
            .unwrap();
        match stale_resp {
            ServerMessage::BrowserResult {
                result: Some(val), ..
            } => {
                assert_eq!(val.get("superseded"), Some(&serde_json::json!(true)));
            }
            other => panic!("Expected superseded BrowserResult, got {:?}", other),
        }

        // And verify "stale" never touched backend.execute_command!
        let executed = backend.executed_commands.lock().clone();
        assert_eq!(executed, vec!["first".to_string(), "newer".to_string()]);
    }

    #[tokio::test]
    async fn test_r6_4_teardown_cancels_outstanding_operations_before_backend_cleanup() {
        use std::sync::atomic::{AtomicBool, Ordering};

        struct HangingBackend {
            in_cleanup: AtomicBool,
        }
        impl RemoteBrowserBackend for HangingBackend {
            fn list_sessions<'a>(
                &'a self,
                _scope: &'a crate::remote::browser_backend::DesktopScope,
            ) -> futures_util::future::BoxFuture<
                'a,
                Result<
                    Vec<crate::remote::browser_backend::RemoteBrowserSessionSummary>,
                    RemoteBrowserError,
                >,
            > {
                Box::pin(async move { Ok(vec![]) })
            }
            fn identify_session<'a>(
                &'a self,
                _scope: &'a crate::remote::browser_backend::DesktopScope,
            ) -> futures_util::future::BoxFuture<
                'a,
                Result<
                    Option<crate::remote::browser_backend::RemoteBrowserSessionSummary>,
                    RemoteBrowserError,
                >,
            > {
                Box::pin(async move { Ok(None) })
            }
            fn get_state<'a>(
                &'a self,
                _browser_id: &'a str,
                _scope: &'a crate::remote::browser_backend::DesktopScope,
            ) -> futures_util::future::BoxFuture<
                'a,
                Result<crate::remote::browser_backend::BrowserRemoteState, RemoteBrowserError>,
            > {
                Box::pin(async move { Err(RemoteBrowserError::NotFound("b1".into())) })
            }
            fn execute_command(
                &self,
                _ctx: BrowserCommandContext,
            ) -> futures_util::future::BoxFuture<'_, Result<BrowserCommandResult, RemoteBrowserError>>
            {
                Box::pin(async move {
                    // Hang forever until cancelled
                    std::future::pending::<()>().await;
                    Ok(BrowserCommandResult {
                        success: true,
                        value: None,
                    })
                })
            }
            fn unsubscribe_viewer<'a>(
                &'a self,
                _browser_id: &'a str,
                _subscription_id: &'a str,
            ) -> futures_util::future::BoxFuture<'a, Result<(), RemoteBrowserError>> {
                Box::pin(async move {
                    self.in_cleanup.store(true, Ordering::SeqCst);
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    Ok(())
                })
            }
            fn capabilities(
                &self,
            ) -> futures_util::future::BoxFuture<
                '_,
                crate::remote::browser_backend::BrowserCapabilities,
            > {
                Box::pin(async move {
                    crate::remote::browser_backend::BrowserCapabilities {
                        browser_available: true,
                        supported_formats: vec![],
                        supported_commands: vec!["click".into()],
                        max_edge: 2048,
                        max_fps: 8,
                    }
                })
            }
        }

        let now = Instant::now();
        let admission = AdmissionController::new();
        let mut session = BrowserWsSession::new(
            "c-r64".into(),
            "d-r64".into(),
            "b-r64-1".into(),
            DevicePermission::Control,
            now,
        );
        let backend = Arc::new(HangingBackend {
            in_cleanup: AtomicBool::new(false),
        });
        let dyn_backend: Arc<dyn RemoteBrowserBackend> = backend.clone();

        // Subscribe & claim
        let sub_msg = ClientMessage::BrowserSubscribe {
            request_id: "r1".into(),
            viewer_instance_id: "v1".into(),
            options: BrowserSubscribeOptions::default(),
        };
        session
            .handle_client_message(sub_msg, &dyn_backend, &admission, now)
            .await
            .unwrap();

        let claim_msg = ClientMessage::BrowserDriverClaim {
            request_id: "r2".into(),
            subscription_id: session.subscription_id.clone().unwrap(),
            browser_id: "b-r64-1".into(),
        };
        session
            .handle_client_message(claim_msg, &dyn_backend, &admission, now)
            .await
            .unwrap();

        let (out_tx, _out_rx) = tokio::sync::mpsc::channel(16);

        // Spawn a command that will hang
        let cmd = ClientMessage::BrowserCommand {
            request_id: "hang1".into(),
            request_seq: "1".into(),
            browser_id: "b-r64-1".into(),
            lease_epoch: session.lease_epoch.unwrap().to_string(),
            browser_instance_id: "bi1".into(),
            desktop_epoch: "1".into(),
            document_generation: "1".into(),
            command: "click".into(),
            params: Some(serde_json::json!({ "reference": "target-1" })),
        };
        session
            .dispatch_client_message(cmd, &dyn_backend, &admission, &out_tx, now)
            .await
            .unwrap();

        assert_eq!(
            session.tracked_tasks.lock().len(),
            1,
            "Command task must be tracked"
        );
        assert!(!session.cancel_token.is_cancelled());

        // Perform teardown
        session
            .teardown_with_backend(&admission, &dyn_backend)
            .await;

        // Verify cancelled and tasks aborted
        assert!(
            session.cancel_token.is_cancelled(),
            "Cancel token must be cancelled on teardown"
        );
        assert_eq!(
            session.tracked_tasks.lock().len(),
            0,
            "Tracked tasks must be aborted on teardown"
        );
    }

    #[tokio::test]
    async fn test_r6_4_snapshot_revalidates_scope_after_await() {
        use std::sync::atomic::{AtomicBool, Ordering};

        struct SnapshotBackend;
        impl RemoteBrowserBackend for SnapshotBackend {
            fn list_sessions<'a>(
                &'a self,
                _scope: &'a crate::remote::browser_backend::DesktopScope,
            ) -> futures_util::future::BoxFuture<
                'a,
                Result<
                    Vec<crate::remote::browser_backend::RemoteBrowserSessionSummary>,
                    RemoteBrowserError,
                >,
            > {
                Box::pin(async move { Ok(vec![]) })
            }
            fn identify_session<'a>(
                &'a self,
                _scope: &'a crate::remote::browser_backend::DesktopScope,
            ) -> futures_util::future::BoxFuture<
                'a,
                Result<
                    Option<crate::remote::browser_backend::RemoteBrowserSessionSummary>,
                    RemoteBrowserError,
                >,
            > {
                Box::pin(async move { Ok(None) })
            }
            fn get_state<'a>(
                &'a self,
                _browser_id: &'a str,
                _scope: &'a crate::remote::browser_backend::DesktopScope,
            ) -> futures_util::future::BoxFuture<
                'a,
                Result<crate::remote::browser_backend::BrowserRemoteState, RemoteBrowserError>,
            > {
                Box::pin(async move { Err(RemoteBrowserError::NotFound("b1".into())) })
            }
            fn execute_command(
                &self,
                ctx: BrowserCommandContext,
            ) -> futures_util::future::BoxFuture<'_, Result<BrowserCommandResult, RemoteBrowserError>>
            {
                Box::pin(async move {
                    if ctx.command == "snapshot" {
                        Ok(BrowserCommandResult {
                            success: true,
                            value: Some(serde_json::json!({
                                "snapshotId": "snap-1",
                                "mapRevision": 1,
                                "root": { "ref": "root-1" },
                                "elements": [{ "ref": "elem-1", "selector": "#btn" }],
                            })),
                        })
                    } else {
                        Ok(BrowserCommandResult {
                            success: true,
                            value: None,
                        })
                    }
                })
            }
            fn capabilities(
                &self,
            ) -> futures_util::future::BoxFuture<
                '_,
                crate::remote::browser_backend::BrowserCapabilities,
            > {
                Box::pin(async move {
                    crate::remote::browser_backend::BrowserCapabilities {
                        browser_available: true,
                        supported_formats: vec![],
                        supported_commands: vec!["snapshot".into()],
                        max_edge: 2048,
                        max_fps: 8,
                    }
                })
            }
        }

        let now = Instant::now();
        let admission = AdmissionController::new();
        let scope_valid = Arc::new(AtomicBool::new(true));
        let scope_valid_clone = Arc::clone(&scope_valid);

        let mut session = BrowserWsSession::new(
            "c-snap-scope".into(),
            "d-snap-scope".into(),
            "b-snap-1".into(),
            DevicePermission::Control,
            now,
        )
        .with_scope_validator(Arc::new(move || scope_valid_clone.load(Ordering::SeqCst)));

        let backend: Arc<dyn RemoteBrowserBackend> = Arc::new(SnapshotBackend);

        // Subscribe
        let sub_msg = ClientMessage::BrowserSubscribe {
            request_id: "r1".into(),
            viewer_instance_id: "v1".into(),
            options: BrowserSubscribeOptions::default(),
        };
        session
            .handle_client_message(sub_msg, &backend, &admission, now)
            .await
            .unwrap();

        let (out_tx, mut out_rx) = tokio::sync::mpsc::channel(16);

        // Invalidate scope now
        scope_valid.store(false, Ordering::SeqCst);

        // Attempt snapshot
        let snap_msg = ClientMessage::BrowserSnapshot {
            request_id: "snap-req-1".into(),
            browser_id: "b-snap-1".into(),
        };
        session
            .dispatch_client_message(snap_msg, &backend, &admission, &out_tx, now)
            .await
            .unwrap();

        // Must NOT publish snapshot to out_tx because scope was invalid
        assert!(
            out_rx.try_recv().is_err(),
            "Snapshot must not be published when scope is revoked"
        );
        assert!(
            session.cancel_token.is_cancelled(),
            "Session cancel token must be cancelled"
        );
    }

    struct SlowBackend {
        cmd_started_tx: tokio::sync::mpsc::Sender<()>,
        cmd_continue_rx: tokio::sync::Mutex<tokio::sync::mpsc::Receiver<()>>,
    }
    impl RemoteBrowserBackend for SlowBackend {
        fn list_sessions<'a>(
            &'a self,
            _scope: &'a crate::remote::browser_backend::DesktopScope,
        ) -> futures_util::future::BoxFuture<
            'a,
            Result<
                Vec<crate::remote::browser_backend::RemoteBrowserSessionSummary>,
                RemoteBrowserError,
            >,
        > {
            Box::pin(async move { Ok(vec![]) })
        }
        fn identify_session<'a>(
            &'a self,
            _scope: &'a crate::remote::browser_backend::DesktopScope,
        ) -> futures_util::future::BoxFuture<
            'a,
            Result<
                Option<crate::remote::browser_backend::RemoteBrowserSessionSummary>,
                RemoteBrowserError,
            >,
        > {
            Box::pin(async move { Ok(None) })
        }
        fn get_state<'a>(
            &'a self,
            _browser_id: &'a str,
            _scope: &'a crate::remote::browser_backend::DesktopScope,
        ) -> futures_util::future::BoxFuture<
            'a,
            Result<crate::remote::browser_backend::BrowserRemoteState, RemoteBrowserError>,
        > {
            Box::pin(async move { Err(RemoteBrowserError::NotFound("b1".into())) })
        }
        fn execute_command(
            &self,
            _ctx: BrowserCommandContext,
        ) -> futures_util::future::BoxFuture<'_, Result<BrowserCommandResult, RemoteBrowserError>>
        {
            Box::pin(async move {
                let _ = self.cmd_started_tx.send(()).await;
                let mut rx = self.cmd_continue_rx.lock().await;
                let _ = rx.recv().await;
                Ok(BrowserCommandResult {
                    success: true,
                    value: Some(serde_json::json!({ "done": true })),
                })
            })
        }
        fn capabilities(
            &self,
        ) -> futures_util::future::BoxFuture<'_, crate::remote::browser_backend::BrowserCapabilities>
        {
            Box::pin(async move {
                crate::remote::browser_backend::BrowserCapabilities {
                    browser_available: true,
                    supported_formats: vec![],
                    supported_commands: vec![],
                    max_edge: 2048,
                    max_fps: 8,
                }
            })
        }
    }

    #[tokio::test]
    async fn test_ws_spawned_command_tasks_non_blocking_control() {
        let (started_tx, mut started_rx) = tokio::sync::mpsc::channel(1);
        let (continue_tx, continue_rx) = tokio::sync::mpsc::channel(1);
        let backend: Arc<dyn RemoteBrowserBackend> = Arc::new(SlowBackend {
            cmd_started_tx: started_tx,
            cmd_continue_rx: tokio::sync::Mutex::new(continue_rx),
        });

        let admission = AdmissionController::new();
        let now = Instant::now();
        let mut session = BrowserWsSession::new(
            "c-nb".into(),
            "d-nb".into(),
            "b1".into(),
            DevicePermission::Control,
            now,
        );

        let (out_tx, mut out_rx) = tokio::sync::mpsc::channel(32);

        // 1. Subscribe
        let sub = ClientMessage::BrowserSubscribe {
            request_id: "r1".into(),
            viewer_instance_id: "v1".into(),
            options: BrowserSubscribeOptions {
                format: crate::remote::browser_protocol::BrowserImageFormat::Jpeg,
                quality: None,
                interval_ms: None,
                max_edge: None,
            },
        };
        session
            .dispatch_client_message(sub, &backend, &admission, &out_tx, now)
            .await
            .unwrap();
        let sub_resp = out_rx.recv().await.unwrap();
        assert!(matches!(sub_resp, ServerMessage::BrowserSubscribed { .. }));

        // 2. Driver claim
        let claim = ClientMessage::BrowserDriverClaim {
            request_id: "r2".into(),
            subscription_id: session.subscription_id.clone().unwrap(),
            browser_id: "b1".into(),
        };
        session
            .dispatch_client_message(claim, &backend, &admission, &out_tx, now)
            .await
            .unwrap();
        let claim_resp = out_rx.recv().await.unwrap();
        assert!(matches!(
            claim_resp,
            ServerMessage::BrowserDriverClaimed { .. }
        ));

        // 3. Dispatch long-running command (eval or wait)
        let cmd = ClientMessage::BrowserCommand {
            request_id: "r-long".into(),
            request_seq: "1".into(),
            browser_id: "b1".into(),
            lease_epoch: session.lease_epoch.unwrap().to_string(),
            browser_instance_id: "bi1".into(),
            desktop_epoch: "1".into(),
            document_generation: "1".into(),
            command: "wait".into(),
            params: None,
        };
        session
            .dispatch_client_message(cmd, &backend, &admission, &out_tx, now)
            .await
            .unwrap();

        // Wait until command has actually started executing in background task
        started_rx.recv().await.unwrap();

        // 4. While command is still running in background, dispatch Heartbeat
        let hb = ClientMessage::BrowserHeartbeat {
            request_id: Some("hb-nb".into()),
            lease_epoch: session.lease_epoch.map(|e| e.to_string()),
            subscription_id: session.subscription_id.clone(),
        };
        session
            .dispatch_client_message(hb, &backend, &admission, &out_tx, now)
            .await
            .unwrap();

        // Heartbeat response is received IMMEDIATELY without waiting for command!
        let hb_resp = out_rx.recv().await.unwrap();
        assert!(matches!(hb_resp, ServerMessage::BrowserPong { .. }));

        // 5. While command is still running in background, dispatch DriverRelease
        let rel = ClientMessage::BrowserDriverRelease {
            request_id: "rel-nb".into(),
            lease_epoch: session.lease_epoch.unwrap().to_string(),
        };
        session
            .dispatch_client_message(rel, &backend, &admission, &out_tx, now)
            .await
            .unwrap();

        // Release response is received IMMEDIATELY!
        let rel_resp = out_rx.recv().await.unwrap();
        assert!(matches!(
            rel_resp,
            ServerMessage::BrowserDriverReleased { .. }
        ));
        assert!(!session.is_driver);

        // 6. Now let the background command finish
        continue_tx.send(()).await.unwrap();

        // The command result arrives on out_rx
        let cmd_resp = out_rx.recv().await.unwrap();
        assert!(matches!(cmd_resp, ServerMessage::BrowserResult { .. }));
    }

    #[tokio::test]
    async fn test_ws_raw_wire_pre_parse_limit() {
        let now = Instant::now();
        let mut session = BrowserWsSession::new(
            "c-raw".into(),
            "d-raw".into(),
            "b1".into(),
            DevicePermission::Control,
            now,
        );
        let backend: Arc<dyn RemoteBrowserBackend> = Arc::new(InProcessTestBackend::new());
        let admission = AdmissionController::new();

        // Raw payload over 64 KiB
        let oversized = "X".repeat(65 * 1024);
        let resp = session
            .handle_raw_text(&oversized, &backend, &admission, now)
            .await
            .unwrap()
            .unwrap();
        match resp {
            ServerMessage::BrowserError { code, message, .. } => {
                assert_eq!(code, "BROWSER_INVALID_REQUEST");
                assert!(message.contains("64 KiB"));
            }
            _ => panic!("Expected BROWSER_INVALID_REQUEST for oversized raw text"),
        }
    }

    struct LeakingBackend;
    impl RemoteBrowserBackend for LeakingBackend {
        fn list_sessions<'a>(
            &'a self,
            _scope: &'a crate::remote::browser_backend::DesktopScope,
        ) -> futures_util::future::BoxFuture<
            'a,
            Result<
                Vec<crate::remote::browser_backend::RemoteBrowserSessionSummary>,
                RemoteBrowserError,
            >,
        > {
            Box::pin(async move { Ok(vec![]) })
        }
        fn identify_session<'a>(
            &'a self,
            _scope: &'a crate::remote::browser_backend::DesktopScope,
        ) -> futures_util::future::BoxFuture<
            'a,
            Result<
                Option<crate::remote::browser_backend::RemoteBrowserSessionSummary>,
                RemoteBrowserError,
            >,
        > {
            Box::pin(async move { Ok(None) })
        }
        fn get_state<'a>(
            &'a self,
            _browser_id: &'a str,
            _scope: &'a crate::remote::browser_backend::DesktopScope,
        ) -> futures_util::future::BoxFuture<
            'a,
            Result<crate::remote::browser_backend::BrowserRemoteState, RemoteBrowserError>,
        > {
            Box::pin(async move { Err(RemoteBrowserError::NotFound("b1".into())) })
        }
        fn execute_command(
            &self,
            _ctx: BrowserCommandContext,
        ) -> futures_util::future::BoxFuture<'_, Result<BrowserCommandResult, RemoteBrowserError>>
        {
            Box::pin(async move {
                Ok(BrowserCommandResult {
                    success: true,
                    value: Some(serde_json::json!({
                        "userPath": "/Users/alice/private/secret.txt",
                        "winPath": "C:\\Windows\\System32\\cmd.exe",
                        "uncPath": "\\\\nas\\share\\data.csv",
                        "safe": "normal value"
                    })),
                })
            })
        }
        fn capabilities(
            &self,
        ) -> futures_util::future::BoxFuture<'_, crate::remote::browser_backend::BrowserCapabilities>
        {
            Box::pin(async move {
                crate::remote::browser_backend::BrowserCapabilities {
                    browser_available: true,
                    supported_formats: vec![],
                    supported_commands: vec![],
                    max_edge: 2048,
                    max_fps: 8,
                }
            })
        }
    }

    #[tokio::test]
    async fn test_ws_success_response_path_sanitization() {
        let now = Instant::now();
        let mut session = BrowserWsSession::new(
            "c-leak".into(),
            "d-leak".into(),
            "b1".into(),
            DevicePermission::Control,
            now,
        );
        let backend: Arc<dyn RemoteBrowserBackend> = Arc::new(LeakingBackend);
        let admission = AdmissionController::new();

        let sub = ClientMessage::BrowserSubscribe {
            request_id: "r1".into(),
            viewer_instance_id: "v1".into(),
            options: BrowserSubscribeOptions {
                format: crate::remote::browser_protocol::BrowserImageFormat::Jpeg,
                quality: None,
                interval_ms: None,
                max_edge: None,
            },
        };
        session
            .handle_client_message(sub, &backend, &admission, now)
            .await
            .unwrap();

        let claim = ClientMessage::BrowserDriverClaim {
            request_id: "r2".into(),
            subscription_id: session.subscription_id.clone().unwrap(),
            browser_id: "b1".into(),
        };
        session
            .handle_client_message(claim, &backend, &admission, now)
            .await
            .unwrap();

        let cmd = ClientMessage::BrowserCommand {
            request_id: "r3".into(),
            request_seq: "1".into(),
            browser_id: "b1".into(),
            lease_epoch: session.lease_epoch.unwrap().to_string(),
            browser_instance_id: "bi1".into(),
            desktop_epoch: "1".into(),
            document_generation: "1".into(),
            command: "getState".into(),
            params: None,
        };
        let (out_tx, mut out_rx) = tokio::sync::mpsc::channel(16);
        session
            .dispatch_client_message(cmd, &backend, &admission, &out_tx, now)
            .await
            .unwrap();
        let res = out_rx.recv().await.unwrap();

        match res {
            ServerMessage::BrowserResult { result, .. } => {
                let json_str = serde_json::to_string(&result.unwrap()).unwrap();
                assert!(!json_str.contains("/Users/"), "Must redact /Users/");
                assert!(!json_str.contains("C:\\"), "Must redact C:\\");
                assert!(!json_str.contains("\\\\nas"), "Must redact UNC path");
                assert!(json_str.contains("[redacted-path]"));
                assert!(json_str.contains("normal value"));
            }
            _ => panic!("Expected BrowserResult"),
        }
    }

    #[tokio::test]
    async fn test_ws_execution_time_lease_revalidation_aborts_forbidden() {
        let now = Instant::now();
        let mut session = BrowserWsSession::new(
            "c-reval".into(),
            "d-reval".into(),
            "b1".into(),
            DevicePermission::Control,
            now,
        );
        let backend: Arc<dyn RemoteBrowserBackend> = Arc::new(InProcessTestBackend::new());
        let admission = AdmissionController::new();

        // Subscribe & claim
        let sub = ClientMessage::BrowserSubscribe {
            request_id: "r1".into(),
            viewer_instance_id: "v1".into(),
            options: BrowserSubscribeOptions {
                format: crate::remote::browser_protocol::BrowserImageFormat::Jpeg,
                quality: None,
                interval_ms: None,
                max_edge: None,
            },
        };
        session
            .handle_client_message(sub, &backend, &admission, now)
            .await
            .unwrap();

        let claim = ClientMessage::BrowserDriverClaim {
            request_id: "r2".into(),
            subscription_id: session.subscription_id.clone().unwrap(),
            browser_id: "b1".into(),
        };
        session
            .handle_client_message(claim, &backend, &admission, now)
            .await
            .unwrap();
        let epoch = session.lease_epoch.unwrap();

        // Hold the eval semaphore so spawned eval command waits
        let held_permit = session
            .eval_semaphore
            .clone()
            .acquire_owned()
            .await
            .unwrap();

        let (out_tx, mut out_rx) = tokio::sync::mpsc::channel(16);
        let cmd = ClientMessage::BrowserCommand {
            request_id: "r-eval-wait".into(),
            request_seq: "1".into(),
            browser_id: "b1".into(),
            lease_epoch: epoch.to_string(),
            browser_instance_id: "bi1".into(),
            desktop_epoch: "1".into(),
            document_generation: "1".into(),
            command: "eval".into(),
            params: Some(serde_json::json!({ "script": "1+1" })),
        };
        session
            .dispatch_client_message(cmd, &backend, &admission, &out_tx, now)
            .await
            .unwrap();

        // Now revoke driver lease via desktop reclaim while task is waiting for permit!
        admission.broker.reclaim_desktop();

        // Release the held permit, allowing the task to acquire it
        drop(held_permit);

        // Task acquires permit, re-validates lease, notices it was revoked, and aborts with BROWSER_FORBIDDEN!
        let reply = out_rx.recv().await.unwrap();
        match reply {
            ServerMessage::BrowserError { code, message, .. } => {
                assert_eq!(code, "BROWSER_FORBIDDEN");
                assert!(message.contains("expired or revoked"));
            }
            _ => panic!("Expected BROWSER_FORBIDDEN error on lease expiration"),
        }
    }

    #[tokio::test]
    async fn test_ws_heartbeat_and_release_5tuple_consistency() {
        let now = Instant::now();
        let mut session = BrowserWsSession::new(
            "c-5t".into(),
            "d-5t".into(),
            "b1".into(),
            DevicePermission::Control,
            now,
        );
        let backend: Arc<dyn RemoteBrowserBackend> = Arc::new(InProcessTestBackend::new());
        let admission = AdmissionController::new();

        // Subscribe & claim
        let sub = ClientMessage::BrowserSubscribe {
            request_id: "r1".into(),
            viewer_instance_id: "v1".into(),
            options: BrowserSubscribeOptions {
                format: crate::remote::browser_protocol::BrowserImageFormat::Jpeg,
                quality: None,
                interval_ms: None,
                max_edge: None,
            },
        };
        session
            .handle_client_message(sub, &backend, &admission, now)
            .await
            .unwrap();

        let claim = ClientMessage::BrowserDriverClaim {
            request_id: "r2".into(),
            subscription_id: session.subscription_id.clone().unwrap(),
            browser_id: "b1".into(),
        };
        session
            .handle_client_message(claim, &backend, &admission, now)
            .await
            .unwrap();
        let epoch = session.lease_epoch.unwrap();

        // 1. Heartbeat with wrong subscription_id -> BROWSER_INVALID_REQUEST
        let bad_sub_hb = ClientMessage::BrowserHeartbeat {
            request_id: Some("hb-bad-sub".into()),
            lease_epoch: Some(epoch.to_string()),
            subscription_id: Some("sub-wrong".into()),
        };
        let bad_hb_resp = session
            .handle_client_message(bad_sub_hb, &backend, &admission, now)
            .await
            .unwrap()
            .unwrap();
        assert!(
            matches!(bad_hb_resp, ServerMessage::BrowserError { ref code, .. } if code == "BROWSER_INVALID_REQUEST")
        );

        // 2. Heartbeat with wrong lease_epoch -> BROWSER_INVALID_REQUEST
        let bad_epoch_hb = ClientMessage::BrowserHeartbeat {
            request_id: Some("hb-bad-epoch".into()),
            lease_epoch: Some("99999".into()),
            subscription_id: session.subscription_id.clone(),
        };
        let bad_epoch_resp = session
            .handle_client_message(bad_epoch_hb, &backend, &admission, now)
            .await
            .unwrap()
            .unwrap();
        assert!(
            matches!(bad_epoch_resp, ServerMessage::BrowserError { ref code, .. } if code == "BROWSER_INVALID_REQUEST")
        );

        // 3. Heartbeat with correct 5-tuple -> BrowserPong
        let good_hb = ClientMessage::BrowserHeartbeat {
            request_id: Some("hb-good".into()),
            lease_epoch: Some(epoch.to_string()),
            subscription_id: session.subscription_id.clone(),
        };
        let good_hb_resp = session
            .handle_client_message(good_hb, &backend, &admission, now)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(good_hb_resp, ServerMessage::BrowserPong { .. }));

        // 4. Release with wrong lease_epoch -> BROWSER_INVALID_REQUEST
        let bad_rel = ClientMessage::BrowserDriverRelease {
            request_id: "rel-bad".into(),
            lease_epoch: "99999".into(),
        };
        let bad_rel_resp = session
            .handle_client_message(bad_rel, &backend, &admission, now)
            .await
            .unwrap()
            .unwrap();
        assert!(
            matches!(bad_rel_resp, ServerMessage::BrowserError { ref code, .. } if code == "BROWSER_INVALID_REQUEST")
        );

        // 5. Release with correct 5-tuple -> BrowserDriverReleased
        let good_rel = ClientMessage::BrowserDriverRelease {
            request_id: "rel-good".into(),
            lease_epoch: epoch.to_string(),
        };
        let good_rel_resp = session
            .handle_client_message(good_rel, &backend, &admission, now)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            good_rel_resp,
            ServerMessage::BrowserDriverReleased { .. }
        ));
        assert!(!session.is_driver);
    }

    #[test]
    fn test_ws_frame_enqueuing_and_ack_flow_control() {
        let now = Instant::now();
        let mut session = BrowserWsSession::new(
            "c-flow".into(),
            "d-flow".into(),
            "b1".into(),
            DevicePermission::Control,
            now,
        );

        // Before subscribing: frames are dropped
        assert!(session
            .enqueue_frame(vec![0x62, 1, 1, 1, 1, 0, 0, 0], now)
            .is_none());

        // Setup queue (as if subscribed)
        session.queue = Some(SubscriberQueue::new("sub-flow".into(), 1));

        // Frame 1: admitted immediately
        let f1 = vec![0x62, 1, 1, 1, 1, 0, 0, 0, 10, 20];
        let res1 = session.enqueue_frame(f1.clone(), now);
        assert_eq!(res1, Some(f1));

        // Frame 2: unacked frame exists -> backpressured into pending slot
        let f2 = vec![0x62, 1, 1, 1, 2, 0, 0, 0, 30, 40];
        assert!(session.enqueue_frame(f2.clone(), now).is_none());

        // Frame 3: overwrites pending slot (latest-only!)
        let f3 = vec![0x62, 1, 1, 1, 3, 0, 0, 0, 50, 60];
        assert!(session.enqueue_frame(f3.clone(), now).is_none());

        // ACK arrives for frame 1: frame 3 is promoted and stored in pending_promoted_frame
        let ack = ClientMessage::BrowserFrameAck {
            stream_id: 1,
            seq: 1,
        };
        let admission = AdmissionController::new();
        let backend: Arc<dyn RemoteBrowserBackend> = Arc::new(InProcessTestBackend::new());
        let (out_tx, _) = tokio::sync::mpsc::channel(16);
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        rt.block_on(session.dispatch_client_message(ack, &backend, &admission, &out_tx, now))
            .unwrap();

        assert_eq!(session.pending_promoted_frame, Some(f3));
    }

    #[tokio::test]
    async fn test_ws_r10_pause_and_resume_protocol() {
        let now = Instant::now();
        let mut session = BrowserWsSession::new(
            "c-pause".into(),
            "d-pause".into(),
            "b1".into(),
            DevicePermission::Control,
            now,
        );
        let admission = AdmissionController::new();
        let backend: Arc<dyn RemoteBrowserBackend> = Arc::new(InProcessTestBackend::new());
        let (out_tx, mut out_rx) = tokio::sync::mpsc::channel(16);

        // 1. Subscribe to enter Streaming state
        let sub = ClientMessage::BrowserSubscribe {
            request_id: "r-sub".into(),
            viewer_instance_id: "v1".into(),
            options: Default::default(),
        };
        session
            .dispatch_client_message(sub, &backend, &admission, &out_tx, now)
            .await
            .unwrap();
        let _ = out_rx.recv().await.unwrap(); // BrowserSubscribed
        assert_eq!(session.state, WsConnectionState::Streaming);

        // Frame 1 admitted when streaming
        let f1 = vec![0x62, 1, 1, 1, 1, 0, 0, 0, 10, 20];
        assert_eq!(session.enqueue_frame(f1.clone(), now), Some(f1));

        // Acknowledge frame 1 so queue has no unacked frame
        let ack1 = ClientMessage::BrowserFrameAck {
            stream_id: 1,
            seq: 1,
        };
        session
            .dispatch_client_message(ack1, &backend, &admission, &out_tx, now)
            .await
            .unwrap();

        // 2. Pause session via BrowserPause
        let pause = ClientMessage::BrowserPause {
            browser_id: "b1".into(),
            stream_id: session.stream_id,
        };
        session
            .dispatch_client_message(pause, &backend, &admission, &out_tx, now)
            .await
            .unwrap();
        assert_eq!(session.state, WsConnectionState::Paused);
        assert!(
            session.subscription_id.is_some(),
            "Session not torn down on pause"
        );

        // While paused: frames are NOT forwarded (enqueue_frame returns None even without unacked frame)
        let f2 = vec![0x62, 1, 1, 1, 2, 0, 0, 0, 30, 40];
        assert_eq!(session.enqueue_frame(f2.clone(), now), None);

        // While paused: ACKs are paused and do NOT promote frames
        let ack2 = ClientMessage::BrowserFrameAck {
            stream_id: 1,
            seq: 2,
        };
        session
            .dispatch_client_message(ack2, &backend, &admission, &out_tx, now)
            .await
            .unwrap();
        assert_eq!(session.pending_promoted_frame, None);

        // 3. Resume session via BrowserResume
        let resume = ClientMessage::BrowserResume {
            browser_id: "b1".into(),
            stream_id: session.stream_id,
        };
        session
            .dispatch_client_message(resume, &backend, &admission, &out_tx, now)
            .await
            .unwrap();
        assert_eq!(session.state, WsConnectionState::Streaming);

        // After resume: frame forwarding works again
        let f3 = vec![0x62, 1, 1, 1, 3, 0, 0, 0, 50, 60];
        assert_eq!(session.enqueue_frame(f3.clone(), now), Some(f3));

        // Subsequent frame is backpressured until ACK
        let f4 = vec![0x62, 1, 1, 1, 4, 0, 0, 0, 70, 80];
        assert_eq!(session.enqueue_frame(f4.clone(), now), None);

        // ACK for frame 3 promotes frame 4
        let ack3 = ClientMessage::BrowserFrameAck {
            stream_id: 1,
            seq: 3,
        };
        session
            .dispatch_client_message(ack3, &backend, &admission, &out_tx, now)
            .await
            .unwrap();
        assert_eq!(session.pending_promoted_frame, Some(f4));
    }

    // -----------------------------------------------------------------------
    // R4-5 / R4-8 / R4-9 / R4-10 regression coverage
    // -----------------------------------------------------------------------

    struct SnapshotBackend {
        value: serde_json::Value,
    }
    impl RemoteBrowserBackend for SnapshotBackend {
        fn list_sessions<'a>(
            &'a self,
            _scope: &'a crate::remote::browser_backend::DesktopScope,
        ) -> futures_util::future::BoxFuture<
            'a,
            Result<
                Vec<crate::remote::browser_backend::RemoteBrowserSessionSummary>,
                RemoteBrowserError,
            >,
        > {
            Box::pin(async move { Ok(vec![]) })
        }
        fn identify_session<'a>(
            &'a self,
            _scope: &'a crate::remote::browser_backend::DesktopScope,
        ) -> futures_util::future::BoxFuture<
            'a,
            Result<
                Option<crate::remote::browser_backend::RemoteBrowserSessionSummary>,
                RemoteBrowserError,
            >,
        > {
            Box::pin(async move { Ok(None) })
        }
        fn get_state<'a>(
            &'a self,
            _browser_id: &'a str,
            _scope: &'a crate::remote::browser_backend::DesktopScope,
        ) -> futures_util::future::BoxFuture<
            'a,
            Result<crate::remote::browser_backend::BrowserRemoteState, RemoteBrowserError>,
        > {
            Box::pin(async move { Err(RemoteBrowserError::NotFound("b1".into())) })
        }
        fn execute_command(
            &self,
            ctx: BrowserCommandContext,
        ) -> futures_util::future::BoxFuture<'_, Result<BrowserCommandResult, RemoteBrowserError>>
        {
            let value = self.value.clone();
            Box::pin(async move {
                assert_eq!(ctx.command, "snapshot");
                Ok(BrowserCommandResult {
                    success: true,
                    value: Some(value),
                })
            })
        }
        fn capabilities(
            &self,
        ) -> futures_util::future::BoxFuture<'_, crate::remote::browser_backend::BrowserCapabilities>
        {
            Box::pin(async move {
                crate::remote::browser_backend::BrowserCapabilities {
                    browser_available: true,
                    supported_formats: vec![],
                    supported_commands: vec!["snapshot".into()],
                    max_edge: 2048,
                    max_fps: 8,
                }
            })
        }
    }

    struct CapturingBackend {
        last_ctx: std::sync::Mutex<Option<BrowserCommandContext>>,
    }
    impl RemoteBrowserBackend for CapturingBackend {
        fn list_sessions<'a>(
            &'a self,
            _scope: &'a crate::remote::browser_backend::DesktopScope,
        ) -> futures_util::future::BoxFuture<
            'a,
            Result<
                Vec<crate::remote::browser_backend::RemoteBrowserSessionSummary>,
                RemoteBrowserError,
            >,
        > {
            Box::pin(async move { Ok(vec![]) })
        }
        fn identify_session<'a>(
            &'a self,
            _scope: &'a crate::remote::browser_backend::DesktopScope,
        ) -> futures_util::future::BoxFuture<
            'a,
            Result<
                Option<crate::remote::browser_backend::RemoteBrowserSessionSummary>,
                RemoteBrowserError,
            >,
        > {
            Box::pin(async move { Ok(None) })
        }
        fn get_state<'a>(
            &'a self,
            _browser_id: &'a str,
            _scope: &'a crate::remote::browser_backend::DesktopScope,
        ) -> futures_util::future::BoxFuture<
            'a,
            Result<crate::remote::browser_backend::BrowserRemoteState, RemoteBrowserError>,
        > {
            Box::pin(async move { Err(RemoteBrowserError::NotFound("b1".into())) })
        }
        fn execute_command(
            &self,
            ctx: BrowserCommandContext,
        ) -> futures_util::future::BoxFuture<'_, Result<BrowserCommandResult, RemoteBrowserError>>
        {
            Box::pin(async move {
                *self.last_ctx.lock().unwrap() = Some(ctx);
                Ok(BrowserCommandResult {
                    success: true,
                    value: Some(serde_json::json!({ "clicked": true })),
                })
            })
        }
        fn subscribe_viewer<'a>(
            &'a self,
            _browser_id: &'a str,
            _device_id: &'a str,
            _viewer_instance_id: &'a str,
            options: Option<crate::remote::browser_protocol::BrowserSubscribeOptions>,
        ) -> futures_util::future::BoxFuture<
            'a,
            Result<
                (
                    String,
                    u32,
                    crate::remote::browser_protocol::BrowserSubscribeOptions,
                    crate::remote::browser_backend::BrowserSubscribeIdentity,
                ),
                RemoteBrowserError,
            >,
        > {
            Box::pin(async move {
                let sub_id = format!("sub-{}", uuid::Uuid::new_v4());
                let identity = crate::remote::browser_backend::BrowserSubscribeIdentity {
                    browser_instance_id: "bi1".into(),
                    browser_service_epoch: "1".into(),
                    desktop_epoch: "1".into(),
                    document_generation: "15".into(),
                };
                Ok((sub_id, 1, options.unwrap_or_default(), identity))
            })
        }
        fn capabilities(
            &self,
        ) -> futures_util::future::BoxFuture<'_, crate::remote::browser_backend::BrowserCapabilities>
        {
            Box::pin(async move {
                crate::remote::browser_backend::BrowserCapabilities {
                    browser_available: true,
                    supported_formats: vec![],
                    supported_commands: vec!["click".into()],
                    max_edge: 2048,
                    max_fps: 8,
                }
            })
        }
    }

    fn click_test_metadata(
        seq_stream_id: u32,
    ) -> crate::remote::browser_protocol::BrowserFrameMetadata {
        crate::remote::browser_protocol::BrowserFrameMetadata {
            offset_top: 0.0,
            page_scale_factor: 1.0,
            device_width: 1280.0,
            device_height: 800.0,
            image_width: 1,
            image_height: 1,
            scroll_offset_x: 0.0,
            scroll_offset_y: 0.0,
            timestamp: 1726560000.0,
            stream_id: seq_stream_id,
            browser_instance_id: "bi1".into(),
            browser_service_epoch: "1".into(),
            desktop_epoch: "1".into(),
            document_generation: "15".into(),
            viewport_revision: "3".into(),
            capture_rect: crate::remote::browser_protocol::BrowserCaptureRect {
                x: 0.0,
                y: 0.0,
                width: 1280.0,
                height: 800.0,
            },
            geometry_source: "wkSnapshot".into(),
        }
    }

    fn sample_click_frame(seq: u32, stream_id: u32) -> Vec<u8> {
        let png: &[u8] = &[
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00,
            0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ];
        crate::remote::browser_protocol::encode_binary_frame(
            crate::remote::browser_protocol::BrowserImageFormat::Png,
            seq,
            &click_test_metadata(stream_id),
            png,
        )
        .expect("encode click test frame")
    }

    async fn subscribe_and_claim(
        session: &mut BrowserWsSession,
        backend: &Arc<dyn RemoteBrowserBackend>,
        admission: &AdmissionController,
        out_tx: &tokio::sync::mpsc::Sender<ServerMessage>,
        out_rx: &mut tokio::sync::mpsc::Receiver<ServerMessage>,
        now: Instant,
    ) {
        let sub = ClientMessage::BrowserSubscribe {
            request_id: "r-sub".into(),
            viewer_instance_id: "v1".into(),
            options: Default::default(),
        };
        session
            .dispatch_client_message(sub, backend, admission, out_tx, now)
            .await
            .unwrap();
        let _ = out_rx.recv().await.unwrap();

        let claim = ClientMessage::BrowserDriverClaim {
            request_id: "r-claim".into(),
            subscription_id: session.subscription_id.clone().unwrap(),
            browser_id: "b1".into(),
        };
        session
            .dispatch_client_message(claim, backend, admission, out_tx, now)
            .await
            .unwrap();
        let _ = out_rx.recv().await.unwrap();
    }

    /// R4-10: a frame dropped while hidden must not strand ACK credit across resume.
    #[tokio::test]
    async fn test_r4_10_resume_resets_stranded_ack_credit() {
        let now = Instant::now();
        let mut session = BrowserWsSession::new(
            "c-credit".into(),
            "d-credit".into(),
            "b1".into(),
            DevicePermission::Control,
            now,
        );
        let admission = AdmissionController::new();
        let backend: Arc<dyn RemoteBrowserBackend> = Arc::new(InProcessTestBackend::new());
        let (out_tx, mut out_rx) = tokio::sync::mpsc::channel(16);

        let sub = ClientMessage::BrowserSubscribe {
            request_id: "r-sub".into(),
            viewer_instance_id: "v1".into(),
            options: Default::default(),
        };
        session
            .dispatch_client_message(sub, &backend, &admission, &out_tx, now)
            .await
            .unwrap();
        let _ = out_rx.recv().await.unwrap();

        // Frame 1 is sent but the viewer goes hidden before acknowledging it.
        let f1 = vec![0x62, 1, 1, 1, 1, 0, 0, 0, 10, 20];
        assert_eq!(session.enqueue_frame(f1.clone(), now), Some(f1));

        let pause = ClientMessage::BrowserPause {
            browser_id: "b1".into(),
            stream_id: session.stream_id,
        };
        session
            .dispatch_client_message(pause, &backend, &admission, &out_tx, now)
            .await
            .unwrap();
        assert_eq!(session.state, WsConnectionState::Paused);

        let resume = ClientMessage::BrowserResume {
            browser_id: "b1".into(),
            stream_id: session.stream_id,
        };
        session
            .dispatch_client_message(resume, &backend, &admission, &out_tx, now)
            .await
            .unwrap();
        assert_eq!(session.state, WsConnectionState::Streaming);

        // Credit must have been reset: the next frame flows without an ACK for the stranded frame 1.
        let f2 = vec![0x62, 1, 1, 1, 2, 0, 0, 0, 30, 40];
        assert_eq!(
            session.enqueue_frame(f2.clone(), now),
            Some(f2),
            "resume must reset ACK credit and request a fresh frame"
        );

        // Pause/resume bound to a foreign stream id must be rejected.
        let bad_pause = ClientMessage::BrowserPause {
            browser_id: "b1".into(),
            stream_id: session.stream_id + 99,
        };
        session
            .dispatch_client_message(bad_pause, &backend, &admission, &out_tx, now)
            .await
            .unwrap();
        assert_eq!(session.state, WsConnectionState::Streaming);
        match out_rx.recv().await.unwrap() {
            ServerMessage::BrowserError { code, .. } => assert_eq!(code, "BROWSER_INVALID_REQUEST"),
            other => panic!("Expected BROWSER_INVALID_REQUEST, got {other:?}"),
        }
    }

    /// R4-9: browserSnapshot must be a View-accessible wire request returning a real reference catalogue.
    #[tokio::test]
    async fn test_r4_9_browser_snapshot_returns_reference_catalogue() {
        let now = Instant::now();
        let admission = AdmissionController::new();
        let mut session = BrowserWsSession::new(
            "c-snap".into(),
            "d-snap".into(),
            "b1".into(),
            DevicePermission::View, // View-only device must be able to snapshot
            now,
        );
        let backend: Arc<dyn RemoteBrowserBackend> = Arc::new(SnapshotBackend {
            value: serde_json::json!({
                "snapshotId": "snap-1",
                "mapRevision": 12,
                "documentGeneration": "15",
                "elements": [
                    { "ref": "e1", "role": "button", "name": "Send" },
                    { "ref": "e2", "role": "textbox", "name": "Message" }
                ]
            }),
        });
        let (out_tx, mut out_rx) = tokio::sync::mpsc::channel(16);

        let sub = ClientMessage::BrowserSubscribe {
            request_id: "r-sub".into(),
            viewer_instance_id: "v1".into(),
            options: Default::default(),
        };
        session
            .dispatch_client_message(sub, &backend, &admission, &out_tx, now)
            .await
            .unwrap();
        let _ = out_rx.recv().await.unwrap();

        let snap = ClientMessage::BrowserSnapshot {
            request_id: "r-snap".into(),
            browser_id: "b1".into(),
        };
        session
            .dispatch_client_message(snap, &backend, &admission, &out_tx, now)
            .await
            .unwrap();

        match out_rx.recv().await.unwrap() {
            ServerMessage::BrowserSnapshot {
                request_id,
                snapshot_id,
                map_revision,
                elements,
                ..
            } => {
                assert_eq!(request_id, "r-snap");
                assert_eq!(snapshot_id, "snap-1");
                assert_eq!(map_revision, "12", "mapRevision must be a decimal string");
                assert_eq!(elements.len(), 2, "catalogue must carry public references");
                assert_eq!(elements[0].get("ref").and_then(|v| v.as_str()), Some("e1"));
            }
            other => panic!("Expected BrowserSnapshot, got {other:?}"),
        }

        // Counts-only responses are not a reference catalogue and must be rejected.
        let counts_backend: Arc<dyn RemoteBrowserBackend> = Arc::new(SnapshotBackend {
            value: serde_json::json!({
                "snapshotId": "snap-2",
                "mapRevision": "13",
                "elementsCount": 7
            }),
        });
        let snap2 = ClientMessage::BrowserSnapshot {
            request_id: "r-snap-2".into(),
            browser_id: "b1".into(),
        };
        session
            .dispatch_client_message(snap2, &counts_backend, &admission, &out_tx, now)
            .await
            .unwrap();
        match out_rx.recv().await.unwrap() {
            ServerMessage::BrowserError { code, .. } => {
                assert_eq!(code, "BROWSER_EXECUTION_FAILED");
            }
            other => {
                panic!("Expected BROWSER_EXECUTION_FAILED for counts-only snapshot, got {other:?}")
            }
        }
    }

    /// R4-8: point clicks are fenced by owned sent-frame records and use the real displayed geometry.
    #[tokio::test]
    async fn test_r4_8_point_click_sent_frame_fence_and_real_geometry() {
        let now = Instant::now();
        let admission = AdmissionController::new();
        let mut session = BrowserWsSession::new(
            "c-click".into(),
            "d-click".into(),
            "b1".into(),
            DevicePermission::Control,
            now,
        );
        let capturing = Arc::new(CapturingBackend {
            last_ctx: std::sync::Mutex::new(None),
        });
        let backend: Arc<dyn RemoteBrowserBackend> = capturing.clone();
        let (out_tx, mut out_rx) = tokio::sync::mpsc::channel(16);

        subscribe_and_claim(
            &mut session,
            &backend,
            &admission,
            &out_tx,
            &mut out_rx,
            now,
        )
        .await;
        let epoch = session.lease_epoch.unwrap().to_string();
        let stream_id = session.stream_id;

        // 1. Point click without displayed-frame metadata is rejected (metadata is required).
        let bare_click = ClientMessage::BrowserCommand {
            request_id: "c-bare".into(),
            request_seq: "1".into(),
            browser_id: "b1".into(),
            lease_epoch: epoch.clone(),
            browser_instance_id: "bi1".into(),
            desktop_epoch: "1".into(),
            document_generation: "15".into(),
            command: "click".into(),
            params: Some(serde_json::json!({ "u": 0.5, "v": 0.25 })),
        };
        session
            .dispatch_client_message(bare_click, &backend, &admission, &out_tx, now)
            .await
            .unwrap();
        match out_rx.recv().await.unwrap() {
            ServerMessage::BrowserError { code, .. } => assert_eq!(code, "BROWSER_INVALID_REQUEST"),
            other => panic!(
                "Expected BROWSER_INVALID_REQUEST for click without frame metadata, got {other:?}"
            ),
        }

        // Send and acknowledge two real frames.
        let f1 = sample_click_frame(1, stream_id);
        assert!(session.enqueue_frame(f1, now).is_some());
        let ack1 = ClientMessage::BrowserFrameAck { stream_id, seq: 1 };
        session
            .dispatch_client_message(ack1, &backend, &admission, &out_tx, now)
            .await
            .unwrap();
        let f2 = sample_click_frame(2, stream_id);
        assert!(session.enqueue_frame(f2, now).is_some());
        let ack2 = ClientMessage::BrowserFrameAck { stream_id, seq: 2 };
        session
            .dispatch_client_message(ack2, &backend, &admission, &out_tx, now)
            .await
            .unwrap();

        // 2. Click referencing a frame older than the last acked frame is rejected.
        let stale_click = ClientMessage::BrowserCommand {
            request_id: "c-stale".into(),
            request_seq: "2".into(),
            browser_id: "b1".into(),
            lease_epoch: epoch.clone(),
            browser_instance_id: "bi1".into(),
            desktop_epoch: "1".into(),
            document_generation: "15".into(),
            command: "click".into(),
            params: Some(serde_json::json!({
                "u": 0.5,
                "v": 0.25,
                "streamId": stream_id,
                "sequenceNumber": 1,
                "documentGeneration": "15",
                "viewportRevision": "3",
                "browserInstanceId": "bi1"
            })),
        };
        session
            .dispatch_client_message(stale_click, &backend, &admission, &out_tx, now)
            .await
            .unwrap();
        match out_rx.recv().await.unwrap() {
            ServerMessage::BrowserError { code, .. } => assert_eq!(code, "BROWSER_STALE_FRAME"),
            other => panic!("Expected BROWSER_STALE_FRAME for superseded frame, got {other:?}"),
        }

        // 3. Click on the current frame succeeds and carries the real displayed geometry.
        let good_click = ClientMessage::BrowserCommand {
            request_id: "c-good".into(),
            request_seq: "3".into(),
            browser_id: "b1".into(),
            lease_epoch: epoch,
            browser_instance_id: "bi1".into(),
            desktop_epoch: "1".into(),
            document_generation: "15".into(),
            command: "click".into(),
            params: Some(serde_json::json!({
                "u": 0.5,
                "v": 0.25,
                "streamId": stream_id,
                "sequenceNumber": 2,
                "documentGeneration": "15",
                "viewportRevision": "3",
                "browserInstanceId": "bi1"
            })),
        };
        session
            .dispatch_client_message(good_click, &backend, &admission, &out_tx, now)
            .await
            .unwrap();
        match out_rx.recv().await.unwrap() {
            ServerMessage::BrowserResult { request_id, .. } => assert_eq!(request_id, "c-good"),
            other => panic!("Expected BrowserResult for fenced click, got {other:?}"),
        }

        let ctx = capturing
            .last_ctx
            .lock()
            .unwrap()
            .clone()
            .expect("click reached backend");
        let params = ctx.params.expect("click params");
        assert_eq!(
            params.get("captureRect"),
            Some(&serde_json::json!({ "x": 0.0, "y": 0.0, "width": 1280.0, "height": 800.0 })),
            "click must carry the real displayed capture rect"
        );
        assert_eq!(params.get("x").and_then(|v| v.as_f64()), Some(640.0));
        assert_eq!(params.get("y").and_then(|v| v.as_f64()), Some(200.0));
        assert_eq!(
            params.get("geometrySource").and_then(|v| v.as_str()),
            Some("wkSnapshot")
        );
    }

    /// R4-5: admission/revocation transitions publish authoritative sharing state for the desktop indicator.
    #[tokio::test]
    async fn test_r4_5_sharing_state_published_on_admission_and_revocation() {
        let now = Instant::now();
        let admission = AdmissionController::new();
        let registry = Arc::new(SharingRegistry::new());
        let mut sharing_rx = registry.subscribe();
        let mut session = BrowserWsSession::new(
            "c-share".into(),
            "d-share".into(),
            "b1".into(),
            DevicePermission::Control,
            now,
        )
        .with_sharing_registry(Arc::clone(&registry));
        let backend: Arc<dyn RemoteBrowserBackend> = Arc::new(InProcessTestBackend::new());
        let (out_tx, mut out_rx) = tokio::sync::mpsc::channel(16);

        subscribe_and_claim(
            &mut session,
            &backend,
            &admission,
            &out_tx,
            &mut out_rx,
            now,
        )
        .await;

        // Viewer admission publishes sharing state
        let viewing = sharing_rx.recv().await.unwrap();
        assert!(viewing.is_sharing);
        assert_eq!(viewing.active_sessions_count, 1);
        assert_eq!(viewing.driver_status, SharingDriverStatus::Viewing);

        // Driver claim publishes driving state with the owning device
        let driving = sharing_rx.recv().await.unwrap();
        assert!(driving.is_sharing);
        assert_eq!(driving.driver_status, SharingDriverStatus::Driving);
        assert_eq!(driving.driver_device_id.as_deref(), Some("d-share"));

        // Desktop reclaim revocation publishes the loss of remote control
        session.mark_driver_revoked("desktop_reclaim");
        let revoked = sharing_rx.recv().await.unwrap();
        assert_eq!(revoked.driver_status, SharingDriverStatus::Viewing);
        assert!(revoked.is_sharing);

        // Teardown publishes the inactive state
        session.teardown(&admission);
        let idle = sharing_rx.recv().await.unwrap();
        assert!(!idle.is_sharing);
        assert_eq!(idle.active_sessions_count, 0);
        assert_eq!(idle.driver_status, SharingDriverStatus::Idle);
    }

    /// R4-15: Capabilities advertisement derives from installed runtime support (querying snapshot source).
    #[tokio::test]
    async fn test_r4_15_capabilities_derived_from_installed_snapshot_source() {
        use crate::browser::manager::BrowserManager;
        use crate::browser::remote_driver::RemoteDriverBroker;
        use crate::browser::remote_service::BrowserRemoteService;
        use crate::browser::snapshot_source::UnsupportedSnapshotSource;
        use crate::remote::browser_backend::InProcessBrowserServiceBackend;

        let manager = Arc::new(BrowserManager::new());
        let broker = Arc::new(RemoteDriverBroker::new());
        let remote_service = Arc::new(BrowserRemoteService::with_snapshot_source(
            (*manager).clone(),
            broker,
            Arc::new(UnsupportedSnapshotSource),
        ));
        let backend = InProcessBrowserServiceBackend::new(remote_service, manager);
        let caps = backend.capabilities().await;

        assert!(
            caps.supported_formats.is_empty(),
            "When snapshot source is unsupported, supported_formats must be empty"
        );
        assert_eq!(caps.max_edge, 0);
        assert_eq!(caps.max_fps, 0);
        assert!(
            !caps.supported_commands.contains(&"snapshot".to_string()),
            "Unsupported screencast platform must not advertise snapshot command"
        );
    }

    /// R4-15: Negotiated capture profile (format / interval / max_edge) threads from WS subscription
    /// and the first negotiated profile is enforced and reported to subsequent subscribers.
    #[tokio::test]
    async fn test_r4_15_first_negotiated_profile_enforced_and_reported() {
        use crate::remote::browser_protocol::BrowserImageFormat;

        let now = Instant::now();
        let admission = AdmissionController::new();
        let backend: Arc<dyn RemoteBrowserBackend> = Arc::new(InProcessTestBackend::new());

        let mut session1 = BrowserWsSession::new(
            "conn-p1".into(),
            "dev-p1".into(),
            "b1".into(),
            DevicePermission::Control,
            now,
        );
        let (tx1, mut rx1) = tokio::sync::mpsc::channel(16);

        // Client 1 negotiates PNG profile with interval 120ms and max_edge 1024
        let sub1 = ClientMessage::BrowserSubscribe {
            request_id: "r-sub-1".into(),
            viewer_instance_id: "v1".into(),
            options: BrowserSubscribeOptions {
                format: BrowserImageFormat::Png,
                quality: None,
                interval_ms: Some(120),
                max_edge: Some(1024),
            },
        };
        session1
            .dispatch_client_message(sub1, &backend, &admission, &tx1, now)
            .await
            .unwrap();

        let resp1 = rx1.recv().await.unwrap();
        match resp1 {
            ServerMessage::BrowserSubscribed { options, .. } => {
                assert_eq!(options.format, BrowserImageFormat::Png);
                assert_eq!(options.interval_ms, Some(120));
                assert_eq!(options.max_edge, Some(1024));
            }
            other => panic!("Expected BrowserSubscribed for sub 1, got {other:?}"),
        }

        // Client 2 subscribes with different options (JPEG, 500ms, 800)
        let mut session2 = BrowserWsSession::new(
            "conn-p2".into(),
            "dev-p2".into(),
            "b1".into(),
            DevicePermission::View,
            now,
        );
        let (tx2, mut rx2) = tokio::sync::mpsc::channel(16);

        let sub2 = ClientMessage::BrowserSubscribe {
            request_id: "r-sub-2".into(),
            viewer_instance_id: "v2".into(),
            options: BrowserSubscribeOptions {
                format: BrowserImageFormat::Jpeg,
                quality: Some(50),
                interval_ms: Some(500),
                max_edge: Some(800),
            },
        };
        session2
            .dispatch_client_message(sub2, &backend, &admission, &tx2, now)
            .await
            .unwrap();

        let resp2 = rx2.recv().await.unwrap();
        match resp2 {
            ServerMessage::BrowserSubscribed { options, .. } => {
                // Must ENFORCE and REPORT the first negotiated profile (PNG, 120ms, 1024)
                assert_eq!(
                    options.format,
                    BrowserImageFormat::Png,
                    "Second subscriber must be bound to the first negotiated profile format"
                );
                assert_eq!(
                    options.interval_ms,
                    Some(120),
                    "Second subscriber must be bound to the first negotiated interval"
                );
                assert_eq!(
                    options.max_edge,
                    Some(1024),
                    "Second subscriber must be bound to the first negotiated maxEdge"
                );
            }
            other => panic!("Expected BrowserSubscribed for sub 2, got {other:?}"),
        }
    }

    /// R4-15: Paused and stalled viewers are not capture-eligible; capture halts when no viewers are eligible.
    #[tokio::test]
    async fn test_r4_15_paused_and_stalled_viewers_not_capture_eligible() {
        let now = Instant::now();
        let admission = AdmissionController::new();
        let mut capture_rx = admission.subscribe_capture();
        let backend: Arc<dyn RemoteBrowserBackend> = Arc::new(InProcessTestBackend::new());

        let mut session = BrowserWsSession::new(
            "conn-pause".into(),
            "dev-pause".into(),
            "b1".into(),
            DevicePermission::Control,
            now,
        );
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);

        let sub = ClientMessage::BrowserSubscribe {
            request_id: "r-sub".into(),
            viewer_instance_id: "v1".into(),
            options: BrowserSubscribeOptions::default(),
        };
        session
            .dispatch_client_message(sub, &backend, &admission, &tx, now)
            .await
            .unwrap();
        let _ = rx.recv().await.unwrap();

        // Initially capturing is active
        assert!(admission.should_capture("b1"));
        let (init_browser, init_capturing) = capture_rx.recv().await.unwrap();
        assert_eq!(init_browser, "b1");
        assert!(init_capturing);

        // Viewer pauses: admission must NOT treat paused viewer as capture-eligible
        let pause = ClientMessage::BrowserPause {
            browser_id: "b1".into(),
            stream_id: session.stream_id,
        };
        session
            .dispatch_client_message(pause, &backend, &admission, &tx, now)
            .await
            .unwrap();

        assert!(
            !admission.should_capture("b1"),
            "Paused viewer must not be capture-eligible"
        );
        // Capture broadcast must signal halt (b1, false)
        let (halted_browser, capturing) = capture_rx.recv().await.unwrap();
        assert_eq!(halted_browser, "b1");
        assert!(!capturing);

        // Viewer resumes: capture eligibility is restored
        let resume = ClientMessage::BrowserResume {
            browser_id: "b1".into(),
            stream_id: session.stream_id,
        };
        session
            .dispatch_client_message(resume, &backend, &admission, &tx, now)
            .await
            .unwrap();

        assert!(
            admission.should_capture("b1"),
            "Resumed viewer must restore capture eligibility"
        );
        let (resumed_browser, capturing) = capture_rx.recv().await.unwrap();
        assert_eq!(resumed_browser, "b1");
        assert!(capturing);
    }

    /// R4-7 Part A: WS subscribe response carries backend's actual identities (not hard-coded literals).
    #[tokio::test]
    async fn test_r4_7_part_a_real_identity_negotiation() {
        use crate::browser::manager::BrowserManager;
        use crate::browser::model::CreateBrowserRequest;
        use crate::browser::remote_driver::RemoteDriverBroker;
        use crate::browser::remote_service::BrowserRemoteService;
        use crate::remote::browser_backend::InProcessBrowserServiceBackend;

        let now = Instant::now();
        let manager = Arc::new(BrowserManager::new());
        let broker = Arc::new(RemoteDriverBroker::new());
        let service = Arc::new(BrowserRemoteService::new((*manager).clone(), broker));
        let backend: Arc<dyn RemoteBrowserBackend> = Arc::new(InProcessBrowserServiceBackend::new(
            service.clone(),
            manager.clone(),
        ));

        let created = manager
            .register_session(CreateBrowserRequest {
                browser_id: Some("b-real-1".into()),
                workspace_id: Some("ws-real".into()),
                worktree_path: None,
                url: "https://example.com".into(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .unwrap();

        let real_instance = manager.get_instance_id("b-real-1").unwrap();
        assert_ne!(
            real_instance, "bi1",
            "Real instance ID must not be bi1 literal"
        );

        let admission = AdmissionController::new();
        let mut session = BrowserWsSession::new(
            "c-real".into(),
            "d-real".into(),
            "b-real-1".into(),
            DevicePermission::Control,
            now,
        );

        let sub_msg = ClientMessage::BrowserSubscribe {
            request_id: "req-sub-real".into(),
            viewer_instance_id: "v-real".into(),
            options: BrowserSubscribeOptions::default(),
        };

        let resp = session
            .handle_client_message(sub_msg, &backend, &admission, now)
            .await
            .unwrap()
            .unwrap();

        let ServerMessage::BrowserSubscribed {
            browser_instance_id,
            browser_service_epoch,
            desktop_epoch,
            document_generation,
            ..
        } = resp
        else {
            panic!("Expected BrowserSubscribed, got: {:?}", resp);
        };

        assert_eq!(
            browser_instance_id, real_instance,
            "browser_instance_id must match backend real instance, not hard-coded bi1"
        );
        assert_eq!(browser_service_epoch, service.service_epoch().to_string());
        assert_eq!(desktop_epoch, service.desktop_epoch().to_string());
        assert_eq!(document_generation, created.generation.to_string());

        // A second configured browser has distinct identity that differs per browser
        let _created2 = manager
            .register_session(CreateBrowserRequest {
                browser_id: Some("b-real-2".into()),
                workspace_id: Some("ws-real".into()),
                worktree_path: None,
                url: "https://example.com/2".into(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .unwrap();
        let real_instance2 = manager.get_instance_id("b-real-2").unwrap();
        assert_ne!(real_instance, real_instance2);

        // Teardown first session and its backend subscription so captured browser slot (max 1) is freed
        session.teardown_with_backend(&admission, &backend).await;

        let mut session2 = BrowserWsSession::new(
            "c-real-2".into(),
            "d-real-2".into(),
            "b-real-2".into(),
            DevicePermission::Control,
            now,
        );
        let sub_msg2 = ClientMessage::BrowserSubscribe {
            request_id: "req-sub-real-2".into(),
            viewer_instance_id: "v-real-2".into(),
            options: BrowserSubscribeOptions::default(),
        };
        let resp2 = session2
            .handle_client_message(sub_msg2, &backend, &admission, now)
            .await
            .unwrap()
            .unwrap();
        let ServerMessage::BrowserSubscribed {
            browser_instance_id: inst2,
            ..
        } = resp2
        else {
            panic!("Expected BrowserSubscribed");
        };
        assert_eq!(inst2, real_instance2);

        // Subscribing to non-existent session fails with BROWSER_SUBSCRIPTION_FAILED and rolls back admission
        let mut session_missing = BrowserWsSession::new(
            "c-missing".into(),
            "d-missing".into(),
            "b-missing".into(),
            DevicePermission::Control,
            now,
        );
        let sub_missing = ClientMessage::BrowserSubscribe {
            request_id: "req-sub-missing".into(),
            viewer_instance_id: "v-missing".into(),
            options: BrowserSubscribeOptions::default(),
        };
        let resp_missing = session_missing
            .handle_client_message(sub_missing, &backend, &admission, now)
            .await
            .unwrap()
            .unwrap();
        match resp_missing {
            ServerMessage::BrowserError { ref code, .. } => {
                assert_eq!(code, "BROWSER_SUBSCRIPTION_FAILED");
            }
            other => panic!("Expected BROWSER_SUBSCRIPTION_FAILED, got: {:?}", other),
        }
        assert!(
            !admission.should_capture("b-missing"),
            "Admission must be rolled back on failed subscribe"
        );
    }

    /// R4-7 Part B: Command identity fencing rejects stale instance/epoch/generation with BROWSER_STALE_IDENTITY.
    #[tokio::test]
    async fn test_r4_7_part_b_command_identity_fencing() {
        let now = Instant::now();
        let mut session = BrowserWsSession::new(
            "c-fence".into(),
            "d-fence".into(),
            "b1".into(),
            DevicePermission::Control,
            now,
        );
        let backend: Arc<dyn RemoteBrowserBackend> = Arc::new(InProcessTestBackend::new());
        let admission = AdmissionController::new();

        // 1. Subscribe
        let sub_msg = ClientMessage::BrowserSubscribe {
            request_id: "r1".into(),
            viewer_instance_id: "v1".into(),
            options: BrowserSubscribeOptions::default(),
        };
        let _ = session
            .handle_client_message(sub_msg, &backend, &admission, now)
            .await
            .unwrap();

        // 2. Claim driver
        let claim_msg = ClientMessage::BrowserDriverClaim {
            request_id: "r2".into(),
            subscription_id: session.subscription_id.clone().unwrap(),
            browser_id: "b1".into(),
        };
        let _ = session
            .handle_client_message(claim_msg, &backend, &admission, now)
            .await
            .unwrap();

        let lease_str = session.lease_epoch.unwrap().to_string();

        // 3. Stale instance ID rejected with BROWSER_STALE_IDENTITY
        let cmd_stale_inst = ClientMessage::BrowserCommand {
            request_id: "cmd-1".into(),
            request_seq: "1".into(),
            browser_id: "b1".into(),
            lease_epoch: lease_str.clone(),
            browser_instance_id: "stale-instance-xyz".into(),
            desktop_epoch: "1".into(),
            document_generation: "1".into(),
            command: "click".into(),
            params: None,
        };
        let resp = session
            .handle_client_message(cmd_stale_inst, &backend, &admission, now)
            .await
            .unwrap()
            .unwrap();
        match resp {
            ServerMessage::BrowserError { ref code, .. } => {
                assert_eq!(
                    code, "BROWSER_STALE_IDENTITY",
                    "Stale instance must be rejected with BROWSER_STALE_IDENTITY"
                );
            }
            other => panic!("Expected BROWSER_STALE_IDENTITY error, got: {:?}", other),
        }

        // 4. Stale desktop epoch rejected with BROWSER_STALE_IDENTITY
        let cmd_stale_epoch = ClientMessage::BrowserCommand {
            request_id: "cmd-2".into(),
            request_seq: "2".into(),
            browser_id: "b1".into(),
            lease_epoch: lease_str.clone(),
            browser_instance_id: "bi1".into(),
            desktop_epoch: "999".into(),
            document_generation: "1".into(),
            command: "click".into(),
            params: None,
        };
        let resp2 = session
            .handle_client_message(cmd_stale_epoch, &backend, &admission, now)
            .await
            .unwrap()
            .unwrap();
        match resp2 {
            ServerMessage::BrowserError { ref code, .. } => {
                assert_eq!(
                    code, "BROWSER_STALE_IDENTITY",
                    "Stale desktop epoch must be rejected with BROWSER_STALE_IDENTITY"
                );
            }
            other => panic!("Expected BROWSER_STALE_IDENTITY error, got: {:?}", other),
        }

        // 5. Mismatched document generation rejected with BROWSER_STALE_IDENTITY
        let cmd_stale_gen = ClientMessage::BrowserCommand {
            request_id: "cmd-3".into(),
            request_seq: "3".into(),
            browser_id: "b1".into(),
            lease_epoch: lease_str.clone(),
            browser_instance_id: "bi1".into(),
            desktop_epoch: "1".into(),
            document_generation: "999".into(),
            command: "click".into(),
            params: None,
        };
        let resp3 = session
            .handle_client_message(cmd_stale_gen, &backend, &admission, now)
            .await
            .unwrap()
            .unwrap();
        match resp3 {
            ServerMessage::BrowserError { ref code, .. } => {
                assert_eq!(
                    code, "BROWSER_STALE_IDENTITY",
                    "Mismatched documentGeneration must be rejected with BROWSER_STALE_IDENTITY"
                );
            }
            other => panic!("Expected BROWSER_STALE_IDENTITY error, got: {:?}", other),
        }

        // 6. Missing document generation (empty string) rejected with BROWSER_STALE_IDENTITY
        let cmd_missing_gen = ClientMessage::BrowserCommand {
            request_id: "cmd-4".into(),
            request_seq: "4".into(),
            browser_id: "b1".into(),
            lease_epoch: lease_str.clone(),
            browser_instance_id: "bi1".into(),
            desktop_epoch: "1".into(),
            document_generation: "".into(),
            command: "click".into(),
            params: None,
        };
        let resp4 = session
            .handle_client_message(cmd_missing_gen, &backend, &admission, now)
            .await
            .unwrap()
            .unwrap();
        match resp4 {
            ServerMessage::BrowserError { ref code, .. } => {
                assert_eq!(
                    code, "BROWSER_STALE_IDENTITY",
                    "Missing documentGeneration must be rejected with BROWSER_STALE_IDENTITY"
                );
            }
            other => panic!("Expected BROWSER_STALE_IDENTITY error, got: {:?}", other),
        }

        // 7. Valid tuple executes successfully
        let cmd_valid = ClientMessage::BrowserCommand {
            request_id: "cmd-5".into(),
            request_seq: "5".into(),
            browser_id: "b1".into(),
            lease_epoch: lease_str,
            browser_instance_id: "bi1".into(),
            desktop_epoch: "1".into(),
            document_generation: "1".into(),
            command: "getState".into(),
            params: None,
        };
        let (out_tx, mut out_rx) = tokio::sync::mpsc::channel(16);
        session
            .dispatch_client_message(cmd_valid, &backend, &admission, &out_tx, now)
            .await
            .unwrap();
        let valid_resp = out_rx.recv().await.unwrap();
        assert!(
            matches!(valid_resp, ServerMessage::BrowserResult { .. }),
            "Valid tuple must execute successfully"
        );
    }

    /// R4-7 Part B: InProcess backend executes through remote_service.execute_command_guard.
    #[tokio::test]
    async fn test_r4_7_part_b_in_process_backend_executes_through_service_guard() {
        use crate::browser::manager::BrowserManager;
        use crate::browser::model::CreateBrowserRequest;
        use crate::browser::remote_driver::RemoteDriverBroker;
        use crate::browser::remote_service::BrowserRemoteService;
        use crate::remote::browser_backend::InProcessBrowserServiceBackend;

        let manager = Arc::new(BrowserManager::new());
        let broker = Arc::new(RemoteDriverBroker::new());
        let service = Arc::new(BrowserRemoteService::new(
            (*manager).clone(),
            Arc::clone(&broker),
        ));
        let backend =
            InProcessBrowserServiceBackend::new(Arc::clone(&service), Arc::clone(&manager));

        manager
            .register_session(CreateBrowserRequest {
                browser_id: Some("b-guard-1".into()),
                workspace_id: Some("ws-1".into()),
                worktree_path: None,
                url: "https://example.com".into(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .unwrap();

        let real_instance = manager.get_instance_id("b-guard-1").unwrap();
        let lease = broker
            .claim("dev-guard", "conn-guard", "sub-guard", "b-guard-1", true)
            .unwrap();

        // Stale instance rejected through guarded path
        let stale_inst_res = backend
            .execute_command(BrowserCommandContext {
                browser_id: "b-guard-1".into(),
                command: "getState".into(),
                params: None,
                document_generation: Some("1".into()),
                browser_instance_id: Some("wrong-inst".into()),
                desktop_epoch: Some(service.desktop_epoch().to_string()),
                lease_epoch: Some(lease.lease_epoch.to_string()),
                device_id: Some("dev-guard".into()),
                connection_id: Some("conn-guard".into()),
            })
            .await;
        assert!(
            stale_inst_res.is_err(),
            "Guarded path must reject stale instance"
        );

        // Stale epoch rejected through guarded path
        let stale_epoch_res = backend
            .execute_command(BrowserCommandContext {
                browser_id: "b-guard-1".into(),
                command: "getState".into(),
                params: None,
                document_generation: Some("1".into()),
                browser_instance_id: Some(real_instance.clone()),
                desktop_epoch: Some("9999".into()),
                lease_epoch: Some(lease.lease_epoch.to_string()),
                device_id: Some("dev-guard".into()),
                connection_id: Some("conn-guard".into()),
            })
            .await;
        assert!(
            stale_epoch_res.is_err(),
            "Guarded path must reject stale desktop epoch"
        );

        // Stale generation rejected through guarded path
        let stale_gen_res = backend
            .execute_command(BrowserCommandContext {
                browser_id: "b-guard-1".into(),
                command: "getState".into(),
                params: None,
                document_generation: Some("9999".into()),
                browser_instance_id: Some(real_instance.clone()),
                desktop_epoch: Some(service.desktop_epoch().to_string()),
                lease_epoch: Some(lease.lease_epoch.to_string()),
                device_id: Some("dev-guard".into()),
                connection_id: Some("conn-guard".into()),
            })
            .await;
        assert!(
            stale_gen_res.is_err(),
            "Guarded path must reject stale generation"
        );

        // Valid tuple succeeds through guarded path
        let valid_res = backend
            .execute_command(BrowserCommandContext {
                browser_id: "b-guard-1".into(),
                command: "getState".into(),
                params: None,
                document_generation: Some("1".into()),
                browser_instance_id: Some(real_instance),
                desktop_epoch: Some(service.desktop_epoch().to_string()),
                lease_epoch: Some(lease.lease_epoch.to_string()),
                device_id: Some("dev-guard".into()),
                connection_id: Some("conn-guard".into()),
            })
            .await;
        assert!(
            valid_res.is_ok(),
            "Guarded path must accept valid tuple: {:?}",
            valid_res
        );
    }
}
