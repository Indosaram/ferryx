use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use tauri::{Emitter, Manager, PhysicalSize, Runtime, WebviewWindow};

use super::composition::{
    CellMetrics, LogicalBounds, PhysicalBounds, PlatformCompositorDescriptor,
    SurfaceCompositionLayout,
};
use super::engine::TerminalEngine;
use super::error::NativeTerminalError;
use super::input::{cursor_style_for_focus, NativeTerminalInput};
use super::platform::PlatformCompositorTarget;
use super::renderer::font_manager;
use super::renderer::{NativeTerminalRenderer, RendererConfig, RendererTheme};
use super::snapshot::RenderSnapshot;
use super::surface_error::{classify_surface_error, SurfaceFrameAction};
pub use super::surface_snapshot::snapshot_for_layout;
use super::terminal::NativeTerminal;
use crate::daemon::{DaemonAttachment, DaemonStreamMessage};
use crate::terminal::preferences::cached_terminal_preferences;

pub const NATIVE_TERMINAL_TITLE_EVENT: &str = "native_terminal_title";
pub const NATIVE_TERMINAL_BELL_EVENT: &str = "native_terminal_bell";
pub const NATIVE_TERMINAL_AGENT_STATE_EVENT: &str = "native_terminal_agent_state";

/// Marks a state the agent reported about itself through the Ferryx extension. Such a report is
/// authoritative: screen rules only infer, so once a session speaks for itself the inferred
/// source must never overwrite it.
pub const AGENT_EXTENSION_MANIFEST_ID: &str = "ferryx-extension";
pub const NATIVE_TERMINAL_SCROLLBAR_EVENT: &str = "native_terminal_scrollbar";

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeTerminalSurfaceReceipt {
    pub cols: u16,
    pub rows: u16,
    pub rebuilt_rows: u16,
    pub reused_rows: u16,
    pub cursor_col: u16,
    pub cursor_row: u16,
    pub cell_width_px: u32,
    pub cell_height_px: u32,
}

