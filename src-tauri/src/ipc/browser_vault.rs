//! Tauri commands for the browser password vault (`crate::browser::vault`).
//!
//! The store opens lazily on first use, through `run_blocking` (Keychain, DPAPI, D-Bus and the
//! store file all block). Passwords cross IPC only for reveal/copy, after the OS confirms the
//! user (`reauth`); listing, matching and prompts never carry them. Fill sends the secret
//! straight to the tab's isolated world (system webview) or the engine's vault world (Chromium).
//!
//! Opening can wait on an OS keystore prompt for as long as the user takes, so it runs under
//! its own `opening` lock (taken only on blocking threads) and never under the store, prompts
//! or backend locks. Closing a tab (`forget_browser`) and the status error path therefore never
//! wait on a keystore. Lock order: `opening`, then `store`, then `prompts`; `backend` is a leaf.

use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};
use std::time::Instant;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::browser::vault::generator::{generate_password, PasswordGeneratorOptions};
use crate::browser::vault::{
    self, fill_for_page, keystore, PendingPrompts, VaultError, VaultItemSummary, VaultSavePrompt,
    VaultStore, BROWSER_VAULT_SAVE_PROMPT_EVENT,
};
use crate::browser::{BrowserEngine, BrowserError, BrowserManager};
use crate::ipc::error::{IpcError, IpcErrorCode};

pub const VAULT_UNAVAILABLE: &str = "BROWSER_VAULT_UNAVAILABLE";
pub const VAULT_CORRUPTED: &str = "BROWSER_VAULT_CORRUPTED";
pub const VAULT_NOT_FOUND: &str = "BROWSER_VAULT_NOT_FOUND";
pub const VAULT_PROMPT_EXPIRED: &str = "BROWSER_VAULT_PROMPT_EXPIRED";
pub const VAULT_REAUTH_FAILED: &str = "BROWSER_VAULT_REAUTH_FAILED";

struct Vault {
    store: Mutex<Option<VaultStore>>,
    backend: Mutex<&'static str>,
    /// Serializes opens; held across the keystore call, so only ever taken on blocking threads.
    opening: Mutex<()>,
    prompts: Mutex<PendingPrompts>,
}

impl Vault {
    fn new() -> Self {
        Self {
            store: Mutex::new(None),
            backend: Mutex::new("unsupported"),
            opening: Mutex::new(()),
            prompts: Mutex::new(PendingPrompts::default()),
        }
    }

    /// Opens the store if it is not open yet. Blocking: may wait on an OS keystore prompt, but
    /// without holding the store, prompts or backend locks.
    fn ensure_open(&self, dir: &Path, keystore: &dyn keystore::KeyStore) -> Result<(), VaultError> {
        if self.store.lock().is_some() {
            return Ok(());
        }
        let _opening = self.opening.lock();
        if self.store.lock().is_some() {
            return Ok(());
        }
        *self.backend.lock() = keystore.backend();
        let opened = VaultStore::open(dir, keystore)?;
        *self.store.lock() = Some(opened);
        Ok(())
    }

    /// Runs `f` on the open store and the pending prompts (opening the store first). Blocking.
    fn with_open<T>(
        &self,
        dir: &Path,
        keystore: &dyn keystore::KeyStore,
        f: impl FnOnce(&mut VaultStore, &mut PendingPrompts) -> Result<T, VaultError>,
    ) -> Result<T, VaultError> {
        self.ensure_open(dir, keystore)?;
        let mut store = self.store.lock();
        let store = store.as_mut().ok_or_else(not_open)?;
        let mut prompts = self.prompts.lock();
        f(store, &mut prompts)
    }

    fn backend(&self) -> &'static str {
        *self.backend.lock()
    }

    fn forget_browser(&self, browser_id: &str) {
        self.prompts.lock().forget_browser(browser_id);
    }
}

fn not_open() -> VaultError {
    VaultError::Unavailable("the password vault is not open".into())
}

