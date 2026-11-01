use crate::browser::{
    browser_find_script, browser_guest_bridge_script, cookie_from_imported, download_url_to_path,
    parse_browser_find_callback, parse_browser_guest_action, parse_cookie_file,
    BrowserAutomationAction, BrowserAutomationRequest,
    BrowserAutomationSnapshot, BrowserAutomationTarget, BrowserDialogEntry,
    BrowserDialogHandleRequest, BrowserDialogPolicyRequest, BrowserDownloadRequest,
    BrowserDownloadRequestedPayload, BrowserElementPickedPayload, BrowserError, BrowserFindResult,
    BrowserGuestAction, BrowserLinkClickedPayload, BrowserManager, BrowserOpenRequestedPayload,
    BrowserProfileId, BrowserSessionSummary, BrowserShortcutRequestedPayload, BrowserState,
    BrowserStateChangedPayload, CreateBrowserRequest, ImportBrowserCookiesRequest,
    ImportBrowserCookiesResult, ImportInstalledBrowserCookiesRequest,
    ImportInstalledBrowserCookiesResult, LogicalRect, SnapshotOptions, BROWSER_CLEAR_FIND_SCRIPT,
    BROWSER_DOWNLOAD_REQUESTED_EVENT, BROWSER_ELEMENT_PICKED_EVENT, BROWSER_LINK_CLICKED_EVENT,
    BROWSER_OPEN_REQUESTED_EVENT, BROWSER_SHORTCUT_REQUESTED_EVENT,
};
use crate::ipc::error::{IpcError, IpcErrorCode};
use base64::Engine;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AutomationSnapshotResult {
    pub(crate) url: String,
    pub(crate) title: String,
    pub(crate) elements: Vec<AutomationSnapshotElement>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AutomationSnapshotElement {
    pub(crate) reference: String,
    pub(crate) selector: String,
    pub(crate) role: String,
    pub(crate) name: String,
    pub(crate) tag_name: String,
}

pub(crate) const AUTOMATION_SNAPSHOT_SCRIPT: &str = r#"(() => {
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

    let (prefix, has_trailing_plus) = if let Some(prefix) = raw.strip_suffix("++") {
        (prefix, true)
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
async fn dispatch_macos_keypress<R: tauri::Runtime>(
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

    // `with_webview` runs the closure inline only on the main thread; from a tokio worker it
    // is queued to the event loop, so the outcome must be awaited rather than read at once.
    let (sender, receiver) = tokio::sync::oneshot::channel::<Result<(), BrowserError>>();
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
            let _ = sender.send(outcome);
        })
        .map_err(|error| BrowserError::AutomationFailed(error.to_string()))?;
    tokio::time::timeout(std::time::Duration::from_secs(5), receiver)
        .await
        .map_err(|_| BrowserError::AutomationFailed("native key dispatch timed out".into()))?
        .map_err(|_| BrowserError::AutomationFailed("native key dispatch did not execute".into()))??;

    Ok(true)
}

/// The page script for an action that addresses no element (keypress, window scroll, typing
/// into the focused element). Element actions go through `observe.js` by ref instead.
fn automation_script(action: &BrowserAutomationAction) -> Result<String, BrowserError> {
    match action {
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
        BrowserAutomationAction::Scroll { reference: None, x, y } => {
            let dx = x.unwrap_or(0);
            let dy = y.unwrap_or(0);
            Ok(format!(
                "(() => {{ window.scrollBy({{ left: {dx}, top: {dy}, behavior: 'instant' }}); return 'ok'; }})()"
            ))
        }
        BrowserAutomationAction::Type { reference: None, text } => {
            let text_json = serde_json::to_string(text)
                .map_err(|error| BrowserError::AutomationFailed(error.to_string()))?;
            Ok(format!(
                "(() => {{ const element = document.activeElement || document.body; if (!element) throw new Error('element disappeared'); element.focus(); const str = {text_json}; for (const ch of str) {{ const init = {{ bubbles: true, cancelable: true, key: ch }}; element.dispatchEvent(new KeyboardEvent('keydown', init)); element.dispatchEvent(new KeyboardEvent('keypress', init)); if (element.value !== undefined) {{ element.value = (element.value || '') + ch; element.dispatchEvent(new Event('input', {{ bubbles: true }})); }} else if (element.isContentEditable) {{ document.execCommand('insertText', false, ch); }} element.dispatchEvent(new KeyboardEvent('keyup', init)); }} if (element.value !== undefined) {{ element.dispatchEvent(new Event('change', {{ bubbles: true }})); }} return 'ok'; }})()"
            ))
        }
        _ => Err(BrowserError::AutomationFailed("element actions are addressed by ref".into())),
    }
}

#[cfg(test)]
mod automation_tests {
    use super::*;

    #[test]
    fn untargeted_actions_encode_text_as_json_and_element_actions_need_a_ref() {
        let typed = automation_script(&BrowserAutomationAction::Type {
            reference: None,
            text: "hello'); window.bad = true; //".into(),
        })
        .expect("type into the focused element");
        assert!(typed.contains("const str = \"hello'); window.bad = true; //\""));
        let scroll = automation_script(&BrowserAutomationAction::Scroll { reference: None, x: Some(0), y: Some(200) })
            .expect("window scroll");
        assert!(scroll.contains("window.scrollBy({ left: 0, top: 200"));
        let error = automation_script(&BrowserAutomationAction::Click { reference: "d1.1".into() })
            .expect_err("element actions go through observe.js");
        assert_eq!(error, BrowserError::AutomationFailed("element actions are addressed by ref".into()));
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
            let script = automation_script(&BrowserAutomationAction::Keypress {
                key: input.to_string(),
            })
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
            let res = automation_script(&BrowserAutomationAction::Keypress {
                key: input.to_string(),
            });
            assert!(
                res.is_err(),
                "keypress input '{input}' should fail validation, but succeeded with: {:?}",
                res.ok()
            );
        }
    }
}

pub(crate) async fn eval_webview<R: tauri::Runtime>(
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

/// Publishes state changes the manager makes outside a webview callback as
/// `browser_state_changed`. The trusted bridge kind is one: Windows upgrades it from a CDP
/// completion that has no webview in scope, and the toolbar's pick capability depends on it.
pub fn install_browser_state_emitter<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
) {
    let app = app.clone();
    manager.set_state_listener(Arc::new(move |state: &BrowserState| {
        let _ = app.emit(
            BROWSER_STATE_CHANGED_EVENT,
            BrowserStateChangedPayload::from(state),
        );
    }));
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

/// Decides whether a freshly created browser child webview should be kept or
/// discarded. `session_exists` reflects whether the manager still holds the
/// browser session: when the async side of `cmd_browser_create` gives up
/// (timeout or dispatch failure) it removes the session and reports an error,
/// but the queued main-thread closure still runs afterwards — in that case it
/// must close the webview instead of leaving it shown and unmanageable.
fn keep_or_discard_fresh_webview(session_exists: bool) -> Result<(), String> {
    if session_exists {
        Ok(())
    } else {
        Err(
            "browser webview was created after its session was abandoned; closing the orphaned webview"
                .to_string(),
        )
    }
}

static CREATED_SESSION_IDS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Re-checks the session once the platform bridge is installed. The 5 s creation timeout
/// can remove the session after the orphan check passed but before the install finished;
/// then `discard` must close the webview and forget the bridge state installed for it.
fn discard_if_abandoned_after_install(
    session_exists: impl FnOnce() -> bool,
    discard: impl FnOnce(),
) -> Result<(), String> {
    if session_exists() {
        return Ok(());
    }
    discard();
    Err("browser session was abandoned while its trusted bridge was installed; closing the orphaned webview".to_string())
}

/// Passes `result` through and, when it is an error, drops the session that was
/// already registered so neither the manager nor CREATED_SESSION_IDS keeps it.
#[cfg(any(not(target_os = "macos"), test))]
fn discard_session_on_error<T, E>(
    manager: &BrowserManager,
    browser_id: &str,
    result: Result<T, E>,
) -> Result<T, E> {
    if result.is_err() {
        discard_failed_session(manager, browser_id);
    }
    result
}

/// Drops a session whose webview creation failed.
fn discard_failed_session(manager: &BrowserManager, browser_id: &str) {
    manager.remove_session(browser_id);
    CREATED_SESSION_IDS.lock().retain(|id| id != browser_id);
}

/// Drops a session whose webview creation failed once the main-thread creation step was
/// queued, which may already have installed the platform bridge for its webview label.
fn discard_failed_creation<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    browser_id: &str,
) {
    discard_failed_creation_with(manager, browser_id, |label| {
        crate::browser::bridge::forget_webview(app, label)
    });
}

fn discard_failed_creation_with(
    manager: &BrowserManager,
    browser_id: &str,
    forget_bridge: impl FnOnce(&str),
) {
    if let Ok(state) = manager.get_state(browser_id) {
        forget_bridge(&state.webview_label);
    }
    discard_failed_session(manager, browser_id);
}

/// Queues a main-thread close of the webview of a session whose main-thread creation
/// step did not report back in time, then drops the session.
///
/// Ordering: the creation closure and this close are both `run_on_main_thread` tasks
/// on the one event-loop proxy (`send_user_message(Message::Task)` in tauri-runtime-wry),
/// which runs tasks in FIFO order. The close is queued after the timeout fired, so after
/// the creation closure was queued, and runs only once that closure has finished:
/// - If the closure was still pending or running, it may have passed its re-check and
///   navigated, attached (Linux overlay) and shown the child; the close then removes the
///   child and its overlay entry.
/// - If the closure had already closed the child itself or never created it,
///   `get_webview` misses and `detach_child` finds nothing, so the close is a no-op.
///
/// The close is queued before the session is dropped. The Linux overlay entry is keyed
/// by `browser_id`, which a caller may supply, and a create reusing it is refused while
/// this session still exists. So any later create with the same id queues its own
/// creation closure after this close, and the close cannot detach that new session's
/// overlay entry.
/// Neither step holds a lock while the other runs: queueing takes no lock, and
/// `discard` takes and releases its own locks.
fn discard_timed_out_creation_with(
    webview_label: &str,
    browser_id: &str,
    discard: impl FnOnce(),
    queue_main_thread_close: impl FnOnce(String, String),
) {
    queue_main_thread_close(webview_label.to_string(), browser_id.to_string());
    discard();
}

fn discard_timed_out_creation<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    state: &BrowserState,
) {
    discard_timed_out_creation_with(
        &state.webview_label,
        &state.browser_id,
        || discard_failed_creation(app, manager, &state.browser_id),
        |webview_label, browser_id| {
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || {
                close_abandoned_webview(&handle, &webview_label, &browser_id);
            });
        },
    );
}

/// Main thread only. Closes a webview whose session is already gone and releases its
/// Linux overlay entry and Windows bridge world; each step is a no-op when nothing is
/// left. The Windows world can outlive `remove_session` when a stale creation closure
/// installs it after the session was dropped.
fn close_abandoned_webview<R: tauri::Runtime>(
    app: &AppHandle<R>,
    webview_label: &str,
    browser_id: &str,
) {
    #[cfg(target_os = "linux")]
    {
        let _ = crate::browser::linux::implementation::detach_child(browser_id);
    }
    #[cfg(not(target_os = "linux"))]
    let _ = browser_id;
    #[cfg(target_os = "windows")]
    crate::browser::bridge::windows::forget_webview(webview_label);
    if let Some(webview) = app.get_webview(webview_label) {
        let _ = webview.close();
    }
}

pub fn record_session_created(browser_id: &str) {
    let mut ids = CREATED_SESSION_IDS.lock();
    ids.retain(|id| id != browser_id);
    ids.push(browser_id.to_string());
}

pub fn identify_browser_session(manager: &BrowserManager) -> Option<BrowserSessionSummary> {
    let visible_sessions: Vec<BrowserSessionSummary> = manager
        .list_sessions()
        .into_iter()
        .filter(|s| s.visible)
        .collect();

    if visible_sessions.is_empty() {
        return None;
    }

    if visible_sessions.len() == 1 {
        return visible_sessions.into_iter().next();
    }

    let ids = CREATED_SESSION_IDS.lock();
    visible_sessions
        .into_iter()
        .max_by_key(|s| ids.iter().rposition(|id| id == &s.browser_id))
}

/// WebView2 runtime `101.0.1210.39` is the oldest one that honors `incognito`;
/// older runtimes ignore the flag and keep cookies and history for what the UI
/// calls a private profile.
#[cfg(any(target_os = "windows", test))]
const WEBVIEW2_PRIVATE_PROFILE_MINIMUM: (u32, u32, u32, u32) = (101, 0, 1210, 39);

/// Whether a WebView2 runtime version string predates the version that honors
/// `incognito`. Unparseable strings count as too old, so the caller warns
/// rather than staying silent.
#[cfg(any(target_os = "windows", test))]
fn webview2_version_is_below_private_profile_minimum(version: &str) -> bool {
    let mut parts = version
        .trim()
        .split('.')
        .map(|part| part.parse::<u32>().unwrap_or(0));
    let parsed = (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    );
    parsed < WEBVIEW2_PRIVATE_PROFILE_MINIMUM
}

pub async fn create_browser_session<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &Arc<BrowserManager>,
    request: CreateBrowserRequest,
) -> Result<BrowserState, IpcError> {
    create_browser_session_with_opener(app, manager, request, None).await
}

/// Creates a browser session, optionally as the popup of an existing tab.
///
/// `opener` carries the link the popup-open action reported: the tab that called
/// `window.open` and the opener page's own popup id. With it, the tab's bridge script
/// installs the opener proxy and the popup -> opener link is registered, which is what
/// lets the OAuth callback travel back through the host instead of a BroadcastChannel
/// that only ever reached the same origin and storage partition.
pub async fn create_browser_session_with_opener<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &Arc<BrowserManager>,
    request: CreateBrowserRequest,
    opener: Option<crate::browser::BrowserOpenerLink>,
) -> Result<BrowserState, IpcError> {
    if let Some(restored_browser_id) = request.browser_id.as_deref() {
        if let Ok(existing) = manager.get_state(restored_browser_id) {
            record_session_created(&existing.browser_id);
            if let Some(link) = opener.as_ref() {
                if !crate::browser::popup::link(&existing.browser_id, &link.browser_id, &link.handle) {
                    let _ = close_browser_session(app, manager, &existing.browser_id).await;
                    return Err(BrowserError::CreateFailed("popup closed while creation pending".into()).into());
                }
            }
            return Ok(existing);
        }
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    let cef_engine_dir = if opener.is_none() {
        engine_data_dir(app)
            .ok()
            .and_then(|data| crate::browser::cef_engine::engine_dir(&data))
    } else {
        None
    };
    // The Chromium engine keeps a named profile in its own directory; WebKit cannot.
    #[cfg(target_os = "macos")]
    if cef_engine_dir.is_none()
        && request
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
    record_session_created(&state.browser_id);

    #[cfg(not(target_os = "macos"))]
    let profile_data_dir = match request.profile.as_ref() {
        Some(BrowserProfileId::Named(profile_id)) => {
            let root = discard_session_on_error(
                manager,
                &state.browser_id,
                app.path()
                    .app_data_dir()
                    .map_err(|error| BrowserError::CreateFailed(error.to_string())),
            )?;
            let data_dir = root.join("browser-profiles").join(profile_id);
            discard_session_on_error(
                manager,
                &state.browser_id,
                tokio::fs::create_dir_all(&data_dir)
                    .await
                    .map_err(|error| BrowserError::CreateFailed(error.to_string())),
            )?;
            Some(data_dir)
        }
        _ => None,
    };

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    if let Some(engine_dir) = cef_engine_dir {
        return create_cef_browser_session(app, manager, state, engine_dir).await;
    }

    if let Some(main_window) = app.get_window("main") {
        let label = state.webview_label.clone();
        let target_url = state.url.clone();
        let bounds = request.bounds.clone();
        let visible = state.visible;
        let browser_id = state.browser_id.clone();
        let incognito = state.profile_id.is_private();
        let zoom_factor = state.zoom_factor;
        let bridge_app = app.clone();
        let bridge_browser_id = browser_id.clone();
        let page_manager = Arc::clone(manager);
        let title_manager = Arc::clone(manager);
        let creation_manager = Arc::clone(manager);
        let bridge_manager = Arc::clone(manager);
        let new_window_manager = Arc::clone(manager);
        let new_window_app = app.clone();
        let new_window_browser_id = browser_id.clone();
        let page_browser_id = browser_id.clone();
        let title_browser_id = browser_id.clone();

        let guest_bridge_nonce = uuid::Uuid::new_v4().to_string();
        let nonce = guest_bridge_nonce.clone();
        let is_popup = opener.is_some();
        let eval_bridge_script = browser_guest_bridge_script(&guest_bridge_nonce, is_popup);
        let page_load_bridge_script = eval_bridge_script.clone();
        // The guest starts at about:blank so the platform bridge install (which only takes
        // effect from the next document on WebKit) is in place before the first real page.
        // Until the target navigation starts, the bootstrap about:blank load events are
        // not a load failure of the target URL, so they are ignored.
        let bootstrapping_blank = Arc::new(std::sync::atomic::AtomicBool::new(
            target_url != "about:blank",
        ));
        let page_bootstrapping_blank = Arc::clone(&bootstrapping_blank);
        let install_ctx = crate::browser::bridge::BridgeInstallCtx {
            browser_id: browser_id.clone(),
            manager: Arc::clone(manager),
            sink: trusted_bridge_sink(app.clone(), Arc::clone(manager)),
        };
        let (creation_sender, creation_receiver) =
            tokio::sync::oneshot::channel::<Result<(), String>>();

        // Tauri documents that `incognito` needs the WebView2 runtime
        // 101.0.1210.39 or newer on Windows and silently does nothing on older
        // runtimes, so a "Private" tab would still keep cookies and history.
        // Warn instead of letting the profile lie; the profile selection itself
        // is unchanged.
        #[cfg(target_os = "windows")]
        if incognito {
            match tauri::webview_version() {
                Ok(version) => {
                    if webview2_version_is_below_private_profile_minimum(&version) {
                        tracing::warn!(
                            version = %version,
                            "WebView2 runtime is older than 101.0.1210.39; the private browser profile will keep cookies and history"
                        );
                    }
                }
                Err(error) => tracing::warn!(
                    %error,
                    "could not read the WebView2 runtime version; the private browser profile may keep cookies and history"
                ),
            }
        }

        let window_clone = main_window.clone();
        let run_result = main_window.run_on_main_thread(move || {
            let target_parsed: Option<tauri::Url> = target_url.parse().ok();
            let parsed_url: tauri::WebviewUrl = match "about:blank".parse() {
                Ok(blank) => tauri::WebviewUrl::External(blank),
                Err(_) => tauri::WebviewUrl::App("about:blank".into()),
            };

            let builder = tauri::WebviewBuilder::new(label, parsed_url)
                .user_agent(crate::browser::default_desktop_user_agent())
                .incognito(incognito)
                .devtools(true)
                .initialization_script(browser_guest_bridge_script(&guest_bridge_nonce, is_popup))
                // Native new windows never open. In an Isolated tab the broker owns
                // window.open; in an Unavailable tab nothing can attest the request, so it is
                // denied visibly instead of vanishing (design 3.5, "as implemented").
                .on_new_window(move |url, _features| {
                    use crate::browser::bridge::{new_window_policy, NewWindowDecision};
                    let kind = new_window_manager
                        .get_state(&new_window_browser_id)
                        .map(|state| state.trusted_bridge)
                        .unwrap_or_default();
                    if let NewWindowDecision::DenyWithWarning(reason) =
                        new_window_policy(kind, url.as_str())
                    {
                        tracing::warn!(browser_id = %new_window_browser_id, url = %url, reason, "native new window denied");
                        let app = new_window_app.clone();
                        let manager = Arc::clone(&new_window_manager);
                        let browser_id = new_window_browser_id.clone();
                        let script = crate::browser::popup::page_warning_script(&format!(
                            "[ferryx] {reason}: {url}"
                        ));
                        tauri::async_runtime::spawn(async move {
                            if let Err(error) =
                                eval_browser_session(&app, &manager, &browser_id, &script).await
                            {
                                tracing::debug!(%error, "could not report a denied new window to the page");
                            }
                        });
                    }
                    tauri::webview::NewWindowResponse::Deny
                })
                .on_navigation(
                    move |target| match parse_browser_guest_action(target, &nonce) {
                        // Class (b) only: self-scoped shortcuts and popup-handle traffic.
                        // Class (a) actions arrive through the trusted bridge sink.
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
                        Some(BrowserGuestAction::PopupNavigate { url, handle }) => {
                            let app = bridge_app.clone();
                            let manager = Arc::clone(&bridge_manager);
                            let opener_browser_id = bridge_browser_id.clone();
                            tauri::async_runtime::spawn(async move {
                                if let Err(error) = navigate_linked_popup(
                                    &app,
                                    &manager,
                                    &opener_browser_id,
                                    &handle,
                                    &url,
                                )
                                .await
                                {
                                    tracing::warn!(%error, opener = %opener_browser_id, "popup navigation refused");
                                }
                            });
                            false
                        }
                        Some(BrowserGuestAction::OpenerMessage { message }) => {
                            let app = bridge_app.clone();
                            let manager = Arc::clone(&bridge_manager);
                            let popup_browser_id = bridge_browser_id.clone();
                            tauri::async_runtime::spawn(async move {
                                deliver_popup_opener_message(
                                    &app,
                                    &manager,
                                    &popup_browser_id,
                                    &message,
                                )
                                .await;
                            });
                            false
                        }
                        Some(BrowserGuestAction::PopupCloseHandle { handle }) => {
                            let app = bridge_app.clone();
                            let manager = Arc::clone(&bridge_manager);
                            let opener_browser_id = bridge_browser_id.clone();
                            tauri::async_runtime::spawn(async move {
                                close_linked_popup(&app, &manager, &opener_browser_id, &handle).await;
                            });
                            false
                        }
                        Some(BrowserGuestAction::PopupMessage { handle, message }) => {
                            let app = bridge_app.clone();
                            let manager = Arc::clone(&bridge_manager);
                            let opener_browser_id = bridge_browser_id.clone();
                            tauri::async_runtime::spawn(async move {
                                deliver_popup_handle_message(
                                    &app,
                                    &manager,
                                    &opener_browser_id,
                                    &handle,
                                    &message,
                                )
                                .await;
                            });
                            false
                        }
                        None => true,
                    },
                )
                .on_page_load(move |webview, payload| {
                    let loading = matches!(payload.event(), PageLoadEvent::Started);
                    let page_url = payload.url().to_string();
                    if page_bootstrapping_blank.load(std::sync::atomic::Ordering::SeqCst) {
                        if page_url == "about:blank" {
                            return;
                        }
                        page_bootstrapping_blank.store(false, std::sync::atomic::Ordering::SeqCst);
                    }
                    if !loading {
                        let _ = webview.eval(&page_load_bridge_script);
                    }
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

            let creation_outcome = match window_clone.add_child(
                builder,
                tauri::LogicalPosition { x: pos.x, y: pos.y },
                tauri::LogicalSize {
                    width: size.width,
                    height: size.height,
                },
            ) {
                Ok(child) => {
                    match keep_or_discard_fresh_webview(
                        creation_manager.get_state(&browser_id).is_ok(),
                    ) {
                        Ok(()) => 'kept: {
                            let _ = child.eval(&eval_bridge_script);
                            let _ = child.set_zoom(zoom_factor);
                            let bridge_kind =
                                crate::browser::bridge::install(&child, &install_ctx);
                            if let Err(error) =
                                creation_manager.set_trusted_bridge(&browser_id, bridge_kind)
                            {
                                tracing::warn!(browser_id = %browser_id, %error, "could not record the trusted bridge kind");
                            }
                            if let Err(message) = discard_if_abandoned_after_install(
                                || creation_manager.get_state(&browser_id).is_ok(),
                                || {
                                    let label = child.label().to_string();
                                    let _ = child.close();
                                    crate::browser::bridge::forget_webview(child.app_handle(), &label);
                                    // remove_session may have run before install inserted the world.
                                    #[cfg(target_os = "windows")]
                                    crate::browser::bridge::windows::forget_webview(&label);
                                },
                            ) {
                                break 'kept Err(message);
                            }
                            match target_parsed.filter(|url| url.as_str() != "about:blank") {
                                Some(url) => {
                                    if let Err(error) = child.navigate(url) {
                                        bootstrapping_blank
                                            .store(false, std::sync::atomic::Ordering::SeqCst);
                                        tracing::warn!(browser_id = %browser_id, %error, "initial navigation after the bridge install failed");
                                        let _ = creation_manager.update_navigation_state(
                                            &browser_id,
                                            None,
                                            None,
                                            Some(false),
                                            None,
                                            None,
                                            Some(error.to_string()),
                                        );
                                    }
                                }
                                None => {
                                    bootstrapping_blank
                                        .store(false, std::sync::atomic::Ordering::SeqCst);
                                }
                            }
                            #[cfg(target_os = "linux")]
                            let initial_bounds = creation_manager
                                .get_bounds(&browser_id)
                                .ok()
                                .flatten()
                                .unwrap_or(LogicalRect {
                                    x: pos.x,
                                    y: pos.y,
                                    width: size.width,
                                    height: size.height,
                                });

                            #[cfg(target_os = "linux")]
                            {
                                if let Err(e) = crate::browser::linux::implementation::attach_child_to_overlay(
                                    &window_clone,
                                    &browser_id,
                                    &child,
                                    &initial_bounds,
                                ) {
                                    tracing::warn!(browser_id = %browser_id, error = %e, "Failed to attach child webview to Linux GTK overlay");
                                }
                            }

                            if let Ok(Some(current_bounds)) =
                                creation_manager.get_bounds(&browser_id)
                            {
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
                            let is_visible =
                                creation_manager.is_visible(&browser_id).unwrap_or(visible);
                            if !is_visible {
                                let _ = child.hide();
                                #[cfg(target_os = "linux")]
                                {
                                    let _ = crate::browser::linux::implementation::set_child_visible(
                                        &browser_id,
                                        false,
                                    );
                                }
                            } else {
                                let _ = child.show();
                                #[cfg(target_os = "linux")]
                                {
                                    let _ = crate::browser::linux::implementation::set_child_visible(
                                        &browser_id,
                                        true,
                                    );
                                }
                            }
                            Ok(())
                        }
                        Err(message) => {
                            // The async side already gave up and removed the
                            // session; never leave a shown webview behind.
                            let _ = child.close();
                            Err(message)
                        }
                    }
                }
                Err(error) => Err(error.to_string()),
            };
            let _ = creation_sender.send(creation_outcome);
        });

        match run_result {
            Ok(()) => {
                let creation_result =
                    tokio::time::timeout(std::time::Duration::from_secs(5), creation_receiver)
                        .await;
                match creation_result {
                    Ok(Ok(Ok(()))) => {}
                    Ok(Ok(Err(error))) => {
                        discard_failed_creation(app, manager, &state.browser_id);
                        return Err(BrowserError::CreateFailed(error).into());
                    }
                    Ok(Err(_)) | Err(_) => {
                        discard_timed_out_creation(app, manager, &state);
                        return Err(BrowserError::CreateFailed(
                            "browser webview creation did not complete on the main thread".into(),
                        )
                        .into());
                    }
                }
            }
            Err(error) => {
                discard_failed_creation(app, manager, &state.browser_id);
                return Err(BrowserError::CreateFailed(error.to_string()).into());
            }
        }
    } else {
        // No host window to attach to: a session without a webview would be picked as the
        // active tab and fail every later command with WEBVIEW_NOT_FOUND.
        discard_failed_session(manager, &state.browser_id);
        return Err(BrowserError::CreateFailed("main window unavailable".into()).into());
    }

    if let Some(link) = opener.as_ref() {
        if !crate::browser::popup::link(&state.browser_id, &link.browser_id, &link.handle) {
            let _ = close_browser_session(app, manager, &state.browser_id).await;
            return Err(BrowserError::CreateFailed("popup closed while creation pending".into()).into());
        }
    }
    // Report the bridge kind the platform install recorded.
    Ok(manager.get_state(&state.browser_id).unwrap_or(state))
}

/// Publishes a session state change without a webview in scope (CEF tabs have none).
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn emit_session_state<R: tauri::Runtime>(app: &AppHandle<R>, state: &BrowserState) {
    let _ = app.emit(
        BROWSER_STATE_CHANGED_EVENT,
        BrowserStateChangedPayload::from(state),
    );
}

/// Opens the tab in its profile's Chromium engine, whose browser window sits over the pane.
/// The engine reports the page's address, title and loading state as events.
#[cfg(any(target_os = "windows", target_os = "macos"))]
async fn create_cef_browser_session<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &Arc<BrowserManager>,
    state: BrowserState,
    engine_dir: std::path::PathBuf,
) -> Result<BrowserState, IpcError> {
    use crate::browser::cef_engine;
    let browser_id = state.browser_id.clone();
    let fail = |message: String| -> IpcError {
        discard_failed_session(manager, &browser_id);
        BrowserError::CreateFailed(message).into()
    };
    // The engine draws its own toolbar, so the pane hides the Ferryx one for this tab.
    let state = match manager.set_engine(&browser_id, crate::browser::BrowserEngine::Chromium) {
        Ok(state) => state,
        Err(error) => return Err(fail(error.to_string())),
    };
    let Some(main_window) = app.get_window("main") else {
        return Err(fail("main window unavailable".into()));
    };
    #[cfg(target_os = "windows")]
    let parent = match main_window.hwnd() {
        Ok(hwnd) => hwnd.0 as isize,
        Err(error) => return Err(fail(format!("main window handle: {error}"))),
    };
    #[cfg(target_os = "macos")]
    let Some(parent) = cef_engine::window_number(&main_window) else {
        return Err(fail("main window number unavailable".into()));
    };
    let bounds = manager
        .get_bounds(&browser_id)
        .ok()
        .flatten()
        .unwrap_or(LogicalRect {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 600.0,
        });
    let rect = cef_pane_rect(&main_window, &bounds);
    // One engine per profile shares cookies, logins and extensions across that profile's tabs;
    // a private tab's engine keeps its data in memory.
    let profile = state.profile_id.as_str().to_string();
    let cache_dir = if state.profile_id.is_private() {
        None
    } else {
        match engine_data_dir(app) {
            Ok(root) => Some(root.join("browser-cef").join(&profile)),
            Err(error) => return Err(fail(error.message)),
        }
    };
    let sink = cef_event_sink(app.clone(), Arc::clone(manager));
    let open_id = browser_id.clone();
    let url = state.url.clone();
    let visible = state.visible;
    if let Err(error) = crate::ipc::run_blocking(move || {
        let launch = cef_engine::EngineLaunch {
            engine_dir: &engine_dir,
            cache_dir: cache_dir.as_deref(),
            parent,
        };
        cef_engine::open(&launch, &profile, sink, &open_id, rect, &url, visible).map_err(IpcError::from)
    })
    .await
    {
        discard_failed_session(manager, &browser_id);
        return Err(error);
    }
    emit_session_state(app, &state);
    Ok(state)
}

/// Applies a Chromium engine's events: page state to the tab, a closed browser to a closed tab,
/// and a window the engine opened on its own (a new tab, a sign-in popup) to a new tab.
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn cef_event_sink<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: Arc<BrowserManager>,
) -> crate::browser::cef_engine::EventSink {
    use crate::browser::cef_engine::{self, Event};
    Arc::new(move |profile: &str, event: Event| {
        let (browser_id, update) = match event {
            Event::Created { tab, .. } => {
                cef_engine::repaint(&tab);
                return;
            }
            Event::Address { tab, url } => {
                let update = manager.update_navigation_state(&tab, Some(url), None, None, None, None, None);
                (tab, update)
            }
            Event::Title { tab, title } => {
                let update = manager.update_navigation_state(&tab, None, Some(title), None, None, None, None);
                (tab, update)
            }
            Event::Loading { tab, loading, can_go_back, can_go_forward } => {
                let update = manager.update_navigation_state(
                    &tab,
                    None,
                    None,
                    Some(loading),
                    Some(can_go_back),
                    Some(can_go_forward),
                    None,
                );
                (tab, update)
            }
            Event::Closed { tab } => {
                let _ = app.emit(
                    crate::browser::guest::BROWSER_CLOSE_REQUESTED_EVENT,
                    crate::browser::model::BrowserCloseRequestedPayload { browser_id: tab },
                );
                return;
            }
            Event::Popup { popup, url, .. } => {
                adopt_cef_popup(&app, &manager, profile, popup, url);
                return;
            }
            // DevTools answers go to their caller in `cef_engine`, never to the sink.
            Event::Devtools { .. } => return,
        };
        if let Ok(state) = update {
            emit_session_state(&app, &state);
            // The engine paints a page's first frame after it reports the change.
            cef_engine::repaint(&browser_id);
        }
    })
}

/// Gives a window the engine opened on its own a Ferryx tab of the same profile.
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn adopt_cef_popup<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    profile: &str,
    popup: u32,
    url: String,
) {
    let request = CreateBrowserRequest {
        browser_id: None,
        workspace_id: None,
        worktree_path: None,
        // The page is already loading in the engine; the address event brings the real URL.
        url: if url.starts_with("http") { url } else { "about:blank".to_string() },
        profile: BrowserProfileId::from_id(profile),
        zoom_factor: None,
        bounds: None,
        visible: Some(false),
    };
    let state = match manager.register_session(request) {
        Ok(state) => state,
        Err(error) => {
            tracing::warn!(%error, %profile, popup, "could not give an engine window a tab");
            return;
        }
    };
    record_session_created(&state.browser_id);
    let state = match manager.set_engine(&state.browser_id, crate::browser::BrowserEngine::Chromium) {
        Ok(state) => state,
        Err(error) => {
            discard_failed_session(manager, &state.browser_id);
            tracing::warn!(%error, "could not give an engine window a tab");
            return;
        }
    };
    if !crate::browser::cef_engine::adopt(profile, &state.browser_id, popup) {
        discard_failed_session(manager, &state.browser_id);
        return;
    }
    let _ = app.emit(
        crate::browser::guest::BROWSER_SESSION_CREATED_EVENT,
        crate::browser::model::BrowserSessionCreatedPayload {
            browser: state,
            workspace_id: None,
        },
    );
}

