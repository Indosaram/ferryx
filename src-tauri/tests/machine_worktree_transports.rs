//! Actual owning Git service through forced relay and DaemonClient UDS.
//! The exact-selected child contains every async task in one owned runtime.
#![cfg(unix)]
use ferryx_lib::{daemon::{client::DaemonClient, protocol::{DaemonRequest, DaemonResponse}, server::DaemonServer}, remote::{auth::{DeviceAccessScope, DevicePermission, MachineIdentity}, relay_client::RelayClient, relay_server::{RelayState, relay_router}, server::create_remote_router}};
use futures_util::FutureExt;
use serde_json::{json, Value};
use std::{path::{Path, PathBuf}, sync::Arc, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const LIMIT: Duration = Duration::from_secs(40);

#[tokio::test]
async fn a08_transports_owned_process_lifecycle() {
    let root = tempfile::Builder::new().prefix("a08-transports-").tempdir_in("/tmp").unwrap();
    let path = root.path().to_owned();
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command.args(["--exact", "a08_transports_private_child", "--nocapture"])
        .env("A08_TRANSPORT_ROOT", &path)
        .env("FERRYX_RUNTIME_DIR", path.join("runtime"))
        .env("FERRYX_DATA_DIR", path.join("data"))
        .env("FERRYX_SESSION_DIR", path.join("sessions"))
        .env("HOME", path.join("home"))
        .env("XDG_CONFIG_HOME", path.join("home/config"))
        .env("XDG_DATA_HOME", path.join("home/data"))
        .current_dir(&path).kill_on_drop(true);
    let mut child = command.spawn().unwrap();
    let pid = child.id().unwrap();
    let status = tokio::time::timeout(Duration::from_secs(180), child.wait()).await;
    let status = match status {
        Ok(status) => status.unwrap(),
        Err(_) => { child.kill().await.unwrap(); child.wait().await.unwrap() }
    };
    root.close().unwrap();
    eprintln!("A08_TRANSPORT_CLEANUP child_pid={pid} reaped=true exit={status} private_root={} absent={}", path.display(), !path.exists());
    assert!(!path.exists());
    assert!(status.success(), "owned transport scenario failed after cleanup");
}

#[test]
fn a08_transports_private_child() {
    let Some(root) = std::env::var_os("A08_TRANSPORT_ROOT") else { return };
    let root = PathBuf::from(root);
    for name in ["runtime", "data", "sessions", "home", "repo", "wrong-root"] {
        std::fs::create_dir_all(root.join(name)).unwrap();
    }
    for name in ["FERRYX_RUNTIME_DIR", "FERRYX_DATA_DIR", "FERRYX_SESSION_DIR", "HOME"] {
        assert!(PathBuf::from(std::env::var_os(name).unwrap()).starts_with(&root));
    }
    eprintln!("A08_TRANSPORT_OWNER pid={} cwd={} runtime={} socket={} config={} auth={}", std::process::id(), std::env::current_dir().unwrap().display(), root.join("runtime").display(), root.join("runtime/daemon.sock").display(), root.join("data/config.json").display(), root.join("data/auth.json").display());
    let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let outcome = runtime.block_on(std::panic::AssertUnwindSafe(scenario(&root)).catch_unwind());
    drop(runtime); // Joins all reverse data, gateway upgrade and owner connection tasks even on panic.
    eprintln!("A08_TRANSPORT_RUNTIME joined=true failed={}", outcome.is_err());
    if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
}

async fn git(path: &Path, args: &[&str]) -> Result<String, String> {
    let path = path.to_owned();
    let args: Vec<String> = args.iter().map(|v| v.to_string()).collect();
    tokio::task::spawn_blocking(move || ferryx_lib::worktree::run_git(path, &args.iter().map(String::as_str).collect::<Vec<_>>()).map_err(|e| e.to_string())).await.unwrap()
}

async fn http(client: &reqwest::Client, method: reqwest::Method, url: &str, token: &str, body: Value) -> (u16, Value) {
    eprintln!("A08_TRANSPORT_REQUEST method={method} url={url} body={body}");
    let response = client.request(method, url).bearer_auth(token).body(body.to_string()).send().await.unwrap();
    let status = response.status().as_u16();
    let bytes = response.bytes().await.unwrap();
    let value = if bytes.is_empty() { Value::Null } else { serde_json::from_slice(&bytes).unwrap() };
    eprintln!("A08_TRANSPORT_RESPONSE status={status} body={value}");
    (status, value)
}

async fn uds(client: &DaemonClient, body: Value) -> Value {
    eprintln!("A08_UDS_REQUEST {body}");
    let request: DaemonRequest = serde_json::from_value(body).unwrap();
    let result = tokio::time::timeout(LIMIT, client.send_request(request)).await.unwrap().unwrap();
    let result = serde_json::to_value(result).unwrap();
    eprintln!("A08_UDS_RESPONSE {result}");
    result
}

async fn scenario(root: &Path) {
    use base64::Engine;
    const GET: reqwest::Method = reqwest::Method::GET;
    const POST: reqwest::Method = reqwest::Method::POST;
    const DELETE: reqwest::Method = reqwest::Method::DELETE;
    let repo = root.join("repo");
    for args in [vec!["init", "--quiet"], vec!["config", "user.name", "A08"], vec!["config", "user.email", "a08@example.invalid"], vec!["commit", "--allow-empty", "-m", "base"]] { git(&repo, &args).await.unwrap(); }
    let head = git(&repo, &["rev-parse", "HEAD"]).await.unwrap().trim().to_owned();
    let config = root.join("data/config.json"); let auth = root.join("data/auth.json");
    let owner = Arc::new(tokio::task::spawn_blocking(move || DaemonServer::new_with_paths(Some(config), Some(auth))).await.unwrap());
    let socket = root.join("runtime/daemon.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(root.join("runtime"), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let uds_owner = owner.clone();
    let uds_task = tokio::spawn(async move {
        let mut clients = tokio::task::JoinSet::new();
        loop { let (stream, _) = listener.accept().await.unwrap(); let owner = uds_owner.clone(); clients.spawn(owner.handle_client(stream)); }
    });
    let gateway_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let gateway_address = gateway_listener.local_addr().unwrap();
    let state = owner.remote_state().clone();
    let gateway = tokio::spawn(async move { axum::serve(gateway_listener, create_remote_router(state)).await.unwrap() });
    let key_path = root.join("data/relay-keys.json");
    let relay_state = tokio::task::spawn_blocking(move || RelayState::new_with_key_store(vec![], key_path).unwrap()).await.unwrap();
    let relay_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let relay_address = relay_listener.local_addr().unwrap();
    let relay = tokio::spawn(async move { axum::serve(relay_listener, relay_router(relay_state).into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap() });
    let key = ed25519_dalek::SigningKey::from_bytes(&[108; 32]);
    let identity = MachineIdentity { machine_id: "a08-transport-owner".into(), display_name: "private A08".into(), public_key: base64::engine::general_purpose::STANDARD.encode(key.verifying_key().to_bytes()), private_key: base64::engine::general_purpose::STANDARD.encode(key.to_bytes()) };
    let relay_url = format!("http://{relay_address}");
    let reverse = RelayClient::with_identity(&relay_url, identity, gateway_address.to_string()).with_auth_manager((*owner.remote_state().auth_manager).clone());
    let coordinator = reverse.pairing_coordinator();
    let control = tokio::spawn(async move { reverse.run().await });
    let pairing = tokio::time::timeout(LIMIT, coordinator.generate_scoped_pairing(Duration::from_secs(60), DevicePermission::Control, DeviceAccessScope::Machine)).await.unwrap().unwrap();
    let client = reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).timeout(LIMIT).build().unwrap();
    let response = client.post(format!("{relay_url}/api/v1/pair/exchange")).header("content-type", "application/json").body(json!({"code":pairing.pairing_token,"deviceName":"a08-transport-device"}).to_string()).send().await.unwrap();
    assert_eq!(response.status().as_u16(), 200);
    let exchange: Value = serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
    let token = exchange["token"].as_str().unwrap();
    // The requesting client receives only this relay origin, never a direct gateway URL.
    let origin = format!("{relay_url}/host/a08-transport-owner/api/v1");
    let (status, project) = http(&client, POST, &format!("{origin}/workspace/projects"), token, json!({"requestId":uuid::Uuid::new_v4(),"repoPath":repo})).await;
    assert_eq!(status, 201);
    let ws = project["workspaceId"].as_str().unwrap();
    let canonical = tokio::fs::canonicalize(&repo).await.unwrap();
    assert_eq!(project["repoRoot"], canonical.to_str().unwrap());
    let endpoint = format!("{origin}/workspace/worktrees");
    let local = DaemonClient::new_with_socket(socket.clone());
    let handshake = local.send_request(DaemonRequest::Handshake { version: 3 }).await.unwrap();
    assert!(matches!(handshake, DaemonResponse::HandshakeOk {pid, ..} if pid == std::process::id()));
    for (slug, delete_branch) in [("relay-keep", false), ("relay-delete", true)] {
        let request = json!({"requestId":uuid::Uuid::new_v4(),"workspaceId":ws,"worktree":{"wsId":ws,"slug":slug},"baseRef":"HEAD"});
        let (status, row) = http(&client, POST, &endpoint, token, request.clone()).await;
        assert_eq!(status, 201);
        let path = PathBuf::from(row["path"].as_str().unwrap());
        let expected_root = if std::env::var_os("A08_TRANSPORT_MUTATION").is_some() { root.join("wrong-root") } else { canonical.clone() };
        assert!(path.starts_with(&expected_root), "A08_MUTATION actual owner target must match original Git root: {} versus {}", path.display(), expected_root.display());
        assert_eq!(row["head"], head);
        assert_eq!(git(&path, &["rev-parse", "HEAD"]).await.unwrap().trim(), head);
        assert_eq!(git(&path, &["branch", "--show-current"]).await.unwrap().trim(), format!("orca/{ws}/{slug}"));
        let (status, list) = http(&client, GET, &format!("{endpoint}?workspaceId={ws}"), token, Value::Null).await;
        assert_eq!(status, 200);
        assert_eq!(list["worktrees"].as_array().unwrap().iter().filter(|v| v["path"] == row["path"]).count(), 1);
        let (status, preview) = http(&client, GET, &format!("{endpoint}/status?workspaceId={ws}&wsId={ws}&slug={slug}"), token, Value::Null).await;
        assert_eq!(status, 200); assert_eq!(preview["dirtyCount"], 0);
        let (status, _) = http(&client, DELETE, &endpoint, token, json!({"requestId":uuid::Uuid::new_v4(),"workspaceId":ws,"worktree":{"wsId":ws,"slug":slug},"deleteBranch":delete_branch,"expectedRevision":preview["revision"]})).await;
        assert_eq!(status, 204); assert!(!tokio::fs::try_exists(&path).await.unwrap());
        assert_eq!(git(&repo, &["show-ref", "--verify", &format!("refs/heads/orca/{ws}/{slug}")]).await.is_ok(), !delete_branch);
    }
    // Real lost reply: downstream sends to relay; intermediary consumes complete relay response and returns zero bytes.
    let proxy = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_address = proxy.local_addr().unwrap();
    let loss = tokio::spawn(async move {
        let (mut downstream, _) = proxy.accept().await.unwrap();
        let mut upstream = tokio::net::TcpStream::connect(relay_address).await.unwrap();
        let mut request = Vec::new();
        loop { let b = downstream.read_u8().await.unwrap(); request.push(b); assert!(request.len() < 65536); if request.ends_with(b"\r\n\r\n") { break } }
        let header = String::from_utf8(request.clone()).unwrap();
        let length: usize = header.lines().find_map(|line| line.to_ascii_lowercase().strip_prefix("content-length:").map(|n| n.trim().parse().unwrap())).unwrap();
        assert!(length < 65536); let end = request.len(); request.resize(end + length, 0); downstream.read_exact(&mut request[end..]).await.unwrap();
        upstream.write_all(&request).await.unwrap();
        let mut reply = Vec::new(); upstream.take(1_048_576).read_to_end(&mut reply).await.unwrap();
        assert!(reply.starts_with(b"HTTP/1.1 201"));
        downstream.shutdown().await.unwrap();
        serde_json::from_slice::<Value>(&reply[reply.windows(4).position(|w| w == b"\r\n\r\n").unwrap()+4..]).unwrap()
    });
    let lost = json!({"requestId":uuid::Uuid::new_v4(),"workspaceId":ws,"worktree":{"wsId":ws,"slug":"relay-lost"}});
    let response = client.post(format!("http://{proxy_address}/host/a08-transport-owner/api/v1/workspace/worktrees")).header("connection", "close").bearer_auth(token).body(lost.to_string()).send().await;
    let original = tokio::time::timeout(LIMIT, loss).await.unwrap().unwrap();
    assert!(response.is_err());
    let (status, replay) = http(&client, POST, &endpoint, token, lost.clone()).await;
    assert_eq!(status, 201); assert_eq!(replay, original);
    let (status, operation) = http(&client, GET, &format!("{origin}/workspace/operations/{}", lost["requestId"].as_str().unwrap()), token, Value::Null).await;
    assert_eq!(status, 200); assert_eq!(operation["outcome"]["worktree"], original);
    let (_, list) = http(&client, GET, &format!("{endpoint}?workspaceId={ws}"), token, Value::Null).await;
    assert_eq!(list["worktrees"].as_array().unwrap().iter().filter(|v| v["path"] == original["path"]).count(), 1);
    let lost_path = canonical.join(".orca-worktrees").join(ws).join("relay-lost");
    assert_eq!(original["path"], lost_path.to_str().unwrap());
    assert_eq!(git(&lost_path, &["rev-parse", "HEAD"]).await.unwrap().trim(), head);
    eprintln!("A08_RELAY_LOST original={original} same_device=true replay_equal=true journal_equal=true actual_target_count=1 forwarded_reply_bytes=0 intermediary_joined=true");
    let row = uds(&local, json!({"type":"createWorktree","workspaceId":ws,"worktree":{"wsId":ws,"slug":"uds-owned"},"baseRef":"HEAD"})).await;
    assert_eq!(row["type"], "createWorktreeOk");
    let path = canonical.join(".orca-worktrees").join(ws).join("uds-owned");
    assert_eq!(git(&path, &["rev-parse", "HEAD"]).await.unwrap().trim(), head);
    tokio::fs::write(path.join("dirty.txt"), "owned dirty fixture").await.unwrap();
    let delete = json!({"type":"deleteWorktree","workspaceId":ws,"worktree":{"wsId":ws,"slug":"uds-owned"},"deleteBranch":true});
    let failure = uds(&local, delete.clone()).await;
    assert_eq!(failure["type"], "worktreeError");
    assert_eq!(failure["error"]["code"], "DIRTY_WORKTREE");
    assert_eq!(failure["error"]["details"]["path"], path.to_str().unwrap());
    assert_eq!(failure["error"]["details"]["count"], 1);
    assert_eq!(failure["error"]["details"]["files"], json!(["dirty.txt"]));
    assert!(tokio::fs::try_exists(&path).await.unwrap());
    tokio::fs::remove_file(path.join("dirty.txt")).await.unwrap();
    assert_eq!(uds(&local, delete).await["type"], "deleteWorktreeOk");
    assert!(!tokio::fs::try_exists(&path).await.unwrap());
    assert!(git(&repo, &["show-ref", "--verify", &format!("refs/heads/orca/{ws}/uds-owned")]).await.is_err());
    assert!(!tokio::fs::try_exists(root.join("wrong-root/.orca-worktrees")).await.unwrap());
    drop(local);
    for task in [control, gateway, relay, uds_task] { task.abort(); assert!(task.await.unwrap_err().is_cancelled()); }
    drop(owner);
    tokio::fs::remove_file(&socket).await.unwrap();
    assert!(tokio::net::UnixStream::connect(&socket).await.is_err());
    assert!(tokio::net::TcpStream::connect(relay_address).await.is_err());
    assert!(tokio::net::TcpStream::connect(gateway_address).await.is_err());
    eprintln!("A08_TRANSPORT_SUCCESS listeners_joined=true sockets_refused=true wrong_root_unchanged=true PTY_spawned=false");
}
