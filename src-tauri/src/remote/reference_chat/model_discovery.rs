//! Standalone parsers for Claude, Codex, and Grok capability probes (plan T07).
//!
//! This module is intentionally not declared from `mod.rs`; the integration owner wires
//! `pub mod model_discovery` later. Nothing here spawns a process or reads a PTY. Each
//! function refuses stdout larger than [`DISCOVERY_STDOUT_LIMIT`] and returns
//! [`DiscoveryError`] instead of panicking on input.
//!
//! Field names were read from Orca @ `b1b97e742fadd6c615192b26dd0678f59ec456eb`, not guessed:
//!
//! * Claude stream-json: `src/shared/claude-model-list-probe.ts` (`value`, `displayName`,
//!   `supportsEffort`, `supportedEffortLevels`, `resolvedModel`, `disabled`, nested
//!   `response.response.models`) and `src/main/claude/claude-control-responses.ts`
//!   (`response.request_id`, `response.subtype == "success"`). The probe stdin shape is
//!   `{type:"control_request", request_id, request:{subtype:"list_models"}}` in the same file.
//! * Codex app-server `model/list`: `src/main/codex/codex-structured-model-catalog.ts`
//!   (`result.data[]` with `model` then `id`, `displayName`, `supportedReasoningEfforts[].reasoningEffort`,
//!   `isDefault`, `hidden`).
//! * Grok text: `src/shared/grok-model-list-probe.ts` (exact header `Available models:`,
//!   `*`/`-` bullets, `(default)` marker) and `src/shared/model-id-label.ts` for labels.

use serde_json::Value;

/// 256 KiB. Larger probe stdout is refused before it is parsed.
pub const DISCOVERY_STDOUT_LIMIT: usize = 256 * 1024;

/// How long a short-lived discovery probe may run before it is killed.
///
/// Orca's catalog probe uses the same 15s bound (`AGENT_MODEL_CATALOG_PROBE_TIMEOUT_MS`).
pub const DISCOVERY_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// Claude program. The probe never uses a client-supplied path.
pub const CLAUDE_PROBE_PROGRAM: &str = "claude";

/// Codex program. `model/list` is JSON-RPC on stdin, not an argv flag.
pub const CODEX_PROBE_PROGRAM: &str = "codex";

/// Grok program. The listing is `grok models` text, not JSON.
pub const GROK_PROBE_PROGRAM: &str = "grok";

/// Claude `--print` stream-json `list_models` args, from Orca `buildClaudeModelListArgs()`
/// at `b1b97e742fadd6c615192b26dd0678f59ec456eb` (`src/shared/claude-model-list-probe.ts`).
pub const CLAUDE_PROBE_ARGS: &[&str] = &[
    "-p",
    "--input-format",
    "stream-json",
    "--output-format",
    "stream-json",
    "--verbose",
    "--strict-mcp-config",
    "--settings",
    "{\"disableAllHooks\":true}",
];

/// Short-lived read-only Codex app-server args, from Orca
/// `CODEX_SHORT_LIVED_PROBE_APP_SERVER_ARGS` (`src/main/codex-cli/codex-read-only-app-server-args.ts`).
pub const CODEX_PROBE_ARGS: &[&str] = &[
    "-c",
    "approval_policy=never",
    "-c",
    "features.plugins=false",
    "-s",
    "read-only",
    "-a",
    "never",
    "app-server",
];

/// `grok models`. Orca `GROK_MODEL_LIST_ARGS` (`src/shared/grok-model-list-probe.ts`).
pub const GROK_PROBE_ARGS: &[&str] = &["models"];

/// Stdin for the Claude probe. The request id is [`CLAUDE_LIST_MODELS_REQUEST_ID`].
pub fn claude_probe_stdin() -> String {
    format!(
        "{{\"type\":\"control_request\",\"request_id\":\"{CLAUDE_LIST_MODELS_REQUEST_ID}\",\"request\":{{\"subtype\":\"list_models\"}}}}\n"
    )
}

/// Stdin for the Codex probe: initialize, then `model/list`. The client name is Ferryx's.
///
/// The handshake shape is Orca's (`initializeCodexAppServerConnection`); the client identity
/// is not. A CLI that rejects the handshake fails discovery, and the controls stay hidden.
pub fn codex_probe_stdin() -> String {
    concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"ferryx","title":"Ferryx","version":"0.0.0"},"capabilities":{"experimentalApi":true,"requestAttestation":false,"mcpServerOpenaiFormElicitation":false,"extensions":{}}}}"#,
        "\n",
        r#"{"jsonrpc":"2.0","method":"initialized"}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":2,"method":"model/list","params":{"limit":100,"includeHidden":false}}"#,
        "\n",
    )
    .to_string()
}