/// The host receiver the platform bridge calls with `(browser_id, raw envelope, attestation)`.
/// It may run on any thread, so it only parses and hands off to the async dispatcher.
pub(crate) fn trusted_bridge_sink<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: Arc<BrowserManager>,
) -> crate::browser::bridge::BridgeSink {
    Arc::new(move |browser_id: &str, raw: &str, attestation| {
        let action = match crate::browser::bridge::parse_envelope(raw) {
            Ok(action) => action,
            Err(error) => {
                tracing::warn!(browser_id = %browser_id, %error, "rejected a trusted bridge envelope");
                return;
            }
        };
        let app = app.clone();
        let manager = Arc::clone(&manager);
        let browser_id = browser_id.to_string();
        tauri::async_runtime::spawn(async move {
            dispatch_trusted_action(&app, &manager, &browser_id, action, attestation).await;
        });
    })
}

/// Gates one class (a) request through [`crate::browser::bridge::admit`] and carries out the
/// decision. These are the actions the page route used to deliver by navigation.
pub(crate) async fn dispatch_trusted_action<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &Arc<BrowserManager>,
    browser_id: &str,
    action: crate::browser::bridge::TrustedAction,
    attestation: crate::browser::bridge::Attestation,
) -> crate::browser::bridge::Decision {
    use crate::browser::bridge::{admit, AdmitCtx, BridgeEmit, Decision};

    let state = match manager.get_state(browser_id) {
        Ok(state) => state,
        Err(error) => {
            tracing::warn!(%error, "trusted bridge action for an unknown browser");
            return Decision::Deny("unknown browser session");
        }
    };
    let armed = manager.armed_pick_token(browser_id);
    let native = manager.recent_native_user_initiated(browser_id);
    let ctx = AdmitCtx {
        is_popup: crate::browser::popup::lookup(browser_id).is_some(),
        armed_pick_token: armed.as_deref(),
        recent_native_user_initiated: &native,
        now: std::time::Instant::now(),
    };
    let decision = admit(action, attestation, state.trusted_bridge, &ctx);
    // Host-side popup cap per opener, on top of the broker's one-open-per-activation rule.
    let decision = match decision {
        Decision::Emit(BridgeEmit::PopupOpen { .. })
            if !manager.admit_popup_open(browser_id, std::time::Instant::now()) =>
        {
            Decision::Deny(crate::browser::bridge::DENY_POPUP_RATE)
        }
        other => other,
    };
    let emit_download = |target_url: String, requires_confirmation: bool| {
        let _ = app.emit(
            BROWSER_DOWNLOAD_REQUESTED_EVENT,
            BrowserDownloadRequestedPayload {
                browser_id: browser_id.to_string(),
                target_url,
                requires_confirmation,
            },
        );
    };
    match &decision {
        Decision::Emit(BridgeEmit::Download { url }) => emit_download(url.clone(), false),
        Decision::NeedsConfirmation(BridgeEmit::Download { url }) => {
            emit_download(url.clone(), true)
        }
        Decision::Emit(BridgeEmit::Link { url, modifier }) => {
            let _ = app.emit(
                BROWSER_LINK_CLICKED_EVENT,
                BrowserLinkClickedPayload {
                    browser_id: browser_id.to_string(),
                    target_url: url.clone(),
                    modifier: *modifier,
                    profile_id: state.profile_id.clone(),
                    worktree_path: state.worktree_path.clone(),
                },
            );
        }
        Decision::Emit(BridgeEmit::NavigateInTab { url })
        | Decision::Downgrade(BridgeEmit::NavigateInTab { url }) => {
            if let Err(error) = navigate_browser_session(app, manager, browser_id, url).await {
                tracing::warn!(%error, browser_id = %browser_id, "downgraded link navigation failed");
            }
        }
        Decision::Emit(BridgeEmit::PopupOpen { url, handle }) => {
            let _ = app.emit(
                BROWSER_OPEN_REQUESTED_EVENT,
                BrowserOpenRequestedPayload {
                    browser_id: browser_id.to_string(),
                    target_url: url.clone(),
                    profile_id: state.profile_id.clone(),
                    worktree_path: state.worktree_path.clone(),
                    opener_browser_id: Some(browser_id.to_string()),
                    popup_handle: Some(handle.clone()),
                },
            );
        }
        Decision::Emit(BridgeEmit::ElementPicked { token }) => {
            // One arm yields one pick: a replayed token finds nothing armed.
            if manager.consume_pick_token(browser_id, token) {
                let _ = app.emit(
                    BROWSER_ELEMENT_PICKED_EVENT,
                    BrowserElementPickedPayload {
                        browser_id: browser_id.to_string(),
                    },
                );
            } else {
                return Decision::Deny(crate::browser::bridge::DENY_PICK_TOKEN);
            }
        }
        Decision::Emit(BridgeEmit::CloseSelf) => {
            let _ = app.emit(
                crate::browser::guest::BROWSER_CLOSE_REQUESTED_EVENT,
                crate::browser::model::BrowserCloseRequestedPayload {
                    browser_id: browser_id.to_string(),
                },
            );
        }
        Decision::Deny(reason) => {
            tracing::warn!(browser_id = %browser_id, reason, "trusted bridge action denied");
            let script = crate::browser::popup::page_warning_script(&format!("[ferryx] {reason}"));
            if let Err(error) = eval_browser_session(app, manager, browser_id, &script).await {
                tracing::debug!(%error, "could not report a denied bridge action to the page");
            }
        }
        other => {
            tracing::warn!(browser_id = %browser_id, decision = ?other, "unexpected trusted bridge decision");
        }
    }
    decision
}

#[tauri::command]
pub async fn cmd_browser_create<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    request: CreateBrowserRequest,
    opener: Option<crate::browser::BrowserOpenerLink>,
) -> Result<BrowserState, IpcError> {
    create_browser_session_with_opener(&app, manager.inner(), request, opener).await
}

#[tauri::command]
pub async fn cmd_browser_open_devtools<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
) -> Result<(), IpcError> {
    let state = manager.get_state(&browser_id)?;
    let webview = app
        .get_webview(&state.webview_label)
        .ok_or_else(|| BrowserError::WebviewNotFound(state.webview_label.clone()))?;
    webview.open_devtools();
    Ok(())
}

#[tauri::command]
pub async fn cmd_browser_inject_element_picker<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
) -> Result<(), IpcError> {
    let webview = browser_webview(&app, manager.inner(), &browser_id)?;
    let token = manager.arm_pick_token(&browser_id);
    if let Err(error) = crate::browser::bridge::eval_in_world(
        webview,
        crate::browser::bridge::picker_arm_script(&token),
    )
    .await
    {
        manager.clear_pick_tokens(&browser_id);
        return Err(error.into());
    }
    Ok(())
}

#[tauri::command]
pub async fn cmd_browser_remove_element_picker<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
) -> Result<(), IpcError> {
    manager.clear_pick_tokens(&browser_id);
    let webview = browser_webview(&app, manager.inner(), &browser_id)?;
    crate::browser::bridge::eval_in_world(
        webview,
        crate::browser::bridge::PICKER_DISARM_SCRIPT.to_string(),
    )
    .await?;
    Ok(())
}

fn browser_webview<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    browser_id: &str,
) -> Result<tauri::Webview<R>, IpcError> {
    let state = manager.get_state(browser_id)?;
    app.get_webview(&state.webview_label)
        .ok_or_else(|| BrowserError::WebviewNotFound(state.webview_label).into())
}

/// The finish step only proceeds for a pick the host admitted (its one-time token was
/// consumed by [`dispatch_trusted_action`]); anything else is the empty-pick error.
fn require_consumed_pick(manager: &BrowserManager, browser_id: &str) -> Result<String, IpcError> {
    manager
        .take_consumed_pick_token(browser_id)
        .ok_or_else(|| IpcError::new(IpcErrorCode::ParseError, "element pick is empty"))
}

#[tauri::command]
pub async fn cmd_browser_finish_element_pick<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
    png_base64: Option<String>,
) -> Result<crate::remote::design_mode::DesignModeSnapshot, IpcError> {
    let state = manager.get_state(&browser_id)?;
    require_consumed_pick(manager.inner(), &browser_id)?;
    let webview = browser_webview(&app, manager.inner(), &browser_id)?;
    let raw = crate::browser::bridge::eval_in_world(
        webview.clone(),
        crate::browser::bridge::PICKER_READ_SCRIPT.to_string(),
    )
    .await?;
    let json = Some(crate::browser::picker::unwrap_eval_json(raw.as_str()))
        .filter(|value| !value.trim().is_empty() && value != "null" && value != "undefined")
        .ok_or_else(|| IpcError::new(IpcErrorCode::ParseError, "element pick is empty"))?;
    if let Err(error) = crate::browser::bridge::eval_in_world(
        webview,
        crate::browser::bridge::PICKER_DISARM_SCRIPT.to_string(),
    )
    .await
    {
        tracing::warn!(%error, browser_id = %browser_id, "could not disarm the element picker");
    }
    let png = match png_base64.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
        Some(encoded) => base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|e| IpcError::new(IpcErrorCode::ParseError, e.to_string()))?,
        None => {
            crate::browser::capture_browser_snapshot(&app, &state.webview_label, SnapshotOptions::png())
                .await?
                .bytes
        }
    };
    let mut snapshot = crate::browser::picker::element_pick_report(&json, &png)
        .map_err(|e| IpcError::new(IpcErrorCode::ParseError, e))?;
    snapshot.session_id = browser_id;
    snapshot.timestamp_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    crate::remote::design_mode::shared_staging_store().stage(snapshot.clone());
    Ok(snapshot)
}

#[cfg(test)]
mod trusted_dispatch_tests {
    use super::*;
    use crate::browser::bridge::{
        Attestation, BridgeEmit, Decision, TrustedAction, TrustedBridgeKind, DENY_PICK_TOKEN,
        DENY_SELF_CLOSE_NOT_POPUP,
    };
    use std::sync::Mutex as StdMutex;
    use tauri::Listener;

    fn session(manager: &BrowserManager, id: &str, kind: TrustedBridgeKind) {
        manager
            .register_session(CreateBrowserRequest {
                browser_id: Some(id.into()),
                workspace_id: None,
                worktree_path: None,
                url: "https://example.com/".into(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: None,
            })
            .unwrap();
        manager.set_trusted_bridge(id, kind).unwrap();
    }

    fn capture(app: &tauri::App<tauri::test::MockRuntime>, event: &str) -> Arc<StdMutex<Vec<String>>> {
        let seen = Arc::new(StdMutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        app.listen_any(event, move |event| sink.lock().unwrap().push(event.payload().to_string()));
        seen
    }

    #[test]
    fn pick_ready_emits_once_per_armed_token_and_finish_requires_consumption() {
        let app = tauri::test::mock_app();
        let manager = Arc::new(BrowserManager::new());
        session(&manager, "pick-1", TrustedBridgeKind::Isolated);
        let picked = capture(&app, BROWSER_ELEMENT_PICKED_EVENT);

        // Nothing armed: denied, nothing emitted, finish has nothing to read.
        let denied = tauri::async_runtime::block_on(dispatch_trusted_action(
            app.handle(), &manager, "pick-1",
            TrustedAction::PickReady { token: "guess".into() }, Attestation::IsolatedWorld,
        ));
        assert_eq!(denied, Decision::Deny(DENY_PICK_TOKEN));
        let error = require_consumed_pick(&manager, "pick-1").unwrap_err();
        assert_eq!(error.code, IpcErrorCode::ParseError);
        assert_eq!(error.message, "element pick is empty");

        let token = manager.arm_pick_token("pick-1");
        let accepted = tauri::async_runtime::block_on(dispatch_trusted_action(
            app.handle(), &manager, "pick-1",
            TrustedAction::PickReady { token: token.clone() }, Attestation::IsolatedWorld,
        ));
        assert_eq!(accepted, Decision::Emit(BridgeEmit::ElementPicked { token: token.clone() }));
        // Replay of the consumed token is denied.
        let replay = tauri::async_runtime::block_on(dispatch_trusted_action(
            app.handle(), &manager, "pick-1",
            TrustedAction::PickReady { token: token.clone() }, Attestation::IsolatedWorld,
        ));
        assert_eq!(replay, Decision::Deny(DENY_PICK_TOKEN));
        assert_eq!(picked.lock().unwrap().len(), 1);
        assert_eq!(require_consumed_pick(&manager, "pick-1").unwrap(), token);
        assert!(require_consumed_pick(&manager, "pick-1").is_err());
    }

    #[test]
    fn unattested_download_is_emitted_as_needing_confirmation() {
        let app = tauri::test::mock_app();
        let manager = Arc::new(BrowserManager::new());
        session(&manager, "dl-1", TrustedBridgeKind::Unavailable);
        session(&manager, "dl-2", TrustedBridgeKind::Isolated);
        let downloads = capture(&app, BROWSER_DOWNLOAD_REQUESTED_EVENT);
        for (id, attestation) in [("dl-1", Attestation::None), ("dl-2", Attestation::IsolatedWorld)] {
            tauri::async_runtime::block_on(dispatch_trusted_action(
                app.handle(), &manager, id,
                TrustedAction::Download { url: "https://example.com/a.zip".into() }, attestation,
            ));
        }
        let seen = downloads.lock().unwrap().clone();
        assert_eq!(seen.len(), 2);
        let first: serde_json::Value = serde_json::from_str(&seen[0]).unwrap();
        let second: serde_json::Value = serde_json::from_str(&seen[1]).unwrap();
        assert_eq!(first["browserId"], "dl-1");
        assert_eq!(first["requiresConfirmation"], true);
        assert_eq!(second["browserId"], "dl-2");
        assert!(second.get("requiresConfirmation").is_none());
    }

    #[test]
    fn self_close_on_a_non_popup_tab_emits_nothing() {
        let _guard = crate::browser::popup::registry_test_lock().blocking_lock();
        let app = tauri::test::mock_app();
        let manager = Arc::new(BrowserManager::new());
        session(&manager, "close-1", TrustedBridgeKind::Isolated);
        let closes = capture(&app, crate::browser::guest::BROWSER_CLOSE_REQUESTED_EVENT);
        let decision = tauri::async_runtime::block_on(dispatch_trusted_action(
            app.handle(), &manager, "close-1",
            TrustedAction::PopupSelfClose, Attestation::IsolatedWorld,
        ));
        assert_eq!(decision, Decision::Deny(DENY_SELF_CLOSE_NOT_POPUP));
        assert!(closes.lock().unwrap().is_empty());

        assert!(crate::browser::popup::link("close-1", "opener-x", "h-1"));
        let decision = tauri::async_runtime::block_on(dispatch_trusted_action(
            app.handle(), &manager, "close-1",
            TrustedAction::PopupSelfClose, Attestation::IsolatedWorld,
        ));
        crate::browser::popup::unlink("close-1");
        assert_eq!(decision, Decision::Emit(BridgeEmit::CloseSelf));
        assert_eq!(closes.lock().unwrap().len(), 1);
    }

    #[test]
    fn sink_rejects_malformed_envelopes_without_dispatching() {
        let app = tauri::test::mock_app();
        let manager = Arc::new(BrowserManager::new());
        session(&manager, "sink-1", TrustedBridgeKind::Isolated);
        let downloads = capture(&app, BROWSER_DOWNLOAD_REQUESTED_EVENT);
        let sink = trusted_bridge_sink(app.handle().clone(), Arc::clone(&manager));
        sink("sink-1", r#"{"v":1,"kind":"download","url":"javascript:alert(1)"}"#, Attestation::IsolatedWorld);
        sink("sink-1", "not json", Attestation::IsolatedWorld);
        // Parsing is synchronous and rejected envelopes never spawn a dispatch.
        assert!(downloads.lock().unwrap().is_empty());
    }

    /// n1: a later trusted bridge upgrade (macOS creation callback, Windows CDP leak probe)
    /// must reach the UI, or a toolbar that read pick:false keeps the picker disabled.
    #[test]
    fn trusted_bridge_change_emits_browser_state_with_the_new_kind() {
        let app = tauri::test::mock_app();
        let manager = Arc::new(BrowserManager::new());
        install_browser_state_emitter(app.handle(), &manager);
        let states = capture(&app, BROWSER_STATE_CHANGED_EVENT);

        // Registering at the default kind and re-recording it is not a change.
        session(&manager, "bridge-1", TrustedBridgeKind::Unavailable);
        assert!(states.lock().unwrap().is_empty());

        manager.set_trusted_bridge("bridge-1", TrustedBridgeKind::Isolated).unwrap();
        manager.set_trusted_bridge("bridge-1", TrustedBridgeKind::Isolated).unwrap();
        let seen = states.lock().unwrap().clone();
        assert_eq!(seen.len(), 1, "exactly one emit per kind change: {seen:?}");
        let payload: serde_json::Value = serde_json::from_str(&seen[0]).unwrap();
        assert_eq!(payload["browserId"], "bridge-1");
        assert_eq!(payload["trustedBridge"], "isolated");

        manager.set_trusted_bridge("bridge-1", TrustedBridgeKind::Unavailable).unwrap();
        let seen = states.lock().unwrap().clone();
        assert_eq!(seen.len(), 2);
        let payload: serde_json::Value = serde_json::from_str(&seen[1]).unwrap();
        assert_eq!(payload["trustedBridge"], "unavailable");
    }
}

pub async fn navigate_browser_session<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &Arc<BrowserManager>,
    browser_id: &str,
    url: &str,
) -> Result<(), IpcError> {
    navigate_browser_session_with_context(
        app,
        manager,
        browser_id,
        url,
        crate::browser::security::BrowserSecurityContext::Local,
    )
    .await
}

/// Navigates a session, validating the URL under `context`; remote executors pass
/// `BrowserSecurityContext::Remote` so local-only schemes (file://) stay denied.
pub async fn navigate_browser_session_with_context<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &Arc<BrowserManager>,
    browser_id: &str,
    url: &str,
    context: crate::browser::security::BrowserSecurityContext,
) -> Result<(), IpcError> {
    // Resolve the webview before recording the navigation: without one, nothing would load,
    // and the session must not be left with a bumped generation and `loading = true`.
    // The URL policy is checked first so a refused URL reports its policy error.
    crate::browser::security::validate_url_with_context(url, context)?;
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    if crate::browser::cef_engine::is_cef_tab(browser_id) {
        let valid_url = manager.update_url_with_context(browser_id, url, context)?;
        emit_session_state(app, &manager.get_state(browser_id)?);
        crate::browser::cef_engine::navigate(browser_id, &valid_url)?;
        return Ok(());
    }
    let label = manager.get_webview_label(browser_id)?;
    let webview = app
        .get_webview(&label)
        .ok_or_else(|| BrowserError::WebviewNotFound(label.clone()))?;
    let valid_url = manager.update_url_with_context(browser_id, url, context)?;
    let state = manager.get_state(browser_id)?;

    emit_browser_state(&webview, &state);
    let parsed = valid_url.parse().map_err(|error| {
        BrowserError::NavigationFailed(format!("invalid target URL: {error}"))
    })?;
    if let Err(error) = webview.navigate(parsed) {
        let message = error.to_string();
        if let Ok(error_state) = manager.update_navigation_state(
            browser_id,
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
    Ok(())
}

#[tauri::command]
pub async fn cmd_browser_navigate<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
    url: String,
) -> Result<(), IpcError> {
    let state = manager.get_state(&browser_id)?;
    // The desktop user navigating a tab takes it back from any agent holding it.
    crate::browser::agent::service::desktop_touched(&browser_id);
    // A Chromium-engine tab has no Tauri webview; its engine process takes the navigation.
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    let has_view = crate::browser::cef_engine::is_cef_tab(&browser_id)
        || app.get_webview(&state.webview_label).is_some();
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let has_view = app.get_webview(&state.webview_label).is_some();
    if !has_view {
        return Err(BrowserError::WebviewNotFound(state.webview_label).into());
    }
    navigate_browser_session(&app, manager.inner(), &browser_id, &url).await
}

pub(crate) fn history_navigation<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &Arc<BrowserManager>,
    browser_id: &str,
    forward: bool,
) -> Result<(), IpcError> {
    // Resolve the webview before the dispatch records anything, so a missing webview
    // leaves the session's loading state and generation untouched.
    let label = manager.get_webview_label(browser_id)?;
    let webview = app
        .get_webview(&label)
        .ok_or_else(|| BrowserError::WebviewNotFound(label.clone()))?;
    let (state, dispatch) = manager.begin_history_dispatch(browser_id, forward)?;
    emit_browser_state(&webview, &state);

    if !dispatch {
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
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    {
        let script = if forward {
            "history.forward()"
        } else {
            "history.back()"
        };
        if let Err(error) = webview.eval(script) {
            // Only a shadow-vector move (loading) has state to roll back; an
            // engine-only dispatch moved nothing.
            if state.loading {
                if let Ok(restored) = manager.cancel_history_navigation(browser_id, forward) {
                    emit_browser_state(&webview, &restored);
                }
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
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    if crate::browser::cef_engine::is_cef_tab(&browser_id) {
        return Ok(crate::browser::cef_engine::history(&browser_id, false)?);
    }
    history_navigation(&app, manager.inner(), &browser_id, false)
}

#[tauri::command]
pub async fn cmd_browser_go_forward<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
) -> Result<(), IpcError> {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    if crate::browser::cef_engine::is_cef_tab(&browser_id) {
        return Ok(crate::browser::cef_engine::history(&browser_id, true)?);
    }
    history_navigation(&app, manager.inner(), &browser_id, true)
}

/// Both cookie-import commands report an unknown target profile and a profile with no open
/// tab with the same code and structured `reason`, so the settings UI handles them once.
fn unsupported_cookie_import_profile(profile_id: &str) -> IpcError {
    IpcError::new(
        IpcErrorCode::BrowserCookieImportFailed,
        format!("unsupported profile: {profile_id}"),
    )
    .with_details(serde_json::json!({ "reason": "no-profile" }))
}

fn no_cookie_import_tab(profile_id: &BrowserProfileId) -> IpcError {
    IpcError::new(
        IpcErrorCode::BrowserCookieImportFailed,
        format!(
            "open a browser tab using the {} profile before importing cookies",
            profile_id.as_str()
        ),
    )
    .with_details(serde_json::json!({ "reason": "no-tab" }))
}

#[tauri::command]
pub async fn cmd_browser_import_cookies<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    request: ImportBrowserCookiesRequest,
) -> Result<ImportBrowserCookiesResult, IpcError> {
    let profile_id = BrowserProfileId::from_id(&request.profile_id)
        .ok_or_else(|| unsupported_cookie_import_profile(&request.profile_id))?;
    let source = tokio::fs::read_to_string(&request.file_path)
        .await
        .map_err(|error| {
            BrowserError::CookieImport(format!("failed to read cookie file: {error}"))
        })?;
    let cookies = parse_cookie_file(&source)?
        .into_iter()
        .map(cookie_from_imported)
        .collect::<Result<Vec<_>, _>>()?;

    let targets = manager
        .webview_labels_for_profile(&profile_id)
        .into_iter()
        .filter_map(|label| app.get_webview(&label))
        .collect::<Vec<_>>();

    // No fallback to the app's own "main" webview. The user asked to import into a
    // BROWSER profile; with no matching tab open (the common case, since import
    // lives in Settings) falling back would inject every cookie from an arbitrary
    // third-party export file -- unrestricted by domain -- into Ferryx's own
    // privileged webview context, while still reporting success. The empty-target
    // error below is the correct outcome for every profile, Default included.
    if targets.is_empty() {
        return Err(no_cookie_import_tab(&profile_id));
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
pub async fn cmd_browser_import_installed_cookies<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    request: ImportInstalledBrowserCookiesRequest,
) -> Result<ImportInstalledBrowserCookiesResult, IpcError> {
    let profile_id = BrowserProfileId::from_id(&request.profile_id)
        .ok_or_else(|| unsupported_cookie_import_profile(&request.profile_id))?;

    let targets = manager
        .webview_labels_for_profile(&profile_id)
        .into_iter()
        .filter_map(|label| app.get_webview(&label))
        .collect::<Vec<_>>();

    if targets.is_empty() {
        return Err(no_cookie_import_tab(&profile_id));
    }

    let home_dir = app.path().home_dir().map_err(|err| {
        IpcError::new(
            IpcErrorCode::BrowserCookieImportFailed,
            format!("failed to resolve home directory: {err}"),
        )
        .with_details(serde_json::json!({ "reason": "cookie-import-failed" }))
    })?;

    let kind = request.source;
    let source_profile = request.source_profile;

    let read_result: crate::browser::chromium_cookies::ChromiumCookieReadResult =
        crate::ipc::run_blocking(move || {
            crate::browser::chromium_cookie_source::import_installed_browser_cookies_sync(
                kind,
                source_profile.as_deref(),
                Some(&home_dir),
            )
            .map_err(|import_err| {
                IpcError::new(
                    IpcErrorCode::BrowserCookieImportFailed,
                    import_err.to_string(),
                )
                .with_details(serde_json::json!({ "reason": import_err.reason.as_str() }))
            })
        })
        .await?;

    let cookies = read_result
        .cookies
        .into_iter()
        .map(cookie_from_imported)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| {
            IpcError::new(
                IpcErrorCode::BrowserCookieImportFailed,
                format!("failed to convert imported cookie: {err}"),
            )
            .with_details(serde_json::json!({ "reason": "cookie-import-failed" }))
        })?;

    for target in targets {
        for cookie in &cookies {
            let cookie_to_set: cookie::Cookie<'static> = cookie.clone();
            target.set_cookie(cookie_to_set).map_err(|error| {
                IpcError::new(
                    IpcErrorCode::BrowserCookieImportFailed,
                    error.to_string(),
                )
                .with_details(serde_json::json!({ "reason": "cookie-import-failed" }))
            })?;
        }
    }

    let skipped_count = read_result.unsupported_v11_count
        + read_result.decrypt_failed_count
        + read_result.invalid_count;

    Ok(ImportInstalledBrowserCookiesResult {
        imported_count: cookies.len(),
        skipped_count,
    })
}

#[tauri::command]
pub async fn cmd_browser_reload<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
    ignore_cache: Option<bool>,
) -> Result<(), IpcError> {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    if crate::browser::cef_engine::is_cef_tab(&browser_id) {
        let state = manager.begin_reload(&browser_id)?;
        emit_session_state(&app, &state);
        return Ok(
            crate::browser::cef_engine::reload(&browser_id, ignore_cache.unwrap_or(false))?,
        );
    }
    let state = manager.begin_reload(&browser_id)?;
    let webview = app
        .get_webview(&state.webview_label)
        .ok_or_else(|| BrowserError::WebviewNotFound(state.webview_label.clone()))?;
    emit_browser_state(&webview, &state);
    // A caller that asked to bypass the cache must not be handed cached content, so a hard reload
    // that cannot run on this platform fails here instead of degrading to `webview.reload()`.
    let reload_result: Result<(), IpcError> = if ignore_cache.unwrap_or(false) {
        crate::browser::hard_reload::hard_reload(&webview)
    } else {
        webview
            .reload()
            .map_err(|error| BrowserError::NavigationFailed(error.to_string()).into())
    };
    if let Err(error) = reload_result {
        if let Ok(error_state) = manager.update_navigation_state(
            &browser_id,
            None,
            None,
            Some(false),
            None,
            None,
            Some(error.message.clone()),
        ) {
            emit_browser_state(&webview, &error_state);
        }
        return Err(error);
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
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    if crate::browser::cef_engine::is_cef_tab(&browser_id) {
        if let Some(main_window) = app.get_window("main") {
            crate::browser::cef_engine::set_bounds(&browser_id, cef_pane_rect(&main_window, &bounds));
        }
        return Ok(());
    }
    let state = manager.get_state(&browser_id)?;
    let webview = app
        .get_webview(&state.webview_label)
        .ok_or_else(|| BrowserError::WebviewNotFound(state.webview_label.clone()))?;
    #[cfg(target_os = "linux")]
    {
        let browser_id_clone = browser_id.clone();
        let bounds_clone = bounds.clone();
        let _ = app.run_on_main_thread(move || {
            if let Err(e) = crate::browser::linux::implementation::update_child_bounds(
                &browser_id_clone,
                &bounds_clone,
            ) {
                tracing::warn!(browser_id = %browser_id_clone, error = %e, "Failed to update child webview bounds in Linux GTK overlay");
            }
        });
    }

    // The frontend reveals the webview only after this call resolves, so a discarded failure
    // here would show an opaque child at its previous frame over unrelated panes.
    webview
        .set_bounds(tauri::Rect {
            position: tauri::Position::Logical(tauri::LogicalPosition {
                x: bounds.x,
                y: bounds.y,
            }),
            size: tauri::Size::Logical(tauri::LogicalSize {
                width: bounds.width,
                height: bounds.height,
            }),
        })
        .map_err(|error| {
            BrowserError::Internal(format!("failed to set browser webview bounds: {error}"))
        })?;
    Ok(())
}

pub fn set_browser_session_visible<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    browser_id: &str,
    visible: bool,
) -> Result<(), IpcError> {
    manager.set_visible(browser_id, visible)?;
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    if crate::browser::cef_engine::set_visible(browser_id, visible) {
        return Ok(());
    }
    let state = manager.get_state(browser_id)?;
    let webview = app
        .get_webview(&state.webview_label)
        .ok_or_else(|| BrowserError::WebviewNotFound(state.webview_label.clone()))?;
    let outcome = if visible {
        #[cfg(target_os = "linux")]
        {
            let browser_id_clone = browser_id.to_string();
            let _ = app.run_on_main_thread(move || {
                let _ = crate::browser::linux::implementation::set_child_visible(
                    &browser_id_clone,
                    true,
                );
            });
        }
        webview.show()
    } else {
        #[cfg(target_os = "linux")]
        {
            let browser_id_clone = browser_id.to_string();
            let _ = app.run_on_main_thread(move || {
                let _ = crate::browser::linux::implementation::set_child_visible(
                    &browser_id_clone,
                    false,
                );
            });
        }
        webview.hide()
    };
    // A dropped hide is what strands an opaque child webview over the pane that replaced it.
    outcome.map_err(|error| {
        BrowserError::Internal(format!("failed to set browser webview visibility: {error}"))
    })?;
    Ok(())
}

#[tauri::command]
pub async fn cmd_browser_set_visible<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
    visible: bool,
) -> Result<(), IpcError> {
    set_browser_session_visible(&app, &manager, &browser_id, visible)
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
    let webview = app
        .get_webview(&state.webview_label)
        .ok_or_else(|| BrowserError::WebviewNotFound(state.webview_label.clone()))?;
    webview.set_zoom(clamped).map_err(|error| {
        BrowserError::Internal(format!("failed to set browser webview zoom: {error}"))
    })?;
    Ok(clamped)
}

pub fn focus_browser_session<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    browser_id: &str,
) -> Result<(), IpcError> {
    set_browser_session_visible(app, manager, browser_id, true)?;
    record_session_created(browser_id);
    let state = manager.get_state(browser_id)?;
    let webview = app
        .get_webview(&state.webview_label)
        .ok_or_else(|| BrowserError::WebviewNotFound(state.webview_label.clone()))?;
    if let Some(window) = app.get_window("main") {
        window.show().map_err(|error| {
            BrowserError::Internal(format!("failed to show the main window: {error}"))
        })?;
        window.unminimize().map_err(|error| {
            BrowserError::Internal(format!("failed to unminimize the main window: {error}"))
        })?;
        window.set_focus().map_err(|error| {
            BrowserError::Internal(format!("failed to focus the main window: {error}"))
        })?;
    }
    webview.set_focus().map_err(|error| {
        BrowserError::Internal(format!("failed to focus browser webview: {error}"))
    })?;
    Ok(())
}

