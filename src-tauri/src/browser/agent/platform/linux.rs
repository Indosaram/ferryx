//! Linux native browser capture (plan C4, T7c): WebKitGTK `webkit_web_view_get_snapshot`
//! with `WEBKIT_SNAPSHOT_REGION_VISIBLE`, finished in the async callback, encoded in memory.
//!
//! Every GObject (WebView, cairo surface, GdkPixbuf) is created, used and dropped on the GTK
//! main thread inside `with_webview` and the snapshot callback. Only the Rust
//! [`BrowserSnapshot`] crosses threads, through the shared [`SnapshotCallbackCoordinator`]
//! (single delivery, timeout and late-callback fence). Nothing touches the filesystem.
//!
//! Encoding uses gdk-pixbuf (`save_to_bufferv`) because the pinned cairo-rs 0.18 is built
//! without its `png` feature; gdk-pixbuf is already linked through `gtk`.

use crate::browser::snapshot_source::{
    await_coordinator_completion, clamp_capture_dimensions, BrowserSnapshot,
    SnapshotCallbackCoordinator, SnapshotFormat, SnapshotOptions,
};
use crate::browser::BrowserError;
use crate::ipc::error::{IpcError, IpcErrorCode};
use gtk::cairo;
use gtk::gdk;
use gtk::gdk_pixbuf;
use std::time::Duration;
use tauri::{AppHandle, Manager};

fn capture_error(message: impl Into<String>) -> IpcError {
    IpcError::new(IpcErrorCode::BrowserScreenshotFailed, message.into())
}

/// Final encoded size for a `source_w` x `source_h` capture. Fits inside the requested
/// bounds preserving aspect ratio, never upscales, and always honours the shared
/// `MAX_CAPTURE_EDGE` / `MAX_CAPTURE_PIXELS` caps.
pub(crate) fn target_dimensions(
    source_w: u32,
    source_h: u32,
    max_width: Option<u32>,
    max_height: Option<u32>,
) -> (u32, u32) {
    let (mut w, mut h) = (source_w.max(1), source_h.max(1));
    let bound_w = max_width.unwrap_or(w).max(1);
    let bound_h = max_height.unwrap_or(h).max(1);
    if w > bound_w || h > bound_h {
        let scale = (f64::from(bound_w) / f64::from(w)).min(f64::from(bound_h) / f64::from(h));
        w = ((f64::from(w) * scale).round() as u32).clamp(1, bound_w);
        h = ((f64::from(h) * scale).round() as u32).clamp(1, bound_h);
    }
    clamp_capture_dimensions(w, h)
}

/// gdk-pixbuf saver name and options for a snapshot format.
pub(crate) fn pixbuf_encoder(format: SnapshotFormat) -> (&'static str, Option<String>) {
    match format {
        SnapshotFormat::Png => ("png", None),
        SnapshotFormat::Jpeg { quality } => ("jpeg", Some(quality.min(100).to_string())),
    }
}

/// Converts the WebKit snapshot surface to encoded bytes. GTK main thread only: the surface
/// and every derived pixbuf are dropped before this returns; only Rust bytes leave.
fn encode_snapshot_surface(
    surface: cairo::Surface,
    options: SnapshotOptions,
) -> Result<BrowserSnapshot, IpcError> {
    surface.status().map_err(|error| {
        capture_error(format!("WebKitGTK snapshot surface is invalid: {error}"))
    })?;
    let image = cairo::ImageSurface::try_from(surface)
        .map_err(|_| capture_error("WebKitGTK snapshot is not an image surface"))?;
    let (pixel_w, pixel_h) = (image.width(), image.height());
    if pixel_w <= 0 || pixel_h <= 0 {
        return Err(capture_error("WebKitGTK snapshot is empty"));
    }
    // The image surface stores device pixels; reset the HiDPI scale so the copy below reads
    // exactly pixel_w x pixel_h instead of resampling through the device scale.
    image.set_device_scale(1.0, 1.0);
    let pixbuf = gdk::pixbuf_get_from_surface(&image, 0, 0, pixel_w, pixel_h)
        .ok_or_else(|| capture_error("cannot convert snapshot surface to a pixbuf"))?;
    drop(image);

    let (target_w, target_h) = target_dimensions(
        pixel_w as u32,
        pixel_h as u32,
        options.max_width,
        options.max_height,
    );
    let pixbuf = if (target_w, target_h) == (pixel_w as u32, pixel_h as u32) {
        pixbuf
    } else {
        pixbuf
            .scale_simple(
                target_w as i32,
                target_h as i32,
                gdk_pixbuf::InterpType::Bilinear,
            )
            .ok_or_else(|| capture_error("cannot scale snapshot to the requested bounds"))?
    };

    let (saver, quality) = pixbuf_encoder(options.format);
    let save_options: Vec<(&str, &str)> = quality
        .as_deref()
        .map(|q| vec![("quality", q)])
        .unwrap_or_default();
    let bytes = pixbuf
        .save_to_bufferv(saver, &save_options)
        .map_err(|error| capture_error(format!("cannot encode snapshot as {saver}: {error}")))?;
    if bytes.is_empty() {
        return Err(capture_error("empty snapshot generated"));
    }
    Ok(BrowserSnapshot::new(
        bytes,
        options.format,
        pixbuf.width() as u32,
        pixbuf.height() as u32,
    ))
}

