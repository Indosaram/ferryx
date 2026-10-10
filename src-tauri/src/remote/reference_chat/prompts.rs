//! Prompt detection and answering over the original pane (plan task 9).
//!
//! Ported from `devswha/herdr-web-ui` @ `54e5a1f67090cb09552d182e7e30dd0ecc314918` (MIT, see
//! `docs/chat/HERDR_LICENSE`). Upstream anchors read at the pinned revision:
//!
//! | Upstream | What this module ports |
//! |---|---|
//! | `server/prompt.ts:1931-1953` (`parsePrompt`) | the five-family chain: one arm per registry id, and the first candidate whose own tail still ends the screen |
//! | `server/prompt.ts:115228` (`answerKeys`, `parsedByPublicPrompt`) | the private card an answer is planned from, and the refusal for an id no detector produced |
//! | `server/prompt.ts:153286` (the answer route) | the fresh read, the card-id check, the folded-form opening and the single-use asking |
//! | `server/prompt.ts:1443` (`codexModelRowKey`) | a Codex model list's own key, read off the screen the answer was planned against |
//! | `server/prompt.ts:2677`, `:1474` (`openQueue`) | `alt+up` opens OmO's folded form, and the form the screen then shows is read and verified again before any answer text |
//! | `server/herdr/client.ts` (`pane.send_keys`) | the key names the plans carry |
//!
//! Frozen contract: `docs/chat/herdr-port-contract.md` §5 (answer: single use, fresh screen)
//! and §6 ([detect_reference_prompt], [reference_answer_keys]).
//!
//! ## What this module is, and is not
//!
//! * It detects the reference's five prompt families on a screen and answers one. It reads no
//!   pane itself: the screen comes from the screen lane's `snapshot_reference_screen`, handed
//!   in through [ReferenceAnswerScreenReader].
//! * It writes nothing but bytes, and only through [ReferenceInputWriter]. There is no signal,
//!   no resize, no process and no provider call anywhere in this file.
//! * It performs no authorization policy of its own: whether a caller may mutate is the route
//!   layer's decision, handed in as the authorize callback, asked before the read and again
//!   before the first key.
//! * It never guesses an outcome. A key that was dispatched and could not be confirmed is
//!   [ReferenceInputOutcome::OutcomeUnknown], and is never replayed automatically.
//!
//! ## The ordering rule, and the step this lane shares
//!
//! Submit, Stop and prompt answers take **one step per target**, so a Stop tapped right after
//! Send cannot land between a message's text and its Enter, and an answer cannot land between
//! another answer's moves and its Enter. [ReferenceInputQueue] owns that step for submit and
//! Stop and exposes it as `run_under_step`; [ReferenceQueueStep] is the adapter that hands the
//! answer route that same one queue instance, so all three share the step.
//!
//! A route that answers prompts must **not** invent a mutex of its own: a second lock would race
//! the submit/Stop transaction it has to be ordered against, which is the whole point of this
//! section. The route layer constructs one queue and one adapter over it and runs submit, Stop
//! and answers through that one queue:
//!
//! ```ignore
//! let queue = ReferenceInputQueue::new();
//! let step = ReferenceQueueStep::new(&queue);
//! // submit/Stop: queue.submit(..), queue.stop(..)
//! // answer:      answers.answer(&request, &step, &screen, &writer, &clock)
//! ```
//!
//! ## What `NotTyped` means on this route
//!
//! [ReferenceInputOutcome::NotTyped] is the answer route's "none of the **answer's** keys went
//! out", which is the same promise submit and Stop make. One case sends a key before the
//! answer's own: opening a folded OmO form with `alt+up`. That refusal carries
//! [REFERENCE_PROMPT_FOLDED_FORM_MESSAGE], which says exactly which key was sent, and is
//! `retryable` so the caller may read the form and answer it.
//!
//! ## The one part of this port the pinned JS does not pin
//!
//! Upstream names keys and hands the names to herdr's own `pane.send_keys` RPC, so the
//! name-to-byte mapping lives in herdr's binary, not in the pinned JavaScript. Ferryx's writer
//! takes bytes, so [reference_key_bytes] carries the conventional terminal encodings for the
//! names the five families plan, and refuses a name it does not carry rather than guessing.
//! `alt+up`/`alt+down` are the ESC-prefixed forms and `ctrl+k`/`ctrl+u` the control
//! bytes: an integrator verifying against a live herdr should confirm those four.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Mutex, OnceLock};

use futures_util::future::BoxFuture;
use regex::Regex;

use crate::scoped_contracts::{DeliveryReceipt, DeliveryStage, ScopeError, ScopeErrorCode};

use super::input::{
    ReferenceInputClock, ReferenceInputOutcome, ReferenceInputQueue, ReferenceInputWriter,
    REFERENCE_CALLER_GONE_MESSAGE,
};
use super::prompt_claude::{parse_claude_prompt, plan_claude_answer, ClaudePrompt};
use super::prompt_codex::{
    codex_model_row_key, parse_reference_codex_prompt, plan_reference_codex_answer,
    ReferenceCodexAnswerStep, ReferenceCodexPrompt,
};
use super::prompt_omo::{
    omo_card_for_screen, omo_form_is_trusted, omo_form_on_screen, omo_reads_forms, open_omo_asks,
    pending_omo_ask, reference_omo_answer_keys, OmoCard, OmoResponder,
    REFERENCE_OMO_OPEN_FORM_KEY,
};
use super::prompt_omp::{detect_omp_prompt_card, remember_omp_prompt, OmpPromptCard};
use super::prompt_pi::{parse_pi_prompt, pi_answer_error_code, plan_pi_answer, PI_AGENT};
use super::types::{
    reference_draft_key, ReferenceKeyStep, ReferencePrompt, ReferencePromptAnswer,
    ReferencePromptAnswerPayload, ReferencePromptDetector, ReferencePromptKind,
    ReferenceScreenSnapshot, ReferenceTargetRef, REFERENCE_OUTCOME_UNKNOWN_CODE,
};

// ---------------------------------------------------------------------------------------------
// The chain's constants
// ---------------------------------------------------------------------------------------------

/// The registry id the OmO arm runs for. Upstream's arm is `agent === "omo" || agent === ""`,
/// with a fall-through from the `pi` and `claude` arms.
pub const REFERENCE_OMO_AGENT: &str = "omo";

/// How many detected cards this module remembers for their answers.
///
/// Bounded and process-local, like the OmO and OMP lanes' own registries: a card that fell out
/// of it cannot authorize a guessed answer, because [reference_answer_keys] refuses an id it
/// does not hold.
pub const REFERENCE_PROMPT_CARD_LIMIT: usize = 256;

/// How many answered cards the single-use ledger remembers.
pub const REFERENCE_ANSWER_LEDGER_LIMIT: usize = 256;

/// How long an answer waits for a folded form to open, or for the screen to show the card's own
/// menu (upstream `SETTLE_MS`, `server/prompt.ts:149087`).
pub const REFERENCE_PROMPT_SETTLE_MS: u64 = 1_500;

/// How long between two reads while waiting for a folded form to open (upstream's inner poll).
pub const REFERENCE_PROMPT_OPEN_POLL_MS: u64 = 50;

/// The gap between two of an answer's keys (upstream's inter-step wait).
pub const REFERENCE_PROMPT_STEP_GAP_MS: u64 = 30;

/// The answer named no card this module detected (upstream's `InvalidAnswer` wording,
/// `server/prompt.ts`: "The prompt was not produced by parseInteractivePrompt.").
pub const REFERENCE_PROMPT_UNKNOWN_CARD_MESSAGE: &str =
    "The prompt was not produced by parseInteractivePrompt.";

/// The screen is not the screen the card was rendered from.
pub const REFERENCE_PROMPT_STALE_SCREEN_MESSAGE: &str =
    "the screen changed since the card was read; nothing was sent";

/// The same card was already answered.
pub const REFERENCE_PROMPT_REPLAYED_ANSWER_MESSAGE: &str =
    "this answer was already sent; nothing was sent again";

/// The pane's history could not be reconstructed, so no revision can be trusted.
pub const REFERENCE_PROMPT_GAP_MESSAGE: &str =
    "the pane's screen could not be reconstructed in full; nothing was sent";

/// The pane's screen could not be read at all.
pub const REFERENCE_PROMPT_UNREADABLE_MESSAGE: &str = "the pane's screen could not be read";

/// The answer does not fit the card the screen shows.
pub const REFERENCE_PROMPT_INVALID_ANSWER_MESSAGE: &str =
    "the answer does not fit the prompt the screen shows; nothing was sent";

/// The answer planned no key at all.
pub const REFERENCE_PROMPT_EMPTY_PLAN_MESSAGE: &str =
    "the answer planned no key; nothing was sent";

/// A planned key this reader cannot send.
pub const REFERENCE_PROMPT_UNKNOWN_KEY_MESSAGE: &str =
    "the answer names a key this reader cannot send";

/// A folded OmO form that did not open. The open key itself did go out.
pub const REFERENCE_PROMPT_FOLDED_FORM_MESSAGE: &str =
    "the folded form did not open; only its open key was sent, and no answer key";

/// Upstream's own wording for an answer that is not exactly one shape (`answerKeys`).
pub const REFERENCE_PROMPT_ONE_ANSWER_MESSAGE: &str = "Exactly one answer is required.";

/// How many lines a footer may wrap onto (the codex lane's `wrapped` span).
const CODEX_FOOTER_SPAN: usize = 3;

// ---------------------------------------------------------------------------------------------
// The five families
// ---------------------------------------------------------------------------------------------

/// One of the reference's five prompt families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReferencePromptFamily {
    Claude,
    Codex,
    Omo,
    Omp,
    Pi,
}

impl ReferencePromptFamily {
    /// Every family, in the order the reference's `parsePrompt` reaches the arms.
    pub const ALL: [Self; 5] = [Self::Codex, Self::Omp, Self::Claude, Self::Pi, Self::Omo];

    /// The registry id this family's arm runs for.
    pub fn agent(self) -> &'static str {
        match self {
            Self::Claude => super::prompt_claude::REFERENCE_CLAUDE_AGENT,
            Self::Codex => super::prompt_codex::REFERENCE_CODEX_AGENT,
            Self::Omo => REFERENCE_OMO_AGENT,
            Self::Omp => super::prompt_omp::OMP_AGENT_ID,
            Self::Pi => PI_AGENT,
        }
    }

    /// The frozen detector this family ships, in task 1's [ReferencePromptDetector] shape.
    ///
    /// These are the families' own entry points; the dispatcher reaches them through
    /// [detect_reference_prompt], which holds the private card each answer is planned from.
    pub fn detector(self) -> ReferencePromptDetector {
        match self {
            Self::Claude => super::prompt_claude::detect_reference_claude_prompt,
            Self::Codex => super::prompt_codex::detect_reference_codex_prompt,
            Self::Omo => super::prompt_omo::detect_reference_omo_prompt,
            Self::Omp => super::prompt_omp::detect_omp_prompt,
            Self::Pi => super::prompt_pi::parse_pi_prompt,
        }
    }

    /// Does this family read a card on a pane this agent names?
    ///
    /// The OmO arm runs for `omo`, for a pane with no agent, and — after the family's own
    /// readers — for `pi` and `claude`, which herdr names an OmO pane while it waits.
    pub fn reads_agent(self, agent: &str) -> bool {
        match self {
            Self::Omo => omo_reads_forms(agent),
            other => agent == other.agent(),
        }
    }

    /// The family whose arm of the chain runs for this agent, when one does.
    pub fn of_agent(agent: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|family| {
            matches!(family, Self::Claude | Self::Codex | Self::Omp | Self::Pi)
                && family.reads_agent(agent)
        })
        .or_else(|| Self::Omo.reads_agent(agent).then_some(Self::Omo))
    }
}

