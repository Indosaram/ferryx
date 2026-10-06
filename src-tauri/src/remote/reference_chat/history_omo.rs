//! OmO native history family (plan task 19).
//!
//! Ported from `devswha/herdr-web-ui` @ `54e5a1f67090cb09552d182e7e30dd0ecc314918`
//! (MIT, see `docs/chat/HERDR_LICENSE`). The pinned sources this file ports are
//! `server/transcript-records.ts` (`parseOmpTranscript`, `piMessage`, `piResults`,
//! `piNotice`, `omoTaskTitles`, `omoTaskResults`, `isContextClear`, `toolSummary`,
//! `MAX_TURNS`, `OMO_TASK_RESULT_MAX`), `server/skill-activity.ts`
//! (`skillInvocationPrompt`, `loadedSkill`, `skillDocument`, the two `label`s) and
//! `server/tool-output.ts` (`trimOutput`, `TOOL_OUTPUT_CHARS`, `WHOLE_OUTPUT_TOOLS`,
//! `WHOLE_OUTPUT_CHARS`), plus the OmO store/process shape from `server/omo.ts`
//! (`isOmoProcess`, `omoSessionFolder`, the session-header record).
//!
//! Why the omp record shape is the OmO record shape: upstream's own header states it —
//! "omo: herdr knows nothing about its store and its label for the pane flips between `pi`
//! and `claude` as omo spawns model CLIs, so the pane's process tree routes it and
//! process/session evidence selects a unique transcript under `<its agent dir>/sessions/
//! <cwd-slug>/`. It writes omp's session shape, so `parseOmpTranscript` reads it."
//! `conversation.ts` therefore dispatches `omo-transcript` through `parseTurns`'s
//! non-Claude/non-Codex arm with `toolImages: false` and the caller's `taskTitles`.
//!
//! What this lane does NOT own: which file a pane's session lives in (that is the
//! resolver's job — task 3 — and `omo.ts`'s `omoCandidates` / `heldSessionIds` /
//! `selectOmoTranscript` inference is acquisition, not record parsing), and the page /
//! cursor machinery of `conversation.ts` (task 2). This file takes bytes and returns
//! turns. It performs no I/O and spawns nothing.
//!
//! Port boundary: `gjc` and `omp` write the same record shape, but their registry ids
//! belong to their own lanes (tasks 18 and 20). [`parse_omo_history`] refuses every kind
//! except [`ReferenceNativeHistoryKind::Omo`]; a shared-shape caller that already owns
//! another registry id can call [`parse_omo_transcript`] directly.

use std::collections::HashMap;
use std::sync::OnceLock;

use regex::Regex;
use serde_json::{json, Map, Value};

use super::types::{
    ReferenceImageRef, ReferenceNativeHistoryKind, ReferencePart, ReferenceSkillActivity,
    ReferenceSkillEvidence, ReferenceSkillStatus, ReferenceTaskResult, ReferenceTaskStatus,
    ReferenceTurn, ReferenceTurnRole, ReferenceTurnSource,
};

/// Enough turns for a conversation. Upstream `MAX_TURNS`; a page reader passes `usize::MAX`
/// (`Infinity`) exactly as `conversation.ts`'s `parseTurns` does, and the family entry point
/// keeps the upstream default so a direct caller cannot read an unbounded transcript.
pub const OMO_MAX_TURNS: usize = 100;

/// Bytes every line that opens a turn contains: the page scanner's cheap filter before
/// `JSON.parse`. Upstream `TURN_MARK["omo-transcript"]`.
pub const OMO_TURN_MARK: &str = "\"user\"";

/// A task's answer is carried up to this many characters; the rest is flagged cut.
/// Upstream `OMO_TASK_RESULT_MAX`.
pub const OMO_TASK_RESULT_MAX_CHARS: usize = 16_000;

/// Past this a tool's output is cut in the page; the rest is fetched on request.
/// Upstream `TOOL_OUTPUT_CHARS`.
pub const OMO_TOOL_OUTPUT_CHARS: usize = 4_000;

/// OmO's goal calls answer with the goal as JSON the chat reads, so a finished goal's
/// answer would lose its status at the usual limit. Upstream `WHOLE_OUTPUT_TOOLS`.
pub const OMO_WHOLE_OUTPUT_TOOLS: [&str; 3] = ["create_goal", "update_goal", "get_goal"];

/// The limit those goal tools get instead. Upstream `WHOLE_OUTPUT_CHARS`.
pub const OMO_WHOLE_OUTPUT_CHARS: usize = 16_000;

/// The image types a chat shows. Upstream `PI_IMAGE_TYPES`.
pub const OMO_IMAGE_TYPES: [&str; 4] = ["image/png", "image/jpeg", "image/gif", "image/webp"];

/// The `customType` OmO wakes its agent with when a background task ends.
const OMO_WAKE_TYPE: &str = "omo-senpi:wake";

/// The one `details` group of a wake that carries task results.
const OMO_TASK_COMPLETION_TYPE: &str = "senpi-task.completion";

/// The `customType` a reset marker carries in this record shape.
const CONTEXT_CLEAR_TYPE: &str = "context_clear";

// ---------------------------------------------------------------------------------------
// The skill part
// ---------------------------------------------------------------------------------------

// Upstream's `parseOmpTranscript` pushes a user turn's parts as
// `[text(asked), skill(SkillActivity)]` — `skill-activity.ts`'s `skillInvocationPrompt` turns a
// `/skill:name` prompt into the request the person typed plus one chip per skill it loaded
// (`transcript-records.ts:259`). This lane emits both: the request as the user's text and each
// skill as its own [`ReferencePart::Skill`]. The turn's skill list draws them; the inline part
// list does not (`ChatView.tsx:322`).

// ---------------------------------------------------------------------------------------
// Small record helpers, mirroring the pinned `record` / `string` / `label` / `amount`
// ---------------------------------------------------------------------------------------

/// A JSON object view that yields `None` for a missing field instead of throwing, the way
/// upstream's `record()` yields `{}`.
#[derive(Clone, Copy)]
struct Row<'a>(Option<&'a Map<String, Value>>);

impl<'a> Row<'a> {
    fn of(value: &'a Value) -> Self {
        Self(value.as_object())
    }

    fn get(&self, key: &str) -> Option<&'a Value> {
        self.0?.get(key)
    }

    /// `typeof value === "string"` — a non-string is skipped, never coerced.
    fn string(&self, key: &str) -> Option<&'a str> {
        self.get(key).and_then(Value::as_str)
    }

    /// Upstream `string(...values)`: the first value that *is* a string.
    fn string_any(&self, keys: &[&str]) -> Option<&'a str> {
        keys.iter().find_map(|key| self.string(key))
    }

    /// Upstream `a ?? b ?? c`: the first value that is neither absent nor `null`.
    fn first_present(&self, keys: &[&str]) -> Option<&'a Value> {
        keys.iter().find_map(|key| self.get(key).filter(|value| !value.is_null()))
    }

    fn bool(&self, key: &str) -> Option<bool> {
        self.get(key).and_then(Value::as_bool)
    }

    fn number(&self, key: &str) -> Option<f64> {
        self.get(key).and_then(Value::as_f64)
    }

    fn row(&self, key: &str) -> Row<'a> {
        Row(self.get(key).and_then(Value::as_object))
    }

    fn array(&self, key: &str) -> Option<&'a Vec<Value>> {
        self.get(key).and_then(Value::as_array)
    }
}

/// Upstream `transcript-records.ts`'s `label`: trimmed, non-empty.
fn record_label(value: Option<&Value>) -> Option<String> {
    let text = value?.as_str()?.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

/// Upstream `skill-activity.ts`'s `label`: no trim, bounded, and free of markup characters.
/// A second, stricter `label` on purpose — the pinned sources really do define both.
fn skill_label(value: Option<&Value>) -> Option<String> {
    let text = value?.as_str()?;
    if text.is_empty() || text.chars().count() > 200 || text.contains(['\r', '\n', '<', '>']) {
        return None;
    }
    Some(text.to_string())
}

/// Upstream `amount()`: a finite, non-negative number.
fn amount(value: Option<&Value>) -> Option<u64> {
    let number = value?.as_f64()?;
    if number.is_finite() && number >= 0.0 {
        Some(number as u64)
    } else {
        None
    }
}

/// Upstream `resultText()`: a string, or the concatenated `text` of its blocks.
pub fn omo_result_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| Row::of(item).string("text").unwrap_or("").to_string())
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

