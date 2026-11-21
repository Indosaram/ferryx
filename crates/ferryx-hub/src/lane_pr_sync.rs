//! The pass that keeps `lane_pull_requests` current, and then lets T6's rule see it.
//!
//! ## Two halves, one pass
//!
//! T7 recorded a pull request but nothing asked GitHub for one, and T6 could settle on a merged PR
//! that no real read ever put in the table. This module is the missing caller for both: every tick
//! it
//!
//! 1. reads the lanes worth polling ([`targets`]),
//! 2. asks the [`PrSource`] once per lane and records what came back ([`sync`]),
//! 3. runs [`crate::lane_settle::sweep`] over the projection it just wrote ([`pass`]).
//!
//! Step 3 is why the two are one pass: a merge observed in step 2 settles the lane in the same
//! tick, in the right order, with no second cadence to keep in step.
//!
//! ## Where the `gh` process lives
//!
//! Not here. [`PrSource`] is the seam: the production implementation is `GhPrSource` in
//! `src-tauri/src/ipc/github_pr.rs`, which reuses the read-only `gh pr view` path this repo already
//! tests and runs it on a blocking worker. This crate therefore spawns nothing, and a sweep can
//! never block the async reactor.
//!
//! ## The clock is a parameter
//!
//! [`pass`] takes `now: &str`, exactly like [`crate::lane_settle::sweep`] and
//! `lanes::lifecycle::apply`: a test says "now is day 8" by passing the day-8 string. Only
//! [`serve`] reads the real clock, once per tick, because it is the loop that owns the cadence.
//!
//! ## Absence is not failure
//!
//! A lane with no pull request, a machine with no `gh`, an unauthenticated `gh` and a project whose
//! origin is not GitHub all answer `Ok(None)`: nothing is recorded and nothing is an error. That is
//! the difference between "this lane has no PR" and "the read broke". A read that really broke is
//! `Err`; it is counted in [`SyncReport::failed`] with its lane key and the pass carries on, so one
//! bad lane cannot stop the other lanes from syncing.
use std::sync::Arc;
use std::time::Duration;

use sqlx::SqlitePool;

use crate::error::Result;
use crate::lane_settle::{self, SettleReport};
use crate::lanes::lifecycle::{self, LaneLifecycleModel};
use crate::lanes::pull_request::{self, PrReadTarget, PrSource};
use crate::storage::now_rfc3339;

/// How often [`serve`] runs a pass: five minutes.
///
/// A pull request's state changes when a human merges or closes it, so this is a warm-up latency,
/// not a live feed. `pr view` is one small read per live lane, and the hub's other GitHub reads
/// (`supervisor/pr_lane::REFRESH_EVERY`) already run at one minute.
pub const SYNC_EVERY: Duration = Duration::from_secs(5 * 60);

/// What one sync did. Counts plus the keys behind the failures, like [`SettleReport`]: a caller
/// logs them and re-reads the model if it wants the rows.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    /// Lanes this pass asked about.
    pub considered: usize,
    /// Lanes whose recorded PR changed (a first observation, a new number, or a new state).
    pub recorded: usize,
    /// Lanes whose read repeated what was already recorded: nothing was written.
    pub unchanged: usize,
    /// Lanes with no pull request to record (no PR, no `gh`, no auth, no GitHub origin).
    pub absent: usize,
    /// Lanes whose read failed. The pass continued; the lane keeps whatever was recorded before.
    pub failed: usize,
    /// The keys behind [`Self::failed`], so a caller can name them.
    pub failed_lanes: Vec<String>,
}

/// What one [`pass`] did: the sync, then the settle decision it enabled.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PassReport {
    pub prs: SyncReport,
    pub settle: SettleReport,
}

/// The lanes worth polling for a pull request: every lane that has not ended yet.
///
/// A lane in `verified_done` / `closed` / `failed` can no longer settle, so polling it would only
/// spend a `gh` call. Its last recorded PR stays in the table and stays readable -- this projection
/// answers "what was this lane's PR", and a row is never deleted by a later pass.
pub async fn targets(pool: &SqlitePool) -> Result<Vec<PrReadTarget>> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT key, cwd FROM lanes
          WHERE cwd <> '' AND status NOT IN ('verified_done','closed','failed')
          ORDER BY key",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(lane_key, cwd)| PrReadTarget { lane_key, cwd })
        .collect())
}

