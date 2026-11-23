//! GitHub Codespaces, driven through the official `gh` CLI.
//!
//! Endpoints this adapter uses, captured from `gh` 2.83.2 on 2026-11-21 (see
//! `tests/fixtures/cloud_host/codespaces/` for the live outputs):
//!
//! | trait call | command |
//! | --- | --- |
//! | `create` | `gh codespace create -R <owner/repo> --display-name <n> --idle-timeout <m>m --default-permissions` |
//! | `get` | `gh codespace view -c <n> --json name,displayName,state,createdAt,idleTimeoutMinutes` |
//! | `list` | `gh codespace list --json name,displayName,state,createdAt,lastUsedAt,repository,machineName,owner` |
//! | `stop` | `gh codespace stop -c <n>` |
//! | `resume` | `gh api --method POST /user/codespaces/<n>/start` (gh has no `start` subcommand) |
//! | `delete` | `gh codespace delete -c <n> --force` |
//! | `exec` | `gh codespace ssh -c <n> -- <command>` (one argv element) |
//! | `ssh_target` | `gh codespace ssh --config -c <n>` (OpenSSH config block) |
//!
//! Three facts about this CLI shape the adapter:
//!
//! - **Create needs a repository.** `gh codespace create` is repository-scoped, and the trait's
//!   `CreateSandbox` carries no repository. The repository comes from the adapter's config
//!   (`FERRYX_CODESPACE_REPO`, `owner/repo`); without it `create` reports `Config` instead of
//!   guessing.
//! - **There is no idempotency key.** GitHub offers none, so `create` first lists codespaces and
//!   returns the one whose display name already matches the request; only a miss creates.
//! - **`gh codespace ssh --config` starts a stopped codespace.** Verified live on 2026-11-21: a
//!   codespace listed as `Shutdown` came back `Available` after that command. `ssh_target` is
//!   therefore only meaningful for a machine `get` already reported `Ready`, and this adapter
//!   refuses it for any other state so a status check can never wake a billing machine.
//! - **`exec` carries one command string, not an argv list.** `gh codespace ssh` hands everything
//!   after `--` to `ssh`, and ssh joins its arguments into a single string before the remote shell
//!   sees them. `-- bash -lc 'echo x'` therefore loses stdout entirely (verified live: stdout
//!   empty, only stderr arrived), so the command is passed as exactly one argument. The remote
//!   shell interprets that string, which is ssh's own contract; no local shell is involved.
//! - **The remote exit status arrives in prose.** A remote `exit 7` makes `gh` itself exit 1 with
//!   `shell closed: exit status 7` on stderr (verified live). [`remote_exit_status`] recovers the
//!   command's code so the caller never mistakes the wrapper's status for the command's.
//!
//! The idle timeout is fixed at create: the REST API's `PATCH /user/codespaces/{name}` accepts
//! only the machine type, display name and recent folders, so `extend_ttl` reports `Config`.
//!
//! **Prerequisites: a repository, a machine type, and a billable account.** A codespace always
//! belongs to a repository, so `create` needs [`REPO_ENV`] (`FERRYX_CODESPACE_REPO=owner/repo`)
//! naming one the signed-in `gh` account can create codespaces in; that value is what `-R`
//! receives. In non-TTY execution (a daemon or piped subprocess), `gh codespace create` fails with
//! `error getting machine type: error getting machine: no terminal` unless `-m`/`--machine` is
//! passed explicitly, so `create` passes [`DEFAULT_MACHINE`] (`basicLinux32gb`, overrideable via
//! [`MACHINE_ENV`]). The other create flags are `--display-name`, `--idle-timeout` and
//! `--default-permissions`, all present in `gh codespace create --help` on 2.83.2.
//!
//! Codespaces is billed per core-hour, so the owner also has to be able to pay: the personal free
//! allowance (120 core-hours and 15 GB-months of storage per month) or the org's spending limit.
//! An account with no payment method, with Codespaces disabled for the repository, or over its
//! spending limit fails the create **after** printing
//! `Codespaces usage for this repository is paid for by <owner>` - an assurance line, not the
//! error. `gh` writes the reason on the lines below it, which is why every failure here goes
//! through [`super::cli_run::failure_full`] rather than [`super::cli_run::failure`]: the live
//! smoke that motivated this lost the cause to the notice (task 14 evidence,
//! `.omo/evidence/ferryx-cloud-host-sandbox/task-14-smoke.md`). A spending-limit or not-enabled
//! refusal is an account setting, so [`super::cli_run::classify`] reports it as `Config`; an
//! account at its codespace maximum is `Conflict`.

