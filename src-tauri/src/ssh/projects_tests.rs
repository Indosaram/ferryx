use super::*;
use crate::ssh::{SshAuthMethod, SshHostSource};
use crate::worktree::WorkspaceRegistry;

pub(crate) fn fixture() -> (tempfile::TempDir, PathBuf, SshHost) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ssh_hosts.json");
    let host = SshHost {
        id: "h1".into(),
        label: "Host".into(),
        hostname: "127.0.0.1".into(),
        username: None,
        port: None,
        identity_file: None,
        jump_host: None,
        source: SshHostSource::Manual,
        auth_method: SshAuthMethod::Agent,
        disabled: None,
    };
    write_hosts(&path, vec![host.clone()]);
    (dir, path, host)
}

pub(crate) fn write_hosts(path: &Path, hosts: Vec<SshHost>) {
    std::fs::write(
        path,
        serde_json::to_vec(&SshHostStore {
            hosts,
            tombstones: vec![],
        })
        .unwrap(),
    )
    .unwrap();
}

pub(crate) fn project(host: &SshHost, root: &str) -> RemoteProject {
    RemoteProject {
        workspace_id: identity(&host.id, root),
        host_id: host.id.clone(),
        repo_root: root.into(),
        git_root: None,
        git_remote: None,
        git_branch: None,
        git_head: None,
        platform: None,
    }
}

#[test]
fn lead_regression_local_and_remote_same_path_can_reveal_locally() {
    let (dir, path, host) = fixture();
    let canonical = dir.path().canonicalize().unwrap();
    let root = canonical.to_str().unwrap();
    let registry = WorkspaceRegistry::new();
    registry.register("local", &canonical).unwrap();
    let remote = persist(&path, project(&host, root)).unwrap();
    assert_eq!(
        registry.repo_root("local").unwrap(),
        PathBuf::from(&remote.repo_root)
    );
    assert!(guard_reveal(Some("local")).is_ok());
    assert!(guard_reveal(None).is_ok());
    assert_eq!(
        guard_reveal(Some(&remote.workspace_id)).unwrap_err().code,
        IpcErrorCode::Unsupported
    );
}

#[test]
fn lead_regression_remote_root_does_not_block_legacy_local_reveal() {
    let (_dir, path, host) = fixture();
    persist(&path, project(&host, "/")).unwrap();
    persist(&path, project(&host, "/Users/admin")).unwrap();
    assert!(guard_reveal(None).is_ok());
    // Even broken SSH bookkeeping must not affect a legacy local reveal.
    std::fs::write(store_path(&path), b"broken").unwrap();
    assert!(guard_reveal(None).is_ok());
}

#[test]
fn lead_regression_identity_uses_fixed_u64_little_endian_byte_length() {
    let expected = Sha256::digest(b"\x03\0\0\0\0\0\0\0\xc3\xa9a/root");
    assert_eq!(identity("\u{e9}a", "/root"), format!("ssh:{expected:x}"));
}

#[test]
fn stored_routing_survives_reload_and_refuses_disabled_or_deleted_host() {
    let (_dir, path, mut host) = fixture();
    let project = persist(&path, project(&host, "/remote path")).unwrap();
    assert_eq!(
        resolve(&path, &project.workspace_id).unwrap(),
        (project.clone(), host.clone())
    );
    host.disabled = Some(true);
    write_hosts(&path, vec![host]);
    let error = resolve(&path, &project.workspace_id).unwrap_err();
    assert_eq!(error.details.unwrap()["reason"], "hostDisabled");
    write_hosts(&path, vec![]);
    assert_eq!(
        resolve(&path, &project.workspace_id)
            .unwrap_err()
            .details
            .unwrap()["reason"],
        "hostMissing"
    );
}

#[test]
fn identity_is_host_qualified_idempotent_and_cannot_collide_with_local_registry() {
    let (dir, path, mut host) = fixture();
    let first = persist(&path, project(&host, "/same/path")).unwrap();
    assert_eq!(persist(&path, first.clone()).unwrap(), first);
    let original = host.clone();
    host.id = "h2".into();
    write_hosts(&path, vec![original, host.clone()]);
    let second = persist(&path, project(&host, "/same/path")).unwrap();
    assert_ne!(first.workspace_id, second.workspace_id);
    let registry = WorkspaceRegistry::new();
    assert!(registry.register(&first.workspace_id, dir.path()).is_err());
    assert!(registry
        .register_unique_root(&first.workspace_id, dir.path())
        .is_err());
    registry.register("local", dir.path()).unwrap();
    assert!(registry.manager("local").is_ok());
    assert_eq!(
        IpcError::from(registry.manager(&first.workspace_id).unwrap_err()).code,
        IpcErrorCode::Unsupported
    );
}

#[test]
fn corrupted_metadata_and_identity_collision_fail_closed_without_overwrite() {
    let (_dir, path, host) = fixture();
    let first = project(&host, "/root");
    persist(&path, first.clone()).unwrap();
    let mut conflicting = first.clone();
    conflicting.repo_root = "/different".into();
    assert_eq!(
        persist(&path, conflicting).unwrap_err().code,
        IpcErrorCode::WorkspaceAlreadyRegistered
    );
    assert_eq!(resolve(&path, &first.workspace_id).unwrap().0, first);
    std::fs::write(store_path(&path), b"broken").unwrap();
    assert_eq!(
        persist(&path, first).unwrap_err().code,
        IpcErrorCode::ParseError
    );
    assert_eq!(std::fs::read(store_path(&path)).unwrap(), b"broken");
}

