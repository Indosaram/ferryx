//! Test-only external-machine entry point. No production trust override exists.
use super::*;
use serde::{Deserialize, Serialize};

pub(super) const CONFIG_ENV: &str = "FERRYX_SSH_QA_CONFIG";
const CHILD_ENV: &str = "FERRYX_SSH_QA_CHILD";
const TEST_NAME: &str =
    "daemon::server::remote_ssh_tests::direct_ssh_real_transport_registration_and_pty";

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct QaConfig {
    pub host: SshHost,
    pub known_hosts_file: PathBuf,
    pub repo_path: String,
    pub expected_repo_root: String,
    pub expected_git_root: Option<String>,
}

pub(super) async fn run(config_path: &Path) {
    assert!(
        config_path.is_absolute(),
        "QA configuration path must be absolute"
    );
    let config: QaConfig =
        serde_json::from_slice(&std::fs::read(config_path).expect("read QA configuration"))
            .expect("parse QA configuration");
    assert!(
        config.known_hosts_file.is_absolute(),
        "QA trust file must be absolute"
    );
    let trust_before = std::fs::read(&config.known_hosts_file).expect("read pinned QA known_hosts");
    assert!(
        !trust_before.is_empty(),
        "QA trust file must contain the independently verified key"
    );
    crate::ssh::direct::validate_host(&config.host).expect("valid QA host");
    crate::ssh::direct::validate_remote_path(&config.repo_path).expect("valid QA remote path");
    if std::env::var_os(CHILD_ENV).is_some() {
        exercise(&config).await;
    } else {
        let launcher = tempfile::tempdir().expect("private QA launcher directory");
        let wrapper = launcher.path().join("ssh");
        std::fs::write(&wrapper, format!(
            "#!/bin/sh\nexec /usr/bin/ssh -F /dev/null -o UserKnownHostsFile={} -o GlobalKnownHostsFile=/dev/null -o StrictHostKeyChecking=yes -o BatchMode=yes -o UpdateHostKeys=no \"$@\"\n",
            crate::ssh::direct::quote_posix(config.known_hosts_file.to_str().expect("UTF-8 trust path")),
        )).expect("write isolated SSH launcher");
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = std::env::var_os("PATH").expect("PATH");
        let mut paths = vec![launcher.path().to_path_buf()];
        paths.extend(std::env::split_paths(&path));
        let output = tokio::time::timeout(
            Duration::from_secs(45),
            tokio::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", TEST_NAME, "--nocapture"])
                .env(CONFIG_ENV, config_path)
                .env(CHILD_ENV, "1")
                .env("PATH", std::env::join_paths(paths).unwrap())
                .kill_on_drop(true)
                .output(),
        )
        .await
        .expect("bounded external SSH QA child")
        .expect("run SSH QA child");
        print!("{}", String::from_utf8_lossy(&output.stdout));
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
        assert!(
            output.status.success(),
            "SSH QA child failed: {}",
            output.status
        );
    }
    assert_eq!(
        std::fs::read(&config.known_hosts_file).unwrap(),
        trust_before,
        "QA must not mutate the pinned trust file"
    );
}

async fn exercise(config: &QaConfig) {
    // All bookkeeping is private to this test. No remote fixtures are created or
    // repositories mutated; VM setup/cleanup belongs to the lead.
    let state = tempfile::Builder::new().prefix("fx").tempdir_in("/tmp").unwrap();
    let host_store = state.path().join("ssh_hosts.json");
    write_hosts(&host_store, vec![config.host.clone()]);
    let response = register_remote_project(
        host_store.clone(),
        RegisterRemoteProjectRequest {
            workspace_id: "qa-project".into(),
            host_id: config.host.id.clone(),
            repo_path: config.repo_path.clone(),
        },
    )
    .await
    .expect("actual backend registration/probe against QA host");
    assert_eq!(response.repo_root, config.expected_repo_root);
    assert_eq!(response.git_root, config.expected_git_root);
    assert_eq!(response.host_id, config.host.id);
    assert_eq!(
        projects::resolve(&host_store, &response.workspace_id)
            .unwrap()
            .0
            .repo_root,
        config.expected_repo_root
    );
    println!(
        "FERRYX_SSH_QA_REGISTERED {}",
        serde_json::to_string(&response).unwrap()
    );
    let mut daemon = DaemonServer::new_with_paths(Some(state.path().join("gateway.json")), Some(state.path().join("auth.json")));
    let _helper = if config.host.hostname == "127.0.0.1" {
        Some(install_test_helper(&config.host, state.path()).await)
    } else {
        // External QA uses its explicitly preinstalled helper, never a local-path fixture.
        daemon.helper_home = None;
        None
    };
    for request in ["qa-new-tab", "qa-split-restore"] {
        let session = daemon
            .handle_spawn(
                request,
                &response.workspace_id,
                None,
                None,
                80,
                24,
                None,
                Some(TerminalStartup::RemoteSsh {
                    host_store_path: host_store.clone(),
                }),
            )
            .await
            .expect("daemon-owned SSH PTY");
        let (mut history, mut events) = daemon.terminal_service.attach(&session).unwrap();
        wait_remote_connected(&daemon, &session).await;
        daemon.write_session_input(&session, b"printf '\\136\\123\\123\\110\\055\\117\\113\\072'; pwd -P\n".to_vec()).await.unwrap();
        let expected = format!("^SSH-OK:{}", config.expected_repo_root);
        let result = tokio::time::timeout(Duration::from_secs(10), async {
            while !String::from_utf8_lossy(&history).contains(&expected) {
                history.extend(events.recv().await.expect("SSH output event"));
            }
        })
        .await;
        daemon
            .handle_close(&session)
            .await
            .expect("close only QA-owned SSH session");
        result.expect("exact remote CWD through SSH PTY, not input echo");
        println!(
            "FERRYX_SSH_QA_PTY_OK {request} {}",
            config.expected_repo_root
        );
    }
    projects::unregister(&host_store, &response.workspace_id).unwrap();
}