use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;

use super::cli_run::{self, CliOutput};
use super::{
    Capabilities, CreateSandbox, ErrorKind, ExecResult, ListedSandbox, SandboxError, SandboxInfo,
    SandboxProvider, SandboxState, SshIdentity, SshTarget, Transport,
};

const PROVIDER: &str = "codespaces";
const EXE: &str = "gh";
/// `gh` is a network client; a listing over a slow link still finishes well inside this.
const QUERY_TIMEOUT: Duration = Duration::from_secs(120);
/// Create clones the repository and provisions the VM, which the CLI waits on.
const CREATE_TIMEOUT: Duration = Duration::from_secs(900);
/// Longest idle timeout GitHub accepts (240 minutes).
const MAX_IDLE_MINUTES: u64 = 240;
/// Environment variable naming the repository `create` provisions in, as `owner/repo`.
pub const REPO_ENV: &str = "FERRYX_CODESPACE_REPO";
/// Environment variable overriding the Codespaces machine type passed to `-m` / `--machine`.
pub const MACHINE_ENV: &str = "FERRYX_CODESPACE_MACHINE";
/// Default machine type passed to `gh codespace create -m` so non-TTY invocations do not fail
/// trying to open an interactive machine-type picker (`error getting machine: no terminal`).
const DEFAULT_MACHINE: &str = "basicLinux32gb";

/// One row of `gh codespace list --json`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodespaceRow {
    pub name: String,
    #[serde(default)]
    pub display_name: Option<String>,
    pub state: String,
    #[serde(default)]
    pub created_at: Option<String>,
}

/// The fields of `gh codespace view --json` this adapter reads.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodespaceView {
    pub name: String,
    #[serde(default)]
    pub display_name: Option<String>,
    pub state: String,
    #[serde(default)]
    pub created_at: Option<String>,
}

pub struct CodespacesProvider {
    /// `owner/repo` that `create` provisions in; `None` when the environment does not name one.
    repo: Option<String>,
    /// Machine type passed to `-m` (`basicLinux32gb` by default, overrideable via [`MACHINE_ENV`]).
    machine: String,
}

impl CodespacesProvider {
    /// Read the repository from `FERRYX_CODESPACE_REPO` and optional machine type from
    /// `FERRYX_CODESPACE_MACHINE`. A missing repository is not an error here: only `create` needs
    /// it, and it reports the missing setting itself.
    pub fn from_env() -> Self {
        let repo = std::env::var(REPO_ENV).ok().map(|value| value.trim().to_string());
        let machine = std::env::var(MACHINE_ENV)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| DEFAULT_MACHINE.to_string());
        Self { repo: repo.filter(|value| !value.is_empty()), machine }
    }

    pub fn with_repo(repo: impl Into<String>) -> Self {
        Self { repo: Some(repo.into()), machine: DEFAULT_MACHINE.to_string() }
    }

    /// A provider instance that will refuse `create` because no repository is configured.
    pub fn without_repo() -> Self {
        Self { repo: None, machine: DEFAULT_MACHINE.to_string() }
    }

    fn repo_or_config(&self) -> Result<&str, SandboxError> {
        self.repo.as_deref().ok_or_else(|| {
            SandboxError::new(
                ErrorKind::Config,
                PROVIDER,
                "repo_unset",
                format!("set {REPO_ENV}=owner/repo; `gh codespace create` is repository-scoped"),
            )
        })
    }
}

impl Default for CodespacesProvider {
    fn default() -> Self {
        Self::from_env()
    }
}

