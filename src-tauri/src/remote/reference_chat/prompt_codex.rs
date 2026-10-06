//! Codex prompt family for the Herdr reference-chat port.
//!
//! Ported from `devswha/herdr-web-ui` @ `54e5a1f67090cb09552d182e7e30dd0ecc314918`
//! (MIT, `docs/chat/HERDR_LICENSE`), the Codex half of the pinned screen reader
//! `server/prompt.ts`:
//!
//! * `parseCodexContinueMenu` (`:288-305`) — responder `codex-menu`.
//! * `parseCodexQuestion` (`:307-332`) — responder `codex-question`.
//! * `parseCodexAsyncQuestion` (`:340-388`) — responder `codex-async-question`.
//! * `parseCodexApproval` (`:1071-1090`) — responder `codex-approval`.
//! * `parseCodexModel` (`:1474-1522`) with `listRow` (`:1296`), `listNames` (`:1318`),
//!   `listRows` (`:1339`), `codexModelHeader` (`:1427`), `codexModelRowKey` (`:1443`) and
//!   `codexModelListWaits` (`:1455`) — responder `codex-model`.
//! * `queuedQuestionCount` (`:399-409`), `queuedPrompt` (`:418-434`), `codexQuestionsCollapsed`
//!   (`:1963-1965`) and `codexQueuedPrompt` (`:1968-1971`) — responder
//!   `codex-queued-question`, whose inputs are the rollout's unanswered questions
//!   (`server/codex.ts:463-464`, `QueuedQuestion`).
//! * the shared readers the branches above call: `cleanLine` (`:135`), `isDivider` (`:142`),
//!   `normalizeText` (`:147`), `findLastIndex` (`:151`), `wrapped` (`:163`),
//!   `nearestQuestion` (`:167`), `parseNumberedRows` (`:201`), `sequentialRows` (`:224`),
//!   `finishPrompt` (`:228`), `promptTailIsActive` (`:1603`, its Codex arms),
//!   `navigationKeys` (`:1987`), `keySteps` (`:1991`), `answerKeys` (`:1995`, its Codex arms)
//!   and `sameText` (`:2525`).
//!
//! Upstream SHA-256: `docs/chat/herdr-port-contract.md` §7 — `server/prompt.ts` =
//! `083a74a29015258f7c1e11016c4f520cf265fb2ca89013feede71c2e030dba58`, `server/codex.ts` =
//! `eedfd59a54ccf0935d0fc0a658ceb457af8953e9a9dc99e60a181d45c659114a`. Responder inventory,
//! fixture map and the deliberate divergences: `fixtures/codex/MANIFEST.md`.
//!
//! **Scope.** This lane ports the six named Codex responders and nothing else. `agent` is the
//! registry id: a screen handed to this family under any other id is refused rather than
//! parsed, because the reference dispatches by agent and a detector must not claim a provider
//! it was not given (contract §6, "no invented support for other providers"). The pinned
//! `fallback-menu` / `fallback-keys` cards (`parseFallbackPrompt`, `:2122-2178`) are the
//! dispatcher's last resort (task 9), not a Codex responder: this lane ports neither, and the
//! `codexModelListWaits` predicate it does port exists only so task 9 can suppress that card
//! over a model list whose Enter would save a default.
//!
//! **Boundaries the frozen DTOs cannot carry.** The reference resolves an answer through
//! `parsedByPublicPrompt`, a `WeakMap` from the public prompt object to the internal parse
//! (`selectedIndex`, `customMenuIndex`, `rejectWithEscapeIndex`, `optionSteps`, `rowKey`).
//! Those five fields are not on `InteractivePrompt`, so
//! [`ReferenceAnswerPlanner`](super::types::ReferenceAnswerPlanner) — whose only input is the
//! public `ReferencePrompt` — cannot answer faithfully for this family without guessing the
//! cursor. This lane therefore returns [`ReferenceCodexPrompt`] (the public prompt plus the
//! internal view) from [`parse_reference_codex_prompt`], plans from it with
//! [`plan_reference_codex_answer`], and satisfies the frozen
//! [`ReferencePromptDetector`] alias with [`detect_reference_codex_prompt`], which is the
//! signature the contract names for the detector lanes (§6). A `pick` step
//! ([`ReferenceCodexAnswerStep::Pick`]) is likewise not representable in `ReferenceKeyStep`:
//! the reference reads the row's own key off the screen *after* its moves, so
//! [`codex_model_row_key`] is exposed for the executor to call on its fresh read and
//! [`reference_key_steps`] refuses to project an unresolved `pick` rather than guess one.

use std::sync::OnceLock;

use regex::Regex;
use sha2::{Digest, Sha256};

use super::types::{
    ReferenceKeyStep, ReferencePrompt, ReferencePromptAnswer, ReferencePromptDetector,
    ReferencePromptKind, ReferencePromptOption, ReferencePromptQueueState,
};

/// The registry id this family answers to.
pub const REFERENCE_CODEX_AGENT: &str = "codex";

/// Every named Codex responder in the pinned union (`server/prompt.ts:83-107`), in the order
/// the pinned `parsePrompt` tries their parsers (`:1936-1938`), with the queue card last
/// because it is not in that candidate list: it is built from the rollout, not the screen.
pub const REFERENCE_CODEX_RESPONDERS: [ReferenceCodexResponder; 6] = [
    ReferenceCodexResponder::Menu,
    ReferenceCodexResponder::Question,
    ReferenceCodexResponder::AsyncQuestion,
    ReferenceCodexResponder::Approval,
    ReferenceCodexResponder::Model,
    ReferenceCodexResponder::QueuedQuestion,
];

/// The pinned `finishPrompt` display cap: a body past this is cut in the card (the id hashes
/// the whole body first, so a change beyond the cap is still a different prompt).
const REFERENCE_CODEX_BODY_CAP: usize = 12_000;

/// Upstream `KEY` (`server/prompt.ts:67-82`), the names herdr sends for a key press.
const KEY_UP: &str = "up";
const KEY_DOWN: &str = "down";
const KEY_ENTER: &str = "enter";
const KEY_ESC: &str = "esc";
const KEY_TAB: &str = "tab";

/// Upstream `CODEX_MODEL_TAIL_LINES`: how far back a list footer is looked for.
const CODEX_MODEL_TAIL_LINES: usize = 30;

/// Upstream `CODEX_MODEL_NOTE_LINES`: the lines Codex puts under a list's title, at most.
const CODEX_MODEL_NOTE_LINES: usize = 2;

/// Which of the pinned Codex branches read this screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceCodexResponder {
    /// `codex-menu` — the "press enter to continue" menu Codex draws between turns.
    Menu,
    /// `codex-question` — a numbered question whose last row takes a typed answer.
    Question,
    /// `codex-async-question` — a question from the queue, open on the screen.
    AsyncQuestion,
    /// `codex-approval` — a tool or folder approval.
    Approval,
    /// `codex-model` — one of Codex's `/model` lists.
    Model,
    /// `codex-queued-question` — the card for the collapsed queue, built from the rollout.
    QueuedQuestion,
}

impl ReferenceCodexResponder {
    /// The pinned responder name, as the reference's `Responder` union spells it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Menu => "codex-menu",
            Self::Question => "codex-question",
            Self::AsyncQuestion => "codex-async-question",
            Self::Approval => "codex-approval",
            Self::Model => "codex-model",
            Self::QueuedQuestion => "codex-queued-question",
        }
    }
}

/// One step of a Codex answer.
///
/// The pinned `AnswerStep` is `{ keys?, text?, pick? }`; `pick` is resolved by the answer
/// executor from the screen it reads *after* the moves, so it is its own variant here rather
/// than a `ReferenceKeyStep` with nothing in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReferenceCodexAnswerStep {
    /// Press these named keys.
    Keys(Vec<String>),
    /// Type this literal text.
    Text(String),
    /// Take the row's own key off the screen once the cursor is there
    /// ([`codex_model_row_key`]).
    Pick,
}

/// A Codex question Codex queued with `request_user_input_async`
/// (upstream `QueuedQuestion`, `server/codex.ts:464`). `options` is empty for a free-form one.
///
/// The rollout scan that produces these is not this lane's: it reads a file
/// (`unansweredCodexQuestions`, `codex.ts:477`), and the reference resolves the pane's rollout
/// path elsewhere (`codexTranscriptPath`). Whoever resolves it (the resolver, task 3) hands the
/// questions in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceCodexQueuedQuestion {
    /// Upstream `key`: `"<call_id>:<index>"`.
    pub key: String,
    pub title: String,
    pub options: Vec<String>,
}

/// The question a pane's queue opened on last time, as it showed there (upstream `QueueFront`,
/// `prompt.ts:412`). The collapsed queue shows only a count, so a skip can leave the rollout's
/// guess one behind the question the pane is actually on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceCodexQueueFront {
    pub question: String,
    pub options: Vec<String>,
}

/// A detected Codex prompt: the frozen public card, plus the internal view the reference keeps
/// beside it in `parsedByPublicPrompt` and that an answer needs.
#[derive(Debug, Clone, PartialEq)]
pub struct ReferenceCodexPrompt {
    /// The card as the contract's `ReferencePrompt` carries it.
    pub prompt: ReferencePrompt,
    pub responder: ReferenceCodexResponder,
    /// The row the cursor is on, as the screen showed it.
    pub selected_index: usize,
    /// The index of the "type your own answer" row, when the menu has one.
    pub custom_menu_index: Option<usize>,
    /// The row that means "back out": answered with Escape, never with Enter.
    pub reject_with_escape_index: Option<usize>,
    /// Each option's own steps, for a card whose options are not rows of a numbered menu.
    pub option_steps: Option<Vec<Vec<ReferenceCodexAnswerStep>>>,
    /// The key the menu itself names for the row under the cursor, for a card whose rows do not
    /// all take the same one. `Some(Pick)`-free: this is the *resolved* value the reference
    /// reads off the screen, so it is `None` where the reference's `rowKey` is `null`.
    pub row_key: Option<ReferenceKeyStep>,
}

/// The lane's detector, satisfying task 1's [`ReferencePromptDetector`] alias. The dispatcher
/// (task 9) routes by agent; a screen handed here under another id is refused.
pub fn detect_reference_codex_prompt(agent: &str, screen: &str) -> Option<ReferencePrompt> {
    if agent != REFERENCE_CODEX_AGENT {
        return None;
    }
    parse_reference_codex_prompt(screen).map(|parsed| parsed.prompt)
}

/// The lane's detector under the name the dispatcher reaches for, proving the frozen
/// [`ReferencePromptDetector`] signature at compile time (contract §6).
///
/// The contract's other lane alias, [`ReferenceAnswerPlanner`](super::types::ReferenceAnswerPlanner),
/// is deliberately **not** satisfied by a stateless function here: its only input is the public
/// `ReferencePrompt`, which does not carry the cursor, so an answer planned from it alone would
/// be a guess. The dispatcher plans through [`parse_reference_codex_prompt`] →
/// [`plan_reference_codex_answer`] → [`reference_key_steps`] instead, and resolves a model
/// list's [`ReferenceCodexAnswerStep::Pick`] with [`codex_model_row_key`] on its fresh read.
/// The boundary is recorded in `fixtures/codex/MANIFEST.md`.
pub const CODEX_PROMPT_DETECTOR: ReferencePromptDetector = detect_reference_codex_prompt;

/// Upstream `parsePrompt("codex", screen)` (`:1936-1938`): the first candidate parser that
/// matches and whose hint still owns the end of the screen.
///
/// The order is the reference's own, and it matters: a continue menu is tried before a question,
/// and both before an approval.
pub fn parse_reference_codex_prompt(screen: &str) -> Option<ReferenceCodexPrompt> {
    let candidates = [
        parse_codex_continue_menu(screen),
        parse_codex_question(screen),
        parse_codex_async_question(screen),
        parse_codex_approval(screen),
        parse_codex_model(screen),
    ];
    candidates
        .into_iter()
        .flatten()
        .find(|parsed| prompt_tail_is_active(parsed, screen))
}

/// Upstream `codexQuestionsCollapsed` (`:1963-1965`): the queue is collapsed and the main
/// prompt, not a question, holds the input.
///
/// True only for that collapsed queue with the main prompt right under it and no other prompt on
/// screen: an open question or an approval below the queue holds the input itself.
pub fn codex_questions_collapsed(screen: &str) -> bool {
    parse_reference_codex_prompt(screen).is_none() && queued_question_count(screen) > 0
}

