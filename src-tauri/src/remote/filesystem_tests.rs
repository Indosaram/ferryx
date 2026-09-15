use super::*;
use futures_util::FutureExt;
use std::{sync::Arc, time::Duration};

#[test]
fn a06_budget_inspected_children_exact_cutoff() {
    use std::sync::atomic::AtomicBool;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().to_owned();
    let outcome = std::panic::catch_unwind(|| {
        // Files consume inspection budget but never retained-entry/JSON budget.
        // All children are equivalent: no dependence on native iterator order.
        for index in 0..9_999 {
            std::fs::write(path.join(format!("file-{index:05}")), b"").unwrap();
        }
        for count in [9_999, 10_000, 10_001] {
            if count > 9_999 {
                std::fs::write(path.join(format!("file-{:05}", count - 1)), b"").unwrap();
            }
            assert_eq!(std::fs::read_dir(&path).unwrap().count(), count);
            let listing = filesystem::scan_with_elapsed(
                "", false, &path, &AtomicBool::new(false), || Duration::ZERO, || {},
            ).unwrap();
            assert!(listing.entries.is_empty(), "regular files must not be retained");
            assert_eq!(listing.truncated, count > 10_000,
                "inspection cutoff at native child count {count}; elapsed=0, retained=0");
            println!("A06 inspected: native_children={count} retained=0 elapsed=0 truncated={}", listing.truncated);
        }
    });
    root.close().unwrap();
    assert!(!path.exists());
    println!("A06 inspected cleanup: private root removed; no listener or worker created");
    if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
}

#[tokio::test]
async fn a06_budget_per_device_burst_and_refill() {
    async fn rate_limited(limits: &filesystem::BrowseLimits, device: &str, now: std::time::Instant) {
        let response = limits.acquire_at(device, now).expect_err("exhausted bucket must reject admission");
        assert_eq!(response.status(), 429);
        assert_eq!(response.headers()["cache-control"], "no-store");
        let bytes = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
        let error: machine_protocol::ErrorEnvelope = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(error.error.code, "RATE_LIMITED", "not concurrent-slot exhaustion");
    }
    let limits = filesystem::BrowseLimits::default();
    let start = std::time::Instant::now();
    for device in ["device-a", "device-b"] {
        for admission in 1..=20 {
            // Releasing each permit isolates token accounting from the four slots.
            drop(limits.acquire_at(device, start).unwrap_or_else(|_| panic!("{device} burst admission {admission} rejected")));
        }
        rate_limited(&limits, device, start).await;
        println!("A06 rate: {device} admitted=20 same-instant next=429/RATE_LIMITED/no-store");
    }
    // Just below and exactly at one-token refill, using integer nanoseconds.
    rate_limited(&limits, "device-a", start + Duration::from_nanos(99_999_999)).await;
    let one_token = start + Duration::from_millis(100);
    drop(limits.acquire_at("device-a", one_token).expect("100ms must refill one token"));
    rate_limited(&limits, "device-a", one_token).await;
    println!("A06 rate: 99,999,999ns rejected; 100ms admitted exactly one");
    let one_second = one_token + Duration::from_secs(1);
    for admission in 1..=10 {
        drop(limits.acquire_at("device-a", one_second).unwrap_or_else(|_| panic!("one-second refill admission {admission} rejected")));
    }
    rate_limited(&limits, "device-a", one_second).await;
    println!("A06 rate: one-second refill admitted=10 next=429/RATE_LIMITED");
    let saturated = one_second + Duration::from_secs(3);
    for admission in 1..=20 {
        drop(limits.acquire_at("device-a", saturated).unwrap_or_else(|_| panic!("saturated refill admission {admission} rejected")));
    }
    rate_limited(&limits, "device-a", saturated).await;
    println!("A06 rate: three-second refill capped at 20; permits released; private limiter dropped; no roots/listeners");
}

#[tokio::test]
async fn r6_auth_routed_deadline_admission() { auth_boundary_fixture(false, false).await; }
#[tokio::test]
async fn r6_auth_routed_revocation() { auth_boundary_fixture(true, false).await; }
#[tokio::test]
async fn r6_auth_routed_failure_cleanup() { auth_boundary_fixture(true, true).await; }