/// Parse `gh codespace list --json ...`. `gh` prints a JSON array.
pub fn parse_list(stdout: &str) -> Result<Vec<ListedSandbox>, SandboxError> {
    let rows: Vec<CodespaceRow> = serde_json::from_str(stdout).map_err(|error| {
        SandboxError::new(
            ErrorKind::Fatal,
            PROVIDER,
            "list_parse_failed",
            format!("gh codespace list did not return JSON: {error}"),
        )
    })?;
    Ok(rows
        .into_iter()
        .map(|row| ListedSandbox {
            sandbox_id: row.name,
            state: map_state(&row.state),
            created_at: row.created_at.as_deref().and_then(epoch_ms),
            name: row.display_name,
        })
        .collect())
}

/// Parse `gh codespace view --json ...`.
pub fn parse_view(stdout: &str) -> Result<SandboxInfo, SandboxError> {
    let view: CodespaceView = serde_json::from_str(stdout).map_err(|error| {
        SandboxError::new(
            ErrorKind::Fatal,
            PROVIDER,
            "view_parse_failed",
            format!("gh codespace view did not return JSON: {error}"),
        )
    })?;
    Ok(SandboxInfo { state: map_state(&view.state), error: None })
}

/// GitHub's lifecycle vocabulary (REST `Codespace.state`) onto the trait's six states.
pub fn map_state(state: &str) -> SandboxState {
    match state {
        "Available" => SandboxState::Ready,
        "Shutdown" | "Archived" | "Unavailable" => SandboxState::Stopped,
        "ShuttingDown" => SandboxState::Stopping,
        "Deleted" | "Moved" => SandboxState::Deleted,
        "Failed" | "Unknown" => SandboxState::Error,
        _ => SandboxState::Provisioning,
    }
}

/// Parse `gh codespace ssh --config` into an SSH endpoint.
///
/// The block `gh` writes is:
///
/// ```text
/// Host cs.<codespace-name>.<ref>
///     User vscode
///     ProxyCommand /opt/homebrew/bin/gh cs ssh -c <name> --stdio -- -i <key>
///     UserKnownHostsFile=/dev/null
///     StrictHostKeyChecking no
///     LogLevel quiet
///     ControlMaster auto
///     IdentityFile <key>
/// ```
///
/// Every line above that decides whether a connection can be made is carried: `Host`/`User`,
/// the identity, and - the one that matters - `ProxyCommand`. `cs.<name>.main` has no DNS record
/// at all (`dig @1.1.1.1 +short cs.<name>.main` is empty), so `gh cs ssh --stdio` **is** the
/// route to the box; an earlier version dropped this line and produced a target nothing could
/// reach. The remaining directives (`LogLevel quiet`, `ControlMaster auto`) only affect noise and
/// connection reuse, and ferryx's per-machine conf overrides them with the same bounds every
/// other provider gets: `BatchMode yes`, `StrictHostKeyChecking` against a per-machine
/// known_hosts, `ControlMaster no`.
///
/// The host key is not pinned by GitHub (the generated config disables host checking), so
/// `known_hosts_line` stays `None` and the caller records the key on first use.
pub fn parse_ssh_config(text: &str) -> Result<SshTarget, SandboxError> {
    let mut host: Option<String> = None;
    let mut hostname: Option<String> = None;
    let mut user: Option<String> = None;
    let mut identity: Option<PathBuf> = None;
    let mut certificate: Option<PathBuf> = None;
    let mut proxy_command: Option<String> = None;
    let mut port: u16 = 22;

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // `gh` writes both `Key value` and `Key=value`; ssh accepts both.
        let (key, value) = match line.split_once(char::is_whitespace) {
            Some(parts) => parts,
            None => match line.split_once('=') {
                Some((key, value)) => (key, value),
                None => continue,
            },
        };
        let value = value.trim().trim_start_matches('=').trim();
        match key.to_ascii_lowercase().as_str() {
            "host" => host = Some(value.to_string()),
            "hostname" => hostname = Some(value.to_string()),
            "user" => user = Some(value.to_string()),
            "port" => {
                if let Ok(parsed) = value.parse() {
                    port = parsed;
                }
            }
            "identityfile" => identity = Some(PathBuf::from(value)),
            "certificatefile" => certificate = Some(PathBuf::from(value)),
            // Verbatim, keyword stripped: the value is a command line ssh runs as-is.
            "proxycommand" => proxy_command = Some(value.to_string()),
            _ => {}
        }
    }

    // `HostName` wins when `gh` starts emitting one; today the box is only reachable through the
    // ProxyCommand and `Host` is the label ssh applies its per-host options to.
    let host = hostname.or(host).ok_or_else(|| incomplete_ssh_config("no Host line"))?;
    let user = user.ok_or_else(|| incomplete_ssh_config("no User line"))?;
    let identity = identity.ok_or_else(|| incomplete_ssh_config("no IdentityFile line"))?;

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

