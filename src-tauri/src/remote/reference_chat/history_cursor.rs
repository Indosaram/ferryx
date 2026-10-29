//! Cursor Agent CLI (`cursor-agent`) native history reader (no upstream port; measured on this
//! Mac against cursor-agent 2026.10.19 on an existing chat; new chats were behind a login wall).
//!
//! Store: `~/.cursor/chats/<md5(abs cwd)>/<chat id>/store.db`, a SQLite database opened read-only
//! (`.omo-parity/facts/local-3.md` §2 "Session store" and its corrections table):
//!
//! * `meta` holds one row, key `0`, whose value is hex-encoded JSON
//!   `{agentId, latestRootBlobId, name, mode, …}`;
//! * `blobs(id, data)` is content-addressed (`id` = sha256 of `data`). A blob is a JSON message
//!   `{role, content}` or a protobuf tree node whose field 1 repeats 32-byte child ids. The root
//!   (`latestRootBlobId`) lists the conversation's messages in order.
//!
//! The reader projects the root's field-1 children, one JSON message per line, and parses that.
//! `system` messages and `user` messages that carry no `<user_query>` (the injected `<user_info>`
//! context) are not the conversation. A user query is `<timestamp>…</timestamp>\n<user_query>\n…
//! \n</user_query>`; assistant content is a string or `[{type: "text"|"reasoning", text}]`. Tool
//! calls were never observed (the only local chat has none); see the parity report's UNSETTLED
//! list.

use std::path::Path;

use serde_json::Value;

use super::history_parse::ReferenceTurnLog;
use super::types::{ReferenceHistoryParser, ReferenceNativeHistoryKind, ReferencePart, ReferenceTurn};

/// The lane's parser: cursor-transcript only; any other kind is refused.
pub fn parse_reference_cursor_history(
    kind: ReferenceNativeHistoryKind,
    text: &str,
) -> Result<Vec<ReferenceTurn>, String> {
    if kind != ReferenceNativeHistoryKind::Cursor {
        return Err(format!(
            "the cursor history family reads cursor-transcript only, not {}",
            kind.as_str()
        ));
    }
    Ok(parse_cursor_projection(text))
}

/// Compile-time proof that the lane satisfies the frozen signature.
const _: ReferenceHistoryParser = parse_reference_cursor_history;

/// One chat's messages, in the root tree's order, one JSON line each. Synchronous: the caller
/// runs it off the reactor.
pub fn cursor_store_projection(db: &Path) -> Result<String, String> {
    let connection = rusqlite::Connection::open_with_flags(
        db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| format!("{}: {error}", db.display()))?;
    connection
        .busy_timeout(std::time::Duration::from_millis(500))
        .map_err(|error| error.to_string())?;
    let meta: String = connection
        .query_row("SELECT value FROM meta WHERE key = '0'", [], |row| row.get(0))
        .map_err(|error| format!("the chat has no meta row: {error}"))?;
    let root = cursor_meta_root(&meta).ok_or("the meta row names no latestRootBlobId")?;
    cursor_projection(&root, |id| {
        connection
            .query_row("SELECT data FROM blobs WHERE id = ?1", [id], |row| row.get::<_, Vec<u8>>(0))
            .ok()
    })
}

/// The `latestRootBlobId` of the hex-encoded JSON in `meta` key `0`.
pub fn cursor_meta_root(hex_json: &str) -> Option<String> {
    let bytes = decode_hex(hex_json.trim())?;
    let meta: Value = serde_json::from_slice(&bytes).ok()?;
    meta.get("latestRootBlobId")?.as_str().map(str::to_string)
}

/// Walk the root's field-1 children through `blob` and emit each JSON message as one line. A
/// child that is missing or not a JSON message is skipped; a missing root is an error.
pub fn cursor_projection(root: &str, blob: impl Fn(&str) -> Option<Vec<u8>>) -> Result<String, String> {
    let node = blob(root).ok_or_else(|| format!("the root blob {root} is missing"))?;
    let children = tree_children(&node).ok_or("the root blob is not a tree node")?;
    let mut projection = String::new();
    for child in children {
        let Some(data) = blob(&child) else {
            continue;
        };
        let Ok(message) = serde_json::from_slice::<Value>(&data) else {
            continue;
        };
        if message.get("role").and_then(Value::as_str).is_none() {
            continue;
        }
        projection.push_str(&message.to_string());
        projection.push('\n');
    }
    Ok(projection)
}

