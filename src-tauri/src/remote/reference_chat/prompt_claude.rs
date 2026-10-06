//! `[CC]` structured-prompt family for the Herdr reference-chat port.
//!
//! Ported from `devswha/herdr-web-ui` @ `54e5a1f67090cb09552d182e7e30dd0ecc314918`
//! (MIT, `docs/chat/HERDR_LICENSE`), the claude arm of the pinned prompt reader:
//!
//! * `server/prompt.ts` — the responders `claude-question`, `claude-submit`,
//!   `claude-approval`, `claude-plan`, `claude-confirm`, `claude-model` (the `Responder`
//!   union and the `agent === "claude"` arm of `parsePrompt`), their parsers, the shared
//!   row/panel helpers (`cleanLine`, `wrapped`, `nearestQuestion`, `parseNumberedRows`,
//!   `sequentialRows`, `finishPrompt`/`publicPrompt`), the list reader (`listRow`,
//!   `listRows`, `listNames`), the tail check (`promptTailIsActive`), the task-list filter
//!   (`withoutClaudeTasks`), the answer planner (`answerKeys`) and Claude's input-box
//!   suggestion (`sgrRuns`, `parseClaudeSuggestion`).
//! * `shared/protocol.ts` — `InteractivePrompt` and `PromptAnswer`.
//!
//! Upstream SHA-256 for `server/prompt.ts`: `docs/chat/herdr-port-contract.md` §7. Fixture
//! inventory, capture provenance and the deliberate divergences:
//! `fixtures/claude/prompt/MANIFEST.md`.
//!
//! **Boundary.** This lane reads the claude family only: a pane herdr labels `claude`
//! (`agent == "claude"`). Upstream's claude arm also appends `omo()` — a pane named `claude`
//! while omo's SDK child runs — but that is task 25's family (`prompt_omo.rs`); claiming it
//! here would advertise support this lane does not port, so [`detect_reference_claude_prompt`]
//! refuses every other agent id, including `""` (which upstream routes to omo alone).
//!
//! **Card ids.** [`ReferencePrompt::id`] is minted as upstream's `finishPrompt` does —
//! `sha256(canonical card JSON).slice(0,12)` over the *uncapped* body — so a card that says
//! something else gets another id, and the same card keeps it while only the cursor moves.
//! The bytes are not upstream's for text holding an astral character (see the manifest).
//!
//! **Planning.** Upstream recovers a card's cursor and responder from a `WeakMap` keyed by the
//! public prompt object. Rust's equivalent is to keep the parsed card: this lane returns
//! [`ClaudePrompt`] (the public card plus that internal state) and plans from it with
//! [`plan_claude_answer`]. No lossy inverse from `&ReferencePrompt` is shipped.

use std::sync::OnceLock;

use regex::Regex;
use sha2::{Digest, Sha256};

use super::types::{
    ReferenceKeyStep, ReferencePrompt, ReferencePromptAnswer, ReferencePromptDetector,
    ReferencePromptKind, ReferencePromptOption,
};

/// Upstream `finishPrompt`'s id: `sha256(...).digest("hex").slice(0, 12)`.
pub const REFERENCE_CLAUDE_PROMPT_ID_CHARS: usize = 12;

/// Upstream `finishPrompt`'s display cap on `body`, applied **after** the id is hashed.
pub const REFERENCE_CLAUDE_BODY_CAP: usize = 12_000;

/// Upstream's window above a question's hint that its numbered rows are read from (64 lines).
pub const REFERENCE_CLAUDE_ROW_WINDOW: usize = 64;

/// Upstream's window above the first row that Claude's question tabs are looked for in (60).
pub const REFERENCE_CLAUDE_TABS_WINDOW: usize = 60;

/// Upstream's window above the first row that a single question's chip is looked for in (40).
pub const REFERENCE_CLAUDE_CHIP_WINDOW: usize = 40;

/// Upstream's window above a question that its own text is joined back over (30 lines).
pub const REFERENCE_CLAUDE_QUESTION_WINDOW: usize = 30;

/// Upstream's window of lines before a list's hint that its rows are read from (80).
pub const REFERENCE_CLAUDE_LIST_WINDOW: usize = 80;

/// Upstream's cap on the non-blank lines between a model list's rows and its hint.
pub const REFERENCE_CLAUDE_MODEL_UNDER_LINES: usize = 3;

/// Upstream's cap on lines between an approval panel's rule and the question it asks (60).
pub const REFERENCE_CLAUDE_APPROVAL_PANEL_WINDOW: usize = 60;

/// Upstream's window above an approval's marker that its panel's lines are read from (8).
pub const REFERENCE_CLAUDE_APPROVAL_PANEL_BACK: usize = 8;

/// Upstream's cap on lines above the rows that a confirm menu's panel is read from (30).
pub const REFERENCE_CLAUDE_CONFIRM_PANEL_WINDOW: usize = 30;

/// Upstream's cap on the lines a confirm menu's rows may wrap onto (9 rows).
pub const REFERENCE_CLAUDE_CONFIRM_MAX_ROWS: usize = 9;

/// Upstream's window of the screen's end that `promptTailIsActive` reads a hint from.
pub const REFERENCE_CLAUDE_HINT_TAIL_LINES: usize = 3;

/// Upstream's cap on lines between a submit card's tabs and its question (40).
pub const REFERENCE_CLAUDE_SUBMIT_TABS_WINDOW: usize = 40;

/// Upstream's window above a row that `nearestQuestion` looks back through (14 lines).
pub const REFERENCE_CLAUDE_NEAREST_WINDOW: usize = 14;

/// The registry id this family reads: upstream's `agent === "claude"` arm.
pub const REFERENCE_CLAUDE_AGENT: &str = "claude";

/// Upstream `Responder` values this family mints.
pub const RESPONDER_CLAUDE_QUESTION: &str = "claude-question";
pub const RESPONDER_CLAUDE_SUBMIT: &str = "claude-submit";
pub const RESPONDER_CLAUDE_APPROVAL: &str = "claude-approval";
pub const RESPONDER_CLAUDE_PLAN: &str = "claude-plan";
pub const RESPONDER_CLAUDE_CONFIRM: &str = "claude-confirm";
pub const RESPONDER_CLAUDE_MODEL: &str = "claude-model";

/// Upstream `KEY` names this family plans with.
const KEY_UP: &str = "up";
const KEY_DOWN: &str = "down";
const KEY_ENTER: &str = "enter";
const KEY_RIGHT: &str = "right";
const KEY_BACKTAB: &str = "shift+tab";

/// The key Claude's `/model` list takes a pick with: `s` keeps it to this session, where Enter
/// would save it as the default for every new one.
const CLAUDE_MODEL_PICK_KEY: &str = "s";

/// The responders whose custom answer is typed **without** an Enter first (upstream
/// `answerKeys`): on these menus the typed row already owns the input.
const CUSTOM_ANSWER_WITHOUT_ENTER: [&str; 5] = [
    RESPONDER_CLAUDE_QUESTION,
    RESPONDER_CLAUDE_PLAN,
    "codex-question",
    "codex-async-question",
    "pi-input",
];

/// Upstream `PREVIEW_EDGE`: the box-drawing characters a preview's own column can hold.
const PREVIEW_EDGE: &str = "┌│└├╭╰┐┘╮╯";

// ---------------------------------------------------------------------------------------
// The parsed card
// ---------------------------------------------------------------------------------------

/// A prompt this family detected: the public card plus the internal state upstream keeps in
/// `parsedByPublicPrompt` (`Responder`, the row labels, the cursor, what is checked, the typed
/// row's menu index, and a list's own option steps). Task 9's dispatcher holds this and plans
/// answers from it.
#[derive(Debug, Clone, PartialEq)]
pub struct ClaudePrompt {
    /// The card a client renders and an answer names.
    pub prompt: ReferencePrompt,
    /// Upstream's `responder` for this card.
    pub responder: String,
    /// Every row of the menu as the reader saw it, including rows the card does not offer.
    pub menu_labels: Vec<String>,
    /// Where the menu's cursor stood when the card was read.
    pub selected_index: usize,
    /// The offered rows that were already checked (multi-select).
    pub checked_option_indices: Vec<usize>,
    /// The menu row that takes a typed answer, when the menu has one.
    pub custom_menu_index: Option<usize>,
    /// Each offered row's own steps, for a card whose rows do not all take the same key
    /// (Claude's `/model` list, picked with `s` and never Enter). Upstream `optionSteps`.
    pub option_steps: Option<Vec<Vec<ReferenceKeyStep>>>,
}

impl ClaudePrompt {
    /// Does the screen still end on this card's own menu (upstream `promptTailIsActive`)?
    ///
    /// A card whose menu was answered in the terminal keeps its text in the buffer; the answer
    /// must not be sent into whatever took the screen after it.
    pub fn tail_is_active(&self, screen: &str) -> bool {
        prompt_tail_is_active(&self.responder, screen)
    }
}

/// A numbered row of a claude menu, as `parseNumberedRows` reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ClaudeRow {
    number: usize,
    label: String,
    description: Option<String>,
    selected: bool,
    checked: bool,
    line_index: usize,
}

/// A row of Claude's `/model` list, as `listRow` reads it.
#[derive(Debug, Clone)]
struct ListRow {
    number: usize,
    cursor: bool,
    /// `↑` or `↓` in the cursor's column: the window's first or last row, more beyond it.
    edge: Option<char>,
    /// The row after its number, as drawn.
    text: String,
    /// The first run of two or more spaces in `text`: where it begins and ends, and the
    /// terminal column the text after it is drawn at (-1 when the row has no such gap).
    gap: i64,
    resumes: i64,
    column: i64,
    /// The lines wrapped under the row at that column.
    wrapped: Vec<String>,
}

/// What `listRows` read out of a screen.
#[derive(Debug, Clone)]
struct ListRows {
    rows: Vec<ListRow>,
    /// The rows a `… +N models` line under them counts.
    below: usize,
    first: i64,
    last: i64,
    /// A line under a row that is not its wrapped text: the rows are not the list as drawn.
    unread: bool,
}

/// Claude's question tabs, `←  ☒ Route  ☐ Author  ✔ Submit  →`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ClaudeTabs {
    index: usize,
    /// Whether the bar was whole (`→` at its end): a cut-off bar does not say how many there
    /// are.
    whole: bool,
    tabs: Vec<ClaudeTab>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ClaudeTab {
    label: String,
    answered: bool,
}

// ---------------------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------------------

/// Read the claude prompt on a screen, in the pinned arm's order: question, submit,
/// plan/approval, confirm, model — the first one whose menu still ends the screen.
pub fn parse_claude_prompt(screen: &str) -> Option<ClaudePrompt> {
    let candidates = [
        parse_claude_question(screen),
        parse_claude_submit(screen),
        parse_claude_approval(screen),
        parse_claude_confirm(screen),
        parse_claude_model(screen),
    ];
    candidates
        .into_iter()
        .flatten()
        .find(|candidate| candidate.tail_is_active(screen))
}

/// The lane's detector, satisfying task 1's [`ReferencePromptDetector`] alias.
///
/// Refuses every agent id but `claude`: this lane ports the claude family alone (see the
/// module docs), and a detector must not claim a provider the reference does not name.
pub fn detect_reference_claude_prompt(agent: &str, screen: &str) -> Option<ReferencePrompt> {
    if agent != REFERENCE_CLAUDE_AGENT {
        return None;
    }
    parse_claude_prompt(screen).map(|parsed| parsed.prompt)
}

/// Compile-time proof that the lane satisfies the frozen detector signature (contract §6).
const _: ReferencePromptDetector = detect_reference_claude_prompt;

