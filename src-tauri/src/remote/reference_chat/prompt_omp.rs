//! omp prompt family (plan task 24).
//!
//! Ported from `devswha/herdr-web-ui` @ `54e5a1f67090cb09552d182e7e30dd0ecc314918` (MIT, see
//! `docs/chat/HERDR_LICENSE`): `server/prompt.ts` `parseOmpQuestion` (lines 263-287) and
//! `parseOmpApproval` (lines 1092-1111), plus every helper those two call — `cleanLine`
//! (`:135-141`), `isDivider` (`:142-146`), `normalizeText` (`:147-150`), `findLastIndex`
//! (`:151-162`), `wrapped` (`:163-166`), `nearestQuestion` (`:167-176`), `parseBorderMenu`
//! (`:177-190`), `findMenuDividers` (`:191-200`), `finishPrompt` (`:228-242`), the omp
//! branches of `promptTailIsActive` (`:1613`, `:1621`) and the omp branches of `answerKeys`
//! (`:2032-2056`), reached only through the `agent === "omp"` gate of `parsePrompt`
//! (`:1941-1943`).
//!
//! Frozen contract: `docs/chat/herdr-port-contract.md` §6. This lane provides
//! [`detect_omp_prompt`] (the frozen [`ReferencePromptDetector`] shape) and
//! [`reference_answer_keys`]; the dispatcher (task 9) owns the cross-family entry points and
//! delegates the `omp` registry id here. The module is **not** declared in `mod.rs` by this
//! task: the integration owner adds `pub mod prompt_omp;` in the change that adds the file.
//!
//! ## What this lane claims, and what it refuses
//!
//! * It claims exactly one agent id, [`OMP_AGENT_ID`]. Upstream reaches these two parsers
//!   only for `agent === "omp"` (`:1941-1943`); every other id returns `None` here rather than
//!   falling through to another family's reader.
//! * It ports the two named omp responders and nothing else. There is no `parseOmpModel` and
//!   no `parseOmpPlan` upstream, so none is invented; `queued`, `steps` and `fallback` stay
//!   unset because the pinned omp branches set none of them.
//! * A screen that merely *looks* like a menu is not a prompt. The question parser requires
//!   the `Other (type your own)` row (`:281`), the approval parser requires **exactly two**
//!   `Approve`/`Deny` rows with one cursor (`:1100`), and both must still be the tail of the
//!   screen (`:1613`, `:1621`). Anything short of that is `None`, never a guess.
//!
//! ## The answer plan needs internal state
//!
//! `answerKeys` reads `selectedIndex`, `checkedOptionIndices`, `customMenuIndex` and the
//! responder off a `WeakMap` beside the public prompt (`:133`, `:1996-1997`) and refuses an
//! answer for a prompt it did not itself parse. Ferryx's [`ReferencePrompt`] crosses the wire,
//! so this lane keeps that state in a bounded registry keyed by the prompt id and exposes it
//! as [`OmpPromptCard`]. The route re-reads the screen and re-detects immediately before
//! answering (`docs/chat/herdr-port-contract.md` §5), which is what repopulates the registry;
//! [`reference_answer_keys`] therefore answers with [`ScopeErrorCode::Unsupported`] for a
//! prompt this lane did not produce, exactly as upstream raises `InvalidAnswer`.
//!
//! ## Known cosmetic divergence
//!
//! Upstream's id hashes `JSON.stringify({agent, ...input, ...hashed})`, whose key order is the
//! object literal's. This port serializes through `serde_json`, whose object order depends on
//! whether the build enables `serde_json/preserve_order`; without it the keys are sorted. The
//! id's *content* — including the `null`s upstream emits for `body` and for every option's
//! `description` — is identical, so the id is deterministic and changes with the menu text.
//! Nothing on the wire compares a Ferryx id against a Herdr id.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use regex::Regex;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::scoped_contracts::ScopeErrorCode;

use super::types::{
    ReferenceKeyStep, ReferencePrompt, ReferencePromptAnswer, ReferencePromptDetector,
    ReferencePromptKind, ReferencePromptOption,
};

/// The one registry id this lane answers for. Upstream's `parsePrompt` reaches the omp
/// candidates only under this name (`server/prompt.ts:1941-1943`).
pub const OMP_AGENT_ID: &str = "omp";

/// The pinned detector, in the frozen [`ReferencePromptDetector`] shape.
pub const OMP_PROMPT_DETECTOR: ReferencePromptDetector = detect_omp_prompt;

/// Upper bound on the prompt cards this lane remembers.
///
/// The registry exists so an answer can be planned against the state the card was detected
/// from; it is not a session store. The route re-detects before every answer, so the newest
/// card is always present.
pub const OMP_PROMPT_CARD_LIMIT: usize = 64;

