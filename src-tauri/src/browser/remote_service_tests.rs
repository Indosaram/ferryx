//! Unit tests for BrowserRemoteService, concurrency budgets, lifecycle, and admission
//! Authoritative Spec: docs/plans/REMOTE_BROWSER_SCREENCAST_PLAN_2026-09-17.md (§4.3, §4.4, §8.1, Phase 7A)

use super::manager::BrowserManager;
use super::model::*;
use super::remote_bridge_protocol::{decode_frame, FRAME_FORMAT_JPEG, MAX_FRAME_PAYLOAD_BYTES};
use super::remote_driver::*;
use super::remote_service::*;
use super::snapshot_source::{FakeBrowserSnapshotSource, FakeSnapshotBehavior};
use crate::ipc::browser_cli::RemoteBrowserOperation;
use std::sync::Arc;

fn setup_test_environment() -> (BrowserRemoteService, BrowserManager, String, String) {
    let manager = BrowserManager::new();

    // Register browser session 1
    let b1 = manager
        .register_session(CreateBrowserRequest {
            browser_id: Some("browser-test-1".into()),
            workspace_id: Some("ws-alpha".into()),
            worktree_path: None,
            url: "https://example.com/home".into(),
            profile: None,
            zoom_factor: None,
            bounds: Some(LogicalRect {
                x: 0.0,
                y: 0.0,
                width: 1024.0,
                height: 768.0,
            }),
            visible: Some(true),
        })
        .expect("Create browser session 1");

    // Register browser session 2
    let b2 = manager
        .register_session(CreateBrowserRequest {
            browser_id: Some("browser-test-2".into()),
            workspace_id: Some("ws-alpha".into()),
            worktree_path: None,
            url: "https://example.com/settings".into(),
            profile: None,
            zoom_factor: None,
            bounds: Some(LogicalRect {
                x: 0.0,
                y: 0.0,
                width: 1024.0,
                height: 768.0,
            }),
            visible: Some(true),
        })
        .expect("Create browser session 2");

    let broker = Arc::new(RemoteDriverBroker::new());
    let service = BrowserRemoteService::new(manager.clone(), broker);
    (service, manager, b1.browser_id, b2.browser_id)
}

#[test]
fn test_concurrency_budget_max_viewers_and_captured_browsers() {
    let (service, _manager, b1, b2) = setup_test_environment();

    // §8.1 Budget Verification:
    // MAX_VIEWERS_PER_BROWSER = 2
    // MAX_CONCURRENT_CAPTURED_BROWSERS = 1
    // MAX_GLOBAL_DRIVERS = 1

    assert_eq!(MAX_VIEWERS_PER_BROWSER, 2);
    assert_eq!(MAX_CONCURRENT_CAPTURED_BROWSERS, 1);
    assert_eq!(MAX_GLOBAL_DRIVERS, 1);

    // 1. Browser 1: viewer 1 subscribes -> ok
    let sub1 = service
        .subscribe(&b1, "device-1", "viewer-1")
        .expect("Viewer 1 should subscribe");

    // 2. Browser 1: viewer 2 subscribes -> ok
    let sub2 = service
        .subscribe(&b1, "device-2", "viewer-2")
        .expect("Viewer 2 should subscribe");

    // 3. Browser 1: viewer 3 subscribes -> rejected (max 2 viewers per browser)
    let sub3_err = service
        .subscribe(&b1, "device-3", "viewer-3")
        .expect_err("Viewer 3 should exceed viewer limit");
    assert!(
        matches!(sub3_err, RemoteServiceError::QuotaExceeded(msg) if msg.contains("2 viewers")),
        "Expected QuotaExceeded for viewers: {:?}",
        sub3_err
    );

    // 4. Concurrently captured browsers: attempting to subscribe to browser 2 while browser 1 is active
    // must fail with QuotaExceeded (max 1 concurrently captured browser)
    let b2_sub_err = service
        .subscribe(&b2, "device-4", "viewer-4")
        .expect_err("Subscribing to second browser must exceed captured browser budget");
    assert!(
        matches!(b2_sub_err, RemoteServiceError::QuotaExceeded(msg) if msg.contains("1 concurrently captured browser")),
        "Expected QuotaExceeded for captured browsers: {:?}",
        b2_sub_err
    );

    // 5. Unsubscribe all viewers from browser 1 -> releases captured browser permit
    assert!(service.unsubscribe(&b1, &sub1));
    assert!(service.unsubscribe(&b1, &sub2));
    assert_eq!(service.subscriber_count(&b1), 0);
    assert_eq!(service.active_capture_count(), 0);

    // 6. Now browser 2 can be subscribed and captured
    let b2_sub_ok = service
        .subscribe(&b2, "device-4", "viewer-4")
        .expect("Browser 2 subscription should succeed now");
    assert_eq!(service.active_capture_count(), 1);
    assert!(service.unsubscribe(&b2, &b2_sub_ok));
}

