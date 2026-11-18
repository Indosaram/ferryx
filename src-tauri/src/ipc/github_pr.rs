//! The pull request a lane's branch has open, read from an already authenticated `gh`.
//!
//! Read only, by construction: this module can construct exactly one `gh` argument vector,
//! `pr view`, which `crates/ferryx-hub/src/effect.rs:1187` classifies as `Effect::Read`. It never
//! issues `pr create`, `pr merge`, `pr close`, `pr ready`, `pr review`, `gh api`, or any other
//! mutating command, so it cannot reach the `Effect::VcsProtected` gate at `effect.rs:1228`.
//! A merged PR is only ever *observed* again: when the owner merges through their own shell, the
//! next read returns `merged` and the lane shows it, with Ferryx writing nothing to GitHub.
//!
//! The executable is invoked as a fixed argument array (never through a shell) on a blocking
//! worker, with a bounded response and a wall-clock deadline, exactly as
//! `ipc/github_issue.rs` already reads an issue. Credentials stay with `gh`: Ferryx stores none.

use crate::ipc::error::{IpcError, IpcErrorCode};
use crate::ipc::run_blocking;
use crate::worktree::git::{git_remote_origin_url, run_git};
#[cfg(feature = "desktop")]
use crate::worktree::WorkspaceRegistry;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::mpsc;
use std::time::{Duration, Instant};
#[cfg(feature = "desktop")]
use tauri::State;
use url::Url;

/// Wall-clock ceiling for one `gh pr view` invocation.
pub const PR_TIMEOUT: Duration = Duration::from_secs(20);
/// A larger response is rejected, never silently cut. A PR view is a few hundred bytes.
pub const PR_MAX_STDOUT_BYTES: usize = 64 * 1024;
pub const PR_MAX_STDERR_BYTES: usize = 32 * 1024;
/// Ceiling on the PR title handed to the UI.
pub const PR_TITLE_MAX_CHARS: usize = 300;

const BRANCH_MAX_CHARS: usize = 200;
const GH_POLL_INTERVAL: Duration = Duration::from_millis(10);
const GH_DRAIN_GRACE: Duration = Duration::from_secs(5);

/// The `--json` field list. Every name is a field `gh pr view` documents.
const PR_JSON_FIELDS: &str = "number,state,url,title,headRefName,isDraft";

/// The Tauri command's request body. Only the command takes one: the hub's sync reads from a lane
/// directory through [`GhPrSource`] and never builds this.
#[cfg(feature = "desktop")]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitHubPrViewRequest {
    pub workspace_id: String,
    /// The lane's branch (`lane/<lane-key>`). Omitted means the project's checked-out branch.
    #[serde(default)]
    pub branch: Option<String>,
}

/// One pull request as `gh pr view` reported it. Inert display data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanePullRequestView {
    pub number: u64,
    /// `open`, `closed`, or `merged` -- GitHub's own lifecycle, lowercased.
    pub state: String,
    pub url: String,
    pub title: String,
    pub head_branch: String,
    pub is_draft: bool,
    /// Canonical `owner/repository` the PR was read from.
    pub repository: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PrTarget {
    repository: RepositorySlug,
    head_branch: String,
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

/// The lane's pull request, or None when its branch has none.
///
/// A branch without a PR is an ordinary state (the lane has not opened one yet), not a failure,
/// so it is reported as an absent link rather than an error the UI would have to decode.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_github_pr_view(
    registry: State<'_, WorkspaceRegistry>,
    request: GitHubPrViewRequest,
) -> Result<Option<LanePullRequestView>, IpcError> {
    let registry = (*registry).clone();
    run_blocking(move || {
        let manager = registry
            .manager(&request.workspace_id)
            .map_err(IpcError::from)?;
        if !manager.is_git_backed() {
            return Err(IpcError::new(
                IpcErrorCode::NotAGitRepository,
                "Reading a pull request needs a local Git-backed project.",
            ));
        }
        // Resolve and validate everything before spawning anything: an unusable branch or a
        // non-GitHub origin never reaches `gh`.
        let target = plan_pr(manager.repo_root(), request.branch.as_deref())?;
        let program = resolve_gh_program()?;
        fetch_pr(&program, &target)
    })
    .await
}

