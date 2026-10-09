use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HangEvidence {
    pub pid: u32,
    pub process_name: String,
    pub version: String,
    pub snapshot_tag: String,
    pub wall_time_ms: f64,
    pub hang_duration_ms: u64,
    pub threshold_ms: u64,
    pub capture_strategy: String,
    pub sample_output_path: Option<String>,
    pub metadata_path: Option<String>,
    pub status: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform_diagnostics: Option<Value>,
}

pub fn default_hang_reports_dir() -> PathBuf {
    std::env::temp_dir().join("ferryx-hang-reports")
}

pub fn collect_hang_evidence(
    pid: u32,
    version: &str,
    snapshot_tag: &str,
    hang_duration_ms: u64,
    threshold_ms: u64,
    output_dir: Option<&Path>,
) -> HangEvidence {
    let process_name = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| "ferryx".to_string());

    let wall_time_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0);

    let base_dir = output_dir
        .map(|p| p.to_path_buf())
        .unwrap_or_else(default_hang_reports_dir);

    if let Err(err) = std::fs::create_dir_all(&base_dir) {
        tracing::warn!(?err, path = %base_dir.display(), "Failed to create hang reports directory");
    }

    let timestamp_u64 = wall_time_ms as u64;
    let sample_file = base_dir.join(format!("ferryx-hang-{pid}-{timestamp_u64}.sample.txt"));
    let meta_file = base_dir.join(format!("ferryx-hang-{pid}-{timestamp_u64}.json"));
    let meta_path_str = meta_file.to_string_lossy().to_string();

    let initial_summary = format!(
        "Main thread unresponsive for {hang_duration_ms}ms (threshold {threshold_ms}ms) in PID {pid} [{version}, snapshot {snapshot_tag}]; detection recorded, capture in progress"
    );

    let mut evidence = HangEvidence {
        pid,
        process_name,
        version: version.to_string(),
        snapshot_tag: snapshot_tag.to_string(),
        wall_time_ms,
        hang_duration_ms,
        threshold_ms,
        capture_strategy: "pending".to_string(),
        sample_output_path: None,
        metadata_path: Some(meta_path_str.clone()),
        status: "Detected".to_string(),
        summary: initial_summary,
        platform_diagnostics: None,
    };

    let initial_write_res = match serde_json::to_string_pretty(&evidence) {
        Ok(json_content) => std::fs::write(&meta_file, json_content),
        Err(err) => Err(std::io::Error::new(std::io::ErrorKind::Other, err)),
    };

    if let Err(err) = initial_write_res {
        tracing::warn!(?err, path = %meta_file.display(), "Failed to write initial hang metadata file");
        evidence.metadata_path = None;
        evidence.status = "Failed".to_string();
        evidence.summary = format!(
            "Failed to write metadata file for PID {pid} to {}: {err}",
            meta_file.display()
        );
        evidence.platform_diagnostics = Some(serde_json::json!({
            "metadataWriteError": err.to_string(),
        }));
        crate::ipc::debug::log_native_switch_debug(serde_json::json!({
            "event": "app.watchdog.hang_detected",
            "runId": "watchdog",
            "sequence": 0,
            "wallTimeMs": wall_time_ms,
            "details": {
                "pid": evidence.pid,
                "version": evidence.version,
                "snapshotTag": evidence.snapshot_tag,
                "hangDurationMs": evidence.hang_duration_ms,
                "thresholdMs": evidence.threshold_ms,
                "status": evidence.status,
                "strategy": evidence.capture_strategy,
                "samplePath": evidence.sample_output_path,
                "metadataPath": evidence.metadata_path,
            }
        }));
        return evidence;
    }

    let (capture_strategy, status, sample_output_path, platform_diagnostics) =
        capture_platform_stack(pid, &sample_file);

    evidence.capture_strategy = capture_strategy;
    evidence.status = status;
    evidence.sample_output_path = sample_output_path;
    evidence.platform_diagnostics = platform_diagnostics;
    evidence.summary = format!(
        "Main thread unresponsive for {hang_duration_ms}ms (threshold {threshold_ms}ms) in PID {pid} [{version}, snapshot {snapshot_tag}]; status={}",
        evidence.status
    );

    match serde_json::to_string_pretty(&evidence) {
        Ok(json_content) => {
            if let Err(err) = std::fs::write(&meta_file, json_content) {
                tracing::warn!(?err, path = %meta_file.display(), "Failed to update hang metadata file");
                evidence.metadata_path = None;
                evidence.status = "Failed".to_string();
                evidence.platform_diagnostics = Some(serde_json::json!({
                    "metadataUpdateError": err.to_string(),
                }));
            }
        }
        Err(err) => {
            evidence.status = "Failed".to_string();
            evidence.metadata_path = None;
            evidence.platform_diagnostics = Some(serde_json::json!({
                "metadataSerializeError": err.to_string(),
            }));
        }
    }

    crate::ipc::debug::log_native_switch_debug(serde_json::json!({
        "event": "app.watchdog.hang_detected",
        "runId": "watchdog",
        "sequence": 0,
        "wallTimeMs": wall_time_ms,
        "details": {
            "pid": evidence.pid,
            "version": evidence.version,
            "snapshotTag": evidence.snapshot_tag,
            "hangDurationMs": evidence.hang_duration_ms,
            "thresholdMs": evidence.threshold_ms,
            "status": evidence.status,
            "strategy": evidence.capture_strategy,
            "samplePath": evidence.sample_output_path,
            "metadataPath": evidence.metadata_path,
        }
    }));

    evidence
}

