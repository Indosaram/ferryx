//! Worker evidence for a lane run (todo 7): `lane_report` and `lane_step` records. Progress is
//! measured by new evidence only: a file whose sha256 was not yet recorded for this lane run,
//! or a remote effect (a pushed commit, an existing pull request) the hub verified itself.
//! Report text, screen text and tool-call ids are never evidence: the hub cannot attest them.
use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use crate::agent_control::LOCAL_HOST_ID;
use crate::storage::WriteTx;

const MAX_TEXT_CHARS: usize = 4_000;
const MAX_FILES: usize = 32;
const MAX_RECEIPTS: usize = 8;
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const VERIFY_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportKind {
    Progress,
    Stuck,
    Waiting,
    Stopped,
    Finished,
}

impl ReportKind {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "progress" => Some(Self::Progress),
            "stuck" => Some(Self::Stuck),
            "waiting" => Some(Self::Waiting),
            "stopped" => Some(Self::Stopped),
            "finished" => Some(Self::Finished),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Progress => "progress",
            Self::Stuck => "stuck",
            Self::Waiting => "waiting",
            Self::Stopped => "stopped",
            Self::Finished => "finished",
        }
    }
}

/// The authenticated connection a report arrived on: the reporting pane's own session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reporter {
    pub host_id: String,
    pub session_id: String,
}

/// A remote effect a step claims. Only these two kinds exist: the hub can check both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Receipt {
    Commit(String),
    PullRequest(String),
}

impl Receipt {
    /// `{"commit": "<7-40 hex>"}` or `{"pr": "https://github.com/..."}`. Anything else (a
    /// tool-call id, a free-form claim) is refused as unattestable.
    pub fn parse(value: &Value) -> Result<Self, ReportError> {
        if let Some(sha) = value.get("commit").and_then(Value::as_str) {
            let sha = sha.trim().to_ascii_lowercase();
            if (7..=40).contains(&sha.len()) && sha.chars().all(|c| c.is_ascii_hexdigit()) {
                return Ok(Self::Commit(sha));
            }
            return Err(ReportError::Unattestable(format!("commit {sha} is not a 7-40 hex sha")));
        }
        if let Some(url) = value.get("pr").and_then(Value::as_str) {
            let url = url.trim();
            if url.starts_with("https://github.com/") && url.contains("/pull/") {
                return Ok(Self::PullRequest(url.to_owned()));
            }
            return Err(ReportError::Unattestable(format!("{url} is not a GitHub pull request URL")));
        }
        Err(ReportError::Unattestable(format!(
            "{value} is not a commit or pull request receipt; tool-call ids are not evidence"
        )))
    }

    pub fn identity(&self) -> String {
        match self {
            Self::Commit(sha) => format!("commit:{sha}"),
            Self::PullRequest(url) => format!("pr:{url}"),
        }
    }
}

/// Checks a receipt against the remote. `true` only when the remote shows the effect.
#[async_trait]
pub trait RemoteVerifier: Send + Sync {
    async fn verify(&self, cwd: &Path, receipt: &Receipt) -> bool;
}

/// `git ls-remote origin` for a commit (it must be the tip of a remote ref), `gh pr view` for a PR.
pub struct GitRemote;

