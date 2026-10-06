//! Optional GitHub issue intake for the Add Worktree dialog.
//!
//! Only local Git-backed projects are supported: the issue reference is resolved
//! against the selected project's own GitHub origin, then read with an already
//! authenticated `gh` executable. The executable is invoked as a fixed argument
//! array (never through a shell) on a blocking worker, with a bounded response
//! and a wall-clock deadline.
//!
//! Issue text reaches the UI as inert preview data. Ferryx never executes it,
//! never submits it as a prompt, and never stores credentials of its own: `gh`
//! keeps owning the authentication it already has.

use crate::ipc::error::{IpcError, IpcErrorCode};
use crate::ipc::run_blocking;
use crate::worktree::git::git_remote_origin_url;
use crate::worktree::WorkspaceRegistry;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tauri::State;
use url::Url;

/// Wall-clock ceiling for one `gh` invocation.
pub const GH_TIMEOUT: Duration = Duration::from_secs(20);
/// A larger response is rejected, never silently cut.
pub const GH_MAX_STDOUT_BYTES: usize = 512 * 1024;
pub const GH_MAX_STDERR_BYTES: usize = 64 * 1024;
/// Ceiling on the issue body handed to the UI as preview text.
pub const ISSUE_BODY_MAX_BYTES: usize = 16 * 1024;

const ISSUE_REF_MAX_CHARS: usize = 512;
const ISSUE_TITLE_MAX_CHARS: usize = 300;
const SLUG_MAX_CHARS: usize = 48;
const SLUG_TITLE_WORDS: usize = 6;
const SLUG_WORD_MAX_CHARS: usize = 8;
const GH_POLL_INTERVAL: Duration = Duration::from_millis(10);
const GH_DRAIN_GRACE: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitHubIssuePreviewRequest {
    pub workspace_id: String,
    /// An issue number (`12`, `#12`) or a github.com issue URL.
    pub issue_ref: String,
}

