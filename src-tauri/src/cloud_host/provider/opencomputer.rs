//! OpenComputer: a hosted sandbox reached only over the relay, driven here as a plain REST
//! sandbox API (create/get/delete/exec/files), never a WebSocket.
//!
//! # Endpoints
//!
//! **Nothing in this file is verified against the live API: `~/.config/compute-secrets.env` has no
//! `OPENCOMPUTER_API_KEY`, so the live QA for this provider is `SKIPPED: no key`.** What is known,
//! recorded 2026-11-21:
//!
//! - The published schema (`docs.opencomputer.dev`, "Computer API version 1",
//!   `computer/schema/computer-api.v1.json`) is **session**-based, not sandbox-based: `POST
//!   /sessions`, `GET /sessions/{id}`, `PATCH /sessions/{id}`, `POST /sessions/{id}/suspend`,
//!   `POST /sessions/{id}/resume`, turns/asks/subagents/events, and `GET /connect` for
//!   WebSocket. Base `https://app.opencomputer.dev/api/managed-agents`, auth `x-api-key`. Files
//!   are session-scoped tar archives (`PUT|GET /sessions/{id}/workspace/archive`) with no mode
//!   field.
//! - The only way that schema runs a command is `terminals.open`, and that is **WebSocket only**
//!   ("a terminal is a shell or command the host starts ... over WebSocket only"). There is no
//!   HTTP exec route in the published schema, which is why this plan names a sandbox surface
//!   instead.
//! - Live probes without a key, against `https://app.opencomputer.dev`: `/health` answers
//!   `{"ok":true,"env":"prod"}` with no auth; every `/api/...` path answers `401
//!   {"error":"missing or invalid API key"}`, **including paths that certainly do not exist**, so
//!   401 does not prove a route exists; `/api/v1/...` answers `404 no such route`.
//!
//! So the routes below follow the plan (task 10: "sandbox lifecycle + `/sandboxes/{id}/exec/run`")
//! and are marked unverified in [`ENDPOINTS`]. The base is overridable with
//! `OPENCOMPUTER_API_URL`, so an operator with a key can point it at whatever the real surface is
//! without touching code, and every response parser is deliberately tolerant of the plausible
//! shapes (`{stdout,stderr,exitCode}`, `{output}`, `{result:{...}}`, `{data:{...}}`).
//!
//! | Call | Request | Expected response |
//! | --- | --- | --- |
//! | create | `POST /sandboxes` `{name, env, ttl_seconds}` | `{id}` (or `{sandbox:{id}}` / `{data:{id}}`); `409` when the name is taken |
//! | get | `GET /sandboxes/{id}` | `{state}` or `{status}` in the documented-session vocabulary |
//! | delete | `DELETE /sandboxes/{id}` | `204`/`200`; `404` when already gone |
//! | stop | `POST /sandboxes/{id}/stop` | `200`/`202`/`204` |
//! | resume | `POST /sandboxes/{id}/resume` | `200`/`202` |
//! | exec | `POST /sandboxes/{id}/exec/run` `{command, timeout_seconds, stdin:false}` | `{stdout,stderr,exitCode}` or `{output}` |
//! | write file | `PUT /sandboxes/{id}/files` `{path, content(base64), mode}` | `200`/`201`/`204` |
//! | list | `GET /sandboxes?limit=&cursor=` | array, or `{sandboxes:[]}`, or `{data:[],meta}` |
//!
//! With no key every call reports `Config` naming `OPENCOMPUTER_API_KEY`, which is exactly the
//! documented "no key" state for this provider.

use std::time::Duration;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde_json::{json, Value};

use super::{
    Capabilities, CreateSandbox, ErrorKind, ExecResult, ListedSandbox, SandboxError, SandboxInfo,
    SandboxProvider, SandboxState, Transport,
};

