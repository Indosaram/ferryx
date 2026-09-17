//! Unit tests for BrowserSnapshotSource, FakeBrowserSnapshotSource, and SnapshotCallbackCoordinator
//! Authoritative Spec: docs/plans/REMOTE_BROWSER_SCREENCAST_PLAN_2026-09-17.md (§7.1, Phase 7A)

use super::snapshot_source::*;
use crate::ipc::error::IpcErrorCode;
use std::time::Duration;

#[tokio::test]
async fn test_snapshot_source_png_capture() {
    let source = FakeBrowserSnapshotSource::new(FakeSnapshotBehavior::Auto {
        width: 1280,
        height: 720,
    });

    let snapshot = source
        .capture_snapshot("webview-main", SnapshotOptions::png())
        .await
        .expect("PNG capture should succeed");

    assert_eq!(snapshot.format, SnapshotFormat::Png);
    assert_eq!(snapshot.width, 1280);
    assert_eq!(snapshot.height, 720);
    assert!(snapshot.bytes.starts_with(&[0x89, b'P', b'N', b'G']));
    assert_eq!(source.call_count(), 1);
    assert_eq!(source.last_format(), Some(SnapshotFormat::Png));
}

#[tokio::test]
async fn test_snapshot_source_jpeg_quality_capture() {
    let qualities = [50u8, 70, 85, 100];
    for quality in qualities {
        let source = FakeBrowserSnapshotSource::new(FakeSnapshotBehavior::Auto {
            width: 800,
            height: 600,
        });

        let snapshot = source
            .capture_snapshot("webview-main", SnapshotOptions::jpeg(quality))
            .await
            .expect("JPEG capture should succeed");

        assert_eq!(snapshot.format, SnapshotFormat::Jpeg { quality });
        assert_eq!(snapshot.width, 800);
        assert_eq!(snapshot.height, 600);
        assert!(snapshot.bytes.starts_with(&[0xff, 0xd8]));
        assert_eq!(source.last_format(), Some(SnapshotFormat::Jpeg { quality }));
    }
}

#[tokio::test]
async fn test_snapshot_error_webview_missing() {
    let source = FakeBrowserSnapshotSource::new(FakeSnapshotBehavior::WebviewNotFound);
    let err = source
        .capture_snapshot("missing-view-42", SnapshotOptions::png())
        .await
        .expect_err("Missing webview must return error");

    assert_eq!(err.code, IpcErrorCode::WebviewNotFound);
    assert!(
        err.message.contains("missing-view-42"),
        "Error message should mention missing webview label: {}",
        err.message
    );
}

#[tokio::test]
async fn test_snapshot_error_native_capture_failure() {
    let source = FakeBrowserSnapshotSource::new(FakeSnapshotBehavior::NativeError(
        "GPU context lost during takeSnapshot".into(),
    ));

    let err = source
        .capture_snapshot("webview-main", SnapshotOptions::png())
        .await
        .expect_err("Native capture failure must return error");

    assert_eq!(err.code, IpcErrorCode::BrowserScreenshotFailed);
    assert!(
        err.message.contains("GPU context lost"),
        "Error message should contain native failure detail: {}",
        err.message
    );
}

#[tokio::test]
async fn test_snapshot_error_null_image_data() {
    let source = FakeBrowserSnapshotSource::new(FakeSnapshotBehavior::NullImage);

    let err = source
        .capture_snapshot("webview-main", SnapshotOptions::png())
        .await
        .expect_err("Null image data must return error");

    assert_eq!(err.code, IpcErrorCode::BrowserScreenshotFailed);
    assert!(
        err.message.contains("null image"),
        "Error message should mention null image: {}",
        err.message
    );
}

#[tokio::test]
async fn test_snapshot_error_bitmap_conversion_failure() {
    let source = FakeBrowserSnapshotSource::new(FakeSnapshotBehavior::BitmapDecodeError);

    let err = source
        .capture_snapshot("webview-main", SnapshotOptions::png())
        .await
        .expect_err("Bitmap decode failure must return error");

    assert_eq!(err.code, IpcErrorCode::BrowserScreenshotFailed);
    assert!(
        err.message.contains("bitmap"),
        "Error message should indicate bitmap error: {}",
        err.message
    );
}