/// Inert preview of one GitHub issue, plus the slug Ferryx proposes for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubIssuePreview {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub body: String,
    pub body_truncated: bool,
    /// Canonical `owner/repository` the issue was read from.
    pub repository: String,
    /// Editable starting point for the worktree slug; never applied on its own.
    pub suggested_slug: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct IssueTarget {
    repository: RepositorySlug,
    number: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RepositorySlug {
    owner: String,
    name: String,
}

impl RepositorySlug {
    fn full_name(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }

    /// GitHub treats owner and repository names case-insensitively.
    fn matches(&self, owner: &str, name: &str) -> bool {
        self.owner.eq_ignore_ascii_case(owner) && self.name.eq_ignore_ascii_case(name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum IssueReference {
    Number(u64),
    Url {
        owner: String,
        repository: String,
        number: u64,
    },
}

#[tauri::command]
pub async fn cmd_github_issue_preview(
    registry: State<'_, WorkspaceRegistry>,
    request: GitHubIssuePreviewRequest,
) -> Result<GitHubIssuePreview, IpcError> {
    let registry = (*registry).clone();
    run_blocking(move || {
        let manager = registry
            .manager(&request.workspace_id)
            .map_err(IpcError::from)?;
        if !manager.is_git_backed() {
            return Err(IpcError::new(
                IpcErrorCode::NotAGitRepository,
                "Reading a GitHub issue needs a local Git-backed project.",
            ));
        }
        // Resolve and validate the request before touching the filesystem or
        // spawning anything: an unusable reference never reaches `gh`.
        let target = plan_issue(manager.repo_root(), &request.issue_ref)?;
        let program = resolve_gh_program()?;
        fetch_issue(&program, &target)
    })
    .await
}

/// Resolves `issue_ref` against the project's GitHub origin.
fn plan_issue(repo_root: &Path, issue_ref: &str) -> Result<IssueTarget, IpcError> {
    let reference = parse_issue_reference(issue_ref)?;
    let remote = git_remote_origin_url(repo_root).ok_or_else(|| {
        IpcError::new(
            IpcErrorCode::Unsupported,
            "This project has no Git remote to read a GitHub issue from.",
        )
    })?;
    let repository = parse_github_origin(&remote)?;
    match reference {
        IssueReference::Number(number) => Ok(IssueTarget {
            repository,
            number,
        }),
        IssueReference::Url {
            owner,
            repository: name,
            number,
        } => {
            if !repository.matches(&owner, &name) {
                return Err(IpcError::new(
                    IpcErrorCode::GitHubIssueRepositoryMismatch,
                    format!(
                        "That issue belongs to {owner}/{name}, but this project's GitHub origin is {}.",
                        repository.full_name()
                    ),
                )
                .with_details(serde_json::json!({
                    "expected": repository.full_name(),
                    "found": format!("{owner}/{name}"),
                })));
            }
            Ok(IssueTarget {
                repository,
                number,
            })
        }
    }
}

/// The exact argument array handed to `gh`. Every element is either a constant
/// or a value that passed the charset/number validation above, so no caller
/// text can turn into a flag or a second command.
fn gh_issue_view_args(target: &IssueTarget) -> Vec<String> {
    vec![
        "issue".to_string(),
        "view".to_string(),
        target.number.to_string(),
        "--repo".to_string(),
        target.repository.full_name(),
        "--json".to_string(),
        "number,title,url,body".to_string(),
    ]
}

fn fetch_issue(program: &Path, target: &IssueTarget) -> Result<GitHubIssuePreview, IpcError> {
    let stdout = run_gh(
        program,
        &gh_issue_view_args(target),
        GH_TIMEOUT,
        GH_MAX_STDOUT_BYTES,
    )?;
    let issue: GhIssueResponse = serde_json::from_str(&stdout).map_err(|_| {
        IpcError::new(
            IpcErrorCode::ParseError,
            "The GitHub CLI returned a response Ferryx could not read.",
        )
    })?;

    // A response that describes a different issue than the one requested means
    // the executable did not answer our question; refuse it rather than show
    // unrelated text as if it belonged to this project.
    let consistent = matches!(
        parse_issue_reference(&issue.url),
        Ok(IssueReference::Url { owner, repository, number })
            if target.repository.matches(&owner, &repository) && number == target.number
    );
    if !consistent {
        return Err(IpcError::new(
            IpcErrorCode::ParseError,
            "The GitHub CLI returned an issue that does not match this project.",
        ));
    }

    let title = bound_chars(
        sanitize_text(&issue.title).trim(),
        ISSUE_TITLE_MAX_CHARS,
    )
    .0;
    let (body, body_truncated) = bound_bytes(
        sanitize_text(issue.body.as_deref().unwrap_or_default()).trim(),
        ISSUE_BODY_MAX_BYTES,
    );
    Ok(GitHubIssuePreview {
        number: target.number,
        url: canonical_issue_url(&target.repository, target.number),
        suggested_slug: suggested_slug(target.number, &title),
        title,
        body,
        body_truncated,
        repository: target.repository.full_name(),
    })
}

fn canonical_issue_url(repository: &RepositorySlug, number: u64) -> String {
    format!(
        "https://github.com/{}/{}/issues/{number}",
        repository.owner, repository.name
    )
}

#[derive(Debug, Deserialize)]
struct GhIssueResponse {
    number: u64,
    title: String,
    url: String,
    #[serde(default)]
    body: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stream {
    Stdout,
    Stderr,
}

#[derive(Debug)]
enum Captured {
    Complete(Vec<u8>),
    Exceeded,
}

#[derive(Debug, Default)]
struct Capture {
    stdout: Option<Captured>,
    stderr: Option<Captured>,
}

impl Capture {
    fn record(&mut self, stream: Stream, outcome: Captured) {
        match stream {
            Stream::Stdout => self.stdout = Some(outcome),
            Stream::Stderr => self.stderr = Some(outcome),
        }
    }
}

/// Runs `gh` with a fixed argument array, a wall-clock deadline, and bounded
/// pipes. The deadline is enforced by polling the child (portable across the
/// three desktop targets) and killing it once it passes, so a hung executable
/// can never hold a blocking worker forever.
fn run_gh(
    program: &Path,
    args: &[String],
    timeout: Duration,
    max_stdout: usize,
) -> Result<String, IpcError> {
    let mut command = crate::util::no_window_command(program);
    command
        .args(args)
        // A GUI flow must never sit on an interactive auth or update prompt.
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_NO_UPDATE_NOTIFIER", "1")
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => IpcError::new(
            IpcErrorCode::CliExecutableNotFound,
            "The GitHub CLI (gh) was not found. Install it and run `gh auth login`.",
        ),
        _ => IpcError::new(
            IpcErrorCode::IoError,
            format!("could not start the GitHub CLI: {error}"),
        ),
    })?;

    let (sender, receiver) = mpsc::channel();
    if let Some(stdout) = child.stdout.take() {
        spawn_capture(stdout, max_stdout, Stream::Stdout, sender.clone());
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_capture(stderr, GH_MAX_STDERR_BYTES, Stream::Stderr, sender.clone());
    }
    drop(sender);

    let deadline = Instant::now() + timeout;
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(error) => {
                let _ = child.kill();
                return Err(IpcError::new(
                    IpcErrorCode::IoError,
                    format!("could not read the GitHub CLI exit status: {error}"),
                ));
            }
        }
        if Instant::now() >= deadline {
            timed_out = true;
            break None;
        }
        std::thread::sleep(GH_POLL_INTERVAL);
    };
    if timed_out {
        let _ = child.kill();
        let _ = child.wait();
    }

    // The readers own the pipes and detach on their own once the child is gone;
    // only the payload is awaited here, with a grace period so a pipe held open
    // by a grandchild cannot block the worker.
    let mut capture = Capture::default();
    let drain_deadline = Instant::now() + GH_DRAIN_GRACE;
    while capture.stdout.is_none() || capture.stderr.is_none() {
        let remaining = drain_deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match receiver.recv_timeout(remaining) {
            Ok((stream, outcome)) => capture.record(stream, outcome),
            Err(_) => break,
        }
    }
    drop(receiver);

    if timed_out {
        return Err(IpcError::new(
            IpcErrorCode::Timeout,
            "Reading the GitHub issue timed out.",
        ));
    }

    let stdout = match capture.stdout {
        Some(Captured::Complete(bytes)) => bytes,
        Some(Captured::Exceeded) => {
            return Err(IpcError::new(
                IpcErrorCode::OutputLimitExceeded,
                "The GitHub CLI returned more data than Ferryx reads.",
            ))
        }
        None => {
            return Err(IpcError::new(
                IpcErrorCode::Timeout,
                "The GitHub CLI did not finish reading the issue.",
            ))
        }
    };
    let stderr = match capture.stderr {
        Some(Captured::Complete(bytes)) => bytes,
        _ => Vec::new(),
    };
    let stderr = String::from_utf8_lossy(&stderr).to_string();

    if !status.is_some_and(|status| status.success()) {
        return Err(classify_gh_failure(&stderr, &target_repository_name(args)));
    }

    Ok(String::from_utf8_lossy(&stdout).to_string())
}

/// The `--repo` value of a well-formed invocation, used only to word failures.
fn target_repository_name(args: &[String]) -> String {
    args.iter()
        .position(|arg| arg == "--repo")
        .and_then(|index| args.get(index + 1))
        .cloned()
        .unwrap_or_else(|| "this repository".to_string())
}

fn spawn_capture<R: Read + Send + 'static>(
    mut reader: R,
    cap: usize,
    stream: Stream,
    sender: mpsc::Sender<(Stream, Captured)>,
) {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut chunk = [0u8; 8192];
        let mut exceeded = false;
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => {
                    if bytes.len() + read > cap {
                        // Keep draining so the child can still exit; the payload
                        // is reported as oversized instead of silently cut.
                        exceeded = true;
                        continue;
                    }
                    bytes.extend_from_slice(&chunk[..read]);
                }
                Err(_) => break,
            }
        }
        let outcome = if exceeded {
            Captured::Exceeded
        } else {
            Captured::Complete(bytes)
        };
        let _ = sender.send((stream, outcome));
    });
}

