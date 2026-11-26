//! E2B: a Firecracker microVM with no inbound port, so agent traffic only ever leaves over the
//! relay. Free Hobby credit is a monthly pool of vCPU-seconds and a 1-hour maximum sandbox TTL.
//!
//! # Endpoints
//!
//! Recorded 2026-11-21 from `e2b-dev/infra` `spec/openapi.yml` (platform API) and
//! `packages/envd/spec/envd.yaml` + `packages/envd/spec/process/process.proto` (the in-sandbox
//! data plane), cross-read against `e2b-dev/E2B` `packages/js-sdk/src/{connectionConfig.ts,
//! sandbox/index.ts, envd/rpc.ts}`. Base `https://api.e2b.app`, auth `X-API-Key: <key>` (never
//! logged). The v1 `/sandboxes` create and list routes are deprecated, so every call below is v2.
//!
//! | Call | Request | Response |
//! | --- | --- | --- |
//! | create | `POST /v2/sandboxes` + `Idempotency-Key` `{templateID,timeout,autoPause,metadata,envVars}` | `201 {"sandboxID",...}`; `409` idempotency mismatch / in progress |
//! | get | `GET /sandboxes/{sandboxID}` | `{"sandboxID","state":"running"\|"paused","envdAccessToken","domain"}`; `404` when gone |
//! | list | `GET /v2/sandboxes?state=running,paused&limit=` (+`nextToken`), pages via the `X-Next-Token` header | `[ListedSandbox]` |
//! | stop | `POST /sandboxes/{sandboxID}/pause` | `204` |
//! | resume | `POST /v2/sandboxes/{sandboxID}/connect` `{timeout}` | `200` already running / `201` resumed |
//! | extend ttl | `POST /sandboxes/{sandboxID}/timeout` `{timeout}` | `204` |
//! | delete | `DELETE /sandboxes/{sandboxID}` | `204`; `404` when already gone |
//! | exec | envd `POST https://sandbox.e2b.app/process.Process/Start`, headers `E2b-Sandbox-Id`, `E2b-Sandbox-Port: 49983`, `X-Access-Token` | Connect server-stream of `StartResponse` |
//! | write file | envd `POST /files?path=<abs>&username=<user>` body = raw bytes (`application/octet-stream`) | `[EntryInfo]`; then `chmod` by one `exec` |
//!
//! Notes that shaped this adapter:
//!
//! - `timeout` is **seconds**. The Hobby plan caps a sandbox at 1 hour, so the adapter clamps
//!   every TTL to 60..=3600 and reports `max_ttl_seconds = Some(3600)`.
//! - Create asks for `autoPause: true`: at the deadline E2B *pauses* the sandbox (a snapshot that
//!   keeps the disk) instead of destroying it, which is what the trait's "disk persists across
//!   stop/resume" requires. `resume` then calls `/connect` explicitly.
//! - A paused sandbox whose snapshot was taken while envd was unresponsive can only be resumed
//!   from disk: `/connect` answers `409 sandbox_snapshot_guest_agent_unresponsive`, and the retry
//!   that asks for `memory: false` cold-boots from disk. That fallback is implemented below.
//! - envd is reached through the stable `https://sandbox.e2b.app` front door (the JS SDK's
//!   `getSandboxUrl` returns exactly that for the supported `e2b.app` domain) and is told which
//!   sandbox to route to by `E2b-Sandbox-Id`; `E2b-Sandbox-Port` is the envd port, 49983. On a
//!   non-E2B domain the SDK switches to `https://{port}-{id}.{domain}`, which this adapter does
//!   too.
//! - `POST /files` takes `multipart/form-data` **or** raw `application/octet-stream` with `path`
//!   as a query parameter; the octet-stream form is used because this crate's `reqwest` has no
//!   `multipart` feature. envd's file API has no mode field, so `write_file` uploads the bytes,
//!   then applies the mode with one `chmod` through `exec`: the **content never travels through
//!   exec**, only the filename does.
//! - Connect streaming framing: each message is `[flags: u8][len: u32 big-endian][payload]`, the
//!   payload is protojson (`{"event":{"data":{"stdout":"<base64>"}}}`), and `flags & 0x02` marks
//!   the end-of-stream envelope (whose payload carries `{"error":{...}}` on failure).
//! - One bounded response, no unbounded stream: output is capped at 4 MiB (`Fatal` on overflow),
//!   and the connection is dropped as soon as the process end event arrives.
//! - The exec bound is enforced **inside** the sandbox, not by dropping the client future. envd
//!   honours a `Connect-Timeout-Ms` header by cancelling its own context, but it does not kill
//!   the child: live 2026-11-22, `sleep 120` with a 2 s deadline still showed up in `ps` five
//!   seconds later. So the command is wrapped in coreutils `timeout` (present in the `base`
//!   template, coreutils 9.1) which kills it at the same bound, and `-k 5` escalates to SIGKILL
//!   for a child that ignores SIGTERM. `timeout` reports an expired bound as exit code 124, so
//!   the adapter turns that into `timed_out: true` with the output captured before the overrun;
//!   a caller command that exits 124 itself is reported the same way (indistinguishable by rc).
//!   The `-k` escalation reports 137 instead, which is not a timeout on its face, so the wrapper
//!   rewrites 137 to 124 when the run reached its bound - live 2026-11-22, `trap '' TERM; sleep
//!   120` at a 3 s bound ended after 8.5 s and used to surface as a signal death.
//! - `timeout` signals **only the process it started** (the inner `/bin/sh`), and it does not wait
//!   for that shell's own children. A command that backgrounds work into a new session - e.g.
//!   `setsid sleep 300 &` - therefore escapes the bound: the wrapper still reports rc 124 at the
//!   deadline (so the caller sees `timed_out: true`), but the detached process keeps running
//!   inside the sandbox until the sandbox itself stops. Live 2026-11-22 at a 3 s bound: rc 124 in
//!   3.2 s, and `ps` still showed the child at +0 s, +1 s and +5 s. Plain background jobs are
//!   reaped because they stay in the wrapper's process group; only the new-session escape
//!   survives. Any in-sandbox bound built on `timeout` has this limit; the providers' `SendSignal`
//!   path has it too, which is why the wrapper is the mechanism and this note is the caveat.
//! - envd's own `Connect-Timeout-Ms` deadline sits well above the wrapper's SIGKILL (`bound + 15`)
//!   so a TERM-ignoring child still produces an end event before envd gives up. When envd's
//!   deadline does fire first, the stream closes with `deadline_exceeded` and **no** end event;
//!   the adapter reads that as an expired bound (`timed_out: true`), not as a retryable failure:
//!   the command was bounded, it just did not end politely.

use std::time::Duration;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};

use super::{
    Capabilities, CreateSandbox, ErrorKind, ExecResult, ListedSandbox, SandboxError, SandboxInfo,
    SandboxProvider, SandboxState, Transport,
};

const PROVIDER: &str = "e2b";
const API_BASE: &str = "https://api.e2b.app";
/// envd's fixed in-sandbox port; the SDK hardcodes it (`ConnectionConfig.envdPort`).
const ENVD_PORT: u16 = 49983;
/// The Hobby plan's maximum sandbox lifetime.
const MAX_TTL_SECONDS: u64 = 3_600;
/// Auto-stop when the caller passes no TTL: cheap enough to leave a box alone for a few minutes.
const DEFAULT_TTL_SECONDS: u64 = 600;
/// Must NOT have: one bounded response, no unbounded stream.
const EXEC_OUTPUT_CAP: usize = 4 * 1024 * 1024;
const LIST_PAGE_LIMIT: usize = 100;
/// Upper bound on pages walked by the orphan scan.
const MAX_LIST_PAGES: usize = 20;
/// Connect envelope flag: the message that closes the stream.
const ENVELOPE_END_STREAM: u8 = 0x02;
/// Coreutils `timeout`'s own exit code when the bound expired (`--preserve-status` unset).
const EXEC_TIMEOUT_EXIT: i32 = 124;
/// Sent to envd as `Connect-Timeout-Ms` beyond the caller's bound: envd must outlive the
/// in-sandbox `timeout` so the end event (and the output before the overrun) still arrives.
/// `timeout -k 5` SIGKILLs at `bound + 5`, so the margin has to clear that with room to spare:
/// 15 s leaves 10 s for the kill, the end event and the round trip. If envd still wins the race the
/// stream closes with `deadline_exceeded`, which `exec` reports as a timeout (see below).
const ENVD_DEADLINE_GRACE_SECONDS: u64 = 15;
/// Connect's code for "my own deadline expired"; on an `exec` we bounded it means the bound.
const DEADLINE_EXCEEDED: &str = "deadline_exceeded";
/// `timeout -k` escalation plus transport slack on top of the bound.
const EXEC_CLIENT_GRACE_SECONDS: u64 = 30;
/// `/bin/sh -c` wrapper: `$1` is the bound in seconds, `$2` the caller's command line. A command
/// line never becomes a program name, so nothing the caller passes can be read as an option.
///
/// The `137` rewrite is what keeps "124 means the bound expired" true. Coreutils `timeout` exits
/// `124` when its SIGTERM ended the command, but `137` (128+KILL) when `-k 5` had to escalate -
/// live 2026-11-22, `trap '' TERM; sleep 120` at a 3 s bound ended after 8.5 s with 137, which the
/// adapter reported as an ordinary signal death rather than a timeout. A command that SIGKILLs
/// *itself* also yields 137 and that is a real signal exit the caller must see, so the rewrite
/// requires the run to have reached its bound: a self-kill happens inside the bound, the
/// escalation can only happen at it. Without `date` the rewrite is skipped and 137 stands.
const EXEC_WRAPPER: &str = "if command -v timeout >/dev/null 2>&1; then\n\
  started=$(date +%s 2>/dev/null || echo 0)\n\
  timeout -s TERM -k 5 \"$1\" /bin/sh -c \"$2\"\n\
  status=$?\n\
  if [ \"$status\" = 137 ] && [ \"$started\" != 0 ]; then\n\
  now=$(date +%s 2>/dev/null || echo 0)\n\
  if [ \"$now\" != 0 ] && [ $((now - started)) -ge \"$1\" ]; then\n\
  exit 124\n\
  fi\n\
  fi\n\
  exit $status\n\
  fi\n\
# No coreutils timeout in this image: the command runs unbounded and only the client\
# bound applies.\nexec /bin/sh -c \"$2\"\n";
/// Connect envelope flag: the message is compressed (never produced by envd's JSON codec).
const ENVELOPE_COMPRESSED: u8 = 0x01;