/// Upstream `codexQueuedPrompt` (`:1968-1971`): the card for the collapsed queue, from the
/// rollout's unanswered questions.
pub fn codex_queued_prompt(
    screen: &str,
    unanswered: &[ReferenceCodexQueuedQuestion],
    front: Option<&ReferenceCodexQueueFront>,
) -> Option<ReferenceCodexPrompt> {
    let count = queued_question_count(screen);
    if count == 0 {
        return None;
    }
    queued_prompt(count, unanswered, front)
}

/// Upstream `queuedQuestionCount` (`:399-409`): how many questions wait in the collapsed queue
/// at the bottom of the screen, with the main prompt right under it; 0 otherwise.
///
/// * A message of the user's own waiting to be submitted replaces the questions' block
///   (alt+↑ then opens nothing, checked on Codex 0.156.1): no count then.
/// * An open question (its "enter submit … skip" hint) or an approval under the queue holds the
///   input itself: no count either.
pub fn queued_question_count(screen: &str) -> usize {
    let lines: Vec<String> = split_lines(screen)
        .into_iter()
        .map(|line| clean_line(&line))
        .filter(|line| !line.is_empty())
        .collect();
    let Some(header) = find_last_index(&lines, |line, _| re_queue_header().is_match(line)) else {
        return 0;
    };
    // the queue sits right above the main prompt and its status line
    if lines.len() - header > 16 {
        return 0;
    }
    if lines[header..].iter().any(|line| {
        re_queue_arrow().is_match(line)
            || re_messages_to_be_submitted().is_match(line)
            || re_codex_async_ask_hint().is_match(line)
    }) {
        return 0;
    }
    let Some(at) = lines
        .iter()
        .enumerate()
        .find(|(index, line)| {
            *index > header && *index <= header + 7 && re_queue_count().is_match(line.as_str())
        })
        .map(|(index, _)| index)
    else {
        return 0;
    };
    if !re_to_answer().is_match(lines.get(at + 1).map(String::as_str).unwrap_or("")) {
        return 0;
    }
    if !is_queue_main_prompt(lines.get(at + 2).map(String::as_str).unwrap_or("")) {
        return 0;
    }
    re_queue_count()
        .captures(lines.get(at).map(String::as_str).unwrap_or(""))
        .and_then(|captures| captures.get(1))
        .and_then(|count| count.as_str().parse::<usize>().ok())
        .unwrap_or(0)
}

/// Upstream `codexModelListWaits` (`:1455-1472`): whether a Codex model list holds the end of
/// the screen, read or not.
///
/// Such a list gets no fallback card while herdr happens to report the pane blocked: that card
/// offers Enter, and Enter under a footer that offers `s` saves the row as the default for every
/// new session. Task 9 consumes this; nothing in this lane acts on it.
pub fn codex_model_list_waits(screen: &str) -> bool {
    let lines: Vec<String> = split_lines(screen)
        .into_iter()
        .map(|line| line.trim_end().to_string())
        .collect();
    let shown: Vec<(String, usize)> = lines
        .iter()
        .enumerate()
        .map(|(index, line)| (clean_line(line), index))
        .filter(|(text, _)| !text.is_empty() && !is_divider(text))
        .collect();
    let shown = &shown[shown.len().saturating_sub(CODEX_MODEL_TAIL_LINES)..];
    for start in 0..shown.len() {
        // Join a footer split inside words by a very narrow pane. Anything printed after it
        // prevents the match, so an old title/footer in the transcript cannot hide a new prompt.
        let footer: String = shown[start..].iter().map(|(text, _)| text.as_str()).collect();
        let footer = re_whitespace_run().replace_all(&footer, "").to_string();
        if re_model_picks_flat().is_match(&footer) {
            return true;
        }
        if !re_model_guard_hint().is_match(&footer) {
            continue;
        }
        // The generic list footer needs a model header in the block immediately above its rows.
        // Quick presets are recognized only by this guard, never offered as a readable card.
        let listed = list_rows(&lines, shown[start].1, re_model_row(), None);
        let intervening = !listed.rows.is_empty()
            && lines[listed.last + 1..shown[start].1]
                .iter()
                .any(|line| !clean_line(line).is_empty() && !is_divider(&clean_line(line)));
        if !listed.rows.is_empty()
            && !intervening
            && codex_model_header(&lines, listed.first, true).is_some()
        {
            return true;
        }
    }
    false
}

/// Upstream `codexModelRowKey` (`:1443-1446`): the key a Codex list's footer names for picking
/// the row under the cursor; `None` for any other footer.
///
/// This is the `rowKey` the answer executor reads off the screen after its moves; it is a
/// function of the screen alone, so the executor calls it on its fresh read.
pub fn codex_model_row_key(footer: &str) -> Option<ReferenceKeyStep> {
    if re_model_pick_hint().is_match(footer) {
        // `s`, never Enter: the pick stays in this session, and the default for new ones is
        // left alone.
        return Some(ReferenceKeyStep::typed("s"));
    }
    if re_model_open_hint().is_match(footer) {
        return Some(ReferenceKeyStep::keys([KEY_ENTER]));
    }
    None
}

/// Upstream `answerKeys` (`:1995-2104`) for the Codex responders.
///
/// Returns the steps the executor sends in order. A [`ReferenceCodexAnswerStep::Pick`] is left
/// unresolved: the reference reads the row's own key off the screen it sees after the moves.
pub fn plan_reference_codex_answer(
    parsed: &ReferenceCodexPrompt,
    answer: &ReferencePromptAnswer,
) -> Result<Vec<ReferenceCodexAnswerStep>, String> {
    if answer.variant_count() != 1 {
        return Err("Exactly one answer is required.".to_string());
    }

    if let Some(custom_text) = &answer.custom_text {
        let text = custom_text.trim();
        let Some(custom_menu_index) = parsed.custom_menu_index else {
            return Err("This prompt does not accept a custom answer.".to_string());
        };
        if text.is_empty() || parsed.prompt.multi_select {
            return Err("This prompt does not accept a custom answer.".to_string());
        }
        let mut navigation = navigation_keys(custom_menu_index as i64 - parsed.selected_index as i64);
        // Codex's queue types into its last row once it is selected: no enter first.
        if !matches!(
            parsed.responder,
            ReferenceCodexResponder::Question | ReferenceCodexResponder::AsyncQuestion
        ) {
            navigation.push(KEY_ENTER.to_string());
        }
        if parsed.responder == ReferenceCodexResponder::Question {
            navigation.push(KEY_TAB.to_string());
        }
        let mut steps = key_steps(navigation);
        steps.push(ReferenceCodexAnswerStep::Text(text.to_string()));
        steps.extend(key_steps(vec![KEY_ENTER.to_string()]));
        return Ok(steps);
    }

    if let Some(option_indices) = &answer.option_indices {
        if !parsed.prompt.multi_select || option_indices.is_empty() {
            return Err("This prompt requires one or more selections.".to_string());
        }
        // Verbatim port: no Codex branch sets `multi_select`, so this arm is unreachable for
        // this family. It is kept so a screen that does set it is refused rather than
        // answered by a guessed toggle sequence.
        return Err("This agent does not support multiple selections.".to_string());
    }

    let Some(index) = answer.option_index else {
        return Err("A valid option index is required.".to_string());
    };
    let index = index as usize;
    if index >= parsed.prompt.options.len()
        || Some(index as u32) == parsed.prompt.custom_option_index
        || parsed.prompt.multi_select
    {
        return Err("A valid option index is required.".to_string());
    }
    if let Some(option_steps) = &parsed.option_steps {
        return Ok(option_steps[index].clone());
    }
    if parsed.reject_with_escape_index == Some(index) {
        return Ok(key_steps(vec![KEY_ESC.to_string()]));
    }
    let mut keys = navigation_keys(index as i64 - parsed.selected_index as i64);
    keys.push(KEY_ENTER.to_string());
    Ok(key_steps(keys))
}

/// Project planned steps onto the frozen wire DTO.
///
/// A [`ReferenceCodexAnswerStep::Pick`] has no `ReferenceKeyStep` form — it is not a key press
/// but a read — so it is refused here rather than dropped: an executor that reaches one must
/// resolve it with [`codex_model_row_key`] on its fresh screen read first.
pub fn reference_key_steps(
    steps: &[ReferenceCodexAnswerStep],
) -> Result<Vec<ReferenceKeyStep>, String> {
    steps
        .iter()
        .map(|step| match step {
            ReferenceCodexAnswerStep::Keys(keys) => Ok(ReferenceKeyStep::keys(keys.clone())),
            ReferenceCodexAnswerStep::Text(text) => Ok(ReferenceKeyStep::typed(text.clone())),
            ReferenceCodexAnswerStep::Pick => Err(
                "a model list's key is the screen's to name: read it with codex_model_row_key"
                    .to_string(),
            ),
        })
        .collect()
}

/// Upstream `parseCodexContinueMenu` (`:288-305`).
pub fn parse_codex_continue_menu(screen: &str) -> Option<ReferenceCodexPrompt> {
    let lines = split_lines(screen);
    let hint_index = find_last_index(&lines, |_, index| {
        re_codex_continue_hint().is_match(&wrapped(&lines, index, 3))
    })?;
    let rows = parse_numbered_rows(&lines, hint_index.saturating_sub(64), hint_index);
    if !sequential_rows(&rows)
        || rows.len() < 2
        || rows.iter().filter(|row| row.selected).count() != 1
    {
        return None;
    }
    let body: Vec<String> = lines[rows[0].line_index.saturating_sub(16)..rows[0].line_index]
        .iter()
        .map(|line| clean_line(line))
        .filter(|line| !line.is_empty() && !is_divider(line))
        .collect();
    let body = join_or_none(&body, "\n");
    let options: Vec<ReferencePromptOption> = rows
        .iter()
        .map(|row| ReferencePromptOption {
            label: row.label.clone(),
            description: row.description.clone(),
        })
        .collect();
    Some(finish_codex_prompt(FinishCodexPrompt {
        kind: ReferencePromptKind::Menu,
        title: "Codex".to_string(),
        question: "Choose how to continue".to_string(),
        body,
        options,
        multi_select: false,
        custom_option_index: None,
        queued: None,
        responder: ReferenceCodexResponder::Menu,
        selected_index: rows.iter().position(|row| row.selected).unwrap_or(0),
        custom_menu_index: None,
        reject_with_escape_index: None,
        option_steps: None,
        row_key: None,
    }))
}

/// Upstream `parseCodexQuestion` (`:307-332`).
pub fn parse_codex_question(screen: &str) -> Option<ReferenceCodexPrompt> {
    let lines = split_lines(screen);
    let hint_index = find_last_index(&lines, |_, index| {
        re_codex_ask_hint().is_match(&wrapped(&lines, index, 3))
    })?;
    let rows = parse_numbered_rows(&lines, hint_index.saturating_sub(48), hint_index);
    if !sequential_rows(&rows) || rows.iter().filter(|row| row.selected).count() != 1 {
        return None;
    }
    let custom_index = rows
        .iter()
        .position(|row| re_none_of_the_above().is_match(&row.label))?;
    if custom_index != rows.len() - 1 || custom_index < 1 {
        return None;
    }
    let question = nearest_question(&lines, rows[0].line_index)?;
    let options: Vec<ReferencePromptOption> = rows[..custom_index]
        .iter()
        .map(|row| {
            let mut parts = re_wide_gap().split(&row.label);
            let label = parts.next().unwrap_or("").to_string();
            let description = parts.collect::<Vec<_>>().join(" ");
            ReferencePromptOption {
                label,
                description: if description.is_empty() {
                    None
                } else {
                    Some(description)
                },
            }
        })
        .collect();
    let progress = lines[rows[0].line_index.saturating_sub(6)..rows[0].line_index]
        .iter()
        .map(|line| clean_line(line))
        .find_map(|line| {
            let captures = re_question_progress().captures(&line)?;
            Some((captures[1].to_string(), captures[2].to_string()))
        });
    let title = match &progress {
        Some((asked, total)) if total != "1" => format!("Question {asked} of {total}"),
        _ => "Question".to_string(),
    };
    Some(finish_codex_prompt(FinishCodexPrompt {
        kind: ReferencePromptKind::Question,
        title,
        question,
        body: None,
        options,
        multi_select: false,
        custom_option_index: Some(custom_index as u32),
        queued: None,
        responder: ReferenceCodexResponder::Question,
        selected_index: rows.iter().position(|row| row.selected).unwrap_or(0),
        custom_menu_index: Some(custom_index),
        reject_with_escape_index: None,
        option_steps: None,
        row_key: None,
    }))
}

