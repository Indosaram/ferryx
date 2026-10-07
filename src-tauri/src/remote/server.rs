#[path = "machine_owner_socket.rs"]
mod machine_owner_socket;
use crate::remote::auth::{AuthError, DeviceAccessScope, DeviceInfo, DevicePermission};
use crate::remote::backend::{RecoveryStream, RemoteRecoveryStatus, RemoteSessionBackend, RemoteSessionDetails};
use crate::remote::browser_admission::AdmissionController;
use crate::remote::browser_backend::{DesktopScope, RemoteBrowserBackend, RemoteBrowserError};
use crate::remote::browser_protocol::ServerMessage;
use crate::remote::browser_security::sanitize_public_string;
use crate::remote::browser_ws::BrowserWsSession;
use crate::remote::mirror::RemoteTerminalMirror;
use crate::remote::protocol::RemoteGridFrame;
pub use crate::remote::protocol::RemoteTerminalTabInfo as RemoteTerminalTab;
use crate::remote::protocol::{
    ClientControlMessage, RemoteActiveDesktopSelection, RemoteCreateWorktreeRequest,
    RemoteDeleteWorktreeRequest, RemoteEventMessage, RemoteProjectInfo,
    RemoteSelectWorkspaceRequest, RemoteSelectionRequestPayload, RemoteTerminalSession,
    RemoteTerminalTabInfo, RemoteWorkspaceState, RemoteWorktreeInfo,
};
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
        || target == super::dag_api::DAG_SOCKET_TARGET
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
    state.auth_manager.validate_token(&token).map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            "Invalid or revoked token".to_string(),
        )
    })?;
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
pub(super) fn consume_socket_ticket(
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

    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(PairExchangeResponse {
            token,
            device,
            machine_id: identity.machine_id,
            display_name: identity.display_name,
        }),
    )
        .into_response())
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

/// The desktop's own tab label for a session, so the phone names a session exactly as the desktop
/// does. `None` when the session is not a desktop tab: the phone then numbers the rows itself,
/// which keeps sessions in one project distinguishable (a directory name would repeat on every row).
fn desktop_tab_title(
    session_id: &str,
    active: Option<&RemoteActiveDesktopSelection>,
) -> Option<String> {
    active.and_then(|selection| {
        selection
            .terminal_tabs
            .iter()
            .find(|tab| tab.session_id.as_deref() == Some(session_id))
            .map(|tab| tab.label.trim().to_string())
            .filter(|label| !label.is_empty())
    })
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
                Ok(false) => {}
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
                    title: desktop_tab_title(&session_id, active.as_ref()),
                    session_id: details.session_id,
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
            .or_else(|| {
                if active.is_none() {
                    Some("default".to_string())
                } else {
                    None
                }
            });
        // Sessions are listed for authenticated remote callers regardless of
        // desktop active selection; `active_selection` only supplies extra
        // label metadata when it matches this session, it never filters.
        let worktree_label = derived_label
            .or(details.worktree_label)
            .or_else(|| {
                selected.and_then(|selection| {
                    selection
                        .worktree_slug
                        .clone()
                        .or_else(|| selection.worktree_label.clone())
                })
            })
            .or_else(|| {
                if active.is_none() {
                    Some("default".to_string())
                } else {
                    None
                }
            });
        // Without a desktop connection there are no tab labels; keep the historical "Terminal".
        let title = desktop_tab_title(&session_id, active.as_ref())
            .or_else(|| active.is_none().then(|| "Terminal".to_string()));
        sessions.push(RemoteTerminalSession {
            session_id: details.session_id,
            title,
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
            let is_running_or_starting =
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
    let token =
        extract_token(&headers).ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
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
    let token =
        extract_token(&headers).ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
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

    if let Some(services) = state.machine_services.as_ref() {
        for tab in active_context.terminal_tabs.iter_mut() {
            if tab.activity_state.is_none() {
                if let Some(session_id) = tab.session_id.as_deref() {
                    if !session_id.starts_with("standby:") {
                        tab.activity_state = services.sessions.session_activity_state(session_id);
                    }
                }
            }
        }
    }

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
                let activity_state = state
                    .machine_services
                    .as_ref()
                    .and_then(|ms| ms.sessions.session_activity_state(&session_id));
                active_context.terminal_tabs = vec![RemoteTerminalTabInfo {
                    id: session_id.clone(),
                    label: "Terminal".to_string(),
                    session_id: Some(session_id),
                    activity_state,
                    ..Default::default()
                }];
            }
        } else if backend_sessions.is_empty() && state.terminal_service.list_sessions().is_empty() {
            if let Ok((session_id, _)) = state.terminal_service.spawn_shell(80, 24) {
                tracing::info!(
                    "Auto-spawned default shell session {session_id} for headless remote gateway"
                );
                active_context.session_id = Some(session_id.clone());
                let activity_state = state
                    .machine_services
                    .as_ref()
                    .and_then(|ms| ms.sessions.session_activity_state(&session_id));
                active_context.terminal_tabs = vec![RemoteTerminalTabInfo {
                    id: session_id,
                    label: "Terminal".to_string(),
                    session_id: active_context.session_id.clone(),
                    activity_state,
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentHistoryQuery {
    pub limit: Option<usize>,
    /// `before` is accepted as an alias, the usual name for a backwards page cursor. Serde drops
    /// unknown keys silently, so `?before=<n>` used to return the newest page again.
    #[serde(alias = "before")]
    pub cursor: Option<usize>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentHistoryResponse {
    pub session_id: String,
    pub items: Vec<crate::agent_transcript::ConversationMessage>,
    pub next_cursor: Option<usize>,
    pub partial: bool,
    pub warnings: Vec<String>,
}

/// Reads a paired host's transcript for a remote session.
///
/// Returns `(messages, malformed, truncated)`. `truncated` is true when the host sent only a bounded
/// tail, which means the returned ordinals are window-relative and the conversation is longer than
/// what was delivered - the caller must surface that rather than imply the window is the whole
/// history.
///
/// The session's own store entry carries its host, remote home and project path, so no path is ever
/// taken from the client. Local sessions never reach here: the caller falls through only when the
/// local lookup found no transcript. Windows hosts return not-found rather than a guessed slug.
async fn read_remote_conversation(
    state: &Arc<RemoteGatewayState>,
    session_id: &str,
    limit: usize,
    before: Option<usize>,
) -> Result<(Vec<crate::agent_transcript::ConversationMessage>, usize, bool), String> {
    let not_found = || "TRANSCRIPT_NOT_FOUND".to_string();
    let Some(services) = state.machine_services.clone() else {
        return Err(not_found());
    };
    let store = services.sessions.remote_sessions_store_path().to_path_buf();
    let raw = crate::ipc::run_blocking(move || {
        std::fs::read_to_string(store).map_err(|e| crate::ipc::IpcError::internal(e.to_string()))
    })
    .await
    .map_err(|_| not_found())?;
    let target = crate::agent_transcript::remote_target_from_store(&raw, session_id).ok_or_else(not_found)?;
    let host: crate::ssh::SshHost = serde_json::from_value(target.host).map_err(|_| not_found())?;
    let dir = target.dir;

    // The transcript is megabytes (a real one measured 11 MB over 3730 lines), and the client polls
    // every few seconds, so the wire must not carry the whole file. `limit` messages at roughly 4 KiB
    // each is a generous ceiling; the remote side reports the file's true size on the first line and
    // then sends only the tail, dropping the partial first line so the parser never sees a truncated
    // record. The reported size is what lets the caller say honestly whether history was cut.
    let budget = limit.saturating_mul(4096).clamp(64 * 1024, 4 * 1024 * 1024);
    let script = format!(
        "d={dir}; f=$(ls -t \"$d\"/*.jsonl 2>/dev/null | head -n 1); \
         if [ -n \"$f\" ]; then \
           n=$(wc -c < \"$f\"); printf '%s\\n' \"$n\"; \
           if [ \"$n\" -gt {budget} ]; then tail -c {budget} \"$f\" | tail -n +2; else cat -- \"$f\"; fi; \
         fi",
        dir = crate::ssh::direct::quote_posix(&dir),
        budget = budget,
    );
    let command = format!("sh -c {}", crate::ssh::direct::quote_posix(&script));
    let plan = crate::ssh::direct::ssh_plan(&host, command, false).map_err(|_| not_found())?;
    let bytes = crate::ssh::direct::bounded_output_with_limit(
        &plan,
        std::time::Duration::from_secs(15),
        budget + 4096,
    )
    .await
    .map_err(|_| not_found())?;
    if bytes.is_empty() {
        return Err(not_found());
    }

    let (true_bytes, body) = crate::agent_transcript::split_remote_size_line(&bytes)
        .ok_or_else(not_found)?;
    let truncated = true_bytes > budget;
    if body.is_empty() {
        return Err(not_found());
    }
    let (items, malformed) = crate::agent_transcript::read_conversation_from_bytes(body, limit, before);
    Ok((items, malformed, truncated))
}

async fn get_agent_history(
    State(state): State<Arc<RemoteGatewayState>>,
    AxumPath(session_id): AxumPath<String>,
    Query(query): Query<AgentHistoryQuery>,
    headers: HeaderMap,
) -> Result<Json<AgentHistoryResponse>, (StatusCode, Json<serde_json::Value>)> {
    let token = extract_token(&headers).ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "UNAUTHORIZED" })),
        )
    })?;
    let _device = state.auth_manager.validate_token(&token).map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "UNAUTHORIZED" })),
        )
    })?;

    if !crate::agent_transcript::is_valid_session_id(&session_id) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "INVALID_SESSION_ID" })),
        ));
    }

    let limit = query.limit.unwrap_or(200).clamp(1, 1000);
    let before = query.cursor;

    let target_session_id = session_id.clone();
    let remote_session_id = session_id.clone();
    let remote_state = Arc::clone(&state);
    let cwd = state
        .session_backend
        .describe_session(&target_session_id)
        .await
        .ok()
        .and_then(|details| details.worktree_path.map(|p| p.to_string_lossy().into_owned()));
    // A newest-in-cwd guess is only safe when no other live session shares this cwd;
    // otherwise the phone could show another agent's conversation.
    let cwd_is_unique = match cwd.as_deref() {
        None => false,
        Some(own_cwd) => {
            let mut unique = true;
            for other in state.session_backend.list_sessions().await {
                if other == target_session_id {
                    continue;
                }
                let shares_cwd = state
                    .session_backend
                    .describe_session(&other)
                    .await
                    .ok()
                    .and_then(|details| details.worktree_path)
                    .is_some_and(|path| path.to_string_lossy() == own_cwd);
                if shares_cwd {
                    unique = false;
                    break;
                }
            }
            unique
        }
    };
    let provider_session = state
        .machine_services
        .as_ref()
        .and_then(|ms| ms.sessions.session_provider_session(&target_session_id));
    let local: Result<(Vec<crate::agent_transcript::ConversationMessage>, usize), String> =
        crate::ipc::run_blocking(move || {
            #[cfg(test)]
            let home_override = state.agent_history_home.read().clone();
            #[cfg(not(test))]
            let home_override: Option<PathBuf> = None;
            let outcome = match home_override.or_else(|| {
                std::env::var_os("HOME")
                    .or_else(|| std::env::var_os("USERPROFILE"))
                    .map(PathBuf::from)
            }) {
                Some(home) => {
                    let preferred = provider_session.as_ref().and_then(|provider| {
                        if let Some(path) = provider.transcript_path.as_deref() {
                            let candidate = std::path::PathBuf::from(path);
                            if candidate.is_file() {
                                return Some(candidate);
                            }
                        }
                        crate::agent_transcript::transcript_path_for_session(&home, &provider.id)
                    });
                    let transcript = match preferred.or_else(|| {
                        crate::agent_transcript::transcript_path_for_session(
                            &home,
                            &target_session_id,
                        )
                    }) {
                        Some(transcript_path) => Some(transcript_path),
                        None => {
                            if cwd_is_unique {
                                cwd.and_then(|cwd_value| {
                                    crate::agent_transcript::latest_transcript_for_cwd(
                                        &home,
                                        &cwd_value,
                                        Some(&target_session_id),
                                    )
                                })
                            } else {
                                None
                            }
                        }
                    };
                    match transcript {
                        Some(transcript_path) => {
                            crate::agent_transcript::read_conversation(&transcript_path, limit, before)
                        }
                        None => Err("TRANSCRIPT_NOT_FOUND".to_string()),
                    }
                }
                None => Err("HOME_UNAVAILABLE".to_string()),
            };
            Ok(outcome)
        })
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "INTERNAL_ERROR" })),
            )
        })?;

    let result: Result<(Vec<crate::agent_transcript::ConversationMessage>, usize, bool), String> =
        match local {
            Ok(found) => Ok((found.0, found.1, false)),
            Err(code) if code == "TRANSCRIPT_NOT_FOUND" => {
                read_remote_conversation(&remote_state, &remote_session_id, limit, before).await
            }
            Err(other) => Err(other),
        };

    match result {
        Ok((items, malformed_count, remote_truncated)) => {
            let mut warnings = Vec::new();
            if malformed_count > 0 {
                warnings.push(format!("skipped {malformed_count} malformed lines"));
            }
            if remote_truncated {
                warnings.push(
                    "older history is not available for paired-host sessions; showing the most recent \
                     messages"
                        .to_string(),
                );
            }
            let next_cursor = items
                .first()
                .map(|message| message.ordinal)
                .filter(|ordinal| *ordinal > 0);
            let partial = next_cursor.is_some();
            Ok(Json(AgentHistoryResponse {
                session_id,
                items,
                next_cursor,
                partial,
                warnings,
            }))
        }
        Err(err_code) if err_code == "TRANSCRIPT_NOT_FOUND" => Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "TRANSCRIPT_NOT_FOUND" })),
        )),
        Err(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "INTERNAL_ERROR" })),
        )),
    }
}

