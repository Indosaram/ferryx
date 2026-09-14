use ferryx_lib::{daemon::server::DaemonServer, remote::server::create_remote_router};
use futures_util::FutureExt;
use std::time::Duration;

async fn exists(path: &std::path::Path) -> bool {
    let path = path.to_owned();
    tokio::task::spawn_blocking(move || path.exists()).await.unwrap()
}

async fn owner_request(server: std::sync::Arc<DaemonServer>, request: serde_json::Value) -> serde_json::Value {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let (client, stream) = tokio::io::duplex(65536);
    let mut worker = tokio::spawn(server.handle_client(stream));
    let result = std::panic::AssertUnwindSafe(async {
        let (read, mut write) = tokio::io::split(client);
        write.write_all(format!("{request}\n").as_bytes()).await.unwrap();
        let mut reader = tokio::io::BufReader::new(read);
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line)).await.unwrap().unwrap();
        serde_json::from_str(&line).unwrap()
    }).catch_unwind().await;
    let joined = tokio::time::timeout(Duration::from_secs(10), &mut worker).await;
    if joined.is_err() { worker.abort(); let _ = worker.await; }
    joined.unwrap().unwrap();
    result.unwrap()
}

#[tokio::test]
async fn worktree_revision_survives_owner_restart() {
    let (root, repo, token) = tokio::task::spawn_blocking(|| {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        std::fs::create_dir(&repo).unwrap();
        for args in [vec!["init", "--quiet"], vec!["config", "user.name", "A08"], vec!["config", "user.email", "a08@example.invalid"], vec!["commit", "--allow-empty", "-m", "base"]] {
            ferryx_lib::worktree::run_git(&repo, &args).unwrap();
        }
        let token = uuid::Uuid::new_v4().to_string();
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        std::fs::write(root.path().join("auth"), serde_json::json!({"devices":{"a08":{"id":"a08","name":"A08","permission":"control","accessScope":"machine","createdAt":now,"lastSeenAt":now}},"tokens":{token.clone():"a08"}}).to_string()).unwrap();
        (root, repo, token)
    }).await.unwrap();
    let mut workspace = String::new();
    let mut revision = serde_json::Value::Null;
    let mut target = std::path::PathBuf::new();
    for phase in ["initial", "unchanged", "changed"] {
        let config = root.path().join("config"); let auth = root.path().join("auth");
        let owner = tokio::task::spawn_blocking(move || DaemonServer::new_with_paths(Some(config), Some(auth))).await.unwrap();
        let state = owner.remote_state().clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move { axum::serve(listener, create_remote_router(state)).with_graceful_shutdown(async { let _ = stopped.await; }).await.unwrap(); });
        let outcome = std::panic::AssertUnwindSafe(async {
            let client = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(40)).build().unwrap();
            let endpoint = format!("http://{addr}/api/v1/workspace/worktrees");
            if phase == "initial" {
                let response = client.post(format!("http://{addr}/api/v1/workspace/projects")).bearer_auth(&token).body(serde_json::json!({"requestId":uuid::Uuid::new_v4().to_string(),"repoPath":repo}).to_string()).send().await.unwrap();
                assert_eq!(response.status().as_u16(), 201);
                workspace = serde_json::from_str::<serde_json::Value>(&response.text().await.unwrap()).unwrap()["workspaceId"].as_str().unwrap().into();
                let response = client.post(&endpoint).bearer_auth(&token).body(serde_json::json!({"requestId":uuid::Uuid::new_v4().to_string(),"workspaceId":workspace,"worktree":{"wsId":workspace,"slug":"restart"}}).to_string()).send().await.unwrap();
                assert_eq!(response.status().as_u16(), 201);
                target = serde_json::from_str::<serde_json::Value>(&response.text().await.unwrap()).unwrap()["path"].as_str().unwrap().into();
                // No intervening observation after commit: the first external
                // change must differ from the baseline saved by that commit.
                let before = owner.remote_state().machine_services.as_ref().unwrap().workspaces.catalog().unwrap().revision;
                let path = target.clone();
                tokio::task::spawn_blocking(move || ferryx_lib::worktree::run_git(path, &["commit", "--allow-empty", "-m", "post-commit-external"]).unwrap()).await.unwrap();
                let response = client.get(format!("{endpoint}/status?workspaceId={workspace}&wsId={workspace}&slug=restart")).bearer_auth(&token).send().await.unwrap();
                assert_eq!(response.status().as_u16(), 200);
                revision = serde_json::from_str::<serde_json::Value>(&response.text().await.unwrap()).unwrap()["revision"].clone();
                assert!(revision.as_str().unwrap().parse::<u64>().unwrap() > before.0);
            } else if phase == "unchanged" {
                let response = client.get(format!("{endpoint}/status?workspaceId={workspace}&wsId={workspace}&slug=restart")).bearer_auth(&token).send().await.unwrap();
                assert_eq!(response.status().as_u16(), 200);
                assert_eq!(serde_json::from_str::<serde_json::Value>(&response.text().await.unwrap()).unwrap()["revision"], revision);
            } else {
                // DELETE is the first observation after restart: no GET repairs
                // the baseline before exercising the stale-preview guard.
                let response = client.delete(&endpoint).bearer_auth(&token).body(serde_json::json!({"requestId":uuid::Uuid::new_v4().to_string(),"workspaceId":workspace,"worktree":{"wsId":workspace,"slug":"restart"},"deleteBranch":false,"expectedRevision":revision}).to_string()).send().await.unwrap();
                assert_eq!(response.status().as_u16(), 409);
                assert_eq!(serde_json::from_str::<serde_json::Value>(&response.text().await.unwrap()).unwrap()["error"]["code"], "STALE_REVISION");
                assert!(exists(&target).await);
            }
        }).catch_unwind().await;
        let _ = stop.send(());
        task.await.unwrap();
        drop(owner);
        assert!(tokio::net::TcpStream::connect(addr).await.is_err());
        if let Err(panic) = outcome {
            tokio::task::spawn_blocking(move || root.close().unwrap()).await.unwrap();
            std::panic::resume_unwind(panic);
        }
        if phase == "unchanged" {
            let path = target.clone();
            tokio::task::spawn_blocking(move || {
                ferryx_lib::worktree::run_git(&path, &["commit", "--allow-empty", "-m", "offline-external"]).unwrap();
                assert!(ferryx_lib::worktree::run_git(path, &["status", "--porcelain"]).unwrap().is_empty());
            }).await.unwrap();
        }
        eprintln!("A08 revision phase={phase} listener_joined=true owner_dropped=true connection_refused=true");
    }
    tokio::task::spawn_blocking(move || root.close().unwrap()).await.unwrap();
    eprintln!("A08 revision post_commit_change=true unchanged_restart_equal=true offline_clean_change_stale_delete=true private_root_removed=true");
}

