//! Ordered original-pane input: submit, Stop and the queue they share (plan task 7).
//!
//! Ported from devswha/herdr-web-ui @
//! 54e5a1f67090cb09552d182e7e30dd0ecc314918 (MIT, docs/chat/HERDR_LICENSE).
//! Upstream anchors, read at the pinned revision:
//!
//! | Upstream | What this module ports |
//! |---|---|
//! | src/lib/compose.ts (composerMessage, composerPayload, MAX_COMPOSER_CHARS) | the normative
//!   byte shaping: trim the composer's own trailing CR/LF, normalize CRLF and a lone CR to LF,
//!   convert the pane's LF to CR, wrap in the paste markers only when the pane's own
//!   bracketed-paste mode is on, and submit the Enter SEPARATELY |
//! | server/index.ts (serialize, paneQueues, submitText, lastTyped, SUBMIT_DELAY_MS,
//!   TYPED_SETTLE_MS, SUBMIT_DEADLINE_MS, authorizeSocket) | the per-pane serialized transaction,
//!   the typed-settle, the deadline, the reauthorization before mutation, and the gap between the
//!   text and its Enter |
//! | server/index.ts case submit / case keys | the request-level refusals that type nothing
//!   (read_only, attach_held, agent_blocked, submit_timeout) and the one that must not
//!   (!clients.has(client)) |
//! | src/components/PaneTerminal.tsx:1332-1339 (abortTurn) | Stop is Escape, never a killing
//!   signal; the explicit terminal's Ctrl-C stays a separate, deliberate action |
//!
//! Frozen contract: docs/chat/herdr-port-contract.md section 5 (submit / Stop) and section 6 (the
//! shape_reference_submit and stop_keys_for entry points). This module owns the ordered input
//! transaction and nothing else: it declares no shared type, registers no route and edits no
//! sibling lane.
//!
//! ## What this module is, and is not
//!
//! * It NEVER launches, replaces or signals a process. Every byte reaches the pane through
//!   [ReferenceInputWriter], which the route layer implements over the EXISTING
//!   [crate::remote::backend::RemoteSessionBackend::write_input] seam ([ReferenceSessionWriter] is
//!   that adapter). There is no provider RPC, no managed child and no signal() call anywhere in
//!   this file; even the explicit terminal's Ctrl-C is the BYTE a terminal sends, not a signal
//!   delivered to a process.
//! * It performs NO authorization policy of its own. Whether a caller may mutate at all is the
//!   route layer's decision, handed in as the authorize callback; this module only decides WHEN
//!   that question is asked, which is what makes a mid-flight revoke safe.
//! * It NEVER guesses an outcome. A write that was dispatched and could not be confirmed is
//!   [ReferenceInputOutcome::OutcomeUnknown]: the pending record is held and is never replayed. A
//!   refusal that typed nothing says so and leaves no record behind.
//!
//! ## The ordering rule
//!
//! Submit, Stop and prompt answers share one queue PER TARGET, so a Stop tapped right after Send
//! cannot land between the text and its Enter. The queue is keyed on the owning target
//! ([reference_draft_key]: host + owner + daemon incarnation + backend session), never on a visual
//! leaf id and never globally - two panes' input do not wait on each other.
//!
//! ## Accept-then-unknown, and why it is not a retry
//!
//! The delivery ladder ends at [DeliveryStage::Accepted] here: the writer took the bytes. That is
//! not the provider having consumed them, and for a source with no native reader it is where the
//! ladder stops. When a write was dispatched and its confirmation was lost - a writer error, a
//! transport loss, or a reauthorization that refuses AFTER the paste already went - the honest
//! answer is [ReferenceInputOutcome::OutcomeUnknown] carrying [REFERENCE_OUTCOME_UNKNOWN_CODE],
//! never a silent success and never an automatic replay.
//!
//! ## One integration need this lane does not own
//!
//! The contract section 3 widening - ScopeErrorCode::OperationOutcomeUnknown with an explicit
//! serde rename to OPERATION_OUTCOME_UNKNOWN - belongs to the owner of
//! src-tauri/src/scoped_contracts.rs (contract section 8), not to this file. Until it lands, the
//! unknown outcome is carried by this lane's own variant with the wire string
//! [REFERENCE_OUTCOME_UNKNOWN_CODE] that types.rs already ships (and reference_is_outcome_unknown
//! already recognises); once the variant exists it maps 1:1 onto
//! ScopeError { code: OperationOutcomeUnknown, retryable: false }. A refusal that typed nothing
//! carries a real, frozen [ScopeErrorCode] today.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::scoped_contracts::{DeliveryReceipt, DeliveryStage, ScopeError, ScopeErrorCode};

use super::types::{
    reference_draft_key, ReferenceStopCapability, ReferenceStopPayload, ReferenceSubmitOrigin,
    ReferenceSubmitPayload, ReferenceTargetRef, REFERENCE_OUTCOME_UNKNOWN_CODE,
    REFERENCE_SUBMIT_MAX_CHARS,
};

/// The bracketed-paste introducer the pane's program reads (compose.ts PASTE_START).
pub const REFERENCE_PASTE_START: &str = "\u{1b}[200~";

/// The bracketed-paste terminator (compose.ts PASTE_END).
pub const REFERENCE_PASTE_END: &str = "\u{1b}[201~";

/// Stop for a TUI that handles an interrupt key: Escape, as abortTurn sends it.
pub const REFERENCE_ESCAPE_KEY: &[u8] = b"\x1b";

/// The explicit terminal's deliberate interrupt: the Ctrl-C KEY, not a process signal.
pub const REFERENCE_SHELL_SIGNAL_KEY: &[u8] = b"\x03";

/// The submit key. It is written on its own, after the text, never inside it.
pub const REFERENCE_SUBMIT_ENTER: &[u8] = b"\r";

/// How many bytes one shaped submit may put on the wire.
///
/// The composer cap is [REFERENCE_SUBMIT_MAX_CHARS] CHARACTERS, but the transport carries BYTES,
/// so a multibyte message can fit the character cap and still need a byte budget. The budget is
/// checked BEFORE anything is written, which is the contract's rule and the reason this check
/// cannot live in the composer alone.
///
/// TRANSPORT ASSUMPTION CORRECTION. The value this replaces was a fixed 16 KiB, justified as
/// "the remote gateway refuses a text frame past 16 KiB". That figure is the machine-control
/// bound, not a submit bound: `server.rs:2249` (`text.len() > 16 * 1024`) guards `Message::Text`
/// frames parsed as `MachineTerminalControl` (`remote/protocol.rs:7-18` = `Resize | Signal |
/// Ping`, the `CONTROL_JSON_MAX_BYTES` bound), and no reference-chat mutation travels it. The
/// independent trace of the real submit path - client POST -> `relay_server.rs:1746` body bound
/// -> `Message::Binary` chunks of `MAX_MESSAGE_SIZE` -> the raw `session_transport.rs` stream
/// proxy -> the gateway `to_bytes` bound (`server.rs:5351`) - finds no 16 KiB hop. The old
/// number was also BELOW the 20,000-character cap, which made every cap-sized message
/// unshapeable ('shaping_refuses_text_past_the_composer_cap_before_anything_is_written').
///
/// The budget is therefore derived from the frozen cap and the bytes this transaction really
/// puts on the wire: the widest UTF-8 encoding of a cap-sized body
/// (`REFERENCE_SUBMIT_MAX_CHARS * 4`), the bracketed-paste framing when that mode is on, and the
/// Enter byte ([REFERENCE_SUBMIT_ENTER]) written separately from the body. This corrects a wrong
/// assumption; it does not relax a measured limit.
pub const REFERENCE_SUBMIT_MAX_BYTES: usize = REFERENCE_SUBMIT_MAX_CHARS * 4
    + REFERENCE_PASTE_START.len()
    + REFERENCE_PASTE_END.len()
    + 1; // the separately written Enter byte, [REFERENCE_SUBMIT_ENTER]

/// The gap the pane sees between a submit's text and its Enter (index.ts SUBMIT_DELAY_MS).
///
/// It is a real gap on purpose: arriving in one chunk, a TUI still busy with the paste can take
/// the Enter for a newline and leave the message unsent in its input box.
pub const REFERENCE_SUBMIT_DELAY_MS: u64 = 120;

/// How long a submit may wait behind earlier input before it types nothing
/// (index.ts SUBMIT_DEADLINE_MS).
pub const REFERENCE_SUBMIT_DEADLINE_MS: u64 = 45_000;

/// How long a paste waits after raw keystrokes reached the pane (index.ts TYPED_SETTLE_MS).
pub const REFERENCE_TYPED_SETTLE_MS: u64 = 300;

/// How many per-target serializers the queue keeps before it prunes the idle ones.
pub const REFERENCE_INPUT_LOCK_LIMIT: usize = 256;

