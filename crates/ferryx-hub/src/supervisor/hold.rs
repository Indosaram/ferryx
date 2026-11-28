//! Confirmed hub-loop holds. Native worker backends must provide their own readback.
use sqlx::SqlitePool;

use super::panel_contract::SessionRef;
use super::request::{RequestContext, RequestFailure};
use super::{execution_binding_of, ExecutionKind, RunState, SupervisorGoal};
use crate::storage::WriteTx;

pub const HOLD_UNCONFIRMED: &str = "HOLD_UNCONFIRMED";
pub const QUIET_KEY: &str = "supervisor_hold_released:";
/// `hub_meta` prefix of a held session whose current working stretch was already surfaced.
pub const ACTIVE_KEY: &str = "supervisor_hold_active:";
/// `lane_events` kind of that surfacing: the lead's once-per-active-stretch notice.
pub const HELD_LANE_ACTIVE: &str = "held_lane_active";

/// Whether a Slack thread owns at least one active (`working` or `waiting`) lane.
pub async fn thread_has_active_lane(
    pool: &SqlitePool,
    channel: &str,
    thread_ts: &str,
) -> crate::error::Result<bool> {
    let thread = crate::lanes::threads::thread_ref(channel, thread_ts);
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT EXISTS(SELECT 1 FROM lanes WHERE thread_channel_id = ? AND status IN ('working', 'waiting'))",
    )
    .bind(thread)
    .fetch_one(pool)
    .await?
        != 0)
}

/// Owner command over every working lane rooted in one Slack thread.
pub async fn slack_thread_command(
    pool: &SqlitePool,
    channel: &str,
    thread_ts: &str,
    owner: &str,
) -> Result<Vec<String>, RequestFailure> {
    let thread = crate::lanes::threads::thread_ref(channel, thread_ts);
    let mut tx = pool.begin_write().await.map_err(|error| RequestFailure::new("STORAGE", error.to_string()))?;
    let lanes: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT key, status, lead_session_key FROM lanes WHERE thread_channel_id = ? AND status IN ('working', 'waiting') ORDER BY key",
    ).bind(&thread).fetch_all(&mut *tx).await.map_err(|error| RequestFailure::new("STORAGE", error.to_string()))?;
    let mut changed = Vec::new();
    for (key, _status, lead) in lanes {
        if crate::lanes::pause::pause_in(&mut tx, &key, owner).await.map_err(|error| RequestFailure::new(HOLD_UNCONFIRMED, format!("not held: {error}")))? {
            changed.push(format!("{key}:{lead}"));
        }
    }
    tx.commit().await.map_err(|error| RequestFailure::new("STORAGE", error.to_string()))?;
    Ok(changed)
}

/// Holds one lane on the hub-loop fence (oversight ceilings). The fence is read back inside the
/// same transaction ([`crate::lanes::pause::pause_in`]); a failed read-back is `HOLD_UNCONFIRMED`
/// and changes nothing. `Ok(false)` when the lane was already held. Only an explicit continue
/// releases it.
pub async fn hold_lane(pool: &SqlitePool, lane: &str, by: &str) -> Result<bool, RequestFailure> {
    let mut tx = pool.begin_write().await?;
    let changed = crate::lanes::pause::pause_in(&mut tx, lane, by)
        .await
        .map_err(|error| RequestFailure::new(HOLD_UNCONFIRMED, format!("not held: {error}")))?;
    tx.commit().await?;
    super::wakes::poke();
    Ok(changed)
}

