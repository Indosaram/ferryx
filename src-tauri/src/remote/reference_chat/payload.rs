//! Bounded lazy payload resolution (T14).
//!
//! A payload ref is opaque. It names no host path. The owning host re-reads the native
//! transcript, checks the ref against the current generation and the turn/part/call it names,
//! and returns that record's body or a typed refusal. A stale generation is `TARGET_EXPIRED`.
//! A missing part is `NOT_FOUND`. Nothing here guesses a neighbouring record.

use serde::Serialize;
use serde_json::{Map, Value};

use crate::scoped_contracts::{ScopeError, ScopeErrorCode};

use super::history::{
    ensure_reference_history_owner, read_reference_history_window, reference_history_page_from_window,
    ReferenceHistoryLocation, ReferenceHistoryStream, REFERENCE_HISTORY_MAX_LIMIT,
    ReferenceHistoryIdentity, REFERENCE_HISTORY_MAX_PI_BYTES,
};
use super::history_parse::reference_native_patch_body;
use super::types::{
    ReferenceNativeHistoryKind, ReferencePart, ReferencePayloadRef, ReferenceTurn,
};

/// The largest body this route returns. A larger record is a typed truncated response, not an
/// unbounded read.
pub const REFERENCE_PAYLOAD_MAX_BYTES: usize = 2 * 1024 * 1024;

const REF_PREFIX: &str = "fp1.";
const REF_MAX_LEN: usize = 8 * 1024;
const FIELD_MAX_LEN: usize = 512;
const WALK_DEPTH: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PayloadKind {
    Output,
    Diff,
}

impl PayloadKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Output => "output",
            Self::Diff => "diff",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "output" => Some(Self::Output),
            "diff" => Some(Self::Diff),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PayloadClaim {
    kind: PayloadKind,
    generation: String,
    turn_id: String,
    part_id: String,
    call_id: String,
    digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FoundBody {
    kind: PayloadKind,
    call_id: Option<String>,
    body: String,
}

/// The body a valid ref resolved to. `truncated` means the route cut `body` at the cap; `digest`
/// and `byte_length` still describe the whole record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferencePayloadResponse {
    pub status: String,
    pub kind: String,
    pub byte_length: u64,
    pub digest: String,
    pub cap: u64,
    pub truncated: bool,
    pub body: String,
}

fn scope(code: ScopeErrorCode, message: &str) -> ScopeError {
    ScopeError {
        code,
        message: message.to_string(),
        retryable: false,
        details: Value::Null,
    }
}

