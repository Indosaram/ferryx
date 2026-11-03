//! Owner alerts that do not depend on an agent turn.
//! A triage verdict that needs the owner is one plain line, recorded as a delivery obligation.
use chrono::{DateTime, Duration, Utc};
use sqlx::SqlitePool;

use crate::error::Result;
use crate::storage::WriteTx;

pub const OBLIGATION_PREFIX: &str = "obl:owner-alert:";
pub const MAX_ALERT_AGE_HOURS: i64 = 24;
pub const MAX_ALERTS_PER_SWEEP: i64 = 10;
/// While the alert of a conversation was queued less than this long ago and the owner has not
/// answered that conversation, a new message of it is added to that alert instead of being
/// posted as an alert of its own.
pub const COALESCE_WINDOW_MINUTES: i64 = 30;
/// A coalesced alert stops growing at this many lines; the next message starts a new alert.
pub const MAX_COALESCED_LINES: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct PendingAlert {
    pub item_id: String,
    pub source: String,
    pub author: Option<String>,
    pub title: Option<String>,
    pub snippet: String,
    pub link: Option<String>,
}

pub(crate) fn source_label(source: &str) -> &str {
    let locale = crate::locale::current();
    match source {
        "mail" => locale.pick("메일", "Mail"),
        "kakao" => locale.pick("카톡", "KakaoTalk"),
        "imessage" => locale.pick("문자", "SMS"),
        "slack" => locale.pick("슬랙", "Slack"),
        "discord" => locale.pick("디스코드", "Discord"),
        "github" => "GitHub",
        "cron" => locale.pick("크론", "Cron"),
        other => other,
    }
}

/// Sources whose `title` is the room or chat name, not a subject: the message is the snippet.
fn is_chat(source: &str) -> bool {
    matches!(source, "kakao" | "imessage" | "slack" | "discord")
}

/// Words a chat app puts in place of a message it cannot show as text (photo, sticker, file).
const PLACEHOLDER_WORDS: &[&str] = &[
    "사진", "이미지", "동영상", "파일", "이모티콘", "스티커",
    "photo", "image", "video", "file", "sticker", "emoticon",
];

/// True when no readable text is left: links and images only, or placeholder words only.
fn is_placeholder(text: &str) -> bool {
    crate::report_gate::is_empty_body(text)
        || text.split_whitespace().all(|word| {
            let word = word.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
            word.is_empty() || PLACEHOLDER_WORDS.contains(&word.as_str())
        })
}

/// What the alert relays: for a chat the message itself (its title is the room), for mail,
/// GitHub and cron the subject, else the body. Empty when nothing readable is left: a bare
/// image or sticker, or a field that only repeats the sender's name.
fn alert_content(alert: &PendingAlert) -> String {
    let author = alert.author.as_deref().map(str::trim).unwrap_or("");
    let readable = |text: &str| {
        let text = text.trim();
        (!text.is_empty() && text != author && !is_placeholder(text)).then(|| text.to_owned())
    };
    let subject = if is_chat(&alert.source) {
        None
    } else {
        alert.title.as_deref().and_then(readable)
    };
    subject.or_else(|| readable(alert.snippet.as_str())).unwrap_or_default()
}

/// Who the alert is from: the sender, after the room name when the chat room is not just the
/// sender's own name.
fn alert_who(alert: &PendingAlert) -> String {
    let author = alert.author.as_deref().map(str::trim).filter(|author| !author.is_empty());
    let room = alert
        .title
        .as_deref()
        .map(str::trim)
        .filter(|room| is_chat(&alert.source) && !room.is_empty() && Some(*room) != author);
    match (room, author) {
        (Some(room), Some(author)) => format!("{} · {}", one_line(room, 40), one_line(author, 40)),
        (Some(room), None) => one_line(room, 60),
        (None, Some(author)) => one_line(author, 60),
        (None, None) => crate::locale::text("알 수 없음", "unknown"),
    }
}

/// One alert line: source, sender and the message. Never the sender's name twice.
pub fn alert_line(alert: &PendingAlert) -> String {
    let label = source_label(&alert.source);
    let who = alert_who(alert);
    let what = one_line(&alert_content(alert), 200);
    let mut line = if what.is_empty() {
        format!("📬 [{label}] {who}")
    } else {
        format!("📬 [{label}] {who} — {what}")
    };
    if let Some(link) = alert.link.as_deref().filter(|link| !link.trim().is_empty()) {
        line.push_str(&format!("\n{link}"));
    }
    line
}

/// A later message of a conversation, as a line added under the conversation's alert. The
/// sender is named only when it is not the sender the alert was first about.
fn continuation_line(alert: &PendingAlert, first_sender: Option<&str>) -> String {
    let what = one_line(&alert_content(alert), 200);
    match alert.author.as_deref().map(str::trim).filter(|author| !author.is_empty()) {
        Some(author) if Some(author) != first_sender.map(str::trim) => {
            format!("↳ {}: {what}", one_line(author, 40))
        }
        _ => format!("↳ {what}"),
    }
}

/// The alert line with the mail brief, when there is one, as two short lines under the subject.
/// A brief with no action (nothing for the owner to do) adds only the summary line.
pub fn alert_text(alert: &PendingAlert, brief: Option<&crate::mail_brief::Brief>) -> String {
    let mut text = alert_line(alert);
    if let Some(brief) = brief {
        let locale = crate::locale::current();
        text.push_str(&format!(
            "\n{} {}",
            locale.pick("내용:", "Summary:"),
            one_line(&brief.summary, 300)
        ));
        if let Some(action) = &brief.action {
            text.push_str(&format!("\n{} {}", locale.pick("할 일:", "To do:"), one_line(action, 160)));
        }
    }
    text
}

pub(crate) fn one_line(text: &str, max_chars: usize) -> String {
    let flat: String = text
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect();
    let flat = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max_chars {
        return flat;
    }
    let mut cut: String = flat.chars().take(max_chars).collect();
    cut.push('…');
    cut
}