/// Confirm the loop fence, preserve the interrupted state, then interrupt the live turn.
pub async fn confirm(
    pool: &SqlitePool,
    context: &RequestContext,
    goal: &SupervisorGoal,
) -> Result<(), RequestFailure> {
    let binding = execution_binding_of(pool, &goal.goal_id).await?;
    if let Some(binding) = &binding {
        match binding.kind {
            ExecutionKind::Mhc => {
                return Err(RequestFailure::new(HOLD_UNCONFIRMED,
                    "not held: native goal hold/readback capability is unavailable"));
            }
            ExecutionKind::Pty => {}
        }
    }
    let mut tx = pool.begin_write().await?;
    if let Err(error) = crate::lanes::pause::pause_in(&mut tx, &goal.lane_key, context.adapter.as_str()).await {
        return Err(RequestFailure::new(HOLD_UNCONFIRMED, format!("not held: {error}")));
    }
    super::transition_goal_in(
        &mut tx, &goal.goal_id, goal.revision, goal.run_state, RunState::Paused,
        Some(super::request::OWNER_PAUSED),
    ).await?;
    sqlx::query("UPDATE supervisor_goals SET held_from = ?, held_detail = ? WHERE goal_id = ?")
        .bind(goal.run_state.as_str()).bind(&goal.state_detail).bind(&goal.goal_id)
        .execute(&mut *tx).await?;
    tx.commit().await?;
    if let (Some(control), Some(binding)) = (&context.control, binding) {
        if let Some(session_id) = binding.provider_session_id {
            if goal.run_state == RunState::Running {
                if let Err(error) = control.execute(Some(&binding.host_id), crate::agent_control::AgentOp::Stop {
                    session_id, request_id: format!("{}-hold-r{}", goal.goal_id, goal.revision),
                }).await {
                    // The confirmed loop fence remains set even when interruption fails.
                    tracing::warn!(%error, goal = %goal.goal_id, "held worker interruption failed");
                }
            }
        }
    }
    super::wakes::poke();
    Ok(())
}

/// Host-qualified bindings prevent one host's held pane from blocking a different host.
pub async fn session_held(pool: &SqlitePool, session: &SessionRef) -> crate::error::Result<bool> {
    let bound: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM supervisor_execution_bindings b
         JOIN supervisor_goals g ON g.goal_id = b.goal_id
         WHERE b.host_id = ? AND b.provider_session_id = ?
           AND (g.run_state = 'paused' OR EXISTS(SELECT 1 FROM lane_pauses p
                WHERE p.lane_key = g.lane_key AND p.resumed_at IS NULL)))",
    )
    .bind(session.host_id())
    .bind(session.backend_session_id())
    .fetch_one(pool)
    .await?;
    if bound {
        return Ok(true);
    }
    if session.host_id() != "local" {
        return Ok(false);
    }
    let lane_paused: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM lanes l
         JOIN lane_pauses p ON p.lane_key = l.key
         WHERE (l.omo_thread_id = ? OR l.lead_session_key = ? OR l.session_key = ?)
           AND p.resumed_at IS NULL)",
    )
    .bind(session.backend_session_id())
    .bind(session.backend_session_id())
    .bind(session.backend_session_id())
    .fetch_one(pool)
    .await?;
    Ok(lane_paused)
}

pub async fn lane_held(pool: &SqlitePool, lane: &str) -> crate::error::Result<bool> {
    Ok(crate::lanes::pause::is_paused(pool, lane).await? || sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM supervisor_goals WHERE lane_key = ? AND run_state = 'paused')",
    ).bind(lane).fetch_one(pool).await?)
}

/// Release only explicitly held lanes in a Slack thread; each release resets its quiet clock
/// and its active-stretch marker, so a lane held again alerts its lead again.
pub async fn resume_slack_thread(pool: &SqlitePool, channel: &str, thread_ts: &str) -> crate::error::Result<Vec<String>> {
    let thread = crate::lanes::threads::thread_ref(channel, thread_ts);
    let keys: Vec<String> = sqlx::query_scalar("SELECT l.key FROM lanes l JOIN lane_pauses p ON p.lane_key=l.key AND p.resumed_at IS NULL WHERE l.thread_channel_id=? AND l.status IN ('working','waiting') ORDER BY l.key")
        .bind(thread).fetch_all(pool).await?;
    let mut resumed = Vec::new();
    for key in keys {
        if crate::lanes::pause::resume(pool, &key).await? {
            let mut tx = pool.begin_write().await?;
            clear_active_in(&mut tx, &key).await?;
            tx.commit().await?;
            resumed.push(key);
        }
    }
    Ok(resumed)
}

