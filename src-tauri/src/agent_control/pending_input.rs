//! Pending user-input requests: a question an agent pane asked that has not been answered yet,
//! kept in the daemon's own journal so it survives a daemon restart.
//!
//! ## Why this exists
//!
//! The panes belong to the daemon and a prompt is read from the pane's screen, so before this
//! module an open question existed only as long as the process that detected it: `ipc::agent_prompt`
//! proxied straight to the daemon and nothing was written down. A daemon restart mid-question left
//! the desktop with no way to name, show or answer the card the agent was still blocked on.
//!
//! ## The concurrency shape
//!
//! Answering is a PTY write, and a PTY write cannot join a journal transaction. So the answer is
//! recorded in the three steps the existing outbox obligations already use
//! (`delivery_obligations`, `chat_outbox.send_claim`): **commit intent, then deliver, then record
//! the receipt.**
//!
//! 1. [`open`] inserts the request under its `prompt_id`. It is idempotent: the same `prompt_id`
//!    read twice, or answered twice, finds the one row already there. A real screen read uses
//!    [`open_observed`], which scopes the row to the pane that observed the card.
//! 2. [`claim`] (or, for a parallel delivery worker, [`claim_for_delivery`]) stamps the attempt -
//!    request id, answer digest and the answer itself - *before* any byte is typed. A row that
//!    carries a claim and no outcome is exactly the crash window between commit and delivery.
//! 3. [`record`] (or [`record_for_session`], when the caller knows the pane that claimed) stores
//!    the outcome, and on `Accepted` the delivery receipt.
//!
//! A crash between 1 and 3 leaves a claimed row with no outcome. Because the PTY write may already
//! have happened, this is an ambiguous external effect and must not be replayed automatically.
//!
//! ## Identity
//!
//! A public prompt id is the card's content hash, so it is not unique across panes: two panes
//! showing the same approval produce the same id. The durable row is therefore keyed by (pane,
//! public id) for every card a screen read actually observed, and the id on the wire is unchanged.
//! A row written before that identity existed carries its pane in the row itself and is still
//! read - and written - where it sits, with no migration. Answering, recording an outcome and
//! dismissing are all pane-scoped, so one pane's card is never replayed, settled or dismissed by
//! another pane's request.
//!
//! ## What idempotency is guaranteed
//!
//! One `prompt_id` produces exactly one delivery:
//!
//! * answering again with the same answer returns the stored receipt and types nothing
//!   ([`Claim::Replayed`]);
//! * answering the same `prompt_id` with a *different* answer is refused rather than silently
//!   discarding the new one ([`Claim::AlreadyAnswered`] once the card is settled,
//!   [`Claim::InFlight`] while an attempt is still outstanding);
//! * a second caller of an attempt still in flight is refused ([`Claim::InFlight`]) rather than
//!   typing twice.
//!
//! ## Write admission
//!
//! Two entry points share one rule table, and they differ only in one respect. [`claim`] keeps the
//! relaxation the callers that re-present the attempt they already hold rely on: it lets such a
//! caller finish that attempt. [`claim_for_delivery`] is the narrow admission a parallel delivery
//! worker asks before it writes: **only an attempt stamped by that call is a write right**, and
//! an attempt already in flight - or one whose outcome is unknown - never becomes a second write
//! right, not even under the same request id.
//!
//! Neither entry point adopts a digest. An answer that conflicts with the one already stamped for
//! the card is refused, so a receipt can only ever certify the answer that was actually claimed.
//!
//! A refusal from the pane (`NotTyped`) clears the attempt and leaves the request pending: nothing
//! was typed, so the person may answer again.
//!
//! ## Clock
//!
//! Every transition takes `now` as a parameter. Nothing here reads a clock and nothing waits, so a
//! test drives the crash window directly instead of sleeping through it.

use serde::{Deserialize, Serialize};

/// The journal schema step that added the pending-input map. The daemon journal records this in
/// `Store::schema`; a journal written before it defaults to `0` and reads with no pending inputs.
///
/// Migration: `src-tauri/src/remote/journal_migrations/0055_pending_input_requests.md`.
pub const JOURNAL_SCHEMA_STEP: u32 = 55;

/// One option of the pending card, in the card's own order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PendingInputOption {
    pub key: String,
    pub label: String,
}

