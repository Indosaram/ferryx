//! Bounded read-only file preview capabilities and loopback range service
//! (plan task 1 of `.omo/plans/file-previews.md`).
//!
//! What this module owns:
//! - `cmd_file_preview_open` / `_open_child` / `_open_child_document` / `_close`:
//!   the desktop IPC surface. Registration in `lib.rs` belongs to task 6.
//! - [`FilePreviewService`]: the handle registry plus a dedicated Axum listener
//!   bound to `127.0.0.1:0` that serves capability URLs for image/video bodies.
//!
//! Security model:
//! - A capability handle is 256 bits of OS randomness. It carries no path, and
//!   the URL exposes nothing but the handle.
//! - Every handle retains the opened file descriptor. Symlinks are resolved once
//!   at open; replacing or deleting the path afterwards can never redirect a
//!   handle to another inode. A size change on the retained inode is reported as
//!   `FileChanged` and requires an explicit reload.
//! - Only the desktop root window (`main`) may hold capabilities; untrusted
//!   child webviews (`browser-*`) are refused. Closing a handle or destroying a
//!   window revokes it, and in-flight delivery stops at the next 64 KiB chunk.
//! - The HTTP surface is GET/HEAD only, checks the exact `Host` authority and an
//!   allowlisted `Origin` (absent `Origin` stays allowed for native media
//!   elements), never emits a wildcard CORS header and never lists a directory.
//!
//! All filesystem work happens on the blocking pool via `crate::ipc::run_blocking`;
//! nothing here reads a file on the async reactor.
use crate::ipc::error::{IpcError, IpcErrorCode};
use crate::ipc::file_preview_contract::{
    limits, preview_error, FilePreviewChildAsset, FilePreviewEncoding, FilePreviewErrorReason,
    FilePreviewKind, FilePreviewPayload,
};
pub use crate::ipc::file_preview_contract::FilePreviewTarget;
use axum::body::{Body, Bytes};
use axum::extract::{Path as AxumPath, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::Response;
use axum::routing::{on, MethodFilter};
use axum::Router;
use parking_lot::Mutex;
use serde_json::json;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Desktop window label allowed to hold preview capabilities.
const ROOT_WINDOW_LABEL: &str = "main";

/// Origins the capability server answers to. Never a wildcard.
const ALLOWED_ORIGINS: &[&str] = &[
    "http://127.0.0.1:5173",
    "http://localhost:5173",
    "tauri://localhost",
    "http://tauri.localhost",
    "https://tauri.localhost",
];

/// Extensions that name a media/document format this preview deliberately does
/// not support. They are refused as `UnsupportedFormat` instead of being
/// mis-decoded as text.
const UNSUPPORTED_EXTENSIONS: &[&str] = &[
    "heic", "heif", "avif", "bmp", "tiff", "tif", "ico", "icns", "psd", "mkv", "avi", "wmv", "flv",
    "mpg", "mpeg", "3gp", "mp3", "m4a", "wav", "flac", "aac", "ogg", "opus", "pdf", "zip", "gz",
    "bz2", "xz", "tar", "7z", "rar", "exe", "dll", "so", "dylib", "wasm", "bin", "class", "jar",
    "o", "a", "pyc", "woff", "woff2", "ttf", "otf",
];

// ---------------------------------------------------------------- pure logic

/// Outcome of parsing a single HTTP `Range` header against a known length.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RangeOutcome {
    /// Inclusive byte offsets, already clamped to the file.
    Satisfiable { start: u64, end: u64 },
    /// Unsatisfiable, multiple or malformed: the caller answers 416.
    Unsatisfiable,
}

/// Parses `bytes=start-end`, `bytes=start-` and `bytes=-suffix`.
///
/// Multiple ranges, non-`bytes` units, reversed bounds, overflowing numbers and
/// any offset at or past EOF are all [`RangeOutcome::Unsatisfiable`]; the frozen
/// contract answers every one of them with 416 and `bytes */length`.
pub fn parse_range_header(raw: &str, len: u64) -> RangeOutcome {
    let Some(spec) = raw.trim().strip_prefix("bytes=") else {
        return RangeOutcome::Unsatisfiable;
    };
    if spec.contains(',') {
        return RangeOutcome::Unsatisfiable;
    }
    let Some((first, last)) = spec.split_once('-') else {
        return RangeOutcome::Unsatisfiable;
    };
    let (first, last) = (first.trim(), last.trim());
    let parse = |value: &str| value.parse::<u64>().ok();

    let (start, end) = match (first.is_empty(), last.is_empty()) {
        // suffix form: last `n` bytes
        (true, false) => {
            let Some(suffix) = parse(last) else {
                return RangeOutcome::Unsatisfiable;
            };
            if suffix == 0 || len == 0 {
                return RangeOutcome::Unsatisfiable;
            }
            (len.saturating_sub(suffix), len - 1)
        }
        // open-ended form: from `start` to EOF
        (false, true) => {
            let Some(start) = parse(first) else {
                return RangeOutcome::Unsatisfiable;
            };
            if start >= len {
                return RangeOutcome::Unsatisfiable;
            }
            (start, len - 1)
        }
        // closed form
        (false, false) => {
            let (Some(start), Some(end)) = (parse(first), parse(last)) else {
                return RangeOutcome::Unsatisfiable;
            };
            if start > end || start >= len {
                return RangeOutcome::Unsatisfiable;
            }
            (start, end.min(len - 1))
        }
        (true, true) => return RangeOutcome::Unsatisfiable,
    };
    RangeOutcome::Satisfiable { start, end }
}

