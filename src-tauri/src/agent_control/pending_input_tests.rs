use super::*;
use crate::scoped_contracts::{DeliveryReceipt, DeliveryStage, Epoch, TargetRef};

/// The journal's own clock: retention compares a stored `at` with `now()`, so a test timestamp
/// taken from the same clock is always "recent" and no calendar date can rot this suite.
fn clock() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_secs()
}

fn journal_in(dir: &tempfile::TempDir) -> Journal {
    Journal::open(dir.path().join("machine-operations.v1.json"))
}

fn card(prompt_id: &str, revision: &str) -> NewPendingInput {
    NewPendingInput {
        prompt_id: prompt_id.into(),
        session_id: "s1".into(),
        screen_revision: revision.into(),
        question: "Run the migration?".into(),
        options: vec![
            PendingInputOption { key: "0".into(), label: "Yes".into() },
            PendingInputOption { key: "1".into(), label: "No".into() },
        ],
        kind: "approval".into(),
        needs_confirmation: true,
    }
}

fn receipt(request_id: &str) -> DeliveryReceipt {
    DeliveryReceipt {
        request_id: request_id.into(),
        target: TargetRef {
            host_id: "local".into(),
            owner_id: "owner".into(),
            epoch: Epoch(1),
            backend_session_id: "s1".into(),
        },
        stage: DeliveryStage::Accepted,
    }
}

fn yes() -> serde_json::Value {
    serde_json::json!({ "key": "0" })
}

fn no() -> serde_json::Value {
    serde_json::json!({ "key": "1" })
}

#[test]
fn opening_the_same_card_twice_keeps_one_row_and_the_first_open_time() {
    let dir = tempfile::tempdir().unwrap();
    let journal = journal_in(&dir);
    let at = clock();
    let first = open(&journal, card("p1", "v1-aaa"), at).unwrap();
    // A second read of the same pane sees the same card at a newer screen revision.
    let second = open(&journal, card("p1", "v1-bbb"), at + 5).unwrap();

    assert_eq!(first.opened_at, second.opened_at, "the open is recorded once");
    assert_eq!(second.screen_revision, "v1-bbb", "the newest read is the answerable screen");
    assert_eq!(journal.open_pending_inputs().unwrap().len(), 1);
    // Two reads of the same daemon's journal, one row: the id is the key, not the read.
    let reopened = journal_in(&dir);
    assert_eq!(reopened.open_pending_inputs().unwrap().len(), 1);
}

#[test]
fn the_same_answer_replayed_returns_the_stored_receipt_and_delivers_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let journal = journal_in(&dir);
    let at = clock();
    open(&journal, card("p1", "v1-aaa"), at).unwrap();

    assert_eq!(claim(&journal, "p1", "s1", "r1", &yes(), at).unwrap(), Claim::Claimed);
    record(
        &journal,
        "p1",
        DeliveryOutcome::Accepted { request_id: "r1".into(), receipt: receipt("r1") },
        at + 1,
    )
    .unwrap();

    // A second tap on the same card, from the desktop or the phone, with the same answer.
    let replayed = claim(&journal, "p1", "s1", "r2", &yes(), at + 2).unwrap();
    assert_eq!(replayed, Claim::Replayed(receipt("r1")));

    let stored = get(&journal, "p1").unwrap().unwrap();
    assert_eq!(
        stored.attempt.as_ref().map(|a| a.request_id.as_str()),
        Some("r1"),
        "the replay did not overwrite the attempt that produced the receipt"
    );
    assert!(journal.open_pending_inputs().unwrap().is_empty(), "a settled card is closed");
}

#[test]
fn a_different_answer_to_the_same_prompt_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let journal = journal_in(&dir);
    let at = clock();
    open(&journal, card("p1", "v1-aaa"), at).unwrap();
    claim(&journal, "p1", "s1", "r1", &yes(), at).unwrap();
    record(
        &journal,
        "p1",
        DeliveryOutcome::Accepted { request_id: "r1".into(), receipt: receipt("r1") },
        at + 1,
    )
    .unwrap();

    assert_eq!(claim(&journal, "p1", "s1", "r2", &no(), at + 2).unwrap(), Claim::AlreadyAnswered);
    // The refusal changed nothing: the first answer is still the delivered one.
    let stored = get(&journal, "p1").unwrap().unwrap();
    assert_eq!(stored.attempt.unwrap().answer.unwrap(), yes());
}

#[test]
fn a_second_caller_cannot_claim_an_attempt_that_is_still_in_flight() {
    let dir = tempfile::tempdir().unwrap();
    let journal = journal_in(&dir);
    let at = clock();
    open(&journal, card("p1", "v1-aaa"), at).unwrap();

    assert_eq!(claim(&journal, "p1", "s1", "r1", &yes(), at).unwrap(), Claim::Claimed);
    assert_eq!(claim(&journal, "p1", "s1", "r2", &yes(), at + 1).unwrap(), Claim::InFlight);
    // The same caller retrying its own attempt is idempotent, not a second claim.
    assert_eq!(claim(&journal, "p1", "s1", "r1", &yes(), at + 2).unwrap(), Claim::Claimed);

    let stored = get(&journal, "p1").unwrap().unwrap();
    assert_eq!(stored.attempt.unwrap().request_id, "r1");
}

