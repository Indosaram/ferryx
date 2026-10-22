use std::path::Path;

use sqlx::SqlitePool;

use crate::error::{HubError, Result};
use crate::storage;

/// One-time copy of the hub tables from an existing omo-gateway database.
///
/// A second invocation is refused. Each table is copied only when the source has it.
/// A table whose columns do not match is skipped rather than failing the whole import.
/// Nothing here starts by itself: the daemon calls it only when import was invoked.
pub async fn import_once(pool: &SqlitePool, source: &Path) -> Result<ImportReport> {
    let already: Option<String> =
        sqlx::query_scalar("SELECT value FROM hub_meta WHERE key = 'imported_from'")
            .fetch_optional(pool)
            .await?;
    if already.is_some() {
        return Err(HubError::config(
            "omo-gateway import already ran for this hub database",
        ));
    }
    if !source.is_file() {
        return Err(HubError::config(format!(
            "omo-gateway database does not exist: {}",
            source.display()
        )));
    }
    let from = storage::connect(source).await?;
    let slack_home_channel = slack_home_from_omo_config(source);
    if let Some(channel) = &slack_home_channel {
        sqlx::query("INSERT INTO hub_meta(key, value) VALUES('slack_home_channel', ?)")
            .bind(channel)
            .execute(pool)
            .await?;
    }
    let report = ImportReport {
        lanes: copy_lanes(&from, pool).await?,
        watches: copy_watches(&from, pool).await?,
        notices: copy_notices(&from, pool).await?,
        issues: copy_issues(&from, pool).await?,
        slack_home_channel,
    };
    let path = source.to_string_lossy().into_owned();
    sqlx::query("INSERT INTO hub_meta(key, value) VALUES('imported_from', ?)")
        .bind(&path)
        .execute(pool)
        .await?;
    from.close().await;
    Ok(report)
}

async fn table_exists(pool: &SqlitePool, name: &str) -> Result<bool> {
    let found: Option<String> =
        sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type = 'table' AND name = ?")
            .bind(name)
            .fetch_optional(pool)
            .await?;
    Ok(found.is_some())
}

type LaneRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    String,
    String,
    Option<String>,
    Option<String>,
    i64,
    String,
);

