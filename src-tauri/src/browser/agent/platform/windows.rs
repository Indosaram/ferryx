//! Windows native browser capture (plan C4, task T7b): WebView2
//! `ICoreWebView2::CapturePreview` into an in-memory `IStream`, returned as the shared
//! [`BrowserSnapshot`] bytes type.
//!
//! Threading: `with_webview` runs the dispatch closure on the WebView2 UI thread. Every COM
//! object (controller, core webview, stream, completion handler) is created, used and
//! released there; the completion handler copies the stream into a Rust `Vec<u8>` before it
//! returns, and only that `Vec<u8>` leaves the UI thread. Optional downscaling and JPEG
//! transcoding run on a blocking worker, never on the UI thread. The result is delivered
//! through [`SnapshotCallbackCoordinator`], so the existing timeout, late-callback fence and
//! permit release apply unchanged. Nothing touches the filesystem.

use crate::browser::snapshot_source::{
    await_coordinator_completion, clamp_capture_dimensions, BrowserSnapshot,
    SnapshotCallbackCoordinator, SnapshotFormat, SnapshotOptions, MAX_CAPTURE_EDGE,
};
use crate::browser::BrowserError;
use crate::ipc::error::{IpcError, IpcErrorCode};
use std::time::Duration;
use tauri::{AppHandle, Manager};
use webview2_com::CapturePreviewCompletedHandler;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2Controller, COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
};
use windows::Win32::Foundation::HGLOBAL;
use windows::Win32::System::Com::StructuredStorage::CreateStreamOnHGlobal;
use windows::Win32::System::Com::{IStream, STREAM_SEEK_SET};

/// The eight-byte PNG file signature.
pub const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

/// Upper bound on the encoded preview copied out of the stream. A 2048x2048 RGBA frame is
/// 16 MiB uncompressed; a PNG of a high-DPI viewport stays well below this.
pub const MAX_PREVIEW_BYTES: usize = 64 * 1024 * 1024;

const STREAM_READ_CHUNK: usize = 64 * 1024;

fn capture_error(message: impl Into<String>) -> IpcError {
    IpcError::new(IpcErrorCode::BrowserScreenshotFailed, message)
}

// ---------------------------------------------------------------------------------------
// Pure helpers (unit-tested).
// ---------------------------------------------------------------------------------------

/// True when `bytes` starts with the PNG signature.
pub fn is_png(bytes: &[u8]) -> bool {
    bytes.starts_with(&PNG_SIGNATURE)
}

/// Width and height from the PNG IHDR chunk, which the format requires to come first.
pub fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if !is_png(bytes) || bytes.len() < 24 || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    if width == 0 || height == 0 {
        return None;
    }
    Some((width, height))
}

/// Output size for a `width` x `height` preview under `options`. Only downscales, keeps the
/// aspect ratio and respects the shared capture limits; without bounds the native size is kept.
pub fn target_dimensions(width: u32, height: u32, options: SnapshotOptions) -> (u32, u32) {
    let (max_width, max_height) = match (options.max_width, options.max_height) {
        (None, None) => return (width, height),
        (max_width, max_height) => (
            max_width.unwrap_or(MAX_CAPTURE_EDGE).max(1),
            max_height.unwrap_or(MAX_CAPTURE_EDGE).max(1),
        ),
    };
    if width <= max_width && height <= max_height {
        return clamp_capture_dimensions(width, height);
    }
    let scale =
        (f64::from(max_width) / f64::from(width)).min(f64::from(max_height) / f64::from(height));
    let scaled_width = ((f64::from(width) * scale).round() as u32).clamp(1, max_width);
    let scaled_height = ((f64::from(height) * scale).round() as u32).clamp(1, max_height);
    clamp_capture_dimensions(scaled_width, scaled_height)
}

