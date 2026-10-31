use crate::browser::{
    BrowserAutomationRequest, BrowserAutomationSnapshot, BrowserConsoleEntry, BrowserCookieEntry,
    BrowserDialogHandleRequest, BrowserDownloadRequest, BrowserError, BrowserManager,
    BrowserSessionCreatedPayload, BrowserSessionSummary, BrowserWaitCondition,
    CreateBrowserRequest,
};
#[cfg(feature = "desktop")]
use crate::ipc::browser::{
    browser_automation_act, browser_automation_snapshot, browser_download_with_session,
    browser_get_dialogs, browser_handle_dialog, browser_set_dialog_policy, close_browser_session,
    create_browser_session, focus_browser_session, highlight_browser_session,
    highlight_browser_session_with_duration, identify_browser_session, navigate_browser_session,
    navigate_browser_session_with_context,
};
use crate::ipc::error::{IpcError, IpcErrorCode};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;
#[cfg(feature = "desktop")]
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::BufReader;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "camelCase")]
pub enum BrowserCliRequest {
    List,
    Snapshot {
        browser_id: String,
    },
    Act {
        request: BrowserAutomationRequest,
    },
    #[serde(rename_all = "camelCase")]
    OpenFile {
        path: String,
        #[serde(default)]
        line: Option<u32>,
        #[serde(default)]
        col: Option<u32>,
    },
    #[serde(rename_all = "camelCase")]
    Open {
        url: String,
        #[serde(default, alias = "workspace_id")]
        workspace_id: Option<String>,
        #[serde(default, alias = "worktree_path")]
        worktree_path: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Navigate {
        #[serde(alias = "browser_id")]
        browser_id: String,
        url: String,
    },
    #[serde(rename_all = "camelCase")]
    Back {
        #[serde(alias = "browser_id")]
        browser_id: String,
    },
    #[serde(rename_all = "camelCase")]
    Forward {
        #[serde(alias = "browser_id")]
        browser_id: String,
    },
    #[serde(rename_all = "camelCase")]
    Reload {
        #[serde(alias = "browser_id")]
        browser_id: String,
    },
    #[serde(rename_all = "camelCase")]
    Close {
        #[serde(alias = "browser_id")]
        browser_id: String,
    },
    Identify,
    #[serde(rename_all = "camelCase")]
    Eval {
        #[serde(alias = "browser_id")]
        browser_id: String,
        script: String,
    },
    #[serde(rename_all = "camelCase")]
    Wait {
        #[serde(alias = "browser_id")]
        browser_id: String,
        condition: BrowserWaitCondition,
    },
    #[serde(rename_all = "camelCase")]
    Console {
        #[serde(alias = "browser_id")]
        browser_id: String,
        #[serde(default, alias = "errors_only")]
        errors_only: Option<bool>,
        #[serde(default)]
        clear: Option<bool>,
    },
    #[serde(rename_all = "camelCase")]
    Focus {
        #[serde(alias = "browser_id")]
        browser_id: String,
    },
    #[serde(rename_all = "camelCase")]
    Screenshot {
        #[serde(alias = "browser_id")]
        browser_id: String,
        #[serde(alias = "out_path")]
        out_path: String,
    },
    #[serde(rename_all = "camelCase")]
    Cookies {
        #[serde(alias = "browser_id")]
        browser_id: String,
        action: String,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        value: Option<String>,
        #[serde(default)]
        domain: Option<String>,
        #[serde(default)]
        path: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Storage {
        #[serde(alias = "browser_id")]
        browser_id: String,
        kind: String,
        action: String,
        #[serde(default)]
        key: Option<String>,
        #[serde(default)]
        value: Option<String>,
        #[serde(default, alias = "clear_all")]
        clear_all: Option<bool>,
        #[serde(default)]
        url: Option<String>,
        #[serde(default)]
        domain: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    TabList,
    #[serde(rename_all = "camelCase")]
    TabNew {
        #[serde(default)]
        url: Option<String>,
        #[serde(default, alias = "workspace_id")]
        workspace_id: Option<String>,
        #[serde(default, alias = "worktree_path")]
        worktree_path: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    TabSwitch {
        #[serde(default, alias = "browser_id")]
        browser_id: Option<String>,
        #[serde(default)]
        index: Option<usize>,
    },
    #[serde(rename_all = "camelCase")]
    TabClose {
        #[serde(default, alias = "browser_id")]
        browser_id: Option<String>,
        #[serde(default)]
        index: Option<usize>,
    },
    #[serde(rename_all = "camelCase")]
    StateSave {
        #[serde(alias = "browser_id")]
        browser_id: String,
        #[serde(alias = "out_path")]
        out_path: String,
    },
    #[serde(rename_all = "camelCase")]
    StateLoad {
        #[serde(alias = "browser_id")]
        browser_id: String,
        #[serde(alias = "in_path")]
        in_path: String,
    },
    #[serde(rename_all = "camelCase")]
    Dialog {
        #[serde(alias = "browser_id")]
        browser_id: String,
        action: String,
        #[serde(default, alias = "prompt_text")]
        prompt_text: Option<String>,
        #[serde(default)]
        policy: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Download {
        #[serde(default, alias = "browser_id")]
        browser_id: Option<String>,
        #[serde(default)]
        url: Option<String>,
        #[serde(default)]
        path: Option<String>,
        #[serde(default)]
        id: Option<String>,
        action: String,
    },
    #[serde(rename_all = "camelCase")]
    Highlight {
        #[serde(alias = "browser_id")]
        browser_id: String,
        selector: String,
        #[serde(default, alias = "duration_ms")]
        duration_ms: Option<u64>,
    },
    #[serde(rename_all = "camelCase")]
    RemoteAttach {
        #[serde(default)]
        protocol_version: Option<u32>,
    },
}

/// Custom serde deserializer that cleanly converts between a u64 integer or decimal string
/// without falling back to legacy modes or non-numeric types.
pub fn deserialize_u64_or_decimal_string<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct U64OrDecimalStringVisitor;

    impl<'de> serde::de::Visitor<'de> for U64OrDecimalStringVisitor {
        type Value = Option<u64>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a u64 integer or a decimal string")
        }

        fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(Some(v))
        }

        fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            if v >= 0 {
                Ok(Some(v as u64))
            } else {
                Err(E::custom("expected non-negative integer for u64"))
            }
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            let trimmed = v.trim();
            if trimmed.is_empty() {
                return Ok(None);
            }
            trimmed
                .parse::<u64>()
                .map(Some)
                .map_err(|_| E::custom(format!("invalid decimal string for u64: '{v}'")))
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(None)
        }

        fn visit_some<D2>(self, deserializer: D2) -> Result<Self::Value, D2::Error>
        where
            D2: serde::Deserializer<'de>,
        {
            deserializer.deserialize_any(self)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(None)
        }
    }

    deserializer.deserialize_option(U64OrDecimalStringVisitor)
}

fn never_grant_client_approval<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let _ = bool::deserialize(deserializer);
    Ok(false)
}

