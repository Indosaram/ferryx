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
        let mut faulted: Vec<SessionRef> = obs
            .iter()
            .filter(|(_, o)| {
                o.session.host_id == inc.host
                    && matches!(o.kind, HealthKind::UpstreamError { .. })
                    && resume_for(&o.session.agent).is_ok()
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
        // later shows real output is confirmed after all.
        for r in inc.silent.clone() {
            match obs.get(&r) {
                Some(o) if o.progress && !matches!(o.kind, HealthKind::UpstreamError { .. }) => {
                    inc.silent.remove(&r);
                    inc.succeeded = true;
                    inc.done.insert(r);
                }
                Some(_) if !faulted.contains(&r) => faulted.push(r),
                Some(_) => {}
                None if st.present.contains(&r) => {}
                None => {
                    inc.silent.remove(&r);
                }
            }
        }
        faulted.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        let unreadable = inc
            .ever
            .iter()
            .any(|r| st.unreadable.contains(r) && !inc.done.contains(r));
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
            if now - st.sent[&r] < CONFIRM_MS && !faulted.contains(&r) {
                continue;
            }
            // Not readable this pass: judge it when it is.
            let Some(o) = obs.get(&r) else { continue };
            st.sent.remove(&r);
            if faulted.contains(&r) {
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
                // 30 s of silence is not a recovery and not a failure: the incident stays open.
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
        if (!inc.succeeded
            || !inc.silent.is_empty()
            || faulted.iter().any(|r| !inc.done.contains(r)))
            && inc.escalated.is_none()
            && now - inc.started_ms >= BUDGET_MS
        {
            let n = inc.ever.len();
            self.escalate(
                inc,
                format!("{} still cannot reach the upstream after 20 minutes.", sessions(n)),
                ["Keep retrying", "Stop the sessions"],
                format!(
                    "Upstream errors outlasted the 20 minute budget for {}; retrying every 10 minutes is cheap and safe",
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
        if !inc.succeeded && !st.sent.contains_key(&inc.primary) {
            if !faulted.contains(&inc.primary) {
                match faulted.first() {
                    Some(next) => inc.primary = next.clone(),
                    None if !unreadable => inc.succeeded = true,
                    None => {}
                }
            }
            if !inc.succeeded && inc.next_at_ms <= now {
                if let Some(o) = obs.get(&inc.primary) {
                    if !self.resume(st, o, false, now).await {
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
                    if self.resume(st, o, false, now).await {
                        inc.done.insert(r.clone());
                    }
                }
            }
            let waiting = inc.ever.iter().any(|r| st.sent.contains_key(r));
            if !waiting
                && !unreadable
                && inc.silent.is_empty()
                && faulted.iter().all(|r| inc.done.contains(r))
            {
                let summary = format!("Resumed {} after upstream errors", sessions(inc.ever.len()));
                self.row(inc, ActionKind::Resumed, summary, false, now).await;
                self.finish(inc);
            }
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
        let Some(o) = obs.get(&inc.primary) else {
            if st.unreadable.contains(&inc.primary) {
                return;
            }
            let summary = "A session hit its usage limit and has ended".to_owned();
            self.row(inc, ActionKind::Finished, summary, false, now).await;
            self.finish(inc);
            return;
        };
        let title = o.session.title.clone();
        if let Some(at) = st.sent.get(&inc.primary).copied() {
            if now - at < GRACE_MS {
                return;
            }
            st.sent.remove(&inc.primary);
            if let HealthKind::Quota { reset_at_ms } = &o.kind {
                inc.attempts += 1;
                inc.next_at_ms = reset_at_ms.unwrap_or(now + quota_ms(rules));
                if inc.attempts >= QUOTA_ATTEMPTS {
                    self.escalate(
                        inc,
                        format!("{title} keeps hitting its usage limit."),
                        ["Keep waiting", "Stop the session"],
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
                if inc.attempts >= QUOTA_ATTEMPTS {
                    self.escalate(
                        inc,
                        format!("{title} stays silent after its usage limit reset."),
                        ["Keep waiting", "Stop the session"],
                        "Resuming at the reset time produced no output; waiting costs nothing".to_owned(),
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
        if inc.next_at_ms <= now && !self.resume(st, o, false, now).await {
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
            if !st.unreadable.contains(&inc.primary) {
                self.finish(inc);
            }
            return;
        };
        let title = o.session.title.clone();
        let (restarts, last) = (inc.restarts, inc.last_restart_ms);
        if !matches!(o.kind, HealthKind::Stalled { .. }) {
            // After a restart the incident stays in confirmation: a changed screen alone is
            // not recovery (the Stop and Send move it themselves). It closes when the turn
            // settled (provider left Working) or a stall-free window of the stall threshold
            // passed. A stall meanwhile stays in this incident and escalates once.
            if restarts == 0 {
                self.finish(inc);
            } else if o.session.state != "working" || now - last >= stall_threshold_ms(rules) {
                let summary = format!("Restarted stalled {title}; it is working again");
                self.row(inc, ActionKind::Resumed, summary, false, now).await;
                self.finish(inc);
            }
            return;
        }
        if restarts < max_restarts(rules) {
            if self.resume(st, o, true, now).await {
                inc.restarts = restarts + 1;
                inc.last_restart_ms = now;
                let summary = format!("Cancelled and resumed stalled {title}");
                self.row(inc, ActionKind::Cancelled, summary, true, now).await;
            }
            return;
        }
        if now - last < stall_threshold_ms(rules) {
            return;
        }
        self.escalate(
            inc,
            format!("{title} stalled again after a restart."),
            ["Restart it once more", "Leave it"],
            "It was cancelled and resumed once and stalled again; one more restart is the cheapest next step".to_owned(),
            now,
        );
        let summary = format!("{title} stalled again after a restart; asked the owner");
        self.row(inc, ActionKind::Escalated, summary, true, now).await;
    }
}
