//! Daemon-wide retained-scrollback budget: admission ceiling, per-session ceiling, eviction.
//!
//! This bounds *retained scrollback* only. Live screens, graphics, replay logs, queues,
//! snapshots and allocator overhead are bounded independently (the keeper replay cap in Phase 3,
//! and the output hub's broadcast/raw capacities), so a moved allocation cannot hide in here.
//!
//! The budget decides *what* to trim and in which order; the output hub performs each trim on the
//! session's retained history and reports the result back. One algorithm, one place.
//!
//! Determinism: every decision is arithmetic over recorded state and an explicit focus stamp. No
//! clock is sampled, so the budget-pressure tests assert on the eviction counter instead of
//! wall-clock behaviour, and they stay separate from the 60-minute footprint soak.

use std::collections::HashMap;

use super::preferences::{DEFAULT_SCROLLBACK_BUDGET_BYTES, MIN_SESSION_HISTORY_BYTES};
use super::PtyError;

/// Admission limit for `budget_bytes`: a deployment that shrinks the budget gets a
/// proportionally lower session limit rather than an undefined state.
pub fn max_concurrent_sessions(budget_bytes: usize) -> usize {
    (budget_bytes / MIN_SESSION_HISTORY_BYTES).max(1)
}

/// Largest retained history one session may hold while `live_sessions` sessions are admitted.
///
/// Floored at `min_session_bytes` and kept strictly above libghostty-vt's own ~10 KiB default,
/// which is what motivated replacing that byte cap: a ceiling at or below that value would silently
/// restore the premature pruning the change removed.
pub fn session_ceiling_bytes_with_floor(
    budget_bytes: usize,
    live_sessions: usize,
    min_session_bytes: usize,
) -> usize {
    let share = budget_bytes / live_sessions.max(1);
    share
        .max(min_session_bytes)
        .max(super::native_scrollback_floor_bytes())
}

/// [`session_ceiling_bytes_with_floor`] at the production floor.
pub fn session_ceiling_bytes(budget_bytes: usize, live_sessions: usize) -> usize {
    session_ceiling_bytes_with_floor(budget_bytes, live_sessions, MIN_SESSION_HISTORY_BYTES)
}

/// One session's share of the daemon-wide budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SessionAllotment {
    retained_bytes: usize,
    /// Monotonic focus stamp, lower is older. Never a timestamp.
    last_focus_tick: u64,
    /// The user is looking at this session right now. Active sessions are evicted second.
    active: bool,
}

/// One trim still to be performed: which session, and the retained bytes it must be left with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trim {
    pub session_id: String,
    pub target_bytes: usize,
}

/// A trim that was performed, and the `scrollback_budget_exhausted` report for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Eviction {
    pub session_id: String,
    pub ceiling_bytes: usize,
    pub released_bytes: usize,
    /// Monotonic count of trims since this daemon started.
    pub eviction_count: u64,
}

/// Retained-scrollback accounting for every live session of one daemon.
#[derive(Debug, Clone)]
pub struct ScrollbackBudget {
    budget_bytes: usize,
    min_session_bytes: usize,
    focused: Option<String>,
    tick: u64,
    sessions: HashMap<String, SessionAllotment>,
    evictions: u64,
    evicted_bytes: u64,
}

impl Default for ScrollbackBudget {
    fn default() -> Self {
        Self::new(DEFAULT_SCROLLBACK_BUDGET_BYTES)
    }
}

impl ScrollbackBudget {
    pub fn new(budget_bytes: usize) -> Self {
        Self::new_with_floor(budget_bytes, MIN_SESSION_HISTORY_BYTES)
    }

    /// The same accounting with an explicit floor. Production uses
    /// [`MIN_SESSION_HISTORY_BYTES`]; tests and deliberately tiny configurations may pick a
    /// smaller one. The per-session ceiling is still never allowed to reach libghostty's default.
    pub fn new_with_floor(budget_bytes: usize, min_session_bytes: usize) -> Self {
        Self {
            budget_bytes,
            min_session_bytes,
            focused: None,
            tick: 0,
            sessions: HashMap::new(),
            evictions: 0,
            evicted_bytes: 0,
        }
    }

    pub fn budget_bytes(&self) -> usize {
        self.budget_bytes
    }

    pub fn min_session_bytes(&self) -> usize {
        self.min_session_bytes
    }

    pub fn live_sessions(&self) -> usize {
        self.sessions.len()
    }

