mod common;

use proptest::prelude::*;
use session_core::budget::{Allocator, RequestKey};
use session_core::replica::{diff, merge, Replica};
use session_core::subscription::{BudgetState, Effect, Subscription};
use fxsh::types::{Cell, ExitInfo, RowData};
use fxsh::Uuid;

fn row(cols: u16, tag: u32) -> RowData {
    let mut r = RowData { wrapped: tag % 2 == 1, cells: vec![Cell::BLANK; cols as usize] };
    r.cells[0].codepoint = 'a' as u32 + tag % 26;
    r
}

#[derive(Debug, Clone)]
enum Op {
    Print(u16, u32),
    Scroll(u32),
    Title(u8),
    Cursor(u16, u16),
    Evict(u8),
    Link(u16, u8),
    Exit,
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        5 => (0u16..6, any::<u32>()).prop_map(|(r, t)| Op::Print(r, t)),
        3 => any::<u32>().prop_map(Op::Scroll),
        1 => any::<u8>().prop_map(Op::Title),
        2 => (0u16..6, 0u16..8).prop_map(|(r, c)| Op::Cursor(r, c)),
        1 => (1u8..4).prop_map(Op::Evict),
        1 => (0u16..6, 1u8..4).prop_map(|(r, l)| Op::Link(r, l)),
        1 => Just(Op::Exit),
    ]
}

fn step(s: &mut Replica, next_line: &mut u64, o: &Op) {
    match o {
        Op::Print(r, t) => s.screen[*r as usize] = row(s.cols, *t),
        Op::Scroll(t) => {
            let top = s.screen.remove(0);
            s.scrollback.push_back((*next_line, top));
            *next_line += 1;
            s.screen.push(row(s.cols, *t));
        }
        Op::Title(t) => s.title = format!("t{t}"),
        Op::Cursor(r, c) => {
            s.cursor.row = *r;
            s.cursor.col = *c;
        }
        Op::Evict(n) => {
            for _ in 0..*n {
                s.scrollback.pop_front();
            }
        }
        Op::Link(r, l) => {
            s.hyperlinks.insert(*l as u32, format!("https://e/{l}"));
            s.screen[*r as usize].cells[1].hyperlink_id = *l as u32;
        }
        Op::Exit => s.exit_info = Some(ExitInfo { exit_code: Some(0), posix_signal: None }),
    }
    s.prune_hyperlinks();
}

proptest! {
    #![proptest_config(ProptestConfig { cases: common::cases(2000), .. ProptestConfig::default() })]

    #[test]
    fn diff_then_apply_reproduces_state(ops in prop::collection::vec(prop::collection::vec(op(), 1..6), 1..30)) {
        let mut host = Replica::new(8, 6);
        let mut client = host.clone();
        let mut next_line = 1;
        let mut rev = 0;
        for batch in ops {
            let prev = host.clone();
            for o in &batch {
                step(&mut host, &mut next_line, o);
            }
            let d = diff(&prev, &host, 1, rev, rev + 1).expect("append-only scrollback");
            rev += 1;
            client.apply_delta(&d);
            prop_assert_eq!(fxsh::state_digest(&client.to_body()), fxsh::state_digest(&host.to_body()));
        }
    }

    #[test]
    fn merged_deltas_equal_sequential_application(ops in prop::collection::vec(prop::collection::vec(op(), 1..4), 2..20)) {
        let mut host = Replica::new(8, 6);
        let start = host.clone();
        let mut next_line = 1;
        let mut deltas = Vec::new();
        for (i, batch) in ops.iter().enumerate() {
            let prev = host.clone();
            for o in batch {
                step(&mut host, &mut next_line, o);
            }
            deltas.push(diff(&prev, &host, 1, i as u64, i as u64 + 1).unwrap());
        }
        let mut seq = start.clone();
        for d in &deltas {
            seq.apply_delta(d);
        }
        let mut merged = start;
        merged.apply_delta(&merge(&deltas).unwrap());
        prop_assert_eq!(merged.to_body(), seq.to_body());
    }
}

