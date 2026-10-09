use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationMessage {
    pub ordinal: usize,
    pub role: String,
    pub text: String,
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
}

pub fn is_valid_session_id(session_id: &str) -> bool {
    if session_id.is_empty() || session_id.len() > 128 {
        return false;
    }
    if session_id.contains('/') || session_id.contains('\\') || session_id.contains("..") {
        return false;
    }
    session_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

pub fn transcript_path_for_session(home: &Path, session_id: &str) -> Option<PathBuf> {
    if !is_valid_session_id(session_id) {
        return None;
    }
    let sessions_root = home.join(".omo").join("agent").join("sessions");
    let entries = std::fs::read_dir(&sessions_root).ok()?;
    let suffix1 = format!("_{session_id}.jsonl");
    let exact = format!("{session_id}.jsonl");

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Ok(sub_entries) = std::fs::read_dir(&path) {
                for sub in sub_entries.flatten() {
                    let sub_path = sub.path();
                    if let Some(file_name) = sub_path.file_name().and_then(|f| f.to_str()) {
                        if file_name == exact || file_name.ends_with(&suffix1) {
                            return Some(sub_path);
                        }
                    }
                }
            }
        }
    }
    None
}

pub fn slug_for_cwd(cwd: &str) -> String {
    let mut slug = String::new();
    slug.push_str("--");
    let mut first = true;
    for part in cwd.split('/') {
        if part.is_empty() {
            continue;
        }
        if !first {
            slug.push_str("-");
        }
        first = false;
        slug.push_str(part);
    }
    slug.push_str("--");
    slug
}

pub fn latest_transcript_for_cwd(
    home: &Path,
    cwd: &str,
    session_id_hint: Option<&str>,
) -> Option<PathBuf> {
    let candidate_dir =
        home.join(".omo").join("agent").join("sessions").join(&slug_for_cwd(cwd));
    let Ok(entries) = std::fs::read_dir(&candidate_dir) else {
        return None;
    };
    let mut newest_modified: Option<std::time::SystemTime> = None;
    let mut newest_path: Option<PathBuf> = None;
    for entry in entries {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }
        let Some(file_name) = path.file_name().and_then(|f| f.to_str()) else {
            continue;
        };
        if let Some(hint) = session_id_hint {
            if file_name.contains(hint) {
                return Some(path);
            }
        }
        let Ok(modified) = entry.metadata().and_then(|metadata| metadata.modified()) else {
            continue;
        };
        let is_newer = match newest_modified {
            None => true,
            Some(previous) => modified > previous,
        };
        if is_newer {
            newest_modified = Some(modified);
            newest_path = Some(path);
        }
    }
    newest_path
}

#[derive(Deserialize)]
struct RecordEnvelope {
    #[serde(rename = "type")]
    record_type: Option<String>,
    id: Option<String>,
    display: Option<bool>,
    message: Option<MessageBody>,
    #[serde(default)]
    timestamp: Option<String>,
}

#[derive(Deserialize)]
struct MessageBody {
    role: Option<String>,
    content: Option<serde_json::Value>,
    #[serde(rename = "toolName")]
    tool_name: Option<String>,
}

#[derive(Deserialize)]
struct ContentPart {
    #[serde(rename = "type")]
    part_type: Option<String>,
    text: Option<String>,
    name: Option<String>,
}

pub fn read_conversation(
    path: &Path,
    limit: usize,
    before: Option<usize>,
) -> Result<(Vec<ConversationMessage>, usize), String> {
    let (messages, malformed, _) =
        read_conversation_with_generation(path, None, "default", limit, before)?;
    Ok((messages, malformed))
}

/// Same filtering and windowing as [`read_conversation`], over bytes already in memory.
/// A remote transcript arrives as command output rather than a local file, so the reader
/// path and the parser are separate.
pub fn read_conversation_from_bytes(
    bytes: &[u8],
    limit: usize,
    before: Option<usize>,
) -> (Vec<ConversationMessage>, usize) {
    let (messages, malformed, _) =
        read_conversation_bytes_with_generation(bytes, None, "default", limit, before);
    (messages, malformed)
}

/// Derives a stable authoritative conversation generation token.
///
/// The token is derived from the authoritative provider conversation ID and the conversation's
/// initial genesis (session header timestamp and initial message identity).
pub fn derive_conversation_generation(
    provider_session_id: Option<&str>,
    session_id: &str,
    session_header_timestamp: Option<&str>,
    first_message: Option<&ConversationMessage>,
) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    let effective_id = provider_session_id.unwrap_or(session_id);
    hasher.update(effective_id.as_bytes());
    if let Some(ts) = session_header_timestamp {
        hasher.update(b":hdr:");
        hasher.update(ts.as_bytes());
    }
    if let Some(first) = first_message {
        hasher.update(b":msg0:");
        if let Some(id) = &first.id {
            hasher.update(id.as_bytes());
        }
        hasher.update(first.role.as_bytes());
        hasher.update(first.text.as_bytes());
        if let Some(ts) = &first.timestamp {
            hasher.update(ts.as_bytes());
        }
    }
    let digest = format!("{:x}", hasher.finalize());
    format!("{effective_id}:{}", &digest[..16])
}

#[derive(Clone, Debug)]
struct SessionIncarnationState {
    provider_session_id: String,
    prefix_hashes: Vec<[u8; 32]>,
    incarnation: u64,
}

#[derive(Clone, Debug, Default)]
pub struct ConversationIncarnationTracker {
    states: Arc<Mutex<HashMap<String, SessionIncarnationState>>>,
}

