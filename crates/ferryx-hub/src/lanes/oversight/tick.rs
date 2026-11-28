//! The effectful pass of lane oversight: read lane states, act on what is due, re-arm.
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use sqlx::SqlitePool;

use super::store::{ensure_run_in, load_in, store_deadline_in, totals};
use super::{decide, reminder_text, AgentState, Backend, Due};
use crate::agent_control::{AgentControl, AgentOp, LOCAL_HOST_ID};
use crate::error::Result;
use crate::storage::WriteTx;
use crate::supervisor::recovery::Clock;

/// Longest wait between passes when no deadline is armed: agent state changes are polled.
const POLL: Duration = Duration::from_secs(60);
const INVENTORY_TIMEOUT: Duration = Duration::from_secs(5);

/// A lane as the pass sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneRef {
    pub key: String,
    pub lead: String,
    pub pane: Option<String>,
    /// `None` is this machine.
    pub host: Option<String>,
}

/// Reads each lane's backend and agent state. A lane missing from the answer is unknown.
#[async_trait]
pub trait LaneStates: Send + Sync {
    async fn read(&self, lanes: &[LaneRef]) -> HashMap<String, (Backend, AgentState)>;
}

/// The pane side of oversight: the reminder and the interrupt after a hold.
#[async_trait]
pub trait OversightEffects: Send + Sync {
    /// `true` only when the reminder positively started in the pane.
    async fn remind(&self, lane: &LaneRef, text: &str) -> bool;
    async fn interrupt(&self, lane: &LaneRef);
}

pub struct Oversight {
    pub pool: SqlitePool,
    pub clock: Arc<dyn Clock>,
    pub states: Arc<dyn LaneStates>,
    pub effects: Arc<dyn OversightEffects>,
}

impl Oversight {
    /// One pass over every live, unheld lane. Returns the earliest armed deadline.
    pub async fn tick(&self) -> Result<Option<i64>> {
        if crate::control::is_paused(&self.pool).await.unwrap_or(true) {
            return Ok(None);
        }
        let now = self.clock.now_ms();
        let rows: Vec<(String, String, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT key, lead_session_key, omo_thread_id, host_id FROM lanes
              WHERE status IN ('working','not_done') ORDER BY key",
        )
        .fetch_all(&self.pool)
        .await?;
        let mut lanes = Vec::with_capacity(rows.len());
        for (key, lead, pane, host) in rows {
            // A held lane is never reminded, alerted or held again; it waits for an explicit continue.
            if crate::supervisor::hold::lane_held(&self.pool, &key).await? {
                continue;
            }
            lanes.push(LaneRef { key, lead, pane, host });
        }
        let states = self.states.read(&lanes).await;
        let mut next: Option<i64> = None;
        for lane in &lanes {
            let (backend, state) = states.get(&lane.key).copied().unwrap_or((Backend::Unknown, AgentState::Unknown));
            if let Some(at) = self.pass(lane, backend, state, now).await? {
                next = Some(next.map_or(at, |next| next.min(at)));
            }
        }
        Ok(next)
    }

    async fn pass(&self, lane: &LaneRef, backend: Backend, state: AgentState, now: i64) -> Result<Option<i64>> {
        let mut tx = self.pool.begin_write().await?;
        ensure_run_in(&mut tx, &lane.key, now).await?;
        sqlx::query("UPDATE lane_oversight SET backend = ? WHERE lane_key = ?")
            .bind(backend.as_str()).bind(&lane.key)
            .execute(&mut *tx)
            .await?;
        let row = load_in(&mut tx, &lane.key).await?.ok_or_else(|| crate::error::HubError::message("oversight row vanished"))?;
        tx.commit().await?;
        if row.ended {
            return Ok(None);
        }
        let totals = totals(&self.pool, &lane.key, row.run).await?;
        for due in decide(&row.snapshot, backend, state, &totals, now) {
            self.act(lane, row.run, &due).await?;
        }
        let mut tx = self.pool.begin_write().await?;
        let row = load_in(&mut tx, &lane.key).await?.ok_or_else(|| crate::error::HubError::message("oversight row vanished"))?;
        let at = store_deadline_in(&mut tx, &lane.key, &row, now).await?;
        tx.commit().await?;
        Ok(at)
    }

    async fn act(&self, lane: &LaneRef, run: i64, due: &Due) -> Result<()> {
        match due {
            Due::Remind(minutes) => {
                // Only a reminder that started counts; a refused send is retried next pass.
                if self.effects.remind(lane, &reminder_text(&lane.key, *minutes)).await {
                    self.mark(lane, run, "reminders_sent = reminders_sent + 1").await?;
                }
            }
            Due::StallAlert => {
                if self.mark(lane, run, "stall_alerted = 1").await? {
                    let body = format!(
                        "Lane {} has produced no new evidence for {} minutes. Look at it: reports without evidence do not count.",
                        lane.key, super::STALL_MINUTES
                    );
                    crate::lanes::dispatch::enqueue(&self.pool, &lane.key, &lane.lead, super::STALL_EVENT, &body).await?;
                }
            }
            Due::Ceiling(why) => {
                if !self.mark(lane, run, "ceiling_reported = 1").await? {
                    return Ok(());
                }
                let held = crate::supervisor::hold::hold_lane(&self.pool, &lane.key, "oversight:ceiling").await;
                let body = match &held {
                    Ok(_) => format!("Lane {} stopped at its ceiling: {why}. The lane is held until the lead or owner continues it.", lane.key),
                    Err(error) => format!("Lane {} reached its ceiling ({why}) but is not held: {error}", lane.key),
                };
                crate::lanes::dispatch::enqueue(&self.pool, &lane.key, &lane.lead, super::OBSTACLE_EVENT, &body).await?;
                if held.is_ok() {
                    self.effects.interrupt(lane).await;
                }
            }
        }
        Ok(())
    }

