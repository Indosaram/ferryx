//! Linux native browser capture (plan C4, T7c): WebKitGTK `webkit_web_view_get_snapshot`
//! with `WEBKIT_SNAPSHOT_REGION_VISIBLE`, finished in the async callback, encoded in memory.
//!
//! Every GObject (WebView, cairo surface) is created, used and dropped on the GTK main
//! thread inside `with_webview` and the snapshot callback. The callback only copies the
//! surface's pixels into a Rust [`RawSnapshot`]; scaling and encoding run on a blocking
//! worker, which hands the [`BrowserSnapshot`] to the shared [`SnapshotCallbackCoordinator`]
//! (single delivery, timeout and late-callback fence). Nothing touches the filesystem.
//!
//! Encoding uses the pure-Rust `image` crate because the pinned cairo-rs 0.18 is built
//! without its `png` feature.

use crate::browser::snapshot_source::{
    await_coordinator_completion, clamp_capture_dimensions, BrowserSnapshot,
    SnapshotCallbackCoordinator, SnapshotFormat, SnapshotOptions,
};
use crate::browser::BrowserError;
use crate::ipc::error::{IpcError, IpcErrorCode};
use gtk::cairo;
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

/// Pixels copied out of a cairo image surface: `stride`-byte rows of native-endian 32-bit
/// words (`ARGB32` premultiplied, or `RGB24` whose top byte is unused when `opaque`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawSnapshot {
    pub width: u32,
    pub height: u32,
    pub stride: usize,
    pub opaque: bool,
    pub data: Vec<u8>,
}

/// Copies the WebKit snapshot surface's pixels into Rust memory. GTK main thread only: the
/// surface is dropped before this returns; only the copied bytes leave.
fn copy_snapshot_surface(surface: cairo::Surface) -> Result<RawSnapshot, IpcError> {
    surface.status().map_err(|error| {
        capture_error(format!("WebKitGTK snapshot surface is invalid: {error}"))
    })?;
    let image = cairo::ImageSurface::try_from(surface)
        .map_err(|_| capture_error("WebKitGTK snapshot is not an image surface"))?;
    let (pixel_w, pixel_h) = (image.width(), image.height());
    if pixel_w <= 0 || pixel_h <= 0 {
        return Err(capture_error("WebKitGTK snapshot is empty"));
    }
    let opaque = match image.format() {
        cairo::Format::ARgb32 => false,
        cairo::Format::Rgb24 => true,
        other => {
            return Err(capture_error(format!(
                "unsupported WebKitGTK snapshot pixel format: {other:?}"
            )))
        }
    };
    // The image surface stores device pixels; reset the HiDPI scale so the copy reads
    // exactly pixel_w x pixel_h device pixels.
    image.set_device_scale(1.0, 1.0);
    let stride = usize::try_from(image.stride())
        .map_err(|_| capture_error("WebKitGTK snapshot has a negative stride"))?;
    let mut data = Vec::new();
    image
        .with_data(|pixels| data.extend_from_slice(pixels))
        .map_err(|error| capture_error(format!("cannot read snapshot pixels: {error}")))?;
    Ok(RawSnapshot {
        width: pixel_w as u32,
        height: pixel_h as u32,
        stride,
        opaque,
        data,
    })
}

/// Converts cairo pixels to straight-alpha RGBA. Pure Rust; runs off the GTK thread.
pub(crate) fn raw_to_rgba(raw: &RawSnapshot) -> Result<image::RgbaImage, IpcError> {
    let (width, height) = (raw.width as usize, raw.height as usize);
    let row_bytes = width * 4;
    if width == 0
        || height == 0
        || raw.stride < row_bytes
        || raw.data.len() < raw.stride * (height - 1) + row_bytes
    {
        return Err(capture_error(
            "WebKitGTK snapshot pixel buffer is truncated",
        ));
    }
    let mut rgba = Vec::with_capacity(row_bytes * height);
    for row in raw.data.chunks(raw.stride).take(height) {
        for pixel in row[..row_bytes].chunks_exact(4) {
            let word = u32::from_ne_bytes([pixel[0], pixel[1], pixel[2], pixel[3]]);
            let alpha = if raw.opaque { 255 } else { (word >> 24) as u8 };
            let channel = |shift: u32| {
                let premultiplied = (word >> shift) & 0xff;
                match alpha {
                    0 => 0,
                    255 => premultiplied as u8,
                    a => ((premultiplied * 255 + u32::from(a) / 2) / u32::from(a)).min(255) as u8,
                }
            };
            rgba.extend_from_slice(&[channel(16), channel(8), channel(0), alpha]);
        }
    }
    image::RgbaImage::from_raw(raw.width, raw.height, rgba)
        .ok_or_else(|| capture_error("WebKitGTK snapshot pixel buffer is truncated"))
}

