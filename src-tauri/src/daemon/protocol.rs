// allow: SIZE_OK — Daemon wire protocol framing, request/response models, and stream encoding
use crate::remote::auth::{DeviceInfo, DevicePermission};
use crate::remote::protocol::RemoteActiveDesktopSelection;
use crate::remote::state::{RemoteGatewayConfig, RemoteNetworkMode};
use crate::session::PersistedWorkspaceSession;
use crate::terminal::output_hub::ReplayGap;
use crate::terminal::TerminalSignal;
use crate::worktree::WorktreeIdentity;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

pub const DAEMON_PROTOCOL_VERSION: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistorySegmentWire {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cols: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rows: Option<u16>,
    #[serde(with = "base64_serde")]
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentProviderSessionKey {
    SessionId,
    ConversationId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProviderSession {
    pub key: AgentProviderSessionKey,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript_path: Option<String>,
}

fn deserialize_optional_provider_session<'de, D>(
    deserializer: D,
) -> Result<Option<AgentProviderSession>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value.and_then(|candidate| serde_json::from_value(candidate).ok()))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TerminalStartup {
    #[serde(rename_all = "camelCase")]
    AgentResume {
        agent_type: String,
        provider_session: AgentProviderSession,
    },
}

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
    UnregisterWorkspace {
        workspace_id: String,
    },
    #[serde(rename_all = "camelCase")]
    Spawn {
        client_request_id: String,
        workspace_id: String,
        worktree: Option<WorktreeIdentity>,
        cwd: Option<String>,
        cols: u16,
        rows: u16,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        shell: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        startup: Option<TerminalStartup>,
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
    DiscoverAgentSession {
        session_id: String,
        agent_type: String,
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
    /// Turns this connection into a one-way stream of desktop-directed remote
    /// events. Without it the gateway (which lives in the daemon) has no way to
    /// reach the GUI, so remote-issued selections are silently dropped.
    SubscribeRemoteEvents,
    #[serde(rename_all = "camelCase")]
    UpgradeBinary {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        new_binary_path: Option<String>,
    },
    PrepareHandover,
    #[serde(rename_all = "camelCase")]
    CommitHandover {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        legacy_socket_path: Option<String>,
    },
    AbortHandover,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        binary_path: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        binary_mtime_ms: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        daemon_version: Option<String>,
    },
    Pong,
    #[serde(rename_all = "camelCase")]
    ProtocolMismatch {
        expected_version: u32,
        received_version: u32,
    },
    RegisterWorkspaceOk,
    UnregisterWorkspaceOk,
    #[serde(rename_all = "camelCase")]
    SpawnOk {
        session_id: String,
        epoch: u64,
        session: DaemonSessionDetails,
    },
    #[serde(rename_all = "camelCase")]
    AgentResumeInvalid {
        message: String,
    },
    #[serde(rename_all = "camelCase")]
    AgentSessionConflict {
        agent_type: String,
        provider_key: AgentProviderSessionKey,
        provider_id: String,
        existing_session_id: String,
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
    DiscoverAgentSessionOk {
        provider_session_id: Option<String>,
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
        #[serde(default)]
        pty_cols: Option<u16>,
        #[serde(default)]
        pty_rows: Option<u16>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        history_segments: Vec<HistorySegmentWire>,
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
    SubscribeRemoteEventsOk,
    UpgradeScheduled,
    UpgradeNotNeeded,
    UpgradeDeferred,
    UpgradeUnsupported,
    #[serde(rename_all = "camelCase")]
    PrepareHandoverOk {
        legacy_socket_path: String,
        active_sessions: Vec<String>,
    },
    CommitHandoverOk,
    AbortHandoverOk,
    #[serde(rename_all = "camelCase")]
    HandoverRejected {
        reason: String,
    },
    #[serde(rename_all = "camelCase")]
    Error {
        message: String,
    },
}

/// One desktop-directed remote event, streamed after `SubscribeRemoteEventsOk`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DaemonRemoteEvent {
    pub event: String,
    pub payload: serde_json::Value,
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
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        segments: Vec<HistorySegmentWire>,
    },
    #[serde(rename_all = "camelCase")]
    AgentState {
        session_id: Cow<'a, str>,
        state: Cow<'a, str>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent: Option<Cow<'a, str>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_session: Option<AgentProviderSession>,
    },
    #[serde(rename_all = "camelCase")]
    Exit {
        session_id: Cow<'a, str>,
        exit_code: Option<i32>,
    },
}