/// `OMP_SINGLE_HINT_RE` (`:13`).
const OMP_SINGLE_HINT: &str = r"(?i)enter select.*↑/↓ move.*esc cancel";
/// `OMP_MULTI_HINT_RE` (`:14`).
const OMP_MULTI_HINT: &str = r"(?i)space/enter toggle.*↑/↓ move.*esc cancel";
/// `ANSI_RE` (`:10`).
const ANSI_RE: &str = r"\x1b\[[0-?]*[ -/]*[@-~]";
/// `SELECTED_RE` (`:11`).
const SELECTED_RE: &str = r"^[❯›>]\s*";
/// `DIVIDER_RE` (`:12`).
const DIVIDER_RE: &str = r"^[\s╭╮╰╯├┤┬┴┼─━═╌▔]+$";
/// The checked marker `parseBorderMenu` reads before stripping a row's marker (`:185-188`).
const CHECKED_MARKER_RE: &str = r"^[☑☒✓]";
/// The marker `parseBorderMenu` strips from a row's label (`:188`).
const ROW_MARKER_RE: &str = r"^[○●◉◯☐☑☒✓]\s*";
/// The row `parseOmpQuestion` requires before a question counts (`:278`).
const CUSTOM_ROW_RE: &str = r"(?i)^Other \(type your own\)$";
/// The suffix `parseOmpQuestion` strips from an option's label (`:283`).
const RECOMMENDED_RE: &str = r"(?i) \(Recommended\)$";
/// `parseOmpApproval`'s header (`:1093`).
const APPROVAL_HEADER_RE: &str = r"(?i)^\s*Allow tool:\s*\S+";
/// One `Approve`/`Deny` row, with the cursor marker `parseOmpApproval` reads (`:1098`).
const APPROVAL_ROW_RE: &str = r"(?i)^([›>❯•])?\s*(Approve|Deny)$";
/// `promptTailIsActive`'s omp-approval branch (`:1621`). The second alternative is
/// deliberately unanchored, exactly as upstream writes it.
const APPROVAL_TAIL_RE: &str = r"(?i)^(?:[›>❯•]\s*)?(?:Approve|Deny)$|esc.*cancel";
/// The lines `nearestQuestion` refuses to read as a question (`:170-173`).
const PLANNING_RE: &str = r"(?i)^Planning:";
const SUBMIT_TAB_RE: &str = r"^[←→].*Submit";
const TASK_ROW_RE: &str = r"^[☐☑✔]\s+\S";
const QUESTION_NUMBER_RE: &str = r"(?i)^Question \d+/\d+";
/// The count `nearestQuestion` strips from the front of a question (`:174`).
const SELECTED_COUNT_RE: &str = r"(?i)^\(\d+\s+selected\)\s*";

/// The pinned `KEY` names this family sends (`:67-78`).
const KEY_UP: &str = "up";
const KEY_DOWN: &str = "down";
const KEY_ENTER: &str = "enter";
const KEY_SPACE: &str = "space";
const KEY_TAB: &str = "tab";

/// How many lines a hint may wrap onto (`wrapped`'s `span = 3`, `:163-166`).
const HINT_SPAN: usize = 3;
/// How far above the menu `nearestQuestion` looks (`:169`).
const NEAREST_QUESTION_LINES: usize = 14;

/// The internal responder a card came from. Upstream keeps this on the `WeakMap` record
/// (`:84-105`) and switches on it in `promptTailIsActive` and `answerKeys`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OmpPromptResponder {
    /// `parseOmpQuestion` (`:263-287`).
    Question,
    /// `parseOmpApproval` (`:1092-1111`).
    Approval,
}

impl OmpPromptResponder {
    /// The pinned responder name, for logs and tests.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Question => "omp-question",
            Self::Approval => "omp-approval",
        }
    }
}

/// A detected omp prompt together with the state upstream keeps beside the public prompt.
///
/// [`Self::answer_steps`] needs the cursor row, the checked rows and the custom row's index;
/// the public [`ReferencePrompt`] carries none of them, so they travel here.
#[derive(Debug, Clone, PartialEq)]
pub struct OmpPromptCard {
    /// The public card. Its `id` is what [`reference_answer_keys`] looks the card up by.
    pub prompt: ReferencePrompt,
    pub responder: OmpPromptResponder,
    /// Every menu row's label, in order, as the card offers it: a row's `(Recommended)` suffix
    /// is stripped exactly as the option list strips it, so one row is named alike in both.
    /// (Divergence: upstream's `menuLabels` (`:282`) keeps the suffix; no pinned matcher
    /// compares this list against another reader's.)
    pub menu_labels: Vec<String>,
    /// The row the cursor is on (`:277`).
    pub selected_index: usize,
    /// The option rows already checked, by **option** index (`:283`).
    pub checked_option_indices: Vec<usize>,
    /// The `Other (type your own)` row's index among the menu rows (`:283`).
    pub custom_menu_index: Option<usize>,
}

impl OmpPromptCard {
    /// The keys and text that answer this card, in order.
    ///
    /// Ports the omp reachable paths of `answerKeys` (`:1995-2056`): a single option is the
    /// moves from the cursor then `enter`; a multiple choice toggles the differing rows with
    /// `space` and leaves with `tab`, `enter`; a custom answer moves to the `Other` row,
    /// presses `enter`, types, and presses `enter`. Every refusal upstream raises as
    /// `InvalidAnswer` is a typed [`ScopeErrorCode`] here — never a guessed key.
    pub fn answer_steps(
        &self,
        answer: &ReferencePromptAnswer,
    ) -> Result<Vec<ReferenceKeyStep>, ScopeErrorCode> {
        // "Exactly one answer is required." (`:1999-2000`)
        if answer.variant_count() != 1 {
            return Err(ScopeErrorCode::InvalidRequest);
        }

        if let Some(text) = answer.custom_text.as_deref() {
            let text = text.trim();
            // "This prompt does not accept a custom answer." (`:2006`)
            let Some(custom_menu_index) = self.custom_menu_index else {
                return Err(ScopeErrorCode::Unsupported);
            };
            if text.is_empty() || self.prompt.multi_select {
                return Err(ScopeErrorCode::Unsupported);
            }
            // The pinned custom path (`:2007-2021`): the moves to the `Other` row, then
            // `enter` — omp is not in the exclusion list `["claude-question", "claude-plan",
            // "codex-question", "codex-async-question", "pi-input"]` — then the text, then
            // `enter`.
            let mut keys = navigation_keys(custom_menu_index as isize - self.selected_index as isize);
            keys.push(KEY_ENTER.to_string());
            let mut steps = key_steps(&keys);
            steps.push(ReferenceKeyStep::typed(text));
            steps.push(ReferenceKeyStep::keys([KEY_ENTER]));
            return Ok(steps);
        }

        if let Some(indices) = answer.option_indices.as_deref() {
            // "This prompt requires one or more selections." (`:2026`)
            if !self.prompt.multi_select || indices.is_empty() {
                return Err(ScopeErrorCode::InvalidRequest);
            }
            let mut choices: Vec<usize> = Vec::new();
            for index in indices {
                let index = *index as usize;
                // "An option index is outside the displayed range." (`:2029`)
                if index >= self.prompt.options.len() {
                    return Err(ScopeErrorCode::InvalidRequest);
                }
                if !choices.contains(&index) {
                    choices.push(index);
                }
            }
            // The pinned multiple-choice path (`:2034-2041`): walk the rows in order and
            // toggle the ones whose wanted state differs from what the screen shows.
            let mut keys: Vec<String> = Vec::new();
            let mut cursor = self.selected_index;
            for option_index in 0..self.prompt.options.len() {
                let wanted = choices.contains(&option_index);
                let shown = self.checked_option_indices.contains(&option_index);
                if wanted == shown {
                    continue;
                }
                keys.extend(navigation_keys(option_index as isize - cursor as isize));
                keys.push(KEY_SPACE.to_string());
                cursor = option_index;
            }
            keys.push(KEY_TAB.to_string());
            keys.push(KEY_ENTER.to_string());
            return Ok(key_steps(&keys));
        }

        let Some(index) = answer.option_index else {
            return Err(ScopeErrorCode::InvalidRequest);
        };
        let index = index as usize;
        // "A valid option index is required." (`:2052-2053`)
        if index >= self.prompt.options.len()
            || Some(index as u32) == self.prompt.custom_option_index
            || self.prompt.multi_select
        {
            return Err(ScopeErrorCode::InvalidRequest);
        }
        // omp sets neither `optionSteps` nor `rejectWithEscapeIndex`, so the generic path
        // applies: the moves from the cursor, then `enter` (`:2054`).
        let mut keys = navigation_keys(index as isize - self.selected_index as isize);
        keys.push(KEY_ENTER.to_string());
        Ok(key_steps(&keys))
    }
}

