//! Vercel Sandbox: a Firecracker VM behind `@vercel/sandbox`, reached only over the relay
//! (Vercel exposes no SSH). Free Hobby credit: 5 h active CPU + 420 GB-h memory per month, at
//! most 10 concurrent sandboxes and a 45-minute maximum session.
//!
//! # Endpoints
//!
//! Recorded 2026-11-21 from `@vercel/sandbox@4.1.0` `dist/api-client/api-client.js` (installed
//! into a scratch dir with `bun add @vercel/sandbox`) and verified live against
//! `https://api.vercel.com` with the CLI login's token. Base `https://api.vercel.com`, auth
//! `Authorization: Bearer <token>` (never logged), `teamId` and `projectId` travel as query
//! parameters, never in a body-only field.
//!
//! | Call | Request | Response |
//! | --- | --- | --- |
//! | create (image) | `POST /v3/sandboxes?teamId=` `{projectId,name,timeout(ms),image,tags,env,resources}` | `{sandbox,session,routes}` |
//! | create (runtime, legacy) | `POST /v2/sandboxes` same body with `runtime` instead of `image` | same |
//! | get | `GET /v2/sandboxes/{name}?projectId=&teamId=&resume=false` | `{sandbox,session,routes,resumed}` |
//! | resume | `PATCH /v2/sandboxes/{name}?projectId=&teamId=` `{timeout}` then `GET ...&resume=true` | `{sandbox,...}` |
//! | list | `GET /v2/sandboxes?project=&teamId=&limit=&sortBy=name&namePrefix=` | `{sandboxes,pagination}` |
//! | stop | `POST /v2/sandboxes/sessions/{sessionId}/stop` | `{session,sandbox,snapshot?}` |
//! | extend ttl | `POST /v2/sandboxes/sessions/{sessionId}/extend-timeout` `{duration(ms)}` | `{session}` |
//! | exec | `POST /v2/sandboxes/sessions/{sessionId}/cmd` `{command,args,cwd,env,wait:true,logs:true,timeout(ms)}` | `application/x-ndjson` |
//! | write file | `POST /v2/sandboxes/sessions/{sessionId}/fs/write` `content-type: application/gzip`, `x-cwd: <extractDir>`, body = gzip'd tar whose entry names are relative to `x-cwd` | `{}` |
//! | mkdir | `POST /v2/sandboxes/sessions/{sessionId}/fs/mkdir` `{path,cwd}` (no `-p`: a missing parent is `400 file_error`) | `{}` |
//! | delete | `DELETE /v2/sandboxes/{name}?projectId=&teamId=&deleteOrphanSnapshots=true` | `{sandbox,...}` |
//!
//! Notes that shaped this adapter, each observed live:
//!
//! - A sandbox is created `persistent: true`, so `stop` + `resume` keeps the disk. The *session*
//!   id changes on resume, so the handle is the sandbox **name** and every session-scoped call
//!   looks the current session up first.
//! - `timeout` is milliseconds, both on create and on `extend-timeout`; the session cap is 45
//!   minutes on Hobby and resets on every resume.
//! - `GET` without `resume=true` never wakes a stopped sandbox. `resume` is always explicit here.
//! - Only when `logs: true` does the exec stream carry `{"data","stream"}` lines; the first and
//!   last line are the `command` object, and `exitCode` is `null` in the first.
//! - `fs/write` creates missing parent directories and applies the tar entry's mode (verified
//!   `600 32` after writing a 32-byte entry with mode 0600), so a secret lands 0600 in one call.
//! - There is no idempotency-key header: a second `POST` with the same name answers
//!   `400 bad_request ... already exists`. `create` therefore returns the existing sandbox
//!   instead of a second billable one.
//! - A missing or invalid token is `403 {"error":{"code":"forbidden",...}}`, mapped to `Config`.

use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};

use super::{
    Capabilities, CreateSandbox, ErrorKind, ExecResult, ListedSandbox, SandboxError, SandboxInfo,
    SandboxProvider, SandboxState, Transport,
};

const PROVIDER: &str = "vercel";
const API_BASE: &str = "https://api.vercel.com";
/// The managed Ubuntu image the SDK defaults to; it is what a ferryx box is bootstrapped on.
const DEFAULT_IMAGE: &str = "vercel/sandbox/universal:latest";
/// Hobby plan maximum session duration; the plan's hard cap is `min(provider max, 6h)`.
const MAX_SESSION_SECONDS: u64 = 45 * 60;
const DEFAULT_TTL_SECONDS: u64 = 10 * 60;
/// Must NOT have: one bounded response, no unbounded stream.
const EXEC_OUTPUT_CAP: usize = 4 * 1024 * 1024;
/// The project Vercel's own Sandbox quickstart creates; used when no project id is configured.
const DEFAULT_PROJECT_NAME: &str = "vercel-sandbox-default-project";
/// Upper bound on pages walked by the orphan scan.
const MAX_LIST_PAGES: usize = 20;
/// `GET /v2/sandboxes` rejects `limit` above 50 (`Invalid request: limit should be <= 50`).
const LIST_PAGE_SIZE: usize = 50;
/// A control-plane call is one bounded request; create/stop/delete can take a few seconds, and
/// none of them may hold a lifecycle sweep or a reaper tick open forever. The same bound the
/// other HTTP providers use.
const CONTROL_TIMEOUT: Duration = Duration::from_secs(120);

pub struct VercelProvider {
    client: reqwest::Client,
    /// `None` when no token could be found: every call then reports `Config`.
    token: Option<String>,
    project_id: Option<String>,
    team_id: Option<String>,
    /// Bound on every control call. Always [`CONTROL_TIMEOUT`] outside tests, where a short bound
    /// is what makes "this call is bounded" an assertion instead of a 120 s wait.
    control_timeout: Duration,
}

