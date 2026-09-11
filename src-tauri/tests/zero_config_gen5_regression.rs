//! Positive Gen5 acceptance regressions for the Gen4 B1/B2/B3 findings.
//! Production relay, reverse client, gateway, daemon request handler and CLI are
//! used. Only IPC handshake metadata is synthetic to avoid upgrade side effects.
//! All identities, stores and sockets are isolated; network traffic is loopback.
//! Environment mutations are serialized within this test binary.
#![cfg(unix)]

use std::{
    ffi::OsString,
    net::SocketAddr,
    os::unix::fs::PermissionsExt,
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use ferryx_lib::{
    daemon::{
        protocol::{DaemonRequest, DaemonResponse, DAEMON_PROTOCOL_VERSION},
        server::DaemonServer,
    },
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
static ENV_LOCK: Mutex<()> = Mutex::new(());

struct IsolatedEnv {
    dir: tempfile::TempDir,
    saved: Vec<(&'static str, Option<OsString>)>,
    _guard: MutexGuard<'static, ()>,
}

impl IsolatedEnv {
    fn new() -> Self {
        let guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let dir = tempfile::tempdir_in(".").unwrap();
        let root = dir.path().canonicalize().unwrap();
        let saved = [
            "FERRYX_DATA_DIR",
            "FERRYX_RUNTIME_DIR",
            "FERRYX_MACHINE_TOKEN",
            "FERRYX_RELAY_URL",
        ]
        .into_iter()
        .map(|key| (key, std::env::var_os(key)))
        .collect();
        std::fs::create_dir_all(root.join("remote")).unwrap();
        std::fs::create_dir_all(root.join("runtime")).unwrap();
        std::fs::set_permissions(root.join("runtime"), std::fs::Permissions::from_mode(0o700))
            .unwrap();
        std::env::set_var("FERRYX_DATA_DIR", &root);
        std::env::set_var("FERRYX_RUNTIME_DIR", root.join("runtime"));
        std::env::remove_var("FERRYX_MACHINE_TOKEN");
        std::env::remove_var("FERRYX_RELAY_URL");
        Self { dir, saved, _guard: guard }
    }
}

impl Drop for IsolatedEnv {
    fn drop(&mut self) {
        for (key, value) in &self.saved {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

struct Running(tokio::task::JoinHandle<()>);
impl Drop for Running {
    fn drop(&mut self) { self.0.abort(); }
}

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
        let relay_state = RelayState::new_with_key_store(vec![], env.dir.path().join("keys.json"))
            .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let relay = Running(tokio::spawn(async move {
            axum::serve(
                listener,
                relay_router(relay_state).into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        }));
        let remote = env.dir.path().join("remote");
        let daemon = Arc::new(DaemonServer::new_with_paths(
            Some(remote.join("remote-config.json")),
            Some(remote.join("remote-auth.json")),
        ));
        *daemon.remote_state().config.write() = RemoteGatewayConfig {
            mode: RemoteNetworkMode::Relay,
            port: 0,
            relay_url: Some(base.clone()),
            ..Default::default()
        };
        let (handle, _) = start_remote_server(Arc::clone(daemon.remote_state())).await.unwrap();
        daemon.remote_state().persist_config().unwrap();
        Self { daemon, base, handle: Some(handle), _relay: relay, env }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() { handle.stop(); }
    }
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
        .body(serde_json::json!({"pin": pin, "deviceName": "isolated Gen5 regression"}).to_string())
        .send().await.unwrap()
}

async fn assert_issued_permission(f: &Fixture, code: &str, permission: DevicePermission) {
    let response = exchange(&f.base, code).await;
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
    assert_eq!(body["device"]["permission"], serde_json::to_value(permission).unwrap());
    let token = body["token"].as_str().expect("real gateway must issue a bearer");
    let device = f.daemon.remote_state().auth_manager.validate_token(token).unwrap();
    assert_eq!(device.permission, permission, "validate the actual bearer, not just response metadata");
    assert_ne!(exchange(&f.base, code).await.status(), reqwest::StatusCode::OK,
        "the PIN must remain single-use");
}

fn pair_cli() -> tokio::process::Command {
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_ferryx"));
    command.args(["pair", "--generate-pin"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    command
}

// The CLI performs its real handshake and pairing call; only version metadata is
// supplied here. Forward the pairing request to the production daemon handler.
async fn serve_one_cli_request(f: &Fixture) -> (Running, tokio::sync::oneshot::Receiver<DaemonResponse>) {
    let socket = f.env.dir.path().join("runtime/daemon.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let daemon = Arc::clone(&f.daemon);
    let (observed_tx, observed_rx) = tokio::sync::oneshot::channel();
    let ipc = Running(tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let (read, mut write) = stream.into_split();
        let mut reader = BufReader::new(read);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        assert!(matches!(serde_json::from_str::<DaemonRequest>(line.trim()).unwrap(), DaemonRequest::Handshake { .. }));
        let handshake = DaemonResponse::HandshakeOk {
            version: DAEMON_PROTOCOL_VERSION,
            pid: std::process::id(),
            epoch: 1,
            binary_path: None,
            binary_mtime_ms: None,
            daemon_version: Some(env!("CARGO_PKG_VERSION").into()),
        };
        write.write_all(format!("{}\n", serde_json::to_string(&handshake).unwrap()).as_bytes()).await.unwrap();
        line.clear();
        reader.read_line(&mut line).await.unwrap();
        let req: DaemonRequest = serde_json::from_str(line.trim()).unwrap();
        assert!(matches!(req, DaemonRequest::RemoteCreatePairingCode { .. }));
        let response = request(&daemon, req).await;
        write.write_all(format!("{}\n", serde_json::to_string(&response).unwrap()).as_bytes()).await.unwrap();
        let _ = observed_tx.send(response);
    }));
    (ipc, observed_rx)
}

#[tokio::test]
async fn daemon_view_and_control_survive_real_relay_gateway_exchange() {
    for permission in [DevicePermission::View, DevicePermission::Control] {
        let f = Fixture::new().await;
        let code = pin(&f.daemon, permission).await;
        assert_issued_permission(&f, &code, permission).await;
    }
}

#[tokio::test]
async fn cli_refusal_exits_without_replacing_owner_and_original_pin_redeems() {
    let f = Fixture::new().await;
    let original_pin = pin(&f.daemon, DevicePermission::Control).await;
    let (_ipc, observed) = serve_one_cli_request(&f).await;
    let child = pair_cli().env("FERRYX_RELAY_URL", &f.base).spawn().unwrap();
    let response = timeout(LIMIT, observed).await.unwrap().unwrap();
    let DaemonResponse::Error { message } = response else { panic!("expected refusal, got {response:?}"); };
    assert!(message.contains("Ready -> Registering"), "unexpected refusal: {message}");
    let output = timeout(LIMIT, child.wait_with_output()).await.unwrap().unwrap();
    assert!(!output.status.success(), "a real refusal must exit unsuccessfully");
    assert!(output.stdout.is_empty(), "a refused request must not print a standalone PIN");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("daemon refused this pairing request"), "{stderr}");
    assert!(stderr.contains("owns this machine's relay identity"), "{stderr}");
    assert!(!stderr.contains("No running daemon answered"), "{stderr}");
    // This positive exchange, after the child has exited, proves that the
    // daemon's prior owner generation and its already-issued PIN still work.
    assert_issued_permission(&f, &original_pin, DevicePermission::Control).await;
}

#[tokio::test]
async fn cli_success_uses_daemon_pin_and_leaves_it_redeemable_after_exit() {
    let f = Fixture::new().await;
    let (_ipc, observed) = serve_one_cli_request(&f).await;
    let child = pair_cli().env("FERRYX_RELAY_URL", &f.base).spawn().unwrap();
    let response = timeout(LIMIT, observed).await.unwrap().unwrap();
    let DaemonResponse::RemotePairingCodeOk { code, .. } = response else { panic!("expected PIN, got {response:?}"); };
    let output = timeout(LIMIT, child.wait_with_output()).await.unwrap().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let stdout_str = String::from_utf8(output.stdout).unwrap();
    assert!(stdout_str.lines().any(|line| line.trim() == code));
    assert!(stdout_str.contains("#pair="));
    assert!(String::from_utf8(output.stderr).unwrap().contains("Pairing registered by the running daemon"));
    assert_issued_permission(&f, &code, DevicePermission::Control).await;
}

#[tokio::test]
async fn no_daemon_socket_fails_missing_relay_url_without_spawning_daemon() {
    let env = IsolatedEnv::new();
    let socket = env.dir.path().join("runtime/daemon.sock");
    assert!(!socket.exists());
    let child = pair_cli().env("FERRYX_RELAY_URL", "").spawn().unwrap();
    let output = timeout(LIMIT, child.wait_with_output()).await.unwrap().unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("Pairing requires a configured relay URL"), "{stderr}");
    assert!(!stderr.contains("Pairing registered by the running daemon"), "{stderr}");
    assert!(!socket.exists(), "CLI must not create a daemon socket on this path");
    assert_eq!(std::fs::read_dir(env.dir.path().join("runtime")).unwrap().count(), 0,
        "no daemon runtime artifacts may be created on the no-socket path");
}

#[tokio::test]
async fn stopped_relay_then_off_allows_real_daemon_local_view_pairing() {
    let mut f = Fixture::new().await;
    let _live_pin = pin(&f.daemon, DevicePermission::Control).await;
    f.handle.take().unwrap().stop();
    assert!(f.daemon.remote_state().relay_pairing.read().is_none());
    f.daemon.handle_remote_configure(RemoteGatewayConfig::default()).await.unwrap();
    let local_pin = pin(&f.daemon, DevicePermission::View).await;
    let (token, device) = f.daemon.remote_state().auth_manager
        .exchange_pairing_code(&local_pin, "local after stop").unwrap();
    assert_eq!(device.permission, DevicePermission::View);
    assert_eq!(f.daemon.remote_state().auth_manager.validate_token(&token).unwrap().permission,
        DevicePermission::View);
}

#[tokio::test]
async fn older_real_handle_stop_preserves_newer_publication_and_live_pin() {
    let mut f = Fixture::new().await;
    let _old_pin = pin(&f.daemon, DevicePermission::Control).await;
    let old_epoch = f.daemon.remote_state().relay_pairing.read().as_ref().unwrap().epoch;
    // Port zero allows two real handles without using the workstation gateway.
    let (new_handle, _) = start_remote_server(Arc::clone(f.daemon.remote_state())).await.unwrap();
    let old_handle = f.handle.replace(new_handle).unwrap();
    let new_epoch = f.daemon.remote_state().relay_pairing.read().as_ref().unwrap().epoch;
    assert_ne!(old_epoch, new_epoch);
    old_handle.stop();
    // Stop the old owner before asking the new one to register, matching the
    // production handover order and avoiding an intentional reconnect race.
    let new_pin = pin(&f.daemon, DevicePermission::View).await;
    assert_eq!(f.daemon.remote_state().relay_pairing.read().as_ref().unwrap().epoch, new_epoch,
        "actual old handle cleanup must not clear a newer owner's publication");
    assert_issued_permission(&f, &new_pin, DevicePermission::View).await;
    f.handle.take().unwrap().stop();
    assert!(f.daemon.remote_state().relay_pairing.read().is_none(),
        "actual owning handle must remove its own publication");
}
