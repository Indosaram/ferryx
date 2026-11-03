//! The app transcript: the owner's conversation with the supervisor from the panel composer
//! (todo 7, G8, D11), persisted in `supervisor_app_transcript` (migration 0030).
//!
//! [`converse`] stores the owner's text, runs one turn in the retained conversation of channel
//! `"app"` and stores the reply. Both rows are keyed by request id, so a retried request never
//! duplicates a row and never repeats a finished turn. Plain-language rule changes are not
//! parsed here: the text goes to the supervisor as written.
//!
//! A reply's `evidence` stays empty unless something attested the claim; the panel shows an
//! empty list as Unverified. This module attests nothing itself.
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::SqlitePool;

use super::panel_contract::{EvidenceRef, SessionRef};
use super::request::RequestFailure;
use crate::meta::mhc_fault::SupervisorFault;
use crate::meta::mhc_profile::ConversationScope;
use crate::meta::mhc_supervisor::MhcSupervisor;

/// The conversation channel of the app transcript.
pub const APP_CHANNEL: &str = "app";
/// Prefix of the request id a supervisor reply is stored under.
const REPLY_PREFIX: &str = "reply:";
/// Longest owner message.
const MAX_TEXT_CHARS: usize = 64_000;
/// Longest request id.
const MAX_REQUEST_ID_CHARS: usize = 128;
/// Messages one transcript page holds at most.
pub const PAGE_LIMIT: i64 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    Owner,
    Supervisor,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Supervisor => "supervisor",
        }
    }

    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "owner" => Some(Self::Owner),
            "supervisor" => Some(Self::Supervisor),
            _ => None,
        }
    }
}

/// One stored message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptMessage {
    pub id: i64,
    pub role: Role,
    pub text: String,
    pub evidence: Vec<EvidenceRef>,
    pub request_id: String,
    pub created_at_ms: i64,
}

/// The body of one converse request, as the gateway and the desktop send it.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ConverseInput {
    pub text: String,
    /// `@session` mentions: SessionRefs the owner's text addresses.
    #[serde(default)]
    pub mentions: Vec<String>,
    pub request_id: String,
}

/// One turn of the supervisor's conversation for the app channel.
#[async_trait]
pub trait AppConversation: Send + Sync {
    async fn converse(
        &self,
        scope: &ConversationScope,
        message: &str,
    ) -> Result<String, SupervisorFault>;
}

#[async_trait]
impl AppConversation for MhcSupervisor {
    async fn converse(
        &self,
        scope: &ConversationScope,
        message: &str,
    ) -> Result<String, SupervisorFault> {
        self.converse_in_turn(scope, None, message).await
    }
}

/// The conversation scope of every app turn: no project, channel `"app"`.
pub fn app_scope() -> ConversationScope {
    ConversationScope {
        project_id: None,
        channel_key: APP_CHANNEL.to_owned(),
    }
}

/// Wall clock in milliseconds; callers inject their own in tests.
pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn internal(message: impl std::fmt::Display) -> RequestFailure {
    RequestFailure::new("INTERNAL", message.to_string())
}

type Row = (i64, String, String, String, String, i64);

const COLUMNS: &str = "id, role, text, evidence_json, request_id, created_at";

fn decode(row: Row) -> Result<TranscriptMessage, RequestFailure> {
    let (id, role, text, evidence, request_id, created_at_ms) = row;
    Ok(TranscriptMessage {
        id,
        role: Role::parse(&role).ok_or_else(|| internal(format!("transcript row {id} has role {role}")))?,
        text,
        evidence: serde_json::from_str(&evidence)
            .map_err(|error| internal(format!("transcript row {id} evidence: {error}")))?,
        request_id,
        created_at_ms,
    })
}

/// The message stored under `request_id`, if any.
pub async fn by_request(
    pool: &SqlitePool,
    request_id: &str,
) -> Result<Option<TranscriptMessage>, RequestFailure> {
    let row: Option<Row> = sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM supervisor_app_transcript WHERE request_id = ?"
    ))
    .bind(request_id)
    .fetch_optional(pool)
    .await?;
    row.map(decode).transpose()
}

