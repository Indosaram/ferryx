//! Real loopback OpenSSH + PTY regression. Trust and PATH changes are confined
//! to a child test process; the user's SSH config/known_hosts are never changed.
use super::*;
use crate::ipc::project_remote::{register_remote_project, RegisterRemoteProjectRequest};
use crate::ssh::{projects, SshAuthMethod, SshHost, SshHostSource};
use std::os::fd::OwnedFd;
use std::os::unix::fs::PermissionsExt;
use std::process::Stdio;

fn write_hosts(path: &Path, hosts: Vec<SshHost>) {
    std::fs::write(
        path,
        serde_json::to_vec(&crate::ipc::ssh::SshHostStore {
            hosts,
            tombstones: vec![],
        })
        .unwrap(),
    )
    .unwrap();
}

#[path = "remote_ssh_qa.rs"]
mod qa;

#[path = "remote_ssh_gateway_qa.rs"]
mod gateway_qa;

#[tokio::test]
async fn direct_ssh_real_transport_registration_and_pty() {
    if let Some(config) = std::env::var_os(qa::CONFIG_ENV) {
        qa::run(Path::new(&config)).await;
        return;
    }
    const CHILD: &str = "FERRYX_SSH_REGRESSION_CHILD";
    if let Some(root) = std::env::var_os(CHILD) {
        exercise_child(Path::new(&root)).await;
        return;
    }
    let dir = tempfile::Builder::new().prefix("fx").tempdir_in("/tmp").unwrap();
    for name in ["host_key", "user_key"] {
        let output = std::process::Command::new("ssh-keygen")
            .args(["-q", "-t", "ed25519", "-N", "", "-f"])
            .arg(dir.path().join(name))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let config = dir.path().join("sshd_config");
    std::fs::write(&config, format!("HostKey {}\nAuthorizedKeysFile {}\nStrictModes no\nUsePAM no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nLogLevel ERROR\n",
        dir.path().join("host_key").display(), dir.path().join("user_key.pub").display())).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let host_key = std::fs::read_to_string(dir.path().join("host_key.pub")).unwrap();
    let known_hosts = dir.path().join("known_hosts");
    std::fs::write(&known_hosts, format!("[127.0.0.1]:{port} {host_key}")).unwrap();
    let wrapper = dir.path().join("ssh");
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\nexec /usr/bin/ssh -F /dev/null -o UserKnownHostsFile={} \"$@\"\n",
            crate::ssh::direct::quote_posix(known_hosts.to_str().unwrap())
        ),
    )
    .unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(dir.path().join("port"), port.to_string()).unwrap();
    let log_path = dir.path().join("sshd.log");
    let log_for_server = log_path.clone();
    // sshd inetd mode uses an already-bound socket: no ephemeral-port race or polling.
    let server = tokio::spawn(async move {
        let mut children = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                connection = listener.accept() => {
                    let (stream, _) = connection.unwrap();
                    let stream = stream.into_std().unwrap();
                    stream.set_nonblocking(false).unwrap();
                    let input = Stdio::from(OwnedFd::from(stream.try_clone().unwrap()));
                    let output = Stdio::from(OwnedFd::from(stream));
                    let log = std::fs::OpenOptions::new().create(true).append(true).open(&log_for_server).unwrap();
                    let mut child = tokio::process::Command::new("/usr/sbin/sshd")
                        .args(["-i", "-e", "-f"]).arg(&config)
                        .stdin(input).stdout(output).stderr(log).kill_on_drop(true).spawn().unwrap();
                    children.spawn(async move { child.wait().await.unwrap() });
                }
                Some(result) = children.join_next(), if !children.is_empty() => { result.unwrap(); }
            }
        }
    });
    let path = std::env::var_os("PATH").unwrap();
    let mut paths = vec![dir.path().to_path_buf()];
    paths.extend(std::env::split_paths(&path));
    let output = tokio::time::timeout(
        Duration::from_secs(45),
        tokio::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "daemon::server::remote_ssh_tests::direct_ssh_real_transport_registration_and_pty",
                "--nocapture",
            ])
            .env(CHILD, dir.path())
            .env("PATH", std::env::join_paths(paths).unwrap())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .expect("bounded child test")
    .unwrap();
    let log = std::fs::read_to_string(log_path).unwrap_or_default();
    assert!(
        output.status.success(),
        "child stdout:\n{}\nchild stderr:\n{}\nsshd:\n{log}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    // Exercise the same external-machine seam on the deterministic loopback fixture.
    let user = std::process::Command::new("id")
        .arg("-un")
        .output()
        .unwrap();
    assert!(user.status.success());
    let canonical = dir
        .path()
        .join("remote project's space")
        .canonicalize()
        .unwrap();
    let qa_config = qa::QaConfig {
        host: SshHost {
            id: "qa-loopback".into(),
            label: "QA seam regression".into(),
            hostname: "127.0.0.1".into(),
            username: Some(String::from_utf8(user.stdout).unwrap().trim().into()),
            port: Some(port),
            identity_file: Some(dir.path().join("user_key").to_str().unwrap().into()),
            jump_host: None,
            source: SshHostSource::Manual,
            auth_method: SshAuthMethod::Key,
            disabled: None,
        },
        known_hosts_file: known_hosts,
        repo_path: dir.path().join("alias").to_str().unwrap().into(),
        expected_repo_root: canonical.to_str().unwrap().into(),
        expected_git_root: Some(canonical.to_str().unwrap().into()),
    };
    let qa_path = dir.path().join("qa.json");
    std::fs::write(&qa_path, serde_json::to_vec(&qa_config).unwrap()).unwrap();
    qa::run(&qa_path).await;
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
}

