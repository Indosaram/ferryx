// Permanent relay pairing regressions over REAL loopback relay sockets.
//
// These lock in the repairs for the defects the Gen3 audit reproduced at 30555f2:
// a second machine's first pairing was denied because the relay compared the
// client's pairing-attempt counter with its own control generation, and a stale
// registration stayed claimable across control replacement. The harness is taken
// from the audit's own reproduction so the coverage matches how it was found.
//
// Unlike the audit's observation file these are NOT #[ignore]d: they must pass on
// every run, and a PASS here means the defect is absent.
// All peers use loopback sockets, synthetic identities/capabilities and temporary stores.
// The exchange responder is synthetic: these tests establish relay admission/forwarding,
// not unauthorized access to a real user's gateway or terminal.
use std::{net::SocketAddr, time::{Duration, SystemTime, UNIX_EPOCH}};
use std::future::IntoFuture;
use axum::{Router, http::StatusCode};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::SigningKey;
use ferryx_lib::remote::{
    auth::{MachineIdentity, sign_control_challenge},
    protocol::{ControlAuth, ControlAuthResponse, ControlChallenge, RegisterPairingPin, RegisterPairingPinAck},
    relay_server::{RelayState, relay_router, IncomingSessionNotice},
};
use futures_util::{FutureExt, SinkExt, StreamExt};
use tokio::{net::{TcpListener, TcpStream}, time::timeout};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite::Message};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

// Run once in a private process environment supplied by the command supervisor.
// The HTTP/WS client receives only the host-qualified relay origin; the direct
// listener address is passed exclusively to the production reverse client.
#[test]
fn a11_joint_machine_relay_runtime() {
    let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| runtime.block_on(a11_joint_fixture(false))));
    drop(runtime);
    if let Err(panic) = result { std::panic::resume_unwind(panic); }
}

async fn a11_response(request: reqwest::RequestBuilder, status: StatusCode) -> serde_json::Value {
    let response = request.send().await.unwrap();
    let actual = response.status();
    let bytes = response.bytes().await.unwrap();
    assert_eq!(actual, status, "{}", String::from_utf8_lossy(&bytes));
    if bytes.is_empty() { serde_json::Value::Null } else { serde_json::from_slice(&bytes).unwrap() }
}

async fn a11_ticket(client: &reqwest::Client, base: &str, token: &str, id: &str) -> String {
    a11_response(client.post(format!("{base}/api/v1/socket-ticket")).bearer_auth(token)
        .header("content-type", "application/json")
        .body(serde_json::json!({"target":format!("/api/v1/terminal/{id}")}).to_string()), StatusCode::OK).await["ticket"].as_str().unwrap().into()
}

async fn a11_marker(socket: &mut Ws, marker: &str) -> String {
    a11_marker_cursor(socket, marker).await.0
}

async fn a11_marker_cursor(socket: &mut Ws, marker: &str) -> (String, u64) {
    timeout(LIMIT, async {
        let mut bytes = Vec::new();
        loop {
            if let Message::Binary(frame) = socket.next().await.expect("marker stream ended").unwrap_or_else(|error| panic!("marker {marker}: {error}")) {
                let decoded = ferryx_lib::remote::terminal_wire::decode_frame(&frame).unwrap();
                bytes.extend_from_slice(decoded.terminal_bytes);
                if let Some((_, tail)) = String::from_utf8_lossy(&bytes).split_once(marker) {
                    if let Some((value, _)) = tail.split_once(":END") {
                        let sequence = match decoded.metadata {
                            ferryx_lib::remote::terminal_wire::Metadata::Output { sequence, .. } => sequence,
                            ferryx_lib::remote::terminal_wire::Metadata::Replay { end, .. } => end.expect("nonempty replay"),
                        };
                        return (value.to_owned(), sequence);
                    }
                }
            }
        }
    }).await.unwrap()
}

#[test]
fn a11_real_pty_failure_containment() {
    let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| runtime.block_on(a11_joint_fixture(true))));
    drop(runtime);
    let panic = result.expect_err("injected scenario must propagate failure after cleanup");
    assert_eq!(panic.downcast_ref::<&str>(), Some(&"A11 injected failure with two live relay PTYs"));
}

