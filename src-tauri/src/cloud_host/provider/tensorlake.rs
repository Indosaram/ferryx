//! Tensorlake, driven through the official `tl` CLI.
//!
//! Commands, captured from `tl` 0.5.165 on 2026-11-21 (`tests/fixtures/cloud_host/tensorlake/`):
//!
//! | trait call | command |
//! | --- | --- |
//! | `create` | `tl sbx create <name> --timeout <secs> --queue` (prints the sandbox id) |
//! | `get` | `tl sbx describe <name>` (table; no JSON mode) |
//! | `list` | `tl sbx ls --all` (tab-separated table; no JSON mode) |
//! | `stop` | `tl sbx suspend <name> --no-wait` |
//! | `resume` | `tl sbx resume <name> --no-wait` |
//! | `delete` | `tl sbx terminate <name>` |
//! | `exec` | `tl sbx exec <name> --timeout <secs> bash -lc <command>` |
//! | `ssh_target` | `tl sbx describe <name>` `SSH Config:` block |
//!
//! **No JSON output.** Unlike `gh` and `instabox`, `tl sbx ls` and `tl sbx describe` have no
//! `--json` flag, so this adapter parses the documented tables instead:
//!
//! `tl sbx ls` prints a tab-separated header plus one row per sandbox and a trailing count:
//!
//! ```text
//! ID\tName\tStatus\tImage\tCPUs\tMemory\tDisk\tGPUs\tCreated At
//! s7jus08qec4axzgbpq76h\tfx-probe\trunning\ttensorlake/ubuntu\t1\t1024 MiB\t10240 MiB\t-\t34 min ago
//! 1 sandbox
//! ```
//!
//! `tl sbx describe` prints `Key:` / value columns, and after `SSH Config:` an OpenSSH block.
//! Both parsers reject a table whose header does not match, rather than guessing column
//! positions, so a future `tl` that reorders columns fails loudly instead of reading the wrong
//! field.
//!
//! **Auth.** `tl login` is a browser/device flow (`we're going to open a web browser for you to
//! enter a one-time code`), so live QA is `SKIPPED: approval pending` until it is done on a
//! machine with a browser. The CLI also accepts `TENSORLAKE_API_KEY`/`TENSORLAKE_PAT` for
//! non-browser projects; nothing here handles either value.
//!
//! **Create does not block on the boot.** `tl sbx create` without a queue flag waits for the
//! sandbox to reach `running`, and a metal host can take 20 minutes; `--queue` is the documented
//! non-blocking form (`return at once with its ID instead of blocking until it runs`), so create
//! returns an id that `get` then polls, which is the contract the trait describes. The CLI also
//! accepts an undocumented `--no-wait` spelling for the same thing; the documented flag is used.
//!
//! `created_at` is only available as a coarse relative age (`34 min ago`) from `ls`. That is
//! enough to order machines for the orphan sweep, and it is the only form the CLI offers.

use std::path::PathBuf;
use std::time::Duration;

use super::cli_run::{self, CliOutput};
use super::{
    Capabilities, CreateSandbox, ErrorKind, ExecResult, ListedSandbox, SandboxError, SandboxInfo,
    SandboxProvider, SandboxState, SshIdentity, SshTarget, Transport,
};

const PROVIDER: &str = "tensorlake";
const EXE: &str = "tl";
const QUERY_TIMEOUT: Duration = Duration::from_secs(120);
const CREATE_TIMEOUT: Duration = Duration::from_secs(900);
/// `tl sbx create` defaults to 600 s; the free plan caps `timeout_secs` at two hours.
const DEFAULT_TIMEOUT_SECONDS: u64 = 600;
const MAX_TIMEOUT_SECONDS: u64 = 2 * 60 * 60;

/// The exact header `tl sbx ls` prints, in order. A mismatch is an error, not a guess.
const LS_HEADER: [&str; 9] =
    ["ID", "Name", "Status", "Image", "CPUs", "Memory", "Disk", "GPUs", "Created At"];

/// One sandbox parsed out of `tl sbx ls`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TensorlakeRow {
    pub sandbox_id: String,
    pub name: String,
    pub status: String,
    /// Raw `Created At` cell, e.g. `34 min ago`.
    pub created: String,
}

/// Parse `tl sbx ls` (default table, tab separated).
///
/// `No sandboxes found.` is the empty case. The trailing `1 sandbox` / `2 sandboxes` summary
/// line is ignored.
pub fn parse_ls_table(stdout: &str) -> Result<Vec<TensorlakeRow>, SandboxError> {
    let mut lines = stdout.lines().filter(|line| !line.trim().is_empty());
    let Some(header) = lines.next() else {
        return Ok(Vec::new());
    };
    if header.trim() == "No sandboxes found." {
        return Ok(Vec::new());
    }

    let header_cells: Vec<&str> = header.split('\t').map(str::trim).collect();
    if header_cells != LS_HEADER {
        return Err(SandboxError::new(
            ErrorKind::Fatal,
            PROVIDER,
            "ls_header_unexpected",
            format!("tl sbx ls header changed; expected {LS_HEADER:?}, got {header_cells:?}"),
        ));
    }

    let mut rows = Vec::new();
    for line in lines {
        let trimmed = line.trim();
        // The summary line: `1 sandbox`, `3 sandboxes`.
        if trimmed.ends_with("sandbox") || trimmed.ends_with("sandboxes") {
            continue;
        }
        let cells: Vec<&str> = line.split('\t').map(str::trim).collect();
        if cells.len() != LS_HEADER.len() {
            return Err(SandboxError::new(
                ErrorKind::Fatal,
                PROVIDER,
                "ls_row_unexpected",
                format!("tl sbx ls row has {} columns, expected {}", cells.len(), LS_HEADER.len()),
            ));
        }
        rows.push(TensorlakeRow {
            sandbox_id: cells[0].to_string(),
            name: cells[1].to_string(),
            status: cells[2].to_string(),
            created: cells[8].to_string(),
        });
    }
    Ok(rows)
}

