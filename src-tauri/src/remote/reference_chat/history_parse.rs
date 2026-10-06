//! Reference-chat history dispatch, and the Codex native reader (plan task 2).
//!
//! Ported from `devswha/herdr-web-ui` @ `54e5a1f67090cb09552d182e7e30dd0ecc314918`
//! (MIT, see `docs/chat/HERDR_LICENSE`). Upstream anchors, read at the pinned revision:
//!
//! | Upstream | What this module ports |
//! |---|---|
//! | `server/conversation.ts` | `parseTurns` — the per-source dispatch this module is, and the Codex `TURN_MARK` / `opensTurn` pair the pager needs |
//! | `server/codex.ts` | `createCodexTranscriptParser`, `parseCodexTranscript`, `codexOutputText`, `codexCallFailed`, `contextOnly`, `questionReply`, `questionTitles`, `contentText`, `entries`, `withoutMemoryCitations`, `codexReadSkills`, `codexReadCall`, `selectedSkill` |
//! | `server/codex-images.ts` | `CODEX_IMAGE_REF`, `codexImageParts`, `imagePart`, `imageValues` |
//! | `server/skill-activity.ts` | `skillDocument`, `label` |
//! | `server/patch.ts` | `patchText`, `patchFiles` |
//! | `server/tool-output.ts` | `trimOutput` (`TOOL_OUTPUT_CHARS` / `WHOLE_OUTPUT_CHARS`) |
//! | `shared/protocol.ts` | `ConversationTurn` / `ConversationPart` |
//!
//! Frozen contract: `docs/chat/herdr-port-contract.md` section 6. [`dispatch_reference_history`]
//! is the shared byte-level entry point the contract names; it hands each
//! [`ReferenceNativeHistoryKind`] to the lane that owns it (tasks 17–21) and reads
//! `codex-transcript` itself, because the two readers Ferryx already had for Codex are not
//! equivalent to the pinned one: `crate::agent_transcript` reads only omp/pi-shaped `message`
//! records under `~/.omo/agent/sessions`, and `crate::ferryx_scope::history` reduces a rollout to
//! flat `role`/`text` messages. Neither can carry a Codex turn's thinking, tool, image or skill
//! parts, its `event_msg`/`response_item` duplicate pairing, or its `end_ts` activity rule, so the
//! pinned parser is ported here rather than adapted from either.
//!
//! ## Boundaries, recorded rather than silently exceeded
//!
//! * **Which rollout belongs to a pane** — `codexTranscriptPath`, `paneCodexHome`,
//!   `codexHistorySegments` and the `historyChain`/`liveChain` paging — is resolution, and it is
//!   task 3's. This module reads the bytes it is handed and never guesses a session from a cwd.
//! * **Image bytes are never read here.** A part carries `codex-<sha256 of the source value>` and
//!   nothing else; `codexTranscriptImage` (which resolves the file on the owning host) belongs to
//!   the file lane.
//! * **The live tail preview is not ported.** Upstream's `snapshot(tail)` parses an uncommitted
//!   final line for the polling path (`codexLiveTurn`). A byte-level reader commits complete lines
//!   only: a torn tail is skipped, exactly as `transcript-records.ts` skips one.
//! * **Standalone skill parts are ported.** Upstream pushes `{ kind: "skill", skill }` for a
//!   completed read whose skill no tool part carries (`codex.ts:241`) and for Codex's explicitly
//!   selected skill (`codex.ts:275`). Both reach the page as [`ReferencePart::Skill`]. A skill the
//!   pinned reader attaches to a *tool call* still stays on that call
//!   ([`ReferencePart::Tool::skill`]) and is never duplicated as a part. [`codex_read_skills`] and
//!   [`codex_selected_skills`] stay exported because this parser consumes them and a caller may
//!   want the evidence alone.
//! * **The dedupe window understands ISO-8601 only.** Upstream compares two records' timestamps
//!   with `Date.parse`; this port understands the `YYYY-MM-DDTHH:MM:SS(.fff)(Z|±HH:MM)` shape Codex
//!   writes and otherwise compares the strings for exact equality, which is the same answer the
//!   reference gives when `Date.parse` yields `NaN`.
//! * **Lengths are counted in characters**, as every sibling lane in this module directory does;
//!   upstream counts UTF-16 units. The two agree on ASCII and differ only for astral text in the
//!   tool-output cut and the 200-character skill label.

use std::collections::HashMap;
use std::sync::OnceLock;

use regex::Regex;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use super::types::{
    ReferenceHistoryParser, ReferenceImageRef, ReferenceNativeHistoryKind, ReferencePart,
    ReferenceSkillActivity, ReferenceSkillEvidence, ReferenceSkillStatus, ReferenceTextPhase,
    ReferenceTurn, ReferenceTurnRole,
};

// ---------------------------------------------------------------------------------------
// The dispatcher
// ---------------------------------------------------------------------------------------

/// Parse one native history family's bytes into turns (contract §6).
///
/// `bytes` are the exact bytes the resolver handed over; they are decoded the way Node decodes
/// them (`Buffer.toString("utf8")`, i.e. lossily), so an undecodable tail byte lands inside a line
/// that then fails to parse and is skipped — never a fatal error and never a repaired record.
///
/// [`ReferenceNativeHistoryKind::Unavailable`] is an error, never an empty success: an empty
/// success would be read as "this session has no turns", which is a different answer
/// (`notStarted`) and a different disclosure. A family handed another family's kind refuses for
/// the same reason.
pub fn dispatch_reference_history(
    kind: ReferenceNativeHistoryKind,
    bytes: &[u8],
) -> Result<Vec<ReferenceTurn>, String> {
    let text = String::from_utf8_lossy(bytes);
    match kind {
        ReferenceNativeHistoryKind::Claude => {
            super::history_claude::parse_reference_claude_history(kind, &text)
        }
        ReferenceNativeHistoryKind::Codex => parse_codex_history(kind, &text),
        ReferenceNativeHistoryKind::Omp => super::history_omp::parse_omp_transcript(kind, &text),
        ReferenceNativeHistoryKind::Omo => super::history_omo::parse_omo_history(kind, &text),
        ReferenceNativeHistoryKind::Gjc => super::history_gjc::parse_reference_gjc(kind, &text),
        ReferenceNativeHistoryKind::Pi => super::history_pi::parse_pi_history(kind, &text),
        ReferenceNativeHistoryKind::Unavailable => Err(format!(
            "no native history reader for `{}`; an unsupported source is disclosed, never answered with an empty conversation",
            kind.as_str()
        )),
    }
}

// ---------------------------------------------------------------------------------------
// Codex constants
// ---------------------------------------------------------------------------------------

/// Past this a tool's output is cut in the page; the whole of it is fetched by its ref.
/// `tool-output.ts`: `TOOL_OUTPUT_CHARS`.
pub const REFERENCE_CODEX_TOOL_OUTPUT_CHARS: usize = 4_000;

/// The whole output kept for the goal tools, whose answer *is* their JSON.
/// `tool-output.ts`: `WHOLE_OUTPUT_CHARS`.
pub const REFERENCE_CODEX_WHOLE_OUTPUT_CHARS: usize = 16_000;

/// The tools whose output is kept whole up to [`REFERENCE_CODEX_WHOLE_OUTPUT_CHARS`].
/// `tool-output.ts`: `WHOLE_OUTPUT_TOOLS`.
pub const REFERENCE_CODEX_WHOLE_OUTPUT_TOOLS: [&str; 3] = ["create_goal", "update_goal", "get_goal"];

/// The content-block types whose text a part carries. `codex.ts`: `contentText`.
pub const REFERENCE_CODEX_CONTENT_KINDS: [&str; 4] =
    ["input_text", "output_text", "text", "summary_text"];

/// The reference's own default cap (`codex.ts`: `parseCodexTranscript(text, maxTurns = 100)`).
///
/// The page reader does **not** apply it: the pinned `conversation.ts` parses a page with
/// `parseCodexTranscript(text, Infinity)` and lets the page bound the turns, so applying the
/// default there would silently drop turns between pages. It stays exported for callers that want
/// the reference's default explicitly.
pub const REFERENCE_CODEX_MAX_TURNS: usize = 100;

/// The bytes every line opening a Codex turn contains: a cheap filter before `JSON.parse`
/// (`conversation.ts`: `TURN_MARK["codex-transcript"]`).
pub const REFERENCE_CODEX_TURN_MARK: &str = "\"task_started\"";

/// How many recent text messages upstream keeps to pair an `event_msg` with its `response_item`
/// duplicate (`codex.ts`: `messages.length > 8` shift).
pub const REFERENCE_CODEX_MESSAGE_WINDOW: usize = 8;

/// How many completed-item ids are remembered so a re-read skill event is not re-applied
/// (`codex.ts`: `skillEvents.size > 512`).
pub const REFERENCE_CODEX_SKILL_EVENT_LIMIT: usize = 512;

/// Two records of the same text within this window are one message
/// (`codex.ts`: `Math.abs(Date.parse(a) - Date.parse(b)) <= 1000`).
pub const REFERENCE_CODEX_DEDUPE_WINDOW_MS: i64 = 1_000;

/// The head of a tool output that carries the runner's own verdict
/// (`codex.ts`: `output.slice(0, 2000)`).
pub const REFERENCE_CODEX_FAILURE_HEAD_CHARS: usize = 2_000;

/// A tool call's summary is cut here (`codex.ts`: `summary.slice(0, 120)`).
pub const REFERENCE_CODEX_SUMMARY_CHARS: usize = 120;

/// The largest image a native record may name (`codex-images.ts`: `MAX_IMAGE_BYTES`).
pub const REFERENCE_CODEX_IMAGE_MAX_BYTES: usize = 8 * 1024 * 1024;

/// The longest source value an image ref is minted from.
///
/// `codex-images.ts` refuses `value.length > MAX_IMAGE_BYTES * 4 / 3 + 128`; that bound is
/// `11184838.666…`, and a length is a whole number, so the comparison is `> 11_184_838`.
pub const REFERENCE_CODEX_IMAGE_VALUE_LIMIT: usize = 11_184_838;

/// The media types a Codex image ref may name (`codex-images.ts`: `TYPES`).
pub const REFERENCE_CODEX_IMAGE_TYPES: [(&str, &str); 4] = [
    (".png", "image/png"),
    (".jpg", "image/jpeg"),
    (".jpeg", "image/jpeg"),
    (".gif", "image/gif"),
];

/// The WebP entry of `TYPES`, kept apart because the table above is indexed by extension in the
/// order upstream writes it and `.webp` is the last one there.
pub const REFERENCE_CODEX_IMAGE_WEBP: (&str, &str) = (".webp", "image/webp");

/// The marker opening a memory-citation block (`codex.ts`: `opening`).
pub const REFERENCE_CODEX_MEM_CITATION_OPEN: &str = "<oai-mem-citation>";

/// The marker closing a memory-citation block (`codex.ts`: `closing`).
pub const REFERENCE_CODEX_MEM_CITATION_CLOSE: &str = "</oai-mem-citation>";

// ---------------------------------------------------------------------------------------
// The Codex family
// ---------------------------------------------------------------------------------------

/// The family's entry point in the shape the frozen [`ReferenceHistoryParser`] alias takes.
///
/// A kind that is not [`ReferenceNativeHistoryKind::Codex`] is **refused**, never answered with
/// an empty turn list: an empty success would read as "this session has no turns", which is what
/// `Unavailable` must never claim.
pub fn parse_codex_history(
    kind: ReferenceNativeHistoryKind,
    text: &str,
) -> Result<Vec<ReferenceTurn>, String> {
    if kind != ReferenceNativeHistoryKind::Codex {
        return Err(format!(
            "the codex history family reads codex-transcript records, not {}",
            kind.as_str()
        ));
    }
    Ok(parse_codex_transcript_unbounded(text))
}

/// The frozen alias, satisfied by this family's entry point.
pub const REFERENCE_CODEX_PARSER: ReferenceHistoryParser = parse_codex_history;

/// Compile-time proof that the lane satisfies the frozen signature (contract §6).
const _: ReferenceHistoryParser = parse_codex_history;

/// Upstream `parseCodexTranscript(text, maxTurns = 100)`: the newest [`REFERENCE_CODEX_MAX_TURNS`]
/// turns of a rollout.
pub fn parse_codex_transcript(text: &str) -> Vec<ReferenceTurn> {
    parse_codex_transcript_with_limit(text, REFERENCE_CODEX_MAX_TURNS)
}

/// Upstream `parseCodexTranscript(text, maxTurns)`: the same parse with the caller's tail window.
///
/// `max_turns == 0` keeps every turn, matching the pinned reader's `slice(-0)`.
pub fn parse_codex_transcript_with_limit(text: &str, max_turns: usize) -> Vec<ReferenceTurn> {
    let mut parser = CodexParser::default();
    parser.write(text);
    let turns = parser.snapshot();
    if max_turns == 0 {
        return turns;
    }
    turns[turns.len().saturating_sub(max_turns)..].to_vec()
}