/// Compile-time proof that all five families ship the frozen detector shape (§6).
const _: [ReferencePromptDetector; 5] = [
    super::prompt_claude::detect_reference_claude_prompt,
    super::prompt_codex::detect_reference_codex_prompt,
    super::prompt_omo::detect_reference_omo_prompt,
    super::prompt_omp::detect_omp_prompt,
    super::prompt_pi::parse_pi_prompt,
];

// ---------------------------------------------------------------------------------------------
// The private card each answer is planned from
// ---------------------------------------------------------------------------------------------

/// A detected card's private state: what an answer needs and the public card does not carry.
///
/// Upstream keeps this beside the public prompt in `parsedByPublicPrompt` and refuses an answer
/// for a prompt it did not itself parse. The OmO, OMP and Pi lanes keep their own state in their
/// own registries; this one holds the claude and codex cards (no module keeps those), and the
/// OmO card, whose responder tells a folded widget from an open form.
#[derive(Debug, Clone, PartialEq)]
pub enum ReferencePromptCard {
    Claude(ClaudePrompt),
    Codex(ReferenceCodexPrompt),
    Omo(OmoCard),
    Omp(OmpPromptCard),
    /// pi keeps its cursor inside the prompt id (`pi_prompt_cursor`), so the state this module
    /// holds for it is only the card it was detected from.
    Pi(ReferencePrompt),
}

impl ReferencePromptCard {
    /// The family this card came from.
    pub fn family(&self) -> ReferencePromptFamily {
        match self {
            Self::Claude(_) => ReferencePromptFamily::Claude,
            Self::Codex(_) => ReferencePromptFamily::Codex,
            Self::Omo(_) => ReferencePromptFamily::Omo,
            Self::Omp(_) => ReferencePromptFamily::Omp,
            Self::Pi(_) => ReferencePromptFamily::Pi,
        }
    }

    /// The public card a client renders and an answer names.
    pub fn prompt(&self) -> &ReferencePrompt {
        match self {
            Self::Claude(parsed) => &parsed.prompt,
            Self::Codex(parsed) => &parsed.prompt,
            Self::Omo(card) => &card.prompt,
            Self::Omp(card) => &card.prompt,
            Self::Pi(prompt) => prompt,
        }
    }

    /// Is this the folded widget OmO draws over its input box, rather than the open form?
    ///
    /// The folded card is not answerable where it stands: the route opens the form with its own
    /// key and answers what the screen then shows.
    pub fn is_folded_omo(&self) -> bool {
        matches!(self, Self::Omo(card) if card.responder == OmoResponder::Pending)
    }
}

/// What the caller read about the pane's own session, beyond its screen.
///
/// The OmO family's forms carry their own text in the session's `ask_user_question` calls, so
/// its arm is the one that needs this. A caller that read no session passes nothing and the
/// screen-only forms are still detected.
#[derive(Debug, Default, Clone, Copy)]
pub struct ReferencePromptSession<'a> {
    /// The pane's session JSONL, when the caller read it.
    pub session_jsonl: Option<&'a str>,
    /// The pane's status as the caller observed it. A pane herdr names `claude` is OmO's only
    /// while it is blocked (`omo_form_is_trusted`).
    pub agent_status: Option<&'a str>,
}

#[derive(Debug, Default)]
struct ReferencePromptRegistry {
    order: VecDeque<String>,
    cards: HashMap<String, ReferencePromptCard>,
}

fn reference_prompt_registry() -> &'static Mutex<ReferencePromptRegistry> {
    static REGISTRY: OnceLock<Mutex<ReferencePromptRegistry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(ReferencePromptRegistry::default()))
}

/// Remember a detected card so [reference_answer_keys] can plan an answer for it.
///
/// Bounded: the oldest card is dropped once [REFERENCE_PROMPT_CARD_LIMIT] are held, and a card
/// that is gone cannot authorize an answer.
pub fn remember_reference_prompt_card(card: &ReferencePromptCard) {
    let id = card.prompt().id.clone();
    let Ok(mut registry) = reference_prompt_registry().lock() else {
        return;
    };
    if !registry.cards.contains_key(&id) {
        if registry.order.len() >= REFERENCE_PROMPT_CARD_LIMIT {
            if let Some(oldest) = registry.order.pop_front() {
                registry.cards.remove(&oldest);
            }
        }
        registry.order.push_back(id.clone());
    }
    registry.cards.insert(id, card.clone());
}

/// The card this module detected for a prompt id, if it still holds it.
pub fn reference_prompt_card(prompt_id: &str) -> Option<ReferencePromptCard> {
    reference_prompt_registry().lock().ok()?.cards.get(prompt_id).cloned()
}

// ---------------------------------------------------------------------------------------------
// Detection: the five-family chain
// ---------------------------------------------------------------------------------------------

/// The card a screen makes for this agent, from the session the caller read.
///
/// The reference's own chain (`parsePrompt`): one arm per registry id, and the first candidate
/// whose own tail still ends the screen. A pane herdr names `pi` reads pi's dialogs first and
/// OmO's forms after them; one it names `claude` reads claude's cards first and OmO's forms
/// after them. Any other id reaches no arm at all — a detector must not claim a provider the
/// reference does not name.
pub fn detect_reference_prompt_card_in_session(
    agent: &str,
    screen: &str,
    session: &ReferencePromptSession<'_>,
) -> Option<ReferencePromptCard> {
    let card = detect_card(agent, screen, session)?;
    remember_reference_prompt_card(&card);
    Some(card)
}

/// The public card a screen makes for this agent, remembering its private state.
pub fn detect_reference_prompt_in_session(
    agent: &str,
    screen: &str,
    session: &ReferencePromptSession<'_>,
) -> Option<ReferencePrompt> {
    Some(detect_reference_prompt_card_in_session(agent, screen, session)?.prompt().clone())
}

/// The frozen, screen-only entry point of contract §6.
///
/// Same chain, with no session read: OmO's screen-only forms are detected, and its folded
/// widget (which is matched against the session's calls) is not. A caller that has read the
/// session uses [detect_reference_prompt_in_session].
pub fn detect_reference_prompt(agent: &str, screen: &str) -> Option<ReferencePrompt> {
    detect_reference_prompt_in_session(agent, screen, &ReferencePromptSession::default())
}

fn detect_card(
    agent: &str,
    screen: &str,
    session: &ReferencePromptSession<'_>,
) -> Option<ReferencePromptCard> {
    match agent {
        _ if agent == super::prompt_codex::REFERENCE_CODEX_AGENT => {
            parse_reference_codex_prompt(screen).map(ReferencePromptCard::Codex)
        }
        _ if agent == super::prompt_omp::OMP_AGENT_ID => {
            detect_omp_prompt_card(agent, screen).map(|card| {
                // The OMP lane's planner reads its own registry, which `detect_omp_prompt_card`
                // alone does not populate: the chain remembers the card it just detected.
                remember_omp_prompt(&card);
                ReferencePromptCard::Omp(card)
            })
        }
        _ if agent == super::prompt_claude::REFERENCE_CLAUDE_AGENT => parse_claude_prompt(screen)
            .map(ReferencePromptCard::Claude)
            .or_else(|| omo_card(agent, screen, session).map(ReferencePromptCard::Omo)),
        _ if agent == PI_AGENT => parse_pi_prompt(agent, screen)
            .map(ReferencePromptCard::Pi)
            .or_else(|| omo_card(agent, screen, session).map(ReferencePromptCard::Omo)),
        _ if agent == REFERENCE_OMO_AGENT || agent.is_empty() => {
            omo_card(agent, screen, session).map(ReferencePromptCard::Omo)
        }
        _ => None,
    }
}

/// The OmO arm: a form on the screen, read with the session's own calls.
fn omo_card(agent: &str, screen: &str, session: &ReferencePromptSession<'_>) -> Option<OmoCard> {
    if !omo_reads_forms(agent) {
        return None;
    }
    // Upstream reads the pane's session only when the screen carries the form marker
    // (`readKnownPrompt`: `OMO_FORM_RE.test(screen)`), so a pane whose screen shows no form is
    // never answered from a call that has since been settled.
    let (ask, open) = match session.session_jsonl.filter(|_| omo_form_on_screen(screen)) {
        Some(jsonl) => (pending_omo_ask(jsonl), open_omo_asks(jsonl)),
        None => (None, Vec::new()),
    };
    omo_card_for_screen(
        agent,
        screen,
        ask.as_ref(),
        omo_form_is_trusted(agent, session.agent_status),
        &open,
    )
}


// ---------------------------------------------------------------------------------------------
// Answer planning
// ---------------------------------------------------------------------------------------------

/// The keys that answer a prompt on the original pane (frozen contract §6).
///
/// Plans from the card [detect_reference_prompt] remembered for this id, exactly as upstream
/// plans from `parsedByPublicPrompt`. An id this module never detected is refused with
/// [ScopeErrorCode::InvalidRequest] rather than answered by a guess.
///
/// A Codex model list's row names its own key off the screen, and this entry point has no screen
/// to read, so such a card is refused with [ScopeErrorCode::Unsupported]: the answer route
/// resolves it on the screen it just read ([ReferencePromptAnswers::answer]).
pub fn reference_answer_keys(
    prompt: &ReferencePrompt,
    answer: &ReferencePromptAnswer,
) -> Result<Vec<ReferenceKeyStep>, ScopeErrorCode> {
    let card = reference_prompt_card(&prompt.id).ok_or(ScopeErrorCode::InvalidRequest)?;
    plan_answer_keys(&card, prompt, answer, None)
}