/// A tree node's field-1 entries that are 32-byte ids, as lowercase hex, in order. `None` when
/// the bytes are not a well-formed protobuf message.
fn tree_children(node: &[u8]) -> Option<Vec<String>> {
    Some(
        length_delimited_fields(node)?
            .into_iter()
            .filter(|(field, value)| *field == 1 && value.len() == 32)
            .map(|(_, value)| value.iter().map(|byte| format!("{byte:02x}")).collect())
            .collect(),
    )
}

/// The workspace a root tree node names: field 21 is a nested message whose field 1 is the raw
/// absolute path (measured on both local stores, beside field 9's `file://` URI of the same path).
/// `None` when the node is torn or names no absolute path.
pub fn cursor_root_workspace(node: &[u8]) -> Option<String> {
    let (_, workspace) = length_delimited_fields(node)?.into_iter().find(|(field, _)| *field == 21)?;
    let (_, path) = length_delimited_fields(workspace)?.into_iter().find(|(field, _)| *field == 1)?;
    String::from_utf8(path.to_vec()).ok().filter(|path| path.starts_with('/'))
}

/// Every length-delimited `(field, bytes)` entry of a protobuf message, in order; other wire
/// types are skipped. `None` when the bytes are not a well-formed protobuf message.
fn length_delimited_fields(node: &[u8]) -> Option<Vec<(u64, &[u8])>> {
    let mut fields = Vec::new();
    let mut at = 0usize;
    while at < node.len() {
        let tag = read_varint(node, &mut at)?;
        let (field, wire) = (tag >> 3, tag & 7);
        match wire {
            0 => {
                read_varint(node, &mut at)?;
            }
            1 => at = at.checked_add(8).filter(|end| *end <= node.len())?,
            2 => {
                let length = usize::try_from(read_varint(node, &mut at)?).ok()?;
                let end = at.checked_add(length).filter(|end| *end <= node.len())?;
                fields.push((field, &node[at..end]));
                at = end;
            }
            5 => at = at.checked_add(4).filter(|end| *end <= node.len())?,
            _ => return None,
        }
    }
    Some(fields)
}

fn read_varint(bytes: &[u8], at: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    for shift in (0..64).step_by(7) {
        let byte = *bytes.get(*at)?;
        *at += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

fn decode_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(text.get(index..index + 2)?, 16).ok())
        .collect()
}

/// A page boundary: a user message that carries a `<user_query>`.
pub fn cursor_line_opens_turn(line: &str) -> bool {
    line.contains("user_query")
        && serde_json::from_str::<Value>(line).is_ok_and(|message| user_query(&message).is_some())
}

