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

const PERSISTENCE_CHILD: &str = "FERRYX_REMOTE_PERSISTENCE_CHILD";
const PERSISTENCE_PARTIAL: &str = "FERRYX_REMOTE_PERSISTENCE_PARTIAL";

/// Child half of [`remote_persistence_migrates_out_of_volatile_runtime_dir`].
///
/// Runs in its own process because it has to point `FERRYX_RUNTIME_DIR` at a fake
/// runtime directory, and that resolution is process-global.
#[tokio::test]
async fn remote_persistence_migration_child() {
    let Some(root) = std::env::var_os(PERSISTENCE_CHILD) else {
        return;
    };
    let root = PathBuf::from(root);
    let legacy_dir = crate::daemon::get_runtime_dir();
    assert_eq!(legacy_dir, root.join("runtime"), "child runs with a fake runtime dir");
    std::fs::create_dir_all(&legacy_dir).unwrap();
    let descriptor = serde_json::json!({
        "backendSessionId": "migrated-remote-session",
        "target": {"hostId":"host","ownerId":"owner","epoch":"1","backendSessionId":"migrated-target"},
        "config": {"host":{"id":"host","label":"host","hostname":"127.0.0.1","port":1,"source":"manual","authMethod":"agent"},"environment":{"platform":"posix","executor":"sh","version":"test","home":"/tmp","temp":"/tmp","git":true},"helper":{"executable":"/helper","root":"/root"},"projectId":"ssh:abcd","projectPath":"/project","worktree":null,"agentIdentity":null},
        "clientRequestId": "migrated-request",
        "remoteCursor": "11",
        "cols": 80,
        "rows": 24
    });
    std::fs::write(
        legacy_dir.join("remote_sessions.json"),
        serde_json::to_vec(&serde_json::json!({
            "version": 3,
            "timestamp": 0,
            "activeWorkspaceId": "",
            "workspaces": {},
            "remoteSessions": [{"descriptor": descriptor, "metadata": null}]
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(legacy_dir.join("paired_descriptors.json"), b"{}").unwrap();

    let isolated = root.join("durable");
    std::fs::create_dir_all(&isolated).unwrap();
    let durable = isolated.join("remote_sessions.json");
    if std::env::var_os(PERSISTENCE_PARTIAL).is_some() {
        // Leftover from an interrupted earlier migration: an unparseable tail that
        // must not suppress re-migration (the reboot-volatile source may be gone).
        std::fs::write(
            &durable,
            b"{\"version\":3,\"timestamp\":0,\"activeWorkspa",
        )
        .unwrap();
    } else {
        assert!(!durable.exists(), "durable location starts empty");
    }
    let daemon = DaemonServer::new_with_paths(
        Some(isolated.join("config")),
        Some(isolated.join("auth")),
    );
    daemon
        .restore_remote_sessions_at(durable.clone())
        .await
        .unwrap();

    assert!(
        durable.is_file(),
        "legacy remote_sessions.json must be migrated to the durable path"
    );
    serde_json::from_str::<serde_json::Value>(
        &std::fs::read_to_string(&durable).unwrap(),
    )
    .expect("migrated durable snapshot must parse");
    assert!(
        isolated.join("paired_descriptors.json").is_file(),
        "paired descriptors must be migrated alongside remote sessions"
    );
    assert!(
        legacy_dir.join("remote_sessions.json").is_file(),
        "migration copies; a draining predecessor still reads the legacy file"
    );
    let details = daemon
        .terminal_service
        .remote()
        .details("migrated-remote-session")
        .expect("migrated descriptor restored into the remote runtime");
    assert_eq!(
        details.state,
        crate::terminal::remote::RemoteConnectionState::Reconnecting
    );
    assert_eq!(details.descriptor.target.backend_session_id, "migrated-target");
}

/// An unparseable leftover (e.g. from an interrupted earlier migration) must not
/// suppress re-migration: after a reboot the volatile source may be all that is left.
#[tokio::test]
async fn remote_persistence_migration_recovers_from_partial_durable_file() {
    let root = tempfile::tempdir().unwrap();
    let output = tokio::time::timeout(
        Duration::from_secs(60),
        tokio::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "daemon::server::remote_ssh_tests::remote_persistence_migration_child",
                "--nocapture",
            ])
            .env(PERSISTENCE_CHILD, root.path())
            .env("FERRYX_RUNTIME_DIR", root.path().join("runtime"))
            .env("FERRYX_DATA_DIR", root.path().join("data"))
            .env("HOME", root.path())
            .env("TMPDIR", root.path())
            .env(PERSISTENCE_PARTIAL, "1")
            .kill_on_drop(true)
            .output(),
    )
    .await
    .expect("bounded child test")
    .unwrap();
    assert!(
        output.status.success(),
        "child stdout:\n{}\nchild stderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// `/tmp/rorca-{uid}` is wiped on reboot, so remote descriptors persisted there vanished
/// while the host-side helper daemons survived. Persistence must move to the durable
/// identity directory, migrating whatever the previous location still holds.
#[tokio::test]
async fn remote_persistence_migrates_out_of_volatile_runtime_dir() {
    let root = tempfile::tempdir().unwrap();
    let output = tokio::time::timeout(
        Duration::from_secs(60),
        tokio::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "daemon::server::remote_ssh_tests::remote_persistence_migration_child",
                "--nocapture",
            ])
            .env(PERSISTENCE_CHILD, root.path())
            .env("FERRYX_RUNTIME_DIR", root.path().join("runtime"))
            .env("FERRYX_DATA_DIR", root.path().join("data"))
            .env("HOME", root.path())
            .env("TMPDIR", root.path())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .expect("bounded child test")
    .unwrap();
    assert!(
        output.status.success(),
        "child stdout:\n{}\nchild stderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[path = "remote_ssh_gateway_qa.rs"]
mod gateway_qa;

/// Loopback `sshd` probing: macOS ships it at `/usr/sbin/sshd`, Linux
/// distributions use `/usr/sbin`, `/usr/local/sbin`, or `/sbin`, so probe the
/// known locations and fall back to PATH resolution.
fn sshd_binary() -> PathBuf {
    let candidates = ["/usr/sbin/sshd", "/usr/local/sbin/sshd", "/sbin/sshd"];
    for candidate in candidates {
        let candidate = PathBuf::from(candidate);
        if candidate.is_file() {
            return candidate;
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path) {
            let candidate = directory.join("sshd");
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    PathBuf::from(candidates[0])
}

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
    let dir = tempfile::Builder::new()
        .prefix("fx")
        .tempdir_in("/tmp")
        .unwrap();
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
    let sshd = sshd_binary();
    assert!(
        sshd.is_file(),
        "Explicit test prerequisite: install an OpenSSH server ({} missing)",
        sshd.display()
    );
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
                    let mut child = tokio::process::Command::new(&sshd)
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
    let child_stdout_file = std::fs::File::create(dir.path().join("child_stdout.log")).unwrap();
    let child_stderr_file = std::fs::File::create(dir.path().join("child_stderr.log")).unwrap();
    let mut child = tokio::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "daemon::server::remote_ssh_tests::direct_ssh_real_transport_registration_and_pty",
            "--nocapture",
        ])
        .env(CHILD, dir.path())
        // The daemon supervises its SSH transport by re-entering this test binary's ignored
        // `ssh_bridge_transport_supervisor_entry`; without the marker the bridge refuses to
        // supervise at all. The real app dispatches `--ferryx-ssh-supervisor` from its own
        // main instead, so this env belongs to the child process only.
        .env("FERRYX_SSH_SUPERVISOR_LIBTEST", "1")
        .env("PATH", std::env::join_paths(paths).unwrap())
        .stdout(child_stdout_file)
        .stderr(child_stderr_file)
        .kill_on_drop(true)
        .spawn()
        .expect("spawn child test process");
    let child_pid = child.id();
    let wait_res = tokio::time::timeout(Duration::from_secs(45), child.wait()).await;
    let status = match wait_res {
        Ok(Ok(status)) => status,
        Ok(Err(e)) => panic!("failed waiting for child test {child_pid:?}: {e}"),
        Err(_) => {
            child.kill().await.expect("kill timed-out child test process");
            let _ = child.wait().await.expect("reap timed-out child test process");
            let child_stdout = std::fs::read_to_string(dir.path().join("child_stdout.log")).unwrap();
            let child_stderr = std::fs::read_to_string(dir.path().join("child_stderr.log")).unwrap();
            let log = std::fs::read_to_string(&log_path).unwrap();
            panic!(
                "bounded child test timeout (45s) for PID {child_pid:?}\nchild stdout:\n{child_stdout}\nchild stderr:\n{child_stderr}\nsshd:\n{log}"
            );
        }
    };
    let child_stdout = std::fs::read_to_string(dir.path().join("child_stdout.log")).unwrap();
    let child_stderr = std::fs::read_to_string(dir.path().join("child_stderr.log")).unwrap();
    let log = std::fs::read_to_string(&log_path).unwrap();
    assert!(
        status.success(),
        "child stdout:\n{}\nchild stderr:\n{}\nsshd:\n{log}",
        child_stdout,
        child_stderr
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
            if updates.borrow_and_update().state
                == crate::terminal::remote::RemoteConnectionState::Connected
            {
                break;
            }
            updates.changed().await.unwrap();
        }
    })
    .await
    .expect("helper target connected");
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
    let location = crate::ssh::helper_setup::qualified_location(
        host,
        &environment,
        crate::ssh::helper_runtime::process::HELPER_VERSION,
    )
    .unwrap();
    let binary = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../remote-helper/target/debug/ferryx-remote-helper");
    assert!(
        binary.is_file(),
        "Explicit test prerequisite: build remote-helper before SSH daemon tests"
    );
    crate::ssh::helper_setup::install(host, &environment, &location, &binary)
        .await
        .unwrap();
    let mut child = std::process::Command::new(&location.executable)
        .args(["daemon", "--root", &location.root, "--host-id", &host.id])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("owned foreground helper");
    let stdout = child.stdout.take().unwrap();
    let helper = OwnedTestHelper(child);
    let (sender, ready) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        use std::io::BufRead;
        let mut line = String::new();
        let result = std::io::BufReader::new(stdout)
            .read_line(&mut line)
            .map(|_| line);
        let _ = sender.send(result);
    });
    let line = ready
        .recv_timeout(Duration::from_secs(5))
        .expect("helper readiness event")
        .unwrap();
    reader.join().unwrap();
    let ready: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(ready["event"], "ready");
    println!("QA_HELPER_OWNED {} {}", helper.0.id(), location.root);
    helper
}

