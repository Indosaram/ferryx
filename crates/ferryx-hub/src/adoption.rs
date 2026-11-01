//! First-rollout identity adoption (plan todo 13): one surviving supervisor per account.
//!
//! The first machine that acquires the supervisor lease keeps its identity. Every other hub,
//! once it learns it is Standby for another machine, archives its database once as
//! `hub.sqlite.pre-lease-<ts>` ([`archive_before_replace`]); the archive is never written or
//! deleted. Its open goals (not completed or cancelled) are listed as adoptable
//! ([`adoptable_goals`]) until the owner adopts them: [`adopt_archived`] sends each one's
//! original request text, contract and project identity to the holder as a
//! `supervisor.request` `submit` whose `requestId` is `archived:<supervisor_id>:<goal_id>`.
//! The holder admits it idempotently by that source id, so a re-adopt returns the same goal.
//! Adoptions are recorded in `<hub_dir>/adopted-goals.json`, never in the archive. Nothing is
//! merged and nothing is adopted without an owner action.
use std::fs;
use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;

use crate::goals::contract::RequirementSpec;
use crate::supervisor::admission::{OperationClass, SubmitWork};
use crate::supervisor::request::RequestFailure;

/// File name prefix of an archived hub database: `hub.sqlite.pre-lease-<unix seconds>`. The
/// checkpoint import treats a hub directory holding one as fresh.
pub const ARCHIVE_PREFIX: &str = "hub.sqlite.pre-lease-";
/// The adoption ledger beside the hub database.
pub const ADOPTION_LEDGER: &str = "adopted-goals.json";
/// Source id prefix of an archived goal's adoption: `archived:<supervisor_id>:<goal_id>`.
pub const ARCHIVED_SOURCE_PREFIX: &str = "archived:";

const LIVE_DB: &str = "hub.sqlite";
/// Temporary names of an archive being written; they never start with [`ARCHIVE_PREFIX`].
const PARTIAL_PREFIX: &str = ".pre-lease-partial-";

/// Failure of an archive read or an adoption.
#[derive(Debug, thiserror::Error)]
pub enum AdoptionError {
    #[error("no archived hub database (hub.sqlite.pre-lease-*) in this hub directory")]
    NoArchive,
    #[error("goal {0} is not an open goal of the archived hub")]
    NotFound(String),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Sql(#[from] sqlx::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl AdoptionError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NoArchive | Self::NotFound(_) => "NOT_FOUND",
            Self::Io(_) | Self::Sql(_) | Self::Json(_) => "STORAGE",
        }
    }
}

/// An open goal of the archived hub that the owner can adopt into the holder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdoptableGoal {
    pub goal_id: String,
    pub title: String,
    /// The machine whose archived hub holds the goal ("Adoptable from <machine>").
    pub from_machine: String,
    pub supervisor_id: String,
    pub project_name: String,
    pub run_state: String,
}

/// One archived open goal with what its adoption sends to the holder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchivedGoal {
    pub goal_id: String,
    pub supervisor_id: String,
    pub run_state: String,
    pub title: String,
    pub project_name: String,
    pub repo_identity: String,
    /// The original owner request text (the goal's first admitted request).
    pub request: String,
    pub requirements: Vec<RequirementSpec>,
    pub operations: Vec<OperationClass>,
}

impl ArchivedGoal {
    /// `archived:<supervisor_id>:<goal_id>`: the holder's idempotency key for this adoption.
    pub fn source_id(&self) -> String {
        format!("{ARCHIVED_SOURCE_PREFIX}{}:{}", self.supervisor_id, self.goal_id)
    }

    /// The `supervisor.request` envelope that adopts this goal into the holder.
    pub fn envelope(&self) -> Result<Value, serde_json::Error> {
        let work = SubmitWork {
            project_id: None,
            host_id: None,
            kind: None,
            title: self.title.clone(),
            instruction: self.request.clone(),
            requirements: self.requirements.clone(),
            cwd: None,
            operations: self.operations.clone(),
            turn_limit: None,
        };
        Ok(json!({
            "requestId": self.source_id(),
            "action": "submit",
            "payload": {
                "request": self.request,
                "work": serde_json::to_value(&work)?,
                "repoIdentity": self.repo_identity,
            },
        }))
    }
}

