//! The lane's pull request, as the last read-only `gh pr view` saw it (migration 0057).
//!
//! A projection only. GitHub owns the PR: [`record`] stores what one read answered, and nothing
//! here calls `gh` at all. The read itself lives in `src-tauri/src/ipc/github_pr.rs`, which can
//! only build `gh pr view` (classified `Read` at `effect.rs:1187`); a merge or close happens in
//! the owner's own shell and shows up here as the next read's `merged` / `closed` state.
//!
//! T6's auto-settle reads the pull request joined onto each row (see
//! [`super::lifecycle::LaneLifecycle`]) for the PR-merged reason, and every sidebar row carries the
//! same record, so this module must stay a pure function of what was observed: no transitions, no
//! notifications, no `gh`.
//!
//! ## Who writes the table
//!
//! [`record`] is the only writer of `lane_pull_requests`, and its only production caller is
//! [`crate::lane_pr_sync::sync`]: that pass asks a [`PrSource`] once per lane and records the
//! answer. The trait is the seam that keeps `gh` out of this crate -- the executable call, its
//! read-only argv and its validation already exist in `src-tauri/src/ipc/github_pr.rs`, whose
//! `GhPrSource` implements it and runs the process on a blocking worker. Nothing here spawns
//! anything, so a sweep cannot block the async reactor.
//!
//! [`crate::lane_pr_sync`] is the pass that runs on a cadence and then hands the projection to
//! T6's rule.
//!
//! ## One join rule, shared with T6
//!
//! A lifecycle row is keyed by WORKTREE (`w:<wsId>/<slug>`, `p:<wsId>/<path>`) while a PR is keyed
//! by LANE (`lane_pull_requests.lane_key REFERENCES lanes(key)`). [`candidates_of`] is the single
//! rule that maps one to the other -- the slug of a `w:` key, then every lane pointing at the row's
//! path -- and both the settle sweep and the row read go through it, so a row can never show a PR
//! that the settle rule cannot see (or the other way round).
use std::collections::HashMap;

use async_trait::async_trait;
use sqlx::SqlitePool;

use crate::error::{HubError, Result};
use crate::storage::now_rfc3339;

use super::state::valid_key;

/// One observed pull request.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanePullRequest {
    pub lane_key: String,
    /// `owner/repository` the PR belongs to.
    pub repo: String,
    pub pr_number: i64,
    /// `open`, `closed`, or `merged` -- GitHub's lifecycle, as last observed.
    pub state: String,
    pub url: String,
    pub title: String,
    pub head_branch: String,
    /// When the read that first observed this state ran (RFC 3339). Unchanged while the state is
    /// unchanged, so it reads as "unchanged since" rather than "last polled".
    pub observed_at: String,
}

impl LanePullRequest {
    /// The lane's PR is merged. T6 and every settle-driven reader is the caller that cares.
    pub fn is_merged(&self) -> bool {
        self.state == "merged"
    }
}

/// One lane to ask GitHub about: the lane key, plus the directory whose Git origin names its repo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrReadTarget {
    pub lane_key: String,
    pub cwd: String,
}

/// The branch a lane's pull request lives on.
///
/// `lanes/worktree.rs` creates `lane/<key>` for a lane that owns a worktree, so this is the lane's
/// branch by construction rather than a guess. A lane running in a shared checkout has no such
/// branch, and its read answers "no pull request".
pub fn lane_branch(lane_key: &str) -> String {
    format!("lane/{lane_key}")
}