async fn auth_boundary_fixture(revoke: bool, inject: bool) {
    let root = tempfile::tempdir().unwrap();
    let root_path = root.path().to_owned();
    let daemon = crate::daemon::server::DaemonServer::new_with_paths(
        Some(root.path().join("config.json")), Some(root.path().join("auth.json")));
    let state = daemon.remote_state().clone();
    *state.browse_home.write() = Some(root.path().to_owned());
    *state.browse_probe.write() = Some(Arc::new(|| panic!("expired/revoked auth must not scan")));
    let pin = state.auth_manager.create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine).unwrap();
    let (token, device) = state.auth_manager.exchange_pairing_code(&pin, "auth-boundary").unwrap();
    let limits = Arc::new(filesystem::BrowseLimits::default());
    let (entry_tx, mut entry_rx) = tokio::sync::mpsc::unbounded_channel();
    let entries = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let worker_entries = entries.clone();
    let (drop_tx, mut drop_rx) = tokio::sync::mpsc::unbounded_channel();
    *limits.1.lock() = Some(drop_tx);
    let (done_tx, mut done_rx) = tokio::sync::mpsc::unbounded_channel();
    *limits.4.lock() = Some(done_tx);
    let gate = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
    let worker_gate = gate.clone();
    *limits.3.lock() = Some(Arc::new(move || {
        worker_entries.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        entry_tx.send(()).unwrap();
        let (lock, signal) = &*worker_gate;
        let released = lock.lock().unwrap();
        let (_released, timeout) = signal.wait_timeout_while(released, Duration::from_secs(30), |released| !*released).unwrap();
        assert!(!timeout.timed_out(), "fixture release missing");
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    // Same production handler/extractors over real TCP, with a private limiter.
    let router = axum::Router::new()
        .route("/api/v1/fs/directories", axum::routing::get(filesystem::directories))
        .layer(axum::Extension(limits.clone())).with_state(state.clone())
        .fallback_service(create_remote_router(state.clone()));
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        axum::serve(listener, router).with_graceful_shutdown(async { let _ = stop_rx.await; }).await.unwrap();
    });
    let count = if revoke { 1 } else { 16 };
    let mut requests = Vec::new();
    let mut completed = 0;
    let outcome = std::panic::AssertUnwindSafe(async {
        let client = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(20)).build().unwrap();
        let url = format!("http://{addr}/api/v1/fs/directories?path=relative");
        for _ in 0..count {
            let request = client.get(&url).bearer_auth(&token);
            requests.push(tokio::spawn(async move { request.send().await.unwrap() }));
        }
        for _ in 0..count { tokio::time::timeout(Duration::from_secs(5), entry_rx.recv()).await.unwrap().unwrap(); }
        let health = tokio::time::timeout(Duration::from_secs(2), client.get(format!("http://{addr}/api/v1/health")).send()).await.unwrap().unwrap();
        assert_eq!(health.status(), 200);
        println!("R6 auth: {count} real routed workers entered; unrelated health=200");
        assert!(!inject, "injected auth fixture failure");
        if revoke {
            let auth = state.auth_manager.clone();
            crate::ipc::run_blocking(move || { assert!(auth.revoke_device(&device.id)); Ok(()) }).await.unwrap();
            *gate.0.lock().unwrap() = true; gate.1.notify_all();
        } else {
            let response = client.get(&url).bearer_auth(&token).send().await.unwrap();
            assert_eq!(response.status(), 429, "auth admission must reject before starting worker 17");
            assert_eq!(response.headers()["cache-control"], "no-store");
        }
        for request in &mut requests {
            let response = tokio::time::timeout(Duration::from_secs(12), request).await.unwrap().unwrap();
            assert_eq!(response.status(), if revoke { 401 } else { 504 });
            assert_eq!(response.headers()["cache-control"], "no-store");
            let body: machine_protocol::ErrorEnvelope = serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
            assert_eq!(body.error.code, if revoke { "UNAUTHORIZED" } else { "TIMEOUT" });
        }
        if !revoke {
            assert_eq!(client.get(&url).bearer_auth(&token).send().await.unwrap().status(), 429, "timed out workers retain admission");
        }
        println!("R6 auth: expected {}; deadline/retained admission or revoke-before-release verified", if revoke {401} else {504});
    }).catch_unwind().await;
    for request in &mut requests {
        if !request.is_finished() { request.abort(); let _ = request.await; }
    }
    if inject {
        let dropped = tokio::time::timeout(Duration::from_secs(5), drop_rx.recv()).await.unwrap().unwrap();
        assert_eq!(dropped, ("request_dropped", true));
        assert_eq!(limits.auth_slots().available_permits(), 15);
        println!("R6 auth injected disconnect: cancellation observed; blocked auth permit retained");
    }
    *gate.0.lock().unwrap() = true; gate.1.notify_all();
    while completed < count {
        tokio::time::timeout(Duration::from_secs(5), done_rx.recv()).await.unwrap().unwrap();
        completed += 1;
    }
    // Keep the entry observer installed until the listener and all scheduled
    // workers finish; count is only the initial expected cohort, not teardown.
    let pin = state.auth_manager.create_pairing_code(DevicePermission::Control);
    let mirror = state.auth_manager.exchange_pairing_code(&pin, "mirror").unwrap().0;
    let client = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(5)).build().unwrap();
    for (credential, expected) in [("invalid", 401), (mirror.as_str(), 403)] {
        let response = client.get(format!("http://{addr}/api/v1/fs/directories?path=relative"))
            .bearer_auth(credential).send().await.unwrap();
        assert_eq!(response.status().as_u16(), expected);
        assert_eq!(response.headers()["cache-control"], "no-store");
    }
    println!("R6 auth recovered admission: invalid=401 mirror=403 before malformed path; no-store");
    stop_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), server).await.unwrap().unwrap();
    assert!(tokio::net::TcpStream::connect(addr).await.is_err());
    limits.4.lock().take();
    tokio::time::timeout(Duration::from_secs(5), async {
        while done_rx.recv().await.is_some() { completed += 1; }
    }).await.expect("every scheduled auth worker drops its completion sender");
    assert_eq!(completed, entries.load(std::sync::atomic::Ordering::SeqCst));
    let recovered_slots = limits.auth_slots().available_permits();
    *limits.3.lock() = None;
    println!("R6 auth exact drain: entered={completed} completed={completed}; completion channel closed before root removal");
    drop(state); drop(daemon); root.close().unwrap();
    assert!(!root_path.exists());
    println!("R6 auth cleanup: workers={completed} completed; listener joined/refused; root removed; injected={inject}");
    assert_eq!(recovered_slots, 16);
    if inject { assert!(outcome.is_err()); } else if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
}

