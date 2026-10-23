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

pub fn alert_line(alert: &PendingAlert) -> String {
    let label = match alert.source.as_str() {
        "mail" => "메일",
        "kakao" => "카톡",
        "imessage" => "문자",
        "slack" => "슬랙",
        "discord" => "디스코드",
        "github" => "GitHub",
        other => other,
    };
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

fn one_line(text: &str, max_chars: usize) -> String {
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
            AND i.source IN ('mail','imessage','slack','discord','github')
            AND julianday(d.decided_at) >= julianday(?)
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
    let lines: Vec<String> = alerts.iter().map(alert_line).collect();
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
    for alert in &alerts {
        sqlx::query(
            "INSERT INTO delivery_obligations(id, state, body, updated_at) VALUES(?, 'delivered', ?, ?)
             ON CONFLICT(id) DO UPDATE SET state = 'delivered', body = excluded.body, updated_at = excluded.updated_at",
        )
        .bind(format!("{OBLIGATION_PREFIX}{}", alert.item_id))
        .bind(&content)
        .bind(&stamp)
        .execute(pool)
        .await?;
    }
    sqlx::query(
        "INSERT INTO chat_outbox(session_key, platform, text, created_at) VALUES(?, 'slack', ?, ?)",
    )
    .bind(owner_session)
    .bind(&content)
    .bind(&stamp)
    .execute(pool)
    .await?;
    Ok(alerts.len())
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
        assert!(delivered(&pool, "mail-1").await.unwrap());
        assert_eq!(sweep(&pool, "owner-dm", now).await.unwrap(), 0);
        let text: String = sqlx::query_scalar("SELECT text FROM chat_outbox")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(text.contains("메일"));
        assert!(text.contains("Quote"));
    }
}