fn extension_of(name: &str) -> String {
    Path::new(name)
        .extension()
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// Content class for a file name, or `None` when the format is explicitly not
/// previewable. Unknown extensions are treated as text; SVG is text on purpose
/// (it is never rendered as an active document).
pub fn classify_extension(name: &str) -> Option<FilePreviewKind> {
    let ext = extension_of(name);
    match ext.as_str() {
        "md" | "markdown" => Some(FilePreviewKind::Markdown),
        "png" | "jpg" | "jpeg" | "gif" | "webp" => Some(FilePreviewKind::Image),
        "mp4" | "m4v" | "mov" | "webm" | "ogv" => Some(FilePreviewKind::Video),
        other if UNSUPPORTED_EXTENSIONS.contains(&other) => None,
        _ => Some(FilePreviewKind::Text),
    }
}

/// Allowlisted video MIME for a recognised container. Recognising a container is
/// not a codec guarantee; playback support stays a runtime question.
pub fn video_media_type(name: &str) -> Option<&'static str> {
    match extension_of(name).as_str() {
        "mp4" | "m4v" => Some("video/mp4"),
        "mov" => Some("video/quicktime"),
        "webm" => Some("video/webm"),
        "ogv" => Some("video/ogg"),
        _ => None,
    }
}

/// Image format and pixel dimensions read from the real file signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageSignature {
    pub media_type: &'static str,
    pub width: u64,
    pub height: u64,
}

fn be16(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u16::from_be_bytes([*bytes.get(at)?, *bytes.get(at + 1)?]) as u64)
}

fn be32(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u32::from_be_bytes([
        *bytes.get(at)?,
        *bytes.get(at + 1)?,
        *bytes.get(at + 2)?,
        *bytes.get(at + 3)?,
    ]) as u64)
}

fn le16(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u16::from_le_bytes([*bytes.get(at)?, *bytes.get(at + 1)?]) as u64)
}

fn le24(bytes: &[u8], at: usize) -> Option<u64> {
    Some(
        u32::from_le_bytes([*bytes.get(at)?, *bytes.get(at + 1)?, *bytes.get(at + 2)?, 0]) as u64,
    )
}

/// Identifies PNG/JPEG/GIF/WebP from the header bytes and extracts the frame
/// dimensions, so the pixel bound is checked before the client ever decodes.
/// A file whose signature does not match one of the four baseline formats is
/// refused, regardless of its extension.
pub fn image_signature(bytes: &[u8]) -> Option<ImageSignature> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        if &bytes.get(12..16)? != b"IHDR" {
            return None;
        }
        return Some(ImageSignature {
            media_type: "image/png",
            width: be32(bytes, 16)?,
            height: be32(bytes, 20)?,
        });
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some(ImageSignature {
            media_type: "image/gif",
            width: le16(bytes, 6)?,
            height: le16(bytes, 8)?,
        });
    }
    if bytes.starts_with(b"RIFF") && bytes.get(8..12)? == b"WEBP" {
        return webp_signature(bytes);
    }
    if bytes.starts_with(b"\xFF\xD8") {
        return jpeg_signature(bytes);
    }
    None
}

fn webp_signature(bytes: &[u8]) -> Option<ImageSignature> {
    match bytes.get(12..16)? {
        b"VP8X" => Some(ImageSignature {
            media_type: "image/webp",
            width: le24(bytes, 24)? + 1,
            height: le24(bytes, 27)? + 1,
        }),
        b"VP8 " => {
            if bytes.get(23..26)? != [0x9D, 0x01, 0x2A] {
                return None;
            }
            Some(ImageSignature {
                media_type: "image/webp",
                width: le16(bytes, 26)? & 0x3FFF,
                height: le16(bytes, 28)? & 0x3FFF,
            })
        }
        b"VP8L" => {
            if *bytes.get(20)? != 0x2F {
                return None;
            }
            let packed = u32::from_le_bytes([
                *bytes.get(21)?,
                *bytes.get(22)?,
                *bytes.get(23)?,
                *bytes.get(24)?,
            ]);
            Some(ImageSignature {
                media_type: "image/webp",
                width: ((packed & 0x3FFF) + 1) as u64,
                height: (((packed >> 14) & 0x3FFF) + 1) as u64,
            })
        }
        _ => None,
    }
}

fn jpeg_signature(bytes: &[u8]) -> Option<ImageSignature> {
    let mut at = 2usize;
    while at + 3 < bytes.len() {
        if bytes[at] != 0xFF {
            return None;
        }
        let marker = bytes[at + 1];
        // standalone markers carry no payload
        if (0xD0..=0xD9).contains(&marker) || marker == 0x01 {
            at += 2;
            continue;
        }
        let length = be16(bytes, at + 2)? as usize;
        let is_start_of_frame =
            (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC);
        if is_start_of_frame {
            return Some(ImageSignature {
                media_type: "image/jpeg",
                height: be16(bytes, at + 5)?,
                width: be16(bytes, at + 7)?,
            });
        }
        if length < 2 {
            return None;
        }
        at += 2 + length;
    }
    None
}