/// The hub's read-only source of a lane's pull request: the production
/// [`ferryx_hub::lanes::pull_request::PrSource`].
///
/// It exists so the hub's lane sync can keep its no-`gh` rule without building a second GitHub
/// path: this is [`plan_pr`] and [`fetch_pr`] -- the same one `gh pr view` argv, the same
/// validation, the same bounded read and deadline -- reached from a lane's directory instead of a
/// Tauri workspace id. Two differences, exactly the two the sync contract asks for:
///
/// * the whole read runs on a blocking worker, so the hub's async reactor never waits on a
///   subprocess;
/// * a lane that simply has no pull request (no `gh`, no auth, no PR for the branch, no GitHub
///   origin, not a Git project) answers `Ok(None)`, because none of those is a failure the sweep
///   should count.
///
/// A read that genuinely broke (a timeout, an oversized response, a rate limit, an unparseable
/// answer) stays an `Err`: [`ferryx_hub::lane_pr_sync::sync`] counts it against the lane and the
/// pass carries on to the next lane.
pub struct GhPrSource;

#[async_trait::async_trait]
impl ferryx_hub::lanes::pull_request::PrSource for GhPrSource {
    async fn read(
        &self,
        target: &ferryx_hub::lanes::pull_request::PrReadTarget,
    ) -> ferryx_hub::Result<Option<ferryx_hub::lanes::pull_request::LanePullRequest>> {
        let directory = PathBuf::from(&target.cwd);
        let branch = ferryx_hub::lanes::pull_request::lane_branch(&target.lane_key);
        let lane_key = target.lane_key.clone();
        let observed = run_blocking(move || {
            let plan = plan_pr(&directory, Some(branch.as_str()))?;
            let program = resolve_gh_program()?;
            fetch_pr(&program, &plan)
        })
        .await;
        let pr = match observed {
            Ok(Some(pr)) => pr,
            Ok(None) => return Ok(None),
            Err(error) if is_absent(&error) => return Ok(None),
            Err(error) => return Err(ferryx_hub::HubError::message(error.to_string())),
        };
        // `gh` reports the number as an unsigned integer; the projection stores the same value in a
        // signed column (migration 0057 checks `pr_number > 0`). A number that cannot be stored is
        // a read that broke, not an absence: refuse it rather than write a clamped one.
        let Ok(pr_number) = i64::try_from(pr.number) else {
            return Err(ferryx_hub::HubError::message(format!(
                "the GitHub CLI reported a pull request number Ferryx cannot store: {}",
                pr.number
            )));
        };
        Ok(Some(ferryx_hub::lanes::pull_request::LanePullRequest {
            lane_key,
            repo: pr.repository,
            pr_number,
            state: pr.state,
            url: pr.url,
            title: pr.title,
            head_branch: pr.head_branch,
            // Stamped by `record` with the read time it is given.
            observed_at: String::new(),
        }))
    }
}

/// Whether a failed read means "this lane has no pull request to record" rather than "the read
/// broke".
///
/// Every code here is one `plan_pr` or `run_gh` raises for an ordinary state: nothing named `gh`
/// on this machine (`CliExecutableNotFound`), `gh` installed but signed out (`Unauthorized`), the
/// lane's directory not a Git project or its remote not GitHub (`Unsupported`), or the branch's PR
/// gone (`NotFound`). Everything else -- a timeout, an oversized response, a rate limit, a response
/// that does not describe this branch -- stays a failure, because it may be true again in five
/// minutes and should be visible while it is not.
fn is_absent(error: &IpcError) -> bool {
    matches!(
        &error.code,
        IpcErrorCode::CliExecutableNotFound
            | IpcErrorCode::Unauthorized
            | IpcErrorCode::Unsupported
            | IpcErrorCode::NotFound
    )
}

/// Resolves the branch and the project's GitHub origin into one read target.
fn plan_pr(repo_root: &Path, branch: Option<&str>) -> Result<PrTarget, IpcError> {
    let head_branch = match branch {
        Some(branch) => validate_branch(branch)?,
        None => current_branch(repo_root)?,
    };
    let remote = git_remote_origin_url(repo_root).ok_or_else(|| {
        IpcError::new(
            IpcErrorCode::Unsupported,
            "This project has no Git remote to read a pull request from.",
        )
    })?;
    let repository = parse_github_origin(&remote)?;
    Ok(PrTarget {
        repository,
        head_branch,
    })
}