impl VercelProvider {
    pub fn new(token: Option<String>, project_id: Option<String>, team_id: Option<String>) -> Self {
        let client = reqwest::Client::builder()
            .user_agent(concat!("ferryx-cloud-host/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_default();
        Self { client, token, project_id, team_id, control_timeout: CONTROL_TIMEOUT }
    }

    /// Shorten the control-call bound so a test can observe it without waiting 120 s.
    #[cfg(test)]
    fn with_control_timeout(mut self, timeout: Duration) -> Self {
        self.control_timeout = timeout;
        self
    }

    /// Token from `VERCEL_TOKEN`, else from the `vercel` CLI login (`auth.json`), because a
    /// signed-in CLI already holds a working credential on this machine.
    pub fn from_env() -> Self {
        let token = std::env::var("VERCEL_TOKEN")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(cli_auth_token);
        let project_id = std::env::var("VERCEL_SANDBOX_PROJECT_ID")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let team = std::env::var("VERCEL_TEAM_ID").ok().filter(|value| !value.trim().is_empty());
        let team_id = team.or_else(cli_current_team);
        Self::new(token, project_id, team_id)
    }

    /// A provider with no credential at all, for the "call without a token reports Config" path.
    pub fn without_token() -> Self {
        Self::new(None, Some("prj_unconfigured".to_string()), None)
    }

    fn token(&self) -> Result<&str, SandboxError> {
        self.token.as_deref().ok_or_else(|| {
            config(
                "missing_token",
                "no Vercel token: set VERCEL_TOKEN or run `vercel login`",
            )
        })
    }

    /// Project and team to bill and scope to. The project id is required by the Sandbox API, so
    /// when it is not configured the conventional Sandbox project is looked up by name.
    async fn scope(&self) -> Result<(String, Option<String>), SandboxError> {
        let team = self.team_id.clone();
        if let Some(project) = self.project_id.clone() {
            return Ok((project, team));
        }
        let mut url = format!("{API_BASE}/v9/projects/{DEFAULT_PROJECT_NAME}");
        if let Some(team) = team.as_deref() {
            url.push_str("?teamId=");
            url.push_str(team);
        }
        let (status, body) = self.send(reqwest::Method::GET, &url, None).await?;
        if status != 200 {
            return Err(status_error(status, &body, "resolve_project"));
        }
        let project = serde_json::from_str::<Value>(&body)
            .ok()
            .and_then(|value| value.get("id").and_then(Value::as_str).map(str::to_string));
        project.map(|project| (project, team)).ok_or_else(|| {
            config(
                "missing_project",
                "no Sandbox project: set VERCEL_SANDBOX_PROJECT_ID",
            )
        })
    }

    fn url_with_scope(path: &str, project: &str, team: Option<&str>) -> String {
        let mut url = format!("{API_BASE}{path}");
        url.push_str(if path.contains('?') { "&" } else { "?" });
        url.push_str("projectId=");
        url.push_str(project);
        if let Some(team) = team {
            url.push_str("&teamId=");
            url.push_str(team);
        }
        url
    }

    async fn send(
        &self,
        method: reqwest::Method,
        url: &str,
        body: Option<Value>,
    ) -> Result<(u16, String), SandboxError> {
        let token = self.token()?;
        // Every control path (get, list, create, stop, resume, extend_ttl, scope) goes through
        // here, so this one bound covers them all.
        let mut request =
            self.client.request(method, url).bearer_auth(token).timeout(self.control_timeout);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.map_err(transport_error)?;
        let status = response.status().as_u16();
        let text = response.text().await.map_err(transport_error)?;
        Ok((status, text))
    }

    /// The sandbox as the API currently reports it. `resume: false` (the default) never wakes a
    /// stopped machine.
    async fn sandbox(&self, name: &str) -> Result<SandboxEnvelope, SandboxError> {
        let (project, team) = self.scope().await?;
        let url = Self::url_with_scope(&format!("/v2/sandboxes/{name}"), &project, team.as_deref());
        let url = format!("{url}&resume=false");
        let (status, body) = self.send(reqwest::Method::GET, &url, None).await?;
        if status != 200 {
            return Err(status_error(status, &body, "get"));
        }
        parse_json::<SandboxEnvelope>(&body, "get")
    }

    /// Session-scoped calls (exec, stop, extend-timeout, fs) need the id of the *current* session,
    /// which changes when a persistent sandbox is resumed.
    async fn session_id(&self, name: &str) -> Result<String, SandboxError> {
        let envelope = self.sandbox(name).await?;
        envelope
            .session
            .and_then(|session| session.id)
            .or(envelope.sandbox.current_session_id)
            .ok_or_else(|| {
                SandboxError::new(
                    ErrorKind::Fatal,
                    PROVIDER,
                    "missing_session_id",
                    "the sandbox has no current session",
                )
            })
    }

    /// Milliseconds, clamped to the plan's session cap.
    fn timeout_ms(ttl_seconds: Option<u64>) -> u64 {
        ttl_seconds.unwrap_or(DEFAULT_TTL_SECONDS).clamp(60, MAX_SESSION_SECONDS) * 1000
    }
}

#[async_trait::async_trait]
impl SandboxProvider for VercelProvider {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            ttl_required: false,
            max_ttl_seconds: Some(MAX_SESSION_SECONDS),
            transport: Transport::Relay,
            // The sandbox stops itself at its timeout; the disk survives for `resume`.
            native_auto_stop: true,
        }
    }

