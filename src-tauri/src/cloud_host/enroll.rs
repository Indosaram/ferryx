//! Relay enrollment for a freshly created T2 box: the box-side provisioning steps and the
//! Mac-side proof that the box is actually reachable.
//!
//! A T2 machine is a host in someone else's cloud. It cannot enroll as a machine of its own: the
//! account is at its record limit, no route deletes a record, and the relay pins each machineId's
//! public key at that machineId's first successful connection forever. So a box adopts one of the
//! records in [`super::slots`]' pool, and this module is what puts that record on the box and gets
//! the box's daemon talking to the relay.
//!
//! Two entry points, in the order `cloud create` uses them:
//!
//! 1. [`provision_relay`] - mint a one-use enrollment code from this Mac's account session, write
//!    the slot's identity and the code onto the box, enroll, delete the code, start
//!    `ferryx-cli --daemon`, and send `remoteConfigure` over the box's daemon socket.
//! 2. [`prove_reachable`] - ask the relay for an attach session **for that record** and require a
//!    session id. `online: true` in a machine listing is not proof: the listing is a stale flag,
//!    while an allocated session is the relay saying it holds a live control channel for the
//!    record.
//!
//! [`provision_and_prove`] is both, which is what a create should call.
//!
//! # Secrets
//!
//! The enrollment code and the slot's private keys never travel through `exec`: argv and stdin are
//! both readable by every process on the box (`ps`, a shell history, a crash reporter). They go
//! through [`SandboxProvider::write_file`](super::SandboxProvider::write_file) as 0600 files, and
//! the code is read by `ferryx-cli account enroll --code-file`, which keeps it out of argv there
//! too. The code file is removed on every path after it was written, including failures, and no
//! command string or error message built here contains a code or a key.
//!
//! # Why the proof is an HTTP allocation and not daemon IPC
//!
//! `remoteAllocateAttachSession` over this Mac's daemon socket takes no record id: it always
//! allocates for the Mac's *own* identity (`daemon::server` -> `relay_client::allocate_opaque_session`).
//! It therefore cannot answer a question about a cloud record. The relay's attach route is the same
//! gate with an explicit `machineId` - authorize with the account session, look the record's control
//! channel up, then `409 MACHINE_OFFLINE` or `200 {sessionId}` - and a 200 with a session id is the
//! `remoteAttachSessionOk` equivalent this module requires. [`AttachProver`] is the seam: the real
//! implementation is [`RelayAccount`], and a test injects its own.
//!
//! # Box requirements
//!
//! `python3` and `ferryx-cli` must be on the box. `python3` is present on every in-scope image
//! (task 13's acceptance depends on it) and is the only way a one-shot `exec` can speak a unix
//! socket; `ferryx-cli` is resolved from `$FERRYX_CLI`, the bootstrap's install root, or `PATH`.
//!
//! # Exit code
//!
//! [`NO_RELAY_SLOT`] is the design token for a dry pool, and [`EXIT_NO_RELAY_SLOT`] (4) is the exit
//! code the CLI maps it onto. Todo 12 owns that wiring; this module only guarantees the token and
//! [`EnrollError::exit_code`].

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};

use super::catalog::STATE_DIR;
use super::provider::{ExecResult, SandboxError, SandboxProvider};
use super::slots::{SlotError, SlotLease, IDENTITY_FILE};
use crate::ssh::direct::quote_posix;

/// Every relay slot in the pool is leased: `cloud create` exits 4 without creating anything.
pub const NO_RELAY_SLOT: &str = "NO_RELAY_SLOT";
/// The exit code [`NO_RELAY_SLOT`] maps onto (todo 12 wires it into `cloud create`).
pub const EXIT_NO_RELAY_SLOT: i32 = 4;
/// The account origin that mints enrollment codes and issues attach sessions.
pub const RELAY_ORIGIN: &str = "https://relay.ferryx.dev";
/// The account session token the desktop and the QA scripts keep, relative to `$HOME`.
pub const ACCOUNT_SESSION_TOKEN_FILE: &str = "ferryx-account-session-token";
/// The relay attach keypair's file name, as `remote::attach_identity` writes it.
const ATTACH_IDENTITY_FILE: &str = "attach-identity.json";
/// The one-use enrollment code's file on the box. Removed on every path after it is written.
const CODE_FILE: &str = ".enroll-code";
/// The python helper that speaks the box daemon's unix socket, written per provisioning run.
const CONFIGURE_SCRIPT_FILE: &str = ".configure-relay.py";
/// The box daemon's log, appended to so a failed start can be read afterwards.
const DAEMON_LOG: &str = "daemon.log";

const HOME_TIMEOUT: u64 = 30;
const PREPARE_TIMEOUT: u64 = 60;
const ENROLL_TIMEOUT: u64 = 180;
const REMOVE_TIMEOUT: u64 = 60;
const DAEMON_TIMEOUT: u64 = 60;
const CONFIGURE_TIMEOUT: u64 = 180;
/// How long the configure step waits for the freshly started daemon's socket to answer.
const DAEMON_READY_SECONDS: u64 = 20;

/// The exit code for a structured code. Only `NO_RELAY_SLOT` has a dedicated one; everything else
/// is the CLI's generic failure, and todo 12 maps the rest.
pub fn exit_code_for_error_code(code: &str) -> i32 {
    if code == NO_RELAY_SLOT {
        EXIT_NO_RELAY_SLOT
    } else {
        1
    }
}

/// Where the Mac keeps its account session token.
pub fn account_session_token_path(home: &Path) -> PathBuf {
    home.join(STATE_DIR).join("hub").join("secrets").join(ACCOUNT_SESSION_TOKEN_FILE)
}

/// One step of provisioning: an `exec` on the box, named so a failure says which step broke and a
/// test can assert the order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BoxStep {
    /// Read the box's `$HOME`, so no image path is assumed.
    Home,
    /// Create the owner-only identity directory and drop a code left by an earlier attempt.
    Prepare,
    /// `ferryx-cli account enroll --code-file ...`.
    Enroll,
    /// `rm -f` the one-use code.
    RemoveCode,
    /// `ferryx-cli --daemon`, detached.
    StartDaemon,
    /// `remoteConfigure` over the daemon's unix socket.
    ConfigureRelay,
}

impl BoxStep {
    pub fn as_str(self) -> &'static str {
        match self {
            BoxStep::Home => "home",
            BoxStep::Prepare => "prepare",
            BoxStep::Enroll => "enroll",
            BoxStep::RemoveCode => "remove_code",
            BoxStep::StartDaemon => "start_daemon",
            BoxStep::ConfigureRelay => "configure_relay",
        }
    }
}

/// The typed failure of a provisioning or proof call. `code()` is the token a CLI or a test asserts
/// on; `exit_code()` is what `cloud create` returns.
///
/// No variant holds a code or a key: every `detail` is built from paths, exit codes and provider
/// text, and provider text is redacted against the code before it is stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnrollError {
    /// Every slot is leased. Raised before any provider call, so nothing is billed.
    NoRelaySlot,
    /// The slot pool refused: the record this machine holds cannot be read.
    Slot(SlotError),
    /// The provider refused a write or a command. Names the provider's own kind.
    Provider(SandboxError),
    /// This Mac has no account session token, so no code can be minted.
    SessionMissing { source: String },
    /// The account refused to issue an enrollment code. `code` is the account's own token.
    CodeIssue { code: String, message: String },
    /// One box-side step failed.
    Step { step: BoxStep, detail: String },
    /// The relay holds no control channel for the record, so the record is not reachable.
    /// `retryable` is set while the box may still be connecting (a fresh `409 MACHINE_OFFLINE`).
    Unreachable { record_id: String, machine_id: String, detail: String, retryable: bool },
    /// A local file could not be read, or a config could not be serialized.
    Io { action: &'static str, message: String },
}

impl EnrollError {
    /// `NO_RELAY_SLOT`, `ENROLL_FAILED`, ... - the stable token a CLI maps onto an exit code.
    pub fn code(&self) -> &'static str {
        match self {
            EnrollError::NoRelaySlot => NO_RELAY_SLOT,
            EnrollError::Slot(error) => error.code(),
            EnrollError::Provider(_) => "PROVIDER_ERROR",
            EnrollError::SessionMissing { .. } => "ACCOUNT_SESSION_MISSING",
            EnrollError::CodeIssue { .. } => "ENROLLMENT_CODE_UNAVAILABLE",
            EnrollError::Step { step, .. } => match step {
                BoxStep::Enroll => "ENROLL_FAILED",
                BoxStep::StartDaemon | BoxStep::ConfigureRelay => "DAEMON_CONFIGURE_FAILED",
                BoxStep::RemoveCode => "CODE_FILE_NOT_REMOVED",
                BoxStep::Home | BoxStep::Prepare => "BOX_STEP_FAILED",
            },
            EnrollError::Unreachable { .. } => "REMOTE_ATTACH_UNPROVEN",
            EnrollError::Io { .. } => "ENROLL_IO_ERROR",
        }
    }

    pub fn exit_code(&self) -> i32 {
        exit_code_for_error_code(self.code())
    }

    /// Whether another attempt could plausibly succeed: only an allocation the relay refused
    /// because no control channel exists yet.
    pub fn is_retryable_proof(&self) -> bool {
        matches!(self, EnrollError::Unreachable { retryable: true, .. })
    }

    fn session(source: impl Into<String>) -> Self {
        EnrollError::SessionMissing { source: source.into() }
    }

    /// Append a fact the operator must know - typically a cleanup step that also failed, which
    /// means a one-use code may still be on the box - without hiding the failure that came first.
    fn note(mut self, note: &str) -> Self {
        match &mut self {
            EnrollError::Step { detail, .. }
            | EnrollError::Unreachable { detail, .. } => {
                detail.push_str("; ");
                detail.push_str(note);
            }
            EnrollError::CodeIssue { message, .. } | EnrollError::Io { message, .. } => {
                message.push_str("; ");
                message.push_str(note);
            }
            // A failed write is reported as the provider's own failure, and a failed removal of what
            // that write may have left behind belongs in the same message.
            EnrollError::Provider(error) => {
                error.message.push_str("; ");
                error.message.push_str(note);
            }
            // The rest cannot own a cleanup failure: they either happen before the code exists or
            // carry a fixed message. Log it rather than drop it.
            EnrollError::NoRelaySlot | EnrollError::Slot(_) | EnrollError::SessionMissing { .. } => {
                tracing::warn!(note, "cloud provisioning failed and a cleanup step also failed");
            }
        }
        self
    }
}

impl std::fmt::Display for EnrollError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EnrollError::NoRelaySlot => f.write_str(
                "NO_RELAY_SLOT: every relay slot is leased; delete a cloud machine to return its \
                 slot to the pool",
            ),
            EnrollError::Slot(error) => write!(f, "{error}"),
            EnrollError::Provider(error) => write!(f, "{error}"),
            EnrollError::SessionMissing { source } => write!(
                f,
                "ACCOUNT_SESSION_MISSING: no account session token at {source}; sign in on this Mac \
                 before creating a relay machine"
            ),
            EnrollError::CodeIssue { code, message } => {
                write!(f, "ENROLLMENT_CODE_UNAVAILABLE: {code}: {message}")
            }
            EnrollError::Step { step, detail } => {
                write!(f, "{}: {} step failed: {detail}", self.code(), step.as_str())
            }
            EnrollError::Unreachable { record_id, machine_id, detail, .. } => write!(
                f,
                "REMOTE_ATTACH_UNPROVEN: relay record {record_id} (machineId {machine_id}) did not \
                 answer with an attach session: {detail}"
            ),
            EnrollError::Io { action, message } => write!(f, "ENROLL_IO_ERROR: {action}: {message}"),
        }
    }
}

impl std::error::Error for EnrollError {}

impl From<SlotError> for EnrollError {
    fn from(error: SlotError) -> Self {
        match error {
            SlotError::NoFreeSlot => EnrollError::NoRelaySlot,
            other => EnrollError::Slot(other),
        }
    }
}

impl From<SandboxError> for EnrollError {
    fn from(error: SandboxError) -> Self {
        EnrollError::Provider(error)
    }
}

