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
    // are the ones the plan requires to surface as `Conflict` (InstaBox at 1/1). GitHub's wording
    // for the same family - `too many codespaces`, `codespace limit reached` - is listed with it
    // rather than in a second table, so one needle scan answers "is the account at capacity?".
    const CONFLICT: &[&str] = &[
        "quota_exceeded",
        "sandbox limit reached",
        "machine limit reached",
        "already exists",
        "already in use",
        "conflict",
        "409",
        "too many codespaces",
        "codespace limit reached",
        "codespaces limit reached",
        "codespaces usage limit",
        "quota exceeded",
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
        // A billing refusal is an account setting the operator must change, not a capacity fight:
        // GitHub answers a codespace create over the account's limit with `HTTP 402: You have
        // exceeded your spending limit`, or says the owner does not have Codespaces enabled. The
        // REST adapters read 402 the same way (`prized`, `opencomputer`).
        //
        // NOT a needle: `codespaces usage for this repository is paid for by <owner>`, the notice
        // `gh codespace create` prints before *every* outcome - classifying on it would relabel a
        // 404 or a 401 as a quota problem.
        "spending limit",
        "payment required",
        "http 402",
        // GitHub's refusal to host a codespace for this repository/account at all. Kept as the
        // bare phrase because the surrounding sentence varies (`Codespaces is not enabled for this
        // organization`, `Repository X is not enabled for Codespaces`); in a CLI failure it is
        // always the same account setting, never an unrelated word.
        "not enabled for",
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
/// operator can match it against the tool's docs. Use [`failure_full`] when the CLI prints a
/// notice before its error.
pub(crate) fn failure(provider: &'static str, exe: &str, output: &CliOutput) -> SandboxError {
    let detail = first_meaningful_line(&output.stderr)
        .or_else(|| first_meaningful_line(&output.stdout))
        .unwrap_or_else(|| "no output".to_string());
    build(provider, exe, output, detail)
}

/// The most of a CLI's own complaint worth carrying: enough for a multi-line `gh` error, small
/// enough that one failure does not bloat a catalog row or an evidence file.
const MAX_DETAIL_BYTES: usize = 2048;

/// Like [`failure`], but keeps **every** meaningful line instead of only the first.
///
/// `gh codespace create` is why this exists. Before the real error it prints an assurance line:
///
/// ```text
/// Codespaces usage for this repository is paid for by Indosaram
/// HTTP 422: ...
/// ```
///
/// (the first line carries a leading check mark).
///
/// [`failure`] reported exactly the reassurance line and dropped the cause underneath it, which is
/// how a live smoke ended with `[fatal] codespaces: Codespaces usage for this repository is paid
/// for by Indosaram (gh_exit_1)` and no reason. stderr is preferred as a whole stream and stdout is
/// only a fallback: mixing the two interleaves a progress line into the error.
pub(crate) fn failure_full(provider: &'static str, exe: &str, output: &CliOutput) -> SandboxError {
    let detail = meaningful_lines(&output.stderr)
        .or_else(|| meaningful_lines(&output.stdout))
        .unwrap_or_else(|| "no output".to_string());
    build(provider, exe, output, detail)
}

fn build(provider: &'static str, exe: &str, output: &CliOutput, detail: String) -> SandboxError {
    let kind = classify(output).unwrap_or(ErrorKind::Fatal);
    SandboxError::new(
        kind,
        provider,
        format!("{exe}_exit_{}", output.code.unwrap_or(-1)),
        detail,
    )
}

fn is_meaningful(line: &str) -> bool {
    !line.is_empty() && !line.starts_with('#')
}

fn first_meaningful_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|line| is_meaningful(line))
        .map(str::to_string)
}

/// Every meaningful line, joined, capped at [`MAX_DETAIL_BYTES`]. Whole lines are kept so a
/// truncated message does not read like corruption; the one exception is a single line that is
/// itself over the cap, which is cut on a char boundary because the text is UTF-8 and the glyphs
/// `gh` prints (`✓`, `·`) are multi-byte.
fn meaningful_lines(text: &str) -> Option<String> {
    let mut kept = String::new();
    for line in text.lines().map(str::trim).filter(|line| is_meaningful(line)) {
        let separator = if kept.is_empty() { 0 } else { 1 };
        if kept.len() + separator + line.len() > MAX_DETAIL_BYTES {
            // A single line over the cap is the one case with nothing whole to keep.
            if kept.is_empty() {
                kept.push_str(head(line, MAX_DETAIL_BYTES));
            }
            break;
        }
        if separator == 1 {
            kept.push('\n');
        }
        kept.push_str(line);
    }
    if kept.is_empty() {
        None
    } else {
        Some(kept)
    }
}