/// How many request records the queue keeps before it evicts the oldest settled ones.
///
/// An in-flight record is never evicted: it is the only thing standing between a duplicate
/// request id and a replayed mutation.
pub const REFERENCE_INPUT_RECORD_LIMIT: usize = 256;

/// Why a submit that typed nothing did not go (the wording the caller reports).
pub const REFERENCE_SUBMIT_DEADLINE_MESSAGE: &str =
    "the message waited too long behind earlier input; nothing was typed";

/// Why a submit that typed nothing was refused while the pane holds a prompt.
pub const REFERENCE_BLOCKED_PROMPT_MESSAGE: &str =
    "the pane is waiting for an answer in the terminal; nothing was typed";

/// Why a Stop that typed nothing was refused.
pub const REFERENCE_STOP_REFUSED_MESSAGE: &str =
    "this target's interrupt capability is unknown; nothing was typed";

/// Why a request id could not be reused.
pub const REFERENCE_REQUEST_CONFLICT_MESSAGE: &str =
    "this request id was already used with a different payload";

/// Why a request id could not be reused while its first request is still in flight.
pub const REFERENCE_REQUEST_IN_FLIGHT_MESSAGE: &str =
    "this request id is already in flight; its outcome must be resolved before it is reused";

/// Why a Stop from a caller that left typed nothing.
pub const REFERENCE_CALLER_GONE_MESSAGE: &str =
    "the caller that asked for this is no longer connected; nothing was typed";

/// Shape one composer message into the bytes a pane's own program expects.
///
/// Normative from the pinned compose.ts:
///
/// 1. trailing CR/LF are the composer's, not the text's, so they are trimmed;
/// 2. CRLF and a lone CR read as one newline (LF);
/// 3. the pane's newline is CR, so every internal LF becomes CR;
/// 4. the whole body is wrapped in the paste markers ONLY when the pane's own bracketed-paste mode
///    is on - a plain shell wants classic paste semantics, where each newline runs its own line;
/// 5. the submit Enter is NOT part of this payload. It is written separately (see
///    [REFERENCE_SUBMIT_ENTER]).
///
/// Refused with [ScopeErrorCode::PayloadTooLarge] when the text is past
/// [REFERENCE_SUBMIT_MAX_CHARS] characters, or when the shaped payload PLUS the Enter this
/// transaction writes separately is past [REFERENCE_SUBMIT_MAX_BYTES] bytes. Both refusals
/// happen before any byte reaches the pane.
pub fn shape_reference_submit(text: &str, bracketed_paste: bool) -> Result<String, ScopeErrorCode> {
    if text.chars().count() > REFERENCE_SUBMIT_MAX_CHARS {
        return Err(ScopeErrorCode::PayloadTooLarge);
    }
    let body = reference_composer_body(text);
    let shaped = if bracketed_paste {
        format!("{REFERENCE_PASTE_START}{body}{REFERENCE_PASTE_END}")
    } else {
        body
    };
    // The Enter is written separately from the body (see [REFERENCE_SUBMIT_ENTER]), so what this
    // submit puts on the wire is the shaped payload PLUS that Enter. Counting it here is what
    // makes the budget tight instead of one byte of slack.
    if shaped.len() + REFERENCE_SUBMIT_ENTER.len() > REFERENCE_SUBMIT_MAX_BYTES {
        return Err(ScopeErrorCode::PayloadTooLarge);
    }
    Ok(shaped)
}

/// The body a composer message types, without its paste markers and without its submit.
///
/// Split out because the shape of the body is the part with a wire meaning: the markers are a
/// mode, the body is the message.
pub fn reference_composer_body(text: &str) -> String {
    let trimmed = text.trim_end_matches(|ch| ch == '\r' || ch == '\n');
    let normalized = trimmed.replace("\r\n", "\n").replace('\r', "\n");
    normalized.replace('\n', "\r")
}

/// The keys that stop the pane's current turn for an observed capability.
///
/// * [ReferenceStopCapability::ProviderInterrupt] - Escape, as the pinned abortTurn sends it to a
///   TUI it knows handles it.
/// * [ReferenceStopCapability::ShellSignal] - the explicit terminal's deliberate Ctrl-C KEY, which
///   is a separate action and not what chat Stop sends.
/// * [ReferenceStopCapability::Refused] - refused with [ScopeErrorCode::Unsupported]. A killing
///   signal is never substituted for unknown behavior.
pub fn stop_keys_for(capability: ReferenceStopCapability) -> Result<&'static [u8], ScopeErrorCode> {
    match capability {
        ReferenceStopCapability::ProviderInterrupt => Ok(REFERENCE_ESCAPE_KEY),
        ReferenceStopCapability::ShellSignal => Ok(REFERENCE_SHELL_SIGNAL_KEY),
        ReferenceStopCapability::Refused => Err(ScopeErrorCode::Unsupported),
    }
}

/// The wire string a [ScopeErrorCode] serializes to (REQUEST_CONFLICT, UNSUPPORTED, ...).
///
/// Read back through the enum's own serde rename so a caller branches on the same string the
/// frozen envelope carries, instead of on a second hand-written table that could drift.
pub fn scope_error_code_wire(code: ScopeErrorCode) -> String {
    match serde_json::to_value(code) {
        Ok(serde_json::Value::String(name)) => name,
        _ => String::from(REFERENCE_OUTCOME_UNKNOWN_CODE),
    }
}

/// The fingerprint a submit's dedupe/conflict rule compares.
///
/// Covers every field the mutation acts on - the text, the origin and the attachment ids - so a
/// retry that changed any of them is a conflict rather than a silent replay of the first one.
pub fn reference_submit_fingerprint(payload: &ReferenceSubmitPayload) -> String {
    let origin = match payload.origin {
        ReferenceSubmitOrigin::Chat => "chat",
        ReferenceSubmitOrigin::Terminal => "terminal",
    };
    let mut parts: Vec<&[u8]> =
        vec![b"ferryx.reference-chat.submit.v1", payload.text.as_bytes(), origin.as_bytes()];
    for attachment in &payload.attachment_ids {
        parts.push(attachment.as_bytes());
    }
    reference_fingerprint(&parts)
}

/// The fingerprint a Stop's dedupe/conflict rule compares.
pub fn reference_stop_fingerprint(payload: &ReferenceStopPayload) -> String {
    let capability = match payload.capability {
        ReferenceStopCapability::ProviderInterrupt => "providerInterrupt",
        ReferenceStopCapability::ShellSignal => "shellSignal",
        ReferenceStopCapability::Refused => "refused",
    };
    reference_fingerprint(&[b"ferryx.reference-chat.stop.v1", capability.as_bytes()])
}

fn reference_fingerprint(parts: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    // Every part is length-prefixed, so "ab" and "a"+"b" cannot collide.
    for part in parts {
        hasher.update((part.len() as u64).to_le_bytes());
        hasher.update(part);
    }
    format!("{:x}", hasher.finalize())
}

/// What happened to one ordered input transaction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ReferenceInputOutcome {
    /// Every byte the transaction writes was accepted by the writer.
    ///
    /// This is [DeliveryStage::Accepted], not providerRead: the writer took the bytes, which is
    /// not the provider having consumed them.
    Accepted { receipt: DeliveryReceipt },
    /// Refused before any byte reached the pane: nothing was typed, and no record was kept, so the
    /// same request id may be used again.
    NotTyped { error: ScopeError },
    /// Bytes may have reached the pane and the outcome cannot be determined.
    ///
    /// code is [REFERENCE_OUTCOME_UNKNOWN_CODE] and the outcome is not retryable: the caller holds
    /// the pending record until it is resolved explicitly. Never auto-replay.
    OutcomeUnknown { code: String, message: String },
}

impl ReferenceInputOutcome {
    /// Did the writer accept every byte of this transaction?
    pub fn is_typed(&self) -> bool {
        matches!(self, Self::Accepted { .. })
    }

    /// Is the outcome undetermined, with bytes possibly already in the pane?
    pub fn is_outcome_unknown(&self) -> bool {
        matches!(self, Self::OutcomeUnknown { .. })
    }

    /// The receipt, when the transaction completed.
    pub fn receipt(&self) -> Option<&DeliveryReceipt> {
        match self {
            Self::Accepted { receipt } => Some(receipt),
            _ => None,
        }
    }

    /// The wire error code a caller branches on, or None when the transaction completed.
    pub fn wire_code(&self) -> Option<String> {
        match self {
            Self::Accepted { .. } => None,
            Self::NotTyped { error, .. } => Some(scope_error_code_wire(error.code)),
            Self::OutcomeUnknown { code, .. } => Some(code.clone()),
        }
    }

    /// The message a caller reports, or None when the transaction completed.
    pub fn wire_message(&self) -> Option<&str> {
        match self {
            Self::Accepted { .. } => None,
            Self::NotTyped { error, .. } => Some(error.message.as_str()),
            Self::OutcomeUnknown { message, .. } => Some(message.as_str()),
        }
    }