#[test]
fn a_refusal_before_any_byte_leaves_the_card_pending_and_answerable() {
    let dir = tempfile::tempdir().unwrap();
    let journal = journal_in(&dir);
    let at = clock();
    open(&journal, card("p1", "v1-aaa"), at).unwrap();
    claim(&journal, "p1", "s1", "r1", &yes(), at).unwrap();
    record(
        &journal,
        "p1",
        DeliveryOutcome::NotTyped { request_id: "r1".into(), code: "CONTROL_CONFLICT".into() },
        at + 1,
    )
    .unwrap();

    let stored = get(&journal, "p1").unwrap().unwrap();
    assert!(stored.is_open(), "nothing was typed, so the question is still being asked");
    assert!(stored.attempt.is_none(), "the failed attempt is cleared");
    assert_eq!(stored.last_refusal.as_deref(), Some("CONTROL_CONFLICT"));
    assert!(!stored.is_in_crash_window(), "a typed-nothing refusal is not the crash window");

    // Someone else may now answer, including with a different answer.
    assert_eq!(claim(&journal, "p1", "s1", "r9", &no(), at + 2).unwrap(), Claim::Claimed);
}

#[test]
fn a_crash_between_commit_and_delivery_is_ambiguous_and_not_replayed_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let at = clock();
    {
        let journal = journal_in(&dir);
        open(&journal, card("p1", "v1-aaa"), at).unwrap();
        // The intent is committed and the process dies before any receipt is written.
        assert_eq!(claim(&journal, "p1", "s1", "r1", &yes(), at).unwrap(), Claim::Claimed);
    }

    // A new process opens the same journal: nothing in memory survived, only the file.
    let restarted = journal_in(&dir);
    let stored = get(&restarted, "p1").unwrap().unwrap();
    assert!(stored.is_in_crash_window(), "claimed with no outcome");
    assert_eq!(stored.screen_revision, "v1-aaa", "the stored screen revision survives a restart");

    assert_eq!(restarted.open_pending_inputs().unwrap().len(), 1, "ambiguous card remains visible");
    assert_eq!(claim(&restarted, "p1", "s1", "r2", &yes(), at + 1).unwrap(), Claim::InFlight);
    assert!(get(&restarted, "p1").unwrap().unwrap().resolution.is_none());
    dismiss_ambiguous(&restarted, "p1", at + 2).unwrap();
    assert!(get(&restarted, "p1").unwrap().unwrap().resolution.is_some());
    assert_eq!(claim(&restarted, "p1", "s1", "r3", &yes(), at + 3).unwrap(), Claim::AlreadyAnswered);
}

#[test]
fn an_unknown_outcome_is_not_re_sent_by_the_restart_path() {
    let dir = tempfile::tempdir().unwrap();
    let journal = journal_in(&dir);
    let at = clock();
    open(&journal, card("p1", "v1-aaa"), at).unwrap();
    claim(&journal, "p1", "s1", "r1", &yes(), at).unwrap();
    record(&journal, "p1", DeliveryOutcome::OutcomeUnknown { request_id: "r1".into() }, at + 1)
        .unwrap();

    assert!(
        journal.open_pending_inputs().unwrap().len() == 1,
        "bytes may have landed; only a person's next answer may retry, not a restart"
    );
    // A fresh answer is not a retry: the attempt's one write right is spent and its bytes may have
    // landed, so claiming again is refused until a person settles the ambiguity.
    assert_eq!(claim(&journal, "p1", "s1", "r2", &yes(), at + 2).unwrap(), Claim::InFlight);
    dismiss_ambiguous(&journal, "p1", at + 3).unwrap();
    assert!(get(&journal, "p1").unwrap().unwrap().resolution.is_some());
}

#[test]
fn dismissing_a_non_ambiguous_prompt_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let journal = journal_in(&dir);
    let at = clock();
    open(&journal, card("p1", "v1-aaa"), at).unwrap();
    assert_eq!(dismiss_ambiguous(&journal, "p1", at + 1).unwrap_err(), "PROMPT_NOT_AMBIGUOUS");
    assert_eq!(
        dismiss_ambiguous(&journal, "missing", at + 2).unwrap_err(),
        "PROMPT_NOT_FOUND"
    );
    assert!(get(&journal, "p1").unwrap().unwrap().resolution.is_none());
    assert_eq!(claim(&journal, "p1", "s1", "r1", &yes(), at + 2).unwrap(), Claim::Claimed);
}