#[test]
fn test_concurrency_budget_single_global_driver() {
    let (service, _manager, b1, b2) = setup_test_environment();
    let sub1 = service.subscribe(&b1, "device-1", "v1").unwrap();
    let sub2 = service.subscribe(&b1, "device-2", "v2").unwrap();

    let broker = service.driver_broker();

    // Device 1 claims driver lease on browser 1 -> succeeds
    let lease1 = broker
        .claim("device-1", "conn-1", &sub1, &b1, true)
        .expect("Device 1 claim should succeed");
    assert_eq!(lease1.device_id, "device-1");

    // Device 2 attempts to claim driver lease -> rejected with BrowserDriverBusy (MAX_GLOBAL_DRIVERS = 1)
    let err_dev2 = broker
        .claim("device-2", "conn-2", &sub2, &b1, true)
        .expect_err("Device 2 claim should be rejected");
    assert!(matches!(
        err_dev2,
        RemoteDriverError::BrowserDriverBusy { .. }
    ));

    // Even attempting to claim on a different browser (browser 2) must be rejected because driver is GLOBAL
    let err_diff_browser = broker
        .claim("device-2", "conn-2", "sub-diff", &b2, true)
        .expect_err("Claiming another browser must be rejected: global driver is busy");
    assert!(matches!(
        err_diff_browser,
        RemoteDriverError::BrowserDriverBusy { .. }
    ));

    // Releasing device 1 lease frees the global driver slot
    assert!(broker.release(&sub1, lease1.lease_epoch).unwrap());

    // Device 3 can now acquire the global driver lease
    let lease3 = broker
        .claim("device-3", "conn-3", "sub-3", &b1, true)
        .expect("Device 3 should acquire driver lease after release");
    assert_eq!(lease3.device_id, "device-3");
}

#[test]
fn test_producer_lifecycle_pause_on_zero_subscribers_and_resume() {
    let (service, _manager, b1, _) = setup_test_environment();

    // Initial state: 0 subscribers, producer inactive
    assert!(!service.is_producer_active(&b1));
    assert_eq!(service.active_capture_count(), 0);

    // 1st subscriber joins -> producer immediately activates
    let sub1 = service.subscribe(&b1, "dev1", "v1").unwrap();
    assert!(service.is_producer_active(&b1));
    assert_eq!(service.active_capture_count(), 1);

    // 2nd subscriber joins -> producer remains active (shared producer)
    let sub2 = service.subscribe(&b1, "dev2", "v2").unwrap();
    assert!(service.is_producer_active(&b1));
    assert_eq!(service.active_capture_count(), 1);

    // 1st subscriber leaves -> 1 subscriber remains, producer remains active
    assert!(service.unsubscribe(&b1, &sub1));
    assert!(service.is_producer_active(&b1));
    assert_eq!(service.active_capture_count(), 1);

    // 2nd subscriber leaves -> 0 subscribers remain: producer immediately pauses
    assert!(service.unsubscribe(&b1, &sub2));
    assert!(
        !service.is_producer_active(&b1),
        "0 subscribers must immediately pause capture"
    );
    assert_eq!(service.active_capture_count(), 0);

    // Re-subscribing a new viewer resumes producer capture
    let sub3 = service.subscribe(&b1, "dev3", "v3").unwrap();
    assert!(
        service.is_producer_active(&b1),
        "New subscriber must resume capture"
    );
    assert_eq!(service.active_capture_count(), 1);

    assert!(service.unsubscribe(&b1, &sub3));
    assert!(!service.is_producer_active(&b1));
}

