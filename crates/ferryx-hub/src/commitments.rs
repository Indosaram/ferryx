//! Commitment ledger and overdue follow-up sweeper (docs/design/hub-owner-reporting.md section 5).
//!
//! Every bot message in an owner conversation passes through [`detect`]. A promise becomes an
//! `open` row; a later bot message in the same thread that delivers it closes the row
//! ([`fulfil`]). [`sweep`] gives each overdue row exactly one follow-up, in the owner's language,
//! in its thread and, when the row belongs to a lane, one `lane_events` wake so the work resumes.
use std::sync::OnceLock;

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use regex::Regex;
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

use crate::error::Result;
use crate::judge::{scrub_for_external, Judge, Purpose};
use crate::storage::WriteTx;

/// Due time of a promise that names none.
pub const DEFAULT_DUE_HOURS: i64 = 2;
/// Lane event kind of an overdue commitment wake.
pub const OVERDUE_EVENT_KIND: &str = "commitment_overdue";
/// Lead session of a wake whose lane row is missing.
const FALLBACK_LEAD: &str = "lead";
/// Longest stored `what`.
const MAX_WHAT_CHARS: usize = 200;
/// Most open commitments one bot message is checked against for delivery.
pub const MAX_CANDIDATES: i64 = 8;

const DETECT_INSTRUCTIONS: &str = "The state is one message the assistant just sent to its owner. \
Answer \"promise\" only when the message commits the assistant to do or report something later \
(for example: will check and let you know, will report back, will share after organizing). \
Answer \"none\" for a finished answer, a question, or a statement of what was already done.";

const DELIVERS_INSTRUCTIONS: &str = "The state has an earlier promise the assistant made and a newer \
message the assistant sent in the same thread. Answer \"delivers\" only when the newer message \
actually delivers what was promised (the result, the report, the answer). Answer \"cannot\" only \
when the newer message says the promised work can no longer be done (no access, the job is gone, \
the owner called it off). Answer \"not_yet\" when it is progress, another promise, or about \
something else.";

/// What a newer bot message does to an earlier promise ([`delivers`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    Delivers,
    NotYet,
    /// The message itself tells the owner the promise can no longer be kept.
    Cannot,
}

/// A promise found in a bot message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detected {
    pub what: String,
    /// The due time the text names, if any. [`default_due`] applies when it is `None`.
    pub due: Option<DateTime<Utc>>,
}

/// One overdue commitment's follow-up, to be posted as a reply in its thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FollowUp {
    pub commitment_id: String,
    pub thread_ref: String,
    /// The follow-up text in the owner's language (the name predates localization).
    pub body_ko: String,
    pub lane_key: Option<String>,
    pub outbox_id: Option<i64>,
}

/// An undone commitment a new bot message may deliver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenCommitment {
    pub id: String,
    pub thread_ref: String,
    pub what: String,
}

/// What [`track`] did with one bot message.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tracked {
    /// Ids of the commitments this message delivered and closed.
    pub fulfilled: Vec<String>,
    /// Ids of the commitments this message said can no longer be kept, closed `abandoned`.
    /// The message already told the owner, so no further line is posted.
    pub abandoned: Vec<String>,
    /// Id of the commitment this message opened, if it promises later work.
    pub recorded: Option<String>,
}

fn marker_regex() -> &'static Regex {
    static MARKER: OnceLock<Regex> = OnceLock::new();
    MARKER.get_or_init(|| {
        Regex::new(
            r"(?i)(알려드리|확인해서|확인 후|다시 말씀|보고드리|공유드리|정리해서|(할|드릴|볼)게요|(하|드리|보)겠습니다|끝나면|나오면|will\s+(let\s+you\s+know|report|check)|I['’]ll\s|before\s+reporting|report\s+back)",
        )
        .expect("static promise marker regex")
    })
}

fn relative_due_regex() -> &'static Regex {
    static DUE: OnceLock<Regex> = OnceLock::new();
    DUE.get_or_init(|| {
        Regex::new(
            r"(?i)(?:(\d{1,3})\s*(분|시간)\s*(?:안에|내에|이내|후에|뒤에|안으로|내로)|(?:within|in)\s+(\d{1,3})\s*(minutes?|mins?|hours?|hrs?)\b)",
        )
        .expect("static relative due regex")
    })
}

/// The deterministic pre-check: `false` means the text cannot be a promise.
pub fn has_promise_marker(text: &str) -> bool {
    marker_regex().is_match(text)
}

/// Due time of a promise sent at `message_at` that names none.
pub fn default_due(message_at: DateTime<Utc>) -> DateTime<Utc> {
    message_at + Duration::hours(DEFAULT_DUE_HOURS)
}

/// A relative deadline ("30분 안에", "in 3 hours") in the promise sentence, from
/// `message_at`. A bare duration ("30분마다", "26분 동안") is not a deadline.
fn parse_due(sentence: &str, message_at: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let captures = relative_due_regex().captures(sentence)?;
    let amount = captures.get(1).or_else(|| captures.get(3))?;
    let unit = captures.get(2).or_else(|| captures.get(4))?;
    let amount: i64 = amount.as_str().parse().ok()?;
    let unit = unit.as_str().to_lowercase();
    let delta = if unit == "분" || unit.starts_with("min") {
        Duration::minutes(amount)
    } else {
        Duration::hours(amount)
    };
    (amount > 0).then_some(message_at + delta)
}

/// The sentence that carries the promise marker, trimmed to [`MAX_WHAT_CHARS`].
fn promise_sentence(text: &str) -> String {
    let sentence = text
        .split(['\n', '.', '!', '?', '。'])
        .map(str::trim)
        .find(|part| has_promise_marker(part))
        .unwrap_or_else(|| text.trim());
    sentence.chars().take(MAX_WHAT_CHARS).collect()
}

/// Whether `text`, a bot message sent now, promises later work.
pub async fn detect(judge: &Judge, text: &str) -> Result<Option<Detected>> {
    detect_at(judge, text, Utc::now()).await
}

/// [`detect`] for a message sent at `message_at`, which anchors a relative due time.
pub async fn detect_at(
    judge: &Judge,
    text: &str,
    message_at: DateTime<Utc>,
) -> Result<Option<Detected>> {
    if !has_promise_marker(text) {
        return Ok(None);
    }
    let state = scrub_for_external(text);
    let choice = judge
        .choice(
            Purpose::Commitment,
            &state,
            DETECT_INSTRUCTIONS,
            &["promise", "none"],
        )
        .await?;
    if choice.as_str() != "promise" {
        return Ok(None);
    }
    let what = promise_sentence(text);
    let due = parse_due(&what, message_at);
    Ok(Some(Detected { what, due }))
}

/// What `text`, a later bot message, does to the promise `what`.
pub async fn delivers(judge: &Judge, what: &str, text: &str) -> Result<Delivery> {
    let state = format!(
        "promise:\n{}\n\nnewer message:\n{}",
        scrub_for_external(what),
        scrub_for_external(text)
    );
    let choice = judge
        .choice(
            Purpose::Commitment,
            &state,
            DELIVERS_INSTRUCTIONS,
            &["delivers", "not_yet", "cannot"],
        )
        .await?;
    Ok(match choice.as_str() {
        "delivers" => Delivery::Delivers,
        "cannot" => Delivery::Cannot,
        _ => Delivery::NotYet,
    })
}

