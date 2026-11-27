//! InstaBox, driven through the official `instabox` CLI.
//!
//! Commands, captured from `instabox` 82b8d2af on 2026-11-21 (`tests/fixtures/cloud_host/instabox/`):
//!
//! | trait call | command |
//! | --- | --- |
//! | `create` | `instabox create --name <n> --auto-suspend <secs>s [--meta k=v ...] --no-wait` |
//! | `get` | `instabox get <n> --json` |
//! | `list` | `instabox ls --json` |
//! | `stop` | `instabox suspend <n> --no-wait` (or `stop`, which shuts it down) |
//! | `resume` | `instabox resume <n> --no-wait` |
//! | `delete` | `instabox rm <n>` |
//! | `extend_ttl` | `instabox auto-suspend <n> <secs>s` |
//! | `exec` | `instabox exec <n> -- <command>` (one argv element) |
//! | `ssh_target` | `instabox ssh-config <n>` (OpenSSH config block) |
//!
//! `stop` maps onto *suspend*, not `stop`: a suspended sandbox keeps its processes and home
//! directory for `resume` (which is what the trait's stop/resume pair promises), while
//! `instabox stop` shuts the machine down.
//!
//! **The idle deadline is `auto-suspend`, not `--timeout`.** `instabox create --timeout` still
//! parses client-side but the server now rejects it (`unknown_field: "timeoutSeconds": sandboxes
//! have no lifetime timeout since 2026-10-20`), so a create that passed it would fail on a field
//! that would do nothing. The trait's TTL means "auto-stop after this long idle", which is
//! exactly `--auto-suspend`; `extend_ttl` moves the same setting with `instabox auto-suspend`.
//! The CLI floors it at 2m and caps it at 24h.
//!
//! **Capacity.** The free plan allows one sandbox (`instabox usage --json` ->
//! `limits.maxMachines: 1`) and this account already uses it. A second create answers
//! `quota_exceeded: sandbox limit reached (1 of 1)`; [`super::cli_run::classify`] turns that
//! into `ErrorKind::Conflict`, which is what the plan's QA asserts.
//!
//! **Credential.** The CLI's stored browser session expires (`instabox: session expired
//! (unauthenticated)`); it accepts `INSTABOX_API_KEY` (`ibk_...`) instead. This adapter passes
//! the environment through, so a caller exporting that variable is authenticated without any
//! secret entering an argv array, a log or this file.

use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;

use super::cli_run::{self, CliOutput};
use super::{
    Capabilities, CreateSandbox, ErrorKind, ExecResult, ListedSandbox, SandboxError, SandboxInfo,
    SandboxProvider, SandboxState, SshIdentity, SshTarget, Transport,
};

const PROVIDER: &str = "instabox";
const EXE: &str = "instabox";
const QUERY_TIMEOUT: Duration = Duration::from_secs(120);
/// Create waits for the sandbox unless `--no-wait`; we pass `--no-wait` and poll `get`.
const CREATE_TIMEOUT: Duration = Duration::from_secs(300);
/// Default idle pause when the caller passes no TTL (the CLI's own default is 30m).
const DEFAULT_AUTO_SUSPEND_SECONDS: u64 = 1800;
/// `--auto-suspend` accepts 2m..24h; 0 means "never".
const MIN_AUTO_SUSPEND_SECONDS: u64 = 120;
const MAX_AUTO_SUSPEND_SECONDS: u64 = 24 * 60 * 60;

/// One sandbox from `instabox ls --json` / `instabox get <n> --json`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstaboxSandbox {
    pub id: String,
    pub name: String,
    pub state: String,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub guest_user: Option<String>,
}

/// `instabox usage --json`, only the fields that matter for a capacity conflict.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstaboxUsage {
    pub machines: u64,
    pub limits: InstaboxLimits,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstaboxLimits {
    pub max_machines: u64,
}

pub struct InstaboxProvider {
    /// `--meta` labels stamped on every machine this adapter creates, so the orphan scan can
    /// recognise its own machines. `role=build` is the existing `bong-i-1`'s label and is never
    /// ours.
    meta: Vec<(String, String)>,
    /// The `instabox` binary every call spawns. Always [`EXE`] outside tests; a test points it at
    /// a stub so the argv and the CLI calls a call site really makes can be asserted without an
    /// account.
    cli: PathBuf,
}

impl InstaboxProvider {
    /// Tag created machines with `fx=<idempotency_key prefix>`, matching the `fx-` name prefix
    /// the lifecycle sweep looks for.
    pub fn new() -> Self {
        Self {
            meta: vec![("managed-by".to_string(), "ferryx-cloud".to_string())],
            cli: PathBuf::from(EXE),
        }
    }