// ---------------------------------------------------------------------------------------
// Context reset
// ---------------------------------------------------------------------------------------

/// Is this entry a context reset for the omp/omo/gjc record shape?
///
/// Upstream `isContextClear(value, "omo-transcript")`: the non-Claude, non-Codex arm —
/// `type === "custom" && customType === "context_clear"`. A reset empties the conversation
/// read so far, exactly as a `/clear` does in the TUI.
pub fn is_omo_context_clear(entry: &Value) -> bool {
    let row = Row::of(entry);
    row.string("type") == Some("custom") && row.string("customType") == Some(CONTEXT_CLEAR_TYPE)
}

// ---------------------------------------------------------------------------------------
// piMessage / piResults / piNotice
// ---------------------------------------------------------------------------------------

/// OmO-family providers use several spellings for the same tool call/result fields.
/// Upstream `piMessage`: normalizes a `message` record, or `None` when the entry is not a
/// displayable message.
///
/// Returns a JSON object with `content` always an array and the tool-call/tool-result
/// spellings folded onto `name` / `id` / `arguments` / `toolCallId` / `content`.
pub fn omo_pi_message(entry: &Value) -> Option<Value> {
    let entry_row = Row::of(entry);
    if entry_row.string("type") != Some("message") {
        return None;
    }
    let message = entry_row.row("message");
    if message.bool("display") == Some(false) || entry_row.bool("display") == Some(false) {
        return None;
    }
    let raw: Vec<Value> = match message.get("content") {
        Some(Value::String(text)) => vec![json!({ "type": "text", "text": text })],
        Some(Value::Array(items)) => items.clone(),
        _ => Vec::new(),
    };
    let content: Vec<Value> = raw
        .into_iter()
        .map(|value| {
            let block = Row::of(&value);
            let mut map = value.as_object().cloned().unwrap_or_default();
            match block.string("type") {
                Some("toolCall") => {
                    overwrite(
                        &mut map,
                        "name",
                        block.string_any(&["toolName", "name"]).map(Value::from),
                    );
                    overwrite(
                        &mut map,
                        "id",
                        block.string_any(&["toolCallId", "id", "callId"]).map(Value::from),
                    );
                    overwrite(
                        &mut map,
                        "arguments",
                        block.first_present(&["toolInput", "input", "arguments"]).cloned(),
                    );
                }
                Some("toolResult") => {
                    overwrite(
                        &mut map,
                        "toolCallId",
                        block.string_any(&["toolCallId", "callId", "id"]).map(Value::from),
                    );
                    overwrite(
                        &mut map,
                        "content",
                        block.first_present(&["output", "content", "result"]).cloned(),
                    );
                }
                _ => {}
            }
            Value::Object(map)
        })
        .collect();
    let mut out = message.0.cloned().unwrap_or_default();
    overwrite(
        &mut out,
        "toolCallId",
        message.string_any(&["toolCallId", "callId"]).map(Value::from),
    );
    out.insert("content".into(), Value::Array(content));
    Some(Value::Object(out))
}

/// Set a key to its new value, or drop it when the provider had none — the Rust spelling of
/// upstream's `{ ...block, name: undefined }` spread override.
fn overwrite(map: &mut Map<String, Value>, key: &str, value: Option<Value>) {
    map.remove(key);
    if let Some(value) = value {
        map.insert(key.to_string(), value);
    }
}

/// One tool result as this record shape records it, with the images it carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OmoToolResult {
    /// The tool call this answers.
    pub id: String,
    pub text: String,
    pub error: bool,
    /// Media types of the images the result carries, in order.
    pub images: Vec<String>,
}

/// The image of a result block, when it is one a chat shows.
fn omo_image_of(value: &Value) -> Option<(String, String)> {
    let image = Row::of(value);
    if image.string("type") != Some("image") {
        return None;
    }
    let media_type = image.string_any(&["mimeType", "media_type"])?;
    if !OMO_IMAGE_TYPES.contains(&media_type) || image.get("data").and_then(Value::as_str).is_none() {
        return None;
    }
    Some((media_type.to_string(), image.string("data")?.to_string()))
}

/// One entry's tool results, with the images each carries.
///
/// Upstream `piResults`. OmO reads a picture into a tool result as inline base64 beside the
/// text; only the media type is carried here, and the ref a caller mints names the call it
/// answers plus the image's index inside it.
pub fn omo_pi_results(message: &Value) -> Vec<OmoToolResult> {
    let row = Row::of(message);
    let blocks: Vec<&Value> = if row.string("role") == Some("toolResult") {
        vec![message]
    } else {
        row.array("content")
            .map(|items| {
                items
                    .iter()
                    .filter(|item| Row::of(item).string("type") == Some("toolResult"))
                    .collect()
            })
            .unwrap_or_default()
    };
    blocks
        .into_iter()
        .filter_map(|block| {
            let block = Row::of(block);
            let id = block.string("toolCallId")?;
            let images = block
                .array("content")
                .map(|items| {
                    items
                        .iter()
                        .filter_map(omo_image_of)
                        .map(|(media_type, _)| media_type)
                        .collect()
                })
                .unwrap_or_default();
            Some(OmoToolResult {
                id: id.to_string(),
                text: omo_result_text(block.get("content")),
                error: block.bool("isError") == Some(true),
                images,
            })
        })
        .collect()
}

/// The notice a runtime put in the user's seat, or `None`.
///
/// Upstream `piNotice`: a `custom_message` the runtime displays, with its
/// `<system-notice>` envelope stripped. The envelope is chrome, never the message.
pub fn omo_pi_notice(entry: &Value) -> Option<ReferencePart> {
    let row = Row::of(entry);
    if row.string("type") != Some("custom_message") || row.bool("display") == Some(false) {
        return None;
    }
    let content = row.string("content")?;
    let text = strip_system_notice(content);
    if text.is_empty() {
        return None;
    }
    Some(ReferencePart::Notice { text, source: row.string("customType").map(str::to_string) })
}

/// The `<system-notice>` envelope is chrome; the message is what it wraps.
fn strip_system_notice(content: &str) -> String {
    let trimmed = content.trim();
    let without_open = trimmed.strip_prefix("<system-notice>").unwrap_or(trimmed);
    let without_close = without_open.strip_suffix("</system-notice>").unwrap_or(without_open);
    without_close.trim().to_string()
}

// ---------------------------------------------------------------------------------------
// OmO background tasks: titles and results
// ---------------------------------------------------------------------------------------

/// The summaries OmO's `task` calls gave their tasks, by task id.
///
/// Upstream `omoTaskTitles`: the call's result names the task it started
/// (`details.task_id`, or one `details.items` entry per task of a batch). A title recorded
/// on an earlier page is inherited by the caller, exactly as `conversation.ts` carries
/// `taskTitles` across pages.
pub fn omo_task_titles(message: &Value, titles: &mut HashMap<String, String>) {
    let row = Row::of(message);
    if row.string("role") != Some("toolResult") || row.string("toolName") != Some("task") {
        return;
    }
    let details = row.row("details");
    let items: Vec<&Value> = details.array("items").map(|items| items.iter().collect()).unwrap_or_else(|| vec![row.get("details").unwrap_or(&Value::Null)]);
    for value in items {
        let item = Row::of(value);
        let id = record_label(item.get("task_id"));
        let title = record_label(item.get("task_summary")).or_else(|| record_label(item.get("description")));
        if let (Some(id), Some(title)) = (id, title) {
            titles.insert(id, title);
        }
    }
}