    /// How many trims the budget forced since this daemon started.
    pub fn eviction_count(&self) -> u64 {
        self.evictions
    }

    /// Retained bytes released by those trims.
    pub fn evicted_bytes(&self) -> u64 {
        self.evicted_bytes
    }

    pub fn total_retained_bytes(&self) -> usize {
        self.sessions.values().map(|s| s.retained_bytes).sum()
    }

    pub fn retained_bytes(&self, session_id: &str) -> usize {
        self.sessions
            .get(session_id)
            .map_or(0, |allotment| allotment.retained_bytes)
    }

    /// How many sessions this budget admits: `floor(budget / min_session_bytes)`, and never zero,
    /// so a deliberately small configuration still admits one session.
    pub fn admission_limit(&self) -> usize {
        (self.budget_bytes / self.min_session_bytes.max(1)).max(1)
    }

    /// The typed refusal for a session beyond the admission limit. It names the limit: a session
    /// is never admitted with a silent zero-history allotment.
    pub fn capacity_error(&self) -> PtyError {
        PtyError::SessionCapacityExhausted {
            limit: self.admission_limit(),
            budget_bytes: self.budget_bytes,
            min_session_bytes: self.min_session_bytes,
        }
    }

    /// Every session's hard retained-history bound right now: `budget / live_sessions`, floored.
    pub fn ceiling_bytes(&self) -> usize {
        session_ceiling_bytes_with_floor(
            self.budget_bytes,
            self.sessions.len(),
            self.min_session_bytes,
        )
    }

    /// Admits a new session, or refuses it with [`PtyError::SessionCapacityExhausted`].
    ///
    /// Re-admitting a live session is a no-op, so a handover or a retried a spawn is idempotent.
    /// Admission shares the budget with one more session, which lowers every survivor's ceiling;
    /// [`Self::next_trim`] reports the trims that follow.
    pub fn admit(&mut self, session_id: &str) -> Result<(), PtyError> {
        if self.sessions.contains_key(session_id) {
            return Ok(());
        }
        if self.sessions.len() >= self.admission_limit() {
            return Err(self.capacity_error());
        }
        self.tick += 1;
        self.sessions.insert(
            session_id.to_string(),
            SessionAllotment {
                retained_bytes: 0,
                last_focus_tick: self.tick,
                active: false,
            },
        );
        Ok(())
    }

    /// Records a session this daemon did not admit through [`Self::admit`] (an adopted handover
    /// session, or a caller that registers output before its spawn gate). Admission is not
    /// re-checked here: the caller's spawn already passed it or the session already exists.
    pub fn track(&mut self, session_id: &str) {
        if self.sessions.contains_key(session_id) {
            return;
        }
        self.tick += 1;
        self.sessions.insert(
            session_id.to_string(),
            SessionAllotment {
                retained_bytes: 0,
                last_focus_tick: self.tick,
                active: false,
            },
        );
    }

    pub fn release(&mut self, session_id: &str) {
        self.sessions.remove(session_id);
        if self.focused.as_deref() == Some(session_id) {
            self.focused = None;
        }
    }

    /// The session gained the user's attention: its focus stamp becomes the newest and it is
    /// active, so only an older active session can be evicted before it.
    pub fn note_focus(&mut self, session_id: &str) {
        self.tick += 1;
        let tick = self.tick;
        let Some(allotment) = self.sessions.get_mut(session_id) else {
            return;
        };
        allotment.last_focus_tick = tick;
        allotment.active = true;
        self.focused = Some(session_id.to_string());
    }

    /// The pane lost focus: it is inactive again and no longer the marker that sorts last.
    pub fn note_blur(&mut self, session_id: &str) {
        if let Some(allotment) = self.sessions.get_mut(session_id) {
            allotment.active = false;
        }
        if self.focused.as_deref() == Some(session_id) {
            self.focused = None;
        }
    }

    /// Records what the session's retained history currently holds.
    pub fn note_retained(&mut self, session_id: &str, retained_bytes: usize) {
        self.track(session_id);
        if let Some(allotment) = self.sessions.get_mut(session_id) {
            allotment.retained_bytes = retained_bytes;
        }
    }