/// Upstream `parseCodexAsyncQuestion` (`:340-388`).
///
/// A question from Codex's queue, open: under the queue header, an optional "1 of 2", the
/// question (wrapped over as many lines as the pane needs), then its options and a last row that
/// takes a typed answer ("Other", or what was typed there). A free-form question has no options,
/// only that answer line ("Type your answer").
pub fn parse_codex_async_question(screen: &str) -> Option<ReferenceCodexPrompt> {
    let lines = split_lines(screen);
    let hint_index = find_last_index(&lines, |_, index| {
        re_codex_async_ask_hint().is_match(&wrapped(&lines, index, 3))
    })?;
    let window_start = hint_index.saturating_sub(48);
    let header = find_last_index(&lines[window_start..hint_index], |line, _| {
        re_queue_header().is_match(&clean_line(line))
    });
    let top = match header {
        Some(offset) => window_start + offset + 1,
        None => window_start,
    };
    let rows = parse_numbered_rows(&lines, top, hint_index);

    let mut question: Option<String>;
    let options: Vec<ReferencePromptOption>;
    let menu_row_count: usize;
    let selected_index: usize;
    let mut position: Option<(String, String)> = None;

    if rows.is_empty() {
        // free form: the answer line sits right above the hint
        if header.is_none() {
            return None;
        }
        let answer_line = find_last_index(&lines[..hint_index], |line, _| {
            let cleaned = clean_line(line);
            !cleaned.is_empty() && !is_divider(&cleaned)
        })?;
        if answer_line < top {
            return None;
        }
        let (text, found) = codex_question_lines(&lines, top, answer_line);
        position = found;
        let normalized = normalize_text(&text.join(" "));
        if normalized.is_empty() {
            return None;
        }
        question = Some(normalized);
        options = Vec::new();
        menu_row_count = 0;
        selected_index = 0;
    } else {
        if !sequential_rows(&rows)
            || rows.len() < 2
            || rows.iter().filter(|row| row.selected).count() != 1
        {
            return None;
        }
        // an old layout without the header must still end in its "Other" row
        if header.is_none() && !re_other_row().is_match(&rows.last()?.label) {
            return None;
        }
        // a wrapped option continues on the lines under it: these rows carry no descriptions
        let labels: Vec<String> = rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                let to = rows
                    .get(index + 1)
                    .map(|next| next.line_index)
                    .unwrap_or(hint_index);
                let tail = codex_text_range(&lines, row.line_index + 1, to);
                normalize_text(&[row.label.clone(), tail.join(" ")].join(" "))
            })
            .collect();
        if header.is_none() {
            question = nearest_question(&lines, rows[0].line_index);
        } else {
            let (text, found) = codex_question_lines(&lines, top, rows[0].line_index);
            position = found;
            let normalized = normalize_text(&text.join(" "));
            question = if normalized.is_empty() {
                None
            } else {
                Some(normalized)
            };
        }
        options = labels[..labels.len() - 1]
            .iter()
            .map(|label| ReferencePromptOption {
                label: label.clone(),
                description: None,
            })
            .collect();
        menu_row_count = labels.len();
        selected_index = rows.iter().position(|row| row.selected).unwrap_or(0);
    }

    let question = question?;
    let title = match &position {
        Some((asked, total)) => format!("Question {asked} of {total}"),
        None => "Question".to_string(),
    };
    let custom_menu_index = menu_row_count.saturating_sub(1);
    let custom_option_index = Some(options.len() as u32);
    let parsed = finish_codex_prompt(FinishCodexPrompt {
        kind: ReferencePromptKind::Question,
        title,
        question,
        body: None,
        options,
        multi_select: false,
        custom_option_index,
        queued: Some(ReferencePromptQueueState::Open),
        responder: ReferenceCodexResponder::AsyncQuestion,
        selected_index,
        custom_menu_index: Some(custom_menu_index),
        reject_with_escape_index: None,
        option_steps: None,
        row_key: None,
    });
    Some(parsed)
}

/// Upstream `parseCodexApproval` (`:1071-1090`).
pub fn parse_codex_approval(screen: &str) -> Option<ReferenceCodexPrompt> {
    let lines = split_lines(screen);
    let header_index = find_last_index(&lines, |line, _| {
        re_codex_approval_header().is_match(line) && !re_numbered_option().is_match(line)
    })?;
    let rows = parse_numbered_rows(&lines, header_index + 1, lines.len());
    if !sequential_rows(&rows)
        || rows.len() < 2
        || rows.iter().filter(|row| row.selected).count() != 1
    {
        return None;
    }
    // "Trust this folder? Codex can read, …": the question heads the card, its explanation
    // joins the body
    let header = clean_line(&lines[header_index]);
    let split = re_question_split()
        .captures(&header)
        .map(|captures| (captures[1].to_string(), captures[2].to_string()));
    let heading = match &split {
        Some((head, _)) => head.clone(),
        None => header.clone(),
    };
    let mut body_parts: Vec<String> = Vec::new();
    if let Some((_, rest)) = &split {
        body_parts.push(rest.clone());
    }
    body_parts.extend(
        lines[header_index + 1..rows[0].line_index]
            .iter()
            .map(|line| clean_line(line))
            .filter(|line| !line.is_empty()),
    );
    let body = join_or_none(&body_parts, "\n");
    let options: Vec<ReferencePromptOption> = rows
        .iter()
        .map(|row| ReferencePromptOption {
            label: row.label.clone(),
            description: None,
        })
        .collect();
    Some(finish_codex_prompt(FinishCodexPrompt {
        kind: ReferencePromptKind::Approval,
        title: heading.clone(),
        question: heading,
        body,
        options,
        multi_select: false,
        custom_option_index: None,
        queued: None,
        responder: ReferenceCodexResponder::Approval,
        selected_index: rows.iter().position(|row| row.selected).unwrap_or(0),
        custom_menu_index: None,
        reject_with_escape_index: rows.iter().position(|row| re_reject_row().is_match(&row.label)),
        option_steps: None,
        row_key: None,
    }))
}

/// Upstream `parseCodexModel` (`:1474-1522`).
///
/// Codex's `/model` lists, as its source draws them. The footer is the row's own, not the
/// list's: under `enter select` Enter only opens the next list; under `enter default · s session`
/// the row picks. Enter is sent in one list only, the list of models — in a list of levels the
/// row that opens a list stands beside rows whose Enter saves a default, so that row is not
/// offered and no row of those lists is answered with Enter. A pane too narrow for what a row
/// says draws the names alone, or wraps it under itself.
pub fn parse_codex_model(screen: &str) -> Option<ReferenceCodexPrompt> {
    let lines: Vec<String> = split_lines(screen)
        .into_iter()
        .map(|line| line.trim_end().to_string())
        .collect();
    let hint_index = find_last_index(&lines, |_, index| {
        codex_model_row_key(&wrapped(&lines, index, 3)).is_some()
    })?;
    let listed = list_rows(&lines, hint_index, re_model_row(), None);
    // the rows end right over their footer, and one of them carries the cursor
    if listed.unread
        || listed.rows.len() < 2
        || listed.rows.iter().filter(|row| row.cursor).count() != 1
        || lines[listed.last + 1..hint_index]
            .iter()
            .any(|line| !line.trim().is_empty())
    {
        return None;
    }
    let header = codex_model_header(&lines, listed.first, false)?;
    let names = list_names(&listed.rows)?;
    let drawn: Vec<(String, Option<String>)> = names
        .iter()
        .map(|(name, said)| (strip_current(name), said.clone()))
        .collect();
    let selected_index = listed.rows.iter().position(|row| row.cursor)?;
    let footer = codex_model_row_key(&wrapped(&lines, hint_index, 3))?;
    let opens = !footer.keys.is_empty();
    // the list of models, where a row opens a model's levels and Enter is its key
    let models = header.title == "Select Model and Effort";
    // elsewhere a row that only opens a list is left to the terminal: the one Codex names so,
    // and the one under the cursor when its footer says so
    let offered: Vec<usize> = listed
        .rows
        .iter()
        .enumerate()
        .filter(|(index, _)| {
            !(!models
                && (re_model_more_row().is_match(drawn[*index].0.as_str())
                    || (*index == selected_index && opens)))
        })
        .map(|(index, _)| index)
        .collect();
    if offered.is_empty() {
        return None;
    }
    let current = names
        .iter()
        .position(|(name, _)| re_current_suffix().is_match(name));
    // "Medium (default)" in use reads as Medium: the tag is the list's, not the level's name
    let now = match current {
        Some(index) => format!(" (currently {})", strip_default(&drawn[index].0)),
        None => String::new(),
    };
    let asked = if header.title == "Advanced Reasoning" {
        "Select advanced reasoning for this session".to_string()
    } else if let Some(model) = &header.model {
        format!("Select reasoning level for {model} for this session")
    } else {
        "Select model for this session".to_string()
    };
    let question = format!(
        "{asked}{now}{}",
        if offered.len() < listed.rows.len() {
            ". More levels are listed in the terminal."
        } else {
            ""
        }
    );
    let options: Vec<ReferencePromptOption> = offered
        .iter()
        .map(|index| ReferencePromptOption {
            label: drawn[*index].0.clone(),
            description: drawn[*index].1.clone(),
        })
        .collect();
    let option_steps: Vec<Vec<ReferenceCodexAnswerStep>> = offered
        .iter()
        .map(|index| {
            let mut steps = key_steps(navigation_keys(*index as i64 - selected_index as i64));
            steps.push(ReferenceCodexAnswerStep::Pick);
            steps
        })
        .collect();
    // Enter in the list of models alone; `s` wherever the footer offers it; no key otherwise
    let row_key = if opens {
        if models {
            Some(footer)
        } else {
            None
        }
    } else {
        Some(footer)
    };
    Some(finish_codex_prompt(FinishCodexPrompt {
        kind: ReferencePromptKind::Question,
        title: String::new(),
        question,
        body: join_or_none(&header.notes, "\n"),
        options,
        multi_select: false,
        custom_option_index: None,
        queued: None,
        responder: ReferenceCodexResponder::Model,
        selected_index,
        custom_menu_index: None,
        reject_with_escape_index: None,
        option_steps: Some(option_steps),
        row_key,
    }))
}

/// Upstream `queuedPrompt` (`:418-434`): the card for the collapsed queue.
///
/// The collapsed queue shows only a count: the card takes its first question from the rollout,
/// the newest `count` unanswered ones (a skipped question leaves no record).
fn queued_prompt(
    count: usize,
    unanswered: &[ReferenceCodexQueuedQuestion],
    front: Option<&ReferenceCodexQueueFront>,
) -> Option<ReferenceCodexPrompt> {
    let waiting = &unanswered[unanswered.len().saturating_sub(count)..];
    // the question the queue opened on last time, when that was not the newest guess: by its
    // title and its options, the newest such one (an older skipped one may share the title)
    let matched = front.and_then(|front| {
        unanswered.iter().rev().find(|question| {
            same_text(&front.question, &question.title)
                && question.options.len() == front.options.len()
                && question
                    .options
                    .iter()
                    .zip(front.options.iter())
                    .all(|(shown, asked)| same_text(shown, asked))
        })
    });
    let first = matched.or_else(|| waiting.first())?;
    if waiting.len() != count || first.title.trim().is_empty() {
        return None;
    }
    let options: Vec<ReferencePromptOption> = first
        .options
        .iter()
        .map(|label| ReferencePromptOption {
            label: label.clone(),
            description: None,
        })
        .collect();
    let custom_menu_index = first.options.len();
    let menu_labels: Vec<String> = if first.options.is_empty() {
        Vec::new()
    } else {
        let mut labels = first.options.clone();
        labels.push("Other".to_string());
        labels
    };
    Some(finish_codex_prompt(FinishCodexPrompt {
        kind: ReferencePromptKind::Question,
        title: if count > 1 {
            format!("Question 1 of {count}")
        } else {
            "Question".to_string()
        },
        question: normalize_text(&first.title),
        body: None,
        options,
        multi_select: false,
        custom_option_index: Some(custom_menu_index as u32),
        queued: Some(ReferencePromptQueueState::Collapsed),
        responder: ReferenceCodexResponder::QueuedQuestion,
        selected_index: 0,
        custom_menu_index: Some(custom_menu_index),
        reject_with_escape_index: None,
        option_steps: None,
        row_key: None,
    }))
}

