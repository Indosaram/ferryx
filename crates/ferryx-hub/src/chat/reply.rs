//! Outbound chat replies. The text is what the caller supplies. Nothing here reads a pane.
use sqlx::SqlitePool;

use super::router::{Outbound, PlatformRouter};
use super::session::SessionKey;
use crate::error::Result;
use crate::storage::now_rfc3339;

pub async fn chat_reply(
    pool: &SqlitePool,
    session: &SessionKey,
    text: &str,
    platforms: Option<&PlatformRouter>,
) -> Result<i64> {
    if let Some(platforms) = platforms {
        platforms.send(session, text).await?;
    }
    let inserted = sqlx::query(
        "INSERT INTO chat_outbox(session_key, platform, text, created_at) VALUES(?,?,?,?)",
    )
    .bind(session.storage_key())
    .bind(&session.platform)
    .bind(text)
    .bind(now_rfc3339())
    .execute(pool)
    .await?;
    let id = inserted.last_insert_rowid();
    if publish(session, text).await? {
        mark_delivered(pool, id).await?;
    }
    Ok(id)
}

/// An owner notice. On Slack a detected prompt is posted as answer buttons under the text;
/// the stored outbox row is the text alone, so a later flush never replays a stale card.
pub async fn chat_reply_card(
    pool: &SqlitePool,
    session: &SessionKey,
    text: &str,
    approval: Option<&crate::sessions::watch::ApprovalCard>,
) -> Result<i64> {
    let Some(card) = approval else {
        return chat_reply(pool, session, text, None).await;
    };
    let inserted = sqlx::query(
        "INSERT INTO chat_outbox(session_key, platform, text, created_at) VALUES(?,?,?,?)",
    )
    .bind(session.storage_key())
    .bind(&session.platform)
    .bind(text)
    .bind(now_rfc3339())
    .execute(pool)
    .await?;
    let id = inserted.last_insert_rowid();
    let posted = if session.platform.eq_ignore_ascii_case("slack") {
        super::slack::publish_card(session, text, card).await?
    } else {
        publish(session, text).await?
    };
    if posted {
        mark_delivered(pool, id).await?;
    }
    Ok(id)
}

/// Where a posted message landed: the channel the platform reports and the message id
/// (Slack `ts`) when it returned one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostedMessage {
    pub channel: String,
    pub ts: Option<String>,
}

/// [`publish`] that returns where the message landed. Only Slack reports it; another
/// platform that posted reports its session channel with no ts.
async fn publish_posted(session: &SessionKey, text: &str) -> Result<Option<PostedMessage>> {
    if session.platform.eq_ignore_ascii_case("slack") {
        return super::slack::publish_posted(session, text).await;
    }
    Ok(publish(session, text).await?.then(|| PostedMessage {
        channel: session.channel_id.clone(),
        ts: None,
    }))
}

async fn publish(session: &SessionKey, text: &str) -> Result<bool> {
    if session.platform.eq_ignore_ascii_case("slack") {
        return super::slack::publish(session, text).await;
    }
    if session.platform.eq_ignore_ascii_case("discord") {
        return super::discord::publish(session, text).await;
    }
    let _ = text;
    Ok(false)
}

/// Delivers replies that were stored before a token was available. One pass, not a poll.
pub async fn flush_pending(pool: &SqlitePool) -> Result<u64> {
    let mut delivered = 0u64;
    for (id, key, text) in pending(pool).await? {
        if let Some(posted) = deliver_stored_posted(pool, id, &key, &text).await? {
            crate::report_feedback::posted_from_outbox(pool, id, &posted).await?;
            delivered += 1;
        }
    }
    Ok(delivered)
}

/// Publishes one stored outbox row now and marks it delivered when the transport took it.
/// A key that is not a chat session (a lane name) cannot be delivered and reports false.
pub async fn deliver_stored(pool: &SqlitePool, id: i64, key: &str, text: &str) -> Result<bool> {
    Ok(deliver_stored_posted(pool, id, key, text).await?.is_some())
}