/// Detect an omp prompt on a screen and remember the card that answers it.
///
/// This is the frozen [`ReferencePromptDetector`] entry point. It claims only
/// [`OMP_AGENT_ID`]; a screen for any other registry id returns `None`.
pub fn detect_omp_prompt(agent: &str, screen: &str) -> Option<ReferencePrompt> {
    let card = detect_omp_prompt_card(agent, screen)?;
    remember_omp_prompt(&card);
    Some(card.prompt)
}

/// As [`detect_omp_prompt`], returning the card with the state an answer needs.
pub fn detect_omp_prompt_card(agent: &str, screen: &str) -> Option<OmpPromptCard> {
    if agent != OMP_AGENT_ID {
        return None;
    }
    // `parsePrompt`'s omp candidate list, first match wins (`:1941-1943`).
    let card = parse_omp_question(screen).or_else(|| parse_omp_approval(screen))?;
    // A candidate only counts while its own tail is still the end of the screen (`:1953`).
    if !omp_prompt_tail_is_active(card.responder, screen) {
        return None;
    }
    Some(card)
}

/// Remember a card so [`reference_answer_keys`] can plan an answer for it.
pub fn remember_omp_prompt(card: &OmpPromptCard) {
    let mut registry = omp_prompt_cards().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if registry.len() >= OMP_PROMPT_CARD_LIMIT && !registry.contains_key(&card.prompt.id) {
        if let Some(oldest) = registry.keys().next().cloned() {
            registry.remove(&oldest);
        }
    }
    registry.insert(card.prompt.id.clone(), card.clone());
}

/// The remembered card for a prompt id, if this lane produced it.
pub fn omp_prompt_card(prompt_id: &str) -> Option<OmpPromptCard> {
    omp_prompt_cards()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(prompt_id)
        .cloned()
}

/// The keys that answer a prompt on the original pane (frozen contract §6).
///
/// An id this lane did not detect is refused with [`ScopeErrorCode::Unsupported`], mirroring
/// upstream's `InvalidAnswer` for "The prompt was not produced by parseInteractivePrompt."
/// (`:1997`). The caller re-reads the screen and re-detects immediately before sending, so a
/// card that is stale by the time the keys would go out is refused by the screen revision,
/// not by a guess here.
pub fn reference_answer_keys(
    prompt: &ReferencePrompt,
    answer: &ReferencePromptAnswer,
) -> Result<Vec<ReferenceKeyStep>, ScopeErrorCode> {
    let card = omp_prompt_card(&prompt.id).ok_or(ScopeErrorCode::Unsupported)?;
    if card.prompt != *prompt {
        return Err(ScopeErrorCode::Unsupported);
    }
    card.answer_steps(answer)
}

// ---------------------------------------------------------------------------------------
// The pinned helpers, ported one for one.
// ---------------------------------------------------------------------------------------

struct Patterns {
    single_hint: Regex,
    multi_hint: Regex,
    ansi: Regex,
    selected: Regex,
    divider: Regex,
    checked_marker: Regex,
    row_marker: Regex,
    custom_row: Regex,
    recommended: Regex,
    approval_header: Regex,
    approval_row: Regex,
    approval_tail: Regex,
    planning: Regex,
    submit_tab: Regex,
    task_row: Regex,
    question_number: Regex,
    selected_count: Regex,
    whitespace: Regex,
}

fn patterns() -> &'static Patterns {
    static PATTERNS: OnceLock<Patterns> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        let compile = |pattern: &str| Regex::new(pattern).expect("the pinned omp pattern compiles");
        Patterns {
            single_hint: compile(OMP_SINGLE_HINT),
            multi_hint: compile(OMP_MULTI_HINT),
            ansi: compile(ANSI_RE),
            selected: compile(SELECTED_RE),
            divider: compile(DIVIDER_RE),
            checked_marker: compile(CHECKED_MARKER_RE),
            row_marker: compile(ROW_MARKER_RE),
            custom_row: compile(CUSTOM_ROW_RE),
            recommended: compile(RECOMMENDED_RE),
            approval_header: compile(APPROVAL_HEADER_RE),
            approval_row: compile(APPROVAL_ROW_RE),
            approval_tail: compile(APPROVAL_TAIL_RE),
            planning: compile(PLANNING_RE),
            submit_tab: compile(SUBMIT_TAB_RE),
            task_row: compile(TASK_ROW_RE),
            question_number: compile(QUESTION_NUMBER_RE),
            selected_count: compile(SELECTED_COUNT_RE),
            whitespace: compile(r"\s+"),
        }
    })
}