#[test]
fn a_changed_screen_does_not_resolve_an_ambiguous_attempt_as_gone() {
    let dir = tempfile::tempdir().unwrap();
    let journal = journal_in(&dir);
    let at = clock();
    open(&journal, card("p1", "v1-aaa"), at).unwrap();
    claim(&journal, "p1", "s1", "r1", &yes(), at).unwrap();

    let reconciled = reconcile(&journal, Some(card("p2", "v1-bbb")), "s1", at + 1).unwrap();
    assert_eq!(reconciled.as_ref().map(|input| input.prompt_id.as_str()), Some("p2"));
    let ambiguous = get(&journal, "p1").unwrap().unwrap();
    assert!(ambiguous.is_open());
    assert!(ambiguous.is_in_crash_window());
}

#[test]
fn a_missing_prompt_cannot_be_dismissed_as_ambiguous() {
    let dir = tempfile::tempdir().unwrap();
    let journal = journal_in(&dir);
    assert_eq!(
        dismiss_ambiguous_for_screen(&journal, "absent", "s1", "rev", clock()).unwrap_err(),
        "PROMPT_NOT_FOUND"
    );
}

#[test]
fn stale_screen_identity_cannot_dismiss_an_ambiguous_attempt() {
    let dir = tempfile::tempdir().unwrap();
    let journal = journal_in(&dir);
    let at = clock();
    open(&journal, card("p1", "v1-aaa"), at).unwrap();
    claim(&journal, "p1", "s1", "r1", &yes(), at).unwrap();
    open(&journal, card("p1", "v1-bbb"), at + 1).unwrap();

    let current = get(&journal, "p1").unwrap().unwrap();
    assert_eq!(current.screen_revision, "v1-bbb");
    let stale_screen_revision = "v1-aaa";
    assert_ne!(current.screen_revision, stale_screen_revision);
    assert_eq!(
        dismiss_ambiguous_for_screen(&journal, "p1", "s1", stale_screen_revision, at + 2).unwrap_err(),
        "PROMPT_CONFLICT"
    );
    assert!(get(&journal, "p1").unwrap().unwrap().resolution.is_none());
    dismiss_ambiguous_for_screen(&journal, "p1", "s1", "v1-bbb", at + 3).unwrap();
    assert_eq!(get(&journal, "p1").unwrap().unwrap().resolution.unwrap().outcome, "dismissedAmbiguous");
}

#[test]
fn ambiguous_open_card_is_found_for_its_session_after_screen_revision_changes() {
    let dir = tempfile::tempdir().unwrap();
    let journal = journal_in(&dir);
    let at = clock();
    open(&journal, card("p1", "rev-old"), at).unwrap();
    claim(&journal, "p1", "s1", "r1", &yes(), at).unwrap();
    reconcile(&journal, Some(card("p2", "rev-new")), "s1", at + 1).unwrap();

    let unresolved = unresolved_ambiguous_for_session(&journal, "s1").unwrap().unwrap();
    assert_eq!(unresolved.prompt_id, "p1");
    assert_eq!(unresolved.screen_revision, "rev-old");
    assert!(unresolved.is_in_crash_window());
}

#[test]
fn the_open_card_is_found_by_session_and_screen_revision() {
    let dir = tempfile::tempdir().unwrap();
    let journal = journal_in(&dir);
    let at = clock();
    open(&journal, card("p1", "v1-aaa"), at).unwrap();

    assert!(open_for_screen(&journal, "s1", "v1-aaa").unwrap().is_some());
    assert!(open_for_screen(&journal, "s1", "v1-other").unwrap().is_none());
    assert!(open_for_screen(&journal, "s2", "v1-aaa").unwrap().is_none());

    resolve_gone(&journal, "p1", at + 1).unwrap();
    assert!(
        open_for_screen(&journal, "s1", "v1-aaa").unwrap().is_none(),
        "a card the pane stopped showing is no longer offered"
    );
}

#[test]
fn a_card_is_bound_to_one_session() {
    let dir = tempfile::tempdir().unwrap();
    let journal = journal_in(&dir);
    let at = clock();
    open(&journal, card("p1", "v1-aaa"), at).unwrap();

    let mut other = card("p1", "v1-aaa");
    other.session_id = "s9".into();
    assert_eq!(open(&journal, other, at).unwrap_err(), "PROMPT_CONFLICT");
    assert_eq!(
        claim(&journal, "p1", "s9", "r1", &yes(), at).unwrap(),
        Claim::AlreadyAnswered,
        "another session may not answer this card"
    );
}

#[test]
fn an_id_this_journal_never_recorded_is_reported_as_unrecorded() {
    let dir = tempfile::tempdir().unwrap();
    let journal = journal_in(&dir);
    assert_eq!(claim(&journal, "nobody", "s1", "r1", &yes(), clock()).unwrap(), Claim::Unrecorded);
}

