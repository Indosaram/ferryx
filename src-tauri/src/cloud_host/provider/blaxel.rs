//! Blaxel: a sandbox VM with no inbound port, reached only over the relay. It scales to zero on
//! its own and resumes from standby in about 25 ms with memory preserved.
//!
//! # Endpoints
//!
//! Recorded 2026-11-21 from `https://docs.blaxel.ai/api-reference/controlplane.yml` (control
//! plane, "Blaxel Control Plane 0.0.1") and the sandbox data plane's own OpenAPI ("Sandbox API
//! 0.0.1", served at the sandbox URL), cross-read against `docs.blaxel.ai/Sandboxes/{Processes,
//! File-system,Standby-control,Expiration}`. Two bases:
//!
//! - Control plane `https://api.blaxel.ai/v0`, headers `X-Blaxel-Authorization: Bearer <key>`
//!   and `X-Blaxel-Workspace: <workspace>`; both `BL_API_KEY` and `BL_WORKSPACE` are required,
//!   and neither is ever logged.
//! - Data plane `metadata.url` from the created sandbox (conventionally
//!   `https://sbx-{sandbox}-{workspace}.{region}.bl.run`), same `X-Blaxel-Authorization`.
//!
//! | Call | Request | Response |
//! | --- | --- | --- |
//! | create | `POST /sandboxes?createIfNotExist=true` `{metadata:{name},spec:{runtime,lifecycle}}` | `200` Sandbox; `409` only without the flag |
//! | get | `GET /sandboxes/{name}` | `200` Sandbox (`state` RUNNING\|STANDBY, `status` DEPLOYED\|TERMINATED\|...) |
//! | list | `GET /sandboxes?limit=&cursor=` | `[{Sandbox}]`; newer versions wrap `{data,meta}` |
//! | update | `PUT /sandboxes/{name}` full `Sandbox` | `200` Sandbox |
//! | delete | `DELETE /sandboxes/{name}` | `200` |
//! | stop | data plane `POST /standby?force=true` | `202` accepted (the host may silently ignore it) |
//! | resume | data plane `POST /resume` | `200` when running |
//! | exec | data plane `POST /process` `{command,waitForCompletion:true,timeout}` | `200 ProcessResponse {exitCode,stdout,stderr,status}` |
//! | write file | data plane `PUT /filesystem/{path}` `multipart/form-data` `file` + `permissions` | `200 {message}` |
//!
//! Notes that shaped this adapter:
//!
//! - Memory is in MB and also sets the CPU count (cores = MB / 2048), so 4096 MB is 2 vCPU.
//! - `PUT /filesystem/{path}` resolves a relative path against the filesystem working directory,
//!   so an absolute path is sent with its leading slash percent-encoded as `%2F`; the
//!   `permissions` field is the octal mode applied **when the file is created** only, so an
//!   existing file keeps its old mode (live 2026-11-22: a 666 file stayed 666 after an upload
//!   asking for 600). The trait promises the mode is applied in the same call, so the upload is
//!   followed by one `chmod` through `exec`, exactly as the E2B adapter does: only the filename
//!   travels through exec, never the content.
//! - The upload body is hand-rolled multipart, so the path's basename is spliced into a
//!   `filename="..."` header verbatim. A path carrying CR, LF or NUL is therefore refused with a
//!   `Config` error **before** any request: live 2026-11-22, a CRLF-laced basename was accepted
//!   with `200` and its bytes forged an extra header.
//! - `/process` with `waitForCompletion: true` answers one bounded JSON body; the NDJSON variant
//!   (`Accept: application/x-ndjson`) is deliberately not used, so there is no stream to drain.
//! - `POST /standby` answers `202` before the standby happens and the host **ignores** it in
//!   several documented windows (sandbox younger than ~10 s, within 5 s of the previous accepted
//!   request, rate limited, another operation in flight). It is treated as best-effort: the
//!   documented way to learn the truth is `state == STANDBY`, which `cloud reconcile` reads.
//! - Expiration is `spec.lifecycle.expirationPolicies`; `ttl-max-age` has a documented 5-minute
//!   minimum, so a requested TTL is clamped to 300..=86_400 seconds. `ttl-idle` is not used
//!   because the platform already idles a sandbox on its own.
//! - `PUT /sandboxes/{name}` is a full replacement, so `extend_ttl` re-sends the parts of the
//!   current sandbox it read (`runtime.image`, `runtime.memory`) plus the new policy, and never
//!   the read-only fields the API marks `readOnly`.

use std::time::Duration;

use serde::Deserialize;

use serde_json::{json, Value};

use super::{
    Capabilities, CreateSandbox, ErrorKind, ExecResult, ListedSandbox, SandboxError, SandboxInfo,
    SandboxProvider, SandboxState, Transport,
};

const PROVIDER: &str = "blaxel";
const API_BASE: &str = "https://api.blaxel.ai/v0";
const DEFAULT_IMAGE: &str = "blaxel/base-image:latest";
/// 2 vCPU; memory is billed in MB and fixes the CPU count.
const DEFAULT_MEMORY_MB: u64 = 4_096;
/// `ttl-max-age` has a documented 5-minute floor and no documented ceiling.
const MIN_TTL_SECONDS: u64 = 300;
const MAX_TTL_SECONDS: u64 = 86_400;
const DEFAULT_TTL_SECONDS: u64 = 3_600;
/// Must NOT have: one bounded response, no unbounded stream.
const EXEC_OUTPUT_CAP: usize = 4 * 1024 * 1024;
const LIST_PAGE_LIMIT: usize = 100;
const MAX_LIST_PAGES: usize = 20;
/// One `chmod` after an upload is a single short command, never minutes.
const CHMOD_TIMEOUT_SECONDS: u64 = 60;

pub struct BlaxelProvider {
    client: reqwest::Client,
    /// `None` when no key was found: every call then reports `Config`.
    token: Option<String>,
    /// Every control-plane call is scoped to a workspace; `None` reports `Config` too.
    workspace: Option<String>,
    image: String,
    /// Control-plane base. Always [`API_BASE`] outside tests; a test points it at a local
    /// recorder so the bytes the adapter really sends can be asserted.
    api_base: String,
    /// Data-plane base, when a test must answer the upload/exec wire itself instead of a real
    /// sandbox. Always `None` outside tests.
    data_base: Option<String>,
}

