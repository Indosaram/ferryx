//! This integration target is an external crate, like the standalone QA harness.
use ferryx_lib::ipc::project_remote::{register_remote_project, RegisterRemoteProjectRequest};
use ferryx_lib::ipc::IpcErrorCode;

#[tokio::test]
async fn public_registration_core_accepts_slug_and_canonical_id_without_tauri() {
    let directory = tempfile::tempdir().expect("private host inventory");
    let store = directory.path().join("ssh_hosts.json");
    std::fs::write(&store, br#"{"hosts":[],"tombstones":[]}"#).expect("host inventory");

    for workspace_id in ["qa-slug".to_string(), format!("ssh:{}", "a".repeat(64))] {
        let error = register_remote_project(
            store.clone(),
            RegisterRemoteProjectRequest {
                workspace_id,
                host_id: "missing-host".into(),
                repo_path: "/remote/qa-project".into(),
            },
        )
        .await
        .expect_err("the real stored-host boundary must reject a missing host");

        // A canonical ID sent through the local validator would instead produce
        // UNSUPPORTED. Both inputs must reach the identical validated host lookup.
        assert_eq!(error.code, IpcErrorCode::WorkspaceNotFound);
        assert_eq!(
            error.details.expect("host failure details")["reason"],
            "hostMissing"
        );
    }
    assert!(!directory.path().join("remote_projects.json").exists());
}
