//! omp native history family (plan task 18).
//!
//! Ported from `devswha/herdr-web-ui` @ `54e5a1f67090cb09552d182e7e30dd0ecc314918` (MIT,
//! see `docs/chat/HERDR_LICENSE`): `server/transcript-records.ts` `parseOmpTranscript`
//! (lines 175-301) plus the helpers it calls — `isContextClear`, `piMessage`, `piResults`,
//! `piNotice`, `omoTaskTitles`, `omoTaskResults`, `toolSummary` (same file),
//! `trimOutput` (`server/tool-output.ts`) and `skillInvocationPrompt`
//! (`server/skill-activity.ts`).
//!
//! Frozen contract: `docs/chat/herdr-port-contract.md` section 6. The lane satisfies the
//! frozen [`ReferenceHistoryParser`] alias; the dispatcher (task 2) owns the byte-level
//! entry point and calls [`parse_omp_transcript`] for
//! [`ReferenceNativeHistoryKind::Omp`].
//!
//! omp hands the reader the absolute session jsonl path under
//! `~/.omp/agent/sessions/<cwd-slug>/` (`conversation.ts:9-10,769-786`). Resolving that
//! path is task 3's job; this module only turns the bytes it is handed into
//! [`ReferenceTurn`]s. omp, omo and gjc write the *same* record shape
//! (`conversation.ts:15-18`), which is why tasks 19 and 20 port the same rules in their own
//! files rather than sharing this one.
//!
//! Boundaries this lane keeps, each a fact read from the pinned source:
//!
//! * `toolImages` stays **off**. Only pi keeps a tool result's images as inline base64
//!   (`conversation.ts:571-574`), so an omp tool part never carries image refs.
//! * `invokedSkill` is not called. The pinned `parseOmpTranscript` does not call it; that
//!   belongs to another family's branch (`conversation.ts:214`).
//! * A skill-invocation prompt keeps the *request* as the user's text, exactly as upstream
//!   does (`transcript-records.ts:257-259`), and the skills the runtime loaded ride it as their
//!   own parts.
//!
//! ## The skill part
//!
//! Upstream pushes `{ kind: "skill", skill }` parts beside that text
//! (`transcript-records.ts:259`), one per skill the prompt invoked, in the order the runtime
//! named them. They reach the page as [`ReferencePart::Skill`]. The turn's skill list draws
//! them; the inline part list does not (`ChatView.tsx:322`).
//!
//! ## Known cosmetic divergence
//!
//! A tool call's `input` is `JSON.stringify(arguments, null, 2)` upstream, so its key order
//! is the record's insertion order. This port serializes through `serde_json`, whose object
//! order depends on whether the build enables `serde_json/preserve_order`; without it the
//! keys are sorted. Only the displayed tool-input text is affected, never a parsed field.

use std::collections::HashMap;
use std::sync::OnceLock;

use regex::Regex;
use serde_json::{Map, Value};

use super::types::{
    ReferenceHistoryParser, ReferenceImageRef, ReferenceNativeHistoryKind, ReferencePart,
    ReferenceSkillActivity, ReferenceSkillEvidence, ReferenceSkillStatus, ReferenceTaskResult,
    ReferenceTaskStatus, ReferenceTurn, ReferenceTurnRole,
};

/// The family this lane reads. A lane handles exactly one kind; `Unavailable` is an error,
/// never an empty success (`docs/chat/herdr-port-contract.md` section 6).
pub const OMP_HISTORY_KIND: ReferenceNativeHistoryKind = ReferenceNativeHistoryKind::Omp;

/// Upstream's default turn bound (`transcript-records.ts:78`). The page reader passes
/// `Infinity` (`conversation.ts:574`), which is what [`parse_omp_transcript`] mirrors.
pub const OMP_MAX_TURNS: usize = 100;

/// Past this a tool's output is cut in the page; the rest is fetched on request
/// (`tool-output.ts:4`).
const TOOL_OUTPUT_CHARS: usize = 4_000;

/// OmO's goal calls answer with JSON the chat reads whole, up to this (`tool-output.ts:11`).
const WHOLE_OUTPUT_CHARS: usize = 16_000;

/// The tools whose output is kept whole (`tool-output.ts:10`).
const WHOLE_OUTPUT_TOOLS: [&str; 3] = ["create_goal", "update_goal", "get_goal"];

/// One task result's retained answer (`transcript-records.ts:93`).
const TASK_RESULT_MAX: usize = 16_000;

/// omp writes no inline tool images; only pi does (`conversation.ts:571-574`).
const TOOL_IMAGES: bool = false;

/// The skill evidence a user prompt carried, when the prompt was a skill invocation.
///
/// [`parse_omp_transcript`] pushes one [`ReferencePart::Skill`] per skill here, beside the
/// request text, exactly as upstream does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OmpSkillInvocation {
    /// The skills the runtime loaded before the user's request, in the order it named them.
    pub skills: Vec<ReferenceSkillActivity>,
    /// What the user actually asked, after the injected instruction envelope.
    pub request: String,
}

/// Turn one omp session jsonl into turns, unbounded — the page reader's own call
/// (`conversation.ts:574` passes `Infinity`).
///
/// `kind` must be [`ReferenceNativeHistoryKind::Omp`]; anything else is an error, because
/// an empty success would be read as "this session has no turns".
pub fn parse_omp_transcript(
    kind: ReferenceNativeHistoryKind,
    text: &str,
) -> Result<Vec<ReferenceTurn>, String> {
    parse_omp_transcript_limited(kind, text, None)
}

/// As [`parse_omp_transcript`], with upstream's `maxTurns` bound applied.
///
/// `None` is unbounded (the page reader). `Some(n)` keeps the newest `n` turns, exactly
/// like upstream's `slice(-maxTurns)` (`transcript-records.ts:300`) — including its quirk
/// that `Some(0)` keeps everything, because `slice(-0)` is `slice(0)`.
pub fn parse_omp_transcript_limited(
    kind: ReferenceNativeHistoryKind,
    text: &str,
    max_turns: Option<usize>,
) -> Result<Vec<ReferenceTurn>, String> {
    if kind != OMP_HISTORY_KIND {
        return Err(format!(
            "history_omp reads the omp family only; got {}",
            kind.as_str()
        ));
    }
    Ok(parse_records(text, max_turns))
}