#[async_trait]
impl RemoteVerifier for GitRemote {
    async fn verify(&self, cwd: &Path, receipt: &Receipt) -> bool {
        let mut command = match receipt {
            Receipt::Commit(_) => {
                let mut command = tokio::process::Command::new("git");
                command.args(["ls-remote", "origin"]);
                command
            }
            Receipt::PullRequest(url) => {
                let mut command = tokio::process::Command::new("gh");
                command.args(["pr", "view", url.as_str(), "--json", "url"]);
                command
            }
        };
        let run = command.current_dir(cwd).kill_on_drop(true).output();
        let Ok(Ok(output)) = tokio::time::timeout(VERIFY_TIMEOUT, run).await else {
            return false;
        };
        if !output.status.success() {
            return false;
        }
        match receipt {
            Receipt::Commit(sha) => String::from_utf8_lossy(&output.stdout).lines().any(|line| {
                line.split_whitespace().next().is_some_and(|hash| hash.starts_with(sha.as_str()))
            }),
            Receipt::PullRequest(_) => true,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ReportError {
    #[error("INVALID_REQUEST: {0}")]
    Invalid(String),
    #[error("NOT_LANE_WORKER: session {session} on host {host} is not the worker of lane {lane}")]
    NotLaneWorker { lane: String, host: String, session: String },
    #[error("UNATTESTABLE: {0}")]
    Unattestable(String),
    #[error("FILE_CHANGED: {0} changed while it was read")]
    FileChanged(String),
    #[error("FILE_UNREADABLE: {path}: {why}")]
    FileUnreadable { path: String, why: String },
    #[error(transparent)]
    Store(#[from] crate::error::HubError),
}

impl From<sqlx::Error> for ReportError {
    fn from(error: sqlx::Error) -> Self {
        Self::Store(error.into())
    }
}

/// One report as the worker sent it.
pub struct ReportInput<'a> {
    pub lane_key: &'a str,
    pub kind: ReportKind,
    pub text: &'a str,
    pub files: &'a [String],
    pub receipts: &'a [Receipt],
}

/// What a report changed. `new_items` empty means the quiet clock did not move.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportOutcome {
    pub run: i64,
    pub new_items: Vec<String>,
    pub repeated: Vec<String>,
    pub unverified: Vec<String>,
    pub last_progress_at_ms: i64,
}

/// Records one report of `reporter` for its own lane. Files are hashed and receipts verified
/// before anything is written; a refused report writes nothing.
pub async fn report(
    pool: &SqlitePool,
    reporter: &Reporter,
    verifier: &dyn RemoteVerifier,
    input: &ReportInput<'_>,
    now_ms: i64,
) -> Result<ReportOutcome, ReportError> {
    let chars = input.text.trim().chars().count();
    if chars == 0 || chars > MAX_TEXT_CHARS {
        return Err(ReportError::Invalid(format!("text must be 1-{MAX_TEXT_CHARS} characters")));
    }
    if input.files.len() > MAX_FILES || input.receipts.len() > MAX_RECEIPTS {
        return Err(ReportError::Invalid(format!(
            "at most {MAX_FILES} files and {MAX_RECEIPTS} receipts per report"
        )));
    }
    let cwd = worker_lane_cwd(pool, reporter, input.lane_key).await?;
    let mut items = Vec::with_capacity(input.files.len() + input.receipts.len());
    for file in input.files {
        let path = inside_cwd(&cwd, file).await?;
        items.push((hash_file(&path).await?, file.clone()));
    }
    let mut unverified = Vec::new();
    for receipt in input.receipts {
        if verifier.verify(Path::new(&cwd), receipt).await {
            items.push((receipt.identity(), receipt.identity()));
        } else {
            unverified.push(receipt.identity());
        }
    }
    let mut tx = pool.begin_write().await?;
    let run = super::oversight::ensure_run_in(&mut tx, input.lane_key, now_ms).await?;
    let (mut new_items, mut repeated) = (Vec::new(), Vec::new());
    for (identity, label) in items {
        let inserted = sqlx::query(
            "INSERT INTO lane_evidence(lane_key, run, identity, label, recorded_at_ms) VALUES(?,?,?,?,?)
             ON CONFLICT(lane_key, run, identity) DO NOTHING",
        )
        .bind(input.lane_key).bind(run).bind(&identity).bind(&label).bind(now_ms)
        .execute(&mut *tx)
        .await?;
        if inserted.rows_affected() == 1 { new_items.push(label) } else { repeated.push(label) }
    }
    sqlx::query(
        "INSERT INTO lane_reports(lane_key, run, kind, text, new_items, reporter, created_at_ms) VALUES(?,?,?,?,?,?,?)",
    )
    .bind(input.lane_key).bind(run).bind(input.kind.as_str()).bind(input.text.trim())
    .bind(i64::try_from(new_items.len()).unwrap_or(i64::MAX))
    .bind(format!("{}:{}", reporter.host_id, reporter.session_id)).bind(now_ms)
    .execute(&mut *tx)
    .await?;
    let progress = !new_items.is_empty();
    let last_progress_at_ms =
        super::oversight::note_report_in(&mut tx, input.lane_key, input.kind, progress, now_ms).await?;
    tx.commit().await?;
    super::oversight::poke();
    Ok(ReportOutcome { run, new_items, repeated, unverified, last_progress_at_ms })
}

/// The lane's working directory, when `reporter` is the lane's own pane (or the session bound
/// to the lane's open supervisor goal) on the lane's own host.
async fn worker_lane_cwd(pool: &SqlitePool, reporter: &Reporter, lane_key: &str) -> Result<String, ReportError> {
    if reporter.session_id.trim().is_empty() || reporter.host_id.trim().is_empty() {
        return Err(ReportError::Invalid("the reporting session is unknown".into()));
    }
    let row: Option<(String, Option<String>, Option<String>)> =
        sqlx::query_as("SELECT cwd, omo_thread_id, host_id FROM lanes WHERE key = ?")
            .bind(lane_key)
            .fetch_optional(pool)
            .await?;
    let Some((cwd, pane, host)) = row else {
        return Err(ReportError::Invalid(format!("lane {lane_key} not found")));
    };
    let own_pane = pane.as_deref() == Some(reporter.session_id.as_str())
        && host.as_deref().unwrap_or(LOCAL_HOST_ID) == reporter.host_id;
    let bound: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM supervisor_execution_bindings b
           JOIN supervisor_goals g ON g.goal_id = b.goal_id
          WHERE g.lane_key = ? AND b.host_id = ? AND b.provider_session_id = ?
            AND g.run_state NOT IN ('completed','cancelled'))",
    )
    .bind(lane_key).bind(&reporter.host_id).bind(&reporter.session_id)
    .fetch_one(pool)
    .await?;
    if own_pane || bound {
        return Ok(cwd);
    }
    Err(ReportError::NotLaneWorker {
        lane: lane_key.to_owned(),
        host: reporter.host_id.clone(),
        session: reporter.session_id.clone(),
    })
}

