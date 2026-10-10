//! pi prompt family (plan task 26).
//!
//! Ported from `devswha/herdr-web-ui` @ `54e5a1f67090cb09552d182e7e30dd0ecc314918`
//! (MIT, see `docs/chat/HERDR_LICENSE`). Upstream anchors, all read at the pinned revision
//! (`server/prompt.ts`, SHA-256 `083a74a29015258f7c1e11016c4f520cf265fb2ca89013feede71c2e030dba58`):
//!
//! | Upstream | What this module ports |
//! |---|---|
//! | `parsePrompt` pi branch (`:1941-1947`) | the candidate order `[parsePiModel, parsePiDialog]`, each gated by the tail check |
//! | `promptTailIsActive` pi branches (`:1665-1671`) | `hintAtEnd` for the `/model` hint and the menu/input hints |
//! | `hintAtEnd` (`:1727-1736`) | the wrapped-hint window a phone's pane needs |
//! | `parsePiModel` (`:1849-1879`) | the `/model` catalogue reader, the filter anchor and the current-model title |
//! | `piModelRows` (`:1802-1844`) | the wrapped-tail join, the note/row discrimination, the mid-bracket void |
//! | `parsePiDialog` (`:1881-1928`) | the select / confirm / input dialog reader and the `moved` rule |
//! | `piDialogRows` (`:1758-1783`) | the row/title split and the wrapped continuation lines |
//! | `answerKeys` pi branches (`:1995-2052`) | the key plans, including the input dialog's `ctrl+k`/`ctrl+u` |
//! | `finishPrompt` (`:228-242`) | the content-hash prompt id |
//!
//! pi draws **one** menu widget for an extension's `ctx.ui.select` / `confirm` / `input` and for
//! the selectors `/login` and `/scoped-models` open, and names what it takes in a hint line at the
//! dialog's end. `/model` draws a widget of its own (its hint carries no `↑↓ navigate`), and
//! `/tree` is deliberately not read at all: its hint says `↑/↓ move`, and answering it from the
//! chat would move the session's branch, which the chat has no way to undo by clicking.
//!
//! Lane boundary, recorded rather than silently exceeded: upstream's pi branch ends with `...omo()`
//! — an OmO form on a pane herdr names `pi`. That fallback is the omo lane's (task 25); this module
//! returns `None` there, and the dispatcher (task 9) composes the two.
//!
//! ## Where the cursor travels
//!
//! The frozen [`ReferencePrompt`] has no cursor field, but pi's answers navigate from **where pi
//! drew the cursor** (`selectedIndex`), which is not always row 0: `/model` starts on the model in
//! use. Upstream keeps that position in a `WeakMap` beside the public prompt; a pure
//! `(prompt, answer) -> keys` planner has no such side channel, and this lane may not reshape the
//! shared DTO. The family therefore appends the cursor to the prompt id after the upstream content
//! hash: `<12 hex chars>@<row>`.
//!
//! * The 12-hex prefix is the pinned `finishPrompt` hash over `{agent, kind, title, question,
//!   body, options, multi_select, custom_option_index}` — byte-identical to upstream, so a content
//!   change still changes it and the footer's ticking clock still does not (upstream's own test).
//! * A cursor move changes only the suffix. Upstream excludes cursor movement from the id; here it
//!   makes an open card stale, which refuses rather than misfires — the safe direction, and the
//!   same direction upstream's `aimed()` takes when the live row is not the one the moves were for.
//!
//! Nothing in this module has been executed: the run is deferred by explicit instruction
//! (`.omo/ulw-execute/herdr-reference-chat-parity-execution.md`).

use std::sync::OnceLock;

use regex::Regex;
use sha2::{Digest, Sha256};

use super::types::{
    ReferenceAnswerPlanner, ReferenceKeyStep, ReferencePrompt, ReferencePromptAnswer,
    ReferencePromptDetector, ReferencePromptKind, ReferencePromptOption,
};
use crate::scoped_contracts::ScopeErrorCode;

/// The registry id whose dialogs this lane reads. Upstream matches it exactly (`agent === "pi"`).
pub const PI_AGENT: &str = "pi";

/// pi's own footer under `/model` and under every dialog alike: the pane's folder, then its
/// context meter. A hint is allowed this many lines of it before the bottom of the screen
/// (`PI_FOOTER_LINES`).
pub const PI_FOOTER_LINES: usize = 2;

/// Separates the upstream content hash from the cursor row in a pi prompt id (see the module
/// docs). The prompt id is opaque to every other lane.
pub const PI_PROMPT_CURSOR_SEPARATOR: char = '@';

/// A refusal for a prompt this lane did not produce — the frozen DTO cannot carry the cursor, so
/// an id without the family's suffix is not one of ours. Upstream throws the same way when its
/// parse table has no entry for the prompt.
pub const PI_UNKNOWN_PROMPT: &str = "The prompt was not produced by parse_pi_prompt.";

/// `Exactly one answer is required.`
pub const PI_INVALID_ONE_ANSWER: &str = "Exactly one answer is required.";
/// `This prompt does not accept a custom answer.`
pub const PI_INVALID_CUSTOM_ANSWER: &str = "This prompt does not accept a custom answer.";
/// `This prompt requires one or more selections.`
pub const PI_INVALID_SELECTIONS: &str = "This prompt requires one or more selections.";
/// `An option index is outside the displayed range.`
pub const PI_INVALID_RANGE: &str = "An option index is outside the displayed range.";
/// `This agent does not support multiple selections.`
pub const PI_INVALID_MULTI_UNSUPPORTED: &str = "This agent does not support multiple selections.";
/// `A valid option index is required.`
pub const PI_INVALID_OPTION: &str = "A valid option index is required.";

/// Which of pi's readers produced a prompt. Upstream's `Responder` union, pi's members only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PiResponder {
    /// `/model`'s catalogue widget.
    Model,
    /// An extension's `ctx.ui.select`, or a dialog that asks among its options.
    Question,
    /// A Yes/No dialog: an approval.
    Confirm,
    /// A dialog that wants text on its `>` line.
    Input,
}

/// One row of pi's menu or catalogue, as the reader took it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PiPromptRow {
    /// The row's label, wrapped continuations joined back on.
    pub label: String,
    /// Whether pi drew its cursor (`→` / `❯` / `➜`) on this row.
    pub cursor: bool,
    /// Whether pi's tick marks this as the model answering now (`/model` only).
    pub current: bool,
}

/// A parsed pi prompt, with the internal position the public DTO cannot carry.
#[derive(Debug, Clone)]
pub struct PiParsedPrompt {
    /// The card the chat renders and the answer names.
    pub prompt: ReferencePrompt,
    /// Which reader produced it.
    pub responder: PiResponder,
    /// The rows as pi drew them, before the label trim the card shows.
    pub menu_labels: Vec<String>,
    /// Where pi's cursor stood when the prompt was read.
    pub selected_index: usize,
    /// The row a typed answer is typed into, when the menu has one.
    pub custom_menu_index: Option<usize>,
    /// Whether the menu toggles rows. Always false for pi: a pi dialog takes one row.
    pub multi_select: bool,
}

