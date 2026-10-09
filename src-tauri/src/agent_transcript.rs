use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationMessage {
    pub ordinal: usize,
    pub role: String,
    pub text: String,
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    /// Reasoning text from `thinking` parts of an assistant record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
    /// Tool calls made by an assistant record, in order. Kept out of `text` so a client can
    /// render them as tool blocks and pair each one with its result.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCallSummary>,
    /// On a `toolResult` record: the id of the call it answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// On a `toolResult` record: the tool that produced it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    /// On a `toolResult` record: whether the tool reported an error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallSummary {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    /// The call's own one-line description (`summary` / `description` argument), when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// The main argument to show: code for `eval`, the command for `bash`, else the path/query or
    /// the compact JSON of all arguments. Bounded by [`TOOL_INPUT_LIMIT`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
}

/// Characters of a tool call's input sent to the client. The client polls every few seconds, so a
/// multi-kilobyte prompt or file body must not ride along in full on every call.
const TOOL_INPUT_LIMIT: usize = 4000;
const TOOL_SUMMARY_LIMIT: usize = 200;

fn truncate_chars(value: &str, limit: usize) -> String {
    match value.char_indices().nth(limit) {
        Some((cut, _)) => format!("{}\n…", &value[..cut]),
        None => value.to_string(),
    }
}

fn tool_call_summary(arguments: &serde_json::Value) -> Option<String> {
    ["summary", "description"]
        .iter()
        .find_map(|key| arguments.get(*key).and_then(|v| v.as_str()))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| truncate_chars(s, TOOL_SUMMARY_LIMIT))
}

fn tool_call_input(arguments: &serde_json::Value) -> Option<String> {
    let direct = [
        "code", "command", "cmd", "pattern", "query", "url", "path", "file_path", "prompt",
    ]
    .iter()
    .find_map(|key| arguments.get(*key).and_then(|v| v.as_str()))
    .map(str::to_string);
    let raw = match direct {
        Some(value) => value,
        None => {
            if arguments.as_object().map_or(true, |object| object.is_empty()) {
                return None;
            }
            serde_json::to_string(arguments).ok()?
        }
    };
    if raw.trim().is_empty() {
        return None;
    }
    Some(truncate_chars(&raw, TOOL_INPUT_LIMIT))
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
    #[serde(rename = "toolCallId")]
    tool_call_id: Option<String>,
    #[serde(rename = "isError")]
    is_error: Option<bool>,
}

#[derive(Deserialize)]
struct ContentPart {
    #[serde(rename = "type")]
    part_type: Option<String>,
    text: Option<String>,
    thinking: Option<String>,
    id: Option<String>,
    name: Option<String>,
    arguments: Option<serde_json::Value>,
}

pub fn read_conversation(
    path: &Path,
    limit: usize,
    before: Option<usize>,
) -> Result<(Vec<ConversationMessage>, usize), String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    Ok(parse_conversation(BufReader::new(file), limit, before))
}

/// Same filtering and windowing as [`read_conversation`], over bytes already in memory.
/// A remote transcript arrives as command output rather than a local file, so the reader
/// path and the parser are separate.
pub fn read_conversation_from_bytes(
    bytes: &[u8],
    limit: usize,
    before: Option<usize>,
) -> (Vec<ConversationMessage>, usize) {
    parse_conversation(std::io::Cursor::new(bytes), limit, before)
}

