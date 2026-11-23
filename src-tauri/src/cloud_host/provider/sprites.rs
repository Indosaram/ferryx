//! Fly Sprites: a persistent Ubuntu VM on `https://api.sprites.dev`, driven through the official
//! `sprite` CLI's authenticated passthrough (`sprite api`) over plain HTTP - never the WebSocket
//! console, never `sprite exec`.
//!
//! # Why the CLI carries the request
//!
//! The CLI keeps its token **encrypted** in the OS keyring (`~/.sprites/users/<id>.token.enc`),
//! and `~/.config/compute-secrets.env` has no Sprites entry, so no plaintext token exists on this
//! machine to put in an `Authorization` header. `sprite api <path> -- <curl args>` performs the
//! authenticated call with the CLI's own login and hands the HTTP response back unchanged, so the
//! REST contract below is preserved exactly. When a token *is* exported the CLI honours
//! `SPRITE_TOKEN`, so the same code path works in CI with no change.
//!
//! # Endpoints
//!
//! Recorded 2026-11-21 from `https://api.sprites.dev/openapi.json` (Sprites API 0.1.12,
//! `x-sprite-env-version: 0.0.5`) and verified live through `sprite api`.
//!
//! | Call | Request | Response |
//! | --- | --- | --- |
//! | create | `POST /v1/sprites` `{name}` | `201` sprite; `409` when the name is taken |
//! | get | `GET /v1/sprites/{name}` | `200` sprite; `404 {"error":"sprite not found"}` |
//! | list | `GET /v1/sprites?max_results=&continuation_token=` | `{sprites, has_more, next_continuation_token, ...}` |
//! | delete | `DELETE /v1/sprites/{name}` | `204`; `404` when gone |
//! | exec | `POST /v1/sprites/{name}/exec?cmd=<repeated>` | `200 application/octet-stream`, framed (below) |
//! | write file | `PUT /v1/sprites/{name}/fs/write?path=&workingDir=&mode=&mkdir=` body = raw bytes | `{path,size,mode}` |
//! | list sessions | `GET /v1/sprites/{name}/exec` | `{sessions, count}` |
//! | kill session | `POST /v1/sprites/{name}/exec/{session_id}/kill` | `200 application/x-ndjson` |
//!
//! `sprite api` prints the response body to stdout and returns 0 even for an HTTP error, so this
//! adapter asks for `-o <file>` (body) plus `-w '%{http_code}'` (status on stdout) and reads the
//! two apart. A request body travels on **stdin** (`--data-binary @-`) and never on the command
//! line, so a credential cannot appear in `ps` output or in a log; the response file is created
//! 0600 and removed before the call returns, and the stdin upload is inside the call's own
//! deadline so a CLI that never drains the pipe cannot outlive it.
//!
//! # `CreateSandbox.env` is refused, not dropped
//!
//! `POST /v1/sprites` takes a name and `url_settings` only: there is no create-time env field, and
//! a Sprite's environment cannot be mutated afterwards either. Rather than silently lose the
//! caller's variables, `create` answers `Config` + `env_unsupported` - the same refusal
//! codespaces, instabox and tensorlake make - so a caller that needs env learns it at create time
//! instead of discovering an empty environment on the box.
//!
//! # A timed-out exec leaves nothing to kill
//!
//! `exec` honours the caller's `timeout_seconds` (plus [`EXEC_TIMEOUT_MARGIN`] for the CLI's own
//! startup) rather than rounding up to a generous ceiling. The one-shot `POST /exec` endpoint has
//! no server-side deadline and no session id: the response carries no session header, `cmd` has no
//! `timeout` parameter, and `GET /exec` reported `{"sessions":[],"count":0}` while a 60-second
//! `POST /exec` was still running (verified live 2026-11-22). So there is no `.../exec/{id}/kill`
//! to issue for a timed-out one-shot exec - the `kill` endpoint only addresses TTY sessions, which
//! `stop` ends. The timed-out CLI is killed on the client side; the command on the box finishes on
//! its own.
//!
//! # The exec body is a framed stream, not text
//!
//! Verified live: `printf OUT; printf ERR >&2; exit 7` answers the ten bytes
//! `02 45 52 52 01 4f 55 54 03 07`, i.e. `0x02` opens a stderr run, `0x01` opens a stdout run,
//! `0x03` opens the exit-code run, and a run continues until the next `0x01`/`0x02`/`0x03` byte.
//! A 2000-byte output arrived as one `0x01` run, and a payload containing the raw bytes
//! `01 02 03` arrived as `01 01 02 03 03 00` - the payload's markers were consumed as markers, so
//! a payload byte equal to `0x01`/`0x02`/`0x03` is lost. That is a protocol limitation, not a
//! parser bug: an exec that prints those bytes cannot be read back through this endpoint. Nothing
//! ferryx sends through exec is binary; files go through `fs/write`, which is byte-exact.
//!
//! The exit code is vulnerable to the same collision: `exit 3` answers `01 OUT 02 ERR 03 03`, so a
//! parser that reads "the byte after `0x03`" sees a marker and reports no code (observed live).
//! The exit frame is always written last, so the code is taken from the **final byte** of the
//! stream, which works for every code including 1, 2 and 3.
//!
//! # Lifecycle quirks
//!
//! - There is no stop/pause endpoint: a Sprite suspends itself ~30 s after its last activity
//!   (warm, then cold), and the filesystem always survives. `stop` therefore ends the exec
//!   sessions that hold it awake, and `resume` is a no-op beyond proving the Sprite still exists.
//! - `ttl_required = false`, `max_ttl_seconds = None`: the platform owns idling, and
//!   `native_auto_stop = true` because it pauses without anyone calling anything.
//! - Free credit is $1 and a Sprite stops without a card once it is spent, so the live QA for
//!   this provider is a single create/exec/delete cycle.
//! - A missing `sprite` binary or a signed-out CLI is a `Config` error naming the fix, which is
//!   the documented "no key" state for this provider.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use super::{
    Capabilities, CreateSandbox, ErrorKind, ExecResult, ListedSandbox, SandboxError, SandboxInfo,
    SandboxProvider, SandboxState, Transport,
};

const PROVIDER: &str = "sprites";
/// `fs/write` refuses a body over 16 MiB.
const WRITE_FILE_CAP: usize = 16 * 1024 * 1024;
/// Must NOT have: one bounded response, no unbounded stream.
const EXEC_OUTPUT_CAP: usize = 4 * 1024 * 1024;
/// Upper bound on pages walked by the orphan scan.
const MAX_LIST_PAGES: usize = 20;
/// `sprite api` sends no request of its own; each call gets its own generous ceiling.
const CLI_TIMEOUT: Duration = Duration::from_secs(180);
/// How much longer than the caller's `timeout_seconds` an exec may take before it is abandoned:
/// enough for the CLI to start and for the response to arrive, small enough that the caller's
/// bound is real.
const EXEC_TIMEOUT_MARGIN: Duration = Duration::from_secs(5);

/// Frame markers of the exec response body.
const FRAME_STDOUT: u8 = 0x01;
const FRAME_STDERR: u8 = 0x02;
const FRAME_EXIT: u8 = 0x03;

