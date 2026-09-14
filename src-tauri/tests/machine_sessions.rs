use ferryx_lib::{daemon::server::DaemonServer, remote::server::create_remote_router};
use futures_util::FutureExt;
use std::time::Duration;
use serde_json::{json, Value};

async fn response(request: reqwest::RequestBuilder, expected: u16) -> Value {
    let response = request.send().await.unwrap();
    let status = response.status().as_u16();
    let body = response.text().await.unwrap();
    assert_eq!(status, expected, "HTTP body: {body}");
    if body.is_empty() { Value::Null } else { serde_json::from_str(&body).unwrap() }
}

async fn shell_proof(backend: &ferryx_lib::terminal::TerminalService, id: &str) -> (u32, String) {
    let (_, mut output) = backend.attach(id).unwrap();
    // The output sentinel is assembled by printf, so echoed input cannot satisfy it.
    backend.write_input(id, b"printf '\\nA09_%s:%s:%s:END\\nA09_%s:%s:END\\n' PROOF \"$$\" \"$PWD\" SHELL \"$0\"\r").unwrap();
    let proof = tokio::time::timeout(Duration::from_secs(10), async {
        let mut bytes = Vec::new();
        loop {
            bytes.extend(output.recv().await.unwrap());
            let text = String::from_utf8_lossy(&bytes);
            if let Some((_, tail)) = text.split_once("A09_PROOF:") {
                if let Some((proof, _)) = tail.split_once(":END") {
                    let (pid, cwd) = proof.split_once(':').unwrap();
                    let Some((_, shell)) = text.split_once("A09_SHELL:") else { continue; };
                    let Some((shell, _)) = shell.split_once(":END") else { continue; };
                    let plan = ferryx_lib::terminal::shell::resolve_shell_command_pure(None,
                        ferryx_lib::terminal::shell::TargetPlatform::CURRENT, |_| false, |key| std::env::var(key).ok());
                    assert_eq!(std::path::Path::new(shell.trim_start_matches('-')).file_name(), std::path::Path::new(&plan.program).file_name());
                    eprintln!("A09 native shell resolver session={id} shell={shell}");
                    return (pid.parse::<u32>().unwrap(), cwd.to_owned());
                }
            }
        }
    }).await.unwrap();
    assert_eq!(backend.get_session(id).unwrap().pid(), Some(proof.0));
    eprintln!("A09 shell session={id} pid={} cwd={}", proof.0, proof.1);
    proof
}