/// Closed remote browser operations permitted over framed remote transport.
///
/// Disallows legacy CLI commands (List, Open, Close, Focus, Screenshot, Cookies, Storage, Act, Snapshot, RemoteAttach)
/// to maintain strict boundary isolation between local CLI driver and remote control sessions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "camelCase")]
pub enum RemoteBrowserOperation {
    #[serde(rename_all = "camelCase")]
    Navigate { browser_id: String, url: String },
    #[serde(rename_all = "camelCase")]
    Back { browser_id: String },
    #[serde(rename_all = "camelCase")]
    Forward { browser_id: String },
    #[serde(rename_all = "camelCase")]
    Reload { browser_id: String },
    #[serde(rename_all = "camelCase")]
    Click {
        browser_id: String,
        #[serde(default)]
        reference: Option<String>,
        #[serde(default)]
        snapshot_id: Option<String>,
        #[serde(default, deserialize_with = "deserialize_u64_or_decimal_string")]
        map_revision: Option<u64>,
        #[serde(default)]
        u: Option<f64>,
        #[serde(default)]
        v: Option<f64>,
        #[serde(default)]
        stream_id: Option<u32>,
        #[serde(default)]
        sequence_number: Option<u32>,
        #[serde(default, deserialize_with = "deserialize_u64_or_decimal_string")]
        document_generation: Option<u64>,
        #[serde(default, deserialize_with = "deserialize_u64_or_decimal_string")]
        viewport_revision: Option<u64>,
        #[serde(default)]
        capture_rect: Option<crate::browser::model::LogicalRect>,
        #[serde(default)]
        geometry_source: Option<String>,
        #[serde(default)]
        x: Option<f64>,
        #[serde(default)]
        y: Option<f64>,
    },
    #[serde(rename_all = "camelCase")]
    Fill {
        browser_id: String,
        reference: String,
        value: String,
        #[serde(default)]
        snapshot_id: Option<String>,
        #[serde(default, deserialize_with = "deserialize_u64_or_decimal_string")]
        map_revision: Option<u64>,
    },
    #[serde(rename_all = "camelCase")]
    Keypress { browser_id: String, key: String },
    #[serde(rename_all = "camelCase")]
    Wait {
        browser_id: String,
        condition: BrowserWaitCondition,
        #[serde(default, alias = "timeout_ms")]
        timeout_ms: Option<u64>,
        #[serde(default, deserialize_with = "never_grant_client_approval")]
        has_approval: bool,
    },
    #[serde(rename_all = "camelCase")]
    Eval {
        browser_id: String,
        script: String,
        #[serde(default, deserialize_with = "never_grant_client_approval")]
        has_approval: bool,
    },
    #[serde(rename_all = "camelCase")]
    GetState { browser_id: String },
    #[serde(rename_all = "camelCase")]
    Snapshot { browser_id: String },
    #[serde(rename_all = "camelCase")]
    List {
        #[serde(default)]
        workspace_id: Option<String>,
        #[serde(default)]
        worktree_slug: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Execute {
        browser_id: String,
        command: String,
        #[serde(default)]
        params: Option<serde_json::Value>,
        #[serde(default)]
        document_generation: Option<String>,
        #[serde(default)]
        browser_instance_id: Option<String>,
        #[serde(default)]
        desktop_epoch: Option<String>,
        #[serde(default)]
        lease_epoch: Option<String>,
        #[serde(default)]
        device_id: Option<String>,
        #[serde(default)]
        connection_id: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    SubscribeViewer {
        browser_id: String,
        device_id: String,
        viewer_instance_id: String,
        #[serde(default)]
        options: Option<crate::remote::browser_protocol::BrowserSubscribeOptions>,
    },
    #[serde(rename_all = "camelCase")]
    UnsubscribeViewer {
        browser_id: String,
        subscription_id: String,
    },
    #[serde(rename_all = "camelCase")]
    ClaimDriver {
        browser_id: String,
        device_id: String,
        connection_id: String,
        subscription_id: String,
        #[serde(default, deserialize_with = "deserialize_u64_or_decimal_string")]
        lease_epoch: Option<u64>,
    },
    #[serde(rename_all = "camelCase")]
    ReleaseDriver {
        subscription_id: String,
        #[serde(default, deserialize_with = "deserialize_u64_or_decimal_string")]
        lease_epoch: Option<u64>,
    },
    #[serde(rename_all = "camelCase")]
    HeartbeatDriver {
        device_id: String,
        connection_id: String,
        subscription_id: String,
        #[serde(default, deserialize_with = "deserialize_u64_or_decimal_string")]
        lease_epoch: Option<u64>,
    },
}

impl RemoteBrowserOperation {
    pub fn validate(&self) -> Result<(), IpcError> {
        match self {
            Self::List { .. } => Ok(()),
            Self::Execute {
                browser_id,
                command,
                ..
            } => {
                if browser_id.trim().is_empty() {
                    return Err(IpcError::new(
                        IpcErrorCode::InvalidArgument,
                        "browserId required",
                    ));
                }
                if command.trim().is_empty() {
                    return Err(IpcError::new(
                        IpcErrorCode::InvalidArgument,
                        "command required",
                    ));
                }
                Ok(())
            }
            Self::SubscribeViewer { browser_id, .. } => {
                if browser_id.trim().is_empty() {
                    return Err(IpcError::new(
                        IpcErrorCode::InvalidArgument,
                        "browserId required",
                    ));
                }
                Ok(())
            }
            Self::UnsubscribeViewer { browser_id, .. } => {
                if browser_id.trim().is_empty() {
                    return Err(IpcError::new(
                        IpcErrorCode::InvalidArgument,
                        "browserId required",
                    ));
                }
                Ok(())
            }
            Self::ClaimDriver { .. }
            | Self::ReleaseDriver { .. }
            | Self::HeartbeatDriver { .. } => Ok(()),
            Self::Navigate { url, .. } => {
                crate::browser::security::validate_url_remote(url)
                    .map_err(|e| remote_url_error_to_ipc(IpcError::from(e)))?;
                Ok(())
            }
            Self::Fill {
                reference,
                value,
                snapshot_id,
                map_revision,
                ..
            } => {
                crate::browser::remote_input::validate_fill(reference, value)
                    .map_err(|e| IpcError::new(IpcErrorCode::InvalidArgument, e.to_string()))?;
                match (snapshot_id, map_revision) {
                    (Some(sid), Some(_)) if !sid.trim().is_empty() => {}
                    _ => {
                        return Err(IpcError::new(
                            IpcErrorCode::Custom("BROWSER_INVALID_SNAPSHOT".into()),
                            "remote fill requires valid snapshot_id and map_revision",
                        ));
                    }
                }
                Ok(())
            }
            Self::Keypress { key, .. } => {
                crate::browser::remote_input::validate_page_key(key)
                    .map_err(|e| IpcError::new(IpcErrorCode::InvalidArgument, e.to_string()))?;
                Ok(())
            }
            Self::Eval {
                script,
                has_approval,
                ..
            } => {
                crate::browser::remote_input::validate_eval_script(script, *has_approval)
                    .map_err(|e| IpcError::new(IpcErrorCode::InvalidArgument, e.to_string()))?;
                Ok(())
            }
            Self::Wait {
                condition,
                has_approval,
                ..
            } => {
                let script: Option<&str> = match condition {
                    BrowserWaitCondition::Function { script } => Some(script.as_str()),
                    BrowserWaitCondition::WithTimeout { inner, .. } => match &**inner {
                        BrowserWaitCondition::Function { script } => Some(script.as_str()),
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(script) = script {
                    crate::browser::remote_input::validate_eval_script(script, *has_approval)
                        .map_err(|e| IpcError::new(IpcErrorCode::InvalidArgument, e.to_string()))?;
                }
                Ok(())
            }
            Self::Click {
                reference,
                snapshot_id,
                map_revision,
                u,
                v,
                ..
            } => {
                if let Some(ref_str) = reference {
                    if ref_str.trim().is_empty() {
                        return Err(IpcError::new(
                            IpcErrorCode::InvalidArgument,
                            "click reference cannot be empty",
                        ));
                    }
                    match (snapshot_id, map_revision) {
                        (Some(sid), Some(_)) if !sid.trim().is_empty() => {}
                        _ => {
                            return Err(IpcError::new(
                                IpcErrorCode::Custom("BROWSER_INVALID_SNAPSHOT".into()),
                                "remote reference click requires valid snapshot_id and map_revision",
                            ));
                        }
                    }
                } else if let (Some(u_val), Some(v_val)) = (*u, *v) {
                    if !u_val.is_finite()
                        || !v_val.is_finite()
                        || !(0.0..=1.0).contains(&u_val)
                        || !(0.0..=1.0).contains(&v_val)
                    {
                        return Err(IpcError::new(
                            IpcErrorCode::InvalidArgument,
                            "click coordinates must be finite and within [0.0, 1.0]",
                        ));
                    }
                } else {
                    return Err(IpcError::new(
                        IpcErrorCode::InvalidArgument,
                        "click requires either reference or (u, v) coordinates",
                    ));
                }
                Ok(())
            }
            Self::Snapshot { browser_id } => {
                if browser_id.trim().is_empty() {
                    return Err(IpcError::new(
                        IpcErrorCode::InvalidArgument,
                        "browser_id cannot be empty",
                    ));
                }
                Ok(())
            }
            Self::Back { .. }
            | Self::Forward { .. }
            | Self::Reload { .. }
            | Self::GetState { .. } => Ok(()),
        }
    }
}

/// Maps a URL policy failure on a remote operation onto the remote error codes:
/// a scheme denial is a policy refusal, a malformed URL is a bad request.
fn remote_url_error_to_ipc(e: IpcError) -> IpcError {
    match e.code {
        IpcErrorCode::BrowserUrlSchemeDenied => {
            IpcError::new(IpcErrorCode::Custom("BROWSER_FORBIDDEN".into()), e.message)
        }
        IpcErrorCode::BrowserUrlInvalid => {
            IpcError::new(IpcErrorCode::Custom("BROWSER_INVALID_REQUEST".into()), e.message)
        }
        _ => e,
    }
}

#[cfg(feature = "desktop")]
/// Builds the script for a remote-operation keypress. It reuses the remote executor's
/// builder, which dispatches the trimmed key that validate_page_key returns.
fn keypress_operation_script(key: &str) -> Result<String, IpcError> {
    crate::ipc::browser::remote_keypress_script(key)
        .map_err(|e| IpcError::new(IpcErrorCode::InvalidArgument, e.to_string()))
}

/// Upper bound on one browser wait requested over the CLI or a framed remote operation.
#[cfg(feature = "desktop")]
const MAX_WAIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

#[cfg(feature = "desktop")]
fn clamp_wait_timeout_ms(timeout_ms: u64) -> u64 {
    timeout_ms.min(MAX_WAIT_TIMEOUT.as_millis() as u64)
}

#[cfg(feature = "desktop")]
fn clamp_cli_wait_condition(condition: BrowserWaitCondition) -> BrowserWaitCondition {
    match condition {
        BrowserWaitCondition::WithTimeout { inner, timeout_ms } => {
            BrowserWaitCondition::WithTimeout {
                inner,
                timeout_ms: clamp_wait_timeout_ms(timeout_ms),
            }
        }
        other => other,
    }
}

#[cfg(feature = "desktop")]
fn browser_unavailable(message: &str) -> IpcError {
    IpcError::new(IpcErrorCode::Custom("BROWSER_UNAVAILABLE".into()), message)
}

/// Maps a driver broker refusal by variant: a lease problem is a policy refusal and a
/// busy lease or a rate limit keeps its own retryable code (the daemon maps
/// BROWSER_DRIVER_BUSY and RATE_LIMITED back to typed errors).
#[cfg(feature = "desktop")]
fn driver_error_to_ipc(e: crate::browser::remote_driver::RemoteDriverError) -> IpcError {
    use crate::browser::remote_driver::RemoteDriverError;
    let code = match e {
        RemoteDriverError::RateLimited => IpcErrorCode::RateLimited,
        RemoteDriverError::BrowserDriverBusy { .. } => {
            IpcErrorCode::Custom("BROWSER_DRIVER_BUSY".into())
        }
        RemoteDriverError::InvalidLease(_) => {
            IpcErrorCode::Custom("BROWSER_INVALID_REQUEST".into())
        }
        RemoteDriverError::Unauthorized(_)
        | RemoteDriverError::StaleLease { .. }
        | RemoteDriverError::DesktopReclaimed => IpcErrorCode::Custom("BROWSER_FORBIDDEN".into()),
    };
    IpcError::new(code, e.to_string())
}

#[cfg(feature = "desktop")]
pub async fn execute_remote_operation<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &Arc<BrowserManager>,
    op: RemoteBrowserOperation,
) -> Result<serde_json::Value, IpcError> {
    op.validate()?;
    // Direct mutations carry no driver lease or identity fence; only `execute` runs the
    // command guard. They are refused after their parameters were validated.
    if matches!(
        op,
        RemoteBrowserOperation::Navigate { .. }
            | RemoteBrowserOperation::Back { .. }
            | RemoteBrowserOperation::Forward { .. }
            | RemoteBrowserOperation::Reload { .. }
            | RemoteBrowserOperation::Click { .. }
            | RemoteBrowserOperation::Fill { .. }
            | RemoteBrowserOperation::Keypress { .. }
    ) {
        return Err(IpcError::new(
            IpcErrorCode::Custom("BROWSER_FORBIDDEN".into()),
            "direct remote mutations are refused; use the execute operation with a driver lease",
        ));
    }
    match op {
        RemoteBrowserOperation::Navigate { browser_id, url } => {
            // A framed remote operation: the Remote URL policy applies (file:// denied).
            // A scheme denial is a policy refusal; a malformed URL stays a bad request.
            navigate_browser_session_with_context(
                app,
                manager,
                &browser_id,
                &url,
                crate::browser::security::BrowserSecurityContext::Remote,
            )
            .await
            .map_err(remote_url_error_to_ipc)?;
            serde_json::to_value(())
                .map_err(|e| IpcError::new(IpcErrorCode::BrowserAutomationFailed, e.to_string()))
        }
        RemoteBrowserOperation::Back { browser_id } => {
            crate::ipc::browser::history_navigation(app, manager, &browser_id, false)?;
            let state = manager.get_state(&browser_id)?;
            serde_json::to_value(&state)
                .map_err(|e| IpcError::new(IpcErrorCode::BrowserAutomationFailed, e.to_string()))
        }
        RemoteBrowserOperation::Forward { browser_id } => {
            crate::ipc::browser::history_navigation(app, manager, &browser_id, true)?;
            let state = manager.get_state(&browser_id)?;
            serde_json::to_value(&state)
                .map_err(|e| IpcError::new(IpcErrorCode::BrowserAutomationFailed, e.to_string()))
        }
        RemoteBrowserOperation::Reload { browser_id } => {
            let state = manager.begin_reload(&browser_id)?;
            let webview = app
                .get_webview(&state.webview_label)
                .ok_or_else(|| BrowserError::WebviewNotFound(state.webview_label.clone()))?;
            webview
                .reload()
                .map_err(|e| BrowserError::Internal(e.to_string()))?;
            serde_json::to_value(&state)
                .map_err(|e| IpcError::new(IpcErrorCode::BrowserAutomationFailed, e.to_string()))
        }
        RemoteBrowserOperation::Click {
            browser_id,
            reference,
            snapshot_id,
            map_revision,
            u,
            v,
            stream_id,
            sequence_number,
            document_generation,
            viewport_revision,
            capture_rect,
            geometry_source,
            x,
            y,
        } => {
            let state = manager.get_state(&browser_id)?;

            // Geometry and document generation fencing against referenced frame metadata:
            if let Some(frame_gen) = document_generation {
                if state.generation != frame_gen {
                    return Err(IpcError::new(
                        IpcErrorCode::Custom("BROWSER_STALE_GENERATION".into()),
                        format!(
                            "stale click document generation: referenced frame has generation {frame_gen}, but manager generation is {}",
                            state.generation
                        ),
                    ));
                }
            }

            let (bounds, _zoom, current_vp_rev) = manager.get_geometry(&browser_id)?;

            if let Some(frame_vp_rev) = viewport_revision {
                if current_vp_rev != frame_vp_rev {
                    return Err(IpcError::new(
                        IpcErrorCode::Custom("BROWSER_STALE_VIEWPORT".into()),
                        format!(
                            "stale click viewport revision: referenced frame has revision {frame_vp_rev}, but manager revision is {current_vp_rev}"
                        ),
                    ));
                }
            }

            let (script, click_x, click_y) = if let Some(ref_str) = reference {
                let (snap_id, map_rev) = match (snapshot_id, map_revision) {
                    (Some(sid), Some(rev)) if !sid.trim().is_empty() => (sid, rev),
                    _ => {
                        return Err(IpcError::new(
                            IpcErrorCode::Custom("BROWSER_INVALID_SNAPSHOT".into()),
                            "remote reference click requires valid snapshot_id and map_revision",
                        ));
                    }
                };
                let selector = manager
                    .verify_remote_target(&browser_id, &snap_id, map_rev, &ref_str)
                    .map_err(|e| match e {
                        BrowserError::AutomationSnapshotStale
                        | BrowserError::AutomationTargetNotFound(_) => IpcError::new(
                            IpcErrorCode::Custom("BROWSER_INVALID_SNAPSHOT".into()),
                            "remote snapshot reference missing, stale, or target not found",
                        ),
                        other => IpcError::from(other),
                    })?;
                let selector_json = serde_json::to_string(&selector).map_err(|e| {
                    IpcError::new(IpcErrorCode::BrowserAutomationFailed, e.to_string())
                })?;
                (
                    crate::browser::remote_input::build_selector_click_script(&selector_json),
                    None,
                    None,
                )
            } else if let (Some(px), Some(py)) = (x, y) {
                (
                    crate::browser::remote_input::build_point_click_script(px, py),
                    Some(px),
                    Some(py),
                )
            } else if let (Some(u_val), Some(v_val)) = (u, v) {
                let rect = capture_rect
                    .or(bounds)
                    .unwrap_or(crate::browser::model::LogicalRect {
                        x: 0.0,
                        y: 0.0,
                        width: 1024.0,
                        height: 768.0,
                    });
                let pt = crate::browser::remote_input::map_point_mainframe(
                    u_val, v_val, &rect, false, false, false,
                )
                .map_err(|e| IpcError::new(e.ipc_error_code(), e.to_string()))?;
                (
                    crate::browser::remote_input::build_point_click_script(pt.x, pt.y),
                    Some(pt.x),
                    Some(pt.y),
                )
            } else {
                return Err(IpcError::new(
                    IpcErrorCode::InvalidArgument,
                    "missing click target",
                ));
            };

            let webview = app
                .get_webview(&state.webview_label)
                .ok_or_else(|| BrowserError::WebviewNotFound(state.webview_label.clone()))?;
            let res_str = crate::ipc::browser::eval_webview(webview, script).await?;
            crate::browser::remote_input::decode_point_click_result(&res_str)
                .map_err(point_click_decode_error_to_ipc)?;
            let mut res_map = serde_json::Map::new();
            res_map.insert("clicked".into(), serde_json::json!(true));
            if let Some(sid) = stream_id {
                res_map.insert("streamId".into(), serde_json::json!(sid));
            }
            if let Some(seq) = sequence_number {
                res_map.insert("sequenceNumber".into(), serde_json::json!(seq));
            }
            res_map.insert(
                "documentGeneration".into(),
                serde_json::json!(state.generation),
            );
            res_map.insert("viewportRevision".into(), serde_json::json!(current_vp_rev));
            if let Some(gs) = geometry_source {
                res_map.insert("geometrySource".into(), serde_json::Value::String(gs));
            }
            if let Some(cx) = click_x {
                res_map.insert("x".into(), serde_json::json!(cx));
            }
            if let Some(cy) = click_y {
                res_map.insert("y".into(), serde_json::json!(cy));
            }
            Ok(serde_json::Value::Object(res_map))
        }
        RemoteBrowserOperation::Fill {
            browser_id,
            reference,
            value,
            snapshot_id,
            map_revision,
        } => {
            let state = manager.get_state(&browser_id)?;
            let (snap_id, map_rev) = match (snapshot_id, map_revision) {
                (Some(sid), Some(rev)) if !sid.trim().is_empty() => (sid, rev),
                _ => {
                    return Err(IpcError::new(
                        IpcErrorCode::Custom("BROWSER_INVALID_SNAPSHOT".into()),
                        "remote fill requires valid snapshot_id and map_revision",
                    ));
                }
            };
            let selector = manager
                .verify_remote_target(&browser_id, &snap_id, map_rev, &reference)
                .map_err(|e| match e {
                    BrowserError::AutomationSnapshotStale
                    | BrowserError::AutomationTargetNotFound(_) => IpcError::new(
                        IpcErrorCode::Custom("BROWSER_INVALID_SNAPSHOT".into()),
                        "remote snapshot reference missing, stale, or target not found",
                    ),
                    other => IpcError::from(other),
                })?;
            let webview = app
                .get_webview(&state.webview_label)
                .ok_or_else(|| BrowserError::WebviewNotFound(state.webview_label.clone()))?;
            let selector_json = serde_json::to_string(&selector)
                .map_err(|e| IpcError::new(IpcErrorCode::BrowserAutomationFailed, e.to_string()))?;
            let value_json = serde_json::to_string(&value)
                .map_err(|e| IpcError::new(IpcErrorCode::BrowserAutomationFailed, e.to_string()))?;
            let script = format!(
                r#"(function() {{
                    const el = document.querySelector({});
                    if (!el) return JSON.stringify({{ ok: false, code: "not_found", error: "element not found" }});
                    el.value = {};
                    el.dispatchEvent(new Event("input", {{ bubbles: true }}));
                    el.dispatchEvent(new Event("change", {{ bubbles: true }}));
                    return JSON.stringify({{ ok: true }});
                }})()"#,
                selector_json, value_json
            );
            let res_str = crate::ipc::browser::eval_webview(webview, script).await?;
            crate::browser::remote_input::decode_action_result(&res_str).map_err(|err| {
                use crate::browser::remote_input::ActionDecodeError;
                match err {
                    ActionDecodeError::NotFound(message) => IpcError::new(
                        IpcErrorCode::Custom("BROWSER_TARGET_NOT_FOUND".into()),
                        message,
                    ),
                    other => IpcError::new(IpcErrorCode::BrowserAutomationFailed, other.to_string()),
                }
            })?;
            Ok(serde_json::json!({ "filled": true }))
        }
        RemoteBrowserOperation::Keypress { browser_id, key } => {
            let state = manager.get_state(&browser_id)?;
            let webview = app
                .get_webview(&state.webview_label)
                .ok_or_else(|| BrowserError::WebviewNotFound(state.webview_label.clone()))?;
            let script = keypress_operation_script(&key)?;
            let _ = crate::ipc::browser::eval_webview(webview, script).await?;
            Ok(serde_json::json!({ "dispatched": true }))
        }
        RemoteBrowserOperation::Wait {
            browser_id,
            condition,
            timeout_ms,
            ..
        } => {
            let timeout =
                timeout_ms.map(|ms| std::time::Duration::from_millis(clamp_wait_timeout_ms(ms)));
            crate::ipc::browser::wait_browser_session_with_timeout(
                app,
                manager,
                &browser_id,
                condition,
                timeout,
            )
            .await?;
            Ok(serde_json::json!({ "conditionMet": true }))
        }
        RemoteBrowserOperation::Eval {
            browser_id, script, ..
        } => {
            let state = manager.get_state(&browser_id)?;
            let webview = app
                .get_webview(&state.webview_label)
                .ok_or_else(|| BrowserError::WebviewNotFound(state.webview_label.clone()))?;
            let raw = crate::ipc::browser::eval_webview(webview, script).await?;
            let (truncated, was_truncated) =
                crate::browser::remote_input::truncate_eval_result(&raw);
            Ok(serde_json::json!({
                "result": truncated,
                "truncated": was_truncated,
            }))
        }
        RemoteBrowserOperation::GetState { browser_id } => {
            let state = manager.get_state(&browser_id)?;
            let instance_id = manager.get_instance_id(&browser_id).unwrap_or_default();
            let (bounds, _, viewport_revision) =
                manager.get_geometry(&browser_id).unwrap_or((None, 1.0, 1));
            let (service_epoch, desktop_epoch) = if let Some(svc) =
                app.try_state::<Arc<crate::browser::remote_service::BrowserRemoteService>>()
            {
                (svc.service_epoch(), svc.desktop_epoch())
            } else if let Some(backend) = app
                .try_state::<Arc<crate::remote::browser_backend::InProcessBrowserServiceBackend>>()
            {
                (
                    backend.remote_service.service_epoch(),
                    backend.remote_service.desktop_epoch(),
                )
            } else if let Some(mgr) =
                app.try_state::<Arc<crate::ipc::remote::RemoteGatewayManager>>()
            {
                if let Some(st) = mgr.state() {
                    (
                        st.browser_service_epoch
                            .load(std::sync::atomic::Ordering::Relaxed),
                        1,
                    )
                } else {
                    (1, 1)
                }
            } else {
                (1, 1)
            };
            Ok(serde_json::json!({
                "browserId": state.browser_id,
                "browserInstanceId": instance_id,
                "browserServiceEpoch": service_epoch.to_string(),
                "desktopEpoch": desktop_epoch.to_string(),
                "documentGeneration": state.generation.to_string(),
                "url": state.url,
                "title": state.title,
                "loading": state.loading,
                "generation": state.generation.to_string(),
                "viewportRevision": viewport_revision.to_string(),
                "bounds": bounds,
            }))
        }
        RemoteBrowserOperation::List {
            workspace_id,
            worktree_slug,
        } => {
            let sessions = manager.list_sessions();
            let filtered: Vec<_> = sessions
                .into_iter()
                .filter(|s| {
                    let ws_match = match &workspace_id {
                        Some(ws) if !ws.is_empty() => s.workspace_id.as_deref() == Some(ws),
                        _ => true,
                    };
                    let wt_path = manager
                        .get_state(&s.browser_id)
                        .ok()
                        .and_then(|st| st.worktree_path);
                    let wt_match = match &worktree_slug {
                        Some(wt) if !wt.is_empty() => {
                            crate::remote::browser_backend::matches_worktree_slug(
                                wt_path.as_deref(),
                                wt,
                            )
                        }
                        _ => true,
                    };
                    ws_match && wt_match
                })
                .map(|s| {
                    let wt_path = manager
                        .get_state(&s.browser_id)
                        .ok()
                        .and_then(|st| st.worktree_path);
                    serde_json::json!({
                        "browserId": s.browser_id,
                        "title": s.title,
                        "url": s.url,
                        "visible": s.visible,
                        "workspaceId": s.workspace_id,
                        "worktreePath": wt_path,
                    })
                })
                .collect();
            Ok(serde_json::Value::Array(filtered))
        }
        RemoteBrowserOperation::Execute {
            browser_id,
            command,
            params,
            document_generation,
            browser_instance_id,
            desktop_epoch,
            lease_epoch,
            device_id,
            connection_id,
        } => {
            let is_mutation = command != "getState" && command != "snapshot";
            let service = app.try_state::<Arc<crate::browser::remote_service::BrowserRemoteService>>()
                .map(|s| Arc::clone(&s))
                .or_else(|| {
                    app.try_state::<Arc<crate::remote::browser_backend::InProcessBrowserServiceBackend>>()
                        .map(|b| Arc::clone(&b.remote_service))
                });

            if is_mutation {
                if let Some(service) = service {
                    let lease_ep = lease_epoch
                        .as_deref()
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or(0);
                    let dt_ep = desktop_epoch
                        .as_deref()
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or(0);
                    let gen = document_generation
                        .as_deref()
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or(0);
                    let dev = device_id.as_deref().unwrap_or("");
                    let conn = connection_id.as_deref().unwrap_or("");
                    let inst = browser_instance_id.as_deref().unwrap_or("");

                    service.execute_command_guard(crate::browser::remote_service::CommandGuardParams { browser_id: &browser_id, lease_epoch: lease_ep, device_id: dev, connection_id: conn, instance_id: inst, desktop_epoch: dt_ep, generation: gen })
                        .map_err(|e| match e {
                            crate::browser::remote_service::RemoteServiceError::BrowserNotFound(id) => {
                                IpcError::new(IpcErrorCode::Custom("BROWSER_NOT_FOUND".into()), id)
                            }
                            crate::browser::remote_service::RemoteServiceError::StaleLease
                            | crate::browser::remote_service::RemoteServiceError::DesktopReclaimed => {
                                IpcError::new(IpcErrorCode::Custom("BROWSER_FORBIDDEN".into()), e.to_string())
                            }
                            crate::browser::remote_service::RemoteServiceError::StaleInstance
                            | crate::browser::remote_service::RemoteServiceError::StaleGeneration => {
                                IpcError::new(IpcErrorCode::Custom("BROWSER_STALE_IDENTITY".into()), e.to_string())
                            }
                            other => IpcError::new(IpcErrorCode::Custom("BROWSER_EXECUTION_FAILED".into()), other.to_string()),
                        })?;
                } else {
                    let broker =
                        app.try_state::<Arc<crate::browser::remote_driver::RemoteDriverBroker>>();
                    let lease_ep = lease_epoch
                        .as_deref()
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or(0);
                    let dev = device_id.as_deref().unwrap_or("");
                    let conn = connection_id.as_deref().unwrap_or("");
                    if let Some(broker) = broker {
                        broker.validate_lease(&browser_id, lease_ep, dev, conn)
                            .map_err(|e| match e {
                                crate::browser::remote_driver::RemoteDriverError::DesktopReclaimed => {
                                    IpcError::new(IpcErrorCode::Custom("BROWSER_FORBIDDEN".into()), "Desktop reclaimed")
                                }
                                _ => IpcError::new(IpcErrorCode::Custom("BROWSER_FORBIDDEN".into()), "Active driver lease required for mutation"),
                            })?;
                    } else {
                        // No broker means no lease can be checked; never fail open.
                        return Err(browser_unavailable(
                            "remote driver broker unavailable; mutation refused",
                        ));
                    }
                }
            }

            if let Some(ref inst) = browser_instance_id {
                let actual_inst = manager.get_instance_id(&browser_id)?;
                if inst != &actual_inst {
                    return Err(IpcError::new(
                        IpcErrorCode::Custom("BROWSER_STALE_IDENTITY".into()),
                        format!("Stale instance ID: expected {actual_inst}, got {inst}"),
                    ));
                }
            }
            if let Some(ref gen_str) = document_generation {
                let state = manager.get_state(&browser_id)?;
                if gen_str != &state.generation.to_string() {
                    return Err(IpcError::new(
                        IpcErrorCode::Custom("BROWSER_STALE_IDENTITY".into()),
                        format!(
                            "Stale document generation: expected {}, got {gen_str}",
                            state.generation
                        ),
                    ));
                }
            }

            // R6-2: Route click/fill/keypress/snapshot/eval/wait through the shared guarded executor
            let executor = crate::ipc::browser::GuiBrowserCommandExecutor::new(
                app.clone(),
                Arc::clone(manager),
            );
            let ctx = crate::remote::browser_backend::BrowserCommandContext {
                browser_id,
                command,
                params,
                document_generation,
                browser_instance_id,
                desktop_epoch,
                lease_epoch,
                device_id,
                connection_id,
            };
            let exec_res =
                crate::remote::browser_backend::BrowserCommandExecutor::execute(&executor, ctx)
                    .await;
            match exec_res {
                Ok(result) => Ok(result
                    .value
                    .unwrap_or_else(|| serde_json::json!({ "success": true }))),
                Err(e) => Err(remote_browser_error_to_ipc(e)),
            }
        }
        RemoteBrowserOperation::SubscribeViewer {
            browser_id,
            device_id,
            viewer_instance_id,
            options,
        } => {
            let service = app.try_state::<Arc<crate::browser::remote_service::BrowserRemoteService>>()
                .map(|s| Arc::clone(&s))
                .or_else(|| {
                    app.try_state::<Arc<crate::remote::browser_backend::InProcessBrowserServiceBackend>>()
                        .map(|b| Arc::clone(&b.remote_service))
                });
            if let Some(service) = service {
                let requested_profile = options.as_ref().map(|opt| {
                    let format = match opt.format {
                        crate::remote::browser_protocol::BrowserImageFormat::Png => {
                            crate::browser::snapshot_source::SnapshotFormat::Png
                        }
                        crate::remote::browser_protocol::BrowserImageFormat::Jpeg => {
                            crate::browser::snapshot_source::SnapshotFormat::Jpeg {
                                quality: opt.quality.unwrap_or(70).clamp(1, 100),
                            }
                        }
                    };
                    crate::browser::remote_service::NegotiatedCaptureProfile {
                        format,
                        quality: opt.quality.unwrap_or(70).clamp(1, 100),
                        interval_ms: opt.interval_ms.unwrap_or(80).clamp(50, 5000),
                        max_edge: opt.max_edge.unwrap_or(2048).clamp(64, 2048),
                    }
                });

                let (sub_id, negotiated_prof) = service
                    .subscribe_with_profile(
                        &browser_id,
                        &device_id,
                        &viewer_instance_id,
                        requested_profile,
                    )
                    .map_err(|e| {
                        IpcError::new(
                            IpcErrorCode::Custom("BROWSER_SUBSCRIPTION_FAILED".into()),
                            e.to_string(),
                        )
                    })?;
                let stream_id = service.active_stream_id(&browser_id).unwrap_or(1);
                // The subscription is live from here on: a failure must not leave it behind.
                let state = match manager.get_state(&browser_id) {
                    Ok(state) => state,
                    Err(error) => {
                        service.unsubscribe(&browser_id, &sub_id);
                        return Err(error.into());
                    }
                };
                let instance_id = manager.get_instance_id(&browser_id).unwrap_or_default();

                let negotiated_options = crate::remote::browser_protocol::BrowserSubscribeOptions {
                    format: match negotiated_prof.format {
                        crate::browser::snapshot_source::SnapshotFormat::Png => {
                            crate::remote::browser_protocol::BrowserImageFormat::Png
                        }
                        crate::browser::snapshot_source::SnapshotFormat::Jpeg { .. } => {
                            crate::remote::browser_protocol::BrowserImageFormat::Jpeg
                        }
                    },
                    quality: match negotiated_prof.format {
                        crate::browser::snapshot_source::SnapshotFormat::Jpeg { quality } => {
                            Some(quality)
                        }
                        _ => None,
                    },
                    interval_ms: Some(negotiated_prof.interval_ms),
                    max_edge: Some(negotiated_prof.max_edge),
                };

                let identity = crate::remote::browser_backend::BrowserSubscribeIdentity {
                    browser_instance_id: instance_id,
                    browser_service_epoch: service.service_epoch().to_string(),
                    desktop_epoch: service.desktop_epoch().to_string(),
                    document_generation: state.generation.to_string(),
                };

                Ok(serde_json::json!({
                    "subscriptionId": sub_id,
                    "streamId": stream_id,
                    "options": negotiated_options,
                    "identity": identity,
                }))
            } else {
                Err(browser_unavailable(
                    "browser remote service unavailable; cannot stream frames",
                ))
            }
        }
        RemoteBrowserOperation::UnsubscribeViewer {
            browser_id,
            subscription_id,
        } => {
            let service = app.try_state::<Arc<crate::browser::remote_service::BrowserRemoteService>>()
                .map(|s| Arc::clone(&s))
                .or_else(|| {
                    app.try_state::<Arc<crate::remote::browser_backend::InProcessBrowserServiceBackend>>()
                        .map(|b| Arc::clone(&b.remote_service))
                });
            if let Some(service) = service {
                service.unsubscribe(&browser_id, &subscription_id);
            }
            Ok(serde_json::json!({ "unsubscribed": true }))
        }
        RemoteBrowserOperation::ClaimDriver {
            browser_id,
            device_id,
            connection_id,
            subscription_id,
            lease_epoch,
        } => {
            let broker = app
                .try_state::<Arc<crate::browser::remote_driver::RemoteDriverBroker>>()
                .map(|b| Arc::clone(&b))
                .or_else(|| {
                    app.try_state::<Arc<crate::browser::remote_service::BrowserRemoteService>>()
                        .map(|s| s.driver_broker().clone())
                });
            if let Some(broker) = broker {
                let lease = broker
                    .claim_with_epoch(
                        &device_id,
                        &connection_id,
                        &subscription_id,
                        &browser_id,
                        true,
                        lease_epoch.unwrap_or(0),
                    )
                    .map_err(driver_error_to_ipc)?;
                // `expires_at` is a future Instant, so report the remaining lifetime.
                let expires_in = lease
                    .expires_at
                    .saturating_duration_since(std::time::Instant::now());
                Ok(serde_json::json!({
                    "leaseEpoch": lease.lease_epoch.to_string(),
                    "expiresInMs": expires_in.as_millis() as u64,
                }))
            } else {
                Err(browser_unavailable("remote driver broker unavailable"))
            }
        }
        RemoteBrowserOperation::ReleaseDriver {
            subscription_id,
            lease_epoch,
        } => {
            let broker = app
                .try_state::<Arc<crate::browser::remote_driver::RemoteDriverBroker>>()
                .map(|b| Arc::clone(&b))
                .or_else(|| {
                    app.try_state::<Arc<crate::browser::remote_service::BrowserRemoteService>>()
                        .map(|s| s.driver_broker().clone())
                });
            if let Some(broker) = broker {
                let _ = broker.release(&subscription_id, lease_epoch.unwrap_or(0));
                Ok(serde_json::json!({ "status": "ok" }))
            } else {
                Err(browser_unavailable("remote driver broker unavailable"))
            }
        }
        RemoteBrowserOperation::HeartbeatDriver {
            device_id,
            connection_id,
            subscription_id,
            lease_epoch,
        } => {
            let broker = app
                .try_state::<Arc<crate::browser::remote_driver::RemoteDriverBroker>>()
                .map(|b| Arc::clone(&b))
                .or_else(|| {
                    app.try_state::<Arc<crate::browser::remote_service::BrowserRemoteService>>()
                        .map(|s| s.driver_broker().clone())
                });
            if let Some(broker) = broker {
                broker
                    .heartbeat(
                        &device_id,
                        &connection_id,
                        &subscription_id,
                        lease_epoch.unwrap_or(0),
                    )
                    .map_err(driver_error_to_ipc)?;
                Ok(serde_json::json!({ "status": "ok" }))
            } else {
                Err(browser_unavailable("remote driver broker unavailable"))
            }
        }
        RemoteBrowserOperation::Snapshot { browser_id } => {
            let state = manager.get_state(&browser_id)?;
            let webview = app
                .get_webview(&state.webview_label)
                .ok_or_else(|| BrowserError::WebviewNotFound(state.webview_label.clone()))?;
            let result = crate::ipc::browser::eval_webview(
                webview,
                crate::ipc::browser::AUTOMATION_SNAPSHOT_SCRIPT.to_string(),
            )
            .await?;
            let snapshot_json: String = serde_json::from_str(&result).map_err(|error| {
                BrowserError::AutomationFailed(format!("invalid snapshot callback result: {error}"))
            })?;
            let snapshot: crate::ipc::browser::AutomationSnapshotResult =
                serde_json::from_str(&snapshot_json).map_err(|error| {
                    BrowserError::AutomationFailed(format!("invalid snapshot response: {error}"))
                })?;
            let targets = snapshot
                .elements
                .iter()
                .map(|element| crate::browser::model::BrowserAutomationTarget {
                    reference: element.reference.clone(),
                    selector: element.selector.clone(),
                })
                .collect();
            let (snapshot_id, map_revision) = manager
                .record_remote_snapshot(&browser_id, state.generation, targets)
                .map_err(|e| match e {
                    BrowserError::AutomationSnapshotStale => IpcError::new(
                        IpcErrorCode::Custom("BROWSER_INVALID_SNAPSHOT".into()),
                        "document generation changed during snapshot capture",
                    ),
                    other => IpcError::from(other),
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

            Ok(serde_json::json!({
                "snapshotId": snapshot_id,
                "mapRevision": map_revision,
                "mapRevisionString": map_revision.to_string(),
                "documentGeneration": state.generation.to_string(),
                "elementsCount": elements_catalogue.len(),
                "elements": elements_catalogue,
            }))
        }
    }
}

/// An authenticated request line: the capability token plus the command itself.
///
/// The control socket drives the user's logged-in browser, so possession of the
/// endpoint address alone must never be sufficient. On Windows the endpoint is a
/// loopback TCP port that every local process can reach, and on unix the socket
/// mode only narrows callers to the same uid. The token is what actually proves
/// the caller was allowed to read the capability file this process wrote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserCliEnvelope {
    pub token: String,
    #[serde(flatten)]
    pub request: BrowserCliRequest,
}

pub const BROWSER_CLI_UNAUTHORIZED: &str = "BROWSER_CLI_UNAUTHORIZED";

/// Length of the hex-encoded capability token (32 bytes of entropy).
const BROWSER_CLI_TOKEN_BYTES: usize = 32;

/// Capability file that carries the token for the current server instance. It
/// sits beside the socket/port file inside the 0700 runtime directory.
pub fn browser_cli_token_path() -> PathBuf {
    crate::daemon::server::get_runtime_dir().join("browser.token")
}

fn token_path_for(endpoint_path: &Path) -> PathBuf {
    endpoint_path.with_file_name(match endpoint_path.file_name().and_then(|n| n.to_str()) {
        Some(name) => format!("{name}.token"),
        None => "browser.token".to_string(),
    })
}

fn generate_browser_cli_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; BROWSER_CLI_TOKEN_BYTES];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Compares two tokens without leaking their matching prefix length through
/// timing. Length is public information here, so an early length check is safe.
fn tokens_match(expected: &str, provided: &str) -> bool {
    let expected = expected.as_bytes();
    let provided = provided.as_bytes();
    if expected.len() != provided.len() {
        return false;
    }
    let mut difference = 0u8;
    for (left, right) in expected.iter().zip(provided.iter()) {
        difference |= left ^ right;
    }
    difference == 0
}

fn write_token_file(path: &Path, token: &str) -> Result<(), BrowserError> {
    match fs::symlink_metadata(path) {
        Ok(_) => fs::remove_file(path).map_err(|error| {
            BrowserError::Internal(format!(
                "Failed to replace browser CLI token file {}: {error}",
                path.display()
            ))
        })?,
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => {
            return Err(BrowserError::Internal(format!(
                "Failed to inspect browser CLI token file {}: {error}",
                path.display()
            )))
        }
    }
    fs::write(path, token).map_err(|error| {
        BrowserError::Internal(format!("Failed to write browser CLI token file: {error}"))
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|error| {
            BrowserError::Internal(format!(
                "Failed to restrict browser CLI token file permissions: {error}"
            ))
        })?;
    }
    Ok(())
}

