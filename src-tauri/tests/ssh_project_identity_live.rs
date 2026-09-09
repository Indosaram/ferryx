use ferryx_lib::ipc::project_remote::{register_remote_project, RegisterRemoteProjectRequest};
use ferryx_lib::ipc::ssh::SshHostStore;
use ferryx_lib::ssh::{operations, runtime, SshAuthMethod, SshHost, SshHostSource};

#[tokio::test]
#[ignore = "requires FERRYX_IDENTITY_HOST and FERRYX_IDENTITY_PATH; reads an existing remote checkout"]
async fn remote_registration_preserves_git_identity_and_execution_path() {
    let host = SshHost {
        id: "identity-qa".into(),
        label: "Identity QA".into(),
        hostname: std::env::var("FERRYX_IDENTITY_HOST").unwrap(),
        username: None,
        port: None,
        identity_file: None,
        jump_host: None,
        source: SshHostSource::Manual,
        auth_method: SshAuthMethod::Agent,
        disabled: None,
    };
    let path = std::env::var("FERRYX_IDENTITY_PATH").unwrap();
    let environment = runtime::detect(&host).await.unwrap();
    let probed = operations::probe(&host, &environment, &path).await.unwrap();
    assert!(probed.git_root.is_some(), "fixture must be a Git checkout");
    assert!(probed.git_common_dir.is_some(), "Git common directory must be resolved");
    let isolated = tempfile::tempdir().unwrap();
    let store = isolated.path().join("ssh_hosts.json");
    std::fs::write(
        &store,
        serde_json::to_vec(&SshHostStore {
            hosts: vec![host.clone()],
            tombstones: vec![],
        })
        .unwrap(),
    )
    .unwrap();
    let registered = register_remote_project(
        store,
        RegisterRemoteProjectRequest {
            workspace_id: "identity-qa".into(),
            host_id: host.id.clone(),
            repo_path: path,
        },
    )
    .await
    .unwrap();
    assert_eq!(registered.repo_root, probed.0);
    assert_eq!(registered.git_root, probed.1);
    assert_eq!(registered.git_remote, probed.2);
    assert_eq!(registered.git_common_dir, probed.3);
    assert_eq!(registered.host_id, host.id);
    assert_eq!(
        registered.workspace_id,
        ferryx_lib::ssh::projects::identity(&registered.host_id, &registered.repo_root)
    );
    println!(
        "IDENTITY_REGISTRATION_OK {}",
        serde_json::to_string(&registered).unwrap()
    );
}