fn omp_prompt_cards() -> &'static Mutex<HashMap<String, OmpPromptCard>> {
    static CARDS: OnceLock<Mutex<HashMap<String, OmpPromptCard>>> = OnceLock::new();
    CARDS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// One menu row as `parseBorderMenu` reads it (`:130`).
struct MenuRow {
    label: String,
    selected: bool,
    checked: bool,
    line_index: usize,
}

/// `cleanLine` (`:135-141`): strip ANSI, trim, drop the box's own border characters, trim.
fn clean_line(raw_line: &str) -> String {
    let mut line = strip_ansi(raw_line);
    line = line.trim().to_string();
    if let Some(rest) = line.strip_prefix('│') {
        line = rest.trim_start().to_string();
    }
    if let Some(rest) = line.strip_suffix('│') {
        line = rest.trim_end().to_string();
    }
    line.trim().to_string()
}

/// `isDivider` (`:142-146`).
fn is_divider(line: &str) -> bool {
    let value = clean_line(line);
    !value.is_empty() && patterns().divider.is_match(&value)
}

/// `normalizeText` (`:147-150`).
fn normalize_text(value: &str) -> String {
    patterns().whitespace.replace_all(value, " ").trim().to_string()
}

fn strip_ansi(text: &str) -> String {
    patterns().ansi.replace_all(text, "").into_owned()
}

/// `findLastIndex` (`:151-162`).
fn find_last_index<T>(lines: &[T], predicate: impl Fn(&T, usize) -> bool) -> Option<usize> {
    for index in (0..lines.len()).rev() {
        if predicate(&lines[index], index) {
            return Some(index);
        }
    }
    None
}

/// `wrapped` (`:163-166`): a line and the `span - 1` after it, as one, so a narrow pane's
/// wrapped hint still matches. The index a window matches from is where the hint begins.
fn wrapped(lines: &[String], index: usize, span: usize) -> String {
    lines
        .iter()
        .skip(index)
        .take(span)
        .map(|line| clean_line(line))
        .filter(|line| !line.is_empty() && !is_divider(line))
        .collect::<Vec<_>>()
        .join(" ")
}

/// `nearestQuestion` (`:167-176`): the closest line above the menu that reads as a question.
fn nearest_question(lines: &[String], before_index: usize) -> Option<String> {
    let floor = before_index.saturating_sub(NEAREST_QUESTION_LINES);
    let mut index = before_index;
    while index > floor {
        index -= 1;
        let line = clean_line(&lines[index]);
        if line.is_empty()
            || is_divider(&line)
            || patterns().planning.is_match(&line)
            || patterns().submit_tab.is_match(&line)
            || patterns().task_row.is_match(&line)
            || patterns().question_number.is_match(&line)
        {
            continue;
        }
        return Some(patterns().selected_count.replace(&line, "").trim().to_string());
    }
    None
}

/// `parseBorderMenu` (`:177-190`): the rows between two dividers, markers stripped.
fn parse_border_menu(lines: &[String], start_divider: usize, end_divider: usize) -> Vec<MenuRow> {
    let mut rows = Vec::new();
    for index in (start_divider + 1)..end_divider {
        let mut text = clean_line(&lines[index]);
        if text.is_empty() || is_divider(&text) {
            continue;
        }
        let selected = patterns().selected.is_match(&text);
        text = patterns().selected.replace(&text, "").trim().to_string();
        let checked = patterns().checked_marker.is_match(&text);
        text = patterns().row_marker.replace(&text, "").trim().to_string();
        if !text.is_empty() {
            rows.push(MenuRow { label: normalize_text(&text), selected, checked, line_index: index });
        }
    }
    rows
}

/// `findMenuDividers` (`:191-200`): the last two dividers above the hint, `(start, end)`.
fn find_menu_dividers(lines: &[String], hint_index: usize) -> Option<(usize, usize)> {
    let mut end: Option<usize> = None;
    let mut index = hint_index;
    while index > 0 {
        index -= 1;
        if !is_divider(&lines[index]) {
            continue;
        }
        match end {
            None => end = Some(index),
            Some(end_index) => return Some((index, end_index)),
        }
    }
    None
}

/// `finishPrompt`'s id (`:228-242`): the first 12 hex of a SHA-256 over the pinned payload.
///
/// The payload is `{agent, kind, title, question, body, options, multi_select,
/// custom_option_index}` with the `null`s upstream emits, hashed **before** any display cap.
/// Cursor movement is deliberately absent, so a move on the same menu keeps the id.
#[allow(clippy::too_many_arguments)]
fn finish_prompt_id(
    kind: ReferencePromptKind,
    title: &str,
    question: &str,
    body: Option<&str>,
    options: &[ReferencePromptOption],
    multi_select: bool,
    custom_option_index: Option<u32>,
) -> String {
    let option_values: Vec<Value> = options
        .iter()
        .map(|option| json!({ "label": option.label, "description": Value::Null }))
        .collect();
    let payload = json!({
        "agent": OMP_AGENT_ID,
        "kind": kind_name(kind),
        "title": title,
        "question": question,
        "body": body.map(Value::from).unwrap_or(Value::Null),
        "options": option_values,
        "multi_select": multi_select,
        "custom_option_index": custom_option_index.map(Value::from).unwrap_or(Value::Null),
    });
    let digest = Sha256::digest(payload.to_string().as_bytes());
    let mut hex = String::with_capacity(64);
    for byte in digest.iter() {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex.truncate(12);
    hex
}

/// The wire name of a prompt kind, as the pinned payload spells it.
fn kind_name(kind: ReferencePromptKind) -> &'static str {
    match kind {
        ReferencePromptKind::Question => "question",
        ReferencePromptKind::Approval => "approval",
        ReferencePromptKind::Plan => "plan",
        ReferencePromptKind::Menu => "menu",
    }
}