fn read_token_file(path: &Path) -> Result<String, BrowserError> {
    let token = fs::read_to_string(path).map_err(|error| {
        BrowserError::CliUnavailable(format!("Ferryx desktop app is not running: {error}"))
    })?;
    let token = token.trim().to_string();
    if token.is_empty() {
        return Err(BrowserError::CliUnavailable(format!(
            "Invalid browser CLI token in {}: token is empty",
            path.display()
        )));
    }
    Ok(token)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum BrowserCliResponse {
    List {
        sessions: Vec<BrowserSessionSummary>,
    },
    Snapshot {
        snapshot: BrowserAutomationSnapshot,
    },
    Acted,
    Opened {
        browser: BrowserSessionSummary,
        adoption: crate::browser::adoption::BrowserAdoption,
    },
    Navigated,
    Closed,
    Identified {
        browser: Option<BrowserSessionSummary>,
    },
    Evaluated {
        result: Option<String>,
        truncated: bool,
    },
    Waited,
    Focused,
    ScreenshotSaved {
        path: String,
    },
    ConsoleEntries {
        entries: Vec<BrowserConsoleEntry>,
    },
    CookieEntries {
        cookies: Vec<BrowserCookieEntry>,
    },
    StorageValue {
        value: Option<String>,
    },
    TabSwitched {
        browser: BrowserSessionSummary,
        index: usize,
    },
    StateSaved {
        path: String,
    },
    StateLoaded,
    #[serde(rename_all = "camelCase")]
    RemoteAttached {
        service_epoch: String,
        protocol_version: u32,
    },
    DialogHandled {
        dialog: crate::browser::model::BrowserDialogEntry,
    },
    DialogEntries {
        dialogs: Vec<crate::browser::model::BrowserDialogEntry>,
    },
    DialogPolicySet {
        policy: String,
    },
    DownloadStarted {
        download: crate::browser::download::DownloadRecord,
    },
    Downloads {
        downloads: Vec<crate::browser::download::DownloadRecord>,
    },
    DownloadCancelled {
        id: String,
        cancelled: bool,
    },
    Highlighted {
        highlight: crate::browser::picker::BrowserHighlightResult,
    },
    #[serde(rename_all = "camelCase")]
    FileOpened {
        path: String,
    },
    Error {
        code: String,
        message: String,
        /// Structured context of the failure (for example a typed input refusal), carried
        /// from `IpcError::details` so the daemon can rebuild the typed error.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        details: Option<serde_json::Value>,
    },
}

/// Builds the error response for an IPC failure, keeping its code, message and details.
#[cfg(feature = "desktop")]
fn error_response(error: IpcError) -> BrowserCliResponse {
    BrowserCliResponse::Error {
        code: ipc_error_code_string(error.code),
        message: error.message,
        details: error.details,
    }
}

/// Error response for a framed remote operation. A bad argument is reported as an invalid
/// request; the details (for example a typed input refusal) always travel with it.
#[cfg(feature = "desktop")]
fn framed_error_response(error: IpcError) -> BrowserCliResponse {
    let code = match error.code {
        IpcErrorCode::InvalidArgument => "BROWSER_CLI_REQUEST_INVALID".to_string(),
        other => ipc_error_code_string(other),
    };
    BrowserCliResponse::Error {
        code,
        message: error.message,
        details: error.details,
    }
}

pub const CLI_OPEN_FILE_EVENT: &str = "ferryx:cli-open-file";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliOpenFilePayload {
    pub path: String,
    pub line: Option<u32>,
    pub col: Option<u32>,
}

#[cfg(unix)]
pub fn browser_cli_socket_path() -> PathBuf {
    crate::daemon::server::get_runtime_dir().join("browser.sock")
}

#[cfg(not(unix))]
pub fn browser_cli_socket_path() -> PathBuf {
    crate::daemon::server::get_runtime_dir().join("browser.port")
}

/// Outcome of starting the browser CLI endpoint. Several GUIs share one runtime directory, so
/// at most one of them serves `browser.sock` (`browser.port` on Windows) and its token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserCliServerStart {
    /// This process holds the endpoint lock and serves the endpoint until its listener ends.
    Serving,
    /// A live instance already holds the endpoint lock. This process left the endpoint and its
    /// token untouched and serves no browser CLI endpoint of its own.
    ServedByPeer,
}

/// Lock file guarding one browser CLI endpoint (`browser.sock.lock` / `browser.port.lock`).
fn endpoint_lock_path(endpoint_path: &Path) -> PathBuf {
    endpoint_path.with_file_name(match endpoint_path.file_name().and_then(|n| n.to_str()) {
        Some(name) => format!("{name}.lock"),
        None => "browser.lock".to_string(),
    })
}

/// Claims the endpoint for this process. Only the holder of this lock may unlink, bind or
/// rewrite the endpoint and its token, and it keeps the lock for the listener's lifetime, so a
/// concurrent start can never delete a live peer's socket or replace its token. The lock is
/// released by the OS when the holder exits, which lets the next start replace a stale endpoint.
/// `Ok(None)` means a live peer holds it.
fn claim_browser_cli_endpoint(
    endpoint_path: &Path,
) -> Result<Option<crate::daemon::server::DaemonLockFile>, BrowserError> {
    let lock_path = endpoint_lock_path(endpoint_path);
    let file = crate::daemon::server::open_secure_lock_file(&lock_path).map_err(|error| {
        BrowserError::Internal(format!("Failed to open browser CLI endpoint lock: {error}"))
    })?;
    Ok(crate::daemon::server::DaemonLockFile::try_lock(file).ok())
}

pub fn write_port_file(path: &Path, port: u16) -> Result<(), BrowserError> {
    if let Some(parent) = path.parent() {
        let file_stem = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("browser_port");
        let temp_path = parent.join(format!(".{file_stem}.tmp.{}", std::process::id()));
        fs::write(&temp_path, port.to_string()).map_err(|error| {
            BrowserError::Internal(format!("Failed to write port file: {error}"))
        })?;

        match fs::symlink_metadata(path) {
            Ok(meta) => {
                if meta.file_type().is_symlink() || meta.is_dir() {
                    let _ = fs::remove_file(&temp_path);
                    return Err(BrowserError::Internal(format!(
                        "Path {} is a directory or symlink, refusing to overwrite",
                        path.display()
                    )));
                }
                if let Err(error) = fs::remove_file(path) {
                    let _ = fs::remove_file(&temp_path);
                    return Err(BrowserError::Internal(format!(
                        "Failed to replace existing port file {}: {error}",
                        path.display()
                    )));
                }
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => {
                let _ = fs::remove_file(&temp_path);
                return Err(BrowserError::Internal(format!(
                    "Failed to inspect existing port file {}: {error}",
                    path.display()
                )));
            }
        }

        if let Err(error) = fs::rename(&temp_path, path) {
            let _ = fs::remove_file(&temp_path);
            fs::write(path, port.to_string()).map_err(|write_err| {
                BrowserError::Internal(format!(
                    "Failed to persist port file: {write_err} (rename error: {error})"
                ))
            })?;
        }
    } else {
        fs::write(path, port.to_string()).map_err(|error| {
            BrowserError::Internal(format!("Failed to write port file: {error}"))
        })?;
    }
    Ok(())
}

pub fn read_port_from_file(path: &Path) -> Result<u16, BrowserError> {
    let content = fs::read_to_string(path).map_err(|error| {
        BrowserError::CliUnavailable(format!("Ferryx desktop app is not running: {error}"))
    })?;
    let port: u16 = content.trim().parse().map_err(|error| {
        BrowserError::CliUnavailable(format!(
            "Invalid browser CLI port in {}: {error}",
            path.display()
        ))
    })?;
    if port == 0 {
        return Err(BrowserError::CliUnavailable(format!(
            "Invalid browser CLI port in {}: port cannot be 0",
            path.display()
        )));
    }
    Ok(port)
}

#[cfg(feature = "desktop")]
#[cfg(unix)]
pub fn start_browser_cli_server<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: Arc<BrowserManager>,
) -> Result<BrowserCliServerStart, BrowserError> {
    start_browser_cli_server_at_path(app, manager, &browser_cli_socket_path())
}

#[cfg(feature = "desktop")]
#[cfg(unix)]
fn start_browser_cli_server_at_path<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: Arc<BrowserManager>,
    socket_path: &Path,
) -> Result<BrowserCliServerStart, BrowserError> {
    use std::os::unix::fs::{FileTypeExt, PermissionsExt};
    use std::os::unix::net::UnixListener;

    let runtime_dir = socket_path
        .parent()
        .ok_or_else(|| BrowserError::Internal("browser CLI socket path has no parent".into()))?;
    fs::create_dir_all(runtime_dir).map_err(|error| BrowserError::Internal(error.to_string()))?;
    fs::set_permissions(runtime_dir, fs::Permissions::from_mode(0o700))
        .map_err(|error| BrowserError::Internal(error.to_string()))?;
    let Some(endpoint_lock) = claim_browser_cli_endpoint(socket_path)? else {
        tracing::info!(
            path = %socket_path.display(),
            "Another Ferryx instance serves the browser CLI socket; leaving it untouched"
        );
        return Ok(BrowserCliServerStart::ServedByPeer);
    };
    match fs::symlink_metadata(socket_path) {
        Ok(metadata) if metadata.file_type().is_socket() => fs::remove_file(socket_path)
            .map_err(|error| BrowserError::Internal(error.to_string()))?,
        Ok(_) => {
            return Err(BrowserError::Internal(
                "browser CLI socket path is not a socket".into(),
            ));
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(BrowserError::Internal(error.to_string())),
    }
    let listener = UnixListener::bind(socket_path)
        .map_err(|error| BrowserError::Internal(error.to_string()))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| BrowserError::Internal(error.to_string()))?;
    fs::set_permissions(socket_path, fs::Permissions::from_mode(0o600))
        .map_err(|error| BrowserError::Internal(error.to_string()))?;
    let token = Arc::new(generate_browser_cli_token());
    write_token_file(&token_path_for(socket_path), token.as_str())?;

    tauri::async_runtime::spawn(async move {
        // Held for as long as this listener serves: releasing it is what allows a later
        // start to replace the endpoint.
        let _endpoint_lock = endpoint_lock;
        let listener = match tokio::net::UnixListener::from_std(listener) {
            Ok(listener) => listener,
            Err(error) => {
                tracing::error!("Failed to register browser CLI socket with Tokio: {error}");
                return;
            }
        };
        let listener = &listener;
        serve_accept_loop(
            move || async move { listener.accept().await.map(|(stream, _)| stream) },
            move |stream| {
                let app = app.clone();
                let manager = Arc::clone(&manager);
                let token = Arc::clone(&token);
                tauri::async_runtime::spawn(async move {
                    log_connection_result(handle_connection(stream, app, manager, token).await);
                });
            },
        )
        .await;
    });
    Ok(BrowserCliServerStart::Serving)
}

#[cfg(not(unix))]
pub fn start_browser_cli_server<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: Arc<BrowserManager>,
) -> Result<BrowserCliServerStart, BrowserError> {
    start_browser_cli_server_at_path(app, manager, &browser_cli_socket_path())
}

#[cfg(not(unix))]
fn start_browser_cli_server_at_path<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: Arc<BrowserManager>,
    port_path: &Path,
) -> Result<BrowserCliServerStart, BrowserError> {
    use std::net::TcpListener;

    let runtime_dir = port_path
        .parent()
        .ok_or_else(|| BrowserError::Internal("browser CLI socket path has no parent".into()))?;
    fs::create_dir_all(&runtime_dir).map_err(|error| BrowserError::Internal(error.to_string()))?;
    let Some(endpoint_lock) = claim_browser_cli_endpoint(port_path)? else {
        tracing::info!(
            path = %port_path.display(),
            "Another Ferryx instance serves the browser CLI port; leaving it untouched"
        );
        return Ok(BrowserCliServerStart::ServedByPeer);
    };

    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|error| BrowserError::Internal(format!("Failed to bind TCP listener: {error}")))?;
    let port = listener
        .local_addr()
        .map_err(|error| BrowserError::Internal(format!("Failed to get local port: {error}")))?
        .port();

    listener
        .set_nonblocking(true)
        .map_err(|error| BrowserError::Internal(error.to_string()))?;

    write_port_file(port_path, port)?;
    let token = Arc::new(generate_browser_cli_token());
    write_token_file(&token_path_for(port_path), token.as_str())?;

    tauri::async_runtime::spawn(async move {
        // Held for as long as this listener serves: releasing it is what allows a later
        // start to replace the endpoint.
        let _endpoint_lock = endpoint_lock;
        let listener = match tokio::net::TcpListener::from_std(listener) {
            Ok(listener) => listener,
            Err(error) => {
                tracing::error!("Failed to register browser CLI TCP listener with Tokio: {error}");
                return;
            }
        };
        let listener = &listener;
        serve_accept_loop(
            move || async move { listener.accept().await.map(|(stream, _)| stream) },
            move |stream| {
                let app = app.clone();
                let manager = Arc::clone(&manager);
                let token = Arc::clone(&token);
                tauri::async_runtime::spawn(async move {
                    log_connection_result(handle_connection(stream, app, manager, token).await);
                });
            },
        )
        .await;
    });
    Ok(BrowserCliServerStart::Serving)
}

/// Pause after a failed `accept` before retrying. Errors such as EMFILE or ECONNABORTED are
/// transient, so the listener keeps serving instead of ending for good.
#[cfg(feature = "desktop")]
const ACCEPT_ERROR_BACKOFF: std::time::Duration = std::time::Duration::from_millis(100);

/// Accepts connections until the task is dropped. A failed accept is logged and retried
/// after a short pause, never ending the listener.
#[cfg(feature = "desktop")]
async fn serve_accept_loop<S, F, Fut>(mut accept: F, mut on_connection: impl FnMut(S))
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = std::io::Result<S>>,
{
    loop {
        match accept().await {
            Ok(stream) => on_connection(stream),
            Err(error) => accept_error_backoff(&error).await,
        }
    }
}

#[cfg(feature = "desktop")]
async fn accept_error_backoff(error: &std::io::Error) {
    tracing::warn!(%error, "browser CLI accept failed; retrying");
    tokio::time::sleep(ACCEPT_ERROR_BACKOFF).await;
}

#[cfg(feature = "desktop")]
fn log_connection_result(result: Result<(), BrowserError>) {
    if let Err(error) = result {
        tracing::debug!(%error, "browser CLI connection ended with an error");
    }
}

/// Upper bound for a single CLI request line (1 MiB). A connection whose line
/// exceeds this is rejected with `BROWSER_CLI_REQUEST_TOO_LARGE` instead of
/// being buffered without bound.
const MAX_REQUEST_BYTES: usize = 1024 * 1024;

/// How long the server waits for a connection's request line before closing it, so idle
/// peers cannot pin connection tasks and buffers.
#[cfg(feature = "desktop")]
const REQUEST_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Upper bound for one CLI response line read by the client.
const MAX_RESPONSE_BYTES: usize = 32 * 1024 * 1024;

/// How long the client waits for the response. It exceeds the longest bounded server-side
/// command (a wait, capped at 120s).
const RESPONSE_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(150);

enum RequestLine {
    Eof,
    Line(String),
    TooLarge,
}

async fn read_limited_line<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
    max_bytes: usize,
) -> Result<RequestLine, std::io::Error> {
    use tokio::io::AsyncBufReadExt;

    let mut line = Vec::new();
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return Ok(if line.is_empty() {
                RequestLine::Eof
            } else {
                RequestLine::Line(String::from_utf8_lossy(&line).into_owned())
            });
        }
        match available.iter().position(|&byte| byte == b'\n') {
            Some(newline_index) => {
                if line.len() + newline_index + 1 > max_bytes {
                    return Ok(RequestLine::TooLarge);
                }
                line.extend_from_slice(&available[..=newline_index]);
                reader.consume(newline_index + 1);
                return Ok(RequestLine::Line(
                    String::from_utf8_lossy(&line).into_owned(),
                ));
            }
            None => {
                if line.len() + available.len() > max_bytes {
                    return Ok(RequestLine::TooLarge);
                }
                let chunk_len = available.len();
                line.extend_from_slice(available);
                reader.consume(chunk_len);
            }
        }
    }
}

/// Maps a typed remote browser executor error onto the public IPC error.
///
/// Classification is by variant only: message text can carry page-controlled
/// strings and must never choose the IPC code.
fn remote_browser_error_to_ipc(e: crate::remote::browser_backend::RemoteBrowserError) -> IpcError {
    use crate::remote::browser_backend::RemoteBrowserError;
    match e {
        RemoteBrowserError::TargetNotFound(msg) => {
            IpcError::new(IpcErrorCode::Custom("BROWSER_TARGET_NOT_FOUND".into()), msg)
        }
        RemoteBrowserError::StaleFrame(msg) => {
            IpcError::new(IpcErrorCode::Custom("BROWSER_STALE_FRAME".into()), msg)
        }
        RemoteBrowserError::Forbidden(msg) => {
            IpcError::new(IpcErrorCode::Custom("BROWSER_FORBIDDEN".into()), msg)
        }
        RemoteBrowserError::NotFound(msg) => {
            IpcError::new(IpcErrorCode::Custom("BROWSER_NOT_FOUND".into()), msg)
        }
        RemoteBrowserError::InvalidRequest(msg) => {
            IpcError::new(IpcErrorCode::Custom("BROWSER_INVALID_REQUEST".into()), msg)
        }
        RemoteBrowserError::WaitTimeout => {
            IpcError::new(IpcErrorCode::BrowserWaitTimeout, "wait timeout expired")
        }
        RemoteBrowserError::Unavailable(msg) => {
            IpcError::new(IpcErrorCode::Custom("BROWSER_UNAVAILABLE".into()), msg)
        }
        RemoteBrowserError::ExecutionFailed(msg) => {
            IpcError::new(IpcErrorCode::Custom("BROWSER_EXECUTION_FAILED".into()), msg)
        }
        RemoteBrowserError::InputRefused(explanation) => {
            IpcError::new(IpcErrorCode::Unsupported, explanation.to_string())
                .with_details(explanation.to_details())
        }
        RemoteBrowserError::DriverBusy(msg) => {
            IpcError::new(IpcErrorCode::Custom("BROWSER_DRIVER_BUSY".into()), msg)
        }
        RemoteBrowserError::RateLimited(msg) => IpcError::new(IpcErrorCode::RateLimited, msg),
    }
}

fn ipc_error_code_string(code: IpcErrorCode) -> String {
    serde_json::to_value(&code)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| format!("{code:?}"))
}

#[cfg(feature = "desktop")]
async fn handle_connection<S, R: tauri::Runtime>(
    stream: S,
    app: AppHandle<R>,
    manager: Arc<BrowserManager>,
    expected_token: Arc<String>,
) -> Result<(), BrowserError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::AsyncWriteExt;

    let unauthorized = || BrowserCliResponse::Error {
        code: BROWSER_CLI_UNAUTHORIZED.into(),
        details: None,
        message: "browser CLI requires the capability token of the running Ferryx app".into(),
    };

    let (reader, mut writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(reader);
    let mut is_remote_attach = false;
    let request_line = match tokio::time::timeout(
        REQUEST_READ_TIMEOUT,
        read_limited_line(&mut reader, MAX_REQUEST_BYTES),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => {
            return Err(BrowserError::Internal(
                "browser CLI request line not received in time".into(),
            ))
        }
    };
    let response = match request_line {
        Ok(RequestLine::Eof) => return Ok(()),
        Ok(RequestLine::TooLarge) => BrowserCliResponse::Error {
            code: "BROWSER_CLI_REQUEST_TOO_LARGE".into(),
            details: None,
            message: format!(
                "request exceeds the maximum of {MAX_REQUEST_BYTES} bytes per connection"
            ),
        },
        Ok(RequestLine::Line(line)) => {
            // Authorization is decided before the command is interpreted, so an
            // unauthorized peer learns nothing about which commands exist or
            // whether its arguments named a real browser session.
            match serde_json::from_str::<BrowserCliEnvelope>(line.trim()) {
                Ok(envelope) if tokens_match(expected_token.as_str(), &envelope.token) => {
                    if matches!(envelope.request, BrowserCliRequest::RemoteAttach { .. }) {
                        is_remote_attach = true;
                    }
                    execute_request(&app, &manager, envelope.request).await
                }
                Ok(_) => unauthorized(),
                Err(error) => {
                    // Only a caller holding the token learns why its request is malformed.
                    let authorized = serde_json::from_str::<serde_json::Value>(line.trim())
                        .ok()
                        .and_then(|value| {
                            value
                                .get("token")
                                .and_then(|token| token.as_str())
                                .map(|token| tokens_match(expected_token.as_str(), token))
                        })
                        .unwrap_or(false);
                    if !authorized {
                        unauthorized()
                    } else {
                        BrowserCliResponse::Error {
                            code: "BROWSER_CLI_REQUEST_INVALID".into(),
                            details: None,
                            message: error.to_string(),
                        }
                    }
                }
            }
        }
        Err(error) => return Err(BrowserError::Internal(error.to_string())),
    };
    let mut response_str = serde_json::to_string(&response)
        .map_err(|error| BrowserError::Internal(error.to_string()))?;
    response_str.push('\n');
    writer
        .write_all(response_str.as_bytes())
        .await
        .map_err(|error| BrowserError::Internal(error.to_string()))?;
    writer
        .flush()
        .await
        .map_err(|error| BrowserError::Internal(error.to_string()))?;

    if is_remote_attach && matches!(response, BrowserCliResponse::RemoteAttached { .. }) {
        return run_framed_ipc_loop(&mut reader, &mut writer, &app, &manager).await;
    }

    Ok(())
}

#[cfg(feature = "desktop")]
async fn run_framed_ipc_loop<R, W, Rt: tauri::Runtime>(
    reader: &mut R,
    writer: &mut W,
    app: &AppHandle<Rt>,
    manager: &Arc<BrowserManager>,
) -> Result<(), BrowserError>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut header = [0u8; 5];
    loop {
        match reader.read_exact(&mut header).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Ok(());
            }
            Err(e) => return Err(BrowserError::Internal(e.to_string())),
        }

        let payload_len = u32::from_le_bytes([header[0], header[1], header[2], header[3]]) as usize;
        let content_type = header[4];

        let max_len = if content_type
            == crate::browser::remote_bridge_protocol::IPC_CONTENT_TYPE_JSON
        {
            crate::browser::remote_bridge_protocol::MAX_JSON_PAYLOAD_BYTES
        } else if content_type == crate::browser::remote_bridge_protocol::IPC_CONTENT_TYPE_IMAGE {
            crate::browser::remote_bridge_protocol::MAX_FRAME_PAYLOAD_BYTES
        } else {
            return Err(BrowserError::Internal(format!(
                "Invalid IPC content type: 0x{:02x}",
                content_type
            )));
        };

        if payload_len > max_len {
            return Err(BrowserError::Internal(format!(
                "Framed IPC payload too large: {} bytes (max {})",
                payload_len, max_len
            )));
        }

        let mut payload = vec![0u8; payload_len];
        if payload_len > 0 {
            reader
                .read_exact(&mut payload)
                .await
                .map_err(|e| BrowserError::Internal(e.to_string()))?;
        }

        if content_type == crate::browser::remote_bridge_protocol::IPC_CONTENT_TYPE_JSON {
            let parsed_op = serde_json::from_slice::<RemoteBrowserOperation>(&payload);
            let subscribed_browser_id = match &parsed_op {
                Ok(RemoteBrowserOperation::SubscribeViewer { browser_id, .. }) => {
                    Some(browser_id.clone())
                }
                _ => None,
            };

            let resp_payload = match parsed_op {
                Ok(op) => match execute_remote_operation(app, manager, op).await {
                    Ok(val) => {
                        let resp = serde_json::json!({
                            "type": "remoteResult",
                            "status": "ok",
                            "result": val,
                        });
                        serde_json::to_vec(&resp).unwrap_or_default()
                    }
                    Err(e) => {
                        let resp = framed_error_response(e);
                        serde_json::to_vec(&resp).unwrap_or_default()
                    }
                },
                Err(e) => {
                    let resp = BrowserCliResponse::Error {
                        code: "BROWSER_CLI_REQUEST_INVALID".into(),
                        details: None,
                        message: format!("invalid remote operation: {e}"),
                    };
                    serde_json::to_vec(&resp).unwrap_or_default()
                }
            };

            let frame = crate::browser::remote_bridge_protocol::encode_ipc_frame(
                crate::browser::remote_bridge_protocol::IPC_CONTENT_TYPE_JSON,
                &resp_payload,
            )
            .map_err(|e| BrowserError::Internal(e.to_string()))?;

            // R6-1: a successful SubscribeViewer turns this connection into a frame stream.
            // The frame receiver is registered before the response is written, so a client
            // that has read the response is guaranteed to get every later frame.
            let stream_target = subscribed_browser_id.and_then(|b_id| {
                let sub_id = serde_json::from_slice::<serde_json::Value>(&resp_payload)
                    .ok()?
                    .get("result")?
                    .get("subscriptionId")?
                    .as_str()?
                    .to_string();
                let service = app
                    .try_state::<Arc<crate::browser::remote_service::BrowserRemoteService>>()
                    .map(|s| Arc::clone(&s))
                    .or_else(|| {
                        app.try_state::<Arc<
                            crate::remote::browser_backend::InProcessBrowserServiceBackend,
                        >>()
                        .map(|b| Arc::clone(&b.remote_service))
                    })?;
                let frame_rx = service.subscribe_frames(&b_id);
                Some((service, b_id, sub_id, frame_rx))
            });

            let written = match writer.write_all(&frame).await {
                Ok(()) => writer.flush().await,
                Err(error) => Err(error),
            };
            if let Err(e) = written {
                // The client is gone: a subscription made for it must not outlive it.
                if let Some((service, b_id, sub_id, _)) = stream_target {
                    service.unsubscribe(&b_id, &sub_id);
                }
                return Err(BrowserError::Internal(e.to_string()));
            }

            if let Some((service, b_id, sub_id, mut frame_rx)) = stream_target {
                // While streaming, any inbound byte or EOF from the client ends the stream.
                // A one-byte read is cancel-safe and never allocates from a client length.
                let mut probe = [0u8; 1];
                loop {
                    tokio::select! {
                        frame_res = frame_rx.recv() => {
                            match frame_res {
                                Ok(frame_data) => {
                                    if let Ok(img_frame) = crate::browser::remote_bridge_protocol::encode_ipc_frame(
                                        crate::browser::remote_bridge_protocol::IPC_CONTENT_TYPE_IMAGE,
                                        &frame_data,
                                    ) {
                                        if writer.write_all(&img_frame).await.is_err() || writer.flush().await.is_err() {
                                            break;
                                        }
                                    }
                                }
                                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                            }
                        }
                        _ = reader.read(&mut probe) => break,
                    }
                }
                service.unsubscribe(&b_id, &sub_id);
                return Ok(());
            }
        }
    }
}

