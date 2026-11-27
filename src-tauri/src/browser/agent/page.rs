//! One page-access seam for both engines: a Chromium (CEF) engine tab is reached over the
//! engine's in-process DevTools API, a system webview tab through its webview. The observe/act
//! entry points in `ipc::browser` call these instead of picking an engine themselves.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

use base64::Engine as _;
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

use crate::browser::model::BrowserAutomationAction;
use crate::browser::snapshot_source::{BrowserSnapshot, SnapshotOptions};
use crate::browser::{BrowserError, BrowserManager};
use crate::ipc::browser_cli::BrowserNativeInputAction;
use crate::ipc::error::IpcError;

/// Maximum size for a single agent file upload (plan C2: 32 MiB).
pub const MAX_UPLOAD_BYTES: u64 = 32 * 1024 * 1024;

type StagedUploadsMap = HashMap<String, Vec<(PathBuf, u64)>>;

static STAGED_UPLOADS: LazyLock<Mutex<StagedUploadsMap>> = LazyLock::new(Default::default);

/// Removes owned upload staging files created for `browser_id` (called on tab close,
/// reclaim/handoff, or controller revocation).
pub fn cleanup_upload_staging(browser_id: &str) {
    let paths = STAGED_UPLOADS.lock().remove(browser_id).unwrap_or_default();
    for (path, _) in paths {
        let _ = std::fs::remove_file(&path);
    }
}

/// Most staged files kept per tab. An engine tab's file must outlive `DOM.setFileInputFiles`
/// (Chromium reads it lazily on read or submit), so older ones are evicted instead of deleted
/// right after the upload.
///
/// The cap is a leak bound, not a form limit: a page may carry a multi-input form whose inputs
/// are filled by several `upload` calls before one submit, so the count is high enough that a
/// real form survives and the byte budget below is what actually bounds the disk.
const MAX_STAGED_PER_TAB: usize = 16;

/// Bytes one tab may keep staged. Bounds the leak at 128 MiB per tab however many uploads run.
const MAX_STAGED_BYTES_PER_TAB: u64 = 128 * 1024 * 1024;

/// Records `(path, len)` for `browser_id`, deleting the oldest staged files beyond either budget.
fn register_staged(
    map: &mut HashMap<String, Vec<(PathBuf, u64)>>,
    browser_id: &str,
    path: PathBuf,
    len: u64,
) {
    let staged = map.entry(browser_id.to_string()).or_default();
    staged.push((path, len));
    // Evict oldest first until both budgets hold. A single oversized entry is kept: it was
    // already accepted by `MAX_UPLOAD_BYTES`, and dropping it would break the upload in flight.
    while staged.len() > MAX_STAGED_PER_TAB
        || (staged.len() > 1 && staged.iter().map(|(_, len)| len).sum::<u64>() > MAX_STAGED_BYTES_PER_TAB)
    {
        let (evicted, _) = staged.remove(0);
        let _ = std::fs::remove_file(evicted);
    }
}

#[derive(Debug)]
enum BoundedReadError {
    TooLarge,
    Io(std::io::Error),
}

/// Reads at most `max` bytes; a source that yields more (a file that grew after it was
/// checked) is refused without ever buffering more than `max + 1` bytes.
fn read_bounded(reader: &mut impl std::io::Read, max: u64) -> Result<Vec<u8>, BoundedReadError> {
    use std::io::Read as _;
    let mut bytes = Vec::new();
    reader
        .take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(BoundedReadError::Io)?;
    if bytes.len() as u64 > max {
        return Err(BoundedReadError::TooLarge);
    }
    Ok(bytes)
}