pub struct E2bProvider {
    client: reqwest::Client,
    /// `None` when no key was found: every call then reports `Config`.
    token: Option<String>,
    /// Image every box boots from; `E2B_TEMPLATE` overrides it.
    template: String,
    /// Deployment domain; `e2b.app` unless `E2B_DOMAIN` says otherwise.
    domain: String,
    /// Control-plane base. Always [`API_BASE`] outside tests; a test points it at a local
    /// recorder so the bytes the adapter really sends can be asserted.
    api_base: String,
    /// envd base, when a test must answer the exec/file wire itself instead of a real sandbox.
    /// Always `None` outside tests.
    envd_base: Option<String>,
}

impl E2bProvider {
    pub fn new(token: Option<String>, template: String, domain: String) -> Self {
        let client = reqwest::Client::builder()
            .user_agent(concat!("ferryx-cloud-host/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_default();
        Self {
            client,
            token,
            template,
            domain,
            api_base: API_BASE.to_string(),
            envd_base: None,
        }
    }

    /// Key from `E2B_API_KEY`, template from `E2B_TEMPLATE` (default `base`), domain from
    /// `E2B_DOMAIN` (default `e2b.app`).
    pub fn from_env() -> Self {
        let token = crate::cloud_host::secrets::load_var("E2B_API_KEY");
        let template = crate::cloud_host::secrets::load_var("E2B_TEMPLATE")
            .unwrap_or_else(|| "base".to_string());
        let domain = crate::cloud_host::secrets::load_var("E2B_DOMAIN")
            .unwrap_or_else(|| "e2b.app".to_string());
        Self::new(token, template, domain)
    }

    /// A provider with no credential, for the "call without a key reports Config" path.
    pub fn without_token() -> Self {
        Self::new(None, "base".to_string(), "e2b.app".to_string())
    }

    fn token(&self) -> Result<&str, SandboxError> {
        self.token
            .as_deref()
            .ok_or_else(|| config("missing_token", "no E2B key: set E2B_API_KEY"))
    }

    async fn call(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
        key_header: Option<&str>,
    ) -> Result<ApiResponse, SandboxError> {
        let token = self.token()?;
        let is_body_method = matches!(
            method,
            reqwest::Method::POST | reqwest::Method::PUT | reqwest::Method::PATCH
        );
        let mut request = self
            .client
            .request(method, format!("{}{path}", self.api_base))
            .header("X-API-Key", token);
        if let Some(key) = key_header {
            request = request.header("Idempotency-Key", key);
        }
        if let Some(body) = body {
            request = request.json(&body);
        } else if is_body_method {
            // E2B's load balancer rejects bodyless POSTs (such as `/pause`) with 411 unless
            // `Content-Length: 0` is sent explicitly.
            request = request.header(reqwest::header::CONTENT_LENGTH, "0");
        }
        let response = request.send().await.map_err(|error| transport_error(&error))?;
        let status = response.status().as_u16();
        let next_token = response
            .headers()
            .get("X-Next-Token")
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let body = response.text().await.map_err(|error| transport_error(&error))?;
        Ok(ApiResponse { status, body, next_token })
    }

    /// The control-plane view of one sandbox, which is also where envd's access token comes from.
    async fn detail(&self, sandbox_id: &str) -> Result<SandboxDetail, SandboxError> {
        let response = self
            .call(
                reqwest::Method::GET,
                &format!("/sandboxes/{sandbox_id}"),
                None,
                None,
            )
            .await?;
        if response.status != 200 {
            return Err(status_error(response.status, &response.body, "get"));
        }
        parse_json::<SandboxDetail>(&response.body, "get")
    }

    /// Seconds, clamped to the 1-hour Hobby cap.
    fn ttl_seconds(ttl_seconds: Option<u64>) -> u64 {
        ttl_seconds.unwrap_or(DEFAULT_TTL_SECONDS).clamp(60, MAX_TTL_SECONDS)
    }

    /// envd's base URL for one sandbox: the shared front door on E2B's own domains, the
    /// per-sandbox host otherwise.
    fn envd_base(&self, sandbox_id: &str, detail_domain: Option<&str>) -> String {
        if let Some(base) = self.envd_base.as_deref() {
            return base.to_string();
        }
        let domain = detail_domain.unwrap_or(&self.domain);
        if supported_domain(domain) {
            format!("https://sandbox.{domain}")
        } else {
            format!("https://{ENVD_PORT}-{sandbox_id}.{domain}")
        }
    }
}

#[async_trait::async_trait]
impl SandboxProvider for E2bProvider {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            // `autoPause` at the timeout is what makes the disk survive, so a TTL must be passed.
            ttl_required: false,
            max_ttl_seconds: Some(MAX_TTL_SECONDS),
            transport: Transport::Relay,
            native_auto_stop: true,
        }
    }

    async fn create(&self, request: CreateSandbox) -> Result<String, SandboxError> {
        let mut body = json!({
            "templateID": self.template,
            "timeout": Self::ttl_seconds(request.ttl_seconds),
            // Pause at the deadline instead of destroying the disk; `resume` reconnects.
            "autoPause": true,
            "metadata": { "launcher": "ferryx", "fx": request.name },
        });
        if !request.env.is_empty() {
            body["envVars"] = json!(request.env);
        }
        // The header only accepts lowercase letters, digits and hyphens, and a retry after a
        // dropped response must land on the same sandbox instead of a second billable one.
        let key = idempotency_key(&request.idempotency_key);
        let response = self
            .call(reqwest::Method::POST, "/v2/sandboxes", Some(body), Some(&key))
            .await?;
        match response.status {
            200 | 201 => {
                let created: Sandbox = parse_json(&response.body, "create")?;
                Ok(created.sandbox_id)
            }
            _ => Err(status_error(response.status, &response.body, "create")),
        }
    }

    async fn get(&self, sandbox_id: &str) -> Result<SandboxInfo, SandboxError> {
        let detail = self.detail(sandbox_id).await?;
        let raw = detail.state.unwrap_or_default();
        let state = match raw.as_str() {
            "running" => SandboxState::Ready,
            // Paused keeps the disk and resumes on `/connect`; it is a stop, not a failure.
            "paused" => SandboxState::Stopped,
            "pausing" => SandboxState::Stopping,
            "starting" | "provisioning" => SandboxState::Provisioning,
            "" => SandboxState::Provisioning,
            _ => SandboxState::Provisioning,
        };
        Ok(SandboxInfo { state, error: None })
    }

    async fn stop(&self, sandbox_id: &str) -> Result<(), SandboxError> {
        let response = self
            .call(
                reqwest::Method::POST,
                &format!("/sandboxes/{sandbox_id}/pause"),
                None,
                None,
            )
            .await?;
        match response.status {
            200 | 204 => Ok(()),
            // Pausing a sandbox that is already paused has nothing left to do.
            409 => Ok(()),
            _ => Err(status_error(response.status, &response.body, "stop")),
        }
    }

    async fn resume(&self, sandbox_id: &str, ttl_seconds: Option<u64>) -> Result<(), SandboxError> {
        let path = format!("/v2/sandboxes/{sandbox_id}/connect");
        let body = json!({ "timeout": Self::ttl_seconds(ttl_seconds) });
        let response = self.call(reqwest::Method::POST, &path, Some(body.clone()), None).await?;
        match response.status {
            200 | 201 => Ok(()),
            // A snapshot taken while envd was unresponsive only resumes from disk.
            409 if response.body.contains("sandbox_snapshot_guest_agent_unresponsive") => {
                let mut from_disk = body;
                from_disk["memory"] = json!(false);
                let retry = self.call(reqwest::Method::POST, &path, Some(from_disk), None).await?;
                match retry.status {
                    200 | 201 => Ok(()),
                    _ => Err(status_error(retry.status, &retry.body, "resume")),
                }
            }
            _ => Err(status_error(response.status, &response.body, "resume")),
        }
    }

