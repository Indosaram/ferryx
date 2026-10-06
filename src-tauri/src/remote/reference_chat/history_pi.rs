//! pi native history family (plan task 21).
//!
//! Ported from `devswha/herdr-web-ui` @ `54e5a1f67090cb09552d182e7e30dd0ecc314918`
//! (MIT, see `docs/chat/HERDR_LICENSE`). Upstream anchors, read at the pinned revision:
//!
//! | Upstream | What this module ports |
//! |---|---|
//! | `server/pi.ts` | why pi's store is an entry tree, and the unwritten-session rule (resolution is task 3's) |
//! | `server/pi-tree.ts` | `piBranchSegments`, `piAbandonedTurns`, `MAX_BRANCH_BYTES`, the index rules |
//! | `server/transcript-records.ts` | `parseOmpTranscript`'s record loop as pi reads it, `piMessage`, `piResults`, `piImageBlock`, `piNotice`, `isContextClear`, `toolSummary` |
//! | `server/skill-activity.ts` | `skillInvocationPrompt` (the user's request behind an invoked skill) |
//! | `server/tool-output.ts` | `trimOutput` (`TOOL_OUTPUT_CHARS` / `WHOLE_OUTPUT_CHARS`) |
//! | `server/conversation.ts` | `parseTurns` routing pi to the omp record shape, the pi branch, `piToolOutput`, `piTranscriptImage`, `PI_IMAGE_REF` |
//! | `shared/protocol.ts` | `ConversationTurn` / `ConversationPart` |
//!
//! pi's session file is an **append-only entry tree**, not a linear log: entries link to their
//! predecessor by `id`/`parentId`, and `/tree` moves the leaf back to an earlier entry without
//! rewriting the file, so a later append grows a branch beside the abandoned one. The conversation
//! pi shows — and the chat must show — is the path from the last entry written back to its root;
//! entries on a side branch stay in the file, unread, and are *disclosed* instead.
//!
//! This module is the pi family lane only. It declares no shared type and edits no other lane:
//! the contract's [`ReferenceHistoryParser`] obligation is [`parse_pi_history`] (which takes the
//! family's text, exactly as `dispatch_reference_history` will call it), and the byte-level tree
//! projection pi alone needs is exposed beside it for the resolver (task 3).
//!
//! Boundary, recorded rather than silently exceeded:
//!
//! * Upstream's `ConversationPart` has a standalone `skill` variant, and the record loop emits one
//!   per skill a user prompt invoked, beside the request text (`transcript-records.ts:259`). This
//!   lane emits both: the request as the user's text and each skill as its own
//!   [`ReferencePart::Skill`].
//! * Upstream's shared omp/omo/gjc/pi parser also folds OmO's `omo-senpi:wake` records into
//!   `task_result` parts. pi's runtime writes no such record, and the `task_result` shape belongs
//!   to the omo lane (task 19); it is deliberately not duplicated here.
//! * Upstream's parse path passes `maxTurns = Infinity` for pi (`parseTurns`), so no page cut is
//!   applied. `MAX_TURNS = 100` is only the standalone parser's default and is not ported.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use base64::Engine as _;
use regex::Regex;
use serde_json::{json, Value};

use super::types::{
    ReferenceAbandonedBranch, ReferenceHistoryParser, ReferenceImageRef, ReferenceNativeHistoryKind,
    ReferencePart, ReferenceSkillActivity, ReferenceSkillEvidence, ReferenceSkillStatus,
    ReferenceTurn, ReferenceTurnRole,
};

/// The error a pi reader answers when the session file is an entry tree it cannot project.
///
/// Upstream throws `ConversationUnavailable("branch_unreadable")` (`server/conversation.ts`) and
/// the chat falls back to the pane's terminal output rather than showing a conversation built from
/// bytes it could not walk.
pub const PI_BRANCH_UNREADABLE: &str = "branch_unreadable";

/// The error a pi reader answers for a kind it does not read. `Unavailable` included: a lane
/// handles exactly one family, and an empty success would be read as "this session has no turns".
pub const PI_UNSUPPORTED_KIND: &str = "unsupported_native_history_kind";

/// A branch longer than this is not returned at all (`pi-tree.ts`: `MAX_BRANCH_BYTES`).
///
/// The largest branch across 147 real session files upstream is 23.3 MiB, so the cap sits 2.7x
/// above what pi has been seen to write; over it the pane shows scrollback instead of a projection
/// built by holding that many bytes of paths in memory.
pub const PI_MAX_BRANCH_BYTES: usize = 64 * 1024 * 1024;

/// Past this a tool's output is cut in the page; the rest is fetched on request
/// (`tool-output.ts`: `TOOL_OUTPUT_CHARS`).
pub const PI_TOOL_OUTPUT_CHARS: usize = 4000;

/// `create_goal` / `update_goal` / `get_goal` answer with the goal as JSON the chat reads, so their
/// output is kept whole up to this (`tool-output.ts`: `WHOLE_OUTPUT_CHARS`).
pub const PI_WHOLE_OUTPUT_CHARS: usize = 16_000;

/// A whole tool output read on demand is bounded by this (`conversation.ts`: `TOOL_OUTPUT_MAX`).
pub const PI_TOOL_OUTPUT_MAX: usize = 2_000_000;

/// A collapsed tool chip's summary is bounded to this (`transcript-records.ts`: `toolSummary`).
pub const PI_TOOL_SUMMARY_LIMIT: usize = 120;

/// One range of the transcript file, in the order it should be read
/// (`pi-tree.ts`: `TranscriptSegment`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PiTranscriptSegment {
    pub start: usize,
    pub end: usize,
}

/// A native image the transcript holds inline, decoded for the caller
/// (`conversation.ts`: `piTranscriptImage`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PiImage {
    pub media_type: String,
    pub data: Vec<u8>,
}

/// The `@`-free ref an image part carries, `pi:<call id>:<nth image>` (`conversation.ts`:
/// `PI_IMAGE_REF`). The bytes stay in the file and are fetched when the row is opened.
pub fn pi_image_ref(call_id: &str, index: usize) -> String {
    format!("pi:{call_id}:{index}")
}

/// Parse one pi transcript's text into turns — the family's [`ReferenceHistoryParser`].
///
/// The text is the *projected active branch* (see [`pi_transcript_segments`]); this function is
/// the pure record loop, so the dispatcher can call it with the family's text exactly as the
/// contract's signature requires.
pub fn parse_pi_history(
    kind: ReferenceNativeHistoryKind,
    text: &str,
) -> Result<Vec<ReferenceTurn>, String> {
    if kind != ReferenceNativeHistoryKind::Pi {
        return Err(format!("{PI_UNSUPPORTED_KIND}: {}", kind.as_str()));
    }
    Ok(parse_pi_records(text))
}

/// The lane's parser as the shared signature sees it (task 2's dispatch table).
pub fn pi_history_parser() -> ReferenceHistoryParser {
    parse_pi_history
}

/// Parse a pi session file's **bytes**: project the branch the leaf stands on, then read it.
///
/// `Err(PI_BRANCH_UNREADABLE)` when the entry tree cannot be projected — a tree the reader cannot
/// walk is not a conversation, and it is never approximated from the file's other bytes.
pub fn parse_pi_transcript(bytes: &[u8]) -> Result<Vec<ReferenceTurn>, String> {
    let segments =
        pi_transcript_segments(bytes).ok_or_else(|| PI_BRANCH_UNREADABLE.to_string())?;
    let text = branch_text(bytes, &segments);
    parse_pi_history(ReferenceNativeHistoryKind::Pi, &text)
}

