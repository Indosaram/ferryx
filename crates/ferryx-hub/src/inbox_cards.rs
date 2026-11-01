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

pub mod approval;

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
    /// The lead session an approved proposal is handed to (stored on the proposal row).
    pub lead: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ask {
    pub command: String,
    pub reason: String,
    /// The `action_proposals` row the card's buttons answer.
    pub proposal_id: i64,
    /// False for an effect that must be asked every time (a tainted outward send).
    pub allow_always: bool,
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
            let addresses_author =
                !author.is_empty() && addresses_sender(&card.proposed_action, author);
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

/// Words that make a drafted action an outward message. The proposed action is model text in
/// whatever language triage wrote it, so every listed language is checked for every owner.
const OUTWARD_WORDS: &[&str] = &[
    "send", "reply", "respond", "answer", "forward",
    "답장", "회신", "발송", "보내", "답변", "전달",
];

/// True when the drafted `action` speaks to the sender: it names `author` or says to send or
/// reply, in any listed language.
fn addresses_sender(action: &str, author: &str) -> bool {
    let lower = action.to_lowercase();
    action.contains(author) || OUTWARD_WORDS.iter().any(|word| lower.contains(word))
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
    status: String,
}

/// A card whose item is already closed is stale: the gate drops it instead of asking.
pub fn card_is_stale(status: &str) -> bool {
    matches!(status, "done" | "dismissed")
}

/// The gate report for one approval card: a `decision` the owner must answer, one topic per
/// item and command, so the same card is never asked twice.
pub fn card_report(card: &InboxCard, ask: &Ask) -> crate::report_gate::Report {
    crate::report_gate::Report::new(
        format!("inbox-card:{}", card.item_id),
        "inbox_card",
        format!("inbox-card:{}", card.item_id),
        crate::report_gate::Salience::Decision,
        crate::locale::text(
            "받은 연락에 대한 작업 승인이 필요합니다.",
            "A task for a message you received needs your approval.",
        ),
    )
    .with_state(&ask.command)
    .with_quote(ask.reason.clone())
    .with_sender(card.author.clone())
    .with_origin(card.item_id.clone())
}

/// Files a card (or auto-runs a lookup) for every bot decision that has a concrete action
/// and no settled ledger row: no row yet, or a pending row whose card never reached the gate
/// (its queueing failed), so a failed enqueue is retried on the next pass. Called after
/// triage when the trusted route is configured.
pub async fn present_ready(pool: &SqlitePool, action: &Action, lead: &str) -> Result<usize> {
    let rows: Vec<ReadyBot> = sqlx::query_as(
        "SELECT i.id, i.author, i.title, i.link, d.proposed_action, d.status
           FROM inbox_decisions d
           JOIN inbox_items i ON i.id = d.item_id
          WHERE d.class = 'bot'
            AND length(trim(COALESCE(d.proposed_action, ''))) > 0
            AND NOT EXISTS (
                SELECT 1 FROM action_proposals p
                 WHERE p.origin = 'inbox'
                   AND json_extract(p.payload_json, '$.item_id') = i.id
                   AND (p.state <> 'pending' OR p.platform_message_id IS NOT NULL)
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
            lead: lead.to_owned(),
        };
        let mut asks = Vec::new();
        evaluate_with_ledger(pool, &card, || async { Ok(()) }, |ask| asks.push(ask), true).await?;
        let stale = card_is_stale(&row.status);
        let mut queued = true;
        for ask in asks {
            let report = card_report(&card, &ask);
            match post_slack(pool, &card.channel_id, &report, &ask, stale).await {
                Ok(crate::report_gate::Decision::Sent) => {}
                Ok(decision) => tracing::info!(
                    item = %card.item_id,
                    decision = decision.as_str(),
                    "inbox approval card held by the report gate"
                ),
                Err(error) => {
                    // The proposal stays pending with no card receipt; the next pass retries it.
                    tracing::warn!(%error, item = %card.item_id, "inbox approval card was not queued");
                    queued = false;
                }
            }
        }
        if queued {
            filed += 1;
        }
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
        proposal_id: id,
        allow_always: !requires_always_ask(effect, tainted) && effect.allows_any_precedent(),
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
        // A row written before the lead was recorded gets it, so its approval can be handed over.
        sqlx::query(
            "UPDATE action_proposals SET payload_json = json_set(payload_json, '$.lead', ?)
              WHERE id = ? AND state = 'pending' AND json_extract(payload_json, '$.lead') IS NULL",
        )
        .bind(&card.lead)
        .bind(id)
        .execute(pool)
        .await?;
        return Ok(id);
    }
    let payload = json!({
        "item_id": card.item_id,
        "action": card.action,
        "lead": card.lead,
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

/// Queues the card through the report gate (mute/digest/duplicate rules apply; a deferred
/// digest card keeps its buttons) and records on the proposal, in the same transaction, that
/// its card reached the gate. Nothing is recorded when the transaction fails, so
/// [`present_ready`] retries it.
async fn post_slack(
    pool: &SqlitePool, channel: &str, report: &crate::report_gate::Report, ask: &Ask, stale: bool,
) -> Result<crate::report_gate::Decision> {
    use crate::storage::WriteTx;
    let session = crate::chat::session::SessionKey::new(
        "slack", None::<String>, channel, None::<String>, "inbox",
    );
    let text = format!("{}\n{}", report.body, ask.reason);
    let blocks = approval::card_blocks(ask.proposal_id, &ask.command, &text, ask.allow_always);
    let mut tx = pool.begin_write().await?;
    let (decision, outbox) = crate::report_gate::queue_card_in(
        &mut tx, &session.storage_key(), report, stale, &text, &blocks,
    ).await?;
    let receipt = match outbox {
        Some(id) => format!("outbox:{id}"),
        None => format!("gate:{}", decision.as_str()),
    };
    sqlx::query(
        "UPDATE action_proposals SET platform_message_id = ? WHERE id = ? AND platform_message_id IS NULL",
    )
    .bind(receipt)
    .bind(ask.proposal_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    if let (Some(id), crate::report_gate::Decision::Sent) = (outbox, decision) {
        crate::owner_alert::post_queued(pool, id, &session.storage_key(), &text).await?;
    }
    Ok(decision)
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
            lead: "lead-session".into(),
        }
    }

    #[test]
    fn an_outward_draft_is_recognized_in_korean_and_english_for_any_owner() {
        for locale in [crate::locale::OwnerLocale::korean(), crate::locale::OwnerLocale::english_utc()] {
            crate::locale::with(locale, || {
                for text in ["Reply that Friday works", "금요일 가능하다고 회신", "견적서를 보내 드리기", "Forward the invoice"] {
                    let (effect, _, _) = classify_inbox_action(&card("i", "ann", text, Action::Draft));
                    assert_eq!(effect, Effect::ExternalSend, "{text}");
                }
                let (effect, _, _) =
                    classify_inbox_action(&card("i", "ann", "prepare summary note", Action::Draft));
                assert_eq!(effect, Effect::Draft);
            });
        }
        let english = crate::locale::with(crate::locale::OwnerLocale::english_utc(), || {
            let ask = Ask { command: "c".into(), reason: "r".into(), proposal_id: 1, allow_always: true };
            card_report(&card("i", "ann", "x", Action::Draft), &ask).body
        });
        assert_eq!(english, "A task for a message you received needs your approval.");
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

    #[tokio::test]
    async fn a_card_for_a_closed_item_is_dropped_as_stale() {
        let pool = crate::storage::memory().await.unwrap();
        let card = card("item-stale", "bob@example.com", "prepare summary note", Action::Draft);
        let ask = Ask {
            command: "draft:item-stale".into(),
            reason: "요약 메모 초안을 만들까요?".into(),
            proposal_id: 1,
            allow_always: true,
        };
        assert!(card_is_stale("done"));
        assert!(!card_is_stale("open"));
        let report = card_report(&card, &ask);
        assert_eq!(
            crate::report_gate::submit_checked(&pool, &report, true).await.unwrap(),
            crate::report_gate::Decision::Stale
        );
        assert_eq!(
            crate::report_gate::submit_checked(&pool, &report, false).await.unwrap(),
            crate::report_gate::Decision::Sent
        );
    }

    #[cfg(feature = "hub-slack")]
    #[tokio::test]
    async fn a_failed_card_post_keeps_its_buttons_for_retry() {
        let pool = crate::storage::memory().await.unwrap();
        let card = card("retry-card", "bob@example.com", "draft summary", Action::Draft);
        let ask = Ask { command: card.proposed_action.clone(), reason: "요약 초안을 승인해주세요.".into(),
            proposal_id: 1, allow_always: true };
        let report = card_report(&card, &ask);
        let (base, calls) = crate::chat::slack_web::mock::serve_scripted("UBOT", vec![
            ("chat.postMessage", vec![json!({"ok":false,"error":"service_unavailable"}),
                json!({"ok":true,"channel":"C1","ts":"100.2"})]),
        ]).await;
        crate::chat::slack::with_test_api(base, "xoxb-test".into(), async {
            assert_eq!(post_slack(&pool, "C1", &report, &ask, false).await.unwrap(),
                crate::report_gate::Decision::Sent);
            assert_eq!(crate::chat::reply::pending(&pool).await.unwrap().len(), 1);
            assert_eq!(crate::chat::reply::flush_pending(&pool).await.unwrap(), 1);
        }).await;
        assert!(crate::chat::reply::pending(&pool).await.unwrap().is_empty());
        let calls = calls.lock().unwrap();
        let posts: Vec<_> = calls.iter().filter(|(method, _)| method == "chat.postMessage").collect();
        assert_eq!(posts.len(), 2);
        assert_eq!(posts[0].1["blocks"], posts[1].1["blocks"]);
        assert_eq!(posts[1].1["blocks"][2]["elements"].as_array().unwrap().len(), 3);
        assert_eq!(posts[1].1["blocks"][2]["elements"][0]["value"], "omon:inbox:1:once");
    }

    #[cfg(feature = "hub-slack")]
    async fn seed_bot_decision(pool: &SqlitePool, id: &str, author: &str, action_text: &str) {
        seed(pool, id, author).await;
        sqlx::query(
            "INSERT INTO inbox_decisions(item_id, class, reason, proposed_action, status, decided_at)
             VALUES(?, 'bot', 'bot_can_handle', ?, 'proposed', ?)",
        )
        .bind(id)
        .bind(action_text)
        .bind(now_rfc3339())
        .execute(pool)
        .await
        .unwrap();
    }

    #[cfg(feature = "hub-slack")]
    fn lead_key() -> String {
        crate::chat::session::SessionKey::new("slack", None::<String>, "C1", None::<String>, "UOWNER")
            .storage_key()
    }

    #[cfg(feature = "hub-slack")]
    #[tokio::test]
    async fn a_card_whose_enqueue_failed_is_presented_again_on_the_next_pass() {
        let (base, calls) = crate::chat::slack_web::mock::serve("UBOT").await;
        let pool = crate::storage::memory().await.unwrap();
        seed_bot_decision(&pool, "item-retry", "bob@example.com", "prepare summary note").await;
        sqlx::query("CREATE TRIGGER reject_outbox BEFORE INSERT ON chat_outbox BEGIN SELECT RAISE(ABORT, 'test failure'); END")
            .execute(&pool).await.unwrap();
        let lead = lead_key();
        crate::chat::slack::with_test_api(base, "xoxb-test".into(), async {
            assert_eq!(present_ready(&pool, &Action::Draft, &lead).await.unwrap(), 0);
            let (state, receipt): (String, Option<String>) = sqlx::query_as(
                "SELECT state, platform_message_id FROM action_proposals WHERE origin = 'inbox'")
                .fetch_one(&pool).await.unwrap();
            assert_eq!((state.as_str(), receipt), ("pending", None));
            sqlx::query("DROP TRIGGER reject_outbox").execute(&pool).await.unwrap();
            assert_eq!(present_ready(&pool, &Action::Draft, &lead).await.unwrap(), 1);
            assert_eq!(present_ready(&pool, &Action::Draft, &lead).await.unwrap(), 0);
        }).await;
        let proposals: Vec<(i64, Option<String>)> = sqlx::query_as(
            "SELECT id, platform_message_id FROM action_proposals WHERE origin = 'inbox'")
            .fetch_all(&pool).await.unwrap();
        assert_eq!(proposals.len(), 1, "the retry reuses the same proposal row");
        assert!(proposals[0].1.as_deref().unwrap().starts_with("outbox:"));
        let calls = calls.lock().unwrap();
        let posts: Vec<_> = calls.iter().filter(|(method, _)| method == "chat.postMessage").collect();
        assert_eq!(posts.len(), 1);
        assert_eq!(posts[0].1["blocks"][2]["elements"][0]["value"],
            format!("omon:inbox:{}:once", proposals[0].0));
    }

    #[cfg(feature = "hub-slack")]
    #[tokio::test]
    async fn the_posted_card_button_approves_its_proposal_through_the_slack_frame() {
        let (base, calls) = crate::chat::slack_web::mock::serve("UBOT").await;
        let pool = crate::storage::memory().await.unwrap();
        seed_bot_decision(&pool, "item-e2e", "bob@example.com", "reply to bob@example.com").await;
        let lead = lead_key();
        let control = crate::agent_control::RecordingControl::default();
        let router = crate::chat::router::ProfileRouter::new(vec![]);
        crate::chat::slack::with_test_api(base, "xoxb-test".into(), async {
            assert_eq!(present_ready(&pool, &Action::Draft, &lead).await.unwrap(), 1);
            let blocks = {
                let calls = calls.lock().unwrap();
                calls.iter().find(|(method, _)| method == "chat.postMessage").unwrap().1["blocks"].clone()
            };
            let elements = blocks[2]["elements"].as_array().unwrap();
            // A tainted outward send offers no "always" button.
            assert_eq!(elements.len(), 2);
            let value = elements[0]["value"].as_str().unwrap().to_owned();
            let frame = |user: &str| json!({"envelope_id": "e1", "type": "interactive", "payload": {
                "type": "block_actions", "user": {"id": user}, "channel": {"id": "C1"},
                "container": {"message_ts": "1700000000.000000"},
                "message": {"ts": "1700000000.000000", "blocks": blocks},
                "actions": [{"action_id": value, "value": value}]}}).to_string();
            for user in ["USTRANGER", "UOWNER", "UOWNER"] {
                crate::chat::slack::apply_frame_with(&frame(user), "UOWNER", "meta", &router,
                    &control, Some(&pool), None).await.unwrap();
            }
        }).await;
        assert!(control.ops().is_empty(), "an inbox approval never reaches a pane");
        let state: String = sqlx::query_scalar("SELECT state FROM action_proposals WHERE origin = 'inbox'")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(state, "approved");
        let events: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM lane_events WHERE kind = 'action_loop' AND lead_session_key = ?")
            .bind(&lead).fetch_one(&pool).await.unwrap();
        assert_eq!(events, 1, "a replayed click hands the item over once");
        let calls = calls.lock().unwrap();
        let updates = calls.iter().filter(|(method, _)| method == "chat.update").count();
        let ephemerals = calls.iter().filter(|(method, _)| method == "chat.postEphemeral").count();
        assert_eq!(updates, 1);
        assert_eq!(ephemerals, 2, "the stranger and the replay are told privately");
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
            let report = card_report(&card("item-slack", "bob@example.com",
                "prepare summary note", Action::Draft), &ask);
            assert_eq!(post_slack(&pool, "C1", &report, &ask, false).await.unwrap(),
                crate::report_gate::Decision::Sent);
            assert_eq!(post_slack(&pool, "C1", &report, &ask, false).await.unwrap(),
                crate::report_gate::Decision::Duplicate);
            let receipts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chat_outbox WHERE delivered_at IS NOT NULL AND posted_ts IS NOT NULL AND blocks_json IS NOT NULL")
                .fetch_one(&pool).await.unwrap();
            assert_eq!(receipts, 1);
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
