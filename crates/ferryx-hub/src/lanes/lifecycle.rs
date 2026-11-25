//! Server-owned sidebar lane lifecycle: pin / settle / snooze / archive per worktree row.
//!
//! ## The lifecycle key (decided here, before the code)
//!
//! The sidebar's rendering unit is a worktree/project row; the pin's unit is a *tab*
//! (`SET_TAB_PINNED`, `ui/src/state/workspaceStore.ts`). Neither is usable alone: tabs are
//! re-created with the pane tree and carry no durable identity, and a bare path changes when a
//! managed worktree is moved on disk. So:
//!
//! **The key is worktree identity, and tab pins are folded into it as a per-worktree pin set.**
//!
//! The mapping step, applied identically on both sides (`worktree_key` here, `laneLifecycleKey`
//! in `ui/src/lib/sidebarPersistence.ts`):
//!
//! | row | key |
//! |---|---|
//! | a managed worktree (`branch = orca/<wsId>/<slug>`, so a slug is known) | `w:<wsId>/<slug>` |
//! | a project root, a plain folder, an unmanaged worktree (no slug) | `p:<workspaceId>/<path>` |
//!
//! `<path>` is normalised to forward slashes with leading, trailing and repeated separators
//! dropped; `wsId` is the slug's owning workspace (the branch's middle segment), not the row's
//! render group. The managed form survives a checkout moving on disk, which is why it is preferred
//! whenever a slug exists.
//!
//! Folding tab pins: a pinned tab has exactly one owning worktree path (the panes' sessions'
//! `worktreePath`), which resolves to exactly one key. `pinned_tabs` therefore stores a JSON
//! array of tab ids under the worktree's row and never mints a key of its own. A row with a
//! non-empty `pinned_tabs` reads as `pinned`: pinning a tab is the owner saying "keep this row
//! where it is", and the row is the only thing a server can order.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::error::{HubError, Result};
use crate::storage::{now_rfc3339, WriteTx};

use super::pull_request::{self, LanePullRequest};

/// The timestamp columns one lifecycle op writes: the new `state` plus the evidence columns for
/// that state. Exactly one carries a value; every other op clears the rest.
type LifecycleStateColumns = (
    &'static str,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// Every state the table accepts, in the order the sidebar renders them.
pub const STATES: [&str; 5] = ["pinned", "active", "snoozed", "settled", "archived"];

/// How long a snooze lasts when the caller names no `until`.
pub const DEFAULT_SNOOZE_SECONDS: i64 = 24 * 60 * 60;

/// The lifecycle key for one sidebar row.
///
/// A managed worktree is keyed by its `orca/<wsId>/<slug>` identity so the row keeps its
/// lifecycle when the checkout moves; everything else is keyed by its workspace and path.
pub fn worktree_key(workspace_id: &str, path: &str, slug: Option<&str>) -> String {
    match slug.map(str::trim).filter(|slug| !slug.is_empty()) {
        Some(slug) => format!("w:{workspace_id}/{slug}"),
        None => format!("p:{workspace_id}/{}", normalize_path(path)),
    }
}

/// Forward slashes, no leading, trailing or repeated separator: `/code/ferryx`, `code/ferryx`,
/// `C:\code\ferryx\` and `/code//ferryx` all key the same row.
///
/// A `p:<workspaceId>/<path>` key already supplies the separator before `<path>`, so a separator
/// kept at either end of the path is not information, it is a duplicated one: an absolute
/// `/code/ferryx` and the same checkout written `code/ferryx` would key `p:ferryx//code/ferryx`
/// and `p:ferryx/code/ferryx`, and one checkout must never become two sidebar rows. Only
/// separators are dropped, never a segment, so `C:/code/ferryx` keeps its drive.
fn normalize_path(path: &str) -> String {
    path.trim()
        .replace('\\', "/")
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

/// A key is accepted only in one of the two documented shapes; anything else is a client bug,
/// and storing it would create a row no later read can match.
///
/// Both shapes are `<prefix>:<workspaceId>/<path>`, and the path part must name a row: no empty
/// segment, so `p:ferryx//code/ferryx` and `p:ferryx/` are refused. That is exactly the shape
/// [`normalize_path`] can never emit, so such a key could never be read back -- it would sit in
/// the table as a row no query matches. The workspace segment itself is allowed to be empty
/// (`p:/code/ferryx`, a row whose workspace is not known yet): it round-trips through
/// [`worktree_key`] unchanged, so it costs nothing to accept.
pub fn valid_key(key: &str) -> bool {
    let Some((prefix, rest)) = key.split_once(':') else {
        return false;
    };
    if !matches!(prefix, "w" | "p") || rest.len() > 512 || rest.contains('\0') {
        return false;
    }
    match rest.split_once('/') {
        Some((_workspace_id, path)) => path.split('/').all(|segment| !segment.is_empty()),
        None => false,
    }
}

/// What the sidebar reads for one row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaneLifecycle {
    pub worktree_key: String,
    pub workspace_id: String,
    pub worktree_path: String,
    /// The stored state; `snoozed` past `snoozed_until` still reads as `snoozed` here.
    pub state: String,
    /// What the sidebar renders: the stored state, with an expired snooze read as `active` and a
    /// non-empty pin set read as `pinned`.
    pub effective_state: String,
    pub pinned_tabs: Vec<String>,
    pub pinned_at: Option<String>,
    pub settled_at: Option<String>,
    /// Why the row settled, when a caller named a reason (migration 0058). `None` for a manual
    /// settle and for every row that is not settled: the reducer clears it with the other state
    /// timestamps, so a row never explains a state it is not in.
    #[serde(default)]
    pub settle_reason: Option<String>,
    pub snoozed_until: Option<String>,
    pub archived_at: Option<String>,
    /// Server-computed order inside the row's workspace section.
    pub position: i64,
    /// Bumped on every write; a client undo compares it and refuses a stale write.
    pub revision: i64,
    pub updated_at: String,
    /// The row's pull request, as the last read-only sync observed it (`lanes/pull_request.rs`).
    ///
    /// `None` covers both "this row has no pull request" and "no sync has looked yet", which read
    /// the same on purpose: neither can be rendered as a PR link. The join is the row's worktree
    /// key to the lane key(s) it can own -- one rule, shared with the settle sweep
    /// (`pull_request::candidates_of`), so a row never shows a PR the settle rule cannot see.
    #[serde(default)]
    pub pull_request: Option<LanePullRequest>,
}

/// One workspace's rows, already in the order the sidebar renders them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaneLifecycleSection {
    pub workspace_id: String,
    pub rows: Vec<LaneLifecycle>,
}