/// Upstream `promptTailIsActive` (`:1603-1725`), its Codex arms: is this prompt still the thing
/// at the bottom of the screen?
///
/// A narrow pane wraps its hint, so the last line alone can be the hint's tail (`cancel`): the
/// lines before it count only when the match runs into the last one, never for a hint that ended
/// above later output (an answered, stale menu).
///
/// `codex-queued-question` has no arm upstream — it is built from the rollout, never from this
/// screen parse, so it never reaches this gate. The arms below are exactly the ones the pinned
/// function has.
fn prompt_tail_is_active(parsed: &ReferenceCodexPrompt, screen: &str) -> bool {
    let clean_lines: Vec<String> = split_lines(screen).iter().map(|line| clean_line(line)).collect();
    let visible: Vec<String> = clean_lines
        .iter()
        .filter(|line| !line.is_empty() && !is_divider(line))
        .cloned()
        .collect();
    let shown = &visible;
    let ends = |pattern: &Regex| -> bool {
        (1..=3usize).any(|span| {
            if span > shown.len() {
                return false;
            }
            let joined = shown[shown.len() - span..].join(" ");
            if !pattern.is_match(&joined) {
                return false;
            }
            span == 1
                || !pattern.is_match(&shown[shown.len() - span..shown.len() - 1].join(" "))
        })
    };
    match parsed.responder {
        ReferenceCodexResponder::Menu => ends(re_codex_continue_hint()),
        ReferenceCodexResponder::Question => ends(re_codex_ask_hint()),
        ReferenceCodexResponder::AsyncQuestion => {
            clean_lines
                .iter()
                .rev()
                .take(4)
                .any(|line| re_codex_async_ask_hint().is_match(line))
                || ends(re_codex_async_ask_hint())
        }
        ReferenceCodexResponder::Approval => ends(re_codex_approval_tail()),
        ReferenceCodexResponder::Model => ends(re_codex_model_tail()),
        // the reference has no arm for the queue card: it is not in the candidate list
        ReferenceCodexResponder::QueuedQuestion => ends(re_codex_unknown_tail()),
    }
}

// ---------------------------------------------------------------------------------------------
// Shared readers, ported from the pinned prompt.ts
// ---------------------------------------------------------------------------------------------

/// Upstream `cleanLine` (`:135-140`): drop the ANSI, trim, and strip a box's side rules.
fn clean_line(raw_line: &str) -> String {
    let mut line = re_ansi().replace_all(raw_line, "").to_string();
    line = line.trim().to_string();
    if let Some(rest) = line.strip_prefix('│') {
        line = rest.trim_start().to_string();
    }
    if let Some(rest) = line.strip_suffix('│') {
        line = rest.trim_end().to_string();
    }
    line.trim().to_string()
}

/// Upstream `isDivider` (`:142-145`).
fn is_divider(line: &str) -> bool {
    let value = clean_line(line);
    !value.is_empty() && re_divider().is_match(&value)
}

/// Upstream `normalizeText` (`:147-149`).
fn normalize_text(value: &str) -> String {
    re_whitespace_run()
        .replace_all(value, " ")
        .trim()
        .to_string()
}

/// Upstream `findLastIndex` (`:151-156`).
fn find_last_index<F>(lines: &[String], predicate: F) -> Option<usize>
where
    F: Fn(&str, usize) -> bool,
{
    (0..lines.len())
        .rev()
        .find(|index| predicate(&lines[*index], *index))
}

