//! Crush (charmbracelet/crush) native history reader (no upstream port; measured against crush
//! v0.98.1 on omarchy, `.omo-parity/facts/remote-crush.md` §5).
//!
//! Store: a per-project SQLite database `<project>/.crush/crush.db`, opened read-only. Every
//! project crush has opened is listed with its absolute `data_dir` in the global index
//! `~/.local/share/crush/projects.json` (`{"projects":[{"path","data_dir","last_accessed"}]}`).
//! A session is a `sessions` row keyed by its UUID; its conversation is its `messages` rows
//! (`id, session_id, role, parts, created_at` in unix seconds). `parts` is a JSON array of
//! `{"type", "data"}` (crush `internal/message/message.go:613-628` @ `8b18242`): `text`
//! (`text`, `hidden`), `reasoning` (`thinking`), `tool_call` (`id`, `name`, `input` as a
//! JSON string), `tool_result` (`tool_call_id`, `content`, `is_error`) on a `tool` row, and
//! `finish` bookkeeping.
//!
//! Rows are not bytes, so the history layer asks [`crush_store_projection`] for one JSON line per
//! message — `{id, created_at, role, parts}` in `created_at, rowid` order — and pages that.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::history_parse::{reference_timestamp, reference_tool_input, reference_tool_summary, ReferenceTurnLog};
use super::types::{ReferenceHistoryParser, ReferenceNativeHistoryKind, ReferencePart, ReferenceTurn};

/// The lane's parser: crush-transcript only; any other kind is refused.
pub fn parse_reference_crush_history(
    kind: ReferenceNativeHistoryKind,
    text: &str,
) -> Result<Vec<ReferenceTurn>, String> {
    if kind != ReferenceNativeHistoryKind::Crush {
        return Err(format!(
            "the crush history family reads crush-transcript only, not {}",
            kind.as_str()
        ));
    }
    Ok(parse_crush_projection(text))
}

/// Compile-time proof that the lane satisfies the frozen signature.
const _: ReferenceHistoryParser = parse_reference_crush_history;

/// The project databases the global index names, in index order. A missing or unreadable index
/// names none. Synchronous: the caller runs it off the reactor.
pub fn crush_indexed_stores(home: &Path) -> Vec<PathBuf> {
    crush_indexed_stores_in(&home.join(".local").join("share").join("crush"))
}

/// [`crush_indexed_stores`] for an already-resolved crush global data dir (the dir holding
/// `projects.json`; see `crate::ipc::agent_history::history_root_for`).
pub fn crush_indexed_stores_in(crush_root: &Path) -> Vec<PathBuf> {
    let index = crush_root.join("projects.json");
    let Ok(text) = std::fs::read_to_string(index) else {
        return Vec::new();
    };
    let Ok(index) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    index
        .get("projects")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|project| project.get("data_dir").and_then(Value::as_str))
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join("crush.db"))
        .filter(|db| db.is_file())
        .collect()
}

/// Does this project database hold this exact session (its UUID)? Synchronous.
pub fn crush_store_has_session(db: &Path, session_id: &str) -> Result<bool, String> {
    let connection = open_read_only(db)?;
    connection
        .query_row("SELECT 1 FROM sessions WHERE id = ?1", [session_id], |_| Ok(()))
        .map(|()| true)
        .or_else(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => Ok(false),
            error => Err(error.to_string()),
        })
}

/// One session's messages as the projection this reader parses. Synchronous.
pub fn crush_store_projection(db: &Path, session_id: &str) -> Result<String, String> {
    let connection = open_read_only(db)?;
    let mut statement = connection
        .prepare("SELECT id, role, parts, created_at FROM messages WHERE session_id = ?1 ORDER BY created_at, rowid")
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([session_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, i64>(3)?))
        })
        .and_then(Iterator::collect::<Result<Vec<_>, _>>)
        .map_err(|error| error.to_string())?;
    Ok(crush_projection(&rows))
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

/// `(id, role, parts, created_at)` rows as one JSON line per message. A row whose `parts` is not
/// JSON is skipped.
pub fn crush_projection(rows: &[(String, String, String, i64)]) -> String {
    let mut projection = String::new();
    for (id, role, parts, created_at) in rows {
        let Ok(parts) = serde_json::from_str::<Value>(parts) else {
            continue;
        };
        let line = json!({ "id": id, "created_at": created_at, "role": role, "parts": parts });
        projection.push_str(&line.to_string());
        projection.push('\n');
    }
    projection
}

