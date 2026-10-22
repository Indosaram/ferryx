//! JSONL stores whose record type, not a `role` field, carries the speaker: copilot, droid,
//! kimi and grok.
use super::*;
use md5::Md5;

/// `~/.copilot/session-state/<uuid>.jsonl`. Envelope `{type,data,id,parentId,timestamp}`;
/// `session.start` carries `data.sessionId` and `data.copilotVersion`, and the display text of a
/// turn is `data.content`. No record type carries a cwd.
/// Evidence: `.omo-parity/facts/local-3.md:72-97`.
pub(super) fn copilot(cand: &Candidate, budget: usize) -> Result<Decoded> {
    let bytes = load(&cand.root, &cand.path, budget)?;
    let mut warnings = vec!["CWD_UNAVAILABLE".to_string()];
    let mut identity = None;
    let mut version = None;
    let mut messages = vec![];
    for (ordinal, v) in records(&bytes, &mut warnings) {
        let data = &v["data"];
        match v["type"].as_str().unwrap_or("") {
            "session.start" => {
                let id = checked_id(&field(data, "sessionId").ok_or(HistoryError::InvalidIdentity)?)?;
                if identity.as_ref().is_some_and(|old| old != &id) { return Err(HistoryError::InvalidIdentity); }
                identity = Some(id);
                version = field(data, "copilotVersion").or(version);
            }
            kind @ ("user.message" | "assistant.message") => {
                let text = content_text(&data["content"]);
                let role = if kind == "user.message" { "user" } else { "assistant" };
                if !text.is_empty() { messages.push(Message { ordinal, role: role.into(), text, id: field(data, "messageId").or_else(|| field(&v, "id")), parent_id: field(&v, "parentId") }); }
            }
            _ => {}
        }
    }
    let id = identity.ok_or(HistoryError::InvalidIdentity)?;
    // copilot writes no cwd, so the entry cannot be matched to a workspace and resume stays blocked
    // by the cwd fence in `History::resume` rather than guessing a directory.
    Ok(Decoded { used: bytes.len(), hash: digest(&bytes), entry: make_entry(CanonicalProvider::Copilot, id, String::new(), version, None, None), messages, warnings })
}

/// `~/.factory/sessions/<cwd-slug>/<uuid>.jsonl`. Line 1 is
/// `{"type":"session_start","id","title","owner","version":2,"cwd","hostId"}` and later turns are
/// `{"type":"message","id","timestamp","message":{...},"parentId"?}`. The inner `message` shape was
/// never measured, so droid lists but does not read.
/// Evidence: `.omo-parity/facts/remote-droid.md:148-156`.
pub(super) fn droid(cand: &Candidate, budget: usize) -> Result<Decoded> {
    let bytes = load(&cand.root, &cand.path, budget)?;
    let mut warnings = vec![];
    let mut identity = None;
    let mut version = None;
    let mut turns = 0;
    for (_, v) in records(&bytes, &mut warnings) {
        match v["type"].as_str().unwrap_or("") {
            "session_start" => {
                let (Some(id), Some(cwd)) = (field(&v, "id"), field(&v, "cwd")) else { return Err(HistoryError::InvalidIdentity) };
                identity = Some((checked_id(&id)?, checked_cwd(&cwd)?));
                version = version_field(&v, "version");
            }
            "message" => turns += 1,
            _ => {}
        }
    }
    let (id, cwd) = identity.ok_or(HistoryError::InvalidIdentity)?;
    // An unauthenticated or abandoned run still leaves a `session_start`-only file; droid's own
    // `find` hides those, so they are not offered as resumable conversations either.
    if turns == 0 { return Err(HistoryError::NotFound); }
    warnings.push("READ_UNSETTLED".into());
    Ok(Decoded { used: bytes.len(), hash: digest(&bytes), entry: make_entry(CanonicalProvider::Droid, id, cwd, version, None, None), messages: vec![], warnings })
}

