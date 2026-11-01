//! Observe/act for Chromium (CEF) engine tabs, driven over the engine's in-process DevTools
//! message API. The engine opens no DevTools port: every call travels the engine's stdin/stdout
//! line protocol (`Command::Devtools` / `Event::Devtools`), so nothing else on the machine can
//! reach it. System webview tabs keep their own path in `ipc::browser`.
//!
//! Clicks and keys are trusted input (`Input.dispatch*Event`), unlike the synthetic DOM events
//! the webview path has to use.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::LazyLock;
use std::time::Duration;

use parking_lot::Mutex;

use serde_json::{json, Value};

use super::{cdp, observe};
use crate::browser::cef_engine;
use crate::browser::model::BrowserAutomationAction;
use crate::browser::snapshot_source::{BrowserSnapshot, BrowserSnapshotSource, SnapshotFormat, SnapshotOptions};
use crate::browser::BrowserError;
use crate::ipc::error::IpcError;

/// One DevTools call; matches the webview evaluation timeout.
const CALL_TIMEOUT: Duration = Duration::from_secs(5);
/// A screenshot of a large page can take longer than a script.
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(15);

async fn call(browser_id: &str, method: &str, params: Value) -> Result<Value, BrowserError> {
    cef_engine::devtools(browser_id, method, params, CALL_TIMEOUT).await
}

/// Evaluates a page script and returns its value as JSON text (`undefined` for none), the
/// same shape the system webview's evaluation callback reports.
pub async fn eval(browser_id: &str, script: &str) -> Result<String, BrowserError> {
    let result = call(browser_id, "Runtime.evaluate", cdp::evaluate_params(script)).await?;
    cdp::evaluate_result(&result)
}

/// Agent scripts run in an isolated world of the tab's main frame: its globals and DOM
/// wrappers are invisible to the page's scripts, so the ref store and the observer's helpers
/// cannot be read or replaced by the page. CDP creates a new world per call, so its context
/// is kept per tab until a navigation destroys it.
static WORLDS: LazyLock<Mutex<HashMap<String, i64>>> = LazyLock::new(Default::default);
const WORLD_NAME: &str = "ferryx-agent";

async fn isolated_context(browser_id: &str) -> Result<i64, BrowserError> {
    if let Some(context) = WORLDS.lock().get(browser_id).copied() {
        return Ok(context);
    }
    let tree = call(browser_id, "Page.getFrameTree", json!({})).await?;
    let frame = tree
        .pointer("/frameTree/frame/id")
        .and_then(Value::as_str)
        .ok_or_else(|| BrowserError::AutomationFailed("the page has no main frame".into()))?
        .to_string();
    let world = call(
        browser_id,
        "Page.createIsolatedWorld",
        json!({ "frameId": frame, "worldName": WORLD_NAME }),
    )
    .await?;
    let context = world
        .get("executionContextId")
        .and_then(Value::as_i64)
        .ok_or_else(|| BrowserError::AutomationFailed("Page.createIsolatedWorld returned no context".into()))?;
    WORLDS.lock().insert(browser_id.to_string(), context);
    Ok(context)
}

/// Evaluates an agent script in the tab's isolated world. A context the last navigation
/// destroyed is replaced once; any other failure is returned, never retried (an act must not
/// run twice).
pub async fn eval_isolated(browser_id: &str, script: &str) -> Result<String, BrowserError> {
    let mut replaced = false;
    loop {
        let context = isolated_context(browser_id).await?;
        let mut params = cdp::evaluate_params(script);
        params["contextId"] = json!(context);
        match call(browser_id, "Runtime.evaluate", params).await {
            Ok(result) => return cdp::evaluate_result(&result),
            Err(BrowserError::AutomationFailed(message)) if !replaced && cdp::is_lost_context(&message) => {
                WORLDS.lock().remove(browser_id);
                replaced = true;
            }
            Err(error) => return Err(error),
        }
    }
}

/// Drops the tab's cached world (tab closed).
pub fn forget(browser_id: &str) {
    WORLDS.lock().remove(browser_id);
}

