//! Owner memory continuity through the native mhc memory (todo 15).
//!
//! mhc owns learning, recall, correction, forgetting and replay suppression. The hub keeps
//! only provenance: the original owner message behind each request, its adapter and source
//! message id, and the opaque mhc receipt it produced (`supervisor_memory_sources`, 0023).
//!
//! - `teach` sends one owner-authored item to the engine's `memory_receipts` teach, idempotent
//!   by the source id `<adapter>:<requestId>`; with `replaces` it is a `memory_correct` of that
//!   receipt instead. A project-scoped item names the project by its repository identity,
//!   never a host path, so it applies in every worktree and on every host of the project.
//! - `forget` closes a receipt with `memory_forget`; mhc records the suppression, so replaying
//!   an old transcript never teaches the item again.
//! - Owner-authored channel text goes to the serialized owner-ingest session ([`ingest`]),
//!   without its task-only lines ([`split_task_only`]) and never with quoted, forwarded, worker
//!   or supervisor text: callers pass only what the authenticated owner wrote.
//! - A task-only exception is task context for one goal: it is appended to that goal's
//!   instruction and never reaches learning.
use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::SqlitePool;

use super::request::{RequestContext, RequestFailure};
use super::{identity_of, ChannelProvider, SupervisorIdentity};
use crate::meta::mhc_supervisor::{IngestOutcome, MemoryRequestError, OwnerMemory};

/// No memory engine is reachable from this process (the MCP server and CLI never run one).
pub const MEMORY_UNAVAILABLE: &str = "MEMORY_UNAVAILABLE";
/// A forget or correction could not remove every restatement; mhc recorded the suppression and
/// kept the receipt open (409).
pub const MEMORY_CONTENT_RETAINED: &str = "MEMORY_CONTENT_RETAINED";
/// The engine refused a memory request with a code the hub does not map.
pub const MEMORY_REFUSED: &str = "MEMORY_REFUSED";

/// Line prefixes an owner uses to mark text as an exception for this one task. Such a line is
/// task context only: it is never sent to learning. Inbound parsing is language-tolerant: every
/// form is accepted whatever the owner's configured language.
pub const TASK_ONLY_PREFIXES: [&str; 6] = [
    "once:",
    "this time only:",
    "just this once:",
    "for this task only:",
    "이번만:",
    "이번 작업만:",
];

/// Longest list the engine serves in one read.
const LIST_LIMIT_MAX: u64 = 1000;

/// `teach` payload. `replaces` names a receipt this item corrects (`memory_correct`).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TeachPayload {
    pub text: String,
    #[serde(default)]
    pub replaces: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

/// `forget` payload. `memoryId` is the mhc receipt id.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ForgetPayload {
    pub memory_id: String,
    #[serde(default)]
    pub reason: Option<String>,
}

/// Splits owner text into what may be learned and its task-only lines. A line whose start
/// (case-insensitive, after whitespace) is one of [`TASK_ONLY_PREFIXES`] is task-only; the
/// prefix is removed from the task-only text.
pub fn split_task_only(text: &str) -> (String, Vec<String>) {
    let mut learnable = Vec::new();
    let mut once = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        let lower = trimmed.to_lowercase();
        match TASK_ONLY_PREFIXES
            .iter()
            .find(|prefix| lower.starts_with(*prefix))
        {
            Some(prefix) => {
                // The prefixes are ASCII or whole Hangul syllables; lowercasing keeps byte
                // lengths of both, so the prefix length indexes the original line.
                let rest = trimmed.get(prefix.len()..).unwrap_or("").trim();
                if !rest.is_empty() {
                    once.push(rest.to_owned());
                }
            }
            None => learnable.push(line),
        }
    }
    (learnable.join("\n").trim().to_owned(), once)
}

/// The worker instruction block of task-only exceptions, appended to one goal's instruction.
pub fn task_only_block(exceptions: &[String]) -> Option<String> {
    if exceptions.is_empty() {
        return None;
    }
    let mut block =
        String::from("One-time owner exception (this goal only; it is not a standing rule and is not remembered):");
    for exception in exceptions {
        block.push_str("\n- ");
        block.push_str(exception.trim());
    }
    Some(block)
}

fn memory(context: &RequestContext) -> Result<&Arc<dyn OwnerMemory>, RequestFailure> {
    context.memory.as_ref().ok_or_else(|| {
        RequestFailure::new(
            MEMORY_UNAVAILABLE,
            "no native supervisor memory engine runs in this process; nothing was changed",
        )
    })
}