/// Once per active stretch, surface a held worker that starts independently. Never release it.
/// The lead hears it through the lane's own event queue, so a held lane that comes back to life
/// on its own is news exactly once per stretch rather than a log line nobody reads.
pub async fn observe(pool: &SqlitePool, session: &super::panel_contract::FleetSession) -> crate::error::Result<bool> {
    if !session_held(pool, &session.session_ref).await? { return Ok(false); }
    let key = format!("{ACTIVE_KEY}{}", session.session_ref);
    let mut tx = pool.begin_write().await?;
    if session.state != "working" {
        sqlx::query("DELETE FROM hub_meta WHERE key = ?").bind(key).execute(&mut *tx).await?;
        tx.commit().await?;
        return Ok(false);
    }
    let inserted = sqlx::query("INSERT INTO hub_meta(key, value) VALUES(?, '1') ON CONFLICT(key) DO NOTHING")
        .bind(&key).execute(&mut *tx).await?;
    if inserted.rows_affected() == 0 {
        tx.commit().await?;
        return Ok(false);
    }
    tracing::warn!(session = %session.session_ref, "held worker started independently; hold remains active");
    if let Some((lane, lead)) = held_lane_of(&mut tx, &session.session_ref).await? {
        sqlx::query("INSERT INTO lane_events(lane_key, lead_session_key, kind, body, created_at) VALUES(?,?,?,?,?)")
            .bind(&lane).bind(&lead).bind(HELD_LANE_ACTIVE)
            .bind(format!(
                "held lane {lane} started working independently (session {}); the hold remains active",
                session.session_ref
            ))
            .bind(crate::storage::now_rfc3339())
            .execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(true)
}

/// The lane a held session belongs to and the lead its events go to: the session's own goal
/// binding first, else the local lane it leads or belongs to.
async fn held_lane_of(
    conn: &mut sqlx::SqliteConnection,
    session: &SessionRef,
) -> crate::error::Result<Option<(String, String)>> {
    let bound: Option<(String, String)> = sqlx::query_as(
        "SELECT g.lane_key, l.lead_session_key FROM supervisor_execution_bindings b
           JOIN supervisor_goals g ON g.goal_id = b.goal_id
           JOIN lanes l ON l.key = g.lane_key
          WHERE b.host_id = ? AND b.provider_session_id = ?
          ORDER BY g.updated_at DESC LIMIT 1",
    )
    .bind(session.host_id())
    .bind(session.backend_session_id())
    .fetch_optional(&mut *conn)
    .await?;
    if bound.is_some() || session.host_id() != "local" {
        return Ok(bound);
    }
    Ok(sqlx::query_as(
        "SELECT l.key, l.lead_session_key FROM lanes l
          WHERE l.omo_thread_id = ? OR l.lead_session_key = ? OR l.session_key = ?
          ORDER BY l.key LIMIT 1",
    )
    .bind(session.backend_session_id())
    .bind(session.backend_session_id())
    .bind(session.backend_session_id())
    .fetch_optional(&mut *conn)
    .await?)
}

/// Clears the active-stretch markers of every session of `lane_key` inside the caller's
/// transaction: a lane held again starts a fresh stretch, and its lead hears it again.
pub(crate) async fn clear_active_in(
    conn: &mut sqlx::SqliteConnection,
    lane_key: &str,
) -> sqlx::Result<()> {
    let bound: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT b.host_id, b.provider_session_id FROM supervisor_execution_bindings b
           JOIN supervisor_goals g ON g.goal_id = b.goal_id
          WHERE g.lane_key = ? AND b.provider_session_id IS NOT NULL",
    )
    .bind(lane_key)
    .fetch_all(&mut *conn)
    .await?;
    let mut keys: Vec<String> = bound
        .into_iter()
        .filter_map(|(host, session)| {
            session.map(|session| format!("{ACTIVE_KEY}{}", SessionRef::new(&host, &session)))
        })
        .collect();
    let lane: Option<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT lead_session_key, session_key, omo_thread_id FROM lanes WHERE key = ?",
    )
    .bind(lane_key)
    .fetch_optional(&mut *conn)
    .await?;
    if let Some((lead, session_key, thread)) = lane {
        for id in [Some(lead), Some(session_key), thread].into_iter().flatten() {
            keys.push(format!("{ACTIVE_KEY}{}", SessionRef::new("local", &id)));
        }
    }
    for key in keys {
        sqlx::query("DELETE FROM hub_meta WHERE key = ?")
            .bind(key)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}