pub async fn pending_alerts(pool: &SqlitePool, now: DateTime<Utc>) -> Result<Vec<PendingAlert>> {
    let since = (now - Duration::hours(MAX_ALERT_AGE_HOURS)).to_rfc3339();
    let rows = sqlx::query_as::<_, PendingAlert>(
        "SELECT i.id AS item_id, i.source, i.author, i.title, i.snippet, i.link
           FROM inbox_decisions d
           JOIN inbox_items i ON i.id = d.item_id
          WHERE d.class = 'owner'
            AND d.status = 'open'
            AND i.source IN ('mail','kakao','imessage','slack','discord','github','cron')
            AND julianday(d.decided_at) >= julianday(?)
            -- Earlier builds surfaced these through the action loop; do not repeat them.
            AND NOT EXISTS (SELECT 1 FROM action_loop_outcomes a WHERE a.item_key = 'inbox:' || i.id)
            AND NOT EXISTS (
                SELECT 1 FROM delivery_obligations o
                 WHERE o.id = ? || i.id
                   AND o.state IN ('pending', 'attempting', 'delivered', 'suppressed')
            )
          ORDER BY d.decided_at ASC
          LIMIT ?",
    )
    .bind(since)
    .bind(OBLIGATION_PREFIX)
    .bind(MAX_ALERTS_PER_SWEEP)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// The gate report for one pending owner item. Its topic is the obligation identity (the
/// item), never the wording, so a re-worded alert about the same item is a duplicate.
/// Its quote is the subject owner rules match (the title, or the message when the title only
/// repeats the sender), and empty when the message has nothing readable ([`alert_content`]),
/// so the gate drops such an alert (`Empty`) and the conversation's next alert carries on.
pub fn alert_report(alert: &PendingAlert) -> crate::report_gate::Report {
    let label = source_label(&alert.source);
    let author = alert.author.as_deref().map(str::trim);
    let quote = if alert_content(alert).is_empty() {
        String::new()
    } else {
        alert
            .title
            .as_deref()
            .map(str::trim)
            .filter(|title| !title.is_empty() && Some(*title) != author)
            .unwrap_or(alert.snippet.trim())
            .to_owned()
    };
    crate::report_gate::Report::new(
        format!("{OBLIGATION_PREFIX}{}", alert.item_id),
        alert.source.clone(),
        format!("inbox:{}", alert.item_id),
        crate::report_gate::Salience::Action,
        crate::locale::current().pick(
            &format!("확인이 필요한 {label} 연락"),
            &format!("A {label} message that needs you"),
        ),
    )
    .with_state(&format!("open:{}", alert.item_id))
    .with_quote(quote)
    .with_sender(alert.author.clone().unwrap_or_default())
    .with_origin(alert.item_id.clone())
}

/// Writes one obligation per pending owner item. Each item goes through the report gate; the
/// ones it holds back are recorded `suppressed` so a later sweep does not resubmit them. A sent
/// item is its own outbox line and Slack message, unless its conversation has a recent
/// unanswered alert ([`coalesce_target`]): then it is added to that alert. No sweep repeats one.
pub async fn sweep(pool: &SqlitePool, owner_session: &str, now: DateTime<Utc>) -> Result<usize> {
    let alerts = pending_alerts(pool, now).await?;
    if alerts.is_empty() {
        return Ok(0);
    }
    for alert in &alerts {
        let report = alert_report(alert);
        let obligation = format!("{OBLIGATION_PREFIX}{}", alert.item_id);
        if let Some(anchor) = coalesce_target(pool, owner_session, alert, now).await? {
            if coalesce(pool, &anchor, alert, &report, &obligation).await? {
                continue;
            }
        }
        let brief = if alert.source == "mail" {
            crate::mail_brief::for_item(&alert.item_id).await
        } else {
            None
        };
        let text = alert_text(alert, brief.as_ref());
        let (_, outbox_id) = crate::report_gate::queue_delivery(
            pool, owner_session, &report, false, &text, Some(&obligation),
        ).await?;
        if let Some(outbox_id) = outbox_id {
            post_queued(pool, outbox_id, owner_session, &text).await?;
        }
    }
    Ok(alerts.len())
}

/// The alert a new message of a conversation is added to.
struct Anchor {
    outbox_id: i64,
    session: String,
    text: String,
    channel: Option<String>,
    ts: Option<String>,
    delivered: bool,
    first_sender: Option<String>,
}

type AnchorRow = (i64, String, String, Option<String>, Option<String>, bool, Option<String>);

/// The owner alert of the same conversation (source and thread) queued for this owner session
/// less than [`COALESCE_WINDOW_MINUTES`] ago, while none of the items it carries was settled
/// (the owner answered in the room, or approved it) and it has room for another line.
async fn coalesce_target(
    pool: &SqlitePool,
    owner_session: &str,
    alert: &PendingAlert,
    now: DateTime<Utc>,
) -> Result<Option<Anchor>> {
    let thread: Option<String> = sqlx::query_scalar("SELECT thread_ref FROM inbox_items WHERE id = ?")
        .bind(&alert.item_id)
        .fetch_optional(pool)
        .await?
        .flatten();
    let Some(thread) = thread.filter(|thread| !thread.trim().is_empty()) else {
        return Ok(None);
    };
    let since = (now - Duration::minutes(COALESCE_WINDOW_MINUTES)).to_rfc3339();
    let row: Option<AnchorRow> = sqlx::query_as(
        "SELECT o.id, o.session_key, o.text, o.posted_channel, o.posted_ts, o.delivered_at IS NOT NULL,
                json_extract((SELECT q.report_json FROM report_delivery_parts q
                               WHERE q.outbox_id = o.id ORDER BY q.rowid LIMIT 1), '$.sender')
           FROM inbox_items i
           JOIN delivery_obligations b ON b.id = ? || i.id
           JOIN report_delivery_parts p ON p.report_id = b.report_id
           JOIN chat_outbox o ON o.id = p.outbox_id
          WHERE i.source = ? AND i.thread_ref = ? AND i.id != ?
            AND o.session_key = ? AND o.cancelled_at IS NULL AND o.report_not_before IS NULL
            AND julianday(o.created_at) >= julianday(?)
            AND NOT EXISTS (
                SELECT 1 FROM report_delivery_parts q
                  JOIN delivery_obligations c ON c.report_id = q.report_id
                  JOIN inbox_decisions d ON c.id = ? || d.item_id
                 WHERE q.outbox_id = o.id AND d.status NOT IN ('open', 'dismissed'))
          ORDER BY o.id DESC LIMIT 1",
    )
    .bind(OBLIGATION_PREFIX)
    .bind(&alert.source)
    .bind(&thread)
    .bind(&alert.item_id)
    .bind(owner_session)
    .bind(since)
    .bind(OBLIGATION_PREFIX)
    .fetch_optional(pool)
    .await?;
    Ok(row
        .map(|(outbox_id, session, text, channel, ts, delivered, first_sender)| Anchor {
            outbox_id, session, text, channel, ts, delivered, first_sender,
        })
        .filter(|anchor| anchor.text.lines().count() < MAX_COALESCED_LINES))
}

/// Adds `alert` to the conversation's `anchor` alert: a posted alert is edited in Slack, a
/// queued one grows before it is sent. Returns true when it was added or the gate held it back;
/// false when it could not be added (too long, the edit failed, a sender took the row), and
/// the caller posts it as an alert of its own.
async fn coalesce(
    pool: &SqlitePool,
    anchor: &Anchor,
    alert: &PendingAlert,
    report: &crate::report_gate::Report,
    obligation: &str,
) -> Result<bool> {
    let text = format!("{}\n{}", anchor.text, continuation_line(alert, anchor.first_sender.as_deref()));
    if crate::chat::slack::outbox_chunks(&text).len() != 1 {
        return Ok(false);
    }
    let stamp = crate::storage::now_rfc3339();
    if crate::report_gate::submit(pool, report).await? != crate::report_gate::Decision::Sent {
        sqlx::query(
            "INSERT INTO delivery_obligations(id, state, body, updated_at, report_id)
             VALUES(?, 'suppressed', ?, ?, ?) ON CONFLICT(id) DO NOTHING",
        )
        .bind(obligation).bind(&text).bind(&stamp).bind(&report.id)
        .execute(pool).await?;
        return Ok(true);
    }
    let edited = anchor.delivered
        && edit_posted(&anchor.session, anchor.channel.as_deref(), anchor.ts.as_deref(), &text).await;
    let mut tx = pool.begin_write().await?;
    let updated = if !anchor.delivered {
        // Not posted yet: the row grows only while no sender holds it.
        sqlx::query(
            "UPDATE chat_outbox SET text = ?
              WHERE id = ? AND delivered_at IS NULL AND cancelled_at IS NULL AND send_chunks_done = 0
                AND send_claim IS NULL AND send_inflight IS NULL AND send_uncertain_at IS NULL",
        )
        .bind(&text).bind(anchor.outbox_id)
        .execute(&mut *tx).await?.rows_affected()
    } else if edited {
        sqlx::query("UPDATE chat_outbox SET text = ? WHERE id = ?")
            .bind(&text).bind(anchor.outbox_id)
            .execute(&mut *tx).await?.rows_affected()
    } else {
        0
    };
    if updated != 1 {
        tx.rollback().await?;
        // Release the topic state the gate just recorded, so the item's own alert is admitted.
        sqlx::query("DELETE FROM report_topics WHERE topic_key = ? AND last_state_hash = ?")
            .bind(&report.topic_key).bind(&report.state_hash)
            .execute(pool).await?;
        return Ok(false);
    }
    let report_json = serde_json::to_string(report)
        .map_err(|error| crate::HubError::message(error.to_string()))?;
    sqlx::query("INSERT OR IGNORE INTO report_delivery_parts(outbox_id, report_id, report_json) VALUES(?, ?, ?)")
        .bind(anchor.outbox_id).bind(&report.id).bind(report_json)
        .execute(&mut *tx).await?;
    sqlx::query(
        "INSERT INTO delivery_obligations(id, state, body, updated_at, report_id)
         VALUES(?, 'pending', ?, ?, ?) ON CONFLICT(id) DO NOTHING",
    )
    .bind(obligation).bind(&text).bind(&stamp).bind(&report.id)
    .execute(&mut *tx).await?;
    tx.commit().await?;
    let alert_ref = [report_alert(report)];
    match (anchor.delivered, anchor.channel.as_deref(), anchor.ts.as_deref()) {
        (true, Some(channel), Some(ts)) => {
            crate::report_feedback::remember_delivered(pool, channel, ts, &alert_ref).await?;
        }
        _ => {
            crate::report_feedback::queue_for_outbox(pool, anchor.outbox_id, &alert_ref).await?;
        }
    }
    reconcile(pool).await?;
    Ok(true)
}

/// Replaces the text of the alert posted as `channel`/`ts` for `session`. False when it is not
/// a Slack post or Slack refused the edit.
#[cfg(feature = "hub-slack")]
async fn edit_posted(session: &str, channel: Option<&str>, ts: Option<&str>, text: &str) -> bool {
    let slack = crate::chat::session::SessionKey::from_storage_key(session)
        .is_ok_and(|key| key.platform.eq_ignore_ascii_case("slack"));
    let (true, Some(channel), Some(ts)) = (slack, channel, ts) else {
        return false;
    };
    let Some(chunk) = crate::chat::slack::outbox_chunks(text).into_iter().next() else {
        return false;
    };
    match crate::chat::slack_web::update_message(channel, ts, &chunk).await {
        Ok(()) => true,
        Err(error) => {
            tracing::warn!(%error, "owner alert edit failed; the message gets its own alert");
            false
        }
    }
}

#[cfg(not(feature = "hub-slack"))]
async fn edit_posted(_session: &str, _channel: Option<&str>, _ts: Option<&str>, _text: &str) -> bool {
    false
}

/// omo's `OwnerAlerter::send_owner_alert`: one line to the owner's DM through the report
/// gate. The text is the topic, so the same line is said once. Returns true when it was sent
/// and a transport took it.
pub async fn notify_owner(pool: &SqlitePool, owner_session: &str, text: &str) -> Result<bool> {
    let report = crate::report_gate::Report::new(
        uuid::Uuid::new_v4().to_string(),
        "hub",
        format!("hub:{}", crate::report_gate::state_hash(text)),
        crate::report_gate::Salience::Action,
        text,
    );
    notify_report(pool, owner_session, &report).await
}

/// Submits a report to the gate and, only when it is `Sent`, posts its body to the owner's
/// DM, kept in the outbox so the retry sends it if no transport takes it now.
pub async fn notify_report(
    pool: &SqlitePool,
    owner_session: &str,
    report: &crate::report_gate::Report,
) -> Result<bool> {
    let (_, outbox_id) = crate::report_gate::queue(pool, owner_session, report, false).await?;
    let Some(outbox_id) = outbox_id else {
        return Ok(false);
    };
    let posted =
        crate::chat::reply::deliver_stored_posted(pool, outbox_id, owner_session, &report.body)
            .await?;
    if let Some(posted) = &posted {
        crate::report_feedback::posted_from_outbox(pool, outbox_id, posted).await?;
    }
    Ok(posted.is_some())
}

/// The alert a gate report stands for, so an owner reply to it can name its kind.
pub fn report_alert(report: &crate::report_gate::Report) -> crate::report_feedback::AlertRef {
    crate::report_feedback::AlertRef {
        source: report.source.clone(),
        sender: report.sender.clone().filter(|sender| !sender.trim().is_empty()),
        subject: Some(report.quote.clone().unwrap_or_else(|| report.body.clone())),
        topic_key: Some(report.topic_key.clone()),
        alert_ref: report.id.clone(),
    }
}

/// Posts one queued owner-alert outbox row now. On a post its alert is remembered by the
/// Slack message and the obligation is reconciled; otherwise the outbox retry posts it later
/// and remembers it then.
pub(crate) async fn post_queued(pool: &SqlitePool, outbox_id: i64, owner_session: &str, text: &str) -> Result<()> {
    match crate::chat::reply::deliver_stored_posted(pool, outbox_id, owner_session, text).await {
        Ok(Some(posted)) => {
            crate::report_feedback::posted_from_outbox(pool, outbox_id, &posted).await?;
            reconcile(pool).await?;
        }
        Ok(None) => tracing::warn!("owner alert is queued; no transport took it yet"),
        Err(error) => {
            tracing::warn!(%error, "owner alert post failed; the outbox retry will send it")
        }
    }
    Ok(())
}

/// omo's Full Disk Access owner DM: once per hub binary identity (`identity`, e.g. size and
/// mtime), recorded in one marker file so a rebuilt binary that lost the grant alerts again
/// and markers never accumulate. Returns true when it alerted.
pub async fn alert_missing_full_disk_access(
    pool: &SqlitePool,
    owner_session: &str,
    denied: bool,
    marker: &std::path::Path,
    identity: &str,
) -> Result<bool> {
    if !denied {
        return Ok(false);
    }
    if tokio::fs::read_to_string(marker).await.ok().as_deref() == Some(identity) {
        return Ok(false);
    }
    // One topic whose state is the binary identity: a rebuilt binary is a new state.
    let report = crate::report_gate::Report::new(
        uuid::Uuid::new_v4().to_string(),
        "hub",
        "hub:full-disk-access",
        crate::report_gate::Salience::Action,
        crate::locale::text(
            "⚠️ Ferryx 허브에 전체 디스크 접근(Full Disk Access) 권한이 없어 iMessage·메일·Safari 데이터를 읽지 못합니다. 시스템 설정 → 개인정보 보호 및 보안 → 전체 디스크 접근에서 Ferryx를 켠 뒤 다시 시작해 주세요.",
            "⚠️ The Ferryx hub has no Full Disk Access, so it cannot read iMessage, Mail or Safari data. Turn Ferryx on in System Settings → Privacy & Security → Full Disk Access, then restart it.",
        ),
    )
    .with_state(&format!("denied:{identity}"));
    notify_report(pool, owner_session, &report).await?;
    if let Some(parent) = marker.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    if let Err(error) = tokio::fs::write(marker, identity).await {
        tracing::warn!(%error, "full disk access alert marker was not written");
    }
    Ok(true)
}

/// Marks owner-alert obligations delivered once their outbox line went out.
pub async fn reconcile(pool: &SqlitePool) -> Result<u64> {
    let updated = sqlx::query(
        "UPDATE delivery_obligations SET state = 'delivered', updated_at = ?
          WHERE id LIKE ? || '%'
            AND state = 'pending'
            AND EXISTS (
                SELECT 1 FROM chat_outbox o
                WHERE o.delivered_at IS NOT NULL AND o.cancelled_at IS NULL
                  AND ((delivery_obligations.report_id IS NOT NULL AND EXISTS (
                      SELECT 1 FROM report_delivery_parts p
                      WHERE p.outbox_id = o.id AND p.report_id = delivery_obligations.report_id))
                    OR (delivery_obligations.report_id IS NULL AND o.text = delivery_obligations.body)))",
    )
    .bind(crate::storage::now_rfc3339())
    .bind(OBLIGATION_PREFIX)
    .execute(pool)
    .await?;
    Ok(updated.rows_affected())
}

/// Earlier builds queued owner alerts under the lead lane name ("meta"), which no transport
/// delivers, and marked them delivered anyway. Recent such lines are re-addressed to the
/// owner session and their obligations reopened, so the outbox retry sends them.
pub async fn repair_misrouted(
    pool: &SqlitePool,
    owner_session: &str,
    now: DateTime<Utc>,
) -> Result<u64> {
    let since = (now - Duration::hours(MAX_ALERT_AGE_HOURS)).to_rfc3339();
    let rows: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT id, session_key, text FROM chat_outbox
          WHERE delivered_at IS NULL AND cancelled_at IS NULL AND julianday(created_at) >= julianday(?)
            AND text IN (SELECT body FROM delivery_obligations WHERE id LIKE ? || '%')",
    )
    .bind(&since)
    .bind(OBLIGATION_PREFIX)
    .fetch_all(pool)
    .await?;
    let mut repaired = 0u64;
    for (id, key, text) in rows {
        if crate::chat::session::SessionKey::from_storage_key(&key).is_ok() {
            continue;
        }
        sqlx::query("UPDATE chat_outbox SET session_key = ? WHERE id = ?")
            .bind(owner_session)
            .bind(id)
            .execute(pool)
            .await?;
        sqlx::query(
            "UPDATE delivery_obligations SET state = 'pending', updated_at = ? WHERE id LIKE ? || '%' AND body = ? AND state = 'delivered'",
        )
        .bind(now.to_rfc3339())
        .bind(OBLIGATION_PREFIX)
        .bind(&text)
        .execute(pool)
        .await?;
        repaired += 1;
    }
    Ok(repaired)
}