#[tokio::test]
async fn test_snapshot_error_image_encode_failure() {
    // Test PNG encode failure
    let source_png = FakeBrowserSnapshotSource::new(FakeSnapshotBehavior::EncodeError);
    let err_png = source_png
        .capture_snapshot("webview-main", SnapshotOptions::png())
        .await
        .expect_err("PNG encode failure must return error");
    assert_eq!(err_png.code, IpcErrorCode::BrowserScreenshotFailed);
    assert!(
        err_png.message.contains("PNG"),
        "Error should reference PNG encode failure: {}",
        err_png.message
    );

    // Test JPEG encode failure
    let source_jpeg = FakeBrowserSnapshotSource::new(FakeSnapshotBehavior::EncodeError);
    let err_jpeg = source_jpeg
        .capture_snapshot("webview-main", SnapshotOptions::jpeg(80))
        .await
        .expect_err("JPEG encode failure must return error");
    assert_eq!(err_jpeg.code, IpcErrorCode::BrowserScreenshotFailed);
    assert!(
        err_jpeg.message.contains("JPEG"),
        "Error should reference JPEG encode failure: {}",
        err_jpeg.message
    );
}

#[test]
fn test_coordinator_single_use_delivery() {
    let (coordinator, mut rx) = SnapshotCallbackCoordinator::new();
    let snap = BrowserSnapshot::new(sample_valid_png_bytes(), SnapshotFormat::Png, 640, 480);

    // First completion must succeed
    assert!(coordinator.complete(Ok(snap.clone())));
    assert_eq!(coordinator.call_count(), 1);

    // Receiver must receive the snapshot
    let received = rx.try_recv().expect("Message should be available");
    let result = received.expect("Result should be Ok");
    assert_eq!(result.width, 640);
    assert_eq!(result.height, 480);
    assert_eq!(result.format, SnapshotFormat::Png);
}

#[test]
fn test_coordinator_duplicate_callback_suppression() {
    let (coordinator, mut rx) = SnapshotCallbackCoordinator::new();
    let snap1 = BrowserSnapshot::new(sample_valid_png_bytes(), SnapshotFormat::Png, 100, 100);
    let snap2 = BrowserSnapshot::new(sample_valid_jpeg_bytes(), SnapshotFormat::Jpeg { quality: 70 }, 200, 200);

    // First completion succeeds
    assert!(coordinator.complete(Ok(snap1)));
    assert_eq!(coordinator.call_count(), 1);

    // Second (duplicate) callback is suppressed and returns false
    assert!(!coordinator.complete(Ok(snap2)));
    assert_eq!(coordinator.call_count(), 2);

    // Third completion is also suppressed
    assert!(!coordinator.complete(Err(crate::ipc::error::IpcError::new(
        IpcErrorCode::BrowserScreenshotFailed,
        "late error",
    ))));
    assert_eq!(coordinator.call_count(), 3);

    // Receiver receives strictly the first snapshot
    let delivered = rx.try_recv().expect("Message must be ready").expect("First snapshot was Ok");
    assert_eq!(delivered.width, 100);
    assert_eq!(delivered.format, SnapshotFormat::Png);
}

#[tokio::test]
async fn test_coordinator_late_callback_arrival_after_timeout() {
    let delayed_snap = BrowserSnapshot::new(sample_valid_png_bytes(), SnapshotFormat::Png, 320, 240);
    let source = FakeBrowserSnapshotSource::new(FakeSnapshotBehavior::Delayed {
        delay: Duration::from_millis(60),
        result: Ok(delayed_snap),
    })
    .with_timeout(Duration::from_millis(15));

    let err = source
        .capture_snapshot("webview-main", SnapshotOptions::png())
        .await
        .expect_err("Capture should time out before delayed callback completes");

    assert_eq!(err.code, IpcErrorCode::BrowserScreenshotFailed);
    assert!(err.message.contains("timed out"));

    // Wait for the background delayed task to fire and ensure no panic on late delivery
    tokio::time::sleep(Duration::from_millis(80)).await;
}

