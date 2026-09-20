//! Operation, lifecycle and persistence wire contracts; no IO or migration.
use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExitMetadata {
    #[serde(deserialize_with = "explicit_null")]
    pub code: Option<i32>,
    #[serde(deserialize_with = "nullable_text")]
    pub signal: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum SessionDetail {
    Running {
        session: Session,
    },
    Exited {
        session: Session,
        exit: ExitMetadata,
    },
    Expired {
        target: RemoteTerminalTarget,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum OperationOutcome {
    NoContent,
    Project { project: Project },
    Worktree { worktree: Worktree },
    Session { session: Session },
    Error { error: MachineError },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Operation {
    Pending {
        #[serde(deserialize_with = "request_id")]
        request_id: String,
    },
    OutcomeUnknown {
        #[serde(deserialize_with = "request_id")]
        request_id: String,
    },
    ResultExpired {
        #[serde(deserialize_with = "request_id")]
        request_id: String,
    },
    Completed {
        #[serde(deserialize_with = "request_id")]
        request_id: String,
        outcome: OperationOutcome,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirtyFile {
    #[serde(deserialize_with = "required")]
    pub status_code: String,
    #[serde(deserialize_with = "required")]
    pub path: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirtyState {
    pub is_dirty: bool,
    pub files: Vec<DirtyFile>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchDeletion {
    #[serde(deserialize_with = "required")]
    pub branch: String,
    #[serde(deserialize_with = "required")]
    pub head: String,
    #[serde(deserialize_with = "nullable_text")]
    pub upstream: Option<String>,
    pub merged: bool,
    #[serde(deserialize_with = "explicit_null")]
    pub ahead: Option<u32>,
    #[serde(deserialize_with = "explicit_null")]
    pub behind: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeStatus {
    #[serde(deserialize_with = "required")]
    pub workspace_id: String,
    pub worktree: WorktreeIdentity,
    pub dirty: DirtyState,
    pub dirty_count: u32,
    #[serde(deserialize_with = "explicit_null")]
    pub branch_deletion: Option<BranchDeletion>,
    #[serde(deserialize_with = "nullable_text")]
    pub locked: Option<String>,
    #[serde(deserialize_with = "nullable_text")]
    pub prunable: Option<String>,
    #[serde(deserialize_with = "text_list")]
    pub live_session_ids: Vec<String>,
    pub revision: Epoch,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionStatus {
    Running,
    Exited,
    Expired,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Interrupt {
    Interrupt,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Control {
    Resize {
        generation: Epoch,
        cols: u16,
        rows: u16,
    },
    Signal {
        generation: Epoch,
        signal: Interrupt,
    },
    Ping,
    Pong,
    Error {
        error: MachineError,
    },
    Status {
        target: RemoteTerminalTarget,
        status: SessionStatus,
    },
    Exit {
        target: RemoteTerminalTarget,
        exit: ExitMetadata,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Startup {
    Shell,
    AgentResume {
        #[serde(deserialize_with = "required")]
        agent_type: String,
        #[serde(deserialize_with = "provider")]
        provider_session: crate::daemon::protocol::AgentProviderSession,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterRequest {
    #[serde(deserialize_with = "request_id")]
    pub request_id: String,
    #[serde(deserialize_with = "required")]
    pub repo_path: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnregisterRequest {
    #[serde(deserialize_with = "request_id")]
    pub request_id: String,
    pub expected_revision: Epoch,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateWorktreeRequest {
    #[serde(deserialize_with = "request_id")]
    pub request_id: String,
    #[serde(deserialize_with = "required")]
    pub workspace_id: String,
    pub worktree: WorktreeIdentity,
    #[serde(
        default,
        deserialize_with = "optional_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub base_ref: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteWorktreeRequest {
    #[serde(deserialize_with = "request_id")]
    pub request_id: String,
    #[serde(deserialize_with = "required")]
    pub workspace_id: String,
    pub worktree: WorktreeIdentity,
    pub delete_branch: bool,
    pub expected_revision: Epoch,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSessionRequest {
    #[serde(deserialize_with = "request_id")]
    pub request_id: String,
    #[serde(deserialize_with = "required")]
    pub workspace_id: String,
    #[serde(deserialize_with = "explicit_null")]
    pub worktree: Option<WorktreeIdentity>,
    pub cols: u16,
    pub rows: u16,
    #[serde(deserialize_with = "nullable_text")]
    pub inherit_from_session_id: Option<String>,
    #[serde(deserialize_with = "nullable_text")]
    pub cwd_relative: Option<String>,
    pub startup: Startup,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloseSessionRequest {
    #[serde(deserialize_with = "request_id")]
    pub request_id: String,
    pub daemon_epoch: Epoch,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeAttachment {
    pub daemon_epoch: Epoch,
    pub last_output_sequence: Epoch,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProcessStatus {
    Running,
    Exited,
    Expired,
    Unknown,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PersistedTransportStatus {
    Disconnected,
    Reconnecting,
    Revoked,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyDescriptor {
    #[serde(deserialize_with = "required")]
    pub host_id: String,
    #[serde(deserialize_with = "workspace_hash")]
    pub workspace_id: String,
    #[serde(deserialize_with = "required")]
    pub remote_workspace_id: String,
    #[serde(deserialize_with = "proxy_hash")]
    pub local_proxy_id: String,
    #[serde(deserialize_with = "explicit_null")]
    pub worktree: Option<WorktreeIdentity>,
    pub remote_target: RemoteTerminalTarget,
    pub remote_cursor: Epoch,
    pub native_attachment: NativeAttachment,
    pub cols: u16,
    pub rows: u16,
    #[serde(deserialize_with = "request_id")]
    pub request_id: String,
    pub process_status: ProcessStatus,
    pub transport_status: PersistedTransportStatus,
}
fn hash_id<'de, D: serde::Deserializer<'de>>(d: D, prefix: &str) -> Result<String, D::Error> {
    let s = required(d)?;
    let valid = s.strip_prefix(prefix).is_some_and(|hash| {
        hash.len() == 64
            && hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    });
    if !valid {
        return Err(serde::de::Error::custom(
            "full lowercase hash identity required",
        ));
    }
    Ok(s)
}
fn workspace_hash<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    hash_id(d, "daemon:")
}
fn proxy_hash<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    hash_id(d, "daemon-session:")
}
