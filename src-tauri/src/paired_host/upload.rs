use crate::daemon::client::DaemonClient;
use crate::ipc::{IpcError, IpcErrorCode};
use crate::paired_host::client::{ClientError, Operation, OperationRequest, OperationResult};
use crate::paired_host::inventory::HostView;
use crate::remote::machine_protocol as m;
use crate::scoped_contracts::RunTarget;
use base64::Engine as _;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

pub const UPLOAD_CHUNK_BYTES: usize = 32 * 1024;
pub const PAIRED_UPLOAD_PARALLELISM: usize = 4;
pub const MAX_CHUNK_RETRIES: usize = 3;

pub async fn resolve_paired_host(
    daemon: &Arc<DaemonClient>,
    data_dir: &Path,
    workspace_id: &str,
) -> Result<HostView, IpcError> {
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

    Ok(host)
}

pub async fn upload_temp_bytes(
    daemon: &Arc<DaemonClient>,
    data_dir: &Path,
    workspace_id: &str,
    file_name: &str,
    bytes: Vec<u8>,
) -> Result<String, IpcError> {
    upload_temp_bytes_with_progress(daemon, data_dir, workspace_id, file_name, bytes, |_| {}).await
}

pub async fn upload_temp_bytes_with_progress<F>(
    daemon: &Arc<DaemonClient>,
    data_dir: &Path,
    workspace_id: &str,
    file_name: &str,
    bytes: Vec<u8>,
    progress: F,
) -> Result<String, IpcError>
where
    F: Fn(u64) + Send + Sync + 'static,
{
    if bytes.is_empty() {
        return Err(IpcError::new(
            IpcErrorCode::InvalidRequest,
            "Refusing to upload an empty file",
        ));
    }

    let host = resolve_paired_host(daemon, data_dir, workspace_id).await?;

    let supports_v2 = match daemon
        .paired_host_operation(OperationRequest {
            host_id: host.host_id.clone(),
            generation: host.generation,
            operation: Operation::Capabilities,
        })
        .await
    {
        Ok(resp) => {
            if let OperationResult::Capabilities(caps) = resp.result {
                caps.capabilities
                    .iter()
                    .any(|c| c == "pairedPasteUploadV2")
            } else {
                false
            }
        }
        Err(_) => false,
    };

    let upload_id = uuid::Uuid::new_v4().to_string();
    let total_bytes = bytes.len() as u64;

    if supports_v2 {
        upload_parallel_v2(
            daemon,
            &host,
            &upload_id,
            file_name,
            &bytes,
            total_bytes,
            Arc::new(progress),
        )
        .await
    } else {
        let legacy_name = crate::clipboard_image::legacy_safe_drop_file_name(&upload_id, file_name);
        upload_sequential_legacy(
            daemon,
            &host,
            &upload_id,
            &legacy_name,
            &bytes,
            progress,
        )
        .await
    }
}

