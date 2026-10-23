//! Delivers lane events that have not been handed to a lead yet.
//! This is the queue, not an in-gateway model loop.
use sqlx::SqlitePool;

use crate::chat::session::SessionKey;
use crate::error::Result;
use crate::storage::now_rfc3339;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneEvent {
    pub id: i64,
    pub lane_key: Option<String>,
    pub lead_session_key: String,
    pub kind: String,
    pub body: String,
}

pub async fn enqueue(
    pool: &SqlitePool,
    lane_key: &str,
    lead: &str,
    kind: &str,
    body: &str,
) -> Result<i64> {
    let result = sqlx::query(
        "INSERT INTO lane_events(lane_key, lead_session_key, kind, body, created_at) VALUES(?,?,?,?,?)",
    )
    .bind(lane_key)
    .bind(lead)
    .bind(kind)
    .bind(body)
    .bind(now_rfc3339())
    .execute(pool)
    .await?;
    Ok(result.last_insert_rowid())
}

pub async fn take_undelivered(pool: &SqlitePool) -> Result<Vec<LaneEvent>> {
    let now = now_rfc3339();
    let rows: Vec<(i64, Option<String>, String, String, String)> = sqlx::query_as(
        "SELECT id, lane_key, lead_session_key, kind, body FROM lane_events
         WHERE delivered_at IS NULL AND (release_at IS NULL OR release_at <= ?)
         ORDER BY id",
    )
    .bind(&now)
    .fetch_all(pool)
    .await?;
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    for (id, _, _, _, _) in &rows {
        sqlx::query("UPDATE lane_events SET delivered_at = ? WHERE id = ?")
            .bind(&now)
            .bind(id)
            .execute(pool)
            .await?;
    }
    Ok(rows
        .into_iter()
        .map(|(id, lane_key, lead_session_key, kind, body)| LaneEvent {
            id,
            lane_key,
            lead_session_key,
            kind,
            body,
        })
        .collect())
}

/// One undelivered event as omo's `EventDispatcher` reads it.
#[derive(Debug, Clone)]
struct QueuedEvent {
    id: i64,
    lane_key: Option<String>,
    lead: String,
    kind: String,
    body: String,
    created_at: chrono::DateTime<chrono::Utc>,
}