fn scrubbed(code: ScopeErrorCode) -> ScopeError {
    let message = match code {
        ScopeErrorCode::PayloadTooLarge => "the transcript is past the read cap",
        ScopeErrorCode::NotFound => "the transcript could not be read",
        ScopeErrorCode::TargetExpired => "the payload ref is stale",
        ScopeErrorCode::InvalidRequest => "the payload ref is not readable",
        _ => "the payload ref could not be resolved",
    };
    scope(code, message)
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn field_is_locator_safe(value: &str, allow_empty: bool) -> bool {
    if value.len() > FIELD_MAX_LEN || (!allow_empty && value.is_empty()) {
        return false;
    }
    !value.contains('/')
        && !value.contains('\\')
        && !value.contains("..")
        && !value.chars().any(char::is_control)
}

fn encode_claim(claim: &PayloadClaim) -> String {
    let json = serde_json::json!({
        "v": 1,
        "k": claim.kind.as_str(),
        "g": claim.generation,
        "t": claim.turn_id,
        "p": claim.part_id,
        "c": claim.call_id,
        "d": claim.digest,
    });
    let bytes = serde_json::to_vec(&json).unwrap_or_default();
    use base64::Engine as _;
    format!(
        "{REF_PREFIX}{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    )
}

fn decode_claim(raw: &str) -> Result<PayloadClaim, ScopeError> {
    let raw = raw.trim();
    if raw.is_empty()
        || raw.len() > REF_MAX_LEN
        || raw.contains('/')
        || raw.contains('\\')
        || raw.contains("..")
        || !raw.starts_with(REF_PREFIX)
    {
        return Err(scope(ScopeErrorCode::InvalidRequest, "the payload ref is not readable"));
    }
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(&raw[REF_PREFIX.len()..])
        .map_err(|_| scope(ScopeErrorCode::InvalidRequest, "the payload ref is not readable"))?;
    if bytes.len() > REF_MAX_LEN {
        return Err(scope(ScopeErrorCode::InvalidRequest, "the payload ref is not readable"));
    }
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|_| scope(ScopeErrorCode::InvalidRequest, "the payload ref is not readable"))?;
    let object = value
        .as_object()
        .ok_or_else(|| scope(ScopeErrorCode::InvalidRequest, "the payload ref is not readable"))?;
    if object.get("v").and_then(Value::as_u64) != Some(1) {
        return Err(scope(ScopeErrorCode::InvalidRequest, "the payload ref is not readable"));
    }
    let kind = object
        .get("k")
        .and_then(Value::as_str)
        .and_then(PayloadKind::parse)
        .ok_or_else(|| scope(ScopeErrorCode::InvalidRequest, "the payload ref is not readable"))?;
    let generation = required_field(object, "g", false)?;
    let turn_id = required_field(object, "t", false)?;
    let part_id = required_field(object, "p", false)?;
    let call_id = required_field(object, "c", true)?;
    let digest = required_field(object, "d", false)?;
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(scope(ScopeErrorCode::InvalidRequest, "the payload ref is not readable"));
    }
    Ok(PayloadClaim {
        kind,
        generation,
        turn_id,
        part_id,
        call_id,
        digest,
    })
}

fn required_field(object: &Map<String, Value>, key: &str, allow_empty: bool) -> Result<String, ScopeError> {
    let value = object
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| scope(ScopeErrorCode::InvalidRequest, "the payload ref is not readable"))?;
    if !field_is_locator_safe(value, allow_empty) {
        return Err(scope(ScopeErrorCode::InvalidRequest, "the payload ref is not readable"));
    }
    Ok(value.to_string())
}

fn call_id_of(object: &Map<String, Value>) -> Option<String> {
    ["call_id", "callId", "tool_use_id", "toolUseId"]
        .iter()
        .find_map(|key| {
            object
                .get(*key)
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty() && field_is_locator_safe(id, false))
                .map(str::to_string)
        })
}

fn text_of(value: &Value) -> Option<String> {
    match value {
        Value::String(text) if !text.is_empty() => Some(text.clone()),
        Value::Array(items) => {
            let mut parts = Vec::new();
            for item in items {
                if let Some(text) = item.as_str().filter(|text| !text.is_empty()) {
                    parts.push(text.to_string());
                    continue;
                }
                if let Some(text) = item.get("text").and_then(Value::as_str).filter(|text| !text.is_empty()) {
                    parts.push(text.to_string());
                }
            }
            (!parts.is_empty()).then(|| parts.join(""))
        }
        _ => None,
    }
}

fn record_output(found: &mut Vec<FoundBody>, call_id: Option<&str>, body: String) {
    let Some(call_id) = call_id.filter(|id| !id.is_empty()) else {
        return;
    };
    if body.is_empty() {
        return;
    }
    if found.iter().any(|held| {
        held.kind == PayloadKind::Output && held.call_id.as_deref() == Some(call_id) && held.body == body
    }) {
        return;
    }
    found.push(FoundBody {
        kind: PayloadKind::Output,
        call_id: Some(call_id.to_string()),
        body,
    });
}

fn record_diff(found: &mut Vec<FoundBody>, call_id: Option<&str>, body: String) {
    if body.is_empty() {
        return;
    }
    if found.iter().any(|held| held.kind == PayloadKind::Diff && held.body == body && held.call_id.as_deref() == call_id)
    {
        return;
    }
    found.push(FoundBody {
        kind: PayloadKind::Diff,
        call_id: call_id.map(str::to_string),
        body,
    });
}