/// A read-only source of one lane's pull request.
///
/// The only implementation is `GhPrSource` in `src-tauri/src/ipc/github_pr.rs`, which owns the one
/// `gh pr view` argument vector this repo builds and runs it on a blocking worker. A trait rather
/// than a direct call so that this crate keeps spawning nothing: a test hands
/// [`crate::lane_pr_sync::sync`] answers instead of starting a process, and the production read
/// stays in the module that already proves it is read-only.
///
/// The contract an implementation owes [`crate::lane_pr_sync::sync`]:
///
/// * **never block the async reactor** -- subprocess and disk work go through `spawn_blocking`.
/// * **answer absence as `Ok(None)`** -- no `gh`, no auth, no PR for the branch, or a project whose
///   origin is not GitHub are ordinary states, not failures.
/// * **`Err` means the read failed.** The sweep counts it, names the lane, and carries on.
/// * `observed_at` may be left empty: the record step stamps the read time.
#[async_trait]
pub trait PrSource: Send + Sync {
    async fn read(&self, target: &PrReadTarget) -> Result<Option<LanePullRequest>>;
}

/// Replaces the lane's PR link with what a fresh read observed. True when the row changed.
///
/// One row per lane, so a lane that opens a second PR replaces its first rather than growing a
/// history: the projection answers "what is this lane's PR now", and `lane_history` already owns
/// state changes.
///
/// Last write wins, but only a write that says something new: an observation identical to the
/// stored one (same repo, number, state, url, title and branch) leaves the row untouched and
/// answers `false`, which keeps `observed_at` meaning "unchanged since" and keeps a five-minute
/// sweep from churning the table and every revision a client compares.
pub async fn record(pool: &SqlitePool, pr: &LanePullRequest) -> Result<bool> {
    if !valid_key(&pr.lane_key) {
        return Err(HubError::message(format!(
            "invalid lane key {}",
            pr.lane_key
        )));
    }
    if pr.pr_number <= 0 {
        return Err(HubError::message("a pull request number must be positive"));
    }
    if !matches!(pr.state.as_str(), "open" | "closed" | "merged") {
        return Err(HubError::message(format!(
            "unknown pull-request state {}",
            pr.state
        )));
    }
    let observed_at = if pr.observed_at.is_empty() {
        now_rfc3339()
    } else {
        pr.observed_at.clone()
    };
    // The WHERE clause is the idempotence guard: an observation that repeats the stored row
    // updates nothing, so `observed_at` keeps the time this state was first seen.
    let updated = sqlx::query(
        "INSERT INTO lane_pull_requests(lane_key, repo, pr_number, state, url, title, head_branch, observed_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(lane_key) DO UPDATE SET
             repo = excluded.repo,
             pr_number = excluded.pr_number,
             state = excluded.state,
             url = excluded.url,
             title = excluded.title,
             head_branch = excluded.head_branch,
             observed_at = excluded.observed_at
         WHERE lane_pull_requests.repo IS NOT excluded.repo
            OR lane_pull_requests.pr_number IS NOT excluded.pr_number
            OR lane_pull_requests.state IS NOT excluded.state
            OR lane_pull_requests.url IS NOT excluded.url
            OR lane_pull_requests.title IS NOT excluded.title
            OR lane_pull_requests.head_branch IS NOT excluded.head_branch",
    )
    .bind(&pr.lane_key)
    .bind(&pr.repo)
    .bind(pr.pr_number)
    .bind(&pr.state)
    .bind(&pr.url)
    .bind(&pr.title)
    .bind(&pr.head_branch)
    .bind(&observed_at)
    .execute(pool)
    .await?;
    Ok(updated.rows_affected() > 0)
}

/// The eight columns [`LanePullRequest`] is built from, in select order.
type LanePullRequestRow = (String, String, i64, String, String, String, String, String);

const ONE_SQL: &str = "SELECT lane_key, repo, pr_number, state, url, title, head_branch, observed_at
   FROM lane_pull_requests WHERE lane_key = ?";

const ALL_SQL: &str = "SELECT lane_key, repo, pr_number, state, url, title, head_branch, observed_at
   FROM lane_pull_requests ORDER BY lane_key";