/// The active branch of a pi session file: the last entry written back to its root, oldest first,
/// as byte ranges (`pi-tree.ts`: `piBranchSegments`).
///
/// Adjacent entries merge into one range, so a session no `/tree` touched reads as the single
/// prefix it is. `None` when the tree cannot be read, holds no entry at all, or its branch is over
/// [`PI_MAX_BRANCH_BYTES`].
pub fn pi_transcript_segments(bytes: &[u8]) -> Option<Vec<PiTranscriptSegment>> {
    pi_transcript_segments_with_limit(bytes, PI_MAX_BRANCH_BYTES)
}

/// [`pi_transcript_segments`] with the branch cap supplied, so the refusal path is reachable
/// without a 64 MiB session.
pub fn pi_transcript_segments_with_limit(
    bytes: &[u8],
    max_branch_bytes: usize,
) -> Option<Vec<PiTranscriptSegment>> {
    let index = build_index(bytes);
    branch_segments(&index, bytes.len(), max_branch_bytes)
}

/// What the file holds that the chat cannot show: the turns on paths pi walked away from
/// (`pi-tree.ts`: `piAbandonedTurns`).
///
/// pi keeps the leaf pointer to itself — `branch()` moves it and writes no entry, and no entry
/// type names the leaf — so the only branch a reader can rebuild is the one ending at the last
/// entry written. Everything else was abandoned, and this counts it so the reader can say so out
/// loud instead of dropping those turns with no trace.
///
/// `count` is turns, not entries; `branches` is how many places the navigation happened; `summary`
/// is pi's own account when the user answered `/tree`'s "Summarize branch?" with one. A session no
/// `/tree` touched answers all zeroes — the common case — and the caller renders only `count > 0`.
pub fn pi_abandoned_branch(bytes: &[u8]) -> Option<ReferenceAbandonedBranch> {
    let index = build_index(bytes);
    abandoned_branch(&index, bytes.len())
}

/// One whole tool output by ref, read on the active branch only (`conversation.ts`:
/// `piToolOutput`).
///
/// Reading the file whole would answer a ref from a branch a `/tree` abandoned, whose output the
/// chat never showed. The output is cut at [`PI_TOOL_OUTPUT_MAX`] when it is longer.
pub fn pi_tool_output(bytes: &[u8], output_ref: &str) -> Option<String> {
    let segments = pi_transcript_segments(bytes)?;
    for segment in &segments {
        let text = branch_text(bytes, std::slice::from_ref(segment));
        for line in text.split('\n') {
            if !line.contains(output_ref) {
                continue;
            }
            let Ok(entry) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            let Some(message) = pi_message(&entry) else {
                continue;
            };
            let Some(output) = pi_results(&message)
                .into_iter()
                .find(|result| result.id == output_ref)
                .map(|result| result.text)
            else {
                continue;
            };
            return Some(truncate_chars(&output, PI_TOOL_OUTPUT_MAX));
        }
    }
    None
}

/// The image a pi tool call returned, by the ref the page gave it (`conversation.ts`:
/// `piTranscriptImage`).
///
/// Like the output, it is read on the active branch only: an image from a branch a `/tree`
/// abandoned is one the chat never showed, so it is not offered.
pub fn pi_transcript_image(bytes: &[u8], image_ref: &str) -> Option<PiImage> {
    let captures = pi_image_ref_regex().captures(image_ref)?;
    let call_id = captures.get(1)?.as_str().to_string();
    let index: usize = captures.get(2)?.as_str().parse().ok()?;
    let segments = pi_transcript_segments(bytes)?;
    for segment in &segments {
        let text = branch_text(bytes, std::slice::from_ref(segment));
        for line in text.split('\n') {
            if !line.contains(&call_id) {
                continue;
            }
            let Ok(entry) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            let Some(message) = pi_message(&entry) else {
                continue;
            };
            let Some(image) = pi_image_block(&message, &call_id, index) else {
                continue;
            };
            let Ok(data) = base64::engine::general_purpose::STANDARD.decode(image.data) else {
                return None;
            };
            return Some(PiImage { media_type: image.media_type, data });
        }
    }
    None
}

// ---------------------------------------------------------------------------------------------
// The record loop (`transcript-records.ts`: `parseOmpTranscript`, as `parseTurns` drives pi)
// ---------------------------------------------------------------------------------------------

/// One record's message, normalized the way `piMessage` does it.
struct PiMessage {
    role: Option<String>,
    /// The normalized content blocks, always an array.
    content: Vec<Value>,
    tool_call_id: Option<String>,
    stop_reason: Option<String>,
    error_message: Option<String>,
    is_error: bool,
}

/// One tool result, with the images it carries (`transcript-records.ts`: `piResults`).
struct PiToolResult {
    id: String,
    text: String,
    error: bool,
    /// Media types only, in order: the bytes stay in the file, and the page's ref quotes the
    /// position of the image inside this one result.
    images: Vec<String>,
}

/// One inline image block, decoded lazily by the caller.
struct PiImageData {
    media_type: String,
    data: String,
}

/// The skill a prompt invoked: the skills the runtime loaded, in the order it named them, and the
/// request behind them (`skill-activity.ts`: `skillInvocationPrompt`).
struct PiSkillInvocation {
    skills: Vec<ReferenceSkillActivity>,
    request: String,
}

