//! Desktop view of account-attested automatic direct trust.
//!
//! The user's toggle is persisted in the remote security settings file, which the daemon and
//! this process both read on every trust decision, so no daemon round-trip is needed for the
//! choice to take effect.
use crate::ipc::{run_blocking, IpcError};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectAutoTrustStatus {
    /// Whether automatic trust is in effect (env opt-out and the persisted toggle both allow it).
    pub enabled: bool,
    /// The user's persisted "disable auto-trust" choice.
    pub disabled_by_setting: bool,
    /// `FERRYX_DIRECT_AUTO_TRUST` turns it off for this process regardless of the toggle.
    pub disabled_by_env: bool,
    /// The account signing key this machine pins, if any.
    pub pinned_account_key: Option<String>,
}

fn read_status() -> DirectAutoTrustStatus {
    use crate::paired_host::direct_trust::{auto_trust_enabled_from, AUTO_TRUST_ENV};
    let disabled_by_env = !auto_trust_enabled_from(std::env::var(AUTO_TRUST_ENV).ok().as_deref());
    let disabled_by_setting = crate::remote::security_settings::load().disable_auto_trust;
    DirectAutoTrustStatus {
        enabled: !disabled_by_env && !disabled_by_setting,
        disabled_by_setting,
        disabled_by_env,
        pinned_account_key: crate::remote::direct_lease::pinned_account_public_key(),
    }
}

#[tauri::command]
pub async fn cmd_remote_auto_trust_status() -> Result<DirectAutoTrustStatus, IpcError> {
    run_blocking(|| Ok(read_status())).await
}

#[tauri::command]
pub async fn cmd_remote_set_auto_trust_disabled(
    disabled: bool,
) -> Result<DirectAutoTrustStatus, IpcError> {
    run_blocking(move || {
        let mut settings = crate::remote::security_settings::load();
        settings.disable_auto_trust = disabled;
        crate::remote::security_settings::store_in(None, &settings).map_err(IpcError::internal)?;
        Ok(read_status())
    })
    .await
}