async fn exercise_child(root: &Path) {
    let phase_started = std::time::Instant::now();
    macro_rules! phase {
        ($($arg:tt)*) => {
            eprintln!("[exercise_child +{:?}] {}", phase_started.elapsed(), format_args!($($arg)*));
        };
    }
    phase!("Phase 1: Reading loopback SSH port and user info");
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
    phase!("Phase 2: Registering remote project (pre-git init)");
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
    phase!("Phase 3: Registering remote project (post-git init)");
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
    let daemon = DaemonServer::new_with_paths(
        Some(root.join("gateway.json")),
        Some(root.join("auth.json")),
    );
    phase!("Phase 4: Installing test helper");
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
        phase!("Phase 5: Spawning session request={request_id}");
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
        phase!("Phase 5a: Connected session id={id}, writing input");
        daemon
            .write_session_input(
                &id,
                b"printf '\\136\\123\\123\\110\\055\\117\\113\\072'; pwd -P\n".to_vec(),
            )
            .await
            .unwrap();
        let expected = format!("^SSH-OK:{}", response.repo_root);
        tokio::time::timeout(Duration::from_secs(10), async {
            while !String::from_utf8_lossy(&history).contains(&expected) {
                match events.recv().await {
                    Ok(chunk) => history.extend(chunk),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(e) => panic!("SSH PTY output event: {e}"),
                }
            }
        })
        .await
        .expect("remote command produced exact root through real SSH PTY");
        phase!("Phase 5b: Verified PTY output for request={request_id}");
        let DaemonResponse::DescribeSessionOk { session, .. } = daemon.handle_describe_session(&id)
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
            phase!("Phase 5c: Exercising gateway_qa");
            gateway_qa::exercise(&daemon, &response.workspace_id, &id, &host_store).await;
            daemon.handle_close(&id).await.unwrap();
        } else {
            retained = Some(id);
        }
    }
    let wt_dir = Path::new(&response.repo_root)
        .join(".orca-worktrees")
        .join("wt-feature");
    std::fs::create_dir_all(&wt_dir).unwrap();
    phase!("Phase 6: Remote worktree spawn starting");
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
    phase!("Phase 6: Remote worktree spawn complete");
    let (mut wt_history, mut wt_events) = daemon.terminal_service.attach(&wt_session_id).unwrap();
    wait_remote_connected(&daemon, &wt_session_id).await;
    phase!("Phase 6a: Remote worktree connected, writing input");
    daemon
        .write_session_input(
            &wt_session_id,
            b"printf '\\136\\123\\123\\110\\055\\117\\113\\072'; pwd -P\n".to_vec(),
        )
        .await
        .unwrap();
    phase!("Phase 6a1: Remote worktree input write complete");
    let expected_wt = format!("^SSH-OK:{}", wt_dir.to_str().unwrap());
    tokio::time::timeout(Duration::from_secs(10), async {
        while !String::from_utf8_lossy(&wt_history).contains(&expected_wt) {
            match wt_events.recv().await {
                Ok(chunk) => wt_history.extend(chunk),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(e) => panic!("SSH PTY output event: {e}"),
            }
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "remote command did not produce exact worktree root through real SSH PTY; buffered output: {}",
            String::from_utf8_lossy(&wt_history)
        )
    });
    phase!("Phase 6b: Remote worktree PTY output verified");
    let DaemonResponse::DescribeSessionOk {
        session: wt_session,
        ..
    } = daemon.handle_describe_session(&wt_session_id)
    else {
        panic!("session details")
    };
    assert_eq!(wt_session.cwd.as_deref(), Some(wt_dir.to_str().unwrap()));
    assert_eq!(wt_session.worktree.as_ref(), Some(&wt_identity));
    daemon.handle_close(&wt_session_id).await.unwrap();

    phase!("Phase 7: Remote shell override check");
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
        shell_err
            .to_string()
            .contains("shell overrides are unsupported"),
        "unexpected error message: {shell_err}"
    );

    phase!("Phase 8: Restart and session persistence restore");
    let retained = retained.unwrap();
    let before = daemon.terminal_service.remote().details(&retained).unwrap();
    assert!(before.pid.is_some());
    let identity_path = daemon.remote_sessions_path.clone();
    phase!("Phase 8b: Persist starting");
    daemon
        .persist_remote_sessions_at(identity_path.clone())
        .await
        .unwrap();
    phase!("Phase 8c: Persist complete; dropping daemon");
    let old_runtime = Arc::downgrade(daemon.terminal_service.remote());
    drop(daemon);
    phase!("Phase 8d: Daemon dropped; checking old runtime");
    assert!(
        old_runtime.upgrade().is_none(),
        "old runtime must be dropped, not retained by watchers"
    );
    let daemon = DaemonServer::new_with_paths(
        Some(root.join("gateway.json")),
        Some(root.join("auth.json")),
    );
    phase!("Phase 8e: Restore starting");
    daemon
        .restore_remote_sessions_at(identity_path)
        .await
        .unwrap();
    phase!("Phase 8f: Restore complete; waiting for helper target");
    wait_remote_connected(&daemon, &retained).await;
    phase!("Phase 8a: Reconnected restored session");
    let after = daemon.terminal_service.remote().details(&retained).unwrap();
    assert_eq!(after.descriptor.target, before.descriptor.target);
    assert_eq!(
        after.pid, before.pid,
        "restart reattaches the original remote process"
    );
    assert_eq!(
        after.descriptor.backend_session_id,
        before.descriptor.backend_session_id
    );
    assert!(daemon.terminal_service.get_session(&retained).is_none());
    let (mut replay, mut output) = daemon.terminal_service.attach(&retained).unwrap();
    let expected = format!("^SSH-OK:{}", response.repo_root);
    tokio::time::timeout(Duration::from_secs(10), async {
        while !String::from_utf8_lossy(&replay).contains(&expected) {
            match output.recv().await {
                Ok(chunk) => replay.extend(chunk),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(e) => panic!("SSH PTY output event: {e}"),
            }
        }
    })
    .await
    .expect("restart replays original retained output without rerunning command");
    phase!("Phase 9: Validating session SSH target and disable host");
    daemon.validate_session_ssh_target(&retained).await.unwrap();
    host.disabled = Some(true);
    write_hosts(&host_store, vec![host]);
    assert!(daemon.validate_session_ssh_target(&retained).await.is_err());
    assert!(crate::remote::RemoteSessionBackend::write_input(
        &*daemon.session_router,
        &retained,
        b"\n"
    )
    .await
    .is_err());
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
    phase!("Phase 10: Complete");
}