    fn not_typed(code: ScopeErrorCode, retryable: bool, message: impl Into<String>) -> Self {
        Self::NotTyped { error: scope_error(code, retryable, message) }
    }

    fn outcome_unknown(message: impl Into<String>) -> Self {
        Self::OutcomeUnknown {
            code: String::from(REFERENCE_OUTCOME_UNKNOWN_CODE),
            message: message.into(),
        }
    }

    fn accepted(request_id: &str, target: &ReferenceTargetRef) -> Self {
        Self::Accepted {
            receipt: DeliveryReceipt {
                request_id: request_id.to_string(),
                target: target.target.clone(),
                stage: DeliveryStage::Accepted,
            },
        }
    }
}

fn scope_error(code: ScopeErrorCode, retryable: bool, message: impl Into<String>) -> ScopeError {
    ScopeError { code, message: message.into(), retryable, details: serde_json::Value::Null }
}

/// The only side effect this lane has: bytes reaching the original pane.
///
/// Implemented over the existing session writer ([ReferenceSessionWriter]) or by a test double.
/// There is deliberately no second method: this lane cannot signal a process, resize a pane or
/// start a provider, because it has nothing to call.
pub trait ReferenceInputWriter: Send + Sync {
    /// Write data to the pane of session_id.
    ///
    /// Err means the write was dispatched and could not be confirmed - NOT that nothing reached
    /// the pane. The caller treats it as accept-then-unknown.
    fn write<'a>(&'a self, session_id: &'a str, data: &'a [u8]) -> BoxFuture<'a, Result<(), String>>;
}

/// [ReferenceInputWriter] over the existing remote session backend.
///
/// The pane is addressed by its own backend session id, so the transaction writes to the process
/// that is already running. Nothing here can create one.
pub struct ReferenceSessionWriter<'a> {
    backend: &'a dyn crate::remote::backend::RemoteSessionBackend,
}

impl<'a> ReferenceSessionWriter<'a> {
    pub fn new(backend: &'a dyn crate::remote::backend::RemoteSessionBackend) -> Self {
        Self { backend }
    }
}

impl<'b> ReferenceInputWriter for ReferenceSessionWriter<'b> {
    fn write<'a>(&'a self, session_id: &'a str, data: &'a [u8]) -> BoxFuture<'a, Result<(), String>> {
        self.backend.write_input(session_id, data)
    }
}

/// Time as the transaction sees it.
///
/// Split out so the gap between a paste and its Enter, the typed-settle and the deadline are
/// behaviour under a controlled clock rather than timing luck in a test.
pub trait ReferenceInputClock: Send + Sync {
    /// Milliseconds since the Unix epoch.
    fn now_ms(&self) -> u64;
    /// Wait ms of this clock's time.
    fn sleep_ms(&self, ms: u64) -> BoxFuture<'static, ()>;
}

/// The real clock: wall time, and a real wait.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemReferenceClock;

impl ReferenceInputClock for SystemReferenceClock {
    fn now_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis() as u64)
            .unwrap_or(0)
    }

    fn sleep_ms(&self, ms: u64) -> BoxFuture<'static, ()> {
        Box::pin(async move { tokio::time::sleep(Duration::from_millis(ms)).await })
    }
}

/// One submit the route layer has authorized and wants typed.
pub struct ReferenceSubmitRequest<'a> {
    /// The owning target: host + owner + daemon incarnation + backend session.
    pub target: &'a ReferenceTargetRef,
    /// The request id the mutation envelope carries; it keys the dedupe record.
    pub request_id: &'a str,
    /// The submit payload. The text is what gets shaped; attachment ids reach the agent as @path
    /// mentions inside it, never as a managed-turn attachment.
    pub payload: &'a ReferenceSubmitPayload,
    /// The pane's OWN bracketed-paste mode, as the caller read it from the pane.
    pub bracketed_paste: bool,
    /// The prompt the pane is holding, when the caller's screen read saw one. A chat message is
    /// not typed into it; the explicit terminal is how the user answers.
    pub blocked_prompt: Option<&'a str>,
    /// When the request arrived, in the clock's milliseconds. The deadline is measured from here,
    /// so a message that waited behind another types nothing rather than landing late.
    pub arrived_at_ms: u64,
    /// When raw keystrokes last reached this pane, if any did.
    pub last_typed_at_ms: Option<u64>,
    /// Whether this caller may mutate at all. Asked before the first byte and again before the
    /// Enter, so a revoke that lands mid-flight cannot be ignored.
    pub authorize: &'a (dyn Fn() -> Result<(), ScopeError> + Send + Sync),
}

/// One Stop the route layer has authorized and wants sent.
pub struct ReferenceStopRequest<'a> {
    /// The owning target.
    pub target: &'a ReferenceTargetRef,
    /// The request id the mutation envelope carries.
    pub request_id: &'a str,
    /// The stop payload: what the caller observed about the pane's ability to stop.
    pub payload: &'a ReferenceStopPayload,
    /// Whether this caller may mutate at all.
    pub authorize: &'a (dyn Fn() -> Result<(), ScopeError> + Send + Sync),
    /// Whether the caller is still there. A Stop from a caller that left types nothing.
    pub alive: &'a (dyn Fn() -> bool + Send + Sync),
}

/// The per-target input queue: submit, Stop and prompt answers take one step at a time.
#[derive(Debug, Default)]
pub struct ReferenceInputQueue {
    target_locks: parking_lot::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    records: parking_lot::Mutex<ReferenceInputRecords>,
}