/// `~/.kimi/sessions/<md5(cwd)>/<uuid>/` holds `context.jsonl` (the model context, which the walk
/// finds the session by) and `wire.jsonl`, the UI event stream that carries the conversation.
/// Each wire line is `{"timestamp","message":{"type","payload"}}`: `TurnBegin.payload.user_input`
/// (a string or text parts) is the prompt, `ContentPart` `{type:"text",text}` is streamed assistant
/// prose (`{type:"think"}` is reasoning and is not shown), and `ToolCall`
/// `{id,function:{name,arguments}}` is a tool call whose arguments `ToolCallPart.arguments_part`
/// may continue. A session without a wire stream falls back to the user records of `context.jsonl`.
/// The cwd is the `~/.kimi/kimi.json` `work_dirs[].path` whose md5 names the session's parent folder.
/// Evidence: `.omo-parity/facts/kimi-wire-vocabulary.md`, `.omo-parity/facts/local-2.md:249-266`.
pub(super) fn kimi(cand: &Candidate, budget: usize) -> Result<Decoded> {
    let context_bytes = load(&cand.root, &cand.path, budget)?;
    let session_dir = cand.path.parent().ok_or(HistoryError::InvalidIdentity)?;
    let id = checked_id(session_dir.file_name().and_then(|name| name.to_str()).ok_or(HistoryError::InvalidIdentity)?)?;
    let mut warnings = vec![];
    // Without a matching `work_dirs` entry the entry keeps an empty cwd, so the workspace filter
    // and the resume cwd fence still refuse it rather than guessing a directory.
    let cwd = kimi_cwd(&cand.root, session_dir).unwrap_or_else(|| { warnings.push("CWD_UNAVAILABLE".into()); String::new() });
    // Most sessions never wrote a wire stream; only one that exists but cannot be read is reported.
    let wire_bytes = match load(&cand.root, &cand.path.with_file_name("wire.jsonl"), budget.saturating_sub(context_bytes.len())) {
        Ok(bytes) => bytes,
        Err(HistoryError::NotFound) => Vec::new(),
        Err(_) => { warnings.push("TRANSCRIPT_UNAVAILABLE".into()); Vec::new() }
    };
    let mut messages = kimi_wire_messages(&wire_bytes, &mut warnings);
    if messages.is_empty() {
        for (ordinal, v) in records(&context_bytes, &mut warnings) {
            if field(&v, "role").as_deref() != Some("user") { continue; }
            let text = content_text(&v["content"]);
            if !text.is_empty() { messages.push(Message { ordinal, role: "user".into(), text, id: None, parent_id: None }); }
        }
    }
    let mut hashed = context_bytes.clone();
    hashed.extend_from_slice(&wire_bytes);
    Ok(Decoded { used: hashed.len(), hash: digest(&hashed), entry: make_entry(CanonicalProvider::Kimi, id, cwd, None, None, None), messages, warnings })
}

/// The conversation in one kimi `wire.jsonl`: a user message per `TurnBegin`, consecutive text
/// parts of a turn joined into one assistant message, and a tool entry per `ToolCall`.
fn kimi_wire_messages(bytes: &[u8], warnings: &mut Vec<String>) -> Vec<Message> {
    let mut messages: Vec<Message> = vec![];
    let mut in_turn = false;
    // What the newest message still accepts: more streamed prose, or more streamed arguments.
    let (mut prose_open, mut arguments_open) = (false, false);
    for (ordinal, v) in records(bytes, warnings) {
        let payload = &v["message"]["payload"];
        match v["message"]["type"].as_str().unwrap_or("") {
            "TurnBegin" => {
                in_turn = true;
                (prose_open, arguments_open) = (false, false);
                let text = content_text(&payload["user_input"]);
                if !text.is_empty() { messages.push(Message { ordinal, role: "user".into(), text, id: None, parent_id: None }); }
            }
            "TurnEnd" => { in_turn = false; (prose_open, arguments_open) = (false, false); }
            "ContentPart" if in_turn && payload["type"] == "text" => {
                let Some(text) = payload["text"].as_str().filter(|text| !text.is_empty()) else { continue };
                arguments_open = false;
                if prose_open {
                    messages.last_mut().expect("open prose has a message").text.push_str(text);
                } else {
                    messages.push(Message { ordinal, role: "assistant".into(), text: text.to_owned(), id: None, parent_id: None });
                    prose_open = true;
                }
            }
            "ToolCall" if in_turn => {
                let Some(name) = payload["function"]["name"].as_str().filter(|name| !name.is_empty()) else { continue };
                let arguments = payload["function"]["arguments"].as_str().unwrap_or("");
                messages.push(Message { ordinal, role: "tool".into(), text: tool_text(name, arguments), id: field(payload, "id"), parent_id: None });
                (prose_open, arguments_open) = (false, true);
            }
            "ToolCallPart" if arguments_open => {
                if let Some(part) = payload["arguments_part"].as_str() {
                    let tool = messages.last_mut().expect("open arguments have a tool entry");
                    if !tool.text.contains(' ') { tool.text.push(' '); }
                    tool.text.push_str(part);
                }
            }
            "ToolResult" => { (prose_open, arguments_open) = (false, false); }
            _ => {}
        }
    }
    messages
}