#[test]
fn a_journal_written_before_step_55_reads_as_an_empty_pending_table() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("machine-operations.v1.json");
    // Exactly what a daemon before this change wrote: no `schema`, no `pendingInputs`.
    std::fs::write(&path, br#"{"version":1,"records":{},"sessions":{},"session_revision":0}"#)
        .unwrap();

    let journal = Journal::open(path.clone());
    assert!(journal.pending_input("p1").unwrap().is_none());
    assert!(journal.open_pending_inputs().unwrap().is_empty());

    // The first write lifts the file to step 55 without losing what was there.
    open(&journal, card("p1", "v1-aaa"), clock()).unwrap();
    let written = std::fs::read_to_string(&path).unwrap();
    // `to_vec_pretty` spaces the separator, so match the pair rather than a compact literal.
    assert!(
        written.contains("\"schema\"") && written.contains("55"),
        "the file is stamped at the step: {written}"
    );
    // `Store` carries no `rename_all`, so its keys are the Rust field names (snake_case).
    assert!(
        written.contains("\"pending_inputs\""),
        "the card is in the file: {written}"
    );
    assert_eq!(journal_in(&dir).open_pending_inputs().unwrap().len(), 1);
}

#[test]
fn delivery_admission_stamps_a_new_attempt_once() {
    let dir = tempfile::tempdir().unwrap();
    let journal = journal_in(&dir);
    let at = clock();
    open(&journal, card("p1", "v1-aaa"), at).unwrap();

    assert_eq!(
        claim_for_delivery(&journal, "p1", "s1", "r1", &yes(), at).unwrap(),
        Claim::Claimed
    );
    let stored = get(&journal, "p1").unwrap().unwrap();
    assert!(stored.is_in_crash_window(), "the written-before state is durable");
    let attempt = stored.attempt.expect("the attempt is stamped");
    assert_eq!(attempt.request_id, "r1");
    assert_eq!(attempt.answer_digest, answer_digest(&yes()));
    assert!(attempt.outcome.is_none());
}

#[test]
fn delivery_admission_reports_an_unrecorded_id_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let journal = journal_in(&dir);
    let at = clock();
    assert_eq!(
        claim_for_delivery(&journal, "detected-only", "s1", "r1", &yes(), at).unwrap(),
        Claim::Unrecorded
    );
    // Nothing was minted for an id this journal never saw: registering the card is the caller's own
    // explicit step, not a side effect of asking for admission.
    assert!(journal.pending_input("detected-only").unwrap().is_none());
    assert!(journal_in(&dir).open_pending_inputs().unwrap().is_empty());
}

#[test]
fn an_in_flight_attempt_never_grants_write_admission_again_under_the_same_id() {
    let dir = tempfile::tempdir().unwrap();
    let journal = journal_in(&dir);
    let at = clock();
    open(&journal, card("p1", "v1-aaa"), at).unwrap();
    assert_eq!(
        claim_for_delivery(&journal, "p1", "s1", "r1", &yes(), at).unwrap(),
        Claim::Claimed
    );

    // The very same request id asking again is not the owner of a second write: the right was
    // granted once, and the caller cannot know whether its first attempt's bytes reached the pane.
    assert_eq!(
        claim_for_delivery(&journal, "p1", "s1", "r1", &yes(), at + 1).unwrap(),
        Claim::InFlight
    );
    assert_eq!(
        claim_for_delivery(&journal, "p1", "s1", "r2", &yes(), at + 2).unwrap(),
        Claim::InFlight
    );
    let attempt = get(&journal, "p1").unwrap().unwrap().attempt.unwrap();
    assert_eq!(attempt.request_id, "r1", "the original stamp is untouched");
    assert_eq!(attempt.claimed_at, at, "a refused admission does not re-stamp");
}

#[test]
fn an_ambiguous_outcome_never_grants_write_admission_again_under_the_same_id() {
    let dir = tempfile::tempdir().unwrap();
    let journal = journal_in(&dir);
    let at = clock();
    open(&journal, card("p1", "v1-aaa"), at).unwrap();
    claim_for_delivery(&journal, "p1", "s1", "r1", &yes(), at).unwrap();
    record(&journal, "p1", DeliveryOutcome::OutcomeUnknown { request_id: "r1".into() }, at + 1)
        .unwrap();

    assert_eq!(
        claim_for_delivery(&journal, "p1", "s1", "r1", &yes(), at + 2).unwrap(),
        Claim::InFlight
    );
    let stored = get(&journal, "p1").unwrap().unwrap();
    assert!(stored.is_open(), "an ambiguous card is never auto-resolved");
    assert_eq!(
        stored.attempt.as_ref().and_then(|attempt| attempt.outcome.as_deref()),
        Some("outcomeUnknown")
    );
}

#[test]
fn delivery_admission_replays_an_accepted_receipt_and_refuses_a_different_answer() {
    let dir = tempfile::tempdir().unwrap();
    let journal = journal_in(&dir);
    let at = clock();
    open(&journal, card("p1", "v1-aaa"), at).unwrap();
    claim_for_delivery(&journal, "p1", "s1", "r1", &yes(), at).unwrap();
    record(
        &journal,
        "p1",
        DeliveryOutcome::Accepted { request_id: "r1".into(), receipt: receipt("r1") },
        at + 1,
    )
    .unwrap();

    assert_eq!(
        claim_for_delivery(&journal, "p1", "s1", "r2", &yes(), at + 2).unwrap(),
        Claim::Replayed(receipt("r1")),
        "a retry after acceptance hands back the receipt for the same answer"
    );
    assert_eq!(
        claim_for_delivery(&journal, "p1", "s1", "r3", &no(), at + 3).unwrap(),
        Claim::AlreadyAnswered,
        "a different answer to a settled card is refused, never adopted"
    );

    let reopened = journal_in(&dir);
    let stored = get(&reopened, "p1").unwrap().unwrap();
    assert_eq!(
        stored.resolution.as_ref().and_then(|resolution| resolution.receipt.clone()),
        Some(receipt("r1")),
        "the receipt survives a restart"
    );
    assert_eq!(stored.attempt.unwrap().answer.unwrap(), yes());
}