static VAULT: LazyLock<Arc<Vault>> = LazyLock::new(|| Arc::new(Vault::new()));

pub(crate) fn vault_error(error: VaultError) -> IpcError {
    let message = error.to_string();
    let code = match error {
        VaultError::Unavailable(_) | VaultError::Keystore(_) => IpcErrorCode::Custom(VAULT_UNAVAILABLE.into()),
        VaultError::Corrupted(_) => IpcErrorCode::Custom(VAULT_CORRUPTED.into()),
        VaultError::NotFound(_) => IpcErrorCode::Custom(VAULT_NOT_FOUND.into()),
        VaultError::PromptExpired => IpcErrorCode::Custom(VAULT_PROMPT_EXPIRED.into()),
        VaultError::InvalidArgument(_) => IpcErrorCode::InvalidArgument,
        VaultError::Io(_) => IpcErrorCode::IoError,
    };
    IpcError::new(code, message)
}

fn vault_dir<R: tauri::Runtime>(app: &AppHandle<R>) -> Result<PathBuf, IpcError> {
    let base = crate::browser::data_paths::app_data_root(app)
        .map_err(|error| IpcError::new(IpcErrorCode::IoError, error))?;
    Ok(base.join(vault::VAULT_DIR))
}

/// Runs `f` on the open store (opening it first) on a blocking thread.
async fn with_store<R, T, F>(app: &AppHandle<R>, f: F) -> Result<T, IpcError>
where
    R: tauri::Runtime,
    T: Send + 'static,
    F: FnOnce(&mut VaultStore, &mut PendingPrompts) -> Result<T, VaultError> + Send + 'static,
{
    let dir = vault_dir(app)?;
    let vault = Arc::clone(&VAULT);
    crate::ipc::run_blocking(move || {
        let keystore = keystore::platform(&dir);
        vault.with_open(&dir, keystore.as_ref(), f).map_err(vault_error)
    })
    .await
}

/// The app's shared vault for the browser import, which saves logins on its own blocking thread.
pub(crate) struct AppVault {
    dir: PathBuf,
}

pub(crate) fn app_vault<R: tauri::Runtime>(app: &AppHandle<R>) -> Result<AppVault, IpcError> {
    Ok(AppVault { dir: vault_dir(app)? })
}

