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
pub use crate::ipc::file_preview_contract::FilePreviewTarget;
use crate::ipc::file_preview_contract::{
    limits, preview_error, FilePreviewChildAsset, FilePreviewEncoding, FilePreviewErrorReason,
    FilePreviewKind, FilePreviewPayload,
};
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
#[cfg(unix)]
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
    "heic", "heif", "avif", "tiff", "tif", "icns", "psd", "mkv", "avi", "wmv", "flv",
    "mpg", "mpeg", "3gp", "zip", "gz",
    "bz2", "xz", "tar", "7z", "rar", "exe", "dll", "so", "dylib", "wasm", "bin", "class", "jar",
    "o", "a", "pyc", "woff", "woff2", "ttf", "otf",
];

// ---------------------------------------------------------------- pure logic

/// Outcome of parsing a single HTTP `Range` header against a known length.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RangeOutcome {
    /// Inclusive byte offsets, already clamped to the file.
    Satisfiable { start: u64, end: u64 },
    /// Syntactically valid but not satisfiable: the caller answers 416 with
    /// `bytes */length`.
    Unsatisfiable,
    /// Unsupported per RFC 9110: unknown units, multiple ranges, or a
    /// malformed specifier. The caller ignores the header and serves 200.
    Ignored,
}

/// Parses `bytes=start-end`, `bytes=start-` and `bytes=-suffix`.
///
/// RFC 9110 conformance: multiple ranges, non-`bytes` units, reversed bounds and
/// malformed numbers are [`RangeOutcome::Ignored`] (serve 200). Only offsets at
/// or past EOF and zero suffix lengths are [`RangeOutcome::Unsatisfiable`] (416). and `bytes */length`.
pub fn parse_range_header(raw: &str, len: u64) -> RangeOutcome {
    // RFC 9110 14.2: a server that does not support a ranges-specifier
    // IGNORES it. Only a well-formed single `bytes=` range that lands past
    // EOF (or a zero suffix length) is unsatisfiable and answered with 416.
    let Some(spec) = raw.trim().strip_prefix("bytes=") else {
        return RangeOutcome::Ignored;
    };
    if spec.contains(',') {
        return RangeOutcome::Ignored;
    }
    let Some((first, last)) = spec.split_once('-') else {
        return RangeOutcome::Ignored;
    };
    let (first, last) = (first.trim(), last.trim());
    let parse = |value: &str| value.parse::<u64>().ok();

    let (start, end) = match (first.is_empty(), last.is_empty()) {
        // suffix form: last `n` bytes
        (true, false) => {
            let Some(suffix) = parse(last) else {
                return RangeOutcome::Ignored;
            };
            if suffix == 0 || len == 0 {
                return RangeOutcome::Unsatisfiable;
            }
            (len.saturating_sub(suffix), len - 1)
        }
        // open-ended form: from `start` to EOF
        (false, true) => {
            let Some(start) = parse(first) else {
                return RangeOutcome::Ignored;
            };
            if start >= len {
                return RangeOutcome::Unsatisfiable;
            }
            (start, len - 1)
        }
        // closed form
        (false, false) => {
            let (Some(start), Some(end)) = (parse(first), parse(last)) else {
                return RangeOutcome::Ignored;
            };
            if start > end {
                // RFC 9110 14.1.2: last-byte-pos < first-byte-pos is invalid -> ignored
                return RangeOutcome::Ignored;
            }
            if start >= len {
                return RangeOutcome::Unsatisfiable;
            }
            (start, end.min(len - 1))
        }
        (true, true) => return RangeOutcome::Ignored,
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
/// previewable. Unknown extensions are treated as text. SVG is an image via
/// `<img>`, never an active document, so scripts in the file do not run.
pub fn classify_extension(name: &str) -> Option<FilePreviewKind> {
    let ext = extension_of(name);
    match ext.as_str() {
        "md" | "markdown" => Some(FilePreviewKind::Markdown),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" | "svg" => {
            Some(FilePreviewKind::Image)
        }
        "mp4" | "m4v" | "mov" | "webm" | "ogv" => Some(FilePreviewKind::Video),
        "mp3" | "m4a" | "wav" | "flac" | "aac" | "ogg" | "opus" | "oga" => {
            Some(FilePreviewKind::Audio)
        }
        "pdf" => Some(FilePreviewKind::Pdf),
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

pub fn audio_media_type(name: &str) -> Option<&'static str> {
    match extension_of(name).as_str() {
        "mp3" => Some("audio/mpeg"),
        "m4a" | "aac" => Some("audio/mp4"),
        "wav" => Some("audio/wav"),
        "flac" => Some("audio/flac"),
        "ogg" | "opus" | "oga" => Some("audio/ogg"),
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
    Some(u32::from_le_bytes([*bytes.get(at)?, *bytes.get(at + 1)?, *bytes.get(at + 2)?, 0]) as u64)
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
    if bytes.starts_with(b"BM") && bytes.len() >= 26 {
        let width = i32::from_le_bytes(bytes[18..22].try_into().ok()?);
        let height = i32::from_le_bytes(bytes[22..26].try_into().ok()?);
        if width > 0 && height != 0 {
            return Some(ImageSignature {
                media_type: "image/bmp",
                width: width as u64,
                height: height.unsigned_abs() as u64,
            });
        }
    }
    if bytes.len() >= 8 && bytes.starts_with(b"\x00\x00\x01\x00") {
        let width = match bytes[6] {
            0 => 256,
            n => u64::from(n),
        };
        let height = match bytes[7] {
            0 => 256,
            n => u64::from(n),
        };
        return Some(ImageSignature {
            media_type: "image/x-icon",
            width,
            height,
        });
    }
    if svg_probe(bytes) {
        return Some(ImageSignature {
            media_type: "image/svg+xml",
            width: 1,
            height: 1,
        });
    }
    None
}

fn svg_probe(bytes: &[u8]) -> bool {
    let end = bytes.len().min(512);
    let text = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
    text.contains("<svg")
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
        (
            decode_utf16(rest, u16::from_le_bytes)?,
            FilePreviewEncoding::Utf16Le,
        )
    } else if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        (
            decode_utf16(rest, u16::from_be_bytes)?,
            FilePreviewEncoding::Utf16Be,
        )
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

/// Counts lines with the same universal-newline rule the renderer uses:
/// \r\n, lone \r and lone \n each end exactly one line, so CR-only files
/// cannot exceed the rendered-line budget.
pub fn count_lines(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    let bytes = text.as_bytes();
    let mut lines = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'\r' => {
                lines += 1;
                if bytes.get(i + 1) == Some(&b'\n') {
                    i += 1;
                }
            }
            b'\n' => lines += 1,
            _ => {}
        }
        i += 1;
    }
    if bytes.ends_with(b"\n") || bytes.ends_with(b"\r") {
        lines
    } else {
        lines + 1
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
        return preview_error(
            FilePreviewErrorReason::RemoteUnsupported,
            error.message,
            None,
        );
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
    /// (device, inode) of the boundary directory captured at open. Unix only;
    /// other platforms fall back to canonical-path containment alone.
    parent_identity: Option<(u64, u64)>,
    revoked: Arc<AtomicBool>,
    /// Serializes cursor-moving media streams of this document: every range
    /// body holds this guard for its lifetime, so concurrent responses can
    /// never interleave seeks on the shared underlying file description.
    stream_lock: Arc<tokio::sync::Mutex<()>>,
    /// Milliseconds since service start of the last capability request.
    last_access_ms: AtomicU64,
}

#[derive(Default)]
struct WindowSlot {
    main: Option<String>,
    children: Vec<String>,
    /// Monotonic open generation per window. A late registration whose epoch
    /// is stale is discarded instead of revoking the newer preview.
    epoch: u64,
    /// Child opens currently in flight, counted atomically with the budget
    /// check so concurrent child requests cannot exceed the bound.
    pending_children: usize,
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
    parent_identity: Option<(u64, u64)>,
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
    /// Tab-scoped owner. Empty keeps the legacy one-slot-per-window behavior.
    pub owner_id: String,
    pub path: String,
    pub cwd: Option<PathBuf>,
    pub line: Option<u32>,
    pub col: Option<u32>,
}

fn slot_key(window_label: &str, owner_id: &str) -> String {
    if owner_id.is_empty() {
        window_label.to_string()
    } else {
        format!("{window_label}\0{owner_id}")
    }
}

fn window_owns_key(window_label: &str, key: &str) -> bool {
    key == window_label
        || key
            .strip_prefix(window_label)
            .is_some_and(|rest| rest.starts_with('\0'))
}

/// Handle registry plus the loopback capability server.
pub struct FilePreviewService {
    registry: Mutex<Registry>,
    origin: String,
    authority: String,
    media_gate: Arc<Semaphore>,
    /// Service-start instant; epoch for last_access_ms TTL stamps.
    started_at: std::time::Instant,
}

/// Idle time after which an unreferenced capability is revoked.
const HANDLE_TTL: std::time::Duration = std::time::Duration::from_secs(15 * 60);
/// Cadence of the TTL sweep.
const HANDLE_TTL_CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

impl FilePreviewService {
    /// Binds a dedicated listener on `127.0.0.1` with an OS-assigned port and
    /// serves capability URLs from it. This is not the remote gateway.
    pub async fn start() -> Result<Arc<Self>, IpcError> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|error| {
                IpcError::internal(format!("preview listener bind failed: {error}"))
            })?;
        let addr = listener.local_addr().map_err(|error| {
            IpcError::internal(format!("preview listener addr failed: {error}"))
        })?;
        let started_at = std::time::Instant::now();
        let service = Arc::new(Self {
            registry: Mutex::new(Registry::default()),
            origin: format!("http://{addr}"),
            authority: addr.to_string(),
            media_gate: Arc::new(Semaphore::new(limits::MAX_CONCURRENT_MEDIA_REQUESTS)),
            started_at,
        });
        // R6: a retained capability expires after HANDLE_TTL without use. The
        // sweeper revokes expired handles so descriptors never outlive their
        // lease, independent of client cleanup.
        let sweeper = Arc::clone(&service);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(HANDLE_TTL_CHECK_INTERVAL);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tick.tick().await;
                sweeper.revoke_expired();
            }
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
    pub async fn open(
        &self,
        request: FilePreviewOpenRequest,
    ) -> Result<FilePreviewPayload, IpcError> {
        ensure_trusted_window(&request.window_label)?;
        let FilePreviewOpenRequest {
            window_label,
            owner_id,
            path,
            cwd,
            line,
            col,
        } = request;
        let slot = slot_key(&window_label, &owner_id);
        // R3: take the open epoch BEFORE awaited I/O. If a newer open for the
        // same owner completes first, this registration is stale and must be
        // discarded instead of revoking the newer preview.
        let epoch = self.begin_main_open(&slot);
        let opened =
            crate::ipc::run_blocking(move || open_blocking(&path, cwd.as_deref(), line, col))
                .await?;
        self.register_main(&slot, opened, epoch)
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
        let slot = self.parent_slot_key(window_label, parent_handle)?;
        let boundary = self.markdown_parent_dir(window_label, parent_handle)?;
        // R4: reserve a child slot atomically with the budget check so
        // concurrent child opens cannot oversubscribe the bound.
        let reservation = self.reserve_child_slot(&slot)?;
        let relative = relative_path.to_string();
        let boundary_for_io = boundary.clone();
        let opened = crate::ipc::run_blocking(move || {
            let path = resolve_contained_child(&boundary_for_io, &relative)?;
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
        .await;
        let opened = match opened {
            Ok(opened) => opened,
            Err(error) => {
                self.refund_child_slot(&slot, reservation);
                return Err(error);
            }
        };
        // R5: the boundary directory must still be the same object it was at
        // open time; a rename+symlink swap during the awaited I/O is refused.
        if !boundary_identity_holds_after_io(&boundary) {
            self.refund_child_slot(&slot, reservation);
            return Err(outside_boundary(relative_path));
        }

        let media_type = opened.media_type.unwrap_or("application/octet-stream");
        let byte_length = opened.byte_length;
        let display_name = opened.display_name.clone();
        let handle = self.register_child(&slot, parent_handle, opened, reservation)?;
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
        let slot = self.parent_slot_key(window_label, parent_handle)?;
        let boundary = self.markdown_parent_dir(window_label, parent_handle)?;
        let epoch = self.begin_main_open(&slot);
        let relative = relative_path.to_string();
        let boundary_for_io = boundary.clone();
        let opened = crate::ipc::run_blocking(move || {
            let path = resolve_contained_child(&boundary_for_io, &relative)?;
            let opened = open_blocking_path(&path, None, None)?;
            if !matches!(
                opened.kind,
                FilePreviewKind::Markdown | FilePreviewKind::Text
            ) {
                return Err(preview_error(
                    FilePreviewErrorReason::UnsupportedFormat,
                    "markdown document links must target a text or markdown document",
                    Some(json!({ "displayName": opened.display_name })),
                ));
            }
            Ok(opened)
        })
        .await?;
        if !boundary_identity_holds_after_io(&boundary) {
            return Err(outside_boundary(relative_path));
        }
        self.register_main(&slot, opened, epoch)
    }

    /// Revokes one handle owned by `window_label`. Idempotent, and a window can
    /// never revoke another window's capability.
    pub fn close(&self, window_label: &str, handle: &str) {
        let mut registry = self.registry.lock();
        let Some(key) = registry.windows.iter().find_map(|(key, slot)| {
            if !window_owns_key(window_label, key) {
                return None;
            }
            if slot.main.as_deref() == Some(handle) || slot.children.iter().any(|child| child == handle) {
                Some(key.clone())
            } else {
                None
            }
        }) else {
            return;
        };
        let Some(slot) = registry.windows.get_mut(&key) else {
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
        let keys: Vec<String> = registry
            .windows
            .keys()
            .filter(|key| window_owns_key(window_label, key))
            .cloned()
            .collect();
        for key in keys {
            let Some(slot) = registry.windows.remove(&key) else {
                continue;
            };
            for handle in slot.children.into_iter().chain(slot.main) {
                registry.revoke(&handle);
            }
        }
    }

    fn parent_slot_key(&self, window_label: &str, parent_handle: &str) -> Result<String, IpcError> {
        let registry = self.registry.lock();
        registry
            .windows
            .iter()
            .find_map(|(key, slot)| {
                if window_owns_key(window_label, key) && slot.main.as_deref() == Some(parent_handle) {
                    Some(key.clone())
                } else {
                    None
                }
            })
            .ok_or_else(|| {
                preview_error(
                    FilePreviewErrorReason::ExpiredHandle,
                    "preview handle is unknown or has been revoked",
                    None,
                )
            })
    }

    fn markdown_parent_dir(
        &self,
        window_label: &str,
        parent_handle: &str,
    ) -> Result<ChildBoundary, IpcError> {
        let registry = self.registry.lock();
        let is_current_main = registry.windows.iter().any(|(key, slot)| {
            window_owns_key(window_label, key) && slot.main.as_deref() == Some(parent_handle)
        });
        let record = registry.handles.get(parent_handle);
        match (is_current_main, record) {
            (true, Some(record)) => {
                let dir = record.parent_dir.clone().ok_or_else(|| {
                    preview_error(
                        FilePreviewErrorReason::PermissionDenied,
                        "this preview does not own a document boundary",
                        None,
                    )
                })?;
                // R5: the boundary carries the retained identity of the
                // document directory, not just a mutable path.
                Ok(ChildBoundary {
                    dir,
                    identity: record.parent_identity,
                })
            }
            _ => Err(preview_error(
                FilePreviewErrorReason::ExpiredHandle,
                "preview handle is unknown or has been revoked",
                None,
            )),
        }
    }

    /// R4: atomically counts this open against the child budget. The slot is
    /// refunded on failure and consumed at registration.
    fn reserve_child_slot(&self, window_label: &str) -> Result<ChildReservation, IpcError> {
        let mut registry = self.registry.lock();
        let slot = registry
            .windows
            .entry(window_label.to_string())
            .or_default();
        let used = slot.children.len() + slot.pending_children;
        if used >= limits::MAX_CHILD_HANDLES {
            return Err(preview_error(
                FilePreviewErrorReason::TooLarge,
                "this document already holds the maximum number of image capabilities",
                Some(json!({ "limit": limits::MAX_CHILD_HANDLES })),
            ));
        }
        slot.pending_children += 1;
        Ok(ChildReservation(window_label.to_string()))
    }

    fn refund_child_slot(&self, window_label: &str, reservation: ChildReservation) {
        let _ = reservation;
        let mut registry = self.registry.lock();
        if let Some(slot) = registry.windows.get_mut(window_label) {
            slot.pending_children = slot.pending_children.saturating_sub(1);
        }
    }

    fn begin_main_open(&self, window_label: &str) -> u64 {
        let mut registry = self.registry.lock();
        let slot = registry
            .windows
            .entry(window_label.to_string())
            .or_default();
        slot.epoch += 1;
        slot.epoch
    }

    fn register_main(
        &self,
        window_label: &str,
        opened: OpenedFile,
        epoch: u64,
    ) -> Result<FilePreviewPayload, IpcError> {
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
        if previous.epoch != epoch {
            // R3: a newer open already owns this window; drop the stale
            // descriptor instead of revoking the newer preview.
            drop(opened);
            return Err(preview_error(
                FilePreviewErrorReason::ExpiredHandle,
                "this preview request was superseded by a newer one",
                None,
            ));
        }
        let stale_main = previous.main.take();
        let stale_children = std::mem::take(&mut previous.children);
        previous.main = Some(handle.clone());
        for stale in stale_children.into_iter().chain(stale_main) {
            registry.revoke(&stale);
        }
        registry
            .handles
            .insert(handle, record_of(opened, self.started_at));
        Ok(payload)
    }

    fn register_child(
        &self,
        window_label: &str,
        parent_handle: &str,
        opened: OpenedFile,
        reservation: ChildReservation,
    ) -> Result<String, IpcError> {
        let handle = new_handle();
        let mut registry = self.registry.lock();
        let slot = registry
            .windows
            .entry(window_label.to_string())
            .or_default();
        // R4: consume the reservation and re-verify the parent is still the
        // window's current main; a late child never attaches to a replaced
        // document.
        if slot.main.as_deref() != Some(parent_handle) {
            slot.pending_children = slot.pending_children.saturating_sub(1);
            drop(opened);
            return Err(preview_error(
                FilePreviewErrorReason::ExpiredHandle,
                "the parent preview was replaced before the child could load",
                None,
            ));
        }
        slot.pending_children = slot.pending_children.saturating_sub(1);
        let _ = reservation;
        slot.children.push(handle.clone());
        registry
            .handles
            .insert(handle.clone(), record_of(opened, self.started_at));
        Ok(handle)
    }

    /// R6: revokes capabilities whose lease expired without any request.
    fn revoke_expired(&self) {
        let now_ms = self.started_at.elapsed().as_millis() as u64;
        let mut expired: Vec<String> = Vec::new();
        {
            let registry = self.registry.lock();
            for (handle, record) in registry.handles.iter() {
                if now_ms.saturating_sub(record.last_access_ms.load(Ordering::Relaxed))
                    > HANDLE_TTL.as_millis() as u64
                {
                    expired.push(handle.clone());
                }
            }
        }
        if expired.is_empty() {
            return;
        }
        let mut registry = self.registry.lock();
        for handle in expired {
            if registry.handles.contains_key(&handle) {
                registry.revoke(&handle);
                for slot in registry.windows.values_mut() {
                    if slot.main.as_deref() == Some(handle.as_str()) {
                        slot.main = None;
                    }
                    slot.children.retain(|child| child != &handle);
                }
            }
        }
    }
}

fn record_of(opened: OpenedFile, started_at: std::time::Instant) -> Arc<HandleRecord> {
    Arc::new(HandleRecord {
        file: opened.file,
        byte_length: opened.byte_length,
        media_type: opened.media_type,
        kind: opened.kind,
        parent_dir: opened.parent_dir,
        parent_identity: opened.parent_identity,
        revoked: Arc::new(AtomicBool::new(false)),
        stream_lock: Arc::new(tokio::sync::Mutex::new(())),
        last_access_ms: AtomicU64::new(started_at.elapsed().as_millis() as u64),
    })
}

/// (device, inode) of a path, when the platform exposes handle identity.
fn directory_identity(path: &Path) -> Option<(u64, u64)> {
    path_identity(path)
}

fn path_identity(path: &Path) -> Option<(u64, u64)> {
    #[cfg(unix)]
    {
        std::fs::metadata(path).ok().map(|m| (m.dev(), m.ino()))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

/// True when the path still names the object captured as identity.
fn identity_holds(path: &Path, identity: Option<(u64, u64)>) -> bool {
    match identity {
        // Non-Unix platforms keep canonical-path containment only.
        None => true,
        Some(expected) => path_identity(path) == Some(expected),
    }
}

/// The retained identity of a Markdown document's parent directory.
#[derive(Clone)]
struct ChildBoundary {
    dir: PathBuf,
    identity: Option<(u64, u64)>,
}

/// A counted reservation of one child-handle budget slot.
struct ChildReservation(String);

/// Re-checks the boundary directory identity after awaited I/O. This closes
/// the rename+symlink swap window that path-only containment leaves open.
fn boundary_identity_holds_after_io(boundary: &ChildBoundary) -> bool {
    match boundary.identity {
        // Non-Unix platforms keep canonical-path containment only.
        None => true,
        Some(expected) => path_identity(&boundary.dir) == Some(expected),
    }
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
                return Err(too_large(
                    &display_name,
                    byte_length,
                    limits::TEXT_MAX_BYTES,
                ));
            }
            // R7: the read itself is capped at the ceiling plus one byte, so a
            // file that GROWS between the stat above and this read can never
            // drive an unbounded allocation. The surplus byte proves overflow.
            let mut bytes = Vec::with_capacity(byte_length as usize);
            let mut limited = (&mut file).take(limits::TEXT_MAX_BYTES + 1);
            limited
                .read_to_end(&mut bytes)
                .map_err(|error| io_failure(&error, &canonical))?;
            if bytes.len() as u64 > limits::TEXT_MAX_BYTES {
                return Err(too_large(
                    &display_name,
                    bytes.len() as u64,
                    limits::TEXT_MAX_BYTES,
                ));
            }
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
                // R5: retain the boundary directory's (dev, inode) identity
                // so later child resolution can detect a swapped path.
                parent_identity: match kind {
                    FilePreviewKind::Markdown => canonical.parent().and_then(directory_identity),
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
                return Err(too_large(
                    &display_name,
                    byte_length,
                    limits::IMAGE_MAX_BYTES,
                ));
            }
            // R12: probe up to IMAGE_HEADER_PROBE bytes with a full read
            // loop, so a JPEG whose SOF marker sits behind large EXIF/ICC
            // segments is still measured. The probe stays bounded.
            let probe_len = byte_length.min(limits::IMAGE_HEADER_PROBE as u64) as usize;
            let mut header = vec![0u8; probe_len];
            let mut filled = 0usize;
            while filled < probe_len {
                let read = file
                    .read(&mut header[filled..])
                    .map_err(|error| io_failure(&error, &canonical))?;
                if read == 0 {
                    break;
                }
                filled += read;
            }
            header.truncate(filled);
            let signature = image_signature(&header).ok_or_else(|| {
                preview_error(
                    FilePreviewErrorReason::UnsupportedFormat,
                    "this image format is not supported",
                    Some(json!({ "displayName": display_name })),
                )
            })?;
            // R12: width and height are bounded per axis as well as by the
            // total pixel product.
            if signature.width > limits::IMAGE_MAX_AXIS as u64
                || signature.height > limits::IMAGE_MAX_AXIS as u64
            {
                return Err(preview_error(
                    FilePreviewErrorReason::TooLarge,
                    "this image exceeds the supported per-axis pixel bound",
                    Some(json!({
                        "displayName": display_name,
                        "width": signature.width,
                        "height": signature.height,
                        "limit": limits::IMAGE_MAX_AXIS,
                    })),
                ));
            }
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
                parent_identity: None,
                text: None,
                encoding: None,
                line_count: None,
                target: None,
            })
        }
        FilePreviewKind::Video | FilePreviewKind::Audio | FilePreviewKind::Pdf => {
            let media_type = match kind {
                FilePreviewKind::Video => video_media_type(&display_name),
                FilePreviewKind::Audio => audio_media_type(&display_name),
                FilePreviewKind::Pdf => Some("application/pdf"),
                _ => None,
            }
            .ok_or_else(|| {
                preview_error(
                    FilePreviewErrorReason::UnsupportedFormat,
                    "this media container is not supported",
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
                parent_identity: None,
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
fn resolve_contained_child(boundary: &ChildBoundary, relative: &str) -> Result<PathBuf, IpcError> {
    // R5: the stored boundary path must still BE the directory object the
    // document was opened from; a rename+symlink swap moving both the stored
    // path and the candidate elsewhere is refused before canonicalization.
    if !boundary_identity_holds_after_io(boundary) {
        return Err(outside_boundary(relative));
    }
    let relative = crate::ipc::file_link::sanitize_path_token(relative)?;
    crate::ipc::file_link::reject_remote_path_spec(relative).map_err(map_session_error)?;
    if crate::ipc::file_link::is_absolute_token(relative) || relative.starts_with('~') {
        return Err(outside_boundary(relative));
    }
    let candidate = boundary.dir.join(&relative);
    let canonical = std::fs::canonicalize(&candidate).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => preview_error(
            FilePreviewErrorReason::MissingFile,
            "this document reference does not exist",
            Some(json!({ "displayName": relative })),
        ),
        _ => outside_boundary(relative),
    })?;
    if !canonical.starts_with(&boundary.dir) {
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
        HeaderValue::from_str(media_type)
            .unwrap_or(HeaderValue::from_static("application/octet-stream")),
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
    let host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok());
    if host != Some(service.authority.as_str()) {
        return status_only(StatusCode::FORBIDDEN);
    }
    // 2. allowlisted Origin; absent Origin stays allowed for native media
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok());
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
    // R6: every capability request refreshes the idle lease.
    record.last_access_ms.store(
        service.started_at.elapsed().as_millis() as u64,
        Ordering::Relaxed,
    );
    // 4. the retained inode must still be the file we measured. R10: the
    // metadata probe runs on the blocking pool, never on the async reactor.
    let probe = record.file.try_clone();
    let current_len = match probe {
        Ok(clone) => {
            match crate::ipc::run_blocking(move || {
                clone
                    .metadata()
                    .map(|metadata| metadata.len())
                    .map_err(|error| io_failure(&error, Path::new("")))
            })
            .await
            {
                Ok(len) => len,
                _ => return status_only(StatusCode::CONFLICT),
            }
        }
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
    // R11: HEAD never processes a Range header. RFC 9110 14.2 defines Range
    // semantics for GET; evaluating it on HEAD leaks 416s to probes.
    let range = if method == Method::HEAD {
        None
    } else {
        headers
            .get(header::RANGE)
            .and_then(|value| value.to_str().ok())
            .map(|raw| parse_range_header(raw, length))
    };

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
        // None and Some(RangeOutcome::Ignored) both serve the whole body.
        _ => (StatusCode::OK, 0, length.saturating_sub(1)),
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
    // R2: the stream owns the per-record lock for its whole lifetime. Rust
    // File clones share the underlying file description cursor, so without
    // this guard two concurrent range responses would interleave seeks and
    // read each other's bytes.
    let stream_guard = record.stream_lock.clone().lock_owned().await;
    *response.body_mut() = Body::from_stream(chunk_stream(
        tokio::fs::File::from_std(clone),
        start,
        body_length,
        Arc::clone(&record.revoked),
        permit,
        stream_guard,
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
    /// Held for the whole stream; see serve_capability (R2).
    _stream_guard: tokio::sync::OwnedMutexGuard<()>,
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
    stream_guard: tokio::sync::OwnedMutexGuard<()>,
) -> impl futures_util::Stream<Item = Result<Bytes, std::io::Error>> {
    futures_util::stream::unfold(
        ChunkState {
            file,
            remaining,
            started: false,
            offset,
            revoked,
            permit: Some(permit),
            _stream_guard: stream_guard,
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
                if let Err(error) = state
                    .file
                    .seek(std::io::SeekFrom::Start(state.offset))
                    .await
                {
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
    webview: tauri::Webview<R>,
    service: tauri::State<'_, Arc<FilePreviewService>>,
    daemon_client: tauri::State<'_, Arc<crate::daemon::DaemonClient>>,
    owner_id: Option<String>,
    path: String,
    backend_session_id: String,
    line: Option<u32>,
    col: Option<u32>,
) -> Result<FilePreviewPayload, IpcError> {
    let window_label = webview.label().to_string();
    ensure_trusted_window(&window_label)?;
    let cwd = crate::ipc::file_preview_contract::resolve_local_session_cwd(
        Some(daemon_client.inner()),
        &backend_session_id,
    )
    .await
    .map_err(map_session_error)?;
    if cwd.is_none() && !crate::ipc::file_link::is_absolute_token(&path) && !path.starts_with('~') {
        return Err(preview_error(
            FilePreviewErrorReason::MissingFile,
            "The terminal's current directory could not be read. Use an absolute file path.",
            None,
        ));
    }
    service
        .open(FilePreviewOpenRequest {
            window_label,
            owner_id: owner_id.unwrap_or_default(),
            path,
            cwd,
            line,
            col,
        })
        .await
}

/// Markdown child image capability under the open document's directory.
#[tauri::command]
pub async fn cmd_file_preview_open_child<R: tauri::Runtime>(
    webview: tauri::Webview<R>,
    service: tauri::State<'_, Arc<FilePreviewService>>,
    parent_handle: String,
    relative_path: String,
) -> Result<FilePreviewChildAsset, IpcError> {
    service
        .open_child_image(webview.label(), &parent_handle, &relative_path)
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
    webview: tauri::Webview<R>,
    service: tauri::State<'_, Arc<FilePreviewService>>,
    parent_handle: String,
    relative_path: String,
) -> Result<FilePreviewPayload, IpcError> {
    service
        .open_child_document(webview.label(), &parent_handle, &relative_path)
        .await
}

/// Idempotent revocation of one capability owned by the calling window.
#[tauri::command]
pub async fn cmd_file_preview_close<R: tauri::Runtime>(
    webview: tauri::Webview<R>,
    service: tauri::State<'_, Arc<FilePreviewService>>,
    handle: String,
) -> Result<(), IpcError> {
    service.close(webview.label(), &handle);
    Ok(())
}

#[cfg(test)]
#[path = "file_preview_tests.rs"]
mod file_preview_tests;