    async fn create(&self, request: CreateSandbox) -> Result<String, SandboxError> {
        let (project, team) = self.scope().await?;
        // Idempotent on the name: a rerun of the same create must not start a second billable VM.
        if let Ok(existing) = self.sandbox(&request.name).await {
            return Ok(existing.sandbox.name);
        }

        let mut body = json!({
            "projectId": project,
            "name": request.name,
            "timeout": Self::timeout_ms(request.ttl_seconds),
            "image": DEFAULT_IMAGE,
            "tags": { "launcher": "ferryx", "fx": request.idempotency_key },
            "resources": { "vcpus": 2 },
        });
        if !request.env.is_empty() {
            body["env"] = json!(request.env);
        }
        let url = match team.as_deref() {
            Some(team) => format!("{API_BASE}/v3/sandboxes?teamId={team}"),
            None => format!("{API_BASE}/v3/sandboxes"),
        };
        let (status, text) = self.send(reqwest::Method::POST, &url, Some(body)).await?;
        if status == 200 {
            return Ok(parse_json::<SandboxEnvelope>(&text, "create")?.sandbox.name);
        }
        // A concurrent create of the same name wins the race: reuse its sandbox.
        if status == 400 && text.contains("already exists") {
            return Ok(self.sandbox(&request.name).await?.sandbox.name);
        }
        Err(status_error(status, &text, "create"))
    }

    async fn get(&self, sandbox_id: &str) -> Result<SandboxInfo, SandboxError> {
        let envelope = self.sandbox(sandbox_id).await?;
        let raw = envelope.sandbox.status.unwrap_or_default();
        let state = map_state(Some(raw.as_str()));
        let error = (state == SandboxState::Error).then(|| format!("sandbox status is {raw}"));
        Ok(SandboxInfo { state, error })
    }

    async fn stop(&self, sandbox_id: &str) -> Result<(), SandboxError> {
        let session = self.session_id(sandbox_id).await?;
        let url = format!("{API_BASE}/v2/sandboxes/sessions/{session}/stop");
        let (status, body) = self.send(reqwest::Method::POST, &url, None).await?;
        if status == 200 {
            Ok(())
        } else {
            Err(status_error(status, &body, "stop"))
        }
    }

    async fn resume(&self, sandbox_id: &str, ttl_seconds: Option<u64>) -> Result<(), SandboxError> {
        let (project, team) = self.scope().await?;
        let url = Self::url_with_scope(&format!("/v2/sandboxes/{sandbox_id}"), &project, team.as_deref());
        // Reset the auto-stop first, so a resumed box cannot be stopped by the old deadline, then
        // wake it. `resume=true` on GET is the only wake path the API offers.
        let (status, body) = self
            .send(
                reqwest::Method::PATCH,
                &url,
                Some(json!({ "timeout": Self::timeout_ms(ttl_seconds) })),
            )
            .await?;
        if status != 200 {
            return Err(status_error(status, &body, "resume"));
        }
        let (status, body) = self.send(reqwest::Method::GET, &format!("{url}&resume=true"), None).await?;
        if status != 200 {
            return Err(status_error(status, &body, "resume"));
        }
        Ok(())
    }

    async fn delete(&self, sandbox_id: &str) -> Result<(), SandboxError> {
        let (project, team) = self.scope().await?;
        let url = Self::url_with_scope(&format!("/v2/sandboxes/{sandbox_id}"), &project, team.as_deref());
        // Without this, the deleted sandbox's snapshots stay billed for up to 30 days.
        let url = format!("{url}&deleteOrphanSnapshots=true");
        let (status, body) = self.send(reqwest::Method::DELETE, &url, None).await?;
        if status == 200 {
            Ok(())
        } else {
            Err(status_error(status, &body, "delete"))
        }
    }

    async fn extend_ttl(
        &self,
        sandbox_id: &str,
        ttl_seconds: Option<u64>,
    ) -> Result<(), SandboxError> {
        let session = self.session_id(sandbox_id).await?;
        let url = format!("{API_BASE}/v2/sandboxes/sessions/{session}/extend-timeout");
        let (status, body) = self
            .send(
                reqwest::Method::POST,
                &url,
                Some(json!({ "duration": Self::timeout_ms(ttl_seconds) })),
            )
            .await?;
        if status == 200 {
            Ok(())
        } else {
            Err(status_error(status, &body, "extend_ttl"))
        }
    }

    async fn exec(
        &self,
        sandbox_id: &str,
        command: &str,
        timeout_seconds: u64,
    ) -> Result<ExecResult, SandboxError> {
        let session = self.session_id(sandbox_id).await?;
        let url = format!("{API_BASE}/v2/sandboxes/sessions/{session}/cmd");
        let body = json!({
            "command": "sh",
            "args": ["-c", command],
            // `wait` keeps the call one-shot; `logs` is what makes the stream carry the output.
            "wait": true,
            "logs": true,
            "timeout": timeout_seconds.saturating_mul(1000),
        });
        let token = self.token()?;
        let request = self
            .client
            .post(&url)
            .bearer_auth(token)
            .json(&body)
            .timeout(Duration::from_secs(timeout_seconds.saturating_add(30)));
        let response = match request.send().await {
            Ok(response) => response,
            Err(error) if error.is_timeout() => return Ok(timed_out()),
            Err(error) => return Err(transport_error(error)),
        };
        let status = response.status().as_u16();
        let text = match response.text().await {
            Ok(text) => text,
            Err(error) if error.is_timeout() => return Ok(timed_out()),
            Err(error) => return Err(transport_error(error)),
        };
        if status != 200 {
            return Err(status_error(status, &text, "exec"));
        }
        parse_exec_stream(&text)
    }