/// Mints an enrollment code with this Mac's account session.
///
/// The real implementation is [`RelayAccount`]. A test injects a canned code, so no test mints
/// anything against the live account.
#[async_trait::async_trait]
pub trait EnrollmentCodeIssuer: Send + Sync {
    async fn issue_code(&self) -> Result<String, EnrollError>;
}

/// A relay attach session for one record: `remoteAttachSessionOk`, with the session the relay
/// handed out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reachability {
    pub session_id: String,
    pub machine_id: String,
    pub relay_origin: String,
}

/// Asks the relay for an attach session for an explicit record.
///
/// `Ok` **must** mean the relay answered with a session id. An offline record, a stale `online`
/// flag and a machine listing are all failures here.
#[async_trait::async_trait]
pub trait AttachProver: Send + Sync {
    async fn prove_attach(
        &self,
        record_id: &str,
        machine_id: &str,
    ) -> Result<Reachability, EnrollError>;
}

/// The Mac's account session, as the HTTP client that mints codes and proves reachability.
///
/// `Debug` is manual on purpose: the token is a 30-day bearer and must never reach a log line.
pub struct RelayAccount {
    origin: String,
    token: String,
    client: reqwest::Client,
}

impl std::fmt::Debug for RelayAccount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RelayAccount")
            .field("origin", &self.origin)
            .field("token", &"<redacted>")
            .finish()
    }
}

impl RelayAccount {
    /// The session this Mac is signed in with, at [`account_session_token_path`].
    pub fn from_env() -> Result<Self, EnrollError> {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .ok_or_else(|| EnrollError::session("the home directory"))?;
        Self::at(Path::new(&home), RELAY_ORIGIN)
    }

    /// [`from_env`](Self::from_env) for an explicit home directory and origin.
    pub fn at(home: &Path, origin: &str) -> Result<Self, EnrollError> {
        let path = account_session_token_path(home);
        let token = std::fs::read_to_string(&path)
            .map_err(|error| EnrollError::session(format!("{} ({error})", path.display())))?;
        let token = token.trim();
        if token.is_empty() {
            return Err(EnrollError::session(format!("{} (the file is empty)", path.display())));
        }
        Self::with_token(origin, token)
    }

    /// An explicit origin and token, for a caller that already holds them.
    pub fn with_token(origin: impl Into<String>, token: impl Into<String>) -> Result<Self, EnrollError> {
        let origin = origin.into();
        let origin = origin.trim().trim_end_matches('/').to_string();
        if origin.is_empty() {
            return Err(EnrollError::session("an empty account origin"));
        }
        let token = token.into();
        let token = token.trim().to_string();
        if token.is_empty() {
            return Err(EnrollError::session("an empty account session token"));
        }
        let client = reqwest::Client::builder()
            .user_agent(concat!("ferryx-cloud-host/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(20))
            .build()
            .unwrap_or_default();
        Ok(Self { origin, token, client })
    }

    pub fn origin(&self) -> &str {
        &self.origin
    }
}

#[async_trait::async_trait]
impl EnrollmentCodeIssuer for RelayAccount {
    async fn issue_code(&self) -> Result<String, EnrollError> {
        let response = self
            .client
            .post(format!("{}/api/account/v1/enrollment-codes", self.origin))
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|error| EnrollError::CodeIssue {
                code: "ACCOUNT_UNREACHABLE".to_string(),
                message: error.to_string(),
            })?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            let code = match status.as_u16() {
                401 | 403 => "UNAUTHORIZED",
                refused if refused >= 500 => "SERVICE_UNAVAILABLE",
                _ => "ENROLLMENT_CODE_REFUSED",
            };
            return Err(EnrollError::CodeIssue {
                code: code.to_string(),
                message: refusal_detail(status, &body),
            });
        }
        let body: Value = response.json().await.map_err(|error| EnrollError::CodeIssue {
            code: "BAD_RESPONSE".to_string(),
            message: error.to_string(),
        })?;
        body.get("code")
            .and_then(Value::as_str)
            .filter(|code| !code.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| EnrollError::CodeIssue {
                code: "BAD_RESPONSE".to_string(),
                message: "the enrollment-code route returned no code".to_string(),
            })
    }
}

#[async_trait::async_trait]
impl AttachProver for RelayAccount {
    async fn prove_attach(
        &self,
        record_id: &str,
        machine_id: &str,
    ) -> Result<Reachability, EnrollError> {
        let unreachable = |detail: String, retryable: bool| EnrollError::Unreachable {
            record_id: record_id.to_string(),
            machine_id: machine_id.to_string(),
            detail,
            retryable,
        };
        let response = self
            .client
            .post(format!("{}/api/v1/attach/session?e2e=1", self.origin))
            .bearer_auth(&self.token)
            .json(&json!({ "machineId": machine_id }))
            .send()
            .await
            .map_err(|error| unreachable(error.to_string(), true))?;
        let status = response.status();
        if status.as_u16() == 409 {
            return Err(unreachable(
                "409 MACHINE_OFFLINE: the relay holds no control channel for this record".to_string(),
                true,
            ));
        }
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(unreachable(refusal_detail(status, &body), status.is_server_error()));
        }
        let body: Value = response
            .json()
            .await
            .map_err(|error| unreachable(error.to_string(), false))?;
        let session_id = body
            .get("sessionId")
            .and_then(Value::as_str)
            .filter(|session| !session.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| unreachable("the attach route returned no sessionId".to_string(), false))?;
        Ok(Reachability {
            session_id,
            machine_id: machine_id.to_string(),
            relay_origin: self.origin.clone(),
        })
    }
}

/// Why a request was refused, in the account's own words when it has any: `code` is the token the
/// route documents (`MACHINE_NOT_FOUND`, `UNAUTHORIZED`, ...), and a body without one falls back to
/// the status line. Truncated, because this text reaches an error message.
fn refusal_detail(status: reqwest::StatusCode, body: &str) -> String {
    let documented = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| value.get("code").and_then(Value::as_str).map(str::to_owned));
    match documented {
        Some(code) => format!("the route answered {status} with {code}"),
        None => {
            let trimmed = body.trim();
            if trimmed.is_empty() {
                format!("the route answered {status}")
            } else {
                format!("the route answered {status}: {}", tail(trimmed, 200))
            }
        }
    }
}

/// What provisioning put on the box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provisioned {
    /// The relay slot's record id - the catalog row's `slot`.
    pub record_id: String,
    /// The account machineId the record's keypair is pinned to.
    pub machine_id: String,
    pub relay_origin: String,
}

/// How hard [`prove_reachable`] tries. A fresh box needs a few seconds for its relay control
/// channel, so the default retries; a test passes one attempt and no delay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProvePolicy {
    pub attempts: u32,
    pub delay: Duration,
}

impl Default for ProvePolicy {
    fn default() -> Self {
        Self { attempts: 8, delay: Duration::from_millis(2_500) }
    }
}

impl ProvePolicy {
    /// One attempt: for a caller that has already waited, and for tests.
    pub fn once() -> Self {
        Self { attempts: 1, delay: Duration::from_millis(0) }
    }
}

/// Provision a created box: enroll it with a pooled record and point its daemon at the relay.
///
/// The lease is only read, never released: the caller (the lifecycle) owns the lease and gives it
/// back when the machine is deleted, or when provisioning failed and no machine can use it.
pub async fn provision_relay(
    provider: &dyn SandboxProvider,
    sandbox_id: &str,
    lease: &SlotLease,
    issuer: &dyn EnrollmentCodeIssuer,
) -> Result<Provisioned, EnrollError> {
    let box_control = ProviderBox { provider, sandbox_id };
    provision(&box_control, lease, issuer, RELAY_ORIGIN).await
}

/// [`provision_relay`] then [`prove_reachable`]: the whole of `cloud create`'s relay half.
pub async fn provision_and_prove(
    provider: &dyn SandboxProvider,
    sandbox_id: &str,
    lease: &SlotLease,
    issuer: &dyn EnrollmentCodeIssuer,
    prover: &dyn AttachProver,
    policy: ProvePolicy,
) -> Result<Reachability, EnrollError> {
    provision_relay(provider, sandbox_id, lease, issuer).await?;
    prove_reachable(prover, lease, policy).await
}

/// Prove the record is reachable through the relay: an attach session must be issued for it.
///
/// `lease` carries both halves of the question - the record id in the pool and the machineId the
/// relay keys its control channels by. A record id alone cannot answer it, because the relay routes
/// by machineId.
pub async fn prove_reachable(
    prover: &dyn AttachProver,
    lease: &SlotLease,
    policy: ProvePolicy,
) -> Result<Reachability, EnrollError> {
    let attempts = policy.attempts.max(1);
    let mut last_error: Option<EnrollError> = None;
    for attempt in 1..=attempts {
        match prover.prove_attach(lease.slot(), lease.machine_id()).await {
            Ok(reachability) => return Ok(reachability),
            Err(error) => {
                let retry = error.is_retryable_proof() && attempt < attempts;
                last_error = Some(error);
                if !retry {
                    break;
                }
                tracing::debug!(
                    slot = lease.slot(),
                    attempt,
                    attempts,
                    "the relay has not accepted the record yet; retrying the attach proof"
                );
                tokio::time::sleep(policy.delay).await;
            }
        }
    }
    Err(last_error.unwrap_or_else(|| EnrollError::Unreachable {
        record_id: lease.slot().to_string(),
        machine_id: lease.machine_id().to_string(),
        detail: "the proof was never attempted".to_string(),
        retryable: false,
    }))
}

/// The box side of provisioning: a file write and a one-shot command, which is all a provider
/// offers. Kept behind a trait so the whole flow, including every failure path, runs without a
/// network or a real daemon.
#[async_trait::async_trait]
trait BoxControl: Send + Sync {
    async fn write(&self, path: &str, bytes: Vec<u8>, mode: u32) -> Result<(), SandboxError>;
    async fn run(
        &self,
        step: BoxStep,
        command: &str,
        timeout_seconds: u64,
    ) -> Result<ExecResult, SandboxError>;
}

struct ProviderBox<'a> {
    provider: &'a dyn SandboxProvider,
    sandbox_id: &'a str,
}

#[async_trait::async_trait]
impl BoxControl for ProviderBox<'_> {
    async fn write(&self, path: &str, bytes: Vec<u8>, mode: u32) -> Result<(), SandboxError> {
        self.provider.write_file(self.sandbox_id, path, bytes, mode).await
    }

    async fn run(
        &self,
        _step: BoxStep,
        command: &str,
        timeout_seconds: u64,
    ) -> Result<ExecResult, SandboxError> {
        // `step` exists for the test double's bookkeeping; the provider sees one shell string.
        self.provider.exec(self.sandbox_id, command, timeout_seconds).await
    }
}

