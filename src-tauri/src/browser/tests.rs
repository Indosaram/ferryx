use super::*;
use crate::ipc::browser::cmd_browser_set_bounds;
use crate::ipc::error::{IpcError, IpcErrorCode};
use tauri::Manager;

#[test]
fn test_default_desktop_user_agent() {
    let ua = default_desktop_user_agent();
    assert!(ua.contains("Mozilla/5.0"));
    #[cfg(target_os = "macos")]
    {
        assert!(ua.contains("Macintosh; Intel Mac OS X 10_15_7"));
        assert!(ua.contains("AppleWebKit/537.36") || ua.contains("AppleWebKit/605.1.15"));
        assert!(ua.contains("Chrome/") || ua.contains("Safari/"));
    }
    #[cfg(target_os = "windows")]
    {
        assert!(ua.contains("Windows NT 10.0; Win64; x64"));
        assert!(ua.contains("AppleWebKit/537.36"));
        assert!(ua.contains("Chrome/"));
    }
    #[cfg(target_os = "linux")]
    {
        assert!(ua.contains("X11; Linux x86_64") || ua.contains("Linux"));
        assert!(ua.contains("AppleWebKit/537.36"));
        assert!(ua.contains("Chrome/"));
    }
}

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
        browser_id: None,
        workspace_id: Some("default".into()),
        worktree_path: None,
        url: "https://example.com".into(),
        profile: Some(BrowserProfileId::Default),
        zoom_factor: None,
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
            browser_id: None,
            workspace_id: Some("default".into()),
            worktree_path: None,
            url: "https://example.com/first".into(),
            profile: Some(BrowserProfileId::Default),
            zoom_factor: None,
            bounds: None,
            visible: Some(true),
        })
        .expect("register browser");

    mgr.update_url(&state.browser_id, "https://example.com/second")
        .expect("update url");

    let current = mgr.get_state(&state.browser_id).expect("get state");
    assert_eq!(current.url, "https://example.com/second");
    assert!(current.can_go_back);
    assert!(!current.can_go_forward);

    let started = mgr
        .begin_history_navigation(&state.browser_id, false)
        .expect("begin history navigation");
    assert_eq!(started.url, "https://example.com/first");
    assert!(started.loading);
    assert_eq!(started.generation, current.generation + 1);

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
fn test_browser_automation_target_rejects_stale_generation() {
    let manager = BrowserManager::new();
    let state = manager
        .register_session(CreateBrowserRequest {
            browser_id: None,
            workspace_id: None,
            worktree_path: None,
            url: "https://example.com".into(),
            profile: None,
            zoom_factor: None,
            bounds: None,
            visible: Some(true),
        })
        .expect("register browser");

    manager
        .record_automation_targets(
            &state.browser_id,
            state.generation,
            vec![BrowserAutomationTarget {
                reference: "e1".into(),
                selector: "button:nth-of-type(1)".into(),
            }],
        )
        .expect("record snapshot targets");

    let selector = manager
        .automation_target(&state.browser_id, state.generation, "e1")
        .expect("resolve current snapshot target");
    assert_eq!(selector, "button:nth-of-type(1)");

    manager
        .update_url(&state.browser_id, "https://example.com/next")
        .expect("navigate");

    let error = manager
        .automation_target(&state.browser_id, state.generation, "e1")
        .expect_err("reject target from prior page generation");
    assert_eq!(error, BrowserError::AutomationSnapshotStale);
}

#[test]
fn test_browser_automation_target_requires_current_snapshot_reference() {
    let manager = BrowserManager::new();
    let state = manager
        .register_session(CreateBrowserRequest {
            browser_id: None,
            workspace_id: None,
            worktree_path: None,
            url: "https://example.com".into(),
            profile: None,
            zoom_factor: None,
            bounds: None,
            visible: Some(true),
        })
        .expect("register browser");

    let error = manager
        .automation_target(&state.browser_id, state.generation, "e1")
        .expect_err("snapshot must create element references first");
    assert_eq!(error, BrowserError::AutomationTargetNotFound("e1".into()));
}

