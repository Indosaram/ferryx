use ferryx_lib::daemon::server::DaemonServer;

async fn ipc_register(server: std::sync::Arc<DaemonServer>, roots: Vec<(String, std::path::PathBuf)>, inject: bool) {
    use ferryx_lib::daemon::protocol::{DaemonRequest, DaemonResponse};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let (client, stream) = tokio::io::duplex(8192);
    let handler = tokio::spawn(server.handle_client(stream));
    let mut client_task = tokio::spawn(async move {
        let (read, mut write) = tokio::io::split(client);
        let mut read = BufReader::new(read);
        for request in std::iter::once(DaemonRequest::Ping).chain(roots.into_iter().map(|(id, root)| DaemonRequest::RegisterWorkspace {
            workspace_id: id, repo_root: root.to_str().unwrap().into(),
        })) {
            write.write_all(format!("{}\n", serde_json::to_string(&request).unwrap()).as_bytes()).await.unwrap();
            let mut line = String::new();
            read.read_line(&mut line).await.unwrap();
            let response: DaemonResponse = serde_json::from_str(&line).unwrap();
            match request {
                DaemonRequest::Ping => assert!(matches!(response, DaemonResponse::Pong)),
                DaemonRequest::RegisterWorkspace { .. } => assert!(matches!(response, DaemonResponse::RegisterWorkspaceOk)),
                _ => unreachable!(),
            }
            assert!(!inject, "injected IPC assertion after Pong");
        }
    });
    let outcome = tokio::time::timeout(std::time::Duration::from_secs(10), &mut client_task).await;
    if outcome.is_err() { client_task.abort(); let _ = client_task.await; }
    let mut handler = handler;
    let joined = tokio::time::timeout(std::time::Duration::from_secs(10), &mut handler).await;
    if joined.is_err() { handler.abort(); let _ = handler.await; }
    eprintln!("IPC cleanup client_joined=true handler_joined=true duplex_closed=true injected={inject}");
    assert!(joined.is_ok());
    if inject { assert!(matches!(outcome, Ok(Err(ref e)) if e.is_panic())); }
    else { assert!(matches!(outcome, Ok(Ok(())))); }
}

// The same test binary is an isolated owner, never the canonical daemon CLI.
#[test]
fn catalog_owner_process() {
    use std::io::{BufRead, Write};
    let Some(root) = std::env::var_os("A05_OWNER_ROOT") else { return };
    let root = std::path::PathBuf::from(root);
    let server = std::sync::Arc::new(DaemonServer::new_with_paths(Some(root.join("data/config")), Some(root.join("data/auth"))));
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(ipc_register(server.clone(), vec![], false));
    // Serial libtest prints its test-name prefix without a newline.
    println!("\nA05_READY"); std::io::stdout().flush().unwrap();
    let mut command = String::new();
    std::io::stdin().lock().read_line(&mut command).unwrap();
    if command.trim() == "register" {
        runtime.block_on(ipc_register(server.clone(), vec![("plain".into(), root.join("plain")), ("git".into(), root.join("git"))], false));
    } else {
        assert!(server.workspace_registry().contains("plain"));
        assert!(server.workspace_registry().contains("git"));
    }
    println!("A05_DONE"); std::io::stdout().flush().unwrap();
}