fn classify_gh_failure(stderr: &str, repository: &str) -> IpcError {
    let lowered = stderr.to_ascii_lowercase();
    let mentions = |markers: &[&str]| markers.iter().any(|marker| lowered.contains(marker));
    if mentions(&[
        "gh auth login",
        "not logged in",
        "authentication",
        "requires authentication",
        "http 401",
        "bad credentials",
    ]) {
        return IpcError::new(
            IpcErrorCode::Unauthorized,
            "The GitHub CLI is not authenticated. Run `gh auth login` and try again.",
        );
    }
    if mentions(&["http 404", "could not resolve to an issue", "not found"]) {
        return IpcError::new(
            IpcErrorCode::NotFound,
            format!("The GitHub CLI could not find that issue in {repository}."),
        );
    }
    if mentions(&["rate limit", "http 403"]) {
        return IpcError::new(
            IpcErrorCode::RateLimited,
            "GitHub refused the request, most likely because of a rate limit. Try again later.",
        );
    }
    let detail = bound_chars(sanitize_text(stderr).trim(), 300).0;
    if detail.is_empty() {
        return IpcError::new(
            IpcErrorCode::InternalError,
            "The GitHub CLI failed to read the issue.",
        );
    }
    IpcError::new(
        IpcErrorCode::InternalError,
        format!("The GitHub CLI failed to read the issue: {detail}"),
    )
}

const GH_PROGRAM: &str = "gh";

/// Resolves `gh` through the shared search path rather than the process PATH
/// alone: a GUI-launched app inherits a minimal PATH on macOS, where a Homebrew
/// or `~/.local/bin` install would be invisible, while the login shell's PATH is
/// what the user's own terminal resolves against.
fn resolve_gh_program() -> Result<PathBuf, IpcError> {
    resolve_gh_in(&crate::ipc::agents::search_paths())
}

