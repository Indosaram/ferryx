//! `[CC]` native transcript family for the Herdr reference-chat port.
//!
//! Ported from `devswha/herdr-web-ui` @ `54e5a1f67090cb09552d182e7e30dd0ecc314918`
//! (MIT, `docs/chat/HERDR_LICENSE`), the pure-parsing half of the pinned Claude reader:
//!
//! * `server/conversation.ts` — `isCommandEntry` (`:73-75`), `unwrapPastes` (`:81-91`),
//!   `claudeResultText` (`:103-108`), `IMAGE_TYPES` (`:110`), `parseClaudeTranscript`
//!   (`:117-226`), `TURN_MARK["claude-transcript"]` (`:427`) and `opensTurn` (`:449-452`).
//! * `server/transcript-records.ts` — `isContextClear` (`:15-20`), `MAX_TURNS` (`:129`),
//!   `label` (`:94`), `toolSummary` (`:153-163`).
//! * `server/tool-output.ts` — `trimOutput`, `TOOL_OUTPUT_CHARS`, `WHOLE_OUTPUT_TOOLS`,
//!   `WHOLE_OUTPUT_CHARS`.
//! * `server/skill-activity.ts` — `invokedSkill`.
//!
//! Upstream SHA-256 for every file above: `docs/chat/herdr-port-contract.md` §7. Fixture
//! inventory and the deliberate divergences: `fixtures/claude/MANIFEST.md`.
//!
//! The pinned reader is **pure**: `parseClaudeTranscript(text)` is handed a transcript the
//! caller already resolved from `~/.claude/projects/<project>/<session>.jsonl`. This module
//! ports that half only. It reads nothing from disk, resolves no session, and never falls back
//! to another session's file — an unresolved transcript is the resolver's error (task 3), not
//! an empty success here. `Unavailable` and every other family's kind are refused, because an
//! empty success would be read as "this session has no turns" (contract §4).
//!
//! Nothing was reused from `crate::agent_transcript`: that reader accepts only omp/pi-shaped
//! `type: "message"` records under `~/.omo/agent/sessions` and yields flat
//! `ConversationMessage { role, text }`, so it cannot represent a Claude transcript's tool,
//! thinking, image, compaction or queued-prompt records. The DTOs are task 1's `types.rs`,
//! used as shipped.

use std::sync::OnceLock;

use regex::Regex;
use serde_json::{Map, Value};

use super::types::{
    ReferenceHistoryParser, ReferenceNativeHistoryKind, ReferencePart, ReferenceSkillActivity,
    ReferenceSkillEvidence, ReferenceSkillStatus, ReferenceTurn, ReferenceTurnRole,
};

/// Upstream `MAX_TURNS`: enough turns for a conversation.
pub const REFERENCE_CLAUDE_MAX_TURNS: usize = 100;

/// Upstream `TOOL_OUTPUT_CHARS`: past this a tool's output is cut in the page; the rest is
/// fetched on request through the tool part's `output_ref`.
pub const REFERENCE_CLAUDE_TOOL_OUTPUT_CHARS: usize = 4_000;

/// Upstream `WHOLE_OUTPUT_CHARS`: the wider cut for the tools whose whole answer is JSON.
pub const REFERENCE_CLAUDE_WHOLE_OUTPUT_CHARS: usize = 16_000;

/// Upstream `WHOLE_OUTPUT_TOOLS`: omo's goal calls answer with the goal as JSON the chat reads,
/// its objective alone up to 4000 characters, so cutting at the usual length lost a finished
/// goal's status.
const WHOLE_OUTPUT_TOOLS: [&str; 3] = ["create_goal", "update_goal", "get_goal"];

/// Upstream `IMAGE_TYPES`: the image types a chat shows; anything else stays out of the page.
const IMAGE_TYPES: [&str; 4] = ["image/png", "image/jpeg", "image/gif", "image/webp"];

/// The bytes every line that opens a Claude turn contains (upstream `TURN_MARK`). Page
/// boundaries (task 3) start on such lines, so a page never splits a turn.
pub const REFERENCE_CLAUDE_TURN_MARK: &str = "\"user\"";

/// The lane's parser, satisfying task 1's [`ReferenceHistoryParser`] alias. The dispatcher
/// (task 2) routes by kind; a family that is handed another kind must refuse rather than
/// silently parse it.
pub fn parse_reference_claude_history(
    kind: ReferenceNativeHistoryKind,
    text: &str,
) -> Result<Vec<ReferenceTurn>, String> {
    if kind != ReferenceNativeHistoryKind::Claude {
        return Err(format!(
            "the claude history family reads claude-transcript only, not {}",
            kind.as_str()
        ));
    }
    parse_claude_transcript(text)
}

/// Compile-time proof that the lane satisfies the frozen signature (contract §6).
const _: ReferenceHistoryParser = parse_reference_claude_history;

/// Upstream `parseClaudeTranscript`: split one Claude transcript into turns, keeping the newest
/// [`REFERENCE_CLAUDE_MAX_TURNS`].
///
/// `text` is the whole transcript, exactly as the pinned reader reads it: a torn tail line while
/// Claude is mid-append is skipped, never repaired. A transcript is always parseable, so the
/// result is an error only when the caller asks this lane for a kind it does not read.
pub fn parse_claude_transcript(text: &str) -> Result<Vec<ReferenceTurn>, String> {
    Ok(parse_claude_transcript_with_limit(
        text,
        REFERENCE_CLAUDE_MAX_TURNS,
    ))
}