/// The keys for one card, planned from the card itself.
///
/// `screen` is the screen this answer was planned against, for the plans that leave a key to it
/// (a Codex model list's row). The pure entry point passes `None` and refuses those plans.
fn plan_answer_keys(
    card: &ReferencePromptCard,
    prompt: &ReferencePrompt,
    answer: &ReferencePromptAnswer,
    screen: Option<&str>,
) -> Result<Vec<ReferenceKeyStep>, ScopeErrorCode> {
    if card.prompt() != prompt {
        // The card is not the card asked about: the caller's prompt has changed under it.
        return Err(ScopeErrorCode::RequestConflict);
    }
    match card {
        ReferencePromptCard::Claude(parsed) => {
            plan_claude_answer(parsed, answer).map_err(|_| ScopeErrorCode::InvalidRequest)
        }
        ReferencePromptCard::Codex(parsed) => {
            let steps = plan_reference_codex_answer(parsed, answer)
                .map_err(|_| ScopeErrorCode::InvalidRequest)?;
            let mut planned = Vec::with_capacity(steps.len());
            for step in &steps {
                match step {
                    ReferenceCodexAnswerStep::Keys(keys) => {
                        planned.push(ReferenceKeyStep::keys(keys.clone()));
                    }
                    ReferenceCodexAnswerStep::Text(text) => {
                        planned.push(ReferenceKeyStep::typed(text.clone()));
                    }
                    // The row's own key is the screen's to name: read it off the screen this
                    // answer was planned against — never a guessed Enter, never a dropped step.
                    ReferenceCodexAnswerStep::Pick => {
                        let footer = screen.and_then(codex_row_key_on_screen);
                        planned.push(footer.ok_or(ScopeErrorCode::Unsupported)?);
                    }
                }
            }
            Ok(planned)
        }
        ReferencePromptCard::Omo(_) => reference_omo_answer_keys(prompt, answer),
        ReferencePromptCard::Omp(_) => super::prompt_omp::reference_answer_keys(prompt, answer),
        ReferencePromptCard::Pi(_) => {
            plan_pi_answer(prompt, answer).map_err(|_| pi_answer_error_code())
        }
    }
}

/// The key a Codex model list's footer names, read off the screen an answer was planned against.
///
/// The pinned reader finds the last line whose own footer — that line and the two after it,
/// joined, blanks and rules dropped — names a key (`parse_codex_model`'s `hint_index`), so this
/// walks the screen the same way. Those helpers are reproduced here because the codex lane keeps
/// them private and exposes only [codex_model_row_key].
fn codex_row_key_on_screen(screen: &str) -> Option<ReferenceKeyStep> {
    let lines = screen_lines(screen);
    (0..lines.len())
        .rev()
        .find_map(|index| codex_model_row_key(&codex_footer(&lines, index)))
}

/// The bytes one planned key names on the original pane.
///
/// Upstream names keys and hands the names to herdr's own `pane.send_keys` RPC, which owns the
/// mapping; Ferryx's writer takes bytes, so the mapping lives here beside the plans that produce
/// the names. A name this table does not carry is `None`: a plan this reader cannot send is
/// refused, never approximated.
pub fn reference_key_bytes(name: &str) -> Option<&'static [u8]> {
    let bytes: &'static [u8] = match name {
        "up" => b"\x1b[A",
        "down" => b"\x1b[B",
        "right" => b"\x1b[C",
        "left" => b"\x1b[D",
        "enter" => b"\r",
        "esc" => b"\x1b",
        "space" => b" ",
        "tab" => b"\t",
        "shift+tab" => b"\x1b[Z",
        "backspace" => b"\x7f",
        "alt+up" => b"\x1b\x1b[A",
        "alt+down" => b"\x1b\x1b[B",
        "ctrl+k" => b"\x0b",
        "ctrl+u" => b"\x15",
        _ => return None,
    };
    Some(bytes)
}

/// The bytes one planned step writes: its named keys when it has any, else its text.
///
/// Upstream's executor takes `keys` first and `text` only when there are none; a step that
/// carries neither writes nothing.
pub fn reference_key_step_bytes(step: &ReferenceKeyStep) -> Result<Vec<u8>, ScopeErrorCode> {
    if !step.keys.is_empty() {
        let mut bytes = Vec::new();
        for key in &step.keys {
            bytes.extend_from_slice(reference_key_bytes(key).ok_or(ScopeErrorCode::Unsupported)?);
        }
        return Ok(bytes);
    }
    Ok(step.text.as_deref().unwrap_or_default().as_bytes().to_vec())
}

// ---------------------------------------------------------------------------------------------
// The serialized answer
// ---------------------------------------------------------------------------------------------

/// One answer's work, run while the target's input step is held.
pub type ReferenceAnswerFuture<'a> = BoxFuture<'a, ReferenceInputOutcome>;

/// The per-pane input step a prompt answer shares with submit and Stop.
///
/// [ReferenceInputQueue] takes that step for submit and Stop, so an answer that took a step of
/// its own would race the transaction it has to be ordered against. [ReferenceQueueStep] is the
/// real adapter over that one queue; the answer path takes this trait, so the route layer wires
/// the queue's adapter and a test can drive its own step. Never a second lock.
pub trait ReferencePaneStep: Send + Sync {
    /// Run `op` while holding this target's input step.
    fn run<'a>(
        &'a self,
        target: &'a ReferenceTargetRef,
        op: Box<dyn FnOnce() -> ReferenceAnswerFuture<'a> + Send + 'a>,
    ) -> ReferenceAnswerFuture<'a>;
}

/// The real [ReferencePaneStep]: the route layer's own [ReferenceInputQueue].
///
/// Submit, Stop and prompt answers then take the SAME step for a target, which is the ordering
/// the contract requires. The route layer constructs one queue and one adapter over it:
///
/// ```ignore
/// let queue = ReferenceInputQueue::new();
/// let step = ReferenceQueueStep::new(&queue);
/// // submit/Stop: queue.submit(..), queue.stop(..)
/// // answer:      answers.answer(&request, &step, &screen, &writer, &clock)
/// ```
pub struct ReferenceQueueStep<'a> {
    queue: &'a ReferenceInputQueue,
}

impl<'a> ReferenceQueueStep<'a> {
    /// The adapter over the one queue the route layer shares with submit and Stop.
    pub fn new(queue: &'a ReferenceInputQueue) -> Self {
        Self { queue }
    }
}

impl ReferencePaneStep for ReferenceQueueStep<'_> {
    fn run<'a>(
        &'a self,
        target: &'a ReferenceTargetRef,
        op: Box<dyn FnOnce() -> ReferenceAnswerFuture<'a> + Send + 'a>,
    ) -> ReferenceAnswerFuture<'a> {
        Box::pin(async move { self.queue.run_under_step(target, op()).await })
    }
}

/// A fresh, read-only screen read for one target, taken inside the pane step.
///
/// The route implements this over the screen lane's `snapshot_reference_screen` (the
/// authenticated backend's mirror and the segmented history it already holds). This module never
/// reads a pane, never resizes one and never writes input through it.
pub trait ReferenceAnswerScreenReader: Send + Sync {
    fn read_screen<'a>(
        &'a self,
        target: &'a ReferenceTargetRef,
    ) -> BoxFuture<'a, Result<ReferenceScreenSnapshot, String>>;
}

/// One answer the route layer has authorized and wants sent to the original pane.
pub struct ReferenceAnswerRequest<'a> {
    /// The owning target: host + owner + daemon incarnation + backend session.
    pub target: &'a ReferenceTargetRef,
    /// The request id the mutation envelope carries.
    pub request_id: &'a str,
    /// The card the user answered: its id, the screen revision it was rendered from, the answer.
    pub payload: &'a ReferencePromptAnswerPayload,
    /// What the caller read about the pane's own session, for the OmO arm.
    pub session: ReferencePromptSession<'a>,
    /// Whether this caller may mutate at all. Asked before the read and again before the first
    /// key, so a revoke that lands mid-flight cannot be ignored.
    pub authorize: &'a (dyn Fn() -> Result<(), ScopeError> + Send + Sync),
    /// Whether the caller is still there. A key for a caller that has gone is not pressed.
    pub alive: &'a (dyn Fn() -> bool + Send + Sync),
}

/// The single-use ledger: a card answered once is not answered again.
#[derive(Debug, Default)]
struct ReferenceAnswerLedger {
    order: VecDeque<String>,
    answered: HashSet<String>,
}

impl ReferenceAnswerLedger {
    fn is_answered(&self, key: &str) -> bool {
        self.answered.contains(key)
    }

    fn record(&mut self, key: String) {
        if self.answered.insert(key.clone()) {
            self.order.push_back(key);
        }
        while self.order.len() > REFERENCE_ANSWER_LEDGER_LIMIT {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            self.answered.remove(&oldest);
        }
    }
}

/// The answer path: fresh screen, single-use identity, serialized keys.
#[derive(Debug, Default)]
pub struct ReferencePromptAnswers {
    ledger: Mutex<ReferenceAnswerLedger>,
}

impl ReferencePromptAnswers {
    pub fn new() -> Self {
        Self::default()
    }