#[derive(Debug)]
struct StagedUpload {
    staged_path: PathBuf,
    file_name: String,
    mime: String,
    bytes: Vec<u8>,
    sha256: String,
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn guess_mime(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("txt") => "text/plain",
        Some("json") => "application/json",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("pdf") => "application/pdf",
        Some("html" | "htm") => "text/html",
        Some("csv") => "text/csv",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
pub(crate) fn register_staged_for_test(browser_id: &str, path: PathBuf, len: u64) {
    register_staged(&mut STAGED_UPLOADS.lock(), browser_id, path, len);
}

#[cfg(test)]
pub(crate) fn staged_count_for_test(browser_id: &str) -> usize {
    STAGED_UPLOADS.lock().get(browser_id).map_or(0, Vec::len)
}

fn stage_upload_blocking(browser_id: &str, raw_path: &str) -> Result<StagedUpload, BrowserError> {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return Err(BrowserError::AutomationFailed("upload path cannot be empty".into()));
    }
    let canonical = std::fs::canonicalize(trimmed).map_err(|error| {
        BrowserError::AutomationFailed(format!("cannot open upload file `{trimmed}`: {error}"))
    })?;
    // Refuse a non-regular file *before* opening it: `open(2)` on a FIFO with no writer blocks
    // until one appears, which would park this blocking-pool thread forever. The handle check
    // below still runs, because the path can be swapped between the stat and the open.
    let meta = std::fs::metadata(&canonical).map_err(|error| {
        BrowserError::AutomationFailed(format!("cannot stat upload file `{trimmed}`: {error}"))
    })?;
    if !meta.is_file() {
        return Err(BrowserError::AutomationFailed(format!(
            "upload target `{trimmed}` is not a regular file"
        )));
    }
    let mut file = std::fs::File::open(&canonical).map_err(|error| {
        BrowserError::AutomationFailed(format!("cannot open upload file `{trimmed}`: {error}"))
    })?;
    let meta = file.metadata().map_err(|error| {
        BrowserError::AutomationFailed(format!("cannot stat upload file `{trimmed}`: {error}"))
    })?;
    if !meta.is_file() {
        return Err(BrowserError::AutomationFailed(format!(
            "upload target `{trimmed}` is not a regular file"
        )));
    }
    let bytes = read_bounded(&mut file, MAX_UPLOAD_BYTES).map_err(|error| match error {
        BoundedReadError::TooLarge => {
            BrowserError::AutomationFailed("upload file exceeds 32 MiB limit".into())
        }
        BoundedReadError::Io(error) => {
            BrowserError::AutomationFailed(format!("cannot read upload file `{trimmed}`: {error}"))
        }
    })?;
    let sha256 = sha256_hex(&bytes);
    let file_name = canonical
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| !n.is_empty())
        .unwrap_or("upload.bin")
        .to_string();
    let mime = guess_mime(&canonical).to_string();

    let stage_dir = std::env::temp_dir().join(format!("ferryx-upload-stage-{}", std::process::id()));
    std::fs::create_dir_all(&stage_dir).map_err(|error| {
        BrowserError::AutomationFailed(format!("cannot create upload staging dir: {error}"))
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&stage_dir, std::fs::Permissions::from_mode(0o700));
    }
    let safe_name: String = file_name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' })
        .collect();
    let staged_path = stage_dir.join(format!("{}-{safe_name}", uuid::Uuid::new_v4()));
    {
        use std::io::Write;
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut file = opts.open(&staged_path).map_err(|error| {
            BrowserError::AutomationFailed(format!("cannot create upload staging file: {error}"))
        })?;
        file.write_all(&bytes).map_err(|error| {
            let _ = std::fs::remove_file(&staged_path);
            BrowserError::AutomationFailed(format!("cannot write upload staging file: {error}"))
        })?;
    }
    register_staged(
        &mut STAGED_UPLOADS.lock(),
        browser_id,
        staged_path.clone(),
        bytes.len() as u64,
    );
    Ok(StagedUpload {
        staged_path,
        file_name,
        mime,
        bytes,
        sha256,
    })
}

/// Whether the tab is served by the Chromium engine.
pub fn is_engine_tab(browser_id: &str) -> bool {
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    {
        crate::browser::cef_engine::is_cef_tab(browser_id)
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        let _ = browser_id;
        false
    }
}

/// Evaluates a page script in the tab and returns the JSON text of its value.
pub async fn eval<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    browser_id: &str,
    script: String,
) -> Result<String, BrowserError> {
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    if is_engine_tab(browser_id) {
        return super::cef_backend::eval(browser_id, &script).await;
    }
    let label = manager.get_webview_label(browser_id)?;
    let webview = app.get_webview(&label).ok_or(BrowserError::WebviewNotFound(label))?;
    crate::ipc::browser::eval_webview(webview, script).await
}

