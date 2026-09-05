#![cfg(unix)]
use ferryx_lib::daemon::client::DaemonClient;
use ferryx_lib::daemon::server::DaemonServer;
use ferryx_lib::ipc::{
    cmd_project_branches, cmd_project_initial, cmd_project_register, derive_workspace_id,
    initial_project, IpcErrorCode, ProjectBranchesRequest, RegisterProjectRequest,
    LEGACY_DEFAULT_WORKSPACE_ID,
};
use ferryx_lib::terminal::{
    load_terminal_preferences_from_path, parse_ghostty_config, TerminalPreferencesSource,
    TerminalPreferencesStatus, DEFAULT_TERMINAL_FONT_FAMILY,
};
use ferryx_lib::worktree::{run_git, WorkspaceRegistry, WorktreeIdentity};
use serde_json::Value;
use std::path::Path;
use std::sync::Arc;
use tauri::Manager;
use tempfile::TempDir;
use tokio::net::UnixListener;

#[test]
fn tauri_metadata_uses_rorca_identity_and_generated_icons() {
    let config: Value =
        serde_json::from_str(include_str!("../tauri.conf.json")).expect("parse tauri.conf.json");

    assert_eq!(config["productName"], "Ferryx");
    assert_eq!(config["identifier"], "com.ferryx.app");
    assert_eq!(config["app"]["windows"][0]["title"], "Ferryx");

    let configured_icons = config["bundle"]["icon"]
        .as_array()
        .expect("bundle.icon must be an array")
        .iter()
        .map(|value| value.as_str().expect("icon path must be a string"))
        .collect::<Vec<_>>();
    let expected_icons = [
        "icons/32x32.png",
        "icons/128x128.png",
        "icons/128x128@2x.png",
        "icons/icon.icns",
        "icons/icon.ico",
    ];

    assert_eq!(configured_icons, expected_icons);
    for relative_path in expected_icons {
        assert!(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(relative_path)
                .is_file(),
            "configured icon must exist: {relative_path}"
        );
    }
}

#[test]
fn main_window_has_explicit_titlebar_drag_permission() {
    let capability: Value = serde_json::from_str(include_str!("../capabilities/default.json"))
        .expect("parse default capability");
    let permissions = capability["permissions"]
        .as_array()
        .expect("capability.permissions must be an array")
        .iter()
        .map(|value| value.as_str().expect("permission must be a string"))
        .collect::<Vec<_>>();

    assert!(
        permissions.contains(&"core:window:allow-start-dragging"),
        "the custom titlebar must be allowed to start native window dragging"
    );
}

#[test]
fn ghostty_parser_combines_font_families_and_reads_macos_option_as_alt() {
    let parsed = parse_ghostty_config(
        "# imported Ghostty preferences\nfont-family = JetBrains Mono\nfont-family=Noto Sans KR\nmacos-option-as-alt = true\n",
    )
    .expect("valid Ghostty subset");

    assert!(
        parsed
            .font_family
            .as_deref()
            .unwrap_or_default()
            .contains("JetBrains Mono")
            && parsed
                .font_family
                .as_deref()
                .unwrap_or_default()
                .contains("Noto Sans KR")
    );
    assert_eq!(parsed.macos_option_as_alt, Some(true));
}

#[test]
fn ghostty_parser_handles_quotes_and_macos_option_keywords() {
    let parsed = parse_ghostty_config(
        "font-family = \"MesloLGS NF\"\nfont-family = 'Noto Sans KR'\nmacos-option-as-alt = left\n",
    )
    .expect("valid quoted Ghostty config");

    assert!(
        parsed
            .font_family
            .as_deref()
            .unwrap_or_default()
            .contains("MesloLGS NF")
            && parsed
                .font_family
                .as_deref()
                .unwrap_or_default()
                .contains("Noto Sans KR")
    );
    assert_eq!(parsed.macos_option_as_alt, Some(true));
}

#[test]
fn loads_real_ghostty_config_from_system() {
    let prefs = ferryx_lib::terminal::load_terminal_preferences();
    println!("SYSTEM LOADED: {:?}", prefs);
    if prefs.source == ferryx_lib::terminal::TerminalPreferencesSource::Ghostty {
        assert!(prefs.font_family.contains("MesloLGS NF"));
        assert!(prefs.macos_option_as_alt);
    }
}