/// The skills a user prompt's instruction envelope named, and the request under it.
///
/// Mirrors `skillInvocationPrompt` (`skill-activity.ts:41-63`): chained
/// `<skill-instruction>` envelopes, then `<user-request>`, or the legacy
/// `<skill name location>` form. Anything else is `None` and the prompt stands as the
/// user's own text.
pub fn omp_skill_invocation(prompt: &str) -> Option<OmpSkillInvocation> {
    let pattern = skill_instruction_pattern();
    let request_pattern = user_request_pattern();
    let legacy_pattern = legacy_skill_pattern();

    let mut skills: Vec<ReferenceSkillActivity> = Vec::new();
    let mut remainder = prompt;
    let mut matched = pattern.captures(remainder);

    while let Some(captures) = matched {
        let name = captures.get(1).map(|group| group.as_str()).unwrap_or_default();
        let declared = captures.get(2).map(|group| group.as_str()).unwrap_or_default();
        let location = captures.get(3).map(|group| group.as_str()).unwrap_or_default();
        // the envelope must agree with itself about which skill it injected
        let skill = if name == declared {
            loaded_skill(name, location)
        } else {
            None
        };
        let Some(skill) = skill else { return None };
        skills.push(skill);
        remainder = &remainder[captures.get(0)?.as_str().len()..];
        if !remainder.starts_with("\n\nThe user explicitly invoked the ") {
            break;
        }
        remainder = &remainder[2..];
        matched = pattern.captures(remainder);
        if matched.is_none() {
            return None;
        }
    }

    if !skills.is_empty() {
        if remainder.is_empty() {
            return Some(OmpSkillInvocation { skills, request: String::new() });
        }
        let request = request_pattern.captures(remainder)?;
        let request = request.get(1).map(|group| group.as_str().trim().to_string())?;
        return Some(OmpSkillInvocation { skills, request });
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
    Some(OmpSkillInvocation { skills: vec![skill], request })
}

/// Compile-time proof that this lane matches the frozen lane signature.
const _: ReferenceHistoryParser = parse_omp_transcript;

/// One tool result, as the record carries it.
struct PiToolResult {
    id: String,
    text: String,
    error: bool,
    /// The media types of the inline images the result held; omp never uses them.
    images: Vec<String>,
}

/// The parser's running state: the turns so far, the tool calls still waiting for their
/// result, OmO's task titles, and whether the last assistant message stopped for good.
#[derive(Default)]
struct OmpState {
    turns: Vec<ReferenceTurn>,
    /// tool call id -> (turn index, part index)
    pending: HashMap<String, (usize, usize)>,
    task_titles: HashMap<String, String>,
    settled: bool,
}

fn parse_records(text: &str, max_turns: Option<usize>) -> Vec<ReferenceTurn> {
    let mut state = OmpState::default();

    for line in text.split('\n') {
        if line.trim().is_empty() {
            continue;
        }
        // A torn tail line while omp is mid-append is skipped, not fatal.
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let entry = match &value {
            Value::Object(entry) => entry,
            _ => continue,
        };
        let timestamp = entry.get("timestamp").and_then(Value::as_str);

        if is_context_clear(&value) {
            state.turns.clear();
            state.pending.clear();
            continue;
        }

        if let Some(notice) = pi_notice(entry) {
            state.turns.push(user_turn(timestamp, vec![notice]));
            continue;
        }

        if let Some(tasks) = omo_task_results(entry, &state.task_titles) {
            state
                .turns
                .push(user_turn(timestamp, vec![ReferencePart::TaskResult { tasks }]));
            continue;
        }

        // A compaction entry is a tree entry, not a message, so it reaches the chat only
        // through this branch: the card says where the conversation was cut.
        if entry.get("type").and_then(Value::as_str) == Some("compaction") {
            let summary = entry.get("summary").and_then(Value::as_str).unwrap_or_default();
            if !summary.trim().is_empty() {
                state
                    .turns
                    .push(user_turn(timestamp, vec![ReferencePart::Compact { text: summary.to_string() }]));
            }
            continue;
        }

        let Some(message) = pi_message(entry) else {
            continue;
        };
        if message.get("role").and_then(Value::as_str) != Some("assistant") {
            apply_results(&mut state, &message);
        }
        omo_task_titles(&message, &mut state.task_titles);

        if message.get("role").and_then(Value::as_str) == Some("user") {
            let prompt = user_prompt(&message);
            // image-only user parts have no text to show
            if prompt.is_empty() {
                continue;
            }
            let invocation = omp_skill_invocation(&prompt);
            let asked = match &invocation {
                None => prompt,
                Some(invocation) => {
                    if invocation.request.is_empty() {
                        invocation
                            .skills
                            .iter()
                            .map(|skill| format!("/skill:{}", skill.name))
                            .collect::<Vec<_>>()
                            .join(" ")
                    } else {
                        invocation.request.clone()
                    }
                }
            };
            // The request is the user's text, and each skill the runtime loaded rides it as its
            // own part, in the order the envelope named them (`transcript-records.ts:259`).
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
            state.turns.push(user_turn(timestamp, parts));
            continue;
        }

        if message.get("role").and_then(Value::as_str) == Some("assistant") {
            let Some(blocks) = message.get("content").and_then(Value::as_array) else {
                continue;
            };
            let turn_index = assistant_turn_index(&mut state, timestamp);
            if let Some(timestamp) = timestamp {
                // last recorded assistant activity, never the next user's timestamp
                state.turns[turn_index].ended_at = Some(timestamp.to_string());
            }
            for block in blocks {
                let block = record(block);
                match block.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        let text = block.get("text").and_then(Value::as_str).unwrap_or_default();
                        if !text.is_empty() {
                            state.turns[turn_index].parts.push(ReferencePart::Text {
                                text: text.to_string(),
                                phase: None,
                            });
                        }
                    }
                    Some("thinking") => {
                        let thinking = block
                            .get("thinking")
                            .and_then(Value::as_str)
                            .or_else(|| block.get("text").and_then(Value::as_str))
                            .unwrap_or_default();
                        if !thinking.is_empty() {
                            state.turns[turn_index]
                                .parts
                                .push(ReferencePart::Thinking { text: thinking.to_string() });
                        }
                    }
                    Some("toolCall") => {
                        let Some(name) = block.get("name").and_then(Value::as_str) else {
                            continue;
                        };
                        let arguments = block
                            .get("arguments")
                            .filter(|value| value.is_object() || value.is_array())
                            .cloned()
                            .unwrap_or_else(|| Value::Object(Map::new()));
                        let input = arguments.as_object().cloned().unwrap_or_default();
                        let summary = block
                            .get("intent")
                            .and_then(Value::as_str)
                            .filter(|intent| !intent.is_empty())
                            .map(str::to_string)
                            .unwrap_or_else(|| tool_summary(name, &input));
                        let summary = summary.chars().take(120).collect::<String>();
                        state.turns[turn_index].parts.push(ReferencePart::Tool {
                            name: name.to_string(),
                            summary,
                            input: serde_json::to_string_pretty(&arguments).unwrap_or_default(),
                            output: String::new(),
                            error: None,
                            // `invokedSkill` belongs to another family's branch
                            // (`conversation.ts:214`); the omp parser does not call it.
                            skill: None,
                            output_ref: None,
                            output_size: None,
                            images: Vec::new(),
                        });
                        if let Some(id) = block.get("id").and_then(Value::as_str) {
                            let part_index = state.turns[turn_index].parts.len() - 1;
                            state.pending.insert(id.to_string(), (turn_index, part_index));
                        }
                    }
                    // unsupported transcript parts are intentionally ignored
                    _ => {}
                }
            }
            apply_results(&mut state, &message);
            // a failed request (a 401, an overloaded provider) leaves an empty message:
            // without its error the chat showed the prompt with no answer at all
            if message.get("stopReason").and_then(Value::as_str) == Some("error") {
                if let Some(error) = message
                    .get("errorMessage")
                    .and_then(Value::as_str)
                    .filter(|error| !error.is_empty())
                {
                    state.turns[turn_index].parts.push(ReferencePart::Text {
                        text: format!("Error: {error}"),
                        phase: None,
                    });
                }
            }
            // a message that stopped for good ends its turn: the next one was woken by
            // something nobody typed, such as a hidden monitor or a task notification
            state.settled = message.get("stopReason").and_then(Value::as_str) == Some("stop");
        }
    }

    let mut turns: Vec<ReferenceTurn> = state
        .turns
        .into_iter()
        .filter(|turn| !turn.parts.is_empty())
        .collect();
    if let Some(max_turns) = max_turns.filter(|max_turns| *max_turns > 0) {
        if turns.len() > max_turns {
            turns = turns.split_off(turns.len() - max_turns);
        }
    }
    turns
}

