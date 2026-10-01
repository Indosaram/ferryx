use crate::daemon::client::DaemonClient;
use crate::ipc::error::{IpcError, IpcErrorCode};
use crate::ipc::run_blocking;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, Runtime, State};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DropUploadProgress {
    pub upload_id: String,
    pub file_index: usize,
    pub total_files: usize,
    pub file_sent_bytes: u64,
    pub file_total_bytes: u64,
    pub aggregate_sent_bytes: u64,
    pub aggregate_total_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteDroppedFile {
    pub local_path: String,
    pub remote_path: String,
    pub byte_length: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteDropUploadResult {
    pub platform: String,
    pub files: Vec<RemoteDroppedFile>,
}

static ACTIVE_UPLOADS: parking_lot::Mutex<Option<HashMap<String, CancellationToken>>> =
    parking_lot::Mutex::new(None);

pub fn cancel_drop_upload(upload_id: &str) -> bool {
    let mut guard = ACTIVE_UPLOADS.lock();
    if let Some(ref mut map) = *guard {
        if let Some(token) = map.remove(upload_id) {
            token.cancel();
            return true;
        }
    }
    false
}

fn register_upload_token(upload_id: &str) -> (CancellationToken, UploadGuard) {
    let token = CancellationToken::new();
    let mut guard = ACTIVE_UPLOADS.lock();
    let map = guard.get_or_insert_with(HashMap::new);
    map.insert(upload_id.to_string(), token.clone());
    (
        token,
        UploadGuard {
            upload_id: upload_id.to_string(),
        },
    )
}

struct UploadGuard {
    upload_id: String,
}

impl Drop for UploadGuard {
    fn drop(&mut self) {
        let mut guard = ACTIVE_UPLOADS.lock();
        if let Some(ref mut map) = *guard {
            map.remove(&self.upload_id);
        }
    }
}

pub struct ValidatedFile {
    pub local_path: String,
    pub file_name: String,
    pub size: u64,
}

pub fn is_windows_style_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    let drive_rooted = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/');
    drive_rooted || path.starts_with("\\\\")
}

fn prevalidate_dropped_paths(paths: &[String]) -> Result<(Vec<ValidatedFile>, u64), IpcError> {
    if paths.is_empty() {
        return Err(IpcError::new(
            IpcErrorCode::InvalidArgument,
            "No files were dropped",
        ));
    }
    if paths.len() > 32 {
        return Err(IpcError::new(
            IpcErrorCode::InvalidArgument,
            "At most 32 files can be dropped at once",
        ));
    }

    let mut validated = Vec::with_capacity(paths.len());
    let mut aggregate_total = 0u64;

    for path_str in paths {
        let path = Path::new(path_str);
        if !path.exists() {
            return Err(IpcError::new(
                IpcErrorCode::InvalidArgument,
                format!("Dropped file not found: {path_str}"),
            ));
        }
        let meta = std::fs::metadata(path).map_err(|e| {
            IpcError::new(
                IpcErrorCode::IoError,
                format!("Failed to read metadata for {path_str}: {e}"),
            )
        })?;

        if meta.is_dir() {
            return Err(IpcError::new(
                IpcErrorCode::Unsupported,
                format!("Folder uploads are not supported: {path_str}"),
            )
            .with_details(serde_json::json!({ "reason": "directoryNotSupported" })));
        }

        let size = meta.len();
        if size == 0 {
            return Err(IpcError::new(
                IpcErrorCode::InvalidArgument,
                format!("Refusing to upload empty file: {path_str}"),
            ));
        }
        if size > crate::clipboard_image::MAX_REMOTE_DROP_FILE_BYTES as u64 {
            return Err(IpcError::new(
                IpcErrorCode::PayloadTooLarge,
                format!(
                    "File {path_str} is {} MiB, exceeding the 30 MiB remote drop limit",
                    size / (1024 * 1024)
                ),
            ));
        }

        aggregate_total = aggregate_total.saturating_add(size);
        if aggregate_total > crate::clipboard_image::MAX_REMOTE_DROP_FILE_BYTES as u64 {
            return Err(IpcError::new(
                IpcErrorCode::PayloadTooLarge,
                format!(
                    "Total dropped files size exceeds the 30 MiB limit (requested {} MiB)",
                    aggregate_total / (1024 * 1024)
                ),
            ));
        }

        let raw_name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("file");
        let file_name = crate::clipboard_image::sanitize_drop_file_name(raw_name);

        validated.push(ValidatedFile {
            local_path: path_str.clone(),
            file_name,
            size,
        });
    }

    Ok((validated, aggregate_total))
}

#[tauri::command]
pub async fn cmd_remote_upload_cancel(upload_id: String) -> Result<bool, IpcError> {
    Ok(cancel_drop_upload(&upload_id))
}

#[tauri::command]
pub async fn cmd_remote_upload_dropped_files<R: Runtime>(
    app: AppHandle<R>,
    daemon: State<'_, Arc<DaemonClient>>,
    workspace_id: String,
    upload_id: String,
    paths: Vec<String>,
    progress: Channel<DropUploadProgress>,
) -> Result<Option<RemoteDropUploadResult>, IpcError> {
    crate::clipboard_image::validate_upload_id(&upload_id)?;

    let paths_clone = paths.clone();
    let (validated, aggregate_total) = run_blocking(move || prevalidate_dropped_paths(&paths_clone)).await?;

    let (cancel_token, _guard) = register_upload_token(&upload_id);

    tokio::select! {
        _ = cancel_token.cancelled() => {
            Ok(None)
        }
        result = upload_all_files(app, daemon, workspace_id, upload_id, validated, aggregate_total, progress, cancel_token.clone()) => {
            match result {
                Ok(res) => Ok(Some(res)),
                Err(e) => {
                    if cancel_token.is_cancelled() {
                        Ok(None)
                    } else {
                        Err(e)
                    }
                }
            }
        }
    }
}

async fn upload_all_files<R: Runtime>(
    app: AppHandle<R>,
    daemon: State<'_, Arc<DaemonClient>>,
    workspace_id: String,
    upload_id: String,
    files: Vec<ValidatedFile>,
    aggregate_total: u64,
    progress: Channel<DropUploadProgress>,
    cancel_token: CancellationToken,
) -> Result<RemoteDropUploadResult, IpcError> {
    let total_files = files.len();
    let mut uploaded_files = Vec::with_capacity(total_files);
    let mut aggregate_sent = 0u64;

    if workspace_id.starts_with("ssh:") {
        let store = crate::ipc::ssh::get_ssh_store_path(&app)?;
        let id = workspace_id.clone();
        let (project, host) =
            run_blocking(move || crate::ssh::projects::resolve(&store, &id)).await?;
        let environment = crate::ssh::runtime::detect_for(&host, project.platform).await?;
        let platform_str = match environment.platform {
            crate::ssh::runtime::RemotePlatform::Posix => "posix",
            crate::ssh::runtime::RemotePlatform::Windows => "windows",
        }
        .to_string();

        for (idx, item) in files.into_iter().enumerate() {
            if cancel_token.is_cancelled() {
                return Err(IpcError::new(
                    IpcErrorCode::Custom("UPLOAD_CANCELLED".into()),
                    "Upload was cancelled",
                ));
            }

            let local_path = item.local_path.clone();
            let bytes = run_blocking(move || {
                std::fs::read(&local_path)
                    .map_err(|e| IpcError::new(IpcErrorCode::IoError, format!("Read failed: {e}")))
            })
            .await?;

            let file_size = bytes.len() as u64;
            let sub_upload_id = format!("{upload_id}-{idx}");

            let _ = progress.send(DropUploadProgress {
                upload_id: upload_id.clone(),
                file_index: idx,
                total_files,
                file_sent_bytes: 0,
                file_total_bytes: file_size,
                aggregate_sent_bytes: aggregate_sent,
                aggregate_total_bytes: aggregate_total,
            });

            let remote_path = crate::ssh::operations::upload_dropped(
                &host,
                &environment,
                &sub_upload_id,
                &item.file_name,
                bytes,
            )
            .await?;

            aggregate_sent += file_size;

            let _ = progress.send(DropUploadProgress {
                upload_id: upload_id.clone(),
                file_index: idx,
                total_files,
                file_sent_bytes: file_size,
                file_total_bytes: file_size,
                aggregate_sent_bytes: aggregate_sent,
                aggregate_total_bytes: aggregate_total,
            });

            uploaded_files.push(RemoteDroppedFile {
                local_path: item.local_path,
                remote_path,
                byte_length: file_size as usize,
            });
        }

        Ok(RemoteDropUploadResult {
            platform: platform_str,
            files: uploaded_files,
        })
    } else if workspace_id.starts_with("daemon:") {
        let data_dir = app
            .path()
            .app_data_dir()
            .map_err(|e| IpcError::internal(e.to_string()))?;

        let host =
            crate::paired_host::upload::resolve_paired_host(&daemon, &data_dir, &workspace_id)
                .await?;

        for (idx, item) in files.into_iter().enumerate() {
            if cancel_token.is_cancelled() {
                return Err(IpcError::new(
                    IpcErrorCode::Custom("UPLOAD_CANCELLED".into()),
                    "Upload was cancelled",
                ));
            }

            let local_path = item.local_path.clone();
            let bytes = run_blocking(move || {
                std::fs::read(&local_path)
                    .map_err(|e| IpcError::new(IpcErrorCode::IoError, format!("Read failed: {e}")))
            })
            .await?;

            let file_size = bytes.len() as u64;
            let current_agg = aggregate_sent;
            let progress_chan = progress.clone();
            let up_id = upload_id.clone();

            let progress_fn = move |sent_file_bytes: u64| {
                let _ = progress_chan.send(DropUploadProgress {
                    upload_id: up_id.clone(),
                    file_index: idx,
                    total_files,
                    file_sent_bytes: sent_file_bytes,
                    file_total_bytes: file_size,
                    aggregate_sent_bytes: current_agg + sent_file_bytes,
                    aggregate_total_bytes: aggregate_total,
                });
            };

            let remote_path = crate::paired_host::upload::upload_temp_bytes_with_progress(
                &daemon,
                &data_dir,
                &workspace_id,
                &item.file_name,
                bytes,
                progress_fn,
            )
            .await?;

            aggregate_sent += file_size;

            uploaded_files.push(RemoteDroppedFile {
                local_path: item.local_path,
                remote_path,
                byte_length: file_size as usize,
            });
        }

        // The pasted path is interpreted by the remote shell, so its quoting style has to follow
        // the host that produced it. A paired host is normally POSIX, but a Windows host returns a
        // drive- or UNC-rooted temp path; derive the style from the path the host actually sent.
        let platform = uploaded_files
            .first()
            .map(|file| {
                if is_windows_style_path(&file.remote_path) {
                    "windows"
                } else {
                    "posix"
                }
            })
            .unwrap_or("posix")
            .to_string();

        Ok(RemoteDropUploadResult {
            platform,
            files: uploaded_files,
        })
    } else {
        Err(IpcError::new(
            IpcErrorCode::Unsupported,
            "Remote drop upload is supported only for ssh: and daemon: workspaces",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir_with_files(files: &[(&str, usize)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (name, len) in files {
            std::fs::write(dir.path().join(name), vec![b'x'; *len]).unwrap();
        }
        dir
    }

    #[test]
    fn windows_style_detection_follows_the_remote_shell() {
        assert!(is_windows_style_path(
            "C:\\Users\\dev\\AppData\\Local\\Temp\\ferryx-paste\\u1\\a.txt"
        ));
        assert!(is_windows_style_path("C:/Users/dev/Temp/ferryx-paste/u1/a.txt"));
        assert!(is_windows_style_path("\\\\server\\share\\ferryx-paste\\u1\\a.txt"));
        assert!(!is_windows_style_path("/tmp/ferryx-paste/u1/a.txt"));
        assert!(!is_windows_style_path("relative/ferryx-paste/a.txt"));
    }

    #[test]
    fn prevalidation_rejects_empty_files_before_any_upload() {
        let dir = temp_dir_with_files(&[("empty.bin", 0)]);
        let path = dir.path().join("empty.bin").to_string_lossy().into_owned();
        let err = prevalidate_dropped_paths(&[path]).err().expect("empty files must be refused");
        assert_eq!(err.code, IpcErrorCode::InvalidArgument);
    }

    #[test]
    fn prevalidation_enforces_the_30_mib_per_file_cap() {
        let limit = crate::clipboard_image::MAX_REMOTE_DROP_FILE_BYTES;
        let dir = tempfile::tempdir().unwrap();
        let oversized = dir.path().join("huge.bin");
        let file = std::fs::File::create(&oversized).unwrap();
        file.set_len(limit as u64 + 1).unwrap();
        drop(file);

        let err = prevalidate_dropped_paths(&[oversized.to_string_lossy().into_owned()])
            .err()
            .expect("oversized files must be refused");
        assert_eq!(err.code, IpcErrorCode::PayloadTooLarge);
    }

    #[test]
    fn prevalidation_enforces_the_aggregate_30_mib_cap_across_files() {
        let half = (crate::clipboard_image::MAX_REMOTE_DROP_FILE_BYTES / 2) + 1;
        let dir = temp_dir_with_files(&[("a.bin", half), ("b.bin", half)]);
        let paths = vec![
            dir.path().join("a.bin").to_string_lossy().into_owned(),
            dir.path().join("b.bin").to_string_lossy().into_owned(),
        ];

        let err = prevalidate_dropped_paths(&paths)
            .err()
            .expect("an over-budget batch must be refused");
        assert_eq!(err.code, IpcErrorCode::PayloadTooLarge);
        assert_eq!(
            err.details.as_ref().and_then(|d| d.get("reason")),
            None,
            "aggregate rejection must not be reported as a per-file directory rejection"
        );
    }

    #[test]
    fn prevalidation_rejects_directories_with_a_typed_reason() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("folder");
        std::fs::create_dir(&nested).unwrap();

        let err = prevalidate_dropped_paths(&[nested.to_string_lossy().into_owned()])
            .err()
            .expect("directories must be refused");
        assert_eq!(err.code, IpcErrorCode::Unsupported);
        assert_eq!(
            err.details.as_ref().and_then(|d| d.get("reason")).and_then(|v| v.as_str()),
            Some("directoryNotSupported")
        );
    }

    #[test]
    fn prevalidation_reports_missing_paths_and_accepts_valid_files() {
        let missing = prevalidate_dropped_paths(&["/definitely/not/here.bin".to_string()])
            .err()
            .expect("missing paths must be refused");
        assert_eq!(missing.code, IpcErrorCode::InvalidArgument);

        let dir = temp_dir_with_files(&[("ok.bin", 4)]);
        let path = dir.path().join("ok.bin").to_string_lossy().into_owned();
        let (validated, total) = prevalidate_dropped_paths(&[path]).unwrap();
        assert_eq!(validated.len(), 1);
        assert_eq!(validated[0].file_name, "ok.bin");
        assert_eq!(total, 4);
    }

    #[test]
    fn sanitized_drop_names_keep_readable_names_and_strip_separators() {
        assert_eq!(
            crate::clipboard_image::sanitize_drop_file_name("report final (1).pdf"),
            "report final (1).pdf"
        );
        assert!(!crate::clipboard_image::sanitize_drop_file_name("../../etc/passwd")
            .contains('/'));
    }
}