/// One attempt to deliver an answer. Stamped by [`claim`] or [`claim_for_delivery`] before the
/// write, completed by [`record`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PendingInputAttempt {
    pub request_id: String,
    pub claimed_at: u64,
    /// [`answer_digest`] of `answer`, so a replay with a different answer is refused, not merged.
    pub answer_digest: String,
    /// The answer itself, kept so a retried delivery sends what the person actually chose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<serde_json::Value>,
    /// How this attempt ended, when it did. `None` with a claim is the crash window: the intent is
    /// committed and the write may or may not have happened.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
}

/// Why and when the request stopped being pending.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PendingInputResolution {
    pub at: u64,
    /// `accepted` (the answer went through), or `gone` (the pane stopped showing the card).
    pub outcome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<crate::scoped_contracts::DeliveryReceipt>,
}

/// The durable record of one open question, keyed by `prompt_id`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PendingInput {
    pub prompt_id: String,
    pub session_id: String,
    /// The screen revision the card was read from. Stored, never recomputed, so a restarted
    /// daemon hands back the revision the person was looking at.
    pub screen_revision: String,
    #[serde(default)]
    pub question: String,
    #[serde(default)]
    pub options: Vec<PendingInputOption>,
    /// The card's kind as the wire spells it (`choice`, `approval`, `freeText`). Kept as the wire
    /// string so this module does not depend on the protocol enum.
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub needs_confirmation: bool,
    pub opened_at: u64,
    /// Why the last attempt was refused before anything was typed, if one was. Cleared when a new
    /// attempt is claimed. Kept on the still-open request so the desktop can say why its answer
    /// did not go out without inventing a second record for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_refusal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt: Option<PendingInputAttempt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<PendingInputResolution>,
}

impl PendingInput {
    /// Still pending: nobody answered it yet, and the pane has not been seen dropping it.
    pub fn is_open(&self) -> bool {
        self.resolution.is_none()
    }

    /// A claim with no outcome: the daemon may have died between committing the intent and
    /// recording what the write did.
    pub fn is_in_crash_window(&self) -> bool {
        self.resolution.is_none()
            && self
                .attempt
                .as_ref()
                .is_some_and(|attempt| attempt.outcome.is_none())
    }
}

/// What [`open_new`] is asked to record: the card as the pane's screen shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPendingInput {
    pub prompt_id: String,
    pub session_id: String,
    pub screen_revision: String,
    pub question: String,
    pub options: Vec<PendingInputOption>,
    pub kind: String,
    pub needs_confirmation: bool,
}

/// Whether a claim may deliver, and what a caller that may not should be told.
#[derive(Debug, Clone, PartialEq)]
pub enum Claim {
    /// This caller may write: the row carries the attempt it is delivering, stamped by this call
    /// (or, through the relaxed [`claim`] path only, by an identical retry of an attempt it holds).
    Claimed,
    /// This `prompt_id` was already delivered with this same answer: hand back the receipt that
    /// delivery produced, and type nothing.
    Replayed(crate::scoped_contracts::DeliveryReceipt),
    /// This `prompt_id` is settled - answered, dismissed, or gone - and not with this answer, or
    /// it belongs to another session. Refused, nothing typed, and no stored answer is replaced.
    AlreadyAnswered,
    /// This caller may not write, and nothing was typed. The card carries an attempt under rules
    /// this call does not meet: another caller holds it, its write right is already spent (in
    /// flight, or outcome unknown), or the answer conflicts with the one that attempt stamped.
    InFlight,
    /// This journal never recorded the card (a predecessor journal, or an answer that reached a
    /// daemon that had not read the card). Nothing is durable for it, so the caller decides: a
    /// caller that re-validates the screen itself may proceed, but cannot promise at-most-once.
    Unrecorded,
}

/// How a delivery ended. The same three shapes the shared answer path reports.
#[derive(Debug, Clone, PartialEq)]
pub enum DeliveryOutcome {
    Accepted {
        request_id: String,
        receipt: crate::scoped_contracts::DeliveryReceipt,
    },
    /// The pane refused the answer before anything was typed (the screen moved, the card was
    /// replaced, the pane went busy). The request stays pending.
    NotTyped {
        request_id: String,
        code: String,
    },
    /// The write may have happened and the caller could not learn what it did. The request stays
    /// pending, and a retry is refused by the answer path if bytes did land.
    OutcomeUnknown {
        request_id: String,
    },
}

