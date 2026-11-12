//! Durable Slack approval for inbox proposals. The card's buttons carry the
//! `action_proposals` row id (`omon:inbox:{id}:{choice}`), so a click is resolved from the
//! database alone: no in-memory waiter, so a click after a hub restart still works.
//!
//! Only the owner may answer. The pending -> approved/declined move is one compare-and-swap in
//! a write transaction, so a replayed or double click answers at most once. An approval hands
//! the item to the lead through the action loop's own path (one `action_loop` lane event and a
//! `proposal:{id}` outcome row, retried by the action loop like any handed-off item).
//! "Always" records an exact allow precedent and is refused for an effect that must be asked
//! every time (a tainted outward send).
use chrono::{DateTime, NaiveDateTime, Utc};
use serde_json::{json, Value};
use sqlx::SqlitePool;

use crate::effect::{requires_always_ask, Effect};
use crate::error::Result;
use crate::storage::{now_rfc3339, WriteTx};

pub const PREFIX: &str = "omon:inbox:";
/// The owner-facing click replies, (Korean, English); [`say`] picks the owner's language.
pub const NOT_AUTHORIZED: (&str, &str) = ("이 승인 요청에 답할 권한이 없습니다.", "You are not allowed to answer this approval request.");
pub const MALFORMED: (&str, &str) = ("승인 버튼 값을 읽을 수 없습니다.", "This approval button could not be read.");
pub const STALE: (&str, &str) = (
    "이 승인 요청은 이미 처리됐거나 만료됐어요. 다시 누르지 않으셔도 돼요.",
    "This approval request was already handled or has expired. No need to press it again.",
);
pub const EXPIRED: (&str, &str) = ("⌛ 항목이 이미 닫혀 승인 요청이 만료됐습니다.", "⌛ The item is already closed, so this approval request expired.");
pub const ALWAYS_REFUSED: (&str, &str) = (
    "외부로 나가는 발송은 매번 직접 승인해야 합니다. '승인'을 눌러 주세요.",
    "Anything sent outside must be approved each time. Please press 'Approve'.",
);
pub const NO_LEAD: (&str, &str) = (
    "이 승인 요청에는 작업을 넘길 리드가 기록되어 있지 않습니다.",
    "This approval request has no lead recorded to hand the task to.",
);

/// One of the texts above in the owner's language.
pub fn say(text: (&str, &str)) -> String {
    crate::locale::text(text.0, text.1)
}
const REASON_LIMIT: usize = 1500;
const COMMAND_LIMIT: usize = 2500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Once,
    Always,
    Deny,
}

