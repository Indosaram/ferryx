//! The three incident state machines.
use super::super::*;
use super::Obs;

impl RecoveryScheduler {
    pub(in super::super) async fn upstream(
        &self,
        st: &mut State,
        inc: &mut Incident,
        obs: &HashMap<SessionRef, Obs>,
        rules: &[StandingRule],
        now: i64,
    ) {
        let confirm = confirm_ms(rules);
        let mut none = false;
        let mut faulted: Vec<SessionRef> = obs
            .iter()
            .filter(|(r, o)| {
                o.session.host_id == inc.host
                    && matches!(o.kind, HealthKind::UpstreamError { .. })
                    && resume_for(&o.session.agent).is_ok()
                    && !st.dismissed.contains(*r)
            })
            .map(|(r, _)| r.clone())
            .collect();
        faulted.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        inc.ever.extend(faulted.iter().cloned());
        // A confirmed member that faults again is no longer confirmed: it leaves `done` and
        // is retried on the backoff, in this same incident.
        for r in &faulted {
            if !st.sent.contains_key(r) && inc.done.remove(r) {
                inc.attempts += 1;
                inc.hold.insert(r.clone(), now + backoff_ms(rules, inc.attempts as usize));
            }
        }
        // Silent sessions stay unresolved and are retried, never counted as resumed; one that
        // later shows real output is confirmed after all, one that ended is dropped.
        let mut silent: Vec<SessionRef> = inc.silent.iter().cloned().collect();
        silent.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        for r in silent {
            match obs.get(&r) {
                Some(o) if o.progress && !matches!(o.kind, HealthKind::UpstreamError { .. }) => {
                    inc.silent.remove(&r);
                    inc.succeeded = true;
                    inc.done.insert(r);
                }
                Some(_) if !faulted.contains(&r) => faulted.push(r),
                Some(_) => {}
                None if st.gone(&r, now, confirm) => {
                    inc.silent.remove(&r);
                }
                None => {}
            }
        }
        faulted.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        // A member that is in the fleet but not observed this pass (unreadable, or showing a
        // prompt) is neither gone nor recovered: nothing closes while one is held (L2).
        let held = inc
            .ever
            .iter()
            .any(|r| !inc.done.contains(r) && !obs.contains_key(r) && !st.gone(r, now, confirm));
        let mut pending: Vec<SessionRef> = st
            .sent
            .keys()
            .filter(|r| inc.ever.contains(*r))
            .cloned()
            .collect();
        pending.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        for r in pending {
            // A fresh fault is judged at once; anything else only after the confirmation
            // window: a spinner or status line right after the send is not a recovery.
            if now - st.sent[&r] < confirm && !faulted.contains(&r) {
                continue;
            }
            // Not observed this pass: judge it when it is.
            let Some(o) = obs.get(&r) else { continue };
            let Some(sent_at) = st.sent.remove(&r) else { continue };
            if faulted.contains(&r) {
                inc.failed_try_ms = Some(sent_at);
                inc.succeeded = false;
                inc.done.remove(&r);
                inc.primary = r.clone();
                inc.attempts += 1;
                inc.next_at_ms = now + backoff_ms(rules, inc.attempts as usize);
            } else if o.progress {
                inc.silent.remove(&r);
                inc.succeeded = true;
                inc.done.insert(r);
            } else {
                // Silence is not a recovery and not a failure: the incident stays open.
                inc.failed_try_ms = Some(sent_at);
                inc.done.remove(&r);
                inc.silent.insert(r.clone());
                faulted.push(r.clone());
                inc.attempts += 1;
                inc.next_at_ms = now + backoff_ms(rules, inc.attempts as usize);
                if !inc.succeeded {
                    inc.primary = r;
                }
            }
        }
        if inc.escalated.is_none() {
            let summary = format!("Retrying {} after upstream errors", sessions(inc.ever.len()));
            self.row(inc, ActionKind::Retried, summary, true, now).await;
        }
        if !inc.succeeded && !st.sent.contains_key(&inc.primary) {
            if !faulted.contains(&inc.primary) {
                match faulted.first() {
                    Some(next) => inc.primary = next.clone(),
                    None if !held => inc.succeeded = true,
                    None => {}
                }
            }
            if !inc.succeeded && inc.next_at_ms <= now {
                if let Some(o) = obs.get(&inc.primary) {
                    if !self.resume(st, o, rules, false, &mut none, now).await {
                        inc.failed_try_ms = Some(now);
                        inc.attempts += 1;
                        inc.next_at_ms = now + backoff_ms(rules, inc.attempts as usize);
                    }
                }
            }
        }
        if inc.succeeded {
            for r in &faulted {
                if inc.done.contains(r) || st.sent.contains_key(r) {
                    continue;
                }
                if inc.silent.contains(r) && inc.next_at_ms > now {
                    continue;
                }
                if inc.hold.get(r).is_some_and(|at| *at > now) {
                    continue;
                }
                if let Some(o) = obs.get(r) {
                    if self.resume(st, o, rules, false, &mut none, now).await {
                        inc.done.insert(r.clone());
                    } else {
                        inc.failed_try_ms = Some(now);
                    }
                }
            }
            let waiting = inc.ever.iter().any(|r| st.sent.contains_key(r));
            if !waiting
                && !held
                && inc.silent.is_empty()
                && faulted.iter().all(|r| inc.done.contains(r))
            {
                // Only sessions still in the fleet were resumed: a vanished one is not. A
                // member that is still here and healthy recovered on its own.
                let resumed = inc.done.iter().filter(|r| st.present.contains(*r)).count();
                let here = inc.ever.iter().filter(|r| st.present.contains(*r)).count();
                if resumed == 0 && here > 0 {
                    let summary =
                        format!("{} recovered from upstream errors on their own", sessions(here));
                    self.row(inc, ActionKind::Finished, summary, false, now).await;
                } else if resumed == 0 {
                    let summary = format!(
                        "{} ended before they could be resumed",
                        sessions(inc.ever.len())
                    );
                    self.row(inc, ActionKind::Finished, summary, false, now).await;
                } else {
                    let summary = format!("Resumed {} after upstream errors", sessions(resumed));
                    self.row(inc, ActionKind::Resumed, summary, false, now).await;
                }
                self.finish(inc);
            }
        }
        // The budget is judged after this pass's probe and sweep, so a member about to be
        // swept after a confirmed probe is not "still failing". A resume still inside its
        // confirmation window has no verdict yet: the owner is asked only once it failed.
        // Elapsed time alone never escalates: only a retry that started at or after the
        // deadline and failed does, so an outage as long as the budget still gets the next
        // scheduled retry (D2: a 20 minute outage recovers without a card).
        let budget = budget_ms(rules);
        let deadline = inc.budget_from_ms.saturating_add(budget);
        let confirming = inc
            .ever
            .iter()
            .any(|r| st.sent.get(r).is_some_and(|at| now - *at < confirm));
        if !inc.closed
            && (!inc.succeeded
                || !inc.silent.is_empty()
                || faulted.iter().any(|r| !inc.done.contains(r)))
            && !confirming
            && inc.escalated.is_none()
            && inc.failed_try_ms.is_some_and(|at| at >= deadline)
        {
            let n = inc.ever.len();
            let mins = minutes(budget);
            let every = minutes(every_secs(rules) * 1000);
            self.escalate(
                inc,
                format!("{} still cannot reach the upstream after {mins} minutes.", sessions(n)),
                ["Keep retrying", "Stop the sessions"],
                [Choice::KeepGoing, Choice::Stop],
                format!(
                    "Upstream errors outlasted the {mins} minute budget for {}; retrying every {every} minutes is cheap and safe",
                    sessions(n)
                ),
                now,
            );
            let summary = format!(
                "Upstream errors outlasted the retry budget for {}; asked the owner",
                sessions(n)
            );
            self.row(inc, ActionKind::Escalated, summary, true, now).await;
        }
    }