fn from_row(
    (lane_key, repo, pr_number, state, url, title, head_branch, observed_at): LanePullRequestRow,
) -> LanePullRequest {
    LanePullRequest {
        lane_key,
        repo,
        pr_number,
        state,
        url,
        title,
        head_branch,
        observed_at,
    }
}

/// The lane's recorded PR, or None when no read has linked one yet.
pub async fn for_lane(pool: &SqlitePool, lane_key: &str) -> Result<Option<LanePullRequest>> {
    let row: Option<LanePullRequestRow> = sqlx::query_as(ONE_SQL)
        .bind(lane_key)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(from_row))
}

/// Every linked PR, one per lane: what a tracker render reads in one query.
pub async fn all(pool: &SqlitePool) -> Result<Vec<LanePullRequest>> {
    let rows: Vec<LanePullRequestRow> = sqlx::query_as(ALL_SQL).fetch_all(pool).await?;
    Ok(rows.into_iter().map(from_row).collect())
}

/// Lane keys grouped by the path that claims them.
///
/// A lane claims the directory it runs in (`lanes.cwd`, which `lanes/worktree.rs` points at the
/// lane's own worktree) and any worktree it registered (`lane_owned_worktrees.path`). Both halves
/// are the same question -- "which lanes could this path belong to" -- so they are one query.
const OWNERS_SQL: &str = "SELECT cwd, key FROM lanes WHERE cwd <> ''
   UNION
   SELECT path, lane_key FROM lane_owned_worktrees
   ORDER BY 1, 2";

/// Every path any lane claims, with its lane keys in a deterministic order.
pub async fn path_owners(pool: &SqlitePool) -> Result<HashMap<String, Vec<String>>> {
    let rows: Vec<(String, String)> = sqlx::query_as(OWNERS_SQL).fetch_all(pool).await?;
    let mut owners: HashMap<String, Vec<String>> = HashMap::new();
    for (path, lane_key) in rows {
        owners.entry(path).or_default().push(lane_key);
    }
    Ok(owners)
}

/// The lane keys a lifecycle row can own, most specific first.
///
/// A `w:<wsId>/<slug>` row names a managed worktree, so its slug is a lane key candidate. Every row
/// also claims the lanes pointing at its path. Ordered and deduplicated, so a sweep is
/// deterministic and the same lane is never queried twice. This is the ONE join rule between a
/// worktree-keyed row and a lane-keyed PR: [`for_row`], [`linked_by_row`] and T6's settle sweep all
/// call it.
pub fn candidates_of(
    worktree_key: &str,
    worktree_path: &str,
    owners: &HashMap<String, Vec<String>>,
) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    if let Some((_, slug)) = worktree_key
        .strip_prefix("w:")
        .and_then(|rest| rest.split_once('/'))
    {
        let slug = slug.trim();
        if valid_key(slug) {
            keys.push(slug.to_owned());
        }
    }
    if !worktree_path.is_empty() {
        let claimed = owners.get(worktree_path);
        for owner in claimed.into_iter().flatten() {
            if !keys.contains(owner) {
                keys.push(owner.clone());
            }
        }
    }
    keys
}

/// [`candidates_of`] for one row, in one query over that row's path.
const OWNERS_OF_PATH_SQL: &str = "SELECT key FROM lanes WHERE cwd = ?1
   UNION
   SELECT lane_key FROM lane_owned_worktrees WHERE path = ?1
   ORDER BY 1";

/// The lane keys one lifecycle row can own. Same rule as [`candidates_of`]; the path half is
/// restricted to `worktree_path` so a single-row read does not scan every lane's directory.
pub async fn candidate_keys(
    pool: &SqlitePool,
    worktree_key: &str,
    worktree_path: &str,
) -> Result<Vec<String>> {
    let mut owners: HashMap<String, Vec<String>> = HashMap::new();
    if !worktree_path.is_empty() {
        let found: Vec<String> = sqlx::query_scalar(OWNERS_OF_PATH_SQL)
            .bind(worktree_path)
            .fetch_all(pool)
            .await?;
        if !found.is_empty() {
            owners.insert(worktree_path.to_owned(), found);
        }
    }
    Ok(candidates_of(worktree_key, worktree_path, &owners))
}

