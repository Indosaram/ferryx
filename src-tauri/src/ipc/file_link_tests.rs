//! Behaviour contract for local desktop terminal file links.
//!
//! Every test here is desktop-safe: no editor, browser or OS opener is ever
//! launched. Launch plans are asserted as data, and the one execution test
//! points at a path that cannot exist.
use super::*;
use crate::daemon::protocol::DaemonSessionDetails;
use crate::ipc::error::IpcErrorCode;

fn details(workspace_id: Option<&str>, cwd: Option<&str>) -> DaemonSessionDetails {
    DaemonSessionDetails {
        session_id: "backend-1".into(),
        workspace_id: workspace_id.map(str::to_string),
        worktree: None,
        cwd: cwd.map(str::to_string),
        cols: 80,
        rows: 24,
        running: true,
        start_sequence: None,
        end_sequence: None,
    }
}

fn no_cli(_: EditorTarget) -> Option<PathBuf> {
    None
}

#[test]
fn editor_argument_parses_supported_values_and_rejects_others() {
    assert_eq!(EditorTarget::parse(None).unwrap(), EditorTarget::System);
    assert_eq!(
        EditorTarget::parse(Some("system")).unwrap(),
        EditorTarget::System
    );
    assert_eq!(
        EditorTarget::parse(Some("vscode")).unwrap(),
        EditorTarget::VsCode
    );
    assert_eq!(
        EditorTarget::parse(Some("cursor")).unwrap(),
        EditorTarget::Cursor
    );
    assert_eq!(EditorTarget::parse(Some("zed")).unwrap(), EditorTarget::Zed);

    let error = EditorTarget::parse(Some("emacs")).expect_err("unknown editor must be rejected");
    assert_eq!(error.code, IpcErrorCode::InvalidArgument);
    assert_eq!(
        error.details.as_ref().and_then(|d| d["editor"].as_str()),
        Some("emacs")
    );
}

#[test]
fn path_token_is_trimmed_and_empty_or_nul_tokens_are_rejected() {
    assert_eq!(sanitize_path_token("  'src/app.rs'  ").unwrap(), "src/app.rs");
    assert_eq!(sanitize_path_token("\"C:\\a b\\x.rs\"").unwrap(), "C:\\a b\\x.rs");
    assert_eq!(
        sanitize_path_token("   ").expect_err("empty token").code,
        IpcErrorCode::InvalidArgument
    );
    assert_eq!(
        sanitize_path_token("src/a\0b.rs")
            .expect_err("NUL token")
            .code,
        IpcErrorCode::InvalidArgument
    );
}

#[test]
fn remote_host_paths_are_refused_and_local_at_signs_are_kept() {
    for remote in [
        "ssh://build-host/srv/app/main.rs",
        "file://remote/share/x.rs",
        "deploy@build-host:/srv/app/main.rs",
    ] {
        let error = reject_remote_path_spec(remote).expect_err("remote path must not open locally");
        assert_eq!(error.code, IpcErrorCode::Unsupported, "for {remote}");
    }

    for local in [
        "/tmp/user@corp/notes.txt",
        "C:\\Users\\me@corp\\notes.txt",
        "@scope/pkg/index.ts",
        "src/app.rs",
    ] {
        reject_remote_path_spec(local).unwrap_or_else(|e| panic!("{local} rejected: {e:?}"));
    }
}

#[test]
fn absolute_detection_covers_windows_drives_and_unc_on_every_host() {
    assert!(is_absolute_token("C:\\proj\\main.rs"));
    assert!(is_absolute_token("c:/proj/main.rs"));
    assert!(is_absolute_token("\\\\server\\share\\main.rs"));
    assert!(is_absolute_token("/usr/local/main.rs"));
    assert!(!is_absolute_token("src/main.rs"));
    assert!(!is_absolute_token("./src/main.rs"));
    assert!(!is_absolute_token("C:main.rs"));
}