#[test]
fn scrollback_rewrite_is_detected() {
    let mut a = Replica::new(4, 2);
    a.scrollback.push_back((1, row(4, 1)));
    a.scrollback.push_back((2, row(4, 2)));
    let mut b = a.clone();
    b.scrollback[1].1 = row(4, 9);
    assert!(diff(&a, &b, 1, 0, 1).is_err());
}

#[test]
fn eviction_keeps_state_under_limit() {
    let mut r = Replica::new(200, 10);
    for i in 0..400 {
        r.scrollback.push_back((i + 1, row(200, i as u32)));
    }
    let before = r.encoded_len();
    let limit = before / 2;
    let evicted = r.evict_to_limit(limit);
    assert!(r.encoded_len() <= limit);
    assert_eq!(evicted, r.first_line_id());
}

#[derive(Debug, Clone)]
enum SubOp {
    Resync(usize),
    Close(usize),
    Grant(usize),
    Written(usize),
    Released(usize),
    Deadline(usize),
    Delta(usize),
}

fn sub_op() -> impl Strategy<Value = SubOp> {
    prop_oneof![
        3 => (0usize..6).prop_map(SubOp::Resync),
        1 => (0usize..6).prop_map(SubOp::Close),
        4 => any::<usize>().prop_map(SubOp::Grant),
        3 => any::<usize>().prop_map(SubOp::Written),
        3 => any::<usize>().prop_map(SubOp::Released),
        1 => any::<usize>().prop_map(SubOp::Deadline),
        3 => (0usize..6).prop_map(SubOp::Delta),
    ]
}

struct World {
    alloc: Allocator,
    subs: Vec<Subscription>,
    grants: Vec<(usize, u64, u64)>,
    written: Vec<(usize, u64, u64)>,
    released: Vec<(usize, u64, u64)>,
    deadlines: Vec<(usize, u64, u64)>,
    revision: u64,
}

impl World {
    fn apply(&mut self, i: usize, fx: Vec<Effect>) {
        for e in fx {
            match e {
                Effect::RequestReservation { request_seq, .. } => {
                    let g = self.alloc.request(RequestKey { session: [0; 16], subscription_id: i as u64, request_seq });
                    self.route(g);
                }
                Effect::CancelRequest { request_seq, .. } => {
                    let g = self.alloc.cancel(RequestKey { session: [0; 16], subscription_id: i as u64, request_seq });
                    self.route(g);
                }
                Effect::ReturnReservation { reservation_id } => {
                    let g = self.alloc.give_back(reservation_id);
                    self.route(g);
                }
                Effect::BuildSnapshot { snapshot_id, reservation_id, .. } => self.written.push((i, reservation_id, snapshot_id)),
                Effect::DropPending { snapshot_id, .. } => {
                    let found: Vec<_> = self.written.iter().filter(|(j, _, s)| *j == i && *s == snapshot_id).cloned().collect();
                    self.written.retain(|(j, _, s)| !(*j == i && *s == snapshot_id));
                    for (j, r, s) in found {
                        self.released.push((j, r, s));
                    }
                }
                Effect::ArmDeadline { reservation_id, snapshot_id, .. } => self.deadlines.push((i, reservation_id, snapshot_id)),
                Effect::CancelDeadline { .. } => self.deadlines.retain(|(j, _, _)| *j != i),
                Effect::CloseSlowConsumer { .. } | Effect::SendAck(_) | Effect::SendDelta(_) => {}
            }
        }
    }