#[tauri::command]
pub async fn cmd_browser_focus<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
) -> Result<(), IpcError> {
    focus_browser_session(&app, &manager, &browser_id)
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
pub async fn cmd_browser_download_with_cookies<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    request: BrowserDownloadRequest,
) -> Result<crate::browser::download::DownloadRecord, IpcError> {
    browser_download_with_session(&app, manager.inner(), request).await
}

/// Builds a `Cookie:` header value from the cookies the webview itself would send
/// to the download URL.
///
/// Pure so the contract is unit-testable: every cookie the cookie store returned is
/// included, whatever its attributes say. That matters because the login session
/// cookie is normally `HttpOnly`, which the previous `document.cookie` read could
/// never see while it did send the current page's cookies to a different origin.
pub(crate) fn cookie_header_from_cookies(
    cookies: &[tauri::webview::Cookie<'static>],
) -> Option<String> {
    let pairs: Vec<String> = cookies
        .iter()
        .filter(|cookie| !cookie.name().trim().is_empty())
        .map(|cookie| format!("{}={}", cookie.name(), cookie.value()))
        .collect();
    if pairs.is_empty() {
        None
    } else {
        Some(pairs.join("; "))
    }
}

/// Resolves the `Cookie` header for one download request.
///
/// An explicit `request.cookies` override always wins. Otherwise the cookies come
/// from the webview's own cookie store for the download URL, which is exactly the
/// set the webview would send: `HttpOnly` included, and never the current page's
/// cookies replayed to a different origin. This stays on the async path because
/// tauri documents that the cookie APIs deadlock on Windows when they are called
/// from a synchronous command. The getter blocks until the main thread replies (and
/// panics if the webview closed first), so it runs on a blocking thread where that
/// panic surfaces as an internal error instead of tearing down an async worker.
async fn download_cookie_header<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    request: &BrowserDownloadRequest,
) -> Result<Option<String>, IpcError> {
    if let Some(cookies) = request.cookies.as_ref() {
        return Ok(Some(cookies.clone()));
    }
    let Some(browser_id) = request.browser_id.as_ref() else {
        return Ok(None);
    };
    let state = manager.get_state(browser_id)?;
    let webview = app
        .get_webview(&state.webview_label)
        .ok_or_else(|| BrowserError::WebviewNotFound(state.webview_label.clone()))?;
    let url = tauri::Url::parse(request.url.trim()).map_err(|error| {
        IpcError::new(
            IpcErrorCode::InvalidArgument,
            format!("invalid download url `{}`: {error}", request.url),
        )
    })?;
    let request_url = request.url.clone();
    let cookies = crate::ipc::run_blocking(move || {
        webview.cookies_for_url(url).map_err(|error| {
            IpcError::new(
                IpcErrorCode::BrowserDownloadFailed,
                format!("failed to read session cookies for {request_url}: {error}"),
            )
        })
    })
    .await?;
    Ok(cookie_header_from_cookies(&cookies))
}

pub async fn browser_download_with_session<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    request: BrowserDownloadRequest,
) -> Result<crate::browser::download::DownloadRecord, IpcError> {
    let cookie_header = download_cookie_header(app, manager, &request).await?;

    let download_id = crate::browser::download::download_url_with_cookies(
        &request.url,
        std::path::Path::new(&request.file_path),
        cookie_header.as_deref(),
        None,
    )
    .await?;

    let record = crate::browser::download::get_download(&download_id).ok_or_else(|| {
        IpcError::new(
            IpcErrorCode::BrowserDownloadFailed,
            "Download registered but missing from registry",
        )
    })?;
    Ok(record)
}

#[tauri::command]
pub async fn cmd_browser_download_list() -> Result<Vec<crate::browser::download::DownloadRecord>, IpcError> {
    Ok(crate::browser::download::list_downloads())
}

#[tauri::command]
pub async fn cmd_browser_download_cancel(download_id: String) -> Result<bool, IpcError> {
    Ok(crate::browser::download::cancel_download(&download_id))
}

#[tauri::command]
pub async fn cmd_browser_handle_dialog<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    request: BrowserDialogHandleRequest,
) -> Result<BrowserDialogEntry, IpcError> {
    browser_handle_dialog(&app, manager.inner(), request).await
}

pub async fn browser_handle_dialog<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    request: BrowserDialogHandleRequest,
) -> Result<BrowserDialogEntry, IpcError> {
    let state = manager.get_state(&request.browser_id)?;
    let webview = app
        .get_webview(&state.webview_label)
        .ok_or_else(|| BrowserError::WebviewNotFound(state.webview_label.clone()))?;

    let script = crate::browser::guest::build_dialog_handle_script(
        &request.action,
        request.prompt_text.as_deref(),
    );
    let result = eval_webview(webview, script).await?;
    let unquoted: String = serde_json::from_str(&result).unwrap_or(result);
    let parsed: serde_json::Value = serde_json::from_str(&unquoted).map_err(|e| {
        IpcError::new(
            IpcErrorCode::InternalError,
            format!("invalid dialog response from webview: {e}"),
        )
    })?;

    if let Some(err) = parsed.get("error").and_then(|v| v.as_str()) {
        if err == "no_active_dialog" {
            return Err(IpcError::new(
                IpcErrorCode::BrowserDialogNotFound,
                "No active browser dialog to handle",
            ));
        }
        return Err(IpcError::new(
            IpcErrorCode::InternalError,
            format!("dialog handling failed: {err}"),
        ));
    }

    if let Some(dialog_val) = parsed.get("dialog") {
        let entry: BrowserDialogEntry = serde_json::from_value(dialog_val.clone()).map_err(|e| {
            IpcError::new(
                IpcErrorCode::InternalError,
                format!("failed to parse handled dialog entry: {e}"),
            )
        })?;
        return Ok(entry);
    }

    Err(IpcError::new(
        IpcErrorCode::BrowserDialogNotFound,
        "No active browser dialog to handle",
    ))
}

pub async fn browser_get_dialogs<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    browser_id: &str,
) -> Result<Vec<BrowserDialogEntry>, IpcError> {
    let state = manager.get_state(browser_id)?;
    let webview = app
        .get_webview(&state.webview_label)
        .ok_or_else(|| BrowserError::WebviewNotFound(state.webview_label.clone()))?;

    let script = crate::browser::guest::build_dialog_drain_script();
    let result = eval_webview(webview, script).await?;
    let entries = crate::browser::guest::parse_dialog_drain_result(&result).map_err(|e| {
        IpcError::new(
            IpcErrorCode::InternalError,
            format!("failed to parse dialog history: {e}"),
        )
    })?;
    Ok(entries)
}

#[tauri::command]
pub async fn cmd_browser_get_dialogs<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
) -> Result<Vec<BrowserDialogEntry>, IpcError> {
    browser_get_dialogs(&app, manager.inner(), &browser_id).await
}

pub async fn browser_set_dialog_policy<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    browser_id: &str,
    policy: &str,
) -> Result<(), IpcError> {
    let state = manager.get_state(browser_id)?;
    let webview = app
        .get_webview(&state.webview_label)
        .ok_or_else(|| BrowserError::WebviewNotFound(state.webview_label.clone()))?;

    let script = crate::browser::guest::build_dialog_policy_script(policy);
    eval_webview(webview, script).await?;
    Ok(())
}

#[tauri::command]
pub async fn cmd_browser_set_dialog_policy<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    request: BrowserDialogPolicyRequest,
) -> Result<(), IpcError> {
    browser_set_dialog_policy(&app, manager.inner(), &request.browser_id, &request.policy).await
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
    use crate::browser::agent::{observe, page, redact};
    let state = manager.get_state(&browser_id)?;
    // Both engines run observe.js: a Chromium engine tab in an isolated world over its
    // DevTools API, a system webview tab directly.
    let isolated = page::agent_world_isolated(&browser_id);
    let raw = page::eval_agent(&app, manager, &browser_id, observe::observe_script(isolated)).await?;
    let observation = observe::parse_observation(&raw)?;
    // The record is bound to the generation read before the observe: a navigation in between
    // makes it stale instead of pairing refs with the next document.
    manager.record_observed_refs(
        &browser_id,
        state.generation,
        observation
            .elements
            .iter()
            .map(|element| (element.reference.clone(), element.fingerprint()))
            .collect(),
    )?;

    Ok(BrowserAutomationSnapshot {
        browser_id,
        generation: state.generation,
        // What an agent observes never carries credentials the page shows or the URL holds.
        url: redact::redact_url(&observation.url),
        title: redact::redact_text(&observation.title).into_owned(),
        elements: observation
            .elements
            .into_iter()
            .map(|element| {
                let mut element = element.into_element();
                element.name = redact::redact_text(&element.name).into_owned();
                element
            })
            .collect(),
        truncated: observation.truncated,
        frames: observation
            .frames
            .into_iter()
            .map(|mut frame| {
                frame.title = redact::redact_text(&frame.title).into_owned();
                frame.src = redact::redact_url(&frame.src);
                frame
            })
            .collect(),
        isolated,
    })
}

#[tauri::command]
pub async fn cmd_browser_automation_act<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    request: BrowserAutomationRequest,
) -> Result<crate::browser::model::BrowserActOutcome, IpcError> {
    browser_automation_act(app, manager.inner(), request).await
}

#[cfg(any(target_os = "windows", test))]
fn windows_keypress_capability() -> Result<(), IpcError> {
    Err(IpcError::new(
        crate::ipc::error::IpcErrorCode::Unsupported,
        "Trusted browser keypress automation is unavailable on Windows",
    ))
}

/// Linux has no trusted key injection either: the only remaining path is the
/// synthetic `KeyboardEvent` built by `automation_script`, and an untrusted
/// event (`isTrusted === false`) triggers no default action - no text
/// insertion, no form submit, no scrolling - so the command used to report
/// success while doing nothing. Refuse it instead of lying. A real fix needs
/// the platform's native key injection (`webkit2gtk` `send_key_event`).
#[cfg(any(target_os = "linux", test))]
fn linux_keypress_capability() -> Result<(), IpcError> {
    Err(IpcError::new(
        crate::ipc::error::IpcErrorCode::Unsupported,
        "Trusted browser keypress automation is unavailable on Linux",
    ))
}

pub async fn browser_automation_act<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: &Arc<BrowserManager>,
    request: BrowserAutomationRequest,
) -> Result<crate::browser::model::BrowserActOutcome, IpcError> {
    use crate::browser::agent::{observe, page};
    let isolated = page::agent_world_isolated(&request.browser_id);
    // A postcondition is checked for shape before anything is dispatched.
    let postcondition = request
        .postcondition
        .as_ref()
        .map(|condition| observe::postcondition_script(condition, isolated))
        .transpose()?;
    // The ref must come from this generation's observe (checked before the page is touched);
    // the page then checks the node is the one it named, still attached and unchanged.
    let expect = observe::action_reference(&request.action)
        .map(|reference| manager.observed_ref(&request.browser_id, request.generation, reference))
        .transpose()?;
    // A postcondition the page already shows says nothing about this act: it is sampled
    // first, and such an act reports no `applied` at all instead of a false success.
    let already = match &postcondition {
        Some((script, _)) => {
            let raw = page::eval_agent(&app, manager, &request.browser_id, script.clone()).await?;
            observe::parse_check(&raw)?
        }
        None => false,
    };
    match expect {
        Some(expect) => page::act_element(&app, manager, &request.browser_id, &request.action, &expect).await?,
        None => dispatch_untargeted_act(&app, manager, &request).await?,
    }
    let applied = match postcondition {
        Some(_) if already => None,
        Some((script, timeout_ms)) => Some(
            await_postcondition(
                &app,
                manager,
                &request.browser_id,
                script,
                timeout_ms.map(std::time::Duration::from_millis),
            )
            .await?,
        ),
        None => None,
    };
    Ok(crate::browser::model::BrowserActOutcome { dispatched: true, applied })
}

/// Actions that address no element: a keypress, a window scroll, typing into whatever has
/// focus. They are fenced by the document generation only.
async fn dispatch_untargeted_act<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &Arc<BrowserManager>,
    request: &BrowserAutomationRequest,
) -> Result<(), IpcError> {
    manager.assert_automation_generation(&request.browser_id, request.generation)?;
    // A Chromium engine tab has trusted key input on every platform.
    let engine_tab = crate::browser::agent::page::is_engine_tab(&request.browser_id);
    #[cfg(target_os = "windows")]
    if !engine_tab && matches!(request.action, BrowserAutomationAction::Keypress { .. }) {
        windows_keypress_capability()?;
    }
    #[cfg(target_os = "linux")]
    if !engine_tab && matches!(request.action, BrowserAutomationAction::Keypress { .. }) {
        linux_keypress_capability()?;
    }
    if engine_tab {
        if !crate::browser::agent::page::trusted_act(&request.browser_id, &request.action).await? {
            let script = automation_script(&request.action)?;
            crate::browser::agent::page::eval(app, manager, &request.browser_id, script).await?;
        }
        return Ok(());
    }
    let state = manager.get_state(&request.browser_id)?;
    let webview = app
        .get_webview(&state.webview_label)
        .ok_or_else(|| BrowserError::WebviewNotFound(state.webview_label.clone()))?;

    #[cfg(target_os = "macos")]
    if let BrowserAutomationAction::Keypress { key } = &request.action {
        let keypress = parse_keypress(key)?;
        if dispatch_macos_keypress(&webview, &keypress).await? {
            return Ok(());
        }
    }

    let script = automation_script(&request.action)?;
    let _ = eval_webview(webview, script).await?;
    Ok(())
}

/// Whether the page reaches an act's postcondition before the timeout: `false` on timeout,
/// never an error for a page that simply did not change. Navigations are expected here (a
/// submit loads the next page), so a failed evaluation is retried until the deadline, as
/// `wait` does; only a closed tab ends it early. A page condition has no event to subscribe
/// to, so it is re-checked on a short interval.
async fn await_postcondition<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    browser_id: &str,
    script: String,
    timeout: Option<std::time::Duration>,
) -> Result<bool, IpcError> {
    let deadline = wait_deadline(tokio::time::Instant::now(), timeout);
    let interval = std::time::Duration::from_millis(250);
    loop {
        match crate::browser::agent::page::eval_agent(app, manager, browser_id, script.clone()).await {
            Ok(raw) => match crate::browser::agent::observe::parse_check(&raw) {
                Ok(true) => return Ok(true),
                Ok(false) => {}
                // An invalid selector never matches; say so instead of waiting it out.
                Err(error) => return Err(error.into()),
            },
            Err(BrowserError::NotFound(id)) => return Err(BrowserError::NotFound(id).into()),
            Err(_) => {}
        }
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Ok(false);
        }
        tokio::time::sleep(interval.min(remaining)).await;
    }
}

/// Navigates the popup tab an opener created for one of its handles.
///
/// `window.open('')` followed by `handle.location.href = url` is the common OAuth shape:
/// the first assignment creates the tab, and every later one lands here.
///
/// The URL is page-originated, so it is validated under the Remote context (http(s) and
/// about:blank only): a page must not load local file:// HTML into its popup.
async fn navigate_linked_popup<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &Arc<BrowserManager>,
    opener_browser_id: &str,
    handle: &str,
    url: &str,
) -> Result<(), IpcError> {
    crate::browser::security::validate_url_remote(url)?;
    let Some(popup_browser_id) = crate::browser::popup::popup_for_handle(opener_browser_id, handle)
    else {
        tracing::warn!(
            opener = %opener_browser_id,
            handle = %handle,
            "popup navigation ignored: no open popup is linked to this handle"
        );
        return Ok(());
    };
    navigate_browser_session_with_context(
        app,
        manager,
        &popup_browser_id,
        url,
        crate::browser::security::BrowserSecurityContext::Remote,
    )
    .await
}

/// Closes the popup tab linked to `handle` for `opener_browser_id`.
///
/// If the popup tab is already created and registered, it emits `BROWSER_CLOSE_REQUESTED_EVENT`
/// for the popup tab (never the opener) and closes the popup session.
/// If the popup tab has not completed creation yet, it records a pending close so that
/// creation immediately discards the popup when ready, and marks the opener's handle closed
/// immediately without dropping or losing the request.
async fn close_linked_popup<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &Arc<BrowserManager>,
    opener_browser_id: &str,
    handle: &str,
) {
    if let Some(popup_browser_id) = crate::browser::popup::popup_for_handle(opener_browser_id, handle) {
        if let Err(error) = app.emit(
            crate::browser::guest::BROWSER_CLOSE_REQUESTED_EVENT,
            crate::browser::model::BrowserCloseRequestedPayload {
                browser_id: popup_browser_id.clone(),
            },
        ) {
            tracing::warn!(
                %error,
                popup = %popup_browser_id,
                opener = %opener_browser_id,
                handle = %handle,
                "failed to emit browser close requested event for linked popup"
            );
        }
        if let Err(error) = close_browser_session(app, manager, &popup_browser_id).await {
            // close_browser_session unlinks only after it finds the session; a popup whose
            // session is already gone would otherwise keep a stale handle -> popup link.
            crate::browser::popup::unlink(&popup_browser_id);
            tracing::warn!(
                %error,
                popup = %popup_browser_id,
                opener = %opener_browser_id,
                handle = %handle,
                "failed to close linked popup browser session"
            );
        }
    } else {
        crate::browser::popup::record_pending_close(opener_browser_id, handle);
        let script = crate::browser::popup::popup_closed_script(handle);
        if let Err(error) = eval_browser_session(app, manager, opener_browser_id, &script).await {
            tracing::warn!(
                %error,
                opener = %opener_browser_id,
                handle = %handle,
                "failed to evaluate popup_closed_script on opener for pending close"
            );
        }
    }
}

/// Shown in a refusal warning when the recipient tab has no recorded http origin.
const UNKNOWN_ORIGIN: &str = "(unknown origin)";