#[test]
fn a_conflicting_answer_during_a_pending_delivery_is_refused_and_not_adopted() {
    let dir = tempfile::tempdir().unwrap();
    let journal = journal_in(&dir);
    let at = clock();
    open(&journal, card("p1", "v1-aaa"), at).unwrap();
    assert_eq!(
        claim_for_delivery(&journal, "p1", "s1", "r1", &yes(), at).unwrap(),
        Claim::Claimed
    );

    assert_eq!(
        claim_for_delivery(&journal, "p1", "s1", "r2", &no(), at + 1).unwrap(),
        Claim::InFlight,
        "a different answer cannot join a delivery that is already in progress"
    );
    // The row still carries the digest that was claimed, so any receipt recorded next can only
    // certify the answer that was actually written.
    let attempt = get(&journal, "p1").unwrap().unwrap().attempt.unwrap();
    assert_eq!(attempt.request_id, "r1");
    assert_eq!(attempt.answer_digest, answer_digest(&yes()));
    assert_eq!(attempt.answer, Some(yes()));

    assert_eq!(
        claim_for_delivery(&journal, "p1", "s1", "r1", &no(), at + 2).unwrap(),
        Claim::InFlight,
        "the same request id cannot swap the answer it is delivering mid-delivery"
    );
    assert_eq!(
        get(&journal, "p1").unwrap().unwrap().attempt.unwrap().answer,
        Some(yes())
    );
}

#[test]
fn a_late_accepted_outcome_cannot_overwrite_another_outstanding_request() {
    let dir = tempfile::tempdir().unwrap();
    let journal = journal_in(&dir);
    let at = clock();
    open(&journal, card("p1", "v1-aaa"), at).unwrap();
    claim_for_delivery(&journal, "p1", "s1", "r1", &yes(), at).unwrap();
    record(
        &journal,
        "p1",
        DeliveryOutcome::NotTyped { request_id: "r1".into(), code: "CONTROL_CONFLICT".into() },
        at + 1,
    )
    .unwrap();
    // A person answers again: a new request now holds the write.
    claim_for_delivery(&journal, "p1", "s1", "r2", &yes(), at + 2).unwrap();

    // The first request's Accepted arrives late. It must not close the card, and it must not
    // replace the second request's stamp with its own receipt.
    record(
        &journal,
        "p1",
        DeliveryOutcome::Accepted { request_id: "r1".into(), receipt: receipt("r1") },
        at + 3,
    )
    .unwrap();
    let stored = get(&journal, "p1").unwrap().unwrap();
    assert!(stored.is_open(), "the outstanding delivery is still outstanding");
    assert!(stored.resolution.is_none());
    assert_eq!(stored.attempt.unwrap().request_id, "r2");
}

#[test]
fn a_late_accepted_outcome_for_a_settled_card_cannot_reopen_or_repair_it() {
    let dir = tempfile::tempdir().unwrap();
    let journal = journal_in(&dir);
    let at = clock();
    open(&journal, card("p1", "v1-aaa"), at).unwrap();
    claim_for_delivery(&journal, "p1", "s1", "r1", &yes(), at).unwrap();
    record(
        &journal,
        "p1",
        DeliveryOutcome::Accepted { request_id: "r1".into(), receipt: receipt("r1") },
        at + 1,
    )
    .unwrap();

    // A second Accepted for an unrelated request id, and one for the settled id at a later time.
    record(
        &journal,
        "p1",
        DeliveryOutcome::Accepted { request_id: "r9".into(), receipt: receipt("r9") },
        at + 2,
    )
    .unwrap();
    record(
        &journal,
        "p1",
        DeliveryOutcome::Accepted { request_id: "r1".into(), receipt: receipt("r1") },
        at + 3,
    )
    .unwrap();

    let stored = get(&journal, "p1").unwrap().unwrap();
    assert_eq!(
        stored.resolution.unwrap(),
        PendingInputResolution {
            at: at + 1,
            outcome: "accepted".into(),
            receipt: Some(receipt("r1")),
        },
        "the settled receipt and its timestamp are never rewritten"
    );
    assert_eq!(stored.attempt.unwrap().request_id, "r1");
}