/// Linux counterpart of the macOS `capture_webview_snapshot`.
pub async fn capture_webview_snapshot<R: tauri::Runtime>(
    app: &AppHandle<R>,
    webview_label: &str,
    options: SnapshotOptions,
    timeout: Duration,
) -> Result<BrowserSnapshot, IpcError> {
    capture_webview_snapshot_with_coordinator(app, webview_label, options, timeout, None).await
}

/// Linux counterpart of the macOS `capture_webview_snapshot_with_coordinator`: same
/// arguments, coordinator and timeout semantics. The native request is issued on the GTK
/// main thread by `with_webview`; WebKit calls back on that thread, where the surface is
/// encoded and the Rust result handed to the coordinator.
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
            use webkit2gtk::WebViewExt;

            platform.inner().snapshot(
                webkit2gtk::SnapshotRegion::Visible,
                webkit2gtk::SnapshotOptions::NONE,
                None::<&webkit2gtk::gio::Cancellable>,
                move |result| {
                    let snapshot = result
                        .map_err(|error| {
                            capture_error(format!("WebKitGTK snapshot failed: {error}"))
                        })
                        .and_then(|surface| encode_snapshot_surface(surface, options));
                    let _ = coordinator.complete(snapshot);
                },
            );
        })
        .map_err(|e| capture_error(format!("failed to attach webview snapshot handler: {e}")))?;

    await_coordinator_completion(&coordinator_clone, rx, timeout).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::snapshot_source::{MAX_CAPTURE_EDGE, MAX_CAPTURE_PIXELS};

    #[test]
    fn unbounded_capture_keeps_native_size() {
        assert_eq!(target_dimensions(1280, 800, None, None), (1280, 800));
    }

    #[test]
    fn bounds_fit_preserving_aspect_ratio() {
        assert_eq!(
            target_dimensions(1600, 800, Some(800), Some(800)),
            (800, 400)
        );
        assert_eq!(
            target_dimensions(800, 1600, Some(800), Some(800)),
            (400, 800)
        );
        assert_eq!(target_dimensions(1600, 800, Some(400), None), (400, 200));
        assert_eq!(target_dimensions(1600, 800, None, Some(200)), (400, 200));
    }

    #[test]
    fn bounds_never_upscale() {
        assert_eq!(
            target_dimensions(640, 480, Some(1920), Some(1080)),
            (640, 480)
        );
    }

    #[test]
    fn hidpi_capture_is_capped_by_shared_limits() {
        let (w, h) = target_dimensions(7680, 4320, None, None);
        assert!(w <= MAX_CAPTURE_EDGE && h <= MAX_CAPTURE_EDGE);
        assert!(u64::from(w) * u64::from(h) <= MAX_CAPTURE_PIXELS);
    }

    #[test]
    fn degenerate_inputs_yield_at_least_one_pixel() {
        assert_eq!(target_dimensions(0, 0, None, None), (1, 1));
        assert_eq!(target_dimensions(100, 100, Some(0), Some(0)), (1, 1));
    }

    #[test]
    fn encoder_matches_requested_format() {
        assert_eq!(pixbuf_encoder(SnapshotFormat::Png), ("png", None));
        assert_eq!(
            pixbuf_encoder(SnapshotFormat::Jpeg { quality: 85 }),
            ("jpeg", Some("85".to_string()))
        );
        assert_eq!(
            pixbuf_encoder(SnapshotFormat::Jpeg { quality: 255 }),
            ("jpeg", Some("100".to_string()))
        );
    }
}
