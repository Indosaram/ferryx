//! Prized: a Linux box behind `https://api.prized.dev`, reached only over the relay because its
//! SSH is **never internet-exposed** — see "Why relay" below.
//!
//! # Endpoints
//!
//! Recorded 2026-11-21 from `https://prized.dev/docs/api.md` (fetched whole) and
//! `https://prized.dev/docs/{cli,boxes,machines}.md`, cross-read against the live `401` from
//! `GET https://api.prized.dev/api/v1/boxes` (no key on this machine). Base control plane
//! `https://api.prized.dev/api/v1`, edge `$EDGE/v1/box/{box}/` where `$EDGE` is the `edge.url` of
//! `GET /api/v1/me`. Auth `Authorization: Bearer dcp_…` on both hosts (never logged).
//!
//! | Call | Request | Response |
//! | --- | --- | --- |
//! | create | `POST /boxes` +`Idempotency-Key` `{name,env,autoPauseMin?}` | `202 {box}` with `observedState: "requested"`; `409 box_limit\|name_taken\|tier_unavailable`; `429 box_create_limit` |
//! | get | `GET /boxes/{id}` | `200 {box,state,vitals,events,job,...}`; `404 not_found` |
//! | list | `GET /boxes?live=1&before=` | `200 {boxes,nextBefore}`, newest first, at most 100 |
//! | stop | `POST /boxes/{id}/suspend` | `202 {box}`; `410 box_deleted` mid-delete |
//! | resume | `POST /boxes/{id}/wake` | `202 {box}`; `410 box_deleted` |
//! | extend ttl | `PATCH /boxes/{id}` `{autoPauseMin}` | `200 {box}`; `409 box_not_running` on a paused box |
//! | delete | `DELETE /boxes/{id}` | `202 {box,operation}` |
//! | exec | `POST {edge}/v1/box/{box}/exec` `{command,cwd,timeoutSeconds,env}` | `200 exec.result` `{exitCode,stdout,stderr,timedOut,...}` |
//! | write file | `PUT {edge}/v1/box/{box}/files` `{path,content(base64),encoding,mode}` | `200 file.written` `{path,size}` |
//! | edge address | `GET /me` | `200 {customer,user,workspace,billing,boxes,edge:{url},key}` |
//!
//! # Why relay
//!
//! `prized.dev/docs/machines.md` states it plainly: "SSH is never on the internet: it and every
//! port you run are reached through the tunnel". A box has a private `boxIp` inside its own
//! WireGuard tunnel and a short-lived certificate; there is no host and port an outside ssh client
//! could dial. So `transport = Relay`, `ssh_target` keeps the trait default (`Config`,
//! `ssh_unsupported`), and a credential reaches the box through the edge's `PUT /files` — which is
//! why `write_file` is implemented here even though every other T1 provider leaves it alone.
//!
//! # Notes that shaped this adapter
//!
//! - **`202` means "in progress"** on create, suspend, wake, resize and delete. `create` returns the
//!   id as soon as the box row exists (`observedState: requested`); `get` then reports
//!   `Provisioning` until the box is `running`. Nothing here waits.
//! - `Idempotency-Key` is documented for create, and the name is the second net: `create` looks the
//!   name up on a `409 name_taken` and returns that box instead of a second billable one.
//! - `env` **is** a create field here, so `SandboxProvider::env` is set on the box by `create` and
//!   the box applies it to everything it runs, `exec` included. It is *not* repeated in the exec
//!   body: that route's body is `{command,cwd?,timeoutSeconds?,env?}` and a per-call `env` would
//!   only overlay the box's own for one command, which is not what the trait asks for.
//! - `ttl_seconds` maps to `autoPauseMin` (30-10080, the idle window). `ttlMinutes` is a *pause
//!   deadline* and `null` clears it, so it is not used: a deadline that elapses mid-run would stop
//!   the box under the operator.
//! - Free credit pins `autoPauseMin` at 120 and a box count of 1, so `native_auto_stop = true` and
//!   `409 box_limit` is a real answer an operator must resolve, not a retry.
//! - `POST /exec` wakes a suspended box (the one exception the docs allow); every other edge route
//!   answers `409 box_not_running`, which `write_file` maps to `Conflict`.
//! - **No key on this machine** (`PRIZED_API_KEY` is absent), so nothing in this file is verified
//!   live. The fixtures are transcribed from the documented wire shapes above and say so.

use std::sync::Mutex;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde::Deserialize;
use serde_json::{json, Value};

use super::{
    Capabilities, CreateSandbox, ErrorKind, ExecResult, ListedSandbox, SandboxError, SandboxInfo,
    SandboxProvider, SandboxState, Transport,
};

const PROVIDER: &str = "prized";
const API_BASE: &str = "https://api.prized.dev/api/v1";
/// A control-plane call is one bounded request; a 202 comes back at once.
const CONTROL_TIMEOUT: Duration = Duration::from_secs(120);
/// An edge call is bounded too: the edge is the box's own agent and a stalled one must not hold a
/// `write_file` (the call that carries a credential) open forever.
const EDGE_TIMEOUT: Duration = Duration::from_secs(120);
/// The edge's own exec limit is 600 s; nothing above it exists.
const MAX_EXEC_SECONDS: u64 = 600;
/// Default idle window when the caller names no TTL: the free credit's own (120 minutes).
const DEFAULT_AUTO_PAUSE_MINUTES: u64 = 120;
/// `autoPauseMin` bounds from the create table.
const MIN_AUTO_PAUSE_MINUTES: u64 = 30;
const MAX_AUTO_PAUSE_MINUTES: u64 = 10_080;
/// Upper bound on pages walked by the orphan scan.
const MAX_LIST_PAGES: usize = 20;