#[test]
fn isolated_owner_restart_and_failure_cleanup() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().to_owned();
    let outcome = std::panic::catch_unwind(|| {
        std::fs::create_dir(root.join("plain")).unwrap();
        std::fs::create_dir(root.join("git")).unwrap();
        assert!(std::process::Command::new("git").args(["init", "--quiet"]).arg(root.join("git")).status().unwrap().success());
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
            for command in ["register", "restore", "inject"] {
                let mut child = tokio::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", "catalog_owner_process", "--nocapture"])
                    .env("A05_OWNER_ROOT", &root).env("HOME", &root).env("FERRYX_DATA_DIR", root.join("data"))
                    .env("FERRYX_RUNTIME_DIR", root.join("runtime"))
                    .env("XDG_RUNTIME_DIR", &root).env("TMPDIR", &root)
                    .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped())
                    .kill_on_drop(true).spawn().unwrap();
                let pid = child.id().unwrap();
                let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
                let mut input = child.stdin.take().unwrap();
                let result = tokio::time::timeout(std::time::Duration::from_secs(20), async {
                    let mut ready = false;
                    while let Some(line) = lines.next_line().await.map_err(|_| "read readiness failed")? {
                        if line == "A05_READY" { ready = true; break; }
                    }
                    if !ready { return Err("owner EOF before readiness"); }
                    eprintln!("OWNER readiness=Ping/Pong pid={pid} command={command}");
                    if command == "inject" { return Err("injected failure after readiness"); }
                    input.write_all(format!("{command}\n").as_bytes()).await.map_err(|_| "write command failed")?;
                    let mut done = false;
                    while let Some(line) = lines.next_line().await.map_err(|_| "read completion failed")? {
                        if line == "A05_DONE" { done = true; }
                    }
                    if done { Ok(()) } else { Err("owner EOF before completion") }
                }).await;
                drop(input); drop(lines);
                if !matches!(result, Ok(Ok(()))) { child.start_kill().unwrap(); }
                let status = match tokio::time::timeout(std::time::Duration::from_secs(10), child.wait()).await {
                    Ok(status) => status.unwrap(),
                    Err(_) => { child.start_kill().unwrap(); child.wait().await.unwrap() }
                };
                eprintln!("OWNER cleanup pid={pid} reaped=true status={status} sockets=[] command={command}");
                if command == "inject" { assert_eq!(result.unwrap(), Err("injected failure after readiness")); }
                else { assert!(matches!(result, Ok(Ok(())))); assert!(status.success()); }
            }
            let server = std::sync::Arc::new(DaemonServer::new_with_paths(Some(root.join("data/config")), Some(root.join("data/auth"))));
            ipc_register(server, vec![], true).await;
        });
    });
    fixture.close().unwrap();
    eprintln!("OWNER CLEANUP root={} absent={}", root.display(), !root.exists());
    if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
}

#[test]
fn normalized_registration_is_idempotent() {
    let root = tempfile::tempdir().unwrap();
    let plain = root.path().join("plain");
    std::fs::create_dir(&plain).unwrap();
    let server = DaemonServer::new_with_paths(Some(root.path().join("data/config")), Some(root.path().join("data/auth")));
    server.handle_register_workspace("  trimmed  ", plain.to_str().unwrap()).unwrap();
    assert!(server.workspace_registry().contains("trimmed"), "normalized registration lost");
    let revision = server.workspace_registry().revision();
    let service = &server.remote_state().machine_services.as_ref().unwrap().workspaces;
    let catalog_revision = service.catalog().unwrap().revision;
    server.handle_register_workspace("trimmed", plain.to_str().unwrap()).unwrap();
    assert_eq!(revision, server.workspace_registry().revision());
    assert_eq!(catalog_revision, service.catalog().unwrap().revision);
    let machine_root = root.path().join("machine");
    std::fs::create_dir(&machine_root).unwrap();
    let id = service.register_machine(machine_root.to_str().unwrap()).unwrap();
    assert!(!server.workspace_registry().contains(&id));
    server.handle_register_workspace(&id, machine_root.to_str().unwrap()).unwrap();
    assert!(server.workspace_registry().contains(&id));
    assert!(service.catalog().unwrap().workspaces[&id].mirror_exposed);
    eprintln!("NORMALIZATION trimmed identity; unchanged revision; explicit exposure promotion verified");
    drop(server);
    let receipt = root.path().to_owned();
    root.close().unwrap();
    eprintln!("NORMALIZATION CLEANUP root={} absent={}", receipt.display(), !receipt.exists());
}