#[test]
fn file_link_resolution_expands_home_joins_cwd_and_preserves_absolute_paths() {
    let home = PathBuf::from("/Users/p13 fixture");
    assert_eq!(resolve_file_link("~", None, Some(home.clone())), home);
    assert_eq!(
        resolve_file_link("~/two words & notes.txt", None, Some(home.clone())),
        home.join("two words & notes.txt")
    );
    assert_eq!(
        resolve_file_link("~\\two words & notes.txt", None, Some(home.clone())),
        home.join("two words & notes.txt")
    );
    assert_eq!(
        resolve_file_link("~/x.rs", None, None),
        PathBuf::from("~/x.rs")
    );
    assert_eq!(
        resolve_file_link("src/app.rs", Some("/work/repo"), None),
        PathBuf::from("/work/repo/src/app.rs")
    );
    assert_eq!(
        resolve_file_link("src/app.rs", Some("   "), None),
        PathBuf::from("src/app.rs")
    );
    // Windows-shaped absolute paths stay verbatim even when resolved on POSIX.
    assert_eq!(
        resolve_file_link("C:\\proj\\main.rs", Some("/work/repo"), None),
        PathBuf::from("C:\\proj\\main.rs")
    );
    assert_eq!(
        resolve_file_link("\\\\srv\\share\\main.rs", Some("/work/repo"), None),
        PathBuf::from("\\\\srv\\share\\main.rs")
    );
}

#[test]
fn position_validation_requires_one_based_line_before_column() {
    assert_eq!(validate_position(None, None).unwrap(), None);
    assert_eq!(validate_position(Some(42), None).unwrap(), Some((42, None)));
    assert_eq!(
        validate_position(Some(42), Some(10)).unwrap(),
        Some((42, Some(10)))
    );
    assert_eq!(
        validate_position(None, Some(10)).expect_err("col without line").code,
        IpcErrorCode::InvalidArgument
    );
    assert_eq!(
        validate_position(Some(0), None).expect_err("zero line").code,
        IpcErrorCode::InvalidArgument
    );
    assert_eq!(
        validate_position(Some(1), Some(0)).expect_err("zero col").code,
        IpcErrorCode::InvalidArgument
    );
}

#[test]
fn session_guard_returns_local_cwd_and_refuses_remote_sessions() {
    let local = details(Some("workspace-local"), Some("/work/repo"));
    assert_eq!(
        session_cwd_guard("backend-1", Some(&local), None).unwrap(),
        Some(PathBuf::from("/work/repo"))
    );

    let no_cwd = details(Some("workspace-local"), None);
    assert_eq!(
        session_cwd_guard("backend-1", Some(&no_cwd), None).unwrap(),
        None
    );

    let ssh = details(Some("ssh:9f2c"), Some("/srv/app"));
    let error = session_cwd_guard("backend-1", Some(&ssh), None)
        .expect_err("ssh workspace paths must not open locally");
    assert_eq!(error.code, IpcErrorCode::Unsupported);
    assert_eq!(
        error.details.as_ref().and_then(|d| d["kind"].as_str()),
        Some("ssh")
    );

    let paired = details(Some("workspace-local"), Some("/srv/app"));
    let error = session_cwd_guard("daemon-session:host/abc", Some(&paired), None)
        .expect_err("paired-host relay paths must not open locally");
    assert_eq!(error.code, IpcErrorCode::Unsupported);
    assert_eq!(
        error.details.as_ref().and_then(|d| d["kind"].as_str()),
        Some("pairedHost")
    );

    let missing = session_cwd_guard("backend-1", None, Some("daemon offline"))
        .expect_err("unknown session must be an error, not a silent fallback");
    assert_eq!(missing.code, IpcErrorCode::SessionNotFound);
    assert_eq!(
        missing.details.as_ref().and_then(|d| d["cause"].as_str()),
        Some("daemon offline")
    );
}

#[test]
fn path_lookup_expands_windows_pathext_and_reports_absence() {
    let sep = if cfg!(windows) { ";" } else { ":" };
    let path_var = OsString::from(format!("/opt/empty{sep}/usr/local/bin"));

    let found = which_in_path(
        "code",
        Some(path_var.as_os_str()),
        None,
        &|candidate: &Path| candidate == Path::new("/usr/local/bin/code"),
    );
    assert_eq!(found, Some(PathBuf::from("/usr/local/bin/code")));

    let pathext = OsString::from(".COM;.EXE;.BAT;.CMD");
    let windows_found = which_in_path(
        "code",
        Some(path_var.as_os_str()),
        Some(pathext.as_os_str()),
        &|candidate: &Path| candidate == Path::new("/usr/local/bin/code.cmd"),
    );
    assert_eq!(windows_found, Some(PathBuf::from("/usr/local/bin/code.cmd")));

    assert_eq!(
        which_in_path("code", Some(path_var.as_os_str()), None, &|_| false),
        None
    );
    assert_eq!(which_in_path("code", None, None, &|_| true), None);
}

