//! OmO prompt family (plan task 25).
//!
//! Ported from `devswha/herdr-web-ui` @ `54e5a1f67090cb09552d182e7e30dd0ecc314918` (MIT, see
//! `docs/chat/HERDR_LICENSE`). The pinned sources this file ports are `server/prompt.ts` and
//! `server/omo-ask.ts`:
//!
//! * the form's constants (`OMO_ASK_TITLE_RE`, `OMO_OPTIONS_HINT_RE`, `OMO_REVIEW_HINT_RE`,
//!   `OMO_TYPING_HINT_RE`, `OMO_OWN_ANSWER`, `OMO_HINT_LINES`, `OMO_FOOTER_LINES`,
//!   `OMO_HINT_END_RE`, `OMO_NOT_FOOTER_RE`, `OMO_PENDING_STATUS_RE`, `OMO_PENDING_HINT_RE`,
//!   `OMO_EMPTY_BOX_RE`) at `prompt.ts:46-65`;
//! * the form's shape: `omoForm` `:589-605`, `askOnScreen` `:668-670`, `sameHeader` `:661-665`,
//!   `omoSteps`/`omoTitle` `:673-680`, `omoWalk` `:686-688`, `joinWrapped` `:699-713`,
//!   `screenWidth` `:716-718`, `omoAnsweredCount` `:721-724`, `omoQuestionView` `:750-776`,
//!   `askedQuestion` `:782-788`, `cutSteps` `:794-801`;
//! * the four parsers: `parseOmoQuestion` `:814-881`, `parseOmoTyping` `:893-934`,
//!   `parseOmoReview` `:954-1021`, `parseOmoPending` `:1038-1069`;
//! * the omo arm of the candidate chain `parsePrompt` `:1931-1952` and of `promptTailIsActive`
//!   `:1626-1664`, and the form marker and trust rule of `readKnownPrompt` `:2451,2462-2478`;
//! * the answer planner `answerKeys` `:1995-2055`, the card id `finishPrompt` `:228-242`, and
//!   `comparable`/`sameText` `:2522-2530`;
//! * the ask state machine of `server/omo-ask.ts` (`OMO_ASK_TOOLS`, `waits`, `omoAsksAfter`,
//!   `openOmoAsks`) and `omoAskOf` `prompt.ts:620-634`.
//!
//! What this lane owns: a screen and (when the route has read one) a session's open calls become
//! one card, and an answer becomes the keys that answer it on the original pane. What it does not
//! own: reading the pane's session file (task 3), reading the live screen (task 6), the serialized
//! write and the asking lifecycle (task 9), and every other provider's dialogs (tasks 22-24, 26).
//! The lane performs no I/O and spawns nothing.
//!
//! Port boundaries, recorded rather than skipped (see the fixture manifest for the full list):
//!
//! * `comparable` folds the compatibility range a pane's own text uses (fullwidth ASCII, the
//!   ideographic space) instead of full NFKC, because the normalization table is not a dependency
//!   of this crate and this lane may not add one.
//! * The answer plan is resolved from the card's own fields through a bounded process-local
//!   registry, because the frozen planner signature takes only the prompt and the answer.
//! * The folded widget's plans are empty on purpose: the route opens the form with its own key and
//!   plans from the opened form, as the reference does.
//! * A footer line longer than `OMO_FOOTER_MAX_CHARS` is refused, so a quoted transcript line
//!   cannot pass as OmO's footer.

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};

use regex::Regex;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use super::types::{
    ReferenceAnswerPlanner, ReferenceKeyStep, ReferencePrompt, ReferencePromptAnswer,
    ReferencePromptDetector, ReferencePromptKind, ReferencePromptOption, ReferencePromptStep,
};
use crate::scoped_contracts::ScopeErrorCode;

/// The lines a narrow pane wraps OmO's form hint onto, at most. Upstream `OMO_HINT_LINES`.
pub const OMO_HINT_LINES: usize = 5;

/// Lines of OmO's footer under the form's rule, at most. Upstream `OMO_FOOTER_LINES`.
pub const OMO_FOOTER_LINES: usize = 3;

/// OmO's own row for an answer typed in the terminal. Upstream `OMO_OWN_ANSWER`.
pub const OMO_OWN_ANSWER: &str = "Type your own answer...";

/// How long one footer line may be before it is not OmO's footer. Upstream bounds the footer by
/// its line count alone, which a quoted transcript line can defeat; this lane also bounds it by
/// width.
pub const OMO_FOOTER_MAX_CHARS: usize = 200;

/// The route's key that opens OmO's form from its folded widget. Upstream `KEY.openQueue`
/// (`prompt.ts:79`), sent as `alt+up` by the answer route (`:2677`).
pub const REFERENCE_OMO_OPEN_FORM_KEY: &str = "alt+up";

/// A card body longer than this is cut before it reaches the wire. Upstream `finishPrompt`.
pub const OMO_PROMPT_BODY_MAX_CHARS: usize = 12_000;

/// How many detected cards the answer planner can still resolve. Upstream keeps them in a
/// `WeakMap` beside the public prompt; a process-local registry needs a bound.
pub const OMO_PROMPT_CACHE_MAX: usize = 256;

/// One option of a question OmO asked, as its call recorded it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OmoAskOption {
    pub label: String,
    pub description: Option<String>,
}

/// One question of a call OmO asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OmoAskQuestion {
    pub header: String,
    pub question: String,
    pub multi_select: bool,
    pub options: Vec<OmoAskOption>,
}

/// A call OmO asked, whose questions the form on screen answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OmoAsk {
    /// Session-qualified tool-call identity.
    pub id: String,
    /// `false`: the call does not wait for its answer, and OmO folds it into a widget over its
    /// input box.
    pub wait: bool,
    pub questions: Vec<OmoAskQuestion>,
}

/// One `ask_user_question` / `request_user_input` call as the session recorded it, before its
/// arguments are checked. Upstream `OmoAskCall`.
#[derive(Debug, Clone, PartialEq)]
pub struct OmoAskCall {
    pub id: String,
    pub wait: bool,
    pub args: Value,
}

/// Which of OmO's four forms the card came from. Upstream `Responder`'s omo arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OmoResponder {
    /// The question's own options, with the row for a typed answer.
    Question,
    /// The typed answer's row opened in the terminal: save it or discard it.
    Typing,
    /// The Submit tab: submit the form, reopen a question, or write the comment.
    Review,
    /// The folded widget for a question asked without waiting.
    Pending,
}

impl OmoResponder {
    /// The reference's own name for this responder.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Question => "omo-question",
            Self::Typing => "omo-typing",
            Self::Review => "omo-review",
            Self::Pending => "omo-pending",
        }
    }
}

/// The keys before and after a typed answer, for a card that takes one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OmoCustomPlan {
    /// The keys that reach the row for a typed answer (its last key opens the row).
    pub lead: Vec<ReferenceKeyStep>,
    /// The keys after the text (usually the Enter that saves it).
    pub tail: Vec<ReferenceKeyStep>,
}

/// How a multiple choice toggles its rows: from the cursor the card was read with, walking to
/// each chosen row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OmoMultiPlan {
    /// The cursor's row when the card was read, when it was in view.
    pub cursor: Option<usize>,
    /// The rows the walk counts to the end from, when the cursor was out of view.
    pub rows: usize,
}

/// A parsed OmO card: the public prompt plus the plan its answer needs.
#[derive(Debug, Clone, PartialEq)]
pub struct OmoCard {
    pub prompt: ReferencePrompt,
    pub responder: OmoResponder,
    /// Each option's own steps, in the order the card shows them.
    pub option_plans: Vec<Vec<ReferenceKeyStep>>,
    /// The typed answer's steps, or `None` for a card that takes none.
    pub custom_plan: Option<OmoCustomPlan>,
    /// The multiple choice's steps, or `None` for a card that is not one.
    pub multi_plan: Option<OmoMultiPlan>,
    /// The option the card itself answers with Escape. OmO has none.
    pub reject_with_escape_index: Option<usize>,
}

/// The tab bar's line span and tabs. Upstream `OmoForm`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct OmoForm {
    /// The line the tab bar ends on (a narrow pane wraps it between tabs).
    bar_end: usize,
    tabs: Vec<OmoTab>,
    /// The Submit tab is the current one: the answers are reviewed.
    reviewing: bool,
}

/// One tab of the form.
#[derive(Debug, Clone, PartialEq, Eq)]
struct OmoTab {
    label: String,
    answered: bool,
    current: bool,
}

/// One option row of the question view.
#[derive(Debug, Clone, PartialEq, Eq)]
struct OmoRow {
    number: usize,
    label: Vec<String>,
    description: Vec<String>,
    selected: bool,
}

/// The question and its rows between two lines of the form. Upstream `OmoQuestionView`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct OmoQuestionView {
    /// The question's lines; none when the pane cut them off.
    question: Vec<String>,
    /// The options in view: a pane too short for the form shows only the last ones.
    rows: Vec<OmoRow>,
    /// The row for a typed answer: whether it has the cursor, and where it is.
    own: Option<(bool, usize)>,
}

/// One answer row of the review, as it shows on screen.
#[derive(Debug, Clone, PartialEq, Eq)]
struct OmoShownRow {
    label: String,
    selected: bool,
}

// ---------------------------------------------------------------------------------------------
// The patterns, as the pin writes them.
// ---------------------------------------------------------------------------------------------

fn ansi_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\x1b\[[0-?]*[ -/]*[@-~]").expect("the pinned ansi pattern is valid"))
}

fn divider_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[\s╭╮╰╯├┤┬┴┼─━═╌▔]+$").expect("the pinned divider pattern is valid"))
}

fn ask_title_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^Ask user(?:\s+·.*)?$").expect("the pinned title pattern is valid"))
}

fn options_hint_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^↑↓ move\s+1-9 select\s+space (select|toggle)\s+enter (?:next|toggle)\b.*\besc cancel")
            .expect("the pinned options hint is a valid pattern")
    })
}

fn review_hint_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^enter (submit|edit answer)\s+↑.*\btab next question\s+esc back")
            .expect("the pinned review hint is a valid pattern")
    })
}

fn typing_hint_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^enter save and next\s+↑↓ back to options\b.*\besc discard")
            .expect("the pinned typing hint is a valid pattern")
    })
}

fn hint_end_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\besc (?:cancel|back|discard)$").expect("the pinned hint end is valid"))
}

fn not_footer_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[>❯›➜λ$%#]|[$%#>❯›λ]$").expect("the pinned footer exclusion is valid"))
}

fn pending_status_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^\?\s+(?:Question pending \(([0-9]+) unanswered\)|[0-9]+ questions pending)(?:\s+·.*)?$")
            .expect("the pinned pending status is a valid pattern")
    })
}

fn pending_hint_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^enter(?: or \S+)? to answer · \/answer · or just type your reply\b")
            .expect("the pinned pending hint is a valid pattern")
    })
}

fn empty_box_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[❯›>]$").expect("the pinned empty box is valid"))
}

fn solid_rule_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[─━]{8,}$").expect("the pinned solid rule is valid"))
}

fn form_marker_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\b1-9 select\b|enter save and next|\btab next question\b|\bor just type your reply\b")
            .expect("the pinned form marker is valid")
    })
}

fn submit_end_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:^|\s)(?:→\s)?Submit$").expect("the pinned submit tab is valid"))
}

fn label_split_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\s{2,}").expect("the pinned label split is valid"))
}

fn row_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^([0-9]+)\.\s+(.+)$").expect("the pinned option row is valid"))
}

fn row_prefix_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(?:→\s+)?[0-9]+\.\s+").expect("the pinned row prefix is valid"))
}

fn current_marker_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^→\s+").expect("the pinned cursor marker is valid"))
}

fn your_answer_label_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^Your answer \(").expect("the pinned typed answer label is valid"))
}

fn submit_count_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"Submit \(([0-9]+)/[0-9]+ answered\)").expect("the pinned answered count is valid")
    })
}

fn comment_label_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^Comment \(").expect("the pinned comment label is valid"))
}

