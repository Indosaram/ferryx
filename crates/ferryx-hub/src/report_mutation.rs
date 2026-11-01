//! Owner-socket follow-ups bound to an actual gated Slack receipt.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use crate::storage::WriteTx;
use crate::{HubError, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub outbox_id: i64,
    pub mutation_id: String,
    pub change: Change,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Change {
    ActionRow { row: Value },
    RestoreActions,
    Attachment { path: String, name: String, title: String },
}

impl Request {
    pub fn validate(&self) -> Result<()> {
        if self.outbox_id <= 0 || self.mutation_id.trim().is_empty() {
            return Err(HubError::message("mutation requires a parent outbox id and stable identity"));
        }
        match &self.change {
            Change::ActionRow { row } => {
                let text = |value: &Value| matches!(value["type"].as_str(), Some("plain_text" | "mrkdwn"))
                    && value["text"].as_str().is_some_and(|text| !text.trim().is_empty());
                let valid = row.get("accessory").is_none() && match row["type"].as_str() {
                    Some("context") => row["elements"].as_array()
                        .is_some_and(|elements| !elements.is_empty() && elements.iter().all(text)),
                    Some("section") => text(&row["text"]) && row.get("elements").is_none(),
                    _ => false,
                };
                if valid { Ok(()) } else { Err(HubError::message("card status must contain only non-interactive text")) }
            }
            Change::Attachment { path, name, title } if !std::path::Path::new(path).is_absolute()
                || name.trim().is_empty() || title.trim().is_empty() => Err(HubError::message("attachment requires absolute path, name and title")),
            Change::RestoreActions | Change::Attachment { .. } => Ok(()),
        }
    }
}

/// Mutations that still hold the parent: any claim, and an unknown card update (awaiting
/// read-back). An unknown attachment completion is final and no longer holds later edits.
const OPEN: &str = "(status='claimed' AND json_extract(request_json,'$.change.kind')='attachment')";

/// How long a claim is expected to run (each Slack call times out within 60 seconds). Past it
/// status reports the claim as stalled. Expiry never proves the request finished and never
/// lets another call take the claim: only the owner's own end (or its process's) does.
const MUTATION_LEASE: chrono::Duration = chrono::Duration::minutes(5);

/// Calls of this process that hold a claim, by owner token.
fn live() -> std::sync::MutexGuard<'static, std::collections::HashSet<String>> {
    static LIVE: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> = std::sync::OnceLock::new();
    LIVE.get_or_init(Default::default).lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// This process incarnation; a restarted hub that reuses the pid is a different owner.
fn instance() -> &'static str {
    static ID: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    ID.get_or_init(|| uuid::Uuid::new_v4().to_string())
}

/// Whether the call that stored this claim may still be running. A claim of this process is
/// live while its call is; another process's while that process exists. Rows without an
/// owner predate ownership and belong to no running call.
fn owner_live(token: Option<&str>, owner: Option<&str>, pid: Option<i64>) -> bool {
    match (token, owner) {
        (Some(token), Some(owner)) if owner == instance() => live().contains(token),
        (Some(_), Some(_)) => pid.and_then(|pid| u32::try_from(pid).ok())
            .is_some_and(|pid| pid != std::process::id() && crate::schedule::process_alive(pid)),
        _ => false,
    }
}

/// One running call's ownership of a claim; dropping it (return, error, cancellation) ends it.
struct Flight { token: String }

impl Flight {
    fn new() -> Self {
        let token = uuid::Uuid::new_v4().to_string();
        live().insert(token.clone());
        Self { token }
    }

    fn lease(&self) -> (String, &'static str, i64, String) {
        (self.token.clone(), instance(), i64::from(std::process::id()),
            (chrono::Utc::now() + MUTATION_LEASE).to_rfc3339())
    }
}

impl Drop for Flight {
    fn drop(&mut self) { live().remove(&self.token); }
}

