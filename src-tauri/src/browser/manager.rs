use crate::browser::model::*;
use crate::browser::security::{validate_url, BrowserError};
use crate::browser::bridge::TrustedBridgeKind;
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;

#[derive(Debug)]
pub struct ManagedBrowserSession {
    pub browser_id: String,
    pub webview_label: String,
    pub workspace_id: Option<String>,
    pub worktree_path: Option<String>,
    pub profile_id: BrowserProfileId,
    pub generation: u64,
    pub url: String,
    pub title: Option<String>,
    pub loading: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub zoom_factor: f64,
    pub load_error: Option<String>,
    pub visible: bool,
    pub bounds: Option<LogicalRect>,
    pub automation_targets: HashMap<String, String>,
    pub remote_targets: HashMap<String, String>,
    pub instance_id: String,
    pub viewport_revision: u64,
    pub remote_snapshot_id: Option<String>,
    pub map_revision: u64,
    pub creation_sequence: u64,
    /// Which guest -> host channel the platform installed for this tab.
    pub trusted_bridge: TrustedBridgeKind,
    history: Vec<String>,
    history_index: usize,
    /// Set once the platform reports real engine history flags. The shadow
    /// vector below cannot represent fragment navigations or `pushState`
    /// entries the engine recorded, so once the engine has spoken it owns
    /// `can_go_back`/`can_go_forward` and the vector is only a fallback for
    /// platforms that expose no native flags.
    native_history_flags: bool,
}

fn browser_state(s: &ManagedBrowserSession) -> BrowserState {
    BrowserState {
        browser_id: s.browser_id.clone(),
        webview_label: s.webview_label.clone(),
        workspace_id: s.workspace_id.clone(),
        worktree_path: s.worktree_path.clone(),
        profile_id: s.profile_id.clone(),
        generation: s.generation,
        url: s.url.clone(),
        title: s.title.clone(),
        loading: s.loading,
        can_go_back: s.can_go_back,
        can_go_forward: s.can_go_forward,
        zoom_factor: s.zoom_factor,
        load_error: s.load_error.clone(),
        visible: s.visible,
        trusted_bridge: s.trusted_bridge,
    }
}

fn sync_history_flags(session: &mut ManagedBrowserSession) {
    if session.native_history_flags {
        return;
    }
    session.can_go_back = session.history_index > 0;
    session.can_go_forward = session.history_index + 1 < session.history.len();
}

fn push_history_url(session: &mut ManagedBrowserSession, url: &str) {
    if session
        .history
        .get(session.history_index)
        .is_some_and(|current| current == url)
    {
        sync_history_flags(session);
        return;
    }

    session
        .history
        .truncate(session.history_index.saturating_add(1));
    session.history.push(url.to_string());
    if session.history.len() > HISTORY_ENTRY_CAP {
        // Oldest entries fall off the front; the engine still owns what lies beyond the cap.
        let excess = session.history.len() - HISTORY_ENTRY_CAP;
        session.history.drain(..excess);
    }
    session.history_index = session.history.len().saturating_sub(1);
    sync_history_flags(session);
}

/// Most shadow-history entries kept per tab; a long-lived tab otherwise grows without bound.
pub const HISTORY_ENTRY_CAP: usize = 500;

// Same strict charset as `is_valid_named_profile_id` in model.rs (ASCII
// alphanumeric first byte, then alphanumeric / `.` / `_` / `-`), plus `:` —
// frontend ids are colon-separated (`tab:<uuid>`, `restored-browser:<id>`) and
// `browser_id` never reaches a filesystem path or a webview label (that is
// `browser-{fresh-uuid}`) — with a 256-byte cap instead of 64. Invalid ids
// fall back to the generated UUID.
fn is_valid_browser_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 256 || !bytes[0].is_ascii_alphanumeric() {
        return false;
    }
    bytes
        .iter()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b':' | b'.' | b'_' | b'-'))
}

/// Most native user-initiated records kept per tab (design 4.3).
pub const NATIVE_USER_INITIATED_CAP: usize = 16;
/// At most this many admitted popups per opener tab per POPUP_RATE_WINDOW.
pub const POPUP_RATE_CAP: usize = 5;
pub const POPUP_RATE_WINDOW: std::time::Duration = std::time::Duration::from_secs(1);

/// Per-tab trusted-bridge bookkeeping that never leaves the host.
#[derive(Default)]
struct BridgeRegistry {
    /// One-time element-pick tokens: armed by the host, consumed by an admitted pick.
    armed_pick_tokens: HashMap<String, String>,
    /// Tokens consumed by an admitted pick and not yet read by finish.
    consumed_pick_tokens: HashMap<String, String>,
    /// Native user-initiated navigations / new-window requests, newest last.
    native_user_initiated: HashMap<String, Vec<(String, Instant)>>,
    /// Native guest view pointers (macOS WKWebView) registered by the platform install.
    guest_views: HashMap<String, usize>,
    /// Admitted popup opens per opener within the last POPUP_RATE_WINDOW, oldest first.
    popup_opens: HashMap<String, Vec<Instant>>,
}

