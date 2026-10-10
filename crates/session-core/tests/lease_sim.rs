mod common;

use proptest::prelude::*;
use session_core::pane_lease::{Out, PaneLease, Policy};
use session_core::resize::{ResizeError, ResizeLease};

const SESSION: u64 = 1;

#[derive(Debug, Clone)]
enum ToHost {
    Acquire(u64),
    Reclaim(u64, u64),
    Release(u64, u64),
    Resize(u64, u64, u16, u16),
}

#[derive(Debug, Clone)]
enum ToPane {
    Granted(u64, u64, u64),
    Reclaimed(u64, bool, u64, u64),
    Released(u64),
    Revoked(u64, u64),
    Vacated,
    Ack(u64, u64),
    StaleResize(u64, u64),
    StaleLease(u64, u64),
    Failed(u64),
}

struct Sim {
    host: ResizeLease,
    panes: Vec<PaneLease>,
    conn: Vec<u64>,
    net: Vec<(usize, u64, Result<ToHost, ToPane>)>,
    req_conn: u64,
    grants_after_quiet: u32,
    quiet: bool,
}

fn who(i: usize) -> [u8; 16] {
    [i as u8 + 1; 16]
}

impl Sim {
    fn flush(&mut self, i: usize) {
        for o in self.panes[i].take_out() {
            let m = match o {
                Out::Acquire { id, .. } => ToHost::Acquire(id),
                Out::Reclaim { id, epoch, .. } => ToHost::Reclaim(id, epoch),
                Out::Release { id, epoch, .. } => ToHost::Release(id, epoch),
                Out::Resize { epoch, seq, cols, rows, .. } => ToHost::Resize(epoch, seq, cols, rows),
            };
            self.net.push((i, self.conn[i], Ok(m)));
        }
    }

    // A response travels back on the connection its request arrived on and is
    // lost with it; events go to the instance's current connection.
    fn reply(&mut self, i: usize, m: ToPane) {
        self.net.push((i, self.req_conn, Err(m)));
    }

    fn event(&mut self, i: usize, m: ToPane) {
        self.net.push((i, self.conn[i], Err(m)));
    }

    fn host(&mut self, i: usize, m: ToHost, fail: bool) {
        let req = match &m {
            ToHost::Acquire(id) | ToHost::Reclaim(id, _) | ToHost::Release(id, _) => Some(*id),
            ToHost::Resize(..) => None,
        };
        if let Some(id) = req {
            if !self.host.fresh(who(i), id) {
                return;
            }
            if fail {
                return self.reply(i, ToPane::Failed(id));
            }
        }
        match m {
            ToHost::Acquire(id) => {
                let (epoch, applied, revoked) = self.host.acquire(who(i));
                if self.quiet {
                    self.grants_after_quiet += 1;
                }
                if let Some((h, e)) = revoked {
                    self.event((h[0] - 1) as usize, ToPane::Revoked(e, epoch));
                }
                self.reply(i, ToPane::Granted(id, epoch, applied.seq));
            }
            ToHost::Reclaim(id, epoch) => {
                let (ok, cur, applied) = self.host.reclaim(who(i), epoch);
                self.reply(i, ToPane::Reclaimed(id, ok, cur, applied.seq));
            }
            ToHost::Release(id, epoch) => {
                let (_, vac) = self.host.release(who(i), epoch);
                for h in vac {
                    self.event((h[0] - 1) as usize, ToPane::Vacated);
                }
                self.reply(i, ToPane::Released(id));
            }
            ToHost::Resize(epoch, seq, c, r) => match self.host.resize(who(i), epoch, seq, c, r) {
                Ok(a) => self.reply(i, ToPane::Ack(a.epoch, a.seq)),
                Err(ResizeError::StaleResize(a)) => self.reply(i, ToPane::StaleResize(a.epoch, a.seq)),
                Err(ResizeError::StaleLease { current_epoch }) => self.reply(i, ToPane::StaleLease(epoch, current_epoch)),
            },
        }
    }

    fn pane(&mut self, i: usize, m: ToPane) {
        let p = &mut self.panes[i];
        match m {
            ToPane::Granted(id, e, seq) => p.on_granted(id, SESSION, e, seq),
            ToPane::Reclaimed(id, ok, cur, seq) => p.on_reclaim_result(id, ok, cur, seq),
            ToPane::Released(id) => p.on_released(id),
            ToPane::Revoked(e, n) => p.on_revoked(SESSION, e, n),
            ToPane::Vacated => p.on_vacated(),
            ToPane::Ack(e, s) => p.on_resize_ack(SESSION, e, s),
            ToPane::StaleResize(e, s) => p.on_stale_resize(e, s),
            ToPane::StaleLease(e, c) => p.on_stale_lease(SESSION, e, c),
            ToPane::Failed(id) => p.on_request_failed(id),
        }
        self.flush(i);
    }

    fn deliver(&mut self, k: usize, fail: bool, lose_if_dead: bool) {
        let (pi, pc, to_host) = {
            let e = &self.net[k];
            (e.0, e.1, e.2.is_ok())
        };
        let head = self.net.iter().position(|e| e.0 == pi && e.1 == pc && e.2.is_ok() == to_host).unwrap();
        let (i, c, m) = self.net.remove(head);
        let dead = c != self.conn[i];
        match m {
            Ok(_) if dead && lose_if_dead => {}
            Ok(h) => {
                self.req_conn = c;
                self.host(i, h, fail)
            }
            Err(_) if dead => {}
            Err(p) => self.pane(i, p),
        }
    }
}

