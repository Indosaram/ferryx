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
use std::path::PathBuf;
use std::sync::Arc;

/// Content classes the modal can render. Nothing else is previewable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FilePreviewKind {
    Text,
    Markdown,
    Image,
    Video,
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

#[cfg(test)]
#[path = "file_preview_contract_tests.rs"]
mod file_preview_contract_tests;
