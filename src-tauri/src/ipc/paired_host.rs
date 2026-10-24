//! Renderer-facing sanitized inventory API; credentials are daemon-owned.
use crate::{
    daemon::client::DaemonClient,
    paired_host::{
        inventory::{HostView, MigrationReceipt},
        service::{InventoryChangeEvent, MigrationRequest, PairRequest, RedeemGrantRequest, Result},
    },
};
use std::sync::Arc;
use tauri::{Emitter, State};

pub const PAIRED_HOST_INVENTORY_CHANGED_EVENT: &str = "paired_host_inventory_changed";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedSessionRetryResult {
    pub admitted: bool,
    pub owner_unavailable: bool,
    pub stale_tuple: bool,
    pub blocked: bool,
    pub affirmative_exit: bool,
    pub actor_identity: Option<PairedSessionActorIdentity>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedSessionActorIdentity {
    pub backend_session_id: String,
    pub daemon_epoch: String,
    pub incarnation: String,
    pub generation: Option<u64>,
}

#[tauri::command]
pub async fn cmd_retry_paired_session<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    daemon: State<'_, Arc<DaemonClient>>,
    backend_session_id: String,
    daemon_epoch: String,
    incarnation: String,
) -> std::result::Result<PairedSessionRetryResult, crate::ipc::IpcError> {
    use crate::daemon::protocol::DaemonResponse;

    let res = daemon.retry_paired_session(backend_session_id.clone(), daemon_epoch.clone(), incarnation.clone()).await?;
    let retry_res = match res {
        DaemonResponse::RetryPairedSessionResult {
            admitted, owner_unavailable, stale_tuple, blocked, affirmative_exit,
        } => PairedSessionRetryResult { admitted, owner_unavailable, stale_tuple, blocked, affirmative_exit, actor_identity: None },
        other => return Err(crate::ipc::IpcError::internal(format!("Unexpected daemon response: {other:?}"))),
    };

    let actor_identity = match daemon.paired_recovery_outcome(backend_session_id.clone()).await {
        Ok(DaemonResponse::PairedRecoveryOutcomeResult { outcome: Some(outcome) })
            if outcome.backend_session_id == backend_session_id &&
                outcome.daemon_epoch == daemon_epoch && outcome.incarnation == incarnation => {
            let actor = PairedSessionActorIdentity {
                backend_session_id: outcome.backend_session_id.clone(),
                daemon_epoch: outcome.daemon_epoch.clone(),
                incarnation: outcome.incarnation.clone(),
                generation: outcome.generation,
            };
            let _ = app.emit("paired_session_recovery_status", &outcome);
            Some(actor)
        }
        _ => None,
    };
    if let Some(actor_identity) = actor_identity {
        return Ok(PairedSessionRetryResult { actor_identity: Some(actor_identity), ..retry_res });
    }

    Ok(retry_res)
}

pub fn emit_paired_host_inventory_changed<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    event: &InventoryChangeEvent,
) -> tauri::Result<()> {
    app.emit(PAIRED_HOST_INVENTORY_CHANGED_EVENT, event)
}

#[tauri::command]
pub async fn paired_host_operation<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    daemon: State<'_, Arc<DaemonClient>>,
    request: crate::paired_host::client::OperationRequest,
) -> std::result::Result<
    crate::paired_host::client::OperationResponse,
    crate::paired_host::client::ClientError,
> {
    let host_id = request.host_id.clone();
    let generation = request.generation;
    let res = daemon.paired_host_operation(request).await;
    if let Err(ref err) = res {
        if err.code == "UNAUTHORIZED" {
            let _ = emit_paired_host_inventory_changed(
                &app,
                &InventoryChangeEvent {
                    r#type: "revoke".into(),
                    host: None,
                    host_id: Some(host_id),
                    generation: Some(generation.0.to_string()),
                },
            );
        }
    }
    res
}

#[tauri::command]
pub async fn paired_host_list(daemon: State<'_, Arc<DaemonClient>>) -> Result<Vec<HostView>> {
    daemon.paired_host_list().await
}

/// Mirrors `PairedHostReachabilityList` in ui/src/lib/pairedHostReachability.ts.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedHostReachabilityList {
    pub supported: bool,
    pub entries: Vec<crate::paired_host::reachability::HostReachability>,
}

