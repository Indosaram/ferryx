use super::*;
#[test]
fn live_project_inventory_decodes_at_desktop_boundary() {
    let wire = include_bytes!("../../../scripts/qa/daas-project-response.json");
    serde_json::from_slice::<crate::daemon::protocol::DaemonResponse>(wire).unwrap();
}

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
    let host_id = "https://relay.ferryx.dev/host/m-1";
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

#[test]
fn recover_deleted_project_only_from_unique_ready_same_host_path() {
    let raw: m::Projects = serde_json::from_value(serde_json::json!({
        "revision": "50", "completeness": "complete", "unavailableWorkspaceIds": [],
        "projects": [{ "workspaceId": "new-id", "repoRoot": "/data/daas",
            "gitRoot": null, "gitCommonDir": null, "gitRemote": null,
            "gitBranch": null, "gitHead": null, "availability": "ready", "revision": "50" }]
    })).unwrap();
    let mut rows = projects("host-a", raw);
    let response = crate::daemon::protocol::DaemonResponse::PairedHostOperationOk {
        response: crate::paired_host::client::OperationResponse {
            host_id: "host-a".into(),
            generation: crate::scoped_contracts::Epoch(1),
            result: crate::paired_host::client::OperationResult::Projects(rows.clone()),
        },
    };
    let wire = serde_json::to_vec(&response).unwrap();
    serde_json::from_slice::<crate::daemon::protocol::DaemonResponse>(&wire)
        .expect("project inventory must survive the desktop IPC response boundary");
    assert_eq!(recover_remote_id(&rows, "host-a", "old-id", "/data/daas"), Some("new-id"));
    assert_eq!(recover_remote_id(&rows, "host-b", "old-id", "/data/daas"), None);
    assert_eq!(recover_remote_id(&rows, "host-a", "old-id", "/data/other"), None);
    assert_eq!(recover_remote_id(&rows, "host-a", "new-id", "/data/daas"), None);
    rows.completeness = m::Completeness::Partial;
    assert_eq!(recover_remote_id(&rows, "host-a", "old-id", "/data/daas"), None);
    rows.completeness = m::Completeness::Complete;
    rows.unavailable_workspace_ids.push(rows.projects[0].metadata.workspace_id.clone());
    assert_eq!(recover_remote_id(&rows, "host-a", "old-id", "/data/daas"), None);
    rows.unavailable_workspace_ids.clear();
    rows.projects[0].metadata.availability = m::Availability::Missing;
    assert_eq!(recover_remote_id(&rows, "host-a", "old-id", "/data/daas"), None);
    rows.projects[0].metadata.availability = m::Availability::Ready;
    let mut duplicate = rows.projects[0].clone();
    duplicate.remote_workspace_id = "other-id".into();
    rows.projects.push(duplicate);
    assert_eq!(recover_remote_id(&rows, "host-a", "old-id", "/data/daas"), None);
}