pub struct PrizedProvider {
    client: reqwest::Client,
    /// `None` when no key was found: every call then reports `Config`.
    token: Option<String>,
    /// `edge.url` from `GET /me`, fetched once per process. The docs say to read it, never assume.
    edge: Mutex<Option<String>>,
    /// Control-plane base. Always [`API_BASE`] outside tests; a test points it at a loopback stub
    /// so `delete`'s own status decision is asserted against the request it really sends.
    api_base: String,
}

impl PrizedProvider {
    pub fn new(token: Option<String>) -> Self {
        let client = reqwest::Client::builder()
            .user_agent(concat!("ferryx-cloud-host/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_default();
        Self { client, token, edge: Mutex::new(None), api_base: API_BASE.to_string() }
    }

    /// Key from `PRIZED_API_KEY` only; `prized login` keeps its own credential file.
    pub fn from_env() -> Self {
        let token = std::env::var("PRIZED_API_KEY").ok().filter(|value| !value.trim().is_empty());
        Self::new(token)
    }

    /// A provider with no credential, for the "call without a key reports Config" path.
    pub fn without_token() -> Self {
        Self::new(None)
    }

    fn token(&self) -> Result<&str, SandboxError> {
        self.token.as_deref().ok_or_else(|| {
            config(
                "missing_token",
                "no Prized key: set PRIZED_API_KEY (Prized has no key in ~/.config/compute-secrets.env)",
            )
        })
    }

    async fn send(
        &self,
        method: reqwest::Method,
        url: &str,
        body: Option<Value>,
        idempotency_key: Option<&str>,
    ) -> Result<(u16, String), SandboxError> {
        let token = self.token()?;
        let mut request =
            self.client.request(method, url).bearer_auth(token).timeout(CONTROL_TIMEOUT);
        if let Some(body) = body {
            request = request.json(&body);
        }
        if let Some(key) = idempotency_key {
            request = request.header("Idempotency-Key", key);
        }
        let response = request.send().await.map_err(|error| transport_error(&error))?;
        let status = response.status().as_u16();
        let text = response.text().await.map_err(|error| transport_error(&error))?;
        Ok((status, text))
    }

    /// The box row from `GET /boxes/{id}`, whose detail answer nests the row under `box`.
    async fn box_row(&self, box_id: &str, call: &str) -> Result<BoxBody, SandboxError> {
        let url = format!("{}/boxes/{}", self.api_base, encode_path(box_id));
        let (status, body) = self.send(reqwest::Method::GET, &url, None, None).await?;
        if status != 200 {
            return Err(status_error(status, &body, call));
        }
        parse_box(&body, call)
    }

    /// The box of this name, if the account has one (the `409 name_taken` fallback).
    async fn find_by_name(&self, name: &str) -> Result<Option<String>, SandboxError> {
        let mut url = url::Url::parse(&format!("{}/boxes", self.api_base))
            .map_err(|error| config("bad_url", format!("could not build the list URL: {error}")))?;
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("live", "1");
        }
        let (status, body) = self.send(reqwest::Method::GET, url.as_str(), None, None).await?;
        if status != 200 {
            return Err(status_error(status, &body, "create"));
        }
        let page: ListEnvelope = parse_json(&body, "create")?;
        Ok(page
            .boxes
            .into_iter()
            .find(|row| row.hostname.as_deref() == Some(name))
            .map(|row| row.id))
    }

    /// `$EDGE` from `GET /me`, cached for the process. Read once because the docs say the address
    /// is the account's and must not be assumed.
    async fn edge(&self) -> Result<String, SandboxError> {
        if let Ok(guard) = self.edge.lock() {
            if let Some(url) = guard.as_deref() {
                return Ok(url.to_string());
            }
        }
        let url = format!("{}/me", self.api_base);
        let (status, body) = self.send(reqwest::Method::GET, &url, None, None).await?;
        if status != 200 {
            return Err(status_error(status, &body, "edge"));
        }
        let parsed: MeBody = parse_json(&body, "edge")?;
        let edge = parsed.edge.map(|edge| edge.url).unwrap_or_default();
        if edge.trim().is_empty() {
            return Err(SandboxError::new(
                ErrorKind::Fatal,
                PROVIDER,
                "missing_edge_url",
                "GET /me answered without edge.url",
            ));
        }
        if let Ok(mut guard) = self.edge.lock() {
            *guard = Some(edge.clone());
        }
        Ok(edge)
    }

    /// One edge call: the box is addressed by name or id, and the answer is `{"ok":…}`-shaped.
    async fn edge_call(
        &self,
        method: reqwest::Method,
        box_id: &str,
        path: &str,
        body: Option<Value>,
        call: &str,
    ) -> Result<String, SandboxError> {
        let edge = self.edge().await?;
        let request = self.edge_request(&edge, method, box_id, path, body)?;
        let response = request.send().await.map_err(|error| transport_error(&error))?;
        let status = response.status().as_u16();
        let text = response.text().await.map_err(|error| transport_error(&error))?;
        if status != 200 {
            return Err(edge_status_error(status, &text, call));
        }
        Ok(text)
    }

    /// The edge request itself, bounded by [`EDGE_TIMEOUT`]. Kept apart from `edge_call` so the
    /// bound is asserted on a built request instead of on a live socket.
    fn edge_request(
        &self,
        edge: &str,
        method: reqwest::Method,
        box_id: &str,
        path: &str,
        body: Option<Value>,
    ) -> Result<reqwest::RequestBuilder, SandboxError> {
        let url = format!("{}/v1/box/{}{path}", edge.trim_end_matches('/'), encode_path(box_id));
        let token = self.token()?;
        let mut request =
            self.client.request(method, &url).bearer_auth(token).timeout(EDGE_TIMEOUT);
        if let Some(body) = body {
            request = request.json(&body);
        }
        Ok(request)
    }
}

#[async_trait::async_trait]
impl SandboxProvider for PrizedProvider {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            // `autoPauseMin` is always sent (the free credit pins it anyway), so a caller may omit
            // a TTL and still get a box that stops itself.
            ttl_required: false,
            max_ttl_seconds: Some(MAX_AUTO_PAUSE_MINUTES * 60),
            // SSH is tunnel-only, so agent traffic goes through the relay's outbound session.
            transport: Transport::Relay,
            native_auto_stop: true,
        }
    }

    async fn create(&self, request: CreateSandbox) -> Result<String, SandboxError> {
        let mut body = json!({
            "name": hostname_for(&request.name),
            "autoPauseMin": auto_pause_minutes(request.ttl_seconds),
        });
        if !request.env.is_empty() {
            body["env"] = json!(request.env);
        }
        let url = format!("{}/boxes", self.api_base);
        let (status, text) =
            self.send(reqwest::Method::POST, &url, Some(body), Some(&request.idempotency_key)).await?;
        match status {
            200..=202 => Ok(parse_box(&text, "create")?.id),
            // Our own earlier create won the name: reuse it rather than starting a second box.
            409 => match self.find_by_name(&hostname_for(&request.name)).await? {
                Some(box_id) => Ok(box_id),
                None => Err(status_error(409, &text, "create")),
            },
            _ => Err(status_error(status, &text, "create")),
        }
    }

    async fn get(&self, sandbox_id: &str) -> Result<SandboxInfo, SandboxError> {
        let row = self.box_row(sandbox_id, "get").await?;
        let raw = row.observed_state.clone().unwrap_or_default();
        let state = map_state(described_state(&raw, row.desired_state.as_deref()));
        // `degraded` is a box that still answers, so it stays `Ready`; only a launch that failed
        // needs the operator, and `job.lastError` is where the provider says why.
        let error = (state == SandboxState::Error).then(|| {
            row.job
                .and_then(|job| job.last_error)
                .unwrap_or_else(|| format!("box state is {raw}"))
        });
        Ok(SandboxInfo { state, error })
    }

    async fn stop(&self, sandbox_id: &str) -> Result<(), SandboxError> {
        let url = format!("{}/boxes/{}/suspend", self.api_base, encode_path(sandbox_id));
        let (status, body) = self.send(reqwest::Method::POST, &url, None, None).await?;
        if status == 200 || status == 202 {
            return Ok(());
        }
        // Suspending an already-suspended box is the `202` no-op the docs promise; anything else
        // is a real failure.
        Err(status_error(status, &body, "stop"))
    }

    async fn resume(&self, sandbox_id: &str, ttl_seconds: Option<u64>) -> Result<(), SandboxError> {
        // The idle window is a box setting, so it is written before the wake: otherwise the box
        // could come up under the old window and pause itself on the first quiet minute.
        if let Some(ttl_seconds) = ttl_seconds {
            self.patch_auto_pause(sandbox_id, auto_pause_minutes(Some(ttl_seconds))).await?;
        }
        let url = format!("{}/boxes/{}/wake", self.api_base, encode_path(sandbox_id));
        let (status, body) = self.send(reqwest::Method::POST, &url, None, None).await?;
        if status == 200 || status == 202 {
            return Ok(());
        }
        Err(status_error(status, &body, "resume"))
    }

    async fn delete(&self, sandbox_id: &str) -> Result<(), SandboxError> {
        let url = format!("{}/boxes/{}", self.api_base, encode_path(sandbox_id));
        let (status, body) = self.send(reqwest::Method::DELETE, &url, None, None).await?;
        // `202 {box,operation}`: the deletion is queued and irreversible. A `404` is returned as
        // [`ErrorKind::NotFound`], which the lifecycle treats as "already gone" - the same
        // outcome, while keeping the fact that the box was absent before this call.
        if delete_accepted(status) {
            return Ok(());
        }
        Err(status_error(status, &body, "delete"))
    }

    async fn extend_ttl(
        &self,
        sandbox_id: &str,
        ttl_seconds: Option<u64>,
    ) -> Result<(), SandboxError> {
        // `None` is the provider's own window (120 minutes on the free credit); writing it keeps
        // the field explicit rather than leaving whatever the box was created with.
        self.patch_auto_pause(sandbox_id, auto_pause_minutes(ttl_seconds)).await
    }

    async fn exec(
        &self,
        sandbox_id: &str,
        command: &str,
        timeout_seconds: u64,
    ) -> Result<ExecResult, SandboxError> {
        let timeout_seconds = timeout_seconds.clamp(1, MAX_EXEC_SECONDS);
        let body = exec_body(command, timeout_seconds);
        let edge = self.edge().await?;
        let url = format!("{}/v1/box/{}/exec", edge.trim_end_matches('/'), encode_path(sandbox_id));
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
        if status != 200 {
            return Err(edge_status_error(status, &text, "exec"));
        }
        let parsed = parse_exec(&text, "exec")?;
        Ok(ExecResult {
            exit_code: parsed.exit_code,
            // `timedOut` distinguishes the limit from a command that exited 124 by itself.
            timed_out: parsed.timed_out.unwrap_or(false),
            stdout: parsed.stdout.unwrap_or_default(),
            stderr: parsed.stderr.unwrap_or_default(),
        })
    }

    async fn list(&self) -> Result<Vec<ListedSandbox>, SandboxError> {
        let mut out = Vec::new();
        let mut before: Option<String> = None;
        // The orphan scan wants every box, terminated included, so `live` is not sent and
        // `nextBefore` is followed while it exists.
        for _ in 0..MAX_LIST_PAGES {
            let mut url = url::Url::parse(&format!("{}/boxes", self.api_base))
                .map_err(|error| config("bad_url", format!("could not build the list URL: {error}")))?;
            if let Some(before) = before.as_deref() {
                url.query_pairs_mut().append_pair("before", before);
            }
            let (status, body) = self.send(reqwest::Method::GET, url.as_str(), None, None).await?;
            if status != 200 {
                return Err(status_error(status, &body, "list"));
            }
            let page: ListEnvelope = parse_json(&body, "list")?;
            for row in page.boxes {
                let raw = row.observed_state.clone().unwrap_or_default();
                out.push(ListedSandbox {
                    sandbox_id: row.id,
                    state: map_state(described_state(&raw, row.desired_state.as_deref())),
                    created_at: row.created_at.as_deref().and_then(rfc3339_millis),
                    name: row.hostname,
                });
            }
            match page.next_before.filter(|next| !next.is_empty()) {
                Some(next) => before = Some(next),
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
        // Base64 in a JSON body: the edge caps an inline write at 1 MiB, and the encoding is what
        // keeps bytes byte-exact.
        let body = json!({
            "path": path,
            "content": BASE64.encode(&bytes),
            "encoding": "base64",
            "mode": format!("0{:o}", mode & 0o7777),
        });
        self.edge_call(reqwest::Method::PUT, sandbox_id, "/files", Some(body), "write_file")
            .await
            .map(|_| ())
    }
}

impl PrizedProvider {
    async fn patch_auto_pause(&self, sandbox_id: &str, minutes: u64) -> Result<(), SandboxError> {
        let url = format!("{}/boxes/{}", self.api_base, encode_path(sandbox_id));
        let body = json!({ "autoPauseMin": minutes });
        let (status, text) = self.send(reqwest::Method::PATCH, &url, Some(body), None).await?;
        if status == 200 {
            Ok(())
        } else {
            Err(status_error(status, &text, "extend_ttl"))
        }
    }
}

/// A box name is also its hostname: lowercase letters, digits and hyphens, at most 32.
///
/// The caller's `name` is a label (`fx: t8`), so it is flattened into that grammar; an empty or
/// unusable name falls back to `fx`, which the API accepts and the `fx` marker already implies.
fn hostname_for(name: &str) -> String {
    let mut host = String::with_capacity(name.len());
    for character in name.chars() {
        let lowered = character.to_ascii_lowercase();
        if lowered.is_ascii_alphanumeric() {
            host.push(lowered);
        } else if !host.ends_with('-') && !host.is_empty() {
            host.push('-');
        }
    }
    let host = host.trim_end_matches('-');
    let host = if host.len() > 32 { &host[..32] } else { host };
    let host = host.trim_end_matches('-');
    if host.is_empty() {
        "fx".to_string()
    } else {
        host.to_string()
    }
}

/// Seconds -> the `autoPauseMin` window (30-10080), rounded up; `None` keeps the free credit's own
/// 120 minutes rather than sending nothing (the field is what makes the box stop itself).
fn auto_pause_minutes(ttl_seconds: Option<u64>) -> u64 {
    match ttl_seconds {
        Some(seconds) => seconds.div_ceil(60).clamp(MIN_AUTO_PAUSE_MINUTES, MAX_AUTO_PAUSE_MINUTES),
        None => DEFAULT_AUTO_PAUSE_MINUTES,
    }
}

/// `observedState` is the live state, but a terminating or terminated `desiredState` outranks it:
/// a delete in progress cannot be walked back.
fn described_state<'a>(observed: &'a str, desired: Option<&'a str>) -> &'a str {
    match desired {
        Some("terminated") => "terminated",
        _ => observed,
    }
}

// ---------------------------------------------------------------------------------------------
// Wire types

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BoxBody {
    id: String,
    #[serde(default)]
    hostname: Option<String>,
    #[serde(default)]
    observed_state: Option<String>,
    #[serde(default)]
    desired_state: Option<String>,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    job: Option<JobBody>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JobBody {
    #[serde(default)]
    last_error: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListEnvelope {
    #[serde(default)]
    boxes: Vec<BoxBody>,
    #[serde(default)]
    next_before: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MeBody {
    #[serde(default)]
    edge: Option<EdgeBody>,
}

#[derive(Debug, Deserialize)]
struct EdgeBody {
    url: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExecBody {
    #[serde(default)]
    exit_code: Option<i32>,
    #[serde(default)]
    stdout: Option<String>,
    #[serde(default)]
    stderr: Option<String>,
    #[serde(default)]
    timed_out: Option<bool>,
}

/// The provider's `observedState` vocabulary (`requested|provisioning|bootstrapping|running|
/// suspending|suspended|waking|deep_sleep|resizing|migrating|degraded|provision_failed|
/// terminating|terminated`) in [`SandboxState`] terms.
fn map_state(raw: &str) -> SandboxState {
    match raw {
        "running" | "degraded" => SandboxState::Ready,
        "suspending" | "terminating" => SandboxState::Stopping,
        "suspended" | "deep_sleep" => SandboxState::Stopped,
        "provision_failed" => SandboxState::Error,
        "terminated" => SandboxState::Deleted,
        // requested, provisioning, bootstrapping, waking, resizing, migrating, and anything new.
        _ => SandboxState::Provisioning,
    }
}

/// The detail answer is `{box, state, vitals, …}`; the create/suspend answers are a bare `{box}`.
/// Both spellings are accepted so one parser serves every route.
fn parse_box(body: &str, call: &str) -> Result<BoxBody, SandboxError> {
    let value: Value = parse_json(body, call)?;
    let unexpected = |error: serde_json::Error| {
        SandboxError::new(
            ErrorKind::Fatal,
            PROVIDER,
            "unexpected_body",
            format!("{call}: the answer is not the documented box shape: {error}"),
        )
    };
    // `box` is a Rust keyword, so the nested row is picked out by name and parsed on its own.
    match value.get("box") {
        Some(row) => serde_json::from_value(row.clone()).map_err(unexpected),
        None if value.get("id").is_some() => serde_json::from_value(value).map_err(unexpected),
        None => Err(SandboxError::new(
            ErrorKind::Fatal,
            PROVIDER,
            "unexpected_body",
            format!("{call}: the answer carries no box"),
        )),
    }
}

fn encode_path(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
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

/// `exec.result` carries its fields beside `ok`/`type` (`{"ok":true,"type":"exec.result",
/// "exitCode":0,…}`), so the payload is read off the top level.
fn parse_exec(body: &str, call: &str) -> Result<ExecBody, SandboxError> {
    let value: Value = parse_json(body, call)?;
    if value.get("ok").and_then(Value::as_bool) == Some(false) {
        return Err(SandboxError::new(
            ErrorKind::Fatal,
            PROVIDER,
            "exec_failed",
            format!("{call}: the edge answered ok=false with HTTP 200"),
        ));
    }
    serde_json::from_value(value).map_err(|error| {
        SandboxError::new(
            ErrorKind::Fatal,
            PROVIDER,
            "unexpected_body",
            format!("{call}: the exec answer is not the documented shape: {error}"),
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

/// The edge `exec` body: `{command,cwd?,timeoutSeconds?,env?}` with only the two fields this
/// adapter sets. No `env`: `create` already set the box's own, and a per-call overlay is not what
/// [`SandboxProvider::env`] asks for.
fn exec_body(command: &str, timeout_seconds: u64) -> Value {
    json!({ "command": command, "timeoutSeconds": timeout_seconds })
}

/// Whether `delete` should answer `Ok`. Only the control plane's "queued" answers do: `202`, and
/// `200` from the idempotent replay.
///
/// A `404` is deliberately NOT success here. The lifecycle treats a delete's `NotFound` as
/// "already gone" (lifecycle.rs:407,413), so the caller's outcome is the same either way, but
/// returning it keeps the fact that the box was absent *before* this call instead of hiding it.
fn delete_accepted(status: u16) -> bool {
    matches!(status, 200 | 202)
}

fn config(code: impl Into<String>, message: impl Into<String>) -> SandboxError {
    SandboxError::new(ErrorKind::Config, PROVIDER, code, message)
}

/// The control plane's error shape is `{"error":"code", "message"?, ...}`; a few refusals add
/// numbers (`limit`, `retryAt`, `balanceMicros`) that the message already names in prose.
fn status_error(status: u16, body: &str, call: &str) -> SandboxError {
    let parsed = serde_json::from_str::<Value>(body).ok();
    let code = parsed
        .as_ref()
        .and_then(|value| value.get("error"))
        .and_then(Value::as_str)
        .unwrap_or("http_error")
        .to_string();
    let detail = parsed
        .as_ref()
        .and_then(|value| value.get("message"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| body.trim().to_string());
    let kind = match status {
        // 402 is a billing refusal the operator must lift; retrying it changes nothing.
        401..=403 => ErrorKind::Config,
        404 => ErrorKind::NotFound,
        // `410 box_deleted`: the box is being deleted and cannot be recovered, so it is gone.
        410 => ErrorKind::NotFound,
        409 => ErrorKind::Conflict,
        429 | 500..=599 => ErrorKind::Retryable,
        _ => ErrorKind::Fatal,
    };
    SandboxError::new(kind, PROVIDER, code, format!("{call}: HTTP {status}: {detail}"))
}

/// The edge's error shape is `{"ok":false, "error":{"code","message"}}`.
fn edge_status_error(status: u16, body: &str, call: &str) -> SandboxError {
    let parsed = serde_json::from_str::<Value>(body).ok();
    let error = parsed.as_ref().and_then(|value| value.get("error"));
    let message = error.and_then(|error| error.get("message")).and_then(Value::as_str);
    let code = error
        .and_then(|error| error.get("code"))
        .and_then(Value::as_str)
        // `POST /desktop/stop` uses the bare `{"error":"code"}` form.
        .or_else(|| parsed.as_ref().and_then(|value| value.get("error")).and_then(Value::as_str))
        .unwrap_or("http_error")
        .to_string();
    let detail = message.map(str::to_string).unwrap_or_else(|| body.trim().to_string());
    let kind = match status {
        401 | 403 => ErrorKind::Config,
        404 => ErrorKind::NotFound,
        // `box_not_running`, `agent_upgrade_required`: the request fights the box's state.
        409 => ErrorKind::Conflict,
        // `box_unreachable` (502) and `control_plane_unavailable` (5xx) are both worth one retry.
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
    // Never include the request: the header carries the key and the URL names the box.
    SandboxError::new(kind, PROVIDER, "transport", format!("request failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CREATE: &str = include_str!("../../../tests/fixtures/cloud_host/prized/create_202.json");
    const GET_RUNNING: &str =
        include_str!("../../../tests/fixtures/cloud_host/prized/get_running.json");
    const GET_SUSPENDED: &str =
        include_str!("../../../tests/fixtures/cloud_host/prized/get_suspended.json");
    const GET_TERMINATED: &str =
        include_str!("../../../tests/fixtures/cloud_host/prized/get_terminated.json");
    const LIST: &str = include_str!("../../../tests/fixtures/cloud_host/prized/list_200.json");
    const EXEC: &str = include_str!("../../../tests/fixtures/cloud_host/prized/exec_200.json");
    const EXEC_TIMEOUT: &str =
        include_str!("../../../tests/fixtures/cloud_host/prized/exec_timeout.json");
    const ME: &str = include_str!("../../../tests/fixtures/cloud_host/prized/me_200.json");
    const FILE_WRITTEN: &str =
        include_str!("../../../tests/fixtures/cloud_host/prized/file_written.json");
    const ERROR_401: &str =
        include_str!("../../../tests/fixtures/cloud_host/prized/error_401.json");
    const EDGE_ERROR_409: &str =
        include_str!("../../../tests/fixtures/cloud_host/prized/edge_error_409.json");
    const ERROR_410: &str =
        include_str!("../../../tests/fixtures/cloud_host/prized/error_410.json");

    #[test]
    fn a_create_answer_is_a_box_already_requested() {
        let row = parse_box(CREATE, "create").unwrap();
        assert_eq!(row.id, "5d0a0000-0000-4000-8000-000000000001");
        assert_eq!(row.hostname.as_deref(), Some("fx-t8-fixture"));
        assert_eq!(row.observed_state.as_deref(), Some("requested"));
        assert_eq!(describe_state(&row), SandboxState::Provisioning);
    }

    #[test]
    fn the_detail_answer_and_the_bare_box_both_parse() {
        let running = parse_box(GET_RUNNING, "get").unwrap();
        assert_eq!(running.id, "5d0a0000-0000-4000-8000-000000000001");
        assert_eq!(describe_state(&running), SandboxState::Ready);
        // The same parser reads the bare `{box}` shape.
        let bare = parse_box(CREATE, "get").unwrap();
        assert_eq!(bare.hostname.as_deref(), Some("fx-t8-fixture"));
    }

    #[test]
    fn suspended_is_stopped_and_terminated_is_deleted() {
        let suspended = parse_box(GET_SUSPENDED, "get").unwrap();
        assert_eq!(describe_state(&suspended), SandboxState::Stopped);
        let terminated = parse_box(GET_TERMINATED, "get").unwrap();
        assert_eq!(describe_state(&terminated), SandboxState::Deleted);
    }

    #[test]
    fn a_terminating_desired_state_outranks_a_running_observed_state() {
        assert_eq!(map_state(described_state("running", Some("terminated"))), SandboxState::Deleted);
        assert_eq!(map_state(described_state("suspended", Some("running"))), SandboxState::Stopped);
    }

    #[test]
    fn every_documented_state_has_a_home() {
        for raw in ["requested", "provisioning", "bootstrapping", "waking", "resizing", "migrating"] {
            assert_eq!(map_state(raw), SandboxState::Provisioning, "{raw}");
        }
        for raw in ["suspending", "terminating"] {
            assert_eq!(map_state(raw), SandboxState::Stopping, "{raw}");
        }
        for raw in ["suspended", "deep_sleep"] {
            assert_eq!(map_state(raw), SandboxState::Stopped, "{raw}");
        }
        assert_eq!(map_state("running"), SandboxState::Ready);
        // A degraded box still answers; only a failed launch needs the operator.
        assert_eq!(map_state("degraded"), SandboxState::Ready);
        assert_eq!(map_state("provision_failed"), SandboxState::Error);
        assert_eq!(map_state("terminated"), SandboxState::Deleted);
    }

    #[test]
    fn list_parses_newest_first_with_a_cursor() {
        let page: ListEnvelope = parse_json(LIST, "list").unwrap();
        assert_eq!(page.boxes.len(), 2);
        assert_eq!(page.boxes[0].hostname.as_deref(), Some("fx-t8-fixture"));
        assert_eq!(page.boxes[0].created_at.as_deref().and_then(rfc3339_millis), Some(1_767_225_600_000));
        assert_eq!(page.next_before.as_deref(), Some("bmV4dA=="));
    }

    #[test]
    fn me_carries_the_edge_address() {
        let parsed: MeBody = parse_json(ME, "edge").unwrap();
        assert_eq!(parsed.edge.map(|edge| edge.url).as_deref(), Some("https://edge.prized.dev"));
    }

    #[test]
    fn exec_answers_both_streams_and_the_exit_code() {
        let result = parse_exec(EXEC, "exec").unwrap();
        assert_eq!(result.exit_code, Some(0));
        assert_eq!(result.stdout.as_deref(), Some("x86_64\n"));
        assert_eq!(result.stderr.as_deref(), Some(""));
        assert_eq!(result.timed_out, Some(false));
    }

    #[test]
    fn a_time_limited_command_says_it_timed_out() {
        let result = parse_exec(EXEC_TIMEOUT, "exec").unwrap();
        assert_eq!(result.timed_out, Some(true));
        assert_eq!(result.exit_code, Some(124));
    }

    #[test]
    fn an_ok_false_answer_at_http_200_is_not_mistaken_for_success() {
        let error = parse_exec(r#"{"ok":false,"error":{"code":"exec_failed","message":"no"}}"#, "exec")
            .expect_err("ok=false is a failure");
        assert_eq!(error.kind, ErrorKind::Fatal);
        assert_eq!(error.provider_code, "exec_failed");
    }

    #[test]
    fn a_file_write_answer_is_the_path_and_size() {
        let value: Value = parse_json(FILE_WRITTEN, "write_file").unwrap();
        assert_eq!(value.get("ok").and_then(Value::as_bool), Some(true));
        assert_eq!(value.get("size").and_then(Value::as_u64), Some(32));
    }

    #[test]
    fn the_live_401_is_config_and_names_the_key_variable() {
        let error = status_error(401, ERROR_401, "list");
        assert_eq!(error.kind, ErrorKind::Config);
        assert_eq!(error.provider_code, "unauthorized");
        let missing = PrizedProvider::without_token().token().expect_err("no key");
        assert_eq!(missing.kind, ErrorKind::Config);
        assert!(missing.message.contains("PRIZED_API_KEY"));
    }

    #[test]
    fn a_delete_in_progress_is_not_found_rather_than_retryable() {
        let error = status_error(410, ERROR_410, "resume");
        assert_eq!(error.kind, ErrorKind::NotFound);
        assert_eq!(error.provider_code, "box_deleted");
    }

    #[test]
    fn the_control_plane_status_mapping() {
        assert_eq!(status_error(403, r#"{"error":"key_access_denied"}"#, "list").kind, ErrorKind::Config);
        assert_eq!(status_error(402, r#"{"error":"secret_limit"}"#, "list").kind, ErrorKind::Config);
        assert_eq!(status_error(404, r#"{"error":"not_found"}"#, "get").kind, ErrorKind::NotFound);
        assert_eq!(status_error(410, r#"{"error":"box_deleted"}"#, "resume").kind, ErrorKind::NotFound);
        assert_eq!(status_error(409, r#"{"error":"box_limit"}"#, "create").kind, ErrorKind::Conflict);
        assert_eq!(status_error(429, r#"{"error":"box_create_limit"}"#, "create").kind, ErrorKind::Retryable);
        assert_eq!(status_error(503, "{}", "list").kind, ErrorKind::Retryable);
        assert_eq!(status_error(422, r#"{"error":"storage_unavailable"}"#, "create").kind, ErrorKind::Fatal);
    }

    #[test]
    fn the_edge_status_mapping_reads_the_nested_error() {
        let error = edge_status_error(409, EDGE_ERROR_409, "write_file");
        assert_eq!(error.kind, ErrorKind::Conflict);
        assert_eq!(error.provider_code, "box_not_running");
        assert_eq!(edge_status_error(401, r#"{"ok":false,"error":{"code":"token_expired"}}"#, "exec").kind, ErrorKind::Config);
        assert_eq!(edge_status_error(404, r#"{"ok":false,"error":{"code":"not_found"}}"#, "exec").kind, ErrorKind::NotFound);
        assert_eq!(edge_status_error(502, r#"{"ok":false,"error":{"code":"box_unreachable"}}"#, "exec").kind, ErrorKind::Retryable);
        assert_eq!(edge_status_error(400, r#"{"ok":false,"error":{"code":"invalid_timeout"}}"#, "exec").kind, ErrorKind::Fatal);
    }

    #[test]
    fn a_ttl_becomes_a_bounded_idle_window() {
        assert_eq!(auto_pause_minutes(None), 120);
        assert_eq!(auto_pause_minutes(Some(60)), 30);
        assert_eq!(auto_pause_minutes(Some(1800)), 30);
        assert_eq!(auto_pause_minutes(Some(7200)), 120);
        assert_eq!(auto_pause_minutes(Some(7 * 24 * 3600)), 10_080);
        assert_eq!(auto_pause_minutes(Some(365 * 24 * 3600)), 10_080);
    }

    #[test]
    fn a_box_name_becomes_a_hostname_the_api_accepts() {
        assert_eq!(hostname_for("fx-t8"), "fx-t8");
        assert_eq!(hostname_for("fx: t8"), "fx-t8");
        assert_eq!(hostname_for("FX T8"), "fx-t8");
        assert_eq!(hostname_for("--"), "fx");
        assert_eq!(hostname_for(""), "fx");
        let long = hostname_for(&"a".repeat(50));
        assert_eq!(long.len(), 32);
        assert!(!long.ends_with('-'));
    }

    #[test]
    fn an_edge_call_is_bounded_like_a_control_plane_call() {
        let provider = PrizedProvider::new(Some("redacted".to_string()));
        let request = provider
            .edge_request(
                "https://edge.prized.dev/",
                reqwest::Method::PUT,
                "box-fixture-0001",
                "/files",
                Some(json!({ "path": "/home/fx/.fx-test" })),
            )
            .expect("a built request")
            .build()
            .expect("a request");
        // `write_file` is the call that carries a credential to the box; on a stalled edge an
        // unbounded one would hold the caller (and `cloud create`) open forever.
        assert_eq!(request.timeout(), Some(&EDGE_TIMEOUT));
        assert_eq!(request.method(), reqwest::Method::PUT);
        assert_eq!(request.url().as_str(), "https://edge.prized.dev/v1/box/box-fixture-0001/files");
        // The launch says `Relay`; nothing here builds an ssh path.
        assert_eq!(provider.capabilities().transport, Transport::Relay);
    }

    #[test]
    fn the_exec_body_carries_no_env_because_create_already_set_it() {
        // The module doc says env is a create field the box applies to every command; the exec
        // body must not also carry one, or the two would disagree.
        let body = exec_body("uname -m", 30);
        assert_eq!(body, json!({ "command": "uname -m", "timeoutSeconds": 30 }));
        assert!(body.get("env").is_none(), "{body}");
        assert!(body.get("cwd").is_none(), "{body}");
        let request = PrizedProvider::new(Some("redacted".to_string()))
            .edge_request(
                "https://edge.prized.dev",
                reqwest::Method::POST,
                "box-fixture-0001",
                "/exec",
                Some(body),
            )
            .expect("a built request")
            .build()
            .expect("a request");
        assert_eq!(request.timeout(), Some(&EDGE_TIMEOUT));
    }

    #[test]
    fn a_delete_that_finds_nothing_reports_not_found() {
        // Doc and code agree: a 404 is `NotFound`, which lifecycle.rs treats as success.
        assert!(!delete_accepted(404), "a 404 must not be answered as Ok");
        assert!(delete_accepted(202), "the queued delete is Ok");
        assert!(delete_accepted(200), "the idempotent replay is Ok");
        assert!(!delete_accepted(500), "a server error is not Ok");

        let error = status_error(404, r#"{"error":"not_found"}"#, "delete");
        assert_eq!(error.kind, ErrorKind::NotFound);
        assert!(error.is_not_found());
        assert!(error.message.starts_with("delete: HTTP 404"));
    }

    /// D5: the assertion above proves the *helper*; it says nothing about whether `delete()` calls
    /// it. Flipping the call site to accept a 404 left all 223 tests green, so `delete` itself is
    /// driven here against a loopback stub and its answer is what is asserted.
    #[tokio::test]
    async fn delete_drives_its_own_request_and_maps_every_status() {
        let box_id = "box-fixture-0001";

        // A `404` is `NotFound`, not `Ok`: the box was already absent before this call.
        let (base, head) =
            stub_answering(404, r#"{"error":"not_found","message":"no such box"}"#).await;
        let provider = provider_at(&base);
        let error = provider
            .delete(box_id)
            .await
            .expect_err("a delete that finds nothing must not answer Ok");
        assert_eq!(error.kind, ErrorKind::NotFound, "{}", error.message);
        assert_eq!(error.provider_code, "not_found");
        assert!(error.is_not_found());
        assert!(error.message.starts_with("delete: HTTP 404"), "{}", error.message);
        let head = head.await.expect("the stub task");
        assert!(head.starts_with(&format!("DELETE /boxes/{box_id} ")), "{head}");
        // The key rode along on the request that was actually sent.
        assert!(head.to_ascii_lowercase().contains("authorization: bearer "), "{head}");

        // `202` is the queued delete and `200` the idempotent replay: both are `Ok`.
        for status in [200u16, 202] {
            let (base, head) = stub_answering(
                status,
                r#"{"box":{"id":"box-fixture-0001"},"operation":"delete"}"#,
            )
            .await;
            let provider = provider_at(&base);
            let outcome = tokio::time::timeout(Duration::from_secs(20), provider.delete(box_id))
                .await
                .expect("delete() hung on a stub that answered");
            assert!(
                outcome.is_ok(),
                "HTTP {status} must be Ok: {:?}",
                outcome.err().map(|error| error.message)
            );
            let head = head.await.expect("the stub task");
            assert!(head.starts_with(&format!("DELETE /boxes/{box_id} ")), "{head}");
        }

        // A server error stays a retryable failure.
        let (base, head) = stub_answering(500, r#"{"error":"control_plane_unavailable"}"#).await;
        let provider = provider_at(&base);
        let error = provider.delete(box_id).await.expect_err("a 500 must not answer Ok");
        assert_eq!(error.kind, ErrorKind::Retryable, "{}", error.message);
        assert_eq!(error.provider_code, "control_plane_unavailable");
        assert!(error.message.starts_with("delete: HTTP 500"), "{}", error.message);
        let head = head.await.expect("the stub task");
        assert!(head.starts_with(&format!("DELETE /boxes/{box_id} ")), "{head}");
    }

    /// A provider whose control plane is the loopback stub instead of [`API_BASE`].
    fn provider_at(base: &str) -> PrizedProvider {
        let mut provider = PrizedProvider::new(Some("prized-fixture-key".to_string()));
        provider.api_base = base.to_string();
        provider
    }

    /// One-shot loopback control plane: it answers the single request `delete` sends with `status`
    /// and hands back the request head it received, so the method, the path and the header are
    /// asserted on the real bytes on the wire.
    async fn stub_answering(
        status: u16,
        body: &'static str,
    ) -> (String, tokio::task::JoinHandle<String>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let handle = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut head = Vec::new();
            let mut chunk = [0u8; 512];
            loop {
                let read = socket.read(&mut chunk).await.expect("read");
                if read == 0 {
                    break;
                }
                head.extend_from_slice(&chunk[..read]);
                if head.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let reason = match status {
                200 => "OK",
                202 => "Accepted",
                404 => "Not Found",
                500 => "Internal Server Error",
                _ => "Status",
            };
            let reply = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(reply.as_bytes()).await.expect("write");
            socket.flush().await.expect("flush");
            String::from_utf8_lossy(&head).into_owned()
        });
        (format!("http://{addr}"), handle)
    }

    #[test]
    fn a_write_mode_becomes_the_octal_string_the_edge_expects() {
        assert_eq!(format!("0{:o}", 0o600), "0600");
        assert_eq!(format!("0{:o}", 0o644), "0644");
        assert_eq!(format!("0{:o}", 0o4755 & 0o7777), "04755");
    }

    #[test]
    fn capabilities_are_relay_with_the_free_credits_idle_window() {
        let capabilities = PrizedProvider::without_token().capabilities();
        assert_eq!(capabilities.transport, Transport::Relay);
        assert!(!capabilities.ttl_required);
        assert_eq!(capabilities.max_ttl_seconds, Some(10_080 * 60));
        assert!(capabilities.native_auto_stop);
    }

    /// The name `describe_state` would have if it were a method: the fixtures are compared through
    /// one helper so the mapping stays in one place.
    fn describe_state(row: &BoxBody) -> SandboxState {
        map_state(described_state(
            row.observed_state.as_deref().unwrap_or_default(),
            row.desired_state.as_deref(),
        ))
    }
}
