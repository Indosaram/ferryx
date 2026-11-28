use chrono::{TimeZone, Utc};
use sqlx::SqlitePool;

use crate::supervisor::memory_import::{daily_tokens, OMO_MEMORY_CAUSE};
use crate::supervisor::{ensure_identity, memory_import::{import_sessions_budgeted, ImportBounds, ImportSession, ImportTurn, OMO_MEMORY_PROVIDER}};
use crate::meta::mhc_fault::SupervisorFault;
use crate::meta::mhc_supervisor::{IngestOutcome, MemoryRequestError, OwnerMemory};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

async fn store() -> SqlitePool {
    let pool = crate::storage::memory().await.unwrap();
    ensure_identity(&pool, "desktop:owner").await.unwrap();
    pool
}

#[derive(Default)]
struct RefusingMemory(Mutex<usize>);

#[async_trait]
impl OwnerMemory for RefusingMemory {
    async fn memory_request(&self, _name: &str, _data: Value) -> Result<Value, MemoryRequestError> {
        Ok(json!({}))
    }

    async fn ingest_owner_text(&self, _source: &str, _text: &str) -> Result<IngestOutcome, SupervisorFault> {
        *self.0.lock().unwrap() += 1;
        Err(SupervisorFault::provider("prompt refused: Cannot submit a prompt while compaction is in progress"))
    }
}

fn one_session() -> Vec<ImportSession> {
    vec![ImportSession {
        provider: OMO_MEMORY_PROVIDER.into(),
        session_id: "compaction-session".into(),
        cwd: None,
        turns: vec![ImportTurn { ordinal: 0, text: "Prefer focused commits".into() }],
    }]
}

#[tokio::test]
async fn three_consecutive_refusals_end_the_pass_and_report_health() {
    // Given: an owner identity and an ingest engine that continually refuses compaction prompts.
    let pool = store().await;
    let memory = Arc::new(RefusingMemory::default());
    // When: a single OMO turn is imported.
    let report = import_sessions_budgeted(&pool, memory.as_ref(), "desktop:owner", one_session(), ImportBounds::default(), None).await.unwrap();
    let health = crate::supervisor::memory_import::last_pass(&pool, crate::supervisor::memory_import::OMO_MEMORY_PASS_KEY).await.unwrap().unwrap();
    // Then: exactly three attempts end the pass and the stop reason is persisted for health.
    assert_eq!(*memory.0.lock().unwrap(), 3);
    assert_eq!(report.refusals, 3);
    assert!(report.stop_reason.as_deref().unwrap().contains("three consecutive refusals"));
    assert_eq!(health["report"]["refusals"], 3);
    assert!(health["stopReason"].as_str().unwrap().contains("three consecutive refusals"));
}

#[tokio::test]
async fn the_daily_token_cap_stops_passes_until_tomorrow() {
    // Given: a UTC-day cap already reached by a known-usage call.
    let pool = store().await;
    sqlx::query("INSERT INTO supervisor_model_calls(cause, role, prompt_bytes, input_tokens, output_tokens, outcome, ms, created_at) VALUES(?, 'owner_ingest', 1, 10, 1, 'ok', 1, ?)")
        .bind(OMO_MEMORY_CAUSE).bind("2026-11-27T12:00:00Z").execute(&pool).await.unwrap();
    let today = Utc.with_ymd_and_hms(2026, 11, 27, 13, 0, 0).unwrap();
    // When: the breaker evaluates the exhausted day, then the next UTC day.
    let reached = daily_tokens(&pool, today, 10).await.unwrap();
    let tomorrow = daily_tokens(&pool, Utc.with_ymd_and_hms(2026, 11, 28, 0, 0, 0).unwrap(), 10).await.unwrap();
    // Then: it holds today and opens only on the next UTC day.
    assert!(reached.reached);
    assert_eq!(reached.input_tokens, 10);
    assert!(!tomorrow.reached);
}

#[tokio::test]
async fn unknown_daily_token_counts_reach_the_cap() {
    // Given: an OMO engine call with unknown input usage on the current day.
    let pool = store().await;
    sqlx::query("INSERT INTO supervisor_model_calls(cause, role, prompt_bytes, outcome, ms, created_at) VALUES(?, 'owner_ingest', 1, 'MHC_PROVIDER', 1, ?)")
        .bind(OMO_MEMORY_CAUSE).bind("2026-11-27T12:00:00Z").execute(&pool).await.unwrap();
    // When: daily usage is counted.
    let status = daily_tokens(&pool, Utc.with_ymd_and_hms(2026, 11, 27, 13, 0, 0).unwrap(), 2_000_000).await.unwrap();
    // Then: unknown usage closes the breaker.
    assert!(status.reached);
    assert_eq!(status.unknown_calls, 1);
}

#[tokio::test]
async fn compaction_refusal_does_not_trip_daily_token_breaker_while_unknown_ok_trips_it() {
    // Given: a compaction refusal (admission refused, no tokens billed) followed by a successful retry.
    let pool = store().await;
    sqlx::query("INSERT INTO supervisor_model_calls(cause, role, prompt_bytes, outcome, ms, created_at) VALUES(?, 'owner_ingest', 1, 'MHC_PROTOCOL', 1, ?)")
        .bind(OMO_MEMORY_CAUSE).bind("2026-11-27T12:00:00Z").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO supervisor_model_calls(cause, role, prompt_bytes, input_tokens, output_tokens, outcome, ms, created_at) VALUES(?, 'owner_ingest', 1, 50, 10, 'ok', 1, ?)")
        .bind(OMO_MEMORY_CAUSE).bind("2026-11-27T12:01:00Z").execute(&pool).await.unwrap();
    let today = Utc.with_ymd_and_hms(2026, 11, 27, 13, 0, 0).unwrap();

    // When: daily usage is evaluated after the refusal and retry.
    let status = daily_tokens(&pool, today, 2_000_000).await.unwrap();

    // Then: the refusal does not count as unknown calls and does not trip the breaker.
    assert_eq!(status.unknown_calls, 0);
    assert_eq!(status.input_tokens, 50);
    assert!(!status.reached);

    // Given: an 'ok' call with unknown input tokens (input_tokens IS NULL).
    let ok_pool = store().await;
    sqlx::query("INSERT INTO supervisor_model_calls(cause, role, prompt_bytes, outcome, ms, created_at) VALUES(?, 'owner_ingest', 1, 'ok', 1, ?)")
        .bind(OMO_MEMORY_CAUSE).bind("2026-11-27T12:02:00Z").execute(&ok_pool).await.unwrap();

    // When: daily usage is counted for the ok call with null input tokens.
    let ok_status = daily_tokens(&ok_pool, today, 2_000_000).await.unwrap();

    // Then: the ok call with null input tokens counts as unknown calls and trips the breaker.
    assert_eq!(ok_status.unknown_calls, 1);
    assert!(ok_status.reached);
}