fn incomplete_ssh_config(what: &str) -> SandboxError {
    SandboxError::new(
        ErrorKind::Config,
        PROVIDER,
        "ssh_config_incomplete",
        format!("`gh codespace ssh --config` output has {what}"),
    )
}

/// RFC3339 -> unix milliseconds, the unit `ListedSandbox.created_at` uses.
fn epoch_ms(rfc3339: &str) -> Option<i64> {
    let parsed = time::OffsetDateTime::parse(rfc3339, &time::format_description::well_known::Rfc3339)
        .ok()?;
    i64::try_from(parsed.unix_timestamp_nanos() / 1_000_000).ok()
}

/// Convert a TTL into the `--idle-timeout` duration `gh` parses (`90m`, `1h30m`).
fn idle_timeout_arg(ttl_seconds: u64) -> String {
    let minutes = ttl_seconds.div_ceil(60).clamp(1, MAX_IDLE_MINUTES);
    format!("{minutes}m")
}

async fn run(args: &[&str], timeout: Duration) -> Result<CliOutput, SandboxError> {
    cli_run::run(PROVIDER, EXE, args, timeout).await
}

/// Build the argv for `gh codespace create`. Always includes `-m <machine>` so non-TTY runs do
/// not fail with `error getting machine: no terminal`.
fn create_args<'a>(repo: &'a str, machine: &'a str, name: &'a str, idle: &'a str) -> Vec<&'a str> {
    vec![
        "codespace",
        "create",
        "-R",
        repo,
        "-m",
        machine,
        "--display-name",
        name,
        "--idle-timeout",
        idle,
        "--default-permissions",
    ]
}

/// Convert `gh`'s exit into a [`SandboxError`], keeping every meaningful line of output.
fn cli_failure(output: &CliOutput) -> SandboxError {
    cli_run::failure_full(PROVIDER, EXE, output)
}

/// Run a read-only query and turn a non-zero exit into a typed error.
async fn query(args: &[&str]) -> Result<String, SandboxError> {
    let output = run(args, QUERY_TIMEOUT).await?;
    if cli_run::succeeded(&output) {
        Ok(output.stdout)
    } else {
        Err(cli_failure(&output))
    }
}

/// Recover a remote command's exit status from `gh codespace ssh`'s stderr.
///
/// The wrapper exits 1 whatever the remote command returned and says so on one line:
///
/// ```text
/// shell closed: exit status 7
/// ```
///
/// That line is removed from the returned stderr so the caller sees only the command's own
/// output. `None` means no remote status was reported, i.e. the failure was `gh`'s or ssh's.
fn remote_exit_status(stderr: &str) -> Option<(i32, String)> {
    const MARKER: &str = "shell closed: exit status ";
    let mut status: Option<i32> = None;
    let mut kept: Vec<&str> = Vec::new();
    for line in stderr.lines() {
        match line.trim().strip_prefix(MARKER) {
            Some(rest) => status = rest.trim().parse().ok(),
            None => kept.push(line),
        }
    }
    status.map(|code| (code, kept.join("\n")))
}