/// CLI reload, matching `cmd_browser_reload` without a hard reload: the GUI toolbar is told
/// about the loading state, and a failed reload clears `loading` and records the error
/// instead of leaving the session stuck. The webview is resolved first, so a missing one
/// leaves the session untouched.
#[cfg(feature = "desktop")]
fn reload_browser_session<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &Arc<BrowserManager>,
    browser_id: &str,
) -> Result<(), IpcError> {
    let emit_state = |state: &crate::browser::BrowserState| {
        let _ = app.emit(
            crate::ipc::browser::BROWSER_STATE_CHANGED_EVENT,
            crate::browser::BrowserStateChangedPayload::from(state),
        );
    };
    let label = manager.get_webview_label(browser_id)?;
    let webview = app
        .get_webview(&label)
        .ok_or_else(|| BrowserError::WebviewNotFound(label.clone()))?;
    let state = manager.begin_reload(browser_id)?;
    emit_state(&state);
    if let Err(error) = webview.reload() {
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
            emit_state(&error_state);
        }
        return Err(BrowserError::NavigationFailed(message).into());
    }
    Ok(())
}

#[cfg(feature = "desktop")]
async fn execute_request<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &Arc<BrowserManager>,
    request: BrowserCliRequest,
) -> BrowserCliResponse {
    match request {
        BrowserCliRequest::List => BrowserCliResponse::List {
            sessions: manager.list_sessions(),
        },
        BrowserCliRequest::Snapshot { browser_id } => {
            match browser_automation_snapshot(app.clone(), manager, browser_id).await {
                Ok(snapshot) => BrowserCliResponse::Snapshot { snapshot },
                Err(error) => error_response(error),
            }
        }
        BrowserCliRequest::Act { request } => {
            match browser_automation_act(app.clone(), manager, request).await {
                Ok(()) => BrowserCliResponse::Acted,
                Err(error) => error_response(error),
            }
        }
        BrowserCliRequest::Open {
            url,
            workspace_id,
            worktree_path,
        } => {
            if let Err(browser_error) = crate::browser::validate_url(&url) {
                let ipc_error = IpcError::from(browser_error);
                return error_response(ipc_error);
            }
            let create_req = CreateBrowserRequest {
                browser_id: None,
                workspace_id: workspace_id.clone(),
                worktree_path,
                url,
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            };
            match create_browser_session(app, manager, create_req).await {
                Ok(state) => {
                    let payload = BrowserSessionCreatedPayload {
                        browser: state.clone(),
                        workspace_id,
                    };
                    // The waiter must exist before the GUI can answer this event.
                    let adoption_rx = crate::browser::adoption::expect(&state.browser_id);
                    if let Err(error) = app.emit(
                        crate::browser::guest::BROWSER_SESSION_CREATED_EVENT,
                        payload,
                    ) {
                        crate::browser::adoption::forget(&state.browser_id);
                        // The session exists but no tab will own it: do not leave it behind.
                        if let Err(close_error) =
                            close_browser_session(app, manager, &state.browser_id).await
                        {
                            tracing::warn!(browser_id = %state.browser_id, error = %close_error.message, "could not close an undelivered browser session");
                        }
                        return BrowserCliResponse::Error {
                            code: ipc_error_code_string(IpcErrorCode::BrowserHostUnavailable),
                            details: None,
                            message: format!(
                                "failed to deliver the browser session to the GUI: {error}"
                            ),
                        };
                    }
                    let adoption =
                        crate::browser::adoption::await_ack(&state.browser_id, adoption_rx).await;
                    BrowserCliResponse::Opened {
                        browser: BrowserSessionSummary::from(state),
                        adoption,
                    }
                }
                Err(error) => error_response(error),
            }
        }
        BrowserCliRequest::Navigate { browser_id, url } => {
            if let Err(browser_error) = crate::browser::validate_url(&url) {
                let ipc_error = IpcError::from(browser_error);
                return error_response(ipc_error);
            }
            match navigate_browser_session(app, manager, &browser_id, &url).await {
                Ok(()) => BrowserCliResponse::Navigated,
                Err(error) => error_response(error),
            }
        }
        BrowserCliRequest::Back { browser_id } => {
            match crate::ipc::browser::history_navigation(app, manager, &browser_id, false) {
                Ok(()) => BrowserCliResponse::Navigated,
                Err(error) => error_response(error),
            }
        }
        BrowserCliRequest::Forward { browser_id } => {
            match crate::ipc::browser::history_navigation(app, manager, &browser_id, true) {
                Ok(()) => BrowserCliResponse::Navigated,
                Err(error) => error_response(error),
            }
        }
        BrowserCliRequest::Reload { browser_id } => {
            match reload_browser_session(app, manager, &browser_id) {
                Ok(()) => BrowserCliResponse::Navigated,
                Err(error) => error_response(error),
            }
        }
        BrowserCliRequest::Close { browser_id } => {
            match close_browser_session(app, manager, &browser_id).await {
                Ok(()) => BrowserCliResponse::Closed,
                Err(error) => error_response(error),
            }
        }
        BrowserCliRequest::Identify => BrowserCliResponse::Identified {
            browser: identify_browser_session(manager),
        },
        BrowserCliRequest::Eval { browser_id, script } => {
            match crate::ipc::browser::eval_browser_session(app, manager, &browser_id, &script)
                .await
            {
                Ok((result, truncated)) => BrowserCliResponse::Evaluated { result, truncated },
                Err(error) => error_response(error),
            }
        }
        BrowserCliRequest::Wait {
            browser_id,
            condition,
        } => {
            match crate::ipc::browser::wait_browser_session(
                app,
                manager,
                &browser_id,
                clamp_cli_wait_condition(condition),
            )
            .await
            {
                Ok(()) => BrowserCliResponse::Waited,
                Err(error) => error_response(error),
            }
        }
        BrowserCliRequest::Console {
            browser_id,
            errors_only,
            clear,
        } => {
            match crate::ipc::browser::console_browser_session(
                app,
                manager,
                &browser_id,
                errors_only.unwrap_or(false),
                clear.unwrap_or(false),
            )
            .await
            {
                Ok(entries) => BrowserCliResponse::ConsoleEntries { entries },
                Err(error) => error_response(error),
            }
        }
        BrowserCliRequest::Focus { browser_id } => {
            match focus_browser_session(app, manager, &browser_id) {
                Ok(()) => BrowserCliResponse::Focused,
                Err(error) => error_response(error),
            }
        }
        BrowserCliRequest::Screenshot {
            browser_id,
            out_path,
        } => {
            match crate::ipc::browser::screenshot_browser_session(
                app,
                manager,
                &browser_id,
                &out_path,
            )
            .await
            {
                Ok(path) => BrowserCliResponse::ScreenshotSaved { path },
                Err(error) => error_response(error),
            }
        }
        BrowserCliRequest::Cookies {
            browser_id,
            action,
            name,
            value,
            domain,
            path,
        } => {
            match crate::ipc::browser::cookies_browser_session(
                app,
                manager,
                &browser_id,
                crate::ipc::browser::BrowserCookieParams {
                    action: &action,
                    name: name.as_deref(),
                    value: value.as_deref(),
                    domain: domain.as_deref(),
                    path: path.as_deref(),
                },
            )
            .await
            {
                Ok(cookies) => BrowserCliResponse::CookieEntries { cookies },
                Err(error) => error_response(error),
            }
        }
        BrowserCliRequest::Storage {
            browser_id,
            kind,
            action,
            key,
            value,
            clear_all,
            url,
            domain,
        } => {
            match crate::ipc::browser::storage_browser_session_extended(
                app,
                manager,
                &browser_id,
                crate::ipc::browser::BrowserStorageParams {
                    kind: &kind,
                    action: &action,
                    key: key.as_deref(),
                    value: value.as_deref(),
                    clear_all: clear_all.unwrap_or(false),
                    url: url.as_deref(),
                    domain: domain.as_deref(),
                },
            )
            .await
            {
                Ok(value) => BrowserCliResponse::StorageValue { value },
                Err(error) => error_response(error),
            }
        }
        BrowserCliRequest::TabList => BrowserCliResponse::List {
            sessions: manager.list_sessions(),
        },
        BrowserCliRequest::TabNew {
            url,
            workspace_id,
            worktree_path,
        } => {
            let target_url = url.unwrap_or_else(|| "http://localhost:3000".to_string());
            if let Err(browser_error) = crate::browser::validate_url(&target_url) {
                let ipc_error = IpcError::from(browser_error);
                return error_response(ipc_error);
            }
            let create_req = CreateBrowserRequest {
                browser_id: None,
                workspace_id: workspace_id.clone(),
                worktree_path,
                url: target_url,
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            };
            match create_browser_session(app, manager, create_req).await {
                Ok(state) => {
                    let payload = BrowserSessionCreatedPayload {
                        browser: state.clone(),
                        workspace_id,
                    };
                    // The waiter must exist before the GUI can answer this event.
                    let adoption_rx = crate::browser::adoption::expect(&state.browser_id);
                    if let Err(error) = app.emit(
                        crate::browser::guest::BROWSER_SESSION_CREATED_EVENT,
                        payload,
                    ) {
                        crate::browser::adoption::forget(&state.browser_id);
                        // The session exists but no tab will own it: do not leave it behind.
                        if let Err(close_error) =
                            close_browser_session(app, manager, &state.browser_id).await
                        {
                            tracing::warn!(browser_id = %state.browser_id, error = %close_error.message, "could not close an undelivered browser session");
                        }
                        return BrowserCliResponse::Error {
                            code: ipc_error_code_string(IpcErrorCode::BrowserHostUnavailable),
                            details: None,
                            message: format!(
                                "failed to deliver the browser session to the GUI: {error}"
                            ),
                        };
                    }
                    let adoption =
                        crate::browser::adoption::await_ack(&state.browser_id, adoption_rx).await;
                    BrowserCliResponse::Opened {
                        browser: BrowserSessionSummary::from(state),
                        adoption,
                    }
                }
                Err(error) => error_response(error),
            }
        }
        BrowserCliRequest::TabSwitch { browser_id, index } => {
            let sessions = manager.list_sessions();
            let target_session = if let Some(idx) = index {
                if idx == 0 || idx > sessions.len() {
                    return BrowserCliResponse::Error {
                        code: "BROWSER_TAB_OUT_OF_RANGE".into(),
                        details: None,
                        message: format!(
                            "tab index {idx} out of range (total tabs: {})",
                            sessions.len()
                        ),
                    };
                }
                Some((sessions[idx - 1].clone(), idx))
            } else if let Some(ref id) = browser_id {
                sessions
                    .iter()
                    .enumerate()
                    .find(|(_, s)| &s.browser_id == id)
                    .map(|(i, s)| (s.clone(), i + 1))
            } else {
                None
            };

            let (session, final_index) = match target_session {
                Some(pair) => pair,
                None => {
                    let identifier = browser_id
                        .or_else(|| index.map(|i| i.to_string()))
                        .unwrap_or_else(|| "unspecified".into());
                    return BrowserCliResponse::Error {
                        code: "BROWSER_NOT_FOUND".into(),
                        details: None,
                        message: format!("browser tab '{identifier}' not found"),
                    };
                }
            };

            if let Err(error) = focus_browser_session(app, manager, &session.browser_id) {
                return error_response(error);
            }

            // The layout tab index space differs from this CLI list, so the GUI resolves the
            // owning tab from browserId alone; a shortcut index here would activate the wrong tab.
            if let Err(error) = app.emit(
                "browser_tab_switch",
                serde_json::json!({
                    "browserId": session.browser_id,
                    "index": final_index
                }),
            ) {
                return BrowserCliResponse::Error {
                    code: ipc_error_code_string(IpcErrorCode::BrowserHostUnavailable),
                    details: None,
                    message: format!("failed to ask the GUI to activate the tab: {error}"),
                };
            }

            BrowserCliResponse::TabSwitched {
                browser: session,
                index: final_index,
            }
        }
        BrowserCliRequest::TabClose { browser_id, index } => {
            let sessions = manager.list_sessions();
            let target_id = if let Some(idx) = index {
                if idx == 0 || idx > sessions.len() {
                    return BrowserCliResponse::Error {
                        code: "BROWSER_TAB_OUT_OF_RANGE".into(),
                        details: None,
                        message: format!(
                            "tab index {idx} out of range (total tabs: {})",
                            sessions.len()
                        ),
                    };
                }
                sessions[idx - 1].browser_id.clone()
            } else if let Some(id) = browser_id {
                if !sessions.iter().any(|s| s.browser_id == id) {
                    return BrowserCliResponse::Error {
                        code: "BROWSER_NOT_FOUND".into(),
                        details: None,
                        message: format!("browser tab '{id}' not found"),
                    };
                }
                id
            } else {
                match identify_browser_session(manager) {
                    Some(s) => s.browser_id,
                    None => {
                        return BrowserCliResponse::Error {
                            code: "BROWSER_NOT_FOUND".into(),
                            details: None,
                            message: "no active browser tab to close".into(),
                        };
                    }
                }
            };

            match close_browser_session(app, manager, &target_id).await {
                Ok(()) => BrowserCliResponse::Closed,
                Err(error) => error_response(error),
            }
        }
        BrowserCliRequest::StateSave {
            browser_id,
            out_path,
        } => {
            match crate::ipc::browser::save_browser_state_session(
                app,
                manager,
                &browser_id,
                &out_path,
            )
            .await
            {
                Ok(path) => BrowserCliResponse::StateSaved { path },
                Err(error) => error_response(error),
            }
        }
        BrowserCliRequest::StateLoad {
            browser_id,
            in_path,
        } => {
            match crate::ipc::browser::load_browser_state_session(
                app,
                manager,
                &browser_id,
                &in_path,
            )
            .await
            {
                Ok(()) => BrowserCliResponse::StateLoaded,
                Err(error) => error_response(error),
            }
        }
        BrowserCliRequest::Dialog {
            browser_id,
            action,
            prompt_text,
            policy,
        } => match action.as_str() {
            "accept" | "dismiss" => {
                let request = BrowserDialogHandleRequest {
                    browser_id: browser_id.clone(),
                    action: action.clone(),
                    prompt_text,
                };
                // With no pending dialog the webview reports `no_active_dialog`,
                // which `browser_handle_dialog` maps to the structured
                // BROWSER_DIALOG_NOT_FOUND code instead of faking a success.
                match browser_handle_dialog(app, manager, request).await {
                    Ok(dialog) => BrowserCliResponse::DialogHandled { dialog },
                    Err(error) => error_response(error),
                }
            }
            "list" => match browser_get_dialogs(app, manager, &browser_id).await {
                Ok(dialogs) => BrowserCliResponse::DialogEntries { dialogs },
                Err(error) => error_response(error),
            },
            "policy" => match policy {
                Some(policy) => {
                    match browser_set_dialog_policy(app, manager, &browser_id, &policy).await {
                        Ok(()) => BrowserCliResponse::DialogPolicySet { policy },
                        Err(error) => error_response(error),
                    }
                }
                None => BrowserCliResponse::Error {
                    code: "BROWSER_CLI_REQUEST_INVALID".into(),
                    details: None,
                    message: "dialog policy requires a policy value".into(),
                },
            },
            other => BrowserCliResponse::Error {
                code: "BROWSER_CLI_REQUEST_INVALID".into(),
                details: None,
                message: format!(
                    "unknown dialog action `{other}`: expected accept, dismiss, list, or policy"
                ),
            },
        },
        BrowserCliRequest::Download {
            browser_id,
            url,
            path,
            id,
            action,
        } => match action.as_str() {
            "start" => match (browser_id, url, path) {
                (Some(browser_id), Some(url), Some(path)) => {
                    let request = BrowserDownloadRequest {
                        url,
                        file_path: path,
                        browser_id: Some(browser_id),
                        cookies: None,
                    };
                    match browser_download_with_session(app, manager, request).await {
                        Ok(download) => BrowserCliResponse::DownloadStarted { download },
                        Err(error) => error_response(error),
                    }
                }
                _ => BrowserCliResponse::Error {
                    code: "BROWSER_CLI_REQUEST_INVALID".into(),
                    details: None,
                    message: "download start requires browserId, url, and path".into(),
                },
            },
            "list" => BrowserCliResponse::Downloads {
                downloads: crate::browser::download::list_downloads(),
            },
            "cancel" => match id {
                Some(id) => {
                    let cancelled = crate::browser::download::cancel_download(&id);
                    BrowserCliResponse::DownloadCancelled { id, cancelled }
                }
                None => BrowserCliResponse::Error {
                    code: "BROWSER_CLI_REQUEST_INVALID".into(),
                    details: None,
                    message: "download cancel requires an id".into(),
                },
            },
            other => BrowserCliResponse::Error {
                code: "BROWSER_CLI_REQUEST_INVALID".into(),
                details: None,
                message: format!(
                    "unknown download action `{other}`: expected start, list, or cancel"
                ),
            },
        },
        BrowserCliRequest::Highlight {
            browser_id,
            selector,
            duration_ms,
        } => {
            let result = match duration_ms {
                Some(duration_ms) => {
                    highlight_browser_session_with_duration(
                        app,
                        manager,
                        &browser_id,
                        &selector,
                        duration_ms,
                    )
                    .await
                }
                None => highlight_browser_session(app, manager, &browser_id, &selector).await,
            };
            match result {
                Ok(highlight) => BrowserCliResponse::Highlighted { highlight },
                Err(error) => error_response(error),
            }
        }
        BrowserCliRequest::RemoteAttach { protocol_version } => {
            BrowserCliResponse::RemoteAttached {
                service_epoch: "1".into(),
                protocol_version: protocol_version.unwrap_or(1),
            }
        }
        BrowserCliRequest::OpenFile { path, line, col } => {
            let path_clone = path.clone();
            let validation = crate::ipc::run_blocking(move || {
                let p = Path::new(&path_clone);
                if !p.is_absolute() {
                    return Err(IpcError::new(
                        IpcErrorCode::InvalidArgument,
                        "open file requires an absolute path",
                    ));
                }
                match fs::metadata(p) {
                    Ok(meta) if meta.is_file() => Ok(()),
                    _ => Err(IpcError::new(IpcErrorCode::NotFound, "file not found")),
                }
            })
            .await;

            if let Err(error) = validation {
                return error_response(error);
            }

            let payload = CliOpenFilePayload {
                path: path.clone(),
                line,
                col,
            };

            if let Err(error) = app.emit(CLI_OPEN_FILE_EVENT, payload) {
                return BrowserCliResponse::Error {
                    code: ipc_error_code_string(IpcErrorCode::BrowserHostUnavailable),
                    details: None,
                    message: format!("failed to deliver open file event to the GUI: {error}"),
                };
            }

            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }

            BrowserCliResponse::FileOpened { path }
        }
    }
}

async fn send_over_stream<S>(
    stream: S,
    request: BrowserCliRequest,
    token: String,
) -> Result<BrowserCliResponse, BrowserError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::AsyncWriteExt;

    let (reader, mut writer) = tokio::io::split(stream);
    let mut request_json = serde_json::to_string(&BrowserCliEnvelope { token, request })
        .map_err(|error| BrowserError::AutomationFailed(error.to_string()))?;
    request_json.push('\n');
    writer
        .write_all(request_json.as_bytes())
        .await
        .map_err(|error| BrowserError::AutomationFailed(error.to_string()))?;
    writer
        .flush()
        .await
        .map_err(|error| BrowserError::AutomationFailed(error.to_string()))?;
    let mut reader = BufReader::new(reader);
    let response = match tokio::time::timeout(
        RESPONSE_READ_TIMEOUT,
        read_limited_line(&mut reader, MAX_RESPONSE_BYTES),
    )
    .await
    {
        Err(_) => {
            return Err(BrowserError::AutomationFailed(
                "timed out waiting for the Ferryx desktop app to respond".into(),
            ))
        }
        Ok(Err(error)) => return Err(BrowserError::AutomationFailed(error.to_string())),
        Ok(Ok(RequestLine::TooLarge)) => {
            return Err(BrowserError::AutomationFailed(format!(
                "browser CLI response exceeds {MAX_RESPONSE_BYTES} bytes"
            )))
        }
        Ok(Ok(RequestLine::Eof)) => String::new(),
        Ok(Ok(RequestLine::Line(line))) => line,
    };
    serde_json::from_str(response.trim())
        .map_err(|error| BrowserError::AutomationFailed(error.to_string()))
}

#[cfg(unix)]
pub async fn send_browser_cli_request(
    request: BrowserCliRequest,
) -> Result<BrowserCliResponse, BrowserError> {
    send_browser_cli_request_at_path(request, &browser_cli_socket_path()).await
}

#[cfg(unix)]
async fn send_browser_cli_request_at_path(
    request: BrowserCliRequest,
    socket_path: &Path,
) -> Result<BrowserCliResponse, BrowserError> {
    use tokio::net::UnixStream;

    let path_buf = token_path_for(socket_path);
    let token = crate::ipc::run_blocking(move || {
        Ok::<_, IpcError>(read_token_file(&path_buf))
    })
    .await
    .map_err(|e| BrowserError::Internal(e.to_string()))??;
    let stream = UnixStream::connect(socket_path).await.map_err(|error| {
        BrowserError::CliUnavailable(format!("Ferryx desktop app is not running: {error}"))
    })?;
    send_over_stream(stream, request, token).await
}

#[cfg(not(unix))]
pub async fn send_browser_cli_request(
    request: BrowserCliRequest,
) -> Result<BrowserCliResponse, BrowserError> {
    send_browser_cli_request_at_path(request, &browser_cli_socket_path()).await
}

#[cfg(not(unix))]
async fn send_browser_cli_request_at_path(
    request: BrowserCliRequest,
    port_path: &Path,
) -> Result<BrowserCliResponse, BrowserError> {
    use tokio::net::TcpStream;

    let port_path_buf = port_path.to_path_buf();
    let port = crate::ipc::run_blocking(move || {
        Ok::<_, IpcError>(read_port_from_file(&port_path_buf))
    })
    .await
    .map_err(|e| BrowserError::Internal(e.to_string()))??;
    let token_path_buf = token_path_for(port_path);
    let token = crate::ipc::run_blocking(move || {
        Ok::<_, IpcError>(read_token_file(&token_path_buf))
    })
    .await
    .map_err(|e| BrowserError::Internal(e.to_string()))??;
    let stream = TcpStream::connect(format!("127.0.0.1:{port}"))
        .await
        .map_err(|error| {
            BrowserError::CliUnavailable(format!("Ferryx desktop app is not running: {error}"))
        })?;
    send_over_stream(stream, request, token).await
}

