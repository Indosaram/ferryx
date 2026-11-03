//! The app transcript: the owner's conversation with the supervisor from the panel composer
//! (todo 7, G8, D11), persisted in `supervisor_app_transcript` (migration 0048).
//!
//! [`converse`] stores the owner's text, runs one turn in the retained conversation of channel
//! `"app"` and stores the reply. Both rows are keyed by request id, so a retried request never
//! duplicates a row and never repeats a finished turn.
//!
//! One request id is one flight ([`Flights`]): a duplicate that arrives while the first is still
//! running waits for it and returns its answer (`replayed: true`) instead of running a second
//! turn. A request dropped before it answered (the HTTP client went away) leaves a
//! `failed:<request id>` supervisor row, so the transcript never just loses it; sending the same
//! request id again runs the turn.
//!
//! A plain-language rule change ("retry every 5 min", optionally "... in mahoquot projects") is
//! not a conversation turn: it becomes a standing `upstreamRetry` rule in the [`RuleStore`] and
//! the reply carries the stored rule as evidence (`kind: "rule"`). Without a rule store the
//! request fails; nothing is claimed that did not happen. Any other reply's `evidence` stays
//! empty and the panel shows it as Unverified.
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, PoisonError};

use async_trait::async_trait;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use tokio::sync::watch;

use super::panel_contract::{EvidenceRef, RuleKind, RuleScope, RuleStore, SessionRef, StandingRule};
use super::request::RequestFailure;
use crate::meta::mhc_fault::SupervisorFault;
use crate::meta::mhc_profile::ConversationScope;
use crate::meta::mhc_supervisor::MhcSupervisor;

/// The conversation channel of the app transcript.
pub const APP_CHANNEL: &str = "app";
/// Prefix of the request id a supervisor reply is stored under.
const REPLY_PREFIX: &str = "reply:";
/// Prefix of the request id the failure marker of a dropped request is stored under.
const FAILED_PREFIX: &str = "failed:";
/// Prefix of the request id a worker report is stored under (`report:<wake event id>`). Owner
/// request ids may not start with it, so a client can never occupy a report's row.
pub const REPORT_PREFIX: &str = "report:";
/// Text of the failure marker.
const ABANDONED_TEXT: &str = "This request did not finish: the connection was dropped before the supervisor answered. Send it again to retry.";
/// Shortest retry interval a rule change may set, in seconds.
const MIN_RULE_SECS: u64 = 10;
/// Longest retry interval a rule change may set, in seconds.
const MAX_RULE_SECS: u64 = 86_400;
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

fn failed_request_id(request_id: &str) -> String {
    format!("{FAILED_PREFIX}{request_id}")
}

/// Stores a worker report as one supervisor row keyed by its wake event id. A replay of the same
/// report returns the stored row and `false`; nothing is written twice.
pub async fn store_report(
    pool: &SqlitePool,
    wake_event_id: &str,
    body: &str,
    now_ms: i64,
) -> Result<(TranscriptMessage, bool), RequestFailure> {
    append(pool, Role::Supervisor, body, &[], &format!("{REPORT_PREFIX}{wake_event_id}"), now_ms).await
}

fn invalid(message: impl Into<String>) -> RequestFailure {
    RequestFailure::new("INVALID_REQUEST", message)
}

type Outcome = Result<Value, RequestFailure>;

fn abandoned() -> RequestFailure {
    RequestFailure::new(
        "SUPERVISOR_UNAVAILABLE",
        "the request was dropped before the supervisor answered; send it again with the same requestId",
    )
}

/// The requests running right now, one flight per request id. Owned by the daemon's gateway
/// state, never global, so two hubs never share a flight.
#[derive(Default)]
pub struct Flights {
    active: Mutex<HashMap<String, watch::Sender<Option<Outcome>>>>,
    #[cfg(test)]
    markers: Mutex<Vec<tokio::task::JoinHandle<()>>>,
}

enum Entry<'a> {
    Lead(Lead<'a>),
    Follow(watch::Receiver<Option<Outcome>>),
}

impl Flights {
    /// The caller leads the flight of `request_id`, or follows the one already running.
    fn enter<'a>(
        &'a self,
        request_id: &str,
        pool: &SqlitePool,
        now: &'a (dyn Fn() -> i64 + Send + Sync),
    ) -> Entry<'a> {
        let mut active = self.active.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(flight) = active.get(request_id) {
            return Entry::Follow(flight.subscribe());
        }
        let (sender, _) = watch::channel(None);
        active.insert(request_id.to_owned(), sender);
        Entry::Lead(Lead {
            flights: self,
            request_id: request_id.to_owned(),
            pool: pool.clone(),
            now,
            armed: true,
        })
    }

    /// Ends the flight of `request_id` and hands `outcome` to everyone waiting on it.
    fn release(&self, request_id: &str, outcome: Outcome) {
        let flight = self
            .active
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(request_id);
        if let Some(flight) = flight {
            flight.send_replace(Some(outcome));
        }
    }

    /// Waits for every failure marker a dropped request scheduled.
    #[cfg(test)]
    pub(crate) async fn settle(&self) {
        let tasks: Vec<_> =
            std::mem::take(&mut *self.markers.lock().unwrap_or_else(PoisonError::into_inner));
        for task in tasks {
            let _ = task.await;
        }
    }
}