async fn owner(pool: &SqlitePool, principal: &str) -> Result<SupervisorIdentity, RequestFailure> {
    identity_of(pool, principal).await?.ok_or_else(|| {
        RequestFailure::new(
            "UNAUTHORIZED",
            format!("{principal} is not the supervisor's owner"),
        )
    })
}

/// The project's repository identity, when the caller's supervisor owns it.
async fn project_identity(
    pool: &SqlitePool,
    identity: &SupervisorIdentity,
    project_id: &str,
) -> Result<String, RequestFailure> {
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT supervisor_id, repo_identity FROM supervisor_projects WHERE project_id = ?",
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await?;
    match row {
        None => Err(RequestFailure::new(
            "NOT_FOUND",
            format!("project {project_id} not found"),
        )),
        Some((supervisor, _)) if supervisor != identity.supervisor_id => Err(RequestFailure::new(
            "FORBIDDEN",
            format!("project {project_id} belongs to another owner"),
        )),
        Some((_, repo_identity)) => Ok(repo_identity),
    }
}

fn engine_failure(error: MemoryRequestError) -> RequestFailure {
    match error {
        MemoryRequestError::Fault(fault) => RequestFailure::new(fault.code(), fault.to_string()),
        MemoryRequestError::Refused { code, message } => {
            let mapped = match code.as_str() {
                "invalid_request" => "INVALID_REQUEST",
                "receipt_not_found" => "NOT_FOUND",
                "source_conflict" => "REQUEST_CONFLICT",
                "receipt_closed" => MEMORY_REFUSED,
                "memory_content_retained" => MEMORY_CONTENT_RETAINED,
                "memory_write_denied"
                | "memory_unavailable"
                | "memory_write_failed"
                | "memory_storage_failed" => MEMORY_UNAVAILABLE,
                _ => MEMORY_REFUSED,
            };
            RequestFailure::new(mapped, format!("{code}: {message}"))
        }
    }
}

/// The fields of one `supervisor_memory_sources` row.
struct Provenance<'a> {
    source_id: &'a str,
    identity: &'a SupervisorIdentity,
    adapter: ChannelProvider,
    source_message_id: &'a str,
    action: &'a str,
    project_id: Option<&'a str>,
    body: &'a str,
    target: Option<&'a str>,
}

/// One provenance row, inserted once per source id. A replay with the same body is the same
/// request; a different body under the same source id is `REQUEST_CONFLICT` (409).
async fn provenance(pool: &SqlitePool, row: Provenance<'_>) -> Result<(), RequestFailure> {
    let Provenance {
        source_id,
        identity,
        adapter,
        source_message_id,
        action,
        project_id,
        body,
        target,
    } = row;
    let hash = crate::usage::sha256_hex(&format!("{action}\u{0}{}\u{0}{body}", target.unwrap_or("")));
    let now = crate::storage::now_rfc3339();
    sqlx::query(
        "INSERT INTO supervisor_memory_sources(source_id, supervisor_id, owner_principal,
            source_adapter, source_message_id, action, project_id, body, body_sha256,
            target_receipt_id, state, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'pending', ?, ?)
         ON CONFLICT(source_id) DO NOTHING",
    )
    .bind(source_id)
    .bind(&identity.supervisor_id)
    .bind(&identity.owner_principal)
    .bind(adapter.as_str())
    .bind(source_message_id)
    .bind(action)
    .bind(project_id)
    .bind(body)
    .bind(&hash)
    .bind(target)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;
    let stored: String =
        sqlx::query_scalar("SELECT body_sha256 FROM supervisor_memory_sources WHERE source_id = ?")
            .bind(source_id)
            .fetch_one(pool)
            .await?;
    if stored != hash {
        return Err(RequestFailure::new(
            "REQUEST_CONFLICT",
            format!("source {source_id} was already used for another memory request"),
        ));
    }
    Ok(())
}

async fn settle(
    pool: &SqlitePool,
    source_id: &str,
    receipt_id: Option<&str>,
    state: &str,
    error: Option<&str>,
) -> Result<(), RequestFailure> {
    sqlx::query(
        "UPDATE supervisor_memory_sources
            SET receipt_id = COALESCE(?, receipt_id), state = ?, last_error = ?, updated_at = ?
          WHERE source_id = ?",
    )
    .bind(receipt_id)
    .bind(state)
    .bind(error)
    .bind(crate::storage::now_rfc3339())
    .bind(source_id)
    .execute(pool)
    .await?;
    Ok(())
}

fn receipt_state(receipt: &Value) -> (Option<String>, String) {
    (
        receipt
            .get("receiptId")
            .and_then(Value::as_str)
            .map(str::to_owned),
        receipt
            .get("state")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_owned(),
    )
}

