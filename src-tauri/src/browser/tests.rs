use super::*;
use crate::ipc::browser::cmd_browser_set_bounds;
use crate::ipc::error::{IpcError, IpcErrorCode};
use tauri::Manager;

#[test]
fn test_validate_url() {
    assert_eq!(
        validate_url("https://github.com").unwrap(),
        "https://github.com/"
    );
    assert_eq!(
        validate_url("http://localhost:3000").unwrap(),
        "http://localhost:3000/"
    );
    assert_eq!(validate_url("about:blank").unwrap(), "about:blank");
    assert_eq!(validate_url("").unwrap(), "about:blank");

    assert!(validate_url("tauri://localhost").is_err());
    assert!(validate_url("file:///etc/passwd").is_err());
    assert!(validate_url("javascript:alert(1)").is_err());
    assert!(validate_url("asset://localhost/index.html").is_err());
}

#[test]
fn test_browser_manager_lifecycle() {
    let mgr = BrowserManager::new();
    assert!(!mgr.has_sessions());
    let req = CreateBrowserRequest {
        workspace_id: Some("default".into()),
        worktree_path: None,
        url: "https://example.com".into(),
        profile: Some(BrowserProfileId::Default),
        bounds: Some(LogicalRect {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 600.0,
        }),
        visible: Some(true),
    };

    let state = mgr.register_session(req).expect("register session");
    assert_eq!(state.url, "https://example.com/");
    assert_eq!(state.generation, 1);
    assert!(state.visible);

    let fetched = mgr.get_state(&state.browser_id).expect("get state");
    assert_eq!(fetched.browser_id, state.browser_id);

    let updated_url = mgr
        .update_url(&state.browser_id, "http://localhost:5173")
        .expect("update url");
    assert_eq!(updated_url, "http://localhost:5173/");
    let fetched2 = mgr.get_state(&state.browser_id).expect("get state 2");
    assert_eq!(fetched2.generation, 2);
    assert_eq!(fetched2.url, "http://localhost:5173/");

    mgr.set_bounds(
        &state.browser_id,
        LogicalRect {
            x: 10.0,
            y: 10.0,
            width: 1000.0,
            height: 700.0,
        },
    )
    .expect("set bounds");

    let zoom = mgr.set_zoom(&state.browser_id, 1.5).expect("set zoom");
    assert_eq!(zoom, 1.5);

    let nav_state = mgr
        .update_navigation_state(
            &state.browser_id,
            None,
            Some("My Page".into()),
            Some(false),
            Some(true),
            Some(false),
            None,
        )
        .expect("update nav state");
    assert_eq!(nav_state.title, Some("My Page".into()));
    assert!(nav_state.can_go_back);
    assert!(!nav_state.can_go_forward);

    let list = mgr.list_sessions();
    assert_eq!(list.len(), 1);
    assert!(mgr.has_sessions());

    let removed = mgr.remove_session(&state.browser_id);
    assert!(removed.is_some());
    assert!(mgr.get_state(&state.browser_id).is_err());
    assert!(!mgr.has_sessions());
}

#[test]
fn test_imported_cookie_converts_to_tauri_cookie() {
    let input = r#"[
      {
        "name": "session",
        "value": "abc123",
        "domain": ".example.com",
        "path": "/app",
        "secure": true,
        "httpOnly": true,
        "sameSite": "lax"
      }
    ]"#;

    let imported = cookies::parse_cookie_file(input).expect("parse cookie file");
    let cookie = cookies::cookie_from_imported(imported.into_iter().next().unwrap())
        .expect("convert imported cookie");

    assert_eq!(cookie.name(), "session");
    assert_eq!(cookie.value(), "abc123");
    assert_eq!(cookie.domain(), Some("example.com"));
    assert_eq!(cookie.path(), Some("/app"));
    assert_eq!(cookie.secure(), Some(true));
    assert_eq!(cookie.http_only(), Some(true));
}

#[test]
fn test_history_navigation_marks_loading_without_overwriting_url() {
    let mgr = BrowserManager::new();
    let state = mgr
        .register_session(CreateBrowserRequest {
            workspace_id: Some("default".into()),
            worktree_path: None,
            url: "https://example.com/second".into(),
            profile: Some(BrowserProfileId::Default),
            bounds: None,
            visible: Some(true),
        })
        .expect("register browser");

    let started = mgr
        .begin_history_navigation(&state.browser_id)
        .expect("begin history navigation");
    assert_eq!(started.url, "https://example.com/second");
    assert!(started.loading);
    assert_eq!(started.generation, state.generation + 1);

    let finished = mgr
        .update_navigation_state(
            &state.browser_id,
            Some("https://example.com/first".into()),
            Some("First".into()),
            Some(false),
            Some(false),
            Some(true),
            None,
        )
        .expect("finish history navigation");
    assert_eq!(finished.url, "https://example.com/first");
    assert!(!finished.loading);
    assert!(!finished.can_go_back);
    assert!(finished.can_go_forward);
}

#[test]
fn test_browser_manager_get_bounds_and_visibility() {
    let mgr = BrowserManager::new();
    let state = mgr
        .register_session(CreateBrowserRequest {
            workspace_id: None,
            worktree_path: None,
            url: "https://example.com".into(),
            profile: Some(BrowserProfileId::Default),
            bounds: Some(LogicalRect {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 600.0,
            }),
            visible: Some(true),
        })
        .expect("register browser");

    assert_eq!(
        mgr.get_bounds(&state.browser_id).expect("get bounds"),
        Some(LogicalRect {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 600.0,
        })
    );
    assert!(mgr.is_visible(&state.browser_id).expect("is visible"));

    mgr.set_bounds(
        &state.browser_id,
        LogicalRect {
            x: 10.0,
            y: 50.0,
            width: 900.0,
            height: 550.0,
        },
    )
    .expect("set bounds");
    assert_eq!(
        mgr.get_bounds(&state.browser_id).expect("get bounds after set"),
        Some(LogicalRect {
            x: 10.0,
            y: 50.0,
            width: 900.0,
            height: 550.0,
        })
    );

    mgr.set_visible(&state.browser_id, false)
        .expect("set visible");
    assert!(!mgr.is_visible(&state.browser_id).expect("is visible"));
}

#[test]
fn test_webview_not_found_ipc_error_mapping() {
    let err = BrowserError::WebviewNotFound("browser-test-label".into());
    let ipc_err: IpcError = err.into();
    assert_eq!(ipc_err.code, IpcErrorCode::WebviewNotFound);
    assert!(ipc_err.message.contains("browser-test-label"));
}

#[tokio::test]
async fn test_cmd_browser_set_bounds_returns_webview_not_found_when_webview_missing() {
    let app = tauri::test::mock_builder()
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");
    let manager = std::sync::Arc::new(BrowserManager::new());
    let state = manager
        .register_session(CreateBrowserRequest {
            workspace_id: None,
            worktree_path: None,
            url: "https://example.com".into(),
            profile: None,
            bounds: None,
            visible: Some(true),
        })
        .expect("register");
    app.manage(std::sync::Arc::clone(&manager));
    let state_handle = app.state::<std::sync::Arc<BrowserManager>>();

    let result = cmd_browser_set_bounds(
        app.handle().clone(),
        state_handle,
        state.browser_id.clone(),
        LogicalRect {
            x: 0.0,
            y: 50.0,
            width: 800.0,
            height: 550.0,
        },
    )
    .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.code, IpcErrorCode::WebviewNotFound);
    assert!(err.message.contains(&state.webview_label));
}