/// [`deliver_stored`] that returns where the message landed, so the caller can remember the
/// posted message (an owner alert's Slack ts). `None` when no transport took it.
///
/// The row is claimed before the owner-rule recheck and the post, so overlapping senders (the
/// writer's own post, `flush_pending`, `flush_stale`, `drain_outbox`, another hub process on
/// the same database) post it at most once while a claim is live; a sender that finds the row
/// claimed reports `None` and leaves it to the claimant. An untaken post or one Slack
/// certainly did not take releases the claim so the row stays retryable.
///
/// On Slack the row posts as its rendered mrkdwn chunks ([`super::slack::outbox_chunks`]);
/// each confirmed chunk is recorded, so a retry resumes at the first unconfirmed chunk and
/// never reposts an earlier one. A chunk whose outcome is unknown (timeout, dropped
/// connection, unreadable answer), or one a crashed sender never recorded, makes the row
/// uncertain: it is never claimed again, and `report_status` reports it as `uncertain`.
pub async fn deliver_stored_posted(
    pool: &SqlitePool,
    id: i64,
    key: &str,
    _text: &str,
) -> Result<Option<PostedMessage>> {
    let Ok(session) = SessionKey::from_storage_key(key) else {
        return Ok(None);
    };
    let Some(claim) = claim_send(pool, id).await? else {
        return Ok(None);
    };
    let posted = post_claimed(pool, id, &session, &claim).await;
    match &posted {
        Ok(Some(receipt)) => {
            // Recorded even if this claim expired and was taken over: the post happened.
            sqlx::query(
                "UPDATE chat_outbox SET delivered_at = ?, posted_channel = ?, posted_ts = ?,
                    send_claim = NULL, send_claim_until = NULL
                  WHERE id = ? AND delivered_at IS NULL",
            )
            .bind(now_rfc3339()).bind(&receipt.channel).bind(&receipt.ts).bind(id)
            .execute(pool).await?;
        }
        // Nothing (more) was posted, or Slack refused it: the row goes back to the retry. An
        // uncertain post stays excluded by `send_uncertain_at`, not by this claim.
        Ok(None) | Err(_) => release_send(pool, id, &claim).await?,
    }
    posted
}

/// How long a send claim on an outbox row holds. It must outlast one post including its
/// Slack table uploads (each call times out after 15 seconds); it only matters when the
/// claimant was cancelled or crashed before recording the outcome.
pub const OUTBOX_SEND_LEASE: chrono::Duration = chrono::Duration::minutes(5);

/// Atomically claims an undelivered, uncancelled row whose previous claim is absent or
/// expired. `None` when another sender holds it or it is finished.
async fn claim_send(pool: &SqlitePool, id: i64) -> Result<Option<String>> {
    let now = chrono::Utc::now();
    let claim = uuid::Uuid::new_v4().to_string();
    let claimed = sqlx::query(
        "UPDATE chat_outbox SET send_claim = ?, send_claim_until = ?
          WHERE id = ? AND delivered_at IS NULL AND cancelled_at IS NULL
            AND send_inflight IS NULL AND send_uncertain_at IS NULL
            AND (send_claim IS NULL OR julianday(send_claim_until) <= julianday(?))",
    )
    .bind(&claim)
    .bind((now + OUTBOX_SEND_LEASE).to_rfc3339())
    .bind(id)
    .bind(now.to_rfc3339())
    .execute(pool)
    .await?
    .rows_affected();
    Ok((claimed == 1).then_some(claim))
}

/// Drops this sender's claim; a claim another sender took over after expiry is left alone.
async fn release_send(pool: &SqlitePool, id: i64, claim: &str) -> Result<()> {
    sqlx::query(
        "UPDATE chat_outbox SET send_claim = NULL, send_claim_until = NULL
          WHERE id = ? AND send_claim = ?",
    )
    .bind(id)
    .bind(claim)
    .execute(pool)
    .await?;
    Ok(())
}

/// The owner-rule recheck and the post of a row this sender holds the claim on.
async fn post_claimed(
    pool: &SqlitePool,
    id: i64,
    session: &SessionKey,
    claim: &str,
) -> Result<Option<PostedMessage>> {
    if session.platform.eq_ignore_ascii_case("slack") {
        return post_slack_chunks(pool, id, session, claim).await;
    }
    let Some(text) = crate::report_gate::prepare_delivery(pool, id).await? else { return Ok(None); };
    publish_posted(session, &text).await
}

type ChunkRow = (String, Option<String>, i64, Option<String>, Option<String>, Option<String>);

fn decode<T: serde::de::DeserializeOwned>(raw: &str) -> Result<T> {
    serde_json::from_str(raw).map_err(|error| crate::HubError::message(error.to_string()))
}