    pub fn with_meta(meta: Vec<(String, String)>) -> Self {
        Self { meta, cli: PathBuf::from(EXE) }
    }

    /// A provider that spawns `cli` instead of the `instabox` on PATH.
    #[cfg(test)]
    fn with_cli(cli: impl Into<PathBuf>) -> Self {
        Self { cli: cli.into(), ..Self::new() }
    }

    async fn run(&self, args: &[&str], timeout: Duration) -> Result<CliOutput, SandboxError> {
        let exe = self.cli.to_string_lossy();
        cli_run::run(PROVIDER, &exe, args, timeout).await
    }

    async fn query(&self, args: &[&str]) -> Result<String, SandboxError> {
        let output = self.run(args, QUERY_TIMEOUT).await?;
        if cli_run::succeeded(&output) {
            Ok(output.stdout)
        } else {
            Err(cli_run::failure(PROVIDER, EXE, &output))
        }
    }

    async fn mutate(&self, args: &[&str]) -> Result<(), SandboxError> {
        let output = self.run(args, QUERY_TIMEOUT).await?;
        if cli_run::succeeded(&output) {
            Ok(())
        } else {
            Err(cli_run::failure(PROVIDER, EXE, &output))
        }
    }
}

impl Default for InstaboxProvider {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse `instabox ls --json` (a JSON array).
pub fn parse_list(stdout: &str) -> Result<Vec<ListedSandbox>, SandboxError> {
    let rows: Vec<InstaboxSandbox> = serde_json::from_str(stdout).map_err(|error| {
        SandboxError::new(
            ErrorKind::Fatal,
            PROVIDER,
            "list_parse_failed",
            format!("instabox ls --json did not return a JSON array: {error}"),
        )
    })?;
    Ok(rows.into_iter().map(to_listed).collect())
}

/// Parse `instabox get <n> --json` (one JSON object).
pub fn parse_get(stdout: &str) -> Result<SandboxInfo, SandboxError> {
    let row: InstaboxSandbox = serde_json::from_str(stdout).map_err(|error| {
        SandboxError::new(
            ErrorKind::Fatal,
            PROVIDER,
            "get_parse_failed",
            format!("instabox get --json did not return a JSON object: {error}"),
        )
    })?;
    Ok(SandboxInfo { state: map_state(&row.state), error: None })
}

/// The sandbox's guest user, needed by `ssh_target` (`instabox ssh-config` also states it).
pub fn parse_guest_user(stdout: &str) -> Result<String, SandboxError> {
    let row: InstaboxSandbox = serde_json::from_str(stdout).map_err(|error| {
        SandboxError::new(
            ErrorKind::Fatal,
            PROVIDER,
            "get_parse_failed",
            format!("instabox get --json did not return a JSON object: {error}"),
        )
    })?;
    Ok(row.guest_user.unwrap_or_else(|| "instabox".to_string()))
}

fn to_listed(row: InstaboxSandbox) -> ListedSandbox {
    ListedSandbox {
        sandbox_id: row.id,
        state: map_state(&row.state),
        created_at: row.created_at.as_deref().and_then(epoch_ms),
        name: Some(row.name),
    }
}

/// InstaBox's `state`/`desiredState` vocabulary onto the trait's states. `suspended` is the
/// paused state a `resume` continues from, so it maps to `Stopped`; `stopped` is a shutdown.
pub fn map_state(state: &str) -> SandboxState {
    match state {
        "running" => SandboxState::Ready,
        "suspended" | "stopped" => SandboxState::Stopped,
        "suspending" | "stopping" | "pausing" => SandboxState::Stopping,
        "deleted" | "removed" | "gone" => SandboxState::Deleted,
        "error" | "failed" => SandboxState::Error,
        _ => SandboxState::Provisioning,
    }
}

/// Parse `instabox ssh-config <n>` into an SSH endpoint.
///
/// Live output (comments stripped by `ssh` itself):
///
/// ```text
/// Host instabox-bong-i-1
///   HostName ssh.instabox.sh
///   Port 2222
///   User m_agqyb2s35jvh2e3brqywyknz6e
///   IdentityFile /Users/indo/.config/instabox/ssh/id_ed25519
///   CertificateFile .../<id>-cert.pub
///   IdentitiesOnly yes
///   UserKnownHostsFile /Users/indo/.config/instabox/ssh/known_hosts
///   StrictHostKeyChecking yes
/// ```
///
/// `Host` is the CLI's local alias; the endpoint is `HostName`. A short-lived certificate
/// accompanies the key (`instabox ssh-cert <n>` refreshes it), and it is **required**: the key
/// alone is refused with `Permission denied (publickey)` (verified live against a running box).
/// The certificate lives at `<config>/<sandbox-id>-cert.pub`, which ssh cannot auto-discover from
/// the key's own name, so it must be carried as `CertificateFile`.
pub fn parse_ssh_config(text: &str) -> Result<SshTarget, SandboxError> {
    let mut alias: Option<String> = None;
    let mut hostname: Option<String> = None;
    let mut user: Option<String> = None;
    let mut identity: Option<std::path::PathBuf> = None;
    let mut certificate: Option<std::path::PathBuf> = None;
    let mut proxy_command: Option<String> = None;
    let mut port: u16 = 22;

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // The CLI writes `Key value`; `Key=value` is accepted too, since ssh accepts both.
        let (key, value) = match line.split_once(char::is_whitespace) {
            Some(parts) => parts,
            None => match line.split_once('=') {
                Some((key, value)) => (key, value),
                None => continue,
            },
        };
        let value = value.trim().trim_start_matches('=').trim();
        match key.to_ascii_lowercase().as_str() {
            "host" => alias = Some(value.to_string()),
            "hostname" => hostname = Some(value.to_string()),
            "user" => user = Some(value.to_string()),
            "port" => {
                if let Ok(parsed) = value.parse() {
                    port = parsed;
                }
            }
            "identityfile" => identity = Some(std::path::PathBuf::from(value)),
            "certificatefile" => certificate = Some(std::path::PathBuf::from(value)),
            "proxycommand" => proxy_command = Some(value.to_string()),
            _ => {}
        }
    }

