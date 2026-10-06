//! gjc native history family (plan task 20).
//!
//! Ported from `devswha/herdr-web-ui` @ `54e5a1f67090cb09552d182e7e30dd0ecc314918`
//! (MIT, see `docs/chat/HERDR_LICENSE`). The upstream sources that define this family are
//! `server/transcript-records.ts` (`parseOmpTranscript`, the reader the pinned
//! `server/conversation.ts` routes `gjc-transcript` to), `server/tool-output.ts`
//! (`TOOL_OUTPUT_CHARS` / `WHOLE_OUTPUT_CHARS` and the cut `output_ref`/`output_size`
//! pair) and `shared/protocol.ts` (`ConversationTurn` / `ConversationPart`).
//!
//! gjc writes **omp's session shape** (`conversation.ts`: "gjc: an open session file or
//! fresh native terminal breadcrumb belonging to its process (gjc-runtime.ts). It writes
//! omp's session shape too."), so this reader is the omp-shaped record family: `session` /
//! `header_patch` headers, `message` records with `user` / `assistant` / `toolResult`
//! roles, `custom_message` notices (gjc's background-job result), a `custom` +
//! `context_clear` reset and `compaction` summaries.
//!
//! This module is the **parser only**. Which file belongs to a pane is
//! `gjcTranscriptForPane`'s job upstream (process identity, terminal breadcrumb, screen
//! title) and is task 3's resolver here — this module never infers a session from a cwd.
//!
//! Boundaries the pinned reference states, kept deliberately:
//! * **no tool images**: `conversation.ts` passes `toolImages: false` for every source but
//!   pi ("omp, omo and gjc are read the same way but would carry image refs nothing can
//!   answer"). An image-only user record is therefore skipped, never an empty turn.
//! * **no `taskResult`**: OmO's `omo-senpi:wake` background-task completion belongs to the
//!   omo family (task 19). A `custom_message` that is not a displayable notice is chrome.
//! * **no abandoned-branch disclosure**: gjc keeps no entry tree; only pi's `/tree` leaves
//!   turns a page cannot reach.
//! * **no prompt detector**: the reference names none for gjc (`docs/chat/herdr-port-contract.md`
//!   section 1), so this module exports no detector.
//! * **no turn cap**: the reference parses a page with `Infinity` and lets the page bound
//!   the turns, so a cap here would silently drop turns between pages.
//!
//! A skill invocation is ported: the reference reads the envelope into the request the person
//! typed plus one chip per skill the runtime loaded (`transcript-records.ts:259`), and so does
//! this reader — the request is the user's text and each skill is its own
//! [`ReferencePart::Skill`]. See the module's test
//! `a_skill_invocation_prompt_keeps_its_request_and_chips_the_skill`.

use std::sync::OnceLock;

use regex::Regex;
use serde_json::Value;

use super::types::{
    ReferenceHistoryParser, ReferenceNativeHistoryKind, ReferencePart, ReferenceSkillActivity,
    ReferenceSkillEvidence, ReferenceSkillStatus, ReferenceTurn, ReferenceTurnRole,
};

/// Past this a tool's output is cut in the page; the whole of it is fetched by its ref.
/// `tool-output.ts`: `TOOL_OUTPUT_CHARS`.
pub const REFERENCE_GJC_TOOL_OUTPUT_CHARS: usize = 4_000;

/// The whole output kept for the goal tools, whose answer *is* their JSON.
/// `tool-output.ts`: `WHOLE_OUTPUT_CHARS`.
pub const REFERENCE_GJC_WHOLE_OUTPUT_CHARS: usize = 16_000;

/// The tools whose output is kept whole up to [`REFERENCE_GJC_WHOLE_OUTPUT_CHARS`].
/// `tool-output.ts`: `WHOLE_OUTPUT_TOOLS`.
pub const REFERENCE_GJC_WHOLE_OUTPUT_TOOLS: [&str; 3] = ["create_goal", "update_goal", "get_goal"];

/// The reference's own default cap (`transcript-records.ts`: `MAX_TURNS`).
///
/// This family does **not** apply it: the pinned `conversation.ts` reads an omp-shaped store
/// through `parseOmpTranscript(text, Infinity)` and lets the page bound the turns, so applying
/// the default here would silently drop turns between pages. It stays exported for callers that
/// want the reference's default explicitly.
pub const REFERENCE_GJC_MAX_TURNS: usize = 100;

/// Every record variant the pinned reference names for a gjc transcript.
///
/// The inventory is the module's own, and `fixtures/gjc/manifest.json` carries the same
/// names with the effect each one has; the regression test compares the two, so a variant
/// added to one without the other fails. `handled: true` in the manifest means *this reader
/// renders it*; `handled: false` means it is chrome or an explicitly refused neighbor
/// (OmO's wake) — never an unexplained skip.
pub const REFERENCE_GJC_RECORD_VARIANTS: [&str; 21] = [
    "title",
    "session",
    "header_patch",
    "message-user",
    "message-assistant",
    "content-text",
    "content-thinking",
    "content-tool-call",
    "content-tool-result",
    "message-tool-result",
    "content-image",
    "custom-message-notice",
    "custom-context-clear",
    "compaction",
    "custom-other",
    "custom-message-omo-wake",
    "stop-reason-stop",
    "stop-reason-error",
    "message-hidden",
    "malformed-line",
    "skill-invocation",
];

/// The family's entry point in the shape the frozen [`ReferenceHistoryParser`] alias takes.
///
/// A kind that is not [`ReferenceNativeHistoryKind::Gjc`] is **refused**, never answered
/// with an empty turn list: an empty success would read as "this session has no turns",
/// which is what `Unavailable` must never claim.
pub fn parse_reference_gjc(
    kind: ReferenceNativeHistoryKind,
    text: &str,
) -> Result<Vec<ReferenceTurn>, String> {
    if kind != ReferenceNativeHistoryKind::Gjc {
        return Err(format!(
            "the gjc history family reads gjc-transcript records, not {}",
            kind.as_str()
        ));
    }
    Ok(parse_reference_gjc_history(text))
}