fn own_answer_typed_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(?:→\s+)?Type your own answer\.\.\.:").expect("the pinned typed own answer is valid")
    })
}

fn ask_record_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#""role":"(?:assistant|toolResult|user)"|ask-user:settlement"#)
            .expect("the pinned ask record marker is valid")
    })
}

fn answer_frame_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^\[Answer to question ([^\]\r\n]+)\]\r?\n").expect("the pinned answer frame is valid")
    })
}

// ---------------------------------------------------------------------------------------------
// The ask state machine (server/omo-ask.ts).
// ---------------------------------------------------------------------------------------------

/// The tools whose calls are questions OmO asks. Upstream `OMO_ASK_TOOLS`.
pub fn omo_ask_tool(tool_name: &str) -> bool {
    matches!(tool_name, "ask_user_question" | "request_user_input")
}

/// Does a session line open or close a question? The rest of a session's tail is not parsed.
/// Upstream `OMO_ASK_RECORD_RE`.
pub fn omo_ask_record_interesting(line: &str) -> bool {
    ask_record_re().is_match(line)
}

/// Does the call wait for its answer? Upstream `waits`: only an explicit `false` says it does
/// not.
pub fn omo_ask_waits(args: &Value) -> bool {
    let record = args.as_object();
    match record.and_then(|record| record.get("waitForAnswer").or_else(|| record.get("wait_for_answer"))) {
        Some(value) => value.as_bool() != Some(false),
        None => true,
    }
}

/// One session record, applied to the open calls. Upstream `omoAsksAfter`.
pub fn omo_asks_after(mut open: Vec<OmoAskCall>, entry: &Value) -> Vec<OmoAskCall> {
    let entry_type = entry.get("type").and_then(Value::as_str);
    if entry_type == Some("custom") {
        let id = if entry.get("customType").and_then(Value::as_str) == Some("ask-user:settlement") {
            entry
                .get("data")
                .and_then(|data| data.get("requestId"))
                .and_then(Value::as_str)
                .map(str::to_string)
        } else {
            None
        };
        return match id {
            Some(id) => omo_asks_without(open, &id),
            None => open,
        };
    }
    let Some(message) = entry.get("message") else { return open };
    match message.get("role").and_then(Value::as_str) {
        Some("assistant") => {
            let calls: Vec<OmoAskCall> = message
                .get("content")
                .and_then(Value::as_array)
                .map(|parts| {
                    parts
                        .iter()
                        .filter_map(|part| {
                            if part.get("type").and_then(Value::as_str) != Some("toolCall") {
                                return None;
                            }
                            let id = part.get("id").and_then(Value::as_str)?;
                            if !omo_ask_tool(part.get("name").and_then(Value::as_str).unwrap_or_default()) {
                                return None;
                            }
                            if part.get("incomplete").and_then(Value::as_bool) == Some(true) {
                                return None;
                            }
                            let args = part.get("arguments").cloned().unwrap_or(Value::Null);
                            Some(OmoAskCall { id: id.to_string(), wait: omo_ask_waits(&args), args })
                        })
                        .collect()
                })
                .unwrap_or_default();
            // Assistant narration is not evidence of an answer, including across a resume.
            if calls.is_empty() {
                return open;
            }
            let ids: Vec<&str> = calls.iter().map(|call| call.id.as_str()).collect();
            let mut next: Vec<OmoAskCall> =
                open.into_iter().filter(|call| !ids.contains(&call.id.as_str())).collect();
            next.extend(calls);
            next
        }
        Some("toolResult") => {
            let Some(tool_call_id) = message.get("toolCallId").and_then(Value::as_str) else { return open };
            let details = message.get("details");
            let accepted = message.get("isError").and_then(Value::as_bool) != Some(true)
                && details.and_then(|details| details.get("accepted")).and_then(Value::as_bool) == Some(true)
                && details.and_then(|details| details.get("status")).and_then(Value::as_str) == Some("pending");
            if accepted {
                open
            } else {
                omo_asks_without(open, tool_call_id)
            }
        }
        Some("user") => {
            let texts: Vec<String> = match message.get("content") {
                Some(Value::String(text)) => vec![text.clone()],
                Some(Value::Array(parts)) => parts
                    .iter()
                    .filter_map(|part| {
                        if part.get("type").and_then(Value::as_str) != Some("text") {
                            return None;
                        }
                        part.get("text").and_then(Value::as_str).map(str::to_string)
                    })
                    .collect(),
                _ => Vec::new(),
            };
            texts.into_iter().fold(open, |rest, text| match answer_frame_id(&text) {
                Some(id) => omo_asks_without(rest, &id),
                None => rest,
            })
        }
        _ => open,
    }
}

fn omo_asks_without(open: Vec<OmoAskCall>, id: &str) -> Vec<OmoAskCall> {
    if open.iter().any(|call| call.id == id) {
        open.into_iter().filter(|call| call.id != id).collect()
    } else {
        open
    }
}

fn answer_frame_id(text: &str) -> Option<String> {
    answer_frame_re().captures(text)?.get(1).map(|id| id.as_str().to_string())
}

/// A call's questions, or `None` for arguments that are not the shape OmO asks with.
/// Upstream `omoAskOf`.
pub fn omo_ask_of(call: &OmoAskCall) -> Option<OmoAsk> {
    let questions = call.args.get("questions").and_then(Value::as_array)?;
    if questions.is_empty() {
        return None;
    }
    let mut parsed: Vec<OmoAskQuestion> = Vec::new();
    for question in questions {
        let header = question.get("header").and_then(Value::as_str)?;
        let text = question.get("question").and_then(Value::as_str)?;
        if let Some(options) = question.get("options") {
            if !options.is_array() {
                return None;
            }
        }
        let mut parsed_options: Vec<OmoAskOption> = Vec::new();
        let options = question.get("options").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
        for option in options {
            let label = option.get("label").and_then(Value::as_str).unwrap_or_default().to_string();
            if label.is_empty() {
                return None;
            }
            let description = option
                .get("description")
                .and_then(Value::as_str)
                .filter(|description| !description.is_empty())
                .map(str::to_string);
            parsed_options.push(OmoAskOption { label, description });
        }
        parsed.push(OmoAskQuestion {
            header: header.to_string(),
            question: text.to_string(),
            multi_select: question.get("multiSelect").and_then(Value::as_bool) == Some(true),
            options: parsed_options,
        });
    }
    Some(OmoAsk { id: call.id.clone(), wait: call.wait, questions: parsed })
}

/// The questions a session's bytes still have open, newest first. Upstream `openOmoAsks`.
pub fn open_omo_asks(jsonl: &str) -> Vec<OmoAsk> {
    let mut open: Vec<OmoAskCall> = Vec::new();
    for line in jsonl.split('\n') {
        if !omo_ask_record_interesting(line) {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<Value>(line) else { continue };
        if entry.is_object() {
            open = omo_asks_after(open, &entry);
        }
    }
    let mut asks: Vec<OmoAsk> = open.iter().filter_map(omo_ask_of).collect();
    asks.reverse();
    asks
}

/// The newest question a session has open, or `None`. Upstream `pendingOmoAsk`.
pub fn pending_omo_ask(jsonl: &str) -> Option<OmoAsk> {
    open_omo_asks(jsonl).into_iter().next()
}

/// Is a line of the screen worth reading the pane's session for? Upstream `OMO_FORM_RE`.
pub fn omo_form_on_screen(screen: &str) -> bool {
    form_marker_re().is_match(screen)
}

/// Is an omo form on this screen the form itself, or a pane herdr names something else?
///
/// A pane herdr names `omo`, `pi` or nothing at all reads an omo form on its own: the form's own
/// text is the evidence, and `omo_reads_forms` is what keeps every other agent out. A pane it
/// names `claude` reads one only while herdr reports it waiting on the user, because a claude pane
/// draws dialogs of its own. Upstream `readKnownPrompt:2476` also gates an unnamed pane on that
/// status; this lane reads it on the form's own text, which is the reader set the MANIFEST records.
pub fn omo_form_is_trusted(agent: &str, agent_status: Option<&str>) -> bool {
    agent != "claude" || agent_status == Some("blocked")
}

/// Does the reference read an omo form on a pane this agent names?
///
/// The omo arm of the chain runs for `omo`, for a pane with no agent, for `pi` (herdr names an
/// omo pane `pi` while it waits) and for `claude` (herdr names an omo pane `claude` while its
/// claude-sdk child runs). Any other agent never reaches this family. Upstream `parsePrompt`
/// `:1937-1951`.
pub fn omo_reads_forms(agent: &str) -> bool {
    matches!(agent, "omo" | "" | "pi" | "claude")
}

// ---------------------------------------------------------------------------------------------
// The screen's own text.
// ---------------------------------------------------------------------------------------------

/// The screen's lines, without the escape sequences. Upstream splits `/\r?\n/`.
fn screen_lines(screen: &str) -> Vec<String> {
    ansi_re()
        .replace_all(screen, "")
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_string())
        .collect()
}

/// A line without its escape sequences, its box drawing and its surrounding space.
fn clean_line(raw: &str) -> String {
    let stripped = ansi_re().replace_all(raw, "");
    let mut line = stripped.trim();
    if let Some(rest) = line.strip_prefix('│') {
        line = rest.trim_start();
    }
    if let Some(rest) = line.strip_suffix('│') {
        line = rest.trim_end();
    }
    line.trim().to_string()
}

/// Is the line one of the form's rules?
fn is_divider(line: &str) -> bool {
    let value = clean_line(line);
    !value.is_empty() && divider_re().is_match(&value)
}

/// Runs of whitespace as one space. Upstream `normalizeText`.
fn normalize_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn find_last_index<F>(lines: &[String], predicate: F) -> Option<usize>
where
    F: Fn(&str, usize) -> bool,
{
    (0..lines.len()).rev().find(|index| predicate(&lines[*index], *index))
}