impl Choice {
    fn parse(raw: &str) -> Option<Self> {
        Some(match raw {
            "once" => Self::Once,
            "always" => Self::Always,
            "deny" => Self::Deny,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Once => "once",
            Self::Always => "always",
            Self::Deny => "deny",
        }
    }

    fn label(self, user: &str) -> String {
        let label = match self {
            Self::Once => crate::locale::text("✅ 승인됨", "✅ Approved"),
            Self::Always => crate::locale::text("✅ 항상 승인됨", "✅ Always approved"),
            Self::Deny => crate::locale::text("❌ 거절됨", "❌ Declined"),
        };
        format!("{label} by <@{user}>")
    }
}

pub fn button_value(id: i64, choice: Choice) -> String {
    format!("{PREFIX}{id}:{}", choice.as_str())
}

pub fn parse_value(value: &str) -> Option<(i64, Choice)> {
    let (id, choice) = value.strip_prefix(PREFIX)?.split_once(':')?;
    let id: i64 = id.parse().ok().filter(|id| *id > 0)?;
    Some((id, Choice::parse(choice)?))
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn truncate(text: &str, limit: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= limit {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(limit).collect();
    out.push('…');
    out
}

/// The card for proposal `id`. `allow_always` is false for an effect that must be asked every
/// time, so no "always" button is offered for it.
pub fn card_blocks(id: i64, command: &str, reason: &str, allow_always: bool) -> Vec<Value> {
    let reason = escape(&truncate(reason, REASON_LIMIT));
    let command = escape(&truncate(command, COMMAND_LIMIT)).replace("```", "`\u{200b}``");
    let button = |label: &str, choice: Choice, style: Option<&str>| {
        let value = button_value(id, choice);
        let mut button = json!({
            "type": "button",
            "text": {"type": "plain_text", "text": label},
            "action_id": value,
            "value": value,
        });
        if let Some(style) = style {
            button["style"] = json!(style);
        }
        button
    };
    let locale = crate::locale::current();
    let mut elements = vec![button(locale.pick("승인", "Approve"), Choice::Once, Some("primary"))];
    if allow_always {
        elements.push(button(locale.pick("항상 승인", "Always approve"), Choice::Always, None));
    }
    elements.push(button(locale.pick("거절", "Decline"), Choice::Deny, Some("danger")));
    let heading = locale.pick("승인 필요", "Approval needed");
    vec![
        json!({"type": "section", "text": {"type": "mrkdwn", "text": format!("*{heading}*\n{reason}")}}),
        json!({"type": "section", "text": {"type": "mrkdwn", "text": format!("```\n{command}\n```")}}),
        json!({"type": "actions", "block_id": format!("{PREFIX}{id}"), "elements": elements}),
    ]
}

/// What one click did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    Approved { lane_event_id: i64 },
    Declined,
    /// The proposal was already answered (its current state). Nothing changed.
    AlreadyAnswered(String),
    /// "Always" on an effect that must be asked every time. Nothing changed.
    AlwaysRefused,
    /// The item was closed; the proposal is now `expired`.
    Expired,
    /// No inbox proposal with this id.
    Unknown,
    /// The proposal has no lead to hand the work to. Nothing changed.
    NoLead,
}

/// Feedback for the clicker: a line replacing the card's buttons, and/or an ephemeral note.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClickReply {
    pub card_line: Option<String>,
    pub ephemeral: Option<String>,
}

fn clicked(payload: &Value) -> Vec<&str> {
    payload
        .get("actions")
        .and_then(Value::as_array)
        .map(|actions| {
            actions
                .iter()
                .filter_map(|action| {
                    ["value", "action_id"]
                        .into_iter()
                        .filter_map(|key| action.get(key).and_then(Value::as_str))
                        .find(|value| value.starts_with(PREFIX))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A Slack `block_actions` payload. `None` when it carries no inbox approval button (the
/// click belongs to another handler). A click from anyone but `owner_id` changes nothing.
pub async fn handle_click(
    pool: &SqlitePool,
    payload: &Value,
    owner_id: &str,
) -> Result<Option<ClickReply>> {
    if payload.get("type").and_then(Value::as_str) != Some("block_actions") {
        return Ok(None);
    }
    let values = clicked(payload);
    if values.is_empty() {
        return Ok(None);
    }
    let user = payload
        .pointer("/user/id")
        .and_then(Value::as_str)
        .unwrap_or("");
    if user.is_empty() || owner_id.is_empty() || user != owner_id {
        tracing::warn!(user, "rejected inbox approval click from a non-owner");
        return Ok(Some(ClickReply {
            card_line: None,
            ephemeral: Some(say(NOT_AUTHORIZED)),
        }));
    }
    let Some((id, choice)) = values.iter().find_map(|value| parse_value(value)) else {
        return Ok(Some(ClickReply {
            card_line: None,
            ephemeral: Some(say(MALFORMED)),
        }));
    };
    let resolution = resolve(pool, id, choice, user).await?;
    tracing::info!(proposal = id, choice = choice.as_str(), ?resolution, "inbox approval click");
    Ok(Some(match resolution {
        Resolution::Approved { .. } | Resolution::Declined => ClickReply {
            card_line: Some(choice.label(user)),
            ephemeral: None,
        },
        Resolution::AlreadyAnswered(state) => ClickReply {
            card_line: None,
            ephemeral: Some(crate::locale::current().pick(
                &format!("이미 처리된 승인 요청입니다 ({state})."),
                &format!("This approval request was already handled ({state})."),
            ).to_owned()),
        },
        Resolution::AlwaysRefused => ClickReply {
            card_line: None,
            ephemeral: Some(say(ALWAYS_REFUSED)),
        },
        Resolution::Expired => ClickReply {
            card_line: Some(say(EXPIRED)),
            ephemeral: None,
        },
        Resolution::Unknown => ClickReply {
            card_line: Some(say(STALE)),
            ephemeral: None,
        },
        Resolution::NoLead => ClickReply {
            card_line: None,
            ephemeral: Some(say(NO_LEAD)),
        },
    }))
}

/// An inbox proposal still pending this many days after `created_at` (inclusive) expires.
pub const AGE_EXPIRY_DAYS: i64 = 30;
/// Cadence of [`run_age_sweep`]: an aged proposal stays pending at most this long unswept.
pub const AGE_SWEEP_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60 * 60);
/// The one age-expiry write, shared by the click path and the sweep so both land identically.
const EXPIRE_AGED_SQL: &str = "UPDATE action_proposals SET state = 'expired', decided_at = ?
  WHERE id = ? AND origin = 'inbox' AND state = 'pending'";

/// `created_at` is RFC 3339 (hub writes) or SQLite's `CURRENT_TIMESTAMP` (UTC, column default).
fn parse_created(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .map(|at| at.with_timezone(&Utc))
        .ok()
        .or_else(|| {
            NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S")
                .ok()
                .map(|at| at.and_utc())
        })
}

/// True when `created_at <= now - 30 days`; an unreadable stamp never ages out.
pub fn is_aged(created_at: &str, now: DateTime<Utc>) -> bool {
    parse_created(created_at)
        .is_some_and(|at| at <= now - chrono::Duration::days(AGE_EXPIRY_DAYS))
}

/// Expires every pending inbox proposal that has aged out at `now`. Idempotent: a second pass
/// (or a click that already expired the row) changes nothing. Creates no approval, precedent
/// or action-loop event; the item becomes eligible for a fresh proposal in `present_ready`.
pub async fn sweep_aged_proposals(pool: &SqlitePool, now: DateTime<Utc>) -> Result<u64> {
    let rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, created_at FROM action_proposals WHERE origin = 'inbox' AND state = 'pending'",
    )
    .fetch_all(pool)
    .await?;
    let stamp = now.to_rfc3339();
    let mut expired = 0;
    for (id, created_at) in rows {
        if !is_aged(&created_at, now) {
            continue;
        }
        expired += sqlx::query(EXPIRE_AGED_SQL)
            .bind(&stamp)
            .bind(id)
            .execute(pool)
            .await?
            .rows_affected();
    }
    Ok(expired)
}

/// Runs [`sweep_aged_proposals`] every [`AGE_SWEEP_INTERVAL`]; spawn once at hub start.
pub async fn run_age_sweep(pool: SqlitePool) {
    let mut tick = tokio::time::interval(AGE_SWEEP_INTERVAL);
    loop {
        tick.tick().await;
        if let Err(error) = sweep_aged_proposals(&pool, Utc::now()).await {
            tracing::warn!(%error, "inbox proposal age sweep failed");
        }
    }
}

#[derive(sqlx::FromRow)]
struct ProposalRow {
    state: String,
    created_at: String,
    effect: String,
    signature: String,
    target: String,
    tainted: i64,
    summary: String,
    payload_json: String,
}

/// The owner's answer to inbox proposal `id`, applied in one write transaction.
pub async fn resolve(pool: &SqlitePool, id: i64, choice: Choice, user: &str) -> Result<Resolution> {
    resolve_at(pool, id, choice, user, Utc::now()).await
}

/// [`resolve`] at a given clock, so the age cutoff is testable.
pub async fn resolve_at(
    pool: &SqlitePool,
    id: i64,
    choice: Choice,
    user: &str,
    now: DateTime<Utc>,
) -> Result<Resolution> {
    let mut tx = pool.begin_write().await?;
    let row: Option<ProposalRow> = sqlx::query_as(
        "SELECT state, created_at, effect, signature, target, tainted, summary, payload_json
           FROM action_proposals WHERE id = ? AND origin = 'inbox'",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(row) = row else {
        return Ok(Resolution::Unknown);
    };
    if row.state != "pending" {
        return Ok(Resolution::AlreadyAnswered(row.state));
    }
    // An aged proposal expires exactly as the sweep would (same write, same stamp), whatever
    // was clicked; this records nothing, so it precedes the Always refusal without weakening it.
    if is_aged(&row.created_at, now) {
        sqlx::query(EXPIRE_AGED_SQL)
            .bind(now.to_rfc3339())
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        return Ok(Resolution::Expired);
    }
    let effect: Effect = row.effect.parse().unwrap_or(Effect::Unknown);
    let tainted = row.tainted != 0;
    if choice == Choice::Always
        && (requires_always_ask(effect, tainted) || !effect.allows_any_precedent())
    {
        return Ok(Resolution::AlwaysRefused);
    }
    let payload: Value = serde_json::from_str(&row.payload_json).unwrap_or(Value::Null);
    let item_id = payload
        .get("item_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let item: Option<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT d.status, i.source, i.link FROM inbox_decisions d
           JOIN inbox_items i ON i.id = d.item_id WHERE d.item_id = ?",
    )
    .bind(&item_id)
    .fetch_optional(&mut *tx)
    .await?;
    let stamp = now_rfc3339();
    let open = item
        .as_ref()
        .is_some_and(|(status, _, _)| !crate::inbox_cards::card_is_stale(status));
    if !open {
        sqlx::query(
            "UPDATE action_proposals SET state = 'expired', decided_at = ? WHERE id = ? AND state = 'pending'",
        )
        .bind(&stamp)
        .bind(id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        return Ok(Resolution::Expired);
    }
    let owner_words = format!("slack:{user}:{}", choice.as_str());
    if choice == Choice::Deny {
        sqlx::query(
            "UPDATE action_proposals SET state = 'declined', owner_words = ?, decided_at = ?
              WHERE id = ? AND state = 'pending'",
        )
        .bind(&owner_words)
        .bind(&stamp)
        .bind(id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        return Ok(Resolution::Declined);
    }
    let Some(lead) = payload
        .get("lead")
        .and_then(Value::as_str)
        .filter(|lead| !lead.trim().is_empty())
        .map(str::to_owned)
    else {
        return Ok(Resolution::NoLead);
    };
    let claimed = sqlx::query(
        "UPDATE action_proposals SET state = 'approved', owner_words = ?, decided_at = ?
          WHERE id = ? AND state = 'pending'",
    )
    .bind(&owner_words)
    .bind(&stamp)
    .bind(id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if claimed != 1 {
        return Ok(Resolution::AlreadyAnswered("approved".into()));
    }
    if choice == Choice::Always {
        sqlx::query(
            "INSERT INTO action_precedents(signature, effect, target_scope, decision, scope, approvals, origin, owner_words, created_at, updated_at)
             VALUES(?, ?, ?, 'allow', 'exact', 1, 'owner', ?, ?, ?)
             ON CONFLICT(signature, scope) DO UPDATE SET decision = 'allow', revoked_at = NULL,
                 approvals = action_precedents.approvals + 1, owner_words = excluded.owner_words,
                 updated_at = excluded.updated_at",
        )
        .bind(&row.signature)
        .bind(effect.as_str())
        .bind(&row.target)
        .bind(&owner_words)
        .bind(&stamp)
        .bind(&stamp)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query(
        "UPDATE inbox_decisions SET status = 'approved' WHERE item_id = ? AND status IN ('open', 'proposed')",
    )
    .bind(&item_id)
    .execute(&mut *tx)
    .await?;
    let route = payload.get("action").map(Value::to_string).unwrap_or_default();
    let line = format!(
        "Owner-approved inbox task inbox-proposal:{id} (item inbox:{item_id}, route {route}, effect {})",
        effect.as_str()
    );
    let body = format!(
        "{line}\n{}",
        crate::chat::fence::fence_untrusted(
            &format!("inbox item {item_id} target={}", row.target),
            &row.summary,
        )
    );
    // The compare-and-swap above admits one approval, so this runs once per proposal.
    let lane_event_id = sqlx::query(
        "INSERT INTO lane_events(lane_key, lead_session_key, kind, body, created_at)
         VALUES(NULL, ?, 'action_loop', ?, ?)",
    )
    .bind(&lead)
    .bind(&body)
    .bind(&stamp)
    .execute(&mut *tx)
    .await?
    .last_insert_rowid();
    let (source, link) = item
        .map(|(_, source, link)| (source, link))
        .unwrap_or_else(|| ("inbox".into(), None));
    sqlx::query(
        "INSERT INTO action_loop_outcomes(item_key, lane_event_id, kind, source, line, link, status, attempts, enqueued_at, updated_at)
         VALUES(?, ?, 'inbox_bot', ?, ?, ?, 'enqueued', 1, ?, ?)
         ON CONFLICT(item_key) DO NOTHING",
    )
    .bind(format!("proposal:{id}"))
    .bind(lane_event_id)
    .bind(&source)
    .bind(&line)
    .bind(&link)
    .bind(&stamp)
    .bind(&stamp)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Resolution::Approved { lane_event_id })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEAD: &str = "lead-session";

    async fn seed(pool: &SqlitePool, item: &str, status: &str, effect: &str, signature: &str) -> i64 {
        sqlx::query(
            "INSERT INTO inbox_items(id, source, source_ref, author, title, snippet, link, received_at, raw_hash, collected_at)
             VALUES(?, 'kakao', ?, 'bob@example.com', 'notes', 'body', 'https://example.com/item', ?, ?, ?)",
        )
        .bind(item).bind(item).bind(now_rfc3339()).bind(item).bind(now_rfc3339())
        .execute(pool).await.unwrap();
        sqlx::query(
            "INSERT INTO inbox_decisions(item_id, class, reason, proposed_action, status, decided_at)
             VALUES(?, 'bot', 'bot_can_handle', 'prepare summary note', ?, ?)",
        )
        .bind(item).bind(status).bind(now_rfc3339())
        .execute(pool).await.unwrap();
        let payload = json!({"item_id": item, "action": {"kind": "draft"}, "lead": LEAD}).to_string();
        sqlx::query(
            "INSERT INTO action_proposals(origin, session_key, signature, effect, target, summary, payload_json, tainted, state, created_at)
             VALUES('inbox', 'C1', ?, ?, 'bob@example.com', 'prepare summary note', ?, 1, 'pending', ?)",
        )
        .bind(signature).bind(effect).bind(payload).bind(now_rfc3339())
        .execute(pool).await.unwrap()
        .last_insert_rowid()
    }

    fn click(user: &str, value: &str) -> Value {
        json!({"type": "block_actions", "user": {"id": user},
            "channel": {"id": "C1"}, "container": {"message_ts": "100.1"},
            "actions": [{"action_id": value, "value": value}]})
    }

    async fn state(pool: &SqlitePool, id: i64) -> String {
        sqlx::query_scalar("SELECT state FROM action_proposals WHERE id = ?")
            .bind(id).fetch_one(pool).await.unwrap()
    }

    async fn count(pool: &SqlitePool, sql: &str) -> i64 {
        sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
    }

    #[test]
    fn button_values_round_trip_and_a_tainted_send_has_no_always_button() {
        assert_eq!(parse_value(&button_value(7, Choice::Always)), Some((7, Choice::Always)));
        assert_eq!(parse_value("omon:inbox:0:once"), None);
        assert_eq!(parse_value("omon:inbox:7:session"), None);
        let values = |blocks: Vec<Value>| -> Vec<String> {
            blocks[2]["elements"].as_array().unwrap().iter()
                .map(|element| element["value"].as_str().unwrap().to_owned()).collect()
        };
        assert_eq!(values(card_blocks(7, "reply", "why", false)),
            vec!["omon:inbox:7:once".to_owned(), "omon:inbox:7:deny".to_owned()]);
        assert_eq!(values(card_blocks(7, "draft", "why", true)).len(), 3);
    }

    #[tokio::test]
    async fn an_owner_approval_hands_the_item_to_the_lead_once() {
        let pool = crate::storage::memory().await.unwrap();
        let id = seed(&pool, "item-a", "proposed", "draft", "draft:inbox_draft:bob@example.com").await;
        let action_loop = crate::autonomy::ActionLoop::new(pool.clone(), LEAD);
        assert!(action_loop.flush(chrono::Utc::now()).await.unwrap().is_none());
        let value = button_value(id, Choice::Once);
        let reply = handle_click(&pool, &click("UOWNER", &value), "UOWNER").await.unwrap().unwrap();
        assert!(reply.card_line.unwrap().contains("UOWNER"));
        assert_eq!(state(&pool, id).await, "approved");
        let replay = handle_click(&pool, &click("UOWNER", &value), "UOWNER").await.unwrap().unwrap();
        assert!(replay.card_line.is_none());
        assert!(replay.ephemeral.unwrap().contains("approved"));
        assert_eq!(count(&pool, "SELECT COUNT(*) FROM lane_events WHERE kind = 'action_loop' AND lead_session_key = 'lead-session'").await, 1);
        let (status, event): (String, i64) = sqlx::query_as(
            "SELECT status, lane_event_id FROM action_loop_outcomes WHERE item_key = ?")
            .bind(format!("proposal:{id}")).fetch_one(&pool).await.unwrap();
        assert_eq!(status, "enqueued");
        let body: String = sqlx::query_scalar("SELECT body FROM lane_events WHERE id = ?")
            .bind(event).fetch_one(&pool).await.unwrap();
        assert!(body.contains("```untrusted"));
        let decision: String = sqlx::query_scalar("SELECT status FROM inbox_decisions WHERE item_id = 'item-a'")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(decision, "approved");
        assert!(action_loop.flush(chrono::Utc::now()).await.unwrap().is_none());
        // The approved row carries no command, so the ledger executor never runs it as a shell command.
        assert_eq!(crate::autonomy::ActionLoop::new(pool.clone(), LEAD).execute_ready(true).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn a_stranger_click_changes_nothing() {
        let pool = crate::storage::memory().await.unwrap();
        let id = seed(&pool, "item-b", "proposed", "draft", "draft:inbox_draft:bob@example.com").await;
        let reply = handle_click(&pool, &click("USTRANGER", &button_value(id, Choice::Once)), "UOWNER")
            .await.unwrap().unwrap();
        assert_eq!(reply.ephemeral.as_deref(), Some(NOT_AUTHORIZED.0));
        let english = crate::locale::scope(crate::locale::OwnerLocale::english_utc(), async {
            handle_click(&pool, &click("USTRANGER", &button_value(id, Choice::Once)), "UOWNER")
                .await.unwrap().unwrap()
        }).await;
        assert_eq!(english.ephemeral.as_deref(), Some(NOT_AUTHORIZED.1));
        assert!(reply.card_line.is_none());
        assert_eq!(state(&pool, id).await, "pending");
        assert_eq!(count(&pool, "SELECT COUNT(*) FROM lane_events").await, 0);
        let other = json!({"type": "block_actions", "user": {"id": "UOWNER"},
            "actions": [{"value": "omon:approval:x:once"}]});
        assert!(handle_click(&pool, &other, "UOWNER").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn always_is_refused_for_a_tainted_outward_send_and_recorded_for_a_draft() {
        let pool = crate::storage::memory().await.unwrap();
        let send = seed(&pool, "item-send", "proposed", "external_send", "external_send:inbox_draft:bob@example.com").await;
        assert_eq!(resolve(&pool, send, Choice::Always, "UOWNER").await.unwrap(), Resolution::AlwaysRefused);
        assert_eq!(state(&pool, send).await, "pending");
        assert_eq!(count(&pool, "SELECT COUNT(*) FROM action_precedents").await, 0);
        assert!(matches!(resolve(&pool, send, Choice::Once, "UOWNER").await.unwrap(), Resolution::Approved { .. }));
        let draft = seed(&pool, "item-draft", "proposed", "draft", "draft:inbox_draft:carol@example.com").await;
        assert!(matches!(resolve(&pool, draft, Choice::Always, "UOWNER").await.unwrap(), Resolution::Approved { .. }));
        let (decision, scope): (String, String) = sqlx::query_as(
            "SELECT decision, scope FROM action_precedents WHERE signature = 'draft:inbox_draft:carol@example.com'")
            .fetch_one(&pool).await.unwrap();
        assert_eq!((decision.as_str(), scope.as_str()), ("allow", "exact"));
    }

    #[tokio::test]
    async fn deny_declines_and_a_closed_item_expires() {
        let pool = crate::storage::memory().await.unwrap();
        let denied = seed(&pool, "item-deny", "proposed", "draft", "draft:inbox_draft:bob@example.com").await;
        assert_eq!(resolve(&pool, denied, Choice::Deny, "UOWNER").await.unwrap(), Resolution::Declined);
        assert_eq!(state(&pool, denied).await, "declined");
        let closed = seed(&pool, "item-done", "done", "draft", "draft:inbox_draft:bob@example.com").await;
        assert_eq!(resolve(&pool, closed, Choice::Once, "UOWNER").await.unwrap(), Resolution::Expired);
        assert_eq!(state(&pool, closed).await, "expired");
        assert_eq!(resolve(&pool, 9999, Choice::Once, "UOWNER").await.unwrap(), Resolution::Unknown);
        assert_eq!(count(&pool, "SELECT COUNT(*) FROM lane_events").await, 0);
    }

    fn fixed_now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-11-12T00:00:00Z").unwrap().with_timezone(&Utc)
    }

    async fn age(pool: &SqlitePool, id: i64, by: chrono::Duration) {
        sqlx::query("UPDATE action_proposals SET created_at = ? WHERE id = ?")
            .bind((fixed_now() - by).to_rfc3339()).bind(id).execute(pool).await.unwrap();
    }

    async fn snapshot(pool: &SqlitePool, id: i64) -> (String, Option<String>, i64, i64, i64) {
        let (state, decided): (String, Option<String>) =
            sqlx::query_as("SELECT state, decided_at FROM action_proposals WHERE id = ?")
                .bind(id).fetch_one(pool).await.unwrap();
        (state, decided,
            count(pool, "SELECT COUNT(*) FROM action_precedents").await,
            count(pool, "SELECT COUNT(*) FROM lane_events").await,
            count(pool, "SELECT COUNT(*) FROM action_loop_outcomes").await)
    }

    #[tokio::test]
    async fn age_expiry_cutoff_is_thirty_days_inclusive() {
        let pool = crate::storage::memory().await.unwrap();
        let day = chrono::Duration::days(1);
        let young = seed(&pool, "age-29", "proposed", "draft", "draft:inbox_draft:a@example.com").await;
        let exact = seed(&pool, "age-30", "proposed", "draft", "draft:inbox_draft:b@example.com").await;
        let older = seed(&pool, "age-30s", "proposed", "draft", "draft:inbox_draft:c@example.com").await;
        age(&pool, young, day * 29).await;
        age(&pool, exact, day * 30).await;
        age(&pool, older, day * 30 + chrono::Duration::seconds(1)).await;
        assert_eq!(sweep_aged_proposals(&pool, fixed_now()).await.unwrap(), 2);
        assert_eq!(state(&pool, young).await, "pending");
        assert_eq!(state(&pool, exact).await, "expired");
        assert_eq!(state(&pool, older).await, "expired");
        assert_eq!(AGE_SWEEP_INTERVAL, std::time::Duration::from_secs(3600));
    }

    #[tokio::test]
    async fn a_young_closed_item_still_expires_through_the_click_path() {
        let pool = crate::storage::memory().await.unwrap();
        let closed = seed(&pool, "age-closed", "done", "draft", "draft:inbox_draft:bob@example.com").await;
        age(&pool, closed, chrono::Duration::days(1)).await;
        assert_eq!(sweep_aged_proposals(&pool, fixed_now()).await.unwrap(), 0);
        assert_eq!(state(&pool, closed).await, "pending");
        assert_eq!(resolve_at(&pool, closed, Choice::Once, "UOWNER", fixed_now()).await.unwrap(), Resolution::Expired);
        assert_eq!(state(&pool, closed).await, "expired");
        assert_eq!(count(&pool, "SELECT COUNT(*) FROM lane_events").await, 0);
    }

    #[tokio::test]
    async fn the_age_sweep_skips_other_origins_and_settled_rows_and_is_idempotent() {
        let pool = crate::storage::memory().await.unwrap();
        let old = chrono::Duration::days(40);
        let aged = seed(&pool, "age-a", "proposed", "draft", "draft:inbox_draft:a@example.com").await;
        let approved = seed(&pool, "age-b", "proposed", "draft", "draft:inbox_draft:b@example.com").await;
        let declined = seed(&pool, "age-c", "proposed", "draft", "draft:inbox_draft:c@example.com").await;
        let github = seed(&pool, "age-d", "proposed", "draft", "draft:inbox_draft:d@example.com").await;
        sqlx::query("UPDATE action_proposals SET origin = 'github' WHERE id = ?")
            .bind(github).execute(&pool).await.unwrap();
        assert!(matches!(resolve_at(&pool, approved, Choice::Once, "UOWNER", fixed_now()).await.unwrap(), Resolution::Approved { .. }));
        assert_eq!(resolve_at(&pool, declined, Choice::Deny, "UOWNER", fixed_now()).await.unwrap(), Resolution::Declined);
        for id in [aged, approved, declined, github] {
            age(&pool, id, old).await;
        }
        let before = |pool: SqlitePool| async move {
            sqlx::query_as::<_, (i64, String, Option<String>)>(
                "SELECT id, state, decided_at FROM action_proposals WHERE id <> ? ORDER BY id")
                .bind(aged).fetch_all(&pool).await.unwrap()
        };
        let untouched = before(pool.clone()).await;
        assert_eq!(sweep_aged_proposals(&pool, fixed_now()).await.unwrap(), 1);
        assert_eq!(before(pool.clone()).await, untouched);
        assert_eq!(state(&pool, github).await, "pending");
        let first = snapshot(&pool, aged).await;
        assert_eq!(first.0, "expired");
        let later = fixed_now() + chrono::Duration::hours(1);
        assert_eq!(sweep_aged_proposals(&pool, later).await.unwrap(), 0);
        assert_eq!(snapshot(&pool, aged).await, first);
    }

    #[tokio::test]
    async fn a_click_and_the_sweep_at_the_cutoff_agree_in_either_order() {
        let cutoff = chrono::Duration::days(AGE_EXPIRY_DAYS);
        for choice in [Choice::Once, Choice::Always, Choice::Deny] {
            let click_first = crate::storage::memory().await.unwrap();
            let a = seed(&click_first, "age-x", "proposed", "draft", "draft:inbox_draft:bob@example.com").await;
            age(&click_first, a, cutoff).await;
            assert_eq!(resolve_at(&click_first, a, choice, "UOWNER", fixed_now()).await.unwrap(), Resolution::Expired);
            assert_eq!(sweep_aged_proposals(&click_first, fixed_now()).await.unwrap(), 0);

            let sweep_first = crate::storage::memory().await.unwrap();
            let b = seed(&sweep_first, "age-x", "proposed", "draft", "draft:inbox_draft:bob@example.com").await;
            age(&sweep_first, b, cutoff).await;
            assert_eq!(sweep_aged_proposals(&sweep_first, fixed_now()).await.unwrap(), 1);
            assert_eq!(resolve_at(&sweep_first, b, choice, "UOWNER", fixed_now()).await.unwrap(),
                Resolution::AlreadyAnswered("expired".into()));

            let left = snapshot(&click_first, a).await;
            assert_eq!(left, snapshot(&sweep_first, b).await);
            assert_eq!(left.0, "expired");
            assert_eq!((left.2, left.3, left.4), (0, 0, 0), "expiry records no approval, precedent or event");
            let decision: String = sqlx::query_scalar("SELECT status FROM inbox_decisions WHERE item_id = 'age-x'")
                .fetch_one(&click_first).await.unwrap();
            assert_eq!(decision, "proposed");
        }
    }

    #[tokio::test]
    async fn a_click_after_a_restart_is_resolved_from_the_database() {
        let path = std::env::temp_dir().join(format!("inbox-approval-{}.db", uuid::Uuid::new_v4()));
        let id = {
            let pool = crate::storage::connect(&path).await.unwrap();
            let id = seed(&pool, "item-r", "proposed", "draft", "draft:inbox_draft:bob@example.com").await;
            pool.close().await;
            id
        };
        let pool = crate::storage::connect(&path).await.unwrap();
        let reply = handle_click(&pool, &click("UOWNER", &button_value(id, Choice::Once)), "UOWNER")
            .await.unwrap().unwrap();
        assert!(reply.card_line.is_some());
        assert_eq!(state(&pool, id).await, "approved");
        pool.close().await;
        for suffix in ["", "-wal", "-shm", ".open-lock"] {
            let mut name = path.as_os_str().to_owned();
            name.push(suffix);
            let _ = std::fs::remove_file(std::path::PathBuf::from(name));
        }
    }
}

