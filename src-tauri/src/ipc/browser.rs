use crate::browser::{
    browser_find_script, cookie_from_imported, download_url_to_path, parse_browser_find_callback,
    parse_browser_guest_action, parse_cookie_file, BrowserAutomationAction,
    BrowserAutomationElement, BrowserAutomationRequest, BrowserAutomationSnapshot,
    BrowserAutomationTarget, BrowserDownloadRequestedPayload, BrowserError, BrowserFindResult,
    BrowserGuestAction, BrowserManager, BrowserOpenRequestedPayload, BrowserProfileId,
    BrowserSessionSummary, BrowserShortcutRequestedPayload, BrowserState,
    BrowserStateChangedPayload, CreateBrowserRequest, ImportBrowserCookiesRequest,
    ImportBrowserCookiesResult, LogicalRect, BROWSER_CLEAR_FIND_SCRIPT,
    BROWSER_DOWNLOAD_REQUESTED_EVENT, BROWSER_GUEST_BRIDGE_SCRIPT, BROWSER_OPEN_REQUESTED_EVENT,
    BROWSER_SHORTCUT_REQUESTED_EVENT,
};
use crate::ipc::error::IpcError;
#[cfg(target_os = "macos")]
use parking_lot::Mutex;
use serde::Deserialize;
use std::sync::Arc;
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AutomationSnapshotResult {
    url: String,
    title: String,
    elements: Vec<AutomationSnapshotElement>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AutomationSnapshotElement {
    reference: String,
    selector: String,
    role: String,
    name: String,
    tag_name: String,
}

const AUTOMATION_SNAPSHOT_SCRIPT: &str = r#"(() => {
  const candidates = Array.from(document.querySelectorAll(
    'a[href], button, input, select, textarea, [role="button"], [role="link"], [contenteditable="true"]'
  ));
  const escape = (value) => CSS.escape(value);
  const selectorFor = (element) => {
    if (element.id) return `#${escape(element.id)}`;
    const segments = [];
    let current = element;
    while (current && current.nodeType === Node.ELEMENT_NODE && segments.length < 8) {
      const tag = current.tagName.toLowerCase();
      const siblings = current.parentElement
        ? Array.from(current.parentElement.children).filter((sibling) => sibling.tagName === current.tagName)
        : [];
      const index = siblings.indexOf(current) + 1;
      segments.unshift(siblings.length > 1 ? `${tag}:nth-of-type(${index})` : tag);
      current = current.parentElement;
    }
    return segments.join(' > ');
  };
  const roleFor = (element) => element.getAttribute('role') || element.tagName.toLowerCase();
  const nameFor = (element) => element.getAttribute('aria-label') || element.getAttribute('title') ||
    element.getAttribute('placeholder') || element.textContent.trim().replace(/\s+/g, ' ').slice(0, 160);
  return JSON.stringify({
    url: location.href,
    title: document.title,
    elements: candidates.slice(0, 200).map((element, index) => ({
      reference: `e${index + 1}`,
      selector: selectorFor(element),
      role: roleFor(element),
      name: nameFor(element),
      tagName: element.tagName.toLowerCase(),
    })),
  });
})()"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedKeypress {
    pub key: String,
    pub meta_key: bool,
    pub ctrl_key: bool,
    pub alt_key: bool,
    pub shift_key: bool,
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, PartialEq, Eq)]
struct MacosKeypressSpec {
    characters: String,
    key_code: u16,
    meta_key: bool,
    ctrl_key: bool,
    alt_key: bool,
    shift_key: bool,
}

fn tokenize_keypress(raw: &str) -> Result<Vec<&str>, BrowserError> {
    if raw.is_empty() {
        return Err(BrowserError::AutomationFailed(
            "keypress cannot be empty".into(),
        ));
    }

    if raw == "+" {
        return Ok(vec!["+"]);
    }

    let (prefix, has_trailing_plus) = if raw.ends_with("++") {
        (&raw[..raw.len() - 2], true)
    } else {
        (raw, false)
    };

    let mut tokens = Vec::new();
    for part in prefix.split('+') {
        if part.is_empty() {
            return Err(BrowserError::AutomationFailed(format!(
                "invalid keypress format: '{raw}'"
            )));
        }
        tokens.push(part);
    }

    if has_trailing_plus {
        tokens.push("+");
    }

    Ok(tokens)
}

pub fn parse_keypress(raw: &str) -> Result<ParsedKeypress, BrowserError> {
    let tokens = tokenize_keypress(raw)?;
    let mut meta_key = false;
    let mut ctrl_key = false;
    let mut alt_key = false;
    let mut shift_key = false;
    let mut base_key: Option<String> = None;

    for token in tokens {
        match token {
            "Meta" => {
                if meta_key {
                    return Err(BrowserError::AutomationFailed(format!(
                        "duplicate modifier 'Meta' in keypress '{raw}'"
                    )));
                }
                meta_key = true;
            }
            "Control" | "Ctrl" => {
                if ctrl_key {
                    return Err(BrowserError::AutomationFailed(format!(
                        "duplicate modifier '{token}' in keypress '{raw}'"
                    )));
                }
                ctrl_key = true;
            }
            "Alt" => {
                if alt_key {
                    return Err(BrowserError::AutomationFailed(format!(
                        "duplicate modifier 'Alt' in keypress '{raw}'"
                    )));
                }
                alt_key = true;
            }
            "Shift" => {
                if shift_key {
                    return Err(BrowserError::AutomationFailed(format!(
                        "duplicate modifier 'Shift' in keypress '{raw}'"
                    )));
                }
                shift_key = true;
            }
            "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown" | "Home" | "End" | "PageUp"
            | "PageDown" | "Backspace" | "Delete" | "Enter" | "Escape" | "Tab" => {
                if base_key.is_some() {
                    return Err(BrowserError::AutomationFailed(format!(
                        "multiple base keys in keypress '{raw}'"
                    )));
                }
                base_key = Some(token.to_string());
            }
            single if single.chars().count() == 1 => {
                if base_key.is_some() {
                    return Err(BrowserError::AutomationFailed(format!(
                        "multiple base keys in keypress '{raw}'"
                    )));
                }
                base_key = Some(single.to_string());
            }
            unsupported => {
                return Err(BrowserError::AutomationFailed(format!(
                    "unsupported key or modifier '{unsupported}' in keypress '{raw}'"
                )));
            }
        }
    }

    let key = base_key.ok_or_else(|| {
        BrowserError::AutomationFailed(format!("missing base key in keypress '{raw}'"))
    })?;

    Ok(ParsedKeypress {
        key,
        meta_key,
        ctrl_key,
        alt_key,
        shift_key,
    })
}