fn head(line: &str, max_bytes: usize) -> &str {
    if line.len() <= max_bytes {
        return line;
    }
    let mut cut = max_bytes;
    while !line.is_char_boundary(cut) {
        cut -= 1;
    }
    &line[..cut]
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

    /// The `gh codespace create` shape that hid a real failure in the task 14 live smoke: an
    /// assurance line first, the reason second. [`failure`] keeps only the assurance; the full
    /// form must keep both, in order, or the operator is told nothing again.
    #[test]
    fn failure_full_keeps_the_billing_notice_and_the_error_below_it() {
        let stderr = "Codespaces usage for this repository is paid for by Indosaram\n\
                      HTTP 422: Validation Failed (https://api.github.com/user/codespaces)\n";
        let output = out("", stderr, 1);

        // The old behaviour, stated so the regression cannot return silently.
        assert_eq!(
            failure("codespaces", "gh", &output).message,
            "Codespaces usage for this repository is paid for by Indosaram"
        );

        let error = failure_full("codespaces", "gh", &output);
        assert_eq!(
            error.message,
            "Codespaces usage for this repository is paid for by Indosaram\n\
             HTTP 422: Validation Failed (https://api.github.com/user/codespaces)"
        );
        assert_eq!(error.provider_code, "gh_exit_1");
        // No quota/limit wording in this text, so the kind stays the adapter's default.
        assert_eq!(error.kind, ErrorKind::Fatal);
    }

    #[test]
    fn failure_full_single_line_and_stdout_only_still_work() {
        let one_line = out("", "  real problem here  \n\n", 2);
        let error = failure_full("tensorlake", "tl", &one_line);
        assert_eq!(error.message, "real problem here");
        assert_eq!(error.provider_code, "tl_exit_2");

        // stderr empty: the stdout stream is the whole story, and comments/blank lines drop.
        let stdout_only = out("# banner\ninstabox: session expired (unauthenticated)\n", "", 1);
        let error = failure_full("instabox", "instabox", &stdout_only);
        assert_eq!(error.message, "instabox: session expired (unauthenticated)");
        assert_eq!(error.kind, ErrorKind::Config);

        // No output at all is still a reportable message, not an empty one.
        assert_eq!(failure_full("codespaces", "gh", &out("", "", 1)).message, "no output");
    }

    /// stderr wins over stdout as a stream: `gh` writes progress to stderr and data to stdout, and
    /// interleaving the two would put a printed name in the middle of an error message.
    #[test]
    fn failure_full_prefers_stderr_and_never_mixes_streams() {
        let output = out("some-name-printed-here\n", "the real error\nsecond line\n", 1);
        let error = failure_full("codespaces", "gh", &output);
        assert_eq!(error.message, "the real error\nsecond line");
    }

    #[test]
    fn failure_full_detail_is_capped_on_a_char_boundary() {
        // A line longer than the cap must be cut, not panicked on: `✓`, `·` and the box-drawing
        // glyphs `gh` prints are multi-byte.
        let long = "\u{2713} ".repeat(MAX_DETAIL_BYTES);
        let error = failure_full("codespaces", "gh", &out("", &long, 1));
        assert!(error.message.len() <= MAX_DETAIL_BYTES, "{} bytes", error.message.len());
        assert!(error.message.starts_with('\u{2713}'), "head kept: {:?}", error.message);

        // Many short lines are capped at a whole-line boundary, so no message ends mid-word.
        let many: String = (0..500).map(|n| format!("line {n}\n")).collect();
        let error = failure_full("codespaces", "gh", &out("", &many, 1));
        assert!(error.message.len() <= MAX_DETAIL_BYTES, "{} bytes", error.message.len());
        assert!(error.message.starts_with("line 0\nline 1\n"), "{:?}", error.message);
        assert!(!error.message.contains("line 499"), "the tail must be cut: {:?}", error.message);
        assert!(
            error.message
                .lines()
                .all(|line| line.strip_prefix("line ").is_some_and(|n| n.parse::<u32>().is_ok())),
            "every kept line is whole: {:?}",
            error.message
        );
    }

    /// The billing and quota wording `gh codespace create` can print. A spending limit or a
    /// disabled Codespaces is a setting the operator changes (`Config`); an account already at its
    /// codespace maximum is at capacity (`Conflict`), matching InstaBox's 1-slot limit.
    #[test]
    fn gh_codespace_billing_and_limit_wording_classifies() {
        for (stderr, expected) in [
            (
                "error creating codespace: HTTP 402: You have exceeded your spending limit",
                ErrorKind::Config,
            ),
            ("error creating codespace: payment required", ErrorKind::Config),
            ("GitHub Codespaces is not enabled for this repository", ErrorKind::Config),
            ("not enabled for this account", ErrorKind::Config),
            ("error creating codespace: too many codespaces", ErrorKind::Conflict),
            ("codespace limit reached (2 of 2)", ErrorKind::Conflict),
            ("codespaces usage limit reached for this billing period", ErrorKind::Conflict),
            ("quota exceeded", ErrorKind::Conflict),
        ] {
            assert_eq!(classify(&out("", stderr, 1)), Some(expected), "{stderr}");
            let error = failure_full("codespaces", "gh", &out("", stderr, 1));
            assert_eq!(error.kind, expected, "{stderr}");
        }

        // The assurance line alone carries no failure family: `gh` prints it before a 404 too, so
        // treating it as quota wording would relabel every codespace error.
        let notice = "Codespaces usage for this repository is paid for by Indosaram";
        assert_eq!(classify(&out("", notice, 1)), None);
        assert_eq!(failure_full("codespaces", "gh", &out("", notice, 1)).kind, ErrorKind::Fatal);
    }
}
