use super::*;
use futures_util::FutureExt;
use crate::remote::{
    auth::{DeviceAccessScope, DevicePermission},
    machine_protocol,
    server::create_remote_router,
};
use std::path::Path;

fn git_dir(repo: &Path, head: &str) {
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::write(repo.join(".git").join("HEAD"), head).unwrap();
}

fn names(walk: &Walk, root: &Path) -> Vec<String> {
    let root = root.to_str().unwrap();
    walk.repos
        .iter()
        .map(|repo| {
            repo.repo_root
                .strip_prefix(root)
                .unwrap()
                .trim_start_matches(['/', '\\'])
                .replace('\\', "/")
        })
        .collect()
}

fn run(root: &Path, depth: u8, hidden: bool, caps: Caps) -> Walk {
    walk(root, depth, hidden, caps, &AtomicBool::new(false), || {
        Duration::ZERO
    })
    .unwrap()
}

/// Fixture: nested repos, a linked worktree, a symlink loop, node_modules holding a
/// repo, a hidden repo, a detached HEAD, and a repo five levels deep.
fn fixture() -> tempfile::TempDir {
    let fixture = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(fixture.path()).unwrap();
    git_dir(&root.join("alpha"), "ref: refs/heads/main\n");
    // Nested inside alpha: descent stops at alpha's .git, so this is never listed.
    git_dir(&root.join("alpha").join("nested"), "ref: refs/heads/inner\n");
    git_dir(
        &root.join("group").join("beta"),
        "0123456789abcdef0123456789abcdef01234567\n",
    );
    // Linked worktree of alpha.
    let admin = root.join("alpha").join(".git").join("worktrees").join("wt");
    std::fs::create_dir_all(&admin).unwrap();
    std::fs::write(admin.join("HEAD"), "ref: refs/heads/feature/x\n").unwrap();
    std::fs::create_dir_all(root.join("wt")).unwrap();
    std::fs::write(
        root.join("wt").join(".git"),
        format!("gitdir: {}\n", admin.display()),
    )
    .unwrap();
    git_dir(&root.join("node_modules").join("pkg"), "ref: refs/heads/main\n");
    git_dir(&root.join("target").join("built"), "ref: refs/heads/main\n");
    git_dir(&root.join(".hidden").join("secret"), "ref: refs/heads/main\n");
    git_dir(&root.join("d1").join("d2").join("d3").join("four"), "ref: refs/heads/main\n");
    git_dir(
        &root.join("d1").join("d2").join("d3").join("d4").join("five"),
        "ref: refs/heads/main\n",
    );
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&root, root.join("group").join("loop")).unwrap();
        std::os::unix::fs::symlink(root.join("alpha"), root.join("alpha-link")).unwrap();
    }
    fixture
}

#[test]
fn walk_applies_depth_ignore_symlink_and_nesting_rules() {
    let fixture = fixture();
    let root = std::fs::canonicalize(fixture.path()).unwrap();
    let result = run(&root, MAX_DEPTH, false, CAPS);
    assert!(!result.truncated);
    // Depth 4 reaches d1/d2/d3/four but not d1/d2/d3/d4/five; node_modules, target,
    // hidden dirs, symlinks and the repo nested inside alpha are never listed.
    assert_eq!(
        names(&result, &root),
        ["alpha", "d1/d2/d3/four", "group/beta", "wt"]
    );
    let shallow = run(&root, 2, false, CAPS);
    assert_eq!(names(&shallow, &root), ["alpha", "group/beta", "wt"]);
    let one = run(&root, 1, false, CAPS);
    assert_eq!(names(&one, &root), ["alpha", "wt"]);
    // Out-of-range depth clamps to the cap rather than walking deeper.
    assert_eq!(names(&run(&root, 9, false, CAPS), &root), names(&result, &root));
}

#[test]
fn hidden_repositories_need_include_hidden() {
    let fixture = fixture();
    let root = std::fs::canonicalize(fixture.path()).unwrap();
    let result = run(&root, 2, true, CAPS);
    assert!(names(&result, &root).contains(&".hidden/secret".to_owned()));
    assert!(!names(&result, &root).iter().any(|n| n.starts_with("node_modules")));
}