/// Split one pi transcript's text into turns.
///
/// Adjacent assistant messages merge into one turn; tool calls adopt the output of the record that
/// answers them (matched by tool-call id); thinking stays private to the agent but is carried as
/// its own part. An assistant message that stopped for good (`stopReason: "stop"`) ends its turn:
/// the next one was woken by something nobody typed.
fn parse_pi_records(text: &str) -> Vec<ReferenceTurn> {
    let mut turns: Vec<ReferenceTurn> = Vec::new();
    // tool parts still waiting for their result, by tool-call id: (turn index, part index)
    let mut pending: HashMap<String, (usize, usize)> = HashMap::new();
    // the last assistant message stopped for good: the next one starts a turn of its own
    let mut settled = false;

    for line in text.split('\n') {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            continue; // a torn tail line while pi is mid-append
        };
        if !entry.is_object() {
            continue;
        }
        if is_context_clear(&entry) {
            turns.clear();
            pending.clear();
            continue;
        }
        let timestamp = entry.get("timestamp").and_then(Value::as_str);
        if let Some(notice) = pi_notice(&entry) {
            turns.push(user_turn(timestamp, vec![notice]));
            continue;
        }
        // pi folds old context into a summary of its own accord and on /compact. The entry is a
        // tree entry, not a message, so it reaches the chat only through this branch: the card says
        // where the conversation was cut, and pi keeps answering from the summary onward.
        if entry.get("type").and_then(Value::as_str) == Some("compaction") {
            if let Some(summary) = entry
                .get("summary")
                .and_then(Value::as_str)
                .filter(|summary| !summary.trim().is_empty())
            {
                turns.push(user_turn(timestamp, vec![ReferencePart::Compact { text: summary.to_string() }]));
            }
            continue;
        }
        let Some(message) = pi_message(&entry) else {
            continue;
        };
        if message.role.as_deref() != Some("assistant") {
            apply_results(&mut turns, &mut pending, &message);
        }

        if message.role.as_deref() == Some("user") {
            let prompt = user_prompt_text(&message);
            if prompt.is_empty() {
                continue; // image-only user parts have no text to show
            }
            // A skill invocation reads as what the user asked: the SKILL.md the runtime put before
            // it is not the user's text. Each skill the runtime loaded rides it as its own part, in
            // the order the envelope named them (`transcript-records.ts:259`).
            let invocation = pi_skill_invocation(&prompt);
            let asked = match &invocation {
                Some(invocation) if !invocation.request.is_empty() => invocation.request.clone(),
                Some(invocation) => invocation
                    .skills
                    .iter()
                    .map(|skill| format!("/skill:{}", skill.name))
                    .collect::<Vec<String>>()
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
            turns.push(user_turn(timestamp, parts));
            continue;
        }

        if message.role.as_deref() == Some("assistant") {
            let turn_index = assistant_turn(&mut turns, settled, timestamp);
            if let Some(timestamp) = timestamp {
                turns[turn_index].ended_at = Some(timestamp.to_string());
            }
            for block in &message.content {
                if block.get("type").and_then(Value::as_str) == Some("text") {
                    if let Some(body) = block.get("text").and_then(Value::as_str) {
                        if !body.is_empty() {
                            turns[turn_index]
                                .parts
                                .push(ReferencePart::Text { text: body.to_string(), phase: None });
                        }
                    }
                } else if block.get("type").and_then(Value::as_str) == Some("thinking") {
                    let thinking = block
                        .get("thinking")
                        .and_then(Value::as_str)
                        .or_else(|| block.get("text").and_then(Value::as_str))
                        .unwrap_or("");
                    if !thinking.is_empty() {
                        turns[turn_index]
                            .parts
                            .push(ReferencePart::Thinking { text: thinking.to_string() });
                    }
                } else if block.get("type").and_then(Value::as_str) == Some("toolCall") {
                    if let Some(name) = block.get("name").and_then(Value::as_str) {
                        let arguments = block
                            .get("arguments")
                            .filter(|value| value.is_object() || value.is_array())
                            .cloned()
                            .unwrap_or_else(|| json!({}));
                        let summary = match block
                            .get("intent")
                            .and_then(Value::as_str)
                            .filter(|intent| !intent.is_empty())
                        {
                            Some(intent) => intent.to_string(),
                            None => tool_summary(name, &arguments),
                        };
                        let part_index = turns[turn_index].parts.len();
                        turns[turn_index].parts.push(ReferencePart::Tool {
                            name: name.to_string(),
                            summary: truncate_chars(&summary, PI_TOOL_SUMMARY_LIMIT),
                            input: serde_json::to_string_pretty(&arguments).unwrap_or_default(),
                            output: String::new(),
                            error: None,
                            skill: None,
                            output_ref: None,
                            output_size: None,
                            images: Vec::new(),
                        });
                        if let Some(id) = block.get("id").and_then(Value::as_str) {
                            pending.insert(id.to_string(), (turn_index, part_index));
                        }
                    }
                }
                // unsupported transcript parts are intentionally ignored
            }
            // A provider can place a result beside its call in the same assistant record.
            apply_results(&mut turns, &mut pending, &message);
            // A failed request (a 401, an overloaded provider) leaves an empty message: without its
            // error the chat showed the prompt with no answer at all.
            if message.stop_reason.as_deref() == Some("error") {
                if let Some(error) = message.error_message.as_deref().filter(|text| !text.is_empty())
                {
                    turns[turn_index]
                        .parts
                        .push(ReferencePart::Text { text: format!("Error: {error}"), phase: None });
                }
            }
            settled = message.stop_reason.as_deref() == Some("stop");
        }
    }

    turns.retain(|turn| !turn.parts.is_empty());
    turns
}

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

/// The assistant turn this message belongs to: the last one when it is still open, else a new one.
fn assistant_turn(turns: &mut Vec<ReferenceTurn>, settled: bool, timestamp: Option<&str>) -> usize {
    if let Some(last) = turns.last() {
        if last.role == ReferenceTurnRole::Assistant && !settled {
            return turns.len() - 1;
        }
    }
    turns.push(ReferenceTurn {
        role: ReferenceTurnRole::Assistant,
        started_at: timestamp.map(str::to_string),
        ended_at: None,
        source: None,
        parts: Vec::new(),
        abandoned: None,
    });
    turns.len() - 1
}

/// Fold this record's tool results into the tool parts they answer.
fn apply_results(
    turns: &mut Vec<ReferenceTurn>,
    pending: &mut HashMap<String, (usize, usize)>,
    message: &PiMessage,
) {
    for result in pi_results(message) {
        let Some((turn_index, part_index)) = pending.remove(&result.id) else {
            continue;
        };
        let Some(ReferencePart::Tool { name, output, error, images, output_ref, output_size, .. }) =
            turns
                .get_mut(turn_index)
                .and_then(|turn| turn.parts.get_mut(part_index))
        else {
            continue;
        };
        trim_output(name, output, output_ref, output_size, &result.text, &result.id);
        if result.error {
            *error = Some(true);
        }
        if !result.images.is_empty() {
            // Addressed by the call it answers: a nested result shares its entry with other blocks,
            // so the entry id alone could not say which one an image came from.
            *images = result
                .images
                .iter()
                .enumerate()
                .map(|(index, media_type)| ReferenceImageRef {
                    media_type: media_type.clone(),
                    r#ref: pi_image_ref(&result.id, index),
                })
                .collect();
        }
    }
}

/// The user text of a message: its text parts, joined the way the reference joins them.
fn user_prompt_text(message: &PiMessage) -> String {
    message
        .content
        .iter()
        .filter_map(|part| {
            if part.get("type").and_then(Value::as_str) != Some("text") {
                return None;
            }
            part.get("text")
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
                .map(str::to_string)
        })
        .collect::<Vec<String>>()
        .join("\n")
}

/// Sets a tool part's output, cut to what a page carries, keeping what it takes to fetch the rest
/// (`tool-output.ts`: `trimOutput`).
fn trim_output(
    name: &str,
    output: &mut String,
    output_ref: &mut Option<String>,
    output_size: &mut Option<u64>,
    text: &str,
    reference: &str,
) {
    let limit = if is_whole_output_tool(name) { PI_WHOLE_OUTPUT_CHARS } else { PI_TOOL_OUTPUT_CHARS };
    let length = text.chars().count();
    if length <= limit {
        *output = text.to_string();
        return;
    }
    *output = format!("{}\n… trimmed", truncate_chars(text, limit));
    *output_ref = Some(reference.to_string());
    *output_size = Some(length as u64);
}

/// omo's goal calls answer with the goal as JSON the chat reads.
fn is_whole_output_tool(name: &str) -> bool {
    matches!(name, "create_goal" | "update_goal" | "get_goal")
}

/// One entry's message, normalized (`transcript-records.ts`: `piMessage`).
///
/// Pi-family providers use several spellings for the same tool call/result fields; the page is
/// built from the normalized record, so every later read must agree with it.
fn pi_message(entry: &Value) -> Option<PiMessage> {
    if entry.get("type").and_then(Value::as_str) != Some("message") {
        return None;
    }
    let message = entry
        .get("message")
        .filter(|value| value.is_object())
        .cloned()
        .unwrap_or(Value::Null);
    if message.get("display").and_then(Value::as_bool) == Some(false) {
        return None;
    }
    if entry.get("display").and_then(Value::as_bool) == Some(false) {
        return None;
    }
    let raw: Vec<Value> = match message.get("content") {
        Some(Value::String(text)) => vec![json!({ "type": "text", "text": text })],
        Some(Value::Array(items)) => items.clone(),
        _ => Vec::new(),
    };
    let content = raw.iter().map(normalize_block).collect();
    Some(PiMessage {
        role: message.get("role").and_then(Value::as_str).map(str::to_string),
        content,
        tool_call_id: first_string(&[message.get("toolCallId"), message.get("callId")]),
        stop_reason: message.get("stopReason").and_then(Value::as_str).map(str::to_string),
        error_message: message.get("errorMessage").and_then(Value::as_str).map(str::to_string),
        is_error: message.get("isError").and_then(Value::as_bool).unwrap_or(false),
    })
}