#[tokio::test]
async fn machine_session_creation_uses_owner() {
    let (root, owner, token, repo) = tokio::task::spawn_blocking(|| {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("project");
        std::fs::create_dir(&repo).unwrap();
        std::fs::create_dir(repo.join("nested")).unwrap();
        std::fs::create_dir(root.path().join("second")).unwrap();
        for args in [vec!["init", "--quiet", "-b", "main"], vec!["-c", "user.name=A09", "-c", "user.email=a09@example.invalid", "commit", "--quiet", "--allow-empty", "-m", "fixture"]] {
            let output = std::process::Command::new("git").arg("-C").arg(root.path().join("second")).args(args).output().unwrap();
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.path().join("second"), repo.join("escape")).unwrap();
        let token = uuid::Uuid::new_v4().to_string();
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        std::fs::write(root.path().join("auth"), json!({"devices":{
            "a09":{"id":"a09","name":"A09","permission":"control","accessScope":"machine","createdAt":now,"lastSeenAt":now},
            "other":{"id":"other","name":"Other","permission":"control","accessScope":"machine","createdAt":now,"lastSeenAt":now},
            "mirror":{"id":"mirror","name":"Mirror","permission":"control","accessScope":"mirror","createdAt":now,"lastSeenAt":now}},
            "tokens":{token.clone():"a09","other-token":"other","mirror-token":"mirror"}}).to_string()).unwrap();
        let owner = DaemonServer::new_with_paths(Some(root.path().join("config")), Some(root.path().join("auth")));
        (root, owner, token, repo)
    }).await.unwrap();
    let state = owner.remote_state().clone();
    let backend = owner.terminal_service().clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move { axum::serve(listener, create_remote_router(state)).with_graceful_shutdown(async { let _ = stopped.await; }).await.unwrap(); });
    let result = std::panic::AssertUnwindSafe(async {
        let client = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(40)).build().unwrap();
        let project_response = client.post(format!("http://{addr}/api/v1/workspace/projects")).bearer_auth(&token).body(serde_json::json!({"requestId":uuid::Uuid::new_v4().to_string(),"repoPath":repo}).to_string()).send().await.unwrap();
        assert_eq!(project_response.status().as_u16(), 201);
        let project: serde_json::Value = serde_json::from_str(&project_response.text().await.unwrap()).unwrap();
        let request = serde_json::json!({"requestId":uuid::Uuid::new_v4().to_string(),"workspaceId":project["workspaceId"],"worktree":null,"cols":80,"rows":24,"inheritFromSessionId":null,"cwdRelative":null,"startup":{"kind":"shell"}});
        let creation_response = client.post(format!("http://{addr}/api/v1/sessions")).bearer_auth(&token).body(request.to_string()).send().await.unwrap();
        let status = creation_response.status().as_u16();
        let body = creation_response.text().await.unwrap();
        assert_eq!(status, 201, "machine creation must use the actual owner: {body}");
        let created: Value = serde_json::from_str(&body).unwrap();
        let id = created["target"]["sessionId"].as_str().unwrap();
        let original = shell_proof(&backend, id).await;
        assert_eq!(std::path::PathBuf::from(&original.1), std::fs::canonicalize(&repo).unwrap());
        assert_eq!(created["cwd"], original.1);
        let epoch = owner.remote_state().daemon_epoch.load(std::sync::atomic::Ordering::Acquire).to_string();
        assert_eq!(created["target"]["daemonEpoch"], epoch);
        let sessions_url = format!("http://{addr}/api/v1/sessions");
        let replay = response(client.post(&sessions_url).bearer_auth(&token).body(request.to_string()), 201).await;
        assert_eq!(replay, created);
        assert_eq!(shell_proof(&backend, id).await, original);
        assert_eq!(backend.list_sessions().len(), 1);
        let mut changed = request.clone(); changed["cols"] = json!(81);
        response(client.post(&sessions_url).bearer_auth(&token).body(changed.to_string()), 409).await;
        let operation = response(client.get(format!("http://{addr}/api/v1/workspace/operations/{}", request["requestId"].as_str().unwrap())).bearer_auth(&token), 200).await;
        assert_eq!(operation["state"], "completed");
        assert_eq!(operation["outcome"]["session"]["target"], created["target"]);
        let second = response(client.post(format!("http://{addr}/api/v1/workspace/projects")).bearer_auth(&token)
            .body(json!({"requestId":uuid::Uuid::new_v4().to_string(),"repoPath":root.path().join("second")}).to_string()), 201).await;
        let mut second_request = request.clone(); second_request["requestId"] = json!(uuid::Uuid::new_v4().to_string()); second_request["workspaceId"] = second["workspaceId"].clone();
        let created_second = response(client.post(&sessions_url).bearer_auth(&token).body(second_request.to_string()), 201).await;
        let second_id = created_second["target"]["sessionId"].as_str().unwrap();
        let second_proof = shell_proof(&backend, second_id).await;
        assert_ne!(original.0, second_proof.0);
        assert_eq!(std::path::PathBuf::from(&second_proof.1), std::fs::canonicalize(root.path().join("second")).unwrap());
        let listed = response(client.get(&sessions_url).bearer_auth(&token), 200).await;
        assert_eq!(listed["completeness"], "complete");
        assert_eq!(listed["sessions"].as_array().unwrap().len(), 2);
        let identity = json!({"wsId":second["workspaceId"],"slug":"a09"});
        let worktree = response(client.post(format!("http://{addr}/api/v1/workspace/worktrees")).bearer_auth(&token)
            .body(json!({"requestId":uuid::Uuid::new_v4().to_string(),"workspaceId":second["workspaceId"],"worktree":identity}).to_string()), 201).await;
        let mut in_worktree = second_request.clone(); in_worktree["requestId"] = json!(uuid::Uuid::new_v4().to_string()); in_worktree["worktree"] = identity;
        let managed = response(client.post(&sessions_url).bearer_auth(&token).body(in_worktree.to_string()), 201).await;
        let managed_id = managed["target"]["sessionId"].as_str().unwrap();
        assert_eq!(shell_proof(&backend, managed_id).await.1, worktree["path"].as_str().unwrap());
        let mut mismatched_parent = second_request.clone(); mismatched_parent["requestId"] = json!(uuid::Uuid::new_v4().to_string()); mismatched_parent["inheritFromSessionId"] = json!(managed_id);
        response(client.post(&sessions_url).bearer_auth(&token).body(mismatched_parent.to_string()), 409).await;
        in_worktree["requestId"] = json!(uuid::Uuid::new_v4().to_string()); in_worktree["worktree"]["slug"] = json!("missing");
        response(client.post(&sessions_url).bearer_auth(&token).body(in_worktree.to_string()), 404).await;
        response(client.delete(format!("{sessions_url}/{managed_id}")).bearer_auth(&token)
            .body(json!({"requestId":uuid::Uuid::new_v4().to_string(),"daemonEpoch":epoch}).to_string()), 204).await;
        let mirror = response(client.get(&sessions_url).bearer_auth("mirror-token"), 200).await;
        assert_eq!(mirror, json!([]));
        owner.remote_state().set_active_selection(serde_json::from_value(json!({"sessionId":id,"workspaceId":project["workspaceId"]})).unwrap());
        let mirror_state = response(client.get(format!("http://{addr}/api/v1/workspace/state")).bearer_auth("mirror-token"), 200).await;
        assert!(mirror_state["activeContext"]["sessionId"].is_null());
        assert_eq!(mirror_state["sessions"], json!([]));
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;
        let mut socket = format!("ws://{addr}/api/v1/terminal/{id}").into_client_request().unwrap();
        socket.headers_mut().insert("authorization", "Bearer mirror-token".parse().unwrap());
        let denied = tokio_tungstenite::connect_async(socket).await.unwrap_err();
        assert!(matches!(denied, tokio_tungstenite::tungstenite::Error::Http(ref response) if response.status().as_u16() == 403));
        owner.remote_state().clear_active_selection();
        response(client.post(&sessions_url).bearer_auth("mirror-token").body(request.to_string()), 403).await;
        response(client.get(format!("{sessions_url}/{id}")).bearer_auth("mirror-token"), 403).await;
        response(client.post(&sessions_url).body(request.to_string()), 401).await;
        response(client.post(&sessions_url).bearer_auth(&token).body("x".repeat(65537)), 413).await;
        for (field, value) in [("shell", json!("/bin/sh")), ("env", json!({"PATH":"/tmp"})), ("cwd", json!("/tmp"))] {
            let mut invalid = request.clone(); invalid[field] = value;
            response(client.post(&sessions_url).bearer_auth(&token).body(invalid.to_string()), 400).await;
        }
        for cwd in ["../second", "/tmp", "escape"] {
            let mut invalid = request.clone(); invalid["requestId"] = json!(uuid::Uuid::new_v4().to_string()); invalid["cwdRelative"] = json!(cwd);
            response(client.post(&sessions_url).bearer_auth(&token).body(invalid.to_string()), 400).await;
        }
        let mut invalid = second_request.clone(); invalid["requestId"] = json!(uuid::Uuid::new_v4().to_string()); invalid["inheritFromSessionId"] = json!(id);
        response(client.post(&sessions_url).bearer_auth(&token).body(invalid.to_string()), 409).await;
        for startup in [json!({"kind":"remoteSsh","hostStorePath":"/tmp"}), json!({"kind":"shell","command":"touch /tmp/injected"})] {
            let mut invalid = request.clone(); invalid["startup"] = startup;
            response(client.post(&sessions_url).bearer_auth(&token).body(invalid.to_string()), 400).await;
        }
        let mut unsupported = request.clone(); unsupported["requestId"] = json!(uuid::Uuid::new_v4().to_string());
        unsupported["startup"] = json!({"kind":"agentResume","agentType":"not-a-provider","providerSession":{"key":"session_id","id":"existing"}});
        response(client.post(&sessions_url).bearer_auth(&token).body(unsupported.to_string()), 422).await;
        let mut nested = request.clone(); nested["requestId"] = json!(uuid::Uuid::new_v4().to_string()); nested["cwdRelative"] = json!("nested");
        let child = response(client.post(&sessions_url).bearer_auth(&token).body(nested.to_string()), 201).await;
        let child_id = child["target"]["sessionId"].as_str().unwrap();
        let child_proof = shell_proof(&backend, child_id).await;
        assert_eq!(std::path::PathBuf::from(&child_proof.1), std::fs::canonicalize(repo.join("nested")).unwrap());
        nested["requestId"] = json!(uuid::Uuid::new_v4().to_string()); nested["cwdRelative"] = Value::Null; nested["inheritFromSessionId"] = json!(child_id);
        let inherited = response(client.post(&sessions_url).bearer_auth(&token).body(nested.to_string()), 201).await;
        let inherited_id = inherited["target"]["sessionId"].as_str().unwrap();
        assert_eq!(shell_proof(&backend, inherited_id).await.1, child_proof.1);
        for close_id in [child_id, inherited_id] {
            response(client.delete(format!("{sessions_url}/{close_id}")).bearer_auth(&token)
                .body(json!({"requestId":uuid::Uuid::new_v4().to_string(),"daemonEpoch":epoch}).to_string()), 204).await;
        }
        let close = json!({"requestId":uuid::Uuid::new_v4().to_string(),"daemonEpoch":epoch});
        let mut stale = close.clone(); stale["daemonEpoch"] = json!("1");
        response(client.delete(format!("{sessions_url}/{id}")).bearer_auth(&token).body(stale.to_string()), 409).await;
        response(client.delete(format!("{sessions_url}/{id}")).bearer_auth("other-token").body(close.to_string()), 409).await;
        let pty = backend.get_session(id).unwrap();
        response(client.delete(format!("{sessions_url}/{id}")).bearer_auth(&token).body(close.to_string()), 204).await;
        assert!(pty.is_reaped());
        assert!(backend.get_session(id).is_none());
        response(client.delete(format!("{sessions_url}/{id}")).bearer_auth(&token).body(close.to_string()), 204).await;
        let detail = response(client.get(format!("{sessions_url}/{id}")).bearer_auth(&token), 200).await;
        assert_eq!(detail["status"], "exited");
        assert_eq!(detail["session"]["target"], created["target"]);
        assert_eq!(shell_proof(&backend, second_id).await, second_proof);
        response(client.delete(format!("{sessions_url}/{second_id}")).bearer_auth(&token)
            .body(json!({"requestId":uuid::Uuid::new_v4().to_string(),"daemonEpoch":epoch}).to_string()), 204).await;
        eprintln!("A09 HTTP replay=same-target same-PID=true changed-digest=409 two-roots=true close=reaped epoch={epoch}");
    }).catch_unwind().await;
    for id in backend.list_sessions() { backend.close_session(&id).await.unwrap(); }
    let _ = stop.send(());
    task.await.unwrap();
    drop(owner);
    assert!(tokio::net::TcpStream::connect(addr).await.is_err());
    tokio::task::spawn_blocking(move || root.close().unwrap()).await.unwrap();
    eprintln!("A09 cleanup: private listener joined; owned sessions closed; private root removed");
    if let Err(panic) = result { std::panic::resume_unwind(panic); }
}