/// First remote spawn on a host without the version-qualified helper must
/// resolve a digest-verified bundled asset, install it at the qualified
/// location, and start it before creating the session.
///
/// The asset fixture has to sit next to the executable (the packaged-app
/// layout), so the child runs from a private copy of this test binary inside
/// its own temporary directory rather than from the shared build directory.
const PROVISION_CHILD: &str = "FERRYX_SSH_PROVISION_CHILD";

/// Maps this machine to the bundled helper target that the loopback SSH host
/// reports. Parent and child run on the same host, so both derive one triple.
fn local_helper_target_triple() -> String {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    };
    crate::ssh::helper_assets::resolve_target_from_probe(os, std::env::consts::ARCH)
        .expect("the local test host must map to a bundled helper target")
        .triple()
        .to_string()
}

fn helper_target_for_triple(triple: &str) -> crate::ssh::helper_assets::HelperTarget {
    serde_json::from_value(serde_json::Value::String(triple.to_string()))
        .unwrap_or_else(|_| panic!("unsupported helper target triple: {triple}"))
}

/// The staged bundle path the packaged-app layout resolves to: `<exe_dir>/helpers`.
fn staged_asset_path(exe_dir: &Path, target_triple: &str) -> PathBuf {
    let target = helper_target_for_triple(target_triple);
    exe_dir
        .join("helpers")
        .join(target.triple())
        .join(target.filename())
}