/// The `Key:`/value pairs and `SSH Config:` block `tl sbx describe` prints.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TensorlakeDescribe {
    pub fields: Vec<(String, String)>,
    pub ssh_config: Option<String>,
}

/// Parse `tl sbx describe`. Keys keep their printed form (`ID`, `Status`, `Created`, ...).
pub fn parse_describe(stdout: &str) -> TensorlakeDescribe {
    let mut describe = TensorlakeDescribe::default();
    let mut in_ssh = false;
    let mut ssh_lines: Vec<&str> = Vec::new();

    for raw in stdout.lines() {
        if let Some(rest) = raw.strip_prefix("SSH Config:") {
            in_ssh = true;
            // The block may start on the same line in a future version.
            if !rest.trim().is_empty() {
                ssh_lines.push(rest.trim());
            }
            continue;
        }
        if in_ssh {
            // The block is indented; a blank line ends it.
            if raw.trim().is_empty() {
                in_ssh = false;
                continue;
            }
            ssh_lines.push(raw);
            continue;
        }
        if let Some((key, value)) = raw.split_once(':') {
            let key = key.trim();
            if !key.is_empty() && !key.contains(char::is_whitespace) {
                describe.fields.push((key.to_string(), value.trim().to_string()));
            }
        }
    }

    if !ssh_lines.is_empty() {
        describe.ssh_config = Some(ssh_lines.join("\n"));
    }
    describe
}

impl TensorlakeDescribe {
    fn field(&self, key: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(key))
            .map(|(_, value)| value.as_str())
    }
}

/// `tl sbx describe` -> [`SandboxInfo`].
pub fn describe_to_info(describe: &TensorlakeDescribe) -> Result<SandboxInfo, SandboxError> {
    let status = describe.field("Status").ok_or_else(|| {
        SandboxError::new(
            ErrorKind::Fatal,
            PROVIDER,
            "describe_missing_status",
            "tl sbx describe printed no Status line",
        )
    })?;
    Ok(SandboxInfo { state: map_state(status), error: describe.field("Error").map(str::to_string) })
}

/// `tl sbx create` prints the new sandbox id on its own line. Returns the first token that
/// looks like a sandbox id (`sandbox_...` prefix or a 21-character opaque id).
pub fn parse_create_stdout(stdout: &str) -> Result<String, SandboxError> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .find_map(|line| {
            let token = line.split_whitespace().next().unwrap_or_default();
            let looks_like_id = token.starts_with("sandbox_")
                || (token.len() == 21 && token.chars().all(|c| c.is_ascii_alphanumeric()));
            looks_like_id.then(|| token.to_string())
        })
        .ok_or_else(|| {
            SandboxError::new(
                ErrorKind::Fatal,
                PROVIDER,
                "create_id_missing",
                "tl sbx create printed no sandbox id",
            )
        })
}

/// The id of an already-existing sandbox with this name, if the listing holds one.
///
/// Split out of [`SandboxProvider::create`] so the reuse decision is testable without spawning
/// `tl`, and so the caller has no way to reach a create without a successfully parsed listing:
/// the rows arrive as a value, not as a `Result` that a `if let Ok` could quietly drop (D6).
fn reuse_by_name(rows: Vec<TensorlakeRow>, name: &str) -> Option<String> {
    rows.into_iter()
        .find(|row| row.name == name)
        .map(|row| row.sandbox_id)
}

/// Tensorlake's lifecycle vocabulary onto the trait's states. Named sandboxes suspend on idle
/// timeout; ephemeral ones terminate.
pub fn map_state(status: &str) -> SandboxState {
    match status {
        "running" => SandboxState::Ready,
        "suspended" | "suspending" => SandboxState::Stopped,
        "terminated" => SandboxState::Deleted,
        "resume_failed" | "failed" | "error" => SandboxState::Error,
        _ => SandboxState::Provisioning,
    }
}

/// Parse the `SSH Config:` block from `tl sbx describe`.
///
/// Live output:
///
/// ```text
/// SSH Config:
/// Host tl-fx-probe
///     HostName fx-probe.sandbox.tensorlake.ai
///     User s7jus08qec4axzgbpq76h
///     IdentityFile ~/.ssh/id_ed25519_tensorlake
///     IdentitiesOnly yes
///     ServerAliveInterval 30
///     ServerAliveCountMax 3
/// ```
///
/// The user is the sandbox id, not a POSIX name: that is how Tensorlake's proxy routes the
/// connection. The identity is a key the operator registered with `tl sbx ssh keys add`, so it
/// is reported as `KeyFile` at its literal path (expandable with `~`).
pub fn parse_ssh_config(text: &str) -> Result<SshTarget, SandboxError> {
    let mut hostname: Option<String> = None;
    let mut alias: Option<String> = None;
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
        let Some((key, value)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let value = value.trim();
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
            // Not emitted by `tl` today, but a describe block that grew either directive must
            // reach the conf rather than be dropped the way Codespaces' ProxyCommand was (D1).
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
            format!("`tl sbx describe` SSH Config has no {what}"),
        )
    };
    let host = hostname.or(alias).ok_or_else(|| missing("HostName"))?;
    let user = user.ok_or_else(|| missing("User line"))?;
    let identity = identity.ok_or_else(|| missing("IdentityFile line"))?;

    Ok(SshTarget {
        host,
        port,
        user,
        identity: SshIdentity::KeyFile(identity),
        known_hosts_line: None,
        // `tl sbx describe` hands out an internet-reachable `HostName` and no proxy hop, and
        // Tensorlake registers a plain key rather than a short-lived certificate.
        proxy_command,
        certificate_file: certificate,
    })
}

