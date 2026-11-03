//! Goal-lane quota deferral: the supervisor's own model hit a usage limit, so the lane is
//! retried once the reset time passes instead of being stopped.
use super::*;

impl RecoveryScheduler {
    /// Defers the goal-loop pass of `lane_key` until the reset time named in `detail` (else
    /// the `quotaResume` fallback). One action-log row per lane, edited in place.
    pub async fn report_goal_quota(
        &self,
        lane_key: &str,
        session: Option<SessionRef>,
        detail: &str,
    ) {
        let now = self.deps.clock.now_ms();
        let rules = self.rules().await;
        let at_ms = reset_at_from_text(detail, now).unwrap_or(now + quota_ms(&rules));
        let started_ms = {
            let mut goals = self.goals.lock().unwrap_or_else(PoisonError::into_inner);
            let wait = goals.entry(lane_key.to_owned()).or_insert(GoalWait {
                at_ms,
                started_ms: now,
                session: session.clone(),
            });
            wait.at_ms = at_ms;
            wait.started_ms
        };
        self.goal_row(lane_key, session.as_ref(), started_ms, now, true)
            .await;
    }

    pub(super) async fn run_goals(&self, now: i64) {
        let due: Vec<String> = {
            let goals = self.goals.lock().unwrap_or_else(PoisonError::into_inner);
            let mut due: Vec<String> = goals
                .iter()
                .filter(|(_, wait)| wait.at_ms <= now)
                .map(|(lane, _)| lane.clone())
                .collect();
            due.sort();
            due
        };
        let retry = self
            .retry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let Some(retry) = retry else { return };
        for lane in due {
            if !self.go().await {
                return;
            }
            let wait = self
                .goals
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&lane);
            let Some(wait) = wait else { continue };
            let outcome = retry.retry(&lane).await;
            let requeued = self
                .goals
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .contains_key(&lane);
            match outcome {
                Ok(()) if requeued => {}
                Ok(()) => {
                    self.goal_row(&lane, wait.session.as_ref(), wait.started_ms, now, false)
                        .await;
                }
                Err(error) => {
                    tracing::warn!(%error, lane = %lane, "goal lane retry failed; trying again later");
                    if !requeued {
                        let rules = self.rules().await;
                        self.goals
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .insert(
                                lane.clone(),
                                GoalWait {
                                    at_ms: now + quota_ms(&rules),
                                    ..wait
                                },
                            );
                    }
                }
            }
        }
    }

    async fn goal_row(
        &self,
        lane_key: &str,
        session: Option<&SessionRef>,
        started_ms: i64,
        now: i64,
        live: bool,
    ) {
        let (kind, summary) = if live {
            (
                ActionKind::Retried,
                format!("Waiting for the supervisor's usage limit to reset (goal {lane_key})"),
            )
        } else {
            (
                ActionKind::Resumed,
                format!("Resumed goal {lane_key} after the usage limit reset"),
            )
        };
        let entry = ActionEntry {
            id: 0,
            incident_key: format!("{}{lane_key}", super::super::GOAL_QUOTA_PREFIX),
            session_ref: session.cloned(),
            kind,
            summary,
            basis: "quotaResume rule: resume at the reset time, else after the fallback".to_owned(),
            evidence: vec![EvidenceRef {
                kind: "goal".to_owned(),
                id: lane_key.to_owned(),
            }],
            undo: None,
            live,
            created_at_ms: started_ms,
            updated_at_ms: now,
        };
        if let Err(error) = self.deps.log.upsert_incident(entry).await {
            tracing::warn!(%error, "recovery action row was not written");
        }
    }
}