/// A line and the lines after it, as one: a narrow pane wraps a hint line, so hints are matched
/// across the wrap. Upstream `wrapped`.
fn wrapped_lines(lines: &[String], index: usize, span: usize) -> String {
    lines
        .iter()
        .skip(index)
        .take(span)
        .map(|line| clean_line(line))
        .filter(|line| !line.is_empty() && !is_divider(line))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The line without the `→` that marks the form's cursor, when it has one.
fn strip_current_marker(line: &str) -> String {
    match line.strip_prefix('→') {
        Some(rest) if rest.starts_with(char::is_whitespace) => rest.trim_start().to_string(),
        _ => line.to_string(),
    }
}

/// Does the label end with the `✓` that marks an answered tab or a chosen row? Upstream `/\s✓$/`.
fn ends_with_answered(label: &str) -> bool {
    let trimmed = label.trim_end_matches('✓');
    trimmed.len() != label.len() && trimmed.ends_with(char::is_whitespace)
}

/// The label without its trailing answered mark.
fn strip_trailing_check(label: &str) -> String {
    match label.strip_suffix('✓') {
        Some(rest) if rest.ends_with(char::is_whitespace) => rest.trim_end().to_string(),
        _ => label.to_string(),
    }
}

/// The tab's own label: the cursor marker and the answered mark taken off.
fn tab_label(raw: &str) -> String {
    let without_current = match raw.strip_prefix('→') {
        Some(rest) if rest.starts_with(char::is_whitespace) => rest.trim_start(),
        _ => raw,
    };
    strip_trailing_check(without_current)
}

/// Does the row say its question is unanswered? Upstream `/:\s*unanswered$/`.
fn is_unanswered_row(label: &str) -> bool {
    match label.trim_end().strip_suffix("unanswered") {
        Some(rest) => rest.trim_end().ends_with(':'),
        None => false,
    }
}

/// Does the text start with the word, and not with a longer one?
fn starts_with_word(text: &str, word: &str) -> bool {
    match text.strip_prefix(word) {
        None => false,
        Some(rest) => rest.chars().next().map(|c| !(c.is_alphanumeric() || c == '_')).unwrap_or(true),
    }
}

/// The width one character takes on the pane, as the mirror measures it.
fn char_width(ch: char) -> usize {
    let value = ch as u32;
    if ch.is_control() {
        return 0;
    }
    if (0x0300..=0x036F).contains(&value)
        || (0x1AB0..=0x1AFF).contains(&value)
        || (0x1DC0..=0x1DFF).contains(&value)
        || (0x20D0..=0x20FF).contains(&value)
        || (0xFE20..=0xFE2F).contains(&value)
        || value == 0x200B
        || value == 0xFEFF
    {
        return 0;
    }
    if (0x1100..=0x115F).contains(&value)
        || (0x2E80..=0xA4CF).contains(&value)
        || (0xAC00..=0xD7A3).contains(&value)
        || (0xF900..=0xFAFF).contains(&value)
        || (0xFE10..=0xFE19).contains(&value)
        || (0xFE30..=0xFE6F).contains(&value)
        || (0xFF00..=0xFF60).contains(&value)
        || (0xFFE0..=0xFFE6).contains(&value)
        || (0x20000..=0x2FFFD).contains(&value)
        || (0x30000..=0x3FFFD).contains(&value)
    {
        2
    } else {
        1
    }
}

/// The display width of one text.
pub fn omo_string_width(text: &str) -> usize {
    text.chars().map(char_width).sum()
}

/// The pane's width as the screen shows it: OmO's rules and its footer span all of it.
fn screen_width(lines: &[String]) -> usize {
    lines.iter().map(|line| omo_string_width(line.trim_end())).max().unwrap_or(0)
}

/// The compatibility folds this lane compares with: fullwidth ASCII and the ideographic space.
/// Upstream normalizes with full NFKC (`comparable`, `prompt.ts:2523`); this bounded port covers
/// the range a pane's own text uses and leaves the rest alone.
fn compat_normalize(text: &str) -> String {
    text.chars()
        .map(|character| {
            let value = character as u32;
            if (0xFF01..=0xFF5E).contains(&value) {
                char::from_u32(value - 0xFEE0).unwrap_or(character)
            } else if value == 0x3000 {
                ' '
            } else {
                character
            }
        })
        .collect()
}

/// Letters and digits only, folded: a question the pane wraps or punctuates differently still
/// compares equal. Upstream `comparable`.
pub fn omo_comparable(text: &str) -> String {
    compat_normalize(text)
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(|character| character.to_lowercase())
        .collect()
}

/// Does the text on screen say what the call asked? A pane too narrow for a line may cut it with
/// an ellipsis. Upstream `sameText`.
fn same_text(shown: &str, asked: &str) -> bool {
    let left = omo_comparable(shown);
    let right = omo_comparable(asked);
    left == right
        || (shown.trim_end().ends_with('…') && left.chars().count() >= 24 && right.starts_with(&left))
}

/// A tab shows its question's header, cut with an ellipsis when the pane is too narrow for it.
/// Upstream `sameHeader`.
fn same_header(tab: &str, header: &str) -> bool {
    let shown = normalize_text(tab);
    let asked = normalize_text(header);
    shown == asked
        || (shown.ends_with('…') && asked.starts_with(shown.trim_end_matches('…').trim_end()))
}

/// The session's call is the form on screen: as many tabs, each its question's header.
fn ask_on_screen(form: &OmoForm, ask: &OmoAsk) -> bool {
    form.tabs.len() == ask.questions.len()
        && form
            .tabs
            .iter()
            .zip(ask.questions.iter())
            .all(|(tab, question)| same_header(&tab.label, &question.header))
}

/// The steps for the card, from the tabs on screen, named as asked when the call is known.
fn omo_steps(tabs: &[OmoTab], ask: Option<&OmoAsk>) -> Vec<ReferencePromptStep> {
    if tabs.len() <= 1 {
        return Vec::new();
    }
    tabs.iter()
        .enumerate()
        .map(|(index, tab)| ReferencePromptStep {
            label: ask
                .and_then(|ask| ask.questions.get(index))
                .map(|question| question.header.clone())
                .unwrap_or_else(|| tab.label.clone()),
            answered: tab.answered,
            current: tab.current,
        })
        .collect()
}

/// The card's title: where the question stands among several, else its header.
fn omo_title(tabs: &[OmoTab], index: usize, ask: Option<&OmoAsk>) -> String {
    if tabs.len() > 1 {
        return format!("Question {} of {}", index + 1, tabs.len());
    }
    ask.and_then(|ask| ask.questions.get(index))
        .map(|question| question.header.clone())
        .unwrap_or_else(|| tabs.get(index).map(|tab| tab.label.clone()).unwrap_or_default())
}

/// Keys from OmO's cursor to a row of its list. Without a cursor on screen (its row above the
/// visible part) they start from the top: `↑` stops at the first row, so `rows` of them get there.
fn omo_walk(to: usize, from: Option<usize>, rows: usize) -> Vec<&'static str> {
    match from {
        Some(from) => navigation_keys(to as i64 - from as i64),
        None => {
            let mut keys = navigation_keys(-(rows as i64));
            keys.extend(navigation_keys(to as i64));
            keys
        }
    }
}

fn navigation_keys(delta: i64) -> Vec<&'static str> {
    (0..delta.abs()).map(|_| if delta > 0 { "down" } else { "up" }).collect()
}

fn key_steps(keys: &[&str]) -> Vec<ReferenceKeyStep> {
    keys.iter().map(|key| ReferenceKeyStep::keys([*key])).collect()
}

/// Lines a narrow pane wrapped, joined back into one text (the first line's lead, a row's
/// number, cut off). A wrap at a space dropped it, so the lines join with one; but a line that
/// ends in a wide character filled to the pane's edge goes on without a space. Upstream
/// `joinWrapped`.
fn join_wrapped(raw: &[String], width: usize, lead: Option<&Regex>) -> String {
    let mut text = String::new();
    let mut previous = String::new();
    for line in raw {
        let cleaned = if text.is_empty() {
            match lead {
                Some(lead) => lead.replace(clean_line(line).as_str(), "").to_string(),
                None => clean_line(line),
            }
        } else {
            clean_line(line)
        };
        if cleaned.is_empty() {
            continue;
        }
        let first = cleaned.chars().next().unwrap_or(' ');
        let last_of_previous = previous.trim_end().chars().last().unwrap_or(' ');
        let glued = !text.is_empty()
            && char_width(last_of_previous) == 2
            && (char_width(first) == 2 || matches!(first, '.' | ',' | '!' | '?' | ';' | ':' | ')' | ']' | '}' | '…'))
            && (omo_string_width(previous.trim_end()) + char_width(first)) as i64 > width as i64 - 1;
        if text.is_empty() || glued {
            text.push_str(&cleaned);
        } else {
            text.push(' ');
            text.push_str(&cleaned);
        }
        previous = line.clone();
    }
    normalize_text(&text)
}

/// "Submit (1/2 answered)": how many of the form's questions have an answer.
fn omo_answered_count(lines: &[String], from: usize, to: usize) -> Option<usize> {
    let joined = lines
        .iter()
        .skip(from)
        .take(to.saturating_sub(from))
        .map(|line| clean_line(line))
        .collect::<Vec<_>>()
        .join(" ");
    submit_count_re().captures(&joined)?.get(1)?.as_str().parse().ok()
}

// ---------------------------------------------------------------------------------------------
// The form on screen.
// ---------------------------------------------------------------------------------------------

/// The head of OmO's form above its hint: the `Ask user` title, then the tab bar, each tab whole
/// on its line and Submit last. Upstream `omoForm`.
fn omo_form(lines: &[String], hint_index: usize) -> Option<OmoForm> {
    let title_index =
        find_last_index(&lines[..hint_index], |line, _| ask_title_re().is_match(&clean_line(line)))?;
    if hint_index - title_index > 120 {
        return None;
    }
    let mut bar_end = title_index + 1;
    while bar_end < hint_index && !submit_end_re().is_match(&clean_line(&lines[bar_end])) {
        bar_end += 1;
    }
    if bar_end >= hint_index || bar_end - title_index > 12 {
        return None;
    }
    let joined = lines[title_index + 1..=bar_end]
        .iter()
        .map(|line| clean_line(line))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("  ");
    let mut labels: Vec<String> = label_split_re().split(&joined).map(str::to_string).collect();
    let submit = labels.pop()?;
    let tabs: Vec<OmoTab> = labels
        .iter()
        .map(|label| OmoTab {
            label: tab_label(label),
            answered: ends_with_answered(label),
            current: label.starts_with('→'),
        })
        .collect();
    let reviewing = submit.starts_with('→');
    if tabs.is_empty() || tabs.iter().any(|tab| tab.label.is_empty()) {
        return None;
    }
    let current = tabs.iter().filter(|tab| tab.current).count();
    if current != if reviewing { 0 } else { 1 } {
        return None;
    }
    Some(OmoForm { bar_end, tabs, reviewing })
}

/// The question, its numbered options with descriptions indented under them, and the row for a
/// typed answer, between `start` and `end`. Upstream `omoQuestionView`.
fn omo_question_view(lines: &[String], start: usize, end: usize, cut: bool) -> OmoQuestionView {
    let mut view = OmoQuestionView { question: Vec::new(), rows: Vec::new(), own: None };
    let mut index = start;
    while index < end && view.own.is_none() {
        let raw = &lines[index];
        let line = clean_line(raw);
        if line.is_empty() || is_divider(&line) {
            index += 1;
            continue;
        }
        let indent = raw.chars().position(|character| !character.is_whitespace()).unwrap_or(0);
        let selected = line.starts_with('→');
        let text = strip_current_marker(&line);
        // The row's label, cut by a narrow pane: "Type your own" / "answer..."
        if (selected || indent >= 2) && starts_with_word(&text, "Type your own") {
            view.own = Some((selected, index));
            index += 1;
            continue;
        }
        let row = row_re().captures(&text).and_then(|captures| {
            let number = captures.get(1)?.as_str().parse::<usize>().ok()?;
            Some(number)
        });
        let next = match view.rows.last() {
            Some(last) => Some(last.number + 1),
            None => {
                if cut {
                    row
                } else {
                    Some(1)
                }
            }
        };
        if let Some(number) = row {
            if Some(number) == next && (selected || (indent >= 2 && indent < 5)) {
                view.rows.push(OmoRow {
                    number,
                    label: vec![raw.clone()],
                    description: Vec::new(),
                    selected,
                });
                index += 1;
                continue;
            }
        }
        match view.rows.last_mut() {
            None => {
                if !cut {
                    view.question.push(raw.clone());
                }
            }
            Some(last) => {
                if !last.description.is_empty() || indent >= 5 {
                    last.description.push(raw.clone());
                } else {
                    last.label.push(raw.clone());
                }
            }
        }
        index += 1;
    }
    view
}

/// The question of the session's call a cut-off form shows: the one whose options end with the
/// rows in view, each matched by its first line. Unique, or nothing.
fn asked_question(view: &OmoQuestionView, ask: &OmoAsk) -> Option<usize> {
    let last = view.rows.last()?.number;
    let mut matches: Vec<usize> = Vec::new();
    for (index, question) in ask.questions.iter().enumerate() {
        if question.options.len() != last {
            continue;
        }
        let every_row = view.rows.iter().all(|row| {
            let start = omo_comparable(&strip_row_prefix(&clean_line(&row.label[0])));
            question
                .options
                .get(row.number.saturating_sub(1))
                .map(|option| omo_comparable(&option.label).starts_with(&start))
                .unwrap_or(false)
        });
        if every_row {
            matches.push(index);
        }
    }
    if matches.len() == 1 {
        Some(matches[0])
    } else {
        None
    }
}

/// The row's own text without its number and its answered mark.
fn strip_row_prefix(label: &str) -> String {
    strip_trailing_check(&row_prefix_re().replace(label, ""))
}