/// Delivers an opener's `popupHandle.postMessage(...)` into the child popup tab.
async fn deliver_popup_handle_message<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &Arc<BrowserManager>,
    opener_browser_id: &str,
    handle: &str,
    raw_message: &str,
) {
    let Some(popup_browser_id) = crate::browser::popup::popup_for_handle(opener_browser_id, handle) else {
        warn_popup_page(
            app,
            manager,
            opener_browser_id,
            "this popup handle is not linked to an open popup, so its message was not delivered",
        )
        .await;
        return;
    };
    let Some(message) = crate::browser::popup::parse_opener_message(raw_message) else {
        warn_popup_page(
            app,
            manager,
            opener_browser_id,
            "the popup handle postMessage payload was not valid JSON, so it was not delivered",
        )
        .await;
        return;
    };
    let Some(opener_origin) = manager
        .get_state(opener_browser_id)
        .ok()
        .and_then(|state| crate::browser::popup::origin_of(&state.url))
    else {
        warn_popup_page(
            app,
            manager,
            opener_browser_id,
            "this opener has no http origin, so its message was not delivered",
        )
        .await;
        return;
    };
    let popup_origin = manager
        .get_state(&popup_browser_id)
        .ok()
        .and_then(|state| crate::browser::popup::origin_of(&state.url));
    let (script, target_dest) = match message.target_origin.as_deref() {
        Some(target_origin)
            if !crate::browser::popup::target_origin_permits(
                popup_origin.as_deref(),
                Some(target_origin),
            ) =>
        {
            (
                crate::browser::popup::popup_message_rejected_script(
                    target_origin,
                    popup_origin.as_deref().unwrap_or(UNKNOWN_ORIGIN),
                    &opener_origin,
                ),
                opener_browser_id,
            )
        }
        _ => (
            crate::browser::popup::popup_message_script(&opener_origin, &message.data),
            popup_browser_id.as_str(),
        ),
    };
    if let Err(error) = eval_browser_session(app, manager, target_dest, &script).await {
        tracing::warn!(
            %error,
            popup = %popup_browser_id,
            "delivering an opener message to the popup failed"
        );
    }
}

/// Delivers a popup's `window.opener.postMessage(...)` into the opener tab.
///
/// The origin the opener sees is the popup's recorded URL origin, never the message's own
/// claim, and a `targetOrigin` that does not match the opener is refused with a console
/// warning in the opener rather than dropped in silence.
async fn deliver_popup_opener_message<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &Arc<BrowserManager>,
    popup_browser_id: &str,
    raw_message: &str,
) {
    let Some(link) = crate::browser::popup::lookup(popup_browser_id) else {
        warn_popup_page(
            app,
            manager,
            popup_browser_id,
            "this popup is not linked to an opener, so its message was not delivered",
        )
        .await;
        return;
    };
    let Some(message) = crate::browser::popup::parse_opener_message(raw_message) else {
        warn_popup_page(
            app,
            manager,
            popup_browser_id,
            "the window.opener.postMessage payload was not valid JSON, so it was not delivered",
        )
        .await;
        return;
    };
    let Some(popup_origin) = manager
        .get_state(popup_browser_id)
        .ok()
        .and_then(|state| crate::browser::popup::origin_of(&state.url))
    else {
        warn_popup_page(
            app,
            manager,
            popup_browser_id,
            "this popup has no http origin, so its message was not delivered",
        )
        .await;
        return;
    };
    let opener_origin = manager
        .get_state(&link.opener_browser_id)
        .ok()
        .and_then(|state| crate::browser::popup::origin_of(&state.url));
    let script = match message.target_origin.as_deref() {
        Some(target_origin)
            if !crate::browser::popup::target_origin_permits(
                opener_origin.as_deref(),
                Some(target_origin),
            ) =>
        {
            crate::browser::popup::opener_message_rejected_script(
                target_origin,
                opener_origin.as_deref().unwrap_or(UNKNOWN_ORIGIN),
                &popup_origin,
            )
        }
        _ => crate::browser::popup::opener_message_script(
            &link.handle,
            &popup_origin,
            &message.data,
        ),
    };
    if let Err(error) = eval_browser_session(app, manager, &link.opener_browser_id, &script).await {
        tracing::warn!(
            %error,
            opener = %link.opener_browser_id,
            "delivering a popup message to the opener failed"
        );
    }
}

/// Reports a routing failure inside the page that produced it, so it is visible in that
/// page's console and to `ferryx browser console` instead of vanishing.
async fn warn_popup_page<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &Arc<BrowserManager>,
    popup_browser_id: &str,
    reason: &str,
) {
    tracing::warn!(popup = %popup_browser_id, reason = %reason, "popup message not delivered");
    let script = crate::browser::popup::page_warning_script(reason);
    if let Err(error) = eval_browser_session(app, manager, popup_browser_id, &script).await {
        tracing::debug!(
            %error,
            popup = %popup_browser_id,
            "could not report the routing failure in the popup page"
        );
    }
}

pub async fn close_browser_session<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &Arc<BrowserManager>,
    browser_id: &str,
) -> Result<(), IpcError> {
    let session = manager
        .remove_session(browser_id)
        .ok_or_else(|| BrowserError::NotFound(browser_id.to_string()))?;
    crate::browser::agent::service::tab_closed(browser_id);
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    crate::browser::agent::cef_backend::forget(browser_id);
    crate::browser::bridge::forget_webview(app, &session.webview_label);
    CREATED_SESSION_IDS.lock().retain(|id| id != browser_id);
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    if crate::browser::cef_engine::close(browser_id) {
        crate::browser::popup::forget_opener(browser_id);
        return Ok(());
    }
    // As an opener, this tab's pending closes and popup links can never be consumed again.
    crate::browser::popup::forget_opener(browser_id);
    // The link is removed before any await so a failed notification cannot leave it behind.
    let popup_link = crate::browser::popup::unlink(browser_id);
    // Take the tab off screen before notifying the opener, which can wait up to the eval
    // timeout.
    #[cfg(target_os = "linux")]
    {
        let browser_id_clone = browser_id.to_string();
        let _ = app.run_on_main_thread(move || {
            let _ = crate::browser::linux::implementation::detach_child(&browser_id_clone);
        });
    }
    if let Some(webview) = app.get_webview(&session.webview_label) {
        let _ = webview.close();
    }
    // A closing popup is how the opener learns the popup is gone. Closing the tab -- by the
    // user, by the page, or by the host -- has to flip the handle's `closed`, or a library
    // polling popup.closed never sees the cancel.
    if let Some(link) = popup_link {
        let script = crate::browser::popup::popup_closed_script(&link.handle);
        if let Err(error) =
            eval_browser_session(app, manager, &link.opener_browser_id, &script).await
        {
            tracing::warn!(
                %error,
                opener = %link.opener_browser_id,
                "notifying the opener that its popup closed failed"
            );
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn cmd_browser_close<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
) -> Result<(), IpcError> {
    match close_browser_session(&app, manager.inner(), &browser_id).await {
        Ok(()) => Ok(()),
        Err(IpcError {
            code: crate::ipc::error::IpcErrorCode::BrowserNotFound,
            ..
        }) => Ok(()),
        Err(error) => Err(error),
    }
}

#[tauri::command]
pub async fn cmd_browser_list(
    manager: State<'_, Arc<BrowserManager>>,
) -> Result<Vec<BrowserSessionSummary>, IpcError> {
    Ok(manager.list_sessions())
}

/// Where the engine and its per-tab caches live: the app data dir, or `FERRYX_DATA_DIR` when an
/// isolated (QA or test) instance set it, so such a run never touches the real install.
fn engine_data_dir<R: tauri::Runtime>(app: &AppHandle<R>) -> Result<std::path::PathBuf, IpcError> {
    if let Some(dir) = std::env::var_os("FERRYX_DATA_DIR").filter(|dir| !dir.is_empty()) {
        return Ok(std::path::PathBuf::from(dir));
    }
    app.path()
        .app_data_dir()
        .map_err(|error| IpcError::new(crate::ipc::error::IpcErrorCode::IoError, error.to_string()))
}

fn engine_install_error(error: crate::browser::engine_install::InstallError) -> IpcError {
    use crate::browser::engine_install::InstallError;
    let code = match error {
        InstallError::Unsupported(_) => crate::ipc::error::IpcErrorCode::UnsupportedCapability,
        InstallError::Io(_) => crate::ipc::error::IpcErrorCode::IoError,
        _ => crate::ipc::error::IpcErrorCode::InternalError,
    };
    IpcError::new(code, error.to_string())
}

/// Whether the Chromium engine is installed. New tabs use it once it is.
#[tauri::command]
pub async fn cmd_browser_engine_status<R: tauri::Runtime>(
    app: AppHandle<R>,
) -> Result<crate::browser::engine_install::EngineStatus, IpcError> {
    let data_dir = engine_data_dir(&app)?;
    crate::ipc::run_blocking(move || {
        Ok(crate::browser::engine_install::status(&data_dir, ENGINE_EXECUTABLE))
    })
    .await
}

/// Downloads, verifies and installs the Chromium engine for this platform.
#[tauri::command]
pub async fn cmd_browser_engine_install<R: tauri::Runtime>(
    app: AppHandle<R>,
) -> Result<crate::browser::engine_install::EngineStatus, IpcError> {
    use crate::browser::engine_install;
    let data_dir = engine_data_dir(&app)?;
    let client = reqwest::Client::builder()
        .user_agent(concat!("Ferryx/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| IpcError::internal(error.to_string()))?;
    let bytes = engine_install::fetch(&client).await.map_err(engine_install_error)?;
    crate::ipc::run_blocking(move || {
        let dir = engine_install::unpack(&bytes, &data_dir, ENGINE_EXECUTABLE)
            .map_err(engine_install_error)?;
        #[cfg(target_os = "windows")]
        crate::browser::cef_engine::grant_sandbox_read(&dir).map_err(IpcError::from)?;
        #[cfg(not(target_os = "windows"))]
        let _ = dir;
        Ok(engine_install::status(&data_dir, ENGINE_EXECUTABLE))
    })
    .await
}

/// Removes the installed engine; open Chromium tabs keep running until closed.
#[tauri::command]
pub async fn cmd_browser_engine_remove<R: tauri::Runtime>(
    app: AppHandle<R>,
) -> Result<crate::browser::engine_install::EngineStatus, IpcError> {
    use crate::browser::engine_install;
    let data_dir = engine_data_dir(&app)?;
    crate::ipc::run_blocking(move || {
        let dir = engine_install::install_dir(&data_dir);
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(|error| {
                IpcError::new(crate::ipc::error::IpcErrorCode::IoError, error.to_string())
            })?;
        }
        Ok(engine_install::status(&data_dir, ENGINE_EXECUTABLE))
    })
    .await
}

/// The engine's executable name, also on platforms without a Chromium backend yet.
#[cfg(any(target_os = "windows", target_os = "macos"))]
const ENGINE_EXECUTABLE: &str = crate::browser::cef_engine::ENGINE_EXE;
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
const ENGINE_EXECUTABLE: &str = "ferryx_engine";

/// Where a pane's engine window goes, in the units the platform module places it in: pixels
/// inside the main window on Windows, screen points (top-left origin) on macOS, where the
/// engine window is a separate window over the main one.
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn cef_pane_rect<R: tauri::Runtime>(
    main_window: &tauri::Window<R>,
    bounds: &LogicalRect,
) -> crate::browser::cef_engine::PhysicalRect {
    let scale = main_window.scale_factor().unwrap_or(1.0);
    #[cfg(target_os = "windows")]
    {
        crate::browser::cef_engine::to_physical(bounds, scale)
    }
    #[cfg(target_os = "macos")]
    {
        let origin = main_window
            .inner_position()
            .map(|position| position.to_logical::<f64>(scale))
            .unwrap_or_default();
        crate::browser::cef_engine::to_physical(
            &LogicalRect {
                x: origin.x + bounds.x,
                y: origin.y + bounds.y,
                width: bounds.width,
                height: bounds.height,
            },
            1.0,
        )
    }
}

/// Moves every engine window back over its pane after the main window moved, resized or came
/// to the front. Only macOS needs it: there the engine windows are separate windows, while on
/// Windows they are children that move with the main window.
#[cfg(target_os = "macos")]
pub(crate) fn follow_main_window<R: tauri::Runtime>(
    window: &tauri::Window<R>,
    manager: &BrowserManager,
) {
    for browser_id in crate::browser::cef_engine::tab_ids() {
        if let Ok(Some(bounds)) = manager.get_bounds(&browser_id) {
            crate::browser::cef_engine::set_bounds(&browser_id, cef_pane_rect(window, &bounds));
        }
    }
}

#[cfg(any(target_os = "windows", test))]
#[derive(Debug, PartialEq, Eq)]
enum WindowsOpenRequest<'a> {
    ShellExecute { target: &'a str },
}

#[cfg(any(target_os = "windows", test))]
fn windows_open_request(target: &str) -> WindowsOpenRequest<'_> {
    WindowsOpenRequest::ShellExecute { target }
}

#[cfg(target_os = "windows")]
fn open_windows_target(target: &std::ffi::OsStr) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn ShellExecuteW(
            hwnd: *mut std::ffi::c_void,
            operation: *const u16,
            file: *const u16,
            parameters: *const u16,
            directory: *const u16,
            show: i32,
        ) -> isize;
    }
    let mut wide: Vec<u16> = target.encode_wide().collect();
    if wide.contains(&0) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "open target contains NUL",
        ));
    }
    wide.push(0);
    // SAFETY: all strings are NUL-terminated and live until the synchronous
    // ShellExecuteW call returns. Null optional parameters mean no arguments,
    // no working directory and no owner HWND; the target is never shell syntax.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            [111u16, 112, 101, 110, 0].as_ptr(),
            wide.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
        )
    };
    shell_execute_result(result)
}

#[cfg(any(target_os = "windows", test))]
fn shell_execute_result(result: isize) -> std::io::Result<()> {
    if result <= 32 {
        return Err(std::io::Error::other(format!(
            "ShellExecuteW failed with code {result}"
        )));
    }
    Ok(())
}

pub(crate) fn open_system_target(target: &std::ffi::OsStr) -> Result<(), IpcError> {
    #[cfg(target_os = "windows")]
    {
        open_windows_target(target).map_err(|error| IpcError::internal(error.to_string()))
    }
    #[cfg(not(target_os = "windows"))]
    {
        #[cfg(target_os = "macos")]
        let candidates: &[(&str, &[&str])] = &[("open", &[])];
        // Minimal and sandboxed Linux setups may ship `gio` without `xdg-open`.
        #[cfg(not(target_os = "macos"))]
        let candidates: &[(&str, &[&str])] = &[("xdg-open", &[]), ("gio", &["open"])];
        open_with_first_available(candidates, target)
    }
}

/// Runs the first opener in `candidates` that exists. Only a missing program falls through
/// to the next one; an opener that ran and failed is reported as is.
#[cfg(not(target_os = "windows"))]
fn open_with_first_available(
    candidates: &[(&str, &[&str])],
    target: &std::ffi::OsStr,
) -> Result<(), IpcError> {
    let mut missing = Vec::new();
    for (program, args) in candidates {
        match std::process::Command::new(program).args(*args).arg(target).status() {
            Ok(status) if status.success() => return Ok(()),
            Ok(status) => {
                return Err(IpcError::internal(format!(
                    "system opener {program} exited with {status}"
                )))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(*program);
            }
            Err(error) => return Err(IpcError::internal(error.to_string())),
        }
    }
    Err(IpcError::internal(format!(
        "no system opener is installed (tried {})",
        missing.join(", ")
    )))
}

