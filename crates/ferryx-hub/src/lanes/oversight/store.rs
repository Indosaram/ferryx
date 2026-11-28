//! Persistence of lane oversight: the run row, its deadline, usage totals and status.
use sqlx::{SqliteConnection, SqlitePool};

use super::{deadline, Backend, Ceilings, Snapshot, Total, Totals};
use crate::error::Result;
use crate::storage::WriteTx;

/// A persisted oversight row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row {
    pub run: i64,
    pub backend: Backend,
    pub ended: bool,
    pub deadline_at_ms: Option<i64>,
    pub snapshot: Snapshot,
}

type RawRow = (i64, String, i64, i64, i64, i64, i64, i64, i64, Option<i64>, Option<i64>, Option<i64>, Option<i64>);

const ROW_SQL: &str = "SELECT run, backend, ended, run_started_at_ms, last_progress_at_ms, reminders_sent,
    stall_alerted, ceiling_reported, waiting_on_owner, deadline_at_ms, wall_ceiling_min, token_ceiling,
    cost_ceiling_micros FROM lane_oversight WHERE lane_key = ?";

fn row_of(raw: RawRow) -> Row {
    let (run, backend, ended, started, progress, reminders, stall, ceiling, waiting, deadline_at, wall, tokens, cost) = raw;
    Row {
        run,
        backend: Backend::parse(&backend),
        ended: ended != 0,
        deadline_at_ms: deadline_at,
        snapshot: Snapshot {
            run_started_at_ms: started,
            last_progress_at_ms: progress,
            reminders_sent: reminders,
            stall_alerted: stall != 0,
            ceiling_reported: ceiling != 0,
            waiting_on_owner: waiting != 0,
            ceilings: Ceilings { wall_minutes: wall, tokens, cost_micros: cost },
        },
    }
}

pub(super) async fn load_in(conn: &mut SqliteConnection, lane: &str) -> sqlx::Result<Option<Row>> {
    let raw: Option<RawRow> = sqlx::query_as(ROW_SQL).bind(lane).fetch_optional(&mut *conn).await?;
    Ok(raw.map(row_of))
}

/// The current run of `lane`, creating run 1 with the default ceilings on first sight.
pub async fn ensure_run_in(conn: &mut SqliteConnection, lane: &str, now_ms: i64) -> sqlx::Result<i64> {
    sqlx::query(
        "INSERT INTO lane_oversight(lane_key, run, run_started_at_ms, last_progress_at_ms) VALUES(?, 1, ?, ?)
         ON CONFLICT(lane_key) DO NOTHING",
    )
    .bind(lane).bind(now_ms).bind(now_ms)
    .execute(&mut *conn)
    .await?;
    sqlx::query_scalar("SELECT run FROM lane_oversight WHERE lane_key = ?").bind(lane).fetch_one(&mut *conn).await
}

/// Starts a new run of `lane` with the ceilings set at open: a fresh quiet clock and evidence set.
pub async fn start_run(pool: &SqlitePool, lane: &str, ceilings: Ceilings, now_ms: i64) -> Result<i64> {
    let mut tx = pool.begin_write().await?;
    let run: i64 = sqlx::query_scalar(
        "INSERT INTO lane_oversight(lane_key, run, run_started_at_ms, last_progress_at_ms, wall_ceiling_min, token_ceiling, cost_ceiling_micros)
         VALUES(?, 1, ?, ?, ?, ?, ?)
         ON CONFLICT(lane_key) DO UPDATE SET run = run + 1, run_started_at_ms = excluded.run_started_at_ms,
             last_progress_at_ms = excluded.last_progress_at_ms, reminders_sent = 0, stall_alerted = 0,
             ceiling_reported = 0, waiting_on_owner = 0, ended = 0, deadline_at_ms = NULL,
             wall_ceiling_min = excluded.wall_ceiling_min, token_ceiling = excluded.token_ceiling,
             cost_ceiling_micros = excluded.cost_ceiling_micros
         RETURNING run",
    )
    .bind(lane).bind(now_ms).bind(now_ms)
    .bind(ceilings.wall_minutes).bind(ceilings.tokens).bind(ceilings.cost_micros)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    super::poke();
    Ok(run)
}

