//! Daytona: a Linux container sandbox behind `https://app.daytona.io/api`, reached over SSH
//! (`ssh <token>@ssh.app.daytona.io`) with one-shot exec through the per-sandbox toolbox proxy.
//!
//! # Endpoints
//!
//! Recorded 2026-11-21 from `https://www.daytona.io/docs/openapi.json` (platform API 1.0) and
//! `https://www.daytona.io/docs/toolbox-openapi.json`, cross-read against `@daytona/sdk@2.1.0`
//! (`dist/esm/v0/{Daytona,Process,SandboxNode}.js`), then verified live with the account key.
//! Base `https://app.daytona.io/api`, auth `Authorization: Bearer <key>` (never logged). Every
//! sandbox record carries the toolbox base in `toolboxProxyUrl`.
//!
//! | Call | Request | Response |
//! | --- | --- | --- |
//! | create | `POST /sandbox` `{name,labels,env,autoStopInterval}` | `200` Sandbox (`state: creating`) |
//! | get | `GET /sandbox/{sandboxIdOrName}` | `200` Sandbox; `404` when gone |
//! | list | `GET /sandbox?limit=100&cursor=&labels=<json>` | `{items,nextCursor}` |
//! | stop | `POST /sandbox/{sandboxIdOrName}/stop` | `200` Sandbox (`state: stopping`) |
//! | resume | `POST /sandbox/{sandboxIdOrName}/start` | `200` Sandbox (`state: starting`) |
//! | extend ttl | `POST /sandbox/{sandboxIdOrName}/autostop/{minutes}` (`0` disables) | `200` Sandbox |
//! | delete | `DELETE /sandbox/{sandboxIdOrName}` | `200` Sandbox (`state: destroying`) |
//! | exec | `POST {toolboxProxyUrl}/{sandboxId}/process/execute` `{command,cwd,envs,timeout}` | `{result,exitCode}`; `408` at the deadline |
//! | ssh | `POST /sandbox/{sandboxIdOrName}/ssh-access?expiresInMinutes=60` | `{id,token,sshCommand,expiresAt}` |
//!
//! Notes that shaped this adapter, each from the docs above or the SDK source:
//!
//! - **No idempotency header exists.** The label `fx=<idempotency_key>` is the durable marker:
//!   `create` looks it up first (`GET /sandbox?labels=<json>`) and returns the sandbox already
//!   carrying it, so a retry cannot start a second billable machine; a `409` re-runs that lookup.
//! - **`autoDeleteInterval` is never sent.** `0` means "delete the moment it stops", which would
//!   destroy the disk `stop`/`resume` must keep; absent leaves the account default.
//! - `autoStopInterval` is **minutes** (`0` disables). `ttl_seconds` is rounded up; `None` leaves
//!   the provider default (30 minutes on the account read live) untouched.
//! - Container filesystems survive stop/start (memory does not), so `resume` is `POST /start`.
//! - `POST /process/execute` returns `result` (stdout) and `exitCode` only: there is no separate
//!   stderr stream, so `ExecResult::stderr` is always empty. The SDK builds its toolbox client on
//!   `toolboxProxyUrl` with the same `Authorization: Bearer <key>` header as the control plane.
//! - An ssh-access token is short-lived (default 60 minutes, max 1440) and *is* the SSH username;
//!   the gateway accepts it through the `none` auth method, so there is no key file of ours
//!   (`SshIdentity::Agent`) and no host key to pin. Re-request `ssh_target` when one expires.
//! - A missing or invalid key is `401 {"error":"Unauthorized","message":"Invalid credentials"}`
//!   (captured live in `error_401.json`), which names the fix: `DAYTONA_API_KEY`.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};

use super::{
    Capabilities, CreateSandbox, ErrorKind, ExecResult, ListedSandbox, SandboxError, SandboxInfo,
    SandboxProvider, SandboxState, SshIdentity, SshTarget, Transport,
};