/// Strict text decoding: UTF-8 (with or without BOM) and BOM-tagged UTF-16
/// LE/BE. Anything else - other encodings, malformed sequences, NUL bytes or
/// control-heavy binary - is refused rather than lossily decoded.
pub fn decode_text(bytes: &[u8]) -> Option<(String, FilePreviewEncoding)> {
    let (text, encoding) = if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        (decode_utf16(rest, u16::from_le_bytes)?, FilePreviewEncoding::Utf16Le)
    } else if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        (decode_utf16(rest, u16::from_be_bytes)?, FilePreviewEncoding::Utf16Be)
    } else {
        let rest = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
        (
            std::str::from_utf8(rest).ok()?.to_string(),
            FilePreviewEncoding::Utf8,
        )
    };
    if is_binary_like(&text) {
        return None;
    }
    Some((text, encoding))
}

fn decode_utf16(bytes: &[u8], convert: fn([u8; 2]) -> u16) -> Option<String> {
    if bytes.len() % 2 != 0 {
        return None;
    }
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| convert([pair[0], pair[1]]))
        .collect();
    String::from_utf16(&units).ok()
}

/// A NUL byte, or more than 1% control characters, marks binary content.
fn is_binary_like(text: &str) -> bool {
    if text.contains('\0') {
        return true;
    }
    let mut total = 0usize;
    let mut control = 0usize;
    for scalar in text.chars() {
        total += 1;
        if scalar.is_control() && !matches!(scalar, '\n' | '\r' | '\t' | '\u{b}' | '\u{c}') {
            control += 1;
        }
    }
    total > 0 && control * 100 > total
}

/// Rendered line count, insensitive to a single trailing newline.
pub fn count_lines(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    let newlines = text.matches('\n').count();
    if text.ends_with('\n') {
        newlines
    } else {
        newlines + 1
    }
}

/// Clamps a 1-based line/column request to the decoded document. Columns count
/// Unicode scalars, never bytes, and may sit one past the last scalar.
pub fn clamp_target(text: &str, line: Option<u32>, col: Option<u32>) -> Option<FilePreviewTarget> {
    let line = line?;
    let total = count_lines(text).max(1) as u32;
    let line = line.clamp(1, total);
    let col = col.map(|col| {
        let content = text
            .split('\n')
            .nth(line as usize - 1)
            .unwrap_or("")
            .trim_end_matches('\r');
        let scalars = content.chars().count() as u32;
        col.clamp(1, scalars + 1)
    });
    Some(FilePreviewTarget { line, col })
}

/// Only the desktop root window may hold capabilities; child webviews such as
/// the browser pane (`browser-<uuid>`) are refused.
pub fn ensure_trusted_window(label: &str) -> Result<(), IpcError> {
    if label == ROOT_WINDOW_LABEL {
        return Ok(());
    }
    Err(preview_error(
        FilePreviewErrorReason::PermissionDenied,
        "file preview is only available from the desktop root window",
        Some(json!({ "window": label })),
    ))
}

/// Exact-match origin allowlist for the capability server. No wildcards.
pub fn is_allowed_origin(origin: &str) -> bool {
    ALLOWED_ORIGINS.contains(&origin)
}

/// Turns a session-resolution refusal into the frozen `RemoteUnsupported`
/// reason, leaving every other failure (missing session, IO) untouched.
pub fn map_session_error(error: IpcError) -> IpcError {
    if error.code == IpcErrorCode::Unsupported {
        return preview_error(FilePreviewErrorReason::RemoteUnsupported, error.message, None);
    }
    error
}

// ------------------------------------------------------------ handle registry

struct HandleRecord {
    /// Retained read-only descriptor: the identity of the previewed inode.
    file: std::fs::File,
    byte_length: u64,
    media_type: Option<&'static str>,
    kind: FilePreviewKind,
    /// Canonical directory a Markdown document may resolve children under.
    parent_dir: Option<PathBuf>,
    revoked: Arc<AtomicBool>,
}

#[derive(Default)]
struct WindowSlot {
    main: Option<String>,
    children: Vec<String>,
}

#[derive(Default)]
struct Registry {
    handles: HashMap<String, Arc<HandleRecord>>,
    windows: HashMap<String, WindowSlot>,
}

impl Registry {
    fn revoke(&mut self, handle: &str) {
        if let Some(record) = self.handles.remove(handle) {
            record.revoked.store(true, Ordering::SeqCst);
        }
    }
}

/// An opened file plus everything derived from it on the blocking pool.
struct OpenedFile {
    file: std::fs::File,
    display_name: String,
    kind: FilePreviewKind,
    media_type: Option<&'static str>,
    byte_length: u64,
    parent_dir: Option<PathBuf>,
    text: Option<String>,
    encoding: Option<FilePreviewEncoding>,
    line_count: Option<usize>,
    target: Option<FilePreviewTarget>,
}

/// Arguments for [`FilePreviewService::open`]. `cwd` is the live terminal
/// working directory resolved through
/// [`crate::ipc::file_preview_contract::resolve_local_session_cwd`].
#[derive(Debug, Clone)]
pub struct FilePreviewOpenRequest {
    pub window_label: String,
    pub path: String,
    pub cwd: Option<PathBuf>,
    pub line: Option<u32>,
    pub col: Option<u32>,
}

