use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogicalRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl LogicalRect {
    pub fn is_valid(&self) -> bool {
        self.width >= 0.0 && self.height >= 0.0 && self.x.is_finite() && self.y.is_finite()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BrowserProfileId {
    Default,
    Private,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBrowserRequest {
    pub workspace_id: Option<String>,
    pub worktree_path: Option<String>,
    pub url: String,
    pub profile: Option<BrowserProfileId>,
    pub bounds: Option<LogicalRect>,
    pub visible: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserState {
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserSessionSummary {
    pub browser_id: String,
    pub webview_label: String,
    pub workspace_id: Option<String>,
    pub url: String,
    pub title: Option<String>,
    pub visible: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserStateChangedPayload {
    pub browser_id: String,
    pub generation: u64,
    pub url: String,
    pub title: Option<String>,
    pub loading: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub zoom_factor: f64,
    pub load_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserOpenRequestedPayload {
    pub browser_id: String,
    pub target_url: String,
}