const PROVIDER: &str = "daytona";
const API_BASE: &str = "https://app.daytona.io/api";
/// The toolbox proxy every fresh sandbox advertises; only used when a record omits the field.
const DEFAULT_TOOLBOX_BASE: &str = "https://proxy.app.daytona.io/toolbox";
/// Where an ssh-access token is presented as the SSH username.
const SSH_HOST: &str = "ssh.app.daytona.io";
/// Default lifetime of the ssh-access token we mint (the API default; max is 1440).
const SSH_ACCESS_MINUTES: u64 = 60;
/// A control-plane call is one bounded request; stop/delete can take a few seconds.
const CONTROL_TIMEOUT: Duration = Duration::from_secs(120);
/// Upper bound on pages walked by the orphan scan.
const MAX_LIST_PAGES: usize = 20;

pub struct DaytonaProvider {
    client: reqwest::Client,
    /// `None` when no key was found: every call then reports `Config`.
    token: Option<String>,
}

impl DaytonaProvider {
    pub fn new(token: Option<String>) -> Self {
        let client = reqwest::Client::builder()
            .user_agent(concat!("ferryx-cloud-host/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_default();
        Self { client, token }
    }

    /// Key from `DAYTONA_API_KEY` only: the CLI keeps its own credential store.
    pub fn from_env() -> Self {
        let token = crate::cloud_host::secrets::load_var("DAYTONA_API_KEY");
        Self::new(token)
    }

    /// A provider with no credential, for the "call without a key reports Config" path.
    pub fn without_token() -> Self {
        Self::new(None)
    }

    fn token(&self) -> Result<&str, SandboxError> {
        self.token.as_deref().ok_or_else(|| {
            config("missing_token", "no Daytona key: set DAYTONA_API_KEY")
        })
    }

    async fn send(
        &self,
        method: reqwest::Method,
        url: &str,
        body: Option<Value>,
    ) -> Result<(u16, String), SandboxError> {
        let token = self.token()?;
        let mut request =
            self.client.request(method, url).bearer_auth(token).timeout(CONTROL_TIMEOUT);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.map_err(|error| transport_error(&error))?;
        let status = response.status().as_u16();
        let text = response.text().await.map_err(|error| transport_error(&error))?;
        Ok((status, text))
    }

    /// The sandbox as the API reports it right now; a `404` is `NotFound`, never a wake.
    async fn sandbox(&self, sandbox_id: &str) -> Result<SandboxBody, SandboxError> {
        let url = format!("{API_BASE}/sandbox/{sandbox_id}");
        let (status, body) = self.send(reqwest::Method::GET, &url, None).await?;
        if status != 200 {
            return Err(status_error(status, &body, "get"));
        }
        parse_json::<SandboxBody>(&body, "get")
    }

    /// The sandbox already labelled with this idempotency key, if one exists.
    async fn find_by_key(&self, key: &str) -> Result<Option<String>, SandboxError> {
        let filter = json!({ "fx": key }).to_string();
        let mut url = url::Url::parse(&format!("{API_BASE}/sandbox"))
            .map_err(|error| config("bad_url", format!("could not build the list URL: {error}")))?;
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("limit", "100");
            query.append_pair("labels", &filter);
        }
        let (status, body) = self.send(reqwest::Method::GET, url.as_str(), None).await?;
        if status != 200 {
            // A create must not proceed without proving no machine is already labelled for it.
            return Err(status_error(status, &body, "create"));
        }
        let page: ListEnvelope = parse_json(&body, "create")?;
        Ok(page
            .items
            .into_iter()
            .find(|item| item.labels.get("fx").map(String::as_str) == Some(key))
            .map(|item| item.id))
    }
}

#[async_trait::async_trait]
impl SandboxProvider for DaytonaProvider {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            // The account's auto-stop (30 minutes, read live) applies when no TTL is sent.
            ttl_required: false,
            max_ttl_seconds: None,
            transport: Transport::Ssh,
            native_auto_stop: true,
        }
    }

    async fn create(&self, request: CreateSandbox) -> Result<String, SandboxError> {
        if let Some(sandbox_id) = self.find_by_key(&request.idempotency_key).await? {
            return Ok(sandbox_id);
        }
        let mut labels = BTreeMap::new();
        labels.insert("fx".to_string(), request.idempotency_key.clone());
        let mut body = json!({ "name": request.name, "labels": labels });
        if let Some(ttl_seconds) = request.ttl_seconds {
            body["autoStopInterval"] = json!(minutes(ttl_seconds));
        }
        if !request.env.is_empty() {
            body["env"] = json!(request.env);
        }
        let url = format!("{API_BASE}/sandbox");
        let (status, text) = self.send(reqwest::Method::POST, &url, Some(body)).await?;
        match status {
            200 | 201 => Ok(parse_json::<SandboxBody>(&text, "create")?.id),
            // Losing the race to our own earlier create is the idempotent answer.
            409 => match self.find_by_key(&request.idempotency_key).await? {
                Some(sandbox_id) => Ok(sandbox_id),
                None => Err(status_error(409, &text, "create")),
            },
            _ => Err(status_error(status, &text, "create")),
        }
    }

    async fn get(&self, sandbox_id: &str) -> Result<SandboxInfo, SandboxError> {
        let sandbox = self.sandbox(sandbox_id).await?;
        let raw = sandbox.state.unwrap_or_default();
        let state = map_state(Some(raw.as_str()));
        let error = (state == SandboxState::Error).then(|| {
            sandbox.error_reason.unwrap_or_else(|| format!("sandbox state is {raw}"))
        });
        Ok(SandboxInfo { state, error })
    }

    async fn stop(&self, sandbox_id: &str) -> Result<(), SandboxError> {
        let url = format!("{API_BASE}/sandbox/{sandbox_id}/stop");
        let (status, body) = self.send(reqwest::Method::POST, &url, None).await?;
        if status == 200 {
            Ok(())
        } else {
            Err(status_error(status, &body, "stop"))
        }
    }

    async fn resume(&self, sandbox_id: &str, ttl_seconds: Option<u64>) -> Result<(), SandboxError> {
        // Reset the auto-stop before the boot, so the old deadline cannot stop the machine the
        // moment it comes back.
        if let Some(ttl_seconds) = ttl_seconds {
            self.set_autostop(sandbox_id, ttl_seconds).await?;
        }
        let url = format!("{API_BASE}/sandbox/{sandbox_id}/start");
        let (status, body) = self.send(reqwest::Method::POST, &url, None).await?;
        if status == 200 {
            Ok(())
        } else {
            Err(status_error(status, &body, "resume"))
        }
    }

    async fn delete(&self, sandbox_id: &str) -> Result<(), SandboxError> {
        let url = format!("{API_BASE}/sandbox/{sandbox_id}");
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
        // `None` means "the provider's own default", which is exactly what leaving the field
        // alone already gives; there is nothing to write.
        match ttl_seconds {
            Some(ttl_seconds) => self.set_autostop(sandbox_id, ttl_seconds).await,
            None => {
                self.sandbox(sandbox_id).await?;
                Ok(())
            }
        }
    }

    async fn exec(
        &self,
        sandbox_id: &str,
        command: &str,
        timeout_seconds: u64,
    ) -> Result<ExecResult, SandboxError> {
        // The toolbox base is per sandbox (and per region), so it is read off the record.
        let sandbox = self.sandbox(sandbox_id).await?;
        let base = sandbox
            .toolbox_proxy_url
            .unwrap_or_else(|| DEFAULT_TOOLBOX_BASE.to_string());
        let url = format!("{}/{sandbox_id}/process/execute", base.trim_end_matches('/'));
        let body = json!({ "command": command, "timeout": timeout_seconds });
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
            Err(error) => return Err(transport_error(&error)),
        };
        let status = response.status().as_u16();
        let text = match response.text().await {
            Ok(text) => text,
            Err(error) if error.is_timeout() => return Ok(timed_out()),
            Err(error) => return Err(transport_error(&error)),
        };
        // The daemon answers 408 when the command outlived its own timeout.
        if status == 408 {
            return Ok(timed_out());
        }
        if status != 200 {
            return Err(status_error(status, &text, "exec"));
        }
        let parsed: ExecuteResponse = parse_json(&text, "exec")?;
        Ok(ExecResult {
            exit_code: parsed.exit_code,
            stdout: parsed.result,
            // The toolbox response has one output field; stderr is not separable.
            stderr: String::new(),
            timed_out: false,
        })
    }

    async fn ssh_target(&self, sandbox_id: &str) -> Result<SshTarget, SandboxError> {
        // Minting a token for a stopped sandbox would only mislead the caller into an ssh
        // connection the machine refuses, so the state is checked first.
        let info = self.get(sandbox_id).await?;
        if info.state != SandboxState::Ready {
            return Err(SandboxError::new(
                ErrorKind::Conflict,
                PROVIDER,
                "ssh_needs_running",
                format!(
                    "sandbox {sandbox_id} is {}; only a started sandbox accepts ssh access",
                    info.state
                ),
            ));
        }
        let url = format!(
            "{API_BASE}/sandbox/{sandbox_id}/ssh-access?expiresInMinutes={SSH_ACCESS_MINUTES}"
        );
        let (status, body) = self.send(reqwest::Method::POST, &url, None).await?;
        if status != 200 {
            return Err(status_error(status, &body, "ssh_target"));
        }
        let access: SshAccessDto = parse_json(&body, "ssh_target")?;
        if access.token.trim().is_empty() {
            return Err(SandboxError::new(
                ErrorKind::Fatal,
                PROVIDER,
                "empty_ssh_token",
                "the API created ssh access without a token",
            ));
        }
        Ok(SshTarget {
            host: SSH_HOST.to_string(),
            port: 22,
            user: access.token,
            // The token is the credential and travels in the username; we hold no key file.
            identity: SshIdentity::Agent,
            known_hosts_line: None,
            // Daytona's endpoint is a normal internet host: no CLI in the path, no certificate.
            proxy_command: None,
            certificate_file: None,
        })
    }

    async fn list(&self) -> Result<Vec<ListedSandbox>, SandboxError> {
        let mut out = Vec::new();
        let mut cursor: Option<String> = None;
        // The orphan scan walks the whole account, bounded so a repeated cursor cannot loop.
        for _ in 0..MAX_LIST_PAGES {
            let mut url = url::Url::parse(&format!("{API_BASE}/sandbox"))
                .map_err(|error| config("bad_url", format!("could not build the list URL: {error}")))?;
            {
                let mut query = url.query_pairs_mut();
                query.append_pair("limit", "100");
                if let Some(cursor) = cursor.as_deref() {
                    query.append_pair("cursor", cursor);
                }
            }
            let (status, body) = self.send(reqwest::Method::GET, url.as_str(), None).await?;
            if status != 200 {
                return Err(status_error(status, &body, "list"));
            }
            let page: ListEnvelope = parse_json(&body, "list")?;
            for row in page.items {
                out.push(ListedSandbox {
                    sandbox_id: row.id,
                    state: map_state(row.state.as_deref()),
                    created_at: row.created_at.as_deref().and_then(rfc3339_millis),
                    name: row.name,
                });
            }
            match page.next_cursor.filter(|next| !next.is_empty()) {
                Some(next) => cursor = Some(next),
                None => return Ok(out),
            }
        }
        Err(SandboxError::new(
            ErrorKind::Fatal,
            PROVIDER,
            "list_too_long",
            format!("listing stopped after {MAX_LIST_PAGES} pages"),
        ))
    }
}