/// One state report from the Ferryx agent extension running inside a PTY session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStateReport {
    pub session_id: String,
    pub state: String,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_provider_session")]
    pub provider_session: Option<AgentProviderSession>,
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
    fn agent_state_report_treats_malformed_provider_session_as_absent() {
        // Given: an otherwise valid extension report with an invalid provider key.
        let report = r#"{"sessionId":"pty-1","state":"working","agent":"omo","providerSession":{"key":"invented","id":"provider-1"}}"#;

        // When: the report crosses the daemon protocol boundary.
        let decoded: AgentStateReport = serde_json::from_str(report).expect("decode report");

        // Then: state delivery survives, but the malformed provider reference does not.
        assert_eq!(decoded.provider_session, None);
    }

    #[test]
    fn agent_state_stream_serializes_authoritative_provider_session() {
        // Given: a provider-authored session reference on an agent state frame.
        let message = DaemonStreamMessage::AgentState {
            session_id: Cow::Borrowed("pty-1"),
            state: Cow::Borrowed("working"),
            agent: Some(Cow::Borrowed("omo")),
            provider_session: Some(AgentProviderSession {
                key: AgentProviderSessionKey::SessionId,
                id: "provider-1".to_string(),
                transcript_path: None,
            }),
        };

        // When: serialized through the production framing contract.
        let frame = encode_daemon_stream_frame(&message).expect("encode frame");

        // Then: the structured provider reference is present in the snapshot.
        assert_eq!(
            frame,
            "{\"type\":\"agentState\",\"sessionId\":\"pty-1\",\"state\":\"working\",\"agent\":\"omo\",\"providerSession\":{\"key\":\"session_id\",\"id\":\"provider-1\"}}\n"
        );
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
            segments: Vec::new(),
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
        assert!(json.contains(&format!(r#""expectedVersion":{DAEMON_PROTOCOL_VERSION}"#)));
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
            pty_cols: None,
            pty_rows: None,
            history_segments: Vec::new(),
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
                pty_cols,
                pty_rows,
                history_segments,
            } => {
                assert_eq!(epoch, 12345);
                assert_eq!(session_id, "term-1");
                assert_eq!(start_sequence, Some(1));
                assert_eq!(end_sequence, Some(10));
                assert!(gap.is_none());
                assert_eq!(history, b"initial prompt $ ");
                assert_eq!(pty_cols, None);
                assert_eq!(pty_rows, None);
                assert_eq!(history_segments, Vec::<HistorySegmentWire>::new());
            }
            _ => panic!("Expected AttachOk variant"),
        }

        // Serde skew test: deserializing legacy AttachOk JSON without ptyCols/ptyRows defaults to None
        let legacy_json = r#"{"type":"attachOk","epoch":12345,"sessionId":"term-1","startSequence":1,"endSequence":10,"gap":null,"history":"aW5pdGlhbCBwcm9tcHQgJCA="}"#;
        let legacy_deserialized: DaemonResponse =
            serde_json::from_str(legacy_json).expect("deserialize legacy attach resp");
        match legacy_deserialized {
            DaemonResponse::AttachOk {
                epoch,
                session_id,
                start_sequence,
                end_sequence,
                gap,
                history,
                pty_cols,
                pty_rows,
                history_segments,
            } => {
                assert_eq!(epoch, 12345);
                assert_eq!(session_id, "term-1");
                assert_eq!(start_sequence, Some(1));
                assert_eq!(end_sequence, Some(10));
                assert!(gap.is_none());
                assert_eq!(history, b"initial prompt $ ");
                assert_eq!(pty_cols, None);
                assert_eq!(pty_rows, None);
                assert_eq!(history_segments, Vec::<HistorySegmentWire>::new());
            }
            _ => panic!("Expected AttachOk variant"),
        }

        // Segmented history round-trip preserving base64 bytes
        let segmented_attach_resp = DaemonResponse::AttachOk {
            epoch: 12345,
            session_id: "term-1".to_string(),
            start_sequence: Some(1),
            end_sequence: Some(2),
            gap: None,
            history: b"AB".to_vec(),
            pty_cols: Some(120),
            pty_rows: Some(30),
            history_segments: vec![
                HistorySegmentWire {
                    cols: Some(80),
                    rows: Some(24),
                    bytes: b"A".to_vec(),
                },
                HistorySegmentWire {
                    cols: Some(120),
                    rows: Some(30),
                    bytes: b"B".to_vec(),
                },
            ],
        };
        let seg_resp_json =
            serde_json::to_string(&segmented_attach_resp).expect("serialize segmented attach resp");
        assert!(seg_resp_json.contains(r#""historySegments":[{""#));
        assert!(seg_resp_json.contains(r#""bytes":"QQ==""#));
        assert!(seg_resp_json.contains(r#""bytes":"Qg==""#));
        let seg_deserialized: DaemonResponse =
            serde_json::from_str(&seg_resp_json).expect("deserialize segmented attach resp");
        match seg_deserialized {
            DaemonResponse::AttachOk {
                history_segments, ..
            } => {
                assert_eq!(
                    history_segments,
                    vec![
                        HistorySegmentWire {
                            cols: Some(80),
                            rows: Some(24),
                            bytes: b"A".to_vec(),
                        },
                        HistorySegmentWire {
                            cols: Some(120),
                            rows: Some(30),
                            bytes: b"B".to_vec(),
                        },
                    ]
                );
            }
            _ => panic!("Expected AttachOk variant"),
        }

        // Serialization and deserialization with ptyCols / ptyRows
        let sized_attach_resp = DaemonResponse::AttachOk {
            epoch: 12345,
            session_id: "term-1".to_string(),
            start_sequence: Some(1),
            end_sequence: Some(10),
            gap: None,
            history: b"initial prompt $ ".to_vec(),
            pty_cols: Some(120),
            pty_rows: Some(30),
            history_segments: Vec::new(),
        };
        let sized_resp_json =
            serde_json::to_string(&sized_attach_resp).expect("serialize sized attach resp");
        assert!(sized_resp_json.contains(r#""ptyCols":120"#));
        assert!(sized_resp_json.contains(r#""ptyRows":30"#));
        let sized_deserialized: DaemonResponse =
            serde_json::from_str(&sized_resp_json).expect("deserialize sized attach resp");
        match sized_deserialized {
            DaemonResponse::AttachOk {
                pty_cols, pty_rows, ..
            } => {
                assert_eq!(pty_cols, Some(120));
                assert_eq!(pty_rows, Some(30));
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
            shell: None,
            startup: None,
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
    fn test_protocol_v3_handshake_and_list_sessions_epoch() {
        assert_eq!(DAEMON_PROTOCOL_VERSION, 3);

        let hs = DaemonResponse::HandshakeOk {
            version: DAEMON_PROTOCOL_VERSION,
            pid: 9999,
            epoch: 777777,
            binary_path: Some("/bin/ferryx".to_string()),
            binary_mtime_ms: Some(1700000000000),
            daemon_version: Some("2026.902.2".to_string()),
        };
        let hs_json = serde_json::to_string(&hs).expect("serialize handshake");
        assert!(hs_json.contains(r#""epoch":777777"#));
        assert!(hs_json.contains(r#""version":3"#));
        assert!(hs_json.contains(r#""binaryPath":"/bin/ferryx""#));
        assert!(hs_json.contains(r#""binaryMtimeMs":1700000000000"#));
        assert!(hs_json.contains(r#""daemonVersion":"2026.902.2""#));

        // Back-compat: JSON without binary/version fields deserializes with None
        let legacy_hs_json = r#"{"type":"handshakeOk","version":3,"pid":9999,"epoch":777777}"#;
        let legacy_hs: DaemonResponse =
            serde_json::from_str(legacy_hs_json).expect("deserialize legacy handshake");
        match legacy_hs {
            DaemonResponse::HandshakeOk {
                binary_path,
                binary_mtime_ms,
                daemon_version,
                ..
            } => {
                assert_eq!(binary_path, None);
                assert_eq!(binary_mtime_ms, None);
                assert_eq!(daemon_version, None);
            }
            _ => panic!("Expected HandshakeOk"),
        }

        let list = DaemonResponse::ListSessionsOk {
            epoch: 777777,
            sessions: vec!["s1".to_string(), "s2".to_string()],
        };
        let list_json = serde_json::to_string(&list).expect("serialize list");
        assert!(list_json.contains(r#""epoch":777777"#));
    }

    #[test]
    fn test_spawn_shell_field_roundtrip_and_backward_compatibility() {
        // Back-compat: JSON without shell field deserializes to None
        let legacy_json = r#"{"type":"spawn","clientRequestId":"req-1","workspaceId":"ws-1","worktree":null,"cwd":"/home","cols":80,"rows":24}"#;
        let req: DaemonRequest =
            serde_json::from_str(legacy_json).expect("deserialize legacy spawn");
        match req {
            DaemonRequest::Spawn {
                client_request_id,
                workspace_id,
                worktree,
                cwd,
                cols,
                rows,
                shell,
                startup,
            } => {
                assert_eq!(client_request_id, "req-1");
                assert_eq!(workspace_id, "ws-1");
                assert_eq!(worktree, None);
                assert_eq!(cwd, Some("/home".to_string()));
                assert_eq!(cols, 80);
                assert_eq!(rows, 24);
                assert_eq!(shell, None);
                assert_eq!(startup, None);
            }
            _ => panic!("Expected Spawn variant"),
        }

        // Back-compat: JSON with explicit "startup": null deserializes to None
        let null_startup_json = r#"{"type":"spawn","clientRequestId":"req-null","workspaceId":"ws-1","worktree":null,"cwd":"/home","cols":80,"rows":24,"startup":null}"#;
        let req_null: DaemonRequest =
            serde_json::from_str(null_startup_json).expect("deserialize null startup spawn");
        match req_null {
            DaemonRequest::Spawn { startup, .. } => {
                assert_eq!(startup, None);
            }
            _ => panic!("Expected Spawn variant"),
        }

        // Roundtrip with shell: "pwsh"
        let spawn_with_shell = DaemonRequest::Spawn {
            client_request_id: "req-2".to_string(),
            workspace_id: "ws-2".to_string(),
            worktree: None,
            cwd: None,
            cols: 100,
            rows: 30,
            shell: Some("pwsh".to_string()),
            startup: None,
        };
        let serialized =
            serde_json::to_string(&spawn_with_shell).expect("serialize spawn with shell");
        assert!(serialized.contains(r#""shell":"pwsh""#));
        let deserialized: DaemonRequest =
            serde_json::from_str(&serialized).expect("deserialize spawn with shell");
        match deserialized {
            DaemonRequest::Spawn { shell, startup, .. } => {
                assert_eq!(shell, Some("pwsh".to_string()));
                assert_eq!(startup, None);
            }
            _ => panic!("Expected Spawn variant"),
        }

        assert_eq!(DAEMON_PROTOCOL_VERSION, 3);
    }

    #[test]
    fn test_spawn_with_agent_resume_startup_roundtrip() {
        // Given: a typed daemon spawn request with structured agent resume startup
        let req_json = r#"{
            "type": "spawn",
            "clientRequestId": "req-resume-1",
            "workspaceId": "ws-1",
            "worktree": null,
            "cwd": "/path/to/repo",
            "cols": 80,
            "rows": 24,
            "startup": {
                "kind": "agentResume",
                "agentType": "claude",
                "providerSession": {
                    "key": "session_id",
                    "id": "claude-session-123",
                    "transcriptPath": "/path/to/transcript.json"
                }
            }
        }"#;

        // When: deserialized into DaemonRequest
        let deserialized: DaemonRequest =
            serde_json::from_str(req_json).expect("deserialize spawn with agent resume startup");

        // Then: startup field contains structured agent resume payload
        match deserialized {
            DaemonRequest::Spawn { startup, .. } => {
                let startup = startup.expect("startup must be present");
                assert_eq!(
                    startup,
                    TerminalStartup::AgentResume {
                        agent_type: "claude".to_string(),
                        provider_session: AgentProviderSession {
                            key: AgentProviderSessionKey::SessionId,
                            id: "claude-session-123".to_string(),
                            transcript_path: Some("/path/to/transcript.json".to_string()),
                        },
                    }
                );
            }
            _ => panic!("Expected DaemonRequest::Spawn variant"),
        }
    }

    #[test]
    fn test_upgrade_binary_protocol_serde_roundtrip() {
        let req = DaemonRequest::UpgradeBinary {
            new_binary_path: None,
        };
        let json = serde_json::to_string(&req).expect("serialize UpgradeBinary");
        assert_eq!(json, r#"{"type":"upgradeBinary"}"#);
        let deserialized: DaemonRequest =
            serde_json::from_str(&json).expect("deserialize UpgradeBinary");
        assert!(matches!(
            deserialized,
            DaemonRequest::UpgradeBinary {
                new_binary_path: None
            }
        ));

        let req_with_path = DaemonRequest::UpgradeBinary {
            new_binary_path: Some("/Applications/Ferryx.app/Contents/MacOS/ferryx".to_string()),
        };
        let json_with_path =
            serde_json::to_string(&req_with_path).expect("serialize UpgradeBinary with path");
        assert!(json_with_path
            .contains(r#""newBinaryPath":"/Applications/Ferryx.app/Contents/MacOS/ferryx""#));
        let deserialized_with_path: DaemonRequest =
            serde_json::from_str(&json_with_path).expect("deserialize UpgradeBinary with path");
        match deserialized_with_path {
            DaemonRequest::UpgradeBinary { new_binary_path } => {
                assert_eq!(
                    new_binary_path,
                    Some("/Applications/Ferryx.app/Contents/MacOS/ferryx".to_string())
                );
            }
            _ => panic!("Expected UpgradeBinary"),
        }

        for (variant, expected_type) in [
            (DaemonResponse::UpgradeScheduled, "upgradeScheduled"),
            (DaemonResponse::UpgradeNotNeeded, "upgradeNotNeeded"),
            (DaemonResponse::UpgradeDeferred, "upgradeDeferred"),
            (DaemonResponse::UpgradeUnsupported, "upgradeUnsupported"),
        ] {
            let resp_json = serde_json::to_string(&variant).expect("serialize response");
            assert_eq!(resp_json, format!(r#"{{"type":"{expected_type}"}}"#));
            let deserialized_resp: DaemonResponse =
                serde_json::from_str(&resp_json).expect("deserialize response");
            match (&variant, &deserialized_resp) {
                (DaemonResponse::UpgradeScheduled, DaemonResponse::UpgradeScheduled) => {}
                (DaemonResponse::UpgradeNotNeeded, DaemonResponse::UpgradeNotNeeded) => {}
                (DaemonResponse::UpgradeDeferred, DaemonResponse::UpgradeDeferred) => {}
                (DaemonResponse::UpgradeUnsupported, DaemonResponse::UpgradeUnsupported) => {}
                _ => panic!("Mismatch deserializing {expected_type}"),
            }
        }
    }

    #[test]
    fn test_handover_protocol_serde_roundtrip() {
        let prep_req = DaemonRequest::PrepareHandover;
        let prep_json = serde_json::to_string(&prep_req).expect("serialize PrepareHandover");
        assert_eq!(prep_json, r#"{"type":"prepareHandover"}"#);
        let prep_deser: DaemonRequest =
            serde_json::from_str(&prep_json).expect("deserialize PrepareHandover");
        assert!(matches!(prep_deser, DaemonRequest::PrepareHandover));

        let commit_req = DaemonRequest::CommitHandover {
            legacy_socket_path: Some("/tmp/legacy.sock".to_string()),
        };
        let commit_json = serde_json::to_string(&commit_req).expect("serialize CommitHandover");
        assert!(commit_json.contains(r#""legacySocketPath":"/tmp/legacy.sock""#));
        let commit_deser: DaemonRequest =
            serde_json::from_str(&commit_json).expect("deserialize CommitHandover");
        match commit_deser {
            DaemonRequest::CommitHandover { legacy_socket_path } => {
                assert_eq!(legacy_socket_path, Some("/tmp/legacy.sock".to_string()));
            }
            _ => panic!("Expected CommitHandover"),
        }

        let abort_req = DaemonRequest::AbortHandover;
        let abort_json = serde_json::to_string(&abort_req).expect("serialize AbortHandover");
        assert_eq!(abort_json, r#"{"type":"abortHandover"}"#);

        let prep_resp = DaemonResponse::PrepareHandoverOk {
            legacy_socket_path: "/tmp/legacy.sock".to_string(),
            active_sessions: vec!["s1".to_string(), "s2".to_string()],
        };
        let prep_resp_json =
            serde_json::to_string(&prep_resp).expect("serialize PrepareHandoverOk");
        assert!(prep_resp_json.contains(r#""legacySocketPath":"/tmp/legacy.sock""#));
        assert!(prep_resp_json.contains(r#""activeSessions":["s1","s2"]"#));

        let commit_resp = DaemonResponse::CommitHandoverOk;
        let commit_resp_json =
            serde_json::to_string(&commit_resp).expect("serialize CommitHandoverOk");
        assert_eq!(commit_resp_json, r#"{"type":"commitHandoverOk"}"#);

        let abort_resp = DaemonResponse::AbortHandoverOk;
        let abort_resp_json =
            serde_json::to_string(&abort_resp).expect("serialize AbortHandoverOk");
        assert_eq!(abort_resp_json, r#"{"type":"abortHandoverOk"}"#);

        let rej_resp = DaemonResponse::HandoverRejected {
            reason: "Already in progress".to_string(),
        };
        let rej_json = serde_json::to_string(&rej_resp).expect("serialize HandoverRejected");
        assert!(rej_json.contains(r#""reason":"Already in progress""#));
    }
}