#[cfg(target_os = "macos")]
fn macos_keypress_spec(keypress: &ParsedKeypress) -> Option<MacosKeypressSpec> {
    let (characters, key_code) = match keypress.key.as_str() {
        "ArrowUp" => ('\u{f700}', 126),
        "ArrowDown" => ('\u{f701}', 125),
        "ArrowLeft" => ('\u{f702}', 123),
        "ArrowRight" => ('\u{f703}', 124),
        "Home" => ('\u{f729}', 115),
        "End" => ('\u{f72b}', 119),
        "PageUp" => ('\u{f72c}', 116),
        "PageDown" => ('\u{f72d}', 121),
        "Backspace" => ('\u{8}', 51),
        "Delete" => ('\u{f728}', 117),
        "Enter" => ('\r', 36),
        "Escape" => ('\u{1b}', 53),
        "Tab" => ('\t', 48),
        _ => return None,
    };

    Some(MacosKeypressSpec {
        characters: characters.to_string(),
        key_code,
        meta_key: keypress.meta_key,
        ctrl_key: keypress.ctrl_key,
        alt_key: keypress.alt_key,
        shift_key: keypress.shift_key,
    })
}

#[cfg(target_os = "macos")]
fn dispatch_macos_keypress<R: tauri::Runtime>(
    webview: &tauri::Webview<R>,
    keypress: &ParsedKeypress,
) -> Result<bool, BrowserError> {
    use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType};
    use objc2_foundation::{NSPoint, NSString};

    let Some(spec) = macos_keypress_spec(keypress) else {
        return Ok(false);
    };

    let mut modifiers = NSEventModifierFlags::empty();
    if spec.meta_key {
        modifiers.insert(NSEventModifierFlags::Command);
    }
    if spec.ctrl_key {
        modifiers.insert(NSEventModifierFlags::Control);
    }
    if spec.alt_key {
        modifiers.insert(NSEventModifierFlags::Option);
    }
    if spec.shift_key {
        modifiers.insert(NSEventModifierFlags::Shift);
    }

    let dispatch_result = Arc::new(Mutex::new(None));
    let result_slot = Arc::clone(&dispatch_result);
    webview
        .with_webview(move |platform_webview| unsafe {
            // SAFETY: Tauri invokes `with_webview` on this app-owned WebView's UI thread.
            // The platform handle is a WKWebView on macOS, as in the existing history and
            // navigation-state bridges above. The retained AppKit events live through both
            // synchronous responder calls and never escape this closure.
            let outcome = (|| -> Result<(), BrowserError> {
                let native: &objc2_web_kit::WKWebView = &*platform_webview.inner().cast();
                let characters = NSString::from_str(&spec.characters);
                let location = NSPoint { x: 0.0, y: 0.0 };
                let key_down = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
                    NSEventType::KeyDown,
                    location,
                    modifiers,
                    0.0,
                    0,
                    None,
                    &characters,
                    &characters,
                    false,
                    spec.key_code,
                )
                .ok_or_else(|| BrowserError::AutomationFailed("failed to create native keydown event".into()))?;
                let key_up = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
                    NSEventType::KeyUp,
                    location,
                    modifiers,
                    0.0,
                    0,
                    None,
                    &characters,
                    &characters,
                    false,
                    spec.key_code,
                )
                .ok_or_else(|| BrowserError::AutomationFailed("failed to create native keyup event".into()))?;

                native.keyDown(&key_down);
                native.keyUp(&key_up);
                Ok(())
            })();
            *result_slot.lock() = Some(outcome);
        })
        .map_err(|error| BrowserError::AutomationFailed(error.to_string()))?;
    dispatch_result.lock().take().ok_or_else(|| {
        BrowserError::AutomationFailed("native key dispatch did not execute".into())
    })??;

    Ok(true)
}