/// The assistant turn this message belongs to: the last one, while it is still open.
fn assistant_turn_index(state: &mut OmpState, timestamp: Option<&str>) -> usize {
    if let Some(last) = state.turns.last() {
        if last.role == ReferenceTurnRole::Assistant && !state.settled {
            return state.turns.len() - 1;
        }
    }
    state.turns.push(ReferenceTurn {
        role: ReferenceTurnRole::Assistant,
        started_at: timestamp.map(str::to_string),
        ended_at: None,
        source: None,
        parts: Vec::new(),
        abandoned: None,
    });
    state.turns.len() - 1
}

/// A turn in the user's seat that nobody typed: a notice, a task result, a compaction, or the
/// request text a skill invocation carried plus one chip per skill it named.
fn user_turn(timestamp: Option<&str>, parts: Vec<ReferencePart>) -> ReferenceTurn {
    ReferenceTurn {
        role: ReferenceTurnRole::User,
        started_at: timestamp.map(str::to_string),
        ended_at: None,
        source: None,
        parts,
        abandoned: None,
    }
}

/// Hand every result this message carries to the tool call still waiting for it.
fn apply_results(state: &mut OmpState, message: &Map<String, Value>) {
    for result in pi_results(message) {
        let Some(position) = state.pending.get(&result.id).copied() else {
            continue;
        };
        state.pending.remove(&result.id);
        let (turn_index, part_index) = position;
        let part = state
            .turns
            .get_mut(turn_index)
            .and_then(|turn| turn.parts.get_mut(part_index));
        if let Some(ReferencePart::Tool {
            name,
            output,
            error,
            output_ref,
            output_size,
            images,
            ..
        }) = part
        {
            trim_output(
                name.as_str(),
                output,
                output_ref,
                output_size,
                &result.text,
                &result.id,
            );
            if result.error {
                *error = Some(true);
            }
            if TOOL_IMAGES && !result.images.is_empty() {
                // ported verbatim, `transcript-records.ts:237-240`: addressed by the call it
                // answers, because a nested result shares its entry with other blocks
                *images = result
                    .images
                    .iter()
                    .enumerate()
                    .map(|(index, media_type)| ReferenceImageRef {
                        media_type: media_type.clone(),
                        r#ref: format!("pi:{}:{index}", result.id),
                    })
                    .collect();
            }
        }
    }
}

/// Sets a tool part's output, cut to what a page carries, keeping what it takes to fetch
/// the rest (`tool-output.ts:14-20`).
fn trim_output(
    name: &str,
    output: &mut String,
    output_ref: &mut Option<String>,
    output_size: &mut Option<u64>,
    text: &str,
    reference: &str,
) {
    let limit = if WHOLE_OUTPUT_TOOLS.contains(&name) {
        WHOLE_OUTPUT_CHARS
    } else {
        TOOL_OUTPUT_CHARS
    };
    let length = text.chars().count();
    if length <= limit {
        *output = text.to_string();
        return;
    }
    let kept: String = text.chars().take(limit).collect();
    *output = format!("{kept}\n… trimmed");
    *output_ref = Some(reference.to_string());
    *output_size = Some(length as u64);
}