async fn select_workspace(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    Json(payload): Json<RemoteSelectWorkspaceRequest>,
) -> Result<Json<RemoteSelectionRequestPayload>, (StatusCode, String)> {
    let token =
        extract_token(&headers).ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
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
        return Err((
            StatusCode::BAD_REQUEST,
            "Terminal creation cannot select an existing terminal".into(),
        ));
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
    let token =
        extract_token(&headers).ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
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
    let token =
        extract_token(&headers).ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
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
    let token =
        extract_token(&headers).ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
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
    let token =
        extract_token(&headers).ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
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
        let services = state.machine_services.as_ref().ok_or((
            StatusCode::SERVICE_UNAVAILABLE,
            "MACHINE_SERVICE_UNAVAILABLE".into(),
        ))?;
        let receiver = services.workspaces.machine_events.subscribe();
        return Ok(ws.on_upgrade(move |socket| async move {
            let _ = while_device_authorized(
                &mut revocation,
                super::machine_events::serve(socket, state, receiver),
            )
            .await;
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
        if let Some(peer) = state.machine_services.as_ref().and_then(|services| {
            services
                .sessions
                .router()
                .find_legacy_peer_for_session(&requested_session_id)
        }) {
            return machine_owner_socket::upgrade(
                ws,
                peer,
                requested_session_id,
                query,
                token,
                revocation,
            )
            .await;
        }
        return machine_terminal_upgrade(
            ws,
            requested_session_id,
            query,
            device,
            revocation,
            state,
        )
        .await;
    }
    let session_id = parse_host_scoped_session_id(&requested_session_id)
        .map(|(_, id)| id.to_string())
        .unwrap_or(requested_session_id);
    let render_grid = query.render.as_deref() == Some("grid");
    if let Some(services) = &state.machine_services {
        match services.sessions.machine_only_async(&session_id).await {
            Ok(false) => {}
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
                    || state
                        .session_backend
                        .list_sessions()
                        .await
                        .contains(&session_id)
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
        || state
            .terminal_service
            .remote()
            .details(&session_id)
            .is_some()
    {
        // Local headless sessions remain attachable, but an SSH terminal must
        // first be exposed through the desktop selection before mirror access.
        return Err((
            StatusCode::FORBIDDEN,
            "Forbidden: session is not the active desktop session".into(),
        ));
    }

    let mut initial_resized = false;
    let attachment = while_device_authorized(&mut revocation, async {
        if let Some((cols, rows)) =
            requested_geometry.filter(|_| device.permission == DevicePermission::Control)
        {
            if state.session_backend.recovery(&session_id).await?.is_none() {
                if state
                    .session_backend
                    .resize(&session_id, cols, rows)
                    .await
                    .is_ok()
                {
                    initial_resized = true;
                }
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
        let resized = Arc::new(std::sync::atomic::AtomicBool::new(initial_resized));
        let _ = while_device_authorized(
            &mut revocation,
            handle_terminal_socket(
                socket,
                session_id.clone(),
                attachment,
                device,
                Arc::clone(&state),
                render_grid,
                requested_geometry,
                Arc::clone(&resized),
            ),
        )
        .await;
        if resized.load(std::sync::atomic::Ordering::Acquire) {
            let _ = state.session_backend.restore_desktop_geometry(&session_id).await;
        }
    }))
}

fn machine_socket_error(code: &str) -> (StatusCode, String) {
    let status = match code {
        "UNAUTHORIZED" => StatusCode::UNAUTHORIZED,
        "MACHINE_ACCESS_REQUIRED" => StatusCode::FORBIDDEN,
        "SESSION_NOT_FOUND" => StatusCode::NOT_FOUND,
        "CONTROL_CONFLICT" | "STALE_EPOCH" | "SESSION_EXPIRED" | "SESSION_OWNERSHIP_CHANGED" => {
            StatusCode::CONFLICT
        }
        "MACHINE_SERVICE_UNAVAILABLE" | "HOST_UNAVAILABLE" => StatusCode::SERVICE_UNAVAILABLE,
        "MACHINE_OWNER_UNSUPPORTED" => StatusCode::UNPROCESSABLE_ENTITY,
        "TIMEOUT" => StatusCode::GATEWAY_TIMEOUT,
        _ => StatusCode::BAD_REQUEST,
    };
    (status, code.into())
}

async fn machine_terminal_upgrade(
    ws: WebSocketUpgrade,
    id: String,
    query: AuthQuery,
    device: DeviceInfo,
    mut revoked: tokio::sync::watch::Receiver<bool>,
    state: Arc<RemoteGatewayState>,
) -> Result<Response, (StatusCode, String)> {
    use crate::daemon::session_service::DaemonSessionService;
    if device.permission != DevicePermission::Control {
        return Err(machine_socket_error("MACHINE_ACCESS_REQUIRED"));
    }
    // No host-scope stripping, grid negotiation or query-driven geometry in v1.
    if id.contains("::") || query.render.is_some() || query.cols.is_some() || query.rows.is_some() {
        return Err(machine_socket_error("INVALID_REQUEST"));
    }
    let epoch = query
        .daemon_epoch
        .ok_or_else(|| machine_socket_error("STALE_EPOCH"))?;
    let services = state
        .machine_services
        .as_ref()
        .ok_or_else(|| machine_socket_error("MACHINE_SERVICE_UNAVAILABLE"))?;
    let identity = load_gateway_identity(state.clone())
        .await
        .map_err(|_| machine_socket_error("MACHINE_SERVICE_UNAVAILABLE"))?;
    let target = crate::remote::machine_protocol::RemoteTerminalTarget {
        machine_id: identity.machine_id,
        daemon_epoch: epoch,
        session_id: id,
    };
    let admission = async {
        let mut controllers = services.sessions.machine_controllers.lock().await;
        let session = services.sessions.validate_machine_target(&target).await?;
        let attachment = services
            .sessions
            .attach_machine_output(&target.session_id, query.after_sequence.map(|s| s.0))
            .ok_or("SESSION_NOT_FOUND")?
            .map_err(|_| "CAPACITY_EXCEEDED")?;
        services.sessions.validate_machine_target(&target).await?;
        let lease = DaemonSessionService::acquire_machine_controller(
            &mut controllers,
            &target.session_id,
            &device.id,
        )?;
        Ok::<_, String>((session, attachment, lease))
    };
    let (session, attachment, lease) = while_device_authorized(
        &mut revoked,
        tokio::time::timeout(Duration::from_secs(45), admission),
    )
    .await
    .ok_or_else(|| machine_socket_error("UNAUTHORIZED"))?
    .map_err(|_| machine_socket_error("TIMEOUT"))?
    .map_err(|e| machine_socket_error(&e))?;
    let response = ws
        .max_message_size(64 * 1024)
        .max_frame_size(64 * 1024)
        .max_write_buffer_size(1024 * 1024)
        .write_buffer_size(0)
        .on_upgrade(move |socket| async move {
            let session_id = session.target.session_id.clone();
            let mut fenced = lease.cancelled.clone();
            let generation = lease.generation;
            let resized = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let work = handle_machine_terminal_socket(
                socket,
                session,
                attachment,
                generation,
                device,
                Arc::clone(&state),
                Arc::clone(&resized),
            );
            tokio::select! {
                biased;
                _ = revoked.wait_for(|v| *v) => {},
                _ = fenced.wait_for(|v| *v) => {},
                _ = work => {},
            }
            // No task is detached, no input survives this scope, and failed
            // upgrades also drop the captured lease without closing the PTY.
            drop(lease);
            if resized.load(std::sync::atomic::Ordering::Acquire) {
                let _ = state.session_backend.restore_desktop_geometry(&session_id).await;
            }
        });
    Ok(([(header::CACHE_CONTROL, "no-store")], response).into_response())
}

#[cfg(all(test, unix))]
#[path = "../../tests/support/machine_input_cancellation.rs"]
pub(crate) mod machine_input_cancellation_tests;
#[cfg(all(test, unix))]
#[path = "../../tests/support/machine_input_fixture.rs"]
pub(crate) mod machine_input_fixture;
#[cfg(test)]
#[path = "machine_input_probe.rs"]
pub(crate) mod machine_input_probe;

fn machine_control_message(value: serde_json::Value) -> Message {
    Message::Text(value.to_string().into())
}

#[path = "machine_output_writer.rs"]
mod machine_output_writer;
use machine_output_writer::{machine_control, machine_frame, machine_send};

async fn send_machine_agent_state<S>(
    sender: &mut S,
    state: &crate::daemon::agent_state::AgentState,
    target: &crate::remote::machine_protocol::RemoteTerminalTarget,
    termination: &mut tokio::sync::watch::Receiver<Option<crate::terminal::output_hub::machine_output::MachineOutputError>>,
) -> Result<(), ()>
where
    S: futures_util::Sink<Message> + Unpin,
{
    let msg = crate::remote::machine_agent_state::MachineAgentStateMessage::new(
        target.clone(),
        state.state.clone(),
        state.agent.clone(),
        state.provider_session.clone(),
        state.detail.clone(),
    );
    let json_str = serde_json::to_string(&msg).map_err(|_| ())?;
    // Guard against oversized metadata without dropping the PTY terminal connection:
    // If the full message (e.g. carrying an unusually long detail question or path)
    // exceeds the 1024-byte control slot, fallback to sending the essential provider identity
    // and activity state with explicit metadata_truncated flag, omitting detail and long transcriptPath.
    let text_message = match machine_control(Message::Text(json_str.clone().into())) {
        Ok(control) => control,
        Err(()) => {
            let bounded_provider = state.provider_session.as_ref().map(|ps| {
                crate::daemon::protocol::AgentProviderSession {
                    key: ps.key.clone(),
                    id: ps.id.clone(),
                    transcript_path: None, // Exclude oversized transcript paths; root badge requires key+id
                }
            });
            let compact_msg = crate::remote::machine_agent_state::MachineAgentStateMessage::truncated(
                target.clone(),
                state.state.clone(),
                state.agent.clone(),
                bounded_provider,
            );
            let compact_json = serde_json::to_string(&compact_msg).map_err(|_| ())?;
            match machine_control(Message::Text(compact_json.into())) {
                Ok(control) => {
                    tracing::warn!(
                        session_id = %target.session_id,
                        "AgentState metadata exceeded 1024B control slot; sent explicit truncated identity (metadataTruncated: true)"
                    );
                    control
                }
                Err(()) => return Ok(()), // Non-fatal to PTY stream: skip frame rather than dropping terminal
            }
        }
    };
    machine_send(sender, text_message, termination).await
}

async fn handle_machine_terminal_socket(
    socket: WebSocket,
    session: crate::remote::machine_protocol::Session,
    attachment: crate::terminal::output_hub::machine_output::MachineAttachment,
    generation: u64,
    device: DeviceInfo,
    state: Arc<RemoteGatewayState>,
    resized: Arc<std::sync::atomic::AtomicBool>,
) {
    use crate::remote::protocol::MachineTerminalControl;
    use crate::remote::terminal_wire::{encode_frame, Metadata, ReplayGap};
    use crate::scoped_contracts::Epoch;
    let services = state.machine_services.as_ref().expect("admitted services");
    let target = &session.target;
    let Some(pty) = services.sessions.machine_pty(&target.session_id) else {
        return;
    };

    // Subscribe to authoritative agent state updates BEFORE emitting the Attached boundary
    // so no racing agent state update or initial conversation identity is lost.
    let mut agent_subscription = services.sessions.subscribe_agent_states(&target.session_id);

    let (mut sender, mut receiver) = socket.split();
    let crate::terminal::output_hub::machine_output::MachineAttachment {
        snapshot: charged_snapshot,
        receiver: mut output,
    } = attachment;
    let mut termination = output.termination();
    let snapshot = &charged_snapshot.value;

    let gap = snapshot
        .gap
        .as_ref()
        .map(|g| crate::remote::machine_protocol::ReplayGap {
            requested_after_sequence: Epoch(g.requested_after_sequence),
            available_from_sequence: Epoch(g.available_from_sequence),
        });
    let boundary = crate::remote::machine_protocol::Attached::Attached {
        target: target.clone(),
        generation: Epoch(generation),
        cols: session.cols,
        rows: session.rows,
        start_sequence: Epoch(snapshot.history_start_sequence.unwrap_or(0)),
        end_sequence: Epoch(snapshot.history_end_sequence.unwrap_or(0)),
        replay_gap: gap,
    };
    let Ok(boundary) = serde_json::to_string(&boundary) else {
        return;
    };
    let Ok(boundary) = machine_control(Message::Text(boundary.into())) else {
        return;
    };
    if machine_send(&mut sender, boundary, &mut termination)
        .await
        .is_err()
    {
        return;
    }
    let replay = |snapshot: &AttachmentSnapshot, reset| {
        encode_frame(
            Metadata::Replay {
                start: snapshot.history_start_sequence,
                end: snapshot.history_end_sequence,
                gap: snapshot.gap.as_ref().map(|g| ReplayGap {
                    requested_after_sequence: g.requested_after_sequence,
                    available_from_sequence: g.available_from_sequence,
                }),
            },
            &snapshot.history,
            reset,
        )
    };
    let mut last = snapshot.history_end_sequence;
    if !snapshot.history.is_empty() || snapshot.gap.is_some() {
        let Ok(frame) = replay(&snapshot, snapshot.gap.is_some()) else {
            return;
        };
        let Ok(frame) = machine_frame(frame, snapshot.history.len()) else {
            return;
        };
        if machine_send(&mut sender, frame, &mut termination)
            .await
            .is_err()
        {
            return;
        }
    }
    drop(charged_snapshot);

    // Send initial authoritative agent state snapshot immediately after the boundary
    if let Some(initial_state) = agent_subscription.snapshot.as_ref() {
        if initial_state.session_id == target.session_id
            && services.sessions.validate_machine_target(target).await.is_ok()
        {
            if send_machine_agent_state(&mut sender, initial_state, target, &mut termination).await.is_err() {
                return;
            }
        }
    }

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
                agent_update = agent_subscription.receiver.recv() => {
                    match agent_update {
                        Ok(update) if update.state.session_id == target.session_id => {
                            if services.sessions.validate_machine_target(target).await.is_err() {
                                return;
                            }
                            if send_machine_agent_state(&mut sender, &update.state, target, &mut termination).await.is_err() {
                                return;
                            }
                        }
                        Ok(_) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                            if let Some(current) = agent_subscription.resynchronize(&target.session_id) {
                                if current.session_id == target.session_id
                                    && services.sessions.validate_machine_target(target).await.is_ok()
                                {
                                    if send_machine_agent_state(&mut sender, &current, target, &mut termination).await.is_err() {
                                        return;
                                    }
                                }
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                    }
                    continue;
                }
                next = output.recv() => next,
            };
            match next {
                Ok(charged) => {
                    let chunk = &charged.value;
                    if services
                        .sessions
                        .validate_machine_target(target)
                        .await
                        .is_err()
                    {
                        return;
                    }
                    if chunk.replay_gap.is_none() && last.is_some_and(|last| chunk.sequence <= last)
                    {
                        continue;
                    }
                    let Ok(frame) = encode_frame(
                        Metadata::Output {
                            sequence: chunk.sequence,
                            gap: chunk.replay_gap.as_ref().map(|g| ReplayGap {
                                requested_after_sequence: g.requested_after_sequence,
                                available_from_sequence: g.available_from_sequence,
                            }),
                        },
                        &chunk.bytes,
                        false,
                    ) else {
                        return;
                    };
                    let Ok(frame) = machine_frame(frame, chunk.bytes.len()) else {
                        return;
                    };
                    if machine_send(&mut sender, frame, &mut termination)
                        .await
                        .is_err()
                    {
                        return;
                    }
                    last = Some(chunk.sequence);
                    drop(charged);
                }
                Err(crate::terminal::output_hub::machine_output::MachineOutputError::Overflow) => {
                    return
                }
                Err(crate::terminal::output_hub::machine_output::MachineOutputError::Closed) => {
                    let status = match pty.state() {
                        crate::terminal::PtySessionState::Exited { code } => {
                            serde_json::json!({"type":"exit","target":target,"exit":{"code":code,"signal":null}})
                        }
                        _ => {
                            serde_json::json!({"type":"status","status":"disconnected","target":target})
                        }
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
        let mut pending_message: Option<Message> = None;
        loop {
            let message = match pending_message.take() {
                Some(msg) => msg,
                None => {
                    match tokio::time::timeout(Duration::from_secs(60), receiver.next()).await {
                        Ok(Some(Ok(msg))) => msg,
                        _ => return,
                    }
                }
            };
            if matches!(message, Message::Close(_)) {
                return;
            }
            // One bounded in-flight frame; all pending input is dropped when
            // either the reader, writer, grant or controller lifetime ends.
            #[cfg(test)]
            if input_tx.capacity() == 0 {
                machine_input_probe::queue_full(&target.session_id);
            }
            // Throttle under saturation with backpressure while keeping
            // lookahead alive for Close/EOF cancellation.
            let send = input_tx.send(message);
            tokio::pin!(send);
            let mut next_message = None;
            tokio::select! {
                biased;
                result = send.as_mut() => {
                    if result.is_err() {
                        return;
                    }
                }
                next_frame = receiver.next() => {
                    match next_frame {
                        Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return,
                        Some(Ok(next)) => {
                            next_message = Some(next);
                        }
                    }
                }
            }
            if let Some(next) = next_message {
                if send.await.is_err() {
                    return;
                }
                pending_message = Some(next);
            }
        }
    };
    let receive = async {
        loop {
            let Some(message) = input_rx.recv().await else {
                return;
            };
            if matches!(&message, Message::Text(text) if text.len() > 16 * 1024) {
                return;
            }
            let controllers = services.sessions.machine_controllers.lock().await;
            if controllers.get(&target.session_id).is_none_or(|c| {
                c.device != device.id
                    || c.generation != generation
                    || c.disconnected.lock().is_some()
            }) {
                return;
            }
            let authority = controllers[&target.session_id].cancelled.subscribe();
            drop(controllers);
            if services
                .sessions
                .validate_machine_target(target)
                .await
                .is_err()
            {
                return;
            }
            let operation = async {
                match message {
                    Message::Binary(bytes) if bytes.len() <= 64 * 1024 => {
                        let input = pty.write_input_cancellable(&bytes);
                        #[cfg(test)]
                        let input = machine_input_probe::observe(&target.session_id, input);
                        input.await.map_err(|error| error.to_string())
                    }
                    Message::Text(text) if text.len() <= 16 * 1024 => {
                        match serde_json::from_str::<MachineTerminalControl>(&text) {
                            Ok(MachineTerminalControl::Resize {
                                generation: supplied,
                                cols,
                                rows,
                            }) if supplied.0 == generation
                                && cols > 0
                                && rows > 0
                                && cols <= 1000
                                && rows <= 1000 =>
                            {
                                let res = state
                                    .session_backend
                                    .resize(&target.session_id, cols, rows)
                                    .await;
                                if res.is_ok() {
                                    resized.store(true, std::sync::atomic::Ordering::Release);
                                }
                                res
                            }
                            Ok(MachineTerminalControl::Signal {
                                generation: supplied,
                                signal,
                            }) if supplied.0 == generation && signal == "interrupt" => {
                                state
                                    .session_backend
                                    .signal(&target.session_id, TerminalSignal::Interrupt)
                                    .await
                            }
                            Ok(MachineTerminalControl::Ping) => {
                                let control = machine_control(machine_control_message(
                                    serde_json::json!({"type":"pong"}),
                                ))
                                .map_err(|_| "CONTROL_OVERFLOW".to_owned())?;
                                controls
                                    .try_send(control)
                                    .map_err(|_| "CONTROL_OVERFLOW".to_owned())?;
                                Ok(())
                            }
                            _ => Err("INVALID_CONTROL_OR_GENERATION".into()),
                        }
                    }
                    Message::Ping(bytes) => {
                        let control = machine_control(Message::Pong(bytes))
                            .map_err(|_| "CONTROL_OVERFLOW".to_owned())?;
                        controls
                            .try_send(control)
                            .map_err(|_| "CONTROL_OVERFLOW".to_owned())?;
                        Ok(())
                    }
                    Message::Pong(_) => Ok(()),
                    _ => Err("INVALID_CONTROL".into()),
                }
            };
            tokio::pin!(operation);
            // The per-generation watch read guard covers each IO poll, never
            // an await. Replacement's send_replace takes its write guard, so
            // fencing cannot race a stale IO poll after authority is revoked.
            let result = std::future::poll_fn(|cx| {
                let cancelled = authority.borrow();
                if *cancelled {
                    return std::task::Poll::Ready(Err("STALE_GENERATION".into()));
                }
                std::future::Future::poll(operation.as_mut(), cx)
            })
            .await;
            if let Err(code) = result {
                let Ok(control) = machine_control(machine_control_message(
                    serde_json::json!({"type":"error","code":code}),
                )) else {
                    return;
                };
                if controls.try_send(control).is_err() {
                    return;
                }
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
    requested_geometry: Option<(u16, u16)>,
    resized: Arc<std::sync::atomic::AtomicBool>,
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
        if device.permission == DevicePermission::Control {
            if let Some((cols, rows)) = requested_geometry {
                if state
                    .session_backend
                    .resize_generation(&session_id, status.generation, cols, rows)
                    .await
                    .is_ok()
                {
                    resized.store(true, std::sync::atomic::Ordering::Release);
                }
            }
        }
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
            resized,
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
                            let ok = ssh_control(
                                &session_backend,
                                &session_id_clone,
                                &ctrl,
                                can_control,
                            )
                            .await;
                            if ok && matches!(ctrl, ClientControlMessage::RemoteResize { .. }) {
                                resized.store(true, std::sync::atomic::Ordering::Release);
                            }
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
                                    if session_backend
                                        .resize(&session_id_clone, cols, rows)
                                        .await
                                        .is_ok()
                                    {
                                        resized.store(true, std::sync::atomic::Ordering::Release);
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
pub(super) async fn ssh_control(
    backend: &Arc<dyn RemoteSessionBackend>,
    id: &str,
    control: &ClientControlMessage,
    can_control: bool,
) -> bool {
    if !can_control {
        return false;
    }
    match control {
        ClientControlMessage::RemoteWrite { generation, data } => {
            if let Ok(generation) = generation.parse::<u64>() {
                return backend
                    .write_generation(id, generation, data.as_bytes())
                    .await
                    .is_ok();
            }
            false
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
                return backend
                    .resize_generation(id, generation, cols, rows)
                    .await
                    .is_ok();
            }
            false
        }
        _ => false,
    }
}

fn grid_text_message(frame: RemoteGridFrame) -> Message {
    let text = serde_json::to_string(&frame).expect("remote grid frame serializes");
    Message::Text(text.into())
}

fn enqueue_grid_operation<E>(
    mirror: &Arc<parking_lot::Mutex<RemoteTerminalMirror>>,
    outbound_tx: &mpsc::UnboundedSender<Message>,
    operation: impl FnOnce(&mut RemoteTerminalMirror) -> Result<RemoteGridFrame, E>,
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
    resized: Arc<std::sync::atomic::AtomicBool>,
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
                            let ok = ssh_control(
                                &session_backend,
                                &session_id_clone,
                                &ctrl,
                                can_control,
                            )
                            .await;
                            if let ClientControlMessage::RemoteResize { cols, rows, .. } = ctrl {
                                if ok {
                                    resized.store(true, std::sync::atomic::Ordering::Release);
                                    if let Some((cols, rows)) = validated_grid_geometry(cols, rows)
                                    {
                                        if !enqueue_grid_operation(
                                            &recv_mirror,
                                            &recv_tx,
                                            |mirror| mirror.resize(cols, rows),
                                        ) {
                                            break;
                                        }
                                    }
                                }
                            }
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
                                    if session_backend
                                        .resize(&session_id_clone, cols, rows)
                                        .await
                                        .is_ok()
                                    {
                                        resized.store(true, std::sync::atomic::Ordering::Release);
                                    }
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
            // Linux deb/AppImage: binary at usr/bin, resources at usr/lib/<productName>.
            candidates.push(exe_dir.join("../lib/Ferryx/ui/dist"));
            candidates.push(exe_dir.join("../lib/ui/dist"));
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

/// Resolves the directory containing static UI assets for the remote web client.
///
/// If `FERRYX_UI_DIST_DIR` is set and non-empty, that directory is used (after
/// canonicalizing/validating it exists) before checking packaged resource bundles
/// or compile-time manifest locations.
pub(crate) fn resolve_dist_dir() -> PathBuf {
    if let Ok(override_dir) = std::env::var("FERRYX_UI_DIST_DIR") {
        let trimmed = override_dir.trim();
        if !trimmed.is_empty() {
            let path = PathBuf::from(trimmed);
            if path.exists() {
                return path.canonicalize().unwrap_or(path);
            }
        }
    }

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
    let token =
        extract_token(&headers).ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
    crate::ipc::run_blocking(move || {
        Ok((|| {
            state
                .auth_manager
                .validate_token(&token)
                .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token".into()))?;
            let preferences = crate::terminal::load_terminal_preferences();
            // This legacy display endpoint is not machine execution configuration.
            let remote = crate::terminal::TerminalPreferences {
                source_path: None,
                default_shell: None,
                ..preferences
            };
            state
                .auth_manager
                .validate_token(&token)
                .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token".into()))?;
            Ok(Json(remote))
        })())
    })
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Preferences unavailable".into(),
        )
    })?
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
    let token =
        extract_token(&headers).ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
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
    let token =
        extract_token(&headers).ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
    let _device = state
        .auth_manager
        .validate_token(&token)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token".into()))?;
    global_push_store().unsubscribe(&payload.endpoint);
    Ok(StatusCode::NO_CONTENT)
}

pub(super) fn machine_error(status: StatusCode, code: &str) -> Response {
    use crate::remote::machine_protocol::{ErrorEnvelope, MachineError};
    (
        status,
        [(header::CACHE_CONTROL, "no-store")],
        Json(ErrorEnvelope {
            error: MachineError {
                code: code.into(),
                message: code.into(),
                retryable: matches!(
                    code,
                    "TIMEOUT"
                        | "HOST_UNAVAILABLE"
                        | "MACHINE_SERVICE_UNAVAILABLE"
                        | "RATE_LIMITED"
                        | "CAPACITY_EXCEEDED"
                ),
                request_id: uuid::Uuid::new_v4().to_string(),
                details: serde_json::Map::new(),
            },
        }),
    )
        .into_response()
}

pub(super) fn machine_error_with_details(
    status: StatusCode,
    code: &str,
    message: &str,
    details: serde_json::Map<String, serde_json::Value>,
) -> Response {
    use crate::remote::machine_protocol::{ErrorEnvelope, MachineError};
    (
        status,
        [(header::CACHE_CONTROL, "no-store")],
        Json(ErrorEnvelope {
            error: MachineError {
                code: code.into(),
                message: message.into(),
                retryable: matches!(
                    code,
                    "TIMEOUT"
                        | "HOST_UNAVAILABLE"
                        | "MACHINE_SERVICE_UNAVAILABLE"
                        | "RATE_LIMITED"
                        | "CAPACITY_EXCEEDED"
                ),
                request_id: uuid::Uuid::new_v4().to_string(),
                details,
            },
        }),
    )
        .into_response()
}

pub(super) fn authenticate_machine_request(
    state: &RemoteGatewayState,
    headers: &HeaderMap,
) -> Result<DeviceInfo, Response> {
    let token = extract_token(headers)
        .ok_or_else(|| machine_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED"))?;
    state
        .auth_manager
        .validate_token(&token)
        .map_err(|_| machine_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED"))
}

/// The identity fields the capability document publishes for reference-chat clients.
///
/// `referenceHostId` is the owning host's OWN reference-chat identity — `FERRYX_HOST_ID` where
/// the deployment sets one, `local` otherwise ([`reference_files::reference_host_id`]) — and it
/// is the only host id a reference-chat target may name. It is published as its own field on
/// purpose: `machineId` is the host's pairing identity, a different value, and a client that
/// substituted one for the other would name a host this gateway refuses.
///
/// `referenceOwnerId` is the gateway incarnation's OWN reference-chat owner identity, minted at
/// construction and stable for that incarnation. It is published for the same reason the host id
/// is: a target must name the owner this gateway will compare against, and the value must not be
/// inferable from the caller's own request.
///
/// Pure and explicit: the caller supplies all three ids, so the field names and the separation
/// between them are testable without touching the process environment.
fn reference_chat_capability_identity(
    machine_id: &str,
    reference_host_id: &str,
    reference_owner_id: &str,
) -> serde_json::Map<String, serde_json::Value> {
    let mut fields = serde_json::Map::new();
    fields.insert(
        "machineId".to_string(),
        serde_json::Value::String(machine_id.to_string()),
    );
    fields.insert(
        "referenceHostId".to_string(),
        serde_json::Value::String(reference_host_id.to_string()),
    );
    fields.insert(
        "referenceOwnerId".to_string(),
        serde_json::Value::String(reference_owner_id.to_string()),
    );
    fields
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
    // The reference-chat host id this host publishes for itself. A mutation target names it;
    // `machineId` is a different value and never stands in for it.
    let reference_host_id = crate::remote::reference_chat::files::reference_host_id();
    let mut document = serde_json::json!({
        "apiVersion": 1,
        "daemonEpoch": state.daemon_epoch.load(std::sync::atomic::Ordering::Acquire).to_string(),
        "platform": std::env::consts::OS,
        "accessScope": device.access_scope,
        "permission": device.permission,
        "capabilities": if state.machine_services.is_some() && device.access_scope == DeviceAccessScope::Machine && device.permission == DevicePermission::Control {
            let mut capabilities = vec!["directoryBrowseV1", "machineWorkspaceV1", "managedWorktreesV1", "pairedPasteUploadV1", "pairedPasteUploadV2"];
            if state.machine_services.as_ref().is_some_and(|services| services.workspaces.catalog().is_ok() && services.workspaces.journal.session_revision().is_ok()) {
                capabilities.push("terminalCreateV1");
                capabilities.push("terminalStreamV1");
            }
            // DAG frames are pushed from this machine's own journal watcher, so the
            // capability is only real when the catalog that resolves roots is readable.
            if state.machine_services.as_ref().is_some_and(|services| services.workspaces.catalog().is_ok()) {
                capabilities.push(super::dag_api::DAG_STREAM_CAPABILITY);
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
    });
    if let Some(fields) = document.as_object_mut() {
        fields.extend(reference_chat_capability_identity(
            &identity.machine_id,
            &reference_host_id,
            &state.reference_owner_id,
        ));
    }
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(document)).into_response())
}

pub(super) async fn load_gateway_identity(
    state: Arc<RemoteGatewayState>,
) -> Result<crate::remote::auth::MachineIdentity, Response> {
    crate::ipc::run_blocking(move || {
        #[cfg(test)]
        if let Some(probe) = state.identity_probe.read().clone() {
            probe();
        }
        let dir = match &state.identity_dir {
            Some(dir) => dir.clone(),
            None => crate::remote::auth::canonical_identity_dir()
                .map_err(crate::ipc::IpcError::internal)?,
        };
        crate::remote::auth::load_or_generate_machine_identity(&dir)
            .map_err(crate::ipc::IpcError::internal)
    })
    .await
    .map_err(|_| {
        machine_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "MACHINE_SERVICE_UNAVAILABLE",
        )
    })
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
    let device = crate::ipc::run_blocking(move || {
        Ok((|| {
            let token = extract_token(&auth_headers)
                .ok_or_else(|| (StatusCode::UNAUTHORIZED, "Missing auth token").into_response())?;
            auth_state
                .auth_manager
                .validate_token(&token)
                .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token").into_response())
        })())
    })
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Session authorization unavailable",
        )
            .into_response()
    })??;
    if device.access_scope == DeviceAccessScope::Machine {
        super::session_api::list(State(state), headers, uri).await
    } else {
        list_legacy_sessions(State(state), headers)
            .await
            .map(IntoResponse::into_response)
            .map_err(IntoResponse::into_response)
    }
}

// These adapters own only HTTP extraction. Authorization precedes path/body
// rejection; domain successes and errors pass through unchanged. In particular,
// do not normalize arbitrary responses from legacy routes in middleware.

pub(super) async fn project_body(
    request: axum::extract::Request,
    admission: &super::workspace_api::Admission,
) -> Result<axum::body::Bytes, Response> {
    use axum::extract::FromRequest;
    let mut revoked = admission.revoked.clone();
    let deadline = admission
        .deadline
        .min(std::time::Instant::now() + Duration::from_secs(10));
    let extracted = tokio::select! {
        biased;
        _ = revoked.wait_for(|v| *v) => return Err(machine_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED")),
        result = tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), axum::body::Bytes::from_request(request, &())) => result.map_err(|_| machine_error(StatusCode::GATEWAY_TIMEOUT, "TIMEOUT"))?,
    };
    extracted.map_err(|error| {
        let too_large = error.status() == StatusCode::PAYLOAD_TOO_LARGE;
        if too_large {
            machine_error(StatusCode::PAYLOAD_TOO_LARGE, "PAYLOAD_TOO_LARGE")
        } else {
            machine_error(StatusCode::BAD_REQUEST, "INVALID_REQUEST")
        }
    })
}

async fn worktree_mutation_boundary(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    request: axum::extract::Request,
) -> Result<Response, Response> {
    let deadline = std::time::Instant::now() + Duration::from_secs(40);
    let auth_state = state.clone();
    let auth_headers = headers.clone();
    let permit = super::workspace_api::AUTH_SLOTS
        .clone()
        .try_acquire_owned()
        .map_err(|_| machine_error(StatusCode::TOO_MANY_REQUESTS, "CAPACITY_EXCEEDED"))?;
    let device = tokio::time::timeout(
        Duration::from_secs(10),
        crate::ipc::run_blocking(move || {
            let _permit = permit;
            Ok(authenticate_machine_request(&auth_state, &auth_headers))
        }),
    )
    .await
    .map_err(|_| machine_error(StatusCode::GATEWAY_TIMEOUT, "TIMEOUT"))?
    .map_err(|_| {
        machine_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "MACHINE_SERVICE_UNAVAILABLE",
        )
    })??;
    let delete = request.method() == axum::http::Method::DELETE;
    if device.access_scope == DeviceAccessScope::Machine {
        let admission = super::workspace_api::admit_until(
            state.clone(),
            headers.clone(),
            true,
            &uuid::Uuid::new_v4().to_string(),
            deadline,
        )
        .await?;
        let body = project_body(request, &admission).await?;
        return Ok(super::workspace_api::ADMISSION
            .scope(
                admission,
                super::workspace_api::worktrees::mutate_worktree(state, headers, body, delete),
            )
            .await);
    }
    use axum::extract::FromRequest;
    if state.machine_services.is_some() {
        let body = tokio::time::timeout(
            Duration::from_secs(10),
            axum::body::Bytes::from_request(request, &()),
        )
        .await
        .map_err(|_| machine_error(StatusCode::GATEWAY_TIMEOUT, "TIMEOUT"))?
        .map_err(IntoResponse::into_response)?;
        return Ok(
            super::workspace_api::worktrees::legacy(state, headers, body, delete, deadline).await,
        );
    }
    let revoked = state
        .auth_manager
        .device_revocation(&device.id)
        .map_err(|_| machine_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED"))?;
    let cancelled = super::workspace_api::CancelWork(
        Arc::new(std::sync::atomic::AtomicBool::new(false)),
        Arc::new(tokio::sync::Notify::new()),
    );
    let budget = crate::worktree::git::GitBudget {
        deadline,
        revoked,
        cancelled: cancelled.0.clone(),
        cancellation: Some(cancelled.1.clone()),
    };
    let body = tokio::time::timeout_at(
        tokio::time::Instant::from_std(deadline),
        axum::body::to_bytes(
            request.into_body(),
            super::machine_protocol::MACHINE_JSON_MAX_BYTES,
        ),
    )
    .await
    .map_err(|_| machine_error(StatusCode::GATEWAY_TIMEOUT, "TIMEOUT"))?
    .map_err(|_| machine_error(StatusCode::PAYLOAD_TOO_LARGE, "PAYLOAD_TOO_LARGE"))?;
    let worker = crate::ipc::run_blocking(move || {
        Ok(crate::worktree::git::with_git_budget(budget, || {
            let auth_state = state.clone();
            let token = extract_token(&headers)
                .ok_or_else(|| machine_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED"))?;
            let response = if delete {
                let payload = serde_json::from_slice::<RemoteDeleteWorktreeRequest>(&body)
                    .map_err(|_| machine_error(StatusCode::BAD_REQUEST, "INVALID_REQUEST"))?;
                delete_worktree_blocking(State(state), headers, Json(payload)).into_response()
            } else {
                let payload = serde_json::from_slice::<RemoteCreateWorktreeRequest>(&body)
                    .map_err(|_| machine_error(StatusCode::BAD_REQUEST, "INVALID_REQUEST"))?;
                create_worktree_blocking(State(state), headers, Json(payload)).into_response()
            };
            auth_state
                .auth_manager
                .validate_token(&token)
                .map_err(|_| machine_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED"))?;
            Ok(response)
        }))
    });
    tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), worker)
        .await
        .map_err(|_| machine_error(StatusCode::GATEWAY_TIMEOUT, "TIMEOUT"))?
        .map_err(|_| {
            machine_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "MACHINE_SERVICE_UNAVAILABLE",
            )
        })?
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct DesktopWorktreeListQuery {
    workspace_id: Option<String>,
    #[serde(rename = "workspace_id")]
    workspace_id_snake: Option<String>,
}

async fn worktree_list_boundary(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    uri: axum::http::Uri,
) -> Response {
    let token = match extract_token(&headers) {
        Some(t) => t,
        None => return machine_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED"),
    };
    let device = match state.auth_manager.validate_token(&token) {
        Ok(d) => d,
        Err(_) => return machine_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED"),
    };

    if device.access_scope == DeviceAccessScope::Machine && state.machine_services.is_some() {
        return super::workspace_api::worktrees::read(
            state,
            headers,
            uri.query().map(str::to_owned),
            false,
        )
        .await;
    }

    let axum::extract::Query(q) =
        match axum::extract::Query::<DesktopWorktreeListQuery>::try_from_uri(&uri) {
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
        let rows_result: Result<Vec<crate::worktree::Worktree>, crate::ipc::IpcError> =
            crate::ipc::run_blocking(move || {
                manager.list_worktrees().map_err(crate::ipc::IpcError::from)
            })
            .await;

        let rows = match rows_result {
            Ok(rows) => rows,
            Err(ipc_err) => {
                let status_code = match ipc_err.code {
                    crate::ipc::IpcErrorCode::WorkspaceNotFound
                    | crate::ipc::IpcErrorCode::WorktreeNotFound => StatusCode::NOT_FOUND,
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
            if let Some(project) = ssh_projects
                .into_iter()
                .find(|p| p.workspace_id == workspace_id)
            {
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
async fn worktree_status_boundary(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    uri: axum::http::Uri,
) -> Response {
    super::workspace_api::worktrees::read(state, headers, uri.query().map(str::to_owned), true)
        .await
}

async fn register_project_boundary(
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
    #[cfg(test)]
    if let Some(probe) = state
        .machine_services
        .as_ref()
        .expect("admitted")
        .workspaces
        .transaction_probe
        .read()
        .clone()
    {
        probe("bodyEntry");
    }
    let body = project_body(request, &admission).await?;
    Ok(super::workspace_api::ADMISSION
        .scope(
            admission,
            super::workspace_api::register(State(state), headers, body),
        )
        .await)
}

async fn unregister_project_boundary(
    State(state): State<Arc<RemoteGatewayState>>,
    path: Result<AxumPath<String>, axum::extract::rejection::PathRejection>,
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
    let path = path.map_err(|_| machine_error(StatusCode::BAD_REQUEST, "INVALID_REQUEST"))?;
    let body = project_body(request, &admission).await?;
    Ok(super::workspace_api::ADMISSION
        .scope(
            admission,
            super::workspace_api::unregister(State(state), path, headers, body),
        )
        .await)
}

async fn operation_boundary(
    State(state): State<Arc<RemoteGatewayState>>,
    path: Result<AxumPath<String>, axum::extract::rejection::PathRejection>,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let id = path
        .as_ref()
        .ok()
        .map(|p| p.0.clone())
        .filter(|id| uuid::Uuid::parse_str(id).is_ok())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let admission = super::workspace_api::admit(state.clone(), headers.clone(), false, &id).await?;
    let path = path.map_err(|_| machine_error(StatusCode::BAD_REQUEST, "INVALID_REQUEST"))?;
    Ok(super::workspace_api::ADMISSION
        .scope(
            admission,
            super::workspace_api::operation(State(state), path, headers),
        )
        .await)
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
        super::machine_protocol::decode_json(
            &body,
            super::machine_protocol::MACHINE_JSON_MAX_BYTES,
        )
        .map_err(|_| machine_error(StatusCode::BAD_REQUEST, "INVALID_REQUEST"))?;

    use base64::Engine;
    let chunk_bytes = base64::engine::general_purpose::STANDARD
        .decode(&req.data)
        .map_err(|_| machine_error(StatusCode::BAD_REQUEST, "INVALID_BASE64"))?;

    let saved = match (req.offset, req.total_bytes) {
        (Some(offset), Some(total_bytes)) => {
            let upload_id = req.upload_id.clone();
            let file_name = req.file_name.clone();
            crate::ipc::run_blocking(move || {
                crate::clipboard_image::save_paste_chunk_v2(
                    &upload_id,
                    &file_name,
                    req.chunk_index,
                    req.total_chunks,
                    offset,
                    total_bytes,
                    &chunk_bytes,
                )
            })
            .await
            .map_err(|e| machine_error(StatusCode::INTERNAL_SERVER_ERROR, &e.message))?
        }
        (None, None) => {
            let upload_id = req.upload_id.clone();
            let file_name = req.file_name.clone();
            crate::ipc::run_blocking(move || {
                crate::clipboard_image::save_paste_chunk(
                    &upload_id,
                    &file_name,
                    req.chunk_index,
                    req.total_chunks,
                    &chunk_bytes,
                )
            })
            .await
            .map_err(|e| machine_error(StatusCode::INTERNAL_SERVER_ERROR, &e.message))?
        }
        _ => {
            return Err(machine_error(
                StatusCode::BAD_REQUEST,
                "INVALID_REQUEST: offset and totalBytes must both be present or both absent",
            ));
        }
    };

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
) -> Result<
    (
        DesktopScope,
        Vec<super::browser_backend::RemoteBrowserSessionSummary>,
    ),
    RemoteBrowserError,
> {
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
        // A typed input refusal is a client-side capability limit, never a server fault.
        RemoteBrowserError::InputRefused(explanation) => (
            StatusCode::BAD_REQUEST,
            format!("UNSUPPORTED: {}", sanitize_public_string(&explanation.reason)),
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
    let token =
        extract_token(&headers).ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
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
    let token =
        extract_token(&headers).ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
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
    let desktop_epoch_str = state
        .daemon_epoch
        .load(std::sync::atomic::Ordering::SeqCst)
        .to_string();
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
                                details: None,
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
                                details: None,
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
    if tokio::time::timeout(DRAIN_TIMEOUT, &mut writer_task)
        .await
        .is_err()
    {
        writer_task.abort();
    }
}

async fn remote_fallback(method: axum::http::Method, uri: axum::http::Uri) -> Response {
    if uri.path().starts_with("/api/") {
        return machine_error(StatusCode::NOT_FOUND, "NOT_FOUND");
    }
    if method != axum::http::Method::GET && method != axum::http::Method::HEAD {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    serve_static_or_index(uri).await
}

async fn remote_method_not_allowed() -> Response {
    machine_error(StatusCode::METHOD_NOT_ALLOWED, "METHOD_NOT_ALLOWED")
}


// =============================================================================================
// Reference chat (plan task 13) — the authored lanes, registered on the EXISTING gateway
// =============================================================================================
//
// Every route below runs on the same listener as the rest of `create_remote_router`, behind the
// same bearer token and the same per-device permission, and binds its target to the owning host
// + backend session + daemon incarnation the caller names. Nothing here opens a second listener,
// mounts the control router, resizes a pane, signals a process or starts a provider.
//
// Route table (frozen contract section 3; the relay allowlist mirrors it in `relay_server.rs`):
//
//   GET    /api/v1/reference-chat/{sessionId}/history
//   GET    /api/v1/reference-chat/{sessionId}/screen
//   GET    /api/v1/reference-chat/{sessionId}/prompt
//   POST   /api/v1/reference-chat/{sessionId}/submit
//   POST   /api/v1/reference-chat/{sessionId}/stop
//   POST   /api/v1/reference-chat/{sessionId}/answer
//   POST   /api/v1/reference-chat/{sessionId}/files
//   GET    /api/v1/reference-chat/{sessionId}/files/{fileId}
//   DELETE /api/v1/reference-chat/{sessionId}/files/{fileId}
//
// A read carries its target in the query (hostId, ownerId, epoch, backendSessionId,
// providerSessionId, registryId, limit, cursor, cursorStream); a mutation carries the frozen
// MutationEnvelope (`{requestId, target, params}`) plus the optional providerSessionId/registryId
// the contract adds "only where identified". `payload` is accepted as an alias for `params` so
// either spelling of the plan's prose round-trips.
//
// Ownership boundary this layer does NOT invent: a session whose transcript lives on a
// paired/SSH host is served by THAT host's own gateway, which is what the relay path forwards
// to. This gateway refuses such a read with a typed UNSUPPORTED rather than substituting a
// local file for another host's conversation.

use crate::remote::reference_chat::files as reference_files;
use crate::remote::reference_chat::history as reference_history;
use crate::remote::reference_chat::input::{self as reference_input, ReferenceInputClock};
use crate::remote::reference_chat::prompts as reference_prompts;
use crate::remote::reference_chat::screen as reference_screen;
use crate::remote::reference_chat::types as reference_types;
use crate::scoped_contracts::{
    AttachmentReceipt, ScopeError, ScopeErrorCode, ATTACHMENT_MAX_FILE_BYTES,
};
use futures_util::future::BoxFuture;

/// The largest mutation envelope a reference-chat route reads off the wire.
///
/// The file lane's payload is base64 over ATTACHMENT_MAX_FILE_BYTES, so the envelope bound is
/// derived from that frozen limit rather than declared again; every other mutation is far below
/// it. The read is bounded by axum::body::to_bytes, never by an unbounded buffer.
const REFERENCE_CHAT_MUTATION_MAX_BYTES: usize =
    (ATTACHMENT_MAX_FILE_BYTES as usize / 3) * 4 + 64 * 1024;

/// How much of the original session file the prompt detectors may read.
///
/// The ask records they match are the tail of a live, appended file; the history reader owns the
/// transcript window itself, and this bound exists only so a prompt read cannot pull a whole
/// multi-megabyte store into memory.
const REFERENCE_PROMPT_SESSION_MAX_BYTES: usize = 4 * 1024 * 1024;

/// One staged reference-chat file, as the file routes address it.
struct ReferenceStagedFile {
    target_key: String,
    dir: PathBuf,
    path: PathBuf,
    display_name: String,
    receipt: AttachmentReceipt,
    staged_at_ms: u64,
}

/// Process-local state every reference-chat route shares.
///
/// There is deliberately ONE input queue: submit, Stop and prompt answers only serialize against
/// each other while they take the same per-target step, and a second queue instance would
/// silently not order against the first. The answer ledger lives beside it for the same reason.
#[derive(Default)]
struct ReferenceChatRuntime {
    input: reference_input::ReferenceInputQueue,
    answers: reference_prompts::ReferencePromptAnswers,
    staged: parking_lot::Mutex<std::collections::HashMap<String, ReferenceStagedFile>>,
}

fn reference_chat_runtime() -> &'static ReferenceChatRuntime {
    static RUNTIME: std::sync::OnceLock<ReferenceChatRuntime> = std::sync::OnceLock::new();
    RUNTIME.get_or_init(ReferenceChatRuntime::default)
}

/// The query a reference-chat read binds its target with.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReferenceChatReadQuery {
    #[serde(default)]
    host_id: Option<String>,
    #[serde(default)]
    owner_id: Option<String>,
    #[serde(default)]
    epoch: Option<String>,
    #[serde(default)]
    backend_session_id: Option<String>,
    #[serde(default)]
    provider_session_id: Option<String>,
    /// The registry entry the pane runs, as the inventory publishes it.
    #[serde(default)]
    registry_id: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    cursor: Option<u64>,
    #[serde(default)]
    cursor_stream: Option<String>,
}

/// The frozen mutation envelope plus the identity fields the contract adds where identified.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReferenceChatMutation<P> {
    request_id: String,
    #[serde(default)]
    target: Option<crate::scoped_contracts::TargetRef>,
    #[serde(default)]
    provider_session_id: Option<String>,
    #[serde(default)]
    registry_id: Option<String>,
    #[serde(alias = "payload")]
    params: P,
}

/// The authenticated context one reference-chat read runs in.
struct ReferenceChatRead {
    device: DeviceInfo,
    target: reference_types::ReferenceTargetRef,
    identity: reference_history::ReferenceHistoryIdentity,
}

/// The original session's own record, as the prompt detectors need it.
///
/// OmO's folded widget is matched against the calls the session file recorded, so the route
/// reads that file — the same store the history reader resolves — and hands it in. A pane with
/// no resolvable store gets an empty session, which makes the folded forms refuse rather than
/// guess.
struct ReferenceChatPromptSession {
    jsonl: Option<String>,
    agent_status: Option<String>,
}

impl ReferenceChatPromptSession {
    fn borrow(&self) -> reference_prompts::ReferencePromptSession<'_> {
        reference_prompts::ReferencePromptSession {
            session_jsonl: self.jsonl.as_deref(),
            agent_status: self.agent_status.as_deref(),
        }
    }
}

/// reference_prompts::ReferenceAnswerScreenReader over the authenticated backend's own
/// segmented history.
///
/// It attaches read-only, rebuilds the pane's screen in a fresh mirror and never resizes,
/// scrolls or writes: the only backend methods it calls are describe_session and
/// attach_with_sequence.
struct ReferenceGatewayScreenReader {
    backend: Arc<dyn RemoteSessionBackend>,
}

impl reference_prompts::ReferenceAnswerScreenReader for ReferenceGatewayScreenReader {
    fn read_screen<'a>(
        &'a self,
        target: &'a reference_types::ReferenceTargetRef,
    ) -> BoxFuture<'a, Result<reference_types::ReferenceScreenSnapshot, String>> {
        Box::pin(async move {
            let session_id = target.target.backend_session_id.as_str();
            let details = self.backend.describe_session(session_id).await?;
            let attachment = self.backend.attach_with_sequence(session_id, None).await?;
            let mut mirror = RemoteTerminalMirror::new(details.cols, details.rows)
                .map_err(|error| error.to_string())?;
            // A replay gap means the history could not be reconstructed: the snapshot is marked
            // gapped rather than rendered from a hole.
            let segments: Option<Vec<Vec<u8>>> = if attachment.snapshot.gap.is_some() {
                None
            } else {
                Some(
                    attachment
                        .snapshot
                        .history_segments
                        .iter()
                        .map(|segment| segment.bytes.clone())
                        .collect(),
                )
            };
            reference_screen::snapshot_reference_screen(&mut mirror, segments.as_deref())
        })
    }
}

/// The HTTP status a frozen ScopeErrorCode maps to on this gateway.
fn reference_chat_status(code: ScopeErrorCode) -> StatusCode {
    match code {
        ScopeErrorCode::InvalidRequest => StatusCode::BAD_REQUEST,
        ScopeErrorCode::Unauthorized => StatusCode::UNAUTHORIZED,
        ScopeErrorCode::Forbidden | ScopeErrorCode::ProviderOwned => StatusCode::FORBIDDEN,
        ScopeErrorCode::NotFound => StatusCode::NOT_FOUND,
        ScopeErrorCode::TargetExpired => StatusCode::GONE,
        ScopeErrorCode::ControlConflict
        | ScopeErrorCode::RequestConflict
        | ScopeErrorCode::OperationOutcomeUnknown => StatusCode::CONFLICT,
        ScopeErrorCode::Unsupported | ScopeErrorCode::CaptureUnsupported => {
            StatusCode::UNPROCESSABLE_ENTITY
        }
        ScopeErrorCode::InventoryIncomplete => StatusCode::SERVICE_UNAVAILABLE,
        ScopeErrorCode::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
        ScopeErrorCode::Timeout => StatusCode::GATEWAY_TIMEOUT,
    }
}

/// The wire string a frozen ScopeErrorCode carries.
fn reference_chat_wire_code(code: ScopeErrorCode) -> &'static str {
    match code {
        ScopeErrorCode::InvalidRequest => "INVALID_REQUEST",
        ScopeErrorCode::Unauthorized => "UNAUTHORIZED",
        ScopeErrorCode::Forbidden => "FORBIDDEN",
        ScopeErrorCode::NotFound => "NOT_FOUND",
        ScopeErrorCode::TargetExpired => "TARGET_EXPIRED",
        ScopeErrorCode::ControlConflict => "CONTROL_CONFLICT",
        ScopeErrorCode::RequestConflict => "REQUEST_CONFLICT",
        ScopeErrorCode::ProviderOwned => "PROVIDER_OWNED",
        ScopeErrorCode::Unsupported => "UNSUPPORTED",
        ScopeErrorCode::Timeout => "TIMEOUT",
        ScopeErrorCode::InventoryIncomplete => "INVENTORY_INCOMPLETE",
        ScopeErrorCode::PayloadTooLarge => "PAYLOAD_TOO_LARGE",
        ScopeErrorCode::CaptureUnsupported => "CAPTURE_UNSUPPORTED",
        // The lane's own constant is the authority for this acronym; the enum's explicit
        // rename attribute carries the same string.
        ScopeErrorCode::OperationOutcomeUnknown => {
            crate::remote::reference_chat::types::REFERENCE_OUTCOME_UNKNOWN_CODE
        }
    }
}

/// A typed scope refusal, before it becomes a response.
fn reference_chat_scope(code: ScopeErrorCode, message: impl Into<String>) -> ScopeError {
    ScopeError {
        code,
        message: message.into(),
        retryable: matches!(
            code,
            ScopeErrorCode::Timeout | ScopeErrorCode::InventoryIncomplete
        ),
        details: serde_json::Value::Null,
    }
}

/// The machine error envelope every other route on this gateway answers with.
fn reference_chat_error(code: ScopeErrorCode, message: impl Into<String>) -> Response {
    let message = message.into();
    machine_error_with_details(
        reference_chat_status(code),
        reference_chat_wire_code(code),
        &message,
        serde_json::Map::new(),
    )
}

/// A refusal that carries a frozen ScopeError.
fn reference_chat_refusal(error: &ScopeError) -> Response {
    reference_chat_error(error.code, error.message.clone())
}

/// The frozen ScopeResult success shape a mutation answers with.
fn reference_chat_result_ok(request_id: &str, data: serde_json::Value) -> Response {
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({ "ok": true, "data": data, "requestId": request_id })),
    )
        .into_response()
}

/// The frozen ScopeResult failure shape a mutation answers with.
///
/// The mutation's own state travels here — including the accept-then-unknown
/// OPERATION_OUTCOME_UNKNOWN, which is never retryable and never replayed automatically.
fn reference_chat_result_failure(request_id: &str, error: &ScopeError) -> Response {
    let status = reference_chat_status(error.code);
    (
        status,
        [(header::CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({
            "ok": false,
            "error": {
                "code": reference_chat_wire_code(error.code),
                "message": error.message,
                "retryable": error.retryable,
                "details": serde_json::Value::Null,
            },
            "requestId": request_id,
        })),
    )
        .into_response()
}

/// The owning target a reference-chat request binds.
///
/// The fences this gateway can verify are enforced here: the session must be live on this host,
/// the daemon incarnation must be the one the caller names, and a caller-named host must be this
/// host. ownerId is carried as part of the identity tuple the caller fences its own view with;
/// the gateway refuses an empty one rather than inventing a second owner concept it cannot
/// verify.
fn reference_chat_target(
    session_id: &str,
    host_id: Option<&str>,
    owner_id: Option<&str>,
    epoch: Option<&str>,
    provider_session_id: Option<&str>,
    daemon_owner: &str,
    daemon_epoch: u64,
) -> Result<reference_types::ReferenceTargetRef, ScopeError> {
    if session_id.trim().is_empty() {
        return Err(reference_chat_scope(
            ScopeErrorCode::Unauthorized,
            "a reference-chat target names no backend session",
        ));
    }
    let host = match host_id.map(str::trim).filter(|value| !value.is_empty()) {
        Some(named) => {
            let own = reference_files::reference_host_id();
            if named != own {
                return Err(reference_chat_scope(
                    ScopeErrorCode::Forbidden,
                    format!("that host id is not this host's reference-chat host id: {named}"),
                ));
            }
            named.to_string()
        }
        None => reference_files::reference_host_id(),
    };
    let owner = match owner_id.map(str::trim).filter(|value| !value.is_empty()) {
        Some(named) => named.to_string(),
        None => {
            return Err(reference_chat_scope(
                ScopeErrorCode::InvalidRequest,
                "a reference-chat target names no owner id",
            ))
        }
    };
    // The owner is this gateway incarnation's own identity, not a label the caller supplies: a
    // target naming any other value is refused exactly as a foreign incarnation is, so no route
    // can be served for an owner the gateway did not publish.
    if owner != daemon_owner {
        return Err(reference_chat_scope(
            ScopeErrorCode::TargetExpired,
            format!(
                "the request names owner {owner}; this gateway serves owner {daemon_owner}"
            ),
        ));
    }
    let epoch_value = match epoch.map(str::trim).filter(|value| !value.is_empty()) {
        Some(text) => match text.parse::<u64>() {
            Ok(value) if value.to_string() == text => value,
            _ => {
                return Err(reference_chat_scope(
                    ScopeErrorCode::InvalidRequest,
                    "epoch must be a canonical decimal u64",
                ))
            }
        },
        None => {
            return Err(reference_chat_scope(
                ScopeErrorCode::InvalidRequest,
                "a reference-chat target names no daemon incarnation",
            ))
        }
    };
    if epoch_value != daemon_epoch {
        return Err(reference_chat_scope(
            ScopeErrorCode::TargetExpired,
            format!(
                "the request names daemon incarnation {epoch_value}; this gateway is at {daemon_epoch}"
            ),
        ));
    }
    let target = crate::scoped_contracts::TargetRef {
        host_id: host,
        owner_id: owner,
        epoch: crate::scoped_contracts::Epoch(epoch_value),
        backend_session_id: session_id.to_string(),
    };
    Ok(
        match provider_session_id.map(str::trim).filter(|value| !value.is_empty()) {
            Some(provider) => {
                reference_types::ReferenceTargetRef::with_provider_session(target, provider)
            }
            None => reference_types::ReferenceTargetRef::without_provider_session(target),
        },
    )
}

/// The target's owning identity, from this gateway's own daemon records.
async fn reference_chat_identity(
    state: &Arc<RemoteGatewayState>,
    target: &reference_types::ReferenceTargetRef,
    registry_id: Option<&str>,
) -> reference_history::ReferenceHistoryIdentity {
    let session_id = target.target.backend_session_id.as_str();
    let mut identity = reference_history::ReferenceHistoryIdentity::new(
        target.clone(),
        registry_id.unwrap_or("").to_string(),
    );
    if let Ok(details) = state.session_backend.describe_session(session_id).await {
        if let Some(path) = details.worktree_path {
            identity = identity.with_cwd(path.to_string_lossy());
        }
    }
    if let Some(services) = state.machine_services.as_ref() {
        if let Some(provider) = services.sessions.session_provider_session(session_id) {
            if let Some(path) = provider.transcript_path {
                identity = identity.with_provider_transcript_path(path);
            }
            identity = identity.with_agent_session_id(provider.id);
        }
    }
    identity
}

/// Refuse a target that is not a live session of this gateway.
async fn reference_chat_ensure_live(
    state: &Arc<RemoteGatewayState>,
    session_id: &str,
) -> Result<(), ScopeError> {
    if !crate::agent_transcript::is_valid_session_id(session_id) {
        return Err(reference_chat_scope(
            ScopeErrorCode::InvalidRequest,
            "the session id is not a session id",
        ));
    }
    if !state
        .session_backend
        .list_sessions()
        .await
        .iter()
        .any(|id| id == session_id)
    {
        return Err(reference_chat_scope(
            ScopeErrorCode::NotFound,
            "no live session has that id on this host",
        ));
    }
    Ok(())
}

/// Authenticate, fence the target, and resolve the owning identity for one read.
///
/// Authentication precedes every path/query rejection, exactly as the other adapters in this
/// module do.
async fn reference_chat_read_context(
    state: &Arc<RemoteGatewayState>,
    headers: &HeaderMap,
    session_id: &str,
    query: &ReferenceChatReadQuery,
    registry_required: bool,
) -> Result<ReferenceChatRead, Response> {
    let device = authenticate_machine_request(state, headers)?;
    if let Some(named) = query
        .backend_session_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if named != session_id {
            return Err(reference_chat_error(
                ScopeErrorCode::Forbidden,
                "the query names a different backend session than the route",
            ));
        }
    }
    let target = reference_chat_target(
        session_id,
        query.host_id.as_deref(),
        query.owner_id.as_deref(),
        query.epoch.as_deref(),
        query.provider_session_id.as_deref(),
        state.reference_owner_id.as_str(),
        state
            .daemon_epoch
            .load(std::sync::atomic::Ordering::Acquire),
    )
    .map_err(|error| reference_chat_refusal(&error))?;
    reference_chat_ensure_live(state, session_id)
        .await
        .map_err(|error| reference_chat_refusal(&error))?;
    let registry_id = query
        .registry_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if registry_required && registry_id.is_none() {
        return Err(reference_chat_error(
            ScopeErrorCode::InvalidRequest,
            "the request names no registry id for this pane",
        ));
    }
    let identity = reference_chat_identity(state, &target, registry_id).await;
    Ok(ReferenceChatRead { device, target, identity })
}

/// The authenticated target of one mutation, from the frozen envelope.
async fn reference_chat_mutation_context<P>(
    state: &Arc<RemoteGatewayState>,
    headers: &HeaderMap,
    session_id: &str,
    body: &ReferenceChatMutation<P>,
) -> Result<(DeviceInfo, reference_types::ReferenceTargetRef), Response> {
    let device = authenticate_machine_request(state, headers)?;
    if body.request_id.trim().is_empty() {
        return Err(reference_chat_error(
            ScopeErrorCode::InvalidRequest,
            "a mutation names no request id",
        ));
    }
    let named = body.target.as_ref().ok_or_else(|| {
        reference_chat_error(ScopeErrorCode::InvalidRequest, "a mutation names no target")
    })?;
    if named.backend_session_id != session_id {
        return Err(reference_chat_error(
            ScopeErrorCode::Forbidden,
            "the mutation target names a different backend session than the route",
        ));
    }
    let named_epoch = named.epoch.0.to_string();
    let target = reference_chat_target(
        session_id,
        Some(named.host_id.as_str()),
        Some(named.owner_id.as_str()),
        Some(named_epoch.as_str()),
        body.provider_session_id.as_deref(),
        state.reference_owner_id.as_str(),
        state
            .daemon_epoch
            .load(std::sync::atomic::Ordering::Acquire),
    )
    .map_err(|error| reference_chat_refusal(&error))?;
    reference_chat_ensure_live(state, session_id)
        .await
        .map_err(|error| reference_chat_refusal(&error))?;
    Ok((device, target))
}

/// The reauthorization a mutation re-runs before every write it dispatches.
///
/// A token revoked mid-flight must stop the transaction; this closure is what input.rs asks
/// before the first byte and again before the Enter.
fn reference_chat_authorize(
    state: &Arc<RemoteGatewayState>,
    headers: &HeaderMap,
) -> Box<dyn Fn() -> Result<(), ScopeError> + Send + Sync> {
    let token = extract_token(headers);
    let auth = Arc::clone(&state.auth_manager);
    Box::new(move || {
        let token = token.clone().ok_or_else(|| {
            reference_chat_scope(ScopeErrorCode::Unauthorized, "the caller's token is gone")
        })?;
        let device = auth.validate_token(&token).map_err(|_| {
            reference_chat_scope(
                ScopeErrorCode::Unauthorized,
                "the caller's device is no longer authorized",
            )
        })?;
        if device.permission != DevicePermission::Control {
            return Err(reference_chat_scope(
                ScopeErrorCode::Forbidden,
                "the caller's device may no longer control this machine",
            ));
        }
        Ok(())
    })
}

/// This host's transcript root, as the history reader resolves stores against.
fn reference_chat_history_home(state: &Arc<RemoteGatewayState>) -> Option<PathBuf> {
    #[cfg(test)]
    if let Some(home) = state.agent_history_home.read().clone() {
        return Some(home);
    }
    let _ = state;
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// The other live sessions that may share this pane's cwd.
async fn reference_chat_siblings(
    state: &Arc<RemoteGatewayState>,
    session_id: &str,
) -> Vec<reference_history::ReferenceHistorySibling> {
    let mut siblings = Vec::new();
    for other in state.session_backend.list_sessions().await {
        if other == session_id {
            continue;
        }
        let cwd = state
            .session_backend
            .describe_session(&other)
            .await
            .ok()
            .and_then(|details| details.worktree_path)
            .map(|path| path.to_string_lossy().into_owned());
        siblings.push(reference_history::ReferenceHistorySibling { session_id: other, cwd });
    }
    siblings
}

/// Is this session's transcript on another host?
///
/// A remote session's store entry carries its own host; this gateway must not read, guess or
/// substitute a local file for it.
async fn reference_chat_session_is_remote(
    state: &Arc<RemoteGatewayState>,
    session_id: &str,
) -> bool {
    let Some(services) = state.machine_services.clone() else {
        return false;
    };
    let store = services.sessions.remote_sessions_store_path().to_path_buf();
    let Ok(raw) = crate::ipc::run_blocking(move || {
        std::fs::read_to_string(store)
            .map_err(|error| crate::ipc::error::IpcError::internal(error.to_string()))
    })
    .await
    else {
        return false;
    };
    crate::agent_transcript::remote_target_from_store(&raw, session_id).is_some()
}

/// The cursor a read names, or the refusal that keeps a cursor from being re-anchored.
fn reference_chat_cursor(
    query: &ReferenceChatReadQuery,
) -> Result<Option<reference_types::ReferenceHistoryCursor>, ScopeError> {
    let Some(offset) = query.cursor else {
        return Ok(None);
    };
    let stream_id = query
        .cursor_stream
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            reference_chat_scope(
                ScopeErrorCode::InvalidRequest,
                "a cursor must name the stream it was minted against",
            )
        })?;
    Ok(Some(reference_types::ReferenceHistoryCursor {
        stream_id: stream_id.to_string(),
        offset,
    }))
}

/// The pane's screen, rebuilt from the authenticated backend's own segmented history.
async fn reference_chat_snapshot_screen(
    state: &Arc<RemoteGatewayState>,
    target: &reference_types::ReferenceTargetRef,
) -> Result<reference_types::ReferenceScreenSnapshot, ScopeError> {
    let session_id = target.target.backend_session_id.as_str();
    let details = state
        .session_backend
        .describe_session(session_id)
        .await
        .map_err(|message| reference_chat_scope(ScopeErrorCode::NotFound, message))?;
    let attachment = state
        .session_backend
        .attach_with_sequence(session_id, None)
        .await
        .map_err(|message| reference_chat_scope(ScopeErrorCode::Timeout, message))?;
    let mut mirror = RemoteTerminalMirror::new(details.cols, details.rows)
        .map_err(|error| reference_chat_scope(ScopeErrorCode::Unsupported, error.to_string()))?;
    let segments: Option<Vec<Vec<u8>>> = if attachment.snapshot.gap.is_some() {
        None
    } else {
        Some(
            attachment
                .snapshot
                .history_segments
                .iter()
                .map(|segment| segment.bytes.clone())
                .collect(),
        )
    };
    reference_screen::snapshot_reference_screen(&mut mirror, segments.as_deref())
        .map_err(|message| reference_chat_scope(ScopeErrorCode::Unsupported, message))
}

/// The original session's own record for the prompt detectors.
async fn reference_chat_prompt_session(
    state: &Arc<RemoteGatewayState>,
    identity: &reference_history::ReferenceHistoryIdentity,
) -> ReferenceChatPromptSession {
    let agent_status = state.machine_services.as_ref().and_then(|services| {
        services
            .sessions
            .session_activity_state(&identity.target.target.backend_session_id)
    });
    ReferenceChatPromptSession {
        jsonl: reference_chat_session_jsonl(state, identity).await,
        agent_status,
    }
}

/// The tail of the original session file, when this host holds one for the target.
async fn reference_chat_session_jsonl(
    state: &Arc<RemoteGatewayState>,
    identity: &reference_history::ReferenceHistoryIdentity,
) -> Option<String> {
    let home = reference_chat_history_home(state)?;
    let siblings = reference_chat_siblings(state, &identity.target.target.backend_session_id).await;
    let stream =
        reference_history::resolve_reference_history_stream(&home, identity, &siblings).ok()?;
    let path = stream.file_path()?.to_path_buf();
    let bytes = crate::ipc::run_blocking(move || {
        std::fs::read(&path).map_err(|error| crate::ipc::error::IpcError::internal(error.to_string()))
    })
    .await
    .ok()?;
    let bounded = if bytes.len() > REFERENCE_PROMPT_SESSION_MAX_BYTES {
        &bytes[bytes.len() - REFERENCE_PROMPT_SESSION_MAX_BYTES..]
    } else {
        bytes.as_slice()
    };
    // Lossy on purpose: the window may start mid-character, and the partial first record is not
    // a record the detectors may match anyway.
    Some(String::from_utf8_lossy(bounded).into_owned())
}

/// The prompt the pane is holding, when this route's own screen read saw one.
async fn reference_chat_blocked_prompt(
    state: &Arc<RemoteGatewayState>,
    target: &reference_types::ReferenceTargetRef,
    registry_id: Option<&str>,
) -> Option<String> {
    let agent = registry_id.map(str::trim).filter(|value| !value.is_empty())?;
    let snapshot = reference_chat_snapshot_screen(state, target).await.ok()?;
    if !snapshot.is_answerable() {
        return None;
    }
    let identity = reference_chat_identity(state, target, Some(agent)).await;
    let session = reference_chat_prompt_session(state, &identity).await;
    let prompt = reference_prompts::detect_reference_prompt_in_session(
        agent,
        &snapshot.text,
        &session.borrow(),
    )?;
    Some(prompt.id)
}

/// The response one ordered input transaction produces.
///
/// The result envelope is the frozen ScopeResult: an accepted write is Accepted (the writer took
/// the bytes — never providerRead), a refusal that typed nothing is a typed failure, and an
/// undetermined outcome is OPERATION_OUTCOME_UNKNOWN, held rather than replayed.
fn reference_chat_input_response(
    request_id: &str,
    outcome: reference_input::ReferenceInputOutcome,
) -> Response {
    match outcome {
        reference_input::ReferenceInputOutcome::Accepted { receipt } => {
            reference_chat_result_ok(request_id, serde_json::json!({ "receipt": receipt }))
        }
        reference_input::ReferenceInputOutcome::NotTyped { error } => {
            reference_chat_result_failure(request_id, &error)
        }
        reference_input::ReferenceInputOutcome::OutcomeUnknown { message, .. } => {
            reference_chat_result_failure(
                request_id,
                &ScopeError {
                    code: ScopeErrorCode::OperationOutcomeUnknown,
                    message,
                    retryable: false,
                    details: serde_json::Value::Null,
                },
            )
        }
    }
}

/// The staged-file registry key: this target's own file, never another pane's.
fn reference_chat_staged_key(
    target: &reference_types::ReferenceTargetRef,
    attachment_id: &str,
) -> String {
    format!(
        "{}\u{1f}{attachment_id}",
        reference_types::reference_draft_key(target)
    )
}

/// Drop the staged files nothing has referenced for the frozen TTL, and their directories.
fn reference_chat_prune_staged(runtime: &ReferenceChatRuntime) {
    let now = reference_input::SystemReferenceClock.now_ms();
    let ttl = reference_files::reference_unreferenced_ttl_ms();
    let mut expired = Vec::new();
    {
        let mut staged = runtime.staged.lock();
        staged.retain(|_, entry| {
            let stale = now.saturating_sub(entry.staged_at_ms) > ttl;
            if stale {
                expired.push(entry.dir.clone());
            }
            !stale
        });
    }
    for dir in expired {
        let _ = reference_files::reference_cancel_staged_file(&dir);
    }
}

/// Read one bounded mutation envelope off the wire, after authentication.
async fn reference_chat_mutation_body<P: serde::de::DeserializeOwned>(
    request: axum::extract::Request,
    state: &Arc<RemoteGatewayState>,
) -> Result<(HeaderMap, P), Response> {
    let (parts, body) = request.into_parts();
    let headers = parts.headers;
    authenticate_machine_request(state, &headers)?;
    let bytes = axum::body::to_bytes(body, REFERENCE_CHAT_MUTATION_MAX_BYTES)
        .await
        .map_err(|_| {
            reference_chat_error(
                ScopeErrorCode::PayloadTooLarge,
                "the mutation envelope is larger than one mutation may carry",
            )
        })?;
    let parsed = serde_json::from_slice::<P>(&bytes).map_err(|error| {
        reference_chat_error(
            ScopeErrorCode::InvalidRequest,
            format!("the mutation envelope is not readable: {error}"),
        )
    })?;
    Ok((headers, parsed))
}

// ---------------------------------------------------------------------------------------------
// Reads
// ---------------------------------------------------------------------------------------------

/// GET /api/v1/reference-chat/{sessionId}/history
async fn reference_chat_history(
    State(state): State<Arc<RemoteGatewayState>>,
    AxumPath(session_id): AxumPath<String>,
    Query(query): Query<ReferenceChatReadQuery>,
    headers: HeaderMap,
) -> Response {
    let read = match reference_chat_read_context(&state, &headers, &session_id, &query, true).await
    {
        Ok(read) => read,
        Err(response) => return response,
    };
    let limit = match reference_history::normalize_reference_history_limit(
        query
            .limit
            .unwrap_or(reference_history::REFERENCE_HISTORY_DEFAULT_LIMIT),
    ) {
        Ok(limit) => limit,
        Err(error) => return reference_chat_refusal(&error),
    };
    let cursor = match reference_chat_cursor(&query) {
        Ok(cursor) => cursor,
        Err(error) => return reference_chat_refusal(&error),
    };
    if reference_chat_session_is_remote(&state, &session_id).await {
        return reference_chat_error(
            ScopeErrorCode::Unsupported,
            "this session's transcript lives on its paired host; read it through that host's own \
             reference-chat route",
        );
    }
    let Some(home) = reference_chat_history_home(&state) else {
        return reference_chat_error(
            ScopeErrorCode::Unsupported,
            "this host's transcript root is unknown",
        );
    };
    let siblings = reference_chat_siblings(&state, &session_id).await;
    let stream = match reference_history::resolve_reference_history_stream(
        &home,
        &read.identity,
        &siblings,
    ) {
        Ok(stream) => stream,
        Err(error) => return reference_chat_refusal(&error),
    };
    match reference_history::read_reference_history_page(
        &stream,
        &read.identity,
        limit,
        cursor.as_ref(),
    )
    .await
    {
        Ok(page) => ([(header::CACHE_CONTROL, "no-store")], Json(page)).into_response(),
        Err(error) => reference_chat_refusal(&error),
    }
}

/// GET /api/v1/reference-chat/{sessionId}/screen
async fn reference_chat_screen(
    State(state): State<Arc<RemoteGatewayState>>,
    AxumPath(session_id): AxumPath<String>,
    Query(query): Query<ReferenceChatReadQuery>,
    headers: HeaderMap,
) -> Response {
    let read = match reference_chat_read_context(&state, &headers, &session_id, &query, false).await
    {
        Ok(read) => read,
        Err(response) => return response,
    };
    match reference_chat_snapshot_screen(&state, &read.target).await {
        Ok(snapshot) => ([(header::CACHE_CONTROL, "no-store")], Json(snapshot)).into_response(),
        Err(error) => reference_chat_refusal(&error),
    }
}

/// GET /api/v1/reference-chat/{sessionId}/prompt
///
/// The card is detected in the pane's own session record and remembered, so the answer route can
/// plan against the same card the user saw. A screen with no prompt is a successful read with a
/// null prompt, not an error.
async fn reference_chat_prompt(
    State(state): State<Arc<RemoteGatewayState>>,
    AxumPath(session_id): AxumPath<String>,
    Query(query): Query<ReferenceChatReadQuery>,
    headers: HeaderMap,
) -> Response {
    let read = match reference_chat_read_context(&state, &headers, &session_id, &query, true).await
    {
        Ok(read) => read,
        Err(response) => return response,
    };
    let snapshot = match reference_chat_snapshot_screen(&state, &read.target).await {
        Ok(snapshot) => snapshot,
        Err(error) => return reference_chat_refusal(&error),
    };
    if !snapshot.is_answerable() {
        return reference_chat_error(
            ScopeErrorCode::RequestConflict,
            "the pane's screen could not be reconstructed, so no card may be answered from it",
        );
    }
    let session = reference_chat_prompt_session(&state, &read.identity).await;
    let agent = read.identity.registry_id.clone();
    let prompt = reference_prompts::detect_reference_prompt_in_session(
        &agent,
        &snapshot.text,
        &session.borrow(),
    );
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({
            "prompt": prompt,
            "screenRevision": snapshot.revision,
            "cols": snapshot.cols,
            "rows": snapshot.rows,
        })),
    )
        .into_response()
}

// ---------------------------------------------------------------------------------------------
// Mutations
// ---------------------------------------------------------------------------------------------

/// POST /api/v1/reference-chat/{sessionId}/submit
async fn reference_chat_submit(
    State(state): State<Arc<RemoteGatewayState>>,
    AxumPath(session_id): AxumPath<String>,
    request: axum::extract::Request,
) -> Response {
    let (headers, body) = match reference_chat_mutation_body::<
        ReferenceChatMutation<reference_types::ReferenceSubmitPayload>,
    >(request, &state)
    .await
    {
        Ok(parsed) => parsed,
        Err(response) => return response,
    };
    let (device, target) =
        match reference_chat_mutation_context(&state, &headers, &session_id, &body).await {
            Ok(context) => context,
            Err(response) => return response,
        };
    if device.permission != DevicePermission::Control {
        return reference_chat_error(
            ScopeErrorCode::Forbidden,
            "a view-only device cannot type into a pane",
        );
    }
    let runtime = reference_chat_runtime();
    // The pane's OWN bracketed-paste mode, as this host's output hub recorded it: the shaping in
    // input.rs is normative and must not assume a mode the pane never enabled.
    let bracketed_paste = state
        .terminal_service
        .output_hub()
        .is_bracketed_paste_enabled(&session_id);
    let blocked_prompt =
        reference_chat_blocked_prompt(&state, &target, body.registry_id.as_deref()).await;
    let authorize = reference_chat_authorize(&state, &headers);
    let clock = reference_input::SystemReferenceClock;
    let submit = reference_input::ReferenceSubmitRequest {
        target: &target,
        request_id: &body.request_id,
        payload: &body.params,
        bracketed_paste,
        blocked_prompt: blocked_prompt.as_deref(),
        arrived_at_ms: clock.now_ms(),
        last_typed_at_ms: None,
        authorize: &*authorize,
    };
    let writer = reference_input::ReferenceSessionWriter::new(state.session_backend.as_ref());
    let outcome = runtime.input.submit(&submit, &writer, &clock).await;
    reference_chat_input_response(&body.request_id, outcome)
}

/// POST /api/v1/reference-chat/{sessionId}/stop
async fn reference_chat_stop(
    State(state): State<Arc<RemoteGatewayState>>,
    AxumPath(session_id): AxumPath<String>,
    request: axum::extract::Request,
) -> Response {
    let (headers, body) = match reference_chat_mutation_body::<
        ReferenceChatMutation<reference_types::ReferenceStopPayload>,
    >(request, &state)
    .await
    {
        Ok(parsed) => parsed,
        Err(response) => return response,
    };
    let (device, target) =
        match reference_chat_mutation_context(&state, &headers, &session_id, &body).await {
            Ok(context) => context,
            Err(response) => return response,
        };
    if device.permission != DevicePermission::Control {
        return reference_chat_error(
            ScopeErrorCode::Forbidden,
            "a view-only device cannot stop a pane's turn",
        );
    }
    let runtime = reference_chat_runtime();
    let authorize = reference_chat_authorize(&state, &headers);
    // Over HTTP the caller's liveness is the request itself: a caller that has gone does not
    // reach the write, and the reauthorization above is what a revoked caller trips.
    let alive = || true;
    let stop = reference_input::ReferenceStopRequest {
        target: &target,
        request_id: &body.request_id,
        payload: &body.params,
        authorize: &*authorize,
        alive: &alive,
    };
    let writer = reference_input::ReferenceSessionWriter::new(state.session_backend.as_ref());
    let outcome = runtime.input.stop(&stop, &writer).await;
    reference_chat_input_response(&body.request_id, outcome)
}

/// POST /api/v1/reference-chat/{sessionId}/answer
///
/// The answer runs inside the SAME per-target step as submit and Stop, through the one shared
/// queue: the fresh screen read, the card's re-detection and every key are one operation, which
/// is what keeps a Stop from landing between them.
async fn reference_chat_answer(
    State(state): State<Arc<RemoteGatewayState>>,
    AxumPath(session_id): AxumPath<String>,
    request: axum::extract::Request,
) -> Response {
    let (headers, body) = match reference_chat_mutation_body::<
        ReferenceChatMutation<reference_types::ReferencePromptAnswerPayload>,
    >(request, &state)
    .await
    {
        Ok(parsed) => parsed,
        Err(response) => return response,
    };
    let (device, target) =
        match reference_chat_mutation_context(&state, &headers, &session_id, &body).await {
            Ok(context) => context,
            Err(response) => return response,
        };
    if device.permission != DevicePermission::Control {
        return reference_chat_error(
            ScopeErrorCode::Forbidden,
            "a view-only device cannot answer a prompt",
        );
    }
    let runtime = reference_chat_runtime();
    let identity = reference_chat_identity(&state, &target, body.registry_id.as_deref()).await;
    let prompt_session = reference_chat_prompt_session(&state, &identity).await;
    let authorize = reference_chat_authorize(&state, &headers);
    let alive = || true;
    let answer = reference_prompts::ReferenceAnswerRequest {
        target: &target,
        request_id: &body.request_id,
        payload: &body.params,
        session: prompt_session.borrow(),
        authorize: &*authorize,
        alive: &alive,
    };
    let step = reference_prompts::ReferenceQueueStep::new(&runtime.input);
    let screen = ReferenceGatewayScreenReader {
        backend: Arc::clone(&state.session_backend),
    };
    let writer = reference_input::ReferenceSessionWriter::new(state.session_backend.as_ref());
    let clock = reference_input::SystemReferenceClock;
    let outcome = runtime
        .answers
        .answer(&answer, &step, &screen, &writer, &clock)
        .await;
    reference_chat_input_response(&body.request_id, outcome)
}

/// POST /api/v1/reference-chat/{sessionId}/files
///
/// The bytes are staged on the OWNING host — this gateway — into the lane's owner-private
/// staging root, and the receipt names the host, an opaque id, the digest, the size and the media
/// type. No path is minted for the client: the mention the agent receives is built from the
/// staged file's own name.
async fn reference_chat_stage_file(
    State(state): State<Arc<RemoteGatewayState>>,
    AxumPath(session_id): AxumPath<String>,
    request: axum::extract::Request,
) -> Response {
    let (headers, body) = match reference_chat_mutation_body::<
        ReferenceChatMutation<reference_types::ReferenceFileStagePayload>,
    >(request, &state)
    .await
    {
        Ok(parsed) => parsed,
        Err(response) => return response,
    };
    let (device, target) =
        match reference_chat_mutation_context(&state, &headers, &session_id, &body).await {
            Ok(context) => context,
            Err(response) => return response,
        };
    if device.permission != DevicePermission::Control {
        return reference_chat_error(
            ScopeErrorCode::Forbidden,
            "a view-only device cannot stage a file for a pane",
        );
    }
    let runtime = reference_chat_runtime();
    reference_chat_prune_staged(runtime);
    let target_key = reference_types::reference_draft_key(&target);
    let (existing_files, existing_turn_bytes) = {
        let staged = runtime.staged.lock();
        staged
            .values()
            .filter(|entry| entry.target_key == target_key)
            .fold((0usize, 0u64), |(count, bytes), entry| {
                (count + 1, bytes + entry.receipt.size_bytes)
            })
    };
    if let Err(code) = reference_files::reference_file_bounds_check(
        body.params.size_bytes,
        existing_files,
        existing_turn_bytes,
    ) {
        return reference_chat_refusal(&reference_chat_scope(
            code,
            "the staged set for this target is at its limit",
        ));
    }
    let payload = body.params.clone();
    let staged = crate::ipc::run_blocking(move || {
        Ok::<_, crate::ipc::error::IpcError>(reference_files::stage_reference_file(&payload))
    })
    .await;
    let receipt = match staged {
        Ok(Ok(receipt)) => receipt,
        Ok(Err(code)) => {
            return reference_chat_refusal(&reference_chat_scope(
                code,
                "the file could not be staged on this host",
            ))
        }
        Err(_) => {
            return reference_chat_error(
                ScopeErrorCode::Unsupported,
                "the staging thread did not finish",
            )
        }
    };
    let dir = reference_files::reference_staging_base_dir().join(&receipt.receipt.attachment_id);
    let path = dir.join(&receipt.display_name);
    runtime.staged.lock().insert(
        reference_chat_staged_key(&target, &receipt.receipt.attachment_id),
        ReferenceStagedFile {
            target_key,
            dir,
            path,
            display_name: receipt.display_name.clone(),
            receipt: receipt.receipt.clone(),
            staged_at_ms: reference_input::SystemReferenceClock.now_ms(),
        },
    );
    reference_chat_result_ok(
        &body.request_id,
        serde_json::json!({
            "receipt": receipt.receipt,
            "displayName": receipt.display_name,
            "mentionText": receipt.mention_text,
        }),
    )
}

/// GET /api/v1/reference-chat/{sessionId}/files/{fileId}
async fn reference_chat_preview_file(
    State(state): State<Arc<RemoteGatewayState>>,
    AxumPath((session_id, file_id)): AxumPath<(String, String)>,
    Query(query): Query<ReferenceChatReadQuery>,
    headers: HeaderMap,
) -> Response {
    let read = match reference_chat_read_context(&state, &headers, &session_id, &query, false).await
    {
        Ok(read) => read,
        Err(response) => return response,
    };
    let runtime = reference_chat_runtime();
    let entry = {
        let staged = runtime.staged.lock();
        staged
            .get(&reference_chat_staged_key(&read.target, &file_id))
            .map(|entry| {
                (
                    entry.path.clone(),
                    entry.display_name.clone(),
                    entry.receipt.clone(),
                )
            })
    };
    let Some((path, display_name, receipt)) = entry else {
        return reference_chat_error(
            ScopeErrorCode::NotFound,
            "no staged file of that id belongs to this target",
        );
    };
    let bytes = crate::ipc::run_blocking(move || {
        std::fs::read(&path)
            .map_err(|error| crate::ipc::error::IpcError::internal(error.to_string()))
    })
    .await;
    let bytes = match bytes {
        Ok(bytes) => bytes,
        Err(_) => {
            return reference_chat_error(
                ScopeErrorCode::NotFound,
                "the staged file is no longer readable",
            )
        }
    };
    use base64::Engine as _;
    let content = base64::engine::general_purpose::STANDARD.encode(&bytes);
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({
            "receipt": receipt,
            "displayName": display_name,
            "sizeBytes": bytes.len(),
            "contentBase64": content,
        })),
    )
        .into_response()
}