/// A page boundary: a user message that carries visible text.
pub fn crush_line_opens_turn(line: &str) -> bool {
    line.contains("\"user\"")
        && serde_json::from_str::<Value>(line).is_ok_and(|message| {
            message.get("role").and_then(Value::as_str) == Some("user") && !user_text(&message).is_empty()
        })
}

/// Split a projection into turns. A torn or unparseable line is skipped.
pub fn parse_crush_projection(text: &str) -> Vec<ReferenceTurn> {
    let mut log = ReferenceTurnLog::default();
    for line in text.split('\n') {
        let Ok(message) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        let at = reference_timestamp(message.get("created_at"), 1000.0);
        let at = at.as_deref();
        match message.get("role").and_then(Value::as_str) {
            Some("user") => {
                let prompt = user_text(&message);
                if !prompt.is_empty() {
                    log.user(
                        at.map(str::to_string),
                        None,
                        vec![ReferencePart::Text { text: prompt.join("\n"), phase: None }],
                    );
                }
            }
            Some("assistant") => {
                for (kind, data) in parts(&message) {
                    match kind {
                        "text" => log.stream_text(at, string(data, "text"), false),
                        "reasoning" => log.stream_text(at, string(data, "thinking"), true),
                        "tool_call" => {
                            let name = string(data, "name");
                            if name.is_empty() {
                                continue;
                            }
                            let input = serde_json::from_str::<Value>(string(data, "input")).ok();
                            log.tool(
                                at,
                                string(data, "id"),
                                name,
                                reference_tool_summary(input.as_ref()),
                                reference_tool_input(input.as_ref()),
                            );
                        }
                        _ => {}
                    }
                }
            }
            Some("tool") => {
                for (kind, data) in parts(&message) {
                    if kind == "tool_result" {
                        let failed = data.get("is_error").and_then(Value::as_bool) == Some(true);
                        log.tool_output(at, string(data, "tool_call_id"), string(data, "content").to_string(), failed);
                    }
                }
            }
            _ => {}
        }
    }
    log.finish()
}

/// A user message's visible prompt: its non-hidden, non-blank text parts.
fn user_text(message: &Value) -> Vec<&str> {
    parts(message)
        .filter(|(kind, data)| *kind == "text" && data.get("hidden").and_then(Value::as_bool) != Some(true))
        .map(|(_, data)| string(data, "text"))
        .filter(|text| !text.trim().is_empty())
        .collect()
}

