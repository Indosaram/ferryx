//! One recovery pass: observe the fleet, open incidents for new faults, advance each one.
use super::*;

mod steps;

/// One live session as this pass sees it. `kind` reads only the output that appeared after
/// the last resume sent to the session, so an old error is never read twice.
pub(super) struct Obs {
    pub session: FleetSession,
    pub tail: String,
    pub kind: HealthKind,
}

fn faulted(kind: &HealthKind) -> bool {
    matches!(
        kind,
        HealthKind::UpstreamError { .. } | HealthKind::Quota { .. } | HealthKind::Stalled { .. }
    )
}

fn new_incident(key: String, scope: String, fault: Fault, o: &Obs, next_at_ms: i64, now: i64) -> Incident {
    let primary = o.session.session_ref.clone();
    Incident {
        key,
        scope,
        fault,
        host: o.session.host_id.clone(),
        primary: primary.clone(),
        started_ms: now,
        attempts: 0,
        next_at_ms,
        succeeded: false,
        closed: false,
        escalated: None,
        ever: HashSet::from([primary]),
        done: HashSet::new(),
        last_row: String::new(),
    }
}

impl RecoveryScheduler {
    /// One deterministic pass. Does nothing at all while the owner has paused the supervisor.
    pub async fn tick(&self) {
        if !self.go().await {
            return;
        }
        let now = self.deps.clock.now_ms();
        let rules = self.rules().await;
        let Ok(fleet) = self.deps.fleet.list().await else {
            return;
        };
        let mut st = self.state.lock().await;
        let mut obs: HashMap<SessionRef, Obs> = HashMap::new();
        for session in fleet {
            if session.stale || session.pending_prompt.is_some() {
                continue;
            }
            let Ok(tail) = self
                .deps
                .control
                .read_tail(&session.session_ref, TAIL_LINES)
                .await
            else {
                continue;
            };
            let seen = match st.base.get(&session.session_ref) {
                Some(base) => fresh_lines(base, &tail).join("\n"),
                None => tail.clone(),
            };
            let kind = self.deps.classifier.classify(&session, &seen, &rules, now);
            obs.insert(session.session_ref.clone(), Obs { session, tail, kind });
        }
        st.sent.retain(|r, _| obs.contains_key(r));
        st.base.retain(|r, _| obs.contains_key(r));
        self.refusals(&mut st, &obs, now).await;
        self.open_incidents(&mut st, &obs, &rules, now);
        let mut incidents = std::mem::take(&mut st.incidents);
        for inc in incidents.iter_mut() {
            match inc.fault {
                Fault::Upstream => self.upstream(&mut st, inc, &obs, &rules, now).await,
                Fault::Quota => self.quota(&mut st, inc, &obs, &rules, now).await,
                Fault::Stall => self.stall(&mut st, inc, &obs, &rules, now).await,
            }
        }
        for inc in incidents.iter().filter(|inc| inc.closed) {
            *st.gens.entry(inc.scope.clone()).or_insert(0) += 1;
        }
        incidents.retain(|inc| !inc.closed);
        st.incidents = incidents;
        drop(st);
        self.run_goals(now).await;
    }

    fn open_incidents(
        &self,
        st: &mut State,
        obs: &HashMap<SessionRef, Obs>,
        rules: &[StandingRule],
        now: i64,
    ) {
        let mut refs: Vec<&SessionRef> = obs.keys().collect();
        refs.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        for r in refs {
            let o = &obs[r];
            if !faulted(&o.kind) || resume_for(&o.session.agent).is_err() {
                continue;
            }
            let (fault, scope, at) = match &o.kind {
                HealthKind::UpstreamError { .. } => (
                    Fault::Upstream,
                    format!("upstream:{}", o.session.host_id),
                    now + backoff_ms(rules, 0),
                ),
                HealthKind::Quota { reset_at_ms } => (
                    Fault::Quota,
                    format!("quota:{r}"),
                    reset_at_ms.unwrap_or(now + quota_ms(rules)),
                ),
                _ => (Fault::Stall, format!("stall:{r}"), now),
            };
            if st.incidents.iter().any(|i| i.scope == scope) {
                continue;
            }
            let generation = st.gens.get(&scope).copied().unwrap_or(0);
            let key = format!("recovery:{scope}:g{generation}");
            st.incidents.push(new_incident(key, scope, fault, o, at, now));
        }
    }

