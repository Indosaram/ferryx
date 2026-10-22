//! Stores that keep one conversation as a pair of whole JSON documents: cline.
use super::*;

/// Strip a wrapper element such as `<user_input mode="act">…</user_input>` or
/// `<USER_REQUEST>…</USER_REQUEST>` that the CLI injects around the typed prompt.
pub(super) fn unwrap_tag(text: &str, tag: &str) -> String {
    let trimmed = text.trim();
    let close = format!("</{tag}>");
    if !trimmed.starts_with(&format!("<{tag}")) || !trimmed.ends_with(&close) { return text.to_owned(); }
    // A malformed wrapper whose only `>` belongs to the closing tag must not slice out of order.
    let body_end = trimmed.len() - close.len();
    let Some(open_end) = trimmed.find('>').filter(|open_end| *open_end < body_end) else { return text.to_owned() };
    trimmed[open_end + 1..body_end].trim().to_owned()
}

/// The body of a wrapper that opens the text, dropping whatever the CLI appends after its close
/// tag. agy writes `<USER_REQUEST>prompt</USER_REQUEST>\n<ADDITIONAL_METADATA>…` (measured:
/// `fixtures/antigravity/transcript_full.jsonl`). Text that does not open with the tag is kept.
pub(super) fn leading_tag_body(text: &str, tag: &str) -> String {
    let trimmed = text.trim_start();
    let (open, close) = (format!("<{tag}>"), format!("</{tag}>"));
    let Some(rest) = trimmed.strip_prefix(&open) else { return text.to_owned() };
    let Some(end) = rest.find(&close) else { return text.to_owned() };
    rest[..end].trim().to_owned()
}

/// `~/.cline/data/sessions/<id>/<id>.json` is the metadata document (`session_id`, `cwd`,
/// `version`) and the sibling `<id>.messages.json` holds
/// `{"sessionId","messages":[{"id","role","content":[{"type":"text","text"}],"ts"}]}`.
/// Evidence: `.omo-parity/facts/local-3.md:272-290`.
pub(super) fn cline(cand: &Candidate, budget: usize) -> Result<Decoded> {
    let meta_bytes = load(&cand.root, &cand.path, budget)?;
    let meta: Value = serde_json::from_slice(&meta_bytes).map_err(|_| HistoryError::InvalidIdentity)?;
    let (Some(id), Some(cwd)) = (field(&meta, "session_id"), field(&meta, "cwd")) else { return Err(HistoryError::InvalidIdentity) };
    let (id, cwd) = (checked_id(&id)?, checked_cwd(&cwd)?);
    let messages_path = cand.path.with_file_name(format!("{id}.messages.json"));
    let mut warnings = vec![];
    let message_bytes = match load(&cand.root, &messages_path, budget.saturating_sub(meta_bytes.len())) {
        Ok(bytes) => bytes,
        Err(_) => { warnings.push("TRANSCRIPT_UNAVAILABLE".into()); Vec::new() }
    };
    let mut messages = vec![];
    if !message_bytes.is_empty() {
        let document: Value = serde_json::from_slice(&message_bytes).map_err(|_| HistoryError::InvalidRequest)?;
        for (ordinal, entry) in document["messages"].as_array().map(Vec::as_slice).unwrap_or_default().iter().enumerate() {
            let text = unwrap_tag(&content_text(&entry["content"]), "user_input");
            if !text.is_empty() { messages.push(Message { ordinal, role: field(entry, "role").unwrap_or_default(), text, id: field(entry, "id"), parent_id: None }); }
        }
    }
    let mut hashed = meta_bytes.clone();
    hashed.extend_from_slice(&message_bytes);
    Ok(Decoded { used: hashed.len(), hash: digest(&hashed), entry: make_entry(CanonicalProvider::Cline, id, cwd, version_field(&meta, "version"), None, None), messages, warnings })
}
