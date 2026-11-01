//! Reversible cutover from the legacy supervision intake to the unified supervisor (todo 16).
//!
//! Two intakes can supervise an owner's work. The legacy intake is the omo app-server owner
//! turn, the legacy lane goal loop and the kibitzer lead wake. The unified intake is admission,
//! the dispatcher and the wake scheduler. They must never both take the same owner's work, so
//! the persisted row in `supervisor_cutover` (migration 0025) refuses both open at once.
//!
//! - **Upgrade.** A hub that held legacy work before the unified supervisor could run it (lanes
//!   or chat sessions at migration time, or an omo import, recorded in
//!   `supervisor_legacy_history`) starts with the legacy intake open and the unified intake
//!   closed. Upgrading schedules nothing: no goal is admitted or dispatched, no wake is
//!   delivered, and no legacy lane is revived.
//! - **Fresh install.** A hub without legacy history has nothing to stop: the unified intake is
//!   open and the legacy intake is idle (it never had work).
//! - **Cutover** ([`cutover`]) is refused unless the installed mhc engine is ready with every
//!   required capability ([`require_engine`]). It commits the legacy stop first and opens the
//!   unified intake in a later commit, so there is no instant when both are open.
//! - **Rollback** ([`rollback`]) closes the unified intake first (no new admission, claim or
//!   wake delivery), then reopens the legacy intake. It deletes nothing: goals, dispatches,
//!   receipts and wakes stay, and no pane is touched. A later cutover opens the unified
//!   intake again over the same rows.
//!
//! Reads never write: the row is created by the first cutover or rollback.
//!
//! - **Automatic cutover** ([`auto_cutover`]) runs the same cutover once setup finishes, when
//!   the owner opted in (`hub_meta.supervisor_auto_cutover = 1`), the upgraded installation
//!   has exactly one identity, the owner never cut over or rolled back (no row: an explicit
//!   rollback is never overridden), no legacy goal turn is pending or sent, and the engine is
//!   ready. Otherwise it changes nothing and [`auto_cutover_blocker`] names why.
use serde::Serialize;
use sqlx::{SqliteConnection, SqlitePool};

use super::{SupervisorError, SupervisorIdentity};
use crate::meta::mhc_profile::REQUIRED_CAPABILITIES;
use crate::meta::mhc_supervisor::{HealthState, SupervisorHealth};
use crate::storage::{now_rfc3339, WriteTx};

/// Wire code of [`SupervisorError::IntakeClosed`]: new unified work is refused (503-class).
pub const SUPERVISOR_INTAKE_CLOSED: &str = "SUPERVISOR_INTAKE_CLOSED";

/// `hub_meta` key: `1` when the owner opted in to the automatic cutover after setup.
pub const AUTO_CUTOVER_KEY: &str = "supervisor_auto_cutover";

/// Why [`auto_cutover`] does nothing now; an engine refusal is the engine's own code instead.
pub const AUTO_CUTOVER_OFF: &str = "AUTO_CUTOVER_OFF";
pub const AUTO_CUTOVER_NO_OWNER: &str = "AUTO_CUTOVER_NO_OWNER";
pub const AUTO_CUTOVER_OWNER_DECIDED: &str = "AUTO_CUTOVER_OWNER_DECIDED";
pub const AUTO_CUTOVER_NO_LEGACY: &str = "AUTO_CUTOVER_NO_LEGACY";
pub const AUTO_CUTOVER_LEGACY_TURN: &str = "AUTO_CUTOVER_LEGACY_TURN";

/// The legacy intake as the installation sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyIntake {
    /// It takes the owner's work (an upgrade before cutover, or after a rollback).
    Open,
    /// A cutover stopped it.
    Stopped,
    /// A fresh install: there is no legacy work to take and nothing to stop.
    Idle,
}

/// Where the current intake came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Fresh,
    Upgrade,
    Cutover,
    Rollback,
}

impl Origin {
    fn as_str(self) -> &'static str {
        match self {
            Origin::Fresh => "fresh",
            Origin::Upgrade => "upgrade",
            Origin::Cutover => "cutover",
            Origin::Rollback => "rollback",
        }
    }

    fn parse(text: &str) -> Self {
        match text {
            "cutover" => Origin::Cutover,
            "rollback" => Origin::Rollback,
            "upgrade" => Origin::Upgrade,
            _ => Origin::Fresh,
        }
    }
}

/// Which intake takes the owner's work now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Intake {
    pub supervisor_id: Option<String>,
    pub legacy: LegacyIntake,
    pub unified_open: bool,
    pub origin: Origin,
    /// True when the hub held legacy work at upgrade or import.
    pub legacy_history: bool,
}

impl Intake {
    /// The legacy paths may take work (anything but an explicit cutover stop).
    pub fn legacy_runs(&self) -> bool {
        self.legacy != LegacyIntake::Stopped
    }
}