/// `parseOmpQuestion` (`:263-287`).
fn parse_omp_question(screen: &str) -> Option<OmpPromptCard> {
    let patterns = patterns();
    let lines: Vec<String> = strip_ansi(screen).split('\n').map(str::to_string).collect();

    let hint_index = find_last_index(&lines, |_, index| {
        let hint = wrapped(&lines, index, HINT_SPAN);
        patterns.single_hint.is_match(&hint) || patterns.multi_hint.is_match(&hint)
    })?;
    let (start_divider, end_divider) = find_menu_dividers(&lines, hint_index)?;
    let rows = parse_border_menu(&lines, start_divider, end_divider);
    let selected_index = rows.iter().position(|row| row.selected)?;
    // The `Other (type your own)` row is required, not optional (`:280-281`).
    let custom_index = rows.iter().position(|row| patterns.custom_row.is_match(&row.label))?;
    let option_rows: Vec<&MenuRow> = rows
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != custom_index)
        .map(|(_, row)| row)
        .collect();
    if option_rows.is_empty() {
        return None;
    }
    let multi_select = patterns.multi_hint.is_match(&clean_line(&lines[hint_index]));
    let question = nearest_question(&lines, start_divider)?;

    let options: Vec<ReferencePromptOption> = option_rows
        .iter()
        .map(|row| ReferencePromptOption {
            label: patterns.recommended.replace(&row.label, "").to_string(),
            description: None,
        })
        .collect();
    let title = if multi_select { "Multiple choice" } else { "Question" };
    // A single-select menu's custom row is the option *after* the last option (`:283`);
    // a multiple choice takes no custom row at all.
    let custom_option_index = if multi_select { None } else { Some(option_rows.len() as u32) };
    let id = finish_prompt_id(
        ReferencePromptKind::Question,
        title,
        &question,
        None,
        &options,
        multi_select,
        custom_option_index,
    );

    Some(OmpPromptCard {
        prompt: ReferencePrompt {
            id,
            agent: OMP_AGENT_ID.to_string(),
            kind: ReferencePromptKind::Question,
            title: title.to_string(),
            question,
            body: None,
            options,
            multi_select,
            custom_option_index,
            queued: None,
            steps: Vec::new(),
            fallback: None,
        },
        responder: OmpPromptResponder::Question,
        // a row's label as the card offers it: the pinned `(Recommended)` strip (`:279`) is not
        // the option list's alone, so `menu_labels` and `options` name the row alike
        menu_labels: rows
            .iter()
            .map(|row| patterns.recommended.replace(&row.label, "").to_string())
            .collect(),
        selected_index,
        checked_option_indices: option_rows
            .iter()
            .enumerate()
            .filter(|(_, row)| row.checked)
            .map(|(index, _)| index)
            .collect(),
        custom_menu_index: Some(custom_index),
    })
}

/// `parseOmpApproval` (`:1092-1111`).
fn parse_omp_approval(screen: &str) -> Option<OmpPromptCard> {
    let patterns = patterns();
    let lines: Vec<String> = strip_ansi(screen).split('\n').map(str::to_string).collect();

    let header_index = find_last_index(&lines, |line, _| patterns.approval_header.is_match(&clean_line(line)))?;
    let mut rows: Vec<MenuRow> = Vec::new();
    for index in (header_index + 1)..lines.len() {
        let text = clean_line(&lines[index]);
        let Some(captures) = patterns.approval_row.captures(&text) else {
            continue;
        };
        let label = captures.get(2).map(|value| value.as_str().to_string()).unwrap_or_default();
        let selected = captures.get(1).is_some();
        rows.push(MenuRow { label, selected, checked: false, line_index: index });
    }
    // Exactly two rows, one of them under the cursor (`:1100`).
    if rows.len() != 2 || rows.iter().filter(|row| row.selected).count() != 1 {
        return None;
    }
    let selected_index = rows.iter().position(|row| row.selected)?;
    let header = clean_line(&lines[header_index]);
    let body: String = lines[(header_index + 1)..rows[0].line_index]
        .iter()
        .map(|line| clean_line(line))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let body = if body.is_empty() { None } else { Some(body) };
    let options: Vec<ReferencePromptOption> = rows
        .iter()
        .map(|row| ReferencePromptOption { label: row.label.clone(), description: None })
        .collect();
    let id = finish_prompt_id(
        ReferencePromptKind::Approval,
        &header,
        &header,
        body.as_deref(),
        &options,
        false,
        None,
    );

    Some(OmpPromptCard {
        prompt: ReferencePrompt {
            id,
            agent: OMP_AGENT_ID.to_string(),
            kind: ReferencePromptKind::Approval,
            title: header.clone(),
            question: header,
            // `input.body?.slice(0, 12_000) ?? null` (`:239`); the hash above saw the
            // uncapped text.
            body: body.map(|body| body.chars().take(12_000).collect()),
            options,
            multi_select: false,
            custom_option_index: None,
            queued: None,
            steps: Vec::new(),
            fallback: None,
        },
        responder: OmpPromptResponder::Approval,
        menu_labels: rows.iter().map(|row| row.label.clone()).collect(),
        selected_index,
        checked_option_indices: Vec::new(),
        custom_menu_index: None,
    })
}

/// The omp branches of `promptTailIsActive` (`:1613`, `:1621`).
fn omp_prompt_tail_is_active(responder: OmpPromptResponder, screen: &str) -> bool {
    let patterns = patterns();
    let clean_lines: Vec<String> = strip_ansi(screen).split('\n').map(|line| clean_line(line)).collect();
    let visible: Vec<String> = clean_lines
        .into_iter()
        .filter(|line| !line.is_empty() && !is_divider(line))
        .collect();
    match responder {
        OmpPromptResponder::Question => {
            tail_ends(&visible, &patterns.single_hint) || tail_ends(&visible, &patterns.multi_hint)
        }
        OmpPromptResponder::Approval => tail_ends(&visible, &patterns.approval_tail),
    }
}

