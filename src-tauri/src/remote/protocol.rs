use crate::worktree::WorktreeIdentity;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlChallenge {
    pub nonce: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlAuth {
    pub machine_id: String,
    pub display_name: String,
    pub public_key: String,
    pub signature: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlAuthResponse {
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterPairingPin {
    pub pin: String,
    pub pairing_token: String,
    pub machine_id: String,
    pub expires_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterPairingPinAck {
    pub pin: String,
    pub machine_id: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimPairingPin {
    pub pin: Option<String>,
    pub pairing_token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingPinClaimed {
    pub pin: String,
    pub machine_id: String,
    pub claimed_at: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PairingState {
    Created,
    Registering,
    Ready,
    Claimed,
    Consumed,
    Expired,
    Cancelled,
}

impl PairingState {
    /// Checks a transition without mutating state. Callers must atomically apply it.
    pub fn can_transition_to(&self, target: &PairingState) -> bool {
        match target {
            Self::Expired | Self::Cancelled => *self != Self::Consumed,
            _ => matches!(
                (self, target),
                (Self::Created, Self::Registering)
                    | (Self::Registering, Self::Ready)
                    | (Self::Ready, Self::Claimed)
                    | (Self::Claimed, Self::Consumed)
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpRequestFrame {
    pub stream_id: String,
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpResponseFrame {
    pub stream_id: String,
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SocketTicketRequest {
    pub machine_id: String,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SocketTicketResponse {
    pub ticket: String,
    pub expires_at: u64,
}

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteWorktreeAttention {
    pub workspace_id: String,
    pub worktree_slug: Option<String>,
    pub worktree_label: Option<String>,
    pub state: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RemoteActiveDesktopSelection {
    #[serde(default)]
    pub attention_inventory: Vec<RemoteWorktreeAttention>,
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
    RemoteWrite { generation: String, data: String },
    RemoteResize { generation: String, cols: u16, rows: u16 },
    Resize { cols: u16, rows: u16 },
    Signal { signal: String },
    Ping,
    Scroll { rows: i16 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ServerControlMessage {
    RemoteStatus { state: crate::terminal::remote::RemoteConnectionState, generation: String },
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

    fn assert_wire<T>(value: T, expected: serde_json::Value)
    where
        T: Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
    {
        let encoded = serde_json::to_string(&value).expect("serialize wire model");
        assert_eq!(serde_json::from_str::<serde_json::Value>(&encoded).unwrap(), expected);
        assert_eq!(serde_json::from_str::<T>(&encoded).unwrap(), value);
        assert_eq!(serde_json::from_value::<T>(expected).unwrap(), value);
    }

    #[test]
    fn control_handshake_wire_roundtrips() {
        use serde_json::json;
        assert_wire(
            ControlChallenge { nonce: "nonce".into(), timestamp: u64::MAX },
            json!({"nonce": "nonce", "timestamp": u64::MAX}),
        );
        assert_wire(
            ControlAuth {
                machine_id: "machine".into(), display_name: "Desktop".into(),
                public_key: "key".into(), signature: "signature".into(), timestamp: 42,
            },
            json!({"machineId": "machine", "displayName": "Desktop", "publicKey": "key", "signature": "signature", "timestamp": 42}),
        );
        assert_wire(
            ControlAuthResponse { success: true, error: None },
            json!({"success": true, "error": null}),
        );
        assert_wire(
            ControlAuthResponse { success: false, error: Some("invalid signature".into()) },
            json!({"success": false, "error": "invalid signature"}),
        );
    }

    #[test]
    fn pairing_wire_roundtrips() {
        use serde_json::json;
        assert_wire(
            RegisterPairingPin { pin: "001234".into(), pairing_token: "token".into(), machine_id: "machine".into(), expires_at: 100 },
            json!({"pin": "001234", "pairingToken": "token", "machineId": "machine", "expiresAt": 100}),
        );
        assert_wire(
            RegisterPairingPinAck { pin: "001234".into(), machine_id: "machine".into(), status: "ready".into() },
            json!({"pin": "001234", "machineId": "machine", "status": "ready"}),
        );
        for pin in [None, Some("001234".to_owned())] {
            for pairing_token in [None, Some("token".to_owned())] {
                let expected = json!({"pin": pin, "pairingToken": pairing_token});
                assert_wire(ClaimPairingPin { pin: pin.clone(), pairing_token }, expected);
            }
        }
        assert_wire(
            PairingPinClaimed { pin: "001234".into(), machine_id: "machine".into(), claimed_at: 99 },
            json!({"pin": "001234", "machineId": "machine", "claimedAt": 99}),
        );
    }

    #[test]
    fn proxy_wire_roundtrips() {
        use serde_json::json;
        for body in [None, Some(vec![]), Some(vec![0, 127, 128, 255])] {
            let headers = HashMap::from([("content-type".into(), "application/octet-stream".into())]);
            let request_json = json!({"streamId": "stream", "method": "POST", "path": "/api?q=1", "headers": headers, "body": body});
            let response_json = json!({"streamId": "stream", "status": 201, "headers": headers, "body": body});
            assert_wire(
                HttpRequestFrame { stream_id: "stream".into(), method: "POST".into(), path: "/api?q=1".into(), headers: headers.clone(), body: body.clone() },
                request_json,
            );
            assert_wire(
                HttpResponseFrame { stream_id: "stream".into(), status: 201, headers, body },
                response_json,
            );
        }
        assert_wire(
            SocketTicketRequest { machine_id: "machine".into(), target: "/ws".into() },
            json!({"machineId": "machine", "target": "/ws"}),
        );
        assert_wire(
            SocketTicketResponse { ticket: "ticket".into(), expires_at: 100 },
            json!({"ticket": "ticket", "expiresAt": 100}),
        );
    }

    #[test]
    fn omitted_optional_fields_deserialize() {
        use serde_json::json;
        assert_eq!(serde_json::from_value::<ControlAuthResponse>(json!({"success": true})).unwrap().error, None);
        let claim: ClaimPairingPin = serde_json::from_value(json!({})).unwrap();
        assert_eq!(claim, ClaimPairingPin { pin: None, pairing_token: None });
        assert_eq!(serde_json::from_value::<HttpRequestFrame>(json!({"streamId": "s", "method": "GET", "path": "/", "headers": {}})).unwrap().body, None);
        assert_eq!(serde_json::from_value::<HttpResponseFrame>(json!({"streamId": "s", "status": 204, "headers": {}})).unwrap().body, None);
    }

    #[test]
    fn pairing_state_wire_roundtrips() {
        for (state, wire) in [
            (PairingState::Created, "created"),
            (PairingState::Registering, "registering"),
            (PairingState::Ready, "ready"),
            (PairingState::Claimed, "claimed"),
            (PairingState::Consumed, "consumed"),
            (PairingState::Expired, "expired"),
            (PairingState::Cancelled, "cancelled"),
        ] {
            assert_wire(state, serde_json::json!(wire));
        }
    }

    #[test]
    fn pairing_state_transition_matrix() {
        use PairingState::*;
        let states = [Created, Registering, Ready, Claimed, Consumed, Expired, Cancelled];
        // Columns use the same order as `states`; cancellation is allowed from
        // every non-Consumed state, as specified by the wire protocol.
        let allowed = [
            [false, true, false, false, false, true, true],
            [false, false, true, false, false, true, true],
            [false, false, false, true, false, true, true],
            [false, false, false, false, true, true, true],
            [false, false, false, false, false, false, false],
            [false, false, false, false, false, true, true],
            [false, false, false, false, false, true, true],
        ];
        for (row, source) in states.iter().enumerate() {
            for (column, target) in states.iter().enumerate() {
                assert_eq!(source.can_transition_to(target), allowed[row][column], "{source:?} -> {target:?}");
            }
        }
    }

    #[test]
    fn malformed_wire_values_are_rejected() {
        use serde_json::json;
        assert!(serde_json::from_value::<ControlChallenge>(json!({"nonce": "n", "timestamp": -1})).is_err());
        assert!(serde_json::from_value::<SocketTicketRequest>(json!({"machine_id": "m", "target": "/ws"})).is_err());
        assert!(serde_json::from_value::<HttpResponseFrame>(json!({"streamId": "s", "status": 65536, "headers": {}})).is_err());
        assert!(serde_json::from_value::<HttpRequestFrame>(json!({"streamId": "s", "method": "POST", "path": "/", "headers": {}, "body": [256]})).is_err());
        assert!(serde_json::from_value::<PairingState>(json!("unknown")).is_err());
    }

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