/// Turns the PNG copied out of the preview stream into the requested snapshot. A PNG request
/// that needs no downscale returns the native bytes untouched; anything else is decoded,
/// resized and re-encoded in memory.
pub fn encode_preview(png: Vec<u8>, options: SnapshotOptions) -> Result<BrowserSnapshot, IpcError> {
    if png.is_empty() {
        return Err(capture_error("empty snapshot generated"));
    }
    let (width, height) = png_dimensions(&png)
        .ok_or_else(|| capture_error("WebView2 preview is not a valid PNG image"))?;
    let (target_width, target_height) = target_dimensions(width, height, options);
    let resized = (target_width, target_height) != (width, height);
    if options.format == SnapshotFormat::Png && !resized {
        return Ok(BrowserSnapshot::new(
            png,
            SnapshotFormat::Png,
            width,
            height,
        ));
    }

    let decoded = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
        .map_err(|error| capture_error(format!("cannot decode snapshot bitmap image: {error}")))?;
    let image = if resized {
        decoded.resize_exact(
            target_width,
            target_height,
            image::imageops::FilterType::Triangle,
        )
    } else {
        decoded
    };

    let mut bytes = Vec::new();
    match options.format {
        SnapshotFormat::Png => image
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .map_err(|error| capture_error(format!("cannot encode snapshot to PNG: {error}")))?,
        SnapshotFormat::Jpeg { quality } => image
            .to_rgb8()
            .write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(
                &mut bytes,
                quality.clamp(1, 100),
            ))
            .map_err(|error| capture_error(format!("cannot encode snapshot to JPEG: {error}")))?,
    }
    if bytes.is_empty() {
        return Err(capture_error("empty snapshot generated"));
    }
    Ok(BrowserSnapshot::new(
        bytes,
        options.format,
        image.width(),
        image.height(),
    ))
}

/// Maps the raw completion outcome (copied bytes or a native error text) to the snapshot.
pub fn preview_result(
    raw: Result<Vec<u8>, String>,
    options: SnapshotOptions,
) -> Result<BrowserSnapshot, IpcError> {
    raw.map_err(capture_error)
        .and_then(|png| encode_preview(png, options))
}

// ---------------------------------------------------------------------------------------
// COM glue (UI thread only).
// ---------------------------------------------------------------------------------------

/// Copies the whole stream, from offset 0, into a Rust buffer. Call on the thread that owns
/// `stream`.
pub fn read_stream_to_vec(stream: &IStream) -> Result<Vec<u8>, String> {
    // SAFETY: `stream` is a live IStream owned by the caller on the current thread.
    unsafe { stream.Seek(0, STREAM_SEEK_SET, None) }
        .map_err(|error| format!("cannot rewind preview stream: {error}"))?;
    let mut bytes = Vec::new();
    let mut chunk = vec![0u8; STREAM_READ_CHUNK];
    loop {
        let mut read = 0u32;
        // SAFETY: `chunk` is a writable buffer of `chunk.len()` bytes and `read` outlives the
        // call; the stream never writes more than `cb` bytes.
        unsafe {
            stream.Read(
                chunk.as_mut_ptr().cast(),
                chunk.len() as u32,
                Some(&mut read as *mut u32),
            )
        }
        .ok()
        .map_err(|error| format!("cannot read preview stream: {error}"))?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read as usize]);
        if bytes.len() > MAX_PREVIEW_BYTES {
            return Err(format!(
                "preview stream exceeds the {MAX_PREVIEW_BYTES}-byte capture limit"
            ));
        }
    }
    Ok(bytes)
}

/// Hands the copied bytes to a blocking worker for encoding, then completes the coordinator.
/// Only `Vec<u8>`, `String` and plain options cross the thread boundary.
fn deliver(
    coordinator: SnapshotCallbackCoordinator,
    raw: Result<Vec<u8>, String>,
    options: SnapshotOptions,
) {
    let _encoding = tauri::async_runtime::spawn_blocking(move || {
        let _ = coordinator.complete(preview_result(raw, options));
    });
}