/// `promptTailIsActive`'s `ends` (`:1609-1612`): the match must run into the last line, and a
/// hint that ended above later output does not count — that is an answered, stale menu.
fn tail_ends(shown: &[String], pattern: &Regex) -> bool {
    for span in 1..=3usize {
        if shown.len() < span {
            continue;
        }
        let joined = shown[shown.len() - span..].join(" ");
        if !pattern.is_match(&joined) {
            continue;
        }
        if span == 1 {
            return true;
        }
        let before = shown[shown.len() - span..shown.len() - 1].join(" ");
        if !pattern.is_match(&before) {
            return true;
        }
    }
    false
}

/// `navigationKeys` (`:1987-1989`): one move per row of difference.
fn navigation_keys(delta: isize) -> Vec<String> {
    let key = if delta > 0 { KEY_DOWN } else { KEY_UP };
    (0..delta.unsigned_abs()).map(|_| key.to_string()).collect()
}

/// `keySteps` (`:1991-1993`): one named key per step.
fn key_steps(keys: &[String]) -> Vec<ReferenceKeyStep> {
    keys.iter().map(|key| ReferenceKeyStep::keys([key.clone()])).collect()
}

#[cfg(test)]
mod tests {
    use super::super::types::{
        ReferenceKeyStep, ReferencePrompt, ReferencePromptAnswer, ReferencePromptKind,
    };
    use super::*;
    use crate::scoped_contracts::ScopeErrorCode;

    const QUESTION_SINGLE: &str = include_str!("fixtures/omp-prompt/question-single.txt");
    const QUESTION_SINGLE_MOVED: &str = include_str!("fixtures/omp-prompt/question-single-moved.txt");
    const QUESTION_MULTI: &str = include_str!("fixtures/omp-prompt/question-multi.txt");
    const QUESTION_WRAPPED_HINT: &str = include_str!("fixtures/omp-prompt/question-wrapped-hint.txt");
    const QUESTION_NO_CUSTOM_ROW: &str = include_str!("fixtures/omp-prompt/question-no-custom-row.txt");
    const QUESTION_STALE: &str = include_str!("fixtures/omp-prompt/question-stale.txt");
    const APPROVAL: &str = include_str!("fixtures/omp-prompt/approval.txt");
    const APPROVAL_THREE_ROWS: &str = include_str!("fixtures/omp-prompt/approval-three-rows.txt");
    const APPROVAL_STALE: &str = include_str!("fixtures/omp-prompt/approval-stale.txt");
    const NOT_OMP_HINT: &str = include_str!("fixtures/omp-prompt/not-omp-hint.txt");

    fn labels(prompt: &ReferencePrompt) -> Vec<&str> {
        prompt.options.iter().map(|option| option.label.as_str()).collect()
    }

    fn card(screen: &str) -> OmpPromptCard {
        detect_omp_prompt_card(OMP_AGENT_ID, screen).expect("the fixture is an omp prompt")
    }

    fn pick(index: u32) -> ReferencePromptAnswer {
        ReferencePromptAnswer { option_index: Some(index), ..Default::default() }
    }

    fn pick_many(indices: &[u32]) -> ReferencePromptAnswer {
        ReferencePromptAnswer { option_indices: Some(indices.to_vec()), ..Default::default() }
    }

    fn typed(text: &str) -> ReferencePromptAnswer {
        ReferencePromptAnswer { custom_text: Some(text.to_string()), ..Default::default() }
    }

    #[test]
    fn omp_question_parses_the_single_select_menu() {
        let card = card(QUESTION_SINGLE);
        assert_eq!(card.responder, OmpPromptResponder::Question);
        assert_eq!(card.responder.as_str(), "omp-question");
        let prompt = &card.prompt;
        assert_eq!(prompt.agent, "omp");
        assert_eq!(prompt.kind, ReferencePromptKind::Question);
        assert_eq!(prompt.title, "Question");
        assert_eq!(prompt.question, "Which file should I open?");
        assert_eq!(prompt.body, None);
        // `(Recommended)` is stripped from the label; the custom row is not an option.
        assert_eq!(labels(prompt), vec!["src/main.rs", "ui/src/App.tsx", "crates/core/src/lib.rs"]);
        assert!(!prompt.multi_select);
        // The custom row's index is the option count, so it never names a real option.
        assert_eq!(prompt.custom_option_index, Some(3));
        assert_eq!(card.selected_index, 0);
        assert_eq!(card.custom_menu_index, Some(3));
        assert_eq!(card.menu_labels, vec!["src/main.rs", "ui/src/App.tsx", "crates/core/src/lib.rs", "Other (type your own)"]);
        assert!(card.checked_option_indices.is_empty());
        assert_eq!(prompt.queued, None);
        assert!(prompt.steps.is_empty());
        assert_eq!(prompt.fallback, None);
        assert_eq!(prompt.id.len(), 12, "the pinned id is 12 hex characters");
    }

    #[test]
    fn omp_question_answers_from_the_row_the_cursor_is_on() {
        let single = card(QUESTION_SINGLE);
        // The cursor sits on row 0, so row 0 is the cursor's own row: no move.
        assert_eq!(single.answer_steps(&pick(0)).unwrap(), vec![ReferenceKeyStep::keys([KEY_ENTER])]);
        // Row 2 takes two moves down.
        assert_eq!(
            single.answer_steps(&pick(2)).unwrap(),
            vec![
                ReferenceKeyStep::keys([KEY_DOWN]),
                ReferenceKeyStep::keys([KEY_DOWN]),
                ReferenceKeyStep::keys([KEY_ENTER]),
            ]
        );

        // The same menu with the cursor already on row 2 answers row 0 with two ups.
        let moved = card(QUESTION_SINGLE_MOVED);
        assert_eq!(moved.selected_index, 2);
        assert_eq!(labels(&moved.prompt), labels(&single.prompt));
        assert_eq!(
            moved.answer_steps(&pick(0)).unwrap(),
            vec![
                ReferenceKeyStep::keys([KEY_UP]),
                ReferenceKeyStep::keys([KEY_UP]),
                ReferenceKeyStep::keys([KEY_ENTER]),
            ]
        );
        assert_eq!(moved.answer_steps(&pick(2)).unwrap(), vec![ReferenceKeyStep::keys([KEY_ENTER])]);
    }