#[test]
fn editor_cli_plans_pass_position_through_argv_without_shell_interpretation() {
    let hostile = Path::new("/work/repo/$(whoami) & 'notes'/app.rs");

    let vscode = plan_launch(
        EditorTarget::VsCode,
        hostile,
        Some((42, Some(10))),
        Some(PathBuf::from("/usr/local/bin/code")),
    )
    .unwrap();
    assert_eq!(
        vscode,
        LaunchPlan::Cli {
            program: PathBuf::from("/usr/local/bin/code"),
            args: vec![
                OsString::from("--goto"),
                OsString::from("/work/repo/$(whoami) & 'notes'/app.rs:42:10"),
            ],
        }
    );

    let cursor = plan_launch(
        EditorTarget::Cursor,
        Path::new("C:\\proj\\a b\\app.rs"),
        Some((7, None)),
        Some(PathBuf::from("C:\\Program Files\\cursor\\cursor.cmd")),
    )
    .unwrap();
    assert_eq!(
        cursor,
        LaunchPlan::Cli {
            program: PathBuf::from("C:\\Program Files\\cursor\\cursor.cmd"),
            args: vec![
                OsString::from("--goto"),
                OsString::from("C:\\proj\\a b\\app.rs:7"),
            ],
        }
    );

    let zed = plan_launch(
        EditorTarget::Zed,
        Path::new("/work/repo/app.rs"),
        Some((3, Some(4))),
        Some(PathBuf::from("/usr/local/bin/zed")),
    )
    .unwrap();
    assert_eq!(
        zed,
        LaunchPlan::Cli {
            program: PathBuf::from("/usr/local/bin/zed"),
            args: vec![OsString::from("/work/repo/app.rs:3:4")],
        }
    );

    let system = plan_launch(
        EditorTarget::System,
        Path::new("/work/repo/app.rs"),
        Some((3, Some(4))),
        Some(PathBuf::from("/usr/local/bin/code")),
    )
    .unwrap();
    assert_eq!(
        system,
        LaunchPlan::SystemOpen {
            target: PathBuf::from("/work/repo/app.rs")
        }
    );
}

#[test]
fn editor_url_fallback_encodes_paths_and_refuses_unc() {
    assert_eq!(
        editor_url("vscode", Path::new("C:\\proj\\a b & c\\app.rs"), Some((5, Some(10)))).unwrap(),
        "vscode://file/C:/proj/a%20b%20%26%20c/app.rs:5:10"
    );
    assert_eq!(
        editor_url("zed", Path::new("/work/repo/app.rs"), Some((5, None))).unwrap(),
        "zed://file/work/repo/app.rs:5"
    );
    assert_eq!(
        editor_url("cursor", Path::new("/work/repo/app.rs"), None).unwrap(),
        "cursor://file/work/repo/app.rs"
    );

    let unc = editor_url("vscode", Path::new("\\\\srv\\share\\app.rs"), None)
        .expect_err("UNC paths have no documented URL form");
    assert_eq!(unc.code, IpcErrorCode::Unsupported);
}

#[test]
fn editor_without_cli_falls_back_to_documented_url_scheme() {
    let plan = plan_launch(
        EditorTarget::VsCode,
        Path::new("/work/repo/a b.rs"),
        Some((12, Some(3))),
        None,
    )
    .unwrap();
    assert_eq!(
        plan,
        LaunchPlan::Url {
            url: "vscode://file/work/repo/a%20b.rs:12:3".into()
        }
    );
}