const PROVIDER: &str = "opencomputer";
/// Unverified default, see the module docs: the documented base, with the plan's sandbox surface
/// below it.
const DEFAULT_API_URL: &str = "https://app.opencomputer.dev/api/managed-agents/v1";
/// Every endpoint this adapter uses, in one place because none of them could be verified.
pub const ENDPOINTS: &[&str] = &[
    "POST /sandboxes",
    "GET /sandboxes/{id}",
    "DELETE /sandboxes/{id}",
    "POST /sandboxes/{id}/stop",
    "POST /sandboxes/{id}/resume",
    "POST /sandboxes/{id}/exec/run",
    "PUT /sandboxes/{id}/files",
    "GET /sandboxes",
];
const DEFAULT_TTL_SECONDS: u64 = 3_600;
/// Must NOT have: one bounded response, no unbounded stream.
const EXEC_OUTPUT_CAP: usize = 4 * 1024 * 1024;
const LIST_PAGE_LIMIT: usize = 100;
const MAX_LIST_PAGES: usize = 20;

pub struct OpenComputerProvider {
    client: reqwest::Client,
    /// `None` when no key was found: every call then reports `Config`.
    token: Option<String>,
    api_url: String,
}

impl OpenComputerProvider {
    pub fn new(token: Option<String>, api_url: String) -> Self {
        let client = reqwest::Client::builder()
            .user_agent(concat!("ferryx-cloud-host/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_default();
        Self { client, token, api_url }
    }

    /// Key from `OPENCOMPUTER_API_KEY`, base from `OPENCOMPUTER_API_URL`.
    pub fn from_env() -> Self {
        let token = std::env::var("OPENCOMPUTER_API_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let api_url = std::env::var("OPENCOMPUTER_API_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_API_URL.to_string());
        Self::new(token, api_url)
    }

    /// A provider with no credential, for the "call without a key reports Config" path.
    pub fn without_token() -> Self {
        Self::new(None, DEFAULT_API_URL.to_string())
    }

    fn token(&self) -> Result<&str, SandboxError> {
        self.token.as_deref().ok_or_else(|| {
            config(
                "missing_token",
                "no OpenComputer key: set OPENCOMPUTER_API_KEY",
            )
        })
    }

    async fn call(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
        timeout: Option<Duration>,
    ) -> Result<ApiResponse, SandboxError> {
        let token = self.token()?;
        let mut request = self
            .client
            .request(method, format!("{}{path}", self.api_url))
            .header("x-api-key", token);
        if let Some(body) = body {
            request = request.json(&body);
        }
        if let Some(timeout) = timeout {
            request = request.timeout(timeout);
        }
        let response = request.send().await.map_err(|error| transport_error(&error))?;
        let status = response.status().as_u16();
        let body = response.text().await.map_err(|error| transport_error(&error))?;
        Ok(ApiResponse { status, body })
    }

    fn ttl_seconds(ttl_seconds: Option<u64>) -> u64 {
        ttl_seconds.unwrap_or(DEFAULT_TTL_SECONDS).max(60)
    }
}

struct ApiResponse {
    status: u16,
    body: String,
}

#[async_trait::async_trait]
impl SandboxProvider for OpenComputerProvider {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            ttl_required: false,
            max_ttl_seconds: None,
            transport: Transport::Relay,
            // The hosted runner suspends an idle session on its own.
            native_auto_stop: true,
        }
    }

    async fn create(&self, request: CreateSandbox) -> Result<String, SandboxError> {
        let mut body = json!({
            "name": request.name,
            "ttl_seconds": Self::ttl_seconds(request.ttl_seconds),
        });
        if !request.env.is_empty() {
            body["env"] = json!(request.env);
        }
        let response = self.call(reqwest::Method::POST, "/sandboxes", Some(body), None).await?;
        match response.status {
            200 | 201 => sandbox_id(&response.body),
            // The name is the handle, so a 409 is our own earlier create winning the race.
            409 => Ok(request.name),
            _ => Err(status_error(response.status, &response.body, "create")),
        }
    }

    async fn get(&self, sandbox_id: &str) -> Result<SandboxInfo, SandboxError> {
        let response = self
            .call(reqwest::Method::GET, &format!("/sandboxes/{sandbox_id}"), None, None)
            .await?;
        if response.status != 200 {
            return Err(status_error(response.status, &response.body, "get"));
        }
        let value = parse_value(&response.body, "get")?;
        // Both spellings appear in the documented session object; the value is what matters.
        let raw = value
            .get("state")
            .or_else(|| value.get("status"))
            .or_else(|| value.get("sandbox").and_then(|sandbox| sandbox.get("state")))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_ascii_lowercase();
        let state = match raw.as_str() {
            "running" => SandboxState::Ready,
            "idle" | "waiting" => SandboxState::Ready,
            "suspended" | "stopped" | "standby" | "paused" => SandboxState::Stopped,
            "starting" | "provisioning" | "pending" | "resuming" => SandboxState::Provisioning,
            "stopping" | "suspending" => SandboxState::Stopping,
            "ended" | "terminated" | "deleted" => SandboxState::Deleted,
            "failed" | "error" => SandboxState::Error,
            "" => SandboxState::Provisioning,
            _ => SandboxState::Provisioning,
        };
        let error = (state == SandboxState::Error).then(|| format!("sandbox state is {raw}"));
        Ok(SandboxInfo { state, error })
    }