/// Resolves `gh` in an explicit search path list, so the lookup stays testable
/// without touching the process environment or spawning a login shell.
fn resolve_gh_in(search_paths: &[PathBuf]) -> Result<PathBuf, IpcError> {
    crate::ipc::agents::resolve_binary(GH_PROGRAM, search_paths).ok_or_else(|| {
        IpcError::new(
            IpcErrorCode::CliExecutableNotFound,
            "The GitHub CLI (gh) was not found on PATH. Install it and run `gh auth login`.",
        )
    })
}

fn invalid_reference() -> IpcError {
    IpcError::new(
        IpcErrorCode::InvalidArgument,
        "Enter an issue number (12) or a github.com issue URL.",
    )
}

fn unsupported_origin() -> IpcError {
    IpcError::new(
        IpcErrorCode::Unsupported,
        "Issue intake needs a github.com origin, and this project's remote does not point at one.",
    )
}

/// Accepts an issue number (`12`, `#12`) or a canonical github.com issue URL.
fn parse_issue_reference(raw: &str) -> Result<IssueReference, IpcError> {
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed.chars().count() > ISSUE_REF_MAX_CHARS
        || trimmed.chars().any(char::is_control)
    {
        return Err(invalid_reference());
    }
    if let Some(number) = parse_issue_number(trimmed) {
        return Ok(IssueReference::Number(number));
    }
    let lowered = trimmed.to_ascii_lowercase();
    if !trimmed.contains("://") && !lowered.starts_with("github.com/") {
        return Err(invalid_reference());
    }
    let candidate = if lowered.starts_with("github.com/") {
        format!("https://{trimmed}")
    } else {
        trimmed.to_string()
    };
    let url = Url::parse(&candidate).map_err(|_| invalid_reference())?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || !matches!(url.port(), None | Some(443))
        || !is_github_host(url.host_str())
    {
        return Err(invalid_reference());
    }
    let segments: Vec<&str> = url.path().split('/').filter(|part| !part.is_empty()).collect();
    let [owner, repository, kind, number] = segments[..] else {
        return Err(invalid_reference());
    };
    if kind != "issues" || !is_repository_component(owner) || !is_repository_component(repository) {
        return Err(invalid_reference());
    }
    let number = parse_issue_number(number).ok_or_else(invalid_reference)?;
    Ok(IssueReference::Url {
        owner: owner.to_string(),
        repository: repository.to_string(),
        number,
    })
}

/// Parses a remote URL into its `owner/repository` pair, rejecting anything
/// that is not a github.com repository.
fn parse_github_origin(remote: &str) -> Result<RepositorySlug, IpcError> {
    let trimmed = remote.trim();
    let path = if trimmed.contains("://") {
        let url = Url::parse(trimmed).map_err(|_| unsupported_origin())?;
        if !matches!(url.scheme(), "ssh" | "git" | "https" | "http") || !is_github_host(url.host_str())
        {
            return Err(unsupported_origin());
        }
        url.path().to_string()
    } else {
        // scp-like form: `git@github.com:owner/repository.git`
        let Some((host, path)) = trimmed.split_once(':') else {
            return Err(unsupported_origin());
        };
        let host = host.rsplit('@').next().unwrap_or(host);
        if host.contains(['/', '\\']) || !is_github_host(Some(host)) {
            return Err(unsupported_origin());
        }
        path.to_string()
    };
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let segments: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    let [owner, name] = segments[..] else {
        return Err(unsupported_origin());
    };
    if !is_repository_component(owner) || !is_repository_component(name) {
        return Err(unsupported_origin());
    }
    Ok(RepositorySlug {
        owner: owner.to_string(),
        name: name.to_string(),
    })
}

fn is_github_host(host: Option<&str>) -> bool {
    host.is_some_and(|host| host.eq_ignore_ascii_case("github.com"))
}

/// GitHub owner and repository names are limited to this charset, which is also
/// what keeps them safe to hand to a child process as a single argument.
fn is_repository_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && !value.starts_with('.')
        && !value.starts_with('-')
        && !value.ends_with('.')
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
}

fn parse_issue_number(value: &str) -> Option<u64> {
    let digits = value.strip_prefix('#').unwrap_or(value);
    if digits.is_empty() || digits.len() > 10 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let number = digits.parse::<u64>().ok()?;
    (number > 0).then_some(number)
}

