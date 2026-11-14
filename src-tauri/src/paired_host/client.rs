//! Typed native machine HTTP. No arbitrary URL/method input or credential projection.
use super::{
    inventory::{AuthStatus, CredentialLease, GrantScope, HostView},
    projects,
    reachability::ReachabilityState,
    service::{PairedHostService, ServiceError},
};
use crate::{remote::machine_protocol as m, scoped_contracts::Epoch};
use reqwest::{Method, Url};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OperationRequest {
    pub host_id: String,
    pub generation: Epoch,
    pub operation: Operation,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Operation {
    Capabilities,
    Directories {
        path: Option<String>,
        include_hidden: bool,
    },
    /// Bounded Git repository discovery below a host directory (`repoDiscoveryV1`).
    DiscoverRepos {
        path: Option<String>,
        depth: Option<u8>,
        include_hidden: bool,
    },
    Projects,
    RegisterProject {
        request: m::RegisterRequest,
    },
    UnregisterProject {
        workspace_id: String,
        request: m::UnregisterRequest,
    },
    Worktrees {
        workspace_id: String,
    },
    WorktreeStatus {
        workspace_id: String,
        worktree: m::WorktreeIdentity,
    },
    CreateWorktree {
        request: m::CreateWorktreeRequest,
    },
    DeleteWorktree {
        request: m::DeleteWorktreeRequest,
    },
    Sessions {
        workspace_id: Option<String>,
    },
    Session {
        session_id: String,
        daemon_epoch: Epoch,
    },
    CreateSession {
        request: m::CreateSessionRequest,
    },
    CloseSession {
        session_id: String,
        request: m::CloseSessionRequest,
    },
    Operation {
        request_id: String,
    },
    PasteUploadChunk {
        request: m::PasteUploadChunkRequest,
    },
    /// Reads one byte range of a host file; idempotent, so it is never journaled.
    FileRead {
        request: m::FileReadRequest,
    },
    /// Capability probe for DAG streaming. The stream itself is a socket, opened by
    /// [`MachineClient::attach_dag`], not an HTTP result.
    DagStream {
        workspace_id: String,
    },
    /// Agent control on the paired host. Send/Answer/Stop are mutations and are not retried.
    AgentControl {
        op: crate::agent_control::AgentControlOp,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationResponse {
    pub host_id: String,
    pub generation: Epoch,
    pub result: OperationResult,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum OperationResult {
    Capabilities(m::Capabilities),
    Directories(m::Directories),
    DiscoverRepos(DiscoveredRepos),
    Projects(projects::Projects),
    RegisterProject(projects::Project),
    UnregisterProject(()),
    Worktrees(m::Worktrees),
    WorktreeStatus(m::WorktreeStatus),
    CreateWorktree(m::Worktree),
    DeleteWorktree(()),
    Sessions(m::Sessions),
    Session(m::SessionDetail),
    CreateSession(m::Session),
    CloseSession(()),
    Operation(m::Operation),
    PasteUploadChunk(m::PasteUploadChunkResult),
    FileRead(m::FileReadResult),
    AgentControl(serde_json::Value),
}
/// Desktop projection of [m::Repositories]: a registered row carries the desktop
/// workspace id (`daemon:<sha256>`) next to the host's own id, so the caller can open
/// the existing project without a second inventory read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredRepos {
    pub path: String,
    pub repos: Vec<DiscoveredRepo>,
    pub truncated: bool,
    pub elapsed_ms: u64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredRepo {
    pub repo_root: String,
    pub is_linked_worktree: bool,
    pub git_common_dir: Option<String>,
    pub branch: Option<String>,
    /// Desktop workspace id of the registered project, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registered_workspace_id: Option<String>,
    /// The host's own workspace id for the same registration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_workspace_id: Option<String>,
    pub target: crate::scoped_contracts::RunTarget,
}
/// Rewrites host workspace ids into desktop ids. Rows are bounded by the host's caps.
#[allow(clippy::result_large_err)]
pub(crate) fn discovered_repos(host: &str, rows: m::Repositories) -> Result<DiscoveredRepos> {
    if rows.repos.len() > 200 {
        return Err(ClientError::local("PAIRED_HOST_INVALID_RESPONSE"));
    }
    Ok(DiscoveredRepos {
        path: rows.path,
        truncated: rows.truncated,
        elapsed_ms: rows.elapsed_ms,
        repos: rows
            .repos
            .into_iter()
            .map(|repo| DiscoveredRepo {
                registered_workspace_id: repo
                    .registered_workspace_id
                    .as_deref()
                    .map(|remote| projects::id(host, remote)),
                remote_workspace_id: repo.registered_workspace_id,
                repo_root: repo.repo_root,
                is_linked_worktree: repo.is_linked_worktree,
                git_common_dir: repo.git_common_dir,
                branch: repo.branch,
                target: crate::scoped_contracts::RunTarget::PairedDaemon {
                    host_id: host.into(),
                },
            })
            .collect(),
    })
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientError {
    pub code: String,
    pub machine_error: Option<Box<m::MachineError>>,
    pub request_id: Option<String>,
    pub ambiguous: bool,
}
impl ClientError {
    pub(crate) fn local(code: &str) -> Self {
        Self {
            code: code.into(),
            machine_error: None,
            request_id: None,
            ambiguous: false,
        }
    }
    /// R5-N3: a structured machine rejection is definitive — the remote
    /// processed and refused the request, so it must stay non-ambiguous.
    /// Only transport/decode uncertainty (no machine_error) may be relabeled.
    pub(crate) fn relabel_transport_uncertainty(&mut self, request_id: &str) {
        self.request_id = Some(request_id.to_string());
        if self.machine_error.is_none() {
            self.ambiguous = true;
        }
    }
}
impl From<ServiceError> for ClientError {
    fn from(e: ServiceError) -> Self {
        Self::local(&e.code)
    }
}
impl From<super::inventory::InventoryError> for ClientError {
    fn from(e: super::inventory::InventoryError) -> Self {
        ServiceError::from(e).into()
    }
}
type Result<T> = std::result::Result<T, ClientError>;

/// Remote prose and arbitrary detail keys never cross the native boundary.
/// Request IDs have already been constrained to UUIDs by the wire decoder.
fn project_remote_error(error: m::MachineError) -> m::MachineError {
    let code = match error.code.as_str() {
        "UNAUTHORIZED"
        | "TOKEN_EXPIRED"
        | "MACHINE_ACCESS_REQUIRED"
        | "MACHINE_OWNER_UNSUPPORTED"
        | "MACHINE_SERVICE_UNAVAILABLE"
        | "HOST_UNAVAILABLE"
        | "TIMEOUT"
        | "INVALID_REQUEST"
        | "INVALID_PATH"
        | "UNSUPPORTED_PATH"
        | "PAYLOAD_TOO_LARGE"
        | "PERMISSION_DENIED"
        | "RATE_LIMITED"
        | "CAPACITY_EXCEEDED"
        | "DIRECTORY_NOT_FOUND"
        | "PROJECT_NOT_FOUND"
        | "PROJECT_BUSY"
        | "NOT_A_GIT_REPOSITORY"
        | "BASE_REF_UNAVAILABLE"
        | "INVALID_BASE_REF"
        | "INVALID_WORKTREE"
        | "WORKSPACE_ID_MISMATCH"
        | "WORKTREE_NOT_FOUND"
        | "WORKTREE_BUSY"
        | "WORKTREE_EXISTS"
        | "WORKTREE_LOCKED"
        | "DIRTY_WORKTREE"
        | "UNMERGED_BRANCH"
        | "WORKTREE_REMOVED_BRANCH_RETAINED"
        | "WORKTREE_REMOVED_PRUNE_FAILED"
        | "OUTPUT_LIMIT_EXCEEDED"
        | "REQUEST_CONFLICT"
        | "STALE_REVISION"
        | "OPERATION_NOT_FOUND"
        | "OPERATION_OUTCOME_UNKNOWN"
        | "OPERATION_RESULT_EXPIRED"
        | "SESSION_EXPIRED"
        | "SESSION_NOT_FOUND"
        | "SESSION_OWNERSHIP_CHANGED"
        | "AGENT_RESUME_UNSUPPORTED"
        | "AGENT_SESSION_CONFLICT"
        | "PARENT_SESSION_MISMATCH"
        | "CONTROL_CONFLICT"
        | "STALE_EPOCH"
        | "STALE_GENERATION"
        | "NOT_FOUND"
        | "METHOD_NOT_ALLOWED"
        | "UNSUPPORTED"
        | "FORBIDDEN"
        | "INVENTORY_INCOMPLETE" => error.code.as_str(),
        _ => "PAIRED_HOST_REMOTE_ERROR",
    };
    let mut details = serde_json::Map::new();
    if matches!(
        code,
        "WORKTREE_REMOVED_BRANCH_RETAINED" | "WORKTREE_REMOVED_PRUNE_FAILED"
    ) {
        for key in ["worktreeRemoved", "branchDeleted", "pruned"] {
            if let Some(value) = error.details.get(key).and_then(serde_json::Value::as_bool) {
                details.insert(key.into(), value.into());
            }
        }
    }
    m::MachineError {
        code: code.into(),
        message: "The paired host could not complete the request.".into(),
        retryable: error.retryable,
        request_id: error.request_id,
        details,
    }
}
/// How one Capabilities round-trip bears on a host's reachability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProbeOutcome {
    Online,
    /// The host (or its relay control channel) cannot be reached right now. Carries no
    /// error: being offline is the whole finding.
    Unreachable,
    /// The credential was rejected: revoke it and leave reachability untouched.
    Unauthorized,
    /// Anything else the host or transport answered; still offline, with the code kept.
    Failed(String),
}

/// Classifies a Capabilities result. A lost direct path is never seen here: `http` has
/// already retried an idempotent request over the relay, so only the relay's answer counts.
pub(crate) fn probe_outcome(result: &Result<m::Capabilities>, host: &HostView) -> ProbeOutcome {
    match result {
        Ok(caps) if caps.machine_id == host.machine_id => ProbeOutcome::Online,
        Ok(_) => ProbeOutcome::Failed("PAIRED_HOST_WRONG_MACHINE".into()),
        Err(error) => match error.code.as_str() {
            "HOST_UNAVAILABLE" | "TIMEOUT" | "MACHINE_SERVICE_UNAVAILABLE" => {
                ProbeOutcome::Unreachable
            }
            // The relay answers 404 with no body when the machine has no live control
            // channel (relay_server.rs `proxy_http`/socket-ticket paths). A bodyless 404 is
            // mapped with a synthesized machine_error, so machine_error cannot tell the two
            // apart. request_id can: it is Some exactly when a JSON error envelope was parsed
            // (its id may be empty), and None for the bodyless relay answer.
            "NOT_FOUND" if error.request_id.is_none() => ProbeOutcome::Unreachable,
            "UNAUTHORIZED" | "TOKEN_EXPIRED" => ProbeOutcome::Unauthorized,
            other => ProbeOutcome::Failed(other.to_owned()),
        },
    }
}

fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    serde_json::from_slice(bytes).map_err(|_| ClientError::local("PAIRED_HOST_INVALID_RESPONSE"))
}
/// Stores a token the host rotated on this reconnect for the lease's exact credential.
async fn adopt_rotation(
    service: &PairedHostService,
    lease: &CredentialLease,
    rotated: Option<String>,
) {
    let (Some(fresh), Ok(previous)) = (rotated, lease.token()) else {
        return;
    };
    service
        .adopt_rotated_token(
            lease.host_id().to_owned(),
            lease.generation(),
            previous.to_owned(),
            fresh,
        )
        .await;
}
struct Route {
    method: Method,
    segments: Vec<String>,
    query: Vec<(String, String)>,
    body: Option<Vec<u8>>,
    capability: Option<&'static str>,
    request_id: Option<String>,
    response_limit: usize,
    /// Safe to resend over the relay after a direct transport loss.
    idempotent: bool,
    /// Overrides the default HTTP budget. `wait` needs up to five minutes.
    budget_secs: Option<u64>,
}
impl Operation {
    pub fn is_mutation(&self) -> bool {
        if let Self::AgentControl { op } = self {
            return op.is_input_mutation();
        }
        matches!(
            self,
            Self::RegisterProject { .. }
                | Self::UnregisterProject { .. }
                | Self::CreateWorktree { .. }
                | Self::DeleteWorktree { .. }
                | Self::CreateSession { .. }
                | Self::CloseSession { .. }
        )
    }

    /// The caller-supplied id of an agent-control mutation, when it has one.
    ///
    /// This is not a workspace-journal id: agent control must not enter that journal, because a
    /// pending record there becomes an `OutcomeUnknown` workspace operation on restart.
    pub fn agent_control_mutation_id(&self) -> Option<String> {
        match self {
            Self::AgentControl { op } if op.is_input_mutation() => op.mutation_request_id(),
            _ => None,
        }
    }

    fn route(&self) -> Result<Route> {
        let mut r = Route {
            method: Method::GET,
            segments: vec![],
            query: vec![],
            body: None,
            capability: None,
            request_id: None,
            response_limit: m::MACHINE_JSON_MAX_BYTES,
            idempotent: true,
            budget_secs: None,
        };
        let mut path;
        match self {
            Self::Capabilities => {
                path = "capabilities";
            }
            Self::Directories {
                path: p,
                include_hidden,
            } => {
                path = "fs/directories";
                r.capability = Some("directoryBrowseV1");
                if let Some(p) = p {
                    r.query.push(("path".into(), p.clone()));
                }
                r.query
                    .push(("includeHidden".into(), include_hidden.to_string()));
            }
            Self::DiscoverRepos {
                path: p,
                depth,
                include_hidden,
            } => {
                path = "fs/repositories";
                r.capability = Some(crate::remote::repo_discovery::REPO_DISCOVERY_CAPABILITY);
                r.response_limit = m::DIRECTORY_JSON_MAX_BYTES;
                if let Some(p) = p {
                    r.query.push(("path".into(), p.clone()));
                }
                if let Some(depth) = depth {
                    if !(1..=crate::remote::repo_discovery::MAX_DEPTH).contains(depth) {
                        return Err(ClientError::local("INVALID_REQUEST"));
                    }
                    r.query.push(("depth".into(), depth.to_string()));
                }
                r.query
                    .push(("includeHidden".into(), include_hidden.to_string()));
            }
            Self::Projects | Self::RegisterProject { .. } | Self::UnregisterProject { .. } => {
                path = "workspace/projects";
                r.capability = Some("machineWorkspaceV1");
            }
            Self::WorktreeStatus {
                workspace_id,
                worktree,
            } => {
                path = "workspace/worktrees/status";
                r.capability = Some("managedWorktreesV1");
                r.query.extend([
                    ("workspaceId".into(), workspace_id.clone()),
                    ("wsId".into(), worktree.ws_id.clone()),
                    ("slug".into(), worktree.slug.clone()),
                ]);
            }
            Self::Worktrees { workspace_id } => {
                path = "workspace/worktrees";
                r.capability = Some("managedWorktreesV1");
                r.query.push(("workspaceId".into(), workspace_id.clone()));
            }
            Self::CreateWorktree { .. } | Self::DeleteWorktree { .. } => {
                path = "workspace/worktrees";
                r.capability = Some("managedWorktreesV1");
            }
            Self::Sessions { workspace_id } => {
                path = "sessions";
                r.capability = Some("terminalCreateV1");
                if let Some(id) = workspace_id {
                    r.query.push(("workspaceId".into(), id.clone()));
                }
            }
            Self::Session { daemon_epoch, .. } => {
                path = "sessions";
                r.capability = Some("terminalCreateV1");
                r.query
                    .push(("daemonEpoch".into(), daemon_epoch.0.to_string()));
            }
            Self::CreateSession { .. } | Self::CloseSession { .. } => {
                path = "sessions";
                r.capability = Some("terminalCreateV1");
            }
            Self::Operation { .. } => {
                path = "workspace/operations";
                r.capability = Some("machineWorkspaceV1");
            }
            Self::PasteUploadChunk { .. } => {
                path = "workspace/paste-upload";
                r.capability = Some("machineWorkspaceV1");
            }
            Self::FileRead { .. } => {
                path = "workspace/file-read";
                r.capability = Some(super::file_read::PAIRED_FILE_READ_CAPABILITY);
                r.response_limit = m::FILE_READ_JSON_MAX_BYTES;
            }
            Self::DagStream { .. } => {
                path = "capabilities";
                r.capability = Some(crate::remote::dag_api::DAG_STREAM_CAPABILITY);
            }
            Self::AgentControl { op } => {
                path = "agent-control/sessions";
                r.capability = Some(crate::agent_control::AGENT_CONTROL_REMOTE_CAPABILITY);
                match op {
                    crate::agent_control::AgentControlOp::List => {}
                    crate::agent_control::AgentControlOp::Read { session_id }
                    | crate::agent_control::AgentControlOp::Send { session_id, .. }
                    | crate::agent_control::AgentControlOp::Answer { session_id, .. }
                    | crate::agent_control::AgentControlOp::Stop { session_id, .. }
                    | crate::agent_control::AgentControlOp::Wait { session_id, .. } => {
                        if !crate::agent_transcript::is_valid_session_id(session_id) {
                            return Err(ClientError::local("INVALID_REQUEST"));
                        }
                    }
                    crate::agent_control::AgentControlOp::Events { after } => {
                        path = "agent-control/events";
                        r.query.push(("after".into(), after.to_string()));
                    }
                }
                if let crate::agent_control::AgentControlOp::Wait {
                    until,
                    after_revision,
                    after,
                    timeout_ms,
                    ..
                } = op
                {
                    if !crate::agent_control::events::until_is_known(until) || *timeout_ms == 0 {
                        return Err(ClientError::local("INVALID_REQUEST"));
                    }
                    let timeout_ms = (*timeout_ms)
                        .min(crate::agent_control::policy::AGENT_CONTROL_TIMEOUT_MAX_MS);
                    r.query.push(("until".into(), until.clone()));
                    r.query.push(("after".into(), after.to_string()));
                    r.query.push(("timeoutMs".into(), timeout_ms.to_string()));
                    if let Some(revision) = after_revision {
                        r.query.push(("revision".into(), revision.clone()));
                    }
                    r.budget_secs = Some(timeout_ms.div_ceil(1000).saturating_add(15).min(315));
                }
            }
        }
        r.segments = path.split('/').map(str::to_owned).collect();
        match self {
            Self::UnregisterProject { workspace_id, .. } => r.segments.push(workspace_id.clone()),
            Self::Session { session_id, .. } | Self::CloseSession { session_id, .. } => {
                r.segments.push(session_id.clone())
            }
            Self::AgentControl { op } => match op {
                crate::agent_control::AgentControlOp::Read { session_id }
                | crate::agent_control::AgentControlOp::Send { session_id, .. }
                | crate::agent_control::AgentControlOp::Answer { session_id, .. }
                | crate::agent_control::AgentControlOp::Stop { session_id, .. }
                | crate::agent_control::AgentControlOp::Wait { session_id, .. } => {
                    r.segments.push(session_id.clone());
                    if matches!(op, crate::agent_control::AgentControlOp::Wait { .. }) {
                        r.segments.push("wait".into());
                    } else if matches!(op, crate::agent_control::AgentControlOp::Send { .. }) {
                        r.segments.push("send".into());
                    } else if matches!(op, crate::agent_control::AgentControlOp::Answer { .. }) {
                        r.segments.push("answer".into());
                    } else if matches!(op, crate::agent_control::AgentControlOp::Stop { .. }) {
                        r.segments.push("stop".into());
                    }
                }
                crate::agent_control::AgentControlOp::List
                | crate::agent_control::AgentControlOp::Events { .. } => {}
            },
            Self::Operation { request_id } => {
                if uuid::Uuid::parse_str(request_id).is_err() {
                    return Err(ClientError::local("INVALID_REQUEST"));
                }
                r.segments.push(request_id.clone());
            }
            _ => {}
        }
        macro_rules! body {
            ($request:expr, $method:expr) => {{
                r.method = $method;
                r.request_id = Some($request.request_id.clone());
                r.body = Some(
                    serde_json::to_vec($request)
                        .map_err(|_| ClientError::local("INVALID_REQUEST"))?,
                );
            }};
        }
        match self {
            Self::RegisterProject { request } => body!(request, Method::POST),
            Self::UnregisterProject { request, .. } => body!(request, Method::DELETE),
            Self::CreateWorktree { request } => body!(request, Method::POST),
            Self::DeleteWorktree { request } => body!(request, Method::DELETE),
            Self::CreateSession { request } => body!(request, Method::POST),
            Self::CloseSession { request, .. } => body!(request, Method::DELETE),
            Self::PasteUploadChunk { request } => body!(request, Method::POST),
            Self::FileRead { request } => {
                r.method = Method::POST;
                r.body = Some(
                    serde_json::to_vec(request)
                        .map_err(|_| ClientError::local("INVALID_REQUEST"))?,
                );
            }
            Self::AgentControl { op } if op.is_input_mutation() => {
                r.method = Method::POST;
                // The request id stays off `route.request_id` so the workspace journal is not
                // consulted. Transport loss is relabeled from `agent_control_mutation_id`.
                r.body = Some(
                    serde_json::to_vec(&agent_control_body(op))
                        .map_err(|_| ClientError::local("INVALID_REQUEST"))?,
                );
            }
            _ => {}
        }
        if r.body
            .as_ref()
            .is_some_and(|b| b.len() > m::MACHINE_JSON_MAX_BYTES)
        {
            return Err(ClientError::local("PAYLOAD_TOO_LARGE"));
        }
        r.idempotent = r.method == Method::GET || matches!(self, Self::FileRead { .. });
        if r.segments
            .iter()
            .any(|s| s.is_empty() || s == "." || s == ".." || s.chars().any(char::is_control))
        {
            return Err(ClientError::local("INVALID_REQUEST"));
        }
        Ok(r)
    }
}
fn is_relay_transport(url: &Url) -> bool {
    url.path_segments()
        .and_then(|mut s| s.next())
        .is_some_and(|first| first == "host")
}

fn classify_content_type(header: Option<&reqwest::header::HeaderValue>) -> &'static str {
    match header.and_then(|v| v.to_str().ok()) {
        Some(v) => {
            let primary = v.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
            if primary == "application/json" {
                "json"
            } else if primary.starts_with("text/") {
                "text"
            } else if !primary.is_empty() {
                "other"
            } else {
                "none"
            }
        }
        None => "none",
    }
}

fn map_ticket_error(status: reqwest::StatusCode, bytes: &[u8]) -> ClientError {
    // The relay refuses plaintext (`E2E_REQUIRED`): this host must be re-paired.
    if status.as_u16() == 426 {
        return ClientError::local("REPAIR_REQUIRED");
    }
    if let Ok(envelope) = serde_json::from_slice::<m::ErrorEnvelope>(bytes) {
        let mut error = project_remote_error(envelope.error);
        if !error.details.contains_key("status") {
            error
                .details
                .insert("status".into(), serde_json::json!(status.as_u16()));
        }
        if !error.details.contains_key("httpStatus") {
            error
                .details
                .insert("httpStatus".into(), serde_json::json!(status.as_u16()));
        }
        return ClientError {
            code: error.code.clone(),
            request_id: if error.request_id.is_empty() {
                None
            } else {
                Some(error.request_id.clone())
            },
            machine_error: Some(Box::new(error)),
            ambiguous: false,
        };
    }
    if let Ok(error) = serde_json::from_slice::<m::MachineError>(bytes) {
        let mut error = project_remote_error(error);
        if !error.details.contains_key("status") {
            error
                .details
                .insert("status".into(), serde_json::json!(status.as_u16()));
        }
        if !error.details.contains_key("httpStatus") {
            error
                .details
                .insert("httpStatus".into(), serde_json::json!(status.as_u16()));
        }
        return ClientError {
            code: error.code.clone(),
            request_id: if error.request_id.is_empty() {
                None
            } else {
                Some(error.request_id.clone())
            },
            machine_error: Some(Box::new(error)),
            ambiguous: false,
        };
    }
    let code = match status.as_u16() {
        401 => "UNAUTHORIZED",
        403 => "PERMISSION_DENIED",
        404 => "NOT_FOUND",
        429 => "RATE_LIMITED",
        503 => "MACHINE_SERVICE_UNAVAILABLE",
        504 => "TIMEOUT",
        400 => "INVALID_REQUEST",
        _ => "PAIRED_HOST_REMOTE_ERROR",
    };
    let message = if let Ok(v) = serde_json::from_slice::<serde_json::Value>(bytes) {
        if let Some(err_str) = v.get("error").and_then(|e| e.as_str()) {
            err_str.to_string()
        } else {
            format!(
                "The paired host ticket request failed with HTTP status {}",
                status.as_u16()
            )
        }
    } else {
        format!(
            "The paired host ticket request failed with HTTP status {}",
            status.as_u16()
        )
    };
    let mut details = serde_json::Map::new();
    details.insert("status".into(), serde_json::json!(status.as_u16()));
    details.insert("httpStatus".into(), serde_json::json!(status.as_u16()));
    let retryable = matches!(status.as_u16(), 429 | 503 | 504);
    let machine_error = m::MachineError {
        code: code.to_string(),
        message,
        retryable,
        request_id: String::new(),
        details,
    };
    ClientError {
        code: code.to_string(),
        request_id: None,
        machine_error: Some(Box::new(machine_error)),
        ambiguous: false,
    }
}

fn sanitize_relay_stage(raw: &str) -> Option<&'static str> {
    match raw {
        crate::remote::relay_server::RELAY_STAGE_DATA_PAIRING_TIMEOUT => {
            Some(crate::remote::relay_server::RELAY_STAGE_DATA_PAIRING_TIMEOUT)
        }
        crate::remote::relay_server::RELAY_STAGE_UPSTREAM_TRANSFER_TIMEOUT => {
            Some(crate::remote::relay_server::RELAY_STAGE_UPSTREAM_TRANSFER_TIMEOUT)
        }
        _ => None,
    }
}

fn map_operation_http_error(
    status: reqwest::StatusCode,
    content_type: Option<&reqwest::header::HeaderValue>,
    relay_stage: Option<&str>,
    bytes: &[u8],
) -> ClientError {
    // The relay refuses plaintext (`E2E_REQUIRED`): this host must be re-paired.
    if status.as_u16() == 426 {
        return ClientError::local("REPAIR_REQUIRED");
    }
    if let Ok(envelope) = serde_json::from_slice::<m::ErrorEnvelope>(bytes) {
        let error = project_remote_error(envelope.error);
        return ClientError {
            code: error.code.clone(),
            request_id: Some(error.request_id.clone()),
            machine_error: Some(Box::new(error)),
            ambiguous: false,
        };
    }
    let code = match status.as_u16() {
        401 => "UNAUTHORIZED",
        403 => "PERMISSION_DENIED",
        404 => "NOT_FOUND",
        429 => "RATE_LIMITED",
        502 => "HOST_UNAVAILABLE",
        503 => "MACHINE_SERVICE_UNAVAILABLE",
        504 => "TIMEOUT",
        400 => "INVALID_REQUEST",
        _ => "PAIRED_HOST_REMOTE_ERROR",
    };
    let message = format!(
        "The paired host operation failed with HTTP status {}",
        status.as_u16()
    );
    let mut details = serde_json::Map::new();
    details.insert("status".into(), serde_json::json!(status.as_u16()));
    details.insert("httpStatus".into(), serde_json::json!(status.as_u16()));
    details.insert(
        "contentTypeClass".into(),
        serde_json::json!(classify_content_type(content_type)),
    );
    if status.as_u16() == 504 {
        if let Some(stage) = relay_stage {
            details.insert("relayStage".into(), serde_json::json!(stage));
        }
    }
    let retryable = matches!(status.as_u16(), 429 | 503 | 504);
    let machine_error = m::MachineError {
        code: code.to_string(),
        message,
        retryable,
        request_id: String::new(),
        details,
    };
    ClientError {
        code: code.to_string(),
        request_id: None,
        machine_error: Some(Box::new(machine_error)),
        ambiguous: false,
    }
}

/// A live authenticated DAG stream. Holding the lease keeps the stream bound to the
/// generation that authorized it: a forget or re-pair cancels it.
pub struct PairedDagStream {
    pub socket: super::secure_http::PairedSocket,
    pub lease: CredentialLease,
}

/// `/api/v1/{segments}?{query}` exactly as the gateway sees it, percent-encoded the same
/// way the network path encodes it.
fn gateway_path(segments: &[String], query: &[(String, String)]) -> Result<String> {
    let mut url = Url::parse("http://localhost/").map_err(|_| ClientError::local("INVALID_REQUEST"))?;
    url.path_segments_mut()
        .map_err(|_| ClientError::local("INVALID_REQUEST"))?
        .pop_if_empty()
        .extend(segments.iter().map(String::as_str));
    if !query.is_empty() {
        url.query_pairs_mut()
            .extend_pairs(query.iter().map(|(k, v)| (k, v)));
    }
    Ok(match url.query() {
        Some(query) => format!("{}?{query}", url.path()),
        None => url.path().to_owned(),
    })
}

pub struct MachineClient {
    /// Relay origins: honors the HTTP proxy policy.
    http: reqwest::Client,
    /// The published direct path: never proxied, however its address looks.
    direct_http: reqwest::Client,
}
impl Default for MachineClient {
    fn default() -> Self {
        Self::new()
    }
}
impl MachineClient {
    pub fn new() -> Self {
        Self::with_relay_proxy(crate::remote::relay_client::proxy::reqwest_proxy())
    }
    /// Proxy use is decided by path kind, not by address: only relay traffic may cross the
    /// HTTP proxy; the direct path never does.
    fn with_relay_proxy(proxy: reqwest::Proxy) -> Self {
        let builder = || {
            reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(65))
        };
        Self {
            http: builder().proxy(proxy).build().expect("native machine HTTP"),
            direct_http: builder().no_proxy().build().expect("native machine HTTP"),
        }
    }
    fn http_for(&self, relay: bool) -> &reqwest::Client {
        if relay {
            &self.http
        } else {
            &self.direct_http
        }
    }
    /// Reattach never creates a session. Authorization and target validation use
    /// the same executor as native HTTP operations before acquiring the socket.
    pub async fn attach_terminal(
        &self,
        service: &PairedHostService,
        descriptor: &crate::terminal::paired_daemon::Descriptor,
    ) -> Result<crate::terminal::paired_daemon::Transport> {
        let response = self
            .execute_with_capability(
                service,
                OperationRequest {
                    host_id: descriptor.host_id.clone(),
                    generation: descriptor.generation,
                    operation: Operation::Session {
                        session_id: descriptor.target.session_id.clone(),
                        daemon_epoch: descriptor.target.daemon_epoch,
                    },
                },
                Some("terminalStreamV1"),
            )
            .await?;
        let remote_pid = match response.result {
            OperationResult::Session(m::SessionDetail::Running { session })
                if session.target == descriptor.target => session.pid,
            OperationResult::Session(m::SessionDetail::Exited { session, .. })
                if session.target == descriptor.target => return Err(ClientError::local("PAIRED_SESSION_EXITED")),
            OperationResult::Session(m::SessionDetail::Expired { target })
                if target == descriptor.target => return Err(ClientError::local("SESSION_EXPIRED")),
            _ => return Err(ClientError::local("PAIRED_HOST_WRONG_MACHINE")),
        };
        let (host, lease) = service
            .capture_operation(descriptor.host_id.clone(), descriptor.generation)
            .await?;
        let segments = [
            "api".to_owned(),
            "v1".to_owned(),
            "terminal".to_owned(),
            descriptor.target.session_id.clone(),
        ];
        let mut query = vec![(
            "daemonEpoch".to_owned(),
            descriptor.target.daemon_epoch.0.to_string(),
        )];
        if let Some(cursor) = descriptor.after_sequence {
            query.push(("afterSequence".to_owned(), cursor.0.to_string()));
        }
        let target_path = format!("/api/v1/terminal/{}", descriptor.target.session_id);
        let (socket, direct_epoch) = self
            .open_socket(
                &host,
                &lease,
                &segments,
                &query,
                &target_path,
                crate::remote::terminal_wire::MAX_FRAME_BYTES,
            )
            .await?;
        service
            .current_generation(descriptor.host_id.clone(), descriptor.generation)
            .await?;
        Ok(crate::terminal::paired_daemon::Transport {
            socket,
            lease,
            direct_epoch,
            remote_pid,
        })
    }

    /// Opens an authenticated DAG stream on the paired host.
    ///
    /// Identity is bound before any frame is read: the capability probe runs through
    /// the same executor as every other machine operation, so a host whose
    /// `machineId` does not match the inventory record, or whose grant is not
    /// machine/control, is refused rather than streamed from. The workspace id is the
    /// host's own id; the desktop never supplies a filesystem path.
    pub async fn attach_dag(
        &self,
        service: &PairedHostService,
        host_id: String,
        generation: Epoch,
        remote_workspace_id: &str,
    ) -> Result<PairedDagStream> {
        // Capability + identity gate. `execute_with_capability` verifies
        // caps.machine_id == host.machine_id and machine/control scope.
        self.execute_with_capability(
            service,
            OperationRequest {
                host_id: host_id.clone(),
                generation,
                operation: Operation::DagStream {
                    workspace_id: remote_workspace_id.to_owned(),
                },
            },
            Some(crate::remote::dag_api::DAG_STREAM_CAPABILITY),
        )
        .await?;

        let (host, lease) = service.capture_operation(host_id, generation).await?;
        let (socket, _) = self
            .open_socket(
                &host,
                &lease,
                &["api", "v1", "workspace", "dag"].map(str::to_owned),
                &[("workspaceId".to_owned(), remote_workspace_id.to_owned())],
                crate::remote::dag_api::DAG_SOCKET_TARGET,
                crate::remote::dag_api::MAX_DAG_FRAME_BYTES,
            )
            .await?;
        // Re-verify the generation: a forget or re-pair during the upgrade must not
        // leave a live stream authorized by a retired credential.
        service
            .current_generation(host.host_id.clone(), generation)
            .await?;
        Ok(PairedDagStream { socket, lease })
    }

    /// Opens a ticketed WebSocket on the paired host. The ticket and the socket
    /// share one base (direct adapter or relay); a transport failure over the direct
    /// path reports the loss and retries once over the relay. Upgrades are
    /// read-only attachments, so the retry cannot repeat a mutation. Returns the
    /// direct epoch the socket actually rides (`None` after a relay fallback).
    async fn open_socket(
        &self,
        host: &HostView,
        lease: &CredentialLease,
        segments: &[String],
        query: &[(String, String)],
        ticket_target: &str,
        max_frame: usize,
    ) -> Result<(super::secure_http::PairedSocket, Option<u64>)> {
        let base = super::direct_client::request_base(host)?;
        if base.direct_epoch.is_none() {
            return self
                .open_relay_socket(host, lease, &base.url, segments, query, ticket_target, max_frame)
                .await
                .map(|socket| (socket, None));
        }
        let first = self
            .open_socket_at(
                &base.url,
                false,
                lease,
                segments,
                query,
                ticket_target,
                max_frame,
            )
            .await;
        match (first, base.direct_epoch) {
            (Err(error), Some(epoch)) if super::direct_client::is_transport_failure(&error) => {
                super::direct_client::report_direct_loss(host, epoch);
                let relay = Url::parse(&crate::remote::relay_client::live_relay_url(&host.host_id))
                    .map_err(|_| ClientError::local("INVALID_REQUEST"))?;
                self.open_relay_socket(host, lease, &relay, segments, query, ticket_target, max_frame)
                    .await
                    .map(|socket| (socket, None))
            }
            (result, epoch) => result.map(|socket| (socket, epoch)),
        }
    }

    /// A relay socket: end-to-end encrypted when this generation was paired with the
    /// machine's attach key (the bearer travels inside the tunnel, no ticket), else the
    /// legacy plaintext relay path with a one-time `REPAIR_REQUIRED` warning.
    #[allow(clippy::too_many_arguments)]
    async fn open_relay_socket(
        &self,
        host: &HostView,
        lease: &CredentialLease,
        relay: &Url,
        segments: &[String],
        query: &[(String, String)],
        ticket_target: &str,
        max_frame: usize,
    ) -> Result<super::secure_http::PairedSocket> {
        let Some(attach) = lease.attach() else {
            super::secure_http::warn_repair_required(&host.host_id, lease.generation().0);
            return self
                .open_socket_at(
                    relay,
                    true,
                    lease,
                    segments,
                    query,
                    ticket_target,
                    max_frame,
                )
                .await;
        };
        let path = gateway_path(segments, query)?;
        let config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
            .max_message_size(Some(max_frame))
            .max_frame_size(Some(max_frame));
        let bearer = lease.token()?.to_owned();
        let relay_origin = crate::remote::relay_client::live_relay_url(&host.relay_origin);
        let target = super::secure_http::SecureTarget {
            http: &self.http,
            host_id: &host.host_id,
            generation: lease.generation().0,
            relay_origin: &relay_origin,
            machine_id: &host.machine_id,
            attach,
            bearer: &bearer,
        };
        let mut cancelled = lease.cancellation();
        let result = tokio::select! { biased;
            _ = cancelled.changed() => Err(ClientError::local("PAIRED_HOST_STALE_GENERATION")),
            result = super::secure_http::open_secure_websocket(&target, &path, config) => result,
        };
        match result {
            Err(error) if error.code == "E2E_UNSUPPORTED" => {
                super::secure_http::warn_e2e_unsupported(&host.host_id, lease.generation().0);
                self.open_socket_at(
                    relay,
                    true,
                    lease,
                    segments,
                    query,
                    ticket_target,
                    max_frame,
                )
                .await
            }
            other => other,
        }
    }

    /// `relay` is the path kind: only a relay socket may go through the HTTP proxy.
    #[allow(clippy::too_many_arguments)]
    async fn open_socket_at(
        &self,
        base: &Url,
        relay: bool,
        lease: &CredentialLease,
        segments: &[String],
        query: &[(String, String)],
        ticket_target: &str,
        max_frame: usize,
    ) -> Result<super::secure_http::PairedSocket> {
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;
        let mut url = base.clone();
        url.path_segments_mut()
            .map_err(|_| ClientError::local("INVALID_REQUEST"))?
            .extend(segments.iter().map(String::as_str));
        url.query_pairs_mut()
            .extend_pairs(query.iter().map(|(k, v)| (k, v)));
        // Mint a single-use socket ticket if the endpoint supports it (relay requires one).
        if let Some(ticket) = self
            .socket_ticket(base, relay, lease, ticket_target)
            .await?
        {
            url.query_pairs_mut().append_pair("ticket", &ticket);
        }
        let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
        url.set_scheme(scheme)
            .map_err(|_| ClientError::local("INVALID_REQUEST"))?;
        let mut request = url
            .as_str()
            .into_client_request()
            .map_err(|_| ClientError::local("INVALID_REQUEST"))?;
        request.headers_mut().insert(
            "authorization",
            format!("Bearer {}", lease.token()?)
                .parse()
                .map_err(|_| ClientError::local("INVALID_REQUEST"))?,
        );
        if is_relay_transport(base) {
            request.headers_mut().insert(
                super::secure_http::CLIENT_BUILD_HEADER,
                tokio_tungstenite::tungstenite::http::HeaderValue::from_static(
                    super::secure_http::CLIENT_BUILD,
                ),
            );
        }
        let mut cancelled = lease.cancellation();
        let config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
            .max_message_size(Some(max_frame))
            .max_frame_size(Some(max_frame));
        let socket = tokio::select! { biased;
            _ = cancelled.changed() => return Err(ClientError::local("PAIRED_HOST_STALE_GENERATION")),
            result = tokio::time::timeout(Duration::from_secs(30), super::secure_http::connect_net_websocket(request, config, relay)) =>
                result.map_err(|_| ClientError::local("TIMEOUT"))?.map_err(|e| {
                    tracing::error!("Failed to connect to paired host websocket: {:?}", e);
                    ClientError::local("HOST_UNAVAILABLE")
                })?,
        };
        Ok(socket)
    }

    /// Mints a single-use socket ticket for `target`, mirroring terminal attachment.
    /// A direct (non-relay) host that predates tickets answers 404 and is allowed to
    /// fall back to the Authorization header; a relay never is.
    async fn socket_ticket(
        &self,
        base: &Url,
        relay: bool,
        lease: &CredentialLease,
        target: &str,
    ) -> Result<Option<String>> {
        let is_relay = is_relay_transport(base);
        let mut url = base.clone();
        url.path_segments_mut()
            .map_err(|_| ClientError::local("INVALID_REQUEST"))?
            .extend(["api", "v1", "socket-ticket"]);
        let body = serde_json::json!({ "target": target }).to_string();
        let mut request = self
            .http_for(relay)
            .post(url)
            .bearer_auth(lease.token()?)
            .header("content-type", "application/json")
            .body(body);
        if is_relay {
            request = request.header(
                super::secure_http::CLIENT_BUILD_HEADER,
                super::secure_http::CLIENT_BUILD,
            );
        }
        let response = request
            .send()
            .await
            .map_err(|error| {
                ClientError::local(if error.is_timeout() {
                    "TIMEOUT"
                } else {
                    "HOST_UNAVAILABLE"
                })
            })?;
        let status = response.status();
        if status.is_success() {
            let mut bytes = Vec::new();
            let mut response = response;
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| ClientError::local("HOST_UNAVAILABLE"))?
            {
                if bytes.len() + chunk.len() > 64 * 1024 {
                    return Err(ClientError::local("PAYLOAD_TOO_LARGE"));
                }
                bytes.extend_from_slice(&chunk);
            }
            #[derive(serde::Deserialize)]
            struct TicketResp {
                ticket: String,
            }
            let parsed = serde_json::from_slice::<TicketResp>(&bytes)
                .map_err(|_| ClientError::local("PAIRED_HOST_INVALID_RESPONSE"))?;
            if parsed.ticket.is_empty() || parsed.ticket.len() > 1024 {
                return Err(ClientError::local("PAIRED_HOST_INVALID_RESPONSE"));
            }
            return Ok(Some(parsed.ticket));
        }
        if !is_relay && status == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let mut bytes = Vec::new();
        let mut response = response;
        while let Ok(Some(chunk)) = response.chunk().await {
            if bytes.len() + chunk.len() > 16 * 1024 {
                break;
            }
            bytes.extend_from_slice(&chunk);
        }
        Err(map_ticket_error(status, &bytes))
    }

    /// Sends one machine request over the direct adapter when one is published,
    /// else the relay. A transport failure over direct reports the loss; only an
    /// idempotent route is retried over the relay, so a mutation is never sent twice.
    async fn http(
        &self,
        host: &HostView,
        lease: &CredentialLease,
        route: &Route,
        directory: bool,
    ) -> Result<Vec<u8>> {
        self.http_with_rotation(host, lease, route, directory)
            .await
            .map(|(bytes, _)| bytes)
    }

    /// The reconnect probe: also answers the replacement token the host rotated, if any.
    async fn http_rotating(
        &self,
        host: &HostView,
        lease: &CredentialLease,
        route: &Route,
    ) -> Result<(Vec<u8>, Option<String>)> {
        self.http_with_rotation(host, lease, route, false).await
    }

    async fn http_with_rotation(
        &self,
        host: &HostView,
        lease: &CredentialLease,
        route: &Route,
        directory: bool,
    ) -> Result<(Vec<u8>, Option<String>)> {
        let base = super::direct_client::request_base(host)?;
        if base.direct_epoch.is_none() {
            return self.http_relay(host, base.url, lease, route, directory).await;
        }
        let first = self.http_at(base.url, false, lease, route, directory).await;
        match (first, base.direct_epoch) {
            (Err(error), Some(epoch)) if super::direct_client::is_transport_failure(&error) => {
                super::direct_client::report_direct_loss(host, epoch);
                if !route.idempotent {
                    return Err(error);
                }
                let relay = Url::parse(&crate::remote::relay_client::live_relay_url(&host.host_id))
                    .map_err(|_| ClientError::local("INVALID_REQUEST"))?;
                self.http_relay(host, relay, lease, route, directory).await
            }
            (result, _) => result,
        }
    }

    /// A relay request: through the host's shared end-to-end encrypted session when this
    /// generation carries the machine attach key, else the legacy plaintext relay path.
    async fn http_relay(
        &self,
        host: &HostView,
        relay: Url,
        lease: &CredentialLease,
        route: &Route,
        directory: bool,
    ) -> Result<(Vec<u8>, Option<String>)> {
        match lease.attach() {
            Some(attach) => match self.http_secure(host, attach, lease, route, directory).await {
                Err(error) if error.code == "E2E_UNSUPPORTED" => {
                    super::secure_http::warn_e2e_unsupported(&host.host_id, lease.generation().0);
                    self.http_at(relay, true, lease, route, directory).await
                }
                other => other,
            },
            None => {
                super::secure_http::warn_repair_required(&host.host_id, lease.generation().0);
                self.http_at(relay, true, lease, route, directory).await
            }
        }
    }

    /// The same request, status, size, content-type, rotation and error handling as
    /// [`Self::http_at`], carried inside the encrypted tunnel to the machine's gateway.
    async fn http_secure(
        &self,
        host: &HostView,
        attach: &super::inventory::HostAttach,
        lease: &CredentialLease,
        route: &Route,
        directory: bool,
    ) -> Result<(Vec<u8>, Option<String>)> {
        let mut segments = vec!["api".to_owned(), "v1".to_owned()];
        segments.extend(route.segments.iter().cloned());
        let path = gateway_path(&segments, &route.query)?;
        let bearer = lease.token()?.to_owned();
        let mut headers = vec![
            ("Authorization", format!("Bearer {bearer}")),
            ("Accept", "application/json".to_owned()),
            ("X-Ferryx-Client-Build", super::secure_http::CLIENT_BUILD.to_owned()),
        ];
        if route.body.is_some() {
            headers.push(("Content-Type", "application/json".to_owned()));
        }
        let limit = if directory { m::DIRECTORY_JSON_MAX_BYTES } else { route.response_limit };
        let budget = route
            .budget_secs
            .unwrap_or(if route.body.is_some() { 60 } else { 45 });
        let relay_origin = crate::remote::relay_client::live_relay_url(&host.relay_origin);
        let target = super::secure_http::SecureTarget {
            http: &self.http,
            host_id: &host.host_id,
            generation: lease.generation().0,
            relay_origin: &relay_origin,
            machine_id: &host.machine_id,
            attach,
            bearer: &bearer,
        };
        let mut cancellation = lease.cancellation();
        let response = tokio::select! { biased;
            _ = cancellation.changed() => return Err(ClientError::local("PAIRED_HOST_STALE_GENERATION")),
            result = tokio::time::timeout(
                Duration::from_secs(budget),
                super::secure_http::shared_request(
                    &target,
                    route.method.as_str(),
                    &path,
                    &headers,
                    route.body.as_deref(),
                    limit,
                    route.idempotent,
                ),
            ) => result.map_err(|_| ClientError::local("TIMEOUT"))??,
        };
        let status = reqwest::StatusCode::from_u16(response.status)
            .map_err(|_| ClientError::local("PAIRED_HOST_INVALID_RESPONSE"))?;
        if status.is_redirection() {
            return Err(ClientError::local("PAIRED_HOST_REDIRECT_REJECTED"));
        }
        if status.as_u16() == 204 {
            return Ok((b"null".to_vec(), None));
        }
        let content_type = response
            .header("content-type")
            .and_then(|v| reqwest::header::HeaderValue::from_str(v).ok());
        let json = response
            .header("content-type")
            .is_some_and(|v| v.split(';').next().map(str::trim) == Some("application/json"));
        if status.is_success() && !json {
            return Err(ClientError::local("PAIRED_HOST_INVALID_RESPONSE"));
        }
        let rotated = response
            .header(crate::remote::server::ROTATED_TOKEN_HEADER)
            .map(str::to_owned);
        if !status.is_success() {
            return Err(map_operation_http_error(status, content_type.as_ref(), None, &response.body));
        }
        Ok((response.body, rotated))
    }

    /// `relay` is the path kind: only a relay request may go through the HTTP proxy.
    async fn http_at(
        &self,
        base: Url,
        relay: bool,
        lease: &CredentialLease,
        route: &Route,
        directory: bool,
    ) -> Result<(Vec<u8>, Option<String>)> {
        let is_relay = is_relay_transport(&base);
        let mut url = base;
        {
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| ClientError::local("INVALID_REQUEST"))?;
            segments.extend(["api", "v1"]);
            segments.extend(route.segments.iter().map(String::as_str));
        }
        if !route.query.is_empty() {
            url.query_pairs_mut()
                .extend_pairs(route.query.iter().map(|(k, v)| (k, v)));
        }
        let mut request = self
            .http_for(relay)
            .request(route.method.clone(), url)
            .bearer_auth(lease.token()?);
        if is_relay {
            request = request.header(
                super::secure_http::CLIENT_BUILD_HEADER,
                super::secure_http::CLIENT_BUILD,
            );
        }
        if let Some(body) = &route.body {
            request = request
                .header("content-type", "application/json")
                .body(body.clone());
        }
        let mut cancellation = lease.cancellation();
        // Read-only machine operations ride the relay tunnel, whose round trips can
        // legitimately exceed 10s under tunnel churn; mutation budget is larger because
        // remote git/filesystem calls on enterprise repos can take tens of seconds.
        // `wait` overrides this: its caller timeout is clamped to five minutes.
        let budget = route
            .budget_secs
            .unwrap_or(if route.body.is_some() { 60 } else { 45 });
        request = request.timeout(Duration::from_secs(budget));
        tokio::select! { biased;
            _=cancellation.changed()=>Err(ClientError::local("PAIRED_HOST_STALE_GENERATION")),
            result=tokio::time::timeout(Duration::from_secs(budget),async {
                let mut response=request.send().await.map_err(|_| ClientError::local("HOST_UNAVAILABLE"))?;
                let status=response.status();
                if status.is_redirection() { return Err(ClientError::local("PAIRED_HOST_REDIRECT_REJECTED")); }
                let limit=if directory {m::DIRECTORY_JSON_MAX_BYTES} else {route.response_limit};
                if response.content_length().is_some_and(|n| n>limit as u64) { return Err(ClientError::local("PAYLOAD_TOO_LARGE")); }
                if status.as_u16() == 204 { return Ok((b"null".to_vec(), None)); }
                let json=response.headers().get("content-type").and_then(|v| v.to_str().ok()).is_some_and(|v| v.split(';').next().map(str::trim)==Some("application/json"));
                if status.is_success() && !json { return Err(ClientError::local("PAIRED_HOST_INVALID_RESPONSE")); }
                let content_type = response.headers().get("content-type").cloned();
                let relay_stage = response
                    .headers()
                    .get(crate::remote::relay_server::RELAY_STAGE_HEADER)
                    .and_then(|v| v.to_str().ok())
                    .and_then(sanitize_relay_stage);
                let rotated = response
                    .headers()
                    .get(crate::remote::server::ROTATED_TOKEN_HEADER)
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_owned);
                let bytes=tokio::time::timeout(Duration::from_secs(budget),async {
                    let mut bytes=Vec::new();
                    while let Some(chunk)=response.chunk().await.map_err(|_| ClientError::local("HOST_UNAVAILABLE"))? {
                        if bytes.len()+chunk.len()>limit { return Err(ClientError::local("PAYLOAD_TOO_LARGE")); } bytes.extend_from_slice(&chunk);
                    } Ok(bytes)
                }).await.map_err(|_| ClientError::local("TIMEOUT"))??;
                if !status.is_success() { return Err(map_operation_http_error(status, content_type.as_ref(), relay_stage, &bytes)); }
                Ok((bytes, rotated))
            })=> result.map_err(|_|ClientError::local("TIMEOUT"))?
        }
    }
    /// Checks one paired host with a Capabilities round-trip and records the result.
    /// Returns the state now recorded for that generation (`Checking` when nothing was
    /// recorded: a stale generation, or a rejected credential that was revoked instead).
    pub async fn probe(
        &self,
        service: &PairedHostService,
        host_id: String,
        generation: Epoch,
    ) -> ReachabilityState {
        let Ok((host, lease)) = service.capture_operation(host_id.clone(), generation).await
        else {
            return ReachabilityState::Checking;
        };
        let result = match Operation::Capabilities.route() {
            Ok(route) => match self.http_rotating(&host, &lease, &route).await {
                Ok((bytes, rotated)) => {
                    adopt_rotation(service, &lease, rotated).await;
                    decode::<m::Capabilities>(&bytes)
                }
                Err(error) => Err(error),
            },
            Err(error) => Err(error),
        };
        drop(lease);
        let (state, last_error) = match probe_outcome(&result, &host) {
            ProbeOutcome::Online => (ReachabilityState::Online, None),
            ProbeOutcome::Unreachable => (ReachabilityState::Offline, None),
            ProbeOutcome::Failed(code) => (ReachabilityState::Offline, Some(code)),
            ProbeOutcome::Unauthorized => {
                service.revoke_on_auth_failure(host_id, generation).await;
                return ReachabilityState::Checking;
            }
        };
        if service
            .record_reachability(host_id, generation, state, last_error)
            .await
        {
            state
        } else {
            ReachabilityState::Checking
        }
    }

    pub async fn execute(
        &self,
        service: &PairedHostService,
        request: OperationRequest,
    ) -> Result<OperationResponse> {
        self.execute_with_capability(service, request, None).await
    }

    async fn execute_with_capability(
        &self,
        service: &PairedHostService,
        request: OperationRequest,
        additional_capability: Option<&str>,
    ) -> Result<OperationResponse> {
        // Roundtrip validates DTO custom deserializers for native as well as IPC callers.
        let request: OperationRequest = decode(
            &serde_json::to_vec(&request).map_err(|_| ClientError::local("INVALID_REQUEST"))?,
        )?;
        let route = request.operation.route()?;
        let (host, lease) = service
            .capture_operation(request.host_id.clone(), request.generation)
            .await?;
        let caps = match self
            .http_rotating(&host, &lease, &Operation::Capabilities.route()?)
            .await
        {
            Ok((bytes, rotated)) => {
                adopt_rotation(service, &lease, rotated).await;
                decode::<m::Capabilities>(&bytes)?
            }
            Err(error) => {
                // Only a "cannot reach the host" answer says anything about reachability.
                if probe_outcome(&Err(error.clone()), &host) == ProbeOutcome::Unreachable {
                    service
                        .record_reachability(
                            request.host_id.clone(),
                            request.generation,
                            ReachabilityState::Offline,
                            None,
                        )
                        .await;
                }
                return Err(error);
            }
        };
        if caps.machine_id != host.machine_id {
            return Err(ClientError::local("PAIRED_HOST_WRONG_MACHINE"));
        }
        // The operation path just reached the host itself.
        service
            .record_reachability(
                request.host_id.clone(),
                request.generation,
                ReachabilityState::Online,
                None,
            )
            .await;
        if caps.access_scope != m::AccessScope::Machine || caps.permission != m::Permission::Control
        {
            return Err(ClientError::local("MACHINE_ACCESS_REQUIRED"));
        }
        // Identity and scope are confirmed: negotiate the direct path in the background.
        super::direct_client::kick(&self.http, &host, &lease);
        let satisfies_capability = |required: &str| -> bool {
            if caps.capabilities.iter().any(|v| v == required) {
                return true;
            }
            // Compatibility for hosts advertising terminalCreateV1 before terminalStreamV1 was explicit.
            if required == "terminalStreamV1"
                && caps.capabilities.iter().any(|v| v == "terminalCreateV1")
            {
                #[cfg(test)]
                if caps.machine_id == "a" {
                    return false;
                }
                return true;
            }
            false
        };
        if route
            .capability
            .into_iter()
            .chain(additional_capability)
            .any(|c| !satisfies_capability(c))
        {
            return Err(ClientError::local("PAIRED_HOST_CAPABILITY_UNAVAILABLE"));
        }
        let bytes = if matches!(request.operation, Operation::Capabilities) {
            serde_json::to_vec(&caps).expect("caps")
        } else {
            // Every submission first queries the journal. Never synthesize a new ID,
            // automatically repeat a pending mutation, or infer not-found from transport failure.
            if let Some(id) = &route.request_id {
                let journal = self
                    .http(
                        &host,
                        &lease,
                        &Operation::Operation {
                            request_id: id.clone(),
                        }
                        .route()?,
                        false,
                    )
                    .await;
                match journal {
                    Ok(bytes) => {
                        let op: m::Operation = decode(&bytes)?;
                        validate_operation(&op, id)?;
                        // Journal lacks request digest: do not adopt a completed result for
                        // potentially different caller input. Server replay validates digest.
                        if !matches!(op, m::Operation::Completed { .. }) {
                            return Err(ClientError {
                                code: "OPERATION_OUTCOME_UNKNOWN".into(),
                                machine_error: None,
                                request_id: Some(id.clone()),
                                ambiguous: true,
                            });
                        }
                    }
                    Err(e) if e.code == "OPERATION_NOT_FOUND" => {}
                    Err(e) => return Err(e),
                }
            }
            match self
                .http(
                    &host,
                    &lease,
                    &route,
                    matches!(request.operation, Operation::Directories { .. }),
                )
                .await
            {
                Ok(bytes) => bytes,
                Err(mut e) => {
                    if let Some(id) = &route.request_id {
                        e.relabel_transport_uncertainty(id);
                    } else if let Some(id) = request.operation.agent_control_mutation_id() {
                        // Same id the caller sent. Do not mint another and do not call execute again.
                        e.relabel_transport_uncertainty(&id);
                        if e.machine_error.is_none() {
                            e.code = "OPERATION_OUTCOME_UNKNOWN".into();
                        }
                    } else if request.operation.is_mutation()
                        && matches!(request.operation, Operation::AgentControl { .. })
                        && e.machine_error.is_none()
                    {
                        e.ambiguous = true;
                        e.code = "OPERATION_OUTCOME_UNKNOWN".into();
                    }
                    return Err(e);
                }
            }
        };
        let result = map_result(&host, &request.operation, &bytes).map_err(|mut error| {
            if let Some(id) = &route.request_id {
                error.request_id = Some(id.clone());
                error.ambiguous = true;
                if let Ok(operation) = decode::<m::Operation>(&bytes) {
                    if validate_operation(&operation, id).is_ok() {
                        error.code = "OPERATION_OUTCOME_UNKNOWN".into();
                    }
                }
            }
            error
        })?;
        service
            .current_generation(request.host_id.clone(), request.generation)
            .await?;
        Ok(OperationResponse {
            host_id: request.host_id,
            generation: request.generation,
            result,
        })
    }
}
/// Daemon probe cadence for paired hosts.
pub const PROBE_INTERVAL: Duration = Duration::from_secs(30);
/// At most this many host probes run at once.
pub const PROBE_CONCURRENCY: usize = 4;

