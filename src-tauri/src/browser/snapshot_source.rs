use crate::browser::BrowserError;
use crate::ipc::error::{IpcError, IpcErrorCode};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Manager};

#[cfg(target_os = "macos")]
use objc2::rc::Retained;
#[cfg(target_os = "macos")]
use objc2::{class, msg_send, runtime::AnyObject};
#[cfg(target_os = "macos")]
use objc2_foundation::NSString;

/// The target image encoding for browser snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotFormat {
    Png,
    Jpeg { quality: u8 },
}

impl Default for SnapshotFormat {
    fn default() -> Self {
        Self::Png
    }
}

/// Options controlling the snapshot capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotOptions {
    pub format: SnapshotFormat,
}

impl SnapshotOptions {
    pub fn png() -> Self {
        Self {
            format: SnapshotFormat::Png,
        }
    }

    pub fn jpeg(quality: u8) -> Self {
        Self {
            format: SnapshotFormat::Jpeg {
                quality: quality.min(100),
            },
        }
    }
}

impl Default for SnapshotOptions {
    fn default() -> Self {
        Self::png()
    }
}

/// An in-memory browser snapshot containing encoded image bytes and metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserSnapshot {
    pub bytes: Vec<u8>,
    pub format: SnapshotFormat,
    pub width: u32,
    pub height: u32,
}

impl BrowserSnapshot {
    pub fn new(bytes: Vec<u8>, format: SnapshotFormat, width: u32, height: u32) -> Self {
        Self {
            bytes,
            format,
            width,
            height,
        }
    }
}

/// Trait abstracting the memory snapshot capture of a browser webview.
pub trait BrowserSnapshotSource: Send + Sync {
    fn capture_snapshot<'a>(
        &'a self,
        webview_label: &'a str,
        options: SnapshotOptions,
    ) -> Pin<Box<dyn Future<Output = Result<BrowserSnapshot, IpcError>> + Send + 'a>>;
}

/// Thread-safe coordinator for native snapshot callbacks.
///
/// Ensures:
/// - Single-use delivery: only the first callback delivers the result.
/// - Duplicate callback rejection: subsequent calls return false without panicking.
/// - Late arrival safety: if the receiver timed out or was dropped, completion returns false without panicking.
#[derive(Clone)]
pub struct SnapshotCallbackCoordinator {
    sender: Arc<Mutex<Option<tokio::sync::oneshot::Sender<Result<BrowserSnapshot, IpcError>>>>>,
    call_count: Arc<AtomicUsize>,
}

impl SnapshotCallbackCoordinator {
    pub fn new() -> (Self, tokio::sync::oneshot::Receiver<Result<BrowserSnapshot, IpcError>>) {
        let (tx, rx) = tokio::sync::oneshot::channel();
        (
            Self {
                sender: Arc::new(Mutex::new(Some(tx))),
                call_count: Arc::new(AtomicUsize::new(0)),
            },
            rx,
        )
    }

    pub fn complete(&self, result: Result<BrowserSnapshot, IpcError>) -> bool {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        let mut guard = match self.sender.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if let Some(tx) = guard.take() {
            tx.send(result).is_ok()
        } else {
            false
        }
    }

    pub fn call_count(&self) -> usize {
        self.call_count.load(Ordering::SeqCst)
    }
}

/// Waits for a snapshot completion on an async oneshot channel with a bounded timeout.
/// Replaces the per-request OS thread spawn from earlier implementations.
pub async fn await_snapshot_completion(
    rx: tokio::sync::oneshot::Receiver<Result<BrowserSnapshot, IpcError>>,
    timeout: Duration,
) -> Result<BrowserSnapshot, IpcError> {
    match tokio::time::timeout(timeout, rx).await {
        Ok(Ok(result)) => result,
        Ok(Err(_closed)) => Err(IpcError::new(
            IpcErrorCode::BrowserScreenshotFailed,
            "snapshot task channel closed",
        )),
        Err(_elapsed) => Err(IpcError::new(
            IpcErrorCode::BrowserScreenshotFailed,
            format!("screenshot timed out after {}ms", timeout.as_millis()),
        )),
    }
}

/// Production snapshot source backed by Tauri's AppHandle and webviews.
pub struct TauriBrowserSnapshotSource<R: tauri::Runtime> {
    app: AppHandle<R>,
    timeout: Duration,
}