/// The leader of a flight. Dropping it before [`Lead::finish`] means the request future was
/// dropped mid-turn: the flight ends as abandoned and a failure marker row is written.
struct Lead<'a> {
    flights: &'a Flights,
    request_id: String,
    pool: SqlitePool,
    now: &'a (dyn Fn() -> i64 + Send + Sync),
    armed: bool,
}

impl Lead<'_> {
    fn finish(mut self, outcome: &Outcome) {
        self.armed = false;
        self.flights.release(&self.request_id, outcome.clone());
    }
}

impl Drop for Lead<'_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        self.flights.release(&self.request_id, Err(abandoned()));
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let pool = self.pool.clone();
        let request_id = self.request_id.clone();
        let at = (self.now)();
        let task = runtime.spawn(async move {
            // One statement, so a retry that answers meanwhile never gets a marker after its
            // reply: only a request that stored its owner text and has no reply is marked.
            let _ = sqlx::query(
                "INSERT INTO supervisor_app_transcript (role, text, evidence_json, request_id, created_at)
                 SELECT 'supervisor', ?, '[]', ?, ?
                 WHERE EXISTS (SELECT 1 FROM supervisor_app_transcript WHERE request_id = ?)
                   AND NOT EXISTS (SELECT 1 FROM supervisor_app_transcript WHERE request_id = ?)
                 ON CONFLICT(request_id) DO NOTHING",
            )
            .bind(ABANDONED_TEXT)
            .bind(failed_request_id(&request_id))
            .bind(at)
            .bind(&request_id)
            .bind(reply_request_id(&request_id))
            .execute(&pool)
            .await;
        });
        #[cfg(test)]
        self.flights
            .markers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(task);
        #[cfg(not(test))]
        drop(task);
    }
}

async fn follow(mut flight: watch::Receiver<Option<Outcome>>) -> Outcome {
    let settled = flight
        .wait_for(|outcome| outcome.is_some())
        .await
        .map_err(|_| abandoned())?;
    let outcome = match settled.as_ref() {
        Some(Ok(answer)) => {
            let mut answer = answer.clone();
            answer["replayed"] = json!(true);
            Ok(answer)
        }
        Some(Err(failure)) => Err(failure.clone()),
        None => Err(abandoned()),
    };
    outcome
}

/// A parsed "retry every N unit [in X projects]" instruction.
#[derive(Debug, PartialEq, Eq)]
struct RuleChange {
    secs: u64,
    repo: Option<String>,
}

static RULE_CHANGE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^\W*(?:(?:please|always|from\s+now\s+on)[\s,]+)*retry(?:\s+(?:upstream|proxy|failed|failing)\s+(?:errors?|requests?|calls?|turns?))?\s+every\s+(\d+)\s*(seconds?|secs?|s|minutes?|mins?|m|hours?|hrs?|h)\b(?:\s+(?:in|for|on)\s+([\w./-]+)\s+(?:projects?|repos?|repositories))?\W*$",
    )
    .expect("rule change pattern")
});

/// `Ok(None)` when the text is not a rule change; an out-of-range interval is refused.
fn parse_rule_change(text: &str) -> Result<Option<RuleChange>, RequestFailure> {
    let Some(found) = RULE_CHANGE.captures(text) else {
        return Ok(None);
    };
    let factor = match found[2].to_ascii_lowercase().chars().next() {
        Some('h') => 3_600,
        Some('m') => 60,
        _ => 1,
    };
    let secs = found[1]
        .parse::<u64>()
        .ok()
        .and_then(|amount| amount.checked_mul(factor))
        .filter(|secs| (MIN_RULE_SECS..=MAX_RULE_SECS).contains(secs))
        .ok_or_else(|| invalid("a retry interval must be between 10 seconds and 24 hours"))?;
    Ok(Some(RuleChange {
        secs,
        repo: found.get(3).map(|repo| repo.as_str().to_owned()),
    }))
}

fn human_interval(secs: u64) -> String {
    if secs.is_multiple_of(3_600) {
        format!("{} h", secs / 3_600)
    } else if secs.is_multiple_of(60) {
        format!("{} min", secs / 60)
    } else {
        format!("{secs} s")
    }
}