/// OmO's background tasks that ended, as OmO reported them back to the agent, or `None`.
///
/// Upstream `omoTaskResults`: OmO wakes its agent with a `custom_message`
/// (`omo-senpi:wake`, `display: false`) whose details hold one `senpi-task.completion` per
/// task. It is the only record that a task ended and what it found. A wake that carries no
/// task result — a monitor's event — is not a task result.
pub fn omo_task_results(entry: &Value, titles: &HashMap<String, String>) -> Option<Vec<ReferenceTaskResult>> {
    let row = Row::of(entry);
    if row.string("type") != Some("custom_message")
        || row.string("customType") != Some(OMO_WAKE_TYPE)
        || row.array("details").is_none()
    {
        return None;
    }
    // One row per task: a task reported twice in one wake is the later report, and the rows
    // keep the order in which each id was first seen (a `Map` set on an existing key).
    let mut order: Vec<String> = Vec::new();
    let mut tasks: HashMap<String, ReferenceTaskResult> = HashMap::new();
    for group in row.array("details")?.iter() {
        let group = Row::of(group);
        if group.string("customType") != Some(OMO_TASK_COMPLETION_TYPE) {
            continue;
        }
        let Some(entries) = group.array("details") else {
            continue;
        };
        for value in entries {
            let task = Row::of(value);
            let Some(id) = record_label(task.get("task_id")) else {
                continue;
            };
            let Some(raw) = record_label(task.get("status")) else {
                continue;
            };
            let stats = task.row("run_stats");
            let agent = record_label(task.get("agent_type"))
                .or_else(|| record_label(task.get("category")))
                .or_else(|| record_label(task.get("subagent_type")));
            let name = record_label(task.get("name"));
            let result = record_label(task.get("final_response"))
                .or_else(|| record_label(task.get("error")))
                .unwrap_or_default();
            let status = match raw.as_str() {
                "completed" => ReferenceTaskStatus::Completed,
                "cancelled" | "canceled" | "aborted" => ReferenceTaskStatus::Cancelled,
                _ => ReferenceTaskStatus::Failed,
            };
            let title = titles
                .get(&id)
                .cloned()
                .or_else(|| match (&name, &agent) {
                    (Some(name), _) if name != &id => Some(name.clone()),
                    (_, Some(agent)) => Some(agent.clone()),
                    _ => Some(id.clone()),
                })
                .unwrap_or_else(|| id.clone());
            let result_cut = result.chars().count() > OMO_TASK_RESULT_MAX_CHARS;
            let result: String = result.chars().take(OMO_TASK_RESULT_MAX_CHARS).collect();
            if !tasks.contains_key(&id) {
                order.push(id.clone());
            }
            tasks.insert(
                id.clone(),
                ReferenceTaskResult {
                    id,
                    title,
                    agent,
                    model: record_label(task.row("resolved_model").get("display"))
                        .or_else(|| record_label(task.get("model"))),
                    status,
                    duration_ms: amount(task.get("duration_ms")).or_else(|| amount(stats.get("runtime_ms"))),
                    turns: amount(stats.get("turns")),
                    tool_calls: amount(stats.get("tool_calls")),
                    tokens: amount(task.get("total_tokens")).or_else(|| amount(task.get("tokens"))),
                    result,
                    result_cut: if result_cut { Some(true) } else { None },
                },
            );
        }
    }
    if tasks.is_empty() {
        return None;
    }
    Some(order.into_iter().filter_map(|id| tasks.remove(&id)).collect())
}

// ---------------------------------------------------------------------------------------
// Tool summary and output trimming
// ---------------------------------------------------------------------------------------

/// The one-line summary a collapsed tool chip shows.
///
/// Upstream `toolSummary`: an OmO or omp `task` call shows the summary it gave the person,
/// one per task of a batch; every other call shows the first identifying argument, and the
/// name when there is none.
pub fn omo_tool_summary(name: &str, input: &Value) -> String {
    let row = Row::of(input);
    if name == "task" {
        let items: Vec<Value> = match row.array("tasks") {
            Some(items) => items.clone(),
            None => vec![input.clone()],
        };
        let titles: Vec<String> = items
            .iter()
            .filter_map(|item| {
                let item = Row::of(item);
                record_label(item.get("task_summary")).or_else(|| record_label(item.get("description")))
            })
            .collect();
        if !titles.is_empty() {
            return titles.join(" · ").chars().take(120).collect();
        }
    }
    for key in ["command", "file_path", "notebook_path", "path", "pattern", "description", "url"] {
        if let Some(first) = row.string(key) {
            return first.chars().take(120).collect();
        }
    }
    name.to_string()
}

/// A tool part's output as a page carries it, plus what it takes to fetch the rest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OmoTrimmedOutput {
    pub output: String,
    /// Set when the output was cut: the call's id, for the on-demand output fetch.
    pub output_ref: Option<String>,
    /// The whole output's length, when it was cut.
    pub output_size: Option<u64>,
}

/// Cut a tool's output to what a page carries, keeping what it takes to fetch the rest.
///
/// Upstream `trimOutput`: `OMO_WHOLE_OUTPUT_TOOLS` get the larger limit so a finished goal's
/// JSON answer keeps its status; everything else is cut at `TOOL_OUTPUT_CHARS`.
pub fn trim_omo_tool_output(name: &str, output: &str, reference: &str) -> OmoTrimmedOutput {
    let limit = if OMO_WHOLE_OUTPUT_TOOLS.contains(&name) {
        OMO_WHOLE_OUTPUT_CHARS
    } else {
        OMO_TOOL_OUTPUT_CHARS
    };
    if output.chars().count() <= limit {
        return OmoTrimmedOutput { output: output.to_string(), output_ref: None, output_size: None };
    }
    let head: String = output.chars().take(limit).collect();
    OmoTrimmedOutput {
        output: format!("{head}\n… trimmed"),
        output_ref: Some(reference.to_string()),
        output_size: Some(output.chars().count() as u64),
    }
}

// ---------------------------------------------------------------------------------------
// Skill invocations in a prompt
// ---------------------------------------------------------------------------------------

/// What a skill-invoking prompt said: the skills it loaded, and the request underneath.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OmoSkillInvocation {
    pub skills: Vec<ReferenceSkillActivity>,
    pub request: String,
}

/// The skill a `SKILL.md` path names, with no filesystem lookup: evidence belongs to the
/// bound transcript, including remote hosts. Upstream `skillDocument`.
pub fn omo_skill_document(path: Option<&Value>) -> Option<ReferenceSkillActivity> {
    let path = path?.as_str()?;
    if path.chars().count() > 4096 || path.contains(['\r', '\n']) {
        return None;
    }
    let normalized = path.replace('\\', "/");
    let document = normalized.strip_suffix("/SKILL.md")?;
    let name = document.rsplit('/').next()?;
    let name = skill_label(Some(&Value::String(name.to_string())))?;
    if name == "." {
        return None;
    }
    Some(ReferenceSkillActivity {
        name,
        evidence: ReferenceSkillEvidence::Instructions,
        status: ReferenceSkillStatus::Loaded,
        path: Some(path.to_string()),
    })
}

/// A skill the transcript named by file: a `SKILL.md`, or a standalone `.md` skill.
/// Upstream `loadedSkill`.
pub fn omo_loaded_skill(name: Option<&str>, location: Option<&str>) -> Option<ReferenceSkillActivity> {
    let name = skill_label(name.map(|name| Value::String(name.to_string())).as_ref())?;
    let location = location?;
    if location.is_empty() || location.chars().count() > 4096 || location.contains(['\r', '\n']) {
        return None;
    }
    Some(ReferenceSkillActivity {
        name,
        evidence: ReferenceSkillEvidence::Instructions,
        status: ReferenceSkillStatus::Loaded,
        path: Some(location.to_string()),
    })
}

fn skill_instruction_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r#"^The user explicitly invoked the "([^"]+)" skill\. Follow the instructions in <skill-instruction> as binding for this request, while respecting higher-priority instructions\.\n\n<skill-instruction name="([^"]+)" location="([^"]+)">\n[\s\S]*?\n</skill-instruction>"#,
        )
        .expect("the pinned skill-instruction envelope is a valid pattern")
    })
}

fn legacy_skill_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"^<skill name="([^"]+)" location="([^"]+)">\n[\s\S]*?\n</skill>(?:\n\n([\s\S]+))?$"#)
            .expect("the pinned legacy skill envelope is a valid pattern")
    })
}

fn user_request_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"^\n\n<user-request>\n([\s\S]*?)\n</user-request>$"#)
            .expect("the pinned user-request envelope is a valid pattern")
    })
}