/// `issue-<number>-<title words>`, restricted to characters a Git branch
/// component accepts. The result is only ever a proposal: the dialog shows it
/// in the editable slug field and the user decides what to create.
fn suggested_slug(number: u64, title: &str) -> String {
    let mut words: Vec<String> = Vec::new();
    let mut current = String::new();
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            current.push(ch.to_ascii_lowercase());
        } else if !current.is_empty() {
            words.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    let mut slug = format!("issue-{number}");
    for word in words.into_iter().take(SLUG_TITLE_WORDS) {
        let word: String = word.chars().take(SLUG_WORD_MAX_CHARS).collect();
        if word.is_empty() {
            continue;
        }
        let candidate = format!("{slug}-{word}");
        if candidate.chars().count() > SLUG_MAX_CHARS {
            break;
        }
        slug = candidate;
    }
    slug
}

/// Drops control characters so preview text (and anything the user copies out
/// of it into a pane) cannot carry terminal escapes. Newlines and tabs survive.
fn sanitize_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {
                if chars.peek() != Some(&'\n') {
                    out.push('\n');
                }
            }
            '\n' | '\t' => out.push(ch),
            ch if ch.is_control() => {}
            ch => out.push(ch),
        }
    }
    out
}

fn bound_bytes(value: &str, max: usize) -> (String, bool) {
    if value.len() <= max {
        return (value.to_string(), false);
    }
    let mut end = max;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    (value[..end].to_string(), true)
}