/// Split a projection into turns. A torn or unparseable line is skipped.
pub fn parse_cursor_projection(text: &str) -> Vec<ReferenceTurn> {
    let mut log = ReferenceTurnLog::default();
    for line in text.split('\n') {
        let Ok(message) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        match message.get("role").and_then(Value::as_str) {
            Some("user") => {
                if let Some(query) = user_query(&message) {
                    log.user(None, None, vec![ReferencePart::Text { text: query, phase: None }]);
                }
            }
            Some("assistant") => match message.get("content") {
                Some(Value::String(text)) if !text.is_empty() => {
                    log.part(None, ReferencePart::Text { text: text.clone(), phase: None });
                }
                Some(Value::Array(blocks)) => {
                    for block in blocks {
                        let text = block.get("text").and_then(Value::as_str).unwrap_or("");
                        if text.is_empty() {
                            continue;
                        }
                        match block.get("type").and_then(Value::as_str) {
                            Some("text") => log.part(None, ReferencePart::Text { text: text.to_string(), phase: None }),
                            Some("reasoning") => log.part(None, ReferencePart::Thinking { text: text.to_string() }),
                            _ => {}
                        }
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
    log.finish()
}

/// The text inside a user message's `<user_query>` envelope, or `None` for context-only messages.
pub fn user_query(message: &Value) -> Option<String> {
    if message.get("role").and_then(Value::as_str) != Some("user") {
        return None;
    }
    let texts: Vec<&str> = match message.get("content")? {
        Value::String(text) => vec![text.as_str()],
        Value::Array(blocks) => blocks
            .iter()
            .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect(),
        _ => return None,
    };
    texts.into_iter().find_map(|text| {
        let (_, rest) = text.split_once("<user_query>")?;
        let (query, _) = rest.split_once("</user_query>")?;
        let query = query.trim_matches('\n');
        (!query.trim().is_empty()).then(|| query.to_string())
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::remote::reference_chat::types::ReferenceTurnRole;

    /// Every `meta` and `blobs` row of a real `~/.cursor/chats/<hash>/<id>/store.db`, one per line.
    /// Message blobs keep their original ids with redacted bodies (the reader looks blobs up by id
    /// and never re-hashes them); tree-node blobs are verbatim hex.
    const ROWS: &str = include_str!("fixtures/cursor/history/store-rows.jsonl");

    fn store() -> (String, HashMap<String, Vec<u8>>) {
        let mut meta = String::new();
        let mut blobs = HashMap::new();
        for line in ROWS.lines() {
            let row: Value = serde_json::from_str(line).expect("a fixture row is JSON");
            match row["table"].as_str() {
                Some("meta") => meta = row["value"].as_str().expect("value").to_string(),
                Some("blobs") => {
                    let id = row["id"].as_str().expect("id").to_string();
                    let data = match (row.get("json"), row.get("hex")) {
                        (Some(message), _) => message.to_string().into_bytes(),
                        (None, Some(Value::String(hex))) => decode_hex(hex).expect("hex"),
                        other => panic!("unexpected blob {other:?}"),
                    };
                    blobs.insert(id, data);
                }
                other => panic!("unexpected table {other:?}"),
            }
        }
        (meta, blobs)
    }

    fn projection() -> String {
        let (meta, blobs) = store();
        let root = cursor_meta_root(&meta).expect("meta names the root");
        assert_eq!(root, "377ad79c140c48a6cb42caa33e9a93a57f9f52306f41cd611f69d1e7134f6107");
        cursor_projection(&root, |id| blobs.get(id).cloned()).expect("the root is a tree")
    }

    #[test]
    fn cursor_chat_walks_the_root_tree_and_keeps_only_the_conversation() {
        let projection = projection();
        assert_eq!(projection.lines().count(), 4, "system, context, query and answer, in order");
        let turns = parse_reference_cursor_history(ReferenceNativeHistoryKind::Cursor, &projection)
            .expect("cursor reads its own store");
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].role, ReferenceTurnRole::User);
        assert_eq!(turns[0].parts, [ReferencePart::Text { text: "hi".into(), phase: None }]);
        assert_eq!(turns[1].role, ReferenceTurnRole::Assistant);
        assert_eq!(
            turns[1].parts,
            [ReferencePart::Text {
                text: "Hi! What are you working on in `opencode-inflearn`, and what would you like me to help with?".into(),
                phase: None,
            }]
        );
    }

    #[test]
    fn cursor_store_is_read_read_only_from_sqlite() {
        let (meta, blobs) = store();
        let dir = std::env::temp_dir().join(format!("ferryx-cursor-history-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("a temp dir");
        let db = dir.join("store.db");
        {
            let connection = rusqlite::Connection::open(&db).expect("a scratch store");
            connection
                .execute_batch(
                    "CREATE TABLE blobs (id TEXT PRIMARY KEY, data BLOB);
                     CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT);",
                )
                .expect("the measured schema");
            connection.execute("INSERT INTO meta VALUES ('0', ?1)", [&meta]).expect("meta");
            for (id, data) in &blobs {
                connection.execute("INSERT INTO blobs VALUES (?1, ?2)", rusqlite::params![id, data]).expect("blob");
            }
        }
        assert_eq!(cursor_store_projection(&db), Ok(projection()));
        assert!(cursor_store_projection(&dir.join("missing.db")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cursor_turns_open_on_user_queries_only() {
        let projection = projection();
        assert_eq!(projection.lines().filter(|line| cursor_line_opens_turn(line)).count(), 1);
        assert!(!cursor_line_opens_turn(r#"{"role":"assistant","content":"<user_query>x</user_query>"}"#));
        assert_eq!(tree_children(&[0x0a, 0x05, 0x01]), None, "a torn node is not a tree");
    }

    #[test]
    fn cursor_root_names_its_workspace_path() {
        let (meta, blobs) = store();
        let root = cursor_meta_root(&meta).expect("meta names the root");
        assert_eq!(
            cursor_root_workspace(&blobs[&root]).as_deref(),
            Some("/Users/user/code/project/opencode-inflearn"),
            "field 21.1 of the root node is the chat's workspace"
        );
        assert_eq!(cursor_root_workspace(&[0x0a, 0x05, 0x01]), None, "a torn node names nothing");
    }

    #[test]
    fn cursor_parser_refuses_another_family() {
        assert!(parse_reference_cursor_history(ReferenceNativeHistoryKind::Opencode, "").is_err());
        assert!(parse_reference_cursor_history(ReferenceNativeHistoryKind::Unavailable, "").is_err());
    }
}