/// Handle registry plus the loopback capability server.
pub struct FilePreviewService {
    registry: Mutex<Registry>,
    origin: String,
    authority: String,
    media_gate: Arc<Semaphore>,
}

impl FilePreviewService {
    /// Binds a dedicated listener on `127.0.0.1` with an OS-assigned port and
    /// serves capability URLs from it. This is not the remote gateway.
    pub async fn start() -> Result<Arc<Self>, IpcError> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|error| IpcError::internal(format!("preview listener bind failed: {error}")))?;
        let addr = listener
            .local_addr()
            .map_err(|error| IpcError::internal(format!("preview listener addr failed: {error}")))?;
        let service = Arc::new(Self {
            registry: Mutex::new(Registry::default()),
            origin: format!("http://{addr}"),
            authority: addr.to_string(),
            media_gate: Arc::new(Semaphore::new(limits::MAX_CONCURRENT_MEDIA_REQUESTS)),
        });
        let router = Router::new()
            .route(
                "/{handle}",
                on(MethodFilter::GET.or(MethodFilter::HEAD), serve_capability),
            )
            .with_state(Arc::clone(&service));
        tokio::spawn(async move {
            if let Err(error) = axum::serve(listener, router).await {
                tracing::warn!(%error, "file preview capability server stopped");
            }
        });
        Ok(service)
    }

    /// `http://127.0.0.1:<port>` - the capability origin.
    pub fn origin(&self) -> &str {
        &self.origin
    }

    /// Concurrency gate for media bodies (8 in flight, then HTTP 429).
    pub fn media_gate(&self) -> Arc<Semaphore> {
        Arc::clone(&self.media_gate)
    }

    /// Opens a previewable file for a window, replacing and revoking whatever
    /// that window previewed before.
    pub async fn open(&self, request: FilePreviewOpenRequest) -> Result<FilePreviewPayload, IpcError> {
        ensure_trusted_window(&request.window_label)?;
        let FilePreviewOpenRequest { window_label, path, cwd, line, col } = request;
        let opened =
            crate::ipc::run_blocking(move || open_blocking(&path, cwd.as_deref(), line, col)).await?;
        Ok(self.register_main(&window_label, opened))
    }

    /// Markdown child image: a bounded, contained sibling asset of the open
    /// document. Returns a capability with no body in the IPC reply.
    pub async fn open_child_image(
        &self,
        window_label: &str,
        parent_handle: &str,
        relative_path: &str,
    ) -> Result<FilePreviewChildAsset, IpcError> {
        ensure_trusted_window(window_label)?;
        let parent_dir = self.markdown_parent_dir(window_label, parent_handle)?;
        self.ensure_child_budget(window_label)?;
        let relative = relative_path.to_string();
        let opened = crate::ipc::run_blocking(move || {
            let path = resolve_contained_child(&parent_dir, &relative)?;
            let opened = open_blocking_path(&path, None, None)?;
            if opened.kind != FilePreviewKind::Image {
                return Err(preview_error(
                    FilePreviewErrorReason::UnsupportedFormat,
                    "markdown child assets must be images",
                    Some(json!({ "displayName": opened.display_name })),
                ));
            }
            Ok(opened)
        })
        .await?;

        let media_type = opened.media_type.unwrap_or("application/octet-stream");
        let byte_length = opened.byte_length;
        let display_name = opened.display_name.clone();
        let handle = self.register_child(window_label, opened);
        Ok(FilePreviewChildAsset {
            handle: handle.clone(),
            display_name,
            kind: FilePreviewKind::Image,
            byte_length,
            media_type: media_type.to_string(),
            media_url: format!("{}/{handle}", self.origin),
        })
    }

    /// Markdown document link: an explicit preview request for another document
    /// under the same parent boundary. The navigated document becomes the
    /// window's main handle and revokes the previous one.
    pub async fn open_child_document(
        &self,
        window_label: &str,
        parent_handle: &str,
        relative_path: &str,
    ) -> Result<FilePreviewPayload, IpcError> {
        ensure_trusted_window(window_label)?;
        let parent_dir = self.markdown_parent_dir(window_label, parent_handle)?;
        let relative = relative_path.to_string();
        let opened = crate::ipc::run_blocking(move || {
            let path = resolve_contained_child(&parent_dir, &relative)?;
            let opened = open_blocking_path(&path, None, None)?;
            if !matches!(opened.kind, FilePreviewKind::Markdown | FilePreviewKind::Text) {
                return Err(preview_error(
                    FilePreviewErrorReason::UnsupportedFormat,
                    "markdown document links must target a text or markdown document",
                    Some(json!({ "displayName": opened.display_name })),
                ));
            }
            Ok(opened)
        })
        .await?;
        Ok(self.register_main(window_label, opened))
    }

    /// Revokes one handle owned by `window_label`. Idempotent, and a window can
    /// never revoke another window's capability.
    pub fn close(&self, window_label: &str, handle: &str) {
        let mut registry = self.registry.lock();
        let Some(slot) = registry.windows.get_mut(window_label) else {
            return;
        };
        if slot.main.as_deref() == Some(handle) {
            let children = std::mem::take(&mut slot.children);
            slot.main = None;
            for child in children {
                registry.revoke(&child);
            }
            registry.revoke(handle);
            return;
        }
        if let Some(index) = slot.children.iter().position(|child| child == handle) {
            slot.children.remove(index);
            registry.revoke(handle);
        }
    }

    /// Revokes every capability a window owns (close or window destruction).
    /// Never terminates a daemon session.
    pub fn close_window(&self, window_label: &str) {
        let mut registry = self.registry.lock();
        let Some(slot) = registry.windows.remove(window_label) else {
            return;
        };
        for handle in slot.children.into_iter().chain(slot.main) {
            registry.revoke(&handle);
        }
    }

    fn markdown_parent_dir(&self, window_label: &str, parent_handle: &str) -> Result<PathBuf, IpcError> {
        let registry = self.registry.lock();
        let is_current_main = registry
            .windows
            .get(window_label)
            .and_then(|slot| slot.main.as_deref())
            == Some(parent_handle);
        let record = registry.handles.get(parent_handle);
        match (is_current_main, record) {
            (true, Some(record)) => record.parent_dir.clone().ok_or_else(|| {
                preview_error(
                    FilePreviewErrorReason::PermissionDenied,
                    "this preview does not own a document boundary",
                    None,
                )
            }),
            _ => Err(preview_error(
                FilePreviewErrorReason::ExpiredHandle,
                "preview handle is unknown or has been revoked",
                None,
            )),
        }
    }

    fn ensure_child_budget(&self, window_label: &str) -> Result<(), IpcError> {
        let registry = self.registry.lock();
        let used = registry
            .windows
            .get(window_label)
            .map(|slot| slot.children.len())
            .unwrap_or(0);
        if used >= limits::MAX_CHILD_HANDLES {
            return Err(preview_error(
                FilePreviewErrorReason::TooLarge,
                "this document already holds the maximum number of image capabilities",
                Some(json!({ "limit": limits::MAX_CHILD_HANDLES })),
            ));
        }
        Ok(())
    }

    fn register_main(&self, window_label: &str, opened: OpenedFile) -> FilePreviewPayload {
        let handle = new_handle();
        let media_url = opened
            .media_type
            .map(|_| format!("{}/{handle}", self.origin));
        let payload = FilePreviewPayload {
            handle: handle.clone(),
            display_name: opened.display_name.clone(),
            kind: opened.kind,
            byte_length: opened.byte_length,
            encoding: opened.encoding,
            media_type: opened.media_type.map(str::to_string),
            media_url,
            text: opened.text.clone(),
            line_count: opened.line_count,
            target: opened.target,
        };

        let mut registry = self.registry.lock();
        let previous = registry
            .windows
            .entry(window_label.to_string())
            .or_default();
        let stale_main = previous.main.take();
        let stale_children = std::mem::take(&mut previous.children);
        previous.main = Some(handle.clone());
        for stale in stale_children.into_iter().chain(stale_main) {
            registry.revoke(&stale);
        }
        registry.handles.insert(handle, record_of(opened));
        payload
    }

    fn register_child(&self, window_label: &str, opened: OpenedFile) -> String {
        let handle = new_handle();
        let mut registry = self.registry.lock();
        registry
            .windows
            .entry(window_label.to_string())
            .or_default()
            .children
            .push(handle.clone());
        registry.handles.insert(handle.clone(), record_of(opened));
        handle
    }
}