/// Where the form stands when its tabs are out of view: the question asked now, answered ones
/// before it when the count says so, none after it. Upstream `cutSteps`.
fn cut_steps(ask: &OmoAsk, current: usize, current_answered: bool, answered_count: Option<usize>) -> Vec<OmoTab> {
    let in_order = answered_count
        .map(|count| count as i64 - i64::from(current_answered) == current as i64)
        .unwrap_or(false);
    ask.questions
        .iter()
        .enumerate()
        .map(|(index, question)| OmoTab {
            label: question.header.clone(),
            answered: if index == current { current_answered } else { in_order && index < current },
            current: index == current,
        })
        .collect()
}

/// The card the question's form makes. Upstream `parseOmoQuestion`.
fn parse_omo_question(lines: &[String], ask: Option<&OmoAsk>, trusted: bool) -> Option<OmoCard> {
    let hint_index = find_last_index(lines, |_, index| {
        options_hint_re().is_match(&wrapped_lines(lines, index, OMO_HINT_LINES))
    })?;
    let form = omo_form(lines, hint_index);
    if form.as_ref().map(|form| form.reviewing).unwrap_or(false) || (form.is_none() && ask.is_none()) {
        return None;
    }
    let known: Option<&OmoAsk> = match (ask, form.as_ref()) {
        (Some(ask), Some(form)) if !ask_on_screen(form, ask) => None,
        (ask, _) => ask,
    };
    if !trusted && known.is_none() {
        return None;
    }
    let view = omo_question_view(
        lines,
        form.as_ref().map(|form| form.bar_end + 1).unwrap_or(0),
        hint_index,
        form.is_none(),
    );
    let Some((own_selected, own_index)) = view.own else { return None };
    if view.rows.is_empty() && known.is_none() {
        return None;
    }
    let multi_select =
        options_hint_re().captures(&wrapped_lines(lines, hint_index, OMO_HINT_LINES))?.get(1)?.as_str() == "toggle";
    let width = screen_width(lines);
    let shown_labels: Vec<String> = view
        .rows
        .iter()
        // A row's label is joined with its number (and the cursor's marker) cut off the front,
        // as the reference's joinWrapped lead does: the card names the option, not its row.
        .map(|row| join_wrapped(&row.label, width, Some(row_prefix_re())))
        .collect();
    let own_line = clean_line(&lines[own_index]);
    let current_answered = shown_labels.iter().any(|label| ends_with_answered(label))
        || own_answer_typed_re().is_match(&own_line);
    let (index, tabs, question, options): (usize, Vec<OmoTab>, String, Vec<ReferencePromptOption>) =
        if let Some(ask) = known {
            let index = match form.as_ref() {
                Some(form) => form.tabs.iter().position(|tab| tab.current),
                None => asked_question(&view, ask),
            }?;
            let asked = ask.questions.get(index)?;
            // The screen must show this question's rows, numbered to its last option.
            if asked.multi_select != multi_select
                || view.rows.last().map(|row| row.number).unwrap_or(0) != asked.options.len()
            {
                return None;
            }
            if !view.question.is_empty()
                && !same_text(&join_wrapped(&view.question, width, None), &asked.question)
            {
                return None;
            }
            let every_row = view.rows.iter().enumerate().all(|(at, row)| {
                asked
                    .options
                    .get(row.number.saturating_sub(1))
                    .map(|option| {
                        same_text(&strip_trailing_check(&shown_labels[at]), &option.label)
                    })
                    .unwrap_or(false)
            });
            if !every_row {
                return None;
            }
            let tabs = match form.as_ref() {
                Some(form) => form.tabs.clone(),
                None => cut_steps(ask, index, current_answered, omo_answered_count(lines, own_index, hint_index)),
            };
            let options = asked
                .options
                .iter()
                .map(|option| ReferencePromptOption {
                    label: normalize_text(&option.label),
                    description: option.description.as_deref().map(normalize_text),
                })
                .collect();
            (index, tabs, normalize_text(&asked.question), options)
        } else {
            let form = form.as_ref()?;
            if view.question.is_empty() || view.rows.first().map(|row| row.number) != Some(1) {
                return None;
            }
            let index = form.tabs.iter().position(|tab| tab.current)?;
            let options = shown_labels
                .iter()
                .enumerate()
                .map(|(at, label)| ReferencePromptOption {
                    label: strip_trailing_check(label),
                    description: view
                        .rows
                        .get(at)
                        .filter(|row| !row.description.is_empty())
                        .map(|row| join_wrapped(&row.description, width, None)),
                })
                .collect();
            (index, form.tabs.clone(), join_wrapped(&view.question, width, None), options)
        };
    let highlighted = view.rows.iter().find(|row| row.selected);
    let selected_index = if own_selected { Some(options.len()) } else { highlighted.map(|row| row.number - 1) };
    let option_count = options.len();
    let rows = option_count + 1;
    let option_plans: Vec<Vec<ReferenceKeyStep>> = (0..option_count)
        .map(|option| {
            let mut keys = omo_walk(option, selected_index, rows);
            keys.push("enter");
            key_steps(&keys)
        })
        .collect();
    let custom_lead = {
        let mut steps = vec![ReferenceKeyStep::keys(["backspace"])];
        steps.extend(key_steps(&omo_walk(option_count, selected_index, rows)));
        steps.push(ReferenceKeyStep::keys(["enter"]));
        steps
    };
    Some(finish_omo_card(OmoCardParts {
        agent: "omo",
        kind: ReferencePromptKind::Question,
        title: omo_title(&tabs, index, known),
        question,
        body: None,
        options,
        multi_select,
        custom_option_index: if multi_select { None } else { Some(option_count as u32) },
        steps: omo_steps(&tabs, known),
        responder: OmoResponder::Question,
        call: known.map(|ask| ask.id.as_str()),
        option_plans,
        custom_plan: Some(OmoCustomPlan { lead: custom_lead, tail: key_steps(&["enter"]) }),
        multi_plan: Some(OmoMultiPlan { cursor: selected_index, rows }),
        reject_with_escape_index: None,
    }))
}

/// The card OmO's form makes while an answer is typed in the terminal. Upstream
/// `parseOmoTyping`.
fn parse_omo_typing(lines: &[String], ask: Option<&OmoAsk>, trusted: bool) -> Option<OmoCard> {
    let hint_index = find_last_index(lines, |_, index| {
        typing_hint_re().is_match(&wrapped_lines(lines, index, OMO_HINT_LINES))
    })?;
    let form = omo_form(lines, hint_index);
    if form.as_ref().map(|form| form.reviewing).unwrap_or(false) || (form.is_none() && ask.is_none()) {
        return None;
    }
    let known: Option<&OmoAsk> = match (ask, form.as_ref()) {
        (Some(ask), Some(form)) if !ask_on_screen(form, ask) => None,
        (ask, _) => ask,
    };
    if !trusted && known.is_none() {
        return None;
    }
    let label = find_last_index(&lines[..hint_index], |line, _| your_answer_label_re().is_match(&clean_line(line)))?;
    let field = find_last_index(&lines[..hint_index], |line, index| {
        index > label && clean_line(line).starts_with('>')
    })?;
    let typed = clean_line(&lines[field])
        .strip_prefix('>')
        .map(|rest| rest.strip_prefix(' ').unwrap_or(rest))
        .unwrap_or_default()
        .trim()
        .to_string();
    let view = omo_question_view(
        lines,
        form.as_ref().map(|form| form.bar_end + 1).unwrap_or(0),
        label,
        form.is_none(),
    );
    let width = screen_width(lines);
    let (index, tabs, question): (usize, Vec<OmoTab>, String) = if let Some(ask) = known {
        let index = match form.as_ref() {
            Some(form) => form.tabs.iter().position(|tab| tab.current),
            None => if view.rows.is_empty() { None } else { asked_question(&view, ask) },
        }?;
        let tabs = match form.as_ref() {
            Some(form) => form.tabs.clone(),
            None => cut_steps(ask, index, false, None),
        };
        let question = ask.questions.get(index).map(|question| normalize_text(&question.question))?;
        (index, tabs, question)
    } else {
        let form = form.as_ref()?;
        if view.question.is_empty() {
            return None;
        }
        let index = form.tabs.iter().position(|tab| tab.current)?;
        (index, form.tabs.clone(), join_wrapped(&view.question, width, None))
    };
    let choices: Vec<(String, Option<String>, Vec<ReferenceKeyStep>)> = vec![
        (
            "Save the typed answer".to_string(),
            if typed.is_empty() { None } else { Some(typed) },
            key_steps(&["enter"]),
        ),
        ("Discard it".to_string(), None, key_steps(&["esc"])),
    ];
    Some(finish_omo_card(OmoCardParts {
        agent: "omo",
        kind: ReferencePromptKind::Menu,
        title: omo_title(&tabs, index, known),
        question,
        body: None,
        options: choices
            .iter()
            .map(|(label, description, _)| ReferencePromptOption {
                label: label.clone(),
                description: description.clone(),
            })
            .collect(),
        multi_select: false,
        custom_option_index: None,
        steps: omo_steps(&tabs, known),
        responder: OmoResponder::Typing,
        call: known.map(|ask| ask.id.as_str()),
        option_plans: choices.iter().map(|(_, _, steps)| steps.clone()).collect(),
        custom_plan: None,
        multi_plan: None,
        reject_with_escape_index: None,
    }))
}

