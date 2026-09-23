//! Renderer-facing sanitized inventory API; credentials are daemon-owned.
use crate::{
    daemon::client::DaemonClient,
    paired_host::{
        inventory::{HostView, MigrationReceipt},
        service::{InventoryChangeEvent, MigrationRequest, PairRequest, Result},
    },
};
use std::sync::Arc;
use tauri::{Emitter, State};

pub const PAIRED_HOST_INVENTORY_CHANGED_EVENT: &str = "paired_host_inventory_changed";

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
        let stored = crate::paired_host::projects::resolve_stored_project(&data_dir, &workspace_id)
            .ok_or_else(|| {
                crate::ipc::IpcError::new(
                    crate::ipc::error::IpcErrorCode::WorkspaceNotFound,
                    "Paired daemon project not found. Re-select or re-pair this project.",
                )
            })?;

        let host_id = match stored.target {
            crate::scoped_contracts::RunTarget::PairedDaemon { host_id } => host_id,
            _ => {
                return Err(crate::ipc::IpcError::new(
                    crate::ipc::error::IpcErrorCode::WorkspaceNotFound,
                    "Project is not a paired daemon project.",
                ));
            }
        };

        let hosts = daemon
            .paired_host_list()
            .await
            .map_err(|e| crate::ipc::IpcError::internal(format!("{e:?}")))?;
        let host = hosts
            .into_iter()
            .find(|h| h.host_id == host_id)
            .ok_or_else(|| {
                crate::ipc::IpcError::internal(format!(
                    "Paired machine '{host_id}' not found in inventory"
                ))
            })?;

        if host.auth_status != crate::paired_host::inventory::AuthStatus::Paired {
            return Err(crate::ipc::IpcError::internal(
                "Machine authorization required for paired host",
            ));
        }

        let upload_id = uuid::Uuid::new_v4().to_string();
        const CHUNK_SIZE: usize = 32 * 1024;
        let chunks: Vec<&[u8]> = image.bytes.chunks(CHUNK_SIZE).collect();
        let total_chunks = chunks.len() as u32;
        let mut final_remote_path = None;

        use base64::Engine;
        for (idx, chunk) in chunks.into_iter().enumerate() {
            let request_id = uuid::Uuid::new_v4().to_string();
            let b64_data = base64::engine::general_purpose::STANDARD.encode(chunk);
            let op_req = crate::paired_host::client::OperationRequest {
                host_id: host_id.clone(),
                generation: host.generation,
                operation: crate::paired_host::client::Operation::PasteUploadChunk {
                    request: crate::remote::machine_protocol::PasteUploadChunkRequest {
                        request_id,
                        upload_id: upload_id.clone(),
                        file_name: file_name.clone(),
                        chunk_index: idx as u32,
                        total_chunks,
                        data: b64_data,
                    },
                },
            };

            let resp = daemon.paired_host_operation(op_req).await.map_err(|e| {
                crate::ipc::IpcError::internal(format!("{}: {:?}", e.code, e.machine_error))
            })?;

            if let crate::paired_host::client::OperationResult::PasteUploadChunk(result) =
                resp.result
            {
                if let Some(path) = result.remote_path {
                    final_remote_path = Some(path);
                }
            }
        }

        let remote_path = final_remote_path.ok_or_else(|| {
            crate::ipc::IpcError::internal("Paired host did not return a finalized remote path")
        })?;

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

#[tauri::command]
pub async fn paired_host_attach_session(
    daemon: State<'_, Arc<DaemonClient>>,
) -> Result<crate::remote::attach_client::AttachSession> {
    daemon.remote_allocate_attach_session().await
}
