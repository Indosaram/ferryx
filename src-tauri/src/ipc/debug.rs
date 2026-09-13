use crate::ipc::{run_blocking, IpcError};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchDebugEntry {
    run_id: String,
    sequence: u64,
    event: String,
    wall_time_ms: f64,
    details: Value,
}

/// Whether the switch-debug sink should persist entries.
///
/// A debug build always traces. A release build traces only when the operator
/// opts in with `FERRYX_SWITCH_DEBUG=1`, which is what makes the shipped app
/// observable without running it under the Vite dev server (and therefore
/// without HMR reloads tearing the page down mid-keystroke).
pub fn switch_debug_sink_enabled(debug_build: bool, env_flag: Option<&str>) -> bool {
    debug_build || env_flag == Some("1")
}

fn switch_debug_sink_enabled_here() -> bool {
    switch_debug_sink_enabled(
        cfg!(debug_assertions),
        std::env::var("FERRYX_SWITCH_DEBUG").ok().as_deref(),
    )
}

fn switch_debug_path(root: &std::path::Path) -> std::path::PathBuf {
    root.join("ferryx-switch-debug.jsonl")
}

/// Native callbacks must not perform filesystem I/O on the UI thread.
/// Unlike the IPC command, they cannot return sink errors to a caller.
pub(crate) fn log_native_switch_debug(entry: Value) {
    if !switch_debug_sink_enabled_here() {
        return;
    }
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(error) = append_switch_debug_entry(&std::env::temp_dir(), &entry) {
            tracing::warn!(?error, "Could not persist native switch debug entry");
        }
    });
}

fn append_switch_debug_entry(
    root: &std::path::Path,
    entry: &impl Serialize,
) -> Result<(), IpcError> {
    use std::fs::OpenOptions;
    use std::io::Write;

    let serialized = serde_json::to_string(entry)
        .map_err(|error| IpcError::internal(format!("serialize switch debug entry: {error}")))?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(switch_debug_path(root))
        .map_err(|error| IpcError::internal(format!("open switch debug log: {error}")))?;
    writeln!(file, "{serialized}")
        .map_err(|error| IpcError::internal(format!("write switch debug log: {error}")))
}

#[tauri::command]
pub async fn cmd_switch_debug_log(entry: SwitchDebugEntry) -> Result<(), IpcError> {
    if !switch_debug_sink_enabled_here() {
        return Ok(());
    }

    run_blocking(move || append_switch_debug_entry(&std::env::temp_dir(), &entry))
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsed_entries_append_to_injected_portable_root() {
        let root = tempfile::tempdir().expect("owned sink root");
        let path = switch_debug_path(root.path());
        // Fail before any I/O if the sink escapes the owned fixture.
        assert_eq!(path.parent(), Some(root.path()));
        let wire = serde_json::json!({
            "runId": "p02-owned", "sequence": 7, "event": "wheel.receipt",
            "wallTimeMs": 123.0, "details": {"cell": [2, 3], "ctrl": true}
        });
        let entry: SwitchDebugEntry = serde_json::from_value(wire.clone()).expect("parsed event");
        append_switch_debug_entry(root.path(), &entry).expect("first append");
        append_switch_debug_entry(root.path(), &entry).expect("second append");
        let text = std::fs::read_to_string(path).expect("read actual sink");
        let entries: Vec<Value> = text.lines().map(|line| serde_json::from_str(line).expect("JSONL record")).collect();
        assert_eq!(entries, vec![wire.clone(), wire]);
        root.close().expect("remove owned sink");
    }

    #[test]
    fn invalid_sink_root_returns_error() {
        let root = tempfile::tempdir().expect("owned sink root");
        let invalid = root.path().join("file-not-directory");
        assert_eq!(switch_debug_path(&invalid).parent(), Some(invalid.as_path()));
        std::fs::write(&invalid, b"sentinel").expect("create non-directory");
        assert!(append_switch_debug_entry(&invalid, &serde_json::json!({"event": "test"})).is_err());
        assert_eq!(std::fs::read(&invalid).expect("read sentinel"), b"sentinel");
        root.close().expect("remove owned sink");
    }

    #[test]
    fn debug_builds_always_trace() {
        assert!(switch_debug_sink_enabled(true, None));
    }

    #[test]
    fn release_builds_stay_silent_by_default() {
        assert!(!switch_debug_sink_enabled(false, None));
    }

    #[test]
    fn release_builds_trace_when_opted_in() {
        assert!(switch_debug_sink_enabled(false, Some("1")));
    }

    #[test]
    fn release_builds_ignore_any_other_flag_value() {
        assert!(!switch_debug_sink_enabled(false, Some("true")));
        assert!(!switch_debug_sink_enabled(false, Some("0")));
    }
}
