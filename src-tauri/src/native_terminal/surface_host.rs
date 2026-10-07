use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use tauri::{Emitter, Manager, PhysicalSize, Runtime, Window};

use super::composition::{
    CellMetrics, LogicalBounds, PhysicalBounds, PlatformCompositorDescriptor,
    SurfaceCompositionLayout, SurfacePresentationGeometry,
};
use super::engine::TerminalEngine;
use super::error::NativeTerminalError;
use super::input::{cursor_style_for_focus, NativeTerminalInput};
use super::platform::PlatformCompositorTarget;
use super::renderer::font_manager;
use super::renderer::{NativeTerminalRenderer, RendererConfig, RendererTheme, SelectionSnapshot};
use super::scroll::ScrollbarOverlayState;
use super::snapshot::{CellSnapshot, CellWide, RenderSnapshot};
use super::surface_error::{classify_surface_error, SurfaceFrameAction};
pub use super::surface_snapshot::snapshot_for_layout;
use super::terminal::NativeTerminal;
use super::thread_ownership::{GpuThread, GpuWorker, LentSlot};
use super::snapshot_slot::SnapshotSlot;
use crate::daemon::{DaemonAttachment, DaemonStreamMessage};
use crate::terminal::output_hub::HistorySegment;
use crate::terminal::preferences::cached_terminal_preferences;

pub const NATIVE_TERMINAL_TITLE_EVENT: &str = "native_terminal_title";
pub const NATIVE_TERMINAL_BELL_EVENT: &str = "native_terminal_bell";
pub const NATIVE_TERMINAL_AGENT_STATE_EVENT: &str = "native_terminal_agent_state";

/// Marks a state the agent reported about itself through the Ferryx extension. Such a report is
/// authoritative: screen rules only infer, so once a session speaks for itself the inferred
/// source must never overwrite it.
pub const AGENT_EXTENSION_MANIFEST_ID: &str = "ferryx-extension";

/// Rows of slack below which the viewport counts as bottom-locked: trackpad inertia
/// routinely parks the viewport 1-2 rows short of `.active`, and a resize there must
/// re-lock to the bottom rather than pin the near-bottom row.
const BOTTOM_LOCK_TOLERANCE_ROWS: u64 = 2;
/// Minimum spacing between agent screen detections while a pane is attached and streaming.
const AGENT_DETECT_INTERVAL_ATTACHED: std::time::Duration = std::time::Duration::from_millis(50);
/// Backgrounded panes only need state transitions (not frames), so detection can be coarser
/// while still bounding CPU under a chatty background pane (perf audit finding NT-02).
const AGENT_DETECT_INTERVAL_DETACHED: std::time::Duration = std::time::Duration::from_millis(250);
/// How long the pump waits for the next output chunk before treating a burst as drained and
/// re-running any detection the throttle skipped (trailing-edge detect).
const AGENT_DETECT_TRAILING_IDLE: std::time::Duration = std::time::Duration::from_millis(60);
/// A remote session refills its local output hub *after* attach, so its retained scrollback
/// arrives as a burst of streamed frames instead of the single history blob a local attach feeds
/// through `feed_attachment_history`. Painting each frame makes the pane visibly scroll from the
/// top of the scrollback down to the end, so the burst is absorbed instead: frames are fed to the
/// grid without scheduling a paint, and one bottom-locked paint follows once the burst drains.
const REPLAY_ABSORB_IDLE: std::time::Duration = std::time::Duration::from_millis(120);
/// Upper bound on absorbing, so a session that keeps producing output cannot stay unpainted.
const REPLAY_ABSORB_MAX: std::time::Duration = std::time::Duration::from_secs(3);

/// Whether the post-attach replay burst is still being absorbed. The window closes on the first
/// gap longer than [`REPLAY_ABSORB_IDLE`], or at [`REPLAY_ABSORB_MAX`] for a stream that never
/// goes idle.
fn replay_absorb_continues(
    since_attach: std::time::Duration,
    since_last_output: std::time::Duration,
) -> bool {
    since_attach < REPLAY_ABSORB_MAX && since_last_output < REPLAY_ABSORB_IDLE
}
pub const NATIVE_TERMINAL_SCROLLBAR_EVENT: &str = "native_terminal_scrollbar";
pub const NATIVE_TERMINAL_FOCUS_EVENT: &str = "native_terminal_focus";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeTerminalFocusPayload {
    pub session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeTerminalTitlePayload {
    pub session_id: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeTerminalBellPayload {
    pub session_id: String,
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeTerminalAgentStatePayload {
    pub session_id: String,
    pub state: String,
    pub rule_id: String,
    pub manifest_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_session: Option<crate::daemon::protocol::AgentProviderSession>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_snapshot: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeTerminalScrollbarPayload {
    pub session_id: String,
    pub total: u64,
    pub offset: u64,
    pub len: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeTerminalEvent {
    Title(NativeTerminalTitlePayload),
    Bell(NativeTerminalBellPayload),
    AgentState(NativeTerminalAgentStatePayload),
    Scrollbar(NativeTerminalScrollbarPayload),
}

pub type NativeTerminalEventSink = Arc<dyn Fn(NativeTerminalEvent) + Send + Sync>;
pub type NativeTerminalPtyResizeSink = Arc<dyn Fn(&str, u16, u16) + Send + Sync>;

fn validate_session_id(session_id: &str) -> Result<(), NativeTerminalError> {
    if session_id.trim().is_empty() {
        return Err(NativeTerminalError::InvalidValue(
            "Native terminal session ID must not be empty".into(),
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub struct NativeTerminalBoundsRequest {
    pub session_id: String,
    pub bounds: LogicalBounds,
}

impl NativeTerminalBoundsRequest {
    pub fn layout(
        &self,
        cell_metrics: CellMetrics,
    ) -> Result<SurfaceCompositionLayout, NativeTerminalError> {
        validate_session_id(&self.session_id)?;
        panic_free_layout(&self.bounds, &cell_metrics)
    }
}

fn panic_free_layout(
    bounds: &LogicalBounds,
    cell_metrics: &CellMetrics,
) -> Result<SurfaceCompositionLayout, NativeTerminalError> {
    SurfaceCompositionLayout::compute(bounds, cell_metrics)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NativeTerminalSurfaceReceipt {
    pub presented: bool,
    pub render_deferred: bool,
    /// The surface is not visible; no retry may be armed until visibility returns.
    pub render_suspended: bool,
    pub cols: u16,
    pub rows: u16,
    pub rebuilt_rows: u16,
    pub reused_rows: u16,
    pub cursor_col: u16,
    pub cursor_row: u16,
    pub cell_width_px: u32,
    pub cell_height_px: u32,
    pub effective_scale_factor: Option<f64>,
}

impl NativeTerminalSurfaceReceipt {
    fn from_snapshot(
        layout: SurfaceCompositionLayout,
        snapshot: &RenderSnapshot,
        rebuilt_rows: u16,
        reused_rows: u16,
        cell_metrics: CellMetrics,
        logical_bounds: Option<LogicalBounds>,
    ) -> Self {
        Self {
            presented: false,
            render_deferred: false,
            render_suspended: false,
            cols: layout.cols,
            rows: layout.rows,
            rebuilt_rows,
            reused_rows,
            cursor_col: snapshot.cursor.x,
            cursor_row: snapshot.cursor.y,
            cell_width_px: cell_metrics.width_px,
            cell_height_px: cell_metrics.height_px,
            effective_scale_factor: logical_bounds.map(|bounds| bounds.scale_factor),
        }
    }
}

/// Coordinates render scheduling to coalesce rapid bursts of terminal updates
/// into at most one pending main-thread render pass.
const RENDER_IDLE: u8 = 0;
const RENDER_SCHEDULED: u8 = 1;
const RENDERING: u8 = 2;
const RENDER_FOLLOW_UP: u8 = 3;

#[derive(Debug, Default)]
pub struct RenderScheduleCoordinator {
    state: AtomicU8,
    frame_clock: FrameClock,
    ownership: Mutex<u64>,
    #[cfg(feature = "local-split-qa")]
    presentation_claim: Mutex<Option<(String, String)>>,
}

impl RenderScheduleCoordinator {
    pub fn new() -> Self {
        Self {
            state: AtomicU8::new(RENDER_IDLE),
            frame_clock: FrameClock::default(),
            ownership: Mutex::new(0),
            #[cfg(feature = "local-split-qa")]
            presentation_claim: Mutex::new(None),
        }
    }

    /// How long the caller must wait before re-dispatching a frame that failed to present.
    pub fn delay_before_retry(&self) -> std::time::Duration {
        self.frame_clock
            .delay_before_retry(std::time::Instant::now())
    }

    /// Attempts to schedule a render pass.
    ///
    /// Returns `true` if this transition successfully scheduled the render (transitioning
    /// from idle to pending). Output arriving during an active frame marks one coalesced follow-up.
    pub fn schedule_render(&self) -> bool {
        loop {
            let state = self.state.load(Ordering::SeqCst);
            match state {
                RENDER_IDLE => {
                    let mut ownership = self.ownership.lock();
                    if self
                        .state
                        .compare_exchange(
                            RENDER_IDLE,
                            RENDER_SCHEDULED,
                            Ordering::SeqCst,
                            Ordering::SeqCst,
                        )
                        .is_ok()
                    {
                        *ownership = ownership.wrapping_add(1);
                        return true;
                    }
                }
                RENDERING => {
                    if self
                        .state
                        .compare_exchange(
                            RENDERING,
                            RENDER_FOLLOW_UP,
                            Ordering::SeqCst,
                            Ordering::SeqCst,
                        )
                        .is_ok()
                    {
                        return false;
                    }
                }
                RENDER_SCHEDULED | RENDER_FOLLOW_UP => return false,
                _ => unreachable!("invalid render coordinator state"),
            }
        }
    }

    /// Marks a scheduled frame as actively rendering without clearing its pending state.
    pub fn begin_render(&self) -> bool {
        let began = self
            .state
            .compare_exchange(
                RENDER_SCHEDULED,
                RENDERING,
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .is_ok();
        if began {
            self.frame_clock
                .mark_frame_started(std::time::Instant::now());
        }
        began
    }

    /// Completes the active frame.
    ///
    /// Returns `true` when output arrived during rendering and one follow-up frame must run.
    pub fn finish_render(&self) -> bool {
        loop {
            let state = self.state.load(Ordering::SeqCst);
            let (next, follow_up) = match state {
                RENDERING => (RENDER_IDLE, false),
                RENDER_FOLLOW_UP => (RENDER_SCHEDULED, true),
                _ => return false,
            };
            if self
                .state
                .compare_exchange(state, next, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return follow_up;
            }
        }
    }

    /// Cancels all scheduled or active render work, transitioning back to idle.
    ///
    /// Returns `true` if work was cancelled, or `false` if the coordinator was already idle.
    pub fn consume_render(&self) -> bool {
        let mut ownership = self.ownership.lock();
        *ownership = ownership.wrapping_add(1);
        self.state.swap(RENDER_IDLE, Ordering::SeqCst) != RENDER_IDLE
    }

    fn begin_owned_render(&self) -> Option<u64> {
        let ownership = self.ownership.lock();
        self.begin_render().then_some(*ownership)
    }

    // Lifecycle cancellation and late GPU callbacks share this lock so an old frame cannot
    // release, finish, or retry a newer attachment's work, including an already active frame.
    fn abandon_render(&self, owner: u64) -> bool {
        let ownership = self.ownership.lock();
        if *ownership != owner {
            return false;
        }
        self.state.swap(RENDER_IDLE, Ordering::SeqCst) != RENDER_IDLE
    }

    fn finish_owned_render(&self, owner: u64, retry: bool) -> bool {
        let mut ownership = self.ownership.lock();
        if *ownership != owner {
            return false;
        }
        loop {
            let state = self.state.load(Ordering::SeqCst);
            let (next, follow_up) = match state {
                RENDERING if retry => (RENDER_SCHEDULED, true),
                RENDERING => (RENDER_IDLE, false),
                RENDER_FOLLOW_UP => (RENDER_SCHEDULED, true),
                _ => return false,
            };
            if self
                .state
                .compare_exchange(state, next, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                if follow_up {
                    *ownership = ownership.wrapping_add(1);
                }
                return follow_up;
            }
        }
    }

    /// Returns `true` if a render pass is scheduled, active, or awaiting a follow-up.
    pub fn is_render_pending(&self) -> bool {
        self.state.load(Ordering::SeqCst) != RENDER_IDLE
    }
}

#[cfg(test)]
mod render_schedule_coordinator_tests {
    use super::RenderScheduleCoordinator;

    #[test]
    fn dropped_frame_retries_without_new_output() {
        let coordinator = RenderScheduleCoordinator::new();
        assert!(coordinator.schedule_render());
        let owner = coordinator.begin_owned_render().unwrap();

        assert!(coordinator.finish_owned_render(owner, true));

        assert!(coordinator.begin_owned_render().is_some());
    }

    #[test]
    fn cancelled_geometry_frame_preserves_requested_follow_up() {
        let coordinator = RenderScheduleCoordinator::new();
        assert!(coordinator.schedule_render());
        let owner = coordinator.begin_owned_render().unwrap();
        assert!(!coordinator.schedule_render());

        assert!(coordinator.finish_owned_render(owner, false));

        assert!(coordinator.begin_owned_render().is_some());
    }

    #[test]
    fn retired_completion_cannot_finish_or_retry_the_live_frame() {
        let coordinator = RenderScheduleCoordinator::new();
        assert!(coordinator.schedule_render());
        let retired = coordinator.begin_owned_render().unwrap();
        coordinator.consume_render();
        assert!(coordinator.schedule_render());
        let current = coordinator.begin_owned_render().unwrap();

        assert!(!coordinator.finish_owned_render(retired, true));

        assert!(coordinator.is_render_pending());
        assert!(!coordinator.finish_owned_render(current, false));
        assert!(!coordinator.is_render_pending());
    }

    #[test]
    fn retired_frame_does_not_cancel_a_new_attachment_already_rendering() {
        // Given an old GPU frame still outstanding across detach and reattach.
        let coordinator = RenderScheduleCoordinator::new();
        assert!(coordinator.schedule_render());
        let retired = coordinator.begin_owned_render().unwrap();
        coordinator.consume_render();
        assert!(coordinator.schedule_render());
        assert!(coordinator.begin_render());
        assert!(!coordinator.schedule_render());

        // When the old frame discovers that its attachment was retired.
        coordinator.abandon_render(retired);

        // Then the live frame still owns its requested follow-up.
        assert!(coordinator.finish_render());
        assert!(coordinator.begin_render());
    }

    #[test]
    fn a_retired_frame_must_not_cancel_the_next_attachments_scheduled_render() {
        // The corruption after a worktree switch away and back. The coordinator is owned by the
        // session, not by the surface, so it is the same object across both attachments.
        let coordinator = RenderScheduleCoordinator::new();

        // Attachment 1 has a frame in flight on the GPU worker.
        assert!(coordinator.schedule_render());
        let retired = coordinator.begin_owned_render().unwrap();

        // Switching away detaches the pane: the lifecycle event cancels attachment 1's work.
        assert!(coordinator.consume_render());

        // Switching back re-attaches and schedules the new attachment's first frame.
        assert!(
            coordinator.schedule_render(),
            "the reattached pane must be able to schedule its first frame"
        );

        // Only now does the GPU worker notice the frame it still holds belongs to the attachment
        // that is gone, and abandons it.
        assert!(
            !coordinator.abandon_render(retired),
            "a frame the coordinator no longer owns must release nothing"
        );

        // The new attachment's frame must survive that, or nothing ever paints it.
        assert!(
            coordinator.begin_render(),
            "a retired frame must not erase the schedule a live attachment is waiting on"
        );
    }

    #[test]
    fn abandoning_a_frame_returns_the_coordinator_to_idle() {
        let coordinator = RenderScheduleCoordinator::new();
        assert!(coordinator.schedule_render());
        let owner = coordinator.begin_owned_render().unwrap();
        assert!(coordinator.abandon_render(owner));
        assert!(
            !coordinator.is_render_pending(),
            "an abandoned frame must not leave the coordinator stuck mid-render"
        );
        assert!(
            coordinator.schedule_render(),
            "a released coordinator must accept the next frame"
        );
    }

    #[test]
    fn abandoning_a_frame_drops_its_own_pending_follow_up() {
        // Output that arrived during the abandoned frame belongs to it, not to a later
        // generation, so it is released with the frame rather than left dangling as a
        // RENDER_SCHEDULED that no dispatch is waiting to pick up.
        let coordinator = RenderScheduleCoordinator::new();
        assert!(coordinator.schedule_render());
        let owner = coordinator.begin_owned_render().unwrap();
        assert!(!coordinator.schedule_render(), "marks a coalesced follow-up");
        assert!(coordinator.abandon_render(owner));
        assert!(!coordinator.is_render_pending());
    }
}

pub struct NativeTerminalSession {
    pub terminal: NativeTerminal,
    pub focused: bool,
    pub preedit: Option<String>,
    pub layout: Option<SurfaceCompositionLayout>,
    pub logical_bounds: Option<LogicalBounds>,
    pub cell_metrics: Option<CellMetrics>,
    pub stream_task: Option<tokio::task::JoinHandle<()>>,
    pub pump_task: Option<tokio::task::JoinHandle<()>>,
    pub pty_write_task: Option<tokio::task::JoinHandle<()>>,
    pub is_remote: bool,
    pub remote_generation: Option<u64>,
    pub last_sequence: Option<u64>,
    /// Daemon epoch the applied sequences belong to. Sequence numbers are only comparable within
    /// one epoch, so a changed epoch invalidates `last_sequence` entirely rather than lowering it.
    pub daemon_epoch: u64,
    /// Incremented by every attach. The pump captures its value at spawn and stops applying output
    /// once it no longer matches, so a pump aborted mid-message cannot advance `last_sequence`
    /// past the replay cursor the new attach just established.
    pub pump_generation: u64,
    /// Active 7-field attach tuple binding frontend/pane/binding/attempt to authoritative backend/incarnation/epoch.
    pub active_attach_tuple: Option<crate::daemon::protocol::PaneAttachTuple>,
    /// Attempt generation of the active attachment. Monotonically checked so older completions cannot overwrite newer streams.
    pub attempt_generation: u64,
    /// Incarnation identifier transferred unchanged across handovers.
    pub incarnation: Option<String>,
    pub update_sender: tokio::sync::watch::Sender<()>,
    detach_sender: tokio::sync::watch::Sender<()>,
    pub render_coordinator: Arc<RenderScheduleCoordinator>,
    pub last_agent_activity: Option<crate::agent_detect::AgentActivity>,
    /// Last provider session reported by the agent extension. An agent rotates its conversation
    /// id in place (`/new`) without changing its activity state, so the rotation has to be part
    /// of the change test; otherwise the report is swallowed here and the pane keeps resuming
    /// the conversation it was opened with.
    pub last_provider_session: Option<crate::daemon::protocol::AgentProviderSession>,
    pub last_agent_detect_at: Option<std::time::Instant>,
    /// Set when the throttle skipped detection on an output chunk; the pump re-runs a forced
    /// detection once the burst drains so the trailing frame still produces transitions.
    pub agent_detect_pending: bool,
    pub last_scrollbar: Option<NativeTerminalScrollbarPayload>,
    pub scrollbar_overlay: ScrollbarOverlayState,
    pub attention_frame: bool,
    /// Set while screen inference is suppressed for this session because a more authoritative
    /// producer owns its state: the agent's own extension reports or a daemon process release
    /// whose leftover screen would otherwise resurrect the activity that just ended. A new
    /// agent process or an explicit manual reset re-enables screen inference.
    pub agent_reports_own_state: bool,
    /// Whether bracketed paste mode (DEC mode 2004) has ever been enabled on this session,
    /// preserved across terminal resets and history re-feeds.
    pub bracketed_paste_seen: bool,
    /// Whether a frontend pane currently owns a compositor surface for this session.
    ///
    /// A session outlives its surface: [`NativeTerminalSurfaceHostState::detach_session`] keeps the
    /// daemon pump running for a backgrounded agent while releasing the GPU host. Geometry updates
    /// that were already in flight when that happened must not resurrect a surface for a pane that
    /// is no longer on screen, so they are rejected with
    /// [`NativeTerminalError::SessionDetached`] instead.
    pub surface_attached: bool,
    pub output_stream_ended: bool,
    pub snapshot_slot: Arc<SnapshotSlot>,
}

impl NativeTerminalSession {
    fn owns_replay_request(&self, token: &ReplayRequestToken) -> bool {
        self.pump_generation == token.generation
            && self.active_attach_tuple.as_ref() == token.attach_tuple.as_ref()
            && token.attempt_generation.map_or(true, |generation| self.attempt_generation == generation)
    }

    fn presentation_tuple(&self) -> Option<&crate::daemon::protocol::PaneAttachTuple> {
        self.active_attach_tuple.as_ref()
    }

    pub fn publish_frame(&self) {
        if let (Some(layout), Some(logical_bounds)) = (self.layout, self.logical_bounds) {
            if let Ok(input) = session_render_snapshot(self) {
                // The frame covers exactly the output this session had already
                // applied; that is the coverage the private QA frame evidence
                // reports, so it is published with the frame.
                self.snapshot_slot.publish(
                    layout,
                    logical_bounds,
                    input,
                    self.active_attach_tuple.clone(),
                    self.last_sequence,
                );
            }
        }
    }
}

pub struct NativeTerminalSurfaceHostState {
    // Lock hosts before sessions and keep it through ownership checks and presentation.
    hosts: Arc<Mutex<HashMap<String, NativeTerminalSurfaceHost>>>,
    sessions: Arc<Mutex<HashMap<String, NativeTerminalSession>>>,
    event_sink: Arc<RwLock<Option<NativeTerminalEventSink>>>,
    pty_resize_sink: Arc<RwLock<Option<NativeTerminalPtyResizeSink>>>,
    pending_startups: Arc<Mutex<HashSet<String>>>,
    gpu_worker: Arc<GpuWorker>,
    /// True while a DOM overlay (dialog or search surface) owns input over the
    /// terminal area. macOS keeps surfaces attached underneath such overlays, so
    /// the native scroll-wheel monitor must let the event reach WebKit then.
    dom_overlay_owns_input: Arc<AtomicBool>,
}

impl NativeTerminalSurfaceHostState {
    pub fn gpu_worker(&self) -> &Arc<GpuWorker> {
        &self.gpu_worker
    }

    /// Records whether a DOM overlay currently owns input over the terminal area.
    ///
    /// The flag is shared by every clone of this state so the AppKit wheel monitor
    /// observes the frontend's current owner instead of its own copy.
    pub fn set_dom_overlay_owns_input(&self, owns_input: bool) {
        self.dom_overlay_owns_input.store(owns_input, Ordering::Relaxed);
    }

    pub fn dom_overlay_owns_input(&self) -> bool {
        self.dom_overlay_owns_input.load(Ordering::Relaxed)
    }

    pub fn session_snapshot_slot(&self, session_id: &str) -> Option<Arc<SnapshotSlot>> {
        self.sessions.lock().get(session_id).map(|s| Arc::clone(&s.snapshot_slot))
    }

    /// Marks a session as freshly spawned so its initial startup VT queries (e.g. CPR) are preserved.
    pub fn mark_pending_startup(&self, session_id: &str) {
        self.pending_startups.lock().insert(session_id.to_string());
    }

    /// Checks if a session has an unconsumed pending startup marker.
    pub fn is_pending_startup(&self, session_id: &str) -> bool {
        self.pending_startups.lock().contains(session_id)
    }

    /// Consumes the pending startup marker if present.
    pub fn consume_pending_startup(&self, session_id: &str) -> bool {
        self.pending_startups.lock().remove(session_id)
    }

    /// Clears any pending startup markers for a session.
    pub fn clear_pending_session(&self, session_id: &str) {
        self.pending_startups.lock().remove(session_id);
    }
}

/// Cross-lane frame-submission evidence. The terminal layer owns the
/// `marker-output` receipt and reads this sidecar (its own `FRAME_SUBMITTED_FILE`
/// in `terminal/qa_liveness.rs`); the native lane is its only writer.
#[cfg(feature = "local-split-qa")]
const QA_FRAME_SUBMITTED_FILE: &str = "frame-submitted.jsonl";

/// Receipt name the runner awaits when an older attempt is fenced off.
#[cfg(feature = "local-split-qa")]
pub const STALE_RECEIPT_REJECTED_BARRIER: &str = "stale-receipt-rejected";

/// Receipt name the runner awaits when a legitimate reattach reuses the backend.
#[cfg(feature = "local-split-qa")]
pub const REATTACH_MARKER_BARRIER: &str = "reattach-marker";

/// Runner->product control that asks for a stale attempt against the live binding
/// (`barrierHub.command('trigger-stale-binding', ...)` in
/// `scripts/lib/qa-scenarios/lifecycle-scenarios.mjs`). The command names the
/// identity field the stale attempt must differ in.
#[cfg(feature = "local-split-qa")]
const TRIGGER_STALE_BINDING: &str = "trigger-stale-binding";

/// Identity fields a stale attempt can really be rejected on by the REGISTRATION
/// fence, i.e. exactly the fields `pane_liveness_registration_matches` compares.
/// `incarnation`/`backendSessionId` are refused earlier by the detach guard (which
/// settles nothing) and `daemonEpoch` is not compared by that fence at all, so a
/// command naming one of those is reported instead of being simulated.
#[cfg(feature = "local-split-qa")]
const STALE_BINDING_MUTABLE_FIELDS: [&str; 4] = [
    "attemptGeneration",
    "frontendSessionId",
    "paneIdentity",
    "bindingKey",
];

/// Bounded wait for the released frame's real presentation. The settlement
/// subscribes to the slot's presentation channel BEFORE the release-driven
/// dispatch and then awaits that signal, so the receipt reports a presentation
/// that really happened instead of racing one.
#[cfg(feature = "local-split-qa")]
const PRESENTATION_SETTLE_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(2_000);

/// Missing-field list for an honestly partial liveness snapshot. Mirrors
/// `QaBarrierChannel`'s own `evidence_missing_fields` (private to
/// `ipc/qa_barrier.rs`): an `Unknown` verdict is reported with the exact fields
/// this layer could not observe instead of a forced verdict.
#[cfg(feature = "local-split-qa")]
fn qa_missing_liveness_fields(
    snapshot: &crate::ipc::debug::PaneLivenessSnapshot,
) -> Vec<&'static str> {
    let mut missing = Vec::new();
    if snapshot.daemon_epoch.is_none() {
        missing.push("daemonEpoch");
    }
    if snapshot.reader_paused.is_none() {
        missing.push("readerPaused");
    }
    if snapshot.kernel_stopped.is_none() {
        missing.push("kernelStopped");
    }
    if snapshot.suspended.is_none() {
        missing.push("suspended");
    }
    if snapshot.presentation_receipt_received.is_none() {
        missing.push("presentationReceiptReceived");
    }
    missing
}

/// The liveness snapshot the render coordinator can honestly attest: session
/// identity plus the REAL pending state of its own coordinator. The daemon-owned
/// suspension/reader observations are not reachable from here and stay `None`,
/// which the classifier reports as `Unknown` with `evidenceMissing` instead of
/// as a positive recovery.
#[cfg(feature = "local-split-qa")]
fn qa_coordinator_snapshot(
    session_id: &str,
    render_pending: bool,
) -> crate::ipc::debug::PaneLivenessSnapshot {
    crate::ipc::debug::PaneLivenessSnapshot {
        telemetry_available: true,
        session_id: Some(session_id.to_string()),
        vt_session_id: Some(session_id.to_string()),
        has_unpresented_frames: Some(render_pending),
        ..Default::default()
    }
}

/// Which field of the seven-field identity really diverged, so a rejection
/// reason names the field that failed instead of a generic message.
#[cfg(feature = "local-split-qa")]
fn qa_tuple_mismatch_field(
    stale: Option<&crate::daemon::protocol::PaneAttachTuple>,
    active: Option<&crate::daemon::protocol::PaneAttachTuple>,
) -> &'static str {
    let (Some(stale), Some(active)) = (stale, active) else {
        return "attachTuple";
    };
    if stale.backend_session_id != active.backend_session_id {
        return "backendSessionId";
    }
    if stale.incarnation != active.incarnation {
        return "incarnation";
    }
    if stale.daemon_epoch != active.daemon_epoch {
        return "daemonEpoch";
    }
    if stale.frontend_session_id != active.frontend_session_id {
        return "frontendSessionId";
    }
    if stale.pane_identity != active.pane_identity {
        return "paneIdentity";
    }
    if stale.binding_key != active.binding_key {
        return "bindingKey";
    }
    if stale.attempt_generation != active.attempt_generation {
        return "attemptGeneration";
    }
    "attachTuple"
}

/// The fields `pane_liveness_registration_matches` actually fences, so a
/// registration rejection names the field that really failed it.
#[cfg(feature = "local-split-qa")]
fn qa_registration_mismatch_field(
    active: &crate::daemon::protocol::PaneAttachTuple,
    incoming: &crate::daemon::protocol::PaneAttachTuple,
) -> &'static str {
    if active.backend_session_id != incoming.backend_session_id {
        return "backendSessionId";
    }
    if active.incarnation != incoming.incarnation {
        return "incarnation";
    }
    if active.frontend_session_id != incoming.frontend_session_id {
        return "frontendSessionId";
    }
    if active.pane_identity != incoming.pane_identity {
        return "paneIdentity";
    }
    if active.binding_key != incoming.binding_key {
        return "bindingKey";
    }
    "attemptGeneration"
}

/// `presentation` barrier, positive branch: the product's OWN
/// `PanePresentationReceipt` is emitted verbatim.
///
/// The runner's `requireSevenTupleReceipt` reads the authoritative seven-field
/// identity off `attachTuple`, so the real value the product already sends to
/// the frontend is the only payload that can satisfy it; nothing here is
/// synthesized, and a frame without an attach tuple emits nothing at all.
#[cfg(feature = "local-split-qa")]
fn emit_native_presentation_receipt_qa(
    channel: &std::sync::Arc<crate::ipc::qa_barrier::QaBarrierChannel>,
    session_id: &str,
    frame_generation: u64,
    frame_epoch: u64,
    receipt: &NativeTerminalSurfaceReceipt,
    presentation_receipt: &crate::daemon::protocol::PanePresentationReceipt,
) {
    // No correlation identity means no correlatable line: refuse to write one
    // rather than invent an operation nonce the runner would reject.
    let Some(operation_id) = channel
        .spec(crate::ipc::qa_barrier::PRESENTATION_BARRIER)
        .map(|s| s.operation_id)
        .or_else(|| channel.operation_id())
    else {
        return;
    };
    let mut payload = serde_json::to_value(presentation_receipt).unwrap_or_default();
    let Some(fields) = payload.as_object_mut() else {
        return;
    };
    // `attachTuple` and `presented` are the real receipt's own values; the rest
    // is the render observation that accompanied the accepted presentation.
    fields.insert("sessionId".into(), serde_json::json!(session_id));
    fields.insert(
        "stage".into(),
        serde_json::json!("native_presentation_settled"),
    );
    fields.insert(
        "frameGeneration".into(),
        serde_json::json!(frame_generation),
    );
    fields.insert("frameEpoch".into(), serde_json::json!(frame_epoch));
    fields.insert("frameSubmitted".into(), serde_json::json!(true));
    fields.insert("cols".into(), serde_json::json!(receipt.cols));
    fields.insert("rows".into(), serde_json::json!(receipt.rows));
    fields.insert("rebuiltRows".into(), serde_json::json!(receipt.rebuilt_rows));
    fields.insert("reusedRows".into(), serde_json::json!(receipt.reused_rows));
    fields.insert("cursorCol".into(), serde_json::json!(receipt.cursor_col));
    fields.insert("cursorRow".into(), serde_json::json!(receipt.cursor_row));
    fields.insert(
        "cellWidthPx".into(),
        serde_json::json!(receipt.cell_width_px),
    );
    fields.insert(
        "cellHeightPx".into(),
        serde_json::json!(receipt.cell_height_px),
    );
    fields.insert(
        "effectiveScaleFactor".into(),
        serde_json::json!(receipt.effective_scale_factor),
    );
    fields.insert(
        "presentationEvidence".into(),
        serde_json::json!("native-presented"),
    );
    fields.insert(
        "producerComponent".into(),
        serde_json::json!("surface-host-gpu-render"),
    );
    schedule_native_qa_receipt(
        channel,
        crate::ipc::qa_barrier::PRESENTATION_BARRIER,
        &operation_id,
        payload,
    );
}

/// The stale attempt the runner asked for: the live binding with exactly the
/// named identity field changed. `None` for a field the registration fence does
/// not compare, or for a generation that cannot be made strictly older.
#[cfg(feature = "local-split-qa")]
fn mutate_attach_tuple_field(
    active: &crate::daemon::protocol::PaneAttachTuple,
    field: &str,
) -> Option<crate::daemon::protocol::PaneAttachTuple> {
    let mut stale = active.clone();
    match field {
        // The fence requires the incoming generation to continue the live one; an
        // older generation is exactly the stale attempt the scenario means.
        "attemptGeneration" => {
            stale.attempt_generation = active.attempt_generation.checked_sub(1)?;
        }
        "frontendSessionId" => {
            stale.frontend_session_id = format!("{}-stale", active.frontend_session_id);
        }
        "paneIdentity" => {
            stale.pane_identity = format!("{}-stale", active.pane_identity);
        }
        "bindingKey" => {
            stale.binding_key = format!("{}-stale", active.binding_key);
        }
        _ => return None,
    }
    Some(stale)
}

/// `presentation` barrier, released branch: the SAME owned frame was handed to
/// the real coordinator path. Written only after that frame's real presentation
/// was observed (the watch is subscribed before the dispatch), and it reports
/// the coordinator's measured pending state, the frame's own presentation receipt
/// and the daemon's reader/kernel facts that were really observed. Those daemon
/// facts are what a positive recovery verdict rests on: a fact the daemon did not
/// report stays unobserved, which keeps the verdict `Unknown` with
/// `evidenceMissing` instead of turning a missing observation into a pass.
#[cfg(feature = "local-split-qa")]
fn emit_presentation_released_settlement_qa(
    channel: &Arc<crate::ipc::qa_barrier::QaBarrierChannel>,
    session_id: &str,
    operation_id: &str,
    coordinator: &RenderScheduleCoordinator,
    frame_presented: bool,
    daemon_facts: Option<&crate::ipc::qa_barrier::QaDaemonLiveness>,
) {
    use crate::ipc::debug::{classify_pane_liveness, PaneLivenessVerdict};
    use crate::ipc::qa_barrier::verdict_str;
    let render_pending_after = coordinator.is_render_pending();
    let mut snapshot = qa_coordinator_snapshot(session_id, render_pending_after);
    // The presentation watch was subscribed BEFORE the release-driven dispatch, so
    // whether THIS frame's own receipt arrived is a real observation too.
    snapshot.presentation_receipt_received = Some(frame_presented);
    if let Some(facts) = daemon_facts {
        facts.apply(&mut snapshot);
    }
    let verdict = classify_pane_liveness(&snapshot);
    let missing = qa_missing_liveness_fields(&snapshot);
    channel.append_receipt(
        crate::ipc::qa_barrier::PRESENTATION_BARRIER,
        operation_id,
        serde_json::json!({
            "sessionId": session_id,
            "stage": "presentation_frame_settled",
            "classifierVerdict": verdict_str(verdict),
            "evidenceMissing": verdict == PaneLivenessVerdict::Unknown,
            "evidenceMissingFields": missing,
            "stageProgress": {
                "frameConsumed": !render_pending_after,
                "renderPendingAfter": render_pending_after,
                "releaseOutcome": "released",
            },
            "framePresented": frame_presented,
            "daemonFacts": daemon_facts
                .map(|facts| serde_json::to_value(facts).unwrap_or(serde_json::Value::Null)),
            "snapshot": serde_json::to_value(&snapshot).unwrap_or(serde_json::Value::Null),
            "coordinatorEvidence": "coordinator-consumed",
            "presentationEvidence": "coordinator-consumed",
            "producerComponent": NativeTerminalSurfaceHostState::PRESENTATION_PRODUCER_ID,
        }),
    );
}

/// `stale-receipt-rejected`: an older attempt's completion or install was
/// refused by the binding fence. The reason names the real divergence.
#[cfg(feature = "local-split-qa")]
fn emit_stale_receipt_rejected_qa(
    channel: &Arc<crate::ipc::qa_barrier::QaBarrierChannel>,
    session_id: &str,
    reason: String,
    mismatched_field: &'static str,
    stale_tuple: Option<&crate::daemon::protocol::PaneAttachTuple>,
    active_tuple: Option<&crate::daemon::protocol::PaneAttachTuple>,
) {
    // A scenario that pre-arms no barrier still settles a correlatable line, so
    // the operation nonce comes from the private env here; without one no line
    // is written at all.
    let Some(operation_id) = channel.operation_id() else {
        return;
    };
    schedule_native_qa_receipt(
        channel,
        STALE_RECEIPT_REJECTED_BARRIER,
        &operation_id,
        serde_json::json!({
            "sessionId": session_id,
            "backendSessionId": stale_tuple
                .or(active_tuple)
                .map(|tuple| tuple.backend_session_id.clone()),
            "stage": "stale_receipt_rejected",
            "rejected": true,
            "reason": reason,
            "mismatchedField": mismatched_field,
            "attemptGeneration": stale_tuple.map(|tuple| tuple.attempt_generation),
            "staleAttachTuple": stale_tuple,
            "activeAttachTuple": active_tuple,
            "producerComponent": "surface-host-render-coordinator",
        }),
    );
}

/// `stale-receipt-rejected` for a delayed POSITIVE completion whose frame is no
/// longer the current submission: the render-path half of the fence.
#[cfg(feature = "local-split-qa")]
fn emit_stale_completion_rejected_qa(
    channel: &Arc<crate::ipc::qa_barrier::QaBarrierChannel>,
    session_id: &str,
    current_generation: Option<u64>,
    current_tuple: Option<&crate::daemon::protocol::PaneAttachTuple>,
    frame_generation: u64,
    stale_tuple: Option<&crate::daemon::protocol::PaneAttachTuple>,
) {
    let (reason, field) = match current_generation {
        None => (
            "delayed positive receipt rejected: no live submission remains for the session"
                .to_string(),
            "frameGeneration",
        ),
        Some(generation) if generation != frame_generation => (
            format!(
                "delayed positive receipt rejected: frame generation {frame_generation} superseded by live generation {generation}"
            ),
            "frameGeneration",
        ),
        Some(_) => {
            let field = qa_tuple_mismatch_field(stale_tuple, current_tuple);
            (
                format!("delayed positive receipt rejected by the binding fence: {field} mismatch"),
                field,
            )
        }
    };
    emit_stale_receipt_rejected_qa(
        channel,
        session_id,
        reason,
        field,
        stale_tuple,
        current_tuple,
    );
}

/// `stale-receipt-rejected` for an older ATTEMPT that cannot install itself over
/// the live binding: the registration-path half of the fence.
#[cfg(feature = "local-split-qa")]
fn emit_stale_registration_rejected_qa(
    session_id: &str,
    active: Option<&crate::daemon::protocol::PaneAttachTuple>,
    incoming: &crate::daemon::protocol::PaneAttachTuple,
) {
    let Some(channel) = crate::ipc::qa_barrier::active_channel() else {
        return;
    };
    let field = match active {
        Some(active) => qa_registration_mismatch_field(active, incoming),
        None => "attemptGeneration",
    };
    emit_stale_receipt_rejected_qa(
        &channel,
        session_id,
        format!(
            "registration rejected: attempt generation {} does not continue the live binding ({field} mismatch)",
            incoming.attempt_generation
        ),
        field,
        Some(incoming),
        active,
    );
}

/// `reattach-marker`: a legitimate reattach reinstalled the surface and stream
/// for the SAME backend, and the live binding is reported verbatim.
#[cfg(feature = "local-split-qa")]
fn emit_reattach_marker_qa(
    channel: &Arc<crate::ipc::qa_barrier::QaBarrierChannel>,
    session_id: &str,
    tuple: &crate::daemon::protocol::PaneAttachTuple,
    bounds_applied: bool,
) {
    let Some(operation_id) = channel.operation_id() else {
        return;
    };
    schedule_native_qa_receipt(
        channel,
        REATTACH_MARKER_BARRIER,
        &operation_id,
        serde_json::json!({
            "sessionId": session_id,
            "backendSessionId": tuple.backend_session_id,
            "incarnation": tuple.incarnation,
            "daemonEpoch": tuple.daemon_epoch,
            "frontendSessionId": tuple.frontend_session_id,
            "paneIdentity": tuple.pane_identity,
            "bindingKey": tuple.binding_key,
            "attemptGeneration": tuple.attempt_generation,
            "attachTuple": tuple,
            "reattached": true,
            // The native reattach reinstalls the surface and stream of an
            // existing backend; this layer owns no PTY, so it created none.
            "newPtyCreated": false,
            "boundsApplied": bounds_applied,
            "producerComponent": "surface-host-reattach",
        }),
    );
}

/// `marker-output`'s `frameSubmitted` half, in the cross-lane contract the
/// terminal layer owns: one correlated JSON object per REAL frame submission,
/// appended to `<barrier dir>/frame-submitted.jsonl`. Its observer only honors a
/// record whose run/operation/session match and whose `coveredSequence` reaches
/// the marker occurrence's own hub sequence, so this is evidence, never a flag.
/// The native lane never writes the `marker-output` receipt itself: the terminal
/// lane owns that file and its line indices.
#[cfg(feature = "local-split-qa")]
fn qa_frame_submission_record(
    run_id: &str,
    operation_id: &str,
    session_id: &str,
    covered_sequence: u64,
) -> serde_json::Value {
    serde_json::json!({
        "runId": run_id,
        "operationId": operation_id,
        "sessionId": session_id,
        "coveredSequence": covered_sequence,
        "producerComponent": "surface-host-gpu-render",
    })
}

#[cfg(feature = "local-split-qa")]
fn append_qa_frame_submission(dir: &std::path::Path, record: &serde_json::Value) -> bool {
    let mut line = record.to_string();
    line.push('\n');
    use std::io::Write as _;
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(QA_FRAME_SUBMITTED_FILE))
    {
        Ok(mut file) => file.write_all(line.as_bytes()).is_ok(),
        Err(_) => false,
    }
}

#[cfg(feature = "local-split-qa")]
fn schedule_native_frame_submitted_qa(
    channel: &Arc<crate::ipc::qa_barrier::QaBarrierChannel>,
    session_id: &str,
    covered_sequence: u64,
) {
    // Read from the env rather than from the channel: the channel deliberately
    // exposes no path accessor, and this is the same value it was built from.
    let Ok(dir) = std::env::var("FERRYX_QA_BARRIER_DIR") else {
        return;
    };
    if dir.trim().is_empty() {
        return;
    }
    let Some(operation_id) = channel.operation_id() else {
        return;
    };
    let record = qa_frame_submission_record(
        channel.run_id(),
        &operation_id,
        session_id,
        covered_sequence,
    );
    let dir = std::path::PathBuf::from(dir);
    tauri::async_runtime::spawn_blocking(move || {
        let _ = append_qa_frame_submission(&dir, &record);
    });
}

#[cfg(feature = "local-split-qa")]
fn schedule_native_qa_receipt(
    channel: &Arc<crate::ipc::qa_barrier::QaBarrierChannel>,
    name: &str,
    operation_id: &str,
    payload: serde_json::Value,
) {
    let channel = Arc::clone(channel);
    let name = name.to_string();
    let operation_id = operation_id.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        channel.append_receipt(&name, &operation_id, payload);
    });
}

fn dispatch_scheduled_render<R: Runtime>(
    window: Window<R>,
    hosts: Arc<Mutex<HashMap<String, NativeTerminalSurfaceHost>>>,
    slot: Arc<SnapshotSlot>,
    session_id: String,
    coordinator: Arc<RenderScheduleCoordinator>,
    gpu_worker: Arc<GpuWorker>,
) {
    let owner = *coordinator.ownership.lock();
    dispatch_owned_render(window, hosts, slot, session_id, coordinator, gpu_worker, owner);
}

fn dispatch_owned_render<R: Runtime>(
    window: Window<R>,
    hosts: Arc<Mutex<HashMap<String, NativeTerminalSurfaceHost>>>,
    slot: Arc<SnapshotSlot>,
    session_id: String,
    coordinator: Arc<RenderScheduleCoordinator>,
    gpu_worker: Arc<GpuWorker>,
    dispatch_owner: u64,
) {
    #[cfg(feature = "local-split-qa")]
    if let Some(channel) = crate::ipc::qa_barrier::active_channel() {
        if let Some(spec) = channel.spec(crate::ipc::qa_barrier::PRESENTATION_BARRIER) {
            // The runner's bind file is read and its bound ack written with
            // synchronous file I/O, so the armed path runs on its own task
            // instead of the runtime worker that drove this frame. The unarmed
            // path (production, and every launch without an armed presentation
            // barrier) still dispatches inline below; the deferral is one task
            // hop, on the QA-armed path only.
            tauri::async_runtime::spawn(dispatch_armed_presentation_barrier(
                window,
                hosts,
                slot,
                session_id,
                coordinator,
                gpu_worker,
                dispatch_owner,
                channel,
                spec,
            ));
            return;
        }
    }

    dispatch_owned_render_inner(
        window,
        hosts,
        slot,
        session_id,
        coordinator,
        gpu_worker,
        dispatch_owner,
    );
}

/// The QA-armed presentation hold, spawned by `dispatch_owned_render` when the
/// runner armed the `presentation` barrier. Adopting the runner's bind is
/// synchronous file I/O, so it must not run on the runtime worker that drove the
/// frame; the body below is the armed branch moved out verbatim.
#[cfg(feature = "local-split-qa")]
#[allow(clippy::too_many_arguments)]
async fn dispatch_armed_presentation_barrier<R: Runtime>(
    window: Window<R>,
    hosts: Arc<Mutex<HashMap<String, NativeTerminalSurfaceHost>>>,
    slot: Arc<SnapshotSlot>,
    session_id: String,
    coordinator: Arc<RenderScheduleCoordinator>,
    gpu_worker: Arc<GpuWorker>,
    dispatch_owner: u64,
    channel: Arc<crate::ipc::qa_barrier::QaBarrierChannel>,
    spec: crate::ipc::qa_barrier::ArmSpec,
) {
    // The runner binds this barrier to the backend session it really
    // created (`bindBackendSession` -> `<name>.bind.json`); no product
    // observation can know that id in advance, so its bind file is the only
    // honest source for the target. A bind that cannot be correlated, or
    // that conflicts with a target the product already bound, is reported
    // instead of being overwritten.
    let mut target_backend_session_id = spec.target_backend_session_id.clone();
    if target_backend_session_id.as_deref().map_or(true, str::is_empty) {
        match crate::ipc::qa_barrier::adopt_runner_bind_off_runtime(
            &channel,
            crate::ipc::qa_barrier::PRESENTATION_BARRIER,
        )
        .await
        {
            Ok(bound) => target_backend_session_id = Some(bound),
            Err(reason) => {
                schedule_native_qa_receipt(
                    &channel,
                    crate::ipc::qa_barrier::PRESENTATION_BARRIER,
                    &spec.operation_id,
                    serde_json::json!({
                        "sessionId": session_id,
                        "backendSessionId": session_id,
                        "stage": "presentation_binding_failed",
                        "status": "failed",
                        "error": format!("BINDING_FAILURE: missing required targetBackendSessionId on armed barrier ({reason})"),
                        "actionable": false,
                        "producerComponent": "surface-host-render-coordinator",
                    }),
                );
                // An armed-but-unbound barrier must not EAT the frame. The pane's
                // very first dispatch always takes this path - the runner can only
                // write `presentation.bind.json` after the pane step has settled the
                // session - and dropping that frame left `frame-submitted.jsonl`
                // ABSENT and the pane with no later dispatch to present or hold, so
                // the run died at `PANE_BINDING_UNBOUND` before the write stage was
                // ever reached. Widening the stage budget to 30 s changed nothing,
                // which is what proves the drop (not a timing) was the cause.
                // Falling through keeps the barrier's meaning - nothing is held until
                // it is bound - while never losing a frame the product would have
                // drawn. Once the pane step binds, the next dispatch adopts the bind
                // and holds for real.
                dispatch_owned_render_inner(
                    window,
                    hosts,
                    slot,
                    session_id,
                    coordinator,
                    gpu_worker,
                    dispatch_owner,
                );
                return;
            }
        }
    }
    // Target equality check: absent target must fail armed binding explicitly,
    // never matching all panes or silently being ignored.
    let target = match target_backend_session_id.as_deref() {
        Some(target) if !target.is_empty() => target,
        _ => {
            schedule_native_qa_receipt(&channel,
                crate::ipc::qa_barrier::PRESENTATION_BARRIER,
                &spec.operation_id,
                serde_json::json!({
                    "sessionId": session_id,
                    "backendSessionId": session_id,
                    "stage": "presentation_binding_failed",
                    "status": "failed",
                    "error": "BINDING_FAILURE: missing required targetBackendSessionId on armed barrier",
                    "actionable": false,
                    "producerComponent": "surface-host-render-coordinator",
                }),
            );
            return;
        }
    };
    if target != session_id {
        // An explicit target that does not match this session is not this
        // barrier's frame: dispatch it normally instead of dropping it.
        dispatch_owned_render_inner(
            window,
            hosts,
            slot,
            session_id,
            coordinator,
            gpu_worker,
            dispatch_owner,
        );
        return;
    }

    // One pending barrier hold per session coordinator; the channel has no claim API.
    let claim_key = (channel.run_id().to_string(), spec.operation_id.clone());
    {
        let mut claim = coordinator.presentation_claim.lock();
        if claim.is_some() {
            return;
        }
        *claim = Some(claim_key.clone());
    }

    let window_clone = window.clone();
    let hosts_clone = Arc::clone(&hosts);
    let slot_clone = Arc::clone(&slot);
    let session_id_clone = session_id.clone();
    let coordinator_clone = Arc::clone(&coordinator);
    let gpu_worker_clone = Arc::clone(&gpu_worker);
    let channel_clone = Arc::clone(&channel);
    // The released branch needs the same coordinator after the render
    // dispatch, and the presentation watch must be subscribed before
    // that dispatch to see THIS frame present.
    let slot_settle = Arc::clone(&slot);
    let coordinator_held = Arc::clone(&coordinator);

    tauri::async_runtime::spawn(async move {
        let channel_held = Arc::clone(&channel_clone);
        let spec_held = spec.clone();
        let session_held = session_id_clone.clone();
        let coordinator_for_held = Arc::clone(&coordinator_held);
        let held_res = tokio::task::spawn_blocking(move || {
            // The held verdict is the REAL classifier over the measured
            // coordinator state: a frame that is no longer pending is
            // reported as what it is instead of as a literal.
            let render_pending = coordinator_for_held.is_render_pending();
            let held_snapshot = qa_coordinator_snapshot(&session_held, render_pending);
            let held_verdict = crate::ipc::qa_barrier::verdict_str(
                crate::ipc::debug::classify_pane_liveness(&held_snapshot),
            );
            channel_held.write_held(
                &spec_held,
                &session_held,
                "presentation_frame_pending",
                serde_json::json!({
                    "sessionId": session_held,
                    "stage": "presentation_frame_pending",
                    "classifierVerdict": held_verdict,
                    "hasUnpresentedFrames": render_pending,
                    "producerComponent": "surface-host-render-coordinator",
                }),
            );
            channel_held.append_receipt(
                crate::ipc::qa_barrier::PRESENTATION_BARRIER,
                &spec_held.operation_id,
                serde_json::json!({
                    "sessionId": session_held,
                    "stage": "presentation_frame_pending",
                    "classifierVerdict": held_verdict,
                    "hasUnpresentedFrames": render_pending,
                    "producerComponent": "surface-host-render-coordinator",
                    "snapshot": serde_json::to_value(&held_snapshot)
                        .unwrap_or(serde_json::Value::Null),
                }),
            );
        })
        .await;
        if let Err(e) = held_res {
            tracing::error!("Error writing presentation held receipt: {e}");
        }

        let outcome = channel_clone.wait_for_release(&spec).await;
        {
            let mut claim = coordinator_clone.presentation_claim.lock();
            if claim.as_ref() == Some(&claim_key) {
                *claim = None;
            }
        }

        match outcome {
            crate::ipc::qa_barrier::ReleaseOutcome::Released => {
                // Subscribe BEFORE the release-driven dispatch so the
                // settlement reports the presentation of THIS frame
                // instead of racing it.
                let mut presentations = slot_settle.subscribe_presentations();
                presentations.borrow_and_update();
                dispatch_owned_render_inner(
                    window_clone,
                    hosts_clone,
                    slot_clone,
                    session_id_clone.clone(),
                    Arc::clone(&coordinator_clone),
                    gpu_worker_clone,
                    dispatch_owner,
                );
                let frame_presented = tokio::time::timeout(
                    PRESENTATION_SETTLE_TIMEOUT,
                    async {
                        loop {
                            if presentations.changed().await.is_err() {
                                return false;
                            }
                            if presentations
                                .borrow_and_update()
                                .as_ref()
                                .is_some_and(|frame| frame.receipt.presented)
                            {
                                return true;
                            }
                        }
                    },
                )
                .await
                .unwrap_or(false);
                let channel_settle = Arc::clone(&channel_clone);
                let session_settle = session_id_clone.clone();
                let op_id_settle = spec.operation_id.clone();
                let coordinator_settle = Arc::clone(&coordinator_clone);
                // The daemon's own reader/kernel facts are what let this
                // settlement report a POSITIVE recovery. They are observed
                // from the daemon that owns the session; without a client in
                // hand (headless lane) they stay unobserved and the receipt
                // keeps its honest non-pass.
                let daemon_facts = match crate::ipc::qa_barrier::qa_daemon_client() {
                    Some(client) => Some(
                        crate::ipc::qa_barrier::observe_daemon_liveness(
                            &client,
                            &session_settle,
                        )
                        .await,
                    ),
                    None => None,
                };
                let settle_res = tokio::task::spawn_blocking(move || {
                    emit_presentation_released_settlement_qa(
                        &channel_settle,
                        &session_settle,
                        &op_id_settle,
                        &coordinator_settle,
                        frame_presented,
                        daemon_facts.as_ref(),
                    );
                })
                .await;
                if let Err(e) = settle_res {
                    tracing::error!("Error writing presentation released settlement: {e}");
                }
            }
            crate::ipc::qa_barrier::ReleaseOutcome::DeadlineExceeded => {
                coordinator_clone.consume_render();
                let channel_deadline = Arc::clone(&channel_clone);
                let session_deadline = session_id_clone.clone();
                let op_id_deadline = spec.operation_id.clone();
                let dead_res = tokio::task::spawn_blocking(move || {
                    channel_deadline.append_receipt(
                        crate::ipc::qa_barrier::PRESENTATION_BARRIER,
                        &op_id_deadline,
                        serde_json::json!({
                            "sessionId": session_deadline,
                            "stage": "presentation_frame_settled",
                            "classifierVerdict": "BlockedInPresentation",
                            "releaseOutcome": "deadline-exceeded",
                            "frameConsumed": false,
                            "producerComponent": "surface-host-render-coordinator",
                        }),
                    );
                })
                .await;
                if let Err(e) = dead_res {
                    tracing::error!("Error writing presentation deadline receipt: {e}");
                }
            }
        }
    });
}

fn dispatch_owned_render_inner<R: Runtime>(
    window: Window<R>,
    hosts: Arc<Mutex<HashMap<String, NativeTerminalSurfaceHost>>>,
    slot: Arc<SnapshotSlot>,
    session_id: String,
    coordinator: Arc<RenderScheduleCoordinator>,
    gpu_worker: Arc<GpuWorker>,
    dispatch_owner: u64,
) {
    let surface_window = window.clone();
    let follow_up_window = window.clone();
    let failure_coordinator = Arc::clone(&coordinator);
    let failure_session_id = session_id.clone();
    if let Err(err) = dispatch_render_on_main_thread(&window, move || {
        let mut hosts_guard = hosts.lock();
        let owner = {
            let ownership = coordinator.ownership.lock();
            if *ownership != dispatch_owner || !coordinator.begin_render() {
                return;
            }
            *ownership
        };
        let Some(frame) = slot.consume() else {
            coordinator.abandon_render(owner);
            return;
        };
        let layout = frame.layout;
        let logical_bounds = frame.logical_bounds;
        let frame_epoch = frame.attachment_epoch;
        let frame_generation = frame.generation;
        let frame_attach_tuple = frame.attach_tuple.clone();
        let render_input = frame.input.clone();

        // Blocker 1: Resurrection prevention TOCTOU check.
        // If the session was detached while waiting for hosts_guard, do NOT create or update any host!
        if !slot.is_attached_with_epoch(frame_epoch) {
            drop(hosts_guard);
            coordinator.abandon_render(owner);
            return;
        }
        let host = match hosts_guard.entry(session_id.clone()) {
            std::collections::hash_map::Entry::Occupied(entry) => Some(entry.into_mut()),
            std::collections::hash_map::Entry::Vacant(entry) => {
                match NativeTerminalSurfaceHost::new(&surface_window, logical_bounds.scale_factor) {
                    Ok(new_host) => Some(entry.insert(new_host)),
                    Err(err) => {
                        tracing::warn!(
                            session_id = %session_id,
                            error = %err,
                            "Failed to lazily create native terminal surface host during scheduled render"
                        );
                        None
                    }
                }
            }
        };
        let Some(host) = host else {
            drop(hosts_guard);
            coordinator.abandon_render(owner);
            return;
        };

        let effective_bounds = host
            .active_presentation_geometry()
            .resolve(logical_bounds)
            .unwrap_or(logical_bounds);
        host.layout = Some(layout);
        host.logical_bounds = Some(effective_bounds);
        host.presentation_slot = Some(Arc::downgrade(&slot));
        host.update_viewport(Some(effective_bounds));

        if render_input.synchronized_output {
            drop(hosts_guard);
            coordinator.abandon_render(owner);
            return;
        }

        match &mut host.frame_target {
            HostFrameTarget::Native(target) => {
                let cell_metrics = target.cell_metrics;
                let completion_slot = target.leg.clone();
                let Some(loan) = target.leg.lend_loan() else {
                    drop(hosts_guard);
                    coordinator.abandon_render(owner);
                    return;
                };
                // CRITICAL: Drop hosts lock before passing execution to the GPU worker thread.
                // The global hosts mutex must not span GPU operations.
                drop(hosts_guard);

                let completion_window = surface_window.clone();
                let completion_host_window = surface_window.clone();
                let completion_hosts = Arc::clone(&hosts);
                let completion_snapshot_slot = Arc::clone(&slot);
                let completion_session_id = session_id.clone();
                let completion_coordinator = Arc::clone(&coordinator);
                let completion_gpu_worker = Arc::clone(&gpu_worker);

                // `marker-output`'s `frameSubmitted` half: the sequence this
                // frame really covers, written to the terminal layer's sidecar
                // once the submission actually happened.
                #[cfg(feature = "local-split-qa")]
                let qa_frame_submission = crate::ipc::qa_barrier::active_channel().and_then(|channel| {
                    let covered_sequence = slot.consume()?.covered_sequence?;
                    Some((channel, session_id.clone(), covered_sequence))
                });
                let submission_coordinator = Arc::clone(&coordinator);
                let worker_snapshot_slot = Arc::clone(&slot);
                if !gpu_worker.enqueue(move |gpu| {
                    let mut loan = loan;
                    let receipt_result = if worker_snapshot_slot.is_attached_with_epoch(frame_epoch) {
                        run_gpu_frame(|| loan.render_snapshot(
                            gpu,
                            Some(effective_bounds),
                            layout,
                            &render_input.snapshot,
                            render_input.selection.as_ref(),
                            render_input.scrollbar_overlay.as_ref(),
                            render_input.attention_frame,
                        ))
                    } else {
                        // Bounds can advance the epoch without retiring the surface. Complete
                        // the cancelled frame normally so its queued geometry update still runs.
                        Ok(NativeTerminalSurfaceReceipt::from_snapshot(
                            layout,
                            &render_input.snapshot,
                            0,
                            0,
                            cell_metrics,
                            Some(effective_bounds),
                        ))
                    };
                    loan.return_to_slot();

                    let dispatch_failure_coordinator = Arc::clone(&completion_coordinator);
                    let completion_window_for_closure = completion_window.clone();
                    if let Err(err) = dispatch_render_on_main_thread(&completion_window, move || {
                        if discard_retired_completion(&completion_slot) {
                            return;
                        }
                        let current_owner = completion_hosts
                            .lock()
                            .get(&completion_session_id)
                            .is_some_and(|host| {
                                host.active_presentation_generation() == Some(frame_generation)
                                    && host.active_presentation_epoch() == Some(frame_epoch)
                            });
                        if !current_owner {
                            completion_coordinator.abandon_render(owner);
                            return;
                        }
                        let retry = gpu_completion_requires_retry(&receipt_result);
                        let receipt = match receipt_result {
                            Ok(r) => r,
                            Err(err) => {
                                tracing::warn!(
                                    session_id = %completion_session_id,
                                    error = %err,
                                    "Failed to render native terminal snapshot"
                                );
                                NativeTerminalSurfaceReceipt::from_snapshot(
                                    layout,
                                    &render_input.snapshot,
                                    0,
                                    0,
                                    cell_metrics,
                                    Some(effective_bounds),
                                )
                            }
                        };
                        let current_frame = completion_snapshot_slot.consume();
                        let tuple_is_current = current_frame.as_ref().is_some_and(|current| {
                            current.generation == frame_generation
                                && current.attach_tuple == frame_attach_tuple
                        });

                        // `stale-receipt-rejected`: a positive completion whose
                        // frame is no longer the current submission is fenced
                        // off here, and the reason names the real divergence.
                        #[cfg(feature = "local-split-qa")]
                        if receipt.presented && !tuple_is_current {
                            if let Some(channel) = crate::ipc::qa_barrier::active_channel() {
                                emit_stale_completion_rejected_qa(
                                    &channel,
                                    &completion_session_id,
                                    current_frame.as_ref().map(|frame| frame.generation),
                                    current_frame
                                        .as_ref()
                                        .and_then(|frame| frame.attach_tuple.as_ref()),
                                    frame_generation,
                                    frame_attach_tuple.as_ref(),
                                );
                            }
                        }

                        if receipt.presented && tuple_is_current {
                            let mut hosts_guard = completion_hosts.lock();
                            // The only place a caller can learn that this frame actually reached
                            // the screen: the direct render path always defers. Stale completions
                            // are rejected inside the slot by generation/epoch, so a detached or
                            // superseded attachment can never be marked presented.
                            let accepted = completion_snapshot_slot.publish_presentation(
                                frame_generation,
                                frame_epoch,
                                frame_attach_tuple.as_ref(),
                                receipt,
                            );

                            if accepted {
                                if let Some(host) = hosts_guard.get_mut(&completion_session_id) {
                                    host.finish_presentation(&completion_host_window);
                                }
                                // The product's own receipt is the one value that
                                // carries the authoritative seven-field tuple; the
                                // frontend emit and the private QA settlement both
                                // consume it unchanged.
                                if let Some(presentation_receipt) =
                                    pane_liveness_presentation_receipt(frame_attach_tuple.clone(), true)
                                {
                                    if let Err(error) = completion_window_for_closure.emit(
                                        crate::ipc::native_terminal::NATIVE_TERMINAL_PRESENTATION_RECEIPT_EVENT,
                                        presentation_receipt.clone(),
                                    ) {
                                        tracing::debug!(%error, "Failed to emit pane presentation receipt");
                                    }
                                    #[cfg(feature = "local-split-qa")]
                                    if let Some(channel) = crate::ipc::qa_barrier::active_channel() {
                                        emit_native_presentation_receipt_qa(
                                            &channel,
                                            &completion_session_id,
                                            frame_generation,
                                            frame_epoch,
                                            &receipt,
                                            &presentation_receipt,
                                        );
                                    }
                                }
                            }
                        }

                        if completion_coordinator.finish_owned_render(owner, retry) {
                            let follow_up_delay = completion_coordinator.delay_before_retry();
                            tauri::async_runtime::spawn(async move {
                                tokio::time::sleep(follow_up_delay).await;
                                dispatch_owned_render(
                                    follow_up_window,
                                    completion_hosts,
                                    completion_snapshot_slot,
                                    completion_session_id,
                                    completion_coordinator,
                                    completion_gpu_worker,
                                    owner.wrapping_add(1),
                                );
                            });
                        }
                    }) {
                        dispatch_failure_coordinator.abandon_render(owner);
                        tracing::warn!(error = %err, "Failed to dispatch GPU frame completion");
                    }
                }) {
                    submission_coordinator.abandon_render(owner);
                    tracing::warn!("Native terminal GPU worker rejected frame submission");
                } else {
                    #[cfg(feature = "local-split-qa")]
                    if let Some((channel, submission_session, covered_sequence)) = qa_frame_submission
                    {
                        schedule_native_frame_submitted_qa(
                            &channel,
                            &submission_session,
                            covered_sequence,
                        );
                    }
                }
            }
            #[cfg(test)]
            HostFrameTarget::Injected(target) => {
                let receipt = target.render_snapshot(layout, &render_input.snapshot);
                drop(hosts_guard);
                let retry = gpu_completion_requires_retry(&receipt);
                match receipt {
                    Ok(receipt) => {
                        if receipt.presented {
                            if slot.publish_presentation(frame_generation, frame_epoch, frame_attach_tuple.as_ref(), receipt) {
                                if let Some(receipt) = pane_liveness_presentation_receipt(frame_attach_tuple, true) {
                                    if let Err(error) = surface_window.emit(
                                        crate::ipc::native_terminal::NATIVE_TERMINAL_PRESENTATION_RECEIPT_EVENT, receipt,
                                    ) {
                                        tracing::debug!(%error, "Failed to emit injected presentation receipt");
                                    }
                                }
                            }
                        }
                    }
                    Err(err) => tracing::warn!(
                        session_id = %session_id,
                        error = %err,
                        "Failed to render native terminal snapshot"
                    ),
                }

                if coordinator.finish_owned_render(owner, retry) {
                    let follow_up_delay = coordinator.delay_before_retry();
                    tauri::async_runtime::spawn(async move {
                        tokio::time::sleep(follow_up_delay).await;
                        dispatch_owned_render(
                            follow_up_window,
                            hosts,
                            slot,
                            session_id,
                            coordinator,
                            gpu_worker,
                            owner.wrapping_add(1),
                        );
                    });
                }
            }
        }
    }) {
        failure_coordinator.abandon_render(dispatch_owner);
        tracing::warn!(
            session_id = %failure_session_id,
            error = %err,
            "Failed to dispatch native terminal render to main thread"
        );
    }
}

fn discard_retired_completion<T>(slot: &LentSlot<T>) -> bool {
    slot.is_retired()
}

fn gpu_completion_requires_retry(
    result: &Result<NativeTerminalSurfaceReceipt, NativeTerminalError>,
) -> bool {
    match result {
        Ok(receipt) => !receipt.presented && !receipt.render_deferred && !receipt.render_suspended,
        Err(_) => false,
    }
}

fn run_gpu_frame<T>(render: impl FnOnce() -> Result<T, NativeTerminalError>) -> Result<T, NativeTerminalError> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(render)).unwrap_or_else(|_| {
        Err(NativeTerminalError::GpuPipelineError("Native terminal GPU frame panicked".into()))
    })
}

/// Lower bound between a dropped frame and its retry.
///
/// A surface that refuses a drawable (Metal `Timeout`, occluded window) used to re-dispatch
/// inline, so a compositor stall became an unbounded main-thread spin at display refresh.
/// Pacing the retry caps that cost regardless of how long the surface stays unavailable.
pub(crate) const RETRY_FRAME_INTERVAL: std::time::Duration = std::time::Duration::from_millis(8);

/// Paces frames against when the last one *started*, not when it finished.
///
/// Sleeping a fixed interval after every frame charges the full interval even when the frame
/// itself already spent it, so a slow frame paid twice and every coalesced follow-up inherited
/// up to a full interval of avoidable latency. Measuring from the frame start makes a burst
/// inside one interval collapse to a single pass while a frame that already overran dispatches
/// immediately.
#[derive(Debug)]
pub struct FrameClock {
    interval: std::time::Duration,
    last_frame_started: Mutex<Option<std::time::Instant>>,
}

impl FrameClock {
    pub const fn new(interval: std::time::Duration) -> Self {
        Self {
            interval,
            last_frame_started: Mutex::new(None),
        }
    }

    /// How long to wait before the next frame may start. `now` is a parameter so the policy is
    /// testable without sleeping.
    pub fn delay_before_next_frame(&self, now: std::time::Instant) -> std::time::Duration {
        let Some(started) = *self.last_frame_started.lock() else {
            // Nothing has rendered yet; the first frame must not be delayed.
            return std::time::Duration::ZERO;
        };
        self.interval
            .saturating_sub(now.saturating_duration_since(started))
    }

    /// Delay before RE-dispatching a frame that just failed to present.
    ///
    /// Distinct from `delay_before_next_frame` because a retry is by definition never the first
    /// frame: if no frame start was recorded, pacing conservatively by a full interval is right,
    /// while returning zero would silently restore the inline re-dispatch spin. Not every render
    /// path marks the clock, so this must not assume one did.
    pub fn delay_before_retry(&self, now: std::time::Instant) -> std::time::Duration {
        let Some(started) = *self.last_frame_started.lock() else {
            return self.interval;
        };
        self.interval
            .saturating_sub(now.saturating_duration_since(started))
    }

    pub fn mark_frame_started(&self, now: std::time::Instant) {
        *self.last_frame_started.lock() = Some(now);
    }
}

impl Default for FrameClock {
    fn default() -> Self {
        Self::new(RETRY_FRAME_INTERVAL)
    }
}

#[cfg(test)]
mod frame_clock_tests {
    use super::FrameClock;
    use std::time::{Duration, Instant};

    const INTERVAL: Duration = Duration::from_millis(8);

    #[test]
    fn the_first_frame_is_never_delayed() {
        let clock = FrameClock::new(INTERVAL);
        assert_eq!(
            clock.delay_before_next_frame(Instant::now()),
            Duration::ZERO,
            "nothing has rendered yet, so the first frame must go immediately"
        );
    }

    #[test]
    fn a_burst_inside_one_interval_waits_only_the_remainder() {
        // SC1: output arriving mid-frame must collapse into one pass, not render back to back.
        let clock = FrameClock::new(INTERVAL);
        let start = Instant::now();
        clock.mark_frame_started(start);
        assert_eq!(
            clock.delay_before_next_frame(start + Duration::from_millis(3)),
            Duration::from_millis(5),
            "3ms into an 8ms budget leaves 5ms, not another full interval"
        );
    }

    #[test]
    fn a_frame_that_already_overran_dispatches_immediately() {
        // The regression a fixed post-frame sleep caused: a 20ms frame still paid another 8ms,
        // so slow frames were charged twice and coalesced follow-ups inherited the latency.
        let clock = FrameClock::new(INTERVAL);
        let start = Instant::now();
        clock.mark_frame_started(start);
        assert_eq!(
            clock.delay_before_next_frame(start + Duration::from_millis(20)),
            Duration::ZERO,
            "a frame that outran its budget must not be delayed again"
        );
    }

    #[test]
    fn an_unrecorded_clock_still_paces_a_retry() {
        // The bug this pins, which I shipped and caught only because an existing test failed:
        // not every render path marks the clock. When nothing was recorded, returning ZERO here
        // silently removed the retry pacing entirely and restored the inline re-dispatch spin.
        // A retry is never the first frame, so an unrecorded clock must pace a full interval.
        let clock = FrameClock::new(INTERVAL);
        assert_eq!(
            clock.delay_before_retry(Instant::now()),
            INTERVAL,
            "an unmarked clock must pace a retry, never let it fire immediately"
        );
        assert_eq!(
            clock.delay_before_next_frame(Instant::now()),
            Duration::ZERO,
            "but a genuine first frame is still not delayed"
        );
    }

    #[test]
    fn marking_a_new_frame_restarts_the_budget() {
        let clock = FrameClock::new(INTERVAL);
        let start = Instant::now();
        clock.mark_frame_started(start);
        let later = start + Duration::from_millis(20);
        clock.mark_frame_started(later);
        assert_eq!(
            clock.delay_before_next_frame(later + Duration::from_millis(1)),
            Duration::from_millis(7),
            "the budget must be measured from the newest frame start"
        );
    }
}

fn defer_scheduled_render<R: Runtime>(
    window: Window<R>,
    hosts: Arc<Mutex<HashMap<String, NativeTerminalSurfaceHost>>>,
    slot: Arc<SnapshotSlot>,
    session_id: String,
    coordinator: Arc<RenderScheduleCoordinator>,
    gpu_worker: Arc<GpuWorker>,
) {
    #[cfg(test)]
    let test_window = window.clone();
    let retry_delay = coordinator.delay_before_retry();
    let owner = *coordinator.ownership.lock();
    // Wry runs main-thread dispatch inline; enqueue off-thread to avoid recursive retries.
    let _task = tauri::async_runtime::spawn(async move {
        tokio::time::sleep(retry_delay).await;
        dispatch_owned_render(window, hosts, slot, session_id, coordinator, gpu_worker, owner);
    });
    #[cfg(test)]
    if let Some(dispatch) = test_window.try_state::<tests::RenderDispatch>() {
        dispatch.submissions.lock().push(_task);
    }
}

#[cfg(test)]
pub(crate) static RENDER_SNAPSHOT_CALL_COUNT: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

pub fn dispatch_main_thread_receipt<R: Runtime>(
    window: &Window<R>,
    task: impl FnOnce() + Send + 'static,
) -> tauri::Result<()> {
    #[cfg(test)]
    if let Some(dispatch) = window.try_state::<tests::RenderDispatch>() {
        dispatch.submit_receipt(Box::new(task));
        return Ok(());
    }
    window.run_on_main_thread(task)
}

fn dispatch_render_on_main_thread<R: Runtime>(
    window: &Window<R>,
    task: impl FnOnce() + Send + 'static,
) -> tauri::Result<()> {
    #[cfg(test)]
    if let Some(dispatch) = window.try_state::<tests::RenderDispatch>() {
        dispatch.submit(Box::new(task));
        return Ok(());
    }
    window.run_on_main_thread(task)
}

impl Clone for NativeTerminalSurfaceHostState {
    fn clone(&self) -> Self {
        Self {
            hosts: Arc::clone(&self.hosts),
            sessions: Arc::clone(&self.sessions),
            event_sink: Arc::clone(&self.event_sink),
            pty_resize_sink: Arc::clone(&self.pty_resize_sink),
            pending_startups: Arc::clone(&self.pending_startups),
            gpu_worker: Arc::clone(&self.gpu_worker),
            dom_overlay_owns_input: Arc::clone(&self.dom_overlay_owns_input),
        }
    }
}

impl Default for NativeTerminalSurfaceHostState {
    fn default() -> Self {
        Self {
            hosts: Arc::new(Mutex::new(HashMap::new())),
            sessions: Arc::new(Mutex::new(HashMap::new())),
            event_sink: Arc::new(RwLock::new(None)),
            pty_resize_sink: Arc::new(RwLock::new(None)),
            pending_startups: Arc::new(Mutex::new(HashSet::new())),
            dom_overlay_owns_input: Arc::new(AtomicBool::new(false)),
            gpu_worker: Arc::new(
                GpuWorker::new("ferryx-gpu-worker").expect("spawn native terminal gpu worker"),
            ),
        }
    }
}

fn take_native_terminal_events(
    session: &mut NativeTerminalSession,
    session_id: &str,
    force_detect: bool,
) -> Vec<NativeTerminalEvent> {
    let mut events = Vec::with_capacity(3);
    if session.terminal.take_title_changed() {
        match session.terminal.title() {
            Ok(title) => events.push(NativeTerminalEvent::Title(NativeTerminalTitlePayload {
                session_id: session_id.to_string(),
                title,
            })),
            Err(error) => tracing::warn!(
                session_id,
                %error,
                "Failed to query native terminal title after title-change callback"
            ),
        }
    }
    let count = session.terminal.take_bell_count();
    if count > 0 {
        events.push(NativeTerminalEvent::Bell(NativeTerminalBellPayload {
            session_id: session_id.to_string(),
            count,
        }));
    }
    if let Some(event) = take_native_terminal_scrollbar_event(session, session_id) {
        events.push(event);
    }

    if session.agent_reports_own_state {
        return events;
    }

    // Agent screen detection: throttled on streaming output chunks to avoid expensive
    // O(rows × cols) snapshot allocations and regex evaluation on every rapid burst chunk.
    // Attached panes re-run a skipped detection once the burst drains (trailing-edge detect
    // in the pump task, keyed on agent_detect_pending); backgrounded panes use a coarser
    // interval because they only need state transitions, not frames.
    let detect_interval = if session.surface_attached {
        AGENT_DETECT_INTERVAL_ATTACHED
    } else {
        AGENT_DETECT_INTERVAL_DETACHED
    };
    let should_detect = force_detect
        || match session.last_agent_detect_at {
            None => true,
            Some(last) => last.elapsed() >= detect_interval,
        };
    session.agent_detect_pending = !should_detect;

    if should_detect {
        session.last_agent_detect_at = Some(std::time::Instant::now());
        if let Ok(snapshot) = session.terminal.render_snapshot() {
            let rows = (0..snapshot.rows as usize)
                .map(|r| snapshot.row_text(r))
                .collect();
            let title = session.terminal.title().unwrap_or_default();
            let input = crate::agent_detect::ScreenInput { rows, title };
            let engine = crate::agent_detect::default_engine();
            if let Some(detection) = engine.detect(&input, session.last_agent_activity) {
                if session.last_agent_activity != Some(detection.state) {
                    session.last_agent_activity = Some(detection.state);
                    let state_str = match detection.state {
                        crate::agent_detect::AgentActivity::Working => "working",
                        crate::agent_detect::AgentActivity::Blocked => "blocked",
                        crate::agent_detect::AgentActivity::Idle => "idle",
                    };
                    tracing::info!(
                        session_id,
                        state = state_str,
                        rule_id = %detection.rule_id,
                        manifest_id = %detection.manifest_id,
                        "agent screen detection state change"
                    );
                    events.push(NativeTerminalEvent::AgentState(
                        NativeTerminalAgentStatePayload {
                            session_id: session_id.to_string(),
                            state: state_str.to_string(),
                            rule_id: detection.rule_id,
                            manifest_id: detection.manifest_id,
                            provider_session: None,
                            detail: None,
                            is_snapshot: false,
                        },
                    ));
                }
            }
        }
    }

    events
}

fn take_native_terminal_scrollbar_event(
    session: &mut NativeTerminalSession,
    session_id: &str,
) -> Option<NativeTerminalEvent> {
    let scrollbar = session.terminal.scrollbar().ok()?;
    let payload = NativeTerminalScrollbarPayload {
        session_id: session_id.to_string(),
        total: scrollbar.total,
        offset: scrollbar.offset,
        len: scrollbar.len,
    };
    session.scrollbar_overlay.metrics = Some(scrollbar);
    if session.last_scrollbar.as_ref() == Some(&payload) {
        return None;
    }
    let was_visible = session
        .last_scrollbar
        .as_ref()
        .is_some_and(|previous| previous.total > previous.len);
    let is_visible = payload.total > payload.len;
    session.last_scrollbar = Some(payload.clone());
    (was_visible || is_visible).then_some(NativeTerminalEvent::Scrollbar(payload))
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SessionRenderInput {
    pub(crate) snapshot: RenderSnapshot,
    pub(crate) selection: Option<SelectionSnapshot>,
    pub(crate) scrollbar_overlay: Option<ScrollbarOverlayState>,
    pub(crate) attention_frame: bool,
    pub(crate) synchronized_output: bool,
}

fn preedit_char_wide(c: char) -> bool {
    matches!(
        c,
        '\u{1100}'..='\u{11ff}'
            | '\u{3130}'..='\u{318f}'
            | '\u{3400}'..='\u{4dbf}'
            | '\u{4e00}'..='\u{9fff}'
            | '\u{a960}'..='\u{a97f}'
            | '\u{ac00}'..='\u{d7a3}'
            | '\u{d7b0}'..='\u{d7ff}'
            | '\u{ff01}'..='\u{ff60}'
            | '\u{ffe0}'..='\u{ffe6}'
    )
}

fn apply_preedit_to_snapshot(snapshot: &mut RenderSnapshot, preedit: &str) {
    if preedit.is_empty() || snapshot.cursor.y >= snapshot.rows {
        return;
    }

    let cols = snapshot.cols as usize;
    let Some(row) = snapshot.grid.get_mut(snapshot.cursor.y as usize) else {
        return;
    };
    let mut col = snapshot.cursor.x as usize;

    for c in preedit.chars() {
        let wide = preedit_char_wide(c);
        let width = if wide { 2 } else { 1 };
        if col + width > cols || col + width > row.len() {
            break;
        }

        row[col] = CellSnapshot {
            text: c.to_string(),
            wide: if wide {
                CellWide::Wide
            } else {
                CellWide::Narrow
            },
            underline: true,
            ..Default::default()
        };
        if wide {
            row[col + 1] = CellSnapshot {
                wide: CellWide::SpacerTail,
                underline: true,
                ..Default::default()
            };
        }
        col += width;
    }
}

fn session_render_snapshot(
    session: &NativeTerminalSession,
) -> Result<SessionRenderInput, NativeTerminalError> {
    let selection =
        session
            .terminal
            .selection_range()?
            .map(
                |(start_col, start_row, end_col, end_row)| SelectionSnapshot {
                    start_col,
                    start_row,
                    end_col,
                    end_row,
                },
            );
    let mut snapshot = session.terminal.render_snapshot()?;
    #[cfg(test)]
    RENDER_SNAPSHOT_CALL_COUNT.fetch_add(1, Ordering::SeqCst);
    snapshot.cursor.visual_style = cursor_style_for_focus(session.focused);
    if session.focused {
        if let Some(preedit) = session
            .preedit
            .as_deref()
            .filter(|preedit| !preedit.is_empty())
        {
            apply_preedit_to_snapshot(&mut snapshot, preedit);
        }
    }
    let scrollbar_overlay = if session.scrollbar_overlay.visible {
        let metrics = session
            .terminal
            .scrollbar()
            .ok()
            .or(session.scrollbar_overlay.metrics);
        Some(ScrollbarOverlayState {
            visible: true,
            metrics,
        })
    } else {
        None
    };
    Ok(SessionRenderInput {
        snapshot,
        selection,
        scrollbar_overlay,
        attention_frame: session.attention_frame,
        synchronized_output: session.terminal.synchronized_output_enabled()?,
    })
}

/// Resolves a session only while `pump_generation` still owns it.
///
/// Every mutating pump branch goes through this: a retired pump can be parked on the `sessions`
/// mutex at the moment a new attach takes ownership, so `get_mut` alone would hand it a session
/// it no longer has any claim to. Aborting the task does not help, because cancellation is only
/// observed at an await point and the branch has already passed its last one.
fn session_owned_by<'a>(
    sessions: &'a mut HashMap<String, NativeTerminalSession>,
    session_id: &str,
    pump_generation: u64,
) -> Option<&'a mut NativeTerminalSession> {
    sessions
        .get_mut(session_id)
        .filter(|session| session.pump_generation == pump_generation)
}

fn lock_replay_to_bottom(
    sessions: &mut HashMap<String, NativeTerminalSession>,
    session_id: &str,
    pump_generation: u64,
) {
    if let Some(session) = session_owned_by(sessions, session_id, pump_generation) {
        let _ = session.terminal.scroll_viewport(crate::native_terminal::ScrollViewport::Bottom);
        session.publish_frame();
    }
}

/// Ownership token minted by [`NativeTerminalSurfaceHostState::begin_replay_request`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayRequestToken {
    pub generation: u64,
    pub epoch: u64,
    pub last_sequence: Option<u64>,
    pub attempt_generation: Option<u64>,
    pub attach_tuple: Option<crate::daemon::protocol::PaneAttachTuple>,
}

fn pane_liveness_tuple_matches(
    active: &crate::daemon::protocol::PaneAttachTuple,
    candidate: &crate::daemon::protocol::PaneAttachTuple,
) -> bool {
    // Compare the full frozen seven-field tuple; this deliberately supersedes the plan's older
    // “five-field” wording by also fencing incarnation and daemonEpoch.
    active.backend_session_id == candidate.backend_session_id
        && active.incarnation == candidate.incarnation
        && active.daemon_epoch == candidate.daemon_epoch
        && active.frontend_session_id == candidate.frontend_session_id
        && active.pane_identity == candidate.pane_identity
        && active.binding_key == candidate.binding_key
        && active.attempt_generation == candidate.attempt_generation
}

fn pane_liveness_presentation_receipt(
    attach_tuple: Option<crate::daemon::protocol::PaneAttachTuple>,
    presented: bool,
) -> Option<crate::daemon::protocol::PanePresentationReceipt> {
    if !presented {
        return None;
    }
    Some(crate::daemon::protocol::PanePresentationReceipt {
        attach_tuple: attach_tuple?,
        presented: true,
        presentation_time_unix_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .and_then(|duration| u64::try_from(duration.as_millis()).ok()),
    })
}

fn pane_liveness_registration_matches(
    active: &crate::daemon::protocol::PaneAttachTuple,
    incoming: &crate::daemon::protocol::PaneAttachTuple,
) -> bool {
    active.backend_session_id == incoming.backend_session_id
        && active.incarnation == incoming.incarnation
        && active.frontend_session_id == incoming.frontend_session_id
        && active.pane_identity == incoming.pane_identity
        && active.binding_key == incoming.binding_key
        && active.attempt_generation <= incoming.attempt_generation
}

#[cfg(test)]
mod pane_liveness_native_binding_tests {
    use super::pane_liveness_tuple_matches;
    use crate::daemon::protocol::PaneAttachTuple;

    fn tuple() -> PaneAttachTuple {
        serde_json::from_value(serde_json::json!({
            "backendSessionId": "backend-1",
            "incarnation": "incarnation-1",
            "daemonEpoch": "7",
            "frontendSessionId": "frontend-1",
            "paneIdentity": "pane-1",
            "bindingKey": "binding-1",
            "attemptGeneration": 3
        }))
        .expect("valid seven-field pane attach tuple")
    }

    #[test]
    fn pane_liveness_native_binding_matches_all_seven_fields() {
        let active = tuple();
        assert!(pane_liveness_tuple_matches(&active, &tuple()));
        for (field, value) in [
            ("backendSessionId", serde_json::json!("backend-2")),
            ("incarnation", serde_json::json!("incarnation-2")),
            ("daemonEpoch", serde_json::json!("8")),
            ("frontendSessionId", serde_json::json!("frontend-2")),
            ("paneIdentity", serde_json::json!("pane-2")),
            ("bindingKey", serde_json::json!("binding-2")),
            ("attemptGeneration", serde_json::json!(4)),
        ] {
            let mut candidate = serde_json::to_value(tuple()).expect("serialize tuple");
            candidate[field] = value;
            let candidate: PaneAttachTuple =
                serde_json::from_value(candidate).expect("valid candidate tuple");
            assert!(!pane_liveness_tuple_matches(&active, &candidate), "{field}");
        }
    }

    #[test]
    fn pane_liveness_native_binding_positive_receipt_requires_real_presentation() {
        let tuple = tuple();
        assert!(super::pane_liveness_presentation_receipt(Some(tuple.clone()), false).is_none());
        let receipt = super::pane_liveness_presentation_receipt(Some(tuple.clone()), true)
            .expect("presented native frame has receipt");
        assert!(receipt.presented);
        assert!(super::pane_liveness_tuple_matches(&tuple, &receipt.attach_tuple));
        assert!(super::pane_liveness_presentation_receipt(None, true).is_none());
    }

    #[test]
    fn pane_liveness_native_binding_rejects_stale_teardown_generation() {
        let active = tuple();
        let mut stale = tuple();
        stale.attempt_generation = active.attempt_generation - 1;
        assert!(!super::pane_liveness_tuple_matches(&active, &stale));
    }

    #[test]
    fn pane_liveness_native_binding_receipt_path_is_tuple_fenced() {
        let active = tuple();
        let mut stale = tuple();
        stale.daemon_epoch = "8".into();
        let receipt = super::pane_liveness_presentation_receipt(Some(stale), true)
            .expect("observed presentation forms receipt");
        assert!(!super::pane_liveness_tuple_matches(&active, &receipt.attach_tuple));
    }

    #[test]
    fn pane_liveness_native_binding_unpresented_frame_has_no_receipt() {
        assert!(super::pane_liveness_presentation_receipt(Some(tuple()), false).is_none());
    }

    fn attachment(session_id: &str) -> (crate::daemon::DaemonAttachment, tokio::sync::mpsc::Sender<crate::daemon::DaemonStreamMessage<'static>>) {
        let (output, messages) = tokio::sync::mpsc::channel(1);
        (crate::daemon::DaemonAttachment {
            session_id: session_id.into(), epoch: 7,
            start_sequence: None, end_sequence: None, gap: None,
            history: bytes::Bytes::new(), history_segments: Vec::new(),
            pty_cols: Some(80), pty_rows: Some(24), remote_generation: None,
            messages, stream_task: tokio::spawn(std::future::pending()),
        }, output)
    }

    #[tokio::test]
    async fn pane_liveness_native_binding_new_completion_before_old_preserves_stream() {
        let state = super::NativeTerminalSurfaceHostState::default();
        let old = tuple();
        state.attach_test_session_for_liveness(&old.backend_session_id, 7, None).unwrap();
        let old_token = state.begin_replay_request_with_tuple(&old.backend_session_id, Some(old.clone())).unwrap();
        let mut new = old.clone();
        new.attempt_generation += 1;
        let new_token = state.begin_replay_request_with_tuple(&new.backend_session_id, Some(new.clone())).unwrap();
        let (new_attachment, _new_output) = attachment(&new.backend_session_id);
        state.attach_daemon_attachment_with_client_and_token::<tauri::test::MockRuntime>(
            &new.backend_session_id, new_attachment, None, None, Some(new_token),
        ).unwrap();
        let stream_id = state.sessions.lock()[&new.backend_session_id].stream_task.as_ref().unwrap().id();
        let (old_attachment, _old_output) = attachment(&old.backend_session_id);
        assert!(state.attach_daemon_attachment_with_client_and_token::<tauri::test::MockRuntime>(
            &old.backend_session_id, old_attachment, None, None, Some(old_token),
        ).is_err());
        assert_eq!(state.session_attach_tuple(&new.backend_session_id), Some(new.clone()));
        assert_eq!(state.sessions.lock()[&new.backend_session_id].stream_task.as_ref().unwrap().id(), stream_id);
        state.teardown();
    }

    #[tokio::test]
    async fn pane_liveness_native_binding_old_teardown_preserves_new_pump_and_receipt() {
        let state = super::NativeTerminalSurfaceHostState::default();
        let old = tuple();
        state.attach_test_session_for_liveness(&old.backend_session_id, 7, None).unwrap();
        let mut new = old.clone();
        new.attempt_generation += 1;
        let token = state.begin_replay_request_with_tuple(&new.backend_session_id, Some(new.clone())).unwrap();
        let (attachment, _output) = attachment(&new.backend_session_id);
        state.attach_daemon_attachment_with_client_and_token::<tauri::test::MockRuntime>(
            &new.backend_session_id, attachment, None, None, Some(token),
        ).unwrap();
        let mut detached = state.subscribe_session_detach(&new.backend_session_id).unwrap();
        detached.borrow_and_update();
        let pump_id = state.sessions.lock()[&new.backend_session_id].pump_task.as_ref().unwrap().id();
        let bounds = super::LogicalBounds { x: 0.0, y: 0.0, width: 800.0, height: 480.0, scale_factor: 1.0 };
        let layout = state.prepare_session_layout(super::NativeTerminalBoundsRequest {
            session_id: new.backend_session_id.clone(), bounds,
        }, super::font_manager::derived_cell_metrics()).unwrap();
        let slot = state.session_snapshot_slot(&new.backend_session_id).unwrap();
        let mut presentations = slot.subscribe_presentations();
        let snapshot = state.snapshot_for_session(&new.backend_session_id).unwrap().unwrap();
        let receipt = super::NativeTerminalSurfaceReceipt {
            presented: true,
            ..super::NativeTerminalSurfaceReceipt::from_snapshot(layout, &snapshot, 0, 0,
                super::font_manager::derived_cell_metrics(), Some(bounds))
        };
        assert!(slot.publish_presentation(slot.generation(), slot.current_epoch(), Some(&new), receipt));
        presentations.borrow_and_update();
        assert!(!state.detach_session_fenced(&new.backend_session_id, Some(&old)));
        assert!(!state.close_session_fenced(&new.backend_session_id, Some(&old)));
        assert!(!detached.has_changed().unwrap());
        assert!(!presentations.has_changed().unwrap());
        assert!(presentations.borrow().as_ref().unwrap().receipt.presented);
        assert_eq!(state.sessions.lock()[&new.backend_session_id].pump_task.as_ref().unwrap().id(), pump_id);
        assert_eq!(state.session_attach_tuple(&new.backend_session_id), Some(new));
        state.teardown();
    }

    #[tokio::test]
    async fn pane_liveness_native_binding_epoch_handover_rebind_accepts_same_incarnation() {
        let state = super::NativeTerminalSurfaceHostState::default();
        let old = tuple();
        state.attach_test_session_for_liveness(&old.backend_session_id, 7, None).unwrap();
        let old_token = state.begin_replay_request_with_tuple(&old.backend_session_id, Some(old.clone())).unwrap();
        let mut new = old.clone();
        new.daemon_epoch = "8".into();
        let new_token = state.begin_replay_request_with_tuple(&new.backend_session_id, Some(new.clone())).expect("same lifetime accepts new owner epoch");
        let (mut offer, _output) = attachment(&new.backend_session_id);
        offer.epoch = 8;
        state.attach_daemon_attachment_with_client_and_token::<tauri::test::MockRuntime>(
            &new.backend_session_id, offer, None, None, Some(new_token),
        ).unwrap();
        assert_eq!(state.session_attach_tuple(&new.backend_session_id), Some(new.clone()));
        assert_eq!(state.sessions.lock()[&new.backend_session_id].daemon_epoch, 8);
        assert!(!state.sessions.lock()[&new.backend_session_id].owns_replay_request(&old_token));
        assert!(!state.detach_session_fenced(&new.backend_session_id, Some(&old)));
        let mut foreign = new.clone();
        foreign.incarnation = Some("foreign-lifetime".into());
        assert!(state.begin_replay_request_with_tuple(&new.backend_session_id, Some(foreign)).is_none());
        assert_eq!(state.session_attach_tuple(&new.backend_session_id), Some(new));
        state.teardown();
    }

    #[tokio::test]
    async fn pane_liveness_native_binding_bounds_response_carries_observed_tuple() {
        let state = super::NativeTerminalSurfaceHostState::default();
        let active = tuple();
        state.attach_test_session_for_liveness(&active.backend_session_id, 7, None).unwrap();
        state.sessions.lock().get_mut(&active.backend_session_id).unwrap().active_attach_tuple = Some(active.clone());
        let bounds = super::LogicalBounds { x: 0.0, y: 0.0, width: 800.0, height: 480.0, scale_factor: 1.0 };
        let layout = state.prepare_session_layout(super::NativeTerminalBoundsRequest {
            session_id: active.backend_session_id.clone(), bounds,
        }, super::font_manager::derived_cell_metrics()).unwrap();
        let snapshot = state.snapshot_for_session(&active.backend_session_id).unwrap().unwrap();
        let receipt = super::NativeTerminalSurfaceReceipt {
            presented: true,
            ..super::NativeTerminalSurfaceReceipt::from_snapshot(layout, &snapshot, 0, 0,
                super::font_manager::derived_cell_metrics(), Some(bounds))
        };
        assert_eq!(state.bound_presentation_observation(&active.backend_session_id, &receipt), (Some(active.clone()), false));
        let slot = state.session_snapshot_slot(&active.backend_session_id).unwrap();
        let mut presentations = slot.subscribe_presentations();
        assert!(slot.publish_presentation(slot.generation(), slot.current_epoch(), Some(&active), receipt));
        assert!(presentations.has_changed().unwrap());
        presentations.borrow_and_update();
        assert_eq!(state.bound_presentation_observation(&active.backend_session_id, &receipt), (Some(active.clone()), true));
        assert!(state.detach_session_fenced(&active.backend_session_id, Some(&active)));
        assert_eq!(state.bound_presentation_observation(&active.backend_session_id, &receipt), (Some(active), false));
        state.teardown();
    }
}

/// How a replay offer relates to what the resident grid has already applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayVerdict {
    /// Every offered byte is new and continues the applied stream: feed without resetting.
    Delta,
    /// The offer is already fully applied: feeding it again would duplicate output.
    NoOp,
    /// The resident grid cannot be reconciled with the offer, so the offer is the only truth.
    Rebuild,
}

/// Classifies a replay offer against the resident grid's last applied sequence.
///
/// `is_delta` is the producer's own statement that no bytes were lost; sequence numbers alone
/// cannot establish it, because a resize allocates a sequence without publishing a chunk and so
/// leaves a legitimate hole that is indistinguishable from dropped output. Anything unproven
/// resolves to [`ReplayVerdict::Rebuild`], which is what this code did unconditionally before.
pub fn classify_replay(
    last_applied: Option<u64>,
    start_sequence: Option<u64>,
    end_sequence: Option<u64>,
    is_delta: Option<bool>,
) -> ReplayVerdict {
    let Some(last_applied) = last_applied else {
        return ReplayVerdict::Rebuild;
    };
    if is_delta != Some(true) {
        return ReplayVerdict::Rebuild;
    }
    // A stale snapshot may report an end at or below what the grid already has; replaying it
    // would duplicate output, and adopting its end would rewind the cursor for the next offer.
    if end_sequence.is_some_and(|end| end <= last_applied) {
        return ReplayVerdict::NoOp;
    }
    match start_sequence {
        // Segment payloads carry no per-sequence offsets, so an offer that straddles the applied
        // boundary cannot be trimmed to its unapplied tail.
        Some(start) if start > last_applied => ReplayVerdict::Delta,
        _ => ReplayVerdict::Rebuild,
    }
}

fn feed_attachment_history(
    terminal: &mut NativeTerminal,
    history: &[u8],
    segments: &[HistorySegment],
) -> Result<(), NativeTerminalError> {
    if segments.is_empty() {
        return terminal.feed(history);
    }
    for segment in segments {
        if let (Some(cols), Some(rows)) = (segment.cols, segment.rows) {
            if terminal.dimensions()? != (cols, rows) {
                let metrics = font_manager::derived_cell_metrics();
                terminal.resize(cols, rows, metrics.width_px, metrics.height_px)?;
            }
        }
        terminal.feed(&segment.bytes)?;
    }
    Ok(())
}

fn emit_native_terminal_event<R: Runtime>(
    app: Option<&tauri::AppHandle<R>>,
    event_sink: &RwLock<Option<NativeTerminalEventSink>>,
    event: NativeTerminalEvent,
) {
    if let Some(app) = app {
        let result = match &event {
            NativeTerminalEvent::Title(payload) => app.emit(NATIVE_TERMINAL_TITLE_EVENT, payload),
            NativeTerminalEvent::Bell(payload) => app.emit(NATIVE_TERMINAL_BELL_EVENT, payload),
            NativeTerminalEvent::AgentState(payload) => {
                app.emit(NATIVE_TERMINAL_AGENT_STATE_EVENT, payload)
            }
            NativeTerminalEvent::Scrollbar(payload) => {
                app.emit(NATIVE_TERMINAL_SCROLLBAR_EVENT, payload)
            }
        };
        if let Err(error) = result {
            tracing::debug!(%error, "Failed to emit native terminal event");
        }
    }
    let sink = event_sink.read().clone();
    if let Some(sink) = sink {
        sink(event);
    }
}

impl NativeTerminalSurfaceHostState {
    pub fn reset_agent_state<R: Runtime>(
        &self,
        session_id: &str,
        app: Option<&tauri::AppHandle<R>>,
    ) {
        let changed = {
            let mut sessions = self.sessions.lock();
            if let Some(sess) = sessions.get_mut(session_id) {
                sess.agent_reports_own_state = false;
                sess.last_agent_activity = Some(crate::agent_detect::AgentActivity::Idle);
                true
            } else {
                false
            }
        };
        if changed {
            let payload = NativeTerminalAgentStatePayload {
                session_id: session_id.to_string(),
                state: "idle".to_string(),
                rule_id: "manual-reset".to_string(),
                manifest_id: "".to_string(),
                provider_session: None,
                detail: None,
                is_snapshot: false,
            };
            emit_native_terminal_event(
                app,
                &self.event_sink,
                NativeTerminalEvent::AgentState(payload),
            );
        }
    }

    pub fn has_focused_session(&self) -> bool {
        self.sessions.lock().values().any(|session| session.focused)
    }

    pub fn set_event_sink(&self, sink: NativeTerminalEventSink) {
        *self.event_sink.write() = Some(sink);
    }

    pub fn set_pty_resize_sink_if_absent(&self, sink: NativeTerminalPtyResizeSink) -> bool {
        let mut current = self.pty_resize_sink.write();
        if current.is_some() {
            return false;
        }
        *current = Some(sink);
        true
    }

    fn notify_pty_resize(&self, session_id: &str, cols: u16, rows: u16) {
        if let Some(sink) = self.pty_resize_sink.read().clone() {
            sink(session_id, cols, rows);
        }
    }

    pub fn session_layout(&self, session_id: &str) -> Option<SurfaceCompositionLayout> {
        self.sessions
            .lock()
            .get(session_id)
            .and_then(|session| session.layout)
    }

    pub fn session_logical_bounds(&self, session_id: &str) -> Option<LogicalBounds> {
        self.sessions
            .lock()
            .get(session_id)
            .and_then(|session| session.logical_bounds)
    }

    pub fn session_cell_metrics(&self, session_id: &str) -> Option<CellMetrics> {
        self.sessions
            .lock()
            .get(session_id)
            .and_then(|session| session.cell_metrics)
    }

    pub fn set_scrollbar_overlay_visible(
        &self,
        session_id: &str,
        visible: bool,
    ) -> Result<(), NativeTerminalError> {
        validate_session_id(session_id)?;
        let mut sessions = self.sessions.lock();
        let session = sessions
            .get_mut(session_id)
            .ok_or(NativeTerminalError::NoValue)?;
        session.scrollbar_overlay.visible = visible;
        if visible {
            if let Ok(scrollbar) = session.terminal.scrollbar() {
                session.scrollbar_overlay.metrics = Some(scrollbar);
            }
        }
        Ok(())
    }

    pub fn set_attention_frame(
        &self,
        session_id: &str,
        attention: bool,
    ) -> Result<(), NativeTerminalError> {
        validate_session_id(session_id)?;
        let mut sessions = self.sessions.lock();
        let session = sessions
            .get_mut(session_id)
            .ok_or(NativeTerminalError::NoValue)?;
        session.attention_frame = attention;
        Ok(())
    }

    /// Maps DOM logical coordinates against attached session viewports.
    /// Uses half-open bounds `left <= x < right` and `top <= y < bottom` so split
    /// boundaries deterministically map to exactly one pane.
    pub fn session_at_logical_point(&self, lx: f64, ly: f64) -> Option<String> {
        let sessions = self.sessions.lock();
        sessions.iter().find_map(|(id, session)| {
            if session.surface_attached {
                if let Some(bounds) = session.logical_bounds {
                    if bounds.contains(lx, ly) {
                        return Some(id.clone());
                    }
                }
            }
            None
        })
    }

    pub fn with_session_terminal_and_context<T>(
        &self,
        session_id: &str,
        f: impl FnOnce(&mut NativeTerminal, bool) -> Result<T, NativeTerminalError>,
    ) -> Result<T, NativeTerminalError> {
        validate_session_id(session_id)?;
        let mut sessions = self.sessions.lock();
        let session = sessions
            .get_mut(session_id)
            .ok_or(NativeTerminalError::NoValue)?;
        let is_agent = session.last_agent_activity.is_some() || session.agent_reports_own_state;
        let bracketed_effective = session.bracketed_paste_seen || is_agent;
        f(&mut session.terminal, bracketed_effective)
    }

    pub fn with_session_terminal<T>(
        &self,
        session_id: &str,
        f: impl FnOnce(&mut NativeTerminal) -> Result<T, NativeTerminalError>,
    ) -> Result<T, NativeTerminalError> {
        self.with_session_terminal_and_context(session_id, |term, _| f(term))
    }

    pub fn reapply_theme_to_sessions(&self) {
        let theme = &cached_terminal_preferences().theme;
        let mut sessions = self.sessions.lock();
        for (session_id, session) in sessions.iter_mut() {
            if let Err(error) = session.terminal.apply_theme_preferences(theme) {
                tracing::warn!(
                    session_id = %session_id,
                    ?error,
                    "ghostty theme injection failed on reapply; using built-in palette"
                );
            }
        }
    }

    pub fn reapply_scrollback_to_sessions(&self) {
        let scrollback = cached_terminal_preferences().scrollback;
        let mut sessions = self.sessions.lock();
        for (session_id, session) in sessions.iter_mut() {
            if let Err(error) = session
                .terminal
                .set_scrollback_limit_lines(Some(scrollback))
            {
                tracing::warn!(
                    session_id = %session_id,
                    ?error,
                    "ghostty scrollback limit update failed on reapply"
                );
            }
        }
    }

    /// Rejects geometry work for a session with no mounted compositor surface.
    ///
    /// Returns [`NativeTerminalError::SessionDetached`] for both a closed session and one that is
    /// still streaming in the background after its pane unmounted.
    pub fn ensure_surface_attached(&self, session_id: &str) -> Result<(), NativeTerminalError> {
        validate_session_id(session_id)?;
        let sessions = self.sessions.lock();
        match sessions.get(session_id) {
            Some(session) if session.surface_attached => Ok(()),
            Some(_) | None => Err(NativeTerminalError::SessionDetached(session_id.to_string())),
        }
    }

    fn lock_attached_hosts(
        &self,
        session_id: &str,
    ) -> Result<
        parking_lot::MutexGuard<'_, HashMap<String, NativeTerminalSurfaceHost>>,
        NativeTerminalError,
    > {
        let hosts = self.hosts.lock();
        self.ensure_surface_attached(session_id)?;
        Ok(hosts)
    }

    pub fn emit_scrollbar_if_changed<R: Runtime>(
        &self,
        app: Option<&tauri::AppHandle<R>>,
        session_id: &str,
    ) {
        let event = {
            let mut sessions = self.sessions.lock();
            let session = match sessions.get_mut(session_id) {
                Some(s) => s,
                None => return,
            };
            take_native_terminal_scrollbar_event(session, session_id)
        };
        if let Some(event) = event {
            emit_native_terminal_event(app, &self.event_sink, event);
        }
    }

    pub fn prepare_session_layout(
        &self,
        request: NativeTerminalBoundsRequest,
        cell_metrics: CellMetrics,
    ) -> Result<SurfaceCompositionLayout, NativeTerminalError> {
        self.prepare_session_layout_fenced(request, cell_metrics, None)
    }

    fn prepare_session_layout_fenced(
        &self,
        request: NativeTerminalBoundsRequest,
        cell_metrics: CellMetrics,
        token: Option<&ReplayRequestToken>,
    ) -> Result<SurfaceCompositionLayout, NativeTerminalError> {
        let layout = request.layout(cell_metrics)?;
        let mut sessions = self.sessions.lock();
        if let Some(token) = token {
            let session = sessions.get_mut(&request.session_id)
                .filter(|session| session.owns_replay_request(token))
                .ok_or_else(|| NativeTerminalError::SessionDetached(request.session_id.clone()))?;
            session.surface_attached = true;
        }
        let (session, initialized) = match sessions.get_mut(&request.session_id) {
            Some(session) if !session.surface_attached => {
                return Err(NativeTerminalError::SessionDetached(request.session_id));
            }
            Some(session) => (session, false),
            None => {
                let mut terminal = NativeTerminal::new(layout.cols, layout.rows)?;
                let prefs = cached_terminal_preferences();
                if let Err(error) = terminal.apply_theme_preferences(&prefs.theme) {
                    tracing::warn!(
                        ?error,
                        "ghostty theme injection failed; using built-in palette"
                    );
                }
                let _ = terminal.set_scrollback_limit_lines(Some(prefs.scrollback));
                let (update_sender, _) = tokio::sync::watch::channel(());
                let render_coordinator = Arc::new(RenderScheduleCoordinator::new());
                sessions.insert(
                    request.session_id.clone(),
                    NativeTerminalSession {
                        terminal,
                        focused: false,
                        preedit: None,
                        layout: None,
                        logical_bounds: None,
                        cell_metrics: None,
                        stream_task: None,
                        pump_task: None,
                        pty_write_task: None,
                        is_remote: false,
                        remote_generation: None,
                        last_sequence: None,
                        daemon_epoch: 0,
                        pump_generation: 0,
                        active_attach_tuple: None,
                        attempt_generation: 0,
                        incarnation: None,
                        update_sender,
                        detach_sender: tokio::sync::watch::channel(()).0,
                        render_coordinator,
                        last_agent_activity: None,
                        last_provider_session: None,
                        last_agent_detect_at: None,
                        agent_detect_pending: false,
                        last_scrollbar: None,
                        scrollbar_overlay: ScrollbarOverlayState::default(),
                        attention_frame: false,
                        agent_reports_own_state: false,
                        surface_attached: true,
                        output_stream_ended: false,
                        bracketed_paste_seen: false,
                        snapshot_slot: Arc::new(SnapshotSlot::new()),
                    },
                );
                (
                    sessions
                        .get_mut(&request.session_id)
                        .ok_or(NativeTerminalError::NoValue)?,
                    true,
                )
            }
        };

        let prior_scrollbar = session.terminal.scrollbar().ok();
        let is_at_bottom = prior_scrollbar.map_or(true, |sb| {
            let max_offset = sb.total.saturating_sub(sb.len);
            max_offset == 0 || sb.offset >= max_offset.saturating_sub(BOTTOM_LOCK_TOLERANCE_ROWS)
        });
        let prior_scroll_ratio = if is_at_bottom {
            None
        } else {
            prior_scrollbar.and_then(|sb| {
                let max_offset = sb.total.saturating_sub(sb.len);
                if max_offset > 0 {
                    Some(sb.offset as f64 / max_offset as f64)
                } else {
                    None
                }
            })
        };

        let dimensions_changed = session.terminal.dimensions()? != (layout.cols, layout.rows);
        let metrics_changed = session.cell_metrics != Some(cell_metrics);
        let resized = if initialized || dimensions_changed || metrics_changed {
            session.terminal.resize(
                layout.cols,
                layout.rows,
                cell_metrics.width_px,
                cell_metrics.height_px,
            )?;

            if let Some(ratio) = prior_scroll_ratio {
                if let Ok(new_sb) = session.terminal.scrollbar() {
                    let new_max_offset = new_sb.total.saturating_sub(new_sb.len);
                    if new_max_offset > 0 {
                        let target_offset = (ratio * new_max_offset as f64).round() as usize;
                        let _ = session.terminal.scroll_viewport(
                            crate::native_terminal::ScrollViewport::Row(target_offset),
                        );
                    }
                }
            } else {
                let _ = session
                    .terminal
                    .scroll_viewport(crate::native_terminal::ScrollViewport::Bottom);
            }
            dimensions_changed
        } else {
            if is_at_bottom {
                let _ = session
                    .terminal
                    .scroll_viewport(crate::native_terminal::ScrollViewport::Bottom);
            }
            false
        };
        session.layout = Some(layout);
        session.logical_bounds = Some(request.bounds);
        session.cell_metrics = Some(cell_metrics);
        session.snapshot_slot.set_attached(true);
        session.publish_frame();
        drop(sessions);
        if initialized || resized {
            self.notify_pty_resize(&request.session_id, layout.cols, layout.rows);
        }
        Ok(layout)
    }

    fn presentation_geometry_for_session(&self, session_id: &str) -> SurfacePresentationGeometry {
        self.hosts
            .lock()
            .get(session_id)
            .map_or(SurfacePresentationGeometry::Default, |host| {
                host.active_presentation_geometry()
            })
    }

    pub fn attach_daemon_attachment_with_bounds<R: Runtime>(
        &self,
        session_id: &str,
        attachment: DaemonAttachment,
        app: Option<tauri::AppHandle<R>>,
        bounds: Option<LogicalBounds>,
    ) -> Result<(), NativeTerminalError> {
        self.attach_daemon_attachment_with_bounds_and_client(
            session_id, attachment, app, bounds, None,
        )
    }

    pub fn attach_daemon_attachment_with_bounds_and_client<R: Runtime>(
        &self,
        session_id: &str,
        attachment: DaemonAttachment,
        app: Option<tauri::AppHandle<R>>,
        bounds: Option<LogicalBounds>,
        daemon_client: Option<Arc<crate::daemon::DaemonClient>>,
    ) -> Result<(), NativeTerminalError> {
        self.attach_daemon_attachment_with_bounds_client_and_token(
            session_id,
            attachment,
            app,
            bounds,
            daemon_client,
            None,
        )
    }

    pub fn attach_daemon_attachment_with_bounds_client_and_token<R: Runtime>(
        &self,
        session_id: &str,
        attachment: DaemonAttachment,
        app: Option<tauri::AppHandle<R>>,
        bounds: Option<LogicalBounds>,
        daemon_client: Option<Arc<crate::daemon::DaemonClient>>,
        token: Option<ReplayRequestToken>,
    ) -> Result<(), NativeTerminalError> {
        validate_session_id(session_id)?;
        if token.is_none() && self.session_attach_tuple(session_id).is_some() {
            attachment.stream_task.abort();
            return Err(NativeTerminalError::SessionDetached(session_id.to_string()));
        }
        if let Some(bounds) = bounds {
            let layout = self
                .presentation_geometry_for_session(session_id)
                .resolve(bounds)
                .and_then(|bounds| {
                    let metrics = font_manager::derived_cell_metrics_for_scale(bounds.scale_factor);
                    self.prepare_session_layout_fenced(
                        NativeTerminalBoundsRequest {
                            session_id: session_id.to_string(),
                            bounds,
                        },
                        metrics,
                        token.as_ref(),
                    )
                });
            if let Err(error) = layout {
                tracing::warn!(
                    session_id,
                    ?error,
                    "initial bounds layout failed during attach; falling back to default dimensions"
                );
            }
        }
        self.attach_daemon_attachment_with_client_and_token(
            session_id,
            attachment,
            app,
            daemon_client,
            token,
        )
    }

    pub fn reattach_existing_session_with_bounds(
        &self,
        session_id: &str,
        bounds: Option<LogicalBounds>,
    ) -> Result<bool, NativeTerminalError> {
        self.reattach_existing_session_with_bounds_and_tuple(session_id, bounds, None)
    }

    pub fn reattach_existing_session_with_bounds_and_tuple(
        &self,
        session_id: &str,
        bounds: Option<LogicalBounds>,
        tuple: Option<&crate::daemon::protocol::PaneAttachTuple>,
    ) -> Result<bool, NativeTerminalError> {
        validate_session_id(session_id)?;
        // A warm attach may schedule output before the next explicit bounds render. Keep
        // its stored grid and density coherent with the already-created native child.
        let geometry = self.presentation_geometry_for_session(session_id);
        let _hosts = self.hosts.lock();
        let mut sessions = self.sessions.lock();
        let Some(session) = sessions.get_mut(session_id) else {
            return Ok(false);
        };
        if tuple.is_none() && session.active_attach_tuple.is_some() {
            return Err(NativeTerminalError::SessionDetached(session_id.to_string()));
        }
        let stream_is_live = session
            .stream_task
            .as_ref()
            .is_some_and(|task| !task.is_finished());
        let pump_is_live = session
            .pump_task
            .as_ref()
            .is_some_and(|task| !task.is_finished());
        if !stream_is_live || !pump_is_live || session.output_stream_ended {
            return Ok(false);
        }
        if tuple.is_some_and(|tuple| tuple.backend_session_id != session_id
            || session.incarnation.as_ref().is_some_and(|incarnation| tuple.incarnation.as_ref() != Some(incarnation))) {
            return Err(NativeTerminalError::SessionDetached(session_id.to_string()));
        }

        if let Some(tuple) = tuple {
            if let Some(active) = &session.active_attach_tuple {
                if !pane_liveness_registration_matches(active, tuple) {
                    // The older attempt cannot install itself over the live
                    // binding; the rejection is settled at the real fence.
                    #[cfg(feature = "local-split-qa")]
                    emit_stale_registration_rejected_qa(
                        session_id,
                        Some(active),
                        tuple,
                    );
                    return Err(NativeTerminalError::SessionDetached(session_id.to_string()));
                }
            } else if session.attempt_generation > tuple.attempt_generation {
                #[cfg(feature = "local-split-qa")]
                emit_stale_registration_rejected_qa(session_id, None, tuple);
                return Err(NativeTerminalError::SessionDetached(session_id.to_string()));
            }
            if tuple.daemon_epoch != session.daemon_epoch.to_string() {
                return Ok(false);
            }
            if session.active_attach_tuple.as_ref() != Some(tuple) {
                session.snapshot_slot.set_attached(false);
                session.render_coordinator.consume_render();
            }
            session.attempt_generation = tuple.attempt_generation;
            session.incarnation = tuple.incarnation.clone();
            session.active_attach_tuple = Some(tuple.clone());
        }

        session.surface_attached = true;
        session.snapshot_slot.set_attached(true);
        let resized_dimensions = if let Some(bounds) = bounds {
            let layout = match geometry.resolve(bounds).and_then(|bounds| {
                let metrics = font_manager::derived_cell_metrics_for_scale(bounds.scale_factor);
                NativeTerminalBoundsRequest {
                    session_id: session_id.to_string(),
                    bounds,
                }
                .layout(metrics)
                .map(|layout| (bounds, metrics, layout))
            }) {
                Ok(layout) => Some(layout),
                Err(error) => {
                    tracing::warn!(
                        session_id,
                        ?error,
                        "bounds layout failed during reattach; keeping existing dimensions"
                    );
                    None
                }
            };
            if let Some((bounds, metrics, layout)) = layout {
                let prior_scrollbar = session.terminal.scrollbar().ok();
                let is_at_bottom = prior_scrollbar.map_or(true, |sb| {
                    let max_offset = sb.total.saturating_sub(sb.len);
                    max_offset == 0
                        || sb.offset >= max_offset.saturating_sub(BOTTOM_LOCK_TOLERANCE_ROWS)
                });
                let prior_scroll_ratio = if is_at_bottom {
                    None
                } else {
                    prior_scrollbar.and_then(|sb| {
                        let max_offset = sb.total.saturating_sub(sb.len);
                        if max_offset > 0 {
                            Some(sb.offset as f64 / max_offset as f64)
                        } else {
                            None
                        }
                    })
                };

                if session.terminal.dimensions()? != (layout.cols, layout.rows) {
                    session.terminal.resize(
                        layout.cols,
                        layout.rows,
                        metrics.width_px,
                        metrics.height_px,
                    )?;

                    if let Some(ratio) = prior_scroll_ratio {
                        if let Ok(new_sb) = session.terminal.scrollbar() {
                            let new_max_offset = new_sb.total.saturating_sub(new_sb.len);
                            if new_max_offset > 0 {
                                let target_offset =
                                    (ratio * new_max_offset as f64).round() as usize;
                                let _ = session.terminal.scroll_viewport(
                                    crate::native_terminal::ScrollViewport::Row(target_offset),
                                );
                            }
                        }
                    } else {
                        let _ = session
                            .terminal
                            .scroll_viewport(crate::native_terminal::ScrollViewport::Bottom);
                    }
                } else if is_at_bottom {
                    let _ = session
                        .terminal
                        .scroll_viewport(crate::native_terminal::ScrollViewport::Bottom);
                }
                session.layout = Some(layout);
                session.logical_bounds = Some(bounds);
                session.cell_metrics = Some(metrics);
                Some((layout.cols, layout.rows))
            } else {
                None
            }
        } else {
            None
        };
        session.publish_frame();
        session.render_coordinator.consume_render();
        drop(sessions);
        if let Some((cols, rows)) = resized_dimensions {
            self.notify_pty_resize(session_id, cols, rows);
        }
        // `reattach-marker`: a legitimate reattach reinstalled the surface and
        // stream for the SAME backend; only a tuple-bearing reattach settles it,
        // so a plain bounds update never occupies the runner's receipt index.
        #[cfg(feature = "local-split-qa")]
        if let Some(reattached_tuple) = tuple {
            if let Some(channel) = crate::ipc::qa_barrier::active_channel() {
                emit_reattach_marker_qa(
                    &channel,
                    session_id,
                    reattached_tuple,
                    resized_dimensions.is_some(),
                );
            }
        }
        Ok(true)
    }

    pub fn attach_daemon_attachment<R: Runtime>(
        &self,
        session_id: &str,
        attachment: DaemonAttachment,
        app: Option<tauri::AppHandle<R>>,
    ) -> Result<(), NativeTerminalError> {
        self.attach_daemon_attachment_with_client(session_id, attachment, app, None)
    }

    pub fn attach_daemon_attachment_with_client<R: Runtime>(
        &self,
        session_id: &str,
        attachment: DaemonAttachment,
        app: Option<tauri::AppHandle<R>>,
        daemon_client: Option<Arc<crate::daemon::DaemonClient>>,
    ) -> Result<(), NativeTerminalError> {
        self.attach_daemon_attachment_with_client_and_token(
            session_id,
            attachment,
            app,
            daemon_client,
            None,
        )
    }

    /// `token` is the ownership proof minted by [`Self::begin_replay_request`]. When present, the
    /// attach refuses to install its stream unless the token still owns the session, so a replay
    /// fetched against a cursor that a newer attach has already superseded is dropped instead of
    /// being applied on top of the newer state.
    pub fn attach_daemon_attachment_with_client_and_token<R: Runtime>(
        &self,
        session_id: &str,
        attachment: DaemonAttachment,
        app: Option<tauri::AppHandle<R>>,
        daemon_client: Option<Arc<crate::daemon::DaemonClient>>,
        token: Option<ReplayRequestToken>,
    ) -> Result<(), NativeTerminalError> {
        validate_session_id(session_id)?;

        let is_remote = attachment.remote_generation.is_some();
        let initial_generation = attachment.remote_generation;
        let initial_dims = (80, 24);

        let (update_sender, render_coordinator, events, slot, pump_generation, absorbs_replay_burst) = {
            let mut sessions = self.sessions.lock();
            // The ownership check lives INSIDE the mutation lock. Checking it under a separate
            // lock first would leave a window where a newer attach takes ownership between the
            // check and the mutation, which is the very race the token exists to close. A missing
            // session is a rejection too: the pane was closed, and recreating it here would
            // resurrect a session the user already dismissed.
            if let Some(ref token) = token {
                let Some(session) = sessions.get(session_id) else {
                    drop(sessions);
                    attachment.stream_task.abort();
                    return Err(NativeTerminalError::SessionDetached(session_id.to_string()));
                };
                if !session.owns_replay_request(token) {
                    drop(sessions);
                    attachment.stream_task.abort();
                    return Err(NativeTerminalError::SessionDetached(session_id.to_string()));
                }
                if let Some(token_gen) = token.attempt_generation {
                    if session.attempt_generation > token_gen
                        || (session.attempt_generation == token_gen
                            && token.attach_tuple.is_none()
                            && token.generation != session.pump_generation)
                    {
                        drop(sessions);
                        attachment.stream_task.abort();
                        return Err(NativeTerminalError::SessionDetached(session_id.to_string()));
                    }
                }
                if let Some(ref token_tuple) = token.attach_tuple {
                    if let Some(ref active_tuple) = session.active_attach_tuple {
                        if active_tuple.attempt_generation > token_tuple.attempt_generation
                            || (active_tuple.attempt_generation == token_tuple.attempt_generation
                                && !pane_liveness_tuple_matches(active_tuple, token_tuple))
                        {
                            drop(sessions);
                            attachment.stream_task.abort();
                            return Err(NativeTerminalError::SessionDetached(session_id.to_string()));
                        }
                    } else if session.attempt_generation > token_tuple.attempt_generation {
                        drop(sessions);
                        attachment.stream_task.abort();
                        return Err(NativeTerminalError::SessionDetached(session_id.to_string()));
                    }
                }
            }
            if token.is_none() && sessions.get(session_id).is_some_and(|session| session.active_attach_tuple.is_some()) {
                attachment.stream_task.abort();
                return Err(NativeTerminalError::SessionDetached(session_id.to_string()));
            }
            let (update_sender, render_coordinator) = if let Some(session) =
                sessions.get_mut(session_id)
            {
                let is_fresh_startup = self.consume_pending_startup(session_id);
                if let Some(ref token) = token {
                    if let Some(ref tuple) = token.attach_tuple {
                        session.attempt_generation = tuple.attempt_generation;
                        session.incarnation = tuple.incarnation.clone();
                        session.active_attach_tuple = Some(tuple.clone());
                    } else if let Some(gen) = token.attempt_generation {
                        session.attempt_generation = gen;
                    }
                }
                // A backgrounded session kept streaming without a surface; this attach gives it
                // one again and re-enables geometry updates.
                session.surface_attached = true;
                session.snapshot_slot.set_attached(true);
                session.output_stream_ended = false;
                if let Some(task) = session.stream_task.take() {
                    task.abort();
                }
                if let Some(task) = session.pump_task.take() {
                    task.abort();
                }
                if let Some(task) = session.pty_write_task.take() {
                    task.abort();
                }
                if is_remote {
                    session.is_remote = true;
                    if initial_generation.is_some() {
                        session.remote_generation = initial_generation;
                        session.terminal.set_remote_generation(initial_generation);
                    }
                }
                if attachment.history_segments.is_empty() {
                    if let (Some(cols), Some(rows)) = (attachment.pty_cols, attachment.pty_rows) {
                        if session.terminal.dimensions()? != (cols, rows) {
                            let metrics = session
                                .cell_metrics
                                .unwrap_or_else(font_manager::derived_cell_metrics);
                            session.terminal.resize(
                                cols,
                                rows,
                                metrics.width_px,
                                metrics.height_px,
                            )?;
                        }
                    }
                }
                let was_bracketed = session.bracketed_paste_seen
                    || session.terminal.bracketed_paste_enabled().unwrap_or(false);
                // Aborting the old pump above is not enough: it may already be inside a message,
                // holding no lock but about to write a sequence from the previous stream. Bumping
                // the generation under this same lock fences those late writes out.
                session.pump_generation = session.pump_generation.wrapping_add(1);
                let epoch_changed = session.daemon_epoch != attachment.epoch;
                session.daemon_epoch = attachment.epoch;
                let verdict = if is_fresh_startup || epoch_changed {
                    ReplayVerdict::Rebuild
                } else {
                    classify_replay(
                        session.last_sequence,
                        attachment.start_sequence,
                        attachment.end_sequence,
                        Some(attachment.gap.is_none()),
                    )
                };
                let was_at_bottom = session.terminal.scrollbar().ok().map_or(true, |sb| {
                    let max_offset = sb.total.saturating_sub(sb.len);
                    max_offset == 0
                        || sb.offset >= max_offset.saturating_sub(BOTTOM_LOCK_TOLERANCE_ROWS)
                });
                if !is_fresh_startup {
                    // An existing resident session re-attaching is display reconstruction:
                    // suppress and discard buffered writes so scrollback queries are not re-emitted.
                    session.terminal.discard_buffered_pty_writes();
                    session.terminal.set_pty_writes_suppressed(true);
                    if verdict == ReplayVerdict::Rebuild {
                        session.terminal.reset();
                    }
                    if verdict != ReplayVerdict::NoOp {
                        feed_attachment_history(
                            &mut session.terminal,
                            &attachment.history,
                            &attachment.history_segments,
                        )?;
                    }
                    session.terminal.set_pty_writes_suppressed(false);
                    session.terminal.discard_buffered_pty_writes();
                } else {
                    // Fresh startup (even if prepare_session_layout created the session placeholder):
                    // do NOT suppress or discard PTY writes!
                    feed_attachment_history(
                        &mut session.terminal,
                        &attachment.history,
                        &attachment.history_segments,
                    )?;
                }
                if was_bracketed && !session.terminal.bracketed_paste_enabled().unwrap_or(false) {
                    let _ = session.terminal.feed_str("\x1b[?2004h");
                    session.bracketed_paste_seen = true;
                }
                if let Some(layout) = session.layout {
                    if session.terminal.dimensions()? != (layout.cols, layout.rows) {
                        let metrics = session
                            .cell_metrics
                            .unwrap_or_else(font_manager::derived_cell_metrics);
                        session.terminal.resize(
                            layout.cols,
                            layout.rows,
                            metrics.width_px,
                            metrics.height_px,
                        )?;
                    }
                }
                if was_at_bottom || verdict == ReplayVerdict::Rebuild {
                    let _ = session
                        .terminal
                        .scroll_viewport(crate::native_terminal::ScrollViewport::Bottom);
                }
                // Sequences only order within one epoch, and a stale snapshot may end below what
                // the grid already applied; neither may rewind the replay cursor. A new epoch
                // discards the old cursor outright, including when it carries no end of its own:
                // keeping a number from a retired counter would fence out the new epoch's output.
                session.last_sequence = if epoch_changed {
                    attachment.end_sequence
                } else {
                    match (session.last_sequence, attachment.end_sequence) {
                        (Some(applied), Some(offered)) => Some(applied.max(offered)),
                        (applied, None) => applied,
                        (None, offered) => offered,
                    }
                };
                // The re-fed history is the session's whole visible state. Cancelling the
                // scheduled render without republishing would leave the slot holding the
                // pre-attach frame, so the first paint after attach shows stale content.
                session.publish_frame();
                session.render_coordinator.consume_render();
                (
                    session.update_sender.clone(),
                    Arc::clone(&session.render_coordinator),
                )
            } else {
                let is_fresh_startup = self.consume_pending_startup(session_id);
                let initial_cols = attachment.pty_cols.unwrap_or(initial_dims.0);
                let initial_rows = attachment.pty_rows.unwrap_or(initial_dims.1);
                let mut terminal = NativeTerminal::new(initial_cols, initial_rows)?;
                let prefs = cached_terminal_preferences();
                if let Err(error) = terminal.apply_theme_preferences(&prefs.theme) {
                    tracing::warn!(
                        ?error,
                        "ghostty theme injection failed; using built-in palette"
                    );
                }
                let _ = terminal.set_scrollback_limit_lines(Some(prefs.scrollback));
                terminal.set_remote_generation(initial_generation);
                if !is_fresh_startup {
                    // Display reconstruction of an existing session: suppress PTY writes
                    terminal.set_pty_writes_suppressed(true);
                    feed_attachment_history(
                        &mut terminal,
                        &attachment.history,
                        &attachment.history_segments,
                    )?;
                    terminal.discard_buffered_pty_writes();
                    terminal.set_pty_writes_suppressed(false);
                } else {
                    feed_attachment_history(
                        &mut terminal,
                        &attachment.history,
                        &attachment.history_segments,
                    )?;
                }
                let _ = terminal.scroll_viewport(crate::native_terminal::ScrollViewport::Bottom);
                let bracketed_paste_seen = terminal.bracketed_paste_enabled().unwrap_or(false);
                // A fresh session created by an attach owns a surface by definition.
                let (update_sender, _) = tokio::sync::watch::channel(());
                let render_coordinator = Arc::new(RenderScheduleCoordinator::new());
                let initial_pump_gen = token
                    .as_ref()
                    .map(|token| token.generation)
                    .unwrap_or(0);
                let (tuple, attempt_gen, incarnation) = match &token {
                    Some(t) => (
                        t.attach_tuple.clone(),
                        t.attempt_generation.unwrap_or(0),
                        t.attach_tuple.as_ref().and_then(|at| at.incarnation.clone()),
                    ),
                    None => (None, 0, None),
                };
                sessions.insert(
                    session_id.to_string(),
                    NativeTerminalSession {
                        terminal,
                        focused: false,
                        preedit: None,
                        layout: None,
                        logical_bounds: None,
                        cell_metrics: None,
                        stream_task: None,
                        pump_task: None,
                        pty_write_task: None,
                        is_remote,
                        remote_generation: initial_generation,
                        last_sequence: attachment.end_sequence,
                        daemon_epoch: attachment.epoch,
                        pump_generation: initial_pump_gen,
                        active_attach_tuple: tuple,
                        attempt_generation: attempt_gen,
                        incarnation,
                        update_sender: update_sender.clone(),
                        detach_sender: tokio::sync::watch::channel(()).0,
                        render_coordinator: Arc::clone(&render_coordinator),
                        last_agent_activity: None,
                        last_provider_session: None,
                        last_agent_detect_at: None,
                        agent_detect_pending: false,
                        last_scrollbar: None,
                        scrollbar_overlay: ScrollbarOverlayState::default(),
                        attention_frame: false,
                        agent_reports_own_state: false,
                        surface_attached: true,
                        bracketed_paste_seen,
                        output_stream_ended: false,
                        snapshot_slot: Arc::new(SnapshotSlot::new()),
                    },
                );
                (update_sender, render_coordinator)
            };
            let session = sessions
                .get_mut(session_id)
                .ok_or(NativeTerminalError::NoValue)?;
            let events = take_native_terminal_events(session, session_id, true);
            // Captured from the SAME transaction that mutated the session. Re-reading these under
            // a fresh lock would sample state a newer attach may already own, so the pump would
            // run with a generation that never fences it.
            let slot = Arc::clone(&session.snapshot_slot);
            let pump_generation = session.pump_generation;
            let absorbs_replay_burst = session.is_remote;
            (
                update_sender,
                render_coordinator,
                events,
                slot,
                pump_generation,
                absorbs_replay_burst,
            )
        };

        for event in events {
            emit_native_terminal_event(app.as_ref(), &self.event_sink, event);
        }

        let stream_task = attachment.stream_task;
        let mut messages = attachment.messages;
        let sessions = Arc::clone(&self.sessions);
        let hosts = Arc::clone(&self.hosts);
        let event_sink = Arc::clone(&self.event_sink);
        let session_id_owned = session_id.to_string();
        let app_handle = app.clone();
        let gpu_worker = Arc::clone(&self.gpu_worker);
        let pump_task = tokio::spawn(async move {
            let schedule_render = || {
                if render_coordinator.schedule_render() {
                    if let Some(window) = app_handle.as_ref().and_then(|app| app.get_window("main"))
                    {
                        dispatch_scheduled_render(
                            window,
                            Arc::clone(&hosts),
                            Arc::clone(&slot),
                            session_id_owned.clone(),
                            Arc::clone(&render_coordinator),
                            Arc::clone(&gpu_worker),
                        );
                    } else {
                        render_coordinator.consume_render();
                    }
                }
            };
            let lock_to_bottom = || {
                let mut sessions_guard = sessions.lock();
                lock_replay_to_bottom(&mut sessions_guard, &session_id_owned, pump_generation);
            };
            let mut replay_absorb = absorbs_replay_burst.then(|| {
                let now = std::time::Instant::now();
                (now, now)
            });
            loop {
                let deadline = sessions
                    .lock()
                    .get(&session_id_owned)
                    .and_then(|session| session.terminal.synchronized_output_deadline());
                let received = tokio::select! {
                    biased;
                    _ = async {
                        match deadline {
                            Some(deadline) => tokio::time::sleep_until(deadline).await,
                            None => std::future::pending::<()>().await,
                        }
                    } => {
                        let mut sessions_guard = sessions.lock();
                        if let Some(session) = sessions_guard
                            .get_mut(&session_id_owned)
                            .filter(|session| session.pump_generation == pump_generation)
                        {
                            if let Err(error) = session.terminal.expire_synchronized_output(tokio::time::Instant::now()) {
                                tracing::warn!(session_id = %session_id_owned, %error, "Failed to end synchronized output");
                            }
                            session.publish_frame();
                        }
                        drop(sessions_guard);
                        update_sender.send_replace(());
                        schedule_render();
                        continue;
                    }
                    result = tokio::time::timeout(AGENT_DETECT_TRAILING_IDLE, messages.recv()) => result,
                };
                let msg = match received {
                    Ok(Some(msg)) => msg,
                    Ok(None) => {
                        let mut sessions_guard = sessions.lock();
                        let mut owned_stream_ended = false;
                        if let Some(session) = sessions_guard
                            .get_mut(&session_id_owned)
                            .filter(|session| session.pump_generation == pump_generation)
                        {
                            if let Err(error) = session.terminal.finish_synchronized_output() {
                                tracing::warn!(session_id = %session_id_owned, %error, "Failed to finish terminal output");
                            }
                            session.output_stream_ended = true;
                            session.publish_frame();
                            owned_stream_ended = true;
                        }
                        drop(sessions_guard);
                        if owned_stream_ended {
                            if let Some(app) = app_handle.as_ref() {
                                if let Err(error) = app.emit("native_terminal_stream_ended", NativeTerminalFocusPayload {
                                    session_id: session_id_owned.clone(),
                                }) {
                                    tracing::warn!(session_id = %session_id_owned, %error, "Failed to notify terminal stream disconnect");
                                }
                            }
                        }
                        update_sender.send_replace(());
                        schedule_render();
                        break;
                    }
                    Err(_) => {
                        // Burst went quiet: re-run any detection the throttle skipped so the
                        // final frame of a burst (often the agent's last word before it blocks
                        // on input) still produces a state transition.
                        let (detected, events) = {
                            let mut sessions_guard = sessions.lock();
                            match sessions_guard.get_mut(&session_id_owned) {
                                Some(sess)
                                    if sess.pump_generation == pump_generation
                                        && sess.agent_detect_pending =>
                                {
                                    (
                                        true,
                                        take_native_terminal_events(sess, &session_id_owned, true),
                                    )
                                }
                                Some(_) => (false, Vec::new()),
                                None => (false, Vec::new()),
                            }
                        };
                        for event in events {
                            emit_native_terminal_event(app_handle.as_ref(), &event_sink, event);
                        }
                        if detected {
                            update_sender.send_replace(());
                        }
                        if let Some((started_at, last_output_at)) = replay_absorb {
                            let now = std::time::Instant::now();
                            if !replay_absorb_continues(
                                now.duration_since(started_at),
                                now.duration_since(last_output_at),
                            ) {
                                replay_absorb = None;
                                lock_to_bottom();
                                schedule_render();
                            }
                        }
                        continue;
                    }
                };
                match msg {
                    DaemonStreamMessage::Output { sequence, data, .. } => {
                        let (session_exists, events) = {
                            let mut sessions_guard = sessions.lock();
                            if let Some(sess) = sessions_guard
                                .get_mut(&session_id_owned)
                                .filter(|sess| sess.pump_generation == pump_generation)
                                // A chunk at or below the watermark was already applied by the
                                // replay this pump was started from. Feeding it would duplicate
                                // output; adopting its sequence would rewind the cursor.
                                .filter(|sess| {
                                    sess.last_sequence.is_none_or(|applied| sequence > applied)
                                })
                            {
                                if let Err(err) = sess.terminal.feed(&data) {
                                    tracing::warn!(
                                        session_id = %session_id_owned,
                                        error = %err,
                                        "Failed to feed daemon output to native terminal"
                                    );
                                }
                                if let Ok(true) = sess.terminal.bracketed_paste_enabled() {
                                    sess.bracketed_paste_seen = true;
                                }
                                sess.last_sequence = Some(sequence);
                                sess.publish_frame();
                                crate::ipc::debug::log_native_switch_debug(serde_json::json!({
                                    "event": "terminal.render.vt_consumed",
                                    "details": {
                                        "sessionId": session_id_owned,
                                        "consumedSequence": sequence,
                                        "renderedAt": std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .ok()
                                            .map(|d| d.as_secs_f64() * 1000.0),
                                    }
                                }));
                                (
                                    true,
                                    take_native_terminal_events(sess, &session_id_owned, false),
                                )
                            } else {
                                (false, Vec::new())
                            }
                        };

                        for event in events {
                            emit_native_terminal_event(app_handle.as_ref(), &event_sink, event);
                        }
                        if session_exists {
                            update_sender.send_replace(());
                        }

                        if session_exists {
                            let now = std::time::Instant::now();
                            let absorbing = match replay_absorb {
                                Some((started_at, last_output_at))
                                    if replay_absorb_continues(
                                        now.duration_since(started_at),
                                        now.duration_since(last_output_at),
                                    ) =>
                                {
                                    replay_absorb = Some((started_at, now));
                                    true
                                }
                                Some(_) => {
                                    replay_absorb = None;
                                    lock_to_bottom();
                                    false
                                }
                                None => false,
                            };
                            if !absorbing {
                                schedule_render();
                            }
                        }
                    }
                    DaemonStreamMessage::Lagged {
                        history,
                        segments,
                        start_sequence,
                        end_sequence,
                        replay_is_delta,
                        ..
                    } => {
                        let (session_exists, events) = {
                            let mut sessions_guard = sessions.lock();
                            if let Some(sess) = sessions_guard
                                .get_mut(&session_id_owned)
                                .filter(|sess| sess.pump_generation == pump_generation)
                            {
                                let verdict = classify_replay(
                                    sess.last_sequence,
                                    start_sequence,
                                    end_sequence,
                                    replay_is_delta,
                                );
                                let was_at_bottom =
                                    sess.terminal.scrollbar().ok().map_or(true, |sb| {
                                        let max_offset = sb.total.saturating_sub(sb.len);
                                        max_offset == 0
                                            || sb.offset >= max_offset
                                                .saturating_sub(BOTTOM_LOCK_TOLERANCE_ROWS)
                                    });
                                let was_bracketed = sess.bracketed_paste_seen
                                    || sess.terminal.bracketed_paste_enabled().unwrap_or(false);
                                sess.terminal.set_pty_writes_suppressed(true);
                                if verdict == ReplayVerdict::Rebuild {
                                    // Keep the grid and scrollback: a reset wiped every line the
                                    // user was reading. Return the VT parser to ground (CAN, ST,
                                    // SGR 0; close any open synchronized update) and mark the
                                    // seam, the way the Gap arm does, then replay on top.
                                    let _ = sess.terminal.feed(b"\x18\x1b\\\x1b[0m");
                                    let _ = sess.terminal.finish_synchronized_output();
                                    let _ = sess.terminal.feed_str(
                                        "\r\n\x1b[33m[Ferryx: output stream resynced — output below may repeat lines above]\x1b[0m\r\n",
                                    );
                                }
                                let parsed_segments: Vec<HistorySegment> = segments
                                    .into_iter()
                                    .map(|s| HistorySegment {
                                        cols: s.cols,
                                        rows: s.rows,
                                        bytes: s.bytes.to_vec(),
                                    })
                                    .collect();
                                if verdict != ReplayVerdict::NoOp {
                                    if let Err(err) = feed_attachment_history(
                                        &mut sess.terminal,
                                        &history,
                                        &parsed_segments,
                                    ) {
                                        tracing::warn!(
                                            session_id = %session_id_owned,
                                            error = %err,
                                            "Failed to feed recovery history to native terminal"
                                        );
                                    }
                                }
                                sess.terminal.set_pty_writes_suppressed(false);
                                sess.terminal.discard_buffered_pty_writes();
                                if was_bracketed
                                    && !sess.terminal.bracketed_paste_enabled().unwrap_or(false)
                                {
                                    let _ = sess.terminal.feed_str("\x1b[?2004h");
                                    sess.bracketed_paste_seen = true;
                                }
                                if let Some(end) = end_sequence {
                                    sess.last_sequence =
                                        Some(sess.last_sequence.map_or(end, |last| last.max(end)));
                                }
                                sess.publish_frame();
                                // feed_attachment_history leaves the grid at the last
                                // segment's dimensions; restore the pane's layout size so the
                                // Bottom lock below applies to the on-screen grid.
                                if let Some(layout) = sess.layout {
                                    if let Ok(dims) = sess.terminal.dimensions() {
                                        if dims != (layout.cols, layout.rows) {
                                            let metrics = sess
                                                .cell_metrics
                                                .unwrap_or_else(font_manager::derived_cell_metrics);
                                            let _ = sess.terminal.resize(
                                                layout.cols,
                                                layout.rows,
                                                metrics.width_px,
                                                metrics.height_px,
                                            );
                                        }
                                    }
                                }
                                if was_at_bottom {
                                    let _ = sess.terminal.scroll_viewport(
                                        crate::native_terminal::ScrollViewport::Bottom,
                                    );
                                }
                                (
                                    true,
                                    take_native_terminal_events(sess, &session_id_owned, true),
                                )
                            } else {
                                (false, Vec::new())
                            }
                        };
                        for event in events {
                            emit_native_terminal_event(app_handle.as_ref(), &event_sink, event);
                        }
                        if session_exists {
                            schedule_render();
                            update_sender.send_replace(());
                        }
                    }
                    DaemonStreamMessage::Gap {
                        available_from_sequence,
                        ..
                    } => {
                        let (session_exists, events) = {
                            let mut sessions_guard = sessions.lock();
                            if let Some(sess) = sessions_guard
                                .get_mut(&session_id_owned)
                                .filter(|sess| sess.pump_generation == pump_generation)
                            {
                                let was_at_bottom =
                                    sess.terminal.scrollbar().ok().map_or(true, |sb| {
                                        let max_offset = sb.total.saturating_sub(sb.len);
                                        max_offset == 0
                                            || sb.offset >= max_offset
                                                .saturating_sub(BOTTOM_LOCK_TOLERANCE_ROWS)
                                    });
                                let was_bracketed = sess.bracketed_paste_seen
                                    || sess.terminal.bracketed_paste_enabled().unwrap_or(false);
                                sess.terminal.set_pty_writes_suppressed(true);
                                // Sanitize parser state without obliterating the screen or scrollback:
                                // 1. \x18 (CAN) resets VT parser state to ground from any escape/CSI/OSC/DCS/APC.
                                // 2. \x1b\ (ST) terminates any open string sequence cleanly.
                                // 3. \x1b[0m (SGR 0) resets graphic rendition attributes to normal defaults.
                                // 4. finish_synchronized_output() exits mode 2026 if an open transaction was interrupted.
                                let _ = sess.terminal.feed(b"\x18\x1b\\\x1b[0m");
                                let _ = sess.terminal.finish_synchronized_output();
                                let _ = sess.terminal.feed_str(
                                    "\r\n\x1b[33m[Ferryx: output stream gap — some terminal output was missed]\x1b[0m\r\n",
                                );
                                sess.terminal.set_pty_writes_suppressed(false);
                                sess.terminal.discard_buffered_pty_writes();
                                if was_bracketed
                                    && !sess.terminal.bracketed_paste_enabled().unwrap_or(false)
                                {
                                    let _ = sess.terminal.feed_str("\x1b[?2004h");
                                    sess.bracketed_paste_seen = true;
                                }
                                if was_at_bottom {
                                    let _ = sess.terminal.scroll_viewport(
                                        crate::native_terminal::ScrollViewport::Bottom,
                                    );
                                }
                                sess.last_sequence = Some(
                                    sess.last_sequence
                                        .map_or(available_from_sequence.saturating_sub(1), |last| {
                                            last.max(available_from_sequence.saturating_sub(1))
                                        }),
                                );
                                sess.publish_frame();
                                (
                                    true,
                                    take_native_terminal_events(sess, &session_id_owned, true),
                                )
                            } else {
                                (false, Vec::new())
                            }
                        };
                        for event in events {
                            emit_native_terminal_event(app_handle.as_ref(), &event_sink, event);
                        }
                        if session_exists {
                            schedule_render();
                            update_sender.send_replace(());
                        }
                    }
                    DaemonStreamMessage::AgentState {
                        state,
                        agent,
                        provider_session,
                        detail,
                        is_snapshot,
                        origin,
                        ..
                    } => {
                        let reported = match state.as_ref() {
                            "working" => Some(crate::agent_detect::AgentActivity::Working),
                            "blocked" => Some(crate::agent_detect::AgentActivity::Blocked),
                            "idle" => Some(crate::agent_detect::AgentActivity::Idle),
                            _ => None,
                        };
                        if let Some(reported) = reported {
                            let changed = {
                                let mut sessions_guard = sessions.lock();
                                match sessions_guard
                                    .get_mut(&session_id_owned)
                                    .filter(|sess| sess.pump_generation == pump_generation)
                                {
                                    Some(sess) => {
                                        // Screen inference is the FALLBACK, not a tiebreaker, and
                                        // this flag suppresses it. Two producers claim the
                                        // session, each for its own reason:
                                        //  - the agent itself, for as long as its process lives,
                                        //    including the `idle` it reports between turns (its
                                        //    last frame is still on screen);
                                        //  - a process release, because the screen the exited
                                        //    agent left behind still shows its spinner and would
                                        //    otherwise resurrect the activity that just ended.
                                        // A later process sighting is what lifts the suppression:
                                        // a new agent is running, so its screen is live evidence
                                        // again until it reports for itself.
                                        sess.agent_reports_own_state = match origin {
                                            crate::daemon::protocol::AgentStateOrigin::Agent
                                            | crate::daemon::protocol::AgentStateOrigin::ProcessReleased => true,
                                            crate::daemon::protocol::AgentStateOrigin::ProcessObserved
                                            | crate::daemon::protocol::AgentStateOrigin::ManualReset => false,
                                        };
                                        // `/new` gives the pane a new conversation while the
                                        // activity state stays put, so a rotated provider session
                                        // is a change in its own right.
                                        let provider_rotated = provider_session.is_some()
                                            && sess.last_provider_session != provider_session;
                                        if provider_rotated {
                                            sess.last_provider_session = provider_session.clone();
                                        }
                                        let repeats = sess.last_agent_activity == Some(reported);
                                        // The reported state is the new inference baseline either
                                        // way: a released session that keeps its old activity here
                                        // would replay it from the exited agent's leftover screen.
                                        sess.last_agent_activity = Some(reported);
                                        !(!is_snapshot && repeats && !provider_rotated)
                                    }
                                    None => false,
                                }
                            };
                            if changed {
                                emit_native_terminal_event(
                                    app_handle.as_ref(),
                                    &event_sink,
                                    NativeTerminalEvent::AgentState(
                                        NativeTerminalAgentStatePayload {
                                            session_id: session_id_owned.clone(),
                                            state: state.to_string(),
                                            rule_id: String::new(),
                                            manifest_id: agent
                                                .as_deref()
                                                .unwrap_or(AGENT_EXTENSION_MANIFEST_ID)
                                                .to_string(),
                                            provider_session: provider_session.clone(),
                                            detail: detail.as_ref().map(|value| value.to_string()),
                                            is_snapshot,
                                        },
                                    ),
                                );
                            }
                        }
                    }
                    DaemonStreamMessage::RemoteStatus {
                        state,
                        generation,
                        failure,
                        replay_gap,
                        ..
                    } => {
                        {
                            let mut sessions_guard = sessions.lock();
                            if let Some(sess) = sessions_guard
                                .get_mut(&session_id_owned)
                                .filter(|sess| sess.pump_generation == pump_generation)
                            {
                                sess.is_remote = true;
                                sess.remote_generation = Some(generation);
                                sess.terminal.set_remote_generation(Some(generation));
                            }
                        }
                        if let Some(app) = app_handle.as_ref() {
                            if let Err(error) = app.emit(
                                "terminal_remote_status",
                                serde_json::json!({
                                    "sessionId": session_id_owned,
                                    "state": state,
                                    "generation": generation,
                                    "failure": failure,
                                    "replayGap": replay_gap,
                                }),
                            ) {
                                tracing::warn!(%error, "Failed to emit remote terminal status");
                            }
                        }
                    }
                    DaemonStreamMessage::DagRunUpdated { .. }
                    | DaemonStreamMessage::DagInventory { .. } => {}
                    DaemonStreamMessage::Exit { .. } => {
                        update_sender.send_replace(());
                        break;
                    }
                }
            }
        });

        let mut pty_write_sender = None;
        let pty_write_task = if let Some(daemon_client) = daemon_client {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<
                crate::native_terminal::bell::PtyWriteRecord,
            >();
            pty_write_sender = Some(tx);
            let client = daemon_client.clone();
            let sessions = Arc::clone(&self.sessions);
            let id = session_id.to_string();
            let app_handle_for_writer = app.as_ref().map(|a| a.clone());
            Some(tokio::spawn(async move {
                while let Some(record) = rx.recv().await {
                    let crate::native_terminal::bell::PtyWriteRecord {
                        generation: orig_gen,
                        data,
                    } = record;
                    let (is_remote, current_gen) = {
                        let guard = sessions.lock();
                        let sess = guard.get(&id);
                        (
                            sess.map(|s| s.is_remote).unwrap_or(false),
                            sess.and_then(|s| s.remote_generation),
                        )
                    };

                    if is_remote || orig_gen.is_some() {
                        let target_gen = match orig_gen {
                            Some(gen) => {
                                if current_gen != Some(gen) {
                                    tracing::warn!(
                                        session_id = %id,
                                        orig_gen = gen,
                                        current_gen = ?current_gen,
                                        "Discarding terminal pty write because originating generation no longer matches"
                                    );
                                    continue;
                                }
                                gen
                            }
                            None => {
                                // Startup reply generated before generation was observed:
                                // wait up to 2 seconds for pump_task to install remote_generation.
                                let start = tokio::time::Instant::now();
                                let mut resolved_gen = None;
                                while tokio::time::Instant::now() - start
                                    < std::time::Duration::from_millis(2000)
                                {
                                    if let Some(gen) =
                                        sessions.lock().get(&id).and_then(|s| s.remote_generation)
                                    {
                                        resolved_gen = Some(gen);
                                        break;
                                    }
                                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                                }
                                let Some(gen) = resolved_gen else {
                                    tracing::warn!(
                                        session_id = %id,
                                        "Timed out waiting for remote generation to deliver startup pty write"
                                    );
                                    continue;
                                };
                                gen
                            }
                        };

                        let deadline =
                            tokio::time::Instant::now() + std::time::Duration::from_millis(2000);
                        let mut backoff_ms = 5u64;
                        loop {
                            match client
                                .write_terminal_at_generation(&id, Some(target_gen), data.clone())
                                .await
                            {
                                Ok(()) => break,
                                Err(err) if is_busy_error(&err) => {
                                    let current =
                                        sessions.lock().get(&id).and_then(|s| s.remote_generation);
                                    if current != Some(target_gen) {
                                        tracing::warn!(
                                            session_id = %id,
                                            "Discarding terminal pty write after remote generation changed during busy retry"
                                        );
                                        break;
                                    }
                                    if tokio::time::Instant::now() >= deadline {
                                        tracing::error!(
                                            session_id = %id,
                                            "Exhausted busy retries (2s) delivering VT response to remote session; remote control stalled"
                                        );
                                        if let Some(app) = app_handle_for_writer.as_ref() {
                                            let _ = app.emit("terminal_remote_status", serde_json::json!({
                                                "sessionId": id,
                                                "state": "disconnected",
                                                "generation": target_gen,
                                                "replayGap": null,
                                                "failure": { "kind": "network", "message": "Remote terminal control connection timed out" },
                                            }));
                                        }
                                        break;
                                    }
                                    tokio::time::sleep(std::time::Duration::from_millis(
                                        backoff_ms,
                                    ))
                                    .await;
                                    backoff_ms = (backoff_ms * 2).min(50);
                                }
                                Err(err) => {
                                    tracing::warn!(
                                        session_id = %id,
                                        %err,
                                        "Failed to deliver terminal pty write to remote session"
                                    );
                                    break;
                                }
                            }
                        }
                    } else {
                        // Local session: deliver locally with None
                        let _ = client.write_terminal_at_generation(&id, None, data).await;
                    }
                }
            }))
        } else {
            None
        };

        {
            let mut sessions = self.sessions.lock();
            // Install only if this attach still owns the session. A newer attach may have bumped
            // the generation while the pump/writer tasks were being spawned; overwriting its
            // handles here would abandon the live stream and leave the pane fed by a pump this
            // attach no longer has any claim to.
            let owns = sessions.get(session_id).is_some_and(|session| {
                session.pump_generation == pump_generation
                    && session.active_attach_tuple.as_ref() == token.as_ref().and_then(|token| token.attach_tuple.as_ref())
            });
            if !owns {
                drop(sessions);
                stream_task.abort();
                pump_task.abort();
                if let Some(task) = pty_write_task {
                    task.abort();
                }
                return Err(NativeTerminalError::SessionDetached(session_id.to_string()));
            }
        if let Some(session) = sessions.get_mut(session_id) {
                if let Some(sender) = pty_write_sender {
                    session.terminal.set_pty_write_sender(sender);
                }
                session.stream_task = Some(stream_task);
                session.pump_task = Some(pump_task);
                session.pty_write_task = pty_write_task;
            }
        }

        Ok(())
    }

    /// Releases the GPU surface for an unmounted pane while KEEPING the terminal session and its
    /// daemon pump alive, so a backgrounded agent keeps reporting title/bell/agent-state instead of
    /// freezing on its last observed value. Use [`Self::close_session`] to discard the session.
    pub fn detach_session(&self, session_id: &str) {
        self.detach_session_fenced(session_id, None);
    }

    pub fn detach_session_fenced(
        &self,
        session_id: &str,
        tuple: Option<&crate::daemon::protocol::PaneAttachTuple>,
    ) -> bool {
        let mut hosts = self.hosts.lock();
        let mut sessions = self.sessions.lock();
        let Some(session) = sessions.get_mut(session_id) else {
            return false;
        };
        if session.active_attach_tuple.as_ref() != tuple {
            return false;
        }
        session.focused = false;
        session.layout = None;
        session.logical_bounds = None;
        session.surface_attached = false;
        session.snapshot_slot.set_attached(false);
        session.last_agent_detect_at = None;
        session.render_coordinator.consume_render();
        session.detach_sender.send_replace(());
        hosts.remove(session_id);
        true
    }

    /// Discards a session entirely, aborting its daemon stream and pump tasks.
    pub fn close_session(&self, session_id: &str) {
        self.close_session_fenced(session_id, None);
    }

    pub fn close_session_fenced(
        &self,
        session_id: &str,
        tuple: Option<&crate::daemon::protocol::PaneAttachTuple>,
    ) -> bool {
        let mut hosts = self.hosts.lock();
        let mut sessions = self.sessions.lock();
        if let Some(expected) = tuple {
            if sessions.get(session_id).is_some_and(|session| {
                session.active_attach_tuple.as_ref() != Some(expected)
                    || session.attempt_generation > expected.attempt_generation
            }) {
                return false;
            }
        }
        self.clear_pending_session(session_id);
        if let Some(mut session) = sessions.remove(session_id) {
            session.snapshot_slot.set_attached(false);
            if let Some(task) = session.stream_task.take() {
                task.abort();
            }
            if let Some(task) = session.pump_task.take() {
                task.abort();
            }
            if let Some(task) = session.pty_write_task.take() {
                task.abort();
            }
        }
        drop(sessions);

        hosts.remove(session_id);
        true
    }

    pub fn teardown(&self) {
        let mut hosts = self.hosts.lock();
        let mut sessions = self.sessions.lock();
        for (_, session) in sessions.drain() {
            if let Some(task) = session.stream_task {
                task.abort();
            }
            if let Some(task) = session.pump_task {
                task.abort();
            }
            if let Some(task) = session.pty_write_task {
                task.abort();
            }
        }
        drop(sessions);
        hosts.clear();
    }

    pub fn target_descriptor(&self) -> PlatformCompositorDescriptor {
        let hosts = self.hosts.lock();
        if let Some(host) = hosts.values().next() {
            host.descriptor()
        } else {
            PlatformCompositorDescriptor::active_for_platform()
        }
    }

    pub fn has_session_host(&self, session_id: &str) -> bool {
        self.hosts.lock().contains_key(session_id)
    }

    pub fn session_host_count(&self) -> usize {
        self.hosts.lock().len()
    }

    pub fn subscribe_session_update(
        &self,
        session_id: &str,
    ) -> Result<tokio::sync::watch::Receiver<()>, NativeTerminalError> {
        validate_session_id(session_id)?;
        let sessions = self.sessions.lock();
        let session = sessions
            .get(session_id)
            .ok_or(NativeTerminalError::NoValue)?;
        Ok(session.update_sender.subscribe())
    }

    pub(crate) fn subscribe_session_detach(
        &self,
        session_id: &str,
    ) -> Result<tokio::sync::watch::Receiver<()>, NativeTerminalError> {
        validate_session_id(session_id)?;
        let sessions = self.sessions.lock();
        let session = sessions
            .get(session_id)
            .ok_or(NativeTerminalError::NoValue)?;
        Ok(session.detach_sender.subscribe())
    }

    pub fn session_render_coordinator(
        &self,
        session_id: &str,
    ) -> Option<Arc<RenderScheduleCoordinator>> {
        self.sessions
            .lock()
            .get(session_id)
            .map(|session| Arc::clone(&session.render_coordinator))
    }

    pub fn schedule_session_render(&self, session_id: &str) -> bool {
        self.sessions
            .lock()
            .get(session_id)
            .map(|session| session.render_coordinator.schedule_render())
            .unwrap_or(false)
    }

    pub fn consume_session_render(&self, session_id: &str) -> bool {
        self.sessions
            .lock()
            .get(session_id)
            .map(|session| session.render_coordinator.consume_render())
            .unwrap_or(false)
    }

    pub fn is_session_render_pending(&self, session_id: &str) -> bool {
        self.sessions
            .lock()
            .get(session_id)
            .map(|session| session.render_coordinator.is_render_pending())
            .unwrap_or(false)
    }

    /// Snapshot of all currently registered session ids (diagnostic helper).
    /// Ownership token for one attach transaction.
    ///
    /// Reading the replay cursor and then awaiting the daemon is not sound: the old pump keeps
    /// applying output across the await, so the response is a delta against a cursor the grid has
    /// already passed. Minting a token fences the old pump and captures the cursor under one lock,
    /// and the attach refuses to install its handles unless the token still owns the session.
    pub fn begin_replay_request(&self, session_id: &str) -> Option<ReplayRequestToken> {
        self.begin_replay_request_with_tuple(session_id, None)
    }

    /// Ownership token for one attach transaction, binding an optional 7-field attach tuple.
    ///
    /// Validates monotonicity: if the session has an active attachment with a strictly newer
    /// attempt generation, this stale attach attempt is rejected early without fencing the active stream.
    pub fn begin_replay_request_with_tuple(
        &self,
        session_id: &str,
        tuple: Option<crate::daemon::protocol::PaneAttachTuple>,
    ) -> Option<ReplayRequestToken> {
        let _hosts = self.hosts.lock();
        let mut sessions = self.sessions.lock();
        let session = sessions.get_mut(session_id)?;

        // If an active tuple exists and incoming tuple is strictly older, reject early!
        if let Some(ref incoming) = tuple {
            if incoming.backend_session_id != session_id {
                return None;
            }
            if let Some(ref active) = session.active_attach_tuple {
                if !pane_liveness_registration_matches(active, incoming) {
                    tracing::warn!(
                        session_id,
                        active_gen = active.attempt_generation,
                        incoming_gen = incoming.attempt_generation,
                        "rejecting stale attach attempt: active generation is newer"
                    );
                    return None;
                }
            } else if session.attempt_generation > incoming.attempt_generation {
                return None;
            }
        }

        if tuple.is_none() && session.active_attach_tuple.is_some() {
            return None;
        }
        if let Some(ref tuple) = tuple {
            session.active_attach_tuple = Some(tuple.clone());
            session.attempt_generation = tuple.attempt_generation;
            session.incarnation = tuple.incarnation.clone();
            session.snapshot_slot.set_attached(false);
            session.render_coordinator.consume_render();
        }
        session.pump_generation = session.pump_generation.wrapping_add(1);
        if let Some(task) = session.stream_task.take() {
            task.abort();
        }
        if let Some(task) = session.pump_task.take() {
            task.abort();
        }
        if let Some(task) = session.pty_write_task.take() {
            task.abort();
        }
        let attempt_gen = tuple.as_ref().map(|t| t.attempt_generation);
        Some(ReplayRequestToken {
            generation: session.pump_generation,
            epoch: session.daemon_epoch,
            last_sequence: session.last_sequence,
            attempt_generation: attempt_gen,
            attach_tuple: tuple,
        })
    }

    /// Authoritative 7-field attach tuple currently bound to the native session.
    pub fn session_attach_tuple(&self, session_id: &str) -> Option<crate::daemon::protocol::PaneAttachTuple> {
        let sessions = self.sessions.lock();
        sessions.get(session_id).and_then(|s| s.active_attach_tuple.clone())
    }

    /// Drives the runner's `trigger-stale-binding` control against the LIVE
    /// binding, through the product's real fences:
    ///
    /// 1. the authoritative seven-field binding is read from the attached session
    ///    (nothing is invented);
    /// 2. an older attempt that differs in exactly the field the command names is
    ///    really offered while that newer binding is active - the existing
    ///    registration fence rejects it, and the `stale-receipt-rejected` receipt
    ///    is written by that fence from its own comparison;
    /// 3. a legitimate reattach of the SAME live binding then proceeds through the
    ///    same production path and settles `reattach-marker`.
    ///
    /// Returns `false` when the command cannot be serviced yet (no daemon session
    /// with a live binding), so the caller keeps it pending instead of answering
    /// with a fabricated outcome. A command that can never be serviced (an
    /// unsupported mutate field) returns `true` after reporting itself.
    #[cfg(feature = "local-split-qa")]
    pub(crate) async fn drive_stale_binding_command(
        &self,
        channel: &Arc<crate::ipc::qa_barrier::QaBarrierChannel>,
        daemon_client: Option<&Arc<crate::daemon::DaemonClient>>,
    ) -> bool {
        let Some(command) = channel.take_command(TRIGGER_STALE_BINDING) else {
            return false;
        };
        let mutate_field = command
            .get("mutateField")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("attemptGeneration");
        if !STALE_BINDING_MUTABLE_FIELDS.contains(&mutate_field) {
            eprintln!(
                "FERRYX_QA_STALE_BINDING_UNSUPPORTED_FIELD: '{mutate_field}' is not compared by the registration fence (supported: {STALE_BINDING_MUTABLE_FIELDS:?}); no receipt was written"
            );
            return true;
        }
        let Some(session_id) = self.stale_binding_target(daemon_client).await else {
            return false;
        };
        let Some(active) = self.session_attach_tuple(&session_id) else {
            return false;
        };
        let Some(stale) = mutate_attach_tuple_field(&active, mutate_field) else {
            eprintln!(
                "FERRYX_QA_STALE_BINDING_UNSATISFIABLE: the live binding has attemptGeneration {} and cannot be made strictly older",
                active.attempt_generation
            );
            return true;
        };

        // A REAL stale offer against the live binding: the ownership-token fence
        // and the registration fence are the production ones, and the rejection
        // receipt comes from the fence, never from this caller.
        let token_refused = self
            .begin_replay_request_with_tuple(&session_id, Some(stale.clone()))
            .is_none();
        let stale_attempt =
            self.reattach_existing_session_with_bounds_and_tuple(&session_id, None, Some(&stale));
        if stale_attempt.is_ok() {
            eprintln!(
                "FERRYX_QA_STALE_BINDING_NOT_REJECTED: the live binding did not reject the stale attempt on '{mutate_field}' (token_refused={token_refused}); no rejection was written"
            );
            return true;
        }

        // Only a fence-rejected attempt is followed by the legitimate reattach, so
        // the two receipts keep the order the scenario awaits.
        let reattach =
            self.reattach_existing_session_with_bounds_and_tuple(&session_id, None, Some(&active));
        if !matches!(reattach, Ok(true)) {
            eprintln!(
                "FERRYX_QA_STALE_BINDING_REATTACH_UNSETTLED: the legitimate reattach of the live binding did not install"
            );
        }
        true
    }

    /// The session the stale-binding control must drive: one that really exists in
    /// the owning daemon AND carries a live seven-field binding here. Nothing is
    /// synthesized: without such a session the command stays unserviced.
    #[cfg(feature = "local-split-qa")]
    async fn stale_binding_target(
        &self,
        daemon_client: Option<&Arc<crate::daemon::DaemonClient>>,
    ) -> Option<String> {
        let mut candidates: Vec<String> = Vec::new();
        if let Some(client) = daemon_client {
            if let Ok(sessions) = client.list_sessions().await {
                candidates.extend(sessions);
            }
        }
        if candidates.is_empty() {
            let mut registered = self.registered_session_ids();
            registered.sort();
            candidates = registered;
        }
        candidates
            .into_iter()
            .find(|session_id| self.session_attach_tuple(session_id).is_some())
    }

    pub fn bound_presentation_observation(
        &self,
        session_id: &str,
        receipt: &NativeTerminalSurfaceReceipt,
    ) -> (Option<crate::daemon::protocol::PaneAttachTuple>, bool) {
        let sessions = self.sessions.lock();
        let Some(session) = sessions.get(session_id) else {
            return (None, false);
        };
        let tuple = session.active_attach_tuple.clone();
        let presentations = session.snapshot_slot.subscribe_presentations();
        let observed = *presentations.borrow();
        let presented = receipt.presented && !receipt.render_deferred && !receipt.render_suspended
            && session.surface_attached
            && observed.is_some_and(|frame| {
                frame.receipt == *receipt
                    && session.snapshot_slot.is_attached_with_epoch(frame.attachment_epoch)
                    && session.snapshot_slot.consume().is_some_and(|current| {
                        current.generation == frame.generation && current.attach_tuple == tuple
                    })
            });
        (tuple, presented)
    }

    /// Attempt generation currently bound to the native session.
    pub fn session_attempt_generation(&self, session_id: &str) -> Option<u64> {
        let sessions = self.sessions.lock();
        sessions.get(session_id).map(|s| s.attempt_generation)
    }

    /// Incarnation currently bound to the native session.
    pub fn session_incarnation(&self, session_id: &str) -> Option<String> {
        let sessions = self.sessions.lock();
        sessions.get(session_id).and_then(|s| s.incarnation.clone())
    }

    /// Last daemon sequence applied to the resident grid, for an attach that must request only
    /// the tail the grid has not seen.
    pub fn session_last_sequence(&self, session_id: &str) -> Option<u64> {
        self.sessions
            .lock()
            .get(session_id)
            .and_then(|session| session.last_sequence)
    }

    /// Task 3 (local-split-qa): producer component identity for the private
    /// presentation barrier receipts emitted by
    /// [`NativeTerminalSurfaceHostState::hold_presentation_barrier_qa`].
    #[cfg(feature = "local-split-qa")]
    pub const PRESENTATION_PRODUCER_ID: &str = "surface-host-render-coordinator";

    #[cfg(any(test, feature = "local-split-qa"))]
    pub fn attach_test_session_for_liveness(
        &self,
        session_id: &str,
        epoch: u64,
        last_sequence: Option<u64>,
    ) -> Result<Arc<RenderScheduleCoordinator>, NativeTerminalError> {
        validate_session_id(session_id)?;
        let mut terminal = NativeTerminal::new(80, 24)?;
        let _ = terminal.set_scrollback_limit_lines(Some(cached_terminal_preferences().scrollback));
        let (update_sender, _) = tokio::sync::watch::channel(());
        let (detach_sender, _) = tokio::sync::watch::channel(());
        let render_coordinator = Arc::new(RenderScheduleCoordinator::new());
        let session = NativeTerminalSession {
            terminal,
            focused: false,
            preedit: None,
            layout: None,
            logical_bounds: None,
            cell_metrics: None,
            stream_task: None,
            pump_task: None,
            pty_write_task: None,
            is_remote: false,
            remote_generation: None,
            last_sequence,
            daemon_epoch: epoch,
            pump_generation: 1,
            active_attach_tuple: None,
            attempt_generation: 0,
            incarnation: None,
            update_sender,
            detach_sender,
            render_coordinator: Arc::clone(&render_coordinator),
            last_agent_activity: None,
            last_provider_session: None,
            last_agent_detect_at: None,
            agent_detect_pending: false,
            last_scrollbar: None,
            scrollbar_overlay: ScrollbarOverlayState::default(),
            attention_frame: false,
            agent_reports_own_state: false,
            bracketed_paste_seen: false,
            surface_attached: true,
            output_stream_ended: false,
            snapshot_slot: Arc::new(SnapshotSlot::new()),
        };
        self.sessions.lock().insert(session_id.to_string(), session);
        Ok(render_coordinator)
    }

    pub fn session_liveness_observation(
        &self,
        session_id: &str,
    ) -> Option<crate::ipc::debug::PaneLivenessSnapshot> {
        let sessions = self.sessions.lock();
        let session = sessions.get(session_id)?;
        Some(crate::ipc::debug::PaneLivenessSnapshot {
            telemetry_available: true,
            session_id: Some(session_id.to_string()),
            vt_session_id: Some(session_id.to_string()),
            vt_epoch: Some(session.daemon_epoch.to_string()),
            vt_consumed_sequence: session.last_sequence,
            has_unpresented_frames: Some(session.render_coordinator.is_render_pending()),
            ..Default::default()
        })
    }

    /// Task 3 (local-split-qa): hold the REAL presentation coordinator
    /// producer on the private QA barrier. A frame is genuinely scheduled and
    /// pending (`has_unpresented_frames` from the real collector), so the
    /// held classification `BlockedInPresentation` is measured producer-level
    /// evidence - coordinator pending state, NOT OS/GPU presentation (that
    /// proof stays native, deferred to task 10). After the correlated
    /// release, the SAME owned frame is rendered through the coordinator's
    /// ownership-fenced begin/finish path and a fresh snapshot settles the
    /// barrier with stage-progress evidence.
    #[cfg(feature = "local-split-qa")]
    pub async fn hold_presentation_barrier_qa(
        &self,
        coordinator: &RenderScheduleCoordinator,
        session_id: &str,
        channel: &crate::ipc::qa_barrier::QaBarrierChannel,
    ) -> Option<crate::ipc::qa_barrier::ReleaseOutcome> {
        use crate::ipc::debug::{classify_pane_liveness, PaneLivenessVerdict};
        use crate::ipc::qa_barrier::{
            verdict_str, PRESENTATION_BARRIER, PRODUCER_ID as CHANNEL_PRODUCER,
        };

        let spec = channel.spec(PRESENTATION_BARRIER)?;
        assert!(
            coordinator.schedule_render(),
            "QA presentation barrier requires a fresh pending frame"
        );
        let held_snapshot = self
            .session_liveness_observation(session_id)
            .unwrap_or_default();
        let held_verdict = classify_pane_liveness(&held_snapshot);
        channel.write_held(
            &spec,
            session_id,
            "presentation_frame_pending",
            serde_json::json!({
                "classifierVerdict": verdict_str(held_verdict),
                "coordinatorEvidence": "coordinator-pending",
                "presentationEvidence": "coordinator-pending",
                "hasUnpresentedFrames": true,
                "producerComponent": Self::PRESENTATION_PRODUCER_ID,
            }),
        );
        let outcome = channel.wait_for_release(&spec).await;
        match outcome {
            crate::ipc::qa_barrier::ReleaseOutcome::Released => {
                // Consume the SAME scheduled frame through the real
                // ownership-fenced completion path.
                let owner = coordinator.begin_owned_render();
                let followed_up = coordinator.finish_owned_render(owner.unwrap_or(0), false);
                debug_assert!(
                    !followed_up,
                    "QA presentation fixture must not mint a follow-up frame"
                );
            }
            crate::ipc::qa_barrier::ReleaseOutcome::DeadlineExceeded => {
                // Bounded cancellation: cancel pending render work, never
                // park the coordinator.
                coordinator.consume_render();
            }
        }
        let fresh_snapshot = self
            .session_liveness_observation(session_id)
            .unwrap_or_default();
        let verdict = classify_pane_liveness(&fresh_snapshot);
        let render_pending_after = fresh_snapshot.has_unpresented_frames == Some(true);
        let missing = [
            ("daemonEpoch", fresh_snapshot.daemon_epoch.is_none()),
            ("readerPaused", fresh_snapshot.reader_paused.is_none()),
            ("kernelStopped", fresh_snapshot.kernel_stopped.is_none()),
            ("suspended", fresh_snapshot.suspended.is_none()),
            (
                "presentationReceiptReceived",
                fresh_snapshot.presentation_receipt_received.is_none(),
            ),
        ]
        .into_iter()
        .filter_map(|(field, missing)| missing.then_some(field))
        .collect::<Vec<_>>();
        channel.append_receipt(
            PRESENTATION_BARRIER,
            &spec.operation_id,
            serde_json::json!({
                "sessionId": session_id,
                "stage": "presentation_frame_settled",
                "classifierVerdict": verdict_str(verdict),
                "evidenceMissing": verdict == PaneLivenessVerdict::Unknown,
                "evidenceMissingFields": missing,
                "stageProgress": {
                    "frameConsumed": !render_pending_after,
                    "renderPendingAfter": render_pending_after,
                    "releaseOutcome": outcome.as_str(),
                },
                "snapshot": serde_json::to_value(&fresh_snapshot)
                    .unwrap_or(serde_json::Value::Null),
                "coordinatorEvidence": "coordinator-consumed",
                "presentationEvidence": "coordinator-consumed",
                "producerComponent": Self::PRESENTATION_PRODUCER_ID,
                "channelProducer": CHANNEL_PRODUCER,
            }),
        );
        Some(outcome)
    }

    /// Resident replay cursor as `(daemon_epoch, last_sequence)`. The epoch travels with the
    /// sequence because a cursor is only meaningful inside the epoch that issued it.
    pub fn session_replay_cursor(&self, session_id: &str) -> Option<(u64, u64)> {
        self.sessions
            .lock()
            .get(session_id)
            .and_then(|session| session.last_sequence.map(|seq| (session.daemon_epoch, seq)))
    }

    pub fn registered_session_ids(&self) -> Vec<String> {
        self.sessions.lock().keys().cloned().collect()
    }

    pub fn snapshot_for_session(
        &self,
        session_id: &str,
    ) -> Result<Option<RenderSnapshot>, NativeTerminalError> {
        validate_session_id(session_id)?;
        let sessions = self.sessions.lock();
        let Some(session) = sessions.get(session_id) else {
            return Ok(None);
        };
        let render_input = session_render_snapshot(session)?;
        Ok(Some(render_input.snapshot))
    }

    pub fn encode_attached_input(
        &self,
        session_id: &str,
        input: &NativeTerminalInput,
    ) -> Result<Vec<u8>, NativeTerminalError> {
        validate_session_id(session_id)?;
        let mut sessions = self.sessions.lock();
        let session = match sessions.get_mut(session_id) {
            Some(session) if session.surface_attached && !session.output_stream_ended => session,
            Some(_) | None => {
                return Err(NativeTerminalError::SessionDetached(session_id.to_string()));
            }
        };
        let bytes = input.encoded(&session.terminal)?;
        if !bytes.is_empty() {
            let _ = session
                .terminal
                .scroll_viewport(crate::native_terminal::ScrollViewport::Bottom);
            if let Ok(sb) = session.terminal.scrollbar() {
                session.scrollbar_overlay.metrics = Some(sb);
            }
            session.publish_frame();
        }
        Ok(bytes)
    }

    pub fn scroll_attached_terminal(
        &self,
        session_id: &str,
        behavior: crate::native_terminal::ScrollViewport,
    ) -> Result<(), NativeTerminalError> {
        validate_session_id(session_id)?;
        let mut sessions = self.sessions.lock();
        let session = match sessions.get_mut(session_id) {
            Some(session) if session.surface_attached => session,
            Some(_) | None => {
                return Err(NativeTerminalError::SessionDetached(session_id.to_string()));
            }
        };
        session.terminal.scroll_viewport(behavior)
    }

    pub fn encode_input(
        &self,
        session_id: &str,
        input: &NativeTerminalInput,
    ) -> Result<Vec<u8>, NativeTerminalError> {
        validate_session_id(session_id)?;
        let mut sessions = self.sessions.lock();
        let session = match sessions.get_mut(session_id) {
            Some(session) => session,
            None => {
                let mut terminal = NativeTerminal::new(80, 24)?;
                let _ = terminal
                    .set_scrollback_limit_lines(Some(cached_terminal_preferences().scrollback));
                let (update_sender, _) = tokio::sync::watch::channel(());
                let render_coordinator = Arc::new(RenderScheduleCoordinator::new());
                sessions.insert(
                    session_id.to_string(),
                    NativeTerminalSession {
                        terminal,
                        focused: false,
                        preedit: None,
                        layout: None,
                        logical_bounds: None,
                        cell_metrics: None,
                        stream_task: None,
                        pump_task: None,
                        pty_write_task: None,
                        is_remote: false,
                        remote_generation: None,
                        last_sequence: None,
                        daemon_epoch: 0,
                        pump_generation: 0,
                        active_attach_tuple: None,
                        attempt_generation: 0,
                        incarnation: None,
                        update_sender,
                        detach_sender: tokio::sync::watch::channel(()).0,
                        render_coordinator,
                        last_agent_activity: None,
                        last_provider_session: None,
                        last_agent_detect_at: None,
                        agent_detect_pending: false,
                        last_scrollbar: None,
                        scrollbar_overlay: ScrollbarOverlayState::default(),
                        attention_frame: false,
                        agent_reports_own_state: false,
                        surface_attached: true,
                        bracketed_paste_seen: false,
                        output_stream_ended: false,
                        snapshot_slot: Arc::new(SnapshotSlot::new()),
                    },
                );
                sessions
                    .get_mut(session_id)
                    .ok_or(NativeTerminalError::NoValue)?
            }
        };
        let bytes = input.encoded(&session.terminal)?;
        if !bytes.is_empty() {
            let _ = session
                .terminal
                .scroll_viewport(crate::native_terminal::ScrollViewport::Bottom);
            if let Ok(sb) = session.terminal.scrollbar() {
                session.scrollbar_overlay.metrics = Some(sb);
            }
            session.publish_frame();
        }
        Ok(bytes)
    }

    pub fn get_receipt<R: Runtime>(
        &self,
        _window: &Window<R>,
        session_id: &str,
    ) -> Result<NativeTerminalSurfaceReceipt, NativeTerminalError> {
        validate_session_id(session_id)?;
        let sessions = self.sessions.lock();
        let session = sessions
            .get(session_id)
            .ok_or(NativeTerminalError::NoValue)?;
        let layout = session.layout.ok_or(NativeTerminalError::NoValue)?;
        let cell_metrics = session.cell_metrics.ok_or(NativeTerminalError::NoValue)?;
        let (cursor_col, cursor_row) = session
            .terminal
            .cursor_position()
            .or_else(|_| {
                session
                    .snapshot_slot
                    .consume()
                    .map(|f| (f.input.snapshot.cursor.x, f.input.snapshot.cursor.y))
                    .ok_or(NativeTerminalError::NoValue)
            })
            .unwrap_or((0, 0));
        Ok(NativeTerminalSurfaceReceipt {
            presented: false,
            render_deferred: false,
            render_suspended: false,
            cols: layout.cols,
            rows: layout.rows,
            rebuilt_rows: 0,
            reused_rows: 0,
            cursor_col,
            cursor_row,
            cell_width_px: cell_metrics.width_px,
            cell_height_px: cell_metrics.height_px,
            effective_scale_factor: session.logical_bounds.map(|bounds| bounds.scale_factor),
        })
    }

    pub fn degraded_receipt(&self, session_id: &str) -> NativeTerminalSurfaceReceipt {
        let session_info = self.sessions.try_lock().and_then(|sessions| {
            sessions.get(session_id).map(|session| {
                let layout = session.layout;
                let cell_metrics = session.cell_metrics;
                let bounds = session.logical_bounds;
                let cursor = session.terminal.cursor_position().ok().or_else(|| {
                    session
                        .snapshot_slot
                        .consume()
                        .map(|f| (f.input.snapshot.cursor.x, f.input.snapshot.cursor.y))
                });
                (layout, cell_metrics, bounds, cursor)
            })
        });

        let (layout, cell_metrics, bounds, cursor) =
            session_info.unwrap_or((None, None, None, None));
        let default_metrics = font_manager::derived_cell_metrics();
        let cell_metrics = cell_metrics.unwrap_or(default_metrics);
        let cols = layout.map(|l| l.cols).unwrap_or(80);
        let rows = layout.map(|l| l.rows).unwrap_or(24);
        let (cursor_col, cursor_row) = cursor.unwrap_or((0, 0));

        NativeTerminalSurfaceReceipt {
            presented: false,
            render_deferred: false,
            render_suspended: false,
            cols,
            rows,
            rebuilt_rows: 0,
            reused_rows: 0,
            cursor_col,
            cursor_row,
            cell_width_px: cell_metrics.width_px,
            cell_height_px: cell_metrics.height_px,
            effective_scale_factor: bounds.map(|b| b.scale_factor),
        }
    }

    pub fn render<R: Runtime>(
        &self,
        window: &Window<R>,
        request: NativeTerminalBoundsRequest,
    ) -> Result<NativeTerminalSurfaceReceipt, NativeTerminalError> {
        self.render_fenced(window, request, None)
    }

    pub fn render_fenced<R: Runtime>(
        &self,
        window: &Window<R>,
        mut request: NativeTerminalBoundsRequest,
        tuple: Option<&crate::daemon::protocol::PaneAttachTuple>,
    ) -> Result<NativeTerminalSurfaceReceipt, NativeTerminalError> {
        let session_id = request.session_id.clone();
        // A pane that unmounted while its ResizeObserver callback was still in flight (rapid tab
        // switching) sends geometry for a session the compositor has already released. Rendering it
        // would rebuild a GPU surface for a pane nobody can see, so report the benign detached state
        // and let the caller drop the update.
        let mut hosts = self.lock_attached_hosts(&session_id)?;
        if tuple.is_some() && self.sessions.lock().get(&session_id)
            .is_none_or(|session| session.active_attach_tuple.as_ref() != tuple) {
            return Err(NativeTerminalError::SessionDetached(session_id));
        }
        let host = match hosts.entry(session_id.clone()) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::hash_map::Entry::Vacant(entry) => entry.insert(
                NativeTerminalSurfaceHost::new(window, request.bounds.scale_factor)?,
            ),
        };
        request.bounds = host
            .active_presentation_geometry()
            .resolve(request.bounds)?;
        let scale_factor = request.bounds.scale_factor;
        let cell_metrics = font_manager::derived_cell_metrics_for_scale(scale_factor);
        let logical_bounds = request.bounds;
        let layout = self.prepare_session_layout(request, cell_metrics)?;
        let render_input = {
            let sessions = self.sessions.lock();
            let session = sessions
                .get(&session_id)
                .ok_or(NativeTerminalError::NoValue)?;
            session_render_snapshot(session)?
        };

        let renderer_config = RendererConfig {
            cell_width_px: cell_metrics.width_px,
            cell_height_px: cell_metrics.height_px,
            device_scale_factor: if scale_factor.is_finite() && scale_factor > 0.0 {
                scale_factor as f32
            } else {
                1.0
            },
            theme: RendererTheme::from(cached_terminal_preferences().as_ref()),
        };
        host.update_config(renderer_config)?;
        host.layout = Some(layout);
        host.logical_bounds = Some(logical_bounds);
        host.update_viewport(Some(logical_bounds));

        let receipt = host.render_snapshot(
            window,
            layout,
            &render_input.snapshot,
            render_input.selection.as_ref(),
            render_input.scrollbar_overlay.as_ref(),
            render_input.attention_frame,
            render_input.synchronized_output,
        )?;
        drop(hosts);
        if receipt.render_deferred && !render_input.synchronized_output {
            let schedule_info = {
                let sessions = self.sessions.lock();
                sessions
                    .get(&session_id)
                    .filter(|session| session.surface_attached)
                    .map(|session| (Arc::clone(&session.render_coordinator), Arc::clone(&session.snapshot_slot)))
            };
            if let Some((coordinator, slot)) = schedule_info {
                if coordinator.schedule_render() {
                    let window = window.clone();
                    let hosts = Arc::clone(&self.hosts);
                    let session_id = session_id.clone();
                    let gpu_worker = Arc::clone(&self.gpu_worker);
                    defer_scheduled_render(
                        window,
                        hosts,
                        slot,
                        session_id,
                        coordinator,
                        gpu_worker,
                    );
                }
            }
        }
        self.rearm_dropped_direct_frame(window, &session_id, receipt);
        Ok(receipt)
    }

    pub fn set_focus<R: Runtime>(
        &self,
        window: &Window<R>,
        session_id: &str,
        focused: bool,
    ) -> Result<NativeTerminalSurfaceReceipt, NativeTerminalError> {
        self.render_current_with_focus(window, session_id, Some(focused), None)
    }

    pub fn render_current<R: Runtime>(
        &self,
        window: &Window<R>,
        session_id: &str,
    ) -> Result<NativeTerminalSurfaceReceipt, NativeTerminalError> {
        self.render_current_with_focus(window, session_id, None, None)
    }

    pub fn render_current_fenced<R: Runtime>(
        &self,
        window: &Window<R>,
        session_id: &str,
        tuple: Option<&crate::daemon::protocol::PaneAttachTuple>,
    ) -> Result<NativeTerminalSurfaceReceipt, NativeTerminalError> {
        self.render_current_with_focus(window, session_id, None, tuple)
    }

    fn render_current_with_focus<R: Runtime>(
        &self,
        window: &Window<R>,
        session_id: &str,
        focused: Option<bool>,
        tuple: Option<&crate::daemon::protocol::PaneAttachTuple>,
    ) -> Result<NativeTerminalSurfaceReceipt, NativeTerminalError> {
        let mut hosts = self.lock_attached_hosts(session_id)?;
        let (layout, logical_bounds, cell_metrics, render_input) = {
            let mut sessions = self.sessions.lock();
            let session = sessions
                .get_mut(session_id)
                .ok_or(NativeTerminalError::NoValue)?;
            if tuple.is_some() && session.active_attach_tuple.as_ref() != tuple {
                return Err(NativeTerminalError::SessionDetached(session_id.to_string()));
            }
            if let Some(focused) = focused {
                session.focused = focused;
            }
            let layout = session.layout.ok_or(NativeTerminalError::NoValue)?;
            let logical_bounds = session.logical_bounds.ok_or(NativeTerminalError::NoValue)?;
            let cell_metrics = session.cell_metrics.ok_or(NativeTerminalError::NoValue)?;
            let render_input = session_render_snapshot(session)?;
            // The native host defers this frame to the GPU worker, which paints whatever the
            // slot holds. Publishing the snapshot that was just computed is what makes the
            // deferred pass paint THIS frame instead of the last one the pump happened to leave.
            session
                .snapshot_slot
                .publish(
                    layout,
                    logical_bounds,
                    render_input.clone(),
                    session.active_attach_tuple.clone(),
                    session.last_sequence,
                );
            (layout, logical_bounds, cell_metrics, render_input)
        };

        if let Some(host) = hosts.get_mut(session_id) {
            host.layout = Some(layout);
            host.logical_bounds = Some(logical_bounds);
            host.update_viewport(Some(logical_bounds));
            let receipt = host.render_snapshot(
                window,
                layout,
                &render_input.snapshot,
                render_input.selection.as_ref(),
                render_input.scrollbar_overlay.as_ref(),
                render_input.attention_frame,
                render_input.synchronized_output,
            )?;
            drop(hosts);
            if receipt.render_deferred && !render_input.synchronized_output {
                let schedule_info = {
                    let sessions = self.sessions.lock();
                    sessions
                        .get(session_id)
                        .filter(|session| session.surface_attached)
                        .map(|session| {
                            (
                                Arc::clone(&session.render_coordinator),
                                Arc::clone(&session.snapshot_slot),
                            )
                        })
                };
                if let Some((coordinator, slot)) = schedule_info {
                    if coordinator.schedule_render() {
                        let window = window.clone();
                        let hosts = Arc::clone(&self.hosts);
                        let session_id = session_id.to_string();
                        let gpu_worker = Arc::clone(&self.gpu_worker);
                        defer_scheduled_render(
                            window,
                            hosts,
                            slot,
                            session_id,
                            coordinator,
                            gpu_worker,
                        );
                    }
                }
            }
            self.rearm_dropped_direct_frame(window, session_id, receipt);
            Ok(receipt)
        } else {
            Ok(NativeTerminalSurfaceReceipt::from_snapshot(
                layout,
                &render_input.snapshot,
                0,
                0,
                cell_metrics,
                Some(logical_bounds),
            ))
        }
    }

    fn rearm_dropped_direct_frame<R: Runtime>(
        &self,
        window: &Window<R>,
        session_id: &str,
        receipt: NativeTerminalSurfaceReceipt,
    ) {
        if receipt.presented || receipt.render_deferred || receipt.render_suspended {
            return;
        }
        let schedule_info = {
            let sessions = self.sessions.lock();
            sessions
                .get(session_id)
                .filter(|session| session.surface_attached)
                .map(|session| {
                    (
                        Arc::clone(&session.render_coordinator),
                        Arc::clone(&session.snapshot_slot),
                    )
                })
        };
        if let Some((coordinator, slot)) = schedule_info {
            if coordinator.schedule_render() {
                let window = window.clone();
                let hosts = Arc::clone(&self.hosts);
                let session_id = session_id.to_string();
                let gpu_worker = Arc::clone(&self.gpu_worker);
                // All host/session guards are released. Wry can dispatch inline on the
                // main thread, so use the same deferred boundary as scheduled completion.
                defer_scheduled_render(
                    window,
                    hosts,
                    slot,
                    session_id,
                    coordinator,
                    gpu_worker,
                );
            }
        }
    }

    pub fn set_preedit<R: Runtime>(
        &self,
        window: &Window<R>,
        session_id: &str,
        preedit: Option<String>,
    ) -> Result<NativeTerminalSurfaceReceipt, NativeTerminalError> {
        validate_session_id(session_id)?;
        let (render_coordinator, slot) = {
            let mut sessions = self.sessions.lock();
            let session = sessions
                .get_mut(session_id)
                .ok_or(NativeTerminalError::NoValue)?;
            if preedit.as_ref().is_some_and(|p| !p.is_empty()) {
                let _ = session
                    .terminal
                    .scroll_viewport(crate::native_terminal::ScrollViewport::Bottom);
                if let Ok(sb) = session.terminal.scrollbar() {
                    session.scrollbar_overlay.metrics = Some(sb);
                }
            }
            if session.preedit != preedit {
                session.preedit = preedit;
            }
            session.publish_frame();
            (
                Arc::clone(&session.render_coordinator),
                Arc::clone(&session.snapshot_slot),
            )
        };

        if render_coordinator.schedule_render() {
            dispatch_scheduled_render(
                window.clone(),
                Arc::clone(&self.hosts),
                slot,
                session_id.to_string(),
                render_coordinator,
                Arc::clone(&self.gpu_worker),
            );
        }

        self.get_receipt(window, session_id)
    }
}

/// True when a write failure came from the interactive connection being busy
/// (see `DaemonClient::send_interactive_request`), which is worth a bounded retry.
fn is_busy_error(error: &crate::ipc::IpcError) -> bool {
    error
        .details
        .as_ref()
        .and_then(|details| details.get("kind"))
        .and_then(|kind| kind.as_str())
        == Some("busy")
}

struct NativeTerminalSurfaceHost {
    frame_target: HostFrameTarget,
    layout: Option<SurfaceCompositionLayout>,
    logical_bounds: Option<LogicalBounds>,
    presentation_slot: Option<std::sync::Weak<SnapshotSlot>>,
}

// Only the native frame target is substituted in headless host tests. Session ownership,
// host-map guards, direct execution and scheduled completion are not substituted.
enum HostFrameTarget {
    Native(NativeSurfaceFrameTarget),
    #[cfg(test)]
    Injected(tests::InjectedFrameTarget),
}

impl NativeTerminalSurfaceHost {
    fn new<R: Runtime>(window: &Window<R>, scale_factor: f64) -> Result<Self, NativeTerminalError> {
        Ok(Self {
            frame_target: HostFrameTarget::Native(NativeSurfaceFrameTarget::new(
                window,
                scale_factor,
            )?),
            layout: None,
            logical_bounds: None,
            presentation_slot: None,
        })
    }

    fn active_presentation_generation(&self) -> Option<u64> {
        let slot = self.presentation_slot.as_ref()?.upgrade()?;
        let frame = slot.consume()?;
        slot.is_attached_with_epoch(frame.attachment_epoch).then_some(frame.generation)
    }

    fn active_presentation_epoch(&self) -> Option<u64> {
        let slot = self.presentation_slot.as_ref()?.upgrade()?;
        let frame = slot.consume()?;
        slot.is_attached_with_epoch(frame.attachment_epoch).then_some(frame.attachment_epoch)
    }

    /// Presentation geometry of the active frame target; injected test targets use the
    /// default (identity) presentation so headless harnesses keep raw webview density.
    fn active_presentation_geometry(&self) -> SurfacePresentationGeometry {
        match &self.frame_target {
            HostFrameTarget::Native(native) => native.presentation_geometry(),
            #[cfg(test)]
            HostFrameTarget::Injected(_) => SurfacePresentationGeometry::Default,
        }
    }

    fn descriptor(&self) -> PlatformCompositorDescriptor {
        match &self.frame_target {
            HostFrameTarget::Native(target) => target.descriptor(),
            #[cfg(test)]
            HostFrameTarget::Injected(_) => PlatformCompositorDescriptor::active_for_platform(),
        }
    }

    fn update_viewport(&self, bounds: Option<LogicalBounds>) {
        match &self.frame_target {
            HostFrameTarget::Native(target) => target.update_viewport(bounds),
            #[cfg(test)]
            HostFrameTarget::Injected(_) => {}
        }
    }

    fn finish_presentation<R: Runtime>(&mut self, window: &Window<R>) {
        match &mut self.frame_target {
            HostFrameTarget::Native(target) => target.finish_presentation(window),
            #[cfg(test)]
            HostFrameTarget::Injected(_) => {}
        }
    }

    fn update_config(&mut self, config: RendererConfig) -> Result<(), NativeTerminalError> {
        config.validate()?;
        match &mut self.frame_target {
            HostFrameTarget::Native(target) => {
                target.cell_metrics = CellMetrics {
                    width_px: config.cell_width_px,
                    height_px: config.cell_height_px,
                };
                Ok(())
            }
            #[cfg(test)]
            HostFrameTarget::Injected(target) => {
                target.cell_metrics = CellMetrics {
                    width_px: config.cell_width_px,
                    height_px: config.cell_height_px,
                };
                Ok(())
            }
        }
    }

    fn render_snapshot<R: Runtime>(
        &mut self,
        _window: &Window<R>,
        layout: SurfaceCompositionLayout,
        snapshot: &RenderSnapshot,
        _selection: Option<&SelectionSnapshot>,
        _scrollbar_overlay: Option<&ScrollbarOverlayState>,
        _attention_frame: bool,
        synchronized_output: bool,
    ) -> Result<NativeTerminalSurfaceReceipt, NativeTerminalError> {
        #[cfg(test)]
        RENDER_SNAPSHOT_CALL_COUNT.fetch_add(1, Ordering::SeqCst);
        if synchronized_output {
            let cell_metrics = match &self.frame_target {
                HostFrameTarget::Native(target) => target.cell_metrics,
                #[cfg(test)]
                HostFrameTarget::Injected(target) => target.cell_metrics,
            };
            return Ok(NativeTerminalSurfaceReceipt {
                render_deferred: true,
                ..NativeTerminalSurfaceReceipt::from_snapshot(
                    layout,
                    snapshot,
                    0,
                    0,
                    cell_metrics,
                    self.logical_bounds,
                )
            });
        }
        match &mut self.frame_target {
            HostFrameTarget::Native(target) => {
                let cell_metrics = target.cell_metrics;
                Ok(NativeTerminalSurfaceReceipt {
                    render_deferred: true,
                    ..NativeTerminalSurfaceReceipt::from_snapshot(
                        layout,
                        snapshot,
                        0,
                        0,
                        cell_metrics,
                        self.logical_bounds,
                    )
                })
            }
            #[cfg(test)]
            HostFrameTarget::Injected(target) => {
                if target.defer_direct {
                    let cell_metrics = target.cell_metrics;
                    return Ok(NativeTerminalSurfaceReceipt {
                        render_deferred: true,
                        ..NativeTerminalSurfaceReceipt::from_snapshot(
                            layout,
                            snapshot,
                            0,
                            0,
                            cell_metrics,
                            self.logical_bounds,
                        )
                    });
                }
                target.render_snapshot(layout, snapshot)
            }
        }
    }
}

// v30 test seam: mirrors the native acquisition/reconfigure/drop sequence for injected frame
// targets; the production path classifies the real `wgpu::CurrentSurfaceTexture` inline.
#[cfg(test)]
enum SimulatedAcquisition {
    Frame,
    NeedsReconfigure,
    Dropped,
    Occluded,
    Fatal,
}

#[cfg(test)]
fn acquire_surface_frame(
    mut acquire: impl FnMut() -> SimulatedAcquisition,
    reconfigure: impl FnOnce() -> Result<(), NativeTerminalError>,
) -> Result<(Option<()>, bool), NativeTerminalError> {
    let outcome = match acquire() {
        SimulatedAcquisition::NeedsReconfigure => {
            reconfigure()?;
            acquire()
        }
        outcome => outcome,
    };
    Ok(match outcome {
        SimulatedAcquisition::Frame => (Some(()), false),
        SimulatedAcquisition::Dropped => (None, false),
        SimulatedAcquisition::Occluded => (None, true),
        SimulatedAcquisition::Fatal => return Err(NativeTerminalError::OutOfMemory),
        SimulatedAcquisition::NeedsReconfigure => {
            unreachable!("reconfigure already applied before final acquisition")
        }
    })
}

/// GPU-owned half of a native frame target: exactly the state the render pass touches, and
/// nothing that talks to the window server. Split out so the render pass can move to a GPU worker
/// thread while the child view stays on the UI thread, where the platform requires it.
struct GpuLeg {
    surface: wgpu::Surface<'static>,
    renderer: Option<NativeTerminalRenderer>,
    initial_config: RendererConfig,
    format: wgpu::TextureFormat,
    size: PhysicalSize<u32>,
}

/// Native WGPU surface, platform compositor child view, and renderer.
///
/// # Drop Order Invariant
///
/// In Rust, struct fields are dropped in top-to-bottom declaration order.
/// 1. `leg` (which owns `surface`) MUST drop before `target`: the WGPU `Surface` and its internal
///    Metal layer must be destroyed while the native child NSView (`target`) is still valid and
///    parented.
/// 2. `target` drops after: unparents (`removeFromSuperview`) and releases the child NSView.
/// 3. The renderer inside `leg` drops with it: GPU device, pipelines, and glyph atlas.
///
/// Safe asynchronous retirement discharges this obligation without blocking the UI thread:
/// if `leg` is currently lent to a worker thread, `target` is retained in the retirement slot
/// and dropped strictly after the worker returns and drops `leg`.
struct NativeSurfaceFrameTarget {
    leg: LentSlot<GpuLeg>,
    cell_metrics: CellMetrics,
    target: Option<PlatformCompositorTarget>,
    main_dispatcher: Arc<dyn Fn(Box<dyn FnOnce() + Send>) + Send + Sync>,
}

impl Drop for NativeSurfaceFrameTarget {
    fn drop(&mut self) {
        if let Some(target) = self.target.take() {
            let dispatcher = Arc::clone(&self.main_dispatcher);
            self.leg.retire_with(move || {
                // Leg (and its wgpu::Surface) has dropped on the GPU worker thread.
                // Now drop the native target strictly on the UI main thread.
                dispatcher(Box::new(move || {
                    drop(target);
                }));
            });
        }
    }
}

impl NativeSurfaceFrameTarget {
    fn new<R: Runtime>(window: &Window<R>, scale_factor: f64) -> Result<Self, NativeTerminalError> {
        let target = PlatformCompositorTarget::new(window)?;
        let descriptor = target.descriptor();
        descriptor.validate_desktop_composition()?;

        let window_clone = window.clone();
        let main_dispatcher: Arc<dyn Fn(Box<dyn FnOnce() + Send>) + Send + Sync> =
            Arc::new(move |task| {
                if dispatch_render_on_main_thread(&window_clone, move || task()).is_err() {
                    tracing::warn!("Failed to dispatch native target drop to main thread during shutdown");
                }
            });

        let scale = if scale_factor.is_finite() && scale_factor > 0.0 {
            scale_factor as f32
        } else {
            1.0
        };
        let cell_metrics = font_manager::derived_cell_metrics_for_scale(scale as f64);
        let initial_config = RendererConfig {
            cell_width_px: cell_metrics.width_px,
            cell_height_px: cell_metrics.height_px,
            device_scale_factor: scale,
            theme: RendererTheme::from(cached_terminal_preferences().as_ref()),
        };
        initial_config.validate()?;
        // AppKit surface creation inspects the NSView and must remain on the UI thread.
        // Adapter/device requests and all surface configuration happen on the GPU worker.
        let instance = super::renderer::gpu_context::GpuContext::shared_surface_instance();
        let surface = match target.surface_layer_ptr() {
            #[cfg(target_os = "macos")]
            Some(layer_ptr) => unsafe {
                // The layer pointer stays valid for the whole surface lifetime: the target
                // holding it is dropped strictly after this surface (drop-order invariant).
                instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(layer_ptr))
            },
            #[cfg(not(target_os = "macos"))]
            // `PlatformCompositorTarget::surface_layer_ptr` returns `None` on every non-macOS
            // target (platform/mod.rs:130-137), so this arm cannot be reached. It exists only to
            // keep the match exhaustive; off macOS the `None` arm below is the only live path.
            Some(_) => unreachable!(
                "surface_layer_ptr returns Some only on macOS (platform/mod.rs:130-137)"
            ),
            None => instance.create_surface(target.surface_target()),
        }
        .map_err(|error| {
            NativeTerminalError::GpuPipelineError(format!("Surface create error: {error}"))
        })?;
        Ok(Self {
            leg: LentSlot::new(GpuLeg {
                surface,
                renderer: None,
                initial_config,
                format: wgpu::TextureFormat::Bgra8Unorm,
                size: PhysicalSize::new(0, 0),
            }),
            cell_metrics: CellMetrics {
                width_px: cell_metrics.width_px,
                height_px: cell_metrics.height_px,
            },
            target: Some(target),
            main_dispatcher,
        })
    }

    fn descriptor(&self) -> PlatformCompositorDescriptor {
        self.target
            .as_ref()
            .map(|t| t.descriptor())
            .unwrap_or_else(PlatformCompositorDescriptor::active_for_platform)
    }

    fn presentation_geometry(&self) -> SurfacePresentationGeometry {
        self.target
            .as_ref()
            .map(|t| t.presentation_geometry())
            .unwrap_or(SurfacePresentationGeometry::Default)
    }

    fn update_viewport(&self, bounds: Option<LogicalBounds>) {
        if let Some(target) = &self.target {
            target.update_viewport(bounds);
        }
    }

    // Runs only when a frame was actually presented: the no-frame path returns a receipt with
    // `presented` unset and must not touch AppKit, or a suspended surface would be revealed.
    fn finish_presentation<R: Runtime>(&mut self, window: &Window<R>) {
        if let Some(target) = &mut self.target {
            target.reveal_after_present();
            target.restore_first_responder(window);
        }
    }
}

impl GpuLeg {
    fn render_snapshot(
        &mut self,
        _gpu: &GpuThread,
        logical_bounds: Option<LogicalBounds>,
        layout: SurfaceCompositionLayout,
        snapshot: &RenderSnapshot,
        selection: Option<&super::renderer::SelectionSnapshot>,
        scrollbar_overlay: Option<&ScrollbarOverlayState>,
        attention_frame: bool,
    ) -> Result<NativeTerminalSurfaceReceipt, NativeTerminalError> {
        if self.renderer.is_none() {
            self.renderer = Some(NativeTerminalRenderer::with_shared_surface_gpu(
                self.initial_config,
            )?);
        }
        let renderer = self.renderer.as_mut().expect("GPU renderer initialized");
        if let Some(bounds) = logical_bounds {
            let scale_factor = bounds.scale_factor;
            let cell_metrics = font_manager::derived_cell_metrics_for_scale(scale_factor);
            let renderer_config = RendererConfig {
                cell_width_px: cell_metrics.width_px,
                cell_height_px: cell_metrics.height_px,
                device_scale_factor: if scale_factor.is_finite() && scale_factor > 0.0 {
                    scale_factor as f32
                } else {
                    1.0
                },
                theme: RendererTheme::from(cached_terminal_preferences().as_ref()),
            };
            renderer.update_config(renderer_config)?;
        }
        let surface_size =
            PhysicalSize::new(layout.physical_bounds.width, layout.physical_bounds.height);
        if surface_size != self.size {
            self.format = renderer.configure_surface(
                &self.surface,
                surface_size.width,
                surface_size.height,
            )?;
            self.size = surface_size;
        }

        let local_viewport = PhysicalBounds {
            x: 0,
            y: 0,
            width: surface_size.width,
            height: surface_size.height,
        };

        let mut render_suspended = false;
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => Some(frame),
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                self.format = renderer.configure_surface(
                    &self.surface,
                    surface_size.width,
                    surface_size.height,
                )?;
                match self.surface.get_current_texture() {
                    wgpu::CurrentSurfaceTexture::Success(frame)
                    | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => Some(frame),
                    ref other => match classify_surface_error(other)? {
                        SurfaceFrameAction::Retry => None,
                        SurfaceFrameAction::Suspend => {
                            render_suspended = true;
                            None
                        }
                    },
                }
            }
            ref other => match classify_surface_error(other)? {
                SurfaceFrameAction::Retry => None,
                SurfaceFrameAction::Suspend => {
                    render_suspended = true;
                    None
                }
            },
        };
        let cell_metrics = CellMetrics {
            width_px: renderer.config().cell_width_px,
            height_px: renderer.config().cell_height_px,
        };
        let Some(frame) = frame else {
            let mut receipt = NativeTerminalSurfaceReceipt::from_snapshot(
                layout,
                snapshot,
                0,
                0,
                cell_metrics,
                logical_bounds,
            );
            receipt.render_suspended = render_suspended;
            return Ok(receipt);
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let (rebuilt_rows, reused_rows) = renderer.render_to_surface_viewport(
            snapshot,
            selection,
            &view,
            surface_size.width,
            surface_size.height,
            self.format,
            local_viewport,
            scrollbar_overlay,
            attention_frame,
        )?;
        renderer.present(frame);
        Ok(NativeTerminalSurfaceReceipt {
            presented: true,
            ..NativeTerminalSurfaceReceipt::from_snapshot(
                layout,
                snapshot,
                rebuilt_rows,
                reused_rows,
                cell_metrics,
                logical_bounds,
            )
        })
    }
}

#[cfg(test)]
impl NativeTerminalSurfaceHostState {
    fn render_snapshot_for_session(
        &self,
        session_id: &str,
    ) -> Result<Option<SessionRenderInput>, NativeTerminalError> {
        validate_session_id(session_id)?;
        let sessions = self.sessions.lock();
        let Some(session) = sessions.get(session_id) else {
            return Ok(None);
        };
        session_render_snapshot(session).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dom_overlay_input_ownership_is_shared_across_host_state_clones() {
        // The macOS wheel monitor holds a clone of this state, so a per-clone copy of
        // the flag would never observe the frontend's overlay transitions.
        let state = NativeTerminalSurfaceHostState::default();
        assert!(!state.dom_overlay_owns_input());
        let monitor_clone = state.clone();
        state.set_dom_overlay_owns_input(true);
        assert!(monitor_clone.dom_overlay_owns_input());
        monitor_clone.set_dom_overlay_owns_input(false);
        assert!(!state.dom_overlay_owns_input());
    }

    #[tokio::test]
    async fn retired_replay_bottom_lock_preserves_new_generation_viewport() {
        let state = NativeTerminalSurfaceHostState::default();
        let id = "retired-bottom-lock";
        let (_tx, messages) = tokio::sync::mpsc::channel(1);
        state.attach_daemon_attachment::<tauri::Wry>(id, DaemonAttachment {
            session_id: id.into(), epoch: 1, start_sequence: Some(1), end_sequence: Some(1),
            gap: None, history: bytes::Bytes::from("history\r\n".repeat(100)),
            history_segments: Vec::new(), pty_cols: None, pty_rows: None,
            remote_generation: None, messages,
            stream_task: tokio::spawn(std::future::pending()),
        }, None).unwrap();
        let mut sessions = state.sessions.lock();
        let session = sessions.get_mut(id).unwrap();
        let retired = session.pump_generation;
        session.pump_generation += 1;
        let current = session.pump_generation;
        session.terminal.scroll_viewport(crate::native_terminal::ScrollViewport::Top).unwrap();
        let before = session.terminal.scrollbar().unwrap();
        assert!(before.total > before.len);
        lock_replay_to_bottom(&mut sessions, id, retired);
        assert_eq!(sessions.get(id).unwrap().terminal.scrollbar().unwrap().offset, before.offset);
        lock_replay_to_bottom(&mut sessions, id, current);
        let after = sessions.get(id).unwrap().terminal.scrollbar().unwrap();
        assert_eq!(after.offset, after.total.saturating_sub(after.len));
    }

    #[tokio::test]
    async fn a_replay_fetched_against_a_superseded_token_is_refused() {
        let state = NativeTerminalSurfaceHostState::default();
        let session_id = "token-ownership-session";
        let (_tx, messages) = tokio::sync::mpsc::channel(1);
        state
            .attach_daemon_attachment::<tauri::Wry>(
                session_id,
                DaemonAttachment {
                    session_id: session_id.to_string(),
                    epoch: 1,
                    start_sequence: Some(1),
                    end_sequence: Some(4),
                    gap: None,
                    history: bytes::Bytes::from_static(b"token-resident-line\r\n"),
                    history_segments: Vec::new(),
                    pty_cols: None,
                    pty_rows: None,
                    remote_generation: None,
                    messages,
                    stream_task: tokio::spawn(std::future::pending()),
                },
                None,
            )
            .expect("initial attach");

        // Two attaches race: the first mints a token, the second supersedes it while the first
        // is still awaiting the daemon. The first must not install its stream afterwards.
        let stale_token = state
            .begin_replay_request(session_id)
            .expect("session exists");
        let winner_token = state
            .begin_replay_request(session_id)
            .expect("session still exists");
        assert_ne!(stale_token.generation, winner_token.generation);

        let (_stale_tx, stale_messages) = tokio::sync::mpsc::channel(1);
        let stale_stream = tokio::spawn(std::future::pending());
        let result = state.attach_daemon_attachment_with_client_and_token::<tauri::Wry>(
            session_id,
            DaemonAttachment {
                session_id: session_id.to_string(),
                epoch: 1,
                start_sequence: Some(5),
                end_sequence: Some(6),
                gap: None,
                history: bytes::Bytes::from_static(b"stale-token-line\r\n"),
                history_segments: Vec::new(),
                pty_cols: None,
                pty_rows: None,
                remote_generation: None,
                messages: stale_messages,
                stream_task: stale_stream,
            },
            None,
            None,
            Some(stale_token),
        );

        assert!(
            matches!(result, Err(NativeTerminalError::SessionDetached(_))),
            "a replay whose token was superseded must be refused"
        );
        let text: String = state
            .snapshot_for_session(session_id)
            .unwrap()
            .unwrap()
            .grid
            .iter()
            .flat_map(|row| row.iter().map(|cell| cell.text.as_str()))
            .collect();
        assert!(
            !text.contains("stale-token-line"),
            "a refused replay must not reach the resident grid"
        );
        assert_eq!(state.session_last_sequence(session_id), Some(4));

        // A closed pane is a rejection, not an invitation to recreate the session: the user
        // already dismissed it, and `is_none_or` would have resurrected it here.
        let orphan_token = state
            .begin_replay_request(session_id)
            .expect("session still exists");
        state.close_session(session_id);
        let (_orphan_tx, orphan_messages) = tokio::sync::mpsc::channel(1);
        let orphan_result = state.attach_daemon_attachment_with_client_and_token::<tauri::Wry>(
            session_id,
            DaemonAttachment {
                session_id: session_id.to_string(),
                epoch: 1,
                start_sequence: Some(5),
                end_sequence: Some(6),
                gap: None,
                history: bytes::Bytes::from_static(b"orphan-line\r\n"),
                history_segments: Vec::new(),
                pty_cols: None,
                pty_rows: None,
                remote_generation: None,
                messages: orphan_messages,
                stream_task: tokio::spawn(std::future::pending()),
            },
            None,
            None,
            Some(orphan_token),
        );
        assert!(
            matches!(orphan_result, Err(NativeTerminalError::SessionDetached(_))),
            "a token attach against a closed session must be refused, not resurrect it"
        );
        assert!(
            state.snapshot_for_session(session_id).unwrap().is_none(),
            "a refused token attach must not recreate the closed session"
        );

        state.teardown();
    }

    #[test]
    fn classify_replay_requires_the_producer_marker_and_tolerates_resize_holes() {
        // A fresh grid has nothing to protect.
        assert_eq!(
            classify_replay(None, Some(1), Some(5), Some(true)),
            ReplayVerdict::Rebuild
        );

        // Sequence arithmetic alone never authorises a delta: an unmarked or gap-marked offer
        // rebuilds even though 6..=7 continues 5 by inspection.
        for marker in [None, Some(false)] {
            assert_eq!(
                classify_replay(Some(5), Some(6), Some(7), marker),
                ReplayVerdict::Rebuild,
                "marker {marker:?} must not be read as proving a delta"
            );
        }

        assert_eq!(
            classify_replay(Some(5), Some(6), Some(7), Some(true)),
            ReplayVerdict::Delta
        );

        // record_resize allocates a sequence without publishing, so 6 after applied 4 is a
        // structural hole, not loss. A `start == last + 1` test would misread this as a gap.
        assert_eq!(
            classify_replay(Some(4), Some(6), Some(7), Some(true)),
            ReplayVerdict::Delta
        );

        // Fully applied, and the stale-end case that must never rewind the cursor.
        assert_eq!(
            classify_replay(Some(10), Some(1), Some(10), Some(true)),
            ReplayVerdict::NoOp
        );
        assert_eq!(
            classify_replay(Some(10), Some(1), Some(6), Some(true)),
            ReplayVerdict::NoOp
        );

        // Straddling the applied boundary: segments carry no per-sequence offsets, so the
        // unapplied tail cannot be isolated and the whole offer must be rebuilt.
        assert_eq!(
            classify_replay(Some(5), Some(3), Some(7), Some(true)),
            ReplayVerdict::Rebuild
        );

        // An unknown start is the same unproven case.
        assert_eq!(
            classify_replay(Some(5), None, Some(9), Some(true)),
            ReplayVerdict::Rebuild
        );
    }

    #[test]
    fn replay_absorb_spans_the_burst_and_closes_on_idle_or_cap() {
        // Frames still arriving inside both bounds: the pane must not paint the scroll-through.
        assert!(replay_absorb_continues(
            std::time::Duration::from_millis(500),
            std::time::Duration::from_millis(10)
        ));
        // The burst drained, so the next wakeup paints once at the bottom.
        assert!(!replay_absorb_continues(
            std::time::Duration::from_millis(500),
            REPLAY_ABSORB_IDLE
        ));
        // A session that never goes idle still paints at the cap instead of staying blank.
        assert!(!replay_absorb_continues(
            REPLAY_ABSORB_MAX,
            std::time::Duration::from_millis(10)
        ));
    }

    #[test]
    fn p06_initial_layout_sets_ghostty_pixel_reply() {
        p06_assert_pixel_reply(false);
    }

    #[test]
    fn p06_dpi_only_layout_updates_ghostty_pixel_reply() {
        p06_assert_pixel_reply(true);
    }

    fn p06_assert_pixel_reply(change_dpi: bool) {
        let state = NativeTerminalSurfaceHostState::default();
        let session_id = "p06-pixel-geometry";
        for scale in 1..=if change_dpi { 2 } else { 1 } {
            let metrics = CellMetrics {
                width_px: 8 * scale,
                height_px: 16 * scale,
            };
            let layout = state
                .prepare_session_layout(
                    NativeTerminalBoundsRequest {
                        session_id: session_id.into(),
                        bounds: LogicalBounds {
                            x: 0.0,
                            y: 0.0,
                            width: 640.0,
                            height: 384.0,
                            scale_factor: f64::from(scale),
                        },
                    },
                    metrics,
                )
                .expect("prepare unchanged 80x24 grid");
            assert_eq!((layout.cols, layout.rows), (80, 24));
        }
        let mut sessions = state.sessions.lock();
        let terminal = &mut sessions
            .get_mut(session_id)
            .expect("prepared session")
            .terminal;
        terminal.discard_buffered_pty_writes();
        terminal
            .feed(b"\x1b[16t")
            .expect("query actual Ghostty cell pixel size");
        let reply: Vec<u8> = terminal
            .buffered_pty_writes()
            .into_iter()
            .flat_map(|record| record.data)
            .collect();
        assert_eq!(
            reply,
            if change_dpi {
                b"\x1b[6;32;16t".as_slice()
            } else {
                b"\x1b[6;16;8t".as_slice()
            }
        );
    }

    type RenderTask = Box<dyn FnOnce() + Send>;
    pub(super) struct RenderDispatch {
        sender: tokio::sync::mpsc::UnboundedSender<RenderTask>,
        owner_thread: std::thread::ThreadId,
        require_deferred: std::sync::atomic::AtomicBool,
        pub submissions: Mutex<Vec<tauri::async_runtime::JoinHandle<()>>>,
    }

    impl RenderDispatch {
        pub fn submit(&self, task: RenderTask) {
            if self.require_deferred.load(Ordering::SeqCst) {
                assert_ne!(std::thread::current().id(), self.owner_thread,
                    "retry dispatch must cross the production off-thread boundary, not recurse inline");
            }
            self.sender.send(task).expect("dispatch receiver alive");
        }

        pub fn submit_receipt(&self, task: RenderTask) {
            self.sender.send(task).expect("dispatch receiver alive");
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum FrameEvent {
        Acquire,
        Reconfigure,
        Dropped,
        Presented,
        Destroyed,
    }

    pub(super) struct InjectedFrameTarget {
        acquisitions: std::collections::VecDeque<SimulatedAcquisition>,
        pub cell_metrics: CellMetrics,
        events: Arc<Mutex<Vec<FrameEvent>>>,
        assert_host_locked: Box<dyn Fn() + Send>,
        /// Mirrors the production native target, which never paints inline: the direct render
        /// call only reports `render_deferred` and the frame is painted by the scheduled GPU
        /// pass. Off by default so the existing direct-path tests keep their inline semantics.
        pub defer_direct: bool,
    }

    impl InjectedFrameTarget {
        pub fn render_snapshot(
            &mut self,
            layout: SurfaceCompositionLayout,
            snapshot: &RenderSnapshot,
        ) -> Result<NativeTerminalSurfaceReceipt, NativeTerminalError> {
            RENDER_SNAPSHOT_CALL_COUNT.fetch_add(1, Ordering::SeqCst);
            (self.assert_host_locked)();
            let (frame, render_suspended) = acquire_surface_frame(
                || {
                    self.events.lock().push(FrameEvent::Acquire);
                    self.acquisitions
                        .pop_front()
                        .expect("unexpected acquisition / retry loop")
                },
                || {
                    self.events.lock().push(FrameEvent::Reconfigure);
                    Ok(())
                },
            )?;
            let presented = frame.is_some();
            self.events.lock().push(if presented {
                FrameEvent::Presented
            } else {
                FrameEvent::Dropped
            });
            Ok(NativeTerminalSurfaceReceipt {
                presented,
                render_suspended,
                ..NativeTerminalSurfaceReceipt::from_snapshot(
                    layout,
                    snapshot,
                    0,
                    0,
                    self.cell_metrics,
                    None,
                )
            })
        }
    }

    impl Drop for InjectedFrameTarget {
        fn drop(&mut self) {
            self.events.lock().push(FrameEvent::Destroyed);
        }
    }

    struct DirectRenderHarness {
        state: NativeTerminalSurfaceHostState,
        window: Window<tauri::test::MockRuntime>,
        _app: tauri::App<tauri::test::MockRuntime>,
        _output: tokio::sync::mpsc::Sender<DaemonStreamMessage<'static>>,
        request: NativeTerminalBoundsRequest,
        dispatched: tokio::sync::mpsc::UnboundedReceiver<RenderTask>,
        events: Arc<Mutex<Vec<FrameEvent>>>,
    }

    impl DirectRenderHarness {
        fn new(acquisitions: Vec<SimulatedAcquisition>) -> Self {
            // Subscribe before attaching/triggering any direct operation.
            let (dispatch, dispatched) = tokio::sync::mpsc::unbounded_channel();
            let app = tauri::test::mock_builder()
                .manage(RenderDispatch {
                    sender: dispatch,
                    owner_thread: std::thread::current().id(),
                    require_deferred: std::sync::atomic::AtomicBool::new(true),
                    submissions: Mutex::new(Vec::new()),
                })
                .build(tauri::test::mock_context(tauri::test::noop_assets()))
                .unwrap();
            let window =
                tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
                    .build()
                    .unwrap();
            let state = NativeTerminalSurfaceHostState::default();
            let request = NativeTerminalBoundsRequest {
                session_id: "direct-retry".into(),
                bounds: LogicalBounds {
                    x: 0.0,
                    y: 0.0,
                    width: 800.0,
                    height: 480.0,
                    scale_factor: 1.0,
                },
            };
            let (output, messages) = tokio::sync::mpsc::channel(1);
            state
                .attach_daemon_attachment_with_bounds::<tauri::test::MockRuntime>(
                    &request.session_id,
                    DaemonAttachment {
                        session_id: request.session_id.clone(),
                        epoch: 1,
                        start_sequence: Some(1),
                        end_sequence: Some(1),
                        gap: None,
                        history: bytes::Bytes::from(
                            (0..100)
                                .map(|line| format!("line {line}\r\n"))
                                .collect::<String>()
                                .into_bytes(),
                        ),
                        history_segments: Vec::new(),
                        pty_cols: Some(80),
                        pty_rows: Some(24),
                        remote_generation: None,
                        messages,
                        stream_task: tokio::spawn(std::future::pending()),
                    },
                    Some(app.handle().clone()),
                    Some(request.bounds),
                )
                .unwrap();
            let events = Arc::new(Mutex::new(Vec::new()));
            let hosts = Arc::downgrade(&state.hosts);
            let sessions = Arc::downgrade(&state.sessions);
            let host = NativeTerminalSurfaceHost {
                frame_target: HostFrameTarget::Injected(InjectedFrameTarget {
                    acquisitions: acquisitions.into(),
                    cell_metrics: font_manager::derived_cell_metrics(),
                    events: Arc::clone(&events),
                    defer_direct: false,
                    assert_host_locked: Box::new(move || {
                        assert!(
                            hosts.upgrade().unwrap().try_lock().is_none(),
                            "presentation must hold host ownership"
                        );
                        assert!(
                            sessions.upgrade().unwrap().try_lock().is_some(),
                            "snapshot session guard must be released"
                        );
                    }),
                }),
                layout: state.session_layout(&request.session_id),
                logical_bounds: Some(request.bounds),
                presentation_slot: state.session_snapshot_slot(&request.session_id).map(|slot| Arc::downgrade(&slot)),
            };
            state.hosts.lock().insert(request.session_id.clone(), host);
            assert!(!state.is_session_render_pending(&request.session_id));
            Self {
                state,
                window: window.as_ref().window(),
                _app: app,
                _output: output,
                request,
                dispatched,
                events,
            }
        }

        /// Harness whose injected target defers the direct render exactly like the production
        /// native target: nothing is painted inline, and the only paint comes from the scheduled
        /// GPU pass.
        fn with_deferred_direct_frames(acquisitions: Vec<SimulatedAcquisition>) -> Self {
            let harness = Self::new(acquisitions);
            {
                let mut hosts = harness.state.hosts.lock();
                let host = hosts
                    .get_mut(&harness.request.session_id)
                    .expect("harness installs an injected host");
                match &mut host.frame_target {
                    HostFrameTarget::Injected(target) => target.defer_direct = true,
                    HostFrameTarget::Native(_) => {
                        unreachable!("harness installs an injected frame target")
                    }
                }
            }
            harness
        }

        fn scroll_once(&self) -> Result<NativeTerminalSurfaceReceipt, NativeTerminalError> {
            let before = self
                .state
                .with_session_terminal(&self.request.session_id, |term| term.scrollbar())?;
            crate::ipc::native_terminal::scroll_attached_native_terminal(
                &self.state,
                &self.request.session_id,
                super::super::ScrollViewport::Top,
            )?;
            let after = self
                .state
                .with_session_terminal(&self.request.session_id, |term| term.scrollbar())?;
            assert_ne!(
                before.offset, after.offset,
                "one-shot input must actually change VT scroll position"
            );
            self.state.render(&self.window, self.request.clone())
        }

        async fn await_submissions(&self) {
            let tasks =
                std::mem::take(&mut *self.window.state::<RenderDispatch>().submissions.lock());
            for task in tasks {
                tokio::time::timeout(std::time::Duration::from_secs(5), task)
                    .await
                    .expect("deferred dispatch task must complete")
                    .expect("dispatch task must not panic");
            }
        }

        async fn execute_dispatched(&mut self) {
            self.await_submissions().await;
            let task =
                tokio::time::timeout(std::time::Duration::from_secs(5), self.dispatched.recv())
                    .await
                    .expect("production completion must dispatch a retry")
                    .expect("dispatch channel open");
            assert!(
                self.state.hosts.try_lock().is_some(),
                "dispatch after host guard release"
            );
            assert!(
                self.state.sessions.try_lock().is_some(),
                "dispatch after session guard release"
            );
            task(); // Execute the submitted main-thread task; never schedule another render here.
            self.await_submissions().await;
        }
    }

    impl Drop for DirectRenderHarness {
        fn drop(&mut self) {
            self.state.teardown();
        }
    }

    #[tokio::test]
    async fn pane_liveness_native_binding_real_render_emits_only_after_completion() {
        use tauri::Listener;
        let mut harness = DirectRenderHarness::with_deferred_direct_frames(vec![SimulatedAcquisition::Frame]);
        let tuple = crate::daemon::protocol::PaneAttachTuple {
            backend_session_id: harness.request.session_id.clone(), incarnation: Some("incarnation".into()),
            daemon_epoch: "1".into(), frontend_session_id: "frontend".into(), pane_identity: "pane".into(),
            binding_key: "binding".into(), attempt_generation: 1,
        };
        harness.state.sessions.lock().get_mut(&harness.request.session_id).unwrap().active_attach_tuple = Some(tuple.clone());
        let (sender, mut receipts) = tokio::sync::mpsc::unbounded_channel();
        let listener = harness._app.listen(crate::ipc::native_terminal::NATIVE_TERMINAL_PRESENTATION_RECEIPT_EVENT, move |event| {
            sender.send(event.payload().to_string()).unwrap();
        });
        let submitted = harness.state.render(&harness.window, harness.request.clone()).unwrap();
        assert!(!submitted.presented);
        assert!(submitted.render_deferred);
        assert!(matches!(receipts.try_recv(), Err(tokio::sync::mpsc::error::TryRecvError::Empty)));
        harness.execute_dispatched().await;
        let payload = tokio::time::timeout(std::time::Duration::from_secs(5), receipts.recv()).await.unwrap().unwrap();
        let receipt: crate::daemon::protocol::PanePresentationReceipt = serde_json::from_str(&payload).unwrap();
        assert!(receipt.presented);
        assert_eq!(receipt.attach_tuple, tuple);
        assert_eq!(*harness.events.lock(), vec![FrameEvent::Acquire, FrameEvent::Presented]);
        harness._app.unlisten(listener);
    }

    #[tokio::test]
    async fn pane_liveness_native_binding_host_accessors_discard_detached_presentation() {
        // The direct path paints inline here (`DirectRenderHarness::new` leaves `defer_direct`
        // off), so one render acquires exactly one drawable. A presented frame neither re-arms
        // the coordinator nor re-enters the retry loop, so the scripted queue holds the real
        // production sequence and nothing more: acquire once, present once. The injected target's
        // `expect` is the retry-loop guard, so an extra acquisition fails loudly here instead of
        // being absorbed by a longer queue.
        let mut harness = DirectRenderHarness::new(vec![SimulatedAcquisition::Frame]);
        let receipt = harness
            .state
            .render(&harness.window, harness.request.clone())
            .unwrap();
        assert!(receipt.presented, "the inline frame must actually present");
        assert!(!receipt.render_deferred);
        assert_eq!(
            *harness.events.lock(),
            vec![FrameEvent::Acquire, FrameEvent::Presented],
            "a presented frame acquires once and must not retry"
        );
        assert!(
            !harness
                .state
                .is_session_render_pending(&harness.request.session_id),
            "a presented frame must leave no retry armed"
        );
        assert!(
            harness.dispatched.try_recv().is_err(),
            "a presented frame must dispatch no retry"
        );
        let slot = harness.state.session_snapshot_slot(&harness.request.session_id).unwrap();
        let frame = slot.consume().unwrap();
        {
            let hosts = harness.state.hosts.lock();
            let host = hosts.get(&harness.request.session_id).unwrap();
            assert_eq!(host.active_presentation_generation(), Some(frame.generation));
            assert_eq!(host.active_presentation_epoch(), Some(frame.attachment_epoch));
        }
        slot.set_attached(false);
        let hosts = harness.state.hosts.lock();
        let host = hosts.get(&harness.request.session_id).unwrap();
        assert_eq!(host.active_presentation_generation(), None);
        assert_eq!(host.active_presentation_epoch(), None);
    }

    #[tokio::test]
    async fn agent_state_snapshot_reaches_desktop_even_when_native_state_is_unchanged() {
        use tauri::Listener;
        let harness = DirectRenderHarness::new(vec![]);
        let (sender, mut received) = tokio::sync::mpsc::unbounded_channel();
        let listener = harness
            ._app
            .listen(NATIVE_TERMINAL_AGENT_STATE_EVENT, move |event| {
                sender.send(event.payload().to_string()).unwrap();
            });

        // Subscribe before driving the real native pump. Repeated snapshots must
        // reach a newly mounted frontend even when the native state is unchanged.
        for (state, is_snapshot) in [
            ("idle", true),
            ("idle", true),
            ("working", false),
            ("idle", false),
        ] {
            let message = serde_json::from_value(serde_json::json!({
                "type": "agentState",
                "sessionId": harness.request.session_id,
                "state": state,
                "agent": "omo",
                "isSnapshot": is_snapshot,
            }))
            .unwrap();
            harness._output.send(message).await.unwrap();
            let payload = tokio::time::timeout(std::time::Duration::from_secs(3), received.recv())
                .await
                .expect("snapshot or live edge must reach the frontend")
                .unwrap();
            let payload: serde_json::Value = serde_json::from_str(&payload).unwrap();
            assert_eq!(payload["sessionId"], harness.request.session_id);
            assert_eq!(payload["state"], state);
            assert_eq!(
                payload["isSnapshot"].as_bool().unwrap_or(false),
                is_snapshot
            );
        }
        harness._app.unlisten(listener);
    }

    #[tokio::test]
    async fn bounds_ipc_resolves_on_the_deferred_gpu_presentation() {
        // The blank-screen regression: a native surface defers EVERY direct render, so a bounds
        // IPC that only re-rendered while `render_deferred` stayed true never resolved, and the
        // frontend never marked the pane presented. The GPU completion is the only presentation
        // evidence, so it must be what ends the wait.
        let mut harness =
            DirectRenderHarness::with_deferred_direct_frames(vec![SimulatedAcquisition::Frame]);
        harness._app.manage(harness.state.clone());
        harness
            .window
            .state::<RenderDispatch>()
            .require_deferred
            .store(false, Ordering::SeqCst);
        let bounds = harness.request.bounds;

        let app_handle = harness._app.handle().clone();
        let mut command = Box::pin(crate::ipc::native_terminal::cmd_native_terminal_set_bounds(
            app_handle.clone(),
            app_handle.state::<NativeTerminalSurfaceHostState>(),
            harness.request.session_id.clone(),
            crate::ipc::native_terminal::NativeTerminalLogicalRect {
                x: bounds.x,
                y: bounds.y,
                width: bounds.width,
                height: bounds.height,
            },
            bounds.scale_factor,
            None,
            None,
        ));
        assert!(
            futures_util::poll!(command.as_mut()).is_pending(),
            "a deferred frame has not reached the screen yet"
        );
        assert!(
            harness.events.lock().is_empty(),
            "the direct path must not paint inline; the GPU pass owns presentation"
        );

        // The deferred GPU pass is the only paint, and its completion is the signal.
        harness.execute_dispatched().await;

        let receipt = tokio::time::timeout(std::time::Duration::from_secs(5), command)
            .await
            .expect("a deferred bounds render must be acknowledged by its GPU presentation")
            .expect("bounds IPC must succeed once the frame is presented");

        assert!(
            receipt.presented,
            "the acknowledged receipt must carry the actual presentation"
        );
        assert!(!receipt.render_deferred);
        assert_eq!(
            *harness.events.lock(),
            vec![FrameEvent::Acquire, FrameEvent::Presented]
        );
    }

    #[tokio::test]
    async fn detached_presentation_cannot_acknowledge_a_later_attachment() {
        // A completion that belongs to a retired attachment must never mark the live one
        // presented: the slot rejects it on epoch, so the pending bounds wait stays open.
        let harness = DirectRenderHarness::new(vec![]);
        let slot = harness
            .state
            .session_snapshot_slot(&harness.request.session_id)
            .expect("attached session owns a snapshot slot");
        let stale_epoch = slot.current_epoch();
        let mut presentations = slot.subscribe_presentations();
        presentations.borrow_and_update();

        let stale_receipt = NativeTerminalSurfaceReceipt {
            presented: true,
            ..NativeTerminalSurfaceReceipt::from_snapshot(
                harness
                    .state
                    .session_layout(&harness.request.session_id)
                    .expect("prepared layout"),
                &harness
                    .state
                    .snapshot_for_session(&harness.request.session_id)
                    .unwrap()
                    .unwrap(),
                0,
                0,
                font_manager::derived_cell_metrics(),
                Some(harness.request.bounds),
            )
        };

        // Reattaching rotates the epoch, retiring the in-flight frame above.
        let live_epoch = slot.set_attached(true);
        assert_ne!(stale_epoch, live_epoch);

        assert!(
            !slot.publish_presentation(1, stale_epoch, None, stale_receipt),
            "a completion from a retired attachment must be rejected"
        );
        assert!(
            presentations.borrow_and_update().is_none(),
            "no waiter may observe a presentation for an attachment that is gone"
        );

        harness.state.sessions.lock()[&harness.request.session_id].publish_frame();
        let live_generation = slot.generation();
        assert!(
            slot.publish_presentation(live_generation, live_epoch, None, stale_receipt),
            "the live attachment's own completion must be accepted"
        );
        let observed =
            (*presentations.borrow_and_update()).expect("live completion reaches the waiter");
        assert_eq!(observed.attachment_epoch, live_epoch);
        assert_eq!(observed.generation, live_generation);
    }

    #[tokio::test]
    async fn ssh_reconnect_safety_native_status_reaches_desktop() {
        use tauri::Listener;
        let harness = DirectRenderHarness::new(vec![]);
        let (sender, mut received) = tokio::sync::mpsc::unbounded_channel();
        let listener = harness._app.listen("terminal_remote_status", move |event| {
            sender.send(event.payload().to_string()).unwrap();
        });
        harness
            ._output
            .send(DaemonStreamMessage::RemoteStatus {
                session_id: harness.request.session_id.clone().into(),
                state: crate::terminal::remote::RemoteConnectionState::Reconnecting,
                generation: 9,
                failure: None,
                replay_gap: None,
            })
            .await
            .unwrap();
        let payload = tokio::time::timeout(std::time::Duration::from_secs(3), received.recv())
            .await
            .expect("native pump must forward remote state")
            .unwrap();
        let payload: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(payload["sessionId"], harness.request.session_id);
        assert_eq!(payload["state"], "reconnecting");
        assert_eq!(payload["generation"], 9);
        harness._app.unlisten(listener);
    }

    #[tokio::test]
    async fn bounds_ipc_presents_when_browser_child_is_open() {
        // Given: the shell and an embedded browser share the main native window, and the native
        // target defers the direct render to the scheduled GPU pass exactly like production.
        let mut harness =
            DirectRenderHarness::with_deferred_direct_frames(vec![SimulatedAcquisition::Frame]);
        harness._app.manage(harness.state.clone());
        harness
            .window
            .state::<RenderDispatch>()
            .require_deferred
            .store(false, Ordering::SeqCst);
        let _browser = harness
            ._app
            .get_window("main")
            .unwrap()
            .add_child(
                tauri::webview::WebviewBuilder::new(
                    "browser-regression",
                    tauri::WebviewUrl::default(),
                ),
                tauri::LogicalPosition::new(0.0, 0.0),
                tauri::LogicalSize::new(400.0, 300.0),
            )
            .unwrap();
        assert!(harness._app.get_webview_window("main").is_none());
        let bounds = harness.request.bounds;

        // When: the frontend updates an attached terminal's bounds.
        let app_handle = harness._app.handle().clone();
        let mut command = Box::pin(crate::ipc::native_terminal::cmd_native_terminal_set_bounds(
            app_handle.clone(),
            app_handle.state::<NativeTerminalSurfaceHostState>(),
            harness.request.session_id.clone(),
            crate::ipc::native_terminal::NativeTerminalLogicalRect {
                x: bounds.x,
                y: bounds.y,
                width: bounds.width,
                height: bounds.height,
            },
            bounds.scale_factor,
            None,
            None,
        ));
        assert!(
            futures_util::poll!(command.as_mut()).is_pending(),
            "the direct call only defers; nothing has reached the screen yet"
        );
        assert!(
            harness.events.lock().is_empty(),
            "the direct path must not paint inline; the GPU pass owns presentation"
        );

        // The scheduled GPU pass is the only paint, and its presentation ends the wait.
        harness.execute_dispatched().await;
        let receipt = tokio::time::timeout(std::time::Duration::from_secs(5), command)
            .await
            .expect("browser child must not make the main terminal window unavailable")
            .expect("browser child must not make the main terminal window unavailable");

        // Then: the normal surface host presents the frame and acknowledges it.
        assert!(receipt.presented);
        assert!(!receipt.render_deferred);
        assert_eq!(
            *harness.events.lock(),
            vec![FrameEvent::Acquire, FrameEvent::Presented]
        );
    }

    #[tokio::test]
    async fn output_presents_when_browser_child_is_open() {
        // Given: dispatch is subscribed before output and the browser is already open.
        let mut harness = DirectRenderHarness::new(vec![SimulatedAcquisition::Frame]);
        harness
            .window
            .state::<RenderDispatch>()
            .require_deferred
            .store(false, Ordering::SeqCst);
        let _browser = harness
            ._app
            .get_window("main")
            .unwrap()
            .add_child(
                tauri::webview::WebviewBuilder::new(
                    "browser-regression",
                    tauri::WebviewUrl::default(),
                ),
                tauri::LogicalPosition::new(0.0, 0.0),
                tauri::LogicalSize::new(400.0, 300.0),
            )
            .unwrap();
        assert!(harness._app.get_webview_window("main").is_none());

        // When: the existing daemon attachment receives another output chunk.
        harness
            ._output
            .send(DaemonStreamMessage::Output {
                session_id: harness.request.session_id.clone().into(),
                sequence: 2,
                data: b"browser coexistence\r\n".to_vec().into(),
                metrics_read_unix_micros: None,
            })
            .await
            .unwrap();
        harness.execute_dispatched().await;

        // Then: the output pump still schedules and presents a terminal frame.
        assert_eq!(
            *harness.events.lock(),
            vec![FrameEvent::Acquire, FrameEvent::Presented]
        );
    }

    #[tokio::test]
    async fn synchronized_output_bounds_ipc_waits_for_actual_presentation() {
        // The native target defers every direct render, so the acknowledgement can only come from
        // the GPU pass that paints the frame the transaction releases.
        let mut harness = DirectRenderHarness::with_deferred_direct_frames(vec![
            SimulatedAcquisition::Frame,
            SimulatedAcquisition::Frame,
        ]);
        harness._app.manage(harness.state.clone());
        harness
            .window
            .state::<RenderDispatch>()
            .require_deferred
            .store(false, Ordering::SeqCst);
        // Settle the surface's first frame before the transaction opens, so the only paint left to
        // observe is the replacement frame the transaction releases.
        harness
            .state
            .render(&harness.window, harness.request.clone())
            .unwrap();
        harness.execute_dispatched().await;
        harness.events.lock().clear();
        harness
            .state
            .with_session_terminal(&harness.request.session_id, |terminal| {
                terminal.feed_str("\x1b[?2026hpartial")
            })
            .unwrap();
        let bounds = harness.request.bounds;
        let app_handle = harness._app.handle().clone();
        let mut command = Box::pin(crate::ipc::native_terminal::cmd_native_terminal_set_bounds(
            app_handle.clone(),
            app_handle.state::<NativeTerminalSurfaceHostState>(),
            harness.request.session_id.clone(),
            crate::ipc::native_terminal::NativeTerminalLogicalRect {
                x: bounds.x,
                y: bounds.y,
                width: bounds.width,
                height: bounds.height,
            },
            bounds.scale_factor,
            None,
            None,
        ));

        assert!(
            futures_util::poll!(command.as_mut()).is_pending(),
            "bounds IPC must not acknowledge an unpresented replacement surface"
        );
        assert!(harness.events.lock().is_empty());

        harness
            ._output
            .send(DaemonStreamMessage::Output {
                session_id: harness.request.session_id.clone().into(),
                sequence: 2,
                data: b"\rcomplete\x1b[?2026l".to_vec().into(),
                metrics_read_unix_micros: None,
            })
            .await
            .unwrap();
        // The transaction has ended: the pump's frame is presented by its GPU pass, and that
        // presentation is the only signal the bounds IPC accepts.
        harness.execute_dispatched().await;
        let receipt = tokio::time::timeout(std::time::Duration::from_secs(5), command)
            .await
            .unwrap()
            .unwrap();
        assert!(receipt.presented);
        assert!(!receipt.render_deferred);
        assert_eq!(
            *harness.events.lock(),
            vec![FrameEvent::Acquire, FrameEvent::Presented]
        );
    }

    #[tokio::test]
    async fn deferred_bounds_retry_does_not_restore_obsolete_width() {
        for newest_finishes_first in [false, true] {
            let mut harness = DirectRenderHarness::with_deferred_direct_frames(vec![
                SimulatedAcquisition::Frame,
                SimulatedAcquisition::Frame,
                SimulatedAcquisition::Frame,
            ]);
            harness._app.manage(harness.state.clone());
            harness
                .window
                .state::<RenderDispatch>()
                .require_deferred
                .store(false, Ordering::SeqCst);
            // Settle the surface's first frame before the transaction opens, so the only paint
            // left to observe is the replacement frame the transaction releases.
            harness
                .state
                .render(&harness.window, harness.request.clone())
                .unwrap();
            harness.execute_dispatched().await;
            let resizes = Arc::new(Mutex::new(Vec::new()));
            let recorded = Arc::clone(&resizes);
            assert!(harness
                .state
                .set_pty_resize_sink_if_absent(Arc::new(move |_, cols, rows| {
                    recorded.lock().push((cols, rows));
                })));
            harness
                .state
                .with_session_terminal(&harness.request.session_id, |terminal| {
                    terminal.feed_str("\x1b[?2026hpartial")
                })
                .unwrap();
            let app_handle = harness._app.handle().clone();
            let mut old_command =
                Box::pin(crate::ipc::native_terminal::cmd_native_terminal_set_bounds(
                    app_handle.clone(),
                    app_handle.state::<NativeTerminalSurfaceHostState>(),
                    harness.request.session_id.clone(),
                    crate::ipc::native_terminal::NativeTerminalLogicalRect {
                        x: 0.0,
                        y: 0.0,
                        width: 640.0,
                        height: 480.0,
                    },
                    1.0,
                    None,
                    None,
                ));
            assert!(futures_util::poll!(old_command.as_mut()).is_pending());
            let mut latest_command =
                Box::pin(crate::ipc::native_terminal::cmd_native_terminal_set_bounds(
                    app_handle.clone(),
                    app_handle.state::<NativeTerminalSurfaceHostState>(),
                    harness.request.session_id.clone(),
                    crate::ipc::native_terminal::NativeTerminalLogicalRect {
                        x: 0.0,
                        y: 0.0,
                        width: 900.0,
                        height: 480.0,
                    },
                    1.0,
                    None,
                    None,
                ));
            assert!(futures_util::poll!(latest_command.as_mut()).is_pending());

            let mut updates = harness
                .state
                .subscribe_session_update(&harness.request.session_id)
                .unwrap();
            harness
                ._output
                .send(DaemonStreamMessage::Output {
                    session_id: harness.request.session_id.clone().into(),
                    sequence: 2,
                    data: b"\rcomplete\x1b[?2026l".to_vec().into(),
                    metrics_read_unix_micros: None,
                })
                .await
                .unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(5), updates.changed())
                .await
                .unwrap()
                .unwrap();
            // The transaction has ended: the pump's frame is presented by its GPU pass, and that
            // presentation resolves both outstanding bounds requests.
            harness.execute_dispatched().await;
            let (old_receipt, latest) =
                tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    if newest_finishes_first {
                        let latest = latest_command.await.unwrap();
                        (old_command.await.unwrap(), latest)
                    } else {
                        let old_receipt = old_command.await.unwrap();
                        assert_eq!(
                            harness
                                .state
                                .session_logical_bounds(&harness.request.session_id)
                                .unwrap()
                                .width,
                            900.0
                        );
                        (old_receipt, latest_command.await.unwrap())
                    }
                })
                .await
                .unwrap();

            assert!(latest.presented);
            assert!(old_receipt.presented);
            assert_eq!(
                harness
                    .state
                    .session_logical_bounds(&harness.request.session_id)
                    .unwrap()
                    .width,
                900.0
            );
            assert_eq!(
                harness
                    .state
                    .hosts
                    .lock()
                    .get(&harness.request.session_id)
                    .unwrap()
                    .logical_bounds
                    .unwrap()
                    .width,
                900.0
            );
            assert_eq!(
                harness
                    .state
                    .with_session_terminal(&harness.request.session_id, |terminal| {
                        terminal.dimensions()
                    })
                    .unwrap(),
                (latest.cols, latest.rows)
            );
            assert_eq!(
                resizes.lock().last().copied(),
                Some((latest.cols, latest.rows))
            );
        }
    }

    #[tokio::test]
    async fn synchronized_output_bounds_ipc_finishes_on_detach_or_stream_end() {
        for detach in [false, true] {
            let mut harness =
                DirectRenderHarness::with_deferred_direct_frames(vec![SimulatedAcquisition::Frame]);
            harness._app.manage(harness.state.clone());
            harness
                .window
                .state::<RenderDispatch>()
                .require_deferred
                .store(false, Ordering::SeqCst);
            harness
                .state
                .with_session_terminal(&harness.request.session_id, |terminal| {
                    terminal.feed_str("\x1b[?2026hpartial")
                })
                .unwrap();
            let bounds = harness.request.bounds;
            let app_handle = harness._app.handle().clone();
            let mut command =
                Box::pin(crate::ipc::native_terminal::cmd_native_terminal_set_bounds(
                    app_handle.clone(),
                    app_handle.state::<NativeTerminalSurfaceHostState>(),
                    harness.request.session_id.clone(),
                    crate::ipc::native_terminal::NativeTerminalLogicalRect {
                        x: bounds.x,
                        y: bounds.y,
                        width: bounds.width,
                        height: bounds.height,
                    },
                    bounds.scale_factor,
                    None,
                    None,
                ));
            assert!(futures_util::poll!(command.as_mut()).is_pending());

            if detach {
                harness.state.detach_session(&harness.request.session_id);
            } else {
                let (replacement, _) = tokio::sync::mpsc::channel(1);
                drop(std::mem::replace(&mut harness._output, replacement));
                // The stream ends with the transaction still open: the pump's own frame is the
                // paint that finishes the request, so run its GPU pass before awaiting it.
                harness.execute_dispatched().await;
            }
            let result = tokio::time::timeout(std::time::Duration::from_secs(5), command)
                .await
                .expect("termination must wake the outstanding bounds request");

            if detach {
                assert!(result.is_err());
                assert_eq!(*harness.events.lock(), vec![FrameEvent::Destroyed]);
            } else {
                assert!(result.unwrap().presented);
                assert_eq!(
                    *harness.events.lock(),
                    vec![FrameEvent::Acquire, FrameEvent::Presented]
                );
            }
        }
    }

    #[tokio::test]
    async fn unpresented_bounds_ipc_returns_after_wait_bound_with_geometry_applied() {
        let harness = DirectRenderHarness::new(vec![SimulatedAcquisition::Frame]);
        harness._app.manage(harness.state.clone());
        harness
            .window
            .state::<RenderDispatch>()
            .require_deferred
            .store(false, Ordering::SeqCst);
        // An open synchronized-output transaction drops every frame without a presentation and,
        // with no further output, nothing else would ever wake the request.
        harness
            .state
            .with_session_terminal(&harness.request.session_id, |terminal| {
                terminal.feed_str("\x1b[?2026hpartial")
            })
            .unwrap();
        let narrowed_width = harness.request.bounds.width / 2.0;
        let command = crate::ipc::native_terminal::cmd_native_terminal_set_bounds(
            harness._app.handle().clone(),
            harness._app.state::<NativeTerminalSurfaceHostState>(),
            harness.request.session_id.clone(),
            crate::ipc::native_terminal::NativeTerminalLogicalRect {
                x: harness.request.bounds.x,
                y: harness.request.bounds.y,
                width: narrowed_width,
                height: harness.request.bounds.height,
            },
            harness.request.bounds.scale_factor,
            None,
            None,
        );

        let receipt = tokio::time::timeout(std::time::Duration::from_secs(5), command)
            .await
            .expect("an unpresentable frame must not hold the bounds request open")
            .unwrap();

        assert!(!receipt.presented);
        assert!(receipt.render_deferred);
        assert_eq!(
            harness
                .state
                .session_logical_bounds(&harness.request.session_id)
                .unwrap()
                .width,
            narrowed_width
        );
        assert_eq!(
            harness
                .state
                .with_session_terminal(&harness.request.session_id, |terminal| {
                    terminal.dimensions()
                })
                .unwrap(),
            (receipt.cols, receipt.rows)
        );
    }

    #[tokio::test]
    async fn synchronized_output_pump_presents_only_the_completed_redraw() {
        let mut harness = DirectRenderHarness::new(vec![SimulatedAcquisition::Frame]);
        harness
            .window
            .state::<RenderDispatch>()
            .require_deferred
            .store(false, Ordering::SeqCst);
        let mut updates = harness
            .state
            .subscribe_session_update(&harness.request.session_id)
            .unwrap();
        for (sequence, data) in [
            (2, b"\x1b[?2026h\x1b[2J\x1b[Hpartial".as_slice()),
            (3, b"\rcomplete\x1b[?2026l".as_slice()),
        ] {
            harness
                ._output
                .send(DaemonStreamMessage::Output {
                    session_id: harness.request.session_id.clone().into(),
                    sequence,
                    data: data.to_vec().into(),
                    metrics_read_unix_micros: None,
                })
                .await
                .unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(5), updates.changed())
                .await
                .unwrap()
                .unwrap();
            harness.execute_dispatched().await;
            if sequence == 2 {
                assert!(
                    harness.events.lock().is_empty(),
                    "the output pump must retain the last complete frame"
                );
                assert!(!harness
                    .state
                    .is_session_render_pending(&harness.request.session_id));
                assert!(
                    harness.dispatched.try_recv().is_err(),
                    "no retry spin during synchronized output"
                );
            }
        }
        assert_eq!(
            *harness.events.lock(),
            vec![FrameEvent::Acquire, FrameEvent::Presented]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn synchronized_output_pump_recovers_a_missing_end_without_more_output() {
        let mut harness = DirectRenderHarness::new(vec![SimulatedAcquisition::Frame]);
        harness
            .window
            .state::<RenderDispatch>()
            .require_deferred
            .store(false, Ordering::SeqCst);
        let mut updates = harness
            .state
            .subscribe_session_update(&harness.request.session_id)
            .unwrap();
        harness
            ._output
            .send(DaemonStreamMessage::Output {
                session_id: harness.request.session_id.clone().into(),
                sequence: 2,
                data: b"\x1b[?2026hpartial".to_vec().into(),
                metrics_read_unix_micros: None,
            })
            .await
            .unwrap();
        updates.changed().await.unwrap();
        harness.execute_dispatched().await;
        assert!(harness.events.lock().is_empty());

        tokio::time::advance(std::time::Duration::from_secs(1)).await;
        harness.execute_dispatched().await;

        assert!(!harness
            .state
            .with_session_terminal(&harness.request.session_id, |terminal| terminal
                .synchronized_output_enabled())
            .unwrap());
        assert_eq!(
            *harness.events.lock(),
            vec![FrameEvent::Acquire, FrameEvent::Presented]
        );
    }

    #[tokio::test]
    async fn synchronized_output_does_not_present_partial_redraw() {
        let harness = DirectRenderHarness::new(vec![
            SimulatedAcquisition::Frame,
            SimulatedAcquisition::Frame,
        ]);
        // Establish the resized grid before the application starts its redraw.
        assert!(
            harness
                .state
                .render(&harness.window, harness.request.clone())
                .unwrap()
                .presented
        );
        harness.events.lock().clear();
        harness
            .state
            .with_session_terminal(&harness.request.session_id, |terminal| {
                terminal.feed_str("\x1b[?2026h\x1b[2J\x1b[Hpartial redraw")
            })
            .unwrap();

        let receipt = harness
            .state
            .render(&harness.window, harness.request.clone())
            .unwrap();

        assert!(
            !receipt.presented,
            "an unfinished synchronized redraw must not reach presentation"
        );
        assert!(
            harness.events.lock().is_empty(),
            "keep the previous drawable without acquiring another"
        );
        assert!(
            !harness
                .state
                .is_session_render_pending(&harness.request.session_id),
            "a protocol-deferred frame must not enter the dropped-frame retry loop"
        );

        harness
            .state
            .with_session_terminal(&harness.request.session_id, |terminal| {
                terminal.feed_str("\rcomplete redraw\x1b[?2026l")
            })
            .unwrap();
        assert!(
            harness
                .state
                .render(&harness.window, harness.request.clone())
                .unwrap()
                .presented
        );
        assert_eq!(
            *harness.events.lock(),
            vec![FrameEvent::Acquire, FrameEvent::Presented]
        );
    }

    #[tokio::test]
    async fn direct_execution_characterization() {
        let harness = DirectRenderHarness::new(vec![
            SimulatedAcquisition::Frame,
            SimulatedAcquisition::Frame,
        ]);
        let receipt = harness.scroll_once().unwrap();
        assert!(receipt.presented);
        assert!(receipt.cols > 0 && receipt.rows > 0);
        let focused = harness
            .state
            .set_focus(&harness.window, &harness.request.session_id, true)
            .unwrap();
        assert!(focused.presented);
        assert!(harness.state.sessions.lock()[&harness.request.session_id].focused);
        assert!(!harness
            .state
            .is_session_render_pending(&harness.request.session_id));
        assert_eq!(
            *harness.events.lock(),
            vec![
                FrameEvent::Acquire,
                FrameEvent::Presented,
                FrameEvent::Acquire,
                FrameEvent::Presented
            ]
        );
        harness.state.detach_session(&harness.request.session_id);
        assert!(matches!(
            harness
                .state
                .render(&harness.window, harness.request.clone()),
            Err(NativeTerminalError::SessionDetached(_))
        ));
    }

    #[tokio::test]
    async fn one_shot_render_requeues_when_frame_is_dropped() {
        let mut harness = DirectRenderHarness::new(vec![
            SimulatedAcquisition::Dropped,
            SimulatedAcquisition::Frame,
        ]);
        let receipt = harness.scroll_once().unwrap();
        assert!(
            !receipt.presented,
            "original direct receipt must remain dropped"
        );
        assert_eq!(
            *harness.events.lock(),
            vec![FrameEvent::Acquire, FrameEvent::Dropped]
        );
        assert!(
            harness
                .state
                .is_session_render_pending(&harness.request.session_id),
            "dropped direct frame must rearm the attached session coordinator"
        );
        harness.execute_dispatched().await;
        assert_eq!(
            *harness.events.lock(),
            vec![
                FrameEvent::Acquire,
                FrameEvent::Dropped,
                FrameEvent::Acquire,
                FrameEvent::Presented
            ]
        );
        assert!(!harness
            .state
            .is_session_render_pending(&harness.request.session_id));
        assert!(harness.dispatched.try_recv().is_err());
        eprintln!("D5_ENTRY: attached daemon history -> scroll_attached_native_terminal -> public render -> Timeout -> original presented=false -> deferred dispatch -> begin_render -> ownership/snapshot -> acquisition success -> presented -> finish_render idle; events={:?}", *harness.events.lock());
    }

    #[tokio::test]
    async fn direct_retry_recovers_lost_then_timeout() {
        let mut harness = DirectRenderHarness::new(vec![
            SimulatedAcquisition::NeedsReconfigure,
            SimulatedAcquisition::Dropped,
            SimulatedAcquisition::Frame,
        ]);
        assert!(!harness.scroll_once().unwrap().presented);
        harness.execute_dispatched().await;
        assert_eq!(
            *harness.events.lock(),
            vec![
                FrameEvent::Acquire,
                FrameEvent::Reconfigure,
                FrameEvent::Acquire,
                FrameEvent::Dropped,
                FrameEvent::Acquire,
                FrameEvent::Presented
            ]
        );
        assert!(!harness
            .state
            .is_session_render_pending(&harness.request.session_id));
        assert!(harness.dispatched.try_recv().is_err());
    }

    #[tokio::test]
    async fn persistent_drop_must_not_immediately_redispatch() {
        // A surface that keeps refusing a drawable (Metal `Timeout`, or an occluded window)
        // must re-arm on the frame clock. Enqueueing the retry inline turns a transient
        // compositor stall into an unbounded main-thread spin.
        let mut harness = DirectRenderHarness::new(
            (0..8)
                .map(|_| SimulatedAcquisition::Dropped)
                .collect::<Vec<_>>(),
        );
        // Timed from before the frame, because the clock measures the budget from frame START:
        // the frame's own cost counts toward the interval, which is the point of a frame clock.
        // An earlier revision timed only the post-frame window, which was correct against a fixed
        // post-frame sleep but contradicts clock-based pacing.
        let started = std::time::Instant::now();
        assert!(!harness.scroll_once().unwrap().presented);
        harness.await_submissions().await;
        let waited = started.elapsed();
        assert!(
            waited >= RETRY_FRAME_INTERVAL,
            "a dropped frame must not retry before one frame interval has passed since the frame began, never re-dispatch inline; waited {waited:?}"
        );
        // No assertion that the queue is empty: eight drops are scripted, so a further retry is
        // legitimately pending here. The deterministic proof of the pacing policy itself lives in
        // frame_clock_tests; this test pins that the dispatch path actually consults the clock.
    }

    #[tokio::test]
    async fn occluded_surface_suspends_instead_of_retrying() {
        // Only ONE acquisition is scripted: a retry would drain the deque and panic, so this
        // fails loudly if occlusion is treated as a transient drop.
        let mut harness = DirectRenderHarness::new(vec![SimulatedAcquisition::Occluded]);
        let receipt = harness.scroll_once().unwrap();
        assert!(!receipt.presented);
        assert!(
            receipt.render_suspended,
            "an occluded surface must report suspension, not a plain dropped frame"
        );
        assert!(
            !harness
                .state
                .is_session_render_pending(&harness.request.session_id),
            "an occluded surface must not arm a retry; it wakes on a visibility command"
        );
        harness.await_submissions().await;
        assert!(
            harness.dispatched.try_recv().is_err(),
            "occlusion must cost zero further dispatches until visibility returns"
        );
    }

    #[tokio::test]
    async fn a_suspended_surface_renders_again_when_output_returns() {
        // Suspension must not be a one-way door. The test above proves occlusion arms no retry,
        // which is only half the contract: if suspending also latched the coordinator, the pane
        // would go dark permanently instead of waking, which is worse than the spin it replaced.
        let mut harness = DirectRenderHarness::new(vec![
            SimulatedAcquisition::Occluded,
            SimulatedAcquisition::Frame,
        ]);

        let suspended = harness.scroll_once().unwrap();
        assert!(suspended.render_suspended);
        assert!(!suspended.presented);

        // Not scroll_once again: it asserts the VT offset moved, and we are already at Top.
        // Driving render directly is what a visibility command ultimately does anyway.
        let resumed = harness
            .state
            .render(&harness.window, harness.request.clone())
            .unwrap();
        assert!(
            resumed.presented,
            "output arriving after an occluded frame must present, not stay suspended"
        );
        assert!(
            !resumed.render_suspended,
            "a surface that presented is no longer suspended"
        );
    }

    #[tokio::test]
    async fn direct_retry_scheduled_timeout_rearms_without_inline_recursion() {
        let mut harness = DirectRenderHarness::new(vec![
            SimulatedAcquisition::Dropped,
            SimulatedAcquisition::Dropped,
            SimulatedAcquisition::Frame,
        ]);
        assert!(!harness.scroll_once().unwrap().presented);
        harness.execute_dispatched().await;
        assert!(harness
            .state
            .is_session_render_pending(&harness.request.session_id));
        harness.execute_dispatched().await;
        assert_eq!(
            *harness.events.lock(),
            vec![
                FrameEvent::Acquire,
                FrameEvent::Dropped,
                FrameEvent::Acquire,
                FrameEvent::Dropped,
                FrameEvent::Acquire,
                FrameEvent::Presented
            ]
        );
        assert!(!harness
            .state
            .is_session_render_pending(&harness.request.session_id));
        assert!(harness.dispatched.try_recv().is_err());
    }

    #[tokio::test]
    async fn direct_retry_focus_preserves_dropped_receipt() {
        let mut harness = DirectRenderHarness::new(vec![
            SimulatedAcquisition::Dropped,
            SimulatedAcquisition::Frame,
        ]);
        let receipt = harness
            .state
            .set_focus(&harness.window, &harness.request.session_id, true)
            .unwrap();
        assert!(!receipt.presented);
        assert!(harness.state.sessions.lock()[&harness.request.session_id].focused);
        harness.execute_dispatched().await;
        assert_eq!(
            *harness.events.lock(),
            vec![
                FrameEvent::Acquire,
                FrameEvent::Dropped,
                FrameEvent::Acquire,
                FrameEvent::Presented
            ]
        );
        assert!(!harness
            .state
            .is_session_render_pending(&harness.request.session_id));
    }

    #[tokio::test]
    async fn direct_retry_detach_or_close_before_dispatch_cannot_reveal_or_resurrect() {
        for close in [false, true] {
            let mut harness = DirectRenderHarness::new(vec![
                SimulatedAcquisition::Dropped,
                SimulatedAcquisition::Frame,
            ]);
            let coordinator = harness
                .state
                .session_render_coordinator(&harness.request.session_id)
                .unwrap();
            assert!(!harness.scroll_once().unwrap().presented);
            if close {
                harness.state.close_session(&harness.request.session_id);
            } else {
                harness.state.detach_session(&harness.request.session_id);
            }
            harness.execute_dispatched().await;
            assert_eq!(
                *harness.events.lock(),
                vec![
                    FrameEvent::Acquire,
                    FrameEvent::Dropped,
                    FrameEvent::Destroyed
                ]
            );
            assert!(!harness.state.has_session_host(&harness.request.session_id));
            assert!(matches!(
                harness
                    .state
                    .ensure_surface_attached(&harness.request.session_id),
                Err(NativeTerminalError::SessionDetached(_))
            ));
            assert!(!coordinator.is_render_pending());
            assert!(harness.dispatched.try_recv().is_err());
        }
    }

    #[tokio::test]
    async fn direct_retry_fatal_errors_do_not_loop() {
        for focus in [false, true] {
            let mut harness = DirectRenderHarness::new(vec![SimulatedAcquisition::Fatal]);
            let result = if focus {
                harness
                    .state
                    .set_focus(&harness.window, &harness.request.session_id, true)
            } else {
                harness.scroll_once()
            };
            assert!(matches!(result, Err(NativeTerminalError::OutOfMemory)));
            assert_eq!(*harness.events.lock(), vec![FrameEvent::Acquire]);
            assert!(!harness
                .state
                .is_session_render_pending(&harness.request.session_id));
            assert!(harness.dispatched.try_recv().is_err());
        }
        let mut harness = DirectRenderHarness::new(vec![
            SimulatedAcquisition::Dropped,
            SimulatedAcquisition::Fatal,
        ]);
        assert!(!harness.scroll_once().unwrap().presented);
        harness.execute_dispatched().await;
        assert_eq!(
            *harness.events.lock(),
            vec![
                FrameEvent::Acquire,
                FrameEvent::Dropped,
                FrameEvent::Acquire
            ]
        );
        assert!(!harness
            .state
            .is_session_render_pending(&harness.request.session_id));
        assert!(harness.dispatched.try_recv().is_err());
    }

    #[tokio::test]
    async fn direct_retry_coalesces_with_existing_pending_render() {
        let mut harness = DirectRenderHarness::new(vec![
            SimulatedAcquisition::Dropped,
            SimulatedAcquisition::Frame,
        ]);
        // A real independent preedit request supplies the already-pending frame, not the missing retry.
        harness
            .window
            .state::<RenderDispatch>()
            .require_deferred
            .store(false, Ordering::SeqCst);
        harness
            .state
            .set_preedit(
                &harness.window,
                &harness.request.session_id,
                Some("x".into()),
            )
            .unwrap();
        harness
            .window
            .state::<RenderDispatch>()
            .require_deferred
            .store(true, Ordering::SeqCst);
        assert!(harness
            .state
            .is_session_render_pending(&harness.request.session_id));
        assert!(!harness.scroll_once().unwrap().presented);
        harness.execute_dispatched().await;
        assert_eq!(
            *harness.events.lock(),
            vec![
                FrameEvent::Acquire,
                FrameEvent::Dropped,
                FrameEvent::Acquire,
                FrameEvent::Presented
            ]
        );
        assert!(!harness
            .state
            .is_session_render_pending(&harness.request.session_id));
        assert!(
            harness.dispatched.try_recv().is_err(),
            "the direct drop must not submit duplicate work"
        );
    }

    #[tokio::test]
    async fn warm_return_reasserts_pty_size_without_replaying_terminal() {
        let state = NativeTerminalSurfaceHostState::default();
        let session_id = "warm-return-size";
        let bounds = LogicalBounds {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 480.0,
            scale_factor: 1.0,
        };
        let (_tx, messages) = tokio::sync::mpsc::channel(1);
        state
            .attach_daemon_attachment_with_bounds::<tauri::Wry>(
                session_id,
                DaemonAttachment {
                    session_id: session_id.into(),
                    epoch: 1,
                    start_sequence: Some(1),
                    end_sequence: Some(1),
                    gap: None,
                    history: bytes::Bytes::from(b"retained screen".to_vec()),
                    history_segments: Vec::new(),
                    pty_cols: Some(80),
                    pty_rows: Some(24),
                    remote_generation: None,
                    messages,
                    stream_task: tokio::spawn(std::future::pending()),
                },
                None,
                Some(bounds),
            )
            .expect("attach");
        let before = state.snapshot_for_session(session_id).unwrap().unwrap();
        state.detach_session(session_id);
        let resizes = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&resizes);
        state.set_pty_resize_sink_if_absent(Arc::new(move |_, cols, rows| {
            observed.lock().push((cols, rows));
        }));

        assert!(state
            .reattach_existing_session_with_bounds(session_id, Some(bounds))
            .unwrap());

        let after = state.snapshot_for_session(session_id).unwrap().unwrap();
        assert_eq!(*resizes.lock(), vec![(before.cols, before.rows)]);
        assert_eq!(before, after);
        state.teardown();
    }

    #[test]
    fn detached_layout_cannot_restore_obsolete_geometry() {
        let state = NativeTerminalSurfaceHostState::default();
        let metrics = font_manager::derived_cell_metrics();
        let request = NativeTerminalBoundsRequest {
            session_id: "detached-layout".into(),
            bounds: LogicalBounds {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 480.0,
                scale_factor: 1.0,
            },
        };
        state
            .prepare_session_layout(request.clone(), metrics)
            .unwrap();
        state.ensure_surface_attached("detached-layout").unwrap();
        state.detach_session("detached-layout");

        let result = state.prepare_session_layout(request, metrics);

        assert!(matches!(
            result,
            Err(NativeTerminalError::SessionDetached(_))
        ));
        assert!(state.session_layout("detached-layout").is_none());
        state.teardown();
    }

    #[test]
    fn presentation_ownership_stays_attached_until_host_guard_is_released() {
        let state = NativeTerminalSurfaceHostState::default();
        let metrics = font_manager::derived_cell_metrics();
        state
            .prepare_session_layout(
                NativeTerminalBoundsRequest {
                    session_id: "presentation-owner".into(),
                    bounds: LogicalBounds {
                        x: 0.0,
                        y: 0.0,
                        width: 800.0,
                        height: 480.0,
                        scale_factor: 1.0,
                    },
                },
                metrics,
            )
            .unwrap();
        let hosts = state.lock_attached_hosts("presentation-owner").unwrap();
        assert!(state.hosts.try_lock().is_none());
        let detached = state.clone();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let detach = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            detached.detach_session("presentation-owner");
        });
        started_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        state.ensure_surface_attached("presentation-owner").unwrap();

        drop(hosts);
        detach.join().unwrap();

        assert!(matches!(
            state.lock_attached_hosts("presentation-owner"),
            Err(NativeTerminalError::SessionDetached(_))
        ));
        assert!(state.session_layout("presentation-owner").is_none());
        state.teardown();
    }

    struct DropRecorder {
        name: &'static str,
        log: Arc<Mutex<Vec<&'static str>>>,
    }

    impl Drop for DropRecorder {
        fn drop(&mut self) {
            self.log.lock().push(self.name);
        }
    }

    /// Mirrors the field order of `NativeSurfaceFrameTarget` to statically and
    /// dynamically prove the drop sequence: `surface` -> `target` -> `renderer`.
    struct SurfaceHostDropOrderSeam {
        _surface: DropRecorder,
        _target: DropRecorder,
        _renderer: DropRecorder,
    }

    #[tokio::test]
    async fn attach_replayed_history_emits_title_for_new_and_existing_sessions() {
        let state = NativeTerminalSurfaceHostState::default();
        let session_id = "replayed-title-session";
        let observed = Arc::new(Mutex::new(Vec::new()));
        let observed_for_sink = Arc::clone(&observed);
        state.set_event_sink(Arc::new(move |event| {
            if let NativeTerminalEvent::Title(payload) = event {
                observed_for_sink.lock().push(payload);
            }
        }));

        for (sequence, title) in [(1, "new-session-title"), (2, "existing-session-title")] {
            let (_tx, messages) = tokio::sync::mpsc::channel(1);
            let attachment = DaemonAttachment {
                session_id: session_id.to_string(),
                epoch: 1,
                start_sequence: Some(sequence),
                end_sequence: Some(sequence),
                gap: None,
                history: bytes::Bytes::from(format!("\x1b]2;{title}\x07").into_bytes()),
                history_segments: Vec::new(),
                pty_cols: None,
                pty_rows: None,
                remote_generation: None,
                messages,
                stream_task: tokio::spawn(std::future::pending()),
            };

            state
                .attach_daemon_attachment::<tauri::Wry>(session_id, attachment, None)
                .expect("attach replayed daemon history");
        }

        assert_eq!(
            *observed.lock(),
            vec![
                NativeTerminalTitlePayload {
                    session_id: session_id.to_string(),
                    title: "new-session-title".to_string(),
                },
                NativeTerminalTitlePayload {
                    session_id: session_id.to_string(),
                    title: "existing-session-title".to_string(),
                },
            ]
        );
        state.teardown();
    }

    #[test]
    fn surface_host_drop_order_guarantees_surface_drops_before_target() {
        let log = Arc::new(Mutex::new(Vec::new()));
        {
            let _seam = SurfaceHostDropOrderSeam {
                _surface: DropRecorder {
                    name: "surface",
                    log: Arc::clone(&log),
                },
                _target: DropRecorder {
                    name: "target",
                    log: Arc::clone(&log),
                },
                _renderer: DropRecorder {
                    name: "renderer",
                    log: Arc::clone(&log),
                },
            };
        }
        let events = log.lock().clone();
        assert_eq!(
            events,
            vec!["surface", "target", "renderer"],
            "Surface must drop before target child view to prevent unparenting NSView while WGPU Surface is active"
        );
    }

    #[test]
    fn safe_async_target_retirement_retains_target_and_drops_surface_first() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let leg_slot = LentSlot::new(DropRecorder {
            name: "surface",
            log: Arc::clone(&log),
        });
        let target = DropRecorder {
            name: "target",
            log: Arc::clone(&log),
        };

        let loan = leg_slot.lend_loan().expect("value starts present");
        let (worker_ready_tx, worker_ready_rx) = std::sync::mpsc::channel();
        let (retire_done_tx, retire_done_rx) = std::sync::mpsc::channel();
        let (worker_done_tx, worker_done_rx) = std::sync::mpsc::channel();

        let worker_handle = std::thread::spawn(move || {
            worker_ready_tx.send(()).unwrap();
            retire_done_rx.recv().unwrap();
            loan.return_to_slot();
            worker_done_tx.send(()).unwrap();
        });

        worker_ready_rx.recv().unwrap();

        let mut frame_target_option = Some(target);
        if let Some(target) = frame_target_option.take() {
            // UI thread retires slot: returns without waiting for borrower.
            // If retire_with blocked on the worker, this would deadlock because
            // the worker is blocked on retire_done_rx.
            leg_slot.retire_with(move || drop(target));
        }

        assert!(
            log.lock().is_empty(),
            "target must be retained while GPU leg is in flight"
        );

        retire_done_tx.send(()).unwrap();
        worker_done_rx.recv().unwrap();
        worker_handle.join().unwrap();

        let events = log.lock().clone();
        assert_eq!(
            events,
            vec!["surface", "target"],
            "Surface must drop before target child view even during asynchronous retirement"
        );
    }

    #[test]
    fn no_global_hosts_mutex_held_during_gpu_work() {
        let state = NativeTerminalSurfaceHostState::default();
        let session_id = "hosts-lock-free-gpu";
        let cell_metrics = font_manager::derived_cell_metrics();
        let _ = state.prepare_session_layout(
            NativeTerminalBoundsRequest {
                session_id: session_id.into(),
                bounds: LogicalBounds {
                    x: 0.0,
                    y: 0.0,
                    width: 640.0,
                    height: 384.0,
                    scale_factor: 1.0,
                },
            },
            cell_metrics,
        );

        let (gpu_started_tx, gpu_started_rx) = std::sync::mpsc::channel();
        let (gpu_continue_tx, gpu_continue_rx) = std::sync::mpsc::channel();

        let worker = state.gpu_worker();
        worker.enqueue(move |_gpu| {
            gpu_started_tx.send(()).unwrap();
            gpu_continue_rx.recv().unwrap();
        });

        gpu_started_rx.recv().unwrap();
        let hosts_guard = state.hosts.try_lock();
        assert!(
            hosts_guard.is_some(),
            "hosts lock must not be held across GPU worker execution"
        );
        drop(hosts_guard);

        gpu_continue_tx.send(()).unwrap();
    }

    #[test]
    fn retired_completion_preserves_reattached_frame_reservation() {
        let coordinator = RenderScheduleCoordinator::new();
        let slot = LentSlot::new(());
        assert!(coordinator.schedule_render());
        assert!(coordinator.begin_render());
        let loan = slot.lend_loan().unwrap();
        coordinator.consume_render();
        slot.retire_with(|| {});
        assert!(coordinator.schedule_render());
        loan.return_to_slot();
        assert!(discard_retired_completion(&slot));
        assert!(coordinator.begin_render(), "old completion cancelled replacement frame");
    }

    #[test]
    fn fatal_gpu_completion_does_not_rearm_without_new_input() {
        let coordinator = RenderScheduleCoordinator::new();
        assert!(coordinator.schedule_render());
        assert!(coordinator.begin_render());
        let result = Err(NativeTerminalError::GpuPipelineError("surface recovery failed".into()));
        if gpu_completion_requires_retry(&result) {
            coordinator.schedule_render();
        }
        assert!(!coordinator.finish_render(), "fatal error created another GPU job");
        assert!(!coordinator.is_render_pending());
    }

    #[test]
    fn panicking_frame_returns_error_and_worker_accepts_next_frame() {
        let worker = GpuWorker::new("ferryx-panic-frame").unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let first = tx.clone();
        assert!(worker.enqueue(move |_| {
            let result = run_gpu_frame::<()>(|| panic!("injected frame panic"));
            first.send(result.is_err()).unwrap();
        }));
        assert!(worker.enqueue(move |_| {
            tx.send(run_gpu_frame(|| Ok(())).is_ok()).unwrap();
        }));
        for _ in 0..2 {
            assert!(rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap());
        }
    }

    #[tokio::test]
    async fn one_panes_vt_work_cannot_block_another_panes_render_preparation() {
        let (dispatch, mut dispatched) = tokio::sync::mpsc::unbounded_channel();
        let app = tauri::test::mock_builder()
            .manage(RenderDispatch {
                sender: dispatch,
                owner_thread: std::thread::current().id(),
                require_deferred: std::sync::atomic::AtomicBool::new(false),
                submissions: Mutex::new(Vec::new()),
            })
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let _window = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
            .build()
            .unwrap();
        let window = _window.as_ref().window();

        let state = NativeTerminalSurfaceHostState::default();
        let session_a = "pane-a";
        let session_b = "pane-b";
        let cell_metrics = font_manager::derived_cell_metrics();

        let _ = state.prepare_session_layout(
            NativeTerminalBoundsRequest {
                session_id: session_a.into(),
                bounds: LogicalBounds {
                    x: 0.0,
                    y: 0.0,
                    width: 640.0,
                    height: 384.0,
                    scale_factor: 1.0,
                },
            },
            cell_metrics,
        );
        let _ = state.prepare_session_layout(
            NativeTerminalBoundsRequest {
                session_id: session_b.into(),
                bounds: LogicalBounds {
                    x: 0.0,
                    y: 0.0,
                    width: 640.0,
                    height: 384.0,
                    scale_factor: 1.0,
                },
            },
            cell_metrics,
        );

        let injected_host = NativeTerminalSurfaceHost {
            frame_target: HostFrameTarget::Injected(InjectedFrameTarget {
                acquisitions: vec![SimulatedAcquisition::Frame].into(),
                cell_metrics,
                events: Arc::new(Mutex::new(Vec::new())),
                defer_direct: false,
                assert_host_locked: Box::new(|| {}),
            }),
            layout: state.session_layout(session_b),
            presentation_slot: state.session_snapshot_slot(session_b).map(|slot| Arc::downgrade(&slot)),
            logical_bounds: Some(LogicalBounds {
                x: 0.0,
                y: 0.0,
                width: 640.0,
                height: 384.0,
                scale_factor: 1.0,
            }),
        };
        state.hosts.lock().insert(session_b.to_string(), injected_host);

        // Pane B has its own render coordinator and snapshot slot obtained before Pane A's lock
        let coordinator = state.session_render_coordinator(session_b).unwrap();
        let slot = state.session_snapshot_slot(session_b).unwrap();
        coordinator.schedule_render();

        // Thread A holds the global sessions mutex (simulating VT feed or agent detect on pane A)
        let (held_tx, held_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let sessions = Arc::clone(&state.sessions);
        std::thread::spawn(move || {
            let guard = sessions.lock();
            held_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            drop(guard);
        });
        held_rx.recv().unwrap();
        dispatch_scheduled_render(
            window,
            Arc::clone(&state.hosts),
            slot,
            session_b.to_string(),
            coordinator,
            Arc::clone(state.gpu_worker()),
        );

        let task = tokio::time::timeout(std::time::Duration::from_secs(2), dispatched.recv())
            .await
            .expect("timeout waiting for dispatched task")
            .expect("dispatched channel closed");
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let handle = std::thread::spawn(move || {
            task();
            let _ = done_tx.send(());
        });

        let completed = done_rx.recv_timeout(std::time::Duration::from_millis(200));
        let _ = release_tx.send(());
        let _ = handle.join();
        assert!(completed.is_ok(), "Pane B's render preparation must not block on Pane A's sessions lock");
    }

    #[tokio::test]
    async fn stale_completion_does_not_reveal_detached_target() {
        let (dispatch, mut dispatched) = tokio::sync::mpsc::unbounded_channel();
        let app = tauri::test::mock_builder()
            .manage(RenderDispatch {
                sender: dispatch,
                owner_thread: std::thread::current().id(),
                require_deferred: std::sync::atomic::AtomicBool::new(false),
                submissions: Mutex::new(Vec::new()),
            })
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let _window = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
            .build()
            .unwrap();

        let state = NativeTerminalSurfaceHostState::default();
        let session_id = "stale-completion-detached";
        let bounds = LogicalBounds {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 480.0,
            scale_factor: 1.0,
        };

        let (_output, messages) = tokio::sync::mpsc::channel(1);
        state
            .attach_daemon_attachment_with_bounds::<tauri::test::MockRuntime>(
                session_id,
                DaemonAttachment {
                    session_id: session_id.into(),
                    epoch: 1,
                    start_sequence: Some(1),
                    end_sequence: Some(1),
                    gap: None,
                    history: b"test output\r\n".to_vec().into(),
                    history_segments: Vec::new(),
                    pty_cols: Some(80),
                    pty_rows: Some(24),
                    remote_generation: None,
                    messages,
                    stream_task: tokio::spawn(std::future::pending()),
                },
                Some(app.handle().clone()),
                Some(bounds),
            )
            .unwrap();

        assert!(state.ensure_surface_attached(session_id).is_ok());

        // Detach session while pipeline is active
        state.detach_session(session_id);

        assert!(!state.has_session_host(session_id));
        assert!(matches!(
            state.ensure_surface_attached(session_id),
            Err(NativeTerminalError::SessionDetached(_))
        ));

        // Process any queued dispatches
        while let Ok(task) = dispatched.try_recv() {
            task();
        }

        // Host must not be resurrected and target must remain detached
        assert!(!state.has_session_host(session_id));
        assert!(matches!(
            state.ensure_surface_attached(session_id),
            Err(NativeTerminalError::SessionDetached(_))
        ));
    }

    #[tokio::test]
    async fn ended_output_pump_rejects_input_until_reattached() {
        let state = NativeTerminalSurfaceHostState::default();
        let session_id = "ended-output-pump";
        let (output, messages) = tokio::sync::mpsc::channel(1);
        state.attach_daemon_attachment_with_bounds::<tauri::test::MockRuntime>(
            session_id,
            DaemonAttachment {
                session_id: session_id.into(),
                epoch: 1,
                start_sequence: None,
                end_sequence: None,
                gap: None,
                history: Vec::new().into(),
                history_segments: Vec::new(),
                pty_cols: Some(80),
                pty_rows: Some(24),
                remote_generation: None,
                messages,
                stream_task: tokio::spawn(std::future::pending()),
            },
            None,
            None,
        ).unwrap();
        let pump = state.sessions.lock().get_mut(session_id).unwrap().pump_task.take().unwrap();
        drop(output);
        tokio::time::timeout(std::time::Duration::from_secs(3), pump).await.unwrap().unwrap();
        let result = state.encode_attached_input(session_id, &NativeTerminalInput::Text { text: "x".into() });
        assert!(matches!(result, Err(NativeTerminalError::SessionDetached(_))));
        state.teardown();
    }

    #[test]
    fn ghostty_grid_resize_notifies_pty_with_matching_dimensions() {
        let state = NativeTerminalSurfaceHostState::default();
        let session_id = "pty-resize-contract";
        let cell_metrics = font_manager::derived_cell_metrics();
        let request = |width| NativeTerminalBoundsRequest {
            session_id: session_id.to_string(),
            bounds: LogicalBounds {
                x: 0.0,
                y: 0.0,
                width,
                height: 480.0,
                scale_factor: 1.0,
            },
        };
        state
            .prepare_session_layout(request(400.0), cell_metrics)
            .expect("create initial grid");

        let observed = Arc::new(Mutex::new(Vec::new()));
        let observed_for_sink = Arc::clone(&observed);
        assert!(state.set_pty_resize_sink_if_absent(Arc::new(
            move |resized_session_id, cols, rows| {
                observed_for_sink
                    .lock()
                    .push((resized_session_id.to_string(), cols, rows));
            },
        )));

        let layout = state
            .prepare_session_layout(request(800.0), cell_metrics)
            .expect("resize grid");

        assert_eq!(
            observed.lock().as_slice(),
            &[(session_id.to_string(), layout.cols, layout.rows)],
            "the PTY resize path must receive the exact ghostty grid dimensions"
        );
        state.teardown();
    }

    #[tokio::test]
    async fn warm_reattach_activates_snapshot_slot_and_consumes_frame() {
        let state = NativeTerminalSurfaceHostState::default();
        let session_id = "warm-reattach-slot-test";
        let bounds = LogicalBounds {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 480.0,
            scale_factor: 1.0,
        };

        let (_output, messages) = tokio::sync::mpsc::channel(1);
        state
            .attach_daemon_attachment_with_bounds::<tauri::test::MockRuntime>(
                session_id,
                DaemonAttachment {
                    session_id: session_id.into(),
                    epoch: 1,
                    start_sequence: Some(1),
                    end_sequence: Some(1),
                    gap: None,
                    history: b"test\r\n".to_vec().into(),
                    history_segments: Vec::new(),
                    pty_cols: Some(80),
                    pty_rows: Some(24),
                    remote_generation: None,
                    messages,
                    stream_task: tokio::spawn(std::future::pending()),
                },
                None,
                Some(bounds),
            )
            .unwrap();

        let slot = state.session_snapshot_slot(session_id).expect("slot exists");
        assert!(slot.is_attached(), "slot must be attached initially");

        // Detach
        state.detach_session(session_id);
        assert!(!slot.is_attached(), "slot must be inactive after detach");
        assert!(slot.consume().is_none(), "detached slot cannot consume frame");

        // Warm reattach
        let reattached = state
            .reattach_existing_session_with_bounds(session_id, Some(bounds))
            .expect("reattach succeeds");
        assert!(reattached, "session reattached");
        assert!(slot.is_attached(), "warm reattach must restore slot attached state");
        assert!(slot.consume().is_some(), "warm reattach must allow consuming newly published frame");

        state.teardown();
    }

    #[tokio::test]
    async fn detached_session_scheduled_render_cannot_resurrect_host() {
        let (dispatch, mut dispatched) = tokio::sync::mpsc::unbounded_channel();
        let app = tauri::test::mock_builder()
            .manage(RenderDispatch {
                sender: dispatch,
                owner_thread: std::thread::current().id(),
                require_deferred: std::sync::atomic::AtomicBool::new(false),
                submissions: Mutex::new(Vec::new()),
            })
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let webview_window = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
            .build()
            .unwrap();
        let window = webview_window.as_ref().window().clone();

        let state = NativeTerminalSurfaceHostState::default();
        let session_id = "resurrection-prevention-test";
        let bounds = LogicalBounds {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 480.0,
            scale_factor: 1.0,
        };

        let (_output, messages) = tokio::sync::mpsc::channel(1);
        state
            .attach_daemon_attachment_with_bounds::<tauri::test::MockRuntime>(
                session_id,
                DaemonAttachment {
                    session_id: session_id.into(),
                    epoch: 1,
                    start_sequence: Some(1),
                    end_sequence: Some(1),
                    gap: None,
                    history: b"test\r\n".to_vec().into(),
                    history_segments: Vec::new(),
                    pty_cols: Some(80),
                    pty_rows: Some(24),
                    remote_generation: None,
                    messages,
                    stream_task: tokio::spawn(std::future::pending()),
                },
                None,
                Some(bounds),
            )
            .unwrap();

        let slot = state.session_snapshot_slot(session_id).expect("slot exists");
        let coordinator = state.session_render_coordinator(session_id).expect("coord");

        // Schedule a render before detach
        assert!(coordinator.schedule_render());
        dispatch_scheduled_render(
            window.clone(),
            Arc::clone(&state.hosts),
            Arc::clone(&slot),
            session_id.to_string(),
            Arc::clone(&coordinator),
            Arc::clone(state.gpu_worker()),
        );

        // Receive the dispatched main-thread closure
        let task = tokio::time::timeout(std::time::Duration::from_secs(2), dispatched.recv())
            .await
            .expect("timeout waiting for task")
            .expect("task received");

        // Now detach the session BEFORE task executes
        state.detach_session(session_id);
        assert!(!state.has_session_host(session_id));
        assert!(!slot.is_attached());

        // Re-arm coordinator so begin_render() returns true and enters slot.consume() + hosts.lock().
        // This forces the dispatched closure to execute down to the production slot.is_attached_with_epoch(frame_epoch) guard!
        coordinator.schedule_render();

        // Execute the dispatched task
        task();

        // Verify that executing the stale task did NOT resurrect the host!
        assert!(
            !state.has_session_host(session_id),
            "stale scheduled dispatch must not resurrect host after detach"
        );

        state.teardown();
    }

    #[tokio::test]
    async fn toctou_resurrection_race_is_prevented_by_epoch_check() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let webview_window = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
            .build()
            .unwrap();
        let window = webview_window.as_ref().window().clone();

        let state = NativeTerminalSurfaceHostState::default();
        let session_id = "toctou-resurrection-race-test";
        let bounds = LogicalBounds {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 480.0,
            scale_factor: 1.0,
        };

        let (_output, messages) = tokio::sync::mpsc::channel(1);
        state
            .attach_daemon_attachment_with_bounds::<tauri::test::MockRuntime>(
                session_id,
                DaemonAttachment {
                    session_id: session_id.into(),
                    epoch: 1,
                    start_sequence: Some(1),
                    end_sequence: Some(1),
                    gap: None,
                    history: b"test\r\n".to_vec().into(),
                    history_segments: Vec::new(),
                    pty_cols: Some(80),
                    pty_rows: Some(24),
                    remote_generation: None,
                    messages,
                    stream_task: tokio::spawn(std::future::pending()),
                },
                None,
                Some(bounds),
            )
            .unwrap();

        let slot = state.session_snapshot_slot(session_id).expect("slot exists");

        // Step 1: Consume a frame before detach (simulating Thread 1 having reached line 434)
        let frame = slot.consume().expect("frame available initially");
        let frame_epoch = frame.attachment_epoch;
        assert_eq!(frame_epoch, slot.current_epoch());

        // Step 2: Detach session (simulating Thread 2 running detach_session while Thread 1 waits for hosts.lock)
        state.detach_session(session_id);
        assert!(!state.has_session_host(session_id));
        assert!(!slot.is_attached());

        // Step 3: Now Thread 1 acquires hosts.lock() and attempts to process the pre-detach consumed frame.
        // With the Blocker 1 guard, slot.is_attached_with_epoch(frame_epoch) is false!
        assert!(
            !slot.is_attached_with_epoch(frame_epoch),
            "stale frame epoch must be rejected after detach"
        );

        let mut hosts_guard = state.hosts.lock();
        let resurrected = if !slot.is_attached_with_epoch(frame_epoch) {
            false
        } else {
            if let std::collections::hash_map::Entry::Vacant(entry) = hosts_guard.entry(session_id.to_string()) {
                if let Ok(new_host) = NativeTerminalSurfaceHost::new(&window, bounds.scale_factor) {
                    entry.insert(new_host);
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };
        drop(hosts_guard);

        assert!(!resurrected, "Blocker 1 check must prevent lazy host resurrection");
        assert!(!state.has_session_host(session_id), "host must remain vacant");

        state.teardown();
    }

    #[tokio::test]
    async fn inflight_gpu_frame_cancelled_when_session_detached_or_reattached() {
        let state = NativeTerminalSurfaceHostState::default();
        let session_id = "inflight-gpu-cancel-test";
        let bounds = LogicalBounds {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 480.0,
            scale_factor: 1.0,
        };

        let (_output, messages) = tokio::sync::mpsc::channel(1);
        state
            .attach_daemon_attachment_with_bounds::<tauri::test::MockRuntime>(
                session_id,
                DaemonAttachment {
                    session_id: session_id.into(),
                    epoch: 1,
                    start_sequence: Some(1),
                    end_sequence: Some(1),
                    gap: None,
                    history: b"test\r\n".to_vec().into(),
                    history_segments: Vec::new(),
                    pty_cols: Some(80),
                    pty_rows: Some(24),
                    remote_generation: None,
                    messages,
                    stream_task: tokio::spawn(std::future::pending()),
                },
                None,
                Some(bounds),
            )
            .unwrap();

        let slot = state.session_snapshot_slot(session_id).expect("slot exists");
        let initial_frame = slot.consume().expect("initial frame exists");
        let initial_epoch = initial_frame.attachment_epoch;

        // While frame is in flight to GPU worker, session detaches
        state.detach_session(session_id);
        assert!(!slot.is_attached_with_epoch(initial_epoch));

        // Warm reattach bumps epoch again
        state
            .reattach_existing_session_with_bounds(session_id, Some(bounds))
            .expect("reattach succeeds");
        assert!(slot.is_attached());
        assert_ne!(slot.current_epoch(), initial_epoch);
        // Stale in-flight frame is STILL rejected because epoch does not match
        assert!(
            !slot.is_attached_with_epoch(initial_epoch),
            "in-flight GPU frame from previous attachment must be rejected on new attachment"
        );

        state.teardown();
    }

    fn preedit_test_snapshot(cols: u16, rows: u16, cursor_x: u16, cursor_y: u16) -> RenderSnapshot {
        RenderSnapshot {
            images: Vec::new(),
            cols,
            rows,
            cursor: super::super::cursor::CursorSnapshot {
                x: cursor_x,
                y: cursor_y,
                visible: true,
                blinking: false,
                wide_tail: false,
                visual_style: super::super::cursor::CursorVisualStyle::Block,
            },
            grid: vec![vec![CellSnapshot::default(); cols as usize]; rows as usize],
        }
    }

    #[test]
    fn preedit_narrow_ascii_overwrites_cells_with_underlines_without_moving_cursor() {
        let mut snapshot = preedit_test_snapshot(5, 2, 1, 1);
        let original_cursor = snapshot.cursor.clone();

        apply_preedit_to_snapshot(&mut snapshot, "ab");

        assert_eq!(snapshot.cursor, original_cursor);
        assert_eq!(snapshot.grid[1][1].text, "a");
        assert_eq!(snapshot.grid[1][1].wide, CellWide::Narrow);
        assert!(snapshot.grid[1][1].underline);
        assert_eq!(snapshot.grid[1][2].text, "b");
        assert_eq!(snapshot.grid[1][2].wide, CellWide::Narrow);
        assert!(snapshot.grid[1][2].underline);
    }

    #[test]
    fn preedit_hangul_syllable_writes_wide_cell_and_underlined_spacer_tail() {
        let mut snapshot = preedit_test_snapshot(4, 1, 1, 0);

        apply_preedit_to_snapshot(&mut snapshot, "한");

        assert_eq!(snapshot.grid[0][1].text, "한");
        assert_eq!(snapshot.grid[0][1].wide, CellWide::Wide);
        assert!(snapshot.grid[0][1].underline);
        assert_eq!(snapshot.grid[0][2].text, "");
        assert_eq!(snapshot.grid[0][2].wide, CellWide::SpacerTail);
        assert!(snapshot.grid[0][2].underline);
    }

    #[test]
    fn preedit_clamps_at_row_end_without_writing_out_of_bounds() {
        let mut snapshot = preedit_test_snapshot(3, 1, 2, 0);

        apply_preedit_to_snapshot(&mut snapshot, "abc");

        assert_eq!(snapshot.grid[0].len(), 3);
        assert_eq!(snapshot.grid[0][2].text, "a");
        assert!(snapshot.grid[0][2].underline);
    }

    #[test]
    fn empty_preedit_leaves_snapshot_grid_unchanged() {
        let mut snapshot = preedit_test_snapshot(3, 1, 1, 0);
        snapshot.grid[0][1].text = "existing".to_string();
        snapshot.grid[0][1].bold = true;
        let original_grid = snapshot.grid.clone();

        apply_preedit_to_snapshot(&mut snapshot, "");

        assert_eq!(snapshot.grid, original_grid);
    }

    #[test]
    fn preedit_fully_replaces_existing_cell_text_colors_and_attributes() {
        let mut snapshot = preedit_test_snapshot(2, 1, 0, 0);
        snapshot.grid[0][0] = CellSnapshot {
            text: "old".to_string(),
            wide: CellWide::SpacerHead,
            fg: Some(super::super::color::ColorRgb { r: 1, g: 2, b: 3 }),
            bg: Some(super::super::color::ColorRgb { r: 4, g: 5, b: 6 }),
            fg_ident: None,
            underline_ident: None,
            bold: true,
            italic: true,
            underline: false,
            inverse: true,
            faint: true,
            blink: true,
            invisible: true,
            strikethrough: true,
            overline: true,
        };

        apply_preedit_to_snapshot(&mut snapshot, "x");

        assert_eq!(
            snapshot.grid[0][0],
            CellSnapshot {
                text: "x".to_string(),
                underline: true,
                ..Default::default()
            }
        );
    }

    #[test]
    fn surface_receipt_carries_render_snapshot_cursor_geometry() {
        let cell_metrics = font_manager::derived_cell_metrics();
        let layout = SurfaceCompositionLayout::compute(
            &LogicalBounds {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 480.0,
                scale_factor: 1.0,
            },
            &cell_metrics,
        )
        .expect("valid surface bounds");
        let (snapshot, _) = snapshot_for_layout(layout);

        let receipt = NativeTerminalSurfaceReceipt::from_snapshot(
            layout,
            &snapshot,
            4,
            20,
            cell_metrics,
            None,
        );

        assert_eq!(receipt.cursor_col, snapshot.cursor.x);
        assert_eq!(receipt.cursor_row, snapshot.cursor.y);
        assert_eq!(receipt.cell_width_px, cell_metrics.width_px);
        assert_eq!(receipt.cell_height_px, cell_metrics.height_px);
        assert_eq!(receipt.rebuilt_rows, 4);
        assert_eq!(receipt.reused_rows, 20);
        assert!(!receipt.presented);
        assert_eq!(receipt.effective_scale_factor, None);
    }

    #[test]
    fn receipt_reports_effective_presentation_scale() {
        let state = NativeTerminalSurfaceHostState::default();
        let bounds = SurfacePresentationGeometry::WaylandSubsurface
            .resolve(LogicalBounds {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 480.0,
                scale_factor: 1.5,
            })
            .expect("resolved presentation bounds");
        let cell_metrics = CellMetrics {
            width_px: 16,
            height_px: 32,
        };
        state
            .prepare_session_layout(
                NativeTerminalBoundsRequest {
                    session_id: "receipt-scale".into(),
                    bounds,
                },
                cell_metrics,
            )
            .expect("stored layout");
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        let receipt = state
            .get_receipt(&window.as_ref().window(), "receipt-scale")
            .expect("session receipt");
        assert_eq!(
            state.sessions.lock()["receipt-scale"]
                .logical_bounds
                .unwrap()
                .scale_factor,
            2.0,
        );
        assert_eq!(receipt.effective_scale_factor, Some(2.0));
        assert_eq!((receipt.cell_width_px, receipt.cell_height_px), (16, 32));
        state.teardown();
    }

    #[test]
    fn render_schedule_coordinator_stays_pending_until_frame_completion() {
        let coordinator = RenderScheduleCoordinator::new();

        assert!(coordinator.schedule_render());
        assert!(coordinator.begin_render());
        assert!(
            coordinator.is_render_pending(),
            "starting a frame must not clear the pending marker before presentation completes"
        );

        assert!(
            !coordinator.schedule_render(),
            "output arriving during the frame must coalesce into one follow-up"
        );
        assert!(coordinator.is_render_pending());
        assert!(
            coordinator.finish_render(),
            "frame completion must report the coalesced follow-up"
        );
        assert!(
            coordinator.is_render_pending(),
            "the coalesced follow-up must remain pending until its frame completes"
        );

        assert!(coordinator.begin_render());
        assert!(
            !coordinator.finish_render(),
            "a clean frame completion must not request another frame"
        );
        assert!(!coordinator.is_render_pending());
    }

    #[test]
    fn render_schedule_coordinator_burst_coalescing_and_consumption_lifecycle() {
        let coordinator = RenderScheduleCoordinator::new();
        assert!(!coordinator.is_render_pending());
        assert!(!coordinator.consume_render());

        // First schedule succeeds
        assert!(coordinator.schedule_render());
        assert!(coordinator.is_render_pending());

        // Burst is coalesced
        for _ in 0..100 {
            assert!(!coordinator.schedule_render());
        }
        assert!(coordinator.is_render_pending());

        // Consume work resets pending
        assert!(coordinator.consume_render());
        assert!(!coordinator.is_render_pending());

        // Second render is now schedulable
        assert!(coordinator.schedule_render());
        assert!(coordinator.is_render_pending());
        assert!(coordinator.consume_render());
    }

    #[tokio::test]
    async fn driver_edge_triggered_agent_state_emission() {
        let state = NativeTerminalSurfaceHostState::default();
        let session_id = "edge-triggered-session";
        let observed_states = Arc::new(Mutex::new(Vec::new()));
        let observed_for_sink = Arc::clone(&observed_states);
        state.set_event_sink(Arc::new(move |event| {
            if let NativeTerminalEvent::AgentState(payload) = event {
                observed_for_sink.lock().push(payload);
            }
        }));

        // First feed: triggers 'working'
        {
            let (_tx, messages) = tokio::sync::mpsc::channel(1);
            let attachment = DaemonAttachment {
                session_id: session_id.to_string(),
                epoch: 1,
                start_sequence: Some(1),
                end_sequence: Some(1),
                gap: None,
                history: bytes::Bytes::from(b"Working (esc to interrupt)\r\n".to_vec()),
                history_segments: Vec::new(),
                pty_cols: None,
                pty_rows: None,
                remote_generation: None,
                messages,
                stream_task: tokio::spawn(std::future::pending()),
            };
            state
                .attach_daemon_attachment::<tauri::Wry>(session_id, attachment, None)
                .expect("attach first daemon history");
        }

        // Check first emission
        assert_eq!(observed_states.lock().len(), 1);
        assert_eq!(observed_states.lock()[0].state, "working");

        // Second feed with same 'working' state: must NOT emit duplicate event
        {
            let (_tx, messages) = tokio::sync::mpsc::channel(1);
            let attachment = DaemonAttachment {
                session_id: session_id.to_string(),
                epoch: 1,
                start_sequence: Some(2),
                end_sequence: Some(2),
                gap: None,
                history: bytes::Bytes::from(b"Still Working (esc to interrupt)\r\n".to_vec()),
                history_segments: Vec::new(),
                pty_cols: None,
                pty_rows: None,
                remote_generation: None,
                messages,
                stream_task: tokio::spawn(std::future::pending()),
            };
            state
                .attach_daemon_attachment::<tauri::Wry>(session_id, attachment, None)
                .expect("attach second daemon history");
        }

        // Count should still be 1 (edge-triggered)
        assert_eq!(observed_states.lock().len(), 1);

        // Third feed: triggers state transition to 'blocked'
        {
            let (_tx, messages) = tokio::sync::mpsc::channel(1);
            let attachment = DaemonAttachment {
                session_id: session_id.to_string(),
                epoch: 1,
                start_sequence: Some(3),
                end_sequence: Some(3),
                gap: None,
                history: bytes::Bytes::from(b"\x1b[2J\x1b[HAction Required: allow command?\r\npress enter to confirm or esc to cancel\r\n".to_vec()),
                history_segments: Vec::new(),
                pty_cols: None,
                pty_rows: None, remote_generation: None,
                messages,
                stream_task: tokio::spawn(std::future::pending()),
            };
            state
                .attach_daemon_attachment::<tauri::Wry>(session_id, attachment, None)
                .expect("attach third daemon history");
        }

        // Now count should be 2, with new state 'blocked'
        assert_eq!(observed_states.lock().len(), 2);
        assert_eq!(observed_states.lock()[1].state, "blocked");

        state.teardown();
    }

    #[tokio::test]
    async fn detached_session_still_reports_agent_state_transitions() {
        let state = NativeTerminalSurfaceHostState::default();
        let session_id = "backgrounded-session";
        let observed_states = Arc::new(Mutex::new(Vec::new()));
        let observed_for_sink = Arc::clone(&observed_states);
        state.set_event_sink(Arc::new(move |event| {
            if let NativeTerminalEvent::AgentState(payload) = event {
                observed_for_sink.lock().push(payload.state);
            }
        }));

        let (tx, messages) = tokio::sync::mpsc::channel(4);
        let attachment = DaemonAttachment {
            session_id: session_id.to_string(),
            epoch: 1,
            start_sequence: Some(1),
            end_sequence: Some(1),
            gap: None,
            history: bytes::Bytes::from(b"Working (esc to interrupt)\r\n".to_vec()),
            history_segments: Vec::new(),
            pty_cols: None,
            pty_rows: None,
            remote_generation: None,
            messages,
            stream_task: tokio::spawn(std::future::pending()),
        };
        state
            .attach_daemon_attachment::<tauri::Wry>(session_id, attachment, None)
            .expect("attach daemon history");
        assert_eq!(observed_states.lock().clone(), vec!["working".to_string()]);

        // The pane scrolls off screen: React unmounts it and the UI detaches the surface.
        state.detach_session(session_id);

        // The agent keeps running in the daemon and now blocks for input.
        tx.send(DaemonStreamMessage::Output {
            session_id: session_id.into(),
            sequence: 2,
            data: b"\x1b[2J\x1b[HAction Required: allow command?\r\npress enter to confirm or esc to cancel\r\n"
                .to_vec()
                .into(),
            metrics_read_unix_micros: None,
        })
        .await
        .expect("send daemon output to a detached session");

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while observed_states.lock().len() < 2 && std::time::Instant::now() < deadline {
            tokio::task::yield_now().await;
        }

        assert_eq!(
            observed_states.lock().clone(),
            vec!["working".to_string(), "blocked".to_string()],
            "a backgrounded pane must keep reporting agent state; otherwise its spinner spins forever"
        );

        state.teardown();
    }
    #[tokio::test]
    async fn extension_reported_state_wins_over_screen_inference() {
        let state = NativeTerminalSurfaceHostState::default();
        let session_id = "extension-owned-session";
        let observed = Arc::new(Mutex::new(Vec::new()));
        let observed_for_sink = Arc::clone(&observed);
        let (reported, mut reports) = tokio::sync::mpsc::unbounded_channel();
        state.set_event_sink(Arc::new(move |event| {
            if let NativeTerminalEvent::AgentState(payload) = event {
                observed_for_sink
                    .lock()
                    .push((payload.state, payload.manifest_id));
                reported.send(()).expect("report receiver alive");
            }
        }));

        let (tx, messages) = tokio::sync::mpsc::channel(4);
        let attachment = DaemonAttachment {
            session_id: session_id.to_string(),
            epoch: 1,
            start_sequence: Some(1),
            end_sequence: Some(1),
            gap: None,
            history: bytes::Bytes::new(),
            history_segments: Vec::new(),
            pty_cols: None,
            pty_rows: None,
            remote_generation: None,
            messages,
            stream_task: tokio::spawn(std::future::pending()),
        };
        state
            .attach_daemon_attachment::<tauri::Wry>(session_id, attachment, None)
            .expect("attach");

        tx.send(DaemonStreamMessage::AgentState {
            session_id: session_id.into(),
            state: "blocked".into(),
            agent: Some("omo".into()),
            provider_session: None,
            detail: None,
            is_snapshot: false,
            origin: crate::daemon::protocol::AgentStateOrigin::Agent,
        })
        .await
        .expect("send agent state report");

        tokio::time::timeout(std::time::Duration::from_secs(5), reports.recv())
            .await
            .expect("extension report delivered")
            .expect("event sink alive");
        assert_eq!(
            observed.lock().clone(),
            vec![("blocked".to_string(), "omo".to_string())]
        );

        let mut updates = state.sessions.lock()[session_id].update_sender.subscribe();
        tx.send(DaemonStreamMessage::Output {
            session_id: session_id.into(),
            sequence: 2,
            data: b"  Working (esc to interrupt)\r\n".to_vec().into(),
            metrics_read_unix_micros: None,
        })
        .await
        .expect("send screen output");

        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                updates.changed().await.expect("native pump alive");
                if state.sessions.lock()[session_id].last_sequence == Some(2) {
                    break;
                }
            }
        })
        .await
        .expect("screen output processed");
        assert_eq!(
            observed.lock().len(),
            1,
            "screen inference must stay disabled once the agent reports its own state"
        );

        state.teardown();
    }

    #[tokio::test]
    async fn an_agents_own_idle_keeps_screen_inference_disabled() {
        // An agent that finishes a turn reports `idle` and keeps running. Its final frame is
        // still on screen, spinner and "esc to interrupt" footer included, so handing the
        // session back to screen inference here re-promotes the pane to "working" with no work
        // in flight — exactly the phantom-running state users see.
        let state = NativeTerminalSurfaceHostState::default();
        let session_id = "self-reported-idle-session";
        let observed = Arc::new(Mutex::new(Vec::new()));
        let observed_for_sink = Arc::clone(&observed);
        let (reported, mut reports) = tokio::sync::mpsc::unbounded_channel();
        state.set_event_sink(Arc::new(move |event| {
            if let NativeTerminalEvent::AgentState(payload) = event {
                observed_for_sink.lock().push(payload.state);
                reported.send(()).expect("report receiver alive");
            }
        }));

        let (tx, messages) = tokio::sync::mpsc::channel(4);
        state
            .attach_daemon_attachment::<tauri::Wry>(
                session_id,
                DaemonAttachment {
                    session_id: session_id.to_string(),
                    epoch: 1,
                    start_sequence: Some(1),
                    end_sequence: Some(1),
                    gap: None,
                    history: bytes::Bytes::new(),
                    history_segments: Vec::new(),
                    pty_cols: None,
                    pty_rows: None,
                    remote_generation: None,
                    messages,
                    stream_task: tokio::spawn(std::future::pending()),
                },
                None,
            )
            .expect("attach");

        for reported_state in ["working", "idle"] {
            tx.send(DaemonStreamMessage::AgentState {
                session_id: session_id.into(),
                state: reported_state.into(),
                agent: Some("omo".into()),
                provider_session: None,
                detail: None,
                is_snapshot: false,
                origin: crate::daemon::protocol::AgentStateOrigin::Agent,
            })
            .await
            .expect("send agent state report");
            tokio::time::timeout(std::time::Duration::from_secs(5), reports.recv())
                .await
                .expect("extension report delivered")
                .expect("event sink alive");
        }
        assert_eq!(
            observed.lock().clone(),
            vec!["working".to_string(), "idle".to_string()]
        );

        // The agent's own leftover frame arrives after its idle report. The barrier is the
        // detection pass itself: `agent_detect_pending` clears only once this screen has been
        // through a detection attempt, whether inline or on the pump's trailing edge.
        let mut updates = state.sessions.lock()[session_id].update_sender.subscribe();
        tx.send(DaemonStreamMessage::Output {
            session_id: session_id.into(),
            sequence: 2,
            data: b"  Working (esc to interrupt)\r\n".to_vec().into(),
            metrics_read_unix_micros: None,
        })
        .await
        .expect("send stale screen output");
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                updates.changed().await.expect("native pump alive");
                let sessions = state.sessions.lock();
                let session = &sessions[session_id];
                if session.last_sequence == Some(2) && !session.agent_detect_pending {
                    break;
                }
            }
        })
        .await
        .expect("stale screen went through a detection pass");

        assert_eq!(
            observed.lock().clone(),
            vec!["working".to_string(), "idle".to_string()],
            "a running agent still owns its state after reporting idle; its stale screen must not re-promote the pane"
        );

        state.teardown();
    }

    #[tokio::test]
    async fn process_release_hands_the_session_back_to_screen_inference() {
        // The opposite case: the daemon saw the agent process leave the PTY, so the extension no
        // longer owns this session. Inference must be armed again for whatever runs next, and it
        // must not replay the released activity from the agent's leftover screen.
        let state = NativeTerminalSurfaceHostState::default();
        let session_id = "process-released-session";
        let observed = Arc::new(Mutex::new(Vec::new()));
        let observed_for_sink = Arc::clone(&observed);
        let (reported, mut reports) = tokio::sync::mpsc::unbounded_channel();
        state.set_event_sink(Arc::new(move |event| {
            if let NativeTerminalEvent::AgentState(payload) = event {
                observed_for_sink.lock().push(payload.state);
                reported.send(()).expect("report receiver alive");
            }
        }));

        let (tx, messages) = tokio::sync::mpsc::channel(4);
        state
            .attach_daemon_attachment::<tauri::Wry>(
                session_id,
                DaemonAttachment {
                    session_id: session_id.to_string(),
                    epoch: 1,
                    start_sequence: Some(1),
                    end_sequence: Some(1),
                    gap: None,
                    history: bytes::Bytes::new(),
                    history_segments: Vec::new(),
                    pty_cols: None,
                    pty_rows: None,
                    remote_generation: None,
                    messages,
                    stream_task: tokio::spawn(std::future::pending()),
                },
                None,
            )
            .expect("attach");

        for (reported_state, origin) in [
            ("working", crate::daemon::protocol::AgentStateOrigin::Agent),
            (
                "idle",
                crate::daemon::protocol::AgentStateOrigin::ProcessReleased,
            ),
        ] {
            tx.send(DaemonStreamMessage::AgentState {
                session_id: session_id.into(),
                state: reported_state.into(),
                agent: Some("omo".into()),
                provider_session: None,
                detail: None,
                is_snapshot: false,
                origin,
            })
            .await
            .expect("send agent state report");
            tokio::time::timeout(std::time::Duration::from_secs(5), reports.recv())
                .await
                .expect("agent state delivered")
                .expect("event sink alive");
        }
        assert_eq!(
            observed.lock().clone(),
            vec!["working".to_string(), "idle".to_string()]
        );

        // THE stale frame that causes the reported bug: the exited agent's own spinner footer is
        // still the visible screen after the release. Inference is armed again here, so this is
        // exactly the content that would re-promote the pane to "working" with no agent alive.
        // The detection pass over this screen is the barrier, not elapsed time.
        let mut updates = state.sessions.lock()[session_id].update_sender.subscribe();
        tx.send(DaemonStreamMessage::Output {
            session_id: session_id.into(),
            sequence: 2,
            data: b"  Working (esc to interrupt)\r\n".to_vec().into(),
            metrics_read_unix_micros: None,
        })
        .await
        .expect("send stale agent screen");
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                updates.changed().await.expect("native pump alive");
                let sessions = state.sessions.lock();
                let session = &sessions[session_id];
                if session.last_sequence == Some(2) && !session.agent_detect_pending {
                    break;
                }
            }
        })
        .await
        .expect("stale agent screen went through a detection pass");
        assert_eq!(
            observed.lock().clone(),
            vec!["working".to_string(), "idle".to_string()],
            "the exited agent's leftover spinner must not resurrect the released activity"
        );

        // A new agent is started in the same pane. The daemon sees its process first and says so;
        // that sighting carries no activity, so it must not emit a state of its own.
        tx.send(DaemonStreamMessage::AgentState {
            session_id: session_id.into(),
            state: "idle".into(),
            agent: Some("omo".into()),
            provider_session: None,
            detail: None,
            is_snapshot: false,
            origin: crate::daemon::protocol::AgentStateOrigin::ProcessObserved,
        })
        .await
        .expect("send process sighting");

        // The new agent then paints its own working screen. Screen inference must still be armed
        // for it, which is what proves the release did not disable detection permanently.
        tx.send(DaemonStreamMessage::Output {
            session_id: session_id.into(),
            sequence: 3,
            data: b"\x1b[2J\x1b[HThinking through the next step...\r\nesc to interrupt\r\n"
                .to_vec()
                .into(),
            metrics_read_unix_micros: None,
        })
        .await
        .expect("send new agent screen");
        tokio::time::timeout(std::time::Duration::from_secs(5), reports.recv())
            .await
            .expect("screen inference re-armed after release")
            .expect("event sink alive");
        assert_eq!(
            observed.lock().clone(),
            vec![
                "working".to_string(),
                "idle".to_string(),
                "working".to_string()
            ],
            "a new agent in a released pane must be detected again"
        );

        state.teardown();
    }

    #[tokio::test]
    async fn rotated_provider_session_reaches_the_frontend_without_an_activity_change() {
        let state = NativeTerminalSurfaceHostState::default();
        let session_id = "conversation-rotation-session";
        let observed = Arc::new(Mutex::new(Vec::new()));
        let observed_for_sink = Arc::clone(&observed);
        let (reported, mut reports) = tokio::sync::mpsc::unbounded_channel();
        state.set_event_sink(Arc::new(move |event| {
            if let NativeTerminalEvent::AgentState(payload) = event {
                observed_for_sink
                    .lock()
                    .push(payload.provider_session.map(|session| session.id));
                reported.send(()).expect("report receiver alive");
            }
        }));

        let (tx, messages) = tokio::sync::mpsc::channel(4);
        let attachment = DaemonAttachment {
            session_id: session_id.to_string(),
            epoch: 1,
            start_sequence: Some(1),
            end_sequence: Some(1),
            gap: None,
            history: bytes::Bytes::new(),
            history_segments: Vec::new(),
            pty_cols: None,
            pty_rows: None,
            remote_generation: None,
            messages,
            stream_task: tokio::spawn(std::future::pending()),
        };
        state
            .attach_daemon_attachment::<tauri::Wry>(session_id, attachment, None)
            .expect("attach");

        let report = |conversation: &'static str| DaemonStreamMessage::AgentState {
            session_id: session_id.into(),
            state: "working".into(),
            agent: Some("omo".into()),
            detail: None,
            provider_session: Some(crate::daemon::protocol::AgentProviderSession {
                key: crate::daemon::protocol::AgentProviderSessionKey::SessionId,
                id: conversation.to_string(),
                transcript_path: None,
            }),
            is_snapshot: false,
            origin: crate::daemon::protocol::AgentStateOrigin::Agent,
        };

        // The agent stays "working" across `/new`, so an activity-only change test swallows the
        // new conversation and the pane keeps resuming the one it was opened with.
        for conversation in ["conversation-first", "conversation-after-new"] {
            tx.send(report(conversation))
                .await
                .expect("send agent state report");
            tokio::time::timeout(std::time::Duration::from_secs(5), reports.recv())
                .await
                .expect("agent report delivered")
                .expect("event sink alive");
        }

        assert_eq!(
            observed.lock().clone(),
            vec![
                Some("conversation-first".to_string()),
                Some("conversation-after-new".to_string()),
            ],
            "a rotated conversation id must reach the frontend even while the activity repeats"
        );

        // A repeat of the same conversation carries no news and must not be forwarded. The
        // following output frame is the barrier that proves the repeat was already processed.
        let mut updates = state.sessions.lock()[session_id].update_sender.subscribe();
        tx.send(report("conversation-after-new"))
            .await
            .expect("send repeated agent state report");
        tx.send(DaemonStreamMessage::Output {
            session_id: session_id.into(),
            sequence: 2,
            data: b"barrier\r\n".to_vec().into(),
            metrics_read_unix_micros: None,
        })
        .await
        .expect("send barrier output");
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                updates.changed().await.expect("native pump alive");
                if state.sessions.lock()[session_id].last_sequence == Some(2) {
                    break;
                }
            }
        })
        .await
        .expect("barrier output processed");

        assert_eq!(
            observed.lock().len(),
            2,
            "an unchanged report must stay coalesced"
        );

        state.teardown();
    }

    #[tokio::test]
    async fn native_terminal_surface_host_extracts_live_selection_snapshot_for_render_input() {
        let state = NativeTerminalSurfaceHostState::default();
        let session_id = "term-selection-snapshot-contract";

        let (_tx, rx) = tokio::sync::mpsc::channel(16);
        let stream_task = tokio::spawn(async {});

        let attachment = DaemonAttachment {
            session_id: session_id.to_string(),
            epoch: 1,
            start_sequence: Some(1),
            end_sequence: Some(1),
            gap: None,
            history: bytes::Bytes::from(b"orca selection rendering verification\r\n".to_vec()),
            history_segments: Vec::new(),
            pty_cols: None,
            pty_rows: None,
            remote_generation: None,
            messages: rx,
            stream_task,
        };

        state
            .attach_daemon_attachment::<tauri::Wry>(session_id, attachment, None)
            .expect("attach daemon session");

        // Before selection is made, production render input selection must be None
        let initial_input = state
            .render_snapshot_for_session(session_id)
            .expect("query initial render snapshot input")
            .expect("session exists");
        assert_eq!(initial_input.selection, None);
        assert_eq!(initial_input.snapshot.cols, 80);

        // Install an active word selection ("orca" -> cols 0..3 on row 0)
        state
            .with_session_terminal(session_id, |term| term.select_word_at(0, 0))
            .expect("select word at (0, 0)");

        let input = state
            .render_snapshot_for_session(session_id)
            .expect("query live render snapshot input")
            .expect("session exists");

        let live_selection = input
            .selection
            .expect("production render input must include live SelectionSnapshot instead of None");

        assert_eq!(
            live_selection,
            SelectionSnapshot {
                start_col: 0,
                start_row: 0,
                end_col: 3,
                end_row: 0,
            }
        );
        assert_eq!(input.snapshot.cols, 80);

        // Clear selection and verify render input returns to None
        state
            .with_session_terminal(session_id, |term| term.clear_selection())
            .expect("clear selection");

        let cleared_input = state
            .render_snapshot_for_session(session_id)
            .expect("query cleared render snapshot input")
            .expect("session exists");
        assert_eq!(cleared_input.selection, None);

        state.teardown();
    }

    #[tokio::test]
    async fn attach_replays_history_at_daemon_pty_size() {
        let state = NativeTerminalSurfaceHostState::default();
        let session_id = "daemon-pty-size-attach";
        let (_tx, messages) = tokio::sync::mpsc::channel(1);
        let attachment = DaemonAttachment {
            session_id: session_id.to_string(),
            epoch: 1,
            start_sequence: Some(1),
            end_sequence: Some(1),
            gap: None,
            history: bytes::Bytes::from(b"\x1b[100GX".to_vec()),
            history_segments: Vec::new(),
            pty_cols: Some(120),
            pty_rows: Some(30),
            remote_generation: None,
            messages,
            stream_task: tokio::spawn(std::future::pending()),
        };

        state
            .attach_daemon_attachment::<tauri::Wry>(session_id, attachment, None)
            .expect("attach daemon session with reported pty size");

        let sessions = state.sessions.lock();
        let sess = sessions.get(session_id).unwrap();
        assert_eq!(sess.terminal.dimensions().unwrap(), (120, 30));
        let snap = sess.terminal.render_snapshot().unwrap();
        assert_eq!(snap.grid[0][99].text, "X");
        drop(sessions);
        state.teardown();
    }

    #[tokio::test]
    async fn attach_replays_segmented_history_across_resizes() {
        let state = NativeTerminalSurfaceHostState::default();
        let session_id = "daemon-segmented-history-attach";
        let (_tx, messages) = tokio::sync::mpsc::channel(1);
        let attachment = DaemonAttachment {
            session_id: session_id.to_string(),
            epoch: 1,
            start_sequence: Some(1),
            end_sequence: Some(2),
            gap: None,
            history: bytes::Bytes::from(b"A\x1b[100GX".to_vec()),
            history_segments: vec![
                crate::terminal::output_hub::HistorySegment {
                    cols: Some(80),
                    rows: Some(24),
                    bytes: b"A".to_vec(),
                },
                crate::terminal::output_hub::HistorySegment {
                    cols: Some(120),
                    rows: Some(30),
                    bytes: b"\x1b[100GX".to_vec(),
                },
            ],
            pty_cols: None,
            pty_rows: None,
            remote_generation: None,
            messages,
            stream_task: tokio::spawn(std::future::pending()),
        };

        state
            .attach_daemon_attachment::<tauri::Wry>(session_id, attachment, None)
            .expect("attach daemon session with segmented history");

        let sessions = state.sessions.lock();
        let sess = sessions.get(session_id).unwrap();
        assert_eq!(sess.terminal.dimensions().unwrap(), (120, 30));
        let snap = sess.terminal.render_snapshot().unwrap();
        assert_eq!(snap.grid[0][0].text, "A");
        assert_eq!(snap.grid[0][99].text, "X");
        drop(sessions);
        state.teardown();
    }

    #[tokio::test]
    async fn test_set_attention_frame_updates_session_state() {
        let state = NativeTerminalSurfaceHostState::default();
        let session_id = "test-session-attention-frame";
        let (_tx, messages) = tokio::sync::mpsc::channel(1);
        let attachment = DaemonAttachment {
            session_id: session_id.to_string(),
            epoch: 1,
            start_sequence: Some(1),
            end_sequence: Some(2),
            gap: None,
            history: bytes::Bytes::from(b"hello".to_vec()),
            history_segments: Vec::new(),
            pty_cols: Some(80),
            pty_rows: Some(24),
            remote_generation: None,
            messages,
            stream_task: tokio::spawn(std::future::pending()),
        };
        state
            .attach_daemon_attachment::<tauri::Wry>(session_id, attachment, None)
            .expect("attach daemon session");

        // Initial attention frame should be false
        let render_input = state
            .render_snapshot_for_session(session_id)
            .unwrap()
            .unwrap();
        assert!(!render_input.attention_frame);

        // Enable attention frame
        state
            .set_attention_frame(session_id, true)
            .expect("set attention frame true");
        let render_input = state
            .render_snapshot_for_session(session_id)
            .unwrap()
            .unwrap();
        assert!(render_input.attention_frame);

        // Disable attention frame
        state
            .set_attention_frame(session_id, false)
            .expect("set attention frame false");
        let render_input = state
            .render_snapshot_for_session(session_id)
            .unwrap()
            .unwrap();
        assert!(!render_input.attention_frame);

        state.teardown();
    }

    #[tokio::test]
    async fn resize_preserves_scrollback_ratio_when_scrolled_up() {
        let state = NativeTerminalSurfaceHostState::default();
        let session_id = "test-scroll-ratio-session";
        let cell_metrics = font_manager::derived_cell_metrics();

        let request = |cols, rows| NativeTerminalBoundsRequest {
            session_id: session_id.to_string(),
            bounds: LogicalBounds {
                x: 0.0,
                y: 0.0,
                width: cols as f64 * cell_metrics.width_px as f64,
                height: rows as f64 * cell_metrics.height_px as f64,
                scale_factor: 1.0,
            },
        };

        state
            .prepare_session_layout(request(80, 24), cell_metrics)
            .expect("create initial grid");

        // Feed enough lines to create a substantial scrollback buffer
        let mut text = String::new();
        for i in 0..100 {
            text.push_str(&format!("Line number {}\r\n", i));
        }
        let (_tx, messages) = tokio::sync::mpsc::channel(1);
        let attachment = DaemonAttachment {
            session_id: session_id.to_string(),
            epoch: 1,
            start_sequence: Some(1),
            end_sequence: Some(1),
            gap: None,
            history: bytes::Bytes::from(text.into_bytes()),
            history_segments: Vec::new(),
            pty_cols: None,
            pty_rows: None,
            remote_generation: None,
            messages,
            stream_task: tokio::spawn(std::future::pending()),
        };
        state
            .attach_daemon_attachment::<tauri::Wry>(session_id, attachment, None)
            .expect("attach daemon session");

        // Now scroll up to half of scrollback
        let initial_sb = {
            let sessions = state.sessions.lock();
            let session = sessions.get(session_id).unwrap();
            session.terminal.scrollbar().unwrap()
        };
        let initial_max = initial_sb.total.saturating_sub(initial_sb.len);
        assert!(initial_max > 0, "must have scrollback lines");

        let half_offset = (initial_max / 2) as usize;
        {
            let mut sessions = state.sessions.lock();
            let session = sessions.get_mut(session_id).unwrap();
            session
                .terminal
                .scroll_viewport(crate::native_terminal::ScrollViewport::Row(half_offset))
                .unwrap();
        }

        // Resize the terminal height (e.g. 24 -> 30)
        state
            .prepare_session_layout(request(80, 30), cell_metrics)
            .expect("resize grid");

        // Check that after resize, we are still scrolled up near half rather than reset to 0 or max_offset
        let new_sb = {
            let sessions = state.sessions.lock();
            let session = sessions.get(session_id).unwrap();
            session.terminal.scrollbar().unwrap()
        };
        let new_max = new_sb.total.saturating_sub(new_sb.len);
        assert!(new_max > 0);
        // Ensure new_sb.offset is not reset to max_offset (which represents bottom)
        assert!(
            new_sb.offset < new_max,
            "viewport must remain scrolled up, not snapped to bottom"
        );
        let ratio = new_sb.offset as f64 / new_max as f64;
        assert!(
            (ratio - 0.5).abs() < 0.1,
            "viewport scroll ratio should be preserved near 0.5, got {}",
            ratio
        );

        state.teardown();
    }

    #[tokio::test]
    async fn resize_keeps_viewport_at_bottom_when_at_bottom() {
        let state = NativeTerminalSurfaceHostState::default();
        let session_id = "test-bottom-lock-session";
        let cell_metrics = font_manager::derived_cell_metrics();

        let request = |cols, rows| NativeTerminalBoundsRequest {
            session_id: session_id.to_string(),
            bounds: LogicalBounds {
                x: 0.0,
                y: 0.0,
                width: cols as f64 * cell_metrics.width_px as f64,
                height: rows as f64 * cell_metrics.height_px as f64,
                scale_factor: 1.0,
            },
        };

        state
            .prepare_session_layout(request(80, 24), cell_metrics)
            .expect("create initial grid");

        // Feed enough lines to create a substantial scrollback buffer
        let mut text = String::new();
        for i in 0..100 {
            text.push_str(&format!("Line number {}\r\n", i));
        }
        let (_tx, messages) = tokio::sync::mpsc::channel(1);
        let attachment = DaemonAttachment {
            session_id: session_id.to_string(),
            epoch: 1,
            start_sequence: Some(1),
            end_sequence: Some(1),
            gap: None,
            history: bytes::Bytes::from(text.into_bytes()),
            history_segments: Vec::new(),
            pty_cols: None,
            pty_rows: None,
            remote_generation: None,
            messages,
            stream_task: tokio::spawn(std::future::pending()),
        };
        state
            .attach_daemon_attachment::<tauri::Wry>(session_id, attachment, None)
            .expect("attach daemon session");

        // Resize columns and rows (e.g. 80x24 -> 120x30, simulating closing a pane or widening window)
        state
            .prepare_session_layout(request(120, 30), cell_metrics)
            .expect("resize grid");

        let assert_bottom_locked = |label: &str| {
            let sb = {
                let sessions = state.sessions.lock();
                let session = sessions.get(session_id).unwrap();
                session.terminal.scrollbar().unwrap()
            };
            let max_offset = sb.total.saturating_sub(sb.len);
            assert!(max_offset > 0, "{label}: scrollback must exist");
            assert_eq!(
                sb.offset, max_offset,
                "{label}: viewport must remain locked at bottom (offset == max_offset)"
            );
        };
        assert_bottom_locked("combined cols+rows grow");

        // Width-only grow (horizontal pane close in a split)
        state
            .prepare_session_layout(request(160, 30), cell_metrics)
            .expect("width-only resize");
        assert_bottom_locked("width-only grow");

        // Height shrink pushes rows into scrollback (opposite max_offset direction)
        state
            .prepare_session_layout(request(160, 12), cell_metrics)
            .expect("height shrink");
        assert_bottom_locked("height shrink");

        state.teardown();
    }

    #[tokio::test]
    async fn attached_throttled_burst_still_emits_trailing_state_transition() {
        let state = NativeTerminalSurfaceHostState::default();
        let session_id = "throttled-attached-session";
        let observed_states = Arc::new(Mutex::new(Vec::new()));
        let observed_for_sink = Arc::clone(&observed_states);
        state.set_event_sink(Arc::new(move |event| {
            if let NativeTerminalEvent::AgentState(payload) = event {
                observed_for_sink.lock().push(payload.state);
            }
        }));

        let (tx, messages) = tokio::sync::mpsc::channel(4);
        let attachment = DaemonAttachment {
            session_id: session_id.to_string(),
            epoch: 1,
            start_sequence: Some(1),
            end_sequence: Some(1),
            gap: None,
            history: bytes::Bytes::from(b"Working (esc to interrupt)\r\n".to_vec()),
            history_segments: Vec::new(),
            pty_cols: None,
            pty_rows: None,
            remote_generation: None,
            messages,
            stream_task: tokio::spawn(std::future::pending()),
        };
        state
            .attach_daemon_attachment::<tauri::Wry>(session_id, attachment, None)
            .expect("attach daemon history");
        assert_eq!(observed_states.lock().clone(), vec!["working".to_string()]);

        // Burst: two chunks inside the throttle window; the second carries the blocked frame.
        tx.send(DaemonStreamMessage::Output {
            session_id: session_id.into(),
            sequence: 2,
            data: b"more output\r\n".to_vec().into(),
            metrics_read_unix_micros: None,
        })
        .await
        .expect("send burst chunk 1");
        tx.send(DaemonStreamMessage::Output {
            session_id: session_id.into(),
            sequence: 3,
            data: b"\x1b[2J\x1b[HAction Required: allow command?\r\npress enter to confirm or esc to cancel\r\n"
                .to_vec()
                .into(),
            metrics_read_unix_micros: None,
        })
        .await
        .expect("send burst chunk 2");

        // The throttle skips both chunks; the pump's trailing-edge detect must still emit
        // the blocked transition once the burst drains.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while observed_states.lock().len() < 2 && std::time::Instant::now() < deadline {
            tokio::task::yield_now().await;
        }

        assert_eq!(
            observed_states.lock().clone(),
            vec!["working".to_string(), "blocked".to_string()],
            "an attached pane's final burst frame must still produce a state transition"
        );

        state.teardown();
    }

    #[tokio::test]
    async fn lagged_recovery_restores_grid_to_pane_layout_dimensions() {
        let state = NativeTerminalSurfaceHostState::default();
        let session_id = "lagged-dims-restore-session";
        let cell_metrics = font_manager::derived_cell_metrics();

        // Create the pane layout first so the session exists with layout (120, 30).
        state
            .prepare_session_layout(
                NativeTerminalBoundsRequest {
                    session_id: session_id.to_string(),
                    bounds: LogicalBounds {
                        x: 0.0,
                        y: 0.0,
                        width: 120.0 * cell_metrics.width_px as f64,
                        height: 30.0 * cell_metrics.height_px as f64,
                        scale_factor: 1.0,
                    },
                },
                cell_metrics,
            )
            .expect("create pane layout");

        let (tx, messages) = tokio::sync::mpsc::channel(4);
        let attachment = DaemonAttachment {
            session_id: session_id.to_string(),
            epoch: 1,
            start_sequence: Some(1),
            end_sequence: Some(1),
            gap: None,
            history: bytes::Bytes::from(b"seed\r\n".to_vec()),
            history_segments: Vec::new(),
            pty_cols: None,
            pty_rows: None,
            remote_generation: None,
            messages,
            stream_task: tokio::spawn(std::future::pending()),
        };
        state
            .attach_daemon_attachment::<tauri::Wry>(session_id, attachment, None)
            .expect("attach daemon session");

        let mut updates = state.subscribe_session_update(session_id).unwrap();
        let coordinator = state.session_render_coordinator(session_id).unwrap();
        assert!(coordinator.schedule_render());
        assert!(coordinator.begin_render());

        // Lagged recovery whose last history segment was recorded at a much narrower size;
        // feed_attachment_history resizes the grid through each segment and leaves it at the
        // last one (40x10). The handler must restore the pane layout size (120x30) before
        // locking the viewport to the bottom, otherwise the bottom lock applies to the
        // wrong grid.
        tx.send(DaemonStreamMessage::Lagged {
            session_id: session_id.into(),
            requested_after_sequence: 1,
            available_from_sequence: 2,
            start_sequence: Some(2),
            end_sequence: Some(3),
            history: bytes::Bytes::from_static(b"replayed at narrow width\r\n"),
            segments: vec![crate::daemon::protocol::HistorySegmentWire {
                cols: Some(40),
                rows: Some(10),
                bytes: bytes::Bytes::from(b"replayed at narrow width\r\n".to_vec()),
            }],
            replay_is_delta: None,
        })
        .await
        .expect("send lagged recovery");

        tokio::time::timeout(std::time::Duration::from_secs(5), updates.changed())
            .await
            .expect("recovery delivered")
            .expect("session alive");
        let snapshot = state.snapshot_for_session(session_id).unwrap().unwrap();
        assert_eq!((snapshot.cols, snapshot.rows), (120, 30));
        assert!(
            coordinator.finish_render(),
            "recovery must request a follow-up frame"
        );

        assert!(coordinator.begin_render());
        tx.send(DaemonStreamMessage::Gap {
            session_id: session_id.into(),
            requested_after_sequence: 3,
            available_from_sequence: 4,
        })
        .await
        .expect("send gap");
        tokio::time::timeout(std::time::Duration::from_secs(5), updates.changed())
            .await
            .expect("gap delivered")
            .expect("session alive");
        assert!(
            coordinator.finish_render(),
            "gap must request a follow-up frame"
        );

        state.teardown();
    }

    #[tokio::test]
    async fn attach_daemon_attachment_with_bounds_tolerates_invalid_initial_bounds() {
        let state = NativeTerminalSurfaceHostState::default();
        let session_id = "test-invalid-bounds-attach";
        let (_tx, messages) = tokio::sync::mpsc::channel(1);
        let attachment = DaemonAttachment {
            session_id: session_id.to_string(),
            epoch: 1,
            start_sequence: Some(1),
            end_sequence: Some(1),
            gap: None,
            history: bytes::Bytes::from(b"hello\r\n".to_vec()),
            history_segments: Vec::new(),
            pty_cols: Some(80),
            pty_rows: Some(24),
            remote_generation: None,
            messages,
            stream_task: tokio::spawn(std::future::pending()),
        };

        let invalid_bounds = LogicalBounds {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
            scale_factor: 1.0,
        };

        let result = state.attach_daemon_attachment_with_bounds::<tauri::Wry>(
            session_id,
            attachment,
            None,
            Some(invalid_bounds),
        );
        assert!(
            result.is_ok(),
            "attach must succeed with fallback dimensions even if initial bounds are invalid"
        );

        let dims = {
            let sessions = state.sessions.lock();
            let session = sessions.get(session_id).expect("session exists");
            session.terminal.dimensions().expect("terminal dimensions")
        };
        assert_eq!(
            dims,
            (80, 24),
            "terminal should fall back to daemon PTY dimensions"
        );

        state.teardown();
    }

    #[tokio::test]
    async fn reattach_existing_session_with_bounds_tolerates_invalid_bounds() {
        let state = NativeTerminalSurfaceHostState::default();
        let session_id = "test-invalid-bounds-reattach";
        let (_tx, messages) = tokio::sync::mpsc::channel(1);
        let attachment = DaemonAttachment {
            session_id: session_id.to_string(),
            epoch: 1,
            start_sequence: Some(1),
            end_sequence: Some(1),
            gap: None,
            history: bytes::Bytes::from(b"hello\r\n".to_vec()),
            history_segments: Vec::new(),
            pty_cols: Some(80),
            pty_rows: Some(24),
            remote_generation: None,
            messages,
            stream_task: tokio::spawn(std::future::pending()),
        };

        state
            .attach_daemon_attachment::<tauri::Wry>(session_id, attachment, None)
            .expect("initial attach");

        // Reattach with invalid bounds (0 height/width)
        let invalid_bounds = LogicalBounds {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
            scale_factor: 1.0,
        };

        let result = state.reattach_existing_session_with_bounds(session_id, Some(invalid_bounds));
        assert!(
            result.is_ok_and(|rebound| rebound),
            "reattach must succeed and return true even with invalid bounds"
        );

        let dims = {
            let sessions = state.sessions.lock();
            let session = sessions.get(session_id).expect("session exists");
            session.terminal.dimensions().expect("terminal dimensions")
        };
        assert_eq!(dims, (80, 24), "terminal should keep existing dimensions");

        state.teardown();
    }

    #[tokio::test]
    async fn test_fresh_startup_with_bounds_preserves_startup_pty_writes() {
        let state = NativeTerminalSurfaceHostState::default();
        let session_id = "test-fresh-startup-bounds";
        state.mark_pending_startup(session_id);

        let (_tx, messages) = tokio::sync::mpsc::channel(1);
        let attachment = DaemonAttachment {
            session_id: session_id.to_string(),
            epoch: 1,
            start_sequence: Some(1),
            end_sequence: Some(1),
            gap: None,
            // Shell emits CPR query during startup
            history: bytes::Bytes::from(b"\x1b[6n".to_vec()),
            history_segments: Vec::new(),
            pty_cols: Some(80),
            pty_rows: Some(24),
            remote_generation: Some(1),
            messages,
            stream_task: tokio::spawn(std::future::pending()),
        };

        let initial_bounds = LogicalBounds {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 600.0,
            scale_factor: 1.0,
        };

        // Attach with bounds: prepare_session_layout runs first and pre-creates session
        state
            .attach_daemon_attachment_with_bounds::<tauri::Wry>(
                session_id,
                attachment,
                None,
                Some(initial_bounds),
            )
            .expect("attach with bounds");

        let buffered_writes = {
            let sessions = state.sessions.lock();
            let session = sessions.get(session_id).expect("session exists");
            session.terminal.buffered_pty_writes()
        };

        // H1 regression check: Startup CPR must be retained in buffer with its remote generation, NOT discarded
        assert!(
            !buffered_writes.is_empty(),
            "fresh startup with bounds must retain startup CPR response"
        );
        assert_eq!(buffered_writes[0].generation, Some(1));
        assert!(buffered_writes[0].data.ends_with(b"R"));

        state.teardown();
    }

    #[tokio::test]
    async fn test_reconstruction_without_pending_startup_discards_buffered_queries() {
        let state = NativeTerminalSurfaceHostState::default();
        let session_id = "test-reconstruction-discard";
        // Do NOT mark pending startup: this is a display reconstruction of an existing session

        let (_tx, messages) = tokio::sync::mpsc::channel(1);
        let attachment = DaemonAttachment {
            session_id: session_id.to_string(),
            epoch: 1,
            start_sequence: Some(1),
            end_sequence: Some(1),
            gap: None,
            history: bytes::Bytes::from(b"\x1b[6n".to_vec()),
            history_segments: Vec::new(),
            pty_cols: Some(80),
            pty_rows: Some(24),
            remote_generation: Some(1),
            messages,
            stream_task: tokio::spawn(std::future::pending()),
        };

        state
            .attach_daemon_attachment::<tauri::Wry>(session_id, attachment, None)
            .expect("attach reconstruction");

        let buffered_writes = {
            let sessions = state.sessions.lock();
            let session = sessions.get(session_id).expect("session exists");
            session.terminal.buffered_pty_writes()
        };

        // R7/H1 regression check: Display reconstruction must discard replayed queries
        assert!(
            buffered_writes.is_empty(),
            "display reconstruction must discard replayed query responses"
        );

        state.teardown();
    }

    #[tokio::test]
    async fn receipt_path_builds_zero_snapshots_while_render_path_builds_snapshots() {
        let harness = DirectRenderHarness::new(vec![SimulatedAcquisition::Frame]);
        RENDER_SNAPSHOT_CALL_COUNT.store(0, Ordering::SeqCst);

        let receipt = harness
            .state
            .get_receipt(&harness.window, &harness.request.session_id)
            .expect("receipt query");
        assert_eq!(
            RENDER_SNAPSHOT_CALL_COUNT.load(Ordering::SeqCst),
            0,
            "receipt path must build zero render snapshots"
        );

        let render_receipt = harness
            .state
            .render(&harness.window, harness.request.clone())
            .expect("render frame");
        assert!(
            RENDER_SNAPSHOT_CALL_COUNT.load(Ordering::SeqCst) > 0,
            "render path must build snapshots"
        );
    }

    #[tokio::test]
    async fn send_input_does_not_wait_for_a_blocked_main_thread() {
        let harness = DirectRenderHarness::new(vec![]);
        // Blocked main thread: queue the dispatch (never drained by this test) without
        // requiring an off-thread origin, so the receipt rendezvous can never complete.
        harness
            .window
            .state::<RenderDispatch>()
            .require_deferred
            .store(false, Ordering::SeqCst);
        let session_id = &harness.request.session_id;
        let input = NativeTerminalInput::Text {
            text: "x".to_string(),
        };
        let start = std::time::Instant::now();
        let result = crate::ipc::native_terminal::send_native_terminal_input_with_writer(
            harness._app.handle(),
            &harness.state,
            session_id,
            &input,
            |_bytes| async { Ok(()) },
        )
        .await;
        let elapsed = start.elapsed();

        // The keystroke is complete once the write lands. The receipt only positions the IME
        // candidate window and is delivered out of band, so a busy main thread must not hold
        // the keystroke: that wait is what queued typing behind the rendezvous.
        result.expect("a written keystroke must succeed while the main thread is blocked");
        assert!(
            elapsed < crate::ipc::native_terminal::NATIVE_TERMINAL_RECEIPT_TIMEOUT,
            "send_input waited for the blocked main thread: {elapsed:?}"
        );
    }

    #[tokio::test]
    async fn gap_message_preserves_grid_scrollback_and_injects_notice() {
        let state = NativeTerminalSurfaceHostState::default();
        let session_id = "test-gap-preserves-content";
        let (tx, messages) = tokio::sync::mpsc::channel(10);
        let attachment = DaemonAttachment {
            session_id: session_id.to_string(),
            epoch: 1,
            start_sequence: Some(1),
            end_sequence: Some(1),
            gap: None,
            history: bytes::Bytes::from(
                (0..30)
                    .map(|i| format!("scrollback line {i}\r\n"))
                    .collect::<String>()
                    .into_bytes(),
            ),
            history_segments: Vec::new(),
            pty_cols: Some(80),
            pty_rows: Some(24),
            remote_generation: None,
            messages,
            stream_task: tokio::spawn(std::future::pending()),
        };

        state
            .attach_daemon_attachment::<tauri::Wry>(session_id, attachment, None)
            .expect("attach session");

        // Verify initial content and scrollback are present
        {
            let sessions = state.sessions.lock();
            let session = sessions.get(session_id).expect("session exists");
            assert!(
                session.terminal.scrollback_rows().expect("scrollback rows") > 0,
                "terminal should have scrollback rows from initial 30 lines"
            );
            let matches = session
                .terminal
                .search_grid("scrollback line 0", false)
                .expect("search grid");
            assert!(
                !matches.is_empty(),
                "scrollback line 0 should be present before gap"
            );
        }

        // Deliver a gap message through the stream pump
        tx.send(DaemonStreamMessage::Gap {
            session_id: session_id.into(),
            requested_after_sequence: 1,
            available_from_sequence: 100,
        })
        .await
        .expect("send gap message");

        // Await processing of the gap message by waiting for last_sequence update
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        let mut processed = false;
        while std::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            let sessions = state.sessions.lock();
            if let Some(session) = sessions.get(session_id) {
                if session.last_sequence.is_some_and(|seq| seq >= 99) {
                    processed = true;
                    break;
                }
            }
        }
        assert!(processed, "pump task should have processed the gap message");

        // Assert earlier content and scrollback are STILL present!
        {
            let sessions = state.sessions.lock();
            let session = sessions.get(session_id).expect("session exists");
            assert!(
                session.terminal.scrollback_rows().expect("scrollback rows") > 0,
                "scrollback rows must survive the gap message"
            );
            let matches = session
                .terminal
                .search_grid("scrollback line 0", false)
                .expect("search grid for earlier content");
            assert!(
                !matches.is_empty(),
                "earlier content in scrollback must survive the gap message"
            );

            // Assert the in-band notice line was injected
            let notice_matches = session
                .terminal
                .search_grid("output stream gap", false)
                .expect("search grid for gap notice");
            assert!(
                !notice_matches.is_empty(),
                "in-band gap notice line must be present in the terminal"
            );
        }

        state.teardown();
    }

    #[tokio::test]
    async fn lagged_rebuild_preserves_scrollback_and_marks_the_resync() {
        let state = NativeTerminalSurfaceHostState::default();
        let session_id = "test-lagged-rebuild-preserves-content";
        let (tx, messages) = tokio::sync::mpsc::channel(10);
        let attachment = DaemonAttachment {
            session_id: session_id.to_string(),
            epoch: 1,
            start_sequence: Some(1),
            end_sequence: Some(1),
            gap: None,
            history: bytes::Bytes::from(
                (0..30)
                    .map(|i| format!("scrollback line {i}\r\n"))
                    .collect::<String>()
                    .into_bytes(),
            ),
            history_segments: Vec::new(),
            pty_cols: Some(80),
            pty_rows: Some(24),
            remote_generation: None,
            messages,
            stream_task: tokio::spawn(std::future::pending()),
        };

        state
            .attach_daemon_attachment::<tauri::Wry>(session_id, attachment, None)
            .expect("attach session");

        // Verify initial content and scrollback are present
        {
            let sessions = state.sessions.lock();
            let session = sessions.get(session_id).expect("session exists");
            assert!(
                session.terminal.scrollback_rows().expect("scrollback rows") > 0,
                "terminal should have scrollback rows from initial 30 lines"
            );
            let matches = session
                .terminal
                .search_grid("scrollback line 0", false)
                .expect("search grid");
            assert!(
                !matches.is_empty(),
                "scrollback line 0 should be present before lagged recovery"
            );
        }

        // Scroll the viewport up so the user is NOT at the bottom
        let offset_before = {
            let mut sessions = state.sessions.lock();
            let session = sessions.get_mut(session_id).expect("session exists");
            session
                .terminal
                .scroll_viewport(crate::native_terminal::ScrollViewport::Top)
                .expect("scroll to top");
            let sb = session.terminal.scrollbar().expect("scrollbar");
            let max_offset = sb.total.saturating_sub(sb.len);
            assert!(max_offset > 0, "test setup must have scrollable range");
            assert_ne!(
                sb.offset, max_offset,
                "viewport must not be at bottom before lagged recovery"
            );
            sb.offset
        };

        // Deliver a Lagged recovery that classify_replay must rebuild (replay_is_delta: None)
        tx.send(DaemonStreamMessage::Lagged {
            session_id: session_id.into(),
            requested_after_sequence: 1,
            available_from_sequence: 50,
            start_sequence: Some(50),
            end_sequence: Some(60),
            history: bytes::Bytes::from_static(b"replayed after overflow\r\n"),
            segments: Vec::new(),
            replay_is_delta: None,
        })
        .await
        .expect("send lagged recovery");

        // Await processing of the lagged recovery by waiting for last_sequence update
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        let mut processed = false;
        while std::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            let sessions = state.sessions.lock();
            if let Some(session) = sessions.get(session_id) {
                if session.last_sequence.is_some_and(|seq| seq >= 60) {
                    processed = true;
                    break;
                }
            }
        }
        assert!(processed, "pump task should have processed the lagged message");

        // Assert scrollback survived, resync notice landed, replay content landed, and viewport did not jump
        {
            let sessions = state.sessions.lock();
            let session = sessions.get(session_id).expect("session exists");
            assert!(
                session.terminal.scrollback_rows().expect("scrollback rows") > 0,
                "scrollback rows must survive the lagged rebuild"
            );
            let earlier_matches = session
                .terminal
                .search_grid("scrollback line 0", false)
                .expect("search grid for earlier content");
            assert!(
                !earlier_matches.is_empty(),
                "earlier content in scrollback must survive the lagged rebuild"
            );

            let notice_matches = session
                .terminal
                .search_grid("output stream resynced", false)
                .expect("search grid for resync notice");
            assert!(
                !notice_matches.is_empty(),
                "in-band resync notice line must be present in the terminal"
            );

            let replay_matches = session
                .terminal
                .search_grid("replayed after overflow", false)
                .expect("search grid for replay content");
            assert!(
                !replay_matches.is_empty(),
                "replayed content must be present in the terminal"
            );

            let offset_after = session
                .terminal
                .scrollbar()
                .expect("scrollbar")
                .offset;
            assert_eq!(
                offset_after, offset_before,
                "viewport scroll offset must not jump when user is scrolled up"
            );
        }

        state.teardown();
    }
}

#[cfg(test)]
mod liveness_tests {
    use super::*;
    use crate::ipc::debug::{classify_pane_liveness, PaneLivenessVerdict};

    // Task 3 (local-split-qa): the private barrier channel drives the REAL
    // presentation coordinator producer without any window.
    #[cfg(feature = "local-split-qa")]
    mod qa_barrier_presentation_tests {
        use super::*;
        use crate::ipc::qa_barrier::{
            active_channel, deactivate, install, QaBarrierChannel, PRODUCER_ID,
            PRESENTATION_BARRIER,
        };
        use serde_json::Value;

        const RUN_ID: &str = "qa-run-presentation";
        const OPERATION_ID: &str = "qa-op-presentation";

        /// The private channel is installed PROCESS-WIDE (`install`/
        /// `active_channel`), so the tests that drive it run one at a time: a
        /// parallel install would replace the channel another test's producer is
        /// writing to, and the receipt would land in the wrong directory.
        static GLOBAL_CHANNEL_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

        struct ChannelGuard;
        impl Drop for ChannelGuard {
            fn drop(&mut self) {
                deactivate();
            }
        }

        #[tokio::test(start_paused = true)]
        async fn real_coordinator_frame_holds_and_recovers_through_private_barrier() {
            let root = tempfile::tempdir().unwrap();
            let dir = root.path().join("barriers");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join(format!("{PRESENTATION_BARRIER}.arm.json")),
                serde_json::to_string(&serde_json::json!({
                    "name": PRESENTATION_BARRIER,
                    "runId": RUN_ID,
                    "operationId": OPERATION_ID,
                    "deadlineMs": 5_000,
                }))
                .unwrap(),
            )
            .unwrap();
            let _serial = GLOBAL_CHANNEL_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            install(QaBarrierChannel::new(dir.clone(), RUN_ID.to_string()));
            let channel = active_channel().expect("channel installed");
            let _guard = ChannelGuard;
            let (acked, rejected) = channel.scan_and_ack_arms();
            assert_eq!(acked, vec![PRESENTATION_BARRIER.to_string()]);
            assert!(rejected.is_empty());

            // Prearm exact event subscribers BEFORE driving the presentation hold
            let mut held_rx = channel.subscribe_held();
            let mut receipt_rx = channel.subscribe_receipts();

            let state = NativeTerminalSurfaceHostState::default();
            let session_id = "session-qa-presentation";
            let coordinator = state
                .attach_test_session_for_liveness(session_id, 1, Some(42))
                .expect("attach test session");

            let hold_fut =
                state.hold_presentation_barrier_qa(&coordinator, session_id, &channel);
            tokio::pin!(hold_fut);

            // Await prearmed exact held event
            let held_event = tokio::select! {
                event = QaBarrierChannel::await_held_event(&mut held_rx, PRESENTATION_BARRIER) => event,
                _ = &mut hold_fut => panic!("presentation hold settled before held event"),
            };
            let held = held_event.payload;
            assert_eq!(held["runId"], serde_json::json!(RUN_ID));
            assert_eq!(held["operationId"], serde_json::json!(OPERATION_ID));
            assert_eq!(held["producer"], serde_json::json!(PRODUCER_ID));
            assert!(held["producerPid"].as_u64().is_some());
            assert_eq!(held["sessionId"], serde_json::json!(session_id));
            assert_eq!(
                held["classifierVerdict"],
                serde_json::json!("BlockedInPresentation")
            );
            assert_eq!(
                held["presentationEvidence"],
                serde_json::json!("coordinator-pending")
            );
            assert_eq!(
                held["coordinatorEvidence"],
                serde_json::json!("coordinator-pending")
            );
            assert!(dir.join(format!("{PRESENTATION_BARRIER}.held.json")).exists());

            // Presentation emits its first receipt only after settlement.
            assert!(coordinator.is_render_pending(), "held frame must remain pending");

            std::fs::write(
                dir.join(format!("{PRESENTATION_BARRIER}.release.json")),
                serde_json::to_string(&serde_json::json!({
                    "name": PRESENTATION_BARRIER,
                    "runId": RUN_ID,
                    "operationId": OPERATION_ID,
                    "releasedAt": "2026-10-03T00:00:01.000Z",
                }))
                .unwrap(),
            )
            .unwrap();
            let (outcome, settled) = tokio::join!(
                &mut hold_fut,
                QaBarrierChannel::await_receipt_event(&mut receipt_rx, PRESENTATION_BARRIER, 1)
            );
            assert_eq!(
                outcome.map(crate::ipc::qa_barrier::ReleaseOutcome::as_str),
                Some("released")
            );
            assert!(
                !coordinator.is_render_pending(),
                "the SAME owned frame must be consumed after release"
            );

            assert_eq!(settled["runId"], serde_json::json!(RUN_ID));
            assert_eq!(settled["operationId"], serde_json::json!(OPERATION_ID));
            assert_eq!(
                settled["stageProgress"]["frameConsumed"],
                serde_json::json!(true)
            );
            assert_eq!(
                settled["stageProgress"]["renderPendingAfter"],
                serde_json::json!(false)
            );
            let verdict = settled["classifierVerdict"].as_str().unwrap();
            match verdict {
                "Idle" => assert_eq!(settled["evidenceMissing"], serde_json::json!(false)),
                "Unknown" => assert_eq!(settled["evidenceMissing"], serde_json::json!(true)),
                other => panic!("unexpected recovery verdict {other}"),
            }
            assert_eq!(
                settled["presentationEvidence"],
                serde_json::json!("coordinator-consumed")
            );
            assert_eq!(
                settled["coordinatorEvidence"],
                serde_json::json!("coordinator-consumed")
            );

            state.teardown();
        }

        #[tokio::test(start_paused = true)]
        async fn presentation_hold_deadline_cancels_pending_frame_bounded() {
            let root = tempfile::tempdir().unwrap();
            let dir = root.path().join("barriers");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join(format!("{PRESENTATION_BARRIER}.arm.json")),
                serde_json::to_string(&serde_json::json!({
                    "name": PRESENTATION_BARRIER,
                    "runId": RUN_ID,
                    "operationId": OPERATION_ID,
                    "deadlineMs": 1_000,
                }))
                .unwrap(),
            )
            .unwrap();
            let _serial = GLOBAL_CHANNEL_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            install(QaBarrierChannel::new(dir.clone(), RUN_ID.to_string()));
            let channel = active_channel().expect("channel installed");
            let _guard = ChannelGuard;
            channel.scan_and_ack_arms();

            let state = NativeTerminalSurfaceHostState::default();
            let session_id = "session-qa-presentation-cancel";
            let coordinator = state
                .attach_test_session_for_liveness(session_id, 1, Some(42))
                .expect("attach test session");

            let started = tokio::time::Instant::now();
            let outcome = state
                .hold_presentation_barrier_qa(&coordinator, session_id, &channel)
                .await;
            assert_eq!(
                outcome.map(crate::ipc::qa_barrier::ReleaseOutcome::as_str),
                Some("deadline-exceeded")
            );
            assert!(
                started.elapsed() >= std::time::Duration::from_millis(1_000),
                "deadline must bound the hold"
            );
            assert!(
                !coordinator.is_render_pending(),
                "cancellation must cancel the pending frame, not park the coordinator"
            );
            state.teardown();
        }

        // The `trigger-stale-binding` consumer: the runner's command drives a REAL
        // stale attempt against the live binding, the product's own registration
        // fence rejects it, and a legitimate reattach of the same backend then
        // settles. Both receipts come from the production fences.
        #[tokio::test]
        async fn stale_binding_command_drives_the_real_fence_and_the_reattach() {
            let _serial = GLOBAL_CHANNEL_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let root = tempfile::tempdir().unwrap();
            let dir = root.path().join("barriers");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join(format!("{STALE_RECEIPT_REJECTED_BARRIER}.arm.json")),
                serde_json::to_string(&serde_json::json!({
                    "name": STALE_RECEIPT_REJECTED_BARRIER,
                    "runId": RUN_ID,
                    "operationId": OPERATION_ID,
                    "deadlineMs": 5_000,
                }))
                .unwrap(),
            )
            .unwrap();
            install(QaBarrierChannel::new(dir.clone(), RUN_ID.to_string()));
            let channel = active_channel().expect("channel installed");
            let _guard = ChannelGuard;
            let (acked, rejected) = channel.scan_and_ack_arms();
            assert_eq!(acked, vec![STALE_RECEIPT_REJECTED_BARRIER.to_string()]);
            assert!(rejected.is_empty());
            // Subscribers exist BEFORE the trigger: the waits below are event-driven.
            let mut stale_rx = channel.subscribe_receipts();
            let mut reattach_rx = channel.subscribe_receipts();

            // A session that really carries a live seven-field binding plus live
            // stream/pump tasks, i.e. exactly what the registration fence protects.
            let state = NativeTerminalSurfaceHostState::default();
            let active: crate::daemon::protocol::PaneAttachTuple =
                serde_json::from_value(serde_json::json!({
                    "backendSessionId": "backend-stale-1",
                    "incarnation": "incarnation-stale-1",
                    "daemonEpoch": "7",
                    "frontendSessionId": "frontend-1",
                    "paneIdentity": "pane-1",
                    "bindingKey": "binding-1",
                    "attemptGeneration": 3,
                }))
                .expect("valid seven-field tuple");
            state
                .attach_test_session_for_liveness(&active.backend_session_id, 7, Some(1))
                .expect("attach test session");
            let token = state
                .begin_replay_request_with_tuple(&active.backend_session_id, Some(active.clone()))
                .expect("live binding token");
            let (output, messages) = tokio::sync::mpsc::channel(1);
            let _keep_output = output;
            let attachment = crate::daemon::DaemonAttachment {
                session_id: active.backend_session_id.clone(),
                epoch: 7,
                start_sequence: Some(1),
                end_sequence: Some(1),
                gap: None,
                history: bytes::Bytes::new(),
                history_segments: Vec::new(),
                pty_cols: Some(80),
                pty_rows: Some(24),
                remote_generation: None,
                messages,
                stream_task: tokio::spawn(std::future::pending()),
            };
            state
                .attach_daemon_attachment_with_client_and_token::<tauri::test::MockRuntime>(
                    &active.backend_session_id,
                    attachment,
                    None,
                    None,
                    Some(token),
                )
                .expect("attach live stream");
            assert_eq!(
                state.session_attach_tuple(&active.backend_session_id),
                Some(active.clone())
            );

            std::fs::write(
                dir.join(format!("{TRIGGER_STALE_BINDING}.request.json")),
                serde_json::to_string(&serde_json::json!({
                    "name": TRIGGER_STALE_BINDING,
                    "runId": RUN_ID,
                    "operationId": OPERATION_ID,
                    "issuedAt": "2026-10-04T00:00:00.000Z",
                    "mutateField": "attemptGeneration",
                }))
                .unwrap(),
            )
            .unwrap();

            assert!(
                state.drive_stale_binding_command(&channel, None).await,
                "the correlated command must be serviced"
            );

            let rejected = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                QaBarrierChannel::await_receipt_event(
                    &mut stale_rx,
                    STALE_RECEIPT_REJECTED_BARRIER,
                    1,
                ),
            )
            .await
            .expect("the registration fence must settle the stale rejection");
            assert_eq!(rejected["rejected"], serde_json::json!(true));
            assert_eq!(
                rejected["mismatchedField"],
                serde_json::json!("attemptGeneration")
            );
            assert_eq!(rejected["attemptGeneration"], serde_json::json!(2));
            assert_eq!(
                rejected["activeAttachTuple"]["attemptGeneration"],
                serde_json::json!(3)
            );
            assert_eq!(
                rejected["backendSessionId"],
                serde_json::json!("backend-stale-1")
            );
            let reason = rejected["reason"].as_str().expect("non-empty reason");
            assert!(reason.contains("attemptGeneration"), "{reason}");

            let reattached = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                QaBarrierChannel::await_receipt_event(&mut reattach_rx, REATTACH_MARKER_BARRIER, 1),
            )
            .await
            .expect("the legitimate reattach must settle");
            assert_eq!(
                reattached["backendSessionId"],
                serde_json::json!("backend-stale-1")
            );
            assert_eq!(reattached["reattached"], serde_json::json!(true));
            assert_eq!(reattached["newPtyCreated"], serde_json::json!(false));

            // The stale attempt never installed itself: the live binding is intact.
            assert_eq!(
                state.session_attach_tuple(&active.backend_session_id),
                Some(active.clone())
            );
            assert_eq!(
                state.session_attempt_generation(&active.backend_session_id),
                Some(3)
            );

            state.teardown();
        }
    }

    #[test]
    fn pane_liveness_diagnostics_prearmed_held_presentation_barrier() {
        let state = NativeTerminalSurfaceHostState::default();
        let session_id = "test-live-presentation-session";

        let coordinator = state
            .attach_test_session_for_liveness(session_id, 1, Some(42))
            .expect("attach test session");

        // 1. Prearm presentation dependency: schedule a pending frame
        assert!(coordinator.schedule_render(), "schedule render must succeed");
        assert!(coordinator.is_render_pending(), "coordinator frame must be pending");

        // 2. Observe real surface-owner production state via session_liveness_observation
        let held_obs = state
            .session_liveness_observation(session_id)
            .expect("observation must exist");
        assert_eq!(held_obs.session_id.as_deref(), Some(session_id));
        assert_eq!(held_obs.vt_epoch.as_deref(), Some("1"));
        assert_eq!(held_obs.vt_consumed_sequence, Some(42));
        assert_eq!(held_obs.has_unpresented_frames, Some(true));

        // 3. Classify: must yield BlockedInPresentation while held
        assert_eq!(
            classify_pane_liveness(&held_obs),
            PaneLivenessVerdict::BlockedInPresentation
        );

        // 4. Release controllable presentation dependency: consume/finish the render
        let owner = coordinator.begin_owned_render().expect("begin render");
        coordinator.finish_owned_render(owner, false);
        assert!(!coordinator.is_render_pending(), "frame must be rendered");

        // 5. Observe released state via session_liveness_observation
        let released_obs = state
            .session_liveness_observation(session_id)
            .expect("observation must exist");
        assert_eq!(released_obs.has_unpresented_frames, Some(false));

        // 6. Complete negative confirmations to assert IDLE after release
        let mut idle_candidate = released_obs;
        idle_candidate.reader_paused = Some(false);
        idle_candidate.kernel_stopped = Some(false);
        idle_candidate.suspended = Some(false);
        assert_eq!(
            classify_pane_liveness(&idle_candidate),
            PaneLivenessVerdict::Idle
        );
    }
}

/// The private QA producers this module owns, exercised through the real
/// channel: every assertion reads a receipt the emitter really wrote, and every
/// event wait subscribes BEFORE the emission it expects.
#[cfg(all(test, feature = "local-split-qa"))]
mod qa_native_producer_tests {
    use super::*;
    use crate::ipc::qa_barrier::{QaBarrierChannel, PRODUCER_ID};
    use serde_json::Value;

    const RUN_ID: &str = "qa-run-native-producer";
    const OPERATION_ID: &str = "qa-op-native-producer";

    fn armed_channel(dir: &std::path::Path, barrier: &str) -> QaBarrierChannel {
        std::fs::write(
            dir.join(format!("{barrier}.arm.json")),
            serde_json::to_string(&serde_json::json!({
                "name": barrier,
                "runId": RUN_ID,
                "operationId": OPERATION_ID,
                "deadlineMs": 5_000,
            }))
            .unwrap(),
        )
        .unwrap();
        let channel = QaBarrierChannel::new(dir.to_path_buf(), RUN_ID.to_string());
        let (acked, rejected) = channel.scan_and_ack_arms();
        assert_eq!(acked, vec![barrier.to_string()]);
        assert!(rejected.is_empty());
        channel
    }

    fn receipt_lines(dir: &std::path::Path, name: &str) -> Vec<Value> {
        std::fs::read_to_string(dir.join(format!("{name}.receipt.jsonl")))
            .map(|text| {
                text.lines()
                    .filter(|line| !line.trim().is_empty())
                    .map(|line| serde_json::from_str(line).unwrap())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn full_tuple() -> crate::daemon::protocol::PaneAttachTuple {
        serde_json::from_value(serde_json::json!({
            "backendSessionId": "backend-native-1",
            "incarnation": "incarnation-native-1",
            "daemonEpoch": "7",
            "frontendSessionId": "frontend-1",
            "paneIdentity": "pane-1",
            "bindingKey": "binding-1",
            "attemptGeneration": 3,
        }))
        .expect("valid seven-field tuple")
    }

    fn render_receipt() -> NativeTerminalSurfaceReceipt {
        NativeTerminalSurfaceReceipt {
            presented: true,
            render_deferred: false,
            render_suspended: false,
            cols: 80,
            rows: 24,
            rebuilt_rows: 24,
            reused_rows: 0,
            cursor_col: 1,
            cursor_row: 2,
            cell_width_px: 9,
            cell_height_px: 18,
            effective_scale_factor: Some(2.0),
        }
    }

    fn grid_snapshot(rows: &[&str]) -> RenderSnapshot {
        let cols = 40u16;
        let mut grid = vec![vec![CellSnapshot::default(); cols as usize]; rows.len()];
        for (row, text) in rows.iter().enumerate() {
            for (col, character) in text.chars().enumerate() {
                if col >= cols as usize {
                    break;
                }
                grid[row][col].text = character.to_string();
            }
        }
        RenderSnapshot {
            images: Vec::new(),
            cols,
            rows: rows.len() as u16,
            cursor: crate::native_terminal::cursor::CursorSnapshot {
                x: 0,
                y: 0,
                visible: true,
                blinking: false,
                wide_tail: false,
                visual_style: crate::native_terminal::cursor::CursorVisualStyle::Block,
            },
            grid,
        }
    }

    // Producer 1: the `presentation` settlement carries the product's OWN
    // receipt, so every field `requireSevenTupleReceipt` reads is real.
    #[tokio::test]
    async fn presentation_settlement_carries_the_product_seven_field_tuple() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("barriers");
        std::fs::create_dir_all(&dir).unwrap();
        let channel = Arc::new(armed_channel(&dir, crate::ipc::qa_barrier::PRESENTATION_BARRIER));
        let mut receipts = channel.subscribe_receipts();

        let tuple = full_tuple();
        let product_receipt = crate::daemon::protocol::PanePresentationReceipt {
            attach_tuple: tuple.clone(),
            presented: true,
            presentation_time_unix_ms: Some(1_700_000_000_000),
        };
        emit_native_presentation_receipt_qa(
            &channel,
            &tuple.backend_session_id,
            4,
            7,
            &render_receipt(),
            &product_receipt,
        );

        let settled = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            QaBarrierChannel::await_receipt_event(
                &mut receipts,
                crate::ipc::qa_barrier::PRESENTATION_BARRIER,
                1,
            ),
        )
        .await
        .expect("the presentation settlement must be written");

        assert_eq!(settled["runId"], serde_json::json!(RUN_ID));
        assert_eq!(settled["operationId"], serde_json::json!(OPERATION_ID));
        assert_eq!(settled["producer"], serde_json::json!(PRODUCER_ID));
        assert_eq!(settled["presented"], serde_json::json!(true));
        for (field, expected) in [
            ("backendSessionId", serde_json::json!("backend-native-1")),
            ("incarnation", serde_json::json!("incarnation-native-1")),
            ("daemonEpoch", serde_json::json!("7")),
            ("frontendSessionId", serde_json::json!("frontend-1")),
            ("paneIdentity", serde_json::json!("pane-1")),
            ("bindingKey", serde_json::json!("binding-1")),
            ("attemptGeneration", serde_json::json!(3)),
        ] {
            assert_eq!(settled["attachTuple"][field], expected, "{field}");
        }
    }

    // Producer 2: the released branch settles the coordinator-consumed shape
    // from the coordinator's measured state.
    #[test]
    fn presentation_released_settlement_reports_released_stage_progress() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("barriers");
        std::fs::create_dir_all(&dir).unwrap();
        let channel = Arc::new(armed_channel(&dir, crate::ipc::qa_barrier::PRESENTATION_BARRIER));

        // A released frame really completed: the coordinator is idle again.
        let coordinator = RenderScheduleCoordinator::new();
        assert!(coordinator.schedule_render());
        let owner = coordinator.begin_owned_render().expect("owned render");
        coordinator.finish_owned_render(owner, false);
        assert!(!coordinator.is_render_pending());

        emit_presentation_released_settlement_qa(
            &channel,
            "backend-native-1",
            OPERATION_ID,
            &coordinator,
            true,
            None,
        );

        let lines = receipt_lines(&dir, crate::ipc::qa_barrier::PRESENTATION_BARRIER);
        assert_eq!(lines.len(), 1);
        let line = &lines[0];
        assert_eq!(
            line["stage"],
            serde_json::json!("presentation_frame_settled")
        );
        assert_eq!(
            line["stageProgress"]["releaseOutcome"],
            serde_json::json!("released")
        );
        assert_eq!(line["stageProgress"]["frameConsumed"], serde_json::json!(true));
        assert_eq!(
            line["stageProgress"]["renderPendingAfter"],
            serde_json::json!(false)
        );
        assert_eq!(
            line["presentationEvidence"],
            serde_json::json!("coordinator-consumed")
        );
        assert_eq!(line["framePresented"], serde_json::json!(true));
        // The verdict is whatever the real classifier returns over the fields
        // this layer can attest; a positive recovery is never forced.
        let verdict = line["classifierVerdict"].as_str().expect("verdict");
        assert!(matches!(verdict, "Idle" | "Unknown"), "{verdict}");
        assert_eq!(
            line["evidenceMissing"],
            serde_json::json!(verdict == "Unknown")
        );
    }

    // Producer 2b: the settlement reports a POSITIVE recovery only from the daemon
    // facts it really observed; an unobserved fact keeps the honest non-pass.
    #[test]
    fn presentation_released_settlement_reports_positive_recovery_only_from_observed_daemon_facts() {
        use crate::ipc::qa_barrier::QaDaemonLiveness;

        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("barriers");
        std::fs::create_dir_all(&dir).unwrap();
        let channel = Arc::new(armed_channel(&dir, crate::ipc::qa_barrier::PRESENTATION_BARRIER));

        let coordinator = RenderScheduleCoordinator::new();
        assert!(coordinator.schedule_render());
        let owner = coordinator.begin_owned_render().expect("owned render");
        coordinator.finish_owned_render(owner, false);
        assert!(!coordinator.is_render_pending());

        // The daemon's own answer for a live, unpaused, running session.
        let observed = QaDaemonLiveness {
            observed: true,
            reader_paused: Some(false),
            kernel_stopped: Some(false),
            suspended: Some(false),
            daemon_epoch: Some("7".to_string()),
            failure: None,
        };
        emit_presentation_released_settlement_qa(
            &channel,
            "backend-native-1",
            OPERATION_ID,
            &coordinator,
            true,
            Some(&observed),
        );

        let lines = receipt_lines(&dir, crate::ipc::qa_barrier::PRESENTATION_BARRIER);
        assert_eq!(lines.len(), 1);
        let line = &lines[0];
        assert_eq!(line["classifierVerdict"], serde_json::json!("Idle"));
        assert_eq!(line["evidenceMissing"], serde_json::json!(false));
        assert_eq!(line["daemonFacts"]["observed"], serde_json::json!(true));
        assert_eq!(line["daemonFacts"]["readerPaused"], serde_json::json!(false));
        assert_eq!(line["daemonFacts"]["kernelStopped"], serde_json::json!(false));
        assert_eq!(line["snapshot"]["readerPaused"], serde_json::json!(false));
        assert_eq!(line["snapshot"]["kernelStopped"], serde_json::json!(false));
        assert_eq!(line["snapshot"]["suspended"], serde_json::json!(false));
        assert_eq!(
            line["snapshot"]["presentationReceiptReceived"],
            serde_json::json!(true)
        );

        // A daemon that did not report those facts cannot produce a pass: the same
        // settlement stays `Unknown` with the missing fields named.
        let unobserved = QaDaemonLiveness {
            observed: false,
            failure: Some("daemon describe unavailable".to_string()),
            ..Default::default()
        };
        emit_presentation_released_settlement_qa(
            &channel,
            "backend-native-1",
            OPERATION_ID,
            &coordinator,
            true,
            Some(&unobserved),
        );
        let lines = receipt_lines(&dir, crate::ipc::qa_barrier::PRESENTATION_BARRIER);
        assert_eq!(lines.len(), 2);
        let line = &lines[1];
        assert_eq!(line["classifierVerdict"], serde_json::json!("Unknown"));
        assert_eq!(line["evidenceMissing"], serde_json::json!(true));
        assert_eq!(line["daemonFacts"]["observed"], serde_json::json!(false));
        assert_eq!(
            line["daemonFacts"]["failure"],
            serde_json::json!("daemon describe unavailable")
        );
        let missing = line["evidenceMissingFields"].as_array().unwrap();
        for field in ["readerPaused", "kernelStopped", "suspended"] {
            assert!(
                missing.iter().any(|entry| entry == field),
                "{field} must be reported as missing"
            );
        }
    }

    // Only the identity fields the registration fence really compares can carry a
    // stale attempt; a generation of 0 has no strictly older value.
    #[test]
    fn stale_attempt_mutation_covers_exactly_the_fenced_fields() {
        let active: crate::daemon::protocol::PaneAttachTuple =
            serde_json::from_value(serde_json::json!({
                "backendSessionId": "backend-1",
                "incarnation": "incarnation-1",
                "daemonEpoch": "7",
                "frontendSessionId": "frontend-1",
                "paneIdentity": "pane-1",
                "bindingKey": "binding-1",
                "attemptGeneration": 3,
            }))
            .unwrap();
        for field in ["frontendSessionId", "paneIdentity", "bindingKey"] {
            let stale = mutate_attach_tuple_field(&active, field).expect("fenced field mutates");
            assert!(!pane_liveness_registration_matches(&active, &stale), "{field}");
            assert_eq!(stale.backend_session_id, active.backend_session_id);
            assert_eq!(stale.attempt_generation, active.attempt_generation);
        }
        let stale = mutate_attach_tuple_field(&active, "attemptGeneration").unwrap();
        assert_eq!(stale.attempt_generation, 2);
        assert!(!pane_liveness_registration_matches(&active, &stale));
        // A field the fence does not compare is never mutated into a stale attempt
        // the fence could not reject.
        for field in ["incarnation", "backendSessionId", "daemonEpoch", "anything"] {
            assert!(mutate_attach_tuple_field(&active, field).is_none(), "{field}");
        }
        let mut zero = active.clone();
        zero.attempt_generation = 0;
        assert!(mutate_attach_tuple_field(&zero, "attemptGeneration").is_none());
    }

    // Producer 5: the `frameSubmitted` half is the terminal layer's sidecar
    // contract, and every real submission is recorded with the coverage it
    // really has.
    #[test]
    fn frame_submission_sidecar_carries_the_correlation_and_real_coverage() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("barriers");
        std::fs::create_dir_all(&dir).unwrap();

        let record = qa_frame_submission_record(RUN_ID, OPERATION_ID, "backend-native-1", 42);
        let mut keys: Vec<&str> = record
            .as_object()
            .expect("object record")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "coveredSequence",
                "operationId",
                "producerComponent",
                "runId",
                "sessionId"
            ]
        );

        assert!(append_qa_frame_submission(&dir, &record));
        assert!(append_qa_frame_submission(&dir, &record));
        let text = std::fs::read_to_string(dir.join("frame-submitted.jsonl")).unwrap();
        let lines: Vec<Value> = text
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines.len(), 2, "one line per real frame submission");
        for line in &lines {
            assert_eq!(line["runId"], serde_json::json!(RUN_ID));
            assert_eq!(line["operationId"], serde_json::json!(OPERATION_ID));
            assert_eq!(line["sessionId"], serde_json::json!("backend-native-1"));
            assert_eq!(line["coveredSequence"], serde_json::json!(42));
        }
    }

    // The frame evidence is only as good as the coverage it reports, so a
    // published frame carries the sequence its session had really applied.
    #[test]
    fn published_frame_reports_the_sequence_it_covers() {
        let slot = SnapshotSlot::new();
        slot.set_attached(true);
        let layout = SurfaceCompositionLayout {
            cols: 80,
            rows: 24,
            physical_bounds: PhysicalBounds {
                x: 0,
                y: 0,
                width: 800,
                height: 480,
            },
        };
        let bounds = LogicalBounds {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 480.0,
            scale_factor: 1.0,
        };
        let input = SessionRenderInput {
            snapshot: grid_snapshot(&["$ echo"]),
            selection: None,
            scrollbar_overlay: None,
            attention_frame: false,
            synchronized_output: false,
        };
        let generation = slot.publish(layout, bounds, input, None, Some(42));
        let frame = slot.consume().expect("published frame");
        assert_eq!(frame.generation, generation);
        assert_eq!(frame.covered_sequence, Some(42));
    }

    // Producer 3: a delayed positive completion is fenced off with the field
    // that really diverged.
    #[tokio::test]
    async fn stale_completion_rejection_names_the_mismatched_identity_field() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("barriers");
        std::fs::create_dir_all(&dir).unwrap();
        let channel = Arc::new(armed_channel(
            &dir,
            STALE_RECEIPT_REJECTED_BARRIER,
        ));
        let mut receipts = channel.subscribe_receipts();

        let active = full_tuple();
        let mut stale = active.clone();
        stale.attempt_generation += 1;
        emit_stale_completion_rejected_qa(
            &channel,
            &stale.backend_session_id,
            Some(3),
            Some(&active),
            3,
            Some(&stale),
        );

        let settled = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            QaBarrierChannel::await_receipt_event(
                &mut receipts,
                STALE_RECEIPT_REJECTED_BARRIER,
                1,
            ),
        )
        .await
        .expect("the stale rejection must be settled");

        assert_eq!(settled["rejected"], serde_json::json!(true));
        assert_eq!(
            settled["mismatchedField"],
            serde_json::json!("attemptGeneration")
        );
        assert_eq!(
            settled["backendSessionId"],
            serde_json::json!("backend-native-1")
        );
        assert_eq!(settled["attemptGeneration"], serde_json::json!(4));
        let reason = settled["reason"].as_str().expect("non-empty reason");
        assert!(reason.contains("attemptGeneration"), "{reason}");

        // A superseded frame generation reports the frame fence, not an
        // identity field.
        emit_stale_completion_rejected_qa(
            &channel,
            &active.backend_session_id,
            Some(9),
            Some(&active),
            3,
            Some(&active),
        );
        let settled = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            QaBarrierChannel::await_receipt_event(
                &mut receipts,
                STALE_RECEIPT_REJECTED_BARRIER,
                2,
            ),
        )
        .await
        .expect("the superseded completion must be settled");
        assert_eq!(
            settled["mismatchedField"],
            serde_json::json!("frameGeneration")
        );
        let reason = settled["reason"].as_str().expect("non-empty reason");
        assert!(reason.contains("superseded"), "{reason}");
    }

    // Producer 4: the legitimate reattach reports the live binding and states
    // that this layer created no PTY for it.
    #[tokio::test]
    async fn reattach_marker_reports_the_live_binding_without_a_new_pty() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("barriers");
        std::fs::create_dir_all(&dir).unwrap();
        let channel = Arc::new(armed_channel(&dir, REATTACH_MARKER_BARRIER));
        let mut receipts = channel.subscribe_receipts();

        let tuple = full_tuple();
        emit_reattach_marker_qa(&channel, &tuple.backend_session_id, &tuple, true);

        let settled = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            QaBarrierChannel::await_receipt_event(
                &mut receipts,
                REATTACH_MARKER_BARRIER,
                1,
            ),
        )
        .await
        .expect("the reattach marker must be settled");

        assert_eq!(
            settled["backendSessionId"],
            serde_json::json!("backend-native-1")
        );
        assert_eq!(settled["attemptGeneration"], serde_json::json!(3));
        assert_eq!(settled["reattached"], serde_json::json!(true));
        assert_eq!(settled["newPtyCreated"], serde_json::json!(false));
        assert_eq!(settled["boundsApplied"], serde_json::json!(true));
        assert_eq!(
            settled["attachTuple"]["bindingKey"],
            serde_json::json!("binding-1")
        );
    }

    // The rejection helpers name the field the fence really compared.
    #[test]
    fn identity_mismatch_helpers_name_the_field_that_really_diverged() {
        let active = full_tuple();

        let mut other_epoch = active.clone();
        other_epoch.daemon_epoch = "8".into();
        assert_eq!(
            qa_tuple_mismatch_field(Some(&other_epoch), Some(&active)),
            "daemonEpoch"
        );
        assert_eq!(qa_tuple_mismatch_field(None, Some(&active)), "attachTuple");
        assert_eq!(qa_tuple_mismatch_field(Some(&other_epoch), None), "attachTuple");

        let mut other_pane = active.clone();
        other_pane.pane_identity = "pane-2".into();
        assert_eq!(
            qa_registration_mismatch_field(&active, &other_pane),
            "paneIdentity"
        );

        let mut newer_attempt = active.clone();
        newer_attempt.attempt_generation = active.attempt_generation + 1;
        assert_eq!(
            qa_registration_mismatch_field(&active, &newer_attempt),
            "attemptGeneration"
        );
    }
}
