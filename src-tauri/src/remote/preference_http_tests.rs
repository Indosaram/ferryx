//! Real HTTP regression; HOME changes belong only to a private child process.
use super::*;
use crate::terminal::{TerminalPreferences, TerminalService};
use crate::worktree::WorkspaceRegistry;
use futures_util::FutureExt;
use std::time::Duration;

#[test]
fn ac09_preferences_redact_paths_when_authenticated() {
    // Given: an isolated config, without a Ghostty executable taking precedence.
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join(".config/ghostty/config.ghostty");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    std::fs::create_dir(root.path().join("empty-bin")).unwrap();
    std::fs::write(&config, "font-family = AC09 Display Font\nfont-size = 19\ncursor-style = bar\nbackground = #123456\nforeground = #abcdef\nmacos-option-as-alt = true\nscrollback-limit = 4321\n").unwrap();
    // When: the real router is exercised in a child with private environment.
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "remote::server::preference_http_tests::ac09_private_http_child", "--nocapture"])
        .env("FERRYX_AC09_CHILD", "1")
        .env("HOME", root.path())
        .env("XDG_CONFIG_HOME", root.path().join(".config"))
        .env("PATH", root.path().join("empty-bin"))
        .output().unwrap();
    // Then: clean the private root even when the child regression fails.
    root.close().unwrap();
    println!("{}", String::from_utf8_lossy(&output.stdout));
    eprintln!("{}", String::from_utf8_lossy(&output.stderr));
    println!("AC09 CLEANUP child_waited=true private_root_removed=true");
    assert!(output.status.success(), "private HTTP child: {}", output.status);
}

#[tokio::test(flavor = "current_thread")]
async fn ac09_private_http_child() {
    if std::env::var_os("FERRYX_AC09_CHILD").is_none() { return; }
    // Given: real auth grants and the unchanged native loader in private HOME.
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let local = crate::terminal::load_terminal_preferences();
    assert_eq!(local.source_path, Some(home.join(".config/ghostty/config.ghostty")));
    assert_eq!(local.font_family, "AC09 Display Font, monospace");
    assert_eq!(local.font_size, 19.0);
    assert_eq!(local.cursor_style, "bar");
    assert_eq!(local.theme.background, "#123456");
    let state = Arc::new(RemoteGatewayState::new_with_paths(
        Arc::new(TerminalService::default()), WorkspaceRegistry::new(),
        Some(home.join("gateway.json")), Some(home.join("auth.json")),
    ));
    let mut grants = Vec::new();
    for (scope, permission) in [
        (DeviceAccessScope::Mirror, DevicePermission::View),
        (DeviceAccessScope::Mirror, DevicePermission::Control),
        (DeviceAccessScope::Machine, DevicePermission::Control),
    ] {
        let pin = state.auth_manager.create_scoped_pairing_code(permission, scope).unwrap();
        grants.push(state.auth_manager.exchange_pairing_code(&pin, "ac09").unwrap());
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    println!("AC09 fixture pid={} listener={addr}", std::process::id());
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let router = create_remote_router(state.clone());
    let mut tasks = tokio::task::JoinSet::new();
    tasks.spawn(async move {
        axum::serve(listener, router).with_graceful_shutdown(async {
            stopped.await.expect("shutdown signal");
        }).await.unwrap();
    });
    let result = std::panic::AssertUnwindSafe(async {
        let client = reqwest::Client::builder().no_proxy()
            .timeout(Duration::from_secs(10)).build().unwrap();
        let url = format!("http://{addr}/api/v1/terminal/preferences");
        let mut expected = local.clone();
        expected.source_path = None;
        expected.default_shell = None;
        let mut disclosed = Vec::new();
        // When: each real grant requests the legacy display endpoint.
        for (token, device) in &grants {
            let response = client.get(&url).bearer_auth(token).send().await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let text = response.text().await.unwrap();
            println!("AC09 {:?}/{:?} HTTP 200 {}", device.access_scope, device.permission,
                text.replace(home.to_str().unwrap(), "<PRIVATE_HOME>"));
            let remote: TerminalPreferences = serde_json::from_str(&text).unwrap();
            // Then: only private metadata differs; all rendering settings survive.
            if remote.source_path.is_some() || remote.default_shell.is_some() {
                disclosed.push((device.access_scope, device.permission));
            }
            let mut rendering = remote;
            rendering.source_path = None;
            rendering.default_shell = None;
            assert_eq!(rendering, expected);
        }
        let anonymous = client.get(&url).send().await.unwrap();
        println!("AC09 anonymous HTTP {}", anonymous.status());
        assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
        for (token, device) in &grants {
            assert!(state.auth_manager.revoke_device(&device.id));
            let revoked = client.get(&url).bearer_auth(token).send().await.unwrap();
            println!("AC09 revoked {:?}/{:?} HTTP {}", device.access_scope, device.permission, revoked.status());
            assert_eq!(revoked.status(), StatusCode::UNAUTHORIZED);
        }
        assert_eq!(crate::terminal::load_terminal_preferences(), local);
        assert!(disclosed.is_empty(), "private preference metadata disclosed to {disclosed:?}");
    }).catch_unwind().await;
    stop.send(()).unwrap();
    match tokio::time::timeout(Duration::from_secs(10), tasks.join_next()).await {
        Ok(Some(joined)) => joined.unwrap(),
        other => {
            tasks.shutdown().await;
            panic!("listener shutdown failed: {other:?}");
        }
    }
    println!("AC09 CLEANUP listener_joined=true");
    if let Err(panic) = result { std::panic::resume_unwind(panic); }
}
