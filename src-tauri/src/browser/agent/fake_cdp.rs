//! A scripted DevTools endpoint for unit tests: a stand-in Chromium engine (see
//! `cef_engine::fake`) that records every CDP call Ferryx sends to one tab and answers it from
//! a test-supplied function, so the CEF observe/act path runs on every OS without an engine.

use std::sync::Arc;

use ferryx_browser_engine_protocol::{Command as EngineCommand, Event};
use parking_lot::Mutex;
use serde_json::{json, Value};

use crate::browser::cef_engine;

/// How the stand-in engine answers one call.
pub enum Reply {
    /// The CDP result object.
    Ok(Value),
    /// A CDP error with this message.
    Fail(&'static str),
    /// A success answer whose result text is sent as is.
    Raw(&'static str),
    /// No answer at all.
    Silent,
    /// The engine exits after reading the call.
    Exit,
}

type Calls = Arc<Mutex<Vec<(String, Value)>>>;

/// One stand-in engine serving one tab. Dropping it stops the engine and forgets the tab.
pub struct FakeCdp {
    pub tab: String,
    profile: String,
    calls: Calls,
}

impl FakeCdp {
    /// Starts an engine for the tab `name` (unique per test: engine state is process wide).
    pub fn start(name: &str, respond: impl Fn(&str, &Value) -> Reply + Send + Sync + 'static) -> Self {
        let tab = format!("fake-cdp-{name}");
        let profile = format!("fake-cdp-profile-{name}");
        let calls: Calls = Arc::default();
        let recorded = Arc::clone(&calls);
        cef_engine::fake::install(
            &profile,
            &[tab.as_str()],
            Arc::new(move |line: &str| {
                let Some(EngineCommand::Devtools { tab, id, method, params }) = EngineCommand::parse(line) else {
                    return Some(Vec::new());
                };
                let params: Value = serde_json::from_str(&params).expect("CDP params are JSON");
                recorded.lock().push((method.clone(), params.clone()));
                let (ok, result) = match respond(&method, &params) {
                    Reply::Ok(result) => (true, result.to_string()),
                    Reply::Fail(message) => (false, json!({ "code": -32000, "message": message }).to_string()),
                    Reply::Raw(text) => (true, text.to_string()),
                    Reply::Silent => return Some(Vec::new()),
                    Reply::Exit => return None,
                };
                Some(vec![Event::Devtools { tab, id, ok, result }.to_line()])
            }),
        );
        Self { tab, profile, calls }
    }

    /// Every call so far: method and params.
    pub fn calls(&self) -> Vec<(String, Value)> {
        self.calls.lock().clone()
    }

    /// The methods called so far, in order.
    pub fn methods(&self) -> Vec<String> {
        self.calls.lock().iter().map(|(method, _)| method.clone()).collect()
    }

    /// The `Runtime.evaluate` expressions sent so far, in order.
    pub fn expressions(&self) -> Vec<String> {
        self.calls
            .lock()
            .iter()
            .filter(|(method, _)| method == "Runtime.evaluate")
            .map(|(_, params)| params["expression"].as_str().unwrap_or_default().to_string())
            .collect()
    }
}

impl Drop for FakeCdp {
    fn drop(&mut self) {
        cef_engine::fake::remove(&self.profile);
        super::cef_backend::forget(&self.tab);
    }
}

/// A `Runtime.evaluate` result carrying `value` by value, as CDP returns a page object.
pub fn evaluated(value: Value) -> Reply {
    Reply::Ok(json!({ "result": { "type": "object", "value": value } }))
}

/// The main-frame and isolated-world answers every agent script in an engine tab needs first;
/// `None` for any other method.
pub fn world(method: &str, context: i64) -> Option<Reply> {
    match method {
        "Page.getFrameTree" => Some(Reply::Ok(json!({ "frameTree": { "frame": { "id": "main-frame" } } }))),
        "Page.createIsolatedWorld" => Some(Reply::Ok(json!({ "executionContextId": context }))),
        _ => None,
    }
}

/// Which observe.js op an evaluation runs (`observe`, `check`, `point`, `act`), or `page` for a
/// plain page script.
pub fn op_of(params: &Value) -> &'static str {
    let expression = params["expression"].as_str().unwrap_or_default();
    if expression.contains(r#""op":"observe""#) {
        "observe"
    } else if expression.contains(r#""op":"check""#) {
        "check"
    } else if expression.contains(r#""type":"point""#) {
        "point"
    } else if expression.contains(r#""op":"act""#) {
        "act"
    } else {
        "page"
    }
}