/// Request id the short-lived Claude probe must send when its stdin is not echoed in stdout.
///
/// Orca hard-codes `orca-model-discovery` (`src/shared/claude-model-list-probe.ts`). Ferryx owns
/// its own id so a captured foreign probe cannot be mistaken for this one. When stdout itself
/// contains the `control_request`, that line's `request_id` wins and this constant is not consulted.
pub const CLAUDE_LIST_MODELS_REQUEST_ID: &str = "ferryx-list-models";

/// One model the probe is willing to publish. `efforts` is empty when the CLI listed none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredModel {
    pub id: String,
    pub label: String,
    pub efforts: Vec<String>,
}

/// The probe's model list and the id it marked current, if it marked one.
///
/// `current` comes only from a verified default marker. A missing marker stays `None`; the
/// first row is never promoted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredModels {
    pub current: Option<String>,
    pub models: Vec<DiscoveredModel>,
    /// Slash commands a provider record in this probe stdout listed. Empty unless that
    /// record was present. Callers must not fill this from a hardcoded catalog.
    pub slash_commands: Vec<DiscoveredSlashCommand>,
}

/// One slash command named by a provider probe record, not by a Ferryx catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredSlashCommand {
    pub name: String,
    pub description: Option<String>,
}

/// Why a probe stdout did not become a model list.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DiscoveryError {
    /// A candidate frame was not the documented object shape, JSON that claimed to be one did not parse, or non-empty stdout contained no model-list answer.
    #[error("discovery output is malformed")]
    Malformed,
    /// The probe answered, and the answer listed no publishable model.
    #[error("discovery output listed no models")]
    Empty,
    /// The CLI answered the probe by rejecting the method or control subtype.
    #[error("discovery CLI does not support this probe")]
    UnsupportedVersion,
    /// Stdout was larger than [`DISCOVERY_STDOUT_LIMIT`].
    #[error("discovery output exceeds 256 KiB")]
    Oversized,
    /// Grok stdout had no `Available models:` header (`src/shared/grok-model-list-probe.ts`).
    #[error("discovery output is missing the required header")]
    MissingHeader,
    /// A control or JSON-RPC response was present, and its id was not the probe's.
    #[error("discovery response request id did not match")]
    MismatchedRequest,
}

/// Parse Claude `--output-format stream-json` stdout for the `list_models` control response.
///
/// Other lines are ignored. The response is the `control_response` whose `response.request_id`
/// equals a `control_request` for `{subtype:"list_models"}` in the same stdout, or
/// [`CLAUDE_LIST_MODELS_REQUEST_ID`] when that request is not echoed. Request and response may
/// arrive in either order.
pub fn parse_claude_list_models(stdout: &str) -> Result<DiscoveredModels, DiscoveryError> {
    let stdout = bounded(stdout)?;
    if stdout.trim().is_empty() {
        return Err(DiscoveryError::Empty);
    }

    let mut expected_ids = Vec::new();
    let mut saw_request = false;
    let mut responses = Vec::new();
    let mut saw_broken_response = false;

    for line in ndjson_lines(stdout) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let looks_control = trimmed.starts_with('{')
            && (trimmed.contains("control_response") || trimmed.contains("control_request"));
        if !looks_control {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
            if trimmed.contains("control_response") {
                saw_broken_response = true;
            }
            continue;
        };
        let Some(kind) = value.get("type").and_then(Value::as_str) else {
            continue;
        };
        if kind == "control_request" && request_subtype(&value) == Some("list_models") {
            saw_request = true;
            if let Some(id) = text(value.get("request_id")) {
                push_unique(&mut expected_ids, id);
            }
            continue;
        }
        if kind != "control_response" {
            continue;
        }
        let Some(response) = value.get("response").filter(|item| item.is_object()) else {
            saw_broken_response = true;
            continue;
        };
        let Some(id) = text(response.get("request_id")) else {
            saw_broken_response = true;
            continue;
        };
        responses.push((id, response.clone()));
    }

    let mut matched = None;
    let mut saw_other_response = false;
    for (id, response) in responses {
        let matches = if saw_request {
            expected_ids.iter().any(|known| known == &id)
        } else {
            id == CLAUDE_LIST_MODELS_REQUEST_ID
        };
        if matches {
            matched = Some(response);
        } else {
            saw_other_response = true;
        }
    }

    let Some(response) = matched else {
        if saw_request && expected_ids.is_empty() {
            return Err(DiscoveryError::Malformed);
        }
        if saw_other_response {
            return Err(DiscoveryError::MismatchedRequest);
        }
        if saw_broken_response {
            return Err(DiscoveryError::Malformed);
        }
        return Err(DiscoveryError::Malformed);
    };
    let mut discovered = interpret_claude_response(&response)?;
    discovered.slash_commands = claude_init_slash_commands(stdout);
    Ok(discovered)
}

