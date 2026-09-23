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

impl From<&BrowserState> for BrowserSessionSummary {
    fn from(state: &BrowserState) -> Self {
        Self {
            browser_id: state.browser_id.clone(),
            webview_label: state.webview_label.clone(),
            workspace_id: state.workspace_id.clone(),
            profile_id: state.profile_id.clone(),
            url: state.url.clone(),
            title: state.title.clone(),
            visible: state.visible,
        }
    }
}

impl From<BrowserState> for BrowserSessionSummary {
    fn from(state: BrowserState) -> Self {
        Self::from(&state)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserSessionCreatedPayload {
    pub browser: BrowserState,
    pub workspace_id: Option<String>,
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
pub struct BrowserElementPickedPayload {
    pub browser_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserLinkClickedPayload {
    pub browser_id: String,
    pub target_url: String,
    pub modifier: bool,
    pub profile_id: BrowserProfileId,
    pub worktree_path: Option<String>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserWaitCondition {
    Selector {
        selector: String,
    },
    Text {
        text: String,
    },
    UrlContains {
        fragment: String,
    },
    LoadState {
        state: String,
    },
    Function {
        script: String,
    },
    WithTimeout {
        inner: Box<BrowserWaitCondition>,
        timeout_ms: u64,
    },
}

impl serde::Serialize for BrowserWaitCondition {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;
        match self {
            Self::Selector { selector } => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("condition", "selector")?;
                map.serialize_entry("selector", selector)?;
                map.end()
            }
            Self::Text { text } => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("condition", "text")?;
                map.serialize_entry("text", text)?;
                map.end()
            }
            Self::UrlContains { fragment } => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("condition", "urlContains")?;
                map.serialize_entry("fragment", fragment)?;
                map.end()
            }
            Self::LoadState { state } => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("condition", "loadState")?;
                map.serialize_entry("state", state)?;
                map.end()
            }
            Self::Function { script } => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("condition", "function")?;
                map.serialize_entry("script", script)?;
                map.end()
            }
            Self::WithTimeout { inner, timeout_ms } => {
                let mut val = serde_json::to_value(inner).map_err(serde::ser::Error::custom)?;
                if let Some(obj) = val.as_object_mut() {
                    obj.insert("timeoutMs".into(), serde_json::json!(timeout_ms));
                }
                val.serialize(serializer)
            }
        }
    }
}