fn automation_script(
    action: &BrowserAutomationAction,
    selector: Option<&str>,
) -> Result<String, BrowserError> {
    match action {
        BrowserAutomationAction::Click { .. } => {
            let selector = serde_json::to_string(selector.ok_or_else(|| {
                BrowserError::AutomationFailed("missing snapshot selector".into())
            })?)
            .map_err(|error| BrowserError::AutomationFailed(error.to_string()))?;
            Ok(format!(
                "(() => {{ const element = document.querySelector({selector}); if (!element) throw new Error('element disappeared'); element.click(); return 'ok'; }})()"
            ))
        }
        BrowserAutomationAction::Fill { value, .. } => {
            let selector = serde_json::to_string(selector.ok_or_else(|| {
                BrowserError::AutomationFailed("missing snapshot selector".into())
            })?)
            .map_err(|error| BrowserError::AutomationFailed(error.to_string()))?;
            let value = serde_json::to_string(value)
                .map_err(|error| BrowserError::AutomationFailed(error.to_string()))?;
            Ok(format!(
                "(() => {{ const element = document.querySelector({selector}); if (!element) throw new Error('element disappeared'); element.focus(); element.value = {value}; element.dispatchEvent(new Event('input', {{ bubbles: true }})); element.dispatchEvent(new Event('change', {{ bubbles: true }})); return 'ok'; }})()"
            ))
        }
        BrowserAutomationAction::Keypress { key } => {
            let parsed = parse_keypress(key)?;
            let key = serde_json::to_string(&parsed.key)
                .map_err(|error| BrowserError::AutomationFailed(error.to_string()))?;
            let meta_key = parsed.meta_key;
            let ctrl_key = parsed.ctrl_key;
            let alt_key = parsed.alt_key;
            let shift_key = parsed.shift_key;
            Ok(format!(
                "(() => {{ const target = document.activeElement || document.body || document.documentElement; const init = {{ bubbles: true, cancelable: true, key: {key}, metaKey: {meta_key}, ctrlKey: {ctrl_key}, altKey: {alt_key}, shiftKey: {shift_key} }}; const keydown = new KeyboardEvent('keydown', init); const notPrevented = target ? target.dispatchEvent(keydown) : true; const keyup = new KeyboardEvent('keyup', init); if (target) {{ target.dispatchEvent(keyup); }} if (!notPrevented) {{ throw new Error('keydown prevented'); }} return 'ok'; }})()"
            ))
        }
    }
}

#[cfg(test)]
mod automation_tests {
    use super::*;

    #[test]
    fn fill_script_encodes_untrusted_values_as_json_strings() {
        let script = automation_script(
            &BrowserAutomationAction::Fill {
                reference: "e1".into(),
                value: "hello'); window.bad = true; //".into(),
            },
            Some("#email"),
        )
        .expect("build fill script");

        assert!(script.contains("element.value = \"hello'); window.bad = true; //\""));
        assert!(!script.contains("element.value = hello');"));
    }

    #[test]
    fn click_requires_snapshot_selector() {
        let error = automation_script(
            &BrowserAutomationAction::Click {
                reference: "e1".into(),
            },
            None,
        )
        .expect_err("click requires resolved snapshot target");

        assert_eq!(
            error,
            BrowserError::AutomationFailed("missing snapshot selector".into())
        );
    }

