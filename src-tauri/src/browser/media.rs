//! Now Playing: the media state a tab's page publishes (media-session metadata plus its dominant
//! `<audio>`/`<video>`), and the commands that drive it.
//!
//! Both engines feed the same type. The system webview pushes reports from the page-world
//! reporter inside its guest bridge: page -> `ferryx-intent` -> isolated-world broker -> the
//! `media` trusted-bridge envelope. The Chromium (CEF) engine has no page-world initialization
//! hook on Ferryx's side of the engine protocol (and the engine binary must not change), so the
//! same reporter source is evaluated there with `post = null` through the existing
//! `Runtime.evaluate` page-script path by [`install_engine_media_poller`].
//!
//! Every field comes from the page and is therefore untrusted: lengths are clamped, control
//! characters are stripped, and only http(s) artwork URLs are accepted. Nothing here evaluates
//! a page-provided string.

use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// Tauri event carrying a [`BrowserMediaState`].
pub const BROWSER_MEDIA_STATE_EVENT: &str = "browser_media_state";

/// Envelope kind the reporter's push path posts.
const MEDIA_ENVELOPE_KIND: &str = "media";
const MAX_ENVELOPE_BYTES: usize = 16 * 1024;
/// Rate limit per tab: the reporter is throttled in the page too, but a page can forge the
/// intent, so the host enforces the same ceiling on its own.
pub const MAX_MEDIA_EVENTS_PER_SECOND: usize = 2;

pub const MAX_TITLE_CHARS: usize = 300;
pub const MAX_ARTIST_CHARS: usize = 200;
pub const MAX_ALBUM_CHARS: usize = 200;
pub const MAX_ARTWORK_CHARS: usize = 2048;

/// What a tab currently plays. Serialized camelCase for the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserMediaState {
    pub browser_id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub artwork_url: String,
    pub playing: bool,
    pub muted: bool,
    pub has_media: bool,
}

impl BrowserMediaState {
    /// A tab with nothing to show. Emitted once when a page that had media loses it.
    pub fn empty(browser_id: &str) -> Self {
        Self {
            browser_id: browser_id.to_string(),
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            artwork_url: String::new(),
            playing: false,
            muted: false,
            has_media: false,
        }
    }
}

/// The report as the page sends it. Every field is optional on the wire so a partial report is
/// still usable, and unknown fields are refused.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MediaReport {
    #[serde(default)]
    title: String,
    #[serde(default)]
    artist: String,
    #[serde(default)]
    album: String,
    #[serde(default)]
    artwork_url: String,
    #[serde(default)]
    playing: bool,
    #[serde(default)]
    muted: bool,
    #[serde(default)]
    has_media: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MediaEnvelope {
    v: u32,
    kind: String,
    report: MediaReport,
}

/// Strips control characters, collapses runs of whitespace, trims and clamps to `limit` chars.
pub fn clamp_text(raw: &str, limit: usize) -> String {
    let cleaned: String = raw
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(limit).collect()
}

/// Only http(s) artwork survives: a `data:` URL from a page is unbounded in a way a URL is not,
/// and the UI only ever renders a remote image.
pub fn clamp_artwork_url(raw: &str) -> String {
    let text = clamp_text(raw, MAX_ARTWORK_CHARS);
    let lower = text.to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        return String::new();
    }
    text
}

impl MediaReport {
    /// Applies the host's clamps and binds the report to its tab.
    fn sanitize(self, browser_id: &str) -> BrowserMediaState {
        let has_media = self.has_media;
        BrowserMediaState {
            browser_id: browser_id.to_string(),
            title: clamp_text(&self.title, MAX_TITLE_CHARS),
            artist: clamp_text(&self.artist, MAX_ARTIST_CHARS),
            album: clamp_text(&self.album, MAX_ALBUM_CHARS),
            artwork_url: clamp_artwork_url(&self.artwork_url),
            // Nothing is "playing" without media, whatever the report claims.
            playing: has_media && self.playing,
            muted: has_media && self.muted,
            has_media,
        }
    }
}

/// Whether `raw` is a media envelope at all (cheap pre-check before the full parse).
pub fn is_media_envelope(raw: &str) -> bool {
    raw.len() <= MAX_ENVELOPE_BYTES && raw.contains("\"media\"")
}