/// The prompt omp/omo record when the user invokes a skill (`/skill:name`, `$name`, a
/// keyword): the whole `SKILL.md` before the request, tens of KB the user never typed.
///
/// Upstream `skillInvocationPrompt`: chained invocations, then `<user-request>`, or the
/// legacy `<skill name location>` form. Anything else is left as the user's text.
pub fn omo_skill_invocation_prompt(text: &str) -> Option<OmoSkillInvocation> {
    let mut skills: Vec<ReferenceSkillActivity> = Vec::new();
    let mut remainder = text;
    let mut matched = skill_instruction_regex().captures(remainder);
    while let Some(captures) = matched {
        let whole = captures.get(0)?;
        let first = captures.get(1)?.as_str();
        let second = captures.get(2)?.as_str();
        let location = captures.get(3)?.as_str();
        let skill = if first == second { omo_loaded_skill(Some(first), Some(location)) } else { None };
        let skill = skill?;
        skills.push(skill);
        remainder = &remainder[whole.end()..];
        if !remainder.starts_with("\n\nThe user explicitly invoked the ") {
            break;
        }
        remainder = &remainder[2..];
        matched = skill_instruction_regex().captures(remainder);
        if matched.is_none() {
            return None;
        }
    }
    if !skills.is_empty() {
        if remainder.is_empty() {
            return Some(OmoSkillInvocation { skills, request: String::new() });
        }
        let captures = user_request_regex().captures(remainder)?;
        let request = captures.get(1)?.as_str().trim().to_string();
        return Some(OmoSkillInvocation { skills, request });
    }
    let captures = legacy_skill_regex().captures(text)?;
    let skill = omo_loaded_skill(Some(captures.get(1)?.as_str()), Some(captures.get(2)?.as_str()))?;
    let request = captures.get(3).map(|value| value.as_str().trim().to_string()).unwrap_or_default();
    Some(OmoSkillInvocation { skills: vec![skill], request })
}

// ---------------------------------------------------------------------------------------
// The transcript parser
// ---------------------------------------------------------------------------------------

fn assistant_turn(turns: &mut Vec<ReferenceTurn>, timestamp: &Option<String>, settled: bool) -> usize {
    let reuse = matches!(
        turns.last(),
        Some(turn) if turn.role == ReferenceTurnRole::Assistant && !settled
    );
    if !reuse {
        turns.push(ReferenceTurn {
            role: ReferenceTurnRole::Assistant,
            started_at: timestamp.clone(),
            ended_at: None,
            source: None,
            parts: Vec::new(),
            abandoned: None,
        });
    }
    turns.len() - 1
}

fn apply_omo_tool_results(
    turns: &mut Vec<ReferenceTurn>,
    pending: &mut HashMap<String, (usize, usize)>,
    results: &[OmoToolResult],
    tool_images: bool,
) {
    for result in results {
        let Some((turn_index, part_index)) = pending.remove(&result.id) else {
            continue;
        };
        let Some(ReferencePart::Tool { name, output, error, images, output_ref, output_size, .. }) =
            turns.get_mut(turn_index).and_then(|turn| turn.parts.get_mut(part_index))
        else {
            continue;
        };
        let trimmed = trim_omo_tool_output(name, &result.text, &result.id);
        *output = trimmed.output;
        *output_ref = trimmed.output_ref;
        *output_size = trimmed.output_size;
        if result.error {
            *error = Some(true);
        }
        if tool_images && !result.images.is_empty() {
            // addressed by the call it answers: a nested result shares its entry with other
            // blocks, so the entry id alone could not say which one an image came from
            *images = result
                .images
                .iter()
                .enumerate()
                .map(|(index, media_type)| ReferenceImageRef {
                    media_type: media_type.clone(),
                    r#ref: format!("pi:{}:{}", result.id, index),
                })
                .collect();
        }
    }
}

fn runtime_turn(timestamp: Option<&str>, parts: Vec<ReferencePart>) -> ReferenceTurn {
    ReferenceTurn {
        role: ReferenceTurnRole::User,
        started_at: timestamp.map(str::to_string),
        ended_at: None,
        source: Some(ReferenceTurnSource::Runtime),
        parts,
        abandoned: None,
    }
}

/// Splits one omo session jsonl into turns.
///
/// A direct port of upstream `parseOmpTranscript` with the arguments
/// `conversation.ts`'s `parseTurns` uses for `omo-transcript`: `maxTurns` from the caller,
/// `tool_images` false (only pi keeps a tool's images in the entry as base64; omo would
/// carry refs nothing can answer), and `task_titles` as the caller's map so a title seen on
/// an earlier page still names a task that ends on this one.
///
/// Adjacent assistant messages merge into one turn and toolCall parts adopt the output of
/// the toolResult entry that answers them. An assistant message that stopped for good
/// (`stopReason: "stop"`) ends its turn: the next one was woken by something nobody typed.
pub fn parse_omo_transcript(
    text: &str,
    max_turns: usize,
    tool_images: bool,
    task_titles: &mut HashMap<String, String>,
) -> Vec<ReferenceTurn> {
    let mut turns: Vec<ReferenceTurn> = Vec::new();
    /// Tool parts still waiting for their result: (turn index, part index).
    let mut pending: HashMap<String, (usize, usize)> = HashMap::new();
    /// The last assistant message stopped for good: the next one starts a turn of its own.
    let mut settled = false;

    for line in text.split('\n') {
        if line.trim().is_empty() {
            continue;
        }
        let entry: Value = match serde_json::from_str(line) {
            Ok(entry) => entry,
            // a torn tail line while omo is mid-append
            Err(_) => continue,
        };
        if !entry.is_object() {
            continue;
        }
        if is_omo_context_clear(&entry) {
            turns.clear();
            pending.clear();
            continue;
        }
        let timestamp = Row::of(&entry).string("timestamp").map(str::to_string);

        if let Some(notice) = omo_pi_notice(&entry) {
            turns.push(runtime_turn(timestamp.as_deref(), vec![notice]));
            continue;
        }
        if let Some(tasks) = omo_task_results(&entry, task_titles) {
            turns.push(runtime_turn(timestamp.as_deref(), vec![ReferencePart::TaskResult { tasks }]));
            continue;
        }
        // A compaction is a tree entry, not a message, so it reaches the chat only through
        // this branch: the card says where the conversation was cut, and the agent keeps
        // answering from the summary onward. `omo_pi_notice` only claims `custom_message`.
        if Row::of(&entry).string("type") == Some("compaction") {
            if let Some(summary) = Row::of(&entry).string("summary").map(str::to_string) {
                if !summary.trim().is_empty() {
                    turns.push(runtime_turn(
                        timestamp.as_deref(),
                        vec![ReferencePart::Compact { text: summary }],
                    ));
                }
            }
            continue;
        }

        let Some(message) = omo_pi_message(&entry) else {
            continue;
        };
        let message_row = Row::of(&message);
        let role = message_row.string("role").unwrap_or("");

        if role != "assistant" {
            let results = omo_pi_results(&message);
            apply_omo_tool_results(&mut turns, &mut pending, &results, tool_images);
        }
        omo_task_titles(&message, task_titles);

        if role == "user" {
            let content = message_row.get("content");
            let prompt = match content {
                Some(Value::String(text)) => text.clone(),
                Some(Value::Array(items)) => items
                    .iter()
                    .map(|item| {
                        let part = Row::of(item);
                        if part.string("type") == Some("text") {
                            part.string("text").unwrap_or("")
                        } else {
                            ""
                        }
                    })
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n"),
                _ => String::new(),
            };
            // image-only user parts have no text to show
            if prompt.is_empty() {
                continue;
            }
            // A skill invocation reads as what the user asked, the skill as a chip on it: not
            // as the SKILL.md the runtime put before it. The request is the user's text and each
            // loaded skill rides it as its own part, in the order the envelope named them
            // (`transcript-records.ts:259`).
            let invocation = omo_skill_invocation_prompt(&prompt);
            let asked = match &invocation {
                Some(invocation) if !invocation.request.is_empty() => invocation.request.clone(),
                Some(invocation) => invocation
                    .skills
                    .iter()
                    .map(|skill| format!("/skill:{}", skill.name))
                    .collect::<Vec<_>>()
                    .join(" "),
                None => prompt.clone(),
            };
            let mut turn_parts = vec![ReferencePart::Text { text: asked, phase: None }];
            if let Some(invocation) = &invocation {
                turn_parts.extend(
                    invocation
                        .skills
                        .iter()
                        .cloned()
                        .map(|skill| ReferencePart::Skill { skill }),
                );
            }
            turns.push(ReferenceTurn {
                role: ReferenceTurnRole::User,
                started_at: timestamp.clone(),
                ended_at: None,
                source: None,
                parts: turn_parts,
                abandoned: None,
            });
            continue;
        }

        if role == "assistant" {
            let turn_index = assistant_turn(&mut turns, &timestamp, settled);
            if timestamp.is_some() {
                turns[turn_index].ended_at = timestamp.clone();
            }
            if let Some(blocks) = message_row.array("content") {
                for block in blocks {
                    let block = Row::of(block);
                    match block.string("type") {
                        Some("text") => {
                            if let Some(text) = block.string("text").filter(|text| !text.is_empty()) {
                                turns[turn_index].parts.push(ReferencePart::Text { text: text.to_string(), phase: None });
                            }
                        }
                        Some("thinking") => {
                            let thinking = block.string("thinking").or_else(|| block.string("text")).unwrap_or("");
                            if !thinking.is_empty() {
                                turns[turn_index].parts.push(ReferencePart::Thinking { text: thinking.to_string() });
                            }
                        }
                        Some("toolCall") => {
                            let Some(name) = block.string("name") else {
                                continue;
                            };
                            let input = block
                                .get("arguments")
                                .filter(|value| value.is_object())
                                .cloned()
                                .unwrap_or_else(|| Value::Object(Map::new()));
                            let summary = match block.string("intent").filter(|intent| !intent.is_empty()) {
                                Some(intent) => intent.to_string(),
                                None => omo_tool_summary(name, &input),
                            };
                            let part_index = turns[turn_index].parts.len();
                            turns[turn_index].parts.push(ReferencePart::Tool {
                                name: name.to_string(),
                                summary: summary.chars().take(120).collect(),
                                input: serde_json::to_string_pretty(&input).unwrap_or_else(|_| input.to_string()),
                                output: String::new(),
                                error: None,
                                skill: None,
                                output_ref: None,
                                output_size: None,
                                images: Vec::new(),
                            });
                            if let Some(id) = block.string("id") {
                                pending.insert(id.to_string(), (turn_index, part_index));
                            }
                        }
                        // unsupported transcript parts are intentionally ignored
                        _ => {}
                    }
                }
            }
            let results = omo_pi_results(&message);
            apply_omo_tool_results(&mut turns, &mut pending, &results, tool_images);
            // a failed request (a 401, an overloaded provider) leaves an empty message: without
            // its error the chat showed the prompt with no answer at all
            if message_row.string("stopReason") == Some("error") {
                if let Some(error) = message_row.string("errorMessage").filter(|error| !error.is_empty()) {
                    turns[turn_index].parts.push(ReferencePart::Text { text: format!("Error: {error}"), phase: None });
                }
            }
            settled = message_row.string("stopReason") == Some("stop");
        }
    }

    turns.retain(|turn| !turn.parts.is_empty());
    if turns.len() > max_turns {
        turns.split_off(turns.len() - max_turns)
    } else {
        turns
    }
}

