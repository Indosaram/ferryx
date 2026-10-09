use crate::remote::{
    auth::{DeviceAccessScope, DevicePermission},
    state::RemoteGatewayState,
};
use crate::scoped_contracts::{
    AttachmentMediaType, AttachmentReceipt, TargetRef, ATTACHMENT_MAX_FILE_BYTES,
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
    collections::{HashMap, HashSet},
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
    pub relative_path: String,
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

struct PendingUpload {
    target: TargetRef,
    file_name: String,
    media_type: AttachmentMediaType,
    total_chunks: u32,
    total_bytes: u64,
    received_chunks: HashSet<u32>,
    received_bytes: u64,
    created_at: Instant,
}

#[derive(Default)]
pub struct AttachmentStore {
    pending: HashMap<String, PendingUpload>,
    completed: HashMap<String, StagedAttachmentRecord>,
}

pub static ATTACHMENT_STORE: LazyLock<Mutex<AttachmentStore>> =
    LazyLock::new(|| Mutex::new(AttachmentStore::default()));

struct PreviewCapabilityRecord {
    target: TargetRef,
    file_path: PathBuf,
    media_type: String,
    byte_length: u64,
    expires_at: Instant,
    boundary_root: PathBuf,
    #[cfg(unix)]
    boundary_identity: Option<(u64, u64)>,
}

pub static PREVIEW_REGISTRY: LazyLock<Mutex<HashMap<String, PreviewCapabilityRecord>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

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

#[cfg(not(unix))]
fn path_identity(_path: &Path) -> Option<(u64, u64)> {
    None
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
    let candidate = canonical_root.join(rel_path);

    let canonical_candidate = std::fs::canonicalize(&candidate)
        .map_err(|e| format!("Target file does not exist or cannot be resolved: {e}"))?;

    if !canonical_candidate.starts_with(&canonical_root) {
        return Err("Target resolves outside worktree root jail".into());
    }

    #[cfg(windows)]
    {
        if let Ok(meta) = std::fs::symlink_metadata(&candidate) {
            if crate::worktree::disk::is_link(&candidate, &meta)
                && !canonical_candidate.starts_with(&canonical_root)
            {
                return Err("Reparse point traversal outside worktree jail".into());
            }
        }
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

    if !target.owner_id.is_empty() && target.owner_id != device.id && target.owner_id != "local" {
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

pub async fn upload_attachment_chunk(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    Json(req): Json<AttachmentUploadChunkRequest>,
) -> Result<Response, (StatusCode, &'static str)> {
    authorize_target(&state, &headers, &req.target).await?;

    if req.total_bytes == 0 || req.total_bytes > ATTACHMENT_MAX_FILE_BYTES {
        return Err((StatusCode::PAYLOAD_TOO_LARGE, "PAYLOAD_TOO_LARGE"));
    }
    if req.total_chunks == 0 || req.chunk_index >= req.total_chunks {
        return Err((StatusCode::BAD_REQUEST, "INVALID_CHUNK_INDEX"));
    }
    if req.offset.saturating_add(req.data.len() as u64) > req.total_bytes {
        return Err((StatusCode::BAD_REQUEST, "OFFSET_OVERFLOW"));
    }

    let sanitized_file_name = validate_attachment_file_name(&req.file_name)
        .map_err(|_| (StatusCode::BAD_REQUEST, "INVALID_FILE_NAME"))?;

    let chunk_bytes = BASE64_STANDARD
        .decode(&req.data)
        .map_err(|_| (StatusCode::BAD_REQUEST, "INVALID_BASE64"))?;

    let base_dir = default_attachments_base_dir();
    let staging_dir = base_dir
        .join(&req.target.backend_session_id)
        .join(&req.attachment_id);
    let staging_path = staging_dir.join(".upload.staging");
    let final_path = staging_dir.join(&sanitized_file_name);

    let result = crate::ipc::run_blocking(move || {
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

        {
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .open(&staging_path)
                .map_err(|_| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "FAILED_OPEN_STAGING_FILE",
                    )
                })?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ =
                    file.set_permissions(std::fs::Permissions::from_mode(0o600));
            }

            file.seek(SeekFrom::Start(req.offset)).map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "FAILED_SEEK_STAGING_FILE",
                )
            })?;
            file.write_all(&chunk_bytes).map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "FAILED_WRITE_STAGING_FILE",
                )
            })?;
            file.flush().map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "FAILED_FLUSH_STAGING_FILE",
                )
            })?;
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
                received_chunks: HashSet::new(),
                received_bytes: 0,
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

        if entry.received_chunks.insert(req.chunk_index) {
            entry.received_bytes = entry
                .received_bytes
                .saturating_add(chunk_bytes.len() as u64);
        }

        let is_complete = entry.received_chunks.len() == entry.total_chunks as usize
            && entry.received_bytes == entry.total_bytes;

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
) -> Result<Response, (StatusCode, &'static str)> {
    authorize_target(&state, &headers, &req.target).await?;

    let base_dir = default_attachments_base_dir();
    let staging_dir = base_dir
        .join(&req.target.backend_session_id)
        .join(&req.attachment_id);

    let cleaned = crate::ipc::run_blocking(move || {
        let mut store = ATTACHMENT_STORE.lock();
        let was_pending = store.pending.remove(&req.attachment_id).is_some();
        let was_completed = store.completed.remove(&req.attachment_id).is_some();

        let dir_cleaned = if staging_dir.exists() {
            std::fs::remove_dir_all(&staging_dir).is_ok()
        } else {
            true
        };

        dir_cleaned && (was_pending || was_completed || !staging_dir.exists())
    })
    .await
    .unwrap_or(false);

    Ok(Json(AttachmentCancelResponse { ok: true, cleaned }).into_response())
}