/// Posts a claimed Slack row chunk by chunk with durable progress (see [`deliver_stored_posted`]).
/// The first chunk carries the row's blocks and is the message the row's receipt names.
async fn post_slack_chunks(
    pool: &SqlitePool,
    id: i64,
    session: &SessionKey,
    claim: &str,
) -> Result<Option<PostedMessage>> {
    use super::slack::SlackSend;
    let (_, stored, done, blocks, channel, ts): ChunkRow = sqlx::query_as(
        "SELECT text, send_chunks_json, send_chunks_done, blocks_json, posted_channel, posted_ts
           FROM chat_outbox WHERE id = ?",
    )
    .bind(id).fetch_one(pool).await?;
    let blocks: Option<serde_json::Value> = blocks.as_deref().map(decode).transpose()?;
    let chunks: Vec<String> = match stored.filter(|_| done > 0) {
        // Part of the row is already in Slack: finish exactly what was started.
        Some(stored) => decode(&stored)?,
        None => {
            let Some(text) = crate::report_gate::prepare_delivery(pool, id).await? else {
                return Ok(None);
            };
            let mut chunks = if blocks.is_some() {
                vec![text.clone()]
            } else {
                super::slack::outbox_chunks(&text)
            };
            if chunks.is_empty() {
                chunks.push(text);
            }
            let encoded = serde_json::to_string(&chunks)
                .map_err(|error| crate::HubError::message(error.to_string()))?;
            sqlx::query("UPDATE chat_outbox SET send_chunks_json = ?, send_chunks_done = 0
                WHERE id = ? AND send_claim = ?")
                .bind(encoded).bind(id).bind(claim).execute(pool).await?;
            chunks
        }
    };
    let mut first = (done > 0).then(|| PostedMessage {
        channel: channel.unwrap_or_else(|| session.channel_id.clone()),
        ts,
    });
    for (index, chunk) in chunks.iter().enumerate().skip(usize::try_from(done).unwrap_or(0)) {
        use crate::report_gate::ParentPolicy;
        match crate::report_gate::check_parent_policy(pool, id).await? {
            ParentPolicy::Muted { .. } | ParentPolicy::Cancelled => return Ok(None),
            ParentPolicy::Deferred { .. } => {
                let due: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM report_digest_deliveries WHERE outbox_id=?) OR COALESCE(julianday(report_not_before)<=julianday(?),0) FROM chat_outbox WHERE id=?")
                    .bind(id).bind(now_rfc3339()).bind(id).fetch_one(pool).await?;
                if !due { return Ok(None); }
            }
            ParentPolicy::Allowed | ParentPolicy::Ungated => {}
        }
        let marked = sqlx::query(
            "UPDATE chat_outbox SET send_inflight = ?, send_claim_until = ?
              WHERE id = ? AND send_claim = ? AND send_inflight IS NULL AND send_uncertain_at IS NULL",
        )
        .bind(index as i64)
        .bind((chrono::Utc::now() + OUTBOX_SEND_LEASE).to_rfc3339())
        .bind(id).bind(claim)
        .execute(pool).await?.rows_affected();
        if marked != 1 {
            return Err(crate::HubError::message("outbox send claim was lost before posting"));
        }
        let card = if index == 0 { blocks.clone() } else { None };
        match super::slack::post_chunk(session, chunk, card).await {
            SlackSend::Posted(receipt) => {
                // Recorded whatever happened to the claim: the chunk is in Slack.
                if index == 0 {
                    sqlx::query("UPDATE chat_outbox SET posted_channel = ?, posted_ts = ? WHERE id = ?")
                        .bind(&receipt.channel).bind(&receipt.ts).bind(id).execute(pool).await?;
                    first = Some(receipt);
                }
                sqlx::query("UPDATE chat_outbox SET send_chunks_done = ?, send_inflight = NULL WHERE id = ?")
                    .bind(index as i64 + 1).bind(id).execute(pool).await?;
            }
            SlackSend::NotConfigured => {
                clear_inflight(pool, id).await?;
                return Ok(None);
            }
            SlackSend::Rejected(error) => {
                clear_inflight(pool, id).await?;
                return Err(error);
            }
            SlackSend::Uncertain(error) => {
                sqlx::query("UPDATE chat_outbox SET send_uncertain_at = ?, send_uncertain_error = ? WHERE id = ?")
                    .bind(now_rfc3339()).bind(error.to_string()).bind(id).execute(pool).await?;
                return Err(error);
            }
        }
    }
    let posted = first.unwrap_or_else(|| PostedMessage { channel: session.channel_id.clone(), ts: None });
    let recorded = sqlx::query(
        "UPDATE chat_outbox SET delivered_at = ?, posted_channel = ?, posted_ts = ?,
            send_claim = NULL, send_claim_until = NULL
          WHERE id = ? AND delivered_at IS NULL",
    )
    .bind(now_rfc3339()).bind(&posted.channel).bind(&posted.ts).bind(id)
    .execute(pool).await?.rows_affected();
    if recorded == 1 {
        let text: String = sqlx::query_scalar("SELECT text FROM chat_outbox WHERE id = ?")
            .bind(id).fetch_one(pool).await?;
        super::slack::finish_outbox_post(session, &text, blocks.is_none()).await;
    }
    Ok(Some(posted))
}

