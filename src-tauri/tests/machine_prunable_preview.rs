use ferryx_lib::{daemon::server::DaemonServer, remote::server::create_remote_router, worktree::run_git};
use futures_util::FutureExt;
use serde_json::{json, Value};
use std::time::Duration;

#[tokio::test]
async fn missing_checkout_rich_preview_http() {
    let (root, owner, repo, token) = tokio::task::spawn_blocking(|| {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        std::fs::create_dir(&repo).unwrap();
        for args in [vec!["init", "--quiet"], vec!["config", "user.name", "A08"], vec!["config", "user.email", "a08@example.invalid"], vec!["commit", "--allow-empty", "-m", "base"]] { run_git(&repo, &args).unwrap(); }
        let token = uuid::Uuid::new_v4().to_string();
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        std::fs::write(root.path().join("auth"), json!({"devices":{"a08":{"id":"a08","name":"A08","permission":"control","accessScope":"machine","createdAt":now,"lastSeenAt":now}},"tokens":{token.clone():"a08"}}).to_string()).unwrap();
        let owner = DaemonServer::new_with_paths(Some(root.path().join("config")), Some(root.path().join("auth")));
        (root, owner, repo, token)
    }).await.unwrap();
    let state = owner.remote_state().clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let gateway = tokio::spawn(async move { axum::serve(listener, create_remote_router(state)).with_graceful_shutdown(async { stopped.await.unwrap(); }).await.unwrap(); });
    let mut owned_sessions = Vec::new();
    let result = std::panic::AssertUnwindSafe(async {
        let client = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(40)).build().unwrap();
        let base = format!("http://{addr}/api/v1/workspace");
        let response = client.post(format!("{base}/projects")).bearer_auth(&token).body(json!({"requestId":uuid::Uuid::new_v4(),"repoPath":repo}).to_string()).send().await.unwrap();
        assert_eq!(response.status(), 201);
        let project: Value = serde_json::from_str(&response.text().await.unwrap()).unwrap();
        let ws = project["workspaceId"].as_str().unwrap();
        for locked in [false, true] {
            let slug = if locked { "locked" } else { "missing" };
            let endpoint = format!("{base}/worktrees");
            let response = client.post(&endpoint).bearer_auth(&token).body(json!({"requestId":uuid::Uuid::new_v4(),"workspaceId":ws,"worktree":{"wsId":ws,"slug":slug}}).to_string()).send().await.unwrap();
            assert_eq!(response.status(), 201);
            let row: Value = serde_json::from_str(&response.text().await.unwrap()).unwrap();
            let path = std::path::PathBuf::from(row["path"].as_str().unwrap());
            let url = format!("{endpoint}/status?workspaceId={ws}&wsId={ws}&slug={slug}");
            let live: Value = serde_json::from_str(&client.get(&url).bearer_auth(&token).send().await.unwrap().text().await.unwrap()).unwrap();
            assert_eq!(live["dirtyCount"], 0);
            let mut expected_sessions: Vec<String> = Vec::new();
            #[cfg(unix)] {
                let terminal = owner.terminal_service().clone();
                let term = terminal.clone();
                let (id, _lifecycle) = tokio::task::spawn_blocking({ let repo = repo.clone(); let path = path.clone(); move || {
                    let manager = ferryx_lib::worktree::WorktreeManager::try_new(&repo).unwrap();
                    let mut cmd = portable_pty::CommandBuilder::new("/bin/sh");
                    cmd.args(["-c", "printf 'A08_READY\\n'; read value"]);
                    cmd.cwd(&path);
                    term.spawn_in_worktree(cmd, 80, 24, &manager, &path).unwrap()
                }}).await.unwrap();
                let (initial, mut output) = terminal.attach(&id).unwrap();
                owned_sessions.push(id.clone());
                expected_sessions.push(id);
                tokio::time::timeout(Duration::from_secs(10), async {
                    let mut bytes = initial;
                    while !String::from_utf8_lossy(&bytes).contains("A08_READY") {
                        bytes.extend(output.recv().await.unwrap());
                    }
                }).await.unwrap();
            }
            let expected_head = tokio::task::spawn_blocking({ let repo = repo.clone(); let path = path.clone(); move || {
                run_git(&path, &["commit", "--allow-empty", "-m", "unmerged"]).unwrap();
                let head = run_git(&path, &["rev-parse", "HEAD"]).unwrap().trim().to_owned();
                if locked { run_git(&repo, &["worktree", "lock", "--reason", "A08 lock", path.to_str().unwrap()]).unwrap(); }
                std::fs::remove_dir_all(&path).unwrap(); head
            }}).await.unwrap();
            let response = client.get(&url).bearer_auth(&token).send().await.unwrap();
            let status = response.status().as_u16(); let body = response.text().await.unwrap();
            eprintln!("A08 HTTP missing locked={locked} status={status} body={body}");
            assert_eq!(status, 200, "registered missing checkout must return truthful rich preview, not unsupported");
            let preview: Value = serde_json::from_str(&body).unwrap();
            assert_eq!(preview["dirtyInspection"], "unavailable");
            assert!(preview.get("dirtyFiles").unwrap().is_null());
            assert!(preview.get("dirtyCount").unwrap().is_null());
            assert_eq!(preview["branchDeletion"]["head"], expected_head);
            assert_eq!(preview["branchDeletion"]["merged"], false);
            assert_eq!(preview["liveSessionIds"], json!(expected_sessions));
            assert_eq!(preview["locked"].is_string(), locked);
            assert_eq!(preview["prunable"].is_string(), !locked);
            let response = client.delete(&endpoint).bearer_auth(&token).body(json!({"requestId":uuid::Uuid::new_v4(),"workspaceId":ws,"worktree":{"wsId":ws,"slug":slug},"expectedRevision":preview["revision"],"deleteBranch":true}).to_string()).send().await.unwrap();
            assert_eq!(response.status(), 404);
            eprintln!("A08 HTTP missing DELETE guarded=404 locked={locked}");
            #[cfg(unix)] {
                for destination in [repo.clone(), root.path().to_owned(), root.path().join("dangling")] {
                tokio::task::spawn_blocking({ let path = path.clone(); move || std::os::unix::fs::symlink(destination, path).unwrap() }).await.unwrap();
                let response = client.get(&url).bearer_auth(&token).send().await.unwrap();
                assert!(!response.status().is_success(), "symlink replacement must not be inspected");
                eprintln!("A08 HTTP symlink preview refused={}", response.status());
                let response = client.delete(&endpoint).bearer_auth(&token).body(json!({"requestId":uuid::Uuid::new_v4(),"workspaceId":ws,"worktree":{"wsId":ws,"slug":slug},"expectedRevision":preview["revision"],"deleteBranch":true}).to_string()).send().await.unwrap();
                assert!(!response.status().is_success(), "symlink DELETE must remain guarded");
                tokio::task::spawn_blocking({ let path = path.clone(); move || std::fs::remove_file(path).unwrap() }).await.unwrap();
                }
            }
            let response = client.get(format!("{endpoint}/status?workspaceId={ws}&wsId=wrong&slug={slug}")).bearer_auth(&token).send().await.unwrap();
            assert_eq!(response.status(), 400);
            let branch = row["branch"].as_str().unwrap().to_owned();
            tokio::task::spawn_blocking({ let repo = repo.clone(); move || { run_git(&repo, &["update-ref", "-d", &branch]).unwrap(); } }).await.unwrap();
            let response = client.get(&url).bearer_auth(&token).send().await.unwrap();
            assert_eq!(response.status(), 200);
            let unavailable: Value = serde_json::from_str(&response.text().await.unwrap()).unwrap();
            assert!(unavailable.get("branchDeletion").unwrap().is_null());
            assert!(unavailable["branchDeletionError"].is_string());
            eprintln!("A08 HTTP unavailable branch preview={unavailable}");
        }
    }).catch_unwind().await;
    for id in owned_sessions {
        tokio::time::timeout(Duration::from_secs(10), owner.terminal_service().close_session(&id)).await.unwrap().unwrap();
        assert!(!owner.terminal_service().pty_manager().has_session(&id));
        eprintln!("A08 cleanup session={id} close_awaited=true session_absent=true");
    }
    stop.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(10), gateway).await.unwrap().unwrap();
    drop(owner);
    assert!(tokio::net::TcpStream::connect(addr).await.is_err());
    tokio::task::spawn_blocking(move || { let path = root.path().to_owned(); root.close().unwrap(); assert!(!path.exists()); }).await.unwrap();
    eprintln!("A08 cleanup listener_joined=true connection_refused=true owner_dropped=true private_root_removed=true owned_sessions_closed=true");
    if let Err(panic) = result { std::panic::resume_unwind(panic); }
}