/// Turn an answer into the keys that answer the card on the original pane.
///
/// The lane-shaped twin of upstream `answerKeys`, planning from the parsed card (upstream's
/// `parsedByPublicPrompt` map). Errors carry upstream's own wording, so a caller maps them to
/// the contract's `INVALID_REQUEST` without a second table.
pub fn plan_claude_answer(
    parsed: &ClaudePrompt,
    answer: &ReferencePromptAnswer,
) -> Result<Vec<ReferenceKeyStep>, String> {
    if answer.variant_count() != 1 {
        return Err("Exactly one answer is required.".to_string());
    }
    let options_len = parsed.prompt.options.len();

    if let Some(raw_text) = answer.custom_text.as_deref() {
        let text = raw_text.trim();
        let typed_row = parsed
            .custom_menu_index
            .filter(|_| !parsed.prompt.multi_select);
        let Some(typed_row) = typed_row.filter(|_| !text.is_empty()) else {
            return Err("This prompt does not accept a custom answer.".to_string());
        };
        let mut steps = key_steps(navigation_keys(typed_row as i64 - parsed.selected_index as i64));
        // Codex's queue types into its last row once it is selected: no enter first. pi's text
        // dialog is the same, and pi's line is emptied before the answer goes in.
        if !CUSTOM_ANSWER_WITHOUT_ENTER.contains(&parsed.responder.as_str()) {
            steps.push(ReferenceKeyStep::keys([KEY_ENTER]));
        }
        steps.push(ReferenceKeyStep::typed(text));
        if parsed.responder == RESPONDER_CLAUDE_PLAN {
            steps.push(ReferenceKeyStep::keys([KEY_BACKTAB]));
        } else {
            steps.push(ReferenceKeyStep::keys([KEY_ENTER]));
        }
        return Ok(steps);
    }

    if let Some(indices) = answer.option_indices.as_deref() {
        if !parsed.prompt.multi_select || indices.is_empty() {
            return Err("This prompt requires one or more selections.".to_string());
        }
        let mut choices: Vec<usize> = Vec::new();
        for index in indices {
            let index = *index as usize;
            if index >= options_len {
                return Err("An option index is outside the displayed range.".to_string());
            }
            if !choices.contains(&index) {
                choices.push(index);
            }
        }
        if parsed.responder != RESPONDER_CLAUDE_QUESTION {
            return Err("This agent does not support multiple selections.".to_string());
        }
        let checked = &parsed.checked_option_indices;
        let toggles: Vec<usize> = (0..options_len)
            .filter(|index| choices.contains(index) != checked.contains(index))
            .collect();
        let mut keys: Vec<String> = Vec::new();
        let mut cursor = parsed.selected_index as i64;
        for option_index in toggles {
            keys.extend(navigation_keys(option_index as i64 - cursor));
            keys.push(KEY_ENTER.to_string());
            cursor = option_index as i64;
        }
        // → leaves the choice for the next question or the review of the answers, never an
        // enter: on the next question it would pick that question's first option
        keys.push(KEY_RIGHT.to_string());
        return Ok(key_steps(keys));
    }

    let Some(index) = answer.option_index else {
        return Err("A valid option index is required.".to_string());
    };
    let index = index as usize;
    if index >= options_len
        || Some(index) == parsed.custom_menu_index
        || parsed.prompt.multi_select
    {
        return Err("A valid option index is required.".to_string());
    }
    // a card whose rows do not all take the same key carries their own steps
    if let Some(steps) = parsed.option_steps.as_ref().and_then(|steps| steps.get(index)) {
        return Ok(steps.clone());
    }
    let mut keys = navigation_keys(index as i64 - parsed.selected_index as i64);
    keys.push(KEY_ENTER.to_string());
    Ok(key_steps(keys))
}

/// Does the screen still end on the menu a `responder` names (upstream `promptTailIsActive`)?
///
/// Claude keeps its task list under an open panel, so the lines it reads are the screen's with
/// that list taken off first.
pub fn prompt_tail_is_active(responder: &str, screen: &str) -> bool {
    let clean_lines: Vec<String> = split_lines(screen)
        .iter()
        .map(|line| clean_line(line))
        .collect();
    let visible: Vec<String> = clean_lines
        .iter()
        .filter(|line| !line.is_empty() && !is_divider(line))
        .cloned()
        .collect();
    let shown = if responder.starts_with("claude-") {
        without_claude_tasks(&visible)
    } else {
        visible
    };
    let last = shown.last().cloned().unwrap_or_default();
    if responder == RESPONDER_CLAUDE_QUESTION {
        return ends(&shown, claude_ask_hint());
    }
    if responder == RESPONDER_CLAUDE_SUBMIT {
        return claude_submit_cancel_row().is_match(&last);
    }
    if responder == RESPONDER_CLAUDE_APPROVAL {
        return ends(&shown, claude_approval_tail());
    }
    if responder == RESPONDER_CLAUDE_CONFIRM {
        return ends(&shown, claude_confirm_hint());
    }
    if responder == RESPONDER_CLAUDE_MODEL {
        return ends(&shown, claude_model_hint());
    }
    // the plan card's own last line
    ends(&shown, claude_plan_tail())
}

/// Whether `[CC]`'s `/model` list holds the end of the screen, read or not: by its hint alone,
/// so a list whose rows a narrow pane cut in two still gets no fallback card — that card offers
/// Enter, and Enter on this list saves the row under the cursor as the default for every new
/// session.
pub fn claude_model_list_waits(screen: &str) -> bool {
    let visible: Vec<String> = split_lines(screen)
        .iter()
        .map(|line| clean_line(line))
        .filter(|line| !line.is_empty() && !is_divider(line))
        .collect();
    let shown = without_claude_tasks(&visible);
    // wider than the reader's own window: a hint wrapped further than it reads is still this
    // list's
    (1..=6usize).any(|span| {
        let from = shown.len().saturating_sub(span);
        claude_model_hint().is_match(&shown[from..].join(" "))
    })
}

/// The screen's lines with `[CC]`'s task list taken off its end, when that list is the last
/// thing on it: the session's rule, `N tasks (…)`, a row per task, the activity under the task
/// in progress, and `… +N pending`.
///
/// Only Claude's own footer goes, and only where it sits directly under the panel's hint.
pub fn without_claude_tasks(shown: &[String]) -> Vec<String> {
    // the screen's lines as the pinned reader sees them (every line through `cleanLine`), so a
    // caller that hands them as drawn still gets Claude's own footer taken off
    let shown: Vec<String> = shown.iter().map(|line| clean_line(line)).collect();
    let mut end = shown.len();
    if let Some(head) = find_last_index(&shown, |line, _| claude_tasks_head().is_match(line)) {
        let total = claude_tasks_head()
            .captures(&shown[head])
            .and_then(|captures| captures.get(1))
            .and_then(|number| number.as_str().parse::<usize>().ok())
            .unwrap_or(0);
        let mut rows = 0usize;
        let mut in_progress = false;
        let mut list = true;
        let mut index = head + 1;
        while index < shown.len() && list {
            let line = &shown[index];
            if claude_task_row().is_match(line) {
                rows += 1;
                in_progress = line.starts_with('◼');
            } else if claude_tasks_more().is_match(line) {
                list = index == shown.len() - 1;
            } else if in_progress
                && line.ends_with('…')
                && !claude_task_activity_guard().is_match(line)
            {
                // the activity under the task in progress: one line, ending in an ellipsis
                in_progress = false;
            } else {
                list = false;
            }
            index += 1;
        }
        if list && rows > 0 && rows <= total {
            end = head;
        }
    }
    // the session's rule is drawn above the list, and with no task list too
    if end > 0 && labeled_rule().is_match(&shown[end - 1]) {
        end -= 1;
    }
    if end < shown.len() && end > 0 && claude_hint_tail().is_match(&shown[end - 1]) {
        shown[..end].to_vec()
    } else {
        shown.to_vec()
    }
}

/// The prompt `[CC]` suggests next, grey in its empty input box: the `❯` line between the
/// screen's last two rules, all of it dim but for Claude's own drawn cursor on its first
/// character. `None` while anything is typed there, for the new-session tip, or for an input
/// box of more than one line.
///
/// Reads ANSI: the dim runs are the whole evidence, so this takes the pane's raw read.
pub fn parse_claude_suggestion(ansi: &str) -> Option<String> {
    let lines: Vec<String> = ansi
        .split('\n')
        .map(|line| line.trim_end_matches('\r').to_string())
        .collect();
    let plain: Vec<String> = lines
        .iter()
        .map(|line| {
            osc()
                .replace_all(&ansi_escape().replace_all(line, ""), "")
                .to_string()
        })
        .collect();

    let mut index = plain.len() as i64 - 1;
    while index >= 0 && !solid_rule().is_match(plain[index as usize].trim()) {
        index -= 1;
    }
    index -= 1;
    if index < 1 {
        return None;
    }
    let line = &plain[index as usize];
    let mut characters = line.chars();
    let starts_with_prompt = characters.next() == Some('❯');
    let second_is_space = characters
        .next()
        .is_some_and(|next| next.is_whitespace() || next == '\u{a0}');
    if !starts_with_prompt
        || !second_is_space
        || !solid_rule().is_match(plain[index as usize - 1].trim())
    {
        return None;
    }

    let raw = &lines[index as usize];
    let mut text = String::new();
    let mut seen_prompt = false;
    let mut cursor = false;
    for (run, dim, inverse) in sgr_runs(raw) {
        for character in run.chars() {
            if !seen_prompt {
                if character == '❯' {
                    seen_prompt = true;
                }
                continue;
            }
            let blank = character.is_whitespace() || character == '\u{a0}';
            // the cursor Claude draws itself sits inverse on the first grey character
            if !dim && !blank && !(inverse && text.trim().is_empty() && !cursor) {
                return None;
            }
            if !dim && !blank {
                cursor = true;
            }
            text.push(character);
        }
    }
    // a cursor over typed text has nothing grey after it
    if cursor
        && !sgr_runs(raw)
            .iter()
            .any(|(run, dim, _)| *dim && !run.trim().is_empty())
    {
        return None;
    }
    let suggestion = text.replace('\u{a0}', " ").trim().to_string();
    if suggestion.is_empty() || claude_tip().is_match(&suggestion) {
        None
    } else {
        Some(suggestion)
    }
}

// ---------------------------------------------------------------------------------------
// Parsers
// ---------------------------------------------------------------------------------------

/// Upstream `parseClaudeQuestion`: a numbered question with its `Type something.` row and
/// `Chat about this` last; the option-preview form; and the question tabs above several
/// questions.
fn parse_claude_question(screen: &str) -> Option<ClaudePrompt> {
    let raw = split_lines(screen);
    let hint_index = find_last_index(&raw, |_, index| {
        claude_ask_hint().is_match(&wrapped(&raw, index, 3))
    })?;
    let preview = claude_preview_hint().is_match(&wrapped(&raw, hint_index, 3));
    let lines: Vec<String> = if preview {
        without_preview(
            &raw,
            hint_index.saturating_sub(REFERENCE_CLAUDE_ROW_WINDOW),
            hint_index,
        )
    } else {
        raw.clone()
    };
    // with a preview the options end at the rule above "Chat about this": nothing under it is
    // theirs
    let end = if preview {
        find_last_index(&lines[..hint_index], |line, _| is_divider(line)).unwrap_or(0)
    } else {
        hint_index
    };
    let rows = parse_numbered_rows(
        &lines,
        hint_index.saturating_sub(REFERENCE_CLAUDE_ROW_WINDOW),
        end,
    );
    if !sequential_rows(&rows) || rows.iter().filter(|row| row.selected).count() != 1 {
        return None;
    }
    let chat_index = rows
        .iter()
        .position(|row| row.label == "Chat about this");
    let custom_index = rows
        .iter()
        .position(|row| typed_answer().is_match(&row.label));
    if preview {
        // its notes are no answer of their own: no typed-answer row, the options are the menu
        if custom_index.is_some() || chat_index.is_some() {
            return None;
        }
    } else {
        let expected_custom = chat_index.and_then(|chat| chat.checked_sub(1));
        if chat_index != Some(rows.len() - 1)
            || expected_custom != custom_index
            || custom_index.is_none_or(|custom| custom < 1)
        {
            return None;
        }
    }
    let tabs = claude_tabs(&lines, rows[0].line_index);
    let question = claude_question_text(
        &lines,
        tabs.as_ref().map(|tabs| tabs.index),
        rows[0].line_index,
    )
    .or_else(|| nearest_question(&lines, rows[0].line_index))?;
    let chip = if tabs.is_none() {
        claude_chip(&lines, rows[0].line_index)
    } else {
        None
    };
    // the preview form has no typed row: its options are the menu
    let option_rows: Vec<&ClaudeRow> = match custom_index {
        Some(custom_index) if !preview => rows[..custom_index].iter().collect(),
        _ => rows.iter().collect(),
    };
    let multi_select = option_rows
        .iter()
        .any(|row| multi_select_row().is_match(&lines[row.line_index]));
    let current = tabs
        .as_ref()
        .and_then(|tabs| tabs.tabs.iter().position(|tab| !tab.answered));
    let title = match (&tabs, current) {
        (Some(tabs), Some(current)) => {
            let mut title = tabs.tabs[current].label.clone();
            // a bar cut off by a narrow pane does not show how many questions there are
            if tabs.whole && tabs.tabs.len() > 1 {
                title.push_str(&format!(" · {} of {}", current + 1, tabs.tabs.len()));
            }
            title
        }
        _ => chip.unwrap_or_else(|| {
            if multi_select {
                "Multiple choice".to_string()
            } else {
                "Question".to_string()
            }
        }),
    };
    let options: Vec<ReferencePromptOption> = option_rows
        .iter()
        .map(|row| ReferencePromptOption {
            label: row.label.clone(),
            description: row.description.clone(),
        })
        .collect();
    let checked = option_rows
        .iter()
        .enumerate()
        .filter(|(_, row)| row.checked)
        .map(|(index, _)| index)
        .collect();
    Some(finish(
        CardParts {
            kind: ReferencePromptKind::Question,
            title,
            question,
            body: None,
            options,
            multi_select,
            custom_option_index: if multi_select || preview {
                None
            } else {
                custom_index.map(|custom| custom as u32)
            },
        },
        RESPONDER_CLAUDE_QUESTION,
        rows.iter().map(|row| row.label.clone()).collect(),
        rows.iter().position(|row| row.selected).unwrap_or(0),
        checked,
        if preview { None } else { custom_index },
        None,
    ))
}