/// The page reader's own call: no cap, because `conversation.ts` parses a page with `Infinity` and
/// the page bounds the turns.
pub fn parse_codex_transcript_unbounded(text: &str) -> Vec<ReferenceTurn> {
    let mut parser = CodexParser::default();
    parser.write(text);
    parser.snapshot()
}

/// Does this line open a Codex turn? Pages start at such lines, so a page never splits a turn
/// (`conversation.ts`: `opensTurn` for `codex-transcript`).
pub fn codex_line_opens_turn(line: &str) -> bool {
    let Ok(entry) = serde_json::from_str::<Value>(line) else {
        return false;
    };
    let Some(entry) = entry.as_object() else {
        return false;
    };
    entry.get("type").and_then(Value::as_str) == Some("event_msg")
        && entry
            .get("payload")
            .and_then(Value::as_object)
            .and_then(|payload| payload.get("type"))
            .and_then(Value::as_str)
            == Some("task_started")
}

/// A tool call's output as Codex recorded it, as text (`codex.ts`: `codexOutputText`).
pub fn codex_output_text(value: Option<&Value>) -> String {
    content_text(value, false)
}

/// Whether a Codex tool output says the call failed (`codex.ts`: `codexCallFailed`).
///
/// Codex records no flag: its command runner writes the exit code into the output, and a script or
/// a patch writes its own verdict. A script that completed is judged as a whole, however its
/// commands went.
pub fn codex_call_failed(output: &str) -> bool {
    let head: String = output.chars().take(REFERENCE_CODEX_FAILURE_HEAD_CHARS).collect();
    if codex_script_completed_pattern().is_match(&head) {
        return false;
    }
    if codex_script_failed_pattern().is_match(&head)
        || codex_apply_patch_verification_pattern().is_match(&head)
    {
        return true;
    }
    let code = codex_exit_code_pattern()
        .captures(&head)
        .or_else(|| codex_json_exit_code_pattern().captures(&head));
    match code {
        Some(found) => found.get(1).map(|digits| digits.as_str() != "0").unwrap_or(false),
        None => false,
    }
}

/// The skills a completed command's own parsed commands read (`skill-activity.ts`:
/// `codexReadSkills`).
///
/// The parser pushes each one beside the call that read it: onto the tool part when one already
/// carries that document, else as its own [`ReferencePart::Skill`] (`codex.ts:236-242`).
pub fn codex_read_skills(value: Option<&Value>) -> Vec<ReferenceSkillActivity> {
    let Some(item) = value.and_then(Value::as_object) else {
        return Vec::new();
    };
    if item.get("type").and_then(Value::as_str) != Some("CommandExecution") {
        return Vec::new();
    }
    let Some(commands) = item.get("parsed_cmd").and_then(Value::as_array) else {
        return Vec::new();
    };
    match item.get("status").and_then(Value::as_str) {
        Some("completed") | Some("failed") => {}
        _ => return Vec::new(),
    }
    let status = match item.get("exit_code") {
        Some(Value::Number(number)) if number.as_i64() == Some(0) => ReferenceSkillStatus::Loaded,
        Some(Value::Number(_)) => ReferenceSkillStatus::Failed,
        _ => return Vec::new(),
    };
    commands
        .iter()
        .flat_map(|command| {
            let command = command.as_object()?;
            if command.get("type").and_then(Value::as_str) != Some("read") {
                return None;
            }
            codex_skill_document(command.get("path"))
        })
        .map(|mut skill| {
            skill.status = status;
            skill
        })
        .collect()
}

/// The skill a `SKILL.md` path names, with no filesystem lookup: the evidence belongs to the bound
/// transcript, including a remote host (`skill-activity.ts`: `skillDocument`).
pub fn codex_skill_document(path: Option<&Value>) -> Option<ReferenceSkillActivity> {
    let path = path?.as_str()?;
    if path.is_empty() || path.chars().count() > 4_096 || path.contains('\r') || path.contains('\n') {
        return None;
    }
    let normalized = path.replace('\\', "/");
    let directory = normalized.strip_suffix("/SKILL.md")?;
    let name = directory.rsplit('/').find(|segment| !segment.is_empty())?;
    let name = codex_label(name)?;
    Some(ReferenceSkillActivity {
        name,
        evidence: ReferenceSkillEvidence::Instructions,
        status: ReferenceSkillStatus::Loaded,
        path: Some(path.to_string()),
    })
}

/// Codex's explicitly selected skill, injected as one complete user-context envelope
/// (`skill-activity.ts`: `selectedSkill`).
pub fn codex_selected_skill(text: &str) -> Option<ReferenceSkillActivity> {
    let found = codex_selected_skill_pattern().captures(text.trim())?;
    let name = codex_label(found.get(1)?.as_str())?;
    let path = Value::String(found.get(2)?.as_str().to_string());
    let document = codex_skill_document(Some(&path))?;
    Some(ReferenceSkillActivity { name, ..document })
}

/// The skills a user message's content blocks name (`codex.ts`: the `selectedSkill` scan over
/// `payload.content`).
///
/// The parser renders each as its own [`ReferencePart::Skill`] on the assistant turn the reference
/// attaches it to, and drops the envelope from the user's own text (`codex.ts:266-276`).
pub fn codex_selected_skills(content: Option<&Value>) -> Vec<ReferenceSkillActivity> {
    codex_blocks(content)
        .iter()
        .filter_map(|block| codex_selected_skill(block.text.unwrap_or("")))
        .collect()
}

/// Older Codex rollouts only record tool calls; a literal, single-file read is recognized and a
/// shell script, search, write, interpolated path or mention stays out (`skill-activity.ts`:
/// `codexReadCall`).
pub fn codex_read_call(name: &str, args: &Map<String, Value>) -> Option<ReferenceSkillActivity> {
    if name == "read_file" || name == "Read" {
        let path = nullish_field(args, "file_path", "path");
        return codex_skill_document(path).map(|mut skill| {
            skill.status = ReferenceSkillStatus::Requested;
            skill
        });
    }
    if !matches!(name, "exec_command" | "shell_command" | "shell") {
        return None;
    }
    let command = nullish_field(args, "cmd", "command")?.as_str()?;
    if codex_shell_meta_pattern().is_match(command) {
        return None;
    }
    let found = codex_shell_read_pattern().captures(command.trim())?;
    let path = found.get(1).or_else(|| found.get(2)).or_else(|| found.get(3))?;
    codex_skill_document(Some(&Value::String(path.as_str().to_string()))).map(|mut skill| {
        skill.status = ReferenceSkillStatus::Requested;
        skill
    })
}

/// Upstream's `a ?? b` over two record fields: the first that is present and not `null`.
fn nullish_field<'a>(
    args: &'a Map<String, Value>,
    first: &str,
    second: &str,
) -> Option<&'a Value> {
    args.get(first)
        .filter(|value| !value.is_null())
        .or_else(|| args.get(second))
}

/// Is this an image reference this reader mints (`codex-images.ts`: `CODEX_IMAGE_REF`)?
pub fn is_codex_image_ref(reference: &str) -> bool {
    codex_image_ref_pattern().is_match(reference)
}

/// The image refs a native user record names, hashed so no path or base64 blob crosses the wire
/// (`codex-images.ts`: `codexImageParts`).
pub fn codex_image_parts(entry: &Map<String, Value>) -> Vec<ReferenceImageRef> {
    let mut seen: Vec<String> = Vec::new();
    codex_image_values(entry)
        .into_iter()
        .filter(|value| {
            if seen.iter().any(|held| held == value) {
                return false;
            }
            seen.push(value.clone());
            true
        })
        .filter_map(|value| codex_image_part(&value))
        .collect()
}

/// The file edits an edit call carries, or `None` when the input is not a patch
/// (`patch.ts`: `patchText`).
pub fn patch_text(input: &str) -> Option<String> {
    const BEGIN: &str = "*** Begin Patch";
    let trimmed = input.trim_start();
    if trimmed.starts_with(BEGIN) {
        return Some(trimmed.to_string());
    }
    let found = codex_apply_patch_pattern().captures(input)?;
    let literal = found.get(1)?.as_str();
    let text = match literal.strip_prefix('`') {
        Some(inner) => inner.strip_suffix('`')?.replace("\\`", "`"),
        None => serde_json::from_str::<String>(literal).ok()?,
    };
    let text = text.trim_start();
    text.starts_with(BEGIN).then(|| text.to_string())
}

/// The files a patch adds, updates or deletes, in the order it names them (`patch.ts`:
/// `patchFiles`).
pub fn patch_files(patch: &str) -> Vec<String> {
    let mut files: Vec<String> = Vec::new();
    for found in codex_patch_file_pattern().captures_iter(patch) {
        let Some(file) = found.get(1) else {
            continue;
        };
        let file = file.as_str().trim();
        if !files.iter().any(|held| held == file) {
            files.push(file.to_string());
        }
    }
    files
}

// ---------------------------------------------------------------------------------------
// The Codex parser
// ---------------------------------------------------------------------------------------

/// A text message held long enough to pair an `event_msg` with the `response_item` that repeats it
/// (`codex.ts`: `CodexParseState.messages`).
struct CodexMessage {
    role: ReferenceTurnRole,
    text: String,
    /// `"event"` for an `event_msg`, `"response"` for a `response_item`. Two records with the same
    /// text from the same source are not a pair.
    source: &'static str,
    ts: Option<String>,
    paired: bool,
    /// Where the part lives, so the earlier of a pair can be updated in place.
    turn: usize,
    part: usize,
}

/// Upstream `createCodexTranscriptParser`'s state, folded into one parser.
#[derive(Default)]
struct CodexParser {
    turns: Vec<ReferenceTurn>,
    /// A tool call's part, by `call_id`, until its output arrives.
    tools: HashMap<String, (usize, usize)>,
    messages: Vec<CodexMessage>,
    /// When the running task started, so the assistant turn it opens is dated by the task and not
    /// by the first record that happened to arrive.
    started_at: Option<String>,
    /// Completed-item ids whose skills were already applied, oldest first (bounded).
    skill_events: Vec<String>,
    active_turn_id: Option<String>,
}

impl CodexParser {
    /// The assistant turn a record belongs to: the last one while the task is running, else a new
    /// one. Returns its index in `turns`.
    fn assistant(&mut self, ts: Option<&str>) -> usize {
        let merge = matches!(
            self.turns.last(),
            Some(turn) if turn.role == ReferenceTurnRole::Assistant
        );
        let at = if merge {
            self.turns.len() - 1
        } else {
            let started_at = self.started_at.clone().or_else(|| ts.map(str::to_string));
            self.turns.push(ReferenceTurn {
                role: ReferenceTurnRole::Assistant,
                started_at,
                ended_at: None,
                source: None,
                parts: Vec::new(),
                abandoned: None,
            });
            self.turns.len() - 1
        };
        if let Some(ts) = ts {
            self.turns[at].ended_at = Some(ts.to_string());
        }
        at
    }

    /// Upstream's `message`: shape the body, drop it when there is nothing to show, pair it with a
    /// duplicate the other record already contributed, else open or extend a turn.
    fn message(
        &mut self,
        role: ReferenceTurnRole,
        raw: &str,
        source: &'static str,
        ts: Option<&str>,
        phase: Option<ReferenceTextPhase>,
        images: Vec<ReferenceImageRef>,
    ) {
        let body = match role {
            ReferenceTurnRole::User => question_reply(raw).unwrap_or_else(|| raw.to_string()),
            ReferenceTurnRole::Assistant => without_memory_citations(raw),
        };
        if body.trim().is_empty() && images.is_empty() {
            return;
        }
        let duplicate = self.messages.iter().rposition(|other| {
            !other.paired
                && other.role == role
                && other.text == body
                && other.source != source
                && timestamps_match(other.ts.as_deref(), ts)
        });
        if let Some(at) = duplicate {
            self.messages[at].paired = true;
            let turn = self.messages[at].turn;
            let part = self.messages[at].part;
            if let Some(phase) = phase {
                if let Some(ReferencePart::Text { phase: slot, .. }) =
                    self.turns[turn].parts.get_mut(part)
                {
                    *slot = Some(phase);
                }
            }
            if !images.is_empty()
                && (source == "event"
                    || !self.turns[turn]
                        .parts
                        .iter()
                        .any(|part| matches!(part, ReferencePart::Image { .. })))
            {
                // Prefer the event's local paths over a response's inline copies of the same
                // images.
                let mut parts: Vec<ReferencePart> = images.into_iter().map(image_part).collect();
                parts.extend(
                    self.turns[turn]
                        .parts
                        .drain(..)
                        .filter(|part| !matches!(part, ReferencePart::Image { .. })),
                );
                self.turns[turn].parts = parts;
                // The text part is the last of a user turn, before and after the rebuild.
                self.messages[at].part = self.turns[turn].parts.len() - 1;
            }
            return;
        }
        let turn = match role {
            ReferenceTurnRole::User => {
                let mut parts: Vec<ReferencePart> = images.into_iter().map(image_part).collect();
                if !body.trim().is_empty() {
                    parts.push(ReferencePart::Text { text: body.clone(), phase });
                }
                self.turns.push(ReferenceTurn {
                    role,
                    started_at: ts.map(str::to_string),
                    ended_at: None,
                    source: None,
                    parts,
                    abandoned: None,
                });
                self.turns.len() - 1
            }
            ReferenceTurnRole::Assistant => {
                let at = self.assistant(ts);
                self.turns[at].parts.push(ReferencePart::Text { text: body.clone(), phase });
                at
            }
        };
        let part = self.turns[turn].parts.len() - 1;
        self.messages.push(CodexMessage {
            role,
            text: body,
            source,
            ts: ts.map(str::to_string),
            paired: false,
            turn,
            part,
        });
        if self.messages.len() > REFERENCE_CODEX_MESSAGE_WINDOW {
            self.messages.remove(0);
        }
    }