/// omp-family providers spell the same tool call/result fields several ways
/// (`transcript-records.ts:23-35`).
fn pi_message(entry: &Map<String, Value>) -> Option<Map<String, Value>> {
    if entry.get("type").and_then(Value::as_str) != Some("message") {
        return None;
    }
    let message = record_of(entry.get("message"));
    if message.get("display").and_then(Value::as_bool) == Some(false) {
        return None;
    }
    if entry.get("display").and_then(Value::as_bool) == Some(false) {
        return None;
    }

    let raw: Vec<Value> = match message.get("content") {
        Some(Value::String(text)) => vec![serde_json::json!({"type": "text", "text": text})],
        Some(Value::Array(blocks)) => blocks.clone(),
        _ => Vec::new(),
    };

    let content: Vec<Value> = raw
        .into_iter()
        .map(|value| {
            let block = record(&value);
            match block.get("type").and_then(Value::as_str) {
                Some("toolCall") => {
                    let mut normalized = block.clone();
                    if let Some(name) =
                        string_of(&[block.get("toolName"), block.get("name")])
                    {
                        normalized.insert("name".to_string(), Value::String(name));
                    }
                    if let Some(id) = string_of(&[
                        block.get("toolCallId"),
                        block.get("id"),
                        block.get("callId"),
                    ]) {
                        normalized.insert("id".to_string(), Value::String(id));
                    }
                    if let Some(arguments) = block
                        .get("toolInput")
                        .or_else(|| block.get("input"))
                        .or_else(|| block.get("arguments"))
                    {
                        normalized.insert("arguments".to_string(), arguments.clone());
                    }
                    Value::Object(normalized)
                }
                Some("toolResult") => {
                    let mut normalized = block.clone();
                    if let Some(id) = string_of(&[
                        block.get("toolCallId"),
                        block.get("callId"),
                        block.get("id"),
                    ]) {
                        normalized.insert("toolCallId".to_string(), Value::String(id));
                    }
                    if let Some(output) = block
                        .get("output")
                        .or_else(|| block.get("content"))
                        .or_else(|| block.get("result"))
                    {
                        normalized.insert("content".to_string(), output.clone());
                    }
                    Value::Object(normalized)
                }
                _ => value,
            }
        })
        .collect();

    let mut normalized = message.clone();
    if let Some(id) = string_of(&[message.get("toolCallId"), message.get("callId")]) {
        normalized.insert("toolCallId".to_string(), Value::String(id));
    }
    normalized.insert("content".to_string(), Value::Array(content));
    Some(normalized)
}

/// One entry's tool results, with the images each carries (`transcript-records.ts:44-52`).
fn pi_results(message: &Map<String, Value>) -> Vec<PiToolResult> {
    let blocks: Vec<&Map<String, Value>> =
        if message.get("role").and_then(Value::as_str) == Some("toolResult") {
            vec![message]
        } else {
            message
                .get("content")
                .and_then(Value::as_array)
                .map(|blocks| {
                    blocks
                        .iter()
                        .map(record)
                        .filter(|block| {
                            block.get("type").and_then(Value::as_str) == Some("toolResult")
                        })
                        .collect()
                })
                .unwrap_or_default()
        };

    let mut results = Vec::new();
    for block in blocks {
        let Some(id) = block.get("toolCallId").and_then(Value::as_str) else {
            continue;
        };
        let images = block
            .get("content")
            .and_then(Value::as_array)
            .map(|blocks| blocks.iter().filter_map(pi_image_of).collect())
            .unwrap_or_default();
        results.push(PiToolResult {
            id: id.to_string(),
            text: result_text(block.get("content")),
            error: block.get("isError").and_then(Value::as_bool) == Some(true),
            images,
        });
    }
    results
}

/// The image types a chat shows; pi names the type `mimeType` where Claude names it
/// `media_type` (`transcript-records.ts:54-62`).
fn pi_image_of(value: &Value) -> Option<String> {
    let image = record(value);
    if image.get("type").and_then(Value::as_str) != Some("image") {
        return None;
    }
    let media_type = image
        .get("mimeType")
        .and_then(Value::as_str)
        .or_else(|| image.get("media_type").and_then(Value::as_str))?;
    if !matches!(media_type, "image/png" | "image/jpeg" | "image/gif" | "image/webp") {
        return None;
    }
    image.get("data").and_then(Value::as_str)?;
    Some(media_type.to_string())
}

/// gjc wakes the agent with a `custom_message` in the user's seat
/// (`transcript-records.ts:85-91`). The envelope is chrome.
fn pi_notice(entry: &Map<String, Value>) -> Option<ReferencePart> {
    if entry.get("type").and_then(Value::as_str) != Some("custom_message") {
        return None;
    }
    if entry.get("display").and_then(Value::as_bool) == Some(false) {
        return None;
    }
    let content = entry.get("content").and_then(Value::as_str)?;
    let mut text = content.trim();
    if let Some(rest) = text.strip_prefix("<system-notice>") {
        text = rest.trim_start();
    }
    if let Some(rest) = text.strip_suffix("</system-notice>") {
        text = rest.trim_end();
    }
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let source = entry.get("customType").and_then(Value::as_str).map(str::to_string);
    Some(ReferencePart::Notice { text: text.to_string(), source })
}

/// The summaries OmO's `task` calls gave their tasks, by task id
/// (`transcript-records.ts:101-110`).
fn omo_task_titles(message: &Map<String, Value>, titles: &mut HashMap<String, String>) {
    if message.get("role").and_then(Value::as_str) != Some("toolResult") {
        return;
    }
    if message.get("toolName").and_then(Value::as_str) != Some("task") {
        return;
    }
    let details = record_of(message.get("details"));
    let items: Vec<&Map<String, Value>> = match details.get("items").and_then(Value::as_array) {
        Some(items) => items.iter().map(record).collect(),
        None => vec![details],
    };
    for item in items {
        let id = label(item.get("task_id"));
        let title = label(item.get("task_summary")).or_else(|| label(item.get("description")));
        if let (Some(id), Some(title)) = (id, title) {
            titles.insert(id, title);
        }
    }
}

