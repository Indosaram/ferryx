use super::*;

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