    /// Feed a rollout, line by line. An unreadable line — a torn tail while Codex appends, a
    /// non-JSON line, a JSON scalar — is skipped; the rest of the file still reads.
    fn write(&mut self, text: &str) {
        for line in text.split('\n') {
            let Ok(entry) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            let Some(entry) = entry.as_object() else {
                continue;
            };
            let ts = entry
                .get("timestamp")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty());
            match entry.get("type").and_then(Value::as_str).unwrap_or("") {
                "event_msg" => self.write_event(entry, ts),
                "response_item" => self.write_response(entry, ts),
                _ => {}
            }
        }
    }

    /// The turns a page carries: every turn that has a part, in order
    /// (`codex.ts`: `turns.filter((turn) => turn.parts.length > 0)`).
    fn snapshot(&self) -> Vec<ReferenceTurn> {
        self.turns
            .iter()
            .filter(|turn| !turn.parts.is_empty())
            .cloned()
            .collect()
    }

    /// A display event: the turn boundary, the user's message, the agent's answer, and the skill
    /// reads a completed command reports.
    fn write_event(&mut self, entry: &Map<String, Value>, ts: Option<&str>) {
        let Some(payload) = entry.get("payload").and_then(Value::as_object) else {
            return;
        };
        match payload.get("type").and_then(Value::as_str).unwrap_or("") {
            "item_completed" => {
                let turn_id = payload
                    .get("turn_id")
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty());
                let current = self.active_turn_id.as_deref();
                if current.is_some() && turn_id.is_some() && turn_id != current {
                    return;
                }
                let item = payload.get("item");
                let id = item
                    .and_then(|item| item.get("id"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let skills = codex_read_skills(item);
                if id.is_empty()
                    || skills.is_empty()
                    || self.skill_events.iter().any(|held| held == id)
                {
                    return;
                }
                self.skill_events.push(id.to_string());
                if self.skill_events.len() > REFERENCE_CODEX_SKILL_EVENT_LIMIT {
                    self.skill_events.remove(0);
                }
                for skill in skills {
                    let at = self.assistant(ts);
                    let existing = self.turns[at].parts.iter().rposition(|part| match part {
                        ReferencePart::Tool { skill: Some(held), .. } => held.path == skill.path,
                        ReferencePart::Tool { skill: None, .. } => skill.path.is_none(),
                        _ => false,
                    });
                    // A skill that already rides a tool call is updated there; one with no call to
                    // ride becomes its own part (`codex.ts:236-242`).
                    match existing {
                        Some(index) => {
                            if let ReferencePart::Tool { skill: Some(held), .. } =
                                &mut self.turns[at].parts[index]
                            {
                                held.status = skill.status;
                            }
                        }
                        None => self.turns[at].parts.push(ReferencePart::Skill { skill }),
                    }
                }
            }
            "task_started" => {
                let started = payload
                    .get("started_at")
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty());
                self.started_at = started.or(ts).map(str::to_string);
                self.active_turn_id = payload
                    .get("turn_id")
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string);
            }
            "task_complete" | "turn_aborted" => {
                if let Some(last) = self.turns.last_mut() {
                    if last.role == ReferenceTurnRole::Assistant {
                        if let Some(ts) = ts {
                            last.ended_at = Some(ts.to_string());
                        }
                    }
                }
                self.started_at = None;
            }
            "user_message" => {
                let plain = payload
                    .get("kind")
                    .and_then(Value::as_str)
                    .map_or(true, |kind| kind == "plain");
                if !plain {
                    return;
                }
                let text = content_text(payload.get("message"), true);
                let images = codex_image_parts(entry);
                self.message(ReferenceTurnRole::User, &text, "event", ts, None, images);
            }
            "agent_message" => {
                let text = content_text(payload.get("message"), false);
                self.message(
                    ReferenceTurnRole::Assistant,
                    &text,
                    "event",
                    ts,
                    phase_of(payload),
                    Vec::new(),
                );
            }
            _ => {}
        }
    }

    /// A model record: a message, reasoning, a tool call, or the output that answers one.
    fn write_response(&mut self, entry: &Map<String, Value>, ts: Option<&str>) {
        let Some(payload) = entry.get("payload").and_then(Value::as_object) else {
            return;
        };
        match payload.get("type").and_then(Value::as_str).unwrap_or("") {
            "message" => match payload.get("role").and_then(Value::as_str).unwrap_or("") {
                "user" => {
                    let blocks = codex_blocks(payload.get("content"));
                    let visible: Vec<CodexBlock> = blocks
                        .iter()
                        .copied()
                        .filter(|block| codex_selected_skill(block.text.unwrap_or("")).is_none())
                        .collect();
                    let text = codex_block_text(&visible, true);
                    let images = codex_image_parts(entry);
                    self.message(ReferenceTurnRole::User, &text, "response", ts, None, images);
                    // Codex injects an explicitly selected skill as a user-context envelope: the
                    // request under it is the user's text, and the skill is its own part on the
                    // assistant turn the reference attaches it to (`codex.ts:266-276`).
                    for skill in codex_selected_skills(payload.get("content")) {
                        let at = self.assistant(ts);
                        let held = self.turns[at].parts.iter().any(|part| {
                            matches!(part, ReferencePart::Skill { skill: existing }
                                if existing.name == skill.name && existing.path == skill.path)
                        });
                        if !held {
                            self.turns[at].parts.push(ReferencePart::Skill { skill });
                        }
                    }
                }
                "assistant" => {
                    let body = content_text(payload.get("content"), false);
                    if payload.get("channel").and_then(Value::as_str) == Some("analysis") {
                        if !body.trim().is_empty() {
                            let at = self.assistant(ts);
                            self.turns[at].parts.push(ReferencePart::Thinking { text: body });
                        }
                    } else {
                        let recipient = payload
                            .get("recipient")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        if recipient.is_empty() || recipient == "all" {
                            self.message(
                                ReferenceTurnRole::Assistant,
                                &body,
                                "response",
                                ts,
                                phase_of(payload),
                                Vec::new(),
                            );
                        }
                    }
                }
                _ => {}
            },
            "reasoning" => {
                let body = content_text(payload.get("summary"), false);
                if !body.trim().is_empty() {
                    let at = self.assistant(ts);
                    self.turns[at].parts.push(ReferencePart::Thinking { text: body });
                }
            }
            "function_call" | "custom_tool_call" => {
                let kind = payload.get("type").and_then(Value::as_str).unwrap_or("");
                let raw = payload.get(if kind == "function_call" {
                    "arguments"
                } else {
                    "input"
                });
                let args = match raw {
                    Some(Value::Object(map)) => map.clone(),
                    Some(Value::String(text)) => match serde_json::from_str::<Value>(text) {
                        Ok(Value::Object(map)) => map,
                        _ => Map::new(),
                    },
                    _ => Map::new(),
                };
                let name = payload
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .unwrap_or("tool");
                // A patch, bare or inside an exec script, is summed up by the files it touches;
                // Codex's own queued questions by the questions they ask.
                let patch = raw.and_then(Value::as_str).and_then(patch_text);
                let files = patch.as_deref().map(patch_files).unwrap_or_default();
                let titles = question_titles(&args);
                let summary = if !files.is_empty() {
                    files.join(", ")
                } else if name.starts_with("request_user_input") && !titles.is_empty() {
                    titles.join(" · ")
                } else {
                    ["cmd", "command", "file_path", "path", "pattern", "description", "url"]
                        .iter()
                        .find_map(|key| args.get(*key).and_then(Value::as_str))
                        .unwrap_or("")
                        .to_string()
                };
                let summary = if summary.is_empty() { name.to_string() } else { summary };
                let summary: String = summary.chars().take(REFERENCE_CODEX_SUMMARY_CHARS).collect();
                let input = if args.is_empty() {
                    raw.and_then(Value::as_str).unwrap_or("").to_string()
                } else {
                    serde_json::to_string_pretty(&Value::Object(args.clone()))
                        .unwrap_or_else(|_| "{}".to_string())
                };
                let skill = codex_read_call(name, &args);
                let at = self.assistant(ts);
                let part = self.turns[at].parts.len();
                self.turns[at].parts.push(ReferencePart::Tool {
                    name: name.to_string(),
                    summary,
                    input,
                    output: String::new(),
                    error: None,
                    skill,
                    output_ref: None,
                    output_size: None,
                    images: Vec::new(),
                });
                if let Some(call_id) = payload.get("call_id").and_then(Value::as_str) {
                    self.tools.insert(call_id.to_string(), (at, part));
                }
            }
            "function_call_output" | "custom_tool_call_output" => {
                let call_id = payload
                    .get("call_id")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                // A result for a call this page never saw is ignored, never a row of its own.
                let Some((turn, part)) = self.tools.remove(call_id) else {
                    return;
                };
                let output = content_text(payload.get("output"), false);
                let failed = codex_call_failed(&output);
                let Some(ReferencePart::Tool {
                    name,
                    output: slot,
                    error,
                    skill,
                    output_ref,
                    output_size,
                    ..
                }) = self.turns[turn].parts.get_mut(part)
                else {
                    return;
                };
                let limit = if REFERENCE_CODEX_WHOLE_OUTPUT_TOOLS.contains(&name.as_str()) {
                    REFERENCE_CODEX_WHOLE_OUTPUT_CHARS
                } else {
                    REFERENCE_CODEX_TOOL_OUTPUT_CHARS
                };
                let length = output.chars().count();
                if length <= limit {
                    *slot = output;
                } else {
                    *slot = format!("{}\n… trimmed", output.chars().take(limit).collect::<String>());
                    *output_ref = Some(call_id.to_string());
                    *output_size = Some(length as u64);
                }
                if failed {
                    *error = Some(true);
                }
                if let Some(skill) = skill {
                    skill.status = if failed {
                        ReferenceSkillStatus::Failed
                    } else {
                        ReferenceSkillStatus::Loaded
                    };
                }
                if let Some(last) = self.turns.last_mut() {
                    if last.role == ReferenceTurnRole::Assistant {
                        if let Some(ts) = ts {
                            last.ended_at = Some(ts.to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// One content block of a Codex message, as `contentText` and `selectedSkill` read it.
#[derive(Debug, Clone, Copy)]
struct CodexBlock<'a> {
    kind: &'a str,
    text: Option<&'a str>,
}

impl<'a> CodexBlock<'a> {
    fn of(value: &'a Value) -> Self {
        let object = value.as_object();
        Self {
            kind: object
                .and_then(|object| object.get("type"))
                .and_then(Value::as_str)
                .unwrap_or(""),
            text: object
                .and_then(|object| object.get("text"))
                .and_then(Value::as_str),
        }
    }
}

/// The blocks a message's content holds. A content that is not an array is one `input_text` block,
/// exactly as upstream wraps it (`codex.ts`: `Array.isArray(payload.content) ? … : [{ type:
/// "input_text", text: payload.content }]`).
fn codex_blocks(content: Option<&Value>) -> Vec<CodexBlock<'_>> {
    match content {
        Some(Value::Array(items)) => items.iter().map(CodexBlock::of).collect(),
        Some(Value::String(text)) => vec![CodexBlock { kind: "input_text", text: Some(text.as_str()) }],
        _ => Vec::new(),
    }
}

/// Upstream `contentText` over an already-split block list.
fn codex_block_text(blocks: &[CodexBlock<'_>], user: bool) -> String {
    blocks
        .iter()
        .filter(|block| REFERENCE_CODEX_CONTENT_KINDS.contains(&block.kind))
        .filter_map(|block| block.text)
        .filter(|text| !(user && context_only(text)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Upstream `contentText`: a string as it is, else the text of its content blocks joined by
/// newlines. In the user's seat, runtime context is not the user's own words.
fn content_text(value: Option<&Value>, user: bool) -> String {
    match value {
        Some(Value::String(text)) => {
            if user && context_only(text) {
                String::new()
            } else {
                text.clone()
            }
        }
        Some(Value::Array(items)) => {
            let blocks: Vec<CodexBlock> = items.iter().map(CodexBlock::of).collect();
            codex_block_text(&blocks, user)
        }
        _ => String::new(),
    }
}

/// Runtime context that reaches the model in the user's seat but that nobody typed
/// (`codex.ts`: `contextOnly`).
fn context_only(text: &str) -> bool {
    let value = text.trim();
    if value.starts_with("# AGENTS.md instructions for ") && value.contains("</INSTRUCTIONS>") {
        return true;
    }
    for tag in [
        "environment_context",
        "permissions instructions",
        "turn_aborted",
        "subagent_notification",
    ] {
        let opening = format!("<{tag}>");
        let closing = format!("</{tag}>");
        if value.starts_with(&opening)
            && value.ends_with(&closing)
            && value.len() >= opening.len() + closing.len()
        {
            return true;
        }
    }
    false
}

/// An answer to Codex's queued questions reaches the model as a user message; the chat shows what
/// was answered, not the envelope (`codex.ts`: `questionReply`).
fn question_reply(text: &str) -> Option<String> {
    let found = codex_question_reply_pattern().captures(text.trim())?;
    let items: Vec<Value> = serde_json::from_str(found.get(1)?.as_str()).ok()?;
    let answers: Vec<String> = items
        .iter()
        .map(|item| {
            item.as_object()
                .and_then(|item| item.get("answer"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string()
        })
        .filter(|answer| !answer.is_empty())
        .collect();
    (!answers.is_empty()).then(|| answers.join("\n"))
}

/// The questions of a `request_user_input(_async)` call: `title` (async) or `question` (plan mode)
/// (`codex.ts`: `questionTitles`).
fn question_titles(args: &Map<String, Value>) -> Vec<String> {
    let Some(items) = args.get("questions").and_then(Value::as_array) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let item = item.as_object();
            let title = item
                .and_then(|item| item.get("title"))
                .and_then(Value::as_str)
                .unwrap_or("");
            if !title.is_empty() {
                return Some(title.to_string());
            }
            item.and_then(|item| item.get("question"))
                .and_then(Value::as_str)
                .filter(|question| !question.is_empty())
                .map(str::to_string)
        })
        .collect()
}

/// Upstream `label` (`skill-activity.ts`): a skill name that is short, single-line and not a
/// bracket of prose.
fn codex_label(value: &str) -> Option<String> {
    let usable = !value.is_empty()
        && value.chars().count() <= 200
        && !value.chars().any(|character| matches!(character, '\r' | '\n' | '<' | '>'));
    usable.then(|| value.to_string())
}

/// The phase a message states, when it states one.
fn phase_of(payload: &Map<String, Value>) -> Option<ReferenceTextPhase> {
    match payload.get("phase").and_then(Value::as_str) {
        Some("commentary") => Some(ReferenceTextPhase::Commentary),
        Some("final_answer") => Some(ReferenceTextPhase::FinalAnswer),
        _ => None,
    }
}

/// The image values a native user record names (`codex-images.ts`: `imageValues`).
fn codex_image_values(entry: &Map<String, Value>) -> Vec<String> {
    let payload = entry.get("payload").and_then(Value::as_object);
    let kind = entry.get("type").and_then(Value::as_str).unwrap_or("");
    if kind == "event_msg" {
        let Some(payload) = payload else {
            return Vec::new();
        };
        let message = payload.get("type").and_then(Value::as_str) == Some("user_message");
        let plain = payload
            .get("kind")
            .and_then(Value::as_str)
            .map_or(true, |kind| kind == "plain");
        if !message || !plain {
            return Vec::new();
        }
        let mut values: Vec<String> = Vec::new();
        for key in ["local_images", "images"] {
            if let Some(items) = payload.get(key).and_then(Value::as_array) {
                values.extend(
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string),
                );
            }
        }
        return values;
    }
    if kind != "response_item" {
        return Vec::new();
    }
    let Some(payload) = payload else {
        return Vec::new();
    };
    if payload.get("type").and_then(Value::as_str) != Some("message")
        || payload.get("role").and_then(Value::as_str) != Some("user")
    {
        return Vec::new();
    }
    let Some(items) = payload.get("content").and_then(Value::as_array) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|block| {
            let block = block.as_object()?;
            match block.get("type").and_then(Value::as_str) {
                Some("input_image") => block.get("image_url").and_then(Value::as_str),
                Some("local_image") => block.get("path").and_then(Value::as_str),
                _ => None,
            }
        })
        .map(str::to_string)
        .collect()
}

/// One image ref, or `None` for a value that names no image this reader can address
/// (`codex-images.ts`: `imagePart`).
fn codex_image_part(value: &str) -> Option<ReferenceImageRef> {
    if value.is_empty()
        || value.contains('\0')
        || value.chars().count() > REFERENCE_CODEX_IMAGE_VALUE_LIMIT
    {
        return None;
    }
    let inline = codex_data_image_pattern().captures(value);
    if inline.is_none() && codex_scheme_pattern().is_match(value) {
        return None;
    }
    let media_type = match inline {
        Some(found) => found.get(1)?.as_str().to_string(),
        None => codex_image_type(value)?,
    };
    Some(ReferenceImageRef {
        media_type,
        r#ref: format!("codex-{:x}", Sha256::digest(value.as_bytes())),
    })
}

/// The media type a file name names, or `None` for an extension no chat shows
/// (`codex-images.ts`: `TYPES[extname(value).toLowerCase()]`).
fn codex_image_type(value: &str) -> Option<String> {
    let name = value.rsplit('/').next().unwrap_or("");
    let dot = name.rfind('.')?;
    if dot == 0 {
        return None;
    }
    let extension = name[dot..].to_ascii_lowercase();
    if extension == REFERENCE_CODEX_IMAGE_WEBP.0 {
        return Some(REFERENCE_CODEX_IMAGE_WEBP.1.to_string());
    }
    REFERENCE_CODEX_IMAGE_TYPES
        .iter()
        .find(|(suffix, _)| *suffix == extension)
        .map(|(_, media_type)| media_type.to_string())
}

/// One part of a turn, from a ref the parser minted.
fn image_part(image: ReferenceImageRef) -> ReferencePart {
    ReferencePart::Image {
        media_type: image.media_type,
        r#ref: image.r#ref,
    }
}

/// Upstream `withoutMemoryCitations` (`codex.ts:74-172`): the `<oai-mem-citation>` blocks Codex
/// appends to an answer are metadata, and they survive only where the answer itself quotes them as
/// code — inside a fenced block, an indented block, or a code span.
fn without_memory_citations(text: &str) -> String {
    if !text.contains(REFERENCE_CODEX_MEM_CITATION_OPEN) {
        return text.trim_end().to_string();
    }

    let mut tokens: Vec<CodexToken> = Vec::new();
    for (start, source) in codex_lines(text) {
        let mut offset = 0usize;
        let mut quote_depth = 0usize;
        loop {
            let bytes = source.as_bytes();
            let mut at = offset;
            while at < bytes.len() && (bytes[at] == b' ' || bytes[at] == b'\t') {
                at += 1;
            }
            if at >= bytes.len() || bytes[at] != b'>' {
                break;
            }
            at += 1;
            if at < bytes.len() && (bytes[at] == b' ' || bytes[at] == b'\t') {
                at += 1;
            }
            offset = at;
            quote_depth += 1;
        }
        let rest = &source[offset..];
        let indent = rest
            .bytes()
            .take_while(|byte| *byte == b' ' || *byte == b'\t')
            .count();
        let content = &rest[indent..];
        let list_len = codex_list_marker_len(content);
        let line = CodexLine {
            index: start,
            quote_depth,
            indent,
            list_indent: list_len.map(|len| indent + len),
            blank: content.trim().is_empty(),
        };
        tokens.push(CodexToken::Line(line));
        let body = &content[list_len.unwrap_or(0)..];
        let marker_len = codex_fence_marker_len(body);
        let marker = &body[..marker_len];
        let info = &body[marker_len..];
        // A backtick fence's info string cannot contain backticks.
        if marker_len > 0 && (marker.as_bytes()[0] != b'`' || !info.contains('`')) {
            tokens.push(CodexToken::Fence {
                index: start + offset + indent + list_len.unwrap_or(0),
                marker: marker.to_string(),
                info: info.to_string(),
                line,
            });
        }
        for (index, found) in codex_delimiters(source) {
            if found == REFERENCE_CODEX_MEM_CITATION_OPEN {
                tokens.push(CodexToken::Citation { index: start + index });
            } else {
                tokens.push(CodexToken::Ticks {
                    index: start + index,
                    size: found.len(),
                });
            }
        }
    }

    // Index matching runs once: an unmatched delimiter must not repeatedly scan the tail.
    let mut closes: HashMap<usize, usize> = HashMap::new();
    let mut pairs: Vec<Option<i64>> = vec![None; tokens.len()];
    for index in (0..tokens.len()).rev() {
        match &tokens[index] {
            CodexToken::Ticks { index: at, size } => {
                let bytes = text.as_bytes();
                let mut backslashes = 0usize;
                let mut cursor = *at;
                while cursor > 0 && bytes[cursor - 1] == b'\\' {
                    backslashes += 1;
                    cursor -= 1;
                }
                let length = size.saturating_sub(backslashes % 2);
                pairs[index] = Some(match closes.get(&length) {
                    Some(found) => *found as i64,
                    None => -1,
                });
                closes.insert(*size, index);
            }
            CodexToken::Line(_) => closes.clear(),
            _ => {}
        }
    }

    let mut parts: Vec<&str> = Vec::new();
    let mut list_indents: Vec<usize> = Vec::new();
    let mut quote_depth = 0usize;
    let mut kept = 0usize;
    let mut index = 0usize;
    while index < tokens.len() {
        match &tokens[index] {
            CodexToken::Line(line) => {
                if line.quote_depth != quote_depth {
                    list_indents.clear();
                    quote_depth = line.quote_depth;
                }
                if line.blank {
                    index += 1;
                    continue;
                }
                while list_indents.last().is_some_and(|held| *held > line.indent) {
                    list_indents.pop();
                }
                if let Some(list_indent) = line.list_indent {
                    list_indents.push(list_indent);
                }
                index += 1;
            }
            CodexToken::Fence { marker, line, .. } => {
                let within = line
                    .list_indent
                    .or_else(|| list_indents.last().copied())
                    .unwrap_or(0);
                let indent = if line.list_indent.is_none() {
                    line.indent as isize - within as isize
                } else {
                    0
                };
                if !(0..=3).contains(&indent) {
                    index += 1;
                    continue;
                }
                // An unfinished fence remains code until its quote or list container ends.
                let mut cursor = index + 1;
                while cursor < tokens.len() {
                    let end = &tokens[cursor];
                    if let CodexToken::Line(end_line) = end {
                        let outside = end_line.quote_depth < line.quote_depth
                            || (within > 0
                                && end_line.quote_depth == line.quote_depth
                                && !end_line.blank
                                && end_line.indent < within);
                        if outside {
                            cursor -= 1;
                            break;
                        }
                    }
                    if let CodexToken::Fence {
                        marker: end_marker,
                        info: end_info,
                        line: end_line,
                        ..
                    } = end
                    {
                        let closing = end_line.quote_depth == line.quote_depth
                            && end_marker.as_bytes()[0] == marker.as_bytes()[0]
                            && end_marker.len() >= marker.len()
                            && end_info.trim().is_empty();
                        if closing {
                            // Skip the closing line's raw backtick and citation tokens too.
                            while cursor + 1 < tokens.len()
                                && !matches!(tokens[cursor + 1], CodexToken::Line(_))
                            {
                                cursor += 1;
                            }
                            break;
                        }
                    }
                    cursor += 1;
                }
                index = cursor + 1;
            }
            CodexToken::Ticks { .. } => match pairs[index] {
                Some(pair) if pair >= 0 => index = pair as usize + 1,
                _ => index += 1,
            },
            CodexToken::Citation { index: at } => {
                parts.push(&text[kept..*at]);
                let close = text[*at + REFERENCE_CODEX_MEM_CITATION_OPEN.len()..]
                    .find(REFERENCE_CODEX_MEM_CITATION_CLOSE)
                    .map(|found| *at + REFERENCE_CODEX_MEM_CITATION_OPEN.len() + found);
                kept = match close {
                    Some(found) => found + REFERENCE_CODEX_MEM_CITATION_CLOSE.len(),
                    None => text.len(),
                };
                index += 1;
                // Metadata contents cannot open code spans or fences in the surrounding answer.
                while index < tokens.len() && codex_token_index(&tokens[index]) < kept {
                    index += 1;
                }
            }
        }
    }
    parts.push(&text[kept..]);
    parts.concat().trim_end().to_string()
}

/// One line of a memory-citation scan, with what the fence and list rules read from it.
#[derive(Debug, Clone, Copy)]
struct CodexLine {
    index: usize,
    quote_depth: usize,
    indent: usize,
    list_indent: Option<usize>,
    blank: bool,
}

/// One delimiter or container a memory-citation scan walks.
#[derive(Debug, Clone)]
enum CodexToken {
    Line(CodexLine),
    Fence {
        index: usize,
        marker: String,
        info: String,
        line: CodexLine,
    },
    Ticks {
        index: usize,
        size: usize,
    },
    Citation {
        index: usize,
    },
}

/// Where a token starts in the answer.
fn codex_token_index(token: &CodexToken) -> usize {
    match token {
        CodexToken::Line(line) => line.index,
        CodexToken::Fence { index, .. }
        | CodexToken::Ticks { index, .. }
        | CodexToken::Citation { index } => *index,
    }
}

/// The lines of an answer, each with where it starts, exactly as the pinned scan walks them: a
/// newline ends a line, and a final empty line always follows the last one.
fn codex_lines(text: &str) -> Vec<(usize, &str)> {
    let bytes = text.as_bytes();
    let mut lines: Vec<(usize, &str)> = Vec::new();
    let mut start = 0usize;
    let mut at = 0usize;
    while at < bytes.len() {
        match bytes[at] {
            b'\n' => {
                lines.push((start, &text[start..at]));
                at += 1;
                start = at;
            }
            b'\r' => {
                lines.push((start, &text[start..at]));
                at += 1;
                if at < bytes.len() && bytes[at] == b'\n' {
                    at += 1;
                }
                start = at;
            }
            _ => at += 1,
        }
    }
    if start < bytes.len() {
        lines.push((start, &text[start..]));
    }
    lines.push((text.len(), ""));
    lines
}

/// The backtick runs and citation markers of one line, in order (`codex.ts`: the
/// `` /`+|<oai-mem-citation>/g `` scan).
fn codex_delimiters(source: &str) -> Vec<(usize, &str)> {
    let mut found: Vec<(usize, &str)> = Vec::new();
    let mut characters = source.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        if character == '`' {
            let mut end = index + 1;
            while let Some(&(next, '`')) = characters.peek() {
                characters.next();
                end = next + 1;
            }
            found.push((index, &source[index..end]));
        } else if character == '<' && source[index..].starts_with(REFERENCE_CODEX_MEM_CITATION_OPEN) {
            found.push((index, REFERENCE_CODEX_MEM_CITATION_OPEN));
            for _ in 1..REFERENCE_CODEX_MEM_CITATION_OPEN.len() {
                characters.next();
            }
        }
    }
    found
}

/// The length of a list marker at the start of a line, when it has one
/// (`codex.ts`: `^(?:[-+*]|\d+[.)])[ \t]+`).
fn codex_list_marker_len(content: &str) -> Option<usize> {
    let bytes = content.as_bytes();
    let mut at = 0usize;
    match bytes.first()? {
        b'-' | b'+' | b'*' => at = 1,
        byte if byte.is_ascii_digit() => {
            while at < bytes.len() && bytes[at].is_ascii_digit() {
                at += 1;
            }
            if at < bytes.len() && (bytes[at] == b'.' || bytes[at] == b')') {
                at += 1;
            } else {
                return None;
            }
        }
        _ => return None,
    }
    let spaces = bytes[at..]
        .iter()
        .take_while(|byte| **byte == b' ' || **byte == b'\t')
        .count();
    (spaces > 0).then_some(at + spaces)
}

/// The length of a code-fence marker at the start of a line body
/// (`codex.ts`: `` /^(`{3,}|~{3,})/ ``).
fn codex_fence_marker_len(body: &str) -> usize {
    let bytes = body.as_bytes();
    let Some(first) = bytes.first() else {
        return 0;
    };
    if *first != b'`' && *first != b'~' {
        return 0;
    }
    let count = bytes.iter().take_while(|byte| **byte == *first).count();
    if count >= 3 {
        count
    } else {
        0
    }
}

/// The milliseconds since the epoch an ISO-8601 timestamp names, or `None` for anything else.
///
/// Only the shape Codex writes is understood (`YYYY-MM-DDTHH:MM:SS(.fff)(Z|±HH:MM)`); a caller
/// comparing two timestamps falls back to exact string equality, which is what the reference does
/// when `Date.parse` yields `NaN`.
fn codex_timestamp_millis(ts: &str) -> Option<i64> {
    let bytes = ts.as_bytes();
    let digits = |from: usize, count: usize| -> Option<i64> {
        let slice = bytes.get(from..from + count)?;
        if !slice.iter().all(u8::is_ascii_digit) {
            return None;
        }
        std::str::from_utf8(slice).ok()?.parse::<i64>().ok()
    };
    let year = digits(0, 4)?;
    if bytes.get(4) != Some(&b'-') {
        return None;
    }
    let month = digits(5, 2)?;
    if bytes.get(7) != Some(&b'-') {
        return None;
    }
    let day = digits(8, 2)?;
    if !matches!(bytes.get(10).copied(), Some(b'T') | Some(b't') | Some(b' ')) {
        return None;
    }
    let hour = digits(11, 2)?;
    if bytes.get(13) != Some(&b':') {
        return None;
    }
    let minute = digits(14, 2)?;
    if bytes.get(16) != Some(&b':') {
        return None;
    }
    let second = digits(17, 2)?;
    let mut at = 19usize;
    let mut millis = 0i64;
    if bytes.get(at) == Some(&b'.') {
        at += 1;
        let mut fraction = String::new();
        while let Some(digit) = bytes.get(at).filter(|byte| byte.is_ascii_digit()) {
            fraction.push(*digit as char);
            at += 1;
        }
        if fraction.is_empty() {
            return None;
        }
        let mut padded = fraction.clone();
        while padded.len() < 3 {
            padded.push('0');
        }
        millis = padded[..3].parse::<i64>().ok()?;
    }
    let offset_minutes = match bytes.get(at).copied() {
        Some(b'Z') | Some(b'z') => 0,
        Some(sign) if sign == b'+' || sign == b'-' => {
            let sign = if sign == b'-' { -1 } else { 1 };
            let hours = digits(at + 1, 2)?;
            if bytes.get(at + 3) != Some(&b':') {
                return None;
            }
            let minutes = digits(at + 4, 2)?;
            sign * (hours * 60 + minutes)
        }
        _ => return None,
    };
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    if !(0..24).contains(&hour) || !(0..60).contains(&minute) || !(0..=60).contains(&second) {
        return None;
    }
    let days = days_from_civil(year, month, day);
    Some(((days * 24 + hour) * 60 + minute - offset_minutes) * 60_000 + second * 1_000 + millis)
}

/// The days between 1970-01-01 and the given civil date (Howard Hinnant's `days_from_civil`).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_prime = (month + 9) % 12;
    let day_of_year = (153 * month_prime + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Do two records name the same instant, or the same string?
fn timestamps_match(left: Option<&str>, right: Option<&str>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) if left == right => true,
        (Some(left), Some(right)) => match (codex_timestamp_millis(left), codex_timestamp_millis(right))
        {
            (Some(left), Some(right)) => (left - right).abs() <= REFERENCE_CODEX_DEDUPE_WINDOW_MS,
            _ => false,
        },
        _ => false,
    }
}

// ---------------------------------------------------------------------------------------
// Static patterns
// ---------------------------------------------------------------------------------------

fn codex_selected_skill_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"(?s)^<skill>\s*<name>([^<>\r\n]+)</name>\s*<path>([^<>\r\n]+)</path>.*</skill>$")
            .expect("static codex selected-skill pattern")
    })
}

fn codex_question_reply_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"(?s)^<send_user_message_question_reply>\s*(.*?)\s*</send_user_message_question_reply>$")
            .expect("static codex question-reply pattern")
    })
}

