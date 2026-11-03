//! Persisted supervisor settings and the owner enablement check.
//!
//! The settings live in `hub_meta` (key [`SETTINGS_KEY`]) beside the pause flag
//! ([`crate::control::set_paused`]), so they survive a restart and never touch a PTY.
//!
//! Enablement is bound to the owner's home: [`owner_home`] resolves `OMO_HOME`, else
//! `<home>/.omo`. Turning the supervisor on stamps that directory into the settings, and
//! [`is_enabled`] is true only while the running daemon resolves the same directory. A hub
//! database copied to another account or machine therefore starts disabled until its owner
//! turns it on there. `remove_all` is the destructive bulk switch and is off by default; only
//! [`set_remove_all`] turns it on.
use std::path::Path;

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::error::{HubError, Result};

/// The `hub_meta` key the settings are stored under.
pub const SETTINGS_KEY: &str = "supervisor_settings";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupervisorSettings {
    /// The owner turned the supervisor on (autonomous answers and recovery).
    #[serde(default)]
    pub enabled: bool,
    /// Bulk removal of supervisor-owned work. Off until the owner turns it on.
    #[serde(default)]
    pub remove_all: bool,
    /// The owner home that was current when `enabled` was set.
    #[serde(default)]
    pub owner_home: Option<String>,
}

/// The owner's home directory: `omo_home` when set, else `<home>/.omo`. Blank values count as
/// unset; `None` when neither names a directory.
pub fn owner_home(omo_home: Option<&str>, home: Option<&str>) -> Option<String> {
    let present = |value: Option<&str>| value.map(str::trim).filter(|value| !value.is_empty());
    if let Some(dir) = present(omo_home) {
        return Some(dir.to_owned());
    }
    present(home).map(|home| Path::new(home).join(".omo").to_string_lossy().into_owned())
}

/// [`owner_home`] from this process: `OMO_HOME`, else `HOME` (or `USERPROFILE` on Windows).
pub fn current_owner_home() -> Option<String> {
    let omo_home = crate::hub_env::var("OMO_HOME").ok();
    let home = crate::hub_env::var("HOME")
        .ok()
        .or_else(|| crate::hub_env::var("USERPROFILE").ok());
    owner_home(omo_home.as_deref(), home.as_deref())
}

/// The stored settings; a hub that never saved any has the defaults (everything off).
pub async fn load(pool: &SqlitePool) -> Result<SupervisorSettings> {
    let stored: Option<String> = sqlx::query_scalar("SELECT value FROM hub_meta WHERE key = ?1")
        .bind(SETTINGS_KEY)
        .fetch_optional(pool)
        .await?;
    match stored {
        None => Ok(SupervisorSettings::default()),
        Some(text) => serde_json::from_str(&text).map_err(|error| {
            HubError::message(format!("supervisor settings are unreadable: {error}"))
        }),
    }
}

async fn save(pool: &SqlitePool, settings: &SupervisorSettings) -> Result<()> {
    let text =
        serde_json::to_string(settings).map_err(|error| HubError::message(error.to_string()))?;
    sqlx::query(
        "INSERT INTO hub_meta(key, value) VALUES(?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(SETTINGS_KEY)
    .bind(text)
    .execute(pool)
    .await?;
    Ok(())
}

/// The owner turns the supervisor on (stamping `owner_home`) or off. Returns the saved settings.
pub async fn set_enabled(
    pool: &SqlitePool,
    enabled: bool,
    owner_home: &str,
) -> Result<SupervisorSettings> {
    let mut settings = load(pool).await?;
    settings.enabled = enabled;
    if enabled {
        settings.owner_home = Some(owner_home.to_owned());
    }
    save(pool, &settings).await?;
    Ok(settings)
}

/// The owner turns bulk removal on or off. Returns the saved settings.
pub async fn set_remove_all(pool: &SqlitePool, remove_all: bool) -> Result<SupervisorSettings> {
    let mut settings = load(pool).await?;
    settings.remove_all = remove_all;
    save(pool, &settings).await?;
    Ok(settings)
}

/// Whether the supervisor is enabled for the owner whose home is `current_owner_home`.
pub async fn is_enabled(pool: &SqlitePool, current_owner_home: Option<&str>) -> Result<bool> {
    let settings = load(pool).await?;
    Ok(settings.enabled
        && current_owner_home.is_some()
        && settings.owner_home.as_deref() == current_owner_home)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omo_home_wins_and_the_fallback_is_dot_omo_under_home() {
        assert_eq!(
            owner_home(Some("/srv/omo"), Some("/home/a")).as_deref(),
            Some("/srv/omo")
        );
        let fallback = owner_home(None, Some("/home/a")).expect("fallback");
        assert_eq!(Path::new(&fallback), Path::new("/home/a").join(".omo"));
        assert_eq!(
            owner_home(Some("  "), Some("/home/a")),
            owner_home(None, Some("/home/a"))
        );
        assert_eq!(owner_home(None, None), None);
        assert_eq!(owner_home(Some(""), Some("")), None);
    }

    #[tokio::test]
    async fn a_fresh_hub_has_everything_off() {
        let pool = crate::storage::memory().await.expect("hub db");
        let settings = load(&pool).await.unwrap();
        assert!(!settings.enabled);
        assert!(!settings.remove_all, "RemoveAll defaults off");
        assert!(!is_enabled(&pool, Some("/home/a/.omo")).await.unwrap());
    }

    #[tokio::test]
    async fn enablement_holds_only_for_the_owner_home_that_set_it() {
        let pool = crate::storage::memory().await.expect("hub db");
        set_enabled(&pool, true, "/home/a/.omo").await.unwrap();
        assert!(is_enabled(&pool, Some("/home/a/.omo")).await.unwrap());
        assert!(
            !is_enabled(&pool, Some("/home/b/.omo")).await.unwrap(),
            "another owner home is not enabled"
        );
        assert!(
            !is_enabled(&pool, None).await.unwrap(),
            "no resolvable home is not enabled"
        );
        set_enabled(&pool, false, "/home/a/.omo").await.unwrap();
        assert!(!is_enabled(&pool, Some("/home/a/.omo")).await.unwrap());
    }

    #[tokio::test]
    async fn enabling_leaves_remove_all_off_and_the_settings_persist() {
        let pool = crate::storage::memory().await.expect("hub db");
        let saved = set_enabled(&pool, true, "/home/a/.omo").await.unwrap();
        assert!(saved.enabled && !saved.remove_all);
        assert_eq!(load(&pool).await.unwrap(), saved);
        set_remove_all(&pool, true).await.unwrap();
        let after = load(&pool).await.unwrap();
        assert!(after.remove_all && after.enabled, "the two switches are independent");
    }
}