/// Scales the copied pixels to the requested bounds and encodes them in memory.
pub(crate) fn encode_raw(
    raw: RawSnapshot,
    options: SnapshotOptions,
) -> Result<BrowserSnapshot, IpcError> {
    let rgba = raw_to_rgba(&raw)?;
    drop(raw);
    let (target_w, target_h) = target_dimensions(
        rgba.width(),
        rgba.height(),
        options.max_width,
        options.max_height,
    );
    let rgba = if (target_w, target_h) == rgba.dimensions() {
        rgba
    } else {
        image::imageops::resize(
            &rgba,
            target_w,
            target_h,
            image::imageops::FilterType::Triangle,
        )
    };
    let (width, height) = rgba.dimensions();
    let mut bytes = Vec::new();
    match options.format {
        SnapshotFormat::Png => rgba
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .map_err(|error| capture_error(format!("cannot encode snapshot to PNG: {error}")))?,
        SnapshotFormat::Jpeg { quality } => image::DynamicImage::ImageRgba8(rgba)
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
    Ok(BrowserSnapshot::new(bytes, options.format, width, height))
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
/// main thread by `with_webview`; WebKit calls back on that thread, where the surface's
/// pixels are copied out; a blocking worker encodes them and completes the coordinator.
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
                    let raw = result
                        .map_err(|error| {
                            capture_error(format!("WebKitGTK snapshot failed: {error}"))
                        })
                        .and_then(copy_snapshot_surface);
                    match raw {
                        Ok(raw) => {
                            let _encoding = tauri::async_runtime::spawn_blocking(move || {
                                let _ = coordinator.complete(encode_raw(raw, options));
                            });
                        }
                        Err(error) => {
                            let _ = coordinator.complete(Err(error));
                        }
                    }
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
    fn raw_pixels_unpremultiply_and_skip_row_padding() {
        let mut data = Vec::new();
        // Row 0: opaque red, half-transparent premultiplied green (0x80 -> 0xff), transparent.
        for word in [0xffff_0000u32, 0x8000_8000, 0x0000_0000, 0xff00_00ff] {
            data.extend_from_slice(&word.to_ne_bytes());
        }
        data.extend_from_slice(&[0xaa; 8]); // stride padding
        for word in [0xff10_2030u32; 4] {
            data.extend_from_slice(&word.to_ne_bytes());
        }
        data.extend_from_slice(&[0xaa; 8]);
        let raw = RawSnapshot {
            width: 4,
            height: 2,
            stride: 24,
            opaque: false,
            data,
        };
        let rgba = raw_to_rgba(&raw).expect("convert");
        assert_eq!(rgba.dimensions(), (4, 2));
        assert_eq!(rgba.get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(rgba.get_pixel(1, 0).0, [0, 255, 0, 128]);
        assert_eq!(rgba.get_pixel(2, 0).0, [0, 0, 0, 0]);
        assert_eq!(rgba.get_pixel(3, 0).0, [0, 0, 255, 255]);
        assert_eq!(rgba.get_pixel(2, 1).0, [0x10, 0x20, 0x30, 255]);

        let truncated = RawSnapshot {
            width: 4,
            height: 2,
            stride: 16,
            opaque: false,
            data: vec![0; 20],
        };
        assert_eq!(
            raw_to_rgba(&truncated).expect_err("short").code,
            IpcErrorCode::BrowserScreenshotFailed
        );
    }

    #[test]
    fn opaque_rgb24_ignores_the_unused_byte() {
        let data = 0x0011_2233u32.to_ne_bytes().to_vec();
        let raw = RawSnapshot {
            width: 1,
            height: 1,
            stride: 4,
            opaque: true,
            data,
        };
        assert_eq!(
            raw_to_rgba(&raw).expect("convert").get_pixel(0, 0).0,
            [0x11, 0x22, 0x33, 255]
        );
    }

    fn solid_raw(width: u32, height: u32) -> RawSnapshot {
        let data = (0..width * height)
            .flat_map(|_| 0xff40_8020u32.to_ne_bytes())
            .collect();
        RawSnapshot {
            width,
            height,
            stride: width as usize * 4,
            opaque: false,
            data,
        }
    }

    #[test]
    fn raw_snapshot_encodes_to_png_at_native_size() {
        let snapshot = encode_raw(solid_raw(4, 2), SnapshotOptions::png()).expect("png");
        assert_eq!(snapshot.format, SnapshotFormat::Png);
        assert_eq!((snapshot.width, snapshot.height), (4, 2));
        let decoded = image::load_from_memory_with_format(&snapshot.bytes, image::ImageFormat::Png)
            .expect("decode")
            .to_rgba8();
        assert_eq!(decoded.dimensions(), (4, 2));
        assert_eq!(decoded.get_pixel(3, 1).0, [0x40, 0x80, 0x20, 255]);
    }

    #[test]
    fn raw_snapshot_is_downscaled_to_bounds() {
        let snapshot = encode_raw(
            solid_raw(40, 20),
            SnapshotOptions::png().with_bounds(10, 10),
        )
        .expect("scaled");
        assert_eq!((snapshot.width, snapshot.height), (10, 5));
        let decoded = image::load_from_memory(&snapshot.bytes).expect("decode");
        assert_eq!((decoded.width(), decoded.height()), (10, 5));
    }

    #[test]
    fn raw_snapshot_encodes_to_jpeg() {
        let snapshot = encode_raw(solid_raw(8, 6), SnapshotOptions::jpeg(70)).expect("jpeg");
        assert!(snapshot.bytes.starts_with(&[0xff, 0xd8]));
        assert_eq!(snapshot.format, SnapshotFormat::Jpeg { quality: 70 });
        assert_eq!((snapshot.width, snapshot.height), (8, 6));
    }
}
