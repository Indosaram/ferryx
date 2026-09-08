use ferryx_lib::ipc::project_remote::{register_remote_project, RegisterRemoteProjectRequest};
use ferryx_lib::ipc::ssh::SshHostStore;
use ferryx_lib::ssh::browse::{list_directories, ListDirectoriesRequest};
use ferryx_lib::ssh::{projects, SshAuthMethod, SshHost, SshHostSource};

#[tokio::test]
#[ignore = "requires FERRYX_SSH_BROWSE_HOST with trusted noninteractive SSH authentication"]
async fn browse_home_child_parent_and_register_without_remote_writes() {
    let hostname = std::env::var("FERRYX_SSH_BROWSE_HOST").expect("explicit QA SSH host");
    let (fixture, store) = tokio::task::spawn_blocking(move || {
        let fixture = tempfile::tempdir().unwrap();
        let store = fixture.path().join("ssh_hosts.json");
        let host = SshHost {
            id: "browse-live".into(),
            label: "Browse live QA".into(),
            hostname,
            username: None,
            port: None,
            identity_file: None,
            jump_host: None,
            source: SshHostSource::Manual,
            auth_method: SshAuthMethod::Agent,
            disabled: None,
        };
        std::fs::write(
            &store,
            serde_json::to_vec(&SshHostStore {
                hosts: vec![host],
                tombstones: vec![],
            })
            .unwrap(),
        )
        .unwrap();
        (fixture, store)
    })
    .await
    .unwrap();
    let request = |path| ListDirectoriesRequest {
        host_id: "browse-live".into(),
        path,
    };
    let home = list_directories(store.clone(), request(None))
        .await
        .unwrap();
    assert!(!home.path.is_empty());
    assert!(!home.entries.is_empty());
    let child = home
        .entries
        .iter()
        .find(|entry| !entry.hidden)
        .expect("a visible home directory");
    let listing = list_directories(store.clone(), request(Some(child.path.clone())))
        .await
        .unwrap();
    let registered = register_remote_project(
        store.clone(),
        RegisterRemoteProjectRequest {
            workspace_id: "browse-live-project".into(),
            host_id: "browse-live".into(),
            repo_path: listing.path.clone(),
        },
    )
    .await
    .unwrap();
    assert_eq!(registered.repo_root, listing.path);
    assert_eq!(
        registered.workspace_id,
        projects::identity("browse-live", &listing.path)
    );
    let expected_parent = listing.parent_path.expect("selected child has a parent");
    let parent = list_directories(store.clone(), request(Some(expected_parent.clone())))
        .await
        .unwrap();
    assert_eq!(parent.path, expected_parent);
    let home_again = list_directories(store.clone(), request(Some("~".into())))
        .await
        .unwrap();
    assert_eq!(home_again.path, home.path);
    tokio::task::spawn_blocking(move || {
        assert!(projects::resolve(&store, &registered.workspace_id).is_ok());
        drop(fixture);
    })
    .await
    .unwrap();
    println!("SSH_BROWSE_HOME_CHILD_PARENT_REGISTRATION_OK");
}