/// A request body and the content type that describes it.
enum Body<'a> {
    Json(&'a str),
    Bytes(&'a [u8]),
}

impl Body<'_> {
    fn content_type(&self) -> &'static str {
        match self {
            Body::Json(_) => "application/json",
            Body::Bytes(_) => "application/octet-stream",
        }
    }

    fn bytes(&self) -> &[u8] {
        match self {
            Body::Json(text) => text.as_bytes(),
            Body::Bytes(bytes) => bytes,
        }
    }
}

pub struct SpritesProvider {
    /// The `sprite` binary. `from_env` reads `SPRITE_CLI` (default `sprite` on PATH).
    cli: PathBuf,
    /// Bound on a control call. [`CLI_TIMEOUT`] outside tests, where a short bound is what makes
    /// "the upload is inside the deadline" an assertion instead of a three-minute wait.
    cli_timeout: Duration,
}

impl SpritesProvider {
    pub fn new(cli: impl Into<PathBuf>) -> Self {
        Self { cli: cli.into(), cli_timeout: CLI_TIMEOUT }
    }

    /// Shorten the control-call bound so a test can observe it without waiting.
    #[cfg(test)]
    fn with_cli_timeout(mut self, timeout: Duration) -> Self {
        self.cli_timeout = timeout;
        self
    }