#[test]
fn test_page_load_url_change_invalidates_automation_targets() {
    let manager = BrowserManager::new();
    let state = manager
        .register_session(CreateBrowserRequest {
            browser_id: None,
            workspace_id: None,
            worktree_path: None,
            url: "https://example.com".into(),
            profile: None,
            zoom_factor: None,
            bounds: None,
            visible: Some(true),
        })
        .expect("register browser");
    manager
        .record_automation_targets(
            &state.browser_id,
            state.generation,
            vec![BrowserAutomationTarget {
                reference: "e1".into(),
                selector: "a".into(),
            }],
        )
        .expect("record snapshot targets");

    let updated = manager
        .update_navigation_state(
            &state.browser_id,
            Some("https://example.com/next".into()),
            None,
            Some(true),
            None,
            None,
            None,
        )
        .expect("handle page load URL change");

    assert_eq!(updated.generation, state.generation + 1);
    assert_eq!(
        manager
            .automation_target(&state.browser_id, state.generation, "e1")
            .expect_err("previous document target is stale"),
        BrowserError::AutomationSnapshotStale
    );
}

#[test]
fn test_browser_manager_get_bounds_and_visibility() {
    let mgr = BrowserManager::new();
    let state = mgr
        .register_session(CreateBrowserRequest {
            browser_id: None,
            workspace_id: None,
            worktree_path: None,
            url: "https://example.com".into(),
            profile: Some(BrowserProfileId::Default),
            zoom_factor: None,
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
        mgr.get_bounds(&state.browser_id)
            .expect("get bounds after set"),
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
            browser_id: None,
            workspace_id: None,
            worktree_path: None,
            url: "https://example.com".into(),
            profile: None,
            zoom_factor: None,
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

#[test]
fn named_profile_ids_round_trip_as_strings() {
    let profile = BrowserProfileId::from_id("work-account").expect("named profile");
    assert_eq!(profile, BrowserProfileId::Named("work-account".into()));
    assert_eq!(profile.as_str(), "work-account");
    assert_eq!(serde_json::to_string(&profile).unwrap(), "\"work-account\"");
    assert_eq!(
        serde_json::from_str::<BrowserProfileId>("\"work-account\"").unwrap(),
        profile
    );
    assert!(BrowserProfileId::from_id("../unsafe").is_none());
}

#[test]
fn restored_browser_id_and_zoom_are_preserved_by_manager() {
    let manager = BrowserManager::new();
    let state = manager
        .register_session(CreateBrowserRequest {
            browser_id: Some("persisted-browser-id".into()),
            workspace_id: Some("workspace-1".into()),
            worktree_path: None,
            url: "https://example.com/restored".into(),
            profile: Some(BrowserProfileId::Private),
            zoom_factor: Some(1.25),
            bounds: None,
            visible: Some(false),
        })
        .expect("register restored browser");

    assert_eq!(state.browser_id, "persisted-browser-id");
    assert_eq!(state.profile_id, BrowserProfileId::Private);
    assert_eq!(state.zoom_factor, 1.25);
    assert!(!state.visible);
}

#[test]
fn history_stack_truncates_forward_entries_after_new_navigation() {
    let manager = BrowserManager::new();
    let state = manager
        .register_session(CreateBrowserRequest {
            browser_id: None,
            workspace_id: None,
            worktree_path: None,
            url: "https://example.com/one".into(),
            profile: None,
            zoom_factor: None,
            bounds: None,
            visible: Some(true),
        })
        .expect("register browser");

    manager
        .update_url(&state.browser_id, "https://example.com/two")
        .unwrap();
    manager
        .update_url(&state.browser_id, "https://example.com/three")
        .unwrap();

    let back = manager
        .begin_history_navigation(&state.browser_id, false)
        .expect("go back");
    assert_eq!(back.url, "https://example.com/two");
    assert!(back.can_go_back);
    assert!(back.can_go_forward);

    manager
        .update_url(&state.browser_id, "https://example.com/replacement")
        .expect("new navigation from history");
    let replaced = manager.get_state(&state.browser_id).unwrap();
    assert_eq!(replaced.url, "https://example.com/replacement");
    assert!(replaced.can_go_back);
    assert!(!replaced.can_go_forward);

    let back_again = manager
        .begin_history_navigation(&state.browser_id, false)
        .expect("go back to two");
    assert_eq!(back_again.url, "https://example.com/two");
    let forward = manager
        .begin_history_navigation(&state.browser_id, true)
        .expect("go forward to replacement");
    assert_eq!(forward.url, "https://example.com/replacement");
}