/// The gjc reader itself, for the dispatcher that has already routed by kind (task 2).
///
/// A torn tail line while gjc appends, or any unreadable line, is skipped rather than
/// fatal; the turns that did parse are returned.
pub fn parse_reference_gjc_history(text: &str) -> Vec<ReferenceTurn> {
    let mut turns: Vec<ReferenceTurn> = Vec::new();
    let mut pending: Vec<PendingTool> = Vec::new();
    let mut settled = false;

    for line in text.split('\n') {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(record) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if !record.is_object() {
            continue;
        }
        // `context_clear` drops every turn recorded before it (transcript-records.ts).
        if is_context_clear(&record) {
            turns.clear();
            pending.clear();
            settled = false;
            continue;
        }
        // gjc's background-job result reaches the user's seat as a notice.
        if let Some(part) = notice_part(&record) {
            turns.push(user_turn(timestamp(&record), vec![part]));
            continue;
        }
        // A compaction entry is not a message: it says where the conversation was folded.
        if record.get("type").and_then(Value::as_str) == Some("compaction") {
            let summary = record
                .get("summary")
                .and_then(Value::as_str)
                .filter(|summary| !summary.trim().is_empty());
            if let Some(summary) = summary {
                turns.push(user_turn(
                    timestamp(&record),
                    vec![ReferencePart::Compact { text: summary.to_string() }],
                ));
            }
            continue;
        }
        if record.get("type").and_then(Value::as_str) != Some("message") {
            continue;
        }
        let Some(message) = record.get("message").and_then(Value::as_object) else {
            continue;
        };
        if record.get("display").and_then(Value::as_bool) == Some(false)
            || message.get("display").and_then(Value::as_bool) == Some(false)
        {
            continue;
        }
        let ts = timestamp(&record);
        match message.get("role").and_then(Value::as_str).unwrap_or("") {
            "user" => {
                let prompt = user_text(message.get("content"));
                if prompt.is_empty() {
                    continue;
                }
                // A skill invocation reads as what the user asked, the skill as a chip on it: not
                // as the SKILL.md the runtime put before it. The request is the user's text and
                // each loaded skill rides it as its own part, in the order the envelope named them
                // (`transcript-records.ts:259`).
                let invocation = gjc_skill_invocation(&prompt);
                let asked = match &invocation {
                    Some(invocation) if !invocation.request.is_empty() => invocation.request.clone(),
                    Some(invocation) => invocation
                        .skills
                        .iter()
                        .map(|skill| format!("/skill:{}", skill.name))
                        .collect::<Vec<_>>()
                        .join(" "),
                    None => prompt,
                };
                let mut parts = vec![ReferencePart::Text { text: asked, phase: None }];
                if let Some(invocation) = &invocation {
                    parts.extend(
                        invocation
                            .skills
                            .iter()
                            .cloned()
                            .map(|skill| ReferencePart::Skill { skill }),
                    );
                }
                turns.push(user_turn(ts, parts));
            }
            "assistant" => {
                let Some(blocks) = message.get("content").and_then(Value::as_array) else {
                    continue;
                };
                let at = assistant_turn(&mut turns, ts.clone(), settled);
                if let Some(ended_at) = ts {
                    turns[at].ended_at = Some(ended_at);
                }
                for block in blocks {
                    if let Some(part) = assistant_block(block) {
                        let index = turns[at].parts.len();
                        if let (ReferencePart::Tool { .. }, Some(id)) =
                            (&part, block.get("id").and_then(Value::as_str))
                        {
                            pending.push(PendingTool { id: id.to_string(), turn: at, part: index });
                        }
                        turns[at].parts.push(part);
                    }
                }
                // a provider can place a result beside its call in the same assistant record
                fold_message_results(&mut turns, &mut pending, message);
                // a failed request leaves an empty message: without its error the chat would
                // show the prompt with no answer at all
                if message.get("stopReason").and_then(Value::as_str) == Some("error") {
                    let error = message.get("errorMessage").and_then(Value::as_str);
                    if let Some(error) = error.filter(|error| !error.is_empty()) {
                        turns[at]
                            .parts
                            .push(ReferencePart::Text { text: format!("Error: {error}"), phase: None });
                    }
                }
                settled = message.get("stopReason").and_then(Value::as_str) == Some("stop");
            }
            "toolResult" => fold_message_results(&mut turns, &mut pending, message),
            _ => {}
        }
    }

    turns.retain(|turn| !turn.parts.is_empty());
    turns
}

/// The frozen alias, satisfied by this family's entry point.
pub const REFERENCE_GJC_PARSER: ReferenceHistoryParser = parse_reference_gjc;

/// A tool call whose result has not been read yet.
struct PendingTool {
    id: String,
    turn: usize,
    part: usize,
}

/// A turn in the user's seat that nobody typed: gjc's notice, a compaction summary, or the
/// request text a skill invocation carried plus one chip per skill it named.
fn user_turn(started_at: Option<String>, parts: Vec<ReferencePart>) -> ReferenceTurn {
    ReferenceTurn {
        role: ReferenceTurnRole::User,
        started_at,
        ended_at: None,
        source: None,
        parts,
        abandoned: None,
    }
}

/// The assistant turn a record belongs to: the last one while it has not stopped for good,
/// else a new one. Returns its index in `turns`.
fn assistant_turn(turns: &mut Vec<ReferenceTurn>, started_at: Option<String>, settled: bool) -> usize {
    let merge = matches!(
        turns.last(),
        Some(last) if last.role == ReferenceTurnRole::Assistant && !settled
    );
    if merge {
        return turns.len() - 1;
    }
    turns.push(ReferenceTurn {
        role: ReferenceTurnRole::Assistant,
        started_at,
        ended_at: None,
        source: None,
        parts: Vec::new(),
        abandoned: None,
    });
    turns.len() - 1
}