#[test]
fn a_late_accepted_outcome_cannot_settle_a_card_the_pane_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let journal = journal_in(&dir);
    let at = clock();
    open(&journal, card("p1", "v1-aaa"), at).unwrap();
    claim_for_delivery(&journal, "p1", "s1", "r1", &yes(), at).unwrap();
    resolve_gone(&journal, "p1", at + 1).unwrap();

    record(
        &journal,
        "p1",
        DeliveryOutcome::Accepted { request_id: "r1".into(), receipt: receipt("r1") },
        at + 2,
    )
    .unwrap();
    let resolution = get(&journal, "p1").unwrap().unwrap().resolution.unwrap();
    assert_eq!(resolution.outcome, "gone", "a gone card is not retroactively accepted");
    assert!(resolution.receipt.is_none());
}

#[test]
fn an_accepted_outcome_with_no_outstanding_attempt_is_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let journal = journal_in(&dir);
    let at = clock();
    open(&journal, card("p1", "v1-aaa"), at).unwrap();

    record(
        &journal,
        "p1",
        DeliveryOutcome::Accepted {
            request_id: "never-claimed".into(),
            receipt: receipt("never-claimed"),
        },
        at + 1,
    )
    .unwrap();
    let stored = get(&journal, "p1").unwrap().unwrap();
    assert!(stored.resolution.is_none(), "no claim, no certification");
    assert!(stored.attempt.is_none(), "a receipt never mints an attempt");
    assert_eq!(
        claim_for_delivery(&journal, "p1", "s1", "r1", &yes(), at + 2).unwrap(),
        Claim::Claimed,
        "the card is still answerable"
    );
}

#[test]
fn a_late_not_typed_refusal_cannot_clear_a_settled_attempt() {
    let dir = tempfile::tempdir().unwrap();
    let journal = journal_in(&dir);
    let at = clock();
    open(&journal, card("p1", "v1-aaa"), at).unwrap();
    claim_for_delivery(&journal, "p1", "s1", "r1", &yes(), at).unwrap();
    record(
        &journal,
        "p1",
        DeliveryOutcome::Accepted { request_id: "r1".into(), receipt: receipt("r1") },
        at + 1,
    )
    .unwrap();

    record(
        &journal,
        "p1",
        DeliveryOutcome::NotTyped { request_id: "r1".into(), code: "SCREEN_MOVED".into() },
        at + 2,
    )
    .unwrap();
    let stored = get(&journal, "p1").unwrap().unwrap();
    assert!(stored.last_refusal.is_none(), "a settled card gains no new refusal");
    assert_eq!(
        stored.attempt.expect("the digest its receipt certifies survives").request_id,
        "r1"
    );
    assert_eq!(
        claim_for_delivery(&journal, "p1", "s1", "r2", &yes(), at + 3).unwrap(),
        Claim::Replayed(receipt("r1")),
        "the replay still answers from the settled receipt"
    );
}

#[test]
fn a_late_ambiguous_report_cannot_unsettle_an_accepted_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let journal = journal_in(&dir);
    let at = clock();
    open(&journal, card("p1", "v1-aaa"), at).unwrap();
    claim_for_delivery(&journal, "p1", "s1", "r1", &yes(), at).unwrap();
    record(
        &journal,
        "p1",
        DeliveryOutcome::Accepted { request_id: "r1".into(), receipt: receipt("r1") },
        at + 1,
    )
    .unwrap();

    record(&journal, "p1", DeliveryOutcome::OutcomeUnknown { request_id: "r1".into() }, at + 2)
        .unwrap();
    let stored = get(&journal, "p1").unwrap().unwrap();
    assert_eq!(
        stored.resolution.as_ref().and_then(|resolution| resolution.receipt.clone()),
        Some(receipt("r1")),
        "a settled receipt stays settled"
    );
    assert_eq!(
        stored.attempt.as_ref().and_then(|attempt| attempt.outcome.as_deref()),
        Some("accepted")
    );
    assert!(!stored.is_in_crash_window());
    assert_eq!(
        claim_for_delivery(&journal, "p1", "s1", "r2", &yes(), at + 3).unwrap(),
        Claim::Replayed(receipt("r1")),
        "the card is not offered for a second write"
    );
}

#[test]
fn the_relaxed_claim_still_finishes_the_attempt_it_already_holds() {
    let dir = tempfile::tempdir().unwrap();
    let journal = journal_in(&dir);
    let at = clock();
    open(&journal, card("p1", "v1-aaa"), at).unwrap();

    assert_eq!(claim(&journal, "p1", "s1", "r1", &yes(), at).unwrap(), Claim::Claimed);
    assert_eq!(
        claim(&journal, "p1", "s1", "r1", &yes(), at + 1).unwrap(),
        Claim::Claimed,
        "re-presenting the id it claimed finishes that attempt"
    );
    assert_eq!(
        claim(&journal, "p1", "s1", "r1", &no(), at + 2).unwrap(),
        Claim::InFlight,
        "even the held id cannot swap the answer that attempt is delivering"
    );
    assert_eq!(get(&journal, "p1").unwrap().unwrap().attempt.unwrap().answer, Some(yes()));
    // The narrower admission refuses the same shape, which is the whole point of having both.
    assert_eq!(
        claim_for_delivery(&journal, "p1", "s1", "r1", &yes(), at + 3).unwrap(),
        Claim::InFlight
    );
}