/// The whole server-owned order: sections by workspace, rows by state rank then position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaneLifecycleModel {
    pub sections: Vec<LaneLifecycleSection>,
    /// Sum of the rows' revisions, so a client can tell "unchanged" from "re-read it".
    pub revision: i64,
}

/// A lifecycle write. `Activate` clears whatever state the row was in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifecycleOp {
    Pin { pinned: bool },
    /// `reason` is what the row records for settling (migration 0058): `None` for an owner's own
    /// settle, `Some("inactivity")` / `Some("pr_merged")` for the auto-settle rule.
    Settle { reason: Option<String> },
    Snooze { until: Option<String> },
    Archive,
    Activate,
}

impl LifecycleOp {
    pub fn as_str(&self) -> &'static str {
        match self {
            LifecycleOp::Pin { .. } => "pin",
            LifecycleOp::Settle { .. } => "settle",
            LifecycleOp::Snooze { .. } => "snooze",
            LifecycleOp::Archive => "archive",
            LifecycleOp::Activate => "activate",
        }
    }

    pub fn parse(name: &str, pinned: bool, until: Option<String>) -> Option<Self> {
        match name.trim() {
            "pin" => Some(LifecycleOp::Pin { pinned }),
            // A settle from the HTTP layer carries no reason: the owner did it, nobody guessed.
            "settle" => Some(LifecycleOp::Settle { reason: None }),
            "snooze" => Some(LifecycleOp::Snooze { until }),
            "archive" => Some(LifecycleOp::Archive),
            "activate" => Some(LifecycleOp::Activate),
            _ => None,
        }
    }
}

/// The result of a write. `Stale` is not an error: it is the undo guard, and the caller answers
/// 409 with the row the server actually holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Applied {
    Written(Box<LaneLifecycle>),
    Stale(Box<LaneLifecycle>),
}

const COLUMNS: &str = "worktree_key, workspace_id, worktree_path, state, pinned_at, settled_at, \
                       settle_reason, snoozed_until, archived_at, pinned_tabs, position, revision, \
                       updated_at";

#[derive(sqlx::FromRow)]
struct Row {
    worktree_key: String,
    workspace_id: String,
    worktree_path: String,
    state: String,
    pinned_at: Option<String>,
    settled_at: Option<String>,
    settle_reason: Option<String>,
    snoozed_until: Option<String>,
    archived_at: Option<String>,
    pinned_tabs: String,
    position: i64,
    revision: i64,
    updated_at: String,
}

impl Row {
    /// `pull_request` is the projection joined for this row, resolved by the caller: [`model`] joins
    /// the whole page in two queries, [`one`] and [`apply`] join a single row.
    fn into_lifecycle(self, now: &str, pull_request: Option<LanePullRequest>) -> LaneLifecycle {
        let pinned_tabs: Vec<String> = serde_json::from_str(&self.pinned_tabs).unwrap_or_default();
        let effective_state = effective_state(&self.state, self.snoozed_until.as_deref(), now, !pinned_tabs.is_empty());
        LaneLifecycle {
            worktree_key: self.worktree_key,
            workspace_id: self.workspace_id,
            worktree_path: self.worktree_path,
            state: self.state,
            effective_state: effective_state.to_owned(),
            pinned_tabs,
            pinned_at: self.pinned_at,
            settled_at: self.settled_at,
            settle_reason: self.settle_reason,
            snoozed_until: self.snoozed_until,
            archived_at: self.archived_at,
            position: self.position,
            revision: self.revision,
            updated_at: self.updated_at,
            pull_request,
        }
    }
}

/// The rendered state. An expiry is computed on read, so no background job is needed to wake a
/// snoozed row and a restart cannot leave one stuck.
pub fn effective_state(state: &str, snoozed_until: Option<&str>, now: &str, pinned: bool) -> &'static str {
    if pinned && state != "archived" {
        return "pinned";
    }
    match state {
        "snoozed" => match snoozed_until.and_then(parse_rfc3339) {
            Some(until) if until > parse_rfc3339(now).unwrap_or(until) => "snoozed",
            _ => "active",
        },
        "pinned" => "pinned",
        "settled" => "settled",
        "archived" => "archived",
        _ => "active",
    }
}

fn parse_rfc3339(text: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|value| value.with_timezone(&chrono::Utc))
}

