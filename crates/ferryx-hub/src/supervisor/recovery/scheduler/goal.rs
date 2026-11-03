//! Goal-lane quota deferral: the supervisor's own model hit a usage limit, so the lane is
//! retried once the reset time passes instead of being stopped. A lane is retried a bounded
//! number of times (`maxGoalAttempts`), and the row only says "Resumed" when the goal loop
//! really ran again.
use super::restore::{EV_ATTEMPTS, EV_RETRY_AT};
use super::*;

enum GoalRow {
    Waiting { at_ms: i64, attempts: u32 },
    Resumed,
    NothingToResume,
    Stopped { reason: String },
    GaveUp { attempts: u32 },
}

impl RecoveryScheduler {
    /// Defers the goal-loop pass of `lane_key` until the reset time named in `detail` (else
    /// the `quotaResume` fallback). One action-log row per lane, edited in place. A report
    /// that arrives while the lane's retry runs means the retry hit the limit again.
    pub async fn report_goal_quota(
        &self,
        lane_key: &str,
        session: Option<SessionRef>,
        detail: &str,
    ) {
        let now = self.deps.clock.now_ms();
        let rules = self.rules().await;
        let at_ms = reset_at_from_text(detail, now).unwrap_or(now + quota_ms(&rules));
        let (started_ms, attempts) = {
            let mut goals = self.goals.lock().unwrap_or_else(PoisonError::into_inner);
            let wait = goals.entry(lane_key.to_owned()).or_insert(GoalWait {
                at_ms,
                started_ms: now,
                session: session.clone(),
                attempts: 0,
                requeued: false,
                in_flight: false,
            });
            wait.at_ms = at_ms;
            if wait.in_flight {
                wait.requeued = true;
            }
            (wait.started_ms, wait.attempts)
        };
        let row = GoalRow::Waiting { at_ms, attempts };
        self.goal_row(lane_key, session.as_ref(), started_ms, now, row).await;
    }

    pub(super) async fn run_goals(&self, now: i64) {
        let due: Vec<String> = {
            let goals = self.goals.lock().unwrap_or_else(PoisonError::into_inner);
            let mut due: Vec<String> = goals
                .iter()
                .filter(|(_, wait)| wait.at_ms <= now && !wait.in_flight)
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
        let rules = self.rules().await;
        for lane in due {
            if !self.go().await {
                return;
            }
            let claimed = {
                let mut goals = self.goals.lock().unwrap_or_else(PoisonError::into_inner);
                match goals.get_mut(&lane) {
                    Some(wait) if !wait.in_flight => {
                        wait.in_flight = true;
                        wait.requeued = false;
                        true
                    }
                    _ => false,
                }
            };
            if !claimed {
                continue;
            }
            let outcome = retry.retry_outcome(&lane).await;
            if let Err(error) = &outcome {
                tracing::warn!(%error, lane = %lane, "goal lane retry failed; trying again later");
            }
            let (row, started_ms, session) = {
                let mut goals = self.goals.lock().unwrap_or_else(PoisonError::into_inner);
                let Some(wait) = goals.get_mut(&lane) else { continue };
                wait.in_flight = false;
                let requeued = std::mem::take(&mut wait.requeued);
                let (started_ms, session) = (wait.started_ms, wait.session.clone());
                let finished = match &outcome {
                    Ok(done) if !requeued => Some(done.clone()),
                    _ => None,
                };
                let row = match finished {
                    Some(done) => {
                        goals.remove(&lane);
                        match done {
                            RetryOutcome::Resumed => GoalRow::Resumed,
                            RetryOutcome::NothingToResume => GoalRow::NothingToResume,
                            RetryOutcome::Stopped(reason) => GoalRow::Stopped { reason },
                        }
                    }
                    None => {
                        wait.attempts += 1;
                        if wait.attempts >= goal_attempts(&rules) {
                            let attempts = wait.attempts;
                            goals.remove(&lane);
                            GoalRow::GaveUp { attempts }
                        } else {
                            if outcome.is_err() {
                                wait.at_ms = now + quota_ms(&rules);
                            }
                            GoalRow::Waiting { at_ms: wait.at_ms, attempts: wait.attempts }
                        }
                    }
                };
                (row, started_ms, session)
            };
            if let GoalRow::GaveUp { attempts } = &row {
                let detail = format!(
                    "recovery gave up after {attempts} retries of a usage-limited goal pass"
                );
                if let Err(error) = retry.give_up(&lane, &detail).await {
                    tracing::warn!(%error, lane = %lane, "an exhausted goal lane was not stopped");
                }
            }
            self.goal_row(&lane, session.as_ref(), started_ms, now, row).await;
        }
    }

    async fn goal_row(
        &self,
        lane_key: &str,
        session: Option<&SessionRef>,
        started_ms: i64,
        now: i64,
        what: GoalRow,
    ) {
        let mut evidence = vec![EvidenceRef {
            kind: "goal".to_owned(),
            id: lane_key.to_owned(),
        }];
        let (kind, summary, live) = match what {
            GoalRow::Waiting { at_ms, attempts } => {
                evidence.push(EvidenceRef { kind: EV_RETRY_AT.to_owned(), id: at_ms.to_string() });
                evidence.push(EvidenceRef { kind: EV_ATTEMPTS.to_owned(), id: attempts.to_string() });
                (
                    ActionKind::Retried,
                    format!("Waiting for the supervisor's usage limit to reset (goal {lane_key})"),
                    true,
                )
            }
            GoalRow::Resumed => (
                ActionKind::Resumed,
                format!("Resumed goal {lane_key} after the usage limit reset"),
                false,
            ),
            GoalRow::NothingToResume => (
                ActionKind::Finished,
                format!("Goal {lane_key} had nothing to resume after the usage limit reset"),
                false,
            ),
            GoalRow::GaveUp { attempts } => (
                ActionKind::Escalated,
                format!("Gave up retrying goal {lane_key} after {attempts} attempts; the lane is blocked for the owner"),
                false,
            ),
            GoalRow::Stopped { reason } => (
                ActionKind::Finished,
                format!("Goal {lane_key} was not resumed: the loop stopped it ({reason})"),
                false,
            ),
        };
        let entry = ActionEntry {
            id: 0,
            incident_key: format!("{}{lane_key}", super::super::GOAL_QUOTA_PREFIX),
            session_ref: session.cloned(),
            kind,
            summary,
            basis: "quotaResume rule: resume at the reset time, else after the fallback".to_owned(),
            evidence,
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