impl<'de> serde::Deserialize<'de> for BrowserWaitCondition {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(
            tag = "condition",
            rename_all = "camelCase",
            rename_all_fields = "camelCase"
        )]
        enum TaggedCondition {
            #[serde(rename_all = "camelCase")]
            Selector {
                selector: String,
                #[serde(default, rename = "timeoutMs", alias = "timeout_ms")]
                timeout_ms: Option<u64>,
            },
            #[serde(rename_all = "camelCase")]
            Text {
                text: String,
                #[serde(default, rename = "timeoutMs", alias = "timeout_ms")]
                timeout_ms: Option<u64>,
            },
            #[serde(rename_all = "camelCase")]
            UrlContains {
                fragment: String,
                #[serde(default, rename = "timeoutMs", alias = "timeout_ms")]
                timeout_ms: Option<u64>,
            },
            #[serde(rename_all = "camelCase")]
            LoadState {
                state: String,
                #[serde(default, rename = "timeoutMs", alias = "timeout_ms")]
                timeout_ms: Option<u64>,
            },
            #[serde(rename_all = "camelCase")]
            Function {
                script: String,
                #[serde(default, rename = "timeoutMs", alias = "timeout_ms")]
                timeout_ms: Option<u64>,
            },
        }

        #[derive(Deserialize)]
        #[serde(untagged, rename_all_fields = "camelCase")]
        enum Helper {
            Tagged(TaggedCondition),
            StringScript(String),
            #[serde(rename_all = "camelCase")]
            StringConditionWithTimeout {
                condition: String,
                #[serde(default, rename = "timeoutMs", alias = "timeout_ms")]
                timeout_ms: Option<u64>,
            },
            #[serde(rename_all = "camelCase")]
            UntaggedScript {
                script: String,
                #[serde(default, rename = "timeoutMs", alias = "timeout_ms")]
                timeout_ms: Option<u64>,
            },
            #[serde(rename_all = "camelCase")]
            UntaggedSelector {
                selector: String,
                #[serde(default, rename = "timeoutMs", alias = "timeout_ms")]
                timeout_ms: Option<u64>,
            },
            #[serde(rename_all = "camelCase")]
            UntaggedText {
                text: String,
                #[serde(default, rename = "timeoutMs", alias = "timeout_ms")]
                timeout_ms: Option<u64>,
            },
            #[serde(rename_all = "camelCase")]
            UntaggedUrl {
                fragment: String,
                #[serde(default, rename = "timeoutMs", alias = "timeout_ms")]
                timeout_ms: Option<u64>,
            },
            #[serde(rename_all = "camelCase")]
            UntaggedLoadState {
                state: String,
                #[serde(default, rename = "timeoutMs", alias = "timeout_ms")]
                timeout_ms: Option<u64>,
            },
        }

        fn wrap(base: BrowserWaitCondition, timeout_ms: Option<u64>) -> BrowserWaitCondition {
            match timeout_ms {
                Some(ms) => BrowserWaitCondition::WithTimeout {
                    inner: Box::new(base),
                    timeout_ms: ms,
                },
                None => base,
            }
        }

        match Helper::deserialize(deserializer)? {
            Helper::Tagged(TaggedCondition::Selector {
                selector,
                timeout_ms,
            }) => Ok(wrap(Self::Selector { selector }, timeout_ms)),
            Helper::Tagged(TaggedCondition::Text { text, timeout_ms }) => {
                Ok(wrap(Self::Text { text }, timeout_ms))
            }
            Helper::Tagged(TaggedCondition::UrlContains {
                fragment,
                timeout_ms,
            }) => Ok(wrap(Self::UrlContains { fragment }, timeout_ms)),
            Helper::Tagged(TaggedCondition::LoadState { state, timeout_ms }) => {
                Ok(wrap(Self::LoadState { state }, timeout_ms))
            }
            Helper::Tagged(TaggedCondition::Function { script, timeout_ms }) => {
                Ok(wrap(Self::Function { script }, timeout_ms))
            }
            Helper::StringScript(script) => Ok(Self::Function { script }),
            Helper::StringConditionWithTimeout {
                condition,
                timeout_ms,
            } => Ok(wrap(Self::Function { script: condition }, timeout_ms)),
            Helper::UntaggedScript { script, timeout_ms } => {
                Ok(wrap(Self::Function { script }, timeout_ms))
            }
            Helper::UntaggedSelector {
                selector,
                timeout_ms,
            } => Ok(wrap(Self::Selector { selector }, timeout_ms)),
            Helper::UntaggedText { text, timeout_ms } => Ok(wrap(Self::Text { text }, timeout_ms)),
            Helper::UntaggedUrl {
                fragment,
                timeout_ms,
            } => Ok(wrap(Self::UrlContains { fragment }, timeout_ms)),
            Helper::UntaggedLoadState { state, timeout_ms } => {
                Ok(wrap(Self::LoadState { state }, timeout_ms))
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserConsoleEntry {
    pub level: String,
    pub text: String,
    #[serde(alias = "at_ms")]
    pub at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserCookieEntry {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteSnapshotRecord {
    pub snapshot_id: String,
    pub map_revision: u64,
    pub document_generation: u64,
    pub targets: std::collections::HashMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_r5_15_wait_condition_string_and_tagged_deserialization() {
        // String form (sent by UI)
        let from_str: BrowserWaitCondition =
            serde_json::from_str(r#""document.title !== ''""#).unwrap();
        assert_eq!(
            from_str,
            BrowserWaitCondition::Function {
                script: "document.title !== ''".into()
            }
        );

        // Tagged form
        let from_tagged: BrowserWaitCondition =
            serde_json::from_str(r##"{"condition":"selector","selector":"#ready"}"##).unwrap();
        assert_eq!(
            from_tagged,
            BrowserWaitCondition::Selector {
                selector: "#ready".into()
            }
        );

        // Untagged script object
        let from_script: BrowserWaitCondition =
            serde_json::from_str(r#"{"script":"1 + 1 === 2"}"#).unwrap();
        assert_eq!(
            from_script,
            BrowserWaitCondition::Function {
                script: "1 + 1 === 2".into()
            }
        );
    }

    #[test]
    fn test_r6_13_wait_condition_with_timeout_serde_roundtrip() {
        // Tagged selector condition with camelCase timeoutMs
        let expected = BrowserWaitCondition::WithTimeout {
            inner: Box::new(BrowserWaitCondition::Selector {
                selector: "#ready".into(),
            }),
            timeout_ms: 25,
        };

        // Forward: Deserialization from JSON with camelCase timeoutMs
        let json_input = r##"{"condition":"selector","selector":"#ready","timeoutMs":25}"##;
        let deserialized: BrowserWaitCondition = serde_json::from_str(json_input).unwrap();
        assert_eq!(deserialized, expected);

        // Backward: Serialization produces JSON with camelCase timeoutMs
        let serialized = serde_json::to_string(&expected).unwrap();
        let serialized_value: serde_json::Value = serde_json::from_str(&serialized).unwrap();
        assert_eq!(
            serialized_value,
            serde_json::json!({
                "condition": "selector",
                "selector": "#ready",
                "timeoutMs": 25
            })
        );

        // Round-trip from serialized
        let roundtrip: BrowserWaitCondition = serde_json::from_str(&serialized).unwrap();
        assert_eq!(roundtrip, expected);

        // Tagged other conditions with camelCase timeoutMs
        let cases = vec![
            (
                r#"{"condition":"text","text":"hello","timeoutMs":100}"#,
                BrowserWaitCondition::WithTimeout {
                    inner: Box::new(BrowserWaitCondition::Text {
                        text: "hello".into(),
                    }),
                    timeout_ms: 100,
                },
            ),
            (
                r#"{"condition":"urlContains","fragment":"/auth","timeoutMs":50}"#,
                BrowserWaitCondition::WithTimeout {
                    inner: Box::new(BrowserWaitCondition::UrlContains {
                        fragment: "/auth".into(),
                    }),
                    timeout_ms: 50,
                },
            ),
            (
                r#"{"condition":"loadState","state":"complete","timeoutMs":200}"#,
                BrowserWaitCondition::WithTimeout {
                    inner: Box::new(BrowserWaitCondition::LoadState {
                        state: "complete".into(),
                    }),
                    timeout_ms: 200,
                },
            ),
            (
                r#"{"condition":"function","script":"window.loaded","timeoutMs":300}"#,
                BrowserWaitCondition::WithTimeout {
                    inner: Box::new(BrowserWaitCondition::Function {
                        script: "window.loaded".into(),
                    }),
                    timeout_ms: 300,
                },
            ),
            // Untagged variants with camelCase timeoutMs
            (
                r##"{"selector":"#btn","timeoutMs":500}"##,
                BrowserWaitCondition::WithTimeout {
                    inner: Box::new(BrowserWaitCondition::Selector {
                        selector: "#btn".into(),
                    }),
                    timeout_ms: 500,
                },
            ),
            (
                r#"{"text":"Click here","timeoutMs":400}"#,
                BrowserWaitCondition::WithTimeout {
                    inner: Box::new(BrowserWaitCondition::Text {
                        text: "Click here".into(),
                    }),
                    timeout_ms: 400,
                },
            ),
            (
                r#"{"script":"return true","timeoutMs":600}"#,
                BrowserWaitCondition::WithTimeout {
                    inner: Box::new(BrowserWaitCondition::Function {
                        script: "return true".into(),
                    }),
                    timeout_ms: 600,
                },
            ),
            (
                r#"{"condition":"custom.check()","timeoutMs":700}"#,
                BrowserWaitCondition::WithTimeout {
                    inner: Box::new(BrowserWaitCondition::Function {
                        script: "custom.check()".into(),
                    }),
                    timeout_ms: 700,
                },
            ),
            // Backward compatibility with snake_case timeout_ms
            (
                r##"{"condition":"selector","selector":"#ready","timeout_ms":25}"##,
                BrowserWaitCondition::WithTimeout {
                    inner: Box::new(BrowserWaitCondition::Selector {
                        selector: "#ready".into(),
                    }),
                    timeout_ms: 25,
                },
            ),
        ];

        for (json_str, expected_cond) in cases {
            let parsed: BrowserWaitCondition = serde_json::from_str(json_str).unwrap();
            assert_eq!(parsed, expected_cond, "Failed for JSON: {}", json_str);
            let reserialized = serde_json::to_string(&parsed).unwrap();
            let reparsed: BrowserWaitCondition = serde_json::from_str(&reserialized).unwrap();
            assert_eq!(
                reparsed, expected_cond,
                "Failed round-trip for: {}",
                json_str
            );
        }
    }
}