async fn copy_lanes(from: &SqlitePool, into: &SqlitePool) -> Result<u64> {
    if !table_exists(from, "lanes").await? {
        return Ok(0);
    }
    let rows: Vec<LaneRow> = sqlx::query_as(
        "SELECT key, title, lead_session_key, cwd, COALESCE(brief_path, ''), session_key, omo_thread_id, status, done_signals, claim_evidence, claim_at, COALESCE(turns_run, 0), last_activity_at FROM lanes",
    )
    .fetch_all(from)
    .await
    .unwrap_or_default();
    let mut copied = 0u64;
    for (
        key,
        title,
        lead,
        cwd,
        brief,
        session,
        thread,
        status,
        signals,
        evidence,
        claim_at,
        turns,
        activity,
    ) in rows
    {
        let inserted = sqlx::query(
            "INSERT OR IGNORE INTO lanes(key,title,lead_session_key,cwd,brief_path,session_key,omo_thread_id,status,done_signals,claim_evidence,claim_at,turns_run,last_activity_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(key)
        .bind(title)
        .bind(lead)
        .bind(cwd)
        .bind(brief)
        .bind(session)
        .bind(thread)
        .bind(status)
        .bind(signals)
        .bind(evidence)
        .bind(claim_at)
        .bind(turns)
        .bind(activity)
        .execute(into)
        .await?;
        copied += inserted.rows_affected();
    }
    Ok(copied)
}

async fn copy_watches(from: &SqlitePool, into: &SqlitePool) -> Result<u64> {
    if !table_exists(from, "session_watches").await? {
        return Ok(0);
    }
    let rows: Vec<(String, String, String)> =
        sqlx::query_as("SELECT target, added_at, COALESCE(reason, 'manual') FROM session_watches")
            .fetch_all(from)
            .await
            .unwrap_or_default();
    let mut copied = 0u64;
    for (target, added_at, reason) in rows {
        let inserted = sqlx::query(
            "INSERT OR IGNORE INTO session_watches(target, added_at, reason) VALUES(?,?,?)",
        )
        .bind(target)
        .bind(added_at)
        .bind(reason)
        .execute(into)
        .await?;
        copied += inserted.rows_affected();
    }
    Ok(copied)
}

async fn copy_notices(from: &SqlitePool, into: &SqlitePool) -> Result<u64> {
    if !table_exists(from, "session_watch_notices").await? {
        return Ok(0);
    }
    let rows: Vec<(String, String, String, String)> =
        sqlx::query_as("SELECT dedupe_key, target, kind, sent_at FROM session_watch_notices")
            .fetch_all(from)
            .await
            .unwrap_or_default();
    let mut copied = 0u64;
    for (key, target, kind, sent_at) in rows {
        let inserted = sqlx::query(
            "INSERT OR IGNORE INTO session_watch_notices(dedupe_key, target, kind, sent_at) VALUES(?,?,?,?)",
        )
        .bind(key)
        .bind(target)
        .bind(kind)
        .bind(sent_at)
        .execute(into)
        .await?;
        copied += inserted.rows_affected();
    }
    Ok(copied)
}

async fn copy_issues(from: &SqlitePool, into: &SqlitePool) -> Result<u64> {
    if !table_exists(from, "tracker_issues").await? {
        return Ok(0);
    }
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT title, COALESCE(body, ''), COALESCE(status, 'open') FROM tracker_issues",
    )
    .fetch_all(from)
    .await
    .unwrap_or_default();
    let mut copied = 0u64;
    for (title, body, status) in rows {
        let inserted = sqlx::query("INSERT INTO tracker_issues(title, body, status, created_at) VALUES(?,?,?, datetime('now'))")
            .bind(title)
            .bind(body)
            .bind(status)
            .execute(into)
            .await?;
        copied += inserted.rows_affected();
    }
    Ok(copied)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportReport {
    pub lanes: u64,
    pub watches: u64,
    pub notices: u64,
    pub issues: u64,
    pub slack_home_channel: Option<String>,
}

/// Home channel copied by the last import, if the omo-gateway config named one.
pub async fn imported_slack_home(pool: &SqlitePool) -> Result<Option<String>> {
    let value: Option<String> =
        sqlx::query_scalar("SELECT value FROM hub_meta WHERE key = 'slack_home_channel'")
            .fetch_optional(pool)
            .await?;
    Ok(value.and_then(|value| crate::config::normalize_slack_home(&value)))
}

/// `SLACK_HOME_CHANNEL` from the omo-gateway config next to the database.
///
/// Looks in the database directory, then its parent, at `config.yaml`, `config.yml`,
/// `config.json`, and `.env`. A Discord `home_channel` is not a Slack channel.
pub fn slack_home_from_omo_config(database: &Path) -> Option<String> {
    let mut dirs = Vec::new();
    if let Some(dir) = database.parent() {
        dirs.push(dir.to_path_buf());
        if let Some(parent) = dir.parent() {
            if parent != dir {
                dirs.push(parent.to_path_buf());
            }
        }
    }
    for dir in dirs {
        for name in ["config.yaml", "config.yml", "config.json", ".env"] {
            let path = dir.join(name);
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let text = text.chars().take(65_536).collect::<String>();
            if let Some(channel) = slack_home_in_config_text(&text, name) {
                return Some(channel);
            }
        }
    }
    None
}

fn slack_home_in_config_text(text: &str, name: &str) -> Option<String> {
    if name.ends_with(".json") {
        let value: serde_json::Value = serde_json::from_str(text).ok()?;
        let raw = value
            .get("slackHomeChannel")
            .or_else(|| value.pointer("/slack/home_channel"))
            .or_else(|| value.get("SLACK_HOME_CHANNEL"))
            .and_then(|item| item.as_str())?;
        return crate::config::normalize_slack_home(raw);
    }
    if name == ".env" {
        return slack_home_from_env_file(text);
    }
    slack_home_from_yaml(text)
}

fn slack_home_from_env_file(text: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let (key, value) = line.split_once('=')?;
        if key.trim() != "SLACK_HOME_CHANNEL" {
            continue;
        }
        let value = value.trim().trim_matches('"').trim_matches('\'');
        return crate::config::normalize_slack_home(value);
    }
    None
}

