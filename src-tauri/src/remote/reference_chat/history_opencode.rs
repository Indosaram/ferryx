//! OpenCode native history reader (no upstream port; measured on this Mac).
//!
//! Store: the shared SQLite database `~/.local/share/opencode/opencode.db`, opened read-only. A
//! session's conversation is its `message` rows (`id, session_id, time_created, data`, `data` a
//! JSON object with `role`) and their `part` rows (`id, message_id, session_id, data`, `data` a
//! JSON object with `type`). Measured part types: `text` (`text`), `tool` (`tool`, `callID`,
//! `state{status, input, output}`), `step-start`, `step-finish` and `patch` (bookkeeping).
//!
//! Rows are not bytes, so the history layer asks [`opencode_store_projection`] for one compact JSON
//! line per message — `{id, time_created, info, parts}` in the store's own order (`message` by
//! `time_created, id`; `part` by `message_id, id`, the store's `part_message_id_id_idx`) — and
//! pages that projection like any other stream.

use std::path::Path;

use serde_json::{json, Value};

use super::history_parse::{
    reference_timestamp, reference_tool_input, reference_tool_summary, ReferenceTurnLog,
};
use super::types::{ReferenceHistoryParser, ReferenceNativeHistoryKind, ReferencePart, ReferenceTurn};

/// The lane's parser: opencode-transcript only; any other kind is refused.
pub fn parse_reference_opencode_history(
    kind: ReferenceNativeHistoryKind,
    text: &str,
) -> Result<Vec<ReferenceTurn>, String> {
    if kind != ReferenceNativeHistoryKind::Opencode {
        return Err(format!(
            "the opencode history family reads opencode-transcript only, not {}",
            kind.as_str()
        ));
    }
    Ok(parse_opencode_projection(text))
}

/// Compile-time proof that the lane satisfies the frozen signature.
const _: ReferenceHistoryParser = parse_reference_opencode_history;

/// Does the store hold this exact session? Synchronous: the caller runs it off the reactor.
pub fn opencode_store_has_session(db: &Path, session_id: &str) -> Result<bool, String> {
    let connection = open_read_only(db)?;
    connection
        .query_row("SELECT 1 FROM session WHERE id = ?1", [session_id], |_| Ok(()))
        .map(|()| true)
        .or_else(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => Ok(false),
            error => Err(error.to_string()),
        })
}

