use session_core::author::{Author, Out, Status, CHUNK};

fn writes(out: &[Out]) -> Vec<(u64, u64, usize)> {
    out.iter()
        .filter_map(|o| match o {
            Out::Write { attempt, start, bytes, .. } => Some((*attempt, *start, bytes.len())),
            _ => None,
        })
        .collect()
}

#[test]
fn input_gap_advances_accepted_known_before_resending() {
    let mut a = Author::new(1);
    a.set_focus(true);
    a.type_bytes(&vec![b'x'; 3 * CHUNK]);
    let acquire = a.take_out();
    let Some(Out::Acquire { id, .. }) = acquire.first() else { panic!("expected Acquire, got {acquire:?}") };
    a.on_granted(*id, 1, 7);
    let sent = writes(&a.take_out());
    assert_eq!(sent.iter().map(|w| w.1).collect::<Vec<_>>(), vec![0, CHUNK as u64, 2 * CHUNK as u64]);

    a.on_write_retryable(sent[2].0, 1, 7, Some(CHUNK as u64));
    assert!(writes(&a.take_out()).is_empty(), "resend waits for the send timer");
    a.on_send_timer();
    let resent = writes(&a.take_out());
    assert_eq!(resent.first().map(|w| w.1), Some(CHUNK as u64), "resend starts at the host's accepted offset: {resent:?}");
    assert_eq!(a.unaccepted_len(), 2 * CHUNK as u64);
}

fn granted(a: &mut Author, session: u64, epoch: u64) {
    let out = a.take_out();
    let id = out.iter().find_map(|o| match o {
        Out::Acquire { id, .. } => Some(*id),
        _ => None,
    });
    a.on_granted(id.unwrap_or_else(|| panic!("expected Acquire in {out:?}")), session, epoch);
}

fn classified(out: &[Out]) -> Vec<(u64, Vec<u8>, Vec<Status>)> {
    out.iter()
        .filter_map(|o| match o {
            Out::Classified { session, bytes, statuses, .. } => Some((*session, bytes.clone(), statuses.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn unknown_epoch_marks_bytes_past_the_committed_high_water_as_outcome_unknown() {
    let mut a = Author::new(1);
    a.set_focus(true);
    a.type_bytes(b"abcdef");
    granted(&mut a, 1, 3);
    a.on_input_ack(None, 1, 3, 4, 2);
    a.on_lease_lost(1, 3, 9);
    let _ = a.take_out();
    a.on_input_ack(None, 1, 3, 6, 5);
    a.on_epoch_unknown(1, 3);
    assert_eq!(classified(&a.take_out()), vec![(1, b"ef".to_vec(), vec![Status::Written, Status::OutcomeUnknown])]);
}

#[test]
fn owner_loss_marks_unconfirmed_bytes_outcome_unknown() {
    let mut a = Author::new(1);
    a.set_focus(true);
    a.type_bytes(b"xyz");
    granted(&mut a, 1, 2);
    let _ = a.take_out();
    a.on_owner_lost(1);
    assert_eq!(classified(&a.take_out()), vec![(1, b"xyz".to_vec(), vec![Status::OutcomeUnknown; 3])]);
    assert!(a.current().is_none());
}

#[test]
fn rebinding_never_carries_typed_bytes_into_another_session() {
    let mut a = Author::new(1);
    a.set_focus(true);
    a.type_bytes(b"xy");
    let out = a.take_out();
    let acquire = out.iter().find_map(|o| match o {
        Out::Acquire { id, session: 1 } => Some(*id),
        _ => None,
    });
    a.rebind(2);
    let out = a.take_out();
    assert_eq!(classified(&out), vec![(1, b"xy".to_vec(), vec![Status::NotDelivered; 2])]);
    let release = out.iter().find_map(|o| match o {
        Out::Release { id, session: 1, epoch: 0 } => Some(*id),
        _ => None,
    });
    a.on_granted(acquire.expect("acquire for session 1"), 1, 5);
    a.on_released(release.expect("confirming release of the abandoned acquire"));
    let _ = a.take_out();
    a.type_bytes(b"z");
    granted(&mut a, 2, 1);
    let sent: Vec<(u64, Vec<u8>)> = a
        .take_out()
        .into_iter()
        .filter_map(|o| match o {
            Out::Write { session, bytes, .. } => Some((session, bytes)),
            _ => None,
        })
        .collect();
    assert_eq!(sent, vec![(2, b"z".to_vec())]);
}