/// Upstream `parseClaudeTranscript(text, maxTurns)`: the same parse with the tail window the
/// caller chooses (task 3 pages a stream at a time). `maxTurns == 0` keeps every turn, matching
/// the pinned reader's `slice(-0)`.
///
/// Adjacent assistant entries merge into a single turn (text, thinking and tool parts); each
/// `tool_use` is followed by the `user` `tool_result` entry that answers it, folded into the
/// tool part by `tool_use_id`. A `/clear` local-command envelope resets the conversation *and*
/// the pending results, a compaction summary becomes a `compact` part, and a prompt Claude
/// queued while it was working becomes the user turn it really was. A turn with no parts is
/// dropped, so bookkeeping records never fabricate an empty turn.
pub fn parse_claude_transcript_with_limit(text: &str, max_turns: usize) -> Vec<ReferenceTurn> {
    let mut turns: Vec<ReferenceTurn> = Vec::new();
    let mut pending: Vec<(String, usize, usize)> = Vec::new();

    for line in text.split('\n') {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(entry) = entry.as_object() else {
            continue;
        };
        if truthy(entry.get("isMeta")) {
            continue;
        }
        let timestamp = entry
            .get("timestamp")
            .and_then(Value::as_str)
            .map(str::to_string);

        if is_claude_context_clear(entry) {
            turns.clear();
            pending.clear();
            continue;
        }

        let content = entry.get("message").and_then(|message| message.get("content"));

        if truthy(entry.get("isCompactSummary")) {
            let summary = match content {
                Some(Value::String(summary)) => summary.clone(),
                Some(Value::Array(blocks)) => blocks
                    .iter()
                    .map(|block| {
                        if block.get("type").and_then(Value::as_str) == Some("text") {
                            js_string_or_empty(block.get("text"))
                        } else {
                            String::new()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
                _ => String::new(),
            };
            turns.push(ReferenceTurn {
                role: ReferenceTurnRole::User,
                started_at: timestamp,
                ended_at: None,
                source: None,
                parts: vec![ReferencePart::Compact { text: summary }],
                abandoned: None,
            });
            continue;
        }

        if entry.get("type").and_then(Value::as_str) == Some("attachment") {
            let queued = entry.get("attachment").and_then(Value::as_object);
            let is_human_prompt = queued
                .map(|queued| {
                    queued.get("type").and_then(Value::as_str) == Some("queued_command")
                        && queued.get("commandMode").and_then(Value::as_str) == Some("prompt")
                        && queued
                            .get("origin")
                            .and_then(Value::as_object)
                            .and_then(|origin| origin.get("kind"))
                            .and_then(Value::as_str)
                            == Some("human")
                })
                .unwrap_or(false);
            if is_human_prompt {
                if let Some(prompt) = queued
                    .and_then(|queued| queued.get("prompt"))
                    .and_then(Value::as_str)
                {
                    if !prompt.trim().is_empty() && !is_claude_command_entry(prompt.trim()) {
                        turns.push(user_turn(
                            timestamp,
                            vec![ReferencePart::Text {
                                text: unwrap_claude_pastes(prompt),
                                phase: None,
                            }],
                        ));
                    }
                }
                continue;
            }
        }

        if entry.get("type").and_then(Value::as_str) == Some("user") {
            match content {
                Some(Value::String(content)) => {
                    if is_claude_command_entry(content) {
                        continue;
                    }
                    turns.push(user_turn(
                        timestamp,
                        vec![ReferencePart::Text {
                            text: unwrap_claude_pastes(content),
                            phase: None,
                        }],
                    ));
                    continue;
                }
                Some(Value::Array(blocks)) => {
                    let prompt = blocks
                        .iter()
                        .filter_map(|block| {
                            if block.get("type").and_then(Value::as_str) != Some("text") {
                                return None;
                            }
                            let text = block.get("text").and_then(Value::as_str)?;
                            if is_claude_command_entry(text.trim()) {
                                None
                            } else {
                                Some(text.to_string())
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\n");

                    for block in blocks {
                        if block.get("type").and_then(Value::as_str) != Some("tool_result") {
                            continue;
                        }
                        let Some(tool_use_id) = block.get("tool_use_id").and_then(Value::as_str)
                        else {
                            continue;
                        };
                        let Some(index) = pending
                            .iter()
                            .rposition(|(key, _, _)| key == tool_use_id)
                        else {
                            continue;
                        };
                        let (_, turn_index, part_index) = pending.remove(index);
                        let is_error =
                            block.get("is_error").and_then(Value::as_bool) == Some(true);
                        let output = claude_result_text(
                            block.get("content").unwrap_or(&Value::Null),
                        );
                        if let Some(part) = turns
                            .get_mut(turn_index)
                            .and_then(|turn| turn.parts.get_mut(part_index))
                        {
                            fold_claude_tool_result(part, output, tool_use_id, is_error);
                        }
                    }

                    let images: Vec<ReferencePart> = match entry
                        .get("uuid")
                        .and_then(Value::as_str)
                    {
                        None => Vec::new(),
                        Some(uuid) => blocks
                            .iter()
                            .enumerate()
                            .filter_map(|(index, block)| {
                                if block.get("type").and_then(Value::as_str) != Some("image") {
                                    return None;
                                }
                                let source = block.get("source").and_then(Value::as_object)?;
                                if source.get("type").and_then(Value::as_str) != Some("base64") {
                                    return None;
                                }
                                let media_type = source.get("media_type")?.as_str()?;
                                if !IMAGE_TYPES.contains(&media_type) {
                                    return None;
                                }
                                Some(ReferencePart::Image {
                                    media_type: media_type.to_string(),
                                    r#ref: format!("{uuid}:{index}"),
                                })
                            })
                            .collect(),
                    };

                    if !prompt.trim().is_empty() || !images.is_empty() {
                        let mut parts = images;
                        if !prompt.trim().is_empty() {
                            parts.push(ReferencePart::Text {
                                text: unwrap_claude_pastes(&prompt),
                                phase: None,
                            });
                        }
                        turns.push(user_turn(timestamp, parts));
                    }
                    continue;
                }
                _ => {}
            }
        }

        if entry.get("type").and_then(Value::as_str) == Some("assistant") {
            let Some(Value::Array(blocks)) = content else {
                continue;
            };
            let turn_index = assistant_turn(&mut turns, timestamp.as_deref());
            if let Some(timestamp) = timestamp.as_deref() {
                if let Some(turn) = turns.get_mut(turn_index) {
                    turn.ended_at = Some(timestamp.to_string());
                }
            }
            for block in blocks {
                let Some(block) = block.as_object() else {
                    continue;
                };
                match block.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(text) = block.get("text").and_then(Value::as_str) {
                            if !text.is_empty() {
                                push_part(
                                    &mut turns,
                                    turn_index,
                                    ReferencePart::Text {
                                        text: text.to_string(),
                                        phase: None,
                                    },
                                );
                            }
                        }
                    }
                    Some("thinking") => {
                        let thinking = block
                            .get("thinking")
                            .and_then(Value::as_str)
                            .or_else(|| block.get("text").and_then(Value::as_str))
                            .unwrap_or("");
                        if !thinking.is_empty() {
                            push_part(
                                &mut turns,
                                turn_index,
                                ReferencePart::Thinking {
                                    text: thinking.to_string(),
                                },
                            );
                        }
                    }
                    Some("tool_use") => {
                        let Some(name) = block.get("name").and_then(Value::as_str) else {
                            continue;
                        };
                        let raw_input = block.get("input").cloned().unwrap_or(Value::Null);
                        let input = raw_input.as_object().cloned().unwrap_or_default();
                        let mut part = ReferencePart::Tool {
                            name: name.to_string(),
                            summary: claude_tool_summary(name, &input),
                            input: stringify_pretty(&Value::Object(input.clone())),
                            output: String::new(),
                            error: None,
                            skill: None,
                            output_ref: None,
                            output_size: None,
                            images: Vec::new(),
                        };
                        if let Some(skill) = invoked_skill(name, &input) {
                            if let ReferencePart::Tool {
                                summary,
                                skill: slot,
                                ..
                            } = &mut part
                            {
                                *summary = skill.name.clone();
                                *slot = Some(skill);
                            }
                        }
                        push_part(&mut turns, turn_index, part);
                        let part_index = turns
                            .get(turn_index)
                            .map(|turn| turn.parts.len().saturating_sub(1))
                            .unwrap_or(0);
                        pending.push((claude_tool_key(block.get("id")), turn_index, part_index));
                    }
                    _ => {}
                }
            }
        }
    }

    turns.retain(|turn| !turn.parts.is_empty());
    if max_turns > 0 && turns.len() > max_turns {
        return turns.split_off(turns.len() - max_turns);
    }
    turns
}

/// Upstream `opensTurn(source, line)` for `claude-transcript`: does this raw line open a turn?
/// Task 3's pager uses it (with [`REFERENCE_CLAUDE_TURN_MARK`] as the cheap pre-filter) so a page
/// never starts mid-turn. A `/clear` envelope is not a turn opener; it is a boundary.
pub fn opens_claude_turn(line: &str) -> bool {
    let Ok(entry) = serde_json::from_str::<Value>(line) else {
        return false;
    };
    let Some(entry) = entry.as_object() else {
        return false;
    };
    if entry.get("type").and_then(Value::as_str) != Some("user") {
        return false;
    }
    if truthy(entry.get("isMeta")) || truthy(entry.get("isCompactSummary")) {
        return false;
    }
    match entry
        .get("message")
        .and_then(Value::as_object)
        .and_then(|message| message.get("content"))
    {
        Some(Value::String(content)) => !is_claude_command_entry(content),
        Some(Value::Array(blocks)) => blocks.iter().any(|block| {
            block.get("type").and_then(Value::as_str) == Some("text")
                && block
                    .get("text")
                    .and_then(Value::as_str)
                    .map(|text| !is_claude_command_entry(text.trim()))
                    .unwrap_or(false)
        }),
        _ => false,
    }
}

/// Slash-command and bookkeeping entries Claude logs as user turns — not conversations.
pub fn is_claude_command_entry(text: &str) -> bool {
    text.starts_with("<command-") || text.starts_with("<local-command") || text.starts_with("<task-")
}

/// Upstream `unwrapPastes`: Claude wraps a long paste in `<pasted_content id="…">` tags so the
/// model can tell it from typed text; its own TUI shows only the text, and so does the chat.
///
/// Only a tag pair whose ids match, are at most 64 characters, and are word-shaped is unwrapped;
/// anything else is left exactly as it came. The text is returned unchanged unless at least one
/// pair unwrapped, and then only its surrounding newlines are trimmed.
pub fn unwrap_claude_pastes(text: &str) -> String {
    let mut unwrapped = false;
    let visible = paste_tag()
        .replace_all(text, |captures: &regex::Captures<'_>| {
            let opening = &captures[1];
            let closing = &captures[3];
            if opening != closing || !is_word_shaped_id(opening) {
                return captures[0].to_string();
            }
            unwrapped = true;
            captures[2].to_string()
        })
        .into_owned();
    if !unwrapped {
        return text.to_string();
    }
    visible.trim_matches('\n').to_string()
}

/// Upstream `toolSummary`: the one line a tool call shows — an omo/omp `task` call's own titles,
/// else the first recognized field, else the tool's name.
pub fn claude_tool_summary(name: &str, input: &Map<String, Value>) -> String {
    if name == "task" {
        let empty = Map::new();
        let items: Vec<&Map<String, Value>> = match input.get("tasks") {
            Some(Value::Array(tasks)) => tasks
                .iter()
                .map(|task| task.as_object().unwrap_or(&empty))
                .collect(),
            _ => vec![input],
        };
        let titles: Vec<String> = items
            .iter()
            .filter_map(|item| {
                plain_label(item.get("task_summary"))
                    .or_else(|| plain_label(item.get("description")))
            })
            .collect();
        if !titles.is_empty() {
            return cut_chars(&titles.join(" · "), 120);
        }
    }
    const FIELDS: [&str; 7] = [
        "command",
        "file_path",
        "notebook_path",
        "path",
        "pattern",
        "description",
        "url",
    ];
    for field in FIELDS {
        if let Some(Value::String(value)) = input.get(field) {
            return cut_chars(value, 120);
        }
    }
    name.to_string()
}

/// Upstream `isContextClear(entry, "claude-transcript")`: a whole local-command envelope around
/// `/clear` resets the conversation. Quoting `/clear` in prose is not a reset.
pub fn is_claude_context_clear(entry: &Map<String, Value>) -> bool {
    if entry.get("type").and_then(Value::as_str) != Some("user") {
        return false;
    }
    if truthy(entry.get("isMeta")) || truthy(entry.get("isCompactSummary")) {
        return false;
    }
    let Some(message) = entry.get("message").and_then(Value::as_object) else {
        return false;
    };
    if message.get("role").and_then(Value::as_str) != Some("user") {
        return false;
    }
    let Some(content) = message.get("content").and_then(Value::as_str) else {
        return false;
    };
    clear_command().is_match(content)
}

/// A new user-seat turn with no `source`: every Claude user record is a typed prompt or a
/// compaction summary, and the pinned reader marks neither as runtime-authored.
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

/// Upstream `assistantTurn`: adjacent assistant entries merge into one turn, and the turn keeps
/// the first entry's timestamp while its end follows the last activity.
fn assistant_turn(turns: &mut Vec<ReferenceTurn>, started_at: Option<&str>) -> usize {
    if let Some(last) = turns.last() {
        if last.role == ReferenceTurnRole::Assistant {
            return turns.len() - 1;
        }
    }
    turns.push(ReferenceTurn {
        role: ReferenceTurnRole::Assistant,
        started_at: started_at.map(str::to_string),
        ended_at: None,
        source: None,
        parts: Vec::new(),
        abandoned: None,
    });
    turns.len() - 1
}

fn push_part(turns: &mut Vec<ReferenceTurn>, turn_index: usize, part: ReferencePart) {
    if let Some(turn) = turns.get_mut(turn_index) {
        turn.parts.push(part);
    }
}

/// Upstream `trimOutput`: set a tool part's output, cut to what a page carries, keeping what it
/// takes to fetch the rest. The cut counts characters, never bytes, so a multi-byte character is
/// never split.
fn fold_claude_tool_result(
    part: &mut ReferencePart,
    output: String,
    reference: &str,
    is_error: bool,
) {
    let ReferencePart::Tool {
        name,
        output: slot,
        output_ref,
        output_size,
        error,
        skill,
        ..
    } = part
    else {
        return;
    };
    let limit = if WHOLE_OUTPUT_TOOLS.contains(&name.as_str()) {
        REFERENCE_CLAUDE_WHOLE_OUTPUT_CHARS
    } else {
        REFERENCE_CLAUDE_TOOL_OUTPUT_CHARS
    };
    let length = output.chars().count();
    if length <= limit {
        *slot = output;
    } else {
        *slot = format!("{}\n… trimmed", cut_chars(&output, limit));
        *output_ref = Some(reference.to_string());
        *output_size = Some(length as u64);
    }
    if is_error {
        *error = Some(true);
    }
    if let Some(skill) = skill {
        skill.status = if is_error {
            ReferenceSkillStatus::Failed
        } else {
            ReferenceSkillStatus::Loaded
        };
    }
}

/// Upstream `claudeResultText`: a result's content is a string or an array of text parts.
fn claude_result_text(output: &Value) -> String {
    match output {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .map(|part| match part.get("text") {
                Some(text) => js_string(text),
                None => String::new(),
            })
            .collect(),
        _ => String::new(),
    }
}

/// The key a `tool_use` is filed under until its result arrives: Claude's own `id`, and the
/// empty key when the record carries none (the pinned reader's `String(id ?? "")`).
fn claude_tool_key(id: Option<&Value>) -> String {
    match id {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(id)) => id.clone(),
        Some(other) => js_string(other),
    }
}

/// Upstream `invokedSkill`: evidence that the agent asked for a skill, not a claim that the
/// skill's workflow completed.
fn invoked_skill(name: &str, input: &Map<String, Value>) -> Option<ReferenceSkillActivity> {
    if name != "Skill" {
        return None;
    }
    let skill = skill_label(input.get("skill"))?;
    Some(ReferenceSkillActivity {
        name: skill,
        evidence: ReferenceSkillEvidence::Invocation,
        status: ReferenceSkillStatus::Requested,
        path: None,
    })
}

/// Upstream `transcript-records.ts:94` `label`, which `toolSummary` uses: a non-blank string,
/// trimmed.
fn plain_label(value: Option<&Value>) -> Option<String> {
    let trimmed = value?.as_str()?.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}

/// Upstream `skill-activity.ts` `label`, which `invokedSkill` uses: the same trim, but a skill
/// name is a short single-line label, so anything long or carrying a line break or an angle
/// bracket is refused rather than shown as a skill.
fn skill_label(value: Option<&Value>) -> Option<String> {
    let trimmed = value?.as_str()?.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 200 || trimmed.contains(['\r', '\n', '<', '>']) {
        return None;
    }
    Some(trimmed.to_string())
}

/// JavaScript truthiness, which the pinned parser uses for `isMeta` / `isCompactSummary`: only
/// `false`, `0`, `""`, `null` and absence are falsy.
fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(value)) => *value,
        Some(Value::Number(value)) => value.as_f64().map(|number| number != 0.0).unwrap_or(true),
        Some(Value::String(value)) => !value.is_empty(),
        Some(Value::Array(_)) | Some(Value::Object(_)) => true,
    }
}

