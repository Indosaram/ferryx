use crate::remote::{
    auth::{DeviceAccessScope, DevicePermission},
    state::RemoteGatewayState,
};
use crate::scoped_contracts::{
    AttachmentMediaType, AttachmentReceipt, TargetRef, ATTACHMENT_MAX_FILE_BYTES,
    ATTACHMENT_MAX_FILES_PER_TURN, ATTACHMENT_MAX_TURN_BYTES,
};
use axum::{
    body::{Body, Bytes},
    extract::{Path as AxumPath, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::{Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
    sync::{Arc, LazyLock},
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentUploadChunkRequest {
    pub target: TargetRef,
    pub attachment_id: String,
    pub file_name: String,
    pub media_type: AttachmentMediaType,
    pub chunk_index: u32,
    pub total_chunks: u32,
    pub offset: u64,
    pub total_bytes: u64,
    pub data: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentUploadSuccessResponse {
    pub ok: bool,
    pub data: AttachmentReceipt,
    pub request_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentChunkAckResponse {
    pub ok: bool,
    pub chunk_index: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentCancelRequest {
    pub target: TargetRef,
    pub attachment_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentCancelResponse {
    pub ok: bool,
    pub cleaned: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultPreviewTokenRequest {
    pub target: TargetRef,
    pub file_id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultFileListRequest {
    pub target: TargetRef,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultFileListEntry {
    pub file_id: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultFileListResponse {
    pub ok: bool,
    pub files: Vec<ResultFileListEntry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultPreviewTokenResponse {
    pub ok: bool,
    pub token: String,
    pub expires_at: u64,
}

#[derive(Debug, Clone)]
pub struct StagedAttachmentRecord {
    pub target: TargetRef,
    pub attachment_id: String,
    pub file_name: String,
    pub media_type: AttachmentMediaType,
    pub sha256: String,
    pub size_bytes: u64,
    pub file_path: PathBuf,
    pub created_at: Instant,
}

#[derive(Debug, Clone)]
pub struct ResultFileRecord {
    pub target: TargetRef,
    pub file_path: PathBuf,
}

struct PendingUpload {
    target: TargetRef,
    file_name: String,
    media_type: AttachmentMediaType,
    total_chunks: u32,
    total_bytes: u64,
    received_chunks: HashMap<u32, (u64, u64, String)>,
    created_at: Instant,
}

#[derive(Default)]
pub struct AttachmentStore {
    pending: HashMap<String, PendingUpload>,
    pub(crate) completed: HashMap<String, StagedAttachmentRecord>,
}

pub static ATTACHMENT_STORE: LazyLock<Mutex<AttachmentStore>> =
    LazyLock::new(|| Mutex::new(AttachmentStore::default()));

#[derive(Clone)]
struct PreviewCapabilityRecord {
    target: TargetRef,
    file_path: PathBuf,
    media_type: String,
    byte_length: u64,
    expires_at: Instant,
    boundary_root: PathBuf,
    boundary_identity: Option<(u64, u64)>,
    file_identity: Option<(u64, u64)>,
}

pub static PREVIEW_REGISTRY: LazyLock<Mutex<HashMap<String, PreviewCapabilityRecord>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub static RESULT_FILES: LazyLock<Mutex<HashMap<String, ResultFileRecord>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn register_result_file(target: TargetRef, file_path: PathBuf) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    RESULT_FILES.lock().insert(id.clone(), ResultFileRecord { target, file_path });
    id
}

fn default_attachments_base_dir() -> PathBuf {
    let tmp = std::env::var_os("TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    tmp.join("ferryx-attachments")
}

pub fn validate_attachment_file_name(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Attachment file name must not be empty".into());
    }
    if trimmed.len() > 255 {
        return Err("Attachment file name exceeds 255 bytes".into());
    }
    if trimmed.contains('/') || trimmed.contains('\\') {
        return Err("Attachment file name must not contain path separators".into());
    }
    if trimmed == "." || trimmed == ".." {
        return Err("Attachment file name must not be a relative directory component".into());
    }
    if trimmed.chars().any(|c| c.is_control()) {
        return Err("Attachment file name must not contain control characters".into());
    }
    Ok(trimmed.to_string())
}

pub fn is_absolute_token(token: &str) -> bool {
    token.starts_with('/')
        || token.starts_with('\\')
        || (token.len() >= 2
            && token.as_bytes()[1] == b':'
            && token.as_bytes()[0].is_ascii_alphabetic())
}

#[cfg(unix)]
fn path_identity(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt as _;
    std::fs::metadata(path).ok().map(|m| (m.dev(), m.ino()))
}

#[cfg(windows)]
#[repr(C)]
struct ByHandleFileInformation {
    file_attributes: u32,
    creation_time: [u32; 2],
    last_access_time: [u32; 2],
    last_write_time: [u32; 2],
    volume_serial_number: u32,
    file_size: [u32; 2],
    number_of_links: u32,
    file_index: [u32; 2],
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateFileW(
        name: *const u16,
        access: u32,
        share: u32,
        security: *mut std::ffi::c_void,
        creation: u32,
        flags: u32,
        template: *mut std::ffi::c_void,
    ) -> *mut std::ffi::c_void;
    fn GetFileInformationByHandle(
        file: *mut std::ffi::c_void,
        info: *mut ByHandleFileInformation,
    ) -> i32;
}

// Identity of an already-open handle: (volume serial number, file index).
// Read-time checks use this so the served file is fenced by the identity of
// the handle actually opened, not by re-stat'ing a path that could be swapped.
#[cfg(windows)]
fn file_identity_from_handle(handle: std::os::windows::io::RawHandle) -> Option<(u64, u64)> {
    let mut info = std::mem::MaybeUninit::<ByHandleFileInformation>::uninit();
    // SAFETY: the caller passes a live handle and info points to a correctly
    // sized output structure matching BY_HANDLE_FILE_INFORMATION.
    let success = unsafe { GetFileInformationByHandle(handle.cast(), info.as_mut_ptr()) };
    if success == 0 {
        return None;
    }
    // SAFETY: GetFileInformationByHandle returned nonzero and initialized info.
    let info = unsafe { info.assume_init() };
    Some((
        u64::from(info.volume_serial_number),
        (u64::from(info.file_index[1]) << 32) | u64::from(info.file_index[0]),
    ))
}

#[cfg(windows)]
fn path_identity(path: &Path) -> Option<(u64, u64)> {
    use std::os::windows::ffi::OsStrExt as _;
    use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _};

    let mut wide_path: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide_path.push(0);
    let handle = unsafe {
        CreateFileW(
            wide_path.as_ptr(),
            0,
            0x0000_0001 | 0x0000_0002 | 0x0000_0004,
            std::ptr::null_mut(),
            3,
            0x0200_0000,
            std::ptr::null_mut(),
        )
    };
    if handle as isize == -1 {
        return None;
    }
    let file = unsafe { std::fs::File::from_raw_handle(handle) };
    file_identity_from_handle(file.as_raw_handle())
}

#[cfg(not(any(unix, windows)))]
fn path_identity(_path: &Path) -> Option<(u64, u64)> {
    // This target has no supported stable file identity API; containment and
    // length checks remain, but replacement identity cannot be guaranteed.
    None
}

fn normalized_canonical_path(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let path = path.as_os_str().to_string_lossy();
        if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{}", rest));
        }
        if let Some(rest) = path.strip_prefix(r"\\?\") {
            return PathBuf::from(rest);
        }
        PathBuf::from(path.as_ref())
    }
    #[cfg(not(windows))]
    {
        path.to_path_buf()
    }
}

pub fn resolve_contained_preview_path(
    worktree_root: &Path,
    relative_str: &str,
) -> Result<PathBuf, String> {
    let trimmed = relative_str.trim();
    if trimmed.is_empty() {
        return Err("Path must not be empty".into());
    }
    if is_absolute_token(trimmed) || trimmed.starts_with('~') {
        return Err("Path must be relative to the worktree root".into());
    }

    let rel_path = Path::new(trimmed);
    for component in rel_path.components() {
        match component {
            Component::ParentDir => return Err("Directory traversal is forbidden".into()),
            Component::RootDir | Component::Prefix(_) => {
                return Err("Absolute path components are forbidden".into())
            }
            _ => {}
        }
    }

    let canonical_root = std::fs::canonicalize(worktree_root)
        .map_err(|e| format!("Invalid worktree root: {e}"))?;
    let comparison_root = normalized_canonical_path(&canonical_root);
    let candidate = comparison_root.join(rel_path);

    let canonical_candidate = std::fs::canonicalize(&candidate)
        .map_err(|e| format!("Target file does not exist or cannot be resolved: {e}"))?;

    if !normalized_canonical_path(&canonical_candidate).starts_with(&comparison_root) {
        return Err("Target resolves outside worktree root jail".into());
    }

    Ok(canonical_candidate)
}

async fn authorize_target(
    state: &RemoteGatewayState,
    headers: &HeaderMap,
    target: &TargetRef,
) -> Result<String, (StatusCode, &'static str)> {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or((StatusCode::UNAUTHORIZED, "UNAUTHORIZED"))?;

    let device = state
        .auth_manager
        .validate_token(token)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "UNAUTHORIZED"))?;

    if device.access_scope != DeviceAccessScope::Machine
        || device.permission != DevicePermission::Control
    {
        return Err((StatusCode::FORBIDDEN, "FORBIDDEN"));
    }

    let daemon_epoch = state
        .daemon_epoch
        .load(std::sync::atomic::Ordering::Relaxed);
    if target.epoch.0 != daemon_epoch {
        return Err((StatusCode::CONFLICT, "TARGET_EXPIRED"));
    }

    // Strict owner binding, matching `managed_chat_api.rs`'s check for the same target type.
    // This route always has an authenticated device (`device.id`), and no caller produces an empty
    // or `"local"` owner — the previous leniency (`is_empty() || == "local"` bypass) let any
    // Control device assert an arbitrary owner by sending `ownerId: ""`, so it is removed.
    if target.owner_id.is_empty() || target.owner_id != device.id {
        return Err((StatusCode::FORBIDDEN, "INVALID_OWNER"));
    }

    if state
        .session_backend
        .describe_session(&target.backend_session_id)
        .await
        .is_err()
    {
        return Err((StatusCode::NOT_FOUND, "SESSION_NOT_FOUND"));
    }

    Ok(device.id)
}

fn attachment_error(status: StatusCode, code: &'static str) -> Response {
    let message = match code {
        "UNAUTHORIZED" => "Authorization token missing or invalid",
        "INVALID_OWNER" => "Target owner does not match authenticated device",
        "TARGET_EXPIRED" => "Target epoch is no longer current",
        "PAYLOAD_TOO_LARGE" | "ATTACHMENT_TURN_LIMIT_EXCEEDED" => "Attachment size limit exceeded",
        "INVALID_CHUNK_GEOMETRY" | "OVERLAPPING_CHUNK_RANGE" | "DUPLICATE_CHUNK_INDEX" => "Attachment chunk geometry is invalid",
        "RESULT_FILE_NOT_FOUND" => "Result file identifier was not found for this target",
        "UNKNOWN_PREVIEW_ID" => "Preview identifier was not found or expired",
        "PERMISSION_DENIED" => "Preview file is outside the selected worktree or changed",
        "FILE_MODIFIED" => "Preview file changed after the token was issued",
        "FILE_NOT_FOUND" => "Preview file was not found",
        "INVALID_RANGE" => "Requested preview byte range is invalid",
        "PREVIEW_READ_FAILED" => "Preview file could not be read",
        "SESSION_NOT_FOUND" => "Target session was not found",
        _ => "Attachment request was rejected",
    };
    (status, Json(json!({ "error": { "code": code, "message": message } }))).into_response()
}

fn attachment_result(result: Result<Response, (StatusCode, &'static str)>) -> Response {
    match result {
        Ok(response) => response,
        Err((status, code)) => attachment_error(status, code),
    }
}

async fn target_project_root(
    state: &RemoteGatewayState,
    target: &TargetRef,
) -> Option<PathBuf> {
    let details = state.session_backend.describe_session(&target.backend_session_id).await.ok()?;
    if let Some(selection) = state.active_selection.read().clone() {
        let selection_matches_target = selection.session_id.as_deref() == Some(&target.backend_session_id)
            || selection.terminal_tabs.iter().any(|tab| tab.session_id.as_deref() == Some(&target.backend_session_id));
        if !selection_matches_target {
            return details.worktree_path;
        }
        if let Some(workspace_id) = selection.workspace_id {
            let manager = state.workspace_registry.manager(&workspace_id).ok()?;
            let slug = selection.worktree_slug?;
            return manager.list_worktrees().ok()?.into_iter()
                .find(|worktree| worktree.orca_info().is_some_and(|info| info.slug == slug))
                .map(|worktree| worktree.path);
        }
    }
    details.worktree_path
}

pub async fn list_result_files(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    Json(req): Json<ResultFileListRequest>,
) -> Response {
    attachment_result(list_result_files_inner(State(state), headers, Json(req)).await)
}

async fn list_result_files_inner(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    Json(req): Json<ResultFileListRequest>,
) -> Result<Response, (StatusCode, &'static str)> {
    authorize_target(&state, &headers, &req.target).await?;
    let Some(project_root) = target_project_root(&state, &req.target).await else {
        return Ok(Json(ResultFileListResponse { ok: true, files: Vec::new() }).into_response());
    };

    let target = req.target;
    let files = crate::ipc::run_blocking(move || {
        let canonical_root = match std::fs::canonicalize(&project_root) {
            Ok(root) => root,
            Err(_) => return Ok(Vec::new()),
        };
        let runs_dir = crate::dag::journal::resolve_dag_runs_dir(&canonical_root);
        let Ok(entries) = std::fs::read_dir(runs_dir) else {
            return Ok(Vec::new());
        };
        let mut discovered = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() || !path.extension().is_some_and(|ext| ext == "json") {
                continue;
            }
            let Ok(metadata) = std::fs::metadata(&path) else { continue };
            if metadata.len() > 1024 * 1024 { continue; }
            let Ok(contents) = std::fs::read_to_string(&path) else { continue };
            let Ok(snapshot) = crate::dag::journal::parse_run_checkpoint(&contents) else { continue };
            for node in snapshot.nodes {
                let Some(artifact) = node.result_artifact else { continue };
                let Ok(file_path) = resolve_contained_preview_path(&canonical_root, &artifact.relative_path) else { continue };
                if !std::fs::metadata(&file_path).is_ok_and(|meta| meta.is_file()) { continue; }
                let Some(display_name) = file_path.file_name().and_then(|name| name.to_str()).map(|name| name.to_owned()) else { continue };
                discovered.push((file_path, display_name));
            }
        }
        discovered.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
        discovered.dedup_by(|a, b| a.0 == b.0);
        Ok(discovered.into_iter().map(|(file_path, display_name)| {
            let file_id = register_result_file(target.clone(), file_path);
            ResultFileListEntry { file_id, display_name }
        }).collect::<Vec<_>>())
    }).await.map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR"))?;

    Ok(Json(ResultFileListResponse { ok: true, files }).into_response())
}

pub async fn upload_attachment_chunk(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    Json(req): Json<AttachmentUploadChunkRequest>,
) -> Response {
    attachment_result(upload_attachment_chunk_inner(State(state), headers, Json(req)).await)
}

async fn upload_attachment_chunk_inner(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    Json(req): Json<AttachmentUploadChunkRequest>,
) -> Result<Response, (StatusCode, &'static str)> {
    authorize_target(&state, &headers, &req.target).await?;

    if req.total_bytes == 0 || req.total_bytes > ATTACHMENT_MAX_FILE_BYTES {
        return Err((StatusCode::PAYLOAD_TOO_LARGE, "PAYLOAD_TOO_LARGE"));
    }
    if req.total_chunks == 0 || req.chunk_index >= req.total_chunks {
        return Err((StatusCode::BAD_REQUEST, "INVALID_CHUNK_GEOMETRY"));
    }
    let sanitized_file_name = validate_attachment_file_name(&req.file_name)
        .map_err(|_| (StatusCode::BAD_REQUEST, "INVALID_FILE_NAME"))?;

    let chunk_bytes = BASE64_STANDARD
        .decode(&req.data)
        .map_err(|_| (StatusCode::BAD_REQUEST, "INVALID_BASE64"))?;
    if req.offset.saturating_add(chunk_bytes.len() as u64) > req.total_bytes {
        return Err((StatusCode::BAD_REQUEST, "INVALID_CHUNK_GEOMETRY"));
    }

    let now = Instant::now();
    let chunk_hash = format!("{:x}", Sha256::digest(&chunk_bytes));
    {
        let store = ATTACHMENT_STORE.lock();
        if store.completed.contains_key(&req.attachment_id) {
            return Err((StatusCode::CONFLICT, "UPLOAD_ALREADY_COMPLETED"));
        }
        if let Some(entry) = store.pending.get(&req.attachment_id) {
            if entry.target != req.target
                || entry.total_chunks != req.total_chunks
                || entry.total_bytes != req.total_bytes
            {
                return Err((StatusCode::BAD_REQUEST, "METADATA_MISMATCH"));
            }
            if entry.received_chunks.contains_key(&req.chunk_index) {
                return Err((StatusCode::BAD_REQUEST, "DUPLICATE_CHUNK_INDEX"));
            }
            if req.offset.checked_add(chunk_bytes.len() as u64).is_none_or(|end| end > req.total_bytes) || chunk_bytes.is_empty() {
                return Err((StatusCode::BAD_REQUEST, "INVALID_CHUNK_GEOMETRY"));
            }
            let end = req.offset + chunk_bytes.len() as u64;
            if entry.received_chunks.values().any(|(offset, len, _)| {
                req.offset < offset.saturating_add(*len) && *offset < end
            }) {
                return Err((StatusCode::BAD_REQUEST, "OVERLAPPING_CHUNK_RANGE"));
            }
        } else {
            if req.offset != 0 || chunk_bytes.is_empty() {
                return Err((StatusCode::BAD_REQUEST, "INVALID_CHUNK_GEOMETRY"));
            }
        }
    }

    let base_dir = default_attachments_base_dir();
    let staging_dir = base_dir
        .join(&req.target.backend_session_id)
        .join(&req.attachment_id);
    let staging_path = staging_dir.join(".upload.staging");
    let final_path = staging_dir.join(&sanitized_file_name);

    let result = crate::ipc::run_blocking(move || {
        Ok((|| -> Result<_, (StatusCode, &'static str)> {
        std::fs::create_dir_all(&staging_dir).map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "FAILED_CREATE_STAGING_DIR",
            )
        })?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&staging_dir, std::fs::Permissions::from_mode(0o700));
        }

        let now = Instant::now();
        let mut store = ATTACHMENT_STORE.lock();

        let thirty_mins = Duration::from_secs(30 * 60);
        store
            .pending
            .retain(|_, v| now.duration_since(v.created_at) < thirty_mins);

        let entry = store
            .pending
            .entry(req.attachment_id.clone())
            .or_insert_with(|| PendingUpload {
                target: req.target.clone(),
                file_name: sanitized_file_name.clone(),
                media_type: req.media_type,
                total_chunks: req.total_chunks,
                total_bytes: req.total_bytes,
                received_chunks: HashMap::new(),
                created_at: now,
            });

        if entry.target != req.target
            || entry.total_chunks != req.total_chunks
            || entry.total_bytes != req.total_bytes
        {
            let _ = std::fs::remove_dir_all(&staging_dir);
            store.pending.remove(&req.attachment_id);
            return Err((StatusCode::BAD_REQUEST, "METADATA_MISMATCH"));
        }

        if entry.received_chunks.is_empty() {
            entry.file_name = sanitized_file_name.clone();
            entry.media_type = req.media_type;
        } else if entry.file_name != sanitized_file_name || entry.media_type != req.media_type {
            return Err((StatusCode::BAD_REQUEST, "METADATA_MISMATCH"));
        }

        if chunk_bytes.is_empty() {
            let _ = std::fs::remove_dir_all(&staging_dir);
            store.pending.remove(&req.attachment_id);
            return Err((StatusCode::BAD_REQUEST, "INVALID_CHUNK_GEOMETRY"));
        }

        let chunk_hash = format!("{:x}", Sha256::digest(&chunk_bytes));
        if entry.received_chunks.contains_key(&req.chunk_index) {
            return Err((StatusCode::BAD_REQUEST, "DUPLICATE_CHUNK_INDEX"));
        } else {
            let end = req.offset + chunk_bytes.len() as u64;
            if entry.received_chunks.values().any(|(offset, len, _)| {
                req.offset < offset.saturating_add(*len) && *offset < end
            }) {
                let _ = std::fs::remove_dir_all(&staging_dir);
                store.pending.remove(&req.attachment_id);
                return Err((StatusCode::BAD_REQUEST, "OVERLAPPING_CHUNK_RANGE"));
            }
            entry.received_chunks.insert(req.chunk_index, (req.offset, chunk_bytes.len() as u64, chunk_hash));
        }

        {
            let mut file = std::fs::OpenOptions::new().create(true).write(true).open(&staging_path)
                .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "FAILED_OPEN_STAGING_FILE"))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = file.set_permissions(std::fs::Permissions::from_mode(0o600));
            }
            file.seek(SeekFrom::Start(req.offset)).map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "FAILED_SEEK_STAGING_FILE"))?;
            file.write_all(&chunk_bytes).map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "FAILED_WRITE_STAGING_FILE"))?;
            file.flush().map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "FAILED_FLUSH_STAGING_FILE"))?;
        }

        let mut ranges: Vec<_> = entry.received_chunks.values().map(|(offset, len, _)| (*offset, *len)).collect();
        ranges.sort_unstable_by_key(|(offset, _)| *offset);
        let mut covered = 0u64;
        let exact_coverage = ranges.iter().all(|(offset, len)| {
            let contiguous = *offset == covered;
            covered = offset.saturating_add(*len);
            contiguous
        }) && covered == entry.total_bytes;
        let is_complete = entry.received_chunks.len() == entry.total_chunks as usize;

        if is_complete && !exact_coverage {
            let _ = std::fs::remove_dir_all(&staging_dir);
            store.pending.remove(&req.attachment_id);
            return Err((StatusCode::BAD_REQUEST, "INVALID_CHUNK_GEOMETRY"));
        }

        if is_complete {
            store.pending.remove(&req.attachment_id);

            let metadata = std::fs::metadata(&staging_path).map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "STAGING_FILE_METADATA_ERROR",
                )
            })?;
            if metadata.len() != req.total_bytes {
                let _ = std::fs::remove_file(&staging_path);
                return Err((StatusCode::BAD_REQUEST, "FILE_SIZE_MISMATCH"));
            }

            let mut file = std::fs::File::open(&staging_path).map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "STAGING_FILE_READ_ERROR",
                )
            })?;
            let mut hasher = Sha256::new();
            let mut buf = [0u8; 8192];
            loop {
                let read = file.read(&mut buf).map_err(|_| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "STAGING_FILE_HASH_ERROR",
                    )
                })?;
                if read == 0 {
                    break;
                }
                hasher.update(&buf[..read]);
            }
            let hash_hex = format!("{:x}", hasher.finalize());

            std::fs::rename(&staging_path, &final_path).map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "FAILED_FINALIZE_ATTACHMENT",
                )
            })?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ =
                    std::fs::set_permissions(&final_path, std::fs::Permissions::from_mode(0o600));
            }

            let receipt = AttachmentReceipt {
                host_id: req.target.host_id.clone(),
                attachment_id: req.attachment_id.clone(),
                sha256: hash_hex.clone(),
                size_bytes: req.total_bytes,
                media_type: req.media_type,
            };

            store.completed.insert(
                req.attachment_id.clone(),
                StagedAttachmentRecord {
                    target: req.target.clone(),
                    attachment_id: req.attachment_id.clone(),
                    file_name: sanitized_file_name,
                    media_type: req.media_type,
                    sha256: hash_hex,
                    size_bytes: req.total_bytes,
                    file_path: final_path,
                    created_at: now,
                },
            );

            Ok(Some(receipt))
        } else {
            Ok(None)
        }
        })())
    })
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR"))??;

    if let Some(receipt) = result {
        Ok(Json(AttachmentUploadSuccessResponse {
            ok: true,
            data: receipt,
            request_id: uuid::Uuid::new_v4().to_string(),
        })
        .into_response())
    } else {
        Ok(Json(AttachmentChunkAckResponse {
            ok: true,
            chunk_index: req.chunk_index,
        })
        .into_response())
    }
}