/// The read path: every live row, sections by workspace, rows by state rank then position.
/// Archived rows are left out of the live list; they are reachable one by one.
///
/// Each row carries its recorded pull request (`LaneLifecycle::pull_request`), joined for the whole
/// page in two queries rather than two per row: this is the read a sidebar re-fetches on a timer.
pub async fn model(pool: &SqlitePool, now: &str) -> Result<LaneLifecycleModel> {
    let rows: Vec<Row> = sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM lane_lifecycles
          WHERE state <> 'archived'
          ORDER BY workspace_id ASC, position ASC, worktree_key ASC"
    ))
    .fetch_all(pool)
    .await?;
    let keys: Vec<(String, String)> = rows
        .iter()
        .map(|row| (row.worktree_key.clone(), row.worktree_path.clone()))
        .collect();
    let linked = pull_request::linked_by_row(pool, &keys).await?;
    let mut sections: Vec<LaneLifecycleSection> = Vec::new();
    let mut total = 0i64;
    for row in rows {
        let pr = linked.get(&row.worktree_key).cloned();
        let lifecycle = row.into_lifecycle(now, pr);
        total += lifecycle.revision;
        match sections.last_mut() {
            Some(section) if section.workspace_id == lifecycle.workspace_id => {
                section.rows.push(lifecycle);
            }
            _ => sections.push(LaneLifecycleSection {
                workspace_id: lifecycle.workspace_id.clone(),
                rows: vec![lifecycle],
            }),
        }
    }
    for section in &mut sections {
        section.rows.sort_by_key(|row| match row.effective_state.as_str() {
            "pinned" => 0,
            "active" | "snoozed" => 1,
            "settled" => 2,
            _ => 3,
        });
    }
    Ok(LaneLifecycleModel { sections, revision: total })
}

/// The model grouped as `workspaceId -> ordered worktree keys`, which is the exact input the
/// sidebar builder orders its rows with.
pub async fn order_by_workspace(pool: &SqlitePool, now: &str) -> Result<BTreeMap<String, Vec<String>>> {
    let model = model(pool, now).await?;
    Ok(model
        .sections
        .into_iter()
        .map(|section| {
            (
                section.workspace_id,
                section.rows.into_iter().map(|row| row.worktree_key).collect(),
            )
        })
        .collect())
}

pub async fn one(pool: &SqlitePool, key: &str, now: &str) -> Result<Option<LaneLifecycle>> {
    let row: Option<Row> = sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM lane_lifecycles WHERE worktree_key = ?"
    ))
    .bind(key)
    .fetch_optional(pool)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let linked = pull_request::for_row(pool, &row.worktree_key, &row.worktree_path).await?;
    Ok(Some(row.into_lifecycle(now, linked)))
}

/// Applies one lifecycle write to `key`.
///
/// `expected_revision` is the client's undo guard: a write carrying a revision the server has
/// already moved past is refused with [`Applied::Stale`] and changes nothing, which is what makes
/// a 5 s undo safe when two windows race. `None` skips the guard (first write of a row).
///
/// The row is created on first write, so a lifecycle survives an app restart even for a row the
/// server has never seen: the key is the worktree's identity, not an id minted at publish time.
pub async fn apply(
    pool: &SqlitePool,
    key: &str,
    workspace_id: &str,
    path: &str,
    op: &LifecycleOp,
    expected_revision: Option<i64>,
    now: &str,
) -> Result<Applied> {
    if matches!(op, LifecycleOp::Snooze { until: None }) && parse_rfc3339(now).is_none() {
        return Err(HubError::message("now must be an RFC 3339 timestamp to calculate the default snooze"));
    }
    if !valid_key(key) {
        return Err(HubError::message(format!("not a lifecycle key: {key}")));
    }
    let mut tx = pool.begin_write().await?;
    converge_managed_row(&mut tx, key, workspace_id, path).await?;
    let current: Option<(i64, i64)> = sqlx::query_as(
        "SELECT revision, position FROM lane_lifecycles WHERE worktree_key = ?",
    )
    .bind(key)
    .fetch_optional(&mut *tx)
    .await?;
    if let (Some(expected), Some((revision, _))) = (expected_revision, current) {
        if revision != expected {
            let row: Option<Row> = sqlx::query_as(&format!(
                "SELECT {COLUMNS} FROM lane_lifecycles WHERE worktree_key = ?"
            ))
            .bind(key)
            .fetch_optional(&mut *tx)
            .await?;
            tx.commit().await?;
            return Ok(match row {
                Some(row) => {
                    let linked = pull_request::for_row(pool, &row.worktree_key, &row.worktree_path).await?;
                    Applied::Stale(Box::new(row.into_lifecycle(now, linked)))
                }
                None => Applied::Stale(Box::new(empty_row(key, workspace_id, path, now))),
            });
        }
    }
    let position = match current {
        Some((_, position)) => position,
        // New rows land after everything the workspace already has, keeping import order.
        None => {
            sqlx::query_scalar::<_, Option<i64>>(
                "SELECT MAX(position) FROM lane_lifecycles WHERE workspace_id = ?",
            )
            .bind(workspace_id)
            .fetch_one(&mut *tx)
            .await?
            .unwrap_or(-1)
                + 1
        }
    };
    // Exactly one timestamp column carries the new state; the others are cleared, so a row
    // never claims two states at once. `settle_reason` is one of those state evidence columns:
    // only `Settle` sets it, every other op clears it.
    let (
        state,
        pinned_at,
        settled_at,
        settle_reason,
        snoozed_until,
        archived_at,
    ): LifecycleStateColumns = match op {
        LifecycleOp::Pin { pinned } => (
            if *pinned { "pinned" } else { "active" },
            pinned.then(|| now.to_owned()),
            None,
            None,
            None,
            None,
        ),
        LifecycleOp::Settle { reason } => (
            "settled",
            None,
            Some(now.to_owned()),
            reason.clone(),
            None,
            None,
        ),
        LifecycleOp::Snooze { until } => (
            "snoozed",
            None,
            None,
            None,
            Some(match until {
                Some(until) => until.clone(),
                None => (parse_rfc3339(now).ok_or_else(|| {
                    HubError::message(
                        "now must be an RFC 3339 timestamp to calculate the default snooze",
                    )
                })? + chrono::Duration::seconds(DEFAULT_SNOOZE_SECONDS))
                    .to_rfc3339(),
            }),
            None,
        ),
        LifecycleOp::Archive => ("archived", None, None, None, None, Some(now.to_owned())),
        LifecycleOp::Activate => ("active", None, None, None, None, None),
    };
    sqlx::query(
        "INSERT INTO lane_lifecycles(worktree_key, workspace_id, worktree_path, state, pinned_at,
             settled_at, settle_reason, snoozed_until, archived_at, pinned_tabs, position,
             revision, updated_at, created_at)
         VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?, '[]', ?, 1, ?, ?)
         ON CONFLICT(worktree_key) DO UPDATE SET
             state = excluded.state,
             worktree_path = excluded.worktree_path,
             pinned_at = excluded.pinned_at,
             settled_at = excluded.settled_at,
             settle_reason = excluded.settle_reason,
             snoozed_until = excluded.snoozed_until,
             archived_at = excluded.archived_at,
             revision = lane_lifecycles.revision + 1,
             updated_at = excluded.updated_at",
    )
    .bind(key)
    .bind(workspace_id)
    .bind(path)
    .bind(state)
    .bind(&pinned_at)
    .bind(&settled_at)
    .bind(&settle_reason)
    .bind(&snoozed_until)
    .bind(&archived_at)
    .bind(position)
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    let row: Row = sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM lane_lifecycles WHERE worktree_key = ?"
    ))
    .bind(key)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    // The answer is the row a client replaces its own copy with, so it carries the PR link too;
    // the join runs after the commit, outside the write lock.
    let linked = pull_request::for_row(pool, &row.worktree_key, &row.worktree_path).await?;
    Ok(Applied::Written(Box::new(row.into_lifecycle(now, linked))))
}

