//! Phase 1 acceptance harness: proves scrollback budget pressure actually evicts.
//!
//! Deterministic by construction: the budget samples no clock (a monotonic focus stamp and an
//! eviction counter only), no PTY is spawned, no subprocess runs and nothing sleeps. Every
//! assertion reads observable accounting after driving the production `drain_trims` loop with a
//! synthetic producer. The 60-minute footprint measurement is a soak, not a unit test, and stays out.

use std::collections::HashMap;

use super::preferences::{DEFAULT_SCROLLBACK_BUDGET_BYTES, MIN_SESSION_HISTORY_BYTES};
use super::scrollback_budget::{max_concurrent_sessions, Eviction, ScrollbackBudget, Trim};
use super::PtyError;

/// Stand-in for one session's retained `RetainedHistory`: a byte count a trim can shrink, exactly
/// as `RetainedHistory::set_budget` shrinks a real session. The budget is driven through its own
/// `drain_trims`, so the loop under test is the production loop - only the storage is synthetic.
#[derive(Default)]
struct SyntheticHistory {
    held: HashMap<String, usize>,
}

impl SyntheticHistory {
    fn hold(&mut self, session_id: &str, retained_bytes: usize) {
        self.held.insert(session_id.to_string(), retained_bytes);
    }

    fn trim(&mut self, trim: &Trim) -> usize {
        let held = self.held.entry(trim.session_id.clone()).or_default();
        *held = (*held).min(trim.target_bytes);
        *held
    }

    /// Reports what the producer holds into the budget, the way `account_retained_history` does
    /// after every published chunk.
    fn account(&self, budget: &mut ScrollbackBudget) {
        for (session_id, held) in &self.held {
            budget.note_retained(session_id, *held);
        }
    }

    /// Runs the budget's own trim plan to completion against this producer.
    fn apply_all(&mut self, budget: &mut ScrollbackBudget) -> Vec<Eviction> {
        budget
            .drain_trims(|trim| Ok::<usize, ()>(self.trim(trim)))
            .expect("the synthetic producer never fails a trim")
    }
}

fn budget_with_sessions(
    budget_bytes: usize,
    min_session_bytes: usize,
    count: usize,
) -> (ScrollbackBudget, Vec<String>) {
    let mut budget = ScrollbackBudget::new_with_floor(budget_bytes, min_session_bytes);
    let ids: Vec<String> = (0..count).map(|index| format!("s{index}")).collect();
    for id in &ids {
        budget.admit(id).expect("every session is within the admission limit");
    }
    (budget, ids)
}