async fn a11_joint_fixture(inject_failure: bool) {
    use ferryx_lib::remote::{auth::{DeviceAccessScope, DevicePermission}, relay_client::RelayClient};
    use serde_json::json;
    let (dir, daemon, path) = tokio::task::spawn_blocking(|| {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("project %2F space");
        std::fs::create_dir(&path).unwrap();
        let daemon = ferryx_lib::daemon::server::DaemonServer::new_with_paths(
            Some(dir.path().join("config.json")), Some(dir.path().join("auth.json")));
        (dir, daemon, path.canonicalize().unwrap())
    }).await.unwrap();
    let backend = daemon.terminal_service().clone();
    let identity_path = dir.path().to_owned();
    let actual_identity = tokio::task::spawn_blocking(move || ferryx_lib::remote::auth::load_or_generate_machine_identity(&identity_path).unwrap()).await.unwrap();
    let machine = actual_identity.machine_id.clone();
    let state = RelayState::new_with_key_store(vec![], dir.path().join("keys.json")).unwrap();
    // Hold exactly one completed relay response before Axum can write any
    // headers to the requesting TCP client. Receiving it proves durable create
    // completed; aborting the client before release really loses the reply.
    let (committed_tx, committed_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let gate = std::sync::Arc::new(tokio::sync::Mutex::new(Some((committed_tx, release_rx))));
    let router = relay_router(state).layer(axum::middleware::from_fn(move |request: axum::extract::Request, next: axum::middleware::Next| {
        let gate = gate.clone();
        async move {
            let lose = request.headers().contains_key("x-a11-lose-reply");
            let response = next.run(request).await;
            if lose {
                let (parts, body) = response.into_parts();
                let bytes = axum::body::to_bytes(body, 65536).await.unwrap();
                let (sent, release) = gate.lock().await.take().unwrap();
                sent.send((parts.status, bytes.clone())).unwrap();
                let _ = timeout(LIMIT, release).await;
                axum::response::Response::from_parts(parts, axum::body::Body::from(bytes))
            } else { response }
        }
    }));
    let (relay_origin, relay) = serve_router(router).await;
    let (direct, gateway) = serve_router(ferryx_lib::remote::server::create_remote_router(daemon.remote_state().clone())).await;
    let reverse = RelayClient::with_identity(&relay_origin, actual_identity, direct.strip_prefix("http://").unwrap())
        .with_auth_manager((*daemon.remote_state().auth_manager).clone());
    let coordinator = reverse.pairing_coordinator();
    let control = tokio::spawn(async move { reverse.run().await });
    let result = std::panic::AssertUnwindSafe(async {
        let pairing = timeout(LIMIT, coordinator.generate_scoped_pairing(Duration::from_secs(60), DevicePermission::Control, DeviceAccessScope::Machine)).await.unwrap().unwrap();
        let client = client();
        let paired = a11_response(client.post(format!("{relay_origin}/api/v1/pair/exchange"))
            .header("content-type", "application/json")
            .body(json!({"code":pairing.pairing_token,"deviceName":"a11"}).to_string()), StatusCode::OK).await;
        let token = paired["token"].as_str().unwrap();
        let base = format!("{relay_origin}/host/{machine}");
        let capabilities = a11_response(client.get(format!("{base}/api/v1/capabilities")).bearer_auth(token), StatusCode::OK).await;
        assert_eq!(capabilities["machineId"], machine);
        assert!(capabilities["capabilities"].as_array().unwrap().contains(&json!("terminalCreateV1")));
        let browse = a11_response(client.get(format!("{base}/api/v1/fs/directories")).bearer_auth(token)
            .query(&[("path",path.to_str().unwrap())]), StatusCode::OK).await;
        assert_eq!(browse["path"], path.to_str().unwrap());
        let project = a11_response(client.post(format!("{base}/api/v1/workspace/projects")).bearer_auth(token)
            .body(json!({"requestId":uuid::Uuid::new_v4().to_string(),"repoPath":path}).to_string()), StatusCode::CREATED).await;
        let create = json!({"requestId":uuid::Uuid::new_v4().to_string(),"workspaceId":project["workspaceId"],"worktree":null,"inheritFromSessionId":null,"cwdRelative":null,"cols":80,"rows":24,"startup":{"kind":"shell"}});
        let request = client.post(format!("{base}/api/v1/sessions")).bearer_auth(token).header("x-a11-lose-reply", "1").body(create.to_string());
        let pending = tokio::spawn(async move { request.send().await });
        let (status, bytes) = timeout(LIMIT, committed_rx).await.unwrap().unwrap();
        assert_eq!(status, StatusCode::CREATED);
        let session: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        pending.abort(); assert!(pending.await.unwrap_err().is_cancelled());
        release_tx.send(()).unwrap();
        let operation = a11_response(client.get(format!("{base}/api/v1/workspace/operations/{}", create["requestId"].as_str().unwrap())).bearer_auth(token), StatusCode::OK).await;
        assert_eq!(operation["outcome"]["session"]["target"], session["target"], "{operation}");
        eprintln!("A11 lost real TCP reply after completed201; operation lookup recovered target={}", session["target"]);
        let target = &session["target"];
        assert_eq!(target["machineId"], machine);
        let id = target["sessionId"].as_str().unwrap();
        let epoch = target["daemonEpoch"].as_str().unwrap();
        let pty = backend.get_session(id).unwrap();
        let pid = pty.pid().unwrap();
        let replay = a11_response(client.post(format!("{base}/api/v1/sessions")).bearer_auth(token).body(create.to_string()), StatusCode::CREATED).await;
        assert_eq!(replay, session);
        assert_eq!(backend.list_sessions(), vec![id.to_owned()]);
        let detail = a11_response(client.get(format!("{base}/api/v1/sessions/{id}")).bearer_auth(token), StatusCode::OK).await;
        assert_eq!(detail["session"]["target"], *target);
        let wsbase = base.replace("http://", "ws://");
        let second_path = path.parent().unwrap().join("second-root");
        let mkdir = second_path.clone();
        tokio::task::spawn_blocking(move || std::fs::create_dir(mkdir).unwrap()).await.unwrap();
        let second_project = a11_response(client.post(format!("{base}/api/v1/workspace/projects")).bearer_auth(token).body(json!({"requestId":uuid::Uuid::new_v4().to_string(),"repoPath":second_path}).to_string()), StatusCode::CREATED).await;
        let mut second_create = create.clone();
        second_create["requestId"] = json!(uuid::Uuid::new_v4().to_string()); second_create["workspaceId"] = second_project["workspaceId"].clone();
        let second = a11_response(client.post(format!("{base}/api/v1/sessions")).bearer_auth(token).body(second_create.to_string()), StatusCode::CREATED).await;
        let second_id = second["target"]["sessionId"].as_str().unwrap();
        let second_pty = backend.get_session(second_id).unwrap(); let second_pid = second_pty.pid().unwrap();
        assert_ne!(pid, second_pid);
        let ticket = a11_ticket(&client, &base, token, second_id).await;
        let (mut second_socket, _) = tokio_tungstenite::connect_async(format!("{wsbase}/api/v1/terminal/{second_id}?ticket={ticket}&daemonEpoch={epoch}")).await.unwrap();
        let boundary: serde_json::Value = json(&mut second_socket).await; assert_eq!(boundary["target"], second["target"]);
        second_socket.send(Message::Binary(b"printf '\\nA11_%s:%s:%s:END\\n' SECOND \"$$\" \"$PWD\"\r".to_vec().into())).await.unwrap();
        assert_eq!(a11_marker(&mut second_socket, "A11_SECOND:").await, format!("{second_pid}:{}", second_path.display()));
        eprintln!("A11 second-root original pid={second_pid} cwd={} epoch={epoch}", second_path.display());
        if inject_failure { panic!("A11 injected failure with two live relay PTYs"); }
        let ticket = a11_ticket(&client, &base, token, id).await;
        let (mut stale, _) = tokio_tungstenite::connect_async(format!("{wsbase}/api/v1/terminal/{id}?ticket={ticket}&daemonEpoch=1")).await.unwrap();
        let rejected = timeout(LIMIT, stale.next()).await.unwrap();
        assert!(matches!(rejected, None | Some(Err(_)) | Some(Ok(Message::Close(_)))));
        for suffix in ["daemonEpoch=01", "daemonEpoch=1&daemonEpoch=2", "afterSequence=01", "afterSequence=18446744073709551616", "token=forbidden", "rows=+2", "rows=02"] {
            let ticket = a11_ticket(&client, &base, token, id).await;
            let response = timeout(LIMIT, tokio_tungstenite::connect_async(format!("{wsbase}/api/v1/terminal/{id}?ticket={ticket}&{suffix}"))).await.unwrap();
            assert!(matches!(response, Err(tokio_tungstenite::tungstenite::Error::Http(ref r)) if r.status() == StatusCode::UNAUTHORIZED), "{suffix}");
        }
        for foreign in [format!("{}/host/foreign/api/v1/terminal/{id}", relay_origin.replace("http://", "ws://")), format!("{wsbase}/api/v1/terminal/foreign")] {
            let ticket = a11_ticket(&client, &base, token, id).await;
            let response = timeout(LIMIT, tokio_tungstenite::connect_async(format!("{foreign}?ticket={ticket}&daemonEpoch={epoch}"))).await.unwrap();
            assert!(matches!(response, Err(tokio_tungstenite::tungstenite::Error::Http(ref r)) if r.status() == StatusCode::UNAUTHORIZED));
        }
        // A noncanonical numeric mirror field must not be normalized and
        // forwarded. Real machine admission otherwise upgrades this request.
        let ticket = a11_ticket(&client, &base, token, id).await;
        let malformed = format!("{wsbase}/api/v1/terminal/{id}?ticket={ticket}&daemonEpoch={epoch}&cols=01");
        let response = timeout(LIMIT, tokio_tungstenite::connect_async(malformed)).await.unwrap();
        assert!(matches!(response, Err(tokio_tungstenite::tungstenite::Error::Http(ref r)) if r.status() == StatusCode::UNAUTHORIZED), "noncanonical cols must fail relay admission before upgrade");
        let ticket = a11_ticket(&client, &base, token, id).await;
        let url = format!("{wsbase}/api/v1/terminal/{id}?ticket={ticket}&daemonEpoch={epoch}&afterSequence=0");
        let (mut socket, _) = timeout(LIMIT, tokio_tungstenite::connect_async(&url)).await.unwrap().unwrap();
        let attached: serde_json::Value = json(&mut socket).await;
        assert_eq!(attached["type"], "attached"); assert_eq!(attached["target"], *target);
        socket.send(Message::Binary(b"printf '\\nA11_%s:%s:%s:END\\n' PROOF \"$$\" \"$PWD\"\r".to_vec().into())).await.unwrap();
        let (proof, cursor) = timeout(LIMIT, async {
            let mut bytes = Vec::new();
            loop {
                if let Message::Binary(frame) = socket.next().await.unwrap().unwrap() {
                    let decoded = ferryx_lib::remote::terminal_wire::decode_frame(&frame).unwrap();
                    bytes.extend_from_slice(decoded.terminal_bytes);
                    if let Some((_, tail)) = String::from_utf8_lossy(&bytes).split_once("A11_PROOF:") {
                        if let Some((proof,_)) = tail.split_once(":END") { break (proof.to_owned(), decoded.metadata); }
                    }
                }
            }
        }).await.unwrap();
        assert_eq!(proof, format!("{pid}:{}", path.display()));
        eprintln!("A11 relay-only executed proof pid={pid} cwd={} target={target} cursor={cursor:?}", path.display());
        let replayed = timeout(LIMIT, tokio_tungstenite::connect_async(&url)).await.unwrap();
        assert!(matches!(replayed, Err(tokio_tungstenite::tungstenite::Error::Http(ref r)) if r.status() == StatusCode::UNAUTHORIZED));
        let after = match cursor { ferryx_lib::remote::terminal_wire::Metadata::Output { sequence, .. } => sequence, _ => panic!("live output cursor") };
        let ticket = a11_ticket(&client, &base, token, id).await;
        let (mut resumed, _) = timeout(LIMIT, tokio_tungstenite::connect_async(format!("{wsbase}/api/v1/terminal/{id}?ticket={ticket}&daemonEpoch={epoch}&afterSequence={after}"))).await.unwrap().unwrap();
        let attached: serde_json::Value = json(&mut resumed).await;
        assert_eq!(attached["target"], *target); assert!(attached["replayGap"].is_null());
        assert_eq!(pty.pid(), Some(pid));
        resumed.send(Message::Text(json!({"type":"resize","generation":attached["generation"],"cols":103,"rows":37}).to_string().into())).await.unwrap();
        resumed.send(Message::Text(json!({"type":"ping"}).to_string().into())).await.unwrap();
        timeout(LIMIT, async { loop { if let Some(Ok(Message::Text(text))) = resumed.next().await { if serde_json::from_str::<serde_json::Value>(&text).unwrap()["type"] == "pong" { break; } } } }).await.unwrap();
        assert_eq!(pty.get_size(), (103,37));
        resumed.send(Message::Binary(b"trap 'printf \"\\nA11_%s:ok:END\\n\" INTERRUPTED' INT; printf '\\nA11_%s:ok:END\\n' ARMED\r".to_vec().into())).await.unwrap();
        assert_eq!(a11_marker(&mut resumed, "A11_ARMED:").await, "ok");
        resumed.send(Message::Text(json!({"type":"signal","generation":attached["generation"],"signal":"interrupt"}).to_string().into())).await.unwrap();
        assert_eq!(a11_marker(&mut resumed, "A11_INTERRUPTED:").await, "ok");
        // PTY-produced replay: no direct publication or synthetic gap injection.
        // Each command is triggered only after the preceding exact output marker.
        resumed.send(Message::Binary(b"printf '\\nA11_%s:ok:END\\n' REAL_BEFORE\r".to_vec().into())).await.unwrap();
        let (_, real_before) = a11_marker_cursor(&mut resumed, "A11_REAL_BEFORE:").await;
        resumed.send(Message::Binary(b"printf '\\nA11_%s:ok:END\\n' REAL_SUFFIX\r".to_vec().into())).await.unwrap();
        assert_eq!(a11_marker(&mut resumed, "A11_REAL_SUFFIX:").await, "ok");
        let ticket = a11_ticket(&client, &base, token, id).await;
        let (mut real_replay, _) = tokio_tungstenite::connect_async(format!("{wsbase}/api/v1/terminal/{id}?ticket={ticket}&daemonEpoch={epoch}&afterSequence={real_before}")).await.unwrap();
        let boundary: serde_json::Value = json(&mut real_replay).await;
        assert_eq!(boundary["target"], *target);
        assert!(boundary["replayGap"].is_null());
        assert_eq!(a11_marker(&mut real_replay, "A11_REAL_SUFFIX:").await, "ok");
        // Fill retained history without also testing slow-consumer eviction:
        // each exact PTY marker acknowledges a drained 64KiB before the next.
        for index in 0..16 {
            let command = format!("printf '%65536s' ''; printf '\\nA11_%s_{index}:ok:END\\n' REAL_GAP\r");
            real_replay.send(Message::Binary(command.into_bytes().into())).await.unwrap();
            assert_eq!(a11_marker(&mut real_replay, &format!("A11_REAL_GAP_{index}:")).await, "ok");
        }
        let ticket = a11_ticket(&client, &base, token, id).await;
        let (mut real_gap, _) = tokio_tungstenite::connect_async(format!("{wsbase}/api/v1/terminal/{id}?ticket={ticket}&daemonEpoch={epoch}&afterSequence={real_before}")).await.unwrap();
        let boundary: serde_json::Value = json(&mut real_gap).await;
        assert_eq!(boundary["target"], *target);
        assert!(!boundary["replayGap"].is_null(), "real retained-history overflow must report a gap");
        assert_eq!(a11_marker(&mut real_gap, "A11_REAL_GAP_15:").await, "ok");
        assert_eq!(pty.pid(), Some(pid));
        real_gap.close(None).await.unwrap();
        real_replay.close(None).await.unwrap();
        eprintln!("A11 real PTY suffix and 1MiB retained-history gap passed; unchanged pid={pid} cursor={real_before}");
        let hub = backend.output_hub();
        let before = hub.publish(id, b"A11_BEFORE".to_vec()).unwrap().sequence;
        hub.publish(id, b"A11_SUFFIX".to_vec()).unwrap();
        for (cursor, gap, expected) in [(before.to_string(), false, "A11_SUFFIX"), ("0".into(), true, "A11_GAP")] {
            if gap { hub.publish_gap(id).unwrap(); hub.publish(id, b"A11_GAP".to_vec()).unwrap(); }
            let ticket = a11_ticket(&client, &base, token, id).await;
            let (mut replay_socket, _) = tokio_tungstenite::connect_async(format!("{wsbase}/api/v1/terminal/{id}?ticket={ticket}&daemonEpoch={epoch}&afterSequence={cursor}")).await.unwrap();
            let boundary: serde_json::Value = json(&mut replay_socket).await;
            assert_eq!(!boundary["replayGap"].is_null(), gap);
            let Message::Binary(frame) = timeout(LIMIT, replay_socket.next()).await.unwrap().unwrap().unwrap() else { panic!("replay bytes") };
            let decoded = ferryx_lib::remote::terminal_wire::decode_frame(&frame).unwrap();
            assert!(decoded.terminal_bytes.ends_with(expected.as_bytes()));
            assert!(!decoded.terminal_bytes.windows(10).any(|w| w == b"A11_BEFORE"));
            if gap { assert!(decoded.terminal_bytes.starts_with(b"\x1bc")); }
            replay_socket.close(None).await.unwrap();
        }
        eprintln!("A11 resize103x37/interrupt/suffix/gap passed");
        resumed.close(None).await.unwrap();
        socket.close(None).await.unwrap();
        let close = json!({"requestId":uuid::Uuid::new_v4().to_string(),"daemonEpoch":epoch});
        a11_response(client.delete(format!("{base}/api/v1/sessions/{id}")).bearer_auth(token).body(close.to_string()), StatusCode::NO_CONTENT).await;
        assert!(pty.is_reaped()); assert_eq!(second_pty.pid(), Some(second_pid));
        second_socket.close(None).await.unwrap();
        a11_response(client.delete(format!("{base}/api/v1/sessions/{second_id}")).bearer_auth(token).body(json!({"requestId":uuid::Uuid::new_v4().to_string(),"daemonEpoch":epoch}).to_string()), StatusCode::NO_CONTENT).await;
        assert!(second_pty.is_reaped()); assert!(backend.list_sessions().is_empty());
        eprintln!("A11 DELETE body preserved; original pid={pid} reaped; create replay/detail/ticket replay passed");
    }).catch_unwind().await;
    control.abort(); assert!(control.await.unwrap_err().is_cancelled());
    for id in backend.list_sessions() {
        let pty = backend.get_session(&id).unwrap();
        timeout(Duration::from_secs(10), backend.close_session(&id)).await.unwrap().unwrap();
        assert!(pty.is_reaped());
        eprintln!("A11 cleanup original pid={:?} reaped", pty.pid());
    }
    drop(gateway);
    assert!(TcpStream::connect(direct.strip_prefix("http://").unwrap()).await.is_err());
    drop(daemon);
    finish_scoped_fixture(relay, dir, result).await;
}

#[tokio::test]
async fn machine_routes_reach_relay_admission() {
    let (dir, state) = isolated_state(vec![]).await;
    let (base, server) = serve(state).await;
    let outcome = std::panic::AssertUnwindSafe(async {
    for route in ["capabilities", "fs/directories?path=%2Ftmp%2Fa%252Fb&includeHidden=true", "sessions"] {
        let response = client().get(format!("{base}/host/offline/api/v1/{route}"))
            .bearer_auth("synthetic").send().await.unwrap();
        // An unknown machine is 404 after admission, not an allowlist 403.
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{route}");
    }
    }).catch_unwind().await;
    finish_scoped_fixture(server, dir, outcome).await;
}
#[test]
fn r4_absolute_replacement_filename_direct_and_relay() {
    let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| runtime.block_on(absolute_filename_fixture())));
    drop(runtime); // Also joins coordinator expiry and reverse client's data tasks.
    if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
}