/// Maps a decoded click failure onto the IPC surface by variant only.
fn point_click_decode_error_to_ipc(err: crate::browser::remote_input::ActionDecodeError) -> IpcError {
    use crate::browser::remote_input::ActionDecodeError;
    match err {
        ActionDecodeError::Refused(input_err) => {
            IpcError::new(input_err.ipc_error_code(), input_err.to_string())
                .with_details(input_err.explanation().to_details())
        }
        ActionDecodeError::NotFound(message) => IpcError::new(
            IpcErrorCode::Custom("BROWSER_TARGET_NOT_FOUND".into()),
            message,
        ),
        ActionDecodeError::Failed(message) | ActionDecodeError::Malformed(message) => {
            IpcError::new(IpcErrorCode::BrowserAutomationFailed, message)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::{BrowserProfileId, BrowserSessionSummary, CreateBrowserRequest};

    #[test]
    fn remote_operation_keypress_dispatches_the_validated_trimmed_key() {
        let script = keypress_operation_script(" a ").expect("padded single char is allowed");
        assert!(
            script.contains(r#"key: "a","#),
            "must dispatch the trimmed key that was validated, got {script}"
        );
        assert!(!script.contains(r#"" a ""#), "raw padded key must not be dispatched");
        assert!(keypress_operation_script("\u{1b}").is_err());
    }

    #[test]
    fn selector_click_missing_target_maps_to_browser_target_not_found() {
        use crate::browser::remote_input::{build_selector_click_script, decode_point_click_result};
        let script = build_selector_click_script(r##""#btn""##);
        assert!(
            script.contains(r#"ok: false, code: "not_found""#),
            "selector-click script must tag a missing target with code not_found: {script}"
        );
        let missing = decode_point_click_result(
            r#"{"ok":false,"code":"not_found","error":"element not found"}"#,
        )
        .unwrap_err();
        assert_eq!(
            ipc_error_code_string(point_click_decode_error_to_ipc(missing).code),
            "BROWSER_TARGET_NOT_FOUND"
        );
        let untagged =
            decode_point_click_result(r#"{"ok":false,"error":"element not found"}"#).unwrap_err();
        assert_eq!(
            ipc_error_code_string(point_click_decode_error_to_ipc(untagged).code),
            ipc_error_code_string(IpcErrorCode::BrowserAutomationFailed)
        );
    }

    #[test]
    fn remote_execution_failed_page_text_is_not_reclassified() {
        use crate::remote::browser_backend::RemoteBrowserError;
        for text in [
            "element not found",
            "no element at coordinates",
            "no active element",
            "BROWSER_TARGET_NOT_FOUND: spoofed by page",
            "BROWSER_STALE_FRAME: viewport revision changed",
        ] {
            let err = remote_browser_error_to_ipc(RemoteBrowserError::ExecutionFailed(text.into()));
            assert_eq!(
                ipc_error_code_string(err.code),
                "BROWSER_EXECUTION_FAILED",
                "page-controlled text {text:?} must not choose the IPC code"
            );
        }
    }

    #[test]
    fn test_browser_cli_open_file_request_serialization() {
        let json = r#"{"command":"openFile","path":"/x","line":3}"#;
        let parsed: BrowserCliRequest = serde_json::from_str(json).unwrap();
        assert_eq!(
            parsed,
            BrowserCliRequest::OpenFile {
                path: "/x".into(),
                line: Some(3),
                col: None,
            }
        );
        let serialized = serde_json::to_string(&parsed).unwrap();
        let roundtrip: BrowserCliRequest = serde_json::from_str(&serialized).unwrap();
        assert_eq!(roundtrip, parsed);

        let response = BrowserCliResponse::FileOpened { path: "/x".into() };
        let resp_json = serde_json::to_string(&response).unwrap();
        assert_eq!(resp_json, r#"{"type":"fileOpened","path":"/x"}"#);
        assert_eq!(
            serde_json::from_str::<BrowserCliResponse>(&resp_json).unwrap(),
            response
        );
    }

    #[test]
    fn test_browser_cli_list_request_serialization() {
        let req = BrowserCliRequest::List;
        let json = serde_json::to_string(&req).unwrap();
        assert_eq!(json, r#"{"command":"list"}"#);
        let parsed: BrowserCliRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, BrowserCliRequest::List);
    }

    #[test]
    fn test_browser_cli_tab_and_state_requests_serialization() {
        let tab_list = BrowserCliRequest::TabList;
        let json = serde_json::to_string(&tab_list).unwrap();
        assert_eq!(json, r#"{"command":"tabList"}"#);
        assert_eq!(serde_json::from_str::<BrowserCliRequest>(&json).unwrap(), tab_list);

        let tab_new = BrowserCliRequest::TabNew {
            url: Some("https://example.com/app".into()),
            workspace_id: Some("ws-main".into()),
            worktree_path: None,
        };
        let new_json = serde_json::to_string(&tab_new).unwrap();
        assert!(new_json.contains(r#""command":"tabNew""#));
        assert_eq!(serde_json::from_str::<BrowserCliRequest>(&new_json).unwrap(), tab_new);

        let tab_switch = BrowserCliRequest::TabSwitch {
            browser_id: None,
            index: Some(2),
        };
        let switch_json = serde_json::to_string(&tab_switch).unwrap();
        assert_eq!(serde_json::from_str::<BrowserCliRequest>(&switch_json).unwrap(), tab_switch);

        let state_save = BrowserCliRequest::StateSave {
            browser_id: "b-1".into(),
            out_path: "/tmp/state.json".into(),
        };
        let save_json = serde_json::to_string(&state_save).unwrap();
        assert_eq!(serde_json::from_str::<BrowserCliRequest>(&save_json).unwrap(), state_save);

        let state_load = BrowserCliRequest::StateLoad {
            browser_id: "b-1".into(),
            in_path: "/tmp/state.json".into(),
        };
        let load_json = serde_json::to_string(&state_load).unwrap();
        assert_eq!(serde_json::from_str::<BrowserCliRequest>(&load_json).unwrap(), state_load);
    }

    #[test]
    fn test_browser_cli_list_response_serialization() {
        let resp = BrowserCliResponse::List {
            sessions: vec![BrowserSessionSummary {
                browser_id: "test-id".to_string(),
                webview_label: "test-label".to_string(),
                workspace_id: Some("ws-1".to_string()),
                profile_id: BrowserProfileId::Default,
                url: "https://example.com/".to_string(),
                title: Some("Example".to_string()),
                visible: true,
            }],
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: BrowserCliResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, resp);
    }

    // This fixture uses the production connection handler over an actual owned
    // TCP socket on every platform. No desktop, daemon or global runtime path.
    async fn p12_raw_tcp_request(request: serde_json::Value) -> BrowserCliResponse {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());
        manager
            .register_session(CreateBrowserRequest {
                browser_id: Some("p12-owned-browser".into()),
                workspace_id: Some("p12-owned-workspace".into()),
                worktree_path: None,
                url: "https://example.test/p12-private".into(),
                profile: Some(BrowserProfileId::Default),
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .expect("register owned browser");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind owned listener");
        let address = listener.local_addr().expect("listener address");
        let server = async {
            let (stream, _) = listener.accept().await.expect("accept independent peer");
            handle_connection(
                stream,
                app.handle().clone(),
                manager,
                Arc::new("p12-owned-capability-token".to_string()),
            )
            .await
            .expect("handle actual TCP connection");
        };
        let client = async {
            let mut stream = tokio::net::TcpStream::connect(address)
                .await
                .expect("connect independent peer");
            let mut bytes = serde_json::to_vec(&request).expect("serialize raw request");
            bytes.push(b'\n');
            stream.write_all(&bytes).await.expect("write raw request");
            let mut response = String::new();
            BufReader::new(stream)
                .read_line(&mut response)
                .await
                .expect("read authorization result");
            serde_json::from_str(&response).expect("parse authorization result")
        };
        // Joining scoped futures, rather than spawning, guarantees that timeout
        // and panic drop the listener and both streams; there is no orphan task.
        let (_, response) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            tokio::join!(server, client)
        })
        .await
        .expect("bounded socket exchange");
        response
    }

    #[tokio::test]
    async fn p12_tcp_rejects_unauthenticated_commands() {
        // Given an actual listener and a registered private browser, when a peer
        // sends each supported command without credentials, then dispatch is denied.
        for request in [
            serde_json::json!({"command": "list"}),
            serde_json::json!({"command": "snapshot", "browser_id": "missing-browser"}),
            serde_json::json!({"command": "act", "request": {
                "browserId": "missing-browser", "generation": 1,
                "action": {"type": "click", "reference": "e1"}
            }}),
        ] {
            let response = p12_raw_tcp_request(request.clone()).await;
            assert!(
                matches!(&response, BrowserCliResponse::Error { code, .. }
                    if code == "BROWSER_CLI_UNAUTHORIZED"),
                "unauthenticated {request} reached dispatch: {response:?}"
            );
        }
    }

    #[tokio::test]
    async fn p12_tcp_rejects_forged_credential() {
        // Given a peer without the capability, when it supplies a forged token,
        // then the actual socket must not disclose the registered browser URL.
        let response = p12_raw_tcp_request(serde_json::json!({
            "command": "list", "token": "p12-forged-not-a-capability"
        }))
        .await;
        assert!(
            matches!(&response, BrowserCliResponse::Error { code, .. }
                if code == "BROWSER_CLI_UNAUTHORIZED"),
            "forged credential reached dispatch: {response:?}"
        );
    }

    #[tokio::test]
    async fn p12_tcp_accepts_the_capability_token() {
        // Given the capability token this server instance minted, when a peer
        // presents it, then the command is dispatched normally. This is what
        // proves the rejection tests above fail on authorization, not on the
        // envelope shape.
        let response = p12_raw_tcp_request(serde_json::json!({
            "command": "list", "token": "p12-owned-capability-token"
        }))
        .await;
        let BrowserCliResponse::List { sessions } = response else {
            panic!("authorized list must dispatch: {response:?}");
        };
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].browser_id, "p12-owned-browser");
    }

    #[test]
    fn browser_cli_tokens_match_only_on_exact_equality() {
        let token = generate_browser_cli_token();
        assert_eq!(token.len(), BROWSER_CLI_TOKEN_BYTES * 2);
        assert!(tokens_match(&token, &token.clone()));
        assert!(!tokens_match(&token, &token[..token.len() - 1]));
        assert!(!tokens_match(&token, ""));
        // Two separately minted tokens must not collide.
        assert_ne!(token, generate_browser_cli_token());
    }

    #[test]
    fn test_read_and_write_port_file_round_trip() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let port_path = temp_dir.path().join("browser.port");
        write_port_file(&port_path, 43210).expect("write port file");
        let read_port = read_port_from_file(&port_path).expect("read port file");
        assert_eq!(read_port, 43210);
    }

    #[test]
    fn test_write_port_file_replaces_existing_file() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let port_path = temp_dir.path().join("browser.port");
        write_port_file(&port_path, 11111).expect("write initial port file");
        assert_eq!(read_port_from_file(&port_path).expect("read port"), 11111);

        write_port_file(&port_path, 22222).expect("overwrite port file");
        assert_eq!(
            read_port_from_file(&port_path).expect("read updated port"),
            22222
        );
    }

    #[test]
    fn test_read_port_from_file_missing_returns_unavailable() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let port_path = temp_dir.path().join("nonexistent.port");
        let result = read_port_from_file(&port_path);
        assert!(matches!(result, Err(BrowserError::CliUnavailable(_))));
    }

    #[test]
    fn test_read_port_from_file_malformed_returns_unavailable() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let port_path = temp_dir.path().join("invalid.port");

        fs::write(&port_path, "not-a-port\n").expect("write malformed");
        assert!(matches!(
            read_port_from_file(&port_path),
            Err(BrowserError::CliUnavailable(_))
        ));

        fs::write(&port_path, "0\n").expect("write port 0");
        assert!(matches!(
            read_port_from_file(&port_path),
            Err(BrowserError::CliUnavailable(_))
        ));

        fs::write(&port_path, "70000\n").expect("write out-of-range port");
        assert!(matches!(
            read_port_from_file(&port_path),
            Err(BrowserError::CliUnavailable(_))
        ));

        fs::write(&port_path, "   \n").expect("write empty/whitespace");
        assert!(matches!(
            read_port_from_file(&port_path),
            Err(BrowserError::CliUnavailable(_))
        ));
    }

    #[tokio::test]
    async fn test_browser_cli_send_over_stream_round_trip() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");

        let manager = Arc::new(BrowserManager::new());
        let registered_session = manager
            .register_session(CreateBrowserRequest {
                browser_id: None,
                workspace_id: Some("workspace-duplex".to_string()),
                worktree_path: Some("/worktree/alpha".to_string()),
                url: "https://ferryx.dev".to_string(),
                profile: Some(BrowserProfileId::Default),
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .expect("register session");

        let (client_stream, server_stream) = tokio::io::duplex(4096);
        let app_handle = app.handle().clone();
        let manager_clone = Arc::clone(&manager);
        let server_task = tokio::spawn(async move {
            handle_connection(
                server_stream,
                app_handle,
                manager_clone,
                Arc::new("duplex-capability-token".to_string()),
            )
            .await
        });

        let response = send_over_stream(
            client_stream,
            BrowserCliRequest::List,
            "duplex-capability-token".to_string(),
        )
        .await
        .expect("send request over stream");

        let server_result = server_task.await.expect("server task completed");
        assert!(server_result.is_ok());

        let expected_summary = BrowserSessionSummary {
            browser_id: registered_session.browser_id,
            webview_label: registered_session.webview_label,
            workspace_id: Some("workspace-duplex".to_string()),
            profile_id: BrowserProfileId::Default,
            url: registered_session.url,
            title: None,
            visible: true,
        };

        assert_eq!(
            response,
            BrowserCliResponse::List {
                sessions: vec![expected_summary],
            }
        );
    }

    #[tokio::test]
    async fn test_read_limited_line_rejects_oversized_request_line() {
        let mut oversized = vec![b'a'; MAX_REQUEST_BYTES];
        oversized.push(b'!');
        oversized.push(b'\n');
        let mut reader = BufReader::new(&oversized[..]);
        let result = read_limited_line(&mut reader, MAX_REQUEST_BYTES).await;
        assert!(matches!(result, Ok(RequestLine::TooLarge)));

        // A line exactly at the cap is still accepted.
        let mut exact = vec![b'a'; MAX_REQUEST_BYTES - 1];
        exact.push(b'\n');
        let mut reader = BufReader::new(&exact[..]);
        let result = read_limited_line(&mut reader, MAX_REQUEST_BYTES).await;
        assert!(matches!(result, Ok(RequestLine::Line(_))));
    }

    #[tokio::test]
    async fn test_browser_cli_oversized_request_gets_too_large_error() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());

        let (client_stream, server_stream) = tokio::io::duplex(64 * 1024);
        let app_handle = app.handle().clone();
        let server_task = tokio::spawn(async move {
            handle_connection(
                server_stream,
                app_handle,
                manager,
                Arc::new("oversize-capability-token".to_string()),
            )
            .await
        });

        let (client_reader, mut client_writer) = tokio::io::split(client_stream);
        let mut oversized_line = vec![b'a'; MAX_REQUEST_BYTES + 16];
        oversized_line.push(b'\n');
        // The server stops reading once the cap is exceeded and closes its half,
        // so the tail of this write may surface as a broken pipe; either way the
        // oversized line must never be accepted.
        let _ = client_writer.write_all(&oversized_line).await;
        let _ = client_writer.flush().await;

        let mut response_line = String::new();
        BufReader::new(client_reader)
            .read_line(&mut response_line)
            .await
            .expect("read too-large response line");
        let response: BrowserCliResponse =
            serde_json::from_str(response_line.trim()).expect("deserialize response");
        assert_eq!(
            response,
            BrowserCliResponse::Error {
                code: "BROWSER_CLI_REQUEST_TOO_LARGE".into(),
                details: None,
                message: format!(
                    "request exceeds the maximum of {MAX_REQUEST_BYTES} bytes per connection"
                ),
            }
        );

        let server_result = server_task.await.expect("server task completed");
        assert!(server_result.is_ok());
    }

    #[tokio::test]
    async fn test_browser_cli_error_code_matches_ipc_wire_format() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());

        let response = execute_request(
            app.handle(),
            &manager,
            BrowserCliRequest::Snapshot {
                browser_id: "missing-browser".into(),
            },
        )
        .await;

        let wire_code =
            serde_json::to_value(IpcErrorCode::BrowserNotFound).expect("serialize IPC error code");
        let wire_code = wire_code.as_str().expect("IPC code serializes to string");
        assert_eq!(wire_code, "BROWSER_NOT_FOUND");

        match response {
            BrowserCliResponse::Error { code, message, .. } => {
                assert_eq!(code, wire_code);
                assert_ne!(code, "BrowserNotFound");
                assert!(!message.is_empty());
            }
            other => panic!("expected error response, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_browser_cli_list_round_trip_unix_stream() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");

        let manager = Arc::new(BrowserManager::new());
        let registered_session = manager
            .register_session(CreateBrowserRequest {
                browser_id: None,
                workspace_id: Some("workspace-regression".to_string()),
                worktree_path: Some("/worktree/alpha".to_string()),
                url: "https://ferryx.dev".to_string(),
                profile: Some(BrowserProfileId::Default),
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .expect("register session");

        let (mut client_stream, server_stream) =
            tokio::net::UnixStream::pair().expect("unix stream pair");

        let app_handle = app.handle().clone();
        let manager_clone = Arc::clone(&manager);
        let handler_task = tokio::spawn(async move {
            handle_connection(
                server_stream,
                app_handle,
                manager_clone,
                Arc::new("unix-pair-capability-token".to_string()),
            )
            .await
        });

        client_stream
            .write_all(b"{\"command\":\"list\",\"token\":\"unix-pair-capability-token\"}\n")
            .await
            .expect("write request line");
        client_stream.flush().await.expect("flush request");

        let (client_reader, _) = client_stream.into_split();
        let mut response_line = String::new();
        BufReader::new(client_reader)
            .read_line(&mut response_line)
            .await
            .expect("read response line");

        let handler_result = handler_task.await.expect("handler task completed");
        assert!(handler_result.is_ok());

        let response: BrowserCliResponse =
            serde_json::from_str(&response_line).expect("deserialize response");

        let expected_summary = BrowserSessionSummary {
            browser_id: registered_session.browser_id,
            webview_label: registered_session.webview_label,
            workspace_id: Some("workspace-regression".to_string()),
            profile_id: BrowserProfileId::Default,
            url: registered_session.url,
            title: None,
            visible: true,
        };

        assert_eq!(
            response,
            BrowserCliResponse::List {
                sessions: vec![expected_summary],
            }
        );
    }

    #[cfg(unix)]
    #[test]
    fn browser_cli_server_starts_without_tokio_reactor() {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::fs::PermissionsExt;
        use std::os::unix::net::UnixStream;
        use std::time::Duration;

        assert!(tokio::runtime::Handle::try_current().is_err());
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let socket_path = temp_dir.path().join("browser.sock");
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");

        start_browser_cli_server_at_path(
            app.handle().clone(),
            Arc::new(BrowserManager::new()),
            &socket_path,
        )
        .expect("browser CLI startup succeeds without Tokio reactor");
        assert_eq!(
            fs::metadata(&socket_path)
                .expect("socket metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600,
        );

        let mut client = UnixStream::connect(&socket_path).expect("connect to browser CLI socket");
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("set response timeout");
        // The server minted this token at startup; reading it back is exactly what
        // an authorized caller does, and the file must be owner-only.
        let token_path = token_path_for(&socket_path);
        assert_eq!(
            fs::metadata(&token_path)
                .expect("token metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600,
        );
        let token = read_token_file(&token_path).expect("read capability token");
        client
            .write_all(format!("{{\"command\":\"list\",\"token\":\"{token}\"}}\n").as_bytes())
            .expect("write list request");
        let mut response = String::new();
        BufReader::new(client)
            .read_line(&mut response)
            .expect("read browser CLI list response");
        assert_eq!(
            serde_json::from_str::<BrowserCliResponse>(response.trim())
                .expect("parse browser CLI list response"),
            BrowserCliResponse::List {
                sessions: Vec::new()
            },
        );
    }

    #[cfg(not(unix))]
    #[test]
    fn browser_cli_server_starts_without_tokio_reactor() {
        use std::io::{BufRead, BufReader, Write};
        use std::net::TcpStream;
        use std::time::Duration;

        assert!(tokio::runtime::Handle::try_current().is_err());
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let port_path = temp_dir.path().join("browser.port");
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");

        start_browser_cli_server_at_path(
            app.handle().clone(),
            Arc::new(BrowserManager::new()),
            &port_path,
        )
        .expect("browser CLI startup succeeds without Tokio reactor");

        assert!(port_path.exists());
        let port = read_port_from_file(&port_path).expect("read port from file");
        assert!(port > 0);

        let mut client =
            TcpStream::connect(format!("127.0.0.1:{port}")).expect("connect to browser CLI port");
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("set response timeout");
        let token = read_token_file(&token_path_for(&port_path)).expect("read capability token");
        client
            .write_all(format!("{{\"command\":\"list\",\"token\":\"{token}\"}}\n").as_bytes())
            .expect("write list request");
        let mut response = String::new();
        BufReader::new(client)
            .read_line(&mut response)
            .expect("read browser CLI list response");
        assert_eq!(
            serde_json::from_str::<BrowserCliResponse>(response.trim())
                .expect("parse browser CLI list response"),
            BrowserCliResponse::List {
                sessions: Vec::new()
            },
        );
    }

    fn manager_with_session(workspace_id: &str) -> Arc<BrowserManager> {
        let manager = Arc::new(BrowserManager::new());
        manager
            .register_session(CreateBrowserRequest {
                browser_id: None,
                workspace_id: Some(workspace_id.to_string()),
                worktree_path: None,
                url: "https://ferryx.dev".to_string(),
                profile: Some(BrowserProfileId::Default),
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .expect("register session");
        manager
    }

    /// Workspace ids of the listed sessions. They identify which instance's manager answered.
    fn listed_workspaces(response: BrowserCliResponse) -> Vec<Option<String>> {
        match response {
            BrowserCliResponse::List { sessions } => sessions
                .into_iter()
                .map(|session| session.workspace_id)
                .collect(),
            other => panic!("expected a list response, got {other:?}"),
        }
    }

    fn mock_app() -> tauri::App<tauri::test::MockRuntime> {
        tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app")
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn browser_cli_second_start_leaves_the_live_endpoint_and_token_untouched() {
        use std::os::unix::fs::MetadataExt;

        let temp_dir = tempfile::tempdir().expect("tempdir");
        let socket_path = temp_dir.path().join("browser.sock");
        let token_path = token_path_for(&socket_path);

        let first_app = mock_app();
        assert_eq!(
            start_browser_cli_server_at_path(
                first_app.handle().clone(),
                manager_with_session("workspace-first"),
                &socket_path,
            )
            .expect("first start"),
            BrowserCliServerStart::Serving,
        );
        let socket_inode = fs::symlink_metadata(&socket_path)
            .expect("socket metadata")
            .ino();
        let token = fs::read(&token_path).expect("read first token");

        let second_app = mock_app();
        assert_eq!(
            start_browser_cli_server_at_path(
                second_app.handle().clone(),
                manager_with_session("workspace-second"),
                &socket_path,
            )
            .expect("second start"),
            BrowserCliServerStart::ServedByPeer,
        );

        assert_eq!(
            fs::symlink_metadata(&socket_path)
                .expect("socket metadata after second start")
                .ino(),
            socket_inode,
        );
        assert_eq!(
            fs::read(&token_path).expect("read token after second start"),
            token
        );
        let response = send_browser_cli_request_at_path(BrowserCliRequest::List, &socket_path)
            .await
            .expect("first instance still answers with its token");
        assert_eq!(
            listed_workspaces(response),
            vec![Some("workspace-first".to_string())]
        );
    }

    #[cfg(not(unix))]
    #[tokio::test]
    async fn browser_cli_second_start_leaves_the_live_endpoint_and_token_untouched() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let port_path = temp_dir.path().join("browser.port");
        let token_path = token_path_for(&port_path);

        let first_app = mock_app();
        assert_eq!(
            start_browser_cli_server_at_path(
                first_app.handle().clone(),
                manager_with_session("workspace-first"),
                &port_path,
            )
            .expect("first start"),
            BrowserCliServerStart::Serving,
        );
        let port_file = fs::read(&port_path).expect("read first port file");
        let token = fs::read(&token_path).expect("read first token");

        let second_app = mock_app();
        assert_eq!(
            start_browser_cli_server_at_path(
                second_app.handle().clone(),
                manager_with_session("workspace-second"),
                &port_path,
            )
            .expect("second start"),
            BrowserCliServerStart::ServedByPeer,
        );

        assert_eq!(
            fs::read(&port_path).expect("read port file after second start"),
            port_file
        );
        assert_eq!(
            fs::read(&token_path).expect("read token after second start"),
            token
        );
        let response = send_browser_cli_request_at_path(BrowserCliRequest::List, &port_path)
            .await
            .expect("first instance still answers on its port with its token");
        assert_eq!(
            listed_workspaces(response),
            vec![Some("workspace-first".to_string())]
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn browser_cli_start_reclaims_the_endpoint_only_after_the_lock_is_released() {
        use std::os::unix::fs::MetadataExt;

        let temp_dir = tempfile::tempdir().expect("tempdir");
        let socket_path = temp_dir.path().join("browser.sock");
        let token_path = token_path_for(&socket_path);
        // An exited holder leaves its socket node behind.
        drop(std::os::unix::net::UnixListener::bind(&socket_path).expect("bind stale socket"));
        let stale_inode = fs::symlink_metadata(&socket_path)
            .expect("stale socket metadata")
            .ino();

        let held = crate::daemon::server::DaemonLockFile::try_lock(
            crate::daemon::server::open_secure_lock_file(&endpoint_lock_path(&socket_path))
                .expect("open endpoint lock"),
        )
        .expect("hold endpoint lock");
        let app = mock_app();
        assert_eq!(
            start_browser_cli_server_at_path(
                app.handle().clone(),
                manager_with_session("workspace-blocked"),
                &socket_path,
            )
            .expect("start while the lock is held"),
            BrowserCliServerStart::ServedByPeer,
        );
        assert_eq!(
            fs::symlink_metadata(&socket_path)
                .expect("socket metadata while held")
                .ino(),
            stale_inode,
        );
        assert!(!token_path.exists());

        drop(held);
        assert_eq!(
            start_browser_cli_server_at_path(
                app.handle().clone(),
                manager_with_session("workspace-reclaimed"),
                &socket_path,
            )
            .expect("start after the lock is released"),
            BrowserCliServerStart::Serving,
        );
        // The stale socket refuses connections, so a successful list proves it was replaced.
        let response = send_browser_cli_request_at_path(BrowserCliRequest::List, &socket_path)
            .await
            .expect("reclaimed endpoint answers");
        assert_eq!(
            listed_workspaces(response),
            vec![Some("workspace-reclaimed".to_string())]
        );
    }

    #[cfg(not(unix))]
    #[tokio::test]
    async fn browser_cli_start_reclaims_the_endpoint_only_after_the_lock_is_released() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let port_path = temp_dir.path().join("browser.port");
        let token_path = token_path_for(&port_path);
        // An exited holder leaves its port and token files behind.
        fs::write(&port_path, "1").expect("write stale port file");
        fs::write(&token_path, "stale-token").expect("write stale token");

        let held = crate::daemon::server::DaemonLockFile::try_lock(
            crate::daemon::server::open_secure_lock_file(&endpoint_lock_path(&port_path))
                .expect("open endpoint lock"),
        )
        .expect("hold endpoint lock");
        let app = mock_app();
        assert_eq!(
            start_browser_cli_server_at_path(
                app.handle().clone(),
                manager_with_session("workspace-blocked"),
                &port_path,
            )
            .expect("start while the lock is held"),
            BrowserCliServerStart::ServedByPeer,
        );
        assert_eq!(fs::read_to_string(&port_path).expect("read port file"), "1");
        assert_eq!(
            fs::read_to_string(&token_path).expect("read token"),
            "stale-token"
        );

        drop(held);
        assert_eq!(
            start_browser_cli_server_at_path(
                app.handle().clone(),
                manager_with_session("workspace-reclaimed"),
                &port_path,
            )
            .expect("start after the lock is released"),
            BrowserCliServerStart::Serving,
        );
        let response = send_browser_cli_request_at_path(BrowserCliRequest::List, &port_path)
            .await
            .expect("reclaimed endpoint answers");
        assert_eq!(
            listed_workspaces(response),
            vec![Some("workspace-reclaimed".to_string())]
        );
    }

    #[cfg(not(unix))]
    #[tokio::test]
    async fn test_browser_cli_list_round_trip_tcp() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");

        let manager = Arc::new(BrowserManager::new());
        let registered_session = manager
            .register_session(CreateBrowserRequest {
                browser_id: None,
                workspace_id: Some("workspace-tcp".to_string()),
                worktree_path: Some("/worktree/alpha".to_string()),
                url: "https://ferryx.dev".to_string(),
                profile: Some(BrowserProfileId::Default),
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .expect("register session");

        let temp_dir = tempfile::tempdir().expect("tempdir");
        let port_path = temp_dir.path().join("browser.port");

        start_browser_cli_server_at_path(app.handle().clone(), Arc::clone(&manager), &port_path)
            .expect("start browser CLI server");

        let response = send_browser_cli_request_at_path(BrowserCliRequest::List, &port_path)
            .await
            .expect("send browser CLI list request");

        let expected_summary = BrowserSessionSummary {
            browser_id: registered_session.browser_id,
            webview_label: registered_session.webview_label,
            workspace_id: Some("workspace-tcp".to_string()),
            profile_id: BrowserProfileId::Default,
            url: registered_session.url,
            title: None,
            visible: true,
        };

        assert_eq!(
            response,
            BrowserCliResponse::List {
                sessions: vec![expected_summary],
            }
        );
    }

    #[cfg(not(unix))]
    #[tokio::test]
    async fn test_browser_cli_send_request_stale_port_fails() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let port_path = temp_dir.path().join("stale.port");

        // Bind to get an unused port and immediately close it
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let closed_port = listener.local_addr().expect("local addr").port();
        drop(listener);

        write_port_file(&port_path, closed_port).expect("write stale port file");

        let result = send_browser_cli_request_at_path(BrowserCliRequest::List, &port_path).await;
        assert!(matches!(result, Err(BrowserError::CliUnavailable(_))));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_browser_cli_send_request_missing_token_file_fails() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let socket_path = temp_dir.path().join("stale.sock");
        std::fs::write(&socket_path, b"").expect("write socket placeholder");

        let result = send_browser_cli_request_at_path(BrowserCliRequest::List, &socket_path).await;
        // The token read must fail first (ENOENT, os error 2). A connect failure against the
        // placeholder file would instead report ECONNREFUSED, so this pins the token path.
        match result {
            Err(BrowserError::CliUnavailable(message)) => assert!(
                message.contains("os error 2"),
                "expected the missing-token read error, got: {message}"
            ),
            other => panic!("expected CliUnavailable from the token read, got {other:?}"),
        }
    }

    async fn send_raw_line<R: tauri::Runtime>(
        app_handle: AppHandle<R>,
        manager: Arc<BrowserManager>,
        token: &str,
        raw_json: &str,
    ) -> serde_json::Value {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let (client_stream, server_stream) = tokio::io::duplex(8192);
        let token_clone = Arc::new(token.to_string());
        let handler_task = tokio::spawn(async move {
            handle_connection(server_stream, app_handle, manager, token_clone).await
        });

        let (client_reader, mut client_writer) = tokio::io::split(client_stream);
        let mut line = raw_json.as_bytes().to_vec();
        line.push(b'\n');
        client_writer
            .write_all(&line)
            .await
            .expect("write raw json line");
        client_writer.flush().await.expect("flush raw json line");

        let mut response_line = String::new();
        BufReader::new(client_reader)
            .read_line(&mut response_line)
            .await
            .expect("read response line");

        let handler_result = handler_task.await.expect("handler task completed");
        assert!(handler_result.is_ok());

        serde_json::from_str(response_line.trim()).expect("deserialize response JSON")
    }

    #[tokio::test]
    async fn test_browser_cli_open_round_trip() {
        use tauri::Listener;

        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());
        let token = "test-token";

        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<serde_json::Value>();
        app.listen("browser_session_created", move |event| {
            if let Ok(payload) = serde_json::from_str::<serde_json::Value>(event.payload()) {
                let _ = event_tx.send(payload);
            }
        });

        // The mock app has no "main" window to host the browser webview, so create fails.
        // A mock "main" window cannot stand in: on Linux the child attach calls
        // `default_vbox`, which MockRuntime leaves unimplemented.
        let raw_req = format!(
            "{{\"command\":\"open\",\"url\":\"https://example.com\",\"workspaceId\":\"ws-a\",\"token\":\"{token}\"}}"
        );
        let resp = send_raw_line(app.handle().clone(), Arc::clone(&manager), token, &raw_req).await;

        assert_eq!(resp["type"], "error", "unexpected response: {resp:?}");
        assert_eq!(resp["code"], "BROWSER_CREATE_FAILED", "unexpected response: {resp:?}");

        // A failed create leaves no session behind and announces nothing to the GUI.
        let list_req = format!("{{\"command\":\"list\",\"token\":\"{token}\"}}");
        let list_resp =
            send_raw_line(app.handle().clone(), Arc::clone(&manager), token, &list_req).await;
        assert_eq!(list_resp["type"], "list");
        let sessions = list_resp["sessions"].as_array().expect("sessions array");
        assert!(sessions.is_empty(), "failed create left a session: {sessions:?}");
        assert!(
            event_rx.try_recv().is_err(),
            "a failed create must not emit browser_session_created"
        );
    }

    #[tokio::test]
    async fn test_browser_cli_open_rejects_file_scheme() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());
        let token = "test-token";

        let raw_req = format!(
            "{{\"command\":\"open\",\"url\":\"file:///etc/passwd\",\"token\":\"{token}\"}}"
        );
        let resp = send_raw_line(app.handle().clone(), Arc::clone(&manager), token, &raw_req).await;

        assert_eq!(resp["type"], "error", "unexpected response: {resp:?}");
        assert_eq!(
            resp["code"], "BROWSER_URL_SCHEME_DENIED",
            "unexpected response: {resp:?}"
        );
    }

    #[tokio::test]
    async fn test_browser_cli_open_rejects_javascript_scheme() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());
        let token = "test-token";

        let raw_req = format!(
            "{{\"command\":\"open\",\"url\":\"javascript:alert(1)\",\"token\":\"{token}\"}}"
        );
        let resp = send_raw_line(app.handle().clone(), Arc::clone(&manager), token, &raw_req).await;

        assert_eq!(resp["type"], "error", "unexpected response: {resp:?}");
        assert_eq!(
            resp["code"], "BROWSER_URL_SCHEME_DENIED",
            "unexpected response: {resp:?}"
        );
    }

    /// A framed remote navigate is a remote operation: the Remote URL policy applies,
    /// so a local file document is a scheme denial even though the desktop allows it.
    #[tokio::test]
    async fn test_remote_operation_navigate_denies_file_scheme() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());
        let state = manager
            .register_session(crate::browser::model::CreateBrowserRequest {
                browser_id: Some("b-remote-op-nav".into()),
                workspace_id: Some("ws1".into()),
                worktree_path: None,
                url: "https://example.com".into(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .unwrap();
        let op = RemoteBrowserOperation::Navigate {
            browser_id: state.browser_id.clone(),
            url: "file:///tmp/x.html".into(),
        };
        let err = execute_remote_operation(&app.handle().clone(), &manager, op)
            .await
            .expect_err("remote navigate to a local file must be refused");
        assert_eq!(ipc_error_code_string(err.code.clone()), "BROWSER_FORBIDDEN", "{err:?}");
        assert_eq!(manager.get_state(&state.browser_id).unwrap().url, state.url);

        let credentialed = RemoteBrowserOperation::Navigate {
            browser_id: state.browser_id.clone(),
            url: "https://user:pw@example.com".into(),
        };
        let err = execute_remote_operation(&app.handle().clone(), &manager, credentialed)
            .await
            .expect_err("credentialed URL must be refused");
        assert_eq!(ipc_error_code_string(err.code.clone()), "BROWSER_INVALID_REQUEST", "{err:?}");
    }

    #[tokio::test]
    async fn test_browser_cli_navigate_round_trip() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());
        let token = "test-token";

        // Navigate to unknown browserId -> Error BROWSER_NOT_FOUND
        let raw_req_unknown = format!(
            "{{\"command\":\"navigate\",\"browserId\":\"unknown-browser-id\",\"url\":\"https://example.com\",\"token\":\"{token}\"}}"
        );
        let resp_unknown = send_raw_line(
            app.handle().clone(),
            Arc::clone(&manager),
            token,
            &raw_req_unknown,
        )
        .await;
        assert_eq!(
            resp_unknown["type"], "error",
            "unexpected response: {resp_unknown:?}"
        );
        assert_eq!(
            resp_unknown["code"], "BROWSER_NOT_FOUND",
            "unexpected response: {resp_unknown:?}"
        );

        // Navigate existing (register one via manager.register_session) -> Navigated and url updates in list
        let registered = manager
            .register_session(CreateBrowserRequest {
                browser_id: None,
                workspace_id: None,
                worktree_path: None,
                url: "https://initial.example.com".to_string(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .expect("register session");
        // Navigation needs the session's webview to exist.
        let _webview =
            tauri::WebviewWindowBuilder::new(&app, &registered.webview_label, Default::default())
                .build()
                .expect("mock webview");

        let raw_req_existing = format!(
            "{{\"command\":\"navigate\",\"browserId\":\"{}\",\"url\":\"https://updated.example.com\",\"token\":\"{token}\"}}",
            registered.browser_id
        );
        let resp_existing = send_raw_line(
            app.handle().clone(),
            Arc::clone(&manager),
            token,
            &raw_req_existing,
        )
        .await;
        assert_eq!(
            resp_existing["type"], "navigated",
            "unexpected response: {resp_existing:?}"
        );

        // List confirms updated url
        let list_req = format!("{{\"command\":\"list\",\"token\":\"{token}\"}}");
        let list_resp =
            send_raw_line(app.handle().clone(), Arc::clone(&manager), token, &list_req).await;
        let sessions = list_resp["sessions"].as_array().expect("sessions array");
        let found = sessions
            .iter()
            .find(|s| s["browserId"] == registered.browser_id);
        assert!(found.is_some(), "session not found in list");
        assert!(
            found.unwrap()["url"] == "https://updated.example.com"
                || found.unwrap()["url"] == "https://updated.example.com/",
            "unexpected updated url: {:?}",
            found.unwrap()["url"]
        );
    }

    #[tokio::test]
    async fn test_browser_cli_close_round_trip() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());
        let token = "test-token";

        let registered = manager
            .register_session(CreateBrowserRequest {
                browser_id: None,
                workspace_id: None,
                worktree_path: None,
                url: "https://example.com".to_string(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .expect("register session");

        // Close existing -> Closed then list is empty
        let raw_req_close = format!(
            "{{\"command\":\"close\",\"browserId\":\"{}\",\"token\":\"{token}\"}}",
            registered.browser_id
        );
        let resp_close = send_raw_line(
            app.handle().clone(),
            Arc::clone(&manager),
            token,
            &raw_req_close,
        )
        .await;
        assert_eq!(
            resp_close["type"], "closed",
            "unexpected response: {resp_close:?}"
        );

        let list_req = format!("{{\"command\":\"list\",\"token\":\"{token}\"}}");
        let list_resp =
            send_raw_line(app.handle().clone(), Arc::clone(&manager), token, &list_req).await;
        let sessions = list_resp["sessions"].as_array().expect("sessions array");
        assert!(
            sessions.is_empty(),
            "expected empty list after close, got: {sessions:?}"
        );

        // Close again -> Error BROWSER_NOT_FOUND
        let resp_close_again = send_raw_line(
            app.handle().clone(),
            Arc::clone(&manager),
            token,
            &raw_req_close,
        )
        .await;
        assert_eq!(
            resp_close_again["type"], "error",
            "unexpected response: {resp_close_again:?}"
        );
        assert_eq!(
            resp_close_again["code"], "BROWSER_NOT_FOUND",
            "unexpected response: {resp_close_again:?}"
        );
    }

    #[tokio::test]
    async fn test_browser_cli_identify_round_trip() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());
        let token = "test-token";

        // Identify with empty manager -> Identified with None (browser == null)
        let raw_req_identify = format!("{{\"command\":\"identify\",\"token\":\"{token}\"}}");
        let resp_empty = send_raw_line(
            app.handle().clone(),
            Arc::clone(&manager),
            token,
            &raw_req_identify,
        )
        .await;
        assert_eq!(
            resp_empty["type"], "identified",
            "unexpected response: {resp_empty:?}"
        );
        assert!(
            resp_empty["browser"].is_null(),
            "expected null browser for empty manager"
        );

        // Identify with a registered visible session -> Identified with Some
        let registered = manager
            .register_session(CreateBrowserRequest {
                browser_id: None,
                workspace_id: None,
                worktree_path: None,
                url: "https://example.com".to_string(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .expect("register session");

        let resp_visible = send_raw_line(
            app.handle().clone(),
            Arc::clone(&manager),
            token,
            &raw_req_identify,
        )
        .await;
        assert_eq!(
            resp_visible["type"], "identified",
            "unexpected response: {resp_visible:?}"
        );
        assert!(
            !resp_visible["browser"].is_null(),
            "expected Some browser for visible session"
        );
        assert_eq!(resp_visible["browser"]["browserId"], registered.browser_id);
    }

    #[test]
    fn test_browser_cli_phase3_wire_serialization() {
        use crate::browser::model::{
            BrowserConsoleEntry, BrowserCookieEntry, BrowserWaitCondition,
        };

        // Eval
        let req = BrowserCliRequest::Eval {
            browser_id: "b1".into(),
            script: "1 + 1".into(),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""command":"eval""#));
        assert!(json.contains(r#""browserId":"b1""#));
        let parsed: BrowserCliRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, req);

        // Wait
        let req = BrowserCliRequest::Wait {
            browser_id: "b1".into(),
            condition: BrowserWaitCondition::Selector {
                selector: "#main".into(),
            },
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""command":"wait""#));
        assert!(json.contains(r##""selector":"#main""##));
        let parsed: BrowserCliRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, req);

        // Console
        let req = BrowserCliRequest::Console {
            browser_id: "b1".into(),
            errors_only: Some(true),
            clear: Some(false),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""command":"console""#));
        let parsed: BrowserCliRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, req);

        // Focus
        let req = BrowserCliRequest::Focus {
            browser_id: "b1".into(),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""command":"focus""#));
        let parsed: BrowserCliRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, req);

        // Screenshot
        let req = BrowserCliRequest::Screenshot {
            browser_id: "b1".into(),
            out_path: "/tmp/shot.png".into(),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""command":"screenshot""#));
        assert!(json.contains(r#""outPath":"/tmp/shot.png""#));
        let parsed: BrowserCliRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, req);

        // Cookies
        let req = BrowserCliRequest::Cookies {
            browser_id: "b1".into(),
            action: "get".into(),
            name: Some("foo".into()),
            value: None,
            domain: None,
            path: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""command":"cookies""#));
        let parsed: BrowserCliRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, req);

        // Storage
        let req = BrowserCliRequest::Storage {
            browser_id: "b1".into(),
            kind: "local".into(),
            action: "get".into(),
            key: Some("theme".into()),
            value: None,
            clear_all: None,
            url: None,
            domain: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""command":"storage""#));
        let parsed: BrowserCliRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, req);

        // Responses
        let resp = BrowserCliResponse::Evaluated {
            result: Some("hello".into()),
            truncated: false,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains(r#""type":"evaluated""#));

        let resp = BrowserCliResponse::Waited;
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains(r#""type":"waited""#));

        let resp = BrowserCliResponse::Focused;
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains(r#""type":"focused""#));

        let resp = BrowserCliResponse::ScreenshotSaved {
            path: "/tmp/shot.png".into(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains(r#""type":"screenshotSaved""#));

        let resp = BrowserCliResponse::ConsoleEntries {
            entries: vec![BrowserConsoleEntry {
                level: "warn".into(),
                text: "careful".into(),
                at_ms: 1234,
            }],
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains(r#""type":"consoleEntries""#));

        let resp = BrowserCliResponse::CookieEntries {
            cookies: vec![BrowserCookieEntry {
                name: "c1".into(),
                value: "v1".into(),
            }],
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains(r#""type":"cookieEntries""#));

        let resp = BrowserCliResponse::StorageValue {
            value: Some("dark".into()),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains(r#""type":"storageValue""#));
    }

    #[tokio::test]
    async fn test_browser_cli_phase3_registered_missing_webview_round_trip() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());
        let token = "test-token";

        let registered = manager
            .register_session(CreateBrowserRequest {
                browser_id: None,
                workspace_id: None,
                worktree_path: None,
                url: "https://example.com".to_string(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .expect("register session");

        let b_id = &registered.browser_id;

        // Eval on registered session without webview -> WEBVIEW_NOT_FOUND
        let raw = format!(
            "{{\"command\":\"eval\",\"browserId\":\"{b_id}\",\"script\":\"1+1\",\"token\":\"{token}\"}}"
        );
        let resp = send_raw_line(app.handle().clone(), Arc::clone(&manager), token, &raw).await;
        assert_eq!(resp["type"], "error");
        assert!(resp["code"] == "WEBVIEW_NOT_FOUND" || resp["code"] == "BROWSER_WEBVIEW_NOT_FOUND");

        // Wait on registered session without webview -> WEBVIEW_NOT_FOUND
        let raw = format!(
            "{{\"command\":\"wait\",\"browserId\":\"{b_id}\",\"condition\":{{\"condition\":\"selector\",\"selector\":\"#none\"}},\"token\":\"{token}\"}}"
        );
        let resp = send_raw_line(app.handle().clone(), Arc::clone(&manager), token, &raw).await;
        assert_eq!(resp["type"], "error");
        assert!(resp["code"] == "WEBVIEW_NOT_FOUND" || resp["code"] == "BROWSER_WEBVIEW_NOT_FOUND");

        // Console on registered session without webview -> WEBVIEW_NOT_FOUND
        let raw =
            format!("{{\"command\":\"console\",\"browserId\":\"{b_id}\",\"token\":\"{token}\"}}");
        let resp = send_raw_line(app.handle().clone(), Arc::clone(&manager), token, &raw).await;
        assert_eq!(resp["type"], "error");
        assert!(resp["code"] == "WEBVIEW_NOT_FOUND" || resp["code"] == "BROWSER_WEBVIEW_NOT_FOUND");

        // Focus on registered session without webview -> WEBVIEW_NOT_FOUND
        let raw =
            format!("{{\"command\":\"focus\",\"browserId\":\"{b_id}\",\"token\":\"{token}\"}}");
        let resp = send_raw_line(app.handle().clone(), Arc::clone(&manager), token, &raw).await;
        assert_eq!(resp["type"], "error");
        assert!(resp["code"] == "WEBVIEW_NOT_FOUND" || resp["code"] == "BROWSER_WEBVIEW_NOT_FOUND");

        // Screenshot on registered session without webview -> WEBVIEW_NOT_FOUND
        let raw = format!(
            "{{\"command\":\"screenshot\",\"browserId\":\"{b_id}\",\"outPath\":\"/tmp/test.png\",\"token\":\"{token}\"}}"
        );
        let resp = send_raw_line(app.handle().clone(), Arc::clone(&manager), token, &raw).await;
        assert_eq!(resp["type"], "error");
        assert!(resp["code"] == "WEBVIEW_NOT_FOUND" || resp["code"] == "BROWSER_WEBVIEW_NOT_FOUND");

        // Cookies on registered session without webview -> WEBVIEW_NOT_FOUND
        let raw = format!(
            "{{\"command\":\"cookies\",\"browserId\":\"{b_id}\",\"action\":\"get\",\"token\":\"{token}\"}}"
        );
        let resp = send_raw_line(app.handle().clone(), Arc::clone(&manager), token, &raw).await;
        assert_eq!(resp["type"], "error");
        assert!(resp["code"] == "WEBVIEW_NOT_FOUND" || resp["code"] == "BROWSER_WEBVIEW_NOT_FOUND");

        // Storage on registered session without webview -> WEBVIEW_NOT_FOUND
        let raw = format!(
            "{{\"command\":\"storage\",\"browserId\":\"{b_id}\",\"kind\":\"local\",\"action\":\"get\",\"key\":\"k\",\"token\":\"{token}\"}}"
        );
        let resp = send_raw_line(app.handle().clone(), Arc::clone(&manager), token, &raw).await;
        assert_eq!(resp["type"], "error");
        assert!(resp["code"] == "WEBVIEW_NOT_FOUND" || resp["code"] == "BROWSER_WEBVIEW_NOT_FOUND");
    }

    #[tokio::test]
    async fn test_browser_cli_focus_returns_structured_errors() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());

        let registered = manager
            .register_session(CreateBrowserRequest {
                browser_id: None,
                workspace_id: None,
                worktree_path: None,
                url: "https://example.com".to_string(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .expect("register session");

        let b_id = &registered.browser_id;

        // (1) A registered session whose webview does not exist (mock app) produces
        // BrowserCliResponse::Error with code WEBVIEW_NOT_FOUND without panicking or silent success.
        let resp = execute_request(
            app.handle(),
            &manager,
            BrowserCliRequest::Focus {
                browser_id: b_id.clone(),
            },
        )
        .await;

        match resp {
            BrowserCliResponse::Error { code, message, .. } => {
                assert_eq!(code, "WEBVIEW_NOT_FOUND");
                assert!(!message.is_empty());
            }
            other => panic!("expected BrowserCliResponse::Error with WEBVIEW_NOT_FOUND, got {other:?}"),
        }

        // (2) An unknown browser id produces BrowserCliResponse::Error with BROWSER_NOT_FOUND.
        let resp_unknown = execute_request(
            app.handle(),
            &manager,
            BrowserCliRequest::Focus {
                browser_id: "unknown-browser-id".to_string(),
            },
        )
        .await;

        match resp_unknown {
            BrowserCliResponse::Error { code, message, .. } => {
                assert_eq!(code, "BROWSER_NOT_FOUND");
                assert!(!message.is_empty());
            }
            other => panic!("expected BrowserCliResponse::Error with BROWSER_NOT_FOUND, got {other:?}"),
        }

        // Also assert the wire round-trip via send_raw_line matching the model test helper.
        let token = "test-token";
        let raw_registered = format!(
            "{{\"command\":\"focus\",\"browserId\":\"{b_id}\",\"token\":\"{token}\"}}"
        );
        let wire_resp_registered =
            send_raw_line(app.handle().clone(), Arc::clone(&manager), token, &raw_registered).await;
        assert_eq!(wire_resp_registered["type"], "error");
        assert_eq!(wire_resp_registered["code"], "WEBVIEW_NOT_FOUND");

        let raw_unknown = format!(
            "{{\"command\":\"focus\",\"browserId\":\"unknown-browser-id\",\"token\":\"{token}\"}}"
        );
        let wire_resp_unknown =
            send_raw_line(app.handle().clone(), Arc::clone(&manager), token, &raw_unknown).await;
        assert_eq!(wire_resp_unknown["type"], "error");
        assert_eq!(wire_resp_unknown["code"], "BROWSER_NOT_FOUND");

        // (3) Both cases returned cleanly rather than unwinding (reaching here proves no panic).
    }

    #[tokio::test]
    async fn test_phase4_remote_attach_handshake_and_framed_mode() {
        use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());
        let registered = manager
            .register_session(CreateBrowserRequest {
                browser_id: Some("b1".into()),
                workspace_id: None,
                worktree_path: None,
                url: "https://example.com".into(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .expect("register session");
        let token = "test-token";

        // 1. Successful handshake -> framed mode round-trip
        let (client_stream, server_stream) = tokio::io::duplex(64 * 1024);
        let app_handle = app.handle().clone();
        let mgr = Arc::clone(&manager);
        let tok = Arc::new(token.to_string());
        let _server_task =
            tokio::spawn(
                async move { handle_connection(server_stream, app_handle, mgr, tok).await },
            );

        let (mut client_reader, mut client_writer) = tokio::io::split(client_stream);

        // Send one-line JSON handshake
        let handshake_raw = format!(r#"{{"command":"remoteAttach","token":"{token}"}}"#);
        let mut line = handshake_raw.into_bytes();
        line.push(b'\n');
        client_writer
            .write_all(&line)
            .await
            .expect("write handshake");
        client_writer.flush().await.expect("flush handshake");

        let mut resp_line = String::new();
        let mut buf_reader = BufReader::new(&mut client_reader);
        buf_reader
            .read_line(&mut resp_line)
            .await
            .expect("read handshake response");
        let resp_json: serde_json::Value =
            serde_json::from_str(resp_line.trim()).expect("parse handshake json");
        assert_eq!(
            resp_json["type"], "remoteAttached",
            "handshake must respond with remoteAttached"
        );
        assert_eq!(resp_json["protocolVersion"], 1);

        // Under R4: legacy BrowserCliRequest::List over framed IPC must be rejected with BROWSER_CLI_REQUEST_INVALID!
        let list_req_bytes = serde_json::to_vec(&BrowserCliRequest::List).unwrap();
        let framed_req = crate::browser::remote_bridge_protocol::encode_ipc_frame(
            crate::browser::remote_bridge_protocol::IPC_CONTENT_TYPE_JSON,
            &list_req_bytes,
        )
        .expect("encode framed request");
        client_writer
            .write_all(&framed_req)
            .await
            .expect("send framed request");
        client_writer.flush().await.expect("flush framed request");

        // Read framed response
        let mut resp_header = [0u8; 5];
        buf_reader
            .read_exact(&mut resp_header)
            .await
            .expect("read framed response header");
        let resp_payload_len = u32::from_le_bytes([
            resp_header[0],
            resp_header[1],
            resp_header[2],
            resp_header[3],
        ]) as usize;
        let resp_content_type = resp_header[4];
        assert_eq!(
            resp_content_type,
            crate::browser::remote_bridge_protocol::IPC_CONTENT_TYPE_JSON
        );

        let mut resp_payload = vec![0u8; resp_payload_len];
        buf_reader
            .read_exact(&mut resp_payload)
            .await
            .expect("read framed payload");
        let framed_resp: BrowserCliResponse =
            serde_json::from_slice(&resp_payload).expect("parse framed response");
        assert!(matches!(
            framed_resp,
            BrowserCliResponse::Error {
                ref code,
                ..
            } if code == "BROWSER_CLI_REQUEST_INVALID"
        ));

        // And valid RemoteBrowserOperation (e.g. GetState) succeeds over framed IPC
        let get_state_bytes = serde_json::to_vec(&RemoteBrowserOperation::GetState {
            browser_id: registered.browser_id.clone(),
        })
        .unwrap();
        let framed_get_state = crate::browser::remote_bridge_protocol::encode_ipc_frame(
            crate::browser::remote_bridge_protocol::IPC_CONTENT_TYPE_JSON,
            &get_state_bytes,
        )
        .unwrap();
        client_writer.write_all(&framed_get_state).await.unwrap();
        client_writer.flush().await.unwrap();

        buf_reader.read_exact(&mut resp_header).await.unwrap();
        let len = u32::from_le_bytes([
            resp_header[0],
            resp_header[1],
            resp_header[2],
            resp_header[3],
        ]) as usize;
        let mut resp_buf = vec![0u8; len];
        buf_reader.read_exact(&mut resp_buf).await.unwrap();
        let resp_val: serde_json::Value = serde_json::from_slice(&resp_buf).unwrap();
        assert_eq!(resp_val["type"], "remoteResult");
        assert_eq!(resp_val["status"], "ok");

        // 2. Unauthenticated handshake rejected before framed mode
        let (bad_client_stream, bad_server_stream) = tokio::io::duplex(4096);
        let app_handle2 = app.handle().clone();
        let mgr2 = Arc::clone(&manager);
        let tok2 = Arc::new(token.to_string());
        let _server_task2 = tokio::spawn(async move {
            handle_connection(bad_server_stream, app_handle2, mgr2, tok2).await
        });

        let (bad_reader, mut bad_writer) = tokio::io::split(bad_client_stream);
        let bad_handshake = r#"{"command":"remoteAttach","token":"wrong-token"}"#;
        let mut bad_line = bad_handshake.as_bytes().to_vec();
        bad_line.push(b'\n');
        bad_writer.write_all(&bad_line).await.unwrap();
        bad_writer.flush().await.unwrap();

        let mut bad_resp_line = String::new();
        let mut bad_buf_reader = BufReader::new(bad_reader);
        bad_buf_reader.read_line(&mut bad_resp_line).await.unwrap();
        let bad_resp: serde_json::Value = serde_json::from_str(bad_resp_line.trim()).unwrap();
        assert_eq!(bad_resp["type"], "error");
        assert_eq!(bad_resp["code"], "BROWSER_CLI_UNAUTHORIZED");
        // Server must close connection, EOF on next read
        let mut eof_check = [0u8; 1];
        let n = bad_buf_reader.read(&mut eof_check).await.unwrap();
        assert_eq!(n, 0, "unauthenticated connection must close immediately");

        // 3. Verify public DTOs do not disclose local credential or internal paths
        let summary = BrowserSessionSummary {
            browser_id: "b1".into(),
            webview_label: "label".into(),
            workspace_id: Some("ws".into()),
            profile_id: BrowserProfileId::Default,
            url: "https://example.com".into(),
            title: Some("Title".into()),
            visible: true,
        };
        let summary_json = serde_json::to_string(&summary).unwrap();
        assert!(!summary_json.contains(token));
        assert!(!summary_json.contains("/Users/"));
        assert!(!summary_json.contains("worktreePath"));
    }

    #[test]
    fn test_phase4_owner_reclaim_invalidates_remote_driver() {
        let broker = crate::browser::remote_driver::RemoteDriverBroker::new();

        // 1. Remote viewer claims driver lease
        let lease = broker
            .claim("dev1", "conn1", "sub1", "b1", true)
            .expect("claim driver");
        assert_eq!(lease.device_id, "dev1");

        // Validate active lease succeeds
        assert!(broker
            .validate_lease("b1", lease.lease_epoch, "dev1", "conn1")
            .is_ok());

        // 2. Desktop owner revokes / reclaims control
        let new_epoch = broker.desktop_reclaim();
        assert_ne!(new_epoch, lease.lease_epoch);

        // 3. Prior remote lease is now rejected with DesktopReclaimed
        let err = broker
            .validate_lease("b1", lease.lease_epoch, "dev1", "conn1")
            .unwrap_err();
        assert_eq!(
            err,
            crate::browser::remote_driver::RemoteDriverError::DesktopReclaimed
        );
    }

    #[tokio::test]
    async fn test_r4_framed_ipc_eliminates_legacy_fallback_and_rejects_legacy_commands() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());
        let (mut client_io, server_io) = tokio::io::duplex(4096);

        let app_handle = app.handle().clone();
        let mgr_clone = Arc::clone(&manager);
        let loop_handle = tokio::spawn(async move {
            let (mut reader, mut writer) = tokio::io::split(server_io);
            run_framed_ipc_loop(&mut reader, &mut writer, &app_handle, &mgr_clone).await
        });

        // 1. Send legacy "list" command in framed IPC mode
        let legacy_list = br#"{"command":"list"}"#;
        let frame = crate::browser::remote_bridge_protocol::encode_ipc_frame(
            crate::browser::remote_bridge_protocol::IPC_CONTENT_TYPE_JSON,
            legacy_list,
        )
        .unwrap();
        client_io.write_all(&frame).await.unwrap();

        // Read framed response
        let mut header = [0u8; 5];
        client_io.read_exact(&mut header).await.unwrap();
        let payload_len = u32::from_le_bytes([header[0], header[1], header[2], header[3]]) as usize;
        let mut resp_payload = vec![0u8; payload_len];
        client_io.read_exact(&mut resp_payload).await.unwrap();
        let resp: serde_json::Value = serde_json::from_slice(&resp_payload).unwrap();

        // MUST be error BROWSER_CLI_REQUEST_INVALID, NOT legacy list output!
        assert_eq!(resp["type"], "error");
        assert_eq!(resp["code"], "BROWSER_CLI_REQUEST_INVALID");

        // 2. Send legacy "close" command in framed IPC mode
        let legacy_close = br#"{"command":"close","browserId":"b1"}"#;
        let frame2 = crate::browser::remote_bridge_protocol::encode_ipc_frame(
            crate::browser::remote_bridge_protocol::IPC_CONTENT_TYPE_JSON,
            legacy_close,
        )
        .unwrap();
        client_io.write_all(&frame2).await.unwrap();

        client_io.read_exact(&mut header).await.unwrap();
        let payload_len2 =
            u32::from_le_bytes([header[0], header[1], header[2], header[3]]) as usize;
        let mut resp_payload2 = vec![0u8; payload_len2];
        client_io.read_exact(&mut resp_payload2).await.unwrap();
        let resp2: serde_json::Value = serde_json::from_slice(&resp_payload2).unwrap();

        assert_eq!(resp2["type"], "error");
        assert_eq!(resp2["code"], "BROWSER_CLI_REQUEST_INVALID");

        // 3. Send invalid / unparseable payload
        let bad_payload = b"not-json-at-all";
        let frame3 = crate::browser::remote_bridge_protocol::encode_ipc_frame(
            crate::browser::remote_bridge_protocol::IPC_CONTENT_TYPE_JSON,
            bad_payload,
        )
        .unwrap();
        client_io.write_all(&frame3).await.unwrap();

        client_io.read_exact(&mut header).await.unwrap();
        let payload_len3 =
            u32::from_le_bytes([header[0], header[1], header[2], header[3]]) as usize;
        let mut resp_payload3 = vec![0u8; payload_len3];
        client_io.read_exact(&mut resp_payload3).await.unwrap();
        let resp3: serde_json::Value = serde_json::from_slice(&resp_payload3).unwrap();

        assert_eq!(resp3["type"], "error");
        assert_eq!(resp3["code"], "BROWSER_CLI_REQUEST_INVALID");

        drop(client_io);
        let _ = loop_handle.await;
    }

    #[tokio::test]
    async fn test_r9_remote_reference_requires_valid_snapshot_and_rejects_legacy_targets() {
        use crate::browser::BrowserAutomationTarget;

        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());
        let b = manager
            .register_session(CreateBrowserRequest {
                browser_id: Some("b-r9".into()),
                workspace_id: None,
                worktree_path: None,
                url: "https://example.com".into(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .expect("register session");

        // Record a legacy automation target
        let legacy_targets = vec![BrowserAutomationTarget {
            reference: "btn-submit".into(),
            selector: "#legacy-btn".into(),
        }];
        manager
            .record_automation_targets(&b.browser_id, b.generation, legacy_targets)
            .unwrap();

        // 1. Click with reference but missing snapshot_id -> validation returns BROWSER_INVALID_SNAPSHOT
        let click_no_snap = RemoteBrowserOperation::Click {
            browser_id: b.browser_id.clone(),
            reference: Some("btn-submit".into()),
            snapshot_id: None,
            map_revision: None,
            u: None,
            v: None,
            stream_id: None,
            sequence_number: None,
            document_generation: None,
            viewport_revision: None,
            capture_rect: None,
            geometry_source: None,
            x: None,
            y: None,
        };
        let err_click = click_no_snap.validate().unwrap_err();
        assert_eq!(
            ipc_error_code_string(err_click.code),
            "BROWSER_INVALID_SNAPSHOT"
        );

        // 2. Fill with missing snapshot_id -> validation returns BROWSER_INVALID_SNAPSHOT
        let fill_no_snap = RemoteBrowserOperation::Fill {
            browser_id: b.browser_id.clone(),
            reference: "btn-submit".into(),
            value: "test".into(),
            snapshot_id: None,
            map_revision: None,
        };
        let err_fill = fill_no_snap.validate().unwrap_err();
        assert_eq!(
            ipc_error_code_string(err_fill.code),
            "BROWSER_INVALID_SNAPSHOT"
        );

        // 3. A direct Click or Fill that passes validation is refused before it can resolve
        // any target: direct mutations bypass the driver-lease guard (only `execute` runs it),
        // so no legacy automation target can be reached through them.
        let click_fake_snap = RemoteBrowserOperation::Click {
            browser_id: b.browser_id.clone(),
            reference: Some("btn-submit".into()),
            snapshot_id: Some("fake-snap-id".into()),
            map_revision: Some(1),
            u: None,
            v: None,
            stream_id: None,
            sequence_number: None,
            document_generation: None,
            viewport_revision: None,
            capture_rect: None,
            geometry_source: None,
            x: None,
            y: None,
        };
        let exec_err = execute_remote_operation(&app.handle().clone(), &manager, click_fake_snap)
            .await
            .unwrap_err();
        assert_eq!(ipc_error_code_string(exec_err.code), "BROWSER_FORBIDDEN");

        let fill_fake_snap = RemoteBrowserOperation::Fill {
            browser_id: b.browser_id.clone(),
            reference: "btn-submit".into(),
            value: "test".into(),
            snapshot_id: Some("fake-snap-id".into()),
            map_revision: Some(1),
        };
        let exec_err2 = execute_remote_operation(&app.handle().clone(), &manager, fill_fake_snap)
            .await
            .unwrap_err();
        assert_eq!(ipc_error_code_string(exec_err2.code), "BROWSER_FORBIDDEN");
    }

    #[tokio::test]
    async fn test_r8_r9_click_frame_metadata_fencing_and_decimal_string_conversion() {
        // 1. Verify JSON deserialization of decimal string vs u64 for map_revision, document_generation, viewport_revision
        let json_decimal_str = serde_json::json!({
            "operation": "click",
            "browserId": "b1",
            "reference": "btn-ok",
            "snapshotId": "snap-123",
            "mapRevision": "42",
            "streamId": 10,
            "sequenceNumber": 100,
            "documentGeneration": "5",
            "viewportRevision": "8"
        });
        let op_from_str: RemoteBrowserOperation = serde_json::from_value(json_decimal_str).unwrap();
        match op_from_str {
            RemoteBrowserOperation::Click {
                map_revision,
                stream_id,
                sequence_number,
                document_generation,
                viewport_revision,
                ..
            } => {
                assert_eq!(map_revision, Some(42));
                assert_eq!(stream_id, Some(10));
                assert_eq!(sequence_number, Some(100));
                assert_eq!(document_generation, Some(5));
                assert_eq!(viewport_revision, Some(8));
            }
            _ => panic!("Expected Click operation"),
        }

        let json_numbers = serde_json::json!({
            "operation": "click",
            "browserId": "b1",
            "reference": "btn-ok",
            "snapshotId": "snap-123",
            "mapRevision": 42,
            "streamId": 10,
            "sequenceNumber": 100,
            "documentGeneration": 5,
            "viewportRevision": 8
        });
        let op_from_num: RemoteBrowserOperation = serde_json::from_value(json_numbers).unwrap();
        match op_from_num {
            RemoteBrowserOperation::Click {
                map_revision,
                document_generation,
                viewport_revision,
                ..
            } => {
                assert_eq!(map_revision, Some(42));
                assert_eq!(document_generation, Some(5));
                assert_eq!(viewport_revision, Some(8));
            }
            _ => panic!("Expected Click operation"),
        }

        // Fill with decimal string mapRevision
        let fill_decimal = serde_json::json!({
            "operation": "fill",
            "browserId": "b1",
            "reference": "txt-name",
            "value": "Ferryx",
            "snapshotId": "snap-123",
            "mapRevision": "99"
        });
        let fill_op: RemoteBrowserOperation = serde_json::from_value(fill_decimal).unwrap();
        match fill_op {
            RemoteBrowserOperation::Fill { map_revision, .. } => {
                assert_eq!(map_revision, Some(99));
            }
            _ => panic!("Expected Fill operation"),
        }

        // Invalid non-decimal string fails deserialization (no legacy fallback)
        let bad_json = serde_json::json!({
            "operation": "click",
            "browserId": "b1",
            "reference": "btn-ok",
            "snapshotId": "snap-123",
            "mapRevision": "invalid-non-number"
        });
        assert!(serde_json::from_value::<RemoteBrowserOperation>(bad_json).is_err());

        // 2. Test geometry / generation fencing in Click execution
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());
        let b = manager
            .register_session(crate::browser::model::CreateBrowserRequest {
                browser_id: Some("fenced-b1".into()),
                workspace_id: None,
                worktree_path: None,
                url: "https://example.com".into(),
                profile: None,
                zoom_factor: None,
                bounds: Some(crate::browser::model::LogicalRect {
                    x: 0.0,
                    y: 0.0,
                    width: 800.0,
                    height: 600.0,
                }),
                visible: Some(true),
            })
            .unwrap();

        // A direct click is refused before any fencing runs (its stale generation and stale
        // viewport never get the chance to matter); `execute` fences clicks instead.
        let stale_gen_click = RemoteBrowserOperation::Click {
            browser_id: b.browser_id.clone(),
            reference: None,
            snapshot_id: None,
            map_revision: None,
            u: Some(0.5),
            v: Some(0.5),
            stream_id: Some(1),
            sequence_number: Some(1),
            document_generation: Some(b.generation + 99), // Stale!
            viewport_revision: Some(1),
            capture_rect: None,
            geometry_source: None,
            x: None,
            y: None,
        };
        let err_gen = execute_remote_operation(&app.handle().clone(), &manager, stale_gen_click)
            .await
            .unwrap_err();
        assert_eq!(ipc_error_code_string(err_gen.code), "BROWSER_FORBIDDEN");

        let stale_vp_click = RemoteBrowserOperation::Click {
            browser_id: b.browser_id.clone(),
            reference: None,
            snapshot_id: None,
            map_revision: None,
            u: Some(0.5),
            v: Some(0.5),
            stream_id: Some(1),
            sequence_number: Some(1),
            document_generation: Some(b.generation),
            viewport_revision: Some(999), // Stale!
            capture_rect: None,
            geometry_source: None,
            x: None,
            y: None,
        };
        let err_vp = execute_remote_operation(&app.handle().clone(), &manager, stale_vp_click)
            .await
            .unwrap_err();
        assert_eq!(ipc_error_code_string(err_vp.code), "BROWSER_FORBIDDEN");

        // 3. RemoteBrowserOperation::Snapshot validation
        let snap_empty = RemoteBrowserOperation::Snapshot {
            browser_id: "  ".into(),
        };
        assert!(snap_empty.validate().is_err());

        let snap_valid = RemoteBrowserOperation::Snapshot {
            browser_id: "fenced-b1".into(),
        };
        assert!(snap_valid.validate().is_ok());
    }

    #[test]
    fn test_secondary_wait_function_condition_consent_and_size_limit() {
        use crate::browser::model::BrowserWaitCondition;

        // 1. Function condition without approval must be rejected
        let wait_no_approval = RemoteBrowserOperation::Wait {
            browser_id: "b1".into(),
            condition: BrowserWaitCondition::Function {
                script: "() => true".into(),
            },
            timeout_ms: None,
            has_approval: false,
        };
        let err_no_appr = wait_no_approval.validate().unwrap_err();
        assert!(
            err_no_appr.message.contains("approval")
                || format!("{:?}", err_no_appr).contains("approval")
        );

        // 2. Function condition with script > 32 KiB must be rejected
        let huge_script = "x".repeat(32 * 1024 + 1);
        let wait_too_large = RemoteBrowserOperation::Wait {
            browser_id: "b1".into(),
            condition: BrowserWaitCondition::Function {
                script: huge_script,
            },
            timeout_ms: None,
            has_approval: true,
        };
        let err_too_large = wait_too_large.validate().unwrap_err();
        assert!(
            err_too_large.message.contains("large")
                || format!("{:?}", err_too_large).contains("large")
        );

        // 3. Function condition with approval and script <= 32 KiB must succeed
        let wait_valid = RemoteBrowserOperation::Wait {
            browser_id: "b1".into(),
            condition: BrowserWaitCondition::Function {
                script: "document.title === 'Done'".into(),
            },
            timeout_ms: None,
            has_approval: true,
        };
        assert!(wait_valid.validate().is_ok());

        // 4. Non-function condition succeeds even with has_approval: false
        let wait_selector = RemoteBrowserOperation::Wait {
            browser_id: "b1".into(),
            condition: BrowserWaitCondition::Selector {
                selector: "#ready-btn".into(),
            },
            timeout_ms: None,
            has_approval: false,
        };
        assert!(wait_selector.validate().is_ok());

        let wait_text = RemoteBrowserOperation::Wait {
            browser_id: "b1".into(),
            condition: BrowserWaitCondition::Text {
                text: "Success".into(),
            },
            timeout_ms: None,
            has_approval: false,
        };
        assert!(wait_text.validate().is_ok());
    }

    #[test]
    fn test_r4_click_consumes_authoritative_geometry_and_snapshot_returns_catalogue() {
        // 1. Click consumes WS-provided authoritative geometry (captureRect, geometrySource, x, y)
        let click_json = serde_json::json!({
            "operation": "click",
            "browserId": "b1",
            "u": 0.5,
            "v": 0.5,
            "streamId": 1,
            "sequenceNumber": 4,
            "documentGeneration": "1",
            "viewportRevision": "1",
            "captureRect": { "x": 10.0, "y": 20.0, "width": 640.0, "height": 480.0 },
            "geometrySource": "wkSnapshot",
            "x": 330.0,
            "y": 260.0,
        });
        let click_op: RemoteBrowserOperation = serde_json::from_value(click_json).unwrap();
        match &click_op {
            RemoteBrowserOperation::Click {
                capture_rect,
                geometry_source,
                x,
                y,
                ..
            } => {
                assert_eq!(
                    capture_rect.as_ref().map(|r| (r.x, r.y, r.width, r.height)),
                    Some((10.0, 20.0, 640.0, 480.0))
                );
                assert_eq!(geometry_source.as_deref(), Some("wkSnapshot"));
                assert_eq!(*x, Some(330.0));
                assert_eq!(*y, Some(260.0));
            }
            _ => panic!("Expected Click operation"),
        }

        // 2. Snapshot catalogue extraction requires elements array, not just elementsCount
        let legacy_snap = serde_json::json!({
            "snapshotId": "s1",
            "mapRevision": "1",
            "documentGeneration": "1",
            "elementsCount": 5,
        });
        let legacy_res = crate::remote::browser_ws::snapshot_catalogue_from_backend(&legacy_snap);
        assert!(
            legacy_res.is_err(),
            "Legacy count-only snapshot payload must be rejected by catalogue parser"
        );

        let full_snap = serde_json::json!({
            "snapshotId": "s1",
            "mapRevision": "1",
            "documentGeneration": "1",
            "elementsCount": 1,
            "elements": [
                {
                    "ref": "btn-1",
                    "role": "button",
                    "name": "Submit",
                    "tagName": "button",
                }
            ]
        });
        let res = crate::remote::browser_ws::snapshot_catalogue_from_backend(&full_snap);
        assert!(
            res.is_ok(),
            "Snapshot backend payload with elements catalogue must succeed"
        );
        let (snap_id, map_rev, _, elements) = res.unwrap();
        assert_eq!(snap_id, "s1");
        assert_eq!(map_rev, "1");
        assert_eq!(elements.len(), 1);
        assert_eq!(elements[0]["ref"], "btn-1");
    }

    #[test]
    fn test_r5_15_eval_wait_lease_approval_and_normalization() {
        use crate::browser::model::BrowserWaitCondition;

        // 1. Client sending hasApproval: true in JSON is NOT trusted (must be rejected)
        let eval_json_untrusted = serde_json::json!({
            "operation": "eval",
            "browserId": "b1",
            "script": "2 + 2",
            "hasApproval": true
        });
        let eval_op: RemoteBrowserOperation = serde_json::from_value(eval_json_untrusted).unwrap();
        assert!(
            eval_op.validate().is_err(),
            "Client-supplied hasApproval boolean must not grant approval"
        );

        // 2. Direct Rust construction with has_approval: true succeeds
        let eval_trusted = RemoteBrowserOperation::Eval {
            browser_id: "b1".into(),
            script: "2 + 2".into(),
            has_approval: true,
        };
        assert!(eval_trusted.validate().is_ok());

        // 3. String wait condition with timeoutMs is accepted and preserved
        let wait_json = serde_json::json!({
            "operation": "wait",
            "browserId": "b1",
            "condition": "document.title !== ''",
            "timeoutMs": 3500
        });
        let wait_op: RemoteBrowserOperation = serde_json::from_value(wait_json).unwrap();
        match wait_op {
            RemoteBrowserOperation::Wait {
                condition,
                timeout_ms,
                ..
            } => {
                assert_eq!(
                    condition,
                    BrowserWaitCondition::Function {
                        script: "document.title !== ''".into()
                    }
                );
                assert_eq!(timeout_ms, Some(3500));
            }
            _ => panic!("Expected Wait operation"),
        }
    }

    #[tokio::test]
    async fn test_r6_2_execute_envelope_routes_all_commands_and_fences_writes() {
        use crate::browser::BrowserRemoteService;
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());
        let broker = Arc::new(crate::browser::remote_driver::RemoteDriverBroker::new());
        let service = Arc::new(BrowserRemoteService::new(
            (*manager).clone(),
            Arc::clone(&broker),
        ));
        use tauri::Manager;
        app.manage(Arc::clone(&service));
        app.manage(Arc::clone(&broker));

        manager
            .register_session(crate::browser::model::CreateBrowserRequest {
                browser_id: Some("b-exec-test".into()),
                workspace_id: Some("ws1".into()),
                worktree_path: None,
                url: "https://example.com".into(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .unwrap();

        // 1. Snapshot / read-only is authorized WITHOUT a driver lease
        let snap_op = RemoteBrowserOperation::Execute {
            browser_id: "b-exec-test".into(),
            command: "getState".into(),
            params: None,
            document_generation: Some("1".into()),
            browser_instance_id: None,
            desktop_epoch: None,
            lease_epoch: None,
            device_id: None,
            connection_id: None,
        };
        let res_state = execute_remote_operation(&app.handle().clone(), &manager, snap_op).await;
        assert!(
            res_state.is_ok(),
            "getState/read-only must not require a driver lease"
        );

        // 2. Click (mutation) WITHOUT a driver lease must fail with BROWSER_FORBIDDEN
        let click_no_lease = RemoteBrowserOperation::Execute {
            browser_id: "b-exec-test".into(),
            command: "click".into(),
            params: Some(serde_json::json!({ "selector": "#btn" })),
            document_generation: Some("1".into()),
            browser_instance_id: None,
            desktop_epoch: None,
            lease_epoch: None,
            device_id: None,
            connection_id: None,
        };
        let err_click = execute_remote_operation(&app.handle().clone(), &manager, click_no_lease)
            .await
            .unwrap_err();
        assert_ne!(
            err_click.message, "unsupported bridge execute command: click",
            "click must be routed through guarded executor, not rejected as unsupported"
        );
        assert_eq!(ipc_error_code_string(err_click.code), "BROWSER_FORBIDDEN");
    }

    #[tokio::test]
    async fn test_r6_6_execute_preserves_typed_target_not_found_and_stale_frame() {
        use crate::browser::BrowserRemoteService;
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());
        let broker = Arc::new(crate::browser::remote_driver::RemoteDriverBroker::new());
        let service = Arc::new(BrowserRemoteService::new(
            (*manager).clone(),
            Arc::clone(&broker),
        ));
        use tauri::Manager;
        app.manage(Arc::clone(&service));
        app.manage(Arc::clone(&broker));

        manager
            .register_session(crate::browser::model::CreateBrowserRequest {
                browser_id: Some("b-err-test".into()),
                workspace_id: Some("ws1".into()),
                worktree_path: None,
                url: "https://example.com".into(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .unwrap();

        // Claim lease so mutation guard passes
        let lease = broker
            .claim("dev1", "conn1", "sub1", "b-err-test", true)
            .unwrap();
        let lease_epoch_str = lease.lease_epoch.to_string();
        let instance_id = manager.get_instance_id("b-err-test").unwrap();

        // 1. Viewport revision mismatch -> must produce BROWSER_STALE_FRAME
        let stale_click = RemoteBrowserOperation::Execute {
            browser_id: "b-err-test".into(),
            command: "click".into(),
            params: Some(serde_json::json!({ "selector": "#btn", "viewportRevision": 9999 })),
            document_generation: Some("1".into()),
            browser_instance_id: Some(instance_id.clone()),
            desktop_epoch: Some("1".into()),
            lease_epoch: Some(lease_epoch_str.clone()),
            device_id: Some("dev1".into()),
            connection_id: Some("conn1".into()),
        };
        let err_stale = execute_remote_operation(&app.handle().clone(), &manager, stale_click)
            .await
            .unwrap_err();
        assert_eq!(ipc_error_code_string(err_stale.code), "BROWSER_STALE_FRAME");

        // 2. Snapshot target missing -> must produce BROWSER_TARGET_NOT_FOUND
        let missing_target_click = RemoteBrowserOperation::Execute {
            browser_id: "b-err-test".into(),
            command: "click".into(),
            params: Some(
                serde_json::json!({ "reference": "missing-ref", "snapshotId": "snap1", "mapRevision": 1 }),
            ),
            document_generation: Some("1".into()),
            browser_instance_id: Some(instance_id),
            desktop_epoch: Some("1".into()),
            lease_epoch: Some(lease_epoch_str.clone()),
            device_id: Some("dev1".into()),
            connection_id: Some("conn1".into()),
        };
        let err_target =
            execute_remote_operation(&app.handle().clone(), &manager, missing_target_click)
                .await
                .unwrap_err();
        let target_code = ipc_error_code_string(err_target.code);
        assert!(
            target_code == "BROWSER_TARGET_NOT_FOUND" || target_code == "BROWSER_INVALID_REQUEST",
            "Target resolution failure must preserve structured code, got {}",
            target_code
        );
        assert_ne!(target_code, "BROWSER_EXECUTION_FAILED");
    }

    #[tokio::test]
    async fn test_r6_1_gui_framed_subscription_forwards_frames_and_cleans_up() {
        use crate::browser::BrowserRemoteService;
        use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());
        let broker = Arc::new(crate::browser::remote_driver::RemoteDriverBroker::new());
        let service = Arc::new(BrowserRemoteService::new(
            (*manager).clone(),
            Arc::clone(&broker),
        ));
        use tauri::Manager;
        app.manage(Arc::clone(&service));
        app.manage(Arc::clone(&broker));

        manager
            .register_session(crate::browser::model::CreateBrowserRequest {
                browser_id: Some("b-gui-stream".into()),
                workspace_id: Some("ws1".into()),
                worktree_path: None,
                url: "https://example.com".into(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .unwrap();

        let (client_stream, server_stream) = tokio::io::duplex(64 * 1024);
        let expected_token = Arc::new("test-token".to_string());
        let app_handle = app.handle().clone();
        let mgr_clone = Arc::clone(&manager);

        let server_task = tokio::spawn(async move {
            let _ = handle_connection(server_stream, app_handle, mgr_clone, expected_token).await;
        });

        let (reader, mut writer) = tokio::io::split(client_stream);
        let mut reader = BufReader::new(reader);

        // 1. Send handshake
        let handshake = serde_json::json!({
            "command": "remoteAttach",
            "token": "test-token"
        });
        let mut hs_bytes = serde_json::to_vec(&handshake).unwrap();
        hs_bytes.push(b'\n');
        writer.write_all(&hs_bytes).await.unwrap();
        writer.flush().await.unwrap();

        let mut hs_line = String::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(3),
            reader.read_line(&mut hs_line),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(hs_line.contains("remoteAttached"));

        // 2. Send SubscribeViewer
        let sub_op = RemoteBrowserOperation::SubscribeViewer {
            browser_id: "b-gui-stream".into(),
            device_id: "dev-sub".into(),
            viewer_instance_id: "view-sub".into(),
            options: None,
        };
        let sub_bytes = serde_json::to_vec(&sub_op).unwrap();
        let frame = crate::browser::remote_bridge_protocol::encode_ipc_frame(
            crate::browser::remote_bridge_protocol::IPC_CONTENT_TYPE_JSON,
            &sub_bytes,
        )
        .unwrap();
        writer.write_all(&frame).await.unwrap();
        writer.flush().await.unwrap();

        // 3. Read SubscribeViewer response JSON
        let mut header = [0u8; 5];
        tokio::time::timeout(
            std::time::Duration::from_secs(3),
            reader.read_exact(&mut header),
        )
        .await
        .unwrap()
        .unwrap();
        let p_len = u32::from_le_bytes([header[0], header[1], header[2], header[3]]) as usize;
        let mut p_buf = vec![0u8; p_len];
        tokio::time::timeout(
            std::time::Duration::from_secs(3),
            reader.read_exact(&mut p_buf),
        )
        .await
        .unwrap()
        .unwrap();
        let resp_val: serde_json::Value = serde_json::from_slice(&p_buf).unwrap();
        assert_eq!(resp_val["status"], "ok");

        // 4. Producer is now active. The server registers its frame receiver before it
        // writes the subscribe response, so having read the response proves it exists.
        assert!(service.is_producer_active("b-gui-stream"));
        let sender = service
            .frame_broadcaster()
            .lock()
            .get("b-gui-stream")
            .cloned()
            .unwrap();
        assert!(
            sender.receiver_count() > 0,
            "Server must have subscribed to frames broadcaster"
        );

        let dummy_frame = vec![0xCA, 0xFE, 0xBA, 0xBE];
        sender.send(dummy_frame.clone()).unwrap();

        // 5. Client receives the image frame over the persistent connection
        tokio::time::timeout(
            std::time::Duration::from_secs(3),
            reader.read_exact(&mut header),
        )
        .await
        .unwrap()
        .unwrap();
        let img_len = u32::from_le_bytes([header[0], header[1], header[2], header[3]]) as usize;
        assert_eq!(
            header[4],
            crate::browser::remote_bridge_protocol::IPC_CONTENT_TYPE_IMAGE
        );
        let mut img_buf = vec![0u8; img_len];
        tokio::time::timeout(
            std::time::Duration::from_secs(3),
            reader.read_exact(&mut img_buf),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(img_buf, dummy_frame);

        // 6. Client shuts down and drops connection
        let _ = writer.shutdown().await;
        drop(writer);
        drop(reader);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(3), server_task)
            .await
            .unwrap();

        // Producer pauses on disconnect cleanup
        assert!(!service.is_producer_active("b-gui-stream"));
    }

    #[tokio::test]
    async fn tab_switch_reports_a_failed_focus_instead_of_a_silent_success() {
        // Given a registered tab whose webview does not exist, when the CLI switches to it, then
        // the focus failure reaches the caller as a structured error, not as a successful switch.
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());
        manager
            .register_session(CreateBrowserRequest {
                browser_id: Some("tab-switch-focus-fails".into()),
                workspace_id: None,
                worktree_path: None,
                url: "https://example.com/".into(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .expect("register session");

        let response = execute_request(
            app.handle(),
            &manager,
            BrowserCliRequest::TabSwitch {
                browser_id: Some("tab-switch-focus-fails".into()),
                index: None,
            },
        )
        .await;

        match response {
            BrowserCliResponse::Error { code, message, .. } => {
                assert_eq!(code, "WEBVIEW_NOT_FOUND");
                assert!(!message.is_empty());
            }
            other => panic!("expected a structured focus failure, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn tab_switch_reports_an_out_of_range_index() {
        // Given two tabs, when the CLI asks for index 5, then the range check answers with the
        // documented code instead of switching to an arbitrary tab.
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());
        for browser_id in ["tab-range-1", "tab-range-2"] {
            manager
                .register_session(CreateBrowserRequest {
                    browser_id: Some(browser_id.into()),
                    workspace_id: None,
                    worktree_path: None,
                    url: "https://example.com/".into(),
                    profile: None,
                    zoom_factor: None,
                    bounds: None,
                    visible: Some(true),
                })
                .expect("register session");
        }

        let response = execute_request(
            app.handle(),
            &manager,
            BrowserCliRequest::TabSwitch {
                browser_id: None,
                index: Some(5),
            },
        )
        .await;

        match response {
            BrowserCliResponse::Error { code, .. } => {
                assert_eq!(code, "BROWSER_TAB_OUT_OF_RANGE");
            }
            other => panic!("expected an out-of-range error, got {other:?}"),
        }
    }

    fn register_remote_session(manager: &BrowserManager, browser_id: &str) -> crate::browser::BrowserState {
        manager
            .register_session(CreateBrowserRequest {
                browser_id: Some(browser_id.into()),
                workspace_id: Some("ws1".into()),
                worktree_path: None,
                url: "https://example.com/".into(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .expect("register session")
    }

    async fn read_ipc_frame<R: tokio::io::AsyncRead + Unpin>(reader: &mut R) -> (u8, Vec<u8>) {
        use tokio::io::AsyncReadExt;
        let mut header = [0u8; 5];
        reader.read_exact(&mut header).await.expect("frame header");
        let len = u32::from_le_bytes([header[0], header[1], header[2], header[3]]) as usize;
        let mut payload = vec![0u8; len];
        reader.read_exact(&mut payload).await.expect("frame payload");
        (header[4], payload)
    }

    fn json_frame(op: &RemoteBrowserOperation) -> Vec<u8> {
        crate::browser::remote_bridge_protocol::encode_ipc_frame(
            crate::browser::remote_bridge_protocol::IPC_CONTENT_TYPE_JSON,
            &serde_json::to_vec(op).expect("serialize op"),
        )
        .expect("encode frame")
    }

    #[test]
    fn f1_cli_and_remote_wait_timeouts_are_clamped() {
        // Given an absurd timeout, when it is clamped, then it is bounded by MAX_WAIT_TIMEOUT
        // (u64::MAX ms would overflow the deadline arithmetic downstream).
        let max_ms = MAX_WAIT_TIMEOUT.as_millis() as u64;
        assert_eq!(max_ms, 120_000);
        assert_eq!(clamp_wait_timeout_ms(u64::MAX), max_ms);
        assert_eq!(clamp_wait_timeout_ms(3_500), 3_500);
        let clamped = clamp_cli_wait_condition(BrowserWaitCondition::WithTimeout {
            inner: Box::new(BrowserWaitCondition::Selector {
                selector: "#ready".into(),
            }),
            timeout_ms: u64::MAX,
        });
        assert_eq!(
            clamped,
            BrowserWaitCondition::WithTimeout {
                inner: Box::new(BrowserWaitCondition::Selector {
                    selector: "#ready".into(),
                }),
                timeout_ms: max_ms,
            }
        );
    }

    #[tokio::test]
    async fn f2_direct_remote_mutations_are_refused_even_with_a_live_webview() {
        // Given a session whose webview exists (so a direct mutation could run), when a framed
        // client sends each direct mutation, then it is refused and the session is unchanged.
        let app = mock_app();
        let manager = Arc::new(BrowserManager::new());
        let state = register_remote_session(&manager, "b-direct-mutation");
        let _webview =
            tauri::WebviewWindowBuilder::new(&app, &state.webview_label, Default::default())
                .build()
                .expect("mock webview");
        let id = state.browser_id.clone();
        let ops = vec![
            RemoteBrowserOperation::Navigate {
                browser_id: id.clone(),
                url: "https://other.example.com/".into(),
            },
            RemoteBrowserOperation::Back { browser_id: id.clone() },
            RemoteBrowserOperation::Forward { browser_id: id.clone() },
            RemoteBrowserOperation::Reload { browser_id: id.clone() },
            RemoteBrowserOperation::Keypress {
                browser_id: id.clone(),
                key: "a".into(),
            },
        ];
        for op in ops {
            let err = execute_remote_operation(app.handle(), &manager, op.clone())
                .await
                .expect_err("a direct mutation must be refused");
            assert_eq!(ipc_error_code_string(err.code), "BROWSER_FORBIDDEN", "{op:?}");
        }
        let after = manager.get_state(&id).expect("session still exists");
        assert_eq!(after.url, state.url);
        assert_eq!(after.generation, state.generation);

        // Validation still runs first: an invalid direct op keeps its validation code.
        let bad_key = RemoteBrowserOperation::Keypress {
            browser_id: id,
            key: "Cmd+Q".into(),
        };
        let err = execute_remote_operation(app.handle(), &manager, bad_key)
            .await
            .expect_err("invalid key");
        assert_eq!(err.code, IpcErrorCode::InvalidArgument);
    }

    fn app_with_remote_service(
        manager: &Arc<BrowserManager>,
    ) -> (
        tauri::App<tauri::test::MockRuntime>,
        Arc<crate::browser::BrowserRemoteService>,
    ) {
        use tauri::Manager;
        let app = mock_app();
        let broker = Arc::new(crate::browser::remote_driver::RemoteDriverBroker::new());
        let service = Arc::new(crate::browser::BrowserRemoteService::new(
            (**manager).clone(),
            Arc::clone(&broker),
        ));
        app.manage(Arc::clone(&service));
        app.manage(broker);
        (app, service)
    }

    #[tokio::test]
    async fn f3_any_inbound_byte_ends_the_frame_stream_without_reading_a_length() {
        // Given a streaming subscription, when the client sends a single byte (not a whole
        // frame header), then the stream ends and the subscription is released.
        use tokio::io::AsyncWriteExt;
        let manager = Arc::new(BrowserManager::new());
        let (app, service) = app_with_remote_service(&manager);
        register_remote_session(&manager, "b-stream-close");
        let (client, server) = tokio::io::duplex(64 * 1024);
        let app_handle = app.handle().clone();
        let mgr = Arc::clone(&manager);
        let server_task = tokio::spawn(async move {
            let (mut reader, mut writer) = tokio::io::split(server);
            run_framed_ipc_loop(&mut reader, &mut writer, &app_handle, &mgr).await
        });
        let (mut client_reader, mut client_writer) = tokio::io::split(client);
        client_writer
            .write_all(&json_frame(&RemoteBrowserOperation::SubscribeViewer {
                browser_id: "b-stream-close".into(),
                device_id: "dev".into(),
                viewer_instance_id: "view".into(),
                options: None,
            }))
            .await
            .expect("send subscribe");
        let (_, payload) = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            read_ipc_frame(&mut client_reader),
        )
        .await
        .expect("subscribe response in time");
        let resp: serde_json::Value = serde_json::from_slice(&payload).expect("json");
        assert_eq!(resp["status"], "ok", "{resp}");
        assert!(service.is_producer_active("b-stream-close"));

        client_writer.write_all(&[0xFF]).await.expect("send one byte");
        tokio::time::timeout(std::time::Duration::from_secs(5), server_task)
            .await
            .expect("the stream must end on the first inbound byte")
            .expect("server task")
            .expect("clean end");
        assert!(!service.is_producer_active("b-stream-close"));
        drop(client_writer);
    }

    #[tokio::test]
    async fn f4_subscription_is_released_when_its_response_cannot_be_written() {
        // Given a client that sends SubscribeViewer and disconnects at once, when the server
        // fails to write the response, then the subscription it made is released.
        use tokio::io::AsyncWriteExt;
        let manager = Arc::new(BrowserManager::new());
        let (app, service) = app_with_remote_service(&manager);
        register_remote_session(&manager, "b-sub-leak");
        let (mut client, server) = tokio::io::duplex(64 * 1024);
        client
            .write_all(&json_frame(&RemoteBrowserOperation::SubscribeViewer {
                browser_id: "b-sub-leak".into(),
                device_id: "dev".into(),
                viewer_instance_id: "view".into(),
                options: None,
            }))
            .await
            .expect("send subscribe");
        drop(client);
        let (mut reader, mut writer) = tokio::io::split(server);
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            run_framed_ipc_loop(&mut reader, &mut writer, app.handle(), &manager),
        )
        .await
        .expect("loop ends");
        assert!(result.is_err(), "the response write must fail on a closed peer");
        assert!(!service.is_producer_active("b-sub-leak"));
    }

    #[tokio::test]
    async fn f5_accept_loop_survives_an_accept_error() {
        // Given an accept that fails once, when the loop runs, then it keeps accepting.
        let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<u32>();
        let counter = Arc::clone(&attempts);
        let accept = move || {
            let n = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            async move {
                match n {
                    0 => Err(std::io::Error::from(std::io::ErrorKind::ConnectionAborted)),
                    1 => Ok(7u32),
                    _ => std::future::pending().await,
                }
            }
        };
        let task = tokio::spawn(serve_accept_loop(accept, move |stream| {
            let _ = tx.send(stream);
        }));
        let accepted = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("a connection after the failed accept");
        assert_eq!(accepted, Some(7));
        task.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn f7_server_closes_a_connection_that_never_sends_a_request() {
        // Given a peer that connects and stays silent, then the server gives up after
        // REQUEST_READ_TIMEOUT instead of holding the connection forever.
        let app = mock_app();
        let manager = Arc::new(BrowserManager::new());
        let (_client, server) = tokio::io::duplex(4096);
        let result = tokio::time::timeout(
            REQUEST_READ_TIMEOUT * 2,
            handle_connection(server, app.handle().clone(), manager, Arc::new("t".into())),
        )
        .await
        .expect("the server must stop waiting for the request line");
        assert!(result.is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn f7_client_gives_up_on_a_server_that_never_answers() {
        let (client, _server) = tokio::io::duplex(64 * 1024);
        let result = tokio::time::timeout(
            RESPONSE_READ_TIMEOUT * 2,
            send_over_stream(client, BrowserCliRequest::List, "t".into()),
        )
        .await
        .expect("the client must stop waiting for the response");
        assert!(matches!(result, Err(BrowserError::AutomationFailed(_))), "{result:?}");
    }

    #[tokio::test]
    async fn f8_cli_reload_reports_state_and_leaves_a_missing_webview_session_untouched() {
        use tauri::Listener;
        let app = mock_app();
        let manager = Arc::new(BrowserManager::new());

        // Without a webview the session's generation and loading flag stay as they were.
        let orphan = register_remote_session(&manager, "b-reload-orphan");
        let resp = execute_request(
            app.handle(),
            &manager,
            BrowserCliRequest::Reload {
                browser_id: orphan.browser_id.clone(),
            },
        )
        .await;
        assert!(
            matches!(&resp, BrowserCliResponse::Error { code, .. } if code == "WEBVIEW_NOT_FOUND"),
            "{resp:?}"
        );
        let after = manager.get_state(&orphan.browser_id).expect("state");
        assert_eq!(after.generation, orphan.generation);
        assert_eq!(after.loading, orphan.loading);

        // With a webview the GUI is told about the reload.
        let live = register_remote_session(&manager, "b-reload-live");
        let _webview =
            tauri::WebviewWindowBuilder::new(&app, &live.webview_label, Default::default())
                .build()
                .expect("mock webview");
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        app.listen(crate::ipc::browser::BROWSER_STATE_CHANGED_EVENT, move |event| {
            let _ = tx.send(event.payload().to_owned());
        });
        let resp = execute_request(
            app.handle(),
            &manager,
            BrowserCliRequest::Reload {
                browser_id: live.browser_id.clone(),
            },
        )
        .await;
        assert_eq!(resp, BrowserCliResponse::Navigated);
        let payload: serde_json::Value = serde_json::from_str(
            &rx.recv_timeout(std::time::Duration::from_secs(5))
                .expect("browser_state_changed after a CLI reload"),
        )
        .expect("payload json");
        assert_eq!(payload["browserId"], live.browser_id);
        assert_eq!(payload["loading"], true);
    }

    #[tokio::test]
    async fn f10_f11_driver_operations_map_errors_by_variant_and_report_remaining_ttl() {
        let manager = Arc::new(BrowserManager::new());
        let (app, _service) = app_with_remote_service(&manager);
        register_remote_session(&manager, "b-driver");
        let claim = |device: &str| RemoteBrowserOperation::ClaimDriver {
            browser_id: "b-driver".into(),
            device_id: device.into(),
            connection_id: format!("conn-{device}"),
            subscription_id: format!("sub-{device}"),
            lease_epoch: None,
        };
        let lease = execute_remote_operation(app.handle(), &manager, claim("dev1"))
            .await
            .expect("first claim");
        let ttl_ms = lease["expiresInMs"].as_u64().expect("remaining TTL in ms");
        assert!(ttl_ms > 0, "{lease}");
        assert!(
            ttl_ms <= crate::browser::remote_driver::LEASE_TTL.as_millis() as u64,
            "{lease}"
        );
        let epoch: u64 = lease["leaseEpoch"]
            .as_str()
            .and_then(|s| s.parse().ok())
            .expect("lease epoch");

        let busy = execute_remote_operation(app.handle(), &manager, claim("dev2"))
            .await
            .expect_err("a second device must not take a live lease");
        assert_eq!(ipc_error_code_string(busy.code), "BROWSER_DRIVER_BUSY");

        let stale = execute_remote_operation(
            app.handle(),
            &manager,
            RemoteBrowserOperation::HeartbeatDriver {
                device_id: "dev1".into(),
                connection_id: "conn-dev1".into(),
                subscription_id: "sub-dev1".into(),
                lease_epoch: Some(epoch + 100),
            },
        )
        .await
        .expect_err("a heartbeat for a stale lease must be refused");
        assert_eq!(ipc_error_code_string(stale.code), "BROWSER_FORBIDDEN");
    }

    #[tokio::test]
    async fn f12_driver_and_stream_operations_fail_closed_without_their_services() {
        // Given an app with neither a remote service nor a driver broker, every operation
        // that would need them is refused instead of answering with a fake success.
        let app = mock_app();
        let manager = Arc::new(BrowserManager::new());
        register_remote_session(&manager, "b-no-service");
        let ops = vec![
            RemoteBrowserOperation::ClaimDriver {
                browser_id: "b-no-service".into(),
                device_id: "dev".into(),
                connection_id: "conn".into(),
                subscription_id: "sub".into(),
                lease_epoch: None,
            },
            RemoteBrowserOperation::ReleaseDriver {
                subscription_id: "sub".into(),
                lease_epoch: Some(1),
            },
            RemoteBrowserOperation::HeartbeatDriver {
                device_id: "dev".into(),
                connection_id: "conn".into(),
                subscription_id: "sub".into(),
                lease_epoch: Some(1),
            },
            RemoteBrowserOperation::SubscribeViewer {
                browser_id: "b-no-service".into(),
                device_id: "dev".into(),
                viewer_instance_id: "view".into(),
                options: None,
            },
            RemoteBrowserOperation::Execute {
                browser_id: "b-no-service".into(),
                command: "click".into(),
                params: Some(serde_json::json!({ "selector": "#btn" })),
                document_generation: None,
                browser_instance_id: None,
                desktop_epoch: None,
                lease_epoch: Some("7".into()),
                device_id: Some("dev".into()),
                connection_id: Some("conn".into()),
            },
        ];
        for op in ops {
            let err = execute_remote_operation(app.handle(), &manager, op.clone())
                .await
                .expect_err("must fail closed");
            assert_eq!(ipc_error_code_string(err.code), "BROWSER_UNAVAILABLE", "{op:?}");
        }
    }

    #[tokio::test]
    async fn f15_open_file_rejects_a_relative_path_as_invalid_argument() {
        let app = mock_app();
        let manager = Arc::new(BrowserManager::new());
        let resp = execute_request(
            app.handle(),
            &manager,
            BrowserCliRequest::OpenFile {
                path: "relative/file.txt".into(),
                line: None,
                col: None,
            },
        )
        .await;
        assert!(
            matches!(&resp, BrowserCliResponse::Error { code, .. } if code == "INVALID_ARGUMENT"),
            "{resp:?}"
        );
    }

    #[tokio::test]
    async fn f16_unauthenticated_malformed_requests_learn_nothing() {
        let app = mock_app();
        let manager = Arc::new(BrowserManager::new());
        let token = "f16-token";
        // Without the token, an unknown command and a known one get the same answer.
        for raw in [
            r#"{"command":"noSuchCommand"}"#.to_string(),
            r#"{"command":"navigate","browserId":"b1"}"#.to_string(),
            r#"not json at all"#.to_string(),
        ] {
            let resp =
                send_raw_line(app.handle().clone(), Arc::clone(&manager), token, &raw).await;
            assert_eq!(resp["code"], BROWSER_CLI_UNAUTHORIZED, "{raw}: {resp}");
        }
        // A caller holding the token still learns why its request is malformed.
        let raw = format!(r#"{{"command":"noSuchCommand","token":"{token}"}}"#);
        let resp = send_raw_line(app.handle().clone(), Arc::clone(&manager), token, &raw).await;
        assert_eq!(resp["code"], "BROWSER_CLI_REQUEST_INVALID", "{resp}");
    }

    #[test]
    fn b9_framed_errors_carry_details_and_typed_driver_codes() {
        use crate::remote::browser_backend::RemoteBrowserError;
        let explanation = crate::browser::remote_input::RefusedInputExplanation {
            code: "ime_composition".into(),
            reason: "input refused".into(),
            input_class: "text".into(),
            remediation: "retry".into(),
        };
        let expected_details = explanation.to_details();
        let ipc = remote_browser_error_to_ipc(RemoteBrowserError::InputRefused(explanation));
        let wire = serde_json::to_value(framed_error_response(ipc)).expect("serialize");
        assert_eq!(wire["type"], "error");
        assert_eq!(wire["code"], "UNSUPPORTED");
        assert_eq!(wire["details"], expected_details);

        // An error without details omits the field entirely.
        let plain = serde_json::to_value(framed_error_response(IpcError::new(
            IpcErrorCode::BrowserNotFound,
            "gone",
        )))
        .expect("serialize");
        assert!(plain.get("details").is_none(), "{plain}");

        let busy = remote_browser_error_to_ipc(RemoteBrowserError::DriverBusy("busy".into()));
        assert_eq!(ipc_error_code_string(busy.code), "BROWSER_DRIVER_BUSY");
        let limited = remote_browser_error_to_ipc(RemoteBrowserError::RateLimited("slow".into()));
        assert_eq!(ipc_error_code_string(limited.code), "RATE_LIMITED");
    }
}
