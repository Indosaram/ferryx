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

/// Past the cap we skip (not truncate): a hung client must not lose earlier stages.
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
        let size = std::fs::metadata(path)
            .map(|m| m.len())
            .unwrap_or_default();
        if size > max_bytes {
            return Ok(());
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
    file.write_all(line.as_bytes())
        .map_err(|error| {
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
        let dir = std::env::temp_dir().join(format!(
            "ferryx-boot-trace-test-{}",
            std::process::id()
        ));
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
    fn skips_append_past_size_cap() {
        let dir = std::env::temp_dir().join(format!(
            "ferryx-boot-trace-cap-{}",
            std::process::id()
        ));
        let path = dir.join("boot-trace.log");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, "x".repeat(2048)).unwrap();
        append_boot_trace(&path, "should.skip", &json!(null), 1024).unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(!body.contains("should.skip"));
        let _ = std::fs::remove_file(&path);
    }
}