/// The card OmO's Submit tab makes. Upstream `parseOmoReview`.
fn parse_omo_review(lines: &[String], ask: Option<&OmoAsk>, trusted: bool) -> Option<OmoCard> {
    let hint_index = find_last_index(lines, |_, index| {
        review_hint_re().is_match(&wrapped_lines(lines, index, OMO_HINT_LINES))
    })?;
    let form = omo_form(lines, hint_index);
    if form.as_ref().map(|form| !form.reviewing).unwrap_or(false) || (form.is_none() && ask.is_none()) {
        return None;
    }
    let known: Option<&OmoAsk> = match (ask, form.as_ref()) {
        (Some(ask), Some(form)) if !ask_on_screen(form, ask) => None,
        (ask, _) => ask,
    };
    if !trusted && known.is_none() {
        return None;
    }
    let bar_end = form.as_ref().map(|form| form.bar_end as i64).unwrap_or(-1);
    let heading = find_last_index(&lines[..hint_index], |line, index| {
        index as i64 > bar_end && clean_line(line) == "Review your answers"
    });
    if heading.is_none() && form.is_some() {
        return None;
    }
    let mut rows: Vec<(Vec<String>, bool)> = Vec::new();
    let mut index = heading.map(|heading| heading + 1).unwrap_or(0);
    while index < hint_index {
        let raw = &lines[index];
        let line = clean_line(raw);
        // The comment field's label, or with that cut off the field itself.
        if comment_label_re().is_match(&line) || line.starts_with('>') {
            break;
        }
        if line.is_empty() {
            if heading.is_some() || !rows.is_empty() {
                break;
            }
            index += 1;
            continue;
        }
        let selected = line.starts_with('→');
        let indent = raw.chars().position(|character| !character.is_whitespace()).unwrap_or(0);
        if selected || indent >= 2 {
            rows.push((vec![raw.clone()], selected));
        } else if let Some(last) = rows.last_mut() {
            last.0.push(raw.clone());
        }
        index += 1;
    }
    let width = screen_width(lines);
    let shown: Vec<OmoShownRow> = rows
        .iter()
        .map(|(text, selected)| OmoShownRow {
            label: join_wrapped(text, width, Some(current_marker_re())),
            selected: *selected,
        })
        .collect();
    let count = known.map(|ask| ask.questions.len()).unwrap_or_else(|| form.as_ref().map(|form| form.tabs.len()).unwrap_or(0));
    let row_of: Vec<Option<usize>> = match known {
        Some(ask) => ask
            .questions
            .iter()
            .map(|question| {
                let prefix = format!("{}:", normalize_text(&question.header));
                shown.iter().position(|row| row.label.starts_with(&prefix))
            })
            .collect(),
        None => {
            if shown.len() == count {
                (0..shown.len()).map(Some).collect()
            } else {
                return None;
            }
        }
    };
    let labels: Vec<String> = row_of
        .iter()
        .enumerate()
        .map(|(at, row)| match row {
            Some(index) => shown[*index].label.clone(),
            None => known
                .and_then(|ask| ask.questions.get(at))
                .map(|question| normalize_text(&question.header))
                .unwrap_or_default(),
        })
        .collect();
    let rest: Vec<String> = lines[index..hint_index]
        .iter()
        .filter(|line| !clean_line(line).is_empty() && !is_divider(line))
        .cloned()
        .collect();
    let field = rest.iter().position(|line| clean_line(line).starts_with('>'))?;
    let comment_label = {
        let joined = join_wrapped(&rest[..field], width, None);
        if joined.is_empty() { "Comment".to_string() } else { joined }
    };
    let comment = clean_line(&rest[field])
        .strip_prefix('>')
        .map(|rest| rest.strip_prefix(' ').unwrap_or(rest))
        .unwrap_or_default()
        .trim()
        .to_string();
    let after = join_wrapped(&rest[field + 1..], width, None);
    let submit_at = submit_count_re().find(&after).map(|found| found.start());
    let notice = match submit_at {
        Some(at) => after[..at].to_string(),
        None => after.clone(),
    };
    let notice = notice.strip_prefix('!').unwrap_or(notice.as_str()).trim().to_string();
    let answered_count = omo_answered_count(lines, index, hint_index);
    let on_comment = review_hint_re()
        .captures(&wrapped_lines(lines, hint_index, OMO_HINT_LINES))?
        .get(1)?
        .as_str()
        == "submit";
    let selected_index = if on_comment {
        Some(count)
    } else {
        row_of.iter().position(|row| row.map(|index| shown[index].selected).unwrap_or(false))
    };
    let tabs: Vec<OmoTab> = match form.as_ref() {
        Some(form) => form.tabs.clone(),
        None => known
            .map(|ask| {
                ask.questions
                    .iter()
                    .enumerate()
                    .map(|(at, question)| OmoTab {
                        label: question.header.clone(),
                        answered: match row_of.get(at).and_then(|row| *row) {
                            Some(index) => !is_unanswered_row(&shown[index].label),
                            None => answered_count == Some(count),
                        },
                        current: false,
                    })
                    .collect()
            })
            .unwrap_or_default(),
    };
    let submit_plan = {
        let mut keys = omo_walk(count, selected_index, count + 1);
        keys.push("enter");
        key_steps(&keys)
    };
    let row_plans: Vec<Vec<ReferenceKeyStep>> = labels
        .iter()
        .enumerate()
        .map(|(row, _)| {
            let mut keys = omo_walk(row, selected_index, count + 1);
            keys.push("enter");
            key_steps(&keys)
        })
        .collect();
    let mut options: Vec<ReferencePromptOption> =
        std::iter::once(ReferencePromptOption { label: "Submit".to_string(), description: None })
            .chain(labels.iter().map(|label| ReferencePromptOption { label: label.clone(), description: None }))
            .collect();
    options.push(ReferencePromptOption { label: comment_label.clone(), description: None });
    let mut option_plans: Vec<Vec<ReferenceKeyStep>> = std::iter::once(submit_plan).chain(row_plans).collect();
    option_plans.push(Vec::new());
    let question = match submit_at {
        Some(at) => after[at..].to_string(),
        None => "Submit your answers?".to_string(),
    };
    let body = {
        let parts: Vec<String> = [notice, if comment.is_empty() { String::new() } else { format!("Comment: {comment}") }]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect();
        if parts.is_empty() { None } else { Some(parts.join("\n")) }
    };
    Some(finish_omo_card(OmoCardParts {
        agent: "omo",
        kind: ReferencePromptKind::Menu,
        title: "Review your answers".to_string(),
        question,
        body,
        options,
        multi_select: false,
        custom_option_index: Some(labels.len() as u32 + 1),
        steps: omo_steps(&tabs, known),
        responder: OmoResponder::Review,
        call: known.map(|ask| ask.id.as_str()),
        option_plans,
        custom_plan: Some(OmoCustomPlan {
            lead: key_steps(&omo_walk(count, selected_index, count + 1)),
            tail: key_steps(&["enter"]),
        }),
        multi_plan: None,
        reject_with_escape_index: None,
    }))
}

/// The card OmO's folded widget makes, over its input box, for a question asked without waiting.
/// Upstream `parseOmoPending`.
fn parse_omo_pending(lines: &[String], pending: &[OmoAsk]) -> Option<OmoCard> {
    let hint_index =
        find_last_index(lines, |_, index| pending_hint_re().is_match(&wrapped_lines(lines, index, 3)))?;
    let status_index = find_last_index(&lines[..hint_index], |line, _| pending_status_re().is_match(&clean_line(line)))?;
    if hint_index - status_index > 12 {
        return None;
    }
    let status_line = clean_line(&lines[status_index]);
    let status = pending_status_re().captures(&status_line)?;
    let unanswered = status.get(1).map(|value| value.as_str().to_string());
    let shown = lines.get(status_index + 1).map(String::as_str).map(clean_line).unwrap_or_default();
    let (header, question_text) = split_em_dash(&shown)?;
    // The line is cut at the pane's edge.
    let start = omo_comparable(question_text.trim_end_matches('…').trim_end_matches("..."));
    let mut matches: Vec<(&OmoAsk, usize)> = Vec::new();
    for ask in pending.iter().filter(|ask| !ask.wait) {
        for (index, question) in ask.questions.iter().enumerate() {
            if same_header(&header, &question.header)
                && omo_comparable(&question.question).starts_with(&start)
            {
                matches.push((ask, index));
            }
        }
    }
    if matches.len() != 1 {
        return None;
    }
    let (ask, index) = matches[0];
    let asked = ask.questions.get(index)?;
    let answered = unanswered
        .and_then(|unanswered| unanswered.parse::<usize>().ok())
        .map(|unanswered| ask.questions.len().saturating_sub(unanswered));
    let tabs = cut_steps(ask, index, false, answered);
    let options: Vec<ReferencePromptOption> = asked
        .options
        .iter()
        .map(|option| ReferencePromptOption {
            label: normalize_text(&option.label),
            description: option.description.as_deref().map(normalize_text),
        })
        .collect();
    let option_count = options.len();
    Some(finish_omo_card(OmoCardParts {
        agent: "omo",
        kind: ReferencePromptKind::Question,
        title: omo_title(&tabs, index, Some(ask)),
        question: normalize_text(&asked.question),
        body: None,
        options,
        multi_select: asked.multi_select,
        // The widget's own-answer row ("Type your reply") sits one past the options, where the
        // reference's menu labels put it; the form it opens shows OmO's own row instead.
        custom_option_index: if asked.multi_select { None } else { Some(option_count as u32 + 1) },
        steps: omo_steps(&tabs, Some(ask)),
        responder: OmoResponder::Pending,
        call: Some(ask.id.as_str()),
        // Validation only: execution requires the opened form and its freshly read cursor.
        option_plans: vec![Vec::new(); option_count],
        custom_plan: Some(OmoCustomPlan { lead: Vec::new(), tail: Vec::new() }),
        multi_plan: Some(OmoMultiPlan { cursor: None, rows: option_count }),
        reject_with_escape_index: None,
    }))
}

/// The two halves of the widget's line: the header, then the question, cut at the pane's edge.
fn split_em_dash(line: &str) -> Option<(String, String)> {
    let (header, rest) = line.split_once(" — ")?;
    Some((header.to_string(), rest.to_string()))
}

// ---------------------------------------------------------------------------------------------
// The card.
// ---------------------------------------------------------------------------------------------

/// The parts one parser hands the card builder.
struct OmoCardParts<'a> {
    agent: &'a str,
    kind: ReferencePromptKind,
    title: String,
    question: String,
    body: Option<String>,
    options: Vec<ReferencePromptOption>,
    multi_select: bool,
    custom_option_index: Option<u32>,
    steps: Vec<ReferencePromptStep>,
    responder: OmoResponder,
    call: Option<&'a str>,
    option_plans: Vec<Vec<ReferenceKeyStep>>,
    custom_plan: Option<OmoCustomPlan>,
    multi_plan: Option<OmoMultiPlan>,
    reject_with_escape_index: Option<usize>,
}

/// The card's id: what the prompt says, and which asking of it this is. An answer names it, and
/// one whose prompt changed between the read and the click is refused instead of misfired.
/// Upstream `finishPrompt` hashes the card's own fields.
fn omo_prompt_id(parts: &OmoCardParts<'_>, body: Option<&str>) -> String {
    let mut root = Map::new();
    root.insert("agent".to_string(), json!(parts.agent));
    root.insert("kind".to_string(), json!(parts.kind));
    root.insert("title".to_string(), json!(parts.title));
    root.insert("question".to_string(), json!(parts.question));
    root.insert("body".to_string(), json!(body));
    root.insert("options".to_string(), json!(parts.options));
    root.insert("multiSelect".to_string(), json!(parts.multi_select));
    root.insert("customOptionIndex".to_string(), json!(parts.custom_option_index));
    if !parts.steps.is_empty() {
        root.insert("steps".to_string(), json!(parts.steps));
    }
    if let Some(call) = parts.call {
        root.insert("call".to_string(), json!(call));
    }
    let digest = Sha256::digest(Value::Object(root).to_string().as_bytes());
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    hex.chars().take(12).collect()
}

fn finish_omo_card(parts: OmoCardParts<'_>) -> OmoCard {
    let id = omo_prompt_id(&parts, parts.body.as_deref());
    let body = parts.body.map(|body| body.chars().take(OMO_PROMPT_BODY_MAX_CHARS).collect());
    let prompt = ReferencePrompt {
        id,
        agent: parts.agent.to_string(),
        kind: parts.kind,
        title: parts.title,
        question: parts.question,
        body,
        options: parts.options,
        multi_select: parts.multi_select,
        custom_option_index: parts.custom_option_index,
        queued: None,
        steps: parts.steps,
        fallback: None,
    };
    register_omo_card(OmoCard {
        prompt,
        responder: parts.responder,
        option_plans: parts.option_plans,
        custom_plan: parts.custom_plan,
        multi_plan: parts.multi_plan,
        reject_with_escape_index: parts.reject_with_escape_index,
    })
}

fn omo_prompt_registry() -> &'static Mutex<OmoPromptRegistry> {
    static REGISTRY: OnceLock<Mutex<OmoPromptRegistry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(OmoPromptRegistry { order: VecDeque::new(), cards: HashMap::new() }))
}

struct OmoPromptRegistry {
    order: VecDeque<String>,
    cards: HashMap<String, OmoCard>,
}

fn register_omo_card(card: OmoCard) -> OmoCard {
    if let Ok(mut registry) = omo_prompt_registry().lock() {
        let id = card.prompt.id.clone();
        if !registry.cards.contains_key(&id) {
            if registry.order.len() >= OMO_PROMPT_CACHE_MAX {
                if let Some(oldest) = registry.order.pop_front() {
                    registry.cards.remove(&oldest);
                }
            }
            registry.order.push_back(id.clone());
        }
        registry.cards.insert(id, card.clone());
    }
    card
}

/// The card a detected prompt came from, for a caller that has the prompt in hand.
pub fn omo_card_for_prompt(prompt: &ReferencePrompt) -> Option<OmoCard> {
    omo_prompt_registry().lock().ok()?.cards.get(&prompt.id).cloned()
}

// ---------------------------------------------------------------------------------------------
// The chain, the tail gate and the answer plan.
// ---------------------------------------------------------------------------------------------