fn parse_conversation<R: BufRead>(
    reader: R,
    limit: usize,
    before: Option<usize>,
) -> (Vec<ConversationMessage>, usize) {
    let mut all_messages: Vec<ConversationMessage> = Vec::new();
    let mut malformed_lines = 0;
    let mut current_ordinal = 0;

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
        let mut thinking = String::new();
        let mut tool_calls: Vec<ToolCallSummary> = Vec::new();

        if let Some(content) = body.content {
            if let Some(s) = content.as_str() {
                text.push_str(s);
            } else if let Some(arr) = content.as_array() {
                for item in arr {
                    if let Ok(part) = serde_json::from_value::<ContentPart>(item.clone()) {
                        let kind = part.part_type.clone();
                        match kind.as_deref() {
                            Some("text") => {
                                if let Some(t) = part.text {
                                    text.push_str(&t);
                                }
                            }
                            Some("thinking") => {
                                if let Some(t) = part.thinking.or(part.text) {
                                    if !t.trim().is_empty() {
                                        if !thinking.is_empty() {
                                            thinking.push_str("\n\n");
                                        }
                                        thinking.push_str(&t);
                                    }
                                }
                            }
                            Some("toolCall") => {
                                let arguments = part.arguments.unwrap_or(serde_json::Value::Null);
                                tool_calls.push(ToolCallSummary {
                                    id: part.id,
                                    name: part.name.unwrap_or_else(|| "tool".to_string()),
                                    summary: tool_call_summary(&arguments),
                                    input: tool_call_input(&arguments),
                                });
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        let is_tool_result = role == "toolResult";
        let tool_name = if is_tool_result { body.tool_name.clone() } else { None };

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
            thinking: (!thinking.is_empty()).then_some(thinking),
            tool_calls,
            tool_call_id: if is_tool_result { body.tool_call_id } else { None },
            tool_name,
            is_error: if is_tool_result { body.is_error } else { None },
        });
        current_ordinal += 1;
    }

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

    (result_messages, malformed_lines)
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

        // The call is structured, not flattened into the prose.
        assert_eq!(messages[0].text, "checking");
        assert_eq!(messages[0].tool_calls.len(), 1);
        assert_eq!(messages[0].tool_calls[0].name, "bash");
        assert_eq!(messages[0].tool_calls[0].id.as_deref(), Some("c1"));
        assert_eq!(messages[1].text, "ok");
        assert_eq!(messages[1].tool_name.as_deref(), Some("bash"));
        assert_eq!(messages[2].text, "← bash result");
    }

    #[test]
    fn test_thinking_and_tool_call_arguments_are_structured() {
        let transcript = concat!(
            r#"{"type":"message","id":"a1","message":{"role":"assistant","content":[{"type":"thinking","thinking":"plan the probe","thinkingSignature":"x"},{"type":"text","text":"Checking now."},{"type":"toolCall","id":"call_1","name":"eval","arguments":{"code":"print(1)","language":"js","summary":"Probe the daemon"}},{"type":"toolCall","id":"call_2","name":"read","arguments":{"path":"/tmp/a.txt","offset":1}}]}}"#,
            "\n",
            r#"{"type":"message","id":"r1","message":{"role":"toolResult","toolCallId":"call_1","toolName":"eval","isError":true,"content":[{"type":"text","text":"boom"}]}}"#,
            "\n",
        );

        let (messages, malformed) = read_conversation_from_bytes(transcript.as_bytes(), 10, None);
        assert_eq!(malformed, 0);
        assert_eq!(messages.len(), 2);

        let assistant = &messages[0];
        assert_eq!(assistant.text, "Checking now.");
        assert_eq!(assistant.thinking.as_deref(), Some("plan the probe"));
        assert_eq!(
            assistant.tool_calls,
            vec![
                ToolCallSummary {
                    id: Some("call_1".into()),
                    name: "eval".into(),
                    summary: Some("Probe the daemon".into()),
                    input: Some("print(1)".into()),
                },
                ToolCallSummary {
                    id: Some("call_2".into()),
                    name: "read".into(),
                    summary: None,
                    input: Some("/tmp/a.txt".into()),
                },
            ]
        );
        assert_eq!(assistant.tool_call_id, None);

        let result = &messages[1];
        assert_eq!(result.tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(result.tool_name.as_deref(), Some("eval"));
        assert_eq!(result.is_error, Some(true));
        assert_eq!(result.text, "boom");

        let json = serde_json::to_value(assistant).unwrap();
        assert_eq!(json["toolCalls"][0]["input"], "print(1)");
        assert!(json.get("toolCallId").is_none(), "absent fields must not be serialized");
    }

    #[test]
    fn test_tool_input_is_bounded() {
        let long = "x".repeat(TOOL_INPUT_LIMIT + 50);
        let args = serde_json::json!({ "code": long });
        let input = tool_call_input(&args).unwrap();
        assert_eq!(input.chars().count(), TOOL_INPUT_LIMIT + 2);
        assert!(input.ends_with("\n…"));
    }
}