/// Starts one CapturePreview into a fresh in-memory stream. Must run on the WebView2 UI
/// thread. On `Ok`, the completion handler completes `coordinator` exactly once; on `Err`,
/// the caller completes it.
fn start_capture(
    controller: &ICoreWebView2Controller,
    coordinator: &SnapshotCallbackCoordinator,
    options: SnapshotOptions,
) -> Result<(), IpcError> {
    // SAFETY: called on the UI thread that owns `controller` (inside `with_webview`).
    let core = unsafe { controller.CoreWebView2() }
        .map_err(|error| capture_error(format!("WebView2 core is unavailable: {error}")))?;
    // SAFETY: a null HGLOBAL asks COM to allocate a growable memory block that the stream
    // owns and frees on its final release.
    let stream = unsafe { CreateStreamOnHGlobal(HGLOBAL::default(), true) }
        .map_err(|error| capture_error(format!("cannot create preview stream: {error}")))?;

    let handler_stream = stream.clone();
    let handler_coordinator = coordinator.clone();
    let handler = CapturePreviewCompletedHandler::create(Box::new(
        move |result: windows::core::Result<()>| {
            let raw = result
                .map_err(|error| format!("WebView2 CapturePreview failed: {error}"))
                .and_then(|()| read_stream_to_vec(&handler_stream));
            deliver(handler_coordinator, raw, options);
            Ok(())
        },
    ));

    // SAFETY: UI thread; WebView2 AddRefs `stream` and `handler` for the async operation.
    unsafe {
        core.CapturePreview(
            COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
            &stream,
            &handler,
        )
    }
    .map_err(|error| capture_error(format!("WebView2 CapturePreview failed: {error}")))
}

/// Windows counterpart of the macOS `capture_webview_snapshot` in `snapshot_source.rs`.
pub async fn capture_webview_snapshot<R: tauri::Runtime>(
    app: &AppHandle<R>,
    webview_label: &str,
    options: SnapshotOptions,
    timeout: Duration,
) -> Result<BrowserSnapshot, IpcError> {
    capture_webview_snapshot_with_coordinator(app, webview_label, options, timeout, None).await
}

