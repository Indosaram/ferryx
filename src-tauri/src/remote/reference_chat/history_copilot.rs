//! Copilot CLI native history reader (no upstream port; measured against Copilot CLI 0.0.375).
//!
//! Store: `~/.copilot/session-state/<session id>.jsonl`, one event per line, every event shaped
//! `{type, data, id, parentId, timestamp}` (`.omo-parity/facts/local-3.md`, "Session store").
//! Read here: `user.message` (`data.content` is the prompt as typed; `transformedContent` is the
//! model's copy with injected context, never shown), `assistant.message` (`data.content` prose,
//! possibly empty, and `data.toolRequests[{toolCallId, name, arguments}]`; a call's one-line
//! summary is the `arguments.description` Copilot writes beside the command, when it wrote one),
//! `tool.execution_complete` (`data.toolCallId`, `data.success`, `data.result.content`) and
//! `session.error` (`data.message`). Every other event (`session.*` bookkeeping, `abort`,
//! `assistant.turn_start/turn_end`, `tool.execution_start`) carries nothing a turn shows.
//!
//! Pure: it reads the bytes the resolver handed over and never resolves a session itself.

use serde_json::Value;

use super::history_parse::{
    reference_bounded_tool_input, reference_native_model, reference_native_patch_body,
    reference_finalize_work_status, reference_tool_input, reference_tool_summary,
    reference_usage_from_object, ReferenceTurnLog,
};
use super::types::{
    ReferenceHistoryParser, ReferenceNativeHistoryKind, ReferencePart, ReferenceTurn, ReferenceTurnRole,
};

/// The lane's parser: copilot-transcript only; any other kind is refused.
pub fn parse_reference_copilot_history(
    kind: ReferenceNativeHistoryKind,
    text: &str,
) -> Result<Vec<ReferenceTurn>, String> {
    if kind != ReferenceNativeHistoryKind::Copilot {
        return Err(format!(
            "the copilot history family reads copilot-transcript only, not {}",
            kind.as_str()
        ));
    }
    Ok(parse_copilot_transcript(text))
}

/// Compile-time proof that the lane satisfies the frozen signature.
const _: ReferenceHistoryParser = parse_reference_copilot_history;

/// A page boundary: every prompt is one `user.message` event.
pub fn copilot_line_opens_turn(line: &str) -> bool {
    line.contains("\"user.message\"")
        && serde_json::from_str::<Value>(line)
            .is_ok_and(|entry| entry.get("type").and_then(Value::as_str) == Some("user.message"))
}

