use ferryx_lib::{daemon::server::DaemonServer, remote::server::create_remote_router};
use futures_util::{FutureExt, SinkExt, StreamExt};
use serde_json::{json, Value};
use std::time::Duration;
use tokio_tungstenite::{connect_async, tungstenite::Message};

type Socket = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
const DEADLINE: Duration = Duration::from_secs(10);

async fn reply(request: reqwest::RequestBuilder, status: u16) -> Value {
    let response = request.send().await.unwrap();
    let actual = response.status().as_u16();
    let text = response.text().await.unwrap();
    assert_eq!(actual, status, "{text}");
    if text.is_empty() { Value::Null } else { serde_json::from_str(&text).unwrap() }
}

async fn ticket(client: &reqwest::Client, base: &str, token: &str, id: &str) -> String {
    reply(client.post(format!("{base}/api/v1/socket-ticket")).bearer_auth(token)
        .header("content-type", "application/json").body(json!({"target":format!("/api/v1/terminal/{id}")}).to_string()), 200).await["ticket"].as_str().unwrap().into()
}

async fn message(socket: &mut Socket) -> Message {
    tokio::time::timeout(DEADLINE, socket.next()).await.unwrap().unwrap().unwrap()
}

async fn closed(socket: &mut Socket) {
    tokio::time::timeout(DEADLINE, async {
        loop { match socket.next().await { None | Some(Err(_)) | Some(Ok(Message::Close(_))) => return, _ => {} } }
    }).await.unwrap();
}

async fn control(socket: &mut Socket, value: Value, expected: &str) -> Value {
    socket.send(Message::Text(value.to_string().into())).await.unwrap();
    tokio::time::timeout(DEADLINE, async {
        loop {
            if let Message::Text(text) = message(socket).await {
                let value: Value = serde_json::from_str(&text).unwrap();
                if value["type"] == expected { return value; }
            }
        }
    }).await.unwrap()
}

async fn denied(url: String, status: u16) {
    let error = tokio::time::timeout(DEADLINE, connect_async(url)).await.unwrap().unwrap_err();
    assert!(matches!(error, tokio_tungstenite::tungstenite::Error::Http(ref r) if r.status().as_u16() == status), "{error}");
}

async fn attach(client: &reqwest::Client, base: &str, token: &str, target: &Value, after: Option<&str>) -> (Socket, Value) {
    let id = target["sessionId"].as_str().unwrap();
    let ticket = ticket(client, base, token, id).await;
    let mut url = format!("{}/api/v1/terminal/{id}?ticket={ticket}&daemonEpoch={}", base.replacen("http", "ws", 1), target["daemonEpoch"].as_str().unwrap());
    if let Some(after) = after { url.push_str(&format!("&afterSequence={after}")); }
    let (mut socket, _) = tokio::time::timeout(DEADLINE, connect_async(url)).await.unwrap().expect("machine socket must attach independently of mirror selection");
    let Message::Text(text) = message(&mut socket).await else { panic!("attached boundary must precede bytes") };
    let boundary: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(boundary["type"], "attached");
    assert_eq!(boundary["target"], *target);
    (socket, boundary)
}