async fn provision(
    box_control: &dyn BoxControl,
    lease: &SlotLease,
    issuer: &dyn EnrollmentCodeIssuer,
    relay_origin: &str,
) -> Result<Provisioned, EnrollError> {
    // 1. The record's two files, read here and never logged. `read_slot_file` keeps them in a local
    //    `Vec<u8>` that goes straight to `write_file` and is dropped with it.
    let identity = read_slot_file(lease.identity_path(), "read the relay slot identity").await?;
    let attach =
        read_slot_file(lease.attach_identity_path(), "read the relay slot attach identity").await?;

    // 2. Where the box keeps them. `exec` is a shell string, so `$HOME` is the box's own answer and
    //    no image layout is assumed here.
    let home = box_home(box_control).await?;
    let remote_dir = box_remote_dir(&home);
    let identity_path = format!("{remote_dir}/{IDENTITY_FILE}");
    let attach_path = format!("{remote_dir}/{ATTACH_IDENTITY_FILE}");
    let code_path = format!("{remote_dir}/{CODE_FILE}");
    let script_path = format!("{remote_dir}/{CONFIGURE_SCRIPT_FILE}");
    let log_path = format!("{remote_dir}/{DAEMON_LOG}");

    // 3. Owner-only directory, and no code left behind by an earlier attempt: a retried create must
    //    not enroll with a code that already expired.
    let prepare = format!(
        "mkdir -p {dir} && chmod 700 {dir} && rm -f {code}",
        dir = quote_posix(&remote_dir),
        code = quote_posix(&code_path)
    );
    run_ok(box_control, BoxStep::Prepare, &prepare, PREPARE_TIMEOUT, "").await?;

    // 4. The record lands as 0600 files first: a write that fails here must not waste a code that
    //    was already minted. A box that fails after this point keeps a copy of the record's keypair;
    //    the caller deletes that box, and the slot is never re-keyed, so the record stays usable.
    write_secret(box_control, &identity_path, identity, 0o600).await?;
    write_secret(box_control, &attach_path, attach, 0o600).await?;

    // 5. A fresh one-use code, minted last and written straight to the box. If this last write
    //    fails the code is removed again and expires unused, which is the only cost of ordering it
    //    here.
    let code = issuer.issue_code().await?;
    write_secret(box_control, &code_path, code_file_bytes(&code), 0o600).await?;

    // 6. From here the box holds a one-use code, so every outcome passes through the same removal.
    let outcome = finish_provision(
        box_control,
        Finish {
            remote_dir: &remote_dir,
            code_path: &code_path,
            script_path: &script_path,
            log_path: &log_path,
            relay_origin,
            code: &code,
        },
    )
    .await;
    let removed = remove_code(box_control, &code_path).await;
    match (outcome, removed) {
        (Ok(()), Ok(())) => {}
        (Ok(()), Err(error)) => return Err(error),
        (Err(error), Ok(())) => return Err(error),
        // Both failed. The step failure is what the operator must fix, but a code left on the box
        // is a credential fact they must know about.
        (Err(error), Err(cleanup)) => return Err(error.note(&cleanup.to_string())),
    }

    Ok(Provisioned {
        record_id: lease.slot().to_string(),
        machine_id: lease.machine_id().to_string(),
        relay_origin: relay_origin.trim_end_matches('/').to_string(),
    })
}

struct Finish<'a> {
    remote_dir: &'a str,
    code_path: &'a str,
    script_path: &'a str,
    log_path: &'a str,
    relay_origin: &'a str,
    code: &'a str,
}

/// Enroll, start the daemon, configure it. Runs with a one-use code already on the box.
async fn finish_provision(box_control: &dyn BoxControl, finish: Finish<'_>) -> Result<(), EnrollError> {
    // 6a. Enroll. Only the path travels in argv; `--code-file` is what keeps the code out of `ps`.
    let enroll = format!(
        "{resolve} \"$FX_CLI\" account enroll --code-file {code} --origin {origin}",
        resolve = RESOLVE_CLI,
        code = quote_posix(finish.code_path),
        origin = quote_posix(finish.relay_origin),
    );
    run_ok(box_control, BoxStep::Enroll, &enroll, ENROLL_TIMEOUT, finish.code).await?;

    // 6b. The relay client, detached. Its readiness is proved by the configure step below, which
    //     waits for the socket: a daemon that dies on start fails there, with its log line named.
    let start = format!(
        "{resolve} mkdir -p {dir} || exit 1; nohup \"$FX_CLI\" --daemon >>{log} 2>&1 </dev/null & exit 0",
        resolve = RESOLVE_CLI,
        dir = quote_posix(finish.remote_dir),
        log = quote_posix(finish.log_path),
    );
    run_ok(box_control, BoxStep::StartDaemon, &start, DAEMON_TIMEOUT, "").await?;

    // 6c. `remoteConfigure` over the daemon's unix socket. No shell can speak a unix socket, so the
    //     helper is a 0700 python file that is removed in the same command that runs it.
    let config = relay_config_json(finish.relay_origin)?;
    box_control
        .write(finish.script_path, CONFIGURE_SCRIPT.as_bytes().to_vec(), 0o700)
        .await
        .map_err(EnrollError::Provider)?;
    let configure = format!(
        "FX_RUNTIME=\"${{FERRYX_RUNTIME_DIR:-/tmp/ferryx-$(id -u)}}\"; python3 {script} \"$FX_RUNTIME/daemon.sock\" {version} {config} {wait}; rc=$?; rm -f {script}; exit $rc",
        script = quote_posix(finish.script_path),
        version = crate::daemon::protocol::DAEMON_PROTOCOL_VERSION,
        config = quote_posix(&config),
        wait = DAEMON_READY_SECONDS,
    );
    run_ok(box_control, BoxStep::ConfigureRelay, &configure, CONFIGURE_TIMEOUT, finish.code).await?;
    Ok(())
}

/// Write one secret-bearing file, and remove whatever may have landed if the write fails.
///
/// A failed `write_file` does not mean nothing arrived: the T2 providers upload first and apply the
/// mode second (`provider/e2b.rs` and `provider/blaxel.rs` both return `chmod_failed` with the bytes
/// already on disk), so a one-use code or a private key can be sitting at 0644 after an `Err`. The
/// record and the code must not survive that, so the path is removed before the error goes out.
async fn write_secret(
    box_control: &dyn BoxControl,
    path: &str,
    bytes: Vec<u8>,
    mode: u32,
) -> Result<(), EnrollError> {
    match box_control.write(path, bytes, mode).await {
        Ok(()) => Ok(()),
        Err(error) => match remove_quietly(box_control, path).await {
            // The provider's failure is the outcome; the successful removal only means no code and
            // no key stayed behind.
            Ok(()) => Err(EnrollError::Provider(error)),
            // A code or a key may still be on the box, which the operator has to know.
            Err(detail) => {
                let note = format!("{path} may still be on the box: {detail}");
                Err(EnrollError::Provider(error).note(&note))
            }
        },
    }
}

async fn remove_code(box_control: &dyn BoxControl, code_path: &str) -> Result<(), EnrollError> {
    let command = format!("rm -f {}", quote_posix(code_path));
    run_ok(box_control, BoxStep::RemoveCode, &command, REMOVE_TIMEOUT, "").await?;
    Ok(())
}

/// `rm -f` one path, reporting a failure as text rather than as [`BoxStep::RemoveCode`]'s own token:
/// this is called for a key or a code whose removal is the aftermath of another failure, so the
/// misleading token would hide the write error that is the operator's actual problem.
async fn remove_quietly(box_control: &dyn BoxControl, path: &str) -> Result<(), String> {
    let command = format!("rm -f {}", quote_posix(path));
    let result = box_control
        .run(BoxStep::RemoveCode, &command, REMOVE_TIMEOUT)
        .await
        .map_err(|error| error.to_string())?;
    match result.exit_code {
        Some(0) => Ok(()),
        other => Err(format!("the removal answered exit {other:?}")),
    }
}

/// The box's `$HOME`, checked to be an absolute path: every path this module builds hangs off it,
/// and a relative one would write into whatever directory the provider happened to start in.
async fn box_home(box_control: &dyn BoxControl) -> Result<String, EnrollError> {
    let result = run_ok(box_control, BoxStep::Home, "printf %s \"$HOME\"", HOME_TIMEOUT, "").await?;
    let home = result.stdout.trim().to_string();
    let usable = home.starts_with('/')
        && !home.contains('\'')
        && !home.chars().any(|character| character.is_whitespace() || character.is_control());
    if !usable {
        return Err(EnrollError::Step {
            step: BoxStep::Home,
            detail: format!("the box reported an unusable HOME ({home:?})"),
        });
    }
    Ok(home)
}

/// The box's identity directory: `$HOME/.ferryx/remote`, the one directory its daemon and
/// `ferryx-cli account enroll` read the machine identity from.
///
/// The layout is asked of [`resolve_canonical_remote_dir`](crate::remote::auth::resolve_canonical_remote_dir)
/// with the box's own `$HOME` standing in for this Mac's, rather than written out here: a record
/// left in any other directory is a record the box never reads, so the two sides must not be able to
/// drift. The result is joined POSIX-style because the box is Linux whatever this build targets.
fn box_remote_dir(home: &str) -> String {
    // `$HOME=/` is a real root-owned box: trimming it to "" would make the join relative, which is
    // the one thing `box_home` refuses to let happen.
    let home = match home.trim_end_matches('/') {
        "" => "/",
        trimmed => trimmed,
    };
    let layout = crate::remote::auth::resolve_canonical_remote_dir(|variable| match variable {
        "HOME" | "USERPROFILE" => Some(std::ffi::OsString::from(home)),
        _ => None,
    })
    .expect("supplying a home directory always resolves a remote directory");
    // POSIX joins on the box, whatever this build targets.
    layout.to_string_lossy().replace('\\', "/")
}

/// Run a step and require it to succeed: a non-zero exit is the step's own failure, never a value
/// the caller might forget to check. `secret` is the code, so it is redacted out of any output.
async fn run_ok(
    box_control: &dyn BoxControl,
    step: BoxStep,
    command: &str,
    timeout_seconds: u64,
    secret: &str,
) -> Result<ExecResult, EnrollError> {
    let result = box_control.run(step, command, timeout_seconds).await.map_err(EnrollError::Provider)?;
    if result.exit_code != Some(0) {
        return Err(EnrollError::Step { step, detail: command_summary(&result, secret) });
    }
    Ok(result)
}

async fn read_slot_file(path: PathBuf, action: &'static str) -> Result<Vec<u8>, EnrollError> {
    let message_path = path.display().to_string();
    let read = crate::ipc::run_blocking(move || Ok::<_, crate::ipc::IpcError>(std::fs::read(&path)))
        .await
        .map_err(|error| EnrollError::Io { action, message: error.message })?;
    read.map_err(|error| EnrollError::Io {
        action,
        message: format!("{message_path}: {error}"),
    })
}

/// The code file's bytes: the code and one newline, which `--code-file` trims.
fn code_file_bytes(code: &str) -> Vec<u8> {
    let mut bytes = code.trim().as_bytes().to_vec();
    bytes.push(b'\n');
    bytes
}

/// The gateway config the box's daemon applies, serialized from the same struct the daemon
/// deserializes so a rename cannot drift.
fn relay_config_json(relay_origin: &str) -> Result<String, EnrollError> {
    use crate::remote::state::{RemoteGatewayConfig, RemoteNetworkMode, REMOTE_GATEWAY_PORT};

    let config = RemoteGatewayConfig {
        mode: RemoteNetworkMode::Relay,
        port: REMOTE_GATEWAY_PORT,
        allow_control: true,
        relay_url: Some(relay_origin.trim_end_matches('/').to_string()),
    };
    let mut value = serde_json::to_value(&config).map_err(|error| EnrollError::Io {
        action: "serialize the relay gateway config",
        message: error.to_string(),
    })?;
    // `RemoteGatewayConfig` has no restart-policy field: a persistent daemon always restores its
    // listener (`restart_policy()`), and serde ignores the extra key. It is sent because the wire
    // shape is documented in the plan, so a reader of a box's log sees the whole intent.
    if let Value::Object(object) = &mut value {
        object.insert("restartPolicy".to_string(), Value::String("restoreListener".to_string()));
    }
    serde_json::to_string(&value).map_err(|error| EnrollError::Io {
        action: "serialize the relay gateway config",
        message: error.to_string(),
    })
}

/// A failed step's detail: exit code, timeout and the tail of the output, with the enrollment code
/// redacted out of it whatever the box printed.
fn command_summary(result: &ExecResult, code: &str) -> String {
    let mut detail = match result.exit_code {
        Some(code) => format!("exit {code}"),
        None => "no exit code".to_string(),
    };
    if result.timed_out {
        detail.push_str(" (timed out)");
    }
    let stderr = result.stderr.trim();
    if !stderr.is_empty() {
        detail.push_str("; stderr: ");
        detail.push_str(&tail(stderr, 400));
    }
    let stdout = result.stdout.trim();
    if !stdout.is_empty() {
        detail.push_str("; stdout: ");
        detail.push_str(&tail(stdout, 200));
    }
    redact(&detail, code)
}

fn tail(text: &str, characters: usize) -> String {
    let count = text.chars().count();
    if count <= characters {
        return text.to_string();
    }
    text.chars().skip(count - characters).collect()
}

fn redact(text: &str, secret: &str) -> String {
    let secret = secret.trim();
    if secret.is_empty() {
        return text.to_string();
    }
    text.replace(secret, "<redacted>")
}