#[test]
fn branch_and_linked_worktree_come_from_head_files() {
    let fixture = fixture();
    let root = std::fs::canonicalize(fixture.path()).unwrap();
    let result = run(&root, 2, false, CAPS);
    let row = |name: &str| {
        result
            .repos
            .iter()
            .find(|repo| Path::new(&repo.repo_root) == root.join(name))
            .unwrap_or_else(|| panic!("{name} listed"))
    };
    let alpha = row("alpha");
    assert_eq!(alpha.branch.as_deref(), Some("main"));
    assert!(!alpha.is_linked_worktree);
    let common = std::fs::canonicalize(root.join("alpha").join(".git")).unwrap();
    assert_eq!(alpha.git_common_dir.as_deref(), common.to_str());
    let beta = row("group/beta");
    assert_eq!(beta.branch, None, "detached HEAD carries no branch");
    let wt = row("wt");
    assert!(wt.is_linked_worktree);
    assert_eq!(wt.branch.as_deref(), Some("feature/x"));
    assert_eq!(wt.git_common_dir.as_deref(), common.to_str());
    assert!(result.repos.iter().all(|r| r.registered_workspace_id.is_none()));
}

#[test]
fn head_parsing_accepts_only_branch_refs() {
    assert_eq!(parse_branch("ref: refs/heads/main\n").as_deref(), Some("main"));
    assert_eq!(parse_branch("ref:refs/heads/a/b").as_deref(), Some("a/b"));
    assert_eq!(parse_branch("ref: refs/remotes/origin/main"), None);
    assert_eq!(parse_branch("0123456789abcdef"), None);
    assert_eq!(parse_branch("ref: refs/heads/"), None);
    assert_eq!(parse_branch(""), None);
}

#[test]
fn root_that_is_a_repository_returns_only_itself() {
    let fixture = fixture();
    let root = std::fs::canonicalize(fixture.path()).unwrap().join("alpha");
    let result = run(&root, MAX_DEPTH, true, CAPS);
    assert_eq!(result.repos.len(), 1);
    assert_eq!(Path::new(&result.repos[0].repo_root), root);
}

#[test]
fn caps_truncate_without_failing() {
    let fixture = fixture();
    let root = std::fs::canonicalize(fixture.path()).unwrap();
    let entries = run(&root, MAX_DEPTH, false, Caps { entries: 3, ..CAPS });
    assert!(entries.truncated, "entry cap sets truncated");
    assert!(entries.repos.len() <= 3);
    let repos = run(&root, MAX_DEPTH, false, Caps { repos: 1, ..CAPS });
    assert!(repos.truncated, "repo cap sets truncated");
    assert_eq!(repos.repos.len(), 1);
    let bytes = run(&root, MAX_DEPTH, false, Caps { json_bytes: 1, ..CAPS });
    assert!(bytes.truncated && bytes.repos.is_empty());
    let late = walk(&root, MAX_DEPTH, false, CAPS, &AtomicBool::new(false), || {
        CAPS.budget
    })
    .unwrap();
    assert!(late.truncated, "time budget sets truncated");
    assert!(matches!(
        walk(&root, MAX_DEPTH, false, CAPS, &AtomicBool::new(true), || Duration::ZERO),
        Err(FsError::Timeout)
    ));
}

#[test]
fn registrations_join_by_canonical_root() {
    let fixture = fixture();
    let root = std::fs::canonicalize(fixture.path()).unwrap();
    let mut repos = run(&root, 2, false, CAPS).repos;
    let alpha = root.join("alpha");
    let elsewhere = root.join("elsewhere");
    attach_registrations(
        &mut repos,
        [("project-alpha", alpha.as_path()), ("project-x", elsewhere.as_path())],
    );
    for repo in &repos {
        let expected = (Path::new(&repo.repo_root) == alpha).then_some("project-alpha");
        assert_eq!(repo.registered_workspace_id.as_deref(), expected);
    }
}

#[test]
fn query_accepts_only_path_depth_and_include_hidden() {
    assert_eq!(
        query(None).unwrap(),
        DiscoveryQuery { path: String::new(), depth: DEFAULT_DEPTH, include_hidden: false }
    );
    assert_eq!(
        query(Some("path=%7E%2Fsrc&depth=4&includeHidden=true")).unwrap(),
        DiscoveryQuery { path: "~/src".into(), depth: 4, include_hidden: true }
    );
    for raw in ["depth=0", "depth=5", "depth=x", "depth=1&depth=2", "includeHidden=1", "token=t", "path"] {
        assert!(query(Some(raw)).is_err(), "{raw}");
    }
}

