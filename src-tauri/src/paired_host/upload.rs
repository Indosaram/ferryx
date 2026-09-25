//! One wire implementation for byte uploads to a paired host: the clipboard paste and the design
//! feedback capture both need it.
use crate::daemon::client::DaemonClient;
use crate::ipc::{IpcError, IpcErrorCode};
use crate::paired_host::client::{Operation, OperationRequest, OperationResult};
use crate::scoped_contracts::RunTarget;
use base64::Engine as _;
use std::path::Path;
use std::sync::Arc;

/// Kept at 32 KiB so each base64 chunk stays inside the gateway's request budget.
const UPLOAD_CHUNK_BYTES: usize = 32 * 1024;

/// Resolves to the host-side path a remote agent can open, or fails when the workspace is not a
/// paired project, the machine is unknown/unpaired, or the host never finalizes a path.
pub async fn upload_temp_bytes(
    daemon: &Arc<DaemonClient>,
    data_dir: &Path,
    workspace_id: &str,
    file_name: &str,
    bytes: Vec<u8>,
) -> Result<String, IpcError> {
    if bytes.is_empty() {
        return Err(IpcError::new(
            IpcErrorCode::InvalidRequest,
            "Refusing to upload an empty file",
        ));
    }

    let stored = crate::paired_host::projects::resolve_stored_project(data_dir, workspace_id)
        .ok_or_else(|| {
            IpcError::new(
                IpcErrorCode::WorkspaceNotFound,
                "Paired daemon project not found. Re-select or re-pair this project.",
            )
        })?;

    let host_id = match stored.target {
        RunTarget::PairedDaemon { host_id } => host_id,
        _ => {
            return Err(IpcError::new(
                IpcErrorCode::WorkspaceNotFound,
                "Project is not a paired daemon project.",
            ))
        }
    };

    let hosts = daemon
        .paired_host_list()
        .await
        .map_err(|error| IpcError::internal(format!("{error:?}")))?;
    let host = hosts
        .into_iter()
        .find(|candidate| candidate.host_id == host_id)
        .ok_or_else(|| {
            IpcError::internal(format!("Paired machine '{host_id}' not found in inventory"))
        })?;

    if host.auth_status != crate::paired_host::inventory::AuthStatus::Paired {
        return Err(IpcError::internal(
            "Machine authorization required for paired host",
        ));
    }

    let upload_id = uuid::Uuid::new_v4().to_string();
    let chunks: Vec<&[u8]> = bytes.chunks(UPLOAD_CHUNK_BYTES).collect();
    let total_chunks = chunks.len() as u32;
    let mut final_remote_path = None;

    for (index, chunk) in chunks.into_iter().enumerate() {
        let request = crate::remote::machine_protocol::PasteUploadChunkRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            upload_id: upload_id.clone(),
            file_name: file_name.to_string(),
            chunk_index: index as u32,
            total_chunks,
            data: base64::engine::general_purpose::STANDARD.encode(chunk),
        };

        let response = daemon
            .paired_host_operation(OperationRequest {
                host_id: host_id.clone(),
                generation: host.generation,
                operation: Operation::PasteUploadChunk { request },
            })
            .await
            .map_err(|error| IpcError::internal(format!("{}: {:?}", error.code, error.machine_error)))?;

        if let OperationResult::PasteUploadChunk(result) = response.result {
            if let Some(path) = result.remote_path {
                final_remote_path = Some(path);
            }
        }
    }

    final_remote_path.ok_or_else(|| {
        IpcError::internal("Paired host did not return a finalized remote path")
    })
}