/// Upstream `parseClaudeSubmit`: after several questions Claude shows the answers and asks
/// before sending them. A menu, not a question: a typed pick submits every answer at once, so
/// it waits for Confirm.
fn parse_claude_submit(screen: &str) -> Option<ClaudePrompt> {
    let lines = split_lines(screen);
    let question_index = find_last_index(&lines, |line, _| {
        ready_to_submit().is_match(&clean_line(line))
    })?;
    let tabs_index = find_last_index(&lines[..question_index], |line, _| {
        claude_tabs_bar().is_match(&clean_line(line))
    })?;
    if question_index - tabs_index > REFERENCE_CLAUDE_SUBMIT_TABS_WINDOW {
        return None;
    }
    let rows = parse_numbered_rows(&lines, question_index + 1, lines.len());
    if !sequential_rows(&rows)
        || rows.len() < 2
        || rows.iter().filter(|row| row.selected).count() != 1
    {
        return None;
    }
    let body = lines[tabs_index + 1..question_index]
        .iter()
        .map(|line| clean_line(line))
        .filter(|line| {
            !line.is_empty() && !is_divider(line) && !review_header().is_match(line)
        })
        .collect::<Vec<String>>()
        .join("\n");
    Some(finish(
        CardParts {
            kind: ReferencePromptKind::Menu,
            title: "Review your answers".to_string(),
            question: clean_line(&lines[question_index]),
            body: (!body.is_empty()).then_some(body),
            options: rows
                .iter()
                .map(|row| ReferencePromptOption {
                    label: row.label.clone(),
                    description: None,
                })
                .collect(),
            multi_select: false,
            custom_option_index: None,
        },
        RESPONDER_CLAUDE_SUBMIT,
        rows.iter().map(|row| row.label.clone()).collect(),
        rows.iter().position(|row| row.selected).unwrap_or(0),
        Vec::new(),
        None,
        None,
    ))
}

/// Upstream `parseClaudeApproval`: the plan card (`Ready to code?` / `Claude has written up a
/// plan …`), and the tool-call approval — the marker form (`This command requires approval`,
/// `Dangerous rm operation`) and the panel form `[CC]` 2.1 draws without either marker.
fn parse_claude_approval(screen: &str) -> Option<ClaudePrompt> {
    let lines = split_lines(screen);
    let plan_index = find_last_index(&lines, |_, index| {
        plan_question().is_match(&wrapped(&lines, index, 3))
    });
    if let Some(plan_index) = plan_index {
        let rows = parse_numbered_rows(&lines, plan_index + 1, lines.len());
        if !sequential_rows(&rows)
            || rows.len() < 3
            || rows.iter().filter(|row| row.selected).count() != 1
        {
            return None;
        }
        let custom_index = rows
            .iter()
            .position(|row| tell_change().is_match(&row.label));
        let body_start = find_last_index(&lines[..plan_index], |line, _| {
            ready_to_code().is_match(&clean_line(line))
        })
        .unwrap_or(0);
        let body = lines[body_start..plan_index]
            .iter()
            .map(|line| clean_line(line))
            .filter(|line| !is_divider(line))
            .collect::<Vec<String>>()
            .join("\n");
        return Some(finish(
            CardParts {
                kind: ReferencePromptKind::Plan,
                title: "Ready to code?".to_string(),
                question: clean_line(&lines[plan_index]),
                body: (!body.is_empty()).then_some(body),
                options: rows
                    .iter()
                    .map(|row| ReferencePromptOption {
                        label: row.label.clone(),
                        description: None,
                    })
                    .collect(),
                multi_select: false,
                custom_option_index: custom_index.map(|index| index as u32),
            },
            RESPONDER_CLAUDE_PLAN,
            rows.iter().map(|row| row.label.clone()).collect(),
            rows.iter().position(|row| row.selected).unwrap_or(0),
            Vec::new(),
            custom_index,
            None,
        ));
    }

    let required_index = find_last_index(&lines, |line, _| {
        requires_approval().is_match(&clean_line(line))
    });
    let dangerous_rm_index = find_last_index(&lines, |line, _| {
        dangerous_rm().is_match(&clean_line(line))
    });
    let approval_index = required_index.max(dangerous_rm_index);
    // "Do you want to proceed?", "Do you want to create hello.txt?", "Do you want to make this
    // edit to a.ts?"
    let question_index = find_last_index(&lines, |line, _| {
        do_you_want().is_match(&clean_line(line))
    })?;
    // options end at the key hint: a line under the last one is then only its wrapped label
    let hint_index = find_last_index(&lines, |_, index| {
        esc_to_cancel().is_match(&wrapped(&lines, index, 3))
    });
    let end = match hint_index {
        Some(hint_index) if hint_index > question_index => hint_index,
        _ => lines.len(),
    };
    let rows = parse_numbered_rows(&lines, question_index + 1, end);
    if !sequential_rows(&rows)
        || rows.len() < 2
        || rows.iter().filter(|row| row.selected).count() != 1
    {
        return None;
    }
    let (title, body) = match approval_index {
        Some(approval_index) if approval_index < question_index => {
            let title = nearest_question(&lines, approval_index)
                .unwrap_or_else(|| "Command approval".to_string());
            let body_end = if dangerous_rm_index > required_index {
                question_index
            } else {
                approval_index
            };
            let body = lines[approval_index
                .saturating_sub(REFERENCE_CLAUDE_APPROVAL_PANEL_BACK)..body_end]
                .iter()
                .map(|line| clean_line(line))
                .filter(|line| !line.is_empty() && !is_divider(line))
                .collect::<Vec<String>>()
                .join("\n");
            (title, body)
        }
        _ => {
            // [CC] 2.1 has neither marker: the panel under a solid rule opens with the tool
            // ("Bash command", "Create file"), then the command or file and its description.
            // The panel's rule is the first one under the tool call (`● Write(a.ts)`): rules
            // further down belong to a file preview; with the call scrolled away, the nearest
            // rule. Claude's own text opens with ● too ("● Results table follows:"): a call is
            // a tool name and "(" (an MCP call reads "● server - tool (MCP)(…)").
            let call_index = find_last_index(&lines[..question_index], |line, _| {
                tool_call().is_match(&clean_line(line))
            });
            let rules: Vec<usize> = lines[..question_index]
                .iter()
                .enumerate()
                .filter(|(index, line)| {
                    let after_call = call_index.is_none_or(|call| *index > call);
                    after_call && solid_rule().is_match(&clean_line(line))
                })
                .map(|(index, _)| index)
                .collect();
            let rule_index = match call_index {
                Some(_) => rules.first().copied(),
                None => rules.last().copied(),
            };
            let Some(rule_index) = rule_index else {
                return None;
            };
            if question_index - rule_index > REFERENCE_CLAUDE_APPROVAL_PANEL_WINDOW {
                return None;
            }
            let panel: Vec<String> = lines[rule_index + 1..question_index]
                .iter()
                .map(|line| clean_line(line))
                .filter(|line| {
                    !line.is_empty() && !is_divider(line) && !tip_line().is_match(line)
                })
                .collect();
            let Some(title) = panel.first().cloned() else {
                return None;
            };
            (title, panel[1..].join("\n"))
        }
    };
    Some(finish(
        CardParts {
            kind: ReferencePromptKind::Approval,
            title,
            question: clean_line(&lines[question_index]),
            body: (!body.is_empty()).then_some(body),
            // an approval's options have no descriptions: lines under one are its label
            // wrapped by a narrow pane
            options: rows
                .iter()
                .enumerate()
                .map(|(index, row)| {
                    let next = rows
                        .get(index + 1)
                        .map(|row| row.line_index)
                        .unwrap_or(end);
                    let mut parts = vec![row.label.clone()];
                    for line_index in row.line_index + 1..next {
                        let line = clean_line(&lines[line_index]);
                        if !line.is_empty() && !is_divider(&line) {
                            parts.push(line);
                        }
                    }
                    ReferencePromptOption {
                        label: normalize_text(&parts.join(" ")),
                        description: None,
                    }
                })
                .collect(),
            multi_select: false,
            custom_option_index: None,
        },
        RESPONDER_CLAUDE_APPROVAL,
        rows.iter().map(|row| row.label.clone()).collect(),
        rows.iter().position(|row| row.selected).unwrap_or(0),
        Vec::new(),
        None,
        None,
    ))
}

/// Upstream `parseClaudeConfirm`: `[CC]`'s unnumbered menus, live in 2.1.285 on a folder it has
/// not seen. The rows are the lines right above the hint, up to a blank line or a rule, exactly
/// one of them `❯`; numbered rows are left to the menus above.
fn parse_claude_confirm(screen: &str) -> Option<ClaudePrompt> {
    let lines = split_lines(screen);
    let hint_index = find_last_index(&lines, |_, index| {
        claude_confirm_hint().is_match(&wrapped(&lines, index, 3))
    })?;
    let mut end = hint_index as i64 - 1;
    while end >= 0 && clean_line(&lines[end as usize]).is_empty() {
        end -= 1;
    }
    if end < 0 {
        return None;
    }
    let mut start = end;
    while start > 0
        && !clean_line(&lines[start as usize - 1]).is_empty()
        && !is_divider(&clean_line(&lines[start as usize - 1]))
    {
        start -= 1;
    }
    // A narrow pane wraps a long label onto the next line, at the label's own indent, so the
    // indent cannot tell a wrapped label from the next row. Words wrap only when the next one
    // no longer fits: a line under a row (without its own ❯) continues that row when its first
    // word would not have fitted after it. The widest line off the rows stands for the pane's
    // width; a row wider than all of them says nothing of it, and the line under it could be
    // either, so a screen like that gets no card rather than one that answers a row it does not
    // show.
    let width = lines
        .iter()
        .enumerate()
        .filter(|(index, _)| *index < start as usize || *index > end as usize)
        .map(|(_, line)| line.trim_end().chars().count())
        .max()
        .unwrap_or(0);
    let mut unsure = false;
    let mut rows: Vec<ClaudeRow> = Vec::new();
    for index in start as usize..=end as usize {
        let line = clean_line(&lines[index]);
        let selected = selected_marker().is_match(&line);
        if let Some(previous) = rows.last_mut() {
            if !selected {
                let above = &lines[index - 1];
                let above_len = above.trim_end().chars().count();
                let first_word = &line[..line.find(char::is_whitespace).unwrap_or(line.len())];
                if above_len + 1 + first_word.chars().count() > width {
                    if above_len > width {
                        unsure = true;
                    }
                    previous.label = normalize_text(&format!("{} {}", previous.label, line));
                    continue;
                }
            }
        }
        rows.push(ClaudeRow {
            number: 0,
            label: selected_marker().replace(&line, "").trim().to_string(),
            description: None,
            selected,
            checked: false,
            line_index: index,
        });
    }
    if unsure {
        return None;
    }
    if rows.len() < 2
        || rows.len() > REFERENCE_CLAUDE_CONFIRM_MAX_ROWS
        || rows.iter().filter(|row| row.selected).count() != 1
    {
        return None;
    }
    if rows
        .iter()
        .any(|row| row.label.is_empty() || numbered_option().is_match(&row.label))
    {
        return None;
    }
    // the panel above the rows: its first line names it, a sentence ending in "?" asks
    let mut top = start as i64 - 1;
    while top >= 0
        && !is_divider(&clean_line(&lines[top as usize]))
        && (start - top) <= REFERENCE_CLAUDE_CONFIRM_PANEL_WINDOW as i64
    {
        top -= 1;
    }
    // upstream's `lines.slice(top + 1, start)`: with no rule above the rows `top` is -1, and
    // the panel then starts at the screen's own first line
    let panel: Vec<String> = lines[(top + 1) as usize..start as usize]
        .iter()
        .map(|line| clean_line(line))
        .filter(|line| !line.is_empty())
        .collect();
    // upstream titles the panel with its first line exactly as drawn, colon included
    let title = panel
        .first()
        .cloned()
        .unwrap_or_else(|| "Choose an option".to_string());
    let prose = normalize_text(&panel[1..].join(" "));
    let asked = asked_question()
        .captures(&prose)
        .and_then(|captures| captures.get(1))
        .map(|question| question.as_str().trim().to_string());
    let question = asked.unwrap_or_else(|| title.clone());
    Some(finish(
        CardParts {
            kind: ReferencePromptKind::Menu,
            title,
            question,
            body: (panel.len() > 1).then(|| panel[1..].join("\n")),
            options: rows
                .iter()
                .map(|row| ReferencePromptOption {
                    label: row.label.clone(),
                    description: None,
                })
                .collect(),
            multi_select: false,
            custom_option_index: None,
        },
        RESPONDER_CLAUDE_CONFIRM,
        rows.iter().map(|row| row.label.clone()).collect(),
        rows.iter().position(|row| row.selected).unwrap_or(0),
        Vec::new(),
        None,
        None,
    ))
}