    #[test]
    fn parse_keypress_valid_combinations() {
        assert_eq!(
            parse_keypress("Meta+ArrowLeft").unwrap(),
            ParsedKeypress {
                key: "ArrowLeft".into(),
                meta_key: true,
                ctrl_key: false,
                alt_key: false,
                shift_key: false,
            }
        );
        assert_eq!(
            parse_keypress("Control+Shift+ArrowRight").unwrap(),
            ParsedKeypress {
                key: "ArrowRight".into(),
                meta_key: false,
                ctrl_key: true,
                alt_key: false,
                shift_key: true,
            }
        );
        assert_eq!(
            parse_keypress("Ctrl+Shift+ArrowRight").unwrap(),
            ParsedKeypress {
                key: "ArrowRight".into(),
                meta_key: false,
                ctrl_key: true,
                alt_key: false,
                shift_key: true,
            }
        );
        assert_eq!(
            parse_keypress("Alt+Backspace").unwrap(),
            ParsedKeypress {
                key: "Backspace".into(),
                meta_key: false,
                ctrl_key: false,
                alt_key: true,
                shift_key: false,
            }
        );
        assert_eq!(
            parse_keypress("Meta+Delete").unwrap(),
            ParsedKeypress {
                key: "Delete".into(),
                meta_key: true,
                ctrl_key: false,
                alt_key: false,
                shift_key: false,
            }
        );
        assert_eq!(
            parse_keypress("Shift+Home").unwrap(),
            ParsedKeypress {
                key: "Home".into(),
                meta_key: false,
                ctrl_key: false,
                alt_key: false,
                shift_key: true,
            }
        );
        assert_eq!(
            parse_keypress("End").unwrap(),
            ParsedKeypress {
                key: "End".into(),
                meta_key: false,
                ctrl_key: false,
                alt_key: false,
                shift_key: false,
            }
        );
        assert_eq!(
            parse_keypress("PageUp").unwrap(),
            ParsedKeypress {
                key: "PageUp".into(),
                meta_key: false,
                ctrl_key: false,
                alt_key: false,
                shift_key: false,
            }
        );
        assert_eq!(
            parse_keypress("PageDown").unwrap(),
            ParsedKeypress {
                key: "PageDown".into(),
                meta_key: false,
                ctrl_key: false,
                alt_key: false,
                shift_key: false,
            }
        );
        assert_eq!(
            parse_keypress("Enter").unwrap(),
            ParsedKeypress {
                key: "Enter".into(),
                meta_key: false,
                ctrl_key: false,
                alt_key: false,
                shift_key: false,
            }
        );
        assert_eq!(
            parse_keypress("Escape").unwrap(),
            ParsedKeypress {
                key: "Escape".into(),
                meta_key: false,
                ctrl_key: false,
                alt_key: false,
                shift_key: false,
            }
        );
        assert_eq!(
            parse_keypress("Tab").unwrap(),
            ParsedKeypress {
                key: "Tab".into(),
                meta_key: false,
                ctrl_key: false,
                alt_key: false,
                shift_key: false,
            }
        );
        assert_eq!(
            parse_keypress("ArrowUp").unwrap(),
            ParsedKeypress {
                key: "ArrowUp".into(),
                meta_key: false,
                ctrl_key: false,
                alt_key: false,
                shift_key: false,
            }
        );
        assert_eq!(
            parse_keypress("ArrowDown").unwrap(),
            ParsedKeypress {
                key: "ArrowDown".into(),
                meta_key: false,
                ctrl_key: false,
                alt_key: false,
                shift_key: false,
            }
        );
        assert_eq!(
            parse_keypress("a").unwrap(),
            ParsedKeypress {
                key: "a".into(),
                meta_key: false,
                ctrl_key: false,
                alt_key: false,
                shift_key: false,
            }
        );
        assert_eq!(
            parse_keypress("ñ").unwrap(),
            ParsedKeypress {
                key: "ñ".into(),
                meta_key: false,
                ctrl_key: false,
                alt_key: false,
                shift_key: false,
            }
        );
        assert_eq!(
            parse_keypress("+").unwrap(),
            ParsedKeypress {
                key: "+".into(),
                meta_key: false,
                ctrl_key: false,
                alt_key: false,
                shift_key: false,
            }
        );
        assert_eq!(
            parse_keypress("Ctrl++").unwrap(),
            ParsedKeypress {
                key: "+".into(),
                meta_key: false,
                ctrl_key: true,
                alt_key: false,
                shift_key: false,
            }
        );
        assert_eq!(
            parse_keypress("Meta+Alt+Control+Shift+Tab").unwrap(),
            ParsedKeypress {
                key: "Tab".into(),
                meta_key: true,
                ctrl_key: true,
                alt_key: true,
                shift_key: true,
            }
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_keypress_spec_preserves_navigation_editing_and_modifiers() {
        let meta_left = macos_keypress_spec(&parse_keypress("Meta+ArrowLeft").unwrap())
            .expect("map Meta+ArrowLeft");
        assert_eq!(meta_left.characters, '\u{f702}'.to_string());
        assert_eq!(meta_left.key_code, 123);
        assert!(meta_left.meta_key);

        let ctrl_shift_right =
            macos_keypress_spec(&parse_keypress("Ctrl+Shift+ArrowRight").unwrap())
                .expect("map Ctrl+Shift+ArrowRight");
        assert_eq!(ctrl_shift_right.characters, '\u{f703}'.to_string());
        assert_eq!(ctrl_shift_right.key_code, 124);
        assert!(ctrl_shift_right.ctrl_key);
        assert!(ctrl_shift_right.shift_key);

        let alt_backspace = macos_keypress_spec(&parse_keypress("Alt+Backspace").unwrap())
            .expect("map Alt+Backspace");
        assert_eq!(alt_backspace.characters, '\u{8}'.to_string());
        assert_eq!(alt_backspace.key_code, 51);
        assert!(alt_backspace.alt_key);

        let meta_delete =
            macos_keypress_spec(&parse_keypress("Meta+Delete").unwrap()).expect("map Meta+Delete");
        assert_eq!(meta_delete.characters, '\u{f728}'.to_string());
        assert_eq!(meta_delete.key_code, 117);
        assert!(meta_delete.meta_key);

        assert!(macos_keypress_spec(&parse_keypress("ñ").unwrap()).is_none());
    }

    #[test]
    fn keypress_script_encodes_modifiers_and_events() {
        let cases = [
            (
                "Meta+ArrowLeft",
                "\"ArrowLeft\"",
                "metaKey: true",
                "ctrlKey: false",
                "altKey: false",
                "shiftKey: false",
            ),
            (
                "Control+Shift+ArrowRight",
                "\"ArrowRight\"",
                "metaKey: false",
                "ctrlKey: true",
                "altKey: false",
                "shiftKey: true",
            ),
            (
                "Ctrl+Shift+ArrowRight",
                "\"ArrowRight\"",
                "metaKey: false",
                "ctrlKey: true",
                "altKey: false",
                "shiftKey: true",
            ),
            (
                "Alt+Backspace",
                "\"Backspace\"",
                "metaKey: false",
                "ctrlKey: false",
                "altKey: true",
                "shiftKey: false",
            ),
            (
                "Meta+Delete",
                "\"Delete\"",
                "metaKey: true",
                "ctrlKey: false",
                "altKey: false",
                "shiftKey: false",
            ),
            (
                "a",
                "\"a\"",
                "metaKey: false",
                "ctrlKey: false",
                "altKey: false",
                "shiftKey: false",
            ),
            (
                "ñ",
                "\"ñ\"",
                "metaKey: false",
                "ctrlKey: false",
                "altKey: false",
                "shiftKey: false",
            ),
        ];

        for (input, expected_key, expected_meta, expected_ctrl, expected_alt, expected_shift) in
            cases
        {
            let script = automation_script(
                &BrowserAutomationAction::Keypress {
                    key: input.to_string(),
                },
                None,
            )
            .unwrap_or_else(|err| panic!("failed to build script for '{input}': {err:?}"));

            assert!(
                script.contains(&format!("key: {expected_key}")),
                "script for '{input}' should contain key: {expected_key}, got: {script}"
            );
            assert!(
                script.contains(expected_meta),
                "script for '{input}' should contain {expected_meta}, got: {script}"
            );
            assert!(
                script.contains(expected_ctrl),
                "script for '{input}' should contain {expected_ctrl}, got: {script}"
            );
            assert!(
                script.contains(expected_alt),
                "script for '{input}' should contain {expected_alt}, got: {script}"
            );
            assert!(
                script.contains(expected_shift),
                "script for '{input}' should contain {expected_shift}, got: {script}"
            );
            assert!(
                script.contains("bubbles: true"),
                "script for '{input}' should contain bubbles: true"
            );
            assert!(
                script.contains("cancelable: true"),
                "script for '{input}' should contain cancelable: true"
            );
            assert!(
                script.contains("'keydown'"),
                "script for '{input}' should dispatch keydown"
            );
            assert!(
                script.contains("'keyup'"),
                "script for '{input}' should dispatch keyup"
            );
            assert!(
                script.contains("throw new Error(") || script.contains("throw new Error"),
                "script for '{input}' should throw error if keydown was prevented"
            );
        }
    }

    #[test]
    fn keypress_script_rejects_invalid_inputs() {
        let invalid_cases = [
            "",
            "Meta",
            "Ctrl",
            "Control",
            "Alt",
            "Shift",
            "Ctrl+Shift",
            "Meta+Alt",
            "ArrowLeft+ArrowRight",
            "a+b",
            "Ctrl+a+b",
            "Enter+Tab",
            "Ctrl+",
            "+a",
            "++",
            "Ctrl+++",
            "Command+ArrowLeft",
            "Super+a",
            "F1",
            "Ctrl+Ctrl+a",
        ];

        for input in invalid_cases {
            let res = automation_script(
                &BrowserAutomationAction::Keypress {
                    key: input.to_string(),
                },
                None,
            );
            assert!(
                res.is_err(),
                "keypress input '{input}' should fail validation, but succeeded with: {:?}",
                res.ok()
            );
        }
    }
}

async fn eval_webview<R: tauri::Runtime>(
    webview: tauri::Webview<R>,
    script: String,
) -> Result<String, BrowserError> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let sender = Arc::new(std::sync::Mutex::new(Some(sender)));
    webview
        .eval_with_callback(script, move |result| {
            if let Some(sender) = sender.lock().ok().and_then(|mut slot| slot.take()) {
                let _ = sender.send(result);
            }
        })
        .map_err(|error| BrowserError::AutomationFailed(error.to_string()))?;
    tokio::time::timeout(std::time::Duration::from_secs(5), receiver)
        .await
        .map_err(|_| BrowserError::AutomationFailed("webview evaluation timed out".into()))?
        .map_err(|_| BrowserError::AutomationFailed("webview evaluation was cancelled".into()))
}

pub const BROWSER_STATE_CHANGED_EVENT: &str = "browser_state_changed";

fn emit_browser_state<R: tauri::Runtime>(webview: &tauri::Webview<R>, state: &BrowserState) {
    let _ = webview.app_handle().emit(
        BROWSER_STATE_CHANGED_EVENT,
        BrowserStateChangedPayload::from(state),
    );
}

fn update_webview_state<R: tauri::Runtime>(
    webview: &tauri::Webview<R>,
    manager: Arc<BrowserManager>,
    browser_id: String,
    url: Option<String>,
    title: Option<String>,
    loading: Option<bool>,
    error: Option<String>,
) {
    if let Ok(state) =
        manager.update_navigation_state(&browser_id, url, title, loading, None, None, error)
    {
        emit_browser_state(webview, &state);
    }

    #[cfg(target_os = "macos")]
    {
        let manager = Arc::clone(&manager);
        let browser_id = browser_id.clone();
        let webview_for_emit = webview.clone();
        let _ = webview.with_webview(move |platform_webview| unsafe {
            let native: &objc2_web_kit::WKWebView = &*platform_webview.inner().cast();
            if let Ok(state) = manager.update_navigation_state(
                &browser_id,
                None,
                None,
                None,
                Some(native.canGoBack()),
                Some(native.canGoForward()),
                None,
            ) {
                emit_browser_state(&webview_for_emit, &state);
            }
        });
    }
}

#[tauri::command]
pub async fn cmd_browser_create<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    request: CreateBrowserRequest,
) -> Result<BrowserState, IpcError> {
    if let Some(restored_browser_id) = request.browser_id.as_deref() {
        if let Ok(existing) = manager.get_state(restored_browser_id) {
            return Ok(existing);
        }
    }

    #[cfg(target_os = "macos")]
    if request
        .profile
        .as_ref()
        .is_some_and(BrowserProfileId::is_named)
    {
        return Err(BrowserError::UnsupportedProfile(
            "named persistent browser profiles are unavailable on macOS WebKit".into(),
        )
        .into());
    }
    let state = manager.register_session(request.clone())?;

    #[cfg(not(target_os = "macos"))]
    let profile_data_dir = match request.profile.as_ref() {
        Some(BrowserProfileId::Named(profile_id)) => {
            let root = app
                .path()
                .app_data_dir()
                .map_err(|error| BrowserError::CreateFailed(error.to_string()))?;
            let data_dir = root.join("browser-profiles").join(profile_id);
            tokio::fs::create_dir_all(&data_dir)
                .await
                .map_err(|error| BrowserError::CreateFailed(error.to_string()))?;
            Some(data_dir)
        }
        _ => None,
    };

    if let Some(main_window) = app.get_window("main") {
        let label = state.webview_label.clone();
        let target_url = state.url.clone();
        let bounds = request.bounds.clone();
        let visible = state.visible;
        let browser_id = state.browser_id.clone();
        let incognito = state.profile_id.is_private();
        let profile_id = state.profile_id.clone();
        let worktree_path = state.worktree_path.clone();
        let zoom_factor = state.zoom_factor;
        let bridge_app = app.clone();
        let bridge_browser_id = browser_id.clone();
        let bridge_profile_id = profile_id.clone();
        let bridge_worktree_path = worktree_path.clone();
        let page_manager = Arc::clone(manager.inner());
        let title_manager = Arc::clone(manager.inner());
        let creation_manager = Arc::clone(manager.inner());
        let page_browser_id = browser_id.clone();
        let title_browser_id = browser_id.clone();

        let window_clone = main_window.clone();
        let _ = main_window.run_on_main_thread(move || {
            let parsed_url: tauri::WebviewUrl = if let Ok(u) = target_url.parse() {
                tauri::WebviewUrl::External(u)
            } else {
                tauri::WebviewUrl::App("about:blank".into())
            };

            let builder = tauri::WebviewBuilder::new(label, parsed_url)
                .user_agent(crate::browser::default_desktop_user_agent())
                .incognito(incognito)
                .initialization_script(BROWSER_GUEST_BRIDGE_SCRIPT)
                .on_navigation(move |target| match parse_browser_guest_action(target) {
                    Some(BrowserGuestAction::Open(target_url)) => {
                        let _ = bridge_app.emit(
                            BROWSER_OPEN_REQUESTED_EVENT,
                            BrowserOpenRequestedPayload {
                                browser_id: bridge_browser_id.clone(),
                                target_url,
                                profile_id: bridge_profile_id.clone(),
                                worktree_path: bridge_worktree_path.clone(),
                            },
                        );
                        false
                    }
                    Some(BrowserGuestAction::Download(target_url)) => {
                        let _ = bridge_app.emit(
                            BROWSER_DOWNLOAD_REQUESTED_EVENT,
                            BrowserDownloadRequestedPayload {
                                browser_id: bridge_browser_id.clone(),
                                target_url,
                            },
                        );
                        false
                    }
                    Some(BrowserGuestAction::Shortcut(action)) => {
                        let _ = bridge_app.emit(
                            BROWSER_SHORTCUT_REQUESTED_EVENT,
                            BrowserShortcutRequestedPayload {
                                browser_id: bridge_browser_id.clone(),
                                action,
                            },
                        );
                        false
                    }
                    None => true,
                })
                .on_page_load(move |webview, payload| {
                    let loading = matches!(payload.event(), PageLoadEvent::Started);
                    let page_url = payload.url().to_string();
                    let current_state = page_manager.get_state(&page_browser_id).ok();
                    let fallback_to_blank = page_url == "about:blank"
                        && current_state
                            .as_ref()
                            .is_some_and(|state| state.url != "about:blank");
                    let error = (!loading && fallback_to_blank).then(|| {
                        format!(
                            "Failed to load {}",
                            current_state
                                .as_ref()
                                .map_or("page", |state| state.url.as_str())
                        )
                    });
                    let navigation_url = (!fallback_to_blank).then_some(page_url);
                    update_webview_state(
                        &webview,
                        Arc::clone(&page_manager),
                        page_browser_id.clone(),
                        navigation_url,
                        None,
                        Some(loading),
                        error,
                    );
                })
                .on_document_title_changed(move |webview, title| {
                    update_webview_state(
                        &webview,
                        Arc::clone(&title_manager),
                        title_browser_id.clone(),
                        None,
                        Some(title),
                        None,
                        None,
                    );
                });

            #[cfg(not(target_os = "macos"))]
            let builder = if let Some(data_dir) = profile_data_dir {
                builder.data_directory(data_dir)
            } else {
                builder
            };

            let pos = if let Some(ref b) = bounds {
                tauri::LogicalPosition { x: b.x, y: b.y }
            } else {
                tauri::LogicalPosition { x: 0.0, y: 0.0 }
            };

            let size = if let Some(ref b) = bounds {
                tauri::LogicalSize {
                    width: b.width,
                    height: b.height,
                }
            } else {
                tauri::LogicalSize {
                    width: 800.0,
                    height: 600.0,
                }
            };

            if let Ok(child) = window_clone.add_child(
                builder,
                tauri::LogicalPosition { x: pos.x, y: pos.y },
                tauri::LogicalSize {
                    width: size.width,
                    height: size.height,
                },
            ) {
                let _ = child.set_zoom(zoom_factor);
                if let Ok(Some(current_bounds)) = creation_manager.get_bounds(&browser_id) {
                    let _ = child.set_bounds(tauri::Rect {
                        position: tauri::Position::Logical(tauri::LogicalPosition {
                            x: current_bounds.x,
                            y: current_bounds.y,
                        }),
                        size: tauri::Size::Logical(tauri::LogicalSize {
                            width: current_bounds.width,
                            height: current_bounds.height,
                        }),
                    });
                }
                let is_visible = creation_manager.is_visible(&browser_id).unwrap_or(visible);
                if !is_visible {
                    let _ = child.hide();
                } else {
                    let _ = child.show();
                }
            }
        });
    }

    Ok(state)
}