impl ReferenceInputQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Type one composer message into its original pane, then its Enter.
    ///
    /// The whole transaction - reauthorize, write the shaped text, wait the gap, reauthorize, write
    /// the Enter - runs inside this target's queue, so no other input for this target can land
    /// between the text and its Enter.
    pub async fn submit(
        &self,
        request: &ReferenceSubmitRequest<'_>,
        writer: &dyn ReferenceInputWriter,
        clock: &dyn ReferenceInputClock,
    ) -> ReferenceInputOutcome {
        // Validation is pure and comes first: a malformed request is malformed whatever its id, and
        // it must never be recorded as if something had been attempted.
        let shaped = match shape_reference_submit(&request.payload.text, request.bracketed_paste) {
            Ok(shaped) => shaped,
            Err(code) => {
                return ReferenceInputOutcome::not_typed(
                    code,
                    false,
                    "the message is larger than one submit may carry; nothing was typed",
                );
            }
        };

        let key = submit_record_key(request.target, request.request_id);
        let fingerprint = reference_submit_fingerprint(request.payload);
        match self.claim(&key, &fingerprint) {
            Claim::Fresh => {}
            Claim::Duplicate(state) => return state.into_outcome(),
            Claim::InFlight => {
                return ReferenceInputOutcome::not_typed(
                    ScopeErrorCode::RequestConflict,
                    false,
                    REFERENCE_REQUEST_IN_FLIGHT_MESSAGE,
                );
            }
            Claim::Conflict => {
                return ReferenceInputOutcome::not_typed(
                    ScopeErrorCode::RequestConflict,
                    false,
                    REFERENCE_REQUEST_CONFLICT_MESSAGE,
                );
            }
        }

        let outcome = self.run_submit(request, &shaped, writer, clock).await;
        self.settle(&key, &outcome);
        outcome
    }

    /// Send Stop's keys to its original pane.
    ///
    /// It takes the same per-target step as a submit, which is the whole point: a Stop tapped right
    /// after Send lands after the message's Enter, never between it and its text.
    pub async fn stop(
        &self,
        request: &ReferenceStopRequest<'_>,
        writer: &dyn ReferenceInputWriter,
    ) -> ReferenceInputOutcome {
        let keys = match stop_keys_for(request.payload.capability) {
            Ok(keys) => keys,
            Err(code) => {
                return ReferenceInputOutcome::not_typed(code, false, REFERENCE_STOP_REFUSED_MESSAGE);
            }
        };

        let key = stop_record_key(request.target, request.request_id);
        let fingerprint = reference_stop_fingerprint(request.payload);
        match self.claim(&key, &fingerprint) {
            Claim::Fresh => {}
            Claim::Duplicate(state) => return state.into_outcome(),
            Claim::InFlight => {
                return ReferenceInputOutcome::not_typed(
                    ScopeErrorCode::RequestConflict,
                    false,
                    REFERENCE_REQUEST_IN_FLIGHT_MESSAGE,
                );
            }
            Claim::Conflict => {
                return ReferenceInputOutcome::not_typed(
                    ScopeErrorCode::RequestConflict,
                    false,
                    REFERENCE_REQUEST_CONFLICT_MESSAGE,
                );
            }
        }

        let outcome = self.run_stop(request, keys, writer).await;
        self.settle(&key, &outcome);
        outcome
    }

    async fn run_submit(
        &self,
        request: &ReferenceSubmitRequest<'_>,
        shaped: &str,
        writer: &dyn ReferenceInputWriter,
        clock: &dyn ReferenceInputClock,
    ) -> ReferenceInputOutcome {
        let _step = self.step(request.target).await;

        // A paste right behind raw keystrokes waits for the pane to finish reading them, so the
        // paste is not swallowed by the program's own redraw.
        if let Some(last_typed_at_ms) = request.last_typed_at_ms {
            let since = clock.now_ms().saturating_sub(last_typed_at_ms);
            if since < REFERENCE_TYPED_SETTLE_MS {
                clock.sleep_ms(REFERENCE_TYPED_SETTLE_MS - since).await;
            }
        }

        // Reauthorize, then the deadline: both before a single byte is written.
        if let Err(error) = (request.authorize)() {
            return ReferenceInputOutcome::NotTyped { error };
        }
        if clock.now_ms().saturating_sub(request.arrived_at_ms) > REFERENCE_SUBMIT_DEADLINE_MS {
            return ReferenceInputOutcome::not_typed(
                ScopeErrorCode::Timeout,
                true,
                REFERENCE_SUBMIT_DEADLINE_MESSAGE,
            );
        }

        // A message the chat types into a pane whose program is holding a menu is refused; the
        // explicit terminal is how the user answers it.
        if request.payload.origin == ReferenceSubmitOrigin::Chat {
            if let Some(prompt) = request.blocked_prompt {
                return ReferenceInputOutcome::not_typed(
                    ScopeErrorCode::ControlConflict,
                    false,
                    format!("{REFERENCE_BLOCKED_PROMPT_MESSAGE} ({prompt})"),
                );
            }
        }

        let session_id = request.target.target.backend_session_id.as_str();
        if let Err(message) = writer.write(session_id, shaped.as_bytes()).await {
            return ReferenceInputOutcome::outcome_unknown(format!(
                "the message may have reached the pane and could not be confirmed: {message}"
            ));
        }

        // The gap the pane sees between the paste and its Enter.
        clock.sleep_ms(REFERENCE_SUBMIT_DELAY_MS).await;

        // The text is already out, so a refusal here cannot un-type it: the outcome is unknown,
        // not "nothing was typed".
        if let Err(error) = (request.authorize)() {
            return ReferenceInputOutcome::outcome_unknown(format!(
                "the message was typed and its Enter was refused: {}",
                error.message
            ));
        }

        if let Err(message) = writer.write(session_id, REFERENCE_SUBMIT_ENTER).await {
            return ReferenceInputOutcome::outcome_unknown(format!(
                "the message was typed and its Enter could not be confirmed: {message}"
            ));
        }

        ReferenceInputOutcome::accepted(request.request_id, request.target)
    }

    async fn run_stop(
        &self,
        request: &ReferenceStopRequest<'_>,
        keys: &'static [u8],
        writer: &dyn ReferenceInputWriter,
    ) -> ReferenceInputOutcome {
        let _step = self.step(request.target).await;

        if let Err(error) = (request.authorize)() {
            return ReferenceInputOutcome::NotTyped { error };
        }
        // A key pressed by a caller that has gone since is not pressed. This is the one gate a
        // submit does not have: a message already asked for is finished for a caller that left,
        // while other input for that pane is dropped.
        if !(request.alive)() {
            return ReferenceInputOutcome::not_typed(
                ScopeErrorCode::Unauthorized,
                false,
                REFERENCE_CALLER_GONE_MESSAGE,
            );
        }

        let session_id = request.target.target.backend_session_id.as_str();
        match writer.write(session_id, keys).await {
            Ok(()) => ReferenceInputOutcome::accepted(request.request_id, request.target),
            Err(message) => ReferenceInputOutcome::outcome_unknown(format!(
                "the stop may have reached the pane and could not be confirmed: {message}"
            )),
        }
    }

    /// Run `op` while holding this target's step.
    ///
    /// The one shared serialization primitive. `submit`, `stop` and the prompt answer route
    /// (plan task 9) all take the SAME per-target step through here, so a Stop or an answer
    /// tapped while a message is going out waits for that message's Enter instead of landing
    /// between it and its text.
    ///
    /// The step is keyed on the owning target ([reference_draft_key]), never on a visual leaf
    /// id and never globally: two panes' input do not wait on each other. A caller that needs an
    /// answer to be atomic with a submit runs both through this one entry.
    pub async fn run_under_step<T, F>(&self, target: &ReferenceTargetRef, op: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        let _step = self.step(target).await;
        op.await
    }

    /// Take this target's step. Only input for the SAME target waits here.
    async fn step(&self, target: &ReferenceTargetRef) -> tokio::sync::OwnedMutexGuard<()> {
        self.lock_for(&reference_draft_key(target)).lock_owned().await
    }

    fn lock_for(&self, target_key: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self.target_locks.lock();
        if locks.len() > REFERENCE_INPUT_LOCK_LIMIT {
            locks.retain(|_, lock| Arc::strong_count(lock) > 1);
        }
        locks
            .entry(target_key.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    fn claim(&self, key: &str, fingerprint: &str) -> Claim {
        self.records.lock().claim(key, fingerprint)
    }

    fn settle(&self, key: &str, outcome: &ReferenceInputOutcome) {
        self.records.lock().settle(key, outcome);
    }
}

#[derive(Debug, Default)]
struct ReferenceInputRecords {
    entries: HashMap<String, ReferenceInputRecord>,
    order: VecDeque<String>,
}

impl ReferenceInputRecords {
    fn claim(&mut self, key: &str, fingerprint: &str) -> Claim {
        match self.entries.get(key) {
            Some(record) if record.fingerprint != fingerprint => Claim::Conflict,
            Some(record) => match &record.settled {
                Some(state) => Claim::Duplicate(state.clone()),
                None => Claim::InFlight,
            },
            None => {
                self.entries.insert(
                    key.to_string(),
                    ReferenceInputRecord { fingerprint: fingerprint.to_string(), settled: None },
                );
                self.order.push_back(key.to_string());
                self.evict_settled();
                Claim::Fresh
            }
        }
    }

    fn settle(&mut self, key: &str, outcome: &ReferenceInputOutcome) {
        match outcome {
            // Nothing was typed, so there is nothing to remember and nothing to replay: the id is
            // free again.
            ReferenceInputOutcome::NotTyped { .. } => {
                if self.entries.remove(key).is_some() {
                    self.order.retain(|candidate| candidate != key);
                }
            }
            ReferenceInputOutcome::Accepted { receipt } => {
                self.record(key, ReferenceInputRecordState::Accepted(receipt.clone()));
            }
            ReferenceInputOutcome::OutcomeUnknown { message, .. } => {
                self.record(key, ReferenceInputRecordState::Unknown(message.clone()));
            }
        }
    }

    fn record(&mut self, key: &str, state: ReferenceInputRecordState) {
        if let Some(record) = self.entries.get_mut(key) {
            record.settled = Some(state);
        }
    }

    fn evict_settled(&mut self) {
        let mut inspected = 0usize;
        while self.entries.len() > REFERENCE_INPUT_RECORD_LIMIT && inspected < self.order.len() {
            let Some(key) = self.order.pop_front() else { break };
            inspected += 1;
            if self.entries.get(&key).is_some_and(|record| record.settled.is_some()) {
                self.entries.remove(&key);
            } else {
                // In flight: never dropped, because dropping it would let the same id be replayed.
                self.order.push_back(key);
            }
        }
    }
}

#[derive(Debug)]
struct ReferenceInputRecord {
    fingerprint: String,
    settled: Option<ReferenceInputRecordState>,
}

#[derive(Debug, Clone)]
enum ReferenceInputRecordState {
    Accepted(DeliveryReceipt),
    Unknown(String),
}

impl ReferenceInputRecordState {
    fn into_outcome(self) -> ReferenceInputOutcome {
        match self {
            Self::Accepted(receipt) => ReferenceInputOutcome::Accepted { receipt },
            Self::Unknown(message) => ReferenceInputOutcome::outcome_unknown(message),
        }
    }
}

enum Claim {
    Fresh,
    Duplicate(ReferenceInputRecordState),
    InFlight,
    Conflict,
}

fn record_key(target: &ReferenceTargetRef, request_id: &str) -> String {
    format!("{}\u{1f}{request_id}", reference_draft_key(target))
}

/// The record key for a SUBMIT's dedupe/conflict rule.
///
/// A submit and a Stop are different mutations, so they must not share one record: the dedupe
/// rule answers a RETRY of the same request, and a Stop that happens to carry the same request
/// id as a submit is not that retry. Sharing the key answered the Stop with the submit's
/// recorded state instead of pressing Escape ('a stop is a different mutation than a submit',
/// input.rs:1580). Each kind keeps its own fingerprint, conflict and in-flight rules unchanged.
fn submit_record_key(target: &ReferenceTargetRef, request_id: &str) -> String {
    format!("submit\u{1f}{}", record_key(target, request_id))
}

/// The record key for a STOP's dedupe/conflict rule.
fn stop_record_key(target: &ReferenceTargetRef, request_id: &str) -> String {
    format!("stop\u{1f}{}", record_key(target, request_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Mutex;

    use tokio::sync::oneshot;

    use super::super::types::{reference_is_outcome_unknown, reference_stage_at_least};
    use crate::scoped_contracts::{Epoch, TargetRef};

    fn target(backend: &str) -> ReferenceTargetRef {
        ReferenceTargetRef::without_provider_session(TargetRef {
            host_id: "host-a".into(),
            owner_id: "owner-a".into(),
            epoch: Epoch(11),
            backend_session_id: backend.into(),
        })
    }

    fn submit_payload(text: &str) -> ReferenceSubmitPayload {
        ReferenceSubmitPayload {
            text: text.into(),
            attachment_ids: vec![],
            origin: ReferenceSubmitOrigin::Chat,
        }
    }

    fn stop_payload(capability: ReferenceStopCapability) -> ReferenceStopPayload {
        ReferenceStopPayload { capability }
    }

    fn allow() -> impl Fn() -> Result<(), ScopeError> + Send + Sync {
        || Ok(())
    }

    fn alive() -> impl Fn() -> bool + Send + Sync {
        || true
    }

    fn gone() -> impl Fn() -> bool + Send + Sync {
        || false
    }

    fn refusal() -> ScopeError {
        scope_error(ScopeErrorCode::Unauthorized, false, "this device's access was revoked")
    }

    /// A clock the test drives: no wall time, so a gap and a deadline are observed values.
    #[derive(Default)]
    struct ManualClock {
        now: AtomicU64,
        sleeps: Mutex<Vec<u64>>,
    }

    impl ManualClock {
        fn at(now_ms: u64) -> Arc<Self> {
            Arc::new(Self { now: AtomicU64::new(now_ms), sleeps: Mutex::new(Vec::new()) })
        }

        fn advance(&self, ms: u64) {
            self.now.fetch_add(ms, Ordering::SeqCst);
            self.sleeps.lock().expect("clock sleeps").push(ms);
        }

        fn sleeps(&self) -> Vec<u64> {
            self.sleeps.lock().expect("clock sleeps").clone()
        }
    }

    // The trait is implemented for the shared handle so sleep_ms can return a future that owns
    // the clock it advances.
    impl ReferenceInputClock for Arc<ManualClock> {
        fn now_ms(&self) -> u64 {
            self.now.load(Ordering::SeqCst)
        }

        fn sleep_ms(&self, ms: u64) -> BoxFuture<'static, ()> {
            let clock = Arc::clone(self);
            Box::pin(async move { clock.advance(ms) })
        }
    }

    /// A writer that records every byte it is handed, can refuse once, and can hold its first
    /// write open so the test can observe what is serialized behind it.
    #[derive(Default)]
    struct RecordingWriter {
        writes: Mutex<Vec<Vec<u8>>>,
        gate: AtomicBool,
        started: Mutex<Option<oneshot::Sender<()>>>,
        release: tokio::sync::Mutex<Option<oneshot::Receiver<()>>>,
        fail_next: AtomicBool,
    }

    impl RecordingWriter {
        fn new() -> Arc<Self> {
            Arc::new(Self::default())
        }

        /// A writer whose first write signals started and then waits for release.
        fn gated() -> (Arc<Self>, oneshot::Receiver<()>, oneshot::Sender<()>) {
            let (started_tx, started_rx) = oneshot::channel();
            let (release_tx, release_rx) = oneshot::channel();
            let writer = Arc::new(Self {
                gate: AtomicBool::new(true),
                started: Mutex::new(Some(started_tx)),
                release: tokio::sync::Mutex::new(Some(release_rx)),
                ..Self::default()
            });
            (writer, started_rx, release_tx)
        }

        fn fail_next_write(&self) {
            self.fail_next.store(true, Ordering::SeqCst);
        }

        fn writes(&self) -> Vec<Vec<u8>> {
            self.writes.lock().expect("writer writes").clone()
        }

        fn typed(&self) -> String {
            self.writes()
                .iter()
                .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
                .collect()
        }
    }

    impl ReferenceInputWriter for RecordingWriter {
        fn write<'a>(
            &'a self,
            _session_id: &'a str,
            data: &'a [u8],
        ) -> BoxFuture<'a, Result<(), String>> {
            Box::pin(async move {
                if self.fail_next.swap(false, Ordering::SeqCst) {
                    // A dispatched write that could not be confirmed, not a write that never went.
                    return Err("the writer could not confirm the write".into());
                }
                if self.gate.swap(false, Ordering::SeqCst) {
                    let release = self.release.lock().await.take();
                    if let Some(started) = self.started.lock().expect("writer started").take() {
                        let _ = started.send(());
                    }
                    if let Some(release) = release {
                        let _ = release.await;
                    }
                }
                self.writes.lock().expect("writer writes").push(data.to_vec());
                Ok(())
            })
        }
    }

    async fn submit_ok(
        queue: &ReferenceInputQueue,
        writer: &dyn ReferenceInputWriter,
        clock: &dyn ReferenceInputClock,
        target: &ReferenceTargetRef,
        request_id: &str,
        payload: &ReferenceSubmitPayload,
    ) -> ReferenceInputOutcome {
        let authorize = allow();
        let request = ReferenceSubmitRequest {
            target,
            request_id,
            payload,
            bracketed_paste: true,
            blocked_prompt: None,
            arrived_at_ms: 0,
            last_typed_at_ms: None,
            authorize: &authorize,
        };
        queue.submit(&request, writer, clock).await
    }

    async fn stop_ok(
        queue: &ReferenceInputQueue,
        writer: &dyn ReferenceInputWriter,
        target: &ReferenceTargetRef,
        request_id: &str,
        payload: &ReferenceStopPayload,
    ) -> ReferenceInputOutcome {
        let authorize = allow();
        let alive = alive();
        let request = ReferenceStopRequest {
            target,
            request_id,
            payload,
            authorize: &authorize,
            alive: &alive,
        };
        queue.stop(&request, writer).await
    }

    // ---- compose.ts: the normative byte shaping ----------------------------------------------

    #[test]
    fn the_body_keeps_inner_newlines_and_drops_the_composer_own_trailing_ones() {
        assert_eq!(reference_composer_body("line one\r\nline two\n\n"), "line one\rline two");
        assert_eq!(reference_composer_body("a\rb\rc"), "a\rb\rc");
        assert_eq!(reference_composer_body("cmd\n"), "cmd");
        assert_eq!(reference_composer_body(""), "");
    }

    #[test]
    fn bracketed_mode_wraps_the_body_as_one_paste_and_keeps_inner_newlines_literal() {
        assert_eq!(shape_reference_submit("hello", true).unwrap(), "\u{1b}[200~hello\u{1b}[201~");
        assert_eq!(
            shape_reference_submit("line one\nline two", true).unwrap(),
            "\u{1b}[200~line one\rline two\u{1b}[201~"
        );
        assert_eq!(shape_reference_submit("", true).unwrap(), "\u{1b}[200~\u{1b}[201~");
    }

    #[test]
    fn plain_mode_uses_classic_paste_semantics_where_every_newline_runs_its_own_line() {
        assert_eq!(
            shape_reference_submit("git status\ngit diff", false).unwrap(),
            "git status\rgit diff"
        );
        assert_eq!(shape_reference_submit("git status", false).unwrap(), "git status");
        assert_eq!(shape_reference_submit("cmd\n\n", false).unwrap(), "cmd");
        assert_eq!(shape_reference_submit("", false).unwrap(), "");
    }

    #[test]
    fn the_submit_enter_is_never_part_of_the_shaped_payload() {
        // Arriving in the paste's own chunk, the Enter is what a busy TUI takes for a newline.
        for bracketed in [true, false] {
            let shaped = shape_reference_submit("one", bracketed).unwrap();
            assert!(!shaped.ends_with('\r'), "the submit key leaked into the payload: {shaped:?}");
            assert!(!shaped.ends_with('\n'), "a newline would submit the message: {shaped:?}");
        }
        assert_eq!(REFERENCE_SUBMIT_ENTER, b"\r");
    }

    #[test]
    fn shaping_refuses_text_past_the_composer_cap_before_anything_is_written() {
        assert!(shape_reference_submit(&"x".repeat(REFERENCE_SUBMIT_MAX_CHARS), false).is_ok());
        assert_eq!(
            shape_reference_submit(&"x".repeat(REFERENCE_SUBMIT_MAX_CHARS + 1), false),
            Err(ScopeErrorCode::PayloadTooLarge)
        );
    }

    #[test]
    fn shaping_admits_the_full_composer_cap_and_counts_the_framing_and_enter() {
        // This replaces a test that asserted a 30,000-byte Hangul message was refused by a fixed
        // 16 KiB budget. That budget was the machine-control bound, not this path's (see the
        // constant's doc), so what is asserted here is what the frozen cap admits and what the
        // byte budget counts: the body, the paste framing, and the separately written Enter.
        assert_eq!(
            REFERENCE_SUBMIT_MAX_BYTES,
            REFERENCE_SUBMIT_MAX_CHARS * 4
                + REFERENCE_PASTE_START.len()
                + REFERENCE_PASTE_END.len()
                + REFERENCE_SUBMIT_ENTER.len()
        );

        // 10,000 Hangul characters (30,000 bytes) sit well inside the cap and shape.
        let hangul = "\u{ac00}".repeat(10_000);
        assert!(hangul.chars().count() < REFERENCE_SUBMIT_MAX_CHARS);
        let shaped = shape_reference_submit(&hangul, true).expect("30,000 bytes fit the budget");
        assert!(shaped.starts_with(REFERENCE_PASTE_START));
        assert_eq!(
            shaped.len(),
            REFERENCE_PASTE_START.len() + 30_000 + REFERENCE_PASTE_END.len()
        );
        assert!(shaped.len() + REFERENCE_SUBMIT_ENTER.len() <= REFERENCE_SUBMIT_MAX_BYTES);

        // A whole cap of four-byte characters (80,000 bytes) plus its framing and Enter is
        // exactly the budget: the framing bytes are counted, not left as slack.
        let four_byte = "\u{10000}".repeat(REFERENCE_SUBMIT_MAX_CHARS);
        assert_eq!(four_byte.chars().count(), REFERENCE_SUBMIT_MAX_CHARS);
        let shaped =
            shape_reference_submit(&four_byte, true).expect("a full cap of 4-byte chars fits");
        assert_eq!(
            shaped.len(),
            REFERENCE_SUBMIT_MAX_CHARS * 4 + REFERENCE_PASTE_START.len() + REFERENCE_PASTE_END.len()
        );
        assert_eq!(shaped.len() + REFERENCE_SUBMIT_ENTER.len(), REFERENCE_SUBMIT_MAX_BYTES);
    }

    #[tokio::test]
    async fn an_oversized_message_is_refused_without_a_write_and_its_id_stays_free() {
        let queue = ReferenceInputQueue::new();
        let writer = RecordingWriter::new();
        let clock = ManualClock::at(0);
        let target = target("sess-a");
        let huge = submit_payload(&"x".repeat(REFERENCE_SUBMIT_MAX_CHARS + 1));

        let refused = submit_ok(&queue, writer.as_ref(), &clock, &target, "r1", &huge).await;
        assert_eq!(refused.wire_code().as_deref(), Some("PAYLOAD_TOO_LARGE"));
        assert!(writer.writes().is_empty());

        // Nothing was typed, so the id is free for the message the user actually sends next.
        let accepted =
            submit_ok(&queue, writer.as_ref(), &clock, &target, "r1", &submit_payload("hello")).await;
        assert!(accepted.is_typed());
    }

    // ---- the ordered submit transaction ------------------------------------------------------

    #[tokio::test]
    async fn submit_writes_the_paste_then_waits_the_gap_then_writes_its_enter() {
        let queue = ReferenceInputQueue::new();
        let writer = RecordingWriter::new();
        let clock = ManualClock::at(1_000);
        let target = target("sess-a");
        let payload = submit_payload("line one\nline two");

        let outcome = submit_ok(&queue, writer.as_ref(), &clock, &target, "r1", &payload).await;

        assert_eq!(
            writer.writes(),
            vec![
                format!("{REFERENCE_PASTE_START}line one\rline two{REFERENCE_PASTE_END}").into_bytes(),
                REFERENCE_SUBMIT_ENTER.to_vec(),
            ]
        );
        // The pane saw the text and its Enter as two writes with a real gap between them.
        assert_eq!(clock.sleeps(), vec![REFERENCE_SUBMIT_DELAY_MS]);
        assert!(outcome.is_typed());
    }

    #[tokio::test]
    async fn an_accepted_submit_reports_the_accepted_stage_and_its_own_target() {
        let queue = ReferenceInputQueue::new();
        let writer = RecordingWriter::new();
        let clock = ManualClock::at(0);
        let target = target("sess-a");

        let outcome =
            submit_ok(&queue, writer.as_ref(), &clock, &target, "r1", &submit_payload("hello")).await;

        let receipt = outcome.receipt().expect("an accepted submit carries a receipt");
        assert_eq!(receipt.stage, DeliveryStage::Accepted);
        assert_eq!(receipt.request_id, "r1");
        assert_eq!(receipt.target, target.target);
        // The ladder stops at accepted: the writer took the bytes, not the provider.
        assert!(!reference_stage_at_least(receipt.stage, DeliveryStage::ProviderRead));
    }

    #[tokio::test]
    async fn a_submit_that_waited_past_its_deadline_types_nothing() {
        let queue = ReferenceInputQueue::new();
        let writer = RecordingWriter::new();
        let clock = ManualClock::at(REFERENCE_SUBMIT_DEADLINE_MS + 1);
        let target = target("sess-a");

        let outcome =
            submit_ok(&queue, writer.as_ref(), &clock, &target, "r1", &submit_payload("late")).await;

        assert_eq!(outcome.wire_code().as_deref(), Some("TIMEOUT"));
        assert_eq!(outcome.wire_message(), Some(REFERENCE_SUBMIT_DEADLINE_MESSAGE));
        assert!(writer.writes().is_empty(), "a message past its deadline must type nothing");
    }

    #[tokio::test]
    async fn a_chat_message_is_refused_while_the_pane_holds_a_prompt_and_the_terminal_still_answers_it() {
        let queue = ReferenceInputQueue::new();
        let writer = RecordingWriter::new();
        let clock = ManualClock::at(0);
        let target = target("sess-a");
        let authorize = allow();
        let payload = submit_payload("y");

        let chat = ReferenceSubmitRequest {
            target: &target,
            request_id: "r1",
            payload: &payload,
            bracketed_paste: true,
            blocked_prompt: Some("Would you like to run the following command?"),
            arrived_at_ms: 0,
            last_typed_at_ms: None,
            authorize: &authorize,
        };
        let refused = queue.submit(&chat, writer.as_ref(), &clock).await;
        assert_eq!(refused.wire_code().as_deref(), Some("CONTROL_CONFLICT"));
        assert!(writer.writes().is_empty(), "a chat message is not typed into a live menu");

        // The explicit terminal is how the user answers the menu: it is deliberately not refused.
        let mut terminal_payload = submit_payload("y");
        terminal_payload.origin = ReferenceSubmitOrigin::Terminal;
        let terminal = ReferenceSubmitRequest {
            target: &target,
            request_id: "r2",
            payload: &terminal_payload,
            bracketed_paste: true,
            blocked_prompt: Some("Would you like to run the following command?"),
            arrived_at_ms: 0,
            last_typed_at_ms: None,
            authorize: &authorize,
        };
        let typed = queue.submit(&terminal, writer.as_ref(), &clock).await;
        assert!(typed.is_typed());
        assert_eq!(writer.writes().len(), 2);
    }

    #[tokio::test]
    async fn a_revoked_caller_types_nothing() {
        let queue = ReferenceInputQueue::new();
        let writer = RecordingWriter::new();
        let clock = ManualClock::at(0);
        let target = target("sess-a");
        let payload = submit_payload("hello");
        let authorize = || Err(refusal());

        let request = ReferenceSubmitRequest {
            target: &target,
            request_id: "r1",
            payload: &payload,
            bracketed_paste: true,
            blocked_prompt: None,
            arrived_at_ms: 0,
            last_typed_at_ms: None,
            authorize: &authorize,
        };
        let outcome = queue.submit(&request, writer.as_ref(), &clock).await;

        assert_eq!(outcome.wire_code().as_deref(), Some("UNAUTHORIZED"));
        assert!(writer.writes().is_empty());
        assert!(clock.sleeps().is_empty(), "a refusal must not wait the submit gap");
    }

    #[tokio::test]
    async fn a_paste_right_behind_keystrokes_waits_for_the_pane_to_finish_reading_them() {
        let queue = ReferenceInputQueue::new();
        let writer = RecordingWriter::new();
        let clock = ManualClock::at(1_000);
        let target = target("sess-a");
        let payload = submit_payload("hello");
        let authorize = allow();

        let request = ReferenceSubmitRequest {
            target: &target,
            request_id: "r1",
            payload: &payload,
            bracketed_paste: true,
            blocked_prompt: None,
            arrived_at_ms: 1_000,
            last_typed_at_ms: Some(900),
            authorize: &authorize,
        };
        assert!(queue.submit(&request, writer.as_ref(), &clock).await.is_typed());

        assert_eq!(clock.sleeps(), vec![REFERENCE_TYPED_SETTLE_MS - 100, REFERENCE_SUBMIT_DELAY_MS]);
    }

    #[tokio::test]
    async fn a_revoke_after_the_paste_cannot_untype_it() {
        let queue = ReferenceInputQueue::new();
        let writer = RecordingWriter::new();
        let clock = ManualClock::at(0);
        let target = target("sess-a");
        let payload = submit_payload("hello");
        let calls = AtomicU64::new(0);
        let authorize = || {
            if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                Ok(())
            } else {
                Err(refusal())
            }
        };

        let request = ReferenceSubmitRequest {
            target: &target,
            request_id: "r1",
            payload: &payload,
            bracketed_paste: true,
            blocked_prompt: None,
            arrived_at_ms: 0,
            last_typed_at_ms: None,
            authorize: &authorize,
        };
        let outcome = queue.submit(&request, writer.as_ref(), &clock).await;

        // The text is already out: this is accept-then-unknown, never "nothing was typed".
        assert!(outcome.is_outcome_unknown());
        assert_eq!(outcome.wire_code().as_deref(), Some(REFERENCE_OUTCOME_UNKNOWN_CODE));
        assert!(reference_is_outcome_unknown(&outcome.wire_code().expect("a code")));
        assert_eq!(
            writer.writes(),
            vec![format!("{REFERENCE_PASTE_START}hello{REFERENCE_PASTE_END}").into_bytes()]
        );
    }

    #[tokio::test]
    async fn a_write_that_cannot_be_confirmed_is_outcome_unknown_and_never_a_success() {
        let queue = ReferenceInputQueue::new();
        let writer = RecordingWriter::new();
        writer.fail_next_write();
        let clock = ManualClock::at(0);
        let target = target("sess-a");

        let outcome =
            submit_ok(&queue, writer.as_ref(), &clock, &target, "r1", &submit_payload("hello")).await;

        assert!(outcome.is_outcome_unknown());
        assert_eq!(outcome.wire_code().as_deref(), Some(REFERENCE_OUTCOME_UNKNOWN_CODE));
        assert!(outcome.receipt().is_none());
        // No Enter was attempted: the transaction stopped where its confirmation was lost.
        assert!(writer.writes().is_empty());
    }

    // ---- Stop: Escape, and never a killing signal --------------------------------------------

    #[test]
    fn stop_sends_escape_for_a_provider_interrupt_and_the_ctrl_c_key_only_for_the_shell_path() {
        assert_eq!(
            stop_keys_for(ReferenceStopCapability::ProviderInterrupt).unwrap(),
            REFERENCE_ESCAPE_KEY
        );
        assert_eq!(REFERENCE_ESCAPE_KEY, b"\x1b");
        assert_eq!(
            stop_keys_for(ReferenceStopCapability::ShellSignal).unwrap(),
            REFERENCE_SHELL_SIGNAL_KEY
        );
        // The explicit terminal's Ctrl-C is the KEY a terminal sends, not a process signal.
        assert_eq!(REFERENCE_SHELL_SIGNAL_KEY, b"\x03");
    }

    #[test]
    fn an_unknown_stop_capability_is_refused_rather_than_guessed() {
        assert_eq!(
            stop_keys_for(ReferenceStopCapability::Refused),
            Err(ScopeErrorCode::Unsupported)
        );
    }

    #[tokio::test]
    async fn a_refused_stop_types_nothing_and_keeps_no_record() {
        let queue = ReferenceInputQueue::new();
        let writer = RecordingWriter::new();
        let target = target("sess-a");
        let payload = stop_payload(ReferenceStopCapability::Refused);

        let outcome = stop_ok(&queue, writer.as_ref(), &target, "r1", &payload).await;

        assert_eq!(outcome.wire_code().as_deref(), Some("UNSUPPORTED"));
        assert_eq!(outcome.wire_message(), Some(REFERENCE_STOP_REFUSED_MESSAGE));
        assert!(writer.writes().is_empty());
    }

    #[tokio::test]
    async fn stop_sends_escape_to_the_original_pane() {
        let queue = ReferenceInputQueue::new();
        let writer = RecordingWriter::new();
        let target = target("sess-a");
        let payload = stop_payload(ReferenceStopCapability::ProviderInterrupt);

        let outcome = stop_ok(&queue, writer.as_ref(), &target, "r1", &payload).await;

        assert!(outcome.is_typed());
        assert_eq!(writer.writes(), vec![REFERENCE_ESCAPE_KEY.to_vec()]);
        assert_eq!(outcome.receipt().expect("a receipt").stage, DeliveryStage::Accepted);
    }

    #[tokio::test]
    async fn a_stop_from_a_caller_that_left_types_nothing() {
        let queue = ReferenceInputQueue::new();
        let writer = RecordingWriter::new();
        let target = target("sess-a");
        let payload = stop_payload(ReferenceStopCapability::ProviderInterrupt);
        let authorize = allow();
        let alive = gone();

        let request = ReferenceStopRequest {
            target: &target,
            request_id: "r1",
            payload: &payload,
            authorize: &authorize,
            alive: &alive,
        };
        let outcome = queue.stop(&request, writer.as_ref()).await;

        assert_eq!(outcome.wire_code().as_deref(), Some("UNAUTHORIZED"));
        assert_eq!(outcome.wire_message(), Some(REFERENCE_CALLER_GONE_MESSAGE));
        assert!(writer.writes().is_empty());
    }

    #[tokio::test]
    async fn a_stop_that_cannot_be_confirmed_is_outcome_unknown() {
        let queue = ReferenceInputQueue::new();
        let writer = RecordingWriter::new();
        writer.fail_next_write();
        let target = target("sess-a");
        let payload = stop_payload(ReferenceStopCapability::ProviderInterrupt);

        let outcome = stop_ok(&queue, writer.as_ref(), &target, "r1", &payload).await;

        assert!(outcome.is_outcome_unknown());
        assert_eq!(outcome.wire_code().as_deref(), Some(REFERENCE_OUTCOME_UNKNOWN_CODE));
    }

    // ---- one step per target, and only per target ---------------------------------------------

    #[tokio::test]
    async fn a_stop_tapped_right_after_send_lands_after_the_messages_enter() {
        let queue = ReferenceInputQueue::new();
        let (writer, started, release) = RecordingWriter::gated();
        let clock = ManualClock::at(0);
        let target = target("sess-a");
        let payload = submit_payload("one");
        let stop_payload = stop_payload(ReferenceStopCapability::ProviderInterrupt);
        let authorize = allow();
        let alive = alive();

        let submit_request = ReferenceSubmitRequest {
            target: &target,
            request_id: "r1",
            payload: &payload,
            bracketed_paste: true,
            blocked_prompt: None,
            arrived_at_ms: 0,
            last_typed_at_ms: None,
            authorize: &authorize,
        };
        let submit = queue.submit(&submit_request, writer.as_ref(), &clock);
        let stop_request = ReferenceStopRequest {
            target: &target,
            request_id: "r2",
            payload: &stop_payload,
            authorize: &authorize,
            alive: &alive,
        };
        let stop = queue.stop(&stop_request, writer.as_ref());

        let tapped = async {
            started.await.expect("the message's paste write started");
            tokio::pin!(stop);
            // Biased: the Stop is polled first, so this either observes it parked on the target's
            // step or catches it typing into the pane ahead of the message's Enter.
            tokio::select! {
                biased;
                outcome = &mut stop => panic!("the stop ran while the message was in flight: {outcome:?}"),
                _ = tokio::task::yield_now() => {}
            }
            assert!(
                writer.writes().is_empty(),
                "the stop typed between the message and its Enter: {:?}",
                writer.typed()
            );
            release.send(()).expect("the message's paste is released");
            stop.await
        };

        let (submitted, stopped) = tokio::join!(submit, tapped);

        assert!(submitted.is_typed());
        assert!(stopped.is_typed());
        assert_eq!(
            writer.writes(),
            vec![
                format!("{REFERENCE_PASTE_START}one{REFERENCE_PASTE_END}").into_bytes(),
                REFERENCE_SUBMIT_ENTER.to_vec(),
                REFERENCE_ESCAPE_KEY.to_vec(),
            ]
        );
    }

    #[tokio::test]
    async fn input_for_another_target_never_waits_on_this_targets_step() {
        let queue = ReferenceInputQueue::new();
        let (writer, started, release) = RecordingWriter::gated();
        let clock = ManualClock::at(0);
        let held = target("sess-a");
        let other = target("sess-b");
        let held_payload = submit_payload("one");
        let other_payload = submit_payload("two");
        let authorize = allow();

        let held_request = ReferenceSubmitRequest {
            target: &held,
            request_id: "r1",
            payload: &held_payload,
            bracketed_paste: true,
            blocked_prompt: None,
            arrived_at_ms: 0,
            last_typed_at_ms: None,
            authorize: &authorize,
        };
        let held_submit = queue.submit(&held_request, writer.as_ref(), &clock);
        let other_submit = async {
            started.await.expect("the held target's paste write started");
            let outcome = tokio::time::timeout(
                Duration::from_secs(2),
                submit_ok(&queue, writer.as_ref(), &clock, &other, "r2", &other_payload),
            )
            .await
            .expect("input for another target must not wait on this target's step");
            release.send(()).expect("the held target's paste is released");
            outcome
        };

        let (first, second) = tokio::join!(held_submit, other_submit);

        assert!(first.is_typed());
        assert!(second.is_typed());
        assert_eq!(writer.writes().len(), 4, "two messages, each with its own Enter");
    }

    // ---- request ids: dedupe, conflict, and no replay -------------------------------------------

    #[tokio::test]
    async fn the_same_request_id_with_the_same_payload_returns_its_recorded_state_without_retyping() {
        let queue = ReferenceInputQueue::new();
        let writer = RecordingWriter::new();
        let clock = ManualClock::at(0);
        let target = target("sess-a");
        let payload = submit_payload("hello");

        let first = submit_ok(&queue, writer.as_ref(), &clock, &target, "r1", &payload).await;
        let second = submit_ok(&queue, writer.as_ref(), &clock, &target, "r1", &payload).await;

        assert_eq!(first, second);
        assert_eq!(writer.writes().len(), 2, "a duplicate must not type the message twice");
    }

    #[tokio::test]
    async fn the_same_request_id_with_a_different_payload_is_a_conflict() {
        let queue = ReferenceInputQueue::new();
        let writer = RecordingWriter::new();
        let clock = ManualClock::at(0);
        let target = target("sess-a");

        assert!(submit_ok(&queue, writer.as_ref(), &clock, &target, "r1", &submit_payload("one"))
            .await
            .is_typed());
        let conflict =
            submit_ok(&queue, writer.as_ref(), &clock, &target, "r1", &submit_payload("two")).await;

        assert_eq!(conflict.wire_code().as_deref(), Some("REQUEST_CONFLICT"));
        assert_eq!(conflict.wire_message(), Some(REFERENCE_REQUEST_CONFLICT_MESSAGE));
        assert_eq!(writer.writes().len(), 2);
    }

    #[tokio::test]
    async fn an_unknown_outcome_is_held_and_a_duplicate_returns_it_rather_than_replaying() {
        let queue = ReferenceInputQueue::new();
        let writer = RecordingWriter::new();
        writer.fail_next_write();
        let clock = ManualClock::at(0);
        let target = target("sess-a");
        let payload = submit_payload("hello");

        let first = submit_ok(&queue, writer.as_ref(), &clock, &target, "r1", &payload).await;
        assert!(first.is_outcome_unknown());
        // The same request, sent again while the first is unresolved, is answered with the held
        // state: the pane is never asked to take the message a second time.
        let duplicate = submit_ok(&queue, writer.as_ref(), &clock, &target, "r1", &payload).await;

        assert_eq!(duplicate, first);
        assert!(writer.writes().is_empty());
    }

    #[tokio::test]
    async fn a_request_that_typed_nothing_leaves_its_id_free() {
        let queue = ReferenceInputQueue::new();
        let writer = RecordingWriter::new();
        let clock = ManualClock::at(0);
        let target = target("sess-a");
        let authorize = allow();
        let payload = submit_payload("hello");

        let blocked = ReferenceSubmitRequest {
            target: &target,
            request_id: "r1",
            payload: &payload,
            bracketed_paste: true,
            blocked_prompt: Some("Approve?"),
            arrived_at_ms: 0,
            last_typed_at_ms: None,
            authorize: &authorize,
        };
        assert!(!queue.submit(&blocked, writer.as_ref(), &clock).await.is_typed());

        // Nothing was typed, so the same id is usable for the retry the user actually makes.
        assert!(submit_ok(&queue, writer.as_ref(), &clock, &target, "r1", &payload).await.is_typed());
        assert_eq!(writer.writes().len(), 2);
    }

    #[tokio::test]
    async fn one_request_id_on_two_targets_is_two_requests() {
        let queue = ReferenceInputQueue::new();
        let writer = RecordingWriter::new();
        let clock = ManualClock::at(0);
        let payload = submit_payload("hello");

        let first = submit_ok(&queue, writer.as_ref(), &clock, &target("sess-a"), "r1", &payload).await;
        let second = submit_ok(&queue, writer.as_ref(), &clock, &target("sess-b"), "r1", &payload).await;

        assert!(first.is_typed());
        assert!(second.is_typed(), "the record is keyed on the owning target, not the id alone");
        assert_eq!(writer.writes().len(), 4);
    }

    #[tokio::test]
    async fn a_stop_and_a_submit_do_not_share_a_request_id_record() {
        let queue = ReferenceInputQueue::new();
        let writer = RecordingWriter::new();
        let clock = ManualClock::at(0);
        let target = target("sess-a");

        assert!(submit_ok(&queue, writer.as_ref(), &clock, &target, "r1", &submit_payload("one"))
            .await
            .is_typed());
        let stopped = stop_ok(
            &queue,
            writer.as_ref(),
            &target,
            "r1",
            &stop_payload(ReferenceStopCapability::ProviderInterrupt),
        )
        .await;

        assert!(stopped.is_typed(), "a stop is a different mutation than a submit");
        assert_eq!(writer.writes().len(), 3);
    }

    // ---- the frozen wire strings -----------------------------------------------------------------

    #[test]
    fn refusals_are_named_by_the_frozen_scope_error_codes() {
        assert_eq!(scope_error_code_wire(ScopeErrorCode::RequestConflict), "REQUEST_CONFLICT");
        assert_eq!(scope_error_code_wire(ScopeErrorCode::Unsupported), "UNSUPPORTED");
        assert_eq!(scope_error_code_wire(ScopeErrorCode::PayloadTooLarge), "PAYLOAD_TOO_LARGE");
        assert_eq!(scope_error_code_wire(ScopeErrorCode::Timeout), "TIMEOUT");
        assert_eq!(scope_error_code_wire(ScopeErrorCode::ControlConflict), "CONTROL_CONFLICT");
        assert_eq!(scope_error_code_wire(ScopeErrorCode::Unauthorized), "UNAUTHORIZED");
    }

    #[test]
    fn the_outcome_wire_shape_names_its_variant_and_its_code() {
        let unknown = ReferenceInputOutcome::outcome_unknown("lost");
        assert_eq!(
            serde_json::to_value(&unknown).expect("the outcome serializes"),
            serde_json::json!({
                "kind": "outcomeUnknown",
                "code": REFERENCE_OUTCOME_UNKNOWN_CODE,
                "message": "lost",
            })
        );

        let not_typed =
            ReferenceInputOutcome::not_typed(ScopeErrorCode::RequestConflict, false, "conflict");
        assert_eq!(
            serde_json::to_value(&not_typed).expect("the outcome serializes"),
            serde_json::json!({
                "kind": "notTyped",
                "error": {
                    "code": "REQUEST_CONFLICT",
                    "message": "conflict",
                    "retryable": false,
                    "details": null,
                },
            })
        );

        let accepted = ReferenceInputOutcome::accepted("r1", &target("sess-a"));
        assert_eq!(
            serde_json::to_value(&accepted).expect("the outcome serializes"),
            serde_json::json!({
                "kind": "accepted",
                "receipt": {
                    "requestId": "r1",
                    "target": {
                        "hostId": "host-a",
                        "ownerId": "owner-a",
                        "epoch": "11",
                        "backendSessionId": "sess-a",
                    },
                    "stage": "accepted",
                },
            })
        );
    }

    #[test]
    fn a_fingerprint_covers_every_field_the_mutation_acts_on() {
        let base = submit_payload("one");
        let other_text = submit_payload("two");
        let mut other_origin = submit_payload("one");
        other_origin.origin = ReferenceSubmitOrigin::Terminal;
        let mut with_attachment = submit_payload("one");
        with_attachment.attachment_ids = vec!["a1".into()];

        assert_ne!(reference_submit_fingerprint(&base), reference_submit_fingerprint(&other_text));
        assert_ne!(reference_submit_fingerprint(&base), reference_submit_fingerprint(&other_origin));
        assert_ne!(reference_submit_fingerprint(&base), reference_submit_fingerprint(&with_attachment));
        assert_eq!(
            reference_submit_fingerprint(&base),
            reference_submit_fingerprint(&submit_payload("one"))
        );

        // Length-prefixed parts: a shifted boundary cannot collide two different payloads.
        let mut split = submit_payload("one");
        split.attachment_ids = vec!["two".into()];
        assert_ne!(reference_submit_fingerprint(&other_text), reference_submit_fingerprint(&split));

        assert_ne!(
            reference_stop_fingerprint(&stop_payload(ReferenceStopCapability::ProviderInterrupt)),
            reference_stop_fingerprint(&stop_payload(ReferenceStopCapability::ShellSignal))
        );
    }
}