/// `(supervisor_id, goal_id)` of an archived adoption's source id, `None` for any other id.
pub fn parse_archived_source(source_id: &str) -> Option<(String, String)> {
    let rest = source_id.strip_prefix(ARCHIVED_SOURCE_PREFIX)?;
    let (supervisor_id, goal_id) = rest.split_once(':')?;
    if supervisor_id.is_empty() || goal_id.is_empty() || goal_id.contains(':') {
        return None;
    }
    Some((supervisor_id.to_owned(), goal_id.to_owned()))
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// The newest archive in `hub_dir` (by its timestamp), `None` when there is none. Blocking.
pub fn latest_archive(hub_dir: &Path) -> io::Result<Option<PathBuf>> {
    let entries = match fs::read_dir(hub_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut newest: Option<(i64, PathBuf)> = None;
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(stamp) = name
            .strip_prefix(ARCHIVE_PREFIX)
            .and_then(|stamp| stamp.parse::<i64>().ok())
        else {
            continue;
        };
        if newest.as_ref().is_none_or(|(held, _)| stamp > *held) {
            newest = Some((stamp, entry.path()));
        }
    }
    Ok(newest.map(|(_, path)| path))
}

/// Copies `<hub_dir>/hub.sqlite` (and its `-wal`) to `hub.sqlite.pre-lease-<now>` once.
/// Returns the archive written now; `None` when the hub was archived before (idempotent) or
/// has no database. The live database is left in place and nothing is ever deleted. Called on
/// the first Standby for another holder and before a `--replace-local` import. Blocking.
pub fn archive_before_replace(hub_dir: &Path, now: i64) -> io::Result<Option<PathBuf>> {
    if latest_archive(hub_dir)?.is_some() {
        return Ok(None);
    }
    let live = hub_dir.join(LIVE_DB);
    if !live.is_file() {
        return Ok(None);
    }
    let archive = hub_dir.join(format!("{ARCHIVE_PREFIX}{now}"));
    let partial = hub_dir.join(format!("{PARTIAL_PREFIX}{now}"));
    fs::copy(&live, &partial)?;
    let wal = with_suffix(&live, "-wal");
    let partial_wal = with_suffix(&partial, "-wal");
    let has_wal = wal.is_file();
    if has_wal {
        fs::copy(&wal, &partial_wal)?;
    }
    // The main file names the archive: once it exists, the hub counts as archived.
    if has_wal {
        fs::rename(&partial_wal, with_suffix(&archive, "-wal"))?;
    }
    fs::rename(&partial, &archive)?;
    Ok(Some(archive))
}

/// One adoption recorded in [`ADOPTION_LEDGER`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdoptedEntry {
    pub source_id: String,
    pub supervisor_id: String,
    pub goal_id: String,
    /// The goal the holder admitted for it.
    pub holder_goal_id: Option<String>,
    pub adopted_at: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Ledger {
    #[serde(default)]
    adopted: Vec<AdoptedEntry>,
}

fn read_ledger(hub_dir: &Path) -> Result<Ledger, AdoptionError> {
    match fs::read(hub_dir.join(ADOPTION_LEDGER)) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Ledger::default()),
        Err(error) => Err(error.into()),
    }
}