/// Upstream `wrapped` (`:163-165`): a line and the two after it, as one, so a hint a narrow
/// pane wrapped is matched across the break.
fn wrapped(lines: &[String], index: usize, span: usize) -> String {
    let end = (index + span).min(lines.len());
    if index >= end {
        return String::new();
    }
    lines[index..end]
        .iter()
        .map(|line| clean_line(line))
        .filter(|line| !line.is_empty() && !is_divider(line))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Upstream `nearestQuestion` (`:167-175`).
fn nearest_question(lines: &[String], before_index: usize) -> Option<String> {
    let floor = before_index.saturating_sub(14);
    for index in (floor..before_index).rev() {
        let line = clean_line(&lines[index]);
        if line.is_empty()
            || is_divider(&line)
            || re_planning().is_match(&line)
            || re_submit_bar().is_match(&line)
            || re_tab_chip().is_match(&line)
            || re_question_progress().is_match(&line)
        {
            continue;
        }
        return Some(re_selected_count().replace(&line, "").trim().to_string());
    }
    None
}

/// Upstream `parseNumberedRows` (`:201-222`).
fn parse_numbered_rows(lines: &[String], start: usize, end: usize) -> Vec<NumberedRow> {
    let mut rows: Vec<NumberedRow> = Vec::new();
    let end = end.min(lines.len());
    for index in start..end {
        let raw = re_ansi().replace_all(&lines[index], "").to_string();
        let Some(captures) = re_numbered_option().captures(raw.trim()) else {
            continue;
        };
        let Some(number) = captures.get(2).and_then(|value| value.as_str().parse::<usize>().ok())
        else {
            continue;
        };
        let label = re_checked_prefix()
            .replace(captures[3].trim(), "")
            .trim()
            .to_string();
        rows.push(NumberedRow {
            number,
            label,
            selected: captures.get(1).is_some(),
            line_index: index,
            description: None,
        });
    }
    for index in 0..rows.len() {
        let row_line = rows[index].line_index;
        let next_line = rows.get(index + 1).map(|row| row.line_index).unwrap_or(end);
        for line_index in row_line + 1..next_line {
            let description = clean_line(&lines[line_index]);
            if description.is_empty() || is_divider(&description) {
                continue;
            }
            rows[index].description = Some(description);
            break;
        }
    }
    rows
}

/// Upstream `sequentialRows` (`:224-226`).
fn sequential_rows(rows: &[NumberedRow]) -> bool {
    !rows.is_empty() && rows.iter().enumerate().all(|(index, row)| row.number == index + 1)
}

/// Upstream `navigationKeys` (`:1987-1989`).
fn navigation_keys(delta: i64) -> Vec<String> {
    (0..delta.abs())
        .map(|_| {
            if delta > 0 {
                KEY_DOWN.to_string()
            } else {
                KEY_UP.to_string()
            }
        })
        .collect()
}

/// Upstream `keySteps` (`:1991-1993`).
fn key_steps(keys: Vec<String>) -> Vec<ReferenceCodexAnswerStep> {
    keys.into_iter()
        .map(|key| ReferenceCodexAnswerStep::Keys(vec![key]))
        .collect()
}

/// Upstream `sameText` (`:2525-2530`): a question the pane wraps or punctuates differently still
/// compares equal; a pane too narrow for a line may cut it with an ellipsis.
fn same_text(shown: &str, asked: &str) -> bool {
    let a = comparable(shown);
    let b = comparable(asked);
    a == b || (re_trailing_ellipsis().is_match(shown) && a.chars().count() >= 24 && b.starts_with(&a))
}

/// Upstream `comparable` (`:2523`): letters and digits only, lowercased.
///
/// Divergence (documented in the manifest): the reference normalizes NFKC first, and this port
/// does not — no Unicode normalization crate is available to a family lane, and adding one is a
/// manifest-level decision. Full-width and compatibility forms therefore compare unequal where
/// the reference compares them equal; every other case is identical.
fn comparable(text: &str) -> String {
    text.chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(|character| character.to_lowercase())
        .collect()
}

/// Upstream `join(...) || null` on a filtered line list.
fn join_or_none(lines: &[String], separator: &str) -> Option<String> {
    if lines.is_empty() {
        None
    } else {
        Some(lines.join(separator))
    }
}

/// Upstream `lines.slice(from, to).map(cleanLine).filter(line => line && !isDivider(line))`.
fn codex_text_range(lines: &[String], from: usize, to: usize) -> Vec<String> {
    if from >= lines.len() {
        return Vec::new();
    }
    let to = to.min(lines.len());
    if from >= to {
        return Vec::new();
    }
    lines[from..to]
        .iter()
        .map(|line| clean_line(line))
        .filter(|line| !line.is_empty() && !is_divider(line))
        .collect()
}

/// Upstream `questionLines(to)` in `parseCodexAsyncQuestion` (`:346-351`): the lines from the
/// question's top to `to`, with the queue's own "1 of 2" read off the first of them.
fn codex_question_lines(
    lines: &[String],
    top: usize,
    to: usize,
) -> (Vec<String>, Option<(String, String)>) {
    let found = codex_text_range(lines, top, to);
    let position = found
        .first()
        .and_then(|line| re_queue_position().captures(line))
        .map(|captures| (captures[1].to_string(), captures[2].to_string()));
    let rest = if position.is_some() {
        found.get(1..).map(<[String]>::to_vec).unwrap_or_default()
    } else {
        found
    };
    (rest, position)
}

/// Upstream `listRow` (`:1296-1308`).
fn list_row(line: &str, shape: &Regex) -> Option<ListRow> {
    let captures = shape.captures(line)?;
    let text = captures.get(3).map(|value| value.as_str()).unwrap_or("");
    let gap = re_wide_gap().find(text);
    let (gap_start, gap_len) = gap
        .map(|found| (found.start(), found.as_str().len()))
        .unwrap_or((usize::MAX, 0));
    let column = if gap.is_some() {
        let prefix_end = line.len() - text.len() + gap_start + gap_len;
        cell_width(&line[..prefix_end])
    } else {
        -1
    };
    let cursor = captures.get(1).is_some();
    let edge = captures.get(1).and_then(|value| {
        if value.as_str() == "↑" || value.as_str() == "↓" {
            Some(value.as_str().to_string())
        } else {
            None
        }
    });
    Some(ListRow {
        number: captures.get(2).and_then(|value| value.as_str().parse::<usize>().ok())?,
        cursor,
        edge,
        text: text.to_string(),
        gap: if gap.is_some() { gap_start as i64 } else { -1 },
        resumes: if gap.is_some() {
            (gap_start + gap_len) as i64
        } else {
            -1
        },
        column,
        wrapped: Vec::new(),
    })
}

/// Upstream `listNames` (`:1318-1336`): each row's name and what the row says of it.
///
/// What the rows say stands in one column for the whole list, so a gap at another column, or in
/// one row alone, is part of that row's name. `None`: lines wrapped under a row whose gap was no
/// column, so the rows are not as drawn.
fn list_names(rows: &[ListRow]) -> Option<Vec<(String, Option<String>)>> {
    let mut counts: Vec<(i64, usize)> = Vec::new();
    for row in rows {
        if row.column < 0 {
            continue;
        }
        match counts.iter_mut().find(|(column, _)| *column == row.column) {
            Some((_, count)) => *count += 1,
            None => counts.push((row.column, 1)),
        }
    }
    let (column, count) = counts
        .iter()
        .copied()
        .fold((-1i64, 0usize), |best, entry| {
            if entry.1 > best.1 {
                entry
            } else {
                best
            }
        });
    let shared = if count >= 2 { column } else { -1 };
    if rows
        .iter()
        .any(|row| row.column != shared && !row.wrapped.is_empty())
    {
        return None;
    }
    Some(
        rows.iter()
            .map(|row| {
                if shared >= 0 && row.column == shared {
                    let name = row.text[..row.gap as usize].to_string();
                    let mut said = vec![row.text[row.resumes as usize..].to_string()];
                    said.extend(row.wrapped.iter().cloned());
                    (name, Some(said.join(" ")))
                } else {
                    (normalize_text(&row.text), None)
                }
            })
            .collect(),
    )
}

/// Upstream `listRows` (`:1339-1370`): the numbered rows right above a list's hint, read
/// downward.
///
/// A row that does not count on from the one above it, that follows anything but its own wrapped
/// text, or that carries the window's `↑` starts the list again, and so does any row under the
/// one that carries its `↓`.
fn list_rows(lines: &[String], hint_index: usize, shape: &Regex, more: Option<&Regex>) -> ListRows {
    let mut rows: Vec<ListRow> = Vec::new();
    let mut ended = false;
    let mut unread = false;
    let mut first = usize::MAX;
    let mut last = 0usize;
    let start = hint_index.saturating_sub(80);
    for index in start..hint_index {
        let line = &lines[index];
        let text = line.trim();
        let above = if ended { None } else { rows.len().checked_sub(1) };
        // what a row says, wrapped under itself: told by its column, before anything it happens
        // to begin with
        if let Some(above_index) = above {
            let leading = line.len() - line.trim_start().len();
            if !text.is_empty()
                && rows[above_index].column >= 0
                && leading == rows[above_index].column as usize
            {
                rows[above_index].wrapped.push(text.to_string());
                last = index;
                continue;
            }
        }
        if let Some(row) = list_row(line, shape) {
            let restarts = match above {
                None => true,
                Some(above_index) => {
                    row.number != rows[above_index].number + 1
                        || row.edge.as_deref() == Some("↑")
                        || rows[above_index].edge.as_deref() == Some("↓")
                }
            };
            if restarts {
                rows.clear();
                first = index;
            }
            rows.push(row);
            ended = false;
            unread = false;
            last = index;
            continue;
        }
        if above.is_none() || text.is_empty() {
            ended = true;
            continue;
        }
        if more.is_some_and(|more| more.is_match(text)) {
            ended = true;
            last = index;
        } else {
            ended = true;
            unread = true;
        }
    }
    ListRows {
        rows,
        first,
        last,
        unread,
    }
}

/// Upstream `codexModelHeader` (`:1427-1437`): a Codex model list's title and the lines under it,
/// from the block of lines right above the rows (blank lines stand between the two).
///
/// The title is looked for in that block alone: another list's header under an older title is
/// another list. A pane too narrow for the levels' title wraps the model's name under it, and the
/// two are read as one. Lines over the title are the conversation's.
fn codex_model_header(
    lines: &[String],
    first: usize,
    allow_presets: bool,
) -> Option<CodexModelHeader> {
    let mut end: i64 = first as i64 - 1;
    while end >= 0 && clean_line(&lines[end as usize]).is_empty() {
        end -= 1;
    }
    let mut start = end;
    while start > 0 && !clean_line(&lines[(start - 1) as usize]).is_empty() {
        start -= 1;
    }
    let block: Vec<String> = if end < 0 {
        Vec::new()
    } else {
        lines[start as usize..=end as usize]
            .iter()
            .map(|line| clean_line(line))
            .collect()
    };
    let at = find_last_index(&block, |line, _| re_model_title_start().is_match(line))?;
    // the levels' title runs on to the end of the block when it wrapped; the others are one line
    let whole = if re_reasoning_title().is_match(&block[at]) {
        block[at..].join(" ")
    } else {
        block[at].clone()
    };
    let matched = re_model_title().captures(&whole);
    let notes: Vec<String> = if whole == block[at] {
        block[at + 1..].to_vec()
    } else {
        Vec::new()
    };
    if !(matched.is_some() || (allow_presets && whole == "Select Model"))
        || notes.len() > CODEX_MODEL_NOTE_LINES
    {
        return None;
    }
    let model =
        matched.and_then(|captures| captures.get(1).map(|value| value.as_str().to_string()));
    Some(CodexModelHeader {
        title: whole,
        model,
        notes,
    })
}

/// The terminal column a string occupies, as `Bun.stringWidth` measures it.
///
/// Same ranges as `crate::remote::mirror`'s private `char_width`, which is not reachable from
/// this module; a row's description column must be counted the way the pane drew it, or a
/// wrapped line would be read as a new row.
fn cell_width(value: &str) -> i64 {
    value
        .chars()
        .map(|character| {
            let unit = character as u32;
            if character.is_control() {
                return 0;
            }
            if (0x0300..=0x036F).contains(&unit)
                || (0x1AB0..=0x1AFF).contains(&unit)
                || (0x1DC0..=0x1DFF).contains(&unit)
                || (0x20D0..=0x20FF).contains(&unit)
                || (0xFE20..=0xFE2F).contains(&unit)
                || unit == 0x200B
                || unit == 0xFEFF
            {
                return 0;
            }
            if (0x1100..=0x115F).contains(&unit)
                || (0x2E80..=0xA4CF).contains(&unit)
                || (0xAC00..=0xD7A3).contains(&unit)
                || (0xF900..=0xFAFF).contains(&unit)
                || (0xFE10..=0xFE19).contains(&unit)
                || (0xFE30..=0xFE6F).contains(&unit)
                || (0xFF00..=0xFF60).contains(&unit)
                || (0xFFE0..=0xFFE6).contains(&unit)
                || (0x20000..=0x2FFFD).contains(&unit)
                || (0x30000..=0x3FFFD).contains(&unit)
            {
                return 2;
            }
            1
        })
        .sum()
}

/// Upstream `queuedQuestionCount`'s last guard: the main prompt, not a numbered menu row the
/// parser did not recognise (`/^›\s(?!\d+\.)/`).
///
/// The regex crate has no lookahead, so the negative assertion is its own check. It is the only
/// place in this port where a pinned pattern is spelled out rather than compiled.
fn is_queue_main_prompt(line: &str) -> bool {
    let Some(rest) = line.strip_prefix('›') else {
        return false;
    };
    let Some(rest) = rest.strip_prefix(char::is_whitespace) else {
        return false;
    };
    !re_numbered_rest().is_match(rest)
}

/// Upstream `String.prototype.replace(/\s*\(current\)$/, "")`.
fn strip_current(name: &str) -> String {
    re_current_suffix().replace(name, "").to_string()
}

/// Upstream `label.replace(/\s*\(default\)$/i, "")`.
fn strip_default(name: &str) -> String {
    re_default_suffix().replace(name, "").to_string()
}

/// Upstream `finishPrompt` (`:228-242`) plus `publicPrompt` (`:244-261`), for a Codex card.
///
/// The id hashes the whole body before the display cap: a change beyond the cap is still a
/// different prompt, and a cursor move is not — which is what keeps a stale card refused rather
/// than misfired.
fn finish_codex_prompt(input: FinishCodexPrompt) -> ReferenceCodexPrompt {
    let hashed_body = input.body.clone();
    let id = codex_prompt_id(&input, hashed_body.as_deref());
    let prompt = ReferencePrompt {
        id,
        agent: REFERENCE_CODEX_AGENT.to_string(),
        kind: input.kind,
        title: input.title,
        question: input.question,
        body: hashed_body.map(|body| body.chars().take(REFERENCE_CODEX_BODY_CAP).collect()),
        options: input.options,
        multi_select: input.multi_select,
        custom_option_index: input.custom_option_index,
        queued: input.queued,
        steps: Vec::new(),
        fallback: None,
    };
    ReferenceCodexPrompt {
        prompt,
        responder: input.responder,
        selected_index: input.selected_index,
        custom_menu_index: input.custom_menu_index,
        reject_with_escape_index: input.reject_with_escape_index,
        option_steps: input.option_steps,
        row_key: input.row_key,
    }
}

/// Upstream `createHash("sha256").update(JSON.stringify({agent, ...input, ...hashed}))…slice(0, 12)`.
///
/// The fields are emitted in the pinned object's own order with `JSON.stringify`'s escaping, so
/// the digest is stable for equal cards and different for any change the reference would see.
/// The `queued` key is present only where the parser passes one, as upstream.
fn codex_prompt_id(input: &FinishCodexPrompt, body: Option<&str>) -> String {
    let mut json = String::new();
    json.push_str("{\"agent\":");
    json.push_str(&js_string(REFERENCE_CODEX_AGENT));
    json.push_str(",\"kind\":");
    json.push_str(&js_string(match input.kind {
        ReferencePromptKind::Question => "question",
        ReferencePromptKind::Approval => "approval",
        ReferencePromptKind::Plan => "plan",
        ReferencePromptKind::Menu => "menu",
    }));
    json.push_str(",\"title\":");
    json.push_str(&js_string(&input.title));
    json.push_str(",\"question\":");
    json.push_str(&js_string(&input.question));
    json.push_str(",\"body\":");
    match body {
        Some(body) => json.push_str(&js_string(body)),
        None => json.push_str("null"),
    }
    json.push_str(",\"options\":[");
    for (index, option) in input.options.iter().enumerate() {
        if index > 0 {
            json.push(',');
        }
        json.push_str("{\"label\":");
        json.push_str(&js_string(&option.label));
        json.push_str(",\"description\":");
        match &option.description {
            Some(description) => json.push_str(&js_string(description)),
            None => json.push_str("null"),
        }
        json.push('}');
    }
    json.push_str("],\"multi_select\":");
    json.push_str(if input.multi_select { "true" } else { "false" });
    json.push_str(",\"custom_option_index\":");
    match input.custom_option_index {
        Some(index) => json.push_str(&index.to_string()),
        None => json.push_str("null"),
    }
    if let Some(queued) = input.queued {
        json.push_str(",\"queued\":");
        json.push_str(&js_string(match queued {
            ReferencePromptQueueState::Collapsed => "collapsed",
            ReferencePromptQueueState::Open => "open",
        }));
    }
    json.push('}');

    let digest = Sha256::digest(json.as_bytes());
    let mut hex = String::with_capacity(12);
    for byte in digest.iter().take(6) {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

/// `JSON.stringify`'s string escaping: the quotes and backslash, the short control escapes, and
/// `\uXXXX` for the rest below 0x20. Everything else — including every non-ASCII character —
/// stays as it is.
fn js_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            character if (character as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => out.push(character),
        }
    }
    out.push('"');
    out
}

/// Upstream `screen.replace(ANSI_RE, "").split(/\r?\n/)`.
fn split_lines(screen: &str) -> Vec<String> {
    re_ansi()
        .replace_all(screen, "")
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_string())
        .collect()
}

/// One numbered row, as `parseNumberedRows` reads it.
#[derive(Debug, Clone)]
struct NumberedRow {
    number: usize,
    label: String,
    selected: bool,
    line_index: usize,
    description: Option<String>,
}

/// One row of a `/model` list, as `listRow` reads it.
#[derive(Debug, Clone)]
struct ListRow {
    number: usize,
    cursor: bool,
    edge: Option<String>,
    text: String,
    gap: i64,
    resumes: i64,
    column: i64,
    wrapped: Vec<String>,
}

/// The rows `listRows` found, and what it saw around them.
#[derive(Debug, Clone)]
struct ListRows {
    rows: Vec<ListRow>,
    first: usize,
    last: usize,
    unread: bool,
}

/// A model list's title and the lines under it.
#[derive(Debug, Clone)]
struct CodexModelHeader {
    title: String,
    model: Option<String>,
    notes: Vec<String>,
}

/// Everything `finishPrompt` is handed for a Codex card.
struct FinishCodexPrompt {
    kind: ReferencePromptKind,
    title: String,
    question: String,
    body: Option<String>,
    options: Vec<ReferencePromptOption>,
    multi_select: bool,
    custom_option_index: Option<u32>,
    queued: Option<ReferencePromptQueueState>,
    responder: ReferenceCodexResponder,
    selected_index: usize,
    custom_menu_index: Option<usize>,
    reject_with_escape_index: Option<usize>,
    option_steps: Option<Vec<Vec<ReferenceCodexAnswerStep>>>,
    row_key: Option<ReferenceKeyStep>,
}

// ---------------------------------------------------------------------------------------------
// Pinned patterns
// ---------------------------------------------------------------------------------------------

macro_rules! pinned_regex {
    ($name:ident, $pattern:expr) => {
        fn $name() -> &'static Regex {
            static PATTERN: OnceLock<Regex> = OnceLock::new();
            PATTERN.get_or_init(|| Regex::new($pattern).expect("a pinned reference pattern compiles"))
        }
    };
}