#[tauri::command]
pub async fn cmd_browser_navigate<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
    url: String,
) -> Result<(), IpcError> {
    let valid_url = manager.update_url(&browser_id, &url)?;
    let state = manager.get_state(&browser_id)?;

    if let Some(webview) = app.get_webview(&state.webview_label) {
        emit_browser_state(&webview, &state);
        let parsed = valid_url.parse().map_err(|error| {
            BrowserError::NavigationFailed(format!("invalid target URL: {error}"))
        })?;
        if let Err(error) = webview.navigate(parsed) {
            let message = error.to_string();
            if let Ok(error_state) = manager.update_navigation_state(
                &browser_id,
                None,
                None,
                Some(false),
                None,
                None,
                Some(message.clone()),
            ) {
                emit_browser_state(&webview, &error_state);
            }
            return Err(BrowserError::NavigationFailed(message).into());
        }
    }
    Ok(())
}

fn history_navigation<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &Arc<BrowserManager>,
    browser_id: &str,
    forward: bool,
) -> Result<(), IpcError> {
    let state = manager.begin_history_navigation(browser_id, forward)?;
    let webview = app
        .get_webview(&state.webview_label)
        .ok_or_else(|| BrowserError::NotFound(browser_id.to_string()))?;
    emit_browser_state(&webview, &state);

    if !state.loading {
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    {
        let manager = Arc::clone(manager);
        let browser_id = browser_id.to_string();
        let webview_for_emit = webview.clone();
        webview
            .with_webview(move |platform_webview| unsafe {
                let native: &objc2_web_kit::WKWebView = &*platform_webview.inner().cast();
                let can_navigate = if forward {
                    native.canGoForward()
                } else {
                    native.canGoBack()
                };

                if can_navigate {
                    if forward {
                        let _ = native.goForward();
                    } else {
                        let _ = native.goBack();
                    }
                }

                let next_state = if can_navigate {
                    manager.update_navigation_state(
                        &browser_id,
                        None,
                        None,
                        Some(true),
                        Some(native.canGoBack()),
                        Some(native.canGoForward()),
                        None,
                    )
                } else {
                    manager.cancel_history_navigation(&browser_id, forward)
                };
                if let Ok(next_state) = next_state {
                    emit_browser_state(&webview_for_emit, &next_state);
                }
            })
            .map_err(|error| BrowserError::HistoryFailed(error.to_string()))?;
        return Ok(());
    }

    #[cfg(not(target_os = "macos"))]
    {
        let script = if forward {
            "history.forward()"
        } else {
            "history.back()"
        };
        if let Err(error) = webview.eval(script) {
            if let Ok(restored) = manager.cancel_history_navigation(browser_id, forward) {
                emit_browser_state(&webview, &restored);
            }
            return Err(BrowserError::HistoryFailed(error.to_string()).into());
        }
        Ok(())
    }
}

#[tauri::command]
pub async fn cmd_browser_go_back<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
) -> Result<(), IpcError> {
    history_navigation(&app, manager.inner(), &browser_id, false)
}