#[test]
fn registration_survives_isolated_service_restart() {
    let fixture = tempfile::tempdir().unwrap();
    let owner = fixture.path().to_owned();
    let result = std::panic::catch_unwind(|| {
        let plain = owner.join("plain");
        let git = owner.join("git");
        std::fs::create_dir(&plain).unwrap();
        std::fs::create_dir(&git).unwrap();
        assert!(std::process::Command::new("git").args(["init", "--quiet"])
            .arg(&git).status().unwrap().success());
        let config = owner.join("data/config.json");
        let auth = owner.join("data/auth.json");
        let server = std::sync::Arc::new(DaemonServer::new_with_paths(Some(config.clone()), Some(auth.clone())));
        eprintln!("READY isolated service root={}", owner.display());
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(ipc_register(server.clone(), vec![("plain".into(), plain.clone()), ("git".into(), git.clone())], false));
        drop(runtime);
        drop(server);
        let restored = DaemonServer::new_with_paths(Some(config), Some(auth));
        eprintln!("RESTART isolated service");
        assert!(restored.workspace_registry().contains("plain"), "plain registration lost on restart");
        assert!(restored.workspace_registry().contains("git"), "Git registration lost on restart");
        let services = restored.remote_state().machine_services.as_ref().unwrap();
        let machine = owner.join("machine");
        std::fs::create_dir(&machine).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&machine, owner.join("alias")).unwrap();
        #[cfg(unix)]
        std::thread::scope(|scope| {
            let gate = std::sync::Arc::new(std::sync::Barrier::new(3));
            let tasks: Vec<_> = [machine.clone(), owner.join("alias")].into_iter().map(|path| {
                let gate = gate.clone();
                let service = &services.workspaces;
                scope.spawn(move || { gate.wait(); service.register_machine(path.to_str().unwrap()).unwrap() })
            }).collect();
            gate.wait();
            let ids: Vec<_> = tasks.into_iter().map(|t| t.join().unwrap()).collect();
            assert_eq!(ids[0], ids[1]);
            assert!(!restored.workspace_registry().contains(&ids[0]));
            eprintln!("CONCURRENT aliases canonical id={} mirror_exposed=false", ids[0]);
        });
        for id in ["ssh:remote", "daemon:desktop"] {
            assert!(restored.handle_register_workspace(id, plain.to_str().unwrap()).is_err());
            assert!(restored.workspace_registry().manager(id).is_err());
            assert!(!restored.workspace_registry().unregister(id));
        }
        let catalog_path = owner.join("data/machine-workspaces.v1.json");
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&catalog_path).unwrap().permissions().mode() & 0o777, 0o600);
            assert_eq!(std::fs::metadata(owner.join("data")).unwrap().permissions().mode() & 0o777, 0o700);
            eprintln!("PERMISSIONS catalog=600 parent=700");
        }
        drop(restored);
        std::fs::remove_dir(&plain).unwrap();
        std::fs::remove_dir_all(&git).unwrap();
        std::fs::write(&git, b"invalid root sentinel").unwrap();
        let restored = DaemonServer::new_with_paths(Some(owner.join("data/config.json")), Some(owner.join("data/auth.json")));
        let services = restored.remote_state().machine_services.as_ref().unwrap();
        assert_eq!(services.workspaces.catalog().unwrap().workspaces["plain"].availability,
            ferryx_lib::remote::machine_protocol::Availability::Missing);
        assert!(!plain.exists());
        assert_eq!(services.workspaces.catalog().unwrap().workspaces["git"].availability,
            ferryx_lib::remote::machine_protocol::Availability::Invalid);
        assert_eq!(std::fs::read(&git).unwrap(), b"invalid root sentinel");
        eprintln!("RESTORE invalid root retained without overwrite");
        std::fs::remove_file(&git).unwrap();
        std::fs::create_dir(&git).unwrap();
        eprintln!("RESTORE missing row retained without recreation");
        let revision = restored.workspace_registry().revision();
        std::fs::remove_file(&catalog_path).unwrap();
        std::fs::create_dir(&catalog_path).unwrap();
        assert!(restored.handle_register_workspace("failed", git.to_str().unwrap()).is_err());
        assert!(!restored.workspace_registry().contains("failed"));
        assert_eq!(revision, restored.workspace_registry().revision());
        assert!(services.workspaces.catalog().is_err());
        eprintln!("WRITE_FAILURE rename target directory: no success or published revision; mutations fenced");
        drop(restored);
        std::fs::remove_dir(&catalog_path).unwrap();
        for bytes in [b"not-json".as_slice(), br#"{"version":2,"revision":"0","workspaces":{}}"#.as_slice()] {
            std::fs::write(&catalog_path, bytes).unwrap();
            let server = DaemonServer::new_with_paths(Some(owner.join("data/config.json")), Some(owner.join("data/auth.json")));
            assert!(server.handle_register_workspace("blocked", git.to_str().unwrap()).is_err());
            assert_eq!(std::fs::read(&catalog_path).unwrap(), bytes);
            eprintln!("QUARANTINE invalid/newer catalog preserved; mutation refused");
        }
    });
    fixture.close().unwrap();
    eprintln!("CLEANUP root={} absent={} daemon_pids=[] sockets=[]", owner.display(), !owner.exists());
    if let Err(panic) = result { std::panic::resume_unwind(panic); }
}
