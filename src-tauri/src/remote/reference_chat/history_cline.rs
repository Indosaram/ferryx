//! Cline CLI native history reader (no upstream port; measured against Cline CLI 3.0.70).
//!
//! Store: `~/.cline/data/sessions/<id>/<id>.messages.json` (`.omo-parity/facts/local-3.md` §3 and
//! its corrections table), one pretty-printed JSON document
//! `{version, updated_at, agent, sessionId, origin, messages, system_prompt}` that Cline rewrites
//! whole. A document cannot be paged by byte offset, so the history layer reads it whole and hands
//! this reader [`cline_messages_projection`]: one compact `messages[]` entry per line, in order.
//!
//! Each message is `{id, role, content: [blocks], ts}` (`ts` in unix milliseconds):
//!
//! * user `text` — the prompt, wrapped `<user_input mode="act">…</user_input>`;
//! * user `tool_result` — `{tool_use_id, name, content: [{query, result, success}]}`;
//! * assistant `thinking` (`thinking`), `text` (`text`) and `tool_use` (`{id, name, input}`);
//! * an assistant message whose `id` starts with `error_` is a runtime error reply.

use serde_json::Value;

use super::history_parse::{
    reference_timestamp, reference_tool_input, reference_tool_summary, ReferenceTurnLog,
};
use super::types::{ReferenceHistoryParser, ReferenceNativeHistoryKind, ReferencePart, ReferenceTurn};

/// The lane's parser: cline-transcript only; any other kind is refused.
pub fn parse_reference_cline_history(
    kind: ReferenceNativeHistoryKind,
    text: &str,
) -> Result<Vec<ReferenceTurn>, String> {
    if kind != ReferenceNativeHistoryKind::Cline {
        return Err(format!(
            "the cline history family reads cline-transcript only, not {}",
            kind.as_str()
        ));
    }
    Ok(parse_cline_messages(text))
}

/// Compile-time proof that the lane satisfies the frozen signature.
const _: ReferenceHistoryParser = parse_reference_cline_history;

/// The `messages[]` of one `<id>.messages.json` document, one compact JSON line per message.
/// `None` when the document is not Cline's shape (a torn rewrite reads as unreadable, never as an
/// empty conversation).
pub fn cline_messages_projection(document: &str) -> Option<String> {
    let document: Value = serde_json::from_str(document).ok()?;
    let messages = document.get("messages")?.as_array()?;
    let mut projection = String::new();
    for message in messages {
        projection.push_str(&serde_json::to_string(message).ok()?);
        projection.push('\n');
    }
    Some(projection)
}

/// A page boundary: a user message that carries a prompt (not a tool result).
pub fn cline_line_opens_turn(line: &str) -> bool {
    line.contains("\"user\"")
        && serde_json::from_str::<Value>(line).is_ok_and(|message| {
            message.get("role").and_then(Value::as_str) == Some("user")
                && blocks(&message).any(|block| block.get("type").and_then(Value::as_str) == Some("text"))
        })
}