// ---------------------------------------------------------------------------------------------
// Shared line rules, ported from the pinned `cleanLine` / `isDivider` / `wrapped` / `hintAtEnd`.
// ---------------------------------------------------------------------------------------------

fn ansi_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| Regex::new(r"\x1b\[[0-?]*[ -/]*[@-~]").expect("pinned ANSI pattern compiles"))
}

/// `DIVIDER_RE`.
fn divider_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| {
        Regex::new(r"^[\s╭╮╰╯├┤┬┴┼─━═╌▔]+$").expect("pinned divider pattern compiles")
    })
}

/// `cleanLine`: strip ANSI, trim, then drop a box border and trim again.
fn clean_line(raw: &str) -> String {
    let stripped = ansi_re().replace_all(raw, "");
    let mut line = stripped.trim().to_string();
    if let Some(rest) = line.strip_prefix('│') {
        line = rest.trim_start().to_string();
    }
    if let Some(rest) = line.strip_suffix('│') {
        line = rest.trim_end().to_string();
    }
    line.trim().to_string()
}

/// `isDivider`.
fn is_divider(raw: &str) -> bool {
    let value = clean_line(raw);
    !value.is_empty() && divider_re().is_match(&value)
}

/// `screen.replace(ANSI_RE, "").split(/\r?\n/)`.
fn screen_lines(screen: &str) -> Vec<String> {
    ansi_re()
        .replace_all(screen, "")
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_string())
        .collect()
}

/// The screen's lines as `promptTailIsActive` reads them: ANSI-stripped, cleaned, non-empty, no
/// dividers.
fn visible_lines(screen: &str) -> Vec<String> {
    screen_lines(screen)
        .iter()
        .map(|line| clean_line(line))
        .filter(|line| !line.is_empty() && !is_divider(line))
        .collect()
}

fn find_last_index<F: Fn(&str, usize) -> bool>(lines: &[String], predicate: F) -> Option<usize> {
    lines
        .iter()
        .enumerate()
        .rev()
        .find(|(index, line)| predicate(line, *index))
        .map(|(index, _)| index)
}

/// `wrapped`: a line and the ones after it, joined, so a narrow pane's hint matches across the
/// wrap. The last line a window matches from is where the hint begins.
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

/// `hintAtEnd`: whether a hint is the last thing before the agent's footer, allowing for a phone's
/// pane being too narrow to hold it on one line. Anchored to the end of the joined window, because
/// a hint's words also match a join that merely starts with them — the footer's own lines read as a
/// hint that way, which would keep an answered, buried list offering a switch into whatever the
/// pane shows by then.
fn hint_at_end(shown: &[String], at_end: &Regex, footer_lines: usize, span: usize) -> bool {
    if shown.is_empty() {
        return false;
    }
    let last = shown.len() - 1;
    let floor = last.saturating_sub(footer_lines);
    let mut end = last as isize;
    while end >= floor as isize {
        for size in 1..=span {
            if (end as usize) + 1 < size {
                break;
            }
            let from = end as usize + 1 - size;
            if at_end.is_match(&shown[from..=end as usize].join(" ")) {
                return true;
            }
        }
        end -= 1;
    }
    false
}

// ---------------------------------------------------------------------------------------------
// pi's own patterns, ported from the pinned `PI_*` constants.
// ---------------------------------------------------------------------------------------------

/// `PI_MENU_HINT_RE`: what pi's menu widget says it takes.
fn menu_hint_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| {
        Regex::new(r"(?i)↑↓ navigate\s+enter select\s+escape/ctrl\+c cancel")
            .expect("pinned pi menu hint compiles")
    })
}

/// `PI_INPUT_HINT_RE`: what pi's text dialog says it takes.
fn input_hint_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| {
        Regex::new(r"(?i)enter submit\s+escape/ctrl\+c cancel")
            .expect("pinned pi input hint compiles")
    })
}

/// `PI_MENU_HINT_AT_END_RE`.
fn menu_hint_at_end_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| {
        Regex::new(r"(?i)↑↓ navigate\s+enter select\s+escape/ctrl\+c cancel$")
            .expect("pinned pi menu hint anchor compiles")
    })
}

/// `PI_INPUT_HINT_AT_END_RE`.
fn input_hint_at_end_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| {
        Regex::new(r"(?i)enter submit\s+escape/ctrl\+c cancel$")
            .expect("pinned pi input hint anchor compiles")
    })
}

/// `PI_MODEL_HINT_RE`: `/model`'s own widget, which carries no `↑↓ navigate`.
fn model_hint_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| {
        Regex::new(r"(?i)enter to select\s*·\s*ctrl\+s to set as default\s*·\s*escape/ctrl\+c to cancel")
            .expect("pinned pi model hint compiles")
    })
}

/// `PI_MODEL_HINT_AT_END_RE`.
fn model_hint_at_end_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| {
        Regex::new(
            r"(?i)enter to select\s*·\s*ctrl\+s to set as default\s*·\s*escape/ctrl\+c to cancel$",
        )
        .expect("pinned pi model hint anchor compiles")
    })
}

/// `PI_INPUT_LINE_RE`: the line pi types an answer into.
fn input_line_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| Regex::new(r"^[›>❯]+\s*(.*)$").expect("pinned pi input line compiles"))
}

/// `PI_ROW_RE`: pi's cursor, an optional tick, then the label.
fn row_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| {
        Regex::new(r"^([→❯➜])?\s*(?:[✓✔]\s+)?(\S.*)$").expect("pinned pi row compiles")
    })
}

/// Whether a cleaned row carries pi's cursor. `^[\u2192\u276f\u279c]\s*\S`.
fn row_cursor_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| Regex::new(r"^[→❯➜]\s*\S").expect("pinned pi cursor compiles"))
}

/// `PI_WRAPPED_REST_RE`: the rest of a row a narrow pane wrapped, one column in. Upstream writes
/// `^ (?![\u2192\u276f\u279c])\S`; the regex crate has no lookahead, and the lookahead's `\S` tests
/// the very character it consumes, so the exact equivalent is a single class.
fn wrapped_rest_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| {
        Regex::new(r"^ [^\s→❯➜]").expect("pinned pi wrapped rest compiles")
    })
}

/// `PI_MODEL_FILTER_RE`: the line `/model` types a filter into.
fn model_filter_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| Regex::new(r"^[›>❯]\s*$").expect("pinned pi filter compiles"))
}

/// `PI_MODEL_CURRENT_RE`: pi ticks the model answering now.
fn model_current_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| Regex::new(r"[✓✔]").expect("pinned pi current mark compiles"))
}

/// `PI_MODEL_DEFAULT_RE`: the mark for the model pi starts on.
fn model_default_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| Regex::new(r"\s*·\s*default$").expect("pinned pi default mark compiles"))
}

/// `PI_MODEL_PROVIDER_RE`: a catalogue row names the provider serving the model, in brackets.
fn model_provider_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| Regex::new(r"\[[^\]]+\]").expect("pinned pi provider bracket compiles"))
}

/// `PI_MODEL_TAIL_RE`: the tail of a model's name a narrow pane wrapped onto its own line.
fn model_tail_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| {
        Regex::new(r"^\[[^\]]+\](\s*·\s*default)?$").expect("pinned pi model tail compiles")
    })
}