fn parts(message: &Value) -> impl Iterator<Item = (&str, &Value)> {
    message
        .get("parts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|part| Some((part.get("type")?.as_str()?, part.get("data")?)))
}

fn string<'a>(data: &'a Value, key: &str) -> &'a str {
    data.get(key).and_then(Value::as_str).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::reference_chat::types::ReferenceTurnRole;

    /// The `messages` rows of the measured session in omarchy's `.crush/crush.db` (crush v0.98.1),
    /// one row per line, home path redacted and the hook metadata trimmed.
    const ROWS: &str = include_str!("fixtures/crush/history/session-rows.jsonl");
    const SESSION: &str = "3df112d9-e56f-4f69-b8bb-c58b5d580e89";

    fn rows() -> Vec<(String, String, String, i64)> {
        ROWS.lines()
            .map(|line| {
                let row: Value = serde_json::from_str(line).expect("a fixture row is JSON");
                (
                    row["id"].as_str().expect("id").to_string(),
                    row["role"].as_str().expect("role").to_string(),
                    row["parts"].as_str().expect("parts").to_string(),
                    row["created_at"].as_i64().expect("created_at"),
                )
            })
            .collect()
    }

    #[test]
    fn crush_session_parses_prompts_tool_calls_and_answers() {
        let turns = parse_reference_crush_history(ReferenceNativeHistoryKind::Crush, &crush_projection(&rows()))
            .expect("crush reads its own store");
        let roles: Vec<ReferenceTurnRole> = turns.iter().map(|turn| turn.role).collect();
        assert_eq!(
            roles,
            [ReferenceTurnRole::User, ReferenceTurnRole::Assistant, ReferenceTurnRole::User, ReferenceTurnRole::Assistant]
        );
        assert_eq!(turns[0].parts, [ReferencePart::Text { text: "SLOW please answer".into(), phase: None }]);
        assert_eq!(turns[0].started_at.as_deref(), Some("2026-10-22T07:06:02.000Z"));
        assert_eq!(turns[1].parts, [ReferencePart::Text { text: "Mock reply done.".into(), phase: None }]);
        assert_eq!(turns[2].parts, [ReferencePart::Text { text: "RUNCMD create the probe file".into(), phase: None }]);
        let ReferencePart::Tool { name, input, output, error, .. } = &turns[3].parts[0] else {
            panic!("the call comes first: {:?}", turns[3].parts);
        };
        assert_eq!(name, "bash");
        assert!(input.contains("touch parity-probe.txt"), "{input}");
        assert_eq!(output, "no output");
        assert_eq!(*error, None);
        assert_eq!(turns[3].parts[1], ReferencePart::Text { text: "Mock reply done.".into(), phase: None });
    }

    #[test]
    fn crush_store_is_found_through_the_index_and_read_read_only() {
        let home = std::env::temp_dir().join(format!("ferryx-crush-history-{}", uuid::Uuid::new_v4()));
        let data_dir = home.join("proj").join(".crush");
        std::fs::create_dir_all(&data_dir).expect("a data dir");
        let index_dir = home.join(".local").join("share").join("crush");
        std::fs::create_dir_all(&index_dir).expect("an index dir");
        std::fs::write(
            index_dir.join("projects.json"),
            json!({"projects": [{"path": home.join("proj"), "data_dir": data_dir, "last_accessed": "2026-10-22T07:07:16Z"}]})
                .to_string(),
        )
        .expect("the index");
        let db = data_dir.join("crush.db");
        let rows = rows();
        {
            let connection = rusqlite::Connection::open(&db).expect("a scratch store");
            connection
                .execute_batch(
                    "CREATE TABLE sessions (id TEXT PRIMARY KEY, parent_session_id TEXT, title TEXT NOT NULL);
                     CREATE TABLE messages (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, role TEXT NOT NULL,
                                            parts TEXT NOT NULL, created_at INTEGER NOT NULL);",
                )
                .expect("the measured schema subset");
            connection
                .execute("INSERT INTO sessions VALUES (?1, NULL, 'Mock reply done.')", [SESSION])
                .expect("session");
            for (id, role, parts, created_at) in &rows {
                connection
                    .execute(
                        "INSERT INTO messages VALUES (?1, ?2, ?3, ?4, ?5)",
                        rusqlite::params![id, SESSION, role, parts, created_at],
                    )
                    .expect("message");
            }
            connection
                .execute("INSERT INTO messages VALUES ('m-other', 'other', 'user', '[]', 1)", [])
                .expect("foreign message");
        }
        assert_eq!(crush_indexed_stores(&home), vec![db.clone()]);
        assert_eq!(crush_store_has_session(&db, SESSION), Ok(true));
        // The CLI-facing XXH3 hash is not the row key; only the UUID names the session.
        assert_eq!(crush_store_has_session(&db, "7f7fbfa62dd136ec"), Ok(false));
        let projection = crush_store_projection(&db, SESSION).expect("the store reads");
        assert_eq!(projection, crush_projection(&rows));
        assert!(!projection.contains("m-other"));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn crush_turns_open_on_user_text_only() {
        let projection = crush_projection(&rows());
        assert_eq!(projection.lines().filter(|line| crush_line_opens_turn(line)).count(), 2);
        assert!(!crush_line_opens_turn(
            r#"{"role":"user","parts":[{"type":"text","data":{"text":"continue","hidden":true}}]}"#
        ));
    }

    #[test]
    fn crush_parser_refuses_another_family() {
        assert!(parse_reference_crush_history(ReferenceNativeHistoryKind::Opencode, "").is_err());
        assert!(parse_reference_crush_history(ReferenceNativeHistoryKind::Unavailable, "").is_err());
    }
}