    pub fn from_env() -> Self {
        let cli = std::env::var("SPRITE_CLI")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "sprite".to_string());
        Self::new(cli)
    }

    /// A provider whose CLI cannot exist, for the "no credential installed" path.
    pub fn without_cli() -> Self {
        Self::new("/nonexistent/ferryx-sprites-cli")
    }

    /// One authenticated HTTP call through `sprite api`. Returns the status and the raw body
    /// whatever the status; only a missing CLI, a failed spawn or a timeout is an error here.
    async fn api(
        &self,
        method: &str,
        path: &str,
        body: Option<Body<'_>>,
        timeout: Duration,
    ) -> Result<(u16, Vec<u8>), SandboxError> {
        let out = TempFile::new("body")?;
        let out_text = out.path().to_string_lossy().into_owned();

        let mut args: Vec<String> = vec![
            "api".to_string(),
            path.to_string(),
            "--".to_string(),
            "-s".to_string(),
            "-o".to_string(),
            out_text,
            "-w".to_string(),
            "%{http_code}".to_string(),
            "-X".to_string(),
            method.to_string(),
        ];
        if let Some(body) = &body {
            args.push("-H".to_string());
            args.push(format!("content-type: {}", body.content_type()));
            // `@-` reads the payload from stdin: nothing secret reaches argv.
            args.push("--data-binary".to_string());
            args.push("@-".to_string());
        }

        let mut child = Command::new(&self.cli)
            .args(&args)
            .stdin(if body.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| spawn_failure(&self.cli, &error))?;

        // The upload and the wait share ONE deadline. Writing the body first, outside it, let a CLI
        // that never drains stdin block the call forever: the budget only started afterwards.
        let mut stdin = child.stdin.take();
        let payload = body.as_ref().map(Body::bytes);
        let finished = tokio::time::timeout(timeout, async move {
            if let (Some(mut stdin), Some(payload)) = (stdin.take(), payload) {
                // A CLI that dies early closes the pipe; the exit status is the real story.
                let _ = stdin.write_all(payload).await;
                let _ = stdin.shutdown().await;
            }
            child.wait_with_output().await
        })
        .await;

        let output = match finished {
            Ok(Ok(output)) => output,
            Ok(Err(error)) => {
                return Err(SandboxError::new(
                    ErrorKind::Retryable,
                    PROVIDER,
                    "cli_spawn_failed",
                    format!("{} could not be run: {error}", self.cli.display()),
                ))
            }
            // Dropping the future dropped the child, and `kill_on_drop` reaped it.
            Err(_) => {
                return Err(SandboxError::new(
                    ErrorKind::Retryable,
                    PROVIDER,
                    "cli_timeout",
                    format!("{} did not finish within {}s", self.cli.display(), timeout.as_secs()),
                ))
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let code = output.status.code();

        // `sprite api` exits 0 for an HTTP error, so a non-zero exit is the CLI itself failing:
        // no login, no network, a bad flag. The status line is then absent or `000`.
        let status = stdout.trim().parse::<u16>().ok().filter(|code| *code >= 100);
        let Some(status) = status else {
            return Err(cli_failure(&self.cli, code, &stderr, &stdout));
        };

        let bytes = std::fs::read(out.path()).unwrap_or_default();
        Ok((status, bytes))
    }

    async fn sprite(&self, name: &str) -> Result<SpriteBody, SandboxError> {
        let (status, body) = self
            .api("GET", &format!("/v1/sprites/{}", encode_query(name)), None, self.cli_timeout)
            .await?;
        if status != 200 {
            return Err(status_error(status, &String::from_utf8_lossy(&body), "get"));
        }
        // A single-sprite response is the sprite object both at the top level and under `data`;
        // `parse_sprite` accepts either shape.
        parse_sprite(&String::from_utf8_lossy(&body), "get")
    }
}

#[async_trait::async_trait]
impl SandboxProvider for SpritesProvider {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            // The platform pauses an idle Sprite on its own: no TTL to pass, none to honour.
            ttl_required: false,
            max_ttl_seconds: None,
            transport: Transport::Relay,
            native_auto_stop: true,
        }
    }

    async fn create(&self, request: CreateSandbox) -> Result<String, SandboxError> {
        // A Sprite's environment is fixed by the platform: `POST /v1/sprites` accepts a name and
        // `url_settings` only, and nothing mutates the environment afterwards. Refuse before
        // spawning anything rather than dropping the caller's variables on the floor.
        if !request.env.is_empty() {
            return Err(SandboxError::new(
                ErrorKind::Config,
                PROVIDER,
                "env_unsupported",
                "a Sprite's environment is fixed by the platform; pass no env",
            ));
        }
        let payload = format!("{{\"name\":{}}}", json_string(&request.name));
        let (status, body) = self
            .api("POST", "/v1/sprites", Some(Body::Json(&payload)), self.cli_timeout)
            .await?;
        match status {
            200 | 201 => {
                let text = String::from_utf8_lossy(&body);
                Ok(parse_sprite(&text, "create")?.name)
            }
            // Idempotent on the name: an earlier create of ours won the race, so the name is the
            // handle and no second billable Sprite starts.
            409 => Ok(self.sprite(&request.name).await?.name),
            _ => Err(status_error(status, &String::from_utf8_lossy(&body), "create")),
        }
    }

    async fn get(&self, sandbox_id: &str) -> Result<SandboxInfo, SandboxError> {
        let sprite = self.sprite(sandbox_id).await?;
        let raw = sprite.status.unwrap_or_default();
        Ok(SandboxInfo { state: map_state(Some(raw.as_str())), error: None })
    }

    async fn stop(&self, sandbox_id: &str) -> Result<(), SandboxError> {
        // No pause endpoint exists. A Sprite pauses ~30 s after its last activity and exec
        // sessions are what count as activity, so stopping means ending them.
        let (status, body) = self
            .api(
                "GET",
                &format!("/v1/sprites/{}/exec", encode_query(sandbox_id)),
                None,
                self.cli_timeout,
            )
            .await?;
        if status != 200 {
            return Err(status_error(status, &String::from_utf8_lossy(&body), "stop"));
        }
        let sessions: SessionsEnvelope = parse_json(&String::from_utf8_lossy(&body), "stop")?;
        for session in sessions.sessions {
            // A session that ended between the list and the kill is already stopped.
            let _ = self
                .api(
                    "POST",
                    &format!(
                        "/v1/sprites/{}/exec/{}/kill",
                        encode_query(sandbox_id),
                        encode_query(&session.id)
                    ),
                    None,
                    self.cli_timeout,
                )
                .await;
        }
        Ok(())
    }

    async fn resume(&self, sandbox_id: &str, _ttl_seconds: Option<u64>) -> Result<(), SandboxError> {
        // Any request wakes a paused Sprite; reading it is the cheapest way to prove it is back.
        let info = self.get(sandbox_id).await?;
        if info.state == SandboxState::Error {
            return Err(SandboxError::new(
                ErrorKind::Fatal,
                PROVIDER,
                "unresumable",
                info.error.unwrap_or_else(|| "the sprite could not be resumed".to_string()),
            ));
        }
        Ok(())
    }

    async fn delete(&self, sandbox_id: &str) -> Result<(), SandboxError> {
        let (status, body) = self
            .api("DELETE", &format!("/v1/sprites/{}", encode_query(sandbox_id)), None, self.cli_timeout)
            .await?;
        if (200..300).contains(&status) {
            Ok(())
        } else {
            Err(status_error(status, &String::from_utf8_lossy(&body), "delete"))
        }
    }

    async fn extend_ttl(
        &self,
        sandbox_id: &str,
        _ttl_seconds: Option<u64>,
    ) -> Result<(), SandboxError> {
        // There is no deadline to move: idling belongs to the platform. Report success so a
        // reconcile pass does not treat the provider as broken, but prove the Sprite exists.
        let _ = self.sprite(sandbox_id).await?;
        Ok(())
    }

    async fn exec(
        &self,
        sandbox_id: &str,
        command: &str,
        timeout_seconds: u64,
    ) -> Result<ExecResult, SandboxError> {
        // `cmd` repeats to spell command + args; `sh -c` keeps the whole line one argument, so
        // nothing in it is re-split or expanded by a shell we did not ask for.
        let path = format!(
            "/v1/sprites/{}/exec?{}",
            encode_query(sandbox_id),
            [
                format!("cmd={}", encode_query("sh")),
                format!("cmd={}", encode_query("-c")),
                format!("cmd={}", encode_query(command)),
            ]
            .join("&")
        );
        // The caller's bound, plus a margin for the CLI's own startup and the response: honouring
        // it is the contract, and rounding it up to the control-call ceiling made
        // `exec(..., 5)` wait three minutes.
        let budget = exec_budget(timeout_seconds);
        let (status, body) = match self.api("POST", &path, None, budget).await {
            Ok(pair) => pair,
            Err(error) if error.provider_code == "cli_timeout" => return Ok(timed_out()),
            Err(error) => return Err(error),
        };
        if status != 200 {
            return Err(status_error(status, &String::from_utf8_lossy(&body), "exec"));
        }
        parse_exec_frames(&body)
    }

    async fn list(&self) -> Result<Vec<ListedSandbox>, SandboxError> {
        let mut out = Vec::new();
        let mut token: Option<String> = None;
        // The orphan scan walks the whole org: page until `has_more` is false, bounded so a
        // repeated continuation token cannot loop forever.
        for _ in 0..MAX_LIST_PAGES {
            let path = match token.as_deref() {
                Some(token) => format!(
                    "/v1/sprites?max_results=100&continuation_token={}",
                    encode_query(token)
                ),
                None => "/v1/sprites?max_results=100".to_string(),
            };
            let (status, body) = self.api("GET", &path, None, self.cli_timeout).await?;
            if status != 200 {
                return Err(status_error(status, &String::from_utf8_lossy(&body), "list"));
            }
            let page: ListEnvelope = parse_json(&String::from_utf8_lossy(&body), "list")?;
            for row in page.sprites {
                out.push(ListedSandbox {
                    sandbox_id: row.name.clone(),
                    state: map_state(row.status.as_deref()),
                    created_at: row.created_at.as_deref().and_then(parse_iso_ms),
                    name: Some(row.name),
                });
            }
            match page.next_continuation_token.filter(|next| !next.is_empty()) {
                Some(next) if page.has_more => token = Some(next),
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
        if bytes.len() > WRITE_FILE_CAP {
            return Err(config(
                "file_too_large",
                format!("the file API accepts at most {WRITE_FILE_CAP} bytes"),
            ));
        }
        let (working_dir, file_name) = split_path(path)?;
        // The mode travels with the write, so a secret is never briefly 0644 on the box.
        let write_path = format!(
            "/v1/sprites/{}/fs/write?{}",
            encode_query(sandbox_id),
            [
                format!("path={}", encode_query(&file_name)),
                format!("workingDir={}", encode_query(&working_dir)),
                format!("mode={mode:04o}"),
                "mkdir=true".to_string(),
            ]
            .join("&")
        );
        let (status, body) = self
            .api("PUT", &write_path, Some(Body::Bytes(&bytes)), self.cli_timeout)
            .await?;
        if status == 200 {
            Ok(())
        } else {
            Err(status_error(status, &String::from_utf8_lossy(&body), "write_file"))
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Wire types

#[derive(Debug, Deserialize)]
struct SpriteBody {
    name: String,
    #[serde(default)]
    status: Option<String>,
}

/// A sprite as the API returns it. `POST /v1/sprites` answers the sprite **flat** (verified
/// live), while `GET /v1/sprites/{name}` repeats it both at the top level and under `data`, so
/// both shapes are accepted rather than assuming either.
fn parse_sprite(body: &str, call: &str) -> Result<SpriteBody, SandboxError> {
    let value: Value = parse_json(body, call)?;
    if let Some(data) = value.get("data") {
        if data.get("name").is_some() {
            return serde_json::from_value(data.clone()).map_err(|error| unexpected(call, &error));
        }
    }
    serde_json::from_value(value).map_err(|error| unexpected(call, &error))
}

#[derive(Debug, Deserialize)]
struct SessionsEnvelope {
    #[serde(default)]
    sessions: Vec<SessionBody>,
}

#[derive(Debug, Deserialize)]
struct SessionBody {
    id: String,
}

#[derive(Debug, Deserialize)]
struct ListEnvelope {
    #[serde(default)]
    sprites: Vec<ListRow>,
    #[serde(default)]
    has_more: bool,
    #[serde(default)]
    next_continuation_token: Option<String>,
}

/// A row of `GET /v1/sprites` (flat, unlike the single-sprite envelope).
#[derive(Debug, Deserialize)]
struct ListRow {
    name: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    created_at: Option<String>,
}

fn parse_json<T: for<'de> Deserialize<'de>>(body: &str, call: &str) -> Result<T, SandboxError> {
    serde_json::from_str(body).map_err(|error| unexpected(call, &error))
}

fn unexpected(call: &str, error: &serde_json::Error) -> SandboxError {
    SandboxError::new(
        ErrorKind::Fatal,
        PROVIDER,
        "unexpected_body",
        format!("{call}: the API answered with an unexpected body: {error}"),
    )
}

/// The provider's status vocabulary (`running|warm|cold`) in [`SandboxState`] terms.
fn map_state(status: Option<&str>) -> SandboxState {
    match status.unwrap_or_default() {
        "running" => SandboxState::Ready,
        // `warm` is suspended with memory frozen and `cold` is fully stopped: both answer a
        // request in under 2 s and neither is a failure.
        "warm" | "cold" => SandboxState::Stopped,
        "starting" | "warming" => SandboxState::Provisioning,
        "stopping" => SandboxState::Stopping,
        _ => SandboxState::Provisioning,
    }
}

/// ISO-8601 to unix milliseconds. `time` is already a dependency; a bad stamp is simply absent.
fn parse_iso_ms(text: &str) -> Option<i64> {
    let format = time::format_description::well_known::Rfc3339;
    let parsed = time::OffsetDateTime::parse(text, &format).ok()?;
    i64::try_from(parsed.unix_timestamp_nanos() / 1_000_000).ok()
}

/// Split an absolute path into the `workingDir` and file name the file API wants.
fn split_path(path: &str) -> Result<(String, String), SandboxError> {
    if !path.starts_with('/') {
        return Err(config("path_not_absolute", "write_file needs an absolute path"));
    }
    match path.rsplit_once('/') {
        Some((parent, name)) if !name.is_empty() => {
            Ok((if parent.is_empty() { "/".to_string() } else { parent.to_string() }, name.to_string()))
        }
        _ => Err(config("bad_path", format!("write_file needs a file path, got {path}"))),
    }
}

/// The deadline for one exec: the caller's bound plus the CLI's own margin, never the generous
/// control-call ceiling (that made `exec(..., 5)` wait 180 s).
fn exec_budget(timeout_seconds: u64) -> Duration {
    Duration::from_secs(timeout_seconds.max(1)).saturating_add(EXEC_TIMEOUT_MARGIN)
}

/// Fold the framed exec body into one [`ExecResult`]. See the module docs for the framing.
///
/// The exit code is read from the **last byte** of the stream rather than from "the byte after the
/// `0x03` marker", because the code itself can be `0x01`, `0x02` or `0x03`: `exit 3` answers
/// `01 OUT 02 ERR 03 03`, where the naive rule would read the second `03` as another marker and
/// lose the code. The exit frame is always the last thing the server writes, so its payload is the
/// final byte.
fn parse_exec_frames(bytes: &[u8]) -> Result<ExecResult, SandboxError> {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut mode = FRAME_STDOUT;
    let mut saw_exit = false;
    for &byte in bytes {
        match byte {
            FRAME_STDOUT | FRAME_STDERR => mode = byte,
            FRAME_EXIT => {
                saw_exit = true;
                mode = FRAME_EXIT;
            }
            _ => match mode {
                FRAME_STDERR => stderr.push(byte),
                // Output past the exit marker cannot exist; ignore anything strange there.
                FRAME_EXIT => {}
                _ => stdout.push(byte),
            },
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
    let exit_code = saw_exit
        .then(|| bytes.last().copied())
        .flatten()
        .map(i32::from);
    Ok(ExecResult {
        exit_code,
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        timed_out: false,
    })
}

fn timed_out() -> ExecResult {
    ExecResult { exit_code: None, stdout: String::new(), stderr: String::new(), timed_out: true }
}

fn config(code: impl Into<String>, message: impl Into<String>) -> SandboxError {
    SandboxError::new(ErrorKind::Config, PROVIDER, code, message)
}

/// Percent-encode a query-string value (RFC 3986 unreserved bytes kept). An exec command or a
/// path that carries `&`, `=`, `+`, a space or a slash must not reach the URL raw.
fn encode_query(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// A JSON string literal, escaped. The name is the only value that travels in a body here.
fn json_string(value: &str) -> String {
    Value::String(value.to_string()).to_string()
}

fn spawn_failure(cli: &Path, error: &std::io::Error) -> SandboxError {
    if error.kind() == std::io::ErrorKind::NotFound {
        return SandboxError::new(
            ErrorKind::Config,
            PROVIDER,
            "cli_missing",
            format!(
                "`{}` is not on PATH; install the sprite CLI and run `sprite login`",
                cli.display()
            ),
        );
    }
    SandboxError::new(ErrorKind::Retryable, PROVIDER, "cli_spawn_failed", error.to_string())
}

/// The CLI itself failed (a non-zero exit, no HTTP status line): almost always "not signed in".
fn cli_failure(cli: &Path, code: Option<i32>, stderr: &str, stdout: &str) -> SandboxError {
    let text = format!("{stderr}\n{stdout}").to_lowercase();
    let detail = first_meaningful_line(stderr)
        .or_else(|| first_meaningful_line(stdout))
        .unwrap_or_else(|| "no output".to_string());
    let kind = if text.contains("not logged in")
        || text.contains("not authenticated")
        || text.contains("unauthorized")
        || text.contains("no organization")
        || text.contains("401")
        || text.contains("403")
        || text.contains("could not authenticate")
    {
        ErrorKind::Config
    } else if text.contains("not found") || text.contains("404") {
        ErrorKind::NotFound
    } else if text.contains("timeout") || text.contains("temporarily") || text.contains("try again")
    {
        ErrorKind::Retryable
    } else {
        ErrorKind::Fatal
    };
    let message = if kind == ErrorKind::Config {
        format!("{} is not signed in: run `sprite login` ({detail})", cli.display())
    } else {
        format!("{} failed: {detail}", cli.display())
    };
    SandboxError::new(kind, PROVIDER, format!("sprite_exit_{}", code.unwrap_or(-1)), message)
}

fn first_meaningful_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("Calling API") && !line.starts_with("URL:"))
        .map(str::to_string)
}

/// Map an HTTP failure onto an [`ErrorKind`]. The API answers `{"error": "<text>"}`.
fn status_error(status: u16, body: &str, call: &str) -> SandboxError {
    let parsed = serde_json::from_str::<Value>(body).ok();
    let detail = parsed
        .as_ref()
        .and_then(|value| value.get("error"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            parsed
                .as_ref()
                .and_then(|value| value.get("message"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| body.trim().to_string());
    let code = if detail.contains("already exists") {
        "already_exists"
    } else if detail.contains("not found") {
        "not_found"
    } else {
        "http_error"
    };
    let kind = match status {
        401 | 403 => ErrorKind::Config,
        404 => ErrorKind::NotFound,
        409 => ErrorKind::Conflict,
        413 => ErrorKind::Config,
        429 | 500..=599 => ErrorKind::Retryable,
        _ => ErrorKind::Fatal,
    };
    SandboxError::new(kind, PROVIDER, code, format!("{call}: HTTP {status}: {detail}"))
}

/// A file that deletes itself: the response body buffer handed to `sprite api -o`.
struct TempFile {
    path: PathBuf,
}

impl TempFile {
    fn new(role: &str) -> Result<Self, SandboxError> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default();
        let path = std::env::temp_dir().join(format!(
            "ferryx-sprites-{}-{nanos}-{unique}-{role}",
            std::process::id()
        ));
        // Created up front so the CLI cannot be pointed at a path that does not exist yet, and
        // removed by `Drop` whatever the call returns. Owner-only from the first byte: this file
        // holds a provider response body, and `/tmp` is world-writable.
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        options.open(&path).map_err(|error| {
            config("temp_file_failed", format!("could not create a response buffer: {error}"))
        })?;
        // A umask can only clear bits and some filesystems ignore the open mode, so state 0600
        // explicitly as well.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/cloud_host/sprites")
            .join(name);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read fixture {}: {error}", path.display()))
    }

    fn fixture_bytes(name: &str) -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/cloud_host/sprites")
            .join(name);
        std::fs::read(&path)
            .unwrap_or_else(|error| panic!("read fixture {}: {error}", path.display()))
    }

    fn make_executable(script: &Path) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(script, std::fs::Permissions::from_mode(0o755))
                .expect("chmod the stub");
        }
        #[cfg(not(unix))]
        let _ = script;
    }

    /// A stub `sprite` that records its argv and stdin, then answers with the canned status and
    /// body. It is a shell script, so it proves the argv contract without a real account.
    struct StubCli {
        dir: tempfile::TempDir,
    }

    impl StubCli {
        fn new(status: u16, body: &str) -> Self {
            let dir = tempfile::tempdir().expect("tempdir");
            let script = dir.path().join("sprite");
            let canned = dir.path().join("canned_body");
            std::fs::write(&canned, body).expect("canned body");
            let argv = dir.path().join("argv");
            let stdin = dir.path().join("stdin");
            // `$@` is written one argument per line; the body file is copied where `-o` points.
            std::fs::write(
                &script,
                format!(
                    "#!/bin/sh\nprintf '%s\\n' \"$@\" > {argv}\ncat > {stdin}\n\
                     out=''\nwhile [ $# -gt 0 ]; do\n  if [ \"$1\" = '-o' ]; then out=$2; fi\n  shift\ndone\n\
                     if [ -n \"$out\" ]; then cp {canned} \"$out\"; fi\n\
                     printf '{status}'\n",
                    argv = argv.display(),
                    stdin = stdin.display(),
                    canned = canned.display(),
                    status = status,
                ),
            )
            .expect("stub script");
            make_executable(&script);
            Self { dir }
        }
        fn provider(&self) -> SpritesProvider {
            SpritesProvider::new(self.dir.path().join("sprite"))
        }

        /// A stub that ignores its arguments and does nothing for `delay`, so an exec's own
        /// deadline is the only thing that can end the call.
        fn sleeping(delay: Duration) -> Self {
            let dir = tempfile::tempdir().expect("tempdir");
            let script = dir.path().join("sprite");
            std::fs::write(&script, format!("#!/bin/sh\nsleep {}\nprintf '200'\n", delay.as_secs()))
                .expect("stub script");
            make_executable(&script);
            Self { dir }
        }

        /// A stub that never reads stdin, so a body larger than the pipe buffer can only finish
        /// if the write is bounded.
        fn deaf() -> Self {
            let dir = tempfile::tempdir().expect("tempdir");
            let script = dir.path().join("sprite");
            std::fs::write(&script, "#!/bin/sh\nsleep 120\n").expect("stub script");
            make_executable(&script);
            Self { dir }
        }

        fn argv(&self) -> String {
            std::fs::read_to_string(self.dir.path().join("argv")).unwrap_or_default()
        }

        fn stdin(&self) -> Vec<u8> {
            std::fs::read(self.dir.path().join("stdin")).unwrap_or_default()
        }
    }

    #[test]
    fn create_live_decodes_a_cold_sprite() {
        // `GET /v1/sprites/{name}` repeats the sprite under `data`.
        let sprite = parse_sprite(&fixture("create_live.json"), "create").unwrap();
        assert_eq!(sprite.name, "fx-qa-probe-1");
        assert_eq!(sprite.status.as_deref(), Some("cold"));
    }

    #[test]
    fn create_flat_live_decodes_the_shape_post_really_returns() {
        // Observed live 2026-11-21: `POST /v1/sprites` answers the sprite flat, with no `data`
        // wrapper, so a `data`-only parser failed on a real create.
        let sprite = parse_sprite(&fixture("create_flat_live.json"), "create").unwrap();
        assert_eq!(sprite.name, "fx-t11-live-68340");
        assert_eq!(sprite.status.as_deref(), Some("cold"));
    }

    #[test]
    fn get_running_live_is_ready() {
        let sprite = parse_sprite(&fixture("get_running_live.json"), "get").unwrap();
        assert_eq!(map_state(sprite.status.as_deref()), SandboxState::Ready);
    }

    #[test]
    fn get_warm_live_is_stopped_not_ready() {
        // `warm` is a suspended VM: reachable in ~500 ms but not running, so a status check must
        // not call it ready (which would imply the daemon inside is live).
        let sprite = parse_sprite(&fixture("get_warm_live.json"), "get").unwrap();
        assert_eq!(map_state(sprite.status.as_deref()), SandboxState::Stopped);
    }

    #[test]
    fn list_live_decodes_the_sprites_array() {
        let parsed: ListEnvelope = parse_json(&fixture("list_live.json"), "list").unwrap();
        assert_eq!(parsed.sprites.len(), 1);
        assert_eq!(parsed.sprites[0].name, "fx-qa-probe-1");
        assert_eq!(map_state(parsed.sprites[0].status.as_deref()), SandboxState::Stopped);
        let stamp = parse_iso_ms(parsed.sprites[0].created_at.as_deref().unwrap()).unwrap();
        assert!(stamp > 1_700_000_000_000, "created_at decodes to unix milliseconds");

        let empty: ListEnvelope = parse_json(&fixture("list_empty_live.json"), "list").unwrap();
        assert!(empty.sprites.is_empty());
    }

    #[test]
    fn exec_stdout_stderr_live_splits_the_frames() {
        let result = parse_exec_frames(&fixture_bytes("exec_stdout_stderr_live.bin")).unwrap();
        assert_eq!(result.exit_code, Some(0));
        assert_eq!(result.stdout, "OUT\n");
        assert_eq!(result.stderr, "ERR\n");
        assert!(!result.timed_out);
    }

    #[test]
    fn exec_exit7_live_reports_exit_code_seven() {
        // Live bytes for `printf OUT; printf ERR >&2; exit 7`.
        let result = parse_exec_frames(&fixture_bytes("exec_exit7_live.bin")).unwrap();
        assert_eq!(result.exit_code, Some(7));
        assert_eq!(result.stdout, "OUT");
        assert_eq!(result.stderr, "ERR");
    }

    #[test]
    fn exec_marker_bytes_live_drop_the_payload_but_keep_the_exit_code() {
        // Live bytes for `printf '\001\002\003'` came back as `01 01 02 03 03 00`: the payload
        // markers were read as markers, so the bytes are gone but the exit code survives. Pinned
        // so the documented limitation cannot silently regress into a wrong byte stream.
        let result = parse_exec_frames(&fixture_bytes("exec_marker_bytes_live.bin")).unwrap();
        assert_eq!(result.exit_code, Some(0));
        assert!(result.stdout.is_empty(), "control bytes are not recoverable output");
        assert!(result.stderr.is_empty());
    }

    #[test]
    fn exec_exit3_keeps_a_code_that_collides_with_its_own_marker() {
        // `printf OUT; printf ERR >&2; exit 3` -> `01 OUT 02 ERR 03 03`. The naive "byte after
        // 0x03" rule reports no code here; the last-byte rule reports 3. Observed live: the naive
        // rule lost it.
        let result = parse_exec_frames(&fixture_bytes("exec_exit3_live.bin")).unwrap();
        assert_eq!(result.exit_code, Some(3));
        assert_eq!(result.stdout, "OUT");
        assert_eq!(result.stderr, "ERR");
    }

    #[test]
    fn exec_exit1_and_0_survive_the_same_collision() {
        let one = parse_exec_frames(&fixture_bytes("exec_exit1_live.bin")).unwrap();
        assert_eq!(one.exit_code, Some(1));
        assert_eq!(one.stdout, "OUT");

        let zero = parse_exec_frames(&fixture_bytes("exec_exit0_only_live.bin")).unwrap();
        assert_eq!(zero.exit_code, Some(0));
        assert!(zero.stdout.is_empty());
    }

    #[test]
    fn exec_empty_body_reports_no_exit_code() {
        let result = parse_exec_frames(&[]).unwrap();
        assert_eq!(result.exit_code, None);
        assert!(result.stdout.is_empty());
        assert!(result.stderr.is_empty());
    }

    #[test]
    fn exec_output_past_the_cap_is_fatal() {
        let mut body = vec![FRAME_STDOUT];
        body.extend(std::iter::repeat_n(b'a', EXEC_OUTPUT_CAP + 1));
        let error = parse_exec_frames(&body).expect_err("output past the cap must fail");
        assert_eq!(error.kind, ErrorKind::Fatal);
        assert_eq!(error.provider_code, "output_limit_exceeded");
    }

    #[test]
    fn not_found_live_maps_to_not_found() {
        let error = status_error(404, &fixture("error_not_found_live.json"), "get");
        assert_eq!(error.kind, ErrorKind::NotFound);
        assert_eq!(error.provider_code, "not_found");
        assert!(error.message.contains("sprite not found"));
    }

    #[test]
    fn name_taken_live_maps_to_conflict() {
        let error = status_error(409, &fixture("error_name_taken_live.json"), "create");
        assert_eq!(error.kind, ErrorKind::Conflict);
        assert_eq!(error.provider_code, "already_exists");
    }

    #[test]
    fn http_statuses_map_to_the_documented_kinds() {
        assert_eq!(status_error(401, "{}", "call").kind, ErrorKind::Config);
        assert_eq!(status_error(403, "{}", "call").kind, ErrorKind::Config);
        assert_eq!(status_error(404, "{}", "call").kind, ErrorKind::NotFound);
        assert_eq!(status_error(409, "{}", "call").kind, ErrorKind::Conflict);
        assert_eq!(status_error(413, "{}", "call").kind, ErrorKind::Config);
        assert_eq!(status_error(429, "{}", "call").kind, ErrorKind::Retryable);
        assert_eq!(status_error(500, "{}", "call").kind, ErrorKind::Retryable);
        assert_eq!(status_error(400, "{}", "call").kind, ErrorKind::Fatal);
    }

    #[test]
    fn exec_sessions_live_decodes_an_empty_list() {
        let sessions: SessionsEnvelope =
            parse_json(&fixture("exec_sessions_live.json"), "stop").unwrap();
        assert!(sessions.sessions.is_empty());
    }

    #[test]
    fn encode_query_escapes_everything_but_unreserved_bytes() {
        assert_eq!(encode_query("abc-_.~XYZ019"), "abc-_.~XYZ019");
        assert_eq!(encode_query("a b&c=d+e/f"), "a%20b%26c%3Dd%2Be%2Ff");
    }

    #[test]
    fn split_path_gives_working_dir_and_name() {
        assert_eq!(
            split_path("/home/sprite/.omo/agent/auth.json").unwrap(),
            ("/home/sprite/.omo/agent".to_string(), "auth.json".to_string())
        );
        assert_eq!(split_path("/etc/hosts").unwrap(), ("/etc".to_string(), "hosts".to_string()));
        assert_eq!(split_path("/x").unwrap(), ("/".to_string(), "x".to_string()));
        assert_eq!(
            split_path("/trailing/").expect_err("a directory is not a file path").kind,
            ErrorKind::Config
        );
        assert_eq!(
            split_path("relative/file").expect_err("a relative path is refused").provider_code,
            "path_not_absolute"
        );
    }

    #[test]
    fn json_string_escapes_a_quote() {
        assert_eq!(json_string("fx-1"), "\"fx-1\"");
        assert_eq!(json_string("a\"b"), "\"a\\\"b\"");
    }

    #[test]
    fn cli_failure_detects_a_signed_out_cli() {
        let error = cli_failure(
            Path::new("sprite"),
            Some(1),
            "Error: not logged in. Run `sprite login`.",
            "",
        );
        assert_eq!(error.kind, ErrorKind::Config);
        assert!(error.message.contains("sprite login"), "{}", error.message);
        assert_eq!(error.provider_code, "sprite_exit_1");
    }

    #[test]
    fn capabilities_say_relay_with_no_ttl_and_native_idling() {
        let provider = SpritesProvider::without_cli();
        let capabilities = provider.capabilities();
        assert_eq!(capabilities.transport, Transport::Relay);
        assert_eq!(capabilities.max_ttl_seconds, None);
        assert!(capabilities.native_auto_stop);
        assert!(!capabilities.ttl_required);
    }

    #[tokio::test]
    async fn a_missing_cli_is_config_mentioning_sprite_login() {
        let provider = SpritesProvider::without_cli();
        for error in [
            provider.get("fx-none").await.expect_err("no cli"),
            provider.delete("fx-none").await.expect_err("no cli"),
            provider.exec("fx-none", "id", 30).await.expect_err("no cli"),
            provider.list().await.expect_err("no cli"),
            provider
                .write_file("fx-none", "/home/sprite/.fx", vec![0u8; 4], 0o600)
                .await
                .expect_err("no cli"),
            provider
                .create(CreateSandbox {
                    idempotency_key: "fx-no-cli".into(),
                    name: "fx-no-cli".into(),
                    ttl_seconds: None,
                    env: Default::default(),
                })
                .await
                .expect_err("no cli"),
        ] {
            assert_eq!(error.kind, ErrorKind::Config, "{}", error.message);
            assert_eq!(error.provider_code, "cli_missing");
            assert!(error.message.contains("sprite login"), "{}", error.message);
        }
    }

    #[tokio::test]
    async fn get_goes_through_the_cli_and_decodes_the_running_sprite() {
        let stub = StubCli::new(200, &fixture("get_running_live.json"));
        let provider = stub.provider();
        let info = provider.get("fx-qa-probe-6").await.expect("get");
        assert_eq!(info.state, SandboxState::Ready);

        let argv = stub.argv();
        assert!(argv.contains("api"), "{argv}");
        assert!(argv.contains("/v1/sprites/fx-qa-probe-6"), "{argv}");
        assert!(argv.contains("%{http_code}"), "the status must be requested");
        assert_eq!(stub.stdin(), b"", "a GET carries no body");
    }

    #[tokio::test]
    async fn create_sends_the_name_as_json_on_stdin() {
        let stub = StubCli::new(201, &fixture("create_live.json"));
        let provider = stub.provider();
        let id = provider
            .create(CreateSandbox {
                idempotency_key: "fx-1".into(),
                name: "fx-qa-probe-1".into(),
                ttl_seconds: None,
                env: Default::default(),
            })
            .await
            .expect("create");
        assert_eq!(id, "fx-qa-probe-1");
        assert_eq!(stub.stdin(), br#"{"name":"fx-qa-probe-1"}"#);
        let argv = stub.argv();
        assert!(argv.contains("POST"), "{argv}");
        assert!(argv.contains("content-type: application/json"), "{argv}");
        assert!(
            !argv.contains("fx-qa-probe-1"),
            "the name must travel in the body, not the argv: {argv}"
        );
    }

    #[tokio::test]
    async fn write_file_puts_the_mode_in_the_query_and_the_bytes_on_stdin() {
        let stub = StubCli::new(200, r#"{"path":"/home/sprite/.fx-test","size":4,"mode":"600"}"#);
        let provider = stub.provider();
        provider
            .write_file("fx-qa-probe-6", "/home/sprite/.fx-test", vec![1, 2, 3, 4], 0o600)
            .await
            .expect("write_file");

        let argv = stub.argv();
        assert!(argv.contains("PUT"), "{argv}");
        assert!(argv.contains("path=.fx-test"), "{argv}");
        // `/` is percent-encoded in a query value; any server decodes it back to the path.
        assert!(argv.contains("workingDir=%2Fhome%2Fsprite"), "{argv}");
        assert!(argv.contains("mode=0600"), "the mode travels with the write: {argv}");
        assert!(argv.contains("mkdir=true"), "{argv}");
        assert_eq!(stub.stdin(), vec![1, 2, 3, 4], "the bytes travel on stdin");
    }

    #[tokio::test]
    async fn exec_encodes_the_command_into_repeated_cmd_parameters() {
        let stub = StubCli::new(200, "");
        std::fs::write(
            stub.dir.path().join("canned_body"),
            fixture_bytes("exec_exit7_live.bin"),
        )
        .expect("canned body");
        let provider = stub.provider();
        let result = provider.exec("fx-qa-probe-6", "echo a&b; exit 7", 30).await.expect("exec");
        assert_eq!(result.exit_code, Some(7));

        let argv = stub.argv();
        assert!(argv.contains("cmd=sh"), "{argv}");
        assert!(argv.contains("cmd=-c"), "{argv}");
        // The ampersand must be encoded, or the CLI would split it into a second parameter.
        assert!(argv.contains("cmd=echo%20a%26b%3B%20exit%207"), "{argv}");
    }

    #[tokio::test]
    async fn a_404_from_the_stub_is_not_found() {
        let stub = StubCli::new(404, &fixture("error_not_found_live.json"));
        let provider = stub.provider();
        let error = provider.get("fx-gone").await.expect_err("404");
        assert_eq!(error.kind, ErrorKind::NotFound);
    }

    #[tokio::test]
    async fn create_reuses_the_sprite_when_the_name_is_taken() {
        // `409` on create falls back to reading the existing Sprite, so a retried create never
        // starts a second billable machine.
        let dir = tempfile::tempdir().expect("tempdir");
        let script = dir.path().join("sprite");
        let existing = dir.path().join("existing.json");
        std::fs::write(&existing, fixture("get_running_live.json")).expect("fixture");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nout=''\nwhile [ $# -gt 0 ]; do\n  if [ \"$1\" = '-o' ]; then out=$2; fi\n  shift\ndone\n\
                 case \"$2\" in\n  /v1/sprites) printf '409'; cp /dev/null \"$out\" ;;\n\
                 *) printf '200'; cp {existing} \"$out\" ;;\nesac\n",
                existing = existing.display()
            ),
        )
        .expect("stub");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        }
        let provider = SpritesProvider::new(&script);
        let id = provider
            .create(CreateSandbox {
                idempotency_key: "fx-1".into(),
                name: "fx-qa-probe-6".into(),
                ttl_seconds: None,
                env: Default::default(),
            })
            .await
            .expect("create");
        assert_eq!(id, "fx-qa-probe-6");
    }

    #[tokio::test]
    async fn write_file_rejects_a_relative_path_before_any_call() {
        let provider = SpritesProvider::without_cli();
        let error = provider
            .write_file("fx-none", "relative", vec![0u8; 4], 0o600)
            .await
            .expect_err("a relative path is refused");
        assert_eq!(error.kind, ErrorKind::Config);
        assert_eq!(error.provider_code, "path_not_absolute");
    }

    #[tokio::test]
    async fn write_file_refuses_a_body_over_the_api_cap() {
        let provider = SpritesProvider::without_cli();
        let error = provider
            .write_file("fx-none", "/home/sprite/big", vec![0u8; WRITE_FILE_CAP + 1], 0o600)
            .await
            .expect_err("over the cap");
        assert_eq!(error.kind, ErrorKind::Config);
        assert_eq!(error.provider_code, "file_too_large");
    }

    /// D3: `exec` must honour the caller's own bound. Before the fix the budget was
    /// `(timeout+30).max(180s)`, so `exec(..., 5)` ran for three minutes.
    #[tokio::test]
    async fn exec_honours_the_callers_timeout() {
        let stub = StubCli::sleeping(Duration::from_secs(60));
        let provider = stub.provider();
        let started = std::time::Instant::now();
        let result = tokio::time::timeout(
            Duration::from_secs(20),
            provider.exec("fx-qa-probe-6", "sleep 60", 5),
        )
        .await
        .expect("exec(timeout_seconds=5) was still running after 20s: the caller's bound is ignored")
        .expect("a timed-out exec reports timed_out, it does not error");
        assert!(result.timed_out, "a killed exec must say so, got {result:?}");
        assert_eq!(result.exit_code, None);
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "the caller's 5 s bound plus the margin must finish well inside 20 s"
        );
    }

    #[test]
    fn exec_budget_is_the_callers_bound_plus_a_margin() {
        // `exec(..., 5)` used to budget 180 s; it must now be about 5 s plus a small margin.
        assert_eq!(exec_budget(5), Duration::from_secs(5) + EXEC_TIMEOUT_MARGIN);
        assert_eq!(exec_budget(600), Duration::from_secs(600) + EXEC_TIMEOUT_MARGIN);
        assert_eq!(
            exec_budget(0),
            Duration::from_secs(1) + EXEC_TIMEOUT_MARGIN,
            "a 0 is a 1 s bound"
        );
        assert!(
            exec_budget(5) < CLI_TIMEOUT,
            "the control-call ceiling must not override the caller's bound"
        );
    }

    /// D4: the body upload must be inside the call's deadline. Before the fix the stdin write ran
    /// before the only timeout, so a CLI that never drains the pipe hung `write_file` forever.
    /// The bound is shortened so the assertion is about the upload being covered, not about
    /// waiting the real ceiling out.
    #[tokio::test]
    async fn write_file_is_bounded_when_the_cli_never_reads_stdin() {
        // 4 MiB cannot fit in a pipe buffer, so this write can only finish if the CLI reads.
        let stub = StubCli::deaf();
        let provider = stub.provider().with_cli_timeout(Duration::from_millis(500));
        let body = vec![0xABu8; 4 * 1024 * 1024];
        let write = provider.write_file("fx-qa-probe-6", "/home/sprite/big.bin", body, 0o600);
        let error = tokio::time::timeout(Duration::from_secs(20), write)
            .await
            .expect("write_file hung past 20s: the stdin write is outside every timeout")
            .expect_err("a CLI that never reads must fail, not hang");
        assert_eq!(error.kind, ErrorKind::Retryable, "{}", error.message);
        assert_eq!(error.provider_code, "cli_timeout", "{}", error.message);
    }

    /// D5: `CreateSandbox.env` cannot be honoured (no create-time env field, none afterwards), so
    /// create must refuse like codespaces/instabox/tensorlake instead of silently dropping it.
    #[tokio::test]
    async fn create_refuses_env_it_cannot_honour() {
        // A stub that would answer 201: if create does not refuse first, this test fails on the
        // `Ok` rather than on a missing CLI.
        let stub = StubCli::new(201, &fixture("create_flat_live.json"));
        let provider = stub.provider();
        let mut env = std::collections::BTreeMap::new();
        env.insert("FX_ENGINE".to_string(), "omo".to_string());
        let error = provider
            .create(CreateSandbox {
                idempotency_key: "fx-1".into(),
                name: "fx-qa-probe-1".into(),
                ttl_seconds: None,
                env,
            })
            .await
            .expect_err("env cannot be honoured by a Sprite create");
        assert_eq!(error.kind, ErrorKind::Config);
        assert_eq!(error.provider_code, "env_unsupported");
        // Refused before any call: the CLI never ran, so no Sprite exists to clean up.
        assert_eq!(stub.argv(), "", "create must refuse before spawning the CLI");
    }

    /// D6: the response buffer holds a provider body in a world-writable `/tmp`, and the module
    /// docs claim 0600.
    #[cfg(unix)]
    #[test]
    fn response_buffer_is_0600() {
        use std::os::unix::fs::PermissionsExt as _;
        let file = TempFile::new("body").expect("temp file");
        let mode = std::fs::metadata(file.path()).expect("stat").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "response buffer is {mode:o}, docs claim 0600");
    }

    #[test]
    fn response_buffer_is_removed_on_drop() {
        let path = {
            let file = TempFile::new("body").expect("temp file");
            file.path().to_path_buf()
        };
        assert!(!path.exists(), "the buffer must not outlive the call: {}", path.display());
    }
}

#[cfg(test)]
mod live {
    //! The live smoke, run on demand against the signed-in `sprite` CLI: exactly one
    //! create/exec/write/stat/delete cycle, deleted in every path including a panic.
    //!
    //! `cargo test --lib cloud_host::provider::sprites::live -- --ignored --nocapture`

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

    /// Deletes the sprite when dropped, including while the test unwinds from a failed
    /// assertion. Free credit is the only budget, so a leaked machine must be impossible.
    ///
    /// This is a safety net, not the test's assertion: the body deletes explicitly and checks the
    /// sprite is gone, and only then disarms this guard. A panic fails the test on its own, so a
    /// guard that prints a failed delete cannot turn a failure into a pass.
    struct Cleanup {
        name: String,
        armed: bool,
    }

    impl Cleanup {
        fn new(name: String) -> Self {
            Self { name, armed: true }
        }

        /// Stops the guard from deleting again once the body has deleted and verified the sprite.
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
                match runtime.block_on(SpritesProvider::from_env().delete(&name)) {
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
    #[ignore = "live: needs a signed-in sprite CLI and spends free credit"]
    async fn live_smoke() {
        let provider = SpritesProvider::from_env();
        let name = name();
        // Armed before any call: a panic anywhere below still deletes the sprite.
        let mut cleanup = Cleanup::new(name.clone());

        let created = provider
            .create(CreateSandbox {
                idempotency_key: name.clone(),
                name: name.clone(),
                ttl_seconds: None,
                env: Default::default(),
            })
            .await;
        let id = match created {
            Ok(id) => id,
            Err(error) => {
                println!("create failed: {error}");
                panic!("create: {error}");
            }
        };
        println!("create -> {id}");

        let result = async {
            let info = provider.get(&id).await?;
            println!("get -> {info:?}");

            let uname = provider.exec(&id, "uname -m", 120).await?;
            println!("exec uname -m -> {uname:?}");
            assert_eq!(uname.exit_code, Some(0));
            assert!(!uname.stdout.trim().is_empty());

            // stderr must not be folded into stdout.
            let split = provider.exec(&id, "printf OUT; printf ERR >&2; exit 3", 120).await?;
            println!("exec split -> {split:?}");
            assert_eq!(split.exit_code, Some(3));
            assert_eq!(split.stdout, "OUT");
            assert_eq!(split.stderr, "ERR");

            let secret: Vec<u8> = (0u8..32).collect();
            let path = format!("/home/sprite/.ferryx-cloud/{name}.json");
            provider.write_file(&id, &path, secret.clone(), 0o600).await?;
            println!("write_file -> {path}");

            let stat = provider
                .exec(&id, &format!("stat -c '%a %s' {path} && od -An -tu1 {path}"), 120)
                .await?;
            println!("stat -> {stat:?}");
            assert_eq!(stat.exit_code, Some(0));
            let first = stat.stdout.lines().next().unwrap_or_default().trim().to_string();
            assert_eq!(first, "600 32", "expected 0600 and 32 bytes, got {first:?}");
            let written: Vec<u8> = stat
                .stdout
                .lines()
                .skip(1)
                .flat_map(|line| line.split_whitespace().filter_map(|n| n.parse().ok()))
                .collect();
            assert_eq!(written, secret);

            let listed = provider.list().await?;
            println!("list -> {} rows", listed.len());
            assert!(listed.iter().any(|row| row.sandbox_id == id));
            Ok::<(), SandboxError>(())
        }
        .await;

        let deleted = provider.delete(&id).await;
        println!("delete -> {deleted:?}");
        let gone = provider.get(&id).await;
        println!("get after delete -> {gone:?}");
        // Cleanup assertions run before the body's result so a failed call cannot hide a machine.
        deleted.expect("delete");
        assert!(gone.is_err(), "a deleted sprite must not be reported present");
        if let Err(error) = &gone {
            assert_eq!(error.kind, ErrorKind::NotFound, "{}", error.message);
        }
        // Deleted and verified: the guard has nothing left to do.
        cleanup.disarm();
        result.expect("live smoke");
    }

    /// Failure scenario, run without an ignored flag: an unusable CLI must answer `Config`
    /// naming `sprite login`, never a network call and never a panic.
    #[tokio::test]
    async fn no_cli_is_config_live() {
        let provider = SpritesProvider::without_cli();
        let error = provider.get("fx-t11-live-absent").await.expect_err("no cli");
        assert_eq!(error.kind, ErrorKind::Config);
        println!("no cli -> {error}");
    }
}
