//! SQLite-backed stores: opencode, crush, cursor and the antigravity conversation index.
//! Every connection is opened read-only with no mutex; these stores belong to the CLI that wrote
//! them and Ferryx must never author a page into them.
use super::*;
use rusqlite::{Connection, OpenFlags};

fn open(root: &Path, path: &Path) -> Result<Connection> {
    guard(root, path)?;
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(|_| HistoryError::Io)
}

/// Rows are not bytes on disk, so the fingerprint that `read` and `resume` re-check is taken over
/// the extracted row text instead of the database file (whose pages churn on every VACUUM).
fn row_digest(provider: CanonicalProvider, locator: &str, rows: &[String]) -> (usize, String) {
    let joined = format!("{}:{locator}:{}", provider_wire(provider), rows.join("\u{1e}"));
    (joined.len(), digest(joined.as_bytes()))
}

fn locator(cand: &Candidate) -> Result<&str> { cand.locator.as_deref().ok_or(HistoryError::InvalidRequest) }

/// `~/.local/share/opencode/opencode.db`: `session(id, parent_id, directory, version, ...)` with
/// `message(id, session_id, time_created, data JSON {role})` and
/// `part(id, message_id, session_id, data JSON {type:"text", text})`.
/// Evidence: `.omo-parity/facts/local-1.md:248-263`.
pub(super) fn opencode_sources(root: &Path, out: &mut Vec<Candidate>, warnings: &mut Vec<String>) {
    let path = root.join("opencode.db");
    let Ok(connection) = open(root, &path) else { warnings.push("ROOT_UNAVAILABLE".into()); return };
    let Ok(mut statement) = connection.prepare("SELECT id FROM session ORDER BY id") else { warnings.push("SCHEMA_UNAVAILABLE".into()); return };
    let Ok(ids) = statement.query_map([], |row| row.get::<_, String>(0)) else { warnings.push("SCHEMA_UNAVAILABLE".into()); return };
    for id in ids.flatten().take(MAX_FILES) { out.push(Candidate { root: root.into(), path: path.clone(), locator: Some(id), context: None }); }
}

pub(super) fn opencode(cand: &Candidate) -> Result<Decoded> {
    let session = locator(cand)?;
    let connection = open(&cand.root, &cand.path)?;
    let (id, parent_id, directory, version) = connection
        .query_row("SELECT id, parent_id, directory, version FROM session WHERE id = ?1", [session], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?, row.get::<_, String>(2)?, row.get::<_, Option<String>>(3)?))
        })
        .map_err(|_| HistoryError::NotFound)?;
    let (id, cwd) = (checked_id(&id)?, checked_cwd(&directory)?);
    let mut statement = connection
        .prepare("SELECT m.id, m.data, p.data FROM message m JOIN part p ON p.message_id = m.id WHERE m.session_id = ?1 ORDER BY m.time_created, m.id, p.time_created, p.id")
        .map_err(|_| HistoryError::Io)?;
    let rows = statement
        .query_map([session], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)))
        .map_err(|_| HistoryError::Io)?;
    let mut fingerprint = vec![directory.clone(), version.clone().unwrap_or_default(), parent_id.clone().unwrap_or_default()];
    let mut messages = vec![];
    for (ordinal, row) in rows.flatten().enumerate() {
        let (message_id, message_data, part_data) = row;
        fingerprint.push(format!("{message_id}|{message_data}|{part_data}"));
        let message: Value = serde_json::from_str(&message_data).map_err(|_| HistoryError::InvalidRequest)?;
        let part: Value = serde_json::from_str(&part_data).map_err(|_| HistoryError::InvalidRequest)?;
        // Role lives on the message row; the rendered text lives on its `text` parts.
        if part["type"] != "text" { continue; }
        let Some(text) = part["text"].as_str().filter(|text| !text.is_empty()) else { continue };
        messages.push(Message { ordinal, role: field(&message, "role").unwrap_or_default(), text: text.to_owned(), id: Some(message_id), parent_id: field(&message, "parentID") });
    }
    let (used, hash) = row_digest(CanonicalProvider::Opencode, session, &fingerprint);
    Ok(Decoded { used, hash, entry: make_entry(CanonicalProvider::Opencode, id, cwd, version, parent_id, None), messages, warnings: vec![] })
}

