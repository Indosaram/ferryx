use crate::worktree::WorktreeIdentity;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RemoteTerminalTabInfo {
    #[serde(alias = "tabId")]
    pub id: String,
    #[serde(alias = "tabLabel", alias = "title")]
    pub label: String,
    #[serde(
        default,
        alias = "activity_state",
        alias = "state",
        skip_serializing_if = "Option::is_none"
    )]
    pub activity_state: Option<String>,
    #[serde(default, alias = "agent_type", skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    #[serde(
        default,
        alias = "worktree_slug",
        skip_serializing_if = "Option::is_none"
    )]
    pub worktree_slug: Option<String>,
    #[serde(
        default,
        alias = "worktree_label",
        skip_serializing_if = "Option::is_none"
    )]
    pub worktree_label: Option<String>,
    #[serde(default, alias = "session_id", skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RemoteActiveDesktopSelection {
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub worktree_slug: Option<String>,
    #[serde(default)]
    pub worktree_label: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default, alias = "activeTabId")]
    pub tab_id: Option<String>,
    #[serde(default, alias = "tabs")]
    pub terminal_tabs: Vec<RemoteTerminalTabInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteSelectWorkspaceRequest {
    pub workspace_id: String,
    #[serde(default)]
    pub worktree: Option<WorktreeIdentity>,
    #[serde(default)]
    pub worktree_slug: Option<String>,
    #[serde(default)]
    pub worktree_label: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default, alias = "activeTabId")]
    pub tab_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteSelectionRequestPayload {
    pub workspace_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree: Option<WorktreeIdentity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_slug: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteTerminalSession {
    pub session_id: String,
    pub title: Option<String>,
    pub workspace_id: Option<String>,
    pub worktree_label: Option<String>,
    pub running: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteProjectInfo {
    pub workspace_id: String,
    pub worktrees: Vec<RemoteWorktreeInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RemoteWorktreeInfo {
    pub worktree_slug: Option<String>,
    pub worktree_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attention: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteWorkspaceState {
    pub projects: Vec<RemoteProjectInfo>,
    pub active_context: RemoteActiveDesktopSelection,
    pub active_workspace_id: String,
    pub worktrees: Vec<RemoteWorktreeInfo>,
    pub sessions: Vec<RemoteTerminalSession>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteCreateWorktreeRequest {
    pub workspace_id: String,
    pub worktree: WorktreeIdentity,
    pub base_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteDeleteWorktreeRequest {
    pub workspace_id: String,
    pub worktree: WorktreeIdentity,
    pub delete_branch: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ClientControlMessage {
    Resize { cols: u16, rows: u16 },
    Signal { signal: String },
    Ping,
    Scroll { rows: i16 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ServerControlMessage {
    Pong,
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteEventMessage {
    pub event: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RemoteGridCursorVisualStyle {
    Bar,
    Block,
    Underline,
    BlockHollow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteGridCursor {
    pub x: u16,
    pub y: u16,
    pub visible: bool,
    pub blinking: bool,
    pub wide_tail: bool,
    pub visual_style: RemoteGridCursorVisualStyle,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteGridRun {
    pub text: String,
    pub fg: Option<[u8; 3]>,
    pub bg: Option<[u8; 3]>,
    pub attrs: u8,
    /// Grid columns this run occupies (wide cells count 2). Lets DOM renderers
    /// snap run boundaries to the terminal grid so the cursor overlay aligns
    /// with CJK/Hangul text.
    pub cells: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteGridLine {
    pub index: u16,
    pub runs: Vec<RemoteGridRun>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum RemoteGridFrame {
    #[serde(rename = "grid")]
    Grid {
        cols: u16,
        rows: u16,
        cursor: RemoteGridCursor,
        lines: Vec<RemoteGridLine>,
    },
    #[serde(rename = "gridDiff")]
    GridDiff {
        cols: u16,
        rows: u16,
        cursor: RemoteGridCursor,
        lines: Vec<RemoteGridLine>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_control_message_scroll_serde_roundtrip() {
        let msg = ClientControlMessage::Scroll { rows: 3 };
        let serialized = serde_json::to_string(&msg).expect("serialize");
        assert_eq!(serialized, r#"{"type":"scroll","rows":3}"#);

        let deserialized: ClientControlMessage =
            serde_json::from_str(r#"{"type":"scroll","rows":3}"#).expect("deserialize positive");
        assert_eq!(deserialized, ClientControlMessage::Scroll { rows: 3 });

        let deserialized_neg: ClientControlMessage =
            serde_json::from_str(r#"{"type":"scroll","rows":-5}"#).expect("deserialize negative");
        assert_eq!(deserialized_neg, ClientControlMessage::Scroll { rows: -5 });

        let malformed =
            serde_json::from_str::<ClientControlMessage>(r#"{"type":"scroll","rows":"abc"}"#);
        assert!(malformed.is_err());
    }
}
