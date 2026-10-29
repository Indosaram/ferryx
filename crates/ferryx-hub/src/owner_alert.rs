//! Owner alerts that do not depend on an agent turn.
//! A triage verdict that needs the owner is one plain line, recorded as a delivery obligation.
use chrono::{DateTime, Duration, Utc};
use sqlx::SqlitePool;

use crate::error::Result;

pub const OBLIGATION_PREFIX: &str = "obl:owner-alert:";
pub const MAX_ALERT_AGE_HOURS: i64 = 24;
pub const MAX_ALERTS_PER_SWEEP: i64 = 10;

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
    match source {
        "mail" => "메일",
        "kakao" => "카톡",
        "imessage" => "문자",
        "slack" => "슬랙",
        "discord" => "디스코드",
        "github" => "GitHub",
        "cron" => "크론",
        other => other,
    }
}

pub fn alert_line(alert: &PendingAlert) -> String {
    let label = source_label(&alert.source);
    let who = one_line(alert.author.as_deref().unwrap_or("알 수 없음"), 60);
    let what = alert
        .title
        .as_deref()
        .filter(|title| !title.trim().is_empty())
        .map(|title| one_line(title, 90))
        .unwrap_or_else(|| one_line(&alert.snippet, 90));
    let mut line = format!("📬 [{label}] {who} — {what}");
    if let Some(link) = alert.link.as_deref().filter(|link| !link.trim().is_empty()) {
        line.push_str(&format!("\n{link}"));
    }
    line
}

/// The alert line with the mail brief, when there is one, as two short lines under the subject.
pub fn alert_text(alert: &PendingAlert, brief: Option<&crate::mail_brief::Brief>) -> String {
    let mut text = alert_line(alert);
    if let Some(brief) = brief {
        text.push_str(&format!(
            "\n내용: {}\n할 일: {}",
            one_line(&brief.summary, 300),
            one_line(&brief.action, 160)
        ));
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
                   AND o.state IN ('pending', 'attempting', 'delivered')
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

/// Writes one obligation per pending owner item and one outbox line. A later sweep does not repeat it.
pub async fn sweep(pool: &SqlitePool, owner_session: &str, now: DateTime<Utc>) -> Result<usize> {
    let alerts = pending_alerts(pool, now).await?;
    if alerts.is_empty() {
        return Ok(0);
    }
    let mut lines = Vec::with_capacity(alerts.len());
    for alert in &alerts {
        let brief = if alert.source == "mail" {
            crate::mail_brief::for_item(&alert.item_id).await
        } else {
            None
        };
        lines.push(alert_text(alert, brief.as_ref()));
    }
    let content = if lines.len() == 1 {
        lines[0].clone()
    } else {
        format!(
            "확인이 필요한 새 연락 {}건\n{}",
            lines.len(),
            lines.join("\n")
        )
    };
    let stamp = now.to_rfc3339();
    // `pending` until a transport takes the line; `delivered` only after it did, so the
    // action loop never records `executed` for an owner item the owner was not told about.
    for alert in &alerts {
        sqlx::query(
            "INSERT INTO delivery_obligations(id, state, body, updated_at) VALUES(?, 'pending', ?, ?)
             ON CONFLICT(id) DO UPDATE SET state = 'pending', body = excluded.body, updated_at = excluded.updated_at",
        )
        .bind(format!("{OBLIGATION_PREFIX}{}", alert.item_id))
        .bind(&content)
        .bind(&stamp)
        .execute(pool)
        .await?;
    }
    let inserted = sqlx::query(
        "INSERT INTO chat_outbox(session_key, platform, text, created_at) VALUES(?, 'slack', ?, ?)",
    )
    .bind(owner_session)
    .bind(&content)
    .bind(&stamp)
    .execute(pool)
    .await?;
    match crate::chat::reply::deliver_stored(
        pool,
        inserted.last_insert_rowid(),
        owner_session,
        &content,
    )
    .await
    {
        Ok(true) => {
            reconcile(pool).await?;
        }
        Ok(false) => tracing::warn!("owner alert is queued; no transport took it yet"),
        Err(error) => {
            tracing::warn!(%error, "owner alert post failed; the outbox retry will send it")
        }
    }
    Ok(alerts.len())
}

/// omo's `OwnerAlerter::send_owner_alert`: one line straight to the owner's DM, kept in the
/// outbox so the retry sends it if no transport takes it now.
pub async fn notify_owner(pool: &SqlitePool, owner_session: &str, text: &str) -> Result<bool> {
    let inserted = sqlx::query(
        "INSERT INTO chat_outbox(session_key, platform, text, created_at) VALUES(?, 'slack', ?, ?)",
    )
    .bind(owner_session)
    .bind(text)
    .bind(Utc::now().to_rfc3339())
    .execute(pool)
    .await?;
    crate::chat::reply::deliver_stored(pool, inserted.last_insert_rowid(), owner_session, text)
        .await
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
    notify_owner(
        pool,
        owner_session,
        "⚠️ Ferryx 허브에 전체 디스크 접근(Full Disk Access) 권한이 없어 iMessage·메일·Safari 데이터를 읽지 못합니다. 시스템 설정 → 개인정보 보호 및 보안 → 전체 디스크 접근에서 Ferryx를 켠 뒤 다시 시작해 주세요.",
    )
    .await?;
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
            AND EXISTS (SELECT 1 FROM chat_outbox o WHERE o.text = delivery_obligations.body AND o.delivered_at IS NOT NULL)",
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
          WHERE delivered_at IS NULL AND julianday(created_at) >= julianday(?)
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
        match crate::chat::reply::flush_stale(
            &pool,
            crate::chat::reply::OUTBOX_SETTLE,
            Duration::hours(MAX_ALERT_AGE_HOURS),
        )
        .await
        {
            Ok(0) => {}
            Ok(count) => tracing::info!(count, "stored chat replies delivered"),
            Err(error) => tracing::warn!(%error, "chat outbox retry failed"),
        }
        if let Err(error) = reconcile(&pool).await {
            tracing::warn!(%error, "owner alert reconcile failed");
        }
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
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
            action: "금요일까지 서명해 회신하세요.".into(),
        };
        assert_eq!(
            alert_text(&alert, Some(&brief)),
            "📬 [메일] Ada <ada@x.example> — Quote\nhttps://mail.example/t1\n내용: 견적서에 서명을 요청합니다.\n할 일: 금요일까지 서명해 회신하세요."
        );
        assert_eq!(alert_text(&alert, None), alert_line(&alert));
    }

    async fn owner_item(pool: &SqlitePool, id: &str, now: DateTime<Utc>) {
        sqlx::query(
            "INSERT INTO inbox_items(id, source, source_ref, author, title, snippet, received_at, link, raw_hash, collected_at)
             VALUES(?,'imessage','ref','Bob','','call me',?,NULL,'h',?)",
        )
        .bind(id)
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
}