fn codex_data_image_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"^data:(image/(?:png|jpeg|gif|webp));base64,([A-Za-z0-9+/]*={0,2})$")
            .expect("static codex data-image pattern")
    })
}

fn codex_scheme_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"^[a-z][a-z\d+.-]*:").expect("static codex scheme pattern")
    })
}

fn codex_image_ref_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"^codex-[a-f0-9]{64}$").expect("static codex image ref pattern"))
}

fn codex_apply_patch_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r#"tools\.apply_patch\(\s*("(?:[^"\\]|\\.)*"|`(?:[^`\\]|\\.)*`)"#)
            .expect("static codex apply-patch pattern")
    })
}

fn codex_patch_file_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"(?m)^\*\*\* (?:Update|Add|Delete) File: (.+)$")
            .expect("static codex patch-file pattern")
    })
}

fn codex_script_completed_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"(?m)^Script completed\b").expect("static codex script-completed pattern"))
}

fn codex_script_failed_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"(?m)^Script failed\b").expect("static codex script-failed pattern"))
}

fn codex_apply_patch_verification_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"apply_patch verification failed").expect("static codex patch-verification pattern")
    })
}

fn codex_exit_code_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"(?m)^(?:Process exited with code|Exit code:) (\d+)$")
            .expect("static codex exit-code pattern")
    })
}