fn record_of(opened: OpenedFile) -> Arc<HandleRecord> {
    Arc::new(HandleRecord {
        file: opened.file,
        byte_length: opened.byte_length,
        media_type: opened.media_type,
        kind: opened.kind,
        parent_dir: opened.parent_dir,
        revoked: Arc::new(AtomicBool::new(false)),
    })
}

/// 256 bits of OS randomness, lowercase hex. Carries no path information.
fn new_handle() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

// --------------------------------------------------------------- open (blocking)

fn io_failure(error: &std::io::Error, path: &Path) -> IpcError {
    let reason = match error.kind() {
        std::io::ErrorKind::NotFound => FilePreviewErrorReason::MissingFile,
        _ => FilePreviewErrorReason::PermissionDenied,
    };
    preview_error(
        reason,
        format!("cannot open this file ({error})"),
        Some(json!({
            "displayName": path.file_name().map(|name| name.to_string_lossy().into_owned()),
        })),
    )
}

/// Resolves a terminal token against the live cwd, then opens it.
fn open_blocking(
    token: &str,
    cwd: Option<&Path>,
    line: Option<u32>,
    col: Option<u32>,
) -> Result<OpenedFile, IpcError> {
    let token = crate::ipc::file_link::sanitize_path_token(token)?;
    crate::ipc::file_link::reject_remote_path_spec(token).map_err(map_session_error)?;
    let cwd = cwd.map(|cwd| cwd.to_string_lossy().into_owned());
    let resolved = crate::ipc::file_link::resolve_file_link(token, cwd.as_deref(), home_dir());
    open_blocking_path(&resolved, line, col)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// Canonicalizes once, verifies a regular file, retains the descriptor and
/// applies the per-kind bounds. Symlinks are resolved exactly here.
fn open_blocking_path(
    path: &Path,
    line: Option<u32>,
    col: Option<u32>,
) -> Result<OpenedFile, IpcError> {
    let canonical = std::fs::canonicalize(path).map_err(|error| io_failure(&error, path))?;
    let display_name = canonical
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();

    // Check the type BEFORE opening: opening a FIFO read-only would block.
    let metadata = std::fs::metadata(&canonical).map_err(|error| io_failure(&error, &canonical))?;
    if !metadata.is_file() {
        return Err(not_regular_file(&display_name));
    }
    let mut file =
        std::fs::File::open(&canonical).map_err(|error| io_failure(&error, &canonical))?;
    // Re-check through the descriptor itself, so a swap between stat and open
    // cannot hand us a device or directory.
    let metadata = file
        .metadata()
        .map_err(|error| io_failure(&error, &canonical))?;
    if !metadata.is_file() {
        return Err(not_regular_file(&display_name));
    }
    let byte_length = metadata.len();

    let kind = classify_extension(&display_name).ok_or_else(|| {
        preview_error(
            FilePreviewErrorReason::UnsupportedFormat,
            "this file format cannot be previewed",
            Some(json!({ "displayName": display_name })),
        )
    })?;

    match kind {
        FilePreviewKind::Text | FilePreviewKind::Markdown => {
            if byte_length > limits::TEXT_MAX_BYTES {
                return Err(too_large(&display_name, byte_length, limits::TEXT_MAX_BYTES));
            }
            let mut bytes = Vec::with_capacity(byte_length as usize);
            file.read_to_end(&mut bytes)
                .map_err(|error| io_failure(&error, &canonical))?;
            let (text, encoding) = decode_text(&bytes).ok_or_else(|| {
                preview_error(
                    FilePreviewErrorReason::UnsupportedEncoding,
                    "this file is not UTF-8 or BOM-tagged UTF-16 text",
                    Some(json!({ "displayName": display_name })),
                )
            })?;
            let line_count = count_lines(&text);
            if line_count > limits::MAX_RENDERED_LINES {
                return Err(preview_error(
                    FilePreviewErrorReason::TooLarge,
                    "this document has too many lines to render",
                    Some(json!({
                        "displayName": display_name,
                        "lineCount": line_count,
                        "limit": limits::MAX_RENDERED_LINES,
                    })),
                ));
            }
            let target = clamp_target(&text, line, col);
            Ok(OpenedFile {
                file,
                display_name,
                kind,
                media_type: None,
                byte_length,
                // Only a Markdown document owns a child boundary; plain text
                // never resolves relative references.
                parent_dir: match kind {
                    FilePreviewKind::Markdown => canonical.parent().map(Path::to_path_buf),
                    _ => None,
                },
                text: Some(text),
                encoding: Some(encoding),
                line_count: Some(line_count),
                target,
            })
        }
        FilePreviewKind::Image => {
            if byte_length > limits::IMAGE_MAX_BYTES {
                return Err(too_large(&display_name, byte_length, limits::IMAGE_MAX_BYTES));
            }
            let mut header = vec![0u8; 64.min(byte_length as usize).max(0)];
            header.resize(byte_length.min(4096) as usize, 0);
            let read = file
                .read(&mut header)
                .map_err(|error| io_failure(&error, &canonical))?;
            header.truncate(read);
            let signature = image_signature(&header).ok_or_else(|| {
                preview_error(
                    FilePreviewErrorReason::UnsupportedFormat,
                    "this image format is not supported",
                    Some(json!({ "displayName": display_name })),
                )
            })?;
            let pixels = signature.width.saturating_mul(signature.height);
            if pixels > limits::IMAGE_MAX_PIXELS {
                return Err(preview_error(
                    FilePreviewErrorReason::TooLarge,
                    "this image exceeds the supported pixel budget",
                    Some(json!({
                        "displayName": display_name,
                        "width": signature.width,
                        "height": signature.height,
                        "limit": limits::IMAGE_MAX_PIXELS,
                    })),
                ));
            }
            Ok(OpenedFile {
                file,
                display_name,
                kind,
                media_type: Some(signature.media_type),
                byte_length,
                parent_dir: None,
                text: None,
                encoding: None,
                line_count: None,
                target: None,
            })
        }
        FilePreviewKind::Video => {
            let media_type = video_media_type(&display_name).ok_or_else(|| {
                preview_error(
                    FilePreviewErrorReason::UnsupportedFormat,
                    "this video container is not supported",
                    Some(json!({ "displayName": display_name })),
                )
            })?;
            Ok(OpenedFile {
                file,
                display_name,
                kind,
                media_type: Some(media_type),
                byte_length,
                parent_dir: None,
                text: None,
                encoding: None,
                line_count: None,
                target: None,
            })
        }
    }
}

fn not_regular_file(display_name: &str) -> IpcError {
    preview_error(
        FilePreviewErrorReason::NotRegularFile,
        "only regular files can be previewed",
        Some(json!({ "displayName": display_name })),
    )
}

fn too_large(display_name: &str, byte_length: u64, limit: u64) -> IpcError {
    preview_error(
        FilePreviewErrorReason::TooLarge,
        "this file is too large to preview",
        Some(json!({
            "displayName": display_name,
            "byteLength": byte_length,
            "limit": limit,
        })),
    )
}

/// Resolves a Markdown-relative reference strictly under the document's
/// canonical directory. Absolute paths, URLs, `..` traversal and symlinks that
/// escape the boundary are all refused.
fn resolve_contained_child(parent_dir: &Path, relative: &str) -> Result<PathBuf, IpcError> {
    let relative = crate::ipc::file_link::sanitize_path_token(relative)?;
    crate::ipc::file_link::reject_remote_path_spec(relative).map_err(map_session_error)?;
    if crate::ipc::file_link::is_absolute_token(relative) || relative.starts_with('~') {
        return Err(outside_boundary(relative));
    }
    let candidate = parent_dir.join(relative);
    let canonical = std::fs::canonicalize(&candidate).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => preview_error(
            FilePreviewErrorReason::MissingFile,
            "this document reference does not exist",
            Some(json!({ "displayName": relative })),
        ),
        _ => outside_boundary(relative),
    })?;
    let boundary = std::fs::canonicalize(parent_dir).unwrap_or_else(|_| parent_dir.to_path_buf());
    if !canonical.starts_with(&boundary) {
        return Err(outside_boundary(relative));
    }
    Ok(canonical)
}

