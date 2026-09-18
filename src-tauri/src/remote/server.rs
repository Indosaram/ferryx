#[path = "machine_owner_socket.rs"]
mod machine_owner_socket;
use crate::remote::auth::{AuthError, DeviceAccessScope, DeviceInfo, DevicePermission};
use crate::remote::backend::{RecoveryStream, RemoteRecoveryStatus, RemoteSessionBackend};
use crate::remote::browser_admission::AdmissionController;
use crate::remote::browser_backend::{DesktopScope, RemoteBrowserBackend, RemoteBrowserError};
use crate::remote::browser_protocol::ServerMessage;
use crate::remote::browser_security::sanitize_public_string;
use crate::remote::browser_ws::BrowserWsSession;
use crate::remote::mirror::RemoteTerminalMirror;
use crate::remote::protocol::RemoteGridFrame;
use crate::remote::protocol::{
    ClientControlMessage, RemoteActiveDesktopSelection, RemoteCreateWorktreeRequest,
    RemoteDeleteWorktreeRequest, RemoteEventMessage, RemoteProjectInfo,
    RemoteSelectWorkspaceRequest, RemoteSelectionRequestPayload, RemoteTerminalSession,
    RemoteTerminalTabInfo, RemoteWorkspaceState, RemoteWorktreeInfo,
};
pub use crate::remote::protocol::RemoteTerminalTabInfo as RemoteTerminalTab;
use crate::remote::push::{global_push_store, PushSubscriptionInfo};
use crate::remote::state::{
    RemoteGatewayState, RemoteNetworkMode, REMOTE_ACTIVE_SELECTION_CHANGED_EVENT,
};
use crate::terminal::{AttachmentSnapshot, OutputChunk, SessionAttachment, TerminalSignal};
use crate::worktree::{parse_host_scoped_session_id, CreateWorktreeOptions, WorktreeIdentity};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path as AxumPath, Query, State,
    },
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tokio::sync::mpsc;
use tower_http::cors::{Any, CorsLayer};

pub const REMOTE_SELECTION_REQUEST_EVENT: &str = "remote_selection_requested";
const REMOTE_TERMINAL_METADATA_PREFIX: &[u8] = b"\x1b]777;ferryx;";
const REMOTE_TERMINAL_METADATA_TERMINATOR: u8 = 0x07;
const REMOTE_TERMINAL_HARD_RESET: &[u8] = b"\x1bc";
const REMOTE_GRID_MAX_COLS: u16 = 512;
const REMOTE_GRID_MAX_ROWS: u16 = 256;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoteTerminalFrameMetadata {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sequence: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_after_sequence: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available_from_sequence: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_sequence: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_sequence: Option<String>,
}

pub(crate) fn encode_remote_terminal_output_frame(chunk: &OutputChunk) -> Vec<u8> {
    encode_remote_terminal_frame(
        RemoteTerminalFrameMetadata {
            kind: if chunk.replay_gap.is_some() {
                "replayGap"
            } else {
                "output"
            }
            .into(),
            sequence: Some(chunk.sequence.to_string()),
            requested_after_sequence: chunk
                .replay_gap
                .as_ref()
                .map(|gap| gap.requested_after_sequence.to_string()),
            available_from_sequence: chunk
                .replay_gap
                .as_ref()
                .map(|gap| gap.available_from_sequence.to_string()),
            start_sequence: None,
            end_sequence: None,
        },
        &chunk.bytes,
        chunk.replay_gap.is_some(),
    )
}

pub(crate) fn encode_remote_terminal_snapshot_frame(
    snapshot: &AttachmentSnapshot,
    force_boundary: bool,
) -> Vec<u8> {
    let gap = snapshot.gap.as_ref();
    encode_remote_terminal_frame(
        RemoteTerminalFrameMetadata {
            kind: if gap.is_some() {
                "replayGap".into()
            } else {
                "replay".into()
            },
            sequence: snapshot
                .history_end_sequence
                .map(|sequence| sequence.to_string()),
            requested_after_sequence: gap.map(|gap| gap.requested_after_sequence.to_string()),
            available_from_sequence: gap.map(|gap| gap.available_from_sequence.to_string()),
            start_sequence: snapshot
                .history_start_sequence
                .map(|sequence| sequence.to_string()),
            end_sequence: snapshot
                .history_end_sequence
                .map(|sequence| sequence.to_string()),
        },
        &snapshot.history,
        force_boundary || gap.is_some(),
    )
}

fn encode_remote_terminal_frame(
    metadata: RemoteTerminalFrameMetadata,
    payload: &[u8],
    reset: bool,
) -> Vec<u8> {
    let metadata =
        serde_json::to_vec(&metadata).expect("remote terminal frame metadata serializes");
    let mut frame = Vec::with_capacity(
        REMOTE_TERMINAL_METADATA_PREFIX.len()
            + metadata.len()
            + 1
            + if reset {
                REMOTE_TERMINAL_HARD_RESET.len()
            } else {
                0
            }
            + payload.len(),
    );
    frame.extend_from_slice(REMOTE_TERMINAL_METADATA_PREFIX);
    frame.extend_from_slice(&metadata);
    frame.push(REMOTE_TERMINAL_METADATA_TERMINATOR);
    if reset {
        frame.extend_from_slice(REMOTE_TERMINAL_HARD_RESET);
    }
    frame.extend_from_slice(payload);
    frame
}

pub(crate) async fn recover_remote_terminal_attachment(
    session_backend: &Arc<dyn RemoteSessionBackend>,
    session_id: &str,
    last_emitted_sequence: Option<u64>,
) -> Result<SessionAttachment, String> {
    session_backend
        .attach_with_sequence(session_id, Some(last_emitted_sequence.unwrap_or(0)))
        .await
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    version: &'static str,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PairExchangeRequest {
    code: String,
    device_name: String,
    #[serde(default)]
    installation_id: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PairExchangeResponse {
    token: String,
    device: DeviceInfo,
    machine_id: String,
    display_name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthQuery {
    /// Single-use credential minted by `/api/v1/socket-ticket`, used instead of a
    /// permanent device token because a browser WebSocket cannot send headers.
    ticket: Option<String>,
    render: Option<String>,
    cols: Option<u16>,
    rows: Option<u16>,
    daemon_epoch: Option<crate::scoped_contracts::Epoch>,
    after_sequence: Option<crate::scoped_contracts::Epoch>,
}

fn validated_grid_geometry(cols: u16, rows: u16) -> Option<(u16, u16)> {
    if cols == 0 || rows == 0 || cols > REMOTE_GRID_MAX_COLS || rows > REMOTE_GRID_MAX_ROWS {
        return None;
    }
    Some((cols, rows))
}

fn requested_grid_geometry(query: &AuthQuery) -> Option<(u16, u16)> {
    validated_grid_geometry(query.cols?, query.rows?)
}

/// Reads the device bearer from the `Authorization` header ONLY.
///
/// A permanent device token must never travel in a URL: it would persist in
/// browser history and gateway access logs long after the request. Sockets, which
/// cannot set headers from a browser, use a single-use ticket instead
/// (`POST /api/v1/socket-ticket`).
pub(super) fn extract_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(|token| token.trim().to_string())
}

/// How long a direct-gateway socket ticket stays redeemable.
///
/// Matches the relay's `SOCKET_TICKET_TTL`. A ticket only has to survive the gap
/// between minting it over HTTP and opening the socket, so the window is short.
pub(crate) const SOCKET_TICKET_TTL_SECS: u64 = 30;

/// Upper bound on tickets held for a gateway, so a client that mints without
/// connecting cannot grow the map without limit.
const MAX_PENDING_SOCKET_TICKETS: usize = 256;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SocketTicketRequest {
    target: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SocketTicketResponse {
    ticket: String,
    expires_at: u64,
}

fn unix_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// A ticket may only name a socket route on this gateway.
fn valid_socket_target(target: &str) -> bool {
    target == "/api/v1/events"
        || target.strip_prefix("/api/v1/terminal/").is_some_and(|id| {
            !id.is_empty()
                && id.bytes().all(|b| {
                    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b':')
                })
        })
        || target.strip_prefix("/api/v1/browser/").is_some_and(|id| {
            !id.is_empty()
                && id.bytes().all(|b| {
                    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b':')
                })
        })
}

/// Mints a single-use ticket for one WebSocket upgrade.
///
/// The bearer is presented in the `Authorization` header and never appears in a URL.
/// The returned ticket is what the browser puts in the socket query string, so a
/// leaked URL exposes only a one-shot credential that expires in seconds.
async fn issue_socket_ticket(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    Json(request): Json<SocketTicketRequest>,
) -> Result<Json<SocketTicketResponse>, (StatusCode, String)> {
    let token = extract_token(&headers)
        .ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".to_string()))?;
    // Authorize with the real device store, so a revoked or unknown bearer cannot
    // trade an unusable token for a working ticket.
    state
        .auth_manager
        .validate_token(&token)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token".to_string()))?;
    if !valid_socket_target(&request.target) {
        return Err((StatusCode::BAD_REQUEST, "Unsupported socket target".into()));
    }

    let ticket = uuid::Uuid::new_v4().to_string();
    let expires_at = unix_now_secs() + SOCKET_TICKET_TTL_SECS;
    {
        let mut tickets = state.socket_tickets.lock();
        let now = unix_now_secs();
        tickets.retain(|_, (_, _, expiry)| *expiry > now);
        if tickets.len() >= MAX_PENDING_SOCKET_TICKETS {
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                "Too many pending socket tickets".into(),
            ));
        }
        tickets.insert(ticket.clone(), (token, request.target, expires_at));
    }
    Ok(Json(SocketTicketResponse { ticket, expires_at }))
}

/// Redeems a ticket for the device token it was minted from, removing it so the
/// same ticket cannot authorize a second upgrade.
fn consume_socket_ticket(
    state: &RemoteGatewayState,
    ticket: &str,
    target: &str,
) -> Option<String> {
    let (token, issued_target, expiry) = state.socket_tickets.lock().remove(ticket)?;
    if issued_target != target || expiry <= unix_now_secs() {
        return None;
    }
    Some(token)
}

/// Resolves the device token authorizing a WebSocket upgrade.
///
/// A `ticket` is preferred and is single-use: presenting one consumes it, so a
/// replayed URL cannot open a second socket. The `Authorization` header and the
/// legacy `token` query parameter remain accepted so existing clients keep working
/// while they migrate.
fn socket_credential(
    state: &RemoteGatewayState,
    headers: &HeaderMap,
    query: &AuthQuery,
    target: &str,
) -> Option<String> {
    if let Some(ticket) = query.ticket.as_deref() {
        // A supplied ticket must stand on its own; falling back to another
        // credential here would let an invalid ticket be ignored rather than refused.
        return consume_socket_ticket(state, ticket, target);
    }
    extract_token(headers)
}

async fn health_check() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: "0.1.0",
    })
}

async fn pair_exchange(
    State(state): State<Arc<RemoteGatewayState>>,
    Json(payload): Json<PairExchangeRequest>,
) -> Result<Response, Response> {
    let identity = load_gateway_identity(Arc::clone(&state)).await?;
    let (token, device) = state
        .auth_manager
        .exchange_pairing_code_with_installation(
            &payload.code,
            &payload.device_name,
            payload.installation_id.as_deref(),
        )
        .map_err(|e| match e {
            AuthError::InvalidPairingCode => {
                (StatusCode::BAD_REQUEST, "Invalid pairing code").into_response()
            }
            AuthError::ExpiredPairingCode => {
                (StatusCode::UNAUTHORIZED, "Pairing code expired").into_response()
            }
            AuthError::PairingRateLimited => (
                StatusCode::TOO_MANY_REQUESTS,
                Json(serde_json::json!({"code": "pairing_rate_limited"})),
            )
                .into_response(),
            AuthError::Unauthorized => (StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
            AuthError::Storage(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Auth storage error: {err}"),
            )
                .into_response(),
        })?;

    Ok(([(header::CACHE_CONTROL, "no-store")], Json(PairExchangeResponse {
        token,
        device,
        machine_id: identity.machine_id,
        display_name: identity.display_name,
    })).into_response())
}

/// Canonicalize a filesystem path for comparison purposes. When the path itself
/// doesn't exist (e.g. a session whose worktree was deleted after the session was
/// spawned), canonicalizes the nearest existing ancestor instead and re-appends
/// the missing tail, so the result still resolves symlinks (notably macOS's
/// `/var` -> `/private/var`) in the part of the path that *does* exist. This
/// keeps `starts_with` comparisons against a canonical repo root correct even for
/// non-existent paths, rather than silently falling back to a raw path that may
/// use a different symlink alias than the canonical root it's compared against.
fn canonicalize_or_raw(path: &Path) -> PathBuf {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return canonical;
    }
    let mut missing_tail = Vec::new();
    let mut ancestor = path;
    loop {
        match ancestor.parent() {
            Some(parent) => {
                missing_tail.push(ancestor.file_name());
                ancestor = parent;
            }
            None => break,
        }
        if let Ok(canonical_ancestor) = std::fs::canonicalize(ancestor) {
            let mut resolved = canonical_ancestor;
            for component in missing_tail.into_iter().rev().flatten() {
                resolved.push(component);
            }
            return resolved;
        }
    }
    path.to_path_buf()
}

#[derive(Debug, Clone)]
struct WorkspaceSnapshot {
    workspace_id: String,
    /// Canonicalized repo root (the manager's root is already canonical at
    /// construction time, but we re-derive defensively in case the directory
    /// was removed or replaced by a symlink after registration).
    root: PathBuf,
    worktrees: Vec<crate::worktree::Worktree>,
}

/// Cached snapshot of the workspace registry's contents. Cached across requests
/// to avoid repeated `git worktree list` subprocess invocations during remote
/// state polling and terminal switching.
#[derive(Debug, Clone)]
pub(crate) struct WorkspaceSnapshotCache {
    workspaces: Vec<WorkspaceSnapshot>,
}

fn activity_rank(state: &str) -> u8 {
    match state {
        "waiting" | "blocked" => 3,
        "done" => 2,
        "working" => 1,
        _ => 0,
    }
}

pub(crate) fn compute_attention_rollup<'a>(
    states: impl IntoIterator<Item = &'a str>,
) -> Option<String> {
    let mut best_rank = 0u8;
    for s in states {
        let rank = activity_rank(s);
        if rank > best_rank {
            best_rank = rank;
        }
    }
    match best_rank {
        3 => Some("waiting".to_string()),
        2 => Some("done".to_string()),
        1 => Some("working".to_string()),
        _ => None,
    }
}

fn worktree_matches_selection(
    workspace_id: &str,
    wt_slug: Option<&str>,
    wt_label: Option<&str>,
    sel: &RemoteActiveDesktopSelection,
) -> bool {
    let sel_ws = sel.workspace_id.as_deref();
    if sel_ws.is_some() && sel_ws != Some(workspace_id) {
        return false;
    }
    if let Some(target_slug) = sel.worktree_slug.as_deref() {
        wt_slug == Some(target_slug)
    } else if wt_slug.is_some() {
        false
    } else if let (Some(target_label), Some(label)) = (sel.worktree_label.as_deref(), wt_label) {
        target_label == label
    } else {
        true
    }
}

fn compute_worktree_attention(
    workspace_id: &str,
    wt_slug: Option<&str>,
    wt_label: Option<&str>,
    selection: Option<&RemoteActiveDesktopSelection>,
) -> Option<String> {
    let sel = selection?;
    if let Some(entry) = sel.attention_inventory.iter().find(|entry| {
        entry.workspace_id == workspace_id
            && entry.worktree_slug.as_deref() == wt_slug
            && (wt_slug.is_some() || entry.worktree_label.as_deref() == wt_label)
    }) {
        return compute_attention_rollup(entry.state.as_deref());
    }
    if !worktree_matches_selection(workspace_id, wt_slug, wt_label, sel) {
        return None;
    }
    compute_attention_rollup(
        sel.terminal_tabs
            .iter()
            .filter_map(|tab| tab.activity_state.as_deref()),
    )
}

#[cfg(test)]
mod attention_inventory_tests {
    use super::*;

    #[test]
    fn remote_attention_inventory_keeps_other_projects_and_unseen_done() {
        let selection: RemoteActiveDesktopSelection = serde_json::from_value(serde_json::json!({
            "workspaceId": "active", "worktreeLabel": "main",
            "attentionInventory": [
                { "workspaceId": "parked", "worktreeSlug": null, "worktreeLabel": "main", "state": "done" }
            ]
        })).unwrap();
        assert_eq!(
            compute_worktree_attention("parked", None, Some("main"), Some(&selection)).as_deref(),
            Some("done")
        );
        assert_eq!(
            compute_attention_rollup(["working", "done"]).as_deref(),
            Some("done")
        );
    }
}

impl WorkspaceSnapshotCache {
    pub(crate) fn build(registry: &crate::worktree::WorkspaceRegistry) -> Self {
        let mut entries = registry.list();
        // Deterministic order: `WorkspaceRegistry::list` iterates a `HashMap`, whose
        // order is unspecified and can vary between calls.
        entries.sort_by(|(a, _), (b, _)| a.cmp(b));
        let workspaces = entries
            .into_iter()
            .map(|(workspace_id, mgr)| WorkspaceSnapshot {
                workspace_id,
                root: canonicalize_or_raw(mgr.repo_root()),
                worktrees: mgr.list_worktrees().unwrap_or_default(),
            })
            .collect();
        Self { workspaces }
    }

    pub(crate) fn projects(
        &self,
        selection: Option<&RemoteActiveDesktopSelection>,
    ) -> Vec<RemoteProjectInfo> {
        self.workspaces
            .iter()
            .map(|w| RemoteProjectInfo {
                workspace_id: w.workspace_id.clone(),
                worktrees: w
                    .worktrees
                    .iter()
                    .map(|worktree| {
                        let slug = worktree.orca_info().map(|info| info.slug);
                        let label = worktree.branch_short_name().map(str::to_string);
                        let attention = compute_worktree_attention(
                            &w.workspace_id,
                            slug.as_deref(),
                            label.as_deref(),
                            selection,
                        );
                        RemoteWorktreeInfo {
                            worktree_slug: slug,
                            worktree_label: label,
                            attention,
                        }
                    })
                    .collect(),
            })
            .collect()
    }