/// Parse a Codex app-server JSON-RPC transcript for the `model/list` result.
///
/// Notifications and unrelated methods are ignored. When the transcript contains a
/// `model/list` request, only the response with that id is read. Frames are one JSON
/// object per line, which is how the app-server writes stdout.
pub fn parse_codex_model_list(stdout: &str) -> Result<DiscoveredModels, DiscoveryError> {
    let stdout = bounded(stdout)?;
    if stdout.trim().is_empty() {
        return Err(DiscoveryError::Empty);
    }

    let mut frames = Vec::new();
    let mut broken = false;
    for line in ndjson_lines(stdout) {
        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.starts_with('{') {
            continue;
        }
        match serde_json::from_str::<Value>(trimmed) {
            Ok(value) => frames.push(value),
            Err(_) => broken = true,
        }
    }
    if frames.is_empty() {
        match serde_json::from_str::<Value>(stdout.trim()) {
            Ok(value) => frames.push(value),
            Err(_) => return Err(DiscoveryError::Malformed),
        }
    }

    let mut request_ids = Vec::new();
    for frame in &frames {
        if text(frame.get("method")).as_deref() == Some("model/list") {
            if let Some(id) = rpc_id(frame.get("id")) {
                push_unique(&mut request_ids, id);
            }
        }
    }

    let mut models = Vec::new();
    let mut current = None;
    let mut saw_list = false;
    let mut saw_other_response = false;
    let mut saw_method_missing = false;
    let mut saw_other_error = false;

    for frame in &frames {
        let method = text(frame.get("method"));
        if method.as_deref().is_some_and(|name| name != "model/list") && frame.get("result").is_none() {
            continue;
        }
        let id = rpc_id(frame.get("id"));
        let id_matches = match (request_ids.as_slice(), id.as_deref()) {
            ([], _) => true,
            (_, None) => false,
            (expected, Some(id)) => expected.iter().any(|known| known == id),
        };
        if frame.get("error").is_some() {
            if !id_matches && !request_ids.is_empty() {
                saw_other_response = true;
                continue;
            }
            if is_method_missing(frame.get("error")) {
                saw_method_missing = true;
            } else if id_matches {
                saw_other_error = true;
            }
            continue;
        }
        let Some(result) = frame.get("result") else {
            continue;
        };
        if !codex_result_is_model_list(result) {
            continue;
        }
        if !id_matches {
            saw_other_response = true;
            continue;
        }
        saw_list = true;
        absorb_codex_rows(result, &mut models, &mut current);
    }

    if saw_list {
        if models.is_empty() {
            return Err(DiscoveryError::Empty);
        }
        return Ok(DiscoveredModels {
            current,
            models,
            slash_commands: Vec::new(),
        });
    }
    if saw_method_missing {
        return Err(DiscoveryError::UnsupportedVersion);
    }
    if saw_other_response && !request_ids.is_empty() {
        return Err(DiscoveryError::MismatchedRequest);
    }
    if saw_other_error || broken {
        return Err(DiscoveryError::Malformed);
    }
    Err(DiscoveryError::Malformed)
}

/// Parse `grok models` text. The `Available models:` header is required.
///
/// Header and bullet rules are Orca's (`src/shared/grok-model-list-probe.ts`): exact header,
/// `*` or `-` bullets, `(default)` read from the row, blank line ends the section once a model
/// has been seen. Labels are `labelFromModelId` (`src/shared/model-id-label.ts`).
pub fn parse_grok_models(stdout: &str) -> Result<DiscoveredModels, DiscoveryError> {
    let stdout = bounded(stdout)?;
    if stdout.trim().is_empty() {
        return Err(DiscoveryError::Empty);
    }
    let lines: Vec<&str> = ndjson_lines(stdout).collect();
    let Some(header_at) = lines.iter().position(|line| line.trim() == "Available models:") else {
        return Err(DiscoveryError::MissingHeader);
    };

    let mut models: Vec<DiscoveredModel> = Vec::new();
    let mut current = None;
    for line in lines.iter().skip(header_at + 1) {
        if line.trim().is_empty() {
            if !models.is_empty() {
                break;
            }
            continue;
        }
        let Some((id, tail)) = grok_bullet(line) else {
            continue;
        };
        if tail.contains("(default)") {
            current = Some(id.to_string());
        }
        if models.iter().any(|model: &DiscoveredModel| model.id == id) {
            continue;
        }
        models.push(DiscoveredModel {
            label: label_from_model_id(id),
            id: id.to_string(),
            efforts: Vec::new(),
        });
    }
    if models.is_empty() {
        return Err(DiscoveryError::Empty);
    }
    Ok(DiscoveredModels {
        current,
        models,
        slash_commands: Vec::new(),
    })
}

fn push_unique(ids: &mut Vec<String>, id: String) {
    if !ids.iter().any(|known| known == &id) {
        ids.push(id);
    }
}

fn bounded(stdout: &str) -> Result<&str, DiscoveryError> {
    let stdout = stdout.strip_prefix('\u{feff}').unwrap_or(stdout);
    if stdout.len() > DISCOVERY_STDOUT_LIMIT {
        Err(DiscoveryError::Oversized)
    } else {
        Ok(stdout)
    }
}