    async fn delete(&self, sandbox_id: &str) -> Result<(), SandboxError> {
        let response = self
            .call(
                reqwest::Method::DELETE,
                &format!("/sandboxes/{sandbox_id}"),
                None,
                None,
            )
            .await?;
        match response.status {
            200 | 204 => Ok(()),
            _ => Err(status_error(response.status, &response.body, "delete")),
        }
    }

    async fn extend_ttl(
        &self,
        sandbox_id: &str,
        ttl_seconds: Option<u64>,
    ) -> Result<(), SandboxError> {
        let body = json!({ "timeout": Self::ttl_seconds(ttl_seconds) });
        let response = self
            .call(
                reqwest::Method::POST,
                &format!("/sandboxes/{sandbox_id}/timeout"),
                Some(body),
                None,
            )
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
        let detail = self.detail(sandbox_id).await?;
        let base = self.envd_base(sandbox_id, detail.domain.as_deref());
        let bound = timeout_seconds.max(1);
        // The bound is enforced by `timeout` inside the sandbox (see `EXEC_WRAPPER`): envd's own
        // deadline stops the stream but leaves the child running, so it is only a backstop here
        // and it is set past the in-sandbox bound so the end event still arrives.
        let body = json!({
            "process": {
                "cmd": "/bin/sh",
                "args": ["-c", EXEC_WRAPPER, "fx-exec", bound.to_string(), command],
            },
            "stdin": false,
        });
        let payload = serde_json::to_vec(&body).map_err(|error| {
            fatal("exec_encode_failed", format!("could not encode the exec request: {error}"))
        })?;
        let envd_deadline_ms = bound.saturating_add(ENVD_DEADLINE_GRACE_SECONDS).saturating_mul(1_000);
        let mut request = self
            .client
            .post(format!("{base}/process.Process/Start"))
            .header("content-type", "application/connect+json")
            .header("connect-protocol-version", "1")
            .header("E2b-Sandbox-Id", sandbox_id)
            .header("E2b-Sandbox-Port", ENVD_PORT.to_string())
            .header("Connect-Timeout-Ms", envd_deadline_ms.to_string())
            .timeout(Duration::from_secs(bound.saturating_add(EXEC_CLIENT_GRACE_SECONDS)))
            .body(encode_envelope(0, &payload));
        if let Some(token) = detail.envd_access_token.as_deref() {
            request = request.header("X-Access-Token", token);
        }
        let response = match request.send().await {
            Ok(response) => response,
            // envd outlived the in-sandbox bound and still did not answer: `timeout` has already
            // killed the command, so this is a timed-out run, not a leak.
            Err(error) if error.is_timeout() => return Ok(timed_out()),
            Err(error) => return Err(transport_error(&error)),
        };
        let status = response.status().as_u16();
        if status != 200 {
            let body = response.text().await.unwrap_or_default();
            return Err(envd_status_error(status, &body, "exec"));
        }
        let mut stream = response.bytes_stream();
        let mut decoder = EnvelopeDecoder::default();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut exit_code = None;
        let mut finished = false;
        while !finished {
            while let Some((flags, payload)) = decoder.next_frame() {
                if flags & ENVELOPE_COMPRESSED != 0 {
                    return Err(fatal(
                        "compressed_stream",
                        "envd answered with a compressed Connect envelope, which is unsupported",
                    ));
                }
                if flags & ENVELOPE_END_STREAM != 0 {
                    if let Some(error) = stream_error(&payload) {
                        // envd's deadline is `bound + ENVD_DEADLINE_GRACE_SECONDS`, so it firing
                        // means this call's own bound expired: `timeout` has already killed the
                        // command (or is about to), and the stream simply died before its end
                        // event. A caller must see a timeout here, not a retryable error - retrying
                        // a command that ran out of time can loop forever.
                        if error.provider_code == DEADLINE_EXCEEDED {
                            return Ok(timed_out_after(&stdout, &stderr));
                        }
                        return Err(error);
                    }
                    finished = true;
                    break;
                }
                match parse_event(&payload)? {
                    Event::Start | Event::KeepAlive | Event::Unknown => {}
                    Event::Stdout(bytes) => {
                        stdout.extend_from_slice(&bytes);
                        check_output_cap(stdout.len() + stderr.len())?;
                    }
                    Event::Stderr(bytes) => {
                        stderr.extend_from_slice(&bytes);
                        check_output_cap(stdout.len() + stderr.len())?;
                    }
                    Event::End { exit_code: code, error } => {
                        exit_code = code;
                        if let Some(error) = error {
                            stderr.extend_from_slice(error.as_bytes());
                        }
                        // The process is over: stop reading and let the connection drop.
                        finished = true;
                        break;
                    }
                }
            }
            if finished {
                break;
            }
            match stream.next().await {
                Some(Ok(chunk)) => decoder.push(&chunk),
                // The client bound is past the in-sandbox one, so reaching it means the end event
                // was lost in transport: report the bound as expired, keeping what was captured.
                Some(Err(error)) if error.is_timeout() => {
                    return Ok(timed_out_after(&stdout, &stderr))
                }
                Some(Err(error)) => return Err(transport_error(&error)),
                None => break,
            }
        }
        // `timeout` reports an expired bound with its own exit code; the output captured before
        // the overrun is the useful part and is kept.
        if exit_code == Some(EXEC_TIMEOUT_EXIT) {
            return Ok(timed_out_after(&stdout, &stderr));
        }
        Ok(ExecResult {
            exit_code,
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
            timed_out: false,
        })
    }

    async fn write_file(
        &self,
        sandbox_id: &str,
        path: &str,
        bytes: Vec<u8>,
        mode: u32,
    ) -> Result<(), SandboxError> {
        ensure_quotable(path)?;
        let detail = self.detail(sandbox_id).await?;
        let base = self.envd_base(sandbox_id, detail.domain.as_deref());
        let url = format!("{base}/files?path={}&username=user", query_encode(path));
        // envd authenticates with the sandbox-scoped token only; the account key never leaves
        // the control plane (it is `token()`'s job to have failed above with Config when unset).
        self.token()?;
        let mut request = self
            .client
            .post(&url)
            .header("content-type", "application/octet-stream")
            .header("E2b-Sandbox-Id", sandbox_id)
            .header("E2b-Sandbox-Port", ENVD_PORT.to_string())
            .timeout(Duration::from_secs(120))
            .body(bytes);
        if let Some(access) = detail.envd_access_token.as_deref() {
            request = request.header("X-Access-Token", access);
        }
        let response = request.send().await.map_err(|error| transport_error(&error))?;
        let status = response.status().as_u16();
        if status != 200 && status != 201 {
            let body = response.text().await.unwrap_or_default();
            return Err(envd_status_error(status, &body, "write_file"));
        }
        // envd's upload has no mode field, so the mode is applied right after, in one one-shot
        // exec. Only the filename travels there; the content is already on disk. This also fixes
        // up an upload onto a path that already existed, whose old mode envd keeps.
        let chmod = self.exec(sandbox_id, &chmod_command(path, mode), 60).await?;
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
        let mut next: Option<String> = None;
        for _ in 0..MAX_LIST_PAGES {
            let mut path = format!(
                "/v2/sandboxes?state=running,paused&limit={LIST_PAGE_LIMIT}"
            );
            if let Some(token) = next.as_deref() {
                path.push_str("&nextToken=");
                path.push_str(&query_encode(token));
            }
            let response = self.call(reqwest::Method::GET, &path, None, None).await?;
            if response.status != 200 {
                return Err(status_error(response.status, &response.body, "list"));
            }
            let page: Vec<ListedSandboxWire> = parse_json(&response.body, "list")?;
            listed.extend(page.into_iter().map(ListedSandboxWire::into_listed));
            next = response.next_token;
            if next.is_none() {
                return Ok(listed);
            }
        }
        Ok(listed)
    }
}

/// One control-plane answer, with the pagination header the listing pages on.
struct ApiResponse {
    status: u16,
    body: String,
    next_token: Option<String>,
}

// ---------------------------------------------------------------------------------------------
// Wire types

#[derive(Debug, Deserialize)]
struct Sandbox {
    #[serde(rename = "sandboxID", alias = "sandboxId")]
    sandbox_id: String,
}

/// One sandbox's detail. The id is deliberately not read back: the caller already holds it, and
/// every field here is either addressed by it (`envd_access_token`, `domain`) or reported.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SandboxDetail {
    #[serde(default)]
    state: Option<String>,
    /// Bearer for envd; the control-plane key must never be sent to the sandbox itself.
    #[serde(default)]
    envd_access_token: Option<String>,
    #[serde(default)]
    domain: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListedSandboxWire {
    #[serde(rename = "sandboxID", alias = "sandboxId")]
    sandbox_id: String,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    started_at: Option<String>,
    #[serde(default)]
    metadata: Option<Value>,
}

