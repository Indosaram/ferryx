//! Best-effort startup trace: append-only `<runtime>/boot-trace.log`, safe to ignore.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::json;

use crate::daemon::server::get_runtime_dir;
use crate::ipc::error::{IpcError, IpcErrorCode};
use crate::ipc::run_blocking;

/// Past the cap the log rotates once to `boot-trace.log.1`, so a long-running GUI keeps
/// recording native presentation evidence instead of going silent for the rest of its life.
const BOOT_TRACE_MAX_BYTES: u64 = 1024 * 1024;

#[derive(Deserialize)]
pub struct BootTraceRequest {
    pub stage: String,
    #[serde(default)]
    pub details: serde_json::Value,
}

#[tauri::command]
pub async fn cmd_boot_trace(request: BootTraceRequest) -> Result<(), IpcError> {
    run_blocking(move || {
        append_boot_trace(
            &boot_trace_path(),
            &request.stage,
            &request.details,
            BOOT_TRACE_MAX_BYTES,
        )
    })
    .await
}

pub fn boot_trace_path() -> PathBuf {
    get_runtime_dir().join("boot-trace.log")
}

pub(crate) fn append_boot_trace(
    path: &Path,
    stage: &str,
    details: &serde_json::Value,
    max_bytes: u64,
) -> Result<(), IpcError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            IpcError::new(
                IpcErrorCode::InternalError,
                format!("boot trace dir creation failed: {error}"),
            )
        })?;
    }
    if path.exists() {
        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or_default();
        if size > max_bytes {
            let mut rotated = path.as_os_str().to_owned();
            rotated.push(".1");
            std::fs::rename(path, PathBuf::from(rotated)).map_err(|error| {
                IpcError::new(
                    IpcErrorCode::InternalError,
                    format!("boot trace rotation failed: {error}"),
                )
            })?;
        }
    }
    let wall_time_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let record = json!({
        "wallTimeMs": wall_time_ms,
        "stage": stage,
        "details": details,
    });
    let mut line = record.to_string();
    line.push('\n');
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| {
            IpcError::new(
                IpcErrorCode::InternalError,
                format!("boot trace append failed: {error}"),
            )
        })?;
    file.write_all(line.as_bytes()).map_err(|error| {
        IpcError::new(
            IpcErrorCode::InternalError,
            format!("boot trace write failed: {error}"),
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_structured_lines() {
        let dir =
            std::env::temp_dir().join(format!("ferryx-boot-trace-test-{}", std::process::id()));
        let path = dir.join("boot-trace.log");
        let _ = std::fs::remove_file(&path);
        append_boot_trace(&path, "boot.start", &json!({"pid": 1}), 1024).unwrap();
        append_boot_trace(&path, "initial.ok", &json!({"ms": 12}), 1024).unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = body.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("\"stage\":\"boot.start\""));
        assert!(lines[1].contains("\"stage\":\"initial.ok\""));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rotates_past_size_cap_and_keeps_appending() {
        let dir =
            std::env::temp_dir().join(format!("ferryx-boot-trace-cap-{}", std::process::id()));
        let path = dir.join("boot-trace.log");
        let rotated = dir.join("boot-trace.log.1");
        std::fs::create_dir_all(&dir).unwrap();
        let _ = std::fs::remove_file(&rotated);
        std::fs::write(&path, "x".repeat(2048)).unwrap();
        append_boot_trace(&path, "after.cap", &json!(null), 1024).unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("after.cap"), "a full trace must keep recording");
        assert!(!body.contains("xxxx"), "the full trace moves aside instead of growing");
        assert_eq!(std::fs::read_to_string(&rotated).unwrap(), "x".repeat(2048));
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&rotated);
    }
}
