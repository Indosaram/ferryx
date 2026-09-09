use tauri::Url;

pub const BROWSER_OPEN_REQUESTED_EVENT: &str = "browser_open_requested";
pub const BROWSER_DOWNLOAD_REQUESTED_EVENT: &str = "browser_download_requested";
pub const BROWSER_SHORTCUT_REQUESTED_EVENT: &str = "browser_shortcut_requested";

const OPEN_HOST: &str = "open.ferryx.invalid";
const DOWNLOAD_HOST: &str = "download.ferryx.invalid";
const SHORTCUT_HOST: &str = "shortcut.ferryx.invalid";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserGuestAction {
    Open(String),
    Download(String),
    Shortcut(String),
}

pub fn parse_browser_guest_action(url: &Url, expected_nonce: &str) -> Option<BrowserGuestAction> {
    let host = url.host_str()?;
    if query_value(url, "nonce").as_deref() != Some(expected_nonce) {
        return None;
    }
    match host {
        OPEN_HOST => query_value(url, "url").map(BrowserGuestAction::Open),
        DOWNLOAD_HOST => query_value(url, "url").map(BrowserGuestAction::Download),
        SHORTCUT_HOST => query_value(url, "action").map(BrowserGuestAction::Shortcut),
        _ => None,
    }
}

fn query_value(url: &Url, key: &str) -> Option<String> {
    url.query_pairs()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.into_owned())
        .filter(|value| !value.trim().is_empty())
}

const BRIDGE_NONCE_PLACEHOLDER: &str = "__FERRYX_BROWSER_BRIDGE_NONCE__";

/// Builds the guest bridge initialization script for one browser webview.
/// `nonce` is JSON-encoded into the script and required as an extra query
/// parameter on every routed control URL, so a page that never received the
/// bridge cannot forge open/download/shortcut requests (see
/// `parse_browser_guest_action`).
pub fn browser_guest_bridge_script(nonce: &str) -> String {
    let nonce_json = serde_json::to_string(nonce).unwrap_or_else(|_| "\"\"".to_string());
    BRIDGE_SCRIPT_TEMPLATE.replace(BRIDGE_NONCE_PLACEHOLDER, &nonce_json)
}