/// `String(value)` for a JSON scalar. Objects and arrays have no protocol meaning in these
/// fields and read as empty text rather than JavaScript's `"[object Object]"` (manifest,
/// divergence 4).
fn js_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Bool(flag) => flag.to_string(),
        Value::Null => "null".to_string(),
        Value::Array(_) | Value::Object(_) => String::new(),
    }
}

/// `String(value ?? "")`: absence and `null` read as empty.
fn js_string_or_empty(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(value) => js_string(value),
    }
}

fn cut_chars(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

/// Upstream `^[\w-]+$` on a paste id: word characters and hyphens, at most 64 of them.
fn is_word_shaped_id(id: &str) -> bool {
    !id.is_empty()
        && id.chars().count() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn paste_tag() -> &'static Regex {
    static TAG: OnceLock<Regex> = OnceLock::new();
    TAG.get_or_init(|| {
        Regex::new(
            r#"<pasted_content id="([^"\r\n]+)">\r?\n([\s\S]*?)\r?\n</pasted_content id="([^"\r\n]+)">"#,
        )
        .expect("the pinned paste-tag pattern is a valid regex")
    })
}

fn clear_command() -> &'static Regex {
    static CLEAR: OnceLock<Regex> = OnceLock::new();
    CLEAR.get_or_init(|| {
        Regex::new(
            r"^\s*<command-name>\s*/clear\s*</command-name>(?:\s*<command-message>clear</command-message>)?(?:\s*<command-args>\s*</command-args>)?\s*$",
        )
        .expect("the pinned clear-envelope pattern is a valid regex")
    })
}