/// (a) Mixed active/inactive: inactive sessions evict first, active second, the focused pane last -
/// last, never exempt - and the aggregate never exceeds the budget.
#[test]
fn inactive_evicted_before_active() {
    // 7 sessions: s0..s3 never touched (inactive, oldest stamps), s4 focused once then blurred
    // (inactive with the newest stamp), s5 active but not in view, s6 the pane in view.
    let (mut budget, ids) =
        budget_with_sessions(DEFAULT_SCROLLBACK_BUDGET_BYTES, MIN_SESSION_HISTORY_BYTES, 7);
    budget.note_focus("s4");
    budget.note_blur("s4");
    budget.note_focus("s5");
    budget.note_focus("s6");

    assert_eq!(
        budget.eviction_order(),
        ids,
        "least-recently-focused inactive first, then active, then the focused pane"
    );

    let ceiling = budget.ceiling_bytes();
    assert!(ceiling > MIN_SESSION_HISTORY_BYTES, "7 sessions of 1.5 GiB exceed the floor each");

    // Every pane holds twice its share, as an imported handover can, so the daemon total is past
    // the budget before any trim runs.
    let mut history = SyntheticHistory::default();
    for id in &ids {
        history.hold(id, ceiling * 2);
    }
    history.account(&mut budget);
    assert_eq!(budget.eviction_count(), 0, "no trim is counted before the plan runs");
    assert_eq!(budget.evicted_bytes(), 0);
    assert!(budget.total_retained_bytes() > budget.budget_bytes(), "pressure is real");
    assert!(budget.next_trim().is_some(), "the budget demands a trim");

    let evictions = history.apply_all(&mut budget);

    assert_eq!(evictions.len(), ids.len(), "every over-share pane is trimmed");
    assert_eq!(
        evictions.iter().map(|e| e.session_id.as_str()).collect::<Vec<_>>(),
        ids.iter().map(String::as_str).collect::<Vec<_>>(),
        "eviction order is the declared order: inactive, then active, then focused"
    );
    assert_eq!(
        evictions.last().map(|e| e.session_id.as_str()),
        Some("s6"),
        "the focused pane is last"
    );
    for (index, eviction) in evictions.iter().enumerate() {
        assert_eq!(eviction.ceiling_bytes, ceiling);
        assert_eq!(eviction.released_bytes, ceiling, "each pane gives exactly its excess");
        assert_eq!(eviction.eviction_count, index as u64 + 1, "the counter is monotonic");
    }
    assert_eq!(budget.eviction_count(), ids.len() as u64);
    assert_eq!(budget.evicted_bytes(), (ceiling * ids.len()) as u64);
    assert!(
        budget.total_retained_bytes() <= budget.budget_bytes(),
        "the aggregate stays inside the budget"
    );
    for id in &ids {
        assert_eq!(budget.retained_bytes(id), ceiling, "{id} is at its ceiling, not past it");
        assert!(budget.retained_bytes(id) <= budget.ceiling_bytes());
    }
}

/// (b) All sessions active at the admission limit: the aggregate stays within the budget, no
/// session passes its own ceiling, and the next admission is refused rather than over-allocated.
#[test]
fn all_sessions_active_stays_within_budget() {
    let budget_bytes = DEFAULT_SCROLLBACK_BUDGET_BYTES;
    let mut budget = ScrollbackBudget::new(budget_bytes);
    let limit = budget.admission_limit();
    // 192 at the production figures (1536 MiB / 8 MiB); asserted against the arithmetic, never a
    // literal, so a budget change moves the limit with it.
    assert_eq!(limit, max_concurrent_sessions(budget_bytes));
    assert_eq!(limit, budget_bytes / MIN_SESSION_HISTORY_BYTES);

    let ids: Vec<String> = (0..limit).map(|index| format!("s{index}")).collect();
    for id in &ids {
        budget.admit(id).expect("admit within the limit");
    }
    for id in &ids {
        budget.note_focus(id);
    }
    assert_eq!(budget.live_sessions(), limit, "every session is live and active");

    let ceiling = budget.ceiling_bytes();
    assert_eq!(ceiling, MIN_SESSION_HISTORY_BYTES, "at the limit each share is the floor");

    let mut history = SyntheticHistory::default();
    for id in &ids {
        history.hold(id, ceiling);
    }
    history.account(&mut budget);
    assert!(history.apply_all(&mut budget).is_empty(), "at its own ceiling nothing is trimmed");
    assert_eq!(budget.eviction_count(), 0);
    assert!(budget.total_retained_bytes() <= budget.budget_bytes());

    // Every pane is active, so the order is the focus stamps alone: the oldest gives first.
    assert_eq!(
        budget.eviction_order(),
        ids,
        "every pane is active, so the least recently focused gives first"
    );

    // One active pane is pushed past its share: the budget trims it back instead of letting the
    // aggregate grow.
    let victim = ids[0].clone();
    history.hold(&victim, ceiling * 2);
    history.account(&mut budget);
    let evictions = history.apply_all(&mut budget);
    assert_eq!(evictions.len(), 1, "only the over-share pane is trimmed");
    assert_eq!(evictions[0].session_id, victim);
    assert_eq!(evictions[0].ceiling_bytes, ceiling);
    assert_eq!(evictions[0].released_bytes, ceiling);
    assert_eq!(budget.retained_bytes(&victim), ceiling);
    assert!(budget.total_retained_bytes() <= budget.budget_bytes(), "the aggregate is bounded");

    for id in &ids {
        let retained = budget.retained_bytes(id);
        assert!(retained <= budget.ceiling_bytes(), "{id} exceeds its own ceiling: {retained}");
    }
    assert_eq!(budget.next_trim(), None, "nothing is left to trim");

    // The refusal path is exercised, not exceeded: session N+1 is refused by name.
    let beyond = format!("s{limit}");
    let error = budget.admit(&beyond).expect_err("beyond the limit is refused");
    assert!(matches!(&error, PtyError::SessionCapacityExhausted { .. }));
    assert_eq!(budget.live_sessions(), limit, "a refusal admits nothing");
    assert_eq!(budget.retained_bytes(&beyond), 0, "no session was given a silent allotment");
}