pinned_regex!(re_ansi, r"\x1b\[[0-?]*[ -/]*[@-~]");
pinned_regex!(re_divider, r"^[\s╭╮╰╯├┤┬┴┼─━═╌▔]+$");
pinned_regex!(re_whitespace_run, r"\s+");
pinned_regex!(re_wide_gap, r"\s{2,}");
pinned_regex!(re_numbered_option, r"^\s*([›>❯])?\s*(\d+)\.\s+(.+)$");
pinned_regex!(re_numbered_rest, r"^\d+\.");
pinned_regex!(re_checked_prefix, r"^\[[ xX✓]\]\s*");
pinned_regex!(re_planning, r"(?i)^Planning:");
pinned_regex!(re_submit_bar, r"^[←→].*Submit");
pinned_regex!(re_tab_chip, r"^[☐☑✔]\s+\S");
pinned_regex!(re_selected_count, r"(?i)^\(\d+\s+selected\)\s*");
pinned_regex!(re_question_progress, r"^Question (\d+)/(\d+)");
pinned_regex!(re_trailing_ellipsis, r"…\s*$");
// Codex's own hints
pinned_regex!(
    re_codex_ask_hint,
    r"(?i)tab to add notes.*enter to submit (?:answer|all).*esc to interrupt"
);
pinned_regex!(
    re_codex_async_ask_hint,
    r"(?i)(?:enter|return).*submit.*(?:ctrl\s*\+\s*\]|skip)"
);
pinned_regex!(re_codex_continue_hint, r"(?i)press\s+enter\s+to\s+continue");
pinned_regex!(re_queue_header, r"^(?:•\s*)?Queued follow-up inputs$");
pinned_regex!(re_queue_count, r"^\?\s*(\d+)\s+questions?\b");
pinned_regex!(re_queue_position, r"^(\d+) of (\d+)$");
pinned_regex!(re_queue_arrow, r"^↳\s");
pinned_regex!(re_messages_to_be_submitted, r"(?i)Messages to be submitted");
pinned_regex!(re_to_answer, r"(?i)\bto answer$");
pinned_regex!(re_none_of_the_above, r"(?i)^None of the above\b");
pinned_regex!(re_other_row, r"(?i)^Other\b");
pinned_regex!(
    re_codex_approval_header,
    r"(?i)(?:Would you like to (?:run|make|apply|continue|grant)|Allow Codex to|Approve (?:this )?(?:app )?tool call|Do you trust the contents|Trust this folder\?|Enable full access)"
);
pinned_regex!(re_question_split, r"^(.*?\?)\s+(.+)$");
pinned_regex!(re_reject_row, r"(?i)^(?:No|Reject|Cancel|Deny)\b");
pinned_regex!(
    re_codex_approval_tail,
    r"(?i)press enter to confirm|esc to cancel|enter continue.*esc back|^(?:[›>❯]\s*)?\d+\.\s+(?:No|Reject|Cancel|Deny)\b"
);
pinned_regex!(
    re_codex_model_tail,
    r"(?i)(?:^|\s)enter select\s*·\s*esc back$|(?:^|\s)enter (?:default|apply)\s*·\s*s session\s*·\s*esc back$"
);
pinned_regex!(re_codex_unknown_tail, r"(?i)ctrl\+g to edit|shift\+tab to approve with this feedback");
// Codex's /model lists
pinned_regex!(re_model_open_hint, r"(?i)^enter select\s*·\s*esc back$");
pinned_regex!(
    re_model_pick_hint,
    r"(?i)^enter (?:default|apply)\s*·\s*s session\s*·\s*esc back$"
);
pinned_regex!(
    re_model_title,
    r"^(?:Select Model and Effort|Select Reasoning Level for (\S.*)|Advanced Reasoning)$"
);
pinned_regex!(re_model_title_start, r"^(?:Select\b|Advanced Reasoning$)");
pinned_regex!(re_reasoning_title, r"^Select Reasoning Level for(?: \S.*)?$");
pinned_regex!(re_model_more_row, r"^More reasoning(?:…|\.{3})$");
pinned_regex!(re_model_row, r"^\s*([❯›>])?\s*(\d+)\.\s+(\S.*)$");
pinned_regex!(re_current_suffix, r"\s*\(current\)$");
pinned_regex!(re_default_suffix, r"(?i)\s*\(default\)$");
pinned_regex!(re_model_picks_flat, r"(?i)^enter(?:default|apply)·ssession·escback$");
pinned_regex!(
    re_model_guard_hint,
    r"(?i)^(?:(?:ctrl|alt|shift|cmd|super)\+)*(?:enter|return|tab|space|esc|escape|backspace|delete|insert|home|end|pageup|pagedown|up|down|left|right|f\d{1,2}|[a-z0-9])(?:select|default|apply|confirm)·(?:ssession·)?(?:(?:ctrl|alt|shift|super)\+)*(?:enter|return|tab|space|esc|escape|backspace|delete|insert|home|end|pageup|pagedown|up|down|left|right|f\d{1,2}|[a-z0-9])back$"
);

#[cfg(test)]
mod tests {
    use super::*;

    const MENU: &str = include_str!("fixtures/codex/menu.txt");
    const QUESTION: &str = include_str!("fixtures/codex/question.txt");
    const QUESTION_LAST: &str = include_str!("fixtures/codex/question-last-of-several.txt");
    const ASYNC_QUESTION: &str = include_str!("fixtures/codex/async-question.txt");
    const ASYNC_FREE_FORM: &str = include_str!("fixtures/codex/async-freeform.txt");
    const ASYNC_TYPED_ROW: &str = include_str!("fixtures/codex/async-typed-row.txt");
    const QUEUED_COLLAPSED: &str = include_str!("fixtures/codex/queued-collapsed.txt");
    const QUEUED_COLLAPSED_USER_MESSAGE: &str =
        include_str!("fixtures/codex/queued-collapsed-user-message.txt");
    const APPROVAL_COMMAND: &str = include_str!("fixtures/codex/approval-command.txt");
    const APPROVAL_TRUST_FOLDER: &str = include_str!("fixtures/codex/approval-trust-folder.txt");
    const MODEL_LIST: &str = include_str!("fixtures/codex/model-list.txt");
    const MODEL_LEVELS: &str = include_str!("fixtures/codex/model-levels.txt");
    const MODEL_LEVELS_MORE: &str = include_str!("fixtures/codex/model-levels-more-row.txt");
    const MODEL_ADVANCED: &str = include_str!("fixtures/codex/model-advanced.txt");
    const MODEL_ADVANCED_PHONE: &str = include_str!("fixtures/codex/model-advanced-phone.txt");
    const MODEL_LEVELS_NAMES_ONLY: &str =
        include_str!("fixtures/codex/model-levels-names-only.txt");
    const MODEL_LEVELS_WRAPPED_TITLE: &str =
        include_str!("fixtures/codex/model-levels-wrapped-title.txt");
    const MODEL_LIST_GAP_IN_NAME: &str =
        include_str!("fixtures/codex/model-list-gap-in-name.txt");
    const MODEL_LIST_GAP_IN_NAME_BESIDE: &str =
        include_str!("fixtures/codex/model-list-gap-in-name-beside.txt");
    const MODEL_LIST_OTHER_TITLE: &str =
        include_str!("fixtures/codex/model-list-other-title.txt");
    const MODEL_LIST_PRESETS: &str = include_str!("fixtures/codex/model-list-presets.txt");
    const MODEL_LEVELS_CONFIRM_FOOTER: &str =
        include_str!("fixtures/codex/model-levels-confirm-footer.txt");
    const STALE_MODEL_ANSWERED: &str =
        include_str!("fixtures/codex/stale-model-answered.txt");
    const MODEL_LEVELS_NO_CURSOR: &str =
        include_str!("fixtures/codex/model-levels-no-cursor.txt");
    const MODEL_LIST_SINGLE_ROW: &str =
        include_str!("fixtures/codex/model-list-single-row.txt");
    const MODEL_LEVELS_LOADING: &str =
        include_str!("fixtures/codex/model-levels-loading.txt");
    const STALE_MENU: &str = include_str!("fixtures/codex/stale-menu.txt");
    const STALE_APPROVAL: &str = include_str!("fixtures/codex/stale-approval.txt");
    const AMBIGUOUS_DOUBLE_CURSOR: &str = include_str!("fixtures/codex/ambiguous-double-cursor.txt");
    const AMBIGUOUS_QUEUE_MENU: &str = include_str!("fixtures/codex/ambiguous-queue-menu.txt");

    fn labels(parsed: &ReferenceCodexPrompt) -> Vec<String> {
        parsed
            .prompt
            .options
            .iter()
            .map(|option| option.label.clone())
            .collect()
    }

    fn texts(steps: &[ReferenceCodexAnswerStep]) -> Vec<ReferenceCodexAnswerStep> {
        steps.to_vec()
    }

    fn keys_of(keys: &[&str]) -> ReferenceCodexAnswerStep {
        ReferenceCodexAnswerStep::Keys(keys.iter().map(|key| key.to_string()).collect())
    }

    fn answer(
        option_index: Option<u32>,
        option_indices: Option<Vec<u32>>,
        custom_text: Option<&str>,
    ) -> ReferencePromptAnswer {
        ReferencePromptAnswer {
            option_index,
            option_indices,
            custom_text: custom_text.map(str::to_string),
        }
    }

    fn queued_question(key: &str, title: &str, options: &[&str]) -> ReferenceCodexQueuedQuestion {
        ReferenceCodexQueuedQuestion {
            key: key.to_string(),
            title: title.to_string(),
            options: options.iter().map(|option| option.to_string()).collect(),
        }
    }

    /// The detector claims the registry id it was handed and refuses every other one: the
    /// reference dispatches by agent, so a Codex screen under another id is not a Codex card.
    #[test]
    fn the_detector_answers_only_for_the_codex_agent_id() {
        let detected = detect_reference_codex_prompt(REFERENCE_CODEX_AGENT, MENU)
            .expect("the codex menu is a codex card");
        assert_eq!(detected.agent, REFERENCE_CODEX_AGENT);
        assert_eq!(detected.kind, ReferencePromptKind::Menu);
        for agent in ["claude", "omp", "omo", "pi", "", "other"] {
            assert!(
                detect_reference_codex_prompt(agent, MENU).is_none(),
                "{agent} must not be answered with a codex card"
            );
        }
        assert!(detect_reference_codex_prompt(REFERENCE_CODEX_AGENT, "no prompt here\n").is_none());
    }

    /// `codex-menu`: the continue menu, its rows, and the body above them.
    #[test]
    fn the_continue_menu_is_read_with_its_body_and_its_selected_row() {
        let parsed = parse_reference_codex_prompt(MENU).expect("the continue menu parses");
        assert_eq!(parsed.responder, ReferenceCodexResponder::Menu);
        assert_eq!(parsed.prompt.kind, ReferencePromptKind::Menu);
        assert_eq!(parsed.prompt.title, "Codex");
        assert_eq!(parsed.prompt.question, "Choose how to continue");
        assert_eq!(
            labels(&parsed),
            vec!["Update now", "Skip", "Skip until next version"]
        );
        assert_eq!(parsed.selected_index, 0);
        assert_eq!(
            parsed.prompt.body.as_deref(),
            Some("✨ Update available! 0.146.0 -> 0.146.1")
        );
        // two moves then Enter: the cursor starts on row 1, so option 2 is two rows down
        assert_eq!(
            texts(&plan_reference_codex_answer(&parsed, &answer(Some(2), None, None)).unwrap()),
            vec![
                keys_of(&["down"]),
                keys_of(&["down"]),
                keys_of(&["enter"])
            ]
        );
        // the frozen DTO projection carries the same keys
        let planned = plan_reference_codex_answer(&parsed, &answer(Some(2), None, None)).unwrap();
        assert_eq!(
            reference_key_steps(&planned).unwrap(),
            vec![
                ReferenceKeyStep::keys(["down"]),
                ReferenceKeyStep::keys(["down"]),
                ReferenceKeyStep::keys(["enter"])
            ]
        );
    }

    /// `codex-question`: the numbered question whose last row takes a typed answer.
    #[test]
    fn the_numbered_question_reads_its_options_descriptions_and_typed_row() {
        let parsed = parse_reference_codex_prompt(QUESTION).expect("the question parses");
        assert_eq!(parsed.responder, ReferenceCodexResponder::Question);
        assert_eq!(parsed.prompt.kind, ReferencePromptKind::Question);
        assert_eq!(parsed.prompt.title, "Question");
        assert_eq!(parsed.prompt.question, "Which export format should we use?");
        assert_eq!(labels(&parsed), vec!["ONNX", "TensorRT", "RKNN"]);
        assert_eq!(
            parsed.prompt.options[0].description.as_deref(),
            Some("Export a portable ONNX model.")
        );
        assert_eq!(parsed.prompt.custom_option_index, Some(3));
        assert_eq!(parsed.custom_menu_index, Some(3));
        assert_eq!(parsed.selected_index, 0);
        assert!(parsed.prompt.queued.is_none());
        // the typed answer goes through the notes row: the moves to it, tab, the text, Enter
        assert_eq!(
            texts(&plan_reference_codex_answer(&parsed, &answer(None, None, Some("ROCm"))).unwrap()),
            vec![
                keys_of(&["down"]),
                keys_of(&["down"]),
                keys_of(&["down"]),
                keys_of(&["tab"]),
                ReferenceCodexAnswerStep::Text("ROCm".to_string()),
                keys_of(&["enter"])
            ]
        );
    }

