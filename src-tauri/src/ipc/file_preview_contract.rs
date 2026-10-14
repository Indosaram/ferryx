//! Shared file-preview contracts (preparation segment of plan task 6).
//!
//! This module holds the wire DTOs, the machine error reasons, the frozen
//! resource bounds and the ONE shared resolution seam that `ipc::file_preview`
//! (task 1) and `ipc::browser`'s external open must agree on. It deliberately
//! contains no Tauri command, no file access and no process launch:
//!
//! - `ipc::file_preview` (task 1) owns handles, decoding and the range service.
//! - `ipc::browser` keeps the external-launch path and simply delegates its
//!   session-cwd resolution here.
//!
//! Wire parity: every type here mirrors `ui/src/lib/filePreviewTypes.ts`.
use crate::ipc::error::{IpcError, IpcErrorCode};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Content classes the preview can render. Editing is not one of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FilePreviewKind {
    Text,
    Markdown,
    Image,
    Video,
    Audio,
    Pdf,
}

/// Text encodings that decode successfully; anything else is
/// [`FilePreviewErrorReason::UnsupportedEncoding`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilePreviewEncoding {
    #[serde(rename = "utf-8")]
    Utf8,
    #[serde(rename = "utf-16le")]
    Utf16Le,
    #[serde(rename = "utf-16be")]
    Utf16Be,
}

/// Machine-readable failure reasons carried in `IpcError.details.reason`.
///
/// The frontend branches on these values; it must never parse `message` prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilePreviewErrorReason {
    MissingFile,
    PermissionDenied,
    NotRegularFile,
    RemoteUnsupported,
    TooLarge,
    UnsupportedEncoding,
    UnsupportedFormat,
    FileChanged,
    ExpiredHandle,
}

impl FilePreviewErrorReason {
    /// Wire spelling, identical to the TypeScript `FILE_PREVIEW_ERROR_REASONS` entries.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MissingFile => "MissingFile",
            Self::PermissionDenied => "PermissionDenied",
            Self::NotRegularFile => "NotRegularFile",
            Self::RemoteUnsupported => "RemoteUnsupported",
            Self::TooLarge => "TooLarge",
            Self::UnsupportedEncoding => "UnsupportedEncoding",
            Self::UnsupportedFormat => "UnsupportedFormat",
            Self::FileChanged => "FileChanged",
            Self::ExpiredHandle => "ExpiredHandle",
        }
    }

    /// Existing `IpcError` code this reason reports under.
    pub fn code(self) -> IpcErrorCode {
        match self {
            Self::MissingFile => IpcErrorCode::InvalidPath,
            Self::PermissionDenied => IpcErrorCode::IoError,
            Self::NotRegularFile => IpcErrorCode::InvalidPath,
            Self::RemoteUnsupported => IpcErrorCode::Unsupported,
            Self::TooLarge => IpcErrorCode::Unsupported,
            Self::UnsupportedEncoding => IpcErrorCode::Unsupported,
            Self::UnsupportedFormat => IpcErrorCode::Unsupported,
            Self::FileChanged => IpcErrorCode::IoError,
            Self::ExpiredHandle => IpcErrorCode::InvalidArgument,
        }
    }
}

/// Builds the structured preview error envelope: `details.reason` is always the
/// machine reason, and extra machine fields are merged in beside it.
pub fn preview_error(
    reason: FilePreviewErrorReason,
    message: impl Into<String>,
    extra: Option<serde_json::Value>,
) -> IpcError {
    let mut details = json!({ "reason": reason.as_str() });
    if let (Some(serde_json::Value::Object(extra)), Some(target)) = (extra, details.as_object_mut())
    {
        for (key, value) in extra {
            target.insert(key, value);
        }
    }
    IpcError::new(reason.code(), message).with_details(details)
}