#[test]
fn test_latest_only_frame_admission_and_oversized_drop() {
    let (service, _manager, b1, _) = setup_test_environment();
    let sub1 = service.subscribe(&b1, "dev1", "v1").unwrap();

    // 1. Frame 1 admitted immediately (1 in-flight)
    let frame1 = vec![0x11, 0x22, 0x33];
    let immediate1 = service.admit_frame(&b1, 101, frame1.clone()).unwrap();
    assert_eq!(immediate1, vec![sub1.clone()]);

    // 2. While Frame 1 is unacknowledged, Frame 2 arrives -> queued in pending slot
    let frame2 = vec![0x44, 0x55, 0x66];
    let immediate2 = service.admit_frame(&b1, 102, frame2.clone()).unwrap();
    assert!(
        immediate2.is_empty(),
        "Busy viewer should not get immediate delivery"
    );

    // 3. Frame 3 arrives before Frame 1 is acknowledged -> displaces Frame 2 (latest-only)
    let frame3 = vec![0x77, 0x88, 0x99];
    let immediate3 = service.admit_frame(&b1, 103, frame3.clone()).unwrap();
    assert!(immediate3.is_empty());

    // 4. Viewer acknowledges Frame 1 -> pending Frame 3 is returned (Frame 2 was dropped)
    let next_frame = service.acknowledge_frame(&b1, &sub1, 101);
    assert_eq!(
        next_frame,
        Some(frame3),
        "Viewer must receive the latest pending frame (frame 3)"
    );

    // 5. Oversized frame (> 2 MiB) is permanently dropped
    let oversized = vec![0xAA; MAX_FRAME_PAYLOAD_BYTES + 1];
    let oversized_admitted = service.admit_frame(&b1, 104, oversized).unwrap();
    assert!(
        oversized_admitted.is_empty(),
        "Oversized frame must be permanently dropped"
    );
}

#[test]
fn test_guard_verification_rejections() {
    let (service, manager, b1, _) = setup_test_environment();
    let sub = service.subscribe(&b1, "dev1", "v1").unwrap();

    let lease = service
        .driver_broker()
        .claim("dev1", "conn1", &sub, &b1, true)
        .unwrap();

    let valid_instance = manager.get_instance_id(&b1).unwrap();
    let valid_desktop_epoch = service.desktop_epoch();
    let valid_gen = manager.get_state(&b1).unwrap().generation;

    // 1. All valid guards pass
    assert!(service
        .execute_command_guard(
            &b1,
            lease.lease_epoch,
            "dev1",
            "conn1",
            &valid_instance,
            valid_desktop_epoch,
            valid_gen
        )
        .is_ok());

    // 2. Stale lease epoch rejected
    let stale_lease_err = service.execute_command_guard(
        &b1,
        lease.lease_epoch + 99,
        "dev1",
        "conn1",
        &valid_instance,
        valid_desktop_epoch,
        valid_gen,
    );
    assert_eq!(stale_lease_err, Err(RemoteServiceError::StaleLease));

    // 3. Stale browser instance ID rejected
    let stale_instance_err = service.execute_command_guard(
        &b1,
        lease.lease_epoch,
        "dev1",
        "conn1",
        "stale-instance-xyz",
        valid_desktop_epoch,
        valid_gen,
    );
    assert_eq!(stale_instance_err, Err(RemoteServiceError::StaleInstance));

    // 4. Stale desktop epoch rejected
    let stale_epoch_err = service.execute_command_guard(
        &b1,
        lease.lease_epoch,
        "dev1",
        "conn1",
        &valid_instance,
        valid_desktop_epoch + 1,
        valid_gen,
    );
    assert_eq!(stale_epoch_err, Err(RemoteServiceError::StaleLease));

    // 5. Stale document generation rejected
    let stale_gen_err = service.execute_command_guard(
        &b1,
        lease.lease_epoch,
        "dev1",
        "conn1",
        &valid_instance,
        valid_desktop_epoch,
        valid_gen + 1,
    );
    assert_eq!(stale_gen_err, Err(RemoteServiceError::StaleGeneration));

    // 6. Desktop owner reclaim revokes lease
    let _ = service.driver_broker().desktop_reclaim();
    let reclaimed_guard_err = service.execute_command_guard(
        &b1,
        lease.lease_epoch,
        "dev1",
        "conn1",
        &valid_instance,
        valid_desktop_epoch,
        valid_gen,
    );
    assert_eq!(
        reclaimed_guard_err,
        Err(RemoteServiceError::DesktopReclaimed)
    );
}

