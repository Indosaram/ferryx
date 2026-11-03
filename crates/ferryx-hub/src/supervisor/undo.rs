//! Undo of one supervisor action-log entry (the panel's "Undo" on a log line).
//!
//! The panel and the gateway call [`ActionUndo`]. [`LoggedUndo`] reads the entry's undo handle
//! from the T2 action log and runs the handler its `kind` names:
//! - `restoreRule` (payload `{"rule": <StandingRule>}`): puts the previous rule back through the
//!   rule store. This is what a plain-language rule change records.
//!
//! Any other kind (an answer the agent cannot take back, a retry) answers
//! [`UndoError::NotSupported`] and leaves the entry untouched. After a handler ran, the entry's
//! undo handle is cleared ([`ActionLog::mark_undone`]), so a second undo is `Nothing`.
//! [`NoUndo`] is the stub for a hub without the stores.
use std::sync::Arc;

use async_trait::async_trait;

use super::panel_contract::{ActionLog, RuleStore, StandingRule};

/// The handler kind that restores a rule.
pub const RESTORE_RULE: &str = "restoreRule";

/// Why an undo did not run.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UndoError {
    /// No undo is implemented for this action yet.
    #[error("undoing this action is not supported yet")]
    NotSupported,
    /// The entry does not exist or has no undo handle left.
    #[error("action {0} has nothing to undo")]
    Nothing(i64),
    /// The stores failed; nothing was marked undone.
    #[error("undo failed: {0}")]
    Storage(String),
}

impl UndoError {
    /// The stable wire code a refusal carries.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotSupported => "UNSUPPORTED",
            Self::Nothing(_) => "NOT_FOUND",
            Self::Storage(_) => "INTERNAL",
        }
    }
}

/// Reverses what action-log entry `id` did, using its undo handle.
#[async_trait]
pub trait ActionUndo: Send + Sync {
    async fn undo(&self, id: i64) -> Result<(), UndoError>;
}

/// The stub: no action can be undone.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoUndo;

#[async_trait]
impl ActionUndo for NoUndo {
    async fn undo(&self, _id: i64) -> Result<(), UndoError> {
        Err(UndoError::NotSupported)
    }
}

/// Undo over the persisted action log and rule store.
pub struct LoggedUndo {
    log: Arc<dyn ActionLog>,
    rules: Arc<dyn RuleStore>,
    now_ms: Arc<dyn Fn() -> i64 + Send + Sync>,
}

impl LoggedUndo {
    pub fn new(
        log: Arc<dyn ActionLog>,
        rules: Arc<dyn RuleStore>,
        now_ms: Arc<dyn Fn() -> i64 + Send + Sync>,
    ) -> Self {
        Self { log, rules, now_ms }
    }
}

fn storage(error: impl std::fmt::Display) -> UndoError {
    UndoError::Storage(error.to_string())
}

#[async_trait]
impl ActionUndo for LoggedUndo {
    async fn undo(&self, id: i64) -> Result<(), UndoError> {
        let entries = self.log.list_today(0).await.map_err(storage)?;
        let handle = entries
            .into_iter()
            .find(|entry| entry.id == id)
            .and_then(|entry| entry.undo)
            .ok_or(UndoError::Nothing(id))?;
        match handle.kind.as_str() {
            RESTORE_RULE => {
                let rule: StandingRule = serde_json::from_value(
                    handle.payload.get("rule").cloned().unwrap_or_default(),
                )
                .map_err(storage)?;
                self.rules.upsert(rule).await.map_err(storage)?;
            }
            _ => return Err(UndoError::NotSupported),
        }
        self.log
            .mark_undone(id, (self.now_ms)())
            .await
            .map_err(storage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::supervisor::activity::SqliteActionLog;
    use crate::supervisor::panel_contract::{ActionEntry, ActionKind, UndoHandle};
    use crate::supervisor::rules::SqliteRuleStore;
    use serde_json::json;

    #[tokio::test]
    async fn the_stub_refuses_every_undo_as_not_supported() {
        let error = NoUndo.undo(7).await.unwrap_err();
        assert_eq!(error, UndoError::NotSupported);
        assert_eq!(error.code(), "UNSUPPORTED");
    }

    async fn fixture() -> (LoggedUndo, SqliteActionLog, SqliteRuleStore) {
        let pool = crate::storage::memory().await.expect("hub db");
        let log = SqliteActionLog::new(pool.clone());
        let rules = SqliteRuleStore::open(pool, 1).await.expect("rules");
        let undo = LoggedUndo::new(
            Arc::new(log.clone()),
            Arc::new(rules.clone()),
            Arc::new(|| 99),
        );
        (undo, log, rules)
    }

    fn entry(key: &str, undo: Option<UndoHandle>) -> ActionEntry {
        ActionEntry {
            id: 0,
            incident_key: key.into(),
            session_ref: None,
            kind: ActionKind::RuleChanged,
            summary: "Changed the retry interval".into(),
            basis: "you asked".into(),
            evidence: Vec::new(),
            undo,
            live: false,
            created_at_ms: 10,
            updated_at_ms: 10,
        }
    }

    #[tokio::test]
    async fn undoing_a_rule_change_restores_the_previous_rule_once() {
        let (undo, log, rules) = fixture().await;
        let before = rules.list().await.unwrap()[0].clone();
        let mut changed = before.clone();
        changed.params = json!({ "backoffSecs": [300], "thenEverySecs": 300 });
        rules.upsert(changed.clone()).await.unwrap();
        let stored = log
            .upsert_incident(entry(
                "rule-1",
                Some(UndoHandle {
                    kind: RESTORE_RULE.into(),
                    payload: json!({ "rule": before }),
                }),
            ))
            .await
            .unwrap();

        undo.undo(stored.id).await.expect("undo runs");

        let restored = rules
            .list()
            .await
            .unwrap()
            .into_iter()
            .find(|rule| rule.id == before.id)
            .unwrap();
        assert_eq!(restored.params, before.params, "the previous rule is back");
        assert_ne!(restored.params, changed.params);
        assert_eq!(
            undo.undo(stored.id).await.unwrap_err(),
            UndoError::Nothing(stored.id),
            "the handle is spent after one undo"
        );
    }

    #[tokio::test]
    async fn an_unknown_handle_kind_is_not_supported_and_keeps_its_handle() {
        let (undo, log, _) = fixture().await;
        let stored = log
            .upsert_incident(entry(
                "answer-1",
                Some(UndoHandle {
                    kind: "takeBackAnswer".into(),
                    payload: json!({}),
                }),
            ))
            .await
            .unwrap();

        assert_eq!(undo.undo(stored.id).await.unwrap_err(), UndoError::NotSupported);
        let kept = log.list_today(0).await.unwrap();
        assert!(kept[0].undo.is_some(), "a refused undo leaves the handle");
    }

    #[tokio::test]
    async fn an_entry_without_a_handle_or_a_missing_entry_has_nothing_to_undo() {
        let (undo, log, _) = fixture().await;
        let stored = log.upsert_incident(entry("plain", None)).await.unwrap();
        assert_eq!(undo.undo(stored.id).await.unwrap_err(), UndoError::Nothing(stored.id));
        assert_eq!(undo.undo(4242).await.unwrap_err(), UndoError::Nothing(4242));
        assert_eq!(UndoError::Nothing(1).code(), "NOT_FOUND");
    }
}