/// Frozen resource bounds. The backend enforces every one of these.
pub mod limits {
    /// Text/Markdown hard ceiling: 2 MiB, reported as `TooLarge` with no truncation.
    pub const TEXT_MAX_BYTES: u64 = 2 * 1024 * 1024;
    /// Rendered line ceiling before `TooLarge`.
    pub const MAX_RENDERED_LINES: usize = 50_000;
    /// Compressed image ceiling: 100 MiB.
    pub const IMAGE_MAX_BYTES: u64 = 32 * 1024 * 1024;
    /// Decoded image ceiling: 40 megapixels per frame.
    pub const IMAGE_MAX_PIXELS: u64 = 40_000_000;
    /// Per-axis image ceiling: 16,384 pixels on width or height.
    pub const IMAGE_MAX_AXIS: u32 = 16_384;
    /// Bytes inspected for image signature/dimension headers (covers EXIF-heavy JPEGs).
    pub const IMAGE_HEADER_PROBE: usize = 64 * 1024;
    /// Markdown child image handles per main handle.
    pub const MAX_CHILD_HANDLES: usize = 32;
    /// Concurrent capability media requests before HTTP 429.
    pub const MAX_CONCURRENT_MEDIA_REQUESTS: usize = 8;
    /// HTTP streaming chunk size; no whole-file allocation.
    pub const STREAM_CHUNK_BYTES: usize = 64 * 1024;
}

/// 1-based Unicode scalar caret target, clamped to the document by task 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilePreviewTarget {
    pub line: u32,
    pub col: Option<u32>,
}

/// `cmd_file_preview_open` result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilePreviewPayload {
    /// Opaque random capability handle; carries no filesystem path.
    pub handle: String,
    /// File name for display only.
    pub display_name: String,
    pub kind: FilePreviewKind,
    pub byte_length: u64,
    /// Set for text/markdown only.
    pub encoding: Option<FilePreviewEncoding>,
    /// Allowlisted MIME for media kinds.
    pub media_type: Option<String>,
    /// Loopback capability URL for image/video.
    pub media_url: Option<String>,
    /// Bounded decoded document for text/markdown.
    pub text: Option<String>,
    /// Rendered line count for text/markdown.
    pub line_count: Option<usize>,
    /// Clamped caret target when the request carried line/col.
    pub target: Option<FilePreviewTarget>,
    /// Canonical absolute path of the opened file (main window only).
    #[serde(default)]
    pub resolved_path: Option<String>,
}

/// Resolved file preview target metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilePreviewResolved {
    pub resolved_path: String,
    pub exists: bool,
    pub is_directory: bool,
}

/// `cmd_file_preview_open_child` result: a Markdown child capability, no body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilePreviewChildAsset {
    pub handle: String,
    pub display_name: String,
    pub kind: FilePreviewKind,
    pub byte_length: u64,
    pub media_type: String,
    pub media_url: String,
}

/// Resolves the live working directory of a LOCAL backend terminal session.
///
/// Extracted verbatim from `ipc::browser::resolve_session_cwd` so the external
/// open path (`cmd_open_file_path`) and the preview open path (task 1) resolve
/// relative terminal paths through exactly one implementation. Resolution only:
/// this function never opens, launches or reads a file.
///
/// Behaviour, unchanged from the external-open path:
/// 1. a paired-host relay session is refused before any daemon round trip;
/// 2. without a daemon client, the guard produces the `SessionNotFound` refusal;
/// 3. otherwise `DescribeSession` reads the live shell cwd (bypassing the UI cwd
///    cache) and the successful result refreshes that cache.
pub async fn resolve_local_session_cwd(
    daemon_client: Option<&Arc<crate::daemon::DaemonClient>>,
    session_id: &str,
) -> Result<Option<PathBuf>, IpcError> {
    if crate::terminal::paired_runtime::Runtime::owns(session_id) {
        return crate::ipc::file_link::session_cwd_guard(session_id, None, None);
    }
    let Some(daemon_client) = daemon_client else {
        return crate::ipc::file_link::session_cwd_guard(
            session_id,
            None,
            Some("daemon client unavailable"),
        );
    };
    let details = match daemon_client.describe_session(session_id).await {
        Ok(details) => details,
        Err(error) => {
            return crate::ipc::file_link::session_cwd_guard(session_id, None, Some(&error.message))
        }
    };
    // Bypass the UI cwd cache: DescribeSession reads the live shell process.
    let cwd = crate::ipc::file_link::session_cwd_guard(session_id, Some(&details), None)?;
    if let Some(cwd) = cwd.clone() {
        crate::ipc::terminal::update_cached_cwd(session_id.to_string(), cwd);
    }
    Ok(cwd)
}