#[test]
fn disabled_host_registration_leaves_no_project_or_temporary_files() {
    let (dir, path, mut host) = fixture();
    host.disabled = Some(true);
    write_hosts(&path, vec![host.clone()]);
    assert!(persist(&path, project(&host, "/root")).is_err());
    assert!(!store_path(&path).exists());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn unregister_revokes_stored_routing_and_reveal_is_explicitly_unsupported() {
    let (_dir, path, host) = fixture();
    let project = persist(&path, project(&host, "/root")).unwrap();
    assert_eq!(
        guard_reveal(Some(&project.workspace_id)).unwrap_err().code,
        IpcErrorCode::Unsupported
    );
    assert!(guard_reveal(Some("local-other")).is_ok());
    unregister(&path, &project.workspace_id).unwrap();
    assert_eq!(
        resolve(&path, &project.workspace_id).unwrap_err().code,
        IpcErrorCode::WorkspaceNotFound
    );
}

#[tokio::test]
async fn connection_failure_does_not_register_or_fall_back_to_existing_local_directory() {
    let (dir, path, mut host) = fixture();
    let socket = tokio::net::TcpSocket::new_v4().unwrap();
    socket.bind("127.0.0.1:0".parse().unwrap()).unwrap();
    host.port = Some(socket.local_addr().unwrap().port());
    write_hosts(&path, vec![host.clone()]);
    let error = crate::ipc::project_remote::register_remote_project(
        path.clone(),
        crate::ipc::project_remote::RegisterRemoteProjectRequest {
            workspace_id: "project".into(),
            host_id: host.id,
            repo_path: dir.path().to_str().unwrap().into(),
        },
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, IpcErrorCode::IoError);
    assert!(!store_path(&path).exists());
}

#[test]
fn failed_atomic_save_cleans_temporary_file() {
    let (dir, path, host) = fixture();
    std::fs::create_dir(store_path(&path)).unwrap();
    let entries = BTreeMap::from([(identity(&host.id, "/root"), project(&host, "/root"))]);
    assert_eq!(
        save(&path, &entries).unwrap_err().code,
        IpcErrorCode::IoError
    );
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
}

#[test]
fn persisted_resolve_and_enabled_host_succeeds_with_legacy_keys_mixed_keys_and_unrelated_malformed_entry() {
    let dir = tempfile::tempdir().unwrap();
    let host_store = dir.path().join("ssh_hosts.json");

    // Raw JSON containing malformed entry, legacy entry, and mixed-key entry
    let raw_hosts_json = r#"{
        "hosts": [
            {
                "invalidField": true,
                "port": "not-a-number"
            },
            {
                "id": "omaki-1",
                "name": "omaki",
                "host": "100.91.254.71",
                "user": "indo",
                "port": 22,
                "authMethod": "agent",
                "source": "manual",
                "remoteContinuity": "on"
            },
            {
                "id": "mixed-1",
                "label": "CanonicalBox",
                "name": { "display": "old metadata" },
                "user": 999,
                "hostname": "127.0.0.1",
                "authMethod": "agent"
            }
        ],
        "tombstones": []
    }"#;
    std::fs::write(&host_store, raw_hosts_json.as_bytes()).unwrap();

    // 1. enabled_host succeeds for legacy entry despite malformed entry at index 0
    let omaki = enabled_host(&host_store, "omaki-1").expect("resolve omaki host");
    assert_eq!(omaki.id, "omaki-1");
    assert_eq!(omaki.label, "omaki");
    assert_eq!(omaki.hostname, "100.91.254.71");
    assert_eq!(omaki.username.as_deref(), Some("indo"));

    // 2. enabled_host succeeds for mixed-key entry preferring canonical label
    let mixed = enabled_host(&host_store, "mixed-1").expect("resolve mixed host");
    assert_eq!(mixed.id, "mixed-1");
    assert_eq!(mixed.label, "CanonicalBox");
    assert_eq!(mixed.hostname, "127.0.0.1");

    // 3. projects::resolve works end-to-end with legacy host and with unknown fields in RemoteProject
    let ws_id = identity("mixed-1", "/srv/repo");
    let raw_projects_json = serde_json::json!({
        ws_id.clone(): {
            "workspaceId": ws_id.clone(),
            "hostId": "mixed-1",
            "repoRoot": "/srv/repo",
            "gitRoot": null,
            "platform": "posix",
            "unknownFutureField": "resilient"
        }
    });
    std::fs::write(
        store_path(&host_store),
        serde_json::to_vec_pretty(&raw_projects_json).unwrap(),
    )
    .unwrap();

    let (resolved_project, resolved_host) = resolve(&host_store, &ws_id).expect("resolve project with extra fields");
    assert_eq!(resolved_project.workspace_id, ws_id);
    assert_eq!(resolved_project.host_id, "mixed-1");
    assert_eq!(resolved_host.label, "CanonicalBox");
}