#[tauri::command]
pub async fn cmd_browser_open_external(url: String) -> Result<(), IpcError> {
    let valid_url = crate::browser::validate_url(&url)?;
    crate::ipc::run_blocking::<(), _>(move || {
        #[cfg(target_os = "windows")]
        let WindowsOpenRequest::ShellExecute { target } = windows_open_request(&valid_url);
        #[cfg(not(target_os = "windows"))]
        let target = valid_url.as_str();
        open_system_target(std::ffi::OsStr::new(target))
    })
    .await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn cmd_open_file_path<R: tauri::Runtime>(
    app: AppHandle<R>,
    daemon_client: tauri::State<'_, std::sync::Arc<crate::daemon::DaemonClient>>,
    path: String,
    cwd: Option<String>,
    session_id: Option<String>,
    workspace_id: Option<String>,
    line: Option<u32>,
    col: Option<u32>,
    editor: Option<String>,
) -> Result<bool, IpcError> {
    open_file_path_admitted(
        Some(daemon_client.inner()),
        path,
        cwd,
        session_id,
        workspace_id,
        line,
        col,
        editor,
        crate::ipc::file_preview_contract::app_remote_files_root(&app),
    )
    .await
}

/// Testable core of `cmd_open_file_path`, independent of Tauri state.
///
/// Resolution order for the terminal working directory:
/// 1. `sessionId` — the live cwd of that local backend session, via the
///    existing daemon cwd cache/describe machinery. Remote sessions are
///    refused here rather than resolved against an unrelated local path.
/// 2. `cwd` — the caller-supplied directory.
/// 3. neither — the path must be absolute or `~`-rooted to resolve.
///
/// Session lookup failures pass through
/// [`crate::ipc::file_preview_contract::admit_session_cwd`]; without a
/// remote-files cache root no remote session may open an absolute path.
pub async fn open_file_path_request(
    daemon_client: Option<&std::sync::Arc<crate::daemon::DaemonClient>>,
    path: String,
    cwd: Option<String>,
    session_id: Option<String>,
    line: Option<u32>,
    col: Option<u32>,
    editor: Option<String>,
) -> Result<bool, IpcError> {
    open_file_path_admitted(daemon_client, path, cwd, session_id, None, line, col, editor, None)
        .await
}

/// [`open_file_path_request`] with the remote-files cache root that remote
/// sessions may open fetched files from, and the caller's workspace id that
/// decides whether a failed session lookup may fall back to the local disk.
#[allow(clippy::too_many_arguments)]
pub async fn open_file_path_admitted(
    daemon_client: Option<&std::sync::Arc<crate::daemon::DaemonClient>>,
    path: String,
    cwd: Option<String>,
    session_id: Option<String>,
    workspace_id: Option<String>,
    line: Option<u32>,
    col: Option<u32>,
    editor: Option<String>,
    remote_files_root: Option<std::path::PathBuf>,
) -> Result<bool, IpcError> {
    let editor = crate::ipc::file_link::EditorTarget::parse(editor.as_deref())?;
    let cwd = admit_file_open(
        daemon_client,
        &path,
        cwd,
        session_id,
        workspace_id,
        editor,
        remote_files_root,
    )
    .await?;

    crate::ipc::run_blocking::<bool, _>(move || {
        crate::ipc::file_link::open_file_link_blocking(&path, cwd.as_deref(), line, col, editor)
    })
    .await
}

/// Admission half of [`open_file_path_admitted`]: resolves the cwd the open may
/// use and refuses OS-opener launches of fetched remote-files cache files that
/// are not passive documents. Never launches anything.
pub async fn admit_file_open(
    daemon_client: Option<&std::sync::Arc<crate::daemon::DaemonClient>>,
    path: &str,
    cwd: Option<String>,
    session_id: Option<String>,
    workspace_id: Option<String>,
    editor: crate::ipc::file_link::EditorTarget,
    remote_files_root: Option<std::path::PathBuf>,
) -> Result<Option<String>, IpcError> {
    let session_id = session_id.filter(|id| !id.is_empty());
    let path = path.to_string();

    let admission = match session_id.as_deref() {
        Some(session_id) => {
            let session = resolve_session_cwd(daemon_client, session_id).await;
            let admit_path = path.clone();
            let fallback_cwd = cwd.clone().map(std::path::PathBuf::from);
            let root = remote_files_root.clone();
            crate::ipc::run_blocking(move || {
                crate::ipc::file_preview_contract::admit_session_cwd_admission(
                    &admit_path,
                    session,
                    workspace_id.as_deref(),
                    fallback_cwd,
                    root.as_deref(),
                )
            })
            .await?
        }
        None => crate::ipc::file_preview_contract::SessionCwdAdmission::Cwd(None),
    };
    let admitted_from_cache =
        admission == crate::ipc::file_preview_contract::SessionCwdAdmission::FetchedRemoteFile;
    let resolved_cwd = admission.cwd();
    let cwd =
        if session_id.is_some() {
            if resolved_cwd.is_none()
                && !crate::ipc::file_link::is_absolute_token(&path)
                && !path.starts_with('~')
            {
                return Err(IpcError::new(crate::ipc::error::IpcErrorCode::Unsupported,
                "The terminal's current directory could not be read. Use an absolute file path."));
            }
            resolved_cwd.map(|cwd| cwd.to_string_lossy().into_owned())
        } else {
            cwd
        };

    let guard_cwd = cwd.clone();
    crate::ipc::run_blocking(move || {
        let Ok(token) = crate::ipc::file_link::sanitize_path_token(&path) else {
            // An invalid token is refused by the launch itself.
            return Ok(());
        };
        let candidate = crate::ipc::file_link::resolve_file_link(
            token,
            guard_cwd.as_deref(),
            crate::ipc::file_link::home_dir(),
        );
        crate::ipc::file_preview_contract::ensure_remote_cache_launchable(
            &candidate,
            editor,
            remote_files_root.as_deref(),
            admitted_from_cache,
        )
    })
    .await?;
    Ok(cwd)
}

/// Live cwd of a local backend terminal session.
///
/// The implementation lives in [`crate::ipc::file_preview_contract`] so the
/// external-open path here and the read-only preview path share exactly one
/// resolution (paired-host refusal, daemon `DescribeSession`, cwd cache
/// refresh). This wrapper keeps the external launch behaviour unchanged.
async fn resolve_session_cwd(
    daemon_client: Option<&std::sync::Arc<crate::daemon::DaemonClient>>,
    session_id: &str,
) -> Result<Option<std::path::PathBuf>, IpcError> {
    crate::ipc::file_preview_contract::resolve_local_session_cwd(daemon_client, session_id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::{windows_open_request, WindowsOpenRequest};

    #[test]
    fn failed_creation_forgets_the_installed_bridge_world() {
        let manager = BrowserManager::new();
        let state = manager
            .register_session(crate::browser::model::CreateBrowserRequest {
                browser_id: None,
                workspace_id: Some("ws-forget".into()),
                worktree_path: None,
                url: "https://example.com".into(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .unwrap();
        record_session_created(&state.browser_id);
        let mut forgotten: Vec<String> = Vec::new();
        discard_failed_creation_with(&manager, &state.browser_id, |label| {
            forgotten.push(label.to_string())
        });
        assert_eq!(forgotten, vec![state.webview_label.clone()]);
        assert!(manager.get_state(&state.browser_id).is_err());
        assert!(!CREATED_SESSION_IDS.lock().iter().any(|id| id == &state.browser_id));
    }

    #[test]
    fn failed_creation_prunes_created_session_id() {
        let manager = BrowserManager::new();
        let state = manager
            .register_session(crate::browser::model::CreateBrowserRequest {
                browser_id: None,
                workspace_id: Some("ws-prune".into()),
                worktree_path: None,
                url: "https://example.com".into(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .unwrap();
        record_session_created(&state.browser_id);
        discard_failed_session(&manager, &state.browser_id);
        assert!(manager.get_state(&state.browser_id).is_err());
        assert!(
            !CREATED_SESSION_IDS
                .lock()
                .iter()
                .any(|id| id == &state.browser_id),
            "a session whose creation failed must not stay in CREATED_SESSION_IDS"
        );
    }

    #[test]
    fn session_removed_during_install_discards_the_webview() {
        let manager = BrowserManager::new();
        let state = manager
            .register_session(crate::browser::model::CreateBrowserRequest {
                browser_id: None,
                workspace_id: Some("ws-install-race".into()),
                worktree_path: None,
                url: "https://example.com".into(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .unwrap();
        let mut discarded = 0;
        assert!(discard_if_abandoned_after_install(
            || manager.get_state(&state.browser_id).is_ok(),
            || discarded += 1,
        )
        .is_ok());
        assert_eq!(discarded, 0, "a live session keeps its webview");
        // The timed-out async side removes the session while install runs.
        manager.remove_session(&state.browser_id);
        let outcome = discard_if_abandoned_after_install(
            || manager.get_state(&state.browser_id).is_ok(),
            || discarded += 1,
        );
        assert!(outcome.unwrap_err().contains("abandoned"));
        assert_eq!(discarded, 1, "an abandoned session must close and forget its webview");
    }

    #[test]
    fn timed_out_creation_queues_a_close_of_its_webview_before_discarding() {
        let calls = std::cell::RefCell::new(Vec::<String>::new());
        discard_timed_out_creation_with(
            "browser-label-1",
            "browser-id-1",
            || calls.borrow_mut().push("discard".into()),
            |label, browser_id| {
                calls.borrow_mut().push(format!("close {label} {browser_id}"))
            },
        );
        assert_eq!(
            calls.into_inner(),
            vec!["close browser-label-1 browser-id-1".to_string(), "discard".to_string()],
            "a timed-out creation must queue its webview close before it drops the session"
        );
    }

    // The seam tests above cannot reach the main-thread creation path, so pin its wiring
    // in source: every error exit after the queue discards the installed bridge, the
    // timeout exit also closes the webview, and the closure re-checks the session after
    // install and, when it is gone, closes the child and forgets its bridge state.
    #[test]
    fn creation_error_exits_discard_the_installed_bridge() {
        // Normalise line endings: a Windows checkout with core.autocrlf has CRLF sources.
        let source = include_str!("browser.rs").replace("\r\n", "\n");
        let source = source.as_str();
        let tests_at = source.find(&["mod ", "tests {"].concat()).unwrap();
        let start = source.find("match run_result {").unwrap();
        let end = start + source[start..].find("if let Some(link) = opener.as_ref()").unwrap();
        assert!(end < tests_at);
        let exits: Vec<&str> = source[start..end]
            .split("return Err(BrowserError::CreateFailed(")
            .collect();
        assert_eq!(
            exits.len(),
            5,
            "expected three CreateFailed exits after the queue and one for a missing main window"
        );
        let discards = [
            "discard_failed_creation(app, manager, &state.browser_id);",
            "discard_timed_out_creation(app, manager, &state);",
            "discard_failed_creation(app, manager, &state.browser_id);",
            "discard_failed_session(manager, &state.browser_id);",
        ];
        for (before, discard) in exits[..4].iter().zip(discards) {
            assert!(
                before.trim_end().ends_with(discard),
                "a creation error exit must call {discard} first"
            );
        }
        let closure = &source[source.find("let bridge_kind =").unwrap()..start];
        let install = closure.find("set_trusted_bridge(&browser_id, bridge_kind)").unwrap();
        let recheck = closure.find("discard_if_abandoned_after_install(").unwrap();
        assert!(install < recheck, "the session must be re-checked after the bridge install");
        let discard_body = &closure[recheck..];
        let discard_body = &discard_body[..discard_body.find("break 'kept Err(message);").unwrap()];
        for call in [
            "let _ = child.close();",
            "crate::browser::bridge::forget_webview(child.app_handle(), &label);",
            "#[cfg(target_os = \"windows\")]\n                                    crate::browser::bridge::windows::forget_webview(&label);",
        ] {
            assert!(
                discard_body.contains(call),
                "the post-install discard must run: {call}"
            );
        }
        // The timeout exit hands the close to the main thread, and the close releases the
        // Linux overlay entry and Windows bridge world before closing the webview.
        let fn_body = |name: &str| {
            let at = source.find(&["fn ", name, "<R: tauri::Runtime>("].concat()).unwrap();
            assert!(at < tests_at);
            let body = &source[at..];
            &body[..body.find("\n}\n").unwrap()]
        };
        let timed_out = fn_body("discard_timed_out_creation");
        let hop = timed_out.find("app.run_on_main_thread(move || {").unwrap();
        let hop_body = &timed_out[hop..];
        let hop_body = &hop_body[..hop_body.find("});").unwrap()];
        assert!(
            hop_body.contains("close_abandoned_webview(&handle, &webview_label, &browser_id);"),
            "the timed-out close must run close_abandoned_webview inside run_on_main_thread"
        );
        let close = fn_body("close_abandoned_webview");
        let mut last = 0;
        for call in [
            "#[cfg(target_os = \"linux\")]\n    {\n        let _ = crate::browser::linux::implementation::detach_child(browser_id);",
            "#[cfg(target_os = \"windows\")]\n    crate::browser::bridge::windows::forget_webview(webview_label);",
            "if let Some(webview) = app.get_webview(webview_label) {\n        let _ = webview.close();",
        ] {
            let at = close.find(call).unwrap_or_else(|| panic!("close_abandoned_webview must run: {call}"));
            assert!(at >= last, "close_abandoned_webview must release before it closes: {call}");
            last = at;
        }
    }

    #[test]
    fn external_open_propagates_failure() {
        for failure in 0..=32 {
            assert!(super::shell_execute_result(failure).is_err());
        }
        assert!(super::shell_execute_result(33).is_ok());
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn system_opener_falls_back_when_the_first_program_is_missing() {
        let target = std::ffi::OsStr::new("https://example.test/");
        let missing = "ferryx-test-opener-that-does-not-exist";
        let no_args: &[&str] = &[];
        let falls_back: &[(&str, &[&str])] = &[(missing, no_args), ("true", no_args)];
        let failed_first: &[(&str, &[&str])] = &[("false", no_args), ("true", no_args)];
        let only_missing: &[(&str, &[&str])] = &[(missing, no_args)];
        assert!(super::open_with_first_available(falls_back, target).is_ok());
        // A program that ran and failed is reported, never skipped for the next one.
        assert!(super::open_with_first_available(failed_first, target).is_err());
        assert!(super::open_with_first_available(only_missing, target).is_err());
    }

    #[test]
    fn windows_keypress_returns_typed_unsupported() {
        let error =
            super::windows_keypress_capability().expect_err("no trusted Windows input adapter");
        assert_eq!(error.code, crate::ipc::error::IpcErrorCode::Unsupported);
    }

    #[test]
    fn linux_keypress_returns_typed_unsupported() {
        let error =
            super::linux_keypress_capability().expect_err("no trusted Linux input adapter");
        assert_eq!(error.code, crate::ipc::error::IpcErrorCode::Unsupported);
    }

    #[test]
    fn webview2_private_profile_minimum_matches_the_documented_runtime() {
        use super::webview2_version_is_below_private_profile_minimum as below_minimum;

        assert!(below_minimum("100.0.1185.39"));
        assert!(below_minimum("101.0.1210.38"));
        assert!(!below_minimum("101.0.1210.39"));
        assert!(!below_minimum("120.0.2210.91"));
        // A version that cannot be parsed cannot be confirmed as new enough.
        assert!(below_minimum("unknown"));
    }

    #[test]
    fn external_open_preserves_target_without_shell() {
        for target in [
            "https://example.test/?a=1&b=2",
            "C:\\QA\\two words & notes.txt",
        ] {
            assert_eq!(
                windows_open_request(target),
                WindowsOpenRequest::ShellExecute { target }
            );
        }
    }

    #[test]
    fn file_link_expands_bare_home() {
        use crate::ipc::file_link::resolve_file_link;
        let home = std::path::PathBuf::from("C:/Users/P13 fixture");
        assert_eq!(
            resolve_file_link("~", Some("C:/work"), Some(home.clone())),
            home
        );
    }

    #[test]
    fn file_link_expands_profile_relative_path() {
        use crate::ipc::file_link::resolve_file_link;
        let home = std::path::PathBuf::from("C:/Users/P13 fixture");
        assert_eq!(
            resolve_file_link("~/two words & notes.txt", None, Some(home.clone())),
            home.join("two words & notes.txt")
        );
    }

    #[test]
    fn keep_or_discard_fresh_webview_covers_both_branches() {
        use super::keep_or_discard_fresh_webview;

        // Session still present: keep the webview, no error.
        assert!(keep_or_discard_fresh_webview(true).is_ok());

        // Session removed by the timed-out async side: discard, with an
        // error describing the abandoned creation.
        let discarded = keep_or_discard_fresh_webview(false);
        assert!(discarded.is_err());
        assert!(discarded.unwrap_err().contains("session was abandoned"));
    }

    #[tokio::test]
    async fn test_cmd_open_file_path_rejects_nonexistent_file() {
        let error = super::open_file_path_request(
            None,
            "/nonexistent/file/path/that/does/not/exist.txt".to_string(),
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .expect_err("missing paths must error, never report a silent false");
        assert_eq!(error.code, crate::ipc::error::IpcErrorCode::InvalidPath);
    }

    #[test]
    fn test_cmd_open_file_path_resolves_relative_with_cwd() {
        use crate::ipc::file_link::resolve_file_link;
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        // Test the production resolver, not the user's default application.
        let resolved = resolve_file_link("Cargo.toml", Some(manifest_dir), None);
        assert_eq!(
            resolved,
            std::path::Path::new(manifest_dir).join("Cargo.toml")
        );
        assert!(resolved.is_file());
    }

    #[test]
    fn wait_deadline_clamps_huge_timeouts_without_overflow() {
        let now = tokio::time::Instant::now();
        assert_eq!(wait_deadline(now, Some(std::time::Duration::MAX)), now + MAX_WAIT_TIMEOUT);
        assert_eq!(
            wait_deadline(now, Some(std::time::Duration::from_millis(u64::MAX))),
            now + MAX_WAIT_TIMEOUT
        );
        assert_eq!(wait_deadline(now, None), now + DEFAULT_WAIT_TIMEOUT);
        assert_eq!(
            wait_deadline(now, Some(std::time::Duration::from_secs(2))),
            now + std::time::Duration::from_secs(2)
        );
    }

    #[test]
    fn test_build_wait_condition_script() {
        use crate::browser::model::BrowserWaitCondition;

        let s = build_wait_condition_script(&BrowserWaitCondition::Selector {
            selector: "#my-id".into(),
        });
        assert!(s.contains("document.querySelector"));

        let t = build_wait_condition_script(&BrowserWaitCondition::Text {
            text: "Loaded".into(),
        });
        assert!(t.contains("innerText.includes"));

        let u = build_wait_condition_script(&BrowserWaitCondition::UrlContains {
            fragment: "/done".into(),
        });
        assert!(u.contains("location.href.includes"));

        let lc = build_wait_condition_script(&BrowserWaitCondition::LoadState {
            state: "complete".into(),
        });
        assert!(lc.contains("document.readyState"));

        let li = build_wait_condition_script(&BrowserWaitCondition::LoadState {
            state: "interactive".into(),
        });
        assert!(li.contains("interactive"));
        assert!(li.contains("complete"));

        let f = build_wait_condition_script(&BrowserWaitCondition::Function {
            script: "1 + 1 === 2".into(),
        });
        assert!(f.contains("eval"));
    }

    #[test]
    fn test_build_wait_condition_script_load_and_domcontentloaded_states() {
        // RED mutation: omit mapping "load" to "complete" or "domcontentloaded" to interactive-or-complete,
        // leaving them to fall through to literal document.readyState === "load" / "domcontentloaded".
        use crate::browser::model::BrowserWaitCondition;

        let load_script = build_wait_condition_script(&BrowserWaitCondition::LoadState {
            state: "load".into(),
        });
        assert!(
            load_script.contains(r#"document.readyState === "complete""#),
            r#"script for "load" must check document.readyState === "complete", got: {load_script}"#
        );
        assert!(
            !load_script.contains(r#"document.readyState === "load""#),
            r#"script for "load" must never check impossible document.readyState === "load""#
        );

        let dcl_script = build_wait_condition_script(&BrowserWaitCondition::LoadState {
            state: "domcontentloaded".into(),
        });
        assert!(
            dcl_script.contains(r#"document.readyState === "interactive" || document.readyState === "complete""#),
            r#"script for "domcontentloaded" must check interactive or complete, got: {dcl_script}"#
        );
        assert!(
            !dcl_script.contains(r#"document.readyState === "domcontentloaded""#),
            r#"script for "domcontentloaded" must never check impossible document.readyState === "domcontentloaded""#
        );

        let upper_load = build_wait_condition_script(&BrowserWaitCondition::LoadState {
            state: "LOAD".into(),
        });
        assert!(upper_load.contains(r#"document.readyState === "complete""#));

        let upper_dcl = build_wait_condition_script(&BrowserWaitCondition::LoadState {
            state: "DOMContentLoaded".into(),
        });
        assert!(upper_dcl.contains(r#"document.readyState === "interactive" || document.readyState === "complete""#));
    }

    #[test]
    fn test_truncate_eval_result() {
        let short = "small string".to_string();
        let (res, truncated) = truncate_eval_result(short.clone());
        assert_eq!(res, short);
        assert!(!truncated);

        // Unicode multibyte character repeat: 30_000 3-byte characters = 90_000 bytes
        let long = "中".repeat(30_000);
        let (res, truncated) = truncate_eval_result(long);
        assert!(truncated);
        assert!(res.len() <= 65536);
        assert!(std::str::from_utf8(res.as_bytes()).is_ok());
    }

    #[test]
    fn test_cookie_parse_and_script() {
        let raw = "a=1; b=2; c=3";
        let parsed = parse_document_cookie(raw);
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].name, "a");
        assert_eq!(parsed[0].value, "1");

        let get_script = build_cookie_script("get", None, None, None, None).unwrap();
        assert!(get_script.contains("document.cookie"));

        let set_script =
            build_cookie_script("set", Some("k"), Some("v"), Some("example.com"), Some("/"))
                .unwrap();
        assert!(set_script.contains("document.cookie ="));
        assert!(set_script.contains("k=v"));
        assert!(set_script.contains("domain=example.com"));

        let clear_script = build_cookie_script("clear", Some("k"), None, None, None).unwrap();
        assert!(clear_script.contains("Max-Age=0") || clear_script.contains("expires="));
    }

    #[test]
    fn cookie_script_rejects_attribute_injection() {
        assert!(build_cookie_script("set", Some("k"), Some("v; domain=.evil.test"), None, None)
            .is_err());
        assert!(build_cookie_script("set", Some("k; path=/"), Some("v"), None, None).is_err());
        assert!(build_cookie_script("set", Some("k"), Some("v\r\nX"), None, None).is_err());
        assert!(build_cookie_script("set", Some("k"), Some("v"), Some("a.com; secure"), None)
            .is_err());
        assert!(build_cookie_script("clear", Some("k"), None, None, Some("/; domain=x")).is_err());
        assert!(build_cookie_script("set", Some("k"), Some("a=b"), None, None).is_ok());
    }

    #[test]
    fn cookie_get_filter_ignores_action_case() {
        let entries = parse_document_cookie(r#""a=1; b=2""#);
        assert_eq!(entries.len(), 2);
        let filtered = filter_cookie_entries("GET", Some("b"), entries.clone());
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "b");
        assert_eq!(filter_cookie_entries("set", Some("b"), entries).len(), 2);
    }

    #[test]
    fn test_storage_script() {
        let script = |kind, action, key, value| {
            build_storage_script_extended(kind, action, key, value, false, None, None)
        };
        let get_s = script("local", "get", Some("key1"), None).unwrap();
        assert!(get_s.contains("localStorage.getItem"));

        let set_s = script("session", "set", Some("key2"), Some("val2")).unwrap();
        assert!(set_s.contains("sessionStorage.setItem"));

        let clear_k = script("local", "clear", Some("key1"), None).unwrap();
        assert!(clear_k.contains("localStorage.removeItem"));

        let clear_all = script("local", "clear", None, None).unwrap();
        assert!(clear_all.contains("localStorage.clear"));
    }

    #[test]
    fn test_storage_script_extended_all_and_scope() {
        let clear_all = build_storage_script_extended("local", "clear", None, None, true, None, None).unwrap();
        assert!(clear_all.contains("localStorage.clear"));

        let clear_domain = build_storage_script_extended(
            "local", "clear", None, None, false, None, Some("example.com")
        ).unwrap();
        assert!(
            !clear_domain.contains("location.hostname.includes"),
            "a domain scope must not match by substring"
        );
        assert!(clear_domain.contains(r#"h === d || h.endsWith("." + d)"#));
        assert!(clear_domain.contains(r#""example.com""#));
        let dotted = build_storage_script_extended(
            "local", "clear", None, None, false, None, Some(".Example.COM")
        ).unwrap();
        assert!(dotted.contains(r#""example.com""#));

        let clear_url = build_storage_script_extended(
            "session", "clear", None, None, false, Some("https://example.com/app"), None
        ).unwrap();
        assert!(clear_url.contains("location.href.includes"));
        assert!(clear_url.contains("sessionStorage.clear"));
    }

    #[test]
    fn test_storage_script_extended_rejects_key_with_all() {
        let err = build_storage_script_extended(
            "local", "clear", Some("mykey"), None, true, None, None
        ).expect_err("cannot specify both key and --all");
        assert!(err.contains("cannot specify both key and --all"));
    }

    #[test]
    fn test_browser_persistent_state_round_trip() {
        let mut local = std::collections::BTreeMap::new();
        local.insert("auth_token".to_string(), "xyz123".to_string());
        local.insert("theme".to_string(), "dark".to_string());

        let mut session = std::collections::BTreeMap::new();
        session.insert("draft_id".to_string(), "d-99".to_string());

        let state = BrowserPersistentState {
            version: 2,
            browser_id: "browser-test-1".into(),
            url: "https://example.com/dashboard".into(),
            cookies: vec![crate::browser::state_cookies::PersistedCookie {
                name: "session".into(),
                value: "abc".into(),
                domain: Some("example.com".into()),
                path: Some("/".into()),
                expires_unix: Some(1_900_000_000),
                secure: true,
                http_only: true,
                same_site: Some("lax".into()),
            }],
            local_storage: local,
            session_storage: session,
        };

        let json = serde_json::to_string(&state).unwrap();
        let parsed: BrowserPersistentState = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, state);
    }

    #[test]
    fn test_browser_persistent_state_backward_compatibility() {
        let old_json = r#"{
            "version": 1,
            "browserId": "browser-legacy-1",
            "url": "https://example.com/login",
            "cookies": [
                { "name": "legacy_sid", "value": "old-val-123" }
            ],
            "localStorage": { "pref": "light" },
            "sessionStorage": {}
        }"#;

        let parsed: BrowserPersistentState = serde_json::from_str(old_json).unwrap();
        assert_eq!(parsed.version, 1);
        assert_eq!(parsed.browser_id, "browser-legacy-1");
        assert_eq!(parsed.cookies.len(), 1);
        assert_eq!(parsed.cookies[0].name, "legacy_sid");
        assert_eq!(parsed.cookies[0].value, "old-val-123");
        assert_eq!(parsed.cookies[0].path.as_deref(), Some("/"));
        assert!(!parsed.cookies[0].secure);
        assert!(!parsed.cookies[0].http_only);
    }

    #[test]
    fn test_persisted_cookie_from_to_cookie_preserves_httponly_and_attributes() {
        use cookie::{Cookie, SameSite};
        use time::OffsetDateTime;

        let original = Cookie::build(("session_token", "secret123"))
            .domain("auth.example.com")
            .path("/auth")
            .secure(true)
            .http_only(true)
            .same_site(SameSite::Strict)
            .expires(OffsetDateTime::from_unix_timestamp(1_893_456_000).unwrap())
            .build();

        let persisted = crate::browser::state_cookies::PersistedCookie::from_cookie(&original);
        assert_eq!(persisted.name, "session_token");
        assert_eq!(persisted.value, "secret123");
        assert_eq!(persisted.domain.as_deref(), Some("auth.example.com"));
        assert_eq!(persisted.path.as_deref(), Some("/auth"));
        assert!(persisted.secure);
        assert!(persisted.http_only);
        assert_eq!(persisted.same_site.as_deref(), Some("strict"));
        assert_eq!(persisted.expires_unix, Some(1_893_456_000));

        let restored = persisted.to_cookie().unwrap();
        assert_eq!(restored.name(), "session_token");
        assert_eq!(restored.value(), "secret123");
        assert_eq!(restored.domain(), Some("auth.example.com"));
        assert_eq!(restored.path(), Some("/auth"));
        assert_eq!(restored.secure(), Some(true));
        assert_eq!(restored.http_only(), Some(true));
        assert_eq!(restored.same_site(), Some(SameSite::Strict));
    }

    #[test]
    fn test_browser_dump_state_script_contains_no_document_cookie() {
        assert!(!BROWSER_DUMP_STATE_SCRIPT.contains("document.cookie"));
    }

    #[test]
    fn test_save_browser_state_fails_on_unwritable_parent() {
        // A regular file cannot be a parent directory on any OS, unlike a root-level path,
        // which Windows resolves to the current drive and happily creates.
        let dir = tempfile::tempdir().expect("tempdir");
        let blocker = dir.path().join("blocker");
        std::fs::write(&blocker, b"x").expect("write blocker file");
        let fake_path = blocker.join("nested").join("state.json");
        let err = write_browser_state_file(&fake_path, b"{}")
            .expect_err("creating a directory under a regular file must fail");
        assert_eq!(err.code, IpcErrorCode::InternalError);
        assert!(err.message.contains("failed to create directory"));
    }

    #[cfg(unix)]
    #[test]
    fn saved_browser_state_file_is_owner_only_and_replaces_existing() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("state.json");
        std::fs::write(&path, b"old").expect("seed existing state");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        write_browser_state_file(&path, b"{\"v\":2}").expect("write state");
        assert_eq!(std::fs::read(&path).unwrap(), b"{\"v\":2}");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "state file holds session cookies and must be owner-only");
    }

    #[test]
    fn storage_restore_requires_the_saved_origin() {
        assert!(same_web_origin("https://a.com/x", "https://a.com/y?q=1"));
        assert!(!same_web_origin("https://b.com/", "https://a.com/"));
        assert!(!same_web_origin("http://a.com/", "https://a.com/"));
        assert!(!same_web_origin("https://a.com:8443/", "https://a.com/"));
        assert!(!same_web_origin("about:blank", "about:blank"));
        assert!(!same_web_origin("not a url", "not a url"));
    }

    #[test]
    fn test_old_v1_load_domain_fallback() {
        let old_json = r#"{
            "version": 1,
            "browserId": "browser-legacy-1",
            "url": "https://auth.example.com:8443/login?redirect=1",
            "cookies": [
                { "name": "legacy_sid", "value": "old-val-123" }
            ],
            "localStorage": {},
            "sessionStorage": {}
        }"#;

        let parsed: BrowserPersistentState = serde_json::from_str(old_json).unwrap();
        let restored = prepare_restored_cookies(&parsed).expect("legacy v1 cookie restoration must succeed");
        assert_eq!(restored.len(), 1);
        let cookie = &restored[0];
        assert_eq!(cookie.name(), "legacy_sid");
        assert_eq!(cookie.value(), "old-val-123");
        assert_eq!(cookie.domain(), Some("auth.example.com"));
        assert_eq!(cookie.path(), Some("/"));
        assert_eq!(cookie.http_only(), Some(false));
        assert_eq!(cookie.secure(), Some(false));
    }

    #[test]
    fn test_old_v1_load_malformed_url_refusal() {
        let malformed_states = [
            r#"{
                "version": 1,
                "browserId": "b1",
                "url": "not-a-valid-url",
                "cookies": [{ "name": "c1", "value": "v1" }]
            }"#,
            r#"{
                "version": 1,
                "browserId": "b2",
                "url": "javascript:alert(1)",
                "cookies": [{ "name": "c2", "value": "v2" }]
            }"#,
            r#"{
                "version": 1,
                "browserId": "b3",
                "url": "",
                "cookies": [{ "name": "c3", "value": "v3" }]
            }"#,
        ];

        for raw_json in malformed_states {
            let state: BrowserPersistentState = serde_json::from_str(raw_json).unwrap();
            let err = prepare_restored_cookies(&state)
                .expect_err("cookie restoration with domainless cookie and malformed URL must fail");
            assert_eq!(err.code, IpcErrorCode::InternalError);
        }
    }

    #[test]
    fn test_old_v1_load_preserves_explicit_domain_and_attributes() {
        let json = r#"{
            "version": 1,
            "browserId": "b1",
            "url": "https://example.com/login",
            "cookies": [
                {
                    "name": "explicit_cookie",
                    "value": "v_explicit",
                    "domain": "api.different.com",
                    "path": "/v1",
                    "secure": true,
                    "httpOnly": true,
                    "sameSite": "strict"
                }
            ]
        }"#;

        let state: BrowserPersistentState = serde_json::from_str(json).unwrap();
        let restored = prepare_restored_cookies(&state).unwrap();
        assert_eq!(restored.len(), 1);
        let cookie = &restored[0];
        assert_eq!(cookie.name(), "explicit_cookie");
        assert_eq!(cookie.domain(), Some("api.different.com"));
        assert_eq!(cookie.path(), Some("/v1"));
        assert_eq!(cookie.secure(), Some(true));
        assert_eq!(cookie.http_only(), Some(true));
        assert_eq!(cookie.same_site(), Some(cookie::SameSite::Strict));
    }

    #[test]
    fn test_browser_highlight_script_structure() {
        let script = crate::browser::picker::browser_highlight_script(".target-class", 1800).unwrap();
        assert!(script.contains(".target-class"));
        assert!(script.contains("1800"));
        assert!(script.contains("__ferryx_highlight_overlay"));
    }
}

pub fn build_wait_condition_script(
    condition: &crate::browser::model::BrowserWaitCondition,
) -> String {
    use crate::browser::model::BrowserWaitCondition;
    match condition {
        BrowserWaitCondition::Selector { selector } => {
            let sel_json = serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".into());
            format!(
                r#"(() => {{ try {{ return document.querySelector({sel_json}) !== null; }} catch (_) {{ return false; }} }})()"#
            )
        }
        BrowserWaitCondition::Text { text } => {
            let text_json = serde_json::to_string(text).unwrap_or_else(|_| "\"\"".into());
            format!(
                r#"(() => {{ try {{ return Boolean(document.body && document.body.innerText.includes({text_json})); }} catch (_) {{ return false; }} }})()"#
            )
        }
        BrowserWaitCondition::UrlContains { fragment } => {
            let frag_json = serde_json::to_string(fragment).unwrap_or_else(|_| "\"\"".into());
            format!(
                r#"(() => {{ try {{ return location.href.includes({frag_json}); }} catch (_) {{ return false; }} }})()"#
            )
        }
        BrowserWaitCondition::LoadState { state } => {
            if state.eq_ignore_ascii_case("interactive")
                || state.eq_ignore_ascii_case("domcontentloaded")
            {
                r#"(() => { return document.readyState === "interactive" || document.readyState === "complete"; })()"#.into()
            } else if state.eq_ignore_ascii_case("complete")
                || state.eq_ignore_ascii_case("load")
            {
                r#"(() => { return document.readyState === "complete"; })()"#.into()
            } else {
                let state_json = serde_json::to_string(state).unwrap_or_else(|_| "\"\"".into());
                format!(r#"(() => {{ return document.readyState === {state_json}; }})()"#)
            }
        }
        BrowserWaitCondition::Function { script } => {
            let script_json = serde_json::to_string(script).unwrap_or_else(|_| "\"\"".into());
            format!(
                r#"(() => {{ try {{ return Boolean(eval({script_json})); }} catch (_) {{ return false; }} }})()"#
            )
        }
        BrowserWaitCondition::WithTimeout { inner, .. } => build_wait_condition_script(inner),
    }
}

pub fn parse_eval_boolean(raw: &str) -> bool {
    let trimmed = raw.trim();
    if trimmed == "true" || trimmed == "\"true\"" {
        return true;
    }
    if let Ok(b) = serde_json::from_str::<bool>(trimmed) {
        return b;
    }
    if let Ok(s) = serde_json::from_str::<String>(trimmed) {
        return s == "true";
    }
    false
}

const MAX_EVAL_BYTES: usize = 65536;

pub fn truncate_eval_result(result: String) -> (String, bool) {
    if result.len() <= MAX_EVAL_BYTES {
        (result, false)
    } else {
        let mut end = MAX_EVAL_BYTES;
        while end > 0 && !result.is_char_boundary(end) {
            end -= 1;
        }
        (result[..end].to_string(), true)
    }
}

pub fn parse_document_cookie(raw: &str) -> Vec<crate::browser::model::BrowserCookieEntry> {
    use crate::browser::model::BrowserCookieEntry;
    let unquoted = if let Ok(s) = serde_json::from_str::<String>(raw) {
        s
    } else {
        raw.to_string()
    };
    let mut entries = Vec::new();
    for part in unquoted.split(';') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some((name, val)) = trimmed.split_once('=') {
            entries.push(BrowserCookieEntry {
                name: name.trim().to_string(),
                value: val.trim().to_string(),
            });
        }
    }
    entries
}

pub fn build_cookie_script(
    action: &str,
    name: Option<&str>,
    value: Option<&str>,
    domain: Option<&str>,
    path: Option<&str>,
) -> Result<String, String> {
    // Every part is spliced into one `document.cookie` assignment, where `;` starts a new
    // attribute: a value like `v; domain=.x` would otherwise set attributes of its own.
    fn check_cookie_part(label: &str, part: &str, extra: &[char]) -> Result<(), String> {
        match part.chars().find(|c| *c == ';' || c.is_control() || extra.contains(c)) {
            Some(c) => Err(format!("cookie {label} contains a forbidden character {c:?}")),
            None => Ok(()),
        }
    }
    if let Some(name) = name {
        check_cookie_part("name", name, &['='])?;
    }
    if let Some(value) = value {
        check_cookie_part("value", value, &[])?;
    }
    if let Some(domain) = domain {
        check_cookie_part("domain", domain, &[])?;
    }
    if let Some(path) = path {
        check_cookie_part("path", path, &[])?;
    }
    match action.to_ascii_lowercase().as_str() {
        "get" | "list" => Ok("(() => { return document.cookie; })()".to_string()),
        "set" => {
            let name = name.ok_or_else(|| "cookie name is required for set".to_string())?;
            let value = value.unwrap_or("");
            let mut cookie_str = format!("{name}={value}");
            if let Some(d) = domain {
                cookie_str.push_str(&format!("; domain={d}"));
            }
            if let Some(p) = path {
                cookie_str.push_str(&format!("; path={p}"));
            } else {
                cookie_str.push_str("; path=/");
            }
            let encoded = serde_json::to_string(&cookie_str).map_err(|e| e.to_string())?;
            Ok(format!(
                "(() => {{ document.cookie = {encoded}; return document.cookie; }})()"
            ))
        }
        "clear" | "delete" => {
            let name = name.ok_or_else(|| "cookie name is required for clear".to_string())?;
            let mut cookie_str =
                format!("{name}=; Max-Age=0; expires=Thu, 01 Jan 1970 00:00:00 GMT");
            if let Some(d) = domain {
                cookie_str.push_str(&format!("; domain={d}"));
            }
            if let Some(p) = path {
                cookie_str.push_str(&format!("; path={p}"));
            } else {
                cookie_str.push_str("; path=/");
            }
            let encoded = serde_json::to_string(&cookie_str).map_err(|e| e.to_string())?;
            Ok(format!(
                "(() => {{ document.cookie = {encoded}; return document.cookie; }})()"
            ))
        }
        other => Err(format!("unknown cookie action: {other}")),
    }
}

pub fn build_storage_script_extended(
    kind: &str,
    action: &str,
    key: Option<&str>,
    value: Option<&str>,
    clear_all: bool,
    url: Option<&str>,
    domain: Option<&str>,
) -> Result<String, String> {
    let storage_obj = match kind.to_ascii_lowercase().as_str() {
        "local" | "localstorage" => "localStorage",
        "session" | "sessionstorage" => "sessionStorage",
        _ => return Err(format!("invalid storage kind: {kind}")),
    };
    match action.to_ascii_lowercase().as_str() {
        "get" => {
            if let Some(key_str) = key {
                let key_json = serde_json::to_string(key_str).map_err(|e| e.to_string())?;
                Ok(format!(
                    "(() => {{ return {storage_obj}.getItem({key_json}); }})()"
                ))
            } else {
                Ok(format!(
                    r#"(() => {{
  const res = {{}};
  for (let i = 0; i < {storage_obj}.length; i++) {{
    const k = {storage_obj}.key(i);
    if (k !== null) res[k] = {storage_obj}.getItem(k);
  }}
  return JSON.stringify(res);
}})()"#
                ))
            }
        }
        "set" => {
            let key_str = key.ok_or_else(|| "key is required for storage set".to_string())?;
            let val_str = value.unwrap_or("");
            let key_json = serde_json::to_string(key_str).map_err(|e| e.to_string())?;
            let val_json = serde_json::to_string(val_str).map_err(|e| e.to_string())?;
            Ok(format!("(() => {{ {storage_obj}.setItem({key_json}, {val_json}); return {storage_obj}.getItem({key_json}); }})()"))
        }
        "clear" | "delete" | "remove" => {
            if clear_all && key.is_some() {
                return Err("cannot specify both key and --all for storage clear".to_string());
            }
            let u_filter = url.map(|u| serde_json::to_string(u).unwrap_or_else(|_| "\"\"".into()));
            // A domain scope matches the host itself or a subdomain of it, never any host that
            // merely contains the text (`example.com` must not match `notexample.com`).
            let d_filter = domain.map(|d| {
                let normalized = d.trim().trim_start_matches('.').to_ascii_lowercase();
                let d_json = serde_json::to_string(&normalized).unwrap_or_else(|_| "\"\"".into());
                format!(
                    "((h, d) => h === d || h.endsWith(\".\" + d))(location.hostname.toLowerCase(), {d_json})"
                )
            });

            let scope_check = match (u_filter, d_filter) {
                (Some(u), Some(d)) => format!("if (!location.href.includes({u}) || !{d}) return null;"),
                (Some(u), None) => format!("if (!location.href.includes({u})) return null;"),
                (None, Some(d)) => format!("if (!{d}) return null;"),
                (None, None) => "".to_string(),
            };

            if let Some(k) = key {
                let key_json = serde_json::to_string(k).map_err(|e| e.to_string())?;
                Ok(format!(
                    "(() => {{ {scope_check} {storage_obj}.removeItem({key_json}); return null; }})()"
                ))
            } else {
                Ok(format!(
                    "(() => {{ {scope_check} {storage_obj}.clear(); return null; }})()"
                ))
            }
        }
        other => Err(format!("unknown storage action: {other}")),
    }
}

pub fn parse_storage_result(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed == "null" || trimmed == "undefined" || trimmed.is_empty() {
        return None;
    }
    if let Ok(s) = serde_json::from_str::<String>(trimmed) {
        Some(s)
    } else {
        Some(trimmed.to_string())
    }
}

pub async fn eval_browser_session<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    browser_id: &str,
    script: &str,
) -> Result<(Option<String>, bool), IpcError> {
    if crate::browser::agent::page::is_engine_tab(browser_id) {
        let raw = crate::browser::agent::page::eval(app, manager, browser_id, script.to_string()).await?;
        let (truncated_str, truncated) = truncate_eval_result(raw);
        return Ok(((truncated_str != "undefined").then_some(truncated_str), truncated));
    }
    let state = manager.get_state(browser_id)?;
    let webview = app
        .get_webview(&state.webview_label)
        .ok_or_else(|| BrowserError::WebviewNotFound(state.webview_label.clone()))?;
    let raw = eval_webview(webview, script.to_string()).await?;
    let (truncated_str, truncated) = truncate_eval_result(raw);
    let result = if truncated_str == "undefined" {
        None
    } else {
        Some(truncated_str)
    };
    Ok((result, truncated))
}

pub async fn wait_browser_session<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    browser_id: &str,
    condition: crate::browser::model::BrowserWaitCondition,
) -> Result<(), IpcError> {
    let (cond, timeout) = match condition {
        crate::browser::model::BrowserWaitCondition::WithTimeout { inner, timeout_ms } => {
            (*inner, Some(std::time::Duration::from_millis(timeout_ms)))
        }
        other => (other, None),
    };
    wait_browser_session_with_timeout(app, manager, browser_id, cond, timeout).await
}

const DEFAULT_WAIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
/// Upper bound on a single browser wait; remote and CLI callers pass unclamped `timeoutMs`.
const MAX_WAIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

fn wait_deadline(
    now: tokio::time::Instant,
    timeout: Option<std::time::Duration>,
) -> tokio::time::Instant {
    let timeout = timeout.unwrap_or(DEFAULT_WAIT_TIMEOUT).min(MAX_WAIT_TIMEOUT);
    now.checked_add(timeout).unwrap_or(now)
}

pub async fn wait_browser_session_with_timeout<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    browser_id: &str,
    condition: crate::browser::model::BrowserWaitCondition,
    timeout: Option<std::time::Duration>,
) -> Result<(), IpcError> {
    if crate::browser::agent::page::is_engine_tab(browser_id) {
        return wait_engine_tab(app, manager, browser_id, &condition, timeout).await;
    }
    let state = manager.get_state(browser_id)?;
    let webview = app
        .get_webview(&state.webview_label)
        .ok_or_else(|| BrowserError::WebviewNotFound(state.webview_label.clone()))?;
    let script = build_wait_condition_script(&condition);
    let deadline = wait_deadline(tokio::time::Instant::now(), timeout);
    let interval = std::time::Duration::from_millis(250);

    loop {
        let now = tokio::time::Instant::now();
        if now >= deadline {
            return Err(IpcError::new(
                IpcErrorCode::BrowserWaitTimeout,
                "browser wait condition timed out",
            ));
        }

        let remaining = deadline - now;
        let eval_timeout = remaining.min(std::time::Duration::from_secs(5));
        match tokio::time::timeout(eval_timeout, eval_webview(webview.clone(), script.clone()))
            .await
        {
            Ok(Ok(res)) if parse_eval_boolean(&res) => return Ok(()),
            Ok(Ok(_)) => {}
            Ok(Err(_)) | Err(_) => {
                // A failed or timed-out eval is retried only while the tab still exists; a
                // closed tab reports that instead of a later BROWSER_WAIT_TIMEOUT.
                let current = manager.get_state(browser_id)?;
                if app.get_webview(&current.webview_label).is_none() {
                    return Err(BrowserError::WebviewNotFound(current.webview_label).into());
                }
            }
        }

        let now_after = tokio::time::Instant::now();
        if now_after >= deadline {
            return Err(IpcError::new(
                IpcErrorCode::BrowserWaitTimeout,
                "browser wait condition timed out",
            ));
        }

        let remaining_after = deadline - now_after;
        let sleep_duration = interval.min(remaining_after);
        tokio::time::sleep(sleep_duration).await;
    }
}

pub async fn console_browser_session<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    browser_id: &str,
    errors_only: bool,
    clear: bool,
) -> Result<Vec<crate::browser::model::BrowserConsoleEntry>, IpcError> {
    let state = manager.get_state(browser_id)?;
    let webview = app
        .get_webview(&state.webview_label)
        .ok_or_else(|| BrowserError::WebviewNotFound(state.webview_label.clone()))?;
    let script = crate::browser::guest::build_console_drain_script(clear, errors_only);
    let raw = eval_webview(webview, script).await?;
    let entries = crate::browser::guest::parse_console_drain_result(&raw)
        .map_err(BrowserError::AutomationFailed)?;
    Ok(entries)
}

pub async fn screenshot_browser_session<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    browser_id: &str,
    out_path: &str,
) -> Result<String, IpcError> {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    if crate::browser::agent::page::is_engine_tab(browser_id) {
        let target = crate::browser::screenshot::resolve_screenshot_path(out_path)?;
        let source = crate::browser::agent::cef_backend::EngineSnapshotSource { browser_id };
        return crate::browser::screenshot::take_browser_screenshot_with_source(&source, browser_id, target)
            .await;
    }
    let state = manager.get_state(browser_id)?;
    crate::browser::screenshot::take_browser_screenshot(app, &state.webview_label, out_path).await
}

/// `wait` for a Chromium engine tab: the same condition scripts, evaluated over the engine's
/// DevTools API until true or the deadline. Like the webview path it re-evaluates on a short
/// interval, because a page condition has no event to subscribe to.
async fn wait_engine_tab<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    browser_id: &str,
    condition: &crate::browser::model::BrowserWaitCondition,
    timeout: Option<std::time::Duration>,
) -> Result<(), IpcError> {
    let script = build_wait_condition_script(condition);
    let deadline = wait_deadline(tokio::time::Instant::now(), timeout);
    let interval = std::time::Duration::from_millis(250);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(IpcError::new(IpcErrorCode::BrowserWaitTimeout, "browser wait condition timed out"));
        }
        match crate::browser::agent::page::eval(app, manager, browser_id, script.clone()).await {
            Ok(raw) if parse_eval_boolean(&raw) => return Ok(()),
            Ok(_) => {}
            // A closed tab ends the wait at once; a navigating page is retried.
            Err(BrowserError::NotFound(id)) => return Err(BrowserError::NotFound(id).into()),
            Err(_) => {}
        }
        tokio::time::sleep(interval.min(deadline.saturating_duration_since(tokio::time::Instant::now()))).await;
    }
}

pub async fn highlight_browser_session<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    browser_id: &str,
    selector: &str,
) -> Result<crate::browser::picker::BrowserHighlightResult, IpcError> {
    highlight_browser_session_with_duration(app, manager, browser_id, selector, 2000).await
}

pub async fn highlight_browser_session_with_duration<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    browser_id: &str,
    selector: &str,
    duration_ms: u64,
) -> Result<crate::browser::picker::BrowserHighlightResult, IpcError> {
    if selector.trim().is_empty() {
        return Err(IpcError::new(
            IpcErrorCode::InvalidArgument,
            "selector cannot be empty",
        ));
    }
    let state = manager.get_state(browser_id)?;
    let webview = app
        .get_webview(&state.webview_label)
        .ok_or_else(|| BrowserError::WebviewNotFound(state.webview_label.clone()))?;
    let script = crate::browser::picker::browser_highlight_script(selector, duration_ms)?;
    let raw = eval_webview(webview, script).await?;
    crate::browser::picker::parse_browser_highlight_callback(&raw, selector)
}

#[tauri::command]
pub async fn cmd_browser_highlight<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
    selector: String,
) -> Result<crate::browser::picker::BrowserHighlightResult, IpcError> {
    highlight_browser_session(&app, manager.inner(), &browser_id, &selector).await
}

pub struct BrowserCookieParams<'a> {
    pub action: &'a str,
    pub name: Option<&'a str>,
    pub value: Option<&'a str>,
    pub domain: Option<&'a str>,
    pub path: Option<&'a str>,
}

pub async fn cookies_browser_session<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    browser_id: &str,
    params: BrowserCookieParams<'_>,
) -> Result<Vec<crate::browser::model::BrowserCookieEntry>, IpcError> {
    let BrowserCookieParams { action, name, value, domain, path } = params;
    let state = manager.get_state(browser_id)?;
    let webview = app
        .get_webview(&state.webview_label)
        .ok_or_else(|| BrowserError::WebviewNotFound(state.webview_label.clone()))?;
    let script = build_cookie_script(action, name, value, domain, path)
        .map_err(|e| IpcError::new(IpcErrorCode::InvalidArgument, e))?;
    let raw = eval_webview(webview, script).await?;
    Ok(filter_cookie_entries(action, name, parse_document_cookie(&raw)))
}