/// Normalize one content block's field spellings (`transcript-records.ts`: `piMessage`'s map).
fn normalize_block(value: &Value) -> Value {
    let Value::Object(mut block) = value.clone() else {
        return json!({});
    };
    let kind = block.get("type").and_then(Value::as_str).map(str::to_string);
    match kind.as_deref() {
        Some("toolCall") => {
            let name = first_string(&[block.get("toolName"), block.get("name")]);
            if let Some(name) = name {
                block.insert("name".to_string(), Value::String(name));
            }
            let id = first_string(&[block.get("toolCallId"), block.get("id"), block.get("callId")]);
            if let Some(id) = id {
                block.insert("id".to_string(), Value::String(id));
            }
            let arguments = ["toolInput", "input", "arguments"]
                .iter()
                .find_map(|key| block.get(*key).filter(|value| !value.is_null()).cloned());
            if let Some(arguments) = arguments {
                block.insert("arguments".to_string(), arguments);
            }
        }
        Some("toolResult") => {
            let id = first_string(&[block.get("toolCallId"), block.get("callId"), block.get("id")]);
            if let Some(id) = id {
                block.insert("toolCallId".to_string(), Value::String(id));
            }
            let content = ["output", "content", "result"]
                .iter()
                .find_map(|key| block.get(*key).filter(|value| !value.is_null()).cloned());
            if let Some(content) = content {
                block.insert("content".to_string(), content);
            }
        }
        _ => {}
    }
    Value::Object(block)
}

/// One entry's tool results, with the images each carries (`transcript-records.ts`: `piResults`).
fn pi_results(message: &PiMessage) -> Vec<PiToolResult> {
    if message.role.as_deref() == Some("toolResult") {
        let content = Value::Array(message.content.clone());
        return result_of(message.tool_call_id.as_deref(), &content, message.is_error)
            .into_iter()
            .collect();
    }
    message
        .content
        .iter()
        .filter_map(|block| {
            if block.get("type").and_then(Value::as_str) != Some("toolResult") {
                return None;
            }
            let id = block.get("toolCallId").and_then(Value::as_str);
            let content = block.get("content").cloned().unwrap_or(Value::Null);
            result_of(id, &content, block.get("isError").and_then(Value::as_bool).unwrap_or(false))
        })
        .collect()
}

fn result_of(tool_call_id: Option<&str>, content: &Value, is_error: bool) -> Option<PiToolResult> {
    let id = tool_call_id?.to_string();
    let images = content
        .as_array()
        .map(|blocks| blocks.iter().filter_map(pi_image_of).map(|image| image.media_type).collect())
        .unwrap_or_default();
    Some(PiToolResult { id, text: result_text(content), error: is_error, images })
}

/// The `index`th image of the tool result answering `tool_call_id`
/// (`transcript-records.ts`: `piImageBlock`). `message` must come from [`pi_message`].
fn pi_image_block(message: &PiMessage, tool_call_id: &str, index: usize) -> Option<PiImageData> {
    if pi_results(message).iter().all(|result| result.id != tool_call_id) {
        return None;
    }
    let mut images: Vec<PiImageData> = Vec::new();
    if message.role.as_deref() == Some("toolResult") {
        collect_images(&Value::Array(message.content.clone()), &mut images);
    } else {
        for block in &message.content {
            if block.get("type").and_then(Value::as_str) != Some("toolResult") {
                continue;
            }
            if block.get("toolCallId").and_then(Value::as_str) != Some(tool_call_id) {
                continue;
            }
            collect_images(block.get("content").unwrap_or(&Value::Null), &mut images);
        }
    }
    images.into_iter().nth(index)
}

fn collect_images(content: &Value, out: &mut Vec<PiImageData>) {
    let Some(blocks) = content.as_array() else {
        return;
    };
    for block in blocks {
        if let Some(image) = pi_image_of(block) {
            out.push(image);
        }
    }
}

/// The image types a chat shows; pi names the type `mimeType` where Claude names it `media_type`.
const PI_IMAGE_TYPES: [&str; 4] = ["image/png", "image/jpeg", "image/gif", "image/webp"];

fn pi_image_of(value: &Value) -> Option<PiImageData> {
    if value.get("type").and_then(Value::as_str) != Some("image") {
        return None;
    }
    let media_type = value
        .get("mimeType")
        .and_then(Value::as_str)
        .or_else(|| value.get("media_type").and_then(Value::as_str))?;
    if !PI_IMAGE_TYPES.contains(&media_type) {
        return None;
    }
    let data = value.get("data").and_then(Value::as_str)?;
    Some(PiImageData { media_type: media_type.to_string(), data: data.to_string() })
}

/// A tool result's text (`transcript-records.ts`: `resultText`).
fn result_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .map(|part| part.get("text").and_then(Value::as_str).unwrap_or(""))
            .collect(),
        _ => String::new(),
    }
}

/// A `custom_message` in the user's seat (`transcript-records.ts`: `piNotice`).
///
/// It starts a turn; nobody typed it. The envelope is chrome: pi writes `<system-notice>` around
/// the runtime's own text, and `customType` is the runtime's name for it.
fn pi_notice(entry: &Value) -> Option<ReferencePart> {
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
    Some(ReferencePart::Notice {
        text: text.to_string(),
        source: entry.get("customType").and_then(Value::as_str).map(str::to_string),
    })
}

/// A reset marker: everything before it is not the conversation any more
/// (`transcript-records.ts`: `isContextClear`).
///
/// Claude's whole-envelope local-command form is the claude lane's own rule; pi and the rest of
/// the omp-family stores write a `custom` record named `context_clear`.
fn is_context_clear(entry: &Value) -> bool {
    entry.get("type").and_then(Value::as_str) == Some("custom")
        && entry.get("customType").and_then(Value::as_str) == Some("context_clear")
}

/// The one-line summary a collapsed tool chip shows (`transcript-records.ts`: `toolSummary`).
fn tool_summary(name: &str, input: &Value) -> String {
    // an OmO or omp `task` call: the summary it gave the person, one per task of a batch
    if name == "task" {
        let items: Vec<&Value> = match input.get("tasks") {
            Some(Value::Array(items)) => items.iter().collect(),
            _ => vec![input],
        };
        let titles: Vec<String> = items
            .iter()
            .filter_map(|item| {
                trimmed_label(item.get("task_summary")).or_else(|| trimmed_label(item.get("description")))
            })
            .collect();
        if !titles.is_empty() {
            return truncate_chars(&titles.join(" · "), PI_TOOL_SUMMARY_LIMIT);
        }
    }
    // pi names a file `path` where Claude names it `file_path`, and a notebook `notebook_path`
    let first = ["command", "file_path", "notebook_path", "path", "pattern", "description", "url"]
        .iter()
        .find_map(|key| input.get(*key).and_then(Value::as_str));
    match first {
        Some(value) => truncate_chars(value, PI_TOOL_SUMMARY_LIMIT),
        None => name.to_string(),
    }
}

