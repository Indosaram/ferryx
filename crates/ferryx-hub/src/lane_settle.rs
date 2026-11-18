//! T6: a lane row settles itself. Two triggers, one recorded reason, and an explicit pin that
//! outranks both.
//!
//! ## The clock is a parameter, never a read
//!
//! Every decision below is a pure function of `now`, the RFC 3339 string the caller passes in.
//! This module contains no `SystemTime::now()`, no `Utc::now()`, no `lifecycle::now()` (that
//! helper reads the real clock and exists for the HTTP layer only, `lanes/lifecycle.rs:589`), no
//! `sleep`, no timer and no polling loop. A test says "now is day 8" by handing [`sweep`] the
//! day-8 string, so the rule is asserted exactly instead of waited for. This is the shape
//! `lifecycle::apply` and `lifecycle::one` already use: time is a `&str` argument, not an ambient
//! clock.
//!
//! ## Triggers, in the order they are checked
//!
//! 1. **Pull request merged**: T7's projection (`lanes/pull_request.rs`) says the row's PR merged.
//! 2. **Inactivity**: no lifecycle write or owning lane activity (including recorded agent turns)
//!    for [`INACTIVITY_WINDOW_SECONDS`] (7 days).
//!
//! The merge is checked first because it is a fact, not an inference from idleness: a row whose PR
//! merged settles even if it was touched a minute ago, and the reason recorded is the merge.
//!
//! ## What blocks a settle
//!
//! * **An explicit pin**, checked before either trigger is evaluated and before any write. A pinned
//!   row is immune however stale it is. The pin has three ways of being recorded (`state =
//!   'pinned'`, a `pinned_at` stamp, a non-empty `pinned_tabs` set) and
//!   [`lifecycle::effective_state`] already folds all three into one answer, so the guard reads
//!   that instead of re-deriving them.
//! * **A terminal row**: already `settled` or `archived` is a no-op -- counted, never an error,
//!   and never rewritten, so the reason recorded by the first settle stands.
//! * **A row that moved between the read and the write**: the settle goes through
//!   [`lifecycle::apply`] carrying the revision this pass read, so a pin (or any other write) that
//!   lands mid-sweep makes the settle refuse instead of overwrite it. That closes the read-then-
//!   write window the pin guard alone cannot see.
//! * **An unreadable clock**: a row whose `updated_at` or lane activity is not RFC 3339 is counted in
//!   [`SettleReport::unreadable`] and left alone. Never guessed at, never a panic.
//!
//! A **snoozed** row is NOT protected. A snooze is a temporary hide with an expiry the read path
//! already honours (`effective_state` reads an expired snooze as `active`), not a keep, so a
//! snoozed row that goes idle for the window settles like any other. Only an explicit pin is the
//! owner saying "leave this alone", and only that is honoured as one.
//!
//! ## The 7-day boundary is inclusive
//!
//! A row idle for exactly 7 days settles; day 6 does not. The deciding comparison is
//! `elapsed >= INACTIVITY_WINDOW` in [`sweep`] -- equality settles -- and
//! `the_seven_day_boundary_is_inclusive` asserts it rather than leaving it undefined.
//!
//! ## Where the reason is recorded
//!
//! In `lane_lifecycles.settle_reason` (migration `0059_lane_settle_reason.sql`), written by
//! [`lifecycle::apply`] through [`LifecycleOp::Settle`]'s `reason` field. Two values:
//! `"inactivity"` and `"pr_merged"`. `NULL` means nobody named a reason: a manual settle from the
//! HTTP layer, a row that never settled, or a row that moved on to another state (the reducer
//! clears `settle_reason` together with the other state evidence, so a row never explains a state
//! it is not in).
//!
//! ## Joining a row to a lane
//!
//! This module's rows are keyed by WORKTREE (`w:<wsId>/<slug>`, `p:<wsId>/<path>`); T7's PR link is
//! keyed by LANE (`lane_pull_requests.lane_key REFERENCES lanes(key)`, migration `0057`) because
//! `pull_request::record` validates its key with `state::valid_key` (lowercase letters, digits and
//! dashes only) and refuses a worktree key outright. So `for_lane(pool, worktree_key)` can never
//! match; the lookup goes through `pull_request::candidates_of`, the ONE derived-key rule this
//! repo has for the two tables: the slug of a `w:` key, and every lane whose `cwd` or registered
//! worktree is the row's path.
//!
//! The merge trigger does not run that join itself: [`sweep`] reads the model from
//! `lifecycle::model`, which already joins each row's pull request with `candidates_of`, and asks
//! `row.pull_request` whether it merged. One rule, one read: a row can never settle on a PR it does
//! not show, and a client can never be shown a merge the settle rule ignored.
use chrono::{DateTime, Utc};
use sqlx::SqlitePool;