impl<R: tauri::Runtime> TauriBrowserSnapshotSource<R> {
    pub fn new(app: AppHandle<R>) -> Self {
        Self {
            app,
            timeout: Duration::from_secs(10),
        }
    }

    pub fn with_timeout(app: AppHandle<R>, timeout: Duration) -> Self {
        Self { app, timeout }
    }
}

#[cfg(target_os = "macos")]
impl<R: tauri::Runtime> BrowserSnapshotSource for TauriBrowserSnapshotSource<R> {
    fn capture_snapshot<'a>(
        &'a self,
        webview_label: &'a str,
        options: SnapshotOptions,
    ) -> Pin<Box<dyn Future<Output = Result<BrowserSnapshot, IpcError>> + Send + 'a>> {
        Box::pin(async move {
            capture_webview_snapshot(&self.app, webview_label, options, self.timeout).await
        })
    }
}

#[cfg(not(target_os = "macos"))]
impl<R: tauri::Runtime> BrowserSnapshotSource for TauriBrowserSnapshotSource<R> {
    fn capture_snapshot<'a>(
        &'a self,
        _webview_label: &'a str,
        _options: SnapshotOptions,
    ) -> Pin<Box<dyn Future<Output = Result<BrowserSnapshot, IpcError>> + Send + 'a>> {
        Box::pin(async move {
            Err(IpcError::new(
                IpcErrorCode::Unsupported,
                "screenshots are unavailable on this platform",
            ))
        })
    }
}

/// Standalone capture helper delegating to `TauriBrowserSnapshotSource`.
pub async fn capture_browser_snapshot<R: tauri::Runtime>(
    app: &AppHandle<R>,
    webview_label: &str,
    options: SnapshotOptions,
) -> Result<BrowserSnapshot, IpcError> {
    TauriBrowserSnapshotSource::new(app.clone())
        .capture_snapshot(webview_label, options)
        .await
}

#[cfg(target_os = "macos")]
pub async fn capture_webview_snapshot<R: tauri::Runtime>(
    app: &AppHandle<R>,
    webview_label: &str,
    options: SnapshotOptions,
    timeout: Duration,
) -> Result<BrowserSnapshot, IpcError> {
    let webview = app
        .get_webview(webview_label)
        .ok_or_else(|| BrowserError::WebviewNotFound(webview_label.to_string()))?;

    let (coordinator, rx) = SnapshotCallbackCoordinator::new();

    webview
        .with_webview(move |platform| unsafe {
            let view: &AnyObject = &*platform.inner().cast();
            let coordinator = coordinator;
            let callback = block2::RcBlock::new(move |image: *mut AnyObject, error: *mut AnyObject| {
                let result = parse_native_snapshot_result(image, error, options);
                let _ = coordinator.complete(result);
            });

            let _: () = msg_send![
                view,
                takeSnapshotWithConfiguration: std::ptr::null::<AnyObject>(),
                completionHandler: &*callback
            ];
        })
        .map_err(|e| {
            IpcError::new(
                IpcErrorCode::BrowserScreenshotFailed,
                format!("failed to attach webview snapshot handler: {e}"),
            )
        })?;

    await_snapshot_completion(rx, timeout).await
}