fn empty_row(key: &str, workspace_id: &str, path: &str, now: &str) -> LaneLifecycle {
    LaneLifecycle {
        worktree_key: key.to_owned(),
        workspace_id: workspace_id.to_owned(),
        worktree_path: path.to_owned(),
        state: "active".into(),
        effective_state: "active".into(),
        pinned_tabs: Vec::new(),
        pinned_at: None,
        settled_at: None,
        settle_reason: None,
        snoozed_until: None,
        archived_at: None,
        position: 0,
        revision: 0,
        updated_at: now.to_owned(),
        // A row the store does not hold has nothing recorded against it, by definition.
        pull_request: None,
    }
}

/// Replaces the per-worktree pin set folded from `SET_TAB_PINNED`. The row reads as `pinned`
/// while the set is non-empty, so pinning a tab needs no separate state write.
pub async fn set_pinned_tabs(
    pool: &SqlitePool,
    key: &str,
    workspace_id: &str,
    path: &str,
    pinned_tabs: &[String],
    now: &str,
) -> Result<LaneLifecycle> {
    if !valid_key(key) {
        return Err(HubError::message(format!("not a lifecycle key: {key}")));
    }
    let encoded = serde_json::to_string(pinned_tabs)
        .map_err(|error| HubError::message(error.to_string()))?;
    let mut tx = pool.begin_write().await?;
    converge_managed_row(&mut tx, key, workspace_id, path).await?;
    let position = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT MAX(position) FROM lane_lifecycles WHERE workspace_id = ?",
    )
    .bind(workspace_id)
    .fetch_one(&mut *tx)
    .await?
    .unwrap_or(-1)
        + 1;
    sqlx::query(
        "INSERT INTO lane_lifecycles(worktree_key, workspace_id, worktree_path, state, pinned_tabs,
             position, revision, updated_at, created_at)
         VALUES(?, ?, ?, 'active', ?, ?, 1, ?, ?)
         ON CONFLICT(worktree_key) DO UPDATE SET
             pinned_tabs = excluded.pinned_tabs,
             worktree_path = excluded.worktree_path,
             revision = lane_lifecycles.revision + 1,
             updated_at = excluded.updated_at",
    )
    .bind(key)
    .bind(workspace_id)
    .bind(path)
    .bind(&encoded)
    .bind(position)
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    let row: Row = sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM lane_lifecycles WHERE worktree_key = ?"
    ))
    .bind(key)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    let linked = pull_request::for_row(pool, &row.worktree_key, &row.worktree_path).await?;
    Ok(row.into_lifecycle(now, linked))
}