    /// Eviction order: least-recently-focused inactive first, then least-recently-focused active,
    /// and the focused session last - last, never exempt.
    pub fn eviction_order(&self) -> Vec<String> {
        let mut order: Vec<(bool, bool, u64, String)> = self
            .sessions
            .iter()
            .map(|(id, allotment)| {
                (
                    self.focused.as_deref() == Some(id.as_str()),
                    allotment.active,
                    allotment.last_focus_tick,
                    id.clone(),
                )
            })
            .collect();
        order.sort_by(|left, right| {
            (left.0, left.1, left.2, &left.3).cmp(&(right.0, right.1, right.2, &right.3))
        });
        order.into_iter().map(|(_, _, _, id)| id).collect()
    }

    /// The next trim the budget requires, or `None` when everything fits.
    ///
    /// Two bounds, in this order: every session is held to `budget / live_sessions` (the hard
    /// per-session ceiling, which admission lowers and removal raises), and then, if the daemon
    /// total is still above the budget, sessions are trimmed further down to
    /// [`Self::min_session_bytes`]. Both stages use [`Self::eviction_order`], so the focused pane
    /// is the last candidate in either - and when it is the only history above the target it is
    /// trimmed by the same rule as any other. Nothing here is exempt, so no unbounded path exists.
    pub fn next_trim(&self) -> Option<Trim> {
        self.next_trim_excluding(&std::collections::HashSet::new())
    }

    /// [`Self::next_trim`], skipping sessions the caller already found it cannot trim (a session
    /// whose whole retained history is the unit it is still assembling, for example). Skipping
    /// keeps one stuck session from hiding the other victims; the skipped session keeps its
    /// ceiling, so the per-session bound still holds while the rest are brought down.
    pub fn next_trim_excluding(
        &self,
        skip: &std::collections::HashSet<String>,
    ) -> Option<Trim> {
        let ceiling = self.ceiling_bytes();
        let order: Vec<String> = self
            .eviction_order()
            .into_iter()
            .filter(|session_id| !skip.contains(session_id))
            .collect();
        for session_id in &order {
            let retained = self.retained_bytes(session_id);
            if retained > ceiling {
                return Some(Trim {
                    session_id: session_id.clone(),
                    target_bytes: ceiling,
                });
            }
        }
        if self.total_retained_bytes() > self.budget_bytes {
            for session_id in &order {
                if self.retained_bytes(session_id) > self.min_session_bytes {
                    return Some(Trim {
                        session_id: session_id.clone(),
                        target_bytes: self.min_session_bytes,
                    });
                }
            }
        }
        None
    }

    /// Records a trim the caller performed: `retained_after` is what the session holds now.
    /// A trim that released nothing is not counted: the eviction counter means "history was
    /// actually dropped", which is the signal the pressure tests assert on.
    pub fn record_trim(
        &mut self,
        session_id: &str,
        ceiling_bytes: usize,
        retained_after: usize,
    ) -> Eviction {
        let released = {
            let Some(allotment) = self.sessions.get_mut(session_id) else {
                return Eviction {
                    session_id: session_id.to_string(),
                    ceiling_bytes,
                    released_bytes: 0,
                    eviction_count: self.evictions,
                };
            };
            let released = allotment.retained_bytes.saturating_sub(retained_after);
            allotment.retained_bytes = retained_after.min(allotment.retained_bytes);
            released
        };
        if released > 0 {
            self.evictions += 1;
            self.evicted_bytes = self.evicted_bytes.saturating_add(released as u64);
        }
        Eviction {
            session_id: session_id.to_string(),
            ceiling_bytes,
            released_bytes: released,
            eviction_count: self.evictions,
        }
    }

    /// Drives [`Self::next_trim`] to completion using `apply` to perform each trim and report
    /// what the session holds afterwards. Used by the hub and by the tests, so both exercise the
    /// same loop.
    pub fn drain_trims<E>(
        &mut self,
        mut apply: impl FnMut(&Trim) -> Result<usize, E>,
    ) -> Result<Vec<Eviction>, E> {
        let mut evictions = Vec::new();
        let mut stuck = std::collections::HashSet::new();
        // Every iteration either strictly lowers one session's retained bytes or retires that
        // session from this pass, so this terminates.
        while let Some(trim) = self.next_trim_excluding(&stuck) {
            let retained_after = apply(&trim)?;
            if retained_after >= self.retained_bytes(&trim.session_id) {
                stuck.insert(trim.session_id);
                continue;
            }
            evictions.push(self.record_trim(
                &trim.session_id,
                trim.target_bytes,
                retained_after,
            ));
        }
        Ok(evictions)
    }
}

#[cfg(test)]
mod tests {
    use super::super::preferences::MAX_CONCURRENT_SESSIONS;
    use super::*;