/// Parses a media envelope and clamps it. `None` for anything malformed.
pub fn parse_media_envelope(raw: &str, browser_id: &str) -> Option<BrowserMediaState> {
    if raw.len() > MAX_ENVELOPE_BYTES {
        return None;
    }
    let envelope: MediaEnvelope = serde_json::from_str(raw).ok()?;
    if envelope.v != 1 || envelope.kind != MEDIA_ENVELOPE_KIND {
        return None;
    }
    Some(envelope.report.sanitize(browser_id))
}

/// The key two reports are compared by: the same key means nothing changed.
pub fn media_state_key(state: &BrowserMediaState) -> String {
    format!(
        "{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}",
        state.title, state.artist, state.album, state.artwork_url, state.playing, state.muted
    )
}

/// Per-tab rate limit plus change detection: at most [`MAX_MEDIA_EVENTS_PER_SECOND`] events in
/// any one-second window, and never twice for the same state.
#[derive(Debug, Default)]
pub struct MediaEventThrottle {
    window_start: Option<Instant>,
    window_count: usize,
    last_key: Option<String>,
}

impl MediaEventThrottle {
    /// Whether this state should become an event at `now`. A suppressed state is not remembered
    /// as sent, so the next tick inside the limit still delivers it.
    pub fn decide(&mut self, state: &BrowserMediaState, now: Instant) -> bool {
        let key = media_state_key(state);
        if self.last_key.as_deref() == Some(key.as_str()) {
            return false;
        }
        let window_expired = self
            .window_start
            .is_none_or(|start| now.saturating_duration_since(start) >= Duration::from_secs(1));
        if window_expired {
            self.window_start = Some(now);
            self.window_count = 0;
        }
        if self.window_count >= MAX_MEDIA_EVENTS_PER_SECOND {
            return false;
        }
        self.window_count += 1;
        self.last_key = Some(key);
        true
    }

    /// The tab went away: the next report from it is a change again.
    pub fn forget(&mut self) {
        self.window_start = None;
        self.window_count = 0;
        self.last_key = None;
    }
}

static THROTTLES: LazyLock<Mutex<HashMap<String, MediaEventThrottle>>> = LazyLock::new(Default::default);

/// Emits `browser_media_state` for `state` when it is a change and within the rate limit.
/// Returns whether it was emitted.
pub fn publish<R: tauri::Runtime>(app: &tauri::AppHandle<R>, state: BrowserMediaState) -> bool {
    let now = Instant::now();
    let emit = {
        let mut throttles = THROTTLES.lock();
        throttles
            .entry(state.browser_id.clone())
            .or_default()
            .decide(&state, now)
    };
    if !emit {
        return false;
    }
    use tauri::Emitter;
    let _ = app.emit(BROWSER_MEDIA_STATE_EVENT, &state);
    true
}

/// Drops a closed tab's throttle state.
pub fn forget_browser(browser_id: &str) {
    THROTTLES.lock().remove(browser_id);
}

/// Handles the `media` envelope the trusted bridge delivered for `browser_id`.
pub fn handle_media_envelope<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    browser_id: &str,
    raw: &str,
) {
    let Some(state) = parse_media_envelope(raw, browser_id) else {
        tracing::debug!(browser_id = %browser_id, "dropped a malformed media report");
        return;
    };
    publish(app, state);
}

/// One media control the Now Playing card can send. A closed set: the page receives only one of
/// these tokens, never a caller-provided string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaControlAction {
    Play,
    Pause,
    Toggle,
    Mute,
    Unmute,
}

impl MediaControlAction {
    pub const ALL: [Self; 5] = [Self::Play, Self::Pause, Self::Toggle, Self::Mute, Self::Unmute];

    pub fn parse(raw: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|action| action.as_str() == raw.trim())
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Play => "play",
            Self::Pause => "pause",
            Self::Toggle => "toggle",
            Self::Mute => "mute",
            Self::Unmute => "unmute",
        }
    }
}

/// Outcome of one media control, as the page reports it back. Fields the page omits are
/// defaulted, so an engine that answers with a narrower object still parses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserMediaControlResult {
    pub action: String,
    pub applied: bool,
    #[serde(default)]
    pub playing: bool,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub has_media: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// The page script that drives the dominant media element. The action is a literal from a closed
