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
        let mut pending: Vec<SessionRef> = st
            .sent
            .keys()
            .filter(|r| inc.ever.contains(*r))
            .cloned()
            .collect();
        pending.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        for r in pending {
            if now - st.sent[&r] < GRACE_MS {
                continue;
            }
            st.sent.remove(&r);
            if faulted.contains(&r) {
                inc.succeeded = false;
                inc.done.remove(&r);
                inc.primary = r.clone();
                inc.attempts += 1;
                inc.next_at_ms = now + backoff_ms(rules, inc.attempts as usize);
            } else {
                inc.succeeded = true;
                inc.done.insert(r);
            }
        }
        if inc.escalated.is_none() {
            let summary = format!("Retrying {} after upstream errors", sessions(inc.ever.len()));
            self.row(inc, ActionKind::Retried, summary, true, now).await;
        }
        if !inc.succeeded && inc.escalated.is_none() && now - inc.started_ms >= BUDGET_MS {
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
                    None => inc.succeeded = true,
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
                if let Some(o) = obs.get(r) {
                    if self.resume(st, o, false, now).await {
                        inc.done.insert(r.clone());
                    }
                }
            }
            let waiting = inc.ever.iter().any(|r| st.sent.contains_key(r));
            if !waiting && faulted.iter().all(|r| inc.done.contains(r)) {
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
            } else {
                let summary = format!("Resumed {title} after the usage limit reset");
                self.row(inc, ActionKind::Resumed, summary, false, now).await;
                self.finish(inc);
                return;
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
            self.finish(inc);
            return;
        };
        let title = o.session.title.clone();
        if !matches!(o.kind, HealthKind::Stalled { .. }) {
            let summary = format!("Restarted stalled {title}; it is working again");
            self.row(inc, ActionKind::Resumed, summary, false, now).await;
            self.finish(inc);
            return;
        }
        let (restarts, last) = st.restarts.get(&inc.primary).copied().unwrap_or((0, 0));
        if restarts < max_restarts(rules) {
            if self.resume(st, o, true, now).await {
                st.restarts.insert(inc.primary.clone(), (restarts + 1, now));
                let summary = format!("Cancelled and resumed stalled {title}");
                self.row(inc, ActionKind::Cancelled, summary, true, now).await;
            }
            return;
        }
        let threshold = rule_param(rules, RuleKind::StallRestart, "thresholdSecs")
            .and_then(|v| v.as_i64())
            .filter(|s| *s > 0)
            .unwrap_or(300)
            * 1000;
        if now - last < threshold {
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
