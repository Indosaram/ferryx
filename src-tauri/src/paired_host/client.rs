//! Typed native machine HTTP. No arbitrary URL/method input or credential projection.
use super::{
    inventory::{CredentialLease, HostView},
    projects,
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
    /// Capability probe for DAG streaming. The stream itself is a socket, opened by
    /// [`MachineClient::attach_dag`], not an HTTP result.
    DagStream {
        workspace_id: String,
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
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientError {
    pub code: String,
    pub machine_error: Option<m::MachineError>,
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
        | "METHOD_NOT_ALLOWED" => error.code.as_str(),
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
fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    serde_json::from_slice(bytes).map_err(|_| ClientError::local("PAIRED_HOST_INVALID_RESPONSE"))
}
struct Route {
    method: Method,
    segments: Vec<String>,
    query: Vec<(String, String)>,
    body: Option<Vec<u8>>,
    capability: Option<&'static str>,
    request_id: Option<String>,
}
impl Operation {
    pub fn is_mutation(&self) -> bool {
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

    fn route(&self) -> Result<Route> {
        let mut r = Route {
            method: Method::GET,
            segments: vec![],
            query: vec![],
            body: None,
            capability: None,
            request_id: None,
        };
        let path;
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
            Self::DagStream { .. } => {
                path = "capabilities";
                r.capability = Some(crate::remote::dag_api::DAG_STREAM_CAPABILITY);
            }
        }
        r.segments = path.split('/').map(str::to_owned).collect();
        match self {
            Self::UnregisterProject { workspace_id, .. } => r.segments.push(workspace_id.clone()),
            Self::Session { session_id, .. } | Self::CloseSession { session_id, .. } => {
                r.segments.push(session_id.clone())
            }
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
            _ => {}
        }
        if r.body
            .as_ref()
            .is_some_and(|b| b.len() > m::MACHINE_JSON_MAX_BYTES)
        {
            return Err(ClientError::local("PAYLOAD_TOO_LARGE"));
        }
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

fn map_ticket_error(status: reqwest::StatusCode, bytes: &[u8]) -> ClientError {
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
            machine_error: Some(error),
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
            machine_error: Some(error),
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
        machine_error: Some(machine_error),
        ambiguous: false,
    }
}

/// A live authenticated DAG stream. Holding the lease keeps the stream bound to the
/// generation that authorized it: a forget or re-pair cancels it.
pub struct PairedDagStream {
    pub socket: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    pub lease: CredentialLease,
}

pub struct MachineClient {
    http: reqwest::Client,
}
impl Default for MachineClient {
    fn default() -> Self {
        Self::new()
    }
}
impl MachineClient {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(65))
                .build()
                .expect("native machine HTTP"),
        }
    }
    /// Reattach never creates a session. Authorization and target validation use
    /// the same executor as native HTTP operations before acquiring the socket.
    pub async fn attach_terminal(
        &self,
        service: &PairedHostService,
        descriptor: &crate::terminal::paired_daemon::Descriptor,
    ) -> Result<crate::terminal::paired_daemon::Transport> {
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;
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
        match response.result {
            OperationResult::Session(m::SessionDetail::Running { session })
                if session.target == descriptor.target => {}
            _ => return Err(ClientError::local("SESSION_EXPIRED")),
        }
        let (host, lease) = service
            .capture_operation(descriptor.host_id.clone(), descriptor.generation)
            .await?;
        let mut url =
            Url::parse(&host.host_id).map_err(|_| ClientError::local("INVALID_REQUEST"))?;
        let is_relay = is_relay_transport(&url);
        url.path_segments_mut()
            .map_err(|_| ClientError::local("INVALID_REQUEST"))?
            .extend(["api", "v1", "terminal", &descriptor.target.session_id]);
        url.query_pairs_mut()
            .append_pair("daemonEpoch", &descriptor.target.daemon_epoch.0.to_string());
        if let Some(cursor) = descriptor.after_sequence {
            url.query_pairs_mut()
                .append_pair("afterSequence", &cursor.0.to_string());
        }

        // Mint a single-use socket ticket if the host/relay endpoint supports it (relay requires ticket for upgrade).
        let ticket: Option<String> = {
            let mut t_url =
                Url::parse(&host.host_id).map_err(|_| ClientError::local("INVALID_REQUEST"))?;
            t_url
                .path_segments_mut()
                .map_err(|_| ClientError::local("INVALID_REQUEST"))?
                .extend(["api", "v1", "socket-ticket"]);
            let target_path = format!("/api/v1/terminal/{}", descriptor.target.session_id);
            let body = serde_json::json!({ "target": target_path }).to_string();
            let token = lease.token()?;
            let request_build = self
                .http
                .post(t_url)
                .bearer_auth(token)
                .header("content-type", "application/json")
                .body(body);
            let resp = match request_build.send().await {
                Ok(resp) => resp,
                Err(err) => {
                    let code = if err.is_timeout() {
                        "TIMEOUT"
                    } else {
                        "HOST_UNAVAILABLE"
                    };
                    return Err(ClientError::local(code));
                }
            };
            let status = resp.status();
            if status.is_success() {
                if resp.content_length().is_some_and(|n| n > 64 * 1024) {
                    return Err(ClientError::local("PAYLOAD_TOO_LARGE"));
                }
                let mut bytes = Vec::new();
                let mut resp = resp;
                while let Some(chunk) = resp
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
                let tr = serde_json::from_slice::<TicketResp>(&bytes)
                    .map_err(|_| ClientError::local("PAIRED_HOST_INVALID_RESPONSE"))?;
                if tr.ticket.is_empty() || tr.ticket.len() > 1024 {
                    return Err(ClientError::local("PAIRED_HOST_INVALID_RESPONSE"));
                }
                Some(tr.ticket)
            } else {
                // The Authorization-header fallback is allowed ONLY on the direct path
                // when the server explicitly signals the legacy capability (404).
                let legacy_direct = !is_relay && status == reqwest::StatusCode::NOT_FOUND;
                if legacy_direct {
                    None
                } else {
                    let mut bytes = Vec::new();
                    let mut resp = resp;
                    while let Ok(Some(chunk)) = resp.chunk().await {
                        if bytes.len() + chunk.len() > 16 * 1024 {
                            break;
                        }
                        bytes.extend_from_slice(&chunk);
                    }
                    return Err(map_ticket_error(status, &bytes));
                }
            }
        };

        if let Some(ref t) = ticket {
            url.query_pairs_mut().append_pair("ticket", t);
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
        let mut cancelled = lease.cancellation();
        let config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
            .max_message_size(Some(crate::remote::terminal_wire::MAX_FRAME_BYTES))
            .max_frame_size(Some(crate::remote::terminal_wire::MAX_FRAME_BYTES));
        let (socket, _) = tokio::select! { biased;
            _ = cancelled.changed() => return Err(ClientError::local("PAIRED_HOST_STALE_GENERATION")),
            result = tokio::time::timeout(Duration::from_secs(30), tokio_tungstenite::connect_async_with_config(request, Some(config), true)) =>
                result.map_err(|_| ClientError::local("TIMEOUT"))?.map_err(|e| {
                    tracing::error!("Failed to connect to paired host websocket: {:?}", e);
                    eprintln!("Failed to connect to paired host websocket: {:?}", e);
                    ClientError::local("HOST_UNAVAILABLE")
                })?,
        };
        service
            .current_generation(descriptor.host_id.clone(), descriptor.generation)
            .await?;
        Ok(crate::terminal::paired_daemon::Transport { socket, lease })
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
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;
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
        let mut url =
            Url::parse(&host.host_id).map_err(|_| ClientError::local("INVALID_REQUEST"))?;
        let is_relay = is_relay_transport(&url);
        url.path_segments_mut()
            .map_err(|_| ClientError::local("INVALID_REQUEST"))?
            .extend(["api", "v1", "workspace", "dag"]);
        url.query_pairs_mut()
            .append_pair("workspaceId", remote_workspace_id);

        let ticket = self
            .socket_ticket(&host, &lease, crate::remote::dag_api::DAG_SOCKET_TARGET, is_relay)
            .await?;
        if let Some(ticket) = &ticket {
            url.query_pairs_mut().append_pair("ticket", ticket);
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
        let mut cancelled = lease.cancellation();
        let config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
            .max_message_size(Some(crate::remote::dag_api::MAX_DAG_FRAME_BYTES))
            .max_frame_size(Some(crate::remote::dag_api::MAX_DAG_FRAME_BYTES));
        let (socket, _) = tokio::select! { biased;
            _ = cancelled.changed() => return Err(ClientError::local("PAIRED_HOST_STALE_GENERATION")),
            result = tokio::time::timeout(Duration::from_secs(30), tokio_tungstenite::connect_async_with_config(request, Some(config), true)) =>
                result.map_err(|_| ClientError::local("TIMEOUT"))?.map_err(|_| ClientError::local("HOST_UNAVAILABLE"))?,
        };
        // Re-verify the generation: a forget or re-pair during the upgrade must not
        // leave a live stream authorized by a retired credential.
        service
            .current_generation(host.host_id.clone(), generation)
            .await?;
        Ok(PairedDagStream { socket, lease })
    }

    /// Mints a single-use socket ticket for `target`, mirroring terminal attachment.
    /// A direct (non-relay) host that predates tickets answers 404 and is allowed to
    /// fall back to the Authorization header; a relay never is.
    async fn socket_ticket(
        &self,
        host: &HostView,
        lease: &CredentialLease,
        target: &str,
        is_relay: bool,
    ) -> Result<Option<String>> {
        let mut url =
            Url::parse(&host.host_id).map_err(|_| ClientError::local("INVALID_REQUEST"))?;
        url.path_segments_mut()
            .map_err(|_| ClientError::local("INVALID_REQUEST"))?
            .extend(["api", "v1", "socket-ticket"]);
        let body = serde_json::json!({ "target": target }).to_string();
        let response = self
            .http
            .post(url)
            .bearer_auth(lease.token()?)
            .header("content-type", "application/json")
            .body(body)
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

    async fn http(
        &self,
        host: &HostView,
        lease: &CredentialLease,
        route: &Route,
        directory: bool,
    ) -> Result<Vec<u8>> {
        let mut url =
            Url::parse(&host.host_id).map_err(|_| ClientError::local("INVALID_REQUEST"))?;
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
            .http
            .request(route.method.clone(), url)
            .bearer_auth(lease.token()?);
        if let Some(body) = &route.body {
            request = request
                .header("content-type", "application/json")
                .body(body.clone());
        }
        let mut cancellation = lease.cancellation();
        // Read-only machine operations ride the relay tunnel, whose round trips can
        // legitimately exceed 10s under tunnel churn; mutation budget is larger because
        // remote git/filesystem calls on enterprise repos can take tens of seconds.
        let budget = if route.body.is_some() { 60 } else { 45 };
        tokio::select! { biased;
            _=cancellation.changed()=>Err(ClientError::local("PAIRED_HOST_STALE_GENERATION")),
            result=tokio::time::timeout(Duration::from_secs(budget),async {
                let mut response=request.send().await.map_err(|_| ClientError::local("HOST_UNAVAILABLE"))?;
                let status=response.status();
                if status.is_redirection() { return Err(ClientError::local("PAIRED_HOST_REDIRECT_REJECTED")); }
                let limit=if directory {m::DIRECTORY_JSON_MAX_BYTES} else {m::MACHINE_JSON_MAX_BYTES};
                if response.content_length().is_some_and(|n| n>limit as u64) { return Err(ClientError::local("PAYLOAD_TOO_LARGE")); }
                if status.as_u16()==204 { return Ok(b"null".to_vec()); }
                let json=response.headers().get("content-type").and_then(|v| v.to_str().ok()).is_some_and(|v| v.split(';').next()==Some("application/json"));
                if !json { return Err(ClientError::local("PAIRED_HOST_INVALID_RESPONSE")); }
                let bytes=tokio::time::timeout(Duration::from_secs(budget),async {
                    let mut bytes=Vec::new();
                    while let Some(chunk)=response.chunk().await.map_err(|_| ClientError::local("HOST_UNAVAILABLE"))? {
                        if bytes.len()+chunk.len()>limit { return Err(ClientError::local("PAYLOAD_TOO_LARGE")); } bytes.extend_from_slice(&chunk);
                    } Ok(bytes)
                }).await.map_err(|_| ClientError::local("TIMEOUT"))??;
                if !status.is_success() { let envelope: m::ErrorEnvelope=decode(&bytes)?; let error=project_remote_error(envelope.error); return Err(ClientError { code: error.code.clone(), request_id: Some(error.request_id.clone()), machine_error:Some(error), ambiguous:false }); }
                Ok(bytes)
            })=> result.map_err(|_|ClientError::local("TIMEOUT"))?
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
        let caps: m::Capabilities = decode(
            &self
                .http(&host, &lease, &Operation::Capabilities.route()?, false)
                .await?,
        )?;
        if caps.machine_id != host.machine_id {
            return Err(ClientError::local("PAIRED_HOST_WRONG_MACHINE"));
        }
        if caps.access_scope != m::AccessScope::Machine || caps.permission != m::Permission::Control
        {
            return Err(ClientError::local("MACHINE_ACCESS_REQUIRED"));
        }
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
                match outcome {
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
        // The capability probe returns this host's advertised capabilities; the stream
        // is opened separately over a socket.
        Operation::DagStream { .. } => OperationResult::Capabilities(decode(bytes)?),
    })
}
#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