/// crush keeps one database per project (`<project>/.crush/crush.db`) and an index of them at
/// `~/.local/share/crush/projects.json` `{"projects":[{"path","data_dir","last_accessed"}]}`.
/// Each indexed data directory is treated as its own containment root, so the symlink and
/// traversal guards still apply to a database that lives outside the configured root.
/// Evidence: `.omo-parity/facts/remote-crush.md:109-117`.
pub(super) fn crush_sources(root: &Path, out: &mut Vec<Candidate>, warnings: &mut Vec<String>) {
    let Ok(bytes) = load(root, &root.join("projects.json"), MAX_RECORD) else { warnings.push("ROOT_UNAVAILABLE".into()); return };
    let Ok(index) = serde_json::from_slice::<Value>(&bytes) else { warnings.push("CORRUPT_RECORD".into()); return };
    for project in index["projects"].as_array().map(Vec::as_slice).unwrap_or_default() {
        let (Some(path), Some(data_dir)) = (field(project, "path"), field(project, "data_dir")) else { warnings.push("CORRUPT_RECORD".into()); continue };
        let Ok(data_root) = Path::new(&data_dir).canonicalize() else { warnings.push("PROJECT_UNAVAILABLE".into()); continue };
        let db = data_root.join("crush.db");
        let Ok(connection) = open(&data_root, &db) else { warnings.push("PROJECT_UNAVAILABLE".into()); continue };
        // Child sessions carry a parent; only top-level conversations are resumable by the user.
        let Ok(mut statement) = connection.prepare("SELECT id FROM sessions WHERE parent_session_id IS NULL OR parent_session_id = '' ORDER BY id") else { warnings.push("SCHEMA_UNAVAILABLE".into()); continue };
        let Ok(ids) = statement.query_map([], |row| row.get::<_, String>(0)) else { warnings.push("SCHEMA_UNAVAILABLE".into()); continue };
        for id in ids.flatten().take(MAX_FILES) { out.push(Candidate { root: data_root.clone(), path: db.clone(), locator: Some(id), context: Some(path.clone()) }); }
    }
}

pub(super) fn crush(cand: &Candidate) -> Result<Decoded> {
    let session = locator(cand)?;
    let cwd = checked_cwd(cand.context.as_deref().ok_or(HistoryError::InvalidIdentity)?)?;
    let connection = open(&cand.root, &cand.path)?;
    let (id, title) = connection
        .query_row("SELECT id, title FROM sessions WHERE id = ?1", [session], |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)))
        .map_err(|_| HistoryError::NotFound)?;
    let id = checked_id(&id)?;
    let mut statement = connection
        .prepare("SELECT id, role, parts FROM messages WHERE session_id = ?1 ORDER BY created_at, rowid")
        .map_err(|_| HistoryError::Io)?;
    let rows = statement
        .query_map([session], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)))
        .map_err(|_| HistoryError::Io)?;
    let mut fingerprint = vec![title.unwrap_or_default()];
    let mut messages = vec![];
    for (ordinal, row) in rows.flatten().enumerate() {
        let (message_id, role, parts) = row;
        fingerprint.push(format!("{message_id}|{role}|{parts}"));
        let parts: Value = serde_json::from_str(&parts).map_err(|_| HistoryError::InvalidRequest)?;
        // `parts` is `[{"type":"text","data":{"text":…}}, …]`; other part types are not displayed.
        let text = parts.as_array().map(Vec::as_slice).unwrap_or_default().iter().filter(|part| part["type"] == "text").filter_map(|part| part["data"]["text"].as_str()).collect::<Vec<_>>().join("\n");
        if !text.is_empty() { messages.push(Message { ordinal, role, text, id: Some(message_id), parent_id: None }); }
    }
    let (used, hash) = row_digest(CanonicalProvider::Crush, session, &fingerprint);
    Ok(Decoded { used, hash, entry: make_entry(CanonicalProvider::Crush, id, cwd, None, None, None), messages, warnings: vec![] })
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(2) { return None; }
    bytes.chunks(2).map(|pair| u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()).collect()
}

