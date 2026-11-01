//! One page-access seam for both engines: a Chromium (CEF) engine tab is reached over the
//! engine's in-process DevTools API, a system webview tab through its webview. The observe/act
//! entry points in `ipc::browser` call these instead of picking an engine themselves.

use std::sync::Arc;

use tauri::{AppHandle, Manager};

use crate::browser::model::BrowserAutomationAction;
use crate::browser::snapshot_source::{BrowserSnapshot, SnapshotOptions};
use crate::browser::{BrowserError, BrowserManager};
use crate::ipc::error::IpcError;

/// Whether the tab is served by the Chromium engine.
pub fn is_engine_tab(browser_id: &str) -> bool {
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    {
        crate::browser::cef_engine::is_cef_tab(browser_id)
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        let _ = browser_id;
        false
    }
}

/// Evaluates a page script in the tab and returns the JSON text of its value.
pub async fn eval<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    browser_id: &str,
    script: String,
) -> Result<String, BrowserError> {
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    if is_engine_tab(browser_id) {
        return super::cef_backend::eval(browser_id, &script).await;
    }
    let label = manager.get_webview_label(browser_id)?;
    let webview = app.get_webview(&label).ok_or(BrowserError::WebviewNotFound(label))?;
    crate::ipc::browser::eval_webview(webview, script).await
}

/// Evaluates an agent script (`observe.js`): in an engine tab inside its isolated world, in a
/// webview tab in the page's world (the script builds its own clean realm there).
pub async fn eval_agent<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    browser_id: &str,
    script: String,
) -> Result<String, BrowserError> {
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    if is_engine_tab(browser_id) {
        return super::cef_backend::eval_isolated(browser_id, &script).await;
    }
    eval(app, manager, browser_id, script).await
}

/// Whether agent scripts in this tab run in an isolated world.
pub fn agent_world_isolated(browser_id: &str) -> bool {
    is_engine_tab(browser_id)
}

/// Runs an element action addressed by ref, bound to the fingerprint its observe recorded.
pub async fn act_element<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    browser_id: &str,
    action: &BrowserAutomationAction,
    expect: &str,
) -> Result<(), BrowserError> {
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    if is_engine_tab(browser_id) {
        return super::cef_backend::act_element(browser_id, action, expect).await;
    }
    let script = super::observe::act_script(action, expect, false)
        .ok_or_else(|| BrowserError::AutomationFailed("the action addresses no element".into()))?;
    super::observe::parse_act(&eval(app, manager, browser_id, script).await?)
}

/// Runs the trusted-input form of an action that addresses no element, in an engine tab.
/// `Ok(false)` means the caller runs its DOM script instead (always for webview tabs).
pub async fn trusted_act(browser_id: &str, action: &BrowserAutomationAction) -> Result<bool, BrowserError> {
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    if is_engine_tab(browser_id) {
        return super::cef_backend::act(browser_id, action).await;
    }
    let _ = (browser_id, action);
    Ok(false)
}

/// Captures the tab's visible page: CDP for an engine tab, native webview capture otherwise.
pub async fn capture<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &Arc<BrowserManager>,
    browser_id: &str,
    options: SnapshotOptions,
) -> Result<BrowserSnapshot, IpcError> {
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    if is_engine_tab(browser_id) {
        return Ok(super::cef_backend::capture(browser_id).await?);
    }
    let state = manager.get_state(browser_id)?;
    crate::browser::capture_browser_snapshot(app, &state.webview_label, options).await
}

#[cfg(all(test, any(target_os = "windows", target_os = "macos", target_os = "linux")))]
mod tests {
    use serde_json::{json, Value};

    use super::super::fake_cdp::{evaluated, op_of, world, FakeCdp, Reply};
    use super::*;
    use crate::browser::model::{BrowserAutomationRequest, BrowserWaitCondition, CreateBrowserRequest};
    use crate::ipc::browser::{browser_automation_act, browser_automation_snapshot};
    use crate::ipc::error::IpcErrorCode;

    const SAVE: &str = "button\nbutton\nSave";

    fn mock_app() -> tauri::App<tauri::test::MockRuntime> {
        tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app")
    }