/// Copies the just-built standalone helper into `exe_dir/helpers` as a real
/// bundled asset: `<exe_dir>/helpers/<triple>/<filename>` plus a manifest
/// carrying its true digest and byte length. `exe_dir` must be a private
/// directory owned by this test - the shared `target/debug/deps` directory is
/// never written to, because foreign artifacts may live there.
fn stage_verified_helper_asset(exe_dir: &Path, target_triple: &str) -> PathBuf {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../remote-helper/target/debug/ferryx-remote-helper");
    assert!(
        source.is_file(),
        "Explicit test prerequisite: build remote-helper before SSH daemon tests"
    );
    let target = helper_target_for_triple(target_triple);

    let binary_path = staged_asset_path(exe_dir, target_triple);
    std::fs::create_dir_all(binary_path.parent().unwrap()).unwrap();
    std::fs::copy(&source, &binary_path).unwrap();
    std::fs::set_permissions(&binary_path, std::fs::Permissions::from_mode(0o755)).unwrap();

    // The manifest is only honest if the staged file really is a current helper.
    let expected_version = crate::ssh::helper_runtime::process::HELPER_VERSION;
    let version = std::process::Command::new(&binary_path)
        .arg("--version")
        .output()
        .expect("run the staged helper");
    let reported = String::from_utf8_lossy(&version.stdout).trim().to_string();
    assert!(
        version.status.success() && reported == expected_version,
        "staged helper must report {expected_version}, got {reported:?} ({})",
        String::from_utf8_lossy(&version.stderr).trim()
    );

    let manifest = crate::ssh::helper_assets::HelperAssetManifest {
        schema_version: 1,
        helper_version: expected_version.to_string(),
        protocol_version: 1,
        artifacts: vec![crate::ssh::helper_assets::HelperAssetEntry {
            target,
            filename: target.filename().to_string(),
            sha256: crate::ssh::helper_assets::compute_file_sha256(&binary_path).unwrap(),
            byte_length: std::fs::metadata(&binary_path).unwrap().len(),
        }],
    };
    std::fs::write(
        exe_dir.join("helpers").join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    binary_path
}

/// Best-effort reap of the detached helper daemon that automatic provisioning
/// started inside this test's own temporary home.
///
/// A PID read from an endpoint file can outlive its process, so the signal is
/// only sent when the live command line still matches all three identity facts
/// of this test's helper: the qualified executable, `--root`, and `--host-id`.
/// A mismatch (including a reused PID) leaves the process alone. This is a
/// cleanliness measure, not a guarantee.
struct OwnedProvisionedHelper {
    root: PathBuf,
    executable: PathBuf,
    host_id: String,
}

/// Every accepted spelling of a path: as given, and canonicalized when the
/// filesystem resolves it differently (macOS reports `/private/tmp` for `/tmp`).
fn path_spellings(path: &Path) -> Vec<String> {
    let given = path.to_string_lossy().into_owned();
    let mut spellings = vec![given.clone()];
    if let Ok(canonical) = path.canonicalize() {
        let canonical = canonical.to_string_lossy().into_owned();
        if canonical != given {
            spellings.push(canonical);
        }
    }
    spellings
}

impl Drop for OwnedProvisionedHelper {
    fn drop(&mut self) {
        let Ok(raw) = std::fs::read_to_string(self.root.join("endpoint.json")) else {
            return;
        };
        let Some(pid) = serde_json::from_str::<serde_json::Value>(&raw)
            .ok()
            .and_then(|value| value.get("pid").and_then(serde_json::Value::as_u64))
        else {
            return;
        };
        let pid = pid.to_string();
        // `-ww` prints the whole argv instead of truncating it to a screen width.
        let Ok(listed) = std::process::Command::new("ps")
            .args(["-ww", "-p", &pid, "-o", "command="])
            .output()
        else {
            return;
        };
        let command = String::from_utf8_lossy(&listed.stdout);
        let is_ours = path_spellings(&self.executable)
            .iter()
            .any(|exe| command.contains(exe.as_str()))
            && path_spellings(&self.root)
                .iter()
                .any(|root| command.contains(&format!("--root {root}")))
            && command.contains(&format!("--host-id {}", self.host_id));
        if !is_ours {
            return;
        }
        let stop = format!("kill -9 {pid}");
        let _ = std::process::Command::new("sh")
            .args(["-c", &stop])
            .status();
    }
}

#[tokio::test]
async fn remote_ssh_first_spawn_provisions_qualified_helper_automatically() {
    // Short root: the qualified runtime root is `<home>/.ferryx/r/<version>/<host digest>`
    // and the helper binds `<root>/helper.sock`, which must stay under the Unix
    // socket limit (103 bytes on macOS, 107 on Linux).
    let dir = tempfile::Builder::new()
        .prefix("fx")
        .rand_bytes(2)
        .tempdir_in("/tmp")
        .unwrap();
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
    let sshd = sshd_binary();
    assert!(
        sshd.is_file(),
        "Explicit test prerequisite: install an OpenSSH server ({} missing)",
        sshd.display()
    );
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
                    let mut child = tokio::process::Command::new(&sshd)
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
    // The asset fixture must be adjacent to the executable, so this test binary
    // runs from a private copy inside its own temporary directory: the shared
    // `target/debug/deps/helpers` subtree (which may hold foreign artifacts) is
    // never written to and never removed.
    let assets = tempfile::Builder::new()
        .prefix("fx-provision-assets")
        .tempdir_in("/tmp")
        .unwrap();
    let child_binary = assets.path().join("ferryx-lib-provision-tests");
    std::fs::copy(std::env::current_exe().unwrap(), &child_binary).unwrap();
    std::fs::set_permissions(&child_binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let staged = stage_verified_helper_asset(assets.path(), &local_helper_target_triple());
    assert!(
        staged.starts_with(assets.path()),
        "the asset fixture must stay inside its own private directory"
    );

    // The private copy loads the ghostty dylib through the inherited
    // `DYLD_FALLBACK_LIBRARY_PATH` that cargo set for this test process.
    let output = tokio::time::timeout(
        Duration::from_secs(120),
        tokio::process::Command::new(&child_binary)
            .args([
                "--exact",
                "daemon::server::remote_ssh_tests::remote_ssh_first_spawn_provisions_qualified_helper_child",
                "--nocapture",
            ])
            .env(PROVISION_CHILD, dir.path())
            // The daemon supervises its SSH transport through this test binary's ignored
            // `ssh_bridge_transport_supervisor_entry`; the real app dispatches
            // `--ferryx-ssh-supervisor` from its own main instead. Child-owned env only.
            .env("FERRYX_SSH_SUPERVISOR_LIBTEST", "1")
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
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn remote_ssh_first_spawn_provisions_qualified_helper_child() {
    let Some(root) = std::env::var_os(PROVISION_CHILD) else {
        return;
    };
    let root = PathBuf::from(root);
    let port = std::fs::read_to_string(root.join("port"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let user = std::process::Command::new("id").arg("-un").output().unwrap();
    assert!(user.status.success());
    let host = SshHost {
        // Short id on purpose: `host_slug` appends "-" plus 32 hex characters, and the
        // fixture root must stay short enough for the helper's socket path.
        id: "fx".into(),
        label: "Loopback provisioning".into(),
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

    let repository = root.join("remote repository");
    std::fs::create_dir(&repository).unwrap();
    let response = register_remote_project(
        host_store.clone(),
        RegisterRemoteProjectRequest {
            workspace_id: "auto-provision".into(),
            host_id: host.id.clone(),
            repo_path: repository.to_string_lossy().into_owned(),
        },
    )
    .await
    .expect("register the loopback project over real SSH");

    let daemon = DaemonServer::new_with_paths(
        Some(root.join("gateway.json")),
        Some(root.join("auth.json")),
    );
    let mut environment = crate::ssh::runtime::detect(&host).await.unwrap();
    environment.home = root.to_string_lossy().into_owned();
    let qualified = crate::ssh::helper_setup::qualified_location(
        &host,
        &environment,
        crate::ssh::helper_runtime::process::HELPER_VERSION,
    )
    .unwrap();
    let legacy = crate::ssh::helper_setup::default_location(&host, &environment).unwrap();

    // The helper canonicalizes the root before binding, so measure the socket path the
    // same way; failing here beats a cryptic SUN_LEN bind error from the remote.
    let socket_path = Path::new(&qualified.root).join("helper.sock");
    let socket_path = match Path::new(&environment.home).canonicalize() {
        Ok(canonical) => canonical.join(socket_path.strip_prefix(&environment.home).unwrap()),
        Err(_) => socket_path,
    };
    assert!(
        socket_path.to_string_lossy().len() < 104,
        "helper socket path exceeds the Unix socket limit: {}",
        socket_path.display()
    );

    // Armed before anything can provision: a failed assertion or panic after the
    // helper starts still reaps the daemon it launched.
    let _helper = OwnedProvisionedHelper {
        root: PathBuf::from(&qualified.root),
        executable: PathBuf::from(&qualified.executable),
        host_id: host.id.clone(),
    };

    // The first spawn must provision from scratch: nothing is installed yet.
    assert!(
        !Path::new(&qualified.executable).exists(),
        "the qualified helper must be absent before the first spawn"
    );
    assert_ne!(
        crate::ssh::helper_setup::probe_ready_at(&host, &environment, &qualified).await,
        crate::ssh::helper_setup::HelperProbeState::Installed
    );

    let exe_dir = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    assert!(
        !exe_dir.ends_with("deps"),
        "the fixture must run from its private copy, never the shared build directory"
    );
    let target_triple = crate::ipc::ssh::resolve_remote_target_triple(&host, &environment)
        .await
        .expect("resolve the remote helper target");
    assert_eq!(
        target_triple,
        local_helper_target_triple(),
        "the loopback SSH host must report this machine's helper target"
    );
    let expected_asset = staged_asset_path(&exe_dir, &target_triple);
    assert!(
        expected_asset.is_file(),
        "the verified asset fixture must be staged beside the private test binary copy"
    );
    // Production resolution must pick the private fixture: it is the first
    // candidate for the packaged layout, ahead of any bundle installed on this
    // machine and ahead of the repository copy.
    assert_eq!(
        crate::ipc::ssh::resolve_bundled_helper_binary(&target_triple)
            .expect("the staged asset must resolve"),
        expected_asset,
        "asset resolution must pick the verified bundle staged for this test"
    );
    let expected_sha256 = crate::ssh::helper_assets::compute_file_sha256(&expected_asset).unwrap();

    let startup = TerminalStartup::RemoteSsh {
        host_store_path: host_store.clone(),
    };
    let session_id = daemon
        .handle_spawn(
            "auto-provision-first-spawn",
            &response.workspace_id,
            None,
            Some(response.repo_root.clone()),
            80,
            24,
            None,
            Some(startup),
        )
        .await
        .expect("first remote spawn must provision the qualified helper automatically");

    assert_eq!(
        crate::ssh::helper_setup::probe_ready_at(&host, &environment, &qualified).await,
        crate::ssh::helper_setup::HelperProbeState::Installed
    );
    assert_eq!(
        crate::ssh::helper_assets::compute_file_sha256(Path::new(&qualified.executable)).unwrap(),
        expected_sha256,
        "the provisioned helper must be the verified bundled asset"
    );
    assert!(
        !Path::new(&legacy.executable).exists(),
        "provisioning must never fall back to a raw default-location executable"
    );

    wait_remote_connected(&daemon, &session_id).await;
    daemon.handle_close(&session_id).await.unwrap();
}