/// The text the engine learns for a teach: the owner's words, scoped to the project's
/// repository identity when the request names a project.
fn scoped_text(text: &str, repo_identity: Option<&str>) -> String {
    match repo_identity {
        Some(repo) => format!("In project {repo}: {}", text.trim()),
        None => text.trim().to_owned(),
    }
}

/// `teach`: one owner-authored item, receipt-backed. With `replaces` it corrects that receipt.
pub async fn teach(
    pool: &SqlitePool,
    context: &RequestContext,
    request_id: &str,
    project_id: Option<&str>,
    payload: &TeachPayload,
) -> Result<Value, RequestFailure> {
    let identity = owner(pool, &context.principal).await?;
    let memory = memory(context)?;
    let (learnable, once) = split_task_only(&payload.text);
    if !once.is_empty() {
        return Err(RequestFailure::new(
            "INVALID_REQUEST",
            "a task-only exception is task context, never a memory; send it with the work instead",
        ));
    }
    let repo = match project_id {
        Some(project) => Some(project_identity(pool, &identity, project).await?),
        None => None,
    };
    let text = scoped_text(&learnable, repo.as_deref());
    let source_id = format!("{}:{request_id}", context.adapter);
    let action = if payload.replaces.is_some() { "correct" } else { "teach" };
    provenance(
        pool,
        Provenance {
            source_id: &source_id,
            identity: &identity,
            adapter: context.adapter,
            source_message_id: request_id,
            action,
            project_id,
            body: &text,
            target: payload.replaces.as_deref(),
        },
    )
    .await?;
    let answer = match &payload.replaces {
        None => {
            memory
                .memory_request(
                    "memory_receipts",
                    json!({"action": "teach", "sourceId": source_id, "text": text}),
                )
                .await
        }
        Some(receipt) => {
            let mut data = json!({"sourceId": source_id, "receiptId": receipt, "replacement": text});
            if let Some(reason) = &payload.reason {
                data["reason"] = json!(reason);
            }
            memory.memory_request("memory_correct", data).await
        }
    };
    let answer = match answer {
        Ok(answer) => answer,
        Err(error) => {
            let failure = engine_failure(error);
            settle(pool, &source_id, None, "failed", Some(&failure.code)).await?;
            return Err(failure);
        }
    };
    let receipt = answer.get("receipt").cloned().unwrap_or(Value::Null);
    let (receipt_id, state) = receipt_state(&receipt);
    settle(pool, &source_id, receipt_id.as_deref(), &state, None).await?;
    let changed = answer.get("changed").and_then(Value::as_bool).unwrap_or(false);
    Ok(json!({
        "action": "teach",
        "memory": {
            "state": state,
            "receiptId": receipt_id,
            "sourceMessageId": source_id,
            "duplicate": !changed,
            "replaces": payload.replaces,
            "corrected": answer.get("corrected"),
            "projectIdentity": repo,
        },
        "receipt": receipt,
    }))
}

/// `forget`: closes one receipt; mhc suppresses replay of its content.
pub async fn forget(
    pool: &SqlitePool,
    context: &RequestContext,
    request_id: &str,
    payload: &ForgetPayload,
) -> Result<Value, RequestFailure> {
    let identity = owner(pool, &context.principal).await?;
    let memory = memory(context)?;
    let source_id = format!("{}:{request_id}", context.adapter);
    provenance(
        pool,
        Provenance {
            source_id: &source_id,
            identity: &identity,
            adapter: context.adapter,
            source_message_id: request_id,
            action: "forget",
            project_id: None,
            body: "",
            target: Some(&payload.memory_id),
        },
    )
    .await?;
    let mut data = json!({"sourceId": source_id, "receiptId": payload.memory_id});
    if let Some(reason) = &payload.reason {
        data["reason"] = json!(reason);
    }
    let answer = match memory.memory_request("memory_forget", data).await {
        Ok(answer) => answer,
        Err(error) => {
            let failure = engine_failure(error);
            settle(pool, &source_id, None, "failed", Some(&failure.code)).await?;
            return Err(failure);
        }
    };
    let receipt = answer.get("receipt").cloned().unwrap_or(Value::Null);
    let (receipt_id, state) = receipt_state(&receipt);
    settle(pool, &source_id, receipt_id.as_deref(), &state, None).await?;
    Ok(json!({
        "action": "forget",
        "memory": {
            "state": state,
            "receiptId": receipt_id,
            "sourceMessageId": source_id,
            "duplicate": !answer.get("changed").and_then(Value::as_bool).unwrap_or(false),
            "possibleRestatements": answer.get("possibleRestatements"),
        },
        "receipt": receipt,
    }))
}

