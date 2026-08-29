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
pub(crate) fn switch_debug_sink_enabled(debug_build: bool, env_flag: Option<&str>) -> bool {
    debug_build || env_flag == Some("1")
}

fn switch_debug_sink_enabled_here() -> bool {
    switch_debug_sink_enabled(
        cfg!(debug_assertions),
        std::env::var("FERRYX_SWITCH_DEBUG").ok().as_deref(),
    )
}

#[tauri::command]
pub async fn cmd_switch_debug_log(entry: SwitchDebugEntry) -> Result<(), IpcError> {
    if !switch_debug_sink_enabled_here() {
        return Ok(());
    }

    run_blocking(move || {
        use std::fs::OpenOptions;
        use std::io::Write;

        let serialized = serde_json::to_string(&entry).map_err(|error| {
            IpcError::internal(format!("serialize switch debug entry: {error}"))
        })?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open("/tmp/ferryx-switch-debug.jsonl")
            .map_err(|error| IpcError::internal(format!("open switch debug log: {error}")))?;
        writeln!(file, "{serialized}")
            .map_err(|error| IpcError::internal(format!("write switch debug log: {error}")))?;
        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::switch_debug_sink_enabled;

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