#[test]
fn test_dom_snapshot_map_revision_url_change_increments_revision() {
    let (service, manager, b1, _) = setup_test_environment();
    let state_initial = manager.get_state(&b1).unwrap();

    let targets1 = vec![
        BrowserAutomationTarget {
            reference: "search-box".into(),
            selector: "input#search".into(),
        },
        BrowserAutomationTarget {
            reference: "submit-btn".into(),
            selector: "button[type='submit']".into(),
        },
    ];

    // Record snapshot on initial document
    let (snap1_id, rev1) = service
        .record_snapshot(&b1, state_initial.generation, targets1)
        .expect("Initial snapshot recording");

    // Reference lookup succeeds
    let found_selector = service
        .verify_snapshot_ref(&b1, &snap1_id, rev1, "search-box")
        .expect("Snapshot reference should resolve");
    assert_eq!(found_selector, "input#search");

    // Navigation / URL change occurs
    manager
        .update_navigation_state(
            &b1,
            Some("https://example.com/search-results".into()),
            Some("Search Results".into()),
            Some(false),
            Some(true),
            Some(false),
            None,
        )
        .expect("Navigation update");

    let state_after_nav = manager.get_state(&b1).unwrap();
    assert_eq!(state_after_nav.url, "https://example.com/search-results");
    assert!(state_after_nav.generation > state_initial.generation);

    // Verification with old snapshot_id or rev1 MUST fail with SnapshotMapMismatch
    let stale_lookup = service.verify_snapshot_ref(&b1, &snap1_id, rev1, "search-box");
    assert_eq!(stale_lookup, Err(RemoteServiceError::SnapshotMapMismatch));

    // Recording new snapshot on the navigated page yields a higher map revision
    let targets2 = vec![BrowserAutomationTarget {
        reference: "first-result".into(),
        selector: ".result-item:first-child".into(),
    }];

    let (snap2_id, rev2) = service
        .record_snapshot(&b1, state_after_nav.generation, targets2)
        .expect("Snapshot on new URL should succeed");

    assert_ne!(snap1_id, snap2_id, "Snapshot IDs must differ");
    assert!(
        rev2 > rev1,
        "URL change must increment snapshot map revision ({} > {})",
        rev2,
        rev1
    );

    // New reference resolves correctly
    let new_selector = service
        .verify_snapshot_ref(&b1, &snap2_id, rev2, "first-result")
        .expect("New target should resolve");
    assert_eq!(new_selector, ".result-item:first-child");
}

#[tokio::test]
async fn test_real_capture_producer_streaming_and_pause_on_zero_subscribers() {
    let (service, _manager, b1, _) = setup_test_environment();

    // Attach fake snapshot source
    let fake_source = Arc::new(FakeBrowserSnapshotSource::new(FakeSnapshotBehavior::Auto {
        width: 800,
        height: 600,
    }));
    service.set_snapshot_source(fake_source.clone());

    // Subscribe frames broadcast receiver BEFORE subscribing to capture stream
    let mut frame_rx = service.subscribe_frames(&b1);

    // Initial state: producer inactive
    assert!(!service.is_producer_active(&b1));

    // Transition subscriber count from 0 -> 1 spawns Tokio capture loop
    let sub = service.subscribe(&b1, "dev-capture", "v-capture").unwrap();
    assert!(service.is_producer_active(&b1));

    // Await frame from the live capture producer
    let frame_bytes = tokio::time::timeout(std::time::Duration::from_millis(500), frame_rx.recv())
        .await
        .expect("Capture loop must emit frame within timeout")
        .expect("Frame channel must not be closed");

    // Decode and verify 16-byte binary frame structure
    let (header, metadata, img) = decode_frame(&frame_bytes).expect("Decoded frame must be valid");
    assert_eq!(header.format, FRAME_FORMAT_JPEG);
    assert_eq!(metadata.image_width, 1024);
    assert_eq!(metadata.image_height, 768);
    assert_eq!(metadata.geometry_source, "wkSnapshot");
    assert_eq!(metadata.stream_id, 1);
    assert_eq!(metadata.viewport_revision, "1");
    assert!(!img.is_empty(), "Image payload must not be empty");

    // Capture count increments on fake source
    assert!(fake_source.call_count() >= 1);

    // Unsubscribe last viewer -> 0 subscribers -> producer pauses immediately
    assert!(service.unsubscribe(&b1, &sub));
    assert!(!service.is_producer_active(&b1));
}

#[test]
fn test_remote_snapshot_reference_namespace_isolation() {
    let manager = BrowserManager::new();
    let b = manager
        .register_session(CreateBrowserRequest {
            browser_id: Some("browser-isolation".into()),
            workspace_id: Some("ws-alpha".into()),
            worktree_path: None,
            url: "https://example.com/app".into(),
            profile: None,
            zoom_factor: None,
            bounds: None,
            visible: Some(true),
        })
        .unwrap();

    let initial_gen = manager.get_state(&b.browser_id).unwrap().generation;

    // 1. Record remote snapshot targets
    let remote_targets = vec![BrowserAutomationTarget {
        reference: "btn-primary".into(),
        selector: "#remote-button".into(),
    }];
    let (snap_id, map_rev) = manager
        .record_remote_snapshot(&b.browser_id, initial_gen, remote_targets)
        .unwrap();

    // Verify remote target resolves
    let remote_sel = manager
        .verify_remote_target(&b.browser_id, &snap_id, map_rev, "btn-primary")
        .unwrap();
    assert_eq!(remote_sel, "#remote-button");

    // 2. Perform a legacy DOM scan that writes into legacy automation_targets
    let legacy_targets = vec![BrowserAutomationTarget {
        reference: "btn-primary".into(),
        selector: "#legacy-button-overwritten".into(),
    }];
    manager
        .record_automation_targets(&b.browser_id, initial_gen, legacy_targets)
        .unwrap();

    // Legacy lookup resolves the legacy selector
    let legacy_sel = manager
        .automation_target(&b.browser_id, initial_gen, "btn-primary")
        .unwrap();
    assert_eq!(legacy_sel, "#legacy-button-overwritten");

    // CRITICAL: Remote target MUST remain isolated and NOT be overwritten by the legacy DOM scan
    let remote_sel_after_legacy_scan = manager
        .verify_remote_target(&b.browser_id, &snap_id, map_rev, "btn-primary")
        .unwrap();
    assert_eq!(
        remote_sel_after_legacy_scan, "#remote-button",
        "Legacy DOM scan must not overwrite remote snapshot targets"
    );
}