impl NativeTerminalSurfaceReceipt {
    fn from_snapshot(
        layout: SurfaceCompositionLayout,
        snapshot: &RenderSnapshot,
        rebuilt_rows: u16,
        reused_rows: u16,
        cell_metrics: CellMetrics,
    ) -> Self {
        Self {
            cols: layout.cols,
            rows: layout.rows,
            rebuilt_rows,
            reused_rows,
            cursor_col: snapshot.cursor.x,
            cursor_row: snapshot.cursor.y,
            cell_width_px: cell_metrics.width_px,
            cell_height_px: cell_metrics.height_px,
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
}

impl RenderScheduleCoordinator {
    pub fn new() -> Self {
        Self {
            state: AtomicU8::new(RENDER_IDLE),
        }
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
        self.state
            .compare_exchange(
                RENDER_SCHEDULED,
                RENDERING,
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .is_ok()
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
        self.state.swap(RENDER_IDLE, Ordering::SeqCst) != RENDER_IDLE
    }

    /// Returns `true` if a render pass is scheduled, active, or awaiting a follow-up.
    pub fn is_render_pending(&self) -> bool {
        self.state.load(Ordering::SeqCst) != RENDER_IDLE
    }
}

pub struct NativeTerminalSession {
    pub terminal: NativeTerminal,
    pub focused: bool,
    pub layout: Option<SurfaceCompositionLayout>,
    pub logical_bounds: Option<LogicalBounds>,
    pub cell_metrics: Option<CellMetrics>,
    pub stream_task: Option<tokio::task::JoinHandle<()>>,
    pub pump_task: Option<tokio::task::JoinHandle<()>>,
    pub last_sequence: Option<u64>,
    pub update_sender: tokio::sync::watch::Sender<()>,
    pub render_coordinator: Arc<RenderScheduleCoordinator>,
    pub last_agent_activity: Option<crate::agent_detect::AgentActivity>,
    pub last_scrollbar: Option<NativeTerminalScrollbarPayload>,
    /// Set once the agent reports its own state through the Ferryx extension, which permanently
    /// disables screen inference for this session.
    pub agent_reports_own_state: bool,
    /// Whether a frontend pane currently owns a compositor surface for this session.
    ///
    /// A session outlives its surface: [`NativeTerminalSurfaceHostState::detach_session`] keeps the
    /// daemon pump running for a backgrounded agent while releasing the GPU host. Geometry updates
    /// that were already in flight when that happened must not resurrect a surface for a pane that
    /// is no longer on screen, so they are rejected with
    /// [`NativeTerminalError::SessionDetached`] instead.
    pub surface_attached: bool,
}

pub struct NativeTerminalSurfaceHostState {
    hosts: Arc<Mutex<HashMap<String, NativeTerminalSurfaceHost>>>,
    sessions: Arc<Mutex<HashMap<String, NativeTerminalSession>>>,
    event_sink: Arc<RwLock<Option<NativeTerminalEventSink>>>,
    pty_resize_sink: Arc<RwLock<Option<NativeTerminalPtyResizeSink>>>,
}

fn dispatch_scheduled_render<R: Runtime>(
    window: WebviewWindow<R>,
    hosts: Arc<Mutex<HashMap<String, NativeTerminalSurfaceHost>>>,
    sessions: Arc<Mutex<HashMap<String, NativeTerminalSession>>>,
    session_id: String,
    coordinator: Arc<RenderScheduleCoordinator>,
) {
    let surface_window = window.clone();
    let follow_up_window = window.clone();
    let failure_coordinator = Arc::clone(&coordinator);
    let failure_session_id = session_id.clone();
    if let Err(err) = window.run_on_main_thread(move || {
        if !coordinator.begin_render() {
            return;
        }
        let render_state = {
            let sessions_guard = sessions.lock();
            sessions_guard.get(&session_id).and_then(|session| {
                let layout = session.layout?;
                let logical_bounds = session.logical_bounds?;
                match session.terminal.render_snapshot() {
                    Ok(mut snapshot) => {
                        snapshot.cursor.visual_style = cursor_style_for_focus(session.focused);
                        Some((layout, logical_bounds, snapshot))
                    }
                    Err(err) => {
                        tracing::warn!(
                            session_id = %session_id,
                            error = %err,
                            "Failed to capture native terminal snapshot for render"
                        );
                        None
                    }
                }
            })
        };
        if let Some((layout, logical_bounds, snapshot)) = render_state {
            let mut hosts_guard = hosts.lock();
            if let Some(host) = hosts_guard.get_mut(&session_id) {
                host.layout = Some(layout);
                host.logical_bounds = Some(logical_bounds);
                host.target.update_viewport(Some(logical_bounds));
                if let Err(err) =
                    host.render_snapshot(&surface_window, layout, &snapshot, None)
                {
                    tracing::warn!(
                        session_id = %session_id,
                        error = %err,
                        "Failed to render native terminal snapshot"
                    );
                }
            }
        }

        if coordinator.finish_render() {
            dispatch_scheduled_render(
                follow_up_window,
                hosts,
                sessions,
                session_id,
                coordinator,
            );
        }
    }) {
        failure_coordinator.consume_render();
        tracing::warn!(
            session_id = %failure_session_id,
            error = %err,
            "Failed to dispatch native terminal render to main thread"
        );
    }
}

impl Clone for NativeTerminalSurfaceHostState {
    fn clone(&self) -> Self {
        Self {
            hosts: Arc::clone(&self.hosts),
            sessions: Arc::clone(&self.sessions),
            event_sink: Arc::clone(&self.event_sink),
            pty_resize_sink: Arc::clone(&self.pty_resize_sink),
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
        }
    }
}

fn take_native_terminal_events(
    session: &mut NativeTerminalSession,
    session_id: &str,
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
                    },
                ));
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

