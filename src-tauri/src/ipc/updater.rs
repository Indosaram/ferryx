use crate::ipc::error::IpcError;

/// Store (MSIX) installs live under the write-protected `WindowsApps` tree where the Store
/// owns update delivery; self-updating those installs would fail or duplicate the app.
#[cfg(windows)]
pub fn updater_managed_externally() -> bool {
    std::env::current_exe()
        .map(|exe| exe.to_string_lossy().to_lowercase().contains(r"\windowsapps\"))
        .unwrap_or(false)
}

#[cfg(not(windows))]
pub fn updater_managed_externally() -> bool {
    false
}

#[tauri::command]
pub async fn cmd_updater_managed_externally() -> Result<bool, IpcError> {
    Ok(updater_managed_externally())
}