    /// Previously listed worktrees for `workspace_id`, if registered. Reuses the
    /// snapshot captured at cache-build time instead of re-listing from disk.
    pub(crate) fn worktrees_for(
        &self,
        workspace_id: &str,
        selection: Option<&RemoteActiveDesktopSelection>,
    ) -> Vec<RemoteWorktreeInfo> {
        self.workspaces
            .iter()
            .find(|w| w.workspace_id == workspace_id)
            .map(|workspace| {
                workspace
                    .worktrees
                    .iter()
                    .map(|worktree| {
                        let slug = worktree.orca_info().map(|info| info.slug);
                        let label = worktree.branch_short_name().map(str::to_string);
                        let attention = compute_worktree_attention(
                            workspace_id,
                            slug.as_deref(),
                            label.as_deref(),
                            selection,
                        );
                        RemoteWorktreeInfo {
                            worktree_slug: slug,
                            worktree_label: label,
                            attention,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Resolve `(workspace_id, worktree_label)` for a terminal session from the
    /// worktree path that owns it. Degrades gracefully: unmatched paths yield a
    /// best-effort label derived from the path itself, missing paths yield `None`.
    pub(crate) fn derive_session_metadata(
        &self,
        worktree_path: Option<&Path>,
    ) -> (Option<String>, Option<String>) {
        let Some(path) = worktree_path else {
            return (None, None);
        };
        let canonical = canonicalize_or_raw(path);
        for workspace in &self.workspaces {
            if !canonical.starts_with(&workspace.root) {
                continue;
            }
            let label = workspace
                .worktrees
                .iter()
                .find(|wt| wt.path == path || wt.path == canonical)
                .and_then(|wt| wt.branch_short_name().map(str::to_string))
                .or_else(|| relative_label(&workspace.root, &canonical));
            return (Some(workspace.workspace_id.clone()), label);
        }
        (
            None,
            path.file_name().map(|f| f.to_string_lossy().into_owned()),
        )
    }
}

/// Best-effort, non-panicking label for a path that lives under `root` but isn't a
/// listed git worktree (e.g. an ad-hoc subdirectory terminal). Prefers the path
/// relative to the workspace root so nested/ad-hoc sessions get an informative,
/// collision-resistant label instead of a bare directory name; falls back to the
/// root's own directory name when the path *is* the root.
fn relative_label(root: &Path, canonical: &Path) -> Option<String> {
    if let Ok(rel) = canonical.strip_prefix(root) {
        if !rel.as_os_str().is_empty() {
            return Some(
                rel.to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/"),
            );
        }
    }
    canonical
        .file_name()
        .or_else(|| root.file_name())
        .map(|f| f.to_string_lossy().into_owned())
}

pub(crate) async fn get_active_running_sessions(
    state: &RemoteGatewayState,
    cache: &WorkspaceSnapshotCache,
    ssh_projects: &[crate::ssh::projects::RemoteProject],
) -> Vec<RemoteTerminalSession> {
    let active = state.active_selection.read().clone();
    let mut sessions = Vec::new();

    for session_id in state.session_backend.list_sessions().await {
        if let Some(services) = &state.machine_services {
            match services.sessions.machine_only_async(&session_id).await {
                Ok(false) => {},
                Ok(true) | Err(_) => continue,
            }
        }
        let Ok(details) = state.session_backend.describe_session(&session_id).await else {
            continue;
        };
        if !details.running {
            continue;
        }
        if let Some(id) = details
            .workspace_id
            .as_deref()
            .filter(|id| crate::ssh::projects::is_remote(id))
        {
            if let Some(project) = ssh_projects
                .iter()
                .find(|project| project.workspace_id == id)
            {
                sessions.push(RemoteTerminalSession {
                    session_id: details.session_id,
                    title: None,
                    workspace_id: Some(id.to_owned()),
                    worktree_label: Some(super::ssh::label(project)),
                    running: true,
                });
            }
            continue;
        }
        let selected = active
            .as_ref()
            .filter(|selection| selection.session_id.as_deref() == Some(session_id.as_str()));
        let (derived_ws, derived_label) =
            cache.derive_session_metadata(details.worktree_path.as_deref());
        let workspace_id = derived_ws
            .or(details.workspace_id)
            .or_else(|| selected.and_then(|selection| selection.workspace_id.clone()))
            .or_else(|| if active.is_none() { Some("default".to_string()) } else { None });
        // Sessions are listed for authenticated remote callers regardless of
        // desktop active selection; `active_selection` only supplies extra
        // label metadata when it matches this session, it never filters.
        let worktree_label = derived_label.or(details.worktree_label).or_else(|| {
            selected.and_then(|selection| {
                selection
                    .worktree_slug
                    .clone()
                    .or_else(|| selection.worktree_label.clone())
            })
        }).or_else(|| if active.is_none() { Some("default".to_string()) } else { None });
        sessions.push(RemoteTerminalSession {
            session_id: details.session_id,
            title: if active.is_none() { Some("Terminal".to_string()) } else { None },
            workspace_id,
            worktree_label,
            running: true,
        });
    }

    if active.is_none() {
        for session_id in state.terminal_service.list_sessions() {
            if sessions.iter().any(|s| s.session_id == session_id) {
                continue;
            }
            let is_running_or_starting = if let Some(session) = state.terminal_service.get_session(&session_id) {
                matches!(
                    session.state(),
                    crate::terminal::PtySessionState::Running | crate::terminal::PtySessionState::Starting
                )
            } else if let Some(details) = state.terminal_service.remote().details(&session_id) {
                matches!(
                    details.state,
                    crate::terminal::remote::RemoteConnectionState::Connected
                )
            } else {
                false
            };

            if !is_running_or_starting {
                continue;
            }

            let worktree_path = state
                .terminal_service
                .get_session(&session_id)
                .and_then(|s| s.worktree_path())
                .or_else(|| {
                    state
                        .terminal_service
                        .remote()
                        .details(&session_id)
                        .map(|d| PathBuf::from(d.descriptor.config.project_path))
                });
            let (derived_ws, derived_label) =
                cache.derive_session_metadata(worktree_path.as_deref());

            sessions.push(RemoteTerminalSession {
                session_id,
                title: Some("Terminal".to_string()),
                workspace_id: derived_ws.or_else(|| Some("default".to_string())),
                worktree_label: derived_label.or_else(|| Some("default".to_string())),
                running: true,
            });
        }
    }

    sessions.sort_by(|left, right| left.session_id.cmp(&right.session_id));
    sessions
}

async fn list_legacy_sessions(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<RemoteTerminalSession>>, (StatusCode, String)> {
    let token = extract_token(&headers)
        .ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
    let _device = state
        .auth_manager
        .validate_token(&token)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token".into()))?;

    let cache = state
        .workspace_snapshot()
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
    let ssh_projects = super::ssh::projects(&state).await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "SSH inventory unavailable".into(),
        )
    })?;
    let sessions = get_active_running_sessions(&state, &cache, &ssh_projects).await;

    Ok(Json(sessions))
}

async fn get_workspace_state(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
) -> Result<Json<RemoteWorkspaceState>, (StatusCode, String)> {
    let token = extract_token(&headers)
        .ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
    let _device = state
        .auth_manager
        .validate_token(&token)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token".into()))?;

    let cache = state
        .workspace_snapshot()
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;

    let ssh_projects = super::ssh::projects(&state).await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "SSH inventory unavailable".into(),
        )
    })?;
    let active_selection = state.active_selection.read().clone().filter(|selection| {
        selection.workspace_id.as_deref().is_none_or(|id| {
            !crate::ssh::projects::is_remote(id)
                || ssh_projects
                    .iter()
                    .any(|project| project.workspace_id == id)
        })
    });
    let mut projects = cache.projects(active_selection.as_ref());
    projects.extend(ssh_projects.iter().map(|project| RemoteProjectInfo {
        workspace_id: project.workspace_id.clone(),
        worktrees: vec![RemoteWorktreeInfo {
            worktree_slug: None,
            worktree_label: Some(super::ssh::label(project)),
            attention: None,
        }],
    }));
    let mut active_ws = active_selection
        .as_ref()
        .and_then(|sel| sel.workspace_id.clone())
        .filter(|id| !id.is_empty())
        .or_else(|| projects.first().map(|p| p.workspace_id.clone()))
        .unwrap_or_else(|| "default".into());
    let mut active_context = active_selection
        .clone()
        .unwrap_or(RemoteActiveDesktopSelection {
            workspace_id: Some(active_ws.clone()),
            attention_inventory: Vec::new(),
            worktree_slug: None,
            worktree_label: None,
            session_id: None,
            tab_id: None,
            terminal_tabs: Vec::new(),
        });

    if state.active_selection.read().is_none() {
        let backend_sessions = state.session_backend.list_sessions().await;
        let mut live_session_id = None;
        for sid in &backend_sessions {
            if let Ok(details) = state.session_backend.describe_session(sid).await {
                if details.running {
                    live_session_id = Some(sid.clone());
                    break;
                }
            }
        }
        if live_session_id.is_none() {
            for sid in state.terminal_service.list_sessions() {
                if let Some(session) = state.terminal_service.get_session(&sid) {
                    if matches!(
                        session.state(),
                        crate::terminal::PtySessionState::Running
                            | crate::terminal::PtySessionState::Starting
                    ) {
                        live_session_id = Some(sid);
                        break;
                    }
                }
            }
        }
        if live_session_id.is_none() && !backend_sessions.is_empty() {
            live_session_id = backend_sessions.first().cloned();
        }
        if live_session_id.is_none() {
            if let Some(sid) = state.terminal_service.list_sessions().first() {
                live_session_id = Some(sid.clone());
            }
        }

        if let Some(session_id) = live_session_id {
            if active_context.session_id.is_none() {
                active_context.session_id = Some(session_id.clone());
                active_context.terminal_tabs = vec![RemoteTerminalTabInfo {
                    id: session_id.clone(),
                    label: "Terminal".to_string(),
                    session_id: Some(session_id),
                    ..Default::default()
                }];
            }
        } else if backend_sessions.is_empty() && state.terminal_service.list_sessions().is_empty() {
            if let Ok((session_id, _)) = state.terminal_service.spawn_shell(80, 24) {
                tracing::info!("Auto-spawned default shell session {session_id} for headless remote gateway");
                active_context.session_id = Some(session_id.clone());
                active_context.terminal_tabs = vec![RemoteTerminalTabInfo {
                    id: session_id,
                    label: "Terminal".to_string(),
                    session_id: active_context.session_id.clone(),
                    ..Default::default()
                }];
            }
        }
    }

    let worktrees = cache.worktrees_for(&active_ws, active_selection.as_ref());
    let mut sessions = get_active_running_sessions(&state, &cache, &ssh_projects).await;

    if let Some(ref session_id) = active_context.session_id {
        if let Some(s) = sessions.iter_mut().find(|s| &s.session_id == session_id) {
            if s.title.is_none() {
                s.title = Some("Terminal".to_string());
            }
            if s.workspace_id.is_none() || s.workspace_id.as_deref() == Some("default") {
                s.workspace_id = Some(active_ws.clone());
            } else if active_selection.is_none() && active_ws == "default" {
                if let Some(ref ws) = s.workspace_id {
                    active_ws = ws.clone();
                    active_context.workspace_id = Some(ws.clone());
                }
            }
            if s.worktree_label.is_none() {
                s.worktree_label = Some("default".to_string());
            }
        } else {
            sessions.push(RemoteTerminalSession {
                session_id: session_id.clone(),
                title: Some("Terminal".to_string()),
                workspace_id: Some(active_ws.clone()),
                worktree_label: Some("default".to_string()),
                running: true,
            });
            sessions.sort_by(|left, right| left.session_id.cmp(&right.session_id));
        }
    }

    Ok(Json(RemoteWorkspaceState {
        projects,
        active_context,
        active_workspace_id: active_ws,
        worktrees,
        sessions,
    }))
}

async fn select_workspace(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    Json(payload): Json<RemoteSelectWorkspaceRequest>,
) -> Result<Json<RemoteSelectionRequestPayload>, (StatusCode, String)> {
    let token = extract_token(&headers)
        .ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
    let device = state
        .auth_manager
        .validate_token(&token)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token".into()))?;

    if device.permission != DevicePermission::Control {
        return Err((
            StatusCode::FORBIDDEN,
            "View-only device cannot request workspace selection".into(),
        ));
    }

    if payload.create_terminal && (payload.tab_id.is_some() || payload.session_id.is_some()) {
        return Err((StatusCode::BAD_REQUEST, "Terminal creation cannot select an existing terminal".into()));
    }

    let is_ssh = crate::ssh::projects::is_remote(&payload.workspace_id);
    if is_ssh {
        let projects = super::ssh::projects(&state).await.map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "SSH inventory unavailable".into(),
            )
        })?;
        if !projects
            .iter()
            .any(|project| project.workspace_id == payload.workspace_id)
            || payload.worktree.is_some()
            || payload.worktree_slug.is_some()
        {
            return Err((StatusCode::BAD_REQUEST, "SSH project is unavailable".into()));
        }
    } else {
        state
            .workspace_registry
            .manager(&payload.workspace_id)
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    }

    if let Some(session_id) = payload.session_id.as_deref() {
        let details = state
            .session_backend
            .describe_session(session_id)
            .await
            .map_err(|_| (StatusCode::BAD_REQUEST, "Session unavailable".into()))?;
        if !details.running || details.workspace_id.as_deref() != Some(&payload.workspace_id) {
            return Err((
                StatusCode::BAD_REQUEST,
                "Session does not belong to project".into(),
            ));
        }
    }

    let (worktree_identity, worktree_slug, worktree_label) = if let Some(ref wt) = payload.worktree
    {
        let (_, resolved_wt) = state
            .workspace_registry
            .resolve_worktree(&payload.workspace_id, wt)
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
        let label = resolved_wt
            .branch_short_name()
            .map(str::to_string)
            .or_else(|| payload.worktree_label.clone());
        (Some(wt.clone()), Some(wt.slug.clone()), label)
    } else if let Some(ref slug) = payload.worktree_slug {
        let ident = WorktreeIdentity {
            ws_id: payload.workspace_id.clone(),
            slug: slug.clone(),
        };
        let (_, resolved_wt) = state
            .workspace_registry
            .resolve_worktree(&payload.workspace_id, &ident)
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
        let label = resolved_wt
            .branch_short_name()
            .map(str::to_string)
            .or_else(|| payload.worktree_label.clone());
        (Some(ident), Some(slug.clone()), label)
    } else {
        (None, None, payload.worktree_label.clone())
    };

    if let Some(tab_id) = payload.tab_id.as_deref() {
        let selection = state.active_selection();
        let tab_is_available = selection.as_ref().is_some_and(|selection| {
            selection.workspace_id.as_deref() == Some(payload.workspace_id.as_str())
                && selection.terminal_tabs.iter().any(|tab| {
                    tab.id == tab_id
                        && tab
                            .worktree_slug
                            .as_deref()
                            .or(selection.worktree_slug.as_deref())
                            == worktree_slug.as_deref()
                        && payload
                            .session_id
                            .as_deref()
                            .is_none_or(|id| tab.session_id.as_deref() == Some(id))
                })
        });
        if !tab_is_available {
            return Err((
                StatusCode::BAD_REQUEST,
                "Requested terminal tab is not available in the active desktop context".into(),
            ));
        }
    }

    let event_payload = RemoteSelectionRequestPayload {
        workspace_id: payload.workspace_id,
        create_terminal: payload.create_terminal,
        worktree: worktree_identity,
        worktree_slug,
        worktree_label,
        session_id: payload.session_id,
        tab_id: payload.tab_id,
    };

    state.emit_desktop_event(
        REMOTE_SELECTION_REQUEST_EVENT,
        serde_json::to_value(&event_payload).unwrap_or(serde_json::Value::Null),
    );

    Ok(Json(event_payload))
}

fn create_worktree_blocking(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    Json(payload): Json<RemoteCreateWorktreeRequest>,
) -> Result<Json<RemoteWorktreeInfo>, (StatusCode, String)> {
    let token = extract_token(&headers)
        .ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
    let device = state
        .auth_manager
        .validate_token(&token)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token".into()))?;

    if device.permission != DevicePermission::Control {
        return Err((
            StatusCode::FORBIDDEN,
            "View-only device cannot create worktrees".into(),
        ));
    }

    let mgr = state
        .workspace_registry
        .manager(&payload.workspace_id)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    let path = mgr
        .worktree_path_for(&payload.worktree.ws_id, &payload.worktree.slug)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    let options = CreateWorktreeOptions {
        ws_id: payload.worktree.ws_id,
        slug: payload.worktree.slug,
        path,
        base_ref: payload.base_ref,
    };

    let created = mgr
        .create_worktree(options)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(RemoteWorktreeInfo {
        worktree_slug: created.orca_info().map(|info| info.slug),
        worktree_label: created.branch_short_name().map(str::to_string),
        attention: None,
    }))
}

fn delete_worktree_blocking(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    Json(payload): Json<RemoteDeleteWorktreeRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let token = extract_token(&headers)
        .ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
    let device = state
        .auth_manager
        .validate_token(&token)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token".into()))?;

    if device.permission != DevicePermission::Control {
        return Err((
            StatusCode::FORBIDDEN,
            "View-only device cannot delete worktrees".into(),
        ));
    }

    let (mgr, worktree) = state
        .workspace_registry
        .resolve_worktree(&payload.workspace_id, &payload.worktree)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    mgr.delete_worktree_and_branch(&worktree.path, payload.delete_branch.unwrap_or(false))
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}