/// Adds `entry` to the ledger (one entry per source id), written atomically.
fn record_adopted(hub_dir: &Path, entry: AdoptedEntry) -> Result<(), AdoptionError> {
    let mut ledger = read_ledger(hub_dir)?;
    ledger
        .adopted
        .retain(|held| held.source_id != entry.source_id);
    ledger.adopted.push(entry);
    let path = hub_dir.join(ADOPTION_LEDGER);
    let tmp = with_suffix(&path, ".tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(&ledger)?)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

/// Every adoption recorded in this hub directory's ledger.
pub fn adopted(hub_dir: &Path) -> Result<Vec<AdoptedEntry>, AdoptionError> {
    Ok(read_ledger(hub_dir)?.adopted)
}

/// Reads the open goals of the newest archive. The archive (and its `-wal`) is copied to a
/// scratch directory first and only the copy is opened, so the archive is never written.
pub async fn archived_goals(hub_dir: &Path) -> Result<Vec<ArchivedGoal>, AdoptionError> {
    let dir = hub_dir.to_owned();
    let Some(archive) = tokio::task::spawn_blocking(move || latest_archive(&dir))
        .await
        .map_err(io::Error::other)??
    else {
        return Err(AdoptionError::NoArchive);
    };
    let scratch = hub_dir.join(format!(".adopt-read-{}", uuid::Uuid::new_v4()));
    let copy = scratch.join(LIVE_DB);
    let (from, to) = (archive, copy.clone());
    tokio::task::spawn_blocking(move || -> io::Result<()> {
        fs::create_dir_all(to.parent().unwrap_or(Path::new(".")))?;
        fs::copy(&from, &to)?;
        let wal = with_suffix(&from, "-wal");
        if wal.is_file() {
            fs::copy(&wal, with_suffix(&to, "-wal"))?;
        }
        Ok(())
    })
    .await
    .map_err(io::Error::other)??;
    let result = read_open_goals(&copy).await;
    let cleanup = scratch.clone();
    if let Err(error) = tokio::task::spawn_blocking(move || fs::remove_dir_all(cleanup))
        .await
        .map_err(io::Error::other)
        .and_then(|removed| removed)
    {
        tracing::warn!(%error, scratch = %scratch.display(), "archive read copy not removed");
    }
    result
}

type GoalRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
);

async fn read_open_goals(copy: &Path) -> Result<Vec<ArchivedGoal>, AdoptionError> {
    let options = SqliteConnectOptions::new()
        .filename(copy)
        .create_if_missing(false);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;
    let goals = open_goals(&pool).await;
    pool.close().await;
    goals
}

async fn open_goals(pool: &SqlitePool) -> Result<Vec<ArchivedGoal>, AdoptionError> {
    let tables: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table'
         AND name IN ('supervisor_goals','supervisor_projects','supervisor_requests','lanes','lane_goal_requirements')",
    )
    .fetch_one(pool)
    .await?;
    if tables < 5 {
        return Ok(Vec::new());
    }
    let rows: Vec<GoalRow> = sqlx::query_as(
        "SELECT g.goal_id, g.supervisor_id, g.run_state, g.operation_grant, g.lane_key,
                l.title, p.name, p.repo_identity,
                (SELECT r.body FROM supervisor_requests r WHERE r.goal_id = g.goal_id
                 ORDER BY r.id LIMIT 1)
         FROM supervisor_goals g
         JOIN lanes l ON l.key = g.lane_key
         JOIN supervisor_projects p ON p.project_id = g.project_id
         WHERE g.run_state NOT IN ('completed','cancelled')
         ORDER BY g.created_at, g.goal_id",
    )
    .fetch_all(pool)
    .await?;
    let mut goals = Vec::with_capacity(rows.len());
    for (goal_id, supervisor_id, run_state, grant, lane_key, title, project_name, repo, body) in
        rows
    {
        let requirements: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT req_id, text, verifier FROM lane_goal_requirements
             WHERE lane_key = ? ORDER BY rowid",
        )
        .bind(&lane_key)
        .fetch_all(pool)
        .await?;
        let operations: Vec<OperationClass> = serde_json::from_str(&grant)?;
        goals.push(ArchivedGoal {
            goal_id,
            supervisor_id,
            run_state,
            request: body.unwrap_or_else(|| title.clone()),
            title,
            project_name,
            repo_identity: repo,
            requirements: requirements
                .into_iter()
                .map(|(id, text, verifier)| RequirementSpec { id, text, verifier })
                .collect(),
            operations,
        });
    }
    Ok(goals)
}