/// `^ {0,1}\S`: the column a wrapped tail arrives in.
fn model_tail_lead_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| Regex::new(r"^ {0,1}\S").expect("pinned pi tail lead compiles"))
}

/// `^ {2,}`: the indentation a non-cursor row carries.
fn model_row_indent_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| Regex::new(r"^ {2,}").expect("pinned pi row indent compiles"))
}

/// `\s{2,}`: a row is a single label; a gap is a second column, which is a command palette.
fn label_gap_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| Regex::new(r"\s{2,}").expect("pinned pi label gap compiles"))
}

/// `^yes\b`.
fn yes_lead_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| Regex::new(r"(?i)^yes\b").expect("pinned yes lead compiles"))
}

/// `^no\b`.
fn no_lead_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| Regex::new(r"(?i)^no\b").expect("pinned no lead compiles"))
}

// ---------------------------------------------------------------------------------------------
// The prompt id: upstream's `finishPrompt` hash, plus the cursor carrier.
// ---------------------------------------------------------------------------------------------

/// `JSON.stringify`'s string encoding, which the pinned id hashes over.
fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The wire name of a kind, as the id hashes it.
fn kind_name(kind: ReferencePromptKind) -> &'static str {
    match kind {
        ReferencePromptKind::Question => "question",
        ReferencePromptKind::Approval => "approval",
        ReferencePromptKind::Plan => "plan",
        ReferencePromptKind::Menu => "menu",
    }
}

/// The exact bytes `finishPrompt` hashes: `JSON.stringify({ agent, kind, title, question, body,
/// options, multi_select, custom_option_index })`, key order included.
fn pi_hash_input(
    kind: ReferencePromptKind,
    title: &str,
    question: &str,
    options: &[String],
    multi_select: bool,
    custom_option_index: Option<usize>,
) -> String {
    let mut out = String::with_capacity(256);
    out.push_str("{\"agent\":");
    out.push_str(&json_string(PI_AGENT));
    out.push_str(",\"kind\":");
    out.push_str(&json_string(kind_name(kind)));
    out.push_str(",\"title\":");
    out.push_str(&json_string(title));
    out.push_str(",\"question\":");
    out.push_str(&json_string(question));
    out.push_str(",\"body\":null,\"options\":[");
    for (index, label) in options.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"label\":");
        out.push_str(&json_string(label));
        out.push_str(",\"description\":null}");
    }
    out.push_str("],\"multi_select\":");
    out.push_str(if multi_select { "true" } else { "false" });
    out.push_str(",\"custom_option_index\":");
    match custom_option_index {
        Some(index) => out.push_str(&index.to_string()),
        None => out.push_str("null"),
    }
    out.push('}');
    out
}

/// The 12 hex characters upstream keeps of the id hash.
fn pi_content_hash(hash_input: &str) -> String {
    let digest = Sha256::digest(hash_input.as_bytes());
    let mut hex = String::with_capacity(64);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex.truncate(12);
    hex
}

/// A pi prompt id: the upstream content hash, then the cursor row this lane must navigate from.
fn pi_prompt_id(hash_input: &str, cursor: usize) -> String {
    format!("{}{}{}", pi_content_hash(hash_input), PI_PROMPT_CURSOR_SEPARATOR, cursor)
}

/// The cursor row a pi prompt id carries, when the id is one of this family's.
pub fn pi_prompt_cursor(id: &str) -> Option<usize> {
    let (_, tail) = id.rsplit_once(PI_PROMPT_CURSOR_SEPARATOR)?;
    tail.parse::<usize>().ok()
}

/// The upstream part of a pi prompt id — the 12-hex content hash, without this lane's cursor
/// suffix. An id from another family is returned unchanged.
pub fn pi_prompt_content_id(id: &str) -> &str {
    match id.split_once(PI_PROMPT_CURSOR_SEPARATOR) {
        Some((content, _)) => content,
        None => id,
    }
}

// ---------------------------------------------------------------------------------------------
// Readers.
// ---------------------------------------------------------------------------------------------

struct PiDraft<'a> {
    kind: ReferencePromptKind,
    title: &'a str,
    question: &'a str,
    options: &'a [String],
    multi_select: bool,
    custom_option_index: Option<usize>,
    responder: PiResponder,
    menu_labels: Vec<String>,
    selected_index: usize,
    custom_menu_index: Option<usize>,
}

fn finish_pi_prompt(draft: PiDraft<'_>) -> PiParsedPrompt {
    let id = pi_prompt_id(
        &pi_hash_input(
            draft.kind,
            draft.title,
            draft.question,
            draft.options,
            draft.multi_select,
            draft.custom_option_index,
        ),
        draft.selected_index,
    );
    let prompt = ReferencePrompt {
        id,
        agent: PI_AGENT.to_string(),
        kind: draft.kind,
        title: draft.title.to_string(),
        question: draft.question.to_string(),
        body: None,
        options: draft
            .options
            .iter()
            .map(|label| ReferencePromptOption { label: label.clone(), description: None })
            .collect(),
        multi_select: draft.multi_select,
        custom_option_index: draft.custom_option_index.map(|index| index as u32),
        queued: None,
        steps: Vec::new(),
        fallback: None,
    };
    PiParsedPrompt {
        prompt,
        responder: draft.responder,
        menu_labels: draft.menu_labels,
        selected_index: draft.selected_index,
        custom_menu_index: draft.custom_menu_index,
        multi_select: draft.multi_select,
    }
}

/// `piModelRows`: the catalogue `/model` lists under its filter line.
///
/// A row cut before its provider bracket closes is a catalogue still being drawn, not a note, and
/// any row still missing its provider voids the whole reading — offering half a name would switch
/// pi to a model that does not exist.
fn pi_model_rows(lines: &[String], start_index: usize) -> Option<Vec<PiPromptRow>> {
    let mut rows: Vec<PiPromptRow> = Vec::new();

    // pi leaves a blank between its filter line and what still matches it
    let mut start = start_index;
    while start < lines.len() && clean_line(&lines[start]).is_empty() {
        start += 1;
    }

    // A phone leaves pi a pane barely wider than a model's name, which wraps it and drops the
    // provider's bracket onto the next line at column zero, where it reads exactly like a note.
    // Join such a tail back onto the line it wrapped from before reading any row.
    let mut block: Vec<String> = Vec::new();
    for line in lines.iter().skip(start) {
        let raw = ansi_re().replace_all(line, "").to_string();
        let cleaned = clean_line(&raw);
        if let Some(previous) = block.last() {
            let previous_clean = clean_line(previous);
            if !previous_clean.is_empty()
                && !model_provider_re().is_match(&previous_clean)
                && model_tail_lead_re().is_match(&raw)
                && model_tail_re().is_match(&cleaned)
            {
                let joined = format!("{previous} {cleaned}");
                let last = block.len() - 1;
                block[last] = joined;
                continue;
            }
        }
        block.push(raw);
    }

    let cut_mid_bracket = |line: &str| line.contains('[') && !model_provider_re().is_match(line);
    for raw in &block {
        let line = clean_line(raw);
        if line.is_empty() || is_divider(&line) || model_hint_re().is_match(&line) {
            break;
        }
        let cursor = row_cursor_re().is_match(&line);
        if !cursor && !model_row_indent_re().is_match(raw) {
            break;
        }
        let label = row_re()
            .captures(&line)
            .and_then(|captures| captures.get(2))
            .map(|label| label.as_str().trim().to_string())
            .filter(|label| !label.is_empty());
        let usable = label
            .as_deref()
            .is_some_and(|label| model_provider_re().is_match(label) && !label_gap_re().is_match(label));
        if !usable {
            if cut_mid_bracket(&line) {
                return None;
            }
            break;
        }
        let label = label.expect("a usable label is present");
        rows.push(PiPromptRow { label, cursor, current: model_current_re().is_match(raw) });
    }

    if !rows.iter().all(|row| model_provider_re().is_match(&row.label)) {
        return None;
    }
    if rows.len() >= 2 && rows.iter().any(|row| row.cursor) {
        Some(rows)
    } else {
        None
    }
}