// ---------------------------------------------------------------------------------------------
// The skill invocation behind a prompt (`skill-activity.ts`: `skillInvocationPrompt`)
// ---------------------------------------------------------------------------------------------

fn skill_instruction_regex() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(
            r#"(?s)^The user explicitly invoked the "([^"]+)" skill\. Follow the instructions in <skill-instruction> as binding for this request, while respecting higher-priority instructions\.\n\n<skill-instruction name="([^"]+)" location="([^"]+)">\n.*?\n</skill-instruction>"#,
        )
        .expect("static skill-instruction pattern")
    })
}

fn skill_user_request_regex() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"(?s)^\n\n<user-request>\n(.*?)\n</user-request>$")
            .expect("static user-request pattern")
    })
}

fn legacy_skill_regex() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r#"(?s)^<skill name="([^"]+)" location="([^"]+)">\n.*?\n</skill>(?:\n\n(.*))?$"#)
            .expect("static legacy skill pattern")
    })
}

fn pi_image_ref_regex() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"^pi:([A-Za-z0-9_:.\-]{1,128}):(\d{1,3})$").expect("static pi image ref pattern")
    })
}

/// The prompt omp/omo/pi record when the user invokes a skill (`/skill:name`, `$name`, a keyword):
/// the whole SKILL.md before the request, tens of KB the user never typed. Mirrors pi's own
/// `parseSkillBlock`: chained invocations, then `<user-request>`, or the legacy `<skill name
/// location>` form. Anything else is left as the user's text.
fn pi_skill_invocation(text: &str) -> Option<PiSkillInvocation> {
    let instruction = skill_instruction_regex();
    let mut skills: Vec<ReferenceSkillActivity> = Vec::new();
    let mut remainder = text.to_string();
    let mut pending: Option<SkillCaptures> =
        instruction.captures(&remainder).map(|found| skill_captures(&found));
    while let Some(captures) = pending {
        // the outer and inner names must agree, or this is not the reference's envelope
        if captures.name != captures.declared {
            return None;
        }
        let skill = pi_loaded_skill(&captures.name, &captures.location)?;
        skills.push(skill);
        remainder = remainder[captures.whole_len..].to_string();
        if !remainder.starts_with("\n\nThe user explicitly invoked the ") {
            break;
        }
        remainder = remainder[2..].to_string();
        pending = match instruction.captures(&remainder) {
            Some(found) => Some(skill_captures(&found)),
            None => return None,
        };
    }
    if !skills.is_empty() {
        if remainder.is_empty() {
            return Some(PiSkillInvocation { skills, request: String::new() });
        }
        let request = skill_user_request_regex()
            .captures(&remainder)
            .and_then(|found| found.get(1))
            .map(|found| found.as_str().trim().to_string());
        return request.map(|request| PiSkillInvocation { skills, request });
    }
    let legacy = legacy_skill_regex()
        .captures(text)
        .map(|found| legacy_captures(&found))?;
    let skill = pi_loaded_skill(&legacy.name, &legacy.location)?;
    Some(PiSkillInvocation { skills: vec![skill], request: legacy.request.trim().to_string() })
}

struct SkillCaptures {
    name: String,
    declared: String,
    location: String,
    whole_len: usize,
}

fn skill_captures(found: &regex::Captures<'_>) -> SkillCaptures {
    SkillCaptures {
        name: found.get(1).map(|found| found.as_str().to_string()).unwrap_or_default(),
        declared: found.get(2).map(|found| found.as_str().to_string()).unwrap_or_default(),
        location: found.get(3).map(|found| found.as_str().to_string()).unwrap_or_default(),
        whole_len: found.get(0).map(|found| found.as_str().len()).unwrap_or(0),
    }
}

struct LegacyCaptures {
    name: String,
    location: String,
    request: String,
}

fn legacy_captures(found: &regex::Captures<'_>) -> LegacyCaptures {
    LegacyCaptures {
        name: found.get(1).map(|found| found.as_str().to_string()).unwrap_or_default(),
        location: found.get(2).map(|found| found.as_str().to_string()).unwrap_or_default(),
        request: found.get(3).map(|found| found.as_str().to_string()).unwrap_or_default(),
    }
}

/// `skill-activity.ts`: `loadedSkill` — a name and the location pi named, no lookup.
fn pi_loaded_skill(name: &str, location: &str) -> Option<ReferenceSkillActivity> {
    let name = pi_skill_name(name)?;
    if !pi_skill_location(location) {
        return None;
    }
    Some(ReferenceSkillActivity {
        name,
        path: Some(location.to_string()),
        evidence: ReferenceSkillEvidence::Instructions,
        status: ReferenceSkillStatus::Loaded,
    })
}

/// `skill-activity.ts`: `label` — a name, never markup.
fn pi_skill_name(value: &str) -> Option<String> {
    if value.is_empty() || value.chars().count() > 200 || value.contains(['\r', '\n', '<', '>']) {
        return None;
    }
    Some(value.to_string())
}

/// `skill-activity.ts`: `loadedSkill` — a location pi named, no lookup.
fn pi_skill_location(location: &str) -> bool {
    !location.is_empty()
        && location.chars().count() <= 4096
        && !location.contains(['\r', '\n'])
}

// ---------------------------------------------------------------------------------------------
// The entry tree (`pi-tree.ts`: `indexEntry` / `buildIndex` / `piBranchSegments` / `piAbandonedTurns`)
// ---------------------------------------------------------------------------------------------

/// One indexed entry (`pi-tree.ts`: `PiEntry`).
struct PiEntry {
    id: String,
    parent: Option<String>,
    start: usize,
    end: usize,
    entry_type: String,
    role: String,
    summary: Option<String>,
}

/// The tree as it stands, in append order (`pi-tree.ts`: `PiIndex`).
///
/// The upstream index caches per file and revalidates by device/inode plus a scanned-tail check;
/// that is file identity, which belongs to the resolver that owns the path. This projection is the
/// pure part: the same bytes always index the same tree.
struct PiIndex {
    entries: Vec<PiEntry>,
    /// children by parent id, in append order, as indices into `entries`
    children: HashMap<String, Vec<usize>>,
}

/// Appends whole lines from the buffer; a last line still without its newline waits for it, so an
/// entry never spans a torn tail.
fn build_index(bytes: &[u8]) -> PiIndex {
    let mut index = PiIndex { entries: Vec::new(), children: HashMap::new() };
    let mut offset = 0usize;
    while let Some(relative) = bytes[offset..].iter().position(|byte| *byte == b'\n') {
        let line_start = offset;
        let line_end = offset + relative;
        let end = line_end + 1;
        let line = String::from_utf8_lossy(&bytes[line_start..line_end]);
        if let Some(entry) = index_entry(&line, line_start, end) {
            add_entry(&mut index, entry);
        }
        offset = end;
    }
    index
}

