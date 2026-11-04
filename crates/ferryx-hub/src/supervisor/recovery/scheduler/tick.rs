//! One recovery pass: observe the fleet, open incidents for new faults, advance each one.
use super::*;

mod steps;

/// One live session as this pass sees it. `kind` reads only the output that appeared after
/// the session's baseline (first sight, or the last resume sent to it), so an old error is
/// never read twice.
pub(super) struct Obs {
    pub session: FleetSession,
    pub tail: String,
    pub kind: HealthKind,
    /// Fresh output since the baseline that is neither empty nor the echo of the resume text
    /// we typed: evidence a resume produced something (H3).
    pub progress: bool,
}

fn faulted(kind: &HealthKind) -> bool {
    matches!(
        kind,
        HealthKind::UpstreamError { .. } | HealthKind::Quota { .. } | HealthKind::Stalled { .. }
    )
}

fn new_incident(
    key: String,
    scope: String,
    fault: Fault,
    o: &Obs,
    next_at_ms: i64,
    now: i64,
) -> Incident {
    let primary = o.session.session_ref.clone();
    Incident {
        key,
        scope,
        fault,
        host: o.session.host_id.clone(),
        primary: primary.clone(),
        title: o.session.title.clone(),
        started_ms: now,
        budget_from_ms: now,
        attempts: 0,
        next_at_ms,
        succeeded: false,
        closed: false,
        escalated: None,
        ever: HashSet::from([primary]),
        done: HashSet::new(),
        last_row: String::new(),
        silent: HashSet::new(),
        restarts: 0,
        last_restart_ms: 0,
        hold: HashMap::new(),
        stopped_pending: false,
        stopped: HashSet::new(),
        choices: Vec::new(),
    }
}

/// A terminal line that only shows the resume text we typed, behind a prompt glyph.
fn is_echo(line: &str, text: &str) -> bool {
    line.trim_start_matches(|c: char| {
        c.is_whitespace() || matches!(c, '>' | '$' | '\u{276f}' | '\u{203a}' | '\u{2502}')
    })
    .trim()
        == text
}

impl State {
    /// `true` while an open stall incident has restarted `r` and is still confirming it.
    pub(super) fn restarted(&self, r: &SessionRef) -> bool {
        self.incidents
            .iter()
            .any(|i| i.fault == Fault::Stall && i.primary == *r && i.restarts > 0 && !i.closed)
    }

    /// `true` once a session has been out of the fleet for at least `grace_ms`: a one-pass
    /// blip of the fleet listing is not a vanish (L2). A session in the fleet is never gone,
    /// even when this pass could not observe it.
    pub(super) fn gone(&self, r: &SessionRef, now: i64, grace_ms: i64) -> bool {
        !self.present.contains(r)
            && self
                .absent_since
                .get(r)
                .is_none_or(|since| now - *since >= grace_ms)
    }