fn slack_home_from_yaml(text: &str) -> Option<String> {
    let mut section = "";
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indent = line.chars().take_while(|c| c.is_whitespace()).count();
        if indent == 0 {
            section = trimmed.strip_suffix(':').unwrap_or("");
            continue;
        }
        if section != "slack" {
            continue;
        }
        let Some(value) = trimmed.strip_prefix("home_channel:") else {
            continue;
        };
        let value = value.trim().trim_matches('"').trim_matches('\'');
        return crate::config::normalize_slack_home(value);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn import_copies_lanes_once_and_refuses_a_second_run() {
        let dir = std::env::temp_dir().join(format!("ferryx-hub-import-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("omo.sqlite");
        let source_pool = storage::connect(&source).await.unwrap();
        sqlx::query("INSERT INTO lanes(key,title,lead_session_key,cwd,session_key,status,done_signals,last_activity_at) VALUES('lane-a','A','lead','/tmp','lead','queued','file /tmp/x','2020-01-01T00:00:00Z')")
            .execute(&source_pool)
            .await
            .unwrap();
        source_pool.close().await;

        let dest = storage::memory().await.unwrap();
        let report = import_once(&dest, &source).await.unwrap();
        assert_eq!(report.lanes, 1);
        let again = import_once(&dest, &source).await;
        assert!(again.is_err(), "a second import must be refused");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_omo_config_home_channel_is_slack_and_not_the_discord_one() {
        let yaml = "discord:\n  home_channel: \"999\"\nslack:\n  home_channel: \"#C0C9XG5FP8W\"\n";
        assert_eq!(slack_home_from_yaml(yaml).as_deref(), Some("C0C9XG5FP8W"));
        assert_eq!(
            slack_home_from_env_file("export SLACK_HOME_CHANNEL=\"CHOME,COTHER\"\n").as_deref(),
            Some("CHOME")
        );
        assert!(slack_home_from_yaml("discord:\n  home_channel: \"999\"\n").is_none());
    }

    #[tokio::test]
    async fn import_carries_the_slack_home_channel_and_the_hub_uses_it() {
        let dir = std::env::temp_dir().join(format!(
            "ferryx-hub-home-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.yaml"),
            "discord:\n  home_channel: \"999\"\nslack:\n  home_channel: \"C0C9XG5FP8W\"\n",
        )
        .unwrap();
        let source = dir.join("omo.sqlite");
        let source_pool = storage::connect(&source).await.unwrap();
        source_pool.close().await;

        let dest_path = dir.join("hub.sqlite");
        let dest = storage::connect(&dest_path).await.unwrap();
        let report = import_once(&dest, &source).await.unwrap();
        assert_eq!(report.slack_home_channel.as_deref(), Some("C0C9XG5FP8W"));
        assert_eq!(
            imported_slack_home(&dest).await.unwrap().as_deref(),
            Some("C0C9XG5FP8W")
        );
        dest.close().await;

        let hub = crate::hub::Hub::open(
            crate::config::HubConfig {
                database_path: Some(dest_path),
                owner_slack_id: Some("UOWNER".into()),
                ..crate::config::HubConfig::default()
            },
            std::sync::Arc::new(crate::agent_control::RecordingControl::default()),
        )
        .await
        .unwrap();
        let session = hub.owner_session();
        assert_eq!(session.channel_id, "C0C9XG5FP8W");
        assert_eq!(session.user_id, "UOWNER");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