    /// Answer one prompt on its original pane.
    ///
    /// Everything happens inside `step`'s one target step: the fresh read, the card's
    /// re-detection, the plan and every key. Nothing is read or sent outside it, so an answer
    /// cannot interleave with a submit, a Stop or another answer for the same pane.
    pub async fn answer<'r>(
        &'r self,
        request: &'r ReferenceAnswerRequest<'r>,
        step: &'r dyn ReferencePaneStep,
        screen: &'r dyn ReferenceAnswerScreenReader,
        writer: &'r dyn ReferenceInputWriter,
        clock: &'r dyn ReferenceInputClock,
    ) -> ReferenceInputOutcome {
        // A malformed answer is malformed whatever the pane shows: it is refused before anything
        // is read, sent or recorded, so the same id may be used again with a corrected answer.
        if request.payload.answer.variant_count() != 1 {
            return refused(
                ScopeErrorCode::InvalidRequest,
                false,
                REFERENCE_PROMPT_ONE_ANSWER_MESSAGE,
            );
        }
        if request.payload.prompt_id.trim().is_empty()
            || request.payload.screen_revision.trim().is_empty()
        {
            return refused(
                ScopeErrorCode::InvalidRequest,
                false,
                REFERENCE_PROMPT_STALE_SCREEN_MESSAGE,
            );
        }
        let target = request.target;
        let work: Box<dyn FnOnce() -> ReferenceAnswerFuture<'r> + Send + 'r> = Box::new(move || {
            Box::pin(async move { self.answer_under_step(request, screen, writer, clock).await })
        });
        step.run(target, work).await
    }

    async fn answer_under_step(
        &self,
        request: &ReferenceAnswerRequest<'_>,
        screen: &dyn ReferenceAnswerScreenReader,
        writer: &dyn ReferenceInputWriter,
        clock: &dyn ReferenceInputClock,
    ) -> ReferenceInputOutcome {
        if let Err(error) = (request.authorize)() {
            return ReferenceInputOutcome::NotTyped { error };
        }
        if !(request.alive)() {
            return refused(
                ScopeErrorCode::Unauthorized,
                false,
                REFERENCE_CALLER_GONE_MESSAGE,
            );
        }

        let Some(known) = reference_prompt_card(&request.payload.prompt_id) else {
            return refused(
                ScopeErrorCode::InvalidRequest,
                false,
                REFERENCE_PROMPT_UNKNOWN_CARD_MESSAGE,
            );
        };

        // Single use: a card already answered is refused before the pane is read again.
        let ledger_key = answer_ledger_key(request);
        let answered = self
            .ledger
            .lock()
            .map(|ledger| ledger.is_answered(&ledger_key))
            .unwrap_or(false);
        if answered {
            return refused(
                ScopeErrorCode::RequestConflict,
                false,
                REFERENCE_PROMPT_REPLAYED_ANSWER_MESSAGE,
            );
        }

        // The screen as it is now: the revision the card was rendered from is checked against it.
        let fresh = match screen.read_screen(request.target).await {
            Ok(snapshot) => snapshot,
            Err(message) => {
                return refused(
                    ScopeErrorCode::Timeout,
                    true,
                    format!("{REFERENCE_PROMPT_UNREADABLE_MESSAGE}: {message}"),
                );
            }
        };
        if !fresh.is_answerable() {
            return refused(
                ScopeErrorCode::RequestConflict,
                true,
                REFERENCE_PROMPT_GAP_MESSAGE,
            );
        }
        if fresh.revision != request.payload.screen_revision {
            return refused(
                ScopeErrorCode::RequestConflict,
                true,
                REFERENCE_PROMPT_STALE_SCREEN_MESSAGE,
            );
        }

        // The card is detected again on the screen as it is now: every family's registry is
        // refreshed, and the card the user answered must still be the card on the screen.
        let agent = known.prompt().agent.clone();
        let mut screen_text = fresh.text;
        let mut card =
            match detect_reference_prompt_card_in_session(&agent, &screen_text, &request.session) {
                Some(card) if card.prompt().id == request.payload.prompt_id => card,
                _ => {
                    return refused(
                        ScopeErrorCode::RequestConflict,
                        true,
                        REFERENCE_PROMPT_STALE_SCREEN_MESSAGE,
                    );
                }
            };

        // A folded OmO widget is not answerable where it stands. Its own key opens the form, and
        // the form the screen then shows — verified to be the same asking — is what is answered.
        if card.is_folded_omo() {
            let open_key =
                reference_key_bytes(REFERENCE_OMO_OPEN_FORM_KEY).expect("the open key is mapped");
            if let Err(message) = writer.write(session_id(request), open_key).await {
                return outcome_unknown(format!(
                    "the form's open key may have reached the pane and could not be confirmed: {message}"
                ));
            }
            match wait_for_open_form(request, screen, clock, &agent).await {
                Ok((opened, text)) => {
                    card = opened;
                    screen_text = text;
                }
                Err(outcome) => return outcome,
            }
        }

        // Reauthorize before the first key: a revoke that landed while the screen was read stops
        // here, with nothing typed.
        if let Err(error) = (request.authorize)() {
            return ReferenceInputOutcome::NotTyped { error };
        }

        let prompt = card.prompt().clone();
        let steps = match plan_answer_keys(&card, &prompt, &request.payload.answer, Some(screen_text.as_str()))
        {
            Ok(steps) => steps,
            Err(code) => {
                return refused(code, false, REFERENCE_PROMPT_INVALID_ANSWER_MESSAGE);
            }
        };

        let mut wrote = false;
        for (index, step) in steps.iter().enumerate() {
            if !(request.alive)() {
                return if wrote {
                    outcome_unknown(REFERENCE_CALLER_GONE_MESSAGE)
                } else {
                    refused(
                        ScopeErrorCode::Unauthorized,
                        false,
                        REFERENCE_CALLER_GONE_MESSAGE,
                    )
                };
            }
            let bytes = match reference_key_step_bytes(step) {
                Ok(bytes) => bytes,
                Err(code) => {
                    return if wrote {
                        outcome_unknown(REFERENCE_PROMPT_UNKNOWN_KEY_MESSAGE)
                    } else {
                        refused(code, false, REFERENCE_PROMPT_UNKNOWN_KEY_MESSAGE)
                    };
                }
            };
            if bytes.is_empty() {
                continue;
            }
            if let Err(message) = writer.write(session_id(request), &bytes).await {
                return outcome_unknown(format!(
                    "an answer key may have reached the pane and could not be confirmed: {message}"
                ));
            }
            wrote = true;
            if index + 1 < steps.len() {
                clock.sleep_ms(REFERENCE_PROMPT_STEP_GAP_MS).await;
            }
        }
        if !wrote {
            return refused(
                ScopeErrorCode::InvalidRequest,
                false,
                REFERENCE_PROMPT_EMPTY_PLAN_MESSAGE,
            );
        }

        // Committed: this card is not answered twice. The record is taken under the pane's step,
        // so a second answer for the same card waits here and is refused rather than racing this
        // one.
        if let Ok(mut ledger) = self.ledger.lock() {
            ledger.record(ledger_key);
        }
        ReferenceInputOutcome::Accepted {
            receipt: DeliveryReceipt {
                request_id: request.request_id.to_string(),
                target: request.target.target.clone(),
                stage: DeliveryStage::Accepted,
            },
        }
    }
}

/// Wait for the form OmO's folded widget opens into, and verify it is the same asking.
///
/// Upstream polls for its settle window and refuses the answer unless the form shows the call and
/// the question the widget named; a shortcut that does not open the form fails closed before any
/// answer text goes out.
async fn wait_for_open_form(
    request: &ReferenceAnswerRequest<'_>,
    screen: &dyn ReferenceAnswerScreenReader,
    clock: &dyn ReferenceInputClock,
    agent: &str,
) -> Result<(ReferencePromptCard, String), ReferenceInputOutcome> {
    let deadline = clock.now_ms().saturating_add(REFERENCE_PROMPT_SETTLE_MS);
    loop {
        if !(request.alive)() {
            return Err(refused(
                ScopeErrorCode::Unauthorized,
                false,
                REFERENCE_CALLER_GONE_MESSAGE,
            ));
        }
        if clock.now_ms() >= deadline {
            return Err(refused(
                ScopeErrorCode::RequestConflict,
                true,
                REFERENCE_PROMPT_FOLDED_FORM_MESSAGE,
            ));
        }
        if let Ok(snapshot) = screen.read_screen(request.target).await {
            if snapshot.is_answerable() {
                if let Some(card) =
                    detect_reference_prompt_card_in_session(agent, &snapshot.text, &request.session)
                {
                    if card.prompt().id == request.payload.prompt_id && !card.is_folded_omo() {
                        return Ok((card, snapshot.text));
                    }
                }
            }
        }
        clock.sleep_ms(REFERENCE_PROMPT_OPEN_POLL_MS).await;
    }
}

/// The single-use key: this target's card, as this asking of it.
fn answer_ledger_key(request: &ReferenceAnswerRequest<'_>) -> String {
    format!(
        "{}\u{1f}{}\u{1f}{}",
        reference_draft_key(request.target),
        request.payload.prompt_id,
        request.payload.screen_revision
    )
}

/// The backend session an answer's keys are typed into.
fn session_id<'a>(request: &ReferenceAnswerRequest<'a>) -> &'a str {
    request.target.target.backend_session_id.as_str()
}

fn refused(
    code: ScopeErrorCode,
    retryable: bool,
    message: impl Into<String>,
) -> ReferenceInputOutcome {
    ReferenceInputOutcome::NotTyped {
        error: ScopeError {
            code,
            message: message.into(),
            retryable,
            details: serde_json::Value::Null,
        },
    }
}

fn outcome_unknown(message: impl Into<String>) -> ReferenceInputOutcome {
    ReferenceInputOutcome::OutcomeUnknown {
        code: REFERENCE_OUTCOME_UNKNOWN_CODE.to_string(),
        message: message.into(),
    }
}

// ---------------------------------------------------------------------------------------------
// The codex lane's private line helpers, for the one key it leaves to the screen
// ---------------------------------------------------------------------------------------------

/// The screen's lines, escape sequences off and a trailing CR trimmed.
fn screen_lines(screen: &str) -> Vec<String> {
    re_ansi()
        .replace_all(screen, "")
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_string())
        .collect()
}

fn re_ansi() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| {
        Regex::new(r"\x1b\[[0-?]*[ -/]*[@-~]").expect("the pinned ansi pattern is valid")
    })
}

fn re_divider() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| {
        Regex::new(r"^[\s╭╮╰╯├┤┬┴┼─━═╌▔]+$").expect("the pinned divider pattern is valid")
    })
}

/// A line without its escape sequences, its box drawing and its surrounding space.
fn clean_screen_line(raw: &str) -> String {
    let mut line = re_ansi().replace_all(raw, "").to_string();
    line = line.trim().to_string();
    if let Some(rest) = line.strip_prefix('│') {
        line = rest.trim_start().to_string();
    }
    if let Some(rest) = line.strip_suffix('│') {
        line = rest.trim_end().to_string();
    }
    line.trim().to_string()
}

fn is_screen_divider(line: &str) -> bool {
    let value = clean_screen_line(line);
    !value.is_empty() && re_divider().is_match(&value)
}

