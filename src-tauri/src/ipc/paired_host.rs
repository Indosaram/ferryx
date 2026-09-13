//! Renderer-facing sanitized inventory API; credentials are daemon-owned.
use crate::{daemon::client::DaemonClient, paired_host::{inventory::{HostView, MigrationReceipt}, service::{MigrationRequest, PairRequest, Result}}};
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub async fn paired_host_operation(daemon: State<'_, Arc<DaemonClient>>, request: crate::paired_host::client::OperationRequest) -> std::result::Result<crate::paired_host::client::OperationResponse, crate::paired_host::client::ClientError> {
    daemon.paired_host_operation(request).await
}

#[tauri::command]
pub async fn paired_host_list(daemon: State<'_, Arc<DaemonClient>>) -> Result<Vec<HostView>> {
    daemon.paired_host_list().await
}
#[tauri::command]
pub async fn paired_host_pair(daemon: State<'_, Arc<DaemonClient>>, request: PairRequest) -> Result<HostView> {
    daemon.paired_host_pair(request).await
}
#[tauri::command]
pub async fn paired_host_migrate_legacy(daemon: State<'_, Arc<DaemonClient>>, request: MigrationRequest) -> Result<MigrationReceipt> {
    daemon.paired_host_migrate_legacy(request).await
}
#[tauri::command]
pub async fn paired_host_forget(daemon: State<'_, Arc<DaemonClient>>, request: MigrationReceipt) -> Result<()> {
    daemon.paired_host_forget(request.host_id, request.generation).await
}
#[tauri::command]
pub async fn paired_host_read(daemon: State<'_, Arc<DaemonClient>>, request: MigrationReceipt) -> Result<HostView> {
    daemon.paired_host_read(request).await
}
#[tauri::command]
pub async fn paired_host_capabilities(daemon: State<'_, Arc<DaemonClient>>) -> Result<serde_json::Value> {
    daemon.paired_host_capabilities().await
}
