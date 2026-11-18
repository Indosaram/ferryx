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
    // A fresh answer is still allowed: the shared answer path refuses the replay if it landed.
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
