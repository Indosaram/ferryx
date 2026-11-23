//! Running an official provider CLI: one argv array, no shell, no inherited stdin.
//!
//! Every CLI-backed adapter in this module family drives the vendor's own binary this way.
//! Three rules keep that safe to do from the app:
//!
//! 1. **argv only.** No adapter builds a shell string; the executable and each argument are
//!    separate `Command` arguments, so a machine name or a command containing `; rm -rf ~` can
//!    never be re-interpreted by a local shell.
//! 2. **No stdin.** Every child gets [`Stdio::null`] on stdin. A CLI that wants to prompt (a
//!    device login, a confirmation) gets EOF and exits instead of hanging a background task or
//!    populating a browser tab.
//! 3. **Bounded.** A CLI that outlives its budget is killed, so a wedged network call cannot
//!    hold a lifecycle lock forever.
//!
//! Credentials never travel through this module: the CLIs read their own stored login or the
//! `INSTABOX_API_KEY`/`GITHUB_TOKEN` already in the process environment. Nothing here logs an
//! argument list or an environment.

use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;

use super::{ErrorKind, SandboxError};

/// Raw result of one CLI invocation. A non-zero exit is *not* an error here: each adapter maps
/// its own tool's exit codes and message wording onto [`ErrorKind`].
pub(crate) struct CliOutput {
    pub stdout: String,
    pub stderr: String,
    pub code: Option<i32>,
}

impl CliOutput {
    /// stdout and stderr together, lowercased, for the wording checks below. Some CLIs report
    /// failures on stdout and some on stderr (`instabox` writes its quota error to stderr,
    /// `tl` writes the login prompt to stderr), so error classification must read both.
    fn combined_lower(&self) -> String {
        format!("{}\n{}", self.stdout, self.stderr).to_lowercase()
    }
}

/// Run `exe` with `args`. Returns the captured output whatever the exit status; errors only
/// when the binary is missing, cannot be spawned, or exceeds `timeout`.
pub(crate) async fn run(
    provider: &'static str,
    exe: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<CliOutput, SandboxError> {
    let mut command = Command::new(exe);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let child = command.output();
    let output = match tokio::time::timeout(timeout, child).await {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => return Err(spawn_failure(provider, exe, &error)),
        Err(_) => {
            // `output()` owns the child; dropping the future drops (and, with `kill_on_drop`,
            // kills) it. The caller retries under backoff rather than assuming the CLI's work
            // did not happen.
            return Err(SandboxError::new(
                ErrorKind::Retryable,
                provider,
                "cli_timeout",
                format!("{exe} did not finish within {}s", timeout.as_secs()),
            ));
        }
    };

    Ok(CliOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        code: output.status.code(),
    })
}

/// A missing binary is a `Config` error that names the fix; anything else is a spawn problem
/// worth retrying.
fn spawn_failure(provider: &'static str, exe: &str, error: &std::io::Error) -> SandboxError {
    if error.kind() == std::io::ErrorKind::NotFound {
        return SandboxError::new(
            ErrorKind::Config,
            provider,
            "cli_missing",
            format!("`{exe}` is not on PATH; install it and sign it in, then retry"),
        );
    }
    SandboxError::new(ErrorKind::Retryable, provider, "cli_spawn_failed", error.to_string())
}

/// Wording shared by all three CLIs for the four failure families the trait distinguishes.
/// Returns `None` when nothing matches, and the adapter then decides (`Fatal` by default).
pub(crate) fn classify(output: &CliOutput) -> Option<ErrorKind> {
    let text = output.combined_lower();

    // Capacity and state fights: an account limit, a name already taken, a slot in use. These
    // are the ones the plan requires to surface as `Conflict` (InstaBox at 1/1).
    const CONFLICT: &[&str] = &[
        "quota_exceeded",
        "sandbox limit reached",
        "machine limit reached",
        "already exists",
        "already in use",
        "conflict",
        "409",
    ];
    if CONFLICT.iter().any(|needle| text.contains(needle)) {
        return Some(ErrorKind::Conflict);
    }

    // No usable credential. Never retried: the operator must sign a CLI in, or replace the key.
    // A *revoked* or invalid key belongs here, not in `Fatal`: `instabox` answers
    // `error revoked_key: API key ... was revoked; create an API key at ...`, which is the CLI
    // spelling of the REST rule "401/403 -> Config". Bare "api key" would be far too broad (the
    // CLI's own help text mentions it), so the needles stay anchored on the failure wording.
    const CONFIG: &[&str] = &[
        "not logged in",
        "not authenticated",
        "unauthenticated",
        "unauthorized",
        "session expired",
        "run `instabox login`",
        "run `tl login`",
        "gh auth login",
        "authentication required",
        "http 401",
        "http 403",
        "permission denied",
        "revoked",
        "invalid api key",
        "invalid_api_key",
        "api key expired",
    ];
    if CONFIG.iter().any(|needle| text.contains(needle)) {
        return Some(ErrorKind::Config);
    }

    const NOT_FOUND: &[&str] = &[
        "not found",
        "does not exist",
        "no such",
        "no sandbox with",
        "unknown sandbox",
        "unknown codespace",
        "http 404",
    ];
    if NOT_FOUND.iter().any(|needle| text.contains(needle)) {
        return Some(ErrorKind::NotFound);
    }

    const RETRYABLE: &[&str] = &[
        "timeout",
        "timed out",
        "temporarily unavailable",
        "try again",
        "rate limit",
        "too many requests",
        "connection reset",
        "http 429",
        "http 500",
        "http 502",
        "http 503",
        "http 504",
    ];
    if RETRYABLE.iter().any(|needle| text.contains(needle)) {
        return Some(ErrorKind::Retryable);
    }

    None
}