/// Narrows a `get`/`list` result to the named cookie. The action matches case-insensitively,
/// like `build_cookie_script`, so `GET name` filters the same way `get name` does.
fn filter_cookie_entries(
    action: &str,
    name: Option<&str>,
    mut entries: Vec<crate::browser::model::BrowserCookieEntry>,
) -> Vec<crate::browser::model::BrowserCookieEntry> {
    if action.eq_ignore_ascii_case("get") || action.eq_ignore_ascii_case("list") {
        if let Some(target_name) = name {
            entries.retain(|c| c.name == target_name);
        }
    }
    entries
}

pub struct BrowserStorageParams<'a> {
    pub kind: &'a str,
    pub action: &'a str,
    pub key: Option<&'a str>,
    pub value: Option<&'a str>,
    pub clear_all: bool,
    pub url: Option<&'a str>,
    pub domain: Option<&'a str>,
}

pub async fn storage_browser_session_extended<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    browser_id: &str,
    params: BrowserStorageParams<'_>,
) -> Result<Option<String>, IpcError> {
    let BrowserStorageParams { kind, action, key, value, clear_all, url, domain } = params;
    let state = manager.get_state(browser_id)?;
    let webview = app
        .get_webview(&state.webview_label)
        .ok_or_else(|| BrowserError::WebviewNotFound(state.webview_label.clone()))?;
    let script = build_storage_script_extended(kind, action, key, value, clear_all, url, domain)
        .map_err(|e| IpcError::new(IpcErrorCode::InvalidArgument, e))?;
    let raw = eval_webview(webview, script).await?;
    Ok(parse_storage_result(&raw))
}

/// Persistent state representation for saving and restoring browser state.
///
/// Cookies are saved and restored using the underlying webview cookie store
/// (`Webview::cookies()` / `Webview::set_cookie()`), preserving all attributes
/// including HttpOnly, Secure, SameSite, domain, path, and expiry.
/// Backward compatibility is retained for legacy state files containing name/value pairs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserPersistentState {
    #[serde(default = "default_state_version")]
    pub version: u32,
    pub browser_id: String,
    pub url: String,
    #[serde(default)]
    pub cookies: Vec<crate::browser::state_cookies::PersistedCookie>,
    #[serde(default)]
    pub local_storage: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub session_storage: std::collections::BTreeMap<String, String>,
}

fn default_state_version() -> u32 {
    2
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrowserDumpResult {
    local_storage: std::collections::BTreeMap<String, String>,
    session_storage: std::collections::BTreeMap<String, String>,
}

pub const BROWSER_DUMP_STATE_SCRIPT: &str = r##"(() => {
  const local = {};
  for (let i = 0; i < localStorage.length; i++) {
    const k = localStorage.key(i);
    if (k !== null) local[k] = localStorage.getItem(k);
  }
  const session = {};
  for (let i = 0; i < sessionStorage.length; i++) {
    const k = sessionStorage.key(i);
    if (k !== null) session[k] = sessionStorage.getItem(k);
  }
  return JSON.stringify({
    localStorage: local,
    sessionStorage: session,
  });
})()"##;

pub async fn save_browser_state_session<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    browser_id: &str,
    out_path: &str,
) -> Result<String, IpcError> {
    let state = manager.get_state(browser_id)?;
    let webview = app
        .get_webview(&state.webview_label)
        .ok_or_else(|| BrowserError::WebviewNotFound(state.webview_label.clone()))?;

    // Read full cookie jar with attributes including HttpOnly via Webview::cookies(); the
    // getter blocks on the main thread's reply, so it runs off the async workers.
    let cookie_webview = webview.clone();
    let raw_cookies = crate::ipc::run_blocking(move || {
        cookie_webview.cookies().map_err(|e| {
            IpcError::new(
                IpcErrorCode::InternalError,
                format!("failed to read cookies from webview: {e}"),
            )
        })
    })
    .await?;
    let cookies = raw_cookies
        .into_iter()
        .map(|c| crate::browser::state_cookies::PersistedCookie::from_cookie(&c))
        .collect::<Vec<_>>();

    let raw = eval_webview(webview, BROWSER_DUMP_STATE_SCRIPT.to_string()).await?;
    let unquoted: String = serde_json::from_str(&raw).unwrap_or(raw);
    let dump: BrowserDumpResult = serde_json::from_str(&unquoted).map_err(|e| {
        IpcError::new(
            IpcErrorCode::ParseError,
            format!("failed to parse browser dump: {e}"),
        )
    })?;

    let persistent_state = BrowserPersistentState {
        version: 2,
        browser_id: browser_id.to_string(),
        url: state.url,
        cookies,
        local_storage: dump.local_storage,
        session_storage: dump.session_storage,
    };
    let json_bytes = serde_json::to_vec_pretty(&persistent_state).map_err(|e| {
        IpcError::new(
            IpcErrorCode::InternalError,
            format!("failed to serialize persistent state: {e}"),
        )
    })?;

    let path = std::path::PathBuf::from(out_path);
    crate::ipc::run_blocking::<(), _>(move || {
        write_browser_state_file(&path, &json_bytes)
    })
    .await?;

    Ok(out_path.to_string())
}

pub(crate) fn write_browser_state_file(
    path: &std::path::Path,
    json_bytes: &[u8],
) -> Result<(), IpcError> {
    use std::io::Write;
    let parent = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => {
            std::fs::create_dir_all(parent).map_err(|e| {
                IpcError::new(
                    IpcErrorCode::InternalError,
                    format!("failed to create directory for state file: {e}"),
                )
            })?;
            parent
        }
        _ => std::path::Path::new("."),
    };
    let write_error = |e: std::io::Error| {
        IpcError::new(
            IpcErrorCode::InternalError,
            format!("failed to write state file '{}': {e}", path.display()),
        )
    };
    // The state holds HttpOnly session cookies: tempfile creates the file owner-only (0600 on
    // unix), and the rename publishes it complete or not at all.
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(write_error)?;
    file.write_all(json_bytes).map_err(write_error)?;
    file.persist(path).map_err(|e| write_error(e.error))?;
    Ok(())
}

pub(crate) fn prepare_restored_cookies(
    persistent_state: &BrowserPersistentState,
) -> Result<Vec<cookie::Cookie<'static>>, IpcError> {
    let mut restored = Vec::with_capacity(persistent_state.cookies.len());
    for persisted_cookie in &persistent_state.cookies {
        let cookie = persisted_cookie
            .to_cookie_for_url(&persistent_state.url)
            .map_err(|e| {
                IpcError::new(
                    IpcErrorCode::InternalError,
                    format!("invalid cookie '{}' in state: {e}", persisted_cookie.name),
                )
            })?;
        restored.push(cookie);
    }
    Ok(restored)
}

pub async fn load_browser_state_session<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    browser_id: &str,
    in_path: &str,
) -> Result<(), IpcError> {
    let state = manager.get_state(browser_id)?;
    let webview = app
        .get_webview(&state.webview_label)
        .ok_or_else(|| BrowserError::WebviewNotFound(state.webview_label.clone()))?;

    let in_path_buf = std::path::PathBuf::from(in_path);
    let content = crate::ipc::run_blocking::<String, _>(move || {
        std::fs::read_to_string(&in_path_buf).map_err(|e| {
            IpcError::new(
                IpcErrorCode::InvalidArgument,
                format!("failed to read state file '{}': {e}", in_path_buf.display()),
            )
        })
    })
    .await?;

    let persistent_state: BrowserPersistentState = serde_json::from_str(&content).map_err(|e| {
        IpcError::new(
            IpcErrorCode::ParseError,
            format!("invalid browser state JSON in '{in_path}': {e}"),
        )
    })?;

    let cookies_to_restore = prepare_restored_cookies(&persistent_state)?;
    // Web storage is written into whatever origin the tab shows now; refuse to plant one
    // site's storage into another's. Cookies carry their own domain and are unaffected.
    let has_storage =
        !persistent_state.local_storage.is_empty() || !persistent_state.session_storage.is_empty();
    if has_storage && !same_web_origin(&state.url, &persistent_state.url) {
        return Err(IpcError::new(
            IpcErrorCode::InvalidArgument,
            format!(
                "browser state was saved on '{}' but the tab shows '{}'; open the saved origin before loading it",
                persistent_state.url, state.url
            ),
        ));
    }
    for cookie in cookies_to_restore {
        let cookie_name = cookie.name().to_string();
        webview.set_cookie(cookie).map_err(|e| {
            IpcError::new(
                IpcErrorCode::InternalError,
                format!("failed to set cookie '{cookie_name}' in webview: {e}"),
            )
        })?;
    }

    let mut script_parts = Vec::new();
    script_parts.push("(() => {".to_string());
    for (k, v) in &persistent_state.local_storage {
        let k_json = serde_json::to_string(k).unwrap_or_default();
        let v_json = serde_json::to_string(v).unwrap_or_default();
        script_parts.push(format!("localStorage.setItem({k_json}, {v_json});"));
    }
    for (k, v) in &persistent_state.session_storage {
        let k_json = serde_json::to_string(k).unwrap_or_default();
        let v_json = serde_json::to_string(v).unwrap_or_default();
        script_parts.push(format!("sessionStorage.setItem({k_json}, {v_json});"));
    }
    script_parts.push("return true;".to_string());
    script_parts.push("})()".to_string());
    let full_script = script_parts.join("\n");
    let _ = eval_webview(webview, full_script).await?;
    Ok(())
}

/// True when both URLs parse and share a tuple (scheme, host, port) origin. Opaque origins
/// (about:blank, data:, malformed URLs) never match.
fn same_web_origin(a: &str, b: &str) -> bool {
    match (url::Url::parse(a), url::Url::parse(b)) {
        (Ok(a), Ok(b)) => {
            let (a, b) = (a.origin(), b.origin());
            a.is_tuple() && a == b
        }
        _ => false,
    }
}