/// One assistant content block, or `None` for a block the reference does not render.
fn assistant_block(block: &Value) -> Option<ReferencePart> {
    match block.get("type").and_then(Value::as_str)? {
        "text" => {
            let text = block.get("text").and_then(Value::as_str)?;
            (!text.is_empty()).then(|| ReferencePart::Text { text: text.to_string(), phase: None })
        }
        "thinking" => {
            let thinking = block
                .get("thinking")
                .and_then(Value::as_str)
                .or_else(|| block.get("text").and_then(Value::as_str))
                .unwrap_or("");
            (!thinking.is_empty()).then(|| ReferencePart::Thinking { text: thinking.to_string() })
        }
        "toolCall" => {
            let name = block.get("name").and_then(Value::as_str)?;
            let input = match block.get("arguments") {
                Some(value @ Value::Object(_)) => value.clone(),
                _ => Value::Object(serde_json::Map::new()),
            };
            // the call's own intent, else the first interesting argument
            let intent = block.get("intent").and_then(Value::as_str).filter(|intent| !intent.is_empty());
            let summary = match intent {
                Some(intent) => take_chars(intent, 120),
                None => tool_summary(name, &input),
            };
            Some(ReferencePart::Tool {
                name: name.to_string(),
                summary,
                input: serde_json::to_string_pretty(&input).unwrap_or_else(|_| "{}".to_string()),
                output: String::new(),
                error: None,
                skill: None,
                output_ref: None,
                output_size: None,
                images: Vec::new(),
            })
        }
        _ => None, // an unsupported transcript block is intentionally ignored
    }
}

/// Fold every tool result a record carries into the call it answers.
fn fold_message_results(
    turns: &mut [ReferenceTurn],
    pending: &mut Vec<PendingTool>,
    message: &serde_json::Map<String, Value>,
) {
    let own = message.get("role").and_then(Value::as_str) == Some("toolResult");
    let blocks: Vec<&Value> = if own {
        vec![]
    } else {
        match message.get("content") {
            Some(Value::Array(items)) => items
                .iter()
                .filter(|item| item.get("type").and_then(Value::as_str) == Some("toolResult"))
                .collect(),
            _ => Vec::new(),
        }
    };
    let results: Vec<&serde_json::Map<String, Value>> = if own {
        std::iter::once(message).collect()
    } else {
        blocks.iter().filter_map(|block| block.as_object()).collect()
    };
    for result in results {
        let Some(id) = result.get("toolCallId").and_then(Value::as_str) else {
            continue;
        };
        let output = result_text(result.get("content"));
        let is_error = result.get("isError").and_then(Value::as_bool) == Some(true);
        fold_result(turns, pending, id, &output, is_error);
    }
}

/// Give one tool part the output of the result that answers it, cut to what a page carries.
fn fold_result(
    turns: &mut [ReferenceTurn],
    pending: &mut Vec<PendingTool>,
    id: &str,
    output: &str,
    is_error: bool,
) {
    let Some(at) = pending.iter().position(|tool| tool.id == id) else {
        return;
    };
    let tool = pending.remove(at);
    let Some(part) = turns.get_mut(tool.turn).and_then(|turn| turn.parts.get_mut(tool.part)) else {
        return;
    };
    let ReferencePart::Tool {
        name,
        output: slot,
        error,
        output_ref,
        output_size,
        ..
    } = part
    else {
        return;
    };
    let limit = if REFERENCE_GJC_WHOLE_OUTPUT_TOOLS.contains(&name.as_str()) {
        REFERENCE_GJC_WHOLE_OUTPUT_CHARS
    } else {
        REFERENCE_GJC_TOOL_OUTPUT_CHARS
    };
    let length = output.chars().count();
    if length <= limit {
        *slot = output.to_string();
    } else {
        *slot = format!("{}\n… trimmed", take_chars(output, limit));
        *output_ref = Some(id.to_string());
        *output_size = Some(length as u64);
    }
    if is_error {
        *error = Some(true);
    }
}

/// `transcript-records.ts`'s `isContextClear` for the omp-shaped stores: a `custom` record
/// whose `customType` is `context_clear`. (Claude's `<command-name>/clear</command-name>`
/// envelope is the claude family's own spelling.)
fn is_context_clear(record: &Value) -> bool {
    record.get("type").and_then(Value::as_str) == Some("custom")
        && record.get("customType").and_then(Value::as_str) == Some("context_clear")
}

/// gjc's background-job result: a displayable `custom_message` in the user's seat, with the
/// `<system-notice>` envelope stripped and the runtime's own name kept as its source.
fn notice_part(record: &Value) -> Option<ReferencePart> {
    if record.get("type").and_then(Value::as_str) != Some("custom_message")
        || record.get("display").and_then(Value::as_bool) == Some(false)
    {
        return None;
    }
    let text = strip_system_notice(record.get("content").and_then(Value::as_str)?);
    if text.is_empty() {
        return None;
    }
    Some(ReferencePart::Notice {
        text,
        source: record.get("customType").and_then(Value::as_str).map(str::to_string),
    })
}

/// The notice's text without the runtime's envelope.
fn strip_system_notice(content: &str) -> String {
    let trimmed = content.trim();
    let body = trimmed.strip_prefix("<system-notice>").unwrap_or(trimmed);
    body.strip_suffix("</system-notice>").unwrap_or(body).trim().to_string()
}

/// A user record's prompt: a string content as it is, else its text blocks joined by newlines.
/// A record whose parts hold no text (an image only) yields nothing and is skipped.
fn user_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .filter(|item| item.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|item| item.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// A tool result's text: a string as it is, else the `text` of each content block.
fn result_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| item.get("text").and_then(Value::as_str).unwrap_or(""))
            .collect::<String>(),
        _ => String::new(),
    }
}

/// The one-line summary a collapsed tool chip shows (`transcript-records.ts`: `toolSummary`).
fn tool_summary(name: &str, input: &Value) -> String {
    // an OmO or omp `task` call: the summary it gave the person, one per task of a batch
    if name == "task" {
        let items: Vec<&Value> = match input.get("tasks") {
            Some(Value::Array(tasks)) => tasks.iter().collect(),
            _ => vec![input],
        };
        let titles: Vec<String> = items
            .iter()
            .filter_map(|item| {
                label(item.get("task_summary")).or_else(|| label(item.get("description")))
            })
            .collect();
        if !titles.is_empty() {
            return take_chars(&titles.join(" · "), 120);
        }
    }
    // pi names a file `path` where Claude names it `file_path`, and a notebook `notebook_path`.
    for key in ["command", "file_path", "notebook_path", "path", "pattern", "description", "url"] {
        if let Some(text) = input.get(key).and_then(Value::as_str) {
            return take_chars(text, 120);
        }
    }
    name.to_string()
}