/// Keeps the outbox moving for the life of the hub: re-addresses misrouted owner alerts once,
/// then every minute retries stored replies a transport has not taken yet (a Slack outage, a
/// token that arrived late) and marks owner alerts delivered once their line went out.
pub async fn serve_delivery(pool: SqlitePool, owner_session: String) {
    match repair_misrouted(&pool, &owner_session, Utc::now()).await {
        Ok(0) => {}
        Ok(count) => tracing::info!(count, "owner alerts re-addressed to the owner session"),
        Err(error) => tracing::warn!(%error, "owner alert repair failed"),
    }
    loop {
        delivery_tick(&pool, &owner_session).await;
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    }
}

pub(crate) async fn delivery_tick(pool: &SqlitePool, owner_session: &str) {
    let digest_session = match crate::import::imported_slack_home(pool).await {
        Ok(Some(channel)) => crate::chat::session::SessionKey::new(
            "slack", None::<String>, channel, None::<String>, "owner",
        ).storage_key(),
        Ok(None) => owner_session.to_owned(),
        Err(error) => {
            tracing::warn!(%error, "digest home unavailable; using owner session");
            owner_session.to_owned()
        }
    };
    if let Err(error) = crate::report_gate::queue_digest(pool, &digest_session, Utc::now()).await {
        tracing::warn!(%error, "report digest was not queued");
    }
    match crate::chat::reply::flush_stale(
        pool,
        crate::chat::reply::OUTBOX_SETTLE,
        Duration::hours(MAX_ALERT_AGE_HOURS),
    )
    .await
    {
        Ok(0) => {}
        Ok(count) => tracing::info!(count, "stored chat replies delivered"),
        Err(error) => tracing::warn!(%error, "chat outbox retry failed"),
    }
    if let Err(error) = reconcile(pool).await {
        tracing::warn!(%error, "owner alert reconcile failed");
    }
    if let Err(error) = crate::report_mutation::drain_deferred(pool).await {
        tracing::warn!(%error, "deferred attachments could not be drained");
    }
    if let Err(error) = crate::report_mutation::reconcile_cards(pool).await {
        tracing::warn!(%error, "report cards could not be reconciled");
    }
}