/// Parses the AppKit / WebKit snapshot completion objects strictly on the main thread.
/// Objective-C objects never leave this function; only Rust `Vec<u8>` is returned.
#[cfg(target_os = "macos")]
pub unsafe fn parse_native_snapshot_result(
    image: *mut AnyObject,
    error: *mut AnyObject,
    options: SnapshotOptions,
) -> Result<BrowserSnapshot, IpcError> {
    if !error.is_null() || image.is_null() {
        return Err(IpcError::new(
            IpcErrorCode::BrowserScreenshotFailed,
            "WKWebView snapshot failed",
        ));
    }

    let tiff: Option<Retained<AnyObject>> = msg_send![image, TIFFRepresentation];
    let tiff = tiff.ok_or_else(|| {
        IpcError::new(
            IpcErrorCode::BrowserScreenshotFailed,
            "snapshot has no TIFF representation",
        )
    })?;

    let bitmap: Option<Retained<AnyObject>> =
        msg_send![class!(NSBitmapImageRep), imageRepWithData: &*tiff];
    let bitmap = bitmap.ok_or_else(|| {
        IpcError::new(
            IpcErrorCode::BrowserScreenshotFailed,
            "cannot decode snapshot bitmap image",
        )
    })?;

    let pixels_wide: isize = msg_send![&*bitmap, pixelsWide];
    let pixels_high: isize = msg_send![&*bitmap, pixelsHigh];
    let (width, height) = if pixels_wide > 0 && pixels_high > 0 {
        (pixels_wide as u32, pixels_high as u32)
    } else {
        let size: objc2_foundation::NSSize = msg_send![image, size];
        (size.width.max(1.0) as u32, size.height.max(1.0) as u32)
    };

    let (type_code, properties) = match options.format {
        SnapshotFormat::Png => {
            let props: Retained<AnyObject> = msg_send![class!(NSDictionary), dictionary];
            (4usize, props) // NSBitmapImageFileTypePNG
        }
        SnapshotFormat::Jpeg { quality } => {
            let factor = (quality.min(100) as f32) / 100.0;
            let factor_num: Option<Retained<AnyObject>> =
                msg_send![class!(NSNumber), numberWithFloat: factor];
            let key = NSString::from_str("NSImageCompressionFactor");
            let props: Option<Retained<AnyObject>> = if let Some(num) = factor_num {
                msg_send![class!(NSDictionary), dictionaryWithObject: &*num, forKey: &*key]
            } else {
                None
            };
            let props = props.unwrap_or_else(|| msg_send![class!(NSDictionary), dictionary]);
            (3usize, props) // NSBitmapImageFileTypeJPEG
        }
    };

    let data: Option<Retained<AnyObject>> = msg_send![
        &*bitmap,
        representationUsingType: type_code,
        properties: &*properties
    ];
    let data = data.ok_or_else(|| {
        IpcError::new(
            IpcErrorCode::BrowserScreenshotFailed,
            match options.format {
                SnapshotFormat::Png => "cannot encode snapshot to PNG",
                SnapshotFormat::Jpeg { .. } => "cannot encode snapshot to JPEG",
            },
        )
    })?;

    let length: usize = msg_send![&*data, length];
    if length == 0 {
        return Err(IpcError::new(
            IpcErrorCode::BrowserScreenshotFailed,
            "empty snapshot generated",
        ));
    }

    let pointer: *const u8 = msg_send![&*data, bytes];
    let bytes = std::slice::from_raw_parts(pointer, length).to_vec();

    Ok(BrowserSnapshot::new(bytes, options.format, width, height))
}

/// Standalone snapshot source that always returns typed `Unsupported`.
pub struct UnsupportedSnapshotSource;

impl BrowserSnapshotSource for UnsupportedSnapshotSource {
    fn capture_snapshot<'a>(
        &'a self,
        _webview_label: &'a str,
        _options: SnapshotOptions,
    ) -> Pin<Box<dyn Future<Output = Result<BrowserSnapshot, IpcError>> + Send + 'a>> {
        Box::pin(async move {
            Err(IpcError::new(
                IpcErrorCode::Unsupported,
                "screenshots are unavailable on this platform",
            ))
        })
    }
}

/// Injectable fake snapshot behaviors for unit and integration tests.
#[derive(Debug, Clone)]
pub enum FakeSnapshotBehavior {
    Fixed(BrowserSnapshot),
    Auto { width: u32, height: u32 },
    WebviewNotFound,
    NativeError(String),
    NullImage,
    BitmapDecodeError,
    EncodeError,
    EmptyImageData,
    Timeout,
    Delayed {
        delay: Duration,
        result: Result<BrowserSnapshot, IpcError>,
    },
    Duplicate {
        first: Result<BrowserSnapshot, IpcError>,
        second: Result<BrowserSnapshot, IpcError>,
    },
    Unsupported,
    Error(IpcError),
}

/// Injectable fake snapshot source for deterministic unit testing.
pub struct FakeBrowserSnapshotSource {
    behaviors: Mutex<std::collections::HashMap<String, FakeSnapshotBehavior>>,
    default_behavior: Mutex<FakeSnapshotBehavior>,
    timeout: Duration,
    call_count: Arc<AtomicUsize>,
    last_requested_format: Mutex<Option<SnapshotFormat>>,
}