/// Asks `source` about every target and records what it saw.
///
/// The recorded row is always keyed by the lane this pass asked about, never by the key the answer
/// names: a source that reports a pull request under another key must not be able to write that
/// lane's row. Failures are counted and named, never returned, so one unreachable lane cannot stop
/// a sweep.
pub async fn sync(pool: &SqlitePool, source: &dyn PrSource, asked: &[PrReadTarget]) -> Result<SyncReport> {
    let mut report = SyncReport::default();
    for target in asked {
        report.considered += 1;
        let observed = match source.read(target).await {
            Ok(observed) => observed,
            Err(error) => {
                tracing::warn!(lane = %target.lane_key, %error, "lane pull request read failed");
                report.failed += 1;
                report.failed_lanes.push(target.lane_key.clone());
                continue;
            }
        };
        let Some(mut pr) = observed else {
            report.absent += 1;
            continue;
        };
        pr.lane_key = target.lane_key.clone();
        match pull_request::record(pool, &pr).await {
            Ok(true) => report.recorded += 1,
            Ok(false) => report.unchanged += 1,
            Err(error) => {
                tracing::warn!(lane = %target.lane_key, %error, "a lane pull request was not recorded");
                report.failed += 1;
                report.failed_lanes.push(target.lane_key.clone());
            }
        }
    }
    Ok(report)
}

/// One pass: sync the projection, then let T6's rule read it.
///
/// `now` is the injected clock, forwarded to [`lane_settle::sweep`] unchanged (which refuses a
/// string that is not RFC 3339 rather than reading it as "nothing is stale").
pub async fn pass(pool: &SqlitePool, source: &dyn PrSource, now: &str) -> Result<PassReport> {
    let polled = targets(pool).await?;
    let prs = sync(pool, source, &polled).await?;
    let settle = lane_settle::sweep(pool, now).await?;
    Ok(PassReport { prs, settle })
}

/// Publishes the durable read model only after a pass changed PRs or settled rows.
///
/// The model carries the existing worktree keys and joined PRs, not lane-key guesses. Keeping
/// this outside [`pass`] leaves the sync/settle decision independent of its owner's event bus.
async fn publish_changes(
    pool: &SqlitePool,
    report: &PassReport,
    now: &str,
    publish: &impl Fn(&LaneLifecycleModel),
) -> Result<()> {
    if report.prs.recorded > 0 || report.settle.settled() > 0 {
        publish(&lifecycle::model(pool, now).await?);
    }
    Ok(())
}