#[tokio::test]
async fn a06_directory_http_home() {
    directory_http_fixture(false).await;
}

#[tokio::test]
async fn a06_http_send_failure_still_tears_down() {
    directory_http_fixture(true).await;
}

async fn directory_http_fixture(inject_send_failure: bool) {
    let (fixture, daemon) = crate::ipc::run_blocking(|| {
        let fixture = tempfile::tempdir().unwrap();
        let home = fixture.path().join("home");
        std::fs::create_dir(&home).unwrap();
        for name in [
            "space dir",
            if cfg!(windows) { "quote'" } else { "quote'\"" },
            "日本語",
            ".hidden",
            "large",
            "denied",
        ] {
            std::fs::create_dir(home.join(name)).unwrap();
        }
        std::fs::create_dir(fixture.path().join("outside")).unwrap();
        std::fs::write(home.join("file"), b"unchanged").unwrap();
        for i in 0..1005 {
            std::fs::create_dir(home.join("large").join(format!("d{i:04}"))).unwrap();
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::{symlink, PermissionsExt};
            symlink(&home, home.join("alias")).unwrap();
            std::fs::create_dir(home.join("back\\slash")).unwrap();
            #[cfg(target_os = "linux")]
            {
                use std::os::unix::ffi::OsStringExt;
                std::fs::create_dir(home.join(std::ffi::OsString::from_vec(vec![255]))).unwrap();
            }
            std::fs::set_permissions(home.join("denied"), std::fs::Permissions::from_mode(0))
                .unwrap();
        }
        let daemon = crate::daemon::server::DaemonServer::new_with_paths(
            Some(fixture.path().join("config.json")),
            Some(fixture.path().join("auth.json")),
        );
        *daemon.remote_state().browse_home.write() = Some(home);
        Ok((fixture, daemon))
    })
    .await
    .unwrap();
    let state = Arc::clone(daemon.remote_state());
    let auth = Arc::clone(&state.auth_manager);
    let (token, device, mirror) = crate::ipc::run_blocking(move || {
        let pin = auth
            .create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine)
            .unwrap();
        let (token, device) = auth.exchange_pairing_code(&pin, "a06").unwrap();
        let pin = auth.create_pairing_code(DevicePermission::Control);
        Ok((
            token,
            device,
            auth.exchange_pairing_code(&pin, "mirror").unwrap().0,
        ))
    })
    .await
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let request_state = state.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, create_remote_router(state))
            .with_graceful_shutdown(async {
                rx.await.unwrap();
            })
            .await
            .unwrap();
    });
    // Defer any assertion/transport panic until owned resources are torn down.
    let outcome = std::panic::AssertUnwindSafe(async {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap();
        let url = format!("http://{addr}/api/v1/fs/directories");
        if inject_send_failure {
            // Invalid header fails reqwest send deterministically, without a
            // connection race or depending on an unused port staying unused.
            client
                .get(&url)
                .header("x-fixture", "\n")
                .send()
                .await
                .unwrap();
        }
        let response = client.get(&url).bearer_auth(&token).send().await.unwrap();
        let status = response.status();
        assert_eq!(status, 200);
        assert_eq!(response.headers()["cache-control"], "no-store");
        let listing: machine_protocol::Directories =
            serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
        assert_eq!(listing.path, listing.home_path);
        #[cfg(windows)]
        {
            for rejected in [r"\\server\share", r"\\?\UNC\server\share", r"\\.\C:\", r"\\?\C:\"] {
                let response = client.get(&url).bearer_auth(&token).query(&[("path", rejected)]).send().await.unwrap();
                assert_eq!(response.status(), 422, "network/device input must remain unsupported");
            }
            let drive = &listing.home_path[..3];
            let response = client.get(&url).bearer_auth(&token).query(&[("path", drive)]).send().await.unwrap();
            assert_eq!(response.status(), 200);
            let root: machine_protocol::Directories = serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
            assert!(root.parent_path.is_none());
            assert!(std::fs::create_dir(std::path::Path::new(&listing.home_path).join("invalid\"name")).is_err());
        }
        assert!(!listing.entries.iter().any(|e| e.name == "file" || e.hidden));
        for name in ["space dir", if cfg!(windows) { "quote'" } else { "quote'\"" }, "日本語"] {
            assert!(listing.entries.iter().any(|e| e.name == name));
        }
        #[cfg(unix)]
        {
            assert!(listing.truncated);
            assert!(listing
                .entries
                .iter()
                .any(|e| e.name == "alias" && e.path == listing.home_path));
        }
        for (path, hidden) in [
            (listing.parent_path.clone().unwrap(), false),
            (
                format!("{}/outside", listing.parent_path.as_ref().unwrap()),
                false,
            ),
            ("~".into(), true),
            ("~/space dir".into(), false),
        ] {
            let response = client
                .get(&url)
                .bearer_auth(&token)
                .query(&[
                    ("path", path.as_str()),
                    ("includeHidden", if hidden { "true" } else { "false" }),
                ])
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            let result: machine_protocol::Directories =
                serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
            if hidden {
                assert!(result.entries.iter().any(|e| e.name == ".hidden"));
            }
            println!(
                "HTTP native browse: hidden={hidden}, entries={}, truncated={}",
                result.entries.len(),
                result.truncated
            );
        }
        let response = client
            .get(&url)
            .bearer_auth(&token)
            .query(&[("path", "~/large")])
            .send()
            .await
            .unwrap();
        let bytes = response.bytes().await.unwrap();
        assert!(bytes.len() <= 256 * 1024);
        let large: machine_protocol::Directories = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(large.entries.len(), 1000);
        assert!(large.truncated);
        assert!(large
            .entries
            .windows(2)
            .all(|pair| pair[0].name < pair[1].name));
        for (query, expected) in [
            ("path=relative", 400),
            ("path=%00", 400),
            ("path=%GG", 400),
            ("path=%FF", 400),
            ("includeHidden=1", 400),
            ("path=~&path=~", 400),
            ("path=~/missing", 404),
            ("path=~/file", 404),
            ("path=https%3A%2F%2Fexample.com", 422),
        ] {
            let response = client
                .get(format!("{url}?{query}"))
                .bearer_auth(&token)
                .send()
                .await
                .unwrap();
            assert_eq!(response.status().as_u16(), expected, "{query}");
            assert_eq!(response.headers()["cache-control"], "no-store");
        }
        #[cfg(unix)]
        assert_eq!(
            client
                .get(&url)
                .bearer_auth(&token)
                .query(&[("path", "~/denied")])
                .send()
                .await
                .unwrap()
                .status(),
            403
        );
        for (credential, expected) in [("invalid", 401), (mirror.as_str(), 403)] {
            assert_eq!(
                client
                    .get(&url)
                    .bearer_auth(credential)
                    .send()
                    .await
                    .unwrap()
                    .status()
                    .as_u16(),
                expected
            );
        }
        let response = client
            .get(format!("http://{addr}/api/v1/capabilities"))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap();
        let capabilities: serde_json::Value =
            serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
        assert_eq!(
            capabilities["capabilities"],
            serde_json::json!(["directoryBrowseV1", "machineWorkspaceV1", "managedWorktreesV1", "terminalCreateV1", "terminalStreamV1"])
        );
        // Subscribe before triggering work; revocation must win while enumeration is blocked.
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let entered_tx = std::sync::Mutex::new(Some(entered_tx));
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let release_rx = std::sync::Mutex::new(release_rx);
        *request_state.browse_probe.write() = Some(Arc::new(move || {
            entered_tx.lock().unwrap().take().unwrap().send(()).unwrap();
            release_rx
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
        }));
        let request = client.get(&url).bearer_auth(&token);
        let pending = tokio::spawn(async move { request.send().await.unwrap() });
        tokio::time::timeout(Duration::from_secs(10), entered_rx)
            .await
            .unwrap()
            .unwrap();
        let auth = request_state.auth_manager.clone();
        crate::ipc::run_blocking(move || {
            assert!(auth.revoke_device(&device.id));
            Ok(())
        })
        .await
        .unwrap();
        let response = tokio::time::timeout(Duration::from_secs(10), pending)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(response.status(), 401);
        release_tx.send(()).unwrap();
        *request_state.browse_probe.write() = None;
        assert_eq!(
            client
                .get(&url)
                .bearer_auth(&token)
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
    })
    .catch_unwind()
    .await;
    *request_state.browse_probe.write() = None;
    let shutdown = tx.send(());
    let mut server = server;
    let joined = tokio::time::timeout(Duration::from_secs(10), &mut server).await;
    if joined.is_err() {
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
    }
    crate::ipc::run_blocking(move || {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                fixture.path().join("home/denied"),
                std::fs::Permissions::from_mode(0o700),
            )
            .unwrap();
        }
        assert_eq!(
            std::fs::read(fixture.path().join("home/file")).unwrap(),
            b"unchanged"
        );
        drop(daemon);
        fixture.close().unwrap();
        Ok(())
    })
    .await
    .unwrap();
    shutdown.expect("signal owned listener");
    joined
        .expect("bounded listener teardown")
        .expect("listener joined");
    if inject_send_failure {
        assert!(
            outcome.is_err(),
            "injected send failure must be observed after teardown"
        );
        println!("A06 injected send failure: listener joined, private fixture removed");
    } else if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