#[test]
fn terminal_preferences_use_safe_absent_and_malformed_defaults() {
    let temp = TempDir::new().expect("tempdir");
    let config_path = temp.path().join("ghostty-config");

    let absent = load_terminal_preferences_from_path(&config_path);
    assert_eq!(absent.font_family, DEFAULT_TERMINAL_FONT_FAMILY);
    assert!(!absent.macos_option_as_alt);
    assert_eq!(absent.source, TerminalPreferencesSource::Defaults);
    assert_eq!(absent.status, TerminalPreferencesStatus::Absent);
    assert_eq!(absent.source_path.as_deref(), Some(config_path.as_path()));

    std::fs::write(
        &config_path,
        "font-family = Unsafe Partial Import\nmacos-option-as-alt = definitely-not-a-bool\n",
    )
    .expect("write malformed config");
    let malformed = load_terminal_preferences_from_path(&config_path);
    assert_eq!(malformed.font_family, DEFAULT_TERMINAL_FONT_FAMILY);
    assert!(!malformed.macos_option_as_alt);
    assert_eq!(malformed.source, TerminalPreferencesSource::Defaults);
    assert_eq!(malformed.status, TerminalPreferencesStatus::Malformed);

    let json = serde_json::to_value(malformed).expect("serialize terminal preferences");
    assert!(json.get("fontFamily").is_some());
    assert!(json.get("macosOptionAsAlt").is_some());
    assert!(json.get("sourcePath").is_some());
    assert!(json.get("font_family").is_none());
    assert!(json.get("macos_option_as_alt").is_none());
}

fn setup_git_project() -> TempDir {
    let repo = TempDir::new().expect("repo tempdir");
    run_git(repo.path(), &["init"]).expect("git init");
    run_git(repo.path(), &["config", "user.email", "test@example.com"]).expect("email");
    run_git(repo.path(), &["config", "user.name", "Test User"]).expect("name");
    std::fs::write(repo.path().join("README.md"), "initial\n").expect("README");
    run_git(repo.path(), &["add", "README.md"]).expect("add");
    run_git(repo.path(), &["commit", "-m", "initial commit"]).expect("commit");
    run_git(repo.path(), &["branch", "feature/zeta"]).expect("zeta branch");
    run_git(repo.path(), &["branch", "feature/alpha"]).expect("alpha branch");
    repo
}

/// Spawns an in-process daemon server on an isolated temp socket so the typed
/// project commands can perform their daemon workspace registration.
struct TestDaemon {
    _dir: TempDir,
    client: Arc<DaemonClient>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for TestDaemon {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn spawn_test_daemon() -> TestDaemon {
    let dir = TempDir::new().expect("daemon tempdir");
    let socket_path = dir.path().join("test_daemon.sock");
    let listener = UnixListener::bind(&socket_path).expect("bind unix listener");
    let server = Arc::new(DaemonServer::new());
    let server_clone = Arc::clone(&server);
    let task = tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let s = Arc::clone(&server_clone);
                    tokio::spawn(async move {
                        s.handle_client(stream).await;
                    });
                }
                Err(_) => break,
            }
        }
    });
    TestDaemon {
        _dir: dir,
        client: Arc::new(DaemonClient::new_with_socket(socket_path)),
        task,
    }
}