/// One session's rows as the projection this reader parses. Synchronous: the caller runs it off
/// the reactor.
pub fn opencode_store_projection(db: &Path, session_id: &str) -> Result<String, String> {
    let connection = open_read_only(db)?;
    let mut message_rows = connection
        .prepare("SELECT id, time_created, data FROM message WHERE session_id = ?1 ORDER BY time_created, id")
        .map_err(|error| error.to_string())?;
    let messages = message_rows
        .query_map([session_id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?)))
        .and_then(Iterator::collect::<Result<Vec<_>, _>>)
        .map_err(|error| error.to_string())?;
    let mut part_rows = connection
        .prepare("SELECT message_id, data FROM part WHERE session_id = ?1 ORDER BY message_id, id")
        .map_err(|error| error.to_string())?;
    let parts = part_rows
        .query_map([session_id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        .and_then(Iterator::collect::<Result<Vec<_>, _>>)
        .map_err(|error| error.to_string())?;
    Ok(opencode_projection(&messages, &parts))
}

fn open_read_only(db: &Path) -> Result<rusqlite::Connection, String> {
    let connection = rusqlite::Connection::open_with_flags(
        db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| format!("{}: {error}", db.display()))?;
    connection
        .busy_timeout(std::time::Duration::from_millis(500))
        .map_err(|error| error.to_string())?;
    Ok(connection)
}

/// Rows (`(id, time_created, data)` messages in order; `(message_id, data)` parts in order) as
/// one JSON line per message. A row whose `data` is not JSON is skipped.
pub fn opencode_projection(messages: &[(String, i64, String)], parts: &[(String, String)]) -> String {
    let mut projection = String::new();
    for (id, time_created, data) in messages {
        let Ok(info) = serde_json::from_str::<Value>(data) else {
            continue;
        };
        let own: Vec<Value> = parts
            .iter()
            .filter(|(message_id, _)| message_id == id)
            .filter_map(|(_, data)| serde_json::from_str::<Value>(data).ok())
            .collect();
        let line = json!({ "id": id, "time_created": time_created, "info": info, "parts": own });
        projection.push_str(&line.to_string());
        projection.push('\n');
    }
    projection
}

/// A page boundary: a user message that carries a text part.
pub fn opencode_line_opens_turn(line: &str) -> bool {
    line.contains("\"user\"")
        && serde_json::from_str::<Value>(line).is_ok_and(|message| {
            message.pointer("/info/role").and_then(Value::as_str) == Some("user")
                && parts(&message).any(|part| part.get("type").and_then(Value::as_str) == Some("text"))
        })
}

/// Split a projection into turns. A torn or unparseable line is skipped.
pub fn parse_opencode_projection(text: &str) -> Vec<ReferenceTurn> {
    let mut log = ReferenceTurnLog::default();
    for line in text.split('\n') {
        let Ok(message) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        let at = reference_timestamp(message.get("time_created"), 1.0);
        let at = at.as_deref();
        match message.pointer("/info/role").and_then(Value::as_str) {
            Some("user") => {
                let prompt: Vec<&str> = parts(&message)
                    .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
                    .filter_map(|part| part.get("text").and_then(Value::as_str))
                    .filter(|text| !text.trim().is_empty())
                    .collect();
                if !prompt.is_empty() {
                    log.user(
                        at.map(str::to_string),
                        None,
                        vec![ReferencePart::Text { text: prompt.join("\n"), phase: None }],
                    );
                }
            }
            Some("assistant") => {
                for part in parts(&message) {
                    match part.get("type").and_then(Value::as_str) {
                        Some("text") => {
                            if let Some(text) = part.get("text").and_then(Value::as_str).filter(|text| !text.is_empty()) {
                                log.part(at, ReferencePart::Text { text: text.to_string(), phase: None });
                            }
                        }
                        Some("tool") => {
                            let Some(name) = part.get("tool").and_then(Value::as_str) else {
                                continue;
                            };
                            let key = part.get("callID").and_then(Value::as_str).unwrap_or("");
                            let input = part.pointer("/state/input");
                            log.tool(at, key, name, reference_tool_summary(input), reference_tool_input(input));
                            if let Some(output) = part.pointer("/state/output").and_then(Value::as_str) {
                                log.tool_output(at, key, output.to_string(), false);
                            }
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

fn parts(message: &Value) -> impl Iterator<Item = &Value> {
    message.get("parts").and_then(Value::as_array).into_iter().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::reference_chat::types::ReferenceTurnRole;

    /// The `message` and `part` rows of a real session in `~/.local/share/opencode/opencode.db`,
    /// one row per line (`{"table": …, …}`), with the per-message diff summaries left out.
    const ROWS: &str = include_str!("fixtures/opencode/history/session-rows.jsonl");
    const SESSION: &str = "ses_e9d886f2bffeJp9DsEMPfnB25I";

    type MessageRow = (String, i64, String);
    type PartRow = (String, String);

    fn rows() -> (Vec<MessageRow>, Vec<PartRow>) {
        let mut messages = Vec::new();
        let mut parts = Vec::new();
        for line in ROWS.lines() {
            let row: Value = serde_json::from_str(line).expect("a fixture row is JSON");
            let data = row["data"].to_string();
            match row["table"].as_str() {
                Some("message") => messages.push((
                    row["id"].as_str().expect("id").to_string(),
                    row["time_created"].as_i64().expect("time"),
                    data,
                )),
                Some("part") => parts.push((row["message_id"].as_str().expect("message id").to_string(), data)),
                other => panic!("unexpected table {other:?}"),
            }
        }
        (messages, parts)
    }

    #[test]
    fn opencode_session_parses_prompts_tool_calls_and_answers() {
        let (messages, parts) = rows();
        let turns = parse_reference_opencode_history(
            ReferenceNativeHistoryKind::Opencode,
            &opencode_projection(&messages, &parts),
        )
        .expect("opencode reads its own store");
        assert_eq!(turns.len(), 4);
        assert_eq!(turns[0].role, ReferenceTurnRole::User);
        assert_eq!(turns[0].parts, [ReferencePart::Text { text: "run ls".into(), phase: None }]);
        assert_eq!(turns[0].started_at.as_deref(), Some("2026-10-22T07:16:05.533Z"));
        let ReferencePart::Tool { name, input, output, .. } = &turns[1].parts[0] else {
            panic!("the call comes first: {:?}", turns[1].parts);
        };
        assert_eq!(name, "bash");
        assert!(input.contains("\"command\": \"ls\""), "{input}");
        assert_eq!(output, "README.md\n");
        assert_eq!(turns[1].parts[1], ReferencePart::Text { text: "README.md".into(), phase: None });
        assert_eq!(
            turns[3].parts.last(),
            Some(&ReferencePart::Text { text: "Created `created.txt`.".into(), phase: None })
        );
    }

    #[test]
    fn opencode_store_is_read_read_only_from_sqlite_rows() {
        let (messages, parts) = rows();
        let dir = std::env::temp_dir().join(format!("ferryx-opencode-history-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("a temp dir");
        let db = dir.join("opencode.db");
        {
            let connection = rusqlite::Connection::open(&db).expect("a scratch store");
            connection
                .execute_batch(
                    "CREATE TABLE session (id TEXT PRIMARY KEY);
                     CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, data TEXT);
                     CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT, session_id TEXT, data TEXT);",
                )
                .expect("the measured schema subset");
            connection.execute("INSERT INTO session (id) VALUES (?1)", [SESSION]).expect("session");
            for (id, time, data) in &messages {
                connection
                    .execute(
                        "INSERT INTO message VALUES (?1, ?2, ?3, ?4)",
                        rusqlite::params![id, SESSION, time, data],
                    )
                    .expect("message");
            }
            // Parts go in out of order: the projection orders them itself.
            for (index, (message_id, data)) in parts.iter().enumerate().rev() {
                connection
                    .execute(
                        "INSERT INTO part VALUES (?1, ?2, ?3, ?4)",
                        rusqlite::params![format!("prt_{index:04}"), message_id, SESSION, data],
                    )
                    .expect("part");
            }
            // Another session in the same store never leaks into this one's projection.
            connection
                .execute(
                    "INSERT INTO message VALUES ('msg_other', 'ses_other', 1, '{\"role\":\"user\"}')",
                    [],
                )
                .expect("foreign message");
        }
        assert_eq!(opencode_store_has_session(&db, SESSION), Ok(true));
        assert_eq!(opencode_store_has_session(&db, "ses_missing"), Ok(false));
        let projection = opencode_store_projection(&db, SESSION).expect("the store reads");
        assert_eq!(projection, opencode_projection(&messages, &parts));
        assert!(!projection.contains("msg_other"));
        assert!(opencode_store_projection(&dir.join("missing.db"), SESSION).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn opencode_turns_open_on_user_text_only() {
        let (messages, parts) = rows();
        let projection = opencode_projection(&messages, &parts);
        assert_eq!(projection.lines().filter(|line| opencode_line_opens_turn(line)).count(), 2);
    }

    #[test]
    fn opencode_parser_refuses_another_family() {
        assert!(parse_reference_opencode_history(ReferenceNativeHistoryKind::Cline, "").is_err());
        assert!(parse_reference_opencode_history(ReferenceNativeHistoryKind::Unavailable, "").is_err());
    }
}