async fn clear_inflight(pool: &SqlitePool, id: i64) -> Result<()> {
    sqlx::query("UPDATE chat_outbox SET send_inflight = NULL WHERE id = ?")
        .bind(id).execute(pool).await?;
    Ok(())
}

/// Retry pacing for an undelivered row. The send claim, not this delay, excludes other senders.
pub const OUTBOX_SETTLE: chrono::Duration = chrono::Duration::minutes(2);

/// The periodic retry of the outbox. Only rows older than `settle` are tried, and only rows younger than
/// `max_age` for ordinary chat. Reports admitted by the owner gate stay retryable until
/// delivered or explicitly cancelled; their age alone must not silently lose an obligation.
/// One failure does not stop the pass.
pub async fn flush_stale(
    pool: &SqlitePool,
    settle: chrono::Duration,
    max_age: chrono::Duration,
) -> Result<u64> {
    let now = chrono::Utc::now();
    let rows: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT id, session_key, text FROM chat_outbox
          WHERE delivered_at IS NULL AND cancelled_at IS NULL
            AND julianday(COALESCE(report_not_before, created_at)) <= julianday(?)
            AND (julianday(COALESCE(report_not_before, created_at)) >= julianday(?)
                 OR EXISTS (SELECT 1 FROM report_delivery_parts p WHERE p.outbox_id = chat_outbox.id))
          ORDER BY id",
    )
    .bind((now - settle).to_rfc3339())
    .bind((now - max_age).to_rfc3339())
    .fetch_all(pool)
    .await?;
    let mut delivered = 0u64;
    for (id, key, text) in rows {
        match deliver_stored_posted(pool, id, &key, &text).await {
            Ok(Some(posted)) => {
                delivered += 1;
                // A retried owner alert is remembered by its posted message, as on first post.
                if let Err(error) =
                    crate::report_feedback::posted_from_outbox(pool, id, &posted).await
                {
                    tracing::warn!(id, %error, "retried owner alert was not remembered");
                }
            }
            Ok(None) => {}
            Err(error) => tracing::warn!(id, %error, "stored chat reply was not delivered"),
        }
    }
    Ok(delivered)
}

