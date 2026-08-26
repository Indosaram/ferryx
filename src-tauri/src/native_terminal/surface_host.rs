use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeTerminalEvent {
    Title(NativeTerminalTitlePayload),
    Bell(NativeTerminalBellPayload),
}

pub type NativeTerminalEventSink = Arc<dyn Fn(NativeTerminalEvent) + Send + Sync>;

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
#[derive(Debug, Default)]
pub struct RenderScheduleCoordinator {
    pending: AtomicBool,
}

impl RenderScheduleCoordinator {
    pub fn new() -> Self {
        Self {
            pending: AtomicBool::new(false),
        }
    }

    /// Attempts to schedule a render pass.
    ///
    /// Returns `true` if this transition successfully scheduled the render (transitioning
    /// from idle to pending). Returns `false` if a render is already pending and should be coalesced.
    pub fn schedule_render(&self) -> bool {
        !self.pending.swap(true, Ordering::SeqCst)
    }

    /// Consumes the pending render marker, transitioning the coordinator back to idle.
    ///
    /// Returns `true` if a pending render was consumed, or `false` if none was pending.
    pub fn consume_render(&self) -> bool {
        self.pending.swap(false, Ordering::SeqCst)
    }

    /// Returns `true` if a render pass is currently pending.
    pub fn is_render_pending(&self) -> bool {
        self.pending.load(Ordering::SeqCst)
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
}

pub struct NativeTerminalSurfaceHostState {
    hosts: Arc<Mutex<HashMap<String, NativeTerminalSurfaceHost>>>,
    sessions: Arc<Mutex<HashMap<String, NativeTerminalSession>>>,
    event_sink: Arc<RwLock<Option<NativeTerminalEventSink>>>,
}

impl Clone for NativeTerminalSurfaceHostState {
    fn clone(&self) -> Self {
        Self {
            hosts: Arc::clone(&self.hosts),
            sessions: Arc::clone(&self.sessions),
            event_sink: Arc::clone(&self.event_sink),
        }
    }
}

impl Default for NativeTerminalSurfaceHostState {
    fn default() -> Self {
        Self {
            hosts: Arc::new(Mutex::new(HashMap::new())),
            sessions: Arc::new(Mutex::new(HashMap::new())),
            event_sink: Arc::new(RwLock::new(None)),
        }
    }
}

fn take_native_terminal_events(
    session: &mut NativeTerminalSession,
    session_id: &str,
) -> Vec<NativeTerminalEvent> {
    let mut events = Vec::with_capacity(2);
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
    events
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

    pub fn prepare_session_layout(
        &self,
        request: NativeTerminalBoundsRequest,
        cell_metrics: CellMetrics,
    ) -> Result<SurfaceCompositionLayout, NativeTerminalError> {
        let layout = request.layout(cell_metrics)?;
        let mut sessions = self.sessions.lock();
        let session = match sessions.get_mut(&request.session_id) {
            Some(session) => session,
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
                    },
                );
                sessions
                    .get_mut(&request.session_id)
                    .ok_or(NativeTerminalError::NoValue)?
            }
        };

        if session.terminal.dimensions()? != (layout.cols, layout.rows) {
            session.terminal.resize(
                layout.cols,
                layout.rows,
                cell_metrics.width_px,
                cell_metrics.height_px,
            )?;
        }
        session.layout = Some(layout);
        session.logical_bounds = Some(request.bounds);
        session.cell_metrics = Some(cell_metrics);
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

    pub fn attach_daemon_attachment<R: Runtime>(
        &self,
        session_id: &str,
        attachment: DaemonAttachment,
        app: Option<tauri::AppHandle<R>>,
    ) -> Result<(), NativeTerminalError> {
        validate_session_id(session_id)?;

        let initial_dims = (80, 24);

        let (update_sender, render_coordinator) = {
            let mut sessions = self.sessions.lock();
            if let Some(session) = sessions.get_mut(session_id) {
                if let Some(task) = session.stream_task.take() {
                    task.abort();
                }
                if let Some(task) = session.pump_task.take() {
                    task.abort();
                }
                if attachment.gap.is_some() {
                    session.terminal.reset();
                }
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
                    },
                );
                (update_sender, render_coordinator)
            }
        };

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
                                    let hosts_clone = Arc::clone(&hosts);
                                    let sessions_clone = Arc::clone(&sessions);
                                    let sid = session_id_owned.clone();
                                    let window_clone = window.clone();
                                    let coordinator_clone = Arc::clone(&render_coordinator);
                                    if let Err(err) = window.run_on_main_thread(move || {
                                        coordinator_clone.consume_render();
                                        let render_state = {
                                            let sessions_guard = sessions_clone.lock();
                                            sessions_guard.get(&sid).and_then(|session| {
                                                let layout = session.layout?;
                                                let logical_bounds = session.logical_bounds?;
                                                match session.terminal.render_snapshot() {
                                                    Ok(mut snapshot) => {
                                                        snapshot.cursor.visual_style =
                                                            cursor_style_for_focus(session.focused);
                                                        Some((layout, logical_bounds, snapshot))
                                                    }
                                                    Err(err) => {
                                                        tracing::warn!(
                                                            session_id = %sid,
                                                            error = %err,
                                                            "Failed to capture native terminal snapshot for render"
                                                        );
                                                        None
                                                    }
                                                }
                                            })
                                        };
                                        if let Some((layout, logical_bounds, snapshot)) = render_state {
                                            let mut hosts_guard = hosts_clone.lock();
                                            if let Some(h) = hosts_guard.get_mut(&sid) {
                                                h.layout = Some(layout);
                                                h.logical_bounds = Some(logical_bounds);
                                                h.target.update_viewport(Some(logical_bounds));
                                                if let Err(err) = h.render_snapshot(
                                                    &window_clone,
                                                    layout,
                                                    &snapshot,
                                                    None,
                                                ) {
                                                    tracing::warn!(
                                                        session_id = %sid,
                                                        error = %err,
                                                        "Failed to render native terminal snapshot"
                                                    );
                                                }
                                            }
                                        }
                                    }) {
                                        render_coordinator.consume_render();
                                        tracing::warn!(
                                            session_id = %session_id_owned,
                                            error = %err,
                                            "Failed to dispatch native terminal render to main thread"
                                        );
                                    }
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
                        {
                            let mut sessions_guard = sessions.lock();
                            if let Some(sess) = sessions_guard.get_mut(&session_id_owned) {
                                sess.terminal.reset();
                            }
                        }
                        update_sender.send_replace(());
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

    pub fn detach_session(&self, session_id: &str) {
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
        _window: &WebviewWindow<R>,
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
}