fn outside_boundary(relative: &str) -> IpcError {
    preview_error(
        FilePreviewErrorReason::PermissionDenied,
        "this reference resolves outside the document directory",
        Some(json!({ "displayName": relative })),
    )
}

// ------------------------------------------------------------- HTTP surface

fn base_headers(media_type: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(media_type).unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );
    headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers
}

fn status_only(status: StatusCode) -> Response {
    let mut response = Response::new(Body::empty());
    *response.status_mut() = status;
    response
}

async fn serve_capability(
    State(service): State<Arc<FilePreviewService>>,
    method: Method,
    AxumPath(handle): AxumPath<String>,
    headers: HeaderMap,
) -> Response {
    // 1. exact Host authority - a forged Host never reaches a capability
    let host = headers.get(header::HOST).and_then(|value| value.to_str().ok());
    if host != Some(service.authority.as_str()) {
        return status_only(StatusCode::FORBIDDEN);
    }
    // 2. allowlisted Origin; absent Origin stays allowed for native media
    let origin = headers.get(header::ORIGIN).and_then(|value| value.to_str().ok());
    if let Some(origin) = origin {
        if !is_allowed_origin(origin) {
            return status_only(StatusCode::FORBIDDEN);
        }
    }
    // 3. unknown or revoked handle
    let record = { service.registry.lock().handles.get(&handle).cloned() };
    let Some(record) = record else {
        return status_only(StatusCode::NOT_FOUND);
    };
    if record.revoked.load(Ordering::SeqCst) {
        return status_only(StatusCode::NOT_FOUND);
    }
    // 4. the retained inode must still be the file we measured
    let current_len = match record.file.metadata() {
        Ok(metadata) => metadata.len(),
        Err(_) => return status_only(StatusCode::CONFLICT),
    };
    if current_len != record.byte_length {
        let mut response = status_only(StatusCode::CONFLICT);
        response.headers_mut().insert(
            "x-preview-reason",
            HeaderValue::from_static(FilePreviewErrorReason::FileChanged.as_str()),
        );
        return response;
    }
    // 5. global media concurrency gate
    let Ok(permit) = Arc::clone(&service.media_gate).try_acquire_owned() else {
        return status_only(StatusCode::TOO_MANY_REQUESTS);
    };

    let media_type = record.media_type.unwrap_or("application/octet-stream");
    let length = record.byte_length;
    let range = headers
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok())
        .map(|raw| parse_range_header(raw, length));

    let (status, start, end) = match range {
        Some(RangeOutcome::Unsatisfiable) => {
            let mut response = status_only(StatusCode::RANGE_NOT_SATISFIABLE);
            response.headers_mut().extend(base_headers(media_type));
            response.headers_mut().insert(
                header::CONTENT_RANGE,
                HeaderValue::from_str(&format!("bytes */{length}"))
                    .unwrap_or(HeaderValue::from_static("bytes */0")),
            );
            apply_cors(&mut response, origin);
            return response;
        }
        Some(RangeOutcome::Satisfiable { start, end }) => (StatusCode::PARTIAL_CONTENT, start, end),
        None => (StatusCode::OK, 0, length.saturating_sub(1)),
    };
    let body_length = if length == 0 { 0 } else { end - start + 1 };

    let mut response = Response::new(Body::empty());
    *response.status_mut() = status;
    response.headers_mut().extend(base_headers(media_type));
    response.headers_mut().insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&body_length.to_string()).unwrap_or(HeaderValue::from_static("0")),
    );
    if status == StatusCode::PARTIAL_CONTENT {
        response.headers_mut().insert(
            header::CONTENT_RANGE,
            HeaderValue::from_str(&format!("bytes {start}-{end}/{length}"))
                .unwrap_or(HeaderValue::from_static("bytes */0")),
        );
    }
    apply_cors(&mut response, origin);

    if method == Method::HEAD || body_length == 0 {
        return response;
    }
    let Ok(clone) = record.file.try_clone() else {
        return status_only(StatusCode::CONFLICT);
    };
    *response.body_mut() = Body::from_stream(chunk_stream(
        tokio::fs::File::from_std(clone),
        start,
        body_length,
        Arc::clone(&record.revoked),
        permit,
    ));
    response
}