#[tokio::test]
async fn project_registration_returns_canonical_root_and_lists_local_branches() {
    let repo = setup_git_project();
    let daemon = spawn_test_daemon().await;
    let nested = repo.path().join("nested");
    std::fs::create_dir(&nested).expect("nested dir");
    let canonical_root = repo.path().canonicalize().expect("canonical repo root");
    let current_branch = run_git(repo.path(), &["branch", "--show-current"])
        .expect("current branch")
        .trim()
        .to_string();

    let registry = WorkspaceRegistry::new();
    let app = tauri::test::mock_builder()
        .manage(Arc::clone(&daemon.client))
        .manage(registry)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");
    let registry_state = app.state::<WorkspaceRegistry>();
    let daemon_state = app.state::<Arc<DaemonClient>>();

    let registered = cmd_project_register(
        daemon_state,
        registry_state.clone(),
        RegisterProjectRequest {
            workspace_id: "project-a".into(),
            repo_path: nested,
        },
    )
    .await
    .expect("register project");
    assert_eq!(registered.workspace_id, "project-a");
    assert_eq!(registered.repo_root, canonical_root);

    let registration_json = serde_json::to_value(&registered).expect("serialize registration");
    assert!(registration_json.get("workspaceId").is_some());
    assert!(registration_json.get("repoRoot").is_some());
    assert!(registration_json.get("workspace_id").is_none());

    let branches = cmd_project_branches(
        registry_state,
        ProjectBranchesRequest {
            workspace_id: "project-a".into(),
        },
    )
    .await
    .expect("list local branches");

    let names = branches
        .iter()
        .map(|branch| branch.name.as_str())
        .collect::<Vec<_>>();
    assert!(names.contains(&"feature/alpha"));
    assert!(names.contains(&"feature/zeta"));
    assert!(names.windows(2).all(|pair| pair[0] <= pair[1]));
    assert_eq!(
        branches.iter().filter(|branch| branch.is_current).count(),
        1
    );
    assert!(branches
        .iter()
        .any(|branch| branch.name == current_branch && branch.is_current));

    let branch_json = serde_json::to_value(&branches[0]).expect("serialize branch");
    assert!(branch_json.get("isCurrent").is_some());
    assert!(branch_json.get("is_current").is_none());
}

#[tokio::test]
async fn project_registration_is_idempotent_for_the_same_workspace_and_root() {
    let repo = setup_git_project();
    let daemon = spawn_test_daemon().await;
    let registry = WorkspaceRegistry::new();
    let app = tauri::test::mock_builder()
        .manage(Arc::clone(&daemon.client))
        .manage(registry)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");
    let registry_state = app.state::<WorkspaceRegistry>();
    let daemon_state = app.state::<Arc<DaemonClient>>();
    let request = RegisterProjectRequest {
        workspace_id: "project-a".into(),
        repo_path: repo.path().to_path_buf(),
    };

    let first = cmd_project_register(
        daemon_state.clone(),
        registry_state.clone(),
        request.clone(),
    )
    .await
    .expect("first registration");
    let repeated = cmd_project_register(daemon_state, registry_state, request)
        .await
        .expect("same project registration");

    assert_eq!(repeated, first);
}

/// The checkout the test binary runs from is the canonical startup repository,
/// so `cmd_project_initial` must resolve to `orca-lite` and must not create a
/// second `default` alias for that same root.
#[tokio::test]
async fn initial_project_is_the_single_canonical_checkout_without_a_default_alias() {
    let daemon = spawn_test_daemon().await;
    let registry = WorkspaceRegistry::new();
    let app = tauri::test::mock_builder()
        .manage(Arc::clone(&daemon.client))
        .manage(registry)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");
    let registry_state = app.state::<WorkspaceRegistry>();
    let daemon_state = app.state::<Arc<DaemonClient>>();

    let initial = cmd_project_initial(daemon_state.clone(), registry_state.clone())
        .await
        .expect("initial project");
    println!("initial project: {initial:?}");

    assert_eq!(initial.workspace_id, "orca-lite");
    assert_eq!(
        initial.repo_root,
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repository root")
            .canonicalize()
            .expect("canonical repository root")
    );

    let registered_ids = registry_state
        .list()
        .into_iter()
        .map(|(workspace_id, _)| workspace_id)
        .collect::<Vec<_>>();
    assert_eq!(registered_ids, vec!["orca-lite".to_string()]);
    assert!(
        !registry_state.contains(LEGACY_DEFAULT_WORKSPACE_ID),
        "the startup root must not be registered under a second `default` alias"
    );

    let repeated = cmd_project_initial(daemon_state, registry_state.clone())
        .await
        .expect("repeated initial project");
    assert_eq!(repeated, initial);
    assert_eq!(registry_state.list().len(), 1);

    let json = serde_json::to_value(&initial).expect("serialize initial project");
    assert!(json.get("workspaceId").is_some());
    assert!(json.get("repoRoot").is_some());
    assert!(json.get("workspace_id").is_none());
}