use crate::error::{HubError, Result};
use crate::lanes::lifecycle::{self, Applied, LaneLifecycle, LifecycleOp};

/// The inactivity window: seven days, exactly. A row idle for this long settles.
pub const INACTIVITY_WINDOW_SECONDS: i64 = 7 * 24 * 60 * 60;

/// The recorded reason of a row settled for inactivity. Also in the migration's comment.
pub const REASON_INACTIVITY: &str = "inactivity";

/// The recorded reason of a row settled because its pull request merged.
pub const REASON_PR_MERGED: &str = "pr_merged";

/// What one pass did. Counts, not lists: a caller logs them and re-reads the model if it needs the
/// rows, so the report cannot go stale against the store.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettleReport {
    /// Live rows this pass looked at.
    pub considered: usize,
    /// Rows settled for inactivity, each with `settle_reason = "inactivity"`.
    pub settled_inactivity: usize,
    /// Rows settled because their pull request merged, each with `settle_reason = "pr_merged"`.
    pub settled_pr_merged: usize,
    /// Rows an explicit pin protected. Never settled, by either trigger.
    pub skipped_pinned: usize,
    /// Rows already `settled` (or `archived`): nothing written, the first settle stands.
    pub skipped_terminal: usize,
    /// Rows inside the inactivity window with no merged pull request: nothing to settle yet.
    pub skipped_recent: usize,
    /// Rows another writer changed between this pass's read and its write (a pin arriving
    /// mid-sweep): the settle was refused, not forced.
    pub concurrent: usize,
    /// Rows whose lifecycle or lane activity timestamp is unreadable, so no idle decision was possible.
    pub unreadable: usize,
    /// The keys behind [`Self::unreadable`], so a caller can name the broken rows it counted.
    pub unreadable_keys: Vec<String>,
}

impl SettleReport {
    /// Every row this pass settled, by either trigger.
    pub fn settled(&self) -> usize {
        self.settled_inactivity + self.settled_pr_merged
    }
}

/// One auto-settle pass over every live row, at the instant `now` names.
///
/// `now` is the injected clock: an RFC 3339 string, exactly what the lifecycle store already
/// takes. A string that is not RFC 3339 is refused here, at the boundary, rather than silently
/// read as "nothing is stale".
pub async fn sweep(pool: &SqlitePool, now: &str) -> Result<SettleReport> {
    let now_at = parse_rfc3339(now).ok_or_else(|| {
        HubError::message(format!(
            "lane_settle: now is not an RFC 3339 timestamp: {now}"
        ))
    })?;
    let model = lifecycle::model(pool, now).await?;
    let mut report = SettleReport::default();
    for section in model.sections {
        for row in section.rows {
            report.considered += 1;
            // The pin guard runs first: before either trigger is evaluated and before any write.
            if is_pinned(&row) {
                report.skipped_pinned += 1;
                continue;
            }
            if is_terminal(&row.state) {
                report.skipped_terminal += 1;
                continue;
            }
            let activity = lane_activity(pool, &row).await?;
            let reason = match trigger(&row, now_at, &activity) {
                Trigger::Settle(reason) => reason,
                Trigger::Recent => {
                    report.skipped_recent += 1;
                    continue;
                }
                Trigger::Unreadable => {
                    report.unreadable += 1;
                    report.unreadable_keys.push(row.worktree_key.clone());
                    continue;
                }
            };
            // Through the lifecycle reducer, never with SQL of this module's own. The revision
            // this pass read is the guard: if a pin (or any other write) landed in between,
            // `apply` answers `Stale` and nothing is overwritten.
            let applied = lifecycle::apply(
                pool,
                &row.worktree_key,
                &row.workspace_id,
                &row.worktree_path,
                &LifecycleOp::Settle {
                    reason: Some(reason.to_owned()),
                },
                Some(row.revision),
                now,
            )
            .await?;
            match applied {
                Applied::Written(_) if reason == REASON_PR_MERGED => report.settled_pr_merged += 1,
                Applied::Written(_) if reason == REASON_INACTIVITY => report.settled_inactivity += 1,
                // Unreachable today: `Trigger::Settle` only ever carries the two constants above.
                // Counted as a refusal rather than mislabelled as one of the two reasons.
                Applied::Written(_) => report.concurrent += 1,
                Applied::Stale(_) => report.concurrent += 1,
            }
        }
    }
    Ok(report)
}