/// `app_cache_dir()/remote-files`, the only absolute location a remote session
/// may open locally. `None` when the cache directory cannot be resolved.
pub fn app_remote_files_root<R: tauri::Runtime>(
    app: &impl tauri::Manager<R>,
) -> Option<PathBuf> {
    app.path()
        .app_cache_dir()
        .ok()
        .map(|cache_root| crate::ipc::remote_file::remote_files_root(&cache_root))
}

/// Decides which working directory a file-link open may use once the session
/// lookup has finished. Pure apart from canonicalizing the remote-file cache
/// check, so callers run it through `run_blocking`.
///
/// `workspace_id` is the workspace the CALLER says the session belongs to. It
/// matters because a failed lookup (`SessionNotFound`) cannot tell a dead local
/// shell from an SSH session whose PTY or transport died.
///
/// - An SSH (`ssh:`) or paired (`daemon:`) workspace is refused whatever the
///   lookup returned.
/// - A local session (`Ok`) keeps its live cwd.
/// - An unknown or dead session (`SessionNotFound`) in a workspace the caller
///   names as local does not block an absolute path, which opens directly; a
///   relative token falls back to the caller-supplied `fallback_cwd` and is
///   refused without one. Without a workspace id the session cannot be proven
///   local, so `SessionNotFound` stays a refusal.
/// - A remote refusal (SSH workspace or paired host, `Unsupported`) stays a
///   refusal.
/// - Every refusal above except a remote workspace's successful lookup admits an
///   absolute path that canonicalizes strictly under `remote_files_root` (the
///   cache `ipc::remote_file` writes fetched files to). A path the remote shell
///   printed never opens from the local disk.
/// - Any other lookup error propagates unchanged.
pub fn admit_session_cwd(
    path: &str,
    session: Result<Option<PathBuf>, IpcError>,
    workspace_id: Option<&str>,
    fallback_cwd: Option<PathBuf>,
    remote_files_root: Option<&Path>,
) -> Result<Option<PathBuf>, IpcError> {
    admit_session_cwd_admission(path, session, workspace_id, fallback_cwd, remote_files_root)
        .map(SessionCwdAdmission::cwd)
}

/// [`admit_session_cwd`] that also reports whether the remote-files cache
/// exception was what admitted the path, so a launcher can refuse handing that
/// remote-controlled file to the OS opener.
pub fn admit_session_cwd_admission(
    path: &str,
    session: Result<Option<PathBuf>, IpcError>,
    workspace_id: Option<&str>,
    fallback_cwd: Option<PathBuf>,
    remote_files_root: Option<&Path>,
) -> Result<SessionCwdAdmission, IpcError> {
    let absolute = crate::ipc::file_link::is_absolute_token(path);
    let fetched = || absolute && is_fetched_remote_file(Path::new(path), remote_files_root);
    let workspace_id = workspace_id.filter(|id| !id.is_empty());
    if let Some(workspace_id) = workspace_id.filter(|id| is_remote_workspace(id)) {
        if fetched() {
            return Ok(SessionCwdAdmission::FetchedRemoteFile);
        }
        return Err(match session {
            Err(error) if error.code == IpcErrorCode::Unsupported => error,
            _ => IpcError::new(
                IpcErrorCode::Unsupported,
                "refusing to open a remote path on this machine (remote workspace)",
            )
            .with_details(json!({ "workspaceId": workspace_id, "kind": "remoteWorkspace" })),
        });
    }
    let error = match session {
        Ok(cwd) => return Ok(SessionCwdAdmission::Cwd(cwd)),
        Err(error) => error,
    };
    match error.code {
        IpcErrorCode::SessionNotFound | IpcErrorCode::Unsupported if fetched() => {
            Ok(SessionCwdAdmission::FetchedRemoteFile)
        }
        IpcErrorCode::SessionNotFound if workspace_id.is_none() => Err(error),
        IpcErrorCode::SessionNotFound if absolute => Ok(SessionCwdAdmission::Cwd(None)),
        IpcErrorCode::SessionNotFound => match fallback_cwd {
            Some(cwd) => Ok(SessionCwdAdmission::Cwd(Some(cwd))),
            None => Err(error),
        },
        _ => Err(error),
    }
}