async fn absolute_filename_fixture() {
    use ferryx_lib::remote::{auth::{DeviceAccessScope, DevicePermission}, relay_client::RelayClient};
    let (dir, daemon, path) = tokio::task::spawn_blocking(|| {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("replacement-\u{fffd}");
        std::fs::create_dir(&path).unwrap();
        std::fs::create_dir(path.join("child")).unwrap();
        let daemon = ferryx_lib::daemon::server::DaemonServer::new_with_paths(
            Some(dir.path().join("config.json")), Some(dir.path().join("auth.json")));
        let canonical = path.canonicalize().unwrap();
        // Browser requests use navigable drive paths, not OS verbatim syntax.
        let path = std::path::PathBuf::from(ferryx_lib::worktree::strip_verbatim_prefix(canonical.to_str().unwrap()));
        (dir, daemon, path)
    }).await.unwrap();
    let state = RelayState::new_with_key_store(vec![], dir.path().join("keys.json")).unwrap();
    let (base, relay) = serve(state).await;
    let (direct, gateway) = serve_router(ferryx_lib::remote::server::create_remote_router(daemon.remote_state().clone())).await;
    let reverse = RelayClient::with_identity(&base, identity(96, "r4-machine"), direct.strip_prefix("http://").unwrap())
        .with_auth_manager((*daemon.remote_state().auth_manager).clone());
    let coordinator = reverse.pairing_coordinator();
    let control = tokio::spawn(async move { reverse.run().await });
    let outcome = std::panic::AssertUnwindSafe(async {
        let pairing = coordinator.generate_scoped_pairing(Duration::from_secs(60), DevicePermission::Control, DeviceAccessScope::Machine).await.unwrap();
        let response = client().post(format!("{base}/api/v1/pair/exchange"))
            .header("content-type", "application/json")
            .body(serde_json::json!({"code": pairing.pairing_token, "deviceName": "r4"}).to_string()).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
        let token = body["token"].as_str().unwrap();
        let mut bodies = Vec::new();
        for origin in [&direct, &format!("{base}/host/r4-machine")] {
            let response = client().get(format!("{origin}/api/v1/fs/directories"))
                .bearer_auth(&token).query(&[("path", path.to_str().unwrap())]).send().await.unwrap();
            assert_eq!(response.status(), StatusCode::OK, "absolute replacement-character filename via {origin}");
            let body: serde_json::Value = serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
            assert_eq!(body["path"], path.to_str().unwrap());
            assert_eq!(body["entries"][0]["name"], "child");
            bodies.push(body);
        }
        assert_eq!(bodies[0], bodies[1]);
        for origin in [&direct, &format!("{base}/host/r4-machine")] {
            assert_eq!(client().get(format!("{origin}/api/v1/fs/directories?path=%2Ftmp%2F%FF"))
                .bearer_auth(&token).send().await.unwrap().status(), StatusCode::BAD_REQUEST);
            assert_eq!(client().get(format!("{origin}/api/v1/terminal/preferences"))
                .bearer_auth(&token).send().await.unwrap().status(), StatusCode::OK);
        }
        println!("R4 real gateway: absolute U+FFFD direct=200 relay=200 equal; malformed direct=400 relay=400; preferences direct=200 relay=200");
    }).catch_unwind().await;
    control.abort();
    assert!(control.await.unwrap_err().is_cancelled());
    drop(gateway);
    drop(daemon);
    finish_scoped_fixture(relay, dir, outcome).await;
}