/// What one row's own evidence says.
enum Trigger {
    /// Settle it, recording this reason.
    Settle(&'static str),
    /// Inside the inactivity window with no merged pull request: nothing to do.
    Recent,
    /// The row's own clock cannot be read, so no idle decision is possible.
    Unreadable,
}

/// Why this row settles, if it does.
///
/// Pure: the merge evidence is the row's own joined projection (`lifecycle::model` fills it), so
/// this reads no store and no clock. `now_at` is the caller's injected instant.
fn trigger(row: &LaneLifecycle, now_at: DateTime<Utc>, activity: &[String]) -> Trigger {
    if row.pull_request.as_ref().is_some_and(|pr| pr.is_merged()) {
        return Trigger::Settle(REASON_PR_MERGED);
    }
    let Some(mut last_active_at) = parse_rfc3339(&row.updated_at) else {
        return Trigger::Unreadable;
    };
    for at in activity {
        let Some(at) = parse_rfc3339(at) else {
            return Trigger::Unreadable;
        };
        last_active_at = last_active_at.max(at);
    }
    // Inclusive: exactly seven days idle settles, measured from the newest actual activity
    // or lifecycle write. Removing the activity fold settles an actively driven old row.
    if now_at - last_active_at >= chrono::Duration::seconds(INACTIVITY_WINDOW_SECONDS) {
        Trigger::Settle(REASON_INACTIVITY)
    } else {
        Trigger::Recent
    }
}

/// Actual activity for the same lane candidates used by the lifecycle's PR projection.
/// `last_activity_at` alone is insufficient: confirmed supervisor turns append `lane_turns.at`
/// without updating either that column or the lifecycle row. Read both, comparing parsed instants
/// in `trigger` rather than taking a lexicographic MAX of RFC 3339 strings with different offsets.
async fn lane_activity(pool: &SqlitePool, row: &LaneLifecycle) -> Result<Vec<String>> {
    let candidates = crate::lanes::pull_request::candidate_keys(
        pool,
        &row.worktree_key,
        &row.worktree_path,
    )
    .await?;
    let mut activity = Vec::new();
    for key in candidates {
        let times: Vec<String> = sqlx::query_scalar(
            "SELECT last_activity_at FROM lanes WHERE key = ?1
             UNION ALL SELECT at FROM lane_turns WHERE lane_key = ?1",
        )
        .bind(&key)
        .fetch_all(pool)
        .await?;
        activity.extend(times);
    }
    Ok(activity)
}

/// A row an explicit pin protects. [`lifecycle::effective_state`] folds every way a pin is
/// recorded into one answer, so this reads that rather than re-deriving the three cases.
fn is_pinned(row: &LaneLifecycle) -> bool {
    row.effective_state == "pinned"
}

/// A row with nothing left to settle.
///
/// `archived` is already filtered out of [`lifecycle::model`], so in practice only `settled`
/// reaches here; the arm is kept so this predicate states the whole rule rather than the half the
/// current read happens to return.
fn is_terminal(state: &str) -> bool {
    matches!(state, "settled" | "archived")
}

/// Same helper shape as `lanes/lifecycle.rs`: parse, then normalise to UTC so two timestamps in
/// different offsets compare correctly.
fn parse_rfc3339(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|at| at.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lanes::pull_request;

    /// Every fixture is anchored to this instant. A test names its "now" as a day offset from it,
    /// so no test reads a clock and none waits: the injected clock is a string computed from a
    /// constant.
    const ANCHOR: &str = "2026-11-01T00:00:00+00:00";

    fn anchor() -> DateTime<Utc> {
        parse_rfc3339(ANCHOR).unwrap()
    }

    /// The injected clock: day `n` after the anchor, RFC 3339 -- exactly what `sweep` takes.
    fn day(n: i64) -> String {
        (anchor() + chrono::Duration::days(n)).to_rfc3339()
    }

    /// A live lifecycle row nothing has written since `updated_at`.
    async fn row(pool: &SqlitePool, key: &str, path: &str, updated_at: &str) {
        sqlx::query(
            "INSERT INTO lane_lifecycles(worktree_key, workspace_id, worktree_path, state,
                 updated_at, created_at)
             VALUES(?, 'ferryx', ?, 'active', ?, ?)",
        )
        .bind(key)
        .bind(path)
        .bind(updated_at)
        .bind(updated_at)
        .execute(pool)
        .await
        .unwrap();
    }