#[cfg(target_os = "macos")]
fn capture_platform_stack(
    pid: u32,
    sample_file: &Path,
) -> (String, String, Option<String>, Option<Value>) {
    let strategy = "macos-sample".to_string();
    let sample_bin = Path::new("/usr/bin/sample");
    if !sample_bin.exists() {
        let fallback_msg = format!("macOS sample binary not found at /usr/bin/sample for PID {pid}\n");
        return match std::fs::write(sample_file, fallback_msg) {
            Ok(()) => (
                strategy,
                "Fallback".to_string(),
                Some(sample_file.to_string_lossy().to_string()),
                Some(serde_json::json!({ "error": "/usr/bin/sample missing" })),
            ),
            Err(err) => (
                strategy,
                "Failed".to_string(),
                None,
                Some(serde_json::json!({
                    "error": "/usr/bin/sample missing",
                    "writeError": err.to_string(),
                })),
            ),
        };
    }

    let spawn_res = std::process::Command::new(sample_bin)
        .arg(pid.to_string())
        .arg("1")
        .arg("-file")
        .arg(sample_file)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();

    let mut child = match spawn_res {
        Ok(c) => c,
        Err(err) => {
            let fallback_content = format!("Failed to spawn /usr/bin/sample: {err}\n");
            return match std::fs::write(sample_file, fallback_content) {
                Ok(()) => (
                    strategy,
                    "Fallback".to_string(),
                    Some(sample_file.to_string_lossy().to_string()),
                    Some(serde_json::json!({ "spawnError": err.to_string() })),
                ),
                Err(write_err) => (
                    strategy,
                    "Failed".to_string(),
                    None,
                    Some(serde_json::json!({
                        "spawnError": err.to_string(),
                        "writeError": write_err.to_string(),
                    })),
                ),
            };
        }
    };

    let deadline = Instant::now() + Duration::from_secs(10);
    let mut finished_status = None;

    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(status)) => {
                finished_status = Some(status);
                break;
            }
            Ok(None) => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(err) => {
                let _ = child.kill();
                let _ = child.wait();
                let fallback = format!("Wait error on sample child: {err}\n");
                return match std::fs::write(sample_file, fallback) {
                    Ok(()) => (
                        strategy,
                        "Fallback".to_string(),
                        Some(sample_file.to_string_lossy().to_string()),
                        Some(serde_json::json!({ "waitError": err.to_string() })),
                    ),
                    Err(w_err) => (
                        strategy,
                        "Failed".to_string(),
                        None,
                        Some(serde_json::json!({
                            "waitError": err.to_string(),
                            "writeError": w_err.to_string(),
                        })),
                    ),
                };
            }
        }
    }

    let status = match finished_status {
        Some(st) => st,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            let timeout_msg = format!("macOS sample child timed out after 10s for PID {pid}\n");
            return match std::fs::write(sample_file, timeout_msg) {
                Ok(()) => (
                    strategy,
                    "Failed".to_string(),
                    Some(sample_file.to_string_lossy().to_string()),
                    Some(serde_json::json!({ "error": "sample command timed out" })),
                ),
                Err(w_err) => (
                    strategy,
                    "Failed".to_string(),
                    None,
                    Some(serde_json::json!({
                        "error": "sample command timed out",
                        "writeError": w_err.to_string(),
                    })),
                ),
            };
        }
    };

    if status.success() && sample_file.exists() {
        (
            strategy,
            "Captured".to_string(),
            Some(sample_file.to_string_lossy().to_string()),
            Some(serde_json::json!({
                "exitCode": status.code(),
            })),
        )
    } else {
        let fallback_content = format!(
            "macOS sample command exited with non-zero code {:?}\n",
            status.code()
        );
        match std::fs::write(sample_file, fallback_content) {
            Ok(()) => (
                strategy,
                "Fallback".to_string(),
                Some(sample_file.to_string_lossy().to_string()),
                Some(serde_json::json!({
                    "exitCode": status.code(),
                })),
            ),
            Err(w_err) => (
                strategy,
                "Failed".to_string(),
                None,
                Some(serde_json::json!({
                    "exitCode": status.code(),
                    "writeError": w_err.to_string(),
                })),
            ),
        }
    }
}

