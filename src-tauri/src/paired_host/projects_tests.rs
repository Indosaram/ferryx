use super::*;
#[test]
fn metadata_and_unavailable_identity_are_lossless() {
    let raw: m::Projects=serde_json::from_value(serde_json::json!({"revision":"9","completeness":"partial","projects":[{"workspaceId":"same","repoRoot":"/same","gitRoot":null,"gitCommonDir":null,"gitRemote":null,"gitBranch":null,"gitHead":null,"availability":"missing","revision":"8"}],"unavailableWorkspaceIds":["same"]})).unwrap();
    let a = projects("https://relay/host/a", raw.clone());
    let b = projects("https://relay/host/b", raw.clone());
    assert_ne!(
        a.projects[0].metadata.workspace_id,
        b.projects[0].metadata.workspace_id
    );
    assert_eq!(
        a.unavailable_workspace_ids[0],
        a.projects[0].metadata.workspace_id
    );
    let mut restored = a.projects[0].metadata.clone();
    restored.workspace_id = a.projects[0].remote_workspace_id.clone();
    assert_eq!(restored, raw.projects[0]);
}

#[test]
fn stored_paired_projects_persist_and_resolve() {
    let dir = tempfile::tempdir().unwrap();
    let raw: m::Projects = serde_json::from_value(serde_json::json!({
        "revision": "1",
        "completeness": "complete",
        "projects": [{
            "workspaceId": "remote-ws-1",
            "repoRoot": "/home/user/project",
            "gitRoot": null,
            "gitCommonDir": null,
            "gitRemote": null,
            "gitBranch": null,
            "gitHead": null,
            "availability": "ready",
            "revision": "1"
        }],
        "unavailableWorkspaceIds": []
    }))
    .unwrap();
    let host_id = "https://relay.checka.cc/host/m-1";
    let p = project(host_id, raw.projects[0].clone());
    let desktop_id = p.metadata.workspace_id.clone();
    assert!(resolve_stored_project(dir.path(), &desktop_id).is_none());
    save_stored_project(dir.path(), p.clone()).unwrap();
    let resolved =
        resolve_stored_project(dir.path(), &desktop_id).expect("must resolve stored project");
    assert_eq!(resolved.remote_workspace_id, "remote-ws-1");
    assert_eq!(
        resolved.target,
        RunTarget::PairedDaemon {
            host_id: host_id.into()
        }
    );
    assert_eq!(resolved.metadata.repo_root, "/home/user/project");
}