#[async_trait::async_trait]
impl SandboxProvider for CodespacesProvider {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            ttl_required: false,
            max_ttl_seconds: Some(MAX_IDLE_MINUTES * 60),
            transport: Transport::Ssh,
            native_auto_stop: true,
        }
    }

    async fn create(&self, request: CreateSandbox) -> Result<String, SandboxError> {
        // `CreateSandbox.env` cannot be honoured: a codespace environment is fixed by its
        // devcontainer. Refuse before spawning anything rather than silently drop the caller's
        // variables.
        if !request.env.is_empty() {
            return Err(SandboxError::new(
                ErrorKind::Config,
                PROVIDER,
                "env_unsupported",
                "a codespace environment comes from its devcontainer; pass no env",
            ));
        }
        let repo = self.repo_or_config()?;

        // No provider-side idempotency key exists, so a display name already in the account is
        // the machine this request means. `displayName` is what `--display-name` sets.
        let listed = parse_list(&query(&["codespace", "list", "--json", "name,displayName,state,createdAt"]).await?)?;
        if let Some(existing) = listed.iter().find(|row| row.name.as_deref() == Some(request.name.as_str())) {
            return Ok(existing.sandbox_id.clone());
        }

        let idle = idle_timeout_arg(request.ttl_seconds.unwrap_or(MAX_IDLE_MINUTES * 60));
        let args = create_args(repo, self.machine.as_str(), request.name.as_str(), idle.as_str());

        let output = run(&args, CREATE_TIMEOUT).await?;
        if !cli_run::succeeded(&output) {
            return Err(cli_failure(&output));
        }

        // `gh codespace create` prints the new name on its last line, but that text is not a
        // documented contract. The display name is: read it back.
        let listed = parse_list(&query(&["codespace", "list", "--json", "name,displayName,state,createdAt"]).await?)?;
        listed
            .into_iter()
            .find(|row| row.name.as_deref() == Some(request.name.as_str()))
            .map(|row| row.sandbox_id)
            .ok_or_else(|| {
                SandboxError::new(
                    ErrorKind::Fatal,
                    PROVIDER,
                    "create_not_visible",
                    format!("gh created a codespace but none is named {}", request.name),
                )
            })
    }

    async fn get(&self, sandbox_id: &str) -> Result<SandboxInfo, SandboxError> {
        let stdout = query(&[
            "codespace",
            "view",
            "-c",
            sandbox_id,
            "--json",
            "name,displayName,state,createdAt,idleTimeoutMinutes",
        ])
        .await?;
        parse_view(&stdout)
    }

    async fn stop(&self, sandbox_id: &str) -> Result<(), SandboxError> {
        let output = run(&["codespace", "stop", "-c", sandbox_id], QUERY_TIMEOUT).await?;
        if cli_run::succeeded(&output) {
            Ok(())
        } else {
            Err(cli_failure(&output))
        }
    }

    async fn resume(&self, sandbox_id: &str, _ttl_seconds: Option<u64>) -> Result<(), SandboxError> {
        // `gh` exposes no `start`; `gh api` is the same CLI's documented REST surface, so the
        // call stays inside the official tool and no HTTP client of ours is involved.
        let path = format!("/user/codespaces/{sandbox_id}/start");
        let output = run(&["api", "--method", "POST", path.as_str()], CREATE_TIMEOUT).await?;
        if cli_run::succeeded(&output) {
            Ok(())
        } else {
            Err(cli_failure(&output))
        }
    }

    async fn delete(&self, sandbox_id: &str) -> Result<(), SandboxError> {
        let output = run(&["codespace", "delete", "-c", sandbox_id, "--force"], QUERY_TIMEOUT).await?;
        if cli_run::succeeded(&output) {
            Ok(())
        } else {
            Err(cli_failure(&output))
        }
    }

    async fn extend_ttl(&self, _sandbox_id: &str, _ttl_seconds: Option<u64>) -> Result<(), SandboxError> {
        Err(SandboxError::new(
            ErrorKind::Config,
            PROVIDER,
            "ttl_fixed_at_create",
            "GitHub fixes the idle timeout at create; delete and recreate to change it",
        ))
    }

    async fn exec(
        &self,
        sandbox_id: &str,
        command: &str,
        timeout_seconds: u64,
    ) -> Result<ExecResult, SandboxError> {
        // See the module note: exactly one command argument, interpreted by the codespace's own
        // shell. Splitting it into argv elements silently loses stdout.
        let output = run(
            &["codespace", "ssh", "-c", sandbox_id, "--", command],
            Duration::from_secs(timeout_seconds.clamp(1, 3600)),
        )
        .await?;

        // The remote command ran, whatever it returned: `gh` reports its status in prose and
        // exits 1 itself. Recover the real code instead of the wrapper's.
        if let Some((code, stderr)) = remote_exit_status(&output.stderr) {
            return Ok(ExecResult {
                exit_code: Some(code),
                stdout: output.stdout,
                stderr,
                timed_out: false,
            });
        }
        if !cli_run::succeeded(&output) {
            // No remote status in the output: ssh or gh itself failed (no session, no such
            // codespace, a network fault), so this is not the command's exit code.
            return Err(cli_failure(&output));
        }
        Ok(ExecResult {
            exit_code: Some(0),
            stdout: output.stdout,
            stderr: output.stderr,
            timed_out: false,
        })
    }

    async fn list(&self) -> Result<Vec<ListedSandbox>, SandboxError> {
        parse_list(&query(&["codespace", "list", "--json", "name,displayName,state,createdAt"]).await?)
    }

    async fn ssh_target(&self, sandbox_id: &str) -> Result<SshTarget, SandboxError> {
        // Guard the billing rule before the CLI: `gh codespace ssh --config` starts a stopped
        // codespace (verified live), and a status check must never do that.
        let info = self.get(sandbox_id).await?;
        if info.state != SandboxState::Ready {
            return Err(SandboxError::new(
                ErrorKind::Conflict,
                PROVIDER,
                "ssh_needs_running",
                format!(
                    "codespace {sandbox_id} is {}; resume it before asking for an ssh target \
                     (gh codespace ssh --config would start it)",
                    info.state
                ),
            ));
        }
        let stdout = query(&["codespace", "ssh", "--config", "-c", sandbox_id]).await?;
        parse_ssh_config(&stdout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST_LIVE: &str = include_str!("../../../tests/fixtures/cloud_host/codespaces/list_live.json");
    const VIEW_LIVE: &str = include_str!("../../../tests/fixtures/cloud_host/codespaces/view_live.json");
    const SSH_CONFIG_LIVE: &str =
        include_str!("../../../tests/fixtures/cloud_host/codespaces/ssh_config_live.txt");

    #[test]
    fn live_list_fixture_parses_to_the_known_codespace() {
        let rows = parse_list(LIST_LIVE).expect("parse live list");
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.sandbox_id, "ghastly-troll-vqw6w44vr4gcprw5");
        assert_eq!(row.name.as_deref(), Some("ghastly troll"));
        // The live capture was taken while the codespace was shut down (`gh codespace list`
        // reports GitHub's own `Shutdown`).
        assert_eq!(row.state, SandboxState::Stopped);
        assert_eq!(row.created_at, Some(1_793_321_607_000));
    }

    #[test]
    fn live_view_fixture_maps_shutdown_to_stopped() {
        let info = parse_view(VIEW_LIVE).expect("parse live view");
        assert_eq!(info.state, SandboxState::Stopped);
        assert!(info.error.is_none());
    }

    #[test]
    fn live_ssh_config_yields_the_documented_target() {
        let target = parse_ssh_config(SSH_CONFIG_LIVE).expect("parse ssh config");
        assert_eq!(target.host, "cs.ghastly-troll-vqw6w44vr4gcprw5.main");
        assert_eq!(target.user, "vscode");
        assert_eq!(target.port, 22);
        assert_eq!(
            target.identity,
            SshIdentity::KeyFile(PathBuf::from("/Users/indo/.ssh/codespaces.auto"))
        );
        // GitHub's own config disables host checking; we record no pinned line rather than
        // pretending one exists.
        assert!(target.known_hosts_line.is_none());
    }

    #[test]
    fn live_ssh_config_carries_the_proxy_command() {
        // D1: `cs.<name>.main` has no DNS record. Without this line the target is unusable,
        // which is exactly the bug the live `ssh -F` proof pins (see task-9-verify.txt).
        let target = parse_ssh_config(SSH_CONFIG_LIVE).expect("parse ssh config");
        let proxy = target.proxy_command.expect("ProxyCommand must be carried");
        assert_eq!(
            proxy,
            "/opt/homebrew/bin/gh cs ssh -c ghastly-troll-vqw6w44vr4gcprw5 --stdio -- \
             -i /Users/indo/.ssh/codespaces.auto"
        );
        // Codespaces authenticates with the key itself, so no separate certificate exists.
        assert!(target.certificate_file.is_none());
    }

    #[test]
    fn equals_form_directives_parse_like_space_form() {
        // `gh` writes `UserKnownHostsFile=/dev/null` with `=` and `User vscode` with a space.
        let text = "Host cs.x.main\n\tUser vscode\n\tIdentityFile=/k\n\tPort=2222\n";
        let target = parse_ssh_config(text).expect("parse");
        assert_eq!(target.user, "vscode");
        assert_eq!(target.port, 2222);
        assert_eq!(target.identity, SshIdentity::KeyFile(PathBuf::from("/k")));
    }

    #[test]
    fn every_documented_state_maps() {
        let cases = [
            ("Available", SandboxState::Ready),
            ("Shutdown", SandboxState::Stopped),
            ("Archived", SandboxState::Stopped),
            ("Unavailable", SandboxState::Stopped),
            ("ShuttingDown", SandboxState::Stopping),
            ("Deleted", SandboxState::Deleted),
            ("Moved", SandboxState::Deleted),
            ("Failed", SandboxState::Error),
            ("Unknown", SandboxState::Error),
            ("Provisioning", SandboxState::Provisioning),
            ("Queued", SandboxState::Provisioning),
            ("Starting", SandboxState::Provisioning),
            ("Rebuilding", SandboxState::Provisioning),
        ];
        for (raw, expected) in cases {
            assert_eq!(map_state(raw), expected, "state {raw}");
        }
    }

    #[test]
    fn idle_timeout_is_rounded_up_to_minutes_and_capped() {
        assert_eq!(idle_timeout_arg(30), "1m");
        assert_eq!(idle_timeout_arg(1800), "30m");
        assert_eq!(idle_timeout_arg(86_400), "240m");
    }

    #[test]
    fn create_without_a_repo_names_the_setting() {
        let provider = CodespacesProvider::without_repo();
        let error = provider.repo_or_config().expect_err("no repo configured");
        assert_eq!(error.kind, ErrorKind::Config);
        assert_eq!(error.provider_code, "repo_unset");
        assert!(error.message.contains("FERRYX_CODESPACE_REPO"));
    }

    #[test]
    fn remote_exit_status_is_recovered_and_stripped() {
        let (code, stderr) = remote_exit_status("shell closed: exit status 7\n").expect("status");
        assert_eq!(code, 7);
        assert_eq!(stderr, "");

        // A command that wrote to stderr keeps its text; only the wrapper's line goes.
        let (code, stderr) =
            remote_exit_status("ERR-1\nshell closed: exit status 5\n").expect("status");
        assert_eq!(code, 5);
        assert_eq!(stderr, "ERR-1");

        // A gh/ssh failure carries no remote status: the caller must treat it as an error.
        assert_eq!(remote_exit_status("no such codespace\n"), None);
        assert_eq!(remote_exit_status(""), None);
    }

    #[test]
    fn malformed_list_json_is_a_fatal_parse_error() {
        let error = parse_list("not json").expect_err("garbage must not parse");
        assert_eq!(error.kind, ErrorKind::Fatal);
        assert_eq!(error.provider_code, "list_parse_failed");
    }

    /// The failure that ended the task 14 live smoke at `gh_exit_1` with no reason: `gh codespace
    /// create` prints the billing assurance first and the cause underneath it. Both must reach
    /// `SandboxError.message`, because the reason is the part an operator can act on.
    #[test]
    fn create_failure_keeps_the_billing_notice_and_the_error_below_it() {
        let output = CliOutput {
            stdout: String::new(),
            stderr: "\u{2713} Codespaces usage for this repository is paid for by Indosaram\n\
                     HTTP 422: Validation Failed (https://api.github.com/user/codespaces)\n\
                     Repository Indosaram/build-farm is not enabled for Codespaces\n"
                .to_string(),
            code: Some(1),
        };
        let error = cli_failure(&output);
        assert_eq!(error.provider, PROVIDER);
        assert_eq!(error.provider_code, "gh_exit_1");
        assert!(
            error.message.contains("paid for by Indosaram"),
            "the notice gh printed must not be dropped: {}",
            error.message
        );
        assert!(
            error.message.contains("HTTP 422: Validation Failed"),
            "the cause must survive the notice: {}",
            error.message
        );
        assert!(
            error.message.contains("not enabled for Codespaces"),
            "every line of the reason is kept: {}",
            error.message
        );
        // "not enabled for" is the account-side prerequisite: a setting to change, not a retry.
        assert_eq!(error.kind, ErrorKind::Config);
    }

    #[test]
    fn create_failure_single_line_and_stdout_only_still_report_cleanly() {
        // One line on stderr: unchanged from the first-line behaviour, minus the wrapper.
        let one_line = CliOutput {
            stdout: String::new(),
            stderr: "no such codespace: fx-cs-1\n".to_string(),
            code: Some(1),
        };
        let error = cli_failure(&one_line);
        assert_eq!(error.message, "no such codespace: fx-cs-1");
        assert_eq!(error.kind, ErrorKind::NotFound);

        // `gh` writes some failures to stdout (a device-flow prompt, `gh`'s own usage text).
        let stdout_only = CliOutput {
            stdout: "To get started with GitHub CLI, please run: gh auth login\n".to_string(),
            stderr: String::new(),
            code: Some(4),
        };
        let error = cli_failure(&stdout_only);
        assert!(error.message.contains("gh auth login"), "{}", error.message);
        assert_eq!(error.provider_code, "gh_exit_4");
        assert_eq!(error.kind, ErrorKind::Config);

        // Silence still produces a message rather than an empty `[fatal] codespaces: `.
        let silent = CliOutput { stdout: String::new(), stderr: String::new(), code: Some(1) };
        assert_eq!(cli_failure(&silent).message, "no output");
    }

    /// A spending-limit refusal is the other billing shape: the account cannot pay, so the create
    /// must report `Config` with GitHub's own words rather than a bare `gh_exit_1`.
    #[test]
    fn create_spending_limit_refusal_is_config_with_the_reason_kept() {
        let output = CliOutput {
            stdout: String::new(),
            stderr: "\u{2713} Codespaces usage for this repository is paid for by Indosaram\n\
                     error creating codespace: HTTP 402: You have exceeded your spending limit\n"
                .to_string(),
            code: Some(1),
        };
        let error = cli_failure(&output);
        assert_eq!(error.kind, ErrorKind::Config);
        assert!(error.message.contains("exceeded your spending limit"), "{}", error.message);
    }

    #[test]
    fn create_args_always_passes_machine_for_non_tty_runs() {
        let args = create_args("Indosaram/build-farm", DEFAULT_MACHINE, "fx-smoke-codespaces", "15m");
        assert!(args.windows(2).any(|w| w == ["-R", "Indosaram/build-farm"]));
        assert!(
            args.windows(2).any(|w| w == ["-m", "basicLinux32gb"]),
            "missing -m makes non-TTY gh fail with 'error getting machine: no terminal': {args:?}"
        );
        assert!(args.windows(2).any(|w| w == ["--display-name", "fx-smoke-codespaces"]));
        assert!(args.windows(2).any(|w| w == ["--idle-timeout", "15m"]));
        assert!(args.contains(&"--default-permissions"));
    }

    #[tokio::test]
    async fn create_refuses_env_before_spawning_anything() {
        let provider = CodespacesProvider::with_repo("Indosaram/build-farm");
        let mut env = std::collections::BTreeMap::new();
        env.insert("FOO".to_string(), "bar".to_string());
        let error = provider
            .create(CreateSandbox {
                idempotency_key: "fx-cs-1".into(),
                name: "fx-cs-1".into(),
                ttl_seconds: Some(1800),
                env,
            })
            .await
            .expect_err("env cannot be honoured by a codespace");
        assert_eq!(error.kind, ErrorKind::Config);
        assert_eq!(error.provider_code, "env_unsupported");
    }

    #[tokio::test]
    async fn extend_ttl_reports_that_github_fixes_it_at_create() {
        let provider = CodespacesProvider::with_repo("Indosaram/build-farm");
        let error = provider.extend_ttl("cs", Some(1800)).await.expect_err("fixed at create");
        assert_eq!(error.kind, ErrorKind::Config);
        assert_eq!(error.provider_code, "ttl_fixed_at_create");
    }
}