    #[test]
    fn omp_question_custom_answer_moves_to_the_other_row_then_types() {
        let card = card(QUESTION_SINGLE);
        let steps = card.answer_steps(&typed("crates/core/src/lib.rs")).unwrap();
        assert_eq!(
            steps,
            vec![
                ReferenceKeyStep::keys([KEY_DOWN]),
                ReferenceKeyStep::keys([KEY_DOWN]),
                ReferenceKeyStep::keys([KEY_DOWN]),
                ReferenceKeyStep::keys([KEY_ENTER]),
                ReferenceKeyStep::typed("crates/core/src/lib.rs"),
                ReferenceKeyStep::keys([KEY_ENTER]),
            ]
        );
        // The pinned path trims the text before it is typed (`:2003`).
        let steps = card.answer_steps(&typed("  src/main.rs  ")).unwrap();
        assert_eq!(steps.last().unwrap(), &ReferenceKeyStep::keys([KEY_ENTER]));
        assert_eq!(steps[steps.len() - 2], ReferenceKeyStep::typed("src/main.rs"));
    }

    #[test]
    fn omp_question_multi_select_toggles_the_differing_rows_with_space() {
        let card = card(QUESTION_MULTI);
        let prompt = &card.prompt;
        assert_eq!(prompt.title, "Multiple choice");
        assert!(prompt.multi_select);
        assert_eq!(prompt.custom_option_index, None, "a multiple choice takes no custom row");
        assert_eq!(labels(prompt), vec!["Lint", "Unit tests", "Typecheck"]);
        assert_eq!(card.selected_index, 1);
        assert_eq!(card.checked_option_indices, vec![0, 2]);

        // Everything already wanted: nothing to toggle, leave with tab, enter.
        assert_eq!(
            card.answer_steps(&pick_many(&[0, 2])).unwrap(),
            vec![ReferenceKeyStep::keys([KEY_TAB]), ReferenceKeyStep::keys([KEY_ENTER])]
        );
        // `[0]` asks for row 2 to come off: move down to it, toggle, then leave.
        assert_eq!(
            card.answer_steps(&pick_many(&[0])).unwrap(),
            vec![
                ReferenceKeyStep::keys([KEY_DOWN]),
                ReferenceKeyStep::keys([KEY_SPACE]),
                ReferenceKeyStep::keys([KEY_TAB]),
                ReferenceKeyStep::keys([KEY_ENTER]),
            ]
        );
        // `[1]` alone: rows 0 and 2 come off and row 1 goes on, so three toggles walk out
        // from the cursor and back.
        assert_eq!(
            card.answer_steps(&pick_many(&[1])).unwrap(),
            vec![
                ReferenceKeyStep::keys([KEY_UP]),
                ReferenceKeyStep::keys([KEY_SPACE]),
                ReferenceKeyStep::keys([KEY_DOWN]),
                ReferenceKeyStep::keys([KEY_SPACE]),
                ReferenceKeyStep::keys([KEY_DOWN]),
                ReferenceKeyStep::keys([KEY_SPACE]),
                ReferenceKeyStep::keys([KEY_TAB]),
                ReferenceKeyStep::keys([KEY_ENTER]),
            ]
        );
        // Duplicates collapse, exactly as the pinned `new Set` does (`:2027`).
        assert_eq!(
            card.answer_steps(&pick_many(&[0, 2, 2])).unwrap(),
            vec![ReferenceKeyStep::keys([KEY_TAB]), ReferenceKeyStep::keys([KEY_ENTER])]
        );
    }

    #[test]
    fn omp_question_hint_wrapped_over_two_lines_is_still_found() {
        let card = card(QUESTION_WRAPPED_HINT);
        assert_eq!(card.prompt.question, "Which file should I open?");
        assert_eq!(labels(&card.prompt), vec!["src/main.rs"]);
        assert_eq!(card.prompt.custom_option_index, Some(1));
        assert_eq!(card.selected_index, 0);
        assert!(!card.prompt.multi_select);
    }

    #[test]
    fn omp_approval_parses_the_two_row_menu() {
        let card = card(APPROVAL);
        assert_eq!(card.responder, OmpPromptResponder::Approval);
        assert_eq!(card.responder.as_str(), "omp-approval");
        let prompt = &card.prompt;
        assert_eq!(prompt.agent, "omp");
        assert_eq!(prompt.kind, ReferencePromptKind::Approval);
        assert_eq!(prompt.title, "Allow tool: Bash");
        assert_eq!(prompt.question, "Allow tool: Bash");
        assert_eq!(prompt.body.as_deref(), Some("Command: git push --force origin main"));
        assert_eq!(labels(prompt), vec!["Approve", "Deny"]);
        assert!(!prompt.multi_select);
        assert_eq!(prompt.custom_option_index, None);
        assert_eq!(card.selected_index, 0);
        assert!(card.custom_menu_index.is_none());
        assert_eq!(card.answer_steps(&pick(1)).unwrap(), vec![
            ReferenceKeyStep::keys([KEY_DOWN]),
            ReferenceKeyStep::keys([KEY_ENTER]),
        ]);
        assert_eq!(card.answer_steps(&pick(0)).unwrap(), vec![ReferenceKeyStep::keys([KEY_ENTER])]);
        // An approval's typed answer is not a custom row: refused, never typed into the pane.
        assert_eq!(card.answer_steps(&typed("yes")).unwrap_err(), ScopeErrorCode::Unsupported);
    }