/// Runs an element action addressed by ref. Clicks are trusted mouse input at the element's
/// center when nothing covers it; typing focuses the element and inserts the text as trusted
/// input; everything else (and a covered click) runs the page-side action.
pub async fn act_element(
    browser_id: &str,
    action: &BrowserAutomationAction,
    expect: &str,
) -> Result<(), BrowserError> {
    let clicks = match action {
        BrowserAutomationAction::Click { .. } => Some(1),
        BrowserAutomationAction::Dblclick { .. } => Some(2),
        _ => None,
    };
    if let (Some(clicks), Some(reference)) = (clicks, observe::action_reference(action)) {
        let raw = eval_isolated(browser_id, &observe::point_script(reference, expect, true)).await?;
        if let Some((x, y)) = observe::parse_point(&raw)? {
            for event in cdp::mouse_click_events(x, y, clicks) {
                call(browser_id, "Input.dispatchMouseEvent", event).await?;
            }
            return Ok(());
        }
    }
    if let BrowserAutomationAction::Type { reference: Some(reference), text } = action {
        let focus = BrowserAutomationAction::Focus { reference: reference.clone() };
        let script = observe::act_script(&focus, expect, true)
            .ok_or_else(|| BrowserError::AutomationFailed("focus addresses no element".into()))?;
        observe::parse_act(&eval_isolated(browser_id, &script).await?)?;
        call(browser_id, "Input.insertText", json!({ "text": text })).await?;
        return Ok(());
    }
    let script = observe::act_script(action, expect, true)
        .ok_or_else(|| BrowserError::AutomationFailed("the action addresses no element".into()))?;
    observe::parse_act(&eval_isolated(browser_id, &script).await?)
}