#[test]
fn the_relaxed_claim_also_refuses_a_conflicting_answer_mid_delivery() {
    let dir = tempfile::tempdir().unwrap();
    let journal = journal_in(&dir);
    let at = clock();
    open(&journal, card("p1", "v1-aaa"), at).unwrap();
    claim(&journal, "p1", "s1", "r1", &yes(), at).unwrap();

    assert_eq!(
        claim(&journal, "p1", "s1", "r2", &no(), at + 1).unwrap(),
        Claim::InFlight,
        "the relaxed entry point refuses a conflicting answer while an attempt is outstanding"
    );
    assert_eq!(get(&journal, "p1").unwrap().unwrap().attempt.unwrap().answer, Some(yes()));
}

#[test]
fn a_not_typed_refusal_still_frees_the_next_claim_for_either_entry_point() {
    let dir = tempfile::tempdir().unwrap();
    let journal = journal_in(&dir);
    let at = clock();
    open(&journal, card("p1", "v1-aaa"), at).unwrap();
    claim_for_delivery(&journal, "p1", "s1", "r1", &yes(), at).unwrap();
    record(
        &journal,
        "p1",
        DeliveryOutcome::NotTyped { request_id: "r1".into(), code: "SCREEN_MOVED".into() },
        at + 1,
    )
    .unwrap();

    assert_eq!(
        claim_for_delivery(&journal, "p1", "s1", "r2", &no(), at + 2).unwrap(),
        Claim::Claimed,
        "a typed-nothing refusal never spends the card's one write"
    );
    record(
        &journal,
        "p1",
        DeliveryOutcome::NotTyped { request_id: "r2".into(), code: "SCREEN_MOVED".into() },
        at + 3,
    )
    .unwrap();
    assert_eq!(claim(&journal, "p1", "s1", "r3", &yes(), at + 4).unwrap(), Claim::Claimed);
}

fn owned(journal: &Journal, prompt_id: &str, session_id: &str) -> Option<PendingInput> {
    match get_for_session(journal, prompt_id, session_id).unwrap() {
        PendingInputEntry::Owned(record) => Some(*record),
        PendingInputEntry::Absent | PendingInputEntry::Elsewhere => None,
    }
}

fn card_in(session_id: &str, prompt_id: &str, revision: &str) -> NewPendingInput {
    NewPendingInput { session_id: session_id.into(), ..card(prompt_id, revision) }
}

#[test]
fn a_duplicate_dismissal_keeps_the_moment_the_person_resolved_the_card() {
    let dir = tempfile::tempdir().unwrap();
    let journal = journal_in(&dir);
    let at = clock();
    open(&journal, card("p1", "v1-aaa"), at).unwrap();
    claim(&journal, "p1", "s1", "r1", &yes(), at).unwrap();
    record(&journal, "p1", DeliveryOutcome::OutcomeUnknown { request_id: "r1".into() }, at + 1)
        .unwrap();

    dismiss_ambiguous_for_screen(&journal, "p1", "s1", "v1-aaa", at + 2).unwrap();
    let settled = get(&journal, "p1").unwrap().unwrap().resolution.expect("dismissed");
    assert_eq!(
        settled,
        PendingInputResolution {
            at: at + 2,
            outcome: "dismissedAmbiguous".into(),
            receipt: None,
        }
    );

    // The same dismissal arriving late must settle nothing a second time.
    dismiss_ambiguous_for_screen(&journal, "p1", "s1", "v1-aaa", at + 90).unwrap();
    let after = get(&journal, "p1").unwrap().unwrap();
    assert_eq!(
        after.resolution.clone().expect("still dismissed"),
        settled,
        "a delayed duplicate does not move the time the person resolved it"
    );
    assert_eq!(
        after.attempt.as_ref().and_then(|attempt| attempt.outcome.as_deref()),
        Some("outcomeUnknown"),
        "the ambiguity is not rewritten, and the card is not reopened"
    );
    assert!(!after.is_open());

    // The pane-less entry point applies the same rule to the row it already settled.
    dismiss_ambiguous(&journal, "p1", at + 91).unwrap();
    assert_eq!(get(&journal, "p1").unwrap().unwrap().resolution.expect("dismissed"), settled);
}