/// `~/.cursor/chats/<md5(workspace)>/<chatId>/store.db` with `meta(key,value)` and
/// `blobs(id,data)`. The single `meta` row keyed `0` is hex-encoded JSON carrying `agentId` and
/// `latestRootBlobId`. The conversation is the root tree node's field-1 children, walked by the F6
/// reader's shared decoder (`remote/reference_chat/history_cursor.rs`). The root node also names
/// the workspace (field 21.1); it is trusted only when its md5 is the store's parent folder name.
/// Evidence: `.omo-parity/facts/local-3.md:165-191`, root fields measured on both local stores.
pub(super) fn cursor(cand: &Candidate) -> Result<Decoded> {
    use crate::remote::reference_chat::history_cursor::{cursor_meta_root, cursor_projection, cursor_root_workspace, user_query};
    use md5::{Digest as _, Md5};
    let connection = open(&cand.root, &cand.path)?;
    let encoded: String = connection.query_row("SELECT value FROM meta WHERE key = '0'", [], |row| row.get(0)).map_err(|_| HistoryError::NotFound)?;
    let decoded = unhex(&encoded).ok_or(HistoryError::InvalidIdentity)?;
    let meta: Value = serde_json::from_slice(&decoded).map_err(|_| HistoryError::InvalidIdentity)?;
    let id = checked_id(&field(&meta, "agentId").ok_or(HistoryError::InvalidIdentity)?)?;
    let blob = |id: &str| connection.query_row("SELECT data FROM blobs WHERE id = ?1", [id], |row| row.get::<_, Vec<u8>>(0)).ok();
    let root = cursor_meta_root(&encoded);
    let mut warnings = vec![];
    let projection = match root.as_deref().map(|root| cursor_projection(root, blob)) {
        Some(Ok(projection)) => projection,
        _ => { warnings.push("TRANSCRIPT_UNAVAILABLE".into()); String::new() }
    };
    let hashed = cand.path.parent().and_then(Path::parent).and_then(Path::file_name).and_then(|name| name.to_str());
    let cwd = root.as_deref().and_then(blob).and_then(|node| cursor_root_workspace(&node))
        .filter(|workspace| hashed == Some(format!("{:x}", Md5::digest(workspace.as_bytes())).as_str()))
        .unwrap_or_else(|| { warnings.push("CWD_UNAVAILABLE".into()); String::new() });
    let mut messages = vec![];
    for (ordinal, line) in projection.lines().enumerate() {
        let Ok(message) = serde_json::from_str::<Value>(line) else { continue };
        let message_id = field(&message, "id");
        match message["role"].as_str() {
            // Context-only user messages (`<user_info>`) carry no `<user_query>` and are not turns.
            Some("user") => if let Some(text) = user_query(&message) { messages.push(Message { ordinal, role: "user".into(), text, id: message_id, parent_id: None }); },
            Some("assistant") => {
                if let Some(text) = message["content"].as_str().filter(|text| !text.is_empty()) {
                    messages.push(Message { ordinal, role: "assistant".into(), text: text.to_owned(), id: message_id.clone(), parent_id: None });
                }
                // Reasoning blocks are encrypted and empty; text parts are prose, tool-call parts are calls.
                for part in message["content"].as_array().map(Vec::as_slice).unwrap_or_default() {
                    match part["type"].as_str() {
                        Some("text") => if let Some(text) = part["text"].as_str().filter(|text| !text.is_empty()) { messages.push(Message { ordinal, role: "assistant".into(), text: text.to_owned(), id: message_id.clone(), parent_id: None }); },
                        Some("tool-call") => if let Some(name) = part["toolName"].as_str() { messages.push(Message { ordinal, role: "tool".into(), text: tool_text(name, &part["args"].to_string()), id: field(part, "toolCallId"), parent_id: None }); },
                        _ => {}
                    }
                }
            }
            // `system` prompts and `tool` results are not displayed turns.
            _ => {}
        }
    }
    let (used, hash) = row_digest(CanonicalProvider::Cursor, &id, &[encoded, projection]);
    Ok(Decoded { used, hash, entry: make_entry(CanonicalProvider::Cursor, id, cwd, None, None, None), messages, warnings })
}