async fn wait_remote_connected(daemon: &DaemonServer, id: &str) {
    let mut updates = daemon.terminal_service.remote().subscribe(id).unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if updates.borrow_and_update().state == crate::terminal::remote::RemoteConnectionState::Connected { break; }
            updates.changed().await.unwrap();
        }
    }).await.expect("helper target connected");
}

struct OwnedTestHelper(std::process::Child);

impl Drop for OwnedTestHelper {
    fn drop(&mut self) {
        let pid = self.0.id();
        let _ = self.0.kill();
        self.0.wait().expect("reap owned test helper");
        println!("QA_HELPER_REAPED {pid}");
    }
}

async fn install_test_helper(host: &SshHost, home: &Path) -> OwnedTestHelper {
    let mut environment = crate::ssh::runtime::detect(host).await.unwrap();
    environment.home = home.to_string_lossy().into_owned();
    let location = crate::ssh::helper_setup::default_location(host, &environment).unwrap();
    let binary = Path::new(env!("CARGO_MANIFEST_DIR")).join("../remote-helper/target/debug/ferryx-remote-helper");
    assert!(binary.is_file(), "Explicit test prerequisite: build remote-helper before SSH daemon tests");
    crate::ssh::helper_setup::install(host, &environment, &location, &binary).await.unwrap();
    let mut child = std::process::Command::new(&location.executable)
        .args(["daemon", "--root", &location.root, "--host-id", &host.id])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn().expect("owned foreground helper");
    let stdout = child.stdout.take().unwrap();
    let helper = OwnedTestHelper(child);
    let (sender, ready) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        use std::io::BufRead;
        let mut line = String::new();
        let result = std::io::BufReader::new(stdout).read_line(&mut line).map(|_| line);
        let _ = sender.send(result);
    });
    let line = ready.recv_timeout(Duration::from_secs(5)).expect("helper readiness event").unwrap();
    reader.join().unwrap();
    let ready: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(ready["event"], "ready");
    println!("QA_HELPER_OWNED {} {}", helper.0.id(), location.root);
    helper
}