/// Per-mutation receipts for `report_status`, with the same vocabulary as [`reply`]: a claim
/// whose call still runs is `pending`/`in_flight` (`stalled` once past its lease, still not
/// finished); an unknown card update is `pending`/`recovering`; only an attachment whose
/// call ended without an outcome is `uncertain`.
pub(crate) async fn statuses(pool: &SqlitePool, outbox_id: i64) -> Result<Vec<Value>> {
    type CardOwner = (String, Option<String>, Option<String>, Option<i64>);
    let desired: Option<CardOwner> = sqlx::query_as("SELECT desired_mutation_id,writer_token,writer_instance,writer_pid FROM report_cards WHERE outbox_id=?")
        .bind(outbox_id).fetch_optional(pool).await?;
    type Row = (String, String, String, Option<String>, Option<String>, Option<i64>, Option<String>);
    let rows: Vec<Row> = sqlx::query_as("SELECT mutation_id,request_json,status,owner_token,owner_instance,owner_pid,claim_until
        FROM report_mutations WHERE outbox_id=? ORDER BY rowid").bind(outbox_id).fetch_all(pool).await?;
    Ok(rows.into_iter().map(|(id, request, status, token, owner, pid, until)| {
        let card = serde_json::from_str::<Value>(&request).ok()
            .is_some_and(|request| request["change"]["kind"] != "attachment");
        match status.as_str() {
            "delivered" => json!({"mutation_id":id,"delivery":"delivered"}),
            _ if card && desired.as_ref().is_some_and(|(target, _, _, _)| target != &id) =>
                json!({"mutation_id":id,"delivery":"superseded"}),
            _ if card && desired.as_ref().is_some_and(|(_, token, owner, pid)| owner_live(token.as_deref(), owner.as_deref(), *pid)) =>
                json!({"mutation_id":id,"delivery":"pending","progress":"in_flight"}),
            "claimed" if owner_live(token.as_deref(), owner.as_deref(), pid) => {
                let stalled = until.as_deref().and_then(|until| chrono::DateTime::parse_from_rfc3339(until).ok())
                    .is_none_or(|until| until <= chrono::Utc::now());
                json!({"mutation_id":id,"delivery":"pending","progress":"in_flight","stalled":stalled})
            }
            _ if card => json!({"mutation_id":id,"delivery":"pending","progress":"recovering"}),
            _ => json!({"mutation_id":id,"delivery":"uncertain"}),
        }
    }).collect())
}

/// `delivery` stays one of `delivered`, `suppressed`, `pending` or `uncertain`. A pending answer
/// about the caller's own identity carries `progress` and names itself in `blocked_by`:
/// `in_flight` while its Slack request is still running, `recovering` while an ambiguous card
/// update awaits read-back confirmation. Both mean "ask again with the same identity"; only
/// `uncertain` is a final unknown outcome, and only an attachment completion ends there.
pub async fn reply(pool: &SqlitePool, request: &Request) -> Value {
    match perform(pool, request).await {
        Ok(progress @ ("in_flight" | "recovering")) => json!({"status":"ok","op":"report_mutate",
            "outbox_id":request.outbox_id,"mutation_id":request.mutation_id,"delivery":"pending",
            "progress":progress,"blocked_by":request.mutation_id}),
        Ok("pending") => {
            let blocker: Result<Option<String>> = sqlx::query_scalar(&format!(
                "SELECT mutation_id FROM report_mutations WHERE outbox_id=? AND mutation_id<>? AND {OPEN} ORDER BY rowid LIMIT 1"))
                .bind(request.outbox_id).bind(&request.mutation_id).fetch_optional(pool).await.map_err(Into::into);
            match blocker {
                Ok(blocker) => json!({"status":"ok","op":"report_mutate","outbox_id":request.outbox_id,
                    "mutation_id":request.mutation_id,"delivery":"pending","blocked_by":blocker}),
                Err(error) => json!({"status":"failed","op":"report_mutate","error":error.to_string()}),
            }
        }
        Ok(status) => json!({"status":"ok","op":"report_mutate","outbox_id":request.outbox_id,
            "mutation_id":request.mutation_id,"delivery":status}),
        Err(error) => json!({"status":"failed","op":"report_mutate","error":error.to_string()}),
    }
}

/// One open mutation per parent serializes every card edit and upload. A claim is held by its
/// owning call until that call ends; a lease expiry never releases it. An unknown attachment
/// completion is final, never repeated, and stops holding the parent once its call has ended.
/// An unknown card update is resolved only by observing its unique block IDs on Slack, never
/// by sending it again. No later edit is claimed before that original update is observed.
async fn perform(pool: &SqlitePool, request: &Request) -> Result<&'static str> {
    request.validate()?;
    if !matches!(request.change, Change::Attachment { .. }) {
        return perform_card(pool, request).await;
    }
    let encoded = serde_json::to_string(request).map_err(|error| HubError::message(error.to_string()))?;
    let prior: Option<(String,String)> = sqlx::query_as("SELECT request_json,status FROM report_mutations WHERE outbox_id=? AND mutation_id=?")
        .bind(request.outbox_id).bind(&request.mutation_id).fetch_optional(pool).await?;
    if let Some((prior, status)) = &prior {
        if *prior != encoded { return Err(HubError::message("mutation identity reused with different content")); }
        if status == "delivered" { return Ok("delivered"); }
        if matches!(request.change, Change::Attachment { .. }) { return settle_attachment(pool, request).await; }
    } else {
        let deferred: Option<String> = sqlx::query_scalar("SELECT request_json FROM report_mutation_deferrals WHERE outbox_id=? AND mutation_id=?")
            .bind(request.outbox_id).bind(&request.mutation_id).fetch_optional(pool).await?;
        if deferred.is_some_and(|stored| stored != encoded) {
            return Err(HubError::message("deferred mutation identity reused with different content"));
        }
    }
    use crate::report_gate::ParentPolicy;
    let mut digest_due = false;
    let (policy, directives) = crate::report_gate::parent_policy_snapshot(pool, request.outbox_id).await?;
    match policy {
        ParentPolicy::Allowed => {},
        ParentPolicy::Deferred { .. } if matches!(request.change, Change::Attachment { .. }) => {
            digest_due = attachment_due(pool, request, chrono::Utc::now()).await?;
            if !digest_due { return Ok("pending"); }
        }
        // Updating a visible card is not a new notification. The receipt check below
        // still holds updates until the parent itself has actually been delivered.
        ParentPolicy::Deferred { .. } => {},
        ParentPolicy::Muted { .. } | ParentPolicy::Cancelled => return Ok("suppressed"),
        ParentPolicy::Ungated => return Err(HubError::message("mutation parent has no gated report")),
    }
    let receipt: Option<(Option<String>, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT posted_channel,posted_ts,blocks_json FROM chat_outbox WHERE id=? AND delivered_at IS NOT NULL AND cancelled_at IS NULL AND platform='slack'")
        .bind(request.outbox_id).fetch_optional(pool).await?;
    let Some((Some(channel), Some(ts), blocks)) = receipt else { return Ok("pending"); };
    let session: String = sqlx::query_scalar("SELECT session_key FROM chat_outbox WHERE id=?")
        .bind(request.outbox_id).fetch_one(pool).await?;
    let thread = session.parse::<crate::chat::session::SessionKey>().ok().and_then(|key| key.thread_id);
    let mut own = Some(prepare(blocks.as_deref(), request).await?);
    loop {
        let mut tx = pool.begin_write().await?;
        // Network-based directive judgment ran above. Recheck durable owner rules and
        // cancellation under the same write lock that claims this side effect.
        match crate::report_gate::check_parent_policy_in(&mut tx, request.outbox_id, &directives).await? {
            ParentPolicy::Muted { .. } | ParentPolicy::Cancelled => return Ok("suppressed"),
            ParentPolicy::Ungated => return Err(HubError::message("mutation parent has no gated report")),
            ParentPolicy::Deferred { .. } if matches!(request.change, Change::Attachment { .. }) && !digest_due => return Ok("pending"),
            ParentPolicy::Allowed | ParentPolicy::Deferred { .. } => {},
        }
        type Head = (String, String, String, Option<String>, Option<String>, Option<i64>);
        let head: Option<Head> = sqlx::query_as(&format!(
            "SELECT mutation_id,request_json,status,owner_token,owner_instance,owner_pid FROM report_mutations WHERE outbox_id=? AND {OPEN} ORDER BY rowid LIMIT 1"))
            .bind(request.outbox_id).fetch_optional(&mut *tx).await?;
        let (flight, target, replay) = match head {
            None => {
                let flight = Flight::new();
                let (token, owner, pid, until) = flight.lease();
                let inserted = sqlx::query("INSERT OR IGNORE INTO report_mutations(outbox_id,mutation_id,request_json,status,owner_token,owner_instance,owner_pid,claim_until) VALUES(?,?,?,'claimed',?,?,?,?)")
                    .bind(request.outbox_id).bind(&request.mutation_id).bind(&encoded)
                    .bind(token).bind(owner).bind(pid).bind(until).execute(&mut *tx).await?.rows_affected();
                // Stored but not open: a concurrent call with this identity settled it.
                if inserted == 0 {
                    let status: String = sqlx::query_scalar("SELECT status FROM report_mutations WHERE outbox_id=? AND mutation_id=?")
                        .bind(request.outbox_id).bind(&request.mutation_id).fetch_one(&mut *tx).await?;
                    return Ok(match status.as_str() {
                        "delivered" => "delivered",
                        "claimed" => "in_flight",
                        _ if matches!(request.change, Change::Attachment { .. }) => "uncertain",
                        _ => "recovering",
                    });
                }
                (flight, request.clone(), false)
            }
            Some((head_id, head_json, status, token, owner, pid)) => {
                let mine = head_id == request.mutation_id;
                let head: Request = serde_json::from_str(&head_json).map_err(|error| HubError::message(error.to_string()))?;
                let card = !matches!(head.change, Change::Attachment { .. });
                // The owning call (here or in another process) still runs, even past its lease.
                if status == "claimed" && owner_live(token.as_deref(), owner.as_deref(), pid) {
                    return Ok(if mine { "in_flight" } else { "pending" });
                }
                if !card {
                    // Its call ended without an outcome. The completion is not idempotent: it
                    // is final, never repeated, and no longer holds the parent.
                    sqlx::query("UPDATE report_mutations SET status='uncertain' WHERE outbox_id=? AND mutation_id=? AND status='claimed' AND owner_token IS ?")
                        .bind(request.outbox_id).bind(&head_id).bind(&token).execute(&mut *tx).await?;
                    tx.commit().await?;
                    if mine { return Ok("uncertain"); }
                    continue;
                }
                // An unknown card update: this call takes the claim only to read it back.
                let flight = Flight::new();
                let (token, owner, pid, until) = flight.lease();
                sqlx::query("UPDATE report_mutations SET status='claimed',owner_token=?,owner_instance=?,owner_pid=?,claim_until=? WHERE outbox_id=? AND mutation_id=?")
                    .bind(token).bind(owner).bind(pid).bind(until)
                    .bind(request.outbox_id).bind(&head_id).execute(&mut *tx).await?;
                (flight, head, true)
            }
        };
        tx.commit().await?;
        let prepared = if replay {
            match prepare(blocks.as_deref(), &target).await {
                Ok(prepared) => prepared,
                Err(error) => {
                    record(pool, &target, &flight.token, "uncertain").await?;
                    return Err(error);
                }
            }
        } else {
            own.take().ok_or_else(|| HubError::message("mutation claimed twice in one call"))?
        };
        let result = if replay {
            reconcile_card(&channel, &ts, thread.as_deref(), prepared).await
        } else {
            transport(&channel, &ts, prepared).await
        };
        let status = match result {
            Ok(true) => "delivered",
            Ok(false) | Err(HubError::SlackRejected { .. } | HubError::SlackNotShared(_)) if !replay => {
                sqlx::query("DELETE FROM report_mutations WHERE outbox_id=? AND mutation_id=? AND status='claimed' AND owner_token=?")
                    .bind(target.outbox_id).bind(&target.mutation_id).bind(&flight.token).execute(pool).await?;
                return Ok("pending");
            }
            // The replay was not applied, but the original may have been.
            Ok(false) | Err(HubError::SlackRejected { .. } | HubError::SlackNotShared(_)) => "uncertain",
            Err(error) => {
                tracing::warn!(%error, mutation_id=%target.mutation_id, "report mutation outcome uncertain");
                "uncertain"
            }
        };
        record(pool, &target, &flight.token, status).await?;
        // Unknown card updates are read back, never replayed; attachments never replay.
        let card = !matches!(target.change, Change::Attachment { .. });
        drop(flight);
        let mine = target.mutation_id == request.mutation_id;
        if !mine && status == "delivered" { continue; }
        return Ok(match status {
            "delivered" => "delivered",
            _ if !mine => "pending",
            _ if card => "recovering",
            _ => "uncertain",
        });
    }
}

/// Stores the outcome of the claim `token` owns. A definite delivery is kept even when another
/// call already judged this owner gone and marked the claim uncertain: a late receipt is truth.
async fn record(pool: &SqlitePool, target: &Request, token: &str, status: &str) -> Result<()> {
    let mut tx = pool.begin_write().await?;
    sqlx::query("UPDATE report_mutations SET status=? WHERE outbox_id=? AND mutation_id=? AND owner_token=?
        AND (status='claimed' OR (?='delivered' AND status='uncertain'))")
        .bind(status).bind(target.outbox_id).bind(&target.mutation_id).bind(token).bind(status)
        .execute(&mut *tx).await?;
    if status == "delivered" {
        sqlx::query("DELETE FROM report_mutation_deferrals WHERE outbox_id=? AND mutation_id=?")
            .bind(target.outbox_id).bind(&target.mutation_id).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}

/// A known attachment: `in_flight` while its owning call runs (in any process), else final
/// `uncertain` unless its receipt was recorded. A released claim (definite rejection) asks again.
async fn settle_attachment(pool: &SqlitePool, request: &Request) -> Result<&'static str> {
    let mut tx = pool.begin_write().await?;
    type OwnerRow = (String, Option<String>, Option<String>, Option<i64>);
    let row: Option<OwnerRow> = sqlx::query_as(
        "SELECT status,owner_token,owner_instance,owner_pid FROM report_mutations WHERE outbox_id=? AND mutation_id=?")
        .bind(request.outbox_id).bind(&request.mutation_id).fetch_optional(&mut *tx).await?;
    let Some((status, token, owner, pid)) = row else { return Ok("pending"); };
    if status == "delivered" { return Ok("delivered"); }
    if status == "claimed" {
        if owner_live(token.as_deref(), owner.as_deref(), pid) { return Ok("in_flight"); }
        sqlx::query("UPDATE report_mutations SET status='uncertain' WHERE outbox_id=? AND mutation_id=? AND status='claimed' AND owner_token IS ?")
            .bind(request.outbox_id).bind(&request.mutation_id).bind(&token).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok("uncertain")
}

async fn attachment_due(pool: &SqlitePool, request: &Request, at: chrono::DateTime<chrono::Utc>) -> Result<bool> {
    let encoded = serde_json::to_string(request).map_err(|error| HubError::message(error.to_string()))?;
    sqlx::query("INSERT OR IGNORE INTO report_mutation_deferrals(outbox_id,mutation_id,request_json,not_before) VALUES(?,?,?,?)")
        .bind(request.outbox_id).bind(&request.mutation_id).bind(&encoded)
        .bind(crate::report_gate::next_digest_slot(at).to_rfc3339()).execute(pool).await?;
    let (stored, due): (String, String) = sqlx::query_as("SELECT request_json,not_before FROM report_mutation_deferrals WHERE outbox_id=? AND mutation_id=?")
        .bind(request.outbox_id).bind(&request.mutation_id).fetch_one(pool).await?;
    if stored != encoded { return Err(HubError::message("deferred mutation identity reused with different content")); }
    let due = chrono::DateTime::parse_from_rfc3339(&due).map_err(|error| HubError::message(error.to_string()))?;
    Ok(at >= due)
}

/// Resumes due attachments from durable hub state, even if the producer never runs again.
pub async fn drain_deferred(pool: &SqlitePool) -> Result<()> {
    let rows: Vec<String> = sqlx::query_scalar("SELECT d.request_json FROM report_mutation_deferrals d
        LEFT JOIN report_mutations m ON m.outbox_id=d.outbox_id AND m.mutation_id=d.mutation_id
        WHERE d.not_before<=? AND m.mutation_id IS NULL ORDER BY d.not_before,d.rowid")
        .bind(chrono::Utc::now().to_rfc3339()).fetch_all(pool).await?;
    for encoded in rows {
        let request: Request = match serde_json::from_str(&encoded) {
            Ok(request) => request,
            Err(error) => {
                tracing::warn!(%error, "invalid deferred attachment retained for inspection");
                continue;
            }
        };
        match perform(pool, &request).await {
            Ok("suppressed") => {
                sqlx::query("DELETE FROM report_mutation_deferrals WHERE outbox_id=? AND mutation_id=?")
                    .bind(request.outbox_id).bind(&request.mutation_id).execute(pool).await?;
            }
            Ok(_) => {},
            Err(error) => tracing::warn!(%error, outbox_id=request.outbox_id, "deferred attachment could not be delivered"),
        }
    }
    Ok(())
}

pub async fn reconcile_cards(pool: &SqlitePool) -> Result<()> {
    // Every target remains scheduled, including confirmed targets: Slack has no write fencing.
    let now = chrono::Utc::now();
    let ids: Vec<i64> = sqlx::query_scalar("SELECT outbox_id FROM report_cards WHERE check_after IS NULL OR check_after<=? ORDER BY check_after,outbox_id LIMIT 5")
        .bind(now.to_rfc3339()).fetch_all(pool).await?;
    for id in ids {
        sqlx::query("UPDATE report_cards SET check_after=? WHERE outbox_id=?")
            .bind((now+chrono::Duration::seconds(60)).to_rfc3339()).bind(id).execute(pool).await?;
        if let Err(error) = sync_card(pool, id).await {
            tracing::warn!(%error, outbox_id=id, "card reconciliation failed");
        }
    }
    Ok(())
}

pub(crate) async fn click_is_current(pool: &SqlitePool, payload: &Value) -> Result<bool> {
    let channel = payload.pointer("/container/channel_id").or_else(|| payload.pointer("/channel/id")).and_then(Value::as_str);
    let ts = payload.pointer("/container/message_ts").or_else(|| payload.pointer("/message/ts")).and_then(Value::as_str);
    let row: Option<(i64, String)> = sqlx::query_as("SELECT c.outbox_id,c.desired_mutation_id FROM report_cards c JOIN chat_outbox o ON o.id=c.outbox_id WHERE o.platform='slack' AND o.posted_channel=? AND o.posted_ts=?")
        .bind(channel).bind(ts).fetch_optional(pool).await?;
    let Some((id, mutation)) = row else { return Ok(true); };
    use sha2::{Digest, Sha256};
    let prefix = format!("ferryx:{id}:{:x}:", Sha256::digest(mutation.as_bytes()));
    let current = payload["actions"].as_array().is_some_and(|actions| !actions.is_empty() && actions.iter().all(|action| {
        action["block_id"].as_str().and_then(|id| id.strip_prefix(&prefix))
            .is_some_and(|index| index.parse::<usize>().is_ok())
    }));
    if !current {
        sqlx::query("UPDATE report_cards SET check_after=NULL WHERE outbox_id=?").bind(id).execute(pool).await?;
    }
    Ok(current)
}

async fn perform_card(pool: &SqlitePool, request: &Request) -> Result<&'static str> {
    let encoded = serde_json::to_string(request).map_err(|e| HubError::message(e.to_string()))?;
    let mut tx = pool.begin_write().await?;
    let deferred: Option<String> = sqlx::query_scalar("SELECT request_json FROM report_mutation_deferrals WHERE outbox_id=? AND mutation_id=?")
        .bind(request.outbox_id).bind(&request.mutation_id).fetch_optional(&mut *tx).await?;
    if deferred.is_some_and(|prior| prior != encoded) {
        return Err(HubError::message("deferred mutation identity reused with different content"));
    }
    let prior: Option<String> = sqlx::query_scalar("SELECT request_json FROM report_mutations WHERE outbox_id=? AND mutation_id=?")
        .bind(request.outbox_id).bind(&request.mutation_id).fetch_optional(&mut *tx).await?;
    if let Some(prior) = prior {
        if prior != encoded { return Err(HubError::message("mutation identity reused with different content")); }
        let delivered: bool = sqlx::query_scalar("SELECT status='delivered' FROM report_mutations WHERE outbox_id=? AND mutation_id=?")
            .bind(request.outbox_id).bind(&request.mutation_id).fetch_one(&mut *tx).await?;
        if delivered { return Ok("delivered"); }
    } else {
        let blocks: Option<String> = sqlx::query_scalar("SELECT blocks_json FROM chat_outbox WHERE id=?")
            .bind(request.outbox_id).fetch_one(&mut *tx).await?;
        prepare(blocks.as_deref(), request).await?;
        let generation: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(desired_gen),0)+1 FROM report_cards WHERE outbox_id=?")
            .bind(request.outbox_id).fetch_one(&mut *tx).await?;
        sqlx::query("INSERT INTO report_mutations(outbox_id,mutation_id,request_json,status,card_gen) VALUES(?,?,?,'accepted',?)")
            .bind(request.outbox_id).bind(&request.mutation_id).bind(&encoded).bind(generation).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO report_cards(outbox_id,desired_gen,desired_mutation_id) VALUES(?,?,?) ON CONFLICT(outbox_id) DO UPDATE SET desired_gen=excluded.desired_gen,desired_mutation_id=excluded.desired_mutation_id,check_after=NULL,check_backoff_secs=0")
            .bind(request.outbox_id).bind(generation).bind(&request.mutation_id).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    let progress = sync_card(pool, request.outbox_id).await?;
    let (status, generation, desired): (String, i64, i64) = sqlx::query_as("SELECT m.status,m.card_gen,c.desired_gen FROM report_mutations m JOIN report_cards c USING(outbox_id) WHERE m.outbox_id=? AND m.mutation_id=?")
        .bind(request.outbox_id).bind(&request.mutation_id).fetch_one(pool).await?;
    Ok(if status == "delivered" { "delivered" } else if generation < desired { "superseded" }
        else if progress == "delivered" { "recovering" } else { progress })
}

async fn sync_card(pool: &SqlitePool, outbox_id: i64) -> Result<&'static str> {
    use crate::report_gate::ParentPolicy;
    let (_, directives) = crate::report_gate::parent_policy_snapshot(pool, outbox_id).await?;
    let mut tx = pool.begin_write().await?;
    match crate::report_gate::check_parent_policy_in(&mut tx, outbox_id, &directives).await? {
        ParentPolicy::Muted { .. } | ParentPolicy::Cancelled => return Ok("suppressed"),
        ParentPolicy::Ungated => return Err(HubError::message("mutation parent has no gated report")),
        _ => {},
    }
    type Writer = (String, Option<String>, Option<String>, Option<i64>);
    let (mutation, token, owner, pid): Writer = sqlx::query_as("SELECT desired_mutation_id,writer_token,writer_instance,writer_pid FROM report_cards WHERE outbox_id=?")
        .bind(outbox_id).fetch_one(&mut *tx).await?;
    if owner_live(token.as_deref(), owner.as_deref(), pid) { return Ok("in_flight"); }
    type Receipt = (String, String, Option<String>, String);
    let receipt: Option<Receipt> = sqlx::query_as("SELECT posted_channel,posted_ts,blocks_json,session_key FROM chat_outbox WHERE id=? AND delivered_at IS NOT NULL AND cancelled_at IS NULL AND posted_channel IS NOT NULL AND posted_ts IS NOT NULL")
        .bind(outbox_id).fetch_optional(&mut *tx).await?;
    let Some((channel, ts, blocks, session)) = receipt else { return Ok("pending"); };
    let encoded: String = sqlx::query_scalar("SELECT request_json FROM report_mutations WHERE outbox_id=? AND mutation_id=?")
        .bind(outbox_id).bind(&mutation).fetch_one(&mut *tx).await?;
    let request: Request = serde_json::from_str(&encoded).map_err(|e| HubError::message(e.to_string()))?;
    let flight = Flight::new();
    let (token, owner, pid, until) = flight.lease();
    sqlx::query("UPDATE report_cards SET writer_token=?,writer_instance=?,writer_pid=?,writer_until=? WHERE outbox_id=?")
        .bind(&token).bind(owner).bind(pid).bind(until).bind(outbox_id).execute(&mut *tx).await?;
    tx.commit().await?;
    let result: Result<bool> = async {
        let thread = session.parse::<crate::chat::session::SessionKey>().ok().and_then(|key| key.thread_id);
        if matches!(reconcile_card(&channel, &ts, thread.as_deref(), prepare(blocks.as_deref(), &request).await?).await, Ok(true)) { return Ok(true); }
        let desired: String = sqlx::query_scalar("SELECT desired_mutation_id FROM report_cards WHERE outbox_id=?")
            .bind(outbox_id).fetch_one(pool).await?;
        if desired != mutation { return Ok(false); }
        transport(&channel, &ts, prepare(blocks.as_deref(), &request).await?).await
    }.await;
    let delivered = matches!(result, Ok(true));
    let mut tx = pool.begin_write().await?;
    if delivered {
        sqlx::query("UPDATE report_mutations SET status='delivered' WHERE outbox_id=? AND mutation_id=?")
            .bind(outbox_id).bind(&mutation).execute(&mut *tx).await?;
    }
    sqlx::query("UPDATE report_cards SET writer_token=NULL,writer_instance=NULL,writer_pid=NULL,writer_until=NULL,check_after=? WHERE outbox_id=? AND writer_token=?")
        .bind((chrono::Utc::now()+chrono::Duration::seconds(60)).to_rfc3339()).bind(outbox_id).bind(&token).execute(&mut *tx).await?;
    tx.commit().await?;
    if let Err(error) = result { tracing::warn!(%error, outbox_id, "card state remains unconfirmed"); }
    Ok(if delivered { "delivered" } else { "recovering" })
}

enum Prepared {
    Attachment { bytes: Vec<u8>, name: String, title: String },
    Blocks(Vec<Value>),
}

async fn prepare(blocks: Option<&str>, request: &Request) -> Result<Prepared> {
    let change = &request.change;
    match change {
        Change::Attachment { path, name, title } => {
            let bytes = tokio::fs::read(path).await?;
            Ok(Prepared::Attachment { bytes, name: name.clone(), title: title.clone() })
        }
        Change::ActionRow { .. } | Change::RestoreActions => {
            let mut blocks: Vec<Value> = serde_json::from_str(blocks.ok_or_else(|| HubError::message("parent has no card blocks"))?)
                .map_err(|error| HubError::message(error.to_string()))?;
            if let Change::ActionRow { row } = change {
                let index = blocks.iter().position(|block| block["type"] == "actions")
                    .ok_or_else(|| HubError::message("parent has no actions"))?;
                blocks[index] = row.clone();
            }
            use sha2::{Digest, Sha256};
            let marker = format!("ferryx:{}:{:x}", request.outbox_id, Sha256::digest(request.mutation_id.as_bytes()));
            for (index, block) in blocks.iter_mut().enumerate() {
                block["block_id"] = json!(format!("{marker}:{index}"));
            }
            Ok(Prepared::Blocks(blocks))
        }
    }
}

async fn reconcile_card(channel: &str, ts: &str, thread: Option<&str>, prepared: Prepared) -> Result<bool> {
    let Prepared::Blocks(expected) = prepared else {
        return Err(HubError::message("attachment completion cannot be reconciled as a card"));
    };
    let observed = match thread {
        Some(root) => crate::chat::slack::read_own_thread_message(channel, root, ts).await?,
        None => crate::chat::slack::read_own_message(channel, ts).await?,
    };
    match observed {
        crate::chat::slack::OwnMessage::Found(message) => {
            let actual = message.blocks.unwrap_or_default();
            Ok(!expected.is_empty() && actual.len() == expected.len()
                && actual.iter().zip(&expected).all(|(actual, expected)| actual["block_id"] == expected["block_id"]))
        }
        _ => Ok(false),
    }
}

async fn transport(channel: &str, ts: &str, prepared: Prepared) -> Result<bool> {
    match prepared {
        Prepared::Attachment { bytes, name, title } => {
            #[cfg(feature = "hub-slack")]
            { crate::chat::slack::upload_external(channel, Some(ts), &name, bytes, &title).await }
            #[cfg(not(feature = "hub-slack"))]
            { let _ = (channel, ts, bytes, name, title); Ok(false) }
        }
        Prepared::Blocks(blocks) => {
            #[cfg(feature = "hub-slack")]
            { crate::chat::slack::call_method("chat.update", json!({"channel":channel,"ts":ts,"blocks":blocks})).await }
            #[cfg(not(feature = "hub-slack"))]
            { let _ = (channel, ts, blocks); Ok(false) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "hub-slack")]
    fn without_delivery_ids(mut blocks: Value) -> Value {
        for block in blocks.as_array_mut().unwrap() {
            let id = block.as_object_mut().unwrap().remove("block_id").unwrap();
            assert!(id.as_str().unwrap().starts_with("ferryx:"));
        }
        blocks
    }

    async fn parent(pool: &SqlitePool) -> Request {
        let report = crate::report_gate::Report::new("parent", "producer", "card:1",
            crate::report_gate::Salience::Decision, "승인해 주세요.");
        let id = crate::report_gate::queue(pool, "owner", &report, false).await.unwrap().1.unwrap();
        sqlx::query("UPDATE chat_outbox SET delivered_at='receipt',posted_channel='C1',posted_ts='1.0' WHERE id=?")
            .bind(id).execute(pool).await.unwrap();
        Request { outbox_id: id, mutation_id: "attachment:1".into(), change: Change::Attachment {
            path: std::env::temp_dir().join(format!("missing-report-attachment-{}", uuid::Uuid::new_v4()))
                .to_string_lossy().into_owned(), name: "preview.png".into(), title: "Preview".into(),
        } }
    }

    #[tokio::test]
    async fn mute_after_parent_delivery_blocks_followups_before_transport() {
        let pool = crate::storage::memory().await.unwrap();
        let request = parent(&pool).await;
        let scope = crate::report_rules::scope_from_alert("producer", None, None, Some("card:1"));
        crate::report_rules::record(&pool, &scope, crate::report_rules::Effect::Mute, "stop", None).await.unwrap();
        assert_eq!(perform(&pool, &request).await.unwrap(), "suppressed");
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM report_mutations").fetch_one(&pool).await.unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn delivered_receipt_survives_mute_and_rejects_changed_identity() {
        let pool = crate::storage::memory().await.unwrap();
        let mut request = parent(&pool).await;
        sqlx::query("INSERT INTO report_mutations(outbox_id,mutation_id,request_json,status) VALUES(?,?,?,'delivered')")
            .bind(request.outbox_id).bind(&request.mutation_id)
            .bind(serde_json::to_string(&request).unwrap()).execute(&pool).await.unwrap();
        let scope = crate::report_rules::scope_from_alert("producer", None, None, Some("card:1"));
        crate::report_rules::record(&pool, &scope, crate::report_rules::Effect::Mute, "stop", None).await.unwrap();
        assert_eq!(perform(&pool, &request).await.unwrap(), "delivered");
        request.change = Change::RestoreActions;
        assert!(perform(&pool, &request).await.unwrap_err().to_string().contains("identity reused"));
    }

    #[tokio::test]
    async fn deferred_identity_is_checked_even_without_a_digest_rule() {
        let pool = crate::storage::memory().await.unwrap();
        let mut request = parent(&pool).await;
        attachment_due(&pool, &request, chrono::Utc::now()).await.unwrap();
        request.change = Change::RestoreActions;
        assert!(perform(&pool, &request).await.unwrap_err().to_string().contains("identity reused"));
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM report_mutations").fetch_one(&pool).await.unwrap();
        assert_eq!(count, 0);
    }


    #[tokio::test]
    async fn corrupt_deferral_does_not_block_a_later_muted_attachment() {
        let pool = crate::storage::memory().await.unwrap();
        let request = parent(&pool).await;
        for (id, body) in [("corrupt", "{".to_owned()),
            (request.mutation_id.as_str(), serde_json::to_string(&request).unwrap())] {
            sqlx::query("INSERT INTO report_mutation_deferrals(outbox_id,mutation_id,request_json,not_before) VALUES(?,?,?,'2000-01-01T00:00:00+00:00')")
                .bind(request.outbox_id).bind(id).bind(body).execute(&pool).await.unwrap();
        }
        let scope = crate::report_rules::scope_from_alert("producer", None, None, Some("card:1"));
        crate::report_rules::record(&pool, &scope, crate::report_rules::Effect::Mute, "stop", None).await.unwrap();
        drain_deferred(&pool).await.unwrap();
        let remaining: Vec<String> = sqlx::query_scalar("SELECT mutation_id FROM report_mutation_deferrals")
            .fetch_all(&pool).await.unwrap();
        assert_eq!(remaining, vec!["corrupt"]);
    }
    #[tokio::test]
    async fn deferred_attachment_keeps_its_original_digest_slot() {
        let pool = crate::storage::memory().await.unwrap();
        let mut request = parent(&pool).await;
        let at = chrono::DateTime::parse_from_rfc3339("2026-10-30T08:00:00Z").unwrap().to_utc();
        assert!(!attachment_due(&pool, &request, at).await.unwrap());
        assert!(!attachment_due(&pool, &request, at + chrono::Duration::minutes(30)).await.unwrap());
        assert!(attachment_due(&pool, &request, at + chrono::Duration::hours(1)).await.unwrap());
        assert!(attachment_due(&pool, &request, at + chrono::Duration::days(1)).await.unwrap());
        request.change = Change::RestoreActions;
        assert!(attachment_due(&pool, &request, at).await.is_err());
    }

    #[tokio::test]
    async fn local_preparation_failure_does_not_claim_delivery() {
        let pool = crate::storage::memory().await.unwrap();
        let mut request = parent(&pool).await;
        assert!(perform(&pool, &request).await.is_err());
        request.change = Change::RestoreActions;
        assert!(perform(&pool, &request).await.is_err());
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM report_mutations")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(count, 0);
        request.outbox_id += 1;
        assert_eq!(reply(&pool, &request).await["status"], "failed");
    }

    #[tokio::test]
    async fn claimed_mutation_is_not_replayed_and_identity_cannot_change() {
        let pool = crate::storage::memory().await.unwrap();
        let mut request = parent(&pool).await;
        sqlx::query("INSERT INTO report_mutations(outbox_id,mutation_id,request_json,status) VALUES(?,?,?,'claimed')")
            .bind(request.outbox_id).bind(&request.mutation_id).bind(serde_json::to_string(&request).unwrap())
            .execute(&pool).await.unwrap();
        assert_eq!(perform(&pool, &request).await.unwrap(), "uncertain");
        let status = crate::control::report_status_reply(&pool, request.outbox_id).await;
        assert_eq!(status["mutations"], json!([{"mutation_id":"attachment:1","delivery":"uncertain"}]));
        request.change = Change::RestoreActions;
        assert!(perform(&pool, &request).await.is_err());
        sqlx::query("UPDATE chat_outbox SET blocks_json='[]' WHERE id=?")
            .bind(request.outbox_id).execute(&pool).await.unwrap();
        request.mutation_id = "restore:2".into();
        // The attachment's call ended without an outcome: it is final and no longer holds the
        // parent, so a later edit proceeds (here its transport is not configured, so it asks again).
        assert_eq!(perform(&pool, &request).await.unwrap(), "recovering");
        assert_eq!(reply(&pool, &request).await["blocked_by"], "restore:2");
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM report_mutations")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(count, 2);
    }

    async fn claim_as(pool: &SqlitePool, request: &Request, token: &str, owner: &str, pid: i64, until: &str) {
        sqlx::query("INSERT INTO report_mutations(outbox_id,mutation_id,request_json,status,owner_token,owner_instance,owner_pid,claim_until) VALUES(?,?,?,'claimed',?,?,?,?)")
            .bind(request.outbox_id).bind(&request.mutation_id).bind(serde_json::to_string(request).unwrap())
            .bind(token).bind(owner).bind(pid).bind(until).execute(pool).await.unwrap();
    }

    #[tokio::test]
    async fn terminal_uncertain_attachment_does_not_hold_later_mutations() {
        let pool = crate::storage::memory().await.unwrap();
        let attachment = parent(&pool).await;
        sqlx::query("INSERT INTO report_mutations(outbox_id,mutation_id,request_json,status) VALUES(?,?,?,'uncertain')")
            .bind(attachment.outbox_id).bind(&attachment.mutation_id).bind(serde_json::to_string(&attachment).unwrap())
            .execute(&pool).await.unwrap();
        sqlx::query("UPDATE chat_outbox SET blocks_json='[]' WHERE id=?")
            .bind(attachment.outbox_id).execute(&pool).await.unwrap();
        let mut later = attachment.clone();
        later.mutation_id = "restore:2".into();
        later.change = Change::RestoreActions;
        assert_eq!(perform(&pool, &later).await.unwrap(), "recovering");
        let mut third = later.clone();
        third.mutation_id = "restore:3".into();
        assert_eq!(reply(&pool, &third).await["blocked_by"], "restore:3");
        // The final attachment is never repeated and stays uncertain.
        assert_eq!(perform(&pool, &attachment).await.unwrap(), "uncertain");
        let statuses = statuses(&pool, attachment.outbox_id).await.unwrap();
        assert_eq!(statuses, vec![json!({"mutation_id":"attachment:1","delivery":"uncertain"}),
            json!({"mutation_id":"restore:2","delivery":"superseded"}),
            json!({"mutation_id":"restore:3","delivery":"pending","progress":"recovering"})]);
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM report_mutations").fetch_one(&pool).await.unwrap();
        assert_eq!(count, 3);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn live_claim_of_another_process_is_not_abandoned_even_past_its_lease() {
        let pool = crate::storage::memory().await.unwrap();
        let attachment = parent(&pool).await;
        // The test runner's parent process stands in for another live hub process.
        let other = i64::from(std::os::unix::process::parent_id());
        let expired = (chrono::Utc::now() - chrono::Duration::hours(1)).to_rfc3339();
        claim_as(&pool, &attachment, "other-call", "other-instance", other, &expired).await;
        let own = reply(&pool, &attachment).await;
        assert_eq!((own["delivery"].as_str(), own["progress"].as_str()), (Some("pending"), Some("in_flight")));
        sqlx::query("UPDATE chat_outbox SET blocks_json='[]' WHERE id=?")
            .bind(attachment.outbox_id).execute(&pool).await.unwrap();
        let mut later = attachment.clone();
        later.mutation_id = "restore:2".into();
        later.change = Change::RestoreActions;
        // Cards proceed independently, but expiry never abandons the live attachment.
        assert_eq!(perform(&pool, &later).await.unwrap(), "recovering");
        assert_eq!(reply(&pool, &later).await["blocked_by"], "restore:2");
        assert_eq!(statuses(&pool, attachment.outbox_id).await.unwrap(),
            vec![json!({"mutation_id":"attachment:1","delivery":"pending","progress":"in_flight","stalled":true}),
                json!({"mutation_id":"restore:2","delivery":"pending","progress":"recovering"})]);
        let status: String = sqlx::query_scalar("SELECT status FROM report_mutations WHERE mutation_id='attachment:1'").fetch_one(&pool).await.unwrap();
        assert_eq!(status, "claimed");
        // Once that process is gone (pid 0 is never a live owner), the completion is final.
        sqlx::query("UPDATE report_mutations SET owner_pid=0").execute(&pool).await.unwrap();
        assert_eq!(perform(&pool, &attachment).await.unwrap(), "uncertain");
        assert_eq!(statuses(&pool, attachment.outbox_id).await.unwrap(),
            vec![json!({"mutation_id":"attachment:1","delivery":"uncertain"}),
                json!({"mutation_id":"restore:2","delivery":"pending","progress":"recovering"})]);
    }

    #[tokio::test]
    async fn card_generation_migration_preserves_attachment_and_latest_card() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new().max_connections(1)
            .connect("sqlite::memory:").await.unwrap();
        for migration in sqlx::migrate!("./migrations").iter().filter(|m| m.version <= 42) {
            sqlx::raw_sql(&migration.sql).execute(&pool).await.unwrap();
        }
        let attachment = parent(&pool).await;
        let mut card = attachment.clone();
        card.mutation_id = "card:1".into();
        card.change = Change::RestoreActions;
        for request in [&attachment, &card] {
            sqlx::query("INSERT INTO report_mutations(outbox_id,mutation_id,request_json,status) VALUES(?,?,?,'uncertain')")
                .bind(request.outbox_id).bind(&request.mutation_id)
                .bind(serde_json::to_string(request).unwrap()).execute(&pool).await.unwrap();
        }
        sqlx::raw_sql(include_str!("../migrations/0043_report_card_generations.sql"))
            .execute(&pool).await.unwrap();
        let rows: Vec<(String, String, Option<i64>)> = sqlx::query_as(
            "SELECT mutation_id,status,card_gen FROM report_mutations ORDER BY rowid")
            .fetch_all(&pool).await.unwrap();
        assert_eq!(rows, vec![(attachment.mutation_id, "uncertain".into(), None),
            ("card:1".into(), "accepted".into(), Some(1))]);
        let desired: (i64, String) = sqlx::query_as("SELECT desired_gen,desired_mutation_id FROM report_cards")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(desired, (1, "card:1".into()));
    }

    #[tokio::test]
    async fn live_card_writer_does_not_block_acceptance_of_new_target() {
        let pool = crate::storage::memory().await.unwrap();
        let mut card = parent(&pool).await;
        sqlx::query("UPDATE chat_outbox SET blocks_json='[]' WHERE id=?").bind(card.outbox_id).execute(&pool).await.unwrap();
        card.change = Change::RestoreActions;
        card.mutation_id = "card:1".into();
        assert_eq!(perform(&pool, &card).await.unwrap(), "recovering");
        let flight = Flight::new();
        let (token, owner, pid, until) = flight.lease();
        sqlx::query("UPDATE report_cards SET writer_token=?,writer_instance=?,writer_pid=?,writer_until=? WHERE outbox_id=?")
            .bind(token).bind(owner).bind(pid).bind(until).bind(card.outbox_id).execute(&pool).await.unwrap();
        let older = card.clone();
        card.mutation_id = "card:2".into();
        assert_eq!(perform(&pool, &card).await.unwrap(), "in_flight");
        assert_eq!(perform(&pool, &older).await.unwrap(), "superseded");
        drop(flight);
        assert_eq!(perform(&pool, &card).await.unwrap(), "recovering");
    }

    #[tokio::test]
    async fn live_claim_is_seen_from_a_separate_pool_on_the_same_database() {
        let path = std::env::temp_dir().join(format!("report-mutation-{}.db", uuid::Uuid::new_v4()));
        let first = crate::storage::connect(&path).await.unwrap();
        let attachment = parent(&first).await;
        let flight = Flight::new();
        let (token, owner, pid, until) = flight.lease();
        claim_as(&first, &attachment, &token, owner, pid, &until).await;
        let second = crate::storage::connect(&path).await.unwrap();
        assert_eq!(perform(&second, &attachment).await.unwrap(), "in_flight");
        assert_eq!(statuses(&second, attachment.outbox_id).await.unwrap(),
            vec![json!({"mutation_id":"attachment:1","delivery":"pending","progress":"in_flight","stalled":false})]);
        drop(flight);
        assert_eq!(perform(&second, &attachment).await.unwrap(), "uncertain");
        first.close().await;
        second.close().await;
        for suffix in ["", "-wal", "-shm", ".open-lock"] {
            let mut name = path.as_os_str().to_owned();
            name.push(suffix);
            let _ = std::fs::remove_file(std::path::PathBuf::from(name));
        }
    }

    #[tokio::test]
    async fn late_owner_receipt_is_kept_after_its_claim_was_judged_uncertain() {
        let pool = crate::storage::memory().await.unwrap();
        let attachment = parent(&pool).await;
        let flight = Flight::new();
        let (token, owner, pid, until) = flight.lease();
        claim_as(&pool, &attachment, &token, owner, pid, &until).await;
        // Another observer concluded the owner was gone and finalized the claim as unknown.
        sqlx::query("UPDATE report_mutations SET status='uncertain'").execute(&pool).await.unwrap();
        // Only the claim's own owner may record; a stranger's receipt changes nothing.
        record(&pool, &attachment, "stranger", "delivered").await.unwrap();
        assert_eq!(perform(&pool, &attachment).await.unwrap(), "uncertain");
        record(&pool, &attachment, &token, "uncertain").await.unwrap();
        record(&pool, &attachment, &token, "delivered").await.unwrap();
        drop(flight);
        assert_eq!(perform(&pool, &attachment).await.unwrap(), "delivered");
        assert_eq!(reply(&pool, &attachment).await["delivery"], "delivered");
        assert_eq!(statuses(&pool, attachment.outbox_id).await.unwrap(),
            vec![json!({"mutation_id":"attachment:1","delivery":"delivered"})]);
    }

    #[tokio::test]
    #[cfg(feature = "hub-slack")]
    async fn http_attachment_uses_parent_thread_and_is_not_uploaded_twice() {
        use axum::extract::State;
        type UploadState = (
            std::sync::Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
            std::sync::Arc<std::sync::Mutex<Vec<Value>>>,
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let received = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Vec<u8>>::new()));
        let completed = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
        let app = axum::Router::new()
            .route("/files.getUploadURLExternal", axum::routing::post({
                let url = format!("{base}/bytes");
                move || { let url = url.clone(); async move { axum::Json(json!({"ok":true,"file_id":"F1","upload_url":url})) } }
            }))
            .route("/bytes", axum::routing::post(|State((bytes, _)): State<UploadState>, body: axum::body::Bytes| async move {
                bytes.lock().unwrap().push(body.to_vec()); axum::http::StatusCode::OK
            }))
            .route("/files.completeUploadExternal", axum::routing::post(|State((_, calls)): State<UploadState>, axum::Json(body): axum::Json<Value>| async move {
                calls.lock().unwrap().push(body); axum::Json(json!({"ok":true}))
            })).with_state((received.clone(), completed.clone()));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        let pool = crate::storage::memory().await.unwrap();
        let mut request = parent(&pool).await;
        let path = std::env::temp_dir().join(format!("report-attachment-{}", uuid::Uuid::new_v4()));
        tokio::fs::write(&path, b"preview-bytes").await.unwrap();
        request.change = Change::Attachment { path: path.to_string_lossy().into_owned(), name: "preview.png".into(), title: "Preview".into() };
        let scope = crate::report_rules::scope_from_alert("producer", None, None, Some("card:1"));
        crate::report_rules::record(&pool, &scope, crate::report_rules::Effect::Digest,
            "digest attachments", None).await.unwrap();
        let before = chrono::Utc::now() - chrono::Duration::days(2);
        assert!(!attachment_due(&pool, &request, before).await.unwrap());
        crate::chat::slack::with_test_api(base, "xoxb-test".into(), async {
            crate::owner_alert::delivery_tick(&pool, "owner").await;
            crate::owner_alert::delivery_tick(&pool, "owner").await;
            assert_eq!(perform(&pool, &request).await.unwrap(), "delivered");
            assert_eq!(perform(&pool, &request).await.unwrap(), "delivered");
        }).await;
        tokio::fs::remove_file(path).await.unwrap();
        server.abort();
        assert_eq!(*received.lock().unwrap(), vec![b"preview-bytes".to_vec()]);
        let deferred: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM report_mutation_deferrals")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(deferred, 0);
        let completed = completed.lock().unwrap();
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0]["channel_id"], "C1");
        assert_eq!(completed[0]["thread_ts"], "1.0");
    }

    #[tokio::test]
    #[cfg(feature = "hub-slack")]
    async fn http_card_updates_use_parent_receipt_and_restore_original_buttons_once() {
        async fn record(axum::extract::State(calls): axum::extract::State<std::sync::Arc<std::sync::Mutex<Vec<Value>>>>,
            axum::Json(body): axum::Json<Value>) -> axum::Json<Value> {
            calls.lock().unwrap().push(body);
            axum::Json(json!({"ok":true}))
        }
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
        let app = axum::Router::new().route("/chat.update", axum::routing::post(record)).with_state(calls.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        let pool = crate::storage::memory().await.unwrap();
        let mut request = parent(&pool).await;
        let original = json!([{"type":"section","text":{"type":"plain_text","text":"approval"}},
            {"type":"actions","elements":[{"type":"button","action_id":"approve:1"}]}]);
        sqlx::query("UPDATE chat_outbox SET blocks_json=? WHERE id=?")
            .bind(original.to_string()).bind(request.outbox_id).execute(&pool).await.unwrap();
        request.mutation_id = "processing:1".into();
        let scope = crate::report_rules::scope_from_alert("producer", None, None, Some("card:1"));
        crate::report_rules::record(&pool, &scope, crate::report_rules::Effect::Digest,
            "digest new reports", None).await.unwrap();
        let status = json!({"type":"context","elements":[{"type":"plain_text","text":"processing"}]});
        request.change = Change::ActionRow { row: status.clone() };
        crate::chat::slack::with_test_api(base, "xoxb-test".into(), async {
            assert_eq!(perform(&pool, &request).await.unwrap(), "delivered");
            assert_eq!(perform(&pool, &request).await.unwrap(), "delivered");
            request.mutation_id = "restore:1".into();
            request.change = Change::RestoreActions;
            assert_eq!(perform(&pool, &request).await.unwrap(), "delivered");
        }).await;
        server.abort();
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0]["channel"], "C1");
        assert_eq!(calls[0]["ts"], "1.0");
        assert_eq!(without_delivery_ids(calls[0]["blocks"].clone())[1], status);
        assert_eq!(without_delivery_ids(calls[1]["blocks"].clone()), original);
    }

    #[tokio::test]
    #[cfg(feature = "hub-slack")]
    async fn definite_update_rejection_preserves_desired_state_for_retry() {
        use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
        let accept = Arc::new(AtomicBool::new(false));
        let app = axum::Router::new().route("/chat.update", axum::routing::post({
            let accept = accept.clone();
            move || { let accept = accept.clone(); async move {
                axum::Json(if accept.load(Ordering::SeqCst) { json!({"ok":true}) }
                    else { json!({"ok":false,"error":"ratelimited"}) })
            }}
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        let pool = crate::storage::memory().await.unwrap();
        let mut request = parent(&pool).await;
        sqlx::query("UPDATE chat_outbox SET blocks_json='[]' WHERE id=?")
            .bind(request.outbox_id).execute(&pool).await.unwrap();
        request.change = Change::RestoreActions;
        crate::chat::slack::with_test_api(base, "test-token".into(), async {
            assert_eq!(perform(&pool, &request).await.unwrap(), "recovering");
            let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM report_mutations")
                .fetch_one(&pool).await.unwrap();
            assert_eq!(count, 1);
            accept.store(true, Ordering::SeqCst);
            assert_eq!(perform(&pool, &request).await.unwrap(), "delivered");
        }).await;
        server.abort();
    }

    #[tokio::test]
    #[cfg(feature = "hub-slack")]
    async fn rejected_upload_ticket_does_not_leave_an_uncertain_claim() {
        let app = axum::Router::new().route("/files.getUploadURLExternal",
            axum::routing::post(|| async { axum::Json(json!({"ok":false,"error":"ratelimited"})) }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        let pool = crate::storage::memory().await.unwrap();
        let request = parent(&pool).await;
        let Change::Attachment { path, .. } = &request.change else { unreachable!() };
        tokio::fs::write(path, b"preview").await.unwrap();
        crate::chat::slack::with_test_api(base, "test-token".into(), async {
            assert_eq!(perform(&pool, &request).await.unwrap(), "pending");
            let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM report_mutations")
                .fetch_one(&pool).await.unwrap();
            assert_eq!(count, 0);
        }).await;
        tokio::fs::remove_file(path).await.unwrap();
        server.abort();
    }

    #[tokio::test]
    #[cfg(feature = "hub-slack")]
    async fn ambiguous_updates_recover_independently_without_false_receipts() {
        use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
        let calls = Arc::new(AtomicUsize::new(0));
        let app = axum::Router::new().route("/chat.update", axum::routing::post({
            let calls = calls.clone();
            move || { let calls = calls.clone(); async move {
                calls.fetch_add(1, Ordering::SeqCst);
                "incomplete response"
            }}
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        let pool = crate::storage::memory().await.unwrap();
        let mut request = parent(&pool).await;
        sqlx::query("UPDATE chat_outbox SET blocks_json='[]' WHERE id=?")
            .bind(request.outbox_id).execute(&pool).await.unwrap();
        request.change = Change::RestoreActions;
        crate::chat::slack::with_test_api(base, "test-token".into(), async {
            // The unknown update is not repeated while it rests, and it names itself as the blocker.
            assert_eq!(perform(&pool, &request).await.unwrap(), "recovering");
            let own = reply(&pool, &request).await;
            assert_eq!((own["delivery"].as_str(), own["progress"].as_str()), (Some("pending"), Some("recovering")));
            assert_eq!(own["blocked_by"], request.mutation_id);
            let previous = request.clone();
            request.mutation_id = "later-update".into();
            let result = reply(&pool, &request).await;
            assert_eq!(result["delivery"], "pending");
            assert_eq!(result["blocked_by"], "later-update");
            assert_eq!(perform(&pool, &previous).await.unwrap(), "superseded");
        }).await;
        assert_eq!(calls.load(Ordering::SeqCst), 4);
        server.abort();
    }


    #[tokio::test]
    async fn in_flight_attachment_is_pending_until_its_call_ends_then_uncertain() {
        let pool = crate::storage::memory().await.unwrap();
        let mut request = parent(&pool).await;
        sqlx::query("INSERT INTO report_mutations(outbox_id,mutation_id,request_json,status) VALUES(?,?,?,'claimed')")
            .bind(request.outbox_id).bind(&request.mutation_id).bind(serde_json::to_string(&request).unwrap())
            .execute(&pool).await.unwrap();
        sqlx::query("DELETE FROM report_mutations").execute(&pool).await.unwrap();
        // The upload is still running in this process: the producer must ask again, not give up.
        let flight = Flight::new();
        let (token, owner, pid, until) = flight.lease();
        claim_as(&pool, &request, &token, owner, pid, &until).await;
        let own = reply(&pool, &request).await;
        assert_eq!((own["delivery"].as_str(), own["progress"].as_str()), (Some("pending"), Some("in_flight")));
        let receipt = crate::control::report_status_reply(&pool, request.outbox_id).await;
        assert_eq!(receipt["mutations"], json!([{"mutation_id":"attachment:1","delivery":"pending","progress":"in_flight","stalled":false}]));
        let status: String = sqlx::query_scalar("SELECT status FROM report_mutations WHERE mutation_id='attachment:1'").fetch_one(&pool).await.unwrap();
        assert_eq!(status, "claimed");
        let attachment = request.clone();
        sqlx::query("UPDATE chat_outbox SET blocks_json='[]' WHERE id=?")
            .bind(request.outbox_id).execute(&pool).await.unwrap();
        request.mutation_id = "restore:2".into();
        request.change = Change::RestoreActions;
        let sibling = reply(&pool, &request).await;
        assert_eq!((sibling["delivery"].as_str(), sibling["blocked_by"].as_str()), (Some("pending"), Some("restore:2")));
        assert_eq!(sibling["progress"], "recovering");
        // The call ended without an outcome (restart): the completion is final and unknown.
        drop(flight);
        assert_eq!(perform(&pool, &attachment).await.unwrap(), "uncertain");
        assert_eq!(perform(&pool, &attachment).await.unwrap(), "uncertain");
        let status: String = sqlx::query_scalar("SELECT status FROM report_mutations WHERE mutation_id='attachment:1'").fetch_one(&pool).await.unwrap();
        assert_eq!(status, "uncertain");
        // A non-idempotent attachment is never replayed; once final it no longer holds the parent.
        assert_eq!(reply(&pool, &request).await["blocked_by"], "restore:2");
        assert_eq!(perform(&pool, &attachment).await.unwrap(), "uncertain");
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM report_mutations").fetch_one(&pool).await.unwrap();
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn accepted_card_after_restart_can_be_superseded() {
        let pool = crate::storage::memory().await.unwrap();
        let mut request = parent(&pool).await;
        sqlx::query("UPDATE chat_outbox SET blocks_json='[]' WHERE id=?")
            .bind(request.outbox_id).execute(&pool).await.unwrap();
        request.mutation_id = "restore:1".into();
        request.change = Change::RestoreActions;
        sqlx::query("INSERT INTO report_mutations(outbox_id,mutation_id,request_json,status,card_gen) VALUES(?,?,?,'accepted',1)")
            .bind(request.outbox_id).bind(&request.mutation_id).bind(serde_json::to_string(&request).unwrap())
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT OR IGNORE INTO report_cards(outbox_id,desired_gen,desired_mutation_id) VALUES(?,1,?)")
            .bind(request.outbox_id).bind(&request.mutation_id).execute(&pool).await.unwrap();
        assert_eq!(perform(&pool, &request).await.unwrap(), "recovering");
        assert_eq!(perform(&pool, &request).await.unwrap(), "recovering");
        let status: String = sqlx::query_scalar("SELECT status FROM report_mutations").fetch_one(&pool).await.unwrap();
        assert_eq!(status, "accepted");
        let mut later = request.clone();
        later.mutation_id = "restore:2".into();
        assert_eq!(reply(&pool, &later).await["blocked_by"], "restore:2");
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM report_mutations").fetch_one(&pool).await.unwrap();
        assert_eq!(count, 2);
    }

    #[tokio::test]
    #[cfg(feature = "hub-slack")]
    async fn ambiguous_update_is_superseded_and_stale_click_is_rejected() {
        use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
        type UpdateState = (Arc<Mutex<Vec<Value>>>, Arc<AtomicBool>);
        async fn update(axum::extract::State((calls, accept)): axum::extract::State<UpdateState>,
            axum::Json(body): axum::Json<Value>) -> axum::response::Response {
            use axum::response::IntoResponse;
            calls.lock().unwrap().push(body);
            if accept.load(Ordering::SeqCst) { axum::Json(json!({"ok":true})).into_response() }
            else { "incomplete response".into_response() }
        }
        let calls = Arc::new(Mutex::new(Vec::<Value>::new()));
        let accept = Arc::new(AtomicBool::new(false));
        let app = axum::Router::new().route("/chat.update", axum::routing::post(update))
            .route("/auth.test", axum::routing::post(|| async { axum::Json(json!({"ok":true,"user_id":"B1"})) }))
            .route("/conversations.history", axum::routing::post({
                let calls = calls.clone();
                let accept = accept.clone();
                move || { let calls = calls.clone(); let accept = accept.clone(); async move {
                    let blocks = if accept.load(Ordering::SeqCst) {
                        calls.lock().unwrap().first().unwrap()["blocks"].clone()
                    } else { json!([]) };
                    axum::Json(json!({"ok":true,"messages":[{"ts":"1.0","user":"B1","blocks":blocks}]}))
                }}
            }))
            .with_state((calls.clone(), accept.clone()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        let pool = crate::storage::memory().await.unwrap();
        let mut request = parent(&pool).await;
        let original = json!([{"type":"section","text":{"type":"plain_text","text":"approval"}},
            {"type":"actions","elements":[{"type":"button","action_id":"approve:1"}]}]);
        sqlx::query("UPDATE chat_outbox SET blocks_json=? WHERE id=?")
            .bind(original.to_string()).bind(request.outbox_id).execute(&pool).await.unwrap();
        let processing = json!({"type":"context","elements":[{"type":"plain_text","text":"processing"}]});
        request.mutation_id = "processing:1".into();
        request.change = Change::ActionRow { row: processing.clone() };
        let first = request.clone();
        let mut restore = request.clone();
        restore.mutation_id = "restore:1".into();
        restore.change = Change::RestoreActions;
        crate::chat::slack::with_test_api(base, "test-token".into(), async {
            assert_eq!(perform(&pool, &first).await.unwrap(), "recovering");
            assert_eq!(perform(&pool, &restore).await.unwrap(), "recovering");
            assert_eq!(calls.lock().unwrap().len(), 2);
            accept.store(true, Ordering::SeqCst);

            // An unconfirmed predecessor never holds the newer desired state.
            assert_eq!(perform(&pool, &restore).await.unwrap(), "delivered");
            assert_eq!(perform(&pool, &first).await.unwrap(), "superseded");
            assert_eq!(perform(&pool, &restore).await.unwrap(), "delivered");
            let (old_id, new_id) = {
                let calls = calls.lock().unwrap();
                (calls[0]["blocks"][0]["block_id"].clone(), calls[2]["blocks"][0]["block_id"].clone())
            };
            let click = |id| json!({"container":{"channel_id":"C1","message_ts":"1.0"},"actions":[{"block_id":id}]});
            assert!(!click_is_current(&pool, &click(old_id)).await.unwrap());
            assert!(click_is_current(&pool, &click(new_id)).await.unwrap());
            // The fake history still exposes the old generation after a successful update.
            // Even a delivered desired state must be checked and repaired on the next tick.
            reconcile_cards(&pool).await.unwrap();
            assert_eq!(calls.lock().unwrap().len(), 5);
        }).await;
        server.abort();
        {
            let calls = calls.lock().unwrap();
            assert_eq!(calls.len(), 5);
            assert_eq!(without_delivery_ids(calls[0]["blocks"].clone())[1], processing);
            assert_eq!(without_delivery_ids(calls[1]["blocks"].clone()), original);
        }
        let open: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM report_mutations WHERE status<>'delivered'")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(open, 1);
    }

    #[tokio::test]
    #[cfg(feature = "hub-slack")]
    async fn ambiguous_attachment_completion_stays_uncertain_and_is_not_repeated() {
        use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
        let completions = Arc::new(AtomicUsize::new(0));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let app = axum::Router::new()
            .route("/files.getUploadURLExternal", axum::routing::post({
                let url = format!("{base}/bytes");
                move || { let url = url.clone(); async move { axum::Json(json!({"ok":true,"file_id":"F1","upload_url":url})) } }
            }))
            .route("/bytes", axum::routing::post(|| async { axum::http::StatusCode::OK }))
            .route("/files.completeUploadExternal", axum::routing::post({
                let completions = completions.clone();
                move || { let completions = completions.clone(); async move {
                    completions.fetch_add(1, Ordering::SeqCst);
                    "incomplete response"
                }}
            }));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        let pool = crate::storage::memory().await.unwrap();
        let request = parent(&pool).await;
        let Change::Attachment { path, .. } = &request.change else { unreachable!() };
        tokio::fs::write(path, b"preview").await.unwrap();
        sqlx::query("UPDATE chat_outbox SET blocks_json='[]' WHERE id=?")
            .bind(request.outbox_id).execute(&pool).await.unwrap();
        let mut later = request.clone();
        later.mutation_id = "restore:2".into();
        later.change = Change::RestoreActions;
        crate::chat::slack::with_test_api(base, "test-token".into(), async {
            assert_eq!(perform(&pool, &request).await.unwrap(), "uncertain");

            assert_eq!(perform(&pool, &request).await.unwrap(), "uncertain");
            // The final attachment no longer holds the parent: the later edit is attempted
            // (this server has no chat.update, so its outcome is unknown and it recovers itself).
            let later = reply(&pool, &later).await;
            assert_eq!((later["delivery"].as_str(), later["progress"].as_str(), later["blocked_by"].as_str()),
                (Some("pending"), Some("recovering"), Some("restore:2")));
            assert_eq!(perform(&pool, &request).await.unwrap(), "uncertain");
        }).await;
        tokio::fs::remove_file(path).await.unwrap();
        server.abort();
        assert_eq!(completions.load(Ordering::SeqCst), 1);
    }
}