    pub(in super::super) async fn quota(
        &self,
        st: &mut State,
        inc: &mut Incident,
        obs: &HashMap<SessionRef, Obs>,
        rules: &[StandingRule],
        now: i64,
    ) {
        let confirm = confirm_ms(rules);
        let Some(o) = obs.get(&inc.primary) else {
            // In the fleet but not observed: wait. Gone: it ended, which is not a resume.
            if !st.gone(&inc.primary, now, confirm) {
                return;
            }
            let summary = format!("{} hit its usage limit and has ended", inc.title);
            self.row(inc, ActionKind::Finished, summary, false, now).await;
            self.finish(inc);
            return;
        };
        let title = o.session.title.clone();
        let mut none = false;
        let max = quota_attempts(rules);
        if let Some(at) = st.sent.get(&inc.primary).copied() {
            if now - at < confirm {
                return;
            }
            st.sent.remove(&inc.primary);
            if let HealthKind::Quota { reset_at_ms } = &o.kind {
                inc.attempts += 1;
                inc.next_at_ms = reset_at_ms.unwrap_or(now + quota_ms(rules));
                if inc.attempts >= max {
                    self.escalate(
                        inc,
                        format!("{title} keeps hitting its usage limit."),
                        ["Keep waiting", "Stop the session"],
                        [Choice::KeepGoing, Choice::Stop],
                        "Resuming at the reset time failed repeatedly; waiting for the next reset costs nothing".to_owned(),
                        now,
                    );
                }
            } else if o.progress {
                let summary = format!("Resumed {title} after the usage limit reset");
                self.row(inc, ActionKind::Resumed, summary, false, now).await;
                self.finish(inc);
                return;
            } else {
                // Silence after the resume is no recovery: wait for the next window.
                inc.attempts += 1;
                inc.next_at_ms = now + quota_ms(rules);
                if inc.attempts >= max {
                    self.escalate(
                        inc,
                        format!("{title} stays silent after its usage limit reset."),
                        ["Keep waiting", "Stop the session"],
                        [Choice::KeepGoing, Choice::Stop],
                        "Resuming at the reset time produced no output; waiting costs nothing"
                            .to_owned(),
                        now,
                    );
                }
            }
        } else if !matches!(o.kind, HealthKind::Quota { .. }) {
            let summary = format!("{title} recovered from its usage limit on its own");
            self.row(inc, ActionKind::Finished, summary, false, now).await;
            self.finish(inc);
            return;
        }
        let summary = format!("Waiting for {title}'s usage limit to reset");
        self.row(inc, ActionKind::Retried, summary, true, now).await;
        if inc.next_at_ms <= now
            && inc.escalated.is_none()
            && !self.resume(st, o, rules, false, &mut none, now).await
        {
            inc.next_at_ms = now + quota_ms(rules);
        }
    }