const BRIDGE_SCRIPT_TEMPLATE: &str = r#"
(() => {
  if (window.__ferryxBrowserBridgeInstalled) return;
  Object.defineProperty(window, '__ferryxBrowserBridgeInstalled', { value: true });

  // This script runs at document start, before any page script, so these
  // references are still pristine. `route` below builds a URL containing the
  // bridge nonce and hands it to `assign`; resolving either function at call
  // time instead would let a page install its own `location.assign` or
  // `encodeURIComponent` and capture the nonce, which is exactly the credential
  // that stops it from forging control messages.
  const assign = location.assign.bind(location);
  const enc = encodeURIComponent;
  const closestOf = Element.prototype.closest;
  const addDocumentListener = document.addEventListener.bind(document);

  const resolveHttpUrl = (raw) => {
    if (!raw) return null;
    try {
      const parsed = new URL(String(raw), location.href);
      return parsed.protocol === 'http:' || parsed.protocol === 'https:' ? parsed.href : null;
    } catch (_) {
      return null;
    }
  };

  const route = (host, key, value) => {
    const target = `https://${host}/?${key}=${enc(value)}&nonce=${__FERRYX_BROWSER_BRIDGE_NONCE__}`;
    assign(target);
  };

  const originalOpen = window.open.bind(window);
  window.open = (url, ...rest) => {
    const target = resolveHttpUrl(url);
    if (target) {
      route('open.ferryx.invalid', 'url', target);
      return null;
    }
    return originalOpen(url, ...rest);
  };

  addDocumentListener('click', (event) => {
    const target = event.target;
    if (!(target instanceof Element)) return;
    const anchor = closestOf.call(target, 'a[href]');
    if (!(anchor instanceof HTMLAnchorElement)) return;
    const url = resolveHttpUrl(anchor.href);
    if (!url) return;
    if (anchor.hasAttribute('download')) {
      event.preventDefault();
      route('download.ferryx.invalid', 'url', url);
      return;
    }
    if ((anchor.target || '').toLowerCase() === '_blank') {
      event.preventDefault();
      route('open.ferryx.invalid', 'url', url);
    }
  }, true);

  const isMac = typeof navigator !== 'undefined' && (
    /Mac|iPhone|iPod|iPad/.test(navigator.platform || '') ||
    /Macintosh/.test(navigator.userAgent || '')
  );

  addDocumentListener('keydown', (event) => {
    let action = null;
    const composing = event.isComposing || event.keyCode === 229;
    if (composing) return;

    const code = event.code || '';
    const key = event.key ? event.key.toLowerCase() : '';
    const primaryMod = isMac ? (event.metaKey && !event.ctrlKey) : (event.ctrlKey && !event.metaKey);

    // Global app commands (new tab, close tab, command palette, sidebar toggle,
    // settings, pane split, tab select, tab cycle, workspace select) work
    // everywhere in the child webview, even while typing in text inputs or
    // textareas, matching native desktop browser expectations.
    if (primaryMod && !event.altKey) {
      if (!event.shiftKey) {
        if (key === 't' || code === 'KeyT') action = 'tab-new-terminal';
        else if (key === 'w' || code === 'KeyW') action = 'tab-close';
        else if (key === 'k' || code === 'KeyK') action = 'command-palette';
        else if (key === 'b' || code === 'KeyB') action = 'sidebar-toggle';
        else if (key === ',' || code === 'Comma') action = 'settings-toggle';
        else if (key === 'd' || code === 'KeyD') action = 'split-right';
      } else {
        if (key === 'd' || code === 'KeyD') action = 'split-down';
      }
    }

    // Workspace selection: Cmd+1..9 on macOS, Alt+1..9 on Windows/Linux
    if (!action && !event.shiftKey) {
      const isWsMod = isMac ? (event.metaKey && !event.ctrlKey && !event.altKey) : (event.altKey && !event.ctrlKey && !event.metaKey);
      if (isWsMod) {
        const digit = /^[1-9]$/.test(event.key) ? event.key : (code.match(/^Digit([1-9])$/) || [])[1];
        if (digit) action = 'workspace-select-' + digit;
      }
    }

    // Tab selection (Ctrl+1..9 on all platforms)
    if (!action && event.ctrlKey && !event.metaKey && !event.altKey && !event.shiftKey) {
      const digit = /^[1-9]$/.test(event.key) ? event.key : (code.match(/^Digit([1-9])$/) || [])[1];
      if (digit) action = 'tab-select-' + digit;
    }

    // Tab cycling (Ctrl+Tab, Ctrl+Shift+Tab, Ctrl+PageUp/Down, Cmd+Shift+[/])
    if (!action) {
      if (event.ctrlKey && !event.metaKey && !event.altKey && (key === 'tab' || code === 'Tab')) {
        action = event.shiftKey ? 'tab-previous' : 'tab-next';
      } else if (event.ctrlKey && !event.metaKey && !event.altKey && (key === 'pagedown' || code === 'PageDown')) {
        action = 'tab-next';
      } else if (event.ctrlKey && !event.metaKey && !event.altKey && (key === 'pageup' || code === 'PageUp')) {
        action = 'tab-previous';
      } else if (primaryMod && event.shiftKey && !event.altKey && (code === 'BracketRight' || key === ']' || key === '}')) {
        action = 'tab-next';
      } else if (primaryMod && event.shiftKey && !event.altKey && (code === 'BracketLeft' || key === '[' || key === '{')) {
        action = 'tab-previous';
      }
    }

    // Browser in-page navigation (only outside editable fields to not interfere with typing)
    const keyTarget = event.target;
    const inEditable = keyTarget instanceof Element && (
      keyTarget.tagName === 'INPUT' || keyTarget.tagName === 'TEXTAREA' ||
      keyTarget.tagName === 'SELECT' || keyTarget.isContentEditable
    );

    if (!action && !inEditable) {
      if (primaryMod && (key === 'l' || code === 'KeyL')) action = 'focus-address';
      else if (primaryMod && (key === 'r' || code === 'KeyR')) action = 'reload';
      else if (primaryMod && key === '[') action = 'back';
      else if (primaryMod && key === ']') action = 'forward';
      else if (primaryMod && (key === 'f' || code === 'KeyF')) action = 'find';
      else if (event.altKey && !event.metaKey && !event.ctrlKey && event.key === 'ArrowLeft') action = 'back';
      else if (event.altKey && !event.metaKey && !event.ctrlKey && event.key === 'ArrowRight') action = 'forward';
    }

    if (!action) return;
    event.preventDefault();
    route('shortcut.ferryx.invalid', 'action', action);
  }, true);

  addDocumentListener('drop', (event) => {
    const uri = event.dataTransfer?.getData('text/uri-list') || event.dataTransfer?.getData('text/plain') || '';
    const target = resolveHttpUrl(uri.split(/\r?\n/).find((line) => line && !line.startsWith('#')) || uri);
    if (!target) return;
    event.preventDefault();
    assign(target);
  }, true);
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_NONCE: &str = "550e8400-e29b-41d4-a716-446655440000";

    #[test]
    fn bridge_script_captures_nonce_carrying_builtins_at_document_start() {
        let script = browser_guest_bridge_script(TEST_NONCE);

        assert!(script.contains("const assign = location.assign.bind(location);"));
        assert!(script.contains("const enc = encodeURIComponent;"));

        // The routed URL carries the nonce, so it must be built and dispatched
        // through the captured references: resolving them at call time lets a
        // page swap either one in and read the nonce out of the target URL.
        assert!(script.contains("&nonce="));
        assert!(
            !script.contains("encodeURIComponent(value)"),
            "route must encode through the captured reference"
        );
        assert!(
            !script.contains("location.assign(target)"),
            "route must navigate through the captured reference"
        );
    }

    #[test]
    fn parses_bridge_actions_and_ignores_normal_navigation() {
        let open = Url::parse(&format!(
            "https://open.ferryx.invalid/?url=https%3A%2F%2Fexample.com%2Foauth&nonce={TEST_NONCE}"
        ))
        .unwrap();
        assert_eq!(
            parse_browser_guest_action(&open, TEST_NONCE),
            Some(BrowserGuestAction::Open("https://example.com/oauth".into()))
        );

        let download = Url::parse(&format!(
            "https://download.ferryx.invalid/?url=https%3A%2F%2Fexample.com%2Ffile.pdf&nonce={TEST_NONCE}"
        ))
        .unwrap();
        assert_eq!(
            parse_browser_guest_action(&download, TEST_NONCE),
            Some(BrowserGuestAction::Download(
                "https://example.com/file.pdf".into()
            ))
        );

        let shortcut = Url::parse(&format!(
            "https://shortcut.ferryx.invalid/?action=find&nonce={TEST_NONCE}"
        ))
        .unwrap();
        assert_eq!(
            parse_browser_guest_action(&shortcut, TEST_NONCE),
            Some(BrowserGuestAction::Shortcut("find".into()))
        );

        // App-level and tab navigation chords ride the same shortcut host so the main
        // webview (which never sees the keydown) can still switch tabs and run app commands.
        for action in [
            "tab-next",
            "tab-previous",
            "tab-select-1",
            "tab-select-9",
            "tab-new-terminal",
            "tab-close",
            "command-palette",
            "sidebar-toggle",
            "settings-toggle",
            "split-right",
            "split-down",
            "workspace-select-1",
            "workspace-select-9",
        ] {
            let forwarded = Url::parse(&format!(
                "https://shortcut.ferryx.invalid/?action={action}&nonce={TEST_NONCE}"
            ))
            .unwrap();
            assert_eq!(
                parse_browser_guest_action(&forwarded, TEST_NONCE),
                Some(BrowserGuestAction::Shortcut(action.into()))
            );
        }

        assert_eq!(
            parse_browser_guest_action(&Url::parse("https://example.com/").unwrap(), TEST_NONCE),
            None
        );
    }

    #[test]
    fn guest_control_urls_require_matching_nonce() {
        // Correct nonce still parses.
        let open = Url::parse(&format!(
            "https://open.ferryx.invalid/?nonce={TEST_NONCE}&url=https%3A%2F%2Fexample.com"
        ))
        .unwrap();
        assert_eq!(
            parse_browser_guest_action(&open, TEST_NONCE),
            Some(BrowserGuestAction::Open("https://example.com".into()))
        );

        // Wrong nonce is rejected on every control host.
        let forged_open = Url::parse(
            "https://open.ferryx.invalid/?url=https%3A%2F%2Fexample.com&nonce=attacker-guess",
        )
        .unwrap();
        assert_eq!(parse_browser_guest_action(&forged_open, TEST_NONCE), None);
        let forged_download = Url::parse(
            "https://download.ferryx.invalid/?url=https%3A%2F%2Fexample.com%2Ff.pdf&nonce=attacker-guess",
        )
        .unwrap();
        assert_eq!(
            parse_browser_guest_action(&forged_download, TEST_NONCE),
            None
        );
        let forged_shortcut =
            Url::parse("https://shortcut.ferryx.invalid/?action=find&nonce=attacker-guess")
                .unwrap();
        assert_eq!(
            parse_browser_guest_action(&forged_shortcut, TEST_NONCE),
            None
        );

        // Missing nonce (the legacy shape any page can produce) is rejected.
        let legacy_open =
            Url::parse("https://open.ferryx.invalid/?url=https%3A%2F%2Fexample.com").unwrap();
        assert_eq!(parse_browser_guest_action(&legacy_open, TEST_NONCE), None);
        let legacy_download =
            Url::parse("https://download.ferryx.invalid/?url=https%3A%2F%2Fexample.com%2Ff.pdf")
                .unwrap();
        assert_eq!(
            parse_browser_guest_action(&legacy_download, TEST_NONCE),
            None
        );
        let legacy_shortcut = Url::parse("https://shortcut.ferryx.invalid/?action=find").unwrap();
        assert_eq!(
            parse_browser_guest_action(&legacy_shortcut, TEST_NONCE),
            None
        );
    }

    #[test]
    fn guest_bridge_covers_popup_download_shortcuts_and_url_drop() {
        let script = browser_guest_bridge_script(TEST_NONCE);
        assert!(script.contains("window.open"));
        assert!(script.contains("download.ferryx.invalid"));
        assert!(script.contains("shortcut.ferryx.invalid"));
        assert!(script.contains("text/uri-list"));
        // The nonce is embedded JSON-encoded and appended to every routed URL.
        let nonce_json = serde_json::to_string(TEST_NONCE).unwrap();
        assert!(script.contains(&format!("&nonce=${{{nonce_json}}}")));
        assert!(!script.contains(BRIDGE_NONCE_PLACEHOLDER));
    }
}