/// What the journal holds for one public prompt id, as one pane asks about it.
///
/// The public id is a content hash, so two panes can show the same card at once. A caller therefore
/// states its own pane, and only ever sees - or writes - the row that pane owns.
#[derive(Debug, Clone, PartialEq)]
pub enum PendingInputEntry {
    /// No row carries this public id on this daemon: nothing is durable for the caller yet.
    Absent,
    /// The row this pane owns, whether it was recorded under a pane-scoped key or, by a journal
    /// written before that identity existed, under the bare public id.
    ///
    /// Boxed because `PendingInput` is several hundred bytes and the other variants carry none:
    /// an unboxed copy would grow every `Absent`/`Elsewhere` answer to the row's size.
    Owned(Box<PendingInput>),
    /// This public id is durable for other panes only. Those rows belong to those panes and this
    /// caller may neither read nor write them.
    Elsewhere,
}

/// The journal schema step an old journal is brought forward to. Called by the store, kept here so
/// the number lives beside the shape it describes.
pub fn schema_step() -> u32 {
    JOURNAL_SCHEMA_STEP
}

/// FNV-1a 64 over the canonical JSON of an answer.
///
/// Hand-rolled, like the screen revision digest, so a digest given to this journal means the same
/// thing in every process and after every restart. A randomised hasher would let two identical
/// answers disagree between the process that claimed and the one that replayed.
pub fn answer_digest(answer: &serde_json::Value) -> String {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let canonical = serde_json::to_string(answer).unwrap_or_default();
    let mut hash = OFFSET;
    for byte in canonical.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }
    format!("k1-{hash:016x}")
}

/// Commit the intent for one card.
///
/// Idempotent on `prompt_id`: the row already there is returned, never replaced. A `prompt_id`
/// that names a different session is a conflict - the id names one occurrence on one pane.
fn open_new(
    existing: Option<PendingInput>,
    request: NewPendingInput,
    now: u64,
    adopt_revision: bool,
) -> Result<PendingInput, String> {
    let NewPendingInput {
        prompt_id,
        session_id,
        screen_revision,
        question,
        options,
        kind,
        needs_confirmation,
    } = request;
    let mut record = match existing {
        None => PendingInput {
            prompt_id,
            session_id,
            screen_revision: screen_revision.clone(),
            question,
            options,
            kind,
            needs_confirmation,
            opened_at: now,
            last_refusal: None,
            attempt: None,
            resolution: None,
        },
        Some(mut record) => {
            if record.session_id != session_id {
                return Err("PROMPT_CONFLICT".into());
            }
            // An answer can be the first thing the daemon records for a card (the phone answered
            // without a read), so the read's view fields are filled in when they arrive later.
            if record.question.is_empty() && !question.is_empty() {
                record.question = question;
            }
            if record.options.is_empty() && !options.is_empty() {
                record.options = options;
            }
            if record.kind.is_empty() && !kind.is_empty() {
                record.kind = kind;
            }
            record.needs_confirmation |= needs_confirmation;
            record
        }
    };
    // The newest read of the card is the screen an answer has to name.
    if adopt_revision && record.is_open() && record.screen_revision != screen_revision {
        record.screen_revision = screen_revision;
    }
    Ok(record)
}

/// The record a screen read leaves behind: the existing row with the revision this read saw (or a
/// new row). Shared by [`open`] and the journal's whole-pane reconcile so both apply one rule.
pub(crate) fn adopt(
    existing: Option<PendingInput>,
    request: NewPendingInput,
    now: u64,
) -> Result<PendingInput, String> {
    open_new(existing, request, now, true)
}