/// The engine's receipts, newest last, each with the hub's provenance of the request that
/// produced it (adapter, source message id, original text) and of the request that closed it.
pub async fn receipts(
    pool: &SqlitePool,
    context: &RequestContext,
    limit: Option<u64>,
) -> Result<Value, RequestFailure> {
    owner(pool, &context.principal).await?;
    let memory = memory(context)?;
    let limit = limit.unwrap_or(100).clamp(1, LIST_LIMIT_MAX);
    let answer = memory
        .memory_request("memory_receipts", json!({"action": "list", "limit": limit}))
        .await
        .map_err(engine_failure)?;
    type Row = (String, String, String, String, Option<String>, String, String, String);
    let mut listed = Vec::new();
    for receipt in answer
        .get("receipts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        let receipt_id = receipt
            .get("receiptId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let sources: Vec<Row> = sqlx::query_as(
            "SELECT source_id, source_adapter, source_message_id, action, project_id, body, state,
                    created_at
               FROM supervisor_memory_sources WHERE receipt_id = ? ORDER BY created_at",
        )
        .bind(&receipt_id)
        .fetch_all(pool)
        .await?;
        let provenance: Vec<Value> = sources
            .into_iter()
            .map(|(source, adapter, message, action, project, body, state, at)| {
                json!({"sourceId": source, "sourceAdapter": adapter, "sourceMessageId": message,
                    "action": action, "projectId": project, "text": body, "state": state,
                    "createdAt": at})
            })
            .collect();
        let mut entry = receipt;
        if let Some(fields) = entry.as_object_mut() {
            fields.insert("provenance".into(), json!(provenance));
        }
        listed.push(entry);
    }
    Ok(json!({
        "receipts": listed,
        "identity": answer.get("identity"),
        "ingests": recent_ingests(pool, 50).await?,
    }))
}

/// Sends authenticated owner-authored channel text to owner ingest through `memory`.
pub async fn ingest(
    pool: &SqlitePool,
    memory: &dyn OwnerMemory,
    principal: &str,
    adapter: ChannelProvider,
    source_message_id: &str,
    project_id: Option<&str>,
    owner_text: &str,
) -> Value {
    ingest_with(
        pool,
        principal,
        adapter,
        source_message_id,
        project_id,
        owner_text,
        |source, text| async move {
            match memory.ingest_owner_text(&source, &text).await {
                Ok(outcome) => Ok(outcome == IngestOutcome::Duplicate),
                Err(fault) => Err(format!("{}: {fault}", fault.code())),
            }
        },
    )
    .await
}

/// Sends authenticated owner-authored text to owner ingest with `send(source_id, text)`, once
/// per source message, recording its provenance. The caller passes only what the owner wrote
/// (no quoted, forwarded, worker or supervisor text). Task-only lines are removed first and
/// nothing is sent when nothing learnable is left. A source already ingested is not sent again,
/// so replaying an old message never re-teaches it. `send` answers whether the engine already
/// had it, or `"CODE: message"`. A failure is recorded on the provenance row and returned,
/// never silently dropped.
pub async fn ingest_with<F, Fut>(
    pool: &SqlitePool,
    principal: &str,
    adapter: ChannelProvider,
    source_message_id: &str,
    project_id: Option<&str>,
    owner_text: &str,
    send: F,
) -> Value
where
    F: FnOnce(String, String) -> Fut,
    Fut: std::future::Future<Output = Result<bool, String>>,
{
    let (learnable, once) = split_task_only(owner_text);
    if learnable.is_empty() {
        return json!({"state": "skipped", "taskOnlyLines": once.len()});
    }
    let identity = match owner(pool, principal).await {
        Ok(identity) => identity,
        Err(failure) => return json!({"state": "refused", "code": failure.code}),
    };
    // A project-bound message is learned for the project's repository identity, never for a
    // checkout path, so it applies in every worktree and on every host of the project.
    let repo = match project_id {
        Some(project) => project_identity(pool, &identity, project).await.ok(),
        None => None,
    };
    let learnable = scoped_text(&learnable, repo.as_deref());
    let source_id = format!("{adapter}:{source_message_id}");
    if let Err(failure) = provenance(
        pool,
        Provenance {
            source_id: &source_id,
            identity: &identity,
            adapter,
            source_message_id,
            action: "ingest",
            project_id,
            body: &learnable,
            target: None,
        },
    )
    .await
    {
        return json!({"state": "refused", "code": failure.code});
    }
    let state: Option<String> =
        sqlx::query_scalar("SELECT state FROM supervisor_memory_sources WHERE source_id = ?")
            .bind(&source_id)
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();
    if state.as_deref() == Some("ingested") {
        return json!({"state": "ingested", "sourceId": source_id, "duplicate": true, "taskOnlyLines": once.len()});
    }
    let outcome = send(source_id.clone(), learnable).await;
    let (state, code) = match &outcome {
        Ok(_) => ("ingested", None),
        Err(error) => (
            "failed",
            Some(error.split(':').next().unwrap_or("MEMORY_REFUSED").trim().to_owned()),
        ),
    };
    if let Err(failure) = settle(pool, &source_id, None, state, code.as_deref()).await {
        tracing::warn!(code = %failure.code, "owner ingest provenance was not updated");
    }
    match outcome {
        Ok(duplicate) => {
            json!({"state": "ingested", "sourceId": source_id, "duplicate": duplicate, "taskOnlyLines": once.len()})
        }
        Err(error) => {
            tracing::warn!(%error, "owner message was not ingested");
            json!({"state": "failed", "sourceId": source_id, "code": code})
        }
    }
}

