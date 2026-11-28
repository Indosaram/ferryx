//! Durable per-message obligations to answer an owner in the originating conversation.
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::SqlitePool;
use tokio::time::{sleep_until, Instant};

const HOLDING_AFTER_SECS: i64 = 60;
const REMINDER_LIMIT: i64 = 3;
const REMINDER_WINDOW_SECS: i64 = 30 * 60;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Obligation {
    pub id: i64,
    pub adapter: String,
    pub conversation_id: String,
    pub source_message_id: String,
    pub channel_id: Option<String>,
    pub thread_id: Option<String>,
    pub turn_status: String,
    pub admitted_at: String,
    pub reminded_count: i64,
    pub last_reminded_at: Option<String>,
    pub answered_at: Option<String>,
    pub delivery_state: String,
    pub last_error: Option<String>,
}

#[async_trait]
pub trait ObligationNotifier: Send + Sync {
    async fn send_holding(&self, ob: &Obligation, text: &str) -> crate::error::Result<bool>;
    async fn wait_for(&self, duration: Duration) {
        sleep_until(Instant::now() + duration).await;
    }
}

pub async fn open_obligation(
    pool: &SqlitePool, adapter: &str, conversation_id: &str, source_message_id: &str,
    channel_id: Option<&str>, thread_id: Option<&str>,
) -> crate::error::Result<i64> {
    sqlx::query(
        "INSERT INTO owner_answer_obligations(adapter, conversation_id, source_message_id, channel_id, thread_id, admitted_at)
         VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT(adapter, source_message_id) DO NOTHING",
    )
    .bind(adapter).bind(conversation_id).bind(source_message_id)
    .bind(channel_id).bind(thread_id).bind(Utc::now().to_rfc3339())
    .execute(pool).await?;
    Ok(sqlx::query_scalar("SELECT id FROM owner_answer_obligations WHERE adapter = ? AND source_message_id = ?")
        .bind(adapter).bind(source_message_id).fetch_one(pool).await?)
}

pub async fn update_status(pool: &SqlitePool, adapter: &str, source_message_id: &str, status: &str) -> crate::error::Result<()> {
    sqlx::query("UPDATE owner_answer_obligations SET turn_status = ? WHERE adapter = ? AND source_message_id = ? AND answered_at IS NULL")
        .bind(status).bind(adapter).bind(source_message_id).execute(pool).await?;
    Ok(())
}

async fn load(pool: &SqlitePool, id: i64) -> crate::error::Result<Option<Obligation>> {
    Ok(sqlx::query_as("SELECT * FROM owner_answer_obligations WHERE id = ?").bind(id).fetch_optional(pool).await?)
}

pub async fn mark_answered(pool: &SqlitePool, adapter: &str, source_message_id: &str) -> crate::error::Result<()> {
    sqlx::query("UPDATE owner_answer_obligations SET answered_at = ?, delivery_state = 'delivered', last_error = NULL WHERE adapter = ? AND source_message_id = ? AND answered_at IS NULL")
        .bind(Utc::now().to_rfc3339()).bind(adapter).bind(source_message_id).execute(pool).await?;
    Ok(())
}

pub async fn mark_delivery_uncertain(pool: &SqlitePool, adapter: &str, source_message_id: &str, error: &str) -> crate::error::Result<()> {
    sqlx::query("UPDATE owner_answer_obligations SET delivery_state = 'uncertain', last_error = ? WHERE adapter = ? AND source_message_id = ? AND answered_at IS NULL")
        .bind(error).bind(adapter).bind(source_message_id).execute(pool).await?;
    Ok(())
}

pub async fn uncertain_deliveries(pool: &SqlitePool) -> crate::error::Result<Vec<Obligation>> {
    Ok(sqlx::query_as("SELECT * FROM owner_answer_obligations WHERE delivery_state = 'uncertain' AND answered_at IS NULL ORDER BY admitted_at").fetch_all(pool).await?)
}

