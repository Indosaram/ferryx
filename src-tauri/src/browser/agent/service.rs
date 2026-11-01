//! The running app's automation authority: one registry for every ingress, persisted under
//! the data dir, plus the desktop (app chrome) commands that resolve approvals, revoke and
//! reclaim.
//!
//! Admission: a CLI request that carries `controllerAuth` is authenticated against the
//! registry and always goes through `authorize` + [`AuthorityState::enforce`]. A request with
//! the transport token only is refused with `BROWSER_CONTROLLER_PAIRING_REQUIRED` while
//! `requirePairing` is on (the default; the app's approval card is where a pairing is
//! allowed). Turning it off in Settings > Browser > Agent access restores token-only access.
//! Exceptions under the token alone: `open-file` (editor policy) and `remoteAttach`, the remote
//! gateway's own connection, which authorizes each framed command for its paired device
//! (moving it to a separate gateway token is plan T3).

use std::path::PathBuf;
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::sync::Notify;

use super::authority::{AuthorityState, ControllerSummary, PendingApproval, PersistedRegistry, TabFacts};
use super::catalog::{GrantScope, Platform, ToolId};
use super::policy::{authorize, classify_cli, Caller, Classification, Obligation, PolicyContext, Target};
use super::protocol::{ApprovalId, ApprovalResolution, AuthorityError, ControllerId, Principal, PAIRING_APPROVAL_TTL};
use crate::browser::BrowserManager;
use crate::ipc::browser_cli::{BrowserCliRequest, ControllerAuth};
use crate::ipc::error::{IpcError, IpcErrorCode};

/// Emitted to the app chrome whenever the pending approval list changes.
pub const BROWSER_AGENT_APPROVALS_EVENT: &str = "browser_agent_approvals_changed";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSettings {
    /// Refuse transport-token-only browser requests (no paired controller credential).
    #[serde(default = "require_pairing_default")]
    pub require_pairing: bool,
}

const fn require_pairing_default() -> bool {
    true
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self { require_pairing: require_pairing_default() }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedFile {
    #[serde(default)]
    settings: AgentSettings,
    #[serde(default)]
    registry: PersistedRegistry,
}

struct Service {
    state: AuthorityState,
    settings: AgentSettings,
    loaded: bool,
}

static SERVICE: LazyLock<parking_lot::Mutex<Service>> = LazyLock::new(|| {
    parking_lot::Mutex::new(Service { state: AuthorityState::default(), settings: initial_settings(), loaded: false })
});
/// Wakes requesters waiting on an approval outcome.
static RESOLVED: LazyLock<Arc<Notify>> = LazyLock::new(|| Arc::new(Notify::new()));

/// `<data>/browser/agent-authority.json`; `FERRYX_DATA_DIR` keeps tests and QA isolated.
#[cfg(not(test))]
fn store_path() -> Option<PathBuf> {
    crate::worktree::naming::data_dir_with_env(|key| std::env::var_os(key))
        .map(|dir| dir.join("browser").join("agent-authority.json"))
}

/// Unit tests never read or write the user's registry.
#[cfg(test)]
fn store_path() -> Option<PathBuf> {
    None
}

/// The registry's starting settings. Unit tests of other IPC paths send token-only requests
/// and share this process-wide registry, so they start with pairing off; the admission rule
/// itself is tested through [`token_only_admission`].
fn initial_settings() -> AgentSettings {
    if cfg!(test) {
        AgentSettings { require_pairing: false }
    } else {
        AgentSettings::default()
    }
}

async fn ensure_loaded() -> Result<(), IpcError> {
    if SERVICE.lock().loaded {
        return Ok(());
    }
    let file = crate::ipc::run_blocking(|| {
        let Some(path) = store_path() else { return Ok(PersistedFile::default()) };
        match std::fs::read(&path) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes).unwrap_or_else(|error| {
                tracing::warn!(%error, path = %path.display(), "unreadable browser agent registry; starting empty");
                PersistedFile::default()
            })),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(PersistedFile::default()),
            Err(error) => Err(IpcError::new(IpcErrorCode::IoError, error.to_string())),
        }
    })
    .await?;
    let mut service = SERVICE.lock();
    if !service.loaded {
        service.state = AuthorityState::from_persisted(file.registry);
        if store_path().is_some() {
            service.settings = file.settings;
        }
        service.loaded = true;
    }
    Ok(())
}