/// Clients that persisted `workspaceId: "default"` migrate by adopting the ID
/// returned by the typed initial-project command. The legacy ID itself stays
/// unregistered and unresolvable, so a stale request fails loudly instead of
/// silently binding to some unrelated workspace.
#[tokio::test]
async fn legacy_default_workspace_id_is_never_registered_and_never_resolves() {
    let registry = WorkspaceRegistry::new();
    let app = tauri::test::mock_builder()
        .manage(registry)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");
    let registry_state = app.state::<WorkspaceRegistry>();

    let initial = initial_project(&registry_state).expect("initial project");
    assert_ne!(initial.workspace_id, LEGACY_DEFAULT_WORKSPACE_ID);

    // The canonical ID a migrating client adopts is immediately usable.
    cmd_project_branches(
        registry_state.clone(),
        ProjectBranchesRequest {
            workspace_id: initial.workspace_id.clone(),
        },
    )
    .await
    .expect("canonical branch listing");

    let error = cmd_project_branches(
        registry_state.clone(),
        ProjectBranchesRequest {
            workspace_id: LEGACY_DEFAULT_WORKSPACE_ID.into(),
        },
    )
    .await
    .expect_err("the legacy `default` alias must no longer resolve");
    assert_eq!(error.code, IpcErrorCode::WorkspaceNotFound);

    assert!(!registry_state.contains(LEGACY_DEFAULT_WORKSPACE_ID));
    assert_eq!(registry_state.list().len(), 1);
}

/// One canonical root may hold only one workspace ID. A second registration
/// for the same root returns the existing project rather than minting an alias,
/// which is what stops a persisted client from re-creating `default`.
#[tokio::test]
async fn project_registration_enforces_one_workspace_id_per_canonical_root() {
    let repo = setup_git_project();
    let daemon = spawn_test_daemon().await;
    let nested = repo.path().join("nested");
    std::fs::create_dir(&nested).expect("nested dir");
    let registry = WorkspaceRegistry::new();
    let app = tauri::test::mock_builder()
        .manage(Arc::clone(&daemon.client))
        .manage(registry)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");
    let registry_state = app.state::<WorkspaceRegistry>();
    let daemon_state = app.state::<Arc<DaemonClient>>();

    let first = cmd_project_register(
        daemon_state.clone(),
        registry_state.clone(),
        RegisterProjectRequest {
            workspace_id: "project-a".into(),
            repo_path: repo.path().to_path_buf(),
        },
    )
    .await
    .expect("first registration");
    assert_eq!(first.workspace_id, "project-a");

    // A different ID, and a nested path inside the same root, both collapse
    // onto the already-registered project.
    for (requested_id, repo_path) in [
        ("project-b", repo.path().to_path_buf()),
        (LEGACY_DEFAULT_WORKSPACE_ID, nested.clone()),
    ] {
        let duplicate = cmd_project_register(
            daemon_state.clone(),
            registry_state.clone(),
            RegisterProjectRequest {
                workspace_id: requested_id.into(),
                repo_path,
            },
        )
        .await
        .expect("duplicate root registration resolves to the canonical project");
        assert_eq!(
            duplicate, first,
            "requested id {requested_id:?} must not alias"
        );
        assert!(!registry_state.contains(requested_id));
    }

    let registered_ids = registry_state
        .list()
        .into_iter()
        .map(|(workspace_id, _)| workspace_id)
        .collect::<Vec<_>>();
    assert_eq!(registered_ids, vec!["project-a".to_string()]);

    // Distinct roots still register independently.
    let other_repo = setup_git_project();
    let other = cmd_project_register(
        daemon_state,
        registry_state.clone(),
        RegisterProjectRequest {
            workspace_id: "project-b".into(),
            repo_path: other_repo.path().to_path_buf(),
        },
    )
    .await
    .expect("a distinct root registers under its own id");
    assert_eq!(other.workspace_id, "project-b");
    assert_eq!(registry_state.list().len(), 2);

    // If another repository also requests "project-a", it disambiguates instead of crashing
    let third_repo = setup_git_project();
    let third = cmd_project_register(
        app.state::<Arc<DaemonClient>>(),
        registry_state.clone(),
        RegisterProjectRequest {
            workspace_id: "project-a".into(),
            repo_path: third_repo.path().to_path_buf(),
        },
    )
    .await
    .expect("colliding id disambiguates to a unique slug");
    assert_eq!(third.workspace_id, "project-a-2");
    assert_eq!(registry_state.list().len(), 3);
}