fn apply_cors(response: &mut Response, origin: Option<&str>) {
    if let Some(origin) = origin {
        if let Ok(value) = HeaderValue::from_str(origin) {
            // exact echo of an allowlisted origin - never `*`
            response
                .headers_mut()
                .insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, value);
        }
    }
}

struct ChunkState {
    file: tokio::fs::File,
    remaining: u64,
    started: bool,
    offset: u64,
    revoked: Arc<AtomicBool>,
    permit: Option<OwnedSemaphorePermit>,
}

/// Streams the selected byte window in fixed 64 KiB chunks, never allocating the
/// whole file. Revocation is observed at every chunk boundary; the permit is
/// released before the aborted body reaches the client.
fn chunk_stream(
    file: tokio::fs::File,
    offset: u64,
    remaining: u64,
    revoked: Arc<AtomicBool>,
    permit: OwnedSemaphorePermit,
) -> impl futures_util::Stream<Item = Result<Bytes, std::io::Error>> {
    futures_util::stream::unfold(
        ChunkState {
            file,
            remaining,
            started: false,
            offset,
            revoked,
            permit: Some(permit),
        },
        |mut state| async move {
            use tokio::io::{AsyncReadExt, AsyncSeekExt};

            if state.remaining == 0 {
                return None;
            }
            if state.revoked.load(Ordering::SeqCst) {
                state.permit.take();
                return Some((
                    Err(std::io::Error::new(
                        std::io::ErrorKind::Interrupted,
                        "preview capability revoked",
                    )),
                    state,
                ));
            }
            if !state.started {
                if let Err(error) = state.file.seek(std::io::SeekFrom::Start(state.offset)).await {
                    state.permit.take();
                    return Some((Err(error), state));
                }
                state.started = true;
            }
            let want = state.remaining.min(limits::STREAM_CHUNK_BYTES as u64) as usize;
            let mut buffer = vec![0u8; want];
            let mut filled = 0usize;
            while filled < want {
                match state.file.read(&mut buffer[filled..]).await {
                    Ok(0) => break,
                    Ok(read) => filled += read,
                    Err(error) => {
                        state.permit.take();
                        return Some((Err(error), state));
                    }
                }
            }
            if filled == 0 {
                // the retained inode shrank under us: refuse to pad the body
                state.permit.take();
                return Some((
                    Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "previewed file changed on disk",
                    )),
                    state,
                ));
            }
            buffer.truncate(filled);
            state.remaining -= filled as u64;
            Some((Ok(Bytes::from(buffer)), state))
        },
    )
}

