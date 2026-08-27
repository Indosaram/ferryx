use serde::{Deserialize, Deserializer, Serialize, Serializer};

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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BrowserProfileId {
    Default,
    Private,
    Named(String),
}

impl BrowserProfileId {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Default => "default",
            Self::Private => "private",
            Self::Named(id) => id.as_str(),
        }
    }

    pub fn from_id(value: &str) -> Option<Self> {
        let trimmed = value.trim();
        match trimmed.to_ascii_lowercase().as_str() {
            "default" => Some(Self::Default),
            "private" => Some(Self::Private),
            _ if is_valid_named_profile_id(trimmed) => Some(Self::Named(trimmed.to_string())),
            _ => None,
        }
    }

    pub fn is_private(&self) -> bool {
        matches!(self, Self::Private)
    }

    pub fn is_named(&self) -> bool {
        matches!(self, Self::Named(_))
    }
}

fn is_valid_named_profile_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 64 || !bytes[0].is_ascii_alphanumeric() {
        return false;
    }
    bytes
        .iter()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'.' | b'_' | b'-'))
}

impl Serialize for BrowserProfileId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for BrowserProfileId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::from_id(&value).ok_or_else(|| serde::de::Error::custom("invalid browser profile id"))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBrowserRequest {
    pub browser_id: Option<String>,
    pub workspace_id: Option<String>,
    pub worktree_path: Option<String>,
    pub url: String,
    pub profile: Option<BrowserProfileId>,
    pub zoom_factor: Option<f64>,
    pub bounds: Option<LogicalRect>,
    pub visible: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportBrowserCookiesRequest {
    pub profile_id: String,
    pub file_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportBrowserCookiesResult {
    pub imported_count: usize,
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
    pub profile_id: BrowserProfileId,
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

impl From<&BrowserState> for BrowserStateChangedPayload {
    fn from(state: &BrowserState) -> Self {
        Self {
            browser_id: state.browser_id.clone(),
            generation: state.generation,
            url: state.url.clone(),
            title: state.title.clone(),
            loading: state.loading,
            can_go_back: state.can_go_back,
            can_go_forward: state.can_go_forward,
            zoom_factor: state.zoom_factor,
            load_error: state.load_error.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserOpenRequestedPayload {
    pub browser_id: String,
    pub target_url: String,
    pub profile_id: BrowserProfileId,
    pub worktree_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserDownloadRequestedPayload {
    pub browser_id: String,
    pub target_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserShortcutRequestedPayload {
    pub browser_id: String,
    pub action: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserFindResult {
    pub match_count: usize,
    pub found: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserAutomationTarget {
    pub reference: String,
    pub selector: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserAutomationElement {
    pub reference: String,
    pub role: String,
    pub name: String,
    pub tag_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserAutomationSnapshot {
    pub browser_id: String,
    pub generation: u64,
    pub url: String,
    pub title: String,
    pub elements: Vec<BrowserAutomationElement>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum BrowserAutomationAction {
    Click { reference: String },
    Fill { reference: String, value: String },
    Keypress { key: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserAutomationRequest {
    pub browser_id: String,
    pub generation: u64,
    pub action: BrowserAutomationAction,
}