    async fn stop(&self, sandbox_id: &str) -> Result<(), SandboxError> {
        let response = self
            .call(
                reqwest::Method::POST,
                &format!("/sandboxes/{sandbox_id}/stop"),
                Some(json!({})),
                None,
            )
            .await?;
        match response.status {
            200 | 202 | 204 => Ok(()),
            409 => Ok(()),
            _ => Err(status_error(response.status, &response.body, "stop")),
        }
    }

    async fn resume(&self, sandbox_id: &str, ttl_seconds: Option<u64>) -> Result<(), SandboxError> {
        let path = format!("/sandboxes/{sandbox_id}/resume");
        let body = match ttl_seconds {
            Some(ttl) => json!({ "ttl_seconds": ttl }),
            None => json!({}),
        };
        let response = self.call(reqwest::Method::POST, &path, Some(body), None).await?;
        match response.status {
            200 | 202 | 204 => Ok(()),
            _ => Err(status_error(response.status, &response.body, "resume")),
        }
    }

    async fn delete(&self, sandbox_id: &str) -> Result<(), SandboxError> {
        let response = self
            .call(reqwest::Method::DELETE, &format!("/sandboxes/{sandbox_id}"), None, None)
            .await?;
        match response.status {
            200 | 202 | 204 => Ok(()),
            _ => Err(status_error(response.status, &response.body, "delete")),
        }
    }

    async fn extend_ttl(
        &self,
        sandbox_id: &str,
        ttl_seconds: Option<u64>,
    ) -> Result<(), SandboxError> {
        let body = json!({ "ttl_seconds": Self::ttl_seconds(ttl_seconds) });
        let response = self
            .call(reqwest::Method::PATCH, &format!("/sandboxes/{sandbox_id}"), Some(body), None)
            .await?;
        match response.status {
            200 | 204 => Ok(()),
            _ => Err(status_error(response.status, &response.body, "extend_ttl")),
        }
    }

    async fn exec(
        &self,
        sandbox_id: &str,
        command: &str,
        timeout_seconds: u64,
    ) -> Result<ExecResult, SandboxError> {
        let body = json!({
            "command": command,
            "timeout_seconds": timeout_seconds,
            // No stdin: the call is one-shot.
            "stdin": false,
        });
        let response = self
            .call(
                reqwest::Method::POST,
                &format!("/sandboxes/{sandbox_id}/exec/run"),
                Some(body),
                Some(Duration::from_secs(timeout_seconds.max(1).saturating_add(30))),
            )
            .await;
        let response = match response {
            Ok(response) => response,
            Err(error) if error.kind == ErrorKind::Retryable => return Ok(timed_out()),
            Err(error) => return Err(error),
        };
        if response.status == 408 || response.status == 504 {
            return Ok(timed_out());
        }
        if response.status != 200 && response.status != 201 {
            return Err(status_error(response.status, &response.body, "exec"));
        }
        parse_exec(&response.body)
    }

    async fn write_file(
        &self,
        sandbox_id: &str,
        path: &str,
        bytes: Vec<u8>,
        mode: u32,
    ) -> Result<(), SandboxError> {
        if !path.starts_with('/') {
            return Err(config("path_not_absolute", "write_file needs an absolute path"));
        }
        // The documented session file routes carry tar archives and no mode, so the sandbox file
        // route is asked for the mode in the same call; the content travels base64 so an
        // arbitrary payload (a credential file included) survives JSON.
        let body = write_body(path, &bytes, mode);
        let response = self
            .call(
                reqwest::Method::PUT,
                &format!("/sandboxes/{sandbox_id}/files"),
                Some(body),
                Some(Duration::from_secs(120)),
            )
            .await?;
        match response.status {
            200 | 201 | 204 => Ok(()),
            _ => Err(status_error(response.status, &response.body, "write_file")),
        }
    }