/// `parsePiModel`: the `/model` catalogue, with the model answering now named in the card.
pub fn parse_pi_model(screen: &str) -> Option<PiParsedPrompt> {
    let lines = screen_lines(screen);
    let hint_index = find_last_index(&lines, |_, index| {
        model_hint_re().is_match(&wrapped(&lines, index, 3))
    })?;
    // the filter line is the anchor: the catalogue is what sits under it, and anything above
    // belongs to whatever the pane showed before `/model` was typed
    let filter_index = find_last_index(&lines[..hint_index], |line, _| {
        model_filter_re().is_match(&clean_line(line))
    })?;
    let rows = pi_model_rows(&lines, filter_index + 1)?;
    let selected_index = rows.iter().position(|row| row.cursor)?;
    let labels: Vec<String> = rows.iter().map(|row| row.label.clone()).collect();
    let question = match rows.iter().find(|row| row.current) {
        Some(current) => format!(
            "Select model (currently {})",
            model_default_re().replace(&current.label, "").to_string()
        ),
        None => "Select model".to_string(),
    };
    Some(finish_pi_prompt(PiDraft {
        kind: ReferencePromptKind::Question,
        title: "",
        question: &question,
        options: &labels,
        multi_select: false,
        custom_option_index: None,
        responder: PiResponder::Model,
        menu_labels: labels.clone(),
        selected_index,
        custom_menu_index: None,
    }))
}

/// One row of a pi dialog, as `piDialogRows` took it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PiDialogRow {
    line: String,
    cursor: bool,
}

/// The rows a dialog takes its answer from, and its own words over them.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PiDialogBlock {
    rows: Vec<PiDialogRow>,
    title: Vec<String>,
}

/// `piDialogRows`: pi separates the rows from its own words with a blank line and ends the block
/// with its hint, so the run of lines directly above the hint is what its arrow keys move through.
fn pi_dialog_rows(lines: &[String], hint_index: usize) -> PiDialogBlock {
    let mut rows: Vec<PiDialogRow> = Vec::new();
    let mut index: isize = hint_index as isize - 1;
    while index >= 0 && clean_line(&lines[index as usize]).is_empty() {
        index -= 1;
    }

    // read upwards, so the wrapped rest of a row is met before the row it belongs to
    let mut rest: Vec<String> = Vec::new();
    while index >= 0 {
        let raw = ansi_re().replace_all(&lines[index as usize], "").to_string();
        let line = clean_line(&raw);
        if line.is_empty() || is_divider(&line) {
            break;
        }
        if wrapped_rest_re().is_match(&raw) {
            rest.insert(0, line);
            index -= 1;
            continue;
        }
        let mut joined = vec![line.clone()];
        joined.extend(rest.iter().cloned());
        rows.insert(0, PiDialogRow { line: joined.join(" "), cursor: row_cursor_re().is_match(&line) });
        rest.clear();
        index -= 1;
    }

    // lines one column in with no row over them are not a wrapped option: they stand as they are,
    // for the caller to refuse (the slash palette's own first line, for one)
    if !rest.is_empty() {
        let mut combined: Vec<PiDialogRow> =
            rest.drain(..).map(|line| PiDialogRow { line, cursor: false }).collect();
        combined.extend(rows);
        rows = combined;
    }

    let mut title: Vec<String> = Vec::new();
    while index >= 0 && title.len() < 8 {
        if is_divider(&lines[index as usize]) {
            break;
        }
        let line = clean_line(&lines[index as usize]);
        if line.is_empty() {
            if !title.is_empty() {
                break;
            }
            index -= 1;
            continue;
        }
        title.insert(0, line);
        index -= 1;
    }

    PiDialogBlock { rows, title }
}

/// `parsePiDialog`: an extension's `select` / `confirm` / `input`, read off the hint at the
/// dialog's end.
///
/// `moved`: whether an answer's own keys moved the cursor. Without it the card is offered only
/// with the cursor on the first row, because the chat would otherwise navigate from a position it
/// cannot see; with it, the reader looks where the cursor stands.
pub fn parse_pi_dialog(screen: &str, moved: bool) -> Option<PiParsedPrompt> {
    let lines = screen_lines(screen);
    let hint_index = find_last_index(&lines, |_, index| {
        let hint = wrapped(&lines, index, 3);
        menu_hint_re().is_match(&hint) || input_hint_re().is_match(&hint)
    })?;
    let block = pi_dialog_rows(&lines, hint_index);
    if block.rows.is_empty() {
        return None;
    }
    // the first line is the dialog's own; what follows is a confirm's message, or more of a
    // wrapped line
    let title = block.title.first().cloned();
    let body = if block.title.len() > 1 { Some(block.title[1..].join(" ")) } else { None };

    // pi wants text on a `>` line: there is nothing to pick, and the answer is typed into it
    if input_hint_re().is_match(&wrapped(&lines, hint_index, 3)) {
        if !block.rows.iter().all(|row| input_line_re().is_match(&row.line)) {
            return None;
        }
        let options = vec!["Type your answer".to_string()];
        return Some(finish_pi_prompt(PiDraft {
            kind: ReferencePromptKind::Question,
            title: body.as_deref().unwrap_or_default(),
            question: title.as_deref().unwrap_or_default(),
            options: &options,
            multi_select: false,
            custom_option_index: Some(0),
            responder: PiResponder::Input,
            menu_labels: Vec::new(),
            selected_index: 0,
            custom_menu_index: Some(0),
        }));
    }

    // every line of the block must be a row, and a row must be a single label: pi sets its command
    // palette out in two columns, and offering those as options would run a slash command on a click
    let mut labels: Vec<String> = Vec::new();
    for row in &block.rows {
        let captures = row_re().captures(&row.line)?;
        let label = captures.get(2)?;
        if label_gap_re().is_match(label.as_str()) {
            return None;
        }
        labels.push(label.as_str().trim().to_string());
    }
    if labels.len() != block.rows.len() || labels.len() < 2 {
        return None;
    }

    // a dialog moved through by hand sits wherever its last key left the cursor, and the chat would
    // then navigate from a position it cannot see
    let cursor = match block.rows.iter().position(|row| row.cursor) {
        Some(at) if moved || at == 0 => at,
        _ => return None,
    };

    // Yes/No reads as a confirmation; anything else is a question asked among its options
    let confirming = labels.len() == 2
        && yes_lead_re().is_match(&labels[0])
        && no_lead_re().is_match(&labels[1]);
    let menu_labels: Vec<String> = block.rows.iter().map(|row| row.line.clone()).collect();
    let question = if confirming {
        body.clone().or_else(|| title.clone()).unwrap_or_default()
    } else {
        block.title.join(" ")
    };
    Some(finish_pi_prompt(PiDraft {
        kind: if confirming { ReferencePromptKind::Approval } else { ReferencePromptKind::Question },
        title: if confirming { title.as_deref().unwrap_or_default() } else { "" },
        question: &question,
        options: &labels,
        multi_select: false,
        custom_option_index: None,
        responder: if confirming { PiResponder::Confirm } else { PiResponder::Question },
        menu_labels,
        selected_index: cursor,
        custom_menu_index: None,
    }))
}