fn walk_value(value: &Value, inherited_call: Option<&str>, depth: usize, found: &mut Vec<FoundBody>) {
    if depth > WALK_DEPTH {
        return;
    }
    let Some(object) = value.as_object() else {
        if let Some(items) = value.as_array() {
            for item in items {
                walk_value(item, inherited_call, depth + 1, found);
            }
        }
        return;
    };
    let own_call = call_id_of(object);
    let call = own_call.as_deref().or(inherited_call);
    let type_name = object.get("type").and_then(Value::as_str).unwrap_or("");
    if call.is_some()
        && matches!(
            type_name,
            "function_call_output" | "custom_tool_call_output" | "tool_result" | "tool_result_output"
        )
    {
        if let Some(body) = object.get("output").and_then(text_of) {
            record_output(found, call, body);
        }
        if let Some(body) = object.get("content").and_then(text_of) {
            record_output(found, call, body);
        }
    } else if call.is_some() {
        if let Some(body) = object.get("output").and_then(text_of) {
            record_output(found, call, body);
        }
    }
    if let Some(name) = object.get("name").and_then(Value::as_str) {
        if let Some(body) = reference_native_patch_body(name, &Value::Object(object.clone())) {
            record_diff(found, call, body);
        }
        for key in ["input", "arguments", "patch"] {
            if let Some(payload) = object.get(key) {
                if let Some(body) = reference_native_patch_body(name, payload) {
                    record_diff(found, call, body);
                }
            }
        }
    }
    for child in object.values() {
        walk_value(child, call, depth + 1, found);
    }
}

fn collect_bodies(bytes: &[u8]) -> Vec<FoundBody> {
    let mut found = Vec::new();
    let mut line_start = 0usize;
    while line_start < bytes.len() {
        let line_end = bytes[line_start..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|index| line_start + index)
            .unwrap_or(bytes.len());
        let mut line = &bytes[line_start..line_end];
        if line.last() == Some(&b'\r') {
            line = &line[..line.len() - 1];
        }
        if !line.is_empty() {
            if let Ok(value) = serde_json::from_slice::<Value>(line) {
                walk_value(&value, None, 0, &mut found);
            }
        }
        line_start = line_end + 1;
    }
    found
}

fn unique_text<'a>(bodies: &[&'a FoundBody]) -> Option<&'a str> {
    let first = bodies.first()?;
    bodies
        .iter()
        .all(|body| body.body == first.body)
        .then_some(first.body.as_str())
}

fn part_id_of(part: &ReferencePart) -> Option<&str> {
    match part {
        ReferencePart::Tool { id, .. } | ReferencePart::Diff { id, .. } => id.as_deref(),
        _ => None,
    }
}

