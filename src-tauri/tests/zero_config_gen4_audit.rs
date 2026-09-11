//! Opt-in Gen4 defect observations at 050e8a6, NOT security acceptance tests.
//! A PASS means the named defect is present. Run serially: environment overrides
//! isolate all identities, auth stores, and IPC endpoints inside temporary dirs.
//! Relay/gateway/daemon request handling are real; all network traffic is loopback.
#![cfg(unix)]

use std::{ffi::OsString, net::SocketAddr, os::unix::fs::PermissionsExt, sync::Arc, time::Duration};
use ferryx_lib::{
    daemon::{protocol::{DaemonRequest, DaemonResponse, DAEMON_PROTOCOL_VERSION}, server::DaemonServer},
    remote::{
        auth::DevicePermission,
        relay_server::{relay_router, RelayState},
        server::{start_remote_server, RemoteServerHandle},
        state::{RemoteGatewayConfig, RemoteNetworkMode},
    },
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, UnixListener, UnixStream},
    time::timeout,
};

const LIMIT: Duration = Duration::from_secs(12);

struct IsolatedEnv {
    dir: tempfile::TempDir,
    saved: Vec<(&'static str, Option<OsString>)>,
}
impl IsolatedEnv {
    fn new() -> Self {
        let dir = tempfile::tempdir_in(".").unwrap();
        let root = dir.path().canonicalize().unwrap();
        let saved = ["FERRYX_DATA_DIR", "FERRYX_RUNTIME_DIR", "FERRYX_MACHINE_TOKEN", "FERRYX_RELAY_URL"]
            .into_iter().map(|key| (key, std::env::var_os(key))).collect();
        std::fs::create_dir_all(root.join("remote")).unwrap();
        std::fs::create_dir_all(root.join("runtime")).unwrap();
        std::fs::set_permissions(root.join("runtime"), std::fs::Permissions::from_mode(0o700)).unwrap();
        std::env::set_var("FERRYX_DATA_DIR", &root);
        std::env::set_var("FERRYX_RUNTIME_DIR", root.join("runtime"));
        std::env::remove_var("FERRYX_MACHINE_TOKEN");
        std::env::remove_var("FERRYX_RELAY_URL");
        Self { dir, saved }
    }
}
impl Drop for IsolatedEnv {
    fn drop(&mut self) {
        for (key, value) in &self.saved {
            match value { Some(value) => std::env::set_var(key, value), None => std::env::remove_var(key) }
        }
    }
}
struct Running(tokio::task::JoinHandle<()>);
impl Drop for Running { fn drop(&mut self) { self.0.abort(); } }

struct Fixture {
    daemon: Arc<DaemonServer>,
    base: String,
    handle: Option<RemoteServerHandle>,
    _relay: Running,
    env: IsolatedEnv,
}
impl Fixture {
    async fn new() -> Self {
        let env = IsolatedEnv::new();
        let state = RelayState::new_with_key_store(vec![], env.dir.path().join("keys.json")).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let relay = Running(tokio::spawn(async move {
            axum::serve(listener, relay_router(state).into_make_service_with_connect_info::<SocketAddr>()).await.unwrap();
        }));
        let remote = env.dir.path().join("remote");
        let daemon = Arc::new(DaemonServer::new_with_paths(
            Some(remote.join("remote-config.json")), Some(remote.join("remote-auth.json")),
        ));
        *daemon.remote_state().config.write() = RemoteGatewayConfig {
            mode: RemoteNetworkMode::Relay, port: 0, relay_url: Some(base.clone()), ..Default::default()
        };
        // Same production gateway starter that publishes the daemon's coordinator;
        // an ephemeral port avoids touching the workstation's actual daemon port.
        let (handle, _) = start_remote_server(Arc::clone(daemon.remote_state())).await.unwrap();
        daemon.remote_state().persist_config().unwrap();
        Self { daemon, base, handle: Some(handle), _relay: relay, env }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) { if let Some(handle) = self.handle.take() { handle.stop(); } }
}

async fn request(daemon: &Arc<DaemonServer>, req: DaemonRequest) -> DaemonResponse {
    let (client, stream) = UnixStream::pair().unwrap();
    let server = Arc::clone(daemon);
    let _task = Running(tokio::spawn(async move { server.handle_client(stream).await; }));
    let (read, mut write) = client.into_split();
    let mut wire = serde_json::to_vec(&req).unwrap();
    wire.push(b'\n');
    write.write_all(&wire).await.unwrap();
    let mut line = String::new();
    timeout(LIMIT, BufReader::new(read).read_line(&mut line)).await.unwrap().unwrap();
    serde_json::from_str(line.trim()).unwrap()
}
async fn pin(daemon: &Arc<DaemonServer>, permission: DevicePermission) -> String {
    match request(daemon, DaemonRequest::RemoteCreatePairingCode { permission: Some(permission) }).await {
        DaemonResponse::RemotePairingCodeOk { code, .. } => code,
        other => panic!("expected a pairing code, got {other:?}"),
    }
}
async fn exchange(base: &str, pin: &str) -> reqwest::Response {
    reqwest::Client::builder().no_proxy().timeout(LIMIT).build().unwrap()
        .post(format!("{base}/api/v1/pair/exchange"))
        .header("content-type", "application/json")
        .body(serde_json::json!({"pin": pin, "deviceName": "isolated Gen4 observer"}).to_string())
        .send().await.unwrap()
}

#[tokio::test]
#[ignore = "defect-presence observation; use --ignored --test-threads=1"]
async fn observes_daemon_view_pairing_mints_control_through_real_relay_and_gateway() {
    let f = Fixture::new().await;
    // Positive control: the same actual daemon handler honors View in local mode.
    let coordinator = f.daemon.remote_state().relay_pairing.write().take().unwrap();
    let local = pin(&f.daemon, DevicePermission::View).await;
    let (_, local_device) = f.daemon.remote_state().auth_manager.exchange_pairing_code(&local, "local view control").unwrap();
    assert_eq!(local_device.permission, DevicePermission::View);
    *f.daemon.remote_state().relay_pairing.write() = Some(coordinator);

    let code = pin(&f.daemon, DevicePermission::View).await;
    let response = exchange(&f.base, &code).await;
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
    assert_eq!(body["device"]["permission"], "control", "observation must show real token issuance, not just a synthetic registration ACK");
    let token = body["token"].as_str().unwrap();
    let device = f.daemon.remote_state().auth_manager.validate_token(token).unwrap();
    assert_eq!(device.permission, DevicePermission::Control);
    println!("OBSERVED: real daemon View request -> real relay/gateway exchange -> live Control bearer; local control remains View");
}

#[tokio::test]
#[ignore = "defect-presence observation; use --ignored --test-threads=1"]
async fn observes_stopped_relay_coordinator_breaks_local_daemon_pairing() {
    let mut f = Fixture::new().await;
    f.handle.take().unwrap().stop();
    // The daemon's actual OFF transition does not clear the published coordinator.
    f.daemon.handle_remote_configure(RemoteGatewayConfig::default()).await.unwrap();
    tokio::task::yield_now().await;
    assert!(f.daemon.remote_state().relay_pairing.read().is_some());
    let result = request(&f.daemon, DaemonRequest::RemoteCreatePairingCode { permission: Some(DevicePermission::View) }).await;
    let DaemonResponse::Error { message } = result else { panic!("expected stale-coordinator error, got {result:?}"); };
    assert!(message.contains("channel closed") || message.contains("disconnected"), "unexpected error: {message}");
    // Remove only the stale handle; the same local request now succeeds.
    *f.daemon.remote_state().relay_pairing.write() = None;
    let local = pin(&f.daemon, DevicePermission::View).await;
    let (_, device) = f.daemon.remote_state().auth_manager.exchange_pairing_code(&local, "local after stop").unwrap();
    assert_eq!(device.permission, DevicePermission::View);
    println!("OBSERVED: relay stop -> OFF -> daemon pairing error ({message}); clearing only stale handle restores View pairing");
}

#[tokio::test]
#[ignore = "defect-presence observation; launches isolated CLI; use --ignored --test-threads=1"]
async fn observes_cli_daemon_error_starts_competing_relay_owner() {
    let f = Fixture::new().await;
    let original_pin = pin(&f.daemon, DevicePermission::Control).await;
    let daemon = Arc::clone(&f.daemon);
    let socket = f.env.dir.path().join("runtime/daemon.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let (observed_tx, observed_rx) = tokio::sync::oneshot::channel();
    // Serve a synthetic handshake without binary upgrade side effects, then pass
    // the CLI's real pairing request to the actual DaemonServer handler.
    let _ipc = Running(tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let (read, mut write) = stream.into_split();
        let mut reader = BufReader::new(read);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        assert!(matches!(serde_json::from_str::<DaemonRequest>(line.trim()).unwrap(), DaemonRequest::Handshake { .. }));
        let handshake = DaemonResponse::HandshakeOk {
            version: DAEMON_PROTOCOL_VERSION, pid: std::process::id(), epoch: 1,
            binary_path: None, binary_mtime_ms: None, daemon_version: Some(env!("CARGO_PKG_VERSION").into()),
        };
        write.write_all(format!("{}\n", serde_json::to_string(&handshake).unwrap()).as_bytes()).await.unwrap();
        line.clear();
        reader.read_line(&mut line).await.unwrap();
        let req: DaemonRequest = serde_json::from_str(line.trim()).unwrap();
        assert!(matches!(req, DaemonRequest::RemoteCreatePairingCode { .. }));
        let response = request(&daemon, req).await;
        let DaemonResponse::Error { ref message } = response else { panic!("expected a real daemon refusal, got {response:?}"); };
        assert!(message.contains("Ready -> Registering"), "unexpected daemon refusal: {message}");
        write.write_all(format!("{}\n", serde_json::to_string(&response).unwrap()).as_bytes()).await.unwrap();
        let _ = observed_tx.send(message.clone());
    }));
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_ferryx"))
        .args(["pair", "--generate-pin"])
        .env("FERRYX_RELAY_URL", &f.base)
        .stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped())
        .kill_on_drop(true).spawn().unwrap();
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let error = timeout(LIMIT, observed_rx).await.unwrap().unwrap();
    let mut stdout = BufReader::new(stdout);
    let mut standalone_pin = String::new();
    timeout(LIMIT, stdout.read_line(&mut standalone_pin)).await.unwrap().unwrap();
    assert_eq!(standalone_pin.trim().len(), 6, "CLI must reach standalone registration for this observation");
    let mut stderr = BufReader::new(stderr);
    let mut warning = String::new();
    timeout(LIMIT, stderr.read_line(&mut warning)).await.unwrap().unwrap();
    assert!(warning.contains("No running daemon answered"), "unexpected CLI warning: {warning}");
    assert_eq!(exchange(&f.base, &original_pin).await.status(), reqwest::StatusCode::NOT_FOUND,
        "the CLI's second owner must invalidate the daemon's first PIN");
    child.kill().await.unwrap();
    let _ = child.wait().await;
    println!("OBSERVED: daemon explicitly answered '{error}', CLI started standalone owner and original ready PIN became 404");
}