#[test]
fn missing_files_produce_structured_errors_instead_of_a_silent_false() {
    let cwd = tempfile::tempdir().expect("tempdir");
    let error = prepare_launch(
        "missing/app.rs",
        Some(&cwd.path().to_string_lossy()),
        Some(1),
        None,
        EditorTarget::System,
        None,
        &no_cli,
    )
    .expect_err("missing files must raise a structured error");
    assert_eq!(error.code, IpcErrorCode::InvalidPath);
    let details = error.details.expect("details");
    assert_eq!(details["reason"].as_str(), Some("missing"));
    assert_eq!(
        details["path"].as_str(),
        Some(cwd.path().join("missing/app.rs").to_string_lossy().as_ref())
    );
}

#[test]
fn existing_file_relative_to_terminal_cwd_resolves_to_an_editor_plan() {
    let cwd = tempfile::tempdir().expect("tempdir");
    let nested = cwd.path().join("src");
    std::fs::create_dir_all(&nested).expect("mkdir");
    let file = nested.join("two words & notes.rs");
    std::fs::write(&file, b"fn main() {}\n").expect("write");

    let plan = prepare_launch(
        "'src/two words & notes.rs'",
        Some(&cwd.path().to_string_lossy()),
        Some(42),
        Some(10),
        EditorTarget::Zed,
        None,
        &|_| Some(PathBuf::from("/usr/local/bin/zed")),
    )
    .expect("plan");

    let mut expected = OsString::from(file.as_os_str());
    expected.push(":42:10");
    assert_eq!(
        plan,
        LaunchPlan::Cli {
            program: PathBuf::from("/usr/local/bin/zed"),
            args: vec![expected],
        }
    );
}

#[test]
fn unlaunchable_editor_binary_maps_to_a_structured_io_error() {
    let program = PathBuf::from("/nonexistent/ferryx-test-editor-binary");
    let error = execute(LaunchPlan::Cli {
        program: program.clone(),
        args: vec![OsString::from("/work/repo/app.rs:1")],
    })
    .expect_err("spawning a missing launcher must fail loudly");
    assert_eq!(error.code, IpcErrorCode::IoError);
    assert_eq!(
        error.details.as_ref().and_then(|d| d["program"].as_str()),
        Some(program.to_string_lossy().as_ref())
    );
}

#[cfg(unix)]
#[test]
fn failing_editor_exit_is_reported() {
    let error = execute(LaunchPlan::Cli {
        program: PathBuf::from("/usr/bin/false"),
        args: vec![],
    }).expect_err("an unsuccessful launcher must not report success");
    assert_eq!(error.code, IpcErrorCode::IoError);
}

#[tokio::test]
async fn command_rejects_missing_paths_remote_specs_and_unknown_editors() {
    use crate::ipc::browser::open_file_path_request;

    let missing = open_file_path_request(
        None,
        "/nonexistent/ferryx/file-link/does-not-exist.rs".into(),
        None,
        None,
        None,
        None,
        None,
    )
    .await
    .expect_err("missing file must be an error");
    assert_eq!(missing.code, IpcErrorCode::InvalidPath);

    let remote = open_file_path_request(
        None,
        "deploy@build-host:/srv/app/main.rs".into(),
        None,
        None,
        None,
        None,
        None,
    )
    .await
    .expect_err("remote spec must be an error");
    assert_eq!(remote.code, IpcErrorCode::Unsupported);

    let editor = open_file_path_request(
        None,
        "/nonexistent/ferryx/file-link/does-not-exist.rs".into(),
        None,
        None,
        None,
        None,
        Some("emacs".into()),
    )
    .await
    .expect_err("unknown editor must be an error");
    assert_eq!(editor.code, IpcErrorCode::InvalidArgument);

    let column = open_file_path_request(
        None,
        "/nonexistent/ferryx/file-link/does-not-exist.rs".into(),
        None,
        None,
        None,
        Some(3),
        None,
    )
    .await
    .expect_err("column without line must be an error");
    assert_eq!(column.code, IpcErrorCode::InvalidArgument);
}

