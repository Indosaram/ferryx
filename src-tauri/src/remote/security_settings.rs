//! Persisted security switches for the remote transports.
//!
//! The insecure-transport environment variables (`FERRYX_ALLOW_INSECURE_DIRECT`,
//! `FERRYX_ALLOW_INSECURE_LAN`, `FERRYX_ALLOW_INSECURE_RELAY`) are development escape hatches. A
//! release build honours them only when the matching flag is also persisted here, so an inherited
//! or injected environment alone cannot downgrade a shipped install to plaintext transport.
//!
//! The same file carries the user's "disable automatic direct trust" choice, so the desktop UI and
//! the daemon read one answer. Every read goes to disk: the GUI and the daemon are separate
//! processes and a cached copy would let one of them keep acting on a stale choice.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const SECURITY_SETTINGS_FILE: &str = "security-settings.json";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SecuritySettings {
    /// Lets a release build honour `FERRYX_ALLOW_INSECURE_DIRECT` / `FERRYX_ALLOW_INSECURE_LAN`.
    pub allow_insecure_direct_env: bool,
    /// Lets a release build honour `FERRYX_ALLOW_INSECURE_RELAY`.
    pub allow_insecure_relay_env: bool,
    /// The user turned account-attested automatic direct trust off on this machine.
    pub disable_auto_trust: bool,
}

fn settings_path(dir: Option<&Path>) -> Option<PathBuf> {
    match dir {
        Some(dir) => Some(dir.join(SECURITY_SETTINGS_FILE)),
        None => crate::remote::auth::canonical_remote_dir()
            .map(|dir| dir.join(SECURITY_SETTINGS_FILE)),
    }
}

/// Blocking. A missing or unreadable file is the closed default: no insecure opt-in, auto-trust on.
pub fn load_in(dir: Option<&Path>) -> SecuritySettings {
    settings_path(dir)
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Blocking: the settings in the canonical remote state directory.
pub fn load() -> SecuritySettings {
    load_in(None)
}

/// Blocking: writes the settings owner-only and atomically.
pub fn store_in(dir: Option<&Path>, settings: &SecuritySettings) -> Result<(), String> {
    let path = settings_path(dir)
        .ok_or_else(|| "cannot resolve the remote settings directory".to_string())?;
    crate::remote::auth::write_private_json(&path, settings)
        .map_err(|error| format!("failed to persist remote security settings: {error}"))
}

fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Whether an insecure-transport environment opt-in takes effect: only when it is set AND either
/// this is a debug build or the user persisted the matching flag. The persisted flag is read
/// lazily so a process without the env never touches the disk.
pub fn insecure_env_effective(
    env_set: bool,
    debug_build: bool,
    persisted_opt_in: impl FnOnce() -> bool,
) -> bool {
    env_set && (debug_build || persisted_opt_in())
}

/// `FERRYX_ALLOW_INSECURE_DIRECT` / `FERRYX_ALLOW_INSECURE_LAN`, gated as described above.
pub fn insecure_direct_env_enabled() -> bool {
    insecure_env_effective(
        env_flag("FERRYX_ALLOW_INSECURE_DIRECT") || env_flag("FERRYX_ALLOW_INSECURE_LAN"),
        cfg!(debug_assertions),
        || load().allow_insecure_direct_env,
    )
}

/// `FERRYX_ALLOW_INSECURE_RELAY`, gated as described above.
pub fn insecure_relay_env_enabled() -> bool {
    insecure_env_effective(
        env_flag("FERRYX_ALLOW_INSECURE_RELAY"),
        cfg!(debug_assertions),
        || load().allow_insecure_relay_env,
    )
}

/// The insecure transport modes in effect in this process, for the desktop warning banner.
pub fn active_insecure_modes() -> Vec<String> {
    let mut modes = Vec::new();
    if crate::remote::server::is_insecure_direct_allowed() {
        modes.push("direct".to_string());
    }
    if crate::remote::relay_client::is_insecure_relay_allowed() {
        modes.push("relay".to_string());
    }
    modes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insecure_env_is_ignored_in_release_without_setting() {
        // A release build with the env set and nothing persisted: the env is ignored.
        assert!(!insecure_env_effective(true, false, || false));
        // The persisted flag is what lets a release build honour the env.
        assert!(insecure_env_effective(true, false, || true));
        // Debug builds keep the developer escape hatch.
        assert!(insecure_env_effective(true, true, || false));
        // Nothing turns insecure transport on without the env itself.
        assert!(!insecure_env_effective(false, true, || true));

        // The persisted flag round-trips, and an absent file is the closed default.
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(load_in(Some(dir.path())), SecuritySettings::default());
        let settings = SecuritySettings {
            allow_insecure_relay_env: true,
            ..SecuritySettings::default()
        };
        store_in(Some(dir.path()), &settings).expect("store");
        let loaded = load_in(Some(dir.path()));
        assert!(loaded.allow_insecure_relay_env);
        assert!(!loaded.allow_insecure_direct_env);
        assert!(!loaded.disable_auto_trust);
    }
}