/// A record's `timestamp`, when it states one.
fn timestamp(record: &Value) -> Option<String> {
    record.get("timestamp").and_then(Value::as_str).map(str::to_string)
}

/// A string field's trimmed value, when it holds anything.
fn non_empty_str(value: Option<&Value>) -> Option<&str> {
    let text = value?.as_str()?.trim();
    (!text.is_empty()).then_some(text)
}

/// A trimmed, non-empty string as a label (the reference's `label`).
fn label(value: Option<&Value>) -> Option<String> {
    non_empty_str(value).map(str::to_string)
}

/// The first `limit` characters, never splitting a character.
fn take_chars(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

// ---------------------------------------------------------------------------------------------
// The skill invocation behind a prompt (`skill-activity.ts`: `skillInvocationPrompt`)
// ---------------------------------------------------------------------------------------------

/// The skill evidence a user prompt carried, when the prompt was a skill invocation.
///
/// The reader pushes one [`ReferencePart::Skill`] per skill here, beside the request text,
/// exactly as upstream does (`transcript-records.ts:259`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GjcSkillInvocation {
    /// The skills the runtime loaded before the user's request, in the order it named them.
    pub skills: Vec<ReferenceSkillActivity>,
    /// What the user actually asked, after the injected instruction envelope.
    pub request: String,
}

/// The prompt omp/omo/gjc/pi record when the user invokes a skill (`/skill:name`, `$name`, a
/// keyword): the whole SKILL.md before the request, tens of KB the user never typed.
///
/// Mirrors `skillInvocationPrompt` (`skill-activity.ts:41-63`): chained `<skill-instruction>`
/// envelopes, then `<user-request>`, or the legacy `<skill name location>` form. Anything else is
/// `None` and the prompt stands as the user's own text.
pub fn gjc_skill_invocation(prompt: &str) -> Option<GjcSkillInvocation> {
    let instruction = skill_instruction_pattern();
    let request_pattern = user_request_pattern();
    let legacy_pattern = legacy_skill_pattern();

    let mut skills: Vec<ReferenceSkillActivity> = Vec::new();
    let mut remainder = prompt;
    let mut matched = instruction.captures(remainder);

    while let Some(captures) = matched {
        let name = captures.get(1).map(|group| group.as_str()).unwrap_or_default();
        let declared = captures.get(2).map(|group| group.as_str()).unwrap_or_default();
        let location = captures.get(3).map(|group| group.as_str()).unwrap_or_default();
        // the envelope must agree with itself about which skill it injected
        let skill = if name == declared { loaded_skill(name, location) } else { None };
        let Some(skill) = skill else { return None };
        skills.push(skill);
        remainder = &remainder[captures.get(0)?.as_str().len()..];
        if !remainder.starts_with("\n\nThe user explicitly invoked the ") {
            break;
        }
        remainder = &remainder[2..];
        matched = instruction.captures(remainder);
        if matched.is_none() {
            return None;
        }
    }

    if !skills.is_empty() {
        if remainder.is_empty() {
            return Some(GjcSkillInvocation { skills, request: String::new() });
        }
        let request = request_pattern.captures(remainder)?;
        let request = request.get(1).map(|group| group.as_str().trim().to_string())?;
        return Some(GjcSkillInvocation { skills, request });
    }

    let legacy = legacy_pattern.captures(prompt)?;
    let skill = loaded_skill(
        legacy.get(1).map(|group| group.as_str()).unwrap_or_default(),
        legacy.get(2).map(|group| group.as_str()).unwrap_or_default(),
    )?;
    let request = legacy
        .get(3)
        .map(|group| group.as_str().trim().to_string())
        .unwrap_or_default();
    Some(GjcSkillInvocation { skills: vec![skill], request })
}

/// The skill file a prompt named (`skill-activity.ts:29-33`).
fn loaded_skill(name: &str, location: &str) -> Option<ReferenceSkillActivity> {
    let name = skill_label(name)?;
    if location.is_empty() || location.chars().count() > 4096 || location.contains('\n') || location.contains('\r') {
        return None;
    }
    Some(ReferenceSkillActivity {
        name,
        path: Some(location.to_string()),
        evidence: ReferenceSkillEvidence::Instructions,
        status: ReferenceSkillStatus::Loaded,
    })
}

/// A skill name a chip can show (`skill-activity.ts:5`): short, single-line, no markup.
fn skill_label(value: &str) -> Option<String> {
    if value.is_empty() || value.chars().count() > 200 {
        return None;
    }
    if value.contains('\r') || value.contains('\n') || value.contains('<') || value.contains('>') {
        return None;
    }
    Some(value.to_string())
}

/// `^The user explicitly invoked the "…" skill. …` (`skill-activity.ts:25`).
const SKILL_INSTRUCTION_PATTERN: &str = r#"^The user explicitly invoked the "([^"]+)" skill\. Follow the instructions in <skill-instruction> as binding for this request, while respecting higher-priority instructions\.\n\n<skill-instruction name="([^"]+)" location="([^"]+)">\n[\s\S]*?\n</skill-instruction>"#;

/// The user's own request under the envelopes (`skill-activity.ts:57`).
const USER_REQUEST_PATTERN: &str = r#"^\n\n<user-request>\n([\s\S]*?)\n</user-request>$"#;

/// A standalone `.md` skill (`--skill review.md`), the older form (`skill-activity.ts:26`).
const LEGACY_SKILL_PATTERN: &str =
    r#"^<skill name="([^"]+)" location="([^"]+)">\n[\s\S]*?\n</skill>(?:\n\n([\s\S]+))?$"#;

fn skill_instruction_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(SKILL_INSTRUCTION_PATTERN)
            .expect("the pinned skill-instruction envelope is a valid regex")
    })
}

