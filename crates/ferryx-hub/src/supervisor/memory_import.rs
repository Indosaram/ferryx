//! Owner memory seeded from earlier agent sessions (supervisor setup).
//!
//! The caller (the desktop app's setup driver) reads recent Claude/Codex/omo sessions and hands
//! their user turns over as [`ImportSession`]s. Only what the owner typed may teach: a turn the
//! hub itself sent into a worker pane (a supervisor dispatch, a `[ferryx ...]` wrapper, a
//! tagged harness message) is rejected by [`owner_turn_text`], every session the hub drove is
//! skipped whole ([`hub_driven_session_ids`]), and text that looks like a credential is never
//! sent. Each kept turn is fenced as untrusted text whose origin names the provider and session
//! it came from (an old transcript is still data, not an instruction), then goes to owner ingest
//! through [`super::memory::ingest`] with the provenance adapter `import`, once per turn;
//! `supervisor_memory_imports` remembers what a session hashed to, so an unchanged session is
//! not read again and a rerun ingests nothing.
use std::collections::HashSet;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;
use sqlx::SqlitePool;

use super::memory::{ingest, split_task_only};
use super::{identity_of, ChannelProvider, SupervisorError};
use crate::meta::mhc_supervisor::OwnerMemory;
use crate::storage::now_rfc3339;

/// Longest `<provider>:<session>:<ordinal>` source message id.
const MAX_SOURCE_ID_CHARS: usize = 256;

/// `hub_meta` key: RFC 3339 time of the last completed import run, written even when the run
/// kept no session, so setup does not wait for an import that has nothing to read.
pub const LAST_RUN_KEY: &str = "supervisor_memory_import_last_run";

/// `hub_meta` key: JSON summary of the last pass over the owner's agent memory
/// (`~/.omo/memory`), see [`record_pass`].
pub const OMO_MEMORY_PASS_KEY: &str = "supervisor_omo_memory_last_pass";

/// Import provider of the agent-memory pass. Its rows never count as the setup seed having run
/// ([`ImportTotals::last_at`]), which stays the session and instruction-file seed's.
pub const OMO_MEMORY_PROVIDER: &str = "omo-memory";

/// Call cause of every owner-ingest engine call an agent-memory pass makes, so its token use is
/// attributable in `supervisor_model_calls`.
pub const OMO_MEMORY_CAUSE: &str = "omo_memory_import";

/// Markers of text the hub or a harness wrote, never the owner: a supervisor dispatch, an
/// untrusted fence around relayed chat text, a lane brief and its completion claim, an
/// injected AGENTS.md block, the summary that opens a compacted, continued session, and the
/// lane event batch, channel context and channel topic a legacy Slack turn carries.
const HUB_MARKERS: [&str; 9] = [
    "Owner request (verbatim):",
    "```untrusted",
    "[CLAIM_DONE]",
    "Claim completion with a standalone line",
    "# AGENTS.md instructions",
    "This session is being continued",
    "[LANE_EVENT]",
    "[Recent channel context]",
    "[Channel Topic]",
];

/// Token prefixes of credentials (OpenAI/Anthropic keys, GitHub tokens, Slack bot and app
/// tokens, AWS access keys, Stripe live keys, GitLab tokens, Google API keys, JWTs). Matched at
/// the start of a token, a run of letters, digits, `_` and `-`, so a key inside quotes, JSON or
/// `NAME=value` is still found while ordinary words such as "task-based" are not taken for one.
const SECRET_PREFIXES: [&str; 10] = [
    "sk-", "ghp_", "github_pat_", "xox", "AKIA", "xapp-", "sk_live_", "glpat-", "AIza", "eyJ",
];

/// A credential recognized by its shape rather than its prefix: a secret, token, password or
/// API key name followed by an assignment (`OPENAI_API_KEY=`, `"token":`, `DB_SECRET:`), or a
/// bearer credential. A name with no `=` or `:` after it ("token-efficient answers", "rotate
/// secrets quarterly") is ordinary prose and still teaches.
static SECRET_SHAPE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(secret|token|passw|api[_-]?key)\w*\s*["']?\s*[=:]|\bbearer\s+\S{8,}"#).unwrap()
});

/// One user turn of a provider session, in session order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportTurn {
    pub ordinal: usize,
    pub text: String,
}

/// One provider session (`claude`, `codex`, `omo`, ...) and its user turns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportSession {
    pub provider: String,
    pub session_id: String,
    pub cwd: Option<String>,
    pub turns: Vec<ImportTurn>,
}

/// How much one import reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportBounds {
    /// Sessions taken, in the order given (the caller passes the most recent first).
    pub max_sessions: usize,
    /// Kept turns per session, the latest ones.
    pub max_turns_per_session: usize,
    /// Characters kept of one turn.
    pub max_chars: usize,
}