/// Does this line open a turn? Pages start at such lines, so a page never splits a turn.
///
/// Upstream `opensTurn("omo-transcript", line)`: a `message` record whose role is `user`
/// with at least one non-empty text block. The page scanner filters on
/// [`OMO_TURN_MARK`] first, exactly as `turnStarts` does.
pub fn omo_line_opens_turn(line: &str) -> bool {
    let Ok(entry) = serde_json::from_str::<Value>(line) else {
        return false;
    };
    if !entry.is_object() {
        return false;
    }
    let Some(message) = omo_pi_message(&entry) else {
        return false;
    };
    let message = Row::of(&message);
    if message.string("role") != Some("user") {
        return false;
    }
    message
        .array("content")
        .map(|items| {
            items.iter().any(|item| {
                let part = Row::of(item);
                part.string("type") == Some("text") && part.string("text").is_some_and(|text| !text.is_empty())
            })
        })
        .unwrap_or(false)
}

/// The OmO history family entry point: bytes in, turns out.
///
/// Satisfies [`super::types::ReferenceHistoryParser`]. The omo store keeps the omp session
/// shape as a log, so the whole stream is read without an entry-tree projection; a page
/// reader that has already seen earlier pages passes its inherited titles through
/// [`parse_omo_transcript`] instead.
///
/// Only [`ReferenceNativeHistoryKind::Omo`] is this lane's. Every other kind — including
/// `Unavailable` — is an error, never an empty success: an empty success would be read as
/// "this session has no turns".
pub fn parse_omo_history(kind: ReferenceNativeHistoryKind, text: &str) -> Result<Vec<ReferenceTurn>, String> {
    if kind != ReferenceNativeHistoryKind::Omo {
        return Err(format!(
            "the omo history family reads `omo-transcript` only; `{}` belongs to another lane",
            kind.as_str()
        ));
    }
    let mut titles: HashMap<String, String> = HashMap::new();
    Ok(parse_omo_transcript(text, usize::MAX, false, &mut titles))
}

/// The frozen lane signature, so a dispatcher can hold the omo family as a value.
pub const OMO_HISTORY_PARSER: super::types::ReferenceHistoryParser = parse_omo_history;

// ---------------------------------------------------------------------------------------
// OmO store and process shape (pure; acquisition stays with the resolver)
// ---------------------------------------------------------------------------------------

/// The session folder OmO's engine names for a cwd: the leading slash dropped, then every
/// slash, backslash and colon a dash. A Windows cwd (`C:\Users\me\app`) is
/// `--C--Users-me-app--`. Upstream `omoSessionFolder`.
pub fn omo_session_folder(cwd: &str) -> String {
    let trimmed = cwd.strip_prefix('/').or_else(|| cwd.strip_prefix('\\')).unwrap_or(cwd);
    let slug: String = trimmed
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' => '-',
            other => other,
        })
        .collect();
    format!("--{slug}--")
}

/// The header OmO writes on its session file's first line, as far as the record shape goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OmoSessionHeader {
    pub id: String,
    pub cwd: String,
    /// The recorded start, verbatim. A selector never orders by mtime: tool activity is not
    /// evidence of pane ownership.
    pub timestamp: Option<String>,
}

/// Parse OmO's session header line, accepting it only for the cwd it was found under.
/// Upstream `omo.ts`'s `candidate()` header read: `type === "session"`, matching `cwd`, and
/// a string `id`.
pub fn omo_session_header(first_line: &str, cwd: &str) -> Option<OmoSessionHeader> {
    let entry: Value = serde_json::from_str(first_line).ok()?;
    let row = Row::of(&entry);
    if row.string("type") != Some("session") || row.string("cwd") != Some(cwd) {
        return None;
    }
    Some(OmoSessionHeader {
        id: row.string("id")?.to_string(),
        cwd: cwd.to_string(),
        timestamp: row.string("timestamp").map(str::to_string),
    })
}

fn omo_process_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(^|/)omo(\.js)?$|/omo-ai/").expect("the pinned omo program pattern is valid")
    })
}

fn js_runtime_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(^|/)(node|nodejs|bun)$").expect("the pinned runtime pattern is valid"))
}

fn senpi_entry_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"/@code-yeongyu/senpi/dist/(bundle/)?cli\.js$")
            .expect("the pinned senpi entry pattern is valid")
    })
}

fn omo_extension_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"/omo-ai/plugin/?$").expect("the pinned omo extension pattern is valid"))
}

fn windows_path_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\\|^[A-Za-z]:[\\/]|\.(exe|cmd|bat)$").expect("the pinned windows path pattern is valid")
    })
}

fn windows_extension_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\.(exe|cmd|bat)$").expect("the pinned windows extension pattern is valid"))
}

/// A Windows PC's process words read as the same paths: backslashes as slashes, and a
/// program's `.exe`/`.cmd` dropped (`C:\Users\me\.bun\bin\bun.exe` is bun, `omo.cmd` is omo).
fn windows_path(word: &str) -> String {
    if !windows_path_regex().is_match(word) {
        return word.to_string();
    }
    windows_extension_regex().replace_all(&word.replace('\\', "/"), "").into_owned()
}