fn stamp(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// Records a promise as an `open` row. Idempotent on `message_ref`: a replayed message keeps
/// the first row. Returns the row id.
pub async fn record(
    pool: &SqlitePool,
    thread_ref: &str,
    message_ref: &str,
    what: &str,
    due_at: DateTime<Utc>,
    lane_key: Option<&str>,
) -> Result<String> {
    sqlx::query(
        "INSERT INTO commitments(id, thread_ref, message_ref, what, due_at, state, lane_key, created_at)
         VALUES(?,?,?,?,?,'open',?,?)
         ON CONFLICT(message_ref) DO NOTHING",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(thread_ref)
    .bind(message_ref)
    .bind(what)
    .bind(stamp(due_at))
    .bind(lane_key)
    .bind(stamp(Utc::now()))
    .execute(pool)
    .await?;
    let id: String = sqlx::query_scalar("SELECT id FROM commitments WHERE message_ref = ?")
        .bind(message_ref)
        .fetch_one(pool)
        .await?;
    Ok(id)
}

/// The undone commitments (`open` or `followed_up`) a new bot message in `thread_ref` may
/// deliver: those of the same thread first, then those of other threads in the same owner
/// conversation `channel`, each newest first, at most [`MAX_CANDIDATES`]. The message's own
/// commitment (`exclude_message_ref`) is never a candidate. Thread refs are `channel` or
/// `channel:thread_ts`.
pub async fn candidates(
    pool: &SqlitePool,
    channel: &str,
    thread_ref: &str,
    exclude_message_ref: &str,
) -> Result<Vec<OpenCommitment>> {
    let rows = sqlx::query(
        "SELECT id, thread_ref, what FROM commitments
         WHERE state IN ('open','followed_up') AND message_ref <> ?
           AND (thread_ref = ? OR thread_ref = ? OR substr(thread_ref, 1, length(?) + 1) = ? || ':')
         ORDER BY (thread_ref = ?) DESC, created_at DESC, rowid DESC
         LIMIT ?",
    )
    .bind(exclude_message_ref)
    .bind(thread_ref)
    .bind(channel)
    .bind(channel)
    .bind(channel)
    .bind(thread_ref)
    .bind(MAX_CANDIDATES)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| OpenCommitment {
            id: row.get("id"),
            thread_ref: row.get("thread_ref"),
            what: row.get("what"),
        })
        .collect())
}

/// Closes commitment `id` as delivered by `by_message_ref`. Returns whether this call closed
/// it (`false` when it was already closed).
pub async fn fulfil_id(pool: &SqlitePool, id: &str, by_message_ref: &str) -> Result<bool> {
    let closed = sqlx::query(
        "UPDATE commitments SET state = 'fulfilled', fulfilled_by = ?
         WHERE id = ? AND state IN ('open','followed_up') AND message_ref <> ?",
    )
    .bind(by_message_ref)
    .bind(id)
    .bind(by_message_ref)
    .execute(pool)
    .await?;
    Ok(closed.rows_affected() == 1)
}

/// The commitment ledger on one bot message in an owner conversation, sent at `at`: every
/// [`candidates`] row the judge says the message delivers is closed (one message may deliver
/// several), every row it says can no longer be kept is closed `abandoned`, then the message
/// itself is checked for a new promise and recorded. A promise made in a lane's Slack thread
/// belongs to that lane, so its overdue sweep wakes the lane.
pub async fn track(
    pool: &SqlitePool,
    judge: &Judge,
    channel: &str,
    thread_ref: &str,
    message_ref: &str,
    text: &str,
    at: DateTime<Utc>,
) -> Result<Tracked> {
    let mut tracked = Tracked::default();
    for candidate in candidates(pool, channel, thread_ref, message_ref).await? {
        match delivers(judge, &candidate.what, text).await {
            Ok(Delivery::Delivers) => {
                if fulfil_id(pool, &candidate.id, message_ref).await? {
                    tracked.fulfilled.push(candidate.id);
                }
            }
            Ok(Delivery::Cannot) => {
                if abandon(pool, &candidate.id, text).await?.is_some() {
                    tracked.abandoned.push(candidate.id);
                }
            }
            Ok(Delivery::NotYet) => {}
            Err(error) => tracing::warn!(
                %error,
                commitment = %candidate.id,
                "commitment delivery was not judged; it stays open"
            ),
        }
    }
    if let Some(detected) = detect_at(judge, text, at).await? {
        let due = detected.due.unwrap_or_else(|| default_due(at));
        let lane = lane_of_thread(pool, thread_ref).await?;
        tracked.recorded = Some(
            record(pool, thread_ref, message_ref, &detected.what, due, lane.as_deref()).await?,
        );
    }
    Ok(tracked)
}

/// The lane whose Slack thread is `thread_ref` (`channel:thread_ts`), if any. Lanes store
/// their thread as `slack:<channel>:<thread_ts>`.
pub async fn lane_of_thread(pool: &SqlitePool, thread_ref: &str) -> Result<Option<String>> {
    if !thread_ref.contains(':') {
        return Ok(None);
    }
    let lane: Option<String> =
        sqlx::query_scalar("SELECT key FROM lanes WHERE thread_channel_id = ? ORDER BY key LIMIT 1")
            .bind(format!("slack:{thread_ref}"))
            .fetch_optional(pool)
            .await?;
    Ok(lane)
}

/// Closes the oldest undone commitment of `thread_ref` as delivered by `by_message_ref`. The
/// caller decided delivery (see [`delivers`]). A message never fulfils its own promise.
/// Returns the closed row id.
pub async fn fulfil(
    pool: &SqlitePool,
    thread_ref: &str,
    by_message_ref: &str,
) -> Result<Option<String>> {
    let id: Option<String> = sqlx::query_scalar(
        "SELECT id FROM commitments
         WHERE thread_ref = ? AND state IN ('open','followed_up') AND message_ref <> ?
         ORDER BY created_at, rowid LIMIT 1",
    )
    .bind(thread_ref)
    .bind(by_message_ref)
    .fetch_optional(pool)
    .await?;
    let Some(id) = id else {
        return Ok(None);
    };
    let closed = sqlx::query(
        "UPDATE commitments SET state = 'fulfilled', fulfilled_by = ?
         WHERE id = ? AND state IN ('open','followed_up')",
    )
    .bind(by_message_ref)
    .bind(&id)
    .execute(pool)
    .await?;
    Ok((closed.rows_affected() == 1).then_some(id))
}

/// Closes a commitment the bot can no longer keep. Returns the one honest line, in the owner's
/// language, to post, or `None` when the row was already closed.
pub async fn abandon(pool: &SqlitePool, id: &str, reason: &str) -> Result<Option<String>> {
    let what: Option<String> = sqlx::query_scalar(
        "SELECT what FROM commitments WHERE id = ? AND state IN ('open','followed_up')",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    let Some(what) = what else {
        return Ok(None);
    };
    let closed = sqlx::query(
        "UPDATE commitments SET state = 'abandoned' WHERE id = ? AND state IN ('open','followed_up')",
    )
    .bind(id)
    .execute(pool)
    .await?;
    if closed.rows_affected() != 1 {
        return Ok(None);
    }
    Ok(Some(if crate::locale::current().is_korean() {
        format!("앞서 말씀드린 \"{what}\"은(는) 진행할 수 없게 되었습니다. 이유: {reason}")
    } else {
        format!("I can no longer do what I said earlier: \"{what}\". Reason: {reason}")
    }))
}

const OWNER_CLOSE_INSTRUCTIONS: &str = "The state has a promise the assistant made and the owner's \
later reply in the same thread. Answer \"called_off\" only when the reply tells the assistant the \
promised work is no longer needed or already handled. Answer \"keep\" for anything else, \
including questions, thanks, or new requests.";

/// The owner's reply in a thread with undone commitments: every row of exactly that thread he
/// calls off ("그건 안 해도 돼", "이미 처리했어") is closed `abandoned` with his words as the
/// reason. He said it himself, so nothing is posted. Returns the closed ids.
pub async fn owner_reply(
    pool: &SqlitePool,
    judge: &Judge,
    thread_ref: &str,
    text: &str,
) -> Result<Vec<String>> {
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT id, what FROM commitments
         WHERE thread_ref = ? AND state IN ('open','followed_up')
         ORDER BY created_at, rowid LIMIT ?",
    )
    .bind(thread_ref)
    .bind(MAX_CANDIDATES)
    .fetch_all(pool)
    .await?;
    let mut closed = Vec::new();
    for (id, what) in rows {
        let state = format!(
            "promise:\n{}\n\nowner reply:\n{}",
            scrub_for_external(&what),
            scrub_for_external(text)
        );
        let choice = judge
            .choice(
                Purpose::Commitment,
                &state,
                OWNER_CLOSE_INSTRUCTIONS,
                &["called_off", "keep"],
            )
            .await?;
        if choice.as_str() == "called_off" && abandon(pool, &id, text).await?.is_some() {
            closed.push(id);
        }
    }
    Ok(closed)
}