async fn isolated_state(tokens: Vec<String>) -> (tempfile::TempDir, RelayState) {
    // The production run_blocking helper is crate-private; integration fixtures
    // use its underlying blocking executor rather than doing disk IO on Tokio.
    tokio::task::spawn_blocking(move || {
        let dir = tempfile::tempdir().unwrap();
        let state = RelayState::new_with_key_store(tokens, dir.path().join("keys.json")).unwrap();
        (dir, state)
    }).await.unwrap()
}

#[tokio::test]
async fn r4_failure_path_joins_connections_and_removes_store() {
    let (dir, state) = isolated_state(vec![]).await;
    let path = dir.path().to_owned();
    let (base, server) = serve(state).await;
    let outcome = std::panic::AssertUnwindSafe(async {
        let mut socket = authenticate(&base, &identity(97, "cleanup")).await;
        register(&mut socket, &registration("cleanup", Some(1), "991122")).await;
        panic!("injected failure after live control registration");
    }).catch_unwind().await;
    assert!(outcome.is_err());
    let propagated = std::panic::AssertUnwindSafe(finish_scoped_fixture(server, dir, outcome)).catch_unwind().await;
    assert!(propagated.is_err());
    assert!(!path.exists());
    assert!(TcpStream::connect(base.strip_prefix("http://").unwrap()).await.is_err());
    println!("R4 injected failure: panic propagated after runtime joined; store absent; listener refused");
}