/// Split one Copilot session file into turns. A torn or unparseable line is skipped.
pub fn parse_copilot_transcript(text: &str) -> Vec<ReferenceTurn> {
    let mut log = ReferenceTurnLog::default();
    for line in text.split('\n') {
        let Ok(entry) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        let at = entry.get("timestamp").and_then(Value::as_str);
        let data = entry.get("data");
        match entry.get("type").and_then(Value::as_str) {
            Some("user.message") => {
                let Some(content) = field(data, "content").filter(|content| !content.trim().is_empty()) else {
                    continue;
                };
                log.user(
                    at.map(str::to_string),
                    None,
                    vec![ReferencePart::Text { text: content.to_string(), phase: None , id: None}],
                );
            }
            Some("assistant.message") => {
                if let Some(content) = field(data, "content").filter(|content| !content.is_empty()) {
                    log.part(at, ReferencePart::Text { text: content.to_string(), phase: None , id: None});
                }
                let requests = data.and_then(|data| data.get("toolRequests")).and_then(Value::as_array);
                for request in requests.into_iter().flatten() {
                    let Some(name) = request.get("name").and_then(Value::as_str) else {
                        continue;
                    };
                    let key = request.get("toolCallId").and_then(Value::as_str).unwrap_or("");
                    let arguments = request.get("arguments");
                    let payload = arguments.cloned().unwrap_or(Value::Null);
                    let summary = arguments
                        .and_then(|arguments| arguments.get("description"))
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|description| !description.is_empty())
                        .map_or_else(|| reference_tool_summary(arguments), str::to_string);
                    log.tool(
                        at,
                        key,
                        name,
                        summary,
                        reference_bounded_tool_input(name, &payload, reference_tool_input(arguments)),
                    );
                    if let Some(raw) = request.get("status").and_then(Value::as_str) {
                        log.tool_status(key, raw);
                    }
                    if let Some(body) = reference_native_patch_body(name, &payload) {
                        log.push_diff(at, &body);
                    }
                }
                log.annotate_role(
                    ReferenceTurnRole::Assistant,
                    ReferenceNativeHistoryKind::Copilot,
                    reference_native_model(data.and_then(|data| data.get("model"))),
                    data.and_then(|data| data.get("usage")).and_then(reference_usage_from_object),
                );
            }
            Some("tool.execution_complete") => {
                let Some(key) = field(data, "toolCallId") else {
                    continue;
                };
                let output = data
                    .and_then(|data| data.pointer("/result/content"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let success = data.and_then(|data| data.get("success")).and_then(Value::as_bool);
                let failed = success == Some(false);
                log.tool_output(at, key, output, failed);
                if let Some(success) = success {
                    log.tool_status(key, if success { "completed" } else { "failed" });
                }
                if let Some(raw) = field(data, "status") {
                    log.tool_status(key, raw);
                }
            }
            Some("session.error") => {
                if let Some(message) = field(data, "message").filter(|message| !message.trim().is_empty()) {
                    log.part(
                        at,
                        ReferencePart::Notice { text: message.to_string(), source: Some("session.error".to_string()) , id: None},
                    );
                }
            }
            _ => {}
        }
    }
    let mut turns = log.finish();
    reference_finalize_work_status(&mut turns, false);
    turns
}

fn field<'a>(data: Option<&'a Value>, key: &str) -> Option<&'a str> {
    data?.get(key)?.as_str()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::reference_chat::types::ReferenceTurnRole;

    /// Redacted copy of a real `~/.copilot/session-state/<uuid>.jsonl` (Copilot CLI 0.0.375).
    const SESSION: &str = include_str!("fixtures/copilot/history/session.jsonl");

    #[test]
    fn copilot_session_parses_prompts_errors_tool_calls_and_answers() {
        let turns = parse_reference_copilot_history(ReferenceNativeHistoryKind::Copilot, SESSION)
            .expect("copilot reads its own store");
        let roles: Vec<ReferenceTurnRole> = turns.iter().map(|turn| turn.role).collect();
        assert_eq!(
            roles,
            [
                ReferenceTurnRole::User,
                ReferenceTurnRole::Assistant,
                ReferenceTurnRole::User,
                ReferenceTurnRole::Assistant,
                ReferenceTurnRole::User,
            ]
        );
        assert_eq!(turns[0].parts, [ReferencePart::Text { text: "run ls".into(), phase: None , id: None}]);
        assert_eq!(turns[0].started_at.as_deref(), Some("2026-10-22T07:03:32.080Z"));
        assert!(matches!(
            &turns[1].parts[..],
            [ReferencePart::Notice { text, source: Some(source) , ..}]
                if text.starts_with("Model call failed") && source == "session.error"
        ));
        let ReferencePart::Tool { name, summary, input, output, error, .. } = &turns[3].parts[0] else {
            panic!("the call comes first: {:?}", turns[3].parts);
        };
        assert_eq!(name, "bash");
        assert_eq!(summary, "List files in the current directory");
        assert!(input.contains("\"command\": \"ls\""), "{input}");
        assert_eq!(output, "README.md\n<exited with exit code 0>");
        assert_eq!(*error, None);
        assert_eq!(
            turns[3].parts[1],
            ReferencePart::Text { text: "README.md is the only file in the current directory.".into(), phase: None , id: None}
        );
        assert_eq!(
            turns[4].parts,
            [ReferencePart::Text { text: "run the shell command: touch probe.txt".into(), phase: None , id: None}]
        );
    }

    #[test]
    fn copilot_turns_open_on_user_messages_only() {
        let opening: Vec<&str> = SESSION.lines().filter(|line| copilot_line_opens_turn(line)).collect();
        assert_eq!(opening.len(), 3);
        assert!(!copilot_line_opens_turn(r#"{"type":"assistant.message","data":{"content":"user.message"}}"#));
    }

    #[test]
    fn copilot_parser_refuses_another_family() {
        assert!(parse_reference_copilot_history(ReferenceNativeHistoryKind::Claude, SESSION).is_err());
        assert!(parse_reference_copilot_history(ReferenceNativeHistoryKind::Unavailable, "").is_err());
    }

    #[test]
    fn native_work_copilot_maps_success_and_cancelled() {
        let text = concat!(
            r#"{"type":"user.message","timestamp":"2026-10-22T07:00:00.000Z","data":{"content":"status failed usage 9"}}"#,
            "\n",
            r#"{"type":"assistant.message","timestamp":"2026-10-22T07:00:01.000Z","data":{"model":"copilot-model","usage":{"input_tokens":1,"output_tokens":2},"toolRequests":[{"toolCallId":"c1","name":"bash","arguments":{"command":"ls"}}]}}"#,
            "\n",
            r#"{"type":"tool.execution_complete","timestamp":"2026-10-22T07:00:02.000Z","data":{"toolCallId":"c1","success":false,"status":"cancelled","result":{"content":"stopped"}}}"#,
            "\n",
        );
        let turns = parse_copilot_transcript(text);
        assert!(turns[0].meta.is_none());
        assert_eq!(turns[1].meta.as_ref().unwrap().model.as_deref(), Some("copilot-model"));
        assert_eq!(turns[1].meta.as_ref().unwrap().usage.as_ref().unwrap().input_tokens, Some(1));
        let ReferencePart::Tool { status, .. } = &turns[1].parts[0] else { panic!("{:?}", turns[1].parts) };
        assert_eq!(*status, Some(crate::remote::reference_chat::types::ReferenceToolStatus::Cancelled));
    }

    fn copilot_call(status: &str) -> String {
        format!(
            "{}\n",
            serde_json::json!({
                "type": "assistant.message",
                "timestamp": "2026-10-22T07:00:01.000Z",
                "data": {
                    "toolRequests": [{
                        "toolCallId": "c-life",
                        "name": "bash",
                        "arguments": { "command": "sleep" },
                        "status": status
                    }]
                }
            })
        )
    }

    fn copilot_status(text: &str) -> Option<crate::remote::reference_chat::types::ReferenceToolStatus> {
        parse_copilot_transcript(text).into_iter().find_map(|turn| {
            turn.parts.into_iter().find_map(|part| match part {
                ReferencePart::Tool { status, .. } => status,
                _ => None,
            })
        })
    }

    #[test]
    fn copilot_unknown_tool_word_is_omitted_not_failed() {
        assert_eq!(copilot_status(&copilot_call("mystery")), None);
    }

    #[test]
    fn copilot_unfinished_tool_in_a_closed_stream_is_unverifiable() {
        let later = format!(
            "{}{}\n",
            copilot_call("running"),
            serde_json::json!({
                "type": "user.message",
                "timestamp": "2026-10-22T07:00:02.000Z",
                "data": { "content": "later" }
            })
        );
        assert_eq!(
            copilot_status(&later),
            Some(crate::remote::reference_chat::types::ReferenceToolStatus::Unverifiable)
        );
    }

    #[test]
    fn copilot_open_tail_tool_is_running_only_on_a_live_owning_tail() {
        let text = copilot_call("running");
        assert_eq!(
            copilot_status(&text),
            Some(crate::remote::reference_chat::types::ReferenceToolStatus::Unverifiable)
        );
        let mut turns = parse_copilot_transcript(&text);
        reference_finalize_work_status(&mut turns, true);
        let ReferencePart::Tool { status, .. } = &turns[0].parts[0] else { panic!("{:?}", turns) };
        assert_eq!(*status, Some(crate::remote::reference_chat::types::ReferenceToolStatus::Running));
    }

    #[test]
    fn copilot_recorded_failure_stays_failed() {
        assert_eq!(
            copilot_status(&copilot_call("failed")),
            Some(crate::remote::reference_chat::types::ReferenceToolStatus::Failed)
        );
    }

    #[test]
    fn copilot_recorded_cancel_stays_cancelled() {
        assert_eq!(
            copilot_status(&copilot_call("cancelled")),
            Some(crate::remote::reference_chat::types::ReferenceToolStatus::Cancelled)
        );
    }
}
