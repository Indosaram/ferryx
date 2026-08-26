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

#[tauri::command]
pub async fn cmd_switch_debug_log(entry: SwitchDebugEntry) -> Result<(), IpcError> {
    #[cfg(debug_assertions)]
    {
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

    #[cfg(not(debug_assertions))]
    {
        let _ = entry;
        Ok(())
    }
}