    // Pure arithmetic and admission accounting. The eviction-order cases live in
    // `terminal::scrollback_budget_tests`, so the two files cannot silently drift apart on the
    // expectations each is responsible for.

    fn budget_with(sessions: &[&str]) -> ScrollbackBudget {
        let mut budget = ScrollbackBudget::new(DEFAULT_SCROLLBACK_BUDGET_BYTES);
        for session in sessions {
            budget.admit(session).expect("admit");
        }
        budget
    }

    #[test]
    fn admission_limit_follows_the_budget_and_session_193_is_refused_by_name() {
        let mut budget = ScrollbackBudget::new(DEFAULT_SCROLLBACK_BUDGET_BYTES);
        assert_eq!(budget.admission_limit(), MAX_CONCURRENT_SESSIONS);
        assert_eq!(MAX_CONCURRENT_SESSIONS, 192);

        for index in 0..192 {
            budget.admit(&format!("s{index}")).expect("admit within limit");
        }

        let error = budget.admit("s192").expect_err("session 193 is refused");
        assert!(
            matches!(
                error,
                PtyError::SessionCapacityExhausted { limit: 192, budget_bytes, min_session_bytes }
                    if budget_bytes == DEFAULT_SCROLLBACK_BUDGET_BYTES
                        && min_session_bytes == MIN_SESSION_HISTORY_BYTES
            ),
            "expected a typed capacity refusal, got: {error:?}"
        );
        assert!(error.to_string().contains("SESSION_CAPACITY_EXHAUSTED"));
        assert!(error.to_string().contains("192 concurrent sessions"));
        assert_eq!(budget.live_sessions(), 192);

        // A reduced budget yields a proportionally lower limit, not an undefined state.
        assert_eq!(ScrollbackBudget::new(256 * 1024 * 1024).admission_limit(), 32);
    }

    #[test]
    fn ceiling_divides_the_budget_and_never_drops_to_the_libghostty_default() {
        let budget = ScrollbackBudget::new(DEFAULT_SCROLLBACK_BUDGET_BYTES);
        assert_eq!(budget.ceiling_bytes(), DEFAULT_SCROLLBACK_BUDGET_BYTES);
        assert_eq!(
            session_ceiling_bytes(DEFAULT_SCROLLBACK_BUDGET_BYTES, 192),
            MIN_SESSION_HISTORY_BYTES
        );
        assert_eq!(
            session_ceiling_bytes(DEFAULT_SCROLLBACK_BUDGET_BYTES, 2),
            768 * 1024 * 1024
        );
        // Below the floor the ceiling is the floor, and it always clears libghostty's default.
        assert_eq!(session_ceiling_bytes(1024, 4), MIN_SESSION_HISTORY_BYTES);
        assert!(session_ceiling_bytes(1024, 4) > super::super::native_scrollback_floor_bytes());
        assert_eq!(
            session_ceiling_bytes_with_floor(1024, 4, 512 * 1024),
            512 * 1024
        );
    }

    #[test]
    fn a_tight_budget_admits_only_what_its_floor_covers() {
        // The admission limit divides by the configured floor, so a small budget is coherent
        // rather than admitting more sessions than its floor can cover.
        let mut budget = ScrollbackBudget::new_with_floor(4 * 1024 * 1024, 1024 * 1024);
        assert_eq!(budget.admission_limit(), 4);
        for index in 0..4 {
            budget.admit(&format!("s{index}")).expect("admit within limit");
        }
        let error = budget.admit("s4").expect_err("session 5 is refused");
        assert!(matches!(
            error,
            PtyError::SessionCapacityExhausted { limit: 4, .. }
        ));
        assert_eq!(budget.ceiling_bytes(), 1024 * 1024);
    }

    #[test]
    fn releasing_a_session_raises_the_remaining_ceilings_again() {
        let mut budget = budget_with(&["a", "b"]);
        assert_eq!(budget.ceiling_bytes(), DEFAULT_SCROLLBACK_BUDGET_BYTES / 2);
        budget.release("b");
        assert_eq!(budget.ceiling_bytes(), DEFAULT_SCROLLBACK_BUDGET_BYTES);
        assert_eq!(budget.live_sessions(), 1);
        // Re-admitting the same id is idempotent: a retried spawn must not double-count.
        budget.admit("a").expect("idempotent");
        assert_eq!(budget.live_sessions(), 1);
    }
}