#[cfg(target_os = "linux")]
fn capture_platform_stack(
    pid: u32,
    sample_file: &Path,
) -> (String, String, Option<String>, Option<Value>) {
    let strategy = "linux-proc".to_string();
    let mut diagnostics = serde_json::Map::new();
    let mut report = format!("Linux Process Diagnostics for PID {pid}\n");

    let status_path = format!("/proc/{pid}/status");
    if let Ok(content) = std::fs::read_to_string(&status_path) {
        report.push_str("\n--- /proc/pid/status ---\n");
        report.push_str(&content);
        diagnostics.insert("statusRead".to_string(), Value::Bool(true));
    }

    let wchan_path = format!("/proc/{pid}/wchan");
    if let Ok(wchan) = std::fs::read_to_string(&wchan_path) {
        report.push_str("\n--- /proc/pid/wchan ---\n");
        report.push_str(&wchan);
        diagnostics.insert("wchan".to_string(), Value::String(wchan.trim().to_string()));
    }

    let stack_path = format!("/proc/{pid}/stack");
    if let Ok(stack) = std::fs::read_to_string(&stack_path) {
        report.push_str("\n--- /proc/pid/stack ---\n");
        report.push_str(&stack);
        diagnostics.insert("kernelStackAvailable".to_string(), Value::Bool(true));
    }

    match std::fs::write(sample_file, report) {
        Ok(()) => (
            strategy,
            "Captured".to_string(),
            Some(sample_file.to_string_lossy().to_string()),
            Some(Value::Object(diagnostics)),
        ),
        Err(err) => (
            strategy,
            "Failed".to_string(),
            None,
            Some(serde_json::json!({
                "writeError": err.to_string(),
                "diagnostics": diagnostics,
            })),
        ),
    }
}

#[cfg(target_os = "windows")]
fn capture_platform_stack(
    pid: u32,
    sample_file: &Path,
) -> (String, String, Option<String>, Option<Value>) {
    let strategy = "windows-fallback".to_string();
    let wall_now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let report = format!(
        "Windows Process Diagnostics for PID {pid}\nTimestamp: {wall_now}\nDiagnostics: Main thread event loop unresponsiveness detected\n"
    );

    match std::fs::write(sample_file, report) {
        Ok(()) => (
            strategy,
            "Captured".to_string(),
            Some(sample_file.to_string_lossy().to_string()),
            Some(serde_json::json!({
                "platform": "windows",
                "timestamp": wall_now,
            })),
        ),
        Err(err) => (
            strategy,
            "Failed".to_string(),
            None,
            Some(serde_json::json!({
                "platform": "windows",
                "writeError": err.to_string(),
            })),
        ),
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn capture_platform_stack(
    pid: u32,
    sample_file: &Path,
) -> (String, String, Option<String>, Option<Value>) {
    let strategy = "portable-fallback".to_string();
    let report = format!("Generic Process Diagnostics for PID {pid}\n");

    match std::fs::write(sample_file, report) {
        Ok(()) => (
            strategy,
            "Captured".to_string(),
            Some(sample_file.to_string_lossy().to_string()),
            Some(serde_json::json!({ "platform": "generic" })),
        ),
        Err(err) => (
            strategy,
            "Failed".to_string(),
            None,
            Some(serde_json::json!({
                "platform": "generic",
                "writeError": err.to_string(),
            })),
        ),
    }
}

#[cfg(test)]
pub fn exercise_evidence_collector_seam(
    target_pid: u32,
    custom_dir: &Path,
    custom_duration_ms: u64,
) -> Result<HangEvidence, String> {
    let evidence = collect_hang_evidence(
        target_pid,
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_VERSION"),
        custom_duration_ms,
        5000,
        Some(custom_dir),
    );

    if evidence.status == "Failed" {
        return Err(format!("Collection failed: {}", evidence.summary));
    }

    if let Some(ref meta_path) = evidence.metadata_path {
        if !Path::new(meta_path).exists() {
            return Err(format!("Metadata file was not created at {meta_path}"));
        }
    } else {
        return Err("No metadata path returned in evidence".to_string());
    }

    if let Some(ref sample_path) = evidence.sample_output_path {
        if !Path::new(sample_path).exists() {
            return Err(format!("Sample file was not created at {sample_path}"));
        }
    }

    Ok(evidence)
}