async fn legacy_history(conn: &mut SqliteConnection) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM supervisor_legacy_history)")
        .fetch_one(conn)
        .await
}

/// The intake of `supervisor_id`, read without writing.
pub async fn intake_in(
    conn: &mut SqliteConnection,
    supervisor_id: &str,
) -> Result<Intake, sqlx::Error> {
    let history = legacy_history(conn).await?;
    let row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT legacy_intake, unified_intake, origin FROM supervisor_cutover WHERE supervisor_id = ?",
    )
    .bind(supervisor_id)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(match row {
        Some((legacy, unified, origin)) => Intake {
            supervisor_id: Some(supervisor_id.to_owned()),
            legacy: if legacy == "stopped" {
                LegacyIntake::Stopped
            } else {
                LegacyIntake::Open
            },
            unified_open: unified == "open",
            origin: Origin::parse(&origin),
            legacy_history: history,
        },
        None => default_intake(Some(supervisor_id.to_owned()), history),
    })
}

fn default_intake(supervisor_id: Option<String>, history: bool) -> Intake {
    if history {
        Intake {
            supervisor_id,
            legacy: LegacyIntake::Open,
            unified_open: false,
            origin: Origin::Upgrade,
            legacy_history: true,
        }
    } else {
        Intake {
            supervisor_id,
            legacy: LegacyIntake::Idle,
            unified_open: true,
            origin: Origin::Fresh,
            legacy_history: false,
        }
    }
}

/// The installation's intake: that of its one supervisor identity, or, before set-up, what an
/// identity would start with.
pub async fn installation_intake(pool: &SqlitePool) -> Result<Intake, sqlx::Error> {
    let mut conn = pool.acquire().await?;
    let supervisor: Option<String> =
        sqlx::query_scalar("SELECT supervisor_id FROM supervisor_identity ORDER BY created_at LIMIT 1")
            .fetch_optional(&mut *conn)
            .await?;
    match supervisor {
        Some(supervisor_id) => intake_in(&mut conn, &supervisor_id).await,
        None => {
            let history = legacy_history(&mut conn).await?;
            Ok(default_intake(None, history))
        }
    }
}

/// True unless a cutover stopped the legacy intake. The legacy goal loop, the legacy approval
/// send and the kibitzer lead wake check it before they act.
pub async fn legacy_intake_runs(pool: &SqlitePool) -> Result<bool, sqlx::Error> {
    Ok(installation_intake(pool).await?.legacy_runs())
}

/// Refuses new unified work for `supervisor_id` while its unified intake is closed. Runs under
/// the caller's write lock (admission, claim), so a rollback commit and the write it gates are
/// ordered: one of them sees the other.
pub async fn require_unified_intake(
    conn: &mut SqliteConnection,
    supervisor_id: &str,
) -> Result<(), SupervisorError> {
    let intake = intake_in(conn, supervisor_id).await?;
    if intake.unified_open {
        return Ok(());
    }
    let why = match intake.origin {
        Origin::Rollback => "a rollback closed the unified supervisor intake; the legacy intake takes the owner's work until the next cutover",
        _ => "this hub was upgraded with legacy work and has not been cut over; run `ferryx hub cutover` once the mhc supervisor engine is ready",
    };
    Err(SupervisorError::IntakeClosed(why.to_owned()))
}

/// The unified intake of the goal's owner is open.
pub async fn unified_open_for(pool: &SqlitePool, supervisor_id: &str) -> Result<bool, sqlx::Error> {
    let mut conn = pool.acquire().await?;
    Ok(intake_in(&mut conn, supervisor_id).await?.unified_open)
}

/// Refuses new autonomy unless `health` is a ready mhc engine that advertises every required
/// capability. The error names the engine's own fault code (`MHC_CAPABILITY_MISSING`,
/// `MHC_NOT_RUNNING`, ...); there is no fallback engine and nothing else is consulted.
pub fn require_engine(health: Option<&SupervisorHealth>) -> Result<(), SupervisorError> {
    let Some(health) = health else {
        return Err(SupervisorError::EngineUnavailable {
            code: "MHC_NOT_RUNNING",
            detail: "no mhc supervisor engine is installed in this daemon (FERRYX_HUB_MHC_SUPERVISOR is off)".into(),
        });
    };
    if health.state != HealthState::Ready {
        let (code, detail) = match &health.fault {
            Some(fault) => (fault.code(), fault.to_string()),
            None => (
                "MHC_NOT_RUNNING",
                format!("the mhc supervisor engine is {:?}", health.state).to_lowercase(),
            ),
        };
        return Err(SupervisorError::EngineUnavailable { code, detail });
    }
    let missing: Vec<&str> = REQUIRED_CAPABILITIES
        .iter()
        .copied()
        .filter(|capability| !health.capabilities.iter().any(|have| have == capability))
        .collect();
    if !missing.is_empty() {
        return Err(SupervisorError::EngineUnavailable {
            code: "MHC_CAPABILITY_MISSING",
            detail: format!("the mhc supervisor engine does not advertise {}", missing.join(", ")),
        });
    }
    Ok(())
}