/// SSH (`ssh:`) and paired-host (`daemon:`) workspaces live on another machine.
fn is_remote_workspace(workspace_id: &str) -> bool {
    crate::ssh::projects::is_remote(workspace_id) || workspace_id.starts_with("daemon:")
}

/// Where an admitted file-link open got its permission from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionCwdAdmission {
    /// Ordinary admission: the live or fallback cwd (or none for an absolute path).
    Cwd(Option<PathBuf>),
    /// Admitted only because the path is a fetched remote-files cache file, whose
    /// content and extension a remote host controls.
    FetchedRemoteFile,
}

impl SessionCwdAdmission {
    pub fn cwd(self) -> Option<PathBuf> {
        match self {
            Self::Cwd(cwd) => cwd,
            Self::FetchedRemoteFile => None,
        }
    }
}

/// Extensions of passive data documents the OS opener may launch from the
/// remote-files cache, beyond the explicit preview classes. Source code is left
/// out on purpose: interpreters register "open" handlers for script types
/// (`.py` via the Python launcher, `.rb`, `.lua`, `.jsx`, ...), and XML can carry
/// script once a browser renders it. Editor CLI targets still open those as text.
const PASSIVE_TEXT_EXTENSIONS: &[&str] = &[
    "txt", "log", "md", "markdown", "json", "jsonl", "csv", "tsv", "yaml", "yml", "toml", "ini",
    "conf", "cfg", "diff", "patch",
];

/// True when a fetched remote file may be handed to the OS opener: an explicit
/// image/audio/video/PDF/Markdown preview class or a passive text extension.
/// Executables and launcher types (`.bat`, `.command`, `.app`, `.lnk`, `.js`, `.sh`,
/// ...) are in neither list, and an unknown extension is refused.
pub fn is_passive_document(path: &Path) -> bool {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = path
        .extension()
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if PASSIVE_TEXT_EXTENSIONS.contains(&ext.as_str()) {
        return true;
    }
    // SVG can carry script once a browser opens it as a document.
    ext != "svg"
        && crate::ipc::file_preview::is_explicit_classify_extension(&name)
        && crate::ipc::file_preview::classify_extension(&name).is_some()
}

/// Refuses launching a fetched remote-files cache file through the OS opener
/// unless it is a passive document. `admitted_from_cache` is the session
/// admission's verdict; a session-less open (e.g. "Open with default app")
/// passes `false`, and the path itself is then checked against the cache root.
/// Editor CLI targets open the file as text and stay allowed.
pub fn ensure_remote_cache_launchable(
    path: &Path,
    editor: crate::ipc::file_link::EditorTarget,
    remote_files_root: Option<&Path>,
    admitted_from_cache: bool,
) -> Result<(), IpcError> {
    if editor != crate::ipc::file_link::EditorTarget::System {
        return Ok(());
    }
    if !admitted_from_cache && !is_fetched_remote_file(path, remote_files_root) {
        return Ok(());
    }
    if is_passive_document(path) {
        return Ok(());
    }
    Err(IpcError::new(
        IpcErrorCode::Unsupported,
        "refusing to launch a file fetched from a remote host with the system opener",
    )
    .with_details(json!({
        "reason": "remoteFileNotLaunchable",
        "path": path.to_string_lossy(),
    })))
}

pub(crate) fn is_fetched_remote_file(path: &Path, remote_files_root: Option<&Path>) -> bool {
    let Some(root) = remote_files_root else {
        return false;
    };
    let (Ok(root), Ok(path)) = (std::fs::canonicalize(root), std::fs::canonicalize(path)) else {
        return false;
    };
    path != root && path.starts_with(&root)
}

#[cfg(test)]
#[path = "file_preview_contract_tests.rs"]
mod file_preview_contract_tests;
