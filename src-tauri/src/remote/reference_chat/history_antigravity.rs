//! Antigravity CLI (`agy`) native history reader (no upstream port; measured against agy 1.4.4).
//!
//! agy keeps two stores per conversation. The SQLite `conversations/<id>.db` `steps.step_payload`
//! column is protobuf with no published schema, so it is not read. The JSONL transcript the hooks
//! name as `transcriptPath`, `~/.gemini/antigravity-cli/brain/<id>/.system_generated/logs/
//! transcript_full.jsonl`, is (`.omo-parity/facts/local-2.md` §1 "Session store", item 2). Its
//! records are `{step_index, source, type, status, created_at, content?, thinking?, tool_calls?,
//! media?, error?}`; the role is in `type`:
//!
//! * `USER_INPUT` — the prompt, wrapped `<USER_REQUEST>\n…\n</USER_REQUEST>` and followed by
//!   `<ADDITIONAL_METADATA>` / `<USER_SETTINGS_CHANGE>` envelopes that are context, not prompt.
//! * `SYSTEM_MESSAGE` — a message the runtime put in the user's seat (`<SYSTEM_MESSAGE>…`).
//! * `PLANNER_RESPONSE` — the model: `thinking`, prose `content`, `tool_calls[{name, args}]`.
//! * `GENERIC` — a tool's output. Calls carry no id, so the outputs answer the pending calls in
//!   order (measured over every local transcript: a two-call step is followed by two `GENERIC`s).
//! * `ERROR_MESSAGE` — a runtime error (`error`), e.g. a 429 quota refusal.
//!
//! Pure: it reads the bytes the resolver handed over and never resolves a session itself.

use std::collections::VecDeque;

use serde_json::Value;

use super::history_parse::{reference_tool_input, reference_tool_summary, ReferenceTurnLog};
use super::types::{
    ReferenceHistoryParser, ReferenceNativeHistoryKind, ReferencePart, ReferenceTurn, ReferenceTurnSource,
};

/// The lane's parser: antigravity-transcript only; any other kind is refused.
pub fn parse_reference_antigravity_history(
    kind: ReferenceNativeHistoryKind,
    text: &str,
) -> Result<Vec<ReferenceTurn>, String> {
    if kind != ReferenceNativeHistoryKind::Antigravity {
        return Err(format!(
            "the antigravity history family reads antigravity-transcript only, not {}",
            kind.as_str()
        ));
    }
    Ok(parse_antigravity_transcript(text))
}

/// Compile-time proof that the lane satisfies the frozen signature.
const _: ReferenceHistoryParser = parse_reference_antigravity_history;

/// A page boundary: a prompt, or a message the runtime put in the user's seat.
pub fn antigravity_line_opens_turn(line: &str) -> bool {
    (line.contains("USER_INPUT") || line.contains("SYSTEM_MESSAGE"))
        && serde_json::from_str::<Value>(line).is_ok_and(|entry| {
            matches!(entry.get("type").and_then(Value::as_str), Some("USER_INPUT" | "SYSTEM_MESSAGE"))
        })
}

/// Split one agy transcript into turns. A torn or unparseable line is skipped.
pub fn parse_antigravity_transcript(text: &str) -> Vec<ReferenceTurn> {
    let mut log = ReferenceTurnLog::default();
    let mut pending: VecDeque<String> = VecDeque::new();
    for line in text.split('\n') {
        let Ok(entry) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        let at = entry.get("created_at").and_then(Value::as_str);
        let content = entry.get("content").and_then(Value::as_str).unwrap_or("");
        match entry.get("type").and_then(Value::as_str) {
            Some("USER_INPUT") => {
                let prompt = user_request(content);
                if prompt.is_empty() {
                    continue;
                }
                pending.clear();
                log.user(at.map(str::to_string), None, vec![ReferencePart::Text { text: prompt, phase: None }]);
            }
            Some("SYSTEM_MESSAGE") => {
                let message = system_message(content);
                if message.is_empty() {
                    continue;
                }
                pending.clear();
                log.user(
                    at.map(str::to_string),
                    Some(ReferenceTurnSource::Runtime),
                    vec![ReferencePart::Notice { text: message, source: Some("SYSTEM_MESSAGE".to_string()) }],
                );
            }
            Some("PLANNER_RESPONSE") => {
                if let Some(thinking) = entry.get("thinking").and_then(Value::as_str).filter(|text| !text.is_empty()) {
                    log.part(at, ReferencePart::Thinking { text: thinking.to_string() });
                }
                if !content.is_empty() {
                    log.part(at, ReferencePart::Text { text: content.to_string(), phase: None });
                }
                let step = entry.get("step_index").map(Value::to_string).unwrap_or_default();
                let calls = entry.get("tool_calls").and_then(Value::as_array);
                for (index, call) in calls.into_iter().flatten().enumerate() {
                    let Some(name) = call.get("name").and_then(Value::as_str) else {
                        continue;
                    };
                    let key = format!("agy-{step}-{index}");
                    let args = call.get("args");
                    log.tool(at, &key, name, reference_tool_summary(args), reference_tool_input(args));
                    pending.push_back(key);
                }
            }
            Some("GENERIC") => {
                let failed = entry.get("error").is_some_and(|error| !error.is_null());
                match pending.pop_front() {
                    Some(key) => log.tool_output(at, &key, content.to_string(), failed),
                    None if !content.is_empty() => {
                        log.part(at, ReferencePart::Notice { text: content.to_string(), source: Some("GENERIC".to_string()) })
                    }
                    None => {}
                }
            }
            Some("ERROR_MESSAGE") => {
                if let Some(error) = entry.get("error").and_then(Value::as_str).filter(|text| !text.is_empty()) {
                    log.part(at, ReferencePart::Notice { text: error.to_string(), source: Some("ERROR_MESSAGE".to_string()) });
                }
            }
            _ => {}
        }
    }
    log.finish()
}