/// (c) Floor exhaustion: every session sits at the 8 MiB floor, a further allocation is trimmed
/// back to the floor, and one more session is refused with the exact typed code - not by
/// panicking and not by over-allocating.
#[test]
fn floor_exhaustion_refuses_typed() {
    let budget_bytes = DEFAULT_SCROLLBACK_BUDGET_BYTES;
    let min_session_bytes = MIN_SESSION_HISTORY_BYTES;
    let (mut budget, ids) = budget_with_sessions(
        budget_bytes,
        min_session_bytes,
        max_concurrent_sessions(budget_bytes),
    );
    let limit = ids.len();
    let focused = ids[limit - 1].clone();
    budget.note_focus(&focused);

    let mut history = SyntheticHistory::default();
    for id in &ids {
        history.hold(id, min_session_bytes);
    }
    history.account(&mut budget);
    assert!(history.apply_all(&mut budget).is_empty(), "every session is already at the floor");
    assert_eq!(budget.eviction_count(), 0, "no eviction is counted before any pressure");
    assert_eq!(budget.evicted_bytes(), 0);
    assert_eq!(
        budget.total_retained_bytes(),
        budget.budget_bytes(),
        "the floor fills the budget exactly: there is no spare byte to grant"
    );

    // A further allocation on the focused pane cannot grow the aggregate: it is held to its own
    // floor, and the trim is counted, not swallowed. No panic is a passing outcome here.
    history.hold(&focused, min_session_bytes * 2);
    history.account(&mut budget);
    let evictions = history.apply_all(&mut budget);
    assert_eq!(evictions.len(), 1, "the over-floor pane is trimmed once");
    assert_eq!(evictions[0].session_id, focused);
    assert_eq!(evictions[0].ceiling_bytes, min_session_bytes, "the floor is the target");
    assert_eq!(evictions[0].released_bytes, min_session_bytes);
    assert_eq!(budget.retained_bytes(&focused), min_session_bytes);
    assert_eq!(budget.eviction_count(), 1);
    assert_eq!(budget.evicted_bytes(), min_session_bytes as u64);
    assert_eq!(budget.total_retained_bytes(), budget.budget_bytes(), "still exactly the budget");

    // The typed refusal, exact code and all: the whole rendered message must match, not a
    // substring, so a changed code or a reformatted limit fails this test.
    let expected = format!(
        "SESSION_CAPACITY_EXHAUSTED: this daemon admits at most {limit} concurrent sessions \
         (scrollback budget {budget_bytes} bytes / {min_session_bytes} bytes minimum history)"
    );
    let error = budget
        .admit(&format!("s{limit}"))
        .expect_err("a session beyond the limit is refused");
    match &error {
        PtyError::SessionCapacityExhausted {
            limit: refused_limit,
            budget_bytes: refused_budget,
            min_session_bytes: refused_min,
        } => {
            assert_eq!(*refused_limit, limit);
            assert_eq!(*refused_budget, budget_bytes);
            assert_eq!(*refused_min, min_session_bytes);
        }
        other => panic!("expected the typed capacity refusal, got: {other:?}"),
    }
    assert_eq!(error.to_string(), expected, "the exact SESSION_CAPACITY_EXHAUSTED code");
    // `capacity_error()` is the same refusal the seam reports, and the refused admission changed
    // neither the live count nor the retained total.
    let reported = budget.capacity_error();
    assert!(matches!(&reported, PtyError::SessionCapacityExhausted { .. }));
    assert_eq!(reported.to_string(), expected);
    assert_eq!(budget.live_sessions(), limit);
    assert_eq!(budget.total_retained_bytes(), budget.budget_bytes());
    assert_eq!(budget.eviction_count(), 1, "a refusal is not an eviction");
}