/// The PR a lifecycle row shows: the first candidate lane key that has one.
pub async fn for_row(
    pool: &SqlitePool,
    worktree_key: &str,
    worktree_path: &str,
) -> Result<Option<LanePullRequest>> {
    for lane_key in candidate_keys(pool, worktree_key, worktree_path).await? {
        if let Some(pr) = for_lane(pool, &lane_key).await? {
            return Ok(Some(pr));
        }
    }
    Ok(None)
}

/// [`for_row`] for a whole model read, as `worktree_key -> PR`.
///
/// Two queries for every row instead of two per row: the sidebar re-reads the model on a timer, so
/// the batched form is the one that belongs on that path. The rule is still [`candidates_of`], so
/// a batched read and a single-row read cannot disagree.
pub async fn linked_by_row(
    pool: &SqlitePool,
    rows: &[(String, String)],
) -> Result<HashMap<String, LanePullRequest>> {
    let mut linked: HashMap<String, LanePullRequest> = HashMap::new();
    if rows.is_empty() {
        return Ok(linked);
    }
    let by_lane: HashMap<String, LanePullRequest> = all(pool)
        .await?
        .into_iter()
        .map(|pr| (pr.lane_key.clone(), pr))
        .collect();
    if by_lane.is_empty() {
        return Ok(linked);
    }
    let owners = path_owners(pool).await?;
    for (worktree_key, worktree_path) in rows {
        for lane_key in candidates_of(worktree_key, worktree_path, &owners) {
            if let Some(pr) = by_lane.get(&lane_key) {
                linked.insert(worktree_key.clone(), pr.clone());
                break;
            }
        }
    }
    Ok(linked)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lanes::state::insert_lane;

    fn pr(lane_key: &str, state: &str) -> LanePullRequest {
        LanePullRequest {
            lane_key: lane_key.to_string(),
            repo: "acme/widgets".to_string(),
            pr_number: 12,
            state: state.to_string(),
            url: "https://github.com/acme/widgets/pull/12".to_string(),
            title: "Fix the parser".to_string(),
            head_branch: "lane/ledger".to_string(),
            observed_at: "2026-11-15T00:00:00+00:00".to_string(),
        }
    }

    async fn lane(pool: &SqlitePool, key: &str) {
        insert_lane(pool, key, "Ledger", "lead", "/tmp", "file marker")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn a_lane_without_a_link_has_none() {
        let pool = crate::storage::memory().await.unwrap();
        lane(&pool, "ledger").await;
        assert_eq!(for_lane(&pool, "ledger").await.unwrap(), None);
    }

    #[tokio::test]
    async fn a_recorded_pull_request_reads_back() {
        let pool = crate::storage::memory().await.unwrap();
        lane(&pool, "ledger").await;
        record(&pool, &pr("ledger", "open")).await.unwrap();
        let linked = for_lane(&pool, "ledger").await.unwrap().unwrap();
        assert_eq!(linked.pr_number, 12);
        assert_eq!(linked.head_branch, "lane/ledger");
        assert!(!linked.is_merged());
    }

    /// The done-when for T7: a merge performed outside Ferryx flips the link on one record.
    #[tokio::test]
    async fn one_record_flips_a_merged_pull_request() {
        let pool = crate::storage::memory().await.unwrap();
        lane(&pool, "ledger").await;
        record(&pool, &pr("ledger", "open")).await.unwrap();
        record(&pool, &pr("ledger", "merged")).await.unwrap();
        let linked = for_lane(&pool, "ledger").await.unwrap().unwrap();
        assert!(linked.is_merged());
        assert_eq!(all(&pool).await.unwrap().len(), 1, "one row per lane");
    }

    #[tokio::test]
    async fn an_unknown_state_and_a_bad_key_are_refused() {
        let pool = crate::storage::memory().await.unwrap();
        lane(&pool, "ledger").await;
        assert!(record(&pool, &pr("ledger", "draft")).await.is_err());
        assert!(record(&pool, &pr("Ledger!", "open")).await.is_err());
    }

    /// Idempotence, asserted on the row itself: re-recording identical state writes nothing, so
    /// `observed_at` keeps meaning "this state has been true since".
    ///
    /// Fails if the conditional upsert is replaced by an unconditional one: the second record
    /// answers `true`, the third re-stamps `observed_at`, and both assertions below break.
    #[tokio::test]
    async fn re_recording_identical_state_writes_nothing() {
        let pool = crate::storage::memory().await.unwrap();
        lane(&pool, "ledger").await;
        let first = pr("ledger", "open");
        assert!(record(&pool, &first).await.unwrap(), "the first record inserts");
        // A later read that observed the same state: a different observation time, nothing else.
        let mut again = pr("ledger", "open");
        again.observed_at = "2026-11-15T06:00:00+00:00".to_string();
        assert!(!record(&pool, &again).await.unwrap(), "identical state writes nothing");
        let stored = for_lane(&pool, "ledger").await.unwrap().unwrap();
        assert_eq!(stored.observed_at, first.observed_at, "observed_at is unchanged");
        // A change still lands, and stamps the read that saw it.
        let mut merged = pr("ledger", "merged");
        merged.observed_at = "2026-11-15T07:00:00+00:00".to_string();
        assert!(record(&pool, &merged).await.unwrap(), "a new state is written");
        let stored = for_lane(&pool, "ledger").await.unwrap().unwrap();
        assert!(stored.is_merged());
        assert_eq!(stored.observed_at, "2026-11-15T07:00:00+00:00");
    }

    /// The join rule: a `w:` row finds the PR through its slug, a `p:` row through its path, and a
    /// row that claims neither finds nothing.
    ///
    /// Fails if `candidates_of` stops deriving the slug or the path owner: `for_row` and
    /// `linked_by_row` would both answer `None` for rows that do carry a linked PR.
    #[tokio::test]
    async fn a_row_finds_its_lanes_pull_request_by_slug_and_by_path() {
        let pool = crate::storage::memory().await.unwrap();
        insert_lane(&pool, "ledger", "Ledger", "lead", "/code/ledger", "file marker")
            .await
            .unwrap();
        insert_lane(&pool, "other", "Other", "lead", "/code/other", "file marker")
            .await
            .unwrap();
        record(&pool, &pr("ledger", "merged")).await.unwrap();

        // (a) the managed-worktree row: `w:<ws>/<slug>` -> slug `ledger`.
        let by_slug = for_row(&pool, "w:ferryx/ledger", "/somewhere/else")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(by_slug.lane_key, "ledger");
        // (b) a path-keyed row whose path a lane runs in.
        let by_path = for_row(&pool, "p:ferryx/code/ledger", "/code/ledger")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(by_path.lane_key, "ledger");
        // (c) a row naming neither a linked lane nor a claimed path.
        assert_eq!(
            for_row(&pool, "p:ferryx/code/mine", "/code/mine").await.unwrap(),
            None
        );

        // The batched read answers the same as the single-row one.
        let rows = vec![
            ("w:ferryx/ledger".to_string(), "/somewhere/else".to_string()),
            ("p:ferryx/code/mine".to_string(), "/code/mine".to_string()),
        ];
        let linked = linked_by_row(&pool, &rows).await.unwrap();
        assert_eq!(linked.len(), 1);
        assert_eq!(linked["w:ferryx/ledger"].lane_key, "ledger");
    }

    #[test]
    fn a_lane_branch_is_the_documented_convention() {
        assert_eq!(lane_branch("ledger"), "lane/ledger");
    }
}
