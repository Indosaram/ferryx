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
    app: tauri::AppHandle<R>,
    daemon: State<'_, Arc<DaemonClient>>,
    request: MigrationRequest,
) -> Result<MigrationReceipt> {
    let receipt = daemon.paired_host_migrate_legacy(request).await?;
    let _ = emit_paired_host_inventory_changed(
        &app,
        &InventoryChangeEvent {
            r#type: "migrate".into(),
            host: None,
            host_id: Some(receipt.host_id.clone()),
            generation: Some(receipt.generation.0.to_string()),
        },
    );
    Ok(receipt)
}

#[tauri::command]
pub async fn paired_host_forget<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    daemon: State<'_, Arc<DaemonClient>>,
    request: MigrationReceipt,
) -> Result<()> {
    daemon.paired_host_forget(request.host_id.clone(), request.generation).await?;
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