#[test]
fn test_remote_browser_operation_validation_and_legacy_isolation() {
    // 1. Valid operations pass validation
    let nav_op = RemoteBrowserOperation::Navigate {
        browser_id: "b1".into(),
        url: "https://example.com".into(),
    };
    assert!(nav_op.validate().is_ok());

    let click_ref_op = RemoteBrowserOperation::Click {
        browser_id: "b1".into(),
        reference: Some("btn-1".into()),
        snapshot_id: Some("s1".into()),
        map_revision: Some(1),
        u: None,
        v: None,
        stream_id: None,
        sequence_number: None,
        document_generation: None,
        viewport_revision: None,
        capture_rect: None,
        geometry_source: None,
        x: None,
        y: None,
    };
    assert!(click_ref_op.validate().is_ok());

    let click_point_op = RemoteBrowserOperation::Click {
        browser_id: "b1".into(),
        reference: None,
        snapshot_id: None,
        map_revision: None,
        u: Some(0.5),
        v: Some(0.5),
        stream_id: None,
        sequence_number: None,
        document_generation: None,
        viewport_revision: None,
        capture_rect: None,
        geometry_source: None,
        x: None,
        y: None,
    };
    assert!(click_point_op.validate().is_ok());

    // 2. Invalid inputs are rejected
    // Click with out of bounds coordinates
    let click_oob = RemoteBrowserOperation::Click {
        browser_id: "b1".into(),
        reference: None,
        snapshot_id: None,
        map_revision: None,
        u: Some(1.5), // > 1.0
        v: Some(0.5),
        stream_id: None,
        sequence_number: None,
        document_generation: None,
        viewport_revision: None,
        capture_rect: None,
        geometry_source: None,
        x: None,
        y: None,
    };
    assert!(click_oob.validate().is_err());

    // Click with NaN coordinate
    let click_nan = RemoteBrowserOperation::Click {
        browser_id: "b1".into(),
        reference: None,
        snapshot_id: None,
        map_revision: None,
        u: Some(f64::NAN),
        v: Some(0.5),
        stream_id: None,
        sequence_number: None,
        document_generation: None,
        viewport_revision: None,
        capture_rect: None,
        geometry_source: None,
        x: None,
        y: None,
    };
    assert!(click_nan.validate().is_err());

    // Click with empty reference
    let click_empty_ref = RemoteBrowserOperation::Click {
        browser_id: "b1".into(),
        reference: Some("   ".into()),
        snapshot_id: None,
        map_revision: None,
        u: None,
        v: None,
        stream_id: None,
        sequence_number: None,
        document_generation: None,
        viewport_revision: None,
        capture_rect: None,
        geometry_source: None,
        x: None,
        y: None,
    };
    assert!(click_empty_ref.validate().is_err());

    // Fill with oversized text (> 16 KiB)
    let fill_oversized = RemoteBrowserOperation::Fill {
        browser_id: "b1".into(),
        reference: "input1".into(),
        value: "a".repeat(20 * 1024),
        snapshot_id: None,
        map_revision: None,
    };
    assert!(fill_oversized.validate().is_err());

    // Keypress with forbidden app-chrome shortcut
    let key_forbidden = RemoteBrowserOperation::Keypress {
        browser_id: "b1".into(),
        key: "Cmd+Q".into(),
    };
    assert!(key_forbidden.validate().is_err());

    // Eval without approval
    let eval_unapproved = RemoteBrowserOperation::Eval {
        browser_id: "b1".into(),
        script: "window.location".into(),
        has_approval: false,
    };
    assert!(eval_unapproved.validate().is_err());

    // 3. Legacy CLI commands fail deserialization as RemoteBrowserOperation
    let legacy_list_json = r#"{"command":"list"}"#;
    let list_deser = serde_json::from_str::<RemoteBrowserOperation>(legacy_list_json);
    assert!(
        list_deser.is_err(),
        "Legacy CLI List command must not deserialize into RemoteBrowserOperation"
    );

    let legacy_open_json = r#"{"command":"open","url":"https://example.com"}"#;
    let open_deser = serde_json::from_str::<RemoteBrowserOperation>(legacy_open_json);
    assert!(
        open_deser.is_err(),
        "Legacy CLI Open command must not deserialize into RemoteBrowserOperation"
    );

    let legacy_close_json = r#"{"command":"close","browserId":"b1"}"#;
    let close_deser = serde_json::from_str::<RemoteBrowserOperation>(legacy_close_json);
    assert!(
        close_deser.is_err(),
        "Legacy CLI Close command must not deserialize into RemoteBrowserOperation"
    );
}