fn user_request_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(USER_REQUEST_PATTERN).expect("the pinned user-request envelope is a valid regex")
    })
}

fn legacy_skill_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(LEGACY_SKILL_PATTERN).expect("the pinned legacy skill envelope is a valid regex")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fixtures live beside the family they pin, in this module's `fixtures/gjc/`.
    /// `include_str!` binds the bytes at compile time, so a fixture edit is a rebuild.
    const NORMAL: &str = include_str!("fixtures/gjc/normal.jsonl");
    const MALFORMED: &str = include_str!("fixtures/gjc/malformed.jsonl");
    const RESET: &str = include_str!("fixtures/gjc/reset.jsonl");
    const BOUNDARY: &str = include_str!("fixtures/gjc/boundary.jsonl");
    const MANIFEST: &str = include_str!("fixtures/gjc/manifest.json");

    fn parse(text: &str) -> Vec<ReferenceTurn> {
        parse_reference_gjc_history(text)
    }

    fn parts_of(turn: &ReferenceTurn) -> Vec<&ReferencePart> {
        turn.parts.iter().collect()
    }

    fn tool_part<'a>(turn: &'a ReferenceTurn, name: &str) -> &'a ReferencePart {
        turn.parts
            .iter()
            .find(|part| matches!(part, ReferencePart::Tool { name: held, .. } if held == name))
            .expect("the turn holds the tool call")
    }

    #[test]
    fn normal_fixture_reads_user_and_merged_assistant_turns() {
        // C1: the ordinary session — a prompt, one assistant turn whose records merge, a
        // second prompt, and the answer to it.
        let turns = parse(NORMAL);
        assert_eq!(
            turns.iter().map(|turn| turn.role).collect::<Vec<_>>(),
            vec![
                ReferenceTurnRole::User,
                ReferenceTurnRole::Assistant,
                ReferenceTurnRole::User,
                ReferenceTurnRole::Assistant
            ]
        );
        assert_eq!(turns[0].started_at.as_deref(), Some("2026-10-06T09:00:01.000Z"));
        assert_eq!(turns[0].parts, vec![ReferencePart::Text { text: "터미널 주소좀 줘봐".into(), phase: None }]);
        // the assistant records merge while the one before them did not stop for good, and
        // the last recorded activity — not the next user's timestamp — ends the turn
        assert_eq!(turns[1].started_at.as_deref(), Some("2026-10-06T09:00:02.000Z"));
        assert_eq!(turns[1].ended_at.as_deref(), Some("2026-10-06T09:00:05.000Z"));
        assert_eq!(turns[1].parts.len(), 4);
        assert_eq!(turns[1].parts[0], ReferencePart::Thinking { text: "internal reasoning stays private".into() });
        assert!(matches!(turns[1].parts[3], ReferencePart::Text { ref text, .. } if text == "http://100.123.228.51:7317"));
        // a result placed beside its call in the same assistant record folds into that call
        let ReferencePart::Tool { name, summary, output, .. } = tool_part(&turns[1], "read") else {
            unreachable!("matched above")
        };
        assert_eq!(name, "read");
        assert_eq!(summary, "Reading the hosts file");
        assert_eq!(output, "127.0.0.1 localhost");
        // the tool part carries the call's own intent as its summary, its pretty-printed
        // arguments as its input, and the output of the result that answers it
        let ReferencePart::Tool { name, summary, input, output, error, output_ref, output_size, .. } =
            tool_part(&turns[1], "bash")
        else {
            unreachable!("matched above")
        };
        assert_eq!(name, "bash");
        assert_eq!(summary, "Checking listening ports");
        assert!(input.contains("ss -tlnp"), "input holds the call's arguments: {input}");
        assert_eq!(output, "LISTEN 0 512 100.123.228.51:7317");
        assert_eq!(*error, None, "a call that succeeded is not marked failed");
        assert_eq!(*output_ref, None);
        assert_eq!(*output_size, None);
        // the image-only user record is skipped, not an empty turn; the plain string prompt reads
        assert_eq!(turns[2].parts, vec![ReferencePart::Text { text: "plain string prompt".into(), phase: None }]);
        assert_eq!(turns[3].parts, vec![ReferencePart::Text { text: "Ack.".into(), phase: None }]);
        // the title, session and header_patch records are chrome, not turns
        let rendered = serde_json::to_string(&turns).expect("turns serialize");
        assert!(!rendered.contains("GJC reference chat parity"));
        assert!(!rendered.contains("header_patch"));
    }

    #[test]
    fn malformed_lines_and_a_torn_tail_survive() {
        // C2: an unreadable line and a tail line gjc has not finished writing are skipped;
        // every record that did parse still reads.
        let turns = parse(MALFORMED);
        assert_eq!(
            turns.iter().map(|turn| turn.role).collect::<Vec<_>>(),
            vec![ReferenceTurnRole::User, ReferenceTurnRole::Assistant, ReferenceTurnRole::User]
        );
        assert_eq!(turns[0].parts, vec![ReferencePart::Text { text: "run the checks".into(), phase: None }]);
        assert_eq!(turns[1].parts[0], ReferencePart::Thinking { text: "thinking about it".into() });
        assert_eq!(turns[1].parts[1], ReferencePart::Text { text: "Starting now.".into(), phase: None });
        // the failed request is an error line, not an empty answer
        assert_eq!(turns[1].parts[2], ReferencePart::Text { text: "Error: 401 Authentication Failed".into(), phase: None });
        assert_eq!(turns[2].parts, vec![ReferencePart::Text { text: "retry".into(), phase: None }]);
    }

    #[test]
    fn context_clear_resets_and_a_compaction_folds() {
        // C3: everything recorded before the reset is gone, and the compaction says where
        // the conversation was folded.
        let turns = parse(RESET);
        assert_eq!(
            turns.iter().map(|turn| turn.role).collect::<Vec<_>>(),
            vec![
                ReferenceTurnRole::User,
                ReferenceTurnRole::Assistant,
                ReferenceTurnRole::User,
                ReferenceTurnRole::User
            ]
        );
        let rendered = serde_json::to_string(&turns).expect("turns serialize");
        assert!(!rendered.contains("first question"), "the reset dropped the turns before it");
        assert!(!rendered.contains("first answer"));
        assert_eq!(turns[0].parts, vec![ReferencePart::Text { text: "after the reset".into(), phase: None }]);
        assert_eq!(turns[1].parts, vec![ReferencePart::Text { text: "fresh answer".into(), phase: None }]);
        assert_eq!(turns[2].parts, vec![ReferencePart::Compact { text: "Folded the earlier debugging session.".into() }]);
        assert_eq!(turns[2].started_at.as_deref(), Some("2026-10-06T11:05:00.000Z"));
        assert_eq!(turns[3].parts, vec![ReferencePart::Text { text: "carry on".into(), phase: None }]);
    }

    #[test]
    fn only_the_gjc_kind_is_answered() {
        // C4: another family's kind is refused, and `Unavailable` is never an empty success.
        for kind in [
            ReferenceNativeHistoryKind::Claude,
            ReferenceNativeHistoryKind::Codex,
            ReferenceNativeHistoryKind::Omp,
            ReferenceNativeHistoryKind::Omo,
            ReferenceNativeHistoryKind::Pi,
            ReferenceNativeHistoryKind::Unavailable,
        ] {
            let refused = parse_reference_gjc(kind, NORMAL);
            assert!(refused.is_err(), "{} must not be answered by the gjc family", kind.as_str());
            assert!(refused.unwrap_err().contains(kind.as_str()));
        }
        let answered = parse_reference_gjc(ReferenceNativeHistoryKind::Gjc, NORMAL).expect("gjc reads its own store");
        assert_eq!(answered.len(), 4);
        assert!(REFERENCE_GJC_PARSER(ReferenceNativeHistoryKind::Gjc, NORMAL).is_ok());
        assert!(REFERENCE_GJC_PARSER(ReferenceNativeHistoryKind::Unavailable, "").is_err());
        // a genuinely empty file is an empty conversation, which is not a refusal
        assert!(parse_reference_gjc(ReferenceNativeHistoryKind::Gjc, "").expect("empty reads").is_empty());
    }

    #[test]
    fn no_abandoned_branches_and_no_tool_images_are_ever_produced() {
        // C4: gjc keeps no entry tree, and its page carries no image ref.
        for text in [NORMAL, MALFORMED, RESET, BOUNDARY] {
            for turn in parse(text) {
                assert!(turn.abandoned.is_none(), "gjc discloses no abandoned branches");
                for part in parts_of(&turn) {
                    assert!(!matches!(part, ReferencePart::Image { .. }), "no image part for gjc");
                    if let ReferencePart::Tool { images, .. } = part {
                        assert!(images.is_empty(), "no tool images for gjc");
                    }
                }
            }
        }
    }

    #[test]
    fn a_notice_is_a_user_seat_turn_and_omo_wakes_are_not_invented() {
        // C4: gjc's own background-job result reads; OmO's task wake belongs to task 19.
        let turns = parse(BOUNDARY);
        assert_eq!(
            turns[0].parts,
            vec![ReferencePart::Notice {
                text: "Background job bg_1 has completed.\nPASS all".into(),
                source: Some("async-result".into())
            }]
        );
        assert_eq!(turns[0].role, ReferenceTurnRole::User);
        let rendered = serde_json::to_string(&turns).expect("turns serialize");
        assert!(!rendered.contains("omo-senpi:wake"), "OmO's wake is the omo family's");
        assert!(!rendered.contains("senpi-monitor:notification"), "a hidden wake is not a turn");
        assert!(!rendered.contains("taskResult"), "no task card is invented here");
    }

    #[test]
    fn a_hidden_record_and_an_unknown_custom_record_are_not_turns() {
        // C4: display:false and an unrecognized `custom` record are chrome.
        let text = [
            r#"{"type":"message","display":false,"message":{"role":"user","content":[{"type":"text","text":"hidden"}]}}"#,
            r#"{"type":"custom","customType":"workflow-intent-diff","data":{"route":"direct"}}"#,
            r#"{"type":"custom_message","display":true,"content":"<system-notice>shown</system-notice>"}"#,
        ]
        .join("\n");
        let turns = parse(&text);
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].parts, vec![ReferencePart::Notice { text: "shown".into(), source: None }]);
    }

    #[test]
    fn an_unanswered_tool_call_stays_in_its_turn() {
        // C4: a call whose result never arrived is still a row, with no output and no ref.
        let turns = parse(BOUNDARY);
        let assistant = turns
            .iter()
            .find(|turn| turn.parts.iter().any(|part| matches!(part, ReferencePart::Tool { .. })))
            .expect("the assistant turn holding the call");
        let ReferencePart::Tool { summary, output, output_ref, output_size, .. } =
            tool_part(assistant, "bash")
        else {
            unreachable!("matched above")
        };
        assert_eq!(summary, "Reading hosts");
        assert_eq!(output, "");
        assert_eq!(*output_ref, None);
        assert_eq!(*output_size, None);
    }

    #[test]
    fn a_skill_invocation_prompt_keeps_its_request_and_chips_the_skill() {
        // The reference turns a skill invocation into the request the person typed plus one chip
        // per skill the runtime loaded (`transcript-records.ts:259`); the tens of KB of SKILL.md
        // the envelope carried are neither the user's text nor a part.
        let turns = parse(BOUNDARY);
        let user = turns
            .iter()
            .find(|turn| {
                matches!(
                    turn.parts.as_slice(),
                    [ReferencePart::Text { .. }, ReferencePart::Skill { .. }]
                )
            })
            .expect("the user turn holding the prompt and its chip");
        // the boundary fixture's envelope carries no <user-request>: the runtime's own name for
        // the skill is the text
        assert_eq!(
            user.parts[0],
            ReferencePart::Text { text: "/skill:delegate-web".into(), phase: None }
        );
        let ReferencePart::Skill { skill } = &user.parts[1] else {
            panic!("the second part is the skill chip");
        };
        assert_eq!(skill.name, "delegate-web");
        assert_eq!(skill.evidence, ReferenceSkillEvidence::Instructions);
        assert_eq!(skill.status, ReferenceSkillStatus::Loaded);
        assert_eq!(
            skill.path.as_deref(),
            Some("/home/dev/.agents/skills/delegate-web/SKILL.md")
        );
        assert_eq!(user.parts.len(), 2, "the request and the chip, nothing else");
        let rendered = serde_json::to_string(&turns).expect("turns serialize");
        assert!(
            !rendered.contains("<skill-instruction"),
            "the envelope is not the user's text"
        );
        let chips = turns
            .iter()
            .flat_map(|turn| turn.parts.iter())
            .filter(|part| matches!(part, ReferencePart::Skill { .. }))
            .count();
        assert_eq!(chips, 1, "one invocation, one chip");

        // the reader itself: an envelope with a <user-request> keeps the request under it
        let envelope = "The user explicitly invoked the \"release\" skill. Follow the instructions in <skill-instruction> as binding for this request, while respecting higher-priority instructions.\n\n<skill-instruction name=\"release\" location=\"/home/dev/.agents/skills/release/SKILL.md\">\n# release\nsteps\n</skill-instruction>\n\n<user-request>\nship 2026.10.7\n</user-request>";
        let invocation = gjc_skill_invocation(envelope).expect("the envelope is a skill invocation");
        assert_eq!(invocation.request, "ship 2026.10.7");
        assert_eq!(invocation.skills.len(), 1);
        assert_eq!(invocation.skills[0].name, "release");
        assert_eq!(
            invocation.skills[0].path.as_deref(),
            Some("/home/dev/.agents/skills/release/SKILL.md")
        );
        assert_eq!(invocation.skills[0].evidence, ReferenceSkillEvidence::Instructions);
        assert_eq!(invocation.skills[0].status, ReferenceSkillStatus::Loaded);

        // the legacy form, and prose that merely mentions a skill
        let legacy = "<skill name=\"tidy\" location=\"tidy.md\">\nbody\n</skill>\n\ndo it";
        let invocation = gjc_skill_invocation(legacy).expect("the legacy envelope is a skill invocation");
        assert_eq!(invocation.skills[0].name, "tidy");
        assert_eq!(invocation.request, "do it");
        assert!(gjc_skill_invocation("please use the frontend skill").is_none());
    }

    #[test]
    fn a_cut_tool_output_keeps_its_ref_and_length() {
        // C1: past 4000 characters the page carries a cut output and what it takes to fetch
        // the rest; the goal tools keep theirs whole to 16000.
        let big = "x".repeat(5_000);
        let goal = "y".repeat(5_000);
        let text = [
            r#"{"type":"message","message":{"role":"assistant","content":[{"type":"toolCall","id":"c1","name":"bash","arguments":{"command":"cat big"},"intent":"Reading it"}]}}"#.to_string(),
            format!(r#"{{"type":"message","message":{{"role":"toolResult","toolCallId":"c1","content":[{{"type":"text","text":"{big}"}}]}}}}"#),
            r#"{"type":"message","message":{"role":"assistant","content":[{"type":"toolCall","id":"c2","name":"create_goal","arguments":{"objective":"long"},"intent":"Setting a goal"}]}}"#.to_string(),
            format!(r#"{{"type":"message","message":{{"role":"toolResult","toolCallId":"c2","content":[{{"type":"text","text":"{goal}"}}]}}}}"#),
        ]
        .join("\n");
        let turns = parse(&text);
        let ReferencePart::Tool { output, output_ref, output_size, .. } = tool_part(&turns[0], "bash") else {
            unreachable!("matched above")
        };
        assert_eq!(output.chars().count(), REFERENCE_GJC_TOOL_OUTPUT_CHARS + "\n… trimmed".chars().count());
        assert!(output.ends_with("\n… trimmed"));
        assert_eq!(output_ref.as_deref(), Some("c1"));
        assert_eq!(*output_size, Some(5_000));
        let ReferencePart::Tool { output, output_ref, .. } = tool_part(&turns[0], "create_goal") else {
            unreachable!("matched above")
        };
        assert_eq!(output.chars().count(), 5_000, "a goal's answer stays whole under 16000");
        assert_eq!(*output_ref, None);
        // an output exactly at the limit is not cut
        let at_limit = "z".repeat(REFERENCE_GJC_TOOL_OUTPUT_CHARS);
        let exact = [
            r#"{"type":"message","message":{"role":"assistant","content":[{"type":"toolCall","id":"c3","name":"read","arguments":{},"intent":"Reading"}]}}"#.to_string(),
            format!(r#"{{"type":"message","message":{{"role":"toolResult","toolCallId":"c3","content":[{{"type":"text","text":"{at_limit}"}}]}}}}"#),
        ]
        .join("\n");
        let exact_turns = parse(&exact);
        let ReferencePart::Tool { output, output_ref, .. } = tool_part(&exact_turns[0], "read") else {
            unreachable!("matched above")
        };
        assert_eq!(output.chars().count(), REFERENCE_GJC_TOOL_OUTPUT_CHARS);
        assert_eq!(*output_ref, None);
    }

    #[test]
    fn a_failed_tool_call_is_marked_on_the_call_it_answers() {
        // C2/C4: isError on the result marks the call, and a result for a call this page
        // does not hold is dropped rather than attached to another.
        let text = [
            r#"{"type":"message","message":{"role":"assistant","content":[{"type":"toolCall","id":"c1","name":"bash","arguments":{"command":"false"},"intent":"Checking"}]}}"#,
            r#"{"type":"message","message":{"role":"toolResult","toolCallId":"c1","isError":true,"content":[{"type":"text","text":"exit 1"}]}}"#,
            r#"{"type":"message","message":{"role":"toolResult","toolCallId":"ghost","content":[{"type":"text","text":"nobody asked"}]}}"#,
        ]
        .join("\n");
        let turns = parse(&text);
        assert_eq!(turns.len(), 1);
        let ReferencePart::Tool { output, error, .. } = tool_part(&turns[0], "bash") else {
            unreachable!("matched above")
        };
        assert_eq!(output, "exit 1");
        assert_eq!(*error, Some(true));
    }

    #[test]
    fn a_tool_result_beside_its_call_is_folded() {
        // C1: a provider may place the result in the same assistant record.
        let text = [
            r#"{"type":"message","message":{"role":"assistant","stopReason":"stop","content":[{"type":"toolCall","id":"c1","name":"bash","arguments":{"command":"ls"},"intent":"Listing"},{"type":"toolResult","toolCallId":"c1","content":[{"type":"text","text":"a.ts"}]}]}}"#,
        ]
        .join("\n");
        let turns = parse(&text);
        let ReferencePart::Tool { output, .. } = tool_part(&turns[0], "bash") else {
            unreachable!("matched above")
        };
        assert_eq!(output, "a.ts");
    }

    #[test]
    fn an_answer_that_stopped_for_good_ends_its_turn() {
        // C1: the record after a `stop` opens a turn of its own, so the answer before a
        // hidden wake-up's work stays the answer.
        let text = [
            r#"{"type":"message","message":{"role":"user","content":[{"type":"text","text":"review it"}]}}"#,
            r#"{"type":"message","message":{"role":"assistant","stopReason":"toolUse","content":[{"type":"toolCall","id":"c1","name":"read","arguments":{"path":"a.ts"},"intent":"Reading"}]}}"#,
            r#"{"type":"message","message":{"role":"toolResult","toolCallId":"c1","content":[{"type":"text","text":"ok"}]}}"#,
            r#"{"type":"message","message":{"role":"assistant","stopReason":"stop","content":[{"type":"thinking","thinking":"done"},{"type":"text","text":"The full review."}]}}"#,
            r#"{"type":"custom_message","customType":"senpi-monitor:notification","display":false,"content":"<system-reminder>READY</system-reminder>"}"#,
            r#"{"type":"message","message":{"role":"assistant","stopReason":"stop","content":[{"type":"text","text":"Nothing new to do."}]}}"#,
        ]
        .join("\n");
        let turns = parse(&text);
        assert_eq!(
            turns.iter().map(|turn| turn.role).collect::<Vec<_>>(),
            vec![ReferenceTurnRole::User, ReferenceTurnRole::Assistant, ReferenceTurnRole::Assistant]
        );
        // transcript-records.ts:184-190, :296 - a turn closes only after a stop message, so the
        // content of that stop message still belongs to the turn it closes: [Tool, Thinking, Text].
        assert_eq!(turns[1].parts.len(), 3);
        assert!(matches!(&turns[1].parts[0], ReferencePart::Tool { name, .. } if name.as_str() == "read"));
        assert_eq!(turns[1].parts[1], ReferencePart::Thinking { text: "done".into() });
        assert_eq!(
            turns[1].parts[2],
            ReferencePart::Text { text: "The full review.".into(), phase: None }
        );
        assert_eq!(turns[2].parts, vec![ReferencePart::Text { text: "Nothing new to do.".into(), phase: None }]);
    }

    #[test]
    fn a_long_transcript_keeps_every_turn_because_the_page_bounds_it() {
        // C4: the pinned page reader passes Infinity, so this family never drops turns of the
        // text it is handed; a cap here would silently lose turns between pages.
        let count = REFERENCE_GJC_MAX_TURNS + 5;
        let text = (0..count)
            .map(|n| format!(r#"{{"type":"message","message":{{"role":"user","content":[{{"type":"text","text":"m{n}"}}]}}}}"#))
            .collect::<Vec<_>>()
            .join("\n");
        let turns = parse(&text);
        assert_eq!(turns.len(), count);
        assert_eq!(turns[0].parts, vec![ReferencePart::Text { text: "m0".into(), phase: None }]);
        assert_eq!(
            turns[count - 1].parts,
            vec![ReferencePart::Text { text: format!("m{}", count - 1), phase: None }]
        );
    }

    #[test]
    fn the_manifest_inventory_matches_the_module() {
        // C4: every named record variant is inventoried, with no unexplained skip, and the
        // fixture manifest is the machine-readable copy of the module's own list.
        let manifest: Value = serde_json::from_str(MANIFEST).expect("the manifest parses");
        let variants = manifest.get("variants").and_then(Value::as_array).expect("variants listed");
        let names: Vec<&str> = variants
            .iter()
            .map(|variant| variant.get("name").and_then(Value::as_str).expect("a named variant"))
            .collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len(), "no variant is listed twice");
        let mut expected = REFERENCE_GJC_RECORD_VARIANTS.to_vec();
        expected.sort_unstable();
        assert_eq!(sorted, expected, "the manifest and the module inventory are the same set");
        for variant in variants {
            assert!(
                variant.get("effect").and_then(Value::as_str).is_some_and(|effect| !effect.trim().is_empty()),
                "every variant says what it does"
            );
            assert!(variant.get("handled").and_then(Value::as_bool).is_some(), "every variant is classified");
        }
        // no unexplained skip: every inventoried variant is exercised by a named fixture
        let exercised: std::collections::BTreeSet<&str> = manifest
            .get("fixtures")
            .and_then(Value::as_array)
            .expect("fixtures listed")
            .iter()
            .flat_map(|fixture| {
                fixture
                    .get("exercises")
                    .and_then(Value::as_array)
                    .expect("every fixture says which variants it exercises")
                    .iter()
                    .map(|name| name.as_str().expect("a named variant"))
                    .collect::<Vec<_>>()
            })
            .collect();
        for name in &names {
            assert!(exercised.contains(name), "{name} is inventoried but no fixture exercises it");
        }
        for name in &exercised {
            assert!(names.contains(name), "{name} is exercised by a fixture but not inventoried");
        }
        let files: Vec<&str> = manifest
            .get("fixtures")
            .and_then(Value::as_array)
            .expect("fixtures listed")
            .iter()
            .map(|fixture| fixture.get("file").and_then(Value::as_str).expect("a named fixture"))
            .collect();
        assert_eq!(files, vec!["normal.jsonl", "malformed.jsonl", "reset.jsonl", "boundary.jsonl"]);
        assert_eq!(
            manifest.get("reader").and_then(Value::as_str),
            Some("gjc-transcript"),
            "the manifest names the reader this module implements"
        );
    }
}
