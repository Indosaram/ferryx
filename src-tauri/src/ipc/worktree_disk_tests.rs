use super::worktree_disk::*;
use crate::ipc::{run_blocking, IpcErrorCode};
use crate::worktree::WorkspaceRegistry;
use std::time::Duration;
use tauri::{Listener, Manager};

#[tokio::test]
async fn disk_scan_commands_emit_completion_and_reuse_cache_until_refresh() {
    let (dir, registry) = run_blocking(|| {
        let dir = tempfile::TempDir::new().unwrap();
        let registry = WorkspaceRegistry::new();
        registry.register("disk-test", dir.path()).unwrap();
        Ok((dir, registry))
    })
    .await
    .unwrap();
    let app = tauri::test::mock_builder()
        .manage(registry)
        .manage(WorktreeDiskScans::default())
        .invoke_handler(tauri::generate_handler![
            cmd_worktree_disk_scan_start,
            cmd_worktree_disk_scan_cancel,
            cmd_worktree_disk_scan_result
        ])
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    app.listen(WORKTREE_DISK_SCAN_PROGRESS_EVENT, move |event| {
        let snapshot: DiskScanSnapshot = serde_json::from_str(event.payload()).unwrap();
        tx.send(snapshot).unwrap();
    });
    assert!(
        cmd_worktree_disk_scan_result(app.state(), "disk-test".into())
            .await
            .unwrap()
            .is_none()
    );
    let webview = tauri::WebviewWindowBuilder::new(&app, "disk-test", Default::default())
        .build()
        .unwrap();
    // Exercise the generated IPC handler and actual camelCase request/response,
    // not only direct Rust calls. The listener above precedes the invocation.
    let first = tokio::time::timeout(
        Duration::from_secs(10),
        run_blocking(move || {
            let response = tauri::test::get_ipc_response(
                &webview,
                tauri::webview::InvokeRequest {
                    cmd: "cmd_worktree_disk_scan_start".into(),
                    callback: tauri::ipc::CallbackFn(0),
                    error: tauri::ipc::CallbackFn(1),
                    url: if cfg!(windows) {
                        "http://tauri.localhost"
                    } else {
                        "tauri://localhost"
                    }
                    .parse()
                    .unwrap(),
                    body: tauri::ipc::InvokeBody::Json(
                        serde_json::json!({"workspaceId": "disk-test", "refresh": false}),
                    ),
                    headers: Default::default(),
                    invoke_key: tauri::test::INVOKE_KEY.into(),
                },
            )
            .unwrap();
            Ok(response.deserialize::<DiskScanSnapshot>().unwrap())
        }),
    )
    .await
    .unwrap()
    .unwrap();
    let completed = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let event = rx.recv().await.unwrap();
            if event.scan_id == first.scan_id && event.status == DiskScanStatus::Completed {
                break event;
            }
        }
    })
    .await
    .unwrap();
    assert!(completed.rows.is_empty());
    let result = cmd_worktree_disk_scan_result(app.state(), "disk-test".into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.status, DiskScanStatus::Completed);
    let cached = cmd_worktree_disk_scan_start(
        app.handle().clone(),
        app.state(),
        app.state(),
        "disk-test".into(),
        false,
    )
    .await
    .unwrap();
    assert_eq!(cached.scan_id, first.scan_id);
    let refresh = cmd_worktree_disk_scan_start(
        app.handle().clone(),
        app.state(),
        app.state(),
        "disk-test".into(),
        true,
    )
    .await
    .unwrap();
    assert_ne!(refresh.scan_id, first.scan_id);
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let event = rx.recv().await.unwrap();
            if event.scan_id == refresh.scan_id && event.status == DiskScanStatus::Completed {
                break;
            }
        }
    })
    .await
    .unwrap();
    assert!(!cmd_worktree_disk_scan_cancel(
        app.handle().clone(),
        app.state(),
        "disk-test".into(),
        first.scan_id
    )
    .await
    .unwrap());
    let error = cmd_worktree_disk_scan_start(
        app.handle().clone(),
        app.state(),
        app.state(),
        "ssh:host/project".into(),
        false,
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, IpcErrorCode::Unsupported);
    run_blocking(move || {
        drop(dir);
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn disk_scan_cancel_command_stops_a_live_walk_without_caching_partial_bytes() {
    use crate::worktree::disk::scan_path;
    let dir = run_blocking(|| {
        let dir = tempfile::TempDir::new().unwrap();
        for index in 0..20 {
            std::fs::write(dir.path().join(index.to_string()), b"payload").unwrap();
        }
        Ok(dir)
    })
    .await
    .unwrap();
    let scans = WorktreeDiskScans::default();
    let (snapshot, token) = scans.begin("workspace", false);
    let app = tauri::test::mock_builder()
        .manage(scans.clone())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
    app.listen(WORKTREE_DISK_SCAN_PROGRESS_EVENT, move |event| {
        event_tx
            .send(serde_json::from_str::<DiskScanSnapshot>(event.payload()).unwrap())
            .unwrap();
    });
    // Exact checkpoints, established before the walk starts. No scheduling luck:
    // the walker cannot proceed past its first file until cancellation is set.
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (resume_tx, resume_rx) = std::sync::mpsc::channel();
    let path = dir.path().to_path_buf();
    let worker = tokio::spawn(run_blocking(move || {
        let mut entered_tx = Some(entered_tx);
        scan_path(&path, &[], &token.unwrap(), |progress| {
            if progress.files == 1 {
                if let Some(tx) = entered_tx.take() {
                    tx.send(()).unwrap();
                    resume_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                }
            }
            Ok(())
        })
    }));
    tokio::time::timeout(Duration::from_secs(10), entered_rx)
        .await
        .unwrap()
        .unwrap();
    assert!(cmd_worktree_disk_scan_cancel(
        app.handle().clone(),
        app.state(),
        "workspace".into(),
        snapshot.scan_id.clone()
    )
    .await
    .unwrap());
    let event = tokio::time::timeout(Duration::from_secs(10), event_rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(event.status, DiskScanStatus::Cancelled);
    resume_tx.send(()).unwrap();
    let error = tokio::time::timeout(Duration::from_secs(10), worker)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert_eq!(error.code, IpcErrorCode::ScanCancelled);
    assert!(scans
        .finish("workspace", &snapshot.scan_id, Err(error))
        .is_none());
    assert!(scans.result("workspace").unwrap().rows.is_empty());
    let (next, token) = scans.begin("workspace", false);
    assert_ne!(next.scan_id, snapshot.scan_id);
    assert!(token.is_some());
    run_blocking(move || {
        drop(dir);
        Ok(())
    })
    .await
    .unwrap();
}

#[test]
fn disk_scan_cancelled_and_superseded_workers_cannot_publish_partial_cache() {
    let scans = WorktreeDiskScans::default();
    let (first, _) = scans.begin("workspace", false);
    assert!(scans.cancel("workspace", &first.scan_id).is_some());
    scans.finish("workspace", &first.scan_id, Ok(Vec::new()));
    assert_eq!(
        scans.result("workspace").unwrap().status,
        DiskScanStatus::Cancelled
    );
    let (second, _) = scans.begin("workspace", false);
    assert_ne!(first.scan_id, second.scan_id);
    let (third, _) = scans.begin("workspace", true);
    scans.finish("workspace", &second.scan_id, Ok(Vec::new()));
    assert_eq!(scans.result("workspace").unwrap().scan_id, third.scan_id);
    assert_eq!(
        scans.result("workspace").unwrap().status,
        DiskScanStatus::Running
    );
    scans.finish("workspace", &third.scan_id, Ok(Vec::new()));
    let (refresh, _) = scans.begin("workspace", true);
    assert!(scans.cancel("workspace", &refresh.scan_id).is_some());
    let (cached, worker) = scans.begin("workspace", false);
    assert_eq!(cached.scan_id, third.scan_id);
    assert_eq!(cached.status, DiskScanStatus::Completed);
    assert!(worker.is_none());
}