#[test]
fn test_coordinator_close_after_callback_safety() {
    let (coordinator, rx) = SnapshotCallbackCoordinator::new();

    // Receiver drops before completion (e.g. caller cancelled or timeout occurred)
    drop(rx);

    // Coordinator completing into dropped receiver returns false and does NOT panic
    let result = coordinator.complete(Ok(BrowserSnapshot::new(
        sample_valid_png_bytes(),
        SnapshotFormat::Png,
        50,
        50,
    )));
    assert!(!result, "Completion into closed receiver must return false");
    assert_eq!(coordinator.call_count(), 1);
}

#[tokio::test]
async fn test_platform_unsupported_error_propagation() {
    let source = UnsupportedSnapshotSource;
    let err = source
        .capture_snapshot("any-webview", SnapshotOptions::png())
        .await
        .expect_err("Unsupported platform must return typed error");

    assert_eq!(err.code, IpcErrorCode::Unsupported);
    assert!(
        err.message.contains("unavailable on this platform"),
        "Error message must indicate platform unavailability: {}",
        err.message
    );
}

#[test]
fn test_capture_dimension_clamping_and_pixel_limits() {
    // 1. Normal dimensions within budget are unchanged
    let (w1, h1) = clamp_capture_dimensions(1920, 1080);
    assert_eq!((w1, h1), (1920, 1080));

    // 2. Max edge cap (2048) enforced with aspect ratio preserved
    let (w2, h2) = clamp_capture_dimensions(4096, 2048);
    assert!(w2 <= MAX_CAPTURE_EDGE, "w2 ({w2}) must be <= {MAX_CAPTURE_EDGE}");
    assert!(h2 <= MAX_CAPTURE_EDGE, "h2 ({h2}) must be <= {MAX_CAPTURE_EDGE}");
    assert_eq!(w2, 2048);
    assert_eq!(h2, 1024);

    // 3. Max pixels cap (4,000,000) enforced: 2048x2048 = 4,194,304 > 4,000,000
    let (w3, h3) = clamp_capture_dimensions(2048, 2048);
    let total_pixels = (w3 as u64) * (h3 as u64);
    assert!(
        total_pixels <= MAX_CAPTURE_PIXELS,
        "Total pixels {total_pixels} must be <= {MAX_CAPTURE_PIXELS}"
    );
    assert_eq!(w3, h3); // Preserved aspect ratio

    // 4. Zero dimensions handled safely
    let (w4, h4) = clamp_capture_dimensions(0, 0);
    assert_eq!((w4, h4), (1, 1));
}

#[tokio::test]
async fn test_coordinator_late_callback_quarantine_and_permit_release() {
    let (coordinator, rx) = SnapshotCallbackCoordinator::new();

    let permit_released = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let permit_released_clone = std::sync::Arc::clone(&permit_released);

    coordinator.set_permit_releaser(move || {
        permit_released_clone.store(true, std::sync::atomic::Ordering::SeqCst);
    });

    // Simulate timeout occurrence
    let _timeout_err = await_coordinator_completion(&coordinator, rx, Duration::from_millis(10))
        .await
        .expect_err("Should time out");

    assert!(coordinator.is_timed_out());
    assert!(!permit_released.load(std::sync::atomic::Ordering::SeqCst), "Permit not released until callback arrives");

    // Late arriving callback after timeout
    let late_snapshot = BrowserSnapshot::new(sample_valid_png_bytes(), SnapshotFormat::Png, 800, 600);
    let delivered = coordinator.complete(Ok(late_snapshot));

    // Late delivery must be quarantined (returned false)
    assert!(!delivered, "Late arrival must be quarantined/discarded");
    assert_eq!(coordinator.late_arrivals(), 1);

    // Permit releaser must be invoked without leaks
    assert!(
        permit_released.load(std::sync::atomic::Ordering::SeqCst),
        "Permit must be released when quarantined late callback is discarded"
    );
}