#[test]
fn two_panes_observing_the_same_public_id_hold_two_independent_cards() {
    let dir = tempfile::tempdir().unwrap();
    let at = clock();
    let journal = journal_in(&dir);
    let first = open_observed(&journal, card_in("s1", "p1", "v1-aaa"), at).unwrap();
    let second = open_observed(&journal, card_in("s2", "p1", "v1-aaa"), at + 1).unwrap();

    assert_eq!(first.prompt_id, second.prompt_id, "the public id is the card's content hash");
    assert_eq!(first.session_id, "s1");
    assert_eq!(second.session_id, "s2");
    assert_eq!(
        journal.open_pending_inputs().unwrap().len(),
        2,
        "the same approval on two panes is two occurrences"
    );
    assert!(
        journal.pending_input("p1").unwrap().is_none(),
        "the pane-less read refuses an id two panes hold"
    );

    assert_eq!(
        claim_for_delivery(&journal, "p1", "s1", "ra", &yes(), at + 2).unwrap(),
        Claim::Claimed
    );
    assert_eq!(
        claim_for_delivery(&journal, "p1", "s2", "rb", &no(), at + 3).unwrap(),
        Claim::Claimed,
        "the second pane's card is not the first pane's attempt"
    );
    record_for_session(
        &journal,
        "p1",
        "s1",
        DeliveryOutcome::Accepted { request_id: "ra".into(), receipt: receipt("ra") },
        at + 4,
    )
    .unwrap();
    record_for_session(
        &journal,
        "p1",
        "s2",
        DeliveryOutcome::OutcomeUnknown { request_id: "rb".into() },
        at + 5,
    )
    .unwrap();

    // A delayed dismissal of the second pane's ambiguity settles only that pane.
    dismiss_ambiguous_for_screen(&journal, "p1", "s2", "v1-aaa", at + 6).unwrap();
    let reopened = journal_in(&dir);
    let own_a = owned(&reopened, "p1", "s1").expect("pane A kept its card");
    assert_eq!(
        own_a.resolution.expect("A is settled by its own receipt"),
        PendingInputResolution {
            at: at + 4,
            outcome: "accepted".into(),
            receipt: Some(receipt("ra")),
        },
        "pane B's dismissal never rewrites pane A's receipt"
    );
    assert_eq!(own_a.attempt.unwrap().answer, Some(yes()));

    let own_b = owned(&reopened, "p1", "s2").expect("pane B kept its card");
    assert_eq!(
        own_b.resolution.as_ref().map(|resolved| resolved.outcome.as_str()),
        Some("dismissedAmbiguous")
    );
    assert!(own_b.resolution.unwrap().receipt.is_none(), "B's dismissal settles no receipt");
    assert_eq!(own_b.attempt.unwrap().answer, Some(no()));

    assert_eq!(
        claim_for_delivery(&reopened, "p1", "s1", "ra2", &yes(), at + 7).unwrap(),
        Claim::Replayed(receipt("ra")),
        "pane A still replays its own receipt"
    );
    assert_eq!(
        claim_for_delivery(&reopened, "p1", "s2", "rb2", &no(), at + 8).unwrap(),
        Claim::AlreadyAnswered,
        "pane B is settled by its own dismissal, not by A's receipt"
    );
}

#[test]
fn a_legacy_bare_key_row_serves_its_pane_and_stops_answering_for_the_public_id() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("machine-operations.v1.json");
    // A journal from before pane-scoped identity: the row sits under the bare public id and names
    // its pane in the row itself.
    std::fs::write(
        &path,
        br#"{"version":1,"records":{},"sessions":{},"session_revision":0,"schema":55,
             "pending_inputs":{"p1":{"promptId":"p1","sessionId":"s1","screenRevision":"v1-aaa",
             "question":"Run the migration?","options":[],"kind":"approval",
             "needsConfirmation":true,"openedAt":100}}}"#,
    )
    .unwrap();

    let journal = Journal::open(path.clone());
    let legacy = owned(&journal, "p1", "s1").expect("the legacy row still serves its own pane");
    assert_eq!(legacy.opened_at, 100);
    assert_eq!(legacy.screen_revision, "v1-aaa");
    assert!(journal.pending_input("p1").unwrap().is_some(), "a lone legacy row is unambiguous");
    assert!(owned(&journal, "p1", "s2").is_none(), "another pane cannot take the legacy row");

    let at = clock();
    let observed = open_observed(&journal, card_in("s2", "p1", "v1-bbb"), at).unwrap();
    assert_eq!(observed.session_id, "s2");
    assert_eq!(observed.opened_at, at, "the second pane started its own occurrence");
    assert_eq!(
        journal.pending_input("p1").unwrap(),
        None,
        "two rows now carry this public id, so the pane-less read answers neither"
    );
    assert_eq!(
        open(&journal, card("p1", "v1-ccc"), at + 1),
        Err("PROMPT_CONFLICT".into()),
        "a pane-less open cannot say which pane it means, so it writes no third row"
    );
    assert_eq!(journal.open_pending_inputs().unwrap().len(), 2, "no phantom card was minted");

    let reopened = journal_in(&dir);
    let own_a = owned(&reopened, "p1", "s1").expect("pane A reads its original row back");
    assert_eq!(own_a.opened_at, 100);
    assert_eq!(own_a.screen_revision, "v1-aaa", "the legacy row was not migrated or rewritten");
    let own_b = owned(&reopened, "p1", "s2").expect("pane B reads its own row back");
    assert_eq!(own_b.opened_at, at);
    assert_eq!(own_b.screen_revision, "v1-bbb");

    assert_eq!(
        claim_for_delivery(&reopened, "p1", "s1", "ra", &yes(), at + 1).unwrap(),
        Claim::Claimed
    );
    assert_eq!(
        owned(&reopened, "p1", "s2").unwrap().attempt,
        None,
        "the other pane's card is untouched"
    );
}