/// Trait hook for external daemon transport to execute authoritative desktop reclaim (R5-5).
/// W4-bridge owns the transport implementation; gateway/UI routes through this hook when
/// in external daemon mode.
pub trait DaemonReclaimTransport: Send + Sync {
    fn reclaim_daemon_broker<'a>(
        &'a self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u64, IpcError>> + Send + 'a>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PureGuiMode(pub bool);

pub struct ProductionDaemonReclaimTransport {
    remote_manager: Arc<crate::ipc::remote::RemoteGatewayManager>,
    daemon_client: Arc<crate::daemon::client::DaemonClient>,
}

impl ProductionDaemonReclaimTransport {
    pub fn new(
        remote_manager: Arc<crate::ipc::remote::RemoteGatewayManager>,
        daemon_client: Arc<crate::daemon::client::DaemonClient>,
    ) -> Self {
        Self {
            remote_manager,
            daemon_client,
        }
    }
}

impl DaemonReclaimTransport for ProductionDaemonReclaimTransport {
    fn reclaim_daemon_broker<'a>(
        &'a self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u64, IpcError>> + Send + 'a>>
    {
        Box::pin(async move {
            // 1. Authoritative in-process state if present
            if let Some(state) = self.remote_manager.state() {
                let lease = state.admission_controller.reclaim_desktop();
                return Ok(lease.map(|l| l.lease_epoch).unwrap_or(1));
            }

            // 2. Authoritative daemon-side reclaim: revoke control permissions & clear active selection.
            // Every failure propagates: reporting a reclaim that left a device in control is a
            // security-relevant false success. All revokes are attempted before the first error returns.
            let devices = self.daemon_client.remote_list_devices().await?;
            let mut first_error = None;
            for dev in devices {
                if dev.permission == crate::remote::DevicePermission::Control {
                    if let Err(error) = self.daemon_client.remote_revoke_device(&dev.id).await {
                        first_error.get_or_insert(error);
                    }
                }
            }
            if let Err(error) = self.daemon_client.remote_set_active_selection(None).await {
                first_error.get_or_insert(error);
            }
            if let Some(error) = first_error {
                return Err(error);
            }

            let epoch = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(1);
            Ok(epoch)
        })
    }
}

#[tauri::command]
pub async fn cmd_browser_remote_reclaim<R: tauri::Runtime>(
    app: AppHandle<R>,
) -> Result<u64, IpcError> {
    use tauri::{Emitter, Manager};

    let mut epoch = None;
    let mut sharing_state = None;

    // 1. Authoritative daemon-side broker when gateway state is present in-process
    if let Some(state) = app.try_state::<Arc<crate::remote::state::RemoteGatewayState>>() {
        let lease = state.admission_controller.reclaim_desktop();
        epoch = Some(lease.map(|l| l.lease_epoch).unwrap_or(1));
        sharing_state = Some(state.admission_controller.sharing_registry().current());
    } else if let Some(mgr) = app.try_state::<Arc<crate::ipc::remote::RemoteGatewayManager>>() {
        if let Some(state) = mgr.state() {
            let lease = state.admission_controller.reclaim_desktop();
            epoch = Some(lease.map(|l| l.lease_epoch).unwrap_or(1));
            sharing_state = Some(state.admission_controller.sharing_registry().current());
        }
    }

    // 2. In external daemon mode: route reclaim to the daemon broker via transport hook
    if epoch.is_none() {
        if let Some(transport) = app.try_state::<Arc<dyn DaemonReclaimTransport>>() {
            epoch = Some(transport.reclaim_daemon_broker().await?);
        }
    }

    // 3. Fall back to local broker only via explicit pure-GUI mode flag; reject missing transport in daemon mode (R6-5)
    let final_epoch = match epoch {
        Some(e) => {
            if let Some(broker) =
                app.try_state::<Arc<crate::browser::remote_driver::RemoteDriverBroker>>()
            {
                let _ = broker.desktop_reclaim();
            }
            e
        }
        None => {
            let is_pure_gui = app.try_state::<PureGuiMode>().map(|m| m.0).unwrap_or(false);
            if is_pure_gui {
                if let Some(broker) =
                    app.try_state::<Arc<crate::browser::remote_driver::RemoteDriverBroker>>()
                {
                    broker.desktop_reclaim()
                } else {
                    1
                }
            } else {
                return Err(IpcError::new(
                    IpcErrorCode::BrowserHostUnavailable,
                    "DaemonReclaimTransport is not available in daemon mode",
                ));
            }
        }
    };

    let state_dto = match sharing_state {
        Some(s) => crate::browser::remote_bridge_protocol::BrowserSharingState::from(&s),
        None => crate::browser::remote_bridge_protocol::BrowserSharingState::idle(),
    };
    let _ = app.emit("browser-remote-sharing", &state_dto);

    Ok(final_epoch)
}

#[tauri::command]
pub async fn cmd_browser_remote_revoke<R: tauri::Runtime>(
    app: AppHandle<R>,
) -> Result<u64, IpcError> {
    cmd_browser_remote_reclaim(app).await
}

pub struct GuiBrowserCommandExecutor<R: tauri::Runtime> {
    app: AppHandle<R>,
    manager: Arc<BrowserManager>,
}

impl<R: tauri::Runtime> GuiBrowserCommandExecutor<R> {
    pub fn new(app: AppHandle<R>, manager: Arc<BrowserManager>) -> Self {
        Self { app, manager }
    }
}

impl<R: tauri::Runtime> crate::remote::browser_backend::BrowserCommandExecutor
    for GuiBrowserCommandExecutor<R>
{
    fn execute<'a>(
        &'a self,
        ctx: crate::remote::browser_backend::BrowserCommandContext,
    ) -> crate::remote::browser_backend::BoxFuture<
        'a,
        Result<
            crate::remote::browser_backend::BrowserCommandResult,
            crate::remote::browser_backend::RemoteBrowserError,
        >,
    > {
        Box::pin(async move {
            use crate::remote::browser_backend::{BrowserCommandResult, RemoteBrowserError};

            if let Some(ref exp_gen) = ctx.document_generation {
                // A fence that cannot be parsed must refuse, never skip the check.
                let expected = exp_gen.trim().parse::<u64>().map_err(|_| {
                    RemoteBrowserError::InvalidRequest(format!(
                        "invalid documentGeneration: {exp_gen}"
                    ))
                })?;
                let st = self
                    .manager
                    .get_state(&ctx.browser_id)
                    .map_err(|_| RemoteBrowserError::NotFound(ctx.browser_id.clone()))?;
                if st.generation != expected {
                    return Err(RemoteBrowserError::Forbidden(format!(
                        "Document generation fencing failed: expected {expected}, current {}",
                        st.generation
                    )));
                }
            }

            match ctx.command.as_str() {
                "navigate" => {
                    let url = ctx
                        .params
                        .as_ref()
                        .and_then(|p| p.get("url").and_then(|v| v.as_str()))
                        .ok_or_else(|| {
                            RemoteBrowserError::InvalidRequest("missing url param".into())
                        })?;
                    navigate_browser_session_with_context(
                        &self.app,
                        &self.manager,
                        &ctx.browser_id,
                        url,
                        crate::browser::security::BrowserSecurityContext::Remote,
                    )
                    .await
                    .map_err(ipc_to_remote)?;
                    let state = self
                        .manager
                        .get_state(&ctx.browser_id)
                        .map_err(|_| RemoteBrowserError::NotFound(ctx.browser_id.clone()))?;
                    Ok(BrowserCommandResult {
                        success: true,
                        value: Some(serde_json::json!({
                            "browserId": state.browser_id,
                            "url": state.url,
                            "generation": state.generation.to_string(),
                        })),
                    })
                }
                "back" => {
                    history_navigation(&self.app, &self.manager, &ctx.browser_id, false)
                        .map_err(ipc_to_remote)?;
                    Ok(BrowserCommandResult {
                        success: true,
                        value: None,
                    })
                }
                "forward" => {
                    history_navigation(&self.app, &self.manager, &ctx.browser_id, true)
                        .map_err(ipc_to_remote)?;
                    Ok(BrowserCommandResult {
                        success: true,
                        value: None,
                    })
                }
                "reload" => {
                    // Resolve the webview before marking the tab loading, so a missing
                    // webview leaves no stuck `loading` state behind.
                    let label = self
                        .manager
                        .get_webview_label(&ctx.browser_id)
                        .map_err(|e| ipc_to_remote(e.into()))?;
                    let webview = self
                        .app
                        .get_webview(&label)
                        .ok_or_else(|| ipc_to_remote(BrowserError::WebviewNotFound(label).into()))?;
                    let state = self
                        .manager
                        .begin_reload(&ctx.browser_id)
                        .map_err(|e| ipc_to_remote(e.into()))?;
                    emit_browser_state(&webview, &state);
                    if let Err(error) = webview.reload() {
                        let message = error.to_string();
                        if let Ok(error_state) = self.manager.update_navigation_state(
                            &ctx.browser_id,
                            None,
                            None,
                            Some(false),
                            None,
                            None,
                            Some(message.clone()),
                        ) {
                            emit_browser_state(&webview, &error_state);
                        }
                        return Err(RemoteBrowserError::ExecutionFailed(message));
                    }
                    Ok(BrowserCommandResult {
                        success: true,
                        value: None,
                    })
                }
                "click" => {
                    let p = ctx.params.as_ref().ok_or_else(|| {
                        RemoteBrowserError::InvalidRequest("click requires params".into())
                    })?;

                    // R5-10, R6-6: Point-click viewport revision recheck at execution time (mismatch => BROWSER_STALE_FRAME)
                    let (_, _, current_vp_rev) = self
                        .manager
                        .get_geometry(&ctx.browser_id)
                        .unwrap_or((None, 1.0, 1));
                    if let Some(expected_vp) = expected_viewport_revision(p)? {
                        if expected_vp != current_vp_rev {
                            return Err(RemoteBrowserError::StaleFrame("viewport revision changed since frame capture".into()));
                        }
                    }

                    let state = self
                        .manager
                        .get_state(&ctx.browser_id)
                        .map_err(|_| RemoteBrowserError::NotFound(ctx.browser_id.clone()))?;

                    // R5-11: Reference resolution via snapshotId and mapRevision
                    let maybe_ref = p
                        .get("reference")
                        .or_else(|| p.get("ref"))
                        .and_then(|v| v.as_str())
                        .or_else(|| {
                            if p.get("snapshotId").is_some() || p.get("snapshot_id").is_some() {
                                p.get("selector").and_then(|v| v.as_str())
                            } else {
                                None
                            }
                        });

                    let script = if let Some(ref_str) = maybe_ref {
                        let snap_id = p
                            .get("snapshotId")
                            .or_else(|| p.get("snapshot_id"))
                            .and_then(|v| v.as_str())
                            .filter(|s| !s.trim().is_empty())
                            .ok_or_else(|| {
                                RemoteBrowserError::InvalidRequest(
                                    "remote click requires snapshotId".into(),
                                )
                            })?;
                        let map_rev = p
                            .get("mapRevision")
                            .or_else(|| p.get("map_revision"))
                            .and_then(|v| {
                                v.as_u64()
                                    .or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok()))
                            })
                            .ok_or_else(|| {
                                RemoteBrowserError::InvalidRequest(
                                    "remote click requires mapRevision".into(),
                                )
                            })?;
                        let selector = self
                            .manager
                            .verify_remote_target(&ctx.browser_id, snap_id, map_rev, ref_str)
                            .map_err(|e| {
                                RemoteBrowserError::TargetNotFound(format!(
                                    "target resolution failed: {e}"
                                ))
                            })?;
                        let sel_json = serde_json::to_string(&selector).unwrap_or_default();
                        crate::browser::remote_input::build_selector_click_script(&sel_json)
                    } else if let Some(selector) = p.get("selector").and_then(|v| v.as_str()) {
                        let sel_json = serde_json::to_string(selector).unwrap_or_default();
                        crate::browser::remote_input::build_selector_click_script(&sel_json)
                    } else if let (Some(px), Some(py)) = (
                        p.get("x").and_then(|n| n.as_f64()),
                        p.get("y").and_then(|n| n.as_f64()),
                    ) {
                        // R5-10: Use the validated capture coordinates from WS fence directly without remapping
                        crate::browser::remote_input::build_point_click_script(px, py)
                    } else if let (Some(u), Some(v)) = (
                        p.get("u").and_then(|n| n.as_f64()),
                        p.get("v").and_then(|n| n.as_f64()),
                    ) {
                        // R5-10: If captureRect was passed from WS fence, use it directly without remapping against current bounds
                        let rect = if let Some(c) = p.get("captureRect") {
                            LogicalRect {
                                x: c.get("x").and_then(|n| n.as_f64()).unwrap_or(0.0),
                                y: c.get("y").and_then(|n| n.as_f64()).unwrap_or(0.0),
                                width: c.get("width").and_then(|n| n.as_f64()).unwrap_or(1024.0),
                                height: c.get("height").and_then(|n| n.as_f64()).unwrap_or(768.0),
                            }
                        } else {
                            let (bounds, _, _) = self
                                .manager
                                .get_geometry(&ctx.browser_id)
                                .unwrap_or((None, 1.0, 1));
                            bounds.unwrap_or(LogicalRect {
                                x: 0.0,
                                y: 0.0,
                                width: 1024.0,
                                height: 768.0,
                            })
                        };
                        let pt = crate::browser::remote_input::map_point_mainframe(
                            u, v, &rect, false, false, false,
                        )
                        .map_err(|e| RemoteBrowserError::InvalidRequest(e.to_string()))?;
                        crate::browser::remote_input::build_point_click_script(pt.x, pt.y)
                    } else {
                        return Err(RemoteBrowserError::TargetNotFound(
                            "missing click target".into(),
                        ));
                    };

                    let webview = self
                        .app
                        .get_webview(&state.webview_label)
                        .ok_or_else(|| RemoteBrowserError::NotFound(state.webview_label.clone()))?;
                    let res_str = eval_webview(webview, script)
                        .await
                        .map_err(|e| RemoteBrowserError::ExecutionFailed(e.to_string()))?;
                    crate::browser::remote_input::decode_point_click_result(&res_str)
                        .map_err(point_click_decode_error_to_remote)?;
                    Ok(BrowserCommandResult {
                        success: true,
                        value: Some(serde_json::json!({ "clicked": true })),
                    })
                }
                "fill" => {
                    let state = self
                        .manager
                        .get_state(&ctx.browser_id)
                        .map_err(|_| RemoteBrowserError::NotFound(ctx.browser_id.clone()))?;
                    let webview = self
                        .app
                        .get_webview(&state.webview_label)
                        .ok_or_else(|| RemoteBrowserError::NotFound(state.webview_label.clone()))?;
                    let p = ctx.params.as_ref().ok_or_else(|| {
                        RemoteBrowserError::InvalidRequest("fill requires params".into())
                    })?;

                    let script = build_fill_script(Some(&self.manager), &ctx.browser_id, p)?;
                    let res_str = eval_webview(webview, script)
                        .await
                        .map_err(|e| RemoteBrowserError::ExecutionFailed(e.to_string()))?;
                    crate::browser::remote_input::decode_action_result(&res_str)
                        .map_err(fill_decode_error_to_remote)?;
                    Ok(BrowserCommandResult {
                        success: true,
                        value: Some(serde_json::json!({ "filled": true })),
                    })
                }
                "keypress" => {
                    let p = ctx.params.as_ref().ok_or_else(|| {
                        RemoteBrowserError::InvalidRequest("keypress requires params".into())
                    })?;
                    let key = p.get("key").and_then(|v| v.as_str()).unwrap_or("");
                    let script = remote_keypress_script(key)
                        .map_err(|e| RemoteBrowserError::InvalidRequest(e.to_string()))?;
                    let state = self
                        .manager
                        .get_state(&ctx.browser_id)
                        .map_err(|_| RemoteBrowserError::NotFound(ctx.browser_id.clone()))?;
                    let webview = self
                        .app
                        .get_webview(&state.webview_label)
                        .ok_or_else(|| RemoteBrowserError::NotFound(state.webview_label.clone()))?;
                    let _ = eval_webview(webview, script)
                        .await
                        .map_err(|e| RemoteBrowserError::ExecutionFailed(e.to_string()))?;
                    Ok(BrowserCommandResult {
                        success: true,
                        value: Some(serde_json::json!({ "dispatched": true })),
                    })
                }
                "eval" => {
                    let p = ctx.params.as_ref().ok_or_else(|| {
                        RemoteBrowserError::InvalidRequest("eval requires params".into())
                    })?;
                    let script = p.get("script").and_then(|v| v.as_str()).ok_or_else(|| {
                        RemoteBrowserError::InvalidRequest("missing script in eval".into())
                    })?;
                    let (eval_res, truncated) =
                        eval_browser_session(&self.app, &self.manager, &ctx.browser_id, script)
                            .await
                            .map_err(ipc_to_remote)?;
                    Ok(BrowserCommandResult {
                        success: true,
                        value: Some(serde_json::json!({
                            "result": eval_res,
                            "truncated": truncated,
                        })),
                    })
                }
                "highlight" => {
                    let p = ctx.params.as_ref().ok_or_else(|| {
                        RemoteBrowserError::InvalidRequest("highlight requires params".into())
                    })?;
                    let selector = p.get("selector").and_then(|v| v.as_str()).ok_or_else(|| {
                        RemoteBrowserError::InvalidRequest("missing selector in highlight".into())
                    })?;
                    let res = highlight_browser_session(
                        &self.app,
                        &self.manager,
                        &ctx.browser_id,
                        selector,
                    )
                    .await
                    .map_err(ipc_to_remote)?;
                    Ok(BrowserCommandResult {
                        success: true,
                        value: Some(serde_json::to_value(&res).unwrap_or_default()),
                    })
                }
                "wait" => {
                    let p = ctx.params.as_ref().ok_or_else(|| {
                        RemoteBrowserError::InvalidRequest("wait requires params".into())
                    })?;
                    let (condition, timeout) =
                        crate::remote::browser_backend::normalize_wait_params(p)?;
                    wait_browser_session_with_timeout(
                        &self.app,
                        &self.manager,
                        &ctx.browser_id,
                        condition,
                        timeout,
                    )
                    .await
                    .map_err(ipc_to_remote)?;
                    Ok(BrowserCommandResult {
                        success: true,
                        value: None,
                    })
                }
                "snapshot" => {
                    let state = self
                        .manager
                        .get_state(&ctx.browser_id)
                        .map_err(|_| RemoteBrowserError::NotFound(ctx.browser_id.clone()))?;
                    let webview = self
                        .app
                        .get_webview(&state.webview_label)
                        .ok_or_else(|| RemoteBrowserError::NotFound(state.webview_label.clone()))?;
                    let result = eval_webview(webview, AUTOMATION_SNAPSHOT_SCRIPT.to_string())
                        .await
                        .map_err(|e| RemoteBrowserError::ExecutionFailed(e.to_string()))?;
                    let snapshot_json: String = serde_json::from_str(&result).map_err(|error| {
                        RemoteBrowserError::ExecutionFailed(format!(
                            "invalid snapshot callback result: {error}"
                        ))
                    })?;
                    let snapshot: AutomationSnapshotResult = serde_json::from_str(&snapshot_json)
                        .map_err(|error| {
                        RemoteBrowserError::ExecutionFailed(format!(
                            "invalid snapshot response: {error}"
                        ))
                    })?;
                    let targets = snapshot
                        .elements
                        .iter()
                        .map(|element| BrowserAutomationTarget {
                            reference: element.reference.clone(),
                            selector: element.selector.clone(),
                        })
                        .collect();
                    let (snapshot_id, map_revision) = self
                        .manager
                        .record_remote_snapshot(&ctx.browser_id, state.generation, targets)
                        .map_err(|e| match e {
                            BrowserError::AutomationSnapshotStale => RemoteBrowserError::Forbidden(
                                "document generation changed during snapshot capture".into(),
                            ),
                            other => RemoteBrowserError::ExecutionFailed(other.to_string()),
                        })?;
                    let elements_catalogue: Vec<serde_json::Value> = snapshot
                        .elements
                        .iter()
                        .map(|element| {
                            serde_json::json!({
                                "ref": element.reference,
                                "role": element.role,
                                "name": element.name,
                                "tagName": element.tag_name,
                            })
                        })
                        .collect();
                    Ok(BrowserCommandResult {
                        success: true,
                        value: Some(serde_json::json!({
                            "snapshotId": snapshot_id,
                            "mapRevision": map_revision,
                            "mapRevisionString": map_revision.to_string(),
                            "documentGeneration": state.generation.to_string(),
                            "elementsCount": elements_catalogue.len(),
                            "elements": elements_catalogue,
                        })),
                    })
                }
                "getState" => {
                    let state = self
                        .manager
                        .get_state(&ctx.browser_id)
                        .map_err(|_| RemoteBrowserError::NotFound(ctx.browser_id.clone()))?;
                    let (_, _, vp_rev) = self
                        .manager
                        .get_geometry(&ctx.browser_id)
                        .unwrap_or((None, 1.0, 1));
                    Ok(BrowserCommandResult {
                        success: true,
                        value: Some(serde_json::json!({
                            "browserId": state.browser_id,
                            "url": state.url,
                            "title": state.title,
                            "generation": state.generation.to_string(),
                            "viewportRevision": vp_rev.to_string(),
                            "loading": state.loading,
                            "visible": state.visible,
                        })),
                    })
                }
                _ => Err(RemoteBrowserError::InvalidRequest(format!(
                    "Unsupported command: '{}'",
                    ctx.command
                ))),
            }
        })
    }
}

/// Validates a remote page keypress against the page-key allowlist and builds the
/// script that dispatches it to the focused element.
pub(crate) fn remote_keypress_script(
    key: &str,
) -> Result<String, crate::browser::remote_input::RemoteInputError> {
    let key = crate::browser::remote_input::validate_page_key(key)?;
    let key_json = serde_json::to_string(&key).unwrap_or_default();
    Ok(format!(
        r#"(function() {{
            const target = document.activeElement || document.body;
            target.dispatchEvent(new KeyboardEvent("keydown", {{ key: {key_json}, bubbles: true }}));
            target.dispatchEvent(new KeyboardEvent("keyup", {{ key: {key_json}, bubbles: true }}));
            return JSON.stringify({{ ok: true }});
        }})()"#
    ))
}

/// The one mapping from a local browser error onto the remote error surface, by code only:
/// every remote command reports a missing tab, a refused URL, a stale reference or a wait
/// timeout the same way.
fn ipc_to_remote(e: IpcError) -> crate::remote::browser_backend::RemoteBrowserError {
    use crate::remote::browser_backend::RemoteBrowserError;
    match e.code {
        IpcErrorCode::BrowserNotFound
        | IpcErrorCode::WebviewNotFound
        | IpcErrorCode::BrowserTabNotFound => RemoteBrowserError::NotFound(e.message),
        IpcErrorCode::BrowserUrlSchemeDenied => RemoteBrowserError::Forbidden(e.message),
        // A stale snapshot means the document moved on, which the snapshot path already
        // reports as a generation-fence refusal.
        IpcErrorCode::BrowserStaleRef | IpcErrorCode::BrowserAutomationSnapshotStale => {
            RemoteBrowserError::Forbidden(e.message)
        }
        IpcErrorCode::BrowserUrlInvalid
        | IpcErrorCode::BrowserBoundsInvalid
        | IpcErrorCode::InvalidArgument => RemoteBrowserError::InvalidRequest(e.message),
        IpcErrorCode::BrowserInvalidReference | IpcErrorCode::BrowserAutomationTargetNotFound => {
            RemoteBrowserError::TargetNotFound(e.message)
        }
        IpcErrorCode::BrowserWaitTimeout => RemoteBrowserError::WaitTimeout,
        IpcErrorCode::BrowserHostUnavailable => RemoteBrowserError::Unavailable(e.message),
        _ => RemoteBrowserError::ExecutionFailed(e.message),
    }
}

/// Reads a point-click's `viewportRevision` fence. A present value that is not a
/// non-negative integer (number or decimal string) is refused rather than skipped.
fn expected_viewport_revision(
    p: &serde_json::Value,
) -> Result<Option<u64>, crate::remote::browser_backend::RemoteBrowserError> {
    use crate::remote::browser_backend::RemoteBrowserError;
    let Some(raw) = p
        .get("viewportRevision")
        .or_else(|| p.get("viewport_revision"))
        .filter(|v| !v.is_null())
    else {
        return Ok(None);
    };
    raw.as_u64()
        .or_else(|| raw.as_str().and_then(|s| s.trim().parse::<u64>().ok()))
        .map(Some)
        .ok_or_else(|| {
            RemoteBrowserError::InvalidRequest(format!("invalid viewportRevision: {raw}"))
        })
}

/// Maps a decoded fill/type failure onto the remote error surface.
///
/// `decode_action_result` never yields `ActionDecodeError::Refused` (only the
/// point-click decoder does), so fill shares the click mapping instead of carrying
/// its own unreachable refusal arm; every reachable variant maps identically.
fn fill_decode_error_to_remote(
    e: crate::browser::remote_input::ActionDecodeError,
) -> crate::remote::browser_backend::RemoteBrowserError {
    point_click_decode_error_to_remote(e)
}

/// Maps a decoded click failure onto the remote error surface by variant only.
fn point_click_decode_error_to_remote(
    e: crate::browser::remote_input::ActionDecodeError,
) -> crate::remote::browser_backend::RemoteBrowserError {
    use crate::browser::remote_input::ActionDecodeError;
    use crate::remote::browser_backend::RemoteBrowserError;
    match e {
        ActionDecodeError::Refused(input_err) => RemoteBrowserError::InputRefused(input_err.explanation()),
        ActionDecodeError::NotFound(message) => RemoteBrowserError::TargetNotFound(message),
        ActionDecodeError::Failed(message) | ActionDecodeError::Malformed(message) => {
            RemoteBrowserError::ExecutionFailed(message)
        }
    }
}

pub fn build_fill_script(
    manager: Option<&BrowserManager>,
    browser_id: &str,
    p: &serde_json::Value,
) -> Result<String, crate::remote::browser_backend::RemoteBrowserError> {
    use crate::remote::browser_backend::RemoteBrowserError;
    let value = p
        .get("value")
        .or_else(|| p.get("text"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let val_json = serde_json::to_string(value).unwrap_or_default();

    let fill_rev = p
        .get("revision")
        .or_else(|| p.get("imeRevision"))
        .or_else(|| p.get("trackedRevision"))
        .and_then(|v| {
            v.as_u64()
                .or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok()))
        });
    let rev_json = serde_json::to_string(&fill_rev).unwrap_or_else(|_| "null".into());

    let snap_id = p
        .get("snapshotId")
        .or_else(|| p.get("snapshot_id"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty());

    let raw_ref = p
        .get("reference")
        .or_else(|| p.get("ref"))
        .and_then(|v| v.as_str());

    let raw_selector = p.get("selector").and_then(|v| v.as_str());

    // R6-10: Skip snapshot-reference work for 'active'-element / CSS-selector fills
    // and only resolve snapshot refs when snapshot_id is present.
    let is_active = raw_ref == Some("active")
        || raw_selector == Some("active")
        || (raw_ref.is_none() && raw_selector.is_none() && snap_id.is_none());

    if is_active {
        // Active element fill
        Ok(format!(
            r#"(function() {{
                const el = document.activeElement;
                if (!el || el === document.body) return JSON.stringify({{ ok: false, code: "not_found", error: "no active element to fill" }});
                const trackedRevision = el.__ferryx_tracked_revision !== undefined ? el.__ferryx_tracked_revision : 0;
                const incomingRevision = {rev_json};
                if (incomingRevision !== null && incomingRevision < trackedRevision) {{
                    return JSON.stringify({{ ok: false, error: "fill superseded: incoming revision older than tracked" }});
                }}
                if (el.__ferryx_tracked_revision !== undefined && el.__ferryx_tracked_revision !== trackedRevision) {{
                    return JSON.stringify({{ ok: false, error: "fill superseded: revision mismatch at mutation boundary" }});
                }}
                if (incomingRevision !== null) {{
                    el.__ferryx_tracked_revision = incomingRevision;
                }} else {{
                    el.__ferryx_tracked_revision = trackedRevision + 1;
                }}
                el.value = {val_json};
                el.dispatchEvent(new Event("input", {{ bubbles: true }}));
                el.dispatchEvent(new Event("change", {{ bubbles: true }}));
                return JSON.stringify({{ ok: true }});
            }})()"#
        ))
    } else if let (Some(sid), Some(ref_str)) = (snap_id, raw_ref) {
        // Snapshot-resolved element fill
        let map_rev = p
            .get("mapRevision")
            .or_else(|| p.get("map_revision"))
            .and_then(|v| {
                v.as_u64()
                    .or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok()))
            })
            .ok_or_else(|| {
                RemoteBrowserError::InvalidRequest("remote fill requires mapRevision".into())
            })?;
        let mgr =
            manager.ok_or_else(|| RemoteBrowserError::ExecutionFailed("missing manager".into()))?;
        let selector = mgr
            .verify_remote_target(browser_id, sid, map_rev, ref_str)
            .map_err(|e| {
                RemoteBrowserError::TargetNotFound(format!("target resolution failed: {e}"))
            })?;
        let sel_json = serde_json::to_string(&selector).unwrap_or_default();
        Ok(format!(
            r#"(function() {{
                const el = document.querySelector({sel_json});
                if (!el) return JSON.stringify({{ ok: false, code: "not_found", error: "element not found" }});
                const trackedRevision = el.__ferryx_tracked_revision !== undefined ? el.__ferryx_tracked_revision : 0;
                const incomingRevision = {rev_json};
                if (incomingRevision !== null && incomingRevision < trackedRevision) {{
                    return JSON.stringify({{ ok: false, error: "fill superseded: incoming revision older than tracked" }});
                }}
                if (el.__ferryx_tracked_revision !== undefined && el.__ferryx_tracked_revision !== trackedRevision) {{
                    return JSON.stringify({{ ok: false, error: "fill superseded: revision mismatch at mutation boundary" }});
                }}
                if (incomingRevision !== null) {{
                    el.__ferryx_tracked_revision = incomingRevision;
                }} else {{
                    el.__ferryx_tracked_revision = trackedRevision + 1;
                }}
                el.value = {val_json};
                el.dispatchEvent(new Event("input", {{ bubbles: true }}));
                el.dispatchEvent(new Event("change", {{ bubbles: true }}));
                return JSON.stringify({{ ok: true }});
            }})()"#
        ))
    } else if let Some(selector) = raw_selector {
        // CSS-selector element fill (no snapshot ref work)
        let sel_json = serde_json::to_string(selector).unwrap_or_default();
        Ok(format!(
            r#"(function() {{
                const el = document.querySelector({sel_json});
                if (!el) return JSON.stringify({{ ok: false, code: "not_found", error: "element not found" }});
                const trackedRevision = el.__ferryx_tracked_revision !== undefined ? el.__ferryx_tracked_revision : 0;
                const incomingRevision = {rev_json};
                if (incomingRevision !== null && incomingRevision < trackedRevision) {{
                    return JSON.stringify({{ ok: false, error: "fill superseded: incoming revision older than tracked" }});
                }}
                if (el.__ferryx_tracked_revision !== undefined && el.__ferryx_tracked_revision !== trackedRevision) {{
                    return JSON.stringify({{ ok: false, error: "fill superseded: revision mismatch at mutation boundary" }});
                }}
                if (incomingRevision !== null) {{
                    el.__ferryx_tracked_revision = incomingRevision;
                }} else {{
                    el.__ferryx_tracked_revision = trackedRevision + 1;
                }}
                el.value = {val_json};
                el.dispatchEvent(new Event("input", {{ bubbles: true }}));
                el.dispatchEvent(new Event("change", {{ bubbles: true }}));
                return JSON.stringify({{ ok: true }});
            }})()"#
        ))
    } else if raw_ref.is_some() {
        // A snapshot ref without its snapshot cannot be resolved; never retarget the
        // value to whatever element happens to be focused.
        Err(RemoteBrowserError::InvalidRequest(
            "remote fill requires snapshotId".into(),
        ))
    } else {
        // Fallback to active element
        Ok(format!(
            r#"(function() {{
                const el = document.activeElement;
                if (!el || el === document.body) return JSON.stringify({{ ok: false, code: "not_found", error: "no active element to fill" }});
                const trackedRevision = el.__ferryx_tracked_revision !== undefined ? el.__ferryx_tracked_revision : 0;
                const incomingRevision = {rev_json};
                if (incomingRevision !== null && incomingRevision < trackedRevision) {{
                    return JSON.stringify({{ ok: false, error: "fill superseded: incoming revision older than tracked" }});
                }}
                if (el.__ferryx_tracked_revision !== undefined && el.__ferryx_tracked_revision !== trackedRevision) {{
                    return JSON.stringify({{ ok: false, error: "fill superseded: revision mismatch at mutation boundary" }});
                }}
                if (incomingRevision !== null) {{
                    el.__ferryx_tracked_revision = incomingRevision;
                }} else {{
                    el.__ferryx_tracked_revision = trackedRevision + 1;
                }}
                el.value = {val_json};
                el.dispatchEvent(new Event("input", {{ bubbles: true }}));
                el.dispatchEvent(new Event("change", {{ bubbles: true }}));
                return JSON.stringify({{ ok: true }});
            }})()"#
        ))
    }
}

#[cfg(test)]
mod r6_fill_tests {
    use super::*;

    /// Builds a mock webview under a session's label so navigation has a target.
    fn open_mock_webview(
        app: &tauri::App<tauri::test::MockRuntime>,
        label: &str,
    ) -> tauri::WebviewWindow<tauri::test::MockRuntime> {
        tauri::WebviewWindowBuilder::new(app, label, Default::default())
            .build()
            .expect("mock webview")
    }

    #[test]
    fn test_r6_10_fill_active_and_css_skip_snapshot_work() {
        // Mobile IME default: reference is "active", no snapshot_id
        let active_params = serde_json::json!({
            "reference": "active",
            "value": "hello",
        });
        let script = build_fill_script(None, "b1", &active_params)
            .expect("active fill must not require snapshotId");
        assert!(
            script.contains("document.activeElement"),
            "Must target activeElement"
        );
        assert!(
            !script.contains("querySelector"),
            "Active fill must not querySelector"
        );
    }

    #[test]
    fn fill_with_ref_but_no_snapshot_is_rejected_not_retargeted() {
        use crate::remote::browser_backend::RemoteBrowserError;
        for params in [
            serde_json::json!({ "ref": "e3", "value": "secret" }),
            serde_json::json!({ "reference": "e3", "value": "secret" }),
        ] {
            let res = build_fill_script(None, "b1", &params);
            assert!(
                matches!(res, Err(RemoteBrowserError::InvalidRequest(_))),
                "a ref without snapshotId must be InvalidRequest: {params}"
            );
        }
    }