/// The prompt inside agy's `<USER_REQUEST>` envelope; the metadata envelopes after it are not
/// the user's words. A record with no envelope is the prompt as written.
fn user_request(content: &str) -> String {
    match content.split_once("<USER_REQUEST>\n") {
        Some((_, rest)) => rest.split_once("\n</USER_REQUEST>").map_or(rest, |(request, _)| request).to_string(),
        None => content.trim().to_string(),
    }
}

/// The body of agy's `<SYSTEM_MESSAGE>` envelope (the last opening tag: the preamble names the tag
/// once in prose before the envelope opens).
fn system_message(content: &str) -> String {
    match content.rsplit_once("<SYSTEM_MESSAGE>\n") {
        Some((_, rest)) => rest.split_once("\n</SYSTEM_MESSAGE>").map_or(rest, |(body, _)| body).to_string(),
        None => content.trim().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::reference_chat::types::ReferenceTurnRole;

    /// Redacted copies of real `brain/<id>/.system_generated/logs/transcript_full.jsonl` files.
    const TOOL_SESSION: &str = include_str!("fixtures/antigravity/history/tool-session.jsonl");
    const SUBAGENT_SESSION: &str = include_str!("fixtures/antigravity/history/subagent-session.jsonl");
    const QUOTA_ERROR: &str = include_str!("fixtures/antigravity/history/quota-error.jsonl");

    fn parse(text: &str) -> Vec<ReferenceTurn> {
        parse_reference_antigravity_history(ReferenceNativeHistoryKind::Antigravity, text)
            .expect("antigravity reads its own store")
    }

    #[test]
    fn antigravity_session_pairs_each_output_with_its_call_in_order() {
        let turns = parse(TOOL_SESSION);
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].role, ReferenceTurnRole::User);
        let ReferencePart::Text { text: prompt, .. } = &turns[0].parts[0] else {
            panic!("the prompt is text: {:?}", turns[0].parts);
        };
        assert!(prompt.starts_with("Use your generate_image tool"), "{prompt}");
        assert!(!prompt.contains("ADDITIONAL_METADATA"), "{prompt}");
        let assistant = &turns[1];
        assert_eq!(assistant.role, ReferenceTurnRole::Assistant);
        assert_eq!(assistant.started_at.as_deref(), Some("2026-09-26T05:55:48Z"));
        assert_eq!(assistant.ended_at.as_deref(), Some("2026-09-26T05:56:19Z"));
        let tools: Vec<(&str, &str)> = assistant
            .parts
            .iter()
            .filter_map(|part| match part {
                ReferencePart::Tool { name, output, .. } => Some((name.as_str(), output.as_str())),
                _ => None,
            })
            .collect();
        assert_eq!(tools.len(), 3);
        assert_eq!(tools[0].0, "generate_image");
        assert!(tools[0].1.contains("Using prompt:"), "{}", tools[0].1);
        assert_eq!(tools[1].0, "run_command");
        assert!(tools[1].1.contains("The command exited with code 0."), "{}", tools[1].1);
        assert!(matches!(assistant.parts.first(), Some(ReferencePart::Thinking { .. })));
        assert!(matches!(
            assistant.parts.last(),
            Some(ReferencePart::Text { text, .. }) if text.starts_with("SAVED ")
        ));
    }

    #[test]
    fn antigravity_runtime_messages_land_in_the_user_seat() {
        let turns = parse(SUBAGENT_SESSION);
        assert_eq!(turns[0].source, Some(ReferenceTurnSource::Runtime));
        let ReferencePart::Notice { text, source } = &turns[0].parts[0] else {
            panic!("a system message is a notice: {:?}", turns[0].parts);
        };
        assert!(text.starts_with("[Message] timestamp="), "{text}");
        assert!(!text.contains("not actually sent by the user"), "{text}");
        assert_eq!(source.as_deref(), Some("SYSTEM_MESSAGE"));
        assert!(matches!(
            turns[1].parts.last(),
            Some(ReferencePart::Text { text, .. }) if text.starts_with("Icon generation complete.")
        ));
    }

    #[test]
    fn antigravity_runtime_errors_are_notices_on_the_answer() {
        let turns = parse(QUOTA_ERROR);
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[1].parts.len(), 2);
        assert!(turns[1].parts.iter().all(|part| matches!(
            part,
            ReferencePart::Notice { text, source: Some(source) }
                if text.contains("RESOURCE_EXHAUSTED") && source == "ERROR_MESSAGE"
        )));
    }

    #[test]
    fn antigravity_turns_open_on_prompts_and_system_messages() {
        assert_eq!(TOOL_SESSION.lines().filter(|line| antigravity_line_opens_turn(line)).count(), 1);
        assert_eq!(SUBAGENT_SESSION.lines().filter(|line| antigravity_line_opens_turn(line)).count(), 1);
        assert!(!antigravity_line_opens_turn(r#"{"type":"PLANNER_RESPONSE","content":"USER_INPUT"}"#));
    }

    #[test]
    fn antigravity_parser_refuses_another_family() {
        assert!(parse_reference_antigravity_history(ReferenceNativeHistoryKind::Copilot, TOOL_SESSION).is_err());
        assert!(parse_reference_antigravity_history(ReferenceNativeHistoryKind::Unavailable, "").is_err());
    }
}