/// Stamp the attempt for `request_id`, or say why this caller may not deliver.
///
/// `admit_held_attempt` is the whole difference between the two entry points: the relaxed entry
/// point may finish the exact attempt it already holds (its caller re-validates the screen), while
/// write admission may only ever be granted by the stamp this call makes.
fn claim_one(
    record: &mut PendingInput,
    request_id: &str,
    digest: &str,
    answer: &serde_json::Value,
    now: u64,
    admit_held_attempt: bool,
) -> Claim {
    if let Some(resolution) = &record.resolution {
        if resolution.outcome == "accepted" {
            if let Some(receipt) = resolution.receipt.clone() {
                // A replay of the same answer gets the receipt that delivery produced; a different
                // answer to the same card is not silently swallowed.
                let same = record
                    .attempt
                    .as_ref()
                    .is_some_and(|attempt| attempt.answer_digest == digest);
                return if same {
                    Claim::Replayed(receipt)
                } else {
                    Claim::AlreadyAnswered
                };
            }
        }
        return Claim::AlreadyAnswered;
    }
    // An attempt on the card holds its one write right, and only for the answer it stamped. Write
    // admission never re-grants that right - not even to the same request id, because it cannot
    // know whether the first attempt's bytes reached the pane. The relaxed entry point may finish
    // the exact attempt it already holds, because its caller re-validates the screen immediately
    // before typing it. A conflicting answer is refused either way rather than adopted into the
    // row, so no receipt recorded later can certify a digest nobody wrote.
    if let Some(attempt) = record.attempt.as_ref() {
        let finishable = admit_held_attempt
            && attempt.request_id == request_id
            && attempt.answer_digest == digest
            && (attempt.outcome.is_none()
                || attempt.outcome.as_deref() == Some("outcomeUnknown"));
        return if finishable {
            Claim::Claimed
        } else {
            Claim::InFlight
        };
    }
    record.attempt = Some(PendingInputAttempt {
        request_id: request_id.to_string(),
        claimed_at: now,
        answer_digest: digest.to_string(),
        answer: Some(answer.clone()),
        outcome: None,
    });
    record.last_refusal = None;
    Claim::Claimed
}

/// The attempt a delivery outcome may still be recorded against: the card is open and the attempt
/// is the one this request stamped and has not already settled.
fn outstanding_attempt_mut<'a>(
    record: &'a mut PendingInput,
    request_id: &str,
) -> Option<&'a mut PendingInputAttempt> {
    if record.resolution.is_some() {
        return None;
    }
    record.attempt.as_mut().filter(|attempt| {
        attempt.request_id == request_id
            && (attempt.outcome.is_none()
                || attempt.outcome.as_deref() == Some("outcomeUnknown"))
    })
}

/// Store what the delivery did. Returns whether the record changed.
fn record_one(record: &mut PendingInput, outcome: DeliveryOutcome, now: u64) -> bool {
    match outcome {
        DeliveryOutcome::Accepted { request_id, receipt } => {
            // A receipt certifies the answer the matching request claimed. An outcome that names no
            // outstanding request is a late report: adopting it would overwrite another caller's
            // in-flight attempt, reopen a card a person already settled, or pair the receipt with
            // a digest nobody wrote. Refused, and the row stays exactly as it was.
            let Some(attempt) = outstanding_attempt_mut(record, &request_id) else {
                return false;
            };
            attempt.outcome = Some("accepted".into());
            record.resolution = Some(PendingInputResolution {
                at: now,
                outcome: "accepted".into(),
                receipt: Some(receipt),
            });
            record.last_refusal = None;
            true
        }
        DeliveryOutcome::NotTyped { request_id, code } => {
            // Nothing was typed, so the question is still open and can be answered again. The
            // refusal is kept so the person can be told why, and the attempt is cleared so the
            // next answer - by anyone, with any request id - is free to claim. A refusal that
            // names no outstanding attempt is late: clearing a settled one would lose the digest
            // its receipt certifies.
            if outstanding_attempt_mut(record, &request_id).is_none() {
                return false;
            }
            record.attempt = None;
            record.last_refusal = Some(code);
            true
        }
        DeliveryOutcome::OutcomeUnknown { request_id } => {
            // Kept as a completed-but-unresolved attempt: the card is still shown, and nothing may
            // resend it automatically. Write admission refuses every further write under it, even
            // for this id; only [`claim`] may finish the attempt a caller already holds, and that
            // caller re-validates the screen first. A person settles this occurrence through
            // `dismiss_ambiguous_for_screen`; the row then stays settled, and only a distinct
            // occurrence - the next card the pane shows - can be answered.
            let Some(attempt) = outstanding_attempt_mut(record, &request_id) else {
                return false;
            };
            attempt.outcome = Some("outcomeUnknown".into());
            true
        }
    }
}