/// Whether the probe loop checks this host: paired, with a machine grant. Mirror-only,
/// revoked and unknown hosts are never probed.
pub(crate) fn is_probe_target(host: &HostView) -> bool {
    host.auth_status == AuthStatus::Paired && host.grant_scope == GrantScope::Machine
}

/// Probes every host that is paired with a machine grant, at most
/// [`PROBE_CONCURRENCY`] at a time (see [`is_probe_target`]).
pub async fn probe_paired_hosts(client: &MachineClient, service: &PairedHostService) {
    use futures_util::stream::{self, StreamExt};
    let Ok(hosts) = service.list().await else {
        return;
    };
    stream::iter(hosts.into_iter().filter(is_probe_target))
    .map(|host| client.probe(service, host.host_id, host.generation))
    .buffer_unordered(PROBE_CONCURRENCY)
    .for_each(|_| async {})
    .await;
}

/// Probes immediately, then on every `interval` tick (missed ticks are skipped), until
/// `alive` reports false. The daemon passes [`PROBE_INTERVAL`]; tests inject a short one.
pub async fn run_probe_loop(
    service: PairedHostService,
    interval: Duration,
    alive: impl Fn() -> bool,
) {
    let client = MachineClient::new();
    let mut ticks = tokio::time::interval(interval);
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        ticks.tick().await;
        if !alive() {
            break;
        }
        probe_paired_hosts(&client, &service).await;
    }
}