/// omo-gateway's `EventDispatcher`, as one tick. Events urgent for the owner stay on the lead
/// (the owner DM); routine ones go to the home channel session. A group is admitted when its
/// oldest event is 30s old (urgent kinds at once), the target had no delivery in the last 30s
/// and `admit` accepts the turn (false while the session is busy). Events are collapsed per
/// (lane, kind) to the newest body, non-critical ones pass the wake judgment, and the rest are
/// receipted. An event whose lead is not a session key (the old `meta` lead) can never be
/// delivered and is receipted as misrouted.
pub async fn dispatch_tick<A, F>(
    pool: &SqlitePool,
    judge: &crate::judge::Judge,
    now: chrono::DateTime<chrono::Utc>,
    mut admit: A,
) -> Result<usize>
where
    A: FnMut(SessionKey, String) -> F,
    F: std::future::Future<Output = bool>,
{
    let stamp = now.to_rfc3339();
    let rows: Vec<(i64, Option<String>, String, String, String, String)> = sqlx::query_as(
        "SELECT id, lane_key, lead_session_key, kind, body, created_at FROM lane_events
         WHERE delivered_at IS NULL AND (release_at IS NULL OR release_at <= ?)
         ORDER BY id",
    )
    .bind(&stamp)
    .fetch_all(pool)
    .await?;
    let mut misrouted = Vec::new();
    let mut groups = std::collections::BTreeMap::<String, Vec<QueuedEvent>>::new();
    for (id, lane_key, lead, kind, body, created_at) in rows {
        if kind == "cron_problem" {
            // One bad cron problem must not block every other lead's events.
            let taken = crate::cron_remediate::handle_event(&body, &lead).await;
            if let Err(error) = &taken {
                tracing::warn!(event_id = id, %error, "cron problem handling failed; event remains queued");
            }
            match cron_disposition(taken) {
                CronDisposition::Receipt => {
                    receipt(pool, &[id], &stamp, "cron_remediation").await?;
                    continue;
                }
                CronDisposition::Keep => continue,
                CronDisposition::Continue => {}
            }
        }
        let Ok(lead_key) = SessionKey::from_storage_key(&lead) else {
            misrouted.push(id);
            continue;
        };
        let created_at = chrono::DateTime::parse_from_rfc3339(&created_at)
            .map(|at| at.with_timezone(&chrono::Utc))
            .unwrap_or(now);
        let event = QueuedEvent {
            id,
            lane_key,
            lead,
            kind,
            body,
            created_at,
        };
        let target = if owner_dm_worthy(&event.kind, &event.body) {
            event.lead.clone()
        } else {
            match super::lead::report_target(pool, &lead_key).await {
                Some(report) => report.storage_key(),
                None => event.lead.clone(),
            }
        };
        groups.entry(target).or_default().push(event);
    }
    if !misrouted.is_empty() {
        tracing::warn!(
            events = misrouted.len(),
            "lane events addressed to a lead that is not a session key were retired"
        );
        receipt(pool, &misrouted, &stamp, "misrouted").await?;
    }
    let mut admitted = 0;
    for (target, events) in groups {
        let Some(oldest) = events.iter().map(|event| event.created_at).min() else {
            continue;
        };
        if (now - oldest).num_seconds() < 30
            && !events.iter().any(|event| {
                matches!(
                    event.kind.as_str(),
                    "done_claimed" | "dead" | "kibitzer" | "cron_report"
                )
            })
        {
            continue;
        }
        let last: Option<String> = sqlx::query_scalar(
            "SELECT MAX(delivered_at) FROM lane_events WHERE lead_session_key = ? AND delivered_at IS NOT NULL AND (delivery_reason IS NULL OR delivery_reason <> 'jev_skip')",
        )
        .bind(&target)
        .fetch_one(pool)
        .await?;
        let recent = last
            .and_then(|last| chrono::DateTime::parse_from_rfc3339(&last).ok())
            .is_some_and(|last| (now - last.with_timezone(&chrono::Utc)).num_seconds() < 30);
        if recent {
            continue;
        }
        let Ok(session) = SessionKey::from_storage_key(&target) else {
            continue;
        };
        let mut collapsed =
            std::collections::BTreeMap::<(Option<String>, String), (QueuedEvent, Vec<i64>)>::new();
        for event in events {
            let entry = collapsed
                .entry((event.lane_key.clone(), event.kind.clone()))
                .or_insert_with(|| (event.clone(), Vec::new()));
            entry.1.push(event.id);
            if event.id >= entry.0.id {
                entry.0 = event;
            }
        }
        let mut selected = Vec::new();
        for (event, ids) in collapsed.into_values() {
            if !critical(&event.kind, &event.body) {
                let state = format!(
                    "{} {}: {}",
                    event.lane_key.as_deref().unwrap_or("-"),
                    event.kind,
                    event.body
                );
                match judge
                    .noul(
                        crate::judge::Purpose::EventWake,
                        &state,
                        "Does this event need the lead agent's judgment now?",
                    )
                    .await
                {
                    Ok(score) if score < 0.5 => {
                        receipt(pool, &ids, &stamp, "jev_skip").await?;
                        continue;
                    }
                    Ok(_) => {}
                    Err(error) => {
                        tracing::warn!(%error, event_id = event.id, "event wake judgment unavailable; retaining event");
                        continue;
                    }
                }
            }
            selected.push((event, ids));
        }
        selected.sort_by_key(|(event, _)| event.id);
        let ids: Vec<i64> = selected
            .iter()
            .flat_map(|(_, ids)| ids.iter().copied())
            .collect();
        if ids.is_empty() {
            continue;
        }
        let lines = selected
            .iter()
            .map(|(event, _)| {
                let body: String = event
                    .body
                    .chars()
                    .take(600)
                    .map(|ch| if ch == '\n' || ch == '\r' { ' ' } else { ch })
                    .collect();
                format!(
                    "{} {}: {}",
                    event.lane_key.as_deref().unwrap_or("-"),
                    event.kind,
                    body
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        if !admit(session, format!("[LANE_EVENT]\n{lines}")).await {
            continue;
        }
        tracing::info!(lead_session_key = %target, events = ids.len(), "lane event admitted to lead turn");
        receipt(pool, &ids, &stamp, "lead_turn").await?;
        admitted += 1;
    }
    Ok(admitted)
}

async fn receipt(pool: &SqlitePool, ids: &[i64], stamp: &str, reason: &str) -> Result<()> {
    for id in ids {
        sqlx::query(
            "UPDATE lane_events SET delivered_at = ?, delivery_reason = ? WHERE id = ? AND delivered_at IS NULL",
        )
        .bind(stamp)
        .bind(reason)
        .bind(id)
        .execute(pool)
        .await?;
    }
    Ok(())
}

/// What a `cron_problem` event does inside one tick. `Ok(true)` from the remediator is a
/// receipt; `Ok(false)` (no remediator) joins the ordinary groups; an error stays queued.
fn cron_disposition(result: Result<bool>) -> CronDisposition {
    match result {
        Ok(true) => CronDisposition::Receipt,
        Ok(false) => CronDisposition::Continue,
        Err(_) => CronDisposition::Keep,
    }
}

enum CronDisposition {
    Receipt,
    Continue,
    Keep,
}

/// omo: only urgent events wake a turn in the owner DM ("DM으로는 급한 용무만").
fn owner_dm_worthy(kind: &str, body: &str) -> bool {
    match kind {
        "inbox_urgent" | "owner_reply_failed" | "dead" | "launch_failed" | "startup_error" => true,
        "cron_problem" => body.contains("needs_owner"),
        _ => false,
    }
}

/// omo: kinds that must reach the lead without the wake judgment.
fn critical(kind: &str, body: &str) -> bool {
    match kind {
        "done_claimed" | "dead" | "not_done" | "inbox_urgent" | "owner_reply_failed"
        | "cron_report" | "unmerged_branch" | "launch_failed" | "startup_error" => true,
        "action_loop" | "no_progress" => true,
        "cron_problem" => body.contains("needs_owner"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn an_event_is_delivered_once() {
        let pool = crate::storage::memory().await.unwrap();
        enqueue(&pool, "lane-a", "lead", "note", "hello")
            .await
            .unwrap();
        let first = take_undelivered(&pool).await.unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].body, "hello");
        let second = take_undelivered(&pool).await.unwrap();
        assert!(second.is_empty());
        let later = "2999-01-01T00:00:00Z";
        sqlx::query(
            "INSERT INTO lane_events(lane_key, lead_session_key, kind, body, created_at, release_at) VALUES(NULL,'lead','action_loop','held',?,?)",
        )
        .bind(crate::storage::now_rfc3339())
        .bind(later)
        .execute(&pool)
        .await
        .unwrap();
        assert!(take_undelivered(&pool).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn lane_events_reach_the_home_channel_or_the_owner_dm_like_omo() {
        let pool = crate::storage::memory().await.unwrap();
        let dm = SessionKey::new("slack", None::<String>, "D0DM", None::<String>, "U0OWNER")
            .with_bot_id("U0BOT")
            .storage_key();
        let old = (chrono::Utc::now() - chrono::Duration::minutes(5)).to_rfc3339();
        for (lead, kind) in [
            (dm.as_str(), "action_loop"),
            (dm.as_str(), "action_loop"),
            (dm.as_str(), "inbox_urgent"),
            ("meta", "action_loop"),
        ] {
            sqlx::query("INSERT INTO lane_events(lane_key, lead_session_key, kind, body, created_at) VALUES(NULL, ?, ?, 'item', ?)")
                .bind(lead)
                .bind(kind)
                .bind(&old)
                .execute(&pool)
                .await
                .unwrap();
        }
        let judge = crate::judge::Judge::forced("no");
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(String, String)>::new()));
        let record = seen.clone();
        let admitted = dispatch_tick(&pool, &judge, chrono::Utc::now(), move |key, text| {
            record.lock().unwrap().push((key.storage_key(), text));
            async { true }
        })
        .await
        .unwrap();
        // No home channel is configured in tests, so routine events stay on the DM lead too;
        // the DM group carries both kinds, collapsed to one action_loop line.
        assert_eq!(admitted, 1);
        let seen = seen.lock().unwrap().clone();
        assert_eq!(seen[0].0, dm);
        assert!(seen[0].1.starts_with("[LANE_EVENT]\n"));
        assert_eq!(seen[0].1.matches("action_loop").count(), 1);
        assert!(seen[0].1.contains("inbox_urgent"));
        let open: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM lane_events WHERE delivered_at IS NULL")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            open, 0,
            "the misrouted meta event is retired, the rest receipted"
        );
    }

    #[tokio::test]
    async fn a_busy_lead_keeps_its_events_queued() {
        let pool = crate::storage::memory().await.unwrap();
        let dm = SessionKey::new("slack", None::<String>, "D0DM", None::<String>, "U0OWNER")
            .with_bot_id("U0BOT")
            .storage_key();
        let old = (chrono::Utc::now() - chrono::Duration::minutes(5)).to_rfc3339();
        sqlx::query("INSERT INTO lane_events(lane_key, lead_session_key, kind, body, created_at) VALUES(NULL, ?, 'action_loop', 'item', ?)")
            .bind(&dm)
            .bind(&old)
            .execute(&pool)
            .await
            .unwrap();
        let judge = crate::judge::Judge::forced("yes");
        let admitted = dispatch_tick(&pool, &judge, chrono::Utc::now(), |_, _| async { false })
            .await
            .unwrap();
        assert_eq!(admitted, 0);
        let open: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM lane_events WHERE delivered_at IS NULL")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(open, 1);
    }

    #[test]
    fn a_handled_cron_problem_is_receipted_and_a_failure_stays_queued() {
        assert!(matches!(
            cron_disposition(Ok(true)),
            CronDisposition::Receipt
        ));
        assert!(matches!(
            cron_disposition(Ok(false)),
            CronDisposition::Continue
        ));
        assert!(matches!(
            cron_disposition(Err(crate::error::HubError::message("down"))),
            CronDisposition::Keep
        ));
    }

    #[tokio::test]
    async fn events_wait_thirty_seconds_then_one_turn_and_a_low_score_is_skipped() {
        let pool = crate::storage::memory().await.unwrap();
        let dm = SessionKey::new("slack", None::<String>, "D0DM", None::<String>, "U0OWNER")
            .with_bot_id("U0BOT")
            .storage_key();
        let now = chrono::Utc::now();
        let recent = (now - chrono::Duration::seconds(10)).to_rfc3339();
        sqlx::query("INSERT INTO lane_events(lane_key, lead_session_key, kind, body, created_at) VALUES('lane-a', ?, 'question', 'old', ?)")
            .bind(&dm)
            .bind(&recent)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO lane_events(lane_key, lead_session_key, kind, body, created_at) VALUES('lane-a', ?, 'question', 'latest', ?)")
            .bind(&dm)
            .bind(&recent)
            .execute(&pool)
            .await
            .unwrap();
        let judge = crate::judge::Judge::forced_noul(0.9);
        let admitted = dispatch_tick(&pool, &judge, now, |_, _| async { true })
            .await
            .unwrap();
        assert_eq!(admitted, 0, "younger than 30s, so the group waits");
        let later = now + chrono::Duration::seconds(31);
        let seen = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let record = seen.clone();
        let admitted = dispatch_tick(&pool, &judge, later, move |_, text| {
            *record.lock().unwrap() = text;
            async { true }
        })
        .await
        .unwrap();
        assert_eq!(admitted, 1);
        let text = seen.lock().unwrap().clone();
        assert_eq!(text.lines().count(), 2);
        assert!(text.contains("latest"));
        assert!(!text.contains("old"));
        let reason: String = sqlx::query_scalar(
            "SELECT delivery_reason FROM lane_events WHERE kind = 'question' ORDER BY id DESC LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(reason, "lead_turn");

        let quiet = (later - chrono::Duration::seconds(40)).to_rfc3339();
        sqlx::query("INSERT INTO lane_events(lane_key, lead_session_key, kind, body, created_at) VALUES('lane-b', ?, 'question', 'routine', ?)")
            .bind(&dm)
            .bind(&quiet)
            .execute(&pool)
            .await
            .unwrap();
        let low = crate::judge::Judge::forced_noul(0.49);
        let admitted = dispatch_tick(
            &pool,
            &low,
            later + chrono::Duration::seconds(31),
            |_, _| async { true },
        )
        .await
        .unwrap();
        assert_eq!(admitted, 0);
        let skip: String =
            sqlx::query_scalar("SELECT delivery_reason FROM lane_events WHERE lane_key = 'lane-b'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(skip, "jev_skip");
    }

    #[tokio::test]
    async fn a_cron_problem_without_a_remediator_still_reaches_the_lead() {
        let pool = crate::storage::memory().await.unwrap();
        let dm = SessionKey::new("slack", None::<String>, "D0DM", None::<String>, "U0OWNER")
            .with_bot_id("U0BOT")
            .storage_key();
        let old = (chrono::Utc::now() - chrono::Duration::minutes(5)).to_rfc3339();
        sqlx::query("INSERT INTO lane_events(lane_key, lead_session_key, kind, body, created_at) VALUES('job', ?, 'cron_problem', ?, ?)")
            .bind(&dm)
            .bind(r#"{"problem_id":1,"kind":"needs_owner"}"#)
            .bind(&old)
            .execute(&pool)
            .await
            .unwrap();
        let judge = crate::judge::Judge::forced_noul(0.0);
        let seen = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let record = seen.clone();
        let admitted = dispatch_tick(&pool, &judge, chrono::Utc::now(), move |_, text| {
            *record.lock().unwrap() = text;
            async { true }
        })
        .await
        .unwrap();
        assert_eq!(admitted, 1);
        assert!(seen.lock().unwrap().contains("needs_owner"));
        let reason: String = sqlx::query_scalar("SELECT delivery_reason FROM lane_events")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(reason, "lead_turn");
    }
}