/// Resolve the box's `ferryx-cli` into `$FX_CLI`.
///
/// The bootstrap installs the pinned client under `$HOME/.local/share/ferryx-cloud/relay/<sha>/ferryx-cli`;
/// `$FERRYX_CLI` and `PATH` are accepted too, so a hand-made box and a bootstrap box both work.
/// Exiting 127 here is what makes a missing client a named failure instead of "command not found".
const RESOLVE_CLI: &str = "FX_CLI=''; for candidate in \"${FERRYX_CLI:-}\" \"$HOME\"/.local/share/ferryx-cloud/relay/*/ferryx-cli; do [ -n \"$candidate\" ] && [ -x \"$candidate\" ] && { FX_CLI=\"$candidate\"; break; }; done; [ -n \"$FX_CLI\" ] || FX_CLI=\"$(command -v ferryx-cli 2>/dev/null || true)\"; [ -n \"$FX_CLI\" ] || { echo 'ferryx-cli-not-found' >&2; exit 127; };";

/// The box-side helper: wait for the daemon socket, handshake, then `remoteConfigure`.
///
/// It has to be a file: `exec` is one shell string with no stdin, and a shell cannot open a unix
/// socket. `python3` is the one interpreter every in-scope image has.
const CONFIGURE_SCRIPT: &str = r#"import json, socket, sys, time

def exchange(reader, writer, request):
    writer.write((json.dumps(request) + "\n").encode("utf-8"))
    writer.flush()
    line = reader.readline()
    if not line:
        raise RuntimeError("the daemon closed the connection")
    return json.loads(line.decode("utf-8"))

def main():
    sock_path, version, config_json, wait = sys.argv[1], int(sys.argv[2]), sys.argv[3], float(sys.argv[4])
    deadline = time.time() + wait
    connection = None
    while connection is None:
        try:
            candidate = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            candidate.settimeout(60.0)
            candidate.connect(sock_path)
            connection = candidate
        except OSError as error:
            if time.time() >= deadline:
                print("daemon-socket-unavailable at %s: %s" % (sock_path, error), file=sys.stderr)
                return 1
            time.sleep(0.2)
    reader = connection.makefile("rb")
    writer = connection.makefile("wb")
    hello = exchange(reader, writer, {"type": "handshake", "version": version})
    if hello.get("type") != "handshakeOk":
        print("handshake-refused: %s" % json.dumps(hello), file=sys.stderr)
        return 1
    answer = exchange(reader, writer, {"type": "remoteConfigure", "config": json.loads(config_json)})
    if answer.get("type") != "remoteConfigureOk":
        print("remoteConfigure-refused: %s" % json.dumps(answer), file=sys.stderr)
        return 1
    print("remoteConfigureOk")
    return 0

