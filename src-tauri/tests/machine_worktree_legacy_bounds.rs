use ferryx_lib::{remote::{server::create_remote_router, state::RemoteGatewayState, auth::DevicePermission}, terminal::TerminalService, worktree::WorkspaceRegistry};
use futures_util::FutureExt;
use std::{sync::Arc, time::Duration};

#[tokio::test]
async fn service_less_legacy_body_and_git_are_bounded() {
    service_less_fixture(false).await;
}

#[tokio::test]
async fn followthrough_service_less_readiness_failure_cleanup() {
    service_less_fixture(true).await;
}

async fn service_less_fixture(inject_readiness_failure: bool) {
    let (root, state, token) = tokio::task::spawn_blocking(|| {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo"); std::fs::create_dir(&repo).unwrap();
        ferryx_lib::worktree::run_git(&repo, &["init", "--quiet"]).unwrap();
        ferryx_lib::worktree::run_git(&repo, &["-c", "user.name=A08", "-c", "user.email=a08@example.invalid", "commit", "--allow-empty", "-m", "base"]).unwrap();
        let registry = WorkspaceRegistry::new(); registry.register("legacy", &repo).unwrap();
        let state = Arc::new(RemoteGatewayState::new_with_paths(Arc::new(TerminalService::default()), registry, Some(root.path().join("config")), Some(root.path().join("auth"))));
        let pin = state.auth_manager.create_pairing_code(DevicePermission::Control);
        let token = state.auth_manager.exchange_pairing_code(&pin, "legacy").unwrap().0;
        (root, state, token)
    }).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let auth = state.auth_manager.clone();
    let gateway = tokio::spawn(async move { axum::serve(listener, create_remote_router(state)).with_graceful_shutdown(async { let _ = stopped.await; }).await.unwrap(); });
    let result = std::panic::AssertUnwindSafe(async {
        let client = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(45)).build().unwrap();
        let endpoint = format!("http://{address}/api/v1/workspace/worktrees");
        let oversized = client.post(&endpoint).bearer_auth(&token).body("x".repeat(65537)).send().await.unwrap();
        assert_eq!(oversized.status().as_u16(), 413);
        let created = client.post(&endpoint).bearer_auth(&token).header("content-type", "application/json").body(serde_json::json!({"workspaceId":"legacy","worktree":{"wsId":"legacy","slug":"bounded"},"baseRef":"HEAD"}).to_string()).send().await.unwrap();
        assert_eq!(created.status().as_u16(), 200);
        let row: serde_json::Value = serde_json::from_slice(&created.bytes().await.unwrap()).unwrap();
        assert_eq!(row["worktreeSlug"], "bounded"); assert!(row.get("path").is_none());
        let removed = client.delete(&endpoint).bearer_auth(&token).header("content-type", "application/json").body(serde_json::json!({"workspaceId":"legacy","worktree":{"wsId":"legacy","slug":"bounded"},"deleteBranch":true}).to_string()).send().await.unwrap();
        assert_eq!(removed.status().as_u16(), 204);
        #[cfg(unix)] {
            use tokio::io::{AsyncBufReadExt, AsyncReadExt};
            let ready = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = ready.local_addr().unwrap().port();
            let hook = root.path().join("repo/.git/hooks/post-checkout");
            tokio::task::spawn_blocking(move || {
                use std::os::unix::fs::PermissionsExt;
                std::fs::write(&hook, format!("#!/bin/bash\nexec 3<>/dev/tcp/127.0.0.1/{port}\nprintf '%s %s\\n' \"$$\" \"$PPID\" >&3\nread -r release <&3\n")).unwrap();
                std::fs::set_permissions(hook, std::fs::Permissions::from_mode(0o700)).unwrap();
            }).await.unwrap();
            let call = client.post(&endpoint).bearer_auth(&token).body(serde_json::json!({"workspaceId":"legacy","worktree":{"wsId":"legacy","slug":"revoked-child"}}).to_string());
            let job = tokio::spawn(async move { call.send().await.unwrap().status().as_u16() });
            let device = auth.validate_token(&token).unwrap();
            let mut observed_pids = Vec::new();
            let readiness = std::panic::AssertUnwindSafe(async {
            let (socket, _) = tokio::time::timeout(Duration::from_secs(10), ready.accept()).await.unwrap().unwrap();
            let mut reader = tokio::io::BufReader::new(socket); let mut line = String::new();
            tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut line)).await.unwrap().unwrap();
            let pids: Vec<libc::pid_t> = line.split_whitespace().map(|value| value.parse().unwrap()).collect();
            observed_pids = pids.clone();
            if inject_readiness_failure { std::panic::panic_any("A08_SERVICE_LESS_READY_FAILURE"); }
            (reader, pids)
            }).catch_unwind().await;
            // Revoke and await the request even if accept/read/parse failed.
            // The bounded Git owner then tears down its process group.
            assert!(auth.revoke_device(&device.id));
            let status = job.await;
            if inject_readiness_failure {
                assert_eq!(status.unwrap(), 401);
                let mut child_status = 0;
                assert_eq!(unsafe { libc::waitpid(observed_pids[1], &mut child_status, libc::WNOHANG) }, -1);
                assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(libc::ECHILD));
                let panic = readiness.unwrap_err();
                assert_eq!(panic.downcast_ref::<&str>(), Some(&"A08_SERVICE_LESS_READY_FAILURE"));
                eprintln!("A08 service-less injected readiness failure Git={} hook={} request_joined=true git_reaped=true", observed_pids[1], observed_pids[0]);
                return;
            }
            let (mut reader, pids) = readiness.unwrap();
            let mut byte = [0];
            let eof = tokio::time::timeout(Duration::from_secs(10), reader.read(&mut byte)).await;
            let status = status.unwrap();
            assert_eq!(eof.unwrap().unwrap(), 0);
            assert_eq!(status, 401);
            let mut child_status = 0;
            assert_eq!(unsafe { libc::waitpid(pids[1], &mut child_status, libc::WNOHANG) }, -1);
            assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(libc::ECHILD));
            eprintln!("A08 service-less live Git pid={} hook_pid={} revoke_http={status} hook_socket_eof=true direct_git_reaped=true", pids[1], pids[0]);
        }
    }).catch_unwind().await;
    stop.send(()).unwrap(); gateway.await.unwrap();
    tokio::task::spawn_blocking(move || root.close().unwrap()).await.unwrap();
    eprintln!("A08 service-less legacy listener_joined=true private_root_removed=true");
    result.unwrap();
    eprintln!("A08 service-less legacy oversized=413 explicit_HEAD_create=200 delete=204");
}
