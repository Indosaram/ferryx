use crate::remote::auth::{DeviceInfo, DevicePermission};
use crate::remote::protocol::RemoteActiveDesktopSelection;
use crate::remote::state::{RemoteGatewayConfig, RemoteNetworkMode};
use crate::session::PersistedWorkspaceSession;
use crate::terminal::output_hub::ReplayGap;
use crate::terminal::TerminalSignal;
use crate::worktree::WorktreeIdentity;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

pub const DAEMON_PROTOCOL_VERSION: u32 = 2;

pub mod base64_serde {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use serde::{de::Error as _, Deserialize, Deserializer, Serializer};
    use std::borrow::Cow;

    pub fn serialize<S, T>(bytes: &T, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        T: AsRef<[u8]>,
    {
        serializer.serialize_str(&STANDARD.encode(bytes.as_ref()))
    }

    pub fn deserialize<'de, D, T>(deserializer: D) -> Result<T, D::Error>
    where
        D: Deserializer<'de>,
        T: From<Vec<u8>>,
    {
        let s = Cow::<'de, str>::deserialize(deserializer)?;
        let decoded = STANDARD.decode(s.as_bytes()).map_err(D::Error::custom)?;
        Ok(T::from(decoded))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonSessionDetails {
    pub session_id: String,
    pub workspace_id: Option<String>,
    pub worktree: Option<WorktreeIdentity>,
    pub cwd: Option<String>,
    pub cols: u16,
    pub rows: u16,
    pub running: bool,
    pub start_sequence: Option<u64>,
    pub end_sequence: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonRemoteStatus {
    pub mode: RemoteNetworkMode,
    pub port: u16,
    pub allow_control: bool,
    pub is_running: bool,
    pub bound_address: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum DaemonRequest {
    #[serde(rename_all = "camelCase")]
    Handshake {
        version: u32,
    },
    Ping,
    #[serde(rename_all = "camelCase")]
    RegisterWorkspace {
        workspace_id: String,
        repo_root: String,
    },
    #[serde(rename_all = "camelCase")]
    Spawn {
        client_request_id: String,
        workspace_id: String,
        worktree: Option<WorktreeIdentity>,
        cwd: Option<String>,
        cols: u16,
        rows: u16,
    },
    #[serde(rename_all = "camelCase")]
    Write {
        session_id: String,
        #[serde(with = "base64_serde")]
        data: Vec<u8>,
    },
    #[serde(rename_all = "camelCase")]
    Resize {
        session_id: String,
        cols: u16,
        rows: u16,
    },
    #[serde(rename_all = "camelCase")]
    Signal {
        session_id: String,
        signal: TerminalSignal,
    },
    #[serde(rename_all = "camelCase")]
    Close {
        session_id: String,
    },
    ListSessions,
    #[serde(rename_all = "camelCase")]
    DescribeSession {
        session_id: String,
    },
    #[serde(rename_all = "camelCase")]
    Attach {
        session_id: String,
        after_sequence: Option<u64>,
    },
    #[serde(rename_all = "camelCase")]
    SaveSession {
        session: PersistedWorkspaceSession,
    },
    LoadSession,
    ClearSession,
    RemoteGetStatus,
    #[serde(rename_all = "camelCase")]
    RemoteConfigure {
        config: RemoteGatewayConfig,
    },
    #[serde(rename_all = "camelCase")]
    RemoteCreatePairingCode {
        permission: Option<DevicePermission>,
    },
    RemoteListDevices,
    #[serde(rename_all = "camelCase")]
    RemoteRevokeDevice {
        device_id: String,
    },
    #[serde(rename_all = "camelCase")]
    RemoteSetActiveSelection {
        selection: Option<RemoteActiveDesktopSelection>,
    },
    RemoteGetActiveSelection,
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum DaemonResponse {
    #[serde(rename_all = "camelCase")]
    HandshakeOk {
        version: u32,
        pid: u32,
        epoch: u64,
    },
    Pong,
    #[serde(rename_all = "camelCase")]
    ProtocolMismatch {
        expected_version: u32,
        received_version: u32,
    },
    RegisterWorkspaceOk,
    #[serde(rename_all = "camelCase")]
    SpawnOk {
        session_id: String,
    },
    WriteOk,
    ResizeOk,
    SignalOk,
    CloseOk,
    #[serde(rename_all = "camelCase")]
    ListSessionsOk {
        epoch: u64,
        sessions: Vec<String>,
    },
    #[serde(rename_all = "camelCase")]
    DescribeSessionOk {
        session: DaemonSessionDetails,
    },
    #[serde(rename_all = "camelCase")]
    AttachOk {
        epoch: u64,
        session_id: String,
        start_sequence: Option<u64>,
        end_sequence: Option<u64>,
        gap: Option<ReplayGap>,
        #[serde(with = "base64_serde")]
        history: Vec<u8>,
    },
    SaveSessionOk,
    #[serde(rename_all = "camelCase")]
    LoadSessionOk {
        session: Option<PersistedWorkspaceSession>,
    },
    ClearSessionOk,
    #[serde(rename_all = "camelCase")]
    RemoteStatusOk {
        status: DaemonRemoteStatus,
    },
    RemoteConfigureOk,
    #[serde(rename_all = "camelCase")]
    RemotePairingCodeOk {
        code: String,
    },
    #[serde(rename_all = "camelCase")]
    RemoteListDevicesOk {
        devices: Vec<DeviceInfo>,
    },
    RemoteRevokeDeviceOk,
    RemoteSetActiveSelectionOk,
    #[serde(rename_all = "camelCase")]
    RemoteGetActiveSelectionOk {
        selection: Option<RemoteActiveDesktopSelection>,
    },
    #[serde(rename_all = "camelCase")]
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum DaemonStreamMessage<'a> {
    #[serde(rename_all = "camelCase")]
    Output {
        session_id: Cow<'a, str>,
        sequence: u64,
        #[serde(with = "base64_serde")]
        data: Cow<'a, [u8]>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metrics_read_unix_micros: Option<u64>,
    },
    #[serde(rename_all = "camelCase")]
    Gap {
        session_id: Cow<'a, str>,
        requested_after_sequence: u64,
        available_from_sequence: u64,
    },
    #[serde(rename = "replayGap", rename_all = "camelCase")]
    Lagged {
        session_id: Cow<'a, str>,
        requested_after_sequence: u64,
        available_from_sequence: u64,
        start_sequence: Option<u64>,
        end_sequence: Option<u64>,
        #[serde(with = "base64_serde")]
        history: Cow<'a, [u8]>,
    },
    #[serde(rename_all = "camelCase")]
    Exit {
        session_id: Cow<'a, str>,
        exit_code: Option<i32>,
    },
}

/// Serialize one daemon streaming message using the production newline-delimited JSON frame.
pub fn encode_daemon_stream_frame(message: &DaemonStreamMessage<'_>) -> serde_json::Result<String> {
    let mut frame = serde_json::to_string(message)?;
    frame.push('\n');
    Ok(frame)
}

/// Parse one daemon streaming message using the production newline-delimited JSON frame.
pub fn decode_daemon_stream_frame(frame: &str) -> serde_json::Result<DaemonStreamMessage<'static>> {
    serde_json::from_str(frame.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_daemon_stream_message_compact_base64_encoding() {
        let output = DaemonStreamMessage::Output {
            session_id: Cow::Borrowed("s1"),
            sequence: 42,
            data: Cow::Borrowed(b"hello world"),
            metrics_read_unix_micros: None,
        };
        let json = serde_json::to_string(&output).expect("serialize output");

        assert!(
            !json.contains('['),
            "Stream output must not contain JSON number arrays: {json}"
        );
        assert!(
            json.contains(r#""data":"aGVsbG8gd29ybGQ=""#),
            "Expected base64-encoded string for data: {json}"
        );
        assert!(
            json.contains(r#""sessionId":"s1""#),
            "Expected camelCase sessionId: {json}"
        );
        assert!(
            json.contains(r#""sequence":42"#),
            "Expected sequence 42: {json}"
        );
        assert!(
            !json.contains("metricsReadUnixMicros"),
            "Disabled metrics metadata must not alter the daemon wire frame: {json}"
        );

        let deserialized: DaemonStreamMessage =
            serde_json::from_str(&json).expect("deserialize output");
        match deserialized {
            DaemonStreamMessage::Output {
                session_id,
                sequence,
                data,
                metrics_read_unix_micros,
            } => {
                assert_eq!(session_id, "s1");
                assert_eq!(sequence, 42);
                assert_eq!(&data[..], b"hello world");
                assert_eq!(metrics_read_unix_micros, None);
            }
            _ => panic!("Expected Output variant"),
        }
    }

    #[test]
    fn test_daemon_stream_message_gap_roundtrip() {
        let gap = DaemonStreamMessage::Gap {
            session_id: Cow::Borrowed("s_gap"),
            requested_after_sequence: 10,
            available_from_sequence: 50,
        };
        let json = serde_json::to_string(&gap).expect("serialize gap");
        assert!(json.contains(r#""type":"gap""#));
        assert!(json.contains(r#""sessionId":"s_gap""#));
        assert!(json.contains(r#""requestedAfterSequence":10"#));
        assert!(json.contains(r#""availableFromSequence":50"#));

        let deserialized: DaemonStreamMessage =
            serde_json::from_str(&json).expect("deserialize gap");
        assert_eq!(
            deserialized,
            DaemonStreamMessage::Gap {
                session_id: Cow::Owned("s_gap".to_string()),
                requested_after_sequence: 10,
                available_from_sequence: 50,
            }
        );
    }

    #[test]
    fn test_daemon_stream_message_lagged_with_replay_gap_roundtrip() {
        let replay = DaemonStreamMessage::Lagged {
            session_id: Cow::Borrowed("s2"),
            requested_after_sequence: 10,
            available_from_sequence: 35,
            start_sequence: Some(35),
            end_sequence: Some(100),
            history: Cow::Borrowed(b"history bytes"),
        };
        let json = serde_json::to_string(&replay).expect("serialize replay gap");
        assert!(!json.contains('['));
        assert!(json.contains(r#""type":"replayGap""#));
        assert!(json.contains(r#""history":"aGlzdG9yeSBieXRlcw==""#));
        assert!(json.contains(r#""sessionId":"s2""#));
        assert!(json.contains(r#""requestedAfterSequence":10"#));
        assert!(json.contains(r#""availableFromSequence":35"#));
        assert!(json.contains(r#""startSequence":35"#));
        assert!(json.contains(r#""endSequence":100"#));

        let deserialized: DaemonStreamMessage =
            serde_json::from_str(&json).expect("deserialize replay gap");
        assert_eq!(deserialized, replay);
    }

    #[test]
    fn test_protocol_mismatch_response_is_typed() {
        let response = DaemonResponse::ProtocolMismatch {
            expected_version: DAEMON_PROTOCOL_VERSION,
            received_version: 99,
        };
        let json = serde_json::to_string(&response).expect("serialize mismatch");
        assert!(json.contains(r#""type":"protocolMismatch""#));
        assert!(json.contains(r#""expectedVersion":2"#));
        assert!(json.contains(r#""receivedVersion":99"#));
    }

    #[test]
    fn test_register_workspace_and_remote_commands_serde() {
        let reg_req = DaemonRequest::RegisterWorkspace {
            workspace_id: "ws-123".to_string(),
            repo_root: "/path/to/repo".to_string(),
        };
        let reg_json = serde_json::to_string(&reg_req).expect("serialize register");
        assert!(reg_json.contains(r#""workspaceId":"ws-123""#));
        assert!(reg_json.contains(r#""repoRoot":"/path/to/repo""#));

        let remote_status_resp = DaemonResponse::RemoteStatusOk {
            status: DaemonRemoteStatus {
                mode: RemoteNetworkMode::LocalNetwork,
                port: 43821,
                allow_control: true,
                is_running: true,
                bound_address: Some("0.0.0.0:43821".to_string()),
            },
        };
        let status_json = serde_json::to_string(&remote_status_resp).expect("serialize status");
        assert!(status_json.contains(r#""isRunning":true"#));
        assert!(status_json.contains(r#""boundAddress":"0.0.0.0:43821""#));

        let pair_resp = DaemonResponse::RemotePairingCodeOk {
            code: "123456".to_string(),
        };
        let pair_json = serde_json::to_string(&pair_resp).expect("serialize pair code");
        assert!(pair_json.contains(r#""code":"123456""#));
    }

    #[test]
    fn test_daemon_request_write_and_response_attach_base64() {
        let write_req = DaemonRequest::Write {
            session_id: "term-1".to_string(),
            data: b"ls -la\n".to_vec(),
        };
        let json = serde_json::to_string(&write_req).expect("serialize write");
        assert!(!json.contains('['));
        assert!(json.contains(r#""data":"bHMgLWxhCg==""#));
        assert!(json.contains(r#""sessionId":"term-1""#));

        let deserialized_req: DaemonRequest =
            serde_json::from_str(&json).expect("deserialize write");
        match deserialized_req {
            DaemonRequest::Write { session_id, data } => {
                assert_eq!(session_id, "term-1");
                assert_eq!(data, b"ls -la\n");
            }
            _ => panic!("Expected Write variant"),
        }

        let attach_resp = DaemonResponse::AttachOk {
            epoch: 12345,
            session_id: "term-1".to_string(),
            start_sequence: Some(1),
            end_sequence: Some(10),
            gap: None,
            history: b"initial prompt $ ".to_vec(),
        };
        let resp_json = serde_json::to_string(&attach_resp).expect("serialize attach resp");
        assert!(!resp_json.contains('['));
        assert!(resp_json.contains(r#""history":"aW5pdGlhbCBwcm9tcHQgJCA=""#));
        assert!(resp_json.contains(r#""epoch":12345"#));
        assert!(resp_json.contains(r#""sessionId":"term-1""#));

        let deserialized_resp: DaemonResponse =
            serde_json::from_str(&resp_json).expect("deserialize attach resp");
        match deserialized_resp {
            DaemonResponse::AttachOk {
                epoch,
                session_id,
                start_sequence,
                end_sequence,
                gap,
                history,
            } => {
                assert_eq!(epoch, 12345);
                assert_eq!(session_id, "term-1");
                assert_eq!(start_sequence, Some(1));
                assert_eq!(end_sequence, Some(10));
                assert!(gap.is_none());
                assert_eq!(history, b"initial prompt $ ");
            }
            _ => panic!("Expected AttachOk variant"),
        }
    }

    #[test]
    fn test_describe_session_and_spawn_v2_serde() {
        let spawn_req = DaemonRequest::Spawn {
            client_request_id: "req-abc-123".to_string(),
            workspace_id: "default".to_string(),
            worktree: None,
            cwd: Some("/tmp/test-cwd".to_string()),
            cols: 120,
            rows: 40,
        };
        let spawn_json = serde_json::to_string(&spawn_req).expect("serialize spawn");
        assert!(spawn_json.contains(r#""clientRequestId":"req-abc-123""#));
        assert!(spawn_json.contains(r#""cwd":"/tmp/test-cwd""#));

        let describe_req = DaemonRequest::DescribeSession {
            session_id: "session-99".to_string(),
        };
        let desc_json = serde_json::to_string(&describe_req).expect("serialize describe");
        assert!(desc_json.contains(r#""sessionId":"session-99""#));

        let desc_resp = DaemonResponse::DescribeSessionOk {
            session: DaemonSessionDetails {
                session_id: "session-99".to_string(),
                workspace_id: Some("default".to_string()),
                worktree: None,
                cwd: Some("/tmp/test-cwd".to_string()),
                cols: 120,
                rows: 40,
                running: true,
                start_sequence: Some(5),
                end_sequence: Some(50),
            },
        };
        let desc_resp_json = serde_json::to_string(&desc_resp).expect("serialize describe resp");
        assert!(desc_resp_json.contains(r#""running":true"#));
        assert!(desc_resp_json.contains(r#""startSequence":5"#));
    }

    #[test]
    fn test_protocol_v2_handshake_and_list_sessions_epoch() {
        assert_eq!(DAEMON_PROTOCOL_VERSION, 2);

        let hs = DaemonResponse::HandshakeOk {
            version: DAEMON_PROTOCOL_VERSION,
            pid: 9999,
            epoch: 777777,
        };
        let hs_json = serde_json::to_string(&hs).expect("serialize handshake");
        assert!(hs_json.contains(r#""epoch":777777"#));
        assert!(hs_json.contains(r#""version":2"#));

        let list = DaemonResponse::ListSessionsOk {
            epoch: 777777,
            sessions: vec!["s1".to_string(), "s2".to_string()],
        };
        let list_json = serde_json::to_string(&list).expect("serialize list");
        assert!(list_json.contains(r#""epoch":777777"#));
    }
}