/// One `senpi-task.completion` per task, in the user's seat
/// (`transcript-records.ts:118-150`). A task reported twice in one wake is the later report.
fn omo_task_results(
    entry: &Map<String, Value>,
    titles: &HashMap<String, String>,
) -> Option<Vec<ReferenceTaskResult>> {
    if entry.get("type").and_then(Value::as_str) != Some("custom_message") {
        return None;
    }
    if entry.get("customType").and_then(Value::as_str) != Some("omo-senpi:wake") {
        return None;
    }
    let details = entry.get("details").and_then(Value::as_array)?;

    let mut order: Vec<String> = Vec::new();
    let mut tasks: HashMap<String, ReferenceTaskResult> = HashMap::new();
    for group in details {
        let group = record(group);
        if group.get("customType").and_then(Value::as_str) != Some("senpi-task.completion") {
            continue;
        }
        let Some(inner) = group.get("details").and_then(Value::as_array) else {
            continue;
        };
        for value in inner {
            let task = record(value);
            let Some(id) = label(task.get("task_id")) else {
                continue;
            };
            let Some(status) = label(task.get("status")) else {
                continue;
            };
            let stats = record_of(task.get("run_stats"));
            let agent = label(task.get("agent_type"))
                .or_else(|| label(task.get("category")))
                .or_else(|| label(task.get("subagent_type")));
            let name = label(task.get("name"));
            let result = label(task.get("final_response"))
                .or_else(|| label(task.get("error")))
                .unwrap_or_default();
            let title = titles.get(&id).cloned().unwrap_or_else(|| match (&name, &agent) {
                (Some(name), _) if name != &id => name.clone(),
                (_, Some(agent)) => agent.clone(),
                _ => id.clone(),
            });
            let model = record_of(task.get("resolved_model"))
                .get("display")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| label(task.get("model")));
            let status = match status.as_str() {
                "completed" => ReferenceTaskStatus::Completed,
                "cancelled" | "canceled" | "aborted" => ReferenceTaskStatus::Cancelled,
                _ => ReferenceTaskStatus::Failed,
            };
            let result_length = result.chars().count();
            let cut = result_length > TASK_RESULT_MAX;
            let result = if cut {
                result.chars().take(TASK_RESULT_MAX).collect::<String>()
            } else {
                result
            };
            if !tasks.contains_key(&id) {
                order.push(id.clone());
            }
            tasks.insert(
                id.clone(),
                ReferenceTaskResult {
                    id,
                    title,
                    agent,
                    model,
                    status,
                    duration_ms: amount(task.get("duration_ms"))
                        .or_else(|| amount(stats.get("runtime_ms"))),
                    turns: amount(stats.get("turns")),
                    tool_calls: amount(stats.get("tool_calls")),
                    tokens: amount(stats.get("total_tokens"))
                        .or_else(|| amount(task.get("tokens"))),
                    result,
                    result_cut: cut.then_some(true),
                },
            );
        }
    }

    if tasks.is_empty() {
        return None;
    }
    Some(order.into_iter().filter_map(|id| tasks.remove(&id)).collect())
}

/// The one-line summary a collapsed tool chip shows (`transcript-records.ts:153-163`).
fn tool_summary(name: &str, input: &Map<String, Value>) -> String {
    if name == "task" {
        let items: Vec<&Map<String, Value>> = match input.get("tasks").and_then(Value::as_array) {
            Some(items) => items.iter().map(record).collect(),
            None => vec![input],
        };
        let titles: Vec<String> = items
            .iter()
            .filter_map(|item| {
                label(item.get("task_summary")).or_else(|| label(item.get("description")))
            })
            .collect();
        if !titles.is_empty() {
            return titles.join(" · ").chars().take(120).collect();
        }
    }
    // pi names a file `path` where Claude names it `file_path`, and a notebook
    // `notebook_path`
    let first = ["command", "file_path", "notebook_path", "path", "pattern", "description", "url"]
        .iter()
        .find_map(|key| input.get(*key).filter(|value| !value.is_null()));
    match first.and_then(Value::as_str) {
        Some(value) => value.chars().take(120).collect(),
        None => name.to_string(),
    }
}

