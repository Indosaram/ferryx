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