/// Mark a card resolved because the pane no longer shows it.
fn mark_gone(record: &mut PendingInput, now: u64) -> bool {
    if record.resolution.is_some() {
        return false;
    }
    record.resolution = Some(PendingInputResolution {
        at: now,
        outcome: "gone".into(),
        receipt: None,
    });
    true
}

/// A claim without an outcome cannot be distinguished after restart from a successful PTY write
/// whose receipt was lost. Fail closed; only a fresh pane observation or explicit user action may
/// resolve this ambiguity.
/// The daemon journal this module writes to, when the process has one.
pub type Journal = crate::remote::machine_operation_journal::MachineOperationJournal;

fn unavailable() -> String {
    "MACHINE_SERVICE_UNAVAILABLE".into()
}

/// Record that `request` is open. Idempotent on `prompt_id`; the row already there wins, except
/// that the newest screen revision is adopted so an answer names the screen the person sees.
pub fn open(
    journal: &Journal,
    request: NewPendingInput,
    now: u64,
) -> Result<PendingInput, String> {
    let prompt_id = request.prompt_id.clone();
    journal.update_pending_input(&prompt_id, move |existing| {
        let before = existing.clone();
        match adopt(existing, request, now) {
            Ok(record) => {
                // A repeated read of the same card writes nothing: the journal is on the
                // `AgentPromptGet` path, which a view may poll.
                let changed = before.as_ref() != Some(&record);
                (Some(record.clone()), changed, Ok(record))
            }
            // A refused open changes nothing, so the refusal is reported without a write.
            Err(error) => (None, false, Err(error)),
        }
    })?
}

/// Record a card a screen read actually observed on `request.session_id`.
///
/// Same row rules as [`open`], with the identity fixed: the row belongs to the pane that read it,
/// so the same public id observed on another pane is a second, independent occurrence rather than
/// a conflict. [`open`] keeps its own contract - a bare id that names a different pane is refused -
/// because it is the low-level entry point, not the detection path.
pub fn open_observed(
    journal: &Journal,
    request: NewPendingInput,
    now: u64,
) -> Result<PendingInput, String> {
    let prompt_id = request.prompt_id.clone();
    let session_id = request.session_id.clone();
    journal.update_pending_input_for_session(&prompt_id, &session_id, move |existing| {
        // Only a row this pane owns is adopted. A row another pane holds for the same public id is
        // a different card, so this pane starts its own instead of refusing the read or, worse,
        // adopting the other pane's answer.
        let own = match existing {
            PendingInputEntry::Owned(record) => Some(*record),
            PendingInputEntry::Absent | PendingInputEntry::Elsewhere => None,
        };
        let before = own.clone();
        match adopt(own, request, now) {
            Ok(record) => {
                let changed = before.as_ref() != Some(&record);
                (Some(record.clone()), changed, Ok(record))
            }
            Err(error) => (None, false, Err(error)),
        }
    })?
}

/// Read the pending input `prompt_id` names, answered or still open.
pub fn get(journal: &Journal, prompt_id: &str) -> Result<Option<PendingInput>, String> {
    journal.pending_input(prompt_id)
}

/// Read what `session_id` holds for `prompt_id`: the row that pane owns, or nothing.
///
/// The pane-scoped read. [`get`] answers only a public id the table holds exactly one row for, so a
/// caller that knows its own pane asks this and never lands on another pane's card.
pub fn get_for_session(
    journal: &Journal,
    prompt_id: &str,
    session_id: &str,
) -> Result<PendingInputEntry, String> {
    journal.pending_input_for_session(prompt_id, session_id)
}

/// The one open card `session_id` is showing at `screen_revision`, if any: what `AgentPromptGet`
/// falls back to when this daemon restarted after the card was recorded.
pub fn open_for_screen(
    journal: &Journal,
    session_id: &str,
    screen_revision: &str,
) -> Result<Option<PendingInput>, String> {
    Ok(journal
        .open_pending_inputs()?
        .into_iter()
        .filter(|input| input.session_id == session_id)
        .find(|input| input.screen_revision == screen_revision))
}