/// The prompt a user message carried: a string, or its text blocks joined by newlines
/// (`transcript-records.ts:247-253`).
fn user_prompt(message: &Map<String, Value>) -> String {
    match message.get("content") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .map(record)
            .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .filter(|text| !text.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// A cleared context: omp, omo, gjc and pi all write `custom` / `context_clear`
/// (`transcript-records.ts:12-20`).
fn is_context_clear(value: &Value) -> bool {
    let entry = record(value);
    entry.get("type").and_then(Value::as_str) == Some("custom")
        && entry.get("customType").and_then(Value::as_str) == Some("context_clear")
}

/// A non-empty trimmed string, or nothing (`transcript-records.ts:94`).
fn label(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

/// A finite, non-negative number, as a count (`transcript-records.ts:95`).
fn amount(value: Option<&Value>) -> Option<u64> {
    value
        .and_then(Value::as_f64)
        .filter(|number| number.is_finite() && *number >= 0.0)
        .map(|number| number as u64)
}

/// The first string among the candidate spellings of one field.
fn string_of(values: &[Option<&Value>]) -> Option<String> {
    values.iter().flatten().find_map(|value| value.as_str()).map(str::to_string)
}

/// A tool result's text: a string, or the text blocks it holds (`transcript-records.ts:9-10`).
fn result_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .map(|block| record(block).get("text").and_then(Value::as_str).unwrap_or_default())
            .collect(),
        _ => String::new(),
    }
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

/// `{}` for anything that is not a JSON object (`transcript-records.ts:7`).
fn record(value: &Value) -> &Map<String, Value> {
    record_of(Some(value))
}

fn record_of(value: Option<&Value>) -> &Map<String, Value> {
    static EMPTY: OnceLock<Map<String, Value>> = OnceLock::new();
    let empty = EMPTY.get_or_init(Map::new);
    value.and_then(Value::as_object).unwrap_or(empty)
}

/// `^The user explicitly invoked the "…" skill. …` (`skill-activity.ts:25`).
const SKILL_INSTRUCTION_PATTERN: &str = r#"^The user explicitly invoked the "([^"]+)" skill\. Follow the instructions in <skill-instruction> as binding for this request, while respecting higher-priority instructions\.\n\n<skill-instruction name="([^"]+)" location="([^"]+)">\n[\s\S]*?\n</skill-instruction>"#;

/// The user's own request under the envelopes (`skill-activity.ts:57`).
const USER_REQUEST_PATTERN: &str = r#"^\n\n<user-request>\n([\s\S]*?)\n</user-request>$"#;

/// A standalone `.md` skill (`--skill review.md`), the older form
/// (`skill-activity.ts:26`).
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
    use super::super::types::{
        ReferencePartKind, ReferenceSkillEvidence, ReferenceSkillStatus, ReferenceTaskStatus,
        ReferenceTurnRole,
    };
    use super::*;

    const BASIC_SESSION: &str = include_str!("fixtures/omp/basic-session.jsonl");
    const COMPACTION_AND_CLEAR: &str = include_str!("fixtures/omp/compaction-and-clear.jsonl");
    const TASK_RESULTS: &str = include_str!("fixtures/omp/task-results.jsonl");
    const PARTIAL_AND_MALFORMED: &str = include_str!("fixtures/omp/partial-and-malformed.jsonl");
    const RECORD_ALIASES: &str = include_str!("fixtures/omp/record-aliases.jsonl");
    const SETTLED_TURN_BOUNDARY: &str = include_str!("fixtures/omp/settled-turn-boundary.jsonl");
    const SKILL_INVOCATION: &str = include_str!("fixtures/omp/skill-invocation.jsonl");

    fn parse(fixture: &str) -> Vec<ReferenceTurn> {
        parse_omp_transcript(OMP_HISTORY_KIND, fixture).expect("the omp family parses")
    }

    fn kinds(turn: &ReferenceTurn) -> Vec<ReferencePartKind> {
        turn.parts.iter().map(ReferencePart::kind).collect()
    }

    fn text_of(part: &ReferencePart) -> &str {
        match part {
            ReferencePart::Text { text, .. } => text,
            other => panic!("expected a text part, got {:?}", other.kind()),
        }
    }

    fn tool_of(part: &ReferencePart) -> (&str, &str, &str, &str, bool) {
        match part {
            ReferencePart::Tool { name, summary, input, output, error, .. } => {
                (name, summary, input, output, error == &Some(true))
            }
            other => panic!("expected a tool part, got {:?}", other.kind()),
        }
    }

    /// One `bash` tool call whose result is `output`, as a session jsonl.
    fn tool_call_session(tool: &str, output: &str) -> String {
        format!(
            concat!(
                "{{\"type\":\"message\",\"id\":\"u\",\"message\":{{\"role\":\"user\",",
                "\"content\":[{{\"type\":\"text\",\"text\":\"run it\"}}]}}}}\n",
                "{{\"type\":\"message\",\"id\":\"a\",\"message\":{{\"role\":\"assistant\",",
                "\"stopReason\":\"toolUse\",\"content\":[{{\"type\":\"toolCall\",\"id\":\"c1\",",
                "\"name\":\"{tool}\",\"arguments\":{{\"command\":\"run\"}}}}]}}}}\n",
                "{{\"type\":\"message\",\"id\":\"r\",\"message\":{{\"role\":\"toolResult\",",
                "\"toolCallId\":\"c1\",\"content\":[{{\"type\":\"text\",\"text\":\"{output}\"}}]}}}}\n"
            ),
            tool = tool,
            output = output
        )
    }

    #[test]
    fn omp_session_parses_prompts_text_thinking_and_tool_results() {
        let turns = parse(BASIC_SESSION);
        assert_eq!(turns.len(), 4, "two prompts, two assistant turns");

        assert_eq!(turns[0].role, ReferenceTurnRole::User);
        assert_eq!(turns[0].started_at.as_deref(), Some("2026-10-06T01:00:01.000Z"));
        assert_eq!(turns[0].parts.len(), 1);
        assert_eq!(text_of(&turns[0].parts[0]), "list the files");

        let assistant = &turns[1];
        assert_eq!(assistant.role, ReferenceTurnRole::Assistant);
        assert_eq!(assistant.started_at.as_deref(), Some("2026-10-06T01:00:02.000Z"));
        // the last recorded assistant activity, not the next user's timestamp
        assert_eq!(assistant.ended_at.as_deref(), Some("2026-10-06T01:00:04.000Z"));
        assert_eq!(
            kinds(assistant),
            vec![
                ReferencePartKind::Thinking,
                ReferencePartKind::Text,
                ReferencePartKind::Tool
            ]
        );
        match &assistant.parts[0] {
            ReferencePart::Thinking { text } => assert_eq!(text, "I should read the directory"),
            other => panic!("expected thinking, got {:?}", other.kind()),
        }
        assert_eq!(text_of(&assistant.parts[1]), "Listing now.");
        let (name, summary, input, output, error) = tool_of(&assistant.parts[2]);
        assert_eq!(name, "bash");
        // the call's own intent wins over the derived summary
        assert_eq!(summary, "list files");
        assert!(input.contains("\"command\": \"ls -la\""), "input was {input}");
        assert_eq!(output, "total 8\nfile-a.txt");
        assert!(!error);

        assert_eq!(turns[2].role, ReferenceTurnRole::User);
        assert_eq!(text_of(&turns[2].parts[0]), "now show its contents");

        let second = &turns[3];
        assert_eq!(second.ended_at.as_deref(), Some("2026-10-06T01:00:08.000Z"));
        assert_eq!(kinds(second), vec![ReferencePartKind::Tool, ReferencePartKind::Text]);
        // no `intent`: the summary falls back to the call's first named argument
        let (name, summary, input, output, error) = tool_of(&second.parts[0]);
        assert_eq!(name, "read");
        assert_eq!(summary, "file-a.txt");
        assert!(input.contains("file-a.txt"), "input was {input}");
        assert_eq!(output, "permission denied");
        assert!(error, "a failed tool result is flagged");
        assert_eq!(text_of(&second.parts[1]), "Cannot read it.");

        for turn in &turns {
            assert!(turn.abandoned.is_none(), "omp keeps no entry tree");
            assert!(turn.source.is_none(), "the omp parser sets no turn source");
        }
    }

    #[test]
    fn omp_compaction_is_a_card_and_a_context_clear_restarts_the_conversation() {
        let turns = parse(COMPACTION_AND_CLEAR);
        // the clear wipes everything before it; the two malformed compactions after it
        // produce nothing
        assert_eq!(turns.len(), 1, "only the turn after the clear survives");
        assert_eq!(turns[0].role, ReferenceTurnRole::Assistant);
        assert_eq!(text_of(&turns[0].parts[0]), "after clear");
        assert_eq!(turns[0].started_at.as_deref(), Some("2026-10-06T02:00:06.000Z"));
    }

    #[test]
    fn omp_task_results_and_notices_land_in_the_user_seat() {
        let turns = parse(TASK_RESULTS);
        assert_eq!(turns.len(), 5);

        let (name, summary, _, output, _) = tool_of(&turns[1].parts[0]);
        assert_eq!(name, "task");
        // the summaries the call gave its tasks, in the order it gave them
        assert_eq!(summary, "audit parser · write tests");
        assert_eq!(output, "started");

        let tasks = match &turns[2].parts[0] {
            ReferencePart::TaskResult { tasks } => tasks,
            other => panic!("expected a task result, got {:?}", other.kind()),
        };
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].id, "st-1");
        assert_eq!(tasks[0].title, "audit parser", "the task call's own summary");
        assert_eq!(tasks[0].agent.as_deref(), Some("explore"));
        assert_eq!(tasks[0].model.as_deref(), Some("gpt-5"));
        assert_eq!(tasks[0].status, ReferenceTaskStatus::Completed);
        assert_eq!(tasks[0].duration_ms, Some(1234));
        assert_eq!(tasks[0].turns, Some(4));
        assert_eq!(tasks[0].tool_calls, Some(9));
        assert_eq!(tasks[0].tokens, Some(1200));
        assert_eq!(tasks[0].result, "parser audit clean");
        assert_eq!(tasks[0].result_cut, None);

        assert_eq!(tasks[1].id, "st-2");
        assert_eq!(tasks[1].title, "write tests");
        assert_eq!(tasks[1].agent.as_deref(), Some("quick"));
        assert_eq!(tasks[1].model.as_deref(), Some("flash"));
        assert_eq!(tasks[1].status, ReferenceTaskStatus::Failed);
        assert_eq!(tasks[1].duration_ms, None);
        assert_eq!(tasks[1].result, "compile error");

        match &turns[3].parts[0] {
            ReferencePart::Notice { text, source } => {
                assert_eq!(text, "context compacted");
                assert_eq!(source.as_deref(), Some("system-notice"));
            }
            other => panic!("expected a notice, got {:?}", other.kind()),
        }
        assert_eq!(turns[4].role, ReferenceTurnRole::Assistant);
    }

    #[test]
    fn omp_partial_records_do_not_fabricate_turns() {
        let turns = parse(PARTIAL_AND_MALFORMED);
        assert_eq!(turns.len(), 2, "only the two real turns survive");
        assert_eq!(text_of(&turns[0].parts[0]), "first");

        let assistant = &turns[1];
        assert_eq!(
            kinds(assistant),
            vec![
                ReferencePartKind::Thinking,
                ReferencePartKind::Tool,
                ReferencePartKind::Tool,
                ReferencePartKind::Tool,
                ReferencePartKind::Text,
                ReferencePartKind::Text,
                ReferencePartKind::Text,
            ],
            "the empty text block, the unknown block and the image-only user part add nothing"
        );
        match &assistant.parts[0] {
            ReferencePart::Thinking { text } => assert_eq!(text, "via the text field"),
            other => panic!("expected thinking, got {:?}", other.kind()),
        }
        let (name, summary, _, output, error) = tool_of(&assistant.parts[1]);
        assert_eq!((name, summary, output, error), ("bash", "echo hi", "plain string output", false));
        // a call with no id is shown but can never adopt a result
        let (name, summary, _, output, _) = tool_of(&assistant.parts[2]);
        assert_eq!((name, summary, output), ("bash", "no id", ""));
        // a call whose arguments are not an object still renders its own name
        let (name, summary, input, _, _) = tool_of(&assistant.parts[3]);
        assert_eq!((name, summary), ("weird", "weird"));
        assert!(input.starts_with('['), "input was {input}");
        assert_eq!(text_of(&assistant.parts[4]), "partial");
        assert_eq!(text_of(&assistant.parts[5]), "Error: 401 unauthorized");
        assert_eq!(text_of(&assistant.parts[6]), "final answer");
        assert_eq!(assistant.started_at.as_deref(), Some("2026-10-06T04:00:02.000Z"));
        assert_eq!(assistant.ended_at.as_deref(), Some("2026-10-06T04:00:08.000Z"));
    }

    #[test]
    fn omp_record_field_aliases_are_normalised() {
        let turns = parse(RECORD_ALIASES);
        assert_eq!(turns.len(), 2);
        // a prompt recorded as a plain string is still the user's text
        assert_eq!(text_of(&turns[0].parts[0]), "plain string prompt");
        // toolName / toolInput / callId / result are the pi-family spellings
        let (name, summary, input, output, error) = tool_of(&turns[1].parts[0]);
        assert_eq!(name, "bash");
        assert_eq!(summary, "pwd");
        assert!(input.contains("pwd"), "input was {input}");
        assert_eq!(output, "/tmp");
        assert!(!error);
    }

    #[test]
    fn omp_skill_invocations_keep_the_request_and_the_skill() {
        let turns = parse(SKILL_INVOCATION);
        assert_eq!(turns.len(), 4);
        // the user reads what they asked, not the tens of KB of SKILL.md before it
        assert_eq!(text_of(&turns[0].parts[0]), "review src/main.rs");
        assert_eq!(text_of(&turns[2].parts[0]), "ship 2026.10.7");
        // the skill the runtime loaded rides the request as its own part
        assert_eq!(turns[0].parts.len(), 2, "the request then the chip");
        let ReferencePart::Skill { skill } = &turns[0].parts[1] else {
            panic!("the second part is the skill chip, got {:?}", turns[0].parts[1].kind());
        };
        assert_eq!(skill.name, "code-review");
        assert_eq!(skill.evidence, ReferenceSkillEvidence::Instructions);
        assert_eq!(skill.status, ReferenceSkillStatus::Loaded);
        assert_eq!(
            skill.path.as_deref(),
            Some("/Users/indo/.agents/skills/code-review/SKILL.md")
        );
        // the legacy envelope form chips the same way
        assert_eq!(turns[2].parts.len(), 2);
        let ReferencePart::Skill { skill } = &turns[2].parts[1] else {
            panic!("the legacy envelope's chip is the second part");
        };
        assert_eq!(skill.name, "release");
        // prose that merely mentions the tag is the user's own text, with no chip on it
        assert_eq!(
            text_of(&turns[3].parts[0]),
            "the skill-instruction tag is documented in the README"
        );
        assert_eq!(turns[3].parts.len(), 1, "a mention is not an invocation");

        let prompt = "The user explicitly invoked the \"code-review\" skill. Follow the instructions in <skill-instruction> as binding for this request, while respecting higher-priority instructions.\n\n<skill-instruction name=\"code-review\" location=\"/Users/indo/.agents/skills/code-review/SKILL.md\">\n# Code review\nreview the diff\n</skill-instruction>\n\n<user-request>\nreview src/main.rs\n</user-request>";
        let invocation = omp_skill_invocation(prompt).expect("an instruction envelope is a skill");
        assert_eq!(invocation.request, "review src/main.rs");
        assert_eq!(invocation.skills.len(), 1);
        assert_eq!(invocation.skills[0].name, "code-review");
        assert_eq!(
            invocation.skills[0].path.as_deref(),
            Some("/Users/indo/.agents/skills/code-review/SKILL.md")
        );
        assert_eq!(invocation.skills[0].evidence, ReferenceSkillEvidence::Instructions);
        assert_eq!(invocation.skills[0].status, ReferenceSkillStatus::Loaded);

        let legacy = "<skill name=\"release\" location=\"/Users/indo/.agents/skills/release/SKILL.md\">\n# release\nsteps\n</skill>\n\nship 2026.10.7";
        let invocation = omp_skill_invocation(legacy).expect("the legacy envelope is a skill");
        assert_eq!(invocation.request, "ship 2026.10.7");
        assert_eq!(invocation.skills[0].name, "release");

        assert!(omp_skill_invocation("just a prompt").is_none());
    }

    #[test]
    fn omp_tool_output_is_trimmed_at_the_page_limit() {
        let turns = parse(&tool_call_session("bash", &"x".repeat(5_000)));
        let (_, _, _, output, _) = tool_of(&turns[1].parts[0]);
        assert_eq!(output.chars().count(), TOOL_OUTPUT_CHARS + "\n… trimmed".chars().count());
        assert!(output.ends_with("\n… trimmed"));
        match &turns[1].parts[0] {
            ReferencePart::Tool { output_ref, output_size, .. } => {
                assert_eq!(output_ref.as_deref(), Some("c1"));
                assert_eq!(*output_size, Some(5_000));
            }
            other => panic!("expected a tool part, got {:?}", other.kind()),
        }

        // a goal call keeps its answer whole up to the longer limit
        let turns = parse(&tool_call_session("get_goal", &"y".repeat(5_000)));
        let (_, _, _, output, _) = tool_of(&turns[1].parts[0]);
        assert_eq!(output.chars().count(), 5_000, "a goal answer under 16000 is kept whole");

        let turns = parse(&tool_call_session("get_goal", &"z".repeat(17_000)));
        match &turns[1].parts[0] {
            ReferencePart::Tool { output, output_size, .. } => {
                assert_eq!(output.chars().count(), WHOLE_OUTPUT_CHARS + "\n… trimmed".chars().count());
                assert_eq!(*output_size, Some(17_000));
            }
            other => panic!("expected a tool part, got {:?}", other.kind()),
        }
    }

    #[test]
    fn omp_parser_refuses_another_family() {
        for kind in [
            ReferenceNativeHistoryKind::Claude,
            ReferenceNativeHistoryKind::Codex,
            ReferenceNativeHistoryKind::Omo,
            ReferenceNativeHistoryKind::Gjc,
            ReferenceNativeHistoryKind::Pi,
            ReferenceNativeHistoryKind::Unavailable,
        ] {
            let error = parse_omp_transcript(kind, BASIC_SESSION)
                .expect_err("a lane reads one family only");
            assert!(error.contains(kind.as_str()), "error was {error}");
        }
        assert!(parse_omp_transcript(OMP_HISTORY_KIND, BASIC_SESSION).is_ok());
    }

    #[test]
    fn omp_empty_and_unparseable_input_yields_no_turns() {
        for text in ["", "\n", "not json at all\n", "null\n[]\n42\n\"text\"\n", "   \n\t\n"] {
            let turns = parse_omp_transcript(OMP_HISTORY_KIND, text)
                .expect("unparseable input is not an error, it is no turns");
            assert!(turns.is_empty(), "input {text:?} produced {} turns", turns.len());
        }
    }

    #[test]
    fn omp_limited_parse_keeps_the_newest_turns() {
        let turns = parse_omp_transcript_limited(OMP_HISTORY_KIND, BASIC_SESSION, Some(1))
            .expect("the omp family parses");
        assert_eq!(turns.len(), 1);
        assert_eq!(text_of(&turns[0].parts[0]), "now show its contents");

        // upstream's `slice(-0)` is `slice(0)`: a zero bound keeps everything
        let turns = parse_omp_transcript_limited(OMP_HISTORY_KIND, BASIC_SESSION, Some(0))
            .expect("the omp family parses");
        assert_eq!(turns.len(), 4);
    }

    #[test]
    fn omp_a_settled_stop_ends_its_turn() {
        let turns = parse(SETTLED_TURN_BOUNDARY);
        assert_eq!(turns.len(), 3, "the second answer is its own turn");
        assert_eq!(text_of(&turns[1].parts[0]), "first answer");
        assert_eq!(text_of(&turns[2].parts[0]), "woken by a monitor");
        assert_eq!(turns[1].ended_at.as_deref(), Some("2026-10-06T07:00:02.000Z"));
    }
}