/// One line as an entry, or `None` for the header and a torn fragment
/// (`pi-tree.ts`: `indexEntry`).
fn index_entry(line: &str, start: usize, end: usize) -> Option<PiEntry> {
    if !line.contains("\"id\"") {
        return None;
    }
    let Ok(entry) = serde_json::from_str::<Value>(line) else {
        return None;
    };
    if !entry.is_object() {
        return None;
    }
    let id = entry.get("id").and_then(Value::as_str).filter(|id| !id.is_empty())?;
    // type and role are taken here because the line is parsed for this anyway: what counts as a
    // turn is decided later by a reader that must not pay for a second pass over the file
    let parent = entry
        .get("parentId")
        .and_then(Value::as_str)
        .filter(|parent| !parent.is_empty())
        .map(str::to_string);
    Some(PiEntry {
        id: id.to_string(),
        parent,
        start,
        end,
        entry_type: entry.get("type").and_then(Value::as_str).unwrap_or("").to_string(),
        role: entry
            .get("message")
            .and_then(|message| message.get("role"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        summary: entry.get("summary").and_then(Value::as_str).map(str::to_string),
    })
}

fn add_entry(index: &mut PiIndex, entry: PiEntry) {
    let position = index.entries.len();
    let parent = entry.parent.clone();
    index.entries.push(entry);
    if let Some(parent) = parent {
        index.children.entry(parent).or_default().push(position);
    }
}

/// Later appends win for a repeated id (never pi, but never trusted).
fn entries_by_id(index: &PiIndex) -> HashMap<&str, usize> {
    let mut by_id: HashMap<&str, usize> = HashMap::new();
    for (position, entry) in index.entries.iter().enumerate() {
        by_id.insert(entry.id.as_str(), position);
    }
    by_id
}

/// The last entry written, or `None` when the tree is empty or longer than the file it came from.
fn leaf_of<'a>(index: &'a PiIndex, size: usize) -> Option<usize> {
    let last = index.entries.len().checked_sub(1)?;
    if index.entries[last].end > size {
        return None;
    }
    Some(last)
}

/// The active branch as merged byte ranges (`pi-tree.ts`: `piBranchSegments`).
fn branch_segments(
    index: &PiIndex,
    size: usize,
    max_branch_bytes: usize,
) -> Option<Vec<PiTranscriptSegment>> {
    let leaf = leaf_of(index, size)?;
    let by_id = entries_by_id(index);
    let mut chain: Vec<usize> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    let mut total = 0usize;
    let mut next = Some(leaf);
    while let Some(current) = next {
        let entry = &index.entries[current];
        if !seen.insert(entry.id.as_str()) {
            break;
        }
        chain.push(current);
        total += entry.end - entry.start;
        if total > max_branch_bytes {
            return None;
        }
        // a parent that never appears (a foreign id, a torn index) ends the walk short: what it
        // still resolves to is shown, the unreadable head is not invented
        next = entry.parent.as_deref().and_then(|parent| by_id.get(parent).copied());
    }
    chain.reverse();
    let mut segments: Vec<PiTranscriptSegment> = Vec::new();
    for current in chain {
        let entry = &index.entries[current];
        match segments.last_mut() {
            Some(last) if last.end == entry.start => last.end = entry.end,
            _ => segments.push(PiTranscriptSegment { start: entry.start, end: entry.end }),
        }
    }
    Some(segments)
}

/// The turns on paths a `/tree` walked away from (`pi-tree.ts`: `piAbandonedTurns`).
fn abandoned_branch(index: &PiIndex, size: usize) -> Option<ReferenceAbandonedBranch> {
    let leaf = leaf_of(index, size)?;
    let by_id = entries_by_id(index);
    let mut live: HashSet<&str> = HashSet::new();
    let mut next = Some(leaf);
    while let Some(current) = next {
        let entry = &index.entries[current];
        if !live.insert(entry.id.as_str()) {
            break;
        }
        next = entry.parent.as_deref().and_then(|parent| by_id.get(parent).copied());
    }
    // pi's summary of an abandoned path sits on the branch that replaced it, so only one on the
    // live path describes what the chat is hiding here; the newest wins, as a later /tree
    // supersedes an earlier answer
    let mut summary: Option<String> = None;
    for entry in &index.entries {
        if entry.entry_type != "branch_summary" || !live.contains(entry.id.as_str()) {
            continue;
        }
        if let Some(text) = entry
            .summary
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
        {
            summary = Some(text.to_string());
        }
    }
    // A branch is a place the pointer was moved away from: a live entry with a child left behind,
    // so an abandoned path attached to the path still in play. Attached is the word — pi's session
    // header is a root no entry ever links to, orphaned from the moment it is written, and counting
    // heads of abandoned paths instead would report it as a navigation in every session.
    let mut heads: Vec<usize> = Vec::new();
    let mut head_ids: HashSet<&str> = HashSet::new();
    for entry in &index.entries {
        if !live.contains(entry.id.as_str()) {
            continue;
        }
        let mut left: Vec<usize> = Vec::new();
        let mut seen_left: HashSet<&str> = HashSet::new();
        for child in index.children.get(entry.id.as_str()).into_iter().flatten() {
            let child_entry = &index.entries[*child];
            if live.contains(child_entry.id.as_str()) || !seen_left.insert(child_entry.id.as_str()) {
                continue;
            }
            left.push(*child);
        }
        for child in left {
            if head_ids.insert(index.entries[child].id.as_str()) {
                heads.push(child);
            }
        }
    }
    for (position, entry) in index.entries.iter().enumerate() {
        // a parentless entry other than pi's header, off the live path, with something left under
        // it: navigating the tree to its very first entry starts a path at the root, and abandoning
        // that path is the same navigation as abandoning one mid-tree
        if entry.parent.is_some()
            || entry.entry_type == "session"
            || live.contains(entry.id.as_str())
        {
            continue;
        }
        let has_abandoned_child = index
            .children
            .get(entry.id.as_str())
            .into_iter()
            .flatten()
            .any(|child| !live.contains(index.entries[*child].id.as_str()));
        if has_abandoned_child && head_ids.insert(entry.id.as_str()) {
            heads.push(position);
        }
    }
    let branches = heads.len();
    // The turns are counted INSIDE these subtrees, never over the whole file. A turn whose chain of
    // parents does not lead to a counted head belongs to no branch, and counting it would let the
    // label name a branch for turns no branch holds.
    let mut count = 0u64;
    if branches > 0 {
        let mut owned: HashSet<&str> = HashSet::new();
        let mut counted: HashSet<&str> = HashSet::new();
        for entry in &index.entries {
            let is_head = head_ids.contains(entry.id.as_str());
            let parent_owned = entry
                .parent
                .as_deref()
                .is_some_and(|parent| owned.contains(parent));
            if is_head || parent_owned {
                owned.insert(entry.id.as_str());
            } else {
                continue;
            }
            if entry.entry_type != "message" || counted.contains(entry.id.as_str()) {
                continue;
            }
            // a tool call and its result are one step of the turn that asked for them
            if entry.role != "user" && entry.role != "assistant" {
                continue;
            }
            counted.insert(entry.id.as_str());
            count += 1;
        }
    }
    Some(ReferenceAbandonedBranch { count, branches: branches as u64, summary })
}

// ---------------------------------------------------------------------------------------------
// Small shared helpers
// ---------------------------------------------------------------------------------------------

/// The first value that is a string (`transcript-records.ts`: `string`).
fn first_string(values: &[Option<&Value>]) -> Option<String> {
    values
        .iter()
        .filter_map(|value| *value)
        .find_map(Value::as_str)
        .map(str::to_string)
}

/// A trimmed, non-empty string (`transcript-records.ts`: `label`).
fn trimmed_label(value: Option<&Value>) -> Option<String> {
    let value = value?.as_str()?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// The first `limit` characters of a string.
///
/// Upstream cuts on UTF-16 code units; cutting on characters keeps a multi-byte character whole,
/// which is what the client renders either way.
fn truncate_chars(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        return value.to_string();
    }
    value.chars().take(limit).collect()
}

/// The text of one or more branch segments, in order.
fn branch_text(bytes: &[u8], segments: &[PiTranscriptSegment]) -> String {
    let mut text = String::new();
    for segment in segments {
        if let Some(slice) = bytes.get(segment.start..segment.end) {
            text.push_str(&String::from_utf8_lossy(slice));
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::reference_chat::types::ReferencePartKind;

    /// pi's own record shape, written by the pinned runtime: `message` records on an entry tree,
    /// a compaction entry, a runtime notice, a duplicate tool result and a torn tail.
    const NORMAL: &str = include_str!("fixtures/pi/normal.jsonl");
    /// A tool result that carries an inline image.
    const IMAGE: &str = include_str!("fixtures/pi/image.jsonl");
    /// The legacy `<skill name location>` invocation form.
    const SKILL: &str = include_str!("fixtures/pi/skill.jsonl");
    /// A `context_clear` reset, a `display: false` entry, and a model-change record.
    const RESET: &str = include_str!("fixtures/pi/reset.jsonl");
    /// An entry tree a `/tree` walked away from: the leaf stands on `b*`, `x*` is abandoned.
    const BRANCH: &str = include_str!("fixtures/pi/branch.jsonl");

    fn parts_of(turn: &ReferenceTurn, kind: ReferencePartKind) -> Vec<&ReferencePart> {
        turn.parts.iter().filter(|part| part.kind() == kind).collect()
    }

    fn text_of(turn: &ReferenceTurn) -> String {
        parts_of(turn, ReferencePartKind::Text)
            .iter()
            .filter_map(|part| match part {
                ReferencePart::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<&str>>()
            .join("")
    }

    #[test]
    fn normal_fixture_reads_prompts_tools_notices_and_compaction() {
        let turns = parse_pi_transcript(NORMAL.as_bytes()).expect("the fixture branch is readable");
        assert_eq!(
            turns.iter().map(|turn| turn.role).collect::<Vec<_>>(),
            vec![
                ReferenceTurnRole::User,
                ReferenceTurnRole::Assistant,
                ReferenceTurnRole::User,
                ReferenceTurnRole::Assistant,
                ReferenceTurnRole::User,
                ReferenceTurnRole::User,
                ReferenceTurnRole::Assistant,
            ]
        );
        assert_eq!(text_of(&turns[0]), "Add a /health route to the API.");
        assert_eq!(turns[0].started_at.as_deref(), Some("2026-10-06T09:00:01.000Z"));

        // the assistant records merge, and the tool result fills the call it answers
        let tools = parts_of(&turns[1], ReferencePartKind::Tool);
        assert_eq!(tools.len(), 1);
        match tools[0] {
            ReferencePart::Tool { name, output, summary, error, .. } => {
                assert_eq!(name, "read");
                assert_eq!(summary, "/Users/dev/project/src/api.rs");
                assert!(output.contains("use axum"));
                assert_eq!(*error, None);
            }
            other => panic!("expected a tool part, got {other:?}"),
        }
        assert_eq!(parts_of(&turns[1], ReferencePartKind::Thinking).len(), 1);
        assert_eq!(turns[1].ended_at.as_deref(), Some("2026-10-06T09:00:09.000Z"));

        // the failed request keeps its error, so the prompt is never left with no answer
        assert!(text_of(&turns[3]).contains("Error: 401 unauthorized from provider"));

        // a runtime notice and a compaction each open their own turn
        match parts_of(&turns[4], ReferencePartKind::Notice).first() {
            Some(ReferencePart::Notice { text, source }) => {
                assert_eq!(text, "background job finished");
                assert_eq!(source.as_deref(), Some("async-result"));
            }
            other => panic!("expected a notice, got {other:?}"),
        }
        match parts_of(&turns[5], ReferencePartKind::Compact).first() {
            Some(ReferencePart::Compact { text }) => {
                assert_eq!(text, "Folded the earlier API discussion into a summary.");
            }
            other => panic!("expected a compaction, got {other:?}"),
        }
        // the torn tail line is not a turn, and the assistant after the compaction is
        assert_eq!(text_of(&turns[6]), "torn tail while pi is mid-append");
    }

    #[test]
    fn duplicate_tool_result_does_not_replace_a_settled_tool() {
        let turns = parse_pi_transcript(NORMAL.as_bytes()).expect("readable");
        let output = match parts_of(&turns[1], ReferencePartKind::Tool)[0] {
            ReferencePart::Tool { output, .. } => output.clone(),
            other => panic!("expected a tool part, got {other:?}"),
        };
        assert!(output.contains("use axum"));
        assert!(!output.contains("duplicate result later in the log"));
    }

    #[test]
    fn skill_invocation_keeps_the_request_and_chips_the_skill() {
        let turns = parse_pi_transcript(SKILL.as_bytes()).expect("readable");
        assert_eq!(turns.len(), 2);
        assert_eq!(text_of(&turns[0]), "Please review the branch.");
        assert!(!text_of(&turns[0]).contains("# Review"));
        // the skill the runtime loaded rides the request as its own part
        assert_eq!(turns[0].parts.len(), 2, "the request then the chip");
        let ReferencePart::Skill { skill } = &turns[0].parts[1] else {
            panic!("the second part is the skill chip, got {:?}", turns[0].parts[1].kind());
        };
        assert_eq!(skill.name, "review");
        assert_eq!(skill.evidence, ReferenceSkillEvidence::Instructions);
        assert_eq!(skill.status, ReferenceSkillStatus::Loaded);
        assert_eq!(skill.path.as_deref(), Some("/Users/dev/.agents/skills/review/SKILL.md"));

        // the reference's own envelope form names the skill twice and carries the request last
        let prompt = r#"The user explicitly invoked the "frontend" skill. Follow the instructions in <skill-instruction> as binding for this request, while respecting higher-priority instructions.

<skill-instruction name="frontend" location="/Users/dev/.agents/skills/frontend/SKILL.md">
# Frontend
</skill-instruction>

<user-request>
Style the badge.
</user-request>"#;
        let invocation = pi_skill_invocation(prompt).expect("the envelope is recognized");
        assert_eq!(invocation.skills.len(), 1);
        assert_eq!(invocation.skills[0].name, "frontend");
        assert_eq!(
            invocation.skills[0].path.as_deref(),
            Some("/Users/dev/.agents/skills/frontend/SKILL.md")
        );
        assert_eq!(invocation.skills[0].evidence, ReferenceSkillEvidence::Instructions);
        assert_eq!(invocation.skills[0].status, ReferenceSkillStatus::Loaded);
        assert_eq!(invocation.request, "Style the badge.");

        // prose that merely mentions a skill is the user's own text, with no chip on it
        assert!(pi_skill_invocation("please use the frontend skill").is_none());
    }

    #[test]
    fn context_clear_drops_every_turn_before_it_and_hidden_entries_stay_hidden() {
        let turns = parse_pi_transcript(RESET.as_bytes()).expect("readable");
        assert_eq!(turns.len(), 2);
        assert_eq!(text_of(&turns[0]), "After the reset.");
        assert_eq!(text_of(&turns[1]), "Answer after the reset.");
        assert!(!turns.iter().any(|turn| text_of(turn).contains("Hidden by the display flag")));
    }

    #[test]
    fn tool_images_carry_a_pi_ref() {
        let turns = parse_pi_transcript(IMAGE.as_bytes()).expect("readable");
        let images = match parts_of(&turns[1], ReferencePartKind::Tool)[0] {
            ReferencePart::Tool { images, output, .. } => {
                assert_eq!(output, "a 2x2 screenshot");
                images.clone()
            }
            other => panic!("expected a tool part, got {other:?}"),
        };
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].media_type, "image/png");
        assert_eq!(images[0].r#ref, "pi:call_img:0");
    }

    #[test]
    fn unknown_image_media_types_are_not_offered() {
        let text = concat!(
            r#"{"type":"message","id":"a","message":{"role":"assistant","content":[{"type":"toolCall","toolCallId":"c","toolName":"view","toolInput":{}}]}}"#,
            "\n",
            r#"{"type":"message","id":"b","message":{"role":"toolResult","toolCallId":"c","content":[{"type":"image","mimeType":"image/svg+xml","data":"PHN2Zz4="},{"type":"text","text":"ok"}]}}"#,
            "\n"
        );
        let turns = parse_pi_history(ReferenceNativeHistoryKind::Pi, text).expect("readable");
        match parts_of(&turns[0], ReferencePartKind::Tool)[0] {
            ReferencePart::Tool { images, output, .. } => {
                assert!(images.is_empty());
                assert_eq!(output, "ok");
            }
            other => panic!("expected a tool part, got {other:?}"),
        }
    }

    #[test]
    fn branch_fixture_projects_only_the_live_path() {
        let segments = pi_transcript_segments(BRANCH.as_bytes()).expect("the branch is readable");
        assert_eq!(segments.len(), 2, "the abandoned entries split the branch in two");

        let turns = parse_pi_transcript(BRANCH.as_bytes()).expect("readable");
        let rendered: String =
            turns.iter().map(text_of).collect::<Vec<String>>().join("\n");
        assert!(rendered.contains("Second answer (kept)."));
        assert!(rendered.contains("Third answer on the chosen path."));
        assert!(!rendered.contains("Alternative answer (abandoned)."));
        assert!(!rendered.contains("Alternative question (abandoned)."));

        // the abandoned path's tool call is not offered either
        assert_eq!(pi_tool_output(BRANCH.as_bytes(), "call_b1").as_deref(), Some("kept output"));
        assert_eq!(pi_tool_output(BRANCH.as_bytes(), "call_x1"), None);
    }

    #[test]
    fn branch_fixture_counts_abandoned_turns_and_its_summary() {
        let abandoned =
            pi_abandoned_branch(BRANCH.as_bytes()).expect("the tree can be walked");
        // one abandoned path: its user turn and its assistant turn; the tool result is not a turn
        assert_eq!(abandoned.count, 2);
        assert_eq!(abandoned.branches, 1);
        assert_eq!(
            abandoned.summary.as_deref(),
            Some("Chose the direct path over the alternative.")
        );
    }

    #[test]
    fn a_straight_session_discloses_nothing() {
        let abandoned = pi_abandoned_branch(NORMAL.as_bytes()).expect("the tree can be walked");
        assert_eq!(abandoned.count, 0);
        assert_eq!(abandoned.branches, 0);
        assert_eq!(abandoned.summary, None);
    }

    #[test]
    fn a_branch_over_the_limit_is_unreadable() {
        assert_eq!(pi_transcript_segments_with_limit(BRANCH.as_bytes(), 4), None);
        assert!(parse_pi_transcript(BRANCH.as_bytes()).is_ok());
    }

    #[test]
    fn an_empty_file_has_no_branch() {
        assert_eq!(pi_transcript_segments(b""), None);
        assert_eq!(pi_abandoned_branch(b""), None);
        assert_eq!(pi_tool_output(b"", "call_1"), None);
        assert!(parse_pi_transcript(b"").is_err());
    }

    #[test]
    fn image_refs_decode_on_the_active_branch() {
        let image = pi_transcript_image(IMAGE.as_bytes(), "pi:call_img:0").expect("the image decodes");
        assert_eq!(image.media_type, "image/png");
        // the fixture holds the PNG signature; a ref the file does not carry answers nothing
        assert_eq!(image.data.len(), 8);
        assert_eq!(image.data[0], 0x89);
        assert_eq!(pi_transcript_image(IMAGE.as_bytes(), "pi:call_img:9"), None);
        assert_eq!(pi_transcript_image(IMAGE.as_bytes(), "pi:call_other:0"), None);
        assert_eq!(pi_transcript_image(BRANCH.as_bytes(), "pi:call_x1:0"), None);
    }

    #[test]
    fn a_long_tool_output_is_cut_and_keeps_its_ref() {
        let long = "x".repeat(PI_TOOL_OUTPUT_CHARS + 1);
        let text = format!(
            "{}\n{}\n",
            r#"{"type":"message","id":"a","message":{"role":"assistant","content":[{"type":"toolCall","toolCallId":"c","toolName":"bash","toolInput":{"command":"ls"}}]}}"#,
            serde_json::json!({
                "type": "message",
                "id": "b",
                "message": { "role": "toolResult", "toolCallId": "c", "content": [{ "type": "text", "text": long }] }
            })
        );
        let turns = parse_pi_history(ReferenceNativeHistoryKind::Pi, &text).expect("readable");
        match parts_of(&turns[0], ReferencePartKind::Tool)[0] {
            ReferencePart::Tool { output, output_ref, output_size, .. } => {
                assert!(output.ends_with("… trimmed"));
                assert_eq!(output_ref.as_deref(), Some("c"));
                assert_eq!(*output_size, Some((PI_TOOL_OUTPUT_CHARS + 1) as u64));
            }
            other => panic!("expected a tool part, got {other:?}"),
        }
        // and the whole output is still reachable from the file by that ref
        assert_eq!(
            pi_tool_output(text.as_bytes(), "c").map(|output| output.len()),
            Some(PI_TOOL_OUTPUT_CHARS + 1)
        );
    }

    #[test]
    fn a_goal_tool_keeps_its_whole_answer() {
        let goal = "y".repeat(PI_TOOL_OUTPUT_CHARS + 1);
        let text = format!(
            "{}\n{}\n",
            r#"{"type":"message","id":"a","message":{"role":"assistant","content":[{"type":"toolCall","toolCallId":"g","toolName":"get_goal","toolInput":{}}]}}"#,
            serde_json::json!({
                "type": "message",
                "id": "b",
                "message": { "role": "toolResult", "toolCallId": "g", "content": [{ "type": "text", "text": goal }] }
            })
        );
        let turns = parse_pi_history(ReferenceNativeHistoryKind::Pi, &text).expect("readable");
        match parts_of(&turns[0], ReferencePartKind::Tool)[0] {
            ReferencePart::Tool { output, output_ref, .. } => {
                assert_eq!(output.chars().count(), PI_TOOL_OUTPUT_CHARS + 1);
                assert_eq!(*output_ref, None);
            }
            other => panic!("expected a tool part, got {other:?}"),
        }
    }

    #[test]
    fn a_kind_this_lane_does_not_read_is_an_error() {
        let error = parse_pi_history(ReferenceNativeHistoryKind::Claude, NORMAL)
            .expect_err("the claude family is another lane's");
        assert!(error.starts_with(PI_UNSUPPORTED_KIND));
        assert!(parse_pi_history(ReferenceNativeHistoryKind::Unavailable, "").is_err());
    }

    #[test]
    fn the_lane_exposes_the_contract_parser() {
        let parser: ReferenceHistoryParser = pi_history_parser();
        let turns = parser(ReferenceNativeHistoryKind::Pi, NORMAL).expect("readable");
        assert_eq!(turns.len(), parse_pi_history(ReferenceNativeHistoryKind::Pi, NORMAL).unwrap().len());
    }
}