    async fn row(&self, inc: &mut Incident, kind: ActionKind, summary: String, live: bool, now: i64) {
        let marker = format!("{kind:?}|{summary}|{live}");
        if inc.last_row == marker {
            return;
        }
        let mut refs: Vec<&SessionRef> = inc.ever.iter().collect();
        refs.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        let entry = ActionEntry {
            id: 0,
            incident_key: inc.key.clone(),
            session_ref: Some(inc.primary.clone()),
            kind,
            summary,
            basis: "default recovery policy (D2): act first, ask only after the retry budget"
                .to_owned(),
            evidence: refs
                .into_iter()
                .map(|r| EvidenceRef {
                    kind: "session".to_owned(),
                    id: r.to_string(),
                })
                .collect(),
            undo: None,
            live,
            created_at_ms: inc.started_ms,
            updated_at_ms: now,
        };
        match self.deps.log.upsert_incident(entry).await {
            Ok(_) => inc.last_row = marker,
            Err(error) => tracing::warn!(%error, "recovery action row was not written"),
        }
    }

    /// Hands the incident to the owner once, with a recommended option and a reason.
    fn escalate(&self, inc: &mut Incident, question: String, options: [&str; 2], reason: String, now: i64) {
        if inc.escalated.is_some() {
            return;
        }
        inc.escalated = Some(inc.primary.clone());
        self.deps.escalations.open(Escalation {
            session_ref: inc.primary.clone(),
            prompt: PendingPrompt {
                prompt_id: format!("recovery:{}", inc.key),
                kind: "recovery".to_owned(),
                question,
                command: None,
                options: options.iter().map(|o| (*o).to_owned()).collect(),
                screen_revision: None,
            },
            recommended_index: 0,
            reason,
            created_at_ms: now,
        });
    }

    fn finish(&self, inc: &mut Incident) {
        if let Some(session) = inc.escalated.take() {
            self.deps.escalations.resolve(&session);
        }
        inc.closed = true;
    }

    /// Stop (optionally) then send the agent's resume text. `false` when refused or failed.
    async fn resume(&self, st: &mut State, o: &Obs, stop_first: bool, now: i64) -> bool {
        if !self.go().await {
            return false;
        }
        let Ok(text) = resume_for(&o.session.agent) else {
            return false;
        };
        let r = &o.session.session_ref;
        if stop_first && self.deps.control.stop(r).await.is_err() {
            return false;
        }
        if self.deps.control.send(r, text).await.is_err() {
            return false;
        }
        st.sent.insert(r.clone(), now);
        st.base.insert(r.clone(), o.tail.clone());
        true
    }

    /// An unknown agent is refused with one typed, owner-visible row and card; nothing is sent.
    async fn refusals(&self, st: &mut State, obs: &HashMap<SessionRef, Obs>, now: i64) {
        let mut refs: Vec<&SessionRef> = obs.keys().collect();
        refs.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        for r in refs {
            let o = &obs[r];
            let Err(refusal) = resume_for(&o.session.agent) else {
                continue;
            };
            if !faulted(&o.kind) {
                if st.refused.remove(r) {
                    self.deps.escalations.resolve(r);
                }
                continue;
            }
            if !st.refused.insert(r.clone()) {
                continue;
            }
            let reason = refusal.to_string();
            let key = format!("recovery:refused:{r}");
            let mut inc = new_incident(key, format!("refused:{r}"), Fault::Stall, o, now, now);
            self.escalate(
                &mut inc,
                format!("{} needs recovery but cannot be resumed automatically.", o.session.title),
                ["Open the session", "Leave it"],
                format!("{reason}; nothing was sent"),
                now,
            );
            let summary = format!("Left {} alone: {reason}", o.session.title);
            self.row(&mut inc, ActionKind::Escalated, summary, true, now).await;
        }
    }
}