/// Evaluates an agent script (`observe.js`): in an engine tab inside its isolated world, in a
/// webview tab in the page's world (the script builds its own clean realm there).
pub async fn eval_agent<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    browser_id: &str,
    script: String,
) -> Result<String, BrowserError> {
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    if is_engine_tab(browser_id) {
        return super::cef_backend::eval_isolated(browser_id, &script).await;
    }
    eval(app, manager, browser_id, script).await
}

/// Whether agent scripts in this tab run in an isolated world.
pub fn agent_world_isolated(browser_id: &str) -> bool {
    is_engine_tab(browser_id)
}

/// Runs an element action addressed by ref, bound to the fingerprint its observe recorded.
pub async fn act_element<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    browser_id: &str,
    action: &BrowserAutomationAction,
    expect: &str,
) -> Result<(), BrowserError> {
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    if is_engine_tab(browser_id) {
        return super::cef_backend::act_element(browser_id, action, expect).await;
    }
    let script = super::observe::act_script(action, expect, false)
        .ok_or_else(|| BrowserError::AutomationFailed("the action addresses no element".into()))?;
    super::service::recheck_act(browser_id)?;
    super::observe::parse_act(&eval(app, manager, browser_id, script).await?)
}

/// Runs the trusted-input form of an action that addresses no element, in an engine tab.
/// `Ok(false)` means the caller runs its DOM script instead (always for webview tabs).
pub async fn trusted_act(browser_id: &str, action: &BrowserAutomationAction) -> Result<bool, BrowserError> {
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    if is_engine_tab(browser_id) {
        return super::cef_backend::act(browser_id, action).await;
    }
    let _ = (browser_id, action);
    Ok(false)
}

/// Captures the tab's visible page: CDP for an engine tab, native webview capture otherwise.
pub async fn capture<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &Arc<BrowserManager>,
    browser_id: &str,
    options: SnapshotOptions,
) -> Result<BrowserSnapshot, IpcError> {
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    if is_engine_tab(browser_id) {
        return Ok(super::cef_backend::capture(browser_id).await?);
    }
    let state = manager.get_state(browser_id)?;
    crate::browser::capture_browser_snapshot(app, &state.webview_label, options).await
}

/// Validates and snapshots a local file (<= 32 MiB) into an owned staging file, then attaches it
/// to the observed `<input type="file">` element: `DOM.setFileInputFiles` on Chromium engine tabs,
/// `DataTransfer` fallback on system webview tabs.
pub async fn upload_file<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    browser_id: &str,
    reference: &str,
    expect: &str,
    raw_path: &str,
) -> Result<(String, u64, String), BrowserError> {
    let bid = browser_id.to_string();
    let path_owned = raw_path.to_string();
    let staged = crate::ipc::run_blocking(move || Ok(stage_upload_blocking(&bid, &path_owned)))
        .await
        .map_err(|error| BrowserError::AutomationFailed(error.message))??;
    let byte_len = staged.bytes.len() as u64;

    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    if is_engine_tab(browser_id) {
        let staged_str = staged.staged_path.to_string_lossy().to_string();
        if let Err(error) = super::cef_backend::upload_file(browser_id, reference, expect, &staged_str).await {
            let _ = std::fs::remove_file(&staged.staged_path);
            return Err(error);
        }
        return Ok((staged.file_name, byte_len, staged.sha256));
    }

    let b64 = base64::engine::general_purpose::STANDARD.encode(&staged.bytes);
    let script = super::observe::upload_webview_script(
        reference,
        expect,
        &staged.file_name,
        &staged.mime,
        &b64,
    );
    let _ = std::fs::remove_file(&staged.staged_path);
    super::service::recheck_act(browser_id)?;
    super::observe::parse_act(&eval(app, manager, browser_id, script).await?)?;
    Ok((staged.file_name, byte_len, staged.sha256))
}