    /// A later question of several: the title carries its position, and a single-question screen
    /// does not.
    #[test]
    fn the_question_title_carries_its_position_only_past_the_first() {
        let parsed =
            parse_reference_codex_prompt(QUESTION_LAST).expect("the last of several parses");
        assert_eq!(parsed.prompt.title, "Question 2 of 2");
        assert_eq!(parsed.prompt.question, "Which split?");
        assert_eq!(labels(&parsed), vec!["train (Recommended)", "test"]);
        assert_eq!(parsed.prompt.custom_option_index, Some(2));
        let first = parse_reference_codex_prompt(QUESTION).expect("the first question parses");
        assert_eq!(first.prompt.title, "Question");
    }

    /// `codex-async-question`: an open queue question, its position, a wrapped title, wrapped
    /// options and the typed-answer row.
    #[test]
    fn the_open_queue_question_reads_position_wrapped_rows_and_the_typed_row() {
        let parsed =
            parse_reference_codex_prompt(ASYNC_QUESTION).expect("the open question parses");
        assert_eq!(parsed.responder, ReferenceCodexResponder::AsyncQuestion);
        assert_eq!(parsed.prompt.title, "Question 1 of 2");
        assert_eq!(parsed.prompt.queued, Some(ReferencePromptQueueState::Open));
        assert_eq!(
            parsed.prompt.question,
            "정리 범위를 현재 Q255 학습 출력과 연결된 산출물로 한정할까요, 아니면 output/test 전체 실험까지 포함할까요?"
        );
        assert_eq!(parsed.prompt.custom_option_index, Some(2));
        assert_eq!(
            labels(&parsed),
            vec![
                "현재 Q255 관련 산출물만",
                "output/test 전체 실험까지 포함해서 모두 정리하고 결과를 표로 남기기"
            ]
        );
        // an answer of its own is typed into the last row once it is selected, then submitted
        assert_eq!(
            texts(
                &plan_reference_codex_answer(&parsed, &answer(None, None, Some("Q255 only, keep logs")))
                    .unwrap()
            ),
            vec![
                keys_of(&["down"]),
                keys_of(&["down"]),
                ReferenceCodexAnswerStep::Text("Q255 only, keep logs".to_string()),
                keys_of(&["enter"])
            ]
        );
        assert_eq!(
            texts(&plan_reference_codex_answer(&parsed, &answer(Some(1), None, None)).unwrap()),
            vec![keys_of(&["down"]), keys_of(&["enter"])]
        );
    }

    /// A free-form queue question has no options and a single typed row.
    #[test]
    fn the_free_form_queue_question_offers_only_its_typed_row() {
        let parsed = parse_reference_codex_prompt(ASYNC_FREE_FORM).expect("the free form parses");
        assert_eq!(parsed.responder, ReferenceCodexResponder::AsyncQuestion);
        assert_eq!(parsed.prompt.title, "Question");
        assert_eq!(parsed.prompt.question, "Any notes?");
        assert!(parsed.prompt.options.is_empty());
        assert_eq!(parsed.prompt.custom_option_index, Some(0));
        assert_eq!(parsed.custom_menu_index, Some(0));
        assert_eq!(
            texts(&plan_reference_codex_answer(&parsed, &answer(None, None, Some("none"))).unwrap()),
            vec![
                ReferenceCodexAnswerStep::Text("none".to_string()),
                keys_of(&["enter"])
            ]
        );
    }

    /// A last row already typed over still counts as the typed row, and its options are the ones
    /// above it.
    #[test]
    fn the_open_queue_question_reads_a_row_already_typed_over() {
        let parsed = parse_reference_codex_prompt(ASYNC_TYPED_ROW).expect("the typed row parses");
        assert_eq!(labels(&parsed), vec!["train", "test"]);
        assert_eq!(parsed.prompt.custom_option_index, Some(2));
        assert_eq!(parsed.selected_index, 2);
        assert_eq!(
            texts(&plan_reference_codex_answer(&parsed, &answer(Some(0), None, None)).unwrap()),
            vec![keys_of(&["up"]), keys_of(&["up"]), keys_of(&["enter"])]
        );
    }

    /// The collapsed queue is not a screen card at all: only the rollout-backed card answers it.
    #[test]
    fn the_collapsed_queue_is_read_from_the_rollout_and_not_from_the_screen() {
        assert!(parse_reference_codex_prompt(QUEUED_COLLAPSED).is_none());
        assert_eq!(queued_question_count(QUEUED_COLLAPSED), 2);
        assert!(codex_questions_collapsed(QUEUED_COLLAPSED));

        let asked = vec![
            queued_question("call_c:0", "Which dataset?", &["LM-O"]),
            queued_question("call_c:1", "Any notes?", &[]),
        ];
        let card = codex_queued_prompt(QUEUED_COLLAPSED, &asked, None)
            .expect("the collapsed queue has a card");
        assert_eq!(card.responder, ReferenceCodexResponder::QueuedQuestion);
        assert_eq!(card.prompt.kind, ReferencePromptKind::Question);
        assert_eq!(card.prompt.queued, Some(ReferencePromptQueueState::Collapsed));
        assert_eq!(card.prompt.title, "Question 1 of 2");
        assert_eq!(card.prompt.question, "Which dataset?");
        assert_eq!(labels(&card), vec!["LM-O"]);
        assert_eq!(card.prompt.custom_option_index, Some(1));

        // a message of the user's own waiting to go replaces the questions' block
        assert!(parse_reference_codex_prompt(QUEUED_COLLAPSED_USER_MESSAGE).is_none());
        assert_eq!(queued_question_count(QUEUED_COLLAPSED_USER_MESSAGE), 0);
        assert!(codex_queued_prompt(QUEUED_COLLAPSED_USER_MESSAGE, &asked, None).is_none());
        assert!(!codex_questions_collapsed(QUEUED_COLLAPSED_USER_MESSAGE));
    }

    /// The card takes the question the queue opened on last time, by title *and* options: an
    /// older skipped question with the same title is not the one it opened on, and fewer
    /// questions on record than the queue holds means the card cannot say which is first.
    #[test]
    fn the_collapsed_queue_card_picks_the_question_it_opened_on() {
        let asked = vec![
            queued_question("call_a:0", "Old question?", &["x", "y"]),
            queued_question("call_b:0", "Which dataset?", &["LM-O", "YCB-V"]),
            queued_question("call_b:1", "Any notes?", &[]),
        ];
        let card = codex_queued_prompt(QUEUED_COLLAPSED, &asked, None).expect("a card");
        assert_eq!(card.prompt.title, "Question 1 of 2");
        assert_eq!(labels(&card), vec!["LM-O", "YCB-V"]);

        let front = ReferenceCodexQueueFront {
            question: "Old question?".to_string(),
            options: vec!["x".to_string(), "y".to_string()],
        };
        let opened = codex_queued_prompt(QUEUED_COLLAPSED, &asked, Some(&front)).expect("a card");
        assert_eq!(opened.prompt.question, "Old question?");
        assert_eq!(opened.prompt.title, "Question 1 of 2");

        // twins: the newest question with the front's own title and options is the one
        let mut twins = vec![queued_question("call_t:0", "Which dataset?", &["COCO"])];
        twins.extend(asked.clone());
        let front_shared = ReferenceCodexQueueFront {
            question: "Which dataset?".to_string(),
            options: vec!["LM-O".to_string(), "YCB-V".to_string()],
        };
        assert_eq!(
            labels(&codex_queued_prompt(QUEUED_COLLAPSED, &twins, Some(&front_shared)).unwrap()),
            vec!["LM-O", "YCB-V"]
        );
        let front_coco = ReferenceCodexQueueFront {
            question: "Which dataset?".to_string(),
            options: vec!["COCO".to_string()],
        };
        assert_eq!(
            labels(&codex_queued_prompt(QUEUED_COLLAPSED, &twins, Some(&front_coco)).unwrap()),
            vec!["COCO"]
        );

        // fewer on record than the queue holds: the card cannot say which is first
        assert!(codex_queued_prompt(QUEUED_COLLAPSED, &asked[2..], None).is_none());
    }

    /// `codex-approval`: the header's question heads the card and its explanation joins the
    /// body, and the "back out" row is answered with Escape, never with Enter.
    #[test]
    fn the_approval_reads_its_question_its_body_and_its_escape_row() {
        let parsed = parse_reference_codex_prompt(APPROVAL_COMMAND).expect("the approval parses");
        assert_eq!(parsed.responder, ReferenceCodexResponder::Approval);
        assert_eq!(parsed.prompt.kind, ReferencePromptKind::Approval);
        assert_eq!(
            parsed.prompt.title,
            "Would you like to run the following command?"
        );
        assert_eq!(parsed.prompt.question, parsed.prompt.title);
        assert!(parsed
            .prompt
            .body
            .as_deref()
            .unwrap_or_default()
            .contains("curl -I"));
        assert_eq!(
            labels(&parsed),
            vec![
                "Yes, proceed (y)",
                "Yes, and don't ask again for commands that start with curl",
                "No, and tell Codex what to do differently (esc)"
            ]
        );
        assert_eq!(parsed.reject_with_escape_index, Some(2));
        assert_eq!(
            texts(&plan_reference_codex_answer(&parsed, &answer(Some(2), None, None)).unwrap()),
            vec![keys_of(&["esc"])]
        );
        assert!(parsed.prompt.needs_confirmation(&answer(Some(0), None, None)));
    }

    /// The folder-trust prompt heads its card with its own question and puts the explanation in
    /// the body, and its selected row takes Enter.
    #[test]
    fn the_folder_trust_prompt_splits_its_header_into_title_and_body() {
        let parsed =
            parse_reference_codex_prompt(APPROVAL_TRUST_FOLDER).expect("the trust prompt parses");
        assert_eq!(parsed.responder, ReferenceCodexResponder::Approval);
        assert_eq!(parsed.prompt.title, "Trust this folder?");
        assert_eq!(
            parsed.prompt.body.as_deref(),
            Some("Codex can read, edit, and run files here, subject to your permission settings.")
        );
        assert_eq!(
            labels(&parsed),
            vec!["Trust and continue", "Back to Agent Command Center"]
        );
        assert_eq!(parsed.selected_index, 0);
        assert_eq!(parsed.reject_with_escape_index, None);
        assert_eq!(
            texts(&plan_reference_codex_answer(&parsed, &answer(Some(0), None, None)).unwrap()),
            vec![keys_of(&["enter"])]
        );
    }

    /// `codex-model`: the list of models, whose rows open the next list. The row's own key is the
    /// screen's to name, so the plan ends on an unresolved `pick`.
    #[test]
    fn the_model_list_plans_moves_and_leaves_the_row_key_to_the_screen() {
        let parsed = parse_reference_codex_prompt(MODEL_LIST).expect("the model list parses");
        assert_eq!(parsed.responder, ReferenceCodexResponder::Model);
        assert_eq!(parsed.prompt.kind, ReferencePromptKind::Question);
        assert_eq!(parsed.prompt.title, "");
        assert_eq!(parsed.prompt.body, None);
        assert_eq!(parsed.prompt.multi_select, false);
        assert_eq!(parsed.prompt.custom_option_index, None);
        assert_eq!(
            parsed.prompt.question,
            "Select model for this session (currently GPT-6-Astra)"
        );
        assert_eq!(parsed.prompt.options.len(), 7);
        assert_eq!(
            parsed.prompt.options[0].label,
            "GPT-6.1-Sol (default)"
        );
        assert_eq!(
            parsed.prompt.options[0].description.as_deref(),
            Some("Latest workhorse model for coding and everyday work.")
        );
        assert_eq!(parsed.selected_index, 1);
        assert_eq!(parsed.row_key, Some(ReferenceKeyStep::keys(["enter"])));

        let planned = plan_reference_codex_answer(&parsed, &answer(Some(3), None, None)).unwrap();
        assert_eq!(
            planned,
            vec![
                keys_of(&["down"]),
                keys_of(&["down"]),
                ReferenceCodexAnswerStep::Pick
            ]
        );
        // the frozen DTO cannot carry a read: the projection refuses rather than guessing
        assert!(reference_key_steps(&planned).is_err());
        assert_eq!(
            plan_reference_codex_answer(&parsed, &answer(Some(1), None, None)).unwrap(),
            vec![ReferenceCodexAnswerStep::Pick]
        );
        // the row under the cursor names its own key off the screen
        assert_eq!(
            codex_model_row_key("enter select · esc back"),
            Some(ReferenceKeyStep::keys(["enter"]))
        );
        assert_eq!(
            codex_model_row_key("enter default · s session · esc back"),
            Some(ReferenceKeyStep::typed("s"))
        );
        assert_eq!(codex_model_row_key("enter confirm · esc back"), None);
    }