impl ListedSandboxWire {
    fn into_listed(self) -> ListedSandbox {
        let state = match self.state.as_deref() {
            Some("running") => SandboxState::Ready,
            Some("paused") => SandboxState::Stopped,
            Some("pausing") => SandboxState::Stopping,
            _ => SandboxState::Provisioning,
        };
        let name = self
            .metadata
            .as_ref()
            .and_then(|value| value.get("fx"))
            .and_then(Value::as_str)
            .map(str::to_string);
        ListedSandbox {
            sandbox_id: self.sandbox_id,
            state,
            created_at: self.started_at.as_deref().and_then(rfc3339_millis),
            name,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Connect streaming

/// One message of a Connect stream, split out of the socket's byte chunks.
#[derive(Default)]
struct EnvelopeDecoder {
    buffer: Vec<u8>,
}

impl EnvelopeDecoder {
    fn push(&mut self, chunk: &[u8]) {
        self.buffer.extend_from_slice(chunk);
    }

    /// `None` until a whole message (5-byte header plus payload) has arrived.
    fn next_frame(&mut self) -> Option<(u8, Vec<u8>)> {
        if self.buffer.len() < 5 {
            return None;
        }
        let flags = self.buffer[0];
        let length =
            u32::from_be_bytes([self.buffer[1], self.buffer[2], self.buffer[3], self.buffer[4]])
                as usize;
        if self.buffer.len() < 5 + length {
            return None;
        }
        let payload = self.buffer[5..5 + length].to_vec();
        self.buffer.drain(..5 + length);
        Some((flags, payload))
    }
}

/// `[flags][len: u32 BE][payload]`, the frame a Connect streaming request body is made of.
fn encode_envelope(flags: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(5 + payload.len());
    out.push(flags);
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(payload);
    out
}

/// One decoded `ProcessEvent`.
enum Event {
    Start,
    Stdout(Vec<u8>),
    Stderr(Vec<u8>),
    End { exit_code: Option<i32>, error: Option<String> },
    KeepAlive,
    Unknown,
}

fn parse_event(payload: &[u8]) -> Result<Event, SandboxError> {
    let value: Value = serde_json::from_slice(payload).map_err(|error| {
        fatal(
            "unexpected_stream_payload",
            format!("envd sent a stream message that is not JSON: {error}"),
        )
    })?;
    let event = value.get("event").unwrap_or(&Value::Null);
    if let Some(data) = event.get("data") {
        if let Some(stdout) = field(data, &["stdout"]).and_then(Value::as_str) {
            return Ok(Event::Stdout(decode_bytes(stdout)));
        }
        if let Some(stderr) = field(data, &["stderr"]).and_then(Value::as_str) {
            return Ok(Event::Stderr(decode_bytes(stderr)));
        }
        if let Some(pty) = field(data, &["pty"]).and_then(Value::as_str) {
            return Ok(Event::Stdout(decode_bytes(pty)));
        }
    }
    if event.get("start").is_some() {
        return Ok(Event::Start);
    }
    if event.get("keepalive").is_some() {
        return Ok(Event::KeepAlive);
    }
    if let Some(end) = event.get("end") {
        let exited = field(end, &["exited"]).and_then(Value::as_bool).unwrap_or(true);
        let status = field(end, &["status"]).and_then(Value::as_str);
        // Live envd 0.2.9 omits `exitCode` when the code is 0 and reports it only inside
        // `status` ("exit status 0"); a non-zero code arrives in both fields. Read `exitCode`
        // first, then fall back to the number in `status`.
        let code = field(end, &["exitCode", "exit_code"])
            .and_then(Value::as_i64)
            .or_else(|| status.and_then(status_exit_code))
            .map(|code| code as i32);
        // `error` repeats `status` ("exit status 7") for an ordinary non-zero exit; that is not
        // the program's stderr and must not be reported as it.
        let error = field(end, &["error"])
            .and_then(Value::as_str)
            .filter(|text| status_exit_code(text).is_none())
            .map(str::to_string);
        // `exited: false` is a process killed by a signal: it ended without an exit code.
        return Ok(Event::End { exit_code: if exited { code } else { None }, error });
    }
    Ok(Event::Unknown)
}

/// The end-of-stream envelope carries `{"error":{"code","message"}}` when the call failed.
fn stream_error(payload: &[u8]) -> Option<SandboxError> {
    let value: Value = serde_json::from_slice(payload).ok()?;
    let error = value.get("error")?;
    if error.is_null() {
        return None;
    }
    let code = error.get("code").and_then(Value::as_str).unwrap_or("stream_error");
    let message = error.get("message").and_then(Value::as_str).unwrap_or("stream failed");
    Some(SandboxError::new(
        connect_kind(code),
        PROVIDER,
        code.to_string(),
        format!("exec: {message}"),
    ))
}

/// Connect's error codes, mapped onto the kinds the control plane retries on.
fn connect_kind(code: &str) -> ErrorKind {
    match code {
        "unauthenticated" | "permission_denied" => ErrorKind::Config,
        "not_found" => ErrorKind::NotFound,
        "already_exists" | "aborted" => ErrorKind::Conflict,
        "unavailable" | "resource_exhausted" => ErrorKind::Retryable,
        // Only a bounded `exec` can see this, and `exec` turns it into `timed_out` before the
        // kind is ever read; elsewhere a peer's deadline is not something to retry blindly.
        DEADLINE_EXCEEDED => ErrorKind::Fatal,
        _ => ErrorKind::Fatal,
    }
}

fn check_output_cap(total: usize) -> Result<(), SandboxError> {
    if total > EXEC_OUTPUT_CAP {
        return Err(fatal(
            "output_limit_exceeded",
            format!("exec output exceeded {EXEC_OUTPUT_CAP} bytes"),
        ));
    }
    Ok(())
}

/// The exit code envd spells inside a status line (`"exit status 7"`), if it carries one.
fn status_exit_code(status: &str) -> Option<i64> {
    let digits = status.strip_prefix("exit status ")?;
    digits.trim().parse().ok()
}

fn timed_out() -> ExecResult {
    ExecResult { exit_code: None, stdout: String::new(), stderr: String::new(), timed_out: true }
}

/// A run whose bound expired after it had already produced output: `timeout` killed it, and the
/// bytes captured up to that point are returned so a caller can diagnose the overrun.
fn timed_out_after(stdout: &[u8], stderr: &[u8]) -> ExecResult {
    ExecResult {
        exit_code: None,
        stdout: String::from_utf8_lossy(stdout).into_owned(),
        stderr: String::from_utf8_lossy(stderr).into_owned(),
        timed_out: true,
    }
}

// ---------------------------------------------------------------------------------------------
// Small helpers

fn field<'a>(value: &'a Value, names: &[&str]) -> Option<&'a Value> {
    names.iter().find_map(|name| value.get(*name))
}

fn decode_bytes(encoded: &str) -> Vec<u8> {
    // protojson carries `bytes` as base64; a payload that is not base64 is taken literally.
    BASE64.decode(encoded).unwrap_or_else(|_| encoded.as_bytes().to_vec())
}

fn parse_json<T: for<'de> Deserialize<'de>>(body: &str, call: &str) -> Result<T, SandboxError> {
    serde_json::from_str(body).map_err(|error| {
        fatal(
            "unexpected_body",
            format!("{call}: the API answered with an unexpected body: {error}"),
        )
    })
}

/// The `Idempotency-Key` header accepts `^[a-z0-9-]+$` and 1 to 255 characters only.
fn idempotency_key(raw: &str) -> String {
    let mut key: String = raw
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    key.truncate(255);
    if key.is_empty() {
        key.push_str("fx");
    }
    key
}

fn supported_domain(domain: &str) -> bool {
    matches!(
        domain,
        "e2b.app" | "e2b.dev" | "e2b.pro" | "e2b-staging.dev" | "e2b-tango.dev"
    )
}

fn format_mode(mode: u32) -> String {
    format!("{:04o}", mode & 0o7777)
}

/// `chmod` with the file name as an operand: the content is never part of this command.
fn chmod_command(path: &str, mode: u32) -> String {
    format!("chmod {} -- '{}'", format_mode(mode), path)
}

/// A path that can be an operand of `chmod` inside single quotes without escaping.
fn ensure_quotable(path: &str) -> Result<(), SandboxError> {
    if !path.starts_with('/') {
        return Err(config("path_not_absolute", "write_file needs an absolute path"));
    }
    let safe = path
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-' | '+' | '@' | ':' | ','));
    if !safe {
        return Err(config(
            "path_not_quotable",
            format!("write_file path must be printable ASCII without quotes or spaces: {path}"),
        ));
    }
    Ok(())
}

/// Percent-encode a query parameter value: unreserved characters survive, the rest is `%XX`.
fn query_encode(raw: &str) -> String {
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

/// Map an envd HTTP failure onto an [`ErrorKind`].
fn envd_status_error(status: u16, body: &str, call: &str) -> SandboxError {
    let parsed = serde_json::from_str::<Value>(body).ok();
    let detail = parsed
        .as_ref()
        .and_then(|value| value.get("message"))
        .and_then(Value::as_str)
        .unwrap_or(body)
        .trim()
        .to_string();
    let code = parsed
        .as_ref()
        .and_then(|value| value.get("code"))
        .and_then(Value::as_str)
        .unwrap_or("http_error")
        .to_string();
    let kind = match status {
        401 | 403 => ErrorKind::Config,
        404 => ErrorKind::NotFound,
        409 => ErrorKind::Conflict,
        // envd's 507 is "no space left on the sandbox disk": nothing a retry fixes.
        429 | 502 | 503 | 504 => ErrorKind::Retryable,
        _ => ErrorKind::Fatal,
    };
    SandboxError::new(kind, PROVIDER, code, format!("{call}: HTTP {status}: {detail}"))
}

/// Map an HTTP status onto a stable token for the failures E2B sends as `{"code":<number>}`.
///
/// Live 2026-11-22: a 401 is `{"code":401,"message":"authorization header is missing"}` and a
/// 404 is `{"code":404,"message":"Sandbox \"x\" doesn't exist ..."}`. Neither carries
/// `error_code`, which only some 4xx/5xx answers (idempotency, capacity) do, so without this the
/// provider_code of every ordinary failure degraded to the meaningless `http_error`.
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

/// Map a control-plane failure onto an [`ErrorKind`], keeping E2B's own `error_code`.
fn status_error(status: u16, body: &str, call: &str) -> SandboxError {
    let parsed = serde_json::from_str::<Value>(body).ok();
    let detail = parsed
        .as_ref()
        .and_then(|value| value.get("message"))
        .and_then(Value::as_str)
        .unwrap_or(body)
        .trim()
        .to_string();
    // `error_code` is the provider's own symbolic code and wins when present; the numeric `code`
    // is what the wire carries otherwise, and it is spelled through `status_code_name`.
    let code = parsed
        .as_ref()
        .and_then(|value| value.get("error_code"))
        .and_then(Value::as_str)
        .filter(|code| !code.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| status_code_name(status).to_string());
    let kind = match (status, code.as_str()) {
        (401 | 403, _) => ErrorKind::Config,
        (404, _) => ErrorKind::NotFound,
        (_, "idempotency_in_progress") => ErrorKind::Retryable,
        (_, "idempotency_request_mismatch") => ErrorKind::Conflict,
        (409, _) => ErrorKind::Conflict,
        // A node refusing a create this time is worth another try; a placement timeout too.
        (_, "sandbox_capacity_unavailable" | "sandbox_placement_timeout" | "sandbox_create_failed") => {
            ErrorKind::Retryable
        }
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
    // Never include the request in the message: the header carries the API key.
    SandboxError::new(kind, PROVIDER, "transport", format!("request failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CREATE_FIXTURE: &str =
        include_str!("../../../tests/fixtures/cloud_host/e2b/create_201.json");
    const GET_RUNNING_FIXTURE: &str =
        include_str!("../../../tests/fixtures/cloud_host/e2b/get_running.json");
    const GET_PAUSED_FIXTURE: &str =
        include_str!("../../../tests/fixtures/cloud_host/e2b/get_paused.json");
    const LIST_FIXTURE: &str =
        include_str!("../../../tests/fixtures/cloud_host/e2b/list_running_paused.json");
    const ERROR_404_FIXTURE: &str =
        include_str!("../../../tests/fixtures/cloud_host/e2b/error_404.json");
    const ERROR_401_FIXTURE: &str =
        include_str!("../../../tests/fixtures/cloud_host/e2b/error_401.json");
    const ERROR_IN_PROGRESS_FIXTURE: &str =
        include_str!("../../../tests/fixtures/cloud_host/e2b/error_409_idempotency_in_progress.json");
    const EXEC_STREAM_FIXTURE: &str =
        include_str!("../../../tests/fixtures/cloud_host/e2b/exec_stream.b64");

    #[test]
    fn create_answers_with_the_sandbox_id() {
        let created: Sandbox = parse_json(CREATE_FIXTURE, "create").expect("create fixture");
        assert_eq!(created.sandbox_id, "ijbv0q9onfnyqm60c1doo");
    }

    #[test]
    fn running_is_ready_and_paused_is_stopped() {
        let running: SandboxDetail = parse_json(GET_RUNNING_FIXTURE, "get").expect("running");
        assert_eq!(running.state.as_deref(), Some("running"));
        assert_eq!(running.envd_access_token.as_deref(), Some("envd-token-for-fixture"));
        assert_eq!(running.domain.as_deref(), Some("e2b.app"));

        let paused: SandboxDetail = parse_json(GET_PAUSED_FIXTURE, "get").expect("paused");
        assert_eq!(paused.state.as_deref(), Some("paused"));
    }

    #[test]
    fn list_rows_keep_their_state_name_and_start_time() {
        let page: Vec<ListedSandboxWire> = parse_json(LIST_FIXTURE, "list").expect("list");
        let listed: Vec<ListedSandbox> =
            page.into_iter().map(ListedSandboxWire::into_listed).collect();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].sandbox_id, "ijbv0q9onfnyqm60c1doo");
        assert_eq!(listed[0].state, SandboxState::Ready);
        assert_eq!(listed[0].name.as_deref(), Some("fx-e2b-1"));
        // 2026-11-21T09:30:00Z
        assert_eq!(listed[0].created_at, Some(1_795_253_400_000));
        assert_eq!(listed[1].state, SandboxState::Stopped);
        assert_eq!(listed[1].created_at, Some(1_795_257_000_000));
    }

    #[test]
    fn a_missing_sandbox_is_not_found_and_a_bad_key_is_config() {
        let not_found = status_error(404, ERROR_404_FIXTURE, "get");
        assert_eq!(not_found.kind, ErrorKind::NotFound);
        assert!(not_found.is_not_found());

        let unauthorised = status_error(401, ERROR_401_FIXTURE, "get");
        assert_eq!(unauthorised.kind, ErrorKind::Config);
        assert_eq!(unauthorised.provider_code, "unauthorized");
    }

    #[test]
    fn the_live_error_bodies_yield_a_meaningful_provider_code() {
        // Live 2026-11-22: neither 401 nor 404 carries `error_code`; both are
        // `{"code":<number>,"message":"..."}`. Reading only `error_code` used to leave every
        // real failure at `http_error`, which matches nothing in E2B's docs.
        assert_eq!(status_error(404, ERROR_404_FIXTURE, "get").provider_code, "not_found");
        assert_eq!(status_error(401, ERROR_401_FIXTURE, "get").provider_code, "unauthorized");
        assert!(status_error(404, ERROR_404_FIXTURE, "get").message.contains("doesn't exist"));

        // A symbolic `error_code`, when the provider does send one, still wins.
        assert_eq!(
            status_error(409, ERROR_IN_PROGRESS_FIXTURE, "create").provider_code,
            "idempotency_in_progress"
        );
        // An unknown non-JSON body names its status instead of degrading to `http_error`.
        assert_eq!(status_error(400, "not json", "create").provider_code, "bad_request");
        assert_eq!(status_error(503, "", "list").provider_code, "unavailable");
    }

    /// A local server standing in for both the control plane and envd, recording what the
    /// adapter actually sends. `reply` is the whole Connect stream envd answers with.
    struct FakeE2b {
        base: String,
        seen: std::sync::Arc<tokio::sync::Mutex<Vec<Recorded>>>,
    }

    #[derive(Clone, Debug)]
    struct Recorded {
        method: String,
        uri: String,
        body: String,
        headers: Vec<(String, String)>,
    }

    impl Recorded {
        fn header(&self, name: &str) -> Option<&str> {
            self.headers.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
        }
    }

    impl FakeE2b {
        async fn start(reply: &'static [u8], upload_status: axum::http::StatusCode) -> Self {
            use axum::extract::State;
            use axum::routing::{get, post};

            type Seen = std::sync::Arc<tokio::sync::Mutex<Vec<Recorded>>>;
            let seen: Seen = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));

            async fn record(
                seen: &Seen,
                method: &str,
                uri: &str,
                headers: &axum::http::HeaderMap,
                body: &[u8],
            ) {
                let headers = headers
                    .iter()
                    .map(|(name, value)| {
                        (name.as_str().to_string(), value.to_str().unwrap_or("<binary>").to_string())
                    })
                    .collect();
                seen.lock().await.push(Recorded {
                    method: method.to_string(),
                    uri: uri.to_string(),
                    body: String::from_utf8_lossy(body).into_owned(),
                    headers,
                });
            }

            let app = axum::Router::new()
                .route(
                    "/sandboxes/{id}",
                    get(|State(seen): State<Seen>, uri: axum::http::Uri, headers: axum::http::HeaderMap| async move {
                        record(&seen, "GET", &uri.to_string(), &headers, b"").await;
                        (
                            axum::http::StatusCode::OK,
                            r#"{"sandboxID":"fx-sbx-1","state":"running","envdAccessToken":"envd-token","domain":"e2b.app"}"#,
                        )
                    }),
                )
                .route(
                    "/files",
                    post(move |State(seen): State<Seen>, uri: axum::http::Uri, headers: axum::http::HeaderMap, body: axum::body::Bytes| async move {
                        record(&seen, "POST", &uri.to_string(), &headers, &body).await;
                        (upload_status, String::new())
                    }),
                )
                .route(
                    "/process.Process/Start",
                    post(move |State(seen): State<Seen>, uri: axum::http::Uri, headers: axum::http::HeaderMap, body: axum::body::Bytes| async move {
                        record(&seen, "POST", &uri.to_string(), &headers, &body).await;
                        (axum::http::StatusCode::OK, reply)
                    }),
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

    fn provider_for(base: &str) -> E2bProvider {
        let mut provider = E2bProvider::new(Some("account-key".into()), "base".into(), "e2b.app".into());
        provider.api_base = base.to_string();
        provider.envd_base = Some(base.to_string());
        provider
    }

    /// The Connect payload inside the one envelope a request body carries.
    fn envelope_payload(body: &str) -> Value {
        let raw: Vec<u8> = body.bytes().collect();
        assert_eq!(raw[0], 0, "a request is one uncompressed envelope");
        let len = u32::from_be_bytes([raw[1], raw[2], raw[3], raw[4]]) as usize;
        serde_json::from_slice(&raw[5..5 + len]).expect("the envelope payload is JSON")
    }

    /// A complete Connect stream: one stdout frame, one end frame, then the end-of-stream
    /// envelope. Exactly the shape live envd answered with on 2026-11-22.
    fn exec_stream(exit_code: i64, stdout: &str) -> Vec<u8> {
        let chunk = json!({"event": {"data": {"stdout": BASE64.encode(stdout)}}}).to_string();
        let end = json!({
            "event": {"end": {"exitCode": exit_code, "exited": true, "status": format!("exit status {exit_code}")}}
        })
        .to_string();
        let mut out = Vec::new();
        out.extend_from_slice(&encode_envelope(0, chunk.as_bytes()));
        out.extend_from_slice(&encode_envelope(0, end.as_bytes()));
        out.extend_from_slice(&encode_envelope(0x02, b"{}"));
        out
    }

    #[tokio::test]
    async fn exec_bounds_the_command_inside_the_sandbox_and_maps_the_expiry() {
        let reply = Box::leak(exec_stream(124, "ab").into_boxed_slice());
        let fake = FakeE2b::start(reply, axum::http::StatusCode::OK).await;
        let provider = provider_for(&fake.base);

        let result = provider.exec("fx-sbx-1", "sleep 120", 3).await.expect("exec");

        // 124 is coreutils `timeout`'s own code for an expired bound: a timed-out run, and the
        // output captured before the overrun is kept.
        assert!(result.timed_out, "exit {:?}", result.exit_code);
        assert_eq!(result.exit_code, None);
        assert_eq!(result.stdout, "ab");

        let calls = fake.calls().await;
        let start = calls.iter().find(|call| call.uri == "/process.Process/Start").expect("a start call");

        // The bound is enforced in the sandbox, not by dropping the client future.
        let payload = envelope_payload(&start.body);
        let args = payload["process"]["args"].as_array().expect("args");
        assert_eq!(args[0], "-c");
        let script = args[1].as_str().expect("script");
        assert!(script.contains("timeout -s TERM -k 5 \"$1\""), "{script}");
        assert!(script.contains("/bin/sh -c \"$2\""), "{script}");
        // `-k`'s SIGKILL exits 137, so the wrapper normalises it to 124 once the bound is reached;
        // otherwise a TERM-ignoring command is reported as a signal death, not a timeout.
        assert!(script.contains("[ \"$status\" = 137 ]"), "{script}");
        assert!(script.contains("exit 124"), "{script}");
        assert!(script.contains("exec /bin/sh -c \"$2\""), "no-coreutils fallback: {script}");
        // The caller's command is an argument, never spliced into the script; `$0` is the wrapper's
        // own name, so `$1` and `$2` are the bound and the command.
        assert_eq!(args[2], "fx-exec");
        assert_eq!(args[3], "3");
        assert_eq!(args[4], "sleep 120");

        // envd's own deadline is set past the in-sandbox bound so the end event still arrives,
        // and past `timeout -k 5`'s SIGKILL (bound + 5 s), which is what a TERM-ignoring child
        // runs into. Anything at or below that instant loses the end event.
        let deadline: u64 = start
            .header("connect-timeout-ms")
            .expect("connect-timeout-ms")
            .parse()
            .expect("a number of ms");
        assert!(deadline >= 18_000, "envd must outlive the SIGKILL by a wide margin: {deadline}");
        assert!(payload["stdin"] == json!(false));
    }

    #[tokio::test]
    async fn a_stream_that_dies_on_envds_deadline_is_a_timeout_not_an_error() {
        // Live 2026-11-22 at a 3 s bound: `trap '' TERM; sleep 120` ignores the wrapper's SIGTERM,
        // `-k 5` SIGKILLs it at bound + 5, and the stream ends with no end event and this exact
        // payload. Mapping the code back to a Retryable error told the caller to retry a command
        // that had already run out of time, which can loop forever.
        let chunk = json!({"event": {"data": {"stdout": BASE64.encode("partial")}}}).to_string();
        let mut reply = Vec::new();
        reply.extend_from_slice(&encode_envelope(0, chunk.as_bytes()));
        reply.extend_from_slice(&encode_envelope(
            0x02,
            br#"{"error":{"code":"deadline_exceeded","message":"context deadline exceeded"}}"#,
        ));
        let reply = Box::leak(reply.into_boxed_slice());
        let fake = FakeE2b::start(reply, axum::http::StatusCode::OK).await;
        let provider = provider_for(&fake.base);

        let result = provider
            .exec("fx-sbx-1", "trap '' TERM; sleep 120", 3)
            .await
            .expect("a bound that expired is a result, not an error");

        assert!(result.timed_out, "the bound expired; the caller must see it: {result:?}");
        assert_eq!(result.exit_code, None);
        assert_eq!(result.stdout, "partial", "output captured before the overrun is kept");
    }

    #[test]
    fn only_an_expired_bound_is_rewritten_as_a_timeout() {
        // The rewrite in `exec` keys on this one code, so every other Connect code keeps its kind.
        assert_eq!(
            stream_error(br#"{"error":{"code":"deadline_exceeded"}}"#)
                .expect("an error")
                .provider_code,
            DEADLINE_EXCEEDED
        );
        assert_eq!(
            stream_error(br#"{"error":{"code":"not_found"}}"#).expect("an error").kind,
            ErrorKind::NotFound
        );
        assert_eq!(
            stream_error(br#"{"error":{"code":"unavailable"}}"#).expect("an error").kind,
            ErrorKind::Retryable
        );
    }

    #[tokio::test]
    async fn an_ordinary_non_zero_exit_is_not_a_timeout() {
        let reply = Box::leak(exec_stream(7, "hi\n").into_boxed_slice());
        let fake = FakeE2b::start(reply, axum::http::StatusCode::OK).await;
        let provider = provider_for(&fake.base);

        let result = provider.exec("fx-sbx-1", "echo hi; exit 7", 30).await.expect("exec");
        assert!(!result.timed_out);
        assert_eq!(result.exit_code, Some(7));
        assert_eq!(result.stdout, "hi\n");
    }

    #[tokio::test]
    async fn write_file_never_sends_the_account_key_to_the_sandbox() {
        // envd authenticates with the sandbox-scoped token only. Live 2026-11-22 the upload path
        // also carried `X-API-Key: <account key>`, which the SDK never does.
        let reply = Box::leak(exec_stream(0, "").into_boxed_slice());
        let fake = FakeE2b::start(reply, axum::http::StatusCode::OK).await;
        let provider = provider_for(&fake.base);

        provider
            .write_file("fx-sbx-1", "/home/user/.fx-secret", vec![1u8; 16], 0o600)
            .await
            .expect("write_file");

        let calls = fake.calls().await;
        let control = calls
            .iter()
            .find(|call| call.method == "GET" && call.uri.starts_with("/sandboxes/"))
            .expect("detail");
        // The control plane still gets the account key; that is its own credential.
        assert_eq!(control.header("x-api-key"), Some("account-key"));

        let upload = calls
            .iter()
            .find(|call| call.method == "POST" && call.uri.starts_with("/files"))
            .expect("upload");
        assert_eq!(upload.header("x-api-key"), None, "the account key must not reach envd");
        assert_eq!(upload.header("x-access-token"), Some("envd-token"));
        assert_eq!(upload.header("e2b-sandbox-id"), Some("fx-sbx-1"));

        // The mode is applied by a follow-up chmod, which also gets only the sandbox token.
        let chmod = calls
            .iter()
            .find(|call| call.method == "POST" && call.uri == "/process.Process/Start")
            .expect("the chmod exec");
        assert_eq!(chmod.header("x-api-key"), None);
        assert_eq!(chmod.header("x-access-token"), Some("envd-token"));
        let payload = envelope_payload(&chmod.body);
        let args = payload["process"]["args"].as_array().expect("args");
        assert_eq!(args[4], "chmod 0600 -- '/home/user/.fx-secret'");
    }

    #[test]
    fn an_idempotent_create_in_progress_is_retryable() {
        let error = status_error(409, ERROR_IN_PROGRESS_FIXTURE, "create");
        assert_eq!(error.kind, ErrorKind::Retryable);
        assert_eq!(error.provider_code, "idempotency_in_progress");
    }

    #[test]
    fn a_capacity_refusal_from_a_node_is_retryable() {
        let body = r#"{"code":500,"error_code":"sandbox_capacity_unavailable","message":"no node"}"#;
        assert_eq!(status_error(500, body, "create").kind, ErrorKind::Retryable);
        let body = r#"{"code":400,"error_code":"idempotency_request_mismatch","message":"differs"}"#;
        assert_eq!(status_error(409, body, "create").kind, ErrorKind::Conflict);
    }

    #[test]
    fn the_idempotency_key_matches_the_documented_pattern() {
        assert_eq!(idempotency_key("fx-E2B-1_1755"), "fx-e2b-1-1755");
        assert_eq!(idempotency_key(""), "fx");
        let long = idempotency_key(&"a".repeat(400));
        assert_eq!(long.len(), 255);
        assert!(long.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'));
    }

    #[test]
    fn envelopes_split_across_chunk_boundaries() {
        let frames = encode_envelope(0, br#"{"event":{"start":{"pid":7}}}"#)
            .into_iter()
            .chain(encode_envelope(0x02, b"{}"))
            .collect::<Vec<u8>>();
        let mut decoder = EnvelopeDecoder::default();
        // One byte at a time: the decoder must not need a message-aligned chunk.
        let mut seen = Vec::new();
        for byte in &frames {
            decoder.push(&[*byte]);
            if let Some(frame) = decoder.next_frame() {
                seen.push(frame);
            }
        }
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].0, 0);
        assert_eq!(seen[0].1, br#"{"event":{"start":{"pid":7}}}"#.to_vec());
        assert_eq!(seen[1].0, 0x02);
    }

    #[test]
    fn the_recorded_exec_stream_parses_into_one_bounded_result() {
        let raw = BASE64.decode(EXEC_STREAM_FIXTURE.trim()).expect("fixture is base64");
        let mut decoder = EnvelopeDecoder::default();
        decoder.push(&raw);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut exit_code = None;
        let mut ended = false;
        while let Some((flags, payload)) = decoder.next_frame() {
            if flags & ENVELOPE_END_STREAM != 0 {
                assert!(stream_error(&payload).is_none());
                break;
            }
            match parse_event(&payload).expect("event") {
                Event::Stdout(bytes) => stdout.extend_from_slice(&bytes),
                Event::Stderr(bytes) => stderr.extend_from_slice(&bytes),
                Event::End { exit_code: code, error } => {
                    exit_code = code;
                    assert!(error.is_none());
                    ended = true;
                }
                Event::Start | Event::KeepAlive | Event::Unknown => {}
            }
        }
        assert!(ended, "the stream must end with a process end event");
        assert_eq!(exit_code, Some(0));
        assert_eq!(String::from_utf8_lossy(&stdout), "x86_64\n");
        assert_eq!(String::from_utf8_lossy(&stderr), "");
    }

    #[test]
    fn the_recorded_stream_ends_without_a_stream_error() {
        let raw = BASE64.decode(EXEC_STREAM_FIXTURE.trim()).expect("fixture is base64");
        let mut decoder = EnvelopeDecoder::default();
        decoder.push(&raw);
        let mut seen_end = false;
        while let Some((flags, payload)) = decoder.next_frame() {
            if flags & ENVELOPE_END_STREAM != 0 {
                assert!(stream_error(&payload).is_none());
                return;
            }
            seen_end |= matches!(parse_event(&payload).expect("event"), Event::End { .. });
        }
        assert!(seen_end, "a stream without an end event and without a closing envelope is a bug");
    }

    #[test]
    fn an_end_event_without_exited_has_no_exit_code() {
        let event = parse_event(br#"{"event":{"end":{"exited":false,"status":"signal: killed"}}}"#)
            .expect("event");
        match event {
            Event::End { exit_code, .. } => assert_eq!(exit_code, None),
            _ => panic!("expected an end event"),
        }
    }

    #[test]
    fn a_zero_exit_code_is_read_from_the_status_line_the_live_stream_sends() {
        // envd 0.2.9 omits `exitCode` for a successful process and says so only in `status`.
        let event = parse_event(br#"{"event":{"end":{"exited":true,"status":"exit status 0"}}}"#)
            .expect("event");
        match event {
            Event::End { exit_code, error } => {
                assert_eq!(exit_code, Some(0));
                assert_eq!(error, None);
            }
            _ => panic!("expected an end event"),
        }
    }

    #[test]
    fn a_non_zero_exit_code_is_read_and_its_repeated_error_is_not_stderr() {
        // The live `exit 7` frame carries exitCode, status and error all saying the same thing.
        let event = parse_event(
            br#"{"event":{"end":{"exitCode":7,"exited":true,"status":"exit status 7","error":"exit status 7"}}}"#,
        )
        .expect("event");
        match event {
            Event::End { exit_code, error } => {
                assert_eq!(exit_code, Some(7));
                assert_eq!(error, None, "an exit status line is not the program's stderr");
            }
            _ => panic!("expected an end event"),
        }
    }

    #[test]
    fn a_real_error_message_survives_the_status_filter() {
        let event = parse_event(
            br#"{"event":{"end":{"exitCode":1,"exited":true,"status":"exit status 1","error":"command not found"}}}"#,
        )
        .expect("event");
        match event {
            Event::End { exit_code, error } => {
                assert_eq!(exit_code, Some(1));
                assert_eq!(error.as_deref(), Some("command not found"));
            }
            _ => panic!("expected an end event"),
        }
    }

    #[test]
    fn a_stream_error_maps_its_connect_code() {
        let payload = br#"{"error":{"code":"not_found","message":"no such process"}}"#;
        let error = stream_error(payload).expect("an error");
        assert_eq!(error.kind, ErrorKind::NotFound);
        assert_eq!(error.provider_code, "not_found");
        assert!(stream_error(b"{}").is_none());
    }

    #[test]
    fn exec_output_over_the_cap_is_fatal() {
        let error = check_output_cap(EXEC_OUTPUT_CAP + 1).expect_err("over the cap");
        assert_eq!(error.kind, ErrorKind::Fatal);
        assert_eq!(error.provider_code, "output_limit_exceeded");
    }

    #[test]
    fn write_file_paths_must_be_absolute_and_quotable() {
        assert!(ensure_quotable("/home/user/.fx-test").is_ok());
        assert_eq!(
            ensure_quotable("relative/.fx-test").expect_err("relative").provider_code,
            "path_not_absolute"
        );
        assert_eq!(
            ensure_quotable("/home/user/it's").expect_err("quote").provider_code,
            "path_not_quotable"
        );
        assert_eq!(
            ensure_quotable("/home/user/a b").expect_err("space").provider_code,
            "path_not_quotable"
        );
    }

    #[test]
    fn the_chmod_command_carries_only_the_name() {
        assert_eq!(chmod_command("/home/user/.fx-test", 0o600), "chmod 0600 -- '/home/user/.fx-test'");
        assert_eq!(format_mode(0o600), "0600");
        assert_eq!(format_mode(0o644), "0644");
    }

    #[test]
    fn the_file_upload_query_encodes_the_path() {
        assert_eq!(query_encode("/home/user/.fx-test"), "/home/user/.fx-test");
        assert_eq!(query_encode("/home/user/a b"), "/home/user/a%20b");
        assert_eq!(query_encode("/home/user/a&b"), "/home/user/a%26b");
    }

    #[test]
    fn the_ttl_is_clamped_to_the_hobby_cap() {
        assert_eq!(E2bProvider::ttl_seconds(None), DEFAULT_TTL_SECONDS);
        assert_eq!(E2bProvider::ttl_seconds(Some(1)), 60);
        assert_eq!(E2bProvider::ttl_seconds(Some(7_200)), MAX_TTL_SECONDS);
        assert_eq!(E2bProvider::ttl_seconds(Some(900)), 900);
    }

    #[test]
    fn capabilities_carry_the_one_hour_cap_and_the_relay() {
        let provider = E2bProvider::without_token();
        let capabilities = provider.capabilities();
        assert_eq!(capabilities.max_ttl_seconds, Some(3_600));
        assert_eq!(capabilities.transport, Transport::Relay);
        assert!(capabilities.native_auto_stop);
    }

    #[tokio::test]
    async fn a_call_without_a_key_reports_config() {
        let provider = E2bProvider::without_token();
        let error = provider.get("ijbv0q9onfnyqm60c1doo").await.expect_err("no key");
        assert_eq!(error.kind, ErrorKind::Config);
        assert_eq!(error.provider_code, "missing_token");
    }

    #[test]
    fn envd_uses_the_shared_front_door_on_e2b_domains_and_a_per_sandbox_host_otherwise() {
        let provider = E2bProvider::without_token();
        assert_eq!(provider.envd_base("sbx1", None), "https://sandbox.e2b.app");
        assert_eq!(provider.envd_base("sbx1", Some("e2b.app")), "https://sandbox.e2b.app");
        assert_eq!(
            provider.envd_base("sbx1", Some("example.internal")),
            "https://49983-sbx1.example.internal"
        );
    }

    // -----------------------------------------------------------------------------------------
    // Live probes against the real API, driving the real adapter. `#[ignore]` so the unit run
    // never makes a billable call; run with `--ignored --test-threads=1` and a key in
    // `E2B_API_KEY`. Each test creates one sandbox and deletes it and forbids leftovers.

    /// A name unique to this run, so a leftover is traceable and `create` never collides.
    fn live_name(tag: &str) -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        format!("fx-t10d-{tag}-{nanos}")
    }

    /// Finishing state: THIS sandbox is no longer listed. Other live tests in the same run have
    /// their own sandboxes, so the rule is "my id is gone", not "the account is empty" - asserting
    /// the latter fails on a peer's row and hides which sandbox actually leaked.
    fn assert_gone(rows: &[ListedSandbox], id: &str) {
        let names: Vec<&str> = rows.iter().map(|row| row.sandbox_id.as_str()).collect();
        assert!(!names.contains(&id), "{id} is still listed: {names:?}");
    }

    /// Create, run `probe`, delete - and fail if THIS sandbox is still listed afterwards.
    async fn with_live_sandbox<F, Fut>(tag: &str, probe: F)
    where
        F: FnOnce(E2bProvider, String) -> Fut,
        Fut: std::future::Future<Output = ()>,
    {
        let provider = E2bProvider::from_env();
        let name = live_name(tag);
        let id = provider
            .create(CreateSandbox {
                idempotency_key: name.clone(),
                name: name.clone(),
                ttl_seconds: Some(120),
                env: std::collections::BTreeMap::new(),
            })
            .await
            .expect("create");
        println!("created {id} ({name})");

        // `create` returns before envd answers.
        let mut ready = false;
        for _ in 0..120 {
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
        let provider = E2bProvider::from_env();
        let delete = provider.delete(&id).await;
        println!("deleted {id}: {delete:?}");
        // Delete is not synchronous everywhere: poll until this id stops being listed, bounded.
        let mut rows = provider.list().await.expect("list");
        for _ in 0..60 {
            if !rows.iter().any(|row| row.sandbox_id == id) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            rows = provider.list().await.expect("list");
        }
        println!("rows still listed: {}", rows.len());
        assert!(delete.is_ok(), "delete failed: {delete:?}");
        assert_gone(&rows, &id);
        if let Err(panic) = outcome {
            std::panic::resume_unwind(panic);
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "live: creates a billable E2B sandbox; needs E2B_API_KEY"]
    async fn live_a_timed_out_exec_leaves_no_process_behind() {
        with_live_sandbox("timeout", |provider, id| async move {
            let started = std::time::Instant::now();
            let result = provider.exec(&id, "sleep 120; echo LATE", 3).await.expect("exec");
            let elapsed = started.elapsed();
            println!(
                "timed_out={} exit={:?} stdout={:?} stderr={:?} in {elapsed:?}",
                result.timed_out, result.exit_code, result.stdout, result.stderr
            );
            assert!(result.timed_out, "the 3 s bound did not expire");
            assert_eq!(result.exit_code, None);
            assert!(!result.stdout.contains("LATE"), "the command outlived its bound");

            // The bound must be enforced inside the sandbox, so nothing is left running. Poll a
            // little: the kill is asynchronous to the end event that ended the stream.
            let mut alive = usize::MAX;
            for _ in 0..24 {
                let ps = provider
                    .exec(&id, "ps -eo args | grep -cE '^sleep 120'", 20)
                    .await
                    .expect("ps");
                alive = ps.stdout.trim().parse().unwrap_or(usize::MAX);
                if alive == 0 {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
            assert_eq!(alive, 0, "a process survived the timeout");
        })
        .await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "live: creates a billable E2B sandbox; needs E2B_API_KEY"]
    async fn live_a_sigterm_ignoring_command_is_still_reported_as_a_timeout() {
        with_live_sandbox("kill", |provider, id| async move {
            // `trap '' TERM` makes the wrapper's SIGTERM a no-op, so the only kill is `-k 5`'s
            // SIGKILL. That lands at bound + 5 and used to collide with envd's deadline, ending
            // the stream with `deadline_exceeded` and turning a timeout into Err(Retryable).
            let started = std::time::Instant::now();
            let result = provider
                .exec(&id, "echo START; trap '' TERM; sleep 120", 3)
                .await
                .expect("a bound that expired is a result, not an error");
            let elapsed = started.elapsed();
            println!(
                "timed_out={} exit={:?} stdout={:?} in {elapsed:?}",
                result.timed_out, result.exit_code, result.stdout
            );
            assert!(result.timed_out, "a TERM-ignoring command must still report a timeout");
            assert_eq!(result.exit_code, None);
            assert!(result.stdout.contains("START"), "output before the overrun is kept");
            assert!(
                elapsed < std::time::Duration::from_secs(30),
                "the run must stop near its bound, took {elapsed:?}"
            );

            // SIGKILL is unconditional, so nothing may survive it.
            let mut alive = usize::MAX;
            for _ in 0..24 {
                let ps = provider
                    .exec(&id, "ps -eo args | grep -cE '^sleep 120'", 20)
                    .await
                    .expect("ps");
                alive = ps.stdout.trim().parse().unwrap_or(usize::MAX);
                if alive == 0 {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
            assert_eq!(alive, 0, "a TERM-ignoring process survived the SIGKILL");
        })
        .await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "live: creates a billable E2B sandbox; needs E2B_API_KEY"]
    async fn live_write_file_keeps_the_mode_on_an_existing_file() {
        with_live_sandbox("mode", |provider, id| async move {
            // The normal second-boot case: the secret path already exists with a lax mode.
            let seed = provider
                .exec(
                    &id,
                    "printf 'x' > /home/user/.fx-mode && chmod 0666 /home/user/.fx-mode && stat -c '%a' /home/user/.fx-mode",
                    30,
                )
                .await
                .expect("seed");
            assert_eq!(seed.stdout.trim(), "666", "seed: {seed:?}");

            provider
                .write_file(&id, "/home/user/.fx-mode", vec![9u8; 32], 0o600)
                .await
                .expect("write_file");

            let stat = provider
                .exec(&id, "stat -c '%a %s' /home/user/.fx-mode", 30)
                .await
                .expect("stat");
            println!("after write_file: {:?}", stat.stdout.trim());
            assert_eq!(stat.stdout.trim(), "600 32", "the old mode survived the write");
        })
        .await;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "live: creates a billable E2B sandbox; needs E2B_API_KEY"]
    async fn live_exec_bounds_exit_codes_and_output() {
        with_live_sandbox("codes", |provider, id| async move {
            let seven = provider.exec(&id, "echo hi; exit 7", 30).await.expect("exit 7");
            assert_eq!(seven.exit_code, Some(7));
            assert!(!seven.timed_out);
            assert_eq!(seven.stdout.trim(), "hi");

            // A signal death is not a timeout. Under the wrapper a `kill -KILL $$` reaches the
            // inner `/bin/sh` as rc 137, which is not 124, so it stays an ordinary signal exit and
            // its code is reported untouched: the caller sees the death, not a fabricated timeout.
            let killed = provider.exec(&id, "kill -KILL $$", 30).await.expect("sigkill");
            assert!(!killed.timed_out, "a signal death is not a timeout");
            assert_eq!(killed.exit_code, Some(137));

            // 6 MB over a 4 MiB cap is a Fatal, not a truncated success.
            let big = provider
                .exec(&id, "head -c 6000000 /dev/zero | tr '\\0' 'a'", 120)
                .await;
            let error = big.expect_err("over the cap");
            assert_eq!(error.provider_code, "output_limit_exceeded");

            // The account list is a live read that must at least contain the caller's own live
            // sandbox (its `limit=100` page is what LIST_PAGE_LIMIT asks for). Asserting the list
            // is EMPTY here was wrong: the helper's own sandbox is running at this moment.
            let rows = provider.list().await.expect("list");
            let ids: Vec<&str> = rows.iter().map(|row| row.sandbox_id.as_str()).collect();
            assert!(ids.contains(&id.as_str()), "the list did not surface {id}: {ids:?}");
        })
        .await;
    }

    /// A syntactically valid id that does not exist: the platform answers 404 for it. A malformed
    /// id is rejected earlier with 400 `Invalid sandbox ID`, a different (also correct) path.
    const ABSENT_ID: &str = "in00000000000000000000";

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "live: needs E2B_API_KEY"]
    async fn live_error_bodies_map_to_their_real_codes() {
        let provider = E2bProvider::from_env();
        let missing = provider.get(ABSENT_ID).await.expect_err("404");
        println!("404 -> {} / {}", missing.provider_code, missing.message);
        assert_eq!(missing.kind, ErrorKind::NotFound);
        assert_eq!(missing.provider_code, "not_found");

        let bare = E2bProvider::new(Some("e2b_invalid".into()), "base".into(), "e2b.app".into());
        let refused = bare.get(ABSENT_ID).await.expect_err("401");
        println!("401 -> {} / {}", refused.provider_code, refused.message);
        assert_eq!(refused.kind, ErrorKind::Config);
        assert_eq!(refused.provider_code, "unauthorized");
    }
}