pub async fn pending(pool: &SqlitePool) -> Result<Vec<(i64, String, String)>> {
    let rows: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT id, session_key, text FROM chat_outbox WHERE delivered_at IS NULL AND cancelled_at IS NULL ORDER BY id",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn mark_delivered(pool: &SqlitePool, id: i64) -> Result<()> {
    sqlx::query("UPDATE chat_outbox SET delivered_at = ? WHERE id = ?")
        .bind(now_rfc3339())
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Records a reply without a platform transport. Used by `chat.reply` when Slack is not connected.
pub struct OutboxOutbound {
    pub pool: SqlitePool,
}

#[async_trait::async_trait]
impl Outbound for OutboxOutbound {
    async fn send(&self, _session: &SessionKey, _text: &str) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stored_delivery_obeys_a_mute_received_after_admission() {
        let pool = crate::storage::memory().await.unwrap();
        let session = SessionKey::new("slack", None::<String>, "C1", None::<String>, "U1");
        let report = crate::report_gate::Report::new("mail", "mail", "inbox:mail",
            crate::report_gate::Salience::Action, "확인이 필요한 메일입니다.");
        let id = crate::report_gate::queue(&pool, &session.storage_key(), &report, false)
            .await.unwrap().1.unwrap();
        let scope = crate::report_rules::scope_from_alert("mail", None, None, Some("inbox:mail"));
        crate::report_rules::record(&pool, &scope, crate::report_rules::Effect::Mute, "stop", None).await.unwrap();
        assert!(deliver_stored_posted(&pool, id, &session.storage_key(), &report.body).await.unwrap().is_none());
        let cancelled: bool = sqlx::query_scalar("SELECT cancelled_at IS NOT NULL AND delivered_at IS NULL FROM chat_outbox WHERE id = ?")
            .bind(id).fetch_one(&pool).await.unwrap();
        assert!(cancelled);
        assert!(pending(&pool).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_reply_is_stored_and_does_not_need_a_screen() {
        let pool = crate::storage::memory().await.unwrap();
        let session = SessionKey::new("slack", None::<String>, "C1", None::<String>, "U1");
        let id = chat_reply(&pool, &session, "shipped", None).await.unwrap();
        let queued = pending(&pool).await.unwrap();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].0, id);
        assert_eq!(queued[0].2, "shipped");
        mark_delivered(&pool, id).await.unwrap();
        assert!(pending(&pool).await.unwrap().is_empty());
    }

    /// A stand-in Slack Web API whose `chat.postMessage` reports its arrival, then waits for
    /// the test to release it, and answers ok or a failure.
    #[cfg(feature = "hub-slack")]
    #[derive(Clone)]
    struct HeldSlack {
        posts: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        arrived: tokio::sync::mpsc::UnboundedSender<()>,
        release: std::sync::Arc<tokio::sync::Semaphore>,
        fail: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    #[cfg(feature = "hub-slack")]
    async fn held_slack_post(
        axum::extract::State(slack): axum::extract::State<HeldSlack>,
        axum::extract::Path(method): axum::extract::Path<String>,
    ) -> axum::Json<serde_json::Value> {
        if method != "chat.postMessage" {
            return axum::Json(serde_json::json!({"ok": true}));
        }
        slack.posts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let _ = slack.arrived.send(());
        slack.release.acquire().await.unwrap().forget();
        if slack.fail.load(std::sync::atomic::Ordering::SeqCst) {
            return axum::Json(serde_json::json!({"ok": false, "error": "ratelimited"}));
        }
        axum::Json(serde_json::json!({"ok": true, "channel": "C1", "ts": "1.1"}))
    }

    #[cfg(feature = "hub-slack")]
    async fn held_slack() -> (String, HeldSlack, tokio::sync::mpsc::UnboundedReceiver<()>) {
        let (arrived, arrivals) = tokio::sync::mpsc::unbounded_channel();
        let slack = HeldSlack {
            posts: Default::default(),
            arrived,
            release: std::sync::Arc::new(tokio::sync::Semaphore::new(0)),
            fail: Default::default(),
        };
        let app = axum::Router::new()
            .route("/{method}", axum::routing::post(held_slack_post))
            .with_state(slack.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (base, slack, arrivals)
    }

    #[cfg(feature = "hub-slack")]
    async fn stored_row(pool: &SqlitePool, key: &str) -> i64 {
        sqlx::query("INSERT INTO chat_outbox(session_key, platform, text, created_at) VALUES(?, 'slack', 'notice', ?)")
            .bind(key).bind(now_rfc3339()).execute(pool).await.unwrap().last_insert_rowid()
    }

    #[cfg(feature = "hub-slack")]
    const HELD_SLACK_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

    #[tokio::test]
    #[cfg(feature = "hub-slack")]
    async fn overlapping_deliveries_of_one_row_post_it_once() {
        let pool = crate::storage::memory().await.unwrap();
        let key = SessionKey::new("slack", None::<String>, "C1", None::<String>, "U1").storage_key();
        let id = stored_row(&pool, &key).await;
        let (base, slack, mut arrivals) = held_slack().await;
        let first = tokio::spawn(super::super::slack::with_test_api(base.clone(), "xoxb-test".into(), {
            let (pool, key) = (pool.clone(), key.clone());
            async move { deliver_stored_posted(&pool, id, &key, "notice").await }
        }));
        tokio::time::timeout(HELD_SLACK_WAIT, arrivals.recv()).await
            .expect("the first post reached Slack").unwrap();
        // The first post is in flight: a retry pass and a direct post both leave the row alone.
        let overlapping = tokio::time::timeout(HELD_SLACK_WAIT, super::super::slack::with_test_api(base, "xoxb-test".into(), async {
            (deliver_stored_posted(&pool, id, &key, "notice").await.unwrap(),
             flush_pending(&pool).await.unwrap())
        })).await.expect("overlapping senders return without a second HTTP post");
        assert_eq!(overlapping, (None, 0));
        slack.release.add_permits(1);
        let posted = tokio::time::timeout(HELD_SLACK_WAIT, first).await
            .expect("the first post finished").unwrap().unwrap();
        assert_eq!(posted, Some(PostedMessage { channel: "C1".into(), ts: Some("1.1".into()) }));
        assert_eq!(slack.posts.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(pending(&pool).await.unwrap().is_empty());
    }

    #[tokio::test]
    #[cfg(feature = "hub-slack")]
    async fn a_failed_post_releases_the_row_for_retry() {
        let pool = crate::storage::memory().await.unwrap();
        let key = SessionKey::new("slack", None::<String>, "C1", None::<String>, "U1").storage_key();
        let id = stored_row(&pool, &key).await;
        let (base, slack, _arrivals) = held_slack().await;
        slack.fail.store(true, std::sync::atomic::Ordering::SeqCst);
        slack.release.add_permits(2);
        super::super::slack::with_test_api(base, "xoxb-test".into(), async {
            assert!(deliver_stored_posted(&pool, id, &key, "notice").await.is_err());
            let claimed: bool = sqlx::query_scalar("SELECT send_claim IS NOT NULL FROM chat_outbox WHERE id = ?")
                .bind(id).fetch_one(&pool).await.unwrap();
            assert!(!claimed);
            assert_eq!(pending(&pool).await.unwrap().len(), 1);
            slack.fail.store(false, std::sync::atomic::Ordering::SeqCst);
            assert!(deliver_stored_posted(&pool, id, &key, "notice").await.unwrap().is_some());
        }).await;
        assert_eq!(slack.posts.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert!(pending(&pool).await.unwrap().is_empty());
    }

    #[tokio::test]
    #[cfg(feature = "hub-slack")]
    async fn http_delivery_receipt_binds_owner_feedback_to_new_inbox_items() {
        use crate::owner_alert::{alert_report, PendingAlert};
        use crate::report_gate::{queue, Decision};
        for source in ["mail", "kakao", "imessage"] {
            let pool = crate::storage::memory().await.unwrap();
            let key = SessionKey::new("slack", None::<String>, "C1", None::<String>, "U1").storage_key();
            let alert = PendingAlert { item_id: "first".into(), source: source.into(),
                author: Some("Service <notify@example.com>".into()), title: Some("Your code 483920 for sign-in".into()),
                snippet: "Your code 483920 for sign-in".into(), link: None };
            let report = alert_report(&alert);
            let (decision, id) = queue(&pool, &key, &report, false).await.unwrap();
            assert_eq!(decision, Decision::Sent);
            let (base, slack, _arrivals) = held_slack().await;
            slack.release.add_permits(1);
            let posted = tokio::time::timeout(HELD_SLACK_WAIT,
                super::super::slack::with_test_api(base, "xoxb-test".into(),
                    deliver_stored_posted(&pool, id.unwrap(), &key, &report.body)))
                .await.unwrap().unwrap().unwrap();
            crate::report_feedback::posted_from_outbox(&pool, id.unwrap(), &posted).await.unwrap();
            assert_eq!(slack.posts.load(std::sync::atomic::Ordering::SeqCst), 1);
            crate::report_feedback::handle_owner_message(&pool, &crate::judge::Judge::forced("mute_kind"),
                &posted.channel, posted.ts.as_deref(), "이거 그만 보내").await.unwrap().unwrap();
            let next = PendingAlert { item_id: "second".into(), title: Some("Your code 791362 for sign-in".into()), ..alert.clone() };
            assert_eq!(queue(&pool, &key, &alert_report(&next), false).await.unwrap().0, Decision::Muted);
            let unrelated = PendingAlert { item_id: "payment".into(), title: Some("Payment approval required".into()), ..alert };
            assert_eq!(queue(&pool, &key, &alert_report(&unrelated), false).await.unwrap().0, Decision::Sent);
            crate::report_feedback::handle_owner_message(&pool, &crate::judge::Judge::forced("send_kind"),
                &posted.channel, posted.ts.as_deref(), "이건 다시 보내").await.unwrap().unwrap();
            assert_eq!(queue(&pool, &key, &alert_report(&next), false).await.unwrap().0, Decision::Sent);
        }
    }

    /// A gated owner answer (the shape `slack_turn` queues) long and formatted enough to need
    /// several Slack messages.
    #[cfg(feature = "hub-slack")]
    async fn queued_answer(pool: &SqlitePool, key: &str) -> (i64, String) {
        let lines: Vec<String> = (0..400).map(|i| format!("{i}번째 항목을 확인했습니다.")).collect();
        let body = format!("**완료** 자세한 내용은 [문서](https://x.y)를 보세요.\n{}", lines.join("\n"));
        let mut report = crate::report_gate::Report::new("answer", "slack_turn", "turn:answer",
            crate::report_gate::Salience::Result, &body);
        report.reply_to_owner = true;
        let (decision, id) = crate::report_gate::queue(pool, key, &report, false).await.unwrap();
        assert_eq!(decision, crate::report_gate::Decision::Sent);
        (id.unwrap(), body)
    }

    #[cfg(feature = "hub-slack")]
    fn chunk_posts(calls: &super::super::slack_web::mock::Calls) -> Vec<serde_json::Value> {
        calls.lock().unwrap().iter()
            .filter(|(method, _)| method == "chat.postMessage")
            .map(|(_, body)| body.clone()).collect()
    }

    #[cfg(feature = "hub-slack")]
    fn ok_post(ts: &str) -> serde_json::Value {
        serde_json::json!({"ok": true, "channel": "C1", "ts": ts})
    }

    #[tokio::test]
    #[cfg(feature = "hub-slack")]
    async fn a_gated_answer_posts_as_rendered_mrkdwn_chunks() {
        let pool = crate::storage::memory().await.unwrap();
        let key = SessionKey::new("slack", None::<String>, "C1", Some("100.1"), "owner").storage_key();
        let (id, body) = queued_answer(&pool, &key).await;
        let expected = super::super::mrkdwn::render(&body);
        assert!(expected.len() > 1, "the fixture must need several messages");
        let (base, calls) = super::super::slack_web::mock::serve("B1").await;
        let posted = super::super::slack::with_test_api(base, "xoxb-test".into(),
            deliver_stored_posted(&pool, id, &key, &body)).await.unwrap().unwrap();
        let posts = chunk_posts(&calls);
        let texts: Vec<&str> = posts.iter().map(|post| post["text"].as_str().unwrap()).collect();
        assert_eq!(texts, expected);
        assert!(texts[0].starts_with("*완료* 자세한 내용은 <https://x.y|문서>"), "{}", texts[0]);
        assert!(texts.iter().all(|text| text.len() <= super::super::mrkdwn::MESSAGE_LIMIT));
        assert!(posts.iter().all(|post| post["channel"] == "C1" && post["thread_ts"] == "100.1"));
        // The receipt names the first message, the one an owner reply or reaction points at.
        assert_eq!(posted.ts.as_deref(), Some("1700000000.000001"));
        let status = crate::control::report_status_reply(&pool, id).await;
        assert_eq!(status["delivery"], "delivered");
        assert_eq!(status["ts"], "1700000000.000001");
        assert_eq!(status["chunks_posted"], expected.len());
        assert_eq!(status["chunks_total"], expected.len());
    }

    #[tokio::test]
    #[cfg(feature = "hub-slack")]
    async fn a_rejected_later_chunk_resumes_without_reposting_earlier_chunks() {
        let pool = crate::storage::memory().await.unwrap();
        let key = SessionKey::new("slack", None::<String>, "C1", None::<String>, "owner").storage_key();
        let (id, body) = queued_answer(&pool, &key).await;
        let expected = super::super::mrkdwn::render(&body);
        assert!(expected.len() >= 3);
        let (base, calls) = super::super::slack_web::mock::serve_scripted("B1", vec![("chat.postMessage", vec![
            ok_post("1.1"), ok_post("1.2"),
            serde_json::json!({"ok": false, "error": "ratelimited"}),
            ok_post("1.3"),
        ])]).await;
        super::super::slack::with_test_api(base, "xoxb-test".into(), async {
            // Slack refused the third chunk: certainly not posted, so the row stays retryable.
            assert!(deliver_stored_posted(&pool, id, &key, &body).await.is_err());
            let status = crate::control::report_status_reply(&pool, id).await;
            assert_eq!(status["delivery"], "pending");
            assert_eq!(status["chunks_posted"], 2);
            assert_eq!(status["ts"], "1.1");
            let retried = deliver_stored_posted(&pool, id, &key, &body).await.unwrap().unwrap();
            assert_eq!(retried, PostedMessage { channel: "C1".into(), ts: Some("1.1".into()) });
        }).await;
        let texts: Vec<String> = chunk_posts(&calls).iter()
            .map(|post| post["text"].as_str().unwrap().to_owned()).collect();
        // Chunks 0 and 1 once, chunk 2 twice (refused, then posted), the rest once.
        let mut want = expected[..3].to_vec();
        want.extend_from_slice(&expected[2..]);
        assert_eq!(texts, want);
        let status = crate::control::report_status_reply(&pool, id).await;
        assert_eq!(status["delivery"], "delivered");
        assert_eq!(status["chunks_posted"], expected.len());
        assert!(pending(&pool).await.unwrap().is_empty());
    }

    #[tokio::test]
    #[cfg(feature = "hub-slack")]
    async fn a_mute_after_partial_delivery_blocks_remaining_chunks() {
        let pool = crate::storage::memory().await.unwrap();
        let key = SessionKey::new("slack", None::<String>, "C1", None::<String>, "owner").storage_key();
        let (id, body) = queued_answer(&pool, &key).await;
        let (base, calls) = super::super::slack_web::mock::serve_scripted("B1", vec![("chat.postMessage", vec![
            ok_post("1.1"), serde_json::json!({"ok":false,"error":"ratelimited"}),
        ])]).await;
        super::super::slack::with_test_api(base, "xoxb-test".into(), async {
            assert!(deliver_stored_posted(&pool, id, &key, &body).await.is_err());
            let scope = crate::report_rules::scope_from_alert("slack_turn", None, None, Some("turn:answer"));
            crate::report_rules::record(&pool, &scope, crate::report_rules::Effect::Mute, "stop", None).await.unwrap();
            assert!(deliver_stored_posted(&pool, id, &key, &body).await.unwrap().is_none());
        }).await;
        assert_eq!(chunk_posts(&calls).len(), 2);
    }

    #[tokio::test]
    #[cfg(feature = "hub-slack")]
    async fn an_ambiguous_post_is_held_uncertain_and_never_retried() {
        for ambiguous in [serde_json::json!({"ok": false, "error": "internal_error"}),
            serde_json::json!({"unexpected": true})] {
            let pool = crate::storage::memory().await.unwrap();
            let key = SessionKey::new("slack", None::<String>, "C1", None::<String>, "owner").storage_key();
            let (id, body) = queued_answer(&pool, &key).await;
            let (base, calls) = super::super::slack_web::mock::serve_scripted("B1", vec![
                ("chat.postMessage", vec![ok_post("1.1"), ambiguous, ok_post("1.2")])]).await;
            super::super::slack::with_test_api(base, "xoxb-test".into(), async {
                assert!(deliver_stored_posted(&pool, id, &key, &body).await.is_err());
                // Every automatic sender leaves the uncertain row alone.
                assert_eq!(flush_pending(&pool).await.unwrap(), 0);
                assert_eq!(flush_stale(&pool, chrono::Duration::zero(), chrono::Duration::days(1)).await.unwrap(), 0);
                assert!(deliver_stored_posted(&pool, id, &key, &body).await.unwrap().is_none());
            }).await;
            assert_eq!(chunk_posts(&calls).len(), 2, "no chunk was posted again");
            let status = crate::control::report_status_reply(&pool, id).await;
            assert_eq!(status["delivery"], "uncertain");
            assert_eq!(status["chunks_posted"], 1);
            assert_eq!(status["ts"], "1.1");
            assert!(status["uncertain_since"].is_string());
        }
    }

    #[tokio::test]
    #[cfg(feature = "hub-slack")]
    async fn a_refused_connection_never_reached_slack_and_stays_retryable() {
        let pool = crate::storage::memory().await.unwrap();
        let key = SessionKey::new("slack", None::<String>, "C1", None::<String>, "owner").storage_key();
        let id = stored_row(&pool, &key).await;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        drop(listener);
        super::super::slack::with_test_api(base, "xoxb-test".into(), async {
            assert!(deliver_stored_posted(&pool, id, &key, "notice").await.is_err());
        }).await;
        let status = crate::control::report_status_reply(&pool, id).await;
        assert_eq!(status["delivery"], "pending");
        let (claimed, inflight): (Option<String>, Option<i64>) = sqlx::query_as(
            "SELECT send_claim, send_inflight FROM chat_outbox WHERE id = ?")
            .bind(id).fetch_one(&pool).await.unwrap();
        assert_eq!((claimed, inflight), (None, None));
    }

    #[tokio::test]
    async fn a_sender_that_died_mid_post_leaves_the_row_uncertain() {
        let pool = crate::storage::memory().await.unwrap();
        let key = SessionKey::new("slack", None::<String>, "C1", None::<String>, "owner").storage_key();
        let id = sqlx::query("INSERT INTO chat_outbox(session_key, platform, text, created_at) VALUES(?, 'slack', 'notice', ?)")
            .bind(&key).bind(now_rfc3339()).execute(&pool).await.unwrap().last_insert_rowid();
        let expired = (chrono::Utc::now() - chrono::Duration::minutes(1)).to_rfc3339();
        sqlx::query("UPDATE chat_outbox SET send_claim = 'gone', send_claim_until = ?, send_inflight = 0 WHERE id = ?")
            .bind(expired).bind(id).execute(&pool).await.unwrap();
        // The lease expired, but the chunk it was posting has no recorded outcome.
        assert!(deliver_stored_posted(&pool, id, &key, "notice").await.unwrap().is_none());
        assert_eq!(crate::control::report_status_reply(&pool, id).await["delivery"], "uncertain");
    }
}