    /// A model's reasoning levels: the row that opens the advanced ones is left to the terminal,
    /// and no row of these lists is answered with Enter.
    #[test]
    fn the_levels_list_offers_no_row_that_only_opens_another_list() {
        let parsed = parse_reference_codex_prompt(MODEL_LEVELS).expect("the levels parse");
        assert_eq!(parsed.responder, ReferenceCodexResponder::Model);
        assert_eq!(
            parsed.prompt.question,
            "Select reasoning level for GPT-6-Astra for this session (currently Medium). More levels are listed in the terminal."
        );
        assert_eq!(
            labels(&parsed),
            vec!["Low", "Medium (default)", "High", "Extra high"]
        );
        // "More reasoning…" is not offered, so its index is outside the card
        assert!(plan_reference_codex_answer(&parsed, &answer(Some(4), None, None)).is_err());
        assert_eq!(parsed.row_key, Some(ReferenceKeyStep::typed("s")));

        // with the cursor on that row in the terminal, its footer is a row that opens a list: the
        // same card, and the moves to a level count from where the cursor is
        let on_more =
            parse_reference_codex_prompt(MODEL_LEVELS_MORE).expect("the more row parses");
        assert_eq!(on_more.prompt.id, parsed.prompt.id);
        assert_eq!(on_more.selected_index, 4);
        assert_eq!(on_more.row_key, None);
        assert_eq!(
            plan_reference_codex_answer(&on_more, &answer(Some(2), None, None)).unwrap(),
            vec![
                keys_of(&["up"]),
                keys_of(&["up"]),
                ReferenceCodexAnswerStep::Pick
            ]
        );
    }

    /// The advanced levels carry what Codex says over them, and a phone's narrow pane wraps the
    /// descriptions under the rows without changing the card.
    #[test]
    fn the_advanced_levels_carry_their_note_and_survive_a_narrow_pane() {
        let wide = parse_reference_codex_prompt(MODEL_ADVANCED).expect("the advanced list parses");
        assert_eq!(wide.prompt.question, "Select advanced reasoning for this session");
        assert_eq!(
            wide.prompt.body.as_deref(),
            Some("⚠ Consumes usage limits faster")
        );
        assert_eq!(labels(&wide), vec!["Max", "Ultra"]);
        assert_eq!(
            wide.prompt.options[0].description.as_deref(),
            Some("For difficult problems when quality matters more than speed · higher usage")
        );

        let phone =
            parse_reference_codex_prompt(MODEL_ADVANCED_PHONE).expect("the phone capture parses");
        assert_eq!(phone.prompt.options, wide.prompt.options);
        assert_eq!(phone.prompt.id, wide.prompt.id);
        assert_eq!(phone.selected_index, 1);
        assert_eq!(phone.prompt.body, wide.prompt.body);
    }

    /// A narrow pane leaves the second column out: the rows are the names alone, and each one's
    /// description is absent rather than invented.
    #[test]
    fn the_levels_list_draws_the_names_alone_where_the_pane_has_no_room() {
        let parsed =
            parse_reference_codex_prompt(MODEL_LEVELS_NAMES_ONLY).expect("the names-only list parses");
        assert_eq!(parsed.responder, ReferenceCodexResponder::Model);
        assert_eq!(
            parsed.prompt.question,
            "Select reasoning level for GPT-6-Astra for this session (currently Medium). More levels are listed in the terminal."
        );
        assert_eq!(labels(&parsed), vec!["Low", "Medium (default)", "High", "Extra high"]);
        assert!(parsed
            .prompt
            .options
            .iter()
            .all(|option| option.description.is_none()));
    }

    /// A pane too narrow for the levels' title wraps the model's name under it, and the two are
    /// read as one title.
    #[test]
    fn the_levels_list_reads_a_title_the_pane_wrapped() {
        let parsed = parse_reference_codex_prompt(MODEL_LEVELS_WRAPPED_TITLE)
            .expect("the wrapped title parses");
        assert_eq!(
            parsed.prompt.question,
            "Select reasoning level for GPT-5.6-Terra for this session (currently Medium)"
        );
        assert_eq!(parsed.prompt.body, None);
        assert_eq!(labels(&parsed), vec!["Low", "Medium (default)", "High"]);
    }

    /// A gap inside one row's name is part of the name when no column is shared, and is left out of
    /// what the rows say when the other rows do share one.
    #[test]
    fn a_gap_inside_one_rows_name_is_read_by_the_column_the_rows_share() {
        let names =
            parse_reference_codex_prompt(MODEL_LIST_GAP_IN_NAME).expect("the names alone parse");
        assert_eq!(
            names.prompt.options,
            vec![
                ReferencePromptOption {
                    label: "Custom Model".to_string(),
                    description: None
                },
                ReferencePromptOption {
                    label: "GPT-6-Sol".to_string(),
                    description: None
                },
                ReferencePromptOption {
                    label: "GPT-6-Luna".to_string(),
                    description: None
                }
            ]
        );
        assert_eq!(
            names.prompt.question,
            "Select model for this session (currently Custom Model)"
        );

        let beside = parse_reference_codex_prompt(MODEL_LIST_GAP_IN_NAME_BESIDE)
            .expect("the list with one row drawn narrow parses");
        assert_eq!(
            beside.prompt.options[2],
            ReferencePromptOption {
                label: "GPT 6 Sol".to_string(),
                description: None
            }
        );
        assert_eq!(
            beside.prompt.options[3],
            ReferencePromptOption {
                label: "GPT-6-Luna".to_string(),
                description: Some("Fast and affordable model for easier tasks.".to_string())
            }
        );
    }

    /// The footer belongs to every Codex list, but only the model lists' titles make it this card:
    /// another list, a list whose footer names no key this reader knows, an answered list, a list
    /// with no cursor or one row, and anything printed between the rows and their footer are all
    /// left to the terminal.
    #[test]
    fn a_list_that_is_not_the_card_is_refused() {
        for (name, screen) in [
            ("another list's title", MODEL_LIST_OTHER_TITLE),
            ("the quick presets", MODEL_LIST_PRESETS),
            ("a footer this reader does not know", MODEL_LEVELS_CONFIRM_FOOTER),
            ("an answered list", STALE_MODEL_ANSWERED),
            ("no cursor on a row", MODEL_LEVELS_NO_CURSOR),
            ("one row alone", MODEL_LIST_SINGLE_ROW),
            ("output between the rows and their footer", MODEL_LEVELS_LOADING),
        ] {
            assert!(
                parse_reference_codex_prompt(screen).is_none(),
                "{name} must not become a card"
            );
        }
        // the guard alone still knows the list holds the screen, so the dispatcher keeps its
        // fallback card — whose Enter would save the row as the default — off it
        assert!(codex_model_list_waits(MODEL_LEVELS_CONFIRM_FOOTER));
    }

    /// Stale: a menu or approval whose hint ended above later output is not the thing at the
    /// bottom of the screen any more, so it is refused instead of answered into whatever is.
    #[test]
    fn a_prompt_whose_hint_ended_above_later_output_is_refused() {
        assert!(parse_reference_codex_prompt(STALE_MENU).is_none());
        assert!(parse_reference_codex_prompt(STALE_APPROVAL).is_none());
        assert!(!codex_questions_collapsed(STALE_MENU));
    }

    /// Ambiguous: rows that do not count on from one another, or two rows carrying the cursor,
    /// are not a menu the answer could be aimed at.
    #[test]
    fn a_menu_the_reader_cannot_aim_at_is_refused() {
        assert!(parse_reference_codex_prompt(AMBIGUOUS_DOUBLE_CURSOR).is_none());
        // a numbered menu under the queue holds the input itself: not the collapsed queue
        assert!(parse_reference_codex_prompt(AMBIGUOUS_QUEUE_MENU).is_none());
        assert_eq!(queued_question_count(AMBIGUOUS_QUEUE_MENU), 0);
        assert!(!codex_questions_collapsed(AMBIGUOUS_QUEUE_MENU));
    }

    /// The id names the content and excludes the cursor: a move is not a new prompt, a different
    /// command is, and a change beyond the display cap is too.
    #[test]
    fn the_prompt_id_names_the_content_and_not_the_cursor() {
        let first = parse_reference_codex_prompt(MENU).expect("the menu parses");
        let moved = MENU.replace("› 1. Update now", "  1. Update now")
            .replace("  2. Skip\n", "› 2. Skip\n");
        let second = parse_reference_codex_prompt(&moved).expect("the moved menu parses");
        assert_eq!(second.selected_index, 1);
        assert_eq!(second.prompt.id, first.prompt.id);

        let changed = MENU.replace("0.146.0 -> 0.146.1", "0.147.0 -> 0.147.1");
        let third = parse_reference_codex_prompt(&changed).expect("the changed menu parses");
        assert_ne!(third.prompt.id, first.prompt.id);

        let long = format!("{}x", "y".repeat(REFERENCE_CODEX_BODY_CAP + 10));
        let screen = |tail: &str| {
            format!(
                "{long}{tail}\n\n› 1. Update now\n  2. Skip\n  3. Skip until next version\n\nPress enter to continue\n"
            )
        };
        let a = parse_reference_codex_prompt(&screen("a")).expect("the long menu parses");
        let b = parse_reference_codex_prompt(&screen("b")).expect("the long menu parses");
        assert_ne!(a.prompt.id, b.prompt.id);
        assert_eq!(
            a.prompt.body.as_ref().map(|body| body.chars().count()),
            Some(REFERENCE_CODEX_BODY_CAP)
        );
    }

    /// An answer shape the card does not take is refused rather than resolved by precedence.
    #[test]
    fn an_answer_shape_the_card_does_not_take_is_refused() {
        let question = parse_reference_codex_prompt(QUESTION).expect("the question parses");
        assert!(plan_reference_codex_answer(
            &question,
            &answer(Some(0), None, Some("also"))
        )
        .is_err());
        assert!(
            plan_reference_codex_answer(&question, &answer(None, Some(vec![0]), None)).is_err()
        );
        assert!(plan_reference_codex_answer(&question, &answer(Some(99), None, None)).is_err());
        // the typed row itself is not an option
        assert!(plan_reference_codex_answer(&question, &answer(Some(3), None, None)).is_err());
        assert!(plan_reference_codex_answer(&question, &answer(None, None, Some("   "))).is_err());

        let menu = parse_reference_codex_prompt(MENU).expect("the menu parses");
        assert!(plan_reference_codex_answer(&menu, &answer(None, None, Some("text"))).is_err());
    }

    /// A model list that holds the end of the screen is reported, so the dispatcher can keep its
    /// fallback card — whose Enter would save the row as the default — off it.
    #[test]
    fn a_model_list_holding_the_end_of_the_screen_is_reported() {
        assert!(codex_model_list_waits(MODEL_LEVELS));
        assert!(codex_model_list_waits(MODEL_LEVELS_MORE));
        assert!(codex_model_list_waits(MODEL_ADVANCED));
        // the quick-preset list is recognized only by the guard, never offered as a card
        assert!(!codex_model_list_waits("› Ask Codex to do anything\n"));
        assert!(!codex_model_list_waits(APPROVAL_COMMAND));
    }

    /// The pinned responder union is the inventory: every name in it has a branch here.
    #[test]
    fn every_named_codex_responder_has_a_branch() {
        assert_eq!(
            REFERENCE_CODEX_RESPONDERS
                .iter()
                .map(|responder| responder.as_str())
                .collect::<Vec<_>>(),
            vec![
                "codex-menu",
                "codex-question",
                "codex-async-question",
                "codex-approval",
                "codex-model",
                "codex-queued-question"
            ]
        );
        let reached = [
            parse_reference_codex_prompt(MENU).map(|parsed| parsed.responder),
            parse_reference_codex_prompt(QUESTION).map(|parsed| parsed.responder),
            parse_reference_codex_prompt(ASYNC_QUESTION).map(|parsed| parsed.responder),
            parse_reference_codex_prompt(APPROVAL_COMMAND).map(|parsed| parsed.responder),
            parse_reference_codex_prompt(MODEL_LIST).map(|parsed| parsed.responder),
            codex_queued_prompt(
                QUEUED_COLLAPSED,
                &[queued_question("call_c:0", "Which dataset?", &["LM-O"])],
                None,
            )
            .map(|parsed| parsed.responder),
        ];
        for responder in REFERENCE_CODEX_RESPONDERS {
            assert!(
                reached.contains(&Some(responder)),
                "{} has no fixture reaching it",
                responder.as_str()
            );
        }
    }
}