impl ConversationIncarnationTracker {
    pub fn new() -> Self {
        Self {
            states: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Derives an authoritative conversation incarnation token for a session.
    ///
    /// Invariants:
    /// 1. Independent of pagination: always computed over the full conversation's message list.
    /// 2. Append alone preserves incarnation: if new messages extend the known prefix without
    ///    modifying any existing message, incarnation number stays identical (no false resets).
    /// 3. Same-first-row rewrite rotates: if message 0 is identical but any subsequent existing
    ///    message is rewritten, the prefix hash mismatch triggers an incarnation increment.
    /// 4. Tail truncation rotates: if the conversation length shrinks below the previous known
    ///    count, incarnation increments.
    /// 5. Head truncation rotates: if message 0 is removed, the entire hash chain shifts and
    ///    incarnation increments.
    /// 6. Replacement rotates: if provider session ID changes, incarnation resets/increments.
    pub fn observe(
        &self,
        session_id: &str,
        provider_session_id: &str,
        all_messages: &[ConversationMessage],
    ) -> String {
        use sha2::{Digest, Sha256};

        let new_hashes: Vec<[u8; 32]> = all_messages
            .iter()
            .map(|m| {
                let mut hasher = Sha256::new();
                hasher.update(m.role.as_bytes());
                hasher.update(b":");
                hasher.update(m.text.as_bytes());
                if let Some(id) = &m.id {
                    hasher.update(b":id:");
                    hasher.update(id.as_bytes());
                }
                if let Some(ts) = &m.timestamp {
                    hasher.update(b":ts:");
                    hasher.update(ts.as_bytes());
                }
                let result = hasher.finalize();
                let mut out = [0u8; 32];
                out.copy_from_slice(&result);
                out
            })
            .collect();

        let mut lock = self.states.lock();
        if let Some(state) = lock.get_mut(session_id) {
            if state.provider_session_id != provider_session_id {
                state.provider_session_id = provider_session_id.to_string();
                state.prefix_hashes = new_hashes;
                state.incarnation += 1;
                return format!("{provider_session_id}:inc-{}", state.incarnation);
            }

            let prev_len = state.prefix_hashes.len();
            let new_len = new_hashes.len();

            let is_tail_truncation = new_len < prev_len;
            let is_prefix_rewrite = if is_tail_truncation {
                true
            } else {
                state
                    .prefix_hashes
                    .iter()
                    .zip(new_hashes.iter())
                    .any(|(prev_h, new_h)| prev_h != new_h)
            };

            if is_tail_truncation || is_prefix_rewrite {
                state.incarnation += 1;
                state.prefix_hashes = new_hashes;
                return format!("{provider_session_id}:inc-{}", state.incarnation);
            }

            state.prefix_hashes = new_hashes;
            return format!("{provider_session_id}:inc-{}", state.incarnation);
        }

        lock.insert(
            session_id.to_string(),
            SessionIncarnationState {
                provider_session_id: provider_session_id.to_string(),
                prefix_hashes: new_hashes,
                incarnation: 1,
            },
        );
        format!("{provider_session_id}:inc-1")
    }
}

static GLOBAL_INCARNATION_TRACKER: OnceLock<ConversationIncarnationTracker> = OnceLock::new();

pub fn global_incarnation_tracker() -> &'static ConversationIncarnationTracker {
    GLOBAL_INCARNATION_TRACKER.get_or_init(ConversationIncarnationTracker::new)
}

pub fn read_conversation_with_generation(
    path: &Path,
    provider_session_id: Option<&str>,
    session_id: &str,
    limit: usize,
    before: Option<usize>,
) -> Result<(Vec<ConversationMessage>, usize, String), String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    Ok(parse_conversation_inner(
        BufReader::new(file),
        provider_session_id,
        session_id,
        limit,
        before,
    ))
}

pub fn read_conversation_bytes_with_generation(
    bytes: &[u8],
    provider_session_id: Option<&str>,
    session_id: &str,
    limit: usize,
    before: Option<usize>,
) -> (Vec<ConversationMessage>, usize, String) {
    parse_conversation_inner(
        std::io::Cursor::new(bytes),
        provider_session_id,
        session_id,
        limit,
        before,
    )
}

fn parse_conversation_inner<R: BufRead>(
    reader: R,
    provider_session_id: Option<&str>,
    session_id: &str,
    limit: usize,
    before: Option<usize>,
) -> (Vec<ConversationMessage>, usize, String) {
    let mut all_messages: Vec<ConversationMessage> = Vec::new();
    let mut malformed_lines = 0;
    let mut current_ordinal = 0;
    let mut session_header_timestamp: Option<String> = None;

    for line_res in reader.lines() {
        let line = match line_res {
            Ok(l) => l,
            Err(_) => {
                malformed_lines += 1;
                continue;
            }
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let record: RecordEnvelope = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(_) => {
                malformed_lines += 1;
                continue;
            }
        };

        if record.record_type.as_deref() == Some("session") {
            if session_header_timestamp.is_none() {
                session_header_timestamp = record.timestamp.clone();
            }
            continue;
        }

        if record.display == Some(false) {
            continue;
        }

        if record.record_type.as_deref() != Some("message") {
            continue;
        }

        let Some(body) = record.message else {
            continue;
        };

        let role = body.role.unwrap_or_else(|| "user".to_string());
        let mut text = String::new();

        if let Some(content) = body.content {
            if let Some(s) = content.as_str() {
                text.push_str(s);
            } else if let Some(arr) = content.as_array() {
                for item in arr {
                    if let Ok(part) = serde_json::from_value::<ContentPart>(item.clone()) {
                        if part.part_type.as_deref() == Some("text") {
                            if let Some(t) = part.text {
                                text.push_str(&t);
                            }
                        } else if part.part_type.as_deref() == Some("toolCall") {
                            let tool = part.name.as_deref().unwrap_or("tool");
                            if !text.is_empty() {
                                text.push('\n');
                            }
                            text.push_str(&format!("→ {tool}"));
                        }
                    }
                }
            }
        }

        if role == "toolResult" && text.is_empty() {
            let tool = body.tool_name.as_deref().unwrap_or("tool");
            text = format!("← {tool} result");
        }

        all_messages.push(ConversationMessage {
            ordinal: current_ordinal,
            role,
            text,
            id: record.id,
            timestamp: record.timestamp.clone(),
        });
        current_ordinal += 1;
    }

    let generation = if let Some(provider_id) = provider_session_id {
        global_incarnation_tracker().observe(session_id, provider_id, &all_messages)
    } else {
        derive_conversation_generation(
            provider_session_id,
            session_id,
            session_header_timestamp.as_deref(),
            all_messages.first(),
        )
    };

    let filtered: Vec<ConversationMessage> = match before {
        Some(cursor) => all_messages
            .into_iter()
            .filter(|m| m.ordinal < cursor)
            .collect(),
        None => all_messages,
    };

    let result_messages = if filtered.len() > limit {
        filtered[filtered.len() - limit..].to_vec()
    } else {
        filtered
    };

    (result_messages, malformed_lines, generation)
}

