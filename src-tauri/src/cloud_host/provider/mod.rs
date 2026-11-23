//! The sandbox-provider seam: everything the cloud control plane needs from whoever hosts a
//! machine, in provider-neutral terms.
//!
//! Provider specifics - URLs, field names, state vocabularies, headers, error codes, account
//! rules - live only in `provider/<name>.rs`. Every implementation must guarantee:
//!
//! 1. Idempotent create: the same `idempotency_key` with the same request returns the same
//!    machine instead of a second billable one.
//! 2. The disk persists across stop/resume, so a resumed box needs no re-bootstrap.
//! 3. `env` from create is visible to `exec`.
//! 4. `get` and `list` never wake a stopped machine; neither may hold a hidden `resume`.
//! 5. Delete is irreversible: the handle reports `Deleted` (or `NotFound`) afterwards.
//!
//! Map every failure onto an [`ErrorKind`]; a `Config` error names what the operator must fix
//! (a missing key variable, or the permission the key lacks).

pub mod blaxel;
pub mod codespaces;
pub mod daytona;
pub mod e2b;
pub mod fake;
pub mod freestyle;
pub mod instabox;
pub mod opencomputer;
pub mod prized;
pub mod sprites;
pub mod tensorlake;
pub mod vercel;

mod cli_run;

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// How the control plane reaches a machine once it is ready.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    /// The provider hands out an SSH endpoint ([`SandboxProvider::ssh_target`] returns one) and
    /// ferryx forwards the box's rpc socket over it.
    Ssh,
    /// No inbound SSH: agent traffic goes through the ferryx relay's outbound E2E session.
    Relay,
}

/// What a provider can do. Asked once, when the provider registry is built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    /// Auto-stop cannot be disabled: create and resume must pass a TTL.
    pub ttl_required: bool,
    /// Largest TTL the provider accepts; `None` = unbounded.
    pub max_ttl_seconds: Option<u64>,
    /// Which transport carries agent traffic to a ready machine.
    pub transport: Transport,
    /// The provider stops an idle machine on its own (E2B `timeout`, Daytona auto-stop, ...).
    /// When false, `cloud create` refuses the provider unless the local reaper is loaded.
    pub native_auto_stop: bool,
}

/// Normalized lifecycle state. `Deleted` = the provider no longer knows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxState {
    Provisioning,
    Ready,
    Stopping,
    Stopped,
    Error,
    Deleted,
}

impl fmt::Display for SandboxState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let token = match self {
            SandboxState::Provisioning => "provisioning",
            SandboxState::Ready => "ready",
            SandboxState::Stopping => "stopping",
            SandboxState::Stopped => "stopped",
            SandboxState::Error => "error",
            SandboxState::Deleted => "deleted",
        };
        f.write_str(token)
    }
}

/// A create request. `idempotency_key` is the only thing that prevents a second billable
/// machine when a create is retried after a dropped response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateSandbox {
    pub idempotency_key: String,
    pub name: String,
    /// Auto-stop after this long; `None` = the provider's own default (subject to
    /// [`Capabilities::ttl_required`]).
    pub ttl_seconds: Option<u64>,
    /// Visible to `exec` on the machine; never contains a provider account secret.
    pub env: BTreeMap<String, String>,
}