/// Split a projected message list into turns. A torn or unparseable line is skipped.
pub fn parse_cline_messages(text: &str) -> Vec<ReferenceTurn> {
    let mut log = ReferenceTurnLog::default();
    for line in text.split('\n') {
        let Ok(message) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        let at = reference_timestamp(message.get("ts"), 1.0);
        let at = at.as_deref();
        let is_error = message.get("id").and_then(Value::as_str).is_some_and(|id| id.starts_with("error_"));
        match message.get("role").and_then(Value::as_str) {
            Some("user") => {
                let prompt: Vec<String> = blocks(&message)
                    .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
                    .filter_map(|block| block.get("text").and_then(Value::as_str))
                    .map(user_input)
                    .filter(|text| !text.trim().is_empty())
                    .collect();
                if !prompt.is_empty() {
                    log.user(
                        at.map(str::to_string),
                        None,
                        vec![ReferencePart::Text { text: prompt.join("\n"), phase: None }],
                    );
                }
                for block in blocks(&message).filter(|block| block.get("type").and_then(Value::as_str) == Some("tool_result")) {
                    let Some(key) = block.get("tool_use_id").and_then(Value::as_str) else {
                        continue;
                    };
                    let (output, failed) = tool_result(block.get("content"));
                    log.tool_output(at, key, output, failed);
                }
            }
            Some("assistant") => {
                for block in blocks(&message) {
                    match block.get("type").and_then(Value::as_str) {
                        Some("thinking") => {
                            if let Some(text) = block.get("thinking").and_then(Value::as_str).filter(|text| !text.is_empty()) {
                                log.part(at, ReferencePart::Thinking { text: text.to_string() });
                            }
                        }
                        Some("text") => {
                            let Some(text) = block.get("text").and_then(Value::as_str).filter(|text| !text.is_empty()) else {
                                continue;
                            };
                            log.part(
                                at,
                                if is_error {
                                    ReferencePart::Notice { text: text.to_string(), source: Some("error".to_string()) }
                                } else {
                                    ReferencePart::Text { text: text.to_string(), phase: None }
                                },
                            );
                        }
                        Some("tool_use") => {
                            let Some(name) = block.get("name").and_then(Value::as_str) else {
                                continue;
                            };
                            let key = block.get("id").and_then(Value::as_str).unwrap_or("");
                            let input = block.get("input");
                            log.tool(at, key, name, reference_tool_summary(input), reference_tool_input(input));
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    log.finish()
}

fn blocks(message: &Value) -> impl Iterator<Item = &Value> {
    message.get("content").and_then(Value::as_array).into_iter().flatten()
}

/// The prompt inside Cline's `<user_input mode="…">` envelope, or the text as written.
fn user_input(text: &str) -> String {
    let trimmed = text.trim();
    match trimmed.strip_prefix("<user_input") {
        Some(rest) => match rest.split_once('>') {
            Some((_, body)) => body.strip_suffix("</user_input>").unwrap_or(body).to_string(),
            None => trimmed.to_string(),
        },
        None => trimmed.to_string(),
    }
}

/// A `tool_result`'s output: the `result` of each entry (or a plain string), and whether any
/// entry reported `success: false`.
fn tool_result(content: Option<&Value>) -> (String, bool) {
    match content {
        Some(Value::String(text)) => (text.clone(), false),
        Some(Value::Array(entries)) => {
            let failed = entries.iter().any(|entry| entry.get("success").and_then(Value::as_bool) == Some(false));
            let output = entries
                .iter()
                .filter_map(|entry| entry.get("result").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n");
            (output, failed)
        }
        _ => (String::new(), false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::reference_chat::types::ReferenceTurnRole;

    /// Redacted copy of a real `~/.cline/data/sessions/<id>/<id>.messages.json` (Cline CLI 3.0.70).
    const DOCUMENT: &str = include_str!("fixtures/cline/history/1792653340197_ys4yu.messages.json");

    fn turns() -> Vec<ReferenceTurn> {
        let projection = cline_messages_projection(DOCUMENT).expect("the document is Cline's shape");
        parse_reference_cline_history(ReferenceNativeHistoryKind::Cline, &projection)
            .expect("cline reads its own store")
    }

    #[test]
    fn cline_document_parses_prompts_errors_tools_and_answers() {
        let turns = turns();
        assert_eq!(turns.len(), 12);
        assert_eq!(turns[0].role, ReferenceTurnRole::User);
        assert_eq!(
            turns[0].parts,
            [ReferencePart::Text { text: "run the shell command: touch probe.txt".into(), phase: None }]
        );
        assert_eq!(turns[0].started_at.as_deref(), Some("2026-10-22T07:16:01.395Z"));
        assert_eq!(
            turns[1].parts,
            [ReferencePart::Notice { text: "model not found".into(), source: Some("error".into()) }]
        );
        let answer = &turns[3];
        assert_eq!(answer.role, ReferenceTurnRole::Assistant);
        let tools: Vec<(&str, &str, &str)> = answer
            .parts
            .iter()
            .filter_map(|part| match part {
                ReferencePart::Tool { name, summary, output, .. } => {
                    Some((name.as_str(), summary.as_str(), output.as_str()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            tools,
            [
                ("run_commands", "", ""),
                ("run_commands", "", "-rw-r--r--@ 1 dev  wheel  0 Oct 22 16:17 probe.txt\n"),
            ]
        );
        assert!(matches!(answer.parts.first(), Some(ReferencePart::Thinking { .. })));
        assert!(matches!(
            answer.parts.last(),
            Some(ReferencePart::Text { text, .. }) if text.starts_with("Done! I ran `touch probe.txt`")
        ));
        assert_eq!(
            turns.last().map(|turn| turn.parts.last().cloned()),
            Some(Some(ReferencePart::Text { text: "OK".into(), phase: None }))
        );
    }

    #[test]
    fn cline_turns_open_on_prompts_not_tool_results() {
        let projection = cline_messages_projection(DOCUMENT).expect("readable");
        assert_eq!(projection.lines().filter(|line| cline_line_opens_turn(line)).count(), 6);
        assert_eq!(cline_messages_projection("{\"messages\": [").as_deref(), None);
        assert_eq!(cline_messages_projection("{}").as_deref(), None);
    }

    #[test]
    fn cline_parser_refuses_another_family() {
        assert!(parse_reference_cline_history(ReferenceNativeHistoryKind::Cursor, "").is_err());
        assert!(parse_reference_cline_history(ReferenceNativeHistoryKind::Unavailable, "").is_err());
    }
}