pub async fn cancel_attachment_upload(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    Json(req): Json<AttachmentCancelRequest>,
) -> Response {
    attachment_result(cancel_attachment_upload_inner(State(state), headers, Json(req)).await)
}

async fn cancel_attachment_upload_inner(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    Json(req): Json<AttachmentCancelRequest>,
) -> Result<Response, (StatusCode, &'static str)> {
    authorize_target(&state, &headers, &req.target).await?;

    let cleaned = crate::ipc::run_blocking(move || {
        let mut store = ATTACHMENT_STORE.lock();
        let owns_pending = store
            .pending
            .get(&req.attachment_id)
            .is_some_and(|record| record.target == req.target);
        let owns_completed = store
            .completed
            .get(&req.attachment_id)
            .is_some_and(|record| record.target == req.target);
        if !owns_pending && !owns_completed {
            return Ok(false);
        }

        if owns_pending && !owns_completed {
            store.pending.remove(&req.attachment_id);
        }
        if owns_completed {
            store.completed.remove(&req.attachment_id);
        }

        let base_dir = default_attachments_base_dir();
        let staging_dir = base_dir
            .join(&req.target.backend_session_id)
            .join(&req.attachment_id);
        let dir_cleaned = if staging_dir.exists() {
            std::fs::remove_dir_all(&staging_dir).is_ok()
        } else {
            true
        };

        Ok(dir_cleaned)
    })
    .await
    .unwrap_or(false);

    Ok(Json(AttachmentCancelResponse { ok: true, cleaned }).into_response())
}