/// (d) The focused pane is last, never exempt: alone with the whole budget to itself, it is still
/// trimmed to its own ceiling. Otherwise "the focused pane is evicted last" could be read as "the
/// focused pane is protected", which would ship an unbounded path.
#[test]
fn a_sole_focused_session_is_trimmed_to_its_own_ceiling() {
    let (mut budget, ids) = budget_with_sessions(
        DEFAULT_SCROLLBACK_BUDGET_BYTES,
        MIN_SESSION_HISTORY_BYTES,
        1,
    );
    let only = ids[0].clone();
    budget.note_focus(&only);
    assert_eq!(budget.ceiling_bytes(), DEFAULT_SCROLLBACK_BUDGET_BYTES);

    let mut history = SyntheticHistory::default();
    history.hold(&only, DEFAULT_SCROLLBACK_BUDGET_BYTES * 2);
    history.account(&mut budget);

    let evictions = history.apply_all(&mut budget);
    assert_eq!(evictions.len(), 1);
    assert_eq!(evictions[0].session_id, only);
    assert_eq!(evictions[0].ceiling_bytes, DEFAULT_SCROLLBACK_BUDGET_BYTES);
    assert_eq!(evictions[0].released_bytes, DEFAULT_SCROLLBACK_BUDGET_BYTES);
    assert_eq!(budget.retained_bytes(&only), DEFAULT_SCROLLBACK_BUDGET_BYTES);
}

/// Counters only ever grow, and start at zero: a double-counted or reset counter would make every
/// pressure assertion above meaningless.
#[test]
fn eviction_counters_only_grow_and_start_at_zero() {
    let (mut budget, ids) = budget_with_sessions(4 * 1024 * 1024, 1024 * 1024, 2);
    assert_eq!(budget.eviction_count(), 0);
    assert_eq!(budget.evicted_bytes(), 0);

    let ceiling = budget.ceiling_bytes();
    assert_eq!(ceiling, 2 * 1024 * 1024, "4 MiB shared by 2 sessions, above a 1 MiB floor");

    let mut history = SyntheticHistory::default();
    for id in &ids {
        history.hold(id, ceiling);
    }
    history.account(&mut budget);
    assert!(history.apply_all(&mut budget).is_empty());
    assert_eq!(budget.eviction_count(), 0, "filling to the ceiling is not pressure");

    let mib = 1024 * 1024;
    let (mut last_count, mut last_bytes) = (0u64, 0u64);
    for round in 1..=4u64 {
        // Each round pushes the pane `round` MiB past its ceiling; the trim releases exactly that.
        history.hold(&ids[0], ceiling + round as usize * mib);
        history.account(&mut budget);
        let evictions = history.apply_all(&mut budget);
        assert_eq!(evictions.len(), 1, "round {round}: one pane is over its share");
        assert_eq!(evictions[0].released_bytes, round as usize * mib);
        assert!(budget.eviction_count() > last_count, "round {round}: the count grows");
        assert!(budget.evicted_bytes() > last_bytes, "round {round}: the byte total grows");
        assert_eq!(budget.retained_bytes(&ids[0]), ceiling, "round {round}: trimmed back");
        last_count = budget.eviction_count();
        last_bytes = budget.evicted_bytes();
    }
    assert_eq!(last_count, 4);
    assert_eq!(last_bytes, (1 + 2 + 3 + 4) * mib as u64, "the total is every release summed");
    assert!(budget.total_retained_bytes() <= budget.budget_bytes());
}