#[tauri::command]
pub async fn cmd_browser_go_forward<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
) -> Result<(), IpcError> {
    history_navigation(&app, manager.inner(), &browser_id, true)
}

#[tauri::command]
pub async fn cmd_browser_import_cookies<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    request: ImportBrowserCookiesRequest,
) -> Result<ImportBrowserCookiesResult, IpcError> {
    let profile_id = BrowserProfileId::from_id(&request.profile_id)
        .ok_or_else(|| BrowserError::UnsupportedProfile(request.profile_id.clone()))?;
    let source = tokio::fs::read_to_string(&request.file_path)
        .await
        .map_err(|error| {
            BrowserError::CookieImport(format!("failed to read cookie file: {error}"))
        })?;
    let cookies = parse_cookie_file(&source)?
        .into_iter()
        .map(cookie_from_imported)
        .collect::<Result<Vec<_>, _>>()?;

    let mut targets = manager
        .webview_labels_for_profile(&profile_id)
        .into_iter()
        .filter_map(|label| app.get_webview(&label))
        .collect::<Vec<_>>();

    if targets.is_empty() && matches!(profile_id, BrowserProfileId::Default) {
        if let Some(main_webview) = app.get_webview("main") {
            targets.push(main_webview);
        }
    }

    if targets.is_empty() {
        return Err(BrowserError::CookieImport(format!(
            "open a browser tab using the {} profile before importing cookies",
            profile_id.as_str()
        ))
        .into());
    }

    for target in targets {
        for cookie in &cookies {
            target
                .set_cookie(cookie.clone())
                .map_err(|error| BrowserError::CookieImport(error.to_string()))?;
        }
    }

    Ok(ImportBrowserCookiesResult {
        imported_count: cookies.len(),
    })
}