pub fn unresolved_ambiguous_for_session(
    journal: &Journal,
    session_id: &str,
) -> Result<Option<PendingInput>, String> {
    Ok(journal
        .open_pending_inputs()?
        .into_iter()
        .filter(|input| input.session_id == session_id)
        .filter(|input| {
            input.is_in_crash_window()
                || input.attempt.as_ref().is_some_and(|attempt| attempt.outcome.as_deref() == Some("outcomeUnknown"))
        })
        .min_by_key(|input| (input.opened_at, input.prompt_id.clone())))
}

/// The lock-scoped half of both entry points: refuse an unknown card or another session's card,
/// then let [`claim_one`] decide whether this caller may write.
fn claim_locked(
    journal: &Journal,
    prompt_id: &str,
    session_id: &str,
    request_id: &str,
    answer: &serde_json::Value,
    now: u64,
    admit_held_attempt: bool,
) -> Result<Claim, String> {
    let digest = answer_digest(answer);
    let answer = answer.clone();
    journal.update_pending_input_for_session(prompt_id, session_id, move |existing| {
        let mut record = match existing {
            // Nothing was ever recorded for this id on this daemon. The caller is told so it can
            // decide; nothing is minted here, so registering a real detected card stays explicit.
            PendingInputEntry::Absent => return (None, false, Claim::Unrecorded),
            // Another pane's card under the same public id. It is not this caller's to answer, and
            // it must never be replayed or settled on this caller's behalf.
            PendingInputEntry::Elsewhere => return (None, false, Claim::AlreadyAnswered),
            PendingInputEntry::Owned(record) => *record,
        };
        if record.session_id != session_id {
            return (None, false, Claim::AlreadyAnswered);
        }
        let claim = claim_one(&mut record, request_id, &digest, &answer, now, admit_held_attempt);
        let changed = claim == Claim::Claimed;
        (Some(record), changed, claim)
    })
}

/// Stamp the intent to deliver `answer` under `request_id`, then tell the caller whether to write.
///
/// This is the relaxed half of the pair: the caller that re-presents the exact attempt the journal
/// already holds - same request id, same answer - finishes that attempt instead of being refused,
/// and is expected to re-validate the screen immediately before it types. A caller that must not
/// write twice for that reason uses [`claim_for_delivery`], which grants no such thing.
pub fn claim(
    journal: &Journal,
    prompt_id: &str,
    session_id: &str,
    request_id: &str,
    answer: &serde_json::Value,
    now: u64,
) -> Result<Claim, String> {
    claim_locked(journal, prompt_id, session_id, request_id, answer, now, true)
}

/// The narrow write admission a parallel delivery worker asks for before typing.
///
/// Same rules as [`claim`] except one: only a stamp made by *this* call is a [`Claim::Claimed`].
/// An attempt already in flight, or one whose outcome is unknown, is refused as [`Claim::InFlight`]
/// even under the same `request_id`, because the worker cannot know whether its predecessor's bytes
/// reached the pane. A card this journal never recorded is [`Claim::Unrecorded`] - nothing is
/// written here, so the worker registers its real detected card through [`open`] instead.
pub fn claim_for_delivery(
    journal: &Journal,
    prompt_id: &str,
    session_id: &str,
    request_id: &str,
    answer: &serde_json::Value,
    now: u64,
) -> Result<Claim, String> {
    claim_locked(journal, prompt_id, session_id, request_id, answer, now, false)
}

/// Store what the delivery did. A write that fails is reported, and the caller must not treat the
/// delivery as un-typed: the bytes may already be in the pane, and the row then stays in the crash
/// window, which is exactly the ambiguity a restart surfaces instead of retrying.
pub fn record(journal: &Journal, prompt_id: &str, outcome: DeliveryOutcome, now: u64) -> Result<(), String> {
    journal.update_pending_input(prompt_id, move |existing| {
        let Some(mut record) = existing else {
            return (None, false, Err(unavailable()));
        };
        let changed = record_one(&mut record, outcome, now);
        (Some(record), changed, Ok(()))
    })?
}

/// [`record`] for the pane that claimed the delivery.
///
/// A late outcome names its request id, and a request id belongs to one pane, so the pane decides
/// which row it may settle. The same claim-then-record order applies: a card this pane has no row
/// for is refused rather than created, because a receipt must certify an answer that was claimed.
pub fn record_for_session(
    journal: &Journal,
    prompt_id: &str,
    session_id: &str,
    outcome: DeliveryOutcome,
    now: u64,
) -> Result<(), String> {
    journal.update_pending_input_for_session(prompt_id, session_id, move |existing| {
        let PendingInputEntry::Owned(record) = existing else {
            return (None, false, Err(unavailable()));
        };
        let mut record = *record;
        let changed = record_one(&mut record, outcome, now);
        (Some(record), changed, Ok(()))
    })?
}