/// The branch checked out at `repo_root`, read through git rather than guessed.
fn current_branch(repo_root: &Path) -> Result<String, IpcError> {
    let raw = run_git(repo_root, &["symbolic-ref", "--quiet", "--short", "HEAD"]).map_err(|_| {
        IpcError::new(
            IpcErrorCode::Unsupported,
            "This project is not on a branch, so there is no pull request to read. Pass the lane's branch.",
        )
    })?;
    let branch = raw.trim();
    if branch.is_empty() {
        return Err(IpcError::new(
            IpcErrorCode::Unsupported,
            "This project is not on a branch, so there is no pull request to read. Pass the lane's branch.",
        ));
    }
    validate_branch(branch)
}

/// A branch name that is safe to hand to `gh` as one argument.
///
/// The charset git accepts in a ref plus a leading-dash rejection: a value that passes here
/// cannot become a flag (`--repo evil/repo`) or a second command, and it cannot escape the
/// `--json` read that follows it.
fn validate_branch(raw: &str) -> Result<String, IpcError> {
    let branch = raw.trim();
    let invalid = || {
        IpcError::new(
            IpcErrorCode::InvalidArgument,
            "The lane branch is not a usable Git branch name.",
        )
    };
    if branch.is_empty()
        || branch.chars().count() > BRANCH_MAX_CHARS
        || branch.starts_with('-')
        || branch.starts_with('/')
        || branch.ends_with('/')
        || branch.ends_with(".lock")
        || branch.contains("..")
        || branch.contains("//")
        || branch.contains("@{")
        || branch
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace())
        || !branch.chars().all(|ch| {
            ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/' | '+')
        })
    {
        return Err(invalid());
    }
    Ok(branch.to_string())
}

/// The exact argument array handed to `gh`. Every element is either a constant or a value that
/// passed [`validate_branch`] / GitHub's own `owner/repository` charset, so no caller text can
/// turn into a flag or a second command.
///
/// `pr view` is in the `Effect::Read` set (`effect.rs:1187`). There is no other argv in this
/// module: no `pr merge`, `pr close`, `pr create`, `pr edit`, `pr ready`, `pr review`, and no
/// `gh api`.
fn gh_pr_view_args(target: &PrTarget) -> Vec<String> {
    vec![
        "pr".to_string(),
        "view".to_string(),
        target.head_branch.clone(),
        "--repo".to_string(),
        target.repository.full_name(),
        "--json".to_string(),
        PR_JSON_FIELDS.to_string(),
    ]
}

fn fetch_pr(
    program: &Path,
    target: &PrTarget,
) -> Result<Option<LanePullRequestView>, IpcError> {
    let stdout = match run_gh(
        program,
        &gh_pr_view_args(target),
        PR_TIMEOUT,
        PR_MAX_STDOUT_BYTES,
    )? {
        GhOutcome::NoPullRequest => return Ok(None),
        GhOutcome::Stdout(stdout) => stdout,
    };
    let pr: GhPrResponse = serde_json::from_str(&stdout).map_err(|_| {
        IpcError::new(
            IpcErrorCode::ParseError,
            "The GitHub CLI returned a response Ferryx could not read.",
        )
    })?;

    // A response describing another branch or repository means the executable did not answer
    // our question; refuse it rather than show an unrelated PR as this lane's.
    let url_consistent = parse_pr_url(&pr.url).is_some_and(|(owner, name, number)| {
        target.repository.matches(&owner, &name) && number == pr.number
    });
    if !url_consistent || pr.head_ref_name != target.head_branch {
        return Err(IpcError::new(
            IpcErrorCode::ParseError,
            "The GitHub CLI returned a pull request that does not match this lane's branch.",
        ));
    }

    Ok(Some(LanePullRequestView {
        number: pr.number,
        state: normalize_state(&pr.state)?,
        url: pr.url,
        title: bound_chars(sanitize_text(&pr.title).trim(), PR_TITLE_MAX_CHARS).0,
        head_branch: pr.head_ref_name,
        is_draft: pr.is_draft,
        repository: target.repository.full_name(),
    }))
}