impl Default for ImportBounds {
    fn default() -> Self {
        Self {
            max_sessions: 20,
            max_turns_per_session: 10,
            max_chars: 2000,
        }
    }
}

/// What one import did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    /// Sessions read and recorded.
    pub sessions: usize,
    /// Turns sent to owner ingest by this import.
    pub turns_ingested: usize,
    /// Turns an earlier import already ingested.
    pub duplicates: usize,
    /// Sessions not read: hub-driven, unchanged since the last import, or past the bound.
    pub skipped: usize,
    /// Turns owner ingest did not accept.
    pub failed: usize,
}

/// Provider session ids the hub itself drove: every session a supervisor dispatch reached or a
/// goal's execution is bound to. Their user turns are the hub's instructions, not the owner's.
pub async fn hub_driven_session_ids(pool: &SqlitePool) -> Result<HashSet<String>, SupervisorError> {
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT provider_session_id FROM supervisor_dispatches WHERE provider_session_id IS NOT NULL
         UNION
         SELECT provider_session_id FROM supervisor_execution_bindings
          WHERE provider_session_id IS NOT NULL",
    )
    .fetch_all(pool)
    .await?;
    Ok(ids.into_iter().collect())
}

/// Whether `text` holds something shaped like a credential ([`SECRET_PREFIXES`],
/// [`SECRET_SHAPE`] or a PEM block).
pub fn looks_like_secret(text: &str) -> bool {
    text.contains("-----BEGIN")
        || SECRET_SHAPE.is_match(text)
        || text
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
            .any(|token| SECRET_PREFIXES.iter().any(|prefix| token.starts_with(prefix)))
}

/// The learnable owner text of one user turn, or `None` when it must not teach: empty, a
/// tagged wrapper (`<...`), hub or harness text (`[ferryx ...` or any of [`HUB_MARKERS`]), or
/// anything that looks like a credential. Task-only lines are removed; the rest is cut to
/// `max_chars` characters.
pub fn owner_turn_text(text: &str, max_chars: usize) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty()
        || trimmed.starts_with('<')
        || trimmed.starts_with("[ferryx")
        || HUB_MARKERS.iter().any(|marker| trimmed.contains(marker))
        || looks_like_secret(trimmed)
    {
        return None;
    }
    let (learnable, _) = split_task_only(trimmed);
    let kept: String = learnable.chars().take(max_chars).collect();
    let kept = kept.trim();
    (!kept.is_empty()).then(|| kept.to_owned())
}

/// `<provider>:<session>:<ordinal>`, with the session id hashed when the id would be too long.
fn source_message_id(provider: &str, session_id: &str, ordinal: usize) -> String {
    let plain = format!("{provider}:{session_id}:{ordinal}");
    if plain.chars().count() <= MAX_SOURCE_ID_CHARS {
        return plain;
    }
    format!(
        "{provider}:{}:{ordinal}",
        crate::usage::sha256_hex(session_id)
    )
}

/// The project whose git common dir on some host is exactly `cwd/.git` (or `cwd` itself);
/// `None` unless exactly one project matches.
async fn project_for_cwd(pool: &SqlitePool, cwd: &str) -> Result<Option<String>, SupervisorError> {
    let dot_git = Path::new(cwd).join(".git").to_string_lossy().into_owned();
    let projects: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT project_id FROM supervisor_project_hosts
          WHERE git_common_dir = ? OR git_common_dir = ? LIMIT 2",
    )
    .bind(cwd)
    .bind(&dot_git)
    .fetch_all(pool)
    .await?;
    Ok(match projects.as_slice() {
        [only] => Some(only.clone()),
        _ => None,
    })
}

async fn stored_hash(
    pool: &SqlitePool,
    provider: &str,
    session_id: &str,
) -> Result<Option<String>, SupervisorError> {
    Ok(sqlx::query_scalar(
        "SELECT content_sha256 FROM supervisor_memory_imports
          WHERE provider = ? AND provider_session_id = ?",
    )
    .bind(provider)
    .bind(session_id)
    .fetch_optional(pool)
    .await?)
}