#[tauri::command]
pub async fn cmd_browser_reload<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
) -> Result<(), IpcError> {
    let state = manager.begin_reload(&browser_id)?;
    if let Some(webview) = app.get_webview(&state.webview_label) {
        emit_browser_state(&webview, &state);
        if let Err(error) = webview.reload() {
            let message = error.to_string();
            if let Ok(error_state) = manager.update_navigation_state(
                &browser_id,
                None,
                None,
                Some(false),
                None,
                None,
                Some(message.clone()),
            ) {
                emit_browser_state(&webview, &error_state);
            }
            return Err(BrowserError::NavigationFailed(message).into());
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn cmd_browser_set_bounds<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
    bounds: LogicalRect,
) -> Result<(), IpcError> {
    manager.set_bounds(&browser_id, bounds.clone())?;
    let state = manager.get_state(&browser_id)?;
    let webview = app
        .get_webview(&state.webview_label)
        .ok_or_else(|| BrowserError::WebviewNotFound(state.webview_label.clone()))?;
    let _ = webview.set_bounds(tauri::Rect {
        position: tauri::Position::Logical(tauri::LogicalPosition {
            x: bounds.x,
            y: bounds.y,
        }),
        size: tauri::Size::Logical(tauri::LogicalSize {
            width: bounds.width,
            height: bounds.height,
        }),
    });
    Ok(())
}

#[tauri::command]
pub async fn cmd_browser_set_visible<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
    visible: bool,
) -> Result<(), IpcError> {
    manager.set_visible(&browser_id, visible)?;
    let state = manager.get_state(&browser_id)?;
    if let Some(webview) = app.get_webview(&state.webview_label) {
        if visible {
            let _ = webview.show();
        } else {
            let _ = webview.hide();
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn cmd_browser_set_zoom<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
    zoom_factor: f64,
) -> Result<f64, IpcError> {
    let clamped = manager.set_zoom(&browser_id, zoom_factor)?;
    let state = manager.get_state(&browser_id)?;
    if let Some(webview) = app.get_webview(&state.webview_label) {
        let _ = webview.set_zoom(clamped);
    }
    Ok(clamped)
}

#[tauri::command]
pub async fn cmd_browser_focus<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
) -> Result<(), IpcError> {
    let state = manager.get_state(&browser_id)?;
    if let Some(webview) = app.get_webview(&state.webview_label) {
        let _ = webview.set_focus();
    }
    Ok(())
}

#[tauri::command]
pub async fn cmd_browser_get_state(
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
) -> Result<BrowserState, IpcError> {
    let state = manager.get_state(&browser_id)?;
    Ok(state)
}

#[tauri::command]
pub async fn cmd_browser_find<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
    query: String,
    backwards: bool,
) -> Result<BrowserFindResult, IpcError> {
    let state = manager.get_state(&browser_id)?;
    let webview = app
        .get_webview(&state.webview_label)
        .ok_or_else(|| BrowserError::WebviewNotFound(state.webview_label.clone()))?;
    if query.trim().is_empty() {
        return Ok(BrowserFindResult {
            match_count: 0,
            found: false,
        });
    }
    let script = browser_find_script(&query, backwards)?;
    let result = eval_webview(webview, script)
        .await
        .map_err(|error| BrowserError::FindFailed(error.to_string()))?;
    Ok(parse_browser_find_callback(&result)?)
}

#[tauri::command]
pub async fn cmd_browser_clear_find<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
) -> Result<(), IpcError> {
    let state = manager.get_state(&browser_id)?;
    let webview = app
        .get_webview(&state.webview_label)
        .ok_or_else(|| BrowserError::WebviewNotFound(state.webview_label.clone()))?;
    let _ = eval_webview(webview, BROWSER_CLEAR_FIND_SCRIPT.to_string())
        .await
        .map_err(|error| BrowserError::FindFailed(error.to_string()))?;
    Ok(())
}