pub async fn mint_result_preview_token(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    Json(req): Json<ResultPreviewTokenRequest>,
) -> Result<Response, (StatusCode, &'static str)> {
    authorize_target(&state, &headers, &req.target).await?;

    let session_details = state
        .session_backend
        .describe_session(&req.target.backend_session_id)
        .await
        .ok();

    let worktree_root = if let Some(selection) = state.active_selection.read().clone() {
        if let Some(ws_id) = selection.workspace_id {
            if let Ok(manager) = state.workspace_registry.manager(&ws_id) {
                if let Some(slug) = selection.worktree_slug {
                    let worktrees = manager.list_worktrees().unwrap_or_default();
                    worktrees
                        .into_iter()
                        .find(|wt| wt.slug == slug)
                        .map(|wt| wt.path)
                        .unwrap_or_else(|| manager.repo_root().to_path_buf())
                } else {
                    manager.repo_root().to_path_buf()
                }
            } else {
                session_details
                    .and_then(|d| d.worktree_path.or_else(|| d.cwd.map(PathBuf::from)))
                    .ok_or((StatusCode::NOT_FOUND, "WORKSPACE_NOT_FOUND"))?
            }
        } else {
            session_details
                .and_then(|d| d.worktree_path.or_else(|| d.cwd.map(PathBuf::from)))
                .ok_or((StatusCode::NOT_FOUND, "WORKSPACE_NOT_FOUND"))?
        }
    } else {
        session_details
            .and_then(|d| d.worktree_path.or_else(|| d.cwd.map(PathBuf::from)))
            .ok_or((StatusCode::NOT_FOUND, "ACTIVE_SELECTION_MISSING"))?
    };

    let canonical_file = crate::ipc::run_blocking(move || {
        resolve_contained_preview_path(&worktree_root, &req.relative_path)
            .map(|path| (worktree_root, path))
    })
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR"))?
    .map_err(|_| (StatusCode::FORBIDDEN, "PERMISSION_DENIED"))?;

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

    #[cfg(unix)]
    let boundary_id = path_identity(&root_dir);

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
                #[cfg(unix)]
                boundary_identity: boundary_id,
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
) -> Response {
    let record = {
        let registry = PREVIEW_REGISTRY.lock();
        registry.get(&token).map(|r| {
            (
                r.file_path.clone(),
                r.media_type.clone(),
                r.byte_length,
                r.expires_at,
                r.boundary_root.clone(),
                #[cfg(unix)]
                r.boundary_identity,
            )
        })
    };

    let Some((path, media_type, expected_len, expires_at, root_dir, #[cfg(unix)] boundary_id)) =
        record
    else {
        return (StatusCode::NOT_FOUND, "Token not found or expired").into_response();
    };

    if Instant::now() > expires_at {
        let mut registry = PREVIEW_REGISTRY.lock();
        registry.remove(&token);
        return (StatusCode::NOT_FOUND, "Token expired").into_response();
    }

    #[cfg(unix)]
    {
        if let Some(expected) = boundary_id {
            if path_identity(&root_dir) != Some(expected) {
                return (StatusCode::FORBIDDEN, "Root boundary modified").into_response();
            }
        }
    }

    let meta = match std::fs::metadata(&path) {
        Ok(m) => m,
        Err(_) => return (StatusCode::NOT_FOUND, "File not found").into_response(),
    };

    if meta.len() != expected_len {
        return (StatusCode::CONFLICT, "File modified").into_response();
    }

    let range_header = headers.get(header::RANGE).and_then(|v| v.to_str().ok());
    let range_outcome = range_header
        .map(|r| crate::ipc::file_preview::parse_range_header(r, expected_len));

    let (status, start, end) = match range_outcome {
        Some(crate::ipc::file_preview::RangeOutcome::Satisfiable { start, end }) => {
            (StatusCode::PARTIAL_CONTENT, start, end)
        }
        Some(crate::ipc::file_preview::RangeOutcome::Unsatisfiable) => {
            return (
                StatusCode::RANGE_NOT_SATISFIABLE,
                [(
                    header::CONTENT_RANGE,
                    format!("bytes */{expected_len}"),
                )],
            )
                .into_response();
        }
        _ => (StatusCode::OK, 0, expected_len.saturating_sub(1)),
    };

    let mut file = match std::fs::File::open(&path) {
        Ok(f) => f,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let body_len = if expected_len == 0 {
        0
    } else {
        end - start + 1
    };

    if let Err(_) = file.seek(SeekFrom::Start(start)) {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    let mut take_stream = file.take(body_len);
    let mut data = Vec::with_capacity(body_len as usize);
    if let Err(_) = take_stream.read_to_end(&mut data) {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
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
            "/api/v1/files/preview/token",
            post(mint_result_preview_token),
        )
        .route(
            "/api/v1/files/preview/{token}",
            get(serve_result_preview),
        )
        .with_state(state)
}

#[path = "attachment_api_tests.rs"]
mod attachment_api_tests;