/// Current state of one machine. `error` carries the provider's detail for `Error`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxInfo {
    pub state: SandboxState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Result of one one-shot, non-interactive command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecResult {
    /// `None` when the provider reports only that the command ended (a signal, a timeout).
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

/// One row of a provider-side listing (the orphan scan). `created_at` is unix milliseconds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListedSandbox {
    pub sandbox_id: String,
    pub state: SandboxState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// How to reach a machine over SSH ([`Transport::Ssh`] providers only).
///
/// `proxy_command` and `certificate_file` exist because some providers hand out an endpoint that
/// is only reachable through their own CLI (`gh codespace ssh`'s `ProxyCommand`) or only
/// authenticates with a short-lived certificate next to the key (`instabox`). An adapter that
/// drops either one returns a target nothing can connect with, so both are carried verbatim and
/// the conf writer must emit them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshTarget {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub identity: SshIdentity,
    /// Pinned host key, as a `known_hosts` line. `None` means the caller records the key on
    /// first use into a per-machine known_hosts file, never the user's global one.
    pub known_hosts_line: Option<String>,
    /// The provider CLI's own `ProxyCommand` value, already split from its keyword. For GitHub
    /// Codespaces this is not an optimisation but the only route: `cs.<name>.main` has no DNS
    /// record, and `gh cs ssh -c <name> --stdio` is what dials the box. Emitted verbatim.
    pub proxy_command: Option<String>,
    /// A certificate that authenticates together with [`SshTarget::identity`]
    /// (OpenSSH `CertificateFile`). InstaBox writes a short-lived one per machine; without it the
    /// bare key is refused with `Permission denied (publickey)`.
    pub certificate_file: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SshIdentity {
    /// A private key file the provider already wrote for this machine.
    KeyFile(PathBuf),
    /// No key of our own: the ssh agent and config this user already has must prove identity
    /// (CLI-backed providers that read their own ssh-config).
    Agent,
}

/// A failure a retry cannot fix, in terms the control plane can act on. Only the provider
/// module knows how its own errors spell these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// Transient: 429, 5xx, a dropped connection. Retry under backoff.
    Retryable,
    /// The account, key or request shape is wrong. Names what the operator must fix.
    Config,
    /// The provider does not know the machine (it no longer exists).
    NotFound,
    /// The request fights the machine's current state (idempotency-key reuse, resume of a
    /// machine that is not stopped).
    Conflict,
    /// Nothing to do but report it.
    Fatal,
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let token = match self {
            ErrorKind::Retryable => "retryable",
            ErrorKind::Config => "config",
            ErrorKind::NotFound => "not_found",
            ErrorKind::Conflict => "conflict",
            ErrorKind::Fatal => "fatal",
        };
        f.write_str(token)
    }
}

/// A provider failure. `provider_code` keeps the provider's own spelling so an operator can
/// match it against the API docs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxError {
    pub kind: ErrorKind,
    /// The provider that produced this, or `"trait-default"` when a trait default refused the
    /// call before any provider ran (`write_file` on an SSH provider, `ssh_target` on a relay
    /// provider).
    pub provider: &'static str,
    pub provider_code: String,
    pub message: String,
}

impl SandboxError {
    pub fn new(
        kind: ErrorKind,
        provider: &'static str,
        provider_code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self { kind, provider, provider_code: provider_code.into(), message: message.into() }
    }

    pub fn is_not_found(&self) -> bool {
        self.kind == ErrorKind::NotFound
    }
}

impl fmt::Display for SandboxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}: {}", self.kind, self.provider, self.message)?;
        if !self.provider_code.is_empty() {
            write!(f, " ({})", self.provider_code)?;
        }
        Ok(())
    }
}

impl std::error::Error for SandboxError {}

/// One provider's whole lifecycle surface. `send` futures only: a provider call never borrows
/// the caller's data past the `.await`.
#[async_trait::async_trait]
pub trait SandboxProvider: Send + Sync {
    fn capabilities(&self) -> Capabilities;

    /// True when a machine this provider just created may answer [`SandboxState::Stopped`] and
    /// still be workable, because the first `write_file`/`exec` starts its runtime.
    ///
    /// Fly Sprites is the reason this exists: a new Sprite reports `cold` or `warm` (both mapped
    /// to `Stopped`) until a request wakes it, and every request - including the `exec` the create
    /// path makes as its next step - is that wake. A create path that insists on `Ready` therefore
    /// spends its whole budget on a state the platform never leaves on its own.
    ///
    /// `false` by default, and it must stay that way for every other provider: there `Stopped`
    /// means the machine is asleep and only [`SandboxProvider::resume`] brings it back, so calling
    /// it ready would run a bootstrap against a box that cannot answer.
    fn accepts_stopped_as_ready_on_create(&self) -> bool {
        false
    }

    /// Create (or return) the machine this idempotency key names; the returned id is the
    /// handle every other call takes.
    async fn create(&self, request: CreateSandbox) -> Result<String, SandboxError>;

    /// Current state. Never wakes a stopped machine.
    async fn get(&self, sandbox_id: &str) -> Result<SandboxInfo, SandboxError>;

    /// Snapshot and stop; the disk survives for `resume`. Idempotent.
    async fn stop(&self, sandbox_id: &str) -> Result<(), SandboxError>;