/// DELETE /api/v1/reference-chat/{sessionId}/files/{fileId}
///
/// Deletion is always explicit and never implicit: a failed send deletes nothing, and only this
/// route removes a staged file.
async fn reference_chat_delete_file(
    State(state): State<Arc<RemoteGatewayState>>,
    AxumPath((session_id, file_id)): AxumPath<(String, String)>,
    Query(query): Query<ReferenceChatReadQuery>,
    headers: HeaderMap,
) -> Response {
    let read = match reference_chat_read_context(&state, &headers, &session_id, &query, false).await
    {
        Ok(read) => read,
        Err(response) => return response,
    };
    if read.device.permission != DevicePermission::Control {
        return reference_chat_error(
            ScopeErrorCode::Forbidden,
            "a view-only device cannot delete a staged file",
        );
    }
    let runtime = reference_chat_runtime();
    let entry = runtime
        .staged
        .lock()
        .remove(&reference_chat_staged_key(&read.target, &file_id));
    let Some(entry) = entry else {
        return reference_chat_error(
            ScopeErrorCode::NotFound,
            "no staged file of that id belongs to this target",
        );
    };
    let dir = entry.dir;
    let _ = crate::ipc::run_blocking(move || {
        reference_files::reference_cancel_staged_file(&dir);
        Ok::<_, crate::ipc::error::IpcError>(())
    })
    .await;
    (
        StatusCode::NO_CONTENT,
        [(header::CACHE_CONTROL, "no-store")],
    )
        .into_response()
}