pub async fn delivered(pool: &SqlitePool, item_id: &str) -> Result<bool> {
    let state: Option<String> =
        sqlx::query_scalar("SELECT state FROM delivery_obligations WHERE id = ?")
            .bind(format!("{OBLIGATION_PREFIX}{item_id}"))
            .fetch_optional(pool)
            .await?;
    Ok(state.as_deref() == Some("delivered"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn identical_text_does_not_complete_another_reports_obligation() {
        // Given two distinct inbox items rendered as identical notification text.
        let pool = crate::storage::memory().await.unwrap();
        let text = "확인이 필요한 연락입니다.";
        let mut ids = Vec::new();
        for item in ["one", "two"] {
            let obligation = format!("{OBLIGATION_PREFIX}{item}");
            let report = crate::report_gate::Report::new(
                &obligation, "mail", format!("inbox:{item}"),
                crate::report_gate::Salience::Action, text,
            );
            ids.push(crate::report_gate::queue_delivery(
                &pool, "owner", &report, false, text, Some(&obligation),
            ).await.unwrap().1.unwrap());
        }
        crate::chat::reply::mark_delivered(&pool, ids[0]).await.unwrap();

        // When only the first item's transport receipt is reconciled.
        assert_eq!(reconcile(&pool).await.unwrap(), 1);

        // Then the unsent item remains pending despite its identical rendered body.
        assert!(delivered(&pool, "one").await.unwrap());
        assert!(!delivered(&pool, "two").await.unwrap());
        let state: String = sqlx::query_scalar("SELECT state FROM delivery_obligations WHERE id = ?")
            .bind(format!("{OBLIGATION_PREFIX}two")).fetch_one(&pool).await.unwrap();
        assert_eq!(state, "pending");
    }

    #[tokio::test]
    async fn an_owner_mail_item_is_alerted_once() {
        let pool = crate::storage::memory().await.unwrap();
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO inbox_items(id, source, source_ref, author, title, snippet, received_at, link, raw_hash, collected_at)
             VALUES('mail-1','mail','ref','Ada','Quote','please sign',?,?, 'h', ?)",
        )
        .bind(now.to_rfc3339())
        .bind("https://mail.example/x")
        .bind(now.to_rfc3339())
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO inbox_decisions(item_id, class, reason, status, decided_at) VALUES('mail-1','owner','needs you','open',?)",
        )
        .bind(now.to_rfc3339())
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(sweep(&pool, "owner-dm", now).await.unwrap(), 1);
        assert!(
            !delivered(&pool, "mail-1").await.unwrap(),
            "no transport took it, so it is not delivered"
        );
        assert_eq!(sweep(&pool, "owner-dm", now).await.unwrap(), 0);
        sqlx::query("UPDATE chat_outbox SET delivered_at = 'x'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(reconcile(&pool).await.unwrap(), 1);
        assert!(delivered(&pool, "mail-1").await.unwrap());
        let text: String = sqlx::query_scalar("SELECT text FROM chat_outbox")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(text.contains("메일"));
        assert!(text.contains("Quote"));
    }

    #[test]
    fn a_mail_brief_adds_content_and_action_lines_under_the_subject() {
        let alert = PendingAlert {
            item_id: "mail:t1".into(),
            source: "mail".into(),
            author: Some("Ada <ada@x.example>".into()),
            title: Some("Quote".into()),
            snippet: "please sign".into(),
            link: Some("https://mail.example/t1".into()),
        };
        let brief = crate::mail_brief::Brief {
            summary: "견적서에 서명을 요청합니다.".into(),
            action: Some("금요일까지 서명해 회신하세요.".into()),
        };
        assert_eq!(
            alert_text(&alert, Some(&brief)),
            "📬 [메일] Ada <ada@x.example> — Quote\nhttps://mail.example/t1\n내용: 견적서에 서명을 요청합니다.\n할 일: 금요일까지 서명해 회신하세요."
        );
        assert_eq!(alert_text(&alert, None), alert_line(&alert));
        let nothing = crate::mail_brief::Brief { action: None, ..brief };
        assert_eq!(
            alert_text(&alert, Some(&nothing)),
            "📬 [메일] Ada <ada@x.example> — Quote\nhttps://mail.example/t1\n내용: 견적서에 서명을 요청합니다."
        );
    }

    #[test]
    fn an_english_owner_gets_english_labels() {
        crate::locale::with(crate::locale::OwnerLocale::english_utc(), || {
            let alert = PendingAlert {
                item_id: "kakao:9".into(), source: "kakao".into(), author: Some("Ann".into()),
                title: Some("Ann".into()), snippet: "Bring the straws".into(), link: None,
            };
            assert_eq!(alert_line(&alert), "📬 [KakaoTalk] Ann — Bring the straws");
            assert_eq!(source_label("imessage"), "SMS");
            assert_eq!(source_label("mail"), "Mail");
            assert_eq!(alert_report(&alert).body, "A KakaoTalk message that needs you");
            let brief = crate::mail_brief::Brief { summary: "A quote.".into(), action: Some("Sign it.".into()) };
            assert!(alert_text(&alert, Some(&brief)).ends_with("\nSummary: A quote.\nTo do: Sign it."));
        });
    }

    /// 2026-10-31: a kakao room named after its only member was alerted as
    /// "📬 [카톡] 여신님 — 여신님", with no message. The line is the message, never the name twice.
    #[test]
    fn a_chat_alert_shows_the_message_and_never_the_name_twice() {
        let alert = PendingAlert {
            item_id: "kakao:1".into(), source: "kakao".into(), author: Some("여신님😍".into()),
            title: Some("여신님😍".into()), snippet: "18페이지 내적부터 해주면 돼".into(), link: None,
        };
        assert_eq!(alert_line(&alert), "📬 [카톡] 여신님😍 — 18페이지 내적부터 해주면 돼");
        // A group room names the room and the sender; its title is not the message.
        let group = PendingAlert {
            title: Some("NESL 연말모임".into()), author: Some("강우용".into()),
            snippet: "저녁 만찬장은 어디인가요??".into(), ..alert.clone()
        };
        assert_eq!(alert_line(&group), "📬 [카톡] NESL 연말모임 · 강우용 — 저녁 만찬장은 어디인가요??");
        for empty in ["", " 여신님😍 ", "[이모티콘]", "사진", "(사진)", "https://x.example/a.png"] {
            let blank = PendingAlert { snippet: empty.into(), ..alert.clone() };
            assert_eq!(alert_report(&blank).quote.as_deref(), Some(""), "{empty:?}");
            assert_eq!(alert_line(&blank), "📬 [카톡] 여신님😍", "{empty:?}");
        }
    }

    async fn chat_item(pool: &SqlitePool, id: &str, thread: &str, snippet: &str, at: DateTime<Utc>) {
        sqlx::query(
            "INSERT INTO inbox_items(id, source, source_ref, thread_ref, author, title, snippet, received_at, link, raw_hash, collected_at)
             VALUES(?,'kakao',?,?,'여신님','여신님',?,?,NULL,'h',?)",
        )
        .bind(id).bind(id).bind(thread).bind(snippet)
        .bind(at.to_rfc3339()).bind(at.to_rfc3339())
        .execute(pool).await.unwrap();
        sqlx::query(
            "INSERT INTO inbox_decisions(item_id, class, reason, status, decided_at) VALUES(?,'owner','needs you','open',?)",
        )
        .bind(id).bind(at.to_rfc3339())
        .execute(pool).await.unwrap();
    }

    async fn live_outbox(pool: &SqlitePool) -> Vec<String> {
        sqlx::query_scalar("SELECT text FROM chat_outbox WHERE cancelled_at IS NULL ORDER BY id")
            .fetch_all(pool).await.unwrap()
    }

    /// 2026-10-31: one kakao conversation produced ten alerts within hours. Messages of a
    /// conversation inside the window join its unanswered alert; another conversation, an
    /// answered one, or a message after the window gets its own.
    #[tokio::test]
    async fn messages_of_one_conversation_join_its_recent_unanswered_alert() {
        let pool = crate::storage::memory().await.unwrap();
        let start = Utc::now() - Duration::minutes(90);
        chat_item(&pool, "k1", "room-a", "빨대 있으면 가져다주세요", start).await;
        assert_eq!(sweep(&pool, "owner-dm", start).await.unwrap(), 1);
        chat_item(&pool, "k2", "room-a", "[이모티콘]", start + Duration::minutes(2)).await;
        chat_item(&pool, "k3", "room-a", "잠옷도 먼저 주세요", start + Duration::minutes(5)).await;
        chat_item(&pool, "o1", "room-b", "다른 방 질문", start + Duration::minutes(5)).await;
        assert_eq!(sweep(&pool, "owner-dm", start + Duration::minutes(6)).await.unwrap(), 3);
        assert_eq!(
            live_outbox(&pool).await,
            vec![
                "📬 [카톡] 여신님 — 빨대 있으면 가져다주세요\n↳ 잠옷도 먼저 주세요".to_owned(),
                "📬 [카톡] 여신님 — 다른 방 질문".to_owned(),
            ]
        );
        // The owner answered room A in the room: its next message is a new alert.
        sqlx::query("UPDATE inbox_decisions SET status = 'done' WHERE item_id = 'k3'")
            .execute(&pool).await.unwrap();
        chat_item(&pool, "k4", "room-a", "고마워요", start + Duration::minutes(10)).await;
        assert_eq!(sweep(&pool, "owner-dm", start + Duration::minutes(10)).await.unwrap(), 1);
        assert_eq!(live_outbox(&pool).await.len(), 3);
        // Outside the window a message is a new alert too.
        sqlx::query("UPDATE chat_outbox SET created_at = ?")
            .bind((Utc::now() - Duration::minutes(COALESCE_WINDOW_MINUTES + 1)).to_rfc3339())
            .execute(&pool).await.unwrap();
        chat_item(&pool, "k5", "room-a", "언제 와요?", start + Duration::minutes(80)).await;
        assert_eq!(sweep(&pool, "owner-dm", Utc::now()).await.unwrap(), 1);
        assert_eq!(live_outbox(&pool).await.len(), 4);
        // Every item of a joined alert is delivered with it; an unreadable one never is.
        sqlx::query("UPDATE chat_outbox SET delivered_at = 'x'").execute(&pool).await.unwrap();
        reconcile(&pool).await.unwrap();
        assert!(delivered(&pool, "k1").await.unwrap());
        assert!(delivered(&pool, "k3").await.unwrap());
        assert!(!delivered(&pool, "k2").await.unwrap(), "an unreadable message is not alerted");
    }

    /// A conversation's alert already posted to Slack is edited in place with the new message.
    #[cfg(feature = "hub-slack")]
    #[tokio::test]
    async fn e2e_a_posted_conversation_alert_is_edited_not_reposted() {
        let (base, calls) = crate::chat::slack_web::mock::serve("B1").await;
        let pool = crate::storage::memory().await.unwrap();
        let now = Utc::now();
        let owner = crate::chat::session::SessionKey::new(
            "slack", None::<String>, "UOWNER", None::<String>, "UOWNER",
        );
        crate::chat::slack::with_test_api(base, "xoxb-test".into(), async {
            chat_item(&pool, "k1", "room-a", "빨대 있으면 가져다주세요", now).await;
            assert_eq!(sweep(&pool, &owner.storage_key(), now).await.unwrap(), 1);
            chat_item(&pool, "k2", "room-a", "잠옷도 먼저 주세요", now).await;
            assert_eq!(sweep(&pool, &owner.storage_key(), now).await.unwrap(), 1);
        })
        .await;
        let calls = calls.lock().unwrap().clone();
        let posts: Vec<_> = calls.iter().filter(|(method, _)| method == "chat.postMessage").collect();
        let edits: Vec<_> = calls.iter().filter(|(method, _)| method == "chat.update").collect();
        assert_eq!(posts.len(), 1, "{calls:?}");
        assert_eq!(edits.len(), 1, "{calls:?}");
        let posted_ts: String = sqlx::query_scalar("SELECT posted_ts FROM chat_outbox")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(edits[0].1["ts"], posted_ts.as_str());
        assert!(edits[0].1["text"].as_str().unwrap().ends_with("↳ 잠옷도 먼저 주세요"));
        assert!(delivered(&pool, "k2").await.unwrap());
        // A reply to the edited message names the conversation's alerts.
        let refs: Vec<String> = sqlx::query_scalar(
            "SELECT alert_ref FROM delivered_alerts WHERE slack_channel = 'UOWNER' AND slack_ts = ? ORDER BY alert_ref",
        )
        .bind(&posted_ts)
        .fetch_all(&pool).await.unwrap();
        assert_eq!(refs, vec![format!("{OBLIGATION_PREFIX}k1"), format!("{OBLIGATION_PREFIX}k2")]);
    }

    async fn owner_item(pool: &SqlitePool, id: &str, now: DateTime<Utc>) {
        sqlx::query(
            "INSERT INTO inbox_items(id, source, source_ref, author, title, snippet, received_at, link, raw_hash, collected_at)
             VALUES(?,'imessage',?,'Bob','','call me',?,NULL,'h',?)",
        )
        .bind(id)
        .bind(format!("ref-{id}"))
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO inbox_decisions(item_id, class, reason, status, decided_at) VALUES(?,'owner','needs you','open',?)",
        )
        .bind(id)
        .bind(now.to_rfc3339())
        .execute(pool)
        .await
        .unwrap();
    }

    /// e2e with a mock Slack API: the alert is posted to the owner DM at once
    /// and only then counts as delivered.
    #[cfg(feature = "hub-slack")]
    #[tokio::test]
    async fn e2e_an_owner_alert_is_posted_to_slack_and_then_delivered() {
        let (base, calls) = crate::chat::slack_web::mock::serve("B1").await;
        let pool = crate::storage::memory().await.unwrap();
        let now = Utc::now();
        owner_item(&pool, "im-1", now).await;
        let owner = crate::chat::session::SessionKey::new(
            "slack",
            None::<String>,
            "UOWNER",
            None::<String>,
            "UOWNER",
        );
        crate::chat::slack::with_test_api(base, "xoxb-test".into(), async {
            assert_eq!(sweep(&pool, &owner.storage_key(), now).await.unwrap(), 1);
        })
        .await;
        let posted = calls.lock().unwrap().clone();
        assert_eq!(posted.len(), 1, "{posted:?}");
        assert_eq!(posted[0].0, "chat.postMessage");
        assert_eq!(posted[0].1["channel"], "UOWNER");
        assert!(posted[0].1["text"]
            .as_str()
            .unwrap()
            .contains("[문자] Bob — call me"));
        assert!(delivered(&pool, "im-1").await.unwrap());
        assert!(crate::chat::reply::pending(&pool).await.unwrap().is_empty());
    }

    #[cfg(feature = "hub-slack")]
    async fn delivered_alert_rows(pool: &SqlitePool) -> Vec<(String, String, String)> {
        sqlx::query_as(
            "SELECT slack_channel, slack_ts, alert_ref FROM delivered_alerts ORDER BY alert_ref",
        )
        .fetch_all(pool)
        .await
        .unwrap()
    }

    /// Two owner items are two Slack messages, each remembered by its own ts, so a reply to
    /// one names exactly that alert.
    #[cfg(feature = "hub-slack")]
    #[tokio::test]
    async fn each_owner_alert_is_its_own_message_and_is_remembered_by_its_ts() {
        let (base, calls) = crate::chat::slack_web::mock::serve("B1").await;
        let pool = crate::storage::memory().await.unwrap();
        let now = Utc::now();
        owner_item(&pool, "im-a", now).await;
        owner_item(&pool, "im-b", now).await;
        let owner = crate::chat::session::SessionKey::new(
            "slack",
            None::<String>,
            "UOWNER",
            None::<String>,
            "UOWNER",
        );
        crate::chat::slack::with_test_api(base, "xoxb-test".into(), async {
            assert_eq!(sweep(&pool, &owner.storage_key(), now).await.unwrap(), 2);
        })
        .await;
        let posted: Vec<String> = calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(method, _)| method == "chat.postMessage")
            .map(|(_, body)| body["text"].as_str().unwrap_or_default().to_owned())
            .collect();
        assert_eq!(posted.len(), 2, "{posted:?}");
        assert!(posted.iter().all(|text| !text.contains("건\n")), "{posted:?}");
        let rows = delivered_alert_rows(&pool).await;
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert!(rows.iter().all(|(channel, ts, _)| channel == "UOWNER" && ts.starts_with("1700000000.")));
        assert_ne!(rows[0].1, rows[1].1);
        let alert = crate::report_feedback::alert_for_thread(&pool, "UOWNER", &rows[0].1)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(alert.alert_ref, format!("{OBLIGATION_PREFIX}im-a"));
        assert_eq!(alert.topic_key.as_deref(), Some("inbox:im-a"));
        assert_eq!(alert.sender.as_deref(), Some("Bob"));
        assert!(delivered(&pool, "im-a").await.unwrap());
        assert!(delivered(&pool, "im-b").await.unwrap());
    }

    /// An alert no transport took at once is remembered when the outbox retry posts it.
    #[cfg(feature = "hub-slack")]
    #[tokio::test]
    async fn a_retried_owner_alert_is_remembered_when_the_retry_posts_it() {
        let pool = crate::storage::memory().await.unwrap();
        let now = Utc::now();
        owner_item(&pool, "im-r", now).await;
        let owner = crate::chat::session::SessionKey::new(
            "slack",
            None::<String>,
            "UOWNER",
            None::<String>,
            "UOWNER",
        );
        assert_eq!(sweep(&pool, &owner.storage_key(), now).await.unwrap(), 1);
        let queued = delivered_alert_rows(&pool).await;
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].0, "outbox");
        sqlx::query("UPDATE chat_outbox SET created_at = ? WHERE delivered_at IS NULL")
            .bind((now - Duration::hours(MAX_ALERT_AGE_HOURS + 1)).to_rfc3339())
            .execute(&pool).await.unwrap();
        let (base, _calls) = crate::chat::slack_web::mock::serve("B1").await;
        crate::chat::slack::with_test_api(base, "xoxb-test".into(), async {
            let sent = crate::chat::reply::flush_stale(
                &pool,
                chrono::Duration::zero(),
                Duration::hours(MAX_ALERT_AGE_HOURS),
            )
            .await
            .unwrap();
            assert_eq!(sent, 1);
        })
        .await;
        let rows = delivered_alert_rows(&pool).await;
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0].0, "UOWNER");
        assert_eq!(rows[0].2, format!("{OBLIGATION_PREFIX}im-r"));
    }

    #[tokio::test]
    async fn a_hub_report_is_queued_with_its_alert_ref() {
        let pool = crate::storage::memory().await.unwrap();
        let report = crate::report_gate::Report::new(
            "rep-1",
            "cron",
            "cron:nightly",
            crate::report_gate::Salience::Action,
            "야간 백업이 실패했습니다. 디스크가 가득 찼습니다.",
        );
        // No transport: not posted, but its alert waits on the outbox row.
        assert!(!notify_report(&pool, "owner-dm", &report).await.unwrap());
        let row: (String, String, Option<String>) = sqlx::query_as(
            "SELECT source, alert_ref, topic_key FROM delivered_alerts WHERE slack_channel = 'outbox'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row, ("cron".into(), "rep-1".into(), Some("cron:nightly".into())));
    }

    #[tokio::test]
    async fn an_alert_that_only_repeats_its_sender_is_empty() {
        let mut alert = PendingAlert {
            item_id: "kakao:1".into(), source: "kakao".into(), author: Some("여신님😍".into()),
            title: Some("여신님😍".into()), snippet: " 여신님😍 ".into(), link: None,
        };
        let pool = crate::storage::memory().await.unwrap();
        assert_eq!(crate::report_gate::submit(&pool, &alert_report(&alert)).await.unwrap(),
            crate::report_gate::Decision::Empty);
        alert.snippet = "빨대 있으면 가져다주세요".into();
        assert_eq!(alert_report(&alert).quote.as_deref(), Some("빨대 있으면 가져다주세요"));
    }

    #[tokio::test]
    async fn alerts_queued_under_the_lead_lane_are_readdressed_and_reopened() {
        let pool = crate::storage::memory().await.unwrap();
        let now = Utc::now();
        owner_item(&pool, "im-2", now).await;
        // What the old build did: queue under "meta" and call it delivered.
        assert_eq!(sweep(&pool, "meta", now).await.unwrap(), 1);
        sqlx::query("UPDATE delivery_obligations SET state = 'delivered'")
            .execute(&pool)
            .await
            .unwrap();
        let owner = crate::chat::session::SessionKey::new(
            "slack",
            None::<String>,
            "CHOME",
            None::<String>,
            "UOWNER",
        );
        assert_eq!(
            repair_misrouted(&pool, &owner.storage_key(), now)
                .await
                .unwrap(),
            1
        );
        assert!(!delivered(&pool, "im-2").await.unwrap());
        let queued = crate::chat::reply::pending(&pool).await.unwrap();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].1, owner.storage_key());
        // A second repair finds nothing left to move.
        assert_eq!(
            repair_misrouted(&pool, &owner.storage_key(), now)
                .await
                .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn missing_full_disk_access_alerts_the_owner_once_per_binary() {
        let pool = crate::storage::memory().await.unwrap();
        let dir = std::env::temp_dir().join(format!("ferryx-fda-{}", std::process::id()));
        let marker = dir.join("hub").join("fda-dm");
        let outbox = || async {
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM chat_outbox WHERE session_key = 'slack:owner' AND text LIKE '%Full Disk Access%'",
            )
            .fetch_one(&pool)
            .await
            .unwrap()
        };
        assert!(
            !alert_missing_full_disk_access(&pool, "slack:owner", false, &marker, "a")
                .await
                .unwrap()
        );
        assert!(
            alert_missing_full_disk_access(&pool, "slack:owner", true, &marker, "a")
                .await
                .unwrap()
        );
        assert!(
            !alert_missing_full_disk_access(&pool, "slack:owner", true, &marker, "a")
                .await
                .unwrap()
        );
        assert_eq!(outbox().await, 1);
        // A rebuilt binary loses the grant, so it alerts again.
        assert!(
            alert_missing_full_disk_access(&pool, "slack:owner", true, &marker, "b")
                .await
                .unwrap()
        );
        assert_eq!(outbox().await, 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_bare_kakao_image_is_not_alerted_and_not_resubmitted() {
        let pool = crate::storage::memory().await.unwrap();
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO inbox_items(id, source, source_ref, author, title, snippet, received_at, link, raw_hash, collected_at)
             VALUES('ka-1','kakao','ref','여신님','','[사진]',?,NULL,'h',?)",
        )
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO inbox_decisions(item_id, class, reason, status, decided_at) VALUES('ka-1','owner','needs you','open',?)",
        )
        .bind(now.to_rfc3339())
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(sweep(&pool, "owner-dm", now).await.unwrap(), 1);
        assert_eq!(sweep(&pool, "owner-dm", now).await.unwrap(), 0);
        let queued: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chat_outbox")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(queued, 0);
        let decision: String = sqlx::query_scalar("SELECT decision FROM report_suppressions")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(decision, "empty");
    }

    #[test]
    fn sender_only_title_uses_the_message_for_display_and_gate() {
        let alert = PendingAlert {
            item_id: "kakao:1".into(),
            source: "kakao".into(),
            author: Some("가족".into()),
            title: Some("가족".into()),
            snippet: "우산을 가져다 주세요".into(),
            link: None,
        };
        assert!(alert_line(&alert).ends_with(&alert.snippet));
        assert_eq!(alert_report(&alert).quote.as_deref(), Some(alert.snippet.as_str()));
        let image = PendingAlert { snippet: "[사진]".into(), ..alert };
        assert_eq!(crate::report_gate::decide(&alert_report(&image), None, None, false),
            crate::report_gate::Decision::Empty);
    }
}