pub async fn mint_result_preview_token(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    Json(req): Json<ResultPreviewTokenRequest>,
) -> Response {
    attachment_result(mint_result_preview_token_inner(State(state), headers, Json(req)).await)
}

async fn mint_result_preview_token_inner(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    Json(req): Json<ResultPreviewTokenRequest>,
) -> Result<Response, (StatusCode, &'static str)> {
    authorize_target(&state, &headers, &req.target).await?;

    let file_record = RESULT_FILES.lock().get(&req.file_id).cloned()
        .filter(|record| record.target == req.target)
        .ok_or((StatusCode::NOT_FOUND, "RESULT_FILE_NOT_FOUND"))?;

    let session_details = state
        .session_backend
        .describe_session(&req.target.backend_session_id)
        .await
        .ok();

    let worktree_root = if let Some(selection) = state.active_selection.read().clone() {
        if let Some(ws_id) = selection.workspace_id {
            let manager = state.workspace_registry.manager(&ws_id)
                .map_err(|_| (StatusCode::NOT_FOUND, "WORKSPACE_NOT_FOUND"))?;
            if let Some(slug) = selection.worktree_slug {
                manager.list_worktrees().map_err(|_| (StatusCode::NOT_FOUND, "WORKTREE_NOT_FOUND"))?
                    .into_iter()
                    .find(|wt| wt.orca_info().is_some_and(|info| info.slug == slug))
                    .map(|wt| wt.path)
                    .ok_or((StatusCode::NOT_FOUND, "WORKTREE_NOT_FOUND"))?
            } else {
                return Err((StatusCode::NOT_FOUND, "WORKTREE_NOT_FOUND"));
            }
        } else {
            session_details
                .and_then(|d| d.worktree_path)
                .ok_or((StatusCode::NOT_FOUND, "WORKSPACE_NOT_FOUND"))?
        }
    } else {
        session_details
            .and_then(|d| d.worktree_path)
            .ok_or((StatusCode::NOT_FOUND, "ACTIVE_SELECTION_MISSING"))?
    };

    let canonical_file = crate::ipc::run_blocking(move || {
        let canonical_root = std::fs::canonicalize(&worktree_root)
            .map_err(|error| crate::ipc::IpcError::internal(error.to_string()))?;
        let registered = std::fs::canonicalize(&file_record.file_path)
            .map_err(|error| crate::ipc::IpcError::internal(error.to_string()))?;
        let comparison_root = normalized_canonical_path(&canonical_root);
        let comparison_registered = normalized_canonical_path(&registered);
        let relative = comparison_registered
            .strip_prefix(&comparison_root)
            .map_err(|_| {
                crate::ipc::IpcError::new(
                    crate::ipc::IpcErrorCode::PermissionDenied,
                    "Preview file is outside the selected worktree",
                )
            })?;
        let path = resolve_contained_preview_path(&comparison_root, &relative.to_string_lossy())
            .map_err(|error| {
                if error == "Target resolves outside worktree root jail" {
                    crate::ipc::IpcError::new(
                        crate::ipc::IpcErrorCode::PermissionDenied,
                        error,
                    )
                } else {
                    crate::ipc::IpcError::internal(error)
                }
            })?;
        Ok((canonical_root, path))
    })
    .await
    .map_err(|error| match error.code {
        crate::ipc::IpcErrorCode::PermissionDenied => {
            (StatusCode::FORBIDDEN, "PERMISSION_DENIED")
        }
        _ => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR"),
    })?;

    let (root_dir, file_path) = canonical_file;

    let metadata = std::fs::metadata(&file_path)
        .map_err(|_| (StatusCode::NOT_FOUND, "FILE_NOT_FOUND"))?;
    if !metadata.is_file() {
        return Err((StatusCode::BAD_REQUEST, "NOT_A_REGULAR_FILE"));
    }

    let token = uuid::Uuid::new_v4().to_string();
    let expires_at_instant = Instant::now() + Duration::from_secs(15 * 60);
    let expires_at_unix = crate::remote::server::unix_now_secs() + 15 * 60;

    let media_type = match file_path.extension().and_then(|s| s.to_str()) {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("txt") | Some("log") => "text/plain; charset=utf-8",
        Some("pdf") => "application/pdf",
        _ => "application/octet-stream",
    }
    .to_string();

    let boundary_id = path_identity(&root_dir);
    let file_id = path_identity(&file_path);

    {
        let mut registry = PREVIEW_REGISTRY.lock();
        let now = Instant::now();
        registry.retain(|_, v| v.expires_at > now);

        registry.insert(
            token.clone(),
            PreviewCapabilityRecord {
                target: req.target,
                file_path,
                media_type,
                byte_length: metadata.len(),
                expires_at: expires_at_instant,
                boundary_root: root_dir,
                boundary_identity: boundary_id,
                file_identity: file_id,
            },
        );
    }

    Ok(Json(ResultPreviewTokenResponse {
        ok: true,
        token,
        expires_at: expires_at_unix,
    })
    .into_response())
}