    let missing = |what: &str| {
        SandboxError::new(
            ErrorKind::Config,
            PROVIDER,
            "ssh_config_incomplete",
            format!("`instabox ssh-config` output has no {what}"),
        )
    };
    let host = hostname.or(alias).ok_or_else(|| missing("HostName or Host"))?;
    let user = user.ok_or_else(|| missing("User line"))?;
    let identity = identity.ok_or_else(|| missing("IdentityFile line"))?;

    Ok(SshTarget {
        host,
        port,
        user,
        identity: SshIdentity::KeyFile(identity),
        known_hosts_line: None,
        proxy_command,
        certificate_file: certificate,
    })
}

/// RFC3339 (InstaBox uses microseconds and sometimes no offset) -> unix milliseconds.
fn epoch_ms(timestamp: &str) -> Option<i64> {
    time::OffsetDateTime::parse(timestamp, &time::format_description::well_known::Rfc3339)
        .ok()
        .and_then(|parsed| i64::try_from(parsed.unix_timestamp_nanos() / 1_000_000).ok())
}

/// `instabox` takes durations as `<n>s`/`<n>m`; seconds are unambiguous. Used for the flags that
/// really are Go durations (`--auto-suspend`). `exec --timeout` is *not* one of them: it is an
/// `int` of seconds, and `30s` there is rejected before anything runs (D2).
fn seconds_arg(seconds: u64) -> String {
    format!("{seconds}s")
}

/// `instabox exec --timeout` is an integer ("seconds before the command is killed (default 60,
/// max 600)"). A duration suffix exits 2 at the flag parser, so this stays a bare integer.
fn exec_timeout_arg(timeout_seconds: u64) -> String {
    timeout_seconds.clamp(1, 600).to_string()
}

/// The exact argv `exec` hands the CLI, kept separate so a test can assert its shape without
/// spawning anything.
fn exec_args<'a>(sandbox_id: &'a str, command: &'a str, timeout: &'a str) -> [&'a str; 6] {
    ["exec", sandbox_id, "--timeout", timeout, "--", command]
}

/// The one place `exec`'s argv is built. [`SandboxProvider::exec`] has no other path to the CLI,
/// so a regression like D2 (a duration where the flag takes an integer) cannot survive by having
/// the caller bypass the helper.
fn exec_argv(sandbox_id: &str, command: &str, timeout_seconds: u64) -> Vec<String> {
    exec_args(sandbox_id, command, &exec_timeout_arg(timeout_seconds))
        .into_iter()
        .map(str::to_string)
        .collect()
}

/// Clamp a TTL into the range `--auto-suspend` accepts (2m..24h).
fn auto_suspend_arg(ttl_seconds: u64) -> String {
    seconds_arg(ttl_seconds.clamp(MIN_AUTO_SUSPEND_SECONDS, MAX_AUTO_SUSPEND_SECONDS))
}