/// `tl sbx ls` prints each `Created At` cell as a coarse relative age. Live output observed
/// 2026-11-22: `9 sec ago`, `2 min ago`; the binary's format table is `<n> {s,m,h,d} ago`
/// (`tl` also renders `in under a minute` / `just now` for a fresh machine).
///
/// Returns unix milliseconds for `now_ms - age`, or `None` when the cell is not one of those
/// forms. The caller decides whether `None` is tolerable; `list()` treats it as a format change
/// because a `None` there silently disables the orphan reaper (D7).
fn parse_relative_age(cell: &str, now_ms: i64) -> Option<i64> {
    let cell = cell.trim().to_ascii_lowercase();
    if cell.is_empty() {
        return None;
    }
    // A machine created within the last minute prints without a number.
    if cell == "just now" || cell == "in under a minute" {
        return Some(now_ms);
    }
    let mut parts = cell.split_whitespace();
    let count: i64 = parts.next()?.parse().ok()?;
    let unit = parts.next()?;
    // `ago` closes the phrase; `in <n> <unit>` is the future form and means a boot in progress,
    // not an age, so it is not an age at all.
    if parts.next()? != "ago" || parts.next().is_some() {
        return None;
    }
    let seconds_per_unit: i64 = match unit.trim_end_matches('s') {
        "sec" | "second" => 1,
        "min" | "minute" => 60,
        "h" | "hr" | "hour" => 60 * 60,
        "d" | "day" => 24 * 60 * 60,
        "w" | "week" => 7 * 24 * 60 * 60,
        "mo" | "month" => 30 * 24 * 60 * 60,
        "y" | "year" => 365 * 24 * 60 * 60,
        _ => return None,
    };
    Some(now_ms.saturating_sub(count.saturating_mul(seconds_per_unit).saturating_mul(1000)))
}

/// Tell a `tl`-level failure apart from a remote command's non-zero exit.
///
/// `tl sbx exec` reports both through the same channel: the remote process's status becomes the
/// CLI's own exit code, and a CLI failure (auth, no such sandbox, a transport fault) also exits
/// non-zero. Returning `output.code` unconditionally therefore reported `tl`'s failures as the
/// command's status (D8), contradicting [`ExecResult`]'s contract and the codespaces precedent.
///
/// The live shapes of a `tl` failure share every one of these traits, and a real remote command
/// shares almost none:
///
/// ```text
/// Error: failed to resolve sandbox fx-nonexistent-t9 (HTTP 404 Not Found): Sandbox not found
/// Error: sandbox 'fx-nonexistent-t9' not found
/// Error: failed to terminate sandbox ... (HTTP 404 Not Found): Sandbox not found
/// ```
///
/// so the test is: the CLI exited non-zero, wrote nothing to stdout (`tl` sends data to stdout
/// and errors to stderr, never both), and its first stderr line is `tl`'s own `Error: ` prefix
/// carrying a failure family [`cli_run::classify`] already knows (HTTP 404/401/403/429/5xx, an
/// auth phrase). Requiring the prefix is what keeps an ordinary command that merely printed the
/// word "not found" on stderr from being mistaken for the CLI.
///
/// Residual, stated rather than hidden: a remote command whose *only* stderr is one `Error: `
/// line containing one of those phrases and that also wrote nothing to stdout would be reported
/// as a provider error instead of its own exit code.
fn cli_failure_not_command(output: &CliOutput) -> Option<SandboxError> {
    if cli_run::succeeded(output) {
        return None;
    }
    if !output.stdout.trim().is_empty() {
        return None;
    }
    let first = output.stderr.lines().map(str::trim).find(|line| !line.is_empty())?;
    if !first.starts_with("Error: ") {
        return None;
    }
    cli_run::classify(output).map(|_| cli_run::failure(PROVIDER, EXE, output))
}

/// Turn parsed `ls` rows into the trait's listing, ageing each row against `now_ms`.
///
/// Split out from [`SandboxProvider::list`] so the conversion - and specifically the fact that
/// `created_at` is `Some` - is testable without spawning `tl`.
///
/// `ls` prints a relative age (`38 min ago`), not an instant, so it is converted at read time. A
/// cell the parser does not recognise is a CLI format change, and returning `None` for it would
/// silently disable the orphan reaper for this provider: lifecycle's gate is
/// `created_at.is_some_and(...)`, so a `None` orphan is reported and never deleted (D7).
fn listed_from_rows(rows: Vec<TensorlakeRow>, now_ms: i64) -> Result<Vec<ListedSandbox>, SandboxError> {
    rows.into_iter()
        .map(|row| {
            let created_at = parse_relative_age(&row.created, now_ms).ok_or_else(|| {
                SandboxError::new(
                    ErrorKind::Fatal,
                    PROVIDER,
                    "ls_age_unexpected",
                    format!(
                        "tl sbx ls printed an unreadable Created At cell {:?} for {}; the orphan \
                         sweep cannot age it",
                        row.created, row.name
                    ),
                )
            })?;
            Ok(ListedSandbox {
                sandbox_id: row.sandbox_id,
                state: map_state(&row.status),
                created_at: Some(created_at),
                name: Some(row.name),
            })
        })
        .collect()
}

pub struct TensorlakeProvider {
    /// The `tl` binary every call spawns. Always [`EXE`] outside tests; a test points it at a
    /// stub so the calls a call site really makes can be asserted without an account.
    cli: PathBuf,
}

impl TensorlakeProvider {
    pub fn new() -> Self {
        Self { cli: PathBuf::from(EXE) }
    }

    /// A provider that spawns `cli` instead of the `tl` on PATH.
    #[cfg(test)]
    fn with_cli(cli: impl Into<PathBuf>) -> Self {
        Self { cli: cli.into() }
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

impl Default for TensorlakeProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl SandboxProvider for TensorlakeProvider {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            ttl_required: false,
            max_ttl_seconds: Some(MAX_TIMEOUT_SECONDS),
            transport: Transport::Ssh,
            native_auto_stop: true,
        }
    }