async fn upload_sequential_legacy<F>(
    daemon: &Arc<DaemonClient>,
    host: &HostView,
    upload_id: &str,
    file_name: &str,
    bytes: &[u8],
    progress: F,
) -> Result<String, IpcError>
where
    F: Fn(u64),
{
    let chunks: Vec<&[u8]> = bytes.chunks(UPLOAD_CHUNK_BYTES).collect();
    let total_chunks = chunks.len() as u32;
    let mut final_remote_path = None;
    let mut sent = 0u64;

    for (index, chunk) in chunks.into_iter().enumerate() {
        let request = m::PasteUploadChunkRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            upload_id: upload_id.to_string(),
            file_name: file_name.to_string(),
            chunk_index: index as u32,
            total_chunks,
            data: base64::engine::general_purpose::STANDARD.encode(chunk),
            offset: None,
            total_bytes: None,
        };

        let response = daemon
            .paired_host_operation(OperationRequest {
                host_id: host.host_id.clone(),
                generation: host.generation,
                operation: Operation::PasteUploadChunk { request },
            })
            .await
            .map_err(|error| {
                IpcError::internal(format!("{}: {:?}", error.code, error.machine_error))
            })?;

        sent += chunk.len() as u64;
        progress(sent);

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

#[derive(Debug, Clone)]
pub struct ChunkSpec {
    pub index: u32,
    pub offset: u64,
    pub length: usize,
}

pub fn plan_chunks(total_bytes: usize, chunk_size: usize) -> Vec<ChunkSpec> {
    if total_bytes == 0 {
        return vec![];
    }
    let mut chunks = Vec::new();
    let mut offset = 0usize;
    let mut index = 0u32;
    while offset < total_bytes {
        let length = (total_bytes - offset).min(chunk_size);
        chunks.push(ChunkSpec {
            index,
            offset: offset as u64,
            length,
        });
        offset += length;
        index += 1;
    }
    chunks
}

pub fn is_retryable_client_error(err: &ClientError) -> bool {
    if let Some(ref m) = err.machine_error {
        if m.retryable {
            return true;
        }
        return matches!(
            m.code.as_str(),
            "CAPACITY_EXCEEDED"
                | "RATE_LIMITED"
                | "TIMEOUT"
                | "HOST_UNAVAILABLE"
                | "MACHINE_SERVICE_UNAVAILABLE"
        );
    }
    matches!(
        err.code.as_str(),
        "CAPACITY_EXCEEDED"
            | "RATE_LIMITED"
            | "TIMEOUT"
            | "HOST_UNAVAILABLE"
            | "MACHINE_SERVICE_UNAVAILABLE"
    ) || err.ambiguous
}

async fn upload_parallel_v2<F>(
    daemon: &Arc<DaemonClient>,
    host: &HostView,
    upload_id: &str,
    file_name: &str,
    bytes: &[u8],
    total_bytes: u64,
    progress: Arc<F>,
) -> Result<String, IpcError>
where
    F: Fn(u64) + Send + Sync + 'static,
{
    use futures_util::stream::{self, StreamExt};

    let chunk_specs = plan_chunks(bytes.len(), UPLOAD_CHUNK_BYTES);
    let total_chunks = chunk_specs.len() as u32;
    let sent_counter = Arc::new(AtomicU64::new(0));

    let stream = stream::iter(chunk_specs).map(|spec| {
        let daemon = daemon.clone();
        let host = host.clone();
        let upload_id = upload_id.to_string();
        let file_name = file_name.to_string();
        let chunk_data = bytes[spec.offset as usize..spec.offset as usize + spec.length].to_vec();
        let progress = progress.clone();
        let sent_counter = sent_counter.clone();

        async move {
            let mut attempts = 0usize;
            loop {
                attempts += 1;
                let request = m::PasteUploadChunkRequest {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    upload_id: upload_id.clone(),
                    file_name: file_name.clone(),
                    chunk_index: spec.index,
                    total_chunks,
                    data: base64::engine::general_purpose::STANDARD.encode(&chunk_data),
                    offset: Some(spec.offset),
                    total_bytes: Some(total_bytes),
                };

                let result = daemon
                    .paired_host_operation(OperationRequest {
                        host_id: host.host_id.clone(),
                        generation: host.generation,
                        operation: Operation::PasteUploadChunk { request },
                    })
                    .await;

                match result {
                    Ok(response) => {
                        let sent = sent_counter.fetch_add(chunk_data.len() as u64, Ordering::SeqCst)
                            + chunk_data.len() as u64;
                        progress(sent);

                        if let OperationResult::PasteUploadChunk(res) = response.result {
                            return Ok((spec.index, res.remote_path));
                        } else {
                            return Err(IpcError::internal("Unexpected OperationResult kind"));
                        }
                    }
                    Err(err) => {
                        if attempts < MAX_CHUNK_RETRIES && is_retryable_client_error(&err) {
                            tokio::time::sleep(std::time::Duration::from_millis(
                                100 * attempts as u64,
                            ))
                            .await;
                            continue;
                        }
                        return Err(IpcError::internal(format!(
                            "Paired chunk {} upload failed after {} attempts: {}: {:?}",
                            spec.index, attempts, err.code, err.machine_error
                        )));
                    }
                }
            }
        }
    });

    let mut buffered = stream.buffer_unordered(PAIRED_UPLOAD_PARALLELISM);
    let mut final_remote_path = None;

    while let Some(result) = buffered.next().await {
        let (_index, path_opt) = result?;
        if let Some(p) = path_opt {
            final_remote_path = Some(p);
        }
    }

    final_remote_path.ok_or_else(|| {
        IpcError::internal("Paired host v2 did not return a finalized remote path on completion")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_chunks_divides_bytes_into_consecutive_offsets() {
        let specs = plan_chunks(100, 32);
        assert_eq!(specs.len(), 4);
        assert_eq!(specs[0].offset, 0);
        assert_eq!(specs[0].length, 32);
        assert_eq!(specs[1].offset, 32);
        assert_eq!(specs[1].length, 32);
        assert_eq!(specs[2].offset, 64);
        assert_eq!(specs[2].length, 32);
        assert_eq!(specs[3].offset, 96);
        assert_eq!(specs[3].length, 4);

        assert_eq!(plan_chunks(0, 32).len(), 0);
        assert_eq!(plan_chunks(32, 32).len(), 1);
    }

    #[test]
    fn is_retryable_client_error_identifies_transient_and_rejects_definitive() {
        let retryable_machine = ClientError {
            code: "CAPACITY_EXCEEDED".into(),
            machine_error: Some(m::MachineError {
                code: "CAPACITY_EXCEEDED".into(),
                message: "busy".into(),
                retryable: true,
                request_id: "req-1".into(),
                details: serde_json::Map::new(),
            }),
            request_id: None,
            ambiguous: false,
        };
        assert!(is_retryable_client_error(&retryable_machine));

        let fatal = ClientError {
            code: "UNAUTHORIZED".into(),
            machine_error: Some(m::MachineError {
                code: "UNAUTHORIZED".into(),
                message: "denied".into(),
                retryable: false,
                request_id: "req-2".into(),
                details: serde_json::Map::new(),
            }),
            request_id: None,
            ambiguous: false,
        };
        assert!(!is_retryable_client_error(&fatal));

        let ambiguous_transport = ClientError {
            code: "HOST_UNAVAILABLE".into(),
            machine_error: None,
            request_id: Some("req-3".into()),
            ambiguous: true,
        };
        assert!(is_retryable_client_error(&ambiguous_transport));
    }
}
