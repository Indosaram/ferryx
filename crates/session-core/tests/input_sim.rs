mod common;

use std::collections::BTreeSet;

use proptest::prelude::*;
use session_core::author::{Author, Out as AOut, Status};
use session_core::input::{InputLedger, WriteError};

const SESSIONS: usize = 2;
const AUTHORS: usize = 2;

#[derive(Debug, Clone)]
enum ToHost {
    Acquire { id: u64, s: usize },
    Release { id: u64, s: usize, epoch: u64 },
    Reclaim { id: u64, s: usize, epoch: u64 },
    Write { attempt: u64, s: usize, epoch: u64, start: u64, bytes: Vec<u8> },
    Get { id: u64, s: usize, epoch: u64 },
}

#[derive(Debug, Clone)]
enum ToClient {
    Granted { id: u64, s: usize, epoch: u64 },
    Released { id: u64 },
    Ack { attempt: Option<u64>, s: usize, epoch: u64, accepted: u64, committed: u64 },
    Retry { attempt: u64, s: usize, epoch: u64, gap: Option<u64> },
    Lost { s: usize, epoch: u64, other: u64 },
    Reclaimed { id: u64, ok: bool, current: u64, accepted: u64, committed: u64, finals: Option<(u64, u64, u64)> },
    ReclaimUnknown { id: u64 },
    Epoch { id: u64, s: usize, epoch: u64, fa: u64, cm: u64, pd: u64, fenced: bool },
    EpochUnknown { s: usize, epoch: u64 },
    Failed { id: u64 },
    Vacated { s: usize },
}

struct Sim {
    hosts: Vec<InputLedger>,
    authors: Vec<Author>,
    conn: Vec<u64>,
    net: Vec<(usize, u64, Result<ToHost, ToClient>)>,
    req_conn: u64,
    typed: Vec<usize>,
    typed_for: Vec<Vec<usize>>,
    taps: Vec<Vec<u8>>,
    statuses: Vec<Vec<(usize, u8, Status)>>,
}

fn holder(i: usize) -> [u8; 16] {
    [i as u8 + 1; 16]
}

fn holder_index(h: [u8; 16]) -> usize {
    (h[0] - 1) as usize
}

impl Sim {
    fn new() -> Self {
        Sim {
            hosts: (0..SESSIONS).map(|_| InputLedger::new()).collect(),
            authors: (0..AUTHORS).map(|_| Author::new(0)).collect(),
            conn: vec![0; AUTHORS],
            net: Vec::new(),
            req_conn: 0,
            typed: vec![0; AUTHORS],
            typed_for: vec![Vec::new(); AUTHORS],
            taps: vec![Vec::new(); SESSIONS],
            statuses: vec![Vec::new(); AUTHORS],
        }
    }

    fn flush(&mut self, i: usize) {
        for o in self.authors[i].take_out() {
            let m = match o {
                AOut::Acquire { id, session } => ToHost::Acquire { id, s: session as usize },
                AOut::Release { id, session, epoch } => ToHost::Release { id, s: session as usize, epoch },
                AOut::Reclaim { id, session, epoch } => ToHost::Reclaim { id, s: session as usize, epoch },
                AOut::Write { attempt, session, epoch, start, bytes } => ToHost::Write { attempt, s: session as usize, epoch, start, bytes },
                AOut::GetEpochState { id, session, epoch } => ToHost::Get { id, s: session as usize, epoch },
                AOut::Classified { session, bytes, statuses, .. } => {
                    self.statuses[i].extend(bytes.into_iter().zip(statuses).map(|(b, st)| (session as usize, b, st)));
                    continue;
                }
            };
            self.net.push((i, self.conn[i], Ok(m)));
        }
    }

    // A response travels back on the connection its request arrived on and is
    // lost with it; events go to the instance's current connection.
    fn reply(&mut self, i: usize, m: ToClient) {
        self.net.push((i, self.req_conn, Err(m)));
    }

    fn event(&mut self, i: usize, m: ToClient) {
        self.net.push((i, self.conn[i], Err(m)));
    }