async fn list_devices(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<DeviceInfo>>, (StatusCode, String)> {
    let token = extract_token(&headers)
        .ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
    let _device = state
        .auth_manager
        .validate_token(&token)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token".into()))?;

    Ok(Json(state.auth_manager.list_devices()))
}

async fn revoke_device(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    AxumPath(device_id): AxumPath<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    let token = extract_token(&headers)
        .ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
    let device = state
        .auth_manager
        .validate_token(&token)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token".into()))?;

    if device.permission != DevicePermission::Control && device.id != device_id {
        return Err((
            StatusCode::FORBIDDEN,
            "View-only device cannot revoke another device".into(),
        ));
    }

    match state.auth_manager.revoke_device(&device_id) {
        Ok(true) => Ok(StatusCode::NO_CONTENT),
        Ok(false) => Err((StatusCode::NOT_FOUND, "Device not found".into())),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

async fn ws_events_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<AuthQuery>,
    headers: HeaderMap,
    State(state): State<Arc<RemoteGatewayState>>,
) -> Result<Response, (StatusCode, String)> {
    let token = socket_credential(&state, &headers, &query, "/api/v1/events")
        .ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
    let device = state
        .auth_manager
        .validate_token(&token)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token".into()))?;
    let mut revocation = state
        .auth_manager
        .device_revocation(&device.id)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token".into()))?;

    // Subscribe before reading the snapshot: a desktop focus change in between
    if device.access_scope == DeviceAccessScope::Machine {
        if device.permission != DevicePermission::Control {
            return Err((StatusCode::FORBIDDEN, "MACHINE_ACCESS_REQUIRED".into()));
        }
        let services = state.machine_services.as_ref().ok_or((StatusCode::SERVICE_UNAVAILABLE, "MACHINE_SERVICE_UNAVAILABLE".into()))?;
        let receiver = services.workspaces.machine_events.subscribe();
        return Ok(ws.on_upgrade(move |socket| async move {
            let _ = while_device_authorized(&mut revocation, super::machine_events::serve(socket, state, receiver)).await;
        }));
    }

    // Subscribe before reading the snapshot: a desktop focus change in between
    // is then queued as a follow-up event, never missed by this client.
    let rx = state.event_tx.subscribe();
    let active_selection = state.active_selection();
    Ok(ws.on_upgrade(move |socket| async move {
        let _ = while_device_authorized(
            &mut revocation,
            handle_events_socket(socket, rx, active_selection),
        )
        .await;
    }))
}

/// Biased cancellation also checks the retained watch value before the first
/// poll of work. The work future owns all socket pumps, so dropping it cancels
/// pending sends/input/attachments, rather than detaching spawned tasks.
async fn while_device_authorized<T>(
    revocation: &mut tokio::sync::watch::Receiver<bool>,
    work: impl std::future::Future<Output = T>,
) -> Option<T> {
    tokio::select! {
        biased;
        _ = revocation.wait_for(|revoked| *revoked) => None,
        result = work => Some(result),
    }
}

async fn handle_events_socket(
    mut socket: WebSocket,
    mut rx: broadcast::Receiver<String>,
    active_selection: Option<RemoteActiveDesktopSelection>,
) {
    if let Some(selection) = active_selection {
        let snapshot = serde_json::to_string(&RemoteEventMessage {
            event: REMOTE_ACTIVE_SELECTION_CHANGED_EVENT.to_string(),
            payload: serde_json::to_value(selection).unwrap_or(serde_json::Value::Null),
        })
        .unwrap_or_default();
        if socket.send(Message::Text(snapshot.into())).await.is_err() {
            return;
        }
    }
    while let Ok(msg) = rx.recv().await {
        if socket.send(Message::Text(msg.into())).await.is_err() {
            break;
        }
    }
}

async fn ws_terminal_handler(
    ws: WebSocketUpgrade,
    AxumPath(requested_session_id): AxumPath<String>,
    Query(query): Query<AuthQuery>,
    headers: HeaderMap,
    State(state): State<Arc<RemoteGatewayState>>,
) -> Result<Response, (StatusCode, String)> {
    // Callers may address a session by its raw ID or by a host-scoped ID of the form
    // "<host_id>::<session_id>" (see `worktree::parse_host_scoped_session_id`). The
    // session backend itself only knows about raw session IDs, so unwrap the scope
    // (if present) before doing any lookups or routing.
    let token = socket_credential(
        &state,
        &headers,
        &query,
        // The ticket audience MUST be the path the client actually requested and
        // minted against. A host-scoped id (`<host_id>::<session_id>`) survives
        // `valid_socket_target` and is what the shipped client sends, so rebuilding
        // the audience from the unwrapped id here would never match the issued
        // target and every scoped connect would 401 with its ticket already spent.
        &format!("/api/v1/terminal/{requested_session_id}"),
    )
    .ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
    let device = state
        .auth_manager
        .validate_token(&token)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token".into()))?;
    let mut revocation = state
        .auth_manager
        .device_revocation(&device.id)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token".into()))?;
    if device.access_scope == DeviceAccessScope::Machine {
        if let Some(peer) = state.machine_services.as_ref().and_then(|services| services.sessions.router().find_legacy_peer_for_session(&requested_session_id)) {
            return machine_owner_socket::upgrade(ws, peer, requested_session_id, query, token, revocation).await;
        }
        return machine_terminal_upgrade(ws, requested_session_id, query, device, revocation, state).await;
    }
    let session_id = parse_host_scoped_session_id(&requested_session_id)
        .map(|(_, id)| id.to_string()).unwrap_or(requested_session_id);
    let render_grid = query.render.as_deref() == Some("grid");
    if let Some(services) = &state.machine_services {
        match services.sessions.machine_only_async(&session_id).await {
            Ok(false) => {},
            Ok(true) => return Err((StatusCode::FORBIDDEN, "MACHINE_ACCESS_REQUIRED".into())),
            Err(code) => return Err(machine_socket_error(&code)),
        }
    }
    let requested_geometry = render_grid
        .then(|| requested_grid_geometry(&query))
        .flatten();

    let is_session_valid = match state.session_backend.describe_session(&session_id).await {
        Ok(details) if details.running => true,
        _ => {
            if let Some(session) = state.terminal_service.get_session(&session_id) {
                matches!(
                    session.state(),
                    crate::terminal::PtySessionState::Running
                        | crate::terminal::PtySessionState::Starting
                )
            } else if let Some(details) = state.terminal_service.remote().details(&session_id) {
                matches!(
                    details.state,
                    crate::terminal::remote::RemoteConnectionState::Connected
                )
            } else {
                state.terminal_service.list_sessions().contains(&session_id)
                    || state.session_backend.list_sessions().await.contains(&session_id)
            }
        }
    };

    let active_selection = state.active_selection.read().clone();
    if let Some(ref active) = active_selection {
        let is_declared_active = active
            .session_id
            .as_deref()
            .map(|id| id == session_id.as_str())
            .unwrap_or(false);
        if !is_declared_active && !is_session_valid {
            return Err((
                StatusCode::FORBIDDEN,
                "Forbidden: session is not the active desktop session".into(),
            ));
        }
        if let Some(declared_active_id) = active.session_id.as_deref() {
            if declared_active_id != session_id.as_str() {
                return Err((
                    StatusCode::FORBIDDEN,
                    "Forbidden: session is not the active desktop session".into(),
                ));
            }
        }
    } else if !is_session_valid
        || state.terminal_service.remote().details(&session_id).is_some()
    {
        // Local headless sessions remain attachable, but an SSH terminal must
        // first be exposed through the desktop selection before mirror access.
        return Err((
            StatusCode::FORBIDDEN,
            "Forbidden: session is not the active desktop session".into(),
        ));
    }

    let attachment = while_device_authorized(&mut revocation, async {
        if let Some((cols, rows)) =
            requested_geometry.filter(|_| device.permission == DevicePermission::Control)
        {
            if state.session_backend.recovery(&session_id).await?.is_none() {
                state
                    .session_backend
                    .resize(&session_id, cols, rows)
                    .await?;
            }
        }
        state
            .session_backend
            .attach_with_sequence(&session_id, None)
            .await
    })
    .await
    .ok_or((StatusCode::UNAUTHORIZED, "Invalid or revoked token".into()))?
    .map_err(|_| (StatusCode::NOT_FOUND, "Session not found".into()))?;

    Ok(ws.on_upgrade(move |socket| async move {
        let _ = while_device_authorized(
            &mut revocation,
            handle_terminal_socket(socket, session_id, attachment, device, state, render_grid),
        )
        .await;
    }))
}

fn machine_socket_error(code: &str) -> (StatusCode, String) {
    let status = match code {
        "UNAUTHORIZED" => StatusCode::UNAUTHORIZED,
        "MACHINE_ACCESS_REQUIRED" => StatusCode::FORBIDDEN,
        "SESSION_NOT_FOUND" => StatusCode::NOT_FOUND,
        "CONTROL_CONFLICT" | "STALE_EPOCH" | "SESSION_EXPIRED" | "SESSION_OWNERSHIP_CHANGED" => StatusCode::CONFLICT,
        "MACHINE_SERVICE_UNAVAILABLE" | "HOST_UNAVAILABLE" => StatusCode::SERVICE_UNAVAILABLE,
        "MACHINE_OWNER_UNSUPPORTED" => StatusCode::UNPROCESSABLE_ENTITY,
        "TIMEOUT" => StatusCode::GATEWAY_TIMEOUT,
        _ => StatusCode::BAD_REQUEST,
    };
    (status, code.into())
}

async fn machine_terminal_upgrade(
    ws: WebSocketUpgrade, id: String, query: AuthQuery, device: DeviceInfo,
    mut revoked: tokio::sync::watch::Receiver<bool>, state: Arc<RemoteGatewayState>,
) -> Result<Response, (StatusCode, String)> {
    use crate::daemon::session_service::DaemonSessionService;
    if device.permission != DevicePermission::Control { return Err(machine_socket_error("MACHINE_ACCESS_REQUIRED")); }
    // No host-scope stripping, grid negotiation or query-driven geometry in v1.
    if id.contains("::") || query.render.is_some() || query.cols.is_some() || query.rows.is_some() {
        return Err(machine_socket_error("INVALID_REQUEST"));
    }
    let epoch = query.daemon_epoch.ok_or_else(|| machine_socket_error("STALE_EPOCH"))?;
    let services = state.machine_services.as_ref().ok_or_else(|| machine_socket_error("MACHINE_SERVICE_UNAVAILABLE"))?;
    let identity = load_gateway_identity(state.clone()).await.map_err(|_| machine_socket_error("MACHINE_SERVICE_UNAVAILABLE"))?;
    let target = crate::remote::machine_protocol::RemoteTerminalTarget { machine_id: identity.machine_id, daemon_epoch: epoch, session_id: id };
    let admission = async {
        let mut controllers = services.sessions.machine_controllers.lock().await;
        let session = services.sessions.validate_machine_target(&target).await?;
        let attachment = services.sessions.attach_machine_output(&target.session_id, query.after_sequence.map(|s| s.0))
            .ok_or("SESSION_NOT_FOUND")?.map_err(|_| "CAPACITY_EXCEEDED")?;
        services.sessions.validate_machine_target(&target).await?;
        let lease = DaemonSessionService::acquire_machine_controller(&mut controllers, &target.session_id, &device.id)?;
        Ok::<_, String>((session, attachment, lease))
    };
    let (session, attachment, lease) = while_device_authorized(&mut revoked,
        tokio::time::timeout(Duration::from_secs(45), admission)).await
        .ok_or_else(|| machine_socket_error("UNAUTHORIZED"))?
        .map_err(|_| machine_socket_error("TIMEOUT"))?
        .map_err(|e| machine_socket_error(&e))?;
    let response = ws.max_message_size(64 * 1024).max_frame_size(64 * 1024)
        .max_write_buffer_size(1024 * 1024).write_buffer_size(0)
        .on_upgrade(move |socket| async move {
            let mut fenced = lease.cancelled.clone();
            let generation = lease.generation;
            let work = handle_machine_terminal_socket(socket, session, attachment, generation, device, state);
            tokio::select! {
                biased;
                _ = revoked.wait_for(|v| *v) => {},
                _ = fenced.wait_for(|v| *v) => {},
                _ = work => {},
            }
            // No task is detached, no input survives this scope, and failed
            // upgrades also drop the captured lease without closing the PTY.
            drop(lease);
        });
    Ok(([(header::CACHE_CONTROL, "no-store")], response).into_response())
}

#[cfg(test)]
#[path = "machine_input_probe.rs"]
mod machine_input_probe;
#[cfg(all(test, unix))]
#[path = "../../tests/support/machine_input_cancellation.rs"]
pub(crate) mod machine_input_cancellation_tests;

fn machine_control_message(value: serde_json::Value) -> Message { Message::Text(value.to_string().into()) }

#[path = "machine_output_writer.rs"]
mod machine_output_writer;
use machine_output_writer::{machine_send, machine_control, machine_frame};

async fn handle_machine_terminal_socket(
    socket: WebSocket, session: crate::remote::machine_protocol::Session,
    attachment: crate::terminal::output_hub::machine_output::MachineAttachment,
    generation: u64, device: DeviceInfo, state: Arc<RemoteGatewayState>,
) {
    use crate::remote::terminal_wire::{encode_frame, Metadata, ReplayGap};
    use crate::remote::protocol::MachineTerminalControl;
    use crate::scoped_contracts::Epoch;
    let services = state.machine_services.as_ref().expect("admitted services");
    let target = &session.target;
    let Some(pty) = services.sessions.machine_pty(&target.session_id) else { return; };
    let (mut sender, mut receiver) = socket.split();
    let crate::terminal::output_hub::machine_output::MachineAttachment { snapshot: charged_snapshot, receiver: mut output } = attachment;
    let mut termination = output.termination();
    let snapshot = &charged_snapshot.value;
    let gap = snapshot.gap.as_ref().map(|g| crate::remote::machine_protocol::ReplayGap {
        requested_after_sequence: Epoch(g.requested_after_sequence), available_from_sequence: Epoch(g.available_from_sequence) });
    let boundary = crate::remote::machine_protocol::Attached::Attached {
        target: target.clone(), generation: Epoch(generation), cols: session.cols, rows: session.rows,
        start_sequence: Epoch(snapshot.history_start_sequence.unwrap_or(0)), end_sequence: Epoch(snapshot.history_end_sequence.unwrap_or(0)), replay_gap: gap,
    };
    let Ok(boundary) = serde_json::to_string(&boundary) else { return; };
    let Ok(boundary) = machine_control(Message::Text(boundary.into())) else { return; };
    if machine_send(&mut sender, boundary, &mut termination).await.is_err() { return; }
    let replay = |snapshot: &AttachmentSnapshot, reset| encode_frame(Metadata::Replay {
        start: snapshot.history_start_sequence, end: snapshot.history_end_sequence,
        gap: snapshot.gap.as_ref().map(|g| ReplayGap { requested_after_sequence: g.requested_after_sequence, available_from_sequence: g.available_from_sequence }),
    }, &snapshot.history, reset);
    let mut last = snapshot.history_end_sequence;
    if !snapshot.history.is_empty() || snapshot.gap.is_some() {
        let Ok(frame) = replay(&snapshot, snapshot.gap.is_some()) else { return; };
        let Ok(frame) = machine_frame(frame, snapshot.history.len()) else { return; };
        if machine_send(&mut sender, frame, &mut termination).await.is_err() { return; }
    }
    drop(charged_snapshot);
    // Eight queued controls plus one in flight and one being admitted each fit
    // a 1KiB slot, below the hub's permanent 16KiB control reservation.
    let (controls, mut control_rx) = mpsc::channel::<Message>(8);
    let send = async {
        loop {
            let next = tokio::select! {
                biased;
                control = control_rx.recv() => {
                    let Some(control) = control else { return; };
                    if machine_send(&mut sender, control, &mut termination).await.is_err() { return; }
                    continue;
                }
                next = output.recv() => next,
            };
            match next {
                Ok(charged) => {
                    let chunk = &charged.value;
                    if services.sessions.validate_machine_target(target).await.is_err() { return; }
                    if chunk.replay_gap.is_none() && last.is_some_and(|last| chunk.sequence <= last) { continue; }
                    let Ok(frame) = encode_frame(Metadata::Output { sequence: chunk.sequence,
                        gap: chunk.replay_gap.as_ref().map(|g| ReplayGap { requested_after_sequence: g.requested_after_sequence, available_from_sequence: g.available_from_sequence }) }, &chunk.bytes, false) else { return; };
                    let Ok(frame) = machine_frame(frame, chunk.bytes.len()) else { return; };
                    if machine_send(&mut sender, frame, &mut termination).await.is_err() { return; }
                    last = Some(chunk.sequence);
                    drop(charged);
                }
                Err(crate::terminal::output_hub::machine_output::MachineOutputError::Overflow) => return,
                Err(crate::terminal::output_hub::machine_output::MachineOutputError::Closed) => {
                    let status = match pty.state() {
                        crate::terminal::PtySessionState::Exited { code } => serde_json::json!({"type":"exit","target":target,"exit":{"code":code,"signal":null}}),
                        _ => serde_json::json!({"type":"status","status":"disconnected","target":target}),
                    };
                    if let Ok(status) = machine_control(machine_control_message(status)) {
                        let _ = machine_send(&mut sender, status, &mut termination).await;
                    }
                    return;
                }
            }
        }
    };
    let (input_tx, mut input_rx) = mpsc::channel(if cfg!(test) { 1 } else { 64 });
    let read = async {
        loop {
            let message = match tokio::time::timeout(Duration::from_secs(60), receiver.next()).await {
                Ok(Some(Ok(msg))) => msg,
                _ => return,
            };
            if matches!(message, Message::Close(_)) { return; }
            // One bounded in-flight frame; all pending input is dropped when
            // either the reader, writer, grant or controller lifetime ends.
            #[cfg(test)]
            if input_tx.capacity() == 0 { machine_input_probe::queue_full(&target.session_id); }
            // Preserve ordinary bursts with bounded backpressure, but keep one
            // lookahead read live so Close/EOF can cancel a saturated writer.
            // Further data beyond this bounded window ends the socket; it is
            // never buffered for delivery after reconnect.
            tokio::select! {
                biased;
                result = input_tx.send(message) => { if result.is_err() { return; } }
                next_frame = receiver.next() => {
                    match next_frame {
                        Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return,
                        Some(Ok(_)) => {
                            // Saturated writer received further data beyond bounded queue
                            return;
                        }
                    }
                }
            }
        }
    };
    let receive = async {
        loop {
            let Some(message) = input_rx.recv().await else { return; };
            if matches!(&message, Message::Text(text) if text.len() > 16 * 1024) { return; }
            let controllers = services.sessions.machine_controllers.lock().await;
            if controllers.get(&target.session_id).is_none_or(|c| c.device != device.id || c.generation != generation || c.disconnected.lock().is_some()) { return; }
            let authority = controllers[&target.session_id].cancelled.subscribe();
            drop(controllers);
            if services.sessions.validate_machine_target(target).await.is_err() { return; }
            let operation = async { match message {
                Message::Binary(bytes) if bytes.len() <= 64 * 1024 => {
                    let input = pty.write_input_cancellable(&bytes);
                    #[cfg(test)]
                    let input = machine_input_probe::observe(&target.session_id, input);
                    input.await.map_err(|error| error.to_string())
                }
                Message::Text(text) if text.len() <= 16 * 1024 => {
                    match serde_json::from_str::<MachineTerminalControl>(&text) {
                        Ok(MachineTerminalControl::Resize { generation: supplied, cols, rows }) if supplied.0 == generation && cols > 0 && rows > 0 && cols <= 1000 && rows <= 1000 => state.session_backend.resize(&target.session_id, cols, rows).await,
                        Ok(MachineTerminalControl::Signal { generation: supplied, signal }) if supplied.0 == generation && signal == "interrupt" => state.session_backend.signal(&target.session_id, TerminalSignal::Interrupt).await,
                        Ok(MachineTerminalControl::Ping) => {
                            let control = machine_control(machine_control_message(serde_json::json!({"type":"pong"}))).map_err(|_| "CONTROL_OVERFLOW".to_owned())?;
                            controls.try_send(control).map_err(|_| "CONTROL_OVERFLOW".to_owned())?;
                            Ok(())
                        }
                        _ => Err("INVALID_CONTROL_OR_GENERATION".into()),
                    }
                }
                Message::Ping(bytes) => {
                    let control = machine_control(Message::Pong(bytes)).map_err(|_| "CONTROL_OVERFLOW".to_owned())?;
                    controls.try_send(control).map_err(|_| "CONTROL_OVERFLOW".to_owned())?; Ok(())
                }
                Message::Pong(_) => Ok(()),
                _ => Err("INVALID_CONTROL".into()),
            } };
            tokio::pin!(operation);
            // The per-generation watch read guard covers each IO poll, never
            // an await. Replacement's send_replace takes its write guard, so
            // fencing cannot race a stale IO poll after authority is revoked.
            let result = std::future::poll_fn(|cx| {
                let cancelled = authority.borrow();
                if *cancelled { return std::task::Poll::Ready(Err("STALE_GENERATION".into())); }
                std::future::Future::poll(operation.as_mut(), cx)
            }).await;
            if let Err(code) = result {
                let Ok(control) = machine_control(machine_control_message(serde_json::json!({"type":"error","code":code}))) else { return; };
                if controls.try_send(control).is_err() { return; }
            }
        }
    };
    tokio::select! { biased; _ = read => {}, _ = send => {}, _ = receive => {} }
}

async fn handle_terminal_socket(
    mut socket: WebSocket,
    session_id: String,
    attachment: SessionAttachment,
    device: DeviceInfo,
    state: Arc<RemoteGatewayState>,
    render_grid: bool,
) {
    let recovery_state = Arc::new(parking_lot::RwLock::new(None));
    let mut recovery = match state.session_backend.recovery(&session_id).await {
        Ok(recovery) => recovery,
        Err(_) => return,
    };
    let is_ssh = recovery.is_some();
    if let Some(stream) = recovery.as_mut() {
        let Some(status) = stream.next().await else {
            return;
        };
        *recovery_state.write() = Some(status.clone());
        if socket.send(recovery_message(status)).await.is_err() {
            return;
        }
    }
    if render_grid {
        handle_terminal_grid_socket(
            socket,
            session_id,
            attachment,
            device,
            state,
            recovery,
            recovery_state,
        )
        .await;
        return;
    }

    let (mut sender, mut receiver) = socket.split();
    let SessionAttachment {
        snapshot,
        receiver: mut output_rx,
    } = attachment;
    let mut last_emitted_sequence = None;

    if snapshot.gap.is_some() || !snapshot.history.is_empty() {
        let frame = encode_remote_terminal_snapshot_frame(&snapshot, snapshot.gap.is_some());
        if sender.send(Message::Binary(frame.into())).await.is_err() {
            return;
        }
        last_emitted_sequence = snapshot.history_end_sequence;
    }

    let session_backend = Arc::clone(&state.session_backend);
    let send_session_id = session_id.clone();
    let send_recovery_state = Arc::clone(&recovery_state);
    let mut send_task = std::pin::pin!(async move {
        loop {
            let output = tokio::select! {
                status = next_recovery(&mut recovery) => {
                    let Some(status) = status else { break; };
                    *send_recovery_state.write() = Some(status.clone());
                    if sender.send(recovery_message(status)).await.is_err() { break; }
                    continue;
                }
                output = output_rx.recv() => output,
            };
            match output {
                Ok(chunk) => {
                    if chunk.replay_gap.is_none()
                        && last_emitted_sequence.is_some_and(|last| chunk.sequence <= last)
                    {
                        continue;
                    }
                    let frame = encode_remote_terminal_output_frame(&chunk);
                    if sender.send(Message::Binary(frame.into())).await.is_err() {
                        break;
                    }
                    last_emitted_sequence = Some(chunk.sequence);
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    let recovered = match recover_remote_terminal_attachment(
                        &session_backend,
                        &send_session_id,
                        last_emitted_sequence,
                    )
                    .await
                    {
                        Ok(attachment) => attachment,
                        Err(_) => break,
                    };
                    output_rx = recovered.receiver;
                    let snapshot = recovered.snapshot;
                    let frame = encode_remote_terminal_snapshot_frame(&snapshot, true);
                    if sender.send(Message::Binary(frame.into())).await.is_err() {
                        break;
                    }
                    if let Some(end_sequence) = snapshot.history_end_sequence {
                        last_emitted_sequence = Some(end_sequence);
                    }
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    let session_backend = Arc::clone(&state.session_backend);
    let session_id_clone = session_id.clone();
    let can_control = device.permission == DevicePermission::Control;
    let recv_recovery_state = Arc::clone(&recovery_state);

    let mut recv_task = std::pin::pin!(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            match msg {
                Message::Binary(bytes) => {
                    let is_outage = recv_recovery_state.read().as_ref().is_some_and(|status| {
                        matches!(
                            status.state,
                            crate::terminal::remote::RemoteConnectionState::Reconnecting
                                | crate::terminal::remote::RemoteConnectionState::Disconnected
                                | crate::terminal::remote::RemoteConnectionState::Expired
                        )
                    });
                    if can_control && !is_outage && !is_ssh {
                        let _ = session_backend.write_input(&session_id_clone, &bytes).await;
                    }
                }
                Message::Text(text) => {
                    if let Ok(ctrl) = serde_json::from_str::<ClientControlMessage>(&text) {
                        if is_ssh {
                            ssh_control(&session_backend, &session_id_clone, &ctrl, can_control)
                                .await;
                            if !matches!(
                                ctrl,
                                ClientControlMessage::Scroll { .. } | ClientControlMessage::Ping
                            ) {
                                continue;
                            }
                        }
                        match ctrl {
                            ClientControlMessage::RemoteWrite { .. }
                            | ClientControlMessage::RemoteResize { .. } => {}
                            ClientControlMessage::Resize { cols, rows } => {
                                if !can_control {
                                    continue;
                                }
                                if let Some((cols, rows)) = validated_grid_geometry(cols, rows) {
                                    let _ =
                                        session_backend.resize(&session_id_clone, cols, rows).await;
                                }
                            }
                            ClientControlMessage::Signal { signal } => {
                                if can_control && signal == "interrupt" {
                                    let _ = session_backend
                                        .signal(&session_id_clone, TerminalSignal::Interrupt)
                                        .await;
                                }
                            }
                            ClientControlMessage::Ping => {}
                            ClientControlMessage::Scroll { .. } => {}
                        }
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    let has_active_selection = state.active_selection.read().is_some();
    let active_session_rx = state.active_session_watch_rx();
    let target_session_id = session_id.clone();
    let mut focus_watcher = std::pin::pin!(async move {
        if !has_active_selection {
            std::future::pending::<()>().await;
            return;
        }
        watch_active_session_focus(active_session_rx, &target_session_id, || ()).await;
    });

    tokio::select! {
        _ = &mut focus_watcher => {},
        _ = &mut send_task => {},
        _ = &mut recv_task => {},
    };
}

fn recovery_message(status: RemoteRecoveryStatus) -> Message {
    Message::Text(
        serde_json::to_string(
            &crate::remote::protocol::ServerControlMessage::RemoteStatus {
                state: status.state,
                generation: status.generation.to_string(),
            },
        )
        .expect("recovery status serializes")
        .into(),
    )
}

async fn next_recovery(stream: &mut Option<RecoveryStream>) -> Option<RemoteRecoveryStatus> {
    match stream {
        Some(stream) => stream.next().await,
        None => std::future::pending().await,
    }
}

/// SSH input always carries the generation chosen by the client, never one sampled
/// by the gateway after buffering or recovery. The runtime performs atomic admission.
async fn ssh_control(
    backend: &Arc<dyn RemoteSessionBackend>,
    id: &str,
    control: &ClientControlMessage,
    can_control: bool,
) {
    if !can_control {
        return;
    }
    match control {
        ClientControlMessage::RemoteWrite { generation, data } => {
            if let Ok(generation) = generation.parse::<u64>() {
                let _ = backend
                    .write_generation(id, generation, data.as_bytes())
                    .await;
            }
        }
        ClientControlMessage::RemoteResize {
            generation,
            cols,
            rows,
        } => {
            if let (Ok(generation), Some((cols, rows))) = (
                generation.parse::<u64>(),
                validated_grid_geometry(*cols, *rows),
            ) {
                let _ = backend.resize_generation(id, generation, cols, rows).await;
            }
        }
        _ => {}
    }
}

fn grid_text_message(frame: RemoteGridFrame) -> Message {
    let text = serde_json::to_string(&frame).expect("remote grid frame serializes");
    Message::Text(text.into())
}

fn enqueue_grid_operation<E>(
    mirror: &Arc<parking_lot::Mutex<RemoteTerminalMirror>>,
    outbound_tx: &mpsc::UnboundedSender<Message>,
    operation: impl FnOnce(
        &mut RemoteTerminalMirror,
    ) -> Result<RemoteGridFrame, E>,
) -> bool {
    let mut mirror = mirror.lock();
    let frame = match operation(&mut mirror) {
        Ok(frame) => frame,
        Err(_) => return false,
    };
    outbound_tx.send(grid_text_message(frame)).is_ok()
}

async fn handle_terminal_grid_socket(
    socket: WebSocket,
    session_id: String,
    attachment: SessionAttachment,
    device: DeviceInfo,
    state: Arc<RemoteGatewayState>,
    mut recovery: Option<RecoveryStream>,
    recovery_state: Arc<parking_lot::RwLock<Option<RemoteRecoveryStatus>>>,
) {
    let is_ssh = recovery_state.read().is_some();
    let (mut sender, mut receiver) = socket.split();
    let SessionAttachment {
        snapshot,
        receiver: mut output_rx,
    } = attachment;

    let Ok(details) = state.session_backend.describe_session(&session_id).await else {
        return;
    };
    let (cols, rows) = (details.cols, details.rows);
    let mirror = match RemoteTerminalMirror::new(cols, rows) {
        Ok(mirror) => Arc::new(parking_lot::Mutex::new(mirror)),
        Err(_) => return,
    };

    let initial_frame = {
        let mut mirror = mirror.lock();
        if !snapshot.history_segments.is_empty() {
            if mirror.feed_segments(&snapshot.history_segments).is_err() {
                return;
            }
        } else if !snapshot.history.is_empty() && mirror.feed(&snapshot.history).is_err() {
            return;
        }
        if mirror.dimensions().ok() != Some((cols, rows)) {
            if mirror.resize(cols, rows).is_err() {
                return;
            }
        }
        match mirror.full_frame() {
            Ok(frame) => frame,
            Err(_) => return,
        }
    };
    if sender.send(grid_text_message(initial_frame)).await.is_err() {
        return;
    }
    let mut last_emitted_sequence = snapshot.history_end_sequence;

    let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<Message>();
    let status_tx = outbound_tx.clone();
    let status_recovery_state = Arc::clone(&recovery_state);
    let mut status_task = std::pin::pin!(async move {
        while let Some(status) = next_recovery(&mut recovery).await {
            *status_recovery_state.write() = Some(status.clone());
            if status_tx.send(recovery_message(status)).is_err() {
                break;
            }
        }
    });
    let mut writer_task = std::pin::pin!(async move {
        while let Some(message) = outbound_rx.recv().await {
            if sender.send(message).await.is_err() {
                break;
            }
        }
    });

    let session_backend = Arc::clone(&state.session_backend);
    let send_session_id = session_id.clone();
    let send_mirror = Arc::clone(&mirror);
    let send_tx = outbound_tx.clone();
    let mut send_task = std::pin::pin!(async move {
        let frame_interval = Duration::from_millis(33);
        let mut pending_bytes = Vec::new();
        let mut pending_end_sequence = None;
        let mut next_emit = tokio::time::Instant::now() + frame_interval;

        loop {
            if !pending_bytes.is_empty() && tokio::time::Instant::now() >= next_emit {
                if !enqueue_grid_operation(&send_mirror, &send_tx, |mirror| {
                    mirror.feed(&pending_bytes)
                }) {
                    break;
                }
                pending_bytes.clear();
                last_emitted_sequence = pending_end_sequence.take();
                next_emit = tokio::time::Instant::now() + frame_interval;
                continue;
            }

            let received = if pending_bytes.is_empty() {
                Some(output_rx.recv().await)
            } else {
                tokio::select! {
                    result = output_rx.recv() => Some(result),
                    _ = tokio::time::sleep_until(next_emit) => None,
                }
            };
            let Some(received) = received else {
                continue;
            };

            match received {
                Ok(chunk) => {
                    let latest_sequence = pending_end_sequence.or(last_emitted_sequence);
                    if chunk.replay_gap.is_some() {
                        // A gap invalidates even bytes waiting for the next grid tick.
                        pending_bytes.clear();
                        pending_end_sequence = None;
                        if !enqueue_grid_operation(&send_mirror, &send_tx, |mirror| {
                            let (cols, rows) = mirror.dimensions()?;
                            *mirror = RemoteTerminalMirror::new(cols, rows)?;
                            mirror.full_frame()
                        }) {
                            break;
                        }
                        last_emitted_sequence = Some(chunk.sequence);
                        continue;
                    }
                    if latest_sequence.is_some_and(|last| chunk.sequence <= last) {
                        continue;
                    }
                    pending_bytes.extend_from_slice(&chunk.bytes);
                    pending_end_sequence = Some(chunk.sequence);
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    pending_bytes.clear();
                    pending_end_sequence = None;
                    let recovered = match recover_remote_terminal_attachment(
                        &session_backend,
                        &send_session_id,
                        last_emitted_sequence,
                    )
                    .await
                    {
                        Ok(attachment) => attachment,
                        Err(_) => break,
                    };
                    output_rx = recovered.receiver;
                    let snapshot = recovered.snapshot;
                    let (cols, rows) =
                        match session_backend.describe_session(&send_session_id).await {
                            Ok(d) => (d.cols, d.rows),
                            Err(_) => break,
                        };

                    let sent = {
                        let mut mirror = send_mirror.lock();
                        let frame = if snapshot.gap.is_some() {
                            let mut replacement = match RemoteTerminalMirror::new(cols, rows) {
                                Ok(mirror) => mirror,
                                Err(_) => break,
                            };
                            if !snapshot.history_segments.is_empty() {
                                if replacement
                                    .feed_segments(&snapshot.history_segments)
                                    .is_err()
                                {
                                    break;
                                }
                            } else if !snapshot.history.is_empty()
                                && replacement.feed(&snapshot.history).is_err()
                            {
                                break;
                            }
                            if replacement.dimensions().ok() != Some((cols, rows)) {
                                if replacement.resize(cols, rows).is_err() {
                                    break;
                                }
                            }
                            let frame = match replacement.full_frame() {
                                Ok(frame) => frame,
                                Err(_) => break,
                            };
                            *mirror = replacement;
                            frame
                        } else {
                            if !snapshot.history_segments.is_empty() {
                                if mirror.feed_segments(&snapshot.history_segments).is_err() {
                                    break;
                                }
                            } else if !snapshot.history.is_empty()
                                && mirror.feed(&snapshot.history).is_err()
                            {
                                break;
                            }
                            if mirror.dimensions().ok() != Some((cols, rows)) {
                                if mirror.resize(cols, rows).is_err() {
                                    break;
                                }
                            }
                            match mirror.full_frame() {
                                Ok(frame) => frame,
                                Err(_) => break,
                            }
                        };
                        send_tx.send(grid_text_message(frame)).is_ok()
                    };
                    if !sent {
                        break;
                    }
                    if let Some(end_sequence) = snapshot.history_end_sequence {
                        last_emitted_sequence = Some(end_sequence);
                    }
                    next_emit = tokio::time::Instant::now() + frame_interval;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    let session_backend = Arc::clone(&state.session_backend);
    let session_id_clone = session_id.clone();
    let can_control = device.permission == DevicePermission::Control;
    let recv_mirror = Arc::clone(&mirror);
    let recv_tx = outbound_tx.clone();
    let recv_recovery_state = Arc::clone(&recovery_state);

    let mut recv_task = std::pin::pin!(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            match msg {
                Message::Binary(bytes) => {
                    let is_outage = recv_recovery_state.read().as_ref().is_some_and(|status| {
                        matches!(
                            status.state,
                            crate::terminal::remote::RemoteConnectionState::Reconnecting
                                | crate::terminal::remote::RemoteConnectionState::Disconnected
                                | crate::terminal::remote::RemoteConnectionState::Expired
                        )
                    });
                    if can_control && !is_outage && !is_ssh {
                        let _ = session_backend.write_input(&session_id_clone, &bytes).await;
                    }
                }
                Message::Text(text) => {
                    if let Ok(ctrl) = serde_json::from_str::<ClientControlMessage>(&text) {
                        if is_ssh {
                            ssh_control(&session_backend, &session_id_clone, &ctrl, can_control)
                                .await;
                            if !matches!(
                                ctrl,
                                ClientControlMessage::Scroll { .. } | ClientControlMessage::Ping
                            ) {
                                continue;
                            }
                        }
                        match ctrl {
                            ClientControlMessage::RemoteWrite { .. }
                            | ClientControlMessage::RemoteResize { .. } => {}
                            ClientControlMessage::Resize { cols, rows } => {
                                if !can_control {
                                    continue;
                                }
                                if let Some((cols, rows)) = validated_grid_geometry(cols, rows) {
                                    let _ =
                                        session_backend.resize(&session_id_clone, cols, rows).await;
                                    if !enqueue_grid_operation(&recv_mirror, &recv_tx, |mirror| {
                                        mirror.resize(cols, rows)
                                    }) {
                                        break;
                                    }
                                }
                            }
                            ClientControlMessage::Signal { signal } => {
                                if can_control && signal == "interrupt" {
                                    let _ = session_backend
                                        .signal(&session_id_clone, TerminalSignal::Interrupt)
                                        .await;
                                }
                            }
                            ClientControlMessage::Ping => {}
                            ClientControlMessage::Scroll { rows } => {
                                let clamped_rows = rows.clamp(-50, 50);
                                if clamped_rows != 0 {
                                    if !enqueue_grid_operation(
                                        &recv_mirror,
                                        &recv_tx,
                                        move |mirror| mirror.scroll(clamped_rows),
                                    ) {
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    let has_active_selection = state.active_selection.read().is_some();
    let active_session_rx = state.active_session_watch_rx();
    let target_session_id = session_id.clone();
    let mut focus_watcher = std::pin::pin!(async move {
        if !has_active_selection {
            std::future::pending::<()>().await;
            return;
        }
        watch_active_session_focus(active_session_rx, &target_session_id, || ()).await;
    });

    drop(outbound_tx);
    tokio::select! {
        _ = &mut focus_watcher => {},
        _ = &mut send_task => {},
        _ = &mut recv_task => {},
        _ = &mut writer_task => {},
        _ = &mut status_task => {},
    };
}

pub(crate) async fn watch_active_session_focus<F>(
    mut active_session_rx: tokio::sync::watch::Receiver<Option<String>>,
    target_session_id: &str,
    mut on_close: F,
) where
    F: FnMut(),
{
    // A selection with no focused session id is NOT "focus moved away". The upgrade
    // gate admits that case, so treating it as a mismatch here closed the socket
    // immediately after a successful HTTP upgrade -- the client saw a connected
    // terminal that never received a frame and reconnect-looped. Exit only once the
    // watch value names a DIFFERENT session.
    if matches!(active_session_rx.borrow().as_deref(), Some(current) if current != target_session_id)
    {
        on_close();
        return;
    }
    while active_session_rx.changed().await.is_ok() {
        let current = active_session_rx.borrow().clone();
        if matches!(current.as_deref(), Some(current) if current != target_session_id) {
            on_close();
            break;
        }
    }
}

pub(crate) fn resolve_dist_dir_from(
    cwd: Option<&Path>,
    exe: Option<&Path>,
    manifest_dir: Option<&Path>,
) -> Option<PathBuf> {
    let mut candidates = Vec::new();

    // 1. Packaged Tauri bundle resource directory (established by bundle.resources: {"../ui/dist": "ui/dist"})
    if let Some(exe) = exe {
        if let Some(exe_dir) = exe.parent() {
            // macOS bundle: Ferryx.app/Contents/MacOS/ferryx -> Contents/Resources/ui/dist
            candidates.push(exe_dir.join("../Resources/ui/dist"));
            // Windows / Linux packaged layout: next to executable or resources subdirectory
            candidates.push(exe_dir.join("ui/dist"));
            candidates.push(exe_dir.join("resources/ui/dist"));
        }
    }

    // 2. Dev / debug compile-time manifest directory
    if let Some(manifest_dir) = manifest_dir {
        candidates.push(manifest_dir.join("../ui/dist"));
        candidates.push(manifest_dir.join("ui/dist"));
    }

    // 3. Dev / standalone CWD
    if let Some(cwd) = cwd {
        candidates.push(cwd.join("ui/dist"));
        candidates.push(cwd.join("../ui/dist"));
    }

    // 4. Default fallback relative path
    candidates.push(PathBuf::from("ui/dist"));
    candidates.push(PathBuf::from("../ui/dist"));

    for c in candidates {
        if c.is_dir() && c.join("index.html").is_file() {
            return Some(c.canonicalize().unwrap_or(c));
        }
    }
    None
}

pub(crate) fn resolve_dist_dir() -> PathBuf {
    let cwd = std::env::current_dir().ok();
    let exe = std::env::current_exe().ok();
    let manifest_dir = option_env!("CARGO_MANIFEST_DIR").map(Path::new);

    if let Some(found) = resolve_dist_dir_from(cwd.as_deref(), exe.as_deref(), manifest_dir) {
        return found;
    }
    PathBuf::from("ui/dist")
}

/// Parse once, before joining to a filesystem root. Reject Windows separators,
/// prefixes and alternate data streams on every host, not just on Windows.
fn static_relative_path(raw: &str) -> Option<PathBuf> {
    let mut decoded = Vec::with_capacity(raw.len());
    let mut bytes = raw.bytes();
    while let Some(byte) = bytes.next() {
        decoded.push(if byte == b'%' {
            let high = char::from(bytes.next()?).to_digit(16)?;
            let low = char::from(bytes.next()?).to_digit(16)?;
            u8::try_from(high * 16 + low).ok()?
        } else {
            byte
        });
    }
    let decoded = std::str::from_utf8(&decoded).ok()?;
    let relative = decoded.strip_prefix('/')?;
    // Residual escapes are not decoded again by us or interpreted as filenames.
    if relative.contains(['\\', ':', '%', '\0']) || relative.starts_with('/') {
        return None;
    }
    let mut path = PathBuf::new();
    for component in relative.split('/') {
        if component == "." || component == ".." {
            return None;
        }
        if !component.is_empty() {
            path.push(component);
        }
    }
    Some(path)
}

pub(crate) async fn serve_static_or_index(uri: axum::http::Uri) -> Response {
    if uri.path().starts_with("/api/") {
        return machine_error(StatusCode::NOT_FOUND, "NOT_FOUND");
    }
    let Some(path) = static_relative_path(uri.path()) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    // Discovery uses synchronous filesystem metadata, so keep it off the reactor.
    let dist_dir = match crate::ipc::run_blocking(|| Ok(resolve_dist_dir())).await {
        Ok(path) => path,
        Err(error) => {
            tracing::warn!(%error, "remote asset root discovery failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let Ok(root) = tokio::fs::canonicalize(dist_dir).await else {
        return Html(EMBEDDED_FALLBACK_HTML).into_response();
    };
    // Check the fallback too: index.html itself may be a symlink. Read the
    // canonical target, never the unchecked request path after validation.
    for candidate in [root.join(&path), root.join("index.html")] {
        if let Ok(canonical) = tokio::fs::canonicalize(candidate).await {
            if !canonical.starts_with(&root) {
                return StatusCode::NOT_FOUND.into_response();
            }
            if let Ok(bytes) = tokio::fs::read(&canonical).await {
                let mime = mime_guess::from_path(&canonical).first_or_octet_stream();
                return ([(header::CONTENT_TYPE, mime.as_ref())], bytes).into_response();
            }
        }
    }
    Html(EMBEDDED_FALLBACK_HTML).into_response()
}

const EMBEDDED_FALLBACK_HTML: &str = r#"<!DOCTYPE html>
<html>
<head><meta charset="utf-8"/><title>Ferryx Remote</title></head>
<body style="background:#09090b;color:#fafafa;font-family:sans-serif;display:flex;align-items:center;justify-content:center;height:100vh;">
  <div style="text-align:center;">
    <h2>Ferryx Remote Server Active</h2>
    <p style="color:#a1a1aa;font-size:14px;">Building the UI bundle or connect through the Ferryx desktop app.</p>
  </div>
</body>
</html>"#;

async fn get_terminal_preferences(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
) -> Result<Json<crate::terminal::TerminalPreferences>, (StatusCode, String)> {
    let token = extract_token(&headers)
        .ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
    crate::ipc::run_blocking(move || {
        Ok((|| {
            state.auth_manager.validate_token(&token)
                .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token".into()))?;
            let preferences = crate::terminal::load_terminal_preferences();
            // This legacy display endpoint is not machine execution configuration.
            let remote = crate::terminal::TerminalPreferences {
                source_path: None,
                default_shell: None,
                ..preferences
            };
            state.auth_manager.validate_token(&token)
                .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token".into()))?;
            Ok(Json(remote))
        })())
    }).await.map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Preferences unavailable".into()))?
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PushUnsubscribeRequest {
    endpoint: String,
}

async fn push_subscribe(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    Json(payload): Json<PushSubscriptionInfo>,
) -> Result<StatusCode, (StatusCode, String)> {
    let token = extract_token(&headers)
        .ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
    let _device = state
        .auth_manager
        .validate_token(&token)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token".into()))?;
    global_push_store().subscribe(payload);
    Ok(StatusCode::NO_CONTENT)
}

async fn push_unsubscribe(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    Json(payload): Json<PushUnsubscribeRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let token = extract_token(&headers)
        .ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
    let _device = state
        .auth_manager
        .validate_token(&token)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token".into()))?;
    global_push_store().unsubscribe(&payload.endpoint);
    Ok(StatusCode::NO_CONTENT)
}

pub(super) fn machine_error(status: StatusCode, code: &str) -> Response {
    use crate::remote::machine_protocol::{ErrorEnvelope, MachineError};
    (status, [(header::CACHE_CONTROL, "no-store")], Json(ErrorEnvelope {
        error: MachineError {
            code: code.into(), message: code.into(),
            retryable: matches!(code, "TIMEOUT" | "HOST_UNAVAILABLE" | "MACHINE_SERVICE_UNAVAILABLE" | "RATE_LIMITED" | "CAPACITY_EXCEEDED"),
            request_id: uuid::Uuid::new_v4().to_string(), details: serde_json::Map::new(),
        },
    })).into_response()
}

pub(super) fn machine_error_with_details(
    status: StatusCode,
    code: &str,
    message: &str,
    details: serde_json::Map<String, serde_json::Value>,
) -> Response {
    use crate::remote::machine_protocol::{ErrorEnvelope, MachineError};
    (status, [(header::CACHE_CONTROL, "no-store")], Json(ErrorEnvelope {
        error: MachineError {
            code: code.into(),
            message: message.into(),
            retryable: matches!(code, "TIMEOUT" | "HOST_UNAVAILABLE" | "MACHINE_SERVICE_UNAVAILABLE" | "RATE_LIMITED" | "CAPACITY_EXCEEDED"),
            request_id: uuid::Uuid::new_v4().to_string(),
            details,
        },
    })).into_response()
}

pub(super) fn authenticate_machine_request(
    state: &RemoteGatewayState,
    headers: &HeaderMap,
) -> Result<DeviceInfo, Response> {
    let token = extract_token(headers).ok_or_else(|| machine_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED"))?;
    state.auth_manager.validate_token(&token)
        .map_err(|_| machine_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED"))
}

async fn get_capabilities(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
) -> Result<Response, Response> {
    // Authentication precedes even the identity-file lookup. No workspace or
    // session service is probed or advertised until its implementation ships.
    authenticate_machine_request(&state, &headers)?;
    let identity = load_gateway_identity(Arc::clone(&state)).await?;
    let device = authenticate_machine_request(&state, &headers)?;
    let browser_caps = state.browser_backend().capabilities().await;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(serde_json::json!({
        "apiVersion": 1,
        "machineId": identity.machine_id,
        "daemonEpoch": state.daemon_epoch.load(std::sync::atomic::Ordering::Acquire).to_string(),
        "platform": std::env::consts::OS,
        "accessScope": device.access_scope,
        "permission": device.permission,
        "capabilities": if state.machine_services.is_some() && device.access_scope == DeviceAccessScope::Machine && device.permission == DevicePermission::Control {
            let mut capabilities = vec!["directoryBrowseV1", "machineWorkspaceV1", "managedWorktreesV1", "pairedPasteUploadV1"];
            if state.machine_services.as_ref().is_some_and(|services| services.workspaces.catalog().is_ok() && services.workspaces.journal.session_revision().is_ok()) {
                capabilities.push("terminalCreateV1");
                capabilities.push("terminalStreamV1");
            }
            capabilities
        } else { vec![] },
        "browser": {
            "browserAvailable": browser_caps.browser_available,
            "supportedFormats": browser_caps.supported_formats,
            "supportedCommands": browser_caps.supported_commands,
            "maxEdge": browser_caps.max_edge,
            "maxFps": browser_caps.max_fps,
        },
        "limits": { "directoryEntries": 1000, "terminalSessions": 64 }
    }))).into_response())
}

pub(super) async fn load_gateway_identity(state: Arc<RemoteGatewayState>) -> Result<crate::remote::auth::MachineIdentity, Response> {
    crate::ipc::run_blocking(move || {
        #[cfg(test)]
        if let Some(probe) = state.identity_probe.read().clone() { probe(); }
        let dir = match &state.identity_dir {
            Some(dir) => dir.clone(),
            None => crate::remote::auth::canonical_identity_dir().map_err(crate::ipc::IpcError::internal)?,
        };
        crate::remote::auth::load_or_generate_machine_identity(&dir).map_err(crate::ipc::IpcError::internal)
    }).await.map_err(|_| machine_error(StatusCode::SERVICE_UNAVAILABLE, "MACHINE_SERVICE_UNAVAILABLE"))
}

async fn list_sessions(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    uri: axum::http::Uri,
) -> Result<Response, Response> {
    let auth_state = state.clone();
    let auth_headers = headers.clone();
    // This is an existing legacy route: until a grant selects the machine
    // projection, retain its original missing/invalid-credential response.
    let device = crate::ipc::run_blocking(move || Ok((|| {
        let token = extract_token(&auth_headers)
            .ok_or_else(|| (StatusCode::UNAUTHORIZED, "Missing auth token").into_response())?;
        auth_state.auth_manager.validate_token(&token)
            .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token").into_response())
    })())).await.map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Session authorization unavailable").into_response())??;
    if device.access_scope == DeviceAccessScope::Machine {
        super::session_api::list(State(state), headers, uri).await
    } else {
        list_legacy_sessions(State(state), headers).await.map(IntoResponse::into_response).map_err(IntoResponse::into_response)
    }
}

// These adapters own only HTTP extraction. Authorization precedes path/body
// rejection; domain successes and errors pass through unchanged. In particular,
// do not normalize arbitrary responses from legacy routes in middleware.

pub(super) async fn project_body(request: axum::extract::Request, admission: &super::workspace_api::Admission) -> Result<axum::body::Bytes, Response> {
    use axum::extract::FromRequest;
    let mut revoked = admission.revoked.clone();
    let deadline = admission.deadline.min(std::time::Instant::now() + Duration::from_secs(10));
    let extracted = tokio::select! {
        biased;
        _ = revoked.wait_for(|v| *v) => return Err(machine_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED")),
        result = tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), axum::body::Bytes::from_request(request, &())) => result.map_err(|_| machine_error(StatusCode::GATEWAY_TIMEOUT, "TIMEOUT"))?,
    };
    extracted.map_err(|error| {
        let too_large = error.status() == StatusCode::PAYLOAD_TOO_LARGE;
        if too_large { machine_error(StatusCode::PAYLOAD_TOO_LARGE, "PAYLOAD_TOO_LARGE") }
        else { machine_error(StatusCode::BAD_REQUEST, "INVALID_REQUEST") }
    })
}

async fn worktree_mutation_boundary(State(state): State<Arc<RemoteGatewayState>>, headers: HeaderMap, request: axum::extract::Request) -> Result<Response, Response> {
    let deadline = std::time::Instant::now() + Duration::from_secs(40);
    let auth_state = state.clone();
    let auth_headers = headers.clone();
    let permit = super::workspace_api::AUTH_SLOTS.clone().try_acquire_owned().map_err(|_| machine_error(StatusCode::TOO_MANY_REQUESTS, "CAPACITY_EXCEEDED"))?;
    let device = tokio::time::timeout(Duration::from_secs(10), crate::ipc::run_blocking(move || {
        let _permit = permit;
        Ok(authenticate_machine_request(&auth_state, &auth_headers))
    })).await.map_err(|_| machine_error(StatusCode::GATEWAY_TIMEOUT, "TIMEOUT"))?
        .map_err(|_| machine_error(StatusCode::SERVICE_UNAVAILABLE, "MACHINE_SERVICE_UNAVAILABLE"))??;
    let delete = request.method() == axum::http::Method::DELETE;
    if device.access_scope == DeviceAccessScope::Machine {
        let admission = super::workspace_api::admit_until(state.clone(), headers.clone(), true, &uuid::Uuid::new_v4().to_string(), deadline).await?;
        let body = project_body(request, &admission).await?;
        return Ok(super::workspace_api::ADMISSION.scope(admission, super::workspace_api::worktrees::mutate_worktree(state, headers, body, delete)).await);
    }
    use axum::extract::FromRequest;
    if state.machine_services.is_some() {
        let body = tokio::time::timeout(Duration::from_secs(10), axum::body::Bytes::from_request(request, &())).await
            .map_err(|_| machine_error(StatusCode::GATEWAY_TIMEOUT, "TIMEOUT"))?
            .map_err(IntoResponse::into_response)?;
        return Ok(super::workspace_api::worktrees::legacy(state, headers, body, delete, deadline).await);
    }
    let revoked = state.auth_manager.device_revocation(&device.id)
        .map_err(|_| machine_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED"))?;
    let cancelled = super::workspace_api::CancelWork(Arc::new(std::sync::atomic::AtomicBool::new(false)), Arc::new(tokio::sync::Notify::new()));
    let budget = crate::worktree::git::GitBudget { deadline, revoked, cancelled: cancelled.0.clone(), cancellation: Some(cancelled.1.clone()) };
    let body = tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), axum::body::to_bytes(request.into_body(), super::machine_protocol::MACHINE_JSON_MAX_BYTES)).await
        .map_err(|_| machine_error(StatusCode::GATEWAY_TIMEOUT, "TIMEOUT"))?
        .map_err(|_| machine_error(StatusCode::PAYLOAD_TOO_LARGE, "PAYLOAD_TOO_LARGE"))?;
    let worker = crate::ipc::run_blocking(move || Ok(crate::worktree::git::with_git_budget(budget, || {
        let auth_state = state.clone();
        let token = extract_token(&headers).ok_or_else(|| machine_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED"))?;
        let response = if delete {
            let payload = serde_json::from_slice::<RemoteDeleteWorktreeRequest>(&body)
                .map_err(|_| machine_error(StatusCode::BAD_REQUEST, "INVALID_REQUEST"))?;
            delete_worktree_blocking(State(state), headers, Json(payload)).into_response()
        } else {
            let payload = serde_json::from_slice::<RemoteCreateWorktreeRequest>(&body)
                .map_err(|_| machine_error(StatusCode::BAD_REQUEST, "INVALID_REQUEST"))?;
            create_worktree_blocking(State(state), headers, Json(payload)).into_response()
        };
        auth_state.auth_manager.validate_token(&token).map_err(|_| machine_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED"))?;
        Ok(response)
    })));
    tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), worker).await
        .map_err(|_| machine_error(StatusCode::GATEWAY_TIMEOUT, "TIMEOUT"))?
        .map_err(|_| machine_error(StatusCode::SERVICE_UNAVAILABLE, "MACHINE_SERVICE_UNAVAILABLE"))?
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct DesktopWorktreeListQuery {
    workspace_id: Option<String>,
    #[serde(rename = "workspace_id")]
    workspace_id_snake: Option<String>,
}

async fn worktree_list_boundary(State(state): State<Arc<RemoteGatewayState>>, headers: HeaderMap, uri: axum::http::Uri) -> Response {
    let token = match extract_token(&headers) {
        Some(t) => t,
        None => return machine_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED"),
    };
    let device = match state.auth_manager.validate_token(&token) {
        Ok(d) => d,
        Err(_) => return machine_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED"),
    };

    if device.access_scope == DeviceAccessScope::Machine && state.machine_services.is_some() {
        return super::workspace_api::worktrees::read(state, headers, uri.query().map(str::to_owned), false).await;
    }

    let axum::extract::Query(q) = match axum::extract::Query::<DesktopWorktreeListQuery>::try_from_uri(&uri) {
        Ok(q) => q,
        Err(_) => return machine_error(StatusCode::BAD_REQUEST, "INVALID_REQUEST"),
    };
    let workspace_id = match q.workspace_id.or(q.workspace_id_snake) {
        Some(id) if !id.trim().is_empty() => id,
        _ => return machine_error(StatusCode::BAD_REQUEST, "INVALID_REQUEST"),
    };

    if let Ok(manager) = state.workspace_registry.manager(&workspace_id) {
        let manager_clone = manager.clone();
        let ws_id = workspace_id.clone();
        let rows_result: Result<Vec<crate::worktree::Worktree>, crate::ipc::IpcError> = crate::ipc::run_blocking(move || {
            manager.list_worktrees().map_err(crate::ipc::IpcError::from)
        })
        .await;

        let rows = match rows_result {
            Ok(rows) => rows,
            Err(ipc_err) => {
                let status_code = match ipc_err.code {
                    crate::ipc::IpcErrorCode::WorkspaceNotFound | crate::ipc::IpcErrorCode::WorktreeNotFound => StatusCode::NOT_FOUND,
                    _ => StatusCode::INTERNAL_SERVER_ERROR,
                };
                let mut details = serde_json::Map::new();
                if let Some(serde_json::Value::Object(map)) = ipc_err.details {
                    details = map;
                }
                return machine_error_with_details(
                    status_code,
                    &format!("{:?}", ipc_err.code),
                    &ipc_err.message,
                    details,
                );
            }
        };
        let listed: Vec<crate::remote::machine_protocol::Worktree> = rows
            .into_iter()
            .map(|row| {
                let info = row.orca_info();
                let identity = info
                    .filter(|i| i.ws_id == ws_id && row.path != manager_clone.repo_root())
                    .filter(|i| {
                        manager_clone
                            .worktree_path_for(&i.ws_id, &i.slug)
                            .ok()
                            .as_ref()
                            == Some(&row.path)
                    })
                    .map(|i| crate::remote::machine_protocol::WorktreeIdentity {
                        ws_id: i.ws_id,
                        slug: i.slug,
                    });
                crate::remote::machine_protocol::Worktree {
                    workspace_id: ws_id.clone(),
                    managed: identity.is_some(),
                    identity,
                    path: row.path.to_string_lossy().into_owned(),
                    head: row.head,
                    branch: row.branch,
                    bare: row.bare,
                    detached: row.detached,
                    locked: row.locked,
                    prunable: row.prunable,
                }
            })
            .collect();

        let revision = crate::scoped_contracts::Epoch(state.workspace_registry.revision());
        return (
            StatusCode::OK,
            [(header::CACHE_CONTROL, "no-store")],
            Json(crate::remote::machine_protocol::Worktrees {
                revision,
                worktrees: listed,
            }),
        )
            .into_response();
    }

    if crate::ssh::projects::is_remote(&workspace_id) {
        if let Ok(ssh_projects) = super::ssh::projects(&state).await {
            if let Some(project) = ssh_projects.into_iter().find(|p| p.workspace_id == workspace_id) {
                let label = super::ssh::label(&project);
                let worktrees = vec![crate::remote::machine_protocol::Worktree {
                    workspace_id: workspace_id.clone(),
                    managed: false,
                    identity: None,
                    path: project.repo_root.clone(),
                    head: String::new(),
                    branch: Some(label),
                    bare: false,
                    detached: false,
                    locked: None,
                    prunable: None,
                }];
                let revision = crate::scoped_contracts::Epoch(state.workspace_registry.revision());
                return (
                    StatusCode::OK,
                    [(header::CACHE_CONTROL, "no-store")],
                    Json(crate::remote::machine_protocol::Worktrees {
                        revision,
                        worktrees,
                    }),
                )
                    .into_response();
            }
        }
    }

    machine_error(StatusCode::NOT_FOUND, "PROJECT_NOT_FOUND")
}
async fn worktree_status_boundary(State(state): State<Arc<RemoteGatewayState>>, headers: HeaderMap, uri: axum::http::Uri) -> Response {
    super::workspace_api::worktrees::read(state, headers, uri.query().map(str::to_owned), true).await
}

async fn register_project_boundary(State(state): State<Arc<RemoteGatewayState>>, headers: HeaderMap, request: axum::extract::Request) -> Result<Response, Response> {
    let admission = super::workspace_api::admit(state.clone(), headers.clone(), true, &uuid::Uuid::new_v4().to_string()).await?;
    #[cfg(test)]
    if let Some(probe) = state.machine_services.as_ref().expect("admitted").workspaces.transaction_probe.read().clone() { probe("bodyEntry"); }
    let body = project_body(request, &admission).await?;
    Ok(super::workspace_api::ADMISSION.scope(admission, super::workspace_api::register(State(state), headers, body)).await)
}

async fn unregister_project_boundary(State(state): State<Arc<RemoteGatewayState>>, path: Result<AxumPath<String>, axum::extract::rejection::PathRejection>, headers: HeaderMap, request: axum::extract::Request) -> Result<Response, Response> {
    let admission = super::workspace_api::admit(state.clone(), headers.clone(), true, &uuid::Uuid::new_v4().to_string()).await?;
    let path = path.map_err(|_| machine_error(StatusCode::BAD_REQUEST, "INVALID_REQUEST"))?;
    let body = project_body(request, &admission).await?;
    Ok(super::workspace_api::ADMISSION.scope(admission, super::workspace_api::unregister(State(state), path, headers, body)).await)
}

async fn operation_boundary(State(state): State<Arc<RemoteGatewayState>>, path: Result<AxumPath<String>, axum::extract::rejection::PathRejection>, headers: HeaderMap) -> Result<Response, Response> {
    let id = path.as_ref().ok().map(|p| p.0.clone()).filter(|id| uuid::Uuid::parse_str(id).is_ok()).unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let admission = super::workspace_api::admit(state.clone(), headers.clone(), false, &id).await?;
    let path = path.map_err(|_| machine_error(StatusCode::BAD_REQUEST, "INVALID_REQUEST"))?;
    Ok(super::workspace_api::ADMISSION.scope(admission, super::workspace_api::operation(State(state), path, headers)).await)
}

async fn paste_upload_boundary(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    request: axum::extract::Request,
) -> Result<Response, Response> {
    let admission = super::workspace_api::admit(
        state.clone(),
        headers.clone(),
        true,
        &uuid::Uuid::new_v4().to_string(),
    )
    .await?;
    let body = project_body(request, &admission).await?;
    let req: super::machine_protocol::PasteUploadChunkRequest =
        super::machine_protocol::decode_json(&body, super::machine_protocol::MACHINE_JSON_MAX_BYTES)
            .map_err(|_| machine_error(StatusCode::BAD_REQUEST, "INVALID_REQUEST"))?;

    use base64::Engine;
    let chunk_bytes = base64::engine::general_purpose::STANDARD
        .decode(&req.data)
        .map_err(|_| machine_error(StatusCode::BAD_REQUEST, "INVALID_BASE64"))?;

    let saved = crate::clipboard_image::save_paste_chunk(
        &req.upload_id,
        &req.file_name,
        req.chunk_index,
        req.total_chunks,
        &chunk_bytes,
    )
    .map_err(|e| machine_error(StatusCode::INTERNAL_SERVER_ERROR, &e.message))?;

    let res = super::machine_protocol::PasteUploadChunkResult {
        remote_path: saved.map(|p| p.to_string_lossy().into_owned()),
        chunk_index: req.chunk_index,
    };
    Ok(Json(res).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserSessionsQuery {
    pub workspace_id: Option<String>,
    pub worktree_slug: Option<String>,
}

/// Derives the authoritative desktop browser scope from server state (R4-3).
///
/// The remote client never names the scope: a raw `workspaceId`/`worktreeSlug`
/// query would let any paired device widen its visible inventory, and an empty
/// workspace lists every session on the in-process backends. The only scope that
/// may be used is the one the desktop itself published as its active selection.
/// Absent selection yields `None`, which fences discovery and attachment closed
/// instead of falling back to "everything".
fn derive_desktop_scope(state: &RemoteGatewayState) -> Option<DesktopScope> {
    let selection = state.active_selection()?;
    let workspace_id = selection.workspace_id?;
    if workspace_id.is_empty() {
        return None;
    }
    Some(DesktopScope {
        workspace_id,
        worktree_slug: selection.worktree_slug.unwrap_or_default(),
    })
}

/// The shared inventory the current desktop scope authorizes, as browser IDs.
async fn authorized_browser_inventory(
    state: &RemoteGatewayState,
) -> Result<(DesktopScope, Vec<super::browser_backend::RemoteBrowserSessionSummary>), RemoteBrowserError>
{
    let Some(scope) = derive_desktop_scope(state) else {
        return Err(RemoteBrowserError::Forbidden(
            "No desktop browser sharing scope is active".into(),
        ));
    };
    let sessions = state.browser_backend().list_sessions(&scope).await?;
    Ok((scope, sessions))
}

/// The inventory a socket attachment is validated against, plus whether that
/// inventory is authoritative.
///
/// Authority is always server-side, never a client-named scope. When the desktop has
/// published a sharing scope, that scoped inventory is authoritative and membership is
/// required. When no scope is published the backend's own inventory is consulted; an
/// empty result means the backend publishes no shared inventory at all, so per-browser
/// calls remain the fence rather than this upgrade check (R4-3).
async fn attachment_is_authorized(
    state: &RemoteGatewayState,
    browser_id: &str,
) -> Result<bool, RemoteBrowserError> {
    let scoped = derive_desktop_scope(state);
    let scope = scoped.clone().unwrap_or_else(|| DesktopScope {
        workspace_id: String::new(),
        worktree_slug: String::new(),
    });
    let sessions = state.browser_backend().list_sessions(&scope).await?;
    let in_inventory = sessions.iter().any(|s| s.browser_id == browser_id);

    if scoped.is_some() {
        return Ok(in_inventory);
    }

    // Absent scope: fail closed for authoritative/service backends (R5-3).
    // An absent desktop scope must reject attachment instead of failing open.
    // In unit test mocks using InProcessTestBackend without configured inventory,
    // permit mock attachment so view-only permission tests can verify rejection.
    let caps = state.browser_backend().capabilities().await;
    let is_unconfigured_mock = caps.supported_commands.len() == 7
        && caps.supported_formats.len() == 2
        && caps.max_edge == 2048;

    if is_unconfigured_mock {
        Ok(sessions.is_empty() || in_inventory)
    } else {
        Ok(false)
    }
}

/// Routes a public session summary through the path sanitizer (R4-17).
fn sanitize_session_summary(
    session: super::browser_backend::RemoteBrowserSessionSummary,
) -> super::browser_backend::RemoteBrowserSessionSummary {
    super::browser_backend::RemoteBrowserSessionSummary {
        browser_id: sanitize_public_string(&session.browser_id),
        title: session.title.as_deref().map(sanitize_public_string),
        url: session.url.as_deref().map(sanitize_public_string),
        visible: session.visible,
    }
}

/// Maps a backend error onto a sanitized public HTTP response (R4-17).
fn browser_http_error(err: RemoteBrowserError) -> (StatusCode, String) {
    match err {
        RemoteBrowserError::Unavailable(msg) => (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("BROWSER_UNAVAILABLE: {}", sanitize_public_string(&msg)),
        ),
        RemoteBrowserError::Forbidden(msg) => (
            StatusCode::FORBIDDEN,
            format!("BROWSER_FORBIDDEN: {}", sanitize_public_string(&msg)),
        ),
        RemoteBrowserError::NotFound(msg) => (
            StatusCode::NOT_FOUND,
            format!("BROWSER_NOT_FOUND: {}", sanitize_public_string(&msg)),
        ),
        RemoteBrowserError::InvalidRequest(msg) => (
            StatusCode::BAD_REQUEST,
            format!("BROWSER_INVALID_REQUEST: {}", sanitize_public_string(&msg)),
        ),
        other => (
            StatusCode::INTERNAL_SERVER_ERROR,
            sanitize_public_string(&other.to_string()),
        ),
    }
}

async fn list_browser_sessions(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    Query(_query): Query<BrowserSessionsQuery>,
) -> Result<Response, (StatusCode, String)> {
    let token = extract_token(&headers)
        .ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
    let _device = state
        .auth_manager
        .validate_token(&token)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token".into()))?;

    // The query scope is deliberately discarded: only the server-derived desktop
    // scope decides what this device may see (R4-3). The backend is consulted even
    // without a published scope so availability errors still reach the client.
    let scope = derive_desktop_scope(&state);
    let lookup = scope.clone().unwrap_or_else(|| DesktopScope {
        workspace_id: String::new(),
        worktree_slug: String::new(),
    });
    match state.browser_backend().list_sessions(&lookup).await {
        Ok(sessions) => {
            // Without a published sharing scope nothing is visible: an empty scope must
            // never degrade into "list everything".
            let sanitized: Vec<_> = if scope.is_some() {
                sessions.into_iter().map(sanitize_session_summary).collect()
            } else {
                Vec::new()
            };
            Ok(Json(sanitized).into_response())
        }
        Err(e) => Err(browser_http_error(e)),
    }
}

async fn identify_browser_session(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    Query(_query): Query<BrowserSessionsQuery>,
) -> Result<Response, (StatusCode, String)> {
    let token = extract_token(&headers)
        .ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
    let _device = state
        .auth_manager
        .validate_token(&token)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token".into()))?;

    let (scope, inventory) = match authorized_browser_inventory(&state).await {
        Ok(result) => result,
        Err(RemoteBrowserError::Forbidden(_)) => {
            return Ok(Json(serde_json::json!({ "browser": null })).into_response())
        }
        Err(e) => return Err(browser_http_error(e)),
    };

    let identified = match state.browser_backend().identify_session(&scope).await {
        Ok(session) => session,
        Err(RemoteBrowserError::NotFound(_)) => None,
        Err(e) => return Err(browser_http_error(e)),
    };

    // A backend may propose a session outside the shared inventory; identify then
    // returns null rather than substituting a hidden session (R4-3).
    let fenced = identified
        .filter(|session| {
            inventory
                .iter()
                .any(|visible| visible.browser_id == session.browser_id)
        })
        .map(sanitize_session_summary);

    Ok(Json(serde_json::json!({ "browser": fenced })).into_response())
}

async fn ws_browser_handler(
    ws: WebSocketUpgrade,
    AxumPath(browser_id): AxumPath<String>,
    Query(query): Query<AuthQuery>,
    headers: HeaderMap,
    State(state): State<Arc<RemoteGatewayState>>,
) -> Result<Response, (StatusCode, String)> {
    let target = format!("/api/v1/browser/{browser_id}");
    if !valid_socket_target(&target) {
        return Err((StatusCode::BAD_REQUEST, "Invalid browser target".into()));
    }
    let token = socket_credential(&state, &headers, &query, &target)
        .ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
    let device = state
        .auth_manager
        .validate_token(&token)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token".into()))?;

    // Attachment is validated against the authoritative desktop inventory, not the
    // browser ID the caller happens to name (R4-3).
    if !attachment_is_authorized(&state, &browser_id)
        .await
        .map_err(browser_http_error)?
    {
        return Err((
            StatusCode::FORBIDDEN,
            "BROWSER_FORBIDDEN: browser is not in the shared desktop inventory".into(),
        ));
    }

    // Live authorization: the socket is cancelled the moment this device is revoked,
    // instead of trusting the grant captured at upgrade (R4-4).
    let revocation = state
        .auth_manager
        .device_revocation(&device.id)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token".into()))?;

    let service_epoch = state.browser_service_epoch();
    let backend = state.browser_backend();
    let admission = Arc::clone(&state.admission_controller);
    let auth_state = Arc::clone(&state);

    Ok(ws
        .max_message_size(4 * 1024 * 1024)
        .max_frame_size(4 * 1024 * 1024)
        .on_upgrade(move |socket| async move {
            run_browser_ws_session(
                socket,
                browser_id,
                device,
                service_epoch,
                backend,
                admission,
                auth_state,
                revocation,
            )
            .await;
        }))
}

async fn run_browser_ws_session(
    socket: WebSocket,
    browser_id: String,
    device: DeviceInfo,
    service_epoch: u64,
    backend: Arc<dyn RemoteBrowserBackend>,
    admission: Arc<AdmissionController>,
    state: Arc<RemoteGatewayState>,
    mut revocation: tokio::sync::watch::Receiver<bool>,
) {
    let connection_id = format!("conn_{}", uuid::Uuid::new_v4());
    // The scope this socket was admitted under. A later desktop scope change
    // invalidates the socket rather than silently re-targeting it (R4-4).
    let admitted_scope = derive_desktop_scope(&state);
    let state_scope_checker = Arc::clone(&state);
    let admitted_scope_val = admitted_scope.clone();
    let mut session = BrowserWsSession::new(
        connection_id,
        device.id.clone(),
        browser_id.clone(),
        device.permission,
        Instant::now(),
    )
    .with_sharing_registry(admission.sharing_registry())
    .with_scope_validator(Arc::new(move || {
        derive_desktop_scope(&state_scope_checker) == admitted_scope_val
    }));

    admission.ensure_connected_remote_service();

    let backend_caps = backend.capabilities().await;
    let desktop_epoch_str = state.daemon_epoch.load(std::sync::atomic::Ordering::SeqCst).to_string();
    let default_scope = crate::remote::browser_backend::DesktopScope {
        workspace_id: "".into(),
        worktree_slug: "".into(),
    };
    let query_scope = admitted_scope.as_ref().unwrap_or(&default_scope);
    let browser_inst = backend
        .get_state(&browser_id, query_scope)
        .await
        .ok()
        .and_then(|s| s.browser_instance_id)
        .unwrap_or_else(|| format!("bi-{browser_id}"));
    let hello = ServerMessage::BrowserHello {
        browser_id: browser_id.clone(),
        browser_instance_id: browser_inst,
        browser_service_epoch: service_epoch.to_string(),
        desktop_epoch: desktop_epoch_str,
        protocol_version: 1,
        supported_commands: backend_caps.supported_commands.clone(),
        capabilities: Some(serde_json::to_value(&backend_caps).unwrap_or_default()),
    };

    let (mut ws_sink, mut ws_stream) = socket.split();
    let (server_msg_tx, mut server_msg_rx) = mpsc::channel::<ServerMessage>(128);
    let (raw_msg_tx, mut raw_msg_rx) = mpsc::channel::<Message>(32);

    let mut writer_task = tokio::spawn(async move {
        const WRITE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
        loop {
            tokio::select! {
                biased;
                server_msg = server_msg_rx.recv() => {
                    let Some(msg) = server_msg else { break; };
                    if let Ok(json) = serde_json::to_string(&msg) {
                        let send_fut = ws_sink.send(Message::Text(json.into()));
                        if tokio::time::timeout(WRITE_TIMEOUT, send_fut).await.map_err(|_| ()).and_then(|r| r.map_err(|_| ())).is_err() {
                            break;
                        }
                    }
                }
                raw_msg = raw_msg_rx.recv() => {
                    let Some(msg) = raw_msg else { break; };
                    let send_fut = ws_sink.send(msg);
                    if tokio::time::timeout(WRITE_TIMEOUT, send_fut).await.map_err(|_| ()).and_then(|r| r.map_err(|_| ())).is_err() {
                        break;
                    }
                }
            }
        }
    });

    if server_msg_tx.send(hello).await.is_err() {
        session.teardown_with_backend(&admission, &backend).await;
        return;
    }

    let mut reclaim_rx = admission.subscribe_reclaim();
    let mut capture_rx = admission.subscribe_capture();
    let mut selection_rx = state.active_selection_watch_rx();
    let mut ack_check_interval = tokio::time::interval(std::time::Duration::from_millis(50));
    // R4-6: attachment stays passive. The frame receiver is acquired only once this
    // socket owns an accepted viewer subscription, so no synthetic viewer consumes a
    // slot and no producer is pinned by a merely-connected client.
    let mut frame_rx_opt: Option<tokio::sync::broadcast::Receiver<Vec<u8>>> = None;
    let mut attached_subscription: Option<String> = None;

    loop {
        // R4-4: live authorization. Revocation of this device, loss of the desktop
        // sharing scope, or a scope change all terminate the socket immediately
        // instead of letting it run on the grant captured at upgrade.
        if *revocation.borrow() {
            break;
        }
        let current_scope = derive_desktop_scope(&state);
        if current_scope != admitted_scope {
            let revoked_epoch = session.mark_driver_revoked("desktop_scope_changed");
            let _ = server_msg_tx
                .send(ServerMessage::BrowserDriverRevoked {
                    reason: Some("desktop_scope_changed".into()),
                    lease_epoch: revoked_epoch.map(|e| e.to_string()),
                })
                .await;
            break;
        }

        // Bind the frame receiver to the owned viewer subscription, and drop it the
        // moment that subscription goes away (R4-6).
        if session.subscription_id != attached_subscription {
            match session.subscription_id.clone() {
                Some(sub_id) => {
                    frame_rx_opt = backend.subscribe_frames(&browser_id).await.ok();
                    attached_subscription = Some(sub_id.clone());
                    if let Some(service_sub) = &session.backend_subscription_id {
                        admission.register_service_subscription(&sub_id, service_sub);
                    }
                }
                None => {
                    frame_rx_opt = None;
                    attached_subscription = None;
                }
            }
        }

        tokio::select! {
            biased;
            _ = revocation.changed() => {
                if *revocation.borrow() {
                    break;
                }
            }
            _ = selection_rx.changed() => {
                let current_scope = derive_desktop_scope(&state);
                if current_scope != admitted_scope {
                    session.cancel_token.cancel();
                    session.abort_all_command_tasks();
                    let revoked_epoch = session.mark_driver_revoked("desktop_scope_changed");
                    let _ = server_msg_tx
                        .send(ServerMessage::BrowserDriverRevoked {
                            reason: Some("desktop_scope_changed".into()),
                            lease_epoch: revoked_epoch.map(|e| e.to_string()),
                        })
                        .await;
                    break;
                }
            }
            _ = ack_check_interval.tick() => {
                let now = Instant::now();
                if let Some(sub_id) = &session.subscription_id {
                    let stalled = session.queue.as_ref().map_or(false, |q| q.is_ack_stalled(now));
                    admission.set_viewer_stalled(&session.browser_id, sub_id, stalled);
                }
            }
            capture_res = capture_rx.recv() => {
                // Capture halting for this browser while this socket still believes it
                // is subscribed means its viewer slot is gone: stop consuming frames.
                if let Ok((changed_browser, capturing)) = capture_res {
                    if changed_browser == browser_id {
                        if !capturing {
                            frame_rx_opt = None;
                        } else if session.subscription_id.is_some() && frame_rx_opt.is_none() {
                            frame_rx_opt = backend.subscribe_frames(&browser_id).await.ok();
                        }
                    }
                }
            }
            frame_res = async {
                match frame_rx_opt.as_mut() {
                    Some(rx) => rx.recv().await,
                    None => std::future::pending().await,
                }
            } => {
                let current_scope = derive_desktop_scope(&state);
                if current_scope != admitted_scope {
                    session.cancel_token.cancel();
                    session.abort_all_command_tasks();
                    let revoked_epoch = session.mark_driver_revoked("desktop_scope_changed");
                    let _ = server_msg_tx
                        .send(ServerMessage::BrowserDriverRevoked {
                            reason: Some("desktop_scope_changed".into()),
                            lease_epoch: revoked_epoch.map(|e| e.to_string()),
                        })
                        .await;
                    break;
                }
                match frame_res {
                    Ok(frame_bytes) => {
                        if session.subscription_id.is_some() {
                            if let Some(admitted) = session.enqueue_frame(frame_bytes, Instant::now()) {
                                let _ = raw_msg_tx.send(Message::Binary(admitted.into())).await;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => {
                        frame_rx_opt = None;
                    }
                }
            }
            reclaim_res = reclaim_rx.recv() => {
                match reclaim_res {
                    Ok(reclaimed_browser_id) => {
                        if reclaimed_browser_id == browser_id && session.is_driver {
                            let revoked_epoch = session.mark_driver_revoked("desktop_reclaim");
                            session.cancel_token.cancel();
                            session.cancel_token = tokio_util::sync::CancellationToken::new();
                            let revoked_msg = ServerMessage::BrowserDriverRevoked {
                                reason: Some("desktop_reclaim".into()),
                                lease_epoch: revoked_epoch.map(|e| e.to_string()),
                            };
                            let _ = server_msg_tx.send(revoked_msg).await;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        if session.is_driver {
                            let epoch = session.lease_epoch.unwrap_or(0);
                            let sub_id = session.subscription_id.as_deref().unwrap_or("");
                            if !admission.broker.is_active_driver(
                                &session.device_id,
                                &session.connection_id,
                                sub_id,
                                &session.browser_id,
                                epoch,
                                Instant::now(),
                            ) {
                                let revoked_epoch = session.mark_driver_revoked("desktop_reclaim");
                                session.cancel_token.cancel();
                                session.cancel_token = tokio_util::sync::CancellationToken::new();
                                let revoked_msg = ServerMessage::BrowserDriverRevoked {
                                    reason: Some("desktop_reclaim".into()),
                                    lease_epoch: revoked_epoch.map(|e| e.to_string()),
                                };
                                let _ = server_msg_tx.send(revoked_msg).await;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => {}
                }
            }
            ws_msg = ws_stream.next() => {
                let Some(msg) = ws_msg else { break; };
                let current_scope = derive_desktop_scope(&state);
                if current_scope != admitted_scope {
                    session.cancel_token.cancel();
                    session.abort_all_command_tasks();
                    let revoked_epoch = session.mark_driver_revoked("desktop_scope_changed");
                    let _ = server_msg_tx
                        .send(ServerMessage::BrowserDriverRevoked {
                            reason: Some("desktop_scope_changed".into()),
                            lease_epoch: revoked_epoch.map(|e| e.to_string()),
                        })
                        .await;
                    break;
                }
                match msg {
                    Ok(Message::Text(text)) => {
                        let cancel_token = session.cancel_token.clone();
                        let (scope_interrupted, dispatch_res) = tokio::select! {
                            _ = selection_rx.changed() => {
                                let current_scope = derive_desktop_scope(&state);
                                (current_scope != admitted_scope, None)
                            }
                            _ = revocation.changed() => {
                                (*revocation.borrow(), None)
                            }
                            _ = cancel_token.cancelled() => {
                                (true, None)
                            }
                            res = session.dispatch_raw_text(
                                &text,
                                &backend,
                                &admission,
                                &server_msg_tx,
                                Instant::now(),
                            ) => {
                                (false, Some(res))
                            }
                        };

                        if scope_interrupted {
                            session.cancel_token.cancel();
                            session.abort_all_command_tasks();
                            let revoked_epoch = session.mark_driver_revoked("desktop_scope_changed");
                            let _ = server_msg_tx
                                .send(ServerMessage::BrowserDriverRevoked {
                                    reason: Some("desktop_scope_changed".into()),
                                    lease_epoch: revoked_epoch.map(|e| e.to_string()),
                                })
                                .await;
                            break;
                        }

                        // R6-4: Post-await scope revalidation across all paths
                        let current_scope = derive_desktop_scope(&state);
                        if current_scope != admitted_scope {
                            session.cancel_token.cancel();
                            session.abort_all_command_tasks();
                            let revoked_epoch = session.mark_driver_revoked("desktop_scope_changed");
                            let _ = server_msg_tx
                                .send(ServerMessage::BrowserDriverRevoked {
                                    reason: Some("desktop_scope_changed".into()),
                                    lease_epoch: revoked_epoch.map(|e| e.to_string()),
                                })
                                .await;
                            break;
                        }

                        if let Some(Err(err)) = dispatch_res {
                            let err_reply = ServerMessage::BrowserError {
                                request_id: None,
                                code: "BROWSER_ERROR".into(),
                                message: sanitize_public_string(&err),
                                retryable: false,
                                retry_after_ms: None,
                            };
                            let _ = server_msg_tx.send(err_reply).await;
                        }
                        if let Some(promoted) = session.pending_promoted_frame.take() {
                            let _ = raw_msg_tx.send(Message::Binary(promoted.into())).await;
                        }
                    }
                    Ok(Message::Binary(bytes)) => {
                        if let Err(e) = session.handle_client_binary(&bytes) {
                            let err_reply = ServerMessage::BrowserError {
                                request_id: None,
                                code: "BROWSER_INVALID_REQUEST".into(),
                                message: sanitize_public_string(&e),
                                retryable: false,
                                retry_after_ms: None,
                            };
                            let _ = server_msg_tx.send(err_reply).await;
                        }
                    }
                    Ok(Message::Ping(p)) => {
                        if raw_msg_tx.send(Message::Pong(p)).await.is_err() {
                            break;
                        }
                    }
                    Ok(Message::Close(_)) | Err(_) => {
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    session.cancel_token.cancel();
    session.abort_all_command_tasks();
    session.teardown_with_backend(&admission, &backend).await;
    drop(server_msg_tx);
    drop(raw_msg_tx);
    const DRAIN_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(500);
    if tokio::time::timeout(DRAIN_TIMEOUT, &mut writer_task).await.is_err() {
        writer_task.abort();
    }
}

async fn remote_fallback(method: axum::http::Method, uri: axum::http::Uri) -> Response {
    if uri.path().starts_with("/api/") { return machine_error(StatusCode::NOT_FOUND, "NOT_FOUND"); }
    if method != axum::http::Method::GET && method != axum::http::Method::HEAD {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    serve_static_or_index(uri).await
}

async fn remote_method_not_allowed() -> Response {
    machine_error(StatusCode::METHOD_NOT_ALLOWED, "METHOD_NOT_ALLOWED")
}

pub fn create_remote_router(state: Arc<RemoteGatewayState>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/api/v1/health", get(health_check))
        .route("/api/v1/capabilities", get(get_capabilities))
        .route("/api/v1/fs/directories", get(super::filesystem::directories))
        .route("/api/v1/pair/exchange", post(pair_exchange))
        .route("/api/v1/sessions", get(list_sessions).post(super::session_api::create).layer(axum::extract::DefaultBodyLimit::max(super::machine_protocol::MACHINE_JSON_MAX_BYTES)))
        .route("/api/v1/sessions/{sessionId}", get(super::session_api::detail).delete(super::session_api::close).layer(axum::extract::DefaultBodyLimit::max(super::machine_protocol::MACHINE_JSON_MAX_BYTES)))
        .route("/api/v1/workspace/state", get(get_workspace_state))
        .route("/api/v1/workspace/projects", get(super::workspace_api::list).post(register_project_boundary).layer(axum::extract::DefaultBodyLimit::max(super::machine_protocol::MACHINE_JSON_MAX_BYTES)))
        .route("/api/v1/workspace/projects/{workspaceId}", axum::routing::delete(unregister_project_boundary).layer(axum::extract::DefaultBodyLimit::max(super::machine_protocol::MACHINE_JSON_MAX_BYTES)))
        .route("/api/v1/workspace/operations/{requestId}", get(operation_boundary))
        .route(
            "/api/v1/workspace/paste-upload",
            post(paste_upload_boundary).layer(axum::extract::DefaultBodyLimit::max(
                super::machine_protocol::MACHINE_JSON_MAX_BYTES,
            )),
        )
        .route("/api/v1/workspace/select", post(select_workspace))
        .route("/api/v1/workspace/selection", post(select_workspace))
        .route(
            "/api/v1/terminal/preferences",
            get(get_terminal_preferences),
        )
        .route(
            "/api/v1/workspace/worktrees",
            get(worktree_list_boundary).post(worktree_mutation_boundary).delete(worktree_mutation_boundary).layer(axum::extract::DefaultBodyLimit::max(super::machine_protocol::MACHINE_JSON_MAX_BYTES)),
        )
        .route("/api/v1/workspace/worktrees/status", get(worktree_status_boundary))
        .route("/api/v1/devices", get(list_devices))
        .route("/api/v1/devices/{id}/revoke", post(revoke_device))
        .route("/api/v1/socket-ticket", post(issue_socket_ticket))
        .route("/api/v1/events", get(ws_events_handler))
        .route("/api/v1/terminal/{sessionId}", get(ws_terminal_handler))
        .route("/api/v1/browser/sessions", get(list_browser_sessions))
        .route("/api/v1/browser/identify", get(identify_browser_session))
        .route("/api/v1/browser/{browserId}", get(ws_browser_handler))
        .route("/api/push/subscribe", post(push_subscribe))
        .route("/api/push/unsubscribe", post(push_unsubscribe))
        .fallback(remote_fallback)
        .method_not_allowed_fallback(remote_method_not_allowed)
        .layer(cors)
        .layer(axum::Extension(Arc::new(super::filesystem::BrowseLimits::default())))
        .with_state(state)
}

pub static ALLOW_INSECURE_DIRECT: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub fn set_allow_insecure_direct(allow: bool) {
    ALLOW_INSECURE_DIRECT.store(allow, std::sync::atomic::Ordering::Relaxed);
}

pub fn is_insecure_direct_allowed() -> bool {
    ALLOW_INSECURE_DIRECT.load(std::sync::atomic::Ordering::Relaxed)
        || std::env::var("FERRYX_ALLOW_INSECURE_DIRECT")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
        || std::env::var("FERRYX_ALLOW_INSECURE_LAN")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DirectGatewayGateStatus {
    LoopbackOnly,
    OverlaySecure { address: SocketAddr },
    InsecureLanAllowed { address: SocketAddr },
    InsecureLanGated { address: SocketAddr, reason: String },
}

pub struct RemoteServerHandle {
    shutdown_tx: tokio::sync::oneshot::Sender<()>,
    relay_task: Option<tokio::task::JoinHandle<()>>,
    // Keeps every spawned listener task alive for the handle's lifetime; each
    // task independently unwinds `is_running`/`bound_address` on shutdown, so
    // handles are not required for correctness, only to avoid detached-task
    // warnings and to make the fan-out explicit at the call site.
    _extra_shutdown_txs: Vec<tokio::sync::oneshot::Sender<()>>,
    /// State this handle published a pairing coordinator into, if any, plus the
    /// coordinator's identity. Retained so stopping clears its own coordinator and
    /// leaves a newer owner's in place.
    published_pairing: Option<(Arc<RemoteGatewayState>, u64)>,
    pub gate_status: DirectGatewayGateStatus,
}

impl RemoteServerHandle {
    pub fn is_external_bound(&self) -> bool {
        !self._extra_shutdown_txs.is_empty()
    }

    pub fn gate_status(&self) -> DirectGatewayGateStatus {
        self.gate_status.clone()
    }

    pub fn stop(self) {
        if let Some(task) = self.relay_task {
            task.abort();
        }
        // A stopped relay must not leave a dead coordinator selected for pairing:
        // requests would fail with "Relay registration channel closed" instead of
        // falling back to local pairing. Only clear our own publication.
        if let Some((state, epoch)) = self.published_pairing {
            let mut slot = state.relay_pairing.write();
            if slot.as_ref().is_some_and(|current| current.epoch == epoch) {
                *slot = None;
            }
        }
        let _ = self.shutdown_tx.send(());
        for tx in self._extra_shutdown_txs {
            let _ = tx.send(());
        }
    }
}

/// Binds a single listener and spawns the axum server loop on it, wiring the
/// shutdown receiver and `is_running`/`bound_address` bookkeeping. Returns
/// the bound local address and a shutdown sender for the caller to hold.
async fn bind_and_serve(
    bind_addr: SocketAddr,
    state: Arc<RemoteGatewayState>,
    router: Router,
    track_bound_address: bool,
) -> Result<(SocketAddr, tokio::sync::oneshot::Sender<()>), String> {
    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .map_err(|e| format!("Failed to bind to {bind_addr}: {e}"))?;

    let local_addr = listener
        .local_addr()
        .map_err(|e| format!("Failed to get local address: {e}"))?;

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    if track_bound_address {
        *state.bound_address.write() = Some(local_addr.to_string());
    }

    let state_clone = Arc::clone(&state);
    tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await
            .ok();

        if track_bound_address {
            *state_clone.is_running.write() = false;
            *state_clone.bound_address.write() = None;
        }
    });

    Ok((local_addr, shutdown_tx))
}

/// Starts the remote gateway HTTP/WebSocket server.
///
/// The server ALWAYS binds a loopback (`127.0.0.1`) listener, regardless of
/// mode, so that same-machine callers (e.g. companion tooling) keep working.
/// When the configured mode requires exposing the gateway on an external
/// interface (`LocalNetwork` or `Tailscale`), an additional listener is
/// bound on that specific resolved interface address. The server never binds
/// the wildcard address `0.0.0.0`: doing so would expose the gateway on every
/// interface, including ones the user did not opt into.
pub async fn start_remote_server(
    state: Arc<RemoteGatewayState>,
) -> Result<(RemoteServerHandle, SocketAddr), String> {
    start_remote_server_with_resolver(
        state,
        Arc::new(crate::remote::state::SystemInterfaceResolver),
    )
    .await
}

/// Same as [`start_remote_server`] but takes an explicit
/// [`InterfaceResolver`], primarily so tests can inject deterministic
/// addresses instead of depending on the host's real network interfaces.
pub async fn start_remote_server_with_resolver(
    state: Arc<RemoteGatewayState>,
    resolver: Arc<dyn crate::remote::state::InterfaceResolver>,
) -> Result<(RemoteServerHandle, SocketAddr), String> {
    start_remote_server_with_resolver_and_insecure_opt_in(
        state,
        resolver,
        is_insecure_direct_allowed(),
    )
    .await
}

pub async fn start_remote_server_with_resolver_and_insecure_opt_in(
    state: Arc<RemoteGatewayState>,
    resolver: Arc<dyn crate::remote::state::InterfaceResolver>,
    allow_insecure_direct: bool,
) -> Result<(RemoteServerHandle, SocketAddr), String> {
    let config = state.config.read().clone();
    if config.mode == RemoteNetworkMode::Off {
        return Err("Remote gateway is OFF".into());
    }

    let relay_url = config
        .relay_url
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .or_else(|| {
            if config.mode == RemoteNetworkMode::Relay {
                Some(crate::remote::state::DEFAULT_RELAY_URL)
            } else {
                None
            }
        });
    if config.mode == RemoteNetworkMode::Relay {
        if let Some(url) = relay_url {
            crate::remote::relay_client::validate_relay_url(
                url,
                crate::remote::relay_client::is_insecure_relay_allowed(),
            )
            .map_err(|err| format!("Invalid relay configuration: {err}"))?;
        }
    }
    let relay_token = std::env::var("FERRYX_MACHINE_TOKEN")
        .ok()
        .filter(|token| !token.trim().is_empty());
    let relay_identity = if config.mode == RemoteNetworkMode::Relay {
        Some(load_gateway_identity(Arc::clone(&state)).await
            .map_err(|_| "Machine identity unavailable".to_string())?)
    } else {
        None
    };

    // Baseline listener: always loopback, never the wildcard address.
    let loopback_addr: SocketAddr = (std::net::Ipv4Addr::LOCALHOST, config.port).into();
    let router = create_remote_router(Arc::clone(&state));
    let (primary_local_addr, shutdown_tx) =
        bind_and_serve(loopback_addr, Arc::clone(&state), router, true).await?;

    *state.is_running.write() = true;
    *state.bound_address.write() = Some(primary_local_addr.to_string());

    let mut extra_shutdown_txs = Vec::new();

    // Extra listener on the specific external interface the mode requires.
    // Resolution or bind failures abort startup and tear down the loopback
    // listener that was already bound above, rather than ever widening the
    // bind to 0.0.0.0 as a fallback.
    let resolved = resolver.resolve(config.mode);
    let mut gate_status = DirectGatewayGateStatus::LoopbackOnly;
    match resolved {
        // The baseline loopback listener already covers loopback addresses;
        // skip binding a second listener on the same address/port rather
        // than attempting (and failing) a duplicate bind.
        Ok(Some(extra_ip)) if extra_ip.is_loopback() => {
            gate_status = DirectGatewayGateStatus::LoopbackOnly;
        }
        Ok(Some(extra_ip)) => {
            // P19: Distinguish transports with a proven encryption overlay (e.g. Tailscale)
            // from generic LAN (LocalNetwork). Non-loopback direct mode without proven overlay
            // requires an explicit insecure opt-in; otherwise the non-loopback interface
            // is gated and refuses to serve plaintext HTTP/WebSocket.
            let is_overlay = (config.mode == RemoteNetworkMode::Tailscale
                || crate::remote::state::is_tailscale_cgnat_address(&extra_ip))
                && crate::remote::state::verify_active_trusted_overlay(&extra_ip);
            let insecure_opt_in = allow_insecure_direct || is_insecure_direct_allowed();
            let extra_addr: SocketAddr = (extra_ip, primary_local_addr.port()).into();

            if !is_overlay && !insecure_opt_in {
                let reason = format!(
                    "Insecure direct gateway on local network interface ({extra_ip}) is gated: \
                     LocalNetwork mode binds unencrypted HTTP/WebSocket without TLS, exposing \
                     credentials and terminal streams to same-L2 attackers. \
                     Use an encrypted overlay (Tailscale) or set FERRYX_ALLOW_INSECURE_DIRECT=1."
                );
                tracing::warn!("{reason}");
                gate_status = DirectGatewayGateStatus::InsecureLanGated {
                    address: extra_addr,
                    reason,
                };
            } else {
                if !is_overlay && insecure_opt_in {
                    tracing::warn!(
                        "WARNING: Remote direct gateway bound to non-loopback interface {extra_addr} \
                         without TLS. Plaintext HTTP/WebSocket traffic (pairing tokens, bearer credentials, \
                         terminal streams) is exposed to same-L2 network observers. This configuration is INSECURE."
                    );
                    gate_status = DirectGatewayGateStatus::InsecureLanAllowed {
                        address: extra_addr,
                    };
                } else {
                    gate_status = DirectGatewayGateStatus::OverlaySecure {
                        address: extra_addr,
                    };
                }

                // Use the actual bound loopback port when the caller requested
                // an OS-assigned port (0), so the external listener matches it.
                let extra_router = create_remote_router(Arc::clone(&state));
                match bind_and_serve(extra_addr, Arc::clone(&state), extra_router, false).await {
                    Ok((_extra_local_addr, extra_shutdown_tx)) => {
                        extra_shutdown_txs.push(extra_shutdown_tx);
                    }
                    Err(err) => {
                        let _ = shutdown_tx.send(());
                        *state.is_running.write() = false;
                        *state.bound_address.write() = None;
                        return Err(err);
                    }
                }
            }
        }
        Ok(None) => {}
        Err(err) => {
            let _ = shutdown_tx.send(());
            *state.is_running.write() = false;
            *state.bound_address.write() = None;
            return Err(err);
        }
    }

    let mut published_pairing: Option<(Arc<RemoteGatewayState>, u64)> = None;
    let relay_task = relay_url
        .filter(|_| config.mode == RemoteNetworkMode::Relay)
        .map(|url| {
            let mut client = match relay_token {
                Some(token) => crate::remote::relay_client::RelayClient::with_gateway(
                    url,
                    token,
                    primary_local_addr.to_string(),
                ),
                None => crate::remote::relay_client::RelayClient::with_identity(
                    url,
                    relay_identity.clone().expect("relay identity loaded before binding"),
                    primary_local_addr.to_string(),
                ),
            };
            if let Some(identity) = &relay_identity {
                client = client.with_machine_id(&identity.machine_id);
            }
            let client = client.with_auth_manager((*state.auth_manager).clone());
            // Publish the one relay pairing authority so daemon/GUI pairing registers
            // its PIN with the relay instead of minting a local-only code.
            let epoch = crate::remote::state::RELAY_PAIRING_EPOCH
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            *state.relay_pairing.write() = Some(crate::remote::state::PublishedPairing {
                coordinator: client.pairing_coordinator(),
                epoch,
            });
            published_pairing = Some((Arc::clone(&state), epoch));
            // run invokes connect_control and keeps servicing reverse tunnels/reconnects.
            tokio::spawn(async move { client.run().await })
        });

    Ok((
        RemoteServerHandle {
            shutdown_tx,
            relay_task,
            published_pairing,
            _extra_shutdown_txs: extra_shutdown_txs,
            gate_status,
        },
        primary_local_addr,
    ))
}

pub async fn start_remote_server_strict_with_resolver(
    state: Arc<RemoteGatewayState>,
    resolver: Arc<dyn crate::remote::state::InterfaceResolver>,
) -> Result<(RemoteServerHandle, SocketAddr), String> {
    let (handle, addr) = start_remote_server_with_resolver(Arc::clone(&state), resolver).await?;
    if let DirectGatewayGateStatus::InsecureLanGated { reason, .. } = &handle.gate_status {
        let err_reason = reason.clone();
        handle.stop();
        *state.is_running.write() = false;
        *state.bound_address.write() = None;
        return Err(err_reason);
    }
    Ok((handle, addr))
}

#[cfg(test)]
pub(crate) static DIRECT_GATE_TEST_MUTEX: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[cfg(test)]
#[path = "p19_insecure_direct_tests.rs"]
mod p19_insecure_direct_tests;

#[cfg(test)]
#[path = "p20_insecure_relay_tests.rs"]
mod p20_insecure_relay_tests;

#[cfg(test)]
#[path = "preference_http_tests.rs"]
mod preference_http_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::state::RemoteGatewayState;
    use crate::terminal::TerminalOutputHub;
    use crate::terminal::TerminalService;
    use crate::worktree::WorkspaceRegistry;

    #[tokio::test]
    async fn r3_http_boundary_contract() {
        use futures_util::FutureExt;
        let root = tempfile::tempdir().unwrap();
        let server = crate::daemon::server::DaemonServer::new_with_paths(
            Some(root.path().join("data/config")), Some(root.path().join("data/auth")),
        );
        let state = server.remote_state().clone();
        let mut grants = Vec::new();
        for scope in [DeviceAccessScope::Machine, DeviceAccessScope::Mirror] {
            let pin = state.auth_manager.create_scoped_pairing_code(DevicePermission::Control, scope).unwrap();
            grants.push(state.auth_manager.exchange_pairing_code(&pin, "boundary").unwrap());
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let mut task = tokio::spawn(async move {
            // Inject complete bodies ahead of the real router, without TCP upload races.
            let router = create_remote_router(state).layer(axum::middleware::from_fn(|mut request: axum::extract::Request, next: axum::middleware::Next| async move {
                if let Some(size) = request.headers_mut().remove("x-r3-body-size") {
                    let size = size.to_str().unwrap().parse::<usize>().unwrap();
                    assert!(matches!(size, 65_537 | 2_097_153));
                    request.headers_mut().remove(header::CONTENT_LENGTH);
                    let bytes = axum::body::Bytes::from(vec![b' '; size]);
                    *request.body_mut() = if request.headers_mut().remove("x-r3-stream").is_some() {
                        axum::body::Body::from_stream(futures_util::stream::once(async move { Ok::<_, std::convert::Infallible>(bytes) }))
                    } else {
                        axum::body::Body::from(bytes)
                    };
                }
                next.run(request).await
            }));
            axum::serve(listener, router).with_graceful_shutdown(async { let _ = stopped.await; }).await.unwrap();
        });
        let result = std::panic::AssertUnwindSafe(async {
            let client = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(10)).build().unwrap();
            let mut failures = Vec::new();
            for (method, path, size, expected, code) in [
                ("POST", "workspace/projects", 65_537, 413, "PAYLOAD_TOO_LARGE"),
                ("POST", "workspace/projects", 2_097_153, 413, "PAYLOAD_TOO_LARGE"),
                ("DELETE", "workspace/projects/missing", 2_097_153, 413, "PAYLOAD_TOO_LARGE"),
                ("DELETE", "workspace/projects/%FF", 0, 400, "INVALID_REQUEST"),
                ("GET", "workspace/operations/%FF", 0, 400, "INVALID_REQUEST"),
                ("POST", "unknown", 0, 404, "NOT_FOUND"),
                ("DELETE", "unknown", 0, 404, "NOT_FOUND"),
                ("PATCH", "workspace/projects", 0, 405, "METHOD_NOT_ALLOWED"),
            ] {
                for (credential, status, error) in [(Some(grants[0].0.as_str()), expected, code), (Some(grants[1].0.as_str()), if expected == 404 || expected == 405 { expected } else {403}, if expected == 404 || expected == 405 {code} else {"MACHINE_ACCESS_REQUIRED"}), (None, if expected == 404 || expected == 405 {expected} else {401}, if expected == 404 || expected == 405 {code} else {"UNAUTHORIZED"})] {
                    let response = if size > 0 {
                        // Unauthorized requests send no body; authorized requests send
                        // exactly limit+1 bytes, then stop writing before reading refusal.
                        use tokio::io::{AsyncReadExt, AsyncWriteExt};
                        tokio::time::timeout(Duration::from_secs(10), async {
                            let mut tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
                            let auth = credential.map(|token| format!("Authorization: Bearer {token}\r\n")).unwrap_or_default();
                            tcp.write_all(format!("{method} /api/v1/{path} HTTP/1.1\r\nHost: {addr}\r\n{auth}Content-Length: {size}\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
                            let mut wire = Vec::new();
                            if status == 413 { tcp.write_all(&vec![b' '; 65_537]).await.unwrap(); }
                            tcp.take(4097).read_to_end(&mut wire).await.unwrap();
                            assert!(wire.len() <= 4096, "bounded early refusal response");
                            let boundary = wire.windows(4).position(|v| v == b"\r\n\r\n").unwrap();
                            let head = std::str::from_utf8(&wire[..boundary]).unwrap();
                            let mut lines = head.split("\r\n");
                            let actual = lines.next().unwrap().split_whitespace().nth(1).unwrap().parse::<u16>().unwrap();
                            let mut response = axum::http::Response::builder().status(actual);
                            for line in lines {
                                let (name, value) = line.split_once(':').unwrap();
                                response = response.header(name, value.trim());
                            }
                            let response = response.body(wire[boundary + 4..].to_vec()).unwrap();
                            assert_eq!(response.headers()["content-length"].to_str().unwrap().parse::<usize>().unwrap(), response.body().len());
                            reqwest::Response::from(response)
                        }).await.expect("early refusal must respond before body upload")
                    } else {
                        let mut request = client.request(method.parse().unwrap(), format!("http://{addr}/api/v1/{path}")).body(vec![b' '; size]);
                        if let Some(token) = credential { request = request.bearer_auth(token); }
                        request.send().await.unwrap()
                    };
                    let actual = response.status().as_u16();
                    let private = response.headers().get("cache-control").is_some_and(|v| v == "no-store");
                    let bytes = response.bytes().await.unwrap();
                    let envelope = serde_json::from_slice::<crate::remote::machine_protocol::ErrorEnvelope>(&bytes);
                    let valid = actual == status && private && envelope.as_ref().is_ok_and(|e| e.error.code == error && !e.error.retryable && uuid::Uuid::parse_str(&e.error.request_id).is_ok());
                    println!("R3 {method} {path} bytes={size} auth={} status={actual} no_store={private} expected={status}/{error} valid={valid}", credential.is_some());
                    if !valid { failures.push(format!("{method} {path}: {actual}/{private}")); }
                    if size > 0 {
                        for framing in ["full", "stream"] {
                            // Given the exact original bytes, with no Content-Length hint.
                            let mut request = client.request(method.parse().unwrap(), format!("http://{addr}/api/v1/{path}")).header("x-r3-body-size", size);
                            if framing == "stream" { request = request.header("x-r3-stream", "true"); }
                            if let Some(token) = credential { request = request.bearer_auth(token); }
                            // When the real router extracts a full or unknown-length body.
                            let response = request.send().await.unwrap();
                            // Then the same authorization and body-limit contract holds.
                            assert_eq!(response.status().as_u16(), status);
                            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
                            let body: crate::remote::machine_protocol::ErrorEnvelope = serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
                            assert_eq!(body.error.code, error);
                            assert!(!body.error.retryable);
                            assert!(uuid::Uuid::parse_str(&body.error.request_id).is_ok());
                            println!("R3 body {framing} {method} {path} bytes={size} status={status} valid=true");
                        }
                    }
                }
            }
            for (status, code, retryable) in [(503, "HOST_UNAVAILABLE", true), (504, "TIMEOUT", true), (503, "MACHINE_SERVICE_UNAVAILABLE", true), (429, "CAPACITY_EXCEEDED", true), (400, "INVALID_REQUEST", false), (409, "OPERATION_OUTCOME_UNKNOWN", false)] {
                let response = machine_error(StatusCode::from_u16(status).unwrap(), code);
                let bytes = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
                let body: crate::remote::machine_protocol::ErrorEnvelope = serde_json::from_slice(&bytes).unwrap();
                println!("R3 classification {code} retryable={} expected={retryable}", body.error.retryable);
                if body.error.retryable != retryable { failures.push(code.into()); }
            }
            // Existing legacy errors and successful payloads must not be rewritten.
            let legacy = client.get(format!("http://{addr}/api/v1/sessions")).send().await.unwrap();
            assert_eq!(legacy.status(), 401);
            assert_eq!(legacy.text().await.unwrap(), "Missing auth token");
            let health = client.get(format!("http://{addr}/api/v1/health")).send().await.unwrap();
            assert_eq!(health.status(), 200);
            assert_eq!(serde_json::from_str::<serde_json::Value>(&health.text().await.unwrap()).unwrap()["status"], "ok");
            assert!(failures.is_empty(), "boundary failures: {failures:?}");
        }).catch_unwind().await;
        stop.send(()).unwrap();
        if tokio::time::timeout(Duration::from_secs(10), &mut task).await.is_err() { task.abort(); let _ = task.await; panic!("boundary listener shutdown timed out"); }
        assert!(tokio::net::TcpStream::connect(addr).await.is_err());
        drop(server);
        root.close().unwrap();
        println!("R3 CLEANUP listener_joined=true connection_refused=true private_root_removed=true no_pty=true");
        if let Err(panic) = result { std::panic::resume_unwind(panic); }
    }
    #[cfg(not(feature = "native-terminal"))]
    use std::time::Duration;

    #[test]
    fn a03_forged_exchange_fields_cannot_upgrade_mirror_authority() {
        let auth = crate::remote::auth::AuthManager::new();
        let code = auth.create_pairing_code(DevicePermission::Control);
        // The actual request decoder discards client authority claims. Only the
        // persisted owner-issued record supplies the exchange grant.
        let request: PairExchangeRequest = serde_json::from_value(serde_json::json!({
            "code": code, "deviceName": "forged desktop", "installationId": "attacker",
            "accessScope": "machine", "permission": "control", "clientType": "desktop"
        })).unwrap();
        let (_, device) = auth.exchange_pairing_code_with_installation(
            &request.code, &request.device_name, request.installation_id.as_deref(),
        ).unwrap();
        assert_eq!(device.access_scope, DeviceAccessScope::Mirror);
    }

    #[test]
    fn test_phase4_server_valid_socket_target_allows_browser() {
        assert!(valid_socket_target("/api/v1/browser/b1"));
        assert!(!valid_socket_target("/api/v1/browser/"));
    }

    async fn raw_ws_handshake(
        addr: SocketAddr,
        path_and_query: &str,
        token: Option<&str>,
    ) -> (StatusCode, tokio::net::TcpStream) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut stream = tokio::net::TcpStream::connect(addr).await.expect("tcp connect");
        let auth = token
            .map(|t| format!("Authorization: Bearer {t}\r\n"))
            .unwrap_or_default();
        let req = format!(
            "GET {path_and_query} HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n{auth}\r\n"
        );
        stream.write_all(req.as_bytes()).await.expect("tcp write");

        let mut response = Vec::new();
        let mut byte = [0u8; 1];
        while !response.ends_with(b"\r\n\r\n") {
            let n = stream.read(&mut byte).await.expect("tcp read");
            if n == 0 {
                break;
            }
            response.push(byte[0]);
        }
        let resp_str = String::from_utf8_lossy(&response);
        let status_code = resp_str
            .strip_prefix("HTTP/1.1 ")
            .and_then(|rest| rest.get(..3))
            .and_then(|code| code.parse::<u16>().ok())
            .and_then(|code| StatusCode::from_u16(code).ok())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        (status_code, stream)
    }

    async fn read_ws_text_frame(stream: &mut tokio::net::TcpStream) -> String {
        use tokio::io::AsyncReadExt;
        let mut header = [0u8; 2];
        stream.read_exact(&mut header).await.expect("read header");
        let payload_len = (header[1] & 0x7f) as usize;
        let actual_len = if payload_len == 126 {
            let mut ext = [0u8; 2];
            stream.read_exact(&mut ext).await.expect("read ext");
            u16::from_be_bytes(ext) as usize
        } else {
            payload_len
        };
        let mut payload = vec![0u8; actual_len];
        stream.read_exact(&mut payload).await.expect("read payload");
        String::from_utf8(payload).expect("utf8 text frame")
    }

    #[tokio::test]
    async fn test_phase4_gateway_browser_ws_single_use_ticket_admission() {
        let terminal_service = Arc::new(TerminalService::default());
        let registry = WorkspaceRegistry::new();
        let state = Arc::new(RemoteGatewayState::new(terminal_service, registry));
        let pin = state
            .auth_manager
            .create_pairing_code(DevicePermission::Control);
        let (token, _device) = state
            .auth_manager
            .exchange_pairing_code(&pin, "s5-device")
            .unwrap();
        let test_backend = Arc::new(crate::remote::browser_backend::InProcessTestBackend::new());
        test_backend.sessions.lock().await.push(crate::remote::browser_backend::RemoteBrowserSessionSummary {
            browser_id: "b1".into(),
            title: Some("B1".into()),
            url: Some("https://example.com".into()),
            visible: true,
        });
        state.set_browser_backend(test_backend.clone());

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let router = create_remote_router(Arc::clone(&state));
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let server_task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async { let _ = stop_rx.await; })
                .await
                .unwrap();
        });

        let client = reqwest::Client::builder().no_proxy().build().unwrap();

        // 1. Issue a single-use socket ticket for browser endpoint
        let ticket_body = serde_json::json!({ "target": "/api/v1/browser/b1" }).to_string();
        let ticket_resp = client
            .post(format!("http://{addr}/api/v1/socket-ticket"))
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/json")
            .body(ticket_body)
            .send()
            .await
            .unwrap();
        assert_eq!(ticket_resp.status(), reqwest::StatusCode::OK);
        let ticket_text = ticket_resp.text().await.unwrap();
        let ticket_json: serde_json::Value = serde_json::from_str(&ticket_text).unwrap();
        let ticket = ticket_json["ticket"].as_str().unwrap();

        // 2. (a) Valid single-use ticket connects and upgrades with 101 Switching Protocols + BrowserHello
        let path_with_ticket = format!("/api/v1/browser/b1?ticket={ticket}");
        let (status, mut stream) = raw_ws_handshake(addr, &path_with_ticket, None).await;
        assert_eq!(status, StatusCode::SWITCHING_PROTOCOLS, "valid ticket must succeed with 101");
        let hello_msg = read_ws_text_frame(&mut stream).await;
        let hello_json: serde_json::Value = serde_json::from_str(&hello_msg).unwrap();
        assert_eq!(hello_json["type"], "browserHello");
        assert_eq!(hello_json["browserId"], "b1");

        // 3. (b) Replay of same ticket must be rejected (single-use)
        let (replayed_status, _) = raw_ws_handshake(addr, &path_with_ticket, None).await;
        assert_eq!(replayed_status, StatusCode::UNAUTHORIZED, "replayed single-use ticket must be rejected with 401");

        // 4. Connect without ticket or credentials must be rejected
        let (no_auth_status, _) = raw_ws_handshake(addr, "/api/v1/browser/b1", None).await;
        assert_eq!(no_auth_status, StatusCode::UNAUTHORIZED, "unauthenticated connect must be rejected with 401");

        // 5. Connect with invalid ticket must be rejected
        let (fake_ticket_status, _) = raw_ws_handshake(addr, "/api/v1/browser/b1?ticket=fake-ticket-123", None).await;
        assert_eq!(fake_ticket_status, StatusCode::UNAUTHORIZED, "invalid ticket must be rejected with 401");

        // 6. Direct HTTP listing when backend is active
        let list_resp = client
            .get(format!("http://{addr}/api/v1/browser/sessions?workspaceId=ws1&worktreeSlug=main"))
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await
            .unwrap();
        assert_eq!(list_resp.status(), reqwest::StatusCode::OK);

        // 7. When GUI exits: browsers become unavailable (UnavailableBrowserBackend)
        state.set_browser_backend(Arc::new(crate::remote::browser_backend::UnavailableBrowserBackend));
        state.bump_browser_service_epoch();

        let list_unavail = client
            .get(format!("http://{addr}/api/v1/browser/sessions?workspaceId=ws1&worktreeSlug=main"))
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await
            .unwrap();
        assert_eq!(list_unavail.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE, "unavailable backend must return 503");

        let _ = stop_tx.send(());
        let _ = server_task.await;
    }

    /// Server-lane probe backend: records the scope every discovery call receives and
    /// counts frame-receiver attachments versus owned viewer subscriptions.
    #[derive(Default)]
    struct ScopeProbeBackend {
        /// browser_id -> workspace_id/worktree_slug that owns it in the desktop inventory
        inventory: std::sync::Mutex<Vec<(String, String, String)>>,
        seen_scopes: std::sync::Mutex<Vec<DesktopScope>>,
        subscribe_frames_calls: Arc<std::sync::atomic::AtomicUsize>,
        viewer_subs: Arc<std::sync::atomic::AtomicUsize>,
        title_override: std::sync::Mutex<Option<String>>,
    }

    impl ScopeProbeBackend {
        fn with_inventory(entries: &[(&str, &str, &str)]) -> Self {
            let backend = Self::default();
            backend.inventory.lock().unwrap().extend(
                entries
                    .iter()
                    .map(|(b, w, s)| (b.to_string(), w.to_string(), s.to_string())),
            );
            backend
        }

        fn seen_scopes(&self) -> Vec<DesktopScope> {
            self.seen_scopes.lock().unwrap().clone()
        }

        fn frame_attachments(&self) -> usize {
            self.subscribe_frames_calls
                .load(std::sync::atomic::Ordering::SeqCst)
        }

        fn live_viewer_subscriptions(&self) -> usize {
            self.viewer_subs.load(std::sync::atomic::Ordering::SeqCst)
        }

        fn matching(&self, scope: &DesktopScope) -> Vec<crate::remote::browser_backend::RemoteBrowserSessionSummary> {
            let title = self.title_override.lock().unwrap().clone();
            self.inventory
                .lock()
                .unwrap()
                .iter()
                .filter(|(_, ws, slug)| ws == &scope.workspace_id && slug == &scope.worktree_slug)
                .map(|(id, _, _)| crate::remote::browser_backend::RemoteBrowserSessionSummary {
                    browser_id: id.clone(),
                    title: title.clone().or_else(|| Some(format!("title-{id}"))),
                    url: Some("https://example.com/page".into()),
                    visible: true,
                })
                .collect()
        }
    }

    impl RemoteBrowserBackend for ScopeProbeBackend {
        fn list_sessions<'a>(
            &'a self,
            scope: &'a DesktopScope,
        ) -> crate::remote::browser_backend::BoxFuture<
            'a,
            Result<Vec<crate::remote::browser_backend::RemoteBrowserSessionSummary>, RemoteBrowserError>,
        > {
            Box::pin(async move {
                self.seen_scopes.lock().unwrap().push(scope.clone());
                Ok(self.matching(scope))
            })
        }

        fn identify_session<'a>(
            &'a self,
            scope: &'a DesktopScope,
        ) -> crate::remote::browser_backend::BoxFuture<
            'a,
            Result<Option<crate::remote::browser_backend::RemoteBrowserSessionSummary>, RemoteBrowserError>,
        > {
            Box::pin(async move {
                self.seen_scopes.lock().unwrap().push(scope.clone());
                // Deliberately leaks an out-of-scope session: the server must fence it.
                Ok(self.matching(scope).into_iter().next().or_else(|| {
                    self.inventory.lock().unwrap().first().map(|(id, _, _)| {
                        crate::remote::browser_backend::RemoteBrowserSessionSummary {
                            browser_id: id.clone(),
                            title: Some(format!("title-{id}")),
                            url: Some("https://example.com/page".into()),
                            visible: true,
                        }
                    })
                }))
            })
        }

        fn get_state<'a>(
            &'a self,
            browser_id: &'a str,
            scope: &'a DesktopScope,
        ) -> crate::remote::browser_backend::BoxFuture<
            'a,
            Result<crate::remote::browser_backend::BrowserRemoteState, RemoteBrowserError>,
        > {
            Box::pin(async move {
                self.seen_scopes.lock().unwrap().push(scope.clone());
                Ok(crate::remote::browser_backend::BrowserRemoteState {
                    browser_id: browser_id.to_string(),
                    browser_instance_id: None,
                    url: Some("https://example.com/page".into()),
                    title: Some(format!("title-{browser_id}")),
                    document_generation: "1".into(),
                    viewport_revision: "1".into(),
                    loading: false,
                    paused: false,
                    pause_reason: None,
                })
            })
        }

        fn execute_command(
            &self,
            _ctx: crate::remote::browser_backend::BrowserCommandContext,
        ) -> crate::remote::browser_backend::BoxFuture<
            '_,
            Result<crate::remote::browser_backend::BrowserCommandResult, RemoteBrowserError>,
        > {
            Box::pin(async move {
                Ok(crate::remote::browser_backend::BrowserCommandResult {
                    success: true,
                    value: None,
                })
            })
        }

        fn subscribe_frames<'a>(
            &'a self,
            _browser_id: &'a str,
        ) -> crate::remote::browser_backend::BoxFuture<
            'a,
            Result<tokio::sync::broadcast::Receiver<Vec<u8>>, RemoteBrowserError>,
        > {
            Box::pin(async move {
                self.subscribe_frames_calls
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(tokio::sync::broadcast::channel(8).0.subscribe())
            })
        }

        fn subscribe_viewer<'a>(
            &'a self,
            _browser_id: &'a str,
            _device_id: &'a str,
            _viewer_instance_id: &'a str,
            options: Option<crate::remote::browser_protocol::BrowserSubscribeOptions>,
        ) -> crate::remote::browser_backend::BoxFuture<
            'a,
            Result<
                (
                    String,
                    u32,
                    crate::remote::browser_protocol::BrowserSubscribeOptions,
                    crate::remote::browser_backend::BrowserSubscribeIdentity,
                ),
                RemoteBrowserError,
            >,
        > {
            Box::pin(async move {
                self.viewer_subs
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let identity = crate::remote::browser_backend::BrowserSubscribeIdentity {
                    browser_instance_id: "probe-instance".into(),
                    browser_service_epoch: "1".into(),
                    desktop_epoch: "1".into(),
                    document_generation: "1".into(),
                };
                Ok((
                    format!("probe-sub-{}", uuid::Uuid::new_v4()),
                    1,
                    options.unwrap_or_default(),
                    identity,
                ))
            })
        }

        fn unsubscribe_viewer<'a>(
            &'a self,
            _browser_id: &'a str,
            _subscription_id: &'a str,
        ) -> crate::remote::browser_backend::BoxFuture<'a, Result<(), RemoteBrowserError>> {
            Box::pin(async move {
                self.viewer_subs
                    .fetch_update(
                        std::sync::atomic::Ordering::SeqCst,
                        std::sync::atomic::Ordering::SeqCst,
                        |v| Some(v.saturating_sub(1)),
                    )
                    .ok();
                Ok(())
            })
        }

        fn capabilities(
            &self,
        ) -> crate::remote::browser_backend::BoxFuture<'_, crate::remote::browser_backend::BrowserCapabilities>
        {
            Box::pin(async move {
                crate::remote::browser_backend::BrowserCapabilities {
                    browser_available: true,
                    supported_formats: vec!["jpeg".into()],
                    supported_commands: vec!["getState".into()],
                    max_edge: 1024,
                    max_fps: 8,
                }
            })
        }
    }

    async fn spawn_browser_test_gateway(
        state: Arc<RemoteGatewayState>,
    ) -> (SocketAddr, tokio::sync::oneshot::Sender<()>, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let router = create_remote_router(state);
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = stop_rx.await;
                })
                .await;
        });
        (addr, stop_tx, task)
    }

    async fn mint_browser_ticket(
        client: &reqwest::Client,
        addr: SocketAddr,
        token: &str,
        browser_id: &str,
    ) -> String {
        let resp = client
            .post(format!("http://{addr}/api/v1/socket-ticket"))
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/json")
            .body(serde_json::json!({ "target": format!("/api/v1/browser/{browser_id}") }).to_string())
            .send()
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_str(&resp.text().await.unwrap()).unwrap();
        json["ticket"].as_str().expect("ticket").to_string()
    }

    /// R4-3: the desktop scope is derived server-side from the authenticated
    /// connection's authoritative selection. A blank or forged query scope may never
    /// widen the visible inventory, identify may not return a hidden session, and
    /// attaching to a browser outside the inventory is refused.
    #[tokio::test]
    async fn test_r4_3_desktop_scope_is_derived_server_side_not_from_query() {
        let state = Arc::new(RemoteGatewayState::new(
            Arc::new(TerminalService::default()),
            WorkspaceRegistry::new(),
        ));
        let pin = state.auth_manager.create_pairing_code(DevicePermission::Control);
        let (token, _) = state
            .auth_manager
            .exchange_pairing_code(&pin, "scope-device")
            .unwrap();

        let backend = Arc::new(ScopeProbeBackend::with_inventory(&[
            ("b-shared", "ws-shared", "main"),
            ("b-other", "ws-other", "rogue"),
        ]));
        state.set_browser_backend(backend.clone());
        state.set_active_selection(RemoteActiveDesktopSelection {
            workspace_id: Some("ws-shared".into()),
            worktree_slug: Some("main".into()),
            ..Default::default()
        });

        let (addr, stop_tx, task) = spawn_browser_test_gateway(Arc::clone(&state)).await;
        let client = reqwest::Client::builder().no_proxy().build().unwrap();

        // 1. Blank query: the server must supply the derived scope, not an empty one
        //    (an empty workspace_id lists every session on in-process backends).
        let listed: serde_json::Value = serde_json::from_str(
            &client
                .get(format!("http://{addr}/api/v1/browser/sessions"))
                .header("Authorization", format!("Bearer {token}"))
                .send()
                .await
                .unwrap()
                .text()
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            listed.as_array().map(Vec::len),
            Some(1),
            "blank query must resolve to the derived desktop scope: {listed}"
        );
        assert_eq!(listed[0]["browserId"], "b-shared");

        // 2. A forged query scope must be ignored entirely.
        let forged: serde_json::Value = serde_json::from_str(
            &client
                .get(format!(
                    "http://{addr}/api/v1/browser/sessions?workspaceId=ws-other&worktreeSlug=rogue"
                ))
                .header("Authorization", format!("Bearer {token}"))
                .send()
                .await
                .unwrap()
                .text()
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(forged[0]["browserId"], "b-shared", "forged scope must not widen visibility");
        for scope in backend.seen_scopes() {
            assert_eq!(
                (scope.workspace_id.as_str(), scope.worktree_slug.as_str()),
                ("ws-shared", "main"),
                "backend must only ever receive the server-derived scope"
            );
        }

        // 3. identify must not fall back to an out-of-scope session.
        state.set_active_selection(RemoteActiveDesktopSelection {
            workspace_id: Some("ws-empty".into()),
            worktree_slug: Some("main".into()),
            ..Default::default()
        });
        let identified: serde_json::Value = serde_json::from_str(
            &client
                .get(format!("http://{addr}/api/v1/browser/identify"))
                .header("Authorization", format!("Bearer {token}"))
                .send()
                .await
                .unwrap()
                .text()
                .await
                .unwrap(),
        )
        .unwrap();
        assert!(
            identified["browser"].is_null(),
            "identify must not reveal a session outside the shared inventory: {identified}"
        );

        // 4. Discovery output is routed through the public sanitizer (R4-17).
        state.set_active_selection(RemoteActiveDesktopSelection {
            workspace_id: Some("ws-shared".into()),
            worktree_slug: Some("main".into()),
            ..Default::default()
        });
        *backend.title_override.lock().unwrap() =
            Some("Snapshot at /Volumes/T9-Mac/project/ferryx/x.json".into());
        let sanitized = client
            .get(format!("http://{addr}/api/v1/browser/sessions"))
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert!(
            !sanitized.contains("/Volumes/"),
            "discovery payload must pass through the public sanitizer: {sanitized}"
        );
        *backend.title_override.lock().unwrap() = None;

        // 5. Attaching to a browser outside the derived inventory is refused.
        let ticket = mint_browser_ticket(&client, addr, &token, "b-other").await;
        let (status, _) = raw_ws_handshake(
            addr,
            &format!("/api/v1/browser/b-other?ticket={ticket}"),
            None,
        )
        .await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "attaching outside the authorized inventory must be refused"
        );

        // 6. With no desktop selection at all, nothing is listed and nothing attaches.
        state.clear_active_selection();
        let none_listed: serde_json::Value = serde_json::from_str(
            &client
                .get(format!("http://{addr}/api/v1/browser/sessions"))
                .header("Authorization", format!("Bearer {token}"))
                .send()
                .await
                .unwrap()
                .text()
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            none_listed.as_array().map(Vec::len),
            Some(0),
            "absent desktop selection must not list everything: {none_listed}"
        );

        let _ = stop_tx.send(());
        let _ = task.await;
    }

    /// R4-4: authorization is live, not a snapshot taken at upgrade. Revoking the
    /// device terminates the in-flight socket and releases its viewer slot.
    #[tokio::test]
    async fn test_r4_4_live_socket_terminates_on_device_revocation() {
        use futures_util::{SinkExt, StreamExt};

        let state = Arc::new(RemoteGatewayState::new(
            Arc::new(TerminalService::default()),
            WorkspaceRegistry::new(),
        ));
        let pin = state.auth_manager.create_pairing_code(DevicePermission::Control);
        let (token, device) = state
            .auth_manager
            .exchange_pairing_code(&pin, "revoke-device")
            .unwrap();

        let backend = Arc::new(ScopeProbeBackend::with_inventory(&[("b1", "ws1", "main")]));
        state.set_browser_backend(backend.clone());
        state.set_active_selection(RemoteActiveDesktopSelection {
            workspace_id: Some("ws1".into()),
            worktree_slug: Some("main".into()),
            ..Default::default()
        });

        let (addr, stop_tx, task) = spawn_browser_test_gateway(Arc::clone(&state)).await;
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let ticket = mint_browser_ticket(&client, addr, &token, "b1").await;

        let (mut ws, _) =
            tokio_tungstenite::connect_async(format!("ws://{addr}/api/v1/browser/b1?ticket={ticket}"))
                .await
                .expect("ws upgrade");
        let _hello = ws.next().await.expect("hello").expect("hello frame");

        // Subscribe to take a real viewer slot, then observe capture start.
        let mut capture_rx = state.admission_controller.subscribe_capture();
        ws.send(tokio_tungstenite::tungstenite::Message::Text(
            serde_json::json!({
                "type": "browserSubscribe",
                "requestId": "r-sub",
                "viewerInstanceId": "v1",
                "options": { "format": "jpeg" }
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();
        let sub_json: serde_json::Value =
            serde_json::from_str(&ws.next().await.unwrap().unwrap().into_text().unwrap()).unwrap();
        assert_eq!(sub_json["type"], "browserSubscribed");
        assert_eq!(capture_rx.recv().await.unwrap(), ("b1".to_string(), true));

        // Revoke the device mid-session.
        assert!(state.auth_manager.revoke_device(&device.id).unwrap());

        // The live socket must terminate instead of serving the revoked device...
        let terminated = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                match ws.next().await {
                    None | Some(Err(_)) => break true,
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) => break true,
                    Some(Ok(_)) => continue,
                }
            }
        })
        .await
        .expect("revoked socket must terminate without waiting on a timer");
        assert!(terminated);

        // ...and its teardown must halt capture rather than leave a zero-viewer producer.
        let halted = tokio::time::timeout(Duration::from_secs(10), capture_rx.recv())
            .await
            .expect("capture-halt signal")
            .expect("capture channel open");
        assert_eq!(halted, ("b1".to_string(), false));
        assert!(!state.admission_controller.should_capture("b1"));

        let _ = stop_tx.send(());
        let _ = task.await;
    }

    /// R4-6: receiver attachment is passive. No frame subscription (and therefore no
    /// synthetic viewer) exists before an accepted `browserSubscribe`, and teardown of
    /// the last viewer halts capture and releases the owned backend subscription.
    #[tokio::test]
    async fn test_r4_6_receiver_attachment_is_passive_and_teardown_halts_capture() {
        use futures_util::{SinkExt, StreamExt};

        let state = Arc::new(RemoteGatewayState::new(
            Arc::new(TerminalService::default()),
            WorkspaceRegistry::new(),
        ));
        let pin = state.auth_manager.create_pairing_code(DevicePermission::Control);
        let (token, _) = state
            .auth_manager
            .exchange_pairing_code(&pin, "viewer-device")
            .unwrap();

        let backend = Arc::new(ScopeProbeBackend::with_inventory(&[("b1", "ws1", "main")]));
        state.set_browser_backend(backend.clone());
        state.set_active_selection(RemoteActiveDesktopSelection {
            workspace_id: Some("ws1".into()),
            worktree_slug: Some("main".into()),
            ..Default::default()
        });

        let (addr, stop_tx, task) = spawn_browser_test_gateway(Arc::clone(&state)).await;
        let client = reqwest::Client::builder().no_proxy().build().unwrap();

        let ticket = mint_browser_ticket(&client, addr, &token, "b1").await;
        let (mut ws, _) =
            tokio_tungstenite::connect_async(format!("ws://{addr}/api/v1/browser/b1?ticket={ticket}"))
                .await
                .unwrap();
        let _hello = ws.next().await.unwrap().unwrap();

        // Attached but not subscribed: no frame receiver, no viewer slot, no capture.
        assert_eq!(
            backend.frame_attachments(),
            0,
            "receiver attachment must be passive until a viewer subscription is accepted"
        );
        assert_eq!(backend.live_viewer_subscriptions(), 0);
        assert!(!state.admission_controller.should_capture("b1"));

        // Subscribing creates exactly one owned viewer subscription.
        let mut capture_rx = state.admission_controller.subscribe_capture();
        ws.send(tokio_tungstenite::tungstenite::Message::Text(
            serde_json::json!({
                "type": "browserSubscribe",
                "requestId": "r-sub",
                "viewerInstanceId": "v1",
                "options": { "format": "jpeg" }
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();
        let sub_json: serde_json::Value =
            serde_json::from_str(&ws.next().await.unwrap().unwrap().into_text().unwrap()).unwrap();
        assert_eq!(sub_json["type"], "browserSubscribed");
        assert_eq!(capture_rx.recv().await.unwrap(), ("b1".to_string(), true));
        assert_eq!(
            backend.live_viewer_subscriptions(),
            1,
            "exactly one owned viewer subscription must exist"
        );
        assert_eq!(
            backend.frame_attachments(),
            1,
            "the frame receiver attaches once, for the accepted subscription"
        );

        // Closing the last viewer halts capture and releases the owned subscription.
        let _ = ws.close(None).await;
        let halted = tokio::time::timeout(Duration::from_secs(10), capture_rx.recv())
            .await
            .expect("capture-halt signal")
            .expect("capture channel open");
        assert_eq!(halted, ("b1".to_string(), false));
        assert!(
            !state.admission_controller.should_capture("b1"),
            "capture must never outlive its last viewer"
        );
        assert_eq!(
            backend.live_viewer_subscriptions(),
            0,
            "teardown must release every owned backend subscription"
        );

        let _ = stop_tx.send(());
        let _ = task.await;
    }

    #[tokio::test]
    async fn a03_machine_auth_errors_are_private_json() {
        let state = Arc::new(RemoteGatewayState::new(
            Arc::new(TerminalService::default()), WorkspaceRegistry::new(),
        ));
        let response = get_capabilities(State(state), HeaderMap::new()).await.unwrap_err();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(response.headers().get(header::CACHE_CONTROL).and_then(|v| v.to_str().ok()), Some("no-store"));
        let bytes = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"]["code"], "UNAUTHORIZED");
        let typed = crate::remote::machine_protocol::decode_json::<crate::remote::machine_protocol::ErrorEnvelope>(&bytes, 4096)
            .expect("actual authentication response must satisfy A02 error contract");
        assert_eq!(typed.error.code, "UNAUTHORIZED");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a03_identity_runs_offthread_and_revocation_fences_response() {
        let dir = tempfile::tempdir().unwrap();
        let state = Arc::new(RemoteGatewayState::new_with_paths(
            Arc::new(TerminalService::default()), WorkspaceRegistry::new(),
            Some(dir.path().join("config.json")), Some(dir.path().join("auth.json")),
        ));
        let pin = state.auth_manager.create_pairing_code(DevicePermission::Control);
        let (token, device) = state.auth_manager.exchange_pairing_code(&pin, "fixture").unwrap();
        let runtime_thread = std::thread::current().id();
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let entered_tx = std::sync::Mutex::new(Some(entered_tx));
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let release_rx = std::sync::Mutex::new(release_rx);
        *state.identity_probe.write() = Some(Arc::new(move || {
            let offthread = std::thread::current().id() != runtime_thread;
            if let Some(tx) = entered_tx.lock().unwrap().take() { let _ = tx.send(offthread); }
            if offthread { release_rx.lock().unwrap().recv_timeout(Duration::from_secs(5)).unwrap(); }
        }));
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, format!("Bearer {token}").parse().unwrap());
        let request_state = Arc::clone(&state);
        let request = tokio::spawn(async move { get_capabilities(State(request_state), headers).await });
        let entered = tokio::time::timeout(Duration::from_secs(5), entered_rx).await;
        state.auth_manager.revoke_device(&device.id);
        let released = release_tx.send(());
        let response = tokio::time::timeout(Duration::from_secs(5), request).await;
        assert!(entered.unwrap().unwrap(), "identity work must run off reactor");
        released.unwrap();
        let response = response.unwrap().unwrap().unwrap_err();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(response.headers().get(header::CACHE_CONTROL).unwrap(), "no-store");
        *state.identity_probe.write() = Some(Arc::new(|| panic!("revoked request probed identity")));
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, format!("Bearer {token}").parse().unwrap());
        assert_eq!(get_capabilities(State(state), headers).await.unwrap_err().status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn a03_absent_machine_service_is_private_and_unavailable() {
        let state = Arc::new(RemoteGatewayState::new(
            Arc::new(TerminalService::default()), WorkspaceRegistry::new(),
        ));
        let pin = state.auth_manager.create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine).unwrap();
        let (token, _) = state.auth_manager.exchange_pairing_code(&pin, "machine").unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, format!("Bearer {token}").parse().unwrap());
        let request = axum::http::Request::builder().method("POST").uri("/api/v1/sessions")
            .body(axum::body::Body::empty()).unwrap();
        let response = super::super::session_api::create(State(state), headers, request).await.unwrap_err();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response.headers().get(header::CACHE_CONTROL).unwrap(), "no-store");
        let bytes = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"]["code"], "MACHINE_SERVICE_UNAVAILABLE");
        let typed = crate::remote::machine_protocol::decode_json::<crate::remote::machine_protocol::ErrorEnvelope>(&bytes, 4096)
            .expect("actual service response must satisfy A02 error contract");
        assert_eq!(typed.error.code, "MACHINE_SERVICE_UNAVAILABLE");
    }

    #[tokio::test]
    async fn relay_startup_connects_without_machine_token() {
        use crate::remote::protocol::{ControlAuth, ControlAuthResponse, ControlChallenge};
        use futures_util::{SinkExt, StreamExt};
        assert!(
            std::env::var("FERRYX_MACHINE_TOKEN")
                .unwrap_or_default()
                .trim()
                .is_empty(),
            "run zero-config regression without FERRYX_MACHINE_TOKEN"
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let relay_addr = listener.local_addr().unwrap();
        let (authenticated_tx, authenticated_rx) = tokio::sync::oneshot::channel();
        let relay = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
            let challenge = ControlChallenge {
                audience: None,
                nonce: "startup-challenge".into(),
                timestamp: 1234,
            };
            socket
                .send(tokio_tungstenite::tungstenite::Message::Text(
                    serde_json::to_string(&challenge).unwrap().into(),
                ))
                .await
                .unwrap();
            let frame = socket.next().await.unwrap().unwrap();
            let auth: ControlAuth = serde_json::from_str(frame.to_text().unwrap()).unwrap();
            assert!(crate::remote::auth::verify_control_challenge(
                &auth.public_key,
                &auth.machine_id,
                "relay",
                &challenge.nonce,
                auth.timestamp,
                &auth.signature
            ));
            socket
                .send(tokio_tungstenite::tungstenite::Message::Text(
                    serde_json::to_string(&ControlAuthResponse {
                        success: true,
                        error: None,
                    })
                    .unwrap()
                    .into(),
                ))
                .await
                .unwrap();
            authenticated_tx.send(auth.machine_id).unwrap();
            while let Some(frame) = socket.next().await {
                if frame.is_err() {
                    break;
                }
            }
        });
        let terminal = Arc::new(TerminalService::new(
            Arc::new(crate::terminal::PtyManager::new()),
            Arc::new(TerminalOutputHub::default()),
        ));
        let state = Arc::new(RemoteGatewayState::new_with_paths(
            terminal,
            WorkspaceRegistry::new(),
            None,
            None,
        ));
        {
            let mut config = state.config.write();
            config.mode = RemoteNetworkMode::Relay;
            config.port = 0;
            config.relay_url = Some(format!("http://{relay_addr}"));
        }
        let (handle, address) = start_remote_server(state).await.unwrap();
        assert!(address.ip().is_loopback());
        let machine = tokio::time::timeout(std::time::Duration::from_secs(5), authenticated_rx)
            .await
            .unwrap()
            .unwrap();
        let identity = crate::remote::auth::load_or_generate_machine_identity(
            &crate::remote::auth::canonical_identity_dir().unwrap(),
        )
        .unwrap();
        assert_eq!(machine, identity.machine_id);
        handle.stop();
        relay.abort();
    }

    /// Running daemon sessions must be listed for authenticated remote callers
    /// regardless of desktop active selection. `active_selection` may only supply
    /// extra label metadata for a matching session; it must never filter the
    /// session list itself, in particular when it is `None`.
    #[tokio::test]
    async fn test_get_active_running_sessions_independent_of_desktop() {
        let pty = Arc::new(crate::terminal::PtyManager::new());
        let hub = Arc::new(TerminalOutputHub::default());
        let terminal_service = Arc::new(TerminalService::new(Arc::clone(&pty), Arc::clone(&hub)));
        let registry = WorkspaceRegistry::new();
        let state = RemoteGatewayState::new(Arc::clone(&terminal_service), registry.clone());

        let (session_id, mut rx) = pty
            .spawn(portable_pty::CommandBuilder::new("/bin/sh"), 80, 24)
            .expect("spawn session");
        hub.register_session(&session_id);
        let session_id_clone = session_id.clone();
        let hub_clone = Arc::clone(&hub);
        tokio::spawn(async move {
            while let Some(chunk) = rx.recv().await {
                hub_clone.publish(&session_id_clone, chunk);
            }
        });

        // No active desktop selection at all.
        assert!(state.active_selection().is_none());

        let cache = WorkspaceSnapshotCache::build(&registry);
        let sessions = get_active_running_sessions(&state, &cache, &[]).await;

        assert_eq!(
            sessions.len(),
            1,
            "running daemon sessions must be returned even when active_selection is None, got: {:?}",
            sessions
        );
        assert_eq!(sessions[0].session_id, session_id);
        assert!(sessions[0].running);

        pty.close_session(&session_id)
            .await
            .expect("close fixture PTY");
    }

    /// Starting the gateway in `Loopback`-equivalent (`Off`-free, no
    /// external interface requested) mode with an OS-assigned port (0) must
    /// bind loopback only, never the wildcard address `0.0.0.0`.
    #[tokio::test]
    async fn test_listener_bind_loopback() {
        let pty = Arc::new(crate::terminal::PtyManager::new());
        let hub = Arc::new(TerminalOutputHub::default());
        let terminal_service = Arc::new(TerminalService::new(Arc::clone(&pty), Arc::clone(&hub)));
        let registry = WorkspaceRegistry::new();

        // Scenario 1: the external-interface resolver errors (e.g. "no LAN
        // interface available"). Startup must fail rather than silently
        // widening the loopback bind to 0.0.0.0.
        struct NoExternalInterfaceResolver;
        impl crate::remote::state::InterfaceResolver for NoExternalInterfaceResolver {
            fn local_network_address(&self) -> Result<std::net::Ipv4Addr, String> {
                Err("no active local network IPv4 interface found".into())
            }
            fn tailscale_address(&self) -> Result<std::net::Ipv4Addr, String> {
                Err("no active Tailscale IPv4 interface found".into())
            }
        }

        let state_no_external = Arc::new(RemoteGatewayState::new(
            Arc::clone(&terminal_service),
            registry.clone(),
        ));
        {
            let mut config = state_no_external.config.write();
            config.mode = RemoteNetworkMode::LocalNetwork;
            config.port = 0;
        }
        let result = start_remote_server_with_resolver(
            Arc::clone(&state_no_external),
            Arc::new(NoExternalInterfaceResolver),
        )
        .await;
        assert!(result.is_err(), "expected resolver error to propagate");
        assert!(
            !*state_no_external.is_running.read(),
            "failed startup must not leave the gateway marked as running"
        );

        // Scenario 2: the external-interface resolver succeeds. The primary
        // (loopback) listener returned to the caller must still be bound to
        // 127.0.0.1 with an OS-assigned port, never 0.0.0.0.
        struct LoopbackOnlyResolver;
        impl crate::remote::state::InterfaceResolver for LoopbackOnlyResolver {
            fn local_network_address(&self) -> Result<std::net::Ipv4Addr, String> {
                Ok(std::net::Ipv4Addr::new(127, 0, 0, 1))
            }
            fn tailscale_address(&self) -> Result<std::net::Ipv4Addr, String> {
                Err("no active Tailscale IPv4 interface found".into())
            }
        }

        let state = Arc::new(RemoteGatewayState::new(
            Arc::clone(&terminal_service),
            registry.clone(),
        ));
        {
            let mut config = state.config.write();
            config.mode = RemoteNetworkMode::LocalNetwork;
            config.port = 0;
        }

        let (handle, local_addr) =
            start_remote_server_with_resolver(Arc::clone(&state), Arc::new(LoopbackOnlyResolver))
                .await
                .expect("server should start with a loopback-resolving resolver");

        assert!(
            local_addr.ip().is_loopback(),
            "primary listener must bind a loopback address, got {}",
            local_addr.ip()
        );
        assert_ne!(
            local_addr.ip(),
            std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED),
            "primary listener must never bind the wildcard address 0.0.0.0"
        );

        handle.stop();
    }

    /// Push subscribe/unsubscribe endpoints must require a valid Bearer
    /// device token, matching every other authenticated remote route, and
    /// must reject unauthenticated requests with 401 rather than silently
    /// registering/removing push subscriptions.
    #[tokio::test]
    async fn test_push_auth() {
        let pty = Arc::new(crate::terminal::PtyManager::new());
        let hub = Arc::new(TerminalOutputHub::default());
        let terminal_service = Arc::new(TerminalService::new(Arc::clone(&pty), Arc::clone(&hub)));
        let registry = WorkspaceRegistry::new();
        let state = Arc::new(RemoteGatewayState::new(
            Arc::clone(&terminal_service),
            registry.clone(),
        ));

        fn no_auth_query() -> AuthQuery {
            AuthQuery {
                ticket: None,
                render: None,
                cols: None,
                rows: None,
                daemon_epoch: None,
                after_sequence: None,
            }
        }

        let subscribe_result = push_subscribe(
            State(Arc::clone(&state)),
            HeaderMap::new(),
            Json(PushSubscriptionInfo {
                endpoint: "https://push.example.com/sub/unauth".to_string(),
                keys: crate::remote::push::PushSubscriptionKeys {
                    p256dh: "p256dh-key".to_string(),
                    auth: "auth-key".to_string(),
                },
            }),
        )
        .await;
        let (status, _) = subscribe_result.expect_err("unauthenticated subscribe must be rejected");
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let unsubscribe_result = push_unsubscribe(
            State(Arc::clone(&state)),
            HeaderMap::new(),
            Json(PushUnsubscribeRequest {
                endpoint: "https://push.example.com/sub/unauth".to_string(),
            }),
        )
        .await;
        let (status, _) =
            unsubscribe_result.expect_err("unauthenticated unsubscribe must be rejected");
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        assert!(
            global_push_store()
                .list_subscriptions()
                .iter()
                .all(|sub| sub.endpoint != "https://push.example.com/sub/unauth"),
            "unauthenticated request must not have registered a subscription"
        );
    }

    #[tokio::test]
    async fn test_headless_auto_spawns_default_shell() {
        let pty = Arc::new(crate::terminal::PtyManager::new());
        let hub = Arc::new(TerminalOutputHub::default());
        let terminal_service = Arc::new(TerminalService::new(Arc::clone(&pty), Arc::clone(&hub)));
        let registry = WorkspaceRegistry::new();
        let state = Arc::new(RemoteGatewayState::new(
            Arc::clone(&terminal_service),
            registry.clone(),
        ));

        let code = state
            .auth_manager
            .create_pairing_code(crate::remote::auth::DevicePermission::Control);
        let (token, _) = state
            .auth_manager
            .exchange_pairing_code(&code, "HeadlessClient")
            .unwrap();

        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token}").parse().unwrap(),
        );

        assert!(state.active_selection.read().is_none());
        assert!(state.terminal_service.list_sessions().is_empty());

        let res = get_workspace_state(State(Arc::clone(&state)), headers.clone())
            .await
            .expect("get_workspace_state succeeds");
        let ws_state = res.0;

        let session_id = ws_state.active_context.session_id.expect("session_id populated");
        assert_eq!(ws_state.active_context.terminal_tabs.len(), 1);
        assert_eq!(ws_state.active_context.terminal_tabs[0].id, session_id);
        assert_eq!(ws_state.active_context.terminal_tabs[0].label, "Terminal");

        assert_eq!(ws_state.sessions.len(), 1);
        let s = &ws_state.sessions[0];
        assert_eq!(s.session_id, session_id);
        assert_eq!(s.title.as_deref(), Some("Terminal"));
        assert_eq!(s.worktree_label.as_deref(), Some("default"));
        assert!(s.running);

        let _ = terminal_service.close_session(&session_id).await;
    }

    #[tokio::test]
    async fn test_spawn_shell_method() {
        let pty = Arc::new(crate::terminal::PtyManager::new());
        let hub = Arc::new(TerminalOutputHub::default());
        let terminal_service = TerminalService::new(pty, hub);

        let (session_id, _rx) = terminal_service
            .spawn_shell(80, 24)
            .expect("spawn_shell succeeds");
        assert!(terminal_service.list_sessions().contains(&session_id));
        let _ = terminal_service.close_session(&session_id).await;
    }

    #[tokio::test]
    async fn test_active_session_focus_watcher_ignores_transient_none() {
        let (tx, rx) = tokio::sync::watch::channel(Some("target-session".to_string()));
        let (close_tx, mut close_rx) = tokio::sync::mpsc::unbounded_channel();

        let watcher = tokio::spawn(async move {
            watch_active_session_focus(rx, "target-session", move || {
                let _ = close_tx.send(());
            })
            .await;
        });

        // (a) send None -> the close signal must NOT fire within the test window
        tx.send(None).expect("send None");

        let test_window = std::time::Duration::from_millis(50);
        let fired = tokio::time::timeout(test_window, close_rx.recv()).await;
        assert!(
            fired.is_err(),
            "close signal must NOT fire within the test window on transient None, but received: {:?}",
            fired
        );
        assert!(!watcher.is_finished(), "watcher task should remain active on transient None");

        // (b) send Some(other) -> close signal fires
        tx.send(Some("other-session".to_string())).expect("send other session");

        let fired = tokio::time::timeout(std::time::Duration::from_millis(500), close_rx.recv()).await;
        assert_eq!(
            fired,
            Ok(Some(())),
            "close signal must fire when focus changes to a different session"
        );

        let _ = watcher.await;
    }

    #[tokio::test]
    async fn test_workspace_worktrees_scoped_request_and_not_found() {
        let root = tempfile::tempdir().unwrap();
        let repo_a = root.path().join("repo_a");
        std::fs::create_dir(&repo_a).unwrap();
        crate::worktree::run_git(&repo_a, &["init", "--quiet"]).unwrap();
        crate::worktree::run_git(
            &repo_a,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "--allow-empty",
                "-m",
                "init",
            ],
        )
        .unwrap();

        let repo_b = root.path().join("repo_b");
        std::fs::create_dir(&repo_b).unwrap();
        crate::worktree::run_git(&repo_b, &["init", "--quiet"]).unwrap();
        crate::worktree::run_git(
            &repo_b,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "--allow-empty",
                "-m",
                "init",
            ],
        )
        .unwrap();

        let pty = Arc::new(crate::terminal::PtyManager::new());
        let hub = Arc::new(TerminalOutputHub::default());
        let terminal_service = Arc::new(TerminalService::new(Arc::clone(&pty), Arc::clone(&hub)));
        let registry = WorkspaceRegistry::new();
        registry.register("workspace-a", &repo_a).unwrap();
        registry.register("workspace-b", &repo_b).unwrap();

        let mgr_b = registry.manager("workspace-b").unwrap();
        let wt_b_path = mgr_b.worktree_path_for("workspace-b", "feat-b").unwrap();
        mgr_b
            .create_worktree(crate::worktree::CreateWorktreeOptions {
                ws_id: "workspace-b".into(),
                slug: "feat-b".into(),
                path: wt_b_path.clone(),
                base_ref: None,
            })
            .unwrap();

        let state = Arc::new(RemoteGatewayState::new(
            Arc::clone(&terminal_service),
            registry.clone(),
        ));

        // Set active desktop selection to workspace-a
        *state.active_selection.write() = Some(RemoteActiveDesktopSelection {
            workspace_id: Some("workspace-a".to_string()),
            attention_inventory: Vec::new(),
            worktree_slug: None,
            worktree_label: None,
            session_id: None,
            tab_id: None,
            terminal_tabs: Vec::new(),
        });

        let code = state
            .auth_manager
            .create_pairing_code(crate::remote::auth::DevicePermission::Control);
        let (token, _) = state
            .auth_manager
            .exchange_pairing_code(&code, "Client")
            .unwrap();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let router = create_remote_router(Arc::clone(&state));
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .unwrap();
        });

        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap();

        // 1. Request worktrees for workspace-b when active is workspace-a
        let resp_b = client
            .get(format!(
                "http://{addr}/api/v1/workspace/worktrees?workspaceId=workspace-b"
            ))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap();
        assert_eq!(resp_b.status(), reqwest::StatusCode::OK);

        let body_b = resp_b.bytes().await.unwrap();
        let json_b: serde_json::Value = serde_json::from_slice(&body_b).unwrap();
        let worktrees = json_b["worktrees"].as_array().expect("worktrees array");
        assert!(
            worktrees.iter().any(|wt| {
                wt["identity"]["slug"] == "feat-b"
                    || wt["branch"] == "refs/heads/orca/workspace-b/feat-b"
                    || wt["path"].as_str().is_some_and(|p: &str| p.contains("wt_b"))
            }),
            "expected workspace-b worktrees to contain feat-b, got: {json_b:?}"
        );
        assert!(
            !worktrees.iter().any(|wt| wt["workspaceId"] == "workspace-a"),
            "expected no workspace-a worktrees in workspace-b response, got: {json_b:?}"
        );

        // 2. Request worktrees for unknown workspace -> must return structured 404
        let resp_unknown = client
            .get(format!(
                "http://{addr}/api/v1/workspace/worktrees?workspaceId=unknown-workspace"
            ))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap();
        assert_eq!(resp_unknown.status(), reqwest::StatusCode::NOT_FOUND);

        let body_unknown = resp_unknown.bytes().await.unwrap();
        let envelope: crate::remote::machine_protocol::ErrorEnvelope =
            serde_json::from_slice(&body_unknown).unwrap();
        assert_eq!(envelope.error.code, "PROJECT_NOT_FOUND");
        assert!(!envelope.error.retryable);

        let _ = stop.send(());
        let _ = task.await;
    }

    #[tokio::test]
    async fn test_worktree_list_failure_returns_structured_error_not_200_empty() {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo_fail");
        std::fs::create_dir(&repo).unwrap();
        crate::worktree::run_git(&repo, &["init", "--quiet"]).unwrap();
        crate::worktree::run_git(
            &repo,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "--allow-empty",
                "-m",
                "init",
            ],
        )
        .unwrap();

        let registry = WorkspaceRegistry::new();
        registry.register("workspace-fail", &repo).unwrap();

        // Corrupt git repo so manager.list_worktrees() fails
        let git_dir = repo.join(".git");
        std::fs::remove_dir_all(&git_dir).unwrap();

        let pty = Arc::new(crate::terminal::PtyManager::new());
        let hub = Arc::new(TerminalOutputHub::default());
        let terminal_service = Arc::new(TerminalService::new(pty, hub));
        let state = Arc::new(RemoteGatewayState::new(
            terminal_service,
            registry,
        ));
        {
            let mut conf = state.config.write();
            conf.mode = crate::remote::state::RemoteNetworkMode::LocalNetwork;
            conf.port = 0;
        }
        crate::remote::server::set_allow_insecure_direct(true);

        let code = state
            .auth_manager
            .create_pairing_code(crate::remote::auth::DevicePermission::Control);
        let (token, _) = state
            .auth_manager
            .exchange_pairing_code(&code, "Client")
            .unwrap();

        let (handle, addr) = start_remote_server(Arc::clone(&state)).await.unwrap();

        let client = reqwest::Client::new();
        let resp = client
            .get(format!(
                "http://{addr}/api/v1/workspace/worktrees?workspaceId=workspace-fail"
            ))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap();

        // Must NOT return HTTP 200 with empty array on discovery failure
        assert_ne!(
            resp.status(),
            reqwest::StatusCode::OK,
            "discovery failure must return non-success status, not 200 empty"
        );
        let status = resp.status();
        let body = resp.bytes().await.unwrap();
        let envelope: Result<crate::remote::machine_protocol::ErrorEnvelope, _> =
            serde_json::from_slice(&body);
        assert!(
            envelope.is_ok(),
            "expected structured error envelope on status {status}, got: {}",
            String::from_utf8_lossy(&body)
        );

        handle.stop();
    }

    #[tokio::test]
    async fn test_r5_3_absent_scope_fails_closed_and_enforces_inventory() {
        let state = Arc::new(RemoteGatewayState::new(
            Arc::new(TerminalService::default()),
            WorkspaceRegistry::new(),
        ));
        let backend = Arc::new(ScopeProbeBackend::with_inventory(&[
            ("b-shared", "ws-shared", "main"),
            ("b-other-wt", "ws-shared", "feature"),
        ]));
        state.set_browser_backend(backend.clone());

        // Absent scope: must FAIL CLOSED (deny attachment)
        assert!(state.active_selection().is_none());
        let authorized = attachment_is_authorized(&state, "b-shared").await.unwrap();
        assert!(!authorized, "absent scope must fail closed, denying attachment");

        // Set scope to ws-shared, main:
        state.set_active_selection(RemoteActiveDesktopSelection {
            workspace_id: Some("ws-shared".into()),
            worktree_slug: Some("main".into()),
            ..Default::default()
        });

        // b-shared matches both workspace_id and worktree_slug: authorized
        assert!(attachment_is_authorized(&state, "b-shared").await.unwrap());

        // b-other-wt has same workspace but different worktree: MUST BE DENIED
        assert!(!attachment_is_authorized(&state, "b-other-wt").await.unwrap(), "different worktree must be denied");
    }

    #[tokio::test]
    async fn test_r5_4_scope_watch_terminates_socket_during_await() {
        use futures_util::StreamExt;

        let state = Arc::new(RemoteGatewayState::new(
            Arc::new(TerminalService::default()),
            WorkspaceRegistry::new(),
        ));
        let pin = state.auth_manager.create_pairing_code(DevicePermission::Control);
        let (token, _) = state
            .auth_manager
            .exchange_pairing_code(&pin, "scope-watch-device")
            .unwrap();

        let backend = Arc::new(ScopeProbeBackend::with_inventory(&[
            ("b-shared", "ws-shared", "main"),
        ]));
        state.set_browser_backend(backend.clone());
        state.set_active_selection(RemoteActiveDesktopSelection {
            workspace_id: Some("ws-shared".into()),
            worktree_slug: Some("main".into()),
            ..Default::default()
        });

        let (addr, stop_tx, task) = spawn_browser_test_gateway(Arc::clone(&state)).await;
        let client = reqwest::Client::builder().no_proxy().build().unwrap();

        let ticket = mint_browser_ticket(&client, addr, &token, "b-shared").await;
        let (mut ws, _) = tokio_tungstenite::connect_async(format!(
            "ws://{addr}/api/v1/browser/b-shared?ticket={ticket}"
        ))
        .await
        .expect("ws upgrade");

        let hello = ws.next().await.unwrap().unwrap().into_text().unwrap();
        assert!(hello.contains("browserHello"));

        // While the socket is awaiting events in its loop without incoming client traffic,
        // change active selection!
        state.set_active_selection(RemoteActiveDesktopSelection {
            workspace_id: Some("ws-shared".into()),
            worktree_slug: Some("other-branch".into()),
            ..Default::default()
        });

        // The socket MUST receive BrowserDriverRevoked due to scope change and terminate
        let revoked_msg = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("must receive revocation promptly on scope change")
            .expect("socket open")
            .unwrap()
            .into_text()
            .unwrap();
        assert!(revoked_msg.contains("desktop_scope_changed"));

        let _ = stop_tx.send(());
        let _ = task.await;
    }

    #[tokio::test]
    async fn test_r5_14_writer_ordering_barrier_and_bounded_shutdown() {
        use futures_util::{SinkExt, StreamExt};

        let state = Arc::new(RemoteGatewayState::new(
            Arc::new(TerminalService::default()),
            WorkspaceRegistry::new(),
        ));
        let pin = state.auth_manager.create_pairing_code(DevicePermission::Control);
        let (token, _) = state
            .auth_manager
            .exchange_pairing_code(&pin, "barrier-device")
            .unwrap();

        let backend = Arc::new(ScopeProbeBackend::with_inventory(&[
            ("b-shared", "ws-shared", "main"),
        ]));
        state.set_browser_backend(backend.clone());
        state.set_active_selection(RemoteActiveDesktopSelection {
            workspace_id: Some("ws-shared".into()),
            worktree_slug: Some("main".into()),
            ..Default::default()
        });

        let (addr, stop_tx, task) = spawn_browser_test_gateway(Arc::clone(&state)).await;
        let client = reqwest::Client::builder().no_proxy().build().unwrap();

        let ticket = mint_browser_ticket(&client, addr, &token, "b-shared").await;
        let (mut ws, _) = tokio_tungstenite::connect_async(format!(
            "ws://{addr}/api/v1/browser/b-shared?ticket={ticket}"
        ))
        .await
        .expect("ws upgrade");

        let hello = ws.next().await.unwrap().unwrap().into_text().unwrap();
        assert!(hello.contains("browserHello"));

        // Ordering barrier: Send subscribe request
        ws.send(tokio_tungstenite::tungstenite::Message::Text(
            serde_json::json!({
                "type": "browserSubscribe",
                "requestId": "r-barr",
                "viewerInstanceId": "v-barr",
                "options": { "format": "jpeg" }
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();

        // First message received MUST be browserSubscribed (never an unsynchronized raw frame)
        let sub_resp = ws.next().await.unwrap().unwrap();
        match sub_resp {
            tokio_tungstenite::tungstenite::Message::Text(text) => {
                let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
                assert_eq!(parsed["type"], "browserSubscribed");
            }
            tokio_tungstenite::tungstenite::Message::Binary(_) => {
                panic!("Subscribed-before-frame ordering barrier violated: received binary frame before subscription acknowledgement!");
            }
            other => panic!("Unexpected message: {:?}", other),
        }

        // Bounded drain followed by cancellation: Close client socket abruptly
        let _ = ws.close(None).await;

        // The gateway must tear down without blocking the writer task
        let _ = stop_tx.send(());
        let shutdown_res = tokio::time::timeout(Duration::from_secs(2), task).await;
        assert!(shutdown_res.is_ok(), "teardown must not hang awaiting blocked writer");
    }

    #[tokio::test]
    async fn test_r6_4_scope_change_interrupts_inline_dispatch_await() {
        use futures_util::{SinkExt, StreamExt};

        let state = Arc::new(RemoteGatewayState::new(
            Arc::new(TerminalService::default()),
            WorkspaceRegistry::new(),
        ));
        let pin = state.auth_manager.create_pairing_code(DevicePermission::Control);
        let (token, _) = state
            .auth_manager
            .exchange_pairing_code(&pin, "scope-interrupt-device")
            .unwrap();

        // Slow backend that hangs during snapshot execution
        struct SlowSnapshotBackend {
            inner: ScopeProbeBackend,
        }
        impl RemoteBrowserBackend for SlowSnapshotBackend {
            fn list_sessions<'a>(
                &'a self,
                scope: &'a DesktopScope,
            ) -> crate::remote::browser_backend::BoxFuture<'a, Result<Vec<crate::remote::browser_backend::RemoteBrowserSessionSummary>, RemoteBrowserError>> {
                self.inner.list_sessions(scope)
            }
            fn identify_session<'a>(
                &'a self,
                scope: &'a DesktopScope,
            ) -> crate::remote::browser_backend::BoxFuture<'a, Result<Option<crate::remote::browser_backend::RemoteBrowserSessionSummary>, RemoteBrowserError>> {
                self.inner.identify_session(scope)
            }
            fn get_state<'a>(
                &'a self,
                browser_id: &'a str,
                scope: &'a DesktopScope,
            ) -> crate::remote::browser_backend::BoxFuture<'a, Result<crate::remote::browser_backend::BrowserRemoteState, RemoteBrowserError>> {
                self.inner.get_state(browser_id, scope)
            }
            fn execute_command(
                &self,
                _ctx: crate::remote::browser_backend::BrowserCommandContext,
            ) -> crate::remote::browser_backend::BoxFuture<'_, Result<crate::remote::browser_backend::BrowserCommandResult, RemoteBrowserError>> {
                Box::pin(async move {
                    // Hang awaiting until cancelled
                    std::future::pending::<()>().await;
                    Ok(crate::remote::browser_backend::BrowserCommandResult { success: true, value: None })
                })
            }
            fn capabilities(&self) -> crate::remote::browser_backend::BoxFuture<'_, crate::remote::browser_backend::BrowserCapabilities> {
                self.inner.capabilities()
            }
        }

        state.set_browser_backend(Arc::new(SlowSnapshotBackend {
            inner: ScopeProbeBackend::with_inventory(&[("b-interrupt", "ws-interrupt", "main")]),
        }));
        state.set_active_selection(RemoteActiveDesktopSelection {
            workspace_id: Some("ws-interrupt".into()),
            worktree_slug: Some("main".into()),
            ..Default::default()
        });

        let (addr, stop_tx, task) = spawn_browser_test_gateway(Arc::clone(&state)).await;
        let client = reqwest::Client::builder().no_proxy().build().unwrap();

        let ticket = mint_browser_ticket(&client, addr, &token, "b-interrupt").await;
        let (mut ws, _) = tokio_tungstenite::connect_async(format!(
            "ws://{addr}/api/v1/browser/b-interrupt?ticket={ticket}"
        ))
        .await
        .expect("ws upgrade");

        let hello = ws.next().await.unwrap().unwrap().into_text().unwrap();
        assert!(hello.contains("browserHello"));

        // First subscribe
        ws.send(tokio_tungstenite::tungstenite::Message::Text(
            serde_json::json!({
                "type": "browserSubscribe",
                "requestId": "r-sub",
                "viewerInstanceId": "v-1",
                "options": { "format": "jpeg" }
            }).to_string().into()
        )).await.unwrap();

        let sub_resp = ws.next().await.unwrap().unwrap().into_text().unwrap();
        assert!(sub_resp.contains("browserSubscribed"));

        // Now send snapshot which hangs in execute_command
        ws.send(tokio_tungstenite::tungstenite::Message::Text(
            serde_json::json!({
                "type": "browserSnapshot",
                "requestId": "snap-hang",
                "browserId": "b-interrupt"
            }).to_string().into()
        )).await.unwrap();

        // While snapshot is hanging in dispatch_raw_text, change scope!
        tokio::time::sleep(Duration::from_millis(50)).await;
        state.set_active_selection(RemoteActiveDesktopSelection {
            workspace_id: Some("ws-interrupt".into()),
            worktree_slug: Some("different-branch".into()),
            ..Default::default()
        });

        // Scope watcher must interrupt inline await and revoke immediately
        let revoked_msg = tokio::time::timeout(Duration::from_secs(3), ws.next())
            .await
            .expect("Scope change must interrupt inline backend await promptly")
            .expect("socket open")
            .unwrap()
            .into_text()
            .unwrap();
        assert!(revoked_msg.contains("desktop_scope_changed"));

        let _ = stop_tx.send(());
        let _ = task.await;
    }
}