    #[test]
    fn test_r6_11_fill_mutation_boundary_revision_check() {
        let params = serde_json::json!({
            "selector": "#input",
            "value": "world",
            "revision": 5,
        });
        let script = build_fill_script(None, "b1", &params).unwrap();
        assert!(
            script.contains("const trackedRevision = el.__ferryx_tracked_revision"),
            "Must capture tracked revision at script start"
        );
        assert!(
            script.contains("el.__ferryx_tracked_revision !== trackedRevision"),
            "Must compare tracked revision before mutation"
        );
        assert!(script.contains("fill superseded"), "Must abort on mismatch");
    }

    #[test]
    fn fill_decode_error_maps_by_variant_not_text() {
        use crate::browser::remote_input::ActionDecodeError;
        use crate::remote::browser_backend::RemoteBrowserError;
        assert!(matches!(
            fill_decode_error_to_remote(ActionDecodeError::Failed("element not found".into())),
            RemoteBrowserError::ExecutionFailed(_)
        ));
        assert!(matches!(
            fill_decode_error_to_remote(ActionDecodeError::Failed("no active element to fill".into())),
            RemoteBrowserError::ExecutionFailed(_)
        ));
        assert!(matches!(
            fill_decode_error_to_remote(ActionDecodeError::NotFound("element not found".into())),
            RemoteBrowserError::TargetNotFound(_)
        ));
        assert!(matches!(
            fill_decode_error_to_remote(ActionDecodeError::Malformed("bad".into())),
            RemoteBrowserError::ExecutionFailed(_)
        ));
    }

    #[test]
    fn fill_scripts_report_missing_target_with_structured_code() {
        for params in [
            serde_json::json!({ "reference": "active", "value": "v" }),
            serde_json::json!({ "selector": "#input", "value": "v" }),
        ] {
            let script = build_fill_script(None, "b1", &params).unwrap();
            assert!(
                script.contains(r#"code: "not_found""#),
                "fill script must tag a missing target with code not_found: {params}"
            );
        }
    }

    #[test]
    fn selector_click_missing_target_maps_to_target_not_found() {
        use crate::browser::remote_input::{build_selector_click_script, decode_point_click_result};
        use crate::remote::browser_backend::RemoteBrowserError;
        let script = build_selector_click_script(r##""#btn""##);
        assert!(
            script.contains(r#"ok: false, code: "not_found""#),
            "selector-click script must tag a missing target with code not_found: {script}"
        );
        let missing = decode_point_click_result(
            r#"{"ok":false,"code":"not_found","error":"element not found"}"#,
        )
        .unwrap_err();
        assert!(matches!(
            point_click_decode_error_to_remote(missing),
            RemoteBrowserError::TargetNotFound(_)
        ));
        let untagged =
            decode_point_click_result(r#"{"ok":false,"error":"element not found"}"#).unwrap_err();
        assert!(matches!(
            point_click_decode_error_to_remote(untagged),
            RemoteBrowserError::ExecutionFailed(_)
        ));
    }

    #[tokio::test]
    async fn linked_popup_navigation_refuses_file_urls() {
        let _guard = crate::browser::popup::registry_test_lock().lock().await;
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());
        let popup = manager
            .register_session(crate::browser::model::CreateBrowserRequest {
                browser_id: Some("b-popup-nav-target".into()),
                workspace_id: Some("ws1".into()),
                worktree_path: None,
                url: "https://example.com/".into(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .unwrap();
        assert!(crate::browser::popup::link(&popup.browser_id, "b-popup-nav-opener", "h1"));
        let _webview = open_mock_webview(&app, &popup.webview_label);
        let refused = navigate_linked_popup(
            app.handle(),
            &manager,
            "b-popup-nav-opener",
            "h1",
            "file:///tmp/x.html",
        )
        .await;
        assert!(refused.is_err(), "a page must not load file:// into its popup");
        assert_eq!(manager.get_state(&popup.browser_id).unwrap().url, "https://example.com/");
        navigate_linked_popup(app.handle(), &manager, "b-popup-nav-opener", "h1", "https://example.org/cb")
            .await
            .expect("http(s) popup navigation is allowed");
        assert_eq!(manager.get_state(&popup.browser_id).unwrap().url, "https://example.org/cb");
        crate::browser::popup::unlink(&popup.browser_id);
    }

    #[tokio::test]
    async fn remote_executor_navigate_rejects_local_file_url() {
        use crate::remote::browser_backend::{
            BrowserCommandContext, BrowserCommandExecutor, RemoteBrowserError,
        };
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());
        let state = manager
            .register_session(crate::browser::model::CreateBrowserRequest {
                browser_id: Some("b-remote-nav".into()),
                workspace_id: Some("ws1".into()),
                worktree_path: None,
                url: "https://example.com".into(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .unwrap();
        let _webview = open_mock_webview(&app, &state.webview_label);
        let executor = GuiBrowserCommandExecutor::new(app.handle().clone(), Arc::clone(&manager));
        let res = executor
            .execute(BrowserCommandContext {
                browser_id: state.browser_id.clone(),
                command: "navigate".into(),
                params: Some(serde_json::json!({ "url": "file:///tmp/x.html" })),
                ..Default::default()
            })
            .await;
        assert!(
            matches!(res, Err(RemoteBrowserError::Forbidden(ref m)) if m.contains("file:///tmp/x.html")),
            "remote navigate to a local file must be denied by the remote URL policy, got {res:?}"
        );
        assert_eq!(manager.get_state(&state.browser_id).unwrap().url, state.url);
        // A malformed URL is a bad request, not a policy denial.
        let malformed = executor
            .execute(BrowserCommandContext {
                browser_id: state.browser_id.clone(),
                command: "navigate".into(),
                params: Some(serde_json::json!({ "url": "https://user:pw@example.com/" })),
                ..Default::default()
            })
            .await;
        assert!(
            matches!(malformed, Err(RemoteBrowserError::InvalidRequest(_))),
            "remote navigate to a malformed URL must be InvalidRequest, got {malformed:?}"
        );
        // Local callers keep the local policy.
        navigate_browser_session(app.handle(), &manager, &state.browser_id, "file:///tmp/x.html")
            .await
            .expect("local navigate keeps the local file policy");
    }

    #[tokio::test]
    async fn navigate_without_webview_fails_and_leaves_state_untouched() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());
        let state = manager
            .register_session(crate::browser::model::CreateBrowserRequest {
                browser_id: Some("b-nav-no-webview".into()),
                workspace_id: Some("ws1".into()),
                worktree_path: None,
                url: "https://example.com/".into(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .unwrap();
        let before = manager.get_state(&state.browser_id).unwrap();
        let err = navigate_browser_session(app.handle(), &manager, &state.browser_id, "https://example.org/")
            .await
            .expect_err("navigation without a webview must fail");
        assert_eq!(err.code, IpcErrorCode::WebviewNotFound);
        let after = manager.get_state(&state.browser_id).unwrap();
        assert_eq!(after.url, before.url);
        assert_eq!(after.generation, before.generation);
        assert_eq!(after.loading, before.loading);
    }

    #[tokio::test(start_paused = true)]
    async fn wait_reports_a_closed_tab_instead_of_timing_out() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());
        let state = manager
            .register_session(crate::browser::model::CreateBrowserRequest {
                browser_id: Some("b-wait-closed".into()),
                workspace_id: Some("ws1".into()),
                worktree_path: None,
                url: "https://example.com/".into(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .unwrap();
        let _webview = open_mock_webview(&app, &state.webview_label);
        let handle = app.handle().clone();
        let mut wait = Box::pin(wait_browser_session_with_timeout(
            &handle,
            &manager,
            &state.browser_id,
            crate::browser::model::BrowserWaitCondition::Selector { selector: "#never".into() },
            Some(std::time::Duration::from_secs(60)),
        ));
        // The mock runtime drops every eval callback, so each poll's eval fails at once; with
        // the tab still open the wait retries and parks on its poll interval.
        assert!(futures_util::poll!(wait.as_mut()).is_pending());
        app.get_webview(&state.webview_label)
            .expect("webview is open")
            .close()
            .expect("close webview");
        let err = tokio::time::timeout(std::time::Duration::from_secs(600), wait)
            .await
            .expect("the wait must settle")
            .expect_err("a closed tab must fail the wait");
        assert_eq!(err.code, IpcErrorCode::WebviewNotFound);
    }

    #[tokio::test]
    async fn create_without_main_window_fails_and_registers_nothing() {
        use tauri::Manager;
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        assert!(app.get_window("main").is_none(), "the mock app has no main window");
        let manager = Arc::new(BrowserManager::new());
        let err = create_browser_session(
            app.handle(),
            &manager,
            crate::browser::model::CreateBrowserRequest {
                browser_id: Some("b-no-main".into()),
                workspace_id: Some("ws1".into()),
                worktree_path: None,
                url: "https://example.com".into(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            },
        )
        .await
        .expect_err("creation without a main window must fail");
        assert_eq!(err.code, IpcErrorCode::BrowserCreateFailed);
        assert!(manager.get_state("b-no-main").is_err());
        assert!(!CREATED_SESSION_IDS.lock().iter().any(|id| id == "b-no-main"));
    }

    #[tokio::test]
    async fn state_load_refuses_storage_from_another_origin() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());
        let state = manager
            .register_session(crate::browser::model::CreateBrowserRequest {
                browser_id: Some("b-state-origin".into()),
                workspace_id: Some("ws1".into()),
                worktree_path: None,
                url: "https://b.example/".into(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .unwrap();
        let _webview = open_mock_webview(&app, &state.webview_label);
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("state.json");
        std::fs::write(
            &path,
            r#"{"version":2,"browserId":"b-saved","url":"https://a.example/","localStorage":{"token":"secret"}}"#,
        )
        .expect("write state");
        let err = load_browser_state_session(
            app.handle(),
            &manager,
            &state.browser_id,
            path.to_str().unwrap(),
        )
        .await
        .expect_err("another origin's storage must not be restored here");
        assert_eq!(err.code, IpcErrorCode::InvalidArgument);
    }

    #[test]
    fn history_navigation_without_webview_reports_webview_not_found_and_keeps_state() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());
        let state = manager
            .register_session(crate::browser::model::CreateBrowserRequest {
                browser_id: Some("b-history-no-webview".into()),
                workspace_id: Some("ws1".into()),
                worktree_path: None,
                url: "https://example.com/".into(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .unwrap();
        manager
            .update_url_with_context(
                &state.browser_id,
                "https://example.org/",
                crate::browser::security::BrowserSecurityContext::Local,
            )
            .unwrap();
        manager
            .update_navigation_state(&state.browser_id, None, None, Some(false), None, None, None)
            .unwrap();
        let before = manager.get_state(&state.browser_id).unwrap();
        let err = history_navigation(app.handle(), &manager, &state.browser_id, false)
            .expect_err("history navigation without a webview must fail");
        assert_eq!(err.code, IpcErrorCode::WebviewNotFound);
        let after = manager.get_state(&state.browser_id).unwrap();
        assert_eq!(after.url, before.url);
        assert_eq!(after.generation, before.generation);
        assert_eq!(after.loading, before.loading);
    }

    #[tokio::test]
    async fn remote_reload_without_webview_is_not_found_and_not_left_loading() {
        use crate::remote::browser_backend::{
            BrowserCommandContext, BrowserCommandExecutor, RemoteBrowserError,
        };
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());
        let state = manager
            .register_session(crate::browser::model::CreateBrowserRequest {
                browser_id: Some("b-remote-reload".into()),
                workspace_id: Some("ws1".into()),
                worktree_path: None,
                url: "https://example.com/".into(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .unwrap();
        manager
            .update_navigation_state(&state.browser_id, None, None, Some(false), None, None, None)
            .unwrap();
        let before = manager.get_state(&state.browser_id).unwrap();
        let executor = GuiBrowserCommandExecutor::new(app.handle().clone(), Arc::clone(&manager));
        let res = executor
            .execute(BrowserCommandContext {
                browser_id: state.browser_id.clone(),
                command: "reload".into(),
                ..Default::default()
            })
            .await;
        assert!(
            matches!(res, Err(RemoteBrowserError::NotFound(_))),
            "a missing webview must map to NotFound, got {res:?}"
        );
        let after = manager.get_state(&state.browser_id).unwrap();
        assert_eq!(after.generation, before.generation);
        assert!(!after.loading);
    }

    #[test]
    fn ipc_errors_map_to_remote_errors_by_code() {
        use crate::remote::browser_backend::RemoteBrowserError;
        let map = |code| ipc_to_remote(IpcError::new(code, "m"));
        assert!(matches!(map(IpcErrorCode::BrowserNotFound), RemoteBrowserError::NotFound(_)));
        assert!(matches!(map(IpcErrorCode::WebviewNotFound), RemoteBrowserError::NotFound(_)));
        assert!(matches!(
            map(IpcErrorCode::BrowserUrlSchemeDenied),
            RemoteBrowserError::Forbidden(_)
        ));
        assert!(matches!(
            map(IpcErrorCode::BrowserUrlInvalid),
            RemoteBrowserError::InvalidRequest(_)
        ));
        assert!(matches!(
            map(IpcErrorCode::BrowserInvalidReference),
            RemoteBrowserError::TargetNotFound(_)
        ));
        assert!(matches!(map(IpcErrorCode::BrowserWaitTimeout), RemoteBrowserError::WaitTimeout));
        assert!(matches!(
            map(IpcErrorCode::BrowserAutomationFailed),
            RemoteBrowserError::ExecutionFailed(_)
        ));
    }

    #[tokio::test]
    async fn file_cookie_import_reports_profile_and_tab_failures_like_installed_import() {
        use tauri::Manager;
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        app.manage(Arc::new(BrowserManager::new()));
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("cookies.txt");
        std::fs::write(&file, ".example.com\tTRUE\t/\tFALSE\t0\tsid\tabc\n")
            .expect("write cookie file");
        let import = |profile_id: &str| ImportBrowserCookiesRequest {
            profile_id: profile_id.into(),
            file_path: file.to_string_lossy().into_owned(),
        };
        let reason = |e: &IpcError| {
            e.details
                .as_ref()
                .and_then(|d| d.get("reason"))
                .and_then(|r| r.as_str())
                .map(str::to_owned)
        };

        let bad = cmd_browser_import_cookies(
            app.handle().clone(),
            app.state::<Arc<BrowserManager>>(),
            import("not a valid profile!"),
        )
        .await
        .expect_err("an unknown profile must fail");
        assert_eq!(bad.code, IpcErrorCode::BrowserCookieImportFailed);
        assert_eq!(reason(&bad).as_deref(), Some("no-profile"));

        let no_tab = cmd_browser_import_cookies(
            app.handle().clone(),
            app.state::<Arc<BrowserManager>>(),
            import("default"),
        )
        .await
        .expect_err("a profile with no open tab must fail");
        assert_eq!(no_tab.code, IpcErrorCode::BrowserCookieImportFailed);
        assert_eq!(reason(&no_tab).as_deref(), Some("no-tab"));
    }

    #[tokio::test]
    async fn remote_executor_keypress_rejects_control_character_key() {
        use crate::remote::browser_backend::{
            BrowserCommandContext, BrowserCommandExecutor, RemoteBrowserError,
        };
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());
        let state = manager
            .register_session(crate::browser::model::CreateBrowserRequest {
                browser_id: Some("b-remote-key".into()),
                workspace_id: Some("ws1".into()),
                worktree_path: None,
                url: "https://example.com".into(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .unwrap();
        let executor = GuiBrowserCommandExecutor::new(app.handle().clone(), Arc::clone(&manager));
        let res = executor
            .execute(BrowserCommandContext {
                browser_id: state.browser_id.clone(),
                command: "keypress".into(),
                params: Some(serde_json::json!({ "key": "\u{1b}" })),
                ..Default::default()
            })
            .await;
        assert!(
            matches!(res, Err(RemoteBrowserError::InvalidRequest(_))),
            "remote keypress with a control character must be InvalidRequest, got {res:?}"
        );
    }

    #[test]
    fn remote_keypress_dispatches_the_validated_trimmed_key() {
        let script = remote_keypress_script(" a ").expect("padded single char is allowed");
        assert!(
            script.contains(r#"key: "a","#),
            "must dispatch the trimmed key that was validated, got {script}"
        );
        assert!(!script.contains(r#"" a ""#), "raw padded key must not be dispatched");
        assert!(remote_keypress_script("\u{1b}").is_err());
    }

    #[tokio::test]
    async fn remote_executor_refuses_unparseable_document_generation_fence() {
        use crate::remote::browser_backend::{
            BrowserCommandContext, BrowserCommandExecutor, RemoteBrowserError,
        };
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());
        let state = manager
            .register_session(crate::browser::model::CreateBrowserRequest {
                browser_id: Some("b-remote-fence".into()),
                workspace_id: Some("ws1".into()),
                worktree_path: None,
                url: "https://example.com".into(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .unwrap();
        let executor = GuiBrowserCommandExecutor::new(app.handle().clone(), Arc::clone(&manager));
        let res = executor
            .execute(BrowserCommandContext {
                browser_id: state.browser_id.clone(),
                command: "getState".into(),
                document_generation: Some("abc".into()),
                ..Default::default()
            })
            .await;
        assert!(
            matches!(res, Err(RemoteBrowserError::InvalidRequest(_))),
            "an unparseable documentGeneration must refuse, got {res:?}"
        );
    }

    #[test]
    fn viewport_revision_fence_refuses_unparseable_values() {
        use crate::remote::browser_backend::RemoteBrowserError;
        assert_eq!(expected_viewport_revision(&serde_json::json!({})).unwrap(), None);
        assert_eq!(
            expected_viewport_revision(&serde_json::json!({ "viewportRevision": "7" })).unwrap(),
            Some(7)
        );
        assert_eq!(
            expected_viewport_revision(&serde_json::json!({ "viewport_revision": 9 })).unwrap(),
            Some(9)
        );
        for bad in [serde_json::json!("abc"), serde_json::json!(-1), serde_json::json!(1.5)] {
            assert!(matches!(
                expected_viewport_revision(&serde_json::json!({ "viewportRevision": bad })),
                Err(RemoteBrowserError::InvalidRequest(_))
            ));
        }
    }

    /// Fill and click decode through one variant-based mapping: a typed refusal is
    /// `InputRefused` on every path, never a generic execution failure.
    #[test]
    fn fill_decode_refusal_maps_like_click_refusal() {
        use crate::browser::remote_input::{ActionDecodeError, RefusedInputToken};
        use crate::remote::browser_backend::RemoteBrowserError;
        let refused = ActionDecodeError::Refused(RefusedInputToken::Iframe.to_input_error());
        assert!(matches!(
            fill_decode_error_to_remote(refused),
            RemoteBrowserError::InputRefused(_)
        ));
    }

    #[test]
    fn failed_named_profile_step_discards_registered_session() {
        let manager = BrowserManager::new();
        let request = |id: &str| crate::browser::model::CreateBrowserRequest {
            browser_id: Some(id.into()),
            workspace_id: Some("ws1".into()),
            worktree_path: None,
            url: "https://example.com".into(),
            profile: None,
            zoom_factor: None,
            bounds: None,
            visible: Some(true),
        };
        let tracked = |id: &str| CREATED_SESSION_IDS.lock().iter().any(|t| t == id);

        let failed = manager.register_session(request("b-profile-dir-fail")).unwrap();
        record_session_created(&failed.browser_id);
        let res: Result<(), BrowserError> = discard_session_on_error(
            &manager,
            &failed.browser_id,
            Err(BrowserError::CreateFailed("profile dir unavailable".into())),
        );
        assert!(res.is_err());
        assert!(manager.get_state(&failed.browser_id).is_err());
        assert!(!tracked(&failed.browser_id));

        let kept = manager.register_session(request("b-profile-dir-ok")).unwrap();
        record_session_created(&kept.browser_id);
        let ok: Result<u8, BrowserError> = discard_session_on_error(&manager, &kept.browser_id, Ok(1));
        assert_eq!(ok, Ok(1));
        assert!(manager.get_state(&kept.browser_id).is_ok());
        assert!(tracked(&kept.browser_id));
        discard_failed_session(&manager, &kept.browser_id);
    }

    #[tokio::test]
    async fn test_r6_5_cmd_browser_remote_reclaim_rejects_missing_transport_in_daemon_mode() {
        use tauri::Manager;
        let daemon_client = Arc::new(crate::daemon::client::DaemonClient::new());
        let mgr = Arc::new(crate::ipc::remote::RemoteGatewayManager::from_daemon(
            daemon_client,
        ));
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        app.manage(mgr);

        // In daemon mode without transport or PureGuiMode flag, reclaim MUST be rejected!
        let res = cmd_browser_remote_reclaim(app.handle().clone()).await;
        assert_eq!(
            res.expect_err("Must reject missing DaemonReclaimTransport in daemon mode").code,
            IpcErrorCode::BrowserHostUnavailable
        );
    }

    #[tokio::test]
    async fn test_r6_5_pure_gui_mode_allows_local_fallback() {
        use tauri::Manager;
        let daemon_client = Arc::new(crate::daemon::client::DaemonClient::new());
        let mgr = Arc::new(crate::ipc::remote::RemoteGatewayManager::from_daemon(
            daemon_client,
        ));
        let broker = Arc::new(crate::browser::remote_driver::RemoteDriverBroker::new());
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        app.manage(mgr);
        app.manage(broker);
        app.manage(PureGuiMode(true));

        let res = cmd_browser_remote_reclaim(app.handle().clone()).await;
        assert!(
            res.is_ok(),
            "PureGuiMode(true) must allow local broker fallback"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn production_reclaim_transport_fails_when_daemon_is_unreachable() {
        let directory = tempfile::tempdir().expect("tempdir");
        // No daemon listens here, and an existing-only client never spawns one.
        let daemon_client = Arc::new(crate::daemon::client::DaemonClient::existing_only_at(
            directory.path().join("daemon.sock"),
            std::time::Duration::ZERO,
        ));
        let remote_manager = Arc::new(crate::ipc::remote::RemoteGatewayManager::from_daemon(
            Arc::clone(&daemon_client),
        ));
        let transport = ProductionDaemonReclaimTransport::new(remote_manager, daemon_client);
        let res = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            transport.reclaim_daemon_broker(),
        )
        .await
        .expect("reclaim against an absent daemon must settle");
        assert!(
            res.is_err(),
            "reclaim must not report success when the daemon could not revoke control, got {res:?}"
        );
    }

    #[tokio::test]
    async fn test_r6_5_daemon_reclaim_transport_routes_authoritatively() {
        use tauri::Manager;
        struct MockTransport(std::sync::atomic::AtomicU64);
        impl DaemonReclaimTransport for MockTransport {
            fn reclaim_daemon_broker<'a>(
                &'a self,
            ) -> std::pin::Pin<
                Box<dyn std::future::Future<Output = Result<u64, IpcError>> + Send + 'a>,
            > {
                Box::pin(
                    async move { Ok(self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst)) },
                )
            }
        }

        let daemon_client = Arc::new(crate::daemon::client::DaemonClient::new());
        let mgr = Arc::new(crate::ipc::remote::RemoteGatewayManager::from_daemon(
            daemon_client,
        ));
        let transport: Arc<dyn DaemonReclaimTransport> =
            Arc::new(MockTransport(std::sync::atomic::AtomicU64::new(77)));
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        app.manage(mgr);
        app.manage(transport);

        let res = cmd_browser_remote_reclaim(app.handle().clone()).await;
        assert_eq!(
            res.unwrap(),
            77,
            "Must return epoch from registered DaemonReclaimTransport"
        );
    }
}

#[cfg(test)]
mod popup_handle_tests {
    use super::*;

    #[tokio::test]
    async fn test_popup_handle_close_targets_linked_popup_not_opener() {
        use tauri::Listener;

        let _guard = crate::browser::popup::registry_test_lock().lock().await;
        crate::browser::popup::clear_all();
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());

        let opener_id = "browser-opener-1";
        let popup_id = "browser-popup-1";
        let handle_id = "p1";

        // Register listener BEFORE calling close_linked_popup to capture the emitted close event
        let (event_tx, mut event_rx) =
            tokio::sync::mpsc::unbounded_channel::<crate::browser::model::BrowserCloseRequestedPayload>();
        app.listen(
            crate::browser::guest::BROWSER_CLOSE_REQUESTED_EVENT,
            move |event: tauri::Event| {
                let payload = serde_json::from_str::<crate::browser::model::BrowserCloseRequestedPayload>(
                    event.payload(),
                )
                .expect("close event payload must deserialize to BrowserCloseRequestedPayload");
                event_tx
                    .send(payload)
                    .expect("close event receiver must remain active to receive emission");
            },
        );

        // Link popup-1 to opener-1 under handle p1.
        crate::browser::popup::link(popup_id, opener_id, handle_id);
        assert_eq!(
            crate::browser::popup::popup_for_handle(opener_id, handle_id).as_deref(),
            Some(popup_id)
        );

        // When close_linked_popup is called with (opener_id, handle_id),
        // it must emit BROWSER_CLOSE_REQUESTED_EVENT targeting popup_id, NEVER opener_id!
        close_linked_popup(app.handle(), &manager, opener_id, handle_id).await;

        // Await event with a bounded timeout to eliminate timing luck from asynchronous dispatch
        let emitted = tokio::time::timeout(std::time::Duration::from_secs(5), event_rx.recv())
            .await
            .expect("close event emission timed out")
            .expect("close event channel closed without event");

        assert_eq!(
            emitted.browser_id, popup_id,
            "emitted close request must target the child popup ({popup_id}), not opener ({opener_id})"
        );
        assert_ne!(
            emitted.browser_id, opener_id,
            "opener must never be targeted by popup handle close"
        );

        // The popup is now unlinked, and was targeted:
        assert_eq!(
            crate::browser::popup::popup_for_handle(opener_id, handle_id),
            None
        );

        crate::browser::popup::clear_all();
    }

    #[tokio::test]
    async fn test_popup_handle_close_race_records_pending_close_and_discards_late_creation() {
        let _guard = crate::browser::popup::registry_test_lock().lock().await;
        crate::browser::popup::clear_all();
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());

        let opener_id = "browser-opener-race";
        let handle_id = "p_race_1";

        // Close request arrives BEFORE the popup finishes creation:
        close_linked_popup(app.handle(), &manager, opener_id, handle_id).await;

        // Pending close is recorded:
        assert!(crate::browser::popup::is_pending_close(opener_id, handle_id));

        // When late creation attempts to link, it is rejected:
        let linked = crate::browser::popup::link("browser-popup-late", opener_id, handle_id);
        assert!(!linked, "late creation must be rejected when pending close exists");

        // The link registry does NOT register the late popup:
        assert_eq!(
            crate::browser::popup::popup_for_handle(opener_id, handle_id),
            None
        );
        assert_eq!(crate::browser::popup::lookup("browser-popup-late"), None);

        crate::browser::popup::clear_all();
    }

    #[tokio::test]
    async fn closing_an_opener_drops_its_pending_closes_and_popup_links() {
        let _guard = crate::browser::popup::registry_test_lock().lock().await;
        crate::browser::popup::clear_all();
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());
        let opener = manager
            .register_session(crate::browser::model::CreateBrowserRequest {
                browser_id: Some("browser-opener-closing".into()),
                workspace_id: Some("ws1".into()),
                worktree_path: None,
                url: "https://example.com".into(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .unwrap();
        crate::browser::popup::record_pending_close(&opener.browser_id, "never-created");
        assert!(crate::browser::popup::link("browser-popup-orphan", &opener.browser_id, "p1"));

        close_browser_session(app.handle(), &manager, &opener.browser_id)
            .await
            .expect("close the opener");

        assert!(!crate::browser::popup::is_pending_close(&opener.browser_id, "never-created"));
        assert_eq!(crate::browser::popup::lookup("browser-popup-orphan"), None);
        crate::browser::popup::clear_all();
    }

}