/// `promptTailIsActive` for pi's responders: is the hint still the last thing before pi's footer?
///
/// An answered dialog leaves its hint on screen while pi carries on under it, so the card that was
/// offered is withdrawn rather than reoffered.
pub fn pi_prompt_tail_is_active(parsed: &PiParsedPrompt, screen: &str) -> bool {
    let shown = visible_lines(screen);
    match parsed.responder {
        PiResponder::Model => hint_at_end(&shown, model_hint_at_end_re(), PI_FOOTER_LINES, 3),
        PiResponder::Question | PiResponder::Confirm | PiResponder::Input => {
            hint_at_end(&shown, menu_hint_at_end_re(), PI_FOOTER_LINES, 4)
                || hint_at_end(&shown, input_hint_at_end_re(), PI_FOOTER_LINES, 4)
        }
    }
}

/// Detect pi's own dialog on a screen — the family's [`ReferencePromptDetector`].
///
/// Only `pi` is claimed: the reference names pi's dialogs for the `pi` registry id alone, and a
/// lane must not offer a provider the reference does not name. Upstream's pi branch falls through
/// to OmO's readers afterwards; that composition belongs to the dispatcher (task 9), not here.
pub fn parse_pi_prompt(agent: &str, screen: &str) -> Option<ReferencePrompt> {
    if agent != PI_AGENT {
        return None;
    }
    if let Some(parsed) = parse_pi_model(screen) {
        if pi_prompt_tail_is_active(&parsed, screen) {
            return Some(parsed.prompt);
        }
    }
    if let Some(parsed) = parse_pi_dialog(screen, false) {
        if pi_prompt_tail_is_active(&parsed, screen) {
            return Some(parsed.prompt);
        }
    }
    None
}

/// The lane's detector as the shared signature sees it (task 9's dispatch table).
pub fn pi_prompt_detector() -> ReferencePromptDetector {
    parse_pi_prompt
}

// ---------------------------------------------------------------------------------------------
// Answer planning: upstream's `answerKeys`, pi's branches.
// ---------------------------------------------------------------------------------------------

/// `navigationKeys`: one move per row between the cursor and the choice.
fn navigation_keys(delta: isize) -> Vec<&'static str> {
    let mut keys = Vec::new();
    let mut remaining = delta.abs();
    while remaining > 0 {
        keys.push(if delta > 0 { "down" } else { "up" });
        remaining -= 1;
    }
    keys
}

/// `keySteps`: one step per key, never an empty step.
fn key_steps(keys: &[&str]) -> Vec<ReferenceKeyStep> {
    keys.iter().map(|key| ReferenceKeyStep::keys([*key])).collect()
}

/// Does this prompt come from pi's text dialog?
///
/// pi's only dialog that takes typed text is the `>` line one, and it is the only pi prompt whose
/// card carries a custom row — at index 0, the row the answer is typed into.
fn is_pi_input(prompt: &ReferencePrompt) -> bool {
    prompt.custom_option_index == Some(0)
}

/// Turn an answer into the keys that answer pi's prompt on the original pane — the family's
/// [`ReferenceAnswerPlanner`].
///
/// The refusals are upstream's `InvalidAnswer` cases, word for word. pi presses **No** rather than
/// cancelling: `rejectWithEscapeIndex` is null for every pi prompt, so no answer of this family
/// becomes an Escape — a confirmation's decline presses the row pi shows.
pub fn plan_pi_answer(
    prompt: &ReferencePrompt,
    answer: &ReferencePromptAnswer,
) -> Result<Vec<ReferenceKeyStep>, String> {
    if !answer.is_single_choice() {
        return Err(PI_INVALID_ONE_ANSWER.to_string());
    }
    let cursor = pi_prompt_cursor(&prompt.id).ok_or_else(|| PI_UNKNOWN_PROMPT.to_string())?;
    let custom = prompt.custom_option_index.map(|index| index as usize);

    if let Some(text) = answer.custom_text.as_deref() {
        let text = text.trim();
        if text.is_empty() || prompt.multi_select || custom.is_none() {
            return Err(PI_INVALID_CUSTOM_ANSWER.to_string());
        }
        let custom_index = custom.expect("checked above");
        let input = is_pi_input(prompt);
        let mut keys = navigation_keys(custom_index as isize - cursor as isize);
        // Codex's queue types into its last row once it is selected: no enter first. pi's text
        // dialog is the same but for a worse reason: its `>` line already owns the input, so an
        // enter typed before the answer submits the dialog empty and leaves the answer behind to be
        // typed into the agent's own prompt.
        if !input {
            keys.push("enter");
        }
        // what was typed into pi's line in the terminal would stay around the answer: the line is
        // emptied first, after the cursor and before it (pi's editor keys, measured on 0.87.1)
        if input {
            keys.push("ctrl+k");
            keys.push("ctrl+u");
        }
        let mut steps = key_steps(&keys);
        steps.push(ReferenceKeyStep::typed(text));
        steps.push(ReferenceKeyStep::keys(["enter"]));
        return Ok(steps);
    }

    if let Some(indices) = answer.option_indices.as_deref() {
        if !prompt.multi_select || indices.is_empty() {
            return Err(PI_INVALID_SELECTIONS.to_string());
        }
        if indices.iter().any(|index| *index as usize >= prompt.options.len()) {
            return Err(PI_INVALID_RANGE.to_string());
        }
        // a pi dialog takes one row: nothing of this family toggles, and upstream refuses the rest
        return Err(PI_INVALID_MULTI_UNSUPPORTED.to_string());
    }

    let Some(index) = answer.option_index else {
        return Err(PI_INVALID_OPTION.to_string());
    };
    let index = index as usize;
    if index >= prompt.options.len() || Some(index) == custom || prompt.multi_select {
        return Err(PI_INVALID_OPTION.to_string());
    }
    let mut keys = navigation_keys(index as isize - cursor as isize);
    keys.push("enter");
    Ok(key_steps(&keys))
}

/// The lane's answer planner as the shared signature sees it (task 9's dispatch table).
pub fn pi_answer_planner() -> ReferenceAnswerPlanner {
    plan_pi_answer
}