async fn record(
    pool: &SqlitePool,
    session: &ImportSession,
    content_sha256: &str,
    turns: usize,
    state: &str,
) -> Result<(), SupervisorError> {
    sqlx::query(
        "INSERT INTO supervisor_memory_imports(provider, provider_session_id, content_sha256,
            turns, state, imported_at)
         VALUES(?,?,?,?,?,?)
         ON CONFLICT(provider, provider_session_id) DO UPDATE SET
            content_sha256 = excluded.content_sha256, turns = excluded.turns,
            state = excluded.state, imported_at = excluded.imported_at",
    )
    .bind(&session.provider)
    .bind(&session.session_id)
    .bind(content_sha256)
    .bind(i64::try_from(turns).unwrap_or(i64::MAX))
    .bind(state)
    .bind(now_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

/// Seeds owner memory from `sessions` for the installation owner `principal`. Sessions the hub
/// drove and sessions unchanged since a successful import are skipped; every kept owner turn
/// ([`owner_turn_text`], latest `max_turns_per_session`) is ingested once with the source id
/// `<provider>:<session>:<ordinal>`. A session's project is mapped only from an exact git dir
/// match. Refused with nothing written when `principal` is not the installation's owner.
pub async fn import_sessions(
    pool: &SqlitePool,
    memory: &dyn OwnerMemory,
    principal: &str,
    sessions: Vec<ImportSession>,
    bounds: ImportBounds,
) -> Result<ImportReport, SupervisorError> {
    let report = import_sessions_budgeted(pool, memory, principal, sessions, bounds, None).await?;
    sqlx::query(
        "INSERT INTO hub_meta(key, value) VALUES(?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(LAST_RUN_KEY)
    .bind(now_rfc3339())
    .execute(pool)
    .await?;
    Ok(report)
}

/// [`import_sessions`] that sends at most `budget` new turns to owner ingest (`None`: no
/// limit). A turn an earlier pass already ingested costs nothing. When the budget runs out
/// inside a session, that session is left unrecorded and it and every later session count as
/// skipped, so the next pass resumes there and a large backlog drains over passes. Unlike
/// [`import_sessions`] it does not mark the setup seed as run ([`LAST_RUN_KEY`]).
pub async fn import_sessions_budgeted(
    pool: &SqlitePool,
    memory: &dyn OwnerMemory,
    principal: &str,
    sessions: Vec<ImportSession>,
    bounds: ImportBounds,
    budget: Option<usize>,
) -> Result<ImportReport, SupervisorError> {
    if identity_of(pool, principal).await?.is_none() {
        return Err(SupervisorError::NotFound {
            kind: "supervisor identity",
            id: principal.to_owned(),
        });
    }
    let driven = hub_driven_session_ids(pool).await?;
    let mut report = ImportReport::default();
    let total = sessions.len();
    let mut taken = 0usize;
    for session in sessions.into_iter().take(bounds.max_sessions) {
        if budget.is_some_and(|budget| report.turns_ingested >= budget) {
            break;
        }
        taken += 1;
        if session.provider.trim().is_empty()
            || session.session_id.trim().is_empty()
            || driven.contains(&session.session_id)
        {
            report.skipped += 1;
            continue;
        }
        let mut kept: Vec<(usize, String)> = session
            .turns
            .iter()
            .filter_map(|turn| owner_turn_text(&turn.text, bounds.max_chars).map(|text| (turn.ordinal, text)))
            .collect();
        if kept.len() > bounds.max_turns_per_session {
            kept.drain(..kept.len() - bounds.max_turns_per_session);
        }
        let content = kept
            .iter()
            .map(|(ordinal, text)| format!("{ordinal}\u{0}{text}"))
            .collect::<Vec<_>>()
            .join("\u{1}");
        let hash = crate::usage::sha256_hex(&content);
        if stored_hash(pool, &session.provider, &session.session_id).await?.as_deref() == Some(hash.as_str()) {
            let state: Option<String> = sqlx::query_scalar(
                "SELECT state FROM supervisor_memory_imports WHERE provider = ? AND provider_session_id = ?",
            )
            .bind(&session.provider)
            .bind(&session.session_id)
            .fetch_optional(pool)
            .await?;
            if state.as_deref() == Some("imported") {
                report.skipped += 1;
                continue;
            }
        }
        let project = match session.cwd.as_deref() {
            Some(cwd) if !cwd.trim().is_empty() => project_for_cwd(pool, cwd).await?,
            _ => None,
        };
        let (mut ok, mut failed) = (0usize, 0usize);
        let mut exhausted = false;
        let origin = format!("import {} {}", session.provider, session.session_id);
        for (ordinal, text) in &kept {
            if budget.is_some_and(|budget| report.turns_ingested >= budget) {
                exhausted = true;
                break;
            }
            let source = source_message_id(&session.provider, &session.session_id, *ordinal);
            let fenced = crate::chat::fence::fence_untrusted(&origin, text);
            let outcome = ingest(
                pool,
                memory,
                principal,
                ChannelProvider::Import,
                &source,
                project.as_deref(),
                &fenced,
            )
            .await;
            match outcome["state"].as_str() {
                Some("ingested") if outcome["duplicate"] == true => {
                    report.duplicates += 1;
                    ok += 1;
                }
                Some("ingested") => {
                    report.turns_ingested += 1;
                    ok += 1;
                }
                Some("skipped") => {}
                _ => {
                    report.failed += 1;
                    failed += 1;
                }
            }
        }
        if exhausted {
            // Not recorded: the next pass reads it again and resumes after its ingested turns.
            report.skipped += 1;
            continue;
        }
        let state = match (ok, failed) {
            (_, 0) => "imported",
            (0, _) => "failed",
            _ => "partial",
        };
        record(pool, &session, &hash, ok, state).await?;
        report.sessions += 1;
    }
    report.skipped += total.saturating_sub(taken);
    Ok(report)
}

/// Records one import pass under the `hub_meta` key `key`: when it started and ended, the
/// report, `extra` fields, and the token sums of the engine calls attributed to `cause` since
/// `started_at` (null when the provider reported none). Usage is written by the engine's meter
/// in its own task, so a call that finished just before this read may be counted in the next
/// pass's sums instead; the per-call rows stay exact. Returns the recorded JSON.
pub async fn record_pass(
    pool: &SqlitePool,
    key: &str,
    cause: &str,
    started_at: &str,
    report: &ImportReport,
    extra: serde_json::Value,
) -> Result<serde_json::Value, SupervisorError> {
    let (calls, input, output): (i64, Option<i64>, Option<i64>) = sqlx::query_as(
        "SELECT COUNT(*), SUM(input_tokens), SUM(output_tokens) FROM supervisor_model_calls
          WHERE cause = ? AND julianday(created_at) >= julianday(?)",
    )
    .bind(cause)
    .bind(started_at)
    .fetch_one(pool)
    .await?;
    let mut value = serde_json::json!({
        "startedAt": started_at,
        "finishedAt": now_rfc3339(),
        "report": report,
        "engineCalls": calls,
        "inputTokens": input,
        "outputTokens": output,
    });
    if let (Some(fields), serde_json::Value::Object(more)) = (value.as_object_mut(), extra) {
        fields.extend(more);
    }
    sqlx::query(
        "INSERT INTO hub_meta(key, value) VALUES(?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(value.to_string())
    .execute(pool)
    .await?;
    Ok(value)
}

/// The pass [`record_pass`] last recorded under `key`, if any.
pub async fn last_pass(pool: &SqlitePool, key: &str) -> Result<Option<serde_json::Value>, SupervisorError> {
    let stored: Option<String> = sqlx::query_scalar("SELECT value FROM hub_meta WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    Ok(stored.and_then(|text| serde_json::from_str(&text).ok()))
}

/// What earlier imports recorded: sessions, turns and the latest import time.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportTotals {
    pub sessions: i64,
    pub turns: i64,
    pub last_at: Option<String>,
}

pub async fn import_totals(pool: &SqlitePool) -> Result<ImportTotals, SupervisorError> {
    let (sessions, turns, last_at): (i64, i64, Option<String>) = sqlx::query_as(
        "SELECT COUNT(*), COALESCE(SUM(turns), 0),
                (SELECT MAX(at) FROM (SELECT MAX(imported_at) AS at FROM supervisor_memory_imports
                                       WHERE provider <> ?
                                      UNION ALL
                                      SELECT value FROM hub_meta WHERE key = ?))
           FROM supervisor_memory_imports",
    )
    .bind(OMO_MEMORY_PROVIDER)
    .bind(LAST_RUN_KEY)
    .fetch_one(pool)
    .await?;
    Ok(ImportTotals {
        sessions,
        turns,
        last_at,
    })
}

/// `hub_meta` key: version of the session reader the last completed seed ran with. A seed
/// whose reader version differs from the caller's is run again, so a reader fix reaches
/// sessions an earlier, broken reader imported as empty (their content hash changes).
pub const READER_VERSION_KEY: &str = "supervisor_memory_import_reader";

/// The reader version the last completed seed recorded, if any.
pub async fn reader_version(pool: &SqlitePool) -> Result<Option<String>, SupervisorError> {
    Ok(sqlx::query_scalar("SELECT value FROM hub_meta WHERE key = ?")
        .bind(READER_VERSION_KEY)
        .fetch_optional(pool)
        .await?)
}

/// Records that a seed completed with reader `version`.
pub async fn set_reader_version(pool: &SqlitePool, version: &str) -> Result<(), SupervisorError> {
    sqlx::query(
        "INSERT INTO hub_meta(key, value) VALUES(?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(READER_VERSION_KEY)
    .bind(version)
    .execute(pool)
    .await?;
    Ok(())
}