/// Turn a failed invocation into a [`SandboxError`] carrying the CLI's own first line, so an
/// operator can match it against the tool's docs.
pub(crate) fn failure(provider: &'static str, exe: &str, output: &CliOutput) -> SandboxError {
    let kind = classify(output).unwrap_or(ErrorKind::Fatal);
    let detail = first_meaningful_line(&output.stderr)
        .or_else(|| first_meaningful_line(&output.stdout))
        .unwrap_or_else(|| "no output".to_string());
    SandboxError::new(
        kind,
        provider,
        format!("{exe}_exit_{}", output.code.unwrap_or(-1)),
        detail,
    )
}

fn first_meaningful_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_string)
}

/// True when the invocation succeeded (exit 0). A CLI killed by a signal has no code; treat
/// that as failure so the caller reports something instead of assuming success.
pub(crate) fn succeeded(output: &CliOutput) -> bool {
    output.code == Some(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(stdout: &str, stderr: &str, code: i32) -> CliOutput {
        CliOutput {
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
            code: Some(code),
        }
    }

    #[test]
    fn quota_error_on_stderr_is_a_conflict() {
        let output = out(
            "",
            "instabox: error quota_exceeded: sandbox limit reached (1 of 1) in org Indosaram",
            1,
        );
        assert_eq!(classify(&output), Some(ErrorKind::Conflict));
        assert_eq!(failure("instabox", "instabox", &output).kind, ErrorKind::Conflict);
    }

    #[test]
    fn expired_session_on_stdout_is_config() {
        let output = out("instabox: session expired (unauthenticated); run `instabox login`", "", 1);
        assert_eq!(classify(&output), Some(ErrorKind::Config));
    }

    #[test]
    fn tensorlake_login_prompt_is_config_not_retryable() {
        let output = out(
            "we're going to open a web browser for you to enter a one-time code.",
            "It seems like you're not logged in. Let's log you in...",
            101,
        );
        assert_eq!(classify(&output), Some(ErrorKind::Config));
    }

    #[test]
    fn gh_404_text_is_not_found() {
        let output = out("", "gh: Not Found (HTTP 404)", 1);
        assert_eq!(classify(&output), Some(ErrorKind::NotFound));
    }

    #[test]
    fn transient_failures_are_retryable_and_unknown_ones_are_fatal() {
        assert_eq!(classify(&out("", "service temporarily unavailable", 1)), Some(ErrorKind::Retryable));
        assert_eq!(classify(&out("", "something odd happened", 3)), None);
        assert_eq!(failure("codespaces", "gh", &out("", "something odd happened", 3)).kind, ErrorKind::Fatal);
    }

    #[test]
    fn revoked_api_key_is_config_not_fatal() {
        // Live text from `instabox ls --json` with a key that had been revoked. The key id and
        // request id are redacted in the fixture; the wording is verbatim.
        const REVOKED: &str =
            include_str!("../../../tests/fixtures/cloud_host/instabox/revoked_key_live.txt");
        let output = out("", REVOKED, 1);
        assert_eq!(classify(&output), Some(ErrorKind::Config));
        let error = failure("instabox", "instabox", &output);
        assert_eq!(error.kind, ErrorKind::Config);
        assert_eq!(error.provider_code, "instabox_exit_1");
    }

    #[test]
    fn github_401_is_config() {
        // Live text from `gh codespace list` on a host whose token had expired.
        let output = out(
            "",
            "error getting codespaces: HTTP 401: Requires authentication \
             (https://api.github.com/user/codespaces?per_page=30)\nTry authenticating with: \
             gh auth login",
            1,
        );
        assert_eq!(classify(&output), Some(ErrorKind::Config));
    }

    #[test]
    fn instabox_no_sandbox_message_is_not_found() {
        let output = out("", "instabox: no sandbox with ID or name \"fx-nonexistent-t9\"", 1);
        assert_eq!(classify(&output), Some(ErrorKind::NotFound));
    }

    #[test]
    fn failure_detail_is_the_first_non_comment_line() {
        let output = out("", "# note\n  real problem here  \nmore", 2);
        let error = failure("tensorlake", "tl", &output);
        assert_eq!(error.message, "real problem here");
        assert_eq!(error.provider_code, "tl_exit_2");
    }
}