/// The cwd whose md5 names this session's parent folder: the `work_dirs[].path` entry of
/// `kimi.json`, which sits beside the sessions root. Read-only; `None` when the config is missing,
/// unreadable, or names no matching directory.
fn kimi_cwd(root: &Path, session_dir: &Path) -> Option<String> {
    let hashed = session_dir.parent()?.file_name()?.to_str()?;
    let file = fs::File::open(root.parent()?.join("kimi.json")).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_RECORD as u64 + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() > MAX_RECORD { return None; }
    let config: Value = serde_json::from_slice(&bytes).ok()?;
    let path = config["work_dirs"].as_array()?.iter().filter_map(|dir| dir["path"].as_str()).find(|path| format!("{:x}", <Md5 as md5::Digest>::digest(path.as_bytes())) == hashed)?;
    checked_cwd(path).ok()
}

/// `~/.grok/sessions/<encoded cwd>/<uuidv7>/summary.json` holds `{"info":{"id","cwd"},...}` and the
/// sibling `updates.jsonl` streams `params.update.sessionUpdate` chunks; `user_message_chunk` and
/// `agent_message_chunk` carry `content.text`.
/// Evidence: `.omo-parity/facts/grok-evidence/sample-summary.json`,
/// `.omo-parity/facts/grok-evidence/sample-updates-head.jsonl`.
pub(super) fn grok(cand: &Candidate, budget: usize) -> Result<Decoded> {
    let summary_bytes = load(&cand.root, &cand.path, budget)?;
    let summary: Value = serde_json::from_slice(&summary_bytes).map_err(|_| HistoryError::InvalidIdentity)?;
    let info = &summary["info"];
    let (Some(id), Some(cwd)) = (field(info, "id"), field(info, "cwd")) else { return Err(HistoryError::InvalidIdentity) };
    let (id, cwd) = (checked_id(&id)?, checked_cwd(&cwd)?);
    let updates_path = cand.path.with_file_name("updates.jsonl");
    let mut warnings = vec![];
    let updates_bytes = match load(&cand.root, &updates_path, budget.saturating_sub(summary_bytes.len())) {
        Ok(bytes) => bytes,
        Err(_) => { warnings.push("TRANSCRIPT_UNAVAILABLE".into()); Vec::new() }
    };
    let mut messages: Vec<Message> = vec![];
    for (ordinal, v) in records(&updates_bytes, &mut warnings) {
        let update = &v["params"]["update"];
        let role = match update["sessionUpdate"].as_str() { Some("user_message_chunk") => "user", Some("agent_message_chunk") => "assistant", _ => continue };
        let Some(text) = update["content"]["text"].as_str() else { continue };
        // The stream is chunked, so consecutive chunks of one speaker are one displayed message.
        if messages.last().is_some_and(|last| last.role == role) {
            messages.last_mut().expect("the speaker check found a last message").text.push_str(text);
        } else {
            messages.push(Message { ordinal, role: role.into(), text: text.to_owned(), id: None, parent_id: None });
        }
    }
    messages.retain(|message| !message.text.is_empty());
    let mut hashed = summary_bytes.clone();
    hashed.extend_from_slice(&updates_bytes);
    Ok(Decoded { used: hashed.len(), hash: digest(&hashed), entry: make_entry(CanonicalProvider::Grok, id, cwd, None, None, None), messages, warnings })
}