    fn vacate(&mut self, s: usize, revokees: Vec<[u8; 16]>) {
        for h in revokees {
            self.event(holder_index(h), ToClient::Vacated { s });
        }
    }

    fn host_handle(&mut self, i: usize, m: ToHost, fail: bool) {
        let who = holder(i);
        let lease_request = match &m {
            ToHost::Acquire { id, s } | ToHost::Reclaim { id, s, .. } | ToHost::Release { id, s, .. } => Some((*id, *s)),
            _ => None,
        };
        if let Some((id, s)) = lease_request {
            if !self.hosts[s].fresh(who, id) {
                return;
            }
        }
        if fail {
            if let ToHost::Acquire { id, .. } | ToHost::Reclaim { id, .. } | ToHost::Get { id, .. } | ToHost::Release { id, .. } = m {
                return self.reply(i, ToClient::Failed { id });
            }
        }
        match m {
            ToHost::Acquire { id, s } => match self.hosts[s].acquire(who) {
                Ok(g) => {
                    if let Some(r) = g.revoked {
                        self.event(holder_index(r.holder), ToClient::Lost { s, epoch: r.revoked_epoch, other: r.new_epoch });
                    }
                    self.reply(i, ToClient::Granted { id, s, epoch: g.epoch });
                }
                Err(_) => self.reply(i, ToClient::Failed { id }),
            },
            ToHost::Release { id, s, epoch } => {
                let (_, vacated) = self.hosts[s].release(who, epoch);
                self.vacate(s, vacated);
                self.reply(i, ToClient::Released { id });
            }
            ToHost::Reclaim { id, s, epoch } => match self.hosts[s].reclaim(who, epoch) {
                Err(()) => self.reply(i, ToClient::ReclaimUnknown { id }),
                Ok(Ok((a, c))) => self.reply(i, ToClient::Reclaimed { id, ok: true, current: epoch, accepted: a, committed: c, finals: None }),
                Ok(Err((cur, fa, cm, pd))) => self.reply(i, ToClient::Reclaimed { id, ok: false, current: cur, accepted: 0, committed: 0, finals: Some((fa, cm, pd)) }),
            },
            ToHost::Write { attempt, s, epoch, start, bytes } => {
                let crc = fxsh::input_crc32c(&bytes);
                match self.hosts[s].write(who, epoch, start, &bytes, crc) {
                    Ok(a) => self.reply(i, ToClient::Ack { attempt: Some(attempt), s, epoch, accepted: a.accepted, committed: a.committed }),
                    Err((WriteError::StaleLease { current_epoch, .. }, _)) => self.reply(i, ToClient::Lost { s, epoch, other: current_epoch }),
                    Err((WriteError::Gap { accepted }, _)) => self.reply(i, ToClient::Retry { attempt, s, epoch, gap: Some(accepted) }),
                    Err((WriteError::Backpressure { .. }, _)) => self.reply(i, ToClient::Retry { attempt, s, epoch, gap: None }),
                    Err((WriteError::Diverged { .. } | WriteError::Unverifiable { .. }, revoke)) => {
                        if let Some((_, vacated)) = revoke {
                            self.vacate(s, vacated);
                        }
                        self.reply(i, ToClient::Lost { s, epoch, other: 0 });
                    }
                    Err(e) => panic!("unexpected write error {e:?}"),
                }
            }
            ToHost::Get { id, s, epoch } => match self.hosts[s].epoch_state(epoch) {
                Some((fa, cm, pd, fenced)) => self.reply(i, ToClient::Epoch { id, s, epoch, fa, cm, pd, fenced }),
                None => self.reply(i, ToClient::EpochUnknown { s, epoch }),
            },
        }
    }