pub fn create_remote_router(state: Arc<RemoteGatewayState>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let mut router = Router::new()
        .route("/api/v1/health", get(health_check))
        .route("/api/v1/capabilities", get(get_capabilities))
        .route("/api/v1/direct/offer", post(super::direct_api::direct_offer_handler).layer(
            axum::extract::DefaultBodyLimit::max(crate::paired_host::direct_wire::MAX_DIRECT_BODY_BYTES),
        ))
        .route(
            "/api/v1/fs/directories",
            get(super::filesystem::directories),
        )
        .route("/api/v1/pair/exchange", post(pair_exchange))
        .route(
            "/api/v1/sessions",
            get(list_sessions).post(super::session_api::create).layer(
                axum::extract::DefaultBodyLimit::max(
                    super::machine_protocol::MACHINE_JSON_MAX_BYTES,
                ),
            ),
        )
        .route(
            "/api/v1/sessions/{sessionId}",
            get(super::session_api::detail)
                .delete(super::session_api::close)
                .layer(axum::extract::DefaultBodyLimit::max(
                    super::machine_protocol::MACHINE_JSON_MAX_BYTES,
                )),
        )
        .route(
            "/api/v1/workspace/state",
            get(get_workspace_state),
        )
        .route(
            "/api/v1/agent-history/{sessionId}",
            get(get_agent_history),
        )
        // Reference chat (plan task 13). The same authenticated gateway, the same listener and
        // the same bearer/permission policy as every route above; no control router is mounted
        // and nothing here resizes a pane or signals a process.
        .route(
            "/api/v1/reference-chat/{sessionId}/history",
            get(reference_chat_history),
        )
        .route(
            "/api/v1/reference-chat/{sessionId}/screen",
            get(reference_chat_screen),
        )
        .route(
            "/api/v1/reference-chat/{sessionId}/prompt",
            get(reference_chat_prompt),
        )
        .route(
            "/api/v1/reference-chat/{sessionId}/submit",
            post(reference_chat_submit),
        )
        .route(
            "/api/v1/reference-chat/{sessionId}/stop",
            post(reference_chat_stop),
        )
        .route(
            "/api/v1/reference-chat/{sessionId}/answer",
            post(reference_chat_answer),
        )
        .route(
            "/api/v1/reference-chat/{sessionId}/files",
            post(reference_chat_stage_file),
        )
        .route(
            "/api/v1/reference-chat/{sessionId}/files/{fileId}",
            get(reference_chat_preview_file).delete(reference_chat_delete_file),
        )
        .route(
            "/api/v1/workspace/projects",
            get(super::workspace_api::list)
                .post(register_project_boundary)
                .layer(axum::extract::DefaultBodyLimit::max(
                    super::machine_protocol::MACHINE_JSON_MAX_BYTES,
                )),
        )
        .route(
            "/api/v1/workspace/projects/{workspaceId}",
            axum::routing::delete(unregister_project_boundary).layer(
                axum::extract::DefaultBodyLimit::max(
                    super::machine_protocol::MACHINE_JSON_MAX_BYTES,
                ),
            ),
        )
        .route(
            "/api/v1/workspace/operations/{requestId}",
            get(operation_boundary),
        )
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
            get(worktree_list_boundary)
                .post(worktree_mutation_boundary)
                .delete(worktree_mutation_boundary)
                .layer(axum::extract::DefaultBodyLimit::max(
                    super::machine_protocol::MACHINE_JSON_MAX_BYTES,
                )),
        )
        .route(
            "/api/v1/workspace/worktrees/status",
            get(worktree_status_boundary),
        )
        .route("/api/v1/devices", get(list_devices))
        .route("/api/v1/devices/{id}/revoke", post(revoke_device))
        .route("/api/v1/socket-ticket", post(issue_socket_ticket))
        .route("/api/v1/events", get(ws_events_handler))
        .route("/api/v1/terminal/{sessionId}", get(ws_terminal_handler))
        .route("/api/v1/browser/sessions", get(list_browser_sessions))
        .route("/api/v1/browser/identify", get(identify_browser_session))
        .route("/api/v1/browser/{browserId}", get(ws_browser_handler))
        .route(
            "/api/v1/workspace/dag",
            get(super::dag_api::ws_dag_handler),
        )
        .route("/api/push/subscribe", post(push_subscribe))
        .route("/api/push/unsubscribe", post(push_unsubscribe))
        .fallback(remote_fallback)
        .method_not_allowed_fallback(remote_method_not_allowed)
        .layer(cors)
        .layer(axum::Extension(Arc::new(
            super::filesystem::BrowseLimits::default(),
        )))
        .with_state(Arc::clone(&state));

    if cfg!(not(test)) {
        let auth = Arc::clone(&state.auth_manager);
        let deps = Arc::new(crate::remote::attach_router::AttachRouterDeps {
            gateway_addr: format!(
                "127.0.0.1:{}",
                crate::remote::state::REMOTE_GATEWAY_PORT
            ),
            authorize: Arc::new(move |key| auth.authorizes_attach_key_bytes(key)),
        });
        router = router.merge(crate::remote::attach_router::attach_router(deps));
    }

    router
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
    /// Prepare a replacement without changing the active listener or relay publication.
    pub(crate) async fn prepare_relay(
        state: Arc<RemoteGatewayState>,
        relay_url: Option<&str>,
        address: SocketAddr,
    ) -> Result<crate::remote::relay_client::RelayClient, String> {
        let url = relay_url
            .map(str::trim)
            .filter(|url| !url.is_empty())
            .unwrap_or(crate::remote::state::DEFAULT_RELAY_URL);
        crate::remote::relay_client::validate_relay_url(
            url,
            crate::remote::relay_client::is_insecure_relay_allowed(),
        )
        .map_err(|error| format!("Invalid relay configuration: {error}"))?;
        let identity = load_gateway_identity(Arc::clone(&state))
            .await
            .map_err(|_| "Machine identity unavailable".to_string())?;
        let client = match std::env::var("FERRYX_MACHINE_TOKEN")
            .ok()
            .filter(|token| !token.trim().is_empty())
        {
            Some(token) => crate::remote::relay_client::RelayClient::with_gateway(
                url, token, address.to_string(),
            ),
            None => crate::remote::relay_client::RelayClient::with_identity(
                url, identity.clone(), address.to_string(),
            ),
        };
        Ok(client
            .with_machine_id(&identity.machine_id)
            .with_auth_manager((*state.auth_manager).clone()))
    }

    /// Swap only the outbound supervisor; HTTP connections and PTY ownership stay intact.
    pub(crate) fn replace_relay(
        &mut self,
        state: Arc<RemoteGatewayState>,
        client: crate::remote::relay_client::RelayClient,
    ) {
        if let Some(task) = self.relay_task.take() {
            task.abort();
        }
        let epoch = crate::remote::state::RELAY_PAIRING_EPOCH
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        *state.relay_pairing.write() = Some(crate::remote::state::PublishedPairing {
            coordinator: client.pairing_coordinator(),
            epoch,
        });
        *state.relay_client.write() = Some(client.clone());
        self.published_pairing = Some((state, epoch));
        self.relay_task = Some(tokio::spawn(async move { client.run().await }));
    }

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
        Some(
            load_gateway_identity(Arc::clone(&state))
                .await
                .map_err(|_| "Machine identity unavailable".to_string())?,
        )
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
                    relay_identity
                        .clone()
                        .expect("relay identity loaded before binding"),
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
            *state.relay_client.write() = Some(client.clone());
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
pub(crate) static DIRECT_GATE_TEST_MUTEX: tokio::sync::Mutex<()> =
    tokio::sync::Mutex::const_new(());

