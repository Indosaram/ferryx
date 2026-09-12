use crate::remote::auth::{AuthError, DeviceInfo, DevicePermission};
use crate::remote::backend::{RecoveryStream, RemoteRecoveryStatus, RemoteSessionBackend};
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
use std::time::Duration;
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
struct AuthQuery {
    /// Single-use credential minted by `/api/v1/socket-ticket`, used instead of a
    /// permanent device token because a browser WebSocket cannot send headers.
    ticket: Option<String>,
    render: Option<String>,
    cols: Option<u16>,
    rows: Option<u16>,
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
fn extract_token(headers: &HeaderMap) -> Option<String> {
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
) -> Result<Json<PairExchangeResponse>, Response> {
    let identity = crate::remote::auth::canonical_identity_dir()
        .and_then(|dir| crate::remote::auth::load_or_generate_machine_identity(&dir))
        .map_err(|error| {
            tracing::error!(%error, "Unable to load pairing machine identity");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Machine identity unavailable",
            )
                .into_response()
        })?;
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
        })?;

    Ok(Json(PairExchangeResponse {
        token,
        device,
        machine_id: identity.machine_id,
        display_name: identity.display_name,
    }))
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

async fn list_sessions(
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

async fn create_worktree(
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

async fn delete_worktree(
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

    if state.auth_manager.revoke_device(&device_id) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, "Device not found".into()))
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
    let session_id = parse_host_scoped_session_id(&requested_session_id)
        .map(|(_host_id, session_id)| session_id.to_string())
        .unwrap_or(requested_session_id);
    let token = socket_credential(
        &state,
        &headers,
        &query,
        &format!("/api/v1/terminal/{session_id}"),
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
    let render_grid = query.render.as_deref() == Some("grid");
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
    } else if !is_session_valid {
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
    let mut active_session_rx = state.active_session_watch_rx();
    let target_session_id = session_id.clone();
    let mut focus_watcher = std::pin::pin!(async move {
        if !has_active_selection {
            std::future::pending::<()>().await;
            return;
        }
        if active_session_rx.borrow().as_deref() != Some(target_session_id.as_str()) {
            return;
        }
        while active_session_rx.changed().await.is_ok() {
            let current = active_session_rx.borrow().clone();
            if current.as_deref() != Some(target_session_id.as_str()) {
                break;
            }
        }
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
    let mut active_session_rx = state.active_session_watch_rx();
    let target_session_id = session_id.clone();
    let mut focus_watcher = std::pin::pin!(async move {
        if !has_active_selection {
            std::future::pending::<()>().await;
            return;
        }
        if active_session_rx.borrow().as_deref() != Some(target_session_id.as_str()) {
            return;
        }
        while active_session_rx.changed().await.is_ok() {
            let current = active_session_rx.borrow().clone();
            if current.as_deref() != Some(target_session_id.as_str()) {
                break;
            }
        }
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
    let _device = state
        .auth_manager
        .validate_token(&token)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token".into()))?;
    Ok(Json(crate::terminal::load_terminal_preferences()))
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

pub fn create_remote_router(state: Arc<RemoteGatewayState>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/api/v1/health", get(health_check))
        .route("/api/v1/pair/exchange", post(pair_exchange))
        .route("/api/v1/sessions", get(list_sessions))
        .route("/api/v1/workspace/state", get(get_workspace_state))
        .route("/api/v1/workspace/select", post(select_workspace))
        .route("/api/v1/workspace/selection", post(select_workspace))
        .route(
            "/api/v1/terminal/preferences",
            get(get_terminal_preferences),
        )
        .route(
            "/api/v1/workspace/worktrees",
            post(create_worktree).delete(delete_worktree),
        )
        .route("/api/v1/devices", get(list_devices))
        .route("/api/v1/devices/{id}/revoke", post(revoke_device))
        .route("/api/v1/socket-ticket", post(issue_socket_ticket))
        .route("/api/v1/events", get(ws_events_handler))
        .route("/api/v1/terminal/{sessionId}", get(ws_terminal_handler))
        .route("/api/push/subscribe", post(push_subscribe))
        .route("/api/push/unsubscribe", post(push_unsubscribe))
        .fallback(get(serve_static_or_index))
        .layer(cors)
        .with_state(state)
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
}

impl RemoteServerHandle {
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
    let relay_token = std::env::var("FERRYX_MACHINE_TOKEN")
        .ok()
        .filter(|token| !token.trim().is_empty());
    let relay_identity = if config.mode == RemoteNetworkMode::Relay && relay_token.is_none() {
        Some(crate::remote::auth::load_or_generate_machine_identity(
            &crate::remote::auth::canonical_identity_dir()?,
        )?)
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
    match resolved {
        // The baseline loopback listener already covers loopback addresses;
        // skip binding a second listener on the same address/port rather
        // than attempting (and failing) a duplicate bind.
        Ok(Some(extra_ip)) if extra_ip.is_loopback() => {}
        Ok(Some(extra_ip)) => {
            // Use the actual bound loopback port when the caller requested
            // an OS-assigned port (0), so the external listener matches it.
            let extra_addr: SocketAddr = (extra_ip, primary_local_addr.port()).into();
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
            let client = match relay_token {
                Some(token) => crate::remote::relay_client::RelayClient::with_gateway(
                    url,
                    token,
                    primary_local_addr.to_string(),
                ),
                None => crate::remote::relay_client::RelayClient::with_identity(
                    url,
                    relay_identity.expect("relay identity loaded before binding"),
                    primary_local_addr.to_string(),
                ),
            }
            .with_auth_manager((*state.auth_manager).clone());
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
        },
        primary_local_addr,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::state::RemoteGatewayState;
    use crate::terminal::TerminalOutputHub;
    use crate::terminal::TerminalService;
    use crate::worktree::WorkspaceRegistry;

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
}