impl FakeBrowserSnapshotSource {
    pub fn new(default_behavior: FakeSnapshotBehavior) -> Self {
        Self {
            behaviors: Mutex::new(std::collections::HashMap::new()),
            default_behavior: Mutex::new(default_behavior),
            timeout: Duration::from_millis(50),
            call_count: Arc::new(AtomicUsize::new(0)),
            last_requested_format: Mutex::new(None),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn set_behavior(&self, webview_label: &str, behavior: FakeSnapshotBehavior) {
        self.behaviors
            .lock()
            .unwrap()
            .insert(webview_label.to_string(), behavior);
    }

    pub fn call_count(&self) -> usize {
        self.call_count.load(Ordering::SeqCst)
    }

    pub fn last_format(&self) -> Option<SnapshotFormat> {
        *self.last_requested_format.lock().unwrap()
    }
}

impl BrowserSnapshotSource for FakeBrowserSnapshotSource {
    fn capture_snapshot<'a>(
        &'a self,
        webview_label: &'a str,
        options: SnapshotOptions,
    ) -> Pin<Box<dyn Future<Output = Result<BrowserSnapshot, IpcError>> + Send + 'a>> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        *self.last_requested_format.lock().unwrap() = Some(options.format);

        let behavior = self
            .behaviors
            .lock()
            .unwrap()
            .get(webview_label)
            .cloned()
            .unwrap_or_else(|| self.default_behavior.lock().unwrap().clone());

        let timeout = self.timeout;
        let label = webview_label.to_string();

        Box::pin(async move {
            match behavior {
                FakeSnapshotBehavior::Fixed(snapshot) => Ok(snapshot),
                FakeSnapshotBehavior::Auto { width, height } => {
                    let bytes = match options.format {
                        SnapshotFormat::Png => sample_valid_png_bytes(),
                        SnapshotFormat::Jpeg { .. } => sample_valid_jpeg_bytes(),
                    };
                    Ok(BrowserSnapshot::new(bytes, options.format, width, height))
                }
                FakeSnapshotBehavior::WebviewNotFound => {
                    Err(BrowserError::WebviewNotFound(label).into())
                }
                FakeSnapshotBehavior::NativeError(msg) => Err(IpcError::new(
                    IpcErrorCode::BrowserScreenshotFailed,
                    format!("WKWebView snapshot failed: {msg}"),
                )),
                FakeSnapshotBehavior::NullImage => Err(IpcError::new(
                    IpcErrorCode::BrowserScreenshotFailed,
                    "WKWebView snapshot failed: null image",
                )),
                FakeSnapshotBehavior::BitmapDecodeError => Err(IpcError::new(
                    IpcErrorCode::BrowserScreenshotFailed,
                    "cannot decode snapshot bitmap image",
                )),
                FakeSnapshotBehavior::EncodeError => Err(IpcError::new(
                    IpcErrorCode::BrowserScreenshotFailed,
                    match options.format {
                        SnapshotFormat::Png => "cannot encode snapshot to PNG",
                        SnapshotFormat::Jpeg { .. } => "cannot encode snapshot to JPEG",
                    },
                )),
                FakeSnapshotBehavior::EmptyImageData => Err(IpcError::new(
                    IpcErrorCode::BrowserScreenshotFailed,
                    "empty snapshot generated",
                )),
                FakeSnapshotBehavior::Timeout => {
                    let (_coordinator, rx) = SnapshotCallbackCoordinator::new();
                    await_snapshot_completion(rx, timeout).await
                }
                FakeSnapshotBehavior::Delayed { delay, result } => {
                    let (coordinator, rx) = SnapshotCallbackCoordinator::new();
                    tokio::spawn(async move {
                        tokio::time::sleep(delay).await;
                        let _ = coordinator.complete(result);
                    });
                    await_snapshot_completion(rx, timeout).await
                }
                FakeSnapshotBehavior::Duplicate { first, second } => {
                    let (coordinator, rx) = SnapshotCallbackCoordinator::new();
                    let _ = coordinator.complete(first);
                    let _ = coordinator.complete(second);
                    await_snapshot_completion(rx, timeout).await
                }
                FakeSnapshotBehavior::Unsupported => Err(IpcError::new(
                    IpcErrorCode::Unsupported,
                    "screenshots are unavailable on this platform",
                )),
                FakeSnapshotBehavior::Error(err) => Err(err),
            }
        })
    }
}

pub fn sample_valid_png_bytes() -> Vec<u8> {
    vec![
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a,
        0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
        0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
        0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
        0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41,
        0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0x00,
        0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xdd, 0x8d,
        0xb0, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e,
        0x44, 0xae, 0x42, 0x60, 0x82,
    ]
}