    /// A browser session whose id is `browser_id` (the stand-in engine's tab when it serves one).
    fn session(manager: &BrowserManager, browser_id: &str) -> crate::browser::BrowserState {
        manager
            .register_session(CreateBrowserRequest {
                browser_id: Some(browser_id.into()),
                workspace_id: None,
                worktree_path: None,
                url: "https://fixture.test/form".into(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .expect("register session")
    }

    /// The fixture page as observe.js reports it: a Save button and an email box, with a token
    /// in the title and the URL that the snapshot must not carry.
    fn observation() -> Value {
        json!({
            "ok": true,
            "url": "https://fixture.test/form?token=abc123secret",
            "title": "Form ghp_abcdefghijklmnopqrstuvwxyz0123",
            "doc": "d1",
            "elements": [
                { "reference": "d1.1", "role": "button", "name": "Save", "tagName": "button" },
                { "reference": "d1.2", "role": "textbox", "name": "Email", "tagName": "input" }
            ],
            "frames": [],
            "truncated": false
        })
    }

    #[tokio::test]
    async fn engine_tabs_take_the_cdp_path_and_webview_tabs_their_webview() {
        let app = mock_app();
        let manager = Arc::new(BrowserManager::new());
        let cdp = FakeCdp::start("page-seam", |method, params| {
            world(method, 11).unwrap_or_else(|| match params.get("contextId") {
                Some(_) => evaluated(json!({ "ok": true })),
                None => Reply::Ok(json!({ "result": { "type": "number", "value": 2 } })),
            })
        });
        session(&manager, &cdp.tab);
        let webview_tab = session(&manager, "page-seam-webview");

        assert!(is_engine_tab(&cdp.tab));
        assert!(agent_world_isolated(&cdp.tab));
        assert!(!is_engine_tab(&webview_tab.browser_id));
        assert!(!agent_world_isolated(&webview_tab.browser_id));

        assert_eq!(eval(app.handle(), &manager, &cdp.tab, "1 + 1".into()).await.unwrap(), "2");
        assert_eq!(eval_agent(app.handle(), &manager, &cdp.tab, "x".into()).await.unwrap(), r#"{"ok":true}"#);
        assert_eq!(
            cdp.methods(),
            ["Runtime.evaluate", "Page.getFrameTree", "Page.createIsolatedWorld", "Runtime.evaluate"]
        );
        assert!(cdp.calls()[0].1.get("contextId").is_none());
        assert_eq!(cdp.calls()[3].1["contextId"], 11);

        // The webview tab never reaches the engine; with no webview it says so.
        let label = webview_tab.webview_label.clone();
        for result in [
            eval(app.handle(), &manager, &webview_tab.browser_id, "1".into()).await,
            eval_agent(app.handle(), &manager, &webview_tab.browser_id, "1".into()).await,
            act_element(app.handle(), &manager, &webview_tab.browser_id, &BrowserAutomationAction::Click { reference: "d1.1".into() }, SAVE)
                .await
                .map(|()| String::new()),
        ] {
            assert_eq!(result, Err(BrowserError::WebviewNotFound(label.clone())));
        }
        let keypress = BrowserAutomationAction::Keypress { key: "Enter".into() };
        assert!(!trusted_act(&webview_tab.browser_id, &keypress).await.unwrap());
        assert!(trusted_act(&cdp.tab, &keypress).await.unwrap());
        assert_eq!(cdp.methods().iter().filter(|method| *method == "Input.dispatchKeyEvent").count(), 2);
        assert_eq!(cdp.expressions().len(), 2);

        // A tab neither engine knows.
        assert_eq!(
            eval(app.handle(), &manager, "page-seam-missing", "1".into()).await,
            Err(BrowserError::NotFound("page-seam-missing".into()))
        );
    }

    #[tokio::test]
    async fn capture_of_an_engine_tab_is_a_cdp_screenshot() {
        use base64::Engine as _;
        let app = mock_app();
        let manager = Arc::new(BrowserManager::new());
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend_from_slice(&320u32.to_be_bytes());
        png.extend_from_slice(&200u32.to_be_bytes());
        let data = base64::engine::general_purpose::STANDARD.encode(&png);
        let cdp = FakeCdp::start("page-capture", move |_, _| Reply::Ok(json!({ "data": data })));
        session(&manager, &cdp.tab);
        let snapshot = capture(app.handle(), &manager, &cdp.tab, SnapshotOptions::png()).await.unwrap();
        assert_eq!((snapshot.width, snapshot.height, snapshot.bytes), (320, 200, png));
        assert_eq!(cdp.methods(), ["Page.captureScreenshot"]);

        let missing = capture(app.handle(), &manager, "page-capture-missing", SnapshotOptions::png())
            .await
            .expect_err("unknown tab");
        assert_eq!(missing.code, IpcErrorCode::BrowserNotFound);
    }

    #[tokio::test]
    async fn snapshot_then_act_on_an_engine_tab_runs_isolated_with_trusted_input() {
        let app = mock_app();
        let manager = Arc::new(BrowserManager::new());
        let checks = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let sampled = Arc::clone(&checks);
        let cdp = FakeCdp::start("page-flow", move |method, params| {
            world(method, 21).unwrap_or_else(|| match (method, op_of(params)) {
                ("Runtime.evaluate", "observe") => evaluated(observation()),
                ("Runtime.evaluate", "point") => evaluated(json!({ "ok": true, "point": { "x": 10.0, "y": 20.0 } })),
                // The postcondition is not met before the click and met after it.
                ("Runtime.evaluate", "check") => {
                    let seen = sampled.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    evaluated(json!({ "ok": true, "matched": seen > 0 }))
                }
                ("Runtime.evaluate", other) => panic!("unexpected {other} script"),
                _ => Reply::Ok(json!({})),
            })
        });
        let state = session(&manager, &cdp.tab);

        let snapshot = browser_automation_snapshot(app.handle().clone(), &manager, cdp.tab.clone()).await.unwrap();
        assert!(snapshot.isolated);
        assert_eq!(snapshot.generation, state.generation);
        assert_eq!(snapshot.elements.len(), 2);
        assert_eq!((snapshot.elements[0].reference.as_str(), snapshot.elements[0].name.as_str()), ("d1.1", "Save"));
        assert!(!snapshot.url.contains("abc123secret"), "{}", snapshot.url);
        assert!(!snapshot.title.contains("ghp_"), "{}", snapshot.title);
        assert!(cdp.expressions()[0].contains(r#""isolated":true"#));
        assert_eq!(manager.observed_ref(&cdp.tab, state.generation, "d1.1").unwrap(), SAVE);

        let outcome = browser_automation_act(
            app.handle().clone(),
            &manager,
            BrowserAutomationRequest {
                browser_id: cdp.tab.clone(),
                generation: snapshot.generation,
                action: BrowserAutomationAction::Click { reference: "d1.1".into() },
                postcondition: Some(BrowserWaitCondition::Text { text: "Saved".into() }),
            },
        )
        .await
        .unwrap();
        assert!(outcome.dispatched);
        assert_eq!(outcome.applied, Some(true));
        let ops: Vec<&str> = cdp
            .calls()
            .iter()
            .filter(|(method, _)| method == "Runtime.evaluate")
            .map(|(_, params)| op_of(params))
            .collect();
        assert_eq!(ops, ["observe", "check", "point", "check"]);
        assert!(cdp
            .calls()
            .iter()
            .filter(|(method, _)| method == "Runtime.evaluate")
            .all(|(_, params)| params["contextId"] == 21));
        assert_eq!(cdp.methods().iter().filter(|method| *method == "Input.dispatchMouseEvent").count(), 3);
        assert!(cdp.expressions()[2].contains(&serde_json::to_string(SAVE).unwrap()));
    }

    #[tokio::test]
    async fn a_postcondition_already_shown_reports_no_applied() {
        let app = mock_app();
        let manager = Arc::new(BrowserManager::new());
        let cdp = FakeCdp::start("page-already", |method, params| {
            world(method, 22).unwrap_or_else(|| match op_of(params) {
                "observe" => evaluated(observation()),
                "check" => evaluated(json!({ "ok": true, "matched": true })),
                _ => evaluated(json!({ "ok": true })),
            })
        });
        session(&manager, &cdp.tab);
        let snapshot = browser_automation_snapshot(app.handle().clone(), &manager, cdp.tab.clone()).await.unwrap();
        let outcome = browser_automation_act(
            app.handle().clone(),
            &manager,
            BrowserAutomationRequest {
                browser_id: cdp.tab.clone(),
                generation: snapshot.generation,
                action: BrowserAutomationAction::Fill { reference: "d1.2".into(), value: "a@b.test".into() },
                postcondition: Some(BrowserWaitCondition::UrlContains { fragment: "/form".into() }),
            },
        )
        .await
        .unwrap();
        assert!(outcome.dispatched);
        assert_eq!(outcome.applied, None, "a state the page already showed proves nothing");
        let ops: Vec<&str> = cdp
            .calls()
            .iter()
            .filter(|(method, _)| method == "Runtime.evaluate")
            .map(|(_, params)| op_of(params))
            .collect();
        assert_eq!(ops, ["observe", "check", "act"]);
        assert!(cdp.expressions()[2].contains(r#""expect":"input\ntextbox\n""#));
    }

    #[tokio::test]
    async fn a_ref_from_a_stale_snapshot_never_reaches_the_page() {
        let app = mock_app();
        let manager = Arc::new(BrowserManager::new());
        let cdp = FakeCdp::start("page-stale", |method, params| {
            world(method, 23).unwrap_or_else(|| match op_of(params) {
                "observe" => evaluated(observation()),
                _ => panic!("a stale ref must not touch the page"),
            })
        });
        session(&manager, &cdp.tab);
        let snapshot = browser_automation_snapshot(app.handle().clone(), &manager, cdp.tab.clone()).await.unwrap();
        let request = |generation: u64, reference: &str, postcondition: Option<BrowserWaitCondition>| BrowserAutomationRequest {
            browser_id: cdp.tab.clone(),
            generation,
            action: BrowserAutomationAction::Click { reference: reference.into() },
            postcondition,
        };

        // A ref the observe never issued.
        let unknown = browser_automation_act(app.handle().clone(), &manager, request(snapshot.generation, "d1.99", None))
            .await
            .expect_err("unknown ref");
        assert_eq!(unknown.code, IpcErrorCode::BrowserInvalidReference);
        // A postcondition that would run a script is refused before anything else.
        let scripted = browser_automation_act(
            app.handle().clone(),
            &manager,
            request(snapshot.generation, "d1.1", Some(BrowserWaitCondition::Function { script: "true".into() })),
        )
        .await
        .expect_err("script postcondition");
        assert_eq!(scripted.code, IpcErrorCode::BrowserAutomationFailed);

        // The tab reloaded after the snapshot: its refs are stale.
        manager.begin_reload(&cdp.tab).unwrap();
        let stale = browser_automation_act(app.handle().clone(), &manager, request(snapshot.generation, "d1.1", None))
            .await
            .expect_err("stale snapshot");
        assert_eq!(stale.code, IpcErrorCode::BrowserStaleRef);
        let current = manager.get_state(&cdp.tab).unwrap().generation;
        let unobserved = browser_automation_act(app.handle().clone(), &manager, request(current, "d1.1", None))
            .await
            .expect_err("nothing observed in the new document");
        assert_eq!(unobserved.code, IpcErrorCode::BrowserInvalidReference);

        assert_eq!(cdp.expressions().len(), 1, "only the observe ran");
        assert!(!cdp.methods().iter().any(|method| method.starts_with("Input.")));
    }

    #[tokio::test]
    async fn a_node_replaced_since_the_observe_fails_as_stale_in_the_page() {
        let app = mock_app();
        let manager = Arc::new(BrowserManager::new());
        let cdp = FakeCdp::start("page-replaced", |method, params| {
            world(method, 24).unwrap_or_else(|| match op_of(params) {
                "observe" => evaluated(observation()),
                _ => evaluated(json!({ "ok": false, "code": "changed", "message": "the element changed since the observe; observe again" })),
            })
        });
        session(&manager, &cdp.tab);
        let snapshot = browser_automation_snapshot(app.handle().clone(), &manager, cdp.tab.clone()).await.unwrap();
        let error = browser_automation_act(
            app.handle().clone(),
            &manager,
            BrowserAutomationRequest {
                browser_id: cdp.tab.clone(),
                generation: snapshot.generation,
                action: BrowserAutomationAction::Click { reference: "d1.1".into() },
                postcondition: None,
            },
        )
        .await
        .expect_err("changed node");
        assert_eq!(error.code, IpcErrorCode::BrowserStaleRef);
        assert!(!cdp.methods().iter().any(|method| method.starts_with("Input.")));
    }

    #[tokio::test]
    async fn an_untargeted_act_on_an_engine_tab_is_trusted_input_on_every_os() {
        let app = mock_app();
        let manager = Arc::new(BrowserManager::new());
        let cdp = FakeCdp::start("page-keys", |_, params| match op_of(params) {
            "page" if params.get("expression").is_some() => Reply::Ok(json!({ "result": { "type": "string", "value": "ok" } })),
            _ => Reply::Ok(json!({})),
        });
        let state = session(&manager, &cdp.tab);
        let request = |action: BrowserAutomationAction, generation: u64| BrowserAutomationRequest {
            browser_id: cdp.tab.clone(),
            generation,
            action,
            postcondition: None,
        };
        let enter = || BrowserAutomationAction::Keypress { key: "Enter".into() };
        let outcome = browser_automation_act(app.handle().clone(), &manager, request(enter(), state.generation))
            .await
            .unwrap();
        assert!(outcome.dispatched && outcome.applied.is_none());
        let scroll = BrowserAutomationAction::Scroll { reference: None, x: None, y: Some(300) };
        browser_automation_act(app.handle().clone(), &manager, request(scroll, state.generation))
            .await
            .unwrap();
        assert_eq!(cdp.methods(), ["Input.dispatchKeyEvent", "Input.dispatchKeyEvent", "Runtime.evaluate"]);
        // The scroll script is a page script, not an agent one.
        assert!(cdp.calls()[2].1.get("contextId").is_none());

        let stale = browser_automation_act(app.handle().clone(), &manager, request(enter(), state.generation + 1))
            .await
            .expect_err("wrong generation");
        assert_eq!(stale.code, IpcErrorCode::BrowserStaleRef);
        assert_eq!(cdp.calls().len(), 3);
    }
}