/// The pretty JSON the reference writes for a tool call's input: `JSON.stringify(input, null, 2)`.
/// Reproduced from the pinned writer — record order, not `serde_json`'s ordered maps — so a
/// ported page's bytes match the reference's (manifest, divergence 5).
fn stringify_pretty(value: &Value) -> String {
    let mut out = String::new();
    write_json(value, 0, &mut out);
    out
}

fn write_json(value: &Value, indent: usize, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
        Value::Number(number) => out.push_str(&number.to_string()),
        Value::String(text) => write_json_string(text, out),
        Value::Array(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push('\n');
                push_indent(indent + 1, out);
                write_json(item, indent + 1, out);
            }
            out.push('\n');
            push_indent(indent, out);
            out.push(']');
        }
        Value::Object(map) => {
            if map.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push('{');
            for (index, (key, item)) in map.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push('\n');
                push_indent(indent + 1, out);
                write_json_string(key, out);
                out.push_str(": ");
                write_json(item, indent + 1, out);
            }
            out.push('\n');
            push_indent(indent, out);
            out.push('}');
        }
    }
}

fn push_indent(indent: usize, out: &mut String) {
    for _ in 0..indent {
        out.push_str("  ");
    }
}

/// `JSON.stringify` string escaping: quotes, backslashes, the short control escapes, `\u00xx` for
/// the rest, and everything printable — including non-ASCII — as itself.
fn write_json_string(text: &str, out: &mut String) {
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{0c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            character if (character as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => out.push(character),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const NORMAL: &str = include_str!("fixtures/claude/normal.jsonl");
    const QUEUED: &str = include_str!("fixtures/claude/queued.jsonl");
    const BRANCH: &str = include_str!("fixtures/claude/branch.jsonl");
    const PARTIAL: &str = include_str!("fixtures/claude/partial.jsonl");
    const ASSISTANT: &str = include_str!("fixtures/claude/assistant.jsonl");

    fn text(value: &str) -> ReferencePart {
        ReferencePart::Text {
            text: value.to_string(),
            phase: None,
        }
    }

    fn thinking(value: &str) -> ReferencePart {
        ReferencePart::Thinking {
            text: value.to_string(),
        }
    }

    fn tool(
        name: &str,
        summary: &str,
        input: &str,
        output: &str,
        error: Option<bool>,
        skill: Option<ReferenceSkillActivity>,
    ) -> ReferencePart {
        ReferencePart::Tool {
            name: name.to_string(),
            summary: summary.to_string(),
            input: input.to_string(),
            output: output.to_string(),
            error,
            skill,
            output_ref: None,
            output_size: None,
            images: Vec::new(),
        }
    }

    fn plain_tool(name: &str, summary: &str, input: &str, output: &str) -> ReferencePart {
        tool(name, summary, input, output, None, None)
    }

    fn user_turn_of(started_at: Option<&str>, parts: Vec<ReferencePart>) -> ReferenceTurn {
        ReferenceTurn {
            role: ReferenceTurnRole::User,
            started_at: started_at.map(str::to_string),
            ended_at: None,
            source: None,
            parts,
            abandoned: None,
        }
    }

    fn assistant_turn_of(
        started_at: Option<&str>,
        ended_at: Option<&str>,
        parts: Vec<ReferencePart>,
    ) -> ReferenceTurn {
        ReferenceTurn {
            role: ReferenceTurnRole::Assistant,
            started_at: started_at.map(str::to_string),
            ended_at: ended_at.map(str::to_string),
            source: None,
            parts,
            abandoned: None,
        }
    }

    fn loaded_skill(name: &str) -> ReferenceSkillActivity {
        ReferenceSkillActivity {
            name: name.to_string(),
            evidence: ReferenceSkillEvidence::Invocation,
            status: ReferenceSkillStatus::Loaded,
            path: None,
        }
    }

    /// The `task` tool call's own titles, in the record's order, joined with the reference's
    /// separator: `task_summary` when it exists, else `description`.
    #[test]
    fn normal_transcript_keeps_every_part_and_timestamp() {
        let turns = parse_claude_transcript(NORMAL).expect("a claude transcript parses");

        assert_eq!(
            turns,
            vec![
                user_turn_of(
                    Some("2026-10-06T09:00:00.000Z"),
                    vec![text("Add a health endpoint to the API.")]
                ),
                assistant_turn_of(
                    Some("2026-10-06T09:00:02.000Z"),
                    Some("2026-10-06T09:00:22.000Z"),
                    vec![
                        thinking(
                            "The router lives in src/api.rs; I will add a GET /health route next to it."
                        ),
                        text("I'll add the route next to the existing handlers."),
                        plain_tool(
                            "Read",
                            "/Users/dev/project/src/api.rs",
                            "{\n  \"file_path\": \"/Users/dev/project/src/api.rs\",\n  \"limit\": 120\n}",
                            "use axum::{Router, routing::get};\n\npub fn router() -> Router {\n    Router::new().route(\"/health\", get(health))\n}\n",
                        ),
                        tool(
                            "Bash",
                            "cargo test --lib api::health",
                            "{\n  \"command\": \"cargo test --lib api::health\",\n  \"description\": \"Run the health test\"\n}",
                            "test result: FAILED. 0 passed; 1 failed",
                            Some(true),
                            None,
                        ),
                        plain_tool(
                            "Task",
                            "Task",
                            "{\n  \"tasks\": [\n    {\n      \"description\": \"Port omp history\"\n    },\n    {\n      \"task_summary\": \"Port claude history\"\n    }\n  ]\n}",
                            "Task 1 completed.",
                        ),
                        tool(
                            "Skill",
                            "frontend",
                            "{\n  \"skill\": \"frontend\"\n}",
                            "Loaded skill: frontend",
                            None,
                            Some(loaded_skill("frontend")),
                        ),
                        text("The test failed because the route is not registered yet. Let me fix that."),
                    ],
                ),
                user_turn_of(
                    Some("2026-10-06T09:01:00.000Z"),
                    vec![
                        ReferencePart::Image {
                            media_type: "image/png".to_string(),
                            r#ref: "9c1b0f4a-0000-4000-8000-000000000013:1".to_string(),
                        },
                        text("Also paste the diff."),
                    ],
                ),
                user_turn_of(
                    Some("2026-10-06T09:01:30.000Z"),
                    vec![text("And bump the version to 1.2.0.")]
                ),
            ]
        );
    }

    /// Only a queued prompt a person typed is a user turn: another agent's message, a system-mode
    /// command, a task envelope, a blank prompt and a file attachment all stay out.
    #[test]
    fn queued_human_prompts_become_user_turns_and_nothing_else_does() {
        let turns = parse_claude_transcript(QUEUED).expect("a claude transcript parses");

        assert_eq!(
            turns,
            vec![
                user_turn_of(Some("2026-10-06T09:20:00.000Z"), vec![text("Start.")]),
                user_turn_of(
                    Some("2026-10-06T09:20:10.000Z"),
                    vec![text("Queued while working.")]
                ),
                user_turn_of(
                    Some("2026-10-06T09:20:14.000Z"),
                    vec![text("Pasted:\nLine one\nLine two\nDone.")]
                ),
                user_turn_of(
                    Some("2026-10-06T09:20:17.000Z"),
                    vec![text(
                        "<pasted_content id=\"p1\">\nOnly pasted\n</pasted_content id=\"p2\">"
                    )]
                ),
                user_turn_of(Some("2026-10-06T09:20:18.000Z"), vec![text("  \n body ")]),
                assistant_turn_of(
                    Some("2026-10-06T09:20:20.000Z"),
                    Some("2026-10-06T09:20:20.000Z"),
                    vec![text("Queued work acknowledged.")]
                ),
            ]
        );
    }

    /// `/clear` resets the conversation and the pending results; a prose mention of `/clear`,
    /// a local-command stdout, and a task notification do not. The compaction summary is a
    /// `compact` part in the user seat, and adjacent assistant entries merge into one turn.
    #[test]
    fn clear_envelope_resets_while_a_prose_mention_and_compaction_do_not() {
        let turns = parse_claude_transcript(BRANCH).expect("a claude transcript parses");

        assert_eq!(
            turns,
            vec![
                user_turn_of(
                    Some("2026-10-06T09:10:20.000Z"),
                    vec![text("What is the branch state now?")]
                ),
                assistant_turn_of(
                    Some("2026-10-06T09:10:21.000Z"),
                    Some("2026-10-06T09:10:22.000Z"),
                    vec![
                        text("The context was cleared, so this is a fresh conversation."),
                        text("Nothing from before the clear remains."),
                    ],
                ),
                user_turn_of(
                    Some("2026-10-06T09:10:30.000Z"),
                    vec![text("Quoting /clear in prose is not a reset: run /clear later.")]
                ),
                assistant_turn_of(
                    Some("2026-10-06T09:10:31.000Z"),
                    Some("2026-10-06T09:10:32.000Z"),
                    vec![
                        text("Understood, I will not reset on a mention."),
                        text("Sidechain (subagent) text."),
                    ],
                ),
                user_turn_of(
                    Some("2026-10-06T09:10:50.000Z"),
                    vec![ReferencePart::Compact {
                        text: "Summary of the folded conversation: the health endpoint was added.\nSecond summary line.".to_string(),
                    }],
                ),
                assistant_turn_of(
                    Some("2026-10-06T09:10:51.000Z"),
                    Some("2026-10-06T09:10:51.000Z"),
                    vec![text("Continuing after compaction.")],
                ),
            ]
        );
    }

    /// Malformed and partial records degrade: torn lines, non-objects, unknown results, unshown
    /// image types and unsupported blocks leave no trace, and nothing is fabricated in their
    /// place. `is_error` is strict, so the string `"true"` is not an error.
    #[test]
    fn partial_and_malformed_records_leave_no_fabricated_turn() {
        let turns = parse_claude_transcript(PARTIAL).expect("a claude transcript parses");

        assert_eq!(
            turns,
            vec![
                assistant_turn_of(
                    None,
                    None,
                    vec![text("No timestamps on this record.")]
                ),
                user_turn_of(None, vec![text("")]),
                assistant_turn_of(
                    None,
                    None,
                    vec![plain_tool(
                        "Bash",
                        "ls -la",
                        "{\n  \"command\": \"ls -la\"\n}",
                        "total 0",
                    )],
                ),
                user_turn_of(None, vec![text("Real question.")]),
                assistant_turn_of(
                    None,
                    None,
                    vec![plain_tool("Bash", "Bash", "{}", "")],
                ),
                user_turn_of(None, vec![ReferencePart::Compact { text: String::new() }]),
            ]
        );
    }

    /// The empty tool key: a `tool_use` without an `id` is filed under `""`, so a `tool_result`
    /// with an empty `tool_use_id` folds into it. Empty text and empty thinking blocks add no
    /// part, a `Skill` input that is not a label carries no skill, a non-array assistant content
    /// is ignored, and `is_error: true` marks both the part and its skill.
    #[test]
    fn assistant_edge_cases_follow_the_pinned_rules() {
        let turns = parse_claude_transcript(ASSISTANT).expect("a claude transcript parses");

        let mut failed_skill = loaded_skill("padded");
        failed_skill.status = ReferenceSkillStatus::Failed;
        assert_eq!(
            turns,
            vec![assistant_turn_of(
                Some("2026-10-06T10:00:01.000Z"),
                Some("2026-10-06T10:00:13.000Z"),
                vec![
                    thinking("Thinking via the text field."),
                    plain_tool("Bash", "Bash", "{}", "ok"),
                    plain_tool("Write", "Write", "{}", "empty-key result"),
                    tool(
                        "Skill",
                        "padded",
                        "{\n  \"skill\": \"   padded   \"\n}",
                        "part one part two",
                        Some(true),
                        Some(failed_skill),
                    ),
                    plain_tool("Skill", "Skill", "{\n  \"skill\": \"a\\nb\"\n}", ""),
                    text("Final answer."),
                ],
            )]
        );
    }

    /// A cut output keeps the first `TOOL_OUTPUT_CHARS` characters and points at the whole one;
    /// the goal tools use the wider cut, and an output exactly at the limit is left alone.
    #[test]
    fn long_tool_output_is_cut_and_points_at_the_whole_output() {
        let long = "x".repeat(REFERENCE_CLAUDE_TOOL_OUTPUT_CHARS + 1);
        let transcript = format!(
            "{}\n{}\n",
            json!({
                "type": "assistant",
                "timestamp": "2026-10-06T11:00:00.000Z",
                "message": { "role": "assistant", "content": [
                    { "type": "tool_use", "id": "toolu_long", "name": "Bash", "input": { "command": "yes" } }
                ] }
            }),
            json!({
                "type": "user",
                "timestamp": "2026-10-06T11:00:01.000Z",
                "message": { "role": "user", "content": [
                    { "type": "tool_result", "tool_use_id": "toolu_long", "content": long }
                ] }
            }),
        );

        let turns = parse_claude_transcript(&transcript).expect("a claude transcript parses");
        let ReferencePart::Tool {
            output,
            output_ref,
            output_size,
            ..
        } = &turns[0].parts[0]
        else {
            panic!("expected a tool part, got {:?}", turns[0].parts[0]);
        };
        assert_eq!(
            output,
            &format!("{}\n… trimmed", cut_chars(&long, REFERENCE_CLAUDE_TOOL_OUTPUT_CHARS))
        );
        assert_eq!(output_ref.as_deref(), Some("toolu_long"));
        assert_eq!(*output_size, Some((REFERENCE_CLAUDE_TOOL_OUTPUT_CHARS + 1) as u64));

        let exact = "y".repeat(REFERENCE_CLAUDE_TOOL_OUTPUT_CHARS);
        let transcript = format!(
            "{}\n{}\n",
            json!({
                "type": "assistant",
                "message": { "role": "assistant", "content": [
                    { "type": "tool_use", "id": "toolu_goal", "name": "create_goal", "input": { "objective": "x" } }
                ] }
            }),
            json!({
                "type": "user",
                "message": { "role": "user", "content": [
                    { "type": "tool_result", "tool_use_id": "toolu_goal", "content": exact }
                ] }
            }),
        );
        let turns = parse_claude_transcript(&transcript).expect("a claude transcript parses");
        let ReferencePart::Tool {
            output,
            output_ref,
            output_size,
            ..
        } = &turns[0].parts[0]
        else {
            panic!("expected a tool part");
        };
        assert_eq!(output, &exact);
        assert_eq!(*output_ref, None);
        assert_eq!(*output_size, None);
    }

    /// The tail window keeps the newest turns; `0` keeps them all, exactly as the pinned reader's
    /// `slice(-0)` does.
    #[test]
    fn max_turns_keeps_the_newest_turns() {
        let transcript = format!(
            "{}\n{}\n{}\n{}\n",
            json!({ "type": "user", "message": { "role": "user", "content": "one" } }),
            json!({ "type": "user", "message": { "role": "user", "content": "two" } }),
            json!({ "type": "user", "message": { "role": "user", "content": "three" } }),
            json!({ "type": "user", "message": { "role": "user", "content": "four" } }),
        );

        let limited = parse_claude_transcript_with_limit(&transcript, 2);
        assert_eq!(
            limited,
            vec![
                user_turn_of(None, vec![text("three")]),
                user_turn_of(None, vec![text("four")]),
            ]
        );
        assert_eq!(parse_claude_transcript_with_limit(&transcript, 0).len(), 4);
    }

    /// The lane refuses any other kind instead of answering with an empty conversation, which
    /// the UI would read as "this session has no turns".
    #[test]
    fn another_provider_kind_is_refused_never_parsed() {
        let error = parse_reference_claude_history(ReferenceNativeHistoryKind::Omp, NORMAL)
            .expect_err("another family's kind is refused");
        assert!(error.contains("claude-transcript"), "{error}");
        assert!(
            parse_reference_claude_history(ReferenceNativeHistoryKind::Claude, NORMAL).is_ok()
        );
        assert!(parse_reference_claude_history(ReferenceNativeHistoryKind::Unavailable, NORMAL).is_err());
    }

    /// The family emits the frozen wire shape: camelCase turn fields, `kind`-tagged parts, and no
    /// empty optional fields.
    #[test]
    fn turns_serialize_to_the_frozen_wire_shape() {
        let turns = parse_claude_transcript(NORMAL).expect("a claude transcript parses");
        assert_eq!(
            serde_json::to_value(&turns[0]).expect("a turn serializes"),
            json!({
                "role": "user",
                "startedAt": "2026-10-06T09:00:00.000Z",
                "parts": [{ "kind": "text", "text": "Add a health endpoint to the API." }],
            })
        );
        assert_eq!(
            serde_json::to_value(&turns[1].parts[0]).expect("a part serializes"),
            json!({
                "kind": "thinking",
                "text": "The router lives in src/api.rs; I will add a GET /health route next to it.",
            })
        );
        assert_eq!(
            serde_json::to_value(&turns[1].parts[3]).expect("a part serializes"),
            json!({
                "kind": "tool",
                "name": "Bash",
                "summary": "cargo test --lib api::health",
                "input": "{\n  \"command\": \"cargo test --lib api::health\",\n  \"description\": \"Run the health test\"\n}",
                "output": "test result: FAILED. 0 passed; 1 failed",
                "error": true,
            })
        );
        assert_eq!(
            serde_json::to_value(&turns[2].parts[0]).expect("a part serializes"),
            json!({
                "kind": "image",
                "mediaType": "image/png",
                "ref": "9c1b0f4a-0000-4000-8000-000000000013:1",
            })
        );
    }

    /// The boundary helpers task 3 pages with, and the record rules they rest on.
    #[test]
    fn turn_boundaries_and_record_rules_match_the_pin() {
        assert_eq!(REFERENCE_CLAUDE_TURN_MARK, "\"user\"");
        assert!(opens_claude_turn(
            r#"{"type":"user","message":{"role":"user","content":"typed"}}"#
        ));
        assert!(opens_claude_turn(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"typed"}]}}"#
        ));
        assert!(!opens_claude_turn(
            r#"{"type":"user","message":{"role":"user","content":"<command-name>/clear</command-name>"}}"#
        ));
        assert!(!opens_claude_turn(
            r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"meta"}}"#
        ));
        assert!(!opens_claude_turn(
            r#"{"type":"user","isCompactSummary":true,"message":{"role":"user","content":"sum"}}"#
        ));
        assert!(!opens_claude_turn(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"x"}]}}"#
        ));
        assert!(!opens_claude_turn("{\"type\":\"user\","));
        assert!(!opens_claude_turn("42"));

        let clear = json!({
            "type": "user",
            "message": { "role": "user", "content": "<command-name>/clear</command-name>\n<command-message>clear</command-message>\n<command-args></command-args>" }
        });
        assert!(is_claude_context_clear(clear.as_object().unwrap()));
        for content in [
            "<command-name>/clear</command-name>",
            "Quoting /clear is not a reset.",
            "<command-name>/clear</command-name> trailing",
        ] {
            let entry = json!({ "type": "user", "message": { "role": "user", "content": content } });
            assert!(
                !is_claude_context_clear(entry.as_object().unwrap()),
                "{content} is not a whole clear envelope"
            );
        }
        let array_content = json!({
            "type": "user",
            "message": { "role": "user", "content": [{ "type": "text", "text": "<command-name>/clear</command-name>" }] }
        });
        assert!(!is_claude_context_clear(array_content.as_object().unwrap()));
        let assistant = json!({
            "type": "assistant",
            "message": { "role": "user", "content": "<command-name>/clear</command-name>" }
        });
        assert!(!is_claude_context_clear(assistant.as_object().unwrap()));

        assert!(is_claude_command_entry("<command-name>/help</command-name>"));
        assert!(is_claude_command_entry("<local-command-stdout>clear</local-command-stdout>"));
        assert!(is_claude_command_entry("<task-notification>x</task-notification>"));
        assert!(!is_claude_command_entry("task-notification without a tag"));
    }

    /// Paste unwrapping accepts only a matching, word-shaped id pair and leaves anything else
    /// byte-for-byte as it was typed.
    #[test]
    fn paste_unwrapping_accepts_only_a_matching_word_shaped_pair() {
        assert_eq!(
            unwrap_claude_pastes("<pasted_content id=\"p1\">\nbody\n</pasted_content id=\"p1\">"),
            "body"
        );
        assert_eq!(
            unwrap_claude_pastes(
                "\n<pasted_content id=\"p1\">\nbody\n</pasted_content id=\"p1\">\n"
            ),
            "body"
        );
        assert_eq!(
            unwrap_claude_pastes("before <pasted_content id=\"p1\">\nbody\n</pasted_content id=\"p1\"> after"),
            "before body after"
        );
        let mismatched = "<pasted_content id=\"p1\">\nbody\n</pasted_content id=\"p2\">";
        assert_eq!(unwrap_claude_pastes(mismatched), mismatched);
        let long_id = format!(
            "<pasted_content id=\"{}\">\nbody\n</pasted_content id=\"{}\">",
            "a".repeat(65),
            "a".repeat(65)
        );
        assert_eq!(unwrap_claude_pastes(&long_id), long_id);
        let dotted = "<pasted_content id=\"a.b\">\nbody\n</pasted_content id=\"a.b\">";
        assert_eq!(unwrap_claude_pastes(dotted), dotted);
        assert_eq!(unwrap_claude_pastes("plain text"), "plain text");
    }

    /// The tool summary reads the reference's fields in order, the `task` call's own titles
    /// first, and falls back to the tool's name.
    #[test]
    fn tool_summaries_follow_the_pinned_field_order() {
        let object = |value: serde_json::Value| value.as_object().cloned().unwrap_or_default();

        assert_eq!(
            claude_tool_summary("task", &object(json!({ "tasks": [
                { "description": "Port omp history" },
                { "task_summary": "Port claude history" },
            ] }))),
            "Port omp history · Port claude history"
        );
        assert_eq!(
            claude_tool_summary("task", &object(json!({ "task_summary": "  Port claude  " }))),
            "Port claude"
        );
        assert_eq!(
            claude_tool_summary("Bash", &object(json!({ "command": "cargo test", "description": "run" }))),
            "cargo test"
        );
        assert_eq!(
            claude_tool_summary("Read", &object(json!({ "file_path": "/a/b.rs" }))),
            "/a/b.rs"
        );
        assert_eq!(
            claude_tool_summary("Read", &object(json!({ "notebook_path": "/a/n.ipynb" }))),
            "/a/n.ipynb"
        );
        assert_eq!(
            claude_tool_summary("Grep", &object(json!({ "pattern": "needle" }))),
            "needle"
        );
        assert_eq!(
            claude_tool_summary("WebFetch", &object(json!({ "url": "https://example.com" }))),
            "https://example.com"
        );
        assert_eq!(claude_tool_summary("Write", &object(json!({}))), "Write");
        assert_eq!(
            claude_tool_summary("Bash", &object(json!({ "command": "x".repeat(200) }))).chars().count(),
            120
        );
        assert_eq!(
            claude_tool_summary("task", &object(json!({ "tasks": [{ "task_summary": 7 }] }))),
            "task"
        );
    }

    /// The pretty writer reproduces `JSON.stringify(input, null, 2)`: record order, the short
    /// control escapes, `\u00xx` for the rest, and non-ASCII characters as themselves.
    #[test]
    fn tool_input_is_serialized_the_way_the_reference_writes_it() {
        assert_eq!(stringify_pretty(&json!({})), "{}");
        assert_eq!(stringify_pretty(&json!([])), "[]");
        assert_eq!(stringify_pretty(&json!(null)), "null");
        assert_eq!(stringify_pretty(&json!("cargo test")), "\"cargo test\"");
        assert_eq!(
            stringify_pretty(&json!({ "b": 1, "a": [1, { "c": true }] })),
            "{\n  \"b\": 1,\n  \"a\": [\n    1,\n    {\n      \"c\": true\n    }\n  ]\n}"
        );
        assert_eq!(
            stringify_pretty(&json!({ "s": "a\"b\\c\nd\te\u{1}f\u{2028}" })),
            "{\n  \"s\": \"a\\\"b\\\\c\\nd\\te\\u0001f\u{2028}\"\n}"
        );
    }

    /// A transcript with no records is an empty conversation, not a failure: an empty native file
    /// is `notStarted`, which the resolver (task 3) labels, not this lane.
    #[test]
    fn an_empty_transcript_is_an_empty_conversation() {
        assert!(parse_claude_transcript("").expect("empty parses").is_empty());
        assert!(parse_claude_transcript("\n\n   \n").expect("blank parses").is_empty());
    }
}