    pub(in super::super) async fn stall(
        &self,
        st: &mut State,
        inc: &mut Incident,
        obs: &HashMap<SessionRef, Obs>,
        rules: &[StandingRule],
        now: i64,
    ) {
        let Some(o) = obs.get(&inc.primary) else {
            // Gone: the session ended, which is not a recovery.
            if st.gone(&inc.primary, now, confirm_ms(rules)) {
                if !inc.last_row.is_empty() {
                    let summary = format!("{} ended while being recovered", inc.title);
                    self.row(inc, ActionKind::Finished, summary, false, now).await;
                }
                self.finish(inc);
            }
            return;
        };
        let title = o.session.title.clone();
        let (restarts, last) = (inc.restarts, inc.last_restart_ms);
        let threshold = stall_threshold_ms(rules);
        if !matches!(o.kind, HealthKind::Stalled { .. }) {
            // After a restart the incident stays in confirmation: a changed screen alone is
            // not recovery (the Stop and Send move it themselves). It closes when the turn
            // settled (provider left Working) or the screen itself last changed a full stall
            // threshold after the restart; the same clock the classifier stalls on, so a
            // freeze after output is Stalled in this incident, not a closed one.
            let moved_on = o
                .session
                .screen_changed_at_ms
                .is_some_and(|at| at >= last.saturating_add(threshold));
            if restarts == 0 {
                if !inc.last_row.is_empty() {
                    let summary = format!("{title} recovered from its stall on its own");
                    self.row(inc, ActionKind::Finished, summary, false, now).await;
                }
                self.finish(inc);
            } else if o.session.state != "working"
                || moved_on
                || o.session.pending_prompt.is_some()
            {
                let summary = format!("Restarted stalled {title}; it is working again");
                self.row(inc, ActionKind::Resumed, summary, false, now).await;
                self.finish(inc);
            }
            return;
        }
        if restarts < max_restarts(rules) {
            if inc.next_at_ms > now {
                return;
            }
            // A delivered Ctrl-C is never repeated: a failed send retries the text only (L3).
            let mut stopped = inc.stopped_pending;
            let sent = self.resume(st, o, rules, true, &mut stopped, now).await;
            inc.stopped_pending = stopped;
            if sent {
                inc.restarts = restarts + 1;
                inc.last_restart_ms = now;
                let summary = format!("Cancelled and resumed stalled {title}");
                self.row(inc, ActionKind::Cancelled, summary, true, now).await;
            } else {
                inc.attempts += 1;
                inc.next_at_ms = now + backoff_ms(rules, inc.attempts as usize);
            }
            return;
        }
        if now - last < threshold {
            return;
        }
        self.escalate(
            inc,
            format!("{title} stalled again after a restart."),
            ["Restart it once more", "Leave it"],
            [Choice::KeepGoing, Choice::Leave],
            "It was cancelled and resumed once and stalled again; one more restart is the cheapest next step".to_owned(),
            now,
        );
        let summary = format!("{title} stalled again after a restart; asked the owner");
        self.row(inc, ActionKind::Escalated, summary, true, now).await;
    }
}