pub async fn run_obligation_timer(
    pool: SqlitePool,
    id: i64,
    notifier: Arc<dyn ObligationNotifier>,
) -> crate::error::Result<()> {
    loop {
        let Some(ob) = load(&pool, id).await? else { return Ok(()) };
        if ob.answered_at.is_some() || ob.reminded_count >= REMINDER_LIMIT { return Ok(()) }
        let admitted = DateTime::parse_from_rfc3339(&ob.admitted_at)
            .map_err(|error| crate::error::HubError::message(error.to_string()))?
            .with_timezone(&Utc);
        let deadline = admitted + chrono::Duration::seconds(HOLDING_AFTER_SECS * (ob.reminded_count + 1));
        let cutoff = admitted + chrono::Duration::seconds(REMINDER_WINDOW_SECS);
        if deadline >= cutoff { return Ok(()) }
        let remaining = deadline.signed_duration_since(Utc::now()).to_std().unwrap_or_default();
        notifier.wait_for(remaining.max(Duration::from_millis(1))).await;
        let Some(current) = load(&pool, id).await? else { return Ok(()) };
        if current.answered_at.is_some() { return Ok(()) }
        let now = Utc::now();
        if now >= cutoff { return Ok(()) }
        // Claim the reminder slot before posting, so two overlapping timers cannot both post it.
        let claimed = sqlx::query("UPDATE owner_answer_obligations SET reminded_count = reminded_count + 1, last_reminded_at = ? WHERE id = ? AND answered_at IS NULL AND reminded_count = ?")
            .bind(now.to_rfc3339()).bind(id).bind(current.reminded_count).execute(&pool).await?;
        if claimed.rows_affected() == 0 { return Ok(()) }
        if !notifier.send_holding(&current, &format!("작업 중: {}", current.turn_status)).await? {
            return Ok(())
        }
    }
}

/// Writes a holding line into the app transcript, which is where every panel surface reads the
/// owner's conversation: the desktop app, the CLI, MCP and a paired phone all share it. Slack
/// and Discord obligations belong to their own notifier.
#[derive(Clone)]
pub struct AppTranscriptNotifier(pub SqlitePool);

#[async_trait]
impl ObligationNotifier for AppTranscriptNotifier {
    async fn send_holding(&self, ob: &Obligation, text: &str) -> crate::error::Result<bool> {
        if matches!(ob.adapter.as_str(), "slack" | "discord") { return Ok(false) }
        sqlx::query("INSERT INTO supervisor_app_transcript(role, text, evidence_json, request_id, created_at) VALUES('supervisor', ?, '[]', ?, ?)")
            .bind(text)
            .bind(format!("holding:{}:{}:{}:{}", ob.adapter, ob.source_message_id, ob.reminded_count + 1, Utc::now().timestamp_millis()))
            .bind(Utc::now().timestamp_millis())
            .execute(&self.0)
            .await?;
        Ok(true)
    }
}

/// Re-arms every open obligation. The returned set owns the timers: dropping or aborting its
/// owner aborts them all, so they end with the hub generation that armed them.
pub async fn reconstruct_pending_obligations(
    pool: &SqlitePool,
    notifier: Arc<dyn ObligationNotifier>,
) -> crate::error::Result<tokio::task::JoinSet<()>> {
    let obligations: Vec<Obligation> = sqlx::query_as("SELECT * FROM owner_answer_obligations WHERE answered_at IS NULL AND reminded_count < ? ORDER BY admitted_at")
        .bind(REMINDER_LIMIT).fetch_all(pool).await?;
    let now = Utc::now();
    let pending: Vec<Obligation> = obligations.into_iter().filter(|ob| {
        DateTime::parse_from_rfc3339(&ob.admitted_at).ok().is_some_and(|at| now.signed_duration_since(at).num_seconds() < REMINDER_WINDOW_SECS)
    }).collect();
    let mut timers = tokio::task::JoinSet::new();
    for ob in pending {
        let pool = pool.clone();
        let notifier = notifier.clone();
        timers.spawn(async move {
            if let Err(error) = run_obligation_timer(pool, ob.id, notifier).await {
                tracing::error!(obligation_id = ob.id, %error, "owner answer obligation timer failed");
            }
        });
    }
    Ok(timers)
}

#[cfg(test)]
mod tests;
