//! Observe/act for Chromium (CEF) engine tabs, driven over the engine's in-process DevTools
//! message API. The engine opens no DevTools port: every call travels the engine's stdin/stdout
//! line protocol (`Command::Devtools` / `Event::Devtools`), so nothing else on the machine can
//! reach it. System webview tabs keep their own path in `ipc::browser`.
//!
//! Clicks and keys are trusted input (`Input.dispatch*Event`), unlike the synthetic DOM events
//! the webview path has to use.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use serde_json::{json, Value};

use super::cdp;
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

async fn click_at(browser_id: &str, selector: &str, clicks: u32) -> Result<bool, BrowserError> {
    let raw = eval(browser_id, &cdp::click_point_script(selector)?).await?;
    let Some((x, y)) = cdp::click_point(&raw) else {
        return Ok(false);
    };
    for event in cdp::mouse_click_events(x, y, clicks) {
        call(browser_id, "Input.dispatchMouseEvent", event).await?;
    }
    Ok(true)
}

/// Performs the trusted-input part of an action. Returns `false` when the action has no
/// trusted form here (or a click target is covered), so the caller runs the DOM script.
pub async fn act(
    browser_id: &str,
    action: &BrowserAutomationAction,
    selector: Option<&str>,
) -> Result<bool, BrowserError> {
    match (action, selector) {
        (BrowserAutomationAction::Click { .. }, Some(selector)) => click_at(browser_id, selector, 1).await,
        (BrowserAutomationAction::Dblclick { .. }, Some(selector)) => click_at(browser_id, selector, 2).await,
        (BrowserAutomationAction::Keypress { key }, _) => {
            let keypress = crate::ipc::browser::parse_keypress(key)?;
            for event in cdp::key_events(&keypress) {
                call(browser_id, "Input.dispatchKeyEvent", event).await?;
            }
            Ok(true)
        }
        (BrowserAutomationAction::Type { text, .. }, selector) => {
            if let Some(selector) = selector {
                let selector = serde_json::to_string(selector)
                    .map_err(|error| BrowserError::AutomationFailed(error.to_string()))?;
                eval(
                    browser_id,
                    &format!(
                        "(() => {{ const element = document.querySelector({selector}); if (!element) throw new Error('element disappeared'); element.focus(); return 'ok'; }})()"
                    ),
                )
                .await?;
            }
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