/// Open goals of the archived hub not yet recorded in the adoption ledger, as status and the
/// standby UI list them ("Adoptable from `from_machine`"). Empty without an archive.
pub async fn adoptable_goals(
    hub_dir: &Path,
    from_machine: &str,
) -> Result<Vec<AdoptableGoal>, AdoptionError> {
    let goals = match archived_goals(hub_dir).await {
        Ok(goals) => goals,
        Err(AdoptionError::NoArchive) => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let ledger = read_ledger(hub_dir)?;
    Ok(goals
        .into_iter()
        .filter(|goal| {
            let source = goal.source_id();
            !ledger.adopted.iter().any(|held| held.source_id == source)
        })
        .map(|goal| AdoptableGoal {
            goal_id: goal.goal_id,
            title: goal.title,
            from_machine: from_machine.to_owned(),
            supervisor_id: goal.supervisor_id,
            project_name: goal.project_name,
            run_state: goal.run_state,
        })
        .collect())
}

/// Which archived goals an owner's Adopt action takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdoptSelection {
    /// Every open goal not yet in the ledger.
    All,
    /// One open goal, also when the ledger already has it: the holder dedupes by source id.
    Goal(String),
}

/// What one adoption returned.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdoptReport {
    pub goal_id: String,
    pub source_id: String,
    /// True when the holder registered the goal (admitted or awaiting the owner).
    pub adopted: bool,
    pub answer: Option<Value>,
    pub error: Option<Value>,
}

/// The owner's Adopt action: sends each selected archived goal to the holder through `send`
/// (a `supervisor.request` envelope; a standby forwards it to the holder) and records every
/// goal the holder registered in the adoption ledger. A failure is reported per goal and
/// recorded nowhere, so the next Adopt retries it under the same source id.
pub async fn adopt_archived<F, Fut>(
    hub_dir: &Path,
    selection: &AdoptSelection,
    mut send: F,
) -> Result<Vec<AdoptReport>, AdoptionError>
where
    F: FnMut(Value) -> Fut,
    Fut: Future<Output = Result<Value, RequestFailure>>,
{
    let goals = archived_goals(hub_dir).await?;
    let selected: Vec<ArchivedGoal> = match selection {
        AdoptSelection::All => {
            let ledger = read_ledger(hub_dir)?;
            goals
                .into_iter()
                .filter(|goal| {
                    let source = goal.source_id();
                    !ledger.adopted.iter().any(|held| held.source_id == source)
                })
                .collect()
        }
        AdoptSelection::Goal(goal_id) => {
            let goal = goals
                .into_iter()
                .find(|goal| &goal.goal_id == goal_id)
                .ok_or_else(|| AdoptionError::NotFound(goal_id.clone()))?;
            vec![goal]
        }
    };
    let mut reports = Vec::with_capacity(selected.len());
    for goal in selected {
        let source_id = goal.source_id();
        let report = match send(goal.envelope()?).await {
            Ok(answer) => {
                let registered = matches!(
                    answer.get("outcome").and_then(Value::as_str),
                    Some("admitted" | "awaiting_owner")
                );
                if registered {
                    record_adopted(
                        hub_dir,
                        AdoptedEntry {
                            source_id: source_id.clone(),
                            supervisor_id: goal.supervisor_id.clone(),
                            goal_id: goal.goal_id.clone(),
                            holder_goal_id: answer
                                .pointer("/receipt/goal_id")
                                .and_then(Value::as_str)
                                .map(str::to_owned),
                            adopted_at: crate::storage::now_rfc3339(),
                        },
                    )?;
                }
                AdoptReport {
                    goal_id: goal.goal_id,
                    source_id,
                    adopted: registered,
                    answer: Some(answer),
                    error: None,
                }
            }
            Err(failure) => AdoptReport {
                goal_id: goal.goal_id,
                source_id,
                adopted: false,
                answer: None,
                error: Some(failure.to_json()),
            },
        };
        reports.push(report);
    }
    Ok(reports)
}

#[cfg(test)]
mod tests;