/// A stored instant in the owner's time zone with its zone abbreviation, e.g. `10/30 12:00 KST`.
fn owner_time(at: &str) -> String {
    DateTime::parse_from_rfc3339(at)
        .map(|parsed| crate::locale::current().format(parsed.with_timezone(&Utc), "%m/%d %H:%M"))
        .unwrap_or_else(|_| at.to_owned())
}

fn follow_up_body(what: &str, due_at: &str) -> String {
    let due = owner_time(due_at);
    if crate::locale::current().is_korean() {
        format!("아직 끝내지 못한 일이 있습니다: \"{what}\". 약속한 시각({due})이 지났는데 결과를 전달하지 못했습니다.")
    } else {
        format!("Something I promised is still not done: \"{what}\". The time I gave ({due}) has passed and I have not delivered the result.")
    }
}

async fn wake_lane(conn: &mut sqlx::SqliteConnection, lane_key: &str, commitment_id: &str, body: &str, now: DateTime<Utc>) -> Result<()> {
    let lead: Option<String> =
        sqlx::query_scalar("SELECT lead_session_key FROM lanes WHERE key = ?")
            .bind(lane_key)
            .fetch_optional(&mut *conn)
            .await?;
    sqlx::query(
        "INSERT OR IGNORE INTO lane_events(lane_key, lead_session_key, kind, body, created_at, source_event_id)
         VALUES(?,?,?,?,?,?)",
    )
    .bind(lane_key)
    .bind(lead.as_deref().unwrap_or(FALLBACK_LEAD))
    .bind(OVERDUE_EVENT_KIND)
    .bind(body)
    .bind(stamp(now))
    .bind(format!("commitment:{commitment_id}"))
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Marks every `open` commitment due at or before `now` as `followed_up`, exactly once, and
/// returns one follow-up per row. A row with a lane also gets a `commitment_overdue` wake.
pub async fn sweep(pool: &SqlitePool, now: DateTime<Utc>) -> Result<Vec<FollowUp>> {
    let rows = sqlx::query(
        "SELECT id, thread_ref, what, due_at, lane_key FROM commitments
         WHERE state = 'open' AND due_at <= ?
         ORDER BY due_at, rowid",
    )
    .bind(stamp(now))
    .fetch_all(pool)
    .await?;
    let mut follow_ups = Vec::with_capacity(rows.len());
    for row in rows {
        let id: String = row.get("id");
        let mut tx = pool.begin_write().await?;
        let claimed = sqlx::query(
            "UPDATE commitments SET state = 'followed_up', followed_up_at = ?
             WHERE id = ? AND state = 'open'",
        )
        .bind(stamp(now))
        .bind(&id)
        .execute(&mut *tx)
        .await?;
        if claimed.rows_affected() != 1 {
            continue;
        }
        let what: String = row.get("what");
        let due_at: String = row.get("due_at");
        let lane_key: Option<String> = row.get("lane_key");
        let body_ko = follow_up_body(&what, &due_at);
        let thread_ref: String = row.get("thread_ref");
        let (channel, thread) = match thread_ref.split_once(':') {
            Some((channel, thread)) => (channel, Some(thread)),
            None => (thread_ref.as_str(), None),
        };
        let session = crate::chat::session::SessionKey::new("slack", None::<String>, channel, thread, "owner");
        let report = crate::report_gate::Report::new(
            format!("commitment:{id}:overdue"), "commitment", format!("commitment:{id}"),
            crate::report_gate::Salience::Result, &body_ko,
        ).with_state("overdue");
        let (_, outbox_id) = crate::report_gate::queue_delivery_in(
            &mut tx, &session.storage_key(), &report, false, &body_ko, None,
        ).await?;
        if let Some(lane) = lane_key.as_deref() {
            let wake = format!("overdue owner commitment {id}: {what}");
            wake_lane(&mut tx, lane, &id, &wake, now).await?;
        }
        tx.commit().await?;
        follow_ups.push(FollowUp {
            commitment_id: id,
            thread_ref,
            body_ko,
            lane_key,
            outbox_id,
        });
    }
    Ok(follow_ups)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .expect("test timestamp")
            .with_timezone(&Utc)
    }

    async fn pool() -> SqlitePool {
        crate::storage::memory().await.expect("memory pool")
    }

    #[tokio::test]
    async fn marker_free_text_skips_the_judge() {
        // A forced judge with an option outside the list errors if it is ever asked.
        let judge = Judge::forced("unreachable");
        let detected = detect(&judge, "배포 끝났습니다. 테스트 전부 통과했어요.")
            .await
            .expect("pre-check answers without the judge");
        assert_eq!(detected, None);
    }

    #[tokio::test]
    async fn promise_is_detected_with_a_relative_due() {
        let judge = Judge::forced("promise");
        let sent = at("2026-10-30T01:00:00Z");
        let detected = detect_at(&judge, "지금 로그 보고 있어요. 30분 안에 확인해서 알려드릴게요.", sent)
            .await
            .expect("detect")
            .expect("a promise");
        assert_eq!(detected.due, Some(at("2026-10-30T01:30:00Z")));
        assert!(detected.what.contains("알려드릴게요"));
        assert_eq!(default_due(sent), at("2026-10-30T03:00:00Z"));
    }

    #[tokio::test]
    async fn overdue_promise_gets_one_follow_up_and_a_lane_wake() {
        let pool = pool().await;
        let sent = at("2026-10-30T01:00:00Z");
        let id = record(
            &pool,
            "C1:1000.1",
            "C1:1000.2",
            "빌드 실패 원인 확인해서 알려드릴게요",
            default_due(sent),
            Some("report-gate"),
        )
        .await
        .expect("record");
        let again = record(
            &pool,
            "C1:1000.1",
            "C1:1000.2",
            "replayed",
            default_due(sent),
            Some("report-gate"),
        )
        .await
        .expect("replayed record");
        assert_eq!(id, again);

        assert!(sweep(&pool, at("2026-10-30T02:59:00Z")).await.expect("early sweep").is_empty());

        let follow_ups = sweep(&pool, at("2026-10-30T03:10:00Z")).await.expect("sweep");
        assert_eq!(follow_ups.len(), 1);
        let follow_up = &follow_ups[0];
        assert_eq!(follow_up.commitment_id, id);
        assert_eq!(follow_up.thread_ref, "C1:1000.1");
        assert_eq!(follow_up.lane_key.as_deref(), Some("report-gate"));
        assert!(follow_up.body_ko.contains("빌드 실패 원인"));
        assert!(follow_up.body_ko.contains("아직"));

        assert!(sweep(&pool, at("2026-10-30T04:00:00Z")).await.expect("second sweep").is_empty());

        let wakes: Vec<(String, String)> = sqlx::query_as(
            "SELECT lane_key, kind FROM lane_events WHERE kind = ?",
        )
        .bind(OVERDUE_EVENT_KIND)
        .fetch_all(&pool)
        .await
        .expect("lane events");
        assert_eq!(wakes, vec![("report-gate".to_owned(), OVERDUE_EVENT_KIND.to_owned())]);

        let outbox_id = follow_up.outbox_id.expect("durable follow-up");
        let (key, text, delivered): (String, String, Option<String>) = sqlx::query_as(
            "SELECT session_key, text, delivered_at FROM chat_outbox WHERE id = ?",
        ).bind(outbox_id).fetch_one(&pool).await.unwrap();
        let session = crate::chat::session::SessionKey::from_storage_key(&key).unwrap();
        assert_eq!(session.channel_id, "C1");
        assert_eq!(session.thread_id.as_deref(), Some("1000.1"));
        assert_eq!(text, follow_up.body_ko);
        assert!(delivered.is_none());
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chat_outbox")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(count, 1);

        let state: String = sqlx::query_scalar("SELECT state FROM commitments WHERE id = ?")
            .bind(&id)
            .fetch_one(&pool)
            .await
            .expect("state");
        assert_eq!(state, "followed_up");
    }

    #[tokio::test]
    async fn failed_follow_up_queue_leaves_the_commitment_open_for_retry() {
        let pool = pool().await;
        let now = at("2026-10-30T03:00:00Z");
        let id = record(&pool, "C1:1000.1", "C1:1000.2", "로그 확인해서 알려드릴게요", now, Some("report-gate"))
            .await.unwrap();
        sqlx::query("CREATE TRIGGER reject_follow_up BEFORE INSERT ON chat_outbox BEGIN SELECT RAISE(ABORT, 'test failure'); END")
            .execute(&pool).await.unwrap();
        assert!(sweep(&pool, now).await.is_err());
        let state: String = sqlx::query_scalar("SELECT state FROM commitments WHERE id = ?")
            .bind(&id).fetch_one(&pool).await.unwrap();
        assert_eq!(state, "open");
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM report_suppressions")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(count, 0);
        sqlx::query("DROP TRIGGER reject_follow_up").execute(&pool).await.unwrap();
        let (first, second) = tokio::join!(sweep(&pool, now), sweep(&pool, now));
        let first = first.unwrap();
        let second = second.unwrap();
        assert_eq!(first.len() + second.len(), 1);
        assert!(first.iter().chain(&second).all(|follow_up| follow_up.outbox_id.is_some()));
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM lane_events WHERE kind = 'commitment_overdue'")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn fulfilled_promise_gets_no_follow_up() {
        let pool = pool().await;
        let sent = at("2026-10-30T01:00:00Z");
        let id = record(&pool, "C1:2000.1", "C1:2000.2", "정리해서 공유드릴게요", default_due(sent), None)
            .await
            .expect("record");
        assert_eq!(
            fulfil(&pool, "C1:2000.1", "C1:2000.2").await.expect("self fulfil"),
            None
        );
        assert_eq!(
            fulfil(&pool, "C1:2000.1", "C1:2000.5").await.expect("fulfil"),
            Some(id.clone())
        );
        assert!(sweep(&pool, at("2026-10-31T00:00:00Z")).await.expect("sweep").is_empty());
        let fulfilled_by: Option<String> =
            sqlx::query_scalar("SELECT fulfilled_by FROM commitments WHERE id = ?")
                .bind(&id)
                .fetch_one(&pool)
                .await
                .expect("fulfilled_by");
        assert_eq!(fulfilled_by.as_deref(), Some("C1:2000.5"));
        let wakes: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM lane_events WHERE kind = ?")
            .bind(OVERDUE_EVENT_KIND)
            .fetch_one(&pool)
            .await
            .expect("count");
        assert_eq!(wakes, 0);
    }

    #[tokio::test]
    async fn abandoned_promise_gets_one_line_and_no_follow_up() {
        let pool = pool().await;
        let sent = at("2026-10-30T01:00:00Z");
        let id = record(&pool, "C1:3000.1", "C1:3000.2", "확인 후 다시 말씀드릴게요", default_due(sent), None)
            .await
            .expect("record");
        let line = abandon(&pool, &id, "로그인 권한이 없습니다").await.expect("abandon");
        assert!(line.expect("one line").contains("로그인 권한"));
        assert_eq!(abandon(&pool, &id, "again").await.expect("second abandon"), None);
        assert!(sweep(&pool, at("2026-10-31T00:00:00Z")).await.expect("sweep").is_empty());
    }

    #[tokio::test]
    async fn follow_ups_use_the_owner_time_zone_and_language() {
        use crate::locale::{scope, OwnerLocale};
        assert_eq!(
            follow_up_body("로그 확인", "2026-10-30T03:00:00Z"),
            "아직 끝내지 못한 일이 있습니다: \"로그 확인\". 약속한 시각(10/30 12:00 KST)이 지났는데 결과를 전달하지 못했습니다."
        );
        scope(OwnerLocale::english_utc(), async {
            assert_eq!(
                follow_up_body("check the logs", "2026-10-30T03:00:00Z"),
                "Something I promised is still not done: \"check the logs\". The time I gave (10/30 03:00 UTC) has passed and I have not delivered the result."
            );
            let pool = pool().await;
            let id = record(&pool, "C1:4000.1", "C1:4000.2", "I'll check and report back", at("2026-10-30T01:00:00Z"), None)
                .await.unwrap();
            let line = abandon(&pool, &id, "no access").await.unwrap().unwrap();
            assert_eq!(line, "I can no longer do what I said earlier: \"I'll check and report back\". Reason: no access");
        }).await;
    }

    #[test]
    fn a_bare_duration_is_not_a_deadline() {
        let sent = at("2026-10-30T01:00:00Z");
        assert_eq!(parse_due("30분마다 다시 넘어왔습니다", sent), None);
        assert_eq!(parse_due("정리 후 26분 동안 지켜보겠습니다", sent), None);
        assert_eq!(
            parse_due("1시간 안에 알려드리겠습니다", sent),
            Some(at("2026-10-30T02:00:00Z"))
        );
        assert_eq!(
            parse_due("I will report back within 15 minutes", sent),
            Some(at("2026-10-30T01:15:00Z"))
        );
    }

    #[tokio::test]
    async fn candidates_are_same_thread_first_then_channel_newest_first_and_capped() {
        let pool = pool().await;
        let due = at("2026-10-30T03:00:00Z");
        for n in 0..10 {
            record(&pool, &format!("D1:{n}.0"), &format!("D1:{n}.0"), &format!("other {n}"), due, None)
                .await
                .expect("record other thread");
        }
        let older = record(&pool, "D1:100.0", "D1:100.0", "same thread older", due, None)
            .await
            .expect("record");
        let newer = record(&pool, "D1:100.0", "D1:100.5", "same thread newer", due, None)
            .await
            .expect("record");
        record(&pool, "D2:1.0", "D2:1.0", "another conversation", due, None)
            .await
            .expect("record");

        let found = candidates(&pool, "D1", "D1:100.0", "D1:101.0").await.expect("candidates");
        assert_eq!(found.len(), MAX_CANDIDATES as usize);
        assert_eq!(found[0].id, newer);
        assert_eq!(found[1].id, older);
        assert_eq!(found[2].what, "other 9");
        assert!(found.iter().all(|c| c.thread_ref.starts_with("D1")));

        let own = candidates(&pool, "D1", "D1:100.0", "D1:100.5").await.expect("candidates");
        assert!(own.iter().all(|c| c.id != newer), "a message is not its own candidate");
    }

    /// A scripted judge that answers `promise` for texts containing "PROMISE", `delivers`
    /// when the newer message names the promise's tag ("DONE:<tag>") and `cannot` when it
    /// says "CANNOT:<tag>".
    fn tag_judge() -> Judge {
        Judge::scripted(|_, state, options| {
            if options.contains(&"promise") {
                return if state.contains("PROMISE") { "promise" } else { "none" }.to_owned();
            }
            let (promise, newer) = state.split_once("newer message:").unwrap_or((state, ""));
            let tags: Vec<&str> = promise
                .split_whitespace()
                .filter_map(|word| word.strip_prefix("tag="))
                .collect();
            if tags.iter().any(|tag| newer.contains(&format!("DONE:{tag}"))) {
                "delivers"
            } else if tags.iter().any(|tag| newer.contains(&format!("CANNOT:{tag}"))) {
                "cannot"
            } else {
                "not_yet"
            }
            .to_owned()
        })
    }

    #[tokio::test]
    async fn the_owner_calling_a_promise_off_closes_only_that_thread() {
        let pool = pool().await;
        let sent = at("2026-10-30T00:00:00Z");
        let mine = record(&pool, "D1:7.1", "D1:7.2", "정리해서 공유드릴게요", default_due(sent), None)
            .await
            .expect("record");
        let other = record(&pool, "D1:8.1", "D1:8.2", "확인 후 다시 말씀드릴게요", default_due(sent), None)
            .await
            .expect("record");
        let keep = Judge::forced("keep");
        assert!(owner_reply(&pool, &keep, "D1:7.1", "고마워").await.expect("keep").is_empty());
        let off = Judge::forced("called_off");
        let closed = owner_reply(&pool, &off, "D1:7.1", "그건 안 해도 돼").await.expect("called off");
        assert_eq!(closed, vec![mine]);
        let follow_ups = sweep(&pool, at("2026-10-31T00:00:00Z")).await.expect("sweep");
        assert_eq!(follow_ups.len(), 1);
        assert_eq!(follow_ups[0].commitment_id, other);
    }

    #[tokio::test]
    async fn a_message_saying_the_work_cannot_be_done_abandons_the_promise() {
        let pool = pool().await;
        let judge = tag_judge();
        let sent = at("2026-10-30T00:00:00Z");
        let promise = track(&pool, &judge, "D1", "D1:k", "D1:k.1", "PROMISE 확인해서 알려드릴게요 tag=k", sent)
            .await
            .expect("track promise")
            .recorded
            .expect("recorded");
        let told = track(&pool, &judge, "D1", "D1:k", "D1:k.2", "CANNOT:k 로그인 권한이 없어 못 합니다", sent)
            .await
            .expect("track cannot");
        assert_eq!(told.abandoned, vec![promise.clone()]);
        assert!(told.fulfilled.is_empty());
        let state: String = sqlx::query_scalar("SELECT state FROM commitments WHERE id = ?")
            .bind(&promise)
            .fetch_one(&pool)
            .await
            .expect("state");
        assert_eq!(state, "abandoned");
        assert!(sweep(&pool, at("2026-10-31T00:00:00Z")).await.expect("sweep").is_empty());
    }

    #[tokio::test]
    async fn a_promise_in_a_lane_thread_belongs_to_the_lane_and_wakes_it_when_overdue() {
        let pool = pool().await;
        sqlx::query(
            "INSERT INTO lanes(key, title, lead_session_key, cwd, session_key, status, done_signals, last_activity_at, thread_channel_id)
             VALUES('nightly','Nightly','lead-nightly','/tmp','s','working','[]','2026-10-30T00:00:00Z','slack:C1:50.1')",
        )
        .execute(&pool)
        .await
        .expect("lane");
        let judge = tag_judge();
        let sent = at("2026-10-30T00:00:00Z");
        let id = track(&pool, &judge, "C1", "C1:50.1", "C1:50.2", "PROMISE 끝나면 보고드릴게요 tag=n", sent)
            .await
            .expect("track")
            .recorded
            .expect("recorded");
        let follow_ups = sweep(&pool, at("2026-10-31T00:00:00Z")).await.expect("sweep");
        assert_eq!(follow_ups.len(), 1);
        assert_eq!(follow_ups[0].commitment_id, id);
        assert_eq!(follow_ups[0].lane_key.as_deref(), Some("nightly"));
        let wake: (String, String) = sqlx::query_as(
            "SELECT lead_session_key, kind FROM lane_events WHERE lane_key = 'nightly'",
        )
        .fetch_one(&pool)
        .await
        .expect("wake");
        assert_eq!(wake, ("lead-nightly".to_owned(), OVERDUE_EVENT_KIND.to_owned()));

        let unbound = track(&pool, &judge, "C1", "C1:60.1", "C1:60.2", "PROMISE 알려드릴게요 tag=u", sent)
            .await
            .expect("track")
            .recorded
            .expect("recorded");
        let lane: Option<String> = sqlx::query_scalar("SELECT lane_key FROM commitments WHERE id = ?")
            .bind(&unbound)
            .fetch_one(&pool)
            .await
            .expect("lane_key");
        assert_eq!(lane, None);
    }

    #[tokio::test]
    async fn one_message_fulfils_several_promises_across_threads() {
        let pool = pool().await;
        let judge = tag_judge();
        let sent = at("2026-10-30T00:00:00Z");
        let mut ids = Vec::new();
        for tag in ["a", "b", "c"] {
            let tracked = track(
                &pool,
                &judge,
                "D1",
                &format!("D1:{tag}"),
                &format!("D1:{tag}"),
                &format!("PROMISE 확인해서 알려드릴게요 tag={tag}"),
                sent,
            )
            .await
            .expect("track promise");
            ids.push(tracked.recorded.expect("recorded"));
        }
        let tracked = track(&pool, &judge, "D1", "D1:z", "D1:z", "DONE:a DONE:c 결과입니다", sent)
            .await
            .expect("track delivery");
        let mut fulfilled = tracked.fulfilled.clone();
        fulfilled.sort();
        let mut expected = vec![ids[0].clone(), ids[2].clone()];
        expected.sort();
        assert_eq!(fulfilled, expected);
        assert_eq!(tracked.recorded, None);

        let follow_ups = sweep(&pool, at("2026-10-31T00:00:00Z")).await.expect("sweep");
        assert_eq!(follow_ups.len(), 1);
        assert_eq!(follow_ups[0].commitment_id, ids[1]);
    }

    #[tokio::test]
    async fn fulfilment_closes_the_delivered_promise_not_the_oldest() {
        let pool = pool().await;
        let judge = tag_judge();
        let sent = at("2026-10-30T00:00:00Z");
        let mut ids = Vec::new();
        for (n, tag) in ["old", "new"].iter().enumerate() {
            let tracked = track(
                &pool,
                &judge,
                "D1",
                "D1:t",
                &format!("D1:t.{n}"),
                &format!("PROMISE 끝나면 알려드리겠습니다 tag={tag}"),
                sent,
            )
            .await
            .expect("track promise");
            ids.push(tracked.recorded.expect("recorded"));
        }
        let tracked = track(&pool, &judge, "D1", "D1:t", "D1:t.9", "DONE:new 완료", sent)
            .await
            .expect("track delivery");
        assert_eq!(tracked.fulfilled, vec![ids[1].clone()]);
        let follow_ups = sweep(&pool, at("2026-10-31T00:00:00Z")).await.expect("sweep");
        assert_eq!(follow_ups.len(), 1);
        assert_eq!(follow_ups[0].commitment_id, ids[0]);
    }

    #[tokio::test]
    async fn a_promise_message_can_also_deliver_an_earlier_one() {
        let pool = pool().await;
        let judge = tag_judge();
        let sent = at("2026-10-30T00:00:00Z");
        let first = track(&pool, &judge, "D1", "D1:1", "D1:1", "PROMISE 끝나면 보고드릴게요 tag=x", sent)
            .await
            .expect("track")
            .recorded
            .expect("recorded");
        let second = track(&pool, &judge, "D1", "D1:2", "D1:2", "DONE:x PROMISE 지켜보겠습니다 tag=y", sent)
            .await
            .expect("track");
        assert_eq!(second.fulfilled, vec![first]);
        assert!(second.recorded.is_some());
    }

    /// Every hub (wawabot) row of the audit that a promise or a fulfilment involves, in time
    /// order, as `(row, sender, thread_ref, sent_at, text)`. Thread refs are
    /// `channel:thread_ts`, or `channel:ts` for a top-level message, so rows 89, 99, 101, 102
    /// and 117 are each their own top-level thread of the owner DM.
    const AUDITED_ROWS: [(u32, &str, &str, &str, &str); 30] = [
        (
            3,
            "wawabot",
            "DM-wawabot:1793242865.232429",
            "2026-10-29T03:01:42Z",
            ":hourglass_flowing_sand: 지원서 작성 접수했습니다. 끝나면 여기에 결과를 남깁니다.",
        ),
        (
            4,
            "wawabot",
            "DM-wawabot:1793242865.232429",
            "2026-10-29T03:02:18Z",
            ":warning: 위시켓 지원 실패 • 지원 내역에 없습니다 (에이전트: 프로젝트가 PRIME·PRO·BOOST 전용이며 현재 지원 버튼이 BOOST 멤버십 신청으로 연결되어 지원서를 제출할 수 없습니다.) Aside 세션: S5Dk0aN11hx5qYad 버튼을 다시 누르면 재시도합니다.",
        ),
        (
            17,
            "wawabot",
            "#work:1793177950.361429",
            "2026-10-29T04:58:25Z",
            "로그를 모으는 중이에요. 결과가 나오면 바로 이어서 원인을 정리할게요.",
        ),
        (
            20,
            "wawabot",
            "DM-wawabot:1793242865.232429",
            "2026-10-29T05:16:00Z",
            "지원 실패 기록에 이유가 나와 있습니다. 실제 공고 화면에서 그 이유가 맞는지 한 번 더 보겠습니다.",
        ),
        (
            21,
            "wawabot",
            "DM-wawabot:1793242865.232429",
            "2026-10-29T05:16:30Z",
            "이 공고는 의뢰인이 PRIME·PRO·BOOST 등급 파트너만 지원받도록 막아 둔 공고여서 지원이 안 됩니다. 아까 멤버십 없이 지원할 수 있다고 한 제 답이 틀렸습니다. 방금 Aside로 공고 페이지(<https://www.wishket.com/project/159301/>)를 직접 열어 확인했습니다. 화면에 \"PRIME·PRO·BOOST 파트너에게\"라는 문구가 있고, 지원 버튼 대신 \"BOOST 파트너 신청하기\" 버튼만 있습니다. 오늘 12:01(KST)에 자동 지원을 시도한 기록도 남아 있습니다. 같은 이유로 지원서를 내지 못했고, 실제로 지원된 것은 없습니다. 이 건에 지원하려면 BOOST에 가입하는 수밖에 없습니다. 가입하면 월 67,500원이 들고, 심사를 통과해야 합니다. 가입하지 않으면 이 건은 넘기시면 됩니다.",
        ),
        (
            75,
            "wawabot",
            "DM-wawabot:1793263383.930969",
            "2026-10-29T15:16:29Z",
            "While that runs, I'll check why proposals were 0 in parallel.",
        ),
        (
            84,
            "wawabot",
            "DM-wawabot:1793263383.930969",
            "2026-10-29T15:22:07Z",
            "프리모아를 다시 고쳤습니다. 지금은 자동 재로그인과 수집이 정상으로 돕니다. *프리모아가 안 된 이유* • 10/27 이후로 프리모아에서 수집된 공고가 하나도 없었습니다. 10/28에는 Aside 오류가 났고, 10/29에는 하루 종일 재로그인에 실패했습니다. • 원인은 두 가지였습니다. • 구글이 계정을 고른 뒤 \"프리모아 서비스로 로그인 → 계속\"이라는 확인 화면을 새로 띄우는데, 이 버튼을 누르는 단계가 코드에 없었습니다. • 한 번 실패하면 그때 열린 구글 로그인 창이 남았습니다. 다음 실행이 이 창을 보고 바로 \"로그인 만료\"로 끝나서, 이후 실행도 계속 실패했습니다. • 그래서 재로그인이 \"계속\" 버튼까지 누르게 하고, 시작할 때 남아 있는 로그인 창을 먼저 닫도록 `freemoa.ts`를 고쳤습니다. • 고친 뒤 재로그인을 실제로 돌려 로그인 성공을 확인했습니다. 프리모아 수집도 오류 없이 끝났고, 새 공고 2건을 판정했습니다. *오늘 제안이 0건인 이유* • 오늘 들어온 공고는 위시켓·Freelancer 15건이었고, 모두 판정까지 마쳤습니다. • 14건은 상주, 기간제 풀타임, 사업자 증빙 필수 조건이거나 적합도가 기준(40점) 미만이라 건너뛰었습니다. • 1건은 대표님이 승인했지만 지원에 실패했습니다. 위시켓에서 BOOST 멤버십 전용 공고라 지원서를 낼 수 없었습니다. • 프리모아는 위 로그인 문제로 오늘 한 건도 못 가져왔습니다. 방금 들어온 2건도 적합도가 18점, 38점이라 건너뛰었습니다. • 결국 오늘은 조건에 맞는 공고가 없었고, 프리모아는 로그인 문제로 빠져 있었습니다. 비교하면 10/28에는 제안 카드가 4건 나갔습니다. 00:05~00:20에도 Aside가 30초마다 죽었다 살아나기를 다시 반복했습니다. 그 시간에 다른 작업이 Aside에 맡긴 ChatGPT Dots 점검 세션이 있었고, Aside가 재시작할 때마다 그 세션을 이어 돌리던 중이었습니다. 지금은 Aside가 정상으로 떠 있습니다.",
        ),
        (
            85,
            "wawabot",
            "#omo-home:1793286885.245939",
            "2026-10-29T15:24:11Z",
            "메일 알림을 끄는 수정을 넣었지만 아직 실제 hub에는 반영되지 않았습니다. 테스트가 아직 돌고 있어서, 통과를 확인한 다음 main에 머지하고 재배포하겠습니다. *원인* • 채널에 올라오던 메일 알림은 대부분 GitHub `Run failed` CI 메일이었습니다. 오늘만 10건 가까이 됩니다. • hub가 이런 메일을 '봇 처리' 항목으로 분류해 봇에게 넘기고, 봇이 그때마다 채널에 요약을 올리는 구조였습니다. 처리되지 않은 항목은 30분마다 다시 넘어왔습니다. *바꾼 것* • *코드:* 이제 메일은 봇에게 넘기지 않고, 이미 넘어간 메일도 다시 넘기지 않게 Ferryx hub 코드를 고쳤습니다. 이 동작을 확인하는 테스트도 하나 추가했습니다. • *봇 규칙:* 메일만 담긴 알림이 오면 아무것도 보내지 않도록 봇 규칙에 적어 두었습니다. 다만 새 대화부터 적용되는지는 아직 확인하지 못했습니다. 계속 받게 되는 메일은 대표님이 직접 처리해야 하는 것뿐입니다. 서류 보완 요청이나 보안·결제 경고 같은 메일만 지금처럼 DM :mailbox_with_mail:으로 한 번 갑니다.",
        ),
        (
            87,
            "wawabot",
            "#omo-home:1793286885.245939",
            "2026-10-29T15:26:38Z",
            "메일 알림을 끄는 수정은 main에 들어갔지만, 실제 hub는 아직 예전 버전이라 지금은 메일 알림이 그대로 올 수 있습니다. • *수정 내용:* GitHub CI 실패 메일, 뉴스레터, 영수증 같은 메일을 더 이상 채널에 올리지 않습니다. 이미 대기 중이던 메일이 30분마다 다시 올라오던 것도 막았습니다. 직접 처리하셔야 하는 메일만 지금처럼 DM :mailbox_with_mail:으로 한 번 갑니다. • *확인:* 관련 테스트 10개가 모두 통과했고 clippy 검사도 문제없이 끝났습니다. 커밋은 `98562d32`이고 버전은 2026.1030.1로 올렸습니다. • *반영 절차:* 지금 공증 빌드의 사전 점검을 돌리고 있습니다. 통과하면 빌드에 20분 넘게 걸리고, 그 뒤 앱을 설치하고 hub를 새 버전으로 넘깁니다. 끝나면 실제로 메일 알림이 멈췄는지까지 확인해서 다시 알려드리겠습니다.",
        ),
        (
            89,
            "wawabot",
            "DM-wawabot:1793287716.405099",
            "2026-10-29T15:28:36Z",
            "카톡·외주 수정은 끝났고, 다음 정기 실행에서 두 크론이 성공하는지 확인을 기다리고 있습니다. 결과가 나오면 바로 알려 드리겠습니다.",
        ),
        (
            99,
            "wawabot",
            "DM-wawabot:1793314995.863159",
            "2026-10-29T23:03:15Z",
            "카톡은 고쳐졌습니다. 수정 이후 카톡 아카이브가 31번 돌았고 실패는 없었습니다. 원인은 맥이 잠들었다 깰 때 카톡 서버가 몇 분씩 응답하지 않던 것입니다. 앞으로는 1시간 안에 다시 연결되면 오류로 치지 않고, 다음 실행에서 이어서 받아 옵니다. 외주는 수정을 마쳤지만 정기 실행으로는 아직 확인하지 못했습니다. 외주 크론은 KST 09~23시에만 돌아서 수정 후 한 번도 실행되지 않았고, 첫 실행은 오늘 09:00입니다. • 프리모아 로그인은 다시 됐습니다. 직접 실행해 보니 로그인 상태가 확인됐고, 공고 7건도 정상으로 읽혔습니다. • Aside가 재시작되는 사이에 나던 시간 초과(CDP timeout)는 앞으로 일시 장애로 보고 오류 알림을 보내지 않게 했습니다. • 지금 수집 전체를 실제 지원 없이 시험으로 돌리고 있어서, 결과가 나오는 대로 알려 드리겠습니다. 근본 원인 하나는 아직 남아 있습니다. Aside 데몬이 파일 감시 한도를 넘어(EMFILE) 30초마다 재시작되고 있습니다. Aside 세션 폴더가 2만 8천 개, 10GB까지 쌓였고, 맥의 파일 열기 기본 한도는 256입니다. 이건 Aside 쪽 문제라 우리 코드로는 증상만 막아 둔 상태입니다. 해결하려면 아래 둘 중 하나를 정해 주셔야 합니다. 1. 오래된 Aside 세션 폴더를 정리합니다. 지워진 세션 기록은 되살릴 수 없습니다. 2. 파일 열기 한도를 올립니다. 대표님이 sudo 비밀번호를 한 번 입력하셔야 합니다. 같이 확인하실 것이 두 가지 있습니다. • 어젯밤 구글 계정(freedomzero91)에 Hark의 데이터 접근을 허용했다는 보안 알림이 왔습니다. 대표님이 허용하신 게 아니라면 권한을 취소해야 합니다. • 서울경제진흥원에 내는 중소기업확인서 마감이 오늘(10/30)입니다. 메일함을 확인해 보니 아직 보내지 않았습니다.",
        ),
        (
            101,
            "wawabot",
            "DM-wawabot:1793315182.430609",
            "2026-10-29T23:06:22Z",
            "Waiting for the 4-minute daemon watch to finish before reporting.",
        ),
        (
            102,
            "wawabot",
            "DM-wawabot:1793316753.140909",
            "2026-10-29T23:32:33Z",
            "오래된 Aside 세션 폴더 24,110개를 정리했습니다. 10월 23일 이전 것만 골라 `~/archive/aside-sessions-u0`로 옮겼고, 지우지 않았기 때문에 필요하면 다시 되돌릴 수 있습니다. 세션 폴더는 28,235개에서 4,144개로 줄었습니다. 정리 후 26분 동안 Aside 데몬이 죽은 적은 없습니다. 다만 마지막 크래시는 어젯밤 00:19(KST)였고, 그 뒤로는 정리 전부터 이미 멈춰 있었습니다. 그래서 크래시가 멈춘 게 이번 정리 덕분이라고는 아직 말할 수 없습니다. 오늘 Aside 작업이 많이 도는 시간대에 다시 크래시가 나는지 지켜보겠습니다. 외주 크론은 오늘 09:00 첫 실행에서 수정 결과를 확인합니다. 아래 두 건은 아직 대표님 답을 기다리고 있습니다. • 구글 계정에 Hark 접근을 허용한 게 대표님이 맞는지 알려 주세요. • 서울경제진흥원 중소기업확인서는 오늘이 마감인데, 아직 보내지 않았습니다.",
        ),
        (
            117,
            "wawabot",
            "DM-wawabot:1793321304.367969",
            "2026-10-30T00:48:24Z",
            "알겠습니다. Hark 알림은 대표님이 허용하신 것으로 정리했습니다. 중소기업확인서도 다른 세션에서 처리 중인 것으로 알고, 같은 알림이 다시 와도 올리지 않겠습니다. 수정한 카톡과 외주 크론은 오늘 아침 정상 실행을 확인했습니다. 외주는 09:40, 카톡은 09:45 실행이 성공했습니다. 09:00 정각에는 두 크론 모두 실패로 찍혔는데, 같은 시각에 hub 크론 9개 중 7개가 한꺼번에 시간 초과로 끝났습니다. 그래서 이번에 고친 코드 문제가 아니라 그 시각 hub 쪽 문제로 보입니다. 원인은 아직 확인하지 못했습니다. Aside 데몬은 세션을 정리한 뒤 지금까지 다시 죽지 않았습니다.",
        ),
        (
            127,
            "wawabot",
            "#work:1793321329.312279",
            "2026-10-30T00:52:12Z",
            "Gmail 발송이 차단됐어요. 지난번에도 Gmail 발송 IP에 따라 반송된 메일과 들어간 메일이 갈렸어요. 그래서 같은 내용을 한 번만 다시 보내 보겠습니다.",
        ),
        (
            129,
            "wawabot",
            "#work:1793321329.312279",
            "2026-10-30T00:53:25Z",
            "회신 메일을 두 번 보냈어요. 첫 번째는 반송됐고, 두 번째는 3분이 지나도록 반송이 없어서 들어갔을 가능성이 높아요. 다만 상대방이 받았는지는 직접 확인할 방법이 없어요. 보낸 내용 • Codex는 로컬 기능만으로 실습할 수 있어요. 클라우드 권한은 켜지 않아도 됩니다. • 앱은 공식 사이트에서 내려받아 설치하고 기관 계정으로 로그인만 하면 돼요. 당일 현장에서 세팅해도 충분합니다. 첫 발송(09:51 KST쯤)은 진흥원 메일 서버가 Gmail 발송 서버 하나를 차단 목록에 올려 둬서 막혔어요. 10/28에 반송된 메일과 같은 원인이에요. 두 번째는 다른 발송 서버로 나간 것으로 보여요. 이 차단은 Gmail이 어느 서버로 보내느냐에 따라 앞으로도 또 생길 수 있어요. 월요일 강의 전까지 꼭 전달돼야 하는 내용이라면, 김다빈 선생님(02-6363-8343)께 문자로 \"메일 회신드렸다\"고 한 줄 남기시는 게 확실해요.",
        ),
        (
            155,
            "wawabot",
            "DM-wawabot:1793326272.243319",
            "2026-10-30T02:34:16Z",
            ":hourglass_flowing_sand: 지원서 작성 접수했습니다. 끝나면 여기에 결과를 남깁니다.",
        ),
        (
            157,
            "Instinct",
            "DM-Instinct:1793055025.828739",
            "2026-10-30T02:34:47Z",
            "네. 제가 영상 제작을 이어가라는 뜻을 놓쳤어요. 예약된 영상 공개만 하는 게 아니라, 남은 영상 제작부터 다시 진행할게요. 완성본은 먼저 보여드릴게요.",
        ),
        (
            158,
            "wawabot",
            "DM-wawabot:1793326272.243319",
            "2026-10-30T02:34:55Z",
            ":warning: 위시켓 지원 실패 • 지원 내역에 없습니다 (에이전트: 프로젝트가 PRIME·PRO·BOOST 전용이며 지원 버튼이 파트너 등급 안내로 연결되어 지원서 제출이 불가능했습니다.) Aside 세션: 6sMpWr0QoF4wQv46 버튼을 다시 누르면 재시도합니다.",
        ),
        (
            159,
            "wawabot",
            "DM-wawabot:1793326864.480529",
            "2026-10-30T02:34:57Z",
            ":hourglass_flowing_sand: Freelancer 입찰 접수했습니다. 끝나면 여기에 결과를 남깁니다.",
        ),
        (
            161,
            "wawabot",
            "DM-wawabot:1793326864.480529",
            "2026-10-30T02:35:09Z",
            ":warning: Freelancer 입찰 실패 • Freelancer 로그아웃 상태입니다. • Aside(u0)에서 Freelancer 로그인이 필요합니다. 버튼을 다시 누르면 재시도합니다.",
        ),
        (
            165,
            "Instinct",
            "DM-Instinct:1793055025.828739",
            "2026-10-30T02:39:06Z",
            "웹툰은 '제삿날 나타난 남편의 아들'이라는 새 이야기로 1화를 만들고 있어요. 가짜 상속인 사기 소재로 잡았어요. 미스터리는 빠져 있던 3화 영어판을 이어서 제작하고 있어요. 둘 다 완성 영상을 먼저 보여드릴게요.",
        ),
        (
            166,
            "wawabot",
            "DM-wawabot:1793326864.480529",
            "2026-10-30T02:39:12Z",
            "Freelancer 계정 자격증명은 `~/.config/outsourcing-autopilot/freelancer.json`에 있습니다. 이걸로 Aside에서 로그인하겠습니다.",
        ),
        (
            167,
            "wawabot",
            "DM-wawabot:1793326864.480529",
            "2026-10-30T02:39:58Z",
            "Aside에서 로그인됐습니다(@osbdevstudio). 이제 입찰을 실행합니다.",
        ),
        (
            168,
            "wawabot",
            "DM-wawabot:1793326864.480529",
            "2026-10-30T02:39:58Z",
            ":hourglass_flowing_sand: Freelancer 입찰 접수했습니다. 끝나면 여기에 결과를 남깁니다.",
        ),
        (
            169,
            "wawabot",
            "DM-wawabot:1793326864.480529",
            "2026-10-30T02:40:11Z",
            ":warning: Freelancer 입찰 실패 • 입찰 폼을 찾지 못했습니다. 버튼을 다시 누르면 재시도합니다.",
        ),
        (
            173,
            "wawabot",
            "DM-wawabot:1793326864.480529",
            "2026-10-30T02:41:53Z",
            ":hourglass_flowing_sand: Freelancer 입찰 접수했습니다. 끝나면 여기에 결과를 남깁니다.",
        ),
        (
            174,
            "wawabot",
            "DM-wawabot:1793326864.480529",
            "2026-10-30T02:42:05Z",
            ":warning: Freelancer 입찰 실패 • Preferred Freelancer만 입찰할 수 있는 공고라 우리 계정으로는 입찰할 수 없습니다. 버튼을 다시 누르면 재시도합니다.",
        ),
        (
            180,
            "wawabot",
            "DM-wawabot:1793326864.480529",
            "2026-10-30T02:46:21Z",
            "Freelancer 자동 재로그인 코드는 고쳤고, 지금 로그아웃 상태를 재현한 별도 프로필에서 검증하고 있습니다. 결과가 나오면 바로 이어서 알려드리겠습니다.",
        ),
        (
            181,
            "wawabot",
            "DM-wawabot:1793326864.480529",
            "2026-10-30T02:46:53Z",
            "네, Preferred Freelancer만 입찰할 수 있는 공고를 말한 겁니다. • 걸러지는 것: 공고 정보에 \"Preferred Freelancer 전용\"이 표시된 공고는 수집할 때 빼서 승인 카드로 올리지 않습니다. 오늘 확인한 시점에는 98건 중 10건이 여기에 해당했습니다. • 아직 못 거르는 것: MYOB 공고에는 \"계정 잔액이 $20 이상이어야 입찰 가능\"이라는 조건도 붙어 있었습니다. 이 조건이 공고 정보 어디에 나오는지는 아직 찾지 못해서 수집 단계에서는 거르지 못합니다. 대신 입찰 버튼을 누르면 \"잔액 $20 필요\"라고 사유를 정확히 보여 주고 멈춥니다. 충전은 하지 않습니다. 자동 재로그인 검증은 로그아웃 상태인 별도 프로필에서 아직 돌고 있습니다. 끝나면 결과를 이어서 알려드리겠습니다.",
        ),
    ];
    /// `(promise row, fulfilling row)`; `None` is never fulfilled. Row 102 is both a promise
    /// and the fulfilment of 101; row 117 fulfils 89, 99 and 102.
    const AUDITED_PAIRS: [(u32, Option<u32>); 20] = [
        (3, Some(4)),
        (17, None),
        (20, Some(21)),
        (75, Some(84)),
        (85, None),
        (87, None),
        (89, Some(117)),
        (99, Some(117)),
        (101, Some(102)),
        (102, Some(117)),
        (127, Some(129)),
        (155, Some(158)),
        (157, None),
        (159, Some(161)),
        (165, None),
        (166, Some(167)),
        (168, Some(169)),
        (173, Some(174)),
        (180, None),
        (181, None),
    ];

    #[test]
    fn audited_unfulfilled_promises_carry_a_marker() {
        for (promise, fulfilled_by) in AUDITED_PAIRS {
            if fulfilled_by.is_none() {
                let text = audited_text(promise);
                assert!(has_promise_marker(text), "row {promise} has no promise marker");
            }
        }
    }

    fn audited_text(row: u32) -> &'static str {
        AUDITED_ROWS
            .iter()
            .find(|audited| audited.0 == row)
            .map(|audited| audited.4)
            .expect("audited row")
    }

    /// Answers from the audit: `promise` for a labelled promise row, `delivers` when the row
    /// being tracked is the one the audit pairs with the candidate's promise.
    fn audit_judge(current: std::sync::Arc<std::sync::Mutex<u32>>) -> Judge {
        Judge::scripted(move |_, state, options| {
            let row = *current.lock().expect("current row");
            if options.contains(&"promise") {
                let promised = AUDITED_PAIRS.iter().any(|(promise, _)| *promise == row);
                return if promised { "promise" } else { "none" }.to_owned();
            }
            let what = state
                .strip_prefix("promise:\n")
                .and_then(|rest| rest.split_once("\n\nnewer message:"))
                .map(|(what, _)| what)
                .unwrap_or_default();
            let delivered = AUDITED_PAIRS.iter().any(|(promise, fulfilled_by)| {
                *fulfilled_by == Some(row) && !what.is_empty() && audited_text(*promise).contains(what)
            });
            (if delivered { "delivers" } else { "not_yet" }).to_owned()
        })
    }

    /// The audited promises through [`track`] and [`sweep`] with production thread refs, in
    /// time order. Only hub rows are tracked: Instinct (157, 165) is another bot the hub never
    /// sees. Row 89 is due at 2h and fulfilled by row 117 eight hours later, so its follow-up
    /// comes first and is honest; no promise is followed up after it was fulfilled.
    #[tokio::test]
    async fn replay_of_audited_promises() {
        let pool = pool().await;
        let current = std::sync::Arc::new(std::sync::Mutex::new(0));
        let judge = audit_judge(current.clone());
        let mut follow_ups: Vec<(String, DateTime<Utc>)> = Vec::new();
        let mut last = at("2026-10-29T00:00:00Z");
        for (row, sender, thread_ref, sent_at, text) in AUDITED_ROWS {
            let now = at(sent_at);
            last = now;
            for follow_up in sweep(&pool, now).await.expect("sweep") {
                follow_ups.push((follow_up.commitment_id, now));
            }
            if sender != "wawabot" {
                continue;
            }
            *current.lock().expect("current row") = row;
            let channel = thread_ref.split_once(':').map_or(thread_ref, |(channel, _)| channel);
            track(&pool, &judge, channel, thread_ref, &format!("row-{row}"), text, now)
                .await
                .expect("track");
        }
        for follow_up in sweep(&pool, last + Duration::hours(24)).await.expect("final sweep") {
            follow_ups.push((follow_up.commitment_id, last + Duration::hours(24)));
        }

        let rows: Vec<(String, String, String, Option<String>)> = sqlx::query_as(
            "SELECT id, message_ref, state, fulfilled_by FROM commitments",
        )
        .fetch_all(&pool)
        .await
        .expect("commitments");
        let row_of = |id: &str| {
            rows.iter()
                .find(|row| row.0 == id)
                .and_then(|row| row.1.strip_prefix("row-"))
                .and_then(|row| row.parse::<u32>().ok())
                .expect("commitment row")
        };
        let mut followed: Vec<u32> = follow_ups.iter().map(|(id, _)| row_of(id)).collect();
        followed.sort_unstable();
        assert_eq!(followed, vec![17, 85, 87, 89, 180, 181], "one follow-up each");

        for (promise, fulfilled_by) in AUDITED_PAIRS {
            let Some(row) = rows.iter().find(|row| row.1 == format!("row-{promise}")) else {
                assert!(
                    matches!(promise, 157 | 165),
                    "hub promise row {promise} was not recorded"
                );
                continue;
            };
            match fulfilled_by {
                Some(by) => {
                    assert_eq!(row.2, "fulfilled", "row {promise}");
                    assert_eq!(row.3.as_deref(), Some(format!("row-{by}").as_str()));
                }
                None => assert_eq!(row.2, "followed_up", "row {promise}"),
            }
        }
        // Fulfilled after its due time: followed up once before row 117 delivered it.
        let late: Vec<u32> = followed
            .iter()
            .copied()
            .filter(|row| AUDITED_PAIRS.iter().any(|(p, by)| p == row && by.is_some()))
            .collect();
        assert_eq!(late, vec![89]);
        let fulfilled_at = at(AUDITED_ROWS.iter().find(|r| r.0 == 117).expect("row 117").3);
        let followed_89 = follow_ups
            .iter()
            .find(|(id, _)| row_of(id) == 89)
            .map(|(_, when)| *when)
            .expect("row 89 follow-up");
        assert!(followed_89 < fulfilled_at);

        assert!(sweep(&pool, last + Duration::hours(48)).await.expect("resweep").is_empty());
    }
}