/// Whether text that arrived through `adapter` may teach. Only adapters that authenticate the
/// owner outside every worker pane learn: the desktop app, a paired phone, and the Slack and
/// Discord owner bindings. MCP and the CLI are reachable from inside a worker's terminal, so
/// text arriving through them is never learned implicitly (an explicit `teach` still works
/// there, behind the daemon's pane gate).
pub fn learns_from(adapter: ChannelProvider) -> bool {
    matches!(
        adapter,
        ChannelProvider::Desktop
            | ChannelProvider::Phone
            | ChannelProvider::Slack
            | ChannelProvider::Discord
    )
}

/// Queues owner learning of one accepted request's owner text (a message, a submitted
/// request, a correction) and returns its receipt for the caller's answer: `queued` with the
/// source id, or `notLearned` with why. The engine turn runs in its own task, so the owner's
/// request never waits on learning; the provenance row records what happened.
pub fn learn_in_background(
    pool: &SqlitePool,
    context: &RequestContext,
    request_id: &str,
    project_id: Option<&str>,
    owner_text: &str,
) -> Value {
    if !learns_from(context.adapter) {
        return json!({"state": "notLearned", "reason": "adapter_not_owner_authenticated"});
    }
    let Some(memory) = context.memory.clone() else {
        return json!({"state": "notLearned", "reason": "memory_unavailable"});
    };
    let (learnable, once) = split_task_only(owner_text);
    if learnable.is_empty() {
        return json!({"state": "notLearned", "reason": "task_only", "taskOnlyLines": once.len()});
    }
    let pool = pool.clone();
    let principal = context.principal.clone();
    let adapter = context.adapter;
    let source = request_id.to_owned();
    let project = project_id.map(str::to_owned);
    let text = owner_text.to_owned();
    tokio::spawn(async move {
        ingest(
            &pool,
            memory.as_ref(),
            &principal,
            adapter,
            &source,
            project.as_deref(),
            &text,
        )
        .await;
    });
    json!({"state": "queued", "sourceId": format!("{adapter}:{request_id}"), "taskOnlyLines": once.len()})
}

/// The latest owner-ingest provenance rows (newest first): what was sent for learning, from
/// which message, and whether it arrived.
pub async fn recent_ingests(pool: &SqlitePool, limit: i64) -> Result<Vec<Value>, RequestFailure> {
    type Row = (String, String, String, Option<String>, String, String, Option<String>, String);
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT source_id, source_adapter, source_message_id, project_id, body, state, last_error,
                created_at
           FROM supervisor_memory_sources WHERE action = 'ingest'
          ORDER BY created_at DESC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(source, adapter, message, project, body, state, error, at)| {
            json!({"sourceId": source, "sourceAdapter": adapter, "sourceMessageId": message,
                "projectId": project, "text": body, "state": state, "error": error, "createdAt": at})
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_only_prefixes_are_recognized_in_every_owner_language() {
        let text = "Remember the staging host.\nOnce: skip the e2e suite\n이번만: 커밋 생략";
        let expected = (
            "Remember the staging host.".to_owned(),
            vec!["skip the e2e suite".to_owned(), "커밋 생략".to_owned()],
        );
        assert_eq!(split_task_only(text), expected);
        let english = crate::locale::with(crate::locale::OwnerLocale::english_utc(), || split_task_only(text));
        assert_eq!(english, expected);
    }
}