/// An older daemon without `pairedHostReachabilityV1` yields `supported: false`, never an
/// error, so the renderer falls back to the legacy `online` flag.
pub async fn reachability_list_view(daemon: &DaemonClient) -> Result<PairedHostReachabilityList> {
    Ok(match daemon.paired_host_reachability_list().await? {
        Some(entries) => PairedHostReachabilityList { supported: true, entries },
        None => PairedHostReachabilityList { supported: false, entries: Vec::new() },
    })
}

#[tauri::command]
pub async fn paired_host_reachability_list(
    daemon: State<'_, Arc<DaemonClient>>,
) -> Result<PairedHostReachabilityList> {
    reachability_list_view(&daemon).await
}

/// Mirrors `PairedHostRouteList` in ui/src/lib/pairedHostRoute.ts.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedHostRouteList {
    pub supported: bool,
    pub entries: Vec<crate::paired_host::route_view::HostRoute>,
}

/// An older daemon without `pairedHostRouteV1` yields `supported: false`, never an error, so
/// the renderer keeps the transport label it showed before.
pub async fn route_list_view(daemon: &DaemonClient) -> Result<PairedHostRouteList> {
    Ok(match daemon.paired_host_route_list().await? {
        Some(entries) => PairedHostRouteList { supported: true, entries },
        None => PairedHostRouteList { supported: false, entries: Vec::new() },
    })
}

#[tauri::command]
pub async fn paired_host_route_list(
    daemon: State<'_, Arc<DaemonClient>>,
) -> Result<PairedHostRouteList> {
    route_list_view(&daemon).await
}

#[tauri::command]
pub async fn paired_host_pair<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    daemon: State<'_, Arc<DaemonClient>>,
    request: PairRequest,
) -> Result<HostView> {
    let host = daemon.paired_host_pair(request).await?;
    let _ = emit_paired_host_inventory_changed(
        &app,
        &InventoryChangeEvent {
            r#type: "pair".into(),
            host: Some(host.clone()),
            host_id: Some(host.host_id.clone()),
            generation: Some(host.generation.0.to_string()),
        },
    );
    Ok(host)
}

#[tauri::command]
pub async fn paired_host_migrate_legacy<R: tauri::Runtime>(
    _app: tauri::AppHandle<R>,
    daemon: State<'_, Arc<DaemonClient>>,
    request: MigrationRequest,
) -> Result<MigrationReceipt> {
    let receipt = daemon.paired_host_migrate_legacy(request).await?;
    Ok(receipt)
}

/// Redeems an account grant end-to-end encrypted with this client's own attach key.
#[tauri::command]
pub async fn paired_host_redeem_grant<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    daemon: State<'_, Arc<DaemonClient>>,
    request: RedeemGrantRequest,
) -> Result<HostView> {
    let host = daemon.paired_host_redeem_grant(request).await?;
    let _ = emit_paired_host_inventory_changed(
        &app,
        &InventoryChangeEvent {
            r#type: "pair".into(),
            host: Some(host.clone()),
            host_id: Some(host.host_id.clone()),
            generation: Some(host.generation.0.to_string()),
        },
    );
    Ok(host)
}

/// The public half of the key this client attaches with; the grant binds it. The private
/// half never leaves the daemon.
#[tauri::command]
pub async fn paired_host_client_attach_public_key(
    daemon: State<'_, Arc<DaemonClient>>,
) -> Result<String> {
    daemon.paired_host_client_attach_public_key().await
}

#[tauri::command]
pub async fn paired_host_forget<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    daemon: State<'_, Arc<DaemonClient>>,
    request: MigrationReceipt,
) -> Result<()> {
    daemon
        .paired_host_forget(request.host_id.clone(), request.generation)
        .await?;
    let _ = emit_paired_host_inventory_changed(
        &app,
        &InventoryChangeEvent {
            r#type: "forget".into(),
            host: None,
            host_id: Some(request.host_id),
            generation: Some(request.generation.0.to_string()),
        },
    );
    Ok(())
}

#[tauri::command]
pub async fn paired_host_read(
    daemon: State<'_, Arc<DaemonClient>>,
    request: MigrationReceipt,
) -> Result<HostView> {
    daemon.paired_host_read(request).await
}