    /// Closes `inc`'s fault generation, so the next incident of the scope has a new key.
    pub(super) fn bump(&mut self, inc: &Incident) {
        *self.gens.entry(inc.scope.clone()).or_insert(0) += 1;
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
        let grace = confirm_ms(&rules);
        let mut st = self.state.lock().await;
        // Without the persisted rows, incident keys could repeat: no pass until they are read.
        if !self.restore(&mut st, now).await {
            return;
        }
        // The first pass only sets baselines: nothing it reads was drawn while we watched,
        // whatever a remote host's clock says.
        let since = st.watching_since.unwrap_or(i64::MAX);
        st.watching_since.get_or_insert(now);
        st.present = fleet.iter().map(|s| s.session_ref.clone()).collect();
        let mut obs: HashMap<SessionRef, Obs> = HashMap::new();
        for session in fleet {
            if session.stale {
                continue;
            }
            // A session showing a real prompt is excluded from fault detection (its card is
            // the owner's decision), but one with an outstanding resume, or a restarted stall
            // primary, has plainly started a new turn: that is progress for judging sends.
            if session.pending_prompt.is_some() {
                if st.sent.contains_key(&session.session_ref)
                    || st.restarted(&session.session_ref)
                {
                    let tail = self
                        .deps
                        .control
                        .read_tail(&session.session_ref, TAIL_LINES)
                        .await
                        .unwrap_or_default();
                    obs.insert(
                        session.session_ref.clone(),
                        Obs { session, tail, kind: HealthKind::Healthy, progress: true },
                    );
                }
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
            // A session seen for the first time (startup, or back after its baseline was
            // lost) only sets the baseline: old scrollback is never news (H2). A session first
            // seen after the first pass whose screen changed since then drew its tail while
            // we watched: a 429 it hit between two passes is news (F1).
            let mut base = st.base.get(&session.session_ref).cloned();
            if base.is_none()
                && !st.known.contains(&session.session_ref)
                && session.screen_changed_at_ms.is_some_and(|at| at > since)
            {
                base = Some(String::new());
                st.base.insert(session.session_ref.clone(), String::new());
            }
            st.known.insert(session.session_ref.clone());
            let (seen, progress) = match base {
                Some(base) => {
                    let fresh = fresh_lines(&base, &tail);
                    let text = resume_text_for(&rules, &session.agent).unwrap_or_default();
                    let progress = fresh
                        .iter()
                        .any(|l| !l.trim().is_empty() && !is_echo(l, &text));
                    (fresh.join("\n"), progress)
                }
                None => {
                    st.base.insert(session.session_ref.clone(), tail.clone());
                    (String::new(), false)
                }
            };
            let kind = self.deps.classifier.classify(&session, &seen, &rules, now);
            obs.insert(
                session.session_ref.clone(),
                Obs { session, tail, kind, progress },
            );
        }
        self.track_absence(&mut st, now, grace);
        // Dismissed sessions: their handled fault lines are consumed; once a pass reads them
        // healthy (or they are gone) the owner's choice ends.
        for r in st.dismissed.clone() {
            match obs.get(&r) {
                Some(o) if faulted(&o.kind) => {
                    st.base.insert(r, o.tail.clone());
                }
                Some(_) => {
                    st.dismissed.remove(&r);
                }
                None if st.gone(&r, now, grace) => {
                    st.dismissed.remove(&r);
                }
                None => {}
            }
        }
        self.refusals(&mut st, &obs, &rules, now).await;
        self.open_incidents(&mut st, &obs, &rules, now);
        let mut incidents = std::mem::take(&mut st.incidents);
        for inc in incidents.iter_mut() {
            if inc.closed {
                continue;
            }
            match inc.fault {
                Fault::Upstream => self.upstream(&mut st, inc, &obs, &rules, now).await,
                Fault::Quota => self.quota(&mut st, inc, &obs, &rules, now).await,
                Fault::Stall => self.stall(&mut st, inc, &obs, &rules, now).await,
            }
        }
        for inc in incidents.iter().filter(|inc| inc.closed) {
            st.bump(inc);
            // The handled fault lines are consumed: they never open the next incident. A
            // member that is faulted right now is never absorbed: its fresh error is news.
            for r in inc.ever.iter().chain([&inc.primary]) {
                if let Some(o) = obs.get(r) {
                    if !faulted(&o.kind) {
                        st.base.insert(r.clone(), o.tail.clone());
                    }
                }
            }
        }
        incidents.retain(|inc| !inc.closed);
        st.incidents = incidents;
        drop(st);
        self.run_goals(now).await;
    }

    /// Sessions out of the fleet keep their baseline and pending send for `grace` (so a
    /// transient listing gap never replays a resume or an old fault); then both are dropped.
    fn track_absence(&self, st: &mut State, now: i64, grace: i64) {
        let mut tracked: HashSet<SessionRef> = st.sent.keys().cloned().collect();
        tracked.extend(st.base.keys().cloned());
        tracked.extend(st.refused.keys().cloned());
        tracked.extend(st.dismissed.iter().cloned());
        for inc in &st.incidents {
            tracked.extend(inc.ever.iter().cloned());
            tracked.insert(inc.primary.clone());
        }
        for r in tracked.iter().filter(|r| !st.present.contains(*r)) {
            st.absent_since.entry(r.clone()).or_insert(now);
        }
        let present = st.present.clone();
        st.absent_since
            .retain(|r, _| !present.contains(r) && tracked.contains(r));
        let gone: Vec<SessionRef> = tracked
            .into_iter()
            .filter(|r| st.gone(r, now, grace))
            .collect();
        for r in gone {
            st.sent.remove(&r);
            st.base.remove(&r);
        }
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
            if !faulted(&o.kind)
                || resume_for(&o.session.agent).is_err()
                || st.dismissed.contains(r)
            {
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
            let key = format!("{CARD_PREFIX}{scope}:g{generation}");
            st.incidents.push(new_incident(key, scope, fault, o, at, now));
        }
    }

    pub(super) async fn row(
        &self,
        inc: &mut Incident,
        kind: ActionKind,
        summary: String,
        live: bool,
        now: i64,
    ) {
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
    /// `choices` says what tapping each option does (see [`Self::answer_card`]).
    pub(super) fn escalate(
        &self,
        inc: &mut Incident,
        question: String,
        options: [&str; 2],
        choices: [Choice; 2],
        reason: String,
        now: i64,
    ) {
        if inc.escalated.is_some() {
            return;
        }
        inc.escalated = Some(inc.primary.clone());
        inc.choices = choices.to_vec();
        self.deps.escalations.open(Escalation {
            session_ref: inc.primary.clone(),
            prompt: PendingPrompt {
                prompt_id: format!("{CARD_PREFIX}{}", inc.key),
                kind: "recovery".to_owned(),
                question,
                command: None,
                options: options.iter().map(|o| (*o).to_owned()).collect(),
                // Owner surfaces (HTTP, desktop) require a revision string on an answer; a
                // recovery card has no screen, so its id stands in and is never compared.
                screen_revision: Some(format!("{CARD_PREFIX}{}", inc.key)),
            },
            recommended_index: 0,
            reason,
            created_at_ms: now,
        });
    }

    pub(super) fn finish(&self, inc: &mut Incident) {
        if let Some(session) = inc.escalated.take() {
            self.deps.escalations.resolve(&session);
        }
        inc.closed = true;
    }

    /// Stop (optionally) then send the resume text. `stopped` records a delivered Ctrl-C: a
    /// failed send leaves it set, so the retry sends the text only and never interrupts the
    /// agent twice (L3). `false` when refused or failed.
    pub(super) async fn resume(
        &self,
        st: &mut State,
        o: &Obs,
        rules: &[StandingRule],
        stop_first: bool,
        stopped: &mut bool,
        now: i64,
    ) -> bool {
        if !self.go().await {
            return false;
        }
        let Some(text) = resume_text_for(rules, &o.session.agent) else {
            return false;
        };
        let r = &o.session.session_ref;
        if stop_first && !*stopped {
            if self.deps.control.stop(r).await.is_err() {
                return false;
            }
            *stopped = true;
        }
        if self.deps.control.send(r, &text).await.is_err() {
            return false;
        }
        *stopped = false;
        st.sent.insert(r.clone(), now);
        st.base.insert(r.clone(), o.tail.clone());
        true
    }

    /// An unknown agent is refused with one typed, owner-visible row and card; nothing is sent.
    /// Row and card are cleared when the session recovers, ends, or the owner answers (L1).
    async fn refusals(
        &self,
        st: &mut State,
        obs: &HashMap<SessionRef, Obs>,
        rules: &[StandingRule],
        now: i64,
    ) {
        let grace = confirm_ms(rules);
        let mut open: Vec<SessionRef> = st.refused.keys().cloned().collect();
        open.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        for r in open {
            // Cleared when the session moved on (fresh healthy output), became resumable, or
            // is gone; the raised fault line itself was consumed when the card opened.
            let cleared = match obs.get(&r) {
                Some(o) => {
                    resume_for(&o.session.agent).is_ok() || (o.progress && !faulted(&o.kind))
                }
                None => st.gone(&r, now, grace),
            };
            if !cleared {
                continue;
            }
            let Some(mut inc) = st.refused.remove(&r) else {
                continue;
            };
            let summary = format!("{} no longer needs recovery", inc.title);
            self.row(&mut inc, ActionKind::Finished, summary, false, now).await;
            self.finish(&mut inc);
            st.bump(&inc);
        }
        let mut refs: Vec<&SessionRef> = obs.keys().collect();
        refs.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        for r in refs {
            let o = &obs[r];
            let Err(refusal) = resume_for(&o.session.agent) else {
                continue;
            };
            if !faulted(&o.kind) || st.refused.contains_key(r) || st.dismissed.contains(r) {
                continue;
            }
            let reason = refusal.to_string();
            let scope = format!("refused:{r}");
            let generation = st.gens.get(&scope).copied().unwrap_or(0);
            let key = format!("{CARD_PREFIX}{scope}:g{generation}");
            let mut inc = new_incident(key, scope, Fault::Stall, o, now, now);
            self.escalate(
                &mut inc,
                format!("{} needs recovery but cannot be resumed automatically.", o.session.title),
                ["Open the session", "Leave it"],
                [Choice::Leave, Choice::Leave],
                format!("{reason}; nothing was sent"),
                now,
            );
            let summary = format!("Left {} alone: {reason}", o.session.title);
            self.row(&mut inc, ActionKind::Escalated, summary, true, now).await;
            st.base.insert(r.clone(), o.tail.clone());
            st.refused.insert(r.clone(), inc);
        }
    }
}