/// Stamp generation, turn, part and call identity onto payload refs this page can re-resolve.
/// A part whose body is not uniquely present in `bytes` keeps no fetchable ref.
pub(crate) fn bind_reference_payload_refs(generation: &str, bytes: &[u8], turns: &mut [ReferenceTurn]) {
    if !field_is_locator_safe(generation, false) {
        return;
    }
    let found = collect_bodies(bytes);
    for turn in turns {
        let Some(turn_id) = turn.id.clone().filter(|id| field_is_locator_safe(id, false)) else {
            continue;
        };
        for part in &mut turn.parts {
            match part {
                ReferencePart::Tool {
                    id,
                    call_id,
                    output_ref,
                    output_payload,
                    ..
                } => {
                    if output_ref.is_none() {
                        continue;
                    }
                    let Some(part_id) = id.clone().filter(|id| field_is_locator_safe(id, false)) else {
                        continue;
                    };
                    let call = call_id
                        .clone()
                        .filter(|id| field_is_locator_safe(id, false))
                        .or_else(|| output_ref.clone().filter(|id| field_is_locator_safe(id, false)));
                    let Some(call) = call else {
                        continue;
                    };
                    let matches: Vec<&FoundBody> = found
                        .iter()
                        .filter(|body| body.kind == PayloadKind::Output && body.call_id.as_deref() == Some(call.as_str()))
                        .collect();
                    let Some(body) = unique_text(&matches) else {
                        continue;
                    };
                    let digest = sha256_hex(body.as_bytes());
                    let claim = PayloadClaim {
                        kind: PayloadKind::Output,
                        generation: generation.to_string(),
                        turn_id: turn_id.clone(),
                        part_id,
                        call_id: call,
                        digest: digest.clone(),
                    };
                    **output_payload = Some(ReferencePayloadRef {
                        r#ref: encode_claim(&claim),
                        byte_length: body.len() as u64,
                        digest,
                        truncated: true,
                    });
                }
                ReferencePart::Diff { id, patch } => {
                    let Some(part_id) = id.clone().filter(|id| field_is_locator_safe(id, false)) else {
                        continue;
                    };
                    if !field_is_locator_safe(&patch.digest, false) {
                        continue;
                    }
                    let matches: Vec<&FoundBody> = found
                        .iter()
                        .filter(|body| {
                            body.kind == PayloadKind::Diff && sha256_hex(body.body.as_bytes()) == patch.digest
                        })
                        .collect();
                    let Some(body) = unique_text(&matches) else {
                        continue;
                    };
                    let call_id = matches
                        .iter()
                        .filter_map(|body| body.call_id.clone())
                        .collect::<Vec<_>>();
                    let call_id = (call_id.len() == matches.len() && call_id.iter().all(|id| id == &call_id[0]))
                        .then(|| call_id[0].clone())
                        .filter(|id| field_is_locator_safe(id, false))
                        .unwrap_or_default();
                    let claim = PayloadClaim {
                        kind: PayloadKind::Diff,
                        generation: generation.to_string(),
                        turn_id: turn_id.clone(),
                        part_id,
                        call_id,
                        digest: patch.digest.clone(),
                    };
                    patch.r#ref = encode_claim(&claim);
                    patch.byte_length = body.len() as u64;
                    let _ = body;
                }
                _ => {}
            }
        }
    }
}

fn utf8_prefix(body: &str, cap: usize) -> String {
    let mut end = cap.min(body.len());
    while end > 0 && !body.is_char_boundary(end) {
        end -= 1;
    }
    body[..end].to_string()
}

fn response_for(kind: PayloadKind, body: &str) -> ReferencePayloadResponse {
    let digest = sha256_hex(body.as_bytes());
    let truncated = body.len() > REFERENCE_PAYLOAD_MAX_BYTES;
    let returned = if truncated {
        utf8_prefix(body, REFERENCE_PAYLOAD_MAX_BYTES)
    } else {
        body.to_string()
    };
    ReferencePayloadResponse {
        status: if truncated { "truncated" } else { "loaded" }.to_string(),
        kind: kind.as_str().to_string(),
        byte_length: body.len() as u64,
        digest,
        cap: REFERENCE_PAYLOAD_MAX_BYTES as u64,
        truncated,
        body: returned,
    }
}