#[tokio::test]
async fn a06_native_bounds_and_query_contract() {
    let limits = filesystem::BrowseLimits::default();
    let permits: Vec<_> = (0..4).map(|_| limits.acquire("fixture").unwrap()).collect();
    assert_eq!(limits.acquire("fixture").unwrap_err().status(), 429);
    drop(permits);
    assert!(limits.acquire("fixture").is_ok());
    for raw in [
        "path=%",
        "path=%0",
        "path=%zz",
        "includeHidden=True",
        "other=true",
        "includeHidden=false&includeHidden=true",
    ] {
        assert!(filesystem::query(Some(raw)).is_err());
    }
    assert_eq!(
        filesystem::query(Some("path=%2520&includeHidden=true")).unwrap(),
        ("%20".into(), true)
    );
    crate::ipc::run_blocking(|| {
        use std::{sync::atomic::AtomicBool, time::Instant};
        let fixture = tempfile::tempdir().unwrap();
        assert!(filesystem::resolve_directory(&"x".repeat(4097), fixture.path()).is_err());
        assert!(filesystem::resolve_directory("~/bad\n", fixture.path()).is_err());
        std::fs::create_dir(fixture.path().join("child")).unwrap();
        let cancelled = AtomicBool::new(true);
        assert!(matches!(
            filesystem::scan("", false, fixture.path(), &cancelled, Instant::now()),
            Err(filesystem::FsError::Timeout)
        ));
        let result = filesystem::scan(
            "",
            false,
            fixture.path(),
            &AtomicBool::new(false),
            Instant::now() - Duration::from_secs(6),
        )
        .unwrap();
        assert!(result.truncated);
        assert!(result.entries.is_empty());
        fixture.close().unwrap();
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn r6_empty_deadline_and_independent_bytes() {
    crate::ipc::run_blocking(|| {
        use std::{sync::atomic::AtomicBool, time::Instant};
        let fixture = tempfile::tempdir().unwrap();
        let outcome = std::panic::catch_unwind(|| {
            let empty = filesystem::scan("", false, fixture.path(), &AtomicBool::new(false),
                Instant::now() - Duration::from_secs(6)).unwrap();
            assert!(empty.truncated, "expired empty scan must not report complete");
            let cancelled = AtomicBool::new(false);
            let result = filesystem::scan_after_resolution("", false, fixture.path(), &cancelled, Instant::now(), || {
                cancelled.store(true, std::sync::atomic::Ordering::Release);
            });
            assert!(matches!(result, Err(filesystem::FsError::Timeout)));
            println!("R6 empty scan: expired result truncated; cancellation after resolution returns TIMEOUT");
            for i in 0..800 {
                std::fs::create_dir(fixture.path().join(format!("{i:04}{}", "x".repeat(220)))).unwrap();
            }
            let listing = filesystem::scan("", false, fixture.path(), &AtomicBool::new(false), Instant::now()).unwrap();
            let bytes = serde_json::to_vec(&listing).unwrap();
            assert!(listing.truncated);
            assert!(!listing.entries.is_empty() && listing.entries.len() < 800);
            assert!(bytes.len() <= 256 * 1024);
            let next = serde_json::to_vec(&listing.entries[0]).unwrap().len() + 1;
            assert!(bytes.len() + next > 256 * 1024, "byte limit, not count or time, stopped scan");
            println!("R6 byte cap: created=800 retained={} json_bytes={} next_bytes={next}", listing.entries.len(), bytes.len());
        });
        fixture.close().unwrap();
        if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
        Ok(())
    }).await.unwrap();
}

#[tokio::test]
async fn r6_http_disconnect_retains_worker_slot() {
    use tokio::io::AsyncWriteExt;
    let fixture = tempfile::tempdir().unwrap();
    let daemon = crate::daemon::server::DaemonServer::new_with_paths(
        Some(fixture.path().join("config.json")), Some(fixture.path().join("auth.json")));
    let state = daemon.remote_state().clone();
    *state.browse_home.write() = Some(fixture.path().to_owned());
    let pin = state.auth_manager.create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine).unwrap();
    let (token, device) = state.auth_manager.exchange_pairing_code(&pin, "r6").unwrap();
    let limits = Arc::new(filesystem::BrowseLimits::default());
    let (events_tx, mut events_rx) = tokio::sync::mpsc::unbounded_channel();
    *limits.1.lock() = Some(events_tx);
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let entered_tx = std::sync::Mutex::new(Some(entered_tx));
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let release_rx = std::sync::Mutex::new(release_rx);
    *state.browse_probe.write() = Some(Arc::new(move || {
        entered_tx.lock().unwrap().take().unwrap().send(()).unwrap();
        // Dropping the sender on an assertion unwind also releases this worker.
        let _ = release_rx.lock().unwrap().recv_timeout(Duration::from_secs(10));
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = axum::Router::new().route("/api/v1/fs/directories", axum::routing::get(filesystem::directories))
        .layer(axum::Extension(limits.clone())).with_state(state.clone());
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        axum::serve(listener, router).with_graceful_shutdown(async { let _ = stop_rx.await; }).await.unwrap();
    });
    let mut worker_entered = false;
    let mut worker_completed = false;
    let outcome = std::panic::AssertUnwindSafe(async {
        let held: Vec<_> = (0..3).map(|_| limits.acquire(&device.id).unwrap()).collect();
        let mut socket = tokio::net::TcpStream::connect(addr).await.unwrap();
        socket.write_all(format!("GET /api/v1/fs/directories HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\n\r\n").as_bytes()).await.unwrap();
        tokio::time::timeout(Duration::from_secs(10), entered_rx).await.unwrap().unwrap();
        worker_entered = true;
        drop(socket);
        let dropped = tokio::time::timeout(Duration::from_secs(10), events_rx.recv()).await.unwrap().unwrap();
        assert_eq!(dropped, ("request_dropped", true));
        assert_eq!(limits.acquire(&device.id).unwrap_err().status(), 429);
        println!("R6 HTTP disconnect: request dropped, cancellation=true, blocked fourth slot retained (429)");
        release_tx.send(()).unwrap();
        let completed = tokio::time::timeout(Duration::from_secs(10), events_rx.recv()).await.unwrap().unwrap();
        worker_completed = completed.0 == "worker_completed";
        assert_eq!(completed, ("worker_completed", true));
        assert!(limits.acquire(&device.id).is_ok());
        drop(held);
        println!("R6 HTTP completion: cancelled scan returned TIMEOUT, slot reusable");
    }).catch_unwind().await;
    drop(release_tx);
    if worker_entered && !worker_completed {
        tokio::time::timeout(Duration::from_secs(10), async {
            while let Some(event) = events_rx.recv().await {
                if event.0 == "worker_completed" { break; }
            }
        }).await.expect("released worker completion before fixture removal");
    }
    *state.browse_probe.write() = None;
    stop_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(10), server).await.unwrap().unwrap();
    assert!(tokio::net::TcpStream::connect(addr).await.is_err());
    drop(state);
    drop(daemon);
    fixture.close().unwrap();
    println!("R6 HTTP cleanup: listener joined, connection refused, private root removed");
    if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
}

#[tokio::test]
async fn a06_cancelled_worker_cannot_publish() {
    let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let guard = filesystem::CancelOnDrop(flag.clone());
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let _guard = guard;
        crate::ipc::run_blocking(move || {
            let fixture = tempfile::tempdir().unwrap();
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            let result =
                filesystem::scan("", false, fixture.path(), &flag, std::time::Instant::now());
            fixture.close().unwrap();
            done_tx
                .send(matches!(result, Err(filesystem::FsError::Timeout)))
                .unwrap();
            Ok(result)
        })
        .await
    });
    tokio::time::timeout(Duration::from_secs(10), entered_rx)
        .await
        .unwrap()
        .unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    release_tx.send(()).unwrap();
    assert!(tokio::time::timeout(Duration::from_secs(10), done_rx)
        .await
        .unwrap()
        .unwrap());
}

