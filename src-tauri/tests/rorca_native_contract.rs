use orca_lite_lib::ipc::{
    cmd_project_branches, cmd_project_register, IpcErrorCode, ProjectBranchesRequest,
    RegisterProjectRequest,
};
use orca_lite_lib::terminal::{
    load_terminal_preferences_from_path, parse_ghostty_config, TerminalPreferencesSource,
    TerminalPreferencesStatus, DEFAULT_TERMINAL_FONT_FAMILY,
};
use orca_lite_lib::worktree::{run_git, WorkspaceRegistry};
use serde_json::Value;
use std::path::Path;
use tauri::Manager;
use tempfile::TempDir;

#[test]
fn tauri_metadata_uses_rorca_identity_and_generated_icons() {
    let config: Value =
        serde_json::from_str(include_str!("../tauri.conf.json")).expect("parse tauri.conf.json");

    assert_eq!(config["productName"], "rorca");
    assert_eq!(config["app"]["windows"][0]["title"], "rorca");

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

    assert_eq!(
        parsed.font_family.as_deref(),
        Some("\"JetBrains Mono\", \"Noto Sans KR\"")
    );
    assert_eq!(parsed.macos_option_as_alt, Some(true));
}

#[test]
fn ghostty_parser_handles_quotes_and_macos_option_keywords() {
    let parsed = parse_ghostty_config(
        "font-family = \"MesloLGS NF\"\nfont-family = 'Noto Sans KR'\nmacos-option-as-alt = left\n",
    )
    .expect("valid quoted Ghostty config");

    assert_eq!(
        parsed.font_family.as_deref(),
        Some("\"MesloLGS NF\", \"Noto Sans KR\"")
    );
    assert_eq!(parsed.macos_option_as_alt, Some(true));
}

#[test]
fn loads_real_ghostty_config_from_system() {
    let prefs = orca_lite_lib::terminal::load_terminal_preferences();
    println!("SYSTEM LOADED: {:?}", prefs);
    if prefs.source == orca_lite_lib::terminal::TerminalPreferencesSource::Ghostty {
        assert_eq!(prefs.font_family, "\"MesloLGS NF\", \"Noto Sans KR\"");
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

#[tokio::test]
async fn project_registration_returns_canonical_root_and_lists_local_branches() {
    let repo = setup_git_project();
    let nested = repo.path().join("nested");
    std::fs::create_dir(&nested).expect("nested dir");
    let canonical_root = repo.path().canonicalize().expect("canonical repo root");
    let current_branch = run_git(repo.path(), &["branch", "--show-current"])
        .expect("current branch")
        .trim()
        .to_string();

    let registry = WorkspaceRegistry::new();
    let app = tauri::test::mock_builder()
        .manage(registry)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");
    let registry_state = app.state::<WorkspaceRegistry>();

    let registered = cmd_project_register(
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
    let registry = WorkspaceRegistry::new();
    let app = tauri::test::mock_builder()
        .manage(registry)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");
    let registry_state = app.state::<WorkspaceRegistry>();
    let request = RegisterProjectRequest {
        workspace_id: "project-a".into(),
        repo_path: repo.path().to_path_buf(),
    };

    let first = cmd_project_register(registry_state.clone(), request.clone())
        .await
        .expect("first registration");
    let repeated = cmd_project_register(registry_state, request)
        .await
        .expect("same project registration");

    assert_eq!(repeated, first);
}

#[tokio::test]
async fn project_registration_rejects_non_git_roots_and_unregistered_branch_queries() {
    let non_repo = TempDir::new().expect("non-repo tempdir");
    let registry = WorkspaceRegistry::new();
    let app = tauri::test::mock_builder()
        .manage(registry)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");
    let registry_state = app.state::<WorkspaceRegistry>();

    let error = cmd_project_register(
        registry_state.clone(),
        RegisterProjectRequest {
            workspace_id: "bad-project".into(),
            repo_path: non_repo.path().to_path_buf(),
        },
    )
    .await
    .expect_err("non-Git roots must be rejected");
    assert_eq!(error.code, IpcErrorCode::InvalidRepoRoot);

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