/// Writes the standing rule and returns the reply text and the evidence, both built from the
/// rule the store returned, not from what was asked for.
async fn apply_rule_change(
    ctx: &Converse<'_>,
    owner: &TranscriptMessage,
    change: &RuleChange,
) -> Result<(String, Vec<EvidenceRef>), RequestFailure> {
    let Some(store) = ctx.rules else {
        return Err(RequestFailure::new(
            "SUPERVISOR_UNAVAILABLE",
            "standing rules are not installed in this daemon; no rule was changed",
        ));
    };
    let storage = |error: crate::error::HubError| RequestFailure::new("STORAGE", error.to_string());
    let scope = RuleScope {
        repo: change.repo.clone(),
        ..RuleScope::default()
    };
    // The same scope keeps one retry rule: replace it in place, so a repeat never stacks rules.
    let existing = store
        .list()
        .await
        .map_err(storage)?
        .into_iter()
        .find(|rule| rule.kind == RuleKind::UpstreamRetry && rule.scope == scope);
    let stored = store
        .upsert(StandingRule {
            id: existing.map_or(0, |rule| rule.id),
            scope: scope.clone(),
            kind: RuleKind::UpstreamRetry,
            params: json!({ "backoffSecs": [change.secs], "thenEverySecs": change.secs }),
            created_from_message_id: Some(owner.id.to_string()),
            created_at_ms: (ctx.now)(),
        })
        .await
        .map_err(storage)?;
    let secs = stored.params["thenEverySecs"].as_u64();
    if stored.kind != RuleKind::UpstreamRetry || stored.scope != scope || secs != Some(change.secs) {
        return Err(internal(format!(
            "rule store returned rule {} that does not match the requested change",
            stored.id
        )));
    }
    let target = stored
        .scope
        .repo
        .as_deref()
        .map_or_else(|| "every session".to_owned(), |repo| format!("{repo} projects"));
    Ok((
        format!(
            "Standing rule #{}: retry upstream errors every {} ({} s) for {target}.",
            stored.id,
            human_interval(change.secs),
            change.secs
        ),
        vec![EvidenceRef {
            kind: "rule".into(),
            id: stored.id.to_string(),
        }],
    ))
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

/// What one converse request runs against.
pub struct Converse<'a> {
    pub pool: &'a SqlitePool,
    pub engine: &'a dyn AppConversation,
    /// The standing rules; `None` when this daemon has none, which refuses rule changes.
    pub rules: Option<&'a dyn RuleStore>,
    pub flights: &'a Flights,
    pub now: &'a (dyn Fn() -> i64 + Send + Sync),
    /// Called each time the transcript gained a row, so views can refetch while a turn runs.
    pub changed: &'a (dyn Fn() + Send + Sync),
}

/// One owner message and the supervisor's reply to it.
///
/// The owner's text is stored first, so it survives a failed turn; retrying the same request id
/// runs the turn again and stores the reply (`replayed: false`). A finished request answers from
/// the stored rows without a second turn, and a duplicate that arrives mid-turn waits for the
/// first and returns its answer, both with `replayed: true`.
pub async fn converse(ctx: &Converse<'_>, input: &ConverseInput) -> Outcome {
    let request_id = input.request_id.as_str();
    if request_id.is_empty()
        || request_id.chars().count() > MAX_REQUEST_ID_CHARS
        || request_id.chars().any(char::is_control)
        || request_id.starts_with(REPLY_PREFIX)
        || request_id.starts_with(FAILED_PREFIX)
        || request_id.starts_with(REPORT_PREFIX)
    {
        return Err(invalid(format!(
            "requestId must be 1-{MAX_REQUEST_ID_CHARS} printable characters and not start with {REPLY_PREFIX}, {FAILED_PREFIX} or {REPORT_PREFIX}"
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
    // A message addressed to sessions with @ is an instruction to them, never a rule change.
    let change = if mentions.is_empty() {
        parse_rule_change(text)?
    } else {
        None
    };

    let lead = match ctx.flights.enter(request_id, ctx.pool, ctx.now) {
        Entry::Follow(flight) => return follow(flight).await,
        Entry::Lead(lead) => lead,
    };
    let outcome = run(ctx, request_id, text, &mentions, change.as_ref()).await;
    lead.finish(&outcome);
    outcome
}

async fn run(
    ctx: &Converse<'_>,
    request_id: &str,
    text: &str,
    mentions: &[SessionRef],
    change: Option<&RuleChange>,
) -> Outcome {
    let pool = ctx.pool;
    let (owner, fresh) = append(pool, Role::Owner, text, &[], request_id, (ctx.now)()).await?;
    if fresh {
        (ctx.changed)();
    }
    let reply_id = reply_request_id(request_id);
    if !fresh {
        if let Some(reply) = by_request(pool, &reply_id).await? {
            return Ok(json!({"owner": owner, "reply": reply, "replayed": true}));
        }
    }
    // The stored text drives the turn, so a retry with different text cannot fork the request.
    let (answer, evidence) = match change {
        Some(change) => apply_rule_change(ctx, &owner, change).await?,
        // An engine reply is unattested: no evidence, shown as Unverified.
        None => {
            let answer = ctx
                .engine
                .converse(&app_scope(), &turn_message(&owner.text, mentions))
                .await
                .map_err(|fault| RequestFailure::new(fault.code(), fault.to_string()))?;
            (answer, Vec::new())
        }
    };
    let (reply, _) =
        append(pool, Role::Supervisor, &answer, &evidence, &reply_id, (ctx.now)()).await?;
    (ctx.changed)();
    Ok(json!({"owner": owner, "reply": reply, "replayed": false}))
}
