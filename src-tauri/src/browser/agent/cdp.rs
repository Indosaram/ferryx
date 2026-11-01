//! Chrome DevTools Protocol requests and answers for Chromium (CEF) engine tabs.
//!
//! The engine runs each call through CEF's in-process DevTools message API (no port), so
//! this module only builds request params and reads results (pure, unit tested); the calls
//! themselves live in `browser::cef_engine`. Built where the engine exists (macOS, Windows).

use serde_json::{json, Value};

use crate::browser::BrowserError;

/// `Runtime.evaluate` params for a page script: the value comes back by value, a returned
/// promise is awaited, and the call counts as a user gesture (as a click in the page would).
pub fn evaluate_params(script: &str) -> Value {
    json!({
        "expression": script,
        "returnByValue": true,
        "awaitPromise": true,
        "userGesture": true,
    })
}

/// Reads a `Runtime.evaluate` result in the shape the system webview reports: the JSON text of
/// the value, or `undefined`. A thrown exception is an automation failure carrying its text.
pub fn evaluate_result(result: &Value) -> Result<String, BrowserError> {
    if let Some(details) = result.get("exceptionDetails") {
        let text = details
            .pointer("/exception/description")
            .and_then(Value::as_str)
            .or_else(|| details.get("text").and_then(Value::as_str))
            .unwrap_or("page script threw");
        return Err(BrowserError::AutomationFailed(text.to_string()));
    }
    let remote = result
        .get("result")
        .ok_or_else(|| BrowserError::AutomationFailed("Runtime.evaluate returned no result".into()))?;
    if remote.get("type").and_then(Value::as_str) == Some("undefined") {
        return Ok("undefined".into());
    }
    match remote.get("value") {
        Some(value) => Ok(value.to_string()),
        // A value that cannot be returned by value (a DOM node, a function) reads as null.
        None => Ok("null".into()),
    }
}

/// Script that scrolls the element into view and returns the viewport point a real click
/// lands on, or `null` when something else covers that point or the element has no box.
pub fn click_point_script(selector: &str) -> Result<String, BrowserError> {
    let selector = serde_json::to_string(selector)
        .map_err(|error| BrowserError::AutomationFailed(error.to_string()))?;
    Ok(format!(
        "(() => {{ const element = document.querySelector({selector}); if (!element) throw new Error('element disappeared'); \
element.scrollIntoView({{ behavior: 'instant', block: 'center', inline: 'center' }}); \
const rect = element.getBoundingClientRect(); if (rect.width <= 0 || rect.height <= 0) return null; \
const x = rect.left + rect.width / 2; const y = rect.top + rect.height / 2; \
const hit = document.elementFromPoint(x, y); if (!hit || !(hit === element || element.contains(hit))) return null; \
return {{ x, y }}; }})()"
    ))
}

/// Reads the point [`click_point_script`] returned (already decoded from its JSON text).
pub fn click_point(value: &str) -> Option<(f64, f64)> {
    let value: Value = serde_json::from_str(value).ok()?;
    Some((value.get("x")?.as_f64()?, value.get("y")?.as_f64()?))
}

/// The `Input.dispatchMouseEvent` calls of one trusted left click at a viewport point:
/// move, press, release (`click_count` 2 for a double click).
pub fn mouse_click_events(x: f64, y: f64, click_count: u32) -> Vec<Value> {
    let mut events = vec![json!({ "type": "mouseMoved", "x": x, "y": y })];
    for count in 1..=click_count.max(1) {
        for (kind, buttons) in [("mousePressed", 1), ("mouseReleased", 0)] {
            events.push(json!({
                "type": kind,
                "x": x,
                "y": y,
                "button": "left",
                "buttons": buttons,
                "clickCount": count,
            }));
        }
    }
    events
}

/// Key modifiers as CDP encodes them: Alt 1, Control 2, Meta 4, Shift 8.
fn modifiers(keypress: &crate::ipc::browser::ParsedKeypress) -> u32 {
    u32::from(keypress.alt_key)
        | u32::from(keypress.ctrl_key) << 1
        | u32::from(keypress.meta_key) << 2
        | u32::from(keypress.shift_key) << 3
}

/// The `Input.dispatchKeyEvent` key-down and key-up of one trusted keypress.
pub fn key_events(keypress: &crate::ipc::browser::ParsedKeypress) -> Vec<Value> {
    let (code, text): (u32, Option<String>) = match keypress.key.as_str() {
        "Enter" => (13, Some("\r".into())),
        "Tab" => (9, None),
        "Escape" => (27, None),
        "Backspace" => (8, None),
        "Delete" => (46, None),
        "ArrowLeft" => (37, None),
        "ArrowUp" => (38, None),
        "ArrowRight" => (39, None),
        "ArrowDown" => (40, None),
        "Home" => (36, None),
        "End" => (35, None),
        "PageUp" => (33, None),
        "PageDown" => (34, None),
        single => {
            let upper = single.chars().next().map(|c| c.to_ascii_uppercase()).unwrap_or(' ');
            let code = if upper.is_ascii_alphanumeric() { u32::from(upper) } else { 0 };
            (code, Some(single.to_string()))
        }
    };
    // A shortcut (Control/Meta held) types nothing; the page sees only the key.
    let text = text.filter(|_| !keypress.ctrl_key && !keypress.meta_key);
    let modifiers = modifiers(keypress);
    let down_type = if text.is_some() { "keyDown" } else { "rawKeyDown" };
    let mut down = json!({
        "type": down_type,
        "key": keypress.key,
        "windowsVirtualKeyCode": code,
        "modifiers": modifiers,
    });
    if let Some(text) = text {
        down["text"] = Value::String(text.clone());
        down["unmodifiedText"] = Value::String(text);
    }
    let up = json!({
        "type": "keyUp",
        "key": keypress.key,
        "windowsVirtualKeyCode": code,
        "modifiers": modifiers,
    });
    vec![down, up]
}

