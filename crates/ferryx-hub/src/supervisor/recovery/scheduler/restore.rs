//! Reads the persisted rows of an earlier run once (M1): fault generations move past every
//! stored incident key, so a new incident never reuses (and overwrites) an old row; incident
//! rows left live by the crash are closed; deferred goal lanes are rescheduled.
use super::*;

/// Evidence kinds that carry a goal row's schedule across a restart.
pub(super) const EV_RETRY_AT: &str = "retryAt";
pub(super) const EV_ATTEMPTS: &str = "attempts";

impl RecoveryScheduler {
    /// `false` when the rows could not be read: the pass is skipped and retried next time, so
    /// no incident is opened with a key that may already exist.
    pub(super) async fn restore(&self, st: &mut State, now: i64) -> bool {
        if st.restored {
            return true;
        }
        let rows = match self.deps.log.list_today(0).await {
            Ok(rows) => rows,
            Err(error) => {
                tracing::warn!(%error, "recovery rows were not read; no recovery pass yet");
                return false;
            }
        };
        let mut interrupted = Vec::new();
        for row in rows {
            if let Some(lane) = row.incident_key.strip_prefix(super::super::GOAL_QUOTA_PREFIX) {
                if row.live {
                    self.restore_goal(lane, &row, now);
                }
                continue;
            }
            let Some(rest) = row.incident_key.strip_prefix(CARD_PREFIX) else {
                continue;
            };
            let Some((scope, generation)) = rest.rsplit_once(":g") else {
                continue;
            };
            let Ok(generation) = generation.parse::<u32>() else {
                continue;
            };
            let slot = st.gens.entry(scope.to_owned()).or_insert(0);
            *slot = (*slot).max(generation.saturating_add(1));
            if row.live {
                interrupted.push(row);
            }
        }
        for mut row in interrupted {
            row.live = false;
            row.kind = ActionKind::Finished;
            row.summary = format!(
                "{} (interrupted by a restart; recovery starts over if the fault is still there)",
                row.summary
            );
            row.updated_at_ms = now;
            if let Err(error) = self.deps.log.upsert_incident(row).await {
                tracing::warn!(%error, "an interrupted recovery row was not closed");
            }
        }
        st.restored = true;
        true
    }

    fn restore_goal(&self, lane: &str, row: &ActionEntry, now: i64) {
        let number = |kind: &str| {
            row.evidence
                .iter()
                .find(|e| e.kind == kind)
                .and_then(|e| e.id.parse::<i64>().ok())
        };
        self.goals
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(lane.to_owned())
            .or_insert(GoalWait {
                at_ms: number(EV_RETRY_AT).unwrap_or(now),
                started_ms: row.created_at_ms,
                session: row.session_ref.clone(),
                attempts: number(EV_ATTEMPTS).map_or(0, |n| u32::try_from(n).unwrap_or(0)),
                requeued: false,
                in_flight: false,
            });
    }
}