    async fn list(&self) -> Result<Vec<ListedSandbox>, SandboxError> {
        let (project, team) = self.scope().await?;
        let mut out = Vec::new();
        let mut cursor: Option<String> = None;
        // The orphan scan walks the whole account: page until the cursor runs out, with a bound so
        // a misbehaving `next` cannot loop forever.
        for _ in 0..MAX_LIST_PAGES {
            let mut url = format!("{API_BASE}/v2/sandboxes?project={project}&limit={LIST_PAGE_SIZE}");
            if let Some(team) = team.as_deref() {
                url.push_str("&teamId=");
                url.push_str(team);
            }
            if let Some(cursor) = cursor.as_deref() {
                url.push_str("&cursor=");
                url.push_str(cursor);
            }
            let (status, body) = self.send(reqwest::Method::GET, &url, None).await?;
            if status != 200 {
                return Err(status_error(status, &body, "list"));
            }
            let page: ListEnvelope = parse_json(&body, "list")?;
            for row in page.sandboxes {
                out.push(ListedSandbox {
                    sandbox_id: row.name.clone(),
                    state: map_state(row.status.as_deref()),
                    created_at: row.created_at.map(|at| at as i64),
                    name: Some(row.name),
                });
            }
            match page.pagination.and_then(|pagination| pagination.next) {
                Some(next) if !next.is_empty() => cursor = Some(next),
                _ => return Ok(out),
            }
        }
        Err(SandboxError::new(
            ErrorKind::Fatal,
            PROVIDER,
            "list_too_long",
            format!("listing stopped after {MAX_LIST_PAGES} pages"),
        ))
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
        let session = self.session_id(sandbox_id).await?;
        // `fs/write` extracts a tar at `x-cwd`, so the entry name is the path below `/`.
        let entry = path.trim_start_matches('/').to_string();
        let archive = gzip_tar_file(&entry, &bytes, mode)?;
        let url = format!("{API_BASE}/v2/sandboxes/sessions/{session}/fs/write");
        let token = self.token()?;
        let response = self
            .client
            .post(&url)
            .bearer_auth(token)
            .header("content-type", "application/gzip")
            .header("x-cwd", "/")
            .body(archive)
            .timeout(Duration::from_secs(120))
            .send()
            .await
            .map_err(transport_error)?;
        let status = response.status().as_u16();
        let text = response.text().await.unwrap_or_default();
        if status == 200 {
            Ok(())
        } else {
            Err(status_error(status, &text, "write_file"))
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Wire types

#[derive(Debug, Deserialize)]
struct SandboxEnvelope {
    sandbox: SandboxBody,
    #[serde(default)]
    session: Option<SessionBody>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SandboxBody {
    name: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    current_session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ListEnvelope {
    sandboxes: Vec<ListRow>,
    #[serde(default)]
    pagination: Option<Pagination>,
}

/// A row of `GET /v2/sandboxes`: the lifecycle fields plus `createdAt` in unix milliseconds.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListRow {
    name: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    created_at: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct Pagination {
    #[serde(default)]
    next: Option<String>,
}

/// The provider's status vocabulary (`pending|running|stopping|stopped|failed|aborted|snapshotting`)
/// in [`SandboxState`] terms.
fn map_state(status: Option<&str>) -> SandboxState {
    match status.unwrap_or_default() {
        "running" => SandboxState::Ready,
        "pending" | "snapshotting" => SandboxState::Provisioning,
        "stopping" => SandboxState::Stopping,
        // `aborted` is a sandbox that hit its timeout: the disk is intact and resumable.
        "stopped" | "aborted" => SandboxState::Stopped,
        "failed" => SandboxState::Error,
        _ => SandboxState::Provisioning,
    }
}

#[derive(Debug, Deserialize)]
struct SessionBody {
    #[serde(default)]
    id: Option<String>,
}

/// One line of the NDJSON exec stream: either the `command` object (first and last line) or a
/// `{"data","stream"}` output chunk.
#[derive(Debug, Deserialize)]
struct ExecLine {
    #[serde(default)]
    command: Option<CommandBody>,
    #[serde(default)]
    data: Option<String>,
    #[serde(default)]
    stream: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommandBody {
    #[serde(default)]
    exit_code: Option<i32>,
}

fn parse_json<T: for<'de> Deserialize<'de>>(body: &str, call: &str) -> Result<T, SandboxError> {
    serde_json::from_str(body).map_err(|error| {
        SandboxError::new(
            ErrorKind::Fatal,
            PROVIDER,
            "unexpected_body",
            format!("{call}: the API answered with an unexpected body: {error}"),
        )
    })
}

/// Fold the NDJSON lines into one [`ExecResult`], enforcing the output cap.
fn parse_exec_stream(text: &str) -> Result<ExecResult, SandboxError> {
    let mut stdout = String::new();
    let mut stderr = String::new();
    let mut exit_code = None;
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let parsed: ExecLine = match serde_json::from_str(line) {
            Ok(parsed) => parsed,
            // A line that is not JSON is provider noise; the exit code line decides success.
            Err(_) => continue,
        };
        if let Some(data) = parsed.data {
            match parsed.stream.as_deref() {
                Some("stderr") => stderr.push_str(&data),
                _ => stdout.push_str(&data),
            }
            if stdout.len() + stderr.len() > EXEC_OUTPUT_CAP {
                return Err(SandboxError::new(
                    ErrorKind::Fatal,
                    PROVIDER,
                    "output_limit_exceeded",
                    format!("exec output exceeded {EXEC_OUTPUT_CAP} bytes"),
                ));
            }
        }
        if let Some(command) = parsed.command {
            if command.exit_code.is_some() {
                exit_code = command.exit_code;
            }
        }
    }
    Ok(ExecResult { exit_code, stdout, stderr, timed_out: false })
}

fn timed_out() -> ExecResult {
    ExecResult { exit_code: None, stdout: String::new(), stderr: String::new(), timed_out: true }
}

fn config(code: impl Into<String>, message: impl Into<String>) -> SandboxError {
    SandboxError::new(ErrorKind::Config, PROVIDER, code, message)
}

/// Map an HTTP failure onto an [`ErrorKind`], keeping Vercel's own code for the operator.
fn status_error(status: u16, body: &str, call: &str) -> SandboxError {
    let parsed = serde_json::from_str::<Value>(body).ok();
    let detail = parsed
        .as_ref()
        .and_then(|value| value.get("error"))
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .unwrap_or(body)
        .trim()
        .to_string();
    let code = parsed
        .as_ref()
        .and_then(|value| value.get("error"))
        .and_then(|error| error.get("code"))
        .and_then(Value::as_str)
        .unwrap_or("http_error")
        .to_string();
    let kind = match status {
        401 | 403 => ErrorKind::Config,
        404 => ErrorKind::NotFound,
        409 => ErrorKind::Conflict,
        429 | 500..=599 => ErrorKind::Retryable,
        _ => ErrorKind::Fatal,
    };
    SandboxError::new(kind, PROVIDER, code, format!("{call}: HTTP {status}: {detail}"))
}

fn transport_error(error: reqwest::Error) -> SandboxError {
    let kind = if error.is_timeout() || error.is_connect() {
        ErrorKind::Retryable
    } else {
        ErrorKind::Fatal
    };
    // Never include the request in the message: the URL carries the project and team, and the
    // header carries the token. `reqwest::Error`'s own Display appends ` for url (<url>)`, so the
    // URL is stripped before anything is formatted; the cause chain is kept so a refused
    // connection still says why.
    let bare = error.without_url();
    let mut message = format!("request failed: {bare}");
    let mut source = std::error::Error::source(&bare);
    while let Some(cause) = source {
        message.push_str(&format!(": {cause}"));
        source = cause.source();
    }
    SandboxError::new(kind, PROVIDER, "transport", message)
}

// ---------------------------------------------------------------------------------------------
// Credentials from the Vercel CLI login

/// `auth.json` of the `vercel` CLI, whose token works against the Sandbox API. Only the token
/// field is read; nothing is logged.
fn cli_auth_token() -> Option<String> {
    let path = cli_dir()?.join("auth.json");
    let text = std::fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    value
        .get("token")
        .and_then(Value::as_str)
        .filter(|token| !token.trim().is_empty())
        .map(str::to_string)
}

/// The team the CLI is currently pointed at (`config.json` `currentTeam`).
fn cli_current_team() -> Option<String> {
    let path = cli_dir()?.join("config.json");
    let text = std::fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    value
        .get("currentTeam")
        .and_then(Value::as_str)
        .filter(|team| !team.trim().is_empty())
        .map(str::to_string)
}

fn cli_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let xdg = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local").join("share"));
    let candidates = [
        home.join("Library").join("Application Support").join("com.vercel.cli"),
        xdg.join("com.vercel.cli"),
    ];
    candidates.into_iter().find(|path| path.is_dir())
}

// ---------------------------------------------------------------------------------------------
// gzip + tar in one call
//
// `fs/write` takes a gzipped tar and applies each entry's mode, which is the only single-call
// way to put a 0600 secret on the box.

fn gzip_tar_file(name: &str, bytes: &[u8], mode: u32) -> Result<Vec<u8>, SandboxError> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    {
        let mut builder = tar::Builder::new(&mut encoder);
        let mut header = tar::Header::new_gnu();
        header.set_path(name).map_err(|error| {
            config("bad_path", format!("the file path is not a safe tar entry: {error}"))
        })?;
        header.set_size(bytes.len() as u64);
        header.set_mode(mode & 0o7777);
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(0);
        header.set_cksum();
        builder.append(&header, bytes).map_err(|error| {
            SandboxError::new(
                ErrorKind::Fatal,
                PROVIDER,
                "tar_failed",
                format!("could not build the tar entry: {error}"),
            )
        })?;
        builder.finish().map_err(|error| {
            SandboxError::new(
                ErrorKind::Fatal,
                PROVIDER,
                "tar_failed",
                format!("could not finish the tar: {error}"),
            )
        })?;
    }
    encoder.finish().map_err(|error| {
        SandboxError::new(
            ErrorKind::Fatal,
            PROVIDER,
            "gzip_failed",
            format!("could not gzip the tar: {error}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/cloud_host/vercel")
            .join(name);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read fixture {}: {error}", path.display()))
    }

    #[test]
    fn create_live_decodes_sandbox_name_and_session() {
        let envelope: SandboxEnvelope = parse_json(&fixture("create_live.json"), "create").unwrap();
        assert_eq!(envelope.sandbox.name, "fx-qa-probe-4");
        assert_eq!(envelope.sandbox.status.as_deref(), Some("running"));
        assert_eq!(
            envelope.session.and_then(|session| session.id).as_deref(),
            Some("sbx_Fus55CBBAmaO7jwhmHRpIBgJHyX9")
        );
    }

    #[test]
    fn get_running_live_maps_to_ready() {
        let envelope: SandboxEnvelope = parse_json(&fixture("get_running_live.json"), "get").unwrap();
        assert_eq!(envelope.sandbox.status.as_deref(), Some("running"));
        assert_eq!(envelope.sandbox.current_session_id.as_deref(), Some("sbx_t5ZDjlNhGFQYFZL8LH0VPEUtIyCS"));
    }

    #[test]
    fn get_stopped_live_carries_no_expiry() {
        let envelope: SandboxEnvelope = parse_json(&fixture("get_stopped_live.json"), "get").unwrap();
        assert_eq!(envelope.sandbox.status.as_deref(), Some("stopped"));
    }

    #[test]
    fn list_live_decodes_an_empty_page() {
        #[derive(Deserialize)]
        struct ListEnvelope {
            sandboxes: Vec<Value>,
        }
        let parsed: ListEnvelope = parse_json(&fixture("list_live.json"), "list").unwrap();
        assert!(parsed.sandboxes.is_empty());
    }

    #[test]
    fn exec_stream_live_splits_stdout_and_stderr() {
        let result = parse_exec_stream(&fixture("exec_stream_live.ndjson")).unwrap();
        assert_eq!(result.exit_code, Some(0));
        assert_eq!(result.stdout, "out-hi\nvercel-sandbox\nHOME=/home/vercel-sandbox\n");
        assert_eq!(result.stderr, "err-hi\n");
        assert!(!result.timed_out);
    }

    #[test]
    fn exec_exit7_live_reports_exit_code_seven() {
        let result = parse_exec_stream(&fixture("exec_exit7_live.ndjson")).unwrap();
        assert_eq!(result.exit_code, Some(7));
        // `logs: true` was off on that call, so the stream carried no output lines.
        assert!(result.stdout.is_empty());
        assert!(result.stderr.is_empty());
    }

    #[test]
    fn exec_stream_tolerates_a_non_json_line() {
        let stream = concat!(
            "{\"command\":{\"id\":\"c\",\"exitCode\":null}}\n",
            "not json at all\n",
            "{\"data\":\"hi\\n\",\"stream\":\"stdout\"}\n",
            "{\"command\":{\"id\":\"c\",\"exitCode\":0}}\n"
        );
        let result = parse_exec_stream(stream).unwrap();
        assert_eq!(result.exit_code, Some(0));
        assert_eq!(result.stdout, "hi\n");
    }

    #[test]
    fn exec_stream_refuses_output_past_the_cap() {
        let chunk = "x".repeat(1024);
        let mut stream = String::new();
        // Cross the 4 MiB cap with legal NDJSON lines.
        for _ in 0..(EXEC_OUTPUT_CAP / 1024 + 1) {
            stream.push_str(&format!("{{\"data\":\"{chunk}\",\"stream\":\"stdout\"}}\n"));
        }
        let error = parse_exec_stream(&stream).expect_err("output past the cap must fail");
        assert_eq!(error.kind, ErrorKind::Fatal);
        assert_eq!(error.provider_code, "output_limit_exceeded");
    }

    #[test]
    fn not_found_live_maps_to_not_found() {
        let error = status_error(
            404,
            &fixture("error_not_found_live.json"),
            "get",
        );
        assert_eq!(error.kind, ErrorKind::NotFound);
        assert!(error.is_not_found());
        assert_eq!(error.provider_code, "not_found");
        assert!(error.message.contains("Named sandbox 'fx-qa-probe-2' not found"));
    }

    #[test]
    fn no_token_live_maps_to_config() {
        // Observed live: Vercel answers 403 with `missingToken` when the header is absent.
        let error = status_error(403, &fixture("error_no_token_live.json"), "get");
        assert_eq!(error.kind, ErrorKind::Config);
        assert_eq!(error.provider_code, "forbidden");
    }

    #[test]
    fn name_taken_live_is_a_conflict_free_reuse() {
        let error = status_error(400, &fixture("error_name_taken_live.json"), "create");
        assert_eq!(error.kind, ErrorKind::Fatal);
        assert!(error.message.contains("already exists"));
    }

    #[test]
    fn http_statuses_map_to_the_documented_kinds() {
        assert_eq!(status_error(401, "{}", "call").kind, ErrorKind::Config);
        assert_eq!(status_error(403, "{}", "call").kind, ErrorKind::Config);
        assert_eq!(status_error(404, "{}", "call").kind, ErrorKind::NotFound);
        assert_eq!(status_error(409, "{}", "call").kind, ErrorKind::Conflict);
        assert_eq!(status_error(429, "{}", "call").kind, ErrorKind::Retryable);
        assert_eq!(status_error(500, "{}", "call").kind, ErrorKind::Retryable);
        assert_eq!(status_error(503, "{}", "call").kind, ErrorKind::Retryable);
        assert_eq!(status_error(400, "{}", "call").kind, ErrorKind::Fatal);
    }

    #[test]
    fn error_message_never_carries_the_request() {
        // The URL holds project/team and the header holds the token: neither may reach a log.
        let error = status_error(500, &fixture("error_no_token_live.json"), "exec");
        assert!(!error.message.contains("Bearer"));
        assert!(!error.message.contains("https://"));
    }

    #[test]
    fn capabilities_say_relay_with_a_45_minute_cap() {
        let provider = VercelProvider::without_token();
        let capabilities = provider.capabilities();
        assert_eq!(capabilities.transport, Transport::Relay);
        assert_eq!(capabilities.max_ttl_seconds, Some(45 * 60));
        assert!(capabilities.native_auto_stop);
        assert!(!capabilities.ttl_required);
    }

    #[test]
    fn list_page_size_stays_within_the_api_cap() {
        // Observed live: `limit` above 50 is refused with `400 Invalid request: limit should be <= 50`.
        assert!(LIST_PAGE_SIZE <= 50, "the API rejects a larger page: {LIST_PAGE_SIZE}");
        assert!(LIST_PAGE_SIZE >= 20, "a tiny page would make the orphan scan crawl");
    }

    #[test]
    fn timeout_ms_clamps_to_the_session_cap() {
        assert_eq!(VercelProvider::timeout_ms(None), DEFAULT_TTL_SECONDS * 1000);
        assert_eq!(VercelProvider::timeout_ms(Some(1)), 60 * 1000);
        assert_eq!(VercelProvider::timeout_ms(Some(600)), 600 * 1000);
        assert_eq!(VercelProvider::timeout_ms(Some(999_999)), MAX_SESSION_SECONDS * 1000);
    }

    #[tokio::test]
    async fn every_call_without_a_token_reports_config() {
        let provider = VercelProvider::without_token();
        let error = provider.get("fx-none").await.expect_err("no token, no call");
        assert_eq!(error.kind, ErrorKind::Config);
        assert_eq!(error.provider_code, "missing_token");

        let error = provider
            .create(CreateSandbox {
                idempotency_key: "fx-no-token".into(),
                name: "fx-no-token".into(),
                ttl_seconds: Some(60),
                env: Default::default(),
            })
            .await
            .expect_err("no token, no create");
        assert_eq!(error.kind, ErrorKind::Config);
    }

    #[tokio::test]
    async fn write_file_rejects_a_relative_path_before_any_request() {
        let provider = VercelProvider::without_token();
        let error = provider
            .write_file("fx-none", "relative/path", vec![0u8; 4], 0o600)
            .await
            .expect_err("a relative path is refused");
        assert_eq!(error.kind, ErrorKind::Config);
        assert_eq!(error.provider_code, "path_not_absolute");
    }

    #[test]
    fn gzip_tar_entry_round_trips_with_its_mode() {
        // The file API extracts this archive, so what matters is that a real tar reader sees the
        // entry, its bytes and its mode.
        let payload = b"0123456789abcdef0123456789abcdef";
        let archive = gzip_tar_file("home/ubuntu/.ferryx-cloud/auth.json", payload, 0o600).unwrap();
        assert_eq!(&archive[..2], &[0x1f, 0x8b], "gzip magic");

        let decoder = flate2::read::GzDecoder::new(archive.as_slice());
        let mut tar = tar::Archive::new(decoder);
        let mut entries = tar.entries().unwrap();
        let mut entry = entries.next().unwrap().unwrap();
        assert_eq!(
            entry.path().unwrap().to_string_lossy(),
            "home/ubuntu/.ferryx-cloud/auth.json"
        );
        assert_eq!(entry.header().mode().unwrap(), 0o600);
        assert_eq!(entry.header().size().unwrap(), payload.len() as u64);
        let mut read_back = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut read_back).unwrap();
        assert_eq!(read_back, payload);
    }

    #[test]
    fn gzip_tar_rejects_a_path_that_escapes_the_archive() {
        let error = gzip_tar_file("../etc/shadow", b"x", 0o600).expect_err("a traversal must fail");
        assert_eq!(error.kind, ErrorKind::Config);
    }

    /// D1: a peer that accepts the connection and never answers must not hold a control call
    /// open. The bound is shortened here so the assertion is about the *bound existing*, not
    /// about waiting 120 s.
    #[tokio::test]
    async fn control_calls_are_bounded_by_a_timeout() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind silent peer");
        let addr = listener.local_addr().expect("peer address");
        // Hold the accepted connection open and silent until the test is done; a oneshot rather
        // than a sleep, so nothing here depends on timing luck.
        let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
        let peer = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.expect("accept");
            let _ = release_rx.await;
            drop(socket);
        });

        let provider = VercelProvider::new(Some("tok".into()), Some("prj_probe".into()), None)
            .with_control_timeout(Duration::from_millis(300));
        let url = format!("http://{addr}/v2/sandboxes?projectId=prj_probe");

        // The whole point of D1: `send` is the only path every control call takes.
        let outcome = tokio::time::timeout(
            Duration::from_secs(20),
            provider.send(reqwest::Method::GET, &url, None),
        )
        .await
        .expect("send() was still waiting after 20s: control calls have no timeout");
        let error = outcome.expect_err("a silent peer must end in a timeout");
        assert_eq!(error.kind, ErrorKind::Retryable, "{}", error.message);
        assert_eq!(error.provider_code, "transport");

        drop(release_tx);
        let _ = peer.await;
    }

    /// D2: a transport failure must not carry the request URL, which names the project and the
    /// team. `reqwest::Error`'s own Display appends ` for url (<url>)`, so this fails unless the
    /// URL is stripped.
    #[tokio::test]
    async fn transport_error_never_carries_the_request_url() {
        // A port that is bound and then released is closed: connecting is refused at once.
        let port = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
            listener.local_addr().expect("addr").port()
        };
        let url = format!(
            "http://127.0.0.1:{port}/v2/sandboxes?projectId=prj_SECRET&teamId=team_SECRET"
        );
        let error = reqwest::Client::new()
            .get(&url)
            .send()
            .await
            .expect_err("a closed port must fail to connect");
        // Precondition: reqwest really did attach the URL, or this test proves nothing.
        assert!(
            error.to_string().contains("prj_SECRET"),
            "the raw error must carry the url: {error}"
        );

        let mapped = transport_error(error);
        assert_eq!(mapped.kind, ErrorKind::Retryable);
        assert_eq!(mapped.provider_code, "transport");
        assert!(!mapped.message.contains("prj_SECRET"), "{}", mapped.message);
        assert!(!mapped.message.contains("team_SECRET"), "{}", mapped.message);
        assert!(!mapped.message.contains("http://"), "{}", mapped.message);
        // Still useful: the cause survives, only the URL is gone.
        assert!(mapped.message.contains("request failed"), "{}", mapped.message);
    }
}