/// Resolve `raw_ref` against bytes already read from the owning transcript.
pub(crate) fn resolve_reference_payload_from_bytes(
    stream: &ReferenceHistoryStream,
    identity: &ReferenceHistoryIdentity,
    window_start: u64,
    bytes: &[u8],
    raw_ref: &str,
) -> Result<ReferencePayloadResponse, ScopeError> {
    let claim = decode_claim(raw_ref)?;
    ensure_reference_history_owner(stream, identity).map_err(|error| scrubbed(error.code))?;
    let page = reference_history_page_from_window(
        stream,
        identity,
        window_start,
        bytes,
        REFERENCE_HISTORY_MAX_LIMIT,
        None,
    )?;
    if page.generation != claim.generation {
        return Err(scope(
            ScopeErrorCode::TargetExpired,
            "the payload ref is from an older generation",
        ));
    }
    let mut matched = 0usize;
    for turn in &page.turns {
        if turn.id.as_deref() != Some(claim.turn_id.as_str()) {
            continue;
        }
        for part in &turn.parts {
            if part_id_of(part) != Some(claim.part_id.as_str()) {
                continue;
            }
            let digest_ok = match (&claim.kind, part) {
                (PayloadKind::Output, ReferencePart::Tool { output_payload, call_id, .. }) => {
                    match output_payload.as_ref() {
                        Some(payload) => {
                            payload.digest == claim.digest
                                && (claim.call_id.is_empty()
                                    || call_id.as_deref() == Some(claim.call_id.as_str()))
                        }
                        None => false,
                    }
                }
                (PayloadKind::Diff, ReferencePart::Diff { patch, .. }) => patch.digest == claim.digest,
                _ => false,
            };
            if digest_ok {
                matched += 1;
            }
        }
    }
    if matched != 1 {
        return Err(scope(ScopeErrorCode::NotFound, "the payload ref names no part in this generation"));
    }
    let found = collect_bodies(bytes);
    let bodies: Vec<&FoundBody> = found
        .iter()
        .filter(|body| {
            body.kind == claim.kind
                && sha256_hex(body.body.as_bytes()) == claim.digest
                && (claim.call_id.is_empty() || body.call_id.as_deref() == Some(claim.call_id.as_str()))
        })
        .collect();
    let Some(body) = unique_text(&bodies) else {
        return Err(scope(ScopeErrorCode::NotFound, "the payload ref names no part in this generation"));
    };
    let response = response_for(claim.kind, body);
    if response.digest != claim.digest {
        return Err(scope(ScopeErrorCode::NotFound, "the payload ref names no part in this generation"));
    }
    Ok(response)
}

fn load_projected_text(stream: &ReferenceHistoryStream) -> Result<(u64, Vec<u8>), ScopeError> {
    match &stream.location {
        ReferenceHistoryLocation::Rows { store, session_id } => {
            let text = if stream.kind == ReferenceNativeHistoryKind::Crush {
                super::history_crush::crush_store_projection(store, session_id)
            } else {
                super::history_opencode::opencode_store_projection(store, session_id)
            }
            .map_err(|_| scrubbed(ScopeErrorCode::NotFound))?;
            Ok((0, text.into_bytes()))
        }
        ReferenceHistoryLocation::File(path) if stream.kind == ReferenceNativeHistoryKind::Cline => {
            let length = std::fs::metadata(path)
                .map_err(|_| scrubbed(ScopeErrorCode::NotFound))?
                .len();
            if length > REFERENCE_HISTORY_MAX_PI_BYTES as u64 {
                return Err(scrubbed(ScopeErrorCode::PayloadTooLarge));
            }
            let document = std::fs::read_to_string(path).map_err(|_| scrubbed(ScopeErrorCode::NotFound))?;
            let projected = super::history_cline::cline_messages_projection(&document)
                .ok_or_else(|| scrubbed(ScopeErrorCode::NotFound))?;
            Ok((0, projected.into_bytes()))
        }
        ReferenceHistoryLocation::File(path) if stream.kind == ReferenceNativeHistoryKind::Cursor => {
            let projected = super::history_cursor::cursor_store_projection(path)
                .map_err(|_| scrubbed(ScopeErrorCode::NotFound))?;
            Ok((0, projected.into_bytes()))
        }
        ReferenceHistoryLocation::File(path) => {
            let window = read_reference_history_window(path, stream.kind).map_err(|error| scrubbed(error.code))?;
            Ok((window.start, window.bytes))
        }
        ReferenceHistoryLocation::Scrollback | ReferenceHistoryLocation::NotStarted => {
            Err(scope(ScopeErrorCode::NotFound, "this pane has no native transcript"))
        }
    }
}

/// Re-read the stream's native transcript and resolve `raw_ref`. Synchronous: the route runs this
/// through [`crate::ipc::run_blocking`].
pub fn read_and_resolve_reference_payload(
    stream: &ReferenceHistoryStream,
    identity: &ReferenceHistoryIdentity,
    raw_ref: &str,
) -> Result<ReferencePayloadResponse, ScopeError> {
    let (start, bytes) = load_projected_text(stream)?;
    resolve_reference_payload_from_bytes(stream, identity, start, &bytes, raw_ref)
}