    fn client_handle(&mut self, i: usize, m: ToClient) {
        let a = &mut self.authors[i];
        match m {
            ToClient::Granted { id, s, epoch } => a.on_granted(id, s as u64, epoch),
            ToClient::Released { id } => a.on_released(id),
            ToClient::Ack { attempt, s, epoch, accepted, committed } => a.on_input_ack(attempt, s as u64, epoch, accepted, committed),
            ToClient::Retry { attempt, s, epoch, gap } => a.on_write_retryable(attempt, s as u64, epoch, gap),
            ToClient::Lost { s, epoch, other } => a.on_lease_lost(s as u64, epoch, other),
            ToClient::Reclaimed { id, ok, current, accepted, committed, finals } => a.on_reclaim_result(id, ok, current, accepted, committed, finals),
            ToClient::ReclaimUnknown { id } => a.on_reclaim_epoch_unknown(id),
            ToClient::Epoch { id, s, epoch, fa, cm, pd, fenced } => a.on_epoch_state(id, s as u64, epoch, fa, cm, pd, fenced),
            ToClient::EpochUnknown { s, epoch } => a.on_epoch_unknown(s as u64, epoch),
            ToClient::Failed { id } => a.on_request_failed(id),
            ToClient::Vacated { s } => a.on_vacated(s as u64),
        }
        self.flush(i);
    }

    fn deliver(&mut self, k: usize, lose_if_dead: bool, fail: bool) {
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
                self.host_handle(i, h, fail)
            }
            Err(_) if dead => {}
            Err(cm) => self.client_handle(i, cm),
        }
    }

    fn write_some(&mut self, s: usize, n: usize) {
        let Some(head) = self.hosts[s].head() else { return };
        let take = n.min(head.len());
        self.taps[s].extend_from_slice(&head[..take]);
        if let Some(ack) = self.hosts[s].on_written(take) {
            self.event(holder_index(ack.holder), ToClient::Ack { attempt: None, s, epoch: ack.epoch, accepted: ack.accepted, committed: ack.committed });
        }
    }

    fn timers(&mut self) {
        for i in 0..AUTHORS {
            self.authors[i].on_retry_timer();
            self.authors[i].on_send_timer();
            self.flush(i);
        }
    }
}

#[derive(Debug, Clone)]
enum Step {
    Type(usize),
    Focus(usize, bool),
    Rebind(usize),
    Reconnect(usize),
    Deliver(usize, bool, bool),
    Write(usize, usize),
    Timeout(usize),
    Timers,
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        4 => (0..AUTHORS).prop_map(Step::Type),
        1 => (0..AUTHORS, any::<bool>()).prop_map(|(i, f)| Step::Focus(i, f)),
        1 => (0..AUTHORS).prop_map(Step::Rebind),
        1 => (0..AUTHORS).prop_map(Step::Reconnect),
        12 => (any::<usize>(), any::<bool>(), prop::bool::weighted(0.05)).prop_map(|(k, l, f)| Step::Deliver(k, l, f)),
        3 => (0..SESSIONS, 1usize..200).prop_map(|(s, n)| Step::Write(s, n)),
        1 => (0..AUTHORS).prop_map(Step::Timeout),
        1 => Just(Step::Timers),
    ]
}