pub async fn serve_result_preview(
    AxumPath(token): AxumPath<String>,
    headers: HeaderMap,
    State(state): State<Arc<RemoteGatewayState>>,
) -> Response {
    let record = {
        let registry = PREVIEW_REGISTRY.lock();
        registry.get(&token).cloned()
    };

    let Some(record) = record else {
        return attachment_error(StatusCode::NOT_FOUND, "UNKNOWN_PREVIEW_ID");
    };
    if let Err((status, code)) = authorize_target(&state, &headers, &record.target).await {
        return attachment_error(status, code);
    }
    let path = record.file_path.clone();
    let media_type = record.media_type;
    let expected_len = record.byte_length;
    let expires_at = record.expires_at;
    let (root_dir, boundary_id) = (record.boundary_root, record.boundary_identity);
    let expected_file_id = record.file_identity;

    if Instant::now() > expires_at {
        let mut registry = PREVIEW_REGISTRY.lock();
        registry.remove(&token);
        return attachment_error(StatusCode::NOT_FOUND, "UNKNOWN_PREVIEW_ID");
    }

    let normalized_root = normalized_canonical_path(&root_dir);
    let normalized_file_path = normalized_canonical_path(&path);
    let relative = normalized_file_path.strip_prefix(&normalized_root).unwrap_or(Path::new(""));
    let canonical = match resolve_contained_preview_path(&normalized_root, &relative.to_string_lossy()) {
        Ok(path) if normalized_canonical_path(&path) == normalized_canonical_path(&record.file_path) => path,
        _ => return attachment_error(StatusCode::FORBIDDEN, "PERMISSION_DENIED"),
    };

    {
        if let Some(expected) = boundary_id {
            if path_identity(&root_dir) != Some(expected) {
                return attachment_error(StatusCode::FORBIDDEN, "PERMISSION_DENIED");
            }
        }
    }

    let meta = match std::fs::symlink_metadata(&canonical) {
        Ok(m) => m,
        Err(_) => return attachment_error(StatusCode::NOT_FOUND, "FILE_NOT_FOUND"),
    };

    if meta.len() != expected_len {
        return attachment_error(StatusCode::CONFLICT, "FILE_MODIFIED");
    }

    let range_header = headers.get(header::RANGE).and_then(|v| v.to_str().ok());
    let range_outcome = range_header
        .map(|r| crate::ipc::file_preview::parse_range_header(r, expected_len));

    let (status, start, end) = match range_outcome {
        Some(crate::ipc::file_preview::RangeOutcome::Satisfiable { start, end }) => {
            (StatusCode::PARTIAL_CONTENT, start, end)
        }
        Some(crate::ipc::file_preview::RangeOutcome::Unsatisfiable) => {
            return attachment_error(StatusCode::RANGE_NOT_SATISFIABLE, "INVALID_RANGE");
        }
        _ => (StatusCode::OK, 0, expected_len.saturating_sub(1)),
    };

    let mut file = match std::fs::File::open(&canonical) {
        Ok(f) => f,
        Err(_) => return attachment_error(StatusCode::INTERNAL_SERVER_ERROR, "PREVIEW_READ_FAILED"),
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        let opened = file.metadata().ok().map(|meta| (meta.dev(), meta.ino()));
        if expected_file_id.is_some() && opened != expected_file_id {
            return attachment_error(StatusCode::FORBIDDEN, "PERMISSION_DENIED");
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle as _;
        if expected_file_id.is_some()
            && file_identity_from_handle(file.as_raw_handle()) != expected_file_id
        {
            return attachment_error(StatusCode::FORBIDDEN, "PERMISSION_DENIED");
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        if expected_file_id.is_some() && path_identity(&canonical) != expected_file_id {
            return attachment_error(StatusCode::FORBIDDEN, "PERMISSION_DENIED");
        }
    }

    let body_len = if expected_len == 0 {
        0
    } else {
        end - start + 1
    };

    if let Err(_) = file.seek(SeekFrom::Start(start)) {
        return attachment_error(StatusCode::INTERNAL_SERVER_ERROR, "PREVIEW_READ_FAILED");
    }

    let mut take_stream = file.take(body_len);
    let mut data = Vec::with_capacity(body_len as usize);
    if let Err(_) = take_stream.read_to_end(&mut data) {
        return attachment_error(StatusCode::INTERNAL_SERVER_ERROR, "PREVIEW_READ_FAILED");
    }

    let mut response = Response::new(Body::from(data));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_str(&media_type)
            .unwrap_or(header::HeaderValue::from_static("application/octet-stream")),
    );
    response.headers_mut().insert(
        header::CONTENT_LENGTH,
        header::HeaderValue::from_str(&body_len.to_string())
            .unwrap_or(header::HeaderValue::from_static("0")),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        header::HeaderValue::from_static("inline"),
    );
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        header::HeaderValue::from_static("nosniff"),
    );
    response.headers_mut().insert(
        header::ACCEPT_RANGES,
        header::HeaderValue::from_static("bytes"),
    );

    if status == StatusCode::PARTIAL_CONTENT {
        response.headers_mut().insert(
            header::CONTENT_RANGE,
            header::HeaderValue::from_str(&format!("bytes {start}-{end}/{expected_len}"))
                .unwrap_or(header::HeaderValue::from_static("bytes */0")),
        );
    }

    response
}

