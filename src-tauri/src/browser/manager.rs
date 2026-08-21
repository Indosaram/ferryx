use crate::browser::model::*;
use crate::browser::security::{validate_url, BrowserError};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
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
}

#[derive(Default, Clone)]
pub struct BrowserManager {
    sessions: Arc<RwLock<HashMap<String, ManagedBrowserSession>>>,
}

impl BrowserManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn register_session(&self, req: CreateBrowserRequest) -> Result<BrowserState, BrowserError> {
        let valid_url = validate_url(&req.url)?;
        if let Some(ref bounds) = req.bounds {
            if !bounds.is_valid() {
                return Err(BrowserError::InvalidBounds);
            }
        }

        let uuid = Uuid::new_v4().to_string();
        let browser_id = uuid.clone();
        let webview_label = format!("browser-{}", uuid);
        let profile_id = req.profile.unwrap_or(BrowserProfileId::Default);
        let visible = req.visible.unwrap_or(true);

        let session = ManagedBrowserSession {
            browser_id: browser_id.clone(),
            webview_label: webview_label.clone(),
            workspace_id: req.workspace_id,
            worktree_path: req.worktree_path,
            profile_id: profile_id.clone(),
            generation: 1,
            url: valid_url.clone(),
            title: None,
            loading: false,
            can_go_back: false,
            can_go_forward: false,
            zoom_factor: 1.0,
            load_error: None,
            visible,
            bounds: req.bounds,
        };

        let state = BrowserState {
            browser_id: session.browser_id.clone(),
            webview_label: session.webview_label.clone(),
            workspace_id: session.workspace_id.clone(),
            worktree_path: session.worktree_path.clone(),
            profile_id,
            generation: session.generation,
            url: session.url.clone(),
            title: None,
            loading: session.loading,
            can_go_back: session.can_go_back,
            can_go_forward: session.can_go_forward,
            zoom_factor: session.zoom_factor,
            load_error: None,
            visible: session.visible,
        };

        self.sessions.write().insert(browser_id, session);
        Ok(state)
    }

    pub fn get_state(&self, browser_id: &str) -> Result<BrowserState, BrowserError> {
        let guard = self.sessions.read();
        let s = guard
            .get(browser_id)
            .ok_or_else(|| BrowserError::NotFound(browser_id.to_string()))?;

        Ok(BrowserState {
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
        })
    }

    pub fn update_url(&self, browser_id: &str, new_url: &str) -> Result<String, BrowserError> {
        let valid_url = validate_url(new_url)?;
        let mut guard = self.sessions.write();
        let s = guard
            .get_mut(browser_id)
            .ok_or_else(|| BrowserError::NotFound(browser_id.to_string()))?;

        s.url = valid_url.clone();
        s.generation += 1;
        s.loading = true;
        s.load_error = None;
        Ok(valid_url)
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
        Ok(())
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
            s.url = u;
        }
        if let Some(t) = title {
            s.title = Some(t);
        }
        if let Some(l) = loading {
            s.loading = l;
        }
        if let Some(b) = can_go_back {
            s.can_go_back = b;
        }
        if let Some(f) = can_go_forward {
            s.can_go_forward = f;
        }
        s.load_error = error;

        Ok(BrowserState {
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
        })
    }

    pub fn remove_session(&self, browser_id: &str) -> Option<ManagedBrowserSession> {
        self.sessions.write().remove(browser_id)
    }

    pub fn list_sessions(&self) -> Vec<BrowserSessionSummary> {
        let guard = self.sessions.read();
        guard
            .values()
            .map(|s| BrowserSessionSummary {
                browser_id: s.browser_id.clone(),
                webview_label: s.webview_label.clone(),
                workspace_id: s.workspace_id.clone(),
                url: s.url.clone(),
                title: s.title.clone(),
                visible: s.visible,
            })
            .collect()
    }
}
