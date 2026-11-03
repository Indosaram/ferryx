//! Persisted standing rules (`supervisor_rules`, migration 0046).
//!
//! [`SqliteRuleStore::open`] seeds [`default_rules`] once, when the table is empty, so the D2
//! defaults are in effect with no instruction and survive a restart.
use async_trait::async_trait;
use serde::de::DeserializeOwned;
use sqlx::SqlitePool;

use super::panel_contract::{default_rules, RuleStore, StandingRule};
use crate::error::{HubError, Result};
use crate::storage::WriteTx;

/// [`RuleStore`] over the hub's SQLite pool.
#[derive(Clone)]
pub struct SqliteRuleStore {
    pool: SqlitePool,
}

type Row = (i64, String, String, String, Option<String>, i64);

fn to_json<T: serde::Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|error| HubError::message(error.to_string()))
}

fn from_json<T: DeserializeOwned>(text: &str) -> Result<T> {
    serde_json::from_str(text).map_err(|error| HubError::message(error.to_string()))
}

fn rule_of(row: Row) -> Result<StandingRule> {
    let (id, scope, kind, params, created_from_message_id, created_at_ms) = row;
    Ok(StandingRule {
        id,
        scope: from_json(&scope)?,
        kind: from_json(&to_json(&kind)?)?,
        params: from_json(&params)?,
        created_from_message_id,
        created_at_ms,
    })
}

impl SqliteRuleStore {
    /// Opens the store and seeds the default rules, stamped `now_ms`, if there are none yet.
    pub async fn open(pool: SqlitePool, now_ms: i64) -> Result<Self> {
        let mut tx = pool.begin_write().await?;
        let existing: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM supervisor_rules")
            .fetch_one(&mut *tx)
            .await?;
        if existing == 0 {
            for rule in default_rules() {
                sqlx::query(
                    "INSERT INTO supervisor_rules(scope_json, kind, params_json,
                         created_from_message_id, created_at)
                     VALUES(?,?,?,?,?)",
                )
                .bind(to_json(&rule.scope)?)
                .bind(from_json::<String>(&to_json(&rule.kind)?)?)
                .bind(to_json(&rule.params)?)
                .bind(rule.created_from_message_id)
                .bind(now_ms)
                .execute(&mut *tx)
                .await?;
            }
        }
        tx.commit().await?;
        Ok(Self { pool })
    }
}

#[async_trait]
impl RuleStore for SqliteRuleStore {
    async fn list(&self) -> Result<Vec<StandingRule>> {
        let rows: Vec<Row> = sqlx::query_as(
            "SELECT id, scope_json, kind, params_json, created_from_message_id, created_at
             FROM supervisor_rules ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(rule_of).collect()
    }

    async fn upsert(&self, rule: StandingRule) -> Result<StandingRule> {
        let mut tx = self.pool.begin_write().await?;
        let result = sqlx::query(
            "INSERT INTO supervisor_rules(id, scope_json, kind, params_json,
                 created_from_message_id, created_at)
             VALUES(?,?,?,?,?,?)
             ON CONFLICT(id) DO UPDATE SET
                 scope_json = excluded.scope_json, kind = excluded.kind,
                 params_json = excluded.params_json,
                 created_from_message_id = excluded.created_from_message_id",
        )
        .bind((rule.id != 0).then_some(rule.id))
        .bind(to_json(&rule.scope)?)
        .bind(from_json::<String>(&to_json(&rule.kind)?)?)
        .bind(to_json(&rule.params)?)
        .bind(&rule.created_from_message_id)
        .bind(rule.created_at_ms)
        .execute(&mut *tx)
        .await?;
        let id = if rule.id == 0 {
            result.last_insert_rowid()
        } else {
            rule.id
        };
        let row: Row = sqlx::query_as(
            "SELECT id, scope_json, kind, params_json, created_from_message_id, created_at
             FROM supervisor_rules WHERE id = ?",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        rule_of(row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::supervisor::panel_contract::{RuleKind, RuleScope};

    #[tokio::test]
    async fn defaults_are_present_on_an_empty_database_and_seeded_once() {
        let pool = crate::storage::memory().await.unwrap();
        let store = SqliteRuleStore::open(pool.clone(), 5_000).await.unwrap();
        let again = SqliteRuleStore::open(pool, 9_000).await.unwrap();

        let rules = again.list().await.unwrap();
        assert_eq!(rules, store.list().await.unwrap());
        let kinds: Vec<RuleKind> = rules.iter().map(|rule| rule.kind).collect();
        let expected: Vec<RuleKind> = default_rules().iter().map(|rule| rule.kind).collect();
        assert_eq!(kinds, expected);
        let params: Vec<_> = rules.iter().map(|rule| rule.params.clone()).collect();
        let expected: Vec<_> = default_rules()
            .into_iter()
            .map(|rule| rule.params)
            .collect();
        assert_eq!(params, expected);
        assert!(rules.iter().all(|rule| rule.created_at_ms == 5_000));
    }

    #[tokio::test]
    async fn rules_survive_reopening_the_database() {
        let dir = std::env::temp_dir().join(format!("ferryx-hub-rules-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("hub.sqlite");

        let pool = crate::storage::connect(&path).await.unwrap();
        let store = SqliteRuleStore::open(pool.clone(), 1_000).await.unwrap();
        let added = store
            .upsert(StandingRule {
                id: 0,
                scope: RuleScope {
                    repo: Some("github.com/example/ferryx".to_string()),
                    ..RuleScope::default()
                },
                kind: RuleKind::StallRestart,
                params: serde_json::json!({"thresholdSecs": 900, "maxRestarts": 2}),
                created_from_message_id: Some("m1".to_string()),
                created_at_ms: 2_000,
            })
            .await
            .unwrap();
        assert_eq!(added.id, default_rules().len() as i64 + 1);
        let before = store.list().await.unwrap();
        pool.close().await;

        let reopened = crate::storage::connect(&path).await.unwrap();
        let store = SqliteRuleStore::open(reopened, 3_000).await.unwrap();
        assert_eq!(store.list().await.unwrap(), before);
        assert!(before.contains(&added));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn upsert_with_an_existing_id_replaces_that_rule() {
        let pool = crate::storage::memory().await.unwrap();
        let store = SqliteRuleStore::open(pool, 1_000).await.unwrap();
        let mut first = store.list().await.unwrap().remove(0);
        first.params = serde_json::json!({"backoffSecs": [30], "thenEverySecs": 30});

        let stored = store.upsert(first.clone()).await.unwrap();

        assert_eq!(stored, first);
        let rules = store.list().await.unwrap();
        assert_eq!(rules.len(), default_rules().len());
        assert_eq!(rules[0], first);
    }
}