fn ndjson_lines(stdout: &str) -> impl Iterator<Item = &str> {
    stdout.split('\n').map(|line| line.strip_suffix('\r').unwrap_or(line))
}

fn text(value: Option<&Value>) -> Option<String> {
    let raw = value?.as_str()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn request_subtype(value: &Value) -> Option<&str> {
    value.get("request")?.get("subtype")?.as_str()
}

fn interpret_claude_response(response: &Value) -> Result<DiscoveredModels, DiscoveryError> {
    let subtype = response.get("subtype").and_then(Value::as_str);
    if subtype == Some("error") {
        let message = response.get("error").and_then(Value::as_str).unwrap_or("");
        if message.to_ascii_lowercase().contains("unsupported") {
            return Err(DiscoveryError::UnsupportedVersion);
        }
        return Err(DiscoveryError::Malformed);
    }
    if subtype != Some("success") {
        return Err(DiscoveryError::Malformed);
    }
    let Some(rows) = response.pointer("/response/models").and_then(Value::as_array) else {
        return Err(DiscoveryError::Malformed);
    };
    let default_resolved = rows.iter().find_map(|row| {
        let obj = row.as_object()?;
        if text(obj.get("value")).as_deref() == Some("default") {
            text(obj.get("resolvedModel"))
        } else {
            None
        }
    });
    let mut models: Vec<DiscoveredModel> = Vec::new();
    let mut current = None;
    for row in rows {
        let Some(obj) = row.as_object() else {
            continue;
        };
        if obj.get("disabled").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let Some(id) = text(obj.get("value")) else {
            continue;
        };
        if id == "default" || models.iter().any(|model: &DiscoveredModel| model.id == id) {
            continue;
        }
        let resolved = text(obj.get("resolvedModel"));
        if current.is_none() && default_resolved.is_some() && resolved == default_resolved {
            current = Some(id.clone());
        }
        let label = text(obj.get("displayName")).unwrap_or_else(|| id.clone());
        models.push(DiscoveredModel {
            id,
            label,
            efforts: claude_efforts(obj),
        });
    }
    if models.is_empty() {
        return Err(DiscoveryError::Empty);
    }
    Ok(DiscoveredModels {
        current,
        models,
        slash_commands: Vec::new(),
    })
}

/// Slash commands from a Claude stream-json `system`/`init` record in the same probe stdout.
///
/// Only that record is evidence. A missing field, a non-init line, or a name that is not a
/// single token is dropped. Descriptions are copied when the record is an object with one;
/// a string entry has none. This is not a catalog.
pub fn claude_init_slash_commands(stdout: &str) -> Vec<DiscoveredSlashCommand> {
    let Ok(stdout) = bounded(stdout) else {
        return Vec::new();
    };
    let mut commands: Vec<DiscoveredSlashCommand> = Vec::new();
    for line in ndjson_lines(stdout) {
        let trimmed = line.trim();
        if !trimmed.starts_with('{') || !trimmed.contains("slash_commands") {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        if value.get("type").and_then(Value::as_str) != Some("system") {
            continue;
        }
        if value.get("subtype").and_then(Value::as_str) != Some("init") {
            continue;
        }
        let Some(rows) = value.get("slash_commands").and_then(Value::as_array) else {
            continue;
        };
        for row in rows {
            if commands.len() >= 64 {
                break;
            }
            let (name, description) = if let Some(name) = row.as_str() {
                (name.to_string(), None)
            } else if let Some(obj) = row.as_object() {
                let Some(name) = text(obj.get("name")).or_else(|| text(obj.get("command"))) else {
                    continue;
                };
                let description = text(obj.get("description")).map(|value| truncate_chars(&value, 200));
                (name, description)
            } else {
                continue;
            };
            let name = name.trim().trim_start_matches('/').trim().to_string();
            if !slash_name_ok(&name) || commands.iter().any(|known| known.name == name) {
                continue;
            }
            commands.push(DiscoveredSlashCommand { name, description });
        }
    }
    commands
}

fn slash_name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= 64
        && !name.chars().any(|ch| ch.is_control() || ch.is_whitespace())
}

fn truncate_chars(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

fn claude_efforts(obj: &serde_json::Map<String, Value>) -> Vec<String> {
    if obj.get("supportsEffort").and_then(Value::as_bool) != Some(true) {
        return Vec::new();
    }
    let Some(levels) = obj.get("supportedEffortLevels").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut efforts = Vec::new();
    for level in levels {
        let Some(effort) = level.as_str().map(str::trim).filter(|effort| !effort.is_empty()) else {
            continue;
        };
        push_unique(&mut efforts, effort.to_string());
    }
    efforts
}

fn rpc_id(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(id) => {
            let trimmed = id.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Value::Number(id) => Some(id.to_string()),
        _ => None,
    }
}

fn is_method_missing(error: Option<&Value>) -> bool {
    let Some(error) = error else {
        return false;
    };
    let message = if let Some(text) = error.as_str() {
        text.to_string()
    } else {
        error.get("message").and_then(Value::as_str).unwrap_or("").to_string()
    };
    if error.get("code").and_then(Value::as_i64) == Some(-32601) {
        return true;
    }
    let lower = message.to_ascii_lowercase();
    lower.contains("model/list")
        && (lower.contains("unsupported") || lower.contains("not found") || lower.contains("unknown"))
}

fn codex_result_is_model_list(result: &Value) -> bool {
    let Some(rows) = result.get("data").and_then(Value::as_array) else {
        return false;
    };
    if rows.is_empty() {
        return true;
    }
    rows.iter().any(|row| {
        let Some(obj) = row.as_object() else {
            return false;
        };
        obj.contains_key("displayName")
            || obj.contains_key("supportedReasoningEfforts")
            || obj.contains_key("model")
            || obj.contains_key("isDefault")
    })
}

fn absorb_codex_rows(result: &Value, models: &mut Vec<DiscoveredModel>, current: &mut Option<String>) {
    let Some(rows) = result.get("data").and_then(Value::as_array) else {
        return;
    };
    for row in rows {
        let Some(obj) = row.as_object() else {
            continue;
        };
        if obj.get("hidden").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let Some(id) = text(obj.get("model")).or_else(|| text(obj.get("id"))) else {
            continue;
        };
        if obj.get("isDefault").and_then(Value::as_bool) == Some(true) && current.is_none() {
            *current = Some(id.clone());
        }
        if models.iter().any(|model| model.id == id) {
            continue;
        }
        let label = text(obj.get("displayName")).unwrap_or_else(|| id.clone());
        models.push(DiscoveredModel {
            id,
            label,
            efforts: codex_efforts(obj.get("supportedReasoningEfforts")),
        });
    }
}

fn codex_efforts(value: Option<&Value>) -> Vec<String> {
    let Some(rows) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut efforts = Vec::new();
    for row in rows {
        let Some(effort) = row
            .get("reasoningEffort")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|effort| !effort.is_empty())
        else {
            continue;
        };
        push_unique(&mut efforts, effort.to_string());
    }
    efforts
}

fn grok_bullet(line: &str) -> Option<(&str, &str)> {
    let rest = line.trim_start();
    let rest = rest.strip_prefix('*').or_else(|| rest.strip_prefix('-'))?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let rest = rest.trim_start();
    let id_len = rest.find([' ', '\t', '(']).unwrap_or(rest.len());
    if id_len == 0 {
        return None;
    }
    Some(rest.split_at(id_len))
}

/// Port of Orca `labelFromModelId` (`src/shared/model-id-label.ts`).
fn label_from_model_id(id: &str) -> String {
    let dotted = dot_dashed_versions(id);
    rewrite_context_suffix(&dotted)
        .split(['/', '-'])
        .filter(|part| !part.is_empty())
        .map(label_part)
        .collect::<Vec<_>>()
        .join(" ")
}

fn dot_dashed_versions(id: &str) -> String {
    let mut out = String::with_capacity(id.len());
    let mut index = 0;
    while index < id.len() {
        if let Some((end, replacement)) = version_match_at(id, index) {
            out.push_str(&replacement);
            index = end;
            continue;
        }
        let Some(ch) = id[index..].chars().next() else {
            break;
        };
        out.push(ch);
        index += ch.len_utf8();
    }
    out
}

fn version_match_at(id: &str, index: usize) -> Option<(usize, String)> {
    if !id.is_char_boundary(index) {
        return None;
    }
    let rest = &id[index..];
    let (delim, after_delim) = if index == 0 {
        ("", rest)
    } else if let Some(stripped) = rest.strip_prefix('-') {
        ("-", stripped)
    } else {
        let stripped = rest.strip_prefix('/')?;
        ("/", stripped)
    };
    let name_letters = after_delim.bytes().take_while(|byte| byte.is_ascii_alphabetic()).count();
    if name_letters == 0 || !after_delim.is_char_boundary(name_letters) {
        return None;
    }
    let after_letters = &after_delim[name_letters..];
    let after_name = after_letters.strip_prefix('-')?;
    let name = &after_delim[..name_letters + 1];
    let (version, after_version) = take_dashed_version(after_name)?;
    if !after_version.is_empty() && !after_version.starts_with(['-', '/', '[']) {
        return None;
    }
    let end = id.len() - after_version.len();
    if end <= index || !id.is_char_boundary(end) {
        return None;
    }
    let replacement = if is_date_version(version) {
        id[index..end].to_string()
    } else {
        format!("{delim}{name}{}", version.replace('-', "."))
    };
    Some((end, replacement))
}

fn take_dashed_version(text: &str) -> Option<(&str, &str)> {
    let first = ascii_num_len(text)?;
    let mut end = first;
    let mut groups = 0;
    loop {
        let rest = &text[end..];
        let Some(after_dash) = rest.strip_prefix('-') else {
            break;
        };
        let Some(len) = ascii_num_len(after_dash) else {
            break;
        };
        end += 1 + len;
        groups += 1;
    }
    if groups == 0 {
        return None;
    }
    Some((&text[..end], &text[end..]))
}

fn ascii_num_len(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_digit() && bytes[0] != b'0' && bytes[1].is_ascii_digit() {
        Some(2)
    } else if bytes.first().is_some_and(|byte| byte.is_ascii_digit()) {
        Some(1)
    } else {
        None
    }
}

fn is_date_version(version: &str) -> bool {
    version.split('-').all(|part| part.len() == 2 && part.bytes().all(|byte| byte.is_ascii_digit()))
}

fn rewrite_context_suffix(id: &str) -> String {
    let Some(open) = id.rfind('[') else {
        return id.to_string();
    };
    if !id.ends_with(']') || open + 2 >= id.len() {
        return id.to_string();
    }
    let inner = &id[open + 1..id.len() - 1];
    let mut chars = inner.chars();
    let Some(unit) = chars.next_back() else {
        return id.to_string();
    };
    let size = chars.as_str();
    if size.is_empty() || !size.bytes().all(|byte| byte.is_ascii_digit()) {
        return id.to_string();
    }
    let unit = match unit.to_ascii_uppercase() {
        'K' => "K",
        'M' => "M",
        _ => return id.to_string(),
    };
    format!("{} ({}{unit} context)", &id[..open], size)
}

fn label_part(part: &str) -> String {
    if part.eq_ignore_ascii_case("gpt") {
        return "GPT".to_string();
    }
    let mut chars = part.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    if part.chars().count() <= 3 && first.is_ascii_digit() {
        return part.to_ascii_uppercase();
    }
    let mut out = String::new();
    out.push(first.to_ascii_uppercase());
    out.extend(chars);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_argv_is_fixed_and_carries_no_client_template() {
        assert_eq!(CLAUDE_PROBE_PROGRAM, "claude");
        assert_eq!(
            CLAUDE_PROBE_ARGS,
            [
                "-p",
                "--input-format",
                "stream-json",
                "--output-format",
                "stream-json",
                "--verbose",
                "--strict-mcp-config",
                "--settings",
                "{\"disableAllHooks\":true}",
            ]
        );
        assert!(claude_probe_stdin().contains(CLAUDE_LIST_MODELS_REQUEST_ID));
        assert!(claude_probe_stdin().contains("list_models"));
        assert_eq!(CODEX_PROBE_PROGRAM, "codex");
        assert!(CODEX_PROBE_ARGS.ends_with(&["app-server"]));
        assert!(codex_probe_stdin().contains("\"method\":\"model/list\""));
        assert!(!codex_probe_stdin().contains("argv"));
        assert_eq!(GROK_PROBE_PROGRAM, "grok");
        assert_eq!(GROK_PROBE_ARGS, ["models"]);
        assert_eq!(DISCOVERY_PROBE_TIMEOUT, std::time::Duration::from_secs(15));
    }

    fn oversized() -> String {
        "x".repeat(DISCOVERY_STDOUT_LIMIT + 1)
    }

    fn claude_response(request_id: &str, models: &str) -> String {
        format!(
            r#"{{"type":"control_response","response":{{"subtype":"success","request_id":"{request_id}","response":{{"models":{models}}}}}}}"#
        )
    }

    #[test]
    fn claude_parses_list_models_and_ignores_unrelated_lines() {
        // One object per line, as `claude --output-format stream-json` writes and as Orca's
        // `JSON.stringify` fixtures do (`src/shared/claude-model-list-probe.test.ts`).
        let models = r#"[{"value":"default","resolvedModel":"claude-opus-5[1m]","displayName":"Default (recommended)","supportsEffort":true,"supportedEffortLevels":["low","medium","high","xhigh","max"]},{"value":"opus[1m]","resolvedModel":"claude-opus-5[1m]","displayName":"Opus (1M context)","description":"Opus 5","supportsEffort":true,"supportedEffortLevels":["low","medium","high","xhigh","max"],"supportsFastMode":true},{"value":"sonnet","resolvedModel":"claude-sonnet-5","displayName":"Sonnet","supportsEffort":true,"supportedEffortLevels":["low","high"]},{"value":"haiku","resolvedModel":"claude-haiku-4-5-20251001","displayName":"Haiku"},{"value":"cc-update-required-1","displayName":"Fable (disabled)","disabled":true,"supportsEffort":true,"supportedEffortLevels":["low"]}]"#;
        let stdout = format!(
            "{}\nnot json at all\r\n{}\n{{\"type\":\"control_response\",\"response\":{{\"subtype\":\"success\",\"request_id\":\"orca-model-settings\",\"response\":{{}}}}}}\n",
            r#"{"type":"system","subtype":"init","model":"claude-sonnet-5"}"#,
            claude_response(CLAUDE_LIST_MODELS_REQUEST_ID, models)
        );
        let parsed = parse_claude_list_models(&stdout).expect("list_models fixture");
        assert_eq!(parsed.current.as_deref(), Some("opus[1m]"));
        assert_eq!(
            parsed.models.iter().map(|model| model.id.as_str()).collect::<Vec<_>>(),
            vec!["opus[1m]", "sonnet", "haiku"]
        );
        assert_eq!(parsed.models[0].label, "Opus (1M context)");
        assert_eq!(parsed.models[0].efforts, ["low", "medium", "high", "xhigh", "max"]);
        assert_eq!(parsed.models[2].efforts, Vec::<String>::new());
        assert!(
            parsed.slash_commands.is_empty(),
            "an init record without slash_commands publishes none"
        );
    }

    #[test]
    fn claude_matches_an_echoed_request_id_and_rejects_a_mismatch() {
        let models = r#"[{"value":"sonnet","displayName":"Sonnet"}]"#;
        let echoed = format!(
            "{}\n{}\n",
            claude_response("probe-1", models),
            r#"{"type":"control_request","request_id":"probe-1","request":{"subtype":"list_models"}}"#,
        );
        let parsed = parse_claude_list_models(&echoed).expect("response before its request");
        assert_eq!(parsed.models[0].id, "sonnet");
        assert_eq!(parsed.current, None);

        let mismatched = format!(
            "{}\n{}\n",
            r#"{"type":"control_request","request_id":"probe-1","request":{"subtype":"list_models"}}"#,
            claude_response("probe-2", models)
        );
        assert_eq!(
            parse_claude_list_models(&mismatched),
            Err(DiscoveryError::MismatchedRequest)
        );
        assert_eq!(
            parse_claude_list_models(&claude_response("someone-else", models)),
            Err(DiscoveryError::MismatchedRequest)
        );
    }

    #[test]
    fn claude_rejects_malformed_empty_oversize_and_old_cli() {
        assert_eq!(parse_claude_list_models(""), Err(DiscoveryError::Empty));
        assert_eq!(parse_claude_list_models(" \n\t"), Err(DiscoveryError::Empty));
        assert_eq!(
            parse_claude_list_models(r#"{"type":"control_response""#),
            Err(DiscoveryError::Malformed)
        );
        assert_eq!(parse_claude_list_models(&oversized()), Err(DiscoveryError::Oversized));
        assert_eq!(
            parse_claude_list_models(&"x".repeat(DISCOVERY_STDOUT_LIMIT)),
            Err(DiscoveryError::Malformed)
        );
        let old = format!(
            r#"{{"type":"control_response","response":{{"subtype":"error","request_id":"{CLAUDE_LIST_MODELS_REQUEST_ID}","error":"Unsupported control request subtype: list_models"}}}}"#
        );
        assert_eq!(parse_claude_list_models(&old), Err(DiscoveryError::UnsupportedVersion));
    }

    #[test]
    fn claude_ignores_effort_levels_without_support_and_duplicate_values() {
        let models = r#"[{"value":"haiku","displayName":"Haiku","supportedEffortLevels":["low","high"]},{"value":"sonnet","displayName":"Sonnet"},{"value":"sonnet","displayName":"Sonnet again"}]"#;
        let parsed = parse_claude_list_models(&claude_response(CLAUDE_LIST_MODELS_REQUEST_ID, models))
            .expect("deduped");
        assert_eq!(parsed.models.len(), 2);
        assert_eq!(parsed.models[0].efforts, Vec::<String>::new());
    }

    #[test]
    fn codex_parses_model_list_result_and_ignores_other_frames() {
        let stdout = concat!(
            r#"{"jsonrpc":"2.0","method":"session/updated","params":{"data":[{"displayName":"not a catalog"}]}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":1,"result":{"data":[{"model":"gpt-live","id":"other-id","displayName":"GPT Live","isDefault":true,"defaultReasoningEffort":"medium","supportedReasoningEfforts":[{"reasoningEffort":"medium","description":"Balanced"},{"reasoningEffort":"high","description":"Deep"}]},{"id":"gpt-mini","displayName":"GPT Mini","isDefault":false,"supportedReasoningEfforts":[{"reasoningEffort":"low","description":"Fast"}]},{"model":"hidden-one","displayName":"Hidden","hidden":true,"isDefault":false}],"nextCursor":null}}"#,
            "\n",
        );
        let parsed = parse_codex_model_list(stdout).expect("model/list");
        assert_eq!(parsed.current.as_deref(), Some("gpt-live"));
        assert_eq!(parsed.models[0].label, "GPT Live");
        assert_eq!(parsed.models[0].efforts, ["medium", "high"]);
        assert_eq!(parsed.models[1].id, "gpt-mini");
        assert_eq!(parsed.models.len(), 2);
    }

    #[test]
    fn codex_matches_request_id_and_rejects_method_not_found() {
        let stdout = concat!(
            r#"{"jsonrpc":"2.0","id":7,"method":"model/list","params":{"limit":100}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":8,"result":{"data":[{"id":"gpt-live","displayName":"GPT Live","isDefault":true}]}}"#,
            "\n",
        );
        assert_eq!(parse_codex_model_list(stdout), Err(DiscoveryError::MismatchedRequest));

        let missing = concat!(
            r#"{"jsonrpc":"2.0","id":7,"method":"model/list","params":{}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":7,"error":{"code":-32601,"message":"Method not found: model/list"}}"#,
            "\n",
        );
        assert_eq!(parse_codex_model_list(missing), Err(DiscoveryError::UnsupportedVersion));
    }

    #[test]
    fn codex_rejects_malformed_empty_and_oversize() {
        assert_eq!(parse_codex_model_list(""), Err(DiscoveryError::Empty));
        assert_eq!(parse_codex_model_list("{not json"), Err(DiscoveryError::Malformed));
        assert_eq!(
            parse_codex_model_list(r#"{"jsonrpc":"2.0","id":1,"result":{"data":[]}}"#),
            Err(DiscoveryError::Empty)
        );
        assert_eq!(parse_codex_model_list(&oversized()), Err(DiscoveryError::Oversized));
    }

    #[test]
    fn grok_parses_the_signed_in_listing() {
        let stdout = "You are logged in with grok.com.\n\nDefault model: grok-4.6\n\nAvailable models:\n  * grok-4.6 (default)\n  - grok-4.5\n\nTips:\n  * Set a default with `grok config`\n";
        let parsed = parse_grok_models(stdout).expect("grok models");
        assert_eq!(parsed.current.as_deref(), Some("grok-4.6"));
        assert_eq!(parsed.models[0].label, "Grok 4.6");
        assert_eq!(parsed.models[1].id, "grok-4.5");
        assert_eq!(parsed.models[1].label, "Grok 4.5");
        assert_eq!(parsed.models.len(), 2);
        assert!(parsed.models.iter().all(|model| model.efforts.is_empty()));
    }

    #[test]
    fn grok_requires_the_header_and_reads_only_the_row_marker() {
        assert_eq!(
            parse_grok_models("Models:\n  * grok-4.5 (default)\n"),
            Err(DiscoveryError::MissingHeader)
        );
        assert_eq!(
            parse_grok_models("Default model: grok-4.5\n"),
            Err(DiscoveryError::MissingHeader)
        );
        assert_eq!(parse_grok_models(""), Err(DiscoveryError::Empty));
        assert_eq!(parse_grok_models(&oversized()), Err(DiscoveryError::Oversized));
        assert_eq!(parse_grok_models("Available models:\n"), Err(DiscoveryError::Empty));

        let unmarked = parse_grok_models("Available models:\r\n  * grok-4.5\r\n  * grok-build (beta)\r\n")
            .expect("unmarked");
        assert_eq!(unmarked.current, None);
        assert_eq!(unmarked.models[1].id, "grok-build");

        let repeated = parse_grok_models("Available models:\n  * grok-4.5\n  * grok-4.5 (default)\n")
            .expect("repeated marker");
        assert_eq!(repeated.models.len(), 1);
        assert_eq!(repeated.current.as_deref(), Some("grok-4.5"));

        let dated = parse_grok_models("Available models:\n  * grok-4.5-fast-2026.07.19\n").expect("dated");
        assert_eq!(dated.models[0].label, "Grok 4.5 Fast 2026.07.19");
    }

    #[test]
    fn claude_init_slash_commands_are_published_and_other_probes_stay_empty() {
        let models = r#"[{"value":"sonnet","displayName":"Sonnet"}]"#;
        let stdout = format!(
            "{}\n{}\n{}\n",
            r#"{"type":"system","subtype":"init","slash_commands":["/compact","compact",{"name":"review","description":"Review the diff"},{"name":"bad name"},"  "],"model":"sonnet"}"#,
            r#"{"type":"assistant","slash_commands":["invented"]}"#,
            claude_response(CLAUDE_LIST_MODELS_REQUEST_ID, models)
        );
        let parsed = parse_claude_list_models(&stdout).expect("list_models with init");
        assert_eq!(
            parsed.slash_commands,
            vec![
                DiscoveredSlashCommand { name: "compact".into(), description: None },
                DiscoveredSlashCommand {
                    name: "review".into(),
                    description: Some("Review the diff".into()),
                },
            ]
        );
        let codex = parse_codex_model_list(
            r#"{"jsonrpc":"2.0","id":2,"result":{"data":[{"model":"gpt-6","displayName":"GPT-6","isDefault":true}]}}"#,
        )
        .expect("codex");
        assert!(codex.slash_commands.is_empty());
        let grok = parse_grok_models("Available models:\n  * grok-4.6 (default)\n").expect("grok");
        assert!(grok.slash_commands.is_empty());
    }
}