/// Dispatches trusted native input on a Chromium engine tab; refuses honestly on system webviews
/// where synthetic DOM events cannot be trusted.
pub async fn native_input(
    browser_id: &str,
    action: &BrowserNativeInputAction,
) -> Result<&'static str, BrowserError> {
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    if is_engine_tab(browser_id) {
        super::cef_backend::native_input(browser_id, action).await?;
        return Ok("trusted");
    }
    let _ = (browser_id, action);
    Err(BrowserError::PlatformUnsupported(
        "trusted native input requires a Chromium engine tab; use request_help to hand off to the user".into(),
    ))
}

#[cfg(test)]
mod staging_tests {
    use super::*;

    #[test]
    fn bounded_read_refuses_a_source_larger_than_the_cap_without_buffering_it_all() {
        struct Endless(u64);
        impl std::io::Read for Endless {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                self.0 += buf.len() as u64;
                buf.fill(7);
                Ok(buf.len())
            }
        }
        let mut endless = Endless(0);
        assert!(matches!(read_bounded(&mut endless, 1024), Err(BoundedReadError::TooLarge)));
        assert!(endless.0 <= 1024 + 1 + 8 * 1024, "read {} bytes", endless.0);
        assert_eq!(read_bounded(&mut &b"abc"[..], 3).unwrap(), b"abc");
        assert!(matches!(read_bounded(&mut &b"abcd"[..], 3), Err(BoundedReadError::TooLarge)));
    }

    #[test]
    fn staging_is_capped_per_tab_and_evicts_the_oldest_file() {
        let dir = std::env::temp_dir().join(format!("ferryx-staging-cap-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut map = HashMap::new();
        let paths: Vec<PathBuf> = (0..MAX_STAGED_PER_TAB + 2)
            .map(|i| {
                let path = dir.join(format!("f{i}"));
                std::fs::write(&path, b"x").unwrap();
                register_staged(&mut map, "tab", path.clone(), 1);
                path
            })
            .collect();
        assert_eq!(map["tab"].len(), MAX_STAGED_PER_TAB);
        assert!(!paths[0].exists() && !paths[1].exists(), "evicted files are removed");
        assert!(paths[2..].iter().all(|path| path.exists()), "recent files stay for lazy reads");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_multi_input_form_survives_because_the_cap_is_not_a_form_limit() {
        // Five file inputs on one page are five uploads before one submit: none may be evicted.
        let dir = std::env::temp_dir().join(format!("ferryx-staging-form-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut map = HashMap::new();
        let paths: Vec<PathBuf> = (0..5)
            .map(|i| {
                let path = dir.join(format!("input{i}"));
                std::fs::write(&path, b"x").unwrap();
                register_staged(&mut map, "tab", path.clone(), 1);
                path
            })
            .collect();
        assert_eq!(map["tab"].len(), 5);
        assert!(paths.iter().all(|path| path.exists()), "every input of the form is still staged");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn staging_evicts_the_oldest_file_when_the_byte_budget_is_exceeded() {
        let dir = std::env::temp_dir().join(format!("ferryx-staging-bytes-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut map = HashMap::new();
        let budget = MAX_STAGED_BYTES_PER_TAB;
        let first = dir.join("old");
        std::fs::write(&first, b"x").unwrap();
        register_staged(&mut map, "tab", first.clone(), budget / 2);
        let second = dir.join("kept");
        std::fs::write(&second, b"x").unwrap();
        register_staged(&mut map, "tab", second.clone(), budget / 2 + 1);
        assert!(!first.exists(), "the oldest entry is evicted to hold the byte budget");
        assert!(second.exists());
        assert_eq!(map["tab"].len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn stage_upload_rejects_non_regular_files() {
        let dir = std::env::temp_dir();
        let error = stage_upload_blocking("tab-not-file", &dir.to_string_lossy()).unwrap_err();
        assert!(error.to_string().contains("not a regular file"), "{error}");
    }
}

#[cfg(all(test, any(target_os = "windows", target_os = "macos", target_os = "linux")))]
mod tests {
    use serde_json::{json, Value};

    use super::super::fake_cdp::{evaluated, op_of, world, FakeCdp, Reply};
    use super::*;
    use crate::browser::model::{BrowserAutomationRequest, BrowserWaitCondition, CreateBrowserRequest};
    use crate::ipc::browser::{browser_automation_act, browser_automation_snapshot};
    use crate::ipc::error::IpcErrorCode;

    const SAVE: &str = "button\nbutton\nSave";

    fn mock_app() -> tauri::App<tauri::test::MockRuntime> {
        tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app")
    }

    /// A browser session whose id is `browser_id` (the stand-in engine's tab when it serves one).
    fn session(manager: &BrowserManager, browser_id: &str) -> crate::browser::BrowserState {
        manager
            .register_session(CreateBrowserRequest {
                browser_id: Some(browser_id.into()),
                workspace_id: None,
                worktree_path: None,
                url: "https://fixture.test/form".into(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .expect("register session")
    }

    /// The fixture page as observe.js reports it: a Save button and an email box, with a token
    /// in the title and the URL that the snapshot must not carry.
    fn observation() -> Value {
        json!({
            "ok": true,
            "url": "https://fixture.test/form?token=abc123secret",
            "title": "Form ghp_abcdefghijklmnopqrstuvwxyz0123",
            "doc": "d1",
            "elements": [
                { "reference": "d1.1", "role": "button", "name": "Save", "tagName": "button" },
                { "reference": "d1.2", "role": "textbox", "name": "Email", "tagName": "input" }
            ],
            "frames": [],
            "truncated": false
        })
    }

    #[tokio::test]
    async fn engine_tabs_take_the_cdp_path_and_webview_tabs_their_webview() {
        let app = mock_app();
        let manager = Arc::new(BrowserManager::new());
        let cdp = FakeCdp::start("page-seam", |method, params| {
            world(method, 11).unwrap_or_else(|| match params.get("contextId") {
                Some(_) => evaluated(json!({ "ok": true })),
                None => Reply::Ok(json!({ "result": { "type": "number", "value": 2 } })),
            })
        });
        session(&manager, &cdp.tab);
        let webview_tab = session(&manager, "page-seam-webview");

        assert!(is_engine_tab(&cdp.tab));
        assert!(agent_world_isolated(&cdp.tab));
        assert!(!is_engine_tab(&webview_tab.browser_id));
        assert!(!agent_world_isolated(&webview_tab.browser_id));

        assert_eq!(eval(app.handle(), &manager, &cdp.tab, "1 + 1".into()).await.unwrap(), "2");
        assert_eq!(eval_agent(app.handle(), &manager, &cdp.tab, "x".into()).await.unwrap(), r#"{"ok":true}"#);
        assert_eq!(
            cdp.methods(),
            ["Runtime.evaluate", "Page.getFrameTree", "Page.createIsolatedWorld", "Runtime.evaluate"]
        );
        assert!(cdp.calls()[0].1.get("contextId").is_none());
        assert_eq!(cdp.calls()[3].1["contextId"], 11);

        // The webview tab never reaches the engine; with no webview it says so.
        let label = webview_tab.webview_label.clone();
        for result in [
            eval(app.handle(), &manager, &webview_tab.browser_id, "1".into()).await,
            eval_agent(app.handle(), &manager, &webview_tab.browser_id, "1".into()).await,
            act_element(app.handle(), &manager, &webview_tab.browser_id, &BrowserAutomationAction::Click { reference: "d1.1".into() }, SAVE)
                .await
                .map(|()| String::new()),
        ] {
            assert_eq!(result, Err(BrowserError::WebviewNotFound(label.clone())));
        }
        let keypress = BrowserAutomationAction::Keypress { key: "Enter".into() };
        assert!(!trusted_act(&webview_tab.browser_id, &keypress).await.unwrap());
        assert!(trusted_act(&cdp.tab, &keypress).await.unwrap());
        assert_eq!(cdp.methods().iter().filter(|method| *method == "Input.dispatchKeyEvent").count(), 2);
        assert_eq!(cdp.expressions().len(), 2);

        // A tab neither engine knows.
        assert_eq!(
            eval(app.handle(), &manager, "page-seam-missing", "1".into()).await,
            Err(BrowserError::NotFound("page-seam-missing".into()))
        );
    }

    #[tokio::test]
    async fn capture_of_an_engine_tab_is_a_cdp_screenshot() {
        use base64::Engine as _;
        let app = mock_app();
        let manager = Arc::new(BrowserManager::new());
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend_from_slice(&320u32.to_be_bytes());
        png.extend_from_slice(&200u32.to_be_bytes());
        let data = base64::engine::general_purpose::STANDARD.encode(&png);
        let cdp = FakeCdp::start("page-capture", move |_, _| Reply::Ok(json!({ "data": data })));
        session(&manager, &cdp.tab);
        let snapshot = capture(app.handle(), &manager, &cdp.tab, SnapshotOptions::png()).await.unwrap();
        assert_eq!((snapshot.width, snapshot.height, snapshot.bytes), (320, 200, png));
        assert_eq!(cdp.methods(), ["Page.captureScreenshot"]);

        let missing = capture(app.handle(), &manager, "page-capture-missing", SnapshotOptions::png())
            .await
            .expect_err("unknown tab");
        assert_eq!(missing.code, IpcErrorCode::BrowserNotFound);
    }

    #[tokio::test]
    async fn snapshot_then_act_on_an_engine_tab_runs_isolated_with_trusted_input() {
        let app = mock_app();
        let manager = Arc::new(BrowserManager::new());
        let checks = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let sampled = Arc::clone(&checks);
        let cdp = FakeCdp::start("page-flow", move |method, params| {
            world(method, 21).unwrap_or_else(|| match (method, op_of(params)) {
                ("Runtime.evaluate", "observe") => evaluated(observation()),
                ("Runtime.evaluate", "point") => evaluated(json!({ "ok": true, "point": { "x": 10.0, "y": 20.0 } })),
                // The postcondition is not met before the click and met after it.
                ("Runtime.evaluate", "check") => {
                    let seen = sampled.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    evaluated(json!({ "ok": true, "matched": seen > 0 }))
                }
                ("Runtime.evaluate", other) => panic!("unexpected {other} script"),
                _ => Reply::Ok(json!({})),
            })
        });
        let state = session(&manager, &cdp.tab);

        let snapshot = browser_automation_snapshot(app.handle().clone(), &manager, cdp.tab.clone()).await.unwrap();
        assert!(snapshot.isolated);
        assert_eq!(snapshot.generation, state.generation);
        assert_eq!(snapshot.elements.len(), 2);
        assert_eq!((snapshot.elements[0].reference.as_str(), snapshot.elements[0].name.as_str()), ("d1.1", "Save"));
        assert!(!snapshot.url.contains("abc123secret"), "{}", snapshot.url);
        assert!(!snapshot.title.contains("ghp_"), "{}", snapshot.title);
        assert!(cdp.expressions()[0].contains(r#""isolated":true"#));
        assert_eq!(manager.observed_ref(&cdp.tab, state.generation, "d1.1").unwrap(), SAVE);

        let outcome = browser_automation_act(
            app.handle().clone(),
            &manager,
            BrowserAutomationRequest {
                browser_id: cdp.tab.clone(),
                generation: snapshot.generation,
                action: BrowserAutomationAction::Click { reference: "d1.1".into() },
                postcondition: Some(BrowserWaitCondition::Text { text: "Saved".into() }),
            },
        )
        .await
        .unwrap();
        assert!(outcome.dispatched);
        assert_eq!(outcome.applied, Some(true));
        let ops: Vec<&str> = cdp
            .calls()
            .iter()
            .filter(|(method, _)| method == "Runtime.evaluate")
            .map(|(_, params)| op_of(params))
            .collect();
        assert_eq!(ops, ["observe", "check", "point", "check"]);
        assert!(cdp
            .calls()
            .iter()
            .filter(|(method, _)| method == "Runtime.evaluate")
            .all(|(_, params)| params["contextId"] == 21));
        assert_eq!(cdp.methods().iter().filter(|method| *method == "Input.dispatchMouseEvent").count(), 3);
        assert!(cdp.expressions()[2].contains(&serde_json::to_string(SAVE).unwrap()));
    }

    #[tokio::test]
    async fn a_postcondition_already_shown_reports_no_applied() {
        let app = mock_app();
        let manager = Arc::new(BrowserManager::new());
        let cdp = FakeCdp::start("page-already", |method, params| {
            world(method, 22).unwrap_or_else(|| match op_of(params) {
                "observe" => evaluated(observation()),
                "check" => evaluated(json!({ "ok": true, "matched": true })),
                _ => evaluated(json!({ "ok": true })),
            })
        });
        session(&manager, &cdp.tab);
        let snapshot = browser_automation_snapshot(app.handle().clone(), &manager, cdp.tab.clone()).await.unwrap();
        let outcome = browser_automation_act(
            app.handle().clone(),
            &manager,
            BrowserAutomationRequest {
                browser_id: cdp.tab.clone(),
                generation: snapshot.generation,
                action: BrowserAutomationAction::Fill { reference: "d1.2".into(), value: "a@b.test".into() },
                postcondition: Some(BrowserWaitCondition::UrlContains { fragment: "/form".into() }),
            },
        )
        .await
        .unwrap();
        assert!(outcome.dispatched);
        assert_eq!(outcome.applied, None, "a state the page already showed proves nothing");
        let ops: Vec<&str> = cdp
            .calls()
            .iter()
            .filter(|(method, _)| method == "Runtime.evaluate")
            .map(|(_, params)| op_of(params))
            .collect();
        assert_eq!(ops, ["observe", "check", "act"]);
        assert!(cdp.expressions()[2].contains(r#""expect":"input\ntextbox\n""#));
    }

    #[tokio::test]
    async fn a_ref_from_a_stale_snapshot_never_reaches_the_page() {
        let app = mock_app();
        let manager = Arc::new(BrowserManager::new());
        let cdp = FakeCdp::start("page-stale", |method, params| {
            world(method, 23).unwrap_or_else(|| match op_of(params) {
                "observe" => evaluated(observation()),
                _ => panic!("a stale ref must not touch the page"),
            })
        });
        session(&manager, &cdp.tab);
        let snapshot = browser_automation_snapshot(app.handle().clone(), &manager, cdp.tab.clone()).await.unwrap();
        let request = |generation: u64, reference: &str, postcondition: Option<BrowserWaitCondition>| BrowserAutomationRequest {
            browser_id: cdp.tab.clone(),
            generation,
            action: BrowserAutomationAction::Click { reference: reference.into() },
            postcondition,
        };

        // A ref the observe never issued.
        let unknown = browser_automation_act(app.handle().clone(), &manager, request(snapshot.generation, "d1.99", None))
            .await
            .expect_err("unknown ref");
        assert_eq!(unknown.code, IpcErrorCode::BrowserInvalidReference);
        // A postcondition that would run a script is refused before anything else.
        let scripted = browser_automation_act(
            app.handle().clone(),
            &manager,
            request(snapshot.generation, "d1.1", Some(BrowserWaitCondition::Function { script: "true".into() })),
        )
        .await
        .expect_err("script postcondition");
        assert_eq!(scripted.code, IpcErrorCode::BrowserAutomationFailed);

        // The tab reloaded after the snapshot: its refs are stale.
        manager.begin_reload(&cdp.tab).unwrap();
        let stale = browser_automation_act(app.handle().clone(), &manager, request(snapshot.generation, "d1.1", None))
            .await
            .expect_err("stale snapshot");
        assert_eq!(stale.code, IpcErrorCode::BrowserStaleRef);
        let current = manager.get_state(&cdp.tab).unwrap().generation;
        let unobserved = browser_automation_act(app.handle().clone(), &manager, request(current, "d1.1", None))
            .await
            .expect_err("nothing observed in the new document");
        assert_eq!(unobserved.code, IpcErrorCode::BrowserInvalidReference);

        assert_eq!(cdp.expressions().len(), 1, "only the observe ran");
        assert!(!cdp.methods().iter().any(|method| method.starts_with("Input.")));
    }

    #[tokio::test]
    async fn a_node_replaced_since_the_observe_fails_as_stale_in_the_page() {
        let app = mock_app();
        let manager = Arc::new(BrowserManager::new());
        let cdp = FakeCdp::start("page-replaced", |method, params| {
            world(method, 24).unwrap_or_else(|| match op_of(params) {
                "observe" => evaluated(observation()),
                _ => evaluated(json!({ "ok": false, "code": "changed", "message": "the element changed since the observe; observe again" })),
            })
        });
        session(&manager, &cdp.tab);
        let snapshot = browser_automation_snapshot(app.handle().clone(), &manager, cdp.tab.clone()).await.unwrap();
        let error = browser_automation_act(
            app.handle().clone(),
            &manager,
            BrowserAutomationRequest {
                browser_id: cdp.tab.clone(),
                generation: snapshot.generation,
                action: BrowserAutomationAction::Click { reference: "d1.1".into() },
                postcondition: None,
            },
        )
        .await
        .expect_err("changed node");
        assert_eq!(error.code, IpcErrorCode::BrowserStaleRef);
        assert!(!cdp.methods().iter().any(|method| method.starts_with("Input.")));
    }

    #[tokio::test]
    async fn an_untargeted_act_on_an_engine_tab_is_trusted_input_on_every_os() {
        let app = mock_app();
        let manager = Arc::new(BrowserManager::new());
        let cdp = FakeCdp::start("page-keys", |_, params| match op_of(params) {
            "page" if params.get("expression").is_some() => Reply::Ok(json!({ "result": { "type": "string", "value": "ok" } })),
            _ => Reply::Ok(json!({})),
        });
        let state = session(&manager, &cdp.tab);
        let request = |action: BrowserAutomationAction, generation: u64| BrowserAutomationRequest {
            browser_id: cdp.tab.clone(),
            generation,
            action,
            postcondition: None,
        };
        let enter = || BrowserAutomationAction::Keypress { key: "Enter".into() };
        let outcome = browser_automation_act(app.handle().clone(), &manager, request(enter(), state.generation))
            .await
            .unwrap();
        assert!(outcome.dispatched && outcome.applied.is_none());
        let scroll = BrowserAutomationAction::Scroll { reference: None, x: None, y: Some(300) };
        browser_automation_act(app.handle().clone(), &manager, request(scroll, state.generation))
            .await
            .unwrap();
        assert_eq!(cdp.methods(), ["Input.dispatchKeyEvent", "Input.dispatchKeyEvent", "Runtime.evaluate"]);
        // The scroll script is a page script, not an agent one.
        assert!(cdp.calls()[2].1.get("contextId").is_none());

        let stale = browser_automation_act(app.handle().clone(), &manager, request(enter(), state.generation + 1))
            .await
            .expect_err("wrong generation");
        assert_eq!(stale.code, IpcErrorCode::BrowserStaleRef);
        assert_eq!(cdp.calls().len(), 3);
    }

    #[tokio::test]
    async fn upload_staging_snapshots_file_hash_and_cleans_up_on_tab_close() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file_path = dir.path().join("sample.txt");
        let payload = b"ferryx-upload-verification-bytes";
        std::fs::write(&file_path, payload).expect("write fixture");

        let staged = stage_upload_blocking("tab-stage-test", file_path.to_str().unwrap())
            .expect("valid file stages");
        assert_eq!(staged.file_name, "sample.txt");
        assert_eq!(staged.mime, "text/plain");
        assert_eq!(staged.bytes, payload);
        assert_eq!(staged.sha256, sha256_hex(payload));
        assert!(staged.staged_path.exists());

        cleanup_upload_staging("tab-stage-test");
        assert!(!staged.staged_path.exists(), "owned staging file must be deleted on cleanup");

        let dir_err = stage_upload_blocking("tab-stage-test", dir.path().to_str().unwrap())
            .expect_err("directory must be refused");
        assert!(matches!(dir_err, BrowserError::AutomationFailed(_)));
    }

    #[tokio::test]
    async fn native_input_on_non_engine_tab_returns_platform_unsupported() {
        let err = native_input(
            "non-engine-tab-id",
            &BrowserNativeInputAction::Key { key: "Tab".into() },
        )
        .await
        .expect_err("non-engine tab must refuse trusted native input");
        assert!(matches!(err, BrowserError::PlatformUnsupported(_)));
    }
}