/// Upstream `parseClaudeModel`: `[CC]`'s `/model` list, live in 2.1.290.
///
/// herdr never reports the pane blocked while it waits, so the card can only come from the
/// screen. The list is read off its hint, the one line that is always there: a pane shorter
/// than the list scrolls the title off its top, and Claude then draws fewer rows so that the
/// one under the cursor stays on screen. What the list holds is Claude's to decide and differs
/// from one session to the next, so a row is read by its shape alone: its number, what it
/// names, `✔` on the model in use, and after two spaces or more what it says of it. The rows
/// drawn are a window on the list: `↑`/`↓` stand in the cursor's column on the first and last
/// of them when more lie beyond, and `… +N models` counts the ones below.
fn parse_claude_model(screen: &str) -> Option<ClaudePrompt> {
    let lines: Vec<String> = split_lines(screen)
        .iter()
        .map(|line| line.trim_end().to_string())
        .collect();
    let hint_index = find_last_index(&lines, |_, index| {
        claude_model_hint().is_match(&wrapped(&lines, index, 3))
    })?;
    let list = list_rows(
        &lines,
        hint_index,
        claude_model_row(),
        Some(claude_model_more()),
    );
    let under_from = if list.last < 0 {
        0
    } else {
        list.last as usize + 1
    };
    let under = if under_from <= hint_index {
        lines[under_from..hint_index]
            .iter()
            .filter(|line| !line.trim().is_empty())
            .count()
    } else {
        0
    };
    let selected_index = list.rows.iter().position(|row| row.cursor);
    if list.unread
        || under > REFERENCE_CLAUDE_MODEL_UNDER_LINES
        || list.rows.len() < 2
        || list.rows.iter().filter(|row| row.cursor).count() != 1
    {
        return None;
    }
    let names = list_names(&list.rows)?;
    let options: Vec<ReferencePromptOption> = names
        .iter()
        .map(|(name, said)| ReferencePromptOption {
            label: trailing_mark().replace(name, "").to_string(),
            description: said.clone(),
        })
        .collect();
    // counted, not named: the rows above the window (the list counts from 1) and the ones below
    let hidden = list.rows[0].number.saturating_sub(1) + list.below;
    let current = names
        .iter()
        .position(|(name, _)| trailing_mark().is_match(name));
    // "Default (recommended)" in use reads as Default: the tag is the list's advice, not the
    // model's name
    let mut asked = String::from("Select model for this session");
    if let Some(current) = current {
        asked.push_str(&format!(
            " (currently {})",
            trailing_recommended().replace(&options[current].label, "")
        ));
    }
    if hidden > 0 {
        asked.push_str(&format!(
            ". {} more {} listed in the terminal.",
            hidden,
            if hidden == 1 { "model is" } else { "models are" }
        ));
    }
    let selected = selected_index.unwrap_or(0);
    // `s`, never Enter: the pick stays in this session, and the default for new ones is left
    // alone
    let option_steps: Vec<Vec<ReferenceKeyStep>> = list
        .rows
        .iter()
        .enumerate()
        .map(|(index, _)| {
            let mut steps = key_steps(navigation_keys(index as i64 - selected as i64));
            steps.push(ReferenceKeyStep::typed(CLAUDE_MODEL_PICK_KEY));
            steps
        })
        .collect();
    Some(finish(
        CardParts {
            kind: ReferencePromptKind::Question,
            title: String::new(),
            question: asked,
            body: None,
            options: options.clone(),
            multi_select: false,
            custom_option_index: None,
        },
        RESPONDER_CLAUDE_MODEL,
        // a row by its number in the whole list and by what it says too: the window moves, two
        // rows may share a name, and a list by family names the model itself only in what the
        // row says
        list.rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                format!(
                    "{}. {}  {}",
                    row.number,
                    options[index].label,
                    options[index].description.clone().unwrap_or_default()
                )
            })
            .collect(),
        selected,
        Vec::new(),
        None,
        Some(option_steps),
    ))
}

// ---------------------------------------------------------------------------------------
// Card construction
// ---------------------------------------------------------------------------------------

/// The parts of a card that upstream's `finishPrompt` hashes and the client renders.
struct CardParts {
    kind: ReferencePromptKind,
    title: String,
    question: String,
    body: Option<String>,
    options: Vec<ReferencePromptOption>,
    multi_select: bool,
    custom_option_index: Option<u32>,
}

/// Upstream `finishPrompt` + `publicPrompt`: hash the card, then cap the body for display.
fn finish(
    parts: CardParts,
    responder: &str,
    menu_labels: Vec<String>,
    selected_index: usize,
    checked_option_indices: Vec<usize>,
    custom_menu_index: Option<usize>,
    option_steps: Option<Vec<Vec<ReferenceKeyStep>>>,
) -> ClaudePrompt {
    let id = claude_prompt_id(&parts);
    let body = parts
        .body
        .map(|body| body.chars().take(REFERENCE_CLAUDE_BODY_CAP).collect());
    ClaudePrompt {
        prompt: ReferencePrompt {
            id,
            agent: REFERENCE_CLAUDE_AGENT.to_string(),
            kind: parts.kind,
            title: parts.title,
            question: parts.question,
            body,
            options: parts.options,
            multi_select: parts.multi_select,
            custom_option_index: parts.custom_option_index,
            queued: None,
            steps: Vec::new(),
            fallback: None,
        },
        responder: responder.to_string(),
        menu_labels,
        selected_index,
        checked_option_indices,
        custom_menu_index,
        option_steps,
    }
}

/// Upstream `finishPrompt`'s id: `sha256(JSON.stringify({agent, kind, title, question, body,
/// options, multi_select, custom_option_index})).slice(0, 12)`.
///
/// The canonical string is built here rather than with a serializer, so the key order is the
/// reference's own and the body is hashed **before** the display cap.
fn claude_prompt_id(parts: &CardParts) -> String {
    let mut json = String::new();
    json.push_str("{\"agent\":");
    json.push_str(&json_string(REFERENCE_CLAUDE_AGENT));
    json.push_str(",\"kind\":");
    json.push_str(&json_string(kind_name(parts.kind)));
    json.push_str(",\"title\":");
    json.push_str(&json_string(&parts.title));
    json.push_str(",\"question\":");
    json.push_str(&json_string(&parts.question));
    json.push_str(",\"body\":");
    match &parts.body {
        Some(body) => json.push_str(&json_string(body)),
        None => json.push_str("null"),
    }
    json.push_str(",\"options\":[");
    for (index, option) in parts.options.iter().enumerate() {
        if index > 0 {
            json.push(',');
        }
        json.push_str("{\"label\":");
        json.push_str(&json_string(&option.label));
        json.push_str(",\"description\":");
        match &option.description {
            Some(description) => json.push_str(&json_string(description)),
            None => json.push_str("null"),
        }
        json.push('}');
    }
    json.push_str("],\"multi_select\":");
    json.push_str(if parts.multi_select { "true" } else { "false" });
    json.push_str(",\"custom_option_index\":");
    match parts.custom_option_index {
        Some(index) => json.push_str(&index.to_string()),
        None => json.push_str("null"),
    }
    json.push('}');
    let digest = Sha256::digest(json.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex.chars().take(REFERENCE_CLAUDE_PROMPT_ID_CHARS).collect()
}

/// Upstream's `kind` strings, as `InteractivePrompt` serializes them.
fn kind_name(kind: ReferencePromptKind) -> &'static str {
    match kind {
        ReferencePromptKind::Question => "question",
        ReferencePromptKind::Approval => "approval",
        ReferencePromptKind::Plan => "plan",
        ReferencePromptKind::Menu => "menu",
    }
}

/// One JSON string literal, as `JSON.stringify` writes it for the text a card carries.
fn json_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

// ---------------------------------------------------------------------------------------
// Shared row/panel helpers (upstream `cleanLine` … `sequentialRows`)
// ---------------------------------------------------------------------------------------

/// Upstream `cleanLine`: the line without ANSI, without the panel's `│` edges, trimmed.
fn clean_line(raw: &str) -> String {
    let stripped = strip_ansi(raw);
    let mut line = stripped.trim().to_string();
    if let Some(rest) = line.strip_prefix('│') {
        line = rest.trim_start().to_string();
    }
    if let Some(rest) = line.strip_suffix('│') {
        line = rest.trim_end().to_string();
    }
    line.trim().to_string()
}

/// The screen's lines: ANSI removed, `\r\n` split as upstream's `/\r?\n/` does.
fn split_lines(screen: &str) -> Vec<String> {
    strip_ansi(screen)
        .split('\n')
        .map(|line| line.trim_end_matches('\r').to_string())
        .collect()
}

fn strip_ansi(text: &str) -> String {
    ansi_escape().replace_all(text, "").to_string()
}

/// Upstream `isDivider`: a rule of box-drawing characters, and never an empty line.
fn is_divider(line: &str) -> bool {
    let value = clean_line(line);
    !value.is_empty() && divider().is_match(&value)
}

/// Upstream `normalizeText`: every run of whitespace one space, trimmed.
fn normalize_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<&str>>().join(" ")
}

/// Upstream `findLastIndex`.
fn find_last_index<F: Fn(&str, usize) -> bool>(lines: &[String], predicate: F) -> Option<usize> {
    (0..lines.len())
        .rev()
        .find(|index| predicate(&lines[*index], *index))
}

/// Upstream `wrapped`: a line and the two after it as one, so a hint a narrow pane wrapped is
/// still read. Blank lines and rules drop out of the join.
fn wrapped(lines: &[String], index: usize, span: usize) -> String {
    lines
        .iter()
        .skip(index)
        .take(span)
        .map(|line| clean_line(line))
        .filter(|line| !line.is_empty() && !is_divider(line))
        .collect::<Vec<String>>()
        .join(" ")
}

/// Upstream `nearestQuestion`: the closest line above that reads as a question, skipping the
/// panel's own furniture.
fn nearest_question(lines: &[String], before_index: usize) -> Option<String> {
    let floor = before_index.saturating_sub(REFERENCE_CLAUDE_NEAREST_WINDOW);
    for index in (floor..before_index).rev() {
        let line = clean_line(&lines[index]);
        if line.is_empty()
            || is_divider(&line)
            || nearest_skip_planning().is_match(&line)
            || nearest_skip_tabs().is_match(&line)
            || nearest_skip_chip().is_match(&line)
            || nearest_skip_progress().is_match(&line)
        {
            continue;
        }
        return Some(selected_count().replace(&line, "").trim().to_string());
    }
    None
}