    /// A lane launched at `cwd`. T7's PR link is keyed by this lane, never by a worktree key.
    async fn lane(pool: &SqlitePool, key: &str, cwd: &str) {
        sqlx::query(
            "INSERT INTO lanes(key, title, lead_session_key, cwd, session_key, status,
                 done_signals, last_activity_at)
             VALUES(?, 'Lane', 'lead', ?, 'lead', 'working', '[]', ?)",
        )
        .bind(key)
        .bind(cwd)
        .bind(ANCHOR)
        .execute(pool)
        .await
        .unwrap();
    }

    fn pr(lane_key: &str, state: &str) -> pull_request::LanePullRequest {
        pull_request::LanePullRequest {
            lane_key: lane_key.to_owned(),
            repo: "acme/widgets".to_owned(),
            pr_number: 12,
            state: state.to_owned(),
            url: "https://github.com/acme/widgets/pull/12".to_owned(),
            title: "Fix the parser".to_owned(),
            head_branch: "lane/ledger".to_owned(),
            observed_at: ANCHOR.to_owned(),
        }
    }

    async fn stored(pool: &SqlitePool, key: &str) -> LaneLifecycle {
        lifecycle::one(pool, key, ANCHOR).await.unwrap().unwrap()
    }

    /// A merged pull request is visible to the settle path, through the same joined row the sweep
    /// reads. Fails if `lifecycle::model` stops filling `pull_request` (the row would show no link)
    /// or if the join stops reaching a lane-keyed PR from a worktree-keyed row (`None` instead of
    /// the merge). The open-PR half makes sure this is not "any PR merges the row".
    #[tokio::test]
    async fn a_merged_pull_request_is_visible_to_the_settle_path() {
        let pool = crate::storage::memory().await.unwrap();
        lane(&pool, "ledger", "/code/ledger").await;
        row(&pool, "w:ferryx/ledger", "/code/ledger", ANCHOR).await;
        pull_request::record(&pool, &pr("ledger", "merged"))
            .await
            .unwrap();
        let shown = lifecycle::one(&pool, "w:ferryx/ledger", ANCHOR)
            .await
            .unwrap()
            .unwrap()
            .pull_request
            .expect("the row carries the merged PR");
        assert!(shown.is_merged());
        // The sweep's own view of that row, one call later: it must settle on exactly this merge.
        assert_eq!(
            match trigger(&stored(&pool, "w:ferryx/ledger").await, anchor(), &[]) {
                Trigger::Settle(reason) => reason,
                _ => panic!("a merged PR must be the pr_merged trigger"),
            },
            REASON_PR_MERGED
        );
        // An open PR is not a settle reason, so the assertion above is not trivially true.
        pull_request::record(&pool, &pr("ledger", "open")).await.unwrap();
        assert!(matches!(
            trigger(&stored(&pool, "w:ferryx/ledger").await, anchor(), &[]),
            Trigger::Recent
        ));
    }

    #[tokio::test]
    async fn an_injected_clock_at_day_six_does_not_settle() {
        let pool = crate::storage::memory().await.unwrap();
        row(&pool, "p:ferryx/code/a", "/code/a", ANCHOR).await;
        let report = sweep(&pool, &day(6)).await.unwrap();
        assert_eq!(report.settled(), 0, "six days is inside the seven-day window");
        assert_eq!(report.considered, 1);
        assert_eq!(report.skipped_recent, 1);
        let saved = stored(&pool, "p:ferryx/code/a").await;
        assert_eq!(saved.state, "active");
        assert!(saved.settled_at.is_none());
        assert!(saved.settle_reason.is_none());
    }

    #[tokio::test]
    async fn an_injected_clock_at_day_eight_settles_with_the_inactivity_reason() {
        let pool = crate::storage::memory().await.unwrap();
        row(&pool, "p:ferryx/code/a", "/code/a", ANCHOR).await;
        let now = day(8);
        let report = sweep(&pool, &now).await.unwrap();
        assert_eq!(report.settled_inactivity, 1);
        assert_eq!(report.settled_pr_merged, 0);
        let saved = stored(&pool, "p:ferryx/code/a").await;
        assert_eq!(saved.state, "settled");
        assert_eq!(saved.settled_at.as_deref(), Some(now.as_str()));
        assert_eq!(saved.settle_reason.as_deref(), Some(REASON_INACTIVITY));
        assert_eq!(saved.settle_reason.as_deref(), Some("inactivity"));
    }

    /// Removing the lane-activity read/fold makes this old lifecycle row settle at day 8.
    #[tokio::test]
    async fn recent_lane_activity_keeps_an_old_lifecycle_row_active() {
        let pool = crate::storage::memory().await.unwrap();
        lane(&pool, "busy", "/code/busy").await;
        row(&pool, "w:ferryx/busy", "/code/busy", ANCHOR).await;
        sqlx::query("UPDATE lanes SET last_activity_at = ? WHERE key = 'busy'")
            .bind(day(7))
            .execute(&pool)
            .await
            .unwrap();
        let report = sweep(&pool, &day(8)).await.unwrap();
        assert_eq!(report.settled(), 0, "recent lane activity blocks inactivity");
        assert_eq!(report.skipped_recent, 1);
        let saved = stored(&pool, "w:ferryx/busy").await;
        assert_eq!(saved.state, "active");
        assert_eq!(saved.updated_at, ANCHOR, "no lifecycle write occurred");
    }

    /// Confirmed agent turns use the real turn recorder, which does not touch last_activity_at.
    /// Removing the lane_turns read settles this row despite a turn just one day ago.
    #[tokio::test]
    async fn a_recent_agent_turn_keeps_an_old_lifecycle_row_active() {
        let pool = crate::storage::memory().await.unwrap();
        lane(&pool, "turning", "/code/turning").await;
        // A plain-folder row exercises the path-owner join rather than the worktree slug.
        row(&pool, "p:ferryx/code/turning", "/code/turning", ANCHOR).await;
        crate::goals::turns::record_turn_at(
            &pool,
            "turning",
            crate::goals::types::TurnOrigin::SupervisorProposal,
            Some("continue the task"),
            Some("screen-2"),
            &day(7),
        )
        .await
        .unwrap();
        let report = sweep(&pool, &day(8)).await.unwrap();
        assert_eq!(report.settled(), 0, "a recent agent turn blocks inactivity");
        assert_eq!(report.skipped_recent, 1);
        let saved = stored(&pool, "p:ferryx/code/turning").await;
        assert_eq!(saved.state, "active");
        assert_eq!(saved.updated_at, ANCHOR);
        // Activity delays, rather than disables, the inclusive seven-day settle rule.
        assert_eq!(sweep(&pool, &day(14)).await.unwrap().settled_inactivity, 1);
    }

    /// The boundary this module documents: exactly seven days idle settles, six does not.
    #[tokio::test]
    async fn the_seven_day_boundary_is_inclusive() {
        let pool = crate::storage::memory().await.unwrap();
        row(&pool, "p:ferryx/code/edge", "/code/edge", ANCHOR).await;
        let report = sweep(&pool, &day(7)).await.unwrap();
        assert_eq!(report.settled_inactivity, 1, "exactly seven days idle settles");
        assert_eq!(
            stored(&pool, "p:ferryx/code/edge").await.settle_reason.as_deref(),
            Some(REASON_INACTIVITY)
        );
    }

    /// Both triggers, one guard: a pinned row is immune at day 800, and immune to a merged pull
    /// request as well.
    #[tokio::test]
    async fn a_pinned_row_never_settles_even_at_day_eight_hundred() {
        let pool = crate::storage::memory().await.unwrap();
        // (a) pinned, stale, no pull request.
        row(&pool, "p:ferryx/code/pinned", "/code/pinned", ANCHOR).await;
        lifecycle::apply(
            &pool,
            "p:ferryx/code/pinned",
            "ferryx",
            "/code/pinned",
            &LifecycleOp::Pin { pinned: true },
            None,
            ANCHOR,
        )
        .await
        .unwrap();
        // (b) pinned, stale, and its pull request merged: the second trigger is blocked too.
        row(&pool, "w:ferryx/merged-pinned", "/code/merged-pinned", ANCHOR).await;
        lifecycle::apply(
            &pool,
            "w:ferryx/merged-pinned",
            "ferryx",
            "/code/merged-pinned",
            &LifecycleOp::Pin { pinned: true },
            None,
            ANCHOR,
        )
        .await
        .unwrap();
        lane(&pool, "merged-pinned", "/code/merged-pinned").await;
        pull_request::record(&pool, &pr("merged-pinned", "merged"))
            .await
            .unwrap();

        let report = sweep(&pool, &day(800)).await.unwrap();
        assert_eq!(report.settled(), 0, "a pin blocks inactivity and a merge alike");
        assert_eq!(report.skipped_pinned, 2);
        for key in ["p:ferryx/code/pinned", "w:ferryx/merged-pinned"] {
            let saved = stored(&pool, key).await;
            assert_eq!(saved.state, "pinned", "{key}");
            assert!(saved.settled_at.is_none(), "{key}");
            assert!(saved.settle_reason.is_none(), "{key}");
        }
    }

    /// Without storing the imported directory, the old row cannot join fresh lane activity and
    /// incorrectly settles on day 8. The idle control proves the sweep actually ran.
    #[tokio::test]
    async fn an_imported_plain_folder_joins_activity_and_pull_requests() {
        let pool = crate::storage::memory().await.expect("memory database");
        let order = std::collections::BTreeMap::from([("ferryx".to_owned(), vec![
            "sidebar-worktree:ferryx:ferryx:/code/active-folder".to_owned(),
            "/code/idle-folder".to_owned(),
        ])]);
        lifecycle::import_local_order(&pool, &order, ANCHOR).await.expect("import");
        lane(&pool, "folder-lane", "/code/active-folder").await;
        sqlx::query("UPDATE lanes SET last_activity_at = ? WHERE key = 'folder-lane'")
            .bind(day(8)).execute(&pool).await.expect("fresh activity");
        pull_request::record(&pool, &pr("folder-lane", "open")).await.expect("record PR");
        let key = "p:ferryx/code/active-folder";
        let imported = stored(&pool, key).await;
        assert_eq!(imported.worktree_path, "/code/active-folder");
        assert_eq!(imported.pull_request.expect("path joins PR").lane_key, "folder-lane");
        let report = sweep(&pool, &day(8)).await.expect("sweep");
        assert_eq!(report.skipped_recent, 1);
        assert_eq!(report.settled_inactivity, 1);
        assert_eq!(stored(&pool, key).await.state, "active");
        assert_eq!(stored(&pool, "p:ferryx/code/idle-folder").await.state, "settled");
    }

    /// A merged pull request settles on its own, at an instant where inactivity cannot fire, and
    /// records a reason distinguishable from the inactivity one.
    #[tokio::test]
    async fn a_merged_pull_request_settles_with_the_pr_reason() {
        let pool = crate::storage::memory().await.unwrap();
        lane(&pool, "ledger", "/code/ledger").await;
        row(&pool, "w:ferryx/ledger", "/code/ledger", ANCHOR).await;

        pull_request::record(&pool, &pr("ledger", "open")).await.unwrap();
        let open = sweep(&pool, &day(1)).await.unwrap();
        assert_eq!(open.settled(), 0, "an open pull request settles nothing");
        assert_eq!(open.skipped_recent, 1);
        assert_eq!(stored(&pool, "w:ferryx/ledger").await.state, "active");

        pull_request::record(&pool, &pr("ledger", "merged"))
            .await
            .unwrap();
        // Day 1: the merge alone is the trigger, so this cannot pass on the inactivity rule.
        let report = sweep(&pool, &day(1)).await.unwrap();
        assert_eq!(report.settled_pr_merged, 1);
        assert_eq!(report.settled_inactivity, 0);
        let saved = stored(&pool, "w:ferryx/ledger").await;
        assert_eq!(saved.state, "settled");
        assert_eq!(saved.settle_reason.as_deref(), Some(REASON_PR_MERGED));
        assert_ne!(
            saved.settle_reason.as_deref(),
            Some(REASON_INACTIVITY),
            "the two triggers must stay tellable apart"
        );
    }

    /// A row that already settled is terminal: the pass counts it and writes nothing, so the
    /// reason recorded by the first settle stands even when both triggers now point at the row.
    #[tokio::test]
    async fn settling_an_already_settled_row_leaves_the_original_reason_untouched() {
        let pool = crate::storage::memory().await.unwrap();
        row(&pool, "p:ferryx/code/b", "/code/b", ANCHOR).await;
        let first = sweep(&pool, &day(8)).await.unwrap();
        assert_eq!(first.settled_inactivity, 1);
        let original = stored(&pool, "p:ferryx/code/b").await;
        assert_eq!(original.settle_reason.as_deref(), Some(REASON_INACTIVITY));

        // At day 800 the row is stale *and* its pull request merged: neither may rewrite it.
        lane(&pool, "late", "/code/b").await;
        pull_request::record(&pool, &pr("late", "merged")).await.unwrap();
        let second = sweep(&pool, &day(800)).await.unwrap();
        assert_eq!(second.settled(), 0, "a settled row is terminal");
        assert_eq!(second.skipped_terminal, 1);

        let after = stored(&pool, "p:ferryx/code/b").await;
        assert_eq!(after.state, "settled");
        assert_eq!(after.settled_at, original.settled_at, "the first settle time stands");
        assert_eq!(
            after.settle_reason.as_deref(),
            Some(REASON_INACTIVITY),
            "the first reason stands, not 'pr_merged'"
        );
        assert_eq!(after.revision, original.revision, "nothing was written");
    }

    /// A row whose own clock cannot be read is counted and skipped: never settled on a guess,
    /// never a panic.
    #[tokio::test]
    async fn a_malformed_updated_at_is_counted_unreadable_and_does_not_panic() {
        let pool = crate::storage::memory().await.unwrap();
        row(&pool, "p:ferryx/code/bad", "/code/bad", "yesterday-ish").await;
        let report = sweep(&pool, &day(800)).await.unwrap();
        assert_eq!(report.settled(), 0);
        assert_eq!(report.unreadable, 1);
        assert_eq!(report.unreadable_keys, ["p:ferryx/code/bad"]);
        let saved = stored(&pool, "p:ferryx/code/bad").await;
        assert_eq!(saved.state, "active");
        assert!(saved.settle_reason.is_none());
    }
}