#[test]
fn workspace_ids_derive_safely_from_repository_folder_names() {
    assert_eq!(
        derive_workspace_id(Path::new("/tmp/orca-lite")),
        "orca-lite"
    );
    assert_eq!(
        derive_workspace_id(Path::new("/tmp/My Project")),
        "My-Project"
    );
    assert_eq!(derive_workspace_id(Path::new("/tmp/--")), "project");
    assert_eq!(derive_workspace_id(Path::new("/")), "project");

    // Every derived ID must be accepted by the registry's own validation.
    let repo = setup_git_project();
    let registry = WorkspaceRegistry::new();
    for folder in [
        "orca-lite",
        "My Project",
        "weird/name",
        "-leading-dash-",
        "..",
    ] {
        let derived = derive_workspace_id(&Path::new("/tmp").join(folder));
        registry
            .register(&derived, repo.path())
            .unwrap_or_else(|error| panic!("derived id {derived:?} must be registrable: {error}"));
        assert!(registry.contains(&derived));
    }
}

#[test]
fn foreign_identity_ws_id_cannot_escape_the_registered_repo_root() {
    let repo = setup_git_project();
    let registry = WorkspaceRegistry::new();
    registry
        .register("workspace-a", repo.path())
        .expect("register workspace-a");

    // A ws_id that does not match the registered workspace ID is accepted by
    // design (worktrees created under a previous ID must stay resolvable), but
    // it must never resolve a path outside the registered root.
    for hostile in ["../../etc", "..", "a/b", "-x"] {
        let identity = WorktreeIdentity {
            ws_id: hostile.to_string(),
            slug: "task".into(),
        };
        match registry.target_path("workspace-a", &identity) {
            Ok(path) => panic!("hostile ws_id {hostile:?} produced a path: {path:?}"),
            Err(_) => {}
        }
    }

    let benign = WorktreeIdentity {
        ws_id: "previous-id".into(),
        slug: "task".into(),
    };
    let path = registry
        .target_path("workspace-a", &benign)
        .expect("a differently-named ws_id still resolves inside the root");
    let root = std::fs::canonicalize(repo.path()).expect("canonical root");
    assert!(
        path.starts_with(&root),
        "resolved path {path:?} escaped root {root:?}"
    );
}

#[test]
fn registry_binds_one_workspace_id_per_canonical_root() {
    let repo = TempDir::new().expect("repo tempdir");
    let registry = WorkspaceRegistry::new();

    let first = registry
        .register_unique_root("first", repo.path())
        .expect("first registration succeeds");
    let second = registry
        .register_unique_root("second", repo.path())
        .expect("a second registration resolves to the existing owner");

    assert_eq!(first, "first");
    assert_eq!(
        second, "first",
        "a canonical root must not gain a second ID"
    );
    assert_eq!(registry.list().len(), 1);
}

#[tokio::test]
async fn project_registration_accepts_non_git_roots_and_rejects_unregistered_branch_queries() {
    let non_repo = TempDir::new().expect("non-repo tempdir");
    let daemon = spawn_test_daemon().await;
    let registry = WorkspaceRegistry::new();
    let app = tauri::test::mock_builder()
        .manage(Arc::clone(&daemon.client))
        .manage(registry)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");
    let registry_state = app.state::<WorkspaceRegistry>();
    let daemon_state = app.state::<Arc<DaemonClient>>();

    // Plain (non-Git) folders register as terminal-only workspaces.
    let registered = cmd_project_register(
        daemon_state,
        registry_state.clone(),
        RegisterProjectRequest {
            workspace_id: "plain-project".into(),
            repo_path: non_repo.path().to_path_buf(),
        },
    )
    .await
    .expect("plain folders register successfully");
    assert_eq!(registered.git_root, None);

    // Plain workspaces have no branches; the dialog must receive an empty list
    // instead of a raw git failure.
    let branches = cmd_project_branches(
        registry_state.clone(),
        ProjectBranchesRequest {
            workspace_id: "plain-project".into(),
        },
    )
    .await
    .expect("plain workspaces report zero branches");
    assert!(branches.is_empty());

    let error = cmd_project_branches(
        registry_state,
        ProjectBranchesRequest {
            workspace_id: "missing-project".into(),
        },
    )
    .await
    .expect_err("unregistered projects must not accept raw branch queries");
    assert_eq!(error.code, IpcErrorCode::WorkspaceNotFound);
}