fn bound_chars(value: &str, max: usize) -> (String, bool) {
    if value.chars().count() <= max {
        return (value.to_string(), false);
    }
    (value.chars().take(max).collect(), true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worktree::{run_git, WorktreeManager};
    use tauri::Manager;

    fn target(owner: &str, name: &str, number: u64) -> IssueTarget {
        IssueTarget {
            repository: RepositorySlug {
                owner: owner.to_string(),
                name: name.to_string(),
            },
            number,
        }
    }

    fn git_fixture(remote: Option<&str>) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        run_git(dir.path(), &["init", "--quiet"]).unwrap();
        if let Some(remote) = remote {
            run_git(dir.path(), &["remote", "add", "origin", remote]).unwrap();
        }
        dir
    }

    fn issue_json(number: u64, title: &str, repository: &str, body: &str) -> String {
        serde_json::json!({
            "number": number,
            "title": title,
            "url": format!("https://github.com/{repository}/issues/{number}"),
            "body": body,
        })
        .to_string()
    }

    #[test]
    fn issue_numbers_and_hash_forms_resolve_to_the_same_number() {
        for raw in ["12", " #12 ", "#12"] {
            assert_eq!(
                parse_issue_reference(raw).unwrap(),
                IssueReference::Number(12),
                "{raw}"
            );
        }
    }

    #[test]
    fn canonical_issue_urls_resolve_to_owner_repository_and_number() {
        let expected = IssueReference::Url {
            owner: "acme".to_string(),
            repository: "widgets".to_string(),
            number: 7,
        };
        for raw in [
            "https://github.com/acme/widgets/issues/7",
            "https://github.com/acme/widgets/issues/7/",
            "https://github.com/acme/widgets/issues/7#issuecomment-1",
            "http://github.com/acme/widgets/issues/7",
            "github.com/acme/widgets/issues/7",
            "  https://github.com/acme/widgets/issues/7  ",
        ] {
            assert_eq!(parse_issue_reference(raw).unwrap(), expected, "{raw}");
        }
    }

    #[test]
    fn non_issue_and_non_github_references_are_rejected() {
        for raw in [
            "",
            "   ",
            "0",
            "#0",
            "12.5",
            "-12",
            "+12",
            "https://github.com/acme/widgets/pull/7",
            "https://github.com/acme/widgets",
            "https://github.com/acme/widgets/issues",
            "https://github.com/acme/widgets/issues/abc",
            "https://github.com/acme/widgets/issues/12/files",
            "https://gitlab.com/acme/widgets/issues/7",
            "https://github.com.evil.test/acme/widgets/issues/7",
            "https://www.github.com/acme/widgets/issues/7",
            "https://user:token@github.com/acme/widgets/issues/7",
            "https://github.com:8443/acme/widgets/issues/7",
            "file:///etc/passwd",
        ] {
            assert_eq!(
                parse_issue_reference(raw).unwrap_err().code,
                IpcErrorCode::InvalidArgument,
                "{raw}"
            );
        }
    }

    #[test]
    fn argument_injection_shaped_references_are_rejected() {
        for raw in [
            "12 --repo evil/repo",
            "12; touch /tmp/ferryx-pwned",
            "$(touch /tmp/ferryx-pwned)",
            "`touch /tmp/ferryx-pwned`",
            "12 && rm -rf /",
            "12 | cat /etc/passwd",
            "12\n--repo evil/repo",
            "https://github.com/acme/widgets/issues/7 --repo evil/repo",
            "https://github.com/acme/widgets/issues/7;id",
            "https://github.com/-acme/widgets/issues/7",
            "https://github.com/acme/wid gets/issues/7",
            "--upload-file=/etc/passwd",
        ] {
            assert_eq!(
                parse_issue_reference(raw).unwrap_err().code,
                IpcErrorCode::InvalidArgument,
                "{raw}"
            );
        }
    }

    #[test]
    fn oversized_and_control_character_references_are_rejected() {
        for raw in [
            "1".repeat(ISSUE_REF_MAX_CHARS + 1),
            "9".repeat(19),
            "12\u{7}".to_string(),
            "12\u{202e}21".to_string(),
        ] {
            assert_eq!(
                parse_issue_reference(&raw).unwrap_err().code,
                IpcErrorCode::InvalidArgument,
                "{raw}"
            );
        }
    }

    #[test]
    fn github_origins_parse_from_ssh_and_https_forms() {
        for remote in [
            "git@github.com:acme/widgets.git",
            "git@github.com:acme/widgets",
            "ssh://git@github.com/acme/widgets.git",
            "https://github.com/acme/widgets.git",
            "https://github.com/acme/widgets",
            "https://github.com/acme/widgets/",
            "git://github.com/acme/widgets.git",
        ] {
            let slug = parse_github_origin(remote).unwrap();
            assert_eq!(slug.full_name(), "acme/widgets", "{remote}");
        }
    }

    #[test]
    fn non_github_origins_are_unsupported() {
        for remote in [
            "",
            "git@gitlab.com:acme/widgets.git",
            "https://gitlab.com/acme/widgets.git",
            "https://github.com.evil.test/acme/widgets.git",
            "https://github.com/acme",
            "https://github.com/acme/widgets/extra",
            "git@github.com:acme/widgets.git/path",
        ] {
            assert_eq!(
                parse_github_origin(remote).unwrap_err().code,
                IpcErrorCode::Unsupported,
                "{remote}"
            );
        }
    }

    #[test]
    fn suggested_slugs_are_branch_safe() {
        let slug = suggested_slug(42, "Add drag-and-drop for panes (v2)!");
        assert_eq!(slug, "issue-42-add-drag-and-drop-for-panes");
        assert!(WorktreeManager::format_branch_name("orca-lite", &slug).is_ok());
        assert_eq!(suggested_slug(7, "   "), "issue-7");
        assert_eq!(
            suggested_slug(8, "\u{d55c}\u{ad6d}\u{c5b4} \u{c81c}\u{baa9}"),
            "issue-8"
        );
        assert!(
            suggested_slug(9, &"verylongword ".repeat(20))
                .chars()
                .count()
                <= SLUG_MAX_CHARS
        );
    }

    #[test]
    fn gh_arguments_are_a_fixed_array() {
        assert_eq!(
            gh_issue_view_args(&target("acme", "widgets", 12)),
            vec![
                "issue",
                "view",
                "12",
                "--repo",
                "acme/widgets",
                "--json",
                "number,title,url,body"
            ]
        );
    }

    #[test]
    fn plan_issue_accepts_a_bare_number_for_the_projects_own_origin() {
        let repo = git_fixture(Some("https://github.com/acme/widgets.git"));
        assert_eq!(
            plan_issue(repo.path(), "#12").unwrap(),
            target("acme", "widgets", 12)
        );
    }

    #[test]
    fn plan_issue_rejects_an_issue_url_for_another_repository() {
        let repo = git_fixture(Some("git@github.com:acme/widgets.git"));
        let error = plan_issue(repo.path(), "https://github.com/other/thing/issues/5").unwrap_err();
        assert_eq!(error.code, IpcErrorCode::GitHubIssueRepositoryMismatch);
        let details = error.details.clone().expect("mismatch details");
        assert_eq!(details["expected"], "acme/widgets");
        assert_eq!(details["found"], "other/thing");
    }

    #[test]
    fn plan_issue_matches_the_origin_case_insensitively() {
        let repo = git_fixture(Some("git@github.com:Acme/Widgets.git"));
        let resolved = plan_issue(repo.path(), "https://github.com/acme/widgets/issues/7").unwrap();
        assert_eq!(resolved.number, 7);
        assert_eq!(resolved.repository.full_name(), "Acme/Widgets");
    }

    #[test]
    fn plan_issue_reports_missing_and_non_github_origins() {
        let bare = git_fixture(None);
        assert_eq!(
            plan_issue(bare.path(), "12").unwrap_err().code,
            IpcErrorCode::Unsupported
        );
        let gitlab = git_fixture(Some("git@gitlab.com:acme/widgets.git"));
        assert_eq!(
            plan_issue(gitlab.path(), "12").unwrap_err().code,
            IpcErrorCode::Unsupported
        );
    }

    #[test]
    fn error_codes_round_trip_through_their_wire_names() {
        assert_eq!(
            serde_json::to_string(&IpcErrorCode::GitHubIssueRepositoryMismatch).unwrap(),
            "\"GITHUB_ISSUE_REPOSITORY_MISMATCH\""
        );
        assert_eq!(
            IpcErrorCode::from_code_str("GITHUB_ISSUE_REPOSITORY_MISMATCH"),
            IpcErrorCode::GitHubIssueRepositoryMismatch
        );
    }

    #[tokio::test]
    async fn command_rejects_a_project_without_git() {
        let dir = tempfile::tempdir().unwrap();
        let registry = WorkspaceRegistry::new();
        registry.register("plain", dir.path()).unwrap();
        let app = tauri::test::mock_builder()
            .manage(registry)
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let error = cmd_github_issue_preview(
            app.state(),
            GitHubIssuePreviewRequest {
                workspace_id: "plain".to_string(),
                issue_ref: "12".to_string(),
            },
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, IpcErrorCode::NotAGitRepository);
    }

    #[cfg(unix)]
    mod fixture_gh {
        use super::*;
        use std::os::unix::fs::PermissionsExt;

        fn write_script(dir: &Path, name: &str, body: &str) -> PathBuf {
            let path = dir.join(name);
            std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            path
        }

        fn fixture_gh(
            dir: &Path,
            stdout_json: &str,
            args_log: Option<&Path>,
        ) -> PathBuf {
            let mut script = String::new();
            if let Some(log) = args_log {
                script.push_str(&format!("printf '%s\\n' \"$@\" > {}\n", log.display()));
            }
            script.push_str(&format!(
                "cat <<'FERRYX_FIXTURE'\n{stdout_json}\nFERRYX_FIXTURE\n"
            ));
            write_script(dir, "gh", &script)
        }

        #[test]
        fn reads_an_issue_with_a_fixed_argument_array() {
            let dir = tempfile::tempdir().unwrap();
            let log = dir.path().join("args.txt");
            let gh = fixture_gh(
                dir.path(),
                &issue_json(12, "Fix the parser", "acme/widgets", "Steps to reproduce"),
                Some(&log),
            );
            let preview = fetch_issue(&gh, &target("acme", "widgets", 12)).unwrap();
            assert_eq!(preview.number, 12);
            assert_eq!(preview.title, "Fix the parser");
            assert_eq!(preview.url, "https://github.com/acme/widgets/issues/12");
            assert_eq!(preview.body, "Steps to reproduce");
            assert!(!preview.body_truncated);
            assert_eq!(preview.repository, "acme/widgets");
            assert_eq!(preview.suggested_slug, "issue-12-fix-the-parser");
            let args: Vec<String> = std::fs::read_to_string(&log)
                .unwrap()
                .lines()
                .map(str::to_string)
                .collect();
            assert_eq!(
                args,
                vec![
                    "issue",
                    "view",
                    "12",
                    "--repo",
                    "acme/widgets",
                    "--json",
                    "number,title,url,body"
                ]
            );
        }

        #[test]
        fn maps_an_unauthenticated_gh_to_a_structured_error() {
            let dir = tempfile::tempdir().unwrap();
            let gh = write_script(
                dir.path(),
                "gh",
                "echo 'To get started with GitHub CLI, please run: gh auth login' >&2\nexit 1",
            );
            let error = fetch_issue(&gh, &target("acme", "widgets", 12)).unwrap_err();
            assert_eq!(error.code, IpcErrorCode::Unauthorized);
            assert!(error.message.contains("gh auth login"));
        }

        #[test]
        fn maps_an_unknown_issue_to_a_structured_not_found() {
            let dir = tempfile::tempdir().unwrap();
            let gh = write_script(
                dir.path(),
                "gh",
                "echo 'GraphQL: Could not resolve to an Issue with the number of 12. (repository.issue)' >&2\nexit 1",
            );
            let error = fetch_issue(&gh, &target("acme", "widgets", 12)).unwrap_err();
            assert_eq!(error.code, IpcErrorCode::NotFound);
            assert!(error.message.contains("acme/widgets"));
        }

        #[test]
        fn reports_a_missing_gh_executable() {
            let dir = tempfile::tempdir().unwrap();
            let missing = dir.path().join("gh-does-not-exist");
            assert_eq!(
                fetch_issue(&missing, &target("acme", "widgets", 12))
                    .unwrap_err()
                    .code,
                IpcErrorCode::CliExecutableNotFound
            );
            let search: Vec<PathBuf> = vec![dir.path().to_path_buf()];
            assert_eq!(
                resolve_gh_in(&search).unwrap_err().code,
                IpcErrorCode::CliExecutableNotFound
            );
            assert_eq!(
                resolve_gh_in(&[]).unwrap_err().code,
                IpcErrorCode::CliExecutableNotFound
            );
            // A file named `gh` that cannot be executed is not the CLI either; the
            // shared resolver enforces the executable bit on unix and PATHEXT on
            // Windows, so a stale plain file never gets spawned.
            let plain = dir.path().join("plain");
            std::fs::create_dir_all(&plain).unwrap();
            std::fs::write(plain.join("gh"), "not a program").unwrap();
            assert_eq!(
                resolve_gh_in(&[plain]).unwrap_err().code,
                IpcErrorCode::CliExecutableNotFound
            );
            let gh = write_script(dir.path(), "gh", "exit 0");
            assert_eq!(resolve_gh_in(&search).unwrap(), gh);
        }

        #[test]
        fn bounds_a_hung_gh_with_the_deadline() {
            let dir = tempfile::tempdir().unwrap();
            let gh = write_script(dir.path(), "gh", "exec sleep 30");
            let started = Instant::now();
            let error = run_gh(
                &gh,
                &["issue".to_string()],
                Duration::from_millis(300),
                GH_MAX_STDOUT_BYTES,
            )
            .unwrap_err();
            assert_eq!(error.code, IpcErrorCode::Timeout);
            assert!(
                started.elapsed() < Duration::from_secs(20),
                "the deadline, not the child, must end the wait"
            );
        }

        #[test]
        fn maps_a_malformed_response_to_a_parse_error() {
            let dir = tempfile::tempdir().unwrap();
            let gh = write_script(dir.path(), "gh", "echo 'not json at all'");
            assert_eq!(
                fetch_issue(&gh, &target("acme", "widgets", 12))
                    .unwrap_err()
                    .code,
                IpcErrorCode::ParseError
            );
        }

        #[test]
        fn refuses_a_response_for_another_issue() {
            let dir = tempfile::tempdir().unwrap();
            let other_repository = fixture_gh(
                dir.path(),
                &issue_json(12, "Elsewhere", "other/thing", "nope"),
                None,
            );
            assert_eq!(
                fetch_issue(&other_repository, &target("acme", "widgets", 12))
                    .unwrap_err()
                    .code,
                IpcErrorCode::ParseError
            );
            let other_number = fixture_gh(
                dir.path(),
                &issue_json(99, "Elsewhere", "acme/widgets", "nope"),
                None,
            );
            assert_eq!(
                fetch_issue(&other_number, &target("acme", "widgets", 12))
                    .unwrap_err()
                    .code,
                IpcErrorCode::ParseError
            );
        }

        #[test]
        fn rejects_an_oversized_response_instead_of_truncating_it() {
            let dir = tempfile::tempdir().unwrap();
            let gh = write_script(
                dir.path(),
                "gh",
                "i=0\nwhile [ $i -lt 4000 ]; do printf 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'; i=$((i+1)); done",
            );
            let error = run_gh(&gh, &["issue".to_string()], GH_TIMEOUT, 4096).unwrap_err();
            assert_eq!(error.code, IpcErrorCode::OutputLimitExceeded);
        }

        #[test]
        fn bounds_an_oversized_issue_body_and_flags_it() {
            let dir = tempfile::tempdir().unwrap();
            let body = "x".repeat(ISSUE_BODY_MAX_BYTES * 4);
            let gh = fixture_gh(
                dir.path(),
                &issue_json(12, "Big issue", "acme/widgets", &body),
                None,
            );
            let preview = fetch_issue(&gh, &target("acme", "widgets", 12)).unwrap();
            assert!(preview.body_truncated);
            assert_eq!(preview.body.len(), ISSUE_BODY_MAX_BYTES);
        }

        #[test]
        fn issue_text_stays_inert_and_never_reaches_a_shell() {
            let dir = tempfile::tempdir().unwrap();
            let pwned = dir.path().join("pwned");
            let hostile = format!(
                "$(touch {p}) `touch {p}` ; touch {p} && touch {p}",
                p = pwned.display()
            );
            let log = dir.path().join("args.txt");
            let gh = fixture_gh(
                dir.path(),
                &issue_json(3, &format!("Inject {hostile}"), "acme/widgets", &hostile),
                Some(&log),
            );
            let preview = fetch_issue(&gh, &target("acme", "widgets", 3)).unwrap();
            assert!(preview.title.contains("$(touch"));
            assert!(preview.body.contains("`touch"));
            assert!(!pwned.exists(), "issue text must never be executed");
            let args = std::fs::read_to_string(&log).unwrap();
            assert!(!args.contains("touch"));
            assert!(!args.contains("$("));
        }
    }
}