#[tauri::command]
pub async fn paired_host_capabilities(
    daemon: State<'_, Arc<DaemonClient>>,
) -> Result<serde_json::Value> {
    daemon.paired_host_capabilities().await
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteClipboardImagePaste {
    pub remote_path: String,
    pub byte_length: usize,
}

#[tauri::command]
pub async fn cmd_daemon_paste_clipboard_image<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    daemon: State<'_, Arc<DaemonClient>>,
    workspace_id: String,
) -> std::result::Result<Option<RemoteClipboardImagePaste>, crate::ipc::IpcError> {
    let Some(image) = crate::clipboard_image::read_clipboard_image_for_app(&app).await? else {
        return Ok(None);
    };

    let byte_length = image.bytes.len();
    if byte_length > crate::clipboard_image::MAX_CLIPBOARD_IMAGE_BYTES {
        return Err(crate::ipc::IpcError::new(
            crate::ipc::error::IpcErrorCode::PayloadTooLarge,
            format!(
                "Clipboard image exceeds maximum size of {} bytes",
                crate::clipboard_image::MAX_CLIPBOARD_IMAGE_BYTES
            ),
        ));
    }

    let file_name = format!("{}.{}", uuid::Uuid::new_v4(), image.extension);

    if workspace_id.starts_with("daemon:") {
        use tauri::Manager;
        let data_dir = app
            .path()
            .app_data_dir()
            .map_err(|e| crate::ipc::IpcError::internal(e.to_string()))?;
        let remote_path = crate::paired_host::upload::upload_temp_bytes(
            &daemon,
            &data_dir,
            &workspace_id,
            &file_name,
            image.bytes,
        )
        .await?;

        return Ok(Some(RemoteClipboardImagePaste {
            remote_path,
            byte_length,
        }));
    }

    let res = daemon
        .send_request(
            crate::daemon::protocol::DaemonRequest::UploadClipboardImage {
                file_name,
                data: image.bytes,
            },
        )
        .await
        .map_err(|e| {
            crate::ipc::IpcError::internal(format!("Daemon communication error: {e:?}"))
        })?;

    match res {
        crate::daemon::protocol::DaemonResponse::UploadClipboardImageOk {
            remote_path,
            byte_length,
        } => Ok(Some(RemoteClipboardImagePaste {
            remote_path,
            byte_length,
        })),
        crate::daemon::protocol::DaemonResponse::Error { message, .. } => {
            Err(crate::ipc::IpcError::internal(message))
        }
        other => Err(crate::ipc::IpcError::internal(format!(
            "Unexpected daemon response: {other:?}"
        ))),
    }
}

/// Fetches a file a paired-host terminal names into the local remote-files cache.
///
/// `session_id` is the id the desktop UI holds: the local relayed proxy id
/// (`daemon-session:<hash>`). It is mapped through the daemon's paired-runtime descriptor
/// to the HOST-side session id, which anchors relative paths on the host; an id the paired
/// runtime does not own is forwarded unchanged, and a relayed id that no longer maps is
/// refused rather than dropped, so the host can still check where the shell runs.
#[tauri::command]
pub async fn cmd_paired_file_fetch<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    daemon: State<'_, Arc<DaemonClient>>,
    workspace_id: String,
    path: String,
    cwd: Option<String>,
    session_id: Option<String>,
) -> std::result::Result<crate::ipc::remote_file::RemoteFileFetched, crate::ipc::IpcError> {
    use tauri::Manager;
    let io = |e: tauri::Error| {
        crate::ipc::IpcError::new(crate::ipc::error::IpcErrorCode::IoError, e.to_string())
    };
    let data_dir = app.path().app_data_dir().map_err(io)?;
    let cache_root = app.path().app_cache_dir().map_err(io)?;
    let host_session_id =
        crate::paired_host::file_read::resolve_host_session_id(session_id, |id| async {
            daemon
                .paired_terminal_descriptor(id)
                .await
                .map(|descriptor| descriptor.map(|d| d.target.session_id))
        })
        .await?;
    let (local, remote_path, byte_length) = crate::paired_host::file_read::fetch_paired_file(
        &daemon,
        &data_dir,
        &cache_root,
        &workspace_id,
        host_session_id,
        cwd,
        path,
    )
    .await?;
    Ok(crate::ipc::remote_file::RemoteFileFetched {
        local_path: local.to_string_lossy().into_owned(),
        remote_path,
        byte_length,
    })
}


#[tauri::command]
pub async fn paired_host_attach_session(
    daemon: State<'_, Arc<DaemonClient>>,
) -> Result<crate::remote::attach_client::AttachSession> {
    daemon.remote_allocate_attach_session().await
}