/// What a cutover or rollback did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Transition {
    pub intake: Intake,
    /// The steps committed by this call, in commit order. Empty when nothing had to change.
    pub steps: Vec<&'static str>,
}

async fn write_row(
    conn: &mut SqliteConnection,
    supervisor_id: &str,
    legacy: &str,
    unified: &str,
    origin: Origin,
    step: &'static str,
    detail: &str,
) -> Result<(), sqlx::Error> {
    let now = now_rfc3339();
    sqlx::query(
        "INSERT INTO supervisor_cutover(supervisor_id, legacy_intake, unified_intake, origin, updated_at)
         VALUES(?,?,?,?,?)
         ON CONFLICT(supervisor_id) DO UPDATE SET legacy_intake = excluded.legacy_intake,
             unified_intake = excluded.unified_intake, origin = excluded.origin,
             updated_at = excluded.updated_at",
    )
    .bind(supervisor_id)
    .bind(legacy)
    .bind(unified)
    .bind(origin.as_str())
    .bind(&now)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "INSERT INTO supervisor_cutover_log(supervisor_id, step, detail, created_at) VALUES(?,?,?,?)",
    )
    .bind(supervisor_id)
    .bind(step)
    .bind(detail)
    .bind(&now)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

async fn the_identity(pool: &SqlitePool) -> Result<SupervisorIdentity, SupervisorError> {
    let owner = super::admission::installation_owner(pool)
        .await
        .map_err(|error| SupervisorError::Invalid(error.to_string()))?;
    super::identity_of(pool, &owner)
        .await?
        .ok_or_else(|| SupervisorError::NotFound {
            kind: "supervisor identity",
            id: owner,
        })
}

/// Moves the installation's owner to the unified intake. Refused, with nothing changed, unless
/// `health` passes [`require_engine`]. The legacy stop commits first; the unified intake opens
/// in a second commit, so no reader ever sees both open. Running it again is a no-op.
pub async fn cutover(
    pool: &SqlitePool,
    health: Option<&SupervisorHealth>,
) -> Result<Transition, SupervisorError> {
    match cutover_with(pool, health, false).await? {
        Some(transition) => Ok(transition),
        // Only an automatic cutover stops early; report the unchanged intake regardless.
        None => Ok(Transition {
            intake: installation_intake(pool).await?,
            steps: Vec::new(),
        }),
    }
}

/// [`cutover`]; `automatic` additionally re-checks, under the first commit's write lock, that
/// the owner never decided (no row) and no legacy goal turn is pending or sent, answering
/// `None` with nothing written otherwise.
async fn cutover_with(
    pool: &SqlitePool,
    health: Option<&SupervisorHealth>,
    automatic: bool,
) -> Result<Option<Transition>, SupervisorError> {
    require_engine(health)?;
    let identity = the_identity(pool).await?;
    let id = identity.supervisor_id.as_str();
    let mut steps = Vec::new();
    let mut tx = pool.begin_write().await?;
    if automatic && (has_row(&mut tx, id).await? || legacy_turn_open(&mut tx).await?) {
        tx.commit().await?;
        return Ok(None);
    }
    let now = intake_in(&mut tx, id).await?;
    if now.legacy == LegacyIntake::Stopped && now.unified_open {
        tx.commit().await?;
        return Ok(Some(Transition { intake: now, steps }));
    }
    let stopped = if automatic {
        "automatic cutover after setup: legacy owner turns, the legacy goal loop and the kibitzer lead wake take no new work"
    } else {
        "legacy owner turns, the legacy goal loop and the kibitzer lead wake take no new work"
    };
    write_row(&mut tx, id, "stopped", "closed", Origin::Cutover, "legacy_intake_stopped", stopped)
        .await?;
    tx.commit().await?;
    steps.push("legacy_intake_stopped");

    let mut tx = pool.begin_write().await?;
    let enabled = if automatic {
        "automatic cutover after setup: the unified supervisor admits, dispatches and wakes"
    } else {
        "the unified supervisor admits, dispatches and wakes"
    };
    write_row(&mut tx, id, "stopped", "open", Origin::Cutover, "unified_intake_enabled", enabled)
        .await?;
    let intake = intake_in(&mut tx, id).await?;
    tx.commit().await?;
    steps.push("unified_intake_enabled");
    super::dispatcher::poke();
    super::wakes::poke();
    Ok(Some(Transition { intake, steps }))
}