/// Resolves a file link against the *live* cwd of a real local terminal
/// session through the existing daemon session machinery. The requested file
/// deliberately does not exist, so no desktop application is ever launched;
/// the structured error carries the resolved absolute path as proof.
#[cfg(unix)]
#[tokio::test]
async fn session_id_resolves_the_live_local_terminal_cwd() {
    use crate::daemon::client::DaemonClient;
    use crate::daemon::server::DaemonServer;
    use crate::ipc::browser::open_file_path_request;
    use crate::ipc::{cmd_terminal_close, cmd_terminal_spawn, SpawnTerminalRequest};
    use crate::worktree::{run_git, WorkspaceRegistry};
    use std::sync::Arc;
    use tauri::Manager;

    let repo = tempfile::tempdir().expect("repo tempdir");
    run_git(repo.path(), &["init"]).expect("git init");
    run_git(repo.path(), &["config", "user.email", "test@example.com"]).expect("git email");
    run_git(repo.path(), &["config", "user.name", "Test User"]).expect("git name");
    std::fs::write(repo.path().join("README.md"), "initial\n").expect("README");
    run_git(repo.path(), &["add", "README.md"]).expect("git add");
    run_git(repo.path(), &["commit", "-m", "initial"]).expect("git commit");

    let registry = WorkspaceRegistry::new();
    registry
        .register("workspace-file-link", repo.path())
        .expect("register workspace");

    let socket_dir = tempfile::tempdir().expect("socket tempdir");
    let socket_path = socket_dir.path().join("file_link_daemon.sock");
    let listener = tokio::net::UnixListener::bind(&socket_path).expect("bind uds");
    let server = Arc::new(DaemonServer::new());
    let accept_server = Arc::clone(&server);
    let server_task = tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let server = Arc::clone(&accept_server);
            tokio::spawn(async move { server.handle_client(stream).await });
        }
    });

    let daemon_client = Arc::new(DaemonClient::new_with_socket(socket_path));
    daemon_client
        .register_workspace("workspace-file-link", &repo.path().to_string_lossy())
        .await
        .expect("register workspace on daemon");

    let app = tauri::test::mock_builder()
        .manage(Arc::clone(&daemon_client))
        .manage(registry.clone())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");

    let client_state = app.state::<Arc<DaemonClient>>();
    let registry_state = app.state::<WorkspaceRegistry>();
    let spawned = cmd_terminal_spawn(
        app.handle().clone(),
        client_state.clone(),
        registry_state,
        SpawnTerminalRequest {
            workspace_id: "workspace-file-link".into(),
            worktree: None,
            cwd: None,
            cols: Some(80),
            rows: Some(24),
            client_request_id: None,
            shell: None,
            startup: None,
            inherit_from_session_id: None,
        },
    )
    .await
    .expect("spawn terminal");

    let nested = repo.path().join("changed-directory");
    std::fs::create_dir(&nested).expect("nested directory");
    let mut attachment = daemon_client.attach(&spawned.session_id, None).await.expect("subscribe before cd");
    daemon_client.write_terminal(&spawned.session_id,
        b"cd changed-directory && printf '\\106\\111\\114\\105\\137\\103\\127\\104\\137\\122\\105\\101\\104\\131\\n'\n".to_vec())
        .await.expect("change directory");
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let mut output = Vec::new();
        while let Some(message) = attachment.messages.recv().await {
            if let crate::daemon::DaemonStreamMessage::Output { data, .. } = message {
                output.extend_from_slice(&data);
                if output.windows(b"FILE_CWD_READY".len()).any(|w| w == b"FILE_CWD_READY") { return; }
            }
        }
        panic!("output stream closed before cd completed");
    }).await.expect("cd completion signal");

    let error = open_file_path_request(
        Some(&daemon_client),
        "no-such-file-from-live-cwd.rs".into(),
        None,
        Some(spawned.session_id.clone()),
        None,
        None,
        None,
    )
    .await
    .expect_err("missing file must error with the session-resolved path");
    assert_eq!(error.code, IpcErrorCode::InvalidPath);
    let resolved = error
        .details
        .as_ref()
        .and_then(|d| d["path"].as_str())
        .expect("resolved path detail")
        .to_string();
    let expected = std::fs::canonicalize(&nested)
        .expect("canonical repo")
        .join("no-such-file-from-live-cwd.rs");
    assert_eq!(
        std::path::Path::new(&resolved)
            .parent()
            .and_then(|p| std::fs::canonicalize(p).ok()),
        expected.parent().map(PathBuf::from),
        "resolved {resolved} should sit in the live session cwd"
    );

    cmd_terminal_close(client_state.clone(), spawned.session_id.clone())
        .await
        .expect("close terminal");
    attachment.stream_task.abort();
    server_task.abort();
}
