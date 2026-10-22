//! Grok CLI native history reader (no upstream port; measured against grok 1.0.67 on omarchy).
//!
//! Store: `~/.grok/sessions/<url-encoded cwd>/<session id>/updates.jsonl`, the authoritative
//! transcript (`.omo-parity/facts/remote-grok.md` §4). Each line is an ACP notification,
//! `{timestamp: <unix seconds>, method, params: {sessionId, update: {sessionUpdate, …}}}`. Read:
//!
//! * `user_message_chunk` — `content.text`, the prompt (measured);
//! * `agent_message_chunk` / `agent_thought_chunk` — `content.text` (measured kind; the field is
//!   the ACP one grok's own README reads, `grok-evidence/grok-1.0.67-README.md:1136-1139`);
//! * `tool_call` — `toolCallId`, `title` (the tool name), `rawInput` (measured);
//! * `tool_call_update` — `toolCallId`, `content[{type:"content", content:{type:"text", text}}]`,
//!   the call's output so far (measured).
//!
//! `hook_execution`, `turn_completed` and the other x.ai bookkeeping kinds show nothing.

use serde_json::Value;

use super::history_parse::{reference_timestamp, reference_tool_input, reference_tool_summary, ReferenceTurnLog};
use super::types::{ReferenceHistoryParser, ReferenceNativeHistoryKind, ReferenceTurn};

/// The lane's parser: grok-transcript only; any other kind is refused.
pub fn parse_reference_grok_history(
    kind: ReferenceNativeHistoryKind,
    text: &str,
) -> Result<Vec<ReferenceTurn>, String> {
    if kind != ReferenceNativeHistoryKind::Grok {
        return Err(format!(
            "the grok history family reads grok-transcript only, not {}",
            kind.as_str()
        ));
    }
    Ok(parse_grok_updates(text))
}

/// Compile-time proof that the lane satisfies the frozen signature.
const _: ReferenceHistoryParser = parse_reference_grok_history;

/// A page boundary: a prompt chunk.
pub fn grok_line_opens_turn(line: &str) -> bool {
    line.contains("user_message_chunk")
        && serde_json::from_str::<Value>(line).is_ok_and(|entry| {
            entry.pointer("/params/update/sessionUpdate").and_then(Value::as_str) == Some("user_message_chunk")
        })
}

/// Split one `updates.jsonl` into turns. A torn or unparseable line is skipped.
pub fn parse_grok_updates(text: &str) -> Vec<ReferenceTurn> {
    let mut log = ReferenceTurnLog::default();
    for line in text.split('\n') {
        let Ok(entry) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        let Some(update) = entry.pointer("/params/update") else {
            continue;
        };
        let at = reference_timestamp(entry.get("timestamp"), 1000.0);
        let chunk = update.pointer("/content/text").and_then(Value::as_str).unwrap_or("");
        match update.get("sessionUpdate").and_then(Value::as_str) {
            Some("user_message_chunk") => log.user_text_chunk(at, chunk),
            Some("agent_message_chunk") => log.stream_text(at.as_deref(), chunk, false),
            Some("agent_thought_chunk") => log.stream_text(at.as_deref(), chunk, true),
            Some("tool_call") => {
                let Some(key) = update.get("toolCallId").and_then(Value::as_str) else {
                    continue;
                };
                let name = update.get("title").and_then(Value::as_str).unwrap_or("tool");
                let input = update.get("rawInput");
                log.tool(at.as_deref(), key, name, reference_tool_summary(input), reference_tool_input(input));
            }
            Some("tool_call_update") => {
                let Some(key) = update.get("toolCallId").and_then(Value::as_str) else {
                    continue;
                };
                let output: String = update
                    .get("content")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter(|block| block.get("type").and_then(Value::as_str) == Some("content"))
                    .filter_map(|block| block.pointer("/content/text").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join("\n");
                if !output.is_empty() {
                    log.tool_output(at.as_deref(), key, output, false);
                }
            }
            _ => {}
        }
    }
    log.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::reference_chat::types::{ReferencePart, ReferenceTurnRole};

    /// The head of a real `updates.jsonl` (`.omo-parity/facts/grok-evidence/sample-updates-head.jsonl`).
    const UPDATES: &str = include_str!("fixtures/grok/history/updates.jsonl");

    #[test]
    fn grok_updates_parse_the_prompt_and_the_tool_call_with_its_output() {
        let turns = parse_reference_grok_history(ReferenceNativeHistoryKind::Grok, UPDATES)
            .expect("grok reads its own store");
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].role, ReferenceTurnRole::User);
        assert_eq!(turns[0].parts, [ReferencePart::Text { text: "PARITY_TOOL remove a file".into(), phase: None }]);
        assert_eq!(turns[0].started_at.as_deref(), Some("2026-10-22T07:13:00.000Z"));
        assert_eq!(turns[1].role, ReferenceTurnRole::Assistant);
        let [ReferencePart::Tool { name, summary, input, output, .. }] = &turns[1].parts[..] else {
            panic!("one tool call: {:?}", turns[1].parts);
        };
        assert_eq!(name, "run_terminal_command");
        assert_eq!(summary, "probe", "the call's own description");
        assert!(input.contains("\"description\": \"probe\""), "{input}");
        assert_eq!(output, "probe");
    }

    #[test]
    fn grok_streamed_chunks_merge_into_one_part() {
        let text = concat!(
            r#"{"timestamp":1,"params":{"update":{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"hi"}}}}"#,
            "\n",
            r#"{"timestamp":2,"params":{"update":{"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"th"}}}}"#,
            "\n",
            r#"{"timestamp":2,"params":{"update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Hel"}}}}"#,
            "\n",
            r#"{"timestamp":3,"params":{"update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"lo"}}}}"#,
            "\n",
        );
        let turns = parse_grok_updates(text);
        assert_eq!(
            turns[1].parts,
            [
                ReferencePart::Thinking { text: "th".into() },
                ReferencePart::Text { text: "Hello".into(), phase: None },
            ]
        );
        assert_eq!(turns[1].ended_at.as_deref(), Some("1970-01-01T00:00:03.000Z"));
    }

    #[test]
    fn grok_turns_open_on_prompt_chunks_only() {
        assert_eq!(UPDATES.lines().filter(|line| grok_line_opens_turn(line)).count(), 1);
    }

    #[test]
    fn grok_parser_refuses_another_family() {
        assert!(parse_reference_grok_history(ReferenceNativeHistoryKind::Kimi, UPDATES).is_err());
        assert!(parse_reference_grok_history(ReferenceNativeHistoryKind::Unavailable, "").is_err());
    }
}
