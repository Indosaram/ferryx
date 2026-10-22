//! Kimi Code CLI native history reader (no upstream port; measured on this Mac).
//!
//! Store: `~/.kimi/sessions/<cwd hash>/<session id>/` (`.omo-parity/facts/local-2.md` §3). The
//! directory holds `context.jsonl` (the model context: `{"role":"user","content":…}` and
//! `{"role":"_checkpoint","id":N}`; the assistant record's shape was never observed) and
//! `wire.jsonl`, the UI event stream, which this reader reads. `wire.jsonl` lines are
//! `{"type":"metadata","protocol_version":"1.3"}` and
//! `{"timestamp": <unix seconds>, "message": {"type": …, "payload": …}}` with, as measured:
//!
//! * `TurnBegin` — `payload.user_input`, a string or `[{type:"text", text}]`;
//! * `ContentPart` — `payload` `{type:"text", text}`, the assistant's prose;
//! * `StepBegin`, `StepInterrupted`, `TurnEnd` — bookkeeping, nothing to show.
//!
//! Tool-call and thinking events were never captured (the account's login had expired), so they
//! are not read; see the parity report's UNSETTLED list.

use serde_json::Value;

use super::history_parse::{reference_timestamp, ReferenceTurnLog};
use super::types::{ReferenceHistoryParser, ReferenceNativeHistoryKind, ReferencePart, ReferenceTurn};

/// The lane's parser: kimi-transcript only; any other kind is refused.
pub fn parse_reference_kimi_history(
    kind: ReferenceNativeHistoryKind,
    text: &str,
) -> Result<Vec<ReferenceTurn>, String> {
    if kind != ReferenceNativeHistoryKind::Kimi {
        return Err(format!(
            "the kimi history family reads kimi-transcript only, not {}",
            kind.as_str()
        ));
    }
    Ok(parse_kimi_wire(text))
}

/// Compile-time proof that the lane satisfies the frozen signature.
const _: ReferenceHistoryParser = parse_reference_kimi_history;

/// A page boundary: every prompt is one `TurnBegin` event.
pub fn kimi_line_opens_turn(line: &str) -> bool {
    line.contains("TurnBegin")
        && serde_json::from_str::<Value>(line)
            .is_ok_and(|entry| entry.pointer("/message/type").and_then(Value::as_str) == Some("TurnBegin"))
}

/// Split one `wire.jsonl` into turns. A torn or unparseable line is skipped.
pub fn parse_kimi_wire(text: &str) -> Vec<ReferenceTurn> {
    let mut log = ReferenceTurnLog::default();
    for line in text.split('\n') {
        let Ok(entry) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        let at = reference_timestamp(entry.get("timestamp"), 1000.0);
        let payload = entry.pointer("/message/payload");
        match entry.pointer("/message/type").and_then(Value::as_str) {
            Some("TurnBegin") => {
                let prompt = match payload.and_then(|payload| payload.get("user_input")) {
                    Some(Value::String(text)) => text.clone(),
                    Some(Value::Array(parts)) => parts
                        .iter()
                        .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
                        .filter_map(|part| part.get("text").and_then(Value::as_str))
                        .collect::<Vec<_>>()
                        .join("\n"),
                    _ => String::new(),
                };
                if !prompt.trim().is_empty() {
                    log.user(at, None, vec![ReferencePart::Text { text: prompt, phase: None }]);
                }
            }
            Some("ContentPart") => {
                if payload.and_then(|payload| payload.get("type")).and_then(Value::as_str) == Some("text") {
                    let text = payload.and_then(|payload| payload.get("text")).and_then(Value::as_str).unwrap_or("");
                    log.stream_text(at.as_deref(), text, false);
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
    use crate::remote::reference_chat::types::ReferenceTurnRole;

    /// Real `~/.kimi/sessions/<hash>/<id>/wire.jsonl` files (no private text to redact).
    const SLASH_COMMAND: &str = include_str!("fixtures/kimi/history/slash-command.wire.jsonl");
    const INTERRUPTED: &str = include_str!("fixtures/kimi/history/interrupted.wire.jsonl");

    fn parse(text: &str) -> Vec<ReferenceTurn> {
        parse_reference_kimi_history(ReferenceNativeHistoryKind::Kimi, text).expect("kimi reads its own store")
    }

    #[test]
    fn kimi_wire_parses_a_string_prompt_and_the_reply() {
        let turns = parse(SLASH_COMMAND);
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].role, ReferenceTurnRole::User);
        assert_eq!(turns[0].parts, [ReferencePart::Text { text: "/usage".into(), phase: None }]);
        assert_eq!(turns[0].started_at.as_deref(), Some("2026-02-14T23:19:40.213Z"));
        assert_eq!(turns[1].role, ReferenceTurnRole::Assistant);
        assert_eq!(
            turns[1].parts,
            [ReferencePart::Text { text: "Unknown slash command \"/usage\".".into(), phase: None }]
        );
    }

    #[test]
    fn kimi_wire_reads_text_block_prompts_and_an_interrupted_step_adds_nothing() {
        let turns = parse(INTERRUPTED);
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].parts, [ReferencePart::Text { text: "Run the shell command: ls".into(), phase: None }]);
    }

    #[test]
    fn kimi_turns_open_on_turn_begin_only() {
        assert_eq!(SLASH_COMMAND.lines().filter(|line| kimi_line_opens_turn(line)).count(), 1);
        assert!(!kimi_line_opens_turn(r#"{"message":{"type":"ContentPart","payload":{"type":"text","text":"TurnBegin"}}}"#));
    }

    #[test]
    fn kimi_parser_refuses_another_family() {
        assert!(parse_reference_kimi_history(ReferenceNativeHistoryKind::Grok, SLASH_COMMAND).is_err());
        assert!(parse_reference_kimi_history(ReferenceNativeHistoryKind::Unavailable, "").is_err());
    }
}