/// Promote a provisional path identity when the caller supplies the managed identity. Renaming
/// the row preserves state, pins, order, timestamps and revision instead of inserting a sibling.
async fn converge_managed_row(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    key: &str,
    workspace_id: &str,
    path: &str,
) -> Result<()> {
    if !key.starts_with("w:") || path.is_empty() {
        return Ok(());
    }
    let provisional = worktree_key(workspace_id, path, None);
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT worktree_key FROM lane_lifecycles WHERE worktree_key = ?",
    )
    .bind(key)
    .fetch_optional(&mut **tx)
    .await?;
    if existing.is_some() {
        // An old import could already have created the alias. The managed row owns state;
        // preserve the imported order before removing that obsolete alias.
        sqlx::query(
            "UPDATE lane_lifecycles SET position = COALESCE(
                 (SELECT position FROM lane_lifecycles WHERE worktree_key = ?), position)
             WHERE worktree_key = ?",
        )
        .bind(&provisional)
        .bind(key)
        .execute(&mut **tx)
        .await?;
        sqlx::query("DELETE FROM lane_lifecycles WHERE worktree_key = ?")
            .bind(&provisional)
            .execute(&mut **tx)
            .await?;
    } else {
        sqlx::query("UPDATE lane_lifecycles SET worktree_key = ?, worktree_path = ? WHERE worktree_key = ?")
            .bind(key)
            .bind(path)
            .bind(&provisional)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

/// Resolve and store import/reorder identically. The key's normalised path is identity only;
/// `worktree_path` retains the actual directory used by activity and PR ownership joins.
async fn store_order_row(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    workspace_id: &str,
    identifier: &str,
    position: i64,
    now: &str,
) -> Result<()> {
    let Some(mut key) = key_for_identifier(workspace_id, identifier) else {
        return Err(HubError::message(format!("identifier names no row: {identifier}")));
    };
    let mut path = if let Some(rest) = key.strip_prefix("p:") {
        let encoded = rest.split_once('/').map(|(_, path)| path).unwrap_or_default();
        if valid_key(identifier.trim()) {
            if is_drive_path(encoded) { encoded.to_owned() } else { format!("/{encoded}") }
        } else {
            identifier_path(identifier.trim()).to_owned()
        }
    } else {
        String::new()
    };
    // Existing managed rows beat path aliases, even when the import sends an explicit p: key.
    let stored: Vec<(String, String)> = sqlx::query_as(
        "SELECT worktree_key, worktree_path FROM lane_lifecycles WHERE workspace_id = ?",
    )
    .bind(workspace_id)
    .fetch_all(&mut **tx)
    .await?;
    let matched = stored.iter().find(|(stored_key, stored_path)| {
        !path.is_empty() && stored_key.starts_with("w:")
            && normalize_path(stored_path) == normalize_path(&path)
    }).or_else(|| stored.iter().find(|(stored_key, _)| stored_key == &key));
    if let Some((managed, directory)) = matched {
        key = managed.clone();
        if !directory.is_empty() { path = directory.clone(); }
    }
    converge_managed_row(tx, &key, workspace_id, &path).await?;
    sqlx::query(
        "INSERT INTO lane_lifecycles(worktree_key, workspace_id, worktree_path, state, pinned_tabs,
             position, revision, updated_at, created_at)
         VALUES(?, ?, ?, 'active', '[]', ?, 1, ?, ?)
         ON CONFLICT(worktree_key) DO UPDATE SET
             position = excluded.position,
             worktree_path = CASE WHEN excluded.worktree_path <> '' THEN excluded.worktree_path
                                 ELSE lane_lifecycles.worktree_path END,
             revision = lane_lifecycles.revision + 1,
             updated_at = excluded.updated_at",
    )
    .bind(&key)
    .bind(workspace_id)
    .bind(&path)
    .bind(position)
    .bind(now)
    .bind(now)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// A listed row's authoritative identity, supplied by the same worktree listing the UI renders.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListedIdentity {
    pub worktree_key: String,
    pub workspace_id: String,
    pub path: String,
}

