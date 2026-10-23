//! Inbox approval cards. omo `inbox/proposals.rs` `evaluate_with_ledger`:
//! a lookup (read) runs with no card, a draft with no exact precedent posts one approval
//! card, an exact allow precedent runs it, and an exact deny records `auto_denied`.
//! A class-only precedent does not cover a tainted outward send.
//!
//! The trusted route is `OMON_INBOX_PROPOSAL_ACTION` (omo's lead config), never text parsed
//! out of the mail. Unset, nothing here runs, so a bot item still reaches the action loop
//! without a generic "handle this" card.
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::SqlitePool;

use crate::effect::{requires_always_ask, Effect};
use crate::error::{HubError, Result};
use crate::storage::now_rfc3339;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Repo { cwd: String, done: String },
    Lookup,
    Draft,
}

#[derive(Clone, Debug)]
pub struct InboxCard {
    pub item_id: String,
    pub channel_id: String,
    pub author: String,
    pub title: String,
    pub link: Option<String>,
    pub proposed_action: String,
    pub action: Action,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ask {
    pub command: String,
    pub reason: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardOutcome {
    Executed,
    Asked,
    Denied,
    LeftPending,
}

/// omo `classify_inbox_proposal`.
pub fn classify_inbox_action(card: &InboxCard) -> (Effect, String, String) {
    match &card.action {
        Action::Lookup => {
            let target = card
                .link
                .clone()
                .filter(|link| !link.is_empty())
                .unwrap_or_else(|| card.item_id.clone());
            (
                Effect::Read,
                target.clone(),
                format!("read:inbox_lookup:{target}"),
            )
        }
        Action::Repo { cwd, .. } => (
            Effect::WriteLocal,
            cwd.clone(),
            format!("write_local:inbox_repo:{cwd}"),
        ),
        Action::Draft => {
            let author = card.author.trim();
            let action = &card.proposed_action;
            let addresses_author = !author.is_empty()
                && (action.contains(author)
                    || action.to_ascii_lowercase().contains("send")
                    || action.to_ascii_lowercase().contains("reply")
                    || action.contains("답장")
                    || action.contains("회신")
                    || action.contains("발송"));
            if addresses_author {
                (
                    Effect::ExternalSend,
                    author.to_owned(),
                    format!("external_send:inbox_draft:{author}"),
                )
            } else {
                let target = if !author.is_empty() {
                    author.to_owned()
                } else {
                    card.channel_id.clone()
                };
                (
                    Effect::Draft,
                    target.clone(),
                    format!("draft:inbox_draft:{target}"),
                )
            }
        }
    }
}

fn display(card: &InboxCard, number: i64) -> String {
    let route = match &card.action {
        Action::Repo { cwd, done } => format!("\n[repo cwd={cwd} done={done}]"),
        Action::Lookup => "\n[lookup]".to_owned(),
        Action::Draft => "\n[draft]".to_owned(),
    };
    format!(
        "#{number} {} -> {}{route}\n{}",
        card.title,
        card.proposed_action,
        card.link.as_deref().unwrap_or("")
    )
}

/// The lead's configured route. Invalid JSON leaves cards off.
pub fn trusted_action() -> Option<Action> {
    let raw = crate::hub_env::var("OMON_INBOX_PROPOSAL_ACTION").ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    match serde_json::from_str(trimmed) {
        Ok(action) => Some(action),
        Err(error) => {
            tracing::warn!(%error, "invalid OMON_INBOX_PROPOSAL_ACTION; inbox cards stay off");
            None
        }
    }
}

#[derive(sqlx::FromRow)]
struct ReadyBot {
    id: String,
    author: Option<String>,
    title: Option<String>,
    link: Option<String>,
    proposed_action: String,
}

/// Files a card (or auto-runs a lookup) for every bot decision that has a concrete action
/// and no ledger row yet. Called after triage when the trusted route is configured.
pub async fn present_ready(pool: &SqlitePool, action: &Action, lead: &str) -> Result<usize> {
    let rows: Vec<ReadyBot> = sqlx::query_as(
        "SELECT i.id, i.author, i.title, i.link, d.proposed_action
           FROM inbox_decisions d
           JOIN inbox_items i ON i.id = d.item_id
          WHERE d.class = 'bot'
            AND length(trim(COALESCE(d.proposed_action, ''))) > 0
            AND NOT EXISTS (
                SELECT 1 FROM action_proposals p
                 WHERE p.origin = 'inbox'
                   AND json_extract(p.payload_json, '$.item_id') = i.id
            )",
    )
    .fetch_all(pool)
    .await?;
    let channel = crate::chat::session::SessionKey::from_storage_key(lead)
        .map(|key| key.channel_id)
        .unwrap_or_else(|_| lead.to_owned());
    let mut filed = 0usize;
    for row in rows {
        let card = InboxCard {
            item_id: row.id,
            channel_id: channel.clone(),
            author: row.author.unwrap_or_default(),
            title: row.title.unwrap_or_default(),
            link: row.link,
            proposed_action: row.proposed_action,
            action: action.clone(),
        };
        let mut asks = Vec::new();
        evaluate_with_ledger(pool, &card, || async { Ok(()) }, |ask| asks.push(ask), true).await?;
        for ask in asks {
            if let Err(error) = post_slack(&card.channel_id, &ask).await {
                tracing::warn!(%error, item = %card.item_id, "inbox approval card was not posted");
            }
        }
        filed += 1;
    }
    Ok(filed)
}

/// One ledger pass. `post` asks the owner; without it a draft stays pending (omo's serve
/// path, which has no card sink). `run` is the lookup or draft that an allow executes.
pub async fn evaluate_with_ledger<R, RF, A>(
    pool: &SqlitePool,
    card: &InboxCard,
    run: R,
    mut ask: A,
    post: bool,
) -> Result<CardOutcome>
where
    R: FnOnce() -> RF,
    RF: std::future::Future<Output = Result<()>>,
    A: FnMut(Ask),
{
    if card.channel_id.is_empty() {
        return Err(HubError::message("proposal channel is required"));
    }
    let (effect, target, signature) = classify_inbox_action(card);
    let id = ensure_proposal(pool, card, &effect, &target, &signature).await?;
    // Inbox items are untrusted, so the ledger row is tainted (omo backfill).
    let tainted = true;
    if effect == Effect::Read {
        settle(pool, id, "auto_allowed", None).await?;
        run().await?;
        return Ok(CardOutcome::Executed);
    }
    if let Some((precedent, decision)) =
        matching_precedent(pool, &signature, effect, &target, tainted).await?
    {
        if decision == "allow" {
            settle(pool, id, "auto_allowed", Some(precedent)).await?;
            run().await?;
            return Ok(CardOutcome::Executed);
        }
        settle(pool, id, "auto_denied", Some(precedent)).await?;
        return Ok(CardOutcome::Denied);
    }
    if !post {
        return Ok(CardOutcome::LeftPending);
    }
    let shown = display(card, id);
    ask(Ask {
        command: card.proposed_action.clone(),
        reason: format!("{shown}\neffect: {} | target: {target}", effect.as_str()),
    });
    Ok(CardOutcome::Asked)
}

async fn ensure_proposal(
    pool: &SqlitePool,
    card: &InboxCard,
    effect: &Effect,
    target: &str,
    signature: &str,
) -> Result<i64> {
    let existing: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM action_proposals
          WHERE origin = 'inbox' AND json_extract(payload_json, '$.item_id') = ?
          ORDER BY id DESC LIMIT 1",
    )
    .bind(&card.item_id)
    .fetch_optional(pool)
    .await?;
    if let Some(id) = existing {
        return Ok(id);
    }
    let payload = json!({
        "item_id": card.item_id,
        "action": card.action,
    })
    .to_string();
    let state = if *effect == Effect::Read {
        "auto_allowed"
    } else {
        "pending"
    };
    let id = sqlx::query(
        "INSERT INTO action_proposals(origin, session_key, signature, effect, target, summary, payload_json, tainted, state, created_at)
         VALUES('inbox', ?, ?, ?, ?, ?, ?, 1, ?, ?)",
    )
    .bind(&card.channel_id)
    .bind(signature)
    .bind(effect.as_str())
    .bind(target)
    .bind(card.proposed_action.as_str())
    .bind(payload)
    .bind(state)
    .bind(now_rfc3339())
    .execute(pool)
    .await?
    .last_insert_rowid();
    Ok(id)
}

async fn settle(pool: &SqlitePool, id: i64, state: &str, precedent: Option<i64>) -> Result<()> {
    sqlx::query(
        "UPDATE action_proposals SET state = ?, precedent_id = ?, decided_at = ? WHERE id = ?",
    )
    .bind(state)
    .bind(precedent)
    .bind(now_rfc3339())
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

async fn matching_precedent(
    pool: &SqlitePool,
    signature: &str,
    effect: Effect,
    target: &str,
    tainted: bool,
) -> Result<Option<(i64, String)>> {
    if effect == Effect::Spend {
        return Ok(None);
    }
    if tainted
        && matches!(
            effect,
            Effect::Delete | Effect::Spend | Effect::VcsProtected
        )
    {
        return Ok(None);
    }
    let exact: Option<(i64, String)> = sqlx::query_as(
        "SELECT id, decision FROM action_precedents
          WHERE signature = ? AND scope = 'exact' AND revoked_at IS NULL
          ORDER BY id DESC LIMIT 1",
    )
    .bind(signature)
    .fetch_optional(pool)
    .await?;
    if exact.is_some() {
        return Ok(exact);
    }
    if requires_always_ask(effect, tainted) || !effect.allows_class_precedent() {
        return Ok(None);
    }
    let class: Option<(i64, String)> = sqlx::query_as(
        "SELECT id, decision FROM action_precedents
          WHERE scope = 'class' AND effect = ? AND target_scope = ? AND revoked_at IS NULL
          ORDER BY id DESC LIMIT 1",
    )
    .bind(effect.as_str())
    .bind(target)
    .fetch_optional(pool)
    .await?;
    Ok(class)
}

async fn post_slack(channel: &str, ask: &Ask) -> Result<bool> {
    #[cfg(feature = "hub-slack")]
    {
        if crate::chat::slack::bot_token().is_none() {
            return Ok(false);
        }
        let id = uuid::Uuid::new_v4();
        let blocks = crate::chat::turn_approval::approval_blocks(id, &ask.command, &ask.reason);
        let body = json!({
            "channel": channel,
            "text": ask.reason,
            "blocks": blocks,
        });
        crate::chat::slack_web::post_json("chat.postMessage", &body).await?;
        Ok(true)
    }
    #[cfg(not(feature = "hub-slack"))]
    {
        let _ = (channel, ask);
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(item: &str, author: &str, action_text: &str, action: Action) -> InboxCard {
        InboxCard {
            item_id: item.into(),
            channel_id: "C1".into(),
            author: author.into(),
            title: "notes".into(),
            link: Some("https://example.com/item".into()),
            proposed_action: action_text.into(),
            action,
        }
    }

    async fn seed(pool: &SqlitePool, id: &str, author: &str) {
        sqlx::query(
            "INSERT INTO inbox_items(id, source, source_ref, author, title, snippet, link, received_at, raw_hash, collected_at)
             VALUES(?, 'mail', ?, ?, 'notes', 'body', 'https://example.com/item', ?, ?, ?)",
        )
        .bind(id)
        .bind(id)
        .bind(author)
        .bind(now_rfc3339())
        .bind(id)
        .bind(now_rfc3339())
        .execute(pool)
        .await
        .unwrap();
    }

    async fn state_of(pool: &SqlitePool, item: &str) -> (String, String, i64) {
        sqlx::query_as(
            "SELECT state, effect, tainted FROM action_proposals
              WHERE origin = 'inbox' AND json_extract(payload_json, '$.item_id') = ?",
        )
        .bind(item)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn a_lookup_auto_runs_with_no_card() {
        let pool = crate::storage::memory().await.unwrap();
        seed(&pool, "item-lookup", "alice@example.com").await;
        let ran = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let runs = ran.clone();
        let mut asks = Vec::new();
        let outcome = evaluate_with_ledger(
            &pool,
            &card(
                "item-lookup",
                "alice@example.com",
                "lookup link",
                Action::Lookup,
            ),
            || {
                runs.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                async { Ok(()) }
            },
            |ask| asks.push(ask),
            true,
        )
        .await
        .unwrap();
        assert_eq!(outcome, CardOutcome::Executed);
        assert_eq!(ran.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(
            asks.is_empty(),
            "lookup must auto-run with 0 approval cards"
        );
        let (state, effect, tainted) = state_of(&pool, "item-lookup").await;
        assert_eq!(state, "auto_allowed");
        assert_eq!(effect, "read");
        assert_eq!(tainted, 1);
    }

    #[tokio::test]
    async fn a_draft_with_no_precedent_posts_one_card() {
        let pool = crate::storage::memory().await.unwrap();
        seed(&pool, "item-draft", "bob@example.com").await;
        let mut asks = Vec::new();
        let outcome = evaluate_with_ledger(
            &pool,
            &card(
                "item-draft",
                "bob@example.com",
                "prepare summary note",
                Action::Draft,
            ),
            || async {
                panic!("a draft with no precedent must not run");
            },
            |ask| asks.push(ask),
            true,
        )
        .await
        .unwrap();
        assert_eq!(outcome, CardOutcome::Asked);
        assert_eq!(
            asks.len(),
            1,
            "draft with no precedent posts exactly 1 card"
        );
        assert!(asks[0].reason.contains("effect: draft"));
        let (state, effect, _) = state_of(&pool, "item-draft").await;
        assert_eq!(state, "pending");
        assert_eq!(effect, "draft");
    }

    #[tokio::test]
    async fn an_exact_allow_precedent_runs_the_draft_with_no_card() {
        let pool = crate::storage::memory().await.unwrap();
        seed(&pool, "item-send", "carol@example.com").await;
        sqlx::query(
            "INSERT INTO action_precedents(signature, effect, target_scope, decision, scope, origin)
             VALUES('external_send:inbox_draft:carol@example.com', 'external_send', 'carol@example.com', 'allow', 'exact', 'owner')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let ran = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let runs = ran.clone();
        let mut asks = Vec::new();
        let outcome = evaluate_with_ledger(
            &pool,
            &card(
                "item-send",
                "carol@example.com",
                "reply to carol@example.com with receipt",
                Action::Draft,
            ),
            || {
                runs.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                async { Ok(()) }
            },
            |ask| asks.push(ask),
            true,
        )
        .await
        .unwrap();
        assert_eq!(outcome, CardOutcome::Executed);
        assert_eq!(ran.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(asks.is_empty());
        let (state, effect, _) = state_of(&pool, "item-send").await;
        assert_eq!(state, "auto_allowed");
        assert_eq!(effect, "external_send");
    }

    #[tokio::test]
    async fn a_class_precedent_still_asks_before_an_outward_send() {
        let pool = crate::storage::memory().await.unwrap();
        seed(&pool, "item-class", "dave@example.com").await;
        sqlx::query(
            "INSERT INTO action_precedents(signature, effect, target_scope, decision, scope, approvals, origin)
             VALUES('external_send:class:dave@example.com', 'external_send', 'dave@example.com', 'allow', 'class', 3, 'test')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let mut asks = Vec::new();
        let outcome = evaluate_with_ledger(
            &pool,
            &card(
                "item-class",
                "dave@example.com",
                "reply to dave@example.com immediately",
                Action::Draft,
            ),
            || async {
                panic!("a class precedent must not auto-run an outward send");
            },
            |ask| asks.push(ask),
            true,
        )
        .await
        .unwrap();
        assert_eq!(outcome, CardOutcome::Asked);
        assert_eq!(asks.len(), 1);
    }

    #[tokio::test]
    async fn an_exact_deny_records_auto_denied_and_posts_nothing() {
        let pool = crate::storage::memory().await.unwrap();
        seed(&pool, "item-deny", "erin@example.com").await;
        sqlx::query(
            "INSERT INTO action_precedents(signature, effect, target_scope, decision, scope, origin)
             VALUES('draft:inbox_draft:erin@example.com', 'draft', 'erin@example.com', 'deny', 'exact', 'owner')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let mut asks = Vec::new();
        let outcome = evaluate_with_ledger(
            &pool,
            &card(
                "item-deny",
                "erin@example.com",
                "prepare summary note",
                Action::Draft,
            ),
            || async {
                panic!("a deny precedent must not run");
            },
            |ask| asks.push(ask),
            true,
        )
        .await
        .unwrap();
        assert_eq!(outcome, CardOutcome::Denied);
        assert!(asks.is_empty());
        let (state, _, _) = state_of(&pool, "item-deny").await;
        assert_eq!(state, "auto_denied");
    }

    #[tokio::test]
    async fn without_a_card_sink_a_lookup_runs_and_a_draft_stays_pending() {
        let pool = crate::storage::memory().await.unwrap();
        seed(&pool, "m4-lookup", "alice@example.com").await;
        seed(&pool, "m4-draft", "bob@example.com").await;
        let lookup = evaluate_with_ledger(
            &pool,
            &card(
                "m4-lookup",
                "alice@example.com",
                "lookup link",
                Action::Lookup,
            ),
            || async { Ok(()) },
            |_| {},
            false,
        )
        .await
        .unwrap();
        let draft = evaluate_with_ledger(
            &pool,
            &card(
                "m4-draft",
                "bob@example.com",
                "prepare summary note",
                Action::Draft,
            ),
            || async {
                panic!("draft stays pending");
            },
            |_| {},
            false,
        )
        .await
        .unwrap();
        assert_eq!(lookup, CardOutcome::Executed);
        assert_eq!(draft, CardOutcome::LeftPending);
    }

    #[cfg(feature = "hub-slack")]
    #[tokio::test]
    async fn a_draft_posts_one_slack_approval_card() {
        let (base, calls) = crate::chat::slack_web::mock::serve("UBOT").await;
        let asks = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let record = asks.clone();
        crate::chat::slack::with_test_api(base, "xoxb-test".into(), async move {
            let pool = crate::storage::memory().await.unwrap();
            seed(&pool, "item-slack", "bob@example.com").await;
            let outcome = evaluate_with_ledger(
                &pool,
                &card(
                    "item-slack",
                    "bob@example.com",
                    "prepare summary note",
                    Action::Draft,
                ),
                || async {
                    panic!("the card is the ask");
                },
                |ask| record.lock().unwrap().push(ask),
                true,
            )
            .await
            .unwrap();
            assert_eq!(outcome, CardOutcome::Asked);
            let ask = record.lock().unwrap()[0].clone();
            let blocks = crate::chat::turn_approval::approval_blocks(
                uuid::Uuid::new_v4(),
                &ask.command,
                &ask.reason,
            );
            crate::chat::slack_web::post_json(
                "chat.postMessage",
                &json!({
                    "channel": "C1",
                    "text": ask.reason,
                    "blocks": blocks,
                }),
            )
            .await
            .unwrap();
        })
        .await;
        let calls = calls.lock().unwrap();
        let posted: Vec<_> = calls
            .iter()
            .filter(|(method, _)| method == "chat.postMessage")
            .collect();
        assert_eq!(posted.len(), 1);
        assert!(posted[0].1["blocks"].as_array().is_some());
        assert!(posted[0].1["text"]
            .as_str()
            .unwrap()
            .contains("effect: draft"));
    }
}