#[derive(Default, Clone)]
pub struct BrowserManager {
    sessions: Arc<RwLock<HashMap<String, ManagedBrowserSession>>>,
    creation_counter: Arc<AtomicU64>,
    bridge: Arc<Mutex<BridgeRegistry>>,
    /// Told about state changes the manager makes on its own, outside any webview callback
    /// (today: the trusted bridge kind, which Windows upgrades from a CDP completion).
    state_listener: Arc<RwLock<Option<BrowserStateListener>>>,
}

/// Receives a tab's state after a manager-originated change. Called without manager locks held.
pub type BrowserStateListener = Arc<dyn Fn(&BrowserState) + Send + Sync>;

impl BrowserManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            creation_counter: Arc::new(AtomicU64::new(0)),
            bridge: Arc::new(Mutex::new(BridgeRegistry::default())),
            state_listener: Arc::new(RwLock::new(None)),
        }
    }

    /// Installs the receiver for manager-originated state changes, replacing any earlier one.
    pub fn set_state_listener(&self, listener: BrowserStateListener) {
        *self.state_listener.write() = Some(listener);
    }

    pub fn register_session(
        &self,
        req: CreateBrowserRequest,
    ) -> Result<BrowserState, BrowserError> {
        let valid_url = validate_url(&req.url)?;
        if let Some(ref bounds) = req.bounds {
            if !bounds.is_valid() {
                return Err(BrowserError::InvalidBounds);
            }
        }

        let uuid = Uuid::new_v4().to_string();
        let requested_browser_id = req
            .browser_id
            .as_deref()
            .map(str::trim)
            .filter(|value| is_valid_browser_id(value))
            .map(str::to_string);
        let browser_id = requested_browser_id.unwrap_or_else(|| uuid.clone());
        // One write guard covers the duplicate check and the insert, so two racing
        // creates for the same id cannot both pass the check and replace each other.
        let mut guard = self.sessions.write();
        let slot = match guard.entry(browser_id.clone()) {
            std::collections::hash_map::Entry::Occupied(_) => {
                return Err(BrowserError::Internal(format!(
                    "browser session id already exists: {browser_id}"
                )));
            }
            std::collections::hash_map::Entry::Vacant(slot) => slot,
        };
        let webview_label = format!("browser-{uuid}");
        let profile_id = req.profile.unwrap_or(BrowserProfileId::Default);
        let visible = req.visible.unwrap_or(true);
        let zoom_factor = req.zoom_factor.unwrap_or(1.0).clamp(0.25, 5.0);

        let creation_sequence = self.creation_counter.fetch_add(1, Ordering::SeqCst);
        let session = ManagedBrowserSession {
            browser_id: browser_id.clone(),
            webview_label: webview_label.clone(),
            workspace_id: req.workspace_id,
            worktree_path: req.worktree_path,
            profile_id,
            generation: 1,
            url: valid_url.clone(),
            title: None,
            loading: false,
            can_go_back: false,
            can_go_forward: false,
            zoom_factor,
            load_error: None,
            visible,
            bounds: req.bounds,
            automation_targets: HashMap::new(),
            remote_targets: HashMap::new(),
            instance_id: uuid.clone(),
            viewport_revision: 1,
            remote_snapshot_id: None,
            map_revision: 0,
            creation_sequence,
            trusted_bridge: TrustedBridgeKind::Unavailable,
            history: vec![valid_url],
            history_index: 0,
            native_history_flags: false,
        };

        let state = browser_state(&session);
        slot.insert(session);
        Ok(state)
    }

    pub fn get_state(&self, browser_id: &str) -> Result<BrowserState, BrowserError> {
        let guard = self.sessions.read();
        let s = guard
            .get(browser_id)
            .ok_or_else(|| BrowserError::NotFound(browser_id.to_string()))?;
        Ok(browser_state(s))
    }

    pub fn get_bounds(&self, browser_id: &str) -> Result<Option<LogicalRect>, BrowserError> {
        let guard = self.sessions.read();
        let s = guard
            .get(browser_id)
            .ok_or_else(|| BrowserError::NotFound(browser_id.to_string()))?;
        Ok(s.bounds.clone())
    }

    pub fn is_visible(&self, browser_id: &str) -> Result<bool, BrowserError> {
        let guard = self.sessions.read();
        let s = guard
            .get(browser_id)
            .ok_or_else(|| BrowserError::NotFound(browser_id.to_string()))?;
        Ok(s.visible)
    }

    pub fn get_webview_label(&self, browser_id: &str) -> Result<String, BrowserError> {
        let guard = self.sessions.read();
        guard
            .get(browser_id)
            .map(|session| session.webview_label.clone())
            .ok_or_else(|| BrowserError::NotFound(browser_id.to_string()))
    }

    pub fn webview_labels_for_profile(&self, profile_id: &BrowserProfileId) -> Vec<String> {
        self.sessions
            .read()
            .values()
            .filter(|session| &session.profile_id == profile_id)
            .map(|session| session.webview_label.clone())
            .collect()
    }

    pub fn update_url(&self, browser_id: &str, new_url: &str) -> Result<String, BrowserError> {
        self.update_url_with_context(
            browser_id,
            new_url,
            crate::browser::security::BrowserSecurityContext::Local,
        )
    }

    /// Validates `new_url` under `context` (remote callers must pass
    /// `BrowserSecurityContext::Remote`) and records it as the session's URL.
    pub fn update_url_with_context(
        &self,
        browser_id: &str,
        new_url: &str,
        context: crate::browser::security::BrowserSecurityContext,
    ) -> Result<String, BrowserError> {
        let valid_url = crate::browser::security::validate_url_with_context(new_url, context)?;
        let mut guard = self.sessions.write();
        let s = guard
            .get_mut(browser_id)
            .ok_or_else(|| BrowserError::NotFound(browser_id.to_string()))?;

        if s.url != valid_url {
            push_history_url(s, &valid_url);
            s.url = valid_url.clone();
        }
        s.generation += 1;
        s.automation_targets.clear();
        s.remote_targets.clear();
        s.remote_snapshot_id = None;
        s.loading = true;
        s.load_error = None;
        Ok(valid_url)
    }

    pub fn begin_reload(&self, browser_id: &str) -> Result<BrowserState, BrowserError> {
        let mut guard = self.sessions.write();
        let s = guard
            .get_mut(browser_id)
            .ok_or_else(|| BrowserError::NotFound(browser_id.to_string()))?;
        s.generation += 1;
        s.automation_targets.clear();
        s.remote_targets.clear();
        s.remote_snapshot_id = None;
        s.loading = true;
        s.load_error = None;
        sync_history_flags(s);
        Ok(browser_state(s))
    }

    pub fn begin_history_navigation(
        &self,
        browser_id: &str,
        forward: bool,
    ) -> Result<BrowserState, BrowserError> {
        self.begin_history_dispatch(browser_id, forward)
            .map(|(state, _dispatch)| state)
    }

    /// Like `begin_history_navigation`, plus whether the platform history call
    /// (`goBack()`/`history.back()`) must be dispatched. Dispatch is reported
    /// separately from `loading`: off macOS an engine-only entry is dispatched
    /// without a pending load, because a no-op `history.back()` fires no event
    /// that would ever clear one.
    pub fn begin_history_dispatch(
        &self,
        browser_id: &str,
        forward: bool,
    ) -> Result<(BrowserState, bool), BrowserError> {
        self.begin_history_dispatch_with_engine_fallback(
            browser_id,
            forward,
            cfg!(not(target_os = "macos")),
        )
    }

    /// `engine_resolves_unrecorded_history`: whether the platform dispatch
    /// (`history.back()`/`history.forward()` off macOS) must run even when the
    /// shadow vector has no entry in that direction. Same-document navigations
    /// (`pushState`, fragments) never reach the vector, so without native
    /// history flags only the engine knows whether the move is possible.
    fn begin_history_dispatch_with_engine_fallback(
        &self,
        browser_id: &str,
        forward: bool,
        engine_resolves_unrecorded_history: bool,
    ) -> Result<(BrowserState, bool), BrowserError> {
        let mut guard = self.sessions.write();
        let s = guard
            .get_mut(browser_id)
            .ok_or_else(|| BrowserError::NotFound(browser_id.to_string()))?;

        if s.native_history_flags {
            // The engine already decided whether this move is possible. Guessing
            // from the shadow vector here used to abort the navigation before the
            // platform code could run `goBack()`, which stranded every entry the
            // vector never saw. The real URL arrives with the load callback, so
            // nothing is updated optimistically.
            let can_navigate = if forward {
                s.can_go_forward
            } else {
                s.can_go_back
            };
            if !can_navigate {
                s.loading = false;
                return Ok((browser_state(s), false));
            }
            // Same-URL history entries can replace the document/state without
            // a URL change. Invalidate the snapshot when dispatch begins.
            s.generation += 1;
            s.automation_targets.clear();
            s.remote_targets.clear();
            s.remote_snapshot_id = None;
            s.loading = true;
            s.load_error = None;
            return Ok((browser_state(s), true));
        }

        let next_index = if forward {
            (s.history_index + 1 < s.history.len()).then_some(s.history_index + 1)
        } else {
            s.history_index.checked_sub(1)
        };

        let Some(next_index) = next_index else {
            s.loading = false;
            sync_history_flags(s);
            if !engine_resolves_unrecorded_history {
                return Ok((browser_state(s), false));
            }
            // pushState/fragment entries never reach the shadow vector, so only the
            // engine knows whether this move exists. Dispatch it without guessing a
            // URL; a real move reports its URL through the bridge. The snapshot is
            // invalidated because the document state may change in place.
            s.generation += 1;
            s.automation_targets.clear();
            s.remote_targets.clear();
            s.remote_snapshot_id = None;
            return Ok((browser_state(s), true));
        };

        s.history_index = next_index;
        s.url = s.history[next_index].clone();
        s.generation += 1;
        s.automation_targets.clear();
        s.remote_targets.clear();
        s.remote_snapshot_id = None;
        s.loading = true;
        s.load_error = None;
        sync_history_flags(s);
        Ok((browser_state(s), true))
    }

    pub fn cancel_history_navigation(
        &self,
        browser_id: &str,
        forward: bool,
    ) -> Result<BrowserState, BrowserError> {
        let mut guard = self.sessions.write();
        let s = guard
            .get_mut(browser_id)
            .ok_or_else(|| BrowserError::NotFound(browser_id.to_string()))?;
        if s.native_history_flags {
            // Nothing was moved optimistically under engine-owned history, so the
            // cancel path only has to clear the pending load.
            s.loading = false;
            return Ok(browser_state(s));
        }

        let restored_index = if forward {
            s.history_index.checked_sub(1)
        } else {
            (s.history_index + 1 < s.history.len()).then_some(s.history_index + 1)
        };
        if let Some(index) = restored_index {
            s.history_index = index;
            s.url = s.history[index].clone();
        }
        s.loading = false;
        sync_history_flags(s);
        Ok(browser_state(s))
    }

    pub fn set_bounds(&self, browser_id: &str, bounds: LogicalRect) -> Result<(), BrowserError> {
        if !bounds.is_valid() {
            return Err(BrowserError::InvalidBounds);
        }
        let mut guard = self.sessions.write();
        let s = guard
            .get_mut(browser_id)
            .ok_or_else(|| BrowserError::NotFound(browser_id.to_string()))?;
        s.bounds = Some(bounds);
        s.viewport_revision += 1;
        Ok(())
    }

    pub fn record_automation_targets(
        &self,
        browser_id: &str,
        generation: u64,
        targets: Vec<BrowserAutomationTarget>,
    ) -> Result<(), BrowserError> {
        let mut guard = self.sessions.write();
        let session = guard
            .get_mut(browser_id)
            .ok_or_else(|| BrowserError::NotFound(browser_id.to_string()))?;
        if session.generation != generation {
            return Err(BrowserError::AutomationSnapshotStale);
        }
        session.automation_targets = targets
            .into_iter()
            .map(|target| (target.reference, target.selector))
            .collect();
        Ok(())
    }

    pub fn automation_target(
        &self,
        browser_id: &str,
        generation: u64,
        reference: &str,
    ) -> Result<String, BrowserError> {
        let guard = self.sessions.read();
        let session = guard
            .get(browser_id)
            .ok_or_else(|| BrowserError::NotFound(browser_id.to_string()))?;
        if session.generation != generation {
            return Err(BrowserError::AutomationSnapshotStale);
        }
        session
            .automation_targets
            .get(reference)
            .cloned()
            .ok_or_else(|| BrowserError::AutomationTargetNotFound(reference.to_string()))
    }

    pub fn assert_automation_generation(
        &self,
        browser_id: &str,
        generation: u64,
    ) -> Result<(), BrowserError> {
        let current = self.get_state(browser_id)?;
        if current.generation != generation {
            return Err(BrowserError::AutomationSnapshotStale);
        }
        Ok(())
    }

    pub fn get_instance_id(&self, browser_id: &str) -> Result<String, BrowserError> {
        let guard = self.sessions.read();
        let s = guard
            .get(browser_id)
            .ok_or_else(|| BrowserError::NotFound(browser_id.to_string()))?;
        Ok(s.instance_id.clone())
    }

    pub fn get_viewport_revision(&self, browser_id: &str) -> Result<u64, BrowserError> {
        let guard = self.sessions.read();
        let s = guard
            .get(browser_id)
            .ok_or_else(|| BrowserError::NotFound(browser_id.to_string()))?;
        Ok(s.viewport_revision)
    }

    pub fn record_remote_snapshot(
        &self,
        browser_id: &str,
        generation: u64,
        targets: Vec<BrowserAutomationTarget>,
    ) -> Result<(String, u64), BrowserError> {
        let mut guard = self.sessions.write();
        let session = guard
            .get_mut(browser_id)
            .ok_or_else(|| BrowserError::NotFound(browser_id.to_string()))?;
        if session.generation != generation {
            return Err(BrowserError::AutomationSnapshotStale);
        }
        let fresh_snapshot_id = Uuid::new_v4().to_string();
        session.remote_snapshot_id = Some(fresh_snapshot_id.clone());
        session.map_revision += 1;
        session.remote_targets = targets
            .into_iter()
            .map(|target| (target.reference, target.selector))
            .collect();
        Ok((fresh_snapshot_id, session.map_revision))
    }

    pub fn verify_remote_target(
        &self,
        browser_id: &str,
        snapshot_id: &str,
        map_revision: u64,
        reference: &str,
    ) -> Result<String, BrowserError> {
        let guard = self.sessions.read();
        let session = guard
            .get(browser_id)
            .ok_or_else(|| BrowserError::NotFound(browser_id.to_string()))?;
        if snapshot_id.trim().is_empty()
            || session.remote_snapshot_id.as_deref() != Some(snapshot_id)
            || session.map_revision != map_revision
        {
            return Err(BrowserError::AutomationSnapshotStale);
        }
        session
            .remote_targets
            .get(reference)
            .cloned()
            .ok_or_else(|| BrowserError::AutomationTargetNotFound(reference.to_string()))
    }

    pub fn get_geometry(
        &self,
        browser_id: &str,
    ) -> Result<(Option<LogicalRect>, f64, u64), BrowserError> {
        let guard = self.sessions.read();
        let s = guard
            .get(browser_id)
            .ok_or_else(|| BrowserError::NotFound(browser_id.to_string()))?;
        Ok((s.bounds.clone(), s.zoom_factor, s.viewport_revision))
    }

    pub fn set_visible(&self, browser_id: &str, visible: bool) -> Result<(), BrowserError> {
        let mut guard = self.sessions.write();
        let s = guard
            .get_mut(browser_id)
            .ok_or_else(|| BrowserError::NotFound(browser_id.to_string()))?;
        s.visible = visible;
        Ok(())
    }

    pub fn set_zoom(&self, browser_id: &str, zoom: f64) -> Result<f64, BrowserError> {
        let clamped = zoom.clamp(0.25, 5.0);
        let mut guard = self.sessions.write();
        let s = guard
            .get_mut(browser_id)
            .ok_or_else(|| BrowserError::NotFound(browser_id.to_string()))?;
        s.zoom_factor = clamped;
        s.viewport_revision += 1;
        Ok(clamped)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_navigation_state(
        &self,
        browser_id: &str,
        url: Option<String>,
        title: Option<String>,
        loading: Option<bool>,
        can_go_back: Option<bool>,
        can_go_forward: Option<bool>,
        error: Option<String>,
    ) -> Result<BrowserState, BrowserError> {
        let mut guard = self.sessions.write();
        let s = guard
            .get_mut(browser_id)
            .ok_or_else(|| BrowserError::NotFound(browser_id.to_string()))?;

        if let Some(u) = url {
            if s.url != u {
                push_history_url(s, &u);
                s.url = u;
                s.generation += 1;
                s.automation_targets.clear();
                s.remote_targets.clear();
                s.remote_snapshot_id = None;
                s.map_revision += 1;
                s.load_error = None;
            }
        }
        if let Some(t) = title {
            s.title = Some(t);
        }
        if let Some(l) = loading {
            s.loading = l;
        }
        if let Some(error) = error {
            s.load_error = Some(error);
        } else if loading == Some(false) {
            s.load_error = None;
        }
        if let Some(value) = can_go_back {
            s.can_go_back = value;
            s.native_history_flags = true;
        }
        if let Some(value) = can_go_forward {
            s.can_go_forward = value;
            s.native_history_flags = true;
        }
        sync_history_flags(s);

        Ok(browser_state(s))
    }

    pub fn has_sessions(&self) -> bool {
        !self.sessions.read().is_empty()
    }

    pub fn remove_session(&self, browser_id: &str) -> Option<ManagedBrowserSession> {
        {
            let mut bridge = self.bridge.lock();
            bridge.armed_pick_tokens.remove(browser_id);
            bridge.consumed_pick_tokens.remove(browser_id);
            bridge.native_user_initiated.remove(browser_id);
            bridge.guest_views.remove(browser_id);
            bridge.popup_opens.remove(browser_id);
        }
        let removed = self.sessions.write().remove(browser_id);
        #[cfg(all(feature = "desktop", target_os = "windows"))]
        if let Some(session) = removed.as_ref() {
            crate::browser::bridge::windows::forget_webview(&session.webview_label);
        }
        removed
    }

    /// Host-side popup cap: records and admits an open for `opener` at `now` unless it
    /// already opened POPUP_RATE_CAP popups within POPUP_RATE_WINDOW.
    pub fn admit_popup_open(&self, opener: &str, now: Instant) -> bool {
        let mut bridge = self.bridge.lock();
        let opens = bridge.popup_opens.entry(opener.to_string()).or_default();
        opens.retain(|at| {
            now.checked_duration_since(*at)
                .is_none_or(|age| age < POPUP_RATE_WINDOW)
        });
        if opens.len() >= POPUP_RATE_CAP {
            return false;
        }
        opens.push(now);
        true
    }

    /// Records the channel kind the platform install achieved for this tab.
    pub fn set_trusted_bridge(
        &self,
        browser_id: &str,
        kind: TrustedBridgeKind,
    ) -> Result<BrowserState, BrowserError> {
        let (state, changed) = {
            let mut guard = self.sessions.write();
            let s = guard
                .get_mut(browser_id)
                .ok_or_else(|| BrowserError::NotFound(browser_id.to_string()))?;
            let changed = s.trusted_bridge != kind;
            s.trusted_bridge = kind;
            (browser_state(s), changed)
        };
        // The pick capability depends on this kind and it can change long after creation
        // (Windows upgrades it from the async leak probe), so the UI must hear about it.
        if changed {
            let listener = self.state_listener.read().clone();
            if let Some(listener) = listener {
                listener(&state);
            }
        }
        Ok(state)
    }

    /// Arms a fresh one-time pick token for this tab, replacing any earlier one.
    pub fn arm_pick_token(&self, browser_id: &str) -> String {
        let token = Uuid::new_v4().to_string();
        let mut bridge = self.bridge.lock();
        bridge.consumed_pick_tokens.remove(browser_id);
        bridge
            .armed_pick_tokens
            .insert(browser_id.to_string(), token.clone());
        token
    }

    pub fn armed_pick_token(&self, browser_id: &str) -> Option<String> {
        self.bridge.lock().armed_pick_tokens.get(browser_id).cloned()
    }

    /// Moves the armed token to consumed if it equals `token`. One arm yields one pick.
    pub fn consume_pick_token(&self, browser_id: &str, token: &str) -> bool {
        let mut bridge = self.bridge.lock();
        if bridge.armed_pick_tokens.get(browser_id).map(String::as_str) != Some(token) {
            return false;
        }
        bridge.armed_pick_tokens.remove(browser_id);
        bridge
            .consumed_pick_tokens
            .insert(browser_id.to_string(), token.to_string());
        true
    }

    /// Takes the consumed token for finish; None means no admitted pick is pending.
    pub fn take_consumed_pick_token(&self, browser_id: &str) -> Option<String> {
        self.bridge.lock().consumed_pick_tokens.remove(browser_id)
    }

    /// Drops both the armed and the consumed token (picker removed).
    pub fn clear_pick_tokens(&self, browser_id: &str) {
        let mut bridge = self.bridge.lock();
        bridge.armed_pick_tokens.remove(browser_id);
        bridge.consumed_pick_tokens.remove(browser_id);
    }

    /// Records a native user-initiated navigation or new-window request for `url`.
    /// Called only by platform code observing a native event (the Windows step), never
    /// with page-supplied data alone.
    pub fn record_native_user_initiated(&self, browser_id: &str, url: &str, at: Instant) {
        let mut bridge = self.bridge.lock();
        let ring = bridge
            .native_user_initiated
            .entry(browser_id.to_string())
            .or_default();
        ring.push((url.to_string(), at));
        if ring.len() > NATIVE_USER_INITIATED_CAP {
            let excess = ring.len() - NATIVE_USER_INITIATED_CAP;
            ring.drain(..excess);
        }
    }

    pub fn recent_native_user_initiated(&self, browser_id: &str) -> Vec<(String, Instant)> {
        self.bridge
            .lock()
            .native_user_initiated
            .get(browser_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Registers the native guest view pointer (macOS WKWebView) for key attribution.
    /// Called from the platform install inside `with_webview`.
    pub fn register_guest_view(&self, browser_id: &str, ptr: usize) {
        self.bridge
            .lock()
            .guest_views
            .insert(browser_id.to_string(), ptr);
    }

    /// The browser whose registered guest view pointer is `ptr`.
    pub fn browser_for_guest_view(&self, ptr: usize) -> Option<String> {
        self.bridge
            .lock()
            .guest_views
            .iter()
            .find(|(_, registered)| **registered == ptr)
            .map(|(browser_id, _)| browser_id.clone())
    }

    /// Cheap pre-check for key monitors: no registered guest view means no guest focus.
    pub fn has_guest_views(&self) -> bool {
        !self.bridge.lock().guest_views.is_empty()
    }

    pub fn list_sessions(&self) -> Vec<BrowserSessionSummary> {
        let guard = self.sessions.read();
        order_sessions_by_creation(
            guard
                .values()
                .map(|s| {
                    (
                        s.creation_sequence,
                        BrowserSessionSummary {
                            browser_id: s.browser_id.clone(),
                            webview_label: s.webview_label.clone(),
                            workspace_id: s.workspace_id.clone(),
                            profile_id: s.profile_id.clone(),
                            url: s.url.clone(),
                            title: s.title.clone(),
                            visible: s.visible,
                        },
                    )
                })
                .collect(),
        )
    }
}

/// Orders browser sessions by creation sequence.
///
/// `tab list` indices are the CLI's index space for `tab switch` and `tab close`, so they must be
/// stable between calls; a `HashMap` walk is not.
pub fn order_sessions_by_creation(
    sessions: Vec<(u64, BrowserSessionSummary)>,
) -> Vec<BrowserSessionSummary> {
    let mut ordered = sessions;
    ordered.sort_by_key(|(sequence, _)| *sequence);
    ordered
        .into_iter()
        .map(|(_, session)| session)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shadow_history_is_capped_and_keeps_the_newest_entries() {
        let manager = BrowserManager::new();
        manager.register_session(create_request("history-cap")).unwrap();
        for index in 0..(HISTORY_ENTRY_CAP + 25) {
            manager
                .update_url("history-cap", &format!("https://example.com/page-{index}"))
                .unwrap();
        }
        let guard = manager.sessions.read();
        let session = guard.get("history-cap").unwrap();
        assert_eq!(session.history.len(), HISTORY_ENTRY_CAP);
        assert_eq!(session.history_index, HISTORY_ENTRY_CAP - 1);
        assert_eq!(
            session.history.last().map(String::as_str),
            Some(format!("https://example.com/page-{}", HISTORY_ENTRY_CAP + 24).as_str())
        );
        assert!(session.can_go_back && !session.can_go_forward);
    }

    #[test]
    fn popup_opens_are_capped_per_opener_per_second() {
        let manager = BrowserManager::new();
        let start = Instant::now();
        for index in 0..POPUP_RATE_CAP {
            assert!(manager.admit_popup_open("opener-a", start), "open {index} must be admitted");
        }
        assert!(!manager.admit_popup_open("opener-a", start));
        assert!(!manager.admit_popup_open("opener-a", start + std::time::Duration::from_millis(999)));
        // Another opener has its own budget.
        assert!(manager.admit_popup_open("opener-b", start));
        // Once the window has passed, opens are admitted again.
        assert!(manager.admit_popup_open("opener-a", start + POPUP_RATE_WINDOW));
        // Removing the session clears its budget.
        manager.remove_session("opener-b");
        for _ in 0..POPUP_RATE_CAP {
            assert!(manager.admit_popup_open("opener-b", start));
        }
    }

    #[test]
    fn pick_tokens_are_one_time_and_cleared_with_the_session() {
        let manager = BrowserManager::new();
        let state = manager.register_session(create_request("pick-b")).unwrap();
        assert_eq!(state.trusted_bridge, TrustedBridgeKind::Unavailable);
        let token = manager.arm_pick_token("pick-b");
        assert_eq!(manager.armed_pick_token("pick-b").as_deref(), Some(token.as_str()));
        assert!(!manager.consume_pick_token("pick-b", "guess"));
        assert!(manager.consume_pick_token("pick-b", &token));
        // Consumed: a replay finds nothing armed.
        assert!(!manager.consume_pick_token("pick-b", &token));
        assert_eq!(manager.armed_pick_token("pick-b"), None);
        assert_eq!(manager.take_consumed_pick_token("pick-b").as_deref(), Some(token.as_str()));
        assert_eq!(manager.take_consumed_pick_token("pick-b"), None);
        // Re-arming replaces; removal clears everything.
        let first = manager.arm_pick_token("pick-b");
        let second = manager.arm_pick_token("pick-b");
        assert_ne!(first, second);
        assert!(!manager.consume_pick_token("pick-b", &first));
        manager.clear_pick_tokens("pick-b");
        assert_eq!(manager.armed_pick_token("pick-b"), None);
        manager.arm_pick_token("pick-b");
        manager.remove_session("pick-b");
        assert_eq!(manager.armed_pick_token("pick-b"), None);
    }

    #[test]
    fn native_user_initiated_ring_is_capped_per_browser() {
        let manager = BrowserManager::new();
        let now = Instant::now();
        for index in 0..(NATIVE_USER_INITIATED_CAP + 5) {
            manager.record_native_user_initiated("ring-a", &format!("https://e.com/{index}"), now);
        }
        manager.record_native_user_initiated("ring-b", "https://other.com/", now);
        let ring = manager.recent_native_user_initiated("ring-a");
        assert_eq!(ring.len(), NATIVE_USER_INITIATED_CAP);
        assert_eq!(ring.first().map(|(url, _)| url.as_str()), Some("https://e.com/5"));
        assert_eq!(
            ring.last().map(|(url, _)| url.clone()),
            Some(format!("https://e.com/{}", NATIVE_USER_INITIATED_CAP + 4))
        );
        assert_eq!(manager.recent_native_user_initiated("ring-b").len(), 1);
        assert!(manager.recent_native_user_initiated("ring-none").is_empty());
    }

    #[test]
    fn guest_views_resolve_to_their_browser_and_trusted_bridge_is_recorded() {
        let manager = BrowserManager::new();
        manager.register_session(create_request("view-b")).unwrap();
        assert!(!manager.has_guest_views());
        manager.register_guest_view("view-b", 0x1234);
        assert!(manager.has_guest_views());
        assert_eq!(manager.browser_for_guest_view(0x1234).as_deref(), Some("view-b"));
        assert_eq!(manager.browser_for_guest_view(0x9999), None);
        let state = manager
            .set_trusted_bridge("view-b", TrustedBridgeKind::Isolated)
            .unwrap();
        assert_eq!(state.trusted_bridge, TrustedBridgeKind::Isolated);
        assert!(manager.set_trusted_bridge("missing", TrustedBridgeKind::Isolated).is_err());
        manager.remove_session("view-b");
        assert_eq!(manager.browser_for_guest_view(0x1234), None);
        assert!(!manager.has_guest_views());
    }

    fn create_request(browser_id: &str) -> CreateBrowserRequest {
        CreateBrowserRequest {
            browser_id: Some(browser_id.to_string()),
            workspace_id: None,
            worktree_path: None,
            url: "https://example.com/".to_string(),
            profile: None,
            zoom_factor: None,
            bounds: None,
            visible: Some(true),
        }
    }

    fn summary(browser_id: &str) -> BrowserSessionSummary {
        BrowserSessionSummary {
            browser_id: browser_id.to_string(),
            webview_label: format!("browser-{browser_id}"),
            workspace_id: None,
            profile_id: BrowserProfileId::Default,
            url: "https://example.com/".to_string(),
            title: None,
            visible: true,
        }
    }

    #[test]
    fn concurrent_register_with_same_id_admits_exactly_one_session() {
        // A-CORE-7: the duplicate-id check and the insert must be one atomic step, so
        // racing creates for the same frontend id never replace a live session.
        const THREADS: usize = 32;
        for round in 0..20 {
            let manager = BrowserManager::new();
            let browser_id = format!("restored-browser:race-{round}");
            let barrier = Arc::new(std::sync::Barrier::new(THREADS));
            let handles: Vec<_> = (0..THREADS)
                .map(|_| {
                    let manager = manager.clone();
                    let barrier = Arc::clone(&barrier);
                    let request = create_request(&browser_id);
                    std::thread::spawn(move || {
                        barrier.wait();
                        manager.register_session(request)
                    })
                })
                .collect();
            let results: Vec<_> = handles
                .into_iter()
                .map(|handle| handle.join().expect("register thread"))
                .collect();
            let ok_count = results.iter().filter(|result| result.is_ok()).count();
            assert_eq!(ok_count, 1, "round {round}: exactly one create may win");
            for result in results.iter().filter(|result| result.is_err()) {
                assert!(matches!(
                    result,
                    Err(BrowserError::Internal(message)) if message.contains("browser session id already exists")
                ));
            }
            assert_eq!(manager.sessions.read().len(), 1);
        }
    }

    #[test]
    fn history_without_native_flags_dispatches_unrecorded_engine_entries() {
        // A-CORE-8: a pushState/hash entry never reaches the shadow vector, so with no
        // native flags the engine (history.back()/forward()) must still be asked.
        let manager = BrowserManager::new();
        let state = manager
            .register_session(create_request("spa-history"))
            .expect("register");
        assert!(!state.can_go_back);

        let (back, dispatch_back) = manager
            .begin_history_dispatch_with_engine_fallback("spa-history", false, true)
            .expect("begin back");
        assert!(dispatch_back, "back must be dispatched to the engine");
        assert!(!back.loading, "a possibly no-op engine move must not leave a stuck load");
        assert_eq!(back.url, state.url, "no shadow entry, so the URL is not guessed");
        assert!(back.generation > state.generation);

        let (forward, dispatch_forward) = manager
            .begin_history_dispatch_with_engine_fallback("spa-history", true, true)
            .expect("begin forward");
        assert!(dispatch_forward, "forward must be dispatched to the engine");
        assert!(!forward.loading);
        assert!(forward.generation > back.generation);

        // macOS keeps vetoing from the shadow vector until WebKit reports flags.
        let vetoed_manager = BrowserManager::new();
        vetoed_manager
            .register_session(create_request("mac-history"))
            .expect("register");
        let (vetoed, dispatch_vetoed) = vetoed_manager
            .begin_history_dispatch_with_engine_fallback("mac-history", false, false)
            .expect("begin back");
        assert!(!dispatch_vetoed);
        assert!(!vetoed.loading);
    }

    #[test]
    fn list_sessions_follows_creation_order_not_hash_order() {
        // Given three sessions created in an order that no id sort would reproduce, when the
        // manager lists them, then the CLI sees them in creation order.
        let manager = BrowserManager::new();
        for browser_id in ["zzz-first", "aaa-second", "mmm-third"] {
            manager
                .register_session(create_request(browser_id))
                .expect("register session");
        }

        let listed: Vec<String> = manager
            .list_sessions()
            .into_iter()
            .map(|session| session.browser_id)
            .collect();

        assert_eq!(listed, vec!["zzz-first", "aaa-second", "mmm-third"]);
    }

    #[test]
    fn list_sessions_is_stable_across_repeated_calls() {
        // Given several sessions, when the list is taken twice, then the index space the CLI's
        // `tab switch` relies on is identical both times.
        let manager = BrowserManager::new();
        for browser_id in ["b-1", "b-2", "b-3", "b-4", "b-5"] {
            manager
                .register_session(create_request(browser_id))
                .expect("register session");
        }

        let first: Vec<String> = manager
            .list_sessions()
            .into_iter()
            .map(|session| session.browser_id)
            .collect();
        let second: Vec<String> = manager
            .list_sessions()
            .into_iter()
            .map(|session| session.browser_id)
            .collect();

        assert_eq!(first, second);
        assert_eq!(first, vec!["b-1", "b-2", "b-3", "b-4", "b-5"]);
    }

    #[test]
    fn order_sessions_by_creation_sorts_an_arbitrary_sequence_set() {
        // Given sessions paired with out-of-order sequences, when they are ordered, then the
        // result follows the sequences rather than the input positions.
        let ordered = order_sessions_by_creation(vec![
            (7, summary("third")),
            (1, summary("first")),
            (4, summary("second")),
        ]);

        assert_eq!(
            ordered
                .into_iter()
                .map(|session| session.browser_id)
                .collect::<Vec<_>>(),
            vec!["first", "second", "third"]
        );
    }

    #[test]
    fn creation_sequence_increases_with_every_registered_session() {
        // Given two sessions, when both are registered, then the second holds a strictly larger
        // creation sequence so the ordering cannot tie.
        let manager = BrowserManager::new();
        let first = manager
            .register_session(create_request("seq-first"))
            .expect("register first session");
        let second = manager
            .register_session(create_request("seq-second"))
            .expect("register second session");

        let sequences: Vec<u64> = manager
            .list_sessions()
            .into_iter()
            .map(|session| {
                manager
                    .sessions
                    .read()
                    .get(&session.browser_id)
                    .expect("managed session")
                    .creation_sequence
            })
            .collect();

        assert_eq!(sequences.len(), 2);
        assert!(sequences[0] < sequences[1]);
        assert_ne!(first.browser_id, second.browser_id);
    }
}