fn run(steps: Vec<Step>) -> Result<(), TestCaseError> {
    let mut s = Sim::new();
    for st in steps {
        match st {
            Step::Type(i) => {
                let n = s.typed[i];
                if n < 128 {
                    s.typed[i] += 1;
                    s.typed_for[i].push(s.authors[i].session as usize);
                    s.authors[i].type_bytes(&[((i as u8) << 7) | n as u8]);
                    s.flush(i);
                }
            }
            Step::Focus(i, f) => {
                s.authors[i].set_focus(f);
                s.flush(i);
            }
            Step::Rebind(i) => {
                let to = 1 - s.authors[i].session;
                s.authors[i].rebind(to);
                s.flush(i);
            }
            Step::Reconnect(i) => {
                s.conn[i] += 1;
                s.authors[i].on_reconnect();
                s.flush(i);
            }
            Step::Deliver(k, l, f) => {
                if !s.net.is_empty() {
                    let k = k % s.net.len();
                    s.deliver(k, l, f);
                }
            }
            Step::Write(sess, n) => s.write_some(sess, n),
            Step::Timeout(i) => {
                if let Some(p) = s.authors[i].pending {
                    s.authors[i].on_request_failed(p.id);
                    s.flush(i);
                }
            }
            Step::Timers => s.timers(),
        }
    }
    s.authors[1].set_focus(false);
    s.flush(1);
    s.authors[0].set_focus(true);
    s.authors[0].typing = true;
    s.flush(0);
    for _ in 0..20_000 {
        if !s.net.is_empty() {
            s.deliver(0, false, false);
        } else if let Some(sess) = (0..SESSIONS).find(|x| s.hosts[*x].head().is_some()) {
            s.write_some(sess, usize::MAX);
        } else {
            s.timers();
            if s.net.is_empty() {
                break;
            }
        }
    }

    let mut seen = BTreeSet::new();
    for (sess, tap) in s.taps.iter().enumerate() {
        for b in tap {
            let (i, n) = ((*b >> 7) as usize, (*b & 0x7f) as usize);
            prop_assert!(seen.insert((i, n)), "byte {n} of author {i} written twice (session {sess})");
            prop_assert_eq!(s.typed_for[i][n], sess, "author {} byte {} typed for session {} but written to {}", i, n, s.typed_for[i][n], sess);
        }
        for i in 0..AUTHORS {
            let order: Vec<usize> = tap.iter().filter(|b| (**b >> 7) as usize == i).map(|b| (*b & 0x7f) as usize).collect();
            prop_assert!(order.windows(2).all(|w| w[0] < w[1]), "author {i} bytes out of order in session {sess}: {order:?}");
        }
    }
    for i in 0..AUTHORS {
        let mut classified = BTreeSet::new();
        for (sess, b, st) in &s.statuses[i] {
            let n = (*b & 0x7f) as usize;
            classified.insert(n);
            let on_own_tap = s.taps[*sess].contains(b);
            match st {
                Status::Written | Status::AcceptedPendingWrite => prop_assert!(on_own_tap, "author {i} byte {n} marked {st:?} but not written to session {sess}"),
                Status::NotDelivered => prop_assert!(!seen.contains(&(i, n)), "author {i} byte {n} marked NotDelivered but written"),
                Status::AcceptedThenDropped | Status::OutcomeUnknown => {}
            }
        }
        let still_held = s.authors[i].unassigned_len() + s.authors[i].unaccepted_len() as usize;
        for n in 0..s.typed[i].saturating_sub(still_held) {
            prop_assert!(seen.contains(&(i, n)) || classified.contains(&n), "author {i} byte {n} lost without classification");
        }
        let a = &s.authors[i];
        prop_assert!(a.finalizing_epochs().is_empty(), "author {i} finalizing stuck: {:?}", a.finalizing_epochs());
        prop_assert!(a.disowned.is_empty() && a.pending.is_none(), "author {i} not settled: disowned={:?} pending={:?}", a.disowned, a.pending);
    }
    let a0 = &s.authors[0];
    prop_assert_eq!(a0.unassigned_len(), 0, "focused author left unassigned bytes");
    prop_assert_eq!(a0.unaccepted_len(), 0, "focused author left unaccepted bytes");
    prop_assert!(s.authors[1].current().is_none(), "unfocused author still holds a lease");
    let (b0, e0) = a0.current().ok_or_else(|| TestCaseError::fail("focused typing author holds no lease"))?;
    for sess in 0..SESSIONS {
        let host = &s.hosts[sess];
        if sess as u64 == b0 {
            prop_assert_eq!((host.holder(), host.current_epoch()), (Some(holder(0)), e0), "session {} lease not with the focused author", sess);
        } else {
            prop_assert_eq!(host.holder(), None, "session {} has an orphan holder", sess);
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: common::cases(3000), max_shrink_iters: 4000, .. ProptestConfig::default() })]
    #[test]
    fn authors_converge_and_never_stall(steps in prop::collection::vec(step(), 0..300)) {
        run(steps)?;
    }
}