#[tauri::command]
pub async fn cmd_browser_download(url: String, file_path: String) -> Result<(), IpcError> {
    download_url_to_path(&url, std::path::Path::new(&file_path)).await?;
    Ok(())
}
#[tauri::command]
pub async fn cmd_browser_automation_snapshot<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
) -> Result<BrowserAutomationSnapshot, IpcError> {
    browser_automation_snapshot(app, manager.inner(), browser_id).await
}

pub async fn browser_automation_snapshot<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: &Arc<BrowserManager>,
    browser_id: String,
) -> Result<BrowserAutomationSnapshot, IpcError> {
    let state = manager.get_state(&browser_id)?;
    let webview = app
        .get_webview(&state.webview_label)
        .ok_or_else(|| BrowserError::WebviewNotFound(state.webview_label.clone()))?;
    let result = eval_webview(webview, AUTOMATION_SNAPSHOT_SCRIPT.to_string()).await?;
    let snapshot_json: String = serde_json::from_str(&result).map_err(|error| {
        BrowserError::AutomationFailed(format!("invalid snapshot callback result: {error}"))
    })?;
    let snapshot: AutomationSnapshotResult =
        serde_json::from_str(&snapshot_json).map_err(|error| {
            BrowserError::AutomationFailed(format!("invalid snapshot response: {error}"))
        })?;
    let targets = snapshot
        .elements
        .iter()
        .map(|element| BrowserAutomationTarget {
            reference: element.reference.clone(),
            selector: element.selector.clone(),
        })
        .collect();
    manager.record_automation_targets(&browser_id, state.generation, targets)?;

    Ok(BrowserAutomationSnapshot {
        browser_id,
        generation: state.generation,
        url: snapshot.url,
        title: snapshot.title,
        elements: snapshot
            .elements
            .into_iter()
            .map(|element| BrowserAutomationElement {
                reference: element.reference,
                role: element.role,
                name: element.name,
                tag_name: element.tag_name,
            })
            .collect(),
    })
}

#[tauri::command]
pub async fn cmd_browser_automation_act<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    request: BrowserAutomationRequest,
) -> Result<(), IpcError> {
    browser_automation_act(app, manager.inner(), request).await
}

pub async fn browser_automation_act<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: &Arc<BrowserManager>,
    request: BrowserAutomationRequest,
) -> Result<(), IpcError> {
    let selector = match &request.action {
        BrowserAutomationAction::Click { reference }
        | BrowserAutomationAction::Fill { reference, .. } => {
            Some(manager.automation_target(&request.browser_id, request.generation, reference)?)
        }
        BrowserAutomationAction::Keypress { .. } => {
            manager.assert_automation_generation(&request.browser_id, request.generation)?;
            None
        }
    };
    let state = manager.get_state(&request.browser_id)?;
    let webview = app
        .get_webview(&state.webview_label)
        .ok_or_else(|| BrowserError::WebviewNotFound(state.webview_label.clone()))?;

    #[cfg(target_os = "macos")]
    if let BrowserAutomationAction::Keypress { key } = &request.action {
        let keypress = parse_keypress(key)?;
        if dispatch_macos_keypress(&webview, &keypress)? {
            return Ok(());
        }
    }

    let script = automation_script(&request.action, selector.as_deref())?;
    let _ = eval_webview(webview, script).await?;
    Ok(())
}

#[tauri::command]
pub async fn cmd_browser_close<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
) -> Result<(), IpcError> {
    if let Some(session) = manager.remove_session(&browser_id) {
        if let Some(webview) = app.get_webview(&session.webview_label) {
            let _ = webview.close();
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn cmd_browser_list(
    manager: State<'_, Arc<BrowserManager>>,
) -> Result<Vec<BrowserSessionSummary>, IpcError> {
    Ok(manager.list_sessions())
}

#[tauri::command]
pub async fn cmd_browser_open_external(url: String) -> Result<(), IpcError> {
    let valid_url = crate::browser::validate_url(&url)?;
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(&valid_url).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open")
            .arg(&valid_url)
            .spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = crate::util::no_window_command("cmd")
            .args(["/C", "start", &valid_url])
            .spawn();
    }
    Ok(())
}