    /// Wake a stopped machine, optionally resetting its auto-stop.
    async fn resume(&self, sandbox_id: &str, ttl_seconds: Option<u64>) -> Result<(), SandboxError>;

    /// Irreversible. The handle reports `Deleted` afterwards.
    async fn delete(&self, sandbox_id: &str) -> Result<(), SandboxError>;

    /// Move the auto-stop deadline without touching the machine.
    async fn extend_ttl(
        &self,
        sandbox_id: &str,
        ttl_seconds: Option<u64>,
    ) -> Result<(), SandboxError>;

    /// One-shot, non-interactive command: no stdin, no PTY, no reconnecting stream.
    async fn exec(
        &self,
        sandbox_id: &str,
        command: &str,
        timeout_seconds: u64,
    ) -> Result<ExecResult, SandboxError>;

    /// Write `bytes` at absolute `path`, applying `mode` in the same call (0600 for secrets).
    ///
    /// T1 ([`Transport::Ssh`]) providers leave this default, which reports `Config`: the caller
    /// pushes files with `scp` over the per-machine conf instead. T2 providers implement it with
    /// their file API, because nothing else may carry a credential to the box.
    async fn write_file(
        &self,
        sandbox_id: &str,
        path: &str,
        bytes: Vec<u8>,
        mode: u32,
    ) -> Result<(), SandboxError> {
        let _ = (sandbox_id, path, bytes, mode);
        Err(SandboxError::new(
            ErrorKind::Config,
            "trait-default",
            "write_file_unsupported",
            "this provider has no file API; push the file over SSH instead",
        ))
    }

    /// SSH endpoint of a ready machine. Only [`Transport::Ssh`] providers implement this; the
    /// default reports `Config` so a relay-only provider can never be reached by an ssh path.
    async fn ssh_target(&self, sandbox_id: &str) -> Result<SshTarget, SandboxError> {
        let _ = sandbox_id;
        Err(SandboxError::new(
            ErrorKind::Config,
            "trait-default",
            "ssh_unsupported",
            "this provider has no SSH endpoint; reach the machine over the relay",
        ))
    }

    /// Every machine on the provider account (the orphan scan). Never wakes a stopped machine.
    async fn list(&self) -> Result<Vec<ListedSandbox>, SandboxError>;
}

#[cfg(test)]
mod tests {
    use super::fake::FakeProvider;
    use super::*;

    #[tokio::test]
    async fn write_file_default_refuses_with_config() {
        let provider = FakeProvider::new();
        let id = provider
            .create(CreateSandbox {
                idempotency_key: "fx-write-file".into(),
                name: "fx-write-file".into(),
                ttl_seconds: None,
                env: BTreeMap::new(),
            })
            .await
            .expect("create");

        let error = provider
            .write_file(&id, "/home/fx/.fx-test", vec![0u8; 32], 0o600)
            .await
            .expect_err("a provider without a file API must refuse");
        assert_eq!(error.kind, ErrorKind::Config);
        assert_eq!(error.provider_code, "write_file_unsupported");
    }

    #[tokio::test]
    async fn ssh_target_default_refuses_with_config() {
        let provider = FakeProvider::new();
        let error = provider.ssh_target("fake-1").await.expect_err("no ssh endpoint");
        assert_eq!(error.kind, ErrorKind::Config);
        assert_eq!(error.provider_code, "ssh_unsupported");
    }

    #[test]
    fn error_display_names_kind_provider_and_code() {
        let error = SandboxError::new(ErrorKind::NotFound, "e2b", "sandbox_not_found", "gone");
        assert_eq!(error.to_string(), "[not_found] e2b: gone (sandbox_not_found)");
        assert!(error.is_not_found());
        assert!(!SandboxError::new(ErrorKind::Fatal, "e2b", "", "boom").is_not_found());
    }

    #[test]
    fn state_and_kind_serialize_in_snake_case() {
        assert_eq!(serde_json::to_string(&SandboxState::Provisioning).unwrap(), "\"provisioning\"");
        assert_eq!(serde_json::to_string(&SandboxState::Ready).unwrap(), "\"ready\"");
        assert_eq!(serde_json::to_string(&ErrorKind::NotFound).unwrap(), "\"not_found\"");
        assert_eq!(serde_json::to_string(&Transport::Ssh).unwrap(), "\"ssh\"");
    }
}