async fn has_row(conn: &mut SqliteConnection, supervisor_id: &str) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM supervisor_cutover WHERE supervisor_id = ?)")
        .bind(supervisor_id)
        .fetch_one(conn)
        .await
}

/// A legacy goal turn is committed but not settled: cutting over now would strand it.
async fn legacy_turn_open(conn: &mut SqliteConnection) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM lane_goal WHERE proposal_state IN ('pending','sent'))",
    )
    .fetch_one(conn)
    .await
}

/// Whether the owner opted in to the automatic cutover.
pub async fn auto_cutover_enabled(pool: &SqlitePool) -> Result<bool, sqlx::Error> {
    let value: Option<String> = sqlx::query_scalar("SELECT value FROM hub_meta WHERE key = ?")
        .bind(AUTO_CUTOVER_KEY)
        .fetch_optional(pool)
        .await?;
    Ok(value.as_deref() == Some("1"))
}

/// Why [`auto_cutover`] would change nothing now, or `None` when it would cut over: one of the
/// `AUTO_CUTOVER_*` codes, or the engine's refusal code from [`require_engine`]
/// (`MHC_NOT_RUNNING`, `MHC_CAPABILITY_MISSING`, ...). Reads only.
pub async fn auto_cutover_blocker(
    pool: &SqlitePool,
    health: Option<&SupervisorHealth>,
) -> Result<Option<&'static str>, SupervisorError> {
    if !auto_cutover_enabled(pool).await? {
        return Ok(Some(AUTO_CUTOVER_OFF));
    }
    let mut conn = pool.acquire().await?;
    let owners: Vec<String> =
        sqlx::query_scalar("SELECT supervisor_id FROM supervisor_identity LIMIT 2")
            .fetch_all(&mut *conn)
            .await?;
    let [supervisor_id] = owners.as_slice() else {
        return Ok(Some(AUTO_CUTOVER_NO_OWNER));
    };
    if has_row(&mut conn, supervisor_id).await? {
        return Ok(Some(AUTO_CUTOVER_OWNER_DECIDED));
    }
    if !legacy_history(&mut conn).await? {
        return Ok(Some(AUTO_CUTOVER_NO_LEGACY));
    }
    if legacy_turn_open(&mut conn).await? {
        return Ok(Some(AUTO_CUTOVER_LEGACY_TURN));
    }
    drop(conn);
    Ok(require_engine(health).err().map(|error| error.code()))
}

/// The automatic cutover after setup: [`cutover`] when [`auto_cutover_blocker`] finds nothing
/// in the way, else `Ok(None)` with nothing written, so the caller retries on the next engine
/// health change. An owner's explicit cutover or rollback is never overridden.
pub async fn auto_cutover(
    pool: &SqlitePool,
    health: Option<&SupervisorHealth>,
) -> Result<Option<Transition>, SupervisorError> {
    if let Some(blocker) = auto_cutover_blocker(pool, health).await? {
        tracing::debug!(blocker, "automatic supervisor cutover is not due");
        return Ok(None);
    }
    let transition = cutover_with(pool, health, true).await?;
    if let Some(transition) = &transition {
        tracing::info!(steps = ?transition.steps, "automatic supervisor cutover after setup");
    }
    Ok(transition)
}

/// Moves the installation's owner back to the legacy intake. The unified intake closes first
/// (no new admission, claim or wake delivery), then the legacy intake reopens. Nothing is
/// deleted and no pane is touched: goals, dispatches, receipts and pending wakes stay for the
/// next cutover. Running it again is a no-op.
pub async fn rollback(pool: &SqlitePool) -> Result<Transition, SupervisorError> {
    let identity = the_identity(pool).await?;
    let id = identity.supervisor_id.as_str();
    let mut steps = Vec::new();
    let mut tx = pool.begin_write().await?;
    let now = intake_in(&mut tx, id).await?;
    if now.legacy == LegacyIntake::Open && !now.unified_open {
        tx.commit().await?;
        return Ok(Transition { intake: now, steps });
    }
    write_row(&mut tx, id, "stopped", "closed", Origin::Rollback, "unified_intake_closed",
        "no new admission, claim or wake delivery; receipts, goals and panes are kept").await?;
    tx.commit().await?;
    steps.push("unified_intake_closed");

    let mut tx = pool.begin_write().await?;
    write_row(&mut tx, id, "open", "closed", Origin::Rollback, "legacy_intake_reopened",
        "legacy owner turns, the legacy goal loop and the kibitzer lead wake take work again").await?;
    let intake = intake_in(&mut tx, id).await?;
    tx.commit().await?;
    steps.push("legacy_intake_reopened");
    Ok(Transition { intake, steps })
}

/// Records that this hub received legacy work by an omo import.
pub async fn note_import(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT OR IGNORE INTO supervisor_legacy_history(id, source, detected_at) VALUES(1, 'import', ?)",
    )
    .bind(now_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}