/// Performs the trusted-input form of an action that addresses no element (keypress, typing
/// into the focused element). Returns `false` when it has none, so the caller runs its script.
pub async fn act(browser_id: &str, action: &BrowserAutomationAction) -> Result<bool, BrowserError> {
    match action {
        BrowserAutomationAction::Keypress { key } => {
            let keypress = crate::ipc::browser::parse_keypress(key)?;
            for event in cdp::key_events(&keypress) {
                call(browser_id, "Input.dispatchKeyEvent", event).await?;
            }
            Ok(true)
        }
        BrowserAutomationAction::Type { reference: None, text } => {
            call(browser_id, "Input.insertText", json!({ "text": text })).await?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// A PNG of the tab's visible page.
pub async fn capture(browser_id: &str) -> Result<BrowserSnapshot, BrowserError> {
    let result = cef_engine::devtools(browser_id, "Page.captureScreenshot", cdp::screenshot_params(), CAPTURE_TIMEOUT)
        .await?;
    let (bytes, width, height) = cdp::screenshot_png(&result)?;
    Ok(BrowserSnapshot::new(bytes, SnapshotFormat::Png, width, height))
}

/// The shared snapshot trait for one engine tab, so screenshots to a file reuse the same
/// writer as webview tabs. The label argument is ignored: the tab is fixed.
pub struct EngineSnapshotSource<'a> {
    pub browser_id: &'a str,
}

impl BrowserSnapshotSource for EngineSnapshotSource<'_> {
    fn supported_formats(&self) -> Vec<String> {
        vec!["png".into()]
    }

    fn capture_snapshot<'a>(
        &'a self,
        _webview_label: &'a str,
        _options: SnapshotOptions,
    ) -> Pin<Box<dyn Future<Output = Result<BrowserSnapshot, IpcError>> + Send + 'a>> {
        Box::pin(async move { Ok(capture(self.browser_id).await?) })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicI64, Ordering};
    use std::sync::Arc;

    use serde_json::{json, Value};

    use super::super::fake_cdp::{evaluated, op_of, world, FakeCdp, Reply};
    use super::*;

    const SAVE: &str = "button\nbutton\nSave";

    fn click(reference: &str) -> BrowserAutomationAction {
        BrowserAutomationAction::Click { reference: reference.into() }
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend_from_slice(&width.to_be_bytes());
        png.extend_from_slice(&height.to_be_bytes());
        png
    }

    #[tokio::test]
    async fn eval_translates_to_runtime_evaluate_in_the_page_world() {
        let cdp = FakeCdp::start("eval", |method, params| match (method, params["expression"].as_str()) {
            ("Runtime.evaluate", Some("document.title")) => Reply::Ok(json!({ "result": { "type": "string", "value": "Fixture" } })),
            ("Runtime.evaluate", Some("void 0")) => Reply::Ok(json!({ "result": { "type": "undefined" } })),
            _ => Reply::Ok(json!({
                "result": { "type": "object" },
                "exceptionDetails": { "text": "Uncaught", "exception": { "description": "ReferenceError: nope is not defined" } }
            })),
        });
        assert_eq!(eval(&cdp.tab, "document.title").await.unwrap(), "\"Fixture\"");
        assert_eq!(eval(&cdp.tab, "void 0").await.unwrap(), "undefined");
        assert_eq!(
            eval(&cdp.tab, "nope()").await,
            Err(BrowserError::AutomationFailed("ReferenceError: nope is not defined".into()))
        );
        let calls = cdp.calls();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0], ("Runtime.evaluate".to_string(), cdp::evaluate_params("document.title")));
        // A page script runs in the page's own world: no isolated context is created or named.
        assert!(calls.iter().all(|(_, params)| params.get("contextId").is_none()));
    }

    #[tokio::test]
    async fn agent_scripts_run_in_one_cached_isolated_world_of_the_main_frame() {
        let cdp = FakeCdp::start("isolated", |method, params| {
            world(method, 41).unwrap_or_else(|| {
                assert_eq!(params["contextId"], 41, "agent scripts never run in the page world");
                evaluated(json!({ "ok": true }))
            })
        });
        for _ in 0..3 {
            assert_eq!(eval_isolated(&cdp.tab, "1").await.unwrap(), r#"{"ok":true}"#);
        }
        assert_eq!(
            cdp.methods(),
            ["Page.getFrameTree", "Page.createIsolatedWorld", "Runtime.evaluate", "Runtime.evaluate", "Runtime.evaluate"]
        );
        assert_eq!(cdp.calls()[1].1, json!({ "frameId": "main-frame", "worldName": WORLD_NAME }));
        assert!(cdp.calls()[2].1["userGesture"].as_bool().unwrap());

        // A closed tab forgets its world; the next script creates a fresh one.
        forget(&cdp.tab);
        eval_isolated(&cdp.tab, "1").await.unwrap();
        assert_eq!(cdp.methods().iter().filter(|method| *method == "Page.createIsolatedWorld").count(), 2);
    }

    #[tokio::test]
    async fn a_world_the_last_navigation_destroyed_is_replaced_once() {
        let next = Arc::new(AtomicI64::new(1));
        let worlds = Arc::clone(&next);
        let cdp = FakeCdp::start("navigated", move |method, params| match method {
            "Page.createIsolatedWorld" => {
                Reply::Ok(json!({ "executionContextId": worlds.fetch_add(1, Ordering::SeqCst) }))
            }
            "Runtime.evaluate" if params["contextId"] == 1 => Reply::Fail("Cannot find context with specified id"),
            other => world(other, 0).unwrap_or_else(|| evaluated(json!("fresh"))),
        });
        assert_eq!(eval_isolated(&cdp.tab, "1").await.unwrap(), r#""fresh""#);
        let contexts: Vec<Value> = cdp
            .calls()
            .into_iter()
            .filter(|(method, _)| method == "Runtime.evaluate")
            .map(|(_, params)| params["contextId"].clone())
            .collect();
        assert_eq!(contexts, [json!(1), json!(2)]);
        assert_eq!(next.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn a_world_that_stays_lost_or_any_other_failure_is_not_retried() {
        let lost = FakeCdp::start("lost", |method, _| {
            world(method, 9).unwrap_or(Reply::Fail("Execution context was destroyed."))
        });
        assert_eq!(
            eval_isolated(&lost.tab, "1").await,
            Err(BrowserError::AutomationFailed("Runtime.evaluate: Execution context was destroyed.".into()))
        );
        assert_eq!(lost.expressions().len(), 2, "replaced once, then the failure is returned");

        // An act must never run twice: only a lost context is retried.
        let failing = FakeCdp::start("failing", |method, _| world(method, 9).unwrap_or(Reply::Fail("Internal error")));
        assert_eq!(
            eval_isolated(&failing.tab, "1").await,
            Err(BrowserError::AutomationFailed("Runtime.evaluate: Internal error".into()))
        );
        assert_eq!(failing.expressions().len(), 1);
    }

    #[tokio::test]
    async fn a_page_without_a_main_frame_or_world_context_is_an_automation_failure() {
        let frameless = FakeCdp::start("frameless", |_, _| Reply::Ok(json!({ "frameTree": {} })));
        assert_eq!(
            eval_isolated(&frameless.tab, "1").await,
            Err(BrowserError::AutomationFailed("the page has no main frame".into()))
        );
        let contextless = FakeCdp::start("contextless", |method, _| match method {
            "Page.createIsolatedWorld" => Reply::Ok(json!({})),
            other => world(other, 0).unwrap_or(Reply::Silent),
        });
        assert_eq!(
            eval_isolated(&contextless.tab, "1").await,
            Err(BrowserError::AutomationFailed("Page.createIsolatedWorld returned no context".into()))
        );
        assert_eq!(contextless.expressions().len(), 0);
    }

    #[tokio::test]
    async fn a_clear_click_is_trusted_mouse_input_at_the_resolved_point() {
        let cdp = FakeCdp::start("click", |method, params| {
            world(method, 3).unwrap_or_else(|| match method {
                "Runtime.evaluate" => evaluated(json!({ "ok": true, "point": { "x": 40.5, "y": 12.0 } })),
                _ => {
                    assert_eq!(op_of(params), "page");
                    Reply::Ok(json!({}))
                }
            })
        });
        act_element(&cdp.tab, &click("d1.1"), SAVE).await.unwrap();
        let expressions = cdp.expressions();
        assert_eq!(expressions.len(), 1, "a trusted click runs no page-side click");
        assert!(expressions[0].contains(r#""type":"point""#));
        assert!(expressions[0].contains(r#""ref":"d1.1""#));
        assert!(expressions[0].contains(r#""expect":"button\nbutton\nSave""#));
        assert!(expressions[0].contains(r#""isolated":true"#));
        let mouse: Vec<Value> = cdp
            .calls()
            .into_iter()
            .filter(|(method, _)| method == "Input.dispatchMouseEvent")
            .map(|(_, params)| params)
            .collect();
        assert_eq!(mouse, cdp::mouse_click_events(40.5, 12.0, 1));

        let double = FakeCdp::start("dblclick", |method, _| {
            world(method, 3).unwrap_or_else(|| evaluated(json!({ "ok": true, "point": { "x": 1.0, "y": 2.0 } })))
        });
        act_element(&double.tab, &BrowserAutomationAction::Dblclick { reference: "d1.1".into() }, SAVE)
            .await
            .unwrap();
        let mouse: Vec<Value> = double
            .calls()
            .into_iter()
            .filter(|(method, _)| method == "Input.dispatchMouseEvent")
            .map(|(_, params)| params)
            .collect();
        assert_eq!(mouse, cdp::mouse_click_events(1.0, 2.0, 2));
    }

    #[tokio::test]
    async fn a_covered_click_falls_back_to_the_page_side_action() {
        let cdp = FakeCdp::start("covered", |method, params| {
            world(method, 3).unwrap_or_else(|| match op_of(params) {
                "point" => evaluated(json!({ "ok": true, "point": null })),
                _ => evaluated(json!({ "ok": true })),
            })
        });
        act_element(&cdp.tab, &click("d1.1"), SAVE).await.unwrap();
        let expressions = cdp.expressions();
        assert_eq!(expressions.len(), 2);
        assert!(expressions[1].contains(r#""action":{"type":"click"}"#));
        assert!(expressions[1].contains(r#""expect":"button\nbutton\nSave""#));
        assert!(!cdp.methods().iter().any(|method| method.starts_with("Input.")));
    }

    #[tokio::test]
    async fn a_stale_changed_or_unknown_ref_dispatches_no_input() {
        for (name, code, expected) in [
            ("stale-ref", "stale", BrowserError::AutomationSnapshotStale),
            ("changed-ref", "changed", BrowserError::AutomationSnapshotStale),
            ("unknown-ref", "unknown", BrowserError::AutomationTargetNotFound("no element has this ref".into())),
            ("unsupported-ref", "unsupported", BrowserError::AutomationFailed("no element has this ref".into())),
        ] {
            let cdp = FakeCdp::start(name, move |method, _| {
                world(method, 5).unwrap_or_else(|| {
                    evaluated(json!({ "ok": false, "code": code, "message": "no element has this ref" }))
                })
            });
            for action in [
                click("d1.1"),
                BrowserAutomationAction::Fill { reference: "d1.1".into(), value: "x".into() },
                BrowserAutomationAction::Type { reference: Some("d1.1".into()), text: "x".into() },
            ] {
                assert_eq!(act_element(&cdp.tab, &action, SAVE).await, Err(expected.clone()), "{name} {action:?}");
            }
            assert!(!cdp.methods().iter().any(|method| method.starts_with("Input.")), "{name}");
        }
    }

    #[tokio::test]
    async fn typing_into_a_ref_focuses_it_then_inserts_trusted_text() {
        let cdp = FakeCdp::start("type-ref", |method, _| {
            world(method, 6).unwrap_or_else(|| match method {
                "Runtime.evaluate" => evaluated(json!({ "ok": true })),
                _ => Reply::Ok(json!({})),
            })
        });
        let action = BrowserAutomationAction::Type { reference: Some("d1.2".into()), text: "hello".into() };
        act_element(&cdp.tab, &action, "input\ntextbox\n").await.unwrap();
        let expressions = cdp.expressions();
        assert_eq!(expressions.len(), 1);
        assert!(expressions[0].contains(r#""action":{"type":"focus"}"#));
        assert_eq!(cdp.calls().last().unwrap(), &("Input.insertText".to_string(), json!({ "text": "hello" })));
    }

    #[tokio::test]
    async fn other_element_actions_run_page_side_and_untargeted_ones_are_refused() {
        let cdp = FakeCdp::start("fill", |method, _| world(method, 7).unwrap_or_else(|| evaluated(json!({ "ok": true }))));
        let fill = BrowserAutomationAction::Fill { reference: "d1.2".into(), value: "a@b.test".into() };
        act_element(&cdp.tab, &fill, "input\ntextbox\n").await.unwrap();
        let hover = BrowserAutomationAction::Hover { reference: "d1.1".into() };
        act_element(&cdp.tab, &hover, SAVE).await.unwrap();
        let expressions = cdp.expressions();
        assert!(expressions[0].contains(r#""action":{"type":"fill","value":"a@b.test"}"#));
        assert!(expressions[1].contains(r#""action":{"type":"hover"}"#));
        assert!(!cdp.methods().iter().any(|method| method.starts_with("Input.")));

        let keypress = BrowserAutomationAction::Keypress { key: "Enter".into() };
        assert_eq!(
            act_element(&cdp.tab, &keypress, SAVE).await,
            Err(BrowserError::AutomationFailed("the action addresses no element".into()))
        );
        assert_eq!(cdp.expressions().len(), 2);
    }

    #[tokio::test]
    async fn untargeted_actions_become_trusted_key_and_text_input() {
        let cdp = FakeCdp::start("untargeted", |_, _| Reply::Ok(json!({})));
        assert!(act(&cdp.tab, &BrowserAutomationAction::Keypress { key: "Shift+Tab".into() }).await.unwrap());
        assert!(act(&cdp.tab, &BrowserAutomationAction::Type { reference: None, text: "typed".into() }).await.unwrap());
        assert!(!act(&cdp.tab, &BrowserAutomationAction::Scroll { reference: None, x: None, y: Some(200) }).await.unwrap());
        assert!(!act(&cdp.tab, &click("d1.1")).await.unwrap());
        let keys = cdp::key_events(&crate::ipc::browser::parse_keypress("Shift+Tab").unwrap());
        assert_eq!(
            cdp.calls(),
            [
                ("Input.dispatchKeyEvent".to_string(), keys[0].clone()),
                ("Input.dispatchKeyEvent".to_string(), keys[1].clone()),
                ("Input.insertText".to_string(), json!({ "text": "typed" })),
            ]
        );
        // A malformed key is refused before anything reaches the page.
        assert!(matches!(
            act(&cdp.tab, &BrowserAutomationAction::Keypress { key: "Hyper+x".into() }).await,
            Err(BrowserError::AutomationFailed(_))
        ));
        assert_eq!(cdp.calls().len(), 3);
    }

    #[tokio::test]
    async fn capture_reads_a_png_screenshot_through_the_snapshot_trait() {
        use base64::Engine as _;
        let data = base64::engine::general_purpose::STANDARD.encode(png(800, 600));
        let cdp = FakeCdp::start("capture", move |method, params| {
            assert_eq!((method, params), ("Page.captureScreenshot", &cdp::screenshot_params()));
            Reply::Ok(json!({ "data": data }))
        });
        let source = EngineSnapshotSource { browser_id: &cdp.tab };
        assert_eq!(source.supported_formats(), ["png"]);
        let snapshot = source.capture_snapshot("ignored-label", SnapshotOptions::png()).await.unwrap();
        assert_eq!((snapshot.width, snapshot.height, snapshot.format), (800, 600, SnapshotFormat::Png));
        assert_eq!(snapshot.bytes, png(800, 600));

        let broken = FakeCdp::start("capture-broken", |_, _| Reply::Ok(json!({ "data": "aGVsbG8=" })));
        let error = EngineSnapshotSource { browser_id: &broken.tab }
            .capture_snapshot("ignored-label", SnapshotOptions::png())
            .await
            .expect_err("not a PNG");
        assert_eq!(error.code, crate::ipc::error::IpcErrorCode::BrowserAutomationFailed);
    }

    #[tokio::test]
    async fn devtools_failures_map_to_typed_errors() {
        // A tab no engine serves.
        assert_eq!(eval("fake-cdp-never-opened", "1").await, Err(BrowserError::NotFound("fake-cdp-never-opened".into())));
        // An engine tab whose engine is not running.
        cef_engine::fake::orphan_tab("fake-cdp-profile-orphan", "fake-cdp-orphan");
        assert_eq!(
            eval("fake-cdp-orphan", "1").await,
            Err(BrowserError::AutomationFailed("the Chromium engine is not running".into()))
        );
        cef_engine::fake::remove("fake-cdp-profile-orphan");
        // A CDP error carries its method.
        let failing = FakeCdp::start("cdp-error", |_, _| Reply::Fail("Target closed"));
        assert_eq!(
            eval(&failing.tab, "1").await,
            Err(BrowserError::AutomationFailed("Runtime.evaluate: Target closed".into()))
        );
        // A malformed answer.
        let garbled = FakeCdp::start("garbled", |_, _| Reply::Raw("{not json"));
        assert!(matches!(
            eval(&garbled.tab, "1").await,
            Err(BrowserError::AutomationFailed(message)) if message.starts_with("Runtime.evaluate returned malformed JSON")
        ));
        // The engine dies with the call in flight: the call ends and the tab is gone.
        let dying = FakeCdp::start("dying", |_, _| Reply::Exit);
        assert_eq!(eval(&dying.tab, "1").await, Err(BrowserError::AutomationFailed("the Chromium engine stopped".into())));
        assert!(!cef_engine::is_cef_tab(&dying.tab));
    }

    #[tokio::test(start_paused = true)]
    async fn an_unanswered_call_times_out() {
        let silent = FakeCdp::start("silent", |_, _| Reply::Silent);
        assert_eq!(
            eval(&silent.tab, "1").await,
            Err(BrowserError::AutomationFailed("Runtime.evaluate timed out in the Chromium engine".into()))
        );
    }
}