#[async_trait::async_trait]
impl SandboxProvider for InstaboxProvider {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            ttl_required: false,
            // `--auto-suspend` runs from 2 minutes to 24 hours; 0 disables it.
            max_ttl_seconds: Some(MAX_AUTO_SUSPEND_SECONDS),
            transport: Transport::Ssh,
            native_auto_stop: true,
        }
    }

    async fn create(&self, request: CreateSandbox) -> Result<String, SandboxError> {
        // `env` cannot be set on an InstaBox sandbox: environment comes from its template and
        // its vault. Refuse before spawning anything instead of dropping the caller's values.
        if !request.env.is_empty() {
            return Err(SandboxError::new(
                ErrorKind::Config,
                PROVIDER,
                "env_unsupported",
                "an InstaBox sandbox gets its environment from its template and vault; pass no env",
            ));
        }

        // InstaBox has no idempotency key; a sandbox already carrying this name is this request.
        // `instabox get` exits 1 with `no sandbox with ID or name` when it does not exist.
        match self.get(&request.name).await {
            Ok(_) => {
                let listed = parse_list(&self.query(&["ls", "--json"]).await?)?;
                if let Some(existing) =
                    listed.into_iter().find(|row| row.name.as_deref() == Some(request.name.as_str()))
                {
                    return Ok(existing.sandbox_id);
                }
            }
            Err(error) if error.kind == ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }

        let ttl = request.ttl_seconds.unwrap_or(DEFAULT_AUTO_SUSPEND_SECONDS);
        let auto_suspend = auto_suspend_arg(ttl);

        let mut args: Vec<String> =
            vec!["create".into(), "--name".into(), request.name.clone(), "--no-wait".into()];
        args.extend(["--auto-suspend".into(), auto_suspend]);
        for (key, value) in &self.meta {
            args.extend(["--meta".into(), format!("{key}={value}")]);
        }

        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let output = self.run(&refs, CREATE_TIMEOUT).await?;
        if !cli_run::succeeded(&output) {
            return Err(cli_run::failure(PROVIDER, EXE, &output));
        }

        // `create` prints `<name>: <state>` then the new sandbox id on the last line. Take the
        // id when it is there and fall back to the listing, since only the listing is a
        // documented shape (`ls --json`).
        if let Some(id) = output.stdout.lines().map(str::trim).rev().find(|line| line.starts_with("m_")) {
            return Ok(id.to_string());
        }
        let listed = parse_list(&self.query(&["ls", "--json"]).await?)?;
        listed
            .into_iter()
            .find(|row| row.name.as_deref() == Some(request.name.as_str()))
            .map(|row| row.sandbox_id)
            .ok_or_else(|| {
                SandboxError::new(
                    ErrorKind::Fatal,
                    PROVIDER,
                    "create_not_visible",
                    format!("instabox created a sandbox but none is named {}", request.name),
                )
            })
    }

    async fn get(&self, sandbox_id: &str) -> Result<SandboxInfo, SandboxError> {
        let stdout = self.query(&["get", sandbox_id, "--json"]).await?;
        parse_get(&stdout)
    }

    async fn stop(&self, sandbox_id: &str) -> Result<(), SandboxError> {
        // Suspend, not stop: suspend keeps memory and processes so `resume` continues where the
        // machine left off, which is what the trait's stop/resume pair must promise.
        self.mutate(&["suspend", sandbox_id, "--no-wait"]).await
    }

    async fn resume(&self, sandbox_id: &str, ttl_seconds: Option<u64>) -> Result<(), SandboxError> {
        self.mutate(&["resume", sandbox_id, "--no-wait"]).await?;
        if let Some(ttl) = ttl_seconds {
            self.extend_ttl(sandbox_id, Some(ttl)).await?;
        }
        Ok(())
    }

    async fn delete(&self, sandbox_id: &str) -> Result<(), SandboxError> {
        self.mutate(&["rm", sandbox_id]).await
    }

    async fn extend_ttl(&self, sandbox_id: &str, ttl_seconds: Option<u64>) -> Result<(), SandboxError> {
        // `0` clears the deadline (the sandbox then runs until it is stopped by hand).
        let value = ttl_seconds.map(auto_suspend_arg).unwrap_or_else(|| "0".to_string());
        self.mutate(&["auto-suspend", sandbox_id, value.as_str()]).await
    }

    async fn exec(
        &self,
        sandbox_id: &str,
        command: &str,
        timeout_seconds: u64,
    ) -> Result<ExecResult, SandboxError> {
        // The CLI joins everything after `--` into one command string and runs it as `bash -lc`
        // on the sandbox, so the command is passed as a single argument (verified against the
        // CLI: `exec <id> -- echo hi there` sends `{"cmd":"echo hi there"}`).
        //
        // `--timeout` takes an `int` of seconds, not a Go duration: `30s` makes the flag parser
        // exit 2 before the sandbox is ever contacted, which is what D2 was. `exec_argv` is the
        // only builder, so the integer form cannot be skipped.
        let argv = exec_argv(sandbox_id, command, timeout_seconds);
        let args: Vec<&str> = argv.iter().map(String::as_str).collect();
        let output = self
            .run(&args, Duration::from_secs(timeout_seconds.clamp(1, 600) + 30))
            .await?;
        if !cli_run::succeeded(&output) && output.code.is_none() {
            return Err(cli_run::failure(PROVIDER, EXE, &output));
        }
        // The CLI exits with the command's own status and streams its stdout/stderr through
        // untouched, so both streams are the command's, not the wrapper's.
        Ok(ExecResult {
            exit_code: output.code,
            stdout: output.stdout,
            stderr: output.stderr,
            timed_out: false,
        })
    }

    async fn list(&self) -> Result<Vec<ListedSandbox>, SandboxError> {
        parse_list(&self.query(&["ls", "--json"]).await?)
    }

    async fn ssh_target(&self, sandbox_id: &str) -> Result<SshTarget, SandboxError> {
        // Guard the billing rule before the CLI, exactly as the other adapters do:
        // `instabox ssh-config` refreshes a certificate that is tied to a live sandbox, and a
        // read-only probe was observed taking a suspended box to running (rev 280 -> 281 in the
        // verifier's session). `get` alone does not bump the revision (verified: rev unchanged),
        // so this check is the one place that keeps a status call from waking a billing machine.
        let info = self.get(sandbox_id).await?;
        if info.state != SandboxState::Ready {
            return Err(SandboxError::new(
                ErrorKind::Conflict,
                PROVIDER,
                "ssh_needs_running",
                format!(
                    "sandbox {sandbox_id} is {}; resume it before asking for an ssh target \
                     (instabox ssh-config refreshes a certificate tied to a live sandbox)",
                    info.state
                ),
            ));
        }
        let stdout = self.query(&["ssh-config", sandbox_id]).await?;
        parse_ssh_config(&stdout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LS_LIVE: &str = include_str!("../../../tests/fixtures/cloud_host/instabox/ls_live.json");
    const GET_LIVE: &str = include_str!("../../../tests/fixtures/cloud_host/instabox/get_live.json");
    const SSH_CONFIG_LIVE: &str =
        include_str!("../../../tests/fixtures/cloud_host/instabox/ssh_config_live.txt");
    const QUOTA_ERROR_LIVE: &str =
        include_str!("../../../tests/fixtures/cloud_host/instabox/create_quota_error_live.txt");
    const UNAUTHENTICATED_LIVE: &str =
        include_str!("../../../tests/fixtures/cloud_host/instabox/unauthenticated_live.txt");
    const USAGE_LIVE: &str = include_str!("../../../tests/fixtures/cloud_host/instabox/usage_live.json");

    #[cfg(unix)]
    fn make_executable(script: &std::path::Path) {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(script, std::fs::Permissions::from_mode(0o755))
            .expect("chmod the stub");
    }

    /// The stub is a POSIX shell script, so only a unix host can execute it. The tests that spawn it
    /// are compile-checked everywhere; on a non-unix host the file simply keeps its default mode.
    #[cfg(not(unix))]
    fn make_executable(_script: &std::path::Path) {}

    /// A stub `instabox` that records every invocation's argv and answers from canned files.
    ///
    /// This is the seam that lets a test exercise a real call site - `exec`, `ssh_target`,
    /// `create` - rather than only the helper underneath it: the provider spawns this script
    /// instead of the `instabox` on PATH, so the argv the call site actually built and the CLI
    /// calls it actually made are both observable.
    struct StubCli {
        dir: tempfile::TempDir,
        /// What `instabox get <n> --json` prints.
        get_json: String,
        /// What `instabox exec ...` writes to stdout, and its exit code.
        exec_stdout: String,
        exec_code: i32,
    }

    impl StubCli {
        fn new() -> Self {
            Self {
                dir: tempfile::tempdir().expect("tempdir"),
                get_json: GET_LIVE.to_string(),
                exec_stdout: String::new(),
                exec_code: 0,
            }
        }

        /// Answer `get` with the given JSON (used to report a non-running state).
        fn reporting(mut self, get_json: &str) -> Self {
            self.get_json = get_json.to_string();
            self
        }

        fn exec_answering(mut self, stdout: &str, code: i32) -> Self {
            self.exec_stdout = stdout.to_string();
            self.exec_code = code;
            self
        }

        fn provider(&self) -> InstaboxProvider {
            let dir = self.dir.path();
            let script = dir.join("instabox");
            let calls = dir.join("calls");
            let get = dir.join("get.json");
            let ls = dir.join("ls.json");
            let ssh = dir.join("ssh_config.txt");
            let exec_out = dir.join("exec_stdout");

            std::fs::write(&get, &self.get_json).expect("stub get body");
            std::fs::write(&ls, LS_LIVE).expect("stub ls body");
            std::fs::write(&ssh, SSH_CONFIG_LIVE).expect("stub ssh config");
            std::fs::write(&exec_out, &self.exec_stdout).expect("stub exec stdout");
            // One tab-separated line per invocation, so a test can see exactly which
            // subcommands the call site ran and with what arguments.
            std::fs::write(
                &script,
                format!(
                    "#!/bin/sh\n\
                     printf '%s\\t' \"$@\" >> \"{calls}\"\n\
                     printf '\\n' >> \"{calls}\"\n\
                     case \"$1\" in\n\
                       get) cat \"{get}\"; exit 0 ;;\n\
                       ls) cat \"{ls}\"; exit 0 ;;\n\
                       ssh-config) cat \"{ssh}\"; exit 0 ;;\n\
                       exec) cat \"{exec_out}\"; exit {code} ;;\n\
                     esac\n\
                     exit 0\n",
                    calls = calls.display(),
                    get = get.display(),
                    ls = ls.display(),
                    ssh = ssh.display(),
                    exec_out = exec_out.display(),
                    code = self.exec_code,
                ),
            )
            .expect("stub script");
            make_executable(&script);
            InstaboxProvider::with_cli(script)
        }

        /// Every invocation's argv, tab separated, in call order.
        fn calls(&self) -> Vec<String> {
            std::fs::read_to_string(self.dir.path().join("calls"))
                .unwrap_or_default()
                .lines()
                .map(str::to_string)
                .collect()
        }

        /// The argv of the single `exec` invocation, or a panic naming what ran instead.
        fn exec_argv(&self) -> Vec<String> {
            let calls = self.calls();
            let exec = calls
                .iter()
                .find(|line| line.starts_with("exec\t"))
                .unwrap_or_else(|| panic!("exec was never invoked; calls: {calls:?}"));
            exec.split('\t').filter(|part| !part.is_empty()).map(str::to_string).collect()
        }
    }

    #[test]
    fn live_ls_fixture_contains_the_known_machine() {
        let rows = parse_list(LS_LIVE).expect("parse live ls");
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.sandbox_id, "m_agqyb2s35jvh2e3brqywyknz6e");
        assert_eq!(row.name.as_deref(), Some("bong-i-1"));
        // The capture was taken while bong-i-1 was running.
        assert_eq!(row.state, SandboxState::Ready);
        assert_eq!(row.created_at, Some(1_793_164_205_156));
    }

    #[test]
    fn live_get_fixture_reads_running_and_the_guest_user() {
        assert_eq!(parse_get(GET_LIVE).expect("parse live get").state, SandboxState::Ready);
        assert_eq!(parse_guest_user(GET_LIVE).expect("guest user"), "indo");
    }

    #[test]
    fn live_ssh_config_yields_hostname_not_the_local_alias() {
        let target = parse_ssh_config(SSH_CONFIG_LIVE).expect("parse ssh config");
        assert_eq!(target.host, "ssh.instabox.sh");
        assert_eq!(target.port, 2222);
        assert_eq!(target.user, "m_agqyb2s35jvh2e3brqywyknz6e");
        assert_eq!(
            target.identity,
            SshIdentity::KeyFile(std::path::PathBuf::from(
                "/Users/indo/.config/instabox/ssh/id_ed25519"
            ))
        );
    }

    #[test]
    fn live_ssh_config_carries_the_certificate() {
        // D5: the key alone is refused with `Permission denied (publickey)`; the certificate is
        // what authenticates. It is named `<sandbox-id>-cert.pub`, so ssh cannot find it from the
        // key's path and the conf must say so explicitly.
        let target = parse_ssh_config(SSH_CONFIG_LIVE).expect("parse ssh config");
        assert_eq!(
            target.certificate_file,
            Some(std::path::PathBuf::from(
                "/Users/indo/.config/instabox/ssh/m_agqyb2s35jvh2e3brqywyknz6e-cert.pub"
            ))
        );
        // InstaBox reaches the box directly; no CLI proxy is involved.
        assert!(target.proxy_command.is_none());
    }

    #[test]
    fn exec_timeout_is_a_bare_integer_not_a_duration() {
        // D2: `--timeout 30s` exits 2 at the flag parser ("invalid value \"30s\" for flag
        // -timeout: parse error"), so every exec failed before running. Asserted through
        // `exec_argv` - the only builder `exec()` uses - so a helper that is correct but bypassed
        // cannot pass this test.
        assert_eq!(exec_timeout_arg(30), "30");
        assert_eq!(exec_timeout_arg(0), "1");
        assert_eq!(exec_timeout_arg(9_999), "600");
        assert_eq!(
            exec_argv("m_probe", "echo hi", 30),
            ["exec", "m_probe", "--timeout", "30", "--", "echo hi"]
        );
        // No element carries a duration suffix, which is exactly what the CLI rejects.
        let argv = exec_argv("m_probe", "echo hi", 30);
        let timeout_element = &argv[3];
        assert!(
            timeout_element.parse::<u64>().is_ok(),
            "the timeout element must be a bare integer, got {timeout_element:?}"
        );
    }

    #[test]
    fn full_account_reports_conflict_not_retryable() {
        // This is the plan's required QA scenario, asserted on the real account's reply.
        let output = CliOutput {
            stdout: String::new(),
            stderr: QUOTA_ERROR_LIVE.to_string(),
            code: Some(1),
        };
        let error = cli_run::failure(PROVIDER, EXE, &output);
        assert_eq!(error.kind, ErrorKind::Conflict);
        assert!(error.message.contains("quota_exceeded"), "{}", error.message);
    }

    #[test]
    fn expired_session_reports_config() {
        let output = CliOutput {
            stdout: UNAUTHENTICATED_LIVE.to_string(),
            stderr: String::new(),
            code: Some(1),
        };
        assert_eq!(cli_run::failure(PROVIDER, EXE, &output).kind, ErrorKind::Config);
    }

    #[test]
    fn usage_fixture_shows_a_single_slot() {
        let usage: InstaboxUsage = serde_json::from_str(USAGE_LIVE).expect("parse usage");
        assert_eq!(usage.limits.max_machines, 1);
        assert_eq!(usage.machines, 1);
    }

    #[test]
    fn states_map_both_directions_of_pause() {
        assert_eq!(map_state("running"), SandboxState::Ready);
        assert_eq!(map_state("suspended"), SandboxState::Stopped);
        assert_eq!(map_state("stopped"), SandboxState::Stopped);
        assert_eq!(map_state("suspending"), SandboxState::Stopping);
        assert_eq!(map_state("provisioning"), SandboxState::Provisioning);
        assert_eq!(map_state("failed"), SandboxState::Error);
    }

    #[test]
    fn durations_use_the_cli_suffix() {
        assert_eq!(seconds_arg(1800), "1800s");
        assert_eq!(seconds_arg(0), "0s");
    }

    #[test]
    fn auto_suspend_clamps_to_the_range_the_flag_accepts() {
        // The CLI documents 2m..24h; anything shorter would be rejected at the flag.
        assert_eq!(auto_suspend_arg(30), "120s");
        assert_eq!(auto_suspend_arg(1800), "1800s");
        assert_eq!(auto_suspend_arg(7 * 24 * 60 * 60), "86400s");
    }

    #[tokio::test]
    async fn create_refuses_env_before_spawning_anything() {
        let provider = InstaboxProvider::new();
        let mut env = std::collections::BTreeMap::new();
        env.insert("FOO".to_string(), "bar".to_string());
        // A name that cannot exist keeps the pre-flight `get` on the NotFound path.
        let error = provider
            .create(CreateSandbox {
                idempotency_key: "fx-ib-1".into(),
                name: "fx-nonexistent-t9-probe".into(),
                ttl_seconds: None,
                env,
            })
            .await
            .expect_err("env cannot be honoured by instabox");
        assert_eq!(error.kind, ErrorKind::Config);
        assert_eq!(error.provider_code, "env_unsupported");
    }

    /// D4, live: a suspended machine must be refused *before* `instabox ssh-config` runs, because
    /// that command refreshes a certificate tied to a live sandbox and was observed taking
    /// bong-i-1 from suspended rev 280 to running rev 281.
    ///
    /// Ignored by default: it needs a real key and a real suspended machine, and `get` is the
    /// only call it makes, so it cannot wake or bill anything. Run on request with
    /// `cargo test --lib -- --ignored instabox_live_ssh_target_refuses_a_suspended_machine`.
    #[tokio::test]
    #[ignore = "live: needs INSTABOX_API_KEY and a suspended sandbox"]
    async fn instabox_live_ssh_target_refuses_a_suspended_machine() {
        let id = std::env::var("FERRYX_IT_SANDBOX")
            .expect("set FERRYX_IT_SANDBOX to a suspended InstaBox sandbox id");
        let provider = InstaboxProvider::new();
        // `get` does not bump the revision (verified: rev unchanged across the call), so this
        // assertion is safe to run against a real suspended box.
        let before = provider.get(&id).await.expect("get");
        assert_eq!(
            before.state,
            SandboxState::Stopped,
            "this proof needs a suspended machine; {id} is {:?}",
            before.state
        );
        let error = provider
            .ssh_target(&id)
            .await
            .expect_err("a suspended machine has no usable ssh target");
        assert_eq!(error.kind, ErrorKind::Conflict);
        assert_eq!(error.provider_code, "ssh_needs_running");
        // And the refused call left the machine exactly where it was.
        let after = provider.get(&id).await.expect("get");
        assert_eq!(after.state, SandboxState::Stopped, "the guard must not wake the box");
    }

    /// D2, at the CALL SITE: `exec()` really builds its argv through `exec_argv`, so the integer
    /// `--timeout` cannot be lost by a future edit that re-inlines the builder.
    ///
    /// The verifier's V4 control replaced `exec`'s `exec_argv(...)` call with an inline
    /// `exec_args`+`seconds_arg` builder and the whole suite stayed green: the only assertion on
    /// the shape lived on the helper. This test drives `exec()` itself against a stub CLI and
    /// reads the argv the process really received, so that mutation fails here.
    #[tokio::test]
    async fn exec_builds_its_argv_through_exec_argv() {
        let stub = StubCli::new().exec_answering("hi\n", 0);
        let provider = stub.provider();

        let result = provider
            .exec("m_probe", "echo hi", 30)
            .await
            .expect("exec through the stub CLI");
        assert_eq!(result.exit_code, Some(0));
        assert_eq!(result.stdout, "hi\n");

        // The exact argv `exec_argv` produces, with the timeout as a bare integer. A duration
        // (`30s`) here is the D2 defect and fails the CLI's flag parser before it ever runs.
        assert_eq!(
            stub.exec_argv(),
            vec!["exec", "m_probe", "--timeout", "30", "--", "echo hi"]
        );
    }

    /// D4, at the CALL SITE, in the default suite: a machine that is not `Ready` is refused by
    /// `ssh_target` with `Conflict ssh_needs_running`, and `instabox ssh-config` is never run.
    ///
    /// That second half is the billing rule: `ssh-config` refreshes a certificate tied to a live
    /// sandbox and was observed taking a suspended box to running. The verifier's U1 could not
    /// run this guard at all because the only cover was `#[ignore]`d, so `cargo test` could not
    /// catch its removal. This one runs against a stub that reports `suspended`.
    #[tokio::test]
    async fn ssh_target_refuses_a_suspended_machine_and_never_asks_for_a_config() {
        const SUSPENDED: &str = r#"{
            "id": "m_agqyb2s35jvh2e3brqywyknz6e",
            "name": "bong-i-1",
            "state": "suspended",
            "guestUser": "indo"
        }"#;
        let stub = StubCli::new().reporting(SUSPENDED);
        let provider = stub.provider();

        let error = provider
            .ssh_target("m_agqyb2s35jvh2e3brqywyknz6e")
            .await
            .expect_err("a suspended machine has no usable ssh target");
        assert_eq!(error.kind, ErrorKind::Conflict);
        assert_eq!(error.provider_code, "ssh_needs_running");
        // The message names the state it refused, in the trait's own vocabulary (`suspended` ->
        // `stopped`), so an operator knows what to resume.
        assert!(
            error.message.contains("stopped"),
            "the message must name the state it refused: {}",
            error.message
        );

        // Only the guard's own read ran; nothing that could wake or bill the machine did.
        let calls = stub.calls();
        assert!(
            !calls.iter().any(|line| line.starts_with("ssh-config")),
            "the guard must refuse before `instabox ssh-config` can wake the box; calls: {calls:?}"
        );
        assert_eq!(calls.len(), 1, "the guard makes exactly one CLI call (get): {calls:?}");
        assert!(calls[0].starts_with("get\t"), "calls: {calls:?}");
    }

    /// The other half of D4: a `Ready` machine still gets its ssh target, so the guard is not
    /// simply a blanket refusal.
    #[tokio::test]
    async fn ssh_target_answers_a_running_machine() {
        let stub = StubCli::new();
        let provider = stub.provider();

        let target = provider
            .ssh_target("m_agqyb2s35jvh2e3brqywyknz6e")
            .await
            .expect("a running machine has an ssh target");
        assert_eq!(target.host, "ssh.instabox.sh");
        assert!(stub.calls().iter().any(|line| line.starts_with("ssh-config\t")));
    }
}