#[test]
fn ghostty_theme_resolution_merges_palette_prefers_dark_and_yields_to_explicit_colors() {
    let temp = TempDir::new().expect("tempdir");
    let themes = temp.path().join("ghostty").join("themes");
    std::fs::create_dir_all(&themes).expect("create theme dir");
    std::fs::write(
        themes.join("ferryx-test-theme"),
        "palette = 0=#111111\npalette = 1=#222222\nbackground = #303030\nforeground = #f0f0f0\ncursor-color = #abcdef\n",
    )
    .expect("write theme");
    std::fs::write(themes.join("Bright Day"), "background = #ffffff\n").expect("write light theme");
    std::fs::write(themes.join("Deep Night"), "background = #101010\n").expect("write dark theme");

    let named_path = temp.path().join("config-named");
    std::fs::write(
        &named_path,
        "theme = ferryx-test-theme\nbackground = #000000\n",
    )
    .expect("write named-theme config");
    let dual_path = temp.path().join("config-dual");
    std::fs::write(&dual_path, "theme = light:Bright Day,dark:Deep Night\n")
        .expect("write dual-theme config");

    // Serialized inside one test: XDG_CONFIG_HOME is process-global, so parallel tests would race.
    let previous = std::env::var_os("XDG_CONFIG_HOME");
    std::env::set_var("XDG_CONFIG_HOME", temp.path());
    let named = load_terminal_preferences_from_path(&named_path);
    let dual = load_terminal_preferences_from_path(&dual_path);
    match previous {
        Some(value) => std::env::set_var("XDG_CONFIG_HOME", value),
        None => std::env::remove_var("XDG_CONFIG_HOME"),
    }

    assert_eq!(named.theme.black, "#111111");
    assert_eq!(named.theme.red, "#222222");
    assert_eq!(named.theme.foreground, "#f0f0f0");
    assert_eq!(named.theme.cursor, "#abcdef");
    assert_eq!(
        named.theme.background, "#000000",
        "explicit config colors must override theme colors"
    );

    assert_eq!(
        dual.theme.background, "#101010",
        "the dark variant applies to the dark terminal surface"
    );
}

#[test]
fn local_overrides_replace_imported_font_and_option_as_alt() {
    let temp = TempDir::new().expect("tempdir");
    let config_path = temp.path().join("config");
    std::fs::write(
        &config_path,
        "font-family = MesloLGS NF\nfont-size = 13\nmacos-option-as-alt = true\n",
    )
    .expect("write config");
    let imported = load_terminal_preferences_from_path(&config_path);
    assert_eq!(imported.font_size, 13.0);
    assert!(imported.macos_option_as_alt);

    let effective = ferryx_lib::terminal::apply_terminal_preference_overrides(
        &imported,
        &ferryx_lib::terminal::TerminalPreferenceOverrides {
            shell: None,
            font_family: Some("JetBrains Mono".into()),
            font_size: Some(17.0),
            macos_option_as_alt: Some(false),
        },
    );
    assert_eq!(effective.font_family, "JetBrains Mono");
    assert_eq!(effective.font_size, 17.0);
    assert!(!effective.macos_option_as_alt);

    let untouched = ferryx_lib::terminal::apply_terminal_preference_overrides(
        &imported,
        &ferryx_lib::terminal::TerminalPreferenceOverrides::default(),
    );
    assert_eq!(untouched, imported);

    let rejected = ferryx_lib::terminal::apply_terminal_preference_overrides(
        &imported,
        &ferryx_lib::terminal::TerminalPreferenceOverrides {
            shell: None,
            font_family: Some("   ".into()),
            font_size: Some(0.0),
            macos_option_as_alt: None,
        },
    );
    assert_eq!(
        rejected, imported,
        "blank/non-positive overrides are ignored"
    );
}