async fn exercise_child(root: &Path) {
    let port = std::fs::read_to_string(root.join("port"))
        .unwrap()
        .parse()
        .unwrap();
    let user = std::process::Command::new("id")
        .arg("-un")
        .output()
        .unwrap();
    assert!(user.status.success());
    let mut host = SshHost {
        id: "real-test-host".into(),
        label: "Loopback SSH".into(),
        hostname: "127.0.0.1".into(),
        username: Some(String::from_utf8(user.stdout).unwrap().trim().into()),
        port: Some(port),
        identity_file: Some(root.join("user_key").to_str().unwrap().into()),
        jump_host: None,
        source: SshHostSource::Manual,
        auth_method: SshAuthMethod::Key,
        disabled: None,
    };
    let host_store = root.join("ssh_hosts.json");
    write_hosts(&host_store, vec![host.clone()]);
    let repository = root.join("remote project's space");
    std::fs::create_dir(&repository).unwrap();
    let alias = root.join("alias");
    std::os::unix::fs::symlink(&repository, &alias).unwrap();
    let response = register_remote_project(
        host_store.clone(),
        RegisterRemoteProjectRequest {
            workspace_id: "friendly-name".into(),
            host_id: host.id.clone(),
            repo_path: alias.to_str().unwrap().into(),
        },
    )
    .await
    .expect("real SSH registration");
    assert_eq!(
        response.repo_root,
        repository.canonicalize().unwrap().to_str().unwrap()
    );
    assert_eq!(response.git_root, None);
    assert_eq!(response.host_id, host.id);
    assert_eq!(response.host_label, host.label);
    let json = serde_json::to_value(&response).unwrap();
    assert!(json["gitRoot"].is_null());
    assert_eq!(json["workspaceId"], response.workspace_id);
    assert!(std::process::Command::new("git")
        .arg("init")
        .arg(&repository)
        .output()
        .unwrap()
        .status
        .success());
    let repeated = register_remote_project(
        host_store.clone(),
        RegisterRemoteProjectRequest {
            workspace_id: response.workspace_id.clone(),
            host_id: host.id.clone(),
            repo_path: alias.to_str().unwrap().into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(repeated.workspace_id, response.workspace_id);
    assert_eq!(repeated.git_root, Some(response.repo_root.clone()));
    // A fresh daemon and fresh disk lookup model GUI restart; no local workspace
    // registration exists, and no local placeholder is needed for either tab.
    let daemon = DaemonServer::new_with_paths(Some(root.join("gateway.json")), Some(root.join("auth.json")));
    let _helper = install_test_helper(&host, root).await;
    assert!(daemon
        .handle_register_workspace(&response.workspace_id, &response.repo_root)
        .is_err());
    assert!(daemon.workspace_registry.list().is_empty());
    let startup = TerminalStartup::RemoteSsh {
        host_store_path: host_store.clone(),
    };
    assert!(ProviderSessionClaimKey::from_startup(Some(&startup)).is_none());
    let wire = serde_json::to_value(&startup).unwrap();
    assert_eq!(wire["kind"], "remoteSsh");
    assert!(wire.get("program").is_none());
    let mut retained = None;
    for request_id in ["new-tab", "split-restore"] {
        let spawn_cwd = if request_id == "new-tab" {
            Some(response.repo_root.clone())
        } else {
            None
        };
        let id = daemon
            .handle_spawn(
                request_id,
                &response.workspace_id,
                None,
                spawn_cwd,
                80,
                24,
                None,
                Some(startup.clone()),
            )
            .await
            .expect("SSH PTY spawn");
        let (mut history, mut events) = daemon.terminal_service.attach(&id).unwrap();
        // Subscribe before writing; octal marker is absent from PTY input echo.
        wait_remote_connected(&daemon, &id).await;
        daemon.write_session_input(&id, b"printf '\\136\\123\\123\\110\\055\\117\\113\\072'; pwd -P\n".to_vec()).await.unwrap();
        let expected = format!("^SSH-OK:{}", response.repo_root);
        tokio::time::timeout(Duration::from_secs(10), async {
            while !String::from_utf8_lossy(&history).contains(&expected) {
                history.extend(events.recv().await.expect("SSH PTY output event"));
            }
        })
        .await
        .expect("remote command produced exact root through real SSH PTY");
        let DaemonResponse::DescribeSessionOk { session } = daemon.handle_describe_session(&id)
        else {
            panic!("session details")
        };
        assert_eq!(session.cwd.as_deref(), Some(response.repo_root.as_str()));
        assert_eq!(
            session.workspace_id.as_deref(),
            Some(response.workspace_id.as_str())
        );
        assert!(daemon.terminal_service.get_session(&id).is_none());
        if request_id == "new-tab" {
            gateway_qa::exercise(&daemon, &response.workspace_id, &id, &host_store).await;
            daemon.handle_close(&id).await.unwrap();
        } else {
            retained = Some(id);
        }
    }
    let wt_dir = Path::new(&response.repo_root).join(".orca-worktrees").join("wt-feature");
    std::fs::create_dir_all(&wt_dir).unwrap();
    let wt_identity = crate::worktree::WorktreeIdentity {
        ws_id: "agent".into(),
        slug: "feature".into(),
    };
    let wt_session_id = daemon
        .handle_spawn(
            "remote-worktree-tab",
            &response.workspace_id,
            Some(wt_identity.clone()),
            Some(wt_dir.to_str().unwrap().into()),
            80,
            24,
            None,
            Some(startup.clone()),
        )
        .await
        .expect("SSH PTY spawn in remote worktree");
    let (mut wt_history, mut wt_events) = daemon.terminal_service.attach(&wt_session_id).unwrap();
    wait_remote_connected(&daemon, &wt_session_id).await;
    daemon.write_session_input(&wt_session_id, b"printf '\\136\\123\\123\\110\\055\\117\\113\\072'; pwd -P\n".to_vec()).await.unwrap();
    let expected_wt = format!("^SSH-OK:{}", wt_dir.to_str().unwrap());
    tokio::time::timeout(Duration::from_secs(10), async {
        while !String::from_utf8_lossy(&wt_history).contains(&expected_wt) {
            wt_history.extend(wt_events.recv().await.expect("SSH PTY output event"));
        }
    })
    .await
    .expect("remote command produced exact worktree root through real SSH PTY");
    let DaemonResponse::DescribeSessionOk { session: wt_session } =
        daemon.handle_describe_session(&wt_session_id)
    else {
        panic!("session details")
    };
    assert_eq!(wt_session.cwd.as_deref(), Some(wt_dir.to_str().unwrap()));
    assert_eq!(wt_session.worktree.as_ref(), Some(&wt_identity));
    daemon.handle_close(&wt_session_id).await.unwrap();

    let shell_err = daemon
        .handle_spawn(
            "remote-shell-override",
            &response.workspace_id,
            None,
            None,
            80,
            24,
            Some("/bin/zsh".into()),
            Some(startup.clone()),
        )
        .await
        .unwrap_err();
    assert!(
        shell_err.to_string().contains("shell overrides are unsupported"),
        "unexpected error message: {shell_err}"
    );

    let retained = retained.unwrap();
    let before = daemon.terminal_service.remote().details(&retained).unwrap();
    assert!(before.pid.is_some());
    let identity_path = daemon.remote_sessions_path.clone();
    daemon.persist_remote_sessions_at(identity_path.clone()).await.unwrap();
    let old_runtime = Arc::downgrade(daemon.terminal_service.remote());
    drop(daemon);
    assert!(old_runtime.upgrade().is_none(), "old runtime must be dropped, not retained by watchers");
    let daemon = DaemonServer::new_with_paths(Some(root.join("gateway.json")), Some(root.join("auth.json")));
    daemon.restore_remote_sessions_at(identity_path).await.unwrap();
    wait_remote_connected(&daemon, &retained).await;
    let after = daemon.terminal_service.remote().details(&retained).unwrap();
    assert_eq!(after.descriptor.target, before.descriptor.target);
    assert_eq!(after.pid, before.pid, "restart reattaches the original remote process");
    assert_eq!(after.descriptor.backend_session_id, before.descriptor.backend_session_id);
    assert!(daemon.terminal_service.get_session(&retained).is_none());
    let (mut replay, mut output) = daemon.terminal_service.attach(&retained).unwrap();
    let expected = format!("^SSH-OK:{}", response.repo_root);
    tokio::time::timeout(Duration::from_secs(10), async {
        while !String::from_utf8_lossy(&replay).contains(&expected) {
            replay.extend(output.recv().await.unwrap());
        }
    }).await.expect("restart replays original retained output without rerunning command");
    daemon.validate_session_ssh_target(&retained).await.unwrap();
    host.disabled = Some(true);
    write_hosts(&host_store, vec![host]);
    assert!(daemon.validate_session_ssh_target(&retained).await.is_err());
    assert!(crate::remote::RemoteSessionBackend::write_input(
        &*daemon.session_router, &retained, b"\n"
    ).await.is_err());
    daemon.handle_close(&retained).await.unwrap();
    assert!(daemon
        .handle_spawn(
            "disabled",
            &response.workspace_id,
            None,
            None,
            80,
            24,
            None,
            Some(startup.clone())
        )
        .await
        .is_err());
    write_hosts(&host_store, vec![]);
    assert!(daemon
        .handle_spawn(
            "deleted",
            &response.workspace_id,
            None,
            None,
            80,
            24,
            None,
            Some(startup)
        )
        .await
        .is_err());
    assert!(daemon
        .handle_spawn(
            "no-fallback",
            &response.workspace_id,
            None,
            None,
            80,
            24,
            None,
            None
        )
        .await
        .is_err());
    assert!(daemon.terminal_service.list_sessions().is_empty());
    assert_eq!(
        projects::resolve(&host_store, &response.workspace_id)
            .unwrap_err()
            .code,
        crate::ipc::IpcErrorCode::WorkspaceNotFound
    );
}