    async fn create(&self, request: CreateSandbox) -> Result<String, SandboxError> {
        // A sandbox's environment is set per command (`--env`) or baked into its image; there is
        // no create-time env map in the CLI, so refuse before spawning anything instead of
        // dropping the caller's values.
        if !request.env.is_empty() {
            return Err(SandboxError::new(
                ErrorKind::Config,
                PROVIDER,
                "env_unsupported",
                "tl sets environment variables per command; create a sandbox image instead",
            ));
        }

        // Names are unique per namespace and a named sandbox supports suspend/resume, so an
        // existing sandbox with this name is this request (Tensorlake has no idempotency key).
        // The listing parse is propagated, not swallowed: `if let Ok(rows)` turned a table-format
        // change into a silent second create under a name the operator already has (D6).
        let listing = self.query(&["sbx", "ls", "--all"]).await?;
        if let Some(existing_id) = reuse_by_name(parse_ls_table(&listing)?, &request.name) {
            return Ok(existing_id);
        }

        let timeout = request
            .ttl_seconds
            .unwrap_or(DEFAULT_TIMEOUT_SECONDS)
            .clamp(1, MAX_TIMEOUT_SECONDS)
            .to_string();
        let output = self
            .run(
                &["sbx", "create", request.name.as_str(), "--timeout", timeout.as_str(), "--queue"],
                CREATE_TIMEOUT,
            )
            .await?;
        if !cli_run::succeeded(&output) {
            return Err(cli_run::failure(PROVIDER, EXE, &output));
        }
        parse_create_stdout(&output.stdout)
    }

    async fn get(&self, sandbox_id: &str) -> Result<SandboxInfo, SandboxError> {
        let stdout = self.query(&["sbx", "describe", sandbox_id]).await?;
        describe_to_info(&parse_describe(&stdout))
    }

    async fn stop(&self, sandbox_id: &str) -> Result<(), SandboxError> {
        self.mutate(&["sbx", "suspend", sandbox_id, "--no-wait"]).await
    }

    async fn resume(&self, sandbox_id: &str, _ttl_seconds: Option<u64>) -> Result<(), SandboxError> {
        // `tl sbx` has no timeout-update verb; the timeout comes from create and is refreshed by
        // proxy traffic. Resume only.
        self.mutate(&["sbx", "resume", sandbox_id, "--no-wait"]).await
    }

    async fn delete(&self, sandbox_id: &str) -> Result<(), SandboxError> {
        self.mutate(&["sbx", "terminate", sandbox_id]).await
    }

    async fn extend_ttl(&self, _sandbox_id: &str, _ttl_seconds: Option<u64>) -> Result<(), SandboxError> {
        Err(SandboxError::new(
            ErrorKind::Config,
            PROVIDER,
            "ttl_fixed_at_create",
            "tl sets the idle timeout at create; recreate the sandbox to change it",
        ))
    }

    async fn exec(
        &self,
        sandbox_id: &str,
        command: &str,
        timeout_seconds: u64,
    ) -> Result<ExecResult, SandboxError> {
        // `tl sbx exec` streams the remote process's stdout/stderr verbatim and exits with the
        // command's own status (verified live: `exit 7` -> 7; `exit 1` with empty output -> 1;
        // a 200 KB stdout arrived byte-intact). A *CLI* failure also exits 1, so the exit code
        // alone cannot tell the two apart - returning it unconditionally reported `tl`'s own
        // failure as the command's status (D8).
        //
        // The discriminator is `tl`'s own error shape: an invocation that never reached the
        // sandbox prints exactly one anyhow line on stderr, `Error: <verb> sandbox ...`, and
        // nothing on stdout. A remote command that fails is not constrained to that shape, and
        // one that *is* identical would have to print no stdout at all, so this misfires only on
        // a command that emits a single `tl`-shaped error line - acceptable for the alternative,
        // which was misreporting every CLI failure as a remote exit code.
        let budget = timeout_seconds.clamp(1, MAX_TIMEOUT_SECONDS);
        let timeout = budget.to_string();
        let output = self
            .run(
                &["sbx", "exec", sandbox_id, "--timeout", timeout.as_str(), "bash", "-lc", command],
                Duration::from_secs(budget + 30),
            )
            .await?;
        if let Some(error) = cli_failure_not_command(&output) {
            return Err(error);
        }
        Ok(ExecResult {
            exit_code: output.code,
            stdout: output.stdout,
            stderr: output.stderr,
            timed_out: false,
        })
    }

    async fn list(&self) -> Result<Vec<ListedSandbox>, SandboxError> {
        // `--all` includes terminated sandboxes: the orphan sweep must see a machine the
        // provider still bills for or still holds a disk for, not only the running ones.
        let stdout = self.query(&["sbx", "ls", "--all"]).await?;
        let rows = parse_ls_table(&stdout)?;
        let now_ms = i64::try_from(
            time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000,
        )
        .unwrap_or(0);
        listed_from_rows(rows, now_ms)
    }