/// set, so no page- or caller-provided string is ever spliced into the script body.
pub fn browser_media_control_script(action: MediaControlAction) -> String {
    // Only this action's branch is emitted: the script never carries the other controls.
    let branch = match action {
        MediaControlAction::Play => "{\n        const started = target.play();\n        if (started && typeof started.catch === 'function') started.catch(() => {});\n        break;\n      }",
        MediaControlAction::Pause => "target.pause(); break;",
        MediaControlAction::Toggle => {
            "{\n        if (playing(target)) target.pause();\n        else {\n          const started = target.play();\n          if (started && typeof started.catch === 'function') started.catch(() => {});\n        }\n        break;\n      }"
        }
        MediaControlAction::Mute => "target.muted = true; break;",
        MediaControlAction::Unmute => "target.muted = false; break;",
    };
    format!(
        r#"(() => {{
  const action = '{action}';
  // The dominant element is chosen exactly like the reporter chooses it: the largest playing
  // one, else the largest one in document order.
  const elements = Array.from(document.querySelectorAll('audio,video'))
    .filter((node) => node.tagName === 'AUDIO' || node.tagName === 'VIDEO');
  if (elements.length === 0) {{
    return JSON.stringify({{ action, applied: false, playing: false, muted: false, hasMedia: false, reason: 'no-media' }});
  }}
  const area = (element) => {{
    const rect = typeof element.getBoundingClientRect === 'function' ? element.getBoundingClientRect() : null;
    if (!rect) return 0;
    return Math.max(0, rect.width) * Math.max(0, rect.height);
  }};
  const playing = (element) => !element.paused && !element.ended && element.readyState > 0;
  const active = elements.filter(playing);
  const target = active.length > 0
    ? active.reduce((best, element) => (area(element) > area(best) ? element : best))
    : elements.reduce((best, element) => (area(element) > area(best) ? element : best));
  try {{
    {branch}
  }} catch (error) {{
    return JSON.stringify({{ action, applied: false, playing: playing(target), muted: target.muted === true, hasMedia: true, reason: String(error) }});
  }}
  return JSON.stringify({{ action, applied: true, playing: playing(target), muted: target.muted === true, hasMedia: true }});
}})()"#,
        action = action.as_str(),
        branch = branch
    )
}

/// The pull form of the shared reporter: one JSON snapshot, used for Chromium engine tabs.
pub fn browser_media_snapshot_script() -> String {
    ferryx_browser_engine_protocol::media_snapshot_script()
}

/// How often a Chromium engine tab is polled for its media state.
#[cfg(all(feature = "desktop", any(target_os = "windows", target_os = "macos", target_os = "linux")))]
const ENGINE_POLL_INTERVAL: Duration = Duration::from_millis(1000);

/// Keeps `browser_media_state` flowing for Chromium engine tabs, which have no page-world push
/// channel: every second each open engine tab is asked for one snapshot through the existing
/// page-script evaluation path, and the answers go through the same throttle as the webview
/// push. Engine tabs only: a system webview tab pushes its own reports. On a build with no
/// engine (a test or headless target) this does nothing.
pub fn install_engine_media_poller<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    manager: std::sync::Arc<crate::browser::manager::BrowserManager>,
) {
    #[cfg(all(feature = "desktop", any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(ENGINE_POLL_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            let tabs = crate::browser::cef_engine::tab_ids();
            if tabs.is_empty() {
                continue;
            }
            for browser_id in tabs {
                let snapshot = match engine_media_snapshot(&browser_id).await {
                    Ok(snapshot) => snapshot,
                    Err(error) => {
                        tracing::trace!(browser_id = %browser_id, %error, "media snapshot unavailable");
                        continue;
                    }
                };
                publish(&app, snapshot);
            }
        }
    });
    #[cfg(not(all(feature = "desktop", any(target_os = "windows", target_os = "macos", target_os = "linux"))))]
    let _ = (app, manager);
}