/// A copy of an issued ref whose part id is not in the transcript. Route tests use it to prove
/// a missing part is `NOT_FOUND` rather than a guess.
#[cfg(test)]
pub(crate) fn reference_payload_ref_for_missing_part(raw: &str) -> String {
    let mut claim = decode_claim(raw).expect("issued payload ref");
    claim.part_id = "missing-part".to_string();
    encode_claim(&claim)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scoped_contracts::{Epoch, TargetRef};
    use super::super::history::reference_history_page_from_bytes;
    use super::super::types::{ReferenceHistoryAvailability, ReferenceHistorySource, ReferenceTargetRef};

    fn target() -> ReferenceTargetRef {
        ReferenceTargetRef::without_provider_session(TargetRef {
            host_id: "host".into(),
            owner_id: "owner".into(),
            epoch: Epoch(1),
            backend_session_id: "sess".into(),
        })
    }

    fn stream(owner: &ReferenceTargetRef, stream_id: &str) -> ReferenceHistoryStream {
        ReferenceHistoryStream {
            source: ReferenceHistorySource::CodexTranscript,
            kind: ReferenceNativeHistoryKind::Codex,
            availability: ReferenceHistoryAvailability::Native,
            stream_id: stream_id.into(),
            location: ReferenceHistoryLocation::NotStarted,
            unavailable_reason: None,
            owner: owner.clone(),
        }
    }

    fn identity(owner: &ReferenceTargetRef) -> ReferenceHistoryIdentity {
        ReferenceHistoryIdentity::new(owner.clone(), "codex")
    }

    fn transcript(user: &str, output: &str) -> String {
        let patch = "*** Begin Patch\n*** Update File: notes.txt\n@@\n-old\n+new\n*** End Patch";
        format!(
            "{}\n{}\n{}\n{}\n{}\n{}\n{}\n",
            serde_json::json!({"timestamp":"2026-10-06T09:00:00.000Z","type":"session_meta","payload":{"id":"s","cwd":"project","cli_version":"0.1.0"}}),
            serde_json::json!({"timestamp":"2026-10-06T09:00:01.000Z","type":"event_msg","payload":{"type":"task_started","turn_id":"t1"}}),
            serde_json::json!({"timestamp":"2026-10-06T09:00:01.100Z","type":"event_msg","payload":{"type":"user_message","kind":"plain","message":user}}),
            serde_json::json!({"timestamp":"2026-10-06T09:00:01.100Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":user}]}}),
            serde_json::json!({"timestamp":"2026-10-06T09:00:02.000Z","type":"response_item","payload":{"type":"function_call","name":"exec_command","arguments":"{\"cmd\":\"cat log\"}","call_id":"c-out"}}),
            serde_json::json!({"timestamp":"2026-10-06T09:00:03.000Z","type":"response_item","payload":{"type":"function_call_output","call_id":"c-out","output":output}}),
            serde_json::json!({"timestamp":"2026-10-06T09:00:04.000Z","type":"response_item","payload":{"type":"custom_tool_call","name":"apply_patch","call_id":"c-diff","input":patch}}),
        )
    }

    fn page_refs(stream_id: &str, bytes: &str) -> (String, String, String) {
        let owner = target();
        let page = reference_history_page_from_bytes(&stream(&owner, stream_id), &identity(&owner), bytes.as_bytes(), 50, None)
            .expect("page");
        let mut output_ref = None;
        let mut diff_ref = None;
        for turn in &page.turns {
            for part in &turn.parts {
                match part {
                    ReferencePart::Tool { output_payload, .. } => {
                        if let Some(payload) = output_payload.as_ref() {
                            output_ref = Some(payload.r#ref.clone());
                        }
                    }
                    ReferencePart::Diff { patch, .. } => diff_ref = Some(patch.r#ref.clone()),
                    _ => {}
                }
            }
        }
        (
            page.generation,
            output_ref.expect("output payload ref"),
            diff_ref.expect("diff payload ref"),
        )
    }

    #[test]
    fn a_bound_ref_returns_the_record_and_names_no_path() {
        let bytes = transcript("show the log", &"x".repeat(4_100));
        let owner = target();
        let stream_id = "payload-unit-bound-ref";
        let (_generation, output_ref, diff_ref) = page_refs(stream_id, &bytes);
        assert!(output_ref.starts_with(REF_PREFIX), "{output_ref}");
        assert!(!output_ref.contains('/'), "{output_ref}");
        assert!(!diff_ref.contains('/'), "{diff_ref}");
        let loaded = resolve_reference_payload_from_bytes(
            &stream(&owner, stream_id),
            &identity(&owner),
            0,
            bytes.as_bytes(),
            &diff_ref,
        )
        .expect("diff body");
        assert_eq!(loaded.status, "loaded");
        assert_eq!(loaded.kind, "diff");
        assert!(loaded.body.contains("*** Begin Patch"));
        assert!(!loaded.truncated);
        assert!(!loaded.body.contains("/Volumes") && !loaded.body.contains("transcript"));
        let output = resolve_reference_payload_from_bytes(
            &stream(&owner, stream_id),
            &identity(&owner),
            0,
            bytes.as_bytes(),
            &output_ref,
        )
        .expect("output body");
        assert_eq!(output.status, "loaded");
        assert_eq!(output.body, "x".repeat(4_100));
        assert_eq!(output.byte_length, 4_100);
    }

    #[test]
    fn a_stale_generation_is_expired_and_a_missing_part_is_not_found() {
        let bytes = transcript("show the log", &"y".repeat(4_100));
        let owner = target();
        let stream_id = "payload-unit-stale-missing";
        let (generation, output_ref, _) = page_refs(stream_id, &bytes);
        let claim = decode_claim(&output_ref).expect("claim");
        assert_eq!(claim.generation, generation);
        // Missing-part is the current generation. Resolve it before the rewrite: that
        // observation rotates this stream's process-wide generation, so a later read of
        // the original bytes would look expired even though the part is simply absent.
        let missing = encode_claim(&PayloadClaim {
            part_id: "missing-part".into(),
            ..claim
        });
        let absent = resolve_reference_payload_from_bytes(
            &stream(&owner, stream_id),
            &identity(&owner),
            0,
            bytes.as_bytes(),
            &missing,
        )
        .expect_err("missing");
        assert_eq!(absent.code, ScopeErrorCode::NotFound);
        let rewritten = transcript("show the log now", &"y".repeat(4_100));
        let stale = resolve_reference_payload_from_bytes(
            &stream(&owner, stream_id),
            &identity(&owner),
            0,
            rewritten.as_bytes(),
            &output_ref,
        )
        .expect_err("stale");
        assert_eq!(stale.code, ScopeErrorCode::TargetExpired);
        assert!(!stale.message.contains('/'));
    }

    #[test]
    fn an_oversize_body_is_truncated_at_the_cap_and_a_path_ref_is_refused() {
        let huge = "z".repeat(REFERENCE_PAYLOAD_MAX_BYTES + 32);
        let bytes = transcript("show the log", &huge);
        let owner = target();
        let stream_id = "payload-unit-oversize";
        let (_generation, output_ref, _) = page_refs(stream_id, &bytes);
        let truncated = resolve_reference_payload_from_bytes(
            &stream(&owner, stream_id),
            &identity(&owner),
            0,
            bytes.as_bytes(),
            &output_ref,
        )
        .expect("truncated");
        assert_eq!(truncated.status, "truncated");
        assert!(truncated.truncated);
        assert_eq!(truncated.cap, REFERENCE_PAYLOAD_MAX_BYTES as u64);
        assert_eq!(truncated.byte_length, huge.len() as u64);
        assert_eq!(truncated.body.len(), REFERENCE_PAYLOAD_MAX_BYTES);
        assert!(huge.starts_with(&truncated.body));
        assert_eq!(truncated.digest, sha256_hex(huge.as_bytes()));
        let path = resolve_reference_payload_from_bytes(
            &stream(&owner, stream_id),
            &identity(&owner),
            0,
            bytes.as_bytes(),
            "/etc/passwd",
        )
        .expect_err("path");
        assert_eq!(path.code, ScopeErrorCode::InvalidRequest);
        assert!(!path.message.contains("passwd"));
    }
}
