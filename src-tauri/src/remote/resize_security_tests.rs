//! V06: real HTTP/WebSocket authorization through the production PTY backend.
//! Close is ordered behind Resize on the same receive loop, not an absence timer.
use super::*;
use futures_util::FutureExt;
use std::panic::AssertUnwindSafe;

async fn close_barrier(socket: &mut tokio::net::TcpStream) {
    write_client_ws_frame(socket, 0x08, &[]).await;
    while !matches!(
        read_server_ws_frame(socket).await,
        ServerWebSocketFrame::Close
    ) {}
}

async fn child_size(service: &TerminalService, id: &str) -> String {
    // Subscribe before triggering the exact child response; no polling or sleeps.
    let (_, mut output) = service.output_hub().subscribe(id).unwrap();
    service.write_input(id, b"measure\n").unwrap();
    let mut bytes = Vec::new();
    loop {
        bytes.extend(output.recv().await.expect("child output"));
        let text = String::from_utf8_lossy(&bytes);
        if let Some(start) = text.find("V06_SIZE_BEGIN") {
            let rest = &text[start + "V06_SIZE_BEGIN".len()..];
            if let Some(end) = rest.find("V06_SIZE_END") {
                return rest[..end].trim().to_string();
            }
        }
    }
}

async fn resize_permissions(grid: bool) {
    let private = tempfile::tempdir().unwrap();
    let pty = Arc::new(crate::terminal::PtyManager::new());
    let hub = Arc::new(crate::terminal::TerminalOutputHub::default());
    let service = Arc::new(TerminalService::new(pty.clone(), hub.clone()));
    let mut command = portable_pty::CommandBuilder::new("/bin/sh");
    command.cwd(private.path());
    command.env("HOME", private.path());
    command.arg("-c");
    command.arg("/bin/stty -echo; printf 'V06_READY\\n'; while IFS= read -r line; do printf 'V06_SIZE_BEGIN\\n'; /bin/stty size; printf 'V06_SIZE_END\\n'; done");
    // Pairing and other fallible preconditions precede PTY allocation.
    let state = Arc::new(RemoteGatewayState::new(
        service.clone(),
        crate::worktree::WorkspaceRegistry::new(),
    ));
    let (view, _) = pair(&state, DevicePermission::View);
    let (control, _) = pair(&state, DevicePermission::Control);
    let private_root = private.path().to_path_buf();
    let mut tasks = tokio::task::JoinSet::new();
    let mut server_owner = None;
    let mut session = None;
    let mut fixture_pid = None;
    let (id, mut output) = pty.spawn(command, 80, 24).unwrap();

    // Every operation after successful spawn is inside the unwind boundary.
    // Owners remain outside so setup panics still reach awaited cleanup.
    let result = AssertUnwindSafe(async {
        tokio::time::timeout(DEADLINE, async {
            session = service.get_session(&id);
            fixture_pid = session.as_ref().and_then(|session| session.pid());
            eprintln!(
                "{}",
                serde_json::json!({
                    "event": "v06_fixture_started", "grid": grid, "session_id": id,
                    "pid": fixture_pid, "private_root": private_root
                })
            );
            hub.register_session(&id);
            hub.record_initial_size(&id, 80, 24);
            let pump_hub = hub.clone();
            let pump_id = id.clone();
            tasks.spawn(async move {
                while let Some(bytes) = output.recv().await {
                    pump_hub
                        .publish(&pump_id, bytes)
                        .expect("registered fixture output");
                }
            });
            state.set_active_selection(RemoteActiveDesktopSelection {
                session_id: Some(id.clone()),
                ..Default::default()
            });
            server_owner = Some(SecurityServer::start(state.clone()).await);
            let server = server_owner.as_ref().unwrap();
            // Actual child response also establishes startup before attaching.
            assert_eq!(child_size(&service, &id).await, "24 80");
            let path = format!("/api/v1/terminal/{id}");
            let view_path = if grid {
                format!("{path}?render=grid&cols=47&rows=18")
            } else {
                path.clone()
            };
            let mut viewer = open_ws_stream(server.addr, &view_path, Some(&view)).await;
            if grid {
                let initial = read_grid_text_frame(&mut viewer).await;
                assert_eq!(
                    (initial["cols"].as_u64(), initial["rows"].as_u64()),
                    (Some(80), Some(24)),
                    "View attach viewport must not resize the PTY or mirror"
                );
            } else {
                assert!(matches!(
                    read_server_ws_frame(&mut viewer).await,
                    ServerWebSocketFrame::Binary(_)
                ));
            }
            assert_eq!(service.get_session(&id).unwrap().get_size(), (80, 24));
            write_client_ws_frame(&mut viewer, 1, br#"{"type":"resize","cols":51,"rows":17}"#)
                .await;
            close_barrier(&mut viewer).await;
            drop(viewer);
            let view_size = service.get_session(&id).unwrap().get_size();
            let view_child_size = child_size(&service, &id).await;

            // Run the positive control BEFORE the denial assertions, even on RED.
            let control_path = if grid {
                format!("{path}?render=grid")
            } else {
                path
            };
            let mut controller = open_ws_stream(server.addr, &control_path, Some(&control)).await;
            if grid {
                read_grid_text_frame(&mut controller).await;
            } else {
                assert!(matches!(
                    read_server_ws_frame(&mut controller).await,
                    ServerWebSocketFrame::Binary(_)
                ));
            }
            write_client_ws_frame(
                &mut controller,
                1,
                br#"{"type":"resize","cols":63,"rows":19}"#,
            )
            .await;
            if grid {
                // A trailing child-output chunk may also produce a gridDiff.
                // Await the exact resize frame, bounded by the enclosing deadline.
                loop {
                    let resized = read_grid_text_frame(&mut controller).await;
                    if resized["type"] == "grid" && resized["cols"] == 63 && resized["rows"] == 19 {
                        break;
                    }
                }
            }
            close_barrier(&mut controller).await;
            drop(controller);
            assert_eq!(
                service.get_session(&id).unwrap().get_size(),
                (63, 19),
                "Control resize must reach the real backend"
            );
            assert_eq!(
                child_size(&service, &id).await,
                "19 63",
                "Control resize must reach the child PTY"
            );
            assert_eq!(
                state
                    .active_selection
                    .read()
                    .as_ref()
                    .unwrap()
                    .session_id
                    .as_deref(),
                Some(id.as_str())
            );
            assert_eq!(
                view_size,
                (80, 24),
                "V06: View Resize changed the real backend after the ordered close barrier"
            );
            assert_eq!(
                view_child_size, "24 80",
                "V06: View Resize changed child-observed PTY geometry"
            );
        })
        .await
        .expect("bounded V06 socket/child events");
    })
    .catch_unwind()
    .await;

    if let Some(server) = server_owner {
        server.stop().await;
    }
    // Attempt each cleanup before asserting results, so a failed close/join
    // cannot bypass private-root removal or the remaining task teardown.
    let closed = tokio::time::timeout(DEADLINE, pty.close_session(&id)).await;
    let joined = tokio::time::timeout(DEADLINE, async {
        let mut results = Vec::new();
        while let Some(result) = tasks.join_next().await {
            results.push(result);
        }
        results
    })
    .await;
    if joined.is_err() {
        tasks.shutdown().await;
    }
    hub.remove_session(&id);
    let private_closed = private.close();
    eprintln!(
        "{}",
        serde_json::json!({
            "event": "v06_fixture_cleanup", "grid": grid, "session_id": id,
            "pid": fixture_pid, "private_root": private_root,
            "pty_close_ok": matches!(&closed, Ok(Ok(()))),
            "pty_reaped": session.as_ref().is_some_and(|session| session.is_reaped()),
            "session_removed": !pty.has_session(&id),
            "output_pump_joined": joined.as_ref().is_ok_and(|results| results.iter().all(Result::is_ok)),
            "private_root_removed": private_closed.is_ok() && !private_root.exists()
        })
    );
    closed
        .expect("bounded fixture PTY cleanup")
        .expect("reap fixture PTY");
    for result in joined.expect("bounded fixture output pump cleanup") {
        result.expect("fixture output pump completed without panic");
    }
    private_closed.expect("remove fixture private root");
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

#[tokio::test]
async fn v06_view_resize_denied_raw_control_allowed() {
    resize_permissions(false).await;
}

// Intentionally not native-terminal-gated: V04 makes the real Ghostty grid
// available headlessly, so both mandatory authorization paths must execute.
#[tokio::test]
async fn v06_view_resize_denied_grid_control_allowed() {
    resize_permissions(true).await;
}