/// Where a paired host keeps a session's transcript, resolved from the daemon's durable store.
pub struct RemoteTranscriptTarget {
    /// The stored `config.host`, left as JSON so this module stays independent of the SSH types.
    pub host: serde_json::Value,
    pub dir: String,
}

/// Finds the stored descriptor for `session_id` and derives the remote transcript directory.
///
/// The store is keyed by `descriptor.backendSessionId`, which is the id the session service and the
/// gateway both route by. Every value used here comes from that record and never from the caller, so
/// no client-supplied string can reach a remote command. A record missing any required field yields
/// `None` rather than a command built from empty parts.
pub fn remote_target_from_store(
    store_json: &str,
    session_id: &str,
) -> Option<RemoteTranscriptTarget> {
    let parsed: serde_json::Value = serde_json::from_str(store_json).ok()?;
    let descriptor = parsed
        .get("remoteSessions")?
        .as_array()?
        .iter()
        .find(|row| {
            row.pointer("/descriptor/backendSessionId").and_then(|v| v.as_str()) == Some(session_id)
        })?
        .get("descriptor")?
        .clone();
    let host = descriptor.pointer("/config/host")?.clone();
    let home = descriptor.pointer("/config/environment/home")?.as_str()?;
    let project_path = descriptor.pointer("/config/projectPath")?.as_str()?;
    Some(RemoteTranscriptTarget {
        host,
        dir: remote_transcript_dir(home, project_path)?,
    })
}

/// Splits the remote read's leading size line from its body.
///
/// The remote script prints the transcript's true byte size on the first line so the route can tell
/// whether the host sent the whole file or only a bounded tail. Returns `None` when the payload does
/// not carry a parseable size line, which callers treat as "no transcript" - failing closed rather
/// than guessing a size from a partial or garbled response.
pub fn split_remote_size_line(bytes: &[u8]) -> Option<(usize, &[u8])> {
    let idx = bytes.iter().position(|b| *b == b'\n')?;
    let size = std::str::from_utf8(&bytes[..idx])
        .ok()?
        .trim()
        .parse::<usize>()
        .ok()?;
    Some((size, &bytes[idx + 1..]))
}

/// Directory a paired POSIX host keeps this project's omo transcripts in.
///
/// The slug comes from the same [`slug_for_cwd`] the local path uses, which is correct because the
/// remote directory name was observed to match it exactly (`--home-indo-piratetalk--` for
/// `/home/indo/piratetalk`). Windows hosts return `None`: the slug separator there is unverified,
/// and guessing it would silently resolve the wrong directory.
pub fn remote_transcript_dir(home: &str, project_path: &str) -> Option<String> {
    if project_path.contains('\\') || project_path.contains(':') {
        return None;
    }
    let home = home.trim_end_matches('/');
    if home.is_empty() {
        return None;
    }
    Some(format!(
        "{home}/.omo/agent/sessions/{}",
        slug_for_cwd(project_path)
    ))
}

/// Resolves a local transcript strictly from the authoritative provider conversation ID.
///
/// Fails closed (returns `None`) if `provider_session_id` is missing, empty, or invalid.
/// Never falls back to `latest_transcript_for_cwd` or timestamp recency (`ls -t`).
pub fn exact_transcript_path_for_provider(
    home: &Path,
    provider_session_id: Option<&str>,
) -> Option<PathBuf> {
    let provider_id = provider_session_id?;
    transcript_path_for_session(home, provider_id)
}