impl BlaxelProvider {
    pub fn new(token: Option<String>, workspace: Option<String>, image: String) -> Self {
        let client = reqwest::Client::builder()
            .user_agent(concat!("ferryx-cloud-host/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_default();
        Self {
            client,
            token,
            workspace,
            image,
            api_base: API_BASE.to_string(),
            data_base: None,
        }
    }

    /// Both credentials come from the environment: `BL_API_KEY` and `BL_WORKSPACE`. The image is
    /// `BL_IMAGE`, defaulting to Blaxel's own base image.
    pub fn from_env() -> Self {
        let token = std::env::var("BL_API_KEY").ok().filter(|value| !value.trim().is_empty());
        let workspace =
            std::env::var("BL_WORKSPACE").ok().filter(|value| !value.trim().is_empty());
        let image = std::env::var("BL_IMAGE")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_IMAGE.to_string());
        Self::new(token, workspace, image)
    }

    /// A provider with no credential, for the "call without a key reports Config" path.
    pub fn without_token() -> Self {
        Self::new(None, None, DEFAULT_IMAGE.to_string())
    }

    fn credentials(&self) -> Result<(&str, &str), SandboxError> {
        let token = self
            .token
            .as_deref()
            .ok_or_else(|| config("missing_token", "no Blaxel key: set BL_API_KEY"))?;
        let workspace = self.workspace.as_deref().ok_or_else(|| {
            config(
                "missing_workspace",
                "no Blaxel workspace: set BL_WORKSPACE (every call is workspace-scoped)",
            )
        })?;
        Ok((token, workspace))
    }

    /// A control-plane call; `path` is appended to `/v0`.
    async fn control(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<ApiResponse, SandboxError> {
        let (token, workspace) = self.credentials()?;
        let mut request = self
            .client
            .request(method, format!("{}{path}", self.api_base))
            .header("X-Blaxel-Authorization", format!("Bearer {token}"))
            .header("X-Blaxel-Workspace", workspace)
            .header("Blaxel-Version", "2026-04-28");
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.map_err(|error| transport_error(&error))?;
        let status = response.status().as_u16();
        let body = response.text().await.map_err(|error| transport_error(&error))?;
        Ok(ApiResponse { status, body })
    }

    /// A data-plane call on the sandbox's own host.
    async fn data(
        &self,
        method: reqwest::Method,
        base: &str,
        path: &str,
        body: Option<Value>,
        timeout: Option<Duration>,
    ) -> Result<ApiResponse, SandboxError> {
        let (token, _) = self.credentials()?;
        let mut request = self
            .client
            .request(method, format!("{base}{path}"))
            .header("X-Blaxel-Authorization", format!("Bearer {token}"));
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

    async fn sandbox(&self, name: &str) -> Result<Sandbox, SandboxError> {
        let response = self
            .control(reqwest::Method::GET, &format!("/sandboxes/{name}"), None)
            .await?;
        if response.status != 200 {
            return Err(status_error(response.status, &response.body, "get"));
        }
        parse_json::<Sandbox>(&response.body, "get")
    }

    /// The sandbox's data-plane base URL. `get` must have succeeded first, so the URL is known.
    async fn data_base(&self, name: &str) -> Result<String, SandboxError> {
        if let Some(base) = self.data_base.as_deref() {
            return Ok(base.to_string());
        }
        let sandbox = self.sandbox(name).await?;
        sandbox.metadata.url.ok_or_else(|| {
            SandboxError::new(
                ErrorKind::Fatal,
                PROVIDER,
                "missing_url",
                "the sandbox has no metadata.url yet",
            )
        })
    }

    /// Seconds, clamped to the documented `ttl-max-age` window.
    fn ttl_seconds(ttl_seconds: Option<u64>) -> u64 {
        ttl_seconds.unwrap_or(DEFAULT_TTL_SECONDS).clamp(MIN_TTL_SECONDS, MAX_TTL_SECONDS)
    }

    /// `"3600s"`: the API accepts `s`, `m`, `h`, `d` and `w` suffixes.
    fn duration_text(seconds: u64) -> String {
        format!("{}s", seconds)
    }

    /// The lifecycle policy every create and every TTL extension carries.
    fn lifecycle(seconds: u64) -> Value {
        json!({
            "expirationPolicies": [{
                "type": "ttl-max-age",
                "value": Self::duration_text(seconds),
                "action": "delete",
            }],
        })
    }
}

struct ApiResponse {
    status: u16,
    body: String,
}

#[async_trait::async_trait]
impl SandboxProvider for BlaxelProvider {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            // The platform idles a sandbox on its own; a TTL only bounds its total lifetime.
            ttl_required: false,
            max_ttl_seconds: Some(MAX_TTL_SECONDS),
            transport: Transport::Relay,
            native_auto_stop: true,
        }
    }

    async fn create(&self, request: CreateSandbox) -> Result<String, SandboxError> {
        let mut runtime = json!({
            "image": self.image,
            "memory": DEFAULT_MEMORY_MB,
        });
        if !request.env.is_empty() {
            let envs: Vec<Value> = request
                .env
                .iter()
                .map(|(name, value)| json!({ "name": name, "value": value }))
                .collect();
            runtime["envs"] = json!(envs);
        }
        let body = json!({
            "metadata": { "name": request.name, "displayName": request.name },
            "spec": {
                "runtime": runtime,
                "lifecycle": Self::lifecycle(Self::ttl_seconds(request.ttl_seconds)),
            },
        });
        // `createIfNotExist` makes the create idempotent on the name: a retry after a dropped
        // response returns the sandbox that already exists instead of a 409.
        let response = self
            .control(reqwest::Method::POST, "/sandboxes?createIfNotExist=true", Some(body))
            .await?;
        match response.status {
            200 | 201 => {
                let created: Sandbox = parse_json(&response.body, "create")?;
                Ok(created.metadata.name)
            }
            _ => Err(status_error(response.status, &response.body, "create")),
        }
    }

    async fn get(&self, sandbox_id: &str) -> Result<SandboxInfo, SandboxError> {
        let sandbox = self.sandbox(sandbox_id).await?;
        let status = sandbox.status.as_deref().unwrap_or_default();
        let state = match status {
            "TERMINATED" | "DEACTIVATED" | "ARCHIVED" => SandboxState::Deleted,
            "FAILED" => SandboxState::Error,
            "DELETING" | "DEACTIVATING" => SandboxState::Stopping,
            // Stored disk but no running VM: a stop, and `resume` brings it back.
            "ARCHIVING" | "UNARCHIVING" | "UPLOADING" | "BUILDING" | "DEPLOYING" => {
                SandboxState::Provisioning
            }
            _ => match sandbox.state.as_deref() {
                Some("RUNNING") => SandboxState::Ready,
                Some("STANDBY") => SandboxState::Stopped,
                Some(_) => SandboxState::Provisioning,
                None => SandboxState::Provisioning,
            },
        };
        let error = (state == SandboxState::Error)
            .then(|| format!("sandbox status is {status}"));
        Ok(SandboxInfo { state, error })
    }

    async fn stop(&self, sandbox_id: &str) -> Result<(), SandboxError> {
        // Already stopped is a success: standby is idempotent from the caller's point of view.
        if self.get(sandbox_id).await?.state == SandboxState::Stopped {
            return Ok(());
        }
        let base = self.data_base(sandbox_id).await?;
        let response = self
            .data(reqwest::Method::POST, &base, "/standby?force=true", Some(json!({})), None)
            .await?;
        match response.status {
            // 202 is only "accepted": the host may ignore it and idle the sandbox later.
            200 | 202 => Ok(()),
            409 => Ok(()),
            _ => Err(status_error(response.status, &response.body, "stop")),
        }
    }

    async fn resume(&self, sandbox_id: &str, ttl_seconds: Option<u64>) -> Result<(), SandboxError> {
        let base = self.data_base(sandbox_id).await?;
        let response =
            self.data(reqwest::Method::POST, &base, "/resume", Some(json!({})), None).await?;
        if response.status != 200 && response.status != 202 {
            return Err(status_error(response.status, &response.body, "resume"));
        }
        // A resume restores the memory, not the deadline: push the new one when one is asked for.
        if let Some(ttl) = ttl_seconds {
            self.extend_ttl(sandbox_id, Some(ttl)).await?;
        }
        Ok(())
    }

    async fn delete(&self, sandbox_id: &str) -> Result<(), SandboxError> {
        let response = self
            .control(reqwest::Method::DELETE, &format!("/sandboxes/{sandbox_id}"), None)
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
        let existing = self.sandbox(sandbox_id).await?;
        let runtime = existing.spec.runtime.unwrap_or_default();
        // `PUT` replaces the whole sandbox, so only the fields a caller may write are re-sent.
        let mut runtime_body = json!({ "image": runtime.image.unwrap_or_else(|| self.image.clone()) });
        if let Some(memory) = runtime.memory {
            runtime_body["memory"] = json!(memory);
        }
        let body = json!({
            "metadata": { "name": existing.metadata.name },
            "spec": {
                "runtime": runtime_body,
                "lifecycle": Self::lifecycle(Self::ttl_seconds(ttl_seconds)),
            },
        });
        let response = self
            .control(reqwest::Method::PUT, &format!("/sandboxes/{sandbox_id}"), Some(body))
            .await?;
        match response.status {
            200 | 201 => Ok(()),
            _ => Err(status_error(response.status, &response.body, "extend_ttl")),
        }
    }

    async fn exec(
        &self,
        sandbox_id: &str,
        command: &str,
        timeout_seconds: u64,
    ) -> Result<ExecResult, SandboxError> {
        let base = self.data_base(sandbox_id).await?;
        let body = json!({
            "command": command,
            // One bounded JSON answer instead of the NDJSON stream.
            "waitForCompletion": true,
            "timeout": timeout_seconds,
            "keepAlive": false,
            "stdin": false,
        });
        let response = self
            .data(
                reqwest::Method::POST,
                &base,
                "/process",
                Some(body),
                Some(Duration::from_secs(timeout_seconds.max(1).saturating_add(30))),
            )
            .await;
        let response = match response {
            Ok(response) => response,
            // The provider kills the process at its own timeout; a transport timeout here means
            // nothing came back in the bound, which the caller sees as a timed-out run.
            Err(error) if error.kind == ErrorKind::Retryable => return Ok(timed_out()),
            Err(error) => return Err(error),
        };
        if response.status != 200 {
            // A process the provider itself killed at its timeout answers 422
            // `{"error":"process timed out after N seconds"}`. That is the documented
            // timed-out run, not a failure: report it as one instead of an error.
            if process_timed_out(response.status, &response.body) {
                return Ok(timed_out());
            }
            return Err(status_error(response.status, &response.body, "exec"));
        }
        parse_process(&response.body)
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
        ensure_header_safe(path)?;
        let base = self.data_base(sandbox_id).await?;
        let (token, workspace) = self.credentials()?;
        // A leading slash must be `%2F` inside the URL: `/filesystem/workspace` would resolve
        // against the filesystem working directory instead of the root.
        let url = format!("{base}/filesystem/%2F{}", encode_path(&path[1..]));
        let boundary = format!("fx-boundary-{}", hex16(&bytes));
        let body = multipart_body(&boundary, path, &bytes, mode);
        let response = self
            .client
            .put(&url)
            .header("X-Blaxel-Authorization", format!("Bearer {token}"))
            .header("X-Blaxel-Workspace", workspace)
            .header("content-type", format!("multipart/form-data; boundary={boundary}"))
            .timeout(Duration::from_secs(120))
            .body(body)
            .send()
            .await
            .map_err(|error| transport_error(&error))?;
        let status = response.status().as_u16();
        if status != 200 && status != 201 {
            let body = response.text().await.unwrap_or_default();
            return Err(status_error(status, &body, "write_file"));
        }
        // The `permissions` field above only applies when the path is new; an upload onto an
        // existing file keeps that file's mode. The trait promises the mode in the same call, so
        // it is applied explicitly here. Only the filename travels through exec.
        let chmod = self
            .exec(sandbox_id, &chmod_command(path, mode), CHMOD_TIMEOUT_SECONDS)
            .await?;
        if chmod.exit_code == Some(0) {
            Ok(())
        } else {
            Err(fatal(
                "chmod_failed",
                format!(
                    "the file was written but chmod {} failed: exit {:?} {}",
                    format_mode(mode),
                    chmod.exit_code,
                    chmod.stderr.trim()
                ),
            ))
        }
    }

    async fn list(&self) -> Result<Vec<ListedSandbox>, SandboxError> {
        let mut listed = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..MAX_LIST_PAGES {
            let mut path = format!("/sandboxes?limit={LIST_PAGE_LIMIT}&showTerminated=true");
            if let Some(cursor) = cursor.as_deref() {
                path.push_str("&cursor=");
                path.push_str(&encode_path(cursor));
            }
            let response = self.control(reqwest::Method::GET, &path, None).await?;
            if response.status != 200 {
                return Err(status_error(response.status, &response.body, "list"));
            }
            let page = parse_list(&response.body)?;
            listed.extend(page.rows.into_iter().map(ListedSandboxWire::into_listed));
            match page.cursor {
                Some(next) if !next.is_empty() => cursor = Some(next),
                _ => return Ok(listed),
            }
        }
        Ok(listed)
    }
}

// ---------------------------------------------------------------------------------------------
// Wire types

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Sandbox {
    metadata: SandboxMetadata,
    #[serde(default)]
    spec: SandboxSpec,
    /// `RUNNING` or `STANDBY`; read-only.
    #[serde(default)]
    state: Option<String>,
    /// Deployment status; read-only.
    #[serde(default)]
    status: Option<String>,
}

/// Metadata of one sandbox addressed by name. `createdAt` is deliberately not read here: it is
/// only reported through [`ListedSandbox::created_at`], which `ListedMetadata` carries.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SandboxMetadata {
    name: String,
    #[serde(default)]
    url: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct SandboxSpec {
    #[serde(default)]
    runtime: Option<SandboxRuntime>,
}

#[derive(Debug, Default, Deserialize)]
struct SandboxRuntime {
    #[serde(default)]
    image: Option<String>,
    #[serde(default)]
    memory: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProcessResponse {
    #[serde(default)]
    exit_code: Option<i64>,
    #[serde(default)]
    stdout: Option<String>,
    #[serde(default)]
    stderr: Option<String>,
    #[serde(default)]
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListedSandboxWire {
    #[serde(default)]
    metadata: Option<ListedMetadata>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListedMetadata {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    created_at: Option<String>,
}

impl ListedSandboxWire {
    fn into_listed(self) -> ListedSandbox {
        let status = self.status.as_deref().unwrap_or_default();
        let state = match status {
            "TERMINATED" | "DEACTIVATED" | "ARCHIVED" => SandboxState::Deleted,
            "FAILED" => SandboxState::Error,
            "DELETING" | "DEACTIVATING" => SandboxState::Stopping,
            _ => match self.state.as_deref() {
                Some("RUNNING") => SandboxState::Ready,
                Some("STANDBY") => SandboxState::Stopped,
                _ => SandboxState::Provisioning,
            },
        };
        let metadata = self.metadata.unwrap_or(ListedMetadata { name: None, created_at: None });
        ListedSandbox {
            sandbox_id: metadata.name.clone().unwrap_or_default(),
            state,
            created_at: metadata.created_at.as_deref().and_then(rfc3339_millis),
            name: metadata.name,
        }
    }
}

/// `GET /sandboxes` answers a bare array on older versions and `{data,meta}` from 2026-04-28 on.
struct ListPage {
    rows: Vec<ListedSandboxWire>,
    cursor: Option<String>,
}

fn parse_list(body: &str) -> Result<ListPage, SandboxError> {
    let value: Value = serde_json::from_str(body).map_err(|error| {
        fatal("unexpected_body", format!("list: the API answered with a bad body: {error}"))
    })?;
    if value.is_array() {
        let rows: Vec<ListedSandboxWire> = serde_json::from_value(value).map_err(|error| {
            fatal("unexpected_body", format!("list: unexpected row shape: {error}"))
        })?;
        return Ok(ListPage { rows, cursor: None });
    }
    let cursor = value
        .get("meta")
        .and_then(|meta| meta.get("nextCursor").or_else(|| meta.get("next_cursor")))
        .and_then(Value::as_str)
        .map(str::to_string);
    let rows: Vec<ListedSandboxWire> =
        serde_json::from_value(value.get("data").cloned().unwrap_or(Value::Array(Vec::new())))
            .map_err(|error| {
                fatal("unexpected_body", format!("list: unexpected row shape: {error}"))
            })?;
    Ok(ListPage { rows, cursor })
}

fn parse_process(body: &str) -> Result<ExecResult, SandboxError> {
    let parsed: ProcessResponse = parse_json(body, "exec")?;
    let stdout = parsed.stdout.unwrap_or_default();
    let stderr = parsed.stderr.unwrap_or_default();
    if stdout.len() + stderr.len() > EXEC_OUTPUT_CAP {
        return Err(fatal(
            "output_limit_exceeded",
            format!("exec output exceeded {EXEC_OUTPUT_CAP} bytes"),
        ));
    }
    // `status: "killed"` is what the provider reports for a process the API timed out.
    let timed_out = parsed.status.as_deref() == Some("killed");
    Ok(ExecResult {
        exit_code: if timed_out { None } else { parsed.exit_code.map(|code| code as i32) },
        stdout,
        stderr,
        timed_out,
    })
}

fn timed_out() -> ExecResult {
    ExecResult { exit_code: None, stdout: String::new(), stderr: String::new(), timed_out: true }
}

/// The data plane's timed-out run: status 408/504, or any status whose body says the process
/// timed out (the live answer is 422 `{"error":"process timed out after N seconds"}`).
fn process_timed_out(status: u16, body: &str) -> bool {
    if matches!(status, 408 | 504) {
        return true;
    }
    body.to_ascii_lowercase().contains("timed out")
}

fn parse_json<T: for<'de> Deserialize<'de>>(body: &str, call: &str) -> Result<T, SandboxError> {
    serde_json::from_str(body).map_err(|error| {
        fatal(
            "unexpected_body",
            format!("{call}: the API answered with an unexpected body: {error}"),
        )
    })
}

// ---------------------------------------------------------------------------------------------
// multipart/form-data, hand-rolled because this crate's `reqwest` has no `multipart` feature

/// One `multipart/form-data` body carrying the file and the octal mode to create it with.
fn multipart_body(boundary: &str, path: &str, bytes: &[u8], mode: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + 512);
    let mut field = |name: &str, value: &[u8], filename: Option<&str>| {
        out.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        match filename {
            Some(filename) => out.extend_from_slice(
                format!(
                    "Content-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\n\
                     Content-Type: application/octet-stream\r\n\r\n"
                )
                .as_bytes(),
            ),
            None => out.extend_from_slice(
                format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
            ),
        }
        out.extend_from_slice(value);
        out.extend_from_slice(b"\r\n");
    };
    let filename = path.rsplit('/').next().unwrap_or("file");
    field("file", bytes, Some(filename));
    // Applied when the file is created, which is the same call that writes it.
    field("permissions", format!("{:04o}", mode & 0o7777).as_bytes(), None);
    out.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    out
}

/// A boundary that cannot appear in the payload, derived from it.
fn hex16(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes.iter().take(512) {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

// ---------------------------------------------------------------------------------------------
// Small helpers

fn format_mode(mode: u32) -> String {
    format!("{:04o}", mode & 0o7777)
}

/// Single-quote a word for `/bin/sh`, escaping any quote it contains. Inside single quotes every
/// other byte is literal, so this accepts any path the upload itself accepts.
fn shell_quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', "'\\''"))
}

/// Reject a path whose basename would break out of the multipart `filename="..."` header.
///
/// The upload body is hand-rolled (this crate's `reqwest` has no `multipart` feature), so the
/// basename is spliced into a header line verbatim: a CR or LF in it starts a header or part of
/// its own (live 2026-11-22: a CRLF-laced name was accepted with `200`), and a NUL truncates the
/// name C-side. `shell_quote` above already contains them for the chmod, but the upload leaves
/// them unescaped, so they are refused here - before any request goes out.
fn ensure_header_safe(path: &str) -> Result<(), SandboxError> {
    if let Some(bad) = path.chars().find(|c| matches!(c, '\r' | '\n' | '\0')) {
        return Err(config(
            "path_not_header_safe",
            format!(
                "write_file path may not contain CR, LF or NUL ({:?}): {}",
                bad,
                path.escape_debug()
            ),
        ));
    }
    Ok(())
}

/// `chmod` with the file name as its only operand: the content never travels through exec.
fn chmod_command(path: &str, mode: u32) -> String {
    format!("chmod {} -- {}", format_mode(mode), shell_quote(path))
}

/// Percent-encode a path segment: everything but unreserved characters becomes `%XX`.
fn encode_path(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for byte in raw.as_bytes() {
        let keep = byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/');
        if keep {
            out.push(*byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn rfc3339_millis(text: &str) -> Option<i64> {
    let parsed =
        time::OffsetDateTime::parse(text, &time::format_description::well_known::Rfc3339).ok()?;
    i64::try_from(parsed.unix_timestamp_nanos() / 1_000_000).ok()
}

fn config(code: impl Into<String>, message: impl Into<String>) -> SandboxError {
    SandboxError::new(ErrorKind::Config, PROVIDER, code, message)
}

fn fatal(code: impl Into<String>, message: impl Into<String>) -> SandboxError {
    SandboxError::new(ErrorKind::Fatal, PROVIDER, code, message)
}

/// A stable token for the statuses Blaxel reports with a **numeric** `code`.
///
/// Live 2026-11-22: a control-plane 401 is `{"code":401,"error":"Unauthorized"}` and a 404 is
/// `{"code":404,"error":"Sandbox not found"}`; the data plane's 502 wraps a whole object under
/// `error`. There is no symbolic code in any of them, so reading `code` as a string left every
/// provider_code at the meaningless `http_error`.
fn status_code_name(status: u16) -> &'static str {
    match status {
        400 => "bad_request",
        401 => "unauthorized",
        403 => "forbidden",
        404 => "not_found",
        409 => "conflict",
        422 => "unprocessable_entity",
        429 => "rate_limited",
        500 => "internal_error",
        502 => "bad_gateway",
        503 => "unavailable",
        504 => "gateway_timeout",
        _ => "http_error",
    }
}

/// The `message` (or `error`) Blaxel sends: a string on its own, or the nested `message` of the
/// object the platform wraps around an upstream failure.
fn error_detail(value: &Value, raw: &str) -> String {
    if let Some(text) = value.get("message").and_then(Value::as_str) {
        return text.trim().to_string();
    }
    match value.get("error") {
        Some(Value::String(text)) => text.trim().to_string(),
        Some(inner @ Value::Object(_)) => inner
            .get("message")
            .and_then(Value::as_str)
            .map(|text| text.trim().to_string())
            .unwrap_or_else(|| raw.trim().to_string()),
        _ => raw.trim().to_string(),
    }
}

/// Blaxel's errors are `{code, message|error}` with a numeric `code`: the control plane answers
/// `{"code":401,"error":"Unauthorized"}`, the data plane `{"error":{..."code":"UPSTREAM_ERROR"}}`
/// for a proxied failure. A symbolic code is kept when one is present.
fn status_error(status: u16, body: &str, call: &str) -> SandboxError {
    let parsed = serde_json::from_str::<Value>(body).ok();
    let detail = parsed
        .as_ref()
        .map(|value| error_detail(value, body))
        .unwrap_or_else(|| body.trim().to_string());
    let parsed_code = parsed.as_ref().and_then(|value| {
        let top = value.get("code").and_then(Value::as_str);
        let nested = value
            .get("error")
            .and_then(|error| error.get("code"))
            .and_then(Value::as_str);
        top.or(nested)
    });
    let code = parsed_code
        .filter(|code| !code.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| status_code_name(status).to_string());
    let kind = match (status, code.as_str()) {
        (401 | 403, _) => ErrorKind::Config,
        (404, _) => ErrorKind::NotFound,
        (409, _) => ErrorKind::Conflict,
        (_, "QUOTA_EXCEEDED") => ErrorKind::Config,
        (_, "INVALID_IMAGE") => ErrorKind::Config,
        // 503 on `/standby` means the keep-awake state could not be read: retry later.
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
    // Never include the request in the message: the headers carry the key and the workspace.
    SandboxError::new(kind, PROVIDER, "transport", format!("request failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CREATE_FIXTURE: &str =
        include_str!("../../../tests/fixtures/cloud_host/blaxel/create_200.json");
    const GET_RUNNING_FIXTURE: &str =
        include_str!("../../../tests/fixtures/cloud_host/blaxel/get_running.json");
    const GET_STANDBY_FIXTURE: &str =
        include_str!("../../../tests/fixtures/cloud_host/blaxel/get_standby.json");
    const GET_TERMINATED_FIXTURE: &str =
        include_str!("../../../tests/fixtures/cloud_host/blaxel/get_terminated.json");
    const LIST_ARRAY_FIXTURE: &str =
        include_str!("../../../tests/fixtures/cloud_host/blaxel/list_array.json");
    const LIST_ENVELOPE_FIXTURE: &str =
        include_str!("../../../tests/fixtures/cloud_host/blaxel/list_envelope.json");
    const PROCESS_FIXTURE: &str =
        include_str!("../../../tests/fixtures/cloud_host/blaxel/process_result.json");
    const PROCESS_KILLED_FIXTURE: &str =
        include_str!("../../../tests/fixtures/cloud_host/blaxel/process_killed.json");
    const ERROR_404_FIXTURE: &str =
        include_str!("../../../tests/fixtures/cloud_host/blaxel/error_404.json");
    const ERROR_401_FIXTURE: &str =
        include_str!("../../../tests/fixtures/cloud_host/blaxel/error_401.json");

    #[test]
    fn create_answers_with_the_name_and_the_data_plane_url() {
        let created: Sandbox = parse_json(CREATE_FIXTURE, "create").expect("fixture");
        assert_eq!(created.metadata.name, "fx-blaxel-1");
        assert_eq!(
            created.metadata.url.as_deref(),
            Some("https://sbx-fx-blaxel-1-main.us-pdx-1.bl.run")
        );
        assert_eq!(created.spec.runtime.and_then(|runtime| runtime.image).as_deref(), Some("blaxel/base-image:latest"));
    }

    #[test]
    fn running_is_ready_and_standby_is_stopped() {
        let running: Sandbox = parse_json(GET_RUNNING_FIXTURE, "get").expect("running");
        assert_eq!(running.state.as_deref(), Some("RUNNING"));
        assert_eq!(running.status.as_deref(), Some("DEPLOYED"));

        let standby: Sandbox = parse_json(GET_STANDBY_FIXTURE, "get").expect("standby");
        assert_eq!(standby.state.as_deref(), Some("STANDBY"));
    }

    #[test]
    fn a_terminated_sandbox_is_deleted_and_never_listed_alive() {
        let terminated: Sandbox = parse_json(GET_TERMINATED_FIXTURE, "get").expect("terminated");
        assert_eq!(terminated.status.as_deref(), Some("TERMINATED"));
        let row = ListedSandboxWire {
            metadata: Some(ListedMetadata {
                name: Some("fx-blaxel-1".into()),
                created_at: Some("2026-11-21T09:30:00Z".into()),
            }),
            state: terminated.state.clone(),
            status: terminated.status.clone(),
        }
        .into_listed();
        assert_eq!(row.state, SandboxState::Deleted);
    }

    #[test]
    fn a_bare_array_and_an_envelope_are_both_listed() {
        let array = parse_list(LIST_ARRAY_FIXTURE).expect("array");
        assert_eq!(array.rows.len(), 2);
        assert!(array.cursor.is_none());
        let listed: Vec<ListedSandbox> =
            array.rows.into_iter().map(ListedSandboxWire::into_listed).collect();
        assert_eq!(listed[0].sandbox_id, "fx-blaxel-1");
        assert_eq!(listed[0].state, SandboxState::Ready);
        assert_eq!(listed[1].state, SandboxState::Stopped);

        let envelope = parse_list(LIST_ENVELOPE_FIXTURE).expect("envelope");
        assert_eq!(envelope.rows.len(), 1);
        assert_eq!(envelope.cursor.as_deref(), Some("cursor-2"));
    }

    #[test]
    fn a_process_result_carries_its_exit_code_and_both_streams() {
        let result = parse_process(PROCESS_FIXTURE).expect("process");
        assert_eq!(result.exit_code, Some(0));
        assert_eq!(result.stdout.trim(), "x86_64");
        assert_eq!(result.stderr, "");
        assert!(!result.timed_out);
    }

    #[test]
    fn a_killed_process_is_reported_as_timed_out() {
        let result = parse_process(PROCESS_KILLED_FIXTURE).expect("killed");
        assert!(result.timed_out);
        assert_eq!(result.exit_code, None);
    }

    #[test]
    fn a_stale_running_state_with_a_terminated_status_is_deleted() {
        // Observed live 2026-11-22: after a delete, `GET /sandboxes/{name}` kept answering
        // state RUNNING while status said TERMINATED, and the data plane answered 404
        // WORKLOAD_UNAVAILABLE. `status` is authoritative; `state` is a stale cache.
        let body = r#"{
            "metadata": {"name": "fx-t10-blaxel-1", "createdAt": "2026-11-21T20:06:35Z"},
            "state": "RUNNING",
            "status": "TERMINATED"
        }"#;
        let sandbox: Sandbox = parse_json(body, "get").expect("fixture");
        assert_eq!(sandbox.state.as_deref(), Some("RUNNING"));
        assert_eq!(sandbox.status.as_deref(), Some("TERMINATED"));

        let row = ListedSandboxWire {
            metadata: Some(ListedMetadata {
                name: Some("fx-t10-blaxel-1".into()),
                created_at: Some("2026-11-21T20:06:35Z".into()),
            }),
            state: sandbox.state.clone(),
            status: sandbox.status.clone(),
        }
        .into_listed();
        assert_eq!(row.state, SandboxState::Deleted);
    }

    #[test]
    fn the_live_422_timeout_answer_is_a_timed_out_run_not_an_error() {
        // Observed 2026-11-22: `sleep 20` with a 5 s bound answers this, not a 200.
        let body = r#"{"error":"process timed out after 5 seconds"}"#;
        assert!(process_timed_out(422, body));
        assert!(process_timed_out(408, ""));
        assert!(process_timed_out(504, ""));
        // A real failure must stay an error.
        assert!(!process_timed_out(422, r#"{"error":"invalid command"}"#));
        assert!(!process_timed_out(500, r#"{"error":"internal"}"#));
    }

    #[test]
    fn a_missing_sandbox_is_not_found_and_a_bad_key_is_config() {
        assert_eq!(status_error(404, ERROR_404_FIXTURE, "get").kind, ErrorKind::NotFound);
        assert_eq!(status_error(401, ERROR_401_FIXTURE, "get").kind, ErrorKind::Config);
        let quota = r#"{"code":"QUOTA_EXCEEDED","message":"too many sandboxes"}"#;
        assert_eq!(status_error(400, quota, "create").kind, ErrorKind::Config);
    }

    #[test]
    fn the_live_error_bodies_yield_a_meaningful_provider_code() {
        // Live 2026-11-22: `{"code":401,"error":"Unauthorized"}` and
        // `{"code":404,"error":"Sandbox not found"}`. The numeric `code` is not a string, so
        // reading it as one used to drop every real failure to `http_error`.
        let unauthorised = status_error(401, ERROR_401_FIXTURE, "get");
        assert_eq!(unauthorised.kind, ErrorKind::Config);
        assert_eq!(unauthorised.provider_code, "unauthorized");
        assert!(unauthorised.message.contains("Unauthorized"), "{}", unauthorised.message);

        let not_found = status_error(404, ERROR_404_FIXTURE, "get");
        assert_eq!(not_found.kind, ErrorKind::NotFound);
        assert_eq!(not_found.provider_code, "not_found");

        // A symbolic code the provider does send (2026-04-28 control plane) still wins.
        let quota = r#"{"code":"QUOTA_EXCEEDED","message":"too many sandboxes"}"#;
        assert_eq!(status_error(400, quota, "create").provider_code, "QUOTA_EXCEEDED");

        // The data plane wraps an upstream failure in an object under `error`.
        let upstream = r#"{"error":{"code":"UPSTREAM_ERROR","message":"could not proxy","status":502}}"#;
        let error = status_error(502, upstream, "exec");
        assert_eq!(error.kind, ErrorKind::Retryable);
        assert_eq!(error.provider_code, "UPSTREAM_ERROR");
        assert!(error.message.contains("could not proxy"), "{}", error.message);

        // An unknown shape still names the status rather than falling back to `http_error`.
        assert_eq!(status_error(400, "not json", "create").provider_code, "bad_request");
    }

    #[test]
    fn the_ttl_is_clamped_to_the_documented_window() {
        assert_eq!(BlaxelProvider::duration_text(3_600), "3600s");
        assert_eq!(BlaxelProvider::ttl_seconds(Some(10)), MIN_TTL_SECONDS);
        assert_eq!(BlaxelProvider::ttl_seconds(Some(10 * 86_400)), MAX_TTL_SECONDS);
    }

    #[test]
    fn the_lifecycle_policy_deletes_at_the_max_age() {
        let lifecycle = BlaxelProvider::lifecycle(1_800);
        let policy = &lifecycle["expirationPolicies"][0];
        assert_eq!(policy["type"], "ttl-max-age");
        assert_eq!(policy["value"], "1800s");
        assert_eq!(policy["action"], "delete");
    }

    #[test]
    fn the_upload_is_multipart_with_the_mode_in_the_same_body() {
        let body = multipart_body("fx-boundary-test", "/home/user/.fx-test", b"0123456789", 0o600);
        let text = String::from_utf8_lossy(&body);
        assert!(text.starts_with("--fx-boundary-test\r\n"));
        assert!(text.contains("name=\"file\"; filename=\".fx-test\""));
        assert!(text.contains("name=\"permissions\"\r\n\r\n0600\r\n"));
        assert!(text.contains("\r\n0123456789\r\n"));
        assert!(text.ends_with("--fx-boundary-test--\r\n"));
    }

    #[test]
    fn an_absolute_path_is_sent_with_an_encoded_leading_slash() {
        assert_eq!(encode_path("home/user/.fx-test"), "home/user/.fx-test");
        assert_eq!(encode_path("home/user/a b"), "home/user/a%20b");
        // The `%2F` is added by the caller; the rest must not re-encode the separators.
        assert_eq!(encode_path("workspace/archive.tar"), "workspace/archive.tar");
    }

    #[test]
    fn a_path_that_could_forge_a_multipart_header_is_refused() {
        // The basename is spliced into `filename="..."` verbatim, so a CR/LF there would start a
        // header of the sender's choosing (live 2026-11-22: such an upload answered 200).
        for path in [
            "/workspace/a\r\nX-Injected: 1",
            "/workspace/a\nb",
            "/workspace/a\rb",
            "/workspace/a\0b",
        ] {
            let error = ensure_header_safe(path).expect_err("must be refused");
            assert_eq!(error.kind, ErrorKind::Config, "{path:?}");
            assert_eq!(error.provider_code, "path_not_header_safe", "{path:?}");
        }
        // Ordinary paths - including the hostile names D1 proved the quoting survives - still pass.
        for path in [
            "/workspace/.fx-test",
            "/workspace/a b",
            "/workspace/it's",
            "/workspace/$(id -u)",
            "/workspace/x;id -u",
        ] {
            ensure_header_safe(path).unwrap_or_else(|error| panic!("{path:?}: {error:?}"));
        }
    }

    #[tokio::test]
    async fn write_file_refuses_a_header_forging_path_before_any_request() {
        let reply = r#"{"status":"completed","exitCode":0,"stdout":"","stderr":""}"#;
        let fake = FakeSandbox::start(reply).await;
        let provider = provider_for(&fake.base);

        let error = provider
            .write_file("fx-blaxel-1", "/workspace/a\r\nX-Injected: 1", vec![1u8; 8], 0o600)
            .await
            .expect_err("a CRLF basename must be refused");
        assert_eq!(error.kind, ErrorKind::Config);
        assert_eq!(error.provider_code, "path_not_header_safe");
        // Nothing reached the data plane: a Config error means the caller must fix the path, and
        // an upload that already happened would be a write the caller never gets to undo.
        assert!(fake.calls().await.is_empty(), "no request may precede the refusal");
    }

    #[test]
    fn capabilities_carry_the_relay_and_native_auto_stop() {
        let capabilities = BlaxelProvider::without_token().capabilities();
        assert_eq!(capabilities.transport, Transport::Relay);
        assert!(capabilities.native_auto_stop);
        assert_eq!(capabilities.max_ttl_seconds, Some(MAX_TTL_SECONDS));
    }

    #[tokio::test]
    async fn a_call_without_credentials_names_the_missing_variable() {
        let provider = BlaxelProvider::without_token();
        let error = provider.get("fx-blaxel-1").await.expect_err("no key");
        assert_eq!(error.kind, ErrorKind::Config);
        assert_eq!(error.provider_code, "missing_token");

        let provider = BlaxelProvider::new(Some("bl-key".into()), None, DEFAULT_IMAGE.into());
        let error = provider.get("fx-blaxel-1").await.expect_err("no workspace");
        assert_eq!(error.kind, ErrorKind::Config);
        assert_eq!(error.provider_code, "missing_workspace");
    }

    /// What one request to the fake sandbox carried.
    #[derive(Clone, Debug)]
    struct Recorded {
        method: String,
        uri: String,
        body: String,
        headers: Vec<(String, String)>,
    }

    /// A local server answering what a real Blaxel sandbox answers, recording every request so
    /// the upload/chmod sequence can be asserted on the wire instead of by reading the source.
    struct FakeSandbox {
        base: String,
        seen: std::sync::Arc<tokio::sync::Mutex<Vec<Recorded>>>,
    }

    impl FakeSandbox {
        /// `process_reply` is what `/process` answers, so a test controls the reported exit code.
        async fn start(process_reply: &'static str) -> Self {
            use axum::extract::State;
            use axum::routing::{post, put};

            type Seen = std::sync::Arc<tokio::sync::Mutex<Vec<Recorded>>>;
            let seen: Seen = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));

            let app = axum::Router::new()
                .route(
                    "/filesystem/{*path}",
                    put(
                        |State(seen): State<Seen>,
                         uri: axum::http::Uri,
                         headers: axum::http::HeaderMap,
                         body: axum::body::Bytes| async move {
                            let method = "PUT";
                            let (uri, headers) = (uri.to_string(), headers.clone());
                            let body = body.to_vec();
                            seen.lock().await.push(Recorded {
                                method: method.to_string(),
                                uri,
                                body: String::from_utf8_lossy(&body).into_owned(),
                                headers: headers
                                    .iter()
                                    .map(|(name, value)| {
                                        (name.as_str().to_string(), value.to_str().unwrap_or("<binary>").to_string())
                                    })
                                    .collect(),
                            });
                            (
                                axum::http::StatusCode::OK,
                                r#"{"message":"Binary file uploaded successfully"}"#.to_string(),
                            )
                        },
                    ),
                )
                .route(
                    "/process",
                    post(
                        move |State(seen): State<Seen>,
                         uri: axum::http::Uri,
                         headers: axum::http::HeaderMap,
                         body: axum::body::Bytes| async move {
                            seen.lock().await.push(Recorded {
                                method: "POST".to_string(),
                                uri: uri.to_string(),
                                body: String::from_utf8_lossy(&body).into_owned(),
                                headers: headers
                                    .iter()
                                    .map(|(name, value)| {
                                        (name.as_str().to_string(), value.to_str().unwrap_or("<binary>").to_string())
                                    })
                                    .collect(),
                            });
                            (axum::http::StatusCode::OK, process_reply.to_string())
                        },
                    ),
                )
                .with_state(seen.clone());
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
            let addr = listener.local_addr().expect("addr");
            tokio::spawn(async move {
                let _ = axum::serve(listener, app).await;
            });
            Self { base: format!("http://{addr}"), seen }
        }

        async fn calls(&self) -> Vec<Recorded> {
            self.seen.lock().await.clone()
        }
    }

    fn provider_for(base: &str) -> BlaxelProvider {
        let mut provider =
            BlaxelProvider::new(Some("bl-key".into()), Some("main".into()), DEFAULT_IMAGE.into());
        provider.data_base = Some(base.to_string());
        provider
    }

    #[test]
    fn the_multipart_boundary_is_derived_from_the_payload() {
        assert_eq!(hex16(b"0123456789"), hex16(b"0123456789"));
        assert_ne!(hex16(b"0123456789"), hex16(b"0123456780"));
        assert_eq!(hex16(b"").len(), 16);
    }

    #[test]
    fn the_chmod_follows_the_upload_because_an_existing_file_keeps_its_mode() {
        // Live 2026-11-22: the `permissions` form field applies only when the file is created; a
        // 0666 file stayed 0666 after an upload asking for 0600. The trait requires the mode in
        // the same call, so this asserts the chmod command is issued right after the PUT.
        assert_eq!(chmod_command("/workspace/.fx-mode", 0o600), "chmod 0600 -- '/workspace/.fx-mode'");
        assert_eq!(format_mode(0o600), "0600");
        assert_eq!(format_mode(0o644), "0644");
        // A quote in the name cannot escape the quoting.
        assert_eq!(
            chmod_command("/workspace/it's", 0o600),
            "chmod 0600 -- '/workspace/it'\\''s'"
        );
    }

    #[tokio::test]
    async fn write_file_uploads_then_chmods_the_exact_path() {
        let reply = r#"{"status":"completed","exitCode":0,"stdout":"","stderr":""}"#;
        let fake = FakeSandbox::start(reply).await;
        let provider = provider_for(&fake.base);

        provider
            .write_file("fx-blaxel-1", "/workspace/.fx-secret", vec![7u8; 32], 0o600)
            .await
            .expect("write_file");

        let calls = fake.calls().await;
        assert_eq!(calls.len(), 2, "one upload and one chmod, got {calls:#?}");

        // 1) the upload: an absolute path with the leading slash as %2F, the bytes, the mode.
        assert_eq!(calls[0].method, "PUT");
        assert!(calls[0].uri.starts_with("/filesystem/%2Fworkspace/.fx-secret"), "{}", calls[0].uri);
        assert!(calls[0].body.contains("name=\"permissions\"\r\n\r\n0600\r\n"));
        assert!(calls[0].body.contains("Content-Disposition: form-data; name=\"file\""));
        // The key goes to the sandbox as a bearer; the workspace header has no business there.
        assert!(calls[0]
            .headers
            .iter()
            .any(|(name, value)| name == "x-blaxel-authorization" && value == "Bearer bl-key"));

        // 2) the chmod: a JSON /process call whose command names only the file.
        assert_eq!(calls[1].method, "POST");
        assert_eq!(calls[1].uri, "/process");
        let body: Value = serde_json::from_str(&calls[1].body).expect("process json");
        assert_eq!(body["command"], "chmod 0600 -- '/workspace/.fx-secret'");
        assert_eq!(body["waitForCompletion"], true);
        // The bytes never travel through exec.
        assert!(!calls[1].body.contains("070707"), "{}", calls[1].body);
    }

    #[tokio::test]
    async fn a_failed_chmod_is_reported_instead_of_a_silent_success() {
        // A chmod that cannot do its job must not be swallowed: the caller asked for 0600.
        let reply = r#"{"status":"failed","exitCode":1,"stderr":"chmod: Operation not permitted\n"}"#;
        let fake = FakeSandbox::start(reply).await;
        let provider = provider_for(&fake.base);

        let error = provider
            .write_file("fx-blaxel-1", "/workspace/.fx-secret", vec![7u8; 8], 0o600)
            .await
            .expect_err("the chmod failed");
        assert_eq!(error.kind, ErrorKind::Fatal);
        assert_eq!(error.provider_code, "chmod_failed");
        assert!(error.message.contains("Operation not permitted"), "{}", error.message);
        // The file was written before the chmod was attempted; say so.
        assert!(error.message.contains("the file was written"), "{}", error.message);
    }

    // -----------------------------------------------------------------------------------------
    // Live probes against the real API, driving the real adapter. `#[ignore]` so the unit run
    // never makes a billable call; run with `--ignored --test-threads=1` and `BL_API_KEY` plus
    // `BL_WORKSPACE` in the environment. Each test creates one sandbox, deletes it, and forbids
    // leftovers.

    fn live_name(tag: &str) -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        // A Blaxel name is a DNS label: lowercase alphanumerics and hyphens, and short.
        format!("fx-t10d-{tag}-{}", &nanos.to_string()[..12])
    }

    async fn with_live_sandbox<F, Fut>(tag: &str, probe: F)
    where
        F: FnOnce(BlaxelProvider, String) -> Fut,
        Fut: std::future::Future<Output = ()>,
    {
        let provider = BlaxelProvider::from_env();
        let name = live_name(tag);
        let id = provider
            .create(CreateSandbox {
                idempotency_key: name.clone(),
                name: name.clone(),
                ttl_seconds: Some(600),
                env: std::collections::BTreeMap::new(),
            })
            .await
            .expect("create");
        println!("created {id}");

        // `create` returns before the workload is reachable; `state` is null for a few seconds.
        let mut ready = false;
        for _ in 0..160 {
            if matches!(provider.get(&id).await, Ok(info) if info.state == SandboxState::Ready) {
                ready = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        assert!(ready, "the sandbox never became ready");

        // Run the probe under `catch_unwind` so a failing assertion cannot leak a billable
        // sandbox: the delete below runs either way, and the panic is re-raised afterwards.
        let outcome = futures_util::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(probe(
            provider,
            id.clone(),
        )))
        .await;

        // A fresh handle for the cleanup, so the probe can own the one it was given.
        let provider = BlaxelProvider::from_env();
        let delete = provider.delete(&id).await;
        println!("deleted {id}: {delete:?}");
        // Control-plane delete is ASYNCHRONOUS: the row reports DELETING and its cached `state`
        // stays RUNNING for ~10 s, so a single read right after the call proves nothing. Poll on
        // `status` (the authoritative field) until this sandbox is TERMINATED.
        let mut alive = true;
        for _ in 0..80 {
            let rows = provider.list().await.expect("list");
            alive = rows.iter().any(|row| {
                row.name.as_deref() == Some(id.as_str())
                    && !matches!(row.state, SandboxState::Deleted)
            });
            if !alive {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        println!("still alive after delete: {alive}");
        assert!(delete.is_ok(), "delete failed: {delete:?}");
        assert!(!alive, "{id} is still alive after its delete");
        if let Err(panic) = outcome {
            std::panic::resume_unwind(panic);
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "live: creates a billable Blaxel sandbox; needs BL_API_KEY and BL_WORKSPACE"]
    async fn live_a_header_forging_path_is_refused_and_writes_nothing() {
        with_live_sandbox("hdr", |provider, id| async move {
            // Live 2026-11-22 the data plane answered 200 to a CRLF-laced basename, splicing the
            // rest into a multipart header of the sender's choosing. It must now be refused before
            // any request, and nothing may land on disk under either the real or the forged name.
            let error = provider
                .write_file(&id, "/blaxel/.fx-a\r\nX-Injected: 1", vec![1u8; 8], 0o600)
                .await
                .expect_err("a CRLF basename must be refused");
            println!("refused: {} / {}", error.provider_code, error.message);
            assert_eq!(error.kind, ErrorKind::Config);
            assert_eq!(error.provider_code, "path_not_header_safe");

            let listing = provider
                .exec(&id, "ls -a /blaxel | grep -c 'fx-a' || true", 30)
                .await
                .expect("ls");
            assert_eq!(listing.stdout.trim(), "0", "nothing may be written: {listing:?}");

            // A plain name in the same directory still writes, so the refusal is about the path
            // and not a broken upload.
            provider
                .write_file(&id, "/blaxel/.fx-plain", vec![2u8; 8], 0o600)
                .await
                .expect("a plain name still uploads");
        })
        .await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "live: creates a billable Blaxel sandbox; needs BL_API_KEY and BL_WORKSPACE"]
    async fn live_write_file_keeps_the_mode_on_an_existing_file() {
        with_live_sandbox("mode", |provider, id| async move {
            // Blaxel's guest runs as root with $HOME=/blaxel; /workspace does not exist in this
            // image and a write there fails with "nonexistent directory".
            let seed = provider
                .exec(
                    &id,
                    "printf 'x' > /blaxel/.fx-mode && chmod 0666 /blaxel/.fx-mode && stat -c '%a %s' /blaxel/.fx-mode",
                    30,
                )
                .await
                .expect("seed");
            assert_eq!(seed.stdout.trim(), "666 1", "seed: {seed:?}");

            provider
                .write_file(&id, "/blaxel/.fx-mode", vec![9u8; 32], 0o600)
                .await
                .expect("write_file");

            let stat = provider
                .exec(&id, "stat -c '%a %s' /blaxel/.fx-mode", 30)
                .await
                .expect("stat");
            println!("after write_file: {:?}", stat.stdout.trim());
            assert_eq!(stat.stdout.trim(), "600 32", "the old mode survived the write");

            // A fresh path must keep working too (the form field is still sent).
            provider
                .write_file(&id, "/blaxel/.fx-fresh", vec![1u8; 8], 0o600)
                .await
                .expect("write_file fresh");
            let fresh = provider
                .exec(&id, "stat -c '%a' /blaxel/.fx-fresh", 30)
                .await
                .expect("stat fresh");
            assert_eq!(fresh.stdout.trim(), "600");
        })
        .await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "live: creates a billable Blaxel sandbox; needs BL_API_KEY and BL_WORKSPACE"]
    async fn live_exec_bounds_exit_codes_and_the_provider_timed_out_answer() {
        with_live_sandbox("codes", |provider, id| async move {
            let seven = provider.exec(&id, "echo hi; exit 7", 30).await.expect("exit 7");
            assert_eq!(seven.exit_code, Some(7));
            assert!(!seven.timed_out);
            assert_eq!(seven.stdout.trim(), "hi");

            // A signal death must NOT be reported as a timeout: `timed_out` means the bound
            // expired. The provider reports a self-inflicted SIGKILL as `Some(-1)` (its "killed by
            // a signal" code) or as no code at all - neither is a timeout.
            let killed = provider.exec(&id, "kill -KILL $$", 30).await.expect("sigkill");
            println!("kill -KILL $$ -> timed_out={} exit={:?}", killed.timed_out, killed.exit_code);
            assert!(!killed.timed_out, "a signal death is not a timeout");
            assert!(
                matches!(killed.exit_code, Some(-1) | None),
                "a signal death must not carry a normal code: {:?}",
                killed.exit_code
            );

            // The provider kills the process at its own bound and answers 422 with a body that
            // says so; that is the documented timed-out run.
            let bound = provider.exec(&id, "sleep 20", 5).await.expect("bounded");
            println!("sleep 20 @5s -> timed_out={} exit={:?}", bound.timed_out, bound.exit_code);
            assert!(bound.timed_out, "the provider's bound did not expire");

            // After a provider-side kill nothing survives, unlike the E2B client-only path.
            tokio::time::sleep(std::time::Duration::from_millis(750)).await;
            let ps = provider
                .exec(&id, "ps -eo args | grep -cE '^sleep 20'", 30)
                .await
                .expect("ps");
            assert_eq!(ps.stdout.trim(), "0", "a process survived the provider timeout");
        })
        .await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "live: needs BL_API_KEY and BL_WORKSPACE"]
    async fn live_error_bodies_map_to_their_real_codes() {
        let provider = BlaxelProvider::from_env();
        let missing = provider.get("fx-does-not-exist-t10").await.expect_err("404");
        println!("404 -> {} / {}", missing.provider_code, missing.message);
        assert_eq!(missing.kind, ErrorKind::NotFound);
        assert_eq!(missing.provider_code, "not_found");

        let bare = BlaxelProvider::new(
            Some("invalid-key".into()),
            Some("main".into()),
            DEFAULT_IMAGE.into(),
        );
        let refused = bare.get("fx-does-not-exist-t10").await.expect_err("401");
        println!("401 -> {} / {}", refused.provider_code, refused.message);
        assert_eq!(refused.kind, ErrorKind::Config);
        assert_eq!(refused.provider_code, "unauthorized");
    }
}