/// The card a screen makes, from the session's open calls and the screen alone.
///
/// `ask` is the newest open call (`pending_omo_ask`) and `open` is every open call
/// (`open_omo_asks`), exactly as the reference's route reads them. Upstream `parsePrompt`'s
/// omo arm and its tail gate.
pub fn omo_card_for_screen(
    agent: &str,
    screen: &str,
    ask: Option<&OmoAsk>,
    trusted: bool,
    open: &[OmoAsk],
) -> Option<OmoCard> {
    if !omo_reads_forms(agent) {
        return None;
    }
    let lines = screen_lines(screen);
    let mut candidates: Vec<OmoCard> = Vec::new();
    if open.is_empty() {
        candidates.extend(parse_omo_question(&lines, ask, trusted));
        candidates.extend(parse_omo_typing(&lines, ask, trusted));
        candidates.extend(parse_omo_review(&lines, ask, trusted));
    } else {
        // With open calls, only the call the screen matches answers it, and only when exactly one
        // of them does.
        let matched: Vec<OmoCard> = open
            .iter()
            .flat_map(|ask| {
                [
                    parse_omo_question(&lines, Some(ask), false),
                    parse_omo_typing(&lines, Some(ask), false),
                    parse_omo_review(&lines, Some(ask), false),
                ]
                .into_iter()
                .flatten()
            })
            .collect();
        // No card from the open calls leaves the widget below in the chain: the form the folded
        // widget opens is still being drawn under its hint while the widget itself is on screen.
        // Several matches mean the screen says nothing about which call it answers.
        if matched.len() <= 1 {
            candidates.extend(matched);
        }
    }
    candidates.extend(parse_omo_pending(&lines, open));
    candidates.into_iter().find(|card| prompt_tail_is_active(card.responder, screen))
}

/// The public prompt a screen makes, for a caller that only wants the card.
pub fn omo_prompt_for_screen(
    agent: &str,
    screen: &str,
    ask: Option<&OmoAsk>,
    trusted: bool,
    open: &[OmoAsk],
) -> Option<ReferencePrompt> {
    omo_card_for_screen(agent, screen, ask, trusted, open).map(|card| card.prompt)
}

/// The screen-only detector: a whole form on the screen, with no session read.
///
/// A pane the reference names `claude` is omo's only while it is blocked, which a screen alone
/// cannot say: the route passes the pane's status through `omo_form_is_trusted` and calls
/// `omo_prompt_for_screen` for that case.
pub fn detect_reference_omo_prompt(agent: &str, screen: &str) -> Option<ReferencePrompt> {
    omo_prompt_for_screen(agent, screen, None, omo_form_is_trusted(agent, None), &[])
}

/// The lane's detector in the frozen alias's shape.
pub fn omo_prompt_detector() -> ReferencePromptDetector {
    detect_reference_omo_prompt
}

/// The lane's answer planner in the frozen alias's shape.
pub fn omo_answer_planner() -> ReferenceAnswerPlanner {
    omo_answer_planner_fn
}

fn omo_answer_planner_fn(prompt: &ReferencePrompt, answer: &ReferencePromptAnswer) -> Result<Vec<ReferenceKeyStep>, String> {
    reference_omo_answer_keys(prompt, answer).map_err(omo_answer_error_message)
}

/// The reference answers an unusable answer with `badRequest("invalid_answer", message)`; this
/// lane keeps the typed code and hands a message-shaped caller its name.
fn omo_answer_error_message(code: ScopeErrorCode) -> String {
    match code {
        ScopeErrorCode::InvalidRequest => "invalid_answer".to_string(),
        other => format!("{other:?}"),
    }
}

/// Is the card the screen shows still the card that waits for an answer?
///
/// An omo form is live only with nothing but OmO's own footer under its hint: blank lines, one
/// rule, then the footer's few lines. Anything else is the form's text in another program
/// (printed in a shell, quoted in a transcript over an input box), where an answer's keys would
/// be typed into that program. The widget is live over the input box the same way. Upstream
/// `promptTailIsActive`'s omo arms.
fn prompt_tail_is_active(responder: OmoResponder, screen: &str) -> bool {
    let clean_lines: Vec<String> = screen_lines(screen).iter().map(String::as_str).map(clean_line).collect();
    let form_hint = match responder {
        OmoResponder::Question => Some(options_hint_re()),
        OmoResponder::Review => Some(review_hint_re()),
        OmoResponder::Typing => Some(typing_hint_re()),
        OmoResponder::Pending => None,
    };
    if let Some(hint) = form_hint {
        let Some(at) = find_last_index(&clean_lines, |line, index| {
            !line.is_empty() && hint.is_match(&wrapped_lines(&clean_lines, index, OMO_HINT_LINES))
        }) else {
            return false;
        };
        let mut end = at;
        while end < at + OMO_HINT_LINES && !hint_end_re().is_match(clean_lines[at..=end].join(" ").trim()) {
            end += 1;
        }
        if end >= at + OMO_HINT_LINES {
            return false;
        }
        return footer_is_omo(&clean_lines[end + 1..]);
    }
    let Some(at) = find_last_index(&clean_lines, |line, index| {
        !line.is_empty() && pending_hint_re().is_match(&wrapped_lines(&clean_lines, index, 3))
    }) else {
        return false;
    };
    let Some(box_index) = clean_lines
        .iter()
        .enumerate()
        .find(|(index, line)| *index > at && empty_box_re().is_match(line))
        .map(|(index, _)| index)
    else {
        return false;
    };
    if box_index - at > 60 || !clean_lines[box_index - 1].starts_with('─') {
        return false;
    }
    footer_is_omo(&clean_lines[box_index + 1..])
}

/// Nothing but OmO's own footer: blank lines, one rule, then a few short footer lines.
fn footer_is_omo(lines: &[String]) -> bool {
    let mut rules = 0usize;
    let mut footer = 0usize;
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if solid_rule_re().is_match(line) {
            if rules > 0 || footer > 0 {
                return false;
            }
            rules = 1;
            continue;
        }
        if rules == 0 {
            return false;
        }
        footer += 1;
        if footer > OMO_FOOTER_LINES
            || not_footer_re().is_match(line)
            || line.chars().count() > OMO_FOOTER_MAX_CHARS
        {
            return false;
        }
    }
    rules == 1
}

/// The keys that answer the prompt on the original pane. Upstream `answerKeys`.
pub fn reference_omo_answer_keys(
    prompt: &ReferencePrompt,
    answer: &ReferencePromptAnswer,
) -> Result<Vec<ReferenceKeyStep>, ScopeErrorCode> {
    let card = omo_card_for_prompt(prompt).ok_or(ScopeErrorCode::InvalidRequest)?;
    omo_answer_plan(&card, answer)
}

