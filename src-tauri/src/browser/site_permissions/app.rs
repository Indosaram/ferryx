//! The running app's side of site permissions: one broker and store for every tab, prompt
//! events for the panes, the Site settings commands, and the engine hooks.

use super::{
    load_store, save_store, site_origin, Answer, Broker, MediaCapture, Outcome, PendingPrompt,
    PermissionAsk, PermissionKind, SitePermission, SitePermissionDecision, SitePermissionStore, STORE_FILE,
};
use crate::ipc::error::{IpcError, IpcErrorCode};
use parking_lot::Mutex;
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, OnceLock};

/// A request waits for the user's answer in the pane: payload [`PendingPrompt`].
pub const BROWSER_PERMISSION_PROMPT_EVENT: &str = "browser_permission_prompt";
/// A prompt was answered or dropped: payload `{ browserId, requestId }`.
pub const BROWSER_PERMISSION_PROMPT_CLOSED_EVENT: &str = "browser_permission_prompt_closed";
/// What a tab captures changed: payload `{ browserId, origin, camera, microphone, screen }`.
pub const BROWSER_MEDIA_CAPTURE_EVENT: &str = "browser_media_capture_changed";
/// The remembered decisions changed (Site settings refreshes).
pub const BROWSER_SITE_PERMISSIONS_CHANGED_EVENT: &str = "browser_site_permissions_changed";