    /// Applies `set` to the lane's row for `run`; `false` when the run moved on.
    async fn mark(&self, lane: &LaneRef, run: i64, set: &str) -> Result<bool> {
        let mut tx = self.pool.begin_write().await?;
        let done = sqlx::query(&format!("UPDATE lane_oversight SET {set} WHERE lane_key = ? AND run = ?"))
            .bind(&lane.key).bind(run)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(done.rows_affected() == 1)
    }

    /// Runs passes until dropped: at the earliest deadline, on [`super::poke`], or every minute.
    pub async fn run(&self) {
        loop {
            let next = match self.tick().await {
                Ok(next) => next,
                Err(error) => {
                    tracing::warn!(%error, "lane oversight pass failed");
                    None
                }
            };
            let wait = next
                .and_then(|at| u64::try_from(at.saturating_sub(self.clock.now_ms())).ok())
                .map_or(POLL, |ms| Duration::from_millis(ms).min(POLL));
            tokio::select! {
                () = tokio::time::sleep(wait) => {}
                () = super::WAKE.notified() => {}
            }
        }
    }
}

/// Reads backend and state from the live fleet listing.
pub struct FleetLaneStates {
    pub pool: SqlitePool,
    pub control: Arc<dyn AgentControl>,
}

#[async_trait]
impl LaneStates for FleetLaneStates {
    async fn read(&self, lanes: &[LaneRef]) -> HashMap<String, (Backend, AgentState)> {
        let mut hosts = vec![LOCAL_HOST_ID.to_owned()];
        for host in lanes.iter().filter_map(|lane| lane.host.clone()) {
            if !hosts.contains(&host) {
                hosts.push(host);
            }
        }
        let entries = crate::fleet::list_persisted(self.control.as_ref(), &self.pool, &hosts, INVENTORY_TIMEOUT).await;
        let mut out = HashMap::with_capacity(lanes.len());
        for lane in lanes {
            let host = lane.host.as_deref().unwrap_or(LOCAL_HOST_ID);
            let Some(pane_id) = lane.pane.as_deref() else { continue };
            let pane = entries
                .iter()
                .filter(|entry| entry.host_id == host && entry.error.is_none() && !entry.stale)
                .flat_map(|entry| entry.sessions.iter())
                .find(|pane| pane.get("sessionId").and_then(serde_json::Value::as_str) == Some(pane_id));
            let Some(pane) = pane else { continue };
            let field = |key: &str| pane.get(key).and_then(serde_json::Value::as_str).unwrap_or("");
            let backend = Backend::of_pane(field("agent"), host == LOCAL_HOST_ID);
            out.insert(lane.key.clone(), (backend, AgentState::of_pane(field("state"))));
        }
        out
    }
}

/// Types reminders into the lane's pane and interrupts a held lane's running turn.
pub struct PaneEffects {
    pub control: Arc<dyn AgentControl>,
    pub clock: Arc<dyn Clock>,
}

#[async_trait]
impl OversightEffects for PaneEffects {
    async fn remind(&self, lane: &LaneRef, text: &str) -> bool {
        let Some(pane) = lane.pane.as_deref() else { return false };
        let request = format!("lane-oversight-{}-{}", lane.key, self.clock.now_ms());
        matches!(
            self.control.send_confirmed(lane.host.as_deref(), pane, text, &request).await,
            crate::goals::types::SendOutcome::Started
        )
    }

    async fn interrupt(&self, lane: &LaneRef) {
        let Some(pane) = lane.pane.clone() else { return };
        let op = AgentOp::Stop { session_id: pane, request_id: format!("lane-oversight-hold-{}", lane.key) };
        if let Err(error) = self.control.execute(lane.host.as_deref(), op).await {
            // The hold fence stays installed even when the interrupt fails.
            tracing::warn!(%error, lane = %lane.key, "held lane interruption failed");
        }
    }
}

/// The production pass over the live fleet with the system clock.
pub async fn serve(pool: SqlitePool, control: Arc<dyn AgentControl>) {
    let clock: Arc<dyn Clock> = Arc::new(crate::supervisor::recovery::SystemClock);
    let oversight = Oversight {
        states: Arc::new(FleetLaneStates { pool: pool.clone(), control: Arc::clone(&control) }),
        effects: Arc::new(PaneEffects { control, clock: Arc::clone(&clock) }),
        clock,
        pool,
    };
    oversight.run().await;
}