#[tokio::test]
async fn test_r1_r2_codec_unification_and_public_protocol_acceptance() {
    let (service, manager, _, _) = setup_test_environment();

    // Register browser session with 1x1 bounds to match sample_valid_jpeg_bytes dimensions
    let b_1x1 = manager
        .register_session(CreateBrowserRequest {
            browser_id: Some("browser-1x1".into()),
            workspace_id: Some("ws-alpha".into()),
            worktree_path: None,
            url: "https://example.com/1x1".into(),
            profile: None,
            zoom_factor: None,
            bounds: Some(LogicalRect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            }),
            visible: Some(true),
        })
        .expect("Create 1x1 browser session");

    // Attach fake snapshot source
    let fake_source = Arc::new(FakeBrowserSnapshotSource::new(FakeSnapshotBehavior::Auto {
        width: 1,
        height: 1,
    }));
    service.set_snapshot_source(fake_source.clone());

    let mut frame_rx = service.subscribe_frames(&b_1x1.browser_id);
    let sub = service
        .subscribe(&b_1x1.browser_id, "dev-codec", "v-codec")
        .unwrap();

    let frame_bytes = tokio::time::timeout(std::time::Duration::from_millis(500), frame_rx.recv())
        .await
        .expect("Receive frame within timeout")
        .expect("Frame channel open");

    // 1. Decode with crate::remote::browser_protocol::decode_binary_frame (authoritative wire decoder)
    let decoded = crate::remote::browser_protocol::decode_binary_frame(&frame_bytes)
        .expect("Frame output produced by BrowserRemoteService must be 100% accepted by public protocol decoder");

    // 2. Assert wire types match public TypeScript contract:
    // stream_id is u32 bound to negotiated stream
    assert_eq!(decoded.metadata.stream_id, 1);
    // viewport_revision is decimal string
    assert_eq!(decoded.metadata.viewport_revision, "1");
    // capture rect logical coordinates
    assert_eq!(decoded.metadata.capture_rect.width, 1.0);
    assert_eq!(decoded.metadata.capture_rect.height, 1.0);
    assert_eq!(decoded.metadata.image_width, 1);
    assert_eq!(decoded.metadata.image_height, 1);
    assert_eq!(decoded.metadata.geometry_source, "wkSnapshot");

    // 3. Verify serialization contains decimal string viewportRevision
    let json_meta = serde_json::to_string(&decoded.metadata).unwrap();
    assert!(json_meta.contains(r#""viewportRevision":"1""#));
    assert!(json_meta.contains(r#""streamId":1"#));

    // 4. Verify MAX_IMAGE_PIXELS is 4_000_000
    assert_eq!(crate::remote::browser_protocol::MAX_IMAGE_PIXELS, 4_000_000);
    assert_eq!(super::remote_bridge_protocol::MAX_IMAGE_PIXELS, 4_000_000);

    service.unsubscribe(&b_1x1.browser_id, &sub);
}

#[tokio::test]
async fn test_r7_producer_abort_handle_cancellation_and_resubscription() {
    let (service, _manager, b1, _) = setup_test_environment();

    let fake_source = Arc::new(FakeBrowserSnapshotSource::new(FakeSnapshotBehavior::Auto {
        width: 800,
        height: 600,
    }));
    service.set_snapshot_source(fake_source.clone());

    // 1. Initial state: 0 active
    assert_eq!(service.active_capture_count(), 0);
    assert!(!service.is_producer_active(&b1));

    // 2. Subscribe -> producer spawns
    let sub1 = service.subscribe(&b1, "dev-abort-1", "v1").unwrap();
    assert_eq!(service.active_capture_count(), 1);
    assert!(service.is_producer_active(&b1));

    // Wait a tick so task runs
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // 3. Unsubscribe -> 0 subscribers -> producer aborted immediately
    assert!(service.unsubscribe(&b1, &sub1));
    assert_eq!(service.active_capture_count(), 0);
    assert!(!service.is_producer_active(&b1));

    // 4. Rapid resubscription: subscribe immediately without delay
    let sub2 = service.subscribe(&b1, "dev-abort-2", "v2").unwrap();
    assert_eq!(service.active_capture_count(), 1);
    assert!(service.is_producer_active(&b1));

    // Wait a tick: new producer task is running cleanly
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert_eq!(service.active_capture_count(), 1);

    assert!(service.unsubscribe(&b1, &sub2));
    assert_eq!(service.active_capture_count(), 0);
}

#[test]
fn test_r9_remote_reference_operations_require_snapshot_and_revision() {
    // 1. Reference click requires snapshot_id and map_revision
    let click_valid = RemoteBrowserOperation::Click {
        browser_id: "b1".into(),
        reference: Some("btn-1".into()),
        snapshot_id: Some("snap-1".into()),
        map_revision: Some(1),
        u: None,
        v: None,
        stream_id: None,
        sequence_number: None,
        document_generation: None,
        viewport_revision: None,
        capture_rect: None,
        geometry_source: None,
        x: None,
        y: None,
    };
    assert!(click_valid.validate().is_ok());

    let click_missing_snap = RemoteBrowserOperation::Click {
        browser_id: "b1".into(),
        reference: Some("btn-1".into()),
        snapshot_id: None,
        map_revision: Some(1),
        u: None,
        v: None,
        stream_id: None,
        sequence_number: None,
        document_generation: None,
        viewport_revision: None,
        capture_rect: None,
        geometry_source: None,
        x: None,
        y: None,
    };
    let err = click_missing_snap.validate().unwrap_err();
    assert_eq!(
        format!("{:?}", err.code),
        "Custom(\"BROWSER_INVALID_SNAPSHOT\")"
    );

    let click_empty_snap = RemoteBrowserOperation::Click {
        browser_id: "b1".into(),
        reference: Some("btn-1".into()),
        snapshot_id: Some("   ".into()),
        map_revision: Some(1),
        u: None,
        v: None,
        stream_id: None,
        sequence_number: None,
        document_generation: None,
        viewport_revision: None,
        capture_rect: None,
        geometry_source: None,
        x: None,
        y: None,
    };
    let err2 = click_empty_snap.validate().unwrap_err();
    assert_eq!(
        format!("{:?}", err2.code),
        "Custom(\"BROWSER_INVALID_SNAPSHOT\")"
    );

    let click_missing_rev = RemoteBrowserOperation::Click {
        browser_id: "b1".into(),
        reference: Some("btn-1".into()),
        snapshot_id: Some("snap-1".into()),
        map_revision: None,
        u: None,
        v: None,
        stream_id: None,
        sequence_number: None,
        document_generation: None,
        viewport_revision: None,
        capture_rect: None,
        geometry_source: None,
        x: None,
        y: None,
    };
    let err3 = click_missing_rev.validate().unwrap_err();
    assert_eq!(
        format!("{:?}", err3.code),
        "Custom(\"BROWSER_INVALID_SNAPSHOT\")"
    );

    // 2. Coordinate click does NOT require snapshot_id
    let click_coord = RemoteBrowserOperation::Click {
        browser_id: "b1".into(),
        reference: None,
        snapshot_id: None,
        map_revision: None,
        u: Some(0.5),
        v: Some(0.5),
        stream_id: None,
        sequence_number: None,
        document_generation: None,
        viewport_revision: None,
        capture_rect: None,
        geometry_source: None,
        x: None,
        y: None,
    };
    assert!(click_coord.validate().is_ok());

    // 3. Fill requires snapshot_id and map_revision
    let fill_valid = RemoteBrowserOperation::Fill {
        browser_id: "b1".into(),
        reference: "field1".into(),
        value: "hello".into(),
        snapshot_id: Some("snap-1".into()),
        map_revision: Some(1),
    };
    assert!(fill_valid.validate().is_ok());

    let fill_missing_snap = RemoteBrowserOperation::Fill {
        browser_id: "b1".into(),
        reference: "field1".into(),
        value: "hello".into(),
        snapshot_id: None,
        map_revision: Some(1),
    };
    let err4 = fill_missing_snap.validate().unwrap_err();
    assert_eq!(
        format!("{:?}", err4.code),
        "Custom(\"BROWSER_INVALID_SNAPSHOT\")"
    );
}

#[tokio::test]
async fn test_r7_native_capture_permit_retention_across_producer_restart_on_timeout() {
    let (service, _manager, b1, _) = setup_test_environment();

    // Source that times out initially
    let fake_source = Arc::new(
        FakeBrowserSnapshotSource::new(FakeSnapshotBehavior::Timeout)
            .with_timeout(std::time::Duration::from_millis(50)),
    );
    service.set_snapshot_source(fake_source.clone());

    assert_eq!(service.native_capture_semaphore().available_permits(), 1);

    // Subscribe viewer 1 -> producer starts
    let sub1 = service.subscribe(&b1, "dev-timeout-1", "v1").unwrap();

    // Wait until timeout occurs inside capture task
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    // Because timeout occurred and callback hasn't arrived, permit is retained!
    assert!(
        service.is_native_capture_active(),
        "Native capture permit must be retained across timeout while callback is in-flight"
    );
    assert_eq!(service.native_capture_semaphore().available_permits(), 0);

    // Unsubscribe viewer 1 (producer stops/aborted)
    service.unsubscribe(&b1, &sub1);

    // Producer restarted: new viewer subscribes
    let sub2 = service.subscribe(&b1, "dev-timeout-2", "v2").unwrap();

    // The permit remains retained so overlapping capture cannot run
    assert_eq!(service.native_capture_semaphore().available_permits(), 0);

    // Release quarantined permit (simulating late arrival or webview destruction)
    service.release_quarantine(&b1);
    assert_eq!(service.native_capture_semaphore().available_permits(), 1);
    assert!(!service.is_native_capture_active());

    service.unsubscribe(&b1, &sub2);
}

#[tokio::test]
async fn test_r4_7_producer_frame_sampled_before_generation_bump_is_dropped() {
    let (service, manager, b1, _) = setup_test_environment();

    let fake_source = Arc::new(
        FakeBrowserSnapshotSource::new(FakeSnapshotBehavior::Delayed {
            delay: std::time::Duration::from_millis(40),
            result: Ok(crate::browser::snapshot_source::BrowserSnapshot::new(
                crate::browser::snapshot_source::sample_valid_jpeg_bytes(),
                crate::browser::snapshot_source::SnapshotFormat::Jpeg { quality: 80 },
                800,
                600,
            )),
        })
        .with_timeout(std::time::Duration::from_millis(500)),
    );
    service.set_snapshot_source(fake_source.clone());

    let mut frame_rx = service.subscribe_frames(&b1);
    let sub = service.subscribe(&b1, "dev-c1", "v-c1").unwrap();

    // Give loop time to tick, sample generation 1, and enter snapshot await
    tokio::time::sleep(std::time::Duration::from_millis(15)).await;

    // Bump generation while snapshot is in-flight
    manager
        .update_url(&b1, "https://example.com/bumped")
        .unwrap();
    let state_after_bump = manager.get_state(&b1).unwrap();
    assert_eq!(state_after_bump.generation, 2);

    // Frame received must NOT have the stale document_generation 1
    let frame_bytes = tokio::time::timeout(std::time::Duration::from_millis(600), frame_rx.recv())
        .await
        .expect("Producer should eventually emit next fresh frame")
        .expect("Frame channel must be open");

    let (_, metadata, _) = decode_frame(&frame_bytes).expect("Valid frame");
    assert_ne!(
        metadata.document_generation, "1",
        "Stale frame sampled before generation bump must be dropped!"
    );
    assert_eq!(metadata.document_generation, "2");

    service.unsubscribe(&b1, &sub);
}

#[tokio::test]
async fn test_r4_7_producer_visibility_loss_halts_publication() {
    let (service, manager, b1, _) = setup_test_environment();

    let fake_source = Arc::new(
        FakeBrowserSnapshotSource::new(FakeSnapshotBehavior::Delayed {
            delay: std::time::Duration::from_millis(40),
            result: Ok(crate::browser::snapshot_source::BrowserSnapshot::new(
                crate::browser::snapshot_source::sample_valid_jpeg_bytes(),
                crate::browser::snapshot_source::SnapshotFormat::Jpeg { quality: 80 },
                800,
                600,
            )),
        })
        .with_timeout(std::time::Duration::from_millis(500)),
    );
    service.set_snapshot_source(fake_source.clone());

    let mut frame_rx = service.subscribe_frames(&b1);
    let sub = service.subscribe(&b1, "dev-c2", "v-c2").unwrap();

    // Give loop time to tick, sample visible=true, and enter snapshot await
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    // Hide browser while snapshot is in-flight
    manager.set_visible(&b1, false).unwrap();

    // Publication must halt: no frame emitted while hidden
    let recv_res =
        tokio::time::timeout(std::time::Duration::from_millis(250), frame_rx.recv()).await;
    assert!(
        recv_res.is_err(),
        "Frame must not be published after visibility loss"
    );

    service.unsubscribe(&b1, &sub);
}