/// The scope error a pi answer refusal maps to.
///
/// Every refusal above is an `InvalidAnswer` upstream, which the reference's answer route answers
/// as a bad request. The dispatcher (task 9) returns `ScopeErrorCode`, so the lane names the
/// mapping rather than leaving the caller to guess it.
pub fn pi_answer_error_code() -> ScopeErrorCode {
    ScopeErrorCode::InvalidRequest
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An extension's `select` on a wide pane, with pi's footer under it.
    const DIALOG_SELECT: &str = include_str!("fixtures/prompt-pi/dialog-select.txt");
    /// The same dialog with pi's cursor moved off the first row by hand.
    const DIALOG_SELECT_CURSOR_MOVED: &str =
        include_str!("fixtures/prompt-pi/dialog-select-cursor-moved.txt");
    /// The same dialog with a different last option.
    const DIALOG_SELECT_OTHER_LABEL: &str =
        include_str!("fixtures/prompt-pi/dialog-select-other-label.txt");
    /// The same dialog whose footer's clock has ticked.
    const DIALOG_SELECT_FOOTER_TIME: &str =
        include_str!("fixtures/prompt-pi/dialog-select-footer-time-changed.txt");
    /// The dialog in a pane narrow enough to wrap a row.
    const DIALOG_SELECT_WRAPPED: &str =
        include_str!("fixtures/prompt-pi/dialog-select-wrapped-narrow.txt");
    /// The same, with the cursor on the row that is not wrapped.
    const DIALOG_SELECT_WRAPPED_SECOND: &str =
        include_str!("fixtures/prompt-pi/dialog-select-wrapped-narrow-second.txt");
    /// A confirm, whose message sits under its own title.
    const DIALOG_CONFIRM: &str = include_str!("fixtures/prompt-pi/dialog-confirm.txt");
    /// A confirm whose message a narrow pane wrapped over two lines.
    const DIALOG_CONFIRM_WRAPPED: &str = include_str!("fixtures/prompt-pi/dialog-confirm-wrapped.txt");
    /// A dialog that wants text on its `>` line.
    const DIALOG_INPUT: &str = include_str!("fixtures/prompt-pi/dialog-input.txt");
    /// `/login` in a 46-column pane: its wrapped hint sits over pi's footer.
    const DIALOG_LOGIN_NARROW: &str = include_str!("fixtures/prompt-pi/dialog-login-narrow.txt");
    /// The same screen once later output has buried the hint.
    const DIALOG_LOGIN_NARROW_BURIED: &str =
        include_str!("fixtures/prompt-pi/dialog-login-narrow-buried.txt");
    /// An answered dialog whose hint is still on screen, far above the bottom.
    const DIALOG_ANSWERED_BURIED: &str = include_str!("fixtures/prompt-pi/dialog-answered-buried.txt");
    /// pi's own main prompt, with no dialog at all.
    const DIALOG_EMPTY: &str = include_str!("fixtures/prompt-pi/dialog-empty.txt");
    /// The `/model` catalogue on a wide pane, with pi's own notes among the rows.
    const MODEL_WIDE: &str = include_str!("fixtures/prompt-pi/model-wide.txt");
    /// The same catalogue with pi's cursor on the model that is not the one in use.
    const MODEL_WIDE_CURSOR_MOVED: &str =
        include_str!("fixtures/prompt-pi/model-wide-cursor-moved.txt");
    /// The same catalogue cut in the middle of a row's provider bracket.
    const MODEL_WIDE_CUT_MID_NAME: &str =
        include_str!("fixtures/prompt-pi/model-wide-cut-mid-name.txt");
    /// The same catalogue filtered down to one row.
    const MODEL_WIDE_ONE_ROW: &str = include_str!("fixtures/prompt-pi/model-wide-one-row.txt");
    /// The same catalogue with pi's cursor on no row at all.
    const MODEL_WIDE_NO_CURSOR: &str = include_str!("fixtures/prompt-pi/model-wide-no-cursor.txt");
    /// `/model` in a 46-column pane: the hint splits and a model's name drops its bracket.
    const MODEL_NARROW: &str = include_str!("fixtures/prompt-pi/model-narrow.txt");
    /// The same screen once later output has buried the wrapped hint.
    const MODEL_NARROW_BURIED: &str = include_str!("fixtures/prompt-pi/model-narrow-buried.txt");
    /// A row cut in the middle of its provider's bracket.
    const MODEL_HALF_NAME_CUT: &str = include_str!("fixtures/prompt-pi/model-half-name-cut.txt");
    /// pi's slash palette: two columns, which are commands rather than answers.
    const PALETTE_TWO_COLUMNS: &str = include_str!("fixtures/prompt-pi/palette-two-columns.txt");
    /// `/tree`, whose hint says `↑/↓ move` and which the chat must not answer.
    const TREE_NAVIGATOR: &str = include_str!("fixtures/prompt-pi/tree-navigator.txt");

    /// Upstream's `answerKeys(...).flatMap(step => step.keys ?? [`text:${step.text}`])`, so the
    /// assertions read like the reference's own `pane.sent` expectations.
    fn key_plan(prompt: &ReferencePrompt, answer: ReferencePromptAnswer) -> Vec<String> {
        plan_pi_answer(prompt, &answer)
            .expect("the answer is plannable")
            .into_iter()
            .map(|step| match step.text {
                Some(text) => format!("text:{text}"),
                None => step.keys.join("+"),
            })
            .collect()
    }

    fn labels(prompt: &ReferencePrompt) -> Vec<String> {
        prompt.options.iter().map(|option| option.label.clone()).collect()
    }

    fn option(index: u32) -> ReferencePromptAnswer {
        ReferencePromptAnswer { option_index: Some(index), ..Default::default() }
    }

    fn custom(text: &str) -> ReferencePromptAnswer {
        ReferencePromptAnswer { custom_text: Some(text.to_string()), ..Default::default() }
    }

    // -- the /model catalogue (happy path) ------------------------------------------------------

    #[test]
    fn model_wide_reads_the_catalogue_and_names_the_model_answering_now() {
        let prompt = parse_pi_prompt("pi", MODEL_WIDE).expect("the catalogue is offered");
        assert_eq!(prompt.kind, ReferencePromptKind::Question);
        assert_eq!(prompt.agent, "pi");
        assert_eq!(prompt.question, "Select model (currently vllm/Qwen/Qwen3.8-27B [lwsa-platform])");
        assert_eq!(
            labels(&prompt),
            vec![
                "vllm/Qwen/Qwen3.8-27B [lwsa-platform] · default",
                "vllm-flash/Qwen3.8-Flash-Next [lwsa-platform]",
            ]
        );
        // pi's own notes sit among the rows and are not models pi can be switched to
        assert!(!labels(&prompt).iter().any(|label| label.contains("Could not refresh")));
        assert_eq!(prompt.custom_option_index, None);
        assert!(!prompt.multi_select);
        // the cursor sits on the first row, so one option is one key step per row above it
        assert_eq!(key_plan(&prompt, option(1)), vec!["down", "enter"]);
        // the id is the pinned content hash; this lane adds the row it must navigate from
        assert_eq!(pi_prompt_content_id(&prompt.id), "d70050f3655a");
        assert_eq!(pi_prompt_cursor(&prompt.id), Some(0));
    }

    #[test]
    fn model_cursor_moved_navigates_from_where_pi_drew_the_cursor() {
        let prompt = parse_pi_prompt("pi", MODEL_WIDE_CURSOR_MOVED).expect("the catalogue is offered");
        // the card still names the model answering now, which is the one pi ticked
        assert_eq!(prompt.question, "Select model (currently vllm/Qwen/Qwen3.8-27B [lwsa-platform])");
        assert_eq!(pi_prompt_cursor(&prompt.id), Some(1));
        // the same content, a different cursor: the hash prefix is the one the wide read produced
        let wide = parse_pi_prompt("pi", MODEL_WIDE).expect("offered");
        assert_eq!(pi_prompt_content_id(&prompt.id), pi_prompt_content_id(&wide.id));
        assert_ne!(prompt.id, wide.id);
        // and the answer walks up from where pi drew the cursor, not down from row zero
        assert_eq!(key_plan(&prompt, option(0)), vec!["up", "enter"]);
    }

    #[test]
    fn model_narrow_joins_the_rows_pi_wrapped_and_stops_at_its_notes() {
        let prompt = parse_pi_prompt("pi", MODEL_NARROW).expect("the catalogue is offered");
        assert_eq!(
            labels(&prompt),
            vec![
                "vllm-flash/Qwen3.8-Flash-Next [lwsa-platform] · default",
                "vllm/Qwen/Qwen3.8-27B [lwsa-platform]",
            ]
        );
        assert_eq!(prompt.question, "Select model (currently vllm-flash/Qwen3.8-Flash-Next [lwsa-platform])");
        assert_eq!(key_plan(&prompt, option(1)), vec!["down", "enter"]);
    }

    #[test]
    fn model_narrow_goes_stale_once_the_wrapped_hint_is_buried() {
        assert!(parse_pi_prompt("pi", MODEL_NARROW).is_some());
        assert!(parse_pi_prompt("pi", MODEL_NARROW_BURIED).is_none());
    }

    #[test]
    fn model_wide_cut_mid_name_voids_the_catalogue() {
        // a screen redrawn while pi is still writing it ends a row in the middle of its provider
        // bracket: the rows before it are a catalogue pi never drew
        assert!(parse_pi_prompt("pi", MODEL_WIDE_CUT_MID_NAME).is_none());
    }

    #[test]
    fn model_half_name_cut_voids_the_catalogue() {
        // a bracket cut mid-word is not a provider: joining it back would invent `lwsa- platform`
        assert!(parse_pi_prompt("pi", MODEL_HALF_NAME_CUT).is_none());
    }

    #[test]
    fn model_one_row_or_no_cursor_offers_nothing() {
        // filtering the list down to one model leaves nothing to choose between
        assert!(parse_pi_prompt("pi", MODEL_WIDE_ONE_ROW).is_none());
        // and with the cursor on no row at all, an answer would navigate from nowhere
        assert!(parse_pi_prompt("pi", MODEL_WIDE_NO_CURSOR).is_none());
    }

    // -- pi's dialogs (happy path) --------------------------------------------------------------

    #[test]
    fn dialog_select_reads_pi_options_in_order_and_pins_the_upstream_id() {
        let prompt = parse_pi_prompt("pi", DIALOG_SELECT).expect("the dialog is offered");
        assert_eq!(prompt.kind, ReferencePromptKind::Question);
        assert_eq!(prompt.question, "Allow dangerous command?");
        assert_eq!(prompt.title, "");
        assert_eq!(labels(&prompt), vec!["Allow once", "Always allow", "Block"]);
        assert_eq!(prompt.custom_option_index, None);
        assert_eq!(key_plan(&prompt, option(1)), vec!["down", "enter"]);
        assert_eq!(key_plan(&prompt, option(2)), vec!["down", "down", "enter"]);
        assert_eq!(prompt.id, "b91f20ed7e62@0");
        assert_eq!(pi_prompt_content_id(&prompt.id), "b91f20ed7e62");
    }

    #[test]
    fn dialog_select_other_label_is_another_asking() {
        let first = parse_pi_prompt("pi", DIALOG_SELECT).expect("offered");
        let other = parse_pi_prompt("pi", DIALOG_SELECT_OTHER_LABEL).expect("offered");
        assert_eq!(
            labels(&other),
            vec!["Allow once", "Always allow", "Block and say why"]
        );
        assert_ne!(other.id, first.id);
        assert_eq!(pi_prompt_content_id(&other.id), "13e14ed07883");
    }

    #[test]
    fn dialog_select_footer_change_keeps_the_id() {
        // the id is the card's own content: what the chat polls keeps its answer open while pi
        // redraws around the dialog, and turns stale only when the dialog itself changes
        let first = parse_pi_prompt("pi", DIALOG_SELECT).expect("offered");
        let ticked = parse_pi_prompt("pi", DIALOG_SELECT_FOOTER_TIME).expect("offered");
        assert_eq!(ticked.id, first.id);
    }

    #[test]
    fn dialog_select_wrapped_narrow_joins_a_wrapped_row() {
        let prompt = parse_pi_prompt("pi", DIALOG_SELECT_WRAPPED).expect("the dialog is offered");
        assert_eq!(
            labels(&prompt),
            vec![
                "Keep it on the staging environment for now and wait for review",
                "Deploy to production",
                "Cancel",
            ]
        );
        assert_eq!(prompt.question, "Where should this change go next?");
        assert_eq!(pi_prompt_content_id(&prompt.id), "7f2fd4f1e5b2");
        assert_eq!(key_plan(&prompt, option(1)), vec!["down", "enter"]);
    }

    #[test]
    fn dialog_select_wrapped_narrow_second_row_order() {
        let prompt = parse_pi_prompt("pi", DIALOG_SELECT_WRAPPED_SECOND).expect("offered");
        assert_eq!(
            labels(&prompt),
            vec![
                "Cancel",
                "Keep it on the staging environment for now and wait for review",
                "Deploy to production",
            ]
        );
    }

    #[test]
    fn dialog_confirm_reads_as_an_approval_and_no_is_pressed() {
        let prompt = parse_pi_prompt("pi", DIALOG_CONFIRM).expect("the confirm is offered");
        assert_eq!(prompt.kind, ReferencePromptKind::Approval);
        assert_eq!(prompt.title, "Clear session?");
        assert_eq!(prompt.question, "All messages will be lost.");
        assert_eq!(labels(&prompt), vec!["Yes", "No"]);
        assert_eq!(prompt.id, "cb9d4c8c3f45@0");
        // measured on pi: pressing "No" answers the confirmation false, where Escape would leave it
        // unanswered, so the card's decline presses the row it shows
        assert_eq!(key_plan(&prompt, option(1)), vec!["down", "enter"]);
        assert_eq!(key_plan(&prompt, option(0)), vec!["enter"]);
    }

    #[test]
    fn dialog_confirm_wrapped_message_reads_whole() {
        let prompt = parse_pi_prompt("pi", DIALOG_CONFIRM_WRAPPED).expect("offered");
        assert_eq!(prompt.kind, ReferencePromptKind::Approval);
        assert_eq!(prompt.title, "Delete the branch?");
        assert_eq!(
            prompt.question,
            "This removes the local branch and its remote counterpart for good."
        );
        assert_eq!(pi_prompt_content_id(&prompt.id), "f9b284b9824e");
    }

    #[test]
    fn dialog_input_takes_text_after_emptying_the_line() {
        let prompt = parse_pi_prompt("pi", DIALOG_INPUT).expect("the input dialog is offered");
        assert_eq!(prompt.kind, ReferencePromptKind::Question);
        assert_eq!(prompt.question, "Branch name?");
        assert_eq!(prompt.custom_option_index, Some(0));
        assert_eq!(labels(&prompt), vec!["Type your answer"]);
        assert_eq!(prompt.id, "36052ff443a2@0");
        // the `>` line already owns the input: an Enter typed before the answer submits the dialog
        // empty, and the line is emptied first so terminal typing does not stay around the answer
        assert_eq!(
            key_plan(&prompt, custom("feat/x")),
            vec!["ctrl+k", "ctrl+u", "text:feat/x", "enter"]
        );
        // the custom row is not an option to pick
        assert!(plan_pi_answer(&prompt, &option(0)).is_err());
    }

    #[test]
    fn dialog_login_narrow_reads_a_dialog_whose_wrapped_hint_sits_over_the_footer() {
        let prompt = parse_pi_prompt("pi", DIALOG_LOGIN_NARROW).expect("the dialog is offered");
        assert_eq!(
            labels(&prompt),
            vec!["Sign in with an account", "Sign in with an API key"]
        );
        assert_eq!(key_plan(&prompt, option(1)), vec!["down", "enter"]);
    }

    #[test]
    fn dialog_login_narrow_goes_stale_once_buried() {
        assert!(parse_pi_prompt("pi", DIALOG_LOGIN_NARROW).is_some());
        // pi keeps the answered dialog on screen; what comes after buries the hint, and a card
        // still open then would press keys into whatever the pane shows by then
        assert!(parse_pi_prompt("pi", DIALOG_LOGIN_NARROW_BURIED).is_none());
    }

    // -- ambiguous and moved screens -----------------------------------------------------------

    #[test]
    fn dialog_moved_off_the_first_row_is_refused_unless_the_reader_moved() {
        // a dialog moved through by hand sits wherever its last key left the cursor, and the chat
        // would then navigate from a position it cannot see
        assert!(parse_pi_prompt("pi", DIALOG_SELECT_CURSOR_MOVED).is_none());
        let moved = parse_pi_dialog(DIALOG_SELECT_CURSOR_MOVED, true).expect("the moved read sees it");
        assert_eq!(moved.selected_index, 1);
        assert_eq!(labels(&moved.prompt), vec!["Allow once", "Always allow", "Block"]);
        assert_eq!(pi_prompt_cursor(&moved.prompt.id), Some(1));
        // the answer walks up to the row the tap named, from where the cursor stands
        assert_eq!(key_plan(&moved.prompt, option(0)), vec!["up", "enter"]);
    }

    #[test]
    fn answer_refuses_two_or_zero_shapes_and_out_of_range() {
        let prompt = parse_pi_prompt("pi", DIALOG_SELECT).expect("offered");
        // zero shapes: an empty answer is not an answer
        assert_eq!(
            plan_pi_answer(&prompt, &ReferencePromptAnswer::default()),
            Err(PI_INVALID_ONE_ANSWER.to_string())
        );
        // two shapes at once is ambiguous, and is refused rather than resolved by precedence
        let both = ReferencePromptAnswer {
            option_index: Some(0),
            custom_text: Some("yes".to_string()),
            ..Default::default()
        };
        assert_eq!(plan_pi_answer(&prompt, &both), Err(PI_INVALID_ONE_ANSWER.to_string()));
        // outside the displayed range
        assert_eq!(plan_pi_answer(&prompt, &option(3)), Err(PI_INVALID_OPTION.to_string()));
        // a pi dialog takes one row: nothing of this family toggles
        let indices = ReferencePromptAnswer {
            option_indices: Some(vec![0, 1]),
            ..Default::default()
        };
        assert_eq!(plan_pi_answer(&prompt, &indices), Err(PI_INVALID_SELECTIONS.to_string()));
        // and a menu does not accept typed text
        assert_eq!(plan_pi_answer(&prompt, &custom("Block")), Err(PI_INVALID_CUSTOM_ANSWER.to_string()));
        // an empty custom answer is not one either
        assert_eq!(plan_pi_answer(&prompt, &custom("   ")), Err(PI_INVALID_CUSTOM_ANSWER.to_string()));
        // a prompt this family did not produce cannot be planned: the id carries the cursor
        let mut foreign = prompt.clone();
        foreign.id = "b91f20ed7e62".to_string();
        assert_eq!(plan_pi_answer(&foreign, &option(0)), Err(PI_UNKNOWN_PROMPT.to_string()));
    }

    // -- stale and void screens ----------------------------------------------------------------

    #[test]
    fn dialog_answered_and_buried_offers_nothing() {
        assert!(parse_pi_prompt("pi", DIALOG_ANSWERED_BURIED).is_none());
    }

    #[test]
    fn palette_two_columns_and_tree_navigator_are_not_answers() {
        // the palette's rows are set out in two columns, which are commands, not answers
        assert!(parse_pi_prompt("pi", PALETTE_TWO_COLUMNS).is_none());
        // /tree navigates the session's branch, which the chat cannot undo by clicking
        assert!(parse_pi_prompt("pi", TREE_NAVIGATOR).is_none());
    }

    #[test]
    fn dialog_empty_screen_offers_nothing() {
        assert!(parse_pi_prompt("pi", DIALOG_EMPTY).is_none());
    }

    // -- the lane's boundary --------------------------------------------------------------------

    #[test]
    fn detector_claims_only_pi_and_the_aliases_are_the_same_functions() {
        for agent in ["claude", "codex", "omp", "omo", "gjc", "opencode", "aider", ""] {
            assert!(parse_pi_prompt(agent, DIALOG_SELECT).is_none(), "{agent} must not be claimed");
            assert!(parse_pi_prompt(agent, MODEL_WIDE).is_none(), "{agent} must not be claimed");
        }
        let detector = pi_prompt_detector();
        assert_eq!(
            detector("pi", DIALOG_SELECT).map(|prompt| prompt.id),
            parse_pi_prompt("pi", DIALOG_SELECT).map(|prompt| prompt.id)
        );
        let planner = pi_answer_planner();
        let prompt = parse_pi_prompt("pi", DIALOG_SELECT).expect("offered");
        assert_eq!(
            planner(&prompt, &option(2)).map(|steps| steps.len()),
            plan_pi_answer(&prompt, &option(2)).map(|steps| steps.len())
        );
        assert_eq!(pi_answer_error_code(), ScopeErrorCode::InvalidRequest);
    }

    #[test]
    fn a_pi_prompt_id_without_the_family_suffix_is_not_a_cursor() {
        assert_eq!(pi_prompt_cursor("b91f20ed7e62"), None);
        assert_eq!(pi_prompt_cursor("b91f20ed7e62@2"), Some(2));
        assert_eq!(pi_prompt_content_id("b91f20ed7e62"), "b91f20ed7e62");
        assert_eq!(pi_prompt_content_id("b91f20ed7e62@2"), "b91f20ed7e62");
    }
}