/// antigravity indexes its conversations in
/// `~/.gemini/antigravity-cli/conversation_summaries.db`
/// `conversation_summaries(conversation_id, title, preview, step_count, last_modified_time, workspace_uris, parent_conversation_id)`,
/// where `workspace_uris` is a JSON array of `file://` URIs. The readable transcript is
/// `brain/<id>/.system_generated/logs/transcript_full.jsonl`, whose records are
/// `{step_index, source, type, status, created_at, content?, thinking?, tool_calls?}` with the role
/// encoded in `type`: `USER_INPUT` is the prompt, `PLANNER_RESPONSE` the model (prose `content`,
/// `tool_calls[{name, args}]`), `GENERIC` a tool's output and `ERROR_MESSAGE` a runtime error.
/// Evidence: `.omo-parity/facts/local-2.md:87-93`, `.omo-parity/facts/agy-assistant-records-2026-10-22.md`.
pub(super) fn antigravity_sources(root: &Path, out: &mut Vec<Candidate>, warnings: &mut Vec<String>) {
    let path = root.join("conversation_summaries.db");
    let Ok(connection) = open(root, &path) else { warnings.push("ROOT_UNAVAILABLE".into()); return };
    let Ok(mut statement) = connection.prepare("SELECT conversation_id FROM conversation_summaries ORDER BY conversation_id") else { warnings.push("SCHEMA_UNAVAILABLE".into()); return };
    let Ok(ids) = statement.query_map([], |row| row.get::<_, String>(0)) else { warnings.push("SCHEMA_UNAVAILABLE".into()); return };
    for id in ids.flatten().take(MAX_FILES) { out.push(Candidate { root: root.into(), path: path.clone(), locator: Some(id), context: None }); }
}

pub(super) fn antigravity(cand: &Candidate, budget: usize) -> Result<Decoded> {
    let conversation = locator(cand)?;
    let connection = open(&cand.root, &cand.path)?;
    let (id, workspaces, parent_id) = connection
        .query_row("SELECT conversation_id, workspace_uris, parent_conversation_id FROM conversation_summaries WHERE conversation_id = ?1", [conversation], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?, row.get::<_, Option<String>>(2)?))
        })
        .map_err(|_| HistoryError::NotFound)?;
    let id = checked_id(&id)?;
    let uris: Value = serde_json::from_str(workspaces.as_deref().unwrap_or("[]")).map_err(|_| HistoryError::InvalidIdentity)?;
    let workspace = uris.as_array().and_then(|uris| uris.first()).and_then(Value::as_str).and_then(|uri| uri.strip_prefix("file://")).ok_or(HistoryError::InvalidIdentity)?;
    let cwd = checked_cwd(workspace)?;
    let transcript = cand.root.join("brain").join(&id).join(".system_generated").join("logs").join("transcript_full.jsonl");
    let mut warnings = vec![];
    let bytes = match load(&cand.root, &transcript, budget) { Ok(bytes) => bytes, Err(_) => { warnings.push("TRANSCRIPT_UNAVAILABLE".into()); Vec::new() } };
    let mut messages = vec![];
    for (ordinal, v) in records(&bytes, &mut warnings) {
        let id = version_field(&v, "step_index");
        match v["type"].as_str() {
            Some("USER_INPUT") => {
                let text = readers_json::leading_tag_body(&content_text(&v["content"]), "USER_REQUEST");
                if !text.is_empty() { messages.push(Message { ordinal, role: "user".into(), text, id, parent_id: None }); }
            }
            // `thinking` is not shown; `GENERIC` tool outputs and `ERROR_MESSAGE`s are not turns.
            Some("PLANNER_RESPONSE") => {
                let text = content_text(&v["content"]);
                if !text.is_empty() { messages.push(Message { ordinal, role: "assistant".into(), text, id, parent_id: None }); }
                for call in v["tool_calls"].as_array().map(Vec::as_slice).unwrap_or_default() {
                    // agy calls carry no id of their own; the step index already names the reply.
                    if let Some(name) = call["name"].as_str() { messages.push(Message { ordinal, role: "tool".into(), text: tool_text(name, &call["args"].to_string()), id: None, parent_id: None }); }
                }
            }
            _ => {}
        }
    }
    let (used, hash) = row_digest(CanonicalProvider::Antigravity, conversation, &[cwd.clone(), parent_id.clone().unwrap_or_default(), digest(&bytes)]);
    Ok(Decoded { used: used + bytes.len(), hash, entry: make_entry(CanonicalProvider::Antigravity, id, cwd, None, parent_id, None), messages, warnings })
}