/// GitHub reports `OPEN` / `CLOSED` / `MERGED`; the projection stores the lowercase form.
fn normalize_state(raw: &str) -> Result<String, IpcError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "open" => Ok("open".to_string()),
        "closed" => Ok("closed".to_string()),
        "merged" => Ok("merged".to_string()),
        _ => Err(IpcError::new(
            IpcErrorCode::ParseError,
            "The GitHub CLI reported a pull-request state Ferryx does not know.",
        )),
    }
}

#[derive(Debug, Deserialize)]
struct GhPrResponse {
    number: u64,
    state: String,
    url: String,
    #[serde(default)]
    title: String,
    #[serde(rename = "headRefName", default)]
    head_ref_name: String,
    #[serde(rename = "isDraft", default)]
    is_draft: bool,
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

/// What a successful `gh pr view` produced: a document, or the CLI's "no pull requests" answer.
#[derive(Debug)]
enum GhOutcome {
    Stdout(String),
    NoPullRequest,
}

/// Runs `gh` with a fixed argument array, a wall-clock deadline, and bounded pipes.
///
/// The deadline is enforced by polling the child (portable across the three desktop targets) and
/// killing it once it passes, so a hung executable can never hold a blocking worker forever.
/// Shaped exactly like `ipc/github_issue.rs::run_gh`, which owns the same contract for issues.
fn run_gh(
    program: &Path,
    args: &[String],
    timeout: Duration,
    max_stdout: usize,
) -> Result<GhOutcome, IpcError> {
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
        spawn_capture(stderr, PR_MAX_STDERR_BYTES, Stream::Stderr, sender.clone());
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

    // The readers own the pipes and detach on their own once the child is gone; only the payload
    // is awaited here, with a grace period so a pipe held open by a grandchild cannot block the
    // worker.
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
            "Reading the pull request timed out.",
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
                "The GitHub CLI did not finish reading the pull request.",
            ))
        }
    };
    let stderr = match capture.stderr {
        Some(Captured::Complete(bytes)) => bytes,
        _ => Vec::new(),
    };
    let stderr = String::from_utf8_lossy(&stderr).to_string();

    if !status.is_some_and(|status| status.success()) {
        // "no pull requests found for branch" is the ordinary answer for a lane that has not
        // opened one, and it arrives on stderr with a non-zero exit.
        if mentions_no_pull_request(&stderr) {
            return Ok(GhOutcome::NoPullRequest);
        }
        return Err(classify_gh_failure(&stderr, &target_repository_name(args)));
    }

    Ok(GhOutcome::Stdout(
        String::from_utf8_lossy(&stdout).to_string(),
    ))
}

fn mentions_no_pull_request(stderr: &str) -> bool {
    let lowered = stderr.to_ascii_lowercase();
    lowered.contains("no pull requests found")
        || lowered.contains("no open pull requests")
        || lowered.contains("no closed pull requests")
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
                        // Keep draining so the child can still exit; the payload is reported as
                        // oversized instead of silently cut.
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
    if mentions(&["http 404", "could not resolve to a pull request", "not found"]) {
        return IpcError::new(
            IpcErrorCode::NotFound,
            format!("The GitHub CLI could not find that pull request in {repository}."),
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
            "The GitHub CLI failed to read the pull request.",
        );
    }
    IpcError::new(
        IpcErrorCode::InternalError,
        format!("The GitHub CLI failed to read the pull request: {detail}"),
    )
}

const GH_PROGRAM: &str = "gh";

/// Resolves `gh` through the shared search path rather than the process PATH alone: a GUI-launched
/// app inherits a minimal PATH on macOS, where a Homebrew or `~/.local/bin` install would be
/// invisible, while the login shell's PATH is what the user's own terminal resolves against.
fn resolve_gh_program() -> Result<PathBuf, IpcError> {
    resolve_gh_in(&crate::ipc::agents::search_paths())
}

/// Resolves `gh` in an explicit search path list, so the lookup stays testable without touching
/// the process environment or spawning a login shell.
fn resolve_gh_in(search_paths: &[PathBuf]) -> Result<PathBuf, IpcError> {
    crate::ipc::agents::resolve_binary(GH_PROGRAM, search_paths).ok_or_else(|| {
        IpcError::new(
            IpcErrorCode::CliExecutableNotFound,
            "The GitHub CLI (gh) was not found on PATH. Install it and run `gh auth login`.",
        )
    })
}