/// A line and the two after it as one, blanks and rules dropped: the codex lane's `wrapped`.
fn codex_footer(lines: &[String], index: usize) -> String {
    let end = (index + CODEX_FOOTER_SPAN).min(lines.len());
    if index >= end {
        return String::new();
    }
    lines[index..end]
        .iter()
        .map(|line| clean_screen_line(line))
        .filter(|line| !line.is_empty() && !is_screen_divider(line))
        .collect::<Vec<_>>()
        .join(" ")
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::sync::Arc;

    use tokio::sync::oneshot;
    use tokio::sync::Mutex as AsyncMutex;

    use super::super::input::ReferenceSubmitRequest;
    use super::super::types::{ReferenceSubmitOrigin, ReferenceSubmitPayload};
    use crate::scoped_contracts::{Epoch, TargetRef};

    // -----------------------------------------------------------------------------------------
    // Fixtures, each one already proven to parse by its own family's tests
    // -----------------------------------------------------------------------------------------

    const CLAUDE_QUESTION: &str = include_str!("fixtures/claude/prompt/question.screen");
    const CODEX_MENU: &str = include_str!("fixtures/codex/menu.txt");
    const CODEX_MODEL_LIST: &str = include_str!("fixtures/codex/model-list.txt");
    const OMP_QUESTION: &str = include_str!("fixtures/omp-prompt/question-single.txt");
    const PI_DIALOG: &str = include_str!("fixtures/prompt-pi/dialog-select.txt");
    const OMO_FORM: &str = include_str!("fixtures/prompt-omo/form-tabbed.screen");
    const OMO_WIDGET: &str = include_str!("fixtures/prompt-omo/widget-pending.screen");
    const OMO_FORM_OPENED: &str = include_str!("fixtures/prompt-omo/form-widget-opened.screen");
    const OMO_ASKS_TWO: &str = include_str!("fixtures/prompt-omo/asks-two-questions.jsonl");
    const OMO_ASKS_FOLDED: &str = include_str!("fixtures/prompt-omo/asks-folded.jsonl");
    const OMO_SHELL_QUOTED: &str = include_str!("fixtures/prompt-omo/form-shell-quoted.screen");

    /// The revision an answer names; the reader hands the same one back unless the test moves it.
    const REVISION: &str = "rev-1";

    fn target(backend: &str) -> ReferenceTargetRef {
        ReferenceTargetRef::without_provider_session(TargetRef {
            host_id: "host-a".into(),
            owner_id: "owner-a".into(),
            epoch: Epoch(11),
            backend_session_id: backend.into(),
        })
    }

    fn snapshot(text: &str) -> ReferenceScreenSnapshot {
        ReferenceScreenSnapshot {
            revision: REVISION.to_string(),
            text: text.to_string(),
            truncated: false,
            gap: false,
            cols: 100,
            rows: 40,
        }
    }

    fn session(jsonl: &str) -> ReferencePromptSession<'_> {
        ReferencePromptSession { session_jsonl: Some(jsonl), agent_status: None }
    }

    fn option(index: u32) -> ReferencePromptAnswer {
        ReferencePromptAnswer { option_index: Some(index), ..Default::default() }
    }

    fn keys_of(step: &ReferenceKeyStep) -> Vec<String> {
        step.keys.clone()
    }

    fn ghost(id: &str, question: &str) -> ReferencePrompt {
        ReferencePrompt {
            id: id.to_string(),
            agent: "claude".into(),
            kind: ReferencePromptKind::Question,
            title: String::new(),
            question: question.to_string(),
            body: None,
            options: vec![],
            multi_select: false,
            custom_option_index: None,
            queued: None,
            steps: vec![],
            fallback: None,
        }
    }

    // -----------------------------------------------------------------------------------------
    // The doubles: a writer that records, a clock the test drives, a screen the test hands over
    // -----------------------------------------------------------------------------------------

    #[derive(Debug, Default)]
    struct RecordingWriter {
        writes: Mutex<Vec<Vec<u8>>>,
        sessions: Mutex<Vec<String>>,
        /// Fail the write at this call index (0-based), once.
        fail_at: AtomicUsize,
        calls: AtomicUsize,
    }

    impl RecordingWriter {
        fn new() -> Self {
            Self { fail_at: AtomicUsize::new(usize::MAX), ..Self::default() }
        }

        fn failing_at(index: usize) -> Self {
            let writer = Self::new();
            writer.fail_at.store(index, Ordering::SeqCst);
            writer
        }

        fn log(&self) -> Vec<Vec<u8>> {
            self.writes.lock().unwrap().clone()
        }

        fn text(&self) -> Vec<String> {
            self.log().iter().map(|bytes| String::from_utf8_lossy(bytes).to_string()).collect()
        }

        /// The distinct sessions these keys reached, in the order they were first written to.
        ///
        /// A plan's steps are separate writes to the one pane an answer targets (`pane.send_keys`
        /// carries key names one at a time, and the route keeps its own gap between steps), so the
        /// raw per-write record would answer "how many writes" instead of what an answer's session
        /// assertion asks: *which* pane's session the keys reached. A step that went to another
        /// session still appears here.
        fn sessions(&self) -> Vec<String> {
            let mut distinct: Vec<String> = Vec::new();
            for session in self.sessions.lock().unwrap().iter() {
                if !distinct.contains(session) {
                    distinct.push(session.clone());
                }
            }
            distinct
        }
    }

    impl ReferenceInputWriter for RecordingWriter {
        fn write<'a>(
            &'a self,
            session_id: &'a str,
            data: &'a [u8],
        ) -> BoxFuture<'a, Result<(), String>> {
            Box::pin(async move {
                let call = self.calls.fetch_add(1, Ordering::SeqCst);
                self.sessions.lock().unwrap().push(session_id.to_string());
                if call == self.fail_at.load(Ordering::SeqCst) {
                    return Err("the transport dropped".to_string());
                }
                self.writes.lock().unwrap().push(data.to_vec());
                Ok(())
            })
        }
    }

    /// The clock's own state, shared with the futures it hands out so they can be `'static`.
    #[derive(Debug, Default)]
    struct ClockState {
        now: AtomicU64,
        sleeps: Mutex<Vec<u64>>,
    }

    /// A clock with no wall time: a settle window and a step gap are observed values, and every
    /// wait advances this clock and yields the runtime rather than sleeping.
    #[derive(Debug, Default, Clone)]
    struct ManualClock {
        state: Arc<ClockState>,
    }

    impl ManualClock {
        fn now(&self) -> u64 {
            self.state.now.load(Ordering::SeqCst)
        }

        fn sleeps(&self) -> Vec<u64> {
            self.state.sleeps.lock().unwrap().clone()
        }
    }

    impl ReferenceInputClock for ManualClock {
        fn now_ms(&self) -> u64 {
            self.now()
        }

        fn sleep_ms(&self, ms: u64) -> BoxFuture<'static, ()> {
            let state = self.state.clone();
            Box::pin(async move {
                // The test owns the time: the wait is recorded, the clock moves and the runtime
                // yields once, so a poll loop terminates without a real sleep anywhere.
                state.sleeps.lock().unwrap().push(ms);
                state.now.fetch_add(ms, Ordering::SeqCst);
                tokio::task::yield_now().await;
            })
        }
    }

    /// A screen reader that hands over the screens it was given, in order, and counts the reads.
    /// The last screen it holds is repeated for every read past the list.
    struct ScriptedScreen {
        screens: Mutex<VecDeque<Result<ReferenceScreenSnapshot, String>>>,
        reads: AtomicUsize,
    }

    impl ScriptedScreen {
        fn one(text: &str) -> Self {
            Self::many(vec![Ok(snapshot(text))])
        }

        fn many(screens: Vec<Result<ReferenceScreenSnapshot, String>>) -> Self {
            Self { screens: Mutex::new(screens.into_iter().collect()), reads: AtomicUsize::new(0) }
        }

        fn reads(&self) -> usize {
            self.reads.load(Ordering::SeqCst)
        }
    }

    impl ReferenceAnswerScreenReader for ScriptedScreen {
        fn read_screen<'a>(
            &'a self,
            _target: &'a ReferenceTargetRef,
        ) -> BoxFuture<'a, Result<ReferenceScreenSnapshot, String>> {
            Box::pin(async move {
                self.reads.fetch_add(1, Ordering::SeqCst);
                let mut screens = self.screens.lock().unwrap();
                if screens.len() > 1 {
                    screens.pop_front().unwrap()
                } else {
                    screens.front().cloned().unwrap_or_else(|| Err("no screen".to_string()))
                }
            })
        }
    }

    /// The pane step, over one queue per target: the same step submit and Stop take.
    #[derive(Default)]
    struct SharedStep {
        locks: Mutex<HashMap<String, Arc<AsyncMutex<()>>>>,
    }

    impl SharedStep {
        fn lock_for(&self, target: &ReferenceTargetRef) -> Arc<AsyncMutex<()>> {
            self.locks
                .lock()
                .unwrap()
                .entry(reference_draft_key(target))
                .or_insert_with(|| Arc::new(AsyncMutex::new(())))
                .clone()
        }
    }

    impl ReferencePaneStep for SharedStep {
        fn run<'a>(
            &'a self,
            target: &'a ReferenceTargetRef,
            op: Box<dyn FnOnce() -> ReferenceAnswerFuture<'a> + Send + 'a>,
        ) -> ReferenceAnswerFuture<'a> {
            let lock = self.lock_for(target);
            Box::pin(async move {
                let _step = lock.lock_owned().await;
                op().await
            })
        }
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

    fn refusal() -> impl Fn() -> Result<(), ScopeError> + Send + Sync {
        || {
            Err(ScopeError {
                code: ScopeErrorCode::Unauthorized,
                message: "this device's access was revoked".to_string(),
                retryable: false,
                details: serde_json::Value::Null,
            })
        }
    }

    fn payload(
        prompt_id: &str,
        revision: &str,
        answer: ReferencePromptAnswer,
    ) -> ReferencePromptAnswerPayload {
        ReferencePromptAnswerPayload {
            prompt_id: prompt_id.to_string(),
            screen_revision: revision.to_string(),
            answer,
        }
    }

    fn assert_not_typed(outcome: &ReferenceInputOutcome, code: ScopeErrorCode) {
        match outcome {
            ReferenceInputOutcome::NotTyped { error } => assert_eq!(error.code, code, "{error:?}"),
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    fn answer_once<'a>(
        answers: &'a ReferencePromptAnswers,
        request: &'a ReferenceAnswerRequest<'a>,
        step: &'a dyn ReferencePaneStep,
        screen: &'a dyn ReferenceAnswerScreenReader,
        writer: &'a dyn ReferenceInputWriter,
        clock: &'a dyn ReferenceInputClock,
    ) -> ReferenceAnswerFuture<'a> {
        Box::pin(answers.answer(request, step, screen, writer, clock))
    }

    // -----------------------------------------------------------------------------------------
    // Detection: the chain and the five families
    // -----------------------------------------------------------------------------------------

    #[test]
    fn every_family_is_reached_by_its_own_registry_id() {
        for (agent, screen) in [
            ("claude", CLAUDE_QUESTION),
            ("codex", CODEX_MENU),
            ("omp", OMP_QUESTION),
            ("pi", PI_DIALOG),
        ] {
            let prompt = detect_reference_prompt(agent, screen)
                .unwrap_or_else(|| panic!("{agent} is a family the reference names"));
            assert_eq!(prompt.agent, agent);
            assert!(
                reference_prompt_card(&prompt.id).is_some(),
                "{agent}'s card is remembered so an answer can be planned from it"
            );
            assert_eq!(ReferencePromptFamily::of_agent(agent), Some(family_of(agent)));
        }
        let omo = detect_reference_prompt_in_session("omo", OMO_FORM, &session(OMO_ASKS_TWO))
            .expect("the omo form is a card");
        assert_eq!(omo.agent, "omo");
        assert_eq!(ReferencePromptFamily::of_agent("omo"), Some(ReferencePromptFamily::Omo));
        assert_eq!(ReferencePromptFamily::of_agent(""), Some(ReferencePromptFamily::Omo));
    }

    #[test]
    fn every_family_ships_the_frozen_detector_shape() {
        // The detectors the integration reaches through the family list are the families' own
        // frozen entry points: each claims its own id and refuses another family's screen.
        for family in ReferencePromptFamily::ALL {
            let detector = family.detector();
            assert!(
                detector(family.agent(), fixture_for(family)).is_some(),
                "{}'s own detector reads its own fixture",
                family.agent()
            );
            assert!(
                detector("grok", fixture_for(family)).is_none(),
                "{} claims no provider the reference does not name",
                family.agent()
            );
        }
        assert!(ReferencePromptFamily::Codex.detector()("pi", PI_DIALOG).is_none());
    }

    fn fixture_for(family: ReferencePromptFamily) -> &'static str {
        match family {
            ReferencePromptFamily::Claude => CLAUDE_QUESTION,
            ReferencePromptFamily::Codex => CODEX_MENU,
            ReferencePromptFamily::Omo => OMO_FORM,
            ReferencePromptFamily::Omp => OMP_QUESTION,
            ReferencePromptFamily::Pi => PI_DIALOG,
        }
    }

    fn family_of(agent: &str) -> ReferencePromptFamily {
        match agent {
            "claude" => ReferencePromptFamily::Claude,
            "codex" => ReferencePromptFamily::Codex,
            "omp" => ReferencePromptFamily::Omp,
            "pi" => ReferencePromptFamily::Pi,
            other => panic!("{other} has no family"),
        }
    }

    #[test]
    fn a_detector_never_claims_a_provider_the_reference_does_not_name() {
        for agent in ["grok", "opencode", "cursor", "prime-agent", "droid"] {
            for screen in [CLAUDE_QUESTION, CODEX_MENU, OMP_QUESTION, PI_DIALOG, OMO_FORM] {
                assert!(
                    detect_reference_prompt(agent, screen).is_none(),
                    "{agent} reaches no arm of the reference's chain"
                );
            }
            assert_eq!(ReferencePromptFamily::of_agent(agent), None);
        }
        // each family's own arm refuses another family's screen
        assert!(detect_reference_prompt("claude", CODEX_MENU).is_none());
        assert!(detect_reference_prompt("codex", CLAUDE_QUESTION).is_none());
        assert!(detect_reference_prompt("omp", PI_DIALOG).is_none());
        assert!(detect_reference_prompt("pi", OMP_QUESTION).is_none());
    }

    #[test]
    fn an_omo_form_is_read_for_pi_and_claude_only_after_their_own_readers() {
        // herdr names an omo pane `pi` while it waits: pi's own reader finds nothing, and the
        // omo arm then reads the form.
        let card = detect_reference_prompt_card_in_session("pi", OMO_FORM, &session(OMO_ASKS_TWO))
            .expect("the omo form is the pi arm's fall-through");
        assert_eq!(card.family(), ReferencePromptFamily::Omo);
        // a pane named claude is omo's only on evidence: with the session's call on screen the
        // form is read, and with nothing to name it, it is not.
        assert!(
            detect_reference_prompt_card_in_session("claude", OMO_FORM, &session(OMO_ASKS_TWO))
                .is_some()
        );
        assert!(
            detect_reference_prompt_card_in_session("claude", OMO_FORM, &session("")).is_none()
        );
        // a form printed in a shell is not a card for anyone
        assert!(detect_reference_prompt("omo", OMO_SHELL_QUOTED).is_none());
    }

    #[test]
    fn the_omo_arm_is_untrusted_on_a_claude_pane_that_is_not_blocked() {
        let untrusted = ReferencePromptSession { session_jsonl: None, agent_status: Some("idle") };
        let trusted = ReferencePromptSession { session_jsonl: None, agent_status: Some("blocked") };
        // with nothing on the screen to name the form, a claude pane's status is the evidence
        assert!(omo_card("claude", OMO_FORM, &untrusted).is_none());
        assert!(omo_card("claude", OMO_FORM, &trusted).is_some());
        assert!(!omo_form_is_trusted("claude", Some("idle")));
        assert!(omo_form_is_trusted("claude", Some("blocked")));
        assert!(omo_form_is_trusted("omo", None));
        // pin parity (prompt.ts:2476): an unnamed pane needs the evidence a claude pane needs,
        // so the screen's own text never trusts itself
        assert!(!omo_form_is_trusted("", None));
        // the route's own entry with the session: the call on screen is the evidence, and an
        // unnamed pane with no such call yields no card
        assert!(omo_card("", OMO_FORM, &session(OMO_ASKS_TWO)).is_some());
        assert!(omo_card("", OMO_FORM, &session("")).is_none());
    }

    #[test]
    fn a_folded_widget_is_matched_against_the_session_and_an_open_form_is_not() {
        let folded =
            detect_reference_prompt_card_in_session("omo", OMO_WIDGET, &session(OMO_ASKS_FOLDED))
                .expect("the widget is a card");
        assert!(folded.is_folded_omo(), "the widget over the input box is not answerable in place");
        // the opened form is the same asking, and it is answerable
        let opened = detect_reference_prompt_card_in_session(
            "omo",
            OMO_FORM_OPENED,
            &session(OMO_ASKS_FOLDED),
        )
        .expect("the opened form is a card");
        assert!(!opened.is_folded_omo());
        assert_eq!(
            folded.prompt().id,
            opened.prompt().id,
            "the route verifies the opened form by the id the widget was answered with"
        );
        // without the session's calls there is no widget to match, and no card
        assert!(detect_reference_prompt("omo", OMO_WIDGET).is_none());
    }

    // -----------------------------------------------------------------------------------------
    // Planning: the private card, the codex row key, the key table
    // -----------------------------------------------------------------------------------------

    #[test]
    fn an_id_this_module_never_detected_is_never_answered() {
        let ghost = ghost("never-detected", "?");
        assert_eq!(reference_answer_keys(&ghost, &option(0)), Err(ScopeErrorCode::InvalidRequest));
    }

    #[test]
    fn a_card_answered_through_another_prompt_value_is_refused() {
        let prompt = detect_reference_prompt("claude", CLAUDE_QUESTION).expect("a claude card");
        let other = ReferencePrompt { question: "another asking".into(), ..prompt.clone() };
        assert_eq!(
            reference_answer_keys(&other, &option(0)),
            Err(ScopeErrorCode::RequestConflict),
            "the same id with a different card is not the card that was detected"
        );
    }

    #[test]
    fn a_codex_model_list_leaves_its_row_key_to_the_screen_and_never_guesses_one() {
        let prompt = detect_reference_prompt("codex", CODEX_MODEL_LIST).expect("the list is a card");
        let card = reference_prompt_card(&prompt.id).expect("the chain remembered the card");
        let answer = option(3);
        // the pure entry point has no screen: it refuses the plan rather than inventing an Enter
        assert_eq!(reference_answer_keys(&prompt, &answer), Err(ScopeErrorCode::Unsupported));
        // with the screen it was planned against, the row's own key is read off that screen
        let planned = plan_answer_keys(&card, &prompt, &answer, Some(CODEX_MODEL_LIST)).unwrap();
        assert_eq!(
            planned.last().map(keys_of),
            Some(vec!["enter".to_string()]),
            "the footer of the screen the answer was planned against names Enter"
        );
        // and the moves before it are the moves to the row
        assert!(planned[..planned.len() - 1]
            .iter()
            .all(|step| step.keys == vec!["down".to_string()] || step.keys == vec!["up".to_string()]));
    }

    #[test]
    fn every_key_the_five_families_plan_has_bytes() {
        for (agent, screen, answer) in [
            ("claude", CLAUDE_QUESTION, option(1)),
            ("codex", CODEX_MENU, option(0)),
            ("omp", OMP_QUESTION, option(0)),
            ("pi", PI_DIALOG, option(1)),
        ] {
            let prompt = detect_reference_prompt(agent, screen)
                .unwrap_or_else(|| panic!("{agent} detects its own fixture"));
            let steps = reference_answer_keys(&prompt, &answer)
                .unwrap_or_else(|code| panic!("{agent} plans its own answer: {code:?}"));
            assert!(!steps.is_empty(), "{agent}'s plan has a key");
            for step in &steps {
                for key in &step.keys {
                    assert!(
                        reference_key_bytes(key).is_some(),
                        "{agent} plans {key:?}, which this reader must be able to send"
                    );
                }
                assert!(step.is_effective(), "{agent} plans no empty step");
            }
        }
    }

    #[test]
    fn a_key_name_this_reader_does_not_carry_is_refused_rather_than_approximated() {
        assert!(reference_key_bytes("f13").is_none());
        assert!(reference_key_bytes("").is_none());
        assert_eq!(
            reference_key_step_bytes(&ReferenceKeyStep::keys(["f13"])),
            Err(ScopeErrorCode::Unsupported)
        );
        for name in ["up", "down", "enter", "esc", "space", "tab", "shift+tab", "backspace", "right"] {
            assert!(reference_key_bytes(name).is_some(), "{name} is planned by a family");
        }
        assert_eq!(
            reference_key_step_bytes(&ReferenceKeyStep::typed("feat/x")).unwrap(),
            b"feat/x".to_vec()
        );
        // the key that opens a folded form is carried, and it is not the bare Up
        assert_eq!(reference_key_bytes(REFERENCE_OMO_OPEN_FORM_KEY), Some(b"\x1b\x1b[A".as_slice()));
        assert_ne!(reference_key_bytes(REFERENCE_OMO_OPEN_FORM_KEY), reference_key_bytes("up"));
    }

    // -----------------------------------------------------------------------------------------
    // Answering: fresh screen, single use, one step per pane
    // -----------------------------------------------------------------------------------------

    #[tokio::test]
    async fn an_answer_is_planned_from_the_card_and_written_to_the_original_pane() {
        let prompt = detect_reference_prompt("claude", CLAUDE_QUESTION).expect("a claude card");
        let target = target("sess-1");
        let writer = RecordingWriter::new();
        let clock = ManualClock::default();
        let step = SharedStep::default();
        let reader = ScriptedScreen::one(CLAUDE_QUESTION);
        let answers = ReferencePromptAnswers::new();
        let payload = payload(&prompt.id, REVISION, option(1));
        let request = ReferenceAnswerRequest {
            target: &target,
            request_id: "req-1",
            payload: &payload,
            session: ReferencePromptSession::default(),
            authorize: &allow(),
            alive: &alive(),
        };
        let outcome = answer_once(&answers, &request, &step, &reader, &writer, &clock).await;
        assert!(outcome.is_typed(), "{outcome:?}");
        assert_eq!(outcome.receipt().map(|receipt| receipt.request_id.as_str()), Some("req-1"));
        assert_eq!(
            outcome.receipt().map(|receipt| receipt.stage),
            Some(DeliveryStage::Accepted),
            "the writer took the keys: that is accepted, never providerRead"
        );
        assert_eq!(
            writer.sessions(),
            vec!["sess-1".to_string()],
            "the keys reach the pane's own session"
        );
        assert_eq!(
            writer.text(),
            vec!["\u{1b}[B".to_string(), "\r".to_string()],
            "the plan's moves then its Enter"
        );
    }

    #[tokio::test]
    async fn a_screen_that_moved_since_the_card_was_rendered_is_refused_with_nothing_sent() {
        let prompt = detect_reference_prompt("claude", CLAUDE_QUESTION).expect("a claude card");
        let target = target("sess-1");
        let writer = RecordingWriter::new();
        let clock = ManualClock::default();
        let step = SharedStep::default();
        let mut moved = snapshot(CLAUDE_QUESTION);
        moved.revision = "rev-2".to_string();
        let reader = ScriptedScreen::many(vec![Ok(moved)]);
        let answers = ReferencePromptAnswers::new();
        let payload = payload(&prompt.id, REVISION, option(1));
        let request = ReferenceAnswerRequest {
            target: &target,
            request_id: "req-1",
            payload: &payload,
            session: ReferencePromptSession::default(),
            authorize: &allow(),
            alive: &alive(),
        };
        let outcome = answer_once(&answers, &request, &step, &reader, &writer, &clock).await;
        assert_not_typed(&outcome, ScopeErrorCode::RequestConflict);
        assert_eq!(outcome.wire_message(), Some(REFERENCE_PROMPT_STALE_SCREEN_MESSAGE));
        assert!(writer.log().is_empty(), "a stale card types nothing");
    }

    #[tokio::test]
    async fn a_card_that_no_longer_ends_the_screen_is_refused() {
        let prompt = detect_reference_prompt("claude", CLAUDE_QUESTION).expect("a claude card");
        let target = target("sess-1");
        let writer = RecordingWriter::new();
        let clock = ManualClock::default();
        let step = SharedStep::default();
        // the same revision, but the screen now shows something else entirely
        let reader = ScriptedScreen::one("$ cargo test\nrunning 12 tests\n");
        let answers = ReferencePromptAnswers::new();
        let payload = payload(&prompt.id, REVISION, option(1));
        let request = ReferenceAnswerRequest {
            target: &target,
            request_id: "req-1",
            payload: &payload,
            session: ReferencePromptSession::default(),
            authorize: &allow(),
            alive: &alive(),
        };
        let outcome = answer_once(&answers, &request, &step, &reader, &writer, &clock).await;
        assert_not_typed(&outcome, ScopeErrorCode::RequestConflict);
        assert_eq!(outcome.wire_message(), Some(REFERENCE_PROMPT_STALE_SCREEN_MESSAGE));
        assert!(writer.log().is_empty());
    }

    #[tokio::test]
    async fn a_replayed_answer_is_refused_and_sends_nothing_the_second_time() {
        let prompt = detect_reference_prompt("claude", CLAUDE_QUESTION).expect("a claude card");
        let target = target("sess-1");
        let writer = RecordingWriter::new();
        let clock = ManualClock::default();
        let step = SharedStep::default();
        let reader = ScriptedScreen::one(CLAUDE_QUESTION);
        let answers = ReferencePromptAnswers::new();
        let payload = payload(&prompt.id, REVISION, option(1));

        let mut answered_bytes = 0;
        for attempt in 0..2 {
            let request = ReferenceAnswerRequest {
                target: &target,
                request_id: "req-1",
                payload: &payload,
                session: ReferencePromptSession::default(),
                authorize: &allow(),
                alive: &alive(),
            };
            let outcome = answer_once(&answers, &request, &step, &reader, &writer, &clock).await;
            if attempt == 0 {
                assert!(outcome.is_typed(), "{outcome:?}");
                answered_bytes = writer.log().len();
                assert!(answered_bytes > 0);
            } else {
                assert_not_typed(&outcome, ScopeErrorCode::RequestConflict);
                assert_eq!(outcome.wire_message(), Some(REFERENCE_PROMPT_REPLAYED_ANSWER_MESSAGE));
                assert_eq!(
                    writer.log().len(),
                    answered_bytes,
                    "the replay wrote no byte: the same card is answered once"
                );
            }
        }
    }

    #[tokio::test]
    async fn the_answer_takes_the_panes_own_step_and_cannot_split_a_submit() {
        // One step per target, shared with the submit transaction: the answer cannot land
        // between a message's paste and its Enter.
        let prompt = detect_reference_prompt("claude", CLAUDE_QUESTION).expect("a claude card");
        let target = target("sess-1");
        let writer = Arc::new(RecordingWriter::new());
        let clock = ManualClock::default();
        let step = SharedStep::default();
        let reader = ScriptedScreen::one(CLAUDE_QUESTION);
        let answers = ReferencePromptAnswers::new();
        let payload = payload(&prompt.id, REVISION, option(1));

        let submit_writer = writer.clone();
        let receipt_target = target.target.clone();
        let (pasted, pasted_rx) = oneshot::channel::<()>();
        let submit = step.run(
            &target,
            Box::new(move || {
                Box::pin(async move {
                    assert!(submit_writer.write("sess-1", b"paste").await.is_ok());
                    let _ = pasted.send(());
                    // the pane's own turn still holds the step across this yield
                    tokio::task::yield_now().await;
                    assert!(submit_writer.write("sess-1", b"\r").await.is_ok());
                    ReferenceInputOutcome::Accepted {
                        receipt: DeliveryReceipt {
                            request_id: "submit-1".to_string(),
                            target: receipt_target,
                            stage: DeliveryStage::Accepted,
                        },
                    }
                })
            }),
        );
        let request = ReferenceAnswerRequest {
            target: &target,
            request_id: "req-1",
            payload: &payload,
            session: ReferencePromptSession::default(),
            authorize: &allow(),
            alive: &alive(),
        };
        let answer = answer_once(&answers, &request, &step, &reader, writer.as_ref(), &clock);
        let (submitted, answered) = tokio::join!(submit, answer);
        assert!(pasted_rx.await.is_ok(), "the submit reached its paste");
        assert!(submitted.is_typed(), "{submitted:?}");
        assert!(answered.is_typed(), "{answered:?}");
        let log = writer.text();
        assert!(log.len() >= 3, "the submit's two writes and the answer's keys: {log:?}");
        assert_eq!(log[0], "paste");
        assert_eq!(log[1], "\r", "the answer's keys never land between the paste and its Enter");
    }

    #[tokio::test]
    async fn a_folded_form_is_opened_with_its_own_key_and_the_answer_waits_for_it() {
        let prompt =
            detect_reference_prompt_in_session("omo", OMO_WIDGET, &session(OMO_ASKS_FOLDED))
                .expect("the folded widget is a card");
        let target = target("sess-1");
        let writer = RecordingWriter::new();
        let clock = ManualClock::default();
        let step = SharedStep::default();
        let reader =
            ScriptedScreen::many(vec![Ok(snapshot(OMO_WIDGET)), Ok(snapshot(OMO_FORM_OPENED))]);
        let answers = ReferencePromptAnswers::new();
        let payload = payload(&prompt.id, REVISION, option(0));
        let request = ReferenceAnswerRequest {
            target: &target,
            request_id: "req-1",
            payload: &payload,
            session: session(OMO_ASKS_FOLDED),
            authorize: &allow(),
            alive: &alive(),
        };
        let outcome = answer_once(&answers, &request, &step, &reader, &writer, &clock).await;
        let log = writer.text();
        assert_eq!(
            log.first().map(String::as_str),
            Some("\u{1b}\u{1b}[A"),
            "the folded form is opened with the route's own key: {log:?}"
        );
        assert!(
            reader.reads() >= 2,
            "the opened form is read again before any answer key: {} read(s)",
            reader.reads()
        );
        match &outcome {
            ReferenceInputOutcome::Accepted { .. } => {
                assert!(log.len() > 1, "the answer's keys follow the open key: {log:?}");
            }
            // the opened form's own plan decides whether keys follow; a refusal is reported,
            // never silent, and the two claims above hold either way
            other => assert!(other.wire_code().is_some(), "{other:?}"),
        }
    }

    #[tokio::test]
    async fn a_folded_form_that_never_opens_refuses_the_answer_and_says_which_key_went() {
        let prompt =
            detect_reference_prompt_in_session("omo", OMO_WIDGET, &session(OMO_ASKS_FOLDED))
                .expect("the folded widget is a card");
        let target = target("sess-1");
        let writer = RecordingWriter::new();
        let clock = ManualClock::default();
        let step = SharedStep::default();
        // the widget stays on screen: the form never opens
        let reader = ScriptedScreen::one(OMO_WIDGET);
        let answers = ReferencePromptAnswers::new();
        let payload = payload(&prompt.id, REVISION, option(0));
        let request = ReferenceAnswerRequest {
            target: &target,
            request_id: "req-1",
            payload: &payload,
            session: session(OMO_ASKS_FOLDED),
            authorize: &allow(),
            alive: &alive(),
        };
        let outcome = answer_once(&answers, &request, &step, &reader, &writer, &clock).await;
        assert_not_typed(&outcome, ScopeErrorCode::RequestConflict);
        assert_eq!(outcome.wire_message(), Some(REFERENCE_PROMPT_FOLDED_FORM_MESSAGE));
        assert_eq!(
            writer.text(),
            vec!["\u{1b}\u{1b}[A".to_string()],
            "only the open key went out, and the refusal says so"
        );
        assert!(reader.reads() > 1, "the form is polled for its settle window");
        assert!(!clock.sleeps().is_empty(), "the wait is the clock's, never a real sleep");
        assert_eq!(
            clock.now(),
            REFERENCE_PROMPT_SETTLE_MS,
            "the settle window is measured on the clock, and it ended it"
        );
    }

    #[tokio::test]
    async fn a_gapped_screen_can_never_validate_a_revision() {
        let prompt = detect_reference_prompt("claude", CLAUDE_QUESTION).expect("a claude card");
        let target = target("sess-1");
        let writer = RecordingWriter::new();
        let clock = ManualClock::default();
        let step = SharedStep::default();
        let mut gapped = snapshot(CLAUDE_QUESTION);
        gapped.gap = true;
        let reader = ScriptedScreen::many(vec![Ok(gapped)]);
        let answers = ReferencePromptAnswers::new();
        let payload = payload(&prompt.id, REVISION, option(1));
        let request = ReferenceAnswerRequest {
            target: &target,
            request_id: "req-1",
            payload: &payload,
            session: ReferencePromptSession::default(),
            authorize: &allow(),
            alive: &alive(),
        };
        let outcome = answer_once(&answers, &request, &step, &reader, &writer, &clock).await;
        assert_not_typed(&outcome, ScopeErrorCode::RequestConflict);
        assert_eq!(outcome.wire_message(), Some(REFERENCE_PROMPT_GAP_MESSAGE));
        assert!(writer.log().is_empty());
    }

    #[tokio::test]
    async fn a_screen_that_cannot_be_read_at_all_is_refused() {
        let prompt = detect_reference_prompt("claude", CLAUDE_QUESTION).expect("a claude card");
        let target = target("sess-1");
        let writer = RecordingWriter::new();
        let clock = ManualClock::default();
        let step = SharedStep::default();
        let reader = ScriptedScreen::many(vec![Err("the mirror is gone".to_string())]);
        let answers = ReferencePromptAnswers::new();
        let payload = payload(&prompt.id, REVISION, option(1));
        let request = ReferenceAnswerRequest {
            target: &target,
            request_id: "req-1",
            payload: &payload,
            session: ReferencePromptSession::default(),
            authorize: &allow(),
            alive: &alive(),
        };
        let outcome = answer_once(&answers, &request, &step, &reader, &writer, &clock).await;
        assert_not_typed(&outcome, ScopeErrorCode::Timeout);
        assert!(writer.log().is_empty());
    }

    #[tokio::test]
    async fn a_revoked_caller_or_one_that_left_types_nothing() {
        let prompt = detect_reference_prompt("claude", CLAUDE_QUESTION).expect("a claude card");
        let target = target("sess-1");
        let clock = ManualClock::default();
        let step = SharedStep::default();
        let reader = ScriptedScreen::one(CLAUDE_QUESTION);
        let answers = ReferencePromptAnswers::new();
        let payload = payload(&prompt.id, REVISION, option(1));

        let writer = RecordingWriter::new();
        let request = ReferenceAnswerRequest {
            target: &target,
            request_id: "req-1",
            payload: &payload,
            session: ReferencePromptSession::default(),
            authorize: &refusal(),
            alive: &alive(),
        };
        let outcome = answer_once(&answers, &request, &step, &reader, &writer, &clock).await;
        assert_not_typed(&outcome, ScopeErrorCode::Unauthorized);
        assert_eq!(reader.reads(), 0, "a refused caller reads nothing");
        assert!(writer.log().is_empty());

        let writer = RecordingWriter::new();
        let request = ReferenceAnswerRequest {
            target: &target,
            request_id: "req-2",
            payload: &payload,
            session: ReferencePromptSession::default(),
            authorize: &allow(),
            alive: &gone(),
        };
        let outcome = answer_once(&answers, &request, &step, &reader, &writer, &clock).await;
        assert_not_typed(&outcome, ScopeErrorCode::Unauthorized);
        assert_eq!(outcome.wire_message(), Some(REFERENCE_CALLER_GONE_MESSAGE));
        assert!(writer.log().is_empty());
    }

    #[tokio::test]
    async fn an_answer_that_is_not_exactly_one_shape_is_refused_before_the_pane_is_read() {
        let prompt = detect_reference_prompt("claude", CLAUDE_QUESTION).expect("a claude card");
        let target = target("sess-1");
        let writer = RecordingWriter::new();
        let clock = ManualClock::default();
        let step = SharedStep::default();
        let reader = ScriptedScreen::one(CLAUDE_QUESTION);
        let answers = ReferencePromptAnswers::new();
        let ambiguous = ReferencePromptAnswer {
            option_index: Some(0),
            option_indices: Some(vec![0]),
            custom_text: None,
        };
        let payload = payload(&prompt.id, REVISION, ambiguous);
        let request = ReferenceAnswerRequest {
            target: &target,
            request_id: "req-1",
            payload: &payload,
            session: ReferencePromptSession::default(),
            authorize: &allow(),
            alive: &alive(),
        };
        let outcome = answer_once(&answers, &request, &step, &reader, &writer, &clock).await;
        assert_not_typed(&outcome, ScopeErrorCode::InvalidRequest);
        assert_eq!(outcome.wire_message(), Some(REFERENCE_PROMPT_ONE_ANSWER_MESSAGE));
        assert_eq!(reader.reads(), 0, "a malformed answer reads nothing");
        assert!(writer.log().is_empty());
    }

    #[tokio::test]
    async fn a_write_that_cannot_be_confirmed_is_unknown_and_never_replayed() {
        let prompt = detect_reference_prompt("claude", CLAUDE_QUESTION).expect("a claude card");
        let target = target("sess-1");
        // the first key is dispatched and its confirmation is lost
        let writer = RecordingWriter::failing_at(0);
        let clock = ManualClock::default();
        let step = SharedStep::default();
        let reader = ScriptedScreen::one(CLAUDE_QUESTION);
        let answers = ReferencePromptAnswers::new();
        let payload = payload(&prompt.id, REVISION, option(1));
        let request = ReferenceAnswerRequest {
            target: &target,
            request_id: "req-1",
            payload: &payload,
            session: ReferencePromptSession::default(),
            authorize: &allow(),
            alive: &alive(),
        };
        let outcome = answer_once(&answers, &request, &step, &reader, &writer, &clock).await;
        assert!(outcome.is_outcome_unknown(), "{outcome:?}");
        assert_eq!(outcome.wire_code().as_deref(), Some(REFERENCE_OUTCOME_UNKNOWN_CODE));
        assert!(writer.log().is_empty(), "the unconfirmed write recorded no byte");
        // and the card is not recorded as answered, so the route's own pending record is what
        // holds it: this module never replays it by itself
        assert!(reference_answer_keys(&prompt, &option(1)).is_ok());
    }

    #[test]
    fn the_fresh_detection_refreshes_every_family_registry_the_answer_needs() {
        // The OMP planner reads the OMP lane's registry, which its own card detector does not
        // populate: the chain does.
        let prompt = detect_reference_prompt("omp", OMP_QUESTION).expect("an omp card");
        assert!(
            super::super::prompt_omp::omp_prompt_card(&prompt.id).is_some(),
            "the chain remembered the omp card its planner answers with"
        );
        // and the omo card an answer is planned from is the freshly detected one
        let omo = detect_reference_prompt_in_session("omo", OMO_FORM, &session(OMO_ASKS_TWO))
            .expect("an omo card");
        let card = reference_prompt_card(&omo.id).expect("the omo card is remembered");
        assert_eq!(card.family(), ReferencePromptFamily::Omo);
        assert_eq!(card.prompt().id, omo.id);
    }

    #[test]
    fn remembering_a_card_again_refreshes_it_in_place_and_keeps_it_answerable() {
        let prompt = detect_reference_prompt("claude", CLAUDE_QUESTION).expect("a claude card");
        let card = reference_prompt_card(&prompt.id).expect("the card is remembered");
        // a fresh read of the same card refreshes the state an answer is planned from
        remember_reference_prompt_card(&card);
        remember_reference_prompt_card(&card);
        let held = reference_prompt_card(&prompt.id).expect("still held");
        assert_eq!(held, card);
        assert!(reference_answer_keys(&prompt, &option(1)).is_ok());
        assert!(REFERENCE_PROMPT_CARD_LIMIT >= 16, "the registry holds more than a screenful");
    }

    #[tokio::test]
    async fn a_second_answer_for_the_same_pane_waits_its_turn_behind_the_first() {
        let prompt = detect_reference_prompt("claude", CLAUDE_QUESTION).expect("a claude card");
        let target = target("sess-1");
        let writer = Arc::new(RecordingWriter::new());
        let clock = ManualClock::default();
        let step = SharedStep::default();
        let reader = ScriptedScreen::one(CLAUDE_QUESTION);
        let answers = ReferencePromptAnswers::new();
        let payload = payload(&prompt.id, REVISION, option(1));
        let request = ReferenceAnswerRequest {
            target: &target,
            request_id: "req-1",
            payload: &payload,
            session: ReferencePromptSession::default(),
            authorize: &allow(),
            alive: &alive(),
        };
        let first = answer_once(&answers, &request, &step, &reader, writer.as_ref(), &clock);
        let second = answer_once(&answers, &request, &step, &reader, writer.as_ref(), &clock);
        let (first, second) = tokio::join!(first, second);
        // one of them answers; the other is refused as a replay, never sent twice
        assert!(first.is_typed() ^ second.is_typed(), "{first:?} / {second:?}");
        let refused = if first.is_typed() { second } else { first };
        assert_not_typed(&refused, ScopeErrorCode::RequestConflict);
    }

    #[tokio::test]
    async fn a_caller_that_left_after_the_first_key_gets_an_unknown_outcome() {
        // A key that may already have been pressed cannot be un-pressed: the answer is held, not
        // silently reported as nothing. pi's row 2 plans two moves and an Enter.
        let prompt = detect_reference_prompt("pi", PI_DIALOG).expect("a pi dialog");
        let target = target("sess-1");
        let writer = RecordingWriter::new();
        let clock = ManualClock::default();
        let step = SharedStep::default();
        let reader = ScriptedScreen::one(PI_DIALOG);
        let answers = ReferencePromptAnswers::new();
        let payload = payload(&prompt.id, REVISION, option(2));
        // the read, then the first key, then the caller is gone
        let calls = AtomicUsize::new(0);
        let alive_fn = move || calls.fetch_add(1, Ordering::SeqCst) < 2;
        let request = ReferenceAnswerRequest {
            target: &target,
            request_id: "req-1",
            payload: &payload,
            session: ReferencePromptSession::default(),
            authorize: &allow(),
            alive: &alive_fn,
        };
        let outcome = answer_once(&answers, &request, &step, &reader, &writer, &clock).await;
        match &outcome {
            ReferenceInputOutcome::OutcomeUnknown { code, .. } => {
                assert_eq!(code, REFERENCE_OUTCOME_UNKNOWN_CODE);
                assert_eq!(
                    writer.text(),
                    vec!["\u{1b}[B".to_string()],
                    "the key that went out is the one the plan sent before the caller left"
                );
            }
            other => panic!("a key already sent is held as unknown, got {other:?}"),
        }
    }

    #[test]
    fn the_codex_row_key_is_read_from_the_screens_own_footer() {
        // The pinned reader finds the last line whose own footer names a key: a screen whose
        // footer is the list's own reads it, and one with nothing after it reads nothing.
        assert_eq!(
            codex_row_key_on_screen(CODEX_MODEL_LIST).map(|step| keys_of(&step)),
            Some(vec!["enter".to_string()])
        );
        assert!(codex_row_key_on_screen("$ cargo test\n").is_none());
        // a footer that offers the session pick is typed, never an Enter
        assert_eq!(
            codex_row_key_on_screen("  1. GPT-6.1-Sol (default)\n\u{276f} 2. Medium\n\n  enter default · s session · esc back\n")
                .map(|step| step.text),
            Some(Some("s".to_string()))
        );
    }

    #[tokio::test]
    async fn the_real_queue_adapter_shares_the_step_submit_and_stop_take() {
        // The adapter the route layer wires: one ReferenceInputQueue, the step submit and Stop
        // take. The submit holds that step across its paste-Enter gap, so the answer cannot land
        // inside it.
        let prompt = detect_reference_prompt("claude", CLAUDE_QUESTION).expect("a claude card");
        let target = target("sess-1");
        let queue = ReferenceInputQueue::new();
        let step = ReferenceQueueStep::new(&queue);
        let writer = Arc::new(RecordingWriter::new());
        let clock = ManualClock::default();
        let reader = ScriptedScreen::one(CLAUDE_QUESTION);
        let answers = ReferencePromptAnswers::new();
        let payload = payload(&prompt.id, REVISION, option(1));

        let submit_payload = ReferenceSubmitPayload {
            text: "hello".to_string(),
            attachment_ids: vec![],
            origin: ReferenceSubmitOrigin::Chat,
        };
        let submit_request = ReferenceSubmitRequest {
            target: &target,
            request_id: "submit-1",
            payload: &submit_payload,
            bracketed_paste: false,
            blocked_prompt: None,
            arrived_at_ms: clock.now(),
            last_typed_at_ms: None,
            authorize: &allow(),
        };
        let request = ReferenceAnswerRequest {
            target: &target,
            request_id: "req-1",
            payload: &payload,
            session: ReferencePromptSession::default(),
            authorize: &allow(),
            alive: &alive(),
        };
        let submitted = queue.submit(&submit_request, writer.as_ref(), &clock);
        let answered = answer_once(&answers, &request, &step, &reader, writer.as_ref(), &clock);
        let (submitted, answered) = tokio::join!(submitted, answered);
        assert!(submitted.is_typed(), "{submitted:?}");
        assert!(answered.is_typed(), "{answered:?}");
        let log = writer.text();
        assert_eq!(log.first().map(String::as_str), Some("hello"));
        assert_eq!(
            log.get(1).map(String::as_str),
            Some("\r"),
            "the answer waits for the message's own Enter: {log:?}"
        );
        assert_eq!(
            log.len(),
            4,
            "the submit's paste and Enter, then the answer's two keys: {log:?}"
        );
    }
}