fn policy(focused: bool, generation: u64) -> Policy {
    Policy { bound_session: Some(SESSION), generation, visible_focused: focused, visible_applied_generation: Some(generation), gpu_ready: true, tombstoned: false }
}

#[derive(Debug, Clone)]
enum Step {
    Focus(usize, bool),
    Rebind(usize),
    Size(usize, u16),
    Reconnect(usize),
    Deliver(usize, bool, bool),
    Timeout(usize),
    Timers,
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        2 => (0usize..3, any::<bool>()).prop_map(|(i, f)| Step::Focus(i, f)),
        1 => (0usize..3).prop_map(Step::Rebind),
        2 => (0usize..3, 1u16..300).prop_map(|(i, c)| Step::Size(i, c)),
        1 => (0usize..3).prop_map(Step::Reconnect),
        12 => (any::<usize>(), prop::bool::weighted(0.05), any::<bool>()).prop_map(|(k, f, l)| Step::Deliver(k, f, l)),
        1 => (0usize..3).prop_map(Step::Timeout),
        2 => Just(Step::Timers),
    ]
}

fn run(steps: Vec<Step>) -> Result<(), TestCaseError> {
    let n = 3;
    let mut s = Sim { host: ResizeLease::new(80, 24), panes: (0..n).map(|_| PaneLease::new()).collect(), conn: vec![0; n], net: Vec::new(), req_conn: 0, grants_after_quiet: 0, quiet: false };
    let mut gens = vec![1u64; n];
    let mut focus = vec![false; n];
    let mut want = vec![(80u16, 24u16); n];
    for i in 0..n {
        s.panes[i].set_policy(policy(false, 1), false);
        s.panes[i].on_visible_applied(80, 24);
        s.flush(i);
    }
    for st in steps {
        match st {
            Step::Focus(i, f) => {
                focus[i] = f;
                s.panes[i].set_policy(policy(f, gens[i]), f);
                s.flush(i);
            }
            Step::Rebind(i) => {
                gens[i] += 1;
                s.panes[i].set_policy(Policy { visible_applied_generation: None, ..policy(focus[i], gens[i]) }, false);
                s.panes[i].on_visible_applied(want[i].0, want[i].1);
                s.flush(i);
            }
            Step::Size(i, c) => {
                want[i] = (c, 24);
                s.panes[i].on_visible_applied(c, 24);
                s.flush(i);
            }
            Step::Reconnect(i) => {
                s.conn[i] += 1;
                s.panes[i].on_reconnect();
                s.flush(i);
            }
            Step::Deliver(k, f, l) => {
                if !s.net.is_empty() {
                    let k = k % s.net.len();
                    s.deliver(k, f, l);
                }
            }
            Step::Timeout(i) => {
                if let Some(p) = s.panes[i].pending {
                    s.panes[i].on_request_failed(p.id);
                    s.flush(i);
                }
            }
            Step::Timers => {
                for i in 0..n {
                    s.panes[i].on_retry_timer();
                    s.panes[i].on_unacked_timeout();
                    s.flush(i);
                }
            }
        }
    }
    for i in 1..n {
        focus[i] = false;
        s.panes[i].set_policy(policy(false, gens[i]), false);
        s.flush(i);
    }
    let gained_focus = !focus[0];
    focus[0] = true;
    s.panes[0].set_policy(policy(true, gens[0]), gained_focus);
    s.flush(0);
    let mut idle_rounds = 0;
    for _ in 0..50_000 {
        if !s.net.is_empty() {
            s.deliver(0, false, false);
            idle_rounds = 0;
            continue;
        }
        idle_rounds += 1;
        if idle_rounds > 3 {
            break;
        }
        if idle_rounds == 2 {
            s.quiet = true;
        }
        for i in 0..n {
            s.panes[i].on_retry_timer();
            s.panes[i].on_unacked_timeout();
            s.flush(i);
        }
    }
    let (epoch, holder) = s.host.current();
    let lease = s.panes[0].lease;
    let states: Vec<_> = s.panes.iter().map(|p| (p.lease, p.pending, p.suppressed, p.disowned.clone())).collect();
    prop_assert!(lease.is_some(), "focused pane has no lease: host=({epoch}, {holder:?}) panes={states:?}");
    prop_assert_eq!(holder, Some(who(0)));
    prop_assert_eq!(lease.unwrap().epoch, epoch);
    prop_assert_eq!((s.host.applied.cols, s.host.applied.rows), want[0], "PTY size did not converge");
    for i in 1..n {
        prop_assert!(s.panes[i].lease.is_none(), "unfocused pane {} still holds a lease", i);
    }
    for (i, p) in s.panes.iter().enumerate() {
        prop_assert!(p.disowned.is_empty() && p.pending.is_none(), "pane {i} not settled: {:?}", states[i]);
    }
    prop_assert!(s.grants_after_quiet == 0, "lease churn after quiescence: {}", s.grants_after_quiet);
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: common::cases(3000), max_shrink_iters: 2000, .. ProptestConfig::default() })]
    #[test]
    fn resize_lease_converges(steps in prop::collection::vec(step(), 0..250)) {
        run(steps)?;
    }
}