    pub fn with_session_terminal<T>(
        &self,
        session_id: &str,
        f: impl FnOnce(&mut NativeTerminal) -> Result<T, NativeTerminalError>,
    ) -> Result<T, NativeTerminalError> {
        validate_session_id(session_id)?;
        let mut sessions = self.sessions.lock();
        let session = sessions
            .get_mut(session_id)
            .ok_or(NativeTerminalError::NoValue)?;
        f(&mut session.terminal)
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

    pub fn prepare_session_layout(
        &self,
        request: NativeTerminalBoundsRequest,
        cell_metrics: CellMetrics,
    ) -> Result<SurfaceCompositionLayout, NativeTerminalError> {
        let layout = request.layout(cell_metrics)?;
        let mut sessions = self.sessions.lock();
        let (session, initialized) = match sessions.get_mut(&request.session_id) {
            Some(session) => (session, false),
            None => {
                let terminal = NativeTerminal::new(layout.cols, layout.rows)?;
                let (update_sender, _) = tokio::sync::watch::channel(());
                let render_coordinator = Arc::new(RenderScheduleCoordinator::new());
                sessions.insert(
                    request.session_id.clone(),
                    NativeTerminalSession {
                        terminal,
                        focused: false,
                        layout: None,
                        logical_bounds: None,
                        cell_metrics: None,
                        stream_task: None,
                        pump_task: None,
                        last_sequence: None,
                        update_sender,
                        render_coordinator,
                        last_agent_activity: None,
                        last_scrollbar: None,
                        agent_reports_own_state: false,
                        surface_attached: true,
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

        let resized = if session.terminal.dimensions()? != (layout.cols, layout.rows) {
            session.terminal.resize(
                layout.cols,
                layout.rows,
                cell_metrics.width_px,
                cell_metrics.height_px,
            )?;
            true
        } else {
            false
        };
        session.layout = Some(layout);
        session.logical_bounds = Some(request.bounds);
        session.cell_metrics = Some(cell_metrics);
        drop(sessions);
        if initialized || resized {
            self.notify_pty_resize(&request.session_id, layout.cols, layout.rows);
        }
        Ok(layout)
    }

    pub fn attach_daemon_attachment_with_bounds<R: Runtime>(
        &self,
        session_id: &str,
        attachment: DaemonAttachment,
        app: Option<tauri::AppHandle<R>>,
        bounds: Option<LogicalBounds>,
    ) -> Result<(), NativeTerminalError> {
        validate_session_id(session_id)?;
        if let Some(bounds) = bounds {
            // Re-arm the surface before laying out: an attach following a detach must accept its own
            // initial geometry even though the detach cleared the attached flag.
            if let Some(session) = self.sessions.lock().get_mut(session_id) {
                session.surface_attached = true;
            }
            let metrics = font_manager::derived_cell_metrics_for_scale(bounds.scale_factor);
            self.prepare_session_layout(
                NativeTerminalBoundsRequest {
                    session_id: session_id.to_string(),
                    bounds,
                },
                metrics,
            )?;
        }
        self.attach_daemon_attachment(session_id, attachment, app)
    }

    pub fn reattach_existing_session_with_bounds(
        &self,
        session_id: &str,
        bounds: Option<LogicalBounds>,
    ) -> Result<bool, NativeTerminalError> {
        validate_session_id(session_id)?;
        let mut sessions = self.sessions.lock();
        let Some(session) = sessions.get_mut(session_id) else {
            return Ok(false);
        };
        let stream_is_live = session
            .stream_task
            .as_ref()
            .is_some_and(|task| !task.is_finished());
        let pump_is_live = session
            .pump_task
            .as_ref()
            .is_some_and(|task| !task.is_finished());
        if !stream_is_live || !pump_is_live {
            return Ok(false);
        }

        session.surface_attached = true;
        let resized_dimensions = if let Some(bounds) = bounds {
            let metrics = font_manager::derived_cell_metrics_for_scale(bounds.scale_factor);
            let layout = NativeTerminalBoundsRequest {
                session_id: session_id.to_string(),
                bounds,
            }
            .layout(metrics)?;
            let resized = if session.terminal.dimensions()? != (layout.cols, layout.rows) {
                session.terminal.resize(
                    layout.cols,
                    layout.rows,
                    metrics.width_px,
                    metrics.height_px,
                )?;
                true
            } else {
                false
            };
            session.layout = Some(layout);
            session.logical_bounds = Some(bounds);
            session.cell_metrics = Some(metrics);
            resized.then_some((layout.cols, layout.rows))
        } else {
            None
        };
        session.render_coordinator.consume_render();
        drop(sessions);
        if let Some((cols, rows)) = resized_dimensions {
            self.notify_pty_resize(session_id, cols, rows);
        }
        Ok(true)
    }

    pub fn attach_daemon_attachment<R: Runtime>(
        &self,
        session_id: &str,
        attachment: DaemonAttachment,
        app: Option<tauri::AppHandle<R>>,
    ) -> Result<(), NativeTerminalError> {
        validate_session_id(session_id)?;

        let initial_dims = (80, 24);

        let (update_sender, render_coordinator, events) = {
            let mut sessions = self.sessions.lock();
            let (update_sender, render_coordinator) =
                if let Some(session) = sessions.get_mut(session_id) {
                    // A backgrounded session kept streaming without a surface; this attach gives it
                    // one again and re-enables geometry updates.
                    session.surface_attached = true;
                    if let Some(task) = session.stream_task.take() {
                        task.abort();
                    }
                    if let Some(task) = session.pump_task.take() {
                        task.abort();
                    }
                    session.terminal.reset();
                    if !attachment.history.is_empty() {
                        session.terminal.feed(&attachment.history)?;
                    }
                    session.last_sequence = attachment.end_sequence;
                    session.render_coordinator.consume_render();
                    (
                        session.update_sender.clone(),
                        Arc::clone(&session.render_coordinator),
                    )
                } else {
                    let mut terminal = NativeTerminal::new(initial_dims.0, initial_dims.1)?;
                    if !attachment.history.is_empty() {
                        terminal.feed(&attachment.history)?;
                    }
                    // A fresh session created by an attach owns a surface by definition.
                    let (update_sender, _) = tokio::sync::watch::channel(());
                    let render_coordinator = Arc::new(RenderScheduleCoordinator::new());
                    sessions.insert(
                        session_id.to_string(),
                        NativeTerminalSession {
                            terminal,
                            focused: false,
                            layout: None,
                            logical_bounds: None,
                            cell_metrics: None,
                            stream_task: None,
                            pump_task: None,
                            last_sequence: attachment.end_sequence,
                            update_sender: update_sender.clone(),
                            render_coordinator: Arc::clone(&render_coordinator),
                            last_agent_activity: None,
                            last_scrollbar: None,
                            agent_reports_own_state: false,
                            surface_attached: true,
                        },
                    );
                    (update_sender, render_coordinator)
                };
            let session = sessions
                .get_mut(session_id)
                .ok_or(NativeTerminalError::NoValue)?;
            let events = take_native_terminal_events(session, session_id);
            (update_sender, render_coordinator, events)
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
        let app_handle = app;
        let pump_task = tokio::spawn(async move {
            while let Some(msg) = messages.recv().await {
                match msg {
                    DaemonStreamMessage::Output { sequence, data, .. } => {
                        let (session_exists, events) = {
                            let mut sessions_guard = sessions.lock();
                            if let Some(sess) = sessions_guard.get_mut(&session_id_owned) {
                                if let Err(err) = sess.terminal.feed(&data) {
                                    tracing::warn!(
                                        session_id = %session_id_owned,
                                        error = %err,
                                        "Failed to feed daemon output to native terminal"
                                    );
                                }
                                sess.last_sequence = Some(sequence);
                                (true, take_native_terminal_events(sess, &session_id_owned))
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

                        if session_exists && render_coordinator.schedule_render() {
                            if let Some(app) = &app_handle {
                                if let Some(window) = app.get_webview_window("main") {
                                    dispatch_scheduled_render(
                                        window,
                                        Arc::clone(&hosts),
                                        Arc::clone(&sessions),
                                        session_id_owned.clone(),
                                        Arc::clone(&render_coordinator),
                                    );
                                } else {
                                    render_coordinator.consume_render();
                                }
                            } else {
                                render_coordinator.consume_render();
                            }
                        }
                    }
                    DaemonStreamMessage::Lagged { history, .. } => {
                        let (session_exists, events) = {
                            let mut sessions_guard = sessions.lock();
                            if let Some(sess) = sessions_guard.get_mut(&session_id_owned) {
                                sess.terminal.reset();
                                if let Err(err) = sess.terminal.feed(&history) {
                                    tracing::warn!(
                                        session_id = %session_id_owned,
                                        error = %err,
                                        "Failed to feed recovery history to native terminal"
                                    );
                                }
                                (true, take_native_terminal_events(sess, &session_id_owned))
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
                    }
                    DaemonStreamMessage::Gap { .. } => {
                        let (session_exists, events) = {
                            let mut sessions_guard = sessions.lock();
                            if let Some(sess) = sessions_guard.get_mut(&session_id_owned) {
                                sess.terminal.reset();
                                (true, take_native_terminal_events(sess, &session_id_owned))
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
                    }
                    DaemonStreamMessage::AgentState { state, agent, .. } => {
                        let reported = match state.as_ref() {
                            "working" => Some(crate::agent_detect::AgentActivity::Working),
                            "blocked" => Some(crate::agent_detect::AgentActivity::Blocked),
                            "idle" => Some(crate::agent_detect::AgentActivity::Idle),
                            _ => None,
                        };
                        if let Some(reported) = reported {
                            let changed = {
                                let mut sessions_guard = sessions.lock();
                                match sessions_guard.get_mut(&session_id_owned) {
                                    Some(sess) => {
                                        sess.agent_reports_own_state = true;
                                        if sess.last_agent_activity == Some(reported) {
                                            false
                                        } else {
                                            sess.last_agent_activity = Some(reported);
                                            true
                                        }
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
                                        },
                                    ),
                                );
                            }
                        }
                    }
                    DaemonStreamMessage::Exit { .. } => {
                        update_sender.send_replace(());
                        break;
                    }
                }
            }
        });

        {
            let mut sessions = self.sessions.lock();
            if let Some(session) = sessions.get_mut(session_id) {
                session.stream_task = Some(stream_task);
                session.pump_task = Some(pump_task);
            }
        }

        Ok(())
    }

    /// Releases the GPU surface for an unmounted pane while KEEPING the terminal session and its
    /// daemon pump alive, so a backgrounded agent keeps reporting title/bell/agent-state instead of
    /// freezing on its last observed value. Use [`Self::close_session`] to discard the session.
    pub fn detach_session(&self, session_id: &str) {
        {
            let mut sessions = self.sessions.lock();
            if let Some(session) = sessions.get_mut(session_id) {
                session.focused = false;
                session.layout = None;
                session.logical_bounds = None;
                session.surface_attached = false;
                session.render_coordinator.consume_render();
            }
        }

        let mut hosts = self.hosts.lock();
        hosts.remove(session_id);
    }

    /// Discards a session entirely, aborting its daemon stream and pump tasks.
    pub fn close_session(&self, session_id: &str) {
        let mut sessions = self.sessions.lock();
        if let Some(mut session) = sessions.remove(session_id) {
            if let Some(task) = session.stream_task.take() {
                task.abort();
            }
            if let Some(task) = session.pump_task.take() {
                task.abort();
            }
        }
        drop(sessions);

        let mut hosts = self.hosts.lock();
        hosts.remove(session_id);
    }

    pub fn teardown(&self) {
        let mut sessions = self.sessions.lock();
        for (_, session) in sessions.drain() {
            if let Some(task) = session.stream_task {
                task.abort();
            }
            if let Some(task) = session.pump_task {
                task.abort();
            }
        }
        self.hosts.lock().clear();
    }

    pub fn target_descriptor(&self) -> PlatformCompositorDescriptor {
        let hosts = self.hosts.lock();
        if let Some(host) = hosts.values().next() {
            host.target.descriptor()
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
        let mut snapshot = session.terminal.render_snapshot()?;
        snapshot.cursor.visual_style = cursor_style_for_focus(session.focused);
        Ok(Some(snapshot))
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
                let terminal = NativeTerminal::new(80, 24)?;
                let (update_sender, _) = tokio::sync::watch::channel(());
                let render_coordinator = Arc::new(RenderScheduleCoordinator::new());
                sessions.insert(
                    session_id.to_string(),
                    NativeTerminalSession {
                        terminal,
                        focused: false,
                        layout: None,
                        logical_bounds: None,
                        cell_metrics: None,
                        stream_task: None,
                        pump_task: None,
                        last_sequence: None,
                        update_sender,
                        render_coordinator,
                        last_agent_activity: None,
                        last_scrollbar: None,
                        agent_reports_own_state: false,
                        surface_attached: true,
                    },
                );
                sessions
                    .get_mut(session_id)
                    .ok_or(NativeTerminalError::NoValue)?
            }
        };
        input.encoded(&session.terminal)
    }

    pub fn get_receipt<R: Runtime>(
        &self,
        _window: &WebviewWindow<R>,
        session_id: &str,
    ) -> Result<NativeTerminalSurfaceReceipt, NativeTerminalError> {
        validate_session_id(session_id)?;
        let sessions = self.sessions.lock();
        let session = sessions
            .get(session_id)
            .ok_or(NativeTerminalError::NoValue)?;
        let layout = session.layout.ok_or(NativeTerminalError::NoValue)?;
        let cell_metrics = session.cell_metrics.ok_or(NativeTerminalError::NoValue)?;
        let snapshot = session.terminal.render_snapshot()?;
        Ok(NativeTerminalSurfaceReceipt::from_snapshot(
            layout,
            &snapshot,
            0,
            0,
            cell_metrics,
        ))
    }

    pub fn render<R: Runtime>(
        &self,
        window: &WebviewWindow<R>,
        request: NativeTerminalBoundsRequest,
    ) -> Result<NativeTerminalSurfaceReceipt, NativeTerminalError> {
        let scale_factor = request.bounds.scale_factor;
        let cell_metrics = font_manager::derived_cell_metrics_for_scale(scale_factor);
        let logical_bounds = request.bounds;
        let session_id = request.session_id.clone();
        // A pane that unmounted while its ResizeObserver callback was still in flight (rapid tab
        // switching) sends geometry for a session the compositor has already released. Rendering it
        // would rebuild a GPU surface for a pane nobody can see, so report the benign detached state
        // and let the caller drop the update.
        self.ensure_surface_attached(&session_id)?;
        let layout = self.prepare_session_layout(request, cell_metrics)?;
        let snapshot = {
            let sessions = self.sessions.lock();
            let session = sessions
                .get(&session_id)
                .ok_or(NativeTerminalError::NoValue)?;
            let mut snapshot = session.terminal.render_snapshot()?;
            snapshot.cursor.visual_style = cursor_style_for_focus(session.focused);
            snapshot
        };

        let mut hosts = self.hosts.lock();
        let host = match hosts.entry(session_id) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(NativeTerminalSurfaceHost::new(window, scale_factor)?)
            }
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
        host.renderer.update_config(renderer_config)?;
        host.layout = Some(layout);
        host.logical_bounds = Some(logical_bounds);

        host.render_snapshot(window, layout, &snapshot, None)
    }

    pub fn set_focus<R: Runtime>(
        &self,
        window: &WebviewWindow<R>,
        session_id: &str,
        focused: bool,
    ) -> Result<NativeTerminalSurfaceReceipt, NativeTerminalError> {
        validate_session_id(session_id)?;
        let (layout, logical_bounds, cell_metrics, snapshot) = {
            let mut sessions = self.sessions.lock();
            let session = sessions
                .get_mut(session_id)
                .ok_or(NativeTerminalError::NoValue)?;
            session.focused = focused;
            let layout = session.layout.ok_or(NativeTerminalError::NoValue)?;
            let logical_bounds = session.logical_bounds.ok_or(NativeTerminalError::NoValue)?;
            let cell_metrics = session.cell_metrics.ok_or(NativeTerminalError::NoValue)?;
            let mut snapshot = session.terminal.render_snapshot()?;
            snapshot.cursor.visual_style = cursor_style_for_focus(focused);
            (layout, logical_bounds, cell_metrics, snapshot)
        };

        let mut hosts = self.hosts.lock();
        if let Some(host) = hosts.get_mut(session_id) {
            host.layout = Some(layout);
            host.logical_bounds = Some(logical_bounds);
            host.render_snapshot(window, layout, &snapshot, None)
        } else {
            Ok(NativeTerminalSurfaceReceipt::from_snapshot(
                layout,
                &snapshot,
                0,
                0,
                cell_metrics,
            ))
        }
    }
}

/// Host container managing the active WGPU surface, platform compositor child view, and renderer.
///
/// # Drop Order Invariant
///
/// In Rust, struct fields are dropped in top-to-bottom declaration order.
/// 1. `surface` MUST drop before `target`: The WGPU `Surface` (and its internal Metal layer) must
///    be dropped and destroyed while the native child NSView (`target`) is still valid and parented.
/// 2. `target` drops after `surface`: Unparents (`removeFromSuperview`) and releases the child NSView.
/// 3. `renderer` drops: Releases GPU device, pipelines, and glyph atlas resources.
struct NativeTerminalSurfaceHost {
    surface: wgpu::Surface<'static>,
    target: PlatformCompositorTarget,
    renderer: NativeTerminalRenderer,
    format: wgpu::TextureFormat,
    size: PhysicalSize<u32>,
    layout: Option<SurfaceCompositionLayout>,
    logical_bounds: Option<LogicalBounds>,
}

impl NativeTerminalSurfaceHost {
    fn new<R: Runtime>(
        window: &WebviewWindow<R>,
        scale_factor: f64,
    ) -> Result<Self, NativeTerminalError> {
        let target = PlatformCompositorTarget::new(window)?;
        let descriptor = target.descriptor();
        descriptor.validate_desktop_composition()?;

        let scale = if scale_factor.is_finite() && scale_factor > 0.0 {
            scale_factor as f32
        } else {
            1.0
        };
        let cell_metrics = font_manager::derived_cell_metrics_for_scale(scale as f64);
        let renderer = NativeTerminalRenderer::new(RendererConfig {
            cell_width_px: cell_metrics.width_px,
            cell_height_px: cell_metrics.height_px,
            device_scale_factor: scale,
            theme: RendererTheme::from(cached_terminal_preferences().as_ref()),
        })?;

        let surface = renderer.create_surface(target.surface_target())?;

        let size = PhysicalSize::new(1, 1);
        let format = renderer.configure_surface(&surface, size.width, size.height)?;
        Ok(Self {
            surface,
            target,
            renderer,
            format,
            size,
            layout: None,
            logical_bounds: None,
        })
    }

    fn render_snapshot<R: Runtime>(
        &mut self,
        window: &WebviewWindow<R>,
        layout: SurfaceCompositionLayout,
        snapshot: &RenderSnapshot,
        selection: Option<&super::renderer::SelectionSnapshot>,
    ) -> Result<NativeTerminalSurfaceReceipt, NativeTerminalError> {
        if let Some(bounds) = self.logical_bounds {
            self.target.update_viewport(Some(bounds));
        }
        let surface_size =
            PhysicalSize::new(layout.physical_bounds.width, layout.physical_bounds.height);
        if surface_size != self.size {
            self.format = self.renderer.configure_surface(
                &self.surface,
                surface_size.width,
                surface_size.height,
            )?;
            self.size = surface_size;
        }

        let local_viewport = PhysicalBounds {
            x: 0,
            y: 0,
            width: layout.physical_bounds.width,
            height: layout.physical_bounds.height,
        };

        let frame = match self.surface.get_current_texture() {
            Ok(frame) => Some(frame),
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.format = self.renderer.configure_surface(
                    &self.surface,
                    surface_size.width,
                    surface_size.height,
                )?;
                match self.surface.get_current_texture() {
                    Ok(frame) => Some(frame),
                    Err(error) => match classify_surface_error(error)? {
                        SurfaceFrameAction::Drop => None,
                    },
                }
            }
            Err(error) => match classify_surface_error(error)? {
                SurfaceFrameAction::Drop => None,
            },
        };
        let cell_metrics = CellMetrics {
            width_px: self.renderer.config().cell_width_px,
            height_px: self.renderer.config().cell_height_px,
        };
        let Some(frame) = frame else {
            return Ok(NativeTerminalSurfaceReceipt::from_snapshot(
                layout,
                snapshot,
                0,
                0,
                cell_metrics,
            ));
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let (rebuilt_rows, reused_rows) = self.renderer.render_to_surface_viewport(
            snapshot,
            selection,
            &view,
            surface_size.width,
            surface_size.height,
            self.format,
            local_viewport,
        )?;
        frame.present();
        self.target.reveal_after_present();
        self.target.restore_first_responder(window);
        Ok(NativeTerminalSurfaceReceipt::from_snapshot(
            layout,
            snapshot,
            rebuilt_rows,
            reused_rows,
            cell_metrics,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DropRecorder {
        name: &'static str,
        log: Arc<Mutex<Vec<&'static str>>>,
    }

    impl Drop for DropRecorder {
        fn drop(&mut self) {
            self.log.lock().push(self.name);
        }
    }

    /// Mirrors the field order of `NativeTerminalSurfaceHost` to statically and
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
                history: format!("\x1b]2;{title}\x07").into_bytes(),
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

        let receipt =
            NativeTerminalSurfaceReceipt::from_snapshot(layout, &snapshot, 4, 20, cell_metrics);

        assert_eq!(receipt.cursor_col, snapshot.cursor.x);
        assert_eq!(receipt.cursor_row, snapshot.cursor.y);
        assert_eq!(receipt.cell_width_px, cell_metrics.width_px);
        assert_eq!(receipt.cell_height_px, cell_metrics.height_px);
        assert_eq!(receipt.rebuilt_rows, 4);
        assert_eq!(receipt.reused_rows, 20);
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
                history: b"Working (esc to interrupt)\r\n".to_vec(),
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
                history: b"Still Working (esc to interrupt)\r\n".to_vec(),
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
                history: b"\x1b[2J\x1b[HAction Required: allow command?\r\npress enter to confirm or esc to cancel\r\n".to_vec(),
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
            history: b"Working (esc to interrupt)\r\n".to_vec(),
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
        state.set_event_sink(Arc::new(move |event| {
            if let NativeTerminalEvent::AgentState(payload) = event {
                observed_for_sink
                    .lock()
                    .push((payload.state, payload.manifest_id));
            }
        }));

        let (tx, messages) = tokio::sync::mpsc::channel(4);
        let attachment = DaemonAttachment {
            session_id: session_id.to_string(),
            epoch: 1,
            start_sequence: Some(1),
            end_sequence: Some(1),
            gap: None,
            history: Vec::new(),
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
        })
        .await
        .expect("send agent state report");

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while observed.lock().is_empty() && std::time::Instant::now() < deadline {
            tokio::task::yield_now().await;
        }
        assert_eq!(
            observed.lock().clone(),
            vec![("blocked".to_string(), "omo".to_string())]
        );

        tx.send(DaemonStreamMessage::Output {
            session_id: session_id.into(),
            sequence: 2,
            data: b"  Working (esc to interrupt)\r\n".to_vec().into(),
            metrics_read_unix_micros: None,
        })
        .await
        .expect("send screen output");

        for _ in 0..200 {
            tokio::task::yield_now().await;
        }
        assert_eq!(
            observed.lock().len(),
            1,
            "screen inference must stay disabled once the agent reports its own state"
        );

        state.teardown();
    }
}