/// Writes the registry (digests only, never secrets) with an atomic replace, owner-only.
async fn persist() -> Result<(), IpcError> {
    let file = {
        let service = SERVICE.lock();
        PersistedFile { settings: service.settings.clone(), registry: service.state.to_persisted() }
    };
    crate::ipc::run_blocking(move || {
        let Some(path) = store_path() else { return Ok(()) };
        let io = |error: std::io::Error| IpcError::new(IpcErrorCode::IoError, error.to_string());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(io)?;
        }
        let bytes = serde_json::to_vec_pretty(&file).map_err(|error| IpcError::internal(error.to_string()))?;
        let temp = path.with_extension(format!("json.{}.tmp", uuid::Uuid::new_v4()));
        write_private(&temp, &bytes).map_err(io)?;
        std::fs::rename(&temp, &path).map_err(|error| {
            let _ = std::fs::remove_file(&temp);
            io(error)
        })
    })
    .await
}

#[cfg(unix)]
fn write_private(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// Windows: the data dir sits under the user's profile, whose ACL already limits it to the
/// current user.
#[cfg(not(unix))]
fn write_private(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn notify_approvals<R: tauri::Runtime>(app: &AppHandle<R>) {
    let pending = SERVICE.lock().state.pending_approvals(Instant::now());
    let _ = app.emit(BROWSER_AGENT_APPROVALS_EVENT, pending);
    RESOLVED.notify_waiters();
}

/// What a CLI request may do after admission.
pub enum Admission {
    /// Run the request; `Some` carries the authenticated controller (listings are filtered).
    Allowed(Option<ControllerId>),
    Refused(IpcError),
}

fn tab_url(manager: &BrowserManager, target: &Target) -> Option<(String, String)> {
    match target {
        Target::Browser(browser_id) => manager.get_state(browser_id).ok().map(|state| (browser_id.clone(), state.url)),
        Target::TabIndex(index) => manager
            .list_sessions()
            .into_iter()
            .nth(index.saturating_sub(1))
            .map(|session| (session.browser_id, session.url)),
        Target::Global | Target::Unspecified | Target::Download(_) => None,
    }
}

/// Admits one local CLI request (the transport token was already checked).
pub async fn admit_cli(
    manager: &BrowserManager,
    auth: Option<&ControllerAuth>,
    request: &BrowserCliRequest,
) -> Admission {
    if let Err(error) = ensure_loaded().await {
        return Admission::Refused(error);
    }
    let classification = match classify_cli(request) {
        Ok(classification) => classification,
        Err(error) => return Admission::Refused(error.into()),
    };
    let mut service = SERVICE.lock();
    let caller = match auth {
        Some(auth) => match service.state.authenticate(&auth.id, &auth.secret) {
            Ok(principal) => Caller::Principal(principal),
            Err(error) => return Admission::Refused(error.into()),
        },
        None => match token_only_admission(&service.settings, request) {
            Some(admission) => return admission,
            None => Caller::TransportOnly,
        },
    };
    match decide(&mut service.state, &caller, &classification, manager) {
        Ok(controller) => Admission::Allowed(controller),
        Err(error) => Admission::Refused(error.into()),
    }
}

/// Admission of a request that carries only the transport token: `Some` decides it outright,
/// `None` sends it through the policy as [`Caller::TransportOnly`] (which refuses browser tools
/// with `BROWSER_CONTROLLER_PAIRING_REQUIRED`).
fn token_only_admission(settings: &AgentSettings, request: &BrowserCliRequest) -> Option<Admission> {
    if !settings.require_pairing || matches!(request, BrowserCliRequest::RemoteAttach { .. }) {
        return Some(Admission::Allowed(None));
    }
    None
}

fn decide(
    state: &mut AuthorityState,
    caller: &Caller,
    classification: &Classification,
    manager: &BrowserManager,
) -> Result<Option<ControllerId>, AuthorityError> {
    let authorization = {
        let ctx = PolicyContext { controllers: &*state, platform: Platform::current() };
        authorize(caller, classification, &ctx)?
    };
    let Caller::Principal(principal) = caller else {
        return Ok(None);
    };
    let tab = tab_url(manager, &classification.target);
    let facts = tab.as_ref().map(|(browser_id, url)| TabFacts { browser_id, url });
    state.enforce(principal, &authorization, facts, Instant::now())?;
    Ok(match principal {
        Principal::LocalController { id, .. } if authorization.obligations.contains(&Obligation::FilterToGranted) => Some(*id),
        _ => None,
    })
}

/// Limits a listing to the tabs the controller may see.
pub fn filter_sessions(
    controller: Option<ControllerId>,
    sessions: Vec<crate::browser::BrowserSessionSummary>,
) -> Vec<crate::browser::BrowserSessionSummary> {
    let Some(controller) = controller else { return sessions };
    let service = SERVICE.lock();
    sessions
        .into_iter()
        .filter(|session| service.state.may_list(&controller, &session.browser_id, &session.url))
        .collect()
}

/// Starts a pairing request; the desktop user approves or denies it in the app.
pub async fn request_pairing<R: tauri::Runtime>(
    app: &AppHandle<R>,
    controller: &str,
    label: &str,
    secret: &str,
) -> Result<ApprovalId, IpcError> {
    ensure_loaded().await?;
    let controller = ControllerId::parse(controller)?;
    let approval = SERVICE.lock().state.request_pairing(controller, label, secret, Instant::now())?;
    notify_approvals(app);
    Ok(approval)
}

/// Starts an origin grant or one-shot capability request for an authenticated controller.
pub async fn request_grant<R: tauri::Runtime>(
    app: &AppHandle<R>,
    auth: &ControllerAuth,
    origin: Option<&str>,
    scopes: &[GrantScope],
    tool: Option<&str>,
) -> Result<ApprovalId, IpcError> {
    ensure_loaded().await?;
    let approval = {
        let mut service = SERVICE.lock();
        let Principal::LocalController { id, .. } = service.state.authenticate(&auth.id, &auth.secret)? else {
            return Err(AuthorityError::PairingRequired.into());
        };
        match (origin, tool) {
            (Some(origin), None) => service.state.request_origin_grant(id, origin, scopes, Instant::now())?,
            (None, Some(tool)) => service.state.request_capability(id, tool.parse::<ToolId>()?, Instant::now())?,
            _ => {
                return Err(IpcError::new(
                    IpcErrorCode::InvalidArgument,
                    "a grant request names either an origin or a tool",
                ))
            }
        }
    };
    notify_approvals(app);
    Ok(approval)
}

/// Waits (event driven, bounded by the approval lifetime) until the desktop user resolves the
/// request or it expires.
pub async fn await_resolution(approval: &str) -> Result<ApprovalResolution, IpcError> {
    ensure_loaded().await?;
    let approval = ApprovalId::parse(approval)
        .ok_or_else(|| IpcError::new(IpcErrorCode::InvalidArgument, "unknown approval id"))?;
    let deadline = tokio::time::Instant::now() + PAIRING_APPROVAL_TTL + Duration::from_secs(1);
    loop {
        // Subscribe before reading, so a resolution between the read and the wait is not lost.
        let notified = RESOLVED.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if let Some(resolution) = take_resolution(approval) {
            return Ok(resolution);
        }
        if tokio::time::timeout_at(deadline, notified).await.is_err() {
            // The request's own expiry has passed by now: report it.
            return Ok(take_resolution(approval).unwrap_or(ApprovalResolution::Expired));
        }
    }
}

/// Reads a resolved approval's outcome and forgets the request (one lock, never nested).
fn take_resolution(approval: ApprovalId) -> Option<ApprovalResolution> {
    let mut service = SERVICE.lock();
    let resolution = service.state.resolution(approval, Instant::now())?;
    service.state.forget_approval(approval);
    Some(resolution)
}

/// The calling controller's own record (no secrets).
pub async fn controller_summary(auth: &ControllerAuth) -> Result<Vec<ControllerSummary>, IpcError> {
    ensure_loaded().await?;
    let service = SERVICE.lock();
    let Principal::LocalController { id, .. } = service.state.authenticate(&auth.id, &auth.secret)? else {
        return Err(AuthorityError::PairingRequired.into());
    };
    Ok(service.state.controllers().into_iter().filter(|summary| summary.id == id).collect())
}

/// A controller revokes itself (`browser controllers revoke`).
pub async fn revoke_self<R: tauri::Runtime>(app: &AppHandle<R>, auth: &ControllerAuth) -> Result<(), IpcError> {
    ensure_loaded().await?;
    {
        let mut service = SERVICE.lock();
        let Principal::LocalController { id, .. } = service.state.authenticate(&auth.id, &auth.secret)? else {
            return Err(AuthorityError::PairingRequired.into());
        };
        service.state.revoke(&id);
    }
    persist().await?;
    notify_approvals(app);
    Ok(())
}

/// A desktop browser command takes the tab back from agents (implicit reclaim).
pub fn desktop_touched(browser_id: &str) {
    let mut service = SERVICE.lock();
    if service.state.lease_epoch(browser_id).is_some() {
        service.state.reclaim(browser_id);
    }
}

/// Forgets a closed tab's lease and reclaim mark.
pub fn tab_closed(browser_id: &str) {
    SERVICE.lock().state.release_tab(browser_id);
}

fn desktop_only<R: tauri::Runtime>(webview: &tauri::Webview<R>) -> Result<(), IpcError> {
    Principal::for_app_webview(webview.label()).map(|_| ()).map_err(IpcError::from)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOverview {
    pub settings: AgentSettings,
    pub pending: Vec<PendingApproval>,
    pub controllers: Vec<ControllerSummary>,
}

#[tauri::command]
pub async fn cmd_browser_agent_overview<R: tauri::Runtime>(webview: tauri::Webview<R>) -> Result<AgentOverview, IpcError> {
    desktop_only(&webview)?;
    ensure_loaded().await?;
    let mut service = SERVICE.lock();
    Ok(AgentOverview {
        settings: service.settings.clone(),
        pending: service.state.pending_approvals(Instant::now()),
        controllers: service.state.controllers(),
    })
}

#[tauri::command]
pub async fn cmd_browser_agent_resolve<R: tauri::Runtime>(
    app: AppHandle<R>,
    webview: tauri::Webview<R>,
    approval_id: String,
    resolution: ApprovalResolution,
) -> Result<(), IpcError> {
    desktop_only(&webview)?;
    ensure_loaded().await?;
    let approval = ApprovalId::parse(&approval_id)
        .ok_or_else(|| IpcError::new(IpcErrorCode::InvalidArgument, "unknown approval id"))?;
    SERVICE.lock().state.resolve(approval, resolution, Instant::now())?;
    persist().await?;
    notify_approvals(&app);
    Ok(())
}

#[tauri::command]
pub async fn cmd_browser_agent_revoke<R: tauri::Runtime>(
    app: AppHandle<R>,
    webview: tauri::Webview<R>,
    controller_id: String,
    origin: Option<String>,
) -> Result<bool, IpcError> {
    desktop_only(&webview)?;
    ensure_loaded().await?;
    let controller = ControllerId::parse(&controller_id)?;
    let changed = {
        let mut service = SERVICE.lock();
        match origin.as_deref() {
            Some(origin) => service.state.revoke_grant(&controller, origin),
            None => service.state.revoke(&controller),
        }
    };
    persist().await?;
    notify_approvals(&app);
    Ok(changed)
}

/// Take control (`reclaim: true`) or hand a tab back to agents (`false`).
#[tauri::command]
pub async fn cmd_browser_agent_reclaim<R: tauri::Runtime>(
    webview: tauri::Webview<R>,
    browser_id: String,
    reclaim: bool,
) -> Result<(), IpcError> {
    desktop_only(&webview)?;
    ensure_loaded().await?;
    let mut service = SERVICE.lock();
    if reclaim {
        service.state.reclaim(&browser_id);
    } else {
        service.state.resume(&browser_id);
    }
    Ok(())
}

#[tauri::command]
pub async fn cmd_browser_agent_set_settings<R: tauri::Runtime>(
    webview: tauri::Webview<R>,
    settings: AgentSettings,
) -> Result<(), IpcError> {
    desktop_only(&webview)?;
    ensure_loaded().await?;
    SERVICE.lock().settings = settings;
    persist().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairing_is_required_by_default_and_for_files_written_before_the_setting() {
        assert!(AgentSettings::default().require_pairing);
        let old: PersistedFile = serde_json::from_str("{}").expect("empty file");
        assert!(old.settings.require_pairing);
        let old: PersistedFile = serde_json::from_str(r#"{"settings":{}}"#).expect("no field");
        assert!(old.settings.require_pairing);
        let off: PersistedFile = serde_json::from_str(r#"{"settings":{"requirePairing":false}}"#).expect("off");
        assert!(!off.settings.require_pairing);
    }

    #[test]
    fn token_only_requests_go_to_the_policy_while_pairing_is_required() {
        let on = AgentSettings { require_pairing: true };
        assert!(token_only_admission(&on, &BrowserCliRequest::List).is_none());
        let attach = BrowserCliRequest::RemoteAttach { protocol_version: None };
        assert!(matches!(token_only_admission(&on, &attach), Some(Admission::Allowed(None))));
        let off = AgentSettings { require_pairing: false };
        assert!(matches!(token_only_admission(&off, &BrowserCliRequest::List), Some(Admission::Allowed(None))));
    }

    #[test]
    fn a_token_only_list_is_refused_with_pairing_required() {
        let classification = classify_cli(&BrowserCliRequest::List).expect("list classifies");
        let state = AuthorityState::default();
        let ctx = PolicyContext { controllers: &state, platform: Platform::current() };
        let error = authorize(&Caller::TransportOnly, &classification, &ctx).expect_err("refused");
        assert_eq!(error.code(), super::super::protocol::BROWSER_CONTROLLER_PAIRING_REQUIRED);
        assert!(error.to_string().contains("ferryx browser controllers pair"));
    }
}