/// Resolves `file` under `cwd`, refusing any path that escapes the lane worktree.
async fn inside_cwd(cwd: &str, file: &str) -> Result<PathBuf, ReportError> {
    let unreadable = |error: std::io::Error| ReportError::FileUnreadable { path: file.to_owned(), why: error.to_string() };
    let root = tokio::fs::canonicalize(cwd).await.map_err(unreadable)?;
    let resolved = tokio::fs::canonicalize(Path::new(cwd).join(file)).await.map_err(unreadable)?;
    if !resolved.starts_with(&root) {
        return Err(ReportError::Invalid(format!("{file} is outside the lane worktree")));
    }
    Ok(resolved)
}

/// `sha256:<hex>` of the file's bytes; refused when its size or mtime moved during the read.
async fn hash_file(path: &Path) -> Result<String, ReportError> {
    let shown = path.display().to_string();
    let unreadable = |error: std::io::Error| ReportError::FileUnreadable { path: shown.clone(), why: error.to_string() };
    let before = tokio::fs::metadata(path).await.map_err(unreadable)?;
    if !before.is_file() || before.len() > MAX_FILE_BYTES {
        return Err(ReportError::Invalid(format!("{shown} is not a file of at most {MAX_FILE_BYTES} bytes")));
    }
    let bytes = tokio::fs::read(path).await.map_err(unreadable)?;
    let after = tokio::fs::metadata(path).await.map_err(unreadable)?;
    let read_len = u64::try_from(bytes.len()).ok();
    if before.len() != after.len()
        || read_len != Some(after.len())
        || before.modified().ok() != after.modified().ok()
    {
        return Err(ReportError::FileChanged(shown));
    }
    Ok(format!("sha256:{:x}", Sha256::digest(&bytes)))
}