/// Upstream `parseNumberedRows`: every `N. label` row in `start..end`, with the line under a
/// row as its description.
fn parse_numbered_rows(lines: &[String], start: usize, end: usize) -> Vec<ClaudeRow> {
    let mut rows: Vec<ClaudeRow> = Vec::new();
    if end <= start || start >= lines.len() {
        return rows;
    }
    for index in start..end.min(lines.len()) {
        let text = strip_ansi(&lines[index]);
        let text = text.trim();
        let Some(captures) = numbered_option().captures(text) else {
            continue;
        };
        let number = captures
            .get(2)
            .and_then(|number| number.as_str().parse::<usize>().ok())
            .unwrap_or(0);
        let raw_label = captures.get(3).map(|label| label.as_str()).unwrap_or("");
        let checked = checkbox_lead().is_match(raw_label);
        let label = checkbox_trim().replace(raw_label, "").trim().to_string();
        rows.push(ClaudeRow {
            number,
            label,
            description: None,
            selected: captures.get(1).is_some(),
            checked,
            line_index: index,
        });
    }
    for index in 0..rows.len() {
        let next = rows
            .get(index + 1)
            .map(|row| row.line_index)
            .unwrap_or(end);
        for line_index in rows[index].line_index + 1..next.min(lines.len()) {
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

/// Upstream `sequentialRows`: the rows count on from 1.
fn sequential_rows(rows: &[ClaudeRow]) -> bool {
    !rows.is_empty()
        && rows
            .iter()
            .enumerate()
            .all(|(index, row)| row.number == index + 1)
}

/// Upstream `claudeTabs`: Claude's question tabs above several questions, looked for up the
/// panel however far a long question wraps. `☐` is unanswered, `☒` answered, `✔` the Submit
/// step; a narrow pane can cut the bar off at its right edge.
fn claude_tabs(lines: &[String], before_index: usize) -> Option<ClaudeTabs> {
    let floor = before_index.saturating_sub(REFERENCE_CLAUDE_TABS_WINDOW);
    for index in (floor..before_index).rev() {
        let line = clean_line(&lines[index]);
        if solid_rule().is_match(&line) {
            return None;
        }
        if !claude_tabs_bar().is_match(&line) {
            continue;
        }
        let whole = line.ends_with('→');
        let inner = line.trim_start_matches('←');
        let inner = inner.strip_suffix('→').unwrap_or(inner);
        let mut tabs = Vec::new();
        for piece in two_spaces().split(inner) {
            let Some(captures) = tab_token().captures(piece) else {
                continue;
            };
            let marker = captures.get(1).map(|marker| marker.as_str()).unwrap_or("");
            if marker == "✔" {
                continue;
            }
            let label = captures
                .get(2)
                .map(|label| label.as_str().trim())
                .unwrap_or("");
            tabs.push(ClaudeTab {
                label: label.to_string(),
                answered: marker != "☐",
            });
        }
        return Some(ClaudeTabs { index, whole, tabs });
    }
    None
}

/// Upstream `claudeChip`: a single question's header chip (`☐ Dataset`), its short name.
fn claude_chip(lines: &[String], first_row: usize) -> Option<String> {
    let floor = first_row.saturating_sub(REFERENCE_CLAUDE_CHIP_WINDOW);
    for index in (floor..first_row).rev() {
        let line = clean_line(&lines[index]);
        if is_divider(&line) || claude_tabs_bar().is_match(&line) {
            return None;
        }
        if let Some(captures) = chip().captures(&line) {
            return captures
                .get(1)
                .map(|label| label.as_str().trim().to_string());
        }
    }
    None
}

/// Upstream `claudeQuestionText`: the question over Claude's options, joined back when a narrow
/// pane wraps it over several lines.
fn claude_question_text(
    lines: &[String],
    tabs_index: Option<usize>,
    first_row: usize,
) -> Option<String> {
    let floor = tabs_index
        .map(|tabs| tabs as i64)
        .unwrap_or(-1)
        .max(first_row as i64 - REFERENCE_CLAUDE_QUESTION_WINDOW as i64);
    let mut text: Vec<String> = Vec::new();
    let mut index = first_row as i64 - 1;
    while index > floor {
        let line = clean_line(&lines[index as usize]);
        if line.is_empty() {
            if !text.is_empty() {
                break;
            }
            index -= 1;
            continue;
        }
        // the single question's header chip or the panel's top rule ends the question
        if chip_any().is_match(&line) || is_divider(&line) || claude_tabs_bar().is_match(&line) {
            break;
        }
        text.insert(0, line);
        index -= 1;
    }
    if text.is_empty() {
        None
    } else {
        Some(normalize_text(&text.join(" ")))
    }
}

// ---------------------------------------------------------------------------------------
// The list reader (upstream `listRow` / `listRows` / `listNames`)
// ---------------------------------------------------------------------------------------

/// A row of a numbered list: the cursor or a scroll mark, the row's number, then its text.
fn list_row(line: &str, shape: &Regex) -> Option<ListRow> {
    let captures = shape.captures(line)?;
    let text = captures
        .get(3)
        .map(|text| text.as_str())
        .unwrap_or("")
        .to_string();
    let gap = two_spaces().find(&text);
    let gap_index = gap.map(|found| found.start() as i64).unwrap_or(-1);
    let gap_len = gap.map(|found| found.end() - found.start()).unwrap_or(0);
    let mark = captures.get(1).map(|mark| mark.as_str()).unwrap_or("");
    let edge = match mark {
        "↑" => Some('↑'),
        "↓" => Some('↓'),
        _ => None,
    };
    let column = if gap_index >= 0 {
        // in the terminal's columns, as the lines wrapped under it are indented
        let prefix = line.len() - text.len() + gap_index as usize + gap_len;
        display_width(&line[..prefix]) as i64
    } else {
        -1
    };
    Some(ListRow {
        number: captures
            .get(2)
            .and_then(|number| number.as_str().parse::<usize>().ok())
            .unwrap_or(0),
        cursor: selected_marker().is_match(mark),
        edge,
        text,
        gap: gap_index,
        resumes: if gap_index >= 0 {
            gap_index + gap_len as i64
        } else {
            -1
        },
        column,
        wrapped: Vec::new(),
    })
}

/// Upstream `listNames`: each row's name and what the row says of it. What the rows say stands
/// in one column for the whole list, so a gap at another column, or in one row alone, is part
/// of that row's name (`Custom  Model`).
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
    // the column most rows share; a tie goes to the column read first, as upstream's stable
    // sort((left, right) => right[1] - left[1]) keeps it
    let mut ordered = counts;
    ordered.sort_by(|left, right| right.1.cmp(&left.1));
    let shared = ordered
        .first()
        .filter(|(_, count)| *count >= 2)
        .map(|(column, _)| *column)
        .unwrap_or(-1);
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

/// Upstream `listRows`: the numbered rows right above a list's hint, read downward. A row that
/// does not count on from the one above it, that follows anything but its own wrapped text, or
/// that carries the window's `↑` starts the list again, and so does any row under the one that
/// carries its `↓`: numbered lines further up (an answer's own list) are nothing of this
/// menu's.
fn list_rows(lines: &[String], hint_index: usize, shape: &Regex, more: Option<&Regex>) -> ListRows {
    let mut rows: Vec<ListRow> = Vec::new();
    let mut below = 0usize;
    let mut ended = false;
    let mut unread = false;
    let mut first: i64 = -1;
    let mut last: i64 = -1;
    let from = hint_index.saturating_sub(REFERENCE_CLAUDE_LIST_WINDOW);
    for index in from..hint_index {
        let line = &lines[index];
        let text = line.trim();
        let above = if ended { None } else { rows.last() };
        // what a row says, wrapped under itself: told by its column, before anything it happens
        // to begin with
        if let Some(above) = above {
            let indent = line.len() - line.trim_start().len();
            if !text.is_empty() && above.column >= 0 && indent as i64 == above.column {
                let wrapped_text = text.to_string();
                if let Some(previous) = rows.last_mut() {
                    previous.wrapped.push(wrapped_text);
                }
                last = index as i64;
                continue;
            }
        }
        if let Some(row) = list_row(line, shape) {
            // upstream restarts the list when the line above it was not its predecessor, and a
            // line that ended the run of rows clears the row it compares against ('ended')
            let restart = match above {
                None => true,
                Some(above) => {
                    row.number != above.number + 1
                        || row.edge == Some('↑')
                        || above.edge == Some('↓')
                }
            };
            if restart {
                rows = Vec::new();
                first = index as i64;
            }
            rows.push(row);
            below = 0;
            ended = false;
            unread = false;
            last = index as i64;
            continue;
        }
        // upstream's guard is the row above, not the list: a line after anything but a row ends
        // the list without making it unread (the effort line under Claude's /model rows)
        if above.is_none() || text.is_empty() {
            ended = true;
            continue;
        }
        let counted = more.and_then(|pattern| pattern.captures(text));
        match counted {
            Some(captures) => {
                below = captures
                    .get(1)
                    .and_then(|count| count.as_str().parse::<usize>().ok())
                    .unwrap_or(0);
                ended = true;
                last = index as i64;
            }
            None => {
                ended = true;
                unread = true;
            }
        }
    }
    ListRows {
        rows,
        below,
        first,
        last,
        unread,
    }
}

// ---------------------------------------------------------------------------------------
// The option preview (upstream `withoutPreview` / `indexAtColumn`)
// ---------------------------------------------------------------------------------------

/// Upstream `withoutPreview`: the selected option's preview is drawn in a box to the right of
/// the options, with a `Notes: press n to add notes` line under it. The box stands in one
/// column on every line (its top corner names it), so only what sits in that column goes: a `│`
/// inside an option's own text stays.
fn without_preview(lines: &[String], from: usize, to: usize) -> Vec<String> {
    let mut column: i64 = -1;
    let mut index = from;
    while index <= to && column < 0 && index < lines.len() {
        let line = &lines[index];
        if let Some(corner) = preview_corner().find(line) {
            // upstream slices to 'corner.index + corner[0].length - 1': the corner's own last
            // character is the box's first column, not the column after it. It is dropped by
            // character, never by byte: a box-drawing corner is three bytes wide, and a byte
            // index inside it is not a char boundary.
            let corner_width = corner
                .as_str()
                .chars()
                .next_back()
                .expect("the preview corner match ends in its own corner character")
                .len_utf8();
            column = display_width(&line[..corner.end() - corner_width]) as i64;
        }
        index += 1;
    }
    lines
        .iter()
        .map(|line| {
            let at = if column >= 2 {
                index_at_column(line, column as usize)
            } else {
                -1
            };
            let boxed = at >= 0
                && line
                    .get(at as usize..)
                    .and_then(|rest| rest.chars().next())
                    .is_some_and(|edge| PREVIEW_EDGE.contains(edge))
                && precedes_two_spaces(line, at as usize);
            let trimmed = if boxed {
                line[..at as usize].trim_end().to_string()
            } else {
                line.clone()
            };
            preview_notes().replace(&trimmed, "").to_string()
        })
        .collect()
}

/// Whether the two characters before `at` are both spaces (the preview's own margin).
fn precedes_two_spaces(line: &str, at: usize) -> bool {
    let mut previous = line[..at].chars().rev();
    previous.next() == Some(' ') && previous.next() == Some(' ')
}

/// Upstream `indexAtColumn`: where the terminal column `column` begins in `line`; -1 when no
/// character starts there. Widths are summed per character (see the manifest's divergences).
fn index_at_column(line: &str, column: usize) -> i64 {
    let mut width = 0usize;
    for (index, character) in line.char_indices() {
        if width == column {
            return index as i64;
        }
        width += char_width(character);
    }
    if width == column {
        line.len() as i64
    } else {
        -1
    }
}

/// The display width of one character, as the port's terminal mirror measures it.
fn char_width(character: char) -> usize {
    let code = character as u32;
    if character.is_control() {
        return 0;
    }
    // combining marks, zero-width and joiner characters
    if (0x0300..=0x036F).contains(&code)
        || (0x1AB0..=0x1AFF).contains(&code)
        || (0x1DC0..=0x1DFF).contains(&code)
        || (0x20D0..=0x20FF).contains(&code)
        || (0xFE20..=0xFE2F).contains(&code)
        || code == 0x200B
        || code == 0x200D
        || code == 0xFEFF
    {
        return 0;
    }
    if (0x1100..=0x115F).contains(&code)
        || (0x2E80..=0xA4CF).contains(&code)
        || (0xAC00..=0xD7A3).contains(&code)
        || (0xF900..=0xFAFF).contains(&code)
        || (0xFE10..=0xFE19).contains(&code)
        || (0xFE30..=0xFE6F).contains(&code)
        || (0xFF00..=0xFF60).contains(&code)
        || (0xFFE0..=0xFFE6).contains(&code)
        || (0x1F300..=0x1FAFF).contains(&code)
        || (0x20000..=0x2FFFD).contains(&code)
    {
        return 2;
    }
    1
}

fn display_width(text: &str) -> usize {
    text.chars().map(char_width).sum()
}

// ---------------------------------------------------------------------------------------
// SGR runs, for Claude's input-box suggestion
// ---------------------------------------------------------------------------------------

/// Upstream `sgrRuns`: whether each character of an ANSI line is drawn dim (SGR 2) and inverse
/// (SGR 7), as `[text, dim, inverse]` runs. Only SGR sequences change the state; 38/48 colors
/// are skipped whole, so the 2 of `38;2;r;g;b` is a color mode, not dim. Other escapes are
/// dropped.
fn sgr_runs(line: &str) -> Vec<(String, bool, bool)> {
    let mut runs: Vec<(String, bool, bool)> = Vec::new();
    let mut dim = false;
    let mut inverse = false;
    let mut offset = 0usize;
    for found in escape().find_iter(line) {
        if found.start() > offset {
            runs.push((line[offset..found.start()].to_string(), dim, inverse));
        }
        offset = found.end();
        let whole = found.as_str();
        let Some(captures) = escape().captures(whole) else {
            continue;
        };
        // only a CSI's final byte can be `m`; an OSC or a two-character escape has none
        let Some(final_byte) = captures.get(2) else {
            continue;
        };
        if final_byte.as_str() != "m" {
            continue;
        }
        let params = captures.get(1).map(|params| params.as_str()).unwrap_or("");
        let codes: Vec<Option<i64>> = params
            .split(';')
            .map(|code| {
                if code.is_empty() {
                    Some(0)
                } else {
                    code.parse::<i64>().ok()
                }
            })
            .collect();
        let mut index = 0usize;
        while index < codes.len() {
            let Some(code) = codes[index] else {
                index += 1;
                continue;
            };
            if code == 38 || code == 48 || code == 58 {
                index += match codes.get(index + 1) {
                    Some(Some(5)) => 2,
                    Some(Some(2)) => 4,
                    _ => 0,
                };
            } else if code == 0 {
                dim = false;
                inverse = false;
            } else if code == 22 {
                dim = false;
            } else if code == 2 {
                dim = true;
            } else if code == 27 {
                inverse = false;
            } else if code == 7 {
                inverse = true;
            }
            index += 1;
        }
    }
    if offset < line.len() {
        runs.push((line[offset..].to_string(), dim, inverse));
    }
    runs
}

// ---------------------------------------------------------------------------------------
// Answer planning helpers
// ---------------------------------------------------------------------------------------

/// Upstream `navigationKeys`.
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

/// Upstream `keySteps`: one key per step.
fn key_steps(keys: Vec<String>) -> Vec<ReferenceKeyStep> {
    keys.into_iter()
        .map(|key| ReferenceKeyStep::keys([key]))
        .collect()
}

/// Upstream `promptTailIsActive`'s `ends`: a hint that runs into the screen's end, so a hint
/// that ended above later output (an answered, stale menu) does not count.
fn ends(shown: &[String], pattern: &Regex) -> bool {
    for span in 1..=REFERENCE_CLAUDE_HINT_TAIL_LINES {
        if shown.len() < span {
            continue;
        }
        let tail = shown[shown.len() - span..].join(" ");
        if !pattern.is_match(&tail) {
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

// ---------------------------------------------------------------------------------------
// Regexes, compiled once (upstream `server/prompt.ts`'s constants)
// ---------------------------------------------------------------------------------------

/// One lazily compiled pattern. Each name below mirrors an upstream constant; the port writes
/// `[0-9]`/`[0-9A-Za-z_]` where upstream writes `\d`/`\w`, because Rust's `regex` reads those
/// classes as Unicode and JavaScript reads them as ASCII.
macro_rules! claude_pattern {
    ($name:ident, $pattern:expr) => {
        fn $name() -> &'static Regex {
            static CELL: OnceLock<Regex> = OnceLock::new();
            CELL.get_or_init(|| {
                Regex::new($pattern).expect("the reference claude pattern compiles")
            })
        }
    };
}

/// Upstream `ANSI_RE`.
claude_pattern!(ansi_escape, r"\x1b\[[0-?]*[ -/]*[@-~]");
/// Upstream's OSC form, dropped alongside `ANSI_RE` in `parseClaudeSuggestion`.
claude_pattern!(osc, r"\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)");
/// Upstream `sgrRuns`'s escape: a CSI with its params, an OSC, or a two-character escape.
claude_pattern!(escape, r"\x1b(?:\[([0-?]*)[ -/]*([@-~])|\][^\x07\x1b]*(?:\x07|\x1b\\)|[@-Z\\-_])");
/// Upstream `SELECTED_RE`.
claude_pattern!(selected_marker, r"^[❯›>]\s*");
/// Upstream `DIVIDER_RE`.
claude_pattern!(divider, r"^[\s╭╮╰╯├┤┬┴┼─━═╌▔]+$");
/// Upstream `NUMBERED_OPTION_RE`.
claude_pattern!(numbered_option, r"^\s*([›>❯])?\s*([0-9]+)\.\s+(.+)$");
/// Upstream `parseNumberedRows`'s checkbox test.
claude_pattern!(checkbox_lead, r"^\[[xX✓]\]");
/// Upstream `parseNumberedRows`'s checkbox strip.
claude_pattern!(checkbox_trim, r"^\[[ xX✓]\]\s*");
/// Upstream `SOLID_RULE_RE`.
claude_pattern!(solid_rule, r"^[─━]{8,}$");
/// Upstream `CLAUDE_ASK_HINT_RE`.
claude_pattern!(
    claude_ask_hint,
    r"(?i)enter to select.*(?:↑/↓|tab/arrow keys) to navigate.*esc to cancel"
);
/// Upstream `CLAUDE_TABS_RE`.
claude_pattern!(claude_tabs_bar, r"^←\s+[☐☒☑✔]");
/// One tab of the bar: its marker and its label.
claude_pattern!(tab_token, r"([☐☒☑✔])\s+([^\s].*?)\s*$");
/// Upstream `CLAUDE_CONFIRM_HINT_RE`.
claude_pattern!(claude_confirm_hint, r"(?i)enter to confirm.*esc to (?:cancel|exit|go back)");
/// Upstream `CLAUDE_MODEL_HINT_RE`.
claude_pattern!(
    claude_model_hint,
    r"(?i)enter to set as default.*\bs to use this session only.*esc to cancel"
);
/// Upstream `CLAUDE_MODEL_ROW_RE`.
claude_pattern!(claude_model_row, r"^\s*([❯›>↑↓])?\s*([0-9]+)\.\s+(\S.*)$");
/// Upstream `CLAUDE_MODEL_MORE_RE`.
claude_pattern!(claude_model_more, r"^…\s*\+([0-9]+) models?$");
/// Upstream `CLAUDE_TASKS_HEAD_RE`.
claude_pattern!(
    claude_tasks_head,
    r"^([0-9]+) tasks \([0-9]+ done, (?:[0-9]+ in progress, )?[0-9]+ open\)$"
);
/// Upstream `CLAUDE_TASK_ROW_RE`.
claude_pattern!(claude_task_row, r"^[◻◼✔]\s");
/// Upstream `CLAUDE_TASKS_MORE_RE`.
claude_pattern!(claude_tasks_more, r"^…\s\+[0-9]+ pending$");
/// Upstream's guard on a task's activity line: a prompt or bullet of its own.
claude_pattern!(claude_task_activity_guard, r"^[❯>›●⏺]");
/// Upstream `LABELED_RULE_RE`.
claude_pattern!(labeled_rule, r"^─{3,}\s.*─$");
/// Upstream `CLAUDE_HINT_TAIL_RE`.
claude_pattern!(claude_hint_tail, r"(?i)\besc to (?:cancel|exit|go back)\b");
/// Upstream `CLAUDE_PREVIEW_HINT_RE`.
claude_pattern!(claude_preview_hint, r"(?i)\bn to add notes\b");
/// Upstream's preview box corner: two or more spaces, then the box's top-left corner.
claude_pattern!(preview_corner, r"\s{2,}[┌╭]");
/// Upstream's preview notes line.
claude_pattern!(preview_notes, r"(?i)^\s*Notes:\s+press n to add notes\b.*$");
/// Upstream `claudeChip`'s chip.
claude_pattern!(chip, r"^[☐☒☑✔]\s+(\S.*)$");
/// Upstream `claudeQuestionText`'s chip test.
claude_pattern!(chip_any, r"^[☐☒☑✔]\s+\S");
/// Upstream's typed-answer row (`Type something.`).
claude_pattern!(typed_answer, r"(?i)^Type something\.?$");
/// Upstream's multiple-choice row test.
claude_pattern!(multi_select_row, r"^\s*(?:[›>❯]\s*)?[0-9]+\.\s+\[[ xX✓]\]");
/// Upstream's plan question.
claude_pattern!(
    plan_question,
    r"(?i)Claude has written up a plan and is ready to execute\. Would you like to proceed\?"
);
/// Upstream's plan panel title.
claude_pattern!(ready_to_code, r"(?i)Ready to code\?");
/// Upstream's plan row that takes feedback.
claude_pattern!(tell_change, r"(?i)^Tell Claude what to change$");
/// Upstream's command-approval marker.
claude_pattern!(requires_approval, r"(?i)This command requires approval");
/// Upstream's dangerous-rm marker.
claude_pattern!(dangerous_rm, r"(?i)^Dangerous rm operation\b");
/// Upstream's approval question.
claude_pattern!(do_you_want, r"(?i)^Do you want to .+\?$");
/// Upstream's approval hint tail.
claude_pattern!(esc_to_cancel, r"(?i)esc to cancel");
/// Upstream's dropped `Tip:` line.
claude_pattern!(tip_line, r"(?i)^Tip:");
/// Upstream's tool call: `● name(`, a dotted or dashed name, an optional `(MCP)`.
claude_pattern!(tool_call, r"^●\s+[0-9A-Za-z_.:-]+(?:\s[0-9A-Za-z_.:-]+)*(?:\s\(MCP\))?\(");
/// Upstream's review header line.
claude_pattern!(review_header, r"(?i)^Review your answers$");
/// Upstream's submit question.
claude_pattern!(ready_to_submit, r"(?i)^Ready to submit your answers\?$");
/// Upstream's submit tail: the last row is `Cancel`.
claude_pattern!(claude_submit_cancel_row, r"(?i)^(?:[›>❯]\s*)?[0-9]+\.\s+Cancel$");
/// Upstream's approval tail.
claude_pattern!(claude_approval_tail, r"(?i)esc to cancel.*(?:tab|ctrl\+e)|ctrl\+e to explain");
/// Upstream's plan tail.
claude_pattern!(
    claude_plan_tail,
    r"(?i)ctrl\+g to edit|shift\+tab to approve with this feedback"
);
/// Upstream `nearestQuestion`'s skipped lines.
claude_pattern!(nearest_skip_planning, r"(?i)^Planning:");
claude_pattern!(nearest_skip_tabs, r"^[←→].*Submit");
claude_pattern!(nearest_skip_chip, r"^[☐☒☑✔]\s+\S");
claude_pattern!(nearest_skip_progress, r"(?i)^Question [0-9]+/[0-9]+");
/// Upstream's `(N selected)` prefix on a question line.
claude_pattern!(selected_count, r"(?i)^\([0-9]+\s+selected\)\s*");
/// Upstream's `asked` sentence inside a confirm panel's prose.
claude_pattern!(asked_question, r"(?:^|[.:!]\s+)([^.:!?]*\?)");
/// Upstream's `✔`/`✓` mark on the model in use.
claude_pattern!(trailing_mark, r"\s*[✔✓]$");
/// Upstream's `(recommended)` tag.
claude_pattern!(trailing_recommended, r"(?i)\s*\(recommended\)$");
/// Upstream's two-or-more-space run.
claude_pattern!(two_spaces, r"\s{2,}");
/// Upstream `CLAUDE_TIP_RE`: Claude's new-session tip, not a suggestion.
claude_pattern!(claude_tip, r#"^Try ""#);

// ---------------------------------------------------------------------------------------
// Tests (authored, NOT executed: the execution override defers every run to the post-merge
// wave)
// ---------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const QUESTION: &str = include_str!("fixtures/claude/prompt/question.screen");
    const QUESTION_TABS: &str = include_str!("fixtures/claude/prompt/question-tabs.screen");
    const QUESTION_TABS_MULTI: &str =
        include_str!("fixtures/claude/prompt/question-tabs-multiselect.screen");
    const QUESTION_TABS_WRAPPED: &str =
        include_str!("fixtures/claude/prompt/question-tabs-wrapped.screen");
    const QUESTION_NARROW: &str = include_str!("fixtures/claude/prompt/question-narrow.screen");
    const QUESTION_PREVIEW: &str = include_str!("fixtures/claude/prompt/question-preview.screen");
    const QUESTION_TASKS: &str = include_str!("fixtures/claude/prompt/question-tasks.screen");
    const SUBMIT: &str = include_str!("fixtures/claude/prompt/submit.screen");
    const PLAN: &str = include_str!("fixtures/claude/prompt/plan.screen");
    /// The plan card once CC has carried on under it: the plan is no longer the screen's end.
    const PLAN_ANSWERED: &str = include_str!("fixtures/claude/prompt/answered-plan.screen");
    const APPROVAL_COMMAND: &str = include_str!("fixtures/claude/prompt/approval-command.screen");
    const APPROVAL_COMMAND_WRAPPED: &str =
        include_str!("fixtures/claude/prompt/approval-command-wrapped.screen");
    const APPROVAL_EDIT_WRAPPED: &str =
        include_str!("fixtures/claude/prompt/approval-edit-wrapped.screen");
    const APPROVAL_UNDER_TEXT: &str =
        include_str!("fixtures/claude/prompt/approval-under-text.screen");
    const APPROVAL_MCP: &str = include_str!("fixtures/claude/prompt/approval-mcp.screen");
    const TRUST_MENU: &str = include_str!("fixtures/claude/prompt/trust-menu.screen");
    const CONFIRM_NARROW: &str = include_str!("fixtures/claude/prompt/confirm-narrow.screen");
    const CONFIRM_AMBIGUOUS: &str = include_str!("fixtures/claude/prompt/confirm-ambiguous.screen");
    const ANSWERED: &str = include_str!("fixtures/claude/prompt/answered.screen");
    const MODEL: &str = include_str!("fixtures/claude/prompt/model.screen");
    const MODEL_FAMILIES: &str = include_str!("fixtures/claude/prompt/model-families.screen");
    const MODEL_PHONE: &str = include_str!("fixtures/claude/prompt/model-phone.screen");

    fn labels(prompt: &ReferencePrompt) -> Vec<String> {
        prompt
            .options
            .iter()
            .map(|option| option.label.clone())
            .collect()
    }

    fn read(screen: &str) -> ClaudePrompt {
        parse_claude_prompt(screen).expect("the screen carries a claude prompt")
    }

    fn enter() -> ReferenceKeyStep {
        ReferenceKeyStep::keys([KEY_ENTER])
    }

    fn option(index: u32) -> ReferencePromptAnswer {
        ReferencePromptAnswer {
            option_index: Some(index),
            ..ReferencePromptAnswer::default()
        }
    }

    fn options(indices: Vec<u32>) -> ReferencePromptAnswer {
        ReferencePromptAnswer {
            option_indices: Some(indices),
            ..ReferencePromptAnswer::default()
        }
    }

    fn custom(text: &str) -> ReferencePromptAnswer {
        ReferencePromptAnswer {
            custom_text: Some(text.to_string()),
            ..ReferencePromptAnswer::default()
        }
    }

    #[test]
    fn question_reads_its_chip_typed_row_and_options() {
        let parsed = read(QUESTION);
        assert_eq!(parsed.responder, RESPONDER_CLAUDE_QUESTION);
        assert_eq!(parsed.prompt.kind, ReferencePromptKind::Question);
        // a single question is titled by its header chip
        assert_eq!(parsed.prompt.title, "Dataset");
        assert_eq!(
            parsed.prompt.question,
            "Which evaluation dataset should we use?"
        );
        assert_eq!(labels(&parsed.prompt), vec!["LM-O", "YCB-V", "T-LESS"]);
        assert_eq!(
            parsed.prompt.options[0].description.as_deref(),
            Some("Occlusion benchmark.")
        );
        assert!(!parsed.prompt.multi_select);
        assert_eq!(parsed.prompt.custom_option_index, Some(3));
        assert_eq!(parsed.selected_index, 0);
        assert_eq!(parsed.menu_labels.len(), 5);
        // the same screen is the same card
        assert_eq!(read(QUESTION).prompt.id, parsed.prompt.id);
        assert_eq!(
            plan_claude_answer(&parsed, &option(1)).expect("a row is an answer"),
            vec![ReferenceKeyStep::keys([KEY_DOWN]), enter()]
        );
    }

    #[test]
    fn question_tabs_title_the_question_and_the_choice_moves_on_with_right() {
        let parsed = read(QUESTION_TABS);
        assert_eq!(parsed.prompt.title, "Route · 1 of 2");
        assert_eq!(parsed.prompt.question, "Which way should the PR go?");
        assert_eq!(labels(&parsed.prompt), vec!["Log in as owner", "Fork"]);
        assert_eq!(parsed.prompt.custom_option_index, Some(2));

        let sets = read(QUESTION_TABS_MULTI);
        assert_eq!(sets.prompt.title, "Sets · 2 of 2");
        assert!(sets.prompt.multi_select);
        assert_eq!(sets.prompt.custom_option_index, None);
        assert_eq!(labels(&sets.prompt), vec!["LM-O", "YCB-V", "T-LESS"]);
        // → moves on to the next tab: an enter there would pick its first option
        assert_eq!(
            plan_claude_answer(&sets, &options(vec![0, 2])).expect("a multi-select is answerable"),
            vec![
                enter(),
                ReferenceKeyStep::keys([KEY_DOWN]),
                ReferenceKeyStep::keys([KEY_DOWN]),
                enter(),
                ReferenceKeyStep::keys([KEY_RIGHT]),
            ]
        );
    }

    #[test]
    fn question_tabs_join_a_wrapped_question_and_a_cut_off_bar() {
        let parsed = read(QUESTION_TABS_WRAPPED);
        // the bar was cut at `✔ Su`, so it does not say how many questions there are
        assert_eq!(parsed.prompt.title, "Author");
        assert_eq!(
            parsed.prompt.question,
            "Who should author the commits that go into the pull request, given that the fork \
             belongs to the lab account and the upstream repository to its owner?"
        );
        assert_eq!(
            plan_claude_answer(&parsed, &option(1)).expect("a row is an answer"),
            vec![ReferenceKeyStep::keys([KEY_DOWN]), enter()]
        );
    }

    #[test]
    fn question_reads_a_wrapped_hint_and_a_panel_bodied_question() {
        let parsed = read(QUESTION_NARROW);
        assert_eq!(parsed.prompt.title, "재현 테스트");
        assert_eq!(
            labels(&parsed.prompt),
            vec!["채팅에 카드가 안 떠요", "채팅에 카드가 떠요"]
        );
        assert_eq!(parsed.prompt.custom_option_index, Some(2));
        assert!(parsed
            .prompt
            .question
            .starts_with("재현용 테스트 질문입니다."));
    }

    #[test]
    fn question_with_option_previews_offers_no_typed_answer() {
        let parsed = read(QUESTION_PREVIEW);
        assert_eq!(parsed.prompt.title, "Layout · 1 of 2");
        assert_eq!(parsed.prompt.question, "Which layout?");
        assert_eq!(labels(&parsed.prompt), vec!["Grid", "List"]);
        assert_eq!(parsed.prompt.options[0].description, None);
        assert_eq!(parsed.prompt.custom_option_index, None);
        assert_eq!(
            plan_claude_answer(&parsed, &option(1)).expect("a row is an answer"),
            vec![ReferenceKeyStep::keys([KEY_DOWN]), enter()]
        );
        assert!(plan_claude_answer(&parsed, &custom("x")).is_err());
    }

    #[test]
    fn question_reads_over_the_task_list() {
        let parsed = read(QUESTION_TASKS);
        assert_eq!(parsed.prompt.title, "방향 검토 · 1 of 2");
        assert_eq!(parsed.prompt.question, "검토용 목업 페이지를 만들까요?");
        assert_eq!(
            labels(&parsed.prompt),
            vec!["만들지 않음 (Recommended)", "만듦"]
        );
        assert_eq!(parsed.prompt.custom_option_index, Some(2));
    }

    #[test]
    fn question_stays_open_over_a_bare_task_list_and_closes_over_later_output() {
        // the task list is Claude's own footer under the panel: the question is still the
        // screen's end
        let with_list = format!(
            "{QUESTION_TASKS}──────────────────────────────────────────────────────────────────────────────────────────────── 세션 이름 ─\n\n  3 tasks (0 done, 3 open)\n  ◻ 준비\n  ◻ 구현\n  ◻ 검증"
        );
        assert_eq!(
            read(&with_list).prompt.question,
            "검토용 목업 페이지를 만들까요?"
        );
        // the agent answered and drew under it: the card is stale
        let answered = format!(
            "{QUESTION_TASKS}\n⏺ 만들지 않음으로 진행합니다.\n\n────────\n❯ \n────────\n  ⏵⏵ bypass permissions on\n  3 tasks (0 done, 3 open)\n  ◻ 준비"
        );
        assert!(parse_claude_prompt(&answered).is_none());
        // output that ends in … after the list is not the list's own activity
        let after = format!(
            "{QUESTION_TASKS}  3 tasks (0 done, 3 open)\n  ◻ 준비\n⏺ Done…\n"
        );
        assert!(parse_claude_prompt(&after).is_none());
    }

    #[test]
    fn submit_is_a_menu_over_the_answers() {
        let parsed = read(SUBMIT);
        assert_eq!(parsed.responder, RESPONDER_CLAUDE_SUBMIT);
        // a menu, not a question: a typed pick submits every answer at once
        assert_eq!(parsed.prompt.kind, ReferencePromptKind::Menu);
        assert_eq!(parsed.prompt.title, "Review your answers");
        assert_eq!(parsed.prompt.question, "Ready to submit your answers?");
        assert_eq!(labels(&parsed.prompt), vec!["Submit answers", "Cancel"]);
        assert_eq!(parsed.prompt.custom_option_index, None);
        assert!(parsed
            .prompt
            .body
            .as_deref()
            .is_some_and(|body| body.contains("→ Repo owner")));
    }

    #[test]
    fn plan_card_takes_feedback_and_confirms_it_with_a_backtab() {
        let parsed = read(PLAN);
        assert_eq!(parsed.responder, RESPONDER_CLAUDE_PLAN);
        assert_eq!(parsed.prompt.kind, ReferencePromptKind::Plan);
        assert_eq!(parsed.prompt.title, "Ready to code?");
        assert_eq!(parsed.prompt.custom_option_index, Some(3));
        assert!(parsed
            .prompt
            .body
            .as_deref()
            .is_some_and(|body| body.contains("Add a heading")));
        assert_eq!(
            plan_claude_answer(&parsed, &custom("Keep the existing introduction"))
                .expect("the plan takes feedback"),
            vec![
                ReferenceKeyStep::keys([KEY_DOWN]),
                ReferenceKeyStep::keys([KEY_DOWN]),
                ReferenceKeyStep::keys([KEY_DOWN]),
                ReferenceKeyStep::typed("Keep the existing introduction"),
                ReferenceKeyStep::keys([KEY_BACKTAB]),
            ]
        );
    }

    #[test]
    fn plan_card_closes_once_its_own_tail_is_buried() {
        // the claude-plan arm of promptTailIsActive: ctrl+g to edit | shift+tab to approve
        assert!(prompt_tail_is_active(RESPONDER_CLAUDE_PLAN, PLAN));
        assert!(!prompt_tail_is_active(RESPONDER_CLAUDE_PLAN, PLAN_ANSWERED));
        assert!(parse_claude_prompt(PLAN_ANSWERED).is_none());
    }

    #[test]
    fn approval_reads_its_panel_and_its_options() {
        let parsed = read(APPROVAL_COMMAND);
        assert_eq!(parsed.responder, RESPONDER_CLAUDE_APPROVAL);
        assert_eq!(parsed.prompt.kind, ReferencePromptKind::Approval);
        assert_eq!(parsed.prompt.title, "Bash command");
        assert_eq!(parsed.prompt.question, "Do you want to proceed?");
        assert_eq!(
            parsed.prompt.body.as_deref(),
            Some("rm -rf junk\nDelete the junk directory")
        );
        assert_eq!(
            labels(&parsed.prompt),
            vec![
                "Yes",
                "Yes, and always allow access to /tmp/prompt-lab/junk from this project",
                "Yes, and switch to auto mode · auto mode handles these prompts for you",
                "No",
            ]
        );
        assert_eq!(
            plan_claude_answer(&parsed, &option(3)).expect("a row is an answer"),
            vec![
                ReferenceKeyStep::keys([KEY_DOWN]),
                ReferenceKeyStep::keys([KEY_DOWN]),
                ReferenceKeyStep::keys([KEY_DOWN]),
                enter(),
            ]
        );
    }

    #[test]
    fn approval_joins_a_label_a_narrow_pane_wrapped() {
        let wrapped = read(APPROVAL_COMMAND_WRAPPED);
        assert_eq!(
            labels(&wrapped.prompt),
            vec![
                "Yes",
                "Yes, and always allow access to /tmp/prompt-lab/junk from this project",
                "No",
            ]
        );
        let edit = read(APPROVAL_EDIT_WRAPPED);
        assert_eq!(edit.prompt.title, "Create file");
        assert_eq!(
            edit.prompt.question,
            "Do you want to create notes.md?"
        );
        assert_eq!(
            labels(&edit.prompt),
            vec![
                "Yes",
                "Yes, and switch to accept edits (auto-approve file edits and common file \
                 commands) for this session",
                "No",
            ]
        );
    }

    #[test]
    fn approval_takes_its_panel_from_the_rule_under_the_tool_call() {
        // Claude's own text opens with ● as well: a rule in its table is not the panel's
        let under_text = read(APPROVAL_UNDER_TEXT);
        assert_eq!(under_text.prompt.kind, ReferencePromptKind::Approval);
        assert_eq!(under_text.prompt.title, "Bash command");
        assert_eq!(
            under_text.prompt.body.as_deref(),
            Some("rm -rf junk\nDelete the junk directory")
        );
        // an MCP call is a call too: the first rule under it opens the panel
        let mcp = read(APPROVAL_MCP);
        assert_eq!(mcp.prompt.kind, ReferencePromptKind::Approval);
        assert_eq!(mcp.prompt.title, "Tool use");
    }

    #[test]
    fn confirm_menu_reads_its_unnumbered_rows() {
        let parsed = read(TRUST_MENU);
        assert_eq!(parsed.responder, RESPONDER_CLAUDE_CONFIRM);
        assert_eq!(parsed.prompt.kind, ReferencePromptKind::Menu);
        // the panel's first line titles the card exactly as drawn, colon included
        assert_eq!(parsed.prompt.title, "Accessing workspace:");
        assert_eq!(
            parsed.prompt.question,
            "Is this a project you created or one you trust?"
        );
        assert_eq!(labels(&parsed.prompt), vec!["No, exit", "Yes, I trust this folder"]);
        assert!(parsed.prompt.body.as_deref().is_some_and(|body| body
            .contains("[CC]'ll be able to read, edit, and execute files here.")));
        // answered from the native cursor
        assert_eq!(
            plan_claude_answer(&parsed, &option(1)).expect("a row is an answer"),
            vec![ReferenceKeyStep::keys([KEY_DOWN]), enter()]
        );
        assert_eq!(
            plan_claude_answer(&parsed, &option(0)).expect("a row is an answer"),
            vec![enter()]
        );
        assert!(plan_claude_answer(&parsed, &custom("maybe")).is_err());
    }

    #[test]
    fn confirm_menu_keeps_a_label_a_narrow_pane_wrapped_as_one_row() {
        let parsed = read(CONFIRM_NARROW);
        assert_eq!(labels(&parsed.prompt), vec!["No, exit", "Yes, I trust this folder"]);
    }

    #[test]
    fn confirm_menu_guesses_nothing_when_a_row_is_the_widest_line() {
        // a row wider than every line off the rows says nothing of the pane's width: the line
        // under it could be its tail or the next row, and a guess answers the wrong row
        assert!(parse_claude_prompt(CONFIRM_AMBIGUOUS).is_none());
    }

    #[test]
    fn an_answered_menu_above_later_output_is_not_open() {
        assert!(parse_claude_prompt(ANSWERED).is_none());
    }

    #[test]
    fn model_list_reads_its_window_and_counts_the_rows_it_holds_back() {
        let parsed = read(MODEL);
        assert_eq!(parsed.responder, RESPONDER_CLAUDE_MODEL);
        assert_eq!(parsed.prompt.kind, ReferencePromptKind::Question);
        assert_eq!(parsed.prompt.title, "");
        assert_eq!(parsed.prompt.body, None);
        assert_eq!(
            parsed.prompt.question,
            "Select model for this session (currently Opus 5.5). 2 more models are listed in the terminal."
        );
        assert_eq!(
            labels(&parsed.prompt),
            vec![
                "Default (recommended)",
                "Opus 5.5",
                "Fable 5.1",
                "Sonnet 5.5",
                "Haiku 4.5",
                "Sonnet 5",
                "Opus 5",
                "Fable 5",
                "Opus 4.8",
                "Opus 4.7",
            ]
        );
        assert_eq!(
            parsed.prompt.options[1].description.as_deref(),
            Some("For complex work and everyday tasks")
        );
        // `s`, never Enter: the pick stays in this session
        assert_eq!(
            plan_claude_answer(&parsed, &option(1)).expect("a row is an answer"),
            vec![ReferenceKeyStep::typed(CLAUDE_MODEL_PICK_KEY)]
        );
        // the list is a window: a row two below the cursor is two moves and the letter
        assert_eq!(
            plan_claude_answer(&parsed, &option(3)).expect("a row is an answer"),
            vec![
                ReferenceKeyStep::keys([KEY_DOWN]),
                ReferenceKeyStep::keys([KEY_DOWN]),
                ReferenceKeyStep::typed(CLAUDE_MODEL_PICK_KEY),
            ]
        );
    }

    #[test]
    fn model_list_reads_a_list_by_family() {
        let parsed = read(MODEL_FAMILIES);
        assert_eq!(
            parsed.prompt.question,
            "Select model for this session (currently Default)"
        );
        assert_eq!(
            labels(&parsed.prompt),
            vec!["Default (recommended)", "Opus", "Fable", "Sonnet", "Haiku"]
        );
        assert_eq!(
            parsed.prompt.options[1].description.as_deref(),
            Some("Opus 5.5 · Best for everyday, complex tasks")
        );
    }

    #[test]
    fn model_list_reads_a_phone_pane_with_wrapped_rows() {
        let parsed = read(MODEL_PHONE);
        assert_eq!(
            parsed.prompt.question,
            "Select model for this session (currently Opus 5.5). 7 more models are listed in the terminal."
        );
        assert_eq!(
            labels(&parsed.prompt),
            vec![
                "Default (recommended)",
                "Opus 5.5",
                "Fable 5.1",
                "Sonnet 5.5",
                "Haiku 4.5",
            ]
        );
        assert_eq!(
            parsed.prompt.options[1].description.as_deref(),
            Some("For complex work and everyday tasks")
        );
    }

    #[test]
    fn model_list_holds_the_screen_for_the_no_fallback_rule() {
        assert!(claude_model_list_waits(MODEL));
        assert!(claude_model_list_waits(MODEL_PHONE));
        assert!(!claude_model_list_waits(QUESTION));
    }

    #[test]
    fn suggestion_reads_the_grey_text_in_the_empty_input_box() {
        let rule = "\u{1b}[0m\u{1b}[38;2;136;136;136m";
        let rule = format!("{rule}{}{}", "─".repeat(60), "\u{1b}[0m");
        let screen = |input: &str, below: Option<String>| {
            [
                "\u{1b}[0m\u{1b}[38;2;255;255;255m● \u{1b}[0m표본 수집이 끝나면 알림이 오도록 걸어 두었습니다.".to_string(),
                String::new(),
                "\u{1b}[0m\u{1b}[38;2;153;153;153m✻ Worked for 2m 14s · done 오후 4:16\u{1b}[0m".to_string(),
                rule.clone(),
                input.to_string(),
                below.unwrap_or_else(|| rule.clone()),
                "  \u{1b}[0m\u{1b}[38;5;6m[Opus 5.5 (1M context)]\u{1b}[0m\u{1b}[38;2;153;153;153m │ \u{1b}[0m\u{1b}[2m\u{1b}[38;2;153;153;153m⏱️  25h\u{1b}[0m".to_string(),
                "  \u{1b}[0m\u{1b}[38;2;255;107;128m⏵⏵ bypass permissions on\u{1b}[0m".to_string(),
            ]
            .join("\r\n")
        };
        assert_eq!(
            parse_claude_suggestion(&screen(
                "❯\u{a0}\u{1b}[0m\u{1b}[2m아직 진행중이야?\u{1b}[0m",
                None
            )),
            Some("아직 진행중이야?".to_string())
        );
        // dim set together with a color, in one sequence
        assert_eq!(
            parse_claude_suggestion(&screen("❯ \u{1b}[2;38;5;8mrun the tests again\u{1b}[0m", None)),
            Some("run the tests again".to_string())
        );
        // nothing while text is typed, the box is empty, or it holds Claude's tip
        assert_eq!(
            parse_claude_suggestion(&screen("❯\u{a0}아직 진행중이야?", None)),
            None
        );
        assert_eq!(
            parse_claude_suggestion(&screen("❯ \u{1b}[2m아직\u{1b}[0m 진행중", None)),
            None
        );
        assert_eq!(parse_claude_suggestion(&screen("❯\u{a0}", None)), None);
        assert_eq!(
            parse_claude_suggestion(&screen(
                "❯ \u{1b}[2mTry \"how does <filepath> work?\"\u{1b}[0m",
                None
            )),
            None
        );
        // a truecolor foreground is not dim: its 2 is the color mode
        assert_eq!(
            parse_claude_suggestion(&screen("❯ \u{1b}[38;2;153;153;153mnot a suggestion\u{1b}[0m", None)),
            None
        );
        // only the input box: not a ❯ line without its rules, nor a box of several lines
        assert_eq!(
            parse_claude_suggestion(&screen(
                "❯ \u{1b}[2mfirst line\u{1b}[0m",
                Some("  \u{1b}[2msecond line\u{1b}[0m".to_string())
            )),
            None
        );
        assert_eq!(
            parse_claude_suggestion("❯ \u{1b}[2mloose text\u{1b}[0m\nmore"),
            None
        );
        // Claude's own drawn cursor on the first grey character
        assert_eq!(
            parse_claude_suggestion(&screen(
                "❯ \u{1b}[7mr\u{1b}[27m\u{1b}[2mun the tests\u{1b}[22m",
                None
            )),
            Some("run the tests".to_string())
        );
        assert_eq!(
            parse_claude_suggestion(&screen("❯ \u{1b}[7mr\u{1b}[27m", None)),
            None
        );
        assert_eq!(
            parse_claude_suggestion(&screen("❯ \u{1b}[7mr\u{1b}[27mun", None)),
            None
        );
    }

    #[test]
    fn the_detector_claims_the_claude_agent_id_only() {
        let detected = detect_reference_claude_prompt(REFERENCE_CLAUDE_AGENT, QUESTION)
            .expect("claude is this lane's agent");
        assert_eq!(detected.title, "Dataset");
        for agent in ["codex", "omp", "omo", "pi", "gjc", ""] {
            assert_eq!(detect_reference_claude_prompt(agent, QUESTION), None);
        }
        // a screen no claude branch knows is no card for any agent
        assert_eq!(parse_claude_prompt("$ cargo test\n"), None);
        assert_eq!(
            detect_reference_claude_prompt(REFERENCE_CLAUDE_AGENT, "$ cargo test\n"),
            None
        );
    }

    #[test]
    fn a_changed_card_is_another_card() {
        let first = read(QUESTION);
        let changed = QUESTION.replace(
            "Which evaluation dataset should we use?",
            "Which evaluation dataset should we drop?",
        );
        let second = read(&changed);
        // the id names what the card says: an answer to the first is refused as stale
        assert_ne!(first.prompt.id, second.prompt.id);
        assert_eq!(first.prompt.id.len(), REFERENCE_CLAUDE_PROMPT_ID_CHARS);
    }

    #[test]
    fn invalid_answer_shapes_are_refused() {
        let question = read(QUESTION);
        // zero shapes, and two shapes at once
        assert!(plan_claude_answer(&question, &ReferencePromptAnswer::default()).is_err());
        assert!(plan_claude_answer(
            &question,
            &ReferencePromptAnswer {
                option_index: Some(0),
                custom_text: Some("also".to_string()),
                ..ReferencePromptAnswer::default()
            }
        )
        .is_err());
        // a multi-select answer to a single-choice card
        assert!(plan_claude_answer(&question, &options(vec![0])).is_err());
        // an index outside the displayed range, and the typed row itself
        assert!(plan_claude_answer(&question, &option(99)).is_err());
        assert!(plan_claude_answer(&question, &option(3)).is_err());
        // a typed answer where the menu has no typed row
        let submit = read(SUBMIT);
        assert!(plan_claude_answer(&submit, &custom("yes")).is_err());
        // a typed answer on a card whose options are not rows of a menu
        let model = read(MODEL);
        assert!(plan_claude_answer(&model, &custom("Opus 5.5")).is_err());
    }

    #[test]
    fn the_tail_check_is_public_and_reads_the_screen_end() {
        let parsed = read(QUESTION);
        assert!(parsed.tail_is_active(QUESTION));
        assert!(!parsed.tail_is_active(&format!("{QUESTION}● Done.\n\n> \n")));
        assert!(prompt_tail_is_active(RESPONDER_CLAUDE_CONFIRM, TRUST_MENU));
    }

    #[test]
    fn without_claude_tasks_keeps_only_claudes_own_footer_off() {
        let shown = |lines: &[&str]| lines.iter().map(|line| line.to_string()).collect::<Vec<_>>();
        // a task list at the screen's end goes, with the session's rule above it
        let with_rule = shown(&[
            "❯ 1. Yes",
            "Enter to select · ↑/↓ to navigate · Esc to cancel",
            "──────────────── 세션 이름 ─",
            "  3 tasks (0 done, 3 open)",
            "  ◻ 준비",
        ]);
        assert_eq!(
            without_claude_tasks(&with_rule),
            shown(&[
                "❯ 1. Yes",
                "Enter to select · ↑/↓ to navigate · Esc to cancel",
            ])
        );
        // a shell's prompt under the hint keeps the panel what it is then
        let under_a_shell = shown(&[
            "❯ 1. Yes",
            "Enter to select · ↑/↓ to navigate · Esc to cancel",
            "⏺ Done.",
            "──── user@host:~/project ─",
        ]);
        assert_eq!(without_claude_tasks(&under_a_shell), under_a_shell);
    }
}
