//! Tauri commands for the browser password vault (`crate::browser::vault`).
//!
//! The store opens lazily on first use, through `run_blocking` (Keychain, DPAPI, D-Bus and the
//! store file all block). Passwords cross IPC only for reveal/copy, after the OS confirms the
//! user (`reauth`); listing, matching and prompts never carry them. Fill sends the secret
//! straight to the tab's isolated world (system webview) or the engine's vault world (Chromium).

use std::path::PathBuf;
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

struct VaultState {
    store: Option<VaultStore>,
    backend: &'static str,
    prompts: PendingPrompts,
}

static VAULT: LazyLock<Arc<Mutex<VaultState>>> = LazyLock::new(|| {
    Arc::new(Mutex::new(VaultState { store: None, backend: "unsupported", prompts: PendingPrompts::default() }))
});

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
    let base = match std::env::var_os("FERRYX_DATA_DIR").filter(|dir| !dir.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => app
            .path()
            .app_data_dir()
            .map_err(|error| IpcError::new(IpcErrorCode::IoError, error.to_string()))?,
    };
    Ok(base.join(vault::VAULT_DIR))
}

/// Runs `f` on the open store (opening it first) on a blocking thread.
async fn with_store<R, T, F>(app: &AppHandle<R>, f: F) -> Result<T, IpcError>
where
    R: tauri::Runtime,
    T: Send + 'static,
    F: FnOnce(&mut VaultState) -> Result<T, VaultError> + Send + 'static,
{
    let dir = vault_dir(app)?;
    let state = Arc::clone(&VAULT);
    crate::ipc::run_blocking(move || {
        let mut state = state.lock();
        if state.store.is_none() {
            let keystore = keystore::platform(&dir);
            state.backend = keystore.backend();
            state.store = Some(VaultStore::open(&dir, keystore.as_ref()).map_err(vault_error)?);
        }
        f(&mut state).map_err(vault_error)
    })
    .await
}

fn store(state: &mut VaultState) -> &mut VaultStore {
    state.store.as_mut().expect("with_store opened the store")
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
    match with_store(&app, |state| Ok(state.backend)).await {
        Ok(backend) => Ok(VaultStatus { available: true, backend: backend.into(), error: None, error_code: None }),
        Err(error) => Ok(VaultStatus {
            available: false,
            backend: VAULT.lock().backend.into(),
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
    with_store(&app, move |state| Ok(store(state).list(query.as_deref()))).await
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
    with_store(&app, move |state| Ok(store(state).credential(&id)?.password.to_string())).await
}

#[tauri::command]
pub async fn cmd_browser_vault_update<R: tauri::Runtime>(
    app: AppHandle<R>,
    id: String,
    username: String,
    password: Option<String>,
) -> Result<VaultItemSummary, IpcError> {
    let password = password.map(zeroize::Zeroizing::new);
    with_store(&app, move |state| store(state).update(&id, &username, password.as_deref().map(String::as_str))).await
}

#[tauri::command]
pub async fn cmd_browser_vault_delete<R: tauri::Runtime>(app: AppHandle<R>, id: String) -> Result<(), IpcError> {
    with_store(&app, move |state| store(state).delete(&id)).await
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
    with_store(&app, move |vault| Ok(store(vault).matches(&url))).await
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
    let script = with_store(&app, move |vault| fill_for_page(&store(vault).credential(&id)?, &url)).await?;
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
    with_store(&app, move |state| {
        let VaultState { store, prompts, .. } = state;
        let store = store.as_mut().expect("with_store opened the store");
        prompts.accept(store, &prompt_id, Instant::now())
    })
    .await
}

#[tauri::command]
pub async fn cmd_browser_vault_dismiss_pending<R: tauri::Runtime>(
    app: AppHandle<R>,
    prompt_id: String,
) -> Result<(), IpcError> {
    with_store(&app, move |state| state.prompts.dismiss(&prompt_id, Instant::now())).await
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
        let offered: Result<Option<VaultSavePrompt>, IpcError> = with_store(&app, move |vault| {
            let VaultState { store, prompts, .. } = vault;
            let store = store.as_ref().expect("with_store opened the store");
            prompts.offer(store, &id, login, Instant::now())
        })
        .await;
        match offered {
            Ok(Some(prompt)) => {
                let _ = app.emit(BROWSER_VAULT_SAVE_PROMPT_EVENT, prompt);
            }
            Ok(None) => {}
            Err(error) => tracing::warn!(browser_id = %browser_id, code = %error.code, "could not offer to save a password"),
        }
    });
}

/// Drops a closed tab's open save prompt.
pub(crate) fn forget_browser(browser_id: &str) {
    VAULT.lock().prompts.forget_browser(browser_id);
}

#[cfg(test)]
mod tests {
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
}