/// Appends a message unless `request_id` is already stored. Returns the stored message (the
/// earlier one on a repeat, unchanged) and whether this call inserted it.
pub async fn append(
    pool: &SqlitePool,
    role: Role,
    text: &str,
    evidence: &[EvidenceRef],
    request_id: &str,
    now_ms: i64,
) -> Result<(TranscriptMessage, bool), RequestFailure> {
    let evidence_json = serde_json::to_string(evidence).map_err(internal)?;
    let done = sqlx::query(
        "INSERT INTO supervisor_app_transcript (role, text, evidence_json, request_id, created_at)
         VALUES (?, ?, ?, ?, ?) ON CONFLICT(request_id) DO NOTHING",
    )
    .bind(role.as_str())
    .bind(text)
    .bind(evidence_json)
    .bind(request_id)
    .bind(now_ms)
    .execute(pool)
    .await?;
    let stored = by_request(pool, request_id)
        .await?
        .ok_or_else(|| internal(format!("transcript row {request_id} vanished after insert")))?;
    Ok((stored, done.rows_affected() == 1))
}

/// Up to `limit` messages after `cursor` (a message id; 0 starts from the beginning), oldest
/// first. `nextCursor` is the last id returned, or `cursor` when the page is empty; pass it
/// back to continue. `hasMore` says whether another page follows.
pub async fn page(pool: &SqlitePool, cursor: i64, limit: i64) -> Result<Value, RequestFailure> {
    let limit = limit.clamp(1, PAGE_LIMIT);
    let rows: Vec<Row> = sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM supervisor_app_transcript WHERE id > ? ORDER BY id LIMIT ?"
    ))
    .bind(cursor)
    .bind(limit + 1)
    .fetch_all(pool)
    .await?;
    let has_more = i64::try_from(rows.len()).map_or(true, |count| count > limit);
    let messages = rows
        .into_iter()
        .take(usize::try_from(limit).unwrap_or(usize::MAX))
        .map(decode)
        .collect::<Result<Vec<_>, _>>()?;
    let next = messages.last().map_or(cursor, |message| message.id);
    Ok(json!({"messages": messages, "nextCursor": next, "hasMore": has_more}))
}

fn reply_request_id(request_id: &str) -> String {
    format!("{REPLY_PREFIX}{request_id}")
}

fn invalid(message: impl Into<String>) -> RequestFailure {
    RequestFailure::new("INVALID_REQUEST", message)
}

/// The message the supervisor receives: the owner's text, plus the sessions it names so the
/// supervisor never has to guess a write target.
fn turn_message(text: &str, mentions: &[SessionRef]) -> String {
    if mentions.is_empty() {
        return text.to_owned();
    }
    let refs: Vec<&str> = mentions.iter().map(SessionRef::as_str).collect();
    format!(
        "{text}\n\n[The owner addressed these sessions with @: {}]",
        refs.join(", ")
    )
}

/// One owner message and the supervisor's reply to it.
///
/// The owner's text is stored first, so it survives a failed turn; retrying the same request id
/// runs the turn again and stores the reply. A finished request answers from the stored rows
/// without a second turn (`replayed: true`).
pub async fn converse(
    pool: &SqlitePool,
    engine: &dyn AppConversation,
    input: &ConverseInput,
    now: &(dyn Fn() -> i64 + Send + Sync),
) -> Result<Value, RequestFailure> {
    let request_id = input.request_id.as_str();
    if request_id.is_empty()
        || request_id.chars().count() > MAX_REQUEST_ID_CHARS
        || request_id.chars().any(char::is_control)
        || request_id.starts_with(REPLY_PREFIX)
    {
        return Err(invalid(format!(
            "requestId must be 1-{MAX_REQUEST_ID_CHARS} printable characters and not start with {REPLY_PREFIX}"
        )));
    }
    let text = input.text.trim();
    if text.is_empty() || text.chars().count() > MAX_TEXT_CHARS {
        return Err(invalid(format!("text must be 1-{MAX_TEXT_CHARS} characters")));
    }
    let mentions = input
        .mentions
        .iter()
        .map(|raw| SessionRef::parse(raw).map_err(|error| invalid(error.to_string())))
        .collect::<Result<Vec<_>, _>>()?;

    let (owner, fresh) = append(pool, Role::Owner, text, &[], request_id, now()).await?;
    let reply_id = reply_request_id(request_id);
    if !fresh {
        if let Some(reply) = by_request(pool, &reply_id).await? {
            return Ok(json!({"owner": owner, "reply": reply, "replayed": true}));
        }
    }
    // The stored text drives the turn, so a retry with different text cannot fork the request.
    let answer = engine
        .converse(&app_scope(), &turn_message(&owner.text, &mentions))
        .await
        .map_err(|fault| RequestFailure::new(fault.code(), fault.to_string()))?;
    // No attested evidence exists for an app reply yet: it stays empty and shows as Unverified.
    let (reply, _) = append(pool, Role::Supervisor, &answer, &[], &reply_id, now()).await?;
    Ok(json!({"owner": owner, "reply": reply, "replayed": !fresh}))
}