    fn route(&mut self, grants: Vec<session_core::budget::Grant>) {
        for g in grants {
            self.grants.push((g.key.subscription_id as usize, g.key.request_seq, g.reservation_id));
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: common::cases(4000), .. ProptestConfig::default() })]

    #[test]
    fn budget_never_leaks_and_never_stalls(ops in prop::collection::vec(sub_op(), 0..300)) {
        let n = 6;
        let mut w = World { alloc: Allocator::new(3, 1), subs: (0..n).map(|i| Subscription::new(i as u64, Uuid([i as u8; 16]), 1)).collect(), grants: vec![], written: vec![], released: vec![], deadlines: vec![], revision: 0 };
        for i in 0..n {
            let mut fx = Vec::new();
            w.subs[i].start_with_ack(Uuid([9; 16]), 0, None, &mut fx);
            w.apply(i, fx);
        }
        let pick = |v: &Vec<(usize, u64, u64)>, k: usize| if v.is_empty() { None } else { Some(k % v.len()) };
        for o in ops {
            let mut fx = Vec::new();
            match o {
                SubOp::Resync(i) => { w.subs[i].resync(&mut fx); w.apply(i, fx); }
                SubOp::Close(i) => { w.subs[i].close(&mut fx); w.apply(i, fx); }
                SubOp::Grant(k) => if let Some(k) = pick(&w.grants, k) {
                    let (i, q, r) = w.grants.remove(k);
                    w.revision += 1;
                    w.subs[i].on_budget_available(q, r, w.revision, 0, &mut fx);
                    w.apply(i, fx);
                },
                SubOp::Written(k) => if let Some(k) = pick(&w.written, k) {
                    let (i, r, s) = w.written.remove(k);
                    w.subs[i].on_frame_written(r, s, true, &mut fx);
                    w.released.push((i, r, s));
                    w.apply(i, fx);
                },
                SubOp::Released(k) => if let Some(k) = pick(&w.released, k) {
                    let (i, r, s) = w.released.remove(k);
                    w.subs[i].on_buffer_released(r, s, &mut fx);
                    w.apply(i, fx);
                },
                SubOp::Deadline(k) => if let Some(k) = pick(&w.deadlines, k) {
                    let (i, r, s) = w.deadlines.remove(k);
                    w.subs[i].on_deadline(r, s, &mut fx);
                    w.apply(i, fx);
                },
                SubOp::Delta(i) => {
                    let d = fxsh::types::Delta { subscription_id: 0, base_revision: w.subs[i].base, new_revision: w.subs[i].base + 1, size: None, cursor: None, modes: None, palette: None, title: Some("x".into()), dirty_rows: vec![], hyperlinks_added: vec![], scrollback_appended: vec![], scrollback_evicted_before: None, exit_info: None, ui_events: vec![] };
                    w.subs[i].push_delta(d, &mut fx);
                    w.subs[i].drain_to_wire(&mut fx);
                    w.apply(i, fx);
                }
            }
            prop_assert!(w.alloc.issued_bytes() <= 3);
        }
        for _ in 0..10_000 {
            let mut progressed = false;
            if let Some((i, q, r)) = w.grants.pop() {
                let mut fx = Vec::new();
                w.revision += 1;
                w.subs[i].on_budget_available(q, r, w.revision, 0, &mut fx);
                w.apply(i, fx);
                progressed = true;
            } else if let Some((i, r, s)) = w.written.pop() {
                let mut fx = Vec::new();
                w.subs[i].on_frame_written(r, s, true, &mut fx);
                w.released.push((i, r, s));
                w.apply(i, fx);
                progressed = true;
            } else if let Some((i, r, s)) = w.released.pop() {
                let mut fx = Vec::new();
                w.subs[i].on_buffer_released(r, s, &mut fx);
                w.apply(i, fx);
                progressed = true;
            }
            if !progressed {
                break;
            }
        }
        for s in &w.subs {
            prop_assert!(matches!(s.budget, BudgetState::Idle | BudgetState::Closed), "sub {} stuck in {:?}", s.id, s.budget);
            prop_assert!(s.budget == BudgetState::Closed || !s.resync_pending, "sub {} left resync pending", s.id);
            prop_assert!(s.unreturned.is_empty(), "sub {} has unreturned reservations", s.id);
        }
        prop_assert_eq!(w.alloc.issued_count(), 0, "reservations leaked");
        prop_assert_eq!(w.alloc.queued().count(), 0, "allocator queue not empty");
    }
}