    #[test]
    fn omp_approval_requires_exactly_two_rows() {
        assert!(
            detect_omp_prompt_card(OMP_AGENT_ID, APPROVAL_THREE_ROWS).is_none(),
            "three Approve/Deny rows are not the pinned two-row menu"
        );
    }

    #[test]
    fn omp_question_requires_the_custom_row() {
        assert!(
            detect_omp_prompt_card(OMP_AGENT_ID, QUESTION_NO_CUSTOM_ROW).is_none(),
            "a menu without `Other (type your own)` is not an omp question"
        );
    }

    #[test]
    fn a_menu_scrolled_away_under_output_is_not_a_prompt() {
        assert!(detect_omp_prompt_card(OMP_AGENT_ID, QUESTION_STALE).is_none());
        assert!(detect_omp_prompt_card(OMP_AGENT_ID, APPROVAL_STALE).is_none());
    }

    #[test]
    fn an_omp_screen_is_never_claimed_for_another_agent() {
        for agent in ["claude", "codex", "omo", "pi", "gjc", "opencode", ""] {
            assert!(
                detect_omp_prompt(agent, QUESTION_SINGLE).is_none(),
                "{agent} must not be answered by the omp family"
            );
            assert!(detect_omp_prompt(agent, APPROVAL).is_none());
        }
        // A Claude-shaped menu is not an omp one either, even under the omp id.
        assert!(detect_omp_prompt_card(OMP_AGENT_ID, NOT_OMP_HINT).is_none());
    }

    #[test]
    fn a_changed_menu_text_changes_the_id_and_a_moved_cursor_does_not() {
        let single = card(QUESTION_SINGLE);
        let moved = card(QUESTION_SINGLE_MOVED);
        assert_eq!(single.prompt.id, moved.prompt.id, "cursor movement is not hashed");

        let other_text = QUESTION_SINGLE.replace("Which file should I open?", "Which test should I run?");
        let other = card(&other_text);
        assert_ne!(single.prompt.id, other.prompt.id, "a different question is a different card");

        let other_option = QUESTION_SINGLE.replace("ui/src/App.tsx", "ui/src/Main.tsx");
        let other = card(&other_option);
        assert_ne!(single.prompt.id, other.prompt.id, "a different option is a different card");

        // The approval's body is part of the id, so a different command is a different card.
        let approval = card(APPROVAL);
        let other_body = APPROVAL.replace("git push --force origin main", "git push --force upstream main");
        assert_ne!(approval.prompt.id, card(&other_body).prompt.id);
    }

    #[test]
    fn the_planner_refuses_an_unparsed_prompt_and_an_ambiguous_answer() {
        let single = card(QUESTION_SINGLE);
        // A prompt this lane did not detect: no internal state, no keys.
        let mut foreign = single.prompt.clone();
        foreign.id = "0123456789ab".to_string();
        assert_eq!(reference_answer_keys(&foreign, &pick(0)).unwrap_err(), ScopeErrorCode::Unsupported);
        // A prompt whose id matches but whose text does not is not the card we hold.
        let mut edited = single.prompt.clone();
        edited.question = "Something else entirely".to_string();
        assert_eq!(reference_answer_keys(&edited, &pick(0)).unwrap_err(), ScopeErrorCode::Unsupported);
        // The card itself plans: row 0 is the cursor's own row, so a single enter.
        assert_eq!(
            reference_answer_keys(&single.prompt, &pick(0)).unwrap(),
            vec![ReferenceKeyStep::keys([KEY_ENTER])]
        );

        // Two answer shapes at once, and none at all.
        let ambiguous = ReferencePromptAnswer {
            option_index: Some(0),
            option_indices: Some(vec![0]),
            custom_text: None,
        };
        assert_eq!(single.answer_steps(&ambiguous).unwrap_err(), ScopeErrorCode::InvalidRequest);
        assert_eq!(
            single.answer_steps(&ReferencePromptAnswer::default()).unwrap_err(),
            ScopeErrorCode::InvalidRequest
        );
        // An index outside the displayed range (the custom row's own index is one of them).
        assert_eq!(single.answer_steps(&pick(9)).unwrap_err(), ScopeErrorCode::InvalidRequest);
        assert_eq!(single.answer_steps(&pick(2)).unwrap().len(), 3);
        // A custom answer with only whitespace is refused, not typed.
        assert_eq!(single.answer_steps(&typed("   ")).unwrap_err(), ScopeErrorCode::Unsupported);
        // Option indices on a single-select menu, and a custom answer on a multiple choice.
        assert_eq!(single.answer_steps(&pick_many(&[0])).unwrap_err(), ScopeErrorCode::InvalidRequest);
        let multi = card(QUESTION_MULTI);
        assert_eq!(multi.answer_steps(&typed("lint")).unwrap_err(), ScopeErrorCode::Unsupported);
        assert_eq!(multi.answer_steps(&pick_many(&[])).unwrap_err(), ScopeErrorCode::InvalidRequest);
        assert_eq!(multi.answer_steps(&pick_many(&[0, 7])).unwrap_err(), ScopeErrorCode::InvalidRequest);
        // A multiple choice's options are not single picks.
        assert_eq!(multi.answer_steps(&pick(0)).unwrap_err(), ScopeErrorCode::InvalidRequest);
    }

    #[test]
    fn detect_registers_the_card_the_planner_answers_with() {
        let prompt = detect_omp_prompt(OMP_AGENT_ID, QUESTION_SINGLE).expect("an omp prompt");
        let card = omp_prompt_card(&prompt.id).expect("detection remembers its card");
        assert_eq!(card.prompt, prompt);
        assert_eq!(
            reference_answer_keys(&prompt, &pick(0)).unwrap(),
            vec![ReferenceKeyStep::keys([KEY_ENTER])]
        );
        assert_eq!(OMP_PROMPT_DETECTOR(OMP_AGENT_ID, QUESTION_SINGLE).map(|prompt| prompt.id), Some(prompt.id));
    }
}