/// Resolve provisional imports before rendering, including when listing arrives after import.
/// This only renames existing rows: listing a checkout must not create a lifecycle or wake it.
pub async fn reconcile_identities(pool: &SqlitePool, rows: &[ListedIdentity]) -> Result<()> {
    let mut tx = pool.begin_write().await?;
    for row in rows {
        if !valid_key(&row.worktree_key) {
            return Err(HubError::message(format!("not a lifecycle key: {}", row.worktree_key)));
        }
        converge_managed_row(&mut tx, &row.worktree_key, &row.workspace_id, &row.path).await?;
        sqlx::query("UPDATE lane_lifecycles SET worktree_path = ? WHERE worktree_key = ?")
            .bind(&row.path)
            .bind(&row.worktree_key)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// The server-owned section order for one workspace: `keys` become positions 0..n. Keys the
/// server does not hold yet are created in that order, which is how the one-time import of the
/// desktop's `localStorage` order lands.
pub async fn reorder(pool: &SqlitePool, workspace_id: &str, keys: &[String], now: &str) -> Result<()> {
    let mut tx = pool.begin_write().await?;
    for (index, key) in keys.iter().enumerate() {
        if !valid_key(key) {
            return Err(HubError::message(format!("not a lifecycle key: {key}")));
        }
        store_order_row(&mut tx, workspace_id, key, index as i64, now).await?;
    }
    tx.commit().await?;
    Ok(())
}

/// The path an order identifier names, when the identifier is not already a lifecycle key.
///
/// Both the drag handler and the pre-server `localStorage` order speak one vocabulary, and only
/// one of its shapes is a qualifier plus a path: `sidebar-worktree:<group>:<rowWorkspace>:<path>`
/// sortable ids and `<workspace>:<path>` member keys. Their qualifier ends at the first `:/`.
///
/// A Windows drive path is the exception that must not be split: `C:/code/ferryx` contains `:/` at
/// index 1, and one-letter-before-`:/` is a drive, not a qualifier. Splitting it there would drop
/// the `C:` and key `p:<ws>/code/ferryx` while [`worktree_key`] stores `p:<ws>/C:/code/ferryx` --
/// the two-writings-one-checkout defect again, one layer up.
pub fn identifier_path(identifier: &str) -> &str {
    if is_drive_path(identifier) {
        return identifier;
    }
    match identifier.find(":/") {
        Some(at) => &identifier[at + 1..],
        None => identifier,
    }
}

/// A single ASCII letter, a colon, then a separator: `C:/x` and `c:\x`. Nothing else is a drive.
fn is_drive_path(identifier: &str) -> bool {
    let bytes = identifier.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'/' || bytes[2] == b'\\')
}

/// The lifecycle key one order identifier names, derived without reading the database.
///
/// This is the single place the identifier vocabulary becomes a key, shared by the drag path
/// (`resolve_identifier`) and the one-time import, so the two cannot mint two keys for one row.
/// A caller that already sends keys is accepted unchanged; everything else is normalised through
/// [`worktree_key`], which is the only place a `p:` key is minted. `None` means the identifier
/// names no row at all (empty, or a path with no segment), which the callers drop rather than
/// store as a key no read could match.
pub fn key_for_identifier(workspace_id: &str, identifier: &str) -> Option<String> {
    let identifier = identifier.trim();
    if identifier.is_empty() {
        return None;
    }
    if valid_key(identifier) {
        return Some(identifier.to_owned());
    }
    let derived = worktree_key(workspace_id, identifier_path(identifier), None);
    valid_key(&derived).then_some(derived)
}

/// The `hub_meta` key recording that the desktop's `localStorage` order was already imported.
pub const IMPORT_MARKER: &str = "lane_lifecycles.imported";

/// True once the one-time import happened; from then on the server order is the only order.
pub async fn already_imported(pool: &SqlitePool) -> Result<bool> {
    let marked: Option<String> = sqlx::query_scalar("SELECT value FROM hub_meta WHERE key = ?")
        .bind(IMPORT_MARKER)
        .fetch_optional(pool)
        .await?;
    Ok(marked.is_some())
}

/// The ONE-TIME import of the desktop's `localStorage` `WorktreeOrder`.
///
/// The payload is what `localStorage` actually holds, i.e. the identifiers the drag handler had --
/// `sidebar-worktree:...` sortable ids, `<workspace>:<path>` member keys, or bare paths -- not
/// lifecycle keys. Each is resolved with [`key_for_identifier`], the same step the drag path uses,
/// so an imported row and a dragged row can never disagree about which key a path names. A client
/// that sends keys instead is accepted unchanged.
///
/// `IMPORT_MARKER` in `hub_meta` is the guard: the second call returns `false` and writes nothing,
/// so a stale localStorage copy can never overwrite a server order the owner has changed since.
/// The order and the marker are written in one transaction, so a crash mid-import leaves no
/// marker and the import simply runs again.
pub async fn import_local_order(
    pool: &SqlitePool,
    order: &BTreeMap<String, Vec<String>>,
    now: &str,
) -> Result<bool> {
    if already_imported(pool).await? {
        return Ok(false);
    }
    let mut tx = pool.begin_write().await?;
    for (workspace_id, identifiers) in order {
        for (index, identifier) in identifiers.iter().enumerate() {
            store_order_row(&mut tx, workspace_id, identifier, index as i64, now).await?;
        }
    }
    sqlx::query(
        "INSERT INTO hub_meta(key, value) VALUES(?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(IMPORT_MARKER)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(true)
}

pub fn now() -> String {
    now_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn row(pool: &SqlitePool, workspace_id: &str, key: &str, position: i64) {
        sqlx::query(
            "INSERT INTO lane_lifecycles(worktree_key, workspace_id, worktree_path, state, position,
                 revision, updated_at, created_at)
             VALUES(?, ?, '', 'active', ?, 1, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        )
        .bind(key)
        .bind(workspace_id)
        .bind(position)
        .execute(pool)
        .await
        .unwrap();
    }

    #[test]
    fn a_managed_worktree_is_keyed_by_identity_and_anything_else_by_path() {
        assert_eq!(worktree_key("ferryx", "/code/ferryx/x", Some("sidebar-polish")), "w:ferryx/sidebar-polish");
        // The checkout may move; the key does not.
        assert_eq!(worktree_key("ferryx", "/elsewhere/x", Some("sidebar-polish")), "w:ferryx/sidebar-polish");
        assert_eq!(worktree_key("ferryx", "/code/ferryx/", None), "p:ferryx/code/ferryx");
        assert_eq!(worktree_key("ferryx", "C:\\code\\ferryx\\", None), "p:ferryx/C:/code/ferryx");
        assert_eq!(worktree_key("ferryx", "/code/ferryx", Some("  ")), "p:ferryx/code/ferryx");
        // One checkout written three ways is ONE row: the separator before the path is supplied by
        // the key itself, so a leading, trailing or doubled one is a duplicate, not a difference.
        let absolute = worktree_key("ferryx", "/code/ferryx", None);
        assert_eq!(worktree_key("ferryx", "code/ferryx", None), absolute);
        assert_eq!(worktree_key("ferryx", "/code//ferryx", None), absolute);
        assert_eq!(worktree_key("ferryx", "/code/ferryx/", None), absolute);
        assert_eq!(absolute, "p:ferryx/code/ferryx");
    }

    #[test]
    fn only_the_two_documented_key_shapes_are_accepted() {
        assert!(valid_key("w:ferryx/sidebar-polish"));
        assert!(valid_key("p:ferryx/code/ferryx"));
        assert!(!valid_key("ferryx"));
        assert!(!valid_key("x:ferryx/a"));
        assert!(!valid_key("p:"));
        // An empty segment names no row: `normalize_path` never emits `//`, so storing one would
        // create a row no read ever asks for.
        assert!(!valid_key("p:ferryx//code/ferryx"));
        assert!(!valid_key("p:ferryx/"));
        assert!(!valid_key("w:ferryx/"));
        // An unknown workspace round-trips through `worktree_key` unchanged, so it stays legal.
        assert!(valid_key("p:/code/ferryx"));
    }

    /// The one place an order identifier becomes a key. Both the drag path and the one-time
    /// import call this, so one row cannot acquire two keys depending on which wrote it.
    #[test]
    fn an_identifier_becomes_the_same_key_however_it_is_written() {
        // A key is already a key.
        assert_eq!(
            key_for_identifier("ferryx", "p:ferryx/code/ferryx").as_deref(),
            Some("p:ferryx/code/ferryx")
        );
        // A sortable id and a bare path (the two shapes localStorage holds) name that same row.
        assert_eq!(
            key_for_identifier("ferryx", "sidebar-worktree:ferryx:ferryx:/code/ferryx").as_deref(),
            Some("p:ferryx/code/ferryx")
        );
        assert_eq!(
            key_for_identifier("ferryx", "/code/ferryx").as_deref(),
            Some("p:ferryx/code/ferryx")
        );
        // The shorter sortable form and a `<workspace>:<path>` member key resolve the same way.
        assert_eq!(
            key_for_identifier("ferryx", "sidebar-worktree:ferryx:/code/ferryx").as_deref(),
            Some("p:ferryx/code/ferryx")
        );
        assert_eq!(
            key_for_identifier("ferryx", "ferryx:/code/ferryx").as_deref(),
            Some("p:ferryx/code/ferryx")
        );
        // A Windows drive path is not a qualifier: the `C:` must survive, or the same checkout
        // keys `p:ferryx/code/ferryx` here and `p:ferryx/C:/code/ferryx` from `worktree_key`.
        assert_eq!(
            key_for_identifier("ferryx", "C:/code/ferryx").as_deref(),
            Some("p:ferryx/C:/code/ferryx")
        );
        assert_eq!(
            key_for_identifier("ferryx", "C:\\code\\ferryx\\").as_deref(),
            Some("p:ferryx/C:/code/ferryx")
        );
        // Nothing to name: dropped, never stored as a key no read could match.
        assert_eq!(key_for_identifier("ferryx", ""), None);
        assert_eq!(key_for_identifier("ferryx", "sidebar-worktree:ferryx:ferryx:/"), None);
    }

    #[test]
    fn an_expired_snooze_reads_as_active_without_a_write() {
        let now = "2026-01-02T00:00:00Z";
        assert_eq!(effective_state("snoozed", Some("2026-01-01T23:59:59Z"), now, false), "active");
        assert_eq!(effective_state("snoozed", Some("2026-01-03T00:00:00Z"), now, false), "snoozed");
        assert_eq!(effective_state("snoozed", None, now, false), "active");
    }

    #[test]
    fn a_non_empty_pin_set_reads_as_pinned() {
        assert_eq!(effective_state("active", None, "2026-01-02T00:00:00Z", true), "pinned");
        assert_eq!(effective_state("settled", None, "2026-01-02T00:00:00Z", true), "pinned");
        // Archiving a row outranks its pin set: an archived row stays out of the live list.
        assert_eq!(effective_state("archived", None, "2026-01-02T00:00:00Z", true), "archived");
    }

    #[tokio::test]
    async fn pins_sort_first_settled_after_and_archived_is_not_listed() {
        let pool = crate::storage::memory().await.unwrap();
        let now = "2026-01-02T00:00:00Z";
        row(&pool, "ferryx", "p:ferryx/a", 0).await;
        row(&pool, "ferryx", "p:ferryx/b", 1).await;
        row(&pool, "ferryx", "p:ferryx/c", 2).await;
        apply(&pool, "p:ferryx/c", "ferryx", "/a", &LifecycleOp::Pin { pinned: true }, None, now)
            .await
            .unwrap();
        apply(
            &pool,
            "p:ferryx/a",
            "ferryx",
            "/b",
            &LifecycleOp::Settle { reason: None },
            None,
            now,
        )
        .await
        .unwrap();
        apply(&pool, "p:ferryx/b", "ferryx", "/c", &LifecycleOp::Archive, None, now)
            .await
            .unwrap();
        let model = model(&pool, now).await.unwrap();
        let keys: Vec<&str> = model.sections[0].rows.iter().map(|row| row.worktree_key.as_str()).collect();
        assert_eq!(keys, ["p:ferryx/c", "p:ferryx/a"]);
        assert_eq!(model.sections[0].rows[0].effective_state, "pinned");
        assert_eq!(model.sections[0].rows[1].effective_state, "settled");
    }

    #[tokio::test]
    async fn tab_pins_sort_before_rows_by_stored_state() {
        let pool = crate::storage::memory().await.unwrap();
        let now = "2026-01-02T00:00:00Z";
        row(&pool, "ferryx", "p:ferryx/snoozed", 0).await;
        row(&pool, "ferryx", "p:ferryx/active", 1).await;
        apply(&pool, "p:ferryx/snoozed", "ferryx", "/snoozed", &LifecycleOp::Snooze { until: Some("2026-01-03T00:00:00Z".into()) }, None, now).await.unwrap();
        set_pinned_tabs(&pool, "p:ferryx/active", "ferryx", "/active", &["tab".into()], now).await.unwrap();
        let model = model(&pool, now).await.unwrap();
        let keys: Vec<&str> = model.sections[0].rows.iter().map(|item| item.worktree_key.as_str()).collect();
        assert_eq!(keys, ["p:ferryx/active", "p:ferryx/snoozed"]);
        assert_eq!(model.sections[0].rows[0].effective_state, "pinned");
    }

    #[tokio::test]
    async fn default_snooze_is_relative_to_the_supplied_timestamp() {
        let pool = crate::storage::memory().await.unwrap();
        let now = "2020-01-01T00:00:00Z";
        let result = apply(&pool, "p:ferryx/snoozed", "ferryx", "/snoozed", &LifecycleOp::Snooze { until: None }, None, now).await.unwrap();
        let Applied::Written(row) = result else { panic!("first write cannot be stale"); };
        assert_eq!(row.snoozed_until.as_deref(), Some("2020-01-02T00:00:00+00:00"));
    }

    #[tokio::test]
    async fn a_lifecycle_survives_reopening_the_database() {
        let dir = std::env::temp_dir().join(format!("ferryx-lane-lifecycle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("hub.sqlite");
        let now = "2026-01-02T00:00:00Z";
        {
            let pool = crate::storage::connect(&path).await.unwrap();
            apply(&pool, "w:ferryx/polish", "ferryx", "/code/ferryx/polish", &LifecycleOp::Pin { pinned: true }, None, now)
                .await
                .unwrap();
            crate::storage::close_fully(&pool).await;
        }
        let pool = crate::storage::connect(&path).await.unwrap();
        let row = one(&pool, "w:ferryx/polish", now).await.unwrap().unwrap();
        assert_eq!(row.effective_state, "pinned");
        assert_eq!(row.worktree_path, "/code/ferryx/polish");
        crate::storage::close_fully(&pool).await;
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_stale_revision_is_refused_and_changes_nothing() {
        let pool = crate::storage::memory().await.unwrap();
        let now = "2026-01-02T00:00:00Z";
        let first = match apply(
            &pool,
            "p:ferryx/a",
            "ferryx",
            "/a",
            &LifecycleOp::Settle { reason: None },
            None,
            now,
        )
        .await
        .unwrap()
        {
            Applied::Written(row) => row,
            Applied::Stale(_) => panic!("the first write cannot be stale"),
        };
        assert_eq!(first.revision, 1);
        let stale = apply(
            &pool,
            "p:ferryx/a",
            "ferryx",
            "/a",
            &LifecycleOp::Archive,
            Some(first.revision - 1),
            now,
        )
        .await
        .unwrap();
        match stale {
            Applied::Stale(row) => assert_eq!(row.state, "settled"),
            Applied::Written(_) => panic!("a stale revision must not write"),
        }
        assert_eq!(one(&pool, "p:ferryx/a", now).await.unwrap().unwrap().state, "settled");
    }

    #[tokio::test]
    async fn the_import_of_the_local_order_runs_once() {
        let pool = crate::storage::memory().await.unwrap();
        let mut order = BTreeMap::new();
        order.insert("ferryx".to_owned(), vec!["p:ferryx/b".to_owned(), "p:ferryx/a".to_owned()]);
        assert!(import_local_order(&pool, &order, "2026-01-02T00:00:00Z").await.unwrap());
        assert_eq!(order_by_workspace(&pool, "2026-01-02T00:00:00Z").await.unwrap()["ferryx"], ["p:ferryx/b", "p:ferryx/a"]);
        // A second import is refused and leaves the order the owner now has alone.
        reorder(&pool, "ferryx", &["p:ferryx/a".into(), "p:ferryx/b".into()], "2026-01-02T00:00:00Z")
            .await
            .unwrap();
        assert!(!import_local_order(&pool, &order, "2026-01-03T00:00:00Z").await.unwrap());
        assert_eq!(order_by_workspace(&pool, "2026-01-03T00:00:00Z").await.unwrap()["ferryx"], ["p:ferryx/a", "p:ferryx/b"]);
    }

    #[tokio::test]
    async fn reorder_lands_positions_the_read_returns() {
        let pool = crate::storage::memory().await.unwrap();
        let now = "2026-01-02T00:00:00Z";
        reorder(&pool, "ferryx", &["p:ferryx/b".into(), "p:ferryx/a".into()], now)
            .await
            .unwrap();
        let order = order_by_workspace(&pool, now).await.unwrap();
        assert_eq!(order["ferryx"], ["p:ferryx/b", "p:ferryx/a"]);
    }

    /// Covers the opposite arrival order: an import must reuse a managed row already stored.
    /// Removing the stored-identity lookup leaves both w: and p: rows after the import.
    #[tokio::test]
    async fn importing_a_path_reuses_its_existing_managed_row() {
        let pool = crate::storage::memory().await.expect("memory database");
        let now = "2026-01-02T00:00:00Z";
        set_pinned_tabs(&pool, "w:ferryx/managed", "ferryx", "/code/managed", &["tab".into()], now)
            .await.expect("pin tab");
        let order = BTreeMap::from([("ferryx".to_owned(), vec!["p:ferryx/code/managed".to_owned()])]);
        import_local_order(&pool, &order, now).await.expect("import");
        let shown = model(&pool, now).await.expect("model");
        assert_eq!(shown.sections[0].rows.len(), 1);
        let row = &shown.sections[0].rows[0];
        assert_eq!(row.worktree_key, "w:ferryx/managed");
        assert_eq!(row.worktree_path, "/code/managed");
        assert_eq!(row.pinned_tabs, ["tab"]);
        assert_eq!(row.effective_state, "pinned");
    }

    /// Without path hydration, first-drag plain folders have no activity/PR ownership join.
    #[tokio::test]
    async fn reordered_plain_folders_keep_unix_and_windows_directories() {
        let pool = crate::storage::memory().await.expect("memory database");
        let now = "2026-01-02T00:00:00Z";
        reorder(&pool, "ferryx", &["p:ferryx/code/plain".into(), "p:ferryx/C:/code/plain".into()], now)
            .await.expect("reorder");
        assert_eq!(one(&pool, "p:ferryx/code/plain", now).await.expect("read").expect("row").worktree_path, "/code/plain");
        assert_eq!(one(&pool, "p:ferryx/C:/code/plain", now).await.expect("read").expect("row").worktree_path, "C:/code/plain");
    }

    #[tokio::test]
    async fn a_pin_set_is_folded_into_the_row_it_belongs_to() {
        let pool = crate::storage::memory().await.unwrap();
        let now = "2026-01-02T00:00:00Z";
        let key = worktree_key("ferryx", "/code/ferryx/polish", Some("polish"));
        let row = set_pinned_tabs(&pool, &key, "ferryx", "/code/ferryx/polish", &["tab-1".into()], now)
            .await
            .unwrap();
        assert_eq!(row.pinned_tabs, ["tab-1"]);
        assert_eq!(row.effective_state, "pinned");
        // Unpinning the last tab drops the row back to its stored state.
        let row = set_pinned_tabs(&pool, &key, "ferryx", "/code/ferryx/polish", &[], now)
            .await
            .unwrap();
        assert_eq!(row.effective_state, "active");
        assert_eq!(row.revision, 2);
    }
}