/// Constructs a POSIX shell command to read an exact remote transcript by provider conversation ID.
///
/// Fails closed (returns `None`) if `provider_session_id` is missing, empty, or invalid.
/// Queries strictly for `*_{provider_id}.jsonl` or `{provider_id}.jsonl`. Never runs `ls -t`
/// or substitutes a different session's file.
pub fn exact_remote_transcript_command(
    dir: &str,
    provider_session_id: Option<&str>,
    budget: usize,
) -> Option<String> {
    let provider_id = provider_session_id?;
    if !is_valid_session_id(provider_id) {
        return None;
    }
    let quoted_dir = crate::ssh::direct::quote_posix(dir);
    let quoted_id = crate::ssh::direct::quote_posix(provider_id);
    let script = format!(
        "d={quoted_dir}; \
         f=$(ls \"$d\"/*_{quoted_id}.jsonl \"$d\"/{quoted_id}.jsonl 2>/dev/null | head -n 1); \
         if [ -n \"$f\" ]; then \
           n=$(wc -c < \"$f\"); printf '%s\\n' \"$n\"; \
           if [ \"$n\" -gt {budget} ]; then tail -c {budget} \"$f\" | tail -n +2; else cat -- \"$f\"; fi; \
         fi"
    );
    Some(format!("sh -c {}", crate::ssh::direct::quote_posix(&script)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;

    #[test]
    fn test_valid_session_id_security() {
        assert!(is_valid_session_id("01a0d650-db7a-7817-a21b-7be552a81e89"));
        assert!(is_valid_session_id("sess_1234"));
        assert!(!is_valid_session_id("../etc/passwd"));
        assert!(!is_valid_session_id("foo/bar"));
        assert!(!is_valid_session_id("foo\\bar"));
        assert!(!is_valid_session_id(""));
    }

    #[test]
    fn test_synthetic_transcript_and_filtering() {
        let temp_dir = std::env::temp_dir().join(format!("test_omo_session_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let file_path = temp_dir.join("2026-09-25_test-session.jsonl");

        let content = r#"{"type":"session","id":"test-session","cwd":"/path","timestamp":"2026-09-25T00:00:00Z"}
{"type":"message","id":"msg-1","timestamp":"2026-09-25T00:00:01Z","message":{"role":"user","content":[{"type":"text","text":"hello "},{"type":"text","text":"world"}]}}
{"type":"custom_message","customType":"info","content":"skip me","display":false}
{"type":"custom_message","customType":"visible_custom","content":"skip me too"}
{MALFORMED JSON LINE
{"type":"message","id":"msg-2","timestamp":"2026-09-25T00:00:02Z","display":false,"message":{"role":"assistant","content":[{"type":"text","text":"hidden"}]}}
{"type":"message","id":"msg-3","timestamp":"2026-09-25T00:00:03Z","message":{"role":"assistant","content":[{"type":"thinking","text":"think"},{"type":"text","text":"response"}]}}
"#;

        {
            let mut f = File::create(&file_path).unwrap();
            f.write_all(content.as_bytes()).unwrap();
        }

        let (messages, malformed) = read_conversation(&file_path, 100, None).unwrap();
        assert_eq!(malformed, 1);
        assert_eq!(messages.len(), 2);

        assert_eq!(messages[0].ordinal, 0);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].text, "hello world");
        assert_eq!(messages[0].id.as_deref(), Some("msg-1"));

        assert_eq!(messages[1].ordinal, 1);
        assert_eq!(messages[1].role, "assistant");
        assert_eq!(messages[1].text, "response");
        assert_eq!(messages[1].id.as_deref(), Some("msg-3"));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_display_false_excluded() {
        let temp_dir = std::env::temp_dir().join(format!("test_omo_display_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let file_path = temp_dir.join("transcript.jsonl");

        let content = r#"{"type":"message","id":"1","display":false,"message":{"role":"user","content":[{"type":"text","text":"secret"}]}}
{"type":"message","id":"2","message":{"role":"user","content":[{"type":"text","text":"visible"}]}}
"#;
        {
            let mut f = File::create(&file_path).unwrap();
            f.write_all(content.as_bytes()).unwrap();
        }

        let (messages, malformed) = read_conversation(&file_path, 10, None).unwrap();
        assert_eq!(malformed, 0);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].text, "visible");

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_path_traversal_rejection() {
        let temp_dir = std::env::temp_dir().join(format!("test_omo_trav_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();

        assert_eq!(transcript_path_for_session(&temp_dir, "../foo"), None);
        assert_eq!(transcript_path_for_session(&temp_dir, "/etc/passwd"), None);
        assert_eq!(transcript_path_for_session(&temp_dir, "foo/bar"), None);
        assert_eq!(transcript_path_for_session(&temp_dir, "foo\\bar"), None);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_limit_tail_and_paging() {
        let temp_dir = std::env::temp_dir().join(format!("test_omo_paging_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let file_path = temp_dir.join("transcript.jsonl");

        let mut lines = Vec::new();
        for i in 0..10 {
            lines.push(format!(
                r#"{{"type":"message","id":"msg-{}","message":{{"role":"user","content":[{{"type":"text","text":"item-{}"}}]}}}}"#,
                i, i
            ));
        }
        {
            let mut f = File::create(&file_path).unwrap();
            f.write_all(lines.join("\n").as_bytes()).unwrap();
        }

        let (tail, malformed) = read_conversation(&file_path, 3, None).unwrap();
        assert_eq!(malformed, 0);
        assert_eq!(tail.len(), 3);
        assert_eq!(tail[0].ordinal, 7);
        assert_eq!(tail[1].ordinal, 8);
        assert_eq!(tail[2].ordinal, 9);

        let (prev_page, _) = read_conversation(&file_path, 3, Some(7)).unwrap();
        assert_eq!(prev_page.len(), 3);
        assert_eq!(prev_page[0].ordinal, 4);
        assert_eq!(prev_page[1].ordinal, 5);
        assert_eq!(prev_page[2].ordinal, 6);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_multi_part_content_concatenation() {
        let temp_dir = std::env::temp_dir().join(format!("test_omo_multipart_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let file_path = temp_dir.join("transcript.jsonl");

        let content = r#"{"type":"message","id":"1","message":{"role":"assistant","content":[{"type":"text","text":"part1 "},{"type":"tool_call","data":"foo"},{"type":"text","text":"part2"}]}}
"#;
        {
            let mut f = File::create(&file_path).unwrap();
            f.write_all(content.as_bytes()).unwrap();
        }

        let (messages, _) = read_conversation(&file_path, 10, None).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].text, "part1 part2");

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_transcript_path_discovery() {
        let temp_dir = std::env::temp_dir().join(format!("test_omo_disc_{}", uuid::Uuid::new_v4()));
        let session_dir = temp_dir.join(".omo").join("agent").join("sessions").join("--some-project--");
        fs::create_dir_all(&session_dir).unwrap();

        let session_id = "01a0d650-db7a-7817-a21b-7be552a81e89";
        let target_file = session_dir.join(format!("2026-09-25T02-06-58-426Z_{session_id}.jsonl"));
        File::create(&target_file).unwrap();

        let found = transcript_path_for_session(&temp_dir, session_id);
        assert_eq!(found, Some(target_file));

        let not_found = transcript_path_for_session(&temp_dir, "non-existent-id");
        assert_eq!(not_found, None);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_slug_for_cwd() {
        assert_eq!(
            slug_for_cwd("/Volumes/T9-Mac/project/ferryx"),
            "--Volumes-T9-Mac-project-ferryx--"
        );
        assert_eq!(slug_for_cwd("/private/tmp"), "--private-tmp--");
        assert_eq!(
            slug_for_cwd("/private/tmp/cmuxhooktest"),
            "--private-tmp-cmuxhooktest--"
        );
    }

    #[test]
    fn test_latest_transcript_for_cwd() {
        let temp_home =
            std::env::temp_dir().join(format!("test_omo_home_{}", uuid::Uuid::new_v4()));
        let session_dir = temp_home
            .join(".omo")
            .join("agent")
            .join("sessions")
            .join("--private-tmp--");
        fs::create_dir_all(&session_dir).unwrap();

        let missing = latest_transcript_for_cwd(&temp_home, "/no/such/path", None);
        assert_eq!(missing, None);

        let older_file = session_dir.join("2026-09-25T00-00-00-000Z_agent-old.jsonl");
        let newer_file = session_dir.join("2026-09-25T00-00-00-001Z_agent-new.jsonl");
        File::create(&older_file).unwrap();
        File::create(&newer_file).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&older_file)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(std::time::UNIX_EPOCH))
            .unwrap();

        let newest = latest_transcript_for_cwd(&temp_home, "/private/tmp", None);
        assert_eq!(newest, Some(newer_file));

        let hint = "08c4edec-26e9-4f0a-8365-47bb6f23166c".to_string();
        let hinted_file =
            session_dir.join(format!("2026-09-25T00-00-00-000Z_agent-{hint}.jsonl"));
        File::create(&hinted_file).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&hinted_file)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(std::time::UNIX_EPOCH))
            .unwrap();

        let hinted = latest_transcript_for_cwd(&temp_home, "/private/tmp", Some(&hint));
        assert_eq!(hinted, Some(hinted_file));

        let _ = fs::remove_dir_all(&temp_home);
    }

    #[test]
    fn test_remote_transcript_dir_matches_the_paired_host_layout() {
        let dir = remote_transcript_dir("/home/indo", "/home/indo/piratetalk");
        assert_eq!(
            dir.as_deref(),
            Some("/home/indo/.omo/agent/sessions/--home-indo-piratetalk--")
        );

        let trailing = remote_transcript_dir("/home/indo/", "/home/indo/piratetalk");
        assert_eq!(trailing, dir);
    }

    #[test]
    fn test_remote_transcript_dir_refuses_hosts_whose_layout_is_unverified() {
        assert_eq!(
            remote_transcript_dir("C:\\Users\\sook", "C:\\Users\\sook\\work\\x"),
            None
        );
        assert_eq!(remote_transcript_dir("", "/home/indo/x"), None);
    }

    #[test]
    fn test_remote_target_resolves_from_a_real_store_record() {
        let store = r#"{
          "version": 3,
          "remoteSessions": [
            {
              "descriptor": {
                "backendSessionId": "08c4edec-26e9-4f0a-8365-47bb6f23166c",
                "clientRequestId": "14ea46ba-b849-4d24-86df-72628f96dd91",
                "cols": 59,
                "rows": 67,
                "config": {
                  "host": {
                    "authMethod": "agent",
                    "hostname": "100.91.254.71",
                    "id": "ssh-omarchy",
                    "label": "omarchy",
                    "source": "config",
                    "username": "indo"
                  },
                  "environment": { "home": "/home/indo", "platform": "posix" },
                  "projectPath": "/home/indo/piratetalk"
                }
              },
              "metadata": { "cwd": "/home/indo/piratetalk" }
            }
          ]
        }"#;

        let target = remote_target_from_store(store, "08c4edec-26e9-4f0a-8365-47bb6f23166c")
            .expect("the stored session resolves");
        assert_eq!(
            target.dir,
            "/home/indo/.omo/agent/sessions/--home-indo-piratetalk--"
        );
        assert_eq!(
            target.host.pointer("/hostname").and_then(|v| v.as_str()),
            Some("100.91.254.71")
        );

        assert!(
            remote_target_from_store(store, "not-a-stored-session").is_none(),
            "an unknown id must not resolve to whichever session happens to be first"
        );
        assert!(remote_target_from_store("not json", "x").is_none());
        assert!(remote_target_from_store("{}", "x").is_none());
        assert!(remote_target_from_store("{\"remoteSessions\":[]}", "x").is_none());
    }

    #[test]
    fn test_remote_target_refuses_a_record_missing_its_host_or_path() {
        let no_home = r#"{"remoteSessions":[{"descriptor":{"backendSessionId":"s1",
            "config":{"host":{"hostname":"h"},"environment":{},"projectPath":"/x"}}}]}"#;
        assert!(
            remote_target_from_store(no_home, "s1").is_none(),
            "an empty home must not build a command"
        );

        let no_host = r#"{"remoteSessions":[{"descriptor":{"backendSessionId":"s2",
            "config":{"environment":{"home":"/home/x"},"projectPath":"/x"}}}]}"#;
        assert!(remote_target_from_store(no_host, "s2").is_none());

        let windows = r#"{"remoteSessions":[{"descriptor":{"backendSessionId":"s3",
            "config":{"host":{"hostname":"h"},"environment":{"home":"C:\\Users\\sook"},
            "projectPath":"C:\\Users\\sook\\w"}}}]}"#;
        assert!(
            remote_target_from_store(windows, "s3").is_none(),
            "a Windows host must not produce a guessed slug"
        );
    }

    /// Exercises the remote path through the real functions, against the real paired host.
    ///
    /// Ignored by default: it needs this machine's SSH credentials and a reachable paired host, so it
    /// cannot run in CI. Run it deliberately with
    /// `cargo test --lib -- --ignored remote_transcript_over_ssh`.
    #[tokio::test]
    #[ignore = "requires a live paired host"]
    async fn remote_transcript_over_ssh() {
        let store_path = std::path::PathBuf::from(std::env::var_os("HOME").expect("HOME"))
            .join(".ferryx")
            .join("remote")
            .join("remote_sessions.json");
        let store = std::fs::read_to_string(&store_path).expect("paired-host store readable");
        let session_id = serde_json::from_str::<serde_json::Value>(&store)
            .expect("store parses")
            .get("remoteSessions")
            .and_then(|v| v.as_array())
            .and_then(|rows| rows.first())
            .and_then(|row| row.pointer("/descriptor/backendSessionId"))
            .and_then(|v| v.as_str())
            .expect("a stored remote session")
            .to_string();

        let target = remote_target_from_store(&store, &session_id)
            .expect("the stored session resolves to a remote target");
        assert!(
            target.dir.contains("/.omo/agent/sessions/"),
            "resolved dir looks like a transcript root: {}",
            target.dir
        );

        let host: crate::ssh::SshHost =
            serde_json::from_value(target.host).expect("stored host deserializes");
        let budget = 200usize.saturating_mul(4096).clamp(64 * 1024, 4 * 1024 * 1024);
        let script = format!(
            "d={dir}; f=$(ls -t \"$d\"/*.jsonl 2>/dev/null | head -n 1); \
             if [ -n \"$f\" ]; then \
               n=$(wc -c < \"$f\"); printf '%s\\n' \"$n\"; \
               if [ \"$n\" -gt {budget} ]; then tail -c {budget} \"$f\" | tail -n +2; else cat -- \"$f\"; fi; \
             fi",
            dir = crate::ssh::direct::quote_posix(&target.dir),
            budget = budget,
        );
        let command = format!("sh -c {}", crate::ssh::direct::quote_posix(&script));
        let plan = crate::ssh::direct::ssh_plan(&host, command, false).expect("ssh plan builds");
        let bytes = crate::ssh::direct::bounded_output_with_limit(
            &plan,
            std::time::Duration::from_secs(20),
            budget + 4096,
        )
        .await
        .expect("remote read succeeds");

        // The production script leads with the file's true size; the route splits that line off and
        // uses it to decide whether history was cut. This test mirrors the script exactly, so a drift
        // between the two shows up here rather than only in production.
        let split = bytes
            .iter()
            .position(|b| *b == b'\n')
            .expect("the size line is present");
        let reported_bytes: usize = std::str::from_utf8(&bytes[..split])
            .expect("size line is utf-8")
            .trim()
            .parse()
            .expect("size line is a number");
        assert!(
            reported_bytes > 0,
            "the host must report a real file size"
        );
        let truncated = reported_bytes > budget;
        assert!(
            truncated,
            "a real transcript ({reported_bytes} bytes) must exceed the {budget}-byte budget"
        );

        let body = &bytes[split + 1..];
        assert!(!body.is_empty(), "the paired host returned an empty transcript");
        let (items, malformed) = read_conversation_from_bytes(body, 200, None);
        assert!(
            items.len() > 10,
            "expected a real conversation, got {} messages",
            items.len()
        );
        assert_eq!(
            malformed, 0,
            "the remote side drops the partial first line, so nothing should be malformed"
        );
        assert!(
            items.iter().any(|m| m.role == "user" && !m.text.trim().is_empty()),
            "a real conversation contains a non-empty user turn"
        );
    }

    #[test]
    fn test_split_remote_size_line_parses_and_fails_closed() {
        let (size, body) = split_remote_size_line(b"11125993\n{\"type\":\"message\"}\n")
            .expect("a numeric first line parses");
        assert_eq!(size, 11_125_993);
        assert_eq!(body, b"{\"type\":\"message\"}\n");

        // Trailing whitespace from the remote printf must not break the parse.
        let (padded, _) = split_remote_size_line(b"42  \nbody").expect("padding tolerated");
        assert_eq!(padded, 42);

        // Each of these means "the host did not give us a usable transcript".
        assert!(split_remote_size_line(b"").is_none(), "empty payload");
        assert!(
            split_remote_size_line(b"no newline at all").is_none(),
            "a body with no size line"
        );
        assert!(
            split_remote_size_line(b"sh: 1: ls: not found\nbody").is_none(),
            "a shell error on stdout must not be read as a size"
        );
        assert!(
            split_remote_size_line(b"-5\nbody").is_none(),
            "a negative size is not a size"
        );
        assert!(
            split_remote_size_line(b"999999999999999999999999\nbody").is_none(),
            "overflow is not a size"
        );

        // A real zero-byte transcript still parses; the caller decides it has no body.
        let (zero, empty_body) = split_remote_size_line(b"0\n").expect("zero parses");
        assert_eq!(zero, 0);
        assert!(empty_body.is_empty());
    }

    #[test]
    fn test_read_conversation_from_bytes_parses_remote_output() {
        let transcript = concat!(
            r#"{"type":"message","id":"u1","message":{"role":"user","content":[{"type":"text","text":"first prompt"}]}}"#,
            "\n",
            r#"{"type":"tool_use","id":"t1","message":{"role":"assistant"}}"#,
            "\n",
            r#"{"type":"message","id":"a1","display":false,"message":{"role":"assistant","content":[{"type":"text","text":"hidden"}]}}"#,
            "\n",
            r#"{"type":"message","id":"a2","message":{"role":"assistant","content":[{"type":"text","text":"the answer"}]}}"#,
            "\n",
            "not json at all\n",
        );

        let (items, malformed) = read_conversation_from_bytes(transcript.as_bytes(), 200, None);
        assert_eq!(items.len(), 2, "only message records with text are kept");
        assert_eq!(items[0].role, "user");
        assert_eq!(items[0].text, "first prompt");
        assert_eq!(items[1].text, "the answer");
        assert_eq!(malformed, 1, "the non-JSON line is counted, not fatal");

        let (tail, _) = read_conversation_from_bytes(transcript.as_bytes(), 1, None);
        assert_eq!(tail.len(), 1);
        assert_eq!(tail[0].text, "the answer");

        let (older, _) = read_conversation_from_bytes(transcript.as_bytes(), 200, Some(1));
        assert_eq!(older.len(), 1);
        assert_eq!(older[0].text, "first prompt");
    }

    #[test]
    fn test_conversation_message_timestamp_passthrough_and_tolerance() {
        let transcript = concat!(
            r#"{"type":"message","id":"msg-1","timestamp":"2026-09-25T02:06:58.426Z","message":{"role":"user","content":[{"type":"text","text":"hello"}]}}"#,
            "\n",
            r#"{"type":"message","id":"msg-2","timestamp":"2026-09-25T02:07:05.100Z","message":{"role":"assistant","content":[{"type":"text","text":"world"}]}}"#,
            "\n",
            r#"{"type":"message","id":"msg-3","message":{"role":"user","content":[{"type":"text","text":"no timestamp"}]}}"#,
            "\n",
        );

        let (messages, malformed) = read_conversation_from_bytes(transcript.as_bytes(), 10, None);
        assert_eq!(malformed, 0);
        assert_eq!(messages.len(), 3);

        assert_eq!(
            messages[0].timestamp.as_deref(),
            Some("2026-09-25T02:06:58.426Z")
        );
        assert_eq!(
            messages[1].timestamp.as_deref(),
            Some("2026-09-25T02:07:05.100Z")
        );
        assert_eq!(messages[2].timestamp, None);
    }

    #[test]
    fn test_tool_call_and_tool_result_formatting() {
        let transcript = concat!(
            r#"{"type":"message","id":"msg-1","message":{"role":"assistant","content":[{"type":"text","text":"checking"},{"type":"toolCall","id":"c1","name":"bash","arguments":{}}]}}"#,
            "\n",
            r#"{"type":"message","id":"msg-2","message":{"role":"toolResult","toolName":"bash","content":[{"type":"text","text":"ok"}]}}"#,
            "\n",
            r#"{"type":"message","id":"msg-3","message":{"role":"toolResult","toolName":"bash"}}"#,
            "\n",
        );

        let (messages, malformed) = read_conversation_from_bytes(transcript.as_bytes(), 10, None);
        assert_eq!(malformed, 0);
        assert_eq!(messages.len(), 3);

        assert!(messages[0].text.contains("checking"));
        assert!(messages[0].text.contains("→ bash"));
        assert_eq!(messages[0].text, "checking\n→ bash");
        assert_eq!(messages[1].text, "ok");
        assert_eq!(messages[2].text, "← bash result");
    }

    #[test]
    fn test_conversation_generation_stable_across_pagination() {
        let transcript = concat!(
            r#"{"type":"session","id":"sess-page-test","timestamp":"2026-10-03T10:00:00Z"}"#,
            "\n",
            r#"{"type":"message","id":"msg-0","message":{"role":"user","content":[{"type":"text","text":"zero"}]}}"#,
            "\n",
            r#"{"type":"message","id":"msg-1","message":{"role":"assistant","content":[{"type":"text","text":"one"}]}}"#,
            "\n",
            r#"{"type":"message","id":"msg-2","message":{"role":"user","content":[{"type":"text","text":"two"}]}}"#,
            "\n",
            r#"{"type":"message","id":"msg-3","message":{"role":"assistant","content":[{"type":"text","text":"three"}]}}"#,
            "\n",
            r#"{"type":"message","id":"msg-4","message":{"role":"user","content":[{"type":"text","text":"four"}]}}"#,
            "\n",
        );

        let (msgs_p1, _, gen_p1) = read_conversation_bytes_with_generation(
            transcript.as_bytes(),
            Some("sess-page-test"),
            "ferryx-sess",
            2,
            None,
        );
        let (msgs_p2, _, gen_p2) = read_conversation_bytes_with_generation(
            transcript.as_bytes(),
            Some("sess-page-test"),
            "ferryx-sess",
            2,
            Some(3),
        );

        assert_eq!(msgs_p1.len(), 2);
        assert_eq!(msgs_p2.len(), 2);
        assert_ne!(msgs_p1[0].ordinal, msgs_p2[0].ordinal);
        assert_eq!(gen_p1, gen_p2, "pagination window must not change conversationGeneration");
    }

    #[test]
    fn test_conversation_generation_stable_across_append() {
        let initial = concat!(
            r#"{"type":"session","id":"sess-append-test","timestamp":"2026-10-03T10:00:00Z"}"#,
            "\n",
            r#"{"type":"message","id":"msg-0","message":{"role":"user","content":[{"type":"text","text":"hello"}]}}"#,
            "\n",
            r#"{"type":"message","id":"msg-1","message":{"role":"assistant","content":[{"type":"text","text":"hi"}]}}"#,
            "\n",
        );

        let appended = concat!(
            r#"{"type":"session","id":"sess-append-test","timestamp":"2026-10-03T10:00:00Z"}"#,
            "\n",
            r#"{"type":"message","id":"msg-0","message":{"role":"user","content":[{"type":"text","text":"hello"}]}}"#,
            "\n",
            r#"{"type":"message","id":"msg-1","message":{"role":"assistant","content":[{"type":"text","text":"hi"}]}}"#,
            "\n",
            r#"{"type":"message","id":"msg-2","message":{"role":"user","content":[{"type":"text","text":"how are you"}]}}"#,
            "\n",
            r#"{"type":"message","id":"msg-3","message":{"role":"assistant","content":[{"type":"text","text":"doing well"}]}}"#,
            "\n",
        );

        let (_, _, gen_initial) = read_conversation_bytes_with_generation(
            initial.as_bytes(),
            Some("sess-append-test"),
            "ferryx-sess",
            10,
            None,
        );
        let (_, _, gen_appended) = read_conversation_bytes_with_generation(
            appended.as_bytes(),
            Some("sess-append-test"),
            "ferryx-sess",
            10,
            None,
        );

        assert_eq!(gen_initial, gen_appended, "append alone must not rotate or reset conversationGeneration");
    }

    #[test]
    fn test_conversation_generation_same_prefix_rewrite_rotates() {
        let session_id = format!("sess-rewrite-{}", uuid::Uuid::new_v4());
        let provider_id = "prov-rewrite-test";

        let transcript_a = concat!(
            r#"{"type":"session","id":"prov-rewrite-test","timestamp":"2026-10-03T10:00:00Z"}"#,
            "\n",
            r#"{"type":"message","id":"msg-0","message":{"role":"user","content":[{"type":"text","text":"identical first prompt"}]}}"#,
            "\n",
            r#"{"type":"message","id":"msg-1","message":{"role":"assistant","content":[{"type":"text","text":"original reply"}]}}"#,
            "\n",
        );

        // Same-first-row rewrite: msg-0 is byte-for-byte identical, but msg-1 is rewritten
        let transcript_b = concat!(
            r#"{"type":"session","id":"prov-rewrite-test","timestamp":"2026-10-03T10:00:00Z"}"#,
            "\n",
            r#"{"type":"message","id":"msg-0","message":{"role":"user","content":[{"type":"text","text":"identical first prompt"}]}}"#,
            "\n",
            r#"{"type":"message","id":"msg-1","message":{"role":"assistant","content":[{"type":"text","text":"REWRITTEN reply"}]}}"#,
            "\n",
        );

        let (_, _, gen_a) = read_conversation_bytes_with_generation(
            transcript_a.as_bytes(),
            Some(provider_id),
            &session_id,
            10,
            None,
        );
        let (_, _, gen_b) = read_conversation_bytes_with_generation(
            transcript_b.as_bytes(),
            Some(provider_id),
            &session_id,
            10,
            None,
        );

        assert_ne!(gen_a, gen_b, "same-first-row rewrite must rotate conversationGeneration");
    }

    #[test]
    fn test_conversation_generation_truncation_rotates() {
        let session_id = format!("sess-trunc-{}", uuid::Uuid::new_v4());
        let provider_id = "prov-trunc-test";

        let full = concat!(
            r#"{"type":"session","id":"prov-trunc-test","timestamp":"2026-10-03T10:00:00Z"}"#,
            "\n",
            r#"{"type":"message","id":"msg-0","message":{"role":"user","content":[{"type":"text","text":"first turn"}]}}"#,
            "\n",
            r#"{"type":"message","id":"msg-1","message":{"role":"assistant","content":[{"type":"text","text":"second turn"}]}}"#,
            "\n",
            r#"{"type":"message","id":"msg-2","message":{"role":"assistant","content":[{"type":"text","text":"third turn to truncate"}]}}"#,
            "\n",
        );

        // Tail-truncated: msg-0 and msg-1 are identical, msg-2 deleted
        let tail_truncated = concat!(
            r#"{"type":"session","id":"prov-trunc-test","timestamp":"2026-10-03T10:00:00Z"}"#,
            "\n",
            r#"{"type":"message","id":"msg-0","message":{"role":"user","content":[{"type":"text","text":"first turn"}]}}"#,
            "\n",
            r#"{"type":"message","id":"msg-1","message":{"role":"assistant","content":[{"type":"text","text":"second turn"}]}}"#,
            "\n",
        );

        let (_, _, gen_full) = read_conversation_bytes_with_generation(
            full.as_bytes(),
            Some(provider_id),
            &session_id,
            10,
            None,
        );
        let (_, _, gen_truncated) = read_conversation_bytes_with_generation(
            tail_truncated.as_bytes(),
            Some(provider_id),
            &session_id,
            10,
            None,
        );

        assert_ne!(gen_full, gen_truncated, "tail truncation must rotate conversationGeneration");
    }

    #[test]
    fn test_exact_provider_path_missing_provider_binding_fails_closed() {
        let temp_dir = std::env::temp_dir().join(format!("test_prov_missing_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();

        assert_eq!(exact_transcript_path_for_provider(&temp_dir, None), None);
        assert_eq!(exact_transcript_path_for_provider(&temp_dir, Some("")), None);
        assert_eq!(exact_transcript_path_for_provider(&temp_dir, Some("../bad")), None);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_exact_remote_command_missing_provider_binding_fails_closed() {
        assert_eq!(exact_remote_transcript_command("/home/indo/proj", None, 65536), None);
        assert_eq!(exact_remote_transcript_command("/home/indo/proj", Some(""), 65536), None);
        assert_eq!(exact_remote_transcript_command("/home/indo/proj", Some("../invalid"), 65536), None);

        let cmd = exact_remote_transcript_command("/home/indo/proj", Some("01a0d650-db7a"), 65536)
            .expect("valid command generated");
        assert!(cmd.contains("01a0d650-db7a"));
        assert!(!cmd.contains("ls -t"), "command must never use ls -t recency fallback");
    }

    #[test]
    fn test_wrong_session_or_host_fails_closed() {
        let store = r#"{
          "version": 3,
          "remoteSessions": [
            {
              "descriptor": {
                "backendSessionId": "correct-session-id",
                "config": {
                  "host": { "hostname": "100.91.254.71" },
                  "environment": { "home": "/home/indo" },
                  "projectPath": "/home/indo/project"
                }
              }
            }
          ]
        }"#;

        assert!(remote_target_from_store(store, "wrong-session-id").is_none());
        assert!(remote_target_from_store(store, "correct-session-id").is_some());
    }
}