pub fn sample_valid_jpeg_bytes() -> Vec<u8> {
    vec![
        0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10, 0x4a, 0x46,
        0x49, 0x46, 0x00, 0x01, 0x01, 0x00, 0x00, 0x01,
        0x00, 0x01, 0x00, 0x00, 0xff, 0xdb, 0x00, 0x43,
        0x00, 0x08, 0x06, 0x06, 0x07, 0x06, 0x05, 0x08,
        0x07, 0x07, 0x07, 0x09, 0x09, 0x08, 0x0a, 0x0c,
        0x14, 0x0d, 0x0c, 0x0b, 0x0b, 0x0c, 0x19, 0x12,
        0x13, 0x0f, 0x14, 0x1d, 0x1a, 0x1f, 0x1e, 0x1d,
        0x1a, 0x1c, 0x1c, 0x20, 0x24, 0x2e, 0x27, 0x20,
        0x22, 0x2c, 0x23, 0x1c, 0x1c, 0x28, 0x37, 0x29,
        0x2c, 0x30, 0x31, 0x34, 0x34, 0x34, 0x1f, 0x27,
        0x39, 0x3d, 0x38, 0x32, 0x3c, 0x2e, 0x33, 0x34,
        0x32, 0xff, 0xc0, 0x00, 0x0b, 0x08, 0x00, 0x01,
        0x00, 0x01, 0x01, 0x01, 0x11, 0x00, 0xff, 0xc4,
        0x00, 0x14, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x09, 0xff, 0xda, 0x00, 0x08,
        0x01, 0x01, 0x00, 0x00, 0x3f, 0x00, 0x37, 0xff,
        0xd9,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_snapshot_png_success() {
        let source = FakeBrowserSnapshotSource::new(FakeSnapshotBehavior::Auto {
            width: 100,
            height: 80,
        });
        let snapshot = source
            .capture_snapshot("main-view", SnapshotOptions::png())
            .await
            .expect("PNG capture should succeed");
        assert_eq!(snapshot.format, SnapshotFormat::Png);
        assert_eq!(snapshot.width, 100);
        assert_eq!(snapshot.height, 80);
        assert!(snapshot.bytes.starts_with(&[0x89, b'P', b'N', b'G']));
    }

    #[tokio::test]
    async fn test_snapshot_jpeg_success() {
        let source = FakeBrowserSnapshotSource::new(FakeSnapshotBehavior::Auto {
            width: 640,
            height: 480,
        });
        let snapshot = source
            .capture_snapshot("main-view", SnapshotOptions::jpeg(75))
            .await
            .expect("JPEG capture should succeed");
        assert_eq!(snapshot.format, SnapshotFormat::Jpeg { quality: 75 });
        assert_eq!(snapshot.width, 640);
        assert_eq!(snapshot.height, 480);
        assert!(snapshot.bytes.starts_with(&[0xff, 0xd8]));
    }

    #[tokio::test]
    async fn test_snapshot_webview_missing() {
        let source = FakeBrowserSnapshotSource::new(FakeSnapshotBehavior::WebviewNotFound);
        let err = source
            .capture_snapshot("nonexistent-view", SnapshotOptions::png())
            .await
            .expect_err("Should fail when webview is missing");
        assert_eq!(err.code, IpcErrorCode::WebviewNotFound);
        assert!(err.message.contains("nonexistent-view"));
    }

    #[tokio::test]
    async fn test_snapshot_native_error() {
        let source = FakeBrowserSnapshotSource::new(FakeSnapshotBehavior::NativeError(
            "internal WebKit rendering error".into(),
        ));
        let err = source
            .capture_snapshot("main-view", SnapshotOptions::png())
            .await
            .expect_err("Should fail on native error");
        assert_eq!(err.code, IpcErrorCode::BrowserScreenshotFailed);
        assert!(err.message.contains("WKWebView snapshot failed"));
    }

    #[tokio::test]
    async fn test_snapshot_null_image() {
        let source = FakeBrowserSnapshotSource::new(FakeSnapshotBehavior::NullImage);
        let err = source
            .capture_snapshot("main-view", SnapshotOptions::png())
            .await
            .expect_err("Should fail on null image");
        assert_eq!(err.code, IpcErrorCode::BrowserScreenshotFailed);
        assert!(err.message.contains("null image"));
    }

    #[tokio::test]
    async fn test_snapshot_bitmap_failure() {
        let source = FakeBrowserSnapshotSource::new(FakeSnapshotBehavior::BitmapDecodeError);
        let err = source
            .capture_snapshot("main-view", SnapshotOptions::png())
            .await
            .expect_err("Should fail on bitmap decode error");
        assert_eq!(err.code, IpcErrorCode::BrowserScreenshotFailed);
        assert!(err.message.contains("bitmap"));
    }

    #[tokio::test]
    async fn test_snapshot_encode_failure() {
        let source = FakeBrowserSnapshotSource::new(FakeSnapshotBehavior::EncodeError);
        let err = source
            .capture_snapshot("main-view", SnapshotOptions::png())
            .await
            .expect_err("Should fail on encode error");
        assert_eq!(err.code, IpcErrorCode::BrowserScreenshotFailed);
        assert!(err.message.contains("encode"));
    }

    #[tokio::test]
    async fn test_snapshot_callback_timeout_no_callback() {
        let source = FakeBrowserSnapshotSource::new(FakeSnapshotBehavior::Timeout)
            .with_timeout(Duration::from_millis(30));
        let err = source
            .capture_snapshot("main-view", SnapshotOptions::png())
            .await
            .expect_err("Should fail on timeout when no callback arrives");
        assert_eq!(err.code, IpcErrorCode::BrowserScreenshotFailed);
        assert!(err.message.contains("timed out"));
    }

    #[tokio::test]
    async fn test_snapshot_duplicate_callback() {
        let ok_snap = BrowserSnapshot::new(sample_valid_png_bytes(), SnapshotFormat::Png, 10, 10);
        let dup_snap = BrowserSnapshot::new(vec![1, 2, 3], SnapshotFormat::Png, 20, 20);
        let source = FakeBrowserSnapshotSource::new(FakeSnapshotBehavior::Duplicate {
            first: Ok(ok_snap),
            second: Ok(dup_snap),
        });
        let res = source
            .capture_snapshot("main-view", SnapshotOptions::png())
            .await
            .expect("First callback should be delivered successfully");
        assert_eq!(res.width, 10);
        assert_eq!(res.height, 10);
    }

    #[tokio::test]
    async fn test_snapshot_late_arrival_after_timeout() {
        let ok_snap = BrowserSnapshot::new(sample_valid_png_bytes(), SnapshotFormat::Png, 10, 10);
        let source = FakeBrowserSnapshotSource::new(FakeSnapshotBehavior::Delayed {
            delay: Duration::from_millis(80),
            result: Ok(ok_snap),
        })
        .with_timeout(Duration::from_millis(20));

        let err = source
            .capture_snapshot("main-view", SnapshotOptions::png())
            .await
            .expect_err("Should time out before delayed callback arrives");
        assert_eq!(err.code, IpcErrorCode::BrowserScreenshotFailed);
        assert!(err.message.contains("timed out"));

        // Wait for late callback to complete in background without panic
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    #[tokio::test]
    async fn test_snapshot_close_after_callback() {
        let (coordinator, rx) = SnapshotCallbackCoordinator::new();
        // Drop receiver before callback completes
        drop(rx);
        // Coordinator completion should return false (channel closed) and NOT panic
        let completed = coordinator.complete(Ok(BrowserSnapshot::new(
            sample_valid_png_bytes(),
            SnapshotFormat::Png,
            1,
            1,
        )));
        assert!(!completed, "Should detect receiver was closed");
    }

    #[tokio::test]
    async fn test_snapshot_platform_unsupported() {
        let source = UnsupportedSnapshotSource;
        let err = source
            .capture_snapshot("main-view", SnapshotOptions::png())
            .await
            .expect_err("Should fail on unsupported platform");
        assert_eq!(err.code, IpcErrorCode::Unsupported);
        assert!(err.message.contains("unavailable on this platform"));
    }

    #[test]
    fn test_coordinator_duplicate_and_late_calls() {
        let (coordinator, rx) = SnapshotCallbackCoordinator::new();
        let snap1 = BrowserSnapshot::new(sample_valid_png_bytes(), SnapshotFormat::Png, 1, 1);
        let snap2 = BrowserSnapshot::new(vec![9], SnapshotFormat::Png, 2, 2);

        // First completion succeeds
        assert!(coordinator.complete(Ok(snap1)));
        assert_eq!(coordinator.call_count(), 1);

        // Duplicate completion fails
        assert!(!coordinator.complete(Ok(snap2)));
        assert_eq!(coordinator.call_count(), 2);

        // Receiver gets the first value
        let received = rx.blocking_recv().expect("Channel has message");
        let snapshot = received.expect("Snapshot ok");
        assert_eq!(snapshot.width, 1);
    }
}