type Emit = Arc<dyn Fn(&'static str, serde_json::Value) + Send + Sync>;
type MainTask = Box<dyn FnOnce() + Send>;
type RunOnMain = Arc<dyn Fn(MainTask) + Send + Sync>;
type ProfileOf = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

struct Host {
    emit: Emit,
    run_on_main: RunOnMain,
    profile_of: ProfileOf,
}

#[derive(Debug, Clone, Default)]
struct Capture {
    origin: String,
    /// What the page was granted.
    granted: MediaCapture,
    /// What it is using now: the grant on the system webview, the engine's report on Chromium.
    active: MediaCapture,
}

#[derive(Default)]
struct State {
    broker: Broker,
    store: SitePermissionStore,
    store_path: Option<PathBuf>,
    save_seq: u64,
    capture: HashMap<String, Capture>,
}

static HOST: OnceLock<Host> = OnceLock::new();
static STATE: LazyLock<Mutex<State>> = LazyLock::new(Default::default);
/// The newest save sequence written to disk, so a slow older save never overwrites a newer one.
static SAVED: LazyLock<Mutex<u64>> = LazyLock::new(Default::default);

fn emit(event: &'static str, payload: serde_json::Value) {
    if let Some(host) = HOST.get() {
        (host.emit)(event, payload);
    }
}

/// Runs `task` on the main thread, where engine objects (WebKit blocks, WebView2 deferrals,
/// GObjects) must be answered. Without a running app the task is dropped, which denies.
pub(crate) fn run_on_main(task: MainTask) {
    if let Some(host) = HOST.get() {
        (host.run_on_main)(task);
    }
}

fn data_dir<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Option<PathBuf> {
    use tauri::Manager;
    if let Some(dir) = std::env::var_os("FERRYX_DATA_DIR").filter(|dir| !dir.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    app.path().app_data_dir().ok()
}

/// Wires site permissions to the app and loads the remembered decisions. Until they are
/// loaded every request is asked about; nothing is granted on a guess.
pub fn init<R: tauri::Runtime>(app: &tauri::AppHandle<R>, manager: Arc<crate::browser::BrowserManager>) {
    use tauri::Emitter;
    let emitter = app.clone();
    let runner = app.clone();
    let host = Host {
        emit: Arc::new(move |event, payload| {
            let _ = emitter.emit(event, payload);
        }),
        run_on_main: Arc::new(move |task| {
            if let Err(error) = runner.run_on_main_thread(task) {
                tracing::warn!(%error, "could not answer a site permission request on the main thread");
            }
        }),
        profile_of: Arc::new(move |browser_id| {
            manager.get_state(browser_id).ok().map(|state| state.profile_id.as_str().to_string())
        }),
    };
    if HOST.set(host).is_err() {
        return;
    }
    let Some(path) = data_dir(app).map(|dir| dir.join(STORE_FILE)) else {
        tracing::warn!("no app data dir: site permission decisions are not remembered");
        return;
    };
    STATE.lock().store_path = Some(path.clone());
    tauri::async_runtime::spawn(async move {
        let loaded = crate::ipc::run_blocking(move || {
            load_store(&path).map_err(|error| IpcError::new(IpcErrorCode::IoError, error.to_string()))
        })
        .await;
        let mut state = STATE.lock();
        match loaded {
            Ok(mut store) => {
                // Decisions made while the file was read win over the file.
                for entry in state.store.entries() {
                    store.set(&entry.profile, &entry.origin, entry.kind, entry.decision);
                }
                state.store = store;
            }
            Err(error) => tracing::warn!(error = %error.message, "could not read remembered site permissions"),
        }
    });
}

/// Writes the store in the background; returns at once.
fn save(state: &mut State) {
    let Some(path) = state.store_path.clone() else { return };
    state.save_seq += 1;
    let seq = state.save_seq;
    let snapshot = state.store.clone();
    tauri::async_runtime::spawn(async move {
        let saved = crate::ipc::run_blocking(move || {
            let mut last = SAVED.lock();
            if seq > *last {
                save_store(&path, &snapshot).map_err(|error| IpcError::new(IpcErrorCode::IoError, error.to_string()))?;
                *last = seq;
            }
            Ok(())
        })
        .await;
        if let Err(error) = saved {
            tracing::warn!(error = %error.message, "could not save site permission decisions");
        }
    });
}

fn capture_payload(browser_id: &str, capture: Option<&Capture>) -> serde_json::Value {
    let active = capture.map(|capture| capture.active).unwrap_or_default();
    json!({
        "browserId": browser_id,
        "origin": capture.map(|capture| capture.origin.clone()),
        "camera": active.camera,
        "microphone": active.microphone,
        "screen": active.screen,
    })
}

/// Records a grant of capture kinds; returns the payload to emit when it changed.
fn record_grant(state: &mut State, browser_id: &str, origin: &str, kinds: &[PermissionKind]) -> Option<serde_json::Value> {
    let grant = MediaCapture::of(kinds);
    if !grant.any() {
        return None;
    }
    let entry = state.capture.entry(browser_id.to_string()).or_default();
    if entry.origin != origin {
        *entry = Capture { origin: origin.to_string(), ..Capture::default() };
    }
    entry.granted = entry.granted.union(grant);
    entry.active = entry.active.union(grant);
    Some(capture_payload(browser_id, Some(&*entry)))
}

fn prompt_closed(prompt: &PendingPrompt) {
    emit(
        BROWSER_PERMISSION_PROMPT_CLOSED_EVENT,
        json!({ "browserId": prompt.browser_id, "requestId": prompt.request_id }),
    );
}

/// Asks about a request: a remembered decision answers it, otherwise the tab's pane prompts.
pub fn request(ask: PermissionAsk, answer: Answer) {
    let browser_id = ask.browser_id.clone();
    let origin = ask.origin.as_deref().and_then(site_origin).unwrap_or_default();
    let kinds = ask.kinds.clone();
    let (outcome, capture) = {
        let mut guard = STATE.lock();
        let state = &mut *guard;
        let outcome = state.broker.request(&state.store, ask, answer);
        let capture = match &outcome {
            Outcome::Decided(_, true) => record_grant(state, &browser_id, &origin, &kinds),
            _ => None,
        };
        (outcome, capture)
    };
    match outcome {
        Outcome::Decided(answer, allow) => answer.send(allow),
        Outcome::Prompt(prompt) => emit(BROWSER_PERMISSION_PROMPT_EVENT, json!(prompt)),
    }
    if let Some(payload) = capture {
        emit(BROWSER_MEDIA_CAPTURE_EVENT, payload);
    }
}

/// A request from a system-webview tab, identified by its browser id.
pub fn request_for_browser(browser_id: &str, origin: Option<String>, kinds: Vec<PermissionKind>, answer: Answer) {
    let profile = HOST.get().and_then(|host| (host.profile_of)(browser_id));
    let Some(profile) = profile else {
        // Not a tab Ferryx knows: denied.
        answer.send(false);
        return;
    };
    request(PermissionAsk { browser_id: browser_id.to_string(), profile, origin, kinds, external: None }, answer);
}

/// The tab closed: its unanswered requests are denied and its prompts and indicator go.
pub fn forget_browser(browser_id: &str) {
    #[cfg(target_os = "macos")]
    {
        let closed = browser_id.to_string();
        run_on_main(Box::new(move || super::macos::forget_browser(&closed)));
    }
    let (forgotten, had_capture) = {
        let mut state = STATE.lock();
        (state.broker.forget_browser(browser_id), state.capture.remove(browser_id).is_some())
    };
    for (prompt, answer) in forgotten {
        drop(answer);
        prompt_closed(&prompt);
    }
    if had_capture {
        emit(BROWSER_MEDIA_CAPTURE_EVENT, capture_payload(browser_id, None));
    }
}

/// Answers a pending prompt. Returns false when it is no longer pending.
pub fn decide(request_id: u64, allow: bool, remember: bool) -> bool {
    let (decided, capture) = {
        let mut guard = STATE.lock();
        let state = &mut *guard;
        let Some(decided) = state.broker.decide(&mut state.store, request_id, allow, remember) else {
            return false;
        };
        if decided.store_changed {
            save(state);
        }
        let capture = if allow {
            let prompt = &decided.prompt;
            record_grant(state, &prompt.browser_id, &prompt.origin, &prompt.kinds)
        } else {
            None
        };
        (decided, capture)
    };
    decided.answer.send(allow);
    prompt_closed(&decided.prompt);
    if let Some(payload) = capture {
        emit(BROWSER_MEDIA_CAPTURE_EVENT, payload);
    }
    if decided.store_changed {
        emit(BROWSER_SITE_PERMISSIONS_CHANGED_EVENT, json!({}));
    }
    true
}

/// Forgets remembered decisions; `kind` `None` forgets all of the site's.
pub fn revoke(profile: &str, origin: &str, kind: Option<PermissionKind>) -> bool {
    let changed = {
        let mut state = STATE.lock();
        let changed = state.store.revoke(profile, origin, kind);
        if changed {
            save(&mut state);
        }
        changed
    };
    if changed {
        emit(BROWSER_SITE_PERMISSIONS_CHANGED_EVENT, json!({}));
    }
    changed
}

/// The remembered decision for `kind` on `origin` (`scheme://host[:port]`) in `profile`,
/// without asking anyone. Before the store is loaded nothing is remembered, so it asks.
pub fn site_decision(profile: &str, origin: &str, kind: PermissionKind) -> SitePermissionDecision {
    STATE.lock().store.decision(profile, origin, kind)
}

/// A Chromium engine request; the engine holds it until [`decide`] or a dismissal.
#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
pub fn cef_request(profile: &str, tab: String, request_id: u64, kinds: Vec<PermissionKind>, origin: String) {
    let engine_profile = profile.to_string();
    let answer = Answer::new(Box::new(move |allow| {
        if !crate::browser::cef_engine::answer_permission(&engine_profile, request_id, allow) {
            tracing::debug!(profile = %engine_profile, request_id, "the engine was gone before its permission answer");
        }
    }));
    let ask = PermissionAsk {
        browser_id: tab,
        profile: profile.to_string(),
        origin: Some(origin),
        kinds,
        external: Some(format!("{profile}:{request_id}")),
    };
    request(ask, answer);
}

/// The engine dropped a request on its own (navigation, the tab closed).
#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
pub fn cef_dismissed(profile: &str, request_id: u64) {
    let dismissed = STATE.lock().broker.dismiss_external(&format!("{profile}:{request_id}"));
    if let Some((prompt, answer)) = dismissed {
        drop(answer);
        prompt_closed(&prompt);
    }
}

/// The engine reports the tab's camera (`video`) and microphone (`audio`) use.
#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
pub fn cef_media_access(tab: &str, video: bool, audio: bool) {
    let payload = {
        let mut state = STATE.lock();
        let entry = state.capture.entry(tab.to_string()).or_default();
        // Video is a camera unless the page was granted only screen capture.
        let screen_only = entry.granted.screen && !entry.granted.camera;
        let active = MediaCapture { camera: video && !screen_only, microphone: audio, screen: video && screen_only };
        if entry.active == active {
            return;
        }
        entry.active = active;
        capture_payload(tab, Some(&*entry))
    };
    emit(BROWSER_MEDIA_CAPTURE_EVENT, payload);
}

/// Hooks a fresh system-webview tab's permission requests (main thread, after creation).
pub fn install_webview<R: tauri::Runtime>(webview: &tauri::Webview<R>, browser_id: &str) {
    #[cfg(target_os = "macos")]
    super::macos::install(webview, browser_id);
    #[cfg(target_os = "windows")]
    super::windows::install(webview, browser_id);
    #[cfg(target_os = "linux")]
    super::linux::install(webview, browser_id);
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    let _ = (webview, browser_id);
}

fn parse_kind_arg(kind: Option<&str>) -> Result<Option<PermissionKind>, IpcError> {
    kind.map(|name| {
        PermissionKind::parse(name)
            .ok_or_else(|| IpcError::new(IpcErrorCode::InvalidArgument, format!("unknown permission kind {name}")))
    })
    .transpose()
}

/// Answers the pane's prompt. `false` when the request is no longer pending.
#[tauri::command]
pub async fn cmd_browser_permission_decide(request_id: u64, allow: bool, remember: bool) -> Result<bool, IpcError> {
    Ok(decide(request_id, allow, remember))
}

/// The prompts a tab waits on (a pane that mounts late shows them).
#[tauri::command]
pub async fn cmd_browser_permission_pending(browser_id: String) -> Result<Vec<PendingPrompt>, IpcError> {
    Ok(STATE.lock().broker.pending_for(&browser_id))
}

/// The tab's camera, microphone and screen use, for the in-use indicator.
#[tauri::command]
pub async fn cmd_browser_media_capture(browser_id: String) -> Result<serde_json::Value, IpcError> {
    let state = STATE.lock();
    Ok(capture_payload(&browser_id, state.capture.get(&browser_id)))
}

/// Every remembered decision, for Settings > Browser > Site settings.
#[tauri::command]
pub async fn cmd_browser_site_permissions_list() -> Result<Vec<SitePermission>, IpcError> {
    Ok(STATE.lock().store.entries().to_vec())
}

/// Forgets a site's remembered decision for `kind`, or all of them when `kind` is absent.
#[tauri::command]
pub async fn cmd_browser_site_permission_revoke(
    profile: String,
    origin: String,
    kind: Option<String>,
) -> Result<bool, IpcError> {
    let kind = parse_kind_arg(kind.as_deref())?;
    Ok(revoke(&profile, &origin, kind))
}