#[tokio::test]
async fn machine_streams_use_original_owner_and_ignore_mirror_focus() {
    let (root, owner) = tokio::task::spawn_blocking(|| {
        let root = tempfile::tempdir().unwrap();
        for name in ["one", "two"] { std::fs::create_dir(root.path().join(name)).unwrap(); }
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        let mut devices = serde_json::Map::new();
        for (id, scope, permission) in [("owner", "machine", "control"), ("other", "machine", "control"), ("mirror", "mirror", "control"), ("view", "mirror", "view")] {
            devices.insert(id.into(), json!({"id":id,"name":id,"permission":permission,"accessScope":scope,"createdAt":now,"lastSeenAt":now}));
        }
        std::fs::write(root.path().join("auth"), json!({"devices":devices,"tokens":{"owner-token":"owner","other-token":"other","mirror-token":"mirror","view-token":"view"}}).to_string()).unwrap();
        let owner = DaemonServer::new_with_paths(Some(root.path().join("config")), Some(root.path().join("auth")));
        (root, owner)
    }).await.unwrap();
    let backend = owner.terminal_service().clone();
    let state = owner.remote_state().clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let router = create_remote_router(state.clone());
    let task = tokio::spawn(async move { axum::serve(listener, router).with_graceful_shutdown(async { let _ = stopped.await; }).await.unwrap(); });
    let result = std::panic::AssertUnwindSafe(async {
        let base = format!("http://{addr}");
        let client = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(40)).build().unwrap();
        let mut created = Vec::new();
        for name in ["one", "two"] {
            let project = reply(client.post(format!("{base}/api/v1/workspace/projects")).bearer_auth("owner-token")
                .body(json!({"requestId":uuid::Uuid::new_v4().to_string(),"repoPath":root.path().join(name)}).to_string()), 201).await;
            created.push(reply(client.post(format!("{base}/api/v1/sessions")).bearer_auth("owner-token")
                .body(json!({"requestId":uuid::Uuid::new_v4().to_string(),"workspaceId":project["workspaceId"],"worktree":null,"inheritFromSessionId":null,"cwdRelative":null,"cols":80,"rows":24,"startup":{"kind":"shell"}}).to_string()), 201).await);
        }
        let (mut one, first) = attach(&client, &base, "owner-token", &created[0]["target"], None).await;
        let (mut two, _) = attach(&client, &base, "owner-token", &created[1]["target"], None).await;
        for selection in ["mirror-one", "mirror-two", "mirror-three"] {
            state.set_active_selection(serde_json::from_value(json!({"sessionId":selection})).unwrap());
            for (index, socket) in [&mut one, &mut two].into_iter().enumerate() {
                socket.send(Message::Binary(b"printf '\\nA10_%s:%s:%s:END\\n' PROOF \"$$\" \"$PWD\"\r".to_vec().into())).await.unwrap();
                let proof = tokio::time::timeout(DEADLINE, async {
                    let mut bytes = Vec::new();
                    loop {
                        if let Message::Binary(frame) = message(socket).await {
                            let decoded = ferryx_lib::remote::terminal_wire::decode_frame(&frame).unwrap();
                            bytes.extend_from_slice(decoded.terminal_bytes);
                        }
                        let text = String::from_utf8_lossy(&bytes);
                        if let Some((_, tail)) = text.split_once("A10_PROOF:") {
                            if let Some((proof, _)) = tail.split_once(":END") { break proof.to_owned(); }
                        }
                    }
                }).await.unwrap();
                let (pid, cwd) = proof.split_once(':').unwrap();
                let id = created[index]["target"]["sessionId"].as_str().unwrap();
                assert_eq!(backend.get_session(id).unwrap().pid(), Some(pid.parse().unwrap()));
                assert_eq!(cwd, created[index]["cwd"].as_str().unwrap());
                eprintln!("A10 executed marker session={id} pid={pid} cwd={cwd} epoch={} selection={selection}", created[index]["target"]["daemonEpoch"]);
            }
        }
        let target = &created[0]["target"];
        let id = target["sessionId"].as_str().unwrap();
        let epoch = target["daemonEpoch"].as_str().unwrap();
        let pty = backend.get_session(id).unwrap();
        let original_pid = pty.pid();
        let ws = base.replacen("http", "ws", 1);
        for token in ["other-token", "mirror-token", "view-token"] {
            let t = ticket(&client, &base, token, id).await;
            denied(format!("{ws}/api/v1/terminal/{id}?ticket={t}&daemonEpoch={epoch}"), if token == "other-token" {409} else {403}).await;
        }
        let close = json!({"requestId":uuid::Uuid::new_v4().to_string(),"daemonEpoch":epoch});
        let conflict = reply(client.delete(format!("{base}/api/v1/sessions/{id}")).bearer_auth("other-token").body(close.to_string()), 409).await;
        assert_eq!(conflict["error"]["code"], "CONTROL_CONFLICT");
        for (suffix, status) in [("daemonEpoch=1",409), ("daemonEpoch=01",400), ("daemonEpoch=18446744073709551616",400), ("daemonEpoch=1&daemonEpoch=2",400), ("render=grid",400), ("afterSequence=01",400)] {
            let t = ticket(&client, &base, "owner-token", id).await;
            denied(format!("{ws}/api/v1/terminal/{id}?ticket={t}&{suffix}"), status).await;
        }
        let t = ticket(&client, &base, "owner-token", id).await;
        denied(format!("{ws}/api/v1/terminal/foreign::{id}?ticket={t}&daemonEpoch={epoch}"), 401).await;
        let (mut replacement, boundary) = attach(&client, &base, "owner-token", target, None).await;
        assert!(boundary["generation"].as_str().unwrap().parse::<u64>().unwrap() > first["generation"].as_str().unwrap().parse::<u64>().unwrap());
        let _ = one.send(Message::Binary(b"A10_STALE=executed\r".to_vec().into())).await;
        closed(&mut one).await;
        control(&mut replacement, json!({"type":"resize","generation":first["generation"],"cols":33,"rows":11}), "error").await;
        assert_eq!(pty.get_size(), (80,24));
        replacement.send(Message::Text(json!({"type":"resize","generation":boundary["generation"],"cols":101,"rows":31}).to_string().into())).await.unwrap();
        control(&mut replacement, json!({"type":"ping"}), "pong").await;
        assert_eq!(pty.get_size(), (101,31));
        let (_, mut raw) = backend.attach(id).unwrap();
        replacement.send(Message::Binary(b"printf '\\nA10_%s:%s:END\\n' FENCE \"${A10_STALE-unset}\"\r".to_vec().into())).await.unwrap();
        tokio::time::timeout(DEADLINE, async {
            let mut bytes = Vec::new();
            loop { bytes.extend(raw.recv().await.unwrap()); if String::from_utf8_lossy(&bytes).contains("A10_FENCE:unset:END") { break; } }
        }).await.unwrap();
        assert_eq!(pty.pid(), original_pid);
        let hub = backend.output_hub();
        let before = hub.publish(id, b"A10_BEFORE_CURSOR".to_vec()).unwrap().sequence;
        hub.publish(id, b"A10_REPLAY_SUFFIX".to_vec()).unwrap();
        let (mut replayed, replay_boundary) = attach(&client, &base, "owner-token", target, Some(&before.to_string())).await;
        closed(&mut replacement).await;
        assert!(replay_boundary["replayGap"].is_null());
        let Message::Binary(bytes) = message(&mut replayed).await else { panic!("replay suffix") };
        let decoded = ferryx_lib::remote::terminal_wire::decode_frame(&bytes).unwrap();
        assert!(decoded.terminal_bytes.windows(b"A10_REPLAY_SUFFIX".len()).any(|w| w == b"A10_REPLAY_SUFFIX"));
        assert!(!decoded.terminal_bytes.windows(b"A10_BEFORE_CURSOR".len()).any(|w| w == b"A10_BEFORE_CURSOR"));
        hub.publish_gap(id).unwrap();
        hub.publish(id, b"A10_AFTER_GAP".to_vec()).unwrap();
        let (mut gapped, gap_boundary) = attach(&client, &base, "owner-token", target, Some("0")).await;
        closed(&mut replayed).await;
        assert!(!gap_boundary["replayGap"].is_null());
        let Message::Binary(bytes) = message(&mut gapped).await else { panic!("gap replay") };
        let decoded = ferryx_lib::remote::terminal_wire::decode_frame(&bytes).unwrap();
        assert!(decoded.terminal_bytes.starts_with(b"\x1bc"));
        assert!(decoded.terminal_bytes.ends_with(b"A10_AFTER_GAP"));
        let end = hub.session_sequence_range(id).unwrap().1.unwrap();
        let (mut empty, empty_boundary) = attach(&client, &base, "owner-token", target, Some(&end.to_string())).await;
        closed(&mut gapped).await;
        assert!(empty_boundary["replayGap"].is_null());
        control(&mut empty, json!({"type":"ping"}), "pong").await;
        empty.send(Message::Text(" ".repeat(16385).into())).await.unwrap(); closed(&mut empty).await;
        assert_eq!(pty.get_size(), (101,31)); assert_eq!(pty.pid(), original_pid);
        let t = ticket(&client, &base, "other-token", id).await;
        denied(format!("{ws}/api/v1/terminal/{id}?ticket={t}&daemonEpoch={epoch}"), 409).await;
        let fresh = ticket(&client, &base, "owner-token", id).await;
        let url = format!("{ws}/api/v1/terminal/{id}?ticket={fresh}&daemonEpoch={epoch}");
        let (mut resumed, _) = connect_async(&url).await.unwrap(); message(&mut resumed).await;
        denied(url, 401).await;
        resumed.send(Message::Binary(vec![b'x';65537].into())).await.unwrap(); closed(&mut resumed).await;
        let (mut revoked, _) = attach(&client, &base, "owner-token", target, None).await;
        control(&mut revoked, json!({"type":"ping"}), "pong").await;
        state.auth_manager.revoke_device("owner");
        let _ = revoked.send(Message::Binary(b"A10_REVOKED=executed\r".to_vec().into())).await;
        closed(&mut revoked).await; closed(&mut two).await;
        assert_eq!(pty.pid(), original_pid);
        let (_, mut raw) = backend.attach(id).unwrap();
        backend.write_input(id, b"printf '\\nA10_%s:%s:END\\n' REVOKED \"${A10_REVOKED-unset}\"\r").unwrap();
        tokio::time::timeout(DEADLINE, async {
            let mut bytes = Vec::new();
            loop { bytes.extend(raw.recv().await.unwrap()); if String::from_utf8_lossy(&bytes).contains("A10_REVOKED:unset:END") { break; } }
        }).await.unwrap();
        eprintln!("A10 controller replacement/stale-input/stale-resize/conflict/HTTP-close-conflict/replay/gap/empty/ticket-reuse/64KiB+1/16KiB+1/revoked-input passed original_pid={original_pid:?} epoch={epoch}");
    }).catch_unwind().await;
    // Revocation cancels every upgrade before joining the listener, including on assertion failure.
    for id in ["owner", "other", "mirror", "view"] { state.auth_manager.revoke_device(id); }
    for id in backend.list_sessions() {
        let pty = backend.get_session(&id).unwrap();
        backend.close_session(&id).await.unwrap();
        assert!(pty.is_reaped());
        eprintln!("A10 cleanup reaped session={id} pid={:?}", pty.pid());
    }
    stop.send(()).unwrap();
    tokio::time::timeout(DEADLINE, task).await.unwrap().unwrap();
    assert!(tokio::net::TcpStream::connect(addr).await.is_err());
    drop(owner); drop(state);
    tokio::task::spawn_blocking(move || root.close().unwrap()).await.unwrap();
    eprintln!("A10 cleanup listener joined/refused; original PTYs reaped; private root removed");
    if let Err(panic) = result { std::panic::resume_unwind(panic); }
}