/// The keys that answer one card. Upstream `answerKeys`.
pub fn omo_answer_plan(card: &OmoCard, answer: &ReferencePromptAnswer) -> Result<Vec<ReferenceKeyStep>, ScopeErrorCode> {
    let supplied = [
        answer.option_index.is_some(),
        answer.option_indices.is_some(),
        answer.custom_text.is_some(),
    ]
    .into_iter()
    .filter(|supplied| *supplied)
    .count();
    if supplied != 1 {
        return Err(ScopeErrorCode::InvalidRequest);
    }
    let prompt = &card.prompt;
    if let Some(text) = answer.custom_text.as_deref() {
        let text = text.trim();
        if text.is_empty() || prompt.custom_option_index.is_none() || prompt.multi_select {
            return Err(ScopeErrorCode::InvalidRequest);
        }
        let plan = card.custom_plan.as_ref().ok_or(ScopeErrorCode::InvalidRequest)?;
        // The folded widget's plans are empty on purpose: the route opens the form with its own
        // key and plans from the opened form, so an answer here sends no keys at all. Upstream
        // `customSteps: () => []` for `omo-pending` (`prompt.ts:2674-2702`).
        if card.responder == OmoResponder::Pending {
            return Ok(Vec::new());
        }
        let mut steps = plan.lead.clone();
        steps.push(ReferenceKeyStep::typed(text));
        steps.extend(plan.tail.iter().cloned());
        return Ok(steps);
    }
    if let Some(indices) = answer.option_indices.as_deref() {
        if !prompt.multi_select || indices.is_empty() {
            return Err(ScopeErrorCode::InvalidRequest);
        }
        let mut choices: Vec<u32> = Vec::new();
        for index in indices {
            if !choices.contains(index) {
                choices.push(*index);
            }
        }
        if choices.iter().any(|choice| *choice as usize >= prompt.options.len()) {
            return Err(ScopeErrorCode::InvalidRequest);
        }
        let multi = card.multi_plan.as_ref().ok_or(ScopeErrorCode::InvalidRequest)?;
        // Validation only, as above: the folded widget's `multiSteps` is `() => []`.
        if card.responder == OmoResponder::Pending {
            return Ok(Vec::new());
        }
        let mut keys: Vec<&'static str> = vec!["backspace"];
        let mut at = multi.cursor;
        let mut sorted = choices.clone();
        sorted.sort_unstable();
        for option in sorted {
            keys.extend(omo_walk(option as usize, at, multi.rows));
            keys.push("space");
            at = Some(option as usize);
        }
        keys.push("tab");
        return Ok(key_steps(&keys));
    }
    let index = answer.option_index.ok_or(ScopeErrorCode::InvalidRequest)?;
    let index = index as usize;
    if index >= prompt.options.len()
        || prompt.custom_option_index == Some(index as u32)
        || prompt.multi_select
    {
        return Err(ScopeErrorCode::InvalidRequest);
    }
    if card.reject_with_escape_index == Some(index) {
        return Ok(key_steps(&["esc"]));
    }
    Ok(card.option_plans.get(index).cloned().unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FORM_TABBED: &str = include_str!("fixtures/prompt-omo/form-tabbed.screen");
    const FORM_CUT: &str = include_str!("fixtures/prompt-omo/form-cut.screen");
    const FORM_MULTI: &str = include_str!("fixtures/prompt-omo/form-multi.screen");
    const FORM_TYPING: &str = include_str!("fixtures/prompt-omo/form-typing.screen");
    const FORM_REVIEW: &str = include_str!("fixtures/prompt-omo/form-review.screen");
    const FORM_WIDGET_OPENED: &str = include_str!("fixtures/prompt-omo/form-widget-opened.screen");
    const WIDGET_PENDING: &str = include_str!("fixtures/prompt-omo/widget-pending.screen");
    const FORM_STALE: &str = include_str!("fixtures/prompt-omo/form-stale.screen");
    const FORM_SHELL_QUOTED: &str = include_str!("fixtures/prompt-omo/form-shell-quoted.screen");
    const FORM_NO_RULE: &str = include_str!("fixtures/prompt-omo/form-no-rule.screen");
    const FORM_TWO_RULES: &str = include_str!("fixtures/prompt-omo/form-two-rules.screen");
    const FORM_BOX_UNDER: &str = include_str!("fixtures/prompt-omo/form-box-under.screen");
    const FORM_LONG_FOOTER: &str = include_str!("fixtures/prompt-omo/form-long-footer.screen");
    const WIDGET_NO_BOX: &str = include_str!("fixtures/prompt-omo/widget-no-box.screen");
    const WIDGET_FAR_BOX: &str = include_str!("fixtures/prompt-omo/widget-far-box.screen");
    const FORM_AMBIGUOUS: &str = include_str!("fixtures/prompt-omo/form-ambiguous.screen");
    const ASKS_TWO_QUESTIONS: &str = include_str!("fixtures/prompt-omo/asks-two-questions.jsonl");
    const ASKS_CUT_THREE: &str = include_str!("fixtures/prompt-omo/asks-cut-three-options.jsonl");
    const ASKS_MULTI: &str = include_str!("fixtures/prompt-omo/asks-multi.jsonl");
    const ASKS_FOLDED: &str = include_str!("fixtures/prompt-omo/asks-folded.jsonl");
    const ASKS_AMBIGUOUS: &str = include_str!("fixtures/prompt-omo/asks-ambiguous.jsonl");
    const ASKS_NONE: &str = include_str!("fixtures/prompt-omo/asks-none.jsonl");
    const ASKS_LIFECYCLE: &str = include_str!("fixtures/prompt-omo/asks-lifecycle.jsonl");

    /// The route's own shape: the newest open call and every open call, as the reference passes
    /// them to the chain.
    fn card_for(agent: &str, screen: &str, session: &str) -> Option<OmoCard> {
        let ask = pending_omo_ask(session);
        let open = open_omo_asks(session);
        omo_card_for_screen(agent, screen, ask.as_ref(), omo_form_is_trusted(agent, None), &open)
    }

    fn option(label: &str, description: Option<&str>) -> ReferencePromptOption {
        ReferencePromptOption { label: label.to_string(), description: description.map(str::to_string) }
    }

    fn step(label: &str, answered: bool, current: bool) -> ReferencePromptStep {
        ReferencePromptStep { label: label.to_string(), answered, current }
    }

    fn option_answer(index: u32) -> ReferencePromptAnswer {
        ReferencePromptAnswer { option_index: Some(index), ..Default::default() }
    }

    #[test]
    fn the_tabbed_form_reads_its_question_options_and_steps() {
        let card = card_for("omo", FORM_TABBED, ASKS_TWO_QUESTIONS).expect("the whole form is a card");
        assert_eq!(card.responder, OmoResponder::Question);
        assert_eq!(card.responder.as_str(), "omo-question");
        let prompt = &card.prompt;
        assert_eq!(prompt.agent, "omo");
        assert_eq!(prompt.kind, ReferencePromptKind::Question);
        assert_eq!(prompt.title, "Question 2 of 2", "the tab bar says where the question stands");
        assert_eq!(prompt.question, "월 한도를 얼마로 할까요?");
        assert_eq!(prompt.body, None);
        assert_eq!(
            prompt.options,
            vec![
                option("월 $5 한도", Some("추정 비용이 넘으면 알려줍니다")),
                option("월 $20 한도", None),
            ]
        );
        assert!(!prompt.multi_select);
        assert_eq!(prompt.custom_option_index, Some(2), "the own-answer row is the last one");
        assert_eq!(
            prompt.steps,
            vec![step("표시 위치", true, false), step("월 한도", false, true)],
            "the card names the questions the call asked, in order"
        );
        assert_eq!(prompt.id.len(), 12);
        assert_eq!(prompt.fallback, None);
        assert_eq!(prompt.queued, None);
    }

    #[test]
    fn the_same_question_parses_from_the_screen_alone() {
        let card = card_for("omo", FORM_TABBED, ASKS_NONE).expect("a whole form reads without the session");
        assert_eq!(card.responder, OmoResponder::Question);
        assert_eq!(card.prompt.title, "Question 2 of 2");
        assert_eq!(card.prompt.question, "월 한도를 얼마로 할까요?");
        assert_eq!(card.prompt.options[0].label, "월 $5 한도");
        assert_eq!(card.prompt.options[0].description.as_deref(), Some("추정 비용이 넘으면 알려줍니다"));
        assert_eq!(card.prompt.custom_option_index, Some(2));
        // The call's own text is what names a session-backed card, so the two ids differ.
        let with_session = card_for("omo", FORM_TABBED, ASKS_TWO_QUESTIONS).expect("the form is a card");
        assert_ne!(card.prompt.id, with_session.prompt.id);
    }

    #[test]
    fn an_untrusted_form_without_the_session_call_is_not_a_card() {
        let lines = screen_lines(FORM_TABBED);
        assert!(parse_omo_question(&lines, None, false).is_none(), "the reference trusts a form it cannot name");
        assert!(parse_omo_question(&lines, None, true).is_some());
        // A pane herdr names claude is omo's only while it is blocked; the route passes the status.
        assert!(omo_card_for_screen("claude", FORM_TABBED, None, false, &[]).is_none());
        assert!(omo_card_for_screen("claude", FORM_TABBED, None, true, &[]).is_some());
    }

    #[test]
    fn a_form_whose_rows_are_not_the_call_s_question_is_refused() {
        // The call asks its second question with three options; the form shows two.
        let three_options = ASKS_CUT_THREE.replace("월 $100 한도", "월 $20 한도");
        let ask = pending_omo_ask(&three_options).expect("the call is recorded");
        assert_eq!(ask.questions[1].options.len(), 3);
        let lines = screen_lines(FORM_TABBED);
        assert!(parse_omo_question(&lines, Some(&ask), false).is_none());
        // A call whose tabs are not the form's is not the form on screen.
        let other_headers = ASKS_CUT_THREE.replace("표시 위치", "다른 위치");
        let ask = pending_omo_ask(&other_headers).expect("the call is recorded");
        assert!(parse_omo_question(&lines, Some(&ask), false).is_none());
    }

    #[test]
    fn the_cut_form_reads_the_question_the_session_call_names() {
        let card = card_for("omo", FORM_CUT, ASKS_CUT_THREE).expect("the cut form is the call's question");
        assert_eq!(card.responder, OmoResponder::Question);
        assert_eq!(card.prompt.title, "Question 1 of 2");
        assert_eq!(card.prompt.question, "음성 사용량과 추정 비용을 어디에 보여줄까요?");
        assert_eq!(card.prompt.options.len(), 2);
        assert_eq!(card.prompt.options[0].label, "설정 > 음성 입력 (추천)");
        assert_eq!(
            card.prompt.options[0].description.as_deref(),
            Some("오늘, 이번 달, 누적의 분·횟수·추정 비용을 보여주고 … 변경 범위가 가장 작습니다."),
            "a description the pane wrapped mid-word joins without a space"
        );
        assert_eq!(
            card.prompt.steps,
            vec![step("표시 위치", false, true), step("월 한도", false, false)],
            "the tabs are out of view: the call's questions stand in for them"
        );
        // Without the call the cut form has no tab bar and no question to stand on.
        assert!(card_for("omo", FORM_CUT, ASKS_NONE).is_none());
    }

    #[test]
    fn a_cut_form_matching_two_open_calls_is_refused() {
        assert!(card_for("omo", FORM_AMBIGUOUS, ASKS_AMBIGUOUS).is_none(), "nothing says which call the screen answers");
        let open = open_omo_asks(ASKS_AMBIGUOUS);
        let single = vec![open[0].clone()];
        let lines = screen_lines(FORM_AMBIGUOUS);
        let matched: Vec<OmoCard> = single
            .iter()
            .flat_map(|ask| {
                [
                    parse_omo_question(&lines, Some(ask), false),
                    parse_omo_typing(&lines, Some(ask), false),
                    parse_omo_review(&lines, Some(ask), false),
                ]
                .into_iter()
                .flatten()
            })
            .collect();
        assert_eq!(matched.len(), 1, "one call of that shape answers the form");
        assert_eq!(matched[0].prompt.question, "음성 사용량과 추정 비용을 어디에 보여줄까요?");
    }

    #[test]
    fn a_multi_select_form_keeps_its_checked_rows_and_tab_plan() {
        let card = card_for("omo", FORM_MULTI, ASKS_MULTI).expect("the multiple choice is a card");
        let prompt = &card.prompt;
        assert!(prompt.multi_select, "the hint says the form toggles");
        assert_eq!(prompt.custom_option_index, None, "a multiple choice has no single own-answer row");
        assert_eq!(prompt.title, "표시 위치");
        assert!(prompt.steps.is_empty(), "one question: the card shows no steps");
        assert_eq!(card.option_plans[0], key_steps(&["enter"]));
        assert_eq!(card.option_plans[1], key_steps(&["down", "enter"]));
        assert_eq!(card.multi_plan, Some(OmoMultiPlan { cursor: Some(0), rows: 3 }));
        assert_eq!(
            omo_answer_plan(&card, &ReferencePromptAnswer { option_indices: Some(vec![0, 1]), ..Default::default() }),
            Ok(key_steps(&["backspace", "space", "down", "space", "tab"])),
            "the chosen row is toggled off, the other on, then the form moves on"
        );
    }

    #[test]
    fn the_typed_answer_card_offers_save_and_discard() {
        let card = card_for("omo", FORM_TYPING, ASKS_CUT_THREE).expect("the typed row is a card");
        assert_eq!(card.responder, OmoResponder::Typing);
        assert_eq!(card.prompt.kind, ReferencePromptKind::Menu, "a number typed in the chat acts on the terminal's input");
        assert_eq!(card.prompt.title, "Question 1 of 2");
        assert_eq!(card.prompt.question, "음성 사용량과 추정 비용을 어디에 보여줄까요?");
        assert_eq!(
            card.prompt.options,
            vec![
                option("Save the typed answer", Some("설정 > 음성 입력 (추천)")),
                option("Discard it", None),
            ]
        );
        assert_eq!(card.prompt.custom_option_index, None);
        assert_eq!(card.option_plans[0], key_steps(&["enter"]));
        assert_eq!(card.option_plans[1], key_steps(&["esc"]));
        assert_eq!(
            omo_answer_plan(&card, &ReferencePromptAnswer { custom_text: Some("무엇이든".to_string()), ..Default::default() }),
            Err(ScopeErrorCode::InvalidRequest),
            "the card takes no typed answer of its own"
        );
    }

    #[test]
    fn the_review_card_lists_submit_each_answer_and_the_comment() {
        let card = card_for("omo", FORM_REVIEW, ASKS_TWO_QUESTIONS).expect("the Submit tab is a card");
        assert_eq!(card.responder, OmoResponder::Review);
        assert_eq!(card.prompt.kind, ReferencePromptKind::Menu);
        assert_eq!(card.prompt.title, "Review your answers");
        assert_eq!(card.prompt.question, "Submit (2/2 answered)");
        assert_eq!(card.prompt.body, None, "no notice and no comment are on this screen");
        assert_eq!(
            card.prompt.options,
            vec![
                option("Submit", None),
                option("표시 위치: 설정 > 음성 입력 (추천)", None),
                option("월 한도: 월 $5 한도", None),
                option("Comment (optional; unanswered questions are reported)", None),
            ]
        );
        assert_eq!(card.prompt.custom_option_index, Some(3), "the comment row is the custom one");
        assert_eq!(card.option_plans[0], key_steps(&["enter"]), "the cursor sits on the comment, below the rows");
        assert_eq!(card.option_plans[1], key_steps(&["up", "up", "enter"]));
        assert_eq!(card.option_plans[2], key_steps(&["up", "enter"]));
        assert!(card.option_plans[3].is_empty(), "the comment row answers with the typed answer alone");
        assert_eq!(
            omo_answer_plan(&card, &ReferencePromptAnswer { custom_text: Some("확인해 주세요".to_string()), ..Default::default() }),
            Ok(vec![ReferenceKeyStep::typed("확인해 주세요"), ReferenceKeyStep::keys(["enter"])])
        );
    }

    #[test]
    fn the_pending_widget_reads_the_call_it_names() {
        let card = card_for("omo", WIDGET_PENDING, ASKS_FOLDED).expect("the widget is a card");
        assert_eq!(card.responder, OmoResponder::Pending);
        assert_eq!(card.prompt.kind, ReferencePromptKind::Question);
        assert_eq!(card.prompt.title, "표시 위치");
        assert_eq!(card.prompt.question, "음성 사용량과 추정 비용을 어디에 보여줄까요?");
        assert_eq!(card.prompt.options.len(), 2);
        assert_eq!(card.prompt.custom_option_index, Some(2));
        assert!(card.prompt.steps.is_empty());
        // The route opens the form with its own key and plans from there: this card validates only.
        assert_eq!(REFERENCE_OMO_OPEN_FORM_KEY, "alt+up");
        assert!(card.option_plans.iter().all(|plan| plan.is_empty()));
        assert!(omo_answer_plan(&card, &option_answer(0)).expect("the card is answerable").is_empty());
        assert!(
            omo_answer_plan(&card, &ReferencePromptAnswer { custom_text: Some("내 답".to_string()), ..Default::default() })
                .expect("the card takes a typed answer")
                .is_empty()
        );
    }

    #[test]
    fn the_folded_widget_and_its_opened_form_share_one_id() {
        let folded = card_for("omo", WIDGET_PENDING, ASKS_FOLDED).expect("the widget is a card");
        let opened = card_for("omo", FORM_WIDGET_OPENED, ASKS_FOLDED).expect("the opened form is a card");
        assert_eq!(folded.responder, OmoResponder::Pending);
        assert_eq!(opened.responder, OmoResponder::Question);
        assert_eq!(
            folded.prompt.id, opened.prompt.id,
            "the answer route verifies the call and the question again by this id"
        );
    }

    #[test]
    fn a_form_that_no_longer_ends_the_screen_is_not_a_card() {
        assert!(card_for("omo", FORM_STALE, ASKS_TWO_QUESTIONS).is_none(), "the input box under the form is not OmO's footer");
    }

    #[test]
    fn a_form_quoted_in_a_shell_is_not_a_card() {
        assert!(card_for("omo", FORM_SHELL_QUOTED, ASKS_NONE).is_none(), "a shell prompt after the footer is not the form");
    }

    #[test]
    fn a_form_without_omo_s_own_footer_is_not_a_card() {
        for (name, screen) in [
            ("no rule", FORM_NO_RULE),
            ("two rules", FORM_TWO_RULES),
            ("an input box under it", FORM_BOX_UNDER),
            ("more footer lines than OmO draws", FORM_LONG_FOOTER),
        ] {
            assert!(card_for("omo", screen, ASKS_NONE).is_none(), "a form with {name} is not a card");
        }
        // The tabbed form, whose footer is OmO's own, is a card: the rule is not "any rule".
        assert!(card_for("omo", FORM_TABBED, ASKS_NONE).is_some());
    }

    #[test]
    fn a_widget_without_its_input_box_is_not_a_card() {
        assert!(card_for("omo", WIDGET_NO_BOX, ASKS_FOLDED).is_none(), "the widget lives over the input box");
        assert!(card_for("omo", WIDGET_FAR_BOX, ASKS_FOLDED).is_none(), "the box is out of the widget's reach");
        assert!(card_for("omo", WIDGET_PENDING, ASKS_FOLDED).is_some());
    }

    #[test]
    fn only_the_agents_the_reference_names_read_an_omo_form() {
        assert!(card_for("omo", FORM_TABBED, ASKS_TWO_QUESTIONS).is_some());
        assert!(card_for("", FORM_TABBED, ASKS_NONE).is_some(), "a pane herdr names no agent is omo's own");
        assert!(card_for("pi", FORM_TABBED, ASKS_NONE).is_some(), "herdr names an omo pane pi while it waits");
        assert!(
            card_for("claude", FORM_TABBED, ASKS_TWO_QUESTIONS).is_some(),
            "herdr names an omo pane claude while its claude-sdk child runs, and the call on screen is the form"
        );
        assert!(
            card_for("claude", FORM_TABBED, ASKS_NONE).is_none(),
            "with nothing to name the form, a claude pane is omo's only while it is blocked"
        );
        assert!(omo_form_is_trusted("claude", Some("blocked")));
        assert!(!omo_form_is_trusted("claude", Some("idle")));
        assert!(omo_form_is_trusted("omo", None));
        assert!(omo_form_is_trusted("", None));
        for agent in ["omp", "codex", "opencode", "grok", "cursor"] {
            assert!(!omo_reads_forms(agent), "{agent} never reaches the omo family");
            assert!(card_for(agent, FORM_TABBED, ASKS_NONE).is_none(), "{agent} is not an omo form's reader");
        }
    }

    #[test]
    fn the_form_marker_and_the_trust_rule_follow_the_route() {
        assert!(omo_form_on_screen(FORM_TABBED), "the hint is what makes a pane's session worth reading");
        assert!(omo_form_on_screen(WIDGET_PENDING));
        assert!(!omo_form_on_screen("hello world"));
        assert!(!omo_form_on_screen(&FORM_STALE.lines().take(2).collect::<Vec<_>>().join("\n")));
    }

    #[test]
    fn the_own_answer_row_is_the_one_the_pin_names() {
        assert!(OMO_OWN_ANSWER.starts_with("Type your own"));
        let lines = screen_lines(FORM_TABBED);
        let hint = find_last_index(&lines, |_, index| {
            options_hint_re().is_match(&wrapped_lines(&lines, index, OMO_HINT_LINES))
        })
        .expect("the options hint is on the screen");
        let bar_end = omo_form(&lines, hint).expect("the tab bar is whole").bar_end;
        let view = omo_question_view(&lines, bar_end + 1, hint, false);
        let own = view.own.expect("the own-answer row is in view");
        assert_eq!(clean_line(&lines[own.1]), OMO_OWN_ANSWER, "the row the view takes is the one the pin names");
    }

    #[test]
    fn the_answer_plan_walks_the_cursor_to_the_chosen_row() {
        let card = card_for("omo", FORM_TABBED, ASKS_TWO_QUESTIONS).expect("the form is a card");
        assert_eq!(card.option_plans[0], key_steps(&["enter"]), "the cursor is already on the first row");
        assert_eq!(card.option_plans[1], key_steps(&["down", "enter"]));
        assert_eq!(omo_answer_plan(&card, &option_answer(1)), Ok(key_steps(&["down", "enter"])));
        // From the first row, two moves reach the own-answer row; the text follows, then Enter.
        assert_eq!(
            omo_answer_plan(&card, &ReferencePromptAnswer { custom_text: Some("월 $7 한도".to_string()), ..Default::default() }),
            Ok(vec![
                ReferenceKeyStep::keys(["backspace"]),
                ReferenceKeyStep::keys(["down"]),
                ReferenceKeyStep::keys(["down"]),
                ReferenceKeyStep::keys(["enter"]),
                ReferenceKeyStep::typed("월 $7 한도"),
                ReferenceKeyStep::keys(["enter"]),
            ])
        );
    }

    #[test]
    fn an_answer_that_is_not_exactly_one_shape_is_refused() {
        let card = card_for("omo", FORM_TABBED, ASKS_TWO_QUESTIONS).expect("the form is a card");
        assert_eq!(omo_answer_plan(&card, &ReferencePromptAnswer::default()), Err(ScopeErrorCode::InvalidRequest));
        let two = ReferencePromptAnswer { option_index: Some(0), custom_text: Some("x".to_string()), ..Default::default() };
        assert_eq!(omo_answer_plan(&card, &two), Err(ScopeErrorCode::InvalidRequest));
        let blank = ReferencePromptAnswer { custom_text: Some("   ".to_string()), ..Default::default() };
        assert_eq!(omo_answer_plan(&card, &blank), Err(ScopeErrorCode::InvalidRequest));
        assert_eq!(omo_answer_plan(&card, &option_answer(2)), Err(ScopeErrorCode::InvalidRequest), "the own-answer row is not an option");
        assert_eq!(omo_answer_plan(&card, &option_answer(9)), Err(ScopeErrorCode::InvalidRequest));
        let list = ReferencePromptAnswer { option_indices: Some(vec![0]), ..Default::default() };
        assert_eq!(omo_answer_plan(&card, &list), Err(ScopeErrorCode::InvalidRequest), "a single choice takes no list");
    }

    #[test]
    fn a_multi_select_card_refuses_one_index_and_an_empty_list() {
        let multi = card_for("omo", FORM_MULTI, ASKS_MULTI).expect("the multiple choice is a card");
        assert_eq!(omo_answer_plan(&multi, &option_answer(0)), Err(ScopeErrorCode::InvalidRequest));
        let empty = ReferencePromptAnswer { option_indices: Some(Vec::new()), ..Default::default() };
        assert_eq!(omo_answer_plan(&multi, &empty), Err(ScopeErrorCode::InvalidRequest));
        let out_of_range = ReferencePromptAnswer { option_indices: Some(vec![0, 5]), ..Default::default() };
        assert_eq!(omo_answer_plan(&multi, &out_of_range), Err(ScopeErrorCode::InvalidRequest));
    }

    #[test]
    fn a_prompt_this_lane_did_not_parse_is_refused() {
        let stranger = ReferencePrompt {
            id: "000000000000".to_string(),
            agent: "omo".to_string(),
            kind: ReferencePromptKind::Question,
            title: "표시 위치".to_string(),
            question: "무엇이든".to_string(),
            body: None,
            options: vec![option("하나", None)],
            multi_select: false,
            custom_option_index: None,
            queued: None,
            steps: Vec::new(),
            fallback: None,
        };
        assert_eq!(reference_omo_answer_keys(&stranger, &option_answer(0)), Err(ScopeErrorCode::InvalidRequest));
        // A parsed card resolves through the same entry point.
        let card = card_for("omo", FORM_TABBED, ASKS_TWO_QUESTIONS).expect("the form is a card");
        assert_eq!(reference_omo_answer_keys(&card.prompt, &option_answer(1)), Ok(key_steps(&["down", "enter"])));
    }

    #[test]
    fn the_session_lifecycle_keeps_only_the_call_that_waits() {
        let open = open_omo_asks(ASKS_LIFECYCLE);
        assert_eq!(open.len(), 1, "a tool result, an error result, an answer frame and a settlement each close their call");
        let ask = &open[0];
        assert_eq!(ask.id, "call-waiting");
        assert!(ask.wait);
        assert_eq!(ask.questions.len(), 2);
        assert_eq!(ask.questions[0].header, "표시 위치");
        assert_eq!(ask.questions[1].options.len(), 2);
        assert_eq!(pending_omo_ask(ASKS_LIFECYCLE).map(|ask| ask.id), Some("call-waiting".to_string()));
        assert!(open_omo_asks(ASKS_NONE).is_empty(), "a session with no ask call has nothing open");
    }

    #[test]
    fn a_call_that_does_not_wait_stays_open_until_it_is_settled() {
        let open = open_omo_asks(ASKS_FOLDED);
        assert_eq!(open.len(), 1, "an accepted pending result keeps the call open");
        assert!(!open[0].wait);
        let settled = format!("{ASKS_FOLDED}{}\n", r#"{"type":"custom","customType":"ask-user:settlement","data":{"requestId":"call-folded"}}"#);
        assert!(open_omo_asks(&settled).is_empty(), "the extension's own record settles it");
        let answered = format!(
            "{ASKS_FOLDED}{}\n",
            r#"{"type":"message","id":"u","message":{"role":"user","content":"[Answer to question call-folded]\n설정 > 음성 입력 (추천)"}}"#
        );
        assert!(open_omo_asks(&answered).is_empty(), "the answer delivered as a user message settles it");
    }

    #[test]
    fn the_ask_shape_and_its_tools_follow_the_pin() {
        assert!(omo_ask_tool("ask_user_question"));
        assert!(omo_ask_tool("request_user_input"));
        assert!(!omo_ask_tool("read"));
        assert!(omo_ask_waits(&json!({ "waitForAnswer": true })));
        assert!(omo_ask_waits(&json!({})));
        assert!(!omo_ask_waits(&json!({ "waitForAnswer": false })));
        assert!(!omo_ask_waits(&json!({ "wait_for_answer": false })));
        assert!(omo_ask_record_interesting(r#"{"message":{"role":"assistant"}}"#));
        assert!(omo_ask_record_interesting(r#"{"customType":"ask-user:settlement"}"#));
        assert!(!omo_ask_record_interesting(r#"{"type":"custom_message","customType":"info"}"#));
        let call = OmoAskCall {
            id: "call-1".to_string(),
            wait: true,
            args: json!({ "questions": [ { "header": "h" } ] }),
        };
        assert!(omo_ask_of(&call).is_none(), "a question without its own text is not the shape OmO asks with");
        let call = OmoAskCall {
            id: "call-1".to_string(),
            wait: true,
            args: json!({ "questions": [ { "header": "h", "question": "q", "options": [ { "description": "no label" } ] } ] }),
        };
        assert!(omo_ask_of(&call).is_none(), "an option without a label drops the call");
    }

    #[test]
    fn the_lane_signatures_are_held_as_values() {
        let detector: ReferencePromptDetector = omo_prompt_detector();
        let card = detector("omo", FORM_TABBED).expect("the detector reads a whole form");
        assert_eq!(card.question, "월 한도를 얼마로 할까요?");
        assert!(detector("omo", FORM_CUT).is_none(), "a cut form needs the session's call");
        assert!(detector("grok", FORM_TABBED).is_none(), "a detector must not claim a provider the reference does not name");
        let planner: ReferenceAnswerPlanner = omo_answer_planner();
        assert_eq!(planner(&card, &option_answer(1)), Ok(key_steps(&["down", "enter"])));
        let unusable = ReferencePromptAnswer::default();
        assert_eq!(planner(&card, &unusable), Err("invalid_answer".to_string()));
    }
}