/// Applies a report: new evidence restarts the quiet clock and re-arms reminders and the stall
/// alert; `waiting` marks the lane as waiting on the owner until the next report; `stopped` and
/// `finished` end oversight of the run. Recomputes the lane's one deadline. Returns the run's
/// last progress time.
pub async fn note_report_in(
    conn: &mut SqliteConnection,
    lane: &str,
    kind: crate::lanes::evidence::ReportKind,
    progress: bool,
    now_ms: i64,
) -> sqlx::Result<i64> {
    use crate::lanes::evidence::ReportKind;
    let waiting = kind == ReportKind::Waiting;
    let ended = matches!(kind, ReportKind::Stopped | ReportKind::Finished);
    if progress {
        sqlx::query(
            "UPDATE lane_oversight SET last_progress_at_ms = ?, reminders_sent = 0, stall_alerted = 0 WHERE lane_key = ?",
        )
        .bind(now_ms).bind(lane)
        .execute(&mut *conn)
        .await?;
    }
    sqlx::query("UPDATE lane_oversight SET waiting_on_owner = ?, ended = ? WHERE lane_key = ?")
        .bind(waiting).bind(ended).bind(lane)
        .execute(&mut *conn)
        .await?;
    let row = load_in(conn, lane).await?.ok_or(sqlx::Error::RowNotFound)?;
    store_deadline_in(conn, lane, &row, now_ms).await?;
    Ok(row.snapshot.last_progress_at_ms)
}

pub(super) async fn store_deadline_in(conn: &mut SqliteConnection, lane: &str, row: &Row, now_ms: i64) -> sqlx::Result<Option<i64>> {
    let at = if row.ended { None } else { deadline(&row.snapshot, row.backend, now_ms) };
    sqlx::query("UPDATE lane_oversight SET deadline_at_ms = ? WHERE lane_key = ?")
        .bind(at).bind(lane)
        .execute(&mut *conn)
        .await?;
    Ok(at)
}

/// Usage totals of a run. No record, or any record without the figure, is unknown.
pub async fn totals(pool: &SqlitePool, lane: &str, run: i64) -> Result<Totals> {
    let (rows, tokens_known, tokens, cost_known, cost): (i64, i64, Option<i64>, i64, Option<i64>) = sqlx::query_as(
        "SELECT COUNT(*), COUNT(tokens), SUM(tokens), COUNT(cost_micros), SUM(cost_micros)
           FROM lane_usage WHERE lane_key = ? AND run = ?",
    )
    .bind(lane).bind(run)
    .fetch_one(pool)
    .await?;
    let total = |known: i64, sum: Option<i64>| match sum {
        Some(sum) if rows > 0 && known == rows => Total::Known(sum),
        _ => Total::Unknown,
    };
    Ok(Totals { tokens: total(tokens_known, tokens), cost_micros: total(cost_known, cost) })
}

/// Records a worker usage figure for the lane's current run. `None` is an unknown figure.
pub async fn record_usage(pool: &SqlitePool, lane: &str, tokens: Option<i64>, cost_micros: Option<i64>, now_ms: i64) -> Result<()> {
    let mut tx = pool.begin_write().await?;
    let run = ensure_run_in(&mut tx, lane, now_ms).await?;
    sqlx::query("INSERT INTO lane_usage(lane_key, run, tokens, cost_micros, recorded_at_ms) VALUES(?,?,?,?,?)")
        .bind(lane).bind(run).bind(tokens).bind(cost_micros).bind(now_ms)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    super::poke();
    Ok(())
}

/// The oversight status of `lane`, or `None` before its first run.
pub async fn status(pool: &SqlitePool, lane: &str, now_ms: i64) -> Result<Option<super::OversightStatus>> {
    let mut conn = pool.acquire().await?;
    let Some(row) = load_in(&mut conn, lane).await? else { return Ok(None) };
    drop(conn);
    let totals = totals(pool, lane, row.run).await?;
    Ok(Some(super::status_of(&row, &totals, now_ms)))
}