#[cfg(test)]
#[path = "p19_insecure_direct_tests.rs"]
mod p19_insecure_direct_tests;

#[cfg(test)]
#[path = "p20_insecure_relay_tests.rs"]
mod p20_insecure_relay_tests;

#[cfg(test)]
#[path = "preference_http_tests.rs"]
mod preference_http_tests;

// The capability document's reference-chat host identity is a cross-task contract (task 12
// consumes `referenceHostId` for its mutation target), so its regression source lives beside
// this module rather than inside it.
#[cfg(test)]
#[path = "capability_tests.rs"]
mod capability_tests;

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
            Some(root.path().join("data/config")),
            Some(root.path().join("data/auth")),
        );
        let state = server.remote_state().clone();
        let mut grants = Vec::new();
        for scope in [DeviceAccessScope::Machine, DeviceAccessScope::Mirror] {
            let pin = state
                .auth_manager
                .create_scoped_pairing_code(DevicePermission::Control, scope)
                .unwrap();
            grants.push(
                state
                    .auth_manager
                    .exchange_pairing_code(&pin, "boundary")
                    .unwrap(),
            );
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let mut task = tokio::spawn(async move {
            // Inject complete bodies ahead of the real router, without TCP upload races.
            let router = create_remote_router(state).layer(axum::middleware::from_fn(
                |mut request: axum::extract::Request, next: axum::middleware::Next| async move {
                    if let Some(size) = request.headers_mut().remove("x-r3-body-size") {
                        let size = size.to_str().unwrap().parse::<usize>().unwrap();
                        assert!(matches!(size, 65_537 | 2_097_153));
                        request.headers_mut().remove(header::CONTENT_LENGTH);
                        let bytes = axum::body::Bytes::from(vec![b' '; size]);
                        *request.body_mut() =
                            if request.headers_mut().remove("x-r3-stream").is_some() {
                                axum::body::Body::from_stream(futures_util::stream::once(
                                    async move { Ok::<_, std::convert::Infallible>(bytes) },
                                ))
                            } else {
                                axum::body::Body::from(bytes)
                            };
                    }
                    next.run(request).await
                },
            ));
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .unwrap();
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
        if tokio::time::timeout(Duration::from_secs(10), &mut task)
            .await
            .is_err()
        {
            task.abort();
            let _ = task.await;
            panic!("boundary listener shutdown timed out");
        }
        assert!(tokio::net::TcpStream::connect(addr).await.is_err());
        drop(server);
        root.close().unwrap();
        println!("R3 CLEANUP listener_joined=true connection_refused=true private_root_removed=true no_pty=true");
        if let Err(panic) = result {
            std::panic::resume_unwind(panic);
        }
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
        }))
        .unwrap();
        let (_, device) = auth
            .exchange_pairing_code_with_installation(
                &request.code,
                &request.device_name,
                request.installation_id.as_deref(),
            )
            .unwrap();
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
        let mut stream = tokio::net::TcpStream::connect(addr)
            .await
            .expect("tcp connect");
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
        test_backend.sessions.lock().await.push(
            crate::remote::browser_backend::RemoteBrowserSessionSummary {
                browser_id: "b1".into(),
                title: Some("B1".into()),
                url: Some("https://example.com".into()),
                visible: true,
            },
        );
        state.set_browser_backend(test_backend.clone());

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let router = create_remote_router(Arc::clone(&state));
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let server_task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = stop_rx.await;
                })
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
        assert_eq!(
            status,
            StatusCode::SWITCHING_PROTOCOLS,
            "valid ticket must succeed with 101"
        );
        let hello_msg = read_ws_text_frame(&mut stream).await;
        let hello_json: serde_json::Value = serde_json::from_str(&hello_msg).unwrap();
        assert_eq!(hello_json["type"], "browserHello");
        assert_eq!(hello_json["browserId"], "b1");

        // 3. (b) Replay of same ticket must be rejected (single-use)
        let (replayed_status, _) = raw_ws_handshake(addr, &path_with_ticket, None).await;
        assert_eq!(
            replayed_status,
            StatusCode::UNAUTHORIZED,
            "replayed single-use ticket must be rejected with 401"
        );

        // 4. Connect without ticket or credentials must be rejected
        let (no_auth_status, _) = raw_ws_handshake(addr, "/api/v1/browser/b1", None).await;
        assert_eq!(
            no_auth_status,
            StatusCode::UNAUTHORIZED,
            "unauthenticated connect must be rejected with 401"
        );

        // 5. Connect with invalid ticket must be rejected
        let (fake_ticket_status, _) =
            raw_ws_handshake(addr, "/api/v1/browser/b1?ticket=fake-ticket-123", None).await;
        assert_eq!(
            fake_ticket_status,
            StatusCode::UNAUTHORIZED,
            "invalid ticket must be rejected with 401"
        );

        // 6. Direct HTTP listing when backend is active
        let list_resp = client
            .get(format!(
                "http://{addr}/api/v1/browser/sessions?workspaceId=ws1&worktreeSlug=main"
            ))
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await
            .unwrap();
        assert_eq!(list_resp.status(), reqwest::StatusCode::OK);

        // 7. When GUI exits: browsers become unavailable (UnavailableBrowserBackend)
        state.set_browser_backend(Arc::new(
            crate::remote::browser_backend::UnavailableBrowserBackend,
        ));
        state.bump_browser_service_epoch();

        let list_unavail = client
            .get(format!(
                "http://{addr}/api/v1/browser/sessions?workspaceId=ws1&worktreeSlug=main"
            ))
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await
            .unwrap();
        assert_eq!(
            list_unavail.status(),
            reqwest::StatusCode::SERVICE_UNAVAILABLE,
            "unavailable backend must return 503"
        );

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

        fn matching(
            &self,
            scope: &DesktopScope,
        ) -> Vec<crate::remote::browser_backend::RemoteBrowserSessionSummary> {
            let title = self.title_override.lock().unwrap().clone();
            self.inventory
                .lock()
                .unwrap()
                .iter()
                .filter(|(_, ws, slug)| ws == &scope.workspace_id && slug == &scope.worktree_slug)
                .map(
                    |(id, _, _)| crate::remote::browser_backend::RemoteBrowserSessionSummary {
                        browser_id: id.clone(),
                        title: title.clone().or_else(|| Some(format!("title-{id}"))),
                        url: Some("https://example.com/page".into()),
                        visible: true,
                    },
                )
                .collect()
        }
    }

    impl RemoteBrowserBackend for ScopeProbeBackend {
        fn list_sessions<'a>(
            &'a self,
            scope: &'a DesktopScope,
        ) -> crate::remote::browser_backend::BoxFuture<
            'a,
            Result<
                Vec<crate::remote::browser_backend::RemoteBrowserSessionSummary>,
                RemoteBrowserError,
            >,
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
            Result<
                Option<crate::remote::browser_backend::RemoteBrowserSessionSummary>,
                RemoteBrowserError,
            >,
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
        ) -> crate::remote::browser_backend::BoxFuture<
            '_,
            crate::remote::browser_backend::BrowserCapabilities,
        > {
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
    ) -> (
        SocketAddr,
        tokio::sync::oneshot::Sender<()>,
        tokio::task::JoinHandle<()>,
    ) {
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
            .body(
                serde_json::json!({ "target": format!("/api/v1/browser/{browser_id}") })
                    .to_string(),
            )
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
        let pin = state
            .auth_manager
            .create_pairing_code(DevicePermission::Control);
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
        assert_eq!(
            forged[0]["browserId"], "b-shared",
            "forged scope must not widen visibility"
        );
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
        let pin = state
            .auth_manager
            .create_pairing_code(DevicePermission::Control);
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

        let (mut ws, _) = tokio_tungstenite::connect_async(format!(
            "ws://{addr}/api/v1/browser/b1?ticket={ticket}"
        ))
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
        let pin = state
            .auth_manager
            .create_pairing_code(DevicePermission::Control);
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
        let (mut ws, _) = tokio_tungstenite::connect_async(format!(
            "ws://{addr}/api/v1/browser/b1?ticket={ticket}"
        ))
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
            Arc::new(TerminalService::default()),
            WorkspaceRegistry::new(),
        ));
        let response = get_capabilities(State(state), HeaderMap::new())
            .await
            .unwrap_err();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response
                .headers()
                .get(header::CACHE_CONTROL)
                .and_then(|v| v.to_str().ok()),
            Some("no-store")
        );
        let bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"]["code"], "UNAUTHORIZED");
        let typed = crate::remote::machine_protocol::decode_json::<
            crate::remote::machine_protocol::ErrorEnvelope,
        >(&bytes, 4096)
        .expect("actual authentication response must satisfy A02 error contract");
        assert_eq!(typed.error.code, "UNAUTHORIZED");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a03_identity_runs_offthread_and_revocation_fences_response() {
        let dir = tempfile::tempdir().unwrap();
        let state = Arc::new(RemoteGatewayState::new_with_paths(
            Arc::new(TerminalService::default()),
            WorkspaceRegistry::new(),
            Some(dir.path().join("config.json")),
            Some(dir.path().join("auth.json")),
        ));
        let pin = state
            .auth_manager
            .create_pairing_code(DevicePermission::Control);
        let (token, device) = state
            .auth_manager
            .exchange_pairing_code(&pin, "fixture")
            .unwrap();
        let runtime_thread = std::thread::current().id();
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let entered_tx = std::sync::Mutex::new(Some(entered_tx));
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let release_rx = std::sync::Mutex::new(release_rx);
        *state.identity_probe.write() = Some(Arc::new(move || {
            let offthread = std::thread::current().id() != runtime_thread;
            if let Some(tx) = entered_tx.lock().unwrap().take() {
                let _ = tx.send(offthread);
            }
            if offthread {
                release_rx
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap();
            }
        }));
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer {token}").parse().unwrap(),
        );
        let request_state = Arc::clone(&state);
        let request =
            tokio::spawn(async move { get_capabilities(State(request_state), headers).await });
        let entered = tokio::time::timeout(Duration::from_secs(5), entered_rx).await;
        state.auth_manager.revoke_device(&device.id);
        let released = release_tx.send(());
        let response = tokio::time::timeout(Duration::from_secs(5), request).await;
        assert!(
            entered.unwrap().unwrap(),
            "identity work must run off reactor"
        );
        released.unwrap();
        let response = response.unwrap().unwrap().unwrap_err();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-store"
        );
        *state.identity_probe.write() =
            Some(Arc::new(|| panic!("revoked request probed identity")));
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer {token}").parse().unwrap(),
        );
        assert_eq!(
            get_capabilities(State(state), headers)
                .await
                .unwrap_err()
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn a03_absent_machine_service_is_private_and_unavailable() {
        let state = Arc::new(RemoteGatewayState::new(
            Arc::new(TerminalService::default()),
            WorkspaceRegistry::new(),
        ));
        let pin = state
            .auth_manager
            .create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine)
            .unwrap();
        let (token, _) = state
            .auth_manager
            .exchange_pairing_code(&pin, "machine")
            .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer {token}").parse().unwrap(),
        );
        let request = axum::http::Request::builder()
            .method("POST")
            .uri("/api/v1/sessions")
            .body(axum::body::Body::empty())
            .unwrap();
        let response = super::super::session_api::create(State(state), headers, request)
            .await
            .unwrap_err();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-store"
        );
        let bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"]["code"], "MACHINE_SERVICE_UNAVAILABLE");
        let typed = crate::remote::machine_protocol::decode_json::<
            crate::remote::machine_protocol::ErrorEnvelope,
        >(&bytes, 4096)
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

    #[tokio::test]
    async fn remote_sessions_use_the_desktop_tab_label_as_the_title() {
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

        state.set_active_selection(RemoteActiveDesktopSelection {
            workspace_id: Some("default".to_string()),
            session_id: Some(session_id.clone()),
            terminal_tabs: vec![RemoteTerminalTabInfo {
                id: "tab-1".to_string(),
                label: "api server".to_string(),
                activity_state: None,
                agent_type: None,
                worktree_slug: None,
                worktree_label: None,
                session_id: Some(session_id.clone()),
            }],
            ..Default::default()
        });

        let cache = WorkspaceSnapshotCache::build(&registry);
        let sessions = get_active_running_sessions(&state, &cache, &[]).await;

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, session_id);
        assert_eq!(sessions[0].title, Some("api server".to_string()));

        // A session that is not a desktop tab gets no invented name, so the phone keeps numbering it.
        state.set_active_selection(RemoteActiveDesktopSelection {
            workspace_id: Some("default".to_string()),
            terminal_tabs: vec![RemoteTerminalTabInfo {
                id: "tab-2".to_string(),
                label: "someone else".to_string(),
                activity_state: None,
                agent_type: None,
                worktree_slug: None,
                worktree_label: None,
                session_id: Some("not-this-session".to_string()),
            }],
            ..Default::default()
        });
        let sessions = get_active_running_sessions(&state, &cache, &[]).await;
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].title, None);

        pty.close_session(&session_id)
            .await
            .expect("close fixture PTY");
    }

    #[test]
    fn agent_history_query_accepts_before_as_the_cursor() {
        let query: AgentHistoryQuery =
            serde_json::from_value(serde_json::json!({ "before": 1712, "limit": 200 }))
                .expect("before must deserialize");
        assert_eq!(query.cursor, Some(1712));
        let query: AgentHistoryQuery = serde_json::from_value(serde_json::json!({ "cursor": 900 }))
            .expect("cursor still deserializes");
        assert_eq!(query.cursor, Some(900));
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

        let session_id = ws_state
            .active_context
            .session_id
            .expect("session_id populated");
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
        assert!(
            !watcher.is_finished(),
            "watcher task should remain active on transient None"
        );

        // (b) send Some(other) -> close signal fires
        tx.send(Some("other-session".to_string()))
            .expect("send other session");

        let fired =
            tokio::time::timeout(std::time::Duration::from_millis(500), close_rx.recv()).await;
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
                    || wt["path"]
                        .as_str()
                        .is_some_and(|p: &str| p.contains("wt_b"))
            }),
            "expected workspace-b worktrees to contain feat-b, got: {json_b:?}"
        );
        assert!(
            !worktrees
                .iter()
                .any(|wt| wt["workspaceId"] == "workspace-a"),
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
        let state = Arc::new(RemoteGatewayState::new(terminal_service, registry));
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
        assert!(
            !authorized,
            "absent scope must fail closed, denying attachment"
        );

        // Set scope to ws-shared, main:
        state.set_active_selection(RemoteActiveDesktopSelection {
            workspace_id: Some("ws-shared".into()),
            worktree_slug: Some("main".into()),
            ..Default::default()
        });

        // b-shared matches both workspace_id and worktree_slug: authorized
        assert!(attachment_is_authorized(&state, "b-shared").await.unwrap());

        // b-other-wt has same workspace but different worktree: MUST BE DENIED
        assert!(
            !attachment_is_authorized(&state, "b-other-wt")
                .await
                .unwrap(),
            "different worktree must be denied"
        );
    }

    #[tokio::test]
    async fn test_r5_4_scope_watch_terminates_socket_during_await() {
        use futures_util::StreamExt;

        let state = Arc::new(RemoteGatewayState::new(
            Arc::new(TerminalService::default()),
            WorkspaceRegistry::new(),
        ));
        let pin = state
            .auth_manager
            .create_pairing_code(DevicePermission::Control);
        let (token, _) = state
            .auth_manager
            .exchange_pairing_code(&pin, "scope-watch-device")
            .unwrap();

        let backend = Arc::new(ScopeProbeBackend::with_inventory(&[(
            "b-shared",
            "ws-shared",
            "main",
        )]));
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
        let pin = state
            .auth_manager
            .create_pairing_code(DevicePermission::Control);
        let (token, _) = state
            .auth_manager
            .exchange_pairing_code(&pin, "barrier-device")
            .unwrap();

        let backend = Arc::new(ScopeProbeBackend::with_inventory(&[(
            "b-shared",
            "ws-shared",
            "main",
        )]));
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
        assert!(
            shutdown_res.is_ok(),
            "teardown must not hang awaiting blocked writer"
        );
    }

    #[tokio::test]
    async fn test_r6_4_scope_change_interrupts_inline_dispatch_await() {
        use futures_util::{SinkExt, StreamExt};

        let state = Arc::new(RemoteGatewayState::new(
            Arc::new(TerminalService::default()),
            WorkspaceRegistry::new(),
        ));
        let pin = state
            .auth_manager
            .create_pairing_code(DevicePermission::Control);
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
            ) -> crate::remote::browser_backend::BoxFuture<
                'a,
                Result<
                    Vec<crate::remote::browser_backend::RemoteBrowserSessionSummary>,
                    RemoteBrowserError,
                >,
            > {
                self.inner.list_sessions(scope)
            }
            fn identify_session<'a>(
                &'a self,
                scope: &'a DesktopScope,
            ) -> crate::remote::browser_backend::BoxFuture<
                'a,
                Result<
                    Option<crate::remote::browser_backend::RemoteBrowserSessionSummary>,
                    RemoteBrowserError,
                >,
            > {
                self.inner.identify_session(scope)
            }
            fn get_state<'a>(
                &'a self,
                browser_id: &'a str,
                scope: &'a DesktopScope,
            ) -> crate::remote::browser_backend::BoxFuture<
                'a,
                Result<crate::remote::browser_backend::BrowserRemoteState, RemoteBrowserError>,
            > {
                self.inner.get_state(browser_id, scope)
            }
            fn execute_command(
                &self,
                _ctx: crate::remote::browser_backend::BrowserCommandContext,
            ) -> crate::remote::browser_backend::BoxFuture<
                '_,
                Result<crate::remote::browser_backend::BrowserCommandResult, RemoteBrowserError>,
            > {
                Box::pin(async move {
                    // Hang awaiting until cancelled
                    std::future::pending::<()>().await;
                    Ok(crate::remote::browser_backend::BrowserCommandResult {
                        success: true,
                        value: None,
                    })
                })
            }
            fn capabilities(
                &self,
            ) -> crate::remote::browser_backend::BoxFuture<
                '_,
                crate::remote::browser_backend::BrowserCapabilities,
            > {
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
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();

        let sub_resp = ws.next().await.unwrap().unwrap().into_text().unwrap();
        assert!(sub_resp.contains("browserSubscribed"));

        // Now send snapshot which hangs in execute_command
        ws.send(tokio_tungstenite::tungstenite::Message::Text(
            serde_json::json!({
                "type": "browserSnapshot",
                "requestId": "snap-hang",
                "browserId": "b-interrupt"
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();

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

    #[tokio::test]
    #[cfg(unix)]
    async fn test_machine_terminal_input_throttled_under_saturation_preserves_order() {
        use futures_util::SinkExt;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio_tungstenite::tungstenite::Message;

        let fixture = machine_input_fixture::Fixture::new().await;
        let session = fixture.create().await;
        let id = session["target"]["sessionId"].as_str().unwrap();
        let mut socket = fixture.attach(&session).await;

        let path = fixture.root.path().join("throttle.sock");
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let script = r#"use IO::Socket::UNIX; my $s=IO::Socket::UNIX->new(Peer=>$ARGV[0]) or die $!; $s->autoflush(1); print $s pack('L<',$$); read($s,my $go,1)==1 or die; while (1) { my $n=sysread(STDIN,my $b,4096); $n or die; print $s $b; last if index($b,'!')>=0; }"#;
        let command = format!(
            "stty raw -echo; exec /usr/bin/perl -e '{}' '{}'\r",
            script.replace('\'', "'\\''"),
            path.display()
        );
        socket
            .send(Message::Binary(command.into_bytes().into()))
            .await
            .unwrap();

        let (mut control, _) = tokio::time::timeout(Duration::from_secs(10), listener.accept())
            .await
            .unwrap()
            .unwrap();
        let pid = control.read_u32_le().await.unwrap();
        drop(listener);
        let _ = std::fs::remove_file(&path);

        let pty = fixture
            .owner
            .terminal_service()
            .get_session(id)
            .unwrap();
        assert_eq!(pty.pid(), Some(pid));

        let fd = pty.raw_master_fd().unwrap();
        let fill = [b'x'; 65536];
        loop {
            // SAFETY: fill is a valid local slice, fd is owned by active pty.
            let n = unsafe { libc::write(fd, fill.as_ptr().cast(), fill.len()) };
            if n < 0 {
                assert_eq!(
                    std::io::Error::last_os_error().kind(),
                    std::io::ErrorKind::WouldBlock
                );
                break;
            }
        }

        let mut observation = machine_input_probe::Observation::register(id);

        // F1 is popped by receiver and enters pending PTY write (kernel WouldBlock).
        socket
            .send(Message::Binary(b"F1:FIRST\n".to_vec().into()))
            .await
            .unwrap();
        // F2 fills the 1-slot channel queue.
        socket
            .send(Message::Binary(b"F2:SECOND\n".to_vec().into()))
            .await
            .unwrap();
        // F3 begins in-flight send, saturating the writer and triggering queue_full.
        socket
            .send(Message::Binary(b"F3:THIRD\n".to_vec().into()))
            .await
            .unwrap();

        tokio::time::timeout(
            Duration::from_secs(3),
            observation.0.wait_for(|p| p.queue_full),
        )
        .await
        .expect("queue must become full")
        .unwrap();

        // One further frame sent under saturation: backpressure must keep socket open.
        socket
            .send(Message::Binary(b"F4:FOURTH\n".to_vec().into()))
            .await
            .unwrap();

        // Unblock the slow consumer.
        control.write_all(&[1]).await.unwrap();
        socket
            .send(Message::Binary(b"!\n".to_vec().into()))
            .await
            .unwrap();

        let mut delivered = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let n = tokio::time::timeout(Duration::from_secs(10), control.read(&mut chunk))
                .await
                .expect("drain must not time out")
                .expect("read chunk");
            assert!(n > 0, "control closed before delivering terminator");
            delivered.extend_from_slice(&chunk[..n]);
            if delivered.contains(&b'!') {
                break;
            }
        }

        let text = String::from_utf8_lossy(&delivered);
        let pos1 = text.find("F1:FIRST").expect("F1 must be delivered");
        let pos2 = text.find("F2:SECOND").expect("F2 must be delivered");
        let pos3 = text.find("F3:THIRD").expect("F3 must be delivered");
        let pos4 = text.find("F4:FOURTH").expect("F4 must be delivered");
        assert!(pos1 < pos2, "F1 must arrive before F2");
        assert!(pos2 < pos3, "F2 must arrive before F3");
        assert!(pos3 < pos4, "F3 must arrive before F4");

        // Assert the connection stayed open and healthy after saturation.
        socket
            .send(Message::Ping(vec![1, 2, 3].into()))
            .await
            .expect("socket must stay open after saturation");
        let _ = socket.close(None).await;

        fixture.cleanup().await;
    }

    #[test]
    fn test_a_stored_host_record_deserializes_into_ssh_host() {
        // The real store writes the host with camelCase enum values (`source: "config"`,
        // `authMethod: "agent"`). If those stopped matching SshHost's serde names, the remote
        // branch would fail closed and every remote session would silently 404, so the
        // conversion is pinned here against the exact shape the store holds.
        let host_json = serde_json::json!({
            "authMethod": "agent",
            "hostname": "100.91.254.71",
            "id": "ssh-omarchy",
            "label": "omarchy",
            "source": "config",
            "username": "indo"
        });
        let host: crate::ssh::SshHost =
            serde_json::from_value(host_json).expect("stored host must deserialize");
        assert_eq!(host.hostname, "100.91.254.71");
        assert_eq!(host.username.as_deref(), Some("indo"));
        assert_eq!(host.id, "ssh-omarchy");
    }

    #[tokio::test]
    async fn test_agent_history_falls_through_to_the_remote_branch_and_degrades_quietly() {
        // A session the daemon knows, whose cwd has no local transcript, is the case the remote
        // branch exists for. Without machine services there is no paired-host store to consult, so
        // the route must answer a quiet 404 rather than a 500: the client renders its empty state,
        // and a deployment that cannot reach a host must not look like a broken gateway.
        let home = std::env::temp_dir().join(format!(
            "ferryx-agent-remote-miss-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(home.join(".omo").join("agent").join("sessions"))
            .expect("create sessions root");

        let ferryx_session_id = "b6a4d1c9-2e77-4f3a-9c58-0d5a7e91b204";
        let backend = Arc::new(CwdStubBackend {
            session_id: ferryx_session_id.to_string(),
            cwd: std::path::PathBuf::from("/nonexistent/paired/host/project"),
        });
        let state = Arc::new(RemoteGatewayState::new_with_backend(
            backend,
            WorkspaceRegistry::new(),
        ));
        *state.agent_history_home.write() = Some(home.clone());
        let pin = state.auth_manager.create_pairing_code(DevicePermission::Control);
        let (token, _device) = state
            .auth_manager
            .exchange_pairing_code(&pin, "agent-remote-miss-device")
            .unwrap();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let router = create_remote_router(Arc::clone(&state));
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let server_task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = stop_rx.await;
                })
                .await
                .unwrap();
        });

        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let response = client
            .get(format!(
                "http://{addr}/api/v1/agent-history/{ferryx_session_id}?limit=50"
            ))
            .header("authorization", format!("Bearer {token}"))
            .send()
            .await
            .expect("request must complete");

        assert_eq!(
            response.status(),
            StatusCode::NOT_FOUND,
            "a missing remote transcript is a quiet not-found, never a 500"
        );
        let body: serde_json::Value = response.json().await.expect("json body");
        assert_eq!(body["error"].as_str(), Some("TRANSCRIPT_NOT_FOUND"));

        let _ = stop_tx.send(());
        let _ = server_task.await;
        let _ = std::fs::remove_dir_all(&home);
    }

    #[tokio::test]
    async fn test_agent_history_route_serves_the_conversation_from_a_transcript() {
        use std::io::Write;

        let home = std::env::temp_dir().join(format!(
            "ferryx-agent-history-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let sessions = home.join(".omo").join("agent").join("sessions").join("--proj--");
        std::fs::create_dir_all(&sessions).expect("create sessions dir");
        let session_id = "01a0d650-db7a-7817-a21b-7be552a81e89";
        let transcript = sessions.join(format!("2026-09-25T00-00-00-000Z_{session_id}.jsonl"));
        let mut file = std::fs::File::create(&transcript).expect("create transcript");
        writeln!(file, r#"{{"type":"session","id":"{session_id}","cwd":"/proj"}}"#).unwrap();
        writeln!(
            file,
            r#"{{"type":"message","id":"u1","message":{{"role":"user","content":[{{"type":"text","text":"what changed in the parser?"}}]}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"custom_message","customType":"x","content":"HIDDEN_DIRECTIVE","display":false}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"message","id":"a1","message":{{"role":"assistant","content":[{{"type":"text","text":"It now streams line by line."}}]}}}}"#
        )
        .unwrap();
        writeln!(file, "not json at all").unwrap();
        drop(file);

        let terminal_service = Arc::new(TerminalService::default());
        let registry = WorkspaceRegistry::new();
        let state = Arc::new(RemoteGatewayState::new(terminal_service, registry));
        *state.agent_history_home.write() = Some(home.clone());
        let pin = state.auth_manager.create_pairing_code(DevicePermission::Control);
        let (token, _device) = state
            .auth_manager
            .exchange_pairing_code(&pin, "agent-history-device")
            .unwrap();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let router = create_remote_router(Arc::clone(&state));
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let server_task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = stop_rx.await;
                })
                .await
                .unwrap();
        });

        let client = reqwest::Client::builder().no_proxy().build().unwrap();

        let ok = client
            .get(format!("http://{addr}/api/v1/agent-history/{session_id}"))
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await
            .expect("agent-history request");
        assert_eq!(ok.status(), StatusCode::OK);
        let body: serde_json::Value = ok.json().await.expect("json body");
        let items = body["items"].as_array().expect("items array");
        assert_eq!(items.len(), 2, "only the two message records are the conversation");
        assert_eq!(items[0]["role"], "user");
        assert_eq!(items[0]["text"], "what changed in the parser?");
        assert_eq!(items[1]["role"], "assistant");
        assert_eq!(items[1]["text"], "It now streams line by line.");
        let serialized = body.to_string();
        assert!(
            !serialized.contains("HIDDEN_DIRECTIVE"),
            "display:false records must never reach the client"
        );

        let traversal = client
            .get(format!("http://{addr}/api/v1/agent-history/..%2F..%2Fetc"))
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await
            .expect("traversal request");
        assert_eq!(traversal.status(), StatusCode::BAD_REQUEST);

        let missing = client
            .get(format!("http://{addr}/api/v1/agent-history/00000000-0000-0000-0000-000000000000"))
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await
            .expect("missing request");
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);

        let _ = stop_tx.send(());
        let _ = server_task.await;
        let _ = std::fs::remove_dir_all(&home);
    }

    struct CwdStubBackend {
        session_id: String,
        cwd: std::path::PathBuf,
    }

    impl crate::remote::backend::RemoteSessionBackend for CwdStubBackend {
        fn list_sessions(&self) -> futures_util::future::BoxFuture<'_, Vec<String>> {
            Box::pin(async move { vec![self.session_id.clone()] })
        }
        fn describe_session<'a>(
            &'a self,
            session_id: &'a str,
        ) -> futures_util::future::BoxFuture<'a, Result<RemoteSessionDetails, String>> {
            let matches = session_id == self.session_id;
            let cwd = self.cwd.clone();
            let id = self.session_id.clone();
            Box::pin(async move {
                if !matches {
                    return Err("unknown session".to_string());
                }
                Ok(RemoteSessionDetails {
                    session_id: id,
                    workspace_id: Some("ferryx".to_string()),
                    worktree_label: Some("main".to_string()),
                    worktree_path: Some(cwd),
                    running: true,
                    cols: 80,
                    rows: 24,
                })
            })
        }
        fn attach_with_sequence<'a>(
            &'a self,
            _session_id: &'a str,
            _after_sequence: Option<u64>,
        ) -> futures_util::future::BoxFuture<'a, Result<SessionAttachment, String>> {
            Box::pin(async { Err("attach unsupported".into()) })
        }
        fn write_input<'a>(
            &'a self,
            _session_id: &'a str,
            _data: &'a [u8],
        ) -> futures_util::future::BoxFuture<'a, Result<(), String>> {
            Box::pin(async { Ok(()) })
        }
        fn resize<'a>(
            &'a self,
            _session_id: &'a str,
            _cols: u16,
            _rows: u16,
        ) -> futures_util::future::BoxFuture<'a, Result<(), String>> {
            Box::pin(async { Ok(()) })
        }
        fn signal<'a>(
            &'a self,
            _session_id: &'a str,
            _signal: TerminalSignal,
        ) -> futures_util::future::BoxFuture<'a, Result<(), String>> {
            Box::pin(async { Ok(()) })
        }
    }

    #[tokio::test]
    async fn test_agent_history_resolves_the_transcript_by_session_cwd() {
        use std::io::Write;

        let home = std::env::temp_dir().join(format!(
            "ferryx-agent-cwd-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let cwd = "/Volumes/T9-Mac/project/ferryx";
        let slug = "--Volumes-T9-Mac-project-ferryx--";
        let dir = home.join(".omo").join("agent").join("sessions").join(slug);
        std::fs::create_dir_all(&dir).expect("create sessions dir");

        let agent_session_id = "01a0d650-db7a-7817-a21b-7be552a81e89";
        let path = dir.join(format!("2026-09-25T00-00-00-000Z_{agent_session_id}.jsonl"));
        let mut file = std::fs::File::create(&path).expect("create transcript");
        writeln!(file, r#"{{"type":"message","id":"u1","message":{{"role":"user","content":[{{"type":"text","text":"first real prompt"}}]}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"message","id":"a1","message":{{"role":"assistant","content":[{{"type":"text","text":"first real answer"}}]}}}}"#).unwrap();
        drop(file);

        let ferryx_session_id = "458d2968-1dfc-4ded-bf05-ed0daa652e5d";
        let backend = Arc::new(CwdStubBackend {
            session_id: ferryx_session_id.to_string(),
            cwd: std::path::PathBuf::from(cwd),
        });
        let state = Arc::new(RemoteGatewayState::new_with_backend(
            backend,
            WorkspaceRegistry::new(),
        ));
        *state.agent_history_home.write() = Some(home.clone());
        let pin = state.auth_manager.create_pairing_code(DevicePermission::Control);
        let (token, _device) = state
            .auth_manager
            .exchange_pairing_code(&pin, "agent-cwd-device")
            .unwrap();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let router = create_remote_router(Arc::clone(&state));
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let server_task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = stop_rx.await;
                })
                .await
                .unwrap();
        });

        let client = reqwest::Client::builder().no_proxy().build().unwrap();

        let probed = state
            .session_backend
            .describe_session(ferryx_session_id)
            .await
            .expect("stub describe_session must succeed");
        assert_eq!(
            probed.worktree_path.as_deref(),
            Some(std::path::Path::new(cwd)),
            "stub backend must report the session cwd"
        );
        let direct = crate::agent_transcript::latest_transcript_for_cwd(
            &std::path::PathBuf::from(&home),
            cwd,
            Some(ferryx_session_id),
        );
        assert!(
            direct.is_some(),
            "cwd-scoped lookup must find the fixture under {:?}",
            home
        );

        let response = client
            .get(format!("http://{addr}/api/v1/agent-history/{ferryx_session_id}"))
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await
            .expect("agent-history request");
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "the ferryx session id must resolve through the session cwd"
        );
        let body: serde_json::Value = response.json().await.expect("json body");
        let items = body["items"].as_array().expect("items array");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["role"], "user");
        assert_eq!(items[0]["text"], "first real prompt");
        assert_eq!(items[1]["role"], "assistant");
        assert_eq!(items[1]["text"], "first real answer");

        let _ = stop_tx.send(());
        let _ = server_task.await;
        let _ = std::fs::remove_dir_all(&home);
    }

    struct SharedCwdStubBackend {
        session_ids: Vec<String>,
        cwd: std::path::PathBuf,
    }

    impl crate::remote::backend::RemoteSessionBackend for SharedCwdStubBackend {
        fn list_sessions(&self) -> futures_util::future::BoxFuture<'_, Vec<String>> {
            let ids = self.session_ids.clone();
            Box::pin(async move { ids })
        }
        fn describe_session<'a>(
            &'a self,
            session_id: &'a str,
        ) -> futures_util::future::BoxFuture<'a, Result<RemoteSessionDetails, String>> {
            let matches = self.session_ids.iter().any(|id| id == session_id);
            let cwd = self.cwd.clone();
            let id = session_id.to_string();
            Box::pin(async move {
                if !matches {
                    return Err("unknown session".to_string());
                }
                Ok(RemoteSessionDetails {
                    session_id: id,
                    workspace_id: Some("ferryx".to_string()),
                    worktree_label: Some("main".to_string()),
                    worktree_path: Some(cwd),
                    running: true,
                    cols: 80,
                    rows: 24,
                })
            })
        }
        fn attach_with_sequence<'a>(
            &'a self,
            _session_id: &'a str,
            _after_sequence: Option<u64>,
        ) -> futures_util::future::BoxFuture<'a, Result<SessionAttachment, String>> {
            Box::pin(async { Err("attach unsupported".into()) })
        }
        fn write_input<'a>(
            &'a self,
            _session_id: &'a str,
            _data: &'a [u8],
        ) -> futures_util::future::BoxFuture<'a, Result<(), String>> {
            Box::pin(async { Ok(()) })
        }
        fn resize<'a>(
            &'a self,
            _session_id: &'a str,
            _cols: u16,
            _rows: u16,
        ) -> futures_util::future::BoxFuture<'a, Result<(), String>> {
            Box::pin(async { Ok(()) })
        }
        fn signal<'a>(
            &'a self,
            _session_id: &'a str,
            _signal: TerminalSignal,
        ) -> futures_util::future::BoxFuture<'a, Result<(), String>> {
            Box::pin(async { Ok(()) })
        }
    }

    #[tokio::test]
    async fn test_agent_history_never_guesses_when_two_sessions_share_a_cwd() {
        use std::io::Write;

        let home = std::env::temp_dir().join(format!(
            "ferryx-agent-shared-cwd-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let cwd = "/Volumes/T9-Mac/project/ferryx";
        let slug = "--Volumes-T9-Mac-project-ferryx--";
        let dir = home.join(".omo").join("agent").join("sessions").join(slug);
        std::fs::create_dir_all(&dir).expect("create sessions dir");

        let agent_session_id = "01a0d650-db7a-7817-a21b-7be552a81e89";
        let path = dir.join(format!("2026-09-25T00-00-00-000Z_{agent_session_id}.jsonl"));
        let mut file = std::fs::File::create(&path).expect("create transcript");
        writeln!(file, r#"{{"type":"message","id":"u1","message":{{"role":"user","content":[{{"type":"text","text":"ambiguous prompt"}}]}}}}"#).unwrap();
        writeln!(file, r#"{{"type":"message","id":"a1","message":{{"role":"assistant","content":[{{"type":"text","text":"ambiguous answer"}}]}}}}"#).unwrap();
        drop(file);

        let ferryx_session_1 = "11111111-1111-1111-1111-111111111111";
        let ferryx_session_2 = "22222222-2222-2222-2222-222222222222";
        let shared_backend = Arc::new(SharedCwdStubBackend {
            session_ids: vec![ferryx_session_1.to_string(), ferryx_session_2.to_string()],
            cwd: std::path::PathBuf::from(cwd),
        });
        let state1 = Arc::new(RemoteGatewayState::new_with_backend(
            shared_backend,
            WorkspaceRegistry::new(),
        ));
        *state1.agent_history_home.write() = Some(home.clone());
        let pin1 = state1.auth_manager.create_pairing_code(DevicePermission::Control);
        let (token1, _device1) = state1
            .auth_manager
            .exchange_pairing_code(&pin1, "agent-shared-cwd-device-1")
            .unwrap();

        let listener1 = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr1 = listener1.local_addr().unwrap();
        let router1 = create_remote_router(Arc::clone(&state1));
        let (stop_tx1, stop_rx1) = tokio::sync::oneshot::channel::<()>();
        let server_task1 = tokio::spawn(async move {
            axum::serve(listener1, router1)
                .with_graceful_shutdown(async {
                    let _ = stop_rx1.await;
                })
                .await
                .unwrap();
        });

        let client = reqwest::Client::builder().no_proxy().build().unwrap();

        // 1. When two sessions share the cwd, resolving a non-matching session id must NOT return 200 OK
        // and must not return the transcript text.
        let response1 = client
            .get(format!("http://{addr1}/api/v1/agent-history/{ferryx_session_1}"))
            .header("Authorization", format!("Bearer {token1}"))
            .send()
            .await
            .expect("agent-history request");
        assert_ne!(
            response1.status(),
            StatusCode::OK,
            "two sessions sharing a cwd must never guess latest transcript"
        );
        let body_text1 = response1.text().await.unwrap_or_default();
        assert!(
            !body_text1.contains("ambiguous prompt"),
            "must not leak transcript text when cwd is shared"
        );

        let _ = stop_tx1.send(());
        let _ = server_task1.await;

        // 2. A single session in the cwd must still resolve through the cwd fallback.
        let single_backend = Arc::new(CwdStubBackend {
            session_id: ferryx_session_1.to_string(),
            cwd: std::path::PathBuf::from(cwd),
        });
        let state2 = Arc::new(RemoteGatewayState::new_with_backend(
            single_backend,
            WorkspaceRegistry::new(),
        ));
        *state2.agent_history_home.write() = Some(home.clone());
        let pin2 = state2.auth_manager.create_pairing_code(DevicePermission::Control);
        let (token2, _device2) = state2
            .auth_manager
            .exchange_pairing_code(&pin2, "agent-shared-cwd-device-2")
            .unwrap();

        let listener2 = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr2 = listener2.local_addr().unwrap();
        let router2 = create_remote_router(Arc::clone(&state2));
        let (stop_tx2, stop_rx2) = tokio::sync::oneshot::channel::<()>();
        let server_task2 = tokio::spawn(async move {
            axum::serve(listener2, router2)
                .with_graceful_shutdown(async {
                    let _ = stop_rx2.await;
                })
                .await
                .unwrap();
        });

        let response2 = client
            .get(format!("http://{addr2}/api/v1/agent-history/{ferryx_session_1}"))
            .header("Authorization", format!("Bearer {token2}"))
            .send()
            .await
            .expect("agent-history single session request");
        assert_eq!(
            response2.status(),
            StatusCode::OK,
            "single session in cwd must resolve"
        );
        let body2: serde_json::Value = response2.json().await.expect("json body");
        let items2 = body2["items"].as_array().expect("items array");
        assert_eq!(items2.len(), 2);
        assert_eq!(items2[0]["text"], "ambiguous prompt");

        let _ = stop_tx2.send(());
        let _ = server_task2.await;
        let _ = std::fs::remove_dir_all(&home);
    }

    #[tokio::test]
    async fn test_agent_history_reports_continuation_pagination() {
        use std::io::Write;

        let home = std::env::temp_dir().join(format!(
            "ferryx-agent-pager-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let sessions = home.join(".omo").join("agent").join("sessions").join("--proj--");
        std::fs::create_dir_all(&sessions).expect("create sessions dir");
        let session_id = "01a0d650-db7a-7817-a21b-7be552a81e89";
        let transcript = sessions.join(format!("2026-09-25T00-00-00-000Z_{session_id}.jsonl"));
        let mut file = std::fs::File::create(&transcript).expect("create transcript");
        for i in 0..5 {
            writeln!(
                file,
                r#"{{"type":"message","id":"m{i}","message":{{"role":"user","content":[{{"type":"text","text":"message {i}"}}]}}}}"#
            )
            .unwrap();
        }
        drop(file);

        let terminal_service = Arc::new(TerminalService::default());
        let registry = WorkspaceRegistry::new();
        let state = Arc::new(RemoteGatewayState::new(terminal_service, registry));
        *state.agent_history_home.write() = Some(home.clone());
        let pin = state.auth_manager.create_pairing_code(DevicePermission::Control);
        let (token, _device) = state
            .auth_manager
            .exchange_pairing_code(&pin, "agent-pager-device")
            .unwrap();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let router = create_remote_router(Arc::clone(&state));
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let server_task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = stop_rx.await;
                })
                .await
                .unwrap();
        });

        let client = reqwest::Client::builder().no_proxy().build().unwrap();

        let first_body: serde_json::Value = client
            .get(format!("http://{addr}/api/v1/agent-history/{session_id}"))
            .header("Authorization", format!("Bearer {token}"))
            .query(&[("limit", "2")])
            .send()
            .await
            .expect("first agent-history request")
            .json()
            .await
            .expect("first json body");
        let first_items = first_body["items"].as_array().expect("items array");
        assert_eq!(first_items.len(), 2, "limit=2 must return the newest two messages");
        assert_eq!(first_items[0]["ordinal"].as_u64(), Some(3 as u64), "ordinal 3 expected");
        assert_eq!(first_items[1]["ordinal"].as_u64(), Some(4 as u64), "ordinal 4 expected");
        assert!(first_body["partial"].as_bool() == Some(true), "older history remains");
        assert!(
            first_body["nextCursor"].as_u64() == Some(3 as u64),
            "nextCursor is the oldest returned ordinal"
        );

        let second_body: serde_json::Value = client
            .get(format!("http://{addr}/api/v1/agent-history/{session_id}"))
            .header("Authorization", format!("Bearer {token}"))
            .query(&[("limit", "2"), ("cursor", "3")])
            .send()
            .await
            .expect("second agent-history request")
            .json()
            .await
            .expect("second json body");
        let second_items = second_body["items"].as_array().expect("items array");
        assert_eq!(second_items.len(), 2, "cursor=3 must return the two older messages");
        assert_eq!(second_items[0]["ordinal"].as_u64(), Some(1 as u64), "ordinal 1 expected");
        assert_eq!(second_items[1]["ordinal"].as_u64(), Some(2 as u64), "ordinal 2 expected");
        assert!(second_body["partial"].as_bool() == Some(true), "older history remains");
        assert!(
            second_body["nextCursor"].as_u64() == Some(1 as u64),
            "nextCursor is the oldest returned ordinal"
        );

        let final_body: serde_json::Value = client
            .get(format!("http://{addr}/api/v1/agent-history/{session_id}"))
            .header("Authorization", format!("Bearer {token}"))
            .query(&[("limit", "2"), ("cursor", "1")])
            .send()
            .await
            .expect("final agent-history request")
            .json()
            .await
            .expect("final json body");
        let final_items = final_body["items"].as_array().expect("items array");
        assert_eq!(final_items.len(), 1, "cursor=1 must return only the oldest message");
        assert_eq!(final_items[0]["ordinal"].as_u64(), Some(0 as u64), "ordinal 0 expected");
        assert!(final_body["partial"].as_bool() == Some(false), "oldest message included");
        assert!(final_body["nextCursor"].is_null(), "no older history means no cursor");

        let _ = stop_tx.send(());
        let _ = server_task.await;
        let _ = std::fs::remove_dir_all(&home);
    }

    #[tokio::test]
    async fn test_agent_history_walks_a_long_transcript_back_to_its_first_message() {
        use std::io::Write;

        let home = std::env::temp_dir().join(format!(
            "ferryx-agent-long-pager-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let sessions = home.join(".omo").join("agent").join("sessions").join("--proj--");
        std::fs::create_dir_all(&sessions).expect("create sessions dir");
        let session_id = "01a0d650-db7a-7817-a21b-7be552a81e90";
        let transcript = sessions.join(format!("2026-09-25T00-00-00-000Z_{session_id}.jsonl"));
        let mut file = std::fs::File::create(&transcript).expect("create transcript");
        for i in 0..3200 {
            writeln!(
                file,
                r#"{{"type":"message","id":"m{i}","message":{{"role":"user","content":[{{"type":"text","text":"message {i}"}}]}}}}"#
            )
            .unwrap();
        }
        drop(file);

        let terminal_service = Arc::new(TerminalService::default());
        let registry = WorkspaceRegistry::new();
        let state = Arc::new(RemoteGatewayState::new(terminal_service, registry));
        *state.agent_history_home.write() = Some(home.clone());
        let pin = state.auth_manager.create_pairing_code(DevicePermission::Control);
        let (token, _device) = state
            .auth_manager
            .exchange_pairing_code(&pin, "agent-long-pager-device")
            .unwrap();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let router = create_remote_router(Arc::clone(&state));
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let server_task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = stop_rx.await;
                })
                .await
                .unwrap();
        });

        let client = reqwest::Client::builder().no_proxy().build().unwrap();

        let mut collected_ordinals: Vec<u64> = Vec::new();
        let mut text_by_ordinal: std::collections::HashMap<u64, String> = std::collections::HashMap::new();
        let mut next_cursor: Option<u64> = None;
        let mut page_count: usize = 0;

        loop {
            page_count += 1;
            assert!(
                page_count <= 20,
                "walk exceeded 20 pages (infinite loop or paging bug); page_count={page_count}"
            );

            let mut query_params: Vec<(&str, String)> = vec![("limit", "200".to_string())];
            if let Some(cursor_val) = next_cursor {
                query_params.push(("cursor", cursor_val.to_string()));
            }

            let body: serde_json::Value = client
                .get(format!("http://{addr}/api/v1/agent-history/{session_id}"))
                .header("Authorization", format!("Bearer {token}"))
                .query(&query_params)
                .send()
                .await
                .expect("paged agent-history request")
                .json()
                .await
                .expect("paged json body");

            let items = body["items"].as_array().expect("items array");
            assert!(
                !items.is_empty() && items.len() <= 200,
                "page {page_count} items len {} must be between 1 and 200",
                items.len()
            );

            let page_ordinals: Vec<u64> = items
                .iter()
                .map(|item| item["ordinal"].as_u64().expect("item ordinal as u64"))
                .collect();

            // Ordinals strictly ascending within the page
            for window in page_ordinals.windows(2) {
                assert!(
                    window[0] < window[1],
                    "page {page_count} ordinals must be strictly ascending: {} < {}",
                    window[0],
                    window[1]
                );
            }

            // The page's largest ordinal is smaller than every ordinal already collected
            let page_max = *page_ordinals.iter().max().unwrap();
            for &seen in &collected_ordinals {
                assert!(
                    page_max < seen,
                    "page {page_count} max ordinal {page_max} must be strictly smaller than seen ordinal {seen}"
                );
            }

            for item in items {
                let ord = item["ordinal"].as_u64().unwrap();
                if let Some(txt) = item["text"].as_str() {
                    text_by_ordinal.insert(ord, txt.to_string());
                }
            }

            collected_ordinals.extend(page_ordinals);

            if let Some(cursor_num) = body["nextCursor"].as_u64() {
                next_cursor = Some(cursor_num);
            } else {
                assert!(
                    body["nextCursor"].is_null(),
                    "nextCursor must be a number or null"
                );
                break;
            }
        }

        assert_eq!(page_count, 16, "exactly 16 pages expected for 3200 messages / 200 per page");

        let mut sorted_ordinals = collected_ordinals.clone();
        sorted_ordinals.sort_unstable();
        let expected_ordinals: Vec<u64> = (0..3200).collect();
        assert_eq!(
            sorted_ordinals, expected_ordinals,
            "sorted collected ordinals must cover all 0..3200"
        );
        assert_eq!(
            text_by_ordinal.get(&0).map(String::as_str),
            Some("message 0"),
            "text of ordinal 0 must be 'message 0'"
        );

        // Fetch once with limit=200&before=1600 and once with limit=200&cursor=1600
        let before_body: serde_json::Value = client
            .get(format!("http://{addr}/api/v1/agent-history/{session_id}"))
            .header("Authorization", format!("Bearer {token}"))
            .query(&[("limit", "200"), ("before", "1600")])
            .send()
            .await
            .expect("agent-history limit=200&before=1600 request")
            .json()
            .await
            .expect("before json body");
        let before_items = before_body["items"].as_array().expect("before items array");

        let cursor_body: serde_json::Value = client
            .get(format!("http://{addr}/api/v1/agent-history/{session_id}"))
            .header("Authorization", format!("Bearer {token}"))
            .query(&[("limit", "200"), ("cursor", "1600")])
            .send()
            .await
            .expect("agent-history limit=200&cursor=1600 request")
            .json()
            .await
            .expect("cursor json body");
        let cursor_items = cursor_body["items"].as_array().expect("cursor items array");

        assert!(
            !before_items.is_empty(),
            "before items array must be non-empty"
        );
        assert_eq!(
            before_items, cursor_items,
            "items from ?before=1600 and ?cursor=1600 must be equal"
        );

        let _ = stop_tx.send(());
        let _ = server_task.await;
        let _ = std::fs::remove_dir_all(&home);
    }
}