fn validate_operation(op: &m::Operation, expected: &str) -> Result<()> {
    let id = match op {
        m::Operation::Pending { request_id }
        | m::Operation::OutcomeUnknown { request_id }
        | m::Operation::ResultExpired { request_id }
        | m::Operation::Completed { request_id, .. } => request_id,
    };
    if id != expected {
        return Err(ClientError::local("PAIRED_HOST_INVALID_RESPONSE"));
    }
    Ok(())
}
fn session(host: &HostView, row: &m::Session) -> Result<()> {
    if row.target.machine_id != host.machine_id {
        Err(ClientError::local("PAIRED_HOST_WRONG_MACHINE"))
    } else {
        workspace_identity(&row.workspace_id, row.worktree.as_ref(), &row.workspace_id)
    }
}
fn workspace_identity(
    workspace: &str,
    worktree: Option<&m::WorktreeIdentity>,
    expected: &str,
) -> Result<()> {
    if workspace != expected || worktree.is_some_and(|w| w.ws_id != expected) {
        return Err(ClientError::local("PAIRED_HOST_INVALID_RESPONSE"));
    }
    Ok(())
}
fn map_result(host: &HostView, operation: &Operation, bytes: &[u8]) -> Result<OperationResult> {
    Ok(match operation {
        Operation::Capabilities => OperationResult::Capabilities(decode(bytes)?),
        Operation::Directories { .. } => OperationResult::Directories(decode(bytes)?),
        Operation::DiscoverRepos { .. } => {
            OperationResult::DiscoverRepos(discovered_repos(&host.host_id, decode(bytes)?)?)
        }
        Operation::Projects => {
            OperationResult::Projects(projects::projects(&host.host_id, decode(bytes)?))
        }
        Operation::RegisterProject { .. } => {
            OperationResult::RegisterProject(projects::project(&host.host_id, decode(bytes)?))
        }
        Operation::UnregisterProject { .. } => OperationResult::UnregisterProject(decode(bytes)?),
        Operation::Worktrees { workspace_id } => {
            let rows: m::Worktrees = decode(bytes)?;
            for row in &rows.worktrees {
                workspace_identity(&row.workspace_id, row.identity.as_ref(), workspace_id)?;
            }
            OperationResult::Worktrees(rows)
        }
        Operation::WorktreeStatus {
            workspace_id,
            worktree,
        } => {
            let row: m::WorktreeStatus = decode(bytes)?;
            workspace_identity(&row.workspace_id, Some(&row.worktree), workspace_id)?;
            if row.worktree != *worktree {
                return Err(ClientError::local("PAIRED_HOST_INVALID_RESPONSE"));
            }
            OperationResult::WorktreeStatus(row)
        }
        Operation::CreateWorktree { request } => {
            let row: m::Worktree = decode(bytes)?;
            workspace_identity(
                &row.workspace_id,
                row.identity.as_ref(),
                &request.workspace_id,
            )?;
            if row.identity.as_ref() != Some(&request.worktree) {
                return Err(ClientError::local("PAIRED_HOST_INVALID_RESPONSE"));
            }
            OperationResult::CreateWorktree(row)
        }
        Operation::DeleteWorktree { .. } => OperationResult::DeleteWorktree(decode(bytes)?),
        Operation::Sessions { workspace_id } => {
            let rows: m::Sessions = decode(bytes)?;
            for row in &rows.sessions {
                session(host, row)?;
                if let Some(expected) = workspace_id {
                    workspace_identity(&row.workspace_id, row.worktree.as_ref(), expected)?;
                }
            }
            if workspace_id.as_ref().is_some_and(|id| {
                rows.unavailable_workspace_ids
                    .iter()
                    .any(|other| other != id)
            }) {
                return Err(ClientError::local("PAIRED_HOST_INVALID_RESPONSE"));
            }
            OperationResult::Sessions(rows)
        }
        Operation::Session {
            session_id,
            daemon_epoch,
        } => {
            let row: m::SessionDetail = decode(bytes)?;
            let target = match &row {
                m::SessionDetail::Running { session }
                | m::SessionDetail::Exited { session, .. } => &session.target,
                m::SessionDetail::Expired { target } => target,
            };
            if target.machine_id != host.machine_id
                || target.session_id != *session_id
                || target.daemon_epoch != *daemon_epoch
            {
                return Err(ClientError::local("PAIRED_HOST_WRONG_MACHINE"));
            }
            OperationResult::Session(row)
        }
        Operation::CreateSession { request } => {
            let row = decode(bytes)?;
            session(host, &row)?;
            workspace_identity(
                &row.workspace_id,
                row.worktree.as_ref(),
                &request.workspace_id,
            )?;
            if row.worktree != request.worktree {
                return Err(ClientError::local("PAIRED_HOST_INVALID_RESPONSE"));
            }
            OperationResult::CreateSession(row)
        }
        Operation::CloseSession { .. } => OperationResult::CloseSession(decode(bytes)?),
        Operation::Operation { request_id } => {
            let mut op = decode(bytes)?;
            validate_operation(&op, request_id)?;
            if let m::Operation::Completed { outcome, .. } = &mut op {
                match outcome.as_mut() {
                    m::OperationOutcome::Session { session: row } => session(host, row)?,
                    m::OperationOutcome::Worktree { worktree: row } => workspace_identity(
                        &row.workspace_id,
                        row.identity.as_ref(),
                        &row.workspace_id,
                    )?,
                    m::OperationOutcome::Error { error } => {
                        if error.request_id != *request_id {
                            return Err(ClientError::local("PAIRED_HOST_INVALID_RESPONSE"));
                        }
                        *error = project_remote_error(error.clone());
                    }
                    _ => {}
                }
            }
            OperationResult::Operation(op)
        }
        Operation::PasteUploadChunk { .. } => OperationResult::PasteUploadChunk(decode(bytes)?),
        Operation::FileRead { .. } => OperationResult::FileRead(decode(bytes)?),
        // The capability probe returns this host's advertised capabilities; the stream
        // is opened separately over a socket.
        Operation::DagStream { .. } => OperationResult::Capabilities(decode(bytes)?),
        Operation::AgentControl { .. } => OperationResult::AgentControl(decode(bytes)?),
    })
}

fn agent_control_body(op: &crate::agent_control::AgentControlOp) -> serde_json::Value {
    match op {
        crate::agent_control::AgentControlOp::Send {
            text, request_id, ..
        } => {
            serde_json::json!({ "text": text, "requestId": request_id })
        }
        crate::agent_control::AgentControlOp::Answer {
            prompt_id,
            screen_revision,
            answer,
            request_id,
            ..
        } => serde_json::json!({
            "promptId": prompt_id,
            "screenRevision": screen_revision,
            "answer": answer,
            "requestId": request_id,
        }),
        crate::agent_control::AgentControlOp::Stop { request_id, .. } => {
            serde_json::json!({ "requestId": request_id })
        }
        _ => serde_json::Value::Null,
    }
}
#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "probe_tests.rs"]
mod probe_tests;