fn unsupported_origin() -> IpcError {
    IpcError::new(
        IpcErrorCode::Unsupported,
        "Reading a pull request needs a github.com origin, and this project's remote does not point at one.",
    )
}

/// Parses a remote URL into its `owner/repository` pair, rejecting anything that is not a
/// github.com repository.
fn parse_github_origin(remote: &str) -> Result<RepositorySlug, IpcError> {
    let trimmed = remote.trim();
    let path = if trimmed.contains("://") {
        let url = Url::parse(trimmed).map_err(|_| unsupported_origin())?;
        if !matches!(url.scheme(), "ssh" | "git" | "https" | "http")
            || !is_github_host(url.host_str())
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

/// The `owner`, `repository` and `number` of a canonical pull-request URL.
fn parse_pr_url(raw: &str) -> Option<(String, String, u64)> {
    let url = Url::parse(raw.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https") || !is_github_host(url.host_str()) {
        return None;
    }
    let segments: Vec<&str> = url.path().split('/').filter(|part| !part.is_empty()).collect();
    let [owner, repository, kind, number] = segments[..] else {
        return None;
    };
    if kind != "pull" {
        return None;
    }
    let number = number.parse::<u64>().ok()?;
    Some((owner.to_string(), repository.to_string(), number))
}

fn is_github_host(host: Option<&str>) -> bool {
    host.is_some_and(|host| host.eq_ignore_ascii_case("github.com"))
}

/// GitHub owner and repository names are limited to this charset, which is also what keeps them
/// safe to hand to a child process as a single argument.
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

/// Drops control characters so display text cannot carry terminal escapes. Newlines and tabs
/// survive.
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

fn bound_chars(value: &str, max: usize) -> (String, bool) {
    if value.chars().count() <= max {
        return (value.to_string(), false);
    }
    (value.chars().take(max).collect(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(branch: &str) -> PrTarget {
        PrTarget {
            repository: RepositorySlug {
                owner: "acme".to_string(),
                name: "widgets".to_string(),
            },
            head_branch: branch.to_string(),
        }
    }

    /// The one argv this module can build. If a future edit adds a second one, this assertion and
    /// the absence of any other `vec![` argv in the module are what the read-only claim rests on.
    #[test]
    fn gh_arguments_are_a_fixed_read_only_array() {
        assert_eq!(
            gh_pr_view_args(&target("lane/ledger")),
            vec![
                "pr",
                "view",
                "lane/ledger",
                "--repo",
                "acme/widgets",
                "--json",
                "number,state,url,title,headRefName,isDraft"
            ]
        );
    }

    #[test]
    fn the_argument_array_never_carries_a_mutating_pr_subcommand() {
        let args = gh_pr_view_args(&target("lane/ledger"));
        assert_eq!(args[0], "pr");
        assert_eq!(args[1], "view");
        for forbidden in [
            "merge", "close", "create", "edit", "ready", "review", "reopen", "comment", "api",
        ] {
            assert!(
                !args.iter().any(|arg| arg.as_str() == forbidden),
                "{forbidden} appears in {args:?}"
            );
        }
    }

    /// The read-only property, checked by the classifier that gates effects rather than by this
    /// module's own opinion of itself: the one argv this module can build is `Effect::Read`, and
    /// `Effect::VcsProtected` (where `pr merge` lands) is never reached.
    #[test]
    fn the_argument_array_classifies_as_read_in_the_hub_effect_gate() {
        let mut argv = vec!["gh".to_string()];
        argv.extend(gh_pr_view_args(&target("lane/ledger")));
        assert_eq!(
            ferryx_hub::effect::classify_argv(&argv, None).effect,
            ferryx_hub::effect::Effect::Read,
            "{argv:?}"
        );
        // The gate it must not reach: `pr merge` is VcsProtected, so this also proves the
        // assertion above is not trivially true of every `gh` argv.
        let merging = vec![
            "gh".to_string(),
            "pr".to_string(),
            "merge".to_string(),
            "12".to_string(),
        ];
        assert_eq!(
            ferryx_hub::effect::classify_argv(&merging, None).effect,
            ferryx_hub::effect::Effect::VcsProtected
        );
    }

    #[test]
    fn lane_branches_are_accepted() {
        for branch in ["lane/ledger", "lane/a-b_c.1", "feature/x+y"] {
            assert_eq!(validate_branch(branch).unwrap(), branch, "{branch}");
        }
    }

    #[test]
    fn injection_shaped_branches_are_rejected() {
        for branch in [
            "",
            "  ",
            "--repo",
            "-D",
            "lane/..",
            "lane//ledger",
            "/lane",
            "lane/",
            "lane/ledger --repo evil/repo",
            "lane/ledger; touch /tmp/ferryx-pwned",
            "$(touch /tmp/ferryx-pwned)",
            "`touch /tmp/ferryx-pwned`",
            "lane/ledger\n--repo evil/repo",
            "lane/ledg er",
            "lane/légende",
            "lane/lock.lock",
            "main@{1}",
        ] {
            assert_eq!(
                validate_branch(branch).unwrap_err().code,
                IpcErrorCode::InvalidArgument,
                "{branch}"
            );
        }
    }

    #[test]
    fn states_normalize_to_the_projection_names() {
        assert_eq!(normalize_state("OPEN").unwrap(), "open");
        assert_eq!(normalize_state("MERGED").unwrap(), "merged");
        assert_eq!(normalize_state("closed").unwrap(), "closed");
        assert_eq!(
            normalize_state("DRAFT").unwrap_err().code,
            IpcErrorCode::ParseError
        );
    }

    #[test]
    fn github_origins_parse_from_ssh_and_https_forms() {
        for remote in [
            "git@github.com:acme/widgets.git",
            "ssh://git@github.com/acme/widgets.git",
            "https://github.com/acme/widgets.git",
            "https://github.com/acme/widgets",
        ] {
            assert_eq!(
                parse_github_origin(remote).unwrap().full_name(),
                "acme/widgets",
                "{remote}"
            );
        }
    }

    #[test]
    fn non_github_origins_are_unsupported() {
        for remote in [
            "",
            "git@gitlab.com:acme/widgets.git",
            "https://github.com/acme",
            "https://github.com.evil.test/acme/widgets.git",
        ] {
            assert_eq!(
                parse_github_origin(remote).unwrap_err().code,
                IpcErrorCode::Unsupported,
                "{remote}"
            );
        }
    }

    #[test]
    fn pull_request_urls_resolve_to_repository_and_number() {
        assert_eq!(
            parse_pr_url("https://github.com/acme/widgets/pull/12"),
            Some(("acme".to_string(), "widgets".to_string(), 12))
        );
        assert_eq!(parse_pr_url("https://github.com/acme/widgets/issues/12"), None);
        assert_eq!(parse_pr_url("https://gitlab.com/acme/widgets/pull/12"), None);
    }

    /// The hub's sync contract, checked on the classifier the sync itself uses: the four ordinary
    /// "no PR to record" answers are `is_absent`, and the codes that mean the read broke are not.
    ///
    /// Fails if `is_absent` is widened (a timeout would be silently reported as "no PR", and a
    /// lane whose GitHub read is broken would look like a lane that never opened one) or narrowed
    /// (a missing `gh` or a signed-out `gh` would count as a sweep failure on every lane of every
    /// machine without it).
    #[test]
    fn absence_is_told_apart_from_a_broken_read() {
        for absent in [
            IpcErrorCode::CliExecutableNotFound,
            IpcErrorCode::Unauthorized,
            IpcErrorCode::Unsupported,
            IpcErrorCode::NotFound,
        ] {
            assert!(
                is_absent(&IpcError::new(absent.clone(), "absent")),
                "{absent:?} is an ordinary 'no pull request' answer"
            );
        }
        for broken in [
            IpcErrorCode::Timeout,
            IpcErrorCode::OutputLimitExceeded,
            IpcErrorCode::RateLimited,
            IpcErrorCode::ParseError,
            IpcErrorCode::IoError,
            IpcErrorCode::NotAGitRepository,
        ] {
            assert!(
                !is_absent(&IpcError::new(broken.clone(), "broken")),
                "{broken:?} must stay a failure the sweep counts"
            );
        }
    }

    /// A non-GitHub origin is `Unsupported`, which the sync reads as "this lane has no pull
    /// request" rather than a failure: a GitLab lane is not a broken read.
    #[test]
    fn a_non_github_origin_reads_as_absent() {
        let error = parse_github_origin("git@gitlab.com:acme/widgets.git").unwrap_err();
        assert_eq!(error.code, IpcErrorCode::Unsupported);
        assert!(is_absent(&error));
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

        fn fixture_gh(dir: &Path, body: &str, args_log: Option<&Path>) -> PathBuf {
            let mut script = String::new();
            if let Some(log) = args_log {
                script.push_str(&format!("printf '%s\\n' \"$@\" > {}\n", log.display()));
            }
            script.push_str(body);
            write_script(dir, "gh", &script)
        }

        fn pr_json(state: &str, branch: &str) -> String {
            serde_json::json!({
                "number": 12,
                "state": state,
                "url": "https://github.com/acme/widgets/pull/12",
                "title": "Fix the parser",
                "headRefName": branch,
                "isDraft": false,
            })
            .to_string()
        }

        #[test]
        fn reads_a_pull_request_with_a_fixed_argument_array() {
            let dir = tempfile::tempdir().unwrap();
            let log = dir.path().join("args.txt");
            let gh = fixture_gh(
                dir.path(),
                &format!(
                    "cat <<'FERRYX_FIXTURE'\n{}\nFERRYX_FIXTURE\n",
                    pr_json("OPEN", "lane/ledger")
                ),
                Some(&log),
            );
            let pr = fetch_pr(&gh, &target("lane/ledger")).unwrap().unwrap();
            assert_eq!(pr.number, 12);
            assert_eq!(pr.state, "open");
            assert_eq!(pr.head_branch, "lane/ledger");
            assert_eq!(pr.repository, "acme/widgets");
            let args = std::fs::read_to_string(&log).unwrap();
            assert!(!args.contains("merge"));
            assert!(args.starts_with("pr\nview\nlane/ledger\n--repo\nacme/widgets\n"));
        }

        /// The read path is what makes a merge outside Ferryx visible: same command, new state.
        #[test]
        fn a_pr_merged_outside_ferryx_reads_back_as_merged() {
            let dir = tempfile::tempdir().unwrap();
            let gh = fixture_gh(
                dir.path(),
                &format!(
                    "cat <<'FERRYX_FIXTURE'\n{}\nFERRYX_FIXTURE\n",
                    pr_json("MERGED", "lane/ledger")
                ),
                None,
            );
            assert_eq!(
                fetch_pr(&gh, &target("lane/ledger")).unwrap().unwrap().state,
                "merged"
            );
        }

        #[test]
        fn a_branch_without_a_pull_request_is_an_absent_link() {
            let dir = tempfile::tempdir().unwrap();
            let gh = fixture_gh(
                dir.path(),
                "echo 'no pull requests found for branch \"lane/ledger\"' >&2\nexit 1",
                None,
            );
            assert_eq!(fetch_pr(&gh, &target("lane/ledger")).unwrap(), None);
        }

        #[test]
        fn a_pull_request_for_another_branch_is_refused() {
            let dir = tempfile::tempdir().unwrap();
            let gh = fixture_gh(
                dir.path(),
                &format!(
                    "cat <<'FERRYX_FIXTURE'\n{}\nFERRYX_FIXTURE\n",
                    pr_json("OPEN", "lane/other")
                ),
                None,
            );
            assert_eq!(
                fetch_pr(&gh, &target("lane/ledger")).unwrap_err().code,
                IpcErrorCode::ParseError
            );
        }

        #[test]
        fn an_unauthenticated_gh_is_reported_as_unauthorized() {
            let dir = tempfile::tempdir().unwrap();
            let gh = fixture_gh(
                dir.path(),
                "echo 'To get started with GitHub CLI, please run:  gh auth login' >&2\nexit 4",
                None,
            );
            assert_eq!(
                fetch_pr(&gh, &target("lane/ledger")).unwrap_err().code,
                IpcErrorCode::Unauthorized
            );
        }
    }
}