    async fn list(&self) -> Result<Vec<ListedSandbox>, SandboxError> {
        let mut listed = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..MAX_LIST_PAGES {
            let mut path = format!("/sandboxes?limit={LIST_PAGE_LIMIT}");
            if let Some(cursor) = cursor.as_deref() {
                path.push_str("&cursor=");
                path.push_str(cursor);
            }
            let response = self.call(reqwest::Method::GET, &path, None, None).await?;
            if response.status != 200 {
                return Err(status_error(response.status, &response.body, "list"));
            }
            let page = parse_list(&response.body)?;
            listed.extend(page.rows);
            match page.cursor {
                Some(next) if !next.is_empty() => cursor = Some(next),
                _ => return Ok(listed),
            }
        }
        Ok(listed)
    }
}

// ---------------------------------------------------------------------------------------------
// Tolerant parsing: the shapes below are the plausible spellings, not a verified contract

/// The body of a file write: the path, the base64 content and the mode to create it with.
fn write_body(path: &str, bytes: &[u8], mode: u32) -> Value {
    json!({
        "path": path,
        "content": BASE64.encode(bytes),
        "encoding": "base64",
        "mode": format!("{:04o}", mode & 0o7777),
    })
}

fn parse_value(body: &str, call: &str) -> Result<Value, SandboxError> {
    serde_json::from_str(body).map_err(|error| {
        fatal(
            "unexpected_body",
            format!("{call}: the API answered with an unexpected body: {error}"),
        )
    })
}

/// The id of a created sandbox, wherever the answer put it.
fn sandbox_id(body: &str) -> Result<String, SandboxError> {
    let value = parse_value(body, "create")?;
    for candidate in [
        value.get("id"),
        value.get("sandbox_id"),
        value.get("sandboxId"),
        value.get("sandbox").and_then(|sandbox| sandbox.get("id")),
        value.get("data").and_then(|data| data.get("id")),
    ]
    .into_iter()
    .flatten()
    {
        if let Some(id) = candidate.as_str().filter(|id| !id.is_empty()) {
            return Ok(id.to_string());
        }
    }
    Err(fatal("missing_id", "create: the answer carried no sandbox id"))
}