/// `Page.captureScreenshot` params for a PNG of the visible page.
pub fn screenshot_params() -> Value {
    json!({ "format": "png", "fromSurface": true })
}

/// Decodes a `Page.captureScreenshot` result into PNG bytes and their pixel size.
pub fn screenshot_png(result: &Value) -> Result<(Vec<u8>, u32, u32), BrowserError> {
    use base64::Engine as _;
    let data = result
        .get("data")
        .and_then(Value::as_str)
        .ok_or_else(|| BrowserError::AutomationFailed("Page.captureScreenshot returned no data".into()))?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|error| BrowserError::AutomationFailed(format!("screenshot data: {error}")))?;
    let (width, height) = png_size(&bytes)
        .ok_or_else(|| BrowserError::AutomationFailed("screenshot is not a PNG".into()))?;
    Ok((bytes, width, height))
}

/// Width and height from a PNG's IHDR chunk.
fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
    if bytes.len() < 24 || !bytes.starts_with(SIGNATURE) || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    (width > 0 && height > 0).then_some((width, height))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::browser::parse_keypress;

    #[test]
    fn evaluate_results_read_like_the_system_webview() {
        assert_eq!(
            evaluate_result(&json!({"result": {"type": "string", "value": "{\"a\":1}"}})).unwrap(),
            "\"{\\\"a\\\":1}\""
        );
        assert_eq!(evaluate_result(&json!({"result": {"type": "boolean", "value": true}})).unwrap(), "true");
        assert_eq!(evaluate_result(&json!({"result": {"type": "undefined"}})).unwrap(), "undefined");
        assert_eq!(evaluate_result(&json!({"result": {"type": "object", "subtype": "node"}})).unwrap(), "null");
        let thrown = json!({
            "result": {"type": "object"},
            "exceptionDetails": {"text": "Uncaught", "exception": {"description": "Error: element disappeared"}}
        });
        assert_eq!(
            evaluate_result(&thrown),
            Err(BrowserError::AutomationFailed("Error: element disappeared".into()))
        );
        assert!(evaluate_result(&json!({})).is_err());
        assert_eq!(evaluate_params("1 + 1")["returnByValue"], true);
    }

    #[test]
    fn a_click_is_a_move_then_press_and_release_at_the_element_center() {
        let script = click_point_script("#a\"b").unwrap();
        assert!(script.contains(r##"document.querySelector("#a\"b")"##));
        assert_eq!(click_point(r#"{"x":10.5,"y":20}"#), Some((10.5, 20.0)));
        assert_eq!(click_point("null"), None);
        let events = mouse_click_events(10.5, 20.0, 2);
        let kinds: Vec<&str> = events.iter().map(|event| event["type"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["mouseMoved", "mousePressed", "mouseReleased", "mousePressed", "mouseReleased"]);
        assert_eq!(events[3]["clickCount"], 2);
        assert_eq!(events[1]["x"], 10.5);
    }

    #[test]
    fn keypresses_map_to_trusted_key_events() {
        let enter = key_events(&parse_keypress("Enter").unwrap());
        assert_eq!(enter[0]["type"], "keyDown");
        assert_eq!(enter[0]["text"], "\r");
        assert_eq!(enter[0]["windowsVirtualKeyCode"], 13);
        assert_eq!(enter[1]["type"], "keyUp");

        let select_all = key_events(&parse_keypress("Meta+a").unwrap());
        assert_eq!(select_all[0]["type"], "rawKeyDown");
        assert_eq!(select_all[0]["modifiers"], 4);
        assert_eq!(select_all[0]["windowsVirtualKeyCode"], u32::from(b'A'));
        assert!(select_all[0].get("text").is_none());

        let shifted = key_events(&parse_keypress("Control+Shift+ArrowRight").unwrap());
        assert_eq!(shifted[0]["modifiers"], 2 | 8);
        assert_eq!(shifted[0]["windowsVirtualKeyCode"], 39);
    }

    #[test]
    fn screenshots_decode_to_png_bytes_with_their_size() {
        use base64::Engine as _;
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend_from_slice(&640u32.to_be_bytes());
        png.extend_from_slice(&480u32.to_be_bytes());
        let data = base64::engine::general_purpose::STANDARD.encode(&png);
        let (bytes, width, height) = screenshot_png(&json!({ "data": data })).unwrap();
        assert_eq!((bytes, width, height), (png, 640, 480));
        assert!(screenshot_png(&json!({ "data": "aGVsbG8=" })).is_err());
        assert!(screenshot_png(&json!({})).is_err());
        assert_eq!(screenshot_params()["format"], "png");
    }
}
