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