/// One Chromium engine tab's media snapshot. Deliberately NOT the agent's `Runtime.evaluate`
/// params: those claim a user gesture, and a once-a-second poll must never hand the page a
/// fresh transient activation it could spend on autoplay or a popup.
#[cfg(all(feature = "desktop", any(target_os = "windows", target_os = "macos", target_os = "linux")))]
async fn engine_media_snapshot(browser_id: &str) -> Result<BrowserMediaState, crate::browser::BrowserError> {
    let result = crate::browser::cef_engine::devtools(
        browser_id,
        "Runtime.evaluate",
        serde_json::json!({ "expression": browser_media_snapshot_script(), "returnByValue": true }),
        Duration::from_secs(5),
    )
    .await?;
    let raw = crate::browser::agent::cdp::evaluate_result(&result)?;
    let payload = crate::browser::picker::unwrap_eval_json(&raw);
    let report: MediaReport = serde_json::from_str(&payload).map_err(|error| {
        crate::browser::BrowserError::AutomationFailed(format!("invalid media snapshot: {error}"))
    })?;
    Ok(report.sanitize(browser_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn report(overrides: serde_json::Value) -> String {
        let mut base = json!({
            "title": "Song",
            "artist": "Artist",
            "album": "Album",
            "artworkUrl": "https://cdn.example.com/a.jpg",
            "playing": true,
            "muted": false,
            "hasMedia": true,
        });
        for (key, value) in overrides.as_object().expect("object") {
            base[key] = value.clone();
        }
        json!({ "v": 1, "kind": "media", "report": base }).to_string()
    }

    fn state(playing: bool, title: &str) -> BrowserMediaState {
        BrowserMediaState {
            browser_id: "b1".into(),
            title: title.into(),
            artist: String::new(),
            album: String::new(),
            artwork_url: String::new(),
            playing,
            muted: false,
            has_media: !title.is_empty(),
        }
    }

    #[test]
    fn state_serializes_with_camel_case_fields() {
        let value = serde_json::to_value(BrowserMediaState {
            browser_id: "b1".into(),
            title: "Song".into(),
            artist: "Artist".into(),
            album: "Album".into(),
            artwork_url: "https://cdn.example.com/a.jpg".into(),
            playing: true,
            muted: false,
            has_media: true,
        })
        .expect("serialize");
        assert_eq!(
            value,
            json!({
                "browserId": "b1",
                "title": "Song",
                "artist": "Artist",
                "album": "Album",
                "artworkUrl": "https://cdn.example.com/a.jpg",
                "playing": true,
                "muted": false,
                "hasMedia": true,
            })
        );
    }

    #[test]
    fn parses_a_well_formed_media_envelope() {
        let parsed = parse_media_envelope(&report(json!({})), "b1").expect("parse");
        assert_eq!(parsed.browser_id, "b1");
        assert_eq!(parsed.title, "Song");
        assert_eq!(parsed.artist, "Artist");
        assert_eq!(parsed.artwork_url, "https://cdn.example.com/a.jpg");
        assert!(parsed.playing);
        assert!(parsed.has_media);
    }

    #[test]
    fn refuses_wrong_kind_version_and_oversized_bodies() {
        assert!(parse_media_envelope(r#"{"v":1,"kind":"link","report":{}}"#, "b1").is_none());
        assert!(parse_media_envelope(r#"{"v":2,"kind":"media","report":{}}"#, "b1").is_none());
        assert!(parse_media_envelope("not json", "b1").is_none());
        let huge = report(json!({ "title": "x".repeat(MAX_ENVELOPE_BYTES) }));
        assert!(!is_media_envelope(&huge));
        assert!(parse_media_envelope(&huge, "b1").is_none());
    }

    #[test]
    fn clamps_page_text_and_strips_control_characters() {
        let title = "a".repeat(MAX_TITLE_CHARS + 50);
        let parsed = parse_media_envelope(
            &report(json!({
                "title": format!("{title}\u{0}\u{7}b"),
                "artist": "b".repeat(MAX_ARTIST_CHARS + 10),
                "album": "c".repeat(MAX_ALBUM_CHARS + 10),
            })),
            "b1",
        )
        .expect("parse");
        assert!(parsed.title.len() <= MAX_TITLE_CHARS);
        assert!(!parsed.title.contains('\u{0}'));
        assert!(parsed.artist.chars().count() <= MAX_ARTIST_CHARS);
        assert!(parsed.album.chars().count() <= MAX_ALBUM_CHARS);
        // Nothing is playing without media, whatever the report claims.
        let idle = parse_media_envelope(
            &report(json!({ "playing": true, "hasMedia": false })),
            "b1",
        )
        .expect("parse");
        assert!(!idle.playing);
        assert!(!idle.muted);
    }

    #[test]
    fn only_http_artwork_survives_clamping() {
        for (raw, kept) in [
            ("https://cdn.example.com/a.jpg", true),
            ("http://cdn.example.com/a.jpg", true),
            ("data:image/png;base64,AAAA", false),
            ("javascript:alert(1)", false),
            ("file:///etc/passwd", false),
            ("", false),
        ] {
            let parsed = parse_media_envelope(&report(json!({ "artworkUrl": raw })), "b1")
                .expect("parse");
            assert_eq!(parsed.artwork_url.is_empty(), !kept, "{raw}");
        }
        let long = format!("https://cdn.example.com/{}", "a".repeat(MAX_ARTWORK_CHARS * 2));
        let parsed = parse_media_envelope(&report(json!({ "artworkUrl": long })), "b1")
            .expect("parse");
        assert!(parsed.artwork_url.len() <= MAX_ARTWORK_CHARS);
    }

    #[test]
    fn text_clamping_collapses_whitespace() {
        assert_eq!(clamp_text("  a\n\t b  ", 100), "a b");
        assert_eq!(clamp_text("abc", 2), "ab");
    }

    #[test]
    fn throttle_emits_only_on_change_and_at_most_twice_a_second() {
        let start = Instant::now();
        let mut throttle = MediaEventThrottle::default();
        assert!(throttle.decide(&state(true, "A"), start));
        // Same state: never repeated, whatever the rate allows.
        assert!(!throttle.decide(&state(true, "A"), start + Duration::from_millis(100)));
        // Second distinct state inside the window: allowed.
        assert!(throttle.decide(&state(false, "A"), start + Duration::from_millis(200)));
        // Third inside the same second: suppressed by the rate limit.
        assert!(!throttle.decide(&state(true, "B"), start + Duration::from_millis(300)));
        // The suppressed state is delivered once the window rolls over.
        assert!(throttle.decide(&state(true, "B"), start + Duration::from_secs(1)));
        assert!(!throttle.decide(&state(true, "B"), start + Duration::from_millis(1100)));
    }

    #[test]
    fn throttle_forgets_a_closed_tab() {
        let start = Instant::now();
        let mut throttle = MediaEventThrottle::default();
        assert!(throttle.decide(&state(true, "A"), start));
        throttle.forget();
        assert!(throttle.decide(&state(true, "A"), start));
    }

    #[test]
    fn control_actions_are_a_closed_set() {
        for action in MediaControlAction::ALL {
            assert_eq!(MediaControlAction::parse(action.as_str()), Some(action));
        }
        assert_eq!(MediaControlAction::parse("play"), Some(MediaControlAction::Play));
        assert_eq!(MediaControlAction::parse("  mute "), Some(MediaControlAction::Mute));
        for raw in ["", "eval", "play;alert(1)", "PLAY", "seek"] {
            assert_eq!(MediaControlAction::parse(raw), None, "{raw}");
        }
    }

    #[test]
    fn control_script_embeds_only_the_action_token() {
        let play = browser_media_control_script(MediaControlAction::Play);
        assert!(play.contains("const action = 'play';"));
        assert!(play.contains("querySelectorAll('audio,video')"));
        assert!(!play.contains("ACTION"));
        // The mute action never carries another action's body.
        let mute = browser_media_control_script(MediaControlAction::Mute);
        assert!(mute.contains("target.muted = true"));
        assert!(!mute.contains("target.pause()"));
        assert!(!mute.contains("const action = 'toggle';"));
        // Every script reports the same outcome shape, so the command always parses it.
        for action in MediaControlAction::ALL {
            let script = browser_media_control_script(action);
            assert!(script.contains("applied: true"));
            assert!(script.contains("hasMedia: true"));
        }
    }

    #[test]
    fn control_result_round_trips_the_page_report() {
        let parsed: BrowserMediaControlResult = serde_json::from_str(
            r#"{"action":"toggle","applied":true,"playing":false,"muted":true,"hasMedia":true}"#,
        )
        .expect("parse");
        assert_eq!(parsed.action, "toggle");
        assert!(parsed.applied);
        assert!(parsed.muted);
        assert!(parsed.has_media);
        assert_eq!(parsed.reason, None);
        let narrow: BrowserMediaControlResult =
            serde_json::from_str(r#"{"action":"play","applied":false}"#).expect("parse");
        assert!(!narrow.applied);
        assert!(!narrow.playing);
    }

    #[test]
    fn snapshot_script_is_the_pull_form_of_the_shared_reporter() {
        let script = browser_media_snapshot_script();
        assert!(script.contains("(null)"));
        assert!(script.contains("navigator.mediaSession"));
    }
}