#[test]
fn relay_allows_only_the_discovery_query_fields() {
    use crate::remote::relay_server::{allowed_http_route, validate_http_query};
    assert!(allowed_http_route(&axum::http::Method::GET, "fs/repositories"));
    assert!(!allowed_http_route(&axum::http::Method::POST, "fs/repositories"));
    assert!(validate_http_query(
        "fs/repositories",
        Some("path=%2Fhome%2Fa&depth=3&includeHidden=false")
    )
    .is_ok());
    for raw in [
        "depth=0",
        "depth=5",
        "includeHidden=1",
        "path=a&path=b",
        "token=secret",
        "workspaceId=x",
        "path=%00",
    ] {
        assert!(validate_http_query("fs/repositories", Some(raw)).is_err(), "{raw}");
    }
    // The directory route did not gain the new field.
    assert!(validate_http_query("fs/directories", Some("depth=3")).is_err());
}

#[tokio::test]
async fn http_route_lists_repositories_and_capability_needs_machine_control() {
    let (fixture, data, daemon) = crate::ipc::run_blocking(|| {
        let fixture = fixture();
        // Daemon state lives outside the walked tree.
        let data = tempfile::tempdir().unwrap();
        let daemon = crate::daemon::server::DaemonServer::new_with_paths(
            Some(data.path().join("config.json")),
            Some(data.path().join("auth.json")),
        );
        *daemon.remote_state().browse_home.write() = Some(fixture.path().to_owned());
        Ok((fixture, data, daemon))
    })
    .await
    .unwrap();
    let state = Arc::clone(daemon.remote_state());
    let auth = Arc::clone(&state.auth_manager);
    let (machine, mirror) = crate::ipc::run_blocking(move || {
        let pin = auth
            .create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine)
            .unwrap();
        let machine = auth.exchange_pairing_code(&pin, "discovery").unwrap().0;
        let pin = auth.create_pairing_code(DevicePermission::Control);
        Ok((machine, auth.exchange_pairing_code(&pin, "mirror").unwrap().0))
    })
    .await
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        axum::serve(listener, create_remote_router(state))
            .with_graceful_shutdown(async {
                let _ = stop_rx.await;
            })
            .await
            .unwrap();
    });
    let outcome = std::panic::AssertUnwindSafe(async {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap();
        let capabilities = |token: String| {
            let client = client.clone();
            async move {
                let response = client
                    .get(format!("http://{addr}/api/v1/capabilities"))
                    .bearer_auth(token)
                    .send()
                    .await
                    .unwrap();
                let value: serde_json::Value =
                    serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
                value["capabilities"].as_array().cloned().unwrap_or_default()
            }
        };
        assert!(capabilities(machine.clone())
            .await
            .iter()
            .any(|c| c == REPO_DISCOVERY_CAPABILITY));
        assert!(!capabilities(mirror.clone())
            .await
            .iter()
            .any(|c| c == REPO_DISCOVERY_CAPABILITY));
        let url = format!("http://{addr}/api/v1/fs/repositories");
        let response = client
            .get(&url)
            .bearer_auth(&machine)
            .query(&[("path", "~"), ("depth", "2")])
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["cache-control"], "no-store");
        let listing: machine_protocol::Repositories =
            serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
        let home = std::fs::canonicalize(fixture.path()).unwrap();
        assert_eq!(Path::new(&listing.path), home);
        assert!(!listing.truncated);
        let mut roots: Vec<PathBuf> = listing
            .repos
            .iter()
            .map(|r| Path::new(&r.repo_root).strip_prefix(&home).unwrap().to_owned())
            .collect();
        roots.sort();
        assert_eq!(
            roots,
            vec![
                PathBuf::from("alpha"),
                Path::new("group").join("beta"),
                PathBuf::from("wt")
            ]
        );
        for (token, query, expected) in [
            ("invalid", "", 401),
            (mirror.as_str(), "", 403),
            (machine.as_str(), "?depth=9", 400),
            (machine.as_str(), "?path=relative", 400),
            (machine.as_str(), "?path=~%2Fmissing", 404),
        ] {
            let response = client
                .get(format!("{url}{query}"))
                .bearer_auth(token)
                .send()
                .await
                .unwrap();
            assert_eq!(response.status().as_u16(), expected, "{query}");
            assert_eq!(response.headers()["cache-control"], "no-store");
        }
    })
    .catch_unwind()
    .await;
    stop_tx.send(()).unwrap();
    server.await.unwrap();
    drop(daemon);
    crate::ipc::run_blocking(move || {
        fixture.close().unwrap();
        data.close().unwrap();
        Ok(())
    })
    .await
    .unwrap();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