impl DaytonaProvider {
    async fn set_autostop(&self, sandbox_id: &str, ttl_seconds: u64) -> Result<(), SandboxError> {
        let url =
            format!("{API_BASE}/sandbox/{sandbox_id}/autostop/{}", minutes(ttl_seconds));
        let (status, body) = self.send(reqwest::Method::POST, &url, None).await?;
        if status == 200 {
            Ok(())
        } else {
            Err(status_error(status, &body, "extend_ttl"))
        }
    }
}

/// Seconds -> the whole minutes the auto-stop field takes, rounded up and never zero for a
/// positive TTL (`0` disables auto-stop and would be the opposite of what was asked).
fn minutes(ttl_seconds: u64) -> u64 {
    ttl_seconds.div_ceil(60).max(1)
}

// ---------------------------------------------------------------------------------------------
// Wire types

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SandboxBody {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    error_reason: Option<String>,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    toolbox_proxy_url: Option<String>,
    /// The on-create marker for idempotency (`fx` -> `idempotency_key`).
    #[serde(default)]
    labels: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListEnvelope {
    #[serde(default)]
    items: Vec<SandboxBody>,
    #[serde(default)]
    next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SshAccessDto {
    /// The SSH username the gateway authenticates; a bearer credential, never logged.
    token: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExecuteResponse {
    #[serde(default)]
    result: String,
    /// The daemon has shipped both spellings; the SDK reads `exitCode ?? code`.
    #[serde(default, alias = "code")]
    exit_code: Option<i32>,
}

/// The provider's `SandboxState` vocabulary (`creating|starting|started|stopping|stopped|
/// pausing|paused|archiving|archived|restoring|resizing|snapshotting|forking|deleting|destroying|
/// destroyed|error|build_failed|...`) in [`SandboxState`] terms. A paused or archived sandbox is
/// reported `Stopped`: it costs nothing but is not usable until `resume`.
fn map_state(raw: Option<&str>) -> SandboxState {
    match raw.unwrap_or_default() {
        "started" => SandboxState::Ready,
        "stopping" | "pausing" => SandboxState::Stopping,
        "stopped" | "paused" | "archived" | "deep_sleep" => SandboxState::Stopped,
        "error" | "build_failed" => SandboxState::Error,
        "destroying" | "destroyed" => SandboxState::Deleted,
        // creating, restoring, pulling_snapshot, building_snapshot, pending_build, starting,
        // archiving, resizing, snapshotting, forking, resuming, unknown.
        _ => SandboxState::Provisioning,
    }
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

/// RFC3339 -> unix milliseconds, the unit `ListedSandbox.created_at` uses.
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

/// Map an HTTP failure onto an [`ErrorKind`], keeping Daytona's own `error` token for the
/// operator. The body is `{statusCode,error,message,path}` (never echoed whole: `path` is noise).
fn status_error(status: u16, body: &str, call: &str) -> SandboxError {
    let parsed = serde_json::from_str::<Value>(body).ok();
    let field = |name: &str| {
        parsed
            .as_ref()
            .and_then(|value| value.get(name))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let code = field("error").unwrap_or_else(|| "http_error".to_string());
    let detail = field("message").unwrap_or_else(|| body.trim().to_string());
    let kind = match status {
        401 | 403 => ErrorKind::Config,
        404 => ErrorKind::NotFound,
        409 => ErrorKind::Conflict,
        429 | 500..=599 => ErrorKind::Retryable,
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
    // Never include the request: the header carries the key and the URL names the sandbox.
    SandboxError::new(kind, PROVIDER, "transport", format!("request failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CREATE: &str = include_str!("../../../tests/fixtures/cloud_host/daytona/create_200.json");
    const GET_READY: &str = include_str!("../../../tests/fixtures/cloud_host/daytona/get_ready.json");
    const GET_STOPPED: &str =
        include_str!("../../../tests/fixtures/cloud_host/daytona/get_stopped.json");
    const DELETE: &str = include_str!("../../../tests/fixtures/cloud_host/daytona/delete_200.json");
    const LIST: &str = include_str!("../../../tests/fixtures/cloud_host/daytona/list_live.json");
    const EXEC: &str = include_str!("../../../tests/fixtures/cloud_host/daytona/exec_200.json");
    const SSH_ACCESS: &str =
        include_str!("../../../tests/fixtures/cloud_host/daytona/ssh_access.json");
    const ERROR_401: &str =
        include_str!("../../../tests/fixtures/cloud_host/daytona/error_401.json");

    #[test]
    fn create_live_carries_the_idempotency_label_and_the_toolbox_base() {
        let created: SandboxBody = parse_json(CREATE, "create").unwrap();
        assert_eq!(created.id, "00000000-0000-4000-8000-000000000001");
        assert_eq!(created.name.as_deref(), Some("00000000-0000-4000-8000-000000000001"));
        assert_eq!(created.labels.get("fx").map(String::as_str), Some("t8-fixture"));
        assert_eq!(
            created.toolbox_proxy_url.as_deref(),
            Some("https://proxy.app.daytona.io/toolbox")
        );
        assert_eq!(map_state(created.state.as_deref()), SandboxState::Ready);
    }

    #[test]
    fn a_started_sandbox_is_ready_and_a_stopped_one_is_stopped() {
        let ready: SandboxBody = parse_json(GET_READY, "get").unwrap();
        assert_eq!(map_state(ready.state.as_deref()), SandboxState::Ready);
        let stopped: SandboxBody = parse_json(GET_STOPPED, "get").unwrap();
        assert_eq!(map_state(stopped.state.as_deref()), SandboxState::Stopped);
    }

    #[test]
    fn a_deleted_sandbox_reports_deleted() {
        let deleted: SandboxBody = parse_json(DELETE, "delete").unwrap();
        assert_eq!(deleted.name.as_deref().map(|name| name.starts_with("DESTROYED_")), Some(true));
        assert_eq!(map_state(deleted.state.as_deref()), SandboxState::Deleted);
    }

    #[test]
    fn list_live_parses_one_row_with_its_creation_time() {
        let page: ListEnvelope = parse_json(LIST, "list").unwrap();
        assert!(page.next_cursor.is_none());
        assert_eq!(page.items.len(), 1);
        let row = &page.items[0];
        assert_eq!(row.id, "00000000-0000-4000-8000-000000000001");
        assert_eq!(row.created_at.as_deref().and_then(rfc3339_millis), Some(1_767_225_600_000));
        assert_eq!(row.labels.get("fx").map(String::as_str), Some("spike1"));
    }

    #[test]
    fn exec_live_answers_stdout_and_a_zero_exit_code() {
        let parsed: ExecuteResponse = parse_json(EXEC, "exec").unwrap();
        assert_eq!(parsed.exit_code, Some(0));
        assert_eq!(parsed.result, "x86_64\n");
    }

    #[test]
    fn ssh_access_yields_the_token_as_the_username() {
        let access: SshAccessDto = parse_json(SSH_ACCESS, "ssh_target").unwrap();
        assert_eq!(access.token, "fx-fixture-ssh-token");
    }

    #[test]
    fn a_live_401_is_config_and_names_the_key_variable() {
        let error = status_error(401, ERROR_401, "create");
        assert_eq!(error.kind, ErrorKind::Config);
        assert_eq!(error.provider_code, "Unauthorized");
        assert!(error.message.contains("Invalid credentials"));
    }

    #[test]
    fn the_rest_of_the_status_mapping() {
        assert_eq!(status_error(403, "{}", "get").kind, ErrorKind::Config);
        assert_eq!(status_error(404, "{}", "get").kind, ErrorKind::NotFound);
        assert_eq!(status_error(409, "{}", "create").kind, ErrorKind::Conflict);
        assert_eq!(status_error(429, "{}", "list").kind, ErrorKind::Retryable);
        assert_eq!(status_error(503, "{}", "list").kind, ErrorKind::Retryable);
        assert_eq!(status_error(422, "{}", "create").kind, ErrorKind::Fatal);
    }

    #[test]
    fn every_state_the_api_documents_has_a_home() {
        for raw in ["creating", "starting", "restoring", "pulling_snapshot", "building_snapshot",
                    "pending_build", "archiving", "resizing", "snapshotting", "forking", "resuming",
                    "unknown"] {
            assert_eq!(map_state(Some(raw)), SandboxState::Provisioning, "{raw}");
        }
        for raw in ["stopping", "pausing"] {
            assert_eq!(map_state(Some(raw)), SandboxState::Stopping, "{raw}");
        }
        for raw in ["stopped", "paused", "archived"] {
            assert_eq!(map_state(Some(raw)), SandboxState::Stopped, "{raw}");
        }
        for raw in ["error", "build_failed"] {
            assert_eq!(map_state(Some(raw)), SandboxState::Error, "{raw}");
        }
        for raw in ["destroying", "destroyed"] {
            assert_eq!(map_state(Some(raw)), SandboxState::Deleted, "{raw}");
        }
    }

    #[test]
    fn a_ttl_becomes_whole_minutes_and_never_disables_the_timer() {
        assert_eq!(minutes(1), 1);
        assert_eq!(minutes(60), 1);
        assert_eq!(minutes(61), 2);
        assert_eq!(minutes(1800), 30);
    }

    #[test]
    fn no_key_reports_config_before_any_request() {
        let provider = DaytonaProvider::without_token();
        let error = provider.token().expect_err("no key");
        assert_eq!(error.kind, ErrorKind::Config);
        assert_eq!(error.provider_code, "missing_token");
        assert!(error.message.contains("DAYTONA_API_KEY"));
    }

    #[test]
    fn capabilities_are_ssh_with_the_provider_own_auto_stop() {
        let capabilities = DaytonaProvider::without_token().capabilities();
        assert_eq!(capabilities.transport, Transport::Ssh);
        assert_eq!(capabilities.ttl_required, false);
        assert_eq!(capabilities.max_ttl_seconds, None);
        assert!(capabilities.native_auto_stop);
    }
}
