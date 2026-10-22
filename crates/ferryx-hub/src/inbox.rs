//! Event-armed inbox. A message older than 24 hours is filed as fyi without a model call.
//! The production path has no model fallback: a missing classification is an error.
use chrono::{DateTime, TimeDelta, Utc};
use serde_json::Value;
use sqlx::SqlitePool;

use crate::error::{HubError, Result};
use crate::storage::now_rfc3339;

pub const STALE_AFTER_HOURS: i64 = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Owner,
    Bot,
    Fyi,
    Ignore,
}

impl Class {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "owner_must_act" | "owner" => Ok(Self::Owner),
            "bot_can_handle" | "bot" => Ok(Self::Bot),
            "fyi" => Ok(Self::Fyi),
            "ignore" => Ok(Self::Ignore),
            _ => Err(HubError::message("inbox judge returned no classification")),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Bot => "bot",
            Self::Fyi => "fyi",
            Self::Ignore => "ignore",
        }
    }
}

pub fn is_stale(received: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    now.signed_duration_since(received) > TimeDelta::hours(STALE_AFTER_HOURS)
}

/// Stale mail is fyi. A fresh item uses the model class and does not invent one.
pub fn classify(
    received: DateTime<Utc>,
    now: DateTime<Utc>,
    model_class: Option<&str>,
) -> Result<Class> {
    if is_stale(received, now) {
        return Ok(Class::Fyi);
    }
    Class::parse(model_class.unwrap_or(""))
}

/// JSON, then NDJSON, then YAML. Empty output is an empty list.
pub fn decode_structured_output(bytes: &[u8]) -> Result<Value> {
    if bytes.is_empty() || bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(Value::Array(Vec::new()));
    }
    if let Ok(value) = serde_json::from_slice::<Value>(bytes) {
        return Ok(value);
    }
    let text = String::from_utf8_lossy(bytes);
    let mut items = Vec::new();
    let mut ndjson = true;
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        match serde_json::from_str::<Value>(line) {
            Ok(item) => items.push(item),
            Err(_) => {
                ndjson = false;
                break;
            }
        }
    }
    if ndjson && !items.is_empty() {
        return Ok(Value::Array(items));
    }
    serde_yaml::from_slice::<Value>(bytes)
        .map_err(|error| HubError::message(format!("invalid structured output: {error}")))
}

pub fn digest(items: &[(String, Class)]) -> String {
    let mut lines = vec!["# inbox".to_owned()];
    for class in [Class::Owner, Class::Bot, Class::Fyi, Class::Ignore] {
        let rows: Vec<_> = items
            .iter()
            .filter(|(_, item)| *item == class)
            .map(|(title, _)| title.as_str())
            .collect();
        if rows.is_empty() {
            continue;
        }
        lines.push(format!("## {}", class.as_str()));
        for title in rows {
            lines.push(format!("- {title}"));
        }
    }
    lines.join("\n")
}

pub async fn ingest(
    pool: &SqlitePool,
    id: &str,
    source: &str,
    source_ref: &str,
    snippet: &str,
    received_at: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT OR IGNORE INTO inbox_items(id, source, source_ref, snippet, received_at, raw_hash, collected_at) VALUES(?,?,?,?,?,?,?)",
    )
    .bind(id)
    .bind(source)
    .bind(source_ref)
    .bind(snippet)
    .bind(received_at)
    .bind(source_ref)
    .bind(now_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn decide(
    pool: &SqlitePool,
    item_id: &str,
    class: Class,
    reason: &str,
    proposed_action: Option<&str>,
) -> Result<()> {
    let status = if proposed_action.is_some() {
        "proposed"
    } else {
        "open"
    };
    sqlx::query(
        "INSERT OR REPLACE INTO inbox_decisions(item_id, class, reason, proposed_action, status, decided_at) VALUES(?,?,?,?,?,?)",
    )
    .bind(item_id)
    .bind(class.as_str())
    .bind(reason)
    .bind(proposed_action)
    .bind(status)
    .bind(now_rfc3339())
    .execute(pool)
    .await?;
    if class == Class::Bot {
        if let Some(action) = proposed_action {
            crate::ledger::propose(pool, "inbox", item_id, action, action, false).await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_mail_is_fyi_and_a_fresh_item_needs_a_real_class() {
        let now = DateTime::parse_from_rfc3339("2026-10-22T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let old = now - TimeDelta::hours(25);
        assert_eq!(classify(old, now, None).unwrap(), Class::Fyi);
        assert_eq!(
            classify(old, now, Some("owner_must_act")).unwrap(),
            Class::Fyi
        );
        let fresh = now - TimeDelta::hours(1);
        assert!(classify(fresh, now, None).is_err());
        assert_eq!(
            classify(fresh, now, Some("bot_can_handle")).unwrap(),
            Class::Bot
        );
        let decoded = decode_structured_output(b"{\"id\":1}\n{\"id\":2}\n").unwrap();
        assert_eq!(decoded.as_array().unwrap().len(), 2);
        let yaml = decode_structured_output(b"- owner_must_act\n- fyi\n").unwrap();
        assert_eq!(yaml[0], "owner_must_act");
        let digest = digest(&[("pay".into(), Class::Owner), ("ci".into(), Class::Fyi)]);
        assert!(digest.contains("pay"));
        assert!(digest.contains("## fyi"));
    }

    #[tokio::test]
    async fn a_bot_item_becomes_a_proposal_and_a_stale_one_does_not() {
        let pool = crate::storage::memory().await.unwrap();
        ingest(
            &pool,
            "in1",
            "github",
            "ref",
            "[CI failed] red",
            "2026-10-22T00:00:00Z",
        )
        .await
        .unwrap();
        decide(
            &pool,
            "in1",
            Class::Bot,
            "agent can rerun",
            Some("rerun checks"),
        )
        .await
        .unwrap();
        let proposals: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM action_proposals WHERE origin = 'inbox'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(proposals, 1);
        ingest(
            &pool,
            "in2",
            "mail",
            "old",
            "newsletter",
            "2020-01-01T00:00:00Z",
        )
        .await
        .unwrap();
        let now = Utc::now();
        let received = DateTime::parse_from_rfc3339("2020-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let class = classify(received, now, None).unwrap();
        decide(&pool, "in2", class, "stale", None).await.unwrap();
        let stored: String =
            sqlx::query_scalar("SELECT class FROM inbox_decisions WHERE item_id = 'in2'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(stored, "fyi");
    }
}