// ------------------------------------------------------------- IPC commands

/// Opens a preview for a terminal file token. The live working directory comes
/// from the shared resolution seam, so external open and preview agree.
#[tauri::command]
pub async fn cmd_file_preview_open<R: tauri::Runtime>(
    window: tauri::Window<R>,
    service: tauri::State<'_, Arc<FilePreviewService>>,
    daemon_client: tauri::State<'_, Arc<crate::daemon::DaemonClient>>,
    path: String,
    backend_session_id: String,
    line: Option<u32>,
    col: Option<u32>,
) -> Result<FilePreviewPayload, IpcError> {
    let window_label = window.label().to_string();
    ensure_trusted_window(&window_label)?;
    let cwd = crate::ipc::file_preview_contract::resolve_local_session_cwd(
        Some(daemon_client.inner()),
        &backend_session_id,
    )
    .await
    .map_err(map_session_error)?;
    if cwd.is_none()
        && !crate::ipc::file_link::is_absolute_token(&path)
        && !path.starts_with('~')
    {
        return Err(preview_error(
            FilePreviewErrorReason::MissingFile,
            "The terminal's current directory could not be read. Use an absolute file path.",
            None,
        ));
    }
    service
        .open(FilePreviewOpenRequest { window_label, path, cwd, line, col })
        .await
}

/// Markdown child image capability under the open document's directory.
#[tauri::command]
pub async fn cmd_file_preview_open_child<R: tauri::Runtime>(
    window: tauri::Window<R>,
    service: tauri::State<'_, Arc<FilePreviewService>>,
    parent_handle: String,
    relative_path: String,
) -> Result<FilePreviewChildAsset, IpcError> {
    service
        .open_child_image(window.label(), &parent_handle, &relative_path)
        .await
}

/// Markdown document link: an explicit preview request under the same boundary.
///
/// NOTE for task 6: `FilePreviewChildAsset` cannot describe a document (it has a
/// mandatory `mediaType`/`mediaUrl` and no text/encoding/lineCount), so document
/// navigation returns a full `FilePreviewPayload` through this additive command.
/// No frozen DTO changed.
#[tauri::command]
pub async fn cmd_file_preview_open_child_document<R: tauri::Runtime>(
    window: tauri::Window<R>,
    service: tauri::State<'_, Arc<FilePreviewService>>,
    parent_handle: String,
    relative_path: String,
) -> Result<FilePreviewPayload, IpcError> {
    service
        .open_child_document(window.label(), &parent_handle, &relative_path)
        .await
}

/// Idempotent revocation of one capability owned by the calling window.
#[tauri::command]
pub async fn cmd_file_preview_close<R: tauri::Runtime>(
    window: tauri::Window<R>,
    service: tauri::State<'_, Arc<FilePreviewService>>,
    handle: String,
) -> Result<(), IpcError> {
    service.close(window.label(), &handle);
    Ok(())
}

#[cfg(test)]
#[path = "file_preview_tests.rs"]
mod file_preview_tests;