    async fn ssh_target(&self, sandbox_id: &str) -> Result<SshTarget, SandboxError> {
        // Read the state first: `tl sbx ssh` resumes a suspended sandbox, so the caller must not
        // be handed an endpoint for a machine a status check has not confirmed is running.
        let info = self.get(sandbox_id).await?;
        if info.state != SandboxState::Ready {
            return Err(SandboxError::new(
                ErrorKind::Conflict,
                PROVIDER,
                "ssh_needs_running",
                format!("sandbox {sandbox_id} is {}; resume it before asking for an ssh target", info.state),
            ));
        }
        let stdout = self.query(&["sbx", "describe", sandbox_id]).await?;
        let describe = parse_describe(&stdout);
        let block = describe.ssh_config.ok_or_else(|| {
            SandboxError::new(
                ErrorKind::Config,
                PROVIDER,
                "ssh_config_missing",
                "tl sbx describe printed no SSH Config block; register a key with `tl sbx ssh keys add`",
            )
        })?;
        parse_ssh_config(&block)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LS_TABLE: &str = include_str!("../../../tests/fixtures/cloud_host/tensorlake/ls_table_stub.txt");
    const DESCRIBE: &str = include_str!("../../../tests/fixtures/cloud_host/tensorlake/describe_stub.txt");
    const CREATE_STDOUT: &str =
        include_str!("../../../tests/fixtures/cloud_host/tensorlake/create_stdout_stub.txt");
    const NOT_LOGGED_IN: &str =
        include_str!("../../../tests/fixtures/cloud_host/tensorlake/not_logged_in_live.txt");
    const LS_LIVE: &str =
        include_str!("../../../tests/fixtures/cloud_host/tensorlake/ls_table_live.txt");
    const DESCRIBE_LIVE: &str =
        include_str!("../../../tests/fixtures/cloud_host/tensorlake/describe_live.txt");

    #[cfg(unix)]
    fn make_executable(script: &std::path::Path) {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(script, std::fs::Permissions::from_mode(0o755))
            .expect("chmod the stub");
    }

    /// A stub `tl` that records every invocation's argv and answers per subcommand.
    ///
    /// The provider spawns this script instead of the `tl` on PATH, so a test can drive a real
    /// call site (`create`, `exec`) and read both what it returned and which CLI calls it made -
    /// the two things the in-process tests above cannot see.
    struct StubCli {
        dir: tempfile::TempDir,
        ls_body: String,
        create_stdout: String,
        create_code: i32,
        exec_stdout: String,
        exec_stderr: String,
        exec_code: i32,
    }

    impl StubCli {
        fn new() -> Self {
            Self {
                dir: tempfile::tempdir().expect("tempdir"),
                ls_body: LS_TABLE.to_string(),
                create_stdout: CREATE_STDOUT.to_string(),
                create_code: 0,
                exec_stdout: String::new(),
                exec_stderr: String::new(),
                exec_code: 0,
            }
        }

        fn listing(mut self, body: &str) -> Self {
            self.ls_body = body.to_string();
            self
        }

        fn exec_answering(mut self, stdout: &str, stderr: &str, code: i32) -> Self {
            self.exec_stdout = stdout.to_string();
            self.exec_stderr = stderr.to_string();
            self.exec_code = code;
            self
        }

        fn provider(&self) -> TensorlakeProvider {
            let dir = self.dir.path();
            let script = dir.join("tl");
            let calls = dir.join("calls");
            let ls = dir.join("ls.txt");
            let create_out = dir.join("create_stdout");
            let exec_out = dir.join("exec_stdout");
            let exec_err = dir.join("exec_stderr");

            std::fs::write(&ls, &self.ls_body).expect("stub ls body");
            std::fs::write(&create_out, &self.create_stdout).expect("stub create stdout");
            std::fs::write(&exec_out, &self.exec_stdout).expect("stub exec stdout");
            std::fs::write(&exec_err, &self.exec_stderr).expect("stub exec stderr");
            // One tab-separated line per invocation. `$1` is always `sbx`; `$2` is the verb.
            std::fs::write(
                &script,
                format!(
                    "#!/bin/sh\n\
                     printf '%s\\t' \"$@\" >> \"{calls}\"\n\
                     printf '\\n' >> \"{calls}\"\n\
                     case \"$2\" in\n\
                       ls) cat \"{ls}\"; exit 0 ;;\n\
                       create) cat \"{create_out}\"; exit {create_code} ;;\n\
                       describe) cat \"{describe}\"; exit 0 ;;\n\
                       exec) cat \"{exec_out}\"; cat \"{exec_err}\" >&2; exit {exec_code} ;;\n\
                     esac\n\
                     exit 0\n",
                    calls = calls.display(),
                    ls = ls.display(),
                    create_out = create_out.display(),
                    create_code = self.create_code,
                    describe = {
                        let path = dir.join("describe.txt");
                        std::fs::write(&path, DESCRIBE).expect("stub describe body");
                        path.display().to_string()
                    },
                    exec_out = exec_out.display(),
                    exec_err = exec_err.display(),
                    exec_code = self.exec_code,
                ),
            )
            .expect("stub script");
            make_executable(&script);
            TensorlakeProvider::with_cli(script)
        }

        fn calls(&self) -> Vec<String> {
            std::fs::read_to_string(self.dir.path().join("calls"))
                .unwrap_or_default()
                .lines()
                // The stub terminates every record with a tab; drop it so a split yields exactly
                // the arguments the process received.
                .map(|line| line.trim_end_matches('\t').to_string())
                .collect()
        }

        fn verbs(&self) -> Vec<String> {
            self.calls()
                .iter()
                .filter_map(|line| line.split('\t').nth(1).map(str::to_string))
                .collect()
        }
    }

    #[test]
    fn ls_table_parses_the_documented_columns() {
        let rows = parse_ls_table(LS_TABLE).expect("parse ls table");
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0],
            TensorlakeRow {
                sandbox_id: "s7jus08qec4axzgbpq76h".into(),
                name: "fx-probe".into(),
                status: "running".into(),
                created: "38 min ago".into(),
            }
        );
    }

    #[test]
    fn a_parse_failure_reaches_create_instead_of_being_swallowed() {
        // D6: the old `if let Ok(rows)` in `create` turned a table-format change into a silent
        // second create under a name the operator already had. `create` now builds the rows as a
        // value and cannot reach the create branch without them, so a broken listing is a typed
        // error all the way out. This asserts both halves: the parse errors, and `reuse_by_name`
        // never sees an unparsed listing.
        let broken = "Name\tID\tStatus\n\n";
        let error = parse_ls_table(broken).expect_err("a header without Created At must be refused");
        assert_eq!(error.kind, ErrorKind::Fatal);
        assert_eq!(error.provider_code, "ls_header_unexpected");

        // A good listing still reuses, so the guard itself is not simply disabled.
        let rows = parse_ls_table(LS_TABLE).expect("parse");
        assert_eq!(reuse_by_name(rows, "fx-probe").as_deref(), Some("s7jus08qec4axzgbpq76h"));
    }

    #[test]
    fn reuse_matches_the_name_exactly_and_creates_when_absent() {
        let rows = parse_ls_table(LS_TABLE).expect("parse");
        // A different name is a different sandbox: the guard must not reuse the wrong machine.
        assert_eq!(reuse_by_name(rows, "fx-probe-2"), None);
    }

    #[test]
    fn empty_listing_is_not_an_error() {
        assert!(parse_ls_table("No sandboxes found.\n").expect("empty").is_empty());
        assert!(parse_ls_table("").expect("blank").is_empty());
    }

    #[test]
    fn reordered_header_fails_loudly_instead_of_misreading_columns() {
        let reordered = "Name\tID\tStatus\tImage\tCPUs\tMemory\tDisk\tGPUs\tCreated At\n\
                         fx-probe\ts7jus08q\trunning\timg\t1\t1 MiB\t1 MiB\t-\tnow\n";
        let error = parse_ls_table(reordered).expect_err("a reordered header must be rejected");
        assert_eq!(error.kind, ErrorKind::Fatal);
        assert_eq!(error.provider_code, "ls_header_unexpected");
    }

    #[test]
    fn summary_line_is_ignored_and_multiple_rows_parse() {
        let table = "ID\tName\tStatus\tImage\tCPUs\tMemory\tDisk\tGPUs\tCreated At\n\
                     a1\tfx-a\trunning\timg\t1\t1 MiB\t1 MiB\t-\t1 min ago\n\
                     b2\tfx-b\tsuspended\timg\t1\t1 MiB\t1 MiB\t-\t2 min ago\n\
                     2 sandboxes\n";
        let rows = parse_ls_table(table).expect("parse two rows");
        assert_eq!(rows.len(), 2);
        assert_eq!(map_state(&rows[1].status), SandboxState::Stopped);
    }

    #[test]
    fn live_describe_yields_a_ready_machine_and_a_usable_target() {
        // The stub above is hand-written; this is the real `tl sbx describe` text, so the two
        // parsers are also proved against the format the binary actually prints.
        let describe = parse_describe(DESCRIBE_LIVE);
        let info = describe_to_info(&describe).expect("live info");
        assert_eq!(info.state, SandboxState::Ready);
        let target = parse_ssh_config(&describe.ssh_config.expect("live ssh block"))
            .expect("live target");
        assert_eq!(target.host, "8gfghorqbabhg5qx1bgj1.sandbox.tensorlake.ai");
        assert_eq!(target.user, "8gfghorqbabhg5qx1bgj1");
    }

    #[test]
    fn describe_parses_fields_and_the_ssh_block() {
        let describe = parse_describe(DESCRIBE);
        assert_eq!(describe.field("ID"), Some("s7jus08qec4axzgbpq76h"));
        assert_eq!(describe.field("Status"), Some("running"));
        assert_eq!(describe.field("Timeout"), Some("600s"));
        let block = describe.ssh_config.expect("ssh block");
        assert!(block.contains("HostName fx-probe.sandbox.tensorlake.ai"));
    }

    #[test]
    fn describe_maps_to_ready_and_carries_no_error() {
        let info = describe_to_info(&parse_describe(DESCRIBE)).expect("info");
        assert_eq!(info.state, SandboxState::Ready);
        assert!(info.error.is_none());
    }

    #[test]
    fn ssh_block_yields_the_sandbox_id_as_user() {
        let describe = parse_describe(DESCRIBE);
        let target = parse_ssh_config(&describe.ssh_config.expect("block")).expect("target");
        assert_eq!(target.host, "fx-probe.sandbox.tensorlake.ai");
        assert_eq!(target.port, 22);
        // Tensorlake routes on the sandbox id, not a POSIX user name.
        assert_eq!(target.user, "s7jus08qec4axzgbpq76h");
        assert_eq!(
            target.identity,
            SshIdentity::KeyFile(std::path::PathBuf::from("~/.ssh/id_ed25519_tensorlake"))
        );
    }

    #[test]
    fn create_stdout_yields_the_id() {
        assert_eq!(parse_create_stdout(CREATE_STDOUT).expect("id"), "s7jus08qec4axzgbpq76h");
        let prefixed = parse_create_stdout("sandbox_abc123\n").expect("prefixed id");
        assert_eq!(prefixed, "sandbox_abc123");
        let error = parse_create_stdout("provisioning...\n").expect_err("no id printed");
        assert_eq!(error.provider_code, "create_id_missing");
    }

    #[test]
    fn every_documented_status_maps() {
        assert_eq!(map_state("running"), SandboxState::Ready);
        assert_eq!(map_state("pending"), SandboxState::Provisioning);
        assert_eq!(map_state("suspending"), SandboxState::Stopped);
        assert_eq!(map_state("suspended"), SandboxState::Stopped);
        assert_eq!(map_state("terminated"), SandboxState::Deleted);
        assert_eq!(map_state("resume_failed"), SandboxState::Error);
    }

    #[test]
    fn login_prompt_is_a_config_error() {
        let output = CliOutput {
            stdout: NOT_LOGGED_IN.to_string(),
            stderr: String::new(),
            code: Some(101),
        };
        assert_eq!(cli_run::failure(PROVIDER, EXE, &output).kind, ErrorKind::Config);
    }

    #[test]
    fn relative_ages_convert_to_instants() {
        // Every unit the binary's own format table can emit (`<n> {s,m,h,d} ago`, found in the
        // `tl` 0.5.165 string data, plus the two numeric-free forms), and nothing that is not one
        // of them. Live cells observed: `9 sec ago`, `7 min ago`, `2 min ago`, `38 min ago`.
        const NOW: i64 = 1_000_000_000_000;
        let minute = 60_000i64;
        for (cell, expected) in [
            ("9 sec ago", NOW - 9_000),
            ("7 min ago", NOW - 7 * minute),
            ("38 min ago", NOW - 38 * minute),
            ("2 h ago", NOW - 120 * minute),
            ("1 d ago", NOW - 24 * 60 * minute),
            ("just now", NOW),
            ("in under a minute", NOW),
            // Spellings a later release plausibly emits; the unit table already accepts them.
            ("3 hours ago", NOW - 3 * 60 * minute),
            ("2 days ago", NOW - 2 * 24 * 60 * minute),
            (" 7 min ago ", NOW - 7 * minute),
        ] {
            assert_eq!(parse_relative_age(cell, NOW), Some(expected), "{cell:?}");
        }
        // `in 5 min` is a boot in progress, not an age: ageing it would place the create in the
        // future and hide a real orphan.
        for not_an_age in [
            "", "-", "running", "5", "ago", "5 min", "in 5 min", "5 min ago extra",
            "min ago", "5 fortnights ago",
        ] {
            assert_eq!(parse_relative_age(not_an_age, NOW), None, "{not_an_age:?}");
        }
    }

    #[test]
    fn unparseable_age_fails_list_instead_of_disabling_the_reaper() {
        // D7: lifecycle's orphan gate is `created_at.is_some_and(...)`, so a `None` here means an
        // orphan is reported and never deleted. An age cell the parser cannot read must be a
        // typed error that stops the sweep, not a fallback to `None`.
        let rows = vec![TensorlakeRow {
            sandbox_id: "8gfg".into(),
            name: "fx-t9-probe".into(),
            status: "running".into(),
            created: "whenever".into(),
        }];
        let error = listed_from_rows(rows, 1_000_000_000_000).expect_err("unknown age must fail");
        assert_eq!(error.kind, ErrorKind::Fatal);
        assert_eq!(error.provider_code, "ls_age_unexpected");
        assert!(error.message.contains("whenever"), "{}", error.message);
    }

    #[test]
    fn live_ls_rows_list_with_a_real_instant_so_orphans_can_be_reaped() {
        // The end-to-end shape of D7: the live table's age becomes `Some(instant)`, which is
        // exactly what lifecycle needs to delete an orphan past the hard ceiling.
        const NOW: i64 = 1_000_000_000_000;
        let listed = listed_from_rows(parse_ls_table(LS_LIVE).expect("live ls"), NOW)
            .expect("live rows list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].sandbox_id, "8gfghorqbabhg5qx1bgj1");
        assert_eq!(listed[0].name.as_deref(), Some("fx-t9-probe"));
        // The capture was taken while that sandbox was suspended; the assertion that matters for
        // D7 is the instant below, not the lifecycle state.
        assert_eq!(listed[0].state, SandboxState::Stopped);
        assert_eq!(listed[0].created_at, Some(NOW - 7 * 60_000));
        assert!(listed[0].created_at.is_some_and(|c| NOW - c > 0));
    }

    #[test]
    fn cli_failures_are_provider_errors_not_exit_codes() {
        // D8. These four strings are verbatim from this machine's `tl` 0.5.165.
        let cli_failures = [
            "Error: failed to resolve sandbox fx-nonexistent-t9 (HTTP 404 Not Found): Sandbox not found",
            "Error: sandbox 'fx-nonexistent-t9' not found",
            "Error: failed to terminate sandbox fx-nonexistent-t9 (HTTP 404 Not Found): Sandbox not found",
        ];
        for stderr in cli_failures {
            let output = CliOutput { stdout: String::new(), stderr: stderr.into(), code: Some(1) };
            let error = cli_failure_not_command(&output)
                .unwrap_or_else(|| panic!("tl-level failure must not be a command exit: {stderr}"));
            assert_eq!(error.kind, ErrorKind::NotFound, "{stderr}");
        }

        // A remote command's own failure is still the command's exit code, even though the CLI
        // exits non-zero for it too. `exit 7` and `command not found` are live captures.
        for (stdout, stderr, code) in [
            ("", "", 7),
            ("", "bash: line 1: nonexistent-cmd-xyz: command not found", 127),
            ("OUT\n", "ERR\n", 7),
            ("", "Error: failed to resolve sandbox decoy", 3),
        ] {
            let output = CliOutput { stdout: stdout.into(), stderr: stderr.into(), code: Some(code) };
            assert!(
                cli_failure_not_command(&output).is_none(),
                "a remote exit must stay a remote exit: stdout={stdout:?} stderr={stderr:?}"
            );
        }

        // Success never errors, whatever the streams held.
        assert!(cli_failure_not_command(&CliOutput {
            stdout: "ok".into(),
            stderr: String::new(),
            code: Some(0)
        })
        .is_none());
    }

    #[tokio::test]
    async fn create_refuses_env_before_spawning_anything() {
        let stub = StubCli::new();
        let provider = stub.provider();
        let mut env = std::collections::BTreeMap::new();
        env.insert("FOO".to_string(), "bar".to_string());
        let error = provider
            .create(CreateSandbox {
                idempotency_key: "fx-tl-1".into(),
                name: "fx-tl-1".into(),
                ttl_seconds: None,
                env,
            })
            .await
            .expect_err("env cannot be honoured by tl create");
        assert_eq!(error.kind, ErrorKind::Config);
        assert_eq!(error.provider_code, "env_unsupported");
        assert!(stub.calls().is_empty(), "create must refuse before spawning: {:?}", stub.calls());
    }

    /// D6, at the CALL SITE: `create()` against a CLI whose `sbx ls` table changed shape returns
    /// the parse error and never reaches `sbx create`.
    ///
    /// This is the whole point of the defect: the old `if let Ok(rows)` swallowed the parse error
    /// and fell through to create, so a table-format change produced a SECOND billable sandbox
    /// under a name the operator already had. The in-process test above can only show that
    /// `parse_ls_table` errors; this one shows that `create` stops - and the stub's call log is
    /// what proves no `create` verb was ever spawned.
    #[tokio::test]
    async fn create_propagates_a_broken_listing_instead_of_creating_twice() {
        // A header `tl` might grow: same columns, wrong order. Parsing must refuse rather than
        // read `Name` as `ID`.
        let broken = "Name\tID\tStatus\tImage\tCPUs\tMemory\tDisk\tGPUs\tCreated At\n\
                      fx-tl-1\ts7jus08q\trunning\timg\t1\t1 MiB\t1 MiB\t-\t1 min ago\n\
                      1 sandbox\n";
        let stub = StubCli::new().listing(broken);
        let provider = stub.provider();

        let error = provider
            .create(CreateSandbox {
                idempotency_key: "fx-tl-1".into(),
                name: "fx-tl-1".into(),
                ttl_seconds: None,
                env: std::collections::BTreeMap::new(),
            })
            .await
            .expect_err("a broken listing must not fall through to create");
        assert_eq!(error.kind, ErrorKind::Fatal);
        assert_eq!(error.provider_code, "ls_header_unexpected");

        let verbs = stub.verbs();
        assert_eq!(verbs, vec!["ls"], "create must not run after an unparsed listing: {verbs:?}");
    }

    /// The other half of D6: a good listing that already holds the name reuses it, and a good
    /// listing that does not still creates. Without both, the previous test would also pass on a
    /// `create` that simply never worked.
    #[tokio::test]
    async fn create_reuses_an_existing_name_and_creates_an_absent_one() {
        let stub = StubCli::new();
        let provider = stub.provider();
        let id = provider
            .create(CreateSandbox {
                idempotency_key: "fx-probe".into(),
                name: "fx-probe".into(),
                ttl_seconds: None,
                env: std::collections::BTreeMap::new(),
            })
            .await
            .expect("reuse the listed sandbox");
        assert_eq!(id, "s7jus08qec4axzgbpq76h");
        assert_eq!(stub.verbs(), vec!["ls"], "an existing name must not be created again");

        let fresh = StubCli::new();
        let id = fresh
            .provider()
            .create(CreateSandbox {
                idempotency_key: "fx-new".into(),
                name: "fx-new".into(),
                ttl_seconds: None,
                env: std::collections::BTreeMap::new(),
            })
            .await
            .expect("create an absent name");
        assert_eq!(id, "s7jus08qec4axzgbpq76h");
        assert_eq!(fresh.verbs(), vec!["ls", "create"]);
        // The documented non-blocking form, so a metal host cannot hold the call for 20 minutes.
        let create_call = &fresh.calls()[1];
        assert!(create_call.contains("--queue"), "create must not block on the boot: {create_call}");
        assert!(create_call.contains("--timeout\t600"), "default TTL: {create_call}");
    }

    /// D8, at the CALL SITE: `exec()` returns the CLI's own failure as a provider error, and a
    /// remote command's non-zero exit as the command's exit code.
    ///
    /// The in-process test covers `cli_failure_not_command` directly; this drives `exec()` so a
    /// future edit that stops calling the discriminator (returning `output.code` unconditionally,
    /// which is exactly what D8 was) fails here.
    #[tokio::test]
    async fn exec_reports_a_cli_failure_as_a_provider_error_not_an_exit_code() {
        let stub = StubCli::new().exec_answering(
            "",
            "Error: failed to resolve sandbox fx-nonexistent-t9 (HTTP 404 Not Found): Sandbox not found\n",
            1,
        );
        let error = stub
            .provider()
            .exec("fx-nonexistent-t9", "echo hi", 30)
            .await
            .expect_err("a tl-level failure is not the command's exit status");
        assert_eq!(error.kind, ErrorKind::NotFound);
        assert_eq!(error.provider, PROVIDER, "the error is tl's, not the remote command's");
    }

    #[tokio::test]
    async fn exec_returns_a_remote_exit_code_and_both_streams() {
        let stub = StubCli::new().exec_answering("partial out\n", "boom\n", 7);
        let result = stub
            .provider()
            .exec("s7jus08qec4axzgbpq76h", "exit 7", 30)
            .await
            .expect("a remote failure is still a result");
        assert_eq!(result.exit_code, Some(7));
        assert_eq!(result.stdout, "partial out\n");
        assert_eq!(result.stderr, "boom\n");
        assert!(!result.timed_out);
    }

    /// The timeout the CLI is given is the caller's budget, clamped to Tensorlake's two-hour cap,
    /// and it is a bare integer of seconds (not a duration string).
    #[tokio::test]
    async fn exec_passes_the_remaining_budget_as_an_integer_seconds_flag() {
        let stub = StubCli::new();
        stub.provider().exec("s7jus08qec4axzgbpq76h", "true", 45).await.expect("exec");
        let calls = stub.calls();
        let exec = calls.iter().find(|line| line.starts_with("sbx\texec\t")).expect("exec call");
        let parts: Vec<&str> = exec.split('\t').collect();
        assert_eq!(
            parts,
            vec!["sbx", "exec", "s7jus08qec4axzgbpq76h", "--timeout", "45", "bash", "-lc", "true"],
            "argv must not be shell-joined and the timeout must be integer seconds: {exec}"
        );
    }
}