impl crate::browser::import::ingest::PasswordVault for AppVault {
    fn with_vault(
        &self,
        f: &mut dyn FnMut(&mut VaultStore) -> Result<(), VaultError>,
    ) -> Result<(), VaultError> {
        let keystore = keystore::platform(&self.dir);
        VAULT.ensure_open(&self.dir, keystore.as_ref())?;
        let mut store = VAULT.store.lock();
        f(store.as_mut().ok_or_else(not_open)?)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultStatus {
    pub available: bool,
    pub backend: String,
    pub error: Option<String>,
    pub error_code: Option<String>,
}

#[tauri::command]
pub async fn cmd_browser_vault_status<R: tauri::Runtime>(app: AppHandle<R>) -> Result<VaultStatus, IpcError> {
    match with_store(&app, |_, _| Ok(VAULT.backend())).await {
        Ok(backend) => Ok(VaultStatus { available: true, backend: backend.into(), error: None, error_code: None }),
        Err(error) => Ok(VaultStatus {
            available: false,
            backend: VAULT.backend().into(),
            error: Some(error.message.clone()),
            error_code: Some(error.code.to_string()),
        }),
    }
}

#[tauri::command]
pub async fn cmd_browser_vault_list<R: tauri::Runtime>(
    app: AppHandle<R>,
    query: Option<String>,
) -> Result<Vec<VaultItemSummary>, IpcError> {
    with_store(&app, move |store, _| Ok(store.list(query.as_deref()))).await
}

/// The password, after the OS confirmed the user (Touch ID / password on macOS, Windows Hello
/// or the account PIN on Windows, polkit on Linux when available).
#[tauri::command]
pub async fn cmd_browser_vault_reveal<R: tauri::Runtime>(app: AppHandle<R>, id: String) -> Result<String, IpcError> {
    crate::ipc::run_blocking(|| {
        crate::browser::vault::reauth::confirm_user("reveal a saved password")
            .map_err(|reason| IpcError::new(IpcErrorCode::Custom(VAULT_REAUTH_FAILED.into()), reason))
    })
    .await?;
    with_store(&app, move |store, _| Ok(store.credential(&id)?.password.to_string())).await
}

#[tauri::command]
pub async fn cmd_browser_vault_update<R: tauri::Runtime>(
    app: AppHandle<R>,
    id: String,
    username: String,
    password: Option<String>,
) -> Result<VaultItemSummary, IpcError> {
    let password = password.map(zeroize::Zeroizing::new);
    with_store(&app, move |store, _| store.update(&id, &username, password.as_deref().map(String::as_str))).await
}

#[tauri::command]
pub async fn cmd_browser_vault_delete<R: tauri::Runtime>(app: AppHandle<R>, id: String) -> Result<(), IpcError> {
    with_store(&app, move |store, _| store.delete(&id)).await
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedPassword {
    pub password: String,
    pub score: u8,
    pub entropy_bits: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratorRequest {
    pub mode: Option<String>,
    pub length: Option<u32>,
    pub include_lowercase: Option<bool>,
    pub include_uppercase: Option<bool>,
    pub include_digits: Option<bool>,
    pub include_symbols: Option<bool>,
    pub avoid_ambiguous: Option<bool>,
    pub word_count: Option<u32>,
    pub separator: Option<String>,
    pub capitalize: Option<bool>,
    pub include_number: Option<bool>,
}

pub fn generator_options(request: GeneratorRequest) -> PasswordGeneratorOptions {
    use crate::browser::vault::generator::PasswordGeneratorMode;
    let defaults = PasswordGeneratorOptions::default();
    PasswordGeneratorOptions {
        mode: match request.mode.as_deref() {
            Some("passphrase") => PasswordGeneratorMode::Passphrase,
            _ => PasswordGeneratorMode::Password,
        },
        length: request.length.unwrap_or(defaults.length),
        include_lowercase: request.include_lowercase.unwrap_or(defaults.include_lowercase),
        include_uppercase: request.include_uppercase.unwrap_or(defaults.include_uppercase),
        include_digits: request.include_digits.unwrap_or(defaults.include_digits),
        include_symbols: request.include_symbols.unwrap_or(defaults.include_symbols),
        avoid_ambiguous: request.avoid_ambiguous.unwrap_or(defaults.avoid_ambiguous),
        word_count: request.word_count.unwrap_or(defaults.word_count),
        separator: request.separator.unwrap_or(defaults.separator),
        capitalize: request.capitalize.unwrap_or(defaults.capitalize),
        include_number: request.include_number.unwrap_or(defaults.include_number),
    }
}

#[tauri::command]
pub async fn cmd_browser_vault_generate(options: Option<GeneratorRequest>) -> Result<GeneratedPassword, IpcError> {
    let generated = generate_password(&generator_options(options.unwrap_or_default()))
        .map_err(|error| IpcError::new(IpcErrorCode::InvalidArgument, error.to_string()))?;
    Ok(GeneratedPassword {
        password: generated.password.unwrap_or_default(),
        score: generated.strength_score,
        entropy_bits: generated.entropy_bits,
    })
}

/// Saved logins for exactly the origin the tab shows. A private tab gets none.
#[tauri::command]
pub async fn cmd_browser_vault_matches<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
) -> Result<Vec<VaultItemSummary>, IpcError> {
    let state = manager.get_state(&browser_id)?;
    if state.profile_id.is_private() {
        return Ok(Vec::new());
    }
    let url = state.url;
    with_store(&app, move |store, _| Ok(store.matches(&url))).await
}

#[tauri::command]
pub async fn cmd_browser_vault_fill<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
    id: String,
) -> Result<(), IpcError> {
    let state = manager.get_state(&browser_id)?;
    let url = state.url.clone();
    // The tab's current origin, checked here and again inside the page.
    let script = with_store(&app, move |store, _| fill_for_page(&store.credential(&id)?, &url)).await?;
    match state.engine {
        BrowserEngine::Chromium => fill_chromium(&browser_id, script.to_string()),
        BrowserEngine::System => {
            if state.trusted_bridge != crate::browser::TrustedBridgeKind::Isolated {
                return Err(IpcError::new(
                    IpcErrorCode::UnsupportedCapability,
                    "this tab has no isolated script world, so a password cannot be filled safely",
                ));
            }
            let webview = app
                .get_webview(&state.webview_label)
                .ok_or_else(|| BrowserError::WebviewNotFound(state.webview_label.clone()))?;
            crate::browser::bridge::eval_in_world(webview, script.to_string()).await?;
            Ok(())
        }
    }
}

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
fn fill_chromium(browser_id: &str, script: String) -> Result<(), IpcError> {
    Ok(crate::browser::cef_engine::fill(browser_id, script)?)
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn fill_chromium(_browser_id: &str, _script: String) -> Result<(), IpcError> {
    Err(IpcError::new(IpcErrorCode::UnsupportedCapability, "the Chromium engine is not available here"))
}

#[tauri::command]
pub async fn cmd_browser_vault_save_pending<R: tauri::Runtime>(
    app: AppHandle<R>,
    prompt_id: String,
) -> Result<VaultItemSummary, IpcError> {
    let id = prompt_id.clone();
    let saved = with_store(&app, move |store, prompts| prompts.accept(store, &prompt_id, Instant::now())).await;
    match &saved {
        Ok(item) => tracing::info!(prompt_id = %id, item_id = %item.id, "saved a password from a save prompt"),
        Err(error) => tracing::warn!(prompt_id = %id, code = %error.code, message = %error.message, "could not save a password from a save prompt"),
    }
    saved
}

#[tauri::command]
pub async fn cmd_browser_vault_dismiss_pending<R: tauri::Runtime>(
    app: AppHandle<R>,
    prompt_id: String,
) -> Result<(), IpcError> {
    with_store(&app, move |_, prompts| prompts.dismiss(&prompt_id, Instant::now())).await
}

/// A login form was submitted in `browser_id` (either engine): bind it to the tab's origin and
/// ask the user whether to save it. Private tabs never save. Runs the store on a blocking thread.
pub(crate) fn handle_form_submitted<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &BrowserManager,
    browser_id: &str,
    raw: &str,
) {
    let Ok(state) = manager.get_state(browser_id) else { return };
    if state.profile_id.is_private() {
        return;
    }
    let Some(login) = crate::browser::vault::parse_submission(raw, &state.url) else {
        tracing::debug!(browser_id = %browser_id, "dropped a login submission that does not match the tab's origin");
        return;
    };
    let app = app.clone();
    let browser_id = browser_id.to_string();
    tauri::async_runtime::spawn(async move {
        let id = browser_id.clone();
        let offered: Result<Option<VaultSavePrompt>, IpcError> =
            with_store(&app, move |store, prompts| prompts.offer(store, &id, login, Instant::now())).await;
        match offered {
            Ok(Some(prompt)) => {
                tracing::info!(browser_id = %browser_id, prompt_id = %prompt.prompt_id, update = prompt.update, "offered to save a password");
                let _ = app.emit(BROWSER_VAULT_SAVE_PROMPT_EVENT, prompt);
            }
            Ok(None) => {}
            Err(error) => tracing::warn!(browser_id = %browser_id, code = %error.code, "could not offer to save a password"),
        }
    });
}

/// Drops a closed tab's open save prompt. Never waits on a keystore open.
pub(crate) fn forget_browser(browser_id: &str) {
    VAULT.forget_browser(browser_id);
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use crate::browser::vault::keystore::{KeyStore, KeystoreError};

    use super::*;

    #[test]
    fn vault_errors_map_to_stable_codes() {
        let code = |error| vault_error(error).code.to_string();
        assert_eq!(code(VaultError::Unavailable("x".into())), VAULT_UNAVAILABLE);
        assert_eq!(code(VaultError::Keystore("x".into())), VAULT_UNAVAILABLE);
        assert_eq!(code(VaultError::Corrupted("x".into())), VAULT_CORRUPTED);
        assert_eq!(code(VaultError::NotFound("x".into())), VAULT_NOT_FOUND);
        assert_eq!(code(VaultError::PromptExpired), VAULT_PROMPT_EXPIRED);
        assert_eq!(code(VaultError::InvalidArgument("x".into())), "INVALID_ARGUMENT");
    }

    #[test]
    fn generator_request_defaults_and_overrides() {
        let defaults = generator_options(GeneratorRequest::default());
        assert_eq!(defaults, PasswordGeneratorOptions::default());
        let phrase = generator_options(GeneratorRequest { mode: Some("passphrase".into()), word_count: Some(4), ..Default::default() });
        assert_eq!(phrase.word_count, 4);
        assert_eq!(phrase.mode, crate::browser::vault::generator::PasswordGeneratorMode::Passphrase);
        let request: GeneratorRequest = serde_json::from_str(r#"{"length":32,"includeSymbols":false}"#).unwrap();
        let options = generator_options(request);
        assert_eq!((options.length, options.include_symbols), (32, false));
    }

    /// A keystore whose first call announces itself on `entered` and then waits for `release`.
    struct BlockingKeyStore {
        entered: Mutex<mpsc::Sender<()>>,
        release: Mutex<mpsc::Receiver<()>>,
    }

    impl BlockingKeyStore {
        fn block(&self) -> Result<(), KeystoreError> {
            let _ = self.entered.lock().send(());
            self.release
                .lock()
                .recv_timeout(Duration::from_secs(10))
                .map_err(|error| KeystoreError::Failed(error.to_string()))
        }
    }

    impl KeyStore for BlockingKeyStore {
        fn backend(&self) -> &'static str {
            "blocking"
        }
        fn load(&self, _account: &str) -> Result<Option<Vec<u8>>, KeystoreError> {
            self.block()?;
            Ok(None)
        }
        fn store(&self, _account: &str, _secret: &[u8]) -> Result<(), KeystoreError> {
            self.block()
        }
        fn delete(&self, _account: &str) -> Result<(), KeystoreError> {
            Ok(())
        }
    }

    #[test]
    fn a_blocked_keystore_open_does_not_block_tab_close_or_status() {
        let dir = tempfile::tempdir().unwrap();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let keystore = BlockingKeyStore { entered: Mutex::new(entered_tx), release: Mutex::new(release_rx) };
        let vault = Arc::new(Vault::new());

        let opener = {
            let vault = Arc::clone(&vault);
            let path = dir.path().to_path_buf();
            std::thread::spawn(move || vault.ensure_open(&path, &keystore))
        };
        entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();

        // The open is in progress: closing a tab and reading the backend must still finish.
        let (done_tx, done_rx) = mpsc::channel();
        {
            let vault = Arc::clone(&vault);
            std::thread::spawn(move || {
                vault.forget_browser("b1");
                let backend = vault.backend();
                let store_open = vault.store.lock().is_some();
                let _ = done_tx.send((backend, store_open));
            });
        }
        assert_eq!(done_rx.recv_timeout(Duration::from_secs(5)).unwrap(), ("blocking", false));

        release_tx.send(()).unwrap();
        opener.join().unwrap().unwrap();
        assert!(vault.store.lock().is_some());
    }
}