/// A PATH-like list (`a:b`), not one path: a drive's colon (`C:/…`) does not make one.
fn is_path_list(word: &str) -> bool {
    let stripped = word
        .strip_prefix(|character: char| character.is_ascii_alphabetic())
        .and_then(|rest| rest.strip_prefix(':'))
        .and_then(|rest| rest.strip_prefix('/'))
        .map(|rest| rest.to_string())
        .unwrap_or_else(|| word.to_string());
    stripped.contains(':')
}

/// Is this process OmO, whatever the pane's label says?
///
/// Upstream `isOmoProcess`: only the program counts — `argv[0]`, or the script a JS runtime
/// runs (`bun …/omo-ai/…/cli.js`, `node …/bin/omo`). An omo-ai path handed to another
/// program (`grep -q …/omo-ai/x`) is that program's argument, not omo. A global bun install
/// hoists omo's engine next to omo-ai instead of inside it, so senpi's own entry counts only
/// with omo-ai's plugin as its `--extension`.
pub fn is_omo_process(argv: &[String]) -> bool {
    let words: Vec<String> = argv.iter().map(|word| windows_path(word)).collect();
    let runtime = words.first().is_some_and(|word| js_runtime_regex().is_match(word));
    let at = if runtime {
        words
            .iter()
            .skip(1)
            .position(|word| !word.starts_with('-'))
            .map(|index| index + 1)
            .unwrap_or(0)
    } else {
        0
    };
    let program = if at > 0 || !runtime { words.get(at).cloned() } else { None };
    let Some(program) = program else {
        return false;
    };
    if is_path_list(&program) {
        return false;
    }
    if omo_process_regex().is_match(&program) {
        return true;
    }
    if !runtime || !senpi_entry_regex().is_match(&program) {
        return false;
    }
    let prompt = words
        .iter()
        .skip(at)
        .position(|word| word == "--")
        .map(|index| index + at);
    let options: &[String] = match prompt {
        Some(prompt) => &words[..prompt],
        None => &words,
    };
    options.iter().enumerate().any(|(index, word)| {
        index > 0
            && options[index - 1] == "--extension"
            && !is_path_list(word)
            && omo_extension_regex().is_match(word)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const AUTHENTIC: &str = include_str!("fixtures/omo/session-authentic.jsonl");
    const PARTIAL: &str = include_str!("fixtures/omo/session-partial.jsonl");
    const CONTEXT_CLEAR: &str = include_str!("fixtures/omo/session-context-clear.jsonl");
    const STOP_ERROR: &str = include_str!("fixtures/omo/session-stop-error.jsonl");

    fn parse(text: &str) -> Vec<ReferenceTurn> {
        let mut titles = HashMap::new();
        parse_omo_transcript(text, usize::MAX, false, &mut titles)
    }

    fn part_kinds(turn: &ReferenceTurn) -> Vec<super::super::types::ReferencePartKind> {
        turn.parts.iter().map(ReferencePart::kind).collect()
    }

    #[test]
    fn authentic_session_parses_its_turns_in_order() {
        let turns = parse(AUTHENTIC);
        let roles: Vec<ReferenceTurnRole> = turns.iter().map(|turn| turn.role).collect();
        assert_eq!(
            roles,
            vec![
                ReferenceTurnRole::User,
                ReferenceTurnRole::Assistant,
                ReferenceTurnRole::User,
                ReferenceTurnRole::User,
                ReferenceTurnRole::User,
                ReferenceTurnRole::Assistant,
                ReferenceTurnRole::Assistant,
            ],
            "the fixture's record order decides the turn order"
        );
        assert_eq!(turns[0].parts, vec![ReferencePart::Text { text: "add a retry to the uploader".into(), phase: None }]);
        assert_eq!(turns[0].started_at.as_deref(), Some("2026-10-06T10:00:05.000Z"));
        assert_eq!(
            part_kinds(&turns[1]),
            vec![
                super::super::types::ReferencePartKind::Thinking,
                super::super::types::ReferencePartKind::Text,
                super::super::types::ReferencePartKind::Tool,
                super::super::types::ReferencePartKind::Tool,
            ]
        );
        assert_eq!(turns[1].ended_at.as_deref(), Some("2026-10-06T10:00:20.000Z"));
    }

    #[test]
    fn tool_results_answer_the_call_that_started_them() {
        let turns = parse(AUTHENTIC);
        let ReferencePart::Tool { name, summary, input, output, error, .. } = &turns[1].parts[2] else {
            panic!("the read call is a tool part");
        };
        assert_eq!(name, "read");
        assert_eq!(summary, "src/uploader.ts");
        assert_eq!(input, "{\n  \"path\": \"src/uploader.ts\"\n}");
        assert_eq!(output, "export function upload() {}\n");
        assert_eq!(*error, None);

        let ReferencePart::Tool { name, summary, .. } = &turns[1].parts[3] else {
            panic!("the task call is a tool part");
        };
        assert_eq!(name, "task");
        assert_eq!(summary, "wire the retry");
    }

    #[test]
    fn display_false_records_never_reach_the_chat() {
        let turns = parse(AUTHENTIC);
        assert!(
            !turns.iter().any(|turn| turn.parts.iter().any(|part| matches!(
                part,
                ReferencePart::Text { text, .. } if text == "hidden"
            ))),
            "a record the runtime marks display:false is not a turn"
        );
    }

    #[test]
    fn task_wake_becomes_a_runtime_turn_with_the_title_from_its_call() {
        let turns = parse(AUTHENTIC);
        let turn = turns
            .iter()
            .find(|turn| turn.parts.iter().any(|part| matches!(part, ReferencePart::TaskResult { .. })))
            .expect("the senpi-task.completion wake is a turn");
        assert_eq!(turn.role, ReferenceTurnRole::User);
        assert_eq!(turn.source, Some(ReferenceTurnSource::Runtime));
        let ReferencePart::TaskResult { tasks } = &turn.parts[0] else {
            panic!("the wake's part is a task result");
        };
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, "t-77");
        assert_eq!(tasks[0].title, "wire the retry", "the task call's summary names it");
        assert_eq!(tasks[0].status, ReferenceTaskStatus::Completed);
        assert_eq!(tasks[0].agent.as_deref(), Some("executor"));
        assert_eq!(tasks[0].model.as_deref(), Some("deepseek-v4.1-flash"));
        assert_eq!(tasks[0].duration_ms, Some(98_000));
        assert_eq!(tasks[0].turns, Some(12));
        assert_eq!(tasks[0].tool_calls, Some(30));
        assert_eq!(tasks[0].tokens, Some(42_000));
        assert_eq!(tasks[0].result, "retry wired and covered");
        assert_eq!(tasks[0].result_cut, None);
    }

    #[test]
    fn notice_strips_the_system_notice_envelope() {
        let turns = parse(AUTHENTIC);
        let notice = turns
            .iter()
            .find_map(|turn| turn.parts.iter().find(|part| matches!(part, ReferencePart::Notice { .. })))
            .expect("the custom_message notice is a part");
        assert_eq!(
            notice,
            &ReferencePart::Notice { text: "build finished".into(), source: Some("info".into()) }
        );
    }

    #[test]
    fn compaction_entry_becomes_a_compact_part_in_the_user_seat() {
        let turns = parse(AUTHENTIC);
        let turn = turns
            .iter()
            .find(|turn| turn.parts.iter().any(|part| matches!(part, ReferencePart::Compact { .. })))
            .expect("the compaction entry is a turn");
        assert_eq!(turn.role, ReferenceTurnRole::User);
        assert_eq!(turn.source, Some(ReferenceTurnSource::Runtime));
        assert_eq!(
            turn.parts[0],
            ReferencePart::Compact { text: "Earlier work established the uploader layout.".into() }
        );
    }

    #[test]
    fn a_settled_assistant_message_starts_the_next_turn() {
        let turns = parse(AUTHENTIC);
        let last_two = &turns[turns.len() - 2..];
        assert_eq!(last_two[0].parts, vec![ReferencePart::Text { text: "Continuing from the summary.".into(), phase: None }]);
        assert_eq!(last_two[1].parts, vec![ReferencePart::Text { text: "Woken by a monitor.".into(), phase: None }]);
        assert_eq!(last_two[1].started_at.as_deref(), Some("2026-10-06T10:03:20.000Z"));
    }

    #[test]
    fn torn_tail_and_non_object_lines_are_skipped_without_losing_the_turns_around_them() {
        let turns = parse(PARTIAL);
        assert_eq!(turns.len(), 3, "the unparseable lines are skipped, not fatal");
        assert_eq!(turns[0].parts, vec![ReferencePart::Text { text: "kept".into(), phase: None }]);
        assert_eq!(turns[1].parts, vec![ReferencePart::Text { text: "   ".into(), phase: None }]);
        assert_eq!(turns[2].parts, vec![ReferencePart::Text { text: "plain string content".into(), phase: None }]);
        assert!(turns[2].parts.iter().all(|part| part.is_inline_text()));
    }

    #[test]
    fn a_context_clear_empties_the_conversation_before_it() {
        let turns = parse(CONTEXT_CLEAR);
        assert_eq!(turns.len(), 2, "only the turns after the reset survive");
        assert_eq!(turns[0].parts, vec![ReferencePart::Text { text: "second".into(), phase: None }]);
        assert_eq!(turns[1].parts, vec![ReferencePart::Text { text: "second answer".into(), phase: None }]);
    }

    #[test]
    fn a_failed_assistant_message_keeps_its_error() {
        let turns = parse(STOP_ERROR);
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[1].parts, vec![ReferencePart::Text { text: "Error: 401 unauthorized".into(), phase: None }]);
        let ReferencePart::Text { text, .. } = &turns[1].parts[0] else {
            panic!("the error is a text part");
        };
        assert!(text.starts_with("Error: "));
    }

    #[test]
    fn tool_output_is_trimmed_at_the_page_limit_with_a_ref_for_the_rest() {
        let long = "x".repeat(OMO_TOOL_OUTPUT_CHARS + 10);
        let trimmed = trim_omo_tool_output("read", &long, "call-9");
        assert_eq!(trimmed.output.chars().count(), OMO_TOOL_OUTPUT_CHARS + "\n… trimmed".chars().count());
        assert!(trimmed.output.ends_with("\n… trimmed"));
        assert_eq!(trimmed.output_ref.as_deref(), Some("call-9"));
        assert_eq!(trimmed.output_size, Some(long.chars().count() as u64));

        let short = trim_omo_tool_output("read", "ok", "call-9");
        assert_eq!(short, OmoTrimmedOutput { output: "ok".into(), output_ref: None, output_size: None });
    }

    #[test]
    fn goal_tools_get_the_whole_output_limit() {
        let goal = "y".repeat(OMO_TOOL_OUTPUT_CHARS + 10);
        assert!(goal.chars().count() <= OMO_WHOLE_OUTPUT_CHARS);
        let kept = trim_omo_tool_output("update_goal", &goal, "call-1");
        assert_eq!(kept.output, goal, "a finished goal's answer keeps its status");
        assert_eq!(kept.output_ref, None);
        assert!(OMO_WHOLE_OUTPUT_TOOLS.contains(&"get_goal"));
    }

    #[test]
    fn tool_summary_prefers_the_task_summaries_then_the_first_argument() {
        let batch = json!({"tasks": [{"task_summary": "one"}, {"description": "two"}]});
        assert_eq!(omo_tool_summary("task", &batch), "one · two");
        let single = json!({"task_summary": "solo"});
        assert_eq!(omo_tool_summary("task", &single), "solo");
        assert_eq!(omo_tool_summary("read", &json!({"path": "src/a.ts"})), "src/a.ts");
        assert_eq!(omo_tool_summary("bash", &json!({"command": "ls -la"})), "ls -la");
        assert_eq!(omo_tool_summary("mystery", &json!({})), "mystery");
    }

    #[test]
    fn task_titles_come_from_the_task_call_and_a_later_wake_overwrites_its_row() {
        let mut titles = HashMap::new();
        omo_task_titles(
            &json!({"role": "toolResult", "toolName": "task", "details": {"items": [{"task_id": "t1", "task_summary": "first"}]}}),
            &mut titles,
        );
        assert_eq!(titles.get("t1").map(String::as_str), Some("first"));
        omo_task_titles(&json!({"role": "toolResult", "toolName": "read", "details": {}}), &mut titles);
        assert_eq!(titles.len(), 1, "only a task call names its tasks");

        let wake = json!({"type": "custom_message", "customType": "omo-senpi:wake", "display": false, "details": [
            {"customType": "senpi-task.completion", "details": [{"task_id": "t1", "status": "failed", "error": "boom"}]}
        ]});
        let tasks = omo_task_results(&wake, &titles).expect("the wake reports its task");
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].title, "first", "the call's summary outlives the page it was read on");
        assert_eq!(tasks[0].status, ReferenceTaskStatus::Failed);
        assert_eq!(tasks[0].result, "boom");
        assert_eq!(tasks[0].model, None);
    }

    #[test]
    fn a_wake_without_a_task_result_is_not_a_task_result() {
        let monitor = json!({"type": "custom_message", "customType": "omo-senpi:wake", "details": [
            {"customType": "monitor.event", "details": [{"task_id": "t1", "status": "completed"}]}
        ]});
        assert_eq!(omo_task_results(&monitor, &HashMap::new()), None);
        assert_eq!(omo_task_results(&json!({"type": "custom_message", "customType": "info"}), &HashMap::new()), None);
    }

    #[test]
    fn a_long_task_answer_is_cut_and_flagged() {
        let answer = "z".repeat(OMO_TASK_RESULT_MAX_CHARS + 5);
        let wake = json!({"type": "custom_message", "customType": "omo-senpi:wake", "details": [
            {"customType": "senpi-task.completion", "details": [
                {"task_id": "t9", "status": "completed", "final_response": answer}
            ]}
        ]});
        let tasks = omo_task_results(&wake, &HashMap::new()).unwrap();
        assert_eq!(tasks[0].result.chars().count(), OMO_TASK_RESULT_MAX_CHARS);
        assert_eq!(tasks[0].result_cut, Some(true));
        assert_eq!(tasks[0].title, "t9", "with no name and no agent the id names it");
    }

    #[test]
    fn a_skill_invocation_prompt_keeps_the_request_and_names_its_skill() {
        let envelope = format!(
            "The user explicitly invoked the \"review\" skill. Follow the instructions in <skill-instruction> as binding for this request, while respecting higher-priority instructions.\n\n<skill-instruction name=\"review\" location=\"/skills/review/SKILL.md\">\n{}\n</skill-instruction>\n\n<user-request>\nreview the diff\n</user-request>",
            "x".repeat(1000)
        );
        let invocation = omo_skill_invocation_prompt(&envelope).expect("the envelope is a skill invocation");
        assert_eq!(invocation.request, "review the diff");
        assert_eq!(invocation.skills.len(), 1);
        assert_eq!(invocation.skills[0].name, "review");
        assert_eq!(invocation.skills[0].evidence, ReferenceSkillEvidence::Instructions);
        assert_eq!(invocation.skills[0].status, ReferenceSkillStatus::Loaded);
        assert_eq!(invocation.skills[0].path.as_deref(), Some("/skills/review/SKILL.md"));

        assert_eq!(omo_skill_invocation_prompt("just a normal prompt"), None);
        let legacy = "<skill name=\"tidy\" location=\"tidy.md\">\nbody\n</skill>\n\ndo it";
        let invocation = omo_skill_invocation_prompt(legacy).expect("the legacy envelope is a skill invocation");
        assert_eq!(invocation.skills[0].name, "tidy");
        assert_eq!(invocation.request, "do it");
    }

    #[test]
    fn a_skill_prompt_reaches_the_turn_as_the_request_that_was_typed() {
        let envelope = format!(
            "The user explicitly invoked the \"review\" skill. Follow the instructions in <skill-instruction> as binding for this request, while respecting higher-priority instructions.\n\n<skill-instruction name=\"review\" location=\"/skills/review/SKILL.md\">\n{}\n</skill-instruction>",
            "x".repeat(500)
        );
        let line = serde_json::to_string(&json!({
            "type": "message",
            "message": {"role": "user", "content": envelope}
        }))
        .unwrap();
        let turns = parse(&line);
        assert_eq!(turns.len(), 1);
        // No `<user-request>` under the envelope: the runtime's own name for the skill is the text,
        // and the skill is its own part beside it.
        assert_eq!(
            turns[0].parts,
            vec![
                ReferencePart::Text { text: "/skill:review".into(), phase: None },
                ReferencePart::Skill {
                    skill: ReferenceSkillActivity {
                        name: "review".into(),
                        evidence: ReferenceSkillEvidence::Instructions,
                        status: ReferenceSkillStatus::Loaded,
                        path: Some("/skills/review/SKILL.md".into()),
                    }
                },
            ],
            "the request is the user's text and the skill rides it as its own part"
        );

        // A prompt that merely mentions the tag is the user's own text, with no chip on it.
        let plain = serde_json::to_string(&json!({
            "type": "message",
            "message": {"role": "user", "content": "the skill-instruction tag is documented in the README"}
        }))
        .unwrap();
        assert_eq!(
            parse(&plain)[0].parts,
            vec![ReferencePart::Text {
                text: "the skill-instruction tag is documented in the README".into(),
                phase: None
            }]
        );
    }

    #[test]
    fn the_omo_family_refuses_every_other_registry_kind() {
        assert!(parse_omo_history(ReferenceNativeHistoryKind::Omo, AUTHENTIC).is_ok());
        for kind in [
            ReferenceNativeHistoryKind::Claude,
            ReferenceNativeHistoryKind::Codex,
            ReferenceNativeHistoryKind::Omp,
            ReferenceNativeHistoryKind::Gjc,
            ReferenceNativeHistoryKind::Pi,
            ReferenceNativeHistoryKind::Unavailable,
        ] {
            let error = parse_omo_history(kind, AUTHENTIC).expect_err("another lane's kind is refused");
            assert!(error.contains(kind.as_str()), "{error} names the refused kind");
        }
    }

    #[test]
    fn the_lane_signature_is_held_as_a_value() {
        let parser = OMO_HISTORY_PARSER;
        assert_eq!(parser(ReferenceNativeHistoryKind::Omo, AUTHENTIC).unwrap().len(), 7);
        assert!(parser(ReferenceNativeHistoryKind::Omp, AUTHENTIC).is_err());
    }

    #[test]
    fn a_max_turn_bound_keeps_the_newest_turns() {
        let mut titles = HashMap::new();
        let turns = parse_omo_transcript(AUTHENTIC, 2, false, &mut titles);
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[1].parts, vec![ReferencePart::Text { text: "Woken by a monitor.".into(), phase: None }]);
        assert_eq!(parse_omo_transcript(AUTHENTIC, OMO_MAX_TURNS, false, &mut HashMap::new()).len(), 7);
    }

    #[test]
    fn the_turn_marker_and_the_page_scanner_agree_on_what_opens_a_turn() {
        assert_eq!(OMO_TURN_MARK, "\"user\"");
        let opens = "{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"hi\"}]}}";
        assert!(opens.contains(OMO_TURN_MARK));
        assert!(omo_line_opens_turn(opens));
        assert!(!omo_line_opens_turn(
            "{\"type\":\"message\",\"message\":{\"role\":\"toolResult\",\"toolCallId\":\"c1\",\"content\":\"ok\"}}"
        ));
        assert!(!omo_line_opens_turn("{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":[]}}"));
        assert!(!omo_line_opens_turn("{\"type\":\"message\""));
        assert!(!omo_line_opens_turn("[1,2,3]"));
    }

    #[test]
    fn image_results_are_refs_only_when_the_caller_asks_for_them() {
        let line = serde_json::to_string(&json!({
            "type": "message",
            "message": {"role": "assistant", "content": [
                {"type": "toolCall", "id": "call-img", "name": "read", "arguments": {"path": "a.png"}}
            ]}
        }))
        .unwrap();
        let result = serde_json::to_string(&json!({
            "type": "message",
            "message": {"role": "toolResult", "toolCallId": "call-img", "toolName": "read", "content": [
                {"type": "image", "mimeType": "image/png", "data": "AAA"},
                {"type": "text", "text": "one picture"}
            ]}
        }))
        .unwrap();
        let text = format!("{line}\n{result}");

        let without = parse_omo_transcript(&text, usize::MAX, false, &mut HashMap::new());
        let ReferencePart::Tool { output, images, .. } = &without[0].parts[0] else {
            panic!("the call is a tool part");
        };
        assert_eq!(output, "one picture");
        assert!(images.is_empty(), "omo carries no image refs nothing can answer");

        let with = parse_omo_transcript(&text, usize::MAX, true, &mut HashMap::new());
        let ReferencePart::Tool { images, .. } = &with[0].parts[0] else {
            panic!("the call is a tool part");
        };
        assert_eq!(
            images,
            &vec![ReferenceImageRef { media_type: "image/png".into(), r#ref: "pi:call-img:0".into() }]
        );
    }

    #[test]
    fn a_failed_tool_result_marks_its_call() {
        let text = [
            "{\"type\":\"message\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"toolCall\",\"id\":\"c1\",\"name\":\"bash\",\"arguments\":{\"command\":\"false\"}}]}}",
            "{\"type\":\"message\",\"message\":{\"role\":\"toolResult\",\"toolCallId\":\"c1\",\"toolName\":\"bash\",\"isError\":true,\"content\":\"exit 1\"}}",
        ]
        .join("\n");
        let turns = parse_omo_transcript(&text, usize::MAX, false, &mut HashMap::new());
        let ReferencePart::Tool { output, error, .. } = &turns[0].parts[0] else {
            panic!("the call is a tool part");
        };
        assert_eq!(output, "exit 1");
        assert_eq!(*error, Some(true));
    }

    #[test]
    fn omo_process_identity_reads_the_program_and_never_a_handed_over_path() {
        let words = |argv: &[&str]| argv.iter().map(|word| word.to_string()).collect::<Vec<_>>();
        assert!(is_omo_process(&words(&["/usr/local/bin/omo"])));
        assert!(is_omo_process(&words(&["/home/me/node_modules/.bin/omo.js"])));
        assert!(is_omo_process(&words(&["bun", "/home/me/omo-ai/dist/cli.js"])));
        assert!(is_omo_process(&words(&["C:\\Users\\me\\.bun\\bin\\bun.exe", "C:\\Users\\me\\.bun\\bin\\omo.cmd"])));
        assert!(is_omo_process(&words(&[
            "/Users/me/.bun/bin/bun",
            "/Users/me/node_modules/@code-yeongyu/senpi/dist/bundle/cli.js",
            "--extension",
            "/Users/me/node_modules/omo-ai/plugin",
        ])));
        assert!(!is_omo_process(&words(&["grep", "-q", "/home/me/omo-ai/x"])));
        assert!(!is_omo_process(&words(&["cat", "/home/me/bin/omo"])));
        assert!(!is_omo_process(&words(&[
            "/Users/me/.bun/bin/bun",
            "/Users/me/node_modules/@code-yeongyu/senpi/dist/cli.js",
        ])));
        assert!(!is_omo_process(&words(&["/a/omo-ai:/b/omo"])));
        assert!(!is_omo_process(&words(&["/bin/zsh", "-c", "echo hi"])));
        assert!(!is_omo_process(&words(&[])));
    }

    #[test]
    fn session_folder_and_header_read_the_omo_store_shape() {
        assert_eq!(omo_session_folder("/Users/me/app"), "--Users-me-app--");
        assert_eq!(omo_session_folder("C:\\Users\\me\\app"), "--C--Users-me-app--");
        assert_eq!(omo_session_folder("/a/b:c"), "--a-b-c--");

        let header = omo_session_header(
            "{\"type\":\"session\",\"id\":\"sess-omo-0001\",\"cwd\":\"/home/dev/app\",\"timestamp\":\"2026-10-06T10:00:00.000Z\"}",
            "/home/dev/app",
        )
        .expect("the fixture header is a session header");
        assert_eq!(header.id, "sess-omo-0001");
        assert_eq!(header.timestamp.as_deref(), Some("2026-10-06T10:00:00.000Z"));
        assert_eq!(omo_session_header("{\"type\":\"session\",\"id\":\"x\",\"cwd\":\"/other\"}", "/home/dev/app"), None);
        assert_eq!(omo_session_header("{\"type\":\"message\"}", "/home/dev/app"), None);
        assert_eq!(omo_session_header("not json", "/home/dev/app"), None);
    }
}