sys.exit(main())
"#;

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use parking_lot::Mutex;
    use serde_json::json;

    use super::*;
    use crate::cloud_host::provider::fake::FakeProvider;
    use crate::cloud_host::provider::{Capabilities, CreateSandbox, ErrorKind, ListedSandbox, SandboxInfo};
    use crate::cloud_host::slots::{self, SlotRegistry};

    /// The one-use code the fake account hands out. It must never appear in a command string.
    const SECRET_CODE: &str = "enrollment-code-must-never-reach-argv";
    /// The slot's private key. It must never appear in a command string either.
    const PLANTED_PRIVATE_KEY: &str = "planted-private-key-must-never-be-printed";
    /// The `$HOME` probe, byte for byte, so the box shell can recognise it.
    const HOME_PROBE: &str = "printf %s \"$HOME\"";

    /// Split out the single-quoted arguments of a command, which is where every path and every
    /// JSON config travels. Enough for this module's commands, and it refuses nothing: a `'` that
    /// never closes simply ends the scan.
    fn quoted(command: &str) -> Vec<String> {
        let mut parts = Vec::new();
        let mut rest = command;
        while let Some(start) = rest.find('\'') {
            let after = &rest[start + 1..];
            match after.find('\'') {
                Some(end) => {
                    parts.push(after[..end].to_string());
                    rest = &after[end + 1..];
                }
                None => break,
            }
        }
        parts
    }

    /// Which step a command is, for the provider-level double that does not get told.
    fn step_of(command: &str) -> Option<BoxStep> {
        if command == HOME_PROBE {
            Some(BoxStep::Home)
        } else if command.contains("account enroll") {
            Some(BoxStep::Enroll)
        } else if command.contains("--daemon") {
            Some(BoxStep::StartDaemon)
        } else if command.contains("python3") {
            Some(BoxStep::ConfigureRelay)
        } else if command.starts_with("rm -f ") {
            Some(BoxStep::RemoveCode)
        } else if command.contains("chmod 700") {
            Some(BoxStep::Prepare)
        } else {
            None
        }
    }

    struct Write {
        path: String,
        mode: u32,
        bytes: Vec<u8>,
    }

    /// A box with a small filesystem, so `rm -f` and "does the code file exist" mean something.
    ///
    /// It answers only the commands this module sends: an unrecognised command exits 127, so a
    /// change to a command string breaks the test that sends it instead of passing quietly.
    #[derive(Default)]
    struct FakeBox {
        writes: Mutex<Vec<Write>>,
        steps: Mutex<Vec<(BoxStep, String)>>,
        files: Mutex<BTreeMap<String, Vec<u8>>>,
        write_failure_at: Mutex<Option<usize>>,
        /// Index of a write that lands its bytes and *then* fails, the way the T2 providers do
        /// (upload, then chmod; an `Err` with the content already on disk).
        partial_write_at: Mutex<Option<usize>>,
        step_failure: Mutex<BTreeMap<BoxStep, i32>>,
        home: Mutex<String>,
        /// Whether the code file existed when `account enroll` ran.
        enroll_saw_code: Mutex<Option<bool>>,
        /// Whether the configure helper existed when python3 was asked to run it.
        configure_saw_script: Mutex<Option<bool>>,
    }

    impl FakeBox {
        fn new() -> Self {
            let fake = Self::default();
            *fake.home.lock() = "/home/tester".to_string();
            fake
        }

        fn with_home(home: &str) -> Self {
            let fake = Self::new();
            *fake.home.lock() = home.to_string();
            fake
        }

        /// Fail the `index`-th write (1-based) before any byte lands.
        fn fail_write_at(&self, index: usize) {
            *self.write_failure_at.lock() = Some(index);
        }

        /// Fail the `index`-th write (1-based) *after* its bytes landed, at the un-chmodded mode an
        /// upload leaves behind - `provider/e2b.rs` and `provider/blaxel.rs` both return
        /// `chmod_failed` with the file already written.
        fn fail_write_after_upload_at(&self, index: usize) {
            *self.partial_write_at.lock() = Some(index);
        }

        fn fail_step(&self, step: BoxStep, exit_code: i32) {
            self.step_failure.lock().insert(step, exit_code);
        }

        fn steps(&self) -> Vec<BoxStep> {
            self.steps.lock().iter().map(|(step, _)| *step).collect()
        }

        fn commands(&self) -> Vec<String> {
            self.steps.lock().iter().map(|(_, command)| command.clone()).collect()
        }

        fn command_for(&self, step: BoxStep) -> Option<String> {
            self.steps
                .lock()
                .iter()
                .find(|(seen, _)| *seen == step)
                .map(|(_, command)| command.clone())
        }

        fn writes(&self) -> Vec<(String, u32, Vec<u8>)> {
            self.writes
                .lock()
                .iter()
                .map(|write| (write.path.clone(), write.mode, write.bytes.clone()))
                .collect()
        }

        fn path_with_mode(&self, mode: u32) -> Vec<String> {
            self.writes
                .lock()
                .iter()
                .filter(|write| write.mode == mode)
                .map(|write| write.path.clone())
                .collect()
        }

        fn leaves_a_code_file(&self) -> bool {
            self.files.lock().keys().any(|path| path.ends_with(CODE_FILE))
        }

        fn file_contents(&self, path: &str) -> Option<Vec<u8>> {
            self.files.lock().get(path).cloned()
        }

        /// The bytes of a write this box was sent, whether or not the file still exists: the code is
        /// written and then deleted, so its content has to be read from the write log.
        fn written_bytes(&self, path: &str) -> Option<Vec<u8>> {
            self.writes
                .lock()
                .iter()
                .find(|write| write.path == path)
                .map(|write| write.bytes.clone())
        }
    }

    #[async_trait::async_trait]
    impl BoxControl for FakeBox {
        async fn write(&self, path: &str, bytes: Vec<u8>, mode: u32) -> Result<(), SandboxError> {
            let index = self.writes.lock().len() + 1;
            if *self.write_failure_at.lock() == Some(index) {
                return Err(SandboxError::new(
                    ErrorKind::Retryable,
                    "fake-box",
                    "write_failed",
                    format!("injected write failure at {path}"),
                ));
            }
            if *self.partial_write_at.lock() == Some(index) {
                // The bytes are on disk, the requested mode never was applied.
                self.writes.lock().push(Write {
                    path: path.to_string(),
                    mode: 0o644,
                    bytes: bytes.clone(),
                });
                self.files.lock().insert(path.to_string(), bytes);
                return Err(SandboxError::new(
                    ErrorKind::Fatal,
                    "fake-box",
                    "chmod_failed",
                    format!("the file was written but chmod failed: {path}"),
                ));
            }
            self.writes.lock().push(Write { path: path.to_string(), mode, bytes: bytes.clone() });
            self.files.lock().insert(path.to_string(), bytes);
            Ok(())
        }

        async fn run(
            &self,
            step: BoxStep,
            command: &str,
            _timeout_seconds: u64,
        ) -> Result<ExecResult, SandboxError> {
            self.steps.lock().push((step, command.to_string()));
            if let Some(exit_code) = self.step_failure.lock().get(&step).copied() {
                return Ok(ExecResult {
                    exit_code: Some(exit_code),
                    stdout: String::new(),
                    stderr: format!("{} step refused", step.as_str()),
                    timed_out: false,
                });
            }
            let mut result = ExecResult {
                exit_code: Some(0),
                stdout: String::new(),
                stderr: String::new(),
                timed_out: false,
            };
            let paths = quoted(command);
            match step {
                BoxStep::Home => result.stdout = self.home.lock().clone(),
                BoxStep::Prepare => {
                    // `mkdir -p <dir> && chmod 700 <dir> && rm -f <code>`: a stale code goes.
                    for path in paths.iter().filter(|path| path.ends_with(CODE_FILE)) {
                        self.files.lock().remove(path);
                    }
                }
                BoxStep::Enroll => {
                    // The enroll step must run with the code file already on the box: `--code-file`
                    // names a path, and a path to nothing would fail here rather than in production.
                    let code_exists = paths
                        .iter()
                        .filter(|path| path.ends_with(CODE_FILE))
                        .any(|path| self.files.lock().contains_key(path));
                    *self.enroll_saw_code.lock() = Some(code_exists);
                }
                BoxStep::StartDaemon => {}
                BoxStep::ConfigureRelay => {
                    let script = paths.first().cloned().unwrap_or_default();
                    let script_exists = self.files.lock().contains_key(&script);
                    *self.configure_saw_script.lock() = Some(script_exists);
                    result.stdout = "remoteConfigureOk".to_string();
                }
                BoxStep::RemoveCode => {
                    for path in &paths {
                        self.files.lock().remove(path);
                    }
                }
            }
            Ok(result)
        }
    }

    /// The production path: a [`SandboxProvider`] whose `exec` and `write_file` land on a
    /// [`FakeBox`]. This is what makes `provision_relay` - the function `cloud create` calls -
    /// testable without a real box, and it answers 127 to any command it does not recognise.
    struct FileProvider {
        box_control: FakeBox,
        inner: FakeProvider,
    }

    impl FileProvider {
        fn new() -> Self {
            Self { box_control: FakeBox::new(), inner: FakeProvider::new() }
        }

        async fn ready_machine(&self) -> String {
            let id = self
                .inner
                .create(CreateSandbox {
                    idempotency_key: "fx-a-1".into(),
                    name: "fx-a".into(),
                    ttl_seconds: None,
                    env: BTreeMap::new(),
                })
                .await
                .expect("create");
            self.inner.get(&id).await.expect("promote to ready");
            id
        }
    }

    #[async_trait::async_trait]
    impl SandboxProvider for FileProvider {
        fn capabilities(&self) -> Capabilities {
            self.inner.capabilities()
        }

        async fn create(&self, request: CreateSandbox) -> Result<String, SandboxError> {
            self.inner.create(request).await
        }

        async fn get(&self, sandbox_id: &str) -> Result<SandboxInfo, SandboxError> {
            self.inner.get(sandbox_id).await
        }

        async fn stop(&self, sandbox_id: &str) -> Result<(), SandboxError> {
            self.inner.stop(sandbox_id).await
        }

        async fn resume(
            &self,
            sandbox_id: &str,
            ttl_seconds: Option<u64>,
        ) -> Result<(), SandboxError> {
            self.inner.resume(sandbox_id, ttl_seconds).await
        }

        async fn delete(&self, sandbox_id: &str) -> Result<(), SandboxError> {
            self.inner.delete(sandbox_id).await
        }

        async fn extend_ttl(
            &self,
            sandbox_id: &str,
            ttl_seconds: Option<u64>,
        ) -> Result<(), SandboxError> {
            self.inner.extend_ttl(sandbox_id, ttl_seconds).await
        }

        async fn write_file(
            &self,
            _sandbox_id: &str,
            path: &str,
            bytes: Vec<u8>,
            mode: u32,
        ) -> Result<(), SandboxError> {
            self.box_control.write(path, bytes, mode).await
        }

        async fn exec(
            &self,
            _sandbox_id: &str,
            command: &str,
            timeout_seconds: u64,
        ) -> Result<ExecResult, SandboxError> {
            let Some(step) = step_of(command) else {
                // A command this double does not know is a drift in the flow, and it says so.
                return Ok(ExecResult {
                    exit_code: Some(127),
                    stdout: String::new(),
                    stderr: format!("unrecognised box command: {command}"),
                    timed_out: false,
                });
            };
            self.box_control.run(step, command, timeout_seconds).await
        }

        async fn list(&self) -> Result<Vec<ListedSandbox>, SandboxError> {
            self.inner.list().await
        }
    }

    struct FakeIssuer {
        code: String,
        issued: AtomicUsize,
    }

    impl FakeIssuer {
        fn new(code: &str) -> Self {
            Self { code: code.to_string(), issued: AtomicUsize::new(0) }
        }

        fn issued(&self) -> usize {
            self.issued.load(Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl EnrollmentCodeIssuer for FakeIssuer {
        async fn issue_code(&self) -> Result<String, EnrollError> {
            self.issued.fetch_add(1, Ordering::SeqCst);
            Ok(self.code.clone())
        }
    }

    struct FakeProver {
        offline_calls: usize,
        calls: AtomicUsize,
    }

    impl FakeProver {
        /// Offline for the first `offline_calls` allocations, reachable afterwards.
        fn offline_for(offline_calls: usize) -> Self {
            Self { offline_calls, calls: AtomicUsize::new(0) }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl AttachProver for FakeProver {
        async fn prove_attach(
            &self,
            record_id: &str,
            machine_id: &str,
        ) -> Result<Reachability, EnrollError> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            if call <= self.offline_calls {
                return Err(EnrollError::Unreachable {
                    record_id: record_id.to_string(),
                    machine_id: machine_id.to_string(),
                    detail: "409 MACHINE_OFFLINE".to_string(),
                    retryable: true,
                });
            }
            Ok(Reachability {
                session_id: format!("session-{call}"),
                machine_id: machine_id.to_string(),
                relay_origin: RELAY_ORIGIN.to_string(),
            })
        }
    }

    /// A pool under the test's temp dir holding one record, shaped exactly like the real
    /// `~/.ferryx/cloud-slots/<record-id>/` that task 18 left on disk.
    async fn lease(dir: &tempfile::TempDir, record: &str, name: &str) -> (SlotRegistry, SlotLease) {
        let registry = SlotRegistry::at(slots::location_in(dir.path()));
        let slot = registry.root().join(record);
        std::fs::create_dir_all(&slot).unwrap();
        std::fs::write(
            slot.join(IDENTITY_FILE),
            format!(
                "{{\"machineId\":\"{record}-machine\",\"displayName\":null,\
                 \"publicKey\":\"cHVibGljLWtleQ==\",\"privateKey\":\"{PLANTED_PRIVATE_KEY}\"}}"
            ),
        )
        .unwrap();
        std::fs::write(
            slot.join(ATTACH_IDENTITY_FILE),
            "{\"publicKey\":\"YXR0YWNoLXB1YmxpYw==\",\"privateKey\":\"YXR0YWNoLXByaXY=\"}",
        )
        .unwrap();
        let lease = registry.lease(name).await.expect("lease the record");
        (registry, lease)
    }

    fn remote_path(file: &str) -> String {
        // Written out literally on purpose: the box's daemon and `ferryx-cli account enroll` read
        // `$HOME/.ferryx/remote`, so deriving this from a production constant would let that
        // constant slip to `$HOME/remote` with every assertion here still green, while the box
        // read nothing and enrolled a fresh machineId from the one-use code.
        format!("/home/tester/.ferryx/remote/{file}")
    }

    fn code_path() -> String {
        remote_path(CODE_FILE)
    }

    // ---- the happy path -----------------------------------------------------------------------

    #[tokio::test]
    async fn provisioning_writes_the_record_enrolls_and_configures_the_daemon() {
        let dir = tempfile::tempdir().unwrap();
        let (_registry, lease) = lease(&dir, "record-1", "fx-a").await;
        let fake = FakeBox::new();
        let issuer = FakeIssuer::new(SECRET_CODE);

        let provisioned =
            provision(&fake, &lease, &issuer, RELAY_ORIGIN).await.expect("provision");

        assert_eq!(provisioned.record_id, "record-1");
        assert_eq!(provisioned.machine_id, "record-1-machine");
        assert_eq!(provisioned.relay_origin, RELAY_ORIGIN);
        assert_eq!(issuer.issued(), 1, "exactly one code is minted");

        // The three secret-bearing files are 0600, under the box's own `$HOME`; the helper that
        // carries no secret is 0700 because it is executed.
        assert_eq!(
            fake.path_with_mode(0o600),
            vec![
                remote_path(IDENTITY_FILE),
                remote_path(ATTACH_IDENTITY_FILE),
                code_path(),
            ]
        );
        assert_eq!(fake.path_with_mode(0o700), vec![remote_path(CONFIGURE_SCRIPT_FILE)]);

        let identity = fake.file_contents(&remote_path(IDENTITY_FILE)).expect("identity written");
        assert!(String::from_utf8_lossy(&identity).contains("record-1-machine"));
        let attach = fake.file_contents(&remote_path(ATTACH_IDENTITY_FILE)).expect("attach written");
        assert!(String::from_utf8_lossy(&attach).contains("YXR0YWNoLXB1YmxpYw=="));
        let code = fake
            .written_bytes(&code_path())
            .expect("the code was written, even though it is gone by the end");
        assert_eq!(String::from_utf8_lossy(&code), format!("{SECRET_CODE}\n"));

        // The order is the order the flow documents, and the code is removed after enrolling.
        assert_eq!(
            fake.steps(),
            vec![
                BoxStep::Home,
                BoxStep::Prepare,
                BoxStep::Enroll,
                BoxStep::StartDaemon,
                BoxStep::ConfigureRelay,
                BoxStep::RemoveCode,
            ]
        );
        // The enroll step ran with the code file already on the box, and the configure step with
        // its helper already written: both are what make the steps meaningful.
        assert_eq!(*fake.enroll_saw_code.lock(), Some(true));
        assert_eq!(*fake.configure_saw_script.lock(), Some(true));
        assert!(!fake.leaves_a_code_file(), "the one-use code does not stay on the box");

        let enroll = fake.command_for(BoxStep::Enroll).expect("an enroll command");
        assert!(enroll.contains("account enroll"), "{enroll}");
        assert!(enroll.contains("--code-file"), "{enroll}");
        assert!(enroll.contains("--origin"), "{enroll}");
        assert!(enroll.contains(RELAY_ORIGIN), "{enroll}");
        assert!(enroll.contains(&code_path()), "the path travels, never the code: {enroll}");
        assert!(!enroll.contains(SECRET_CODE), "{enroll}");
    }

    #[tokio::test]
    async fn no_command_string_carries_the_code_or_a_key() {
        let dir = tempfile::tempdir().unwrap();
        let (_registry, lease) = lease(&dir, "record-1", "fx-a").await;
        let fake = FakeBox::new();

        provision(&fake, &lease, &FakeIssuer::new(SECRET_CODE), RELAY_ORIGIN)
            .await
            .expect("provision");

        for command in fake.commands() {
            assert!(!command.contains(SECRET_CODE), "the code reached argv: {command}");
            assert!(!command.contains(PLANTED_PRIVATE_KEY), "a key reached argv: {command}");
            assert!(!command.contains("cHVibGljLWtleQ=="), "a key reached argv: {command}");
            assert!(!command.to_lowercase().contains("privatekey"), "{command}");
        }
        // Only the three steps that must name the code's path do.
        let mentioning: Vec<BoxStep> = fake
            .steps
            .lock()
            .iter()
            .filter(|(_, command)| command.contains(&code_path()))
            .map(|(step, _)| *step)
            .collect();
        assert_eq!(
            mentioning,
            vec![BoxStep::Prepare, BoxStep::Enroll, BoxStep::RemoveCode],
            "the code's path reaches only the steps that need it"
        );
    }

    #[tokio::test]
    async fn the_configure_payload_names_the_relay_the_control_and_the_protocol() {
        let dir = tempfile::tempdir().unwrap();
        let (_registry, lease) = lease(&dir, "record-1", "fx-a").await;
        let fake = FakeBox::new();

        provision(&fake, &lease, &FakeIssuer::new(SECRET_CODE), RELAY_ORIGIN)
            .await
            .expect("provision");

        let configure = fake.command_for(BoxStep::ConfigureRelay).expect("a configure command");
        // The config is one quoted JSON literal in argv; the daemon reads exactly these keys.
        assert!(configure.contains("\"mode\":\"relay\""), "{configure}");
        assert!(configure.contains("\"allowControl\":true"), "{configure}");
        assert!(configure.contains("\"relayUrl\":\"https://relay.ferryx.dev\""), "{configure}");
        assert!(configure.contains("\"restartPolicy\":\"restoreListener\""), "{configure}");
        // The handshake the helper sends is the protocol this build speaks.
        assert!(
            configure.contains(&crate::daemon::protocol::DAEMON_PROTOCOL_VERSION.to_string()),
            "{configure}"
        );
        // The helper is removed by the same command that runs it.
        assert!(configure.contains(&remote_path(CONFIGURE_SCRIPT_FILE)), "{configure}");
        assert!(configure.contains("daemon.sock"), "{configure}");
        assert!(configure.contains("FERRYX_RUNTIME_DIR"), "{configure}");
    }

    // ---- failure paths, and what each leaves behind -------------------------------------------

    #[tokio::test]
    async fn an_enroll_failure_still_removes_the_code_and_keeps_the_lease() {
        let dir = tempfile::tempdir().unwrap();
        let (registry, lease) = lease(&dir, "record-1", "fx-a").await;
        let fake = FakeBox::new();
        fake.fail_step(BoxStep::Enroll, 3);

        let error = provision(&fake, &lease, &FakeIssuer::new(SECRET_CODE), RELAY_ORIGIN)
            .await
            .expect_err("the box refused the code");

        assert_eq!(error.code(), "ENROLL_FAILED");
        assert!(error.to_string().starts_with("ENROLL_FAILED:"), "{error}");
        assert!(error.to_string().contains("enroll step failed"), "{error}");
        assert_eq!(error.exit_code(), 1);
        assert!(!fake.leaves_a_code_file(), "a failed enroll must not leave the code behind");
        assert!(fake.steps().contains(&BoxStep::RemoveCode));
        assert!(!fake.steps().contains(&BoxStep::StartDaemon), "no daemon after a failed enroll");
        // Provisioning never releases the lease: that is the lifecycle's decision.
        assert!(
            registry.lease_of("fx-a").await.unwrap().is_some(),
            "the record stays with the machine that holds it"
        );
    }

    #[tokio::test]
    async fn a_daemon_that_will_not_start_removes_the_code_and_names_the_step() {
        let dir = tempfile::tempdir().unwrap();
        let (_registry, lease) = lease(&dir, "record-1", "fx-a").await;
        let fake = FakeBox::new();
        fake.fail_step(BoxStep::StartDaemon, 1);

        let error = provision(&fake, &lease, &FakeIssuer::new(SECRET_CODE), RELAY_ORIGIN)
            .await
            .expect_err("the daemon did not start");

        assert_eq!(error.code(), "DAEMON_CONFIGURE_FAILED");
        assert!(error.to_string().contains("start_daemon"), "{error}");
        assert!(!fake.leaves_a_code_file());
        assert!(!fake.steps().contains(&BoxStep::ConfigureRelay));
    }

    #[tokio::test]
    async fn a_configure_refusal_after_a_successful_enroll_still_removes_the_code() {
        let dir = tempfile::tempdir().unwrap();
        let (_registry, lease) = lease(&dir, "record-1", "fx-a").await;
        let fake = FakeBox::new();
        fake.fail_step(BoxStep::ConfigureRelay, 1);

        let error = provision(&fake, &lease, &FakeIssuer::new(SECRET_CODE), RELAY_ORIGIN)
            .await
            .expect_err("the daemon refused the configure");

        assert_eq!(error.code(), "DAEMON_CONFIGURE_FAILED");
        assert!(error.to_string().contains("configure_relay"), "{error}");
        assert!(!fake.leaves_a_code_file(), "the code is gone even when the daemon failed");
        assert!(fake.steps().contains(&BoxStep::Enroll), "the enroll had already run");
    }

    #[tokio::test]
    async fn a_code_that_could_not_be_removed_is_reported_as_such() {
        let dir = tempfile::tempdir().unwrap();
        let (_registry, lease) = lease(&dir, "record-1", "fx-a").await;
        let fake = FakeBox::new();
        // The enrollment and the daemon both succeed; only the cleanup fails.
        fake.fail_step(BoxStep::RemoveCode, 1);

        let error = provision(&fake, &lease, &FakeIssuer::new(SECRET_CODE), RELAY_ORIGIN)
            .await
            .expect_err("a code left on the box is a failure");

        assert_eq!(error.code(), "CODE_FILE_NOT_REMOVED");
        assert!(error.to_string().contains("remove_code"), "{error}");
        assert!(fake.steps().contains(&BoxStep::ConfigureRelay), "the daemon was configured");
    }

    #[tokio::test]
    async fn a_step_failure_and_a_stuck_code_are_both_reported() {
        let dir = tempfile::tempdir().unwrap();
        let (_registry, lease) = lease(&dir, "record-1", "fx-a").await;
        let fake = FakeBox::new();
        fake.fail_step(BoxStep::ConfigureRelay, 1);
        fake.fail_step(BoxStep::RemoveCode, 1);

        let error = provision(&fake, &lease, &FakeIssuer::new(SECRET_CODE), RELAY_ORIGIN)
            .await
            .expect_err("both the daemon and the cleanup failed");

        // The step failure is the code, and the leftover credential is not lost.
        assert_eq!(error.code(), "DAEMON_CONFIGURE_FAILED");
        assert!(error.to_string().contains("CODE_FILE_NOT_REMOVED"), "{error}");
    }

    #[tokio::test]
    async fn a_failed_identity_write_mints_no_code() {
        let dir = tempfile::tempdir().unwrap();
        let (_registry, lease) = lease(&dir, "record-1", "fx-a").await;
        let fake = FakeBox::new();
        fake.fail_write_at(1);
        let issuer = FakeIssuer::new(SECRET_CODE);

        let error = provision(&fake, &lease, &issuer, RELAY_ORIGIN)
            .await
            .expect_err("the identity could not be written");

        assert_eq!(error.code(), "PROVIDER_ERROR");
        assert_eq!(issuer.issued(), 0, "no code is minted for a box that cannot hold the record");
        assert!(!fake.leaves_a_code_file());
        assert!(!fake.steps().contains(&BoxStep::Enroll));
        assert!(fake.writes().is_empty(), "the refused write is not a write");
    }

    #[tokio::test]
    async fn a_failed_code_write_leaves_the_code_file_out_and_enrolls_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let (_registry, lease) = lease(&dir, "record-1", "fx-a").await;
        let fake = FakeBox::new();
        // The third write is the code: the identity and the attach keypair are already on the box.
        fake.fail_write_at(3);
        let issuer = FakeIssuer::new(SECRET_CODE);

        let error = provision(&fake, &lease, &issuer, RELAY_ORIGIN)
            .await
            .expect_err("the code could not be written");

        assert_eq!(error.code(), "PROVIDER_ERROR");
        assert_eq!(issuer.issued(), 1, "the code was minted; it expires unused");
        assert!(!fake.leaves_a_code_file());
        assert!(!fake.steps().contains(&BoxStep::Enroll));
        assert_eq!(fake.path_with_mode(0o600), vec![
            remote_path(IDENTITY_FILE),
            remote_path(ATTACH_IDENTITY_FILE),
        ]);
    }

    /// Every path the flow writes is spelled out literally here, not built from a production
    /// constant, because the box's daemon and `ferryx-cli account enroll` read one specific
    /// directory: `$HOME/.ferryx/remote`. A record left anywhere else is a record the box never
    /// reads, so enroll would generate a fresh machineId and burn the one-use code for nothing.
    #[tokio::test]
    async fn the_record_lands_in_the_directory_the_box_daemon_reads() {
        let dir = tempfile::tempdir().unwrap();
        let (_registry, lease) = lease(&dir, "record-1", "fx-a").await;
        let fake = FakeBox::new();

        provision(&fake, &lease, &FakeIssuer::new(SECRET_CODE), RELAY_ORIGIN)
            .await
            .expect("provision");

        assert_eq!(
            fake.writes().iter().map(|(path, _, _)| path.clone()).collect::<Vec<_>>(),
            vec![
                "/home/tester/.ferryx/remote/identity.json".to_string(),
                "/home/tester/.ferryx/remote/attach-identity.json".to_string(),
                "/home/tester/.ferryx/remote/.enroll-code".to_string(),
                "/home/tester/.ferryx/remote/.configure-relay.py".to_string(),
            ],
            "the record must land in $HOME/.ferryx/remote, where the box reads it"
        );
        // The describe/rm commands name the same directory, so nothing is prepared or cleaned up
        // somewhere else than the files were written.
        let prepare = fake.command_for(BoxStep::Prepare).expect("a prepare command");
        assert!(prepare.contains("'/home/tester/.ferryx/remote'"), "{prepare}");
        assert_eq!(
            fake.command_for(BoxStep::RemoveCode).as_deref(),
            Some("rm -f '/home/tester/.ferryx/remote/.enroll-code'"),
            "the one-use code is deleted from where it was written"
        );
        assert_eq!(fake.steps()[0..2], [BoxStep::Home, BoxStep::Prepare]);
    }

    /// `$HOME` is the box's, not this Mac's, and the layout under it is the canonical one whatever
    /// the home directory happens to be.
    #[test]
    fn the_box_remote_dir_is_the_canonical_layout_under_the_box_home() {
        assert_eq!(box_remote_dir("/home/tester"), "/home/tester/.ferryx/remote");
        assert_eq!(box_remote_dir("/home/tester/"), "/home/tester/.ferryx/remote");
        assert_eq!(box_remote_dir("/root"), "/root/.ferryx/remote");
        // A root-owned box: never a relative path.
        assert_eq!(box_remote_dir("/"), "/.ferryx/remote");
        // The canonical resolver is the authority; this module must not carry its own copy of the
        // layout that could drift from it.
        let canonical = crate::remote::auth::resolve_canonical_remote_dir(|variable| {
            (variable == "HOME").then(|| std::ffi::OsString::from("/home/tester"))
        })
        .expect("resolve");
        assert_eq!(box_remote_dir("/home/tester"), canonical.to_string_lossy().as_ref());
    }

    /// A `write_file` that lands its bytes and then fails its `chmod` is exactly what the T2
    /// providers do, and it leaves the secret on the box at 0644. The code must not survive it.
    #[tokio::test]
    async fn a_code_write_that_fails_after_uploading_still_removes_the_code() {
        let dir = tempfile::tempdir().unwrap();
        let (_registry, lease) = lease(&dir, "record-1", "fx-a").await;
        let fake = FakeBox::new();
        // The third write is the code; its bytes land and the chmod fails.
        fake.fail_write_after_upload_at(3);
        let issuer = FakeIssuer::new(SECRET_CODE);

        let error = provision(&fake, &lease, &issuer, RELAY_ORIGIN)
            .await
            .expect_err("the code's mode could not be applied");

        assert_eq!(error.code(), "PROVIDER_ERROR");
        assert!(error.to_string().contains("chmod_failed"), "{error}");
        assert!(!fake.leaves_a_code_file(), "a half-written code must be removed, not left at 0644");
        assert_eq!(
            fake.command_for(BoxStep::RemoveCode).as_deref(),
            Some("rm -f '/home/tester/.ferryx/remote/.enroll-code'"),
            "the removal names the file that may have landed"
        );
        assert!(!fake.steps().contains(&BoxStep::Enroll), "the code never reached enroll");
    }

    /// The same shape on the record's own files: both private keys ate 0644 would otherwise stay on
    /// a shared box.
    #[tokio::test]
    async fn an_identity_write_that_fails_after_uploading_is_removed_again() {
        let dir = tempfile::tempdir().unwrap();
        let (_registry, lease) = lease(&dir, "record-1", "fx-a").await;
        let fake = FakeBox::new();
        fake.fail_write_after_upload_at(1);
        let issuer = FakeIssuer::new(SECRET_CODE);

        let error = provision(&fake, &lease, &issuer, RELAY_ORIGIN)
            .await
            .expect_err("the identity's mode could not be applied");

        assert_eq!(error.code(), "PROVIDER_ERROR");
        assert!(!fake.files.lock().contains_key(&remote_path(IDENTITY_FILE)), "the key must not stay");
        assert_eq!(issuer.issued(), 0, "a box that cannot hold the record receives no code");
        assert!(!fake.steps().contains(&BoxStep::Enroll));
    }

    /// When the cleanup after a failed write fails too, the operator learns both: the write broke
    /// and a secret may still be on the box.
    #[tokio::test]
    async fn a_failed_cleanup_after_a_failed_write_is_reported_with_it() {
        let dir = tempfile::tempdir().unwrap();
        let (_registry, lease) = lease(&dir, "record-1", "fx-a").await;
        let fake = FakeBox::new();
        fake.fail_write_after_upload_at(3);
        fake.fail_step(BoxStep::RemoveCode, 1);

        let error = provision(&fake, &lease, &FakeIssuer::new(SECRET_CODE), RELAY_ORIGIN)
            .await
            .expect_err("the code could not be written or removed");

        assert_eq!(error.code(), "PROVIDER_ERROR", "the write is the failure the operator fixes");
        assert!(error.to_string().contains("chmod_failed"), "{error}");
        assert!(error.to_string().contains("may still be on the box"), "{error}");
        assert!(
            error.to_string().contains("/home/tester/.ferryx/remote/.enroll-code"),
            "the leftover is named: {error}"
        );
    }

    #[tokio::test]
    async fn an_unusable_home_is_refused_before_a_code_is_minted() {
        let dir = tempfile::tempdir().unwrap();
        let (_registry, lease) = lease(&dir, "record-1", "fx-a").await;
        let fake = FakeBox::with_home("relative/home");
        let issuer = FakeIssuer::new(SECRET_CODE);

        let error = provision(&fake, &lease, &issuer, RELAY_ORIGIN)
            .await
            .expect_err("a relative HOME cannot anchor a path");

        assert_eq!(error.code(), "BOX_STEP_FAILED");
        assert!(error.to_string().contains("HOME"), "{error}");
        assert_eq!(issuer.issued(), 0);
        assert!(fake.writes().is_empty(), "no file is written to a guessed path");
    }

    #[tokio::test]
    async fn a_prepare_failure_never_mints_a_code() {
        let dir = tempfile::tempdir().unwrap();
        let (_registry, lease) = lease(&dir, "record-1", "fx-a").await;
        let fake = FakeBox::new();
        fake.fail_step(BoxStep::Prepare, 1);
        let issuer = FakeIssuer::new(SECRET_CODE);

        let error =
            provision(&fake, &lease, &issuer, RELAY_ORIGIN).await.expect_err("prepare failed");

        assert_eq!(error.code(), "BOX_STEP_FAILED");
        assert_eq!(issuer.issued(), 0, "a directory that cannot be made mints nothing");
        assert!(fake.writes().is_empty());
    }

    // ---- the production wiring ----------------------------------------------------------------

    #[tokio::test]
    async fn provision_relay_drives_a_provider_through_exec_and_write_file() {
        let dir = tempfile::tempdir().unwrap();
        let (registry, lease) = lease(&dir, "record-1", "fx-a").await;
        let provider = FileProvider::new();
        let id = provider.ready_machine().await;

        let provisioned = provision_relay(&provider, &id, &lease, &FakeIssuer::new(SECRET_CODE))
            .await
            .expect("provision through the provider trait");

        assert_eq!(provisioned.record_id, "record-1");
        assert_eq!(provisioned.machine_id, "record-1-machine");
        assert_eq!(provider.box_control.steps(), vec![
            BoxStep::Home,
            BoxStep::Prepare,
            BoxStep::Enroll,
            BoxStep::StartDaemon,
            BoxStep::ConfigureRelay,
            BoxStep::RemoveCode,
        ]);
        assert!(!provider.box_control.leaves_a_code_file());
        // Nothing bypassed the trait: every command the provider saw is one this module sent, and
        // none of them carries the code.
        let commands = provider.box_control.commands();
        assert_eq!(commands.len(), 6, "{commands:?}");
        assert!(commands.iter().all(|command| !command.contains(SECRET_CODE)), "{commands:?}");
        assert!(registry.lease_of("fx-a").await.unwrap().is_some(), "the lease is untouched");
    }

    /// A provider that answers the `$HOME` probe but keeps `write_file` at its trait default, which
    /// refuses with `Config`. This is what a T1 (SSH) provider looks like to this module, and it is
    /// the evidence that the record travels through the file API and not through `exec`.
    struct NoFileApi {
        inner: FakeProvider,
        home: String,
        execs: Mutex<Vec<String>>,
    }

    impl NoFileApi {
        fn new() -> Self {
            Self {
                inner: FakeProvider::ssh(),
                home: "/home/tester".to_string(),
                execs: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait::async_trait]
    impl SandboxProvider for NoFileApi {
        fn capabilities(&self) -> Capabilities {
            self.inner.capabilities()
        }

        async fn create(&self, request: CreateSandbox) -> Result<String, SandboxError> {
            self.inner.create(request).await
        }

        async fn get(&self, sandbox_id: &str) -> Result<SandboxInfo, SandboxError> {
            self.inner.get(sandbox_id).await
        }

        async fn stop(&self, sandbox_id: &str) -> Result<(), SandboxError> {
            self.inner.stop(sandbox_id).await
        }

        async fn resume(
            &self,
            sandbox_id: &str,
            ttl_seconds: Option<u64>,
        ) -> Result<(), SandboxError> {
            self.inner.resume(sandbox_id, ttl_seconds).await
        }

        async fn delete(&self, sandbox_id: &str) -> Result<(), SandboxError> {
            self.inner.delete(sandbox_id).await
        }

        async fn extend_ttl(
            &self,
            sandbox_id: &str,
            ttl_seconds: Option<u64>,
        ) -> Result<(), SandboxError> {
            self.inner.extend_ttl(sandbox_id, ttl_seconds).await
        }

        async fn exec(
            &self,
            _sandbox_id: &str,
            command: &str,
            _timeout_seconds: u64,
        ) -> Result<ExecResult, SandboxError> {
            self.execs.lock().push(command.to_string());
            let stdout = if command == HOME_PROBE { self.home.clone() } else { String::new() };
            Ok(ExecResult { exit_code: Some(0), stdout, stderr: String::new(), timed_out: false })
        }

        async fn list(&self) -> Result<Vec<ListedSandbox>, SandboxError> {
            self.inner.list().await
        }
    }

    #[tokio::test]
    async fn a_provider_with_no_file_api_is_refused_after_the_home_probe() {
        let dir = tempfile::tempdir().unwrap();
        let (_registry, lease) = lease(&dir, "record-1", "fx-a").await;
        let provider = NoFileApi::new();
        let id = provider
            .create(CreateSandbox {
                idempotency_key: "fx-a-1".into(),
                name: "fx-a".into(),
                ttl_seconds: None,
                env: BTreeMap::new(),
            })
            .await
            .expect("create");
        provider.get(&id).await.expect("promote to ready");
        let issuer = FakeIssuer::new(SECRET_CODE);

        let error = provision_relay(&provider, &id, &lease, &issuer)
            .await
            .expect_err("this provider has no file API");

        assert_eq!(error.code(), "PROVIDER_ERROR");
        match &error {
            EnrollError::Provider(provided) => {
                assert_eq!(provided.kind, ErrorKind::Config);
                assert_eq!(provided.provider_code, "write_file_unsupported");
            }
            other => panic!("expected a provider error, got {other:?}"),
        }
        // Only the probe, the directory preparation and the removal of what the refused write may
        // have left ran: the record itself never reached the box, which is the whole reason the
        // file API is the only route a secret takes.
        let execs = provider.execs.lock().clone();
        assert_eq!(execs.len(), 3, "{execs:?}");
        assert_eq!(execs[0], HOME_PROBE);
        assert!(execs[2].starts_with("rm -f "), "{execs:?}");
        assert!(execs.iter().all(|command| !command.contains("account enroll")));
        assert_eq!(issuer.issued(), 0);
    }

    #[tokio::test]
    async fn provision_and_prove_does_both_through_the_provider() {
        let dir = tempfile::tempdir().unwrap();
        let (_registry, lease) = lease(&dir, "record-1", "fx-a").await;
        let provider = FileProvider::new();
        let id = provider.ready_machine().await;

        let reachability = provision_and_prove(
            &provider,
            &id,
            &lease,
            &FakeIssuer::new(SECRET_CODE),
            &FakeProver::offline_for(0),
            ProvePolicy::once(),
        )
        .await
        .expect("provision and prove");

        assert_eq!(reachability.session_id, "session-1");
        assert_eq!(reachability.machine_id, lease.machine_id());
        assert!(!provider.box_control.leaves_a_code_file());
    }

    // ---- the reachability proof ---------------------------------------------------------------

    #[tokio::test]
    async fn a_session_is_the_proof_of_reachability() {
        let dir = tempfile::tempdir().unwrap();
        let (_registry, lease) = lease(&dir, "record-1", "fx-a").await;
        let prover = FakeProver::offline_for(0);

        let reachability =
            prove_reachable(&prover, &lease, ProvePolicy::once()).await.expect("reachable");

        assert_eq!(reachability.session_id, "session-1");
        assert_eq!(reachability.machine_id, "record-1-machine");
        assert_eq!(prover.calls(), 1, "a live record is proved on the first allocation");
    }

    #[tokio::test]
    async fn an_offline_record_is_not_proof_and_the_error_names_both_ids() {
        let dir = tempfile::tempdir().unwrap();
        let (_registry, lease) = lease(&dir, "record-1", "fx-a").await;
        let prover = FakeProver::offline_for(usize::MAX);

        let error =
            prove_reachable(&prover, &lease, ProvePolicy::once()).await.expect_err("offline");

        assert_eq!(error.code(), "REMOTE_ATTACH_UNPROVEN");
        // Both ids are in the message, because they are two different things an operator must
        // match against two different listings.
        assert!(error.to_string().contains("record-1"), "{error}");
        assert!(error.to_string().contains("record-1-machine"), "{error}");
        assert_eq!(error.exit_code(), 1, "only NO_RELAY_SLOT gets its own code");
        assert_eq!(prover.calls(), 1, "one attempt when the policy says once");
        assert!(error.is_retryable_proof(), "an offline record may yet connect");
    }

    #[tokio::test]
    async fn a_record_that_connects_late_is_proved_with_a_retry() {
        let dir = tempfile::tempdir().unwrap();
        let (_registry, lease) = lease(&dir, "record-1", "fx-a").await;
        let prover = FakeProver::offline_for(2);

        let reachability = prove_reachable(
            &prover,
            &lease,
            ProvePolicy { attempts: 4, delay: Duration::from_millis(0) },
        )
        .await
        .expect("reachable on the third attempt");

        assert_eq!(prover.calls(), 3);
        assert_eq!(reachability.session_id, "session-3");
    }

    #[tokio::test]
    async fn the_retry_stops_at_the_attempt_budget() {
        let dir = tempfile::tempdir().unwrap();
        let (_registry, lease) = lease(&dir, "record-1", "fx-a").await;
        let prover = FakeProver::offline_for(usize::MAX);

        let error = prove_reachable(
            &prover,
            &lease,
            ProvePolicy { attempts: 3, delay: Duration::from_millis(0) },
        )
        .await
        .expect_err("still offline");

        assert_eq!(error.code(), "REMOTE_ATTACH_UNPROVEN");
        assert_eq!(prover.calls(), 3, "the budget is what bounds the wait");
    }

    #[tokio::test]
    async fn a_refusal_the_relay_will_not_change_is_not_retried() {
        /// A prover that refuses the way a wrong token does.
        struct Refusing;

        #[async_trait::async_trait]
        impl AttachProver for Refusing {
            async fn prove_attach(
                &self,
                record_id: &str,
                machine_id: &str,
            ) -> Result<Reachability, EnrollError> {
                Err(EnrollError::Unreachable {
                    record_id: record_id.to_string(),
                    machine_id: machine_id.to_string(),
                    detail: "401 UNAUTHORIZED".to_string(),
                    retryable: false,
                })
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let (_registry, lease) = lease(&dir, "record-1", "fx-a").await;

        let error = prove_reachable(
            &Refusing,
            &lease,
            ProvePolicy { attempts: 8, delay: Duration::from_millis(0) },
        )
        .await
        .expect_err("a refused credential is not a waiting game");

        assert!(!error.is_retryable_proof(), "a 401 must not be retried eight times");
    }

    // ---- the account session, over a stub that speaks the real routes --------------------------

    #[derive(Default)]
    struct Stub {
        code_route_bearers: Mutex<Vec<String>>,
        allocations: Mutex<Vec<Value>>,
        attach_bearers: Mutex<Vec<String>>,
    }

    /// A local stand-in for the account origin: the two routes this module calls, with the
    /// answers the real ones give.
    async fn stub_origin() -> (String, Arc<Stub>) {
        use axum::extract::State;
        use axum::http::HeaderMap;
        use axum::response::{IntoResponse, Response};
        use axum::routing::post;

        let stub = Arc::new(Stub::default());

        async fn codes(State(stub): State<Arc<Stub>>, headers: HeaderMap) -> Response {
            let bearer = headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .trim_start_matches("Bearer ")
                .to_string();
            stub.code_route_bearers.lock().push(bearer);
            axum::Json(json!({ "code": SECRET_CODE, "expiresAt": 1 })).into_response()
        }

        async fn attach(
            State(stub): State<Arc<Stub>>,
            headers: HeaderMap,
            axum::Json(body): axum::Json<Value>,
        ) -> Response {
            let bearer = headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .trim_start_matches("Bearer ")
                .to_string();
            stub.attach_bearers.lock().push(bearer);
            stub.allocations.lock().push(body.clone());
            match body["machineId"].as_str().unwrap_or_default() {
                "offline-machine" => (
                    axum::http::StatusCode::CONFLICT,
                    axum::Json(json!({ "code": "MACHINE_OFFLINE" })),
                )
                    .into_response(),
                "unknown-machine" => (
                    axum::http::StatusCode::NOT_FOUND,
                    axum::Json(json!({ "code": "MACHINE_NOT_FOUND" })),
                )
                    .into_response(),
                "refused-machine" => (
                    axum::http::StatusCode::UNAUTHORIZED,
                    axum::Json(json!({ "code": "UNAUTHORIZED" })),
                )
                    .into_response(),
                "online-only-machine" => {
                    // A 200 that says the record is online but hands out no session: exactly what a
                    // stale `online` flag looks like on the wire. It is not proof.
                    axum::Json(json!({ "online": true })).into_response()
                }
                other => axum::Json(json!({
                    "sessionId": "relay-session-1",
                    "machineId": other,
                    "opaque": true,
                }))
                .into_response(),
            }
        }

        let router = axum::Router::new()
            .route("/api/account/v1/enrollment-codes", post(codes))
            .route("/api/v1/attach/session", post(attach))
            .with_state(stub.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let address = listener.local_addr().expect("address");
        tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        (format!("http://{address}"), stub)
    }

    #[tokio::test]
    async fn the_account_session_mints_a_code_over_the_real_route() {
        let (origin, stub) = stub_origin().await;
        let account = RelayAccount::with_token(origin.as_str(), "session-token-1").expect("account");

        let code = account.issue_code().await.expect("mint a code");

        assert_eq!(code, SECRET_CODE);
        assert_eq!(stub.code_route_bearers.lock().clone(), vec!["session-token-1".to_string()]);
    }

    #[tokio::test]
    async fn a_session_held_for_the_record_is_the_proof() {
        let (origin, stub) = stub_origin().await;
        let account = RelayAccount::with_token(origin.as_str(), "session-token-1").expect("account");

        let reachability =
            account.prove_attach("record-1", "record-1-machine").await.expect("reachable");

        assert_eq!(reachability.session_id, "relay-session-1");
        assert_eq!(reachability.machine_id, "record-1-machine");
        assert_eq!(reachability.relay_origin, origin);
        // The route is asked about the machineId, and the account session authorizes it.
        assert_eq!(stub.allocations.lock().clone(), vec![json!({ "machineId": "record-1-machine" })]);
        assert_eq!(stub.attach_bearers.lock().clone(), vec!["session-token-1".to_string()]);
    }

    #[tokio::test]
    async fn an_offline_record_is_a_retryable_unproven_attach() {
        let (origin, _stub) = stub_origin().await;
        let account = RelayAccount::with_token(origin.as_str(), "session-token-1").expect("account");

        let error = account
            .prove_attach("record-1", "offline-machine")
            .await
            .expect_err("409 is not a proof");

        assert_eq!(error.code(), "REMOTE_ATTACH_UNPROVEN");
        assert!(error.is_retryable_proof());
        assert!(error.to_string().contains("MACHINE_OFFLINE"), "{error}");
    }

    #[tokio::test]
    async fn an_unknown_record_and_a_refused_session_are_not_retried() {
        let (origin, _stub) = stub_origin().await;
        let account = RelayAccount::with_token(origin.as_str(), "session-token-1").expect("account");

        let unknown = account
            .prove_attach("record-1", "unknown-machine")
            .await
            .expect_err("the account does not hold this record");
        assert!(!unknown.is_retryable_proof());
        assert!(unknown.to_string().contains("MACHINE_NOT_FOUND"), "{unknown}");

        let refused = account
            .prove_attach("record-1", "refused-machine")
            .await
            .expect_err("the session was refused");
        assert!(!refused.is_retryable_proof(), "a refused credential is not retried");
        assert!(refused.to_string().contains("UNAUTHORIZED"), "{refused}");
    }

    #[tokio::test]
    async fn a_two_hundred_without_a_session_id_is_not_proof() {
        let (origin, stub) = stub_origin().await;
        let account = RelayAccount::with_token(origin.as_str(), "session-token-1").expect("account");

        let error = account
            .prove_attach("record-1", "online-only-machine")
            .await
            .expect_err("200 with no sessionId is not a proof");

        // The route answered successfully and even called the record online; only an allocated
        // session is the relay saying it holds a live control channel.
        assert_eq!(error.code(), "REMOTE_ATTACH_UNPROVEN");
        assert!(!error.is_retryable_proof(), "an answer with no session is not a waiting game");
        assert!(error.to_string().contains("sessionId"), "{error}");
        assert_eq!(stub.allocations.lock().len(), 1, "the route was asked exactly once");
    }

    #[tokio::test]
    async fn a_refused_code_request_names_the_account_answer() {
        use axum::response::IntoResponse;

        let router = axum::Router::new().route(
            "/api/account/v1/enrollment-codes",
            axum::routing::post(|| async {
                (axum::http::StatusCode::UNAUTHORIZED, axum::Json(json!({ "code": "UNAUTHORIZED" })))
                    .into_response()
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let address = listener.local_addr().expect("address");
        tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        let account =
            RelayAccount::with_token(format!("http://{address}"), "stale-token").expect("account");

        let error = account.issue_code().await.expect_err("a stale session");

        assert_eq!(error.code(), "ENROLLMENT_CODE_UNAVAILABLE");
        assert!(error.to_string().contains("UNAUTHORIZED"), "{error}");
        // The token itself never reaches the message.
        assert!(!error.to_string().contains("stale-token"), "{error}");
    }

    // ---- pure contracts -----------------------------------------------------------------------

    #[test]
    fn a_dry_pool_is_exit_four() {
        let error = EnrollError::from(SlotError::NoFreeSlot);
        assert_eq!(error.code(), NO_RELAY_SLOT);
        assert_eq!(error.exit_code(), EXIT_NO_RELAY_SLOT);
        assert_eq!(EXIT_NO_RELAY_SLOT, 4);
        assert!(error.to_string().starts_with("NO_RELAY_SLOT:"));

        // A slot error that is not the dry pool keeps its own token and the generic code.
        let other = EnrollError::from(SlotError::UnknownSlot("nope".into()));
        assert_eq!(other.code(), "SLOT_UNKNOWN");
        assert_eq!(other.exit_code(), 1);
        assert_eq!(exit_code_for_error_code("SOMETHING_NEW"), 1);
    }

    #[test]
    fn every_step_has_a_stable_name_and_code() {
        let cases = [
            (BoxStep::Home, "home", "BOX_STEP_FAILED"),
            (BoxStep::Prepare, "prepare", "BOX_STEP_FAILED"),
            (BoxStep::Enroll, "enroll", "ENROLL_FAILED"),
            (BoxStep::RemoveCode, "remove_code", "CODE_FILE_NOT_REMOVED"),
            (BoxStep::StartDaemon, "start_daemon", "DAEMON_CONFIGURE_FAILED"),
            (BoxStep::ConfigureRelay, "configure_relay", "DAEMON_CONFIGURE_FAILED"),
        ];
        for (step, name, code) in cases {
            assert_eq!(step.as_str(), name);
            let error = EnrollError::Step { step, detail: "x".into() };
            assert_eq!(error.code(), code, "{name}");
            assert_eq!(error.exit_code(), 1);
            assert!(error.to_string().contains(name), "{error}");
        }
    }

    #[test]
    fn a_missing_session_names_the_file_and_never_prints_it() {
        let dir = tempfile::tempdir().unwrap();
        let missing = RelayAccount::at(dir.path(), RELAY_ORIGIN).expect_err("no session file");
        assert_eq!(missing.code(), "ACCOUNT_SESSION_MISSING");
        assert!(missing.to_string().contains(ACCOUNT_SESSION_TOKEN_FILE), "{missing}");

        // An empty file is "not signed in" too, not an empty bearer.
        let empty = tempfile::tempdir().unwrap();
        let path = account_session_token_path(empty.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "   \n").unwrap();
        let blank = RelayAccount::at(empty.path(), RELAY_ORIGIN).expect_err("an empty token");
        assert_eq!(blank.code(), "ACCOUNT_SESSION_MISSING");
        assert!(blank.to_string().contains("empty"), "{blank}");
    }

    #[test]
    fn a_real_session_is_read_from_disk_and_redacted_in_debug() {
        let home = tempfile::tempdir().unwrap();
        let path = account_session_token_path(home.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "  a-real-looking-token\n").unwrap();

        let account = RelayAccount::at(home.path(), "https://relay.example/").expect("account");

        assert_eq!(account.origin(), "https://relay.example", "a trailing slash is trimmed");
        assert!(!format!("{account:?}").contains("a-real-looking-token"), "Debug must redact");
        assert!(format!("{account:?}").contains("https://relay.example"));
    }

    #[test]
    fn an_empty_token_or_origin_is_refused() {
        assert_eq!(
            RelayAccount::with_token(RELAY_ORIGIN, "  ").expect_err("an empty token").code(),
            "ACCOUNT_SESSION_MISSING"
        );
        assert_eq!(
            RelayAccount::with_token("  ", "token").expect_err("an empty origin").code(),
            "ACCOUNT_SESSION_MISSING"
        );
    }

    #[test]
    fn the_session_path_is_the_desktop_secret() {
        assert_eq!(
            account_session_token_path(Path::new("/Users/someone")),
            PathBuf::from("/Users/someone/.ferryx/hub/secrets/ferryx-account-session-token")
        );
    }

    #[test]
    fn the_code_file_holds_the_code_and_one_newline() {
        assert_eq!(code_file_bytes("  abc123 \n"), b"abc123\n".to_vec());
        assert_eq!(code_file_bytes("abc123"), b"abc123\n".to_vec());
    }

    #[test]
    fn a_failed_step_never_echoes_the_code() {
        let result = ExecResult {
            exit_code: Some(4),
            stdout: format!("argv had {SECRET_CODE} in it"),
            stderr: format!("error near {SECRET_CODE}"),
            timed_out: false,
        };
        let summary = command_summary(&result, SECRET_CODE);
        assert!(!summary.contains(SECRET_CODE), "{summary}");
        assert!(summary.contains("<redacted>"), "{summary}");
        assert!(summary.starts_with("exit 4"), "{summary}");

        let timed_out = ExecResult {
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            timed_out: true,
        };
        let summary = command_summary(&timed_out, "");
        assert!(summary.contains("no exit code"), "{summary}");
        assert!(summary.contains("timed out"), "{summary}");
    }

    #[test]
    fn a_long_output_is_cut_to_its_tail() {
        let long = "x".repeat(1_000);
        assert_eq!(tail(&long, 10).len(), 10);
        assert_eq!(tail("short", 10), "short");
        assert_eq!(redact("a b a", "a"), "<redacted> b <redacted>");
        assert_eq!(redact("nothing", ""), "nothing");
    }

    #[test]
    fn the_resolve_snippet_accepts_every_install_shape() {
        // The bootstrap's install root, an explicit override and PATH must all be searchable; a
        // missing client exits 127 instead of "command not found".
        assert!(RESOLVE_CLI.contains("FERRYX_CLI"));
        assert!(RESOLVE_CLI.contains("command -v ferryx-cli"));
        assert!(RESOLVE_CLI.contains("exit 127"));
        // Every step that needs the client interpolates this, so a missing client is one named
        // failure rather than "command not found" from whichever step ran first.
        assert!(RESOLVE_CLI.ends_with(';'));
    }

    #[test]
    fn the_configure_script_speaks_the_daemon_protocol_and_writes_nothing() {
        assert!(CONFIGURE_SCRIPT.contains("\"remoteConfigure\""));
        assert!(CONFIGURE_SCRIPT.contains("remoteConfigureOk"));
        assert!(CONFIGURE_SCRIPT.contains("\"handshake\""));
        assert!(CONFIGURE_SCRIPT.contains("handshakeOk"));
        // It never opens a file: the config arrives on argv and the helper writes nothing.
        assert!(!CONFIGURE_SCRIPT.contains("open("));
        assert!(!CONFIGURE_SCRIPT.contains("relay.ferryx.dev"), "the origin arrives on argv");
    }

    #[test]
    fn the_relay_config_carries_the_plan_wire_shape() {
        let serialized = relay_config_json(RELAY_ORIGIN).expect("serialize");
        let value: Value = serde_json::from_str(&serialized).expect("parse back");
        assert_eq!(value["mode"], "relay");
        assert_eq!(value["allowControl"], true);
        assert_eq!(value["restartPolicy"], "restoreListener");
        assert_eq!(value["relayUrl"], RELAY_ORIGIN);
        // The daemon overwrites the port, but the struct requires one on the wire.
        assert!(value.get("port").is_some());
        // A trailing slash never reaches the box.
        let trimmed = relay_config_json("https://relay.example/").expect("serialize");
        assert!(trimmed.contains("\"relayUrl\":\"https://relay.example\""), "{trimmed}");
    }

    #[test]
    fn a_prove_policy_once_does_not_wait() {
        assert_eq!(ProvePolicy::once().attempts, 1);
        assert_eq!(ProvePolicy::once().delay, Duration::from_millis(0));
        assert!(ProvePolicy::default().attempts > 1, "a fresh box needs a few seconds");
    }
}