pub struct GatewayStagedAttachments;

impl crate::ferryx_scope::chat::attachments::StagedAttachments for GatewayStagedAttachments {
    fn verified_input(
        &self,
        target: &TargetRef,
        receipt: &AttachmentReceipt,
    ) -> Result<serde_json::Value, crate::ferryx_scope::chat::ChatError> {
        if receipt.host_id != target.host_id {
            return Err(crate::ferryx_scope::chat::ChatError::Invalid);
        }

        let store = ATTACHMENT_STORE.lock();
        let record = store
            .completed
            .get(&receipt.attachment_id)
            .ok_or(crate::ferryx_scope::chat::ChatError::Stale)?;

        if record.target != *target
            || record.sha256 != receipt.sha256
            || record.size_bytes != receipt.size_bytes
        {
            return Err(crate::ferryx_scope::chat::ChatError::Invalid);
        }

        let meta = std::fs::metadata(&record.file_path)
            .map_err(|_| crate::ferryx_scope::chat::ChatError::Exited)?;
        if meta.len() != receipt.size_bytes {
            return Err(crate::ferryx_scope::chat::ChatError::Invalid);
        }

        Ok(json!({
            "type": "attachment",
            "attachmentId": receipt.attachment_id,
            "mediaType": receipt.media_type,
            "sizeBytes": receipt.size_bytes,
            "sha256": receipt.sha256
        }))
    }
}

pub fn attachment_router(state: Arc<RemoteGatewayState>) -> Router {
    Router::new()
        .route(
            "/api/v1/chat/attachments/upload",
            post(upload_attachment_chunk).layer(axum::extract::DefaultBodyLimit::max(
                super::machine_protocol::MACHINE_JSON_MAX_BYTES,
            )),
        )
        .route(
            "/api/v1/chat/attachments/cancel",
            post(cancel_attachment_upload),
        )
        .route(
            "/api/v1/files/results/list",
            post(list_result_files),
        )
        .route(
            "/api/v1/files/preview/token",
            post(mint_result_preview_token),
        )
        .route(
            "/api/v1/files/preview/{token}",
            get(serve_result_preview),
        )
        .with_state(state)
}

#[cfg(test)]
#[path = "attachment_api_tests.rs"]
mod attachment_api_tests;

