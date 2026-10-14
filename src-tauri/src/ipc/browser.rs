use crate::browser::{
    browser_find_script, browser_guest_bridge_script, cookie_from_imported, download_url_to_path,
    parse_browser_find_callback, parse_browser_guest_action, parse_cookie_file,
    BrowserAutomationAction, BrowserAutomationElement, BrowserAutomationRequest,
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
        BrowserAutomationAction::Dblclick { .. } => {
            let selector = serde_json::to_string(selector.ok_or_else(|| {
                BrowserError::AutomationFailed("missing snapshot selector".into())
            })?)
            .map_err(|error| BrowserError::AutomationFailed(error.to_string()))?;
            Ok(format!(
                "(() => {{ const element = document.querySelector({selector}); if (!element) throw new Error('element disappeared'); element.dispatchEvent(new MouseEvent('dblclick', {{ bubbles: true, cancelable: true, view: window }})); return 'ok'; }})()"
            ))
        }
        BrowserAutomationAction::Hover { .. } => {
            let selector = serde_json::to_string(selector.ok_or_else(|| {
                BrowserError::AutomationFailed("missing snapshot selector".into())
            })?)
            .map_err(|error| BrowserError::AutomationFailed(error.to_string()))?;
            Ok(format!(
                "(() => {{ const element = document.querySelector({selector}); if (!element) throw new Error('element disappeared'); element.dispatchEvent(new MouseEvent('mouseover', {{ bubbles: true, cancelable: true, view: window }})); element.dispatchEvent(new MouseEvent('mouseenter', {{ bubbles: false, cancelable: false, view: window }})); element.dispatchEvent(new MouseEvent('mousemove', {{ bubbles: true, cancelable: true, view: window }})); return 'ok'; }})()"
            ))
        }
        BrowserAutomationAction::Focus { .. } => {
            let selector = serde_json::to_string(selector.ok_or_else(|| {
                BrowserError::AutomationFailed("missing snapshot selector".into())
            })?)
            .map_err(|error| BrowserError::AutomationFailed(error.to_string()))?;
            Ok(format!(
                "(() => {{ const element = document.querySelector({selector}); if (!element) throw new Error('element disappeared'); element.focus(); return 'ok'; }})()"
            ))
        }
        BrowserAutomationAction::Check { .. } => {
            let selector = serde_json::to_string(selector.ok_or_else(|| {
                BrowserError::AutomationFailed("missing snapshot selector".into())
            })?)
            .map_err(|error| BrowserError::AutomationFailed(error.to_string()))?;
            Ok(format!(
                "(() => {{ const element = document.querySelector({selector}); if (!element) throw new Error('element disappeared'); if (!element.checked) {{ element.checked = true; element.dispatchEvent(new Event('input', {{ bubbles: true }})); element.dispatchEvent(new Event('change', {{ bubbles: true }})); }} if (element.getAttribute('aria-checked') === 'false') {{ element.setAttribute('aria-checked', 'true'); }} return 'ok'; }})()"
            ))
        }
        BrowserAutomationAction::Uncheck { .. } => {
            let selector = serde_json::to_string(selector.ok_or_else(|| {
                BrowserError::AutomationFailed("missing snapshot selector".into())
            })?)
            .map_err(|error| BrowserError::AutomationFailed(error.to_string()))?;
            Ok(format!(
                "(() => {{ const element = document.querySelector({selector}); if (!element) throw new Error('element disappeared'); if (element.checked) {{ element.checked = false; element.dispatchEvent(new Event('input', {{ bubbles: true }})); element.dispatchEvent(new Event('change', {{ bubbles: true }})); }} if (element.getAttribute('aria-checked') === 'true') {{ element.setAttribute('aria-checked', 'false'); }} return 'ok'; }})()"
            ))
        }
        BrowserAutomationAction::ScrollIntoView { .. } => {
            let selector = serde_json::to_string(selector.ok_or_else(|| {
                BrowserError::AutomationFailed("missing snapshot selector".into())
            })?)
            .map_err(|error| BrowserError::AutomationFailed(error.to_string()))?;
            Ok(format!(
                "(() => {{ const element = document.querySelector({selector}); if (!element) throw new Error('element disappeared'); element.scrollIntoView({{ behavior: 'instant', block: 'center', inline: 'center' }}); return 'ok'; }})()"
            ))
        }
        BrowserAutomationAction::Select { value, .. } => {
            let selector = serde_json::to_string(selector.ok_or_else(|| {
                BrowserError::AutomationFailed("missing snapshot selector".into())
            })?)
            .map_err(|error| BrowserError::AutomationFailed(error.to_string()))?;
            let value_json = serde_json::to_string(value)
                .map_err(|error| BrowserError::AutomationFailed(error.to_string()))?;
            Ok(format!(
                "(() => {{ const element = document.querySelector({selector}); if (!element) throw new Error('element disappeared'); element.focus(); let matched = false; if (element.tagName === 'SELECT') {{ for (const opt of element.options) {{ if (opt.value === {value_json} || opt.text === {value_json}) {{ opt.selected = true; matched = true; break; }} }} }} if (!matched && element.value !== undefined) {{ element.value = {value_json}; }} element.dispatchEvent(new Event('input', {{ bubbles: true }})); element.dispatchEvent(new Event('change', {{ bubbles: true }})); return 'ok'; }})()"
            ))
        }
        BrowserAutomationAction::Scroll { x, y, .. } => {
            let dx = x.unwrap_or(0);
            let dy = y.unwrap_or(0);
            if let Some(sel) = selector {
                let selector = serde_json::to_string(sel)
                    .map_err(|error| BrowserError::AutomationFailed(error.to_string()))?;
                Ok(format!(
                    "(() => {{ const element = document.querySelector({selector}); if (!element) throw new Error('element disappeared'); element.scrollBy({{ left: {dx}, top: {dy}, behavior: 'instant' }}); return 'ok'; }})()"
                ))
            } else {
                Ok(format!(
                    "(() => {{ window.scrollBy({{ left: {dx}, top: {dy}, behavior: 'instant' }}); return 'ok'; }})()"
                ))
            }
        }
        BrowserAutomationAction::Type { text, .. } => {
            let text_json = serde_json::to_string(text)
                .map_err(|error| BrowserError::AutomationFailed(error.to_string()))?;
            if let Some(sel) = selector {
                let selector = serde_json::to_string(sel)
                    .map_err(|error| BrowserError::AutomationFailed(error.to_string()))?;
                Ok(format!(
                    "(() => {{ const element = document.querySelector({selector}); if (!element) throw new Error('element disappeared'); element.focus(); const str = {text_json}; for (const ch of str) {{ const init = {{ bubbles: true, cancelable: true, key: ch }}; element.dispatchEvent(new KeyboardEvent('keydown', init)); element.dispatchEvent(new KeyboardEvent('keypress', init)); if (element.value !== undefined) {{ element.value = (element.value || '') + ch; element.dispatchEvent(new Event('input', {{ bubbles: true }})); }} else if (element.isContentEditable) {{ document.execCommand('insertText', false, ch); }} element.dispatchEvent(new KeyboardEvent('keyup', init)); }} if (element.value !== undefined) {{ element.dispatchEvent(new Event('change', {{ bubbles: true }})); }} return 'ok'; }})()"
                ))
            } else {
                Ok(format!(
                    "(() => {{ const element = document.activeElement || document.body; if (!element) throw new Error('element disappeared'); element.focus(); const str = {text_json}; for (const ch of str) {{ const init = {{ bubbles: true, cancelable: true, key: ch }}; element.dispatchEvent(new KeyboardEvent('keydown', init)); element.dispatchEvent(new KeyboardEvent('keypress', init)); if (element.value !== undefined) {{ element.value = (element.value || '') + ch; element.dispatchEvent(new Event('input', {{ bubbles: true }})); }} else if (element.isContentEditable) {{ document.execCommand('insertText', false, ch); }} element.dispatchEvent(new KeyboardEvent('keyup', init)); }} if (element.value !== undefined) {{ element.dispatchEvent(new Event('change', {{ bubbles: true }})); }} return 'ok'; }})()"
                ))
            }
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

    #[test]
    fn extended_automation_actions_script_generation() {
        // dblclick
        let dblclick = automation_script(
            &BrowserAutomationAction::Dblclick {
                reference: "row1".into(),
            },
            Some("#row1"),
        )
        .expect("dblclick script");
        assert!(dblclick.contains("querySelector(\"#row1\")"));
        assert!(dblclick.contains("new MouseEvent('dblclick'"));

        // hover
        let hover = automation_script(
            &BrowserAutomationAction::Hover {
                reference: "menu".into(),
            },
            Some(".menu-trigger"),
        )
        .expect("hover script");
        assert!(hover.contains("querySelector(\".menu-trigger\")"));
        assert!(hover.contains("mouseover"));
        assert!(hover.contains("mouseenter"));
        assert!(hover.contains("mousemove"));

        // focus
        let focus = automation_script(
            &BrowserAutomationAction::Focus {
                reference: "input".into(),
            },
            Some("#email"),
        )
        .expect("focus script");
        assert!(focus.contains("querySelector(\"#email\")"));
        assert!(focus.contains("element.focus()"));

        // check
        let check = automation_script(
            &BrowserAutomationAction::Check {
                reference: "chk".into(),
            },
            Some("#accept-terms"),
        )
        .expect("check script");
        assert!(check.contains("querySelector(\"#accept-terms\")"));
        assert!(check.contains("element.checked = true"));
        assert!(check.contains("new Event('change'"));

        // uncheck
        let uncheck = automation_script(
            &BrowserAutomationAction::Uncheck {
                reference: "chk".into(),
            },
            Some("#accept-terms"),
        )
        .expect("uncheck script");
        assert!(uncheck.contains("querySelector(\"#accept-terms\")"));
        assert!(uncheck.contains("element.checked = false"));
        assert!(uncheck.contains("new Event('change'"));

        // scroll-into-view
        let scroll_into_view = automation_script(
            &BrowserAutomationAction::ScrollIntoView {
                reference: "footer".into(),
            },
            Some("#site-footer"),
        )
        .expect("scrollIntoView script");
        assert!(scroll_into_view.contains("querySelector(\"#site-footer\")"));
        assert!(scroll_into_view.contains("element.scrollIntoView"));

        // select
        let select = automation_script(
            &BrowserAutomationAction::Select {
                reference: "country".into(),
                value: "US".into(),
            },
            Some("#country-select"),
        )
        .expect("select script");
        assert!(select.contains("querySelector(\"#country-select\")"));
        assert!(select.contains("opt.value === \"US\""));
        assert!(select.contains("new Event('change'"));

        // scroll (with and without element selector)
        let scroll_elem = automation_script(
            &BrowserAutomationAction::Scroll {
                reference: Some("pane".into()),
                x: Some(10),
                y: Some(50),
            },
            Some("#scroll-pane"),
        )
        .expect("scroll elem script");
        assert!(scroll_elem.contains("querySelector(\"#scroll-pane\")"));
        assert!(scroll_elem.contains("element.scrollBy({ left: 10, top: 50"));

        let scroll_win = automation_script(
            &BrowserAutomationAction::Scroll {
                reference: None,
                x: Some(0),
                y: Some(200),
            },
            None,
        )
        .expect("scroll window script");
        assert!(scroll_win.contains("window.scrollBy({ left: 0, top: 200"));

        // type (with and without element selector)
        let type_elem = automation_script(
            &BrowserAutomationAction::Type {
                reference: Some("txt".into()),
                text: "hello".into(),
            },
            Some("#name-input"),
        )
        .expect("type elem script");
        assert!(type_elem.contains("querySelector(\"#name-input\")"));
        assert!(type_elem.contains("element.focus()"));
        assert!(type_elem.contains("new KeyboardEvent('keydown'"));
        assert!(type_elem.contains("element.value = (element.value || '') + ch"));

        let type_active = automation_script(
            &BrowserAutomationAction::Type {
                reference: None,
                text: "world".into(),
            },
            None,
        )
        .expect("type active script");
        assert!(type_active.contains("document.activeElement || document.body"));
        assert!(type_active.contains("element.focus()"));
    }

    #[test]
    fn extended_automation_actions_require_snapshot_selector() {
        let require_selector = vec![
            BrowserAutomationAction::Dblclick { reference: "e1".into() },
            BrowserAutomationAction::Hover { reference: "e1".into() },
            BrowserAutomationAction::Focus { reference: "e1".into() },
            BrowserAutomationAction::Check { reference: "e1".into() },
            BrowserAutomationAction::Uncheck { reference: "e1".into() },
            BrowserAutomationAction::ScrollIntoView { reference: "e1".into() },
            BrowserAutomationAction::Select { reference: "e1".into(), value: "v".into() },
        ];

        for action in require_selector {
            let error = automation_script(&action, None)
                .expect_err("action requiring element must fail without selector");
            assert_eq!(
                error,
                BrowserError::AutomationFailed("missing snapshot selector".into())
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

/// Drops a session whose webview creation failed.
fn discard_failed_session(manager: &BrowserManager, browser_id: &str) {
    manager.remove_session(browser_id);
    CREATED_SESSION_IDS.lock().retain(|id| id != browser_id);
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
    record_session_created(&state.browser_id);

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
        let page_manager = Arc::clone(manager);
        let title_manager = Arc::clone(manager);
        let creation_manager = Arc::clone(manager);
        let bridge_manager = Arc::clone(manager);
        let page_browser_id = browser_id.clone();
        let title_browser_id = browser_id.clone();

        let guest_bridge_nonce = uuid::Uuid::new_v4().to_string();
        let nonce = guest_bridge_nonce.clone();
        let is_popup = opener.is_some();
        let eval_bridge_script = browser_guest_bridge_script(&guest_bridge_nonce, is_popup);
        let page_load_bridge_script = eval_bridge_script.clone();
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
            let parsed_url: tauri::WebviewUrl = if let Ok(u) = target_url.parse() {
                tauri::WebviewUrl::External(u)
            } else {
                tauri::WebviewUrl::App("about:blank".into())
            };

            let builder = tauri::WebviewBuilder::new(label, parsed_url)
                .user_agent(crate::browser::default_desktop_user_agent())
                .incognito(incognito)
                .devtools(true)
                .initialization_script(browser_guest_bridge_script(&guest_bridge_nonce, is_popup))
                .on_navigation(
                    move |target| match parse_browser_guest_action(target, &nonce) {
                        Some(BrowserGuestAction::Open(target_url)) => {
                            let _ = bridge_app.emit(
                                BROWSER_OPEN_REQUESTED_EVENT,
                                BrowserOpenRequestedPayload {
                                    browser_id: bridge_browser_id.clone(),
                                    target_url,
                                    profile_id: bridge_profile_id.clone(),
                                    worktree_path: bridge_worktree_path.clone(),
                                    opener_browser_id: None,
                                    popup_handle: None,
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
                        Some(BrowserGuestAction::ElementPick) => {
                            let _ = bridge_app.emit(
                                BROWSER_ELEMENT_PICKED_EVENT,
                                BrowserElementPickedPayload {
                                    browser_id: bridge_browser_id.clone(),
                                },
                            );
                            false
                        }
                        Some(BrowserGuestAction::LinkClick(target_url))
                        | Some(BrowserGuestAction::ModifierLinkClick(target_url)) => {
                            let modifier = target.host_str() == Some("modclick.ferryx.invalid");
                            let _ = bridge_app.emit(
                                BROWSER_LINK_CLICKED_EVENT,
                                BrowserLinkClickedPayload {
                                    browser_id: bridge_browser_id.clone(),
                                    target_url,
                                    modifier,
                                    profile_id: bridge_profile_id.clone(),
                                    worktree_path: bridge_worktree_path.clone(),
                                },
                            );
                            false
                        }
                        Some(BrowserGuestAction::PopupOpen { url, handle }) => {
                            let _ = bridge_app.emit(
                                BROWSER_OPEN_REQUESTED_EVENT,
                                BrowserOpenRequestedPayload {
                                    browser_id: bridge_browser_id.clone(),
                                    target_url: url,
                                    profile_id: bridge_profile_id.clone(),
                                    worktree_path: bridge_worktree_path.clone(),
                                    opener_browser_id: Some(bridge_browser_id.clone()),
                                    popup_handle: Some(handle),
                                },
                            );
                            false
                        }
                        Some(BrowserGuestAction::PopupNavigate { url, handle }) => {
                            let app = bridge_app.clone();
                            let manager = Arc::clone(&bridge_manager);
                            let opener_browser_id = bridge_browser_id.clone();
                            tauri::async_runtime::spawn(async move {
                                navigate_linked_popup(&app, &manager, &opener_browser_id, &handle, &url)
                                    .await;
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
                        Some(BrowserGuestAction::PopupCloseRequested) => {
                            let _ = bridge_app.emit(
                                crate::browser::guest::BROWSER_CLOSE_REQUESTED_EVENT,
                                crate::browser::model::BrowserCloseRequestedPayload {
                                    browser_id: bridge_browser_id.clone(),
                                },
                            );
                            false
                        }
                        None => true,
                    },
                )
                .on_page_load(move |webview, payload| {
                    let loading = matches!(payload.event(), PageLoadEvent::Started);
                    let page_url = payload.url().to_string();
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
                        Ok(()) => {
                            let _ = child.eval(&eval_bridge_script);
                            let _ = child.set_zoom(zoom_factor);
                            #[cfg(target_os = "linux")]
                            let initial_bounds = creation_manager
                                .get_bounds(&browser_id)
                                .ok()
                                .flatten()
                                .unwrap_or_else(|| LogicalRect {
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
                        discard_failed_session(manager, &state.browser_id);
                        return Err(BrowserError::CreateFailed(error).into());
                    }
                    Ok(Err(_)) | Err(_) => {
                        discard_failed_session(manager, &state.browser_id);
                        return Err(BrowserError::CreateFailed(
                            "browser webview creation did not complete on the main thread".into(),
                        )
                        .into());
                    }
                }
            }
            Err(error) => {
                discard_failed_session(manager, &state.browser_id);
                return Err(BrowserError::CreateFailed(error.to_string()).into());
            }
        }
    }

    if let Some(link) = opener.as_ref() {
        if !crate::browser::popup::link(&state.browser_id, &link.browser_id, &link.handle) {
            let _ = close_browser_session(app, manager, &state.browser_id).await;
            return Err(BrowserError::CreateFailed("popup closed while creation pending".into()).into());
        }
    }
    Ok(state)
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

pub const fn browser_session_devtools_enabled() -> bool {
    true
}

#[tauri::command]
pub async fn cmd_browser_open_devtools<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
) -> Result<(), IpcError> {
    let state = manager.get_state(&browser_id)?;
    let webview = app.get_webview(&state.webview_label).ok_or_else(|| {
        IpcError::new(
            IpcErrorCode::BrowserNotFound,
            "browser webview is not open",
        )
    })?;
    webview.open_devtools();
    Ok(())
}

#[tauri::command]
pub async fn cmd_browser_inject_element_picker<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
) -> Result<(), IpcError> {
    eval_browser_session(
        &app,
        manager.inner(),
        &browser_id,
        crate::browser::picker::ELEMENT_PICKER_SCRIPT,
    )
    .await?;
    Ok(())
}

#[tauri::command]
pub async fn cmd_browser_remove_element_picker<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
) -> Result<(), IpcError> {
    eval_browser_session(
        &app,
        manager.inner(),
        &browser_id,
        crate::browser::picker::ELEMENT_PICKER_REMOVE_SCRIPT,
    )
    .await?;
    Ok(())
}

#[tauri::command]
pub async fn cmd_browser_finish_element_pick<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
    png_base64: Option<String>,
) -> Result<crate::remote::design_mode::DesignModeSnapshot, IpcError> {
    let state = manager.get_state(&browser_id)?;
    let (raw, _) = eval_browser_session(
        &app,
        manager.inner(),
        &browser_id,
        "window.__ferryxElementPick",
    )
    .await?;
    let json = raw
        .as_deref()
        .map(crate::browser::picker::unwrap_eval_json)
        .filter(|value| !value.trim().is_empty() && value != "null" && value != "undefined")
        .ok_or_else(|| IpcError::new(IpcErrorCode::ParseError, "element pick is empty"))?;
    let _ = eval_browser_session(
        &app,
        manager.inner(),
        &browser_id,
        crate::browser::picker::ELEMENT_PICKER_REMOVE_SCRIPT,
    )
    .await;
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
mod devtools_tests {
    use super::*;

    #[test]
    fn browser_sessions_enable_devtools() {
        assert!(browser_session_devtools_enabled());
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
    let valid_url = manager.update_url_with_context(browser_id, url, context)?;
    let state = manager.get_state(browser_id)?;

    if let Some(webview) = app.get_webview(&state.webview_label) {
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
    if app.get_webview(&state.webview_label).is_none() {
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
pub async fn cmd_browser_import_installed_cookies<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    request: ImportInstalledBrowserCookiesRequest,
) -> Result<ImportInstalledBrowserCookiesResult, IpcError> {
    let profile_id = BrowserProfileId::from_id(&request.profile_id)
        .ok_or_else(|| {
            IpcError::new(
                IpcErrorCode::BrowserCookieImportFailed,
                format!("unsupported profile: {}", request.profile_id),
            )
            .with_details(serde_json::json!({ "reason": "no-profile" }))
        })?;

    let targets = manager
        .webview_labels_for_profile(&profile_id)
        .into_iter()
        .filter_map(|label| app.get_webview(&label))
        .collect::<Vec<_>>();

    if targets.is_empty() {
        return Err(IpcError::new(
            IpcErrorCode::BrowserCookieImportFailed,
            format!(
                "open a browser tab using the {} profile before importing cookies",
                profile_id.as_str()
            ),
        )
        .with_details(serde_json::json!({ "reason": "no-tab" })));
    }

    let home_dir = app.path().home_dir().map_err(|err| {
        IpcError::new(
            IpcErrorCode::BrowserCookieImportFailed,
            format!("failed to resolve home directory: {err}"),
        )
        .with_details(serde_json::json!({ "reason": "no-profile" }))
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
/// from a synchronous command.
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
    let cookies = webview.cookies_for_url(url).map_err(|error| {
        IpcError::new(
            IpcErrorCode::BrowserDownloadFailed,
            format!(
                "failed to read session cookies for {}: {error}",
                request.url
            ),
        )
    })?;
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
) -> Result<(), IpcError> {
    let selector = match &request.action {
        BrowserAutomationAction::Click { reference }
        | BrowserAutomationAction::Fill { reference, .. }
        | BrowserAutomationAction::Dblclick { reference }
        | BrowserAutomationAction::Hover { reference }
        | BrowserAutomationAction::Focus { reference }
        | BrowserAutomationAction::Check { reference }
        | BrowserAutomationAction::Uncheck { reference }
        | BrowserAutomationAction::ScrollIntoView { reference }
        | BrowserAutomationAction::Select { reference, .. } => {
            Some(manager.automation_target(&request.browser_id, request.generation, reference)?)
        }
        BrowserAutomationAction::Scroll {
            reference: Some(reference),
            ..
        }
        | BrowserAutomationAction::Type {
            reference: Some(reference),
            ..
        } => {
            Some(manager.automation_target(&request.browser_id, request.generation, reference)?)
        }
        BrowserAutomationAction::Scroll { reference: None, .. }
        | BrowserAutomationAction::Type { reference: None, .. }
        | BrowserAutomationAction::Keypress { .. } => {
            manager.assert_automation_generation(&request.browser_id, request.generation)?;
            #[cfg(target_os = "windows")]
            if matches!(request.action, BrowserAutomationAction::Keypress { .. }) {
                windows_keypress_capability()?;
            }
            #[cfg(target_os = "linux")]
            if matches!(request.action, BrowserAutomationAction::Keypress { .. }) {
                linux_keypress_capability()?;
            }
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

/// Navigates the popup tab an opener created for one of its handles.
///
/// `window.open('')` followed by `handle.location.href = url` is the common OAuth shape:
/// the first assignment creates the tab, and every later one lands here.
async fn navigate_linked_popup<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &Arc<BrowserManager>,
    opener_browser_id: &str,
    handle: &str,
    url: &str,
) {
    let Some(popup_browser_id) = crate::browser::popup::popup_for_handle(opener_browser_id, handle)
    else {
        tracing::warn!(
            opener = %opener_browser_id,
            handle = %handle,
            "popup navigation ignored: no open popup is linked to this handle"
        );
        return;
    };
    if let Err(error) = navigate_browser_session(app, manager, &popup_browser_id, url).await {
        tracing::warn!(%error, popup = %popup_browser_id, "popup navigation failed");
    }
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
    let script = match (popup_origin.as_deref(), message.target_origin.as_deref()) {
        (Some(popup_origin), Some(target_origin))
            if !crate::browser::popup::target_origin_allows(popup_origin, target_origin) =>
        {
            crate::browser::popup::popup_message_rejected_script(
                target_origin,
                popup_origin,
                &opener_origin,
            )
        }
        _ => crate::browser::popup::popup_message_script(
            &opener_origin,
            &message.data,
        ),
    };
    let target_dest = match (popup_origin.as_deref(), message.target_origin.as_deref()) {
        (Some(popup_origin), Some(target_origin))
            if !crate::browser::popup::target_origin_allows(popup_origin, target_origin) =>
        {
            opener_browser_id
        }
        _ => &popup_browser_id,
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
    let script = match (opener_origin.as_deref(), message.target_origin.as_deref()) {
        (Some(opener_origin), Some(target_origin))
            if !crate::browser::popup::target_origin_allows(opener_origin, target_origin) =>
        {
            crate::browser::popup::opener_message_rejected_script(
                target_origin,
                opener_origin,
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
    CREATED_SESSION_IDS.lock().retain(|id| id != browser_id);
    // A closing popup is how the opener learns the popup is gone. Closing the tab -- by the
    // user, by the page, or by the host -- has to flip the handle's `closed`, or a library
    // polling popup.closed never sees the cancel. The link is removed first so a failed
    // notification cannot leave a stale link behind.
    if let Some(link) = crate::browser::popup::unlink(browser_id) {
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
        let program = "open";
        #[cfg(not(target_os = "macos"))]
        let program = "xdg-open";
        let status = std::process::Command::new(program)
            .arg(target)
            .status()
            .map_err(|error| IpcError::internal(error.to_string()))?;
        if !status.success() {
            return Err(IpcError::internal(format!(
                "system opener exited with {status}"
            )));
        }
        Ok(())
    }
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
    fn external_open_propagates_failure() {
        for failure in 0..=32 {
            assert!(super::shell_execute_result(failure).is_err());
        }
        assert!(super::shell_execute_result(33).is_ok());
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
    fn test_storage_script() {
        let get_s = build_storage_script("local", "get", Some("key1"), None).unwrap();
        assert!(get_s.contains("localStorage.getItem"));

        let set_s = build_storage_script("session", "set", Some("key2"), Some("val2")).unwrap();
        assert!(set_s.contains("sessionStorage.setItem"));

        let clear_k = build_storage_script("local", "clear", Some("key1"), None).unwrap();
        assert!(clear_k.contains("localStorage.removeItem"));

        let clear_all = build_storage_script("local", "clear", None, None).unwrap();
        assert!(clear_all.contains("localStorage.clear"));
    }

    #[test]
    fn test_storage_script_extended_all_and_scope() {
        let clear_all = build_storage_script_extended("local", "clear", None, None, true, None, None).unwrap();
        assert!(clear_all.contains("localStorage.clear"));

        let clear_domain = build_storage_script_extended(
            "local", "clear", None, None, false, None, Some("example.com")
        ).unwrap();
        assert!(clear_domain.contains("location.hostname.includes"));
        assert!(clear_domain.contains("example.com"));

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
            let d_filter = domain.map(|d| serde_json::to_string(d).unwrap_or_else(|_| "\"\"".into()));

            let scope_check = match (u_filter, d_filter) {
                (Some(u), Some(d)) => format!("if (!location.href.includes({u}) || !location.hostname.includes({d})) return null;"),
                (Some(u), None) => format!("if (!location.href.includes({u})) return null;"),
                (None, Some(d)) => format!("if (!location.hostname.includes({d})) return null;"),
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

pub fn build_storage_script(
    kind: &str,
    action: &str,
    key: Option<&str>,
    value: Option<&str>,
) -> Result<String, String> {
    build_storage_script_extended(kind, action, key, value, false, None, None)
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

pub async fn wait_browser_session_with_timeout<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    browser_id: &str,
    condition: crate::browser::model::BrowserWaitCondition,
    timeout: Option<std::time::Duration>,
) -> Result<(), IpcError> {
    let state = manager.get_state(browser_id)?;
    let webview = app
        .get_webview(&state.webview_label)
        .ok_or_else(|| BrowserError::WebviewNotFound(state.webview_label.clone()))?;
    let script = build_wait_condition_script(&condition);
    let timeout = timeout.unwrap_or_else(|| std::time::Duration::from_secs(15));
    let deadline = tokio::time::Instant::now() + timeout;
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
        if let Ok(Ok(res)) =
            tokio::time::timeout(eval_timeout, eval_webview(webview.clone(), script.clone())).await
        {
            if parse_eval_boolean(&res) {
                return Ok(());
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
        .map_err(|e| BrowserError::AutomationFailed(e))?;
    Ok(entries)
}

pub async fn screenshot_browser_session<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    browser_id: &str,
    out_path: &str,
) -> Result<String, IpcError> {
    let state = manager.get_state(browser_id)?;
    crate::browser::screenshot::take_browser_screenshot(app, &state.webview_label, out_path).await
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

pub async fn cookies_browser_session<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    browser_id: &str,
    action: &str,
    name: Option<&str>,
    value: Option<&str>,
    domain: Option<&str>,
    path: Option<&str>,
) -> Result<Vec<crate::browser::model::BrowserCookieEntry>, IpcError> {
    let state = manager.get_state(browser_id)?;
    let webview = app
        .get_webview(&state.webview_label)
        .ok_or_else(|| BrowserError::WebviewNotFound(state.webview_label.clone()))?;
    let script = build_cookie_script(action, name, value, domain, path)
        .map_err(|e| IpcError::new(IpcErrorCode::InvalidArgument, e))?;
    let raw = eval_webview(webview, script).await?;
    let mut entries = parse_document_cookie(&raw);
    if action == "get" || action == "list" {
        if let Some(target_name) = name {
            entries.retain(|c| c.name == target_name);
        }
    }
    Ok(entries)
}

pub async fn storage_browser_session_extended<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    browser_id: &str,
    kind: &str,
    action: &str,
    key: Option<&str>,
    value: Option<&str>,
    clear_all: bool,
    url: Option<&str>,
    domain: Option<&str>,
) -> Result<Option<String>, IpcError> {
    let state = manager.get_state(browser_id)?;
    let webview = app
        .get_webview(&state.webview_label)
        .ok_or_else(|| BrowserError::WebviewNotFound(state.webview_label.clone()))?;
    let script = build_storage_script_extended(kind, action, key, value, clear_all, url, domain)
        .map_err(|e| IpcError::new(IpcErrorCode::InvalidArgument, e))?;
    let raw = eval_webview(webview, script).await?;
    Ok(parse_storage_result(&raw))
}

pub async fn storage_browser_session<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    browser_id: &str,
    kind: &str,
    action: &str,
    key: Option<&str>,
    value: Option<&str>,
) -> Result<Option<String>, IpcError> {
    storage_browser_session_extended(
        app, manager, browser_id, kind, action, key, value, false, None, None,
    )
    .await
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

    // Read full cookie jar with attributes including HttpOnly via Webview::cookies()
    let raw_cookies = webview.cookies().map_err(|e| {
        IpcError::new(
            IpcErrorCode::InternalError,
            format!("failed to read cookies from webview: {e}"),
        )
    })?;
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
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| {
                IpcError::new(
                    IpcErrorCode::InternalError,
                    format!("failed to create directory for state file: {e}"),
                )
            })?;
        }
    }
    std::fs::write(path, json_bytes).map_err(|e| {
        IpcError::new(
            IpcErrorCode::InternalError,
            format!("failed to write state file '{}': {e}", path.display()),
        )
    })?;
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

            // 2. Authoritative daemon-side reclaim: revoke control permissions & clear active selection
            let devices = self
                .daemon_client
                .remote_list_devices()
                .await
                .unwrap_or_default();
            for dev in devices {
                if dev.permission == crate::remote::DevicePermission::Control {
                    let _ = self.daemon_client.remote_revoke_device(&dev.id).await;
                }
            }
            let _ = self.daemon_client.remote_set_active_selection(None).await;

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
                    IpcErrorCode::Custom("BROWSER_UNAVAILABLE".into()),
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
                if let Ok(expected) = exp_gen.parse::<u64>() {
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
                    .map_err(|e| match e.code {
                        IpcErrorCode::BrowserUrlSchemeDenied | IpcErrorCode::BrowserUrlInvalid => {
                            RemoteBrowserError::Forbidden(e.message)
                        }
                        _ => RemoteBrowserError::ExecutionFailed(e.to_string()),
                    })?;
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
                        .map_err(|e| RemoteBrowserError::ExecutionFailed(e.to_string()))?;
                    Ok(BrowserCommandResult {
                        success: true,
                        value: None,
                    })
                }
                "forward" => {
                    history_navigation(&self.app, &self.manager, &ctx.browser_id, true)
                        .map_err(|e| RemoteBrowserError::ExecutionFailed(e.to_string()))?;
                    Ok(BrowserCommandResult {
                        success: true,
                        value: None,
                    })
                }
                "reload" => {
                    let state = self
                        .manager
                        .begin_reload(&ctx.browser_id)
                        .map_err(|e| RemoteBrowserError::ExecutionFailed(e.to_string()))?;
                    let webview = self
                        .app
                        .get_webview(&state.webview_label)
                        .ok_or_else(|| RemoteBrowserError::NotFound(state.webview_label.clone()))?;
                    webview
                        .reload()
                        .map_err(|e| RemoteBrowserError::ExecutionFailed(e.to_string()))?;
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
                    if let Some(vp_str) = p
                        .get("viewportRevision")
                        .and_then(|v| v.as_str())
                        .or_else(|| p.get("viewport_revision").and_then(|v| v.as_str()))
                    {
                        if let Ok(expected_vp) = vp_str.parse::<u64>() {
                            if expected_vp != current_vp_rev as u64 {
                                return Err(RemoteBrowserError::StaleFrame("viewport revision changed since frame capture".into()));
                            }
                        }
                    } else if let Some(expected_vp) = p
                        .get("viewportRevision")
                        .or_else(|| p.get("viewport_revision"))
                        .and_then(|v| v.as_u64())
                    {
                        if expected_vp != current_vp_rev as u64 {
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
                        format!(
                            r#"(function() {{
                                const el = document.querySelector({});
                                if (!el) return JSON.stringify({{ ok: false, error: "element not found" }});
                                el.click();
                                return JSON.stringify({{ ok: true }});
                            }})()"#,
                            sel_json
                        )
                    } else if let Some(selector) = p.get("selector").and_then(|v| v.as_str()) {
                        let sel_json = serde_json::to_string(selector).unwrap_or_default();
                        format!(
                            r#"(function() {{
                                const el = document.querySelector({});
                                if (!el) return JSON.stringify({{ ok: false, error: "element not found" }});
                                el.click();
                                return JSON.stringify({{ ok: true }});
                            }})()"#,
                            sel_json
                        )
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
                        return Err(RemoteBrowserError::InvalidRequest(
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
                    crate::browser::remote_input::decode_point_click_result(&res_str).map_err(|e| {
                        match e {
                            crate::browser::remote_input::ActionDecodeError::Refused(input_err) => {
                                RemoteBrowserError::InputRefused(input_err.explanation())
                            }
                            crate::browser::remote_input::ActionDecodeError::NotFound(message) => {
                                RemoteBrowserError::TargetNotFound(message)
                            }
                            crate::browser::remote_input::ActionDecodeError::Malformed(message) => {
                                RemoteBrowserError::ExecutionFailed(message)
                            }
                        }
                    })?;
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
                    crate::browser::remote_input::decode_action_result(&res_str).map_err(|e| {
                        if e.contains("element not found") || e.contains("no active element") {
                            RemoteBrowserError::TargetNotFound(e)
                        } else {
                            RemoteBrowserError::ExecutionFailed(e)
                        }
                    })?;
                    Ok(BrowserCommandResult {
                        success: true,
                        value: Some(serde_json::json!({ "filled": true })),
                    })
                }
                "keypress" => {
                    let state = self
                        .manager
                        .get_state(&ctx.browser_id)
                        .map_err(|_| RemoteBrowserError::NotFound(ctx.browser_id.clone()))?;
                    let webview = self
                        .app
                        .get_webview(&state.webview_label)
                        .ok_or_else(|| RemoteBrowserError::NotFound(state.webview_label.clone()))?;
                    let p = ctx.params.as_ref().ok_or_else(|| {
                        RemoteBrowserError::InvalidRequest("keypress requires params".into())
                    })?;
                    let key = p.get("key").and_then(|v| v.as_str()).unwrap_or("");
                    let key_json = serde_json::to_string(key).unwrap_or_default();
                    let script = format!(
                        r#"(function() {{
                            const target = document.activeElement || document.body;
                            target.dispatchEvent(new KeyboardEvent("keydown", {{ key: {}, bubbles: true }}));
                            target.dispatchEvent(new KeyboardEvent("keyup", {{ key: {}, bubbles: true }}));
                            return JSON.stringify({{ ok: true }});
                        }})()"#,
                        key_json, key_json
                    );
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
                            .map_err(|e| RemoteBrowserError::ExecutionFailed(e.to_string()))?;
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
                    .map_err(|e| match e.code {
                        IpcErrorCode::BrowserAutomationTargetNotFound => {
                            RemoteBrowserError::TargetNotFound(e.message)
                        }
                        IpcErrorCode::InvalidArgument => {
                            RemoteBrowserError::InvalidRequest(e.message)
                        }
                        _ => RemoteBrowserError::ExecutionFailed(e.to_string()),
                    })?;
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
                    .map_err(|e| match e.code {
                        IpcErrorCode::BrowserWaitTimeout => RemoteBrowserError::WaitTimeout,
                        _ => RemoteBrowserError::ExecutionFailed(e.to_string()),
                    })?;
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
                if (!el || el === document.body) return JSON.stringify({{ ok: false, error: "no active element to fill" }});
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
                if (!el) return JSON.stringify({{ ok: false, error: "element not found" }});
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
                if (!el) return JSON.stringify({{ ok: false, error: "element not found" }});
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
    } else {
        // Fallback to active element
        Ok(format!(
            r#"(function() {{
                const el = document.activeElement;
                if (!el || el === document.body) return JSON.stringify({{ ok: false, error: "no active element to fill" }});
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
        // Local callers keep the local policy.
        navigate_browser_session(app.handle(), &manager, &state.browser_id, "file:///tmp/x.html")
            .await
            .expect("local navigate keeps the local file policy");
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
        assert!(
            res.is_err(),
            "Must reject missing DaemonReclaimTransport in daemon mode"
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
}