#[tokio::test]
async fn a06_service_less_typed_503_without_probe() {
    service_less_fixture(false).await;
}

#[tokio::test]
async fn r6_service_less_injected_failure() {
    service_less_fixture(true).await;
}

async fn service_less_fixture(inject_failure: bool) {
    let state = Arc::new(RemoteGatewayState::new(
        Arc::new(crate::terminal::TerminalService::default()),
        crate::worktree::WorkspaceRegistry::new(),
    ));
    *state.browse_probe.write() = Some(Arc::new(|| panic!("service-less gateway must not probe")));
    let pin = state
        .auth_manager
        .create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine)
        .unwrap();
    let token = state
        .auth_manager
        .exchange_pairing_code(&pin, "fixture")
        .unwrap()
        .0;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        axum::serve(listener, create_remote_router(state))
            .with_graceful_shutdown(async {
                rx.await.unwrap();
            })
            .await
            .unwrap();
    });
    let outcome = std::panic::AssertUnwindSafe(async {
    let response = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap()
        .get(format!("http://{addr}/api/v1/fs/directories"))
        .bearer_auth(token)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 503);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let error: machine_protocol::ErrorEnvelope =
        serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
    assert_eq!(error.error.code, "MACHINE_SERVICE_UNAVAILABLE");
    assert!(!inject_failure, "R6 injected service-less assertion failure");
    }).catch_unwind().await;
    tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(10), server)
        .await
        .unwrap()
        .unwrap();
    assert!(tokio::net::TcpStream::connect(addr).await.is_err());
    println!("R6 service-less cleanup: listener joined, connection refused, injected={inject_failure}");
    if inject_failure {
        assert!(outcome.is_err());
    } else if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}