fn codex_json_exit_code_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r#""exit_code":\s*(\d+)"#).expect("static codex json exit-code pattern")
    })
}

fn codex_shell_meta_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"[\n;&|<>`$]").expect("static codex shell-meta pattern"))
}

fn codex_shell_read_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r#"^(?:cat|head(?:\s+-n\s+\d+)?|sed\s+-n\s+['"]?\d+(?:,\d+)?p['"]?)\s+(?:"([^"\n]+)"|'([^'\n]+)'|(\S+))\s*$"#)
            .expect("static codex shell-read pattern")
    })
}

// ---------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::reference_chat::{
        history_claude, history_gjc, history_omp, history_omo, history_pi,
    };

    /// The family lanes' own fixtures, read here to prove the dispatcher routes each kind to the
    /// reader that owns it. `include_str!` binds the bytes at compile time.
    const CLAUDE_NORMAL: &str = include_str!("fixtures/claude/normal.jsonl");
    const OMP_BASIC: &str = include_str!("fixtures/omp/basic-session.jsonl");
    const OMO_AUTHENTIC: &str = include_str!("fixtures/omo/session-authentic.jsonl");
    const GJC_NORMAL: &str = include_str!("fixtures/gjc/normal.jsonl");
    const PI_NORMAL: &str = include_str!("fixtures/pi/normal.jsonl");

    /// This lane's own Codex fixtures live inline: plan task 2's scope is this one file, so the
    /// Codex records are constants here instead of a `fixtures/codex/` directory.
    const CODEX_NORMAL: &str = concat!(
        r#"{"timestamp":"2026-10-06T09:00:00.000Z","type":"session_meta","payload":{"id":"0192f0c1-1111-7000-8000-000000000001","cwd":"/Users/dev/project","cli_version":"0.44.0"}}"#,
        "\n",
        r#"{"timestamp":"2026-10-06T09:00:01.000Z","type":"event_msg","payload":{"type":"task_started","turn_id":"t1","started_at":"2026-10-06T09:00:01.000Z"}}"#,
        "\n",
        r#"{"timestamp":"2026-10-06T09:00:01.100Z","type":"event_msg","payload":{"type":"user_message","kind":"plain","message":"터미널 주소좀 줘봐"}}"#,
        "\n",
        r#"{"timestamp":"2026-10-06T09:00:01.100Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"터미널 주소좀 줘봐"}]}}"#,
        "\n",
        r#"{"timestamp":"2026-10-06T09:00:02.000Z","type":"event_msg","payload":{"type":"agent_message","message":"확인해볼게.","phase":"commentary"}}"#,
        "\n",
        r#"{"timestamp":"2026-10-06T09:00:02.500Z","type":"response_item","payload":{"type":"reasoning","summary":[{"type":"summary_text","text":"read the hosts file first"}]}}"#,
        "\n",
        r#"{"timestamp":"2026-10-06T09:00:03.000Z","type":"response_item","payload":{"type":"function_call","name":"exec_command","arguments":"{\"cmd\":\"cat hosts\"}","call_id":"c1"}}"#,
        "\n",
        r#"{"timestamp":"2026-10-06T09:00:04.000Z","type":"response_item","payload":{"type":"function_call_output","call_id":"c1","output":"127.0.0.1 localhost"}}"#,
        "\n",
        r#"{"timestamp":"2026-10-06T09:00:05.000Z","type":"event_msg","payload":{"type":"agent_message","message":"http://100.123.228.51:7317","phase":"final_answer"}}"#,
        "\n",
        r#"{"timestamp":"2026-10-06T09:00:06.000Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"t1"}}"#,
        "\n",
    );

    const CODEX_PATCH: &str = concat!(
        r#"{"timestamp":"2026-10-06T10:00:00.000Z","type":"event_msg","payload":{"type":"task_started","turn_id":"t2"}}"#,
        "\n",
        r#"{"timestamp":"2026-10-06T10:00:00.500Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"apply the patch"}]}}"#,
        "\n",
        r#"{"timestamp":"2026-10-06T10:00:01.000Z","type":"response_item","payload":{"type":"custom_tool_call","name":"apply_patch","input":"*** Begin Patch\n*** Update File: src/a.rs\n*** Add File: src/b.rs\n*** End Patch","call_id":"p1"}}"#,
        "\n",
        r#"{"timestamp":"2026-10-06T10:00:02.000Z","type":"response_item","payload":{"type":"custom_tool_call_output","call_id":"p1","output":"Success. Updated the following files:\nM src/a.rs\nA src/b.rs"}}"#,
        "\n",
        r#"{"timestamp":"2026-10-06T10:00:03.000Z","type":"response_item","payload":{"type":"function_call","name":"exec_command","arguments":"{\"cmd\":\"cargo test --lib\"}","call_id":"c9"}}"#,
        "\n",
        r#"{"timestamp":"2026-10-06T10:00:03.500Z","type":"response_item","payload":{"type":"function_call_output","call_id":"gone-call","output":"orphan output"}}"#,
        "\n",
        r#"{"timestamp":"2026-10-06T10:00:04.000Z","type":"response_item","payload":{"type":"function_call_output","call_id":"c9","output":"Exit code: 101"}}"#,
        "\n",
        r#"{"timestamp":"2026-10-06T10:00:05.000Z","type":"event_msg","payload":{"type":"agent_message","message":"Answer line\n<oai-mem-citation>\nmem block\n</oai-mem-citation>","phase":"final_answer"}}"#,
        "\n",
        r#"{"timestamp":"2026-10-06T10:00:06.000Z","type":"event_msg","payload":{"type":"user_message","kind":"plain","message":"<send_user_message_question_reply>[{\"answer\":\"yes\"}]</send_user_message_question_reply>"}}"#,
        "\n",
        r##"{"timestamp":"2026-10-06T10:00:07.000Z","type":"event_msg","payload":{"type":"user_message","kind":"plain","message":"# AGENTS.md instructions for /Users/dev/project\n<INSTRUCTIONS>\nbe nice\n</INSTRUCTIONS>"}}"##,
        "\n",
    );

    const CODEX_SKILLS: &str = concat!(
        r#"{"timestamp":"2026-10-06T11:00:00.000Z","type":"event_msg","payload":{"type":"task_started","turn_id":"t3"}}"#,
        "\n",
        r#"{"timestamp":"2026-10-06T11:00:00.500Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<skill>\n<name>frontend</name>\n<path>/Users/dev/.claude/skills/frontend/SKILL.md</path>\nDo the frontend thing.\n</skill>"},{"type":"input_text","text":"make the card nicer"}]}}"#,
        "\n",
        r#"{"timestamp":"2026-10-06T11:00:01.000Z","type":"response_item","payload":{"type":"function_call","name":"read_file","arguments":"{\"file_path\":\"/Users/dev/.claude/skills/frontend/SKILL.md\"}","call_id":"s1"}}"#,
        "\n",
        r#"{"timestamp":"2026-10-06T11:00:02.000Z","type":"event_msg","payload":{"type":"item_completed","turn_id":"t3","item":{"id":"i1","type":"CommandExecution","status":"completed","exit_code":0,"parsed_cmd":[{"type":"read","path":"/Users/dev/.claude/skills/frontend/SKILL.md"}]}}}"#,
        "\n",
        r#"{"timestamp":"2026-10-06T11:00:03.000Z","type":"event_msg","payload":{"type":"item_completed","turn_id":"other-turn","item":{"id":"i2","type":"CommandExecution","status":"completed","exit_code":0,"parsed_cmd":[{"type":"read","path":"/Users/dev/.claude/skills/other/SKILL.md"}]}}}"#,
        "\n",
        r#"{"timestamp":"2026-10-06T11:00:04.000Z","type":"event_msg","payload":{"type":"item_completed","turn_id":"t3","item":{"id":"i1","type":"CommandExecution","status":"failed","exit_code":1,"parsed_cmd":[{"type":"read","path":"/Users/dev/.claude/skills/frontend/SKILL.md"}]}}}"#,
        "\n",
    );

    const CODEX_IMAGES: &str = concat!(
        r#"{"timestamp":"2026-10-06T12:00:00.000Z","type":"event_msg","payload":{"type":"task_started","turn_id":"t4"}}"#,
        "\n",
        r#"{"timestamp":"2026-10-06T12:00:01.000Z","type":"event_msg","payload":{"type":"user_message","kind":"plain","message":"what is this?","local_images":["/Users/dev/shot.png","data:image/png;base64,iVBORw0KGgo="],"images":["/Users/dev/shot.png","https://example.com/remote.png","notes.txt"]}}"#,
        "\n",
    );

    const CODEX_MALFORMED: &str = concat!(
        r#"{"timestamp":"2026-10-06T13:00:00.000Z","type":"event_msg","payload":{"type":"task_started","turn_id":"t5"}}"#,
        "\n",
        "{ not json at all\n",
        "[1,2,3]\n",
        "\"a scalar\"\n",
        "null\n",
        "\n",
        r#"{"timestamp":"2026-10-06T13:00:01.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"still reads"}]}}"#,
        "\n",
        r#"{"timestamp":"2026-10-06T13:00:02.000Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"torn tail has no newline"}]}"#,
    );

    fn parse(text: &str) -> Vec<ReferenceTurn> {
        parse_codex_transcript_unbounded(text)
    }

    fn tool_part<'a>(turn: &'a ReferenceTurn, name: &str) -> &'a ReferencePart {
        turn.parts
            .iter()
            .find(|part| matches!(part, ReferencePart::Tool { name: held, .. } if held == name))
            .expect("the turn holds the tool call")
    }

    #[test]
    fn the_dispatcher_routes_each_family_to_its_own_reader() {
        // C1: every kind is handed to the lane that reads it, byte for byte — the dispatcher
        // neither re-parses another family's records nor drops the caller's bytes.
        let families = [
            (
                ReferenceNativeHistoryKind::Claude,
                CLAUDE_NORMAL,
                "Add a health endpoint to the API.",
                history_claude::parse_reference_claude_history(
                    ReferenceNativeHistoryKind::Claude,
                    CLAUDE_NORMAL,
                )
                .expect("claude reads its own store"),
            ),
            (
                ReferenceNativeHistoryKind::Omp,
                OMP_BASIC,
                "list the files",
                history_omp::parse_omp_transcript(ReferenceNativeHistoryKind::Omp, OMP_BASIC)
                    .expect("omp reads its own store"),
            ),
            (
                ReferenceNativeHistoryKind::Omo,
                OMO_AUTHENTIC,
                "add a retry to the uploader",
                history_omo::parse_omo_history(ReferenceNativeHistoryKind::Omo, OMO_AUTHENTIC)
                    .expect("omo reads its own store"),
            ),
            (
                ReferenceNativeHistoryKind::Gjc,
                GJC_NORMAL,
                "터미널 주소좀 줘봐",
                history_gjc::parse_reference_gjc(ReferenceNativeHistoryKind::Gjc, GJC_NORMAL)
                    .expect("gjc reads its own store"),
            ),
            (
                ReferenceNativeHistoryKind::Pi,
                PI_NORMAL,
                "Add a /health route to the API.",
                history_pi::parse_pi_history(ReferenceNativeHistoryKind::Pi, PI_NORMAL)
                    .expect("pi reads its own store"),
            ),
            (
                ReferenceNativeHistoryKind::Codex,
                CODEX_NORMAL,
                "터미널 주소좀 줘봐",
                parse(CODEX_NORMAL),
            ),
        ];
        for (kind, bytes, prompt, expected) in families {
            assert!(!expected.is_empty(), "{} reads the fixture", kind.as_str());
            let dispatched = dispatch_reference_history(kind, bytes.as_bytes())
                .unwrap_or_else(|error| panic!("{} dispatches: {error}", kind.as_str()));
            assert_eq!(dispatched, expected, "{} keeps its reader's answer", kind.as_str());
            let rendered = serde_json::to_string(&dispatched).expect("turns serialize");
            assert!(
                rendered.contains(prompt),
                "{} carries its own prompt: {rendered}",
                kind.as_str()
            );
        }
    }

    #[test]
    fn the_dispatcher_refuses_unavailable_and_another_familys_kind() {
        // C1: `Unavailable` is a refusal, never an empty success, and each family refuses a kind
        // that is not its own rather than parsing records it does not read.
        for bytes in [b"".as_slice(), CODEX_NORMAL.as_bytes(), GJC_NORMAL.as_bytes()] {
            let refused = dispatch_reference_history(ReferenceNativeHistoryKind::Unavailable, bytes)
                .expect_err("no reader serves an unsupported source");
            assert!(refused.contains(ReferenceNativeHistoryKind::Unavailable.as_str()));
        }
        for (own, parse_family) in [
            (
                ReferenceNativeHistoryKind::Claude,
                history_claude::parse_reference_claude_history as ReferenceHistoryParser,
            ),
            (ReferenceNativeHistoryKind::Omp, history_omp::parse_omp_transcript),
            (ReferenceNativeHistoryKind::Omo, history_omo::parse_omo_history),
            (ReferenceNativeHistoryKind::Gjc, history_gjc::parse_reference_gjc),
            (ReferenceNativeHistoryKind::Pi, history_pi::parse_pi_history),
            (ReferenceNativeHistoryKind::Codex, parse_codex_history),
        ] {
            assert!(
                parse_family(own, CLAUDE_NORMAL).is_ok(),
                "{} answers its own kind",
                own.as_str()
            );
            for kind in [
                ReferenceNativeHistoryKind::Claude,
                ReferenceNativeHistoryKind::Codex,
                ReferenceNativeHistoryKind::Omp,
                ReferenceNativeHistoryKind::Omo,
                ReferenceNativeHistoryKind::Gjc,
                ReferenceNativeHistoryKind::Pi,
                ReferenceNativeHistoryKind::Unavailable,
            ] {
                if kind == own {
                    continue;
                }
                assert!(
                    parse_family(kind, CLAUDE_NORMAL).is_err(),
                    "{} must refuse {}",
                    own.as_str(),
                    kind.as_str()
                );
            }
        }
        // A genuinely empty file is an empty conversation, which is not a refusal.
        assert!(dispatch_reference_history(ReferenceNativeHistoryKind::Codex, b"")
            .expect("empty reads")
            .is_empty());
        // Undecodable bytes are skipped with the line that holds them, never fatal.
        let mut bytes = CODEX_NORMAL.as_bytes().to_vec();
        bytes.extend_from_slice(&[0xff, 0xfe]);
        assert_eq!(
            dispatch_reference_history(ReferenceNativeHistoryKind::Codex, &bytes)
                .expect("a torn tail byte still reads"),
            parse(CODEX_NORMAL)
        );
    }

    #[test]
    fn a_rollout_reads_a_prompt_merged_assistant_turn_and_its_tool_output() {
        // C2: the ordinary rollout — a task boundary, one prompt, the assistant records that
        // merge into a single turn, and the output that answers the call.
        let turns = parse(CODEX_NORMAL);
        assert_eq!(
            turns.iter().map(|turn| turn.role).collect::<Vec<_>>(),
            vec![ReferenceTurnRole::User, ReferenceTurnRole::Assistant]
        );
        assert_eq!(turns[0].started_at.as_deref(), Some("2026-10-06T09:00:01.100Z"));
        assert_eq!(
            turns[0].parts,
            vec![ReferencePart::Text { text: "터미널 주소좀 줘봐".into(), phase: None }]
        );
        // the assistant turn is dated by the task's own start, and ended by the last recorded
        // activity — the completion event, not the next user's timestamp
        assert_eq!(turns[1].started_at.as_deref(), Some("2026-10-06T09:00:01.000Z"));
        assert_eq!(turns[1].ended_at.as_deref(), Some("2026-10-06T09:00:06.000Z"));
        assert_eq!(turns[1].parts.len(), 4);
        assert_eq!(
            turns[1].parts[0],
            ReferencePart::Text { text: "확인해볼게.".into(), phase: Some(ReferenceTextPhase::Commentary) }
        );
        assert_eq!(
            turns[1].parts[1],
            ReferencePart::Thinking { text: "read the hosts file first".into() }
        );
        let ReferencePart::Tool { name, summary, input, output, error, output_ref, output_size, skill, images } =
            tool_part(&turns[1], "exec_command")
        else {
            unreachable!("matched above")
        };
        assert_eq!(name, "exec_command");
        assert_eq!(summary, "cat hosts");
        assert_eq!(input, "{\n  \"cmd\": \"cat hosts\"\n}");
        assert_eq!(output, "127.0.0.1 localhost");
        assert_eq!(*error, None, "a call that succeeded is not marked failed");
        assert_eq!(*output_ref, None);
        assert_eq!(*output_size, None);
        assert_eq!(*skill, None, "a plain shell read names no skill");
        assert!(images.is_empty());
        assert_eq!(
            turns[1].parts[3],
            ReferencePart::Text {
                text: "http://100.123.228.51:7317".into(),
                phase: Some(ReferenceTextPhase::FinalAnswer)
            }
        );
        // the session header is chrome, not a turn
        let rendered = serde_json::to_string(&turns).expect("turns serialize");
        assert!(!rendered.contains("cli_version"));
        assert!(!rendered.contains("session_meta"));
    }

    #[test]
    fn the_same_message_recorded_twice_is_one_turn() {
        // C2: Codex writes the prompt as an event and again as a response item; the pair is one
        // user turn, not two, and the response's copy does not open a second one.
        let turns = parse(CODEX_NORMAL);
        let users: Vec<&ReferenceTurn> = turns
            .iter()
            .filter(|turn| turn.role == ReferenceTurnRole::User)
            .collect();
        assert_eq!(users.len(), 1, "the duplicate record is not a second turn");
        assert_eq!(users[0].parts.len(), 1);
        // the duplicate is paired, so a third record with the same text opens a turn of its own
        let repeated = concat!(
            r#"{"timestamp":"2026-10-06T09:00:07.000Z","type":"event_msg","payload":{"type":"task_started","turn_id":"t2"}}"#,
            "\n",
            r#"{"timestamp":"2026-10-06T09:00:08.000Z","type":"event_msg","payload":{"type":"user_message","kind":"plain","message":"same words"}}"#,
            "\n",
            r#"{"timestamp":"2026-10-06T09:00:08.200Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"same words"}]}}"#,
            "\n",
            r#"{"timestamp":"2026-10-06T09:00:30.000Z","type":"event_msg","payload":{"type":"user_message","kind":"plain","message":"same words"}}"#,
            "\n",
        );
        let turns = parse(repeated);
        assert_eq!(
            turns.iter().map(|turn| turn.role).collect::<Vec<_>>(),
            vec![ReferenceTurnRole::User, ReferenceTurnRole::User],
            "a message far outside the dedupe window is its own turn"
        );
    }

    #[test]
    fn a_patch_call_is_summed_up_by_the_files_it_touches() {
        // C2: a patch carries its files as the call's summary and the raw patch as its input.
        let turns = parse(CODEX_PATCH);
        let assistant = turns
            .iter()
            .find(|turn| turn.role == ReferenceTurnRole::Assistant)
            .expect("the assistant turn holding the calls");
        let ReferencePart::Tool { summary, input, output, .. } = tool_part(assistant, "apply_patch")
        else {
            unreachable!("matched above")
        };
        assert_eq!(summary, "src/a.rs, src/b.rs");
        assert!(input.starts_with("*** Begin Patch"), "input holds the patch: {input}");
        assert_eq!(output, "Success. Updated the following files:\nM src/a.rs\nA src/b.rs");
    }

    #[test]
    fn a_failing_command_marks_its_call_and_a_result_without_a_call_is_ignored() {
        // C2: the runner's exit code is the only failure flag Codex records, and a result for a
        // call this page never saw is not a row.
        let turns = parse(CODEX_PATCH);
        let assistant = turns
            .iter()
            .find(|turn| turn.role == ReferenceTurnRole::Assistant)
            .expect("the assistant turn holding the calls");
        let ReferencePart::Tool { output, error, output_ref, .. } = tool_part(assistant, "exec_command")
        else {
            unreachable!("matched above")
        };
        assert_eq!(output, "Exit code: 101");
        assert_eq!(*error, Some(true));
        assert_eq!(*output_ref, None, "a short output keeps no fetch ref");
        let rendered = serde_json::to_string(&turns).expect("turns serialize");
        assert!(!rendered.contains("orphan output"), "an unmatched result is dropped");
    }

    #[test]
    fn codex_call_failed_reads_the_runners_own_verdict() {
        // C2: the failure rules, including the 2000-character head they are read from.
        assert!(!codex_call_failed("Script completed\nProcess exited with code 1"));
        assert!(codex_call_failed("Script failed\nboom"));
        assert!(codex_call_failed("apply_patch verification failed: nothing to do"));
        assert!(codex_call_failed("Process exited with code 1"));
        assert!(codex_call_failed("Exit code: 101"));
        assert!(!codex_call_failed("Exit code: 0"));
        assert!(codex_call_failed("{\"exit_code\": 2}"));
        assert!(!codex_call_failed("{\"exit_code\": 0}"));
        assert!(!codex_call_failed("all good"));
        let far_tail = format!("{}\nScript failed\n", "x".repeat(2_100));
        assert!(
            !codex_call_failed(&far_tail),
            "only the head of the output carries the verdict"
        );
    }

    #[test]
    fn a_long_output_is_cut_with_what_it_takes_to_fetch_the_rest() {
        // C2: past the page limit the output is cut, and the call keeps its ref and full length.
        let long = "x".repeat(REFERENCE_CODEX_TOOL_OUTPUT_CHARS + 1_000);
        let text = format!(
            concat!(
                r#"{{"timestamp":"2026-10-06T14:00:00.000Z","type":"response_item","payload":{{"type":"function_call","name":"exec_command","arguments":"{{\"cmd\":\"cat big\"}}","call_id":"big"}}}}"#,
                "\n",
                r#"{{"timestamp":"2026-10-06T14:00:01.000Z","type":"response_item","payload":{{"type":"function_call_output","call_id":"big","output":"{long}"}}}}"#,
                "\n",
            ),
            long = long
        );
        let turns = parse(&text);
        let ReferencePart::Tool { output, output_ref, output_size, .. } =
            tool_part(&turns[0], "exec_command")
        else {
            unreachable!("matched above")
        };
        assert_eq!(output.chars().count(), REFERENCE_CODEX_TOOL_OUTPUT_CHARS + "\n… trimmed".chars().count());
        assert!(output.ends_with("\n… trimmed"));
        assert_eq!(output_ref.as_deref(), Some("big"));
        assert_eq!(*output_size, Some(long.chars().count() as u64));
    }

    #[test]
    fn memory_citations_are_metadata_unless_the_answer_quotes_them() {
        // C2: a memory block appended to an answer is metadata; one the answer quotes as code is
        // the answer's own text and stays.
        assert_eq!(
            without_memory_citations("Answer line\n<oai-mem-citation>\nmem block\n</oai-mem-citation>"),
            "Answer line"
        );
        assert_eq!(
            without_memory_citations("Use `<oai-mem-citation>` literally."),
            "Use `<oai-mem-citation>` literally."
        );
        assert_eq!(without_memory_citations("no citations here\n"), "no citations here");
        // and through the parser: the assistant's text is the answer, not the block
        let turns = parse(CODEX_PATCH);
        let assistant = turns
            .iter()
            .find(|turn| turn.role == ReferenceTurnRole::Assistant)
            .expect("the assistant turn");
        let ReferencePart::Text { text, .. } = assistant.parts.last().expect("the answer part") else {
            unreachable!("the last part is the answer")
        };
        assert_eq!(text, "Answer line");
    }

    #[test]
    fn a_question_reply_envelope_shows_what_was_answered() {
        // C2: an answer to Codex's queued questions is the answer, not the envelope.
        assert_eq!(
            question_reply("<send_user_message_question_reply>[{\"answer\":\"yes\"}]</send_user_message_question_reply>"),
            Some("yes".to_string())
        );
        assert_eq!(
            question_reply("<send_user_message_question_reply>[{\"answer\":\"a\"},{\"answer\":\"\"}]</send_user_message_question_reply>"),
            Some("a".to_string())
        );
        assert_eq!(question_reply("just words"), None);
        assert_eq!(question_reply("<send_user_message_question_reply>not json</send_user_message_question_reply>"), None);
        let turns = parse(CODEX_PATCH);
        let reply = turns
            .iter()
            .find(|turn| {
                matches!(turn.parts.as_slice(), [ReferencePart::Text { text, .. }] if text == "yes")
            })
            .expect("the answered question is a user turn");
        assert_eq!(reply.role, ReferenceTurnRole::User);
    }

    #[test]
    fn runtime_context_in_the_users_seat_is_not_a_turn() {
        // C2: AGENTS.md instructions and an environment envelope reach the model as user messages
        // but nobody typed them, so they open no turn.
        let turns = parse(CODEX_PATCH);
        let rendered = serde_json::to_string(&turns).expect("turns serialize");
        assert!(!rendered.contains("AGENTS.md"), "runtime context is not the user's words");
        assert!(!rendered.contains("be nice"));
        assert_eq!(
            turns.iter().filter(|turn| turn.role == ReferenceTurnRole::User).count(),
            2,
            "only the prompt and the answered question are user turns"
        );
        assert!(context_only("<environment_context>\n cwd: /tmp\n</environment_context>"));
        assert!(context_only("<turn_aborted>\nThe user interrupted\n</turn_aborted>"));
        assert!(!context_only("<turn_aborted>a</environment_context>"));
        assert!(!context_only("please read <environment_context> for me"));
    }

    #[test]
    fn an_image_the_user_attached_becomes_a_ref_without_the_bytes() {
        // C2: a native image is addressed by a hash ref; a remote URL and an unknown extension
        // are not images this reader can serve, and no path or blob reaches the turns.
        let turns = parse(CODEX_IMAGES);
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].parts.len(), 3, "two images then the prompt");
        let ReferencePart::Image { media_type, r#ref } = &turns[0].parts[0] else {
            unreachable!("the first part is the local image")
        };
        assert_eq!(media_type, "image/png");
        assert!(is_codex_image_ref(r#ref), "the ref is a codex image ref");
        let ReferencePart::Image { media_type: inline, r#ref: inline_ref } = &turns[0].parts[1] else {
            unreachable!("the second part is the inline image")
        };
        assert_eq!(inline, "image/png");
        assert_ne!(r#ref, inline_ref, "different sources keep different refs");
        assert_eq!(
            turns[0].parts[2],
            ReferencePart::Text { text: "what is this?".into(), phase: None }
        );
        let rendered = serde_json::to_string(&turns).expect("turns serialize");
        assert!(!rendered.contains("shot.png"));
        assert!(!rendered.contains("base64"));
        assert!(!rendered.contains("example.com"));
        assert!(!rendered.contains("notes.txt"));
        assert!(codex_image_part("shot.webp").is_some());
        assert!(codex_image_part("shot.tiff").is_none());
        assert!(codex_image_part("https://example.com/shot.png").is_none());
        assert!(codex_image_part(".png").is_none());
    }

    #[test]
    fn a_skill_read_rides_its_tool_call_and_a_completed_item_updates_it() {
        // C2: the pinned reader attaches a skill to the call that read it, and the command's own
        // completion event settles that skill's status — once, for a given item id.
        let turns = parse(CODEX_SKILLS);
        let assistant = turns
            .iter()
            .find(|turn| turn.role == ReferenceTurnRole::Assistant)
            .expect("the assistant turn holding the read");
        let ReferencePart::Tool { skill, .. } = tool_part(assistant, "read_file") else {
            unreachable!("matched above")
        };
        let skill = skill.as_ref().expect("the read names the skill it read");
        assert_eq!(skill.name, "frontend");
        assert_eq!(skill.evidence, ReferenceSkillEvidence::Instructions);
        assert_eq!(
            skill.status,
            ReferenceSkillStatus::Loaded,
            "the completed item settles it; the failing repeat with the same id does not"
        );
        assert_eq!(
            skill.path.as_deref(),
            Some("/Users/dev/.claude/skills/frontend/SKILL.md")
        );
        // the user's own words are the request, not the envelope it arrived in
        assert_eq!(
            turns[0].parts,
            vec![ReferencePart::Text { text: "make the card nicer".into(), phase: None }]
        );
        let rendered = serde_json::to_string(&turns).expect("turns serialize");
        assert!(!rendered.contains("<skill>"), "the envelope is not the user's text");
        assert!(
            !rendered.contains("other/SKILL.md"),
            "an item completed on another turn is not this page's"
        );
    }

    #[test]
    fn a_standalone_skill_becomes_its_own_part() {
        // C2: upstream renders a skill part of its own for a completed read no tool part carries
        // (`codex.ts:241`); the same item id read twice is applied once, so no second chip appears.
        let text = concat!(
            r#"{"timestamp":"2026-10-06T11:30:00.000Z","type":"event_msg","payload":{"type":"task_started","turn_id":"t9"}}"#,
            "\n",
            r#"{"timestamp":"2026-10-06T11:30:01.000Z","type":"event_msg","payload":{"type":"item_completed","turn_id":"t9","item":{"id":"i9","type":"CommandExecution","status":"completed","exit_code":0,"parsed_cmd":[{"type":"read","path":"/Users/dev/.claude/skills/lonely/SKILL.md"}]}}}"#,
            "\n",
            r#"{"timestamp":"2026-10-06T11:30:02.000Z","type":"event_msg","payload":{"type":"item_completed","turn_id":"t9","item":{"id":"i9","type":"CommandExecution","status":"failed","exit_code":1,"parsed_cmd":[{"type":"read","path":"/Users/dev/.claude/skills/lonely/SKILL.md"}]}}}"#,
            "\n",
        );
        let turns = parse(text);
        assert_eq!(turns.len(), 1, "the chip opens the assistant turn the reference gives it");
        assert_eq!(turns[0].role, ReferenceTurnRole::Assistant);
        assert_eq!(turns[0].parts.len(), 1, "the repeated item id is applied once");
        let ReferencePart::Skill { skill } = &turns[0].parts[0] else {
            panic!("the part is a standalone skill, got {:?}", turns[0].parts[0].kind());
        };
        assert_eq!(skill.name, "lonely");
        assert_eq!(skill.evidence, ReferenceSkillEvidence::Instructions);
        assert_eq!(skill.status, ReferenceSkillStatus::Loaded);
        assert_eq!(skill.path.as_deref(), Some("/Users/dev/.claude/skills/lonely/SKILL.md"));
        assert_eq!(
            serde_json::to_value(&turns[0].parts[0]).unwrap(),
            serde_json::json!({"kind": "skill", "skill": {
                "name": "lonely", "evidence": "instructions", "status": "loaded",
                "path": "/Users/dev/.claude/skills/lonely/SKILL.md"}})
        );
        // the reader helper still reports the evidence the parser consumed
        let item = serde_json::json!({
            "id": "i9",
            "type": "CommandExecution",
            "status": "completed",
            "exit_code": 0,
            "parsed_cmd": [{ "type": "read", "path": "/Users/dev/.claude/skills/lonely/SKILL.md" }],
        });
        let skills = codex_read_skills(Some(&item));
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "lonely");
        assert_eq!(skills[0].status, ReferenceSkillStatus::Loaded);
        // a command that is not a read, or a read of something that is not a skill, carries none
        assert!(codex_read_skills(Some(&serde_json::json!({
            "type": "CommandExecution",
            "status": "completed",
            "exit_code": 0,
            "parsed_cmd": [{ "type": "read", "path": "/Users/dev/notes.md" }],
        })))
        .is_empty());
        assert!(codex_read_skills(Some(&serde_json::json!({
            "type": "CommandExecution",
            "status": "completed",
            "parsed_cmd": [{ "type": "read", "path": "/a/SKILL.md" }],
        })))
        .is_empty());
    }

    #[test]
    fn a_selected_skill_envelope_is_not_the_users_text() {
        // C2: Codex injects an explicitly selected skill as a user-context envelope; the chat shows
        // the request underneath it, and the skill is its own part, once (`codex.ts:266-276`).
        let envelope = "<skill>\n<name>frontend</name>\n<path>/Users/dev/.claude/skills/frontend/SKILL.md</path>\nDo the thing.\n</skill>";
        let content = serde_json::json!([
            { "type": "input_text", "text": envelope },
            { "type": "input_text", "text": envelope },
            { "type": "input_text", "text": "style the badge" },
        ]);
        let skills = codex_selected_skills(Some(&content));
        assert_eq!(skills.len(), 2, "the scan reports every envelope block it found");
        assert_eq!(skills[0].name, "frontend");
        assert_eq!(skills[0].evidence, ReferenceSkillEvidence::Instructions);
        assert_eq!(
            codex_selected_skills(Some(&serde_json::json!([
                { "type": "input_text", "text": "no envelope here" }
            ]))),
            Vec::new()
        );
        assert_eq!(codex_selected_skill("no envelope here"), None);

        let text = serde_json::to_string(&serde_json::json!({
            "timestamp": "2026-10-06T11:40:00.000Z",
            "type": "response_item",
            "payload": { "type": "message", "role": "user", "content": content },
        }))
        .expect("the record serializes");
        let turns = parse(&text);
        let user = turns
            .iter()
            .find(|turn| turn.role == ReferenceTurnRole::User)
            .expect("the prompt is a user turn");
        assert_eq!(
            user.parts,
            vec![ReferencePart::Text { text: "style the badge".into(), phase: None }],
            "the envelope is not the user's text"
        );
        let chips: Vec<&ReferencePart> = turns
            .iter()
            .flat_map(|turn| turn.parts.iter())
            .filter(|part| matches!(part, ReferencePart::Skill { .. }))
            .collect();
        assert_eq!(chips.len(), 1, "one chip per skill, however many envelopes named it");
        let ReferencePart::Skill { skill } = chips[0] else {
            unreachable!("matched above")
        };
        assert_eq!(skill.name, "frontend");
        assert_eq!(skill.path.as_deref(), Some("/Users/dev/.claude/skills/frontend/SKILL.md"));
        assert_eq!(skill.status, ReferenceSkillStatus::Loaded);
        let rendered = serde_json::to_string(&turns).expect("turns serialize");
        assert!(!rendered.contains("<skill>"), "the envelope is not the user's text");
    }

    #[test]
    fn malformed_and_torn_lines_are_skipped_without_losing_the_rest() {
        // C2: an unreadable line, a JSON scalar, an array and a tail line Codex has not finished
        // writing are all skipped; the records that did parse still read.
        let turns = parse(CODEX_MALFORMED);
        assert_eq!(
            turns.iter().map(|turn| turn.role).collect::<Vec<_>>(),
            vec![ReferenceTurnRole::User]
        );
        assert_eq!(
            turns[0].parts,
            vec![ReferencePart::Text { text: "still reads".into(), phase: None }]
        );
        let rendered = serde_json::to_string(&turns).expect("turns serialize");
        assert!(!rendered.contains("torn tail has no newline"));
        assert!(!rendered.contains("a scalar"));
    }

    #[test]
    fn the_turn_cap_keeps_the_newest_turns_and_zero_keeps_them_all() {
        // C2: the reference's `slice(-maxTurns)` window, including its `slice(-0)` quirk.
        let text: String = (0..3)
            .map(|index| {
                format!(
                    "{{\"timestamp\":\"2026-10-06T09:00:0{index}.000Z\",\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":[{{\"type\":\"input_text\",\"text\":\"prompt {index}\"}}]}}}}\n"
                )
            })
            .collect();
        let all = parse_codex_transcript_with_limit(&text, 0);
        assert_eq!(all.len(), 3, "zero keeps every turn");
        assert_eq!(all.len(), parse_codex_transcript(&text).len());
        let newest = parse_codex_transcript_with_limit(&text, 2);
        assert_eq!(newest.len(), 2);
        assert_eq!(
            newest[0].parts,
            vec![ReferencePart::Text { text: "prompt 1".into(), phase: None }]
        );
        assert_eq!(
            newest[1].parts,
            vec![ReferencePart::Text { text: "prompt 2".into(), phase: None }]
        );
        assert_eq!(
            parse_codex_history(ReferenceNativeHistoryKind::Codex, &text)
                .expect("the page reader reads")
                .len(),
            3,
            "the page reader applies no cap"
        );
    }

    #[test]
    fn the_codex_turn_mark_is_the_task_boundary() {
        // C2: the pager's cheap filter and its structural check agree, and neither claims a line
        // that is not a task boundary.
        let started = r#"{"timestamp":"2026-10-06T09:00:01.000Z","type":"event_msg","payload":{"type":"task_started","turn_id":"t1"}}"#;
        let prompt = r#"{"timestamp":"2026-10-06T09:00:02.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}]}}"#;
        assert!(started.contains(REFERENCE_CODEX_TURN_MARK));
        assert!(codex_line_opens_turn(started));
        assert!(!prompt.contains(REFERENCE_CODEX_TURN_MARK));
        assert!(!codex_line_opens_turn(prompt));
        assert!(!codex_line_opens_turn("{ not json"));
        assert!(!codex_line_opens_turn("[1,2,3]"));
        assert!(!codex_line_opens_turn(
            r#"{"type":"event_msg","payload":{"type":"task_complete","turn_id":"t1"}}"#
        ));
    }

    #[test]
    fn only_the_codex_kind_is_answered() {
        // C1: another family's kind is refused, and `Unavailable` is never an empty success.
        for kind in [
            ReferenceNativeHistoryKind::Claude,
            ReferenceNativeHistoryKind::Omp,
            ReferenceNativeHistoryKind::Omo,
            ReferenceNativeHistoryKind::Gjc,
            ReferenceNativeHistoryKind::Pi,
            ReferenceNativeHistoryKind::Unavailable,
        ] {
            let refused = parse_codex_history(kind, CODEX_NORMAL);
            assert!(refused.is_err(), "{} must not be answered by the codex family", kind.as_str());
            assert!(refused.unwrap_err().contains(kind.as_str()));
        }
        assert!(REFERENCE_CODEX_PARSER(ReferenceNativeHistoryKind::Codex, CODEX_NORMAL).is_ok());
        assert!(REFERENCE_CODEX_PARSER(ReferenceNativeHistoryKind::Unavailable, "").is_err());
    }

    #[test]
    fn no_abandoned_branches_and_no_fabricated_turns_are_ever_produced() {
        // C2: a rollout keeps no entry tree, and every turn this reader returns carries a part.
        for text in [CODEX_NORMAL, CODEX_PATCH, CODEX_SKILLS, CODEX_IMAGES, CODEX_MALFORMED] {
            for turn in parse(text) {
                assert!(turn.abandoned.is_none(), "codex discloses no abandoned branches");
                assert!(!turn.parts.is_empty(), "a turn with no parts is dropped, not returned");
            }
        }
    }

    #[test]
    fn the_timestamp_window_is_read_only_from_the_shapes_codex_writes() {
        // C2: the dedupe window is a real instant comparison for ISO-8601, and exact string
        // equality for anything else — never a wrong "same message" answer.
        assert_eq!(codex_timestamp_millis("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(codex_timestamp_millis("2026-10-06T09:00:01.000Z"), Some(1_791_277_201_000));
        assert_eq!(codex_timestamp_millis("2026-10-06T09:00:01+00:00"), Some(1_791_277_201_000));
        assert_eq!(codex_timestamp_millis("2024-01-01T00:00:00Z"), Some(1_704_067_200_000));
        assert_eq!(
            codex_timestamp_millis("2026-10-06T10:00:01+01:00"),
            codex_timestamp_millis("2026-10-06T09:00:01Z")
        );
        assert_eq!(codex_timestamp_millis("not a timestamp"), None);
        assert!(timestamps_match(None, None));
        assert!(timestamps_match(Some(""), Some("")));
        assert!(timestamps_match(
            Some("2026-10-06T09:00:01.000Z"),
            Some("2026-10-06T09:00:01.900Z")
        ));
        assert!(!timestamps_match(
            Some("2026-10-06T09:00:01.000Z"),
            Some("2026-10-06T09:00:03.000Z")
        ));
        assert!(!timestamps_match(Some(""), Some("2026-10-06T09:00:01.000Z")));
    }

    #[test]
    fn a_patch_is_only_a_patch_when_it_opens_one() {
        // C2: `patchText` finds a patch a bare call carries or an exec script wraps, and refuses
        // prose that merely mentions one.
        assert_eq!(
            patch_text("*** Begin Patch\n*** Update File: a.rs\n*** End Patch").as_deref(),
            Some("*** Begin Patch\n*** Update File: a.rs\n*** End Patch")
        );
        assert_eq!(
            patch_text("const p = await tools.apply_patch(\"*** Begin Patch\\n*** Add File: b.rs\\n*** End Patch\");")
                .as_deref(),
            Some("*** Begin Patch\n*** Add File: b.rs\n*** End Patch")
        );
        assert_eq!(patch_text("just prose"), None);
        assert_eq!(
            patch_files("*** Begin Patch\n*** Update File: a.rs\n*** Add File: b.rs\n*** Delete File: c.rs\n*** Update File: a.rs\n*** End Patch"),
            vec!["a.rs".to_string(), "b.rs".to_string(), "c.rs".to_string()]
        );
        assert!(patch_files("no files here").is_empty());
    }

    #[test]
    fn a_shell_read_names_a_skill_only_when_it_reads_one() {
        // C2: a literal single-file read is evidence; a script, a search, an interpolated path and
        // a mention are not.
        let read = serde_json::json!({ "cmd": "cat /Users/dev/.claude/skills/frontend/SKILL.md" });
        let skill = codex_read_call("exec_command", read.as_object().expect("an object"))
            .expect("a literal read is evidence");
        assert_eq!(skill.name, "frontend");
        assert_eq!(skill.status, ReferenceSkillStatus::Requested);
        assert_eq!(skill.evidence, ReferenceSkillEvidence::Instructions);
        for command in [
            "cat $HOME/SKILL.md",
            "cat /a/SKILL.md && echo done",
            "grep -r SKILL.md .",
            "ls /a/SKILL.md",
        ] {
            let args = serde_json::json!({ "cmd": command });
            assert!(
                codex_read_call("exec_command", args.as_object().expect("an object")).is_none(),
                "{command} is not a literal read"
            );
        }
        let read_file = serde_json::json!({ "path": "/a/b/SKILL.md" });
        assert!(
            codex_read_call("read_file", read_file.as_object().expect("an object")).is_some(),
            "the read_file spelling reads the path"
        );
        assert!(codex_read_call("write_file", read_file.as_object().expect("an object")).is_none());
    }
}