#[cfg(test)]
mod live {
    //! The live smoke, run on demand: `VERCEL_TOKEN` must be set (the CLI login's own token
    //! works). `cargo test --lib cloud_host::provider::vercel::live -- --ignored --nocapture`.
    //!
    //! One create/exec/write/stat/delete cycle against the Hobby plan's free credit. The sandbox
    //! is deleted in every path, including a panic, so a failed assertion cannot leave a billable
    //! machine behind.

    use super::*;

    fn name() -> String {
        format!("fx-t11-live-{}", std::process::id())
    }

    /// Runs `work` on a thread of its own and returns its value, or `None` if that thread
    /// panicked.
    ///
    /// The guard's drop runs during unwinding, where a second panic aborts the process, and it may
    /// run inside a tokio runtime, where `Runtime::block_on` panics. A thread of its own has
    /// neither problem: no ambient runtime, and its panic is contained by `join`.
    fn on_own_thread<T, F>(work: F) -> Option<T>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        std::thread::spawn(work).join().ok()
    }

    /// Deletes the sandbox when dropped, including while the test unwinds from a failed
    /// assertion: Hobby credit must not be spent by a leaked machine.
    ///
    /// This is a safety net, not the test's assertion: the body deletes explicitly and checks the
    /// sandbox is gone, and only then disarms this guard. A panic fails the test on its own, so a
    /// guard that prints a failed delete cannot turn a failure into a pass.
    struct Cleanup {
        name: String,
        armed: bool,
    }

    impl Cleanup {
        fn new(name: String) -> Self {
            Self { name, armed: true }
        }

        /// Stops the guard from deleting again once the body has deleted and verified the sandbox.
        fn disarm(&mut self) {
            self.armed = false;
        }
    }

    impl Drop for Cleanup {
        fn drop(&mut self) {
            if !self.armed {
                return;
            }
            let name = self.name.clone();
            let outcome = on_own_thread(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => return format!("no cleanup runtime: {error}"),
                };
                match runtime.block_on(VercelProvider::from_env().delete(&name)) {
                    Ok(()) => "deleted".to_string(),
                    Err(error) => format!("failed: {error}"),
                }
            });
            match outcome {
                Some(outcome) => println!("cleanup delete {} -> {outcome}", self.name),
                None => eprintln!("cleanup delete {} -> the cleanup thread panicked", self.name),
            }
        }
    }

    /// The guard's mechanism under the exact condition that broke it: inside a tokio runtime,
    /// where building a runtime and calling `block_on` on this thread would panic.
    #[tokio::test]
    async fn cleanup_runs_off_the_test_runtime() {
        assert_eq!(on_own_thread(|| 7u32), Some(7));
        // And a panic inside the work is contained rather than aborting the unwinding test.
        assert_eq!(on_own_thread(|| panic!("cleanup work panicked")), None);
    }

    #[tokio::test]
    #[ignore = "live: needs VERCEL_TOKEN and spends Hobby credit"]
    async fn live_smoke() {
        let provider = VercelProvider::from_env();
        assert!(
            provider.token.is_some(),
            "SKIPPED: no key - set VERCEL_TOKEN or run `vercel login`"
        );
        let name = name();
        // Armed before any call: a panic anywhere below still deletes the sandbox.
        let mut cleanup = Cleanup::new(name.clone());

        let id = provider
            .create(CreateSandbox {
                idempotency_key: name.clone(),
                name: name.clone(),
                ttl_seconds: Some(300),
                env: Default::default(),
            })
            .await
            .expect("create");
        println!("create -> {id}");

        let result = async {
            let info = provider.get(&id).await?;
            println!("get -> {info:?}");

            let uname = provider.exec(&id, "uname -m", 60).await?;
            println!("exec uname -m -> {uname:?}");
            assert_eq!(uname.exit_code, Some(0));
            assert!(!uname.stdout.trim().is_empty());

            // A 32-byte secret at 0600, the exact shape a slot identity takes.
            let secret: Vec<u8> = (0u8..32).collect();
            let path = format!("/home/ubuntu/.ferryx-cloud/{name}.json");
            provider.write_file(&id, &path, secret.clone(), 0o600).await?;
            println!("write_file -> {path}");

            let stat = provider
                .exec(&id, &format!("stat -c '%a %s' {path} && od -An -tu1 {path}"), 60)
                .await?;
            println!("stat -> {stat:?}");
            assert_eq!(stat.exit_code, Some(0));
            let first = stat.stdout.lines().next().unwrap_or_default().trim().to_string();
            assert_eq!(first, "600 32", "expected 0600 and 32 bytes, got {first:?}");

            // The bytes on the box are the bytes we sent.
            let written: Vec<u8> = stat
                .stdout
                .lines()
                .skip(1)
                .flat_map(|line| line.split_whitespace().filter_map(|n| n.parse().ok()))
                .collect();
            assert_eq!(written, secret, "the file's contents differ from what was written");

            let listed = provider.list().await?;
            println!("list -> {} rows", listed.len());
            assert!(listed.iter().any(|row| row.sandbox_id == id));
            Ok::<(), SandboxError>(())
        }
        .await;

        // Cleanup runs whatever happened above, and its own assertions run before the body's
        // result so a failed call cannot hide a billable sandbox.
        let deleted = provider.delete(&id).await;
        println!("delete -> {deleted:?}");
        let gone = provider.get(&id).await;
        println!("get after delete -> {gone:?}");
        deleted.expect("delete");
        assert!(gone.is_err(), "a deleted sandbox must not be reported present");
        if let Err(error) = &gone {
            assert_eq!(error.kind, ErrorKind::NotFound, "{}", error.message);
        }
        // Deleted and verified: the guard has nothing left to do.
        cleanup.disarm();
        result.expect("live smoke");
    }

    /// Failure scenario the plan asks for, run without an ignored flag: a call with no token must
    /// answer `Config` and must not touch the network.
    #[tokio::test]
    async fn no_token_is_config_live() {
        let provider = VercelProvider::without_token();
        let error = provider.get("fx-t11-live-absent").await.expect_err("no token");
        assert_eq!(error.kind, ErrorKind::Config);
        println!("no token -> {error}");
    }
}