/// Runs [`pass`] every `every`, on the caller's task, publishing changed lifecycle rows.
///
/// The lifecycle belongs to the caller: the hub's generation spawns this and aborts it with the
/// other generation tasks (`HUB_TASKS` in `src-tauri/src/hub_runtime.rs`), so a standing-down hub
/// stops polling `gh` and this loop has no restart policy of its own. The first tick fires
/// immediately, so a hub start syncs before the first interval elapses.
pub async fn serve(
    pool: SqlitePool,
    source: Arc<dyn PrSource>,
    every: Duration,
    publish: impl Fn(&LaneLifecycleModel) + Send + Sync,
) {
    let mut ticks = tokio::time::interval(every);
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        ticks.tick().await;
        let now = now_rfc3339();
        match pass(&pool, source.as_ref(), &now).await {
            Ok(report) => {
                if let Err(error) = publish_changes(&pool, &report, &now, &publish).await {
                    tracing::warn!(%error, "lane lifecycle publication failed");
                }
                tracing::info!(
                    considered = report.prs.considered,
                    recorded = report.prs.recorded,
                    absent = report.prs.absent,
                    failed = report.prs.failed,
                    settled = report.settle.settled(),
                    "lane pull-request sync finished"
                );
            }
            Err(error) => tracing::warn!(%error, "lane pull-request sync failed"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lanes::lifecycle::{self, LifecycleOp};
    use crate::lanes::pull_request::{lane_branch, LanePullRequest};
    use async_trait::async_trait;

    const ANCHOR: &str = "2026-11-01T00:00:00+00:00";

    /// Day `n` after the anchor, RFC 3339 -- the injected clock.
    fn day(n: i64) -> String {
        (chrono::DateTime::parse_from_rfc3339(ANCHOR)
            .unwrap()
            .with_timezone(&chrono::Utc)
            + chrono::Duration::days(n))
        .to_rfc3339()
    }

    /// A [`PrSource`] that answers from a table instead of starting `gh`.
    ///
    /// `Answer::Fail` is the case that must not stop a sweep: one lane whose read broke, beside a
    /// lane whose read succeeded.
    enum Answer {
        Pr(&'static str),
        None,
        Fail,
    }

    struct FakeSource {
        answers: Vec<(&'static str, Answer)>,
        /// The observation time this source reports. Two passes with different times are how an
        /// "identical state" is told apart from "a new read that saw the same thing".
        observed_at: &'static str,
    }

    impl FakeSource {
        fn new(answers: Vec<(&'static str, Answer)>) -> Self {
            Self { answers, observed_at: ANCHOR }
        }

        fn observed_at(mut self, at: &'static str) -> Self {
            self.observed_at = at;
            self
        }
    }

    #[async_trait]
    impl PrSource for FakeSource {
        async fn read(&self, target: &PrReadTarget) -> Result<Option<LanePullRequest>> {
            let answer = self
                .answers
                .iter()
                .find(|(lane, _)| *lane == target.lane_key.as_str())
                .map(|(_, answer)| answer);
            match answer {
                Some(Answer::Pr(state)) => Ok(Some(LanePullRequest {
                    // Deliberately the wrong key: `sync` must re-key it to the lane it asked about.
                    lane_key: "wrong-key".to_owned(),
                    repo: "acme/widgets".to_owned(),
                    pr_number: 12,
                    state: (*state).to_owned(),
                    url: "https://github.com/acme/widgets/pull/12".to_owned(),
                    title: "Fix the parser".to_owned(),
                    head_branch: lane_branch(&target.lane_key),
                    observed_at: self.observed_at.to_owned(),
                })),
                Some(Answer::Fail) => Err(crate::error::HubError::message("gh is on fire")),
                Some(Answer::None) | None => Ok(None),
            }
        }
    }

    /// A lane running at `cwd`, which is the directory whose origin names its repository.
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

    /// A live lifecycle row for a managed worktree `w:ferryx/<slug>`.
    async fn row(pool: &SqlitePool, slug: &str, path: &str) {
        lifecycle::apply(
            pool,
            &format!("w:ferryx/{slug}"),
            "ferryx",
            path,
            &LifecycleOp::Activate,
            None,
            ANCHOR,
        )
        .await
        .unwrap();
    }

    /// A lane with no pull request records nothing, and nothing about it is an error.
    ///
    /// Fails if `sync` treats absence as a failure (`failed` would be 1, not `absent`) or records a
    /// placeholder row (`all` would not be empty).
    #[tokio::test]
    async fn a_lane_without_a_pull_request_records_nothing_and_does_not_error() {
        let pool = crate::storage::memory().await.unwrap();
        lane(&pool, "ledger", "/code/ledger").await;
        let source = FakeSource::new(vec![("ledger", Answer::None)]);
        let report = sync(&pool, &source, &targets(&pool).await.unwrap()).await.unwrap();
        assert_eq!(report.considered, 1);
        assert_eq!(report.absent, 1);
        assert_eq!(report.recorded, 0);
        assert_eq!(report.failed, 0);
        assert!(pull_request::all(&pool).await.unwrap().is_empty());
        // And the row a client reads carries no link rather than an empty one.
        row(&pool, "ledger", "/code/ledger").await;
        let stored = lifecycle::one(&pool, "w:ferryx/ledger", ANCHOR)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.pull_request, None);
    }

    /// The whole point: a read reaches the row a client renders, keyed by the lane that was asked
    /// about rather than the key the answer happened to carry.
    ///
    /// Fails if the row stops carrying the projection, or if `sync` trusts the answer's lane key
    /// (`wrong-key` would be recorded and `for_lane("ledger")` would be empty).
    #[tokio::test]
    async fn a_recorded_pull_request_reads_back_on_the_lane_row() {
        let pool = crate::storage::memory().await.unwrap();
        lane(&pool, "ledger", "/code/ledger").await;
        row(&pool, "ledger", "/code/ledger").await;
        let source = FakeSource::new(vec![("ledger", Answer::Pr("open"))]);
        let report = sync(&pool, &source, &targets(&pool).await.unwrap()).await.unwrap();
        assert_eq!((report.recorded, report.absent, report.failed), (1, 0, 0));

        let recorded = pull_request::for_lane(&pool, "ledger").await.unwrap().unwrap();
        assert_eq!(recorded.state, "open");
        assert_eq!(recorded.head_branch, "lane/ledger");

        let stored = lifecycle::one(&pool, "w:ferryx/ledger", ANCHOR)
            .await
            .unwrap()
            .unwrap();
        let linked = stored.pull_request.expect("the row carries the recorded PR");
        assert_eq!(linked.pr_number, 12);
        assert_eq!(linked.state, "open");

        // The same row through the whole-model read, which is what the sidebar fetches.
        let model = lifecycle::model(&pool, ANCHOR).await.unwrap();
        let row = &model.sections[0].rows[0];
        assert_eq!(row.worktree_key, "w:ferryx/ledger");
        assert_eq!(row.pull_request.as_ref().map(|pr| pr.pr_number), Some(12));
    }

    /// A second identical read writes nothing: not the row, not its `observed_at`.
    ///
    /// The two passes report *different* observation times, so "nothing changed" can only be the
    /// row comparison. Fails if `record`'s conditional upsert is dropped: the second pass would
    /// answer `recorded: 1` and re-stamp `observed_at` with the later read.
    #[tokio::test]
    async fn a_second_pass_over_identical_state_is_idempotent() {
        let pool = crate::storage::memory().await.unwrap();
        lane(&pool, "ledger", "/code/ledger").await;
        let early = FakeSource::new(vec![("ledger", Answer::Pr("open"))]);
        let first = sync(&pool, &early, &targets(&pool).await.unwrap()).await.unwrap();
        assert_eq!(first.recorded, 1);
        let before = pull_request::for_lane(&pool, "ledger").await.unwrap().unwrap();
        assert_eq!(before.observed_at, ANCHOR);

        let later = FakeSource::new(vec![("ledger", Answer::Pr("open"))])
            .observed_at("2026-11-01T06:00:00+00:00");
        let second = sync(&pool, &later, &targets(&pool).await.unwrap()).await.unwrap();
        assert_eq!(second.recorded, 0);
        assert_eq!(second.unchanged, 1);
        let after = pull_request::for_lane(&pool, "ledger").await.unwrap().unwrap();
        assert_eq!(before, after, "an unchanged observation does not touch the row");
        assert_eq!(after.observed_at, ANCHOR, "observed_at still means 'unchanged since'");
    }

    /// T6's path, reached the way production reaches it: the merge is observed by the sync and the
    /// same pass settles the row with the `pr_merged` reason, at an instant where the inactivity
    /// rule cannot fire.
    ///
    /// Fails if `pass` stops running the settle sweep (the row would stay `active`), if the writer
    /// stops running (there would be no merged PR to see), or if the row and the lane key stop
    /// joining (`sweep` would find no merged PR).
    #[tokio::test]
    async fn a_merged_pull_request_observed_by_the_sync_settles_the_lane() {
        let pool = crate::storage::memory().await.unwrap();
        lane(&pool, "ledger", "/code/ledger").await;
        row(&pool, "ledger", "/code/ledger").await;

        // Day 1: an open PR settles nothing, so a later settle cannot be the inactivity rule.
        let open = FakeSource::new(vec![("ledger", Answer::Pr("open"))]);
        let report = pass(&pool, &open, &day(1)).await.unwrap();
        assert_eq!(report.prs.recorded, 1);
        assert_eq!(report.settle.settled(), 0);
        assert_eq!(
            lifecycle::one(&pool, "w:ferryx/ledger", &day(1))
                .await
                .unwrap()
                .unwrap()
                .state,
            "active"
        );

        // The owner merges in their own shell; the next pass observes it and settles on it.
        let merged = FakeSource::new(vec![("ledger", Answer::Pr("merged"))])
            .observed_at("2026-11-02T00:00:00+00:00");
        let report = pass(&pool, &merged, &day(1)).await.unwrap();
        assert_eq!(report.prs.recorded, 1, "the merge is a new observation");
        assert_eq!(report.settle.settled_pr_merged, 1);
        assert_eq!(report.settle.settled_inactivity, 0);
        let stored = lifecycle::one(&pool, "w:ferryx/ledger", &day(1))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.state, "settled");
        assert_eq!(
            stored.settle_reason.as_deref(),
            Some(crate::lane_settle::REASON_PR_MERGED)
        );
        assert_eq!(
            stored.pull_request.as_ref().map(|pr| pr.is_merged()),
            Some(true),
            "the settled row still shows the merge it settled on"
        );
    }

    /// One lane whose read broke does not stop the sweep: the other lane still syncs.
    ///
    /// Fails if `sync` returns the error instead of counting it (`pass` would fail and the healthy
    /// lane's PR would never be recorded).
    #[tokio::test]
    async fn a_failing_read_is_counted_and_does_not_stop_the_sweep() {
        let pool = crate::storage::memory().await.unwrap();
        lane(&pool, "broken", "/code/broken").await;
        lane(&pool, "ledger", "/code/ledger").await;
        let source = FakeSource::new(vec![
            ("broken", Answer::Fail),
            ("ledger", Answer::Pr("open")),
        ]);
        let report = sync(&pool, &source, &targets(&pool).await.unwrap()).await.unwrap();
        assert_eq!(report.considered, 2);
        assert_eq!(report.failed, 1);
        assert_eq!(report.failed_lanes, ["broken"]);
        assert_eq!(report.recorded, 1);
        assert_eq!(
            pull_request::for_lane(&pool, "ledger").await.unwrap().unwrap().state,
            "open"
        );
        assert_eq!(pull_request::for_lane(&pool, "broken").await.unwrap(), None);
    }

    /// Without this test an unchanged pass could call the publisher on every tick.
    #[tokio::test]
    async fn an_unchanged_pass_does_not_publish() {
        let pool = crate::storage::memory().await.expect("database");
        lane(&pool, "ledger", "/code/ledger").await;
        row(&pool, "ledger", "/code/ledger").await;
        let source = FakeSource::new(vec![("ledger", Answer::Pr("open"))]);
        pass(&pool, &source, &day(1)).await.expect("seed observation");
        let report = pass(&pool, &source, &day(1)).await.expect("unchanged pass");
        let frames = std::cell::RefCell::new(Vec::new());
        publish_changes(&pool, &report, &day(1), &|model| frames.borrow_mut().push(model.clone()))
            .await.expect("publication");
        assert_eq!(report.prs.recorded, 0);
        assert_eq!(report.settle.settled(), 0);
        assert!(frames.borrow().is_empty(), "no callback for an unchanged pass");
    }

    /// Without this test a PR-only change could miss publication or carry a lane key instead
    /// of the managed owner's worktree identity (the render group need not be the owner).
    #[tokio::test]
    async fn a_pr_change_publishes_once_with_the_changed_row_identity() {
        let pool = crate::storage::memory().await.expect("database");
        lane(&pool, "ledger", "/code/ledger").await;
        lifecycle::apply(&pool, "w:owner/ledger", "owner", "/code/ledger", &LifecycleOp::Activate, None, ANCHOR)
            .await.expect("managed owner row");
        let source = FakeSource::new(vec![("ledger", Answer::Pr("open"))]);
        let report = pass(&pool, &source, &day(1)).await.expect("changed pass");
        let frames = std::cell::RefCell::new(Vec::new());
        publish_changes(&pool, &report, &day(1), &|model| frames.borrow_mut().push(model.clone()))
            .await.expect("publication");
        let frames = frames.into_inner();
        assert_eq!(frames.len(), 1);
        let row = &frames[0].sections[0].rows[0];
        assert_eq!(row.worktree_key, "w:owner/ledger");
        assert_eq!(row.pull_request.as_ref().map(|pr| pr.state.as_str()), Some("open"));
        assert_eq!(row.effective_state, "active");
        // The frame is the same durable read a client that missed it gets, not transient state.
        assert_eq!(frames[0], lifecycle::model(&pool, &day(1)).await.expect("reconnect read"));
    }

    /// Without this test a settle-only pass (no PR changed) could stay silent, or plain-folder
    /// settlements could be published under an invented identity instead of the persisted key.
    #[tokio::test]
    async fn a_settle_without_a_pr_change_publishes_the_settlement() {
        let pool = crate::storage::memory().await.expect("database");
        let key = lifecycle::worktree_key("owner", "C:\\code\\folder\\", None);
        lifecycle::apply(&pool, &key, "owner", "C:\\code\\folder\\", &LifecycleOp::Activate, None, ANCHOR)
            .await.expect("plain folder row");
        let source = FakeSource::new(Vec::new());
        let report = pass(&pool, &source, &day(8)).await.expect("settle pass");
        let frames = std::cell::RefCell::new(Vec::new());
        publish_changes(&pool, &report, &day(8), &|model| frames.borrow_mut().push(model.clone()))
            .await.expect("publication");
        let frames = frames.into_inner();
        assert_eq!(report.prs.recorded, 0);
        assert_eq!(frames.len(), 1);
        let row = &frames[0].sections[0].rows[0];
        assert_eq!(row.worktree_key, "p:owner/C:/code/folder");
        assert_eq!(row.effective_state, "settled");
        assert_eq!(row.settle_reason.as_deref(), Some(lane_settle::REASON_INACTIVITY));
    }

    /// Without this test the loop could forget to invoke its callback even while its helper
    /// tests passed. Subscribe before starting it and await the callback, never a guessed delay.
    #[tokio::test]
    async fn serve_publishes_a_merge_and_its_settlement_in_the_same_frame() {
        let pool = crate::storage::memory().await.expect("database");
        lane(&pool, "ledger", "/code/ledger").await;
        row(&pool, "ledger", "/code/ledger").await;
        let open = FakeSource::new(vec![("ledger", Answer::Pr("open"))]);
        sync(&pool, &open, &targets(&pool).await.expect("targets")).await.expect("open PR");
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let worker = tokio::spawn(serve(
            pool.clone(),
            Arc::new(FakeSource::new(vec![("ledger", Answer::Pr("merged"))])),
            SYNC_EVERY,
            move |model| { tx.send(model.clone()).expect("callback receiver"); },
        ));
        let received = tokio::time::timeout(Duration::from_secs(5), rx.recv()).await;
        worker.abort();
        assert!(worker.await.expect_err("aborted generation task").is_cancelled());
        let model = received.expect("bounded callback").expect("one publication");
        let row = &model.sections[0].rows[0];
        assert_eq!(row.worktree_key, "w:ferryx/ledger");
        assert_eq!(row.pull_request.as_ref().map(|pr| pr.state.as_str()), Some("merged"));
        assert_eq!(row.settle_reason.as_deref(), Some(lane_settle::REASON_PR_MERGED));
        assert_eq!(row.effective_state, "settled");
        assert!(rx.try_recv().is_err(), "one pass publishes exactly once");
        assert_eq!(model, lifecycle::model(&pool, &now_rfc3339()).await.expect("durable read"));
    }

    /// Ended lanes are not polled; every other lane is, and a lane with no directory is skipped
    /// rather than read from the process's own working directory.
    ///
    /// Fails if the target query loses its status filter (a closed lane would be polled) or its
    /// empty-`cwd` filter (a lane with no directory would be read from wherever the hub runs).
    #[tokio::test]
    async fn only_unended_lanes_with_a_directory_are_polled() {
        let pool = crate::storage::memory().await.unwrap();
        lane(&pool, "live", "/code/live").await;
        lane(&pool, "ended", "/code/ended").await;
        lane(&pool, "nowhere", "").await;
        sqlx::query("UPDATE lanes SET status = 'closed' WHERE key = 'ended'")
            .execute(&pool)
            .await
            .unwrap();
        let polled: Vec<String> = targets(&pool)
            .await
            .unwrap()
            .into_iter()
            .map(|target| target.lane_key)
            .collect();
        assert_eq!(polled, ["live"]);
    }
}