/// Resolve a card the pane stopped showing. Leaves an answered card alone.
pub fn resolve_gone(journal: &Journal, prompt_id: &str, now: u64) -> Result<(), String> {
    journal.update_pending_input(prompt_id, move |existing| {
        let Some(mut record) = existing else {
            return (None, false, Ok(()));
        };
        let changed = mark_gone(&mut record, now);
        (Some(record), changed, Ok(()))
    })?
}

/// Whether this row records a delivery nobody can vouch for: a claimed attempt with no outcome, or
/// one whose outcome is unknown. Settled or not, that is what a person is asked to dismiss.
fn attempted_ambiguously(record: &PendingInput) -> bool {
    record
        .attempt
        .as_ref()
        .is_some_and(|attempt| {
            attempt.outcome.is_none() || attempt.outcome.as_deref() == Some("outcomeUnknown")
        })
}

/// Settle one ambiguous occurrence for a person. Returns whether the row changed.
///
/// A second dismissal of the same occurrence is a no-op that keeps the first one's time: a delayed
/// duplicate must neither move the moment the person resolved it nor reopen it. A row that records
/// a settled answer, or no attempt at all, was never ambiguous to begin with.
fn dismiss_ambiguous_record(record: &mut PendingInput, now: u64) -> Result<bool, String> {
    if !attempted_ambiguously(record) {
        return Err("PROMPT_NOT_AMBIGUOUS".into());
    }
    match &record.resolution {
        Some(resolution) if resolution.outcome == "dismissedAmbiguous" => return Ok(false),
        Some(_) => return Err("PROMPT_NOT_AMBIGUOUS".into()),
        None => {}
    }
    record.resolution = Some(PendingInputResolution {
        at: now,
        outcome: "dismissedAmbiguous".into(),
        receipt: None,
    });
    Ok(true)
}

pub fn dismiss_ambiguous(journal: &Journal, prompt_id: &str, now: u64) -> Result<(), String> {
    journal.update_pending_input(prompt_id, move |existing| {
        let Some(mut record) = existing else {
            return (None, false, Err("PROMPT_NOT_FOUND".into()));
        };
        match dismiss_ambiguous_record(&mut record, now) {
            Ok(changed) => (Some(record), changed, Ok(())),
            Err(error) => (Some(record), false, Err(error)),
        }
    })?
}

pub fn dismiss_ambiguous_for_screen(
    journal: &Journal,
    prompt_id: &str,
    session_id: &str,
    screen_revision: &str,
    now: u64,
) -> Result<(), String> {
    journal.update_pending_input_for_session(prompt_id, session_id, move |existing| {
        let PendingInputEntry::Owned(record) = existing else {
            return (None, false, Err("PROMPT_NOT_FOUND".into()));
        };
        let mut record = *record;
        if record.session_id != session_id || record.screen_revision != screen_revision {
            return (Some(record), false, Err("PROMPT_CONFLICT".into()));
        }
        match dismiss_ambiguous_record(&mut record, now) {
            Ok(changed) => (Some(record), changed, Ok(())),
            Err(error) => (Some(record), false, Err(error)),
        }
    })?
}

/// One screen read reconciled against the durable table: the card this read found is recorded (or
/// its revision refreshed), and every other open card of the same pane is settled as `gone`.
///
/// This is what makes the promises meet after a restart. A card the daemon recorded before it died
/// is already there, so a read that finds the same card returns that row - same `prompt_id`, same
/// stored `screen_revision` - instead of minting a second one; a read that finds no card settles the
/// row rather than leaving a phantom question behind forever.
pub fn reconcile(
    journal: &Journal,
    found: Option<NewPendingInput>,
    session_id: &str,
    now: u64,
) -> Result<Option<PendingInput>, String> {
    journal.reconcile_pending_inputs(session_id, found, now)
}

#[cfg(test)]
#[path = "pending_input_tests.rs"]
mod tests;