#[tokio::test]
async fn machine_worktree_http_contract() {
    let (root, server, token, repo) = tokio::task::spawn_blocking(|| {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join(if cfg!(windows) { "repo space 'quote' ü" } else { "repo space \"quote\" ü" });
        std::fs::create_dir(&repo).unwrap();
        for args in [vec!["init", "--quiet"], vec!["config", "user.name", "A08"], vec!["config", "user.email", "a08@example.invalid"], vec!["commit", "--allow-empty", "-m", "base"]] {
            ferryx_lib::worktree::run_git(&repo, &args).unwrap();
        }
        let token = uuid::Uuid::new_v4().to_string();
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        std::fs::create_dir(root.path().join("data")).unwrap();
        std::fs::write(root.path().join("data/auth"), serde_json::json!({"devices": {"a08": {"id":"a08", "name":"A08", "permission":"control", "accessScope":"machine", "createdAt":now, "lastSeenAt":now}}, "tokens": {token.clone(): "a08"}}).to_string()).unwrap();
        let server = DaemonServer::new_with_paths(Some(root.path().join("data/config")), Some(root.path().join("data/auth")));
        (root, server, token, repo)
    }).await.unwrap();
    let server = std::sync::Arc::new(server);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let state = server.remote_state().clone();
    let mut changes = state.machine_services.as_ref().unwrap().workspaces.subscribe_worktree_changes();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let mut gateway = tokio::spawn(async move {
        axum::serve(listener, create_remote_router(state)).with_graceful_shutdown(async { let _ = stopped.await; }).await.unwrap();
    });
    let mut owned_session: Option<String> = None;
    let mut restart_replay = None;
    let outcome = std::panic::AssertUnwindSafe(async {
        let client = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(40)).build().unwrap();
        let response = client.post(format!("http://{addr}/api/v1/workspace/projects")).bearer_auth(&token).header("content-type", "application/json").body(serde_json::json!({"requestId": uuid::Uuid::new_v4().to_string(), "repoPath": repo}).to_string()).send().await.unwrap();
        assert_eq!(response.status().as_u16(), 201);
        let project: serde_json::Value = serde_json::from_str(&response.text().await.unwrap()).unwrap();
        let response = client.get(format!("http://{addr}/api/v1/workspace/worktrees")).bearer_auth(&token).query(&[("workspaceId", project["workspaceId"].as_str().unwrap())]).send().await.unwrap();
        let status = response.status().as_u16();
        let body = response.text().await.unwrap();
        eprintln!("A08 HTTP GET worktrees status={status} body={body}");
        assert_eq!(status, 200, "machine list must expose rich canonical rows");
        let list: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(list["worktrees"][0]["workspaceId"], project["workspaceId"]);
        assert_eq!(list["worktrees"][0]["path"], project["repoRoot"]);
        assert!(list["worktrees"][0]["identity"].is_null());
        let ws = project["workspaceId"].as_str().unwrap();
        let endpoint = format!("http://{addr}/api/v1/workspace/worktrees");
        let missing = client.get(format!("{endpoint}/status?workspaceId={ws}&wsId={ws}&slug=absent")).bearer_auth(&token).send().await.unwrap();
        assert_eq!(missing.status().as_u16(), 404, "known missing worktree");
        let local = owner_request(server.clone(), serde_json::json!({"type":"createWorktree","workspaceId":ws,"worktree":{"wsId":ws,"slug":"owner-ipc"},"baseRef":"HEAD"})).await;
        assert_eq!(local["type"], "createWorktreeOk", "{local}");
        let local_change = tokio::time::timeout(Duration::from_secs(5), changes.recv()).await.unwrap().unwrap();
        assert_eq!(local_change.workspace_id, ws);
        let create = serde_json::json!({"requestId":uuid::Uuid::new_v4().to_string(), "workspaceId":ws, "worktree":{"wsId":ws,"slug":"feature"},"baseRef":"HEAD"});
        let response = client.post(&endpoint).bearer_auth(&token).header("content-type","application/json").body(create.to_string()).send().await.unwrap();
        let status = response.status().as_u16();
        let created = response.text().await.unwrap();
        assert_eq!(status, 201, "{created}");
        let row: serde_json::Value = serde_json::from_str(&created).unwrap();
        assert_eq!(row["managed"], true);
        assert_eq!(row["head"], list["worktrees"][0]["head"]);
        let change = tokio::time::timeout(Duration::from_secs(5), changes.recv()).await.unwrap().unwrap();
        assert_eq!(change.workspace_id, ws);
        assert!(!change.removed);
        assert_eq!(change.revision.0, local_change.revision.0 + 1);
        let replay = client.post(&endpoint).bearer_auth(&token).header("content-type","application/json").body(create.to_string()).send().await.unwrap();
        assert_eq!(replay.status().as_u16(), 201);
        assert_eq!(replay.text().await.unwrap(), created);
        let lost = serde_json::json!({"requestId":uuid::Uuid::new_v4().to_string(),"workspaceId":ws,"worktree":{"wsId":ws,"slug":"lost-reply"}});
        // One-shot TCP intermediary forwards the request, reads the owner's entire
        // committed HTTP response, then closes without forwarding any reply byte.
        let proxy = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_addr = proxy.local_addr().unwrap();
        let proxy_task = tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            tokio::time::timeout(Duration::from_secs(40), async {
                let (mut downstream, _) = proxy.accept().await.unwrap();
                let mut upstream = tokio::net::TcpStream::connect(addr).await.unwrap();
                let mut request = Vec::new();
                let header_end = loop {
                    let mut byte = [0]; downstream.read_exact(&mut byte).await.unwrap(); request.push(byte[0]);
                    assert!(request.len() <= 65536);
                    if request.ends_with(b"\r\n\r\n") { break request.len(); }
                };
                let header = String::from_utf8(request.clone()).unwrap();
                let length: usize = header.lines().find_map(|line| line.to_ascii_lowercase().strip_prefix("content-length:").map(|value| value.trim().parse().unwrap())).unwrap();
                assert!(length <= 65536);
                request.resize(header_end + length, 0);
                downstream.read_exact(&mut request[header_end..]).await.unwrap();
                upstream.write_all(&request).await.unwrap();
                let mut reply = Vec::new();
                upstream.take(1_048_577).read_to_end(&mut reply).await.unwrap();
                assert!(reply.len() <= 1_048_576);
                assert!(reply.starts_with(b"HTTP/1.1 201"));
                downstream.shutdown().await.unwrap();
                eprintln!("A08 LOST_REPLY owner_status=201 forwarded_response_bytes=0 intermediary_joined=true");
            }).await.unwrap();
        });
        let lost_result = client.post(format!("http://{proxy_addr}/api/v1/workspace/worktrees")).header("connection", "close").bearer_auth(&token).body(lost.to_string()).send().await;
        let proxy_join = proxy_task.await;
        proxy_join.unwrap();
        assert!(lost_result.is_err(), "intermediary must not deliver a response");
        let operation = client.get(format!("http://{addr}/api/v1/workspace/operations/{}", lost["requestId"].as_str().unwrap())).bearer_auth(&token).send().await.unwrap();
        assert_eq!(operation.status().as_u16(), 200);
        let operation: serde_json::Value = serde_json::from_str(&operation.text().await.unwrap()).unwrap();
        let replay = client.post(&endpoint).bearer_auth(&token).body(lost.to_string()).send().await.unwrap();
        assert_eq!(replay.status().as_u16(), 201);
        let replay: serde_json::Value = serde_json::from_str(&replay.text().await.unwrap()).unwrap();
        assert_eq!(operation["outcome"]["worktree"], replay);
        restart_replay = Some((lost.clone(), replay.clone()));
        for (owner, slug, base, expected) in [("wrong", "other", "HEAD", 400), (ws, "--force", "HEAD", 400), (ws, "bad-base", "--help", 400), (ws, "missing", "does-not-exist", 422)] {
            let response = client.post(&endpoint).bearer_auth(&token).body(serde_json::json!({"requestId":uuid::Uuid::new_v4().to_string(),"workspaceId":ws,"worktree":{"wsId":owner,"slug":slug},"baseRef":base}).to_string()).send().await.unwrap();
            let status = response.status().as_u16(); let body = response.text().await.unwrap();
            assert_eq!(status, expected, "{body}");
        }
        let path = std::path::PathBuf::from(row["path"].as_str().unwrap());
        let preview_url = format!("{endpoint}/status?workspaceId={ws}&wsId={ws}&slug=feature");
        let stale_request = serde_json::json!({"requestId":uuid::Uuid::new_v4().to_string(),"workspaceId":ws,"worktree":{"wsId":ws,"slug":"feature"},"deleteBranch":true,"expectedRevision":"1"});
        let stale = client.delete(&endpoint).bearer_auth(&token).body(stale_request.to_string()).send().await.unwrap();
        assert_eq!(stale.status().as_u16(), 409);
        let stale_body = stale.text().await.unwrap();
        let error: serde_json::Value = serde_json::from_str(&stale_body).unwrap();
        assert_eq!(error["error"]["code"], "STALE_REVISION");
        assert!(exists(&path).await);
        let replay = client.delete(&endpoint).bearer_auth(&token).body(stale_request.to_string()).send().await.unwrap();
        assert_eq!(replay.status().as_u16(), 409);
        assert_eq!(replay.text().await.unwrap(), stale_body);
        eprintln!("A08 STALE_REVISION status=409 target_preserved=true refusal_replayed=true");
        let auth = server.remote_state().auth_manager.clone();
        let mirror = tokio::task::spawn_blocking(move || {
            let pin = auth.create_pairing_code(ferryx_lib::remote::auth::DevicePermission::Control);
            auth.exchange_pairing_code(&pin, "mirror").unwrap().0
        }).await.unwrap();
        let denied = client.get(&endpoint).bearer_auth(&mirror).query(&[("workspaceId",ws)]).send().await.unwrap();
        assert_eq!(denied.status().as_u16(), 403);
        let denied = client.post(&endpoint).bearer_auth(&mirror).body(serde_json::json!({"workspaceId":ws,"worktree":{"wsId":ws,"slug":"mirror-intrusion"}}).to_string()).send().await.unwrap();
        assert_eq!(denied.status().as_u16(), 403);
        #[cfg(unix)] {
            let registered = owner_request(server.clone(), serde_json::json!({"type":"registerWorkspace","workspaceId":ws,"repoRoot":repo})).await;
            assert_eq!(registered["type"], "registerWorkspaceOk");
            let spawned = owner_request(server.clone(), serde_json::json!({"type":"spawn","clientRequestId":uuid::Uuid::new_v4().to_string(),"workspaceId":ws,"worktree":{"wsId":ws,"slug":"feature"},"cwd":null,"cols":80,"rows":24,"shell":null,"startup":null})).await;
            assert_eq!(spawned["type"], "spawnOk", "{spawned}");
            let session_id = spawned["sessionId"].as_str().unwrap().to_owned();
            owned_session = Some(session_id.clone());
            let (_, mut output) = server.terminal_service().attach(&session_id).unwrap();
            server.terminal_service().write_input(&session_id, b"printf '\\10108_OWNER pid=%s cwd=%s\\n' \"$$\" \"$PWD\"\n").unwrap();
            let bytes = tokio::time::timeout(Duration::from_secs(5), async {
                let mut bytes = Vec::new();
                loop {
                    bytes.extend(output.recv().await.unwrap());
                    let text = String::from_utf8_lossy(&bytes);
                    if text.contains("A08_OWNER pid=") && text.contains(&format!("cwd={}\r", path.display())) { break bytes; }
                }
            }).await.unwrap();
            let text = String::from_utf8_lossy(&bytes);
            let marker = text.split("A08_OWNER pid=").nth(1).unwrap().lines().next().unwrap();
            assert!(marker.contains(path.to_str().unwrap()), "{marker}");
            let shell_pid: libc::pid_t = marker.split_whitespace().next().unwrap().parse().unwrap();
            eprintln!("A08_OWNER pid={marker}");
            let preview = client.get(&preview_url).bearer_auth(&token).send().await.unwrap();
            let preview: serde_json::Value = serde_json::from_str(&preview.text().await.unwrap()).unwrap();
            assert_eq!(preview["liveSessionIds"], serde_json::json!([session_id]));
            let busy = client.delete(&endpoint).bearer_auth(&token).body(serde_json::json!({"requestId":uuid::Uuid::new_v4().to_string(),"workspaceId":ws,"worktree":{"wsId":ws,"slug":"feature"},"deleteBranch":false,"expectedRevision":preview["revision"]}).to_string()).send().await.unwrap();
            assert_eq!(busy.status().as_u16(), 409);
            let busy: serde_json::Value = serde_json::from_str(&busy.text().await.unwrap()).unwrap();
            assert_eq!(busy["error"]["code"], "WORKTREE_BUSY");
            let local_busy = owner_request(server.clone(), serde_json::json!({"type":"deleteWorktree","workspaceId":ws,"worktree":{"wsId":ws,"slug":"feature"},"deleteBranch":false})).await;
            assert_eq!(local_busy["type"], "worktreeError");
            assert_eq!(local_busy["error"]["code"], "WORKTREE_BUSY");
            assert_eq!(local_busy["error"]["details"]["path"], path.to_str().unwrap());
            assert_eq!(local_busy["error"]["details"]["liveSessionIds"], serde_json::json!([session_id]));
            assert_eq!(unsafe { libc::kill(shell_pid, 0) }, 0, "both refusals preserve original shell");
            let closed = owner_request(server.clone(), serde_json::json!({"type":"close","sessionId":session_id})).await;
            assert_eq!(closed["type"], "closeOk");
            owned_session = None;
            assert!(!server.terminal_service().pty_manager().has_session(&session_id));
            let mut status = 0;
            assert_eq!(unsafe { libc::waitpid(shell_pid, &mut status, libc::WNOHANG) }, -1);
            assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(libc::ECHILD));
            assert_eq!(unsafe { libc::kill(shell_pid, 0) }, -1);
            assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
            eprintln!("A08 OWNER close_complete=true session_removed=true pid={shell_pid} absent=true already_reaped=true (shared owner fixture, not A09)");
        }
        let initial_preview = client.get(&preview_url).bearer_auth(&token).send().await.unwrap();
        let initial_preview: serde_json::Value = serde_json::from_str(&initial_preview.text().await.unwrap()).unwrap();
        let mut observed_revision: u64 = initial_preview["revision"].as_str().unwrap().parse().unwrap();
        for dirty in [true, false] {
            let unusual_name = if cfg!(windows) { "space 'quote' ü.txt" } else { "space \"quote\" ü.txt" };
            let file = path.join(unusual_name);
            tokio::task::spawn_blocking(move || if dirty { std::fs::write(file,"dirty") } else { std::fs::remove_file(file) }).await.unwrap().unwrap();
            let preview = client.get(&preview_url).bearer_auth(&token).send().await.unwrap();
            assert_eq!(preview.status().as_u16(), 200);
            let preview: serde_json::Value = serde_json::from_str(&preview.text().await.unwrap()).unwrap();
            assert_eq!(preview["dirtyCount"], if dirty {1} else {0});
            if dirty { assert_eq!(preview["dirtyFiles"][0]["path"], unusual_name); }
            let revision: u64 = preview["revision"].as_str().unwrap().parse().unwrap();
            assert!(revision > observed_revision, "external dirty-state change must advance the durable revision");
            observed_revision = revision;
            let response = client.delete(&endpoint).bearer_auth(&token).body(serde_json::json!({"requestId":uuid::Uuid::new_v4().to_string(),"workspaceId":ws,"worktree":{"wsId":ws,"slug":"feature"},"deleteBranch":false,"expectedRevision":preview["revision"]}).to_string()).send().await.unwrap();
            let status = response.status().as_u16(); let body = response.text().await.unwrap();
            assert_eq!(status, if dirty {409} else {204}, "{body}");
        }
        let retained_repo = repo.clone(); let branch = format!("orca/{ws}/feature");
        assert!(!exists(&path).await);
        tokio::task::spawn_blocking(move || ferryx_lib::worktree::run_git(retained_repo, &["show-ref","--verify",&format!("refs/heads/{branch}")]).unwrap()).await.unwrap();
        for scenario in ["locked", "unmerged", "partial", "remove-branch"] {
            let response = client.post(&endpoint).bearer_auth(&token).body(serde_json::json!({"requestId":uuid::Uuid::new_v4().to_string(),"workspaceId":ws,"worktree":{"wsId":ws,"slug":scenario}}).to_string()).send().await.unwrap();
            assert_eq!(response.status().as_u16(), 201);
            let row: serde_json::Value = serde_json::from_str(&response.text().await.unwrap()).unwrap();
            let path = std::path::PathBuf::from(row["path"].as_str().unwrap());
            let fixture_path = path.clone(); let fixture_repo = repo.clone();
            let branch = format!("orca/{ws}/{scenario}"); let fixture_branch = branch.clone();
            tokio::task::spawn_blocking(move || {
                if scenario == "locked" { ferryx_lib::worktree::run_git(&fixture_repo, &["worktree","lock",fixture_path.to_str().unwrap()]).unwrap(); }
                if scenario == "unmerged" { ferryx_lib::worktree::run_git(&fixture_path, &["commit","--allow-empty","-m","unmerged"]).unwrap(); }
                if scenario == "partial" {
                    let lock = fixture_repo.join(".git/refs/heads").join(format!("{fixture_branch}.lock"));
                    std::fs::write(lock, "fixture owns branch lock").unwrap();
                }
            }).await.unwrap();
            let preview = client.get(format!("{endpoint}/status?workspaceId={ws}&wsId={ws}&slug={scenario}")).bearer_auth(&token).send().await.unwrap();
            assert_eq!(preview.status().as_u16(), 200);
            let preview: serde_json::Value = serde_json::from_str(&preview.text().await.unwrap()).unwrap();
            let payload = serde_json::json!({"requestId":uuid::Uuid::new_v4().to_string(),"workspaceId":ws,"worktree":{"wsId":ws,"slug":scenario},"deleteBranch":true,"expectedRevision":preview["revision"]});
            let response = client.delete(&endpoint).bearer_auth(&token).body(payload.to_string()).send().await.unwrap();
            let status = response.status().as_u16(); let body = response.text().await.unwrap();
            eprintln!("A08 delete scenario={scenario} status={status} body={body}");
            assert_eq!(status, if scenario == "remove-branch" {204} else {409}, "{body}");
            if scenario == "partial" {
                let error: serde_json::Value = serde_json::from_str(&body).unwrap();
                assert_eq!(error["error"]["details"]["worktreeRemoved"], true);
                let retry = client.delete(&endpoint).bearer_auth(&token).body(payload.to_string()).send().await.unwrap();
                assert_eq!(retry.status().as_u16(), 409);
                assert_eq!(retry.text().await.unwrap(), body);
                assert!(!exists(&path).await);
            }
            if scenario == "remove-branch" {
                assert!(!exists(&path).await);
                let repo = repo.clone();
                assert!(tokio::task::spawn_blocking(move || ferryx_lib::worktree::run_git(repo, &["show-ref","--verify",&format!("refs/heads/{branch}")])).await.unwrap().is_err());
            }
        }
        #[cfg(unix)] {
            let slug = "replacement";
            let response = client.post(&endpoint).bearer_auth(&token).body(serde_json::json!({"requestId":uuid::Uuid::new_v4().to_string(),"workspaceId":ws,"worktree":{"wsId":ws,"slug":slug}}).to_string()).send().await.unwrap();
            assert_eq!(response.status().as_u16(), 201);
            let row: serde_json::Value = serde_json::from_str(&response.text().await.unwrap()).unwrap();
            let path = std::path::PathBuf::from(row["path"].as_str().unwrap());
            let original = path.with_extension("original");
            let outside = root.path().join("outside");
            let (path, original, outside) = tokio::task::spawn_blocking(move || {
                std::fs::create_dir(&outside).unwrap();
                std::fs::write(outside.join("sentinel"), "untouched").unwrap();
                std::fs::rename(&path, &original).unwrap();
                std::os::unix::fs::symlink(&outside, &path).unwrap();
                (path, original, outside)
            }).await.unwrap();
            let inventory = client.get(&endpoint).bearer_auth(&token).query(&[("workspaceId",ws)]).send().await.unwrap();
            let inventory: serde_json::Value = serde_json::from_str(&inventory.text().await.unwrap()).unwrap();
            let response = client.delete(&endpoint).bearer_auth(&token).body(serde_json::json!({"requestId":uuid::Uuid::new_v4().to_string(),"workspaceId":ws,"worktree":{"wsId":ws,"slug":slug},"deleteBranch":false,"expectedRevision":inventory["revision"]}).to_string()).send().await.unwrap();
            assert_eq!(response.status().as_u16(), 400);
            tokio::task::spawn_blocking(move || {
                assert_eq!(std::fs::read_to_string(outside.join("sentinel")).unwrap(), "untouched");
                std::fs::remove_file(&path).unwrap(); std::fs::rename(&original, &path).unwrap();
            }).await.unwrap();
            let response = client.delete(&endpoint).bearer_auth(&token).body(serde_json::json!({"requestId":uuid::Uuid::new_v4().to_string(),"workspaceId":ws,"worktree":{"wsId":ws,"slug":"../../.."},"deleteBranch":false,"expectedRevision":inventory["revision"]}).to_string()).send().await.unwrap();
            assert_eq!(response.status().as_u16(), 400);
            assert!(exists(&repo).await);
        }
        let mut changed = create.clone(); changed["baseRef"] = serde_json::json!("main");
        let response = client.post(&endpoint).bearer_auth(&token).body(changed.to_string()).send().await.unwrap();
        assert_eq!(response.status().as_u16(), 409);
        let body: serde_json::Value = serde_json::from_str(&response.text().await.unwrap()).unwrap();
        assert_eq!(body["error"]["code"], "REQUEST_CONFLICT");
        let service = server.remote_state().machine_services.as_ref().unwrap().workspaces.clone();
        let exposed_repo = repo.clone(); let exposed_ws = ws.to_owned();
        tokio::task::spawn_blocking(move || service.register(&exposed_ws, exposed_repo.to_str().unwrap()).unwrap()).await.unwrap();
        let legacy = client.post(&endpoint).bearer_auth(&mirror).body(serde_json::json!({"workspaceId":ws,"worktree":{"wsId":ws,"slug":"mirror-compatible"}}).to_string()).send().await.unwrap();
        assert_eq!(legacy.status().as_u16(), 200);
        let legacy: serde_json::Value = serde_json::from_str(&legacy.text().await.unwrap()).unwrap();
        assert_eq!(legacy["worktreeSlug"], "mirror-compatible");
        assert!(legacy.get("path").is_none());
        assert!(legacy.get("head").is_none());
        let lock = repo.join(".git/refs/heads").join(format!("orca/{ws}/mirror-compatible.lock"));
        tokio::task::spawn_blocking(move || std::fs::write(lock, "branch lock").unwrap()).await.unwrap();
        let mut removal = server.remote_state().machine_services.as_ref().unwrap().workspaces.subscribe_worktree_changes();
        let partial = client.delete(&endpoint).bearer_auth(&mirror).body(serde_json::json!({"workspaceId":ws,"worktree":{"wsId":ws,"slug":"mirror-compatible"},"deleteBranch":true}).to_string()).send().await.unwrap();
        assert_eq!(partial.status().as_u16(), 409);
        let partial: serde_json::Value = serde_json::from_str(&partial.text().await.unwrap()).unwrap();
        assert_eq!(partial["error"]["details"]["worktreeRemoved"], true);
        assert!(tokio::time::timeout(Duration::from_secs(5), removal.recv()).await.unwrap().unwrap().removed);
        for git in [false, true] {
            let folder = root.path().join(if git {"unborn"} else {"plain"});
            let setup = folder.clone();
            tokio::task::spawn_blocking(move || {
                std::fs::create_dir(&setup).unwrap();
                if git { ferryx_lib::worktree::run_git(setup, &["init","--quiet"]).unwrap(); }
            }).await.unwrap();
            let response = client.post(format!("http://{addr}/api/v1/workspace/projects")).bearer_auth(&token).body(serde_json::json!({"requestId":uuid::Uuid::new_v4().to_string(),"repoPath":folder}).to_string()).send().await.unwrap();
            assert_eq!(response.status().as_u16(), 201);
            let project: serde_json::Value = serde_json::from_str(&response.text().await.unwrap()).unwrap();
            let ws = &project["workspaceId"];
            let response = client.post(&endpoint).bearer_auth(&token).body(serde_json::json!({"requestId":uuid::Uuid::new_v4().to_string(),"workspaceId":ws,"worktree":{"wsId":ws,"slug":"no-partial"}}).to_string()).send().await.unwrap();
            assert_eq!(response.status().as_u16(), 422);
            let error: serde_json::Value = serde_json::from_str(&response.text().await.unwrap()).unwrap();
            assert_eq!(error["error"]["code"], if git {"BASE_REF_UNAVAILABLE"} else {"NOT_A_GIT_REPOSITORY"});
            assert!(!exists(&folder.join(".orca-worktrees")).await);
        }
        let external = root.path().join(if cfg!(windows) { "external space 'quote'" } else { "external space \"quote\"" });
        let target = external.clone(); let git_root = repo.clone();
        tokio::task::spawn_blocking(move || ferryx_lib::worktree::run_git(git_root, &["worktree", "add", "--detach", target.to_str().unwrap(), "HEAD"]).unwrap()).await.unwrap();
        let inventory = client.get(&endpoint).bearer_auth(&token).query(&[("workspaceId",ws)]).send().await.unwrap();
        assert_eq!(inventory.status().as_u16(), 200);
        let inventory: serde_json::Value = serde_json::from_str(&inventory.text().await.unwrap()).unwrap();
        let canonical = { let external = external.clone(); tokio::task::spawn_blocking(move || std::fs::canonicalize(external).unwrap()).await.unwrap() };
        let row = inventory["worktrees"].as_array().unwrap().iter().find(|row| row["path"].as_str() == canonical.to_str()).unwrap();
        assert_eq!(row["managed"], false);
        assert!(row["identity"].is_null());
        tokio::task::spawn_blocking(move || std::fs::remove_dir_all(external).unwrap()).await.unwrap();
        let inventory = client.get(&endpoint).bearer_auth(&token).query(&[("workspaceId",ws)]).send().await.unwrap();
        assert_eq!(inventory.status().as_u16(), 200);
        let inventory: serde_json::Value = serde_json::from_str(&inventory.text().await.unwrap()).unwrap();
        let row = inventory["worktrees"].as_array().unwrap().iter().find(|row| row["path"].as_str() == canonical.to_str()).unwrap();
        assert!(row["prunable"].is_string());
    }).catch_unwind().await;
    if let Some(session) = owned_session { server.terminal_service().close_session(&session).await.unwrap(); }
    let _ = stop.send(());
    let joined = tokio::time::timeout(Duration::from_secs(45), &mut gateway).await;
    if joined.is_err() { gateway.abort(); let _ = gateway.await; }
    drop(server);
    if outcome.is_ok() {
        let (request, expected) = restart_replay.expect("committed creation");
        let config = root.path().join("data/config");
        let auth = root.path().join("data/auth");
        let owner = tokio::task::spawn_blocking(move || DaemonServer::new_with_paths(Some(config), Some(auth))).await.unwrap();
        let state = owner.remote_state().clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let mut listener_task = tokio::spawn(async move {
            axum::serve(listener, create_remote_router(state)).with_graceful_shutdown(async { let _ = stopped.await; }).await.unwrap();
        });
        let replay_outcome = std::panic::AssertUnwindSafe(async {
            let client = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(40)).build().unwrap();
            let response = client.post(format!("http://{addr}/api/v1/workspace/worktrees")).bearer_auth(&token).body(request.to_string()).send().await.unwrap();
            assert_eq!(response.status().as_u16(), 201);
            let replay: serde_json::Value = serde_json::from_str(&response.text().await.unwrap()).unwrap();
            assert_eq!(replay, expected);
            let inventory = client.get(format!("http://{addr}/api/v1/workspace/worktrees")).bearer_auth(&token).query(&[("workspaceId",request["workspaceId"].as_str().unwrap())]).send().await.unwrap();
            assert_eq!(inventory.status().as_u16(), 200);
            let inventory: serde_json::Value = serde_json::from_str(&inventory.text().await.unwrap()).unwrap();
            assert_eq!(inventory["worktrees"].as_array().unwrap().iter().filter(|row| row["path"] == expected["path"]).count(), 1);
            eprintln!("A08 RESTART same_request_same_row=true managed_target_count=1");
        }).catch_unwind().await;
        let _ = stop.send(());
        let joined = tokio::time::timeout(Duration::from_secs(45), &mut listener_task).await;
        if joined.is_err() { listener_task.abort(); let _ = listener_task.await; }
        drop(owner);
        if let Err(panic) = replay_outcome {
            tokio::task::spawn_blocking(move || root.close().unwrap()).await.unwrap();
            std::panic::resume_unwind(panic);
        }
        joined.unwrap().unwrap();
    }
    let receipt = root.path().to_owned();
    tokio::task::spawn_blocking(move || root.close().unwrap()).await.unwrap();
    eprintln!("A08 CLEANUP listener_joined={} private_root_removed={}", joined.is_ok(), !exists(&receipt).await);
    joined.unwrap().unwrap();
    if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
}