/// Windows counterpart of the macOS `capture_webview_snapshot_with_coordinator`: same
/// signature and the same coordinator contract (attach channel, single delivery, timeout
/// fence).
pub async fn capture_webview_snapshot_with_coordinator<R: tauri::Runtime>(
    app: &AppHandle<R>,
    webview_label: &str,
    options: SnapshotOptions,
    timeout: Duration,
    coordinator_opt: Option<SnapshotCallbackCoordinator>,
) -> Result<BrowserSnapshot, IpcError> {
    let webview = app
        .get_webview(webview_label)
        .ok_or_else(|| BrowserError::WebviewNotFound(webview_label.to_string()))?;

    let (coordinator, rx) = match coordinator_opt {
        Some(c) => {
            let rx = c.attach_channel();
            (c, rx)
        }
        None => SnapshotCallbackCoordinator::new(),
    };
    let coordinator_clone = coordinator.clone();

    webview
        .with_webview(move |platform| {
            if let Err(error) = start_capture(&platform.controller(), &coordinator, options) {
                let _ = coordinator.complete(Err(error));
            }
        })
        .map_err(|e| capture_error(format!("failed to attach webview snapshot handler: {e}")))?;

    await_coordinator_completion(&coordinator_clone, rx, timeout).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::snapshot_source::sample_valid_png_bytes;

    fn png_of(width: u32, height: u32) -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(width, height, image::Rgba([10, 20, 30, 255]));
        let mut bytes = Vec::new();
        image
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .expect("encode test PNG");
        bytes
    }

    #[test]
    fn png_signature_and_dimensions() {
        let sample = sample_valid_png_bytes();
        assert!(is_png(&sample));
        assert_eq!(png_dimensions(&sample), Some((1, 1)));
        assert_eq!(png_dimensions(&png_of(7, 3)), Some((7, 3)));

        assert!(!is_png(&[]));
        assert!(!is_png(&[0xff, 0xd8, 0xff]));
        assert_eq!(png_dimensions(&sample[..20]), None, "truncated IHDR");
        let mut not_ihdr = sample.clone();
        not_ihdr[12..16].copy_from_slice(b"IDAT");
        assert_eq!(png_dimensions(&not_ihdr), None);
        let mut zero_width = sample;
        zero_width[16..20].copy_from_slice(&[0, 0, 0, 0]);
        assert_eq!(png_dimensions(&zero_width), None);
    }

    #[test]
    fn target_dimensions_only_downscales_and_keeps_aspect() {
        assert_eq!(
            target_dimensions(1600, 900, SnapshotOptions::png()),
            (1600, 900)
        );
        assert_eq!(
            target_dimensions(800, 600, SnapshotOptions::png().with_bounds(1024, 768)),
            (800, 600)
        );
        assert_eq!(
            target_dimensions(2000, 1000, SnapshotOptions::png().with_bounds(1000, 1000)),
            (1000, 500)
        );
        let width_only = SnapshotOptions {
            max_width: Some(400),
            ..SnapshotOptions::png()
        };
        assert_eq!(target_dimensions(800, 200, width_only), (400, 100));
    }

    #[test]
    fn png_without_resize_passes_native_bytes_through() {
        let native = png_of(5, 4);
        let snapshot = encode_preview(native.clone(), SnapshotOptions::png()).expect("passthrough");
        assert_eq!(snapshot.bytes, native);
        assert_eq!((snapshot.width, snapshot.height), (5, 4));
        assert_eq!(snapshot.format, SnapshotFormat::Png);
    }

    #[test]
    fn png_with_bounds_is_downscaled() {
        let snapshot = encode_preview(png_of(40, 20), SnapshotOptions::png().with_bounds(10, 10))
            .expect("downscale");
        assert!(is_png(&snapshot.bytes));
        assert_eq!((snapshot.width, snapshot.height), (10, 5));
        assert_eq!(png_dimensions(&snapshot.bytes), Some((10, 5)));
    }

    #[test]
    fn jpeg_request_transcodes_in_memory() {
        let snapshot = encode_preview(png_of(8, 6), SnapshotOptions::jpeg(70)).expect("jpeg");
        assert!(snapshot.bytes.starts_with(&[0xff, 0xd8]));
        assert_eq!(snapshot.format, SnapshotFormat::Jpeg { quality: 70 });
        assert_eq!((snapshot.width, snapshot.height), (8, 6));
    }

    #[test]
    fn invalid_preview_bytes_are_typed_failures() {
        let empty = encode_preview(Vec::new(), SnapshotOptions::png()).expect_err("empty");
        assert_eq!(empty.code, IpcErrorCode::BrowserScreenshotFailed);
        let garbage = encode_preview(vec![1, 2, 3], SnapshotOptions::png()).expect_err("garbage");
        assert_eq!(garbage.code, IpcErrorCode::BrowserScreenshotFailed);
        let native =
            preview_result(Err("E_FAIL".into()), SnapshotOptions::png()).expect_err("native");
        assert_eq!(native.code, IpcErrorCode::BrowserScreenshotFailed);
        assert!(native.message.contains("E_FAIL"));
    }

    #[test]
    fn stream_is_read_from_the_start_regardless_of_position() {
        let payload: Vec<u8> = (0..(STREAM_READ_CHUNK * 2 + 17))
            .map(|index| (index % 251) as u8)
            .collect();
        // SAFETY: test-local stream used only on this thread; `payload` outlives the Write.
        let stream = unsafe { CreateStreamOnHGlobal(HGLOBAL::default(), true) }.expect("stream");
        // SAFETY: as above.
        unsafe { stream.Write(payload.as_ptr().cast(), payload.len() as u32, None) }
            .ok()
            .expect("write");
        assert_eq!(read_stream_to_vec(&stream).expect("read"), payload);

        // SAFETY: as above.
        let empty = unsafe { CreateStreamOnHGlobal(HGLOBAL::default(), true) }.expect("stream");
        assert!(read_stream_to_vec(&empty).expect("read empty").is_empty());
    }

    #[test]
    fn late_preview_after_timeout_is_fenced() {
        let (coordinator, rx) = SnapshotCallbackCoordinator::new();
        let released = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let released_flag = std::sync::Arc::clone(&released);
        coordinator.set_permit_releaser(move || {
            released_flag.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        coordinator.mark_timed_out();

        let delivered =
            coordinator.complete(preview_result(Ok(png_of(2, 2)), SnapshotOptions::png()));
        assert!(!delivered, "a preview after timeout must not be delivered");
        assert_eq!(coordinator.late_arrivals(), 1);
        assert!(released.load(std::sync::atomic::Ordering::SeqCst));
        drop(coordinator);
        assert!(rx.blocking_recv().is_err(), "receiver must see no value");
    }
}