#[tokio::test]
async fn machine_http_transport() {
    let (dir, state) = isolated_state(vec!["fixture-machine".into()]).await;
    let (base, server) = serve(state).await;
    let outcome = std::panic::AssertUnwindSafe(async {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let mut request = format!("{}/tunnel/control", base.replace("http://", "ws://")).into_client_request().unwrap();
    request.headers_mut().insert("authorization", "Bearer fixture-machine".parse().unwrap());
    let (mut control, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    control.send(Message::Text(r#"{"type":"AllocateSession"}"#.into())).await.unwrap();
    let _: IncomingSessionNotice = json(&mut control).await;
    for (method, path) in [
        ("GET", "capabilities"),
        ("GET", "fs/directories?path=%2Ftmp%2Fa%252Fb+%E9%9B%AA&includeHidden=true"),
        ("GET", "workspace/projects"), ("POST", "workspace/projects"),
        ("DELETE", "workspace/projects/project-one"),
        ("GET", "workspace/worktrees?workspaceId=project-one"),
        ("POST", "workspace/worktrees"), ("DELETE", "workspace/worktrees"),
        ("GET", "workspace/worktrees/status?workspaceId=p&wsId=p&slug=s"),
        ("GET", "workspace/operations/request-one"),
        ("GET", "sessions"), ("POST", "sessions"),
        ("GET", "sessions/session-one"), ("DELETE", "sessions/session-one"),
    ] {
        let body = if method == "DELETE" { r#"{"requestId":"fixture","daemonEpoch":"9007199254740993","expectedRevision":"7"}"# } else { "" };
        let http = client().request(method.parse().unwrap(), format!("{base}/host/fixture-machine/api/v1/{path}"))
            .bearer_auth("fixture-device").body(body).send();
        let peer = async {
            let notice: IncomingSessionNotice = json(&mut control).await;
            let (mut data, _) = tokio_tungstenite::connect_async(format!("{}/tunnel/data/{}", base.replace("http://", "ws://"), notice.session_id)).await.unwrap();
            let wire = timeout(LIMIT, data.next()).await.unwrap().unwrap().unwrap().into_data();
            let raw = std::str::from_utf8(&wire).unwrap();
            assert!(raw.starts_with(&format!("{method} /api/v1/{path} HTTP/1.1\r\n")));
            assert!(raw.contains("authorization: Bearer fixture-device\r\n"));
            assert_eq!(raw.split_once("\r\n\r\n").unwrap().1, body);
            data.send(Message::Binary(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}".to_vec().into())).await.unwrap();
        };
        let (response, ()) = timeout(LIMIT, async { tokio::join!(http, peer) }).await.unwrap();
        assert_eq!(response.unwrap().status(), StatusCode::OK, "{method} {path}");
    }
    for path in ["fs/read", "workspace/arbitrary", "sessions/s/extra"] {
        assert_eq!(client().get(format!("{base}/host/fixture-machine/api/v1/{path}")).send().await.unwrap().status(), StatusCode::FORBIDDEN);
    }
    assert_eq!(client().put(format!("{base}/host/fixture-machine/api/v1/sessions")).send().await.unwrap().status(), StatusCode::FORBIDDEN);
    assert_eq!(client().post(format!("{base}/host/fixture-machine/api/v1/sessions")).body(vec![0; 65537]).send().await.unwrap().status(), StatusCode::PAYLOAD_TOO_LARGE);
    control.close(None).await.unwrap();
    }).catch_unwind().await;
    finish_scoped_fixture(server, dir, outcome).await;
}

// Capture assertion failures so fixture cleanup completes before the test reports
// failure. The runtime thread also owns independent Axum connection tasks.
async fn finish_scoped_fixture(
    mut server: Running,
    dir: tempfile::TempDir,
    outcome: std::thread::Result<()>,
) {
    drop(server.0.take());
    let thread = server.1.take().unwrap();
    let stopped = tokio::task::spawn_blocking(move || thread.join()).await;
    let removed = tokio::task::spawn_blocking(move || dir.close()).await;
    assert!(matches!(stopped, Ok(Ok(()))), "relay runtime did not join: {stopped:?}");
    removed.expect("fixture cleanup task failed").expect("fixture directory cleanup failed");
    println!("R4 CLEANUP runtime/connection tasks joined; private directory explicitly removed; failed_scenario={}", outcome.is_err());
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

const LIMIT: Duration = Duration::from_secs(8);
// A dedicated runtime owns Axum's otherwise detached connection/upgrade tasks.
// Stopping and joining its thread drops every task, not only the listener.
struct Running(Option<tokio::sync::oneshot::Sender<()>>, Option<std::thread::JoinHandle<()>>);
impl Drop for Running {
    fn drop(&mut self) {
        drop(self.0.take());
        if let Some(thread) = self.1.take() { thread.join().expect("fixture runtime panicked"); }
    }
}

fn identity(seed: u8, machine: &str) -> MachineIdentity {
    let key = SigningKey::from_bytes(&[seed; 32]);
    MachineIdentity {
        machine_id: machine.into(), display_name: "isolated final audit".into(),
        public_key: STANDARD.encode(key.verifying_key().to_bytes()),
        private_key: STANDARD.encode(key.to_bytes()),
    }
}
async fn serve(state: RelayState) -> (String, Running) {
    serve_router(relay_router(state)).await
}
async fn serve_router(router: Router) -> (String, Running) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();

    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let task = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async move {
            let listener = TcpListener::from_std(listener).unwrap();
            tokio::select! {
                result = axum::serve(listener, router.into_make_service_with_connect_info::<SocketAddr>()).into_future() => result.unwrap(),
                _ = stopped => {}
            }
        });
        drop(runtime);
    });
    (format!("http://{address}"), Running(Some(stop), Some(task)))
}
fn client() -> reqwest::Client {
    // Keep loopback observations independent of workstation/system HTTP proxies.
    reqwest::Client::builder().no_proxy().timeout(LIMIT).redirect(reqwest::redirect::Policy::none()).build().unwrap()
}
async fn json<T: serde::de::DeserializeOwned>(socket: &mut Ws) -> T {
    loop {
        let message = timeout(LIMIT, socket.next()).await.unwrap().unwrap().unwrap();
        match message {
            Message::Text(text) => return serde_json::from_str(&text).unwrap(),
            Message::Ping(data) => socket.send(Message::Pong(data)).await.unwrap(),
            other => panic!("Expected JSON, received {other:?}"),
        }
    }
}
async fn authenticate(base: &str, id: &MachineIdentity) -> Ws {
    let (mut socket, _) = tokio_tungstenite::connect_async(format!("{}/tunnel/control", base.replace("http://", "ws://"))).await.unwrap();
    let challenge: ControlChallenge = json(&mut socket).await;
    let auth = ControlAuth {
        enrollment_token: None, machine_id: id.machine_id.clone(),
        display_name: id.display_name.clone(), public_key: id.public_key.clone(),
        signature: sign_control_challenge(id, "relay", &challenge.nonce, challenge.timestamp).unwrap(),
        timestamp: challenge.timestamp,
    };
    socket.send(Message::Text(serde_json::to_string(&auth).unwrap().into())).await.unwrap();
    let reply: ControlAuthResponse = json(&mut socket).await;
    assert!(reply.success, "Synthetic owner failed authentication: {:?}", reply.error);
    socket
}
fn registration(machine: &str, generation: Option<u64>, pin: &str) -> RegisterPairingPin {
    RegisterPairingPin {
        generation, machine_id: machine.into(), pin: pin.into(),
        pairing_token: format!("audit-capability-{pin}-synthetic"),
        expires_at: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() + 60,
    }
}
async fn register(socket: &mut Ws, request: &RegisterPairingPin) {
    socket.send(Message::Text(serde_json::to_string(request).unwrap().into())).await.unwrap();
    let ack: RegisterPairingPinAck = json(socket).await;
    assert_eq!(ack.status, "ready");
    assert_eq!(ack.pin, request.pin);
    assert_eq!(ack.generation, request.generation);
}
async fn exchange(base: &str, pin: &str) -> reqwest::Response {
    client().post(format!("{base}/api/v1/pair/exchange"))
        .header("content-type", "application/json")
        .body(serde_json::json!({"pin":pin,"deviceName":"audit browser"}).to_string())
        .send().await.unwrap()
}
// A 409 from the synthetic responder makes it unambiguous that the relay accepted
// the claim and forwarded its secret. It is NOT a successful real device exchange.
async fn observe_forwarded_claim(base: &str, owner: &mut Ws, request: &RegisterPairingPin) {
    let http = exchange(base, &request.pin);
    let peer = async {
    let notice: IncomingSessionNotice = json(owner).await;
    let (mut data, _) = tokio_tungstenite::connect_async(format!("{}/tunnel/data/{}", base.replace("http://", "ws://"), notice.session_id)).await.unwrap();
    let wire = timeout(LIMIT, data.next()).await.unwrap().unwrap().unwrap();
    let bytes = wire.into_data();
    let raw = std::str::from_utf8(&bytes).unwrap();
    assert!(raw.starts_with("POST /api/v1/pair/exchange HTTP/1.1\r\n"));
    assert!(raw.contains(&request.pairing_token), "Claim must actually forward the old capability");
    data.send(Message::Binary(b"HTTP/1.1 409 Conflict\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec().into())).await.unwrap();
    };
    let (result, ()) = timeout(LIMIT, async { tokio::join!(http, peer) }).await.unwrap();
    assert_eq!(result.status(), StatusCode::CONFLICT, "Expected the responder's status, not relay rejection");
}

/// The exact case the audit reproduced: an unrelated machine connects first and
/// advances the relay-wide counter, then a second machine performs its very first
/// pairing, which the real PairingCoordinator sends as attempt 1. That must pair.
#[tokio::test]
async fn second_machine_first_pairing_is_not_blocked_by_an_unrelated_connection() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let state = RelayState::new_with_key_store(vec![], dir.path().join("keys.json")).unwrap();
    let (base, server) = serve(state).await;
    let outcome = std::panic::AssertUnwindSafe(async {

    let mut unrelated = authenticate(&base, &identity(91, "unrelated")).await;
    register(&mut unrelated, &registration("unrelated", Some(1), "110001")).await;

    let mut second = authenticate(&base, &identity(92, "second-machine")).await;
    let request = registration("second-machine", Some(1), "110002");
    register(&mut second, &request).await;

    // The relay must accept the claim and forward it to the owning daemon. The
    // synthetic responder's 409 proves forwarding happened rather than rejection.
    observe_forwarded_claim(&base, &mut second, &request).await;
    }).catch_unwind().await;
    finish_scoped_fixture(server, dir, outcome).await;
}

/// A registration whose owner's control channel has been replaced must stop being
/// claimable, whether the client sent no attempt number or an arbitrary one.
#[tokio::test]
async fn stale_registration_stops_being_claimable_after_control_replacement() {
    for (seed, machine, generation, pin) in [
        (93u8, "optional-generation", None, "220001"),
        (94u8, "chosen-generation", Some(2), "230001"),
    ] {
        let dir = tempfile::tempdir_in(".").unwrap();
        let state = RelayState::new_with_key_store(vec![], dir.path().join("keys.json")).unwrap();
        let (base, _server) = serve(state).await;
        let outcome = std::panic::AssertUnwindSafe(async {

        let id = identity(seed, machine);
        let mut original = authenticate(&base, &id).await;
        let request = registration(&id.machine_id, generation, pin);
        register(&mut original, &request).await;

        // Replace the owner's control channel. ACKing a DIFFERENT registration proves
        // the replacement is live without touching the PIN under test.
        let mut replacement = authenticate(&base, &id).await;
        let other = format!("{}9", &pin[..5]);
        register(&mut replacement, &registration(&id.machine_id, generation, &other)).await;

        assert_eq!(
            exchange(&base, &request.pin).await.status(),
            StatusCode::NOT_FOUND,
            "a registration from a superseded control generation must not be claimable ({machine})"
        );
        }).catch_unwind().await;
        finish_scoped_fixture(_server, dir, outcome).await;
    }
}