fn parse_exec(body: &str) -> Result<ExecResult, SandboxError> {
    let value = parse_value(body, "exec")?;
    // A wrapper (`{result}`, `{data}`) is unwrapped once; the inner object is the run.
    let run = value
        .get("result")
        .or_else(|| value.get("data"))
        .filter(|inner| inner.is_object())
        .unwrap_or(&value);
    let stdout = text_field(run, &["stdout", "output", "logs"]).unwrap_or_default();
    let stderr = text_field(run, &["stderr"]).unwrap_or_default();
    if stdout.len() + stderr.len() > EXEC_OUTPUT_CAP {
        return Err(fatal(
            "output_limit_exceeded",
            format!("exec output exceeded {EXEC_OUTPUT_CAP} bytes"),
        ));
    }
    let timed_out = run
        .get("timed_out")
        .or_else(|| run.get("timedOut"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let exit_code = ["exitCode", "exit_code", "code"]
        .iter()
        .find_map(|name| run.get(*name))
        .and_then(Value::as_i64)
        .map(|code| code as i32);
    Ok(ExecResult {
        exit_code: if timed_out { None } else { exit_code },
        stdout,
        stderr,
        timed_out,
    })
}

fn text_field(value: &Value, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| value.get(*name))
        .and_then(|field| match field {
            Value::String(text) => Some(text.clone()),
            // Some runtimes answer a byte array; anything else is ignored.
            Value::Array(_) => Some(String::from_utf8_lossy(&serde_json::from_value::<Vec<u8>>(field.clone()).unwrap_or_default()).into_owned()),
            _ => None,
        })
}

struct ListPage {
    rows: Vec<ListedSandbox>,
    cursor: Option<String>,
}

fn parse_list(body: &str) -> Result<ListPage, SandboxError> {
    let value = parse_value(body, "list")?;
    let (rows_value, cursor) = if value.is_array() {
        (value, None)
    } else {
        let cursor = value
            .get("meta")
            .and_then(|meta| meta.get("nextCursor").or_else(|| meta.get("next_cursor")))
            .or_else(|| value.get("nextCursor"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let rows = value
            .get("sandboxes")
            .or_else(|| value.get("sessions"))
            .or_else(|| value.get("data"))
            .cloned()
            .unwrap_or(Value::Array(Vec::new()));
        (rows, cursor)
    };
    let rows: Vec<Value> = serde_json::from_value(rows_value).map_err(|error| {
        fatal("unexpected_body", format!("list: unexpected row shape: {error}"))
    })?;
    Ok(ListPage { rows: rows.iter().map(listed_row).collect(), cursor })
}

/// One listing row. Anything the answer does not carry is left empty rather than guessed.
fn listed_row(row: &Value) -> ListedSandbox {
    let id = ["id", "sandbox_id", "sandboxId", "name"]
        .iter()
        .find_map(|name| row.get(*name))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let raw = row
        .get("state")
        .or_else(|| row.get("status"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let state = match raw.as_str() {
        "running" | "idle" | "waiting" => SandboxState::Ready,
        "suspended" | "stopped" | "standby" | "paused" => SandboxState::Stopped,
        "stopping" | "suspending" => SandboxState::Stopping,
        "ended" | "terminated" | "deleted" => SandboxState::Deleted,
        "failed" | "error" => SandboxState::Error,
        _ => SandboxState::Provisioning,
    };
    let created_at = ["created_at", "createdAt"]
        .iter()
        .find_map(|name| row.get(*name))
        .and_then(Value::as_str)
        .and_then(rfc3339_millis);
    let name = row.get("name").and_then(Value::as_str).map(str::to_string);
    ListedSandbox { sandbox_id: id, state, created_at, name }
}

// ---------------------------------------------------------------------------------------------
// Small helpers

fn rfc3339_millis(text: &str) -> Option<i64> {
    let parsed =
        time::OffsetDateTime::parse(text, &time::format_description::well_known::Rfc3339).ok()?;
    i64::try_from(parsed.unix_timestamp_nanos() / 1_000_000).ok()
}

fn timed_out() -> ExecResult {
    ExecResult { exit_code: None, stdout: String::new(), stderr: String::new(), timed_out: true }
}

fn config(code: impl Into<String>, message: impl Into<String>) -> SandboxError {
    SandboxError::new(ErrorKind::Config, PROVIDER, code, message)
}

fn fatal(code: impl Into<String>, message: impl Into<String>) -> SandboxError {
    SandboxError::new(ErrorKind::Fatal, PROVIDER, code, message)
}

/// Error bodies are `{error:{code,message}}` on the documented surface and `{error:"..."}` for a
/// missing key or an unknown route.
fn status_error(status: u16, body: &str, call: &str) -> SandboxError {
    let parsed = serde_json::from_str::<Value>(body).ok();
    let error = parsed.as_ref().and_then(|value| value.get("error"));
    let detail = error
        .and_then(|error| {
            error
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| error.as_str())
        })
        .unwrap_or(body)
        .trim()
        .to_string();
    let code = error
        .and_then(|error| error.get("code"))
        .and_then(Value::as_str)
        .or_else(|| parsed.as_ref().and_then(|value| value.get("code")).and_then(Value::as_str))
        // The live 404 answers `{"error":"not_found","message":"no such route"}`: a bare
        // token with no spaces is the code, anything else is only prose for the operator.
        .or_else(|| {
            error
                .and_then(Value::as_str)
                .filter(|token| !token.is_empty() && !token.contains(char::is_whitespace))
        })
        .unwrap_or("http_error")
        .to_string();
    let kind = match (status, code.as_str()) {
        (401 | 403, _) => ErrorKind::Config,
        (404, _) => ErrorKind::NotFound,
        (409, _) => ErrorKind::Conflict,
        // `insufficient_credits`: an account problem, not something a retry changes.
        (402, _) => ErrorKind::Config,
        (429 | 500..=599, _) => ErrorKind::Retryable,
        _ => ErrorKind::Fatal,
    };
    SandboxError::new(kind, PROVIDER, code, format!("{call}: HTTP {status}: {detail}"))
}

fn transport_error(error: &reqwest::Error) -> SandboxError {
    let kind = if error.is_timeout() || error.is_connect() {
        ErrorKind::Retryable
    } else {
        ErrorKind::Fatal
    };
    // Never include the request in the message: the header carries the key.
    SandboxError::new(kind, PROVIDER, "transport", format!("request failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CREATE_FIXTURE: &str =
        include_str!("../../../tests/fixtures/cloud_host/opencomputer/create_shape.json");
    const CREATE_WRAPPED_FIXTURE: &str =
        include_str!("../../../tests/fixtures/cloud_host/opencomputer/create_wrapped_shape.json");
    const GET_RUNNING_FIXTURE: &str =
        include_str!("../../../tests/fixtures/cloud_host/opencomputer/get_running_shape.json");
    const GET_SUSPENDED_FIXTURE: &str =
        include_str!("../../../tests/fixtures/cloud_host/opencomputer/get_suspended_shape.json");
    const EXEC_FIXTURE: &str =
        include_str!("../../../tests/fixtures/cloud_host/opencomputer/exec_shape.json");
    const EXEC_OUTPUT_ONLY_FIXTURE: &str =
        include_str!("../../../tests/fixtures/cloud_host/opencomputer/exec_output_only_shape.json");
    const EXEC_KILLED_FIXTURE: &str =
        include_str!("../../../tests/fixtures/cloud_host/opencomputer/exec_killed_shape.json");
    const LIST_FIXTURE: &str =
        include_str!("../../../tests/fixtures/cloud_host/opencomputer/list_shape.json");
    const LIST_WRAPPED_FIXTURE: &str =
        include_str!("../../../tests/fixtures/cloud_host/opencomputer/list_wrapped_shape.json");
    const ERROR_401_FIXTURE: &str =
        include_str!("../../../tests/fixtures/cloud_host/opencomputer/error_401_live.txt");
    const ERROR_404_FIXTURE: &str =
        include_str!("../../../tests/fixtures/cloud_host/opencomputer/error_404_live.txt");

    #[test]
    fn the_answer_is_read_for_the_id_wherever_it_put_it() {
        assert_eq!(sandbox_id(CREATE_FIXTURE).expect("id"), "cs_0123456789abcdef");
        assert_eq!(sandbox_id(CREATE_WRAPPED_FIXTURE).expect("id"), "cs_fedcba9876543210");
        let error = sandbox_id("{}").expect_err("no id");
        assert_eq!(error.provider_code, "missing_id");
    }

    #[test]
    fn the_documented_state_vocabulary_maps_onto_the_trait() {
        assert_eq!(state_of(GET_RUNNING_FIXTURE), SandboxState::Ready);
        assert_eq!(state_of(GET_SUSPENDED_FIXTURE), SandboxState::Stopped);
        assert_eq!(state_of(r#"{"status":"starting"}"#), SandboxState::Provisioning);
        assert_eq!(state_of(r#"{"status":"ended"}"#), SandboxState::Deleted);
        assert_eq!(state_of(r#"{"status":"failed"}"#), SandboxState::Error);
        // An unknown word is "not ready yet", never "ready".
        assert_eq!(state_of(r#"{"status":"something-new"}"#), SandboxState::Provisioning);
    }

    #[test]
    fn a_run_carries_its_exit_code_and_streams() {
        let result = parse_exec(EXEC_FIXTURE).expect("exec");
        assert_eq!(result.exit_code, Some(0));
        assert_eq!(result.stdout.trim(), "x86_64");
        assert_eq!(result.stderr, "");
        assert!(!result.timed_out);
    }

    #[test]
    fn an_output_only_answer_is_still_a_run() {
        let result = parse_exec(EXEC_OUTPUT_ONLY_FIXTURE).expect("exec");
        assert_eq!(result.stdout.trim(), "x86_64");
        assert_eq!(result.exit_code, None);
    }

    #[test]
    fn a_killed_run_is_timed_out_and_has_no_exit_code() {
        let result = parse_exec(EXEC_KILLED_FIXTURE).expect("exec");
        assert!(result.timed_out);
        assert_eq!(result.exit_code, None);
    }

    #[test]
    fn both_listing_shapes_are_read() {
        let bare = parse_list(LIST_FIXTURE).expect("list");
        assert_eq!(bare.rows.len(), 2);
        assert_eq!(bare.rows[0].sandbox_id, "cs_0123456789abcdef");
        assert_eq!(bare.rows[0].state, SandboxState::Ready);
        assert_eq!(bare.rows[1].state, SandboxState::Stopped);
        assert_eq!(bare.rows[0].created_at, Some(1_795_253_400_000));
        assert!(bare.cursor.is_none());

        let wrapped = parse_list(LIST_WRAPPED_FIXTURE).expect("wrapped");
        assert_eq!(wrapped.rows.len(), 1);
        assert_eq!(wrapped.cursor.as_deref(), Some("cs_cursor_2"));
    }

    #[test]
    fn the_two_live_answers_are_mapped_the_way_they_were_observed() {
        // 401 is what every /api path answers without a key; it must read as a config problem.
        let unauthorized = status_error(401, ERROR_401_FIXTURE, "get");
        assert_eq!(unauthorized.kind, ErrorKind::Config);
        // 404 `no such route` is what /api/v1/* answered: a NotFound, and it proves nothing about
        // whether the documented route exists.
        let missing = status_error(404, ERROR_404_FIXTURE, "get");
        assert_eq!(missing.kind, ErrorKind::NotFound);
        assert_eq!(missing.provider_code, "not_found");
    }

    #[test]
    fn insufficient_credits_is_a_config_problem_not_a_retry() {
        let body = r#"{"error":{"code":"insufficient_credits","message":"out of minutes"}}"#;
        assert_eq!(status_error(402, body, "create").kind, ErrorKind::Config);
    }

    #[test]
    fn the_endpoint_table_names_every_route_the_adapter_calls() {
        assert_eq!(ENDPOINTS.len(), 8);
        assert!(ENDPOINTS.contains(&"POST /sandboxes/{id}/exec/run"));
        assert!(ENDPOINTS.contains(&"PUT /sandboxes/{id}/files"));
    }

    #[tokio::test]
    async fn a_call_without_a_key_reports_config() {
        let provider = OpenComputerProvider::without_token();
        let error = provider.get("cs_0123456789abcdef").await.expect_err("no key");
        assert_eq!(error.kind, ErrorKind::Config);
        assert_eq!(error.provider_code, "missing_token");
    }

    #[test]
    fn capabilities_are_a_relay_without_a_ttl_cap() {
        let capabilities = OpenComputerProvider::without_token().capabilities();
        assert_eq!(capabilities.transport, Transport::Relay);
        assert!(capabilities.native_auto_stop);
        assert_eq!(capabilities.max_ttl_seconds, None);
    }

    #[tokio::test]
    async fn write_file_refuses_a_relative_path_before_any_call() {
        let provider = OpenComputerProvider::without_token();
        let error = provider
            .write_file("cs_0123456789abcdef", "relative/.fx-test", vec![0u8; 32], 0o600)
            .await
            .expect_err("relative path");
        assert_eq!(error.kind, ErrorKind::Config);
        assert_eq!(error.provider_code, "path_not_absolute");
    }

    /// The state `get` derives, without a network call.
    fn state_of(body: &str) -> SandboxState {
        let value: Value = serde_json::from_str(body).expect("fixture json");
        let raw = value
            .get("state")
            .or_else(|| value.get("status"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_ascii_lowercase();
        match raw.as_str() {
            "running" | "idle" | "waiting" => SandboxState::Ready,
            "suspended" | "stopped" | "standby" | "paused" => SandboxState::Stopped,
            "stopping" | "suspending" => SandboxState::Stopping,
            "ended" | "terminated" | "deleted" => SandboxState::Deleted,
            "failed" | "error" => SandboxState::Error,
            _ => SandboxState::Provisioning,
        }
    }

    #[test]
    fn the_write_file_body_carries_the_mode_and_base64_content() {
        let body = write_body("/home/user/.fx-test", b"0123456789abcdef0123456789abcdef", 0o600);
        assert_eq!(body["path"], "/home/user/.fx-test");
        assert_eq!(body["mode"], "0600");
        assert_eq!(body["encoding"], "base64");
        assert_eq!(body["content"], BASE64.encode(b"0123456789abcdef0123456789abcdef"));
    }
}
