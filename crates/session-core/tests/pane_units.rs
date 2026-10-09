use session_core::pane_lease::{Out, PaneLease, Policy};

fn focused(session: u64) -> Policy {
    Policy { bound_session: Some(session), generation: 1, visible_focused: true, visible_applied_generation: Some(1), gpu_ready: true, tombstoned: false }
}

fn acquire_id(out: &[Out]) -> u64 {
    out.iter().find_map(|o| match o { Out::Acquire { id, .. } => Some(*id), _ => None }).unwrap_or_else(|| panic!("no Acquire in {out:?}"))
}

#[test]
fn acquire_that_times_out_is_released_with_confirmation_once_unwanted() {
    let mut p = PaneLease::new();
    p.set_policy(focused(4), true);
    p.on_visible_applied(80, 24);
    let id = acquire_id(&p.take_out());
    p.on_request_failed(id);
    assert!(p.disowned.contains(&4), "an Acquire that ended without a Grant may have been granted");
    p.set_policy(Policy { visible_focused: false, ..focused(4) }, false);
    p.on_retry_timer();
    let out = p.take_out();
    let release = out.iter().find_map(|o| match o { Out::Release { id, session: 4, epoch: 0 } => Some(*id), _ => None });
    let release = release.unwrap_or_else(|| panic!("expected a confirming Release{{epoch: 0}}, got {out:?}"));
    p.on_released(release);
    assert!(p.disowned.is_empty() && p.pending.is_none());
}

#[test]
fn acquire_that_times_out_is_superseded_by_a_kept_grant() {
    let mut p = PaneLease::new();
    p.set_policy(focused(4), true);
    p.on_visible_applied(80, 24);
    let first = acquire_id(&p.take_out());
    p.on_request_failed(first);
    p.on_retry_timer();
    let second = acquire_id(&p.take_out());
    p.on_granted(second, 4, 9, 0);
    assert_eq!(p.lease.map(|l| l.epoch), Some(9));
    assert!(p.disowned.is_empty(), "a kept Grant on the session clears its disowned entry");
}
