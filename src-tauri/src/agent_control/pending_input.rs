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
//!    read twice, or answered twice, finds the one row already there.
//! 2. [`claim`] stamps the attempt - request id, answer digest and the answer itself - *before*
//!    any byte is typed. A row that carries a claim and no outcome is exactly the crash window
//!    between commit and delivery.
//! 3. [`record`] stores the outcome, and on `Accepted` the delivery receipt.
//!
//! A crash between 1 and 3 leaves a claimed row with no outcome. Because the PTY write may already
//! have happened, this is an ambiguous external effect and must not be replayed automatically.
//!
//! ## What idempotency is guaranteed
//!
//! One `prompt_id` produces exactly one delivery:
//!
//! * answering again with the same answer returns the stored receipt and types nothing
//!   ([`Claim::Replayed`]);
//! * answering the same `prompt_id` with a *different* answer is refused
//!   ([`Claim::AlreadyAnswered`]) instead of silently discarding the new one;
//! * a second caller of an attempt still in flight is refused ([`Claim::InFlight`]) rather than
//!   typing twice.
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

/// One attempt to deliver an answer. Stamped by [`claim`] before the write, completed by
/// [`record`].
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
    /// This caller owns the write. The row was stamped and persisted.
    Claimed,
    /// This `prompt_id` was already delivered with this same answer: hand back the receipt that
    /// delivery produced, and type nothing.
    Replayed(crate::scoped_contracts::DeliveryReceipt),
    /// This `prompt_id` was answered with a different answer already. Refused, nothing typed.
    AlreadyAnswered,
    /// Another caller holds an attempt that has not finished. Refused, nothing typed.
    InFlight,
    /// This journal never recorded the card (a predecessor journal, or an answer that reached a
    /// daemon that had not read the card). Nothing is durable for it, so the caller decides: the
    /// shared answer path still validates the screen, it just cannot promise at-most-once.
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
fn claim_one(
    record: &mut PendingInput,
    request_id: &str,
    digest: &str,
    answer: &serde_json::Value,
    now: u64,
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
    if let Some(attempt) = &record.attempt {
        if attempt.outcome.is_none() || attempt.outcome.as_deref() == Some("outcomeUnknown") {
            return Claim::InFlight;
        }
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

/// Store what the delivery did. Returns whether the record changed.
fn record_one(record: &mut PendingInput, outcome: DeliveryOutcome, now: u64) -> bool {
    match outcome {
        DeliveryOutcome::Accepted { request_id, receipt } => {
            record.attempt = Some(PendingInputAttempt {
                request_id: request_id.clone(),
                claimed_at: record
                    .attempt
                    .as_ref()
                    .map_or(now, |attempt| attempt.claimed_at),
                answer_digest: record
                    .attempt
                    .as_ref()
                    .map(|attempt| attempt.answer_digest.clone())
                    .unwrap_or_default(),
                answer: record.attempt.as_ref().and_then(|attempt| attempt.answer.clone()),
                outcome: Some("accepted".into()),
            });
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
            // next answer - by anyone, with any request id - is free to claim.
            match record.attempt.as_ref() {
                Some(attempt) if attempt.request_id == request_id => {}
                _ => return false,
            }
            record.attempt = None;
            record.last_refusal = Some(code);
            true
        }
        DeliveryOutcome::OutcomeUnknown { request_id } => {
            let Some(attempt) = record.attempt.as_mut() else {
                return false;
            };
            if attempt.request_id != request_id {
                return false;
            }
            // Kept as a completed-but-unresolved attempt: the card is still shown, and a retry
            // with a fresh request id is allowed, because the answer path refuses a replay whose
            // screen already moved.
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

/// Read the pending input `prompt_id` names, answered or still open.
pub fn get(journal: &Journal, prompt_id: &str) -> Result<Option<PendingInput>, String> {
    journal.pending_input(prompt_id)
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

/// Stamp the intent to deliver `answer` under `request_id`, then tell the caller whether to write.
pub fn claim(
    journal: &Journal,
    prompt_id: &str,
    session_id: &str,
    request_id: &str,
    answer: &serde_json::Value,
    now: u64,
) -> Result<Claim, String> {
    let digest = answer_digest(answer);
    let answer = answer.clone();
    journal.update_pending_input(prompt_id, move |existing| {
        let Some(mut record) = existing else {
            // Nothing was ever recorded for this id on this daemon. The caller is told so it can
            // decide; the shared answer path still validates the screen.
            return (None, false, Claim::Unrecorded);
        };
        if record.session_id != session_id {
            return (None, false, Claim::AlreadyAnswered);
        }
        let claim = claim_one(&mut record, request_id, &digest, &answer, now);
        let changed = claim == Claim::Claimed;
        (Some(record), changed, claim)
    })
}

/// Store what the delivery did. A write that fails is reported, and the caller must not treat the
/// delivery as un-typed: the bytes may already be in the pane, and the row then stays in the crash
/// window, which is exactly the state a restart retries.
pub fn record(journal: &Journal, prompt_id: &str, outcome: DeliveryOutcome, now: u64) -> Result<(), String> {
    journal.update_pending_input(prompt_id, move |existing| {
        let Some(mut record) = existing else {
            return (None, false, Err(unavailable()));
        };
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

pub fn dismiss_ambiguous(journal: &Journal, prompt_id: &str, now: u64) -> Result<(), String> {
    journal.update_pending_input(prompt_id, move |existing| {
        let Some(mut record) = existing else {
            return (None, false, Err("PROMPT_NOT_FOUND".into()));
        };
        let ambiguous = record.is_in_crash_window()
            || record.attempt.as_ref().is_some_and(|attempt| attempt.outcome.as_deref() == Some("outcomeUnknown"));
        if !ambiguous {
            return (Some(record), false, Err("PROMPT_NOT_AMBIGUOUS".into()));
        }
        record.resolution = Some(PendingInputResolution { at: now, outcome: "dismissedAmbiguous".into(), receipt: None });
        (Some(record), true, Ok(()))
    })?
}

pub fn dismiss_ambiguous_for_screen(
    journal: &Journal,
    prompt_id: &str,
    session_id: &str,
    screen_revision: &str,
    now: u64,
) -> Result<(), String> {
    journal.update_pending_input(prompt_id, move |existing| {
        let Some(record) = existing else {
            return (None, false, Err("PROMPT_NOT_FOUND".into()));
        };
        if record.session_id != session_id || record.screen_revision != screen_revision {
            return (Some(record), false, Err("PROMPT_CONFLICT".into()));
        }
        let ambiguous = record.is_in_crash_window()
            || record.attempt.as_ref().is_some_and(|attempt| attempt.outcome.as_deref() == Some("outcomeUnknown"));
        if !ambiguous {
            return (Some(record), false, Err("PROMPT_NOT_AMBIGUOUS".into()));
        }
        let mut record = record;
        record.resolution = Some(PendingInputResolution { at: now, outcome: "dismissedAmbiguous".into(), receipt: None });
        (Some(record), true, Ok(()))
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
