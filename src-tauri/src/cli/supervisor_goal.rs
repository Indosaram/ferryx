//! `ferryx-cli supervisor goal ...`: show or steer one lane's goal contract through the running
//! daemon. Connect-only like `ferryx-cli agents`: never spawns, upgrades or retries, and prints one
//! JSON value. Exit codes follow `agent_control::exit_code_for` (REQUEST_CONFLICT is 4).
//!
//! `ferryx-cli supervisor setup ...` sets up this installation's supervisor identity, one project
//! and its mapping on one host ([`ferryx_hub::supervisor::provision_owner`]). It writes the hub
//! database the same way `ferryx-cli mcp` opens it, needs no running daemon and prints one JSON
//! value. A conflicting set-up (SETUP_CONFLICT) exits 4 and writes nothing.
//! The git common dir is resolved here, on the host it names: only `--host local` is accepted,
//! the path must be a repository's common dir or a checkout root, and its canonical common dir
//! is what is stored, so two spellings of one checkout are one mapping. A pane worker
//! (FERRYX_SESSION_ID set) is refused: set-up is the owner's act.
//! `--remote` maps the project on a paired host instead (`--host` other than `local`). That
//! path cannot be resolved here, so it is normalized lexically only (`remote_git_common_dir`):
//! absolute, trailing separators stripped, no `.`/`..`/empty components, ending in `.git`
//! (the common dir itself, never a checkout root), and stored as given.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::agent_control::{failure, UsageError, EXIT_INVALID, EXIT_NOT_FOUND, EXIT_OK};
use crate::daemon::protocol::{DaemonRequest, DaemonResponse, SupervisorGoalAction, DAEMON_PROTOCOL_VERSION};
use ferryx_hub::goals::contract::RequirementSpec;
use ferryx_hub::goals::GoalMode;

pub const SUPERVISOR_USAGE: &str = "usage: ferryx-cli supervisor goal set <lane> (--requirement <id>|<text>|<verifier>)... [--requirements-json <json>] [--mode <off|supervised|full>] [--turn-limit <n>] [--json]\n       ferryx-cli supervisor goal show <lane> [--json]\n       ferryx-cli supervisor goal mode <lane> <off|supervised|full> [--json]\n       ferryx-cli supervisor goal approve <lane> [--json]\n       ferryx-cli supervisor goal discard <lane> [--json]\n       ferryx-cli supervisor goal accept-revision <id> [--json]\n       ferryx-cli supervisor goal reject-revision <id> [--json]";

pub const SETUP_USAGE: &str = "usage: ferryx-cli supervisor setup --owner <principal> --project <name> --repo-identity <id> --host <host-id> --git-common-dir <absolute dir> [--remote] [--json]\n  --remote: --host is a paired host (not local) and --git-common-dir is its absolute <repo>/.git, stored as given";

/// The arguments of `ferryx-cli supervisor setup`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupArgs {
    pub owner: String,
    pub project: String,
    pub repo_identity: String,
    pub host: String,
    pub git_common_dir: String,
    /// `--remote`: the host is a paired host; its dir is normalized lexically, not resolved.
    pub remote: bool,
}

/// Parse `ferryx-cli supervisor setup ...`. Every flag is required once; nothing else is accepted.
pub fn parse_setup_cli(args: &[String]) -> Result<SetupArgs, UsageError> {
    if args.get(1).map(String::as_str) != Some("supervisor") || args.get(2).map(String::as_str) != Some("setup") {
        return Err(UsageError(SETUP_USAGE.into()));
    }
    let mut rest: Vec<String> = args.iter().skip(3).filter(|arg| *arg != "--json").cloned().collect();
    let remote = match rest.iter().filter(|arg| *arg == "--remote").count() {
        0 => false,
        1 => true,
        _ => return Err(UsageError(format!("--remote may be given once\n{SETUP_USAGE}"))),
    };
    rest.retain(|arg| arg != "--remote");
    let mut flag = |name: &str| -> Result<String, UsageError> {
        match take_value(&mut rest, name)? {
            Some(value) if !value.trim().is_empty() && !value.starts_with("--") => Ok(value),
            _ => Err(UsageError(format!("{name} is required\n{SETUP_USAGE}"))),
        }
    };
    let parsed = SetupArgs {
        owner: flag("--owner")?,
        project: flag("--project")?,
        repo_identity: flag("--repo-identity")?,
        host: flag("--host")?,
        git_common_dir: flag("--git-common-dir")?,
        remote,
    };
    if let Some(extra) = rest.first() {
        return Err(UsageError(format!("unexpected argument {extra}\n{SETUP_USAGE}")));
    }
    if !std::path::Path::new(&parsed.git_common_dir).is_absolute() {
        return Err(UsageError("--git-common-dir must be an absolute path".into()));
    }
    if parsed.remote && parsed.host == ferryx_hub::agent_control::LOCAL_HOST_ID {
        return Err(UsageError(format!("--remote is for a paired host; this host (local) is resolved without it\n{SETUP_USAGE}")));
    }
    Ok(parsed)
}

/// Exit code of a set-up error code: a conflict is a refusal (4), a bad argument is invalid (2).
fn setup_exit_code(code: &str) -> i32 {
    match code {
        ferryx_hub::supervisor::SETUP_CONFLICT => crate::agent_control::EXIT_REFUSED,
        NOT_A_GIT_DIR | HOST_NOT_LOCAL => EXIT_INVALID,
        other => crate::agent_control::exit_code_for(other),
    }
}

/// `--git-common-dir` does not name an existing git common dir or checkout root (exit 2).
pub const NOT_A_GIT_DIR: &str = "NOT_A_GIT_DIR";
/// `--host` is not this host; its mapping has to be made on that host (exit 2).
pub const HOST_NOT_LOCAL: &str = "HOST_NOT_LOCAL";

/// Set-up is refused inside a Ferryx pane: a worker runs as the same OS user and must not
/// provision the owner's supervisor.
fn refused_in_pane(session_id: Option<&std::ffi::OsStr>) -> bool {
    session_id.is_some_and(|session| !session.is_empty())
}

fn canonical(path: &Path) -> std::io::Result<PathBuf> {
    let resolved = std::fs::canonicalize(path)?;
    Ok(PathBuf::from(crate::worktree::git::strip_verbatim_prefix(&resolved.to_string_lossy())))
}

/// One `git rev-parse <flag>` in `dir`, free of any inherited GIT_DIR-style override.
fn rev_parse(dir: &Path, flag: &str) -> Option<String> {
    let output = crate::util::no_window_command("git")
        .args(["rev-parse", flag])
        .current_dir(dir)
        .env_remove("GIT_DIR")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let text = text.trim_end_matches(['\r', '\n']);
    (!text.is_empty()).then(|| text.to_owned())
}

/// The canonical absolute git common dir `dir` names on `host`. Only this host (`local`) can
/// resolve one. Accepted spellings are the common dir itself (`/r/.git`, `/r/.git/`, a symlink
/// to it) and a checkout root (`/r`, a linked worktree's root); a path inside a repository or
/// outside any is refused.
pub fn canonical_git_common_dir(host: &str, dir: &str) -> Result<String, (&'static str, String)> {
    let local = ferryx_hub::agent_control::LOCAL_HOST_ID;
    if host != local {
        return Err((
            HOST_NOT_LOCAL,
            format!("host {host} is not this host ({local}); its git common dir can only be resolved there, so run `ferryx-cli supervisor setup --host {local}` on that host"),
        ));
    }
    let refuse = |why: String| (NOT_A_GIT_DIR, format!("{dir} is not a git common dir or checkout root: {why}"));
    let given = canonical(Path::new(dir)).map_err(|error| refuse(error.to_string()))?;
    if !given.is_dir() {
        return Err(refuse("not a directory".into()));
    }
    let common = rev_parse(&given, "--git-common-dir").ok_or_else(|| refuse("git rev-parse --git-common-dir failed".into()))?;
    let common = Path::new(&common);
    let common = canonical(&if common.is_absolute() { common.to_path_buf() } else { given.join(common) })
        .map_err(|error| refuse(error.to_string()))?;
    let checkout_root = || {
        rev_parse(&given, "--show-toplevel")
            .and_then(|top| canonical(Path::new(&top)).ok())
            .is_some_and(|top| top == given)
    };
    if given != common && !checkout_root() {
        return Err(refuse(format!("it is inside the repository whose common dir is {}", common.display())));
    }
    common.to_str().map(str::to_owned).ok_or_else(|| refuse("the path is not UTF-8".into()))
}

/// The git common dir `dir` names on paired host `host`, normalized lexically: it cannot be
/// resolved from this machine. Absolute, trailing separators stripped, and refused with
/// NOT_A_GIT_DIR when any component is empty, `.` or `..`, or the last one is not `.git` (the
/// common dir itself, not a checkout root). `local` is resolved by [`canonical_git_common_dir`].
pub fn remote_git_common_dir(host: &str, dir: &str) -> Result<String, (&'static str, String)> {
    if host == ferryx_hub::agent_control::LOCAL_HOST_ID {
        return Err(("INVALID_REQUEST", "a remote mapping names a paired host, not local".into()));
    }
    let refuse = |why: &str| (NOT_A_GIT_DIR, format!("{dir} is not an absolute <repo>/.git path on host {host}: {why}"));
    if !Path::new(dir).is_absolute() {
        return Err(refuse("it is not absolute"));
    }
    let separators: &[char] = if cfg!(windows) { &['/', '\\'] } else { &['/'] };
    let trimmed = dir.trim_end_matches(separators);
    let components: Vec<&str> = trimmed.split(separators).collect();
    // The first component is the root (empty on POSIX, the drive on Windows).
    if components.iter().skip(1).any(|part| part.is_empty() || *part == "." || *part == "..") {
        return Err(refuse("it has an empty, `.` or `..` component"));
    }
    if components.len() < 2 || components.last() != Some(&".git") {
        return Err(refuse("it does not end in a .git component"));
    }
    Ok(trimmed.to_owned())
}

async fn run_setup(setup: SetupArgs) -> (Value, i32) {
    // Resolved before the database is opened: a refused path writes and migrates nothing.
    let resolved = if setup.remote {
        remote_git_common_dir(&setup.host, &setup.git_common_dir)
    } else {
        canonical_git_common_dir(&setup.host, &setup.git_common_dir)
    };
    let git_common_dir = match resolved {
        Ok(dir) => dir,
        Err((code, message)) => {
            let (value, _) = failure(code, &message);
            return (value, setup_exit_code(code));
        }
    };
    // The same database `ferryx-cli mcp` and the daemon's hub open: the config file path wins,
    // then FERRYX_HUB_DB, then the default.
    let config = match ferryx_hub::HubConfig::load() {
        Ok(config) => config,
        Err(error) => return failure("INVALID_REQUEST", &format!("hub config: {error}")),
    };
    let path = config.database_path.clone().unwrap_or_else(ferryx_hub::hub::default_database_path);
    if let Some(parent) = path.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            return failure("STORAGE", &format!("cannot create {}: {error}", parent.display()));
        }
    }
    let pool = match ferryx_hub::storage::connect(&path).await {
        Ok(pool) => pool,
        Err(error) => return failure("STORAGE", &error.to_string()),
    };
    let provisioned = ferryx_hub::supervisor::provision_owner(
        &pool,
        &setup.owner,
        &setup.project,
        &setup.repo_identity,
        &setup.host,
        &git_common_dir,
    )
    .await;
    pool.close().await;
    match provisioned {
        Ok((identity, project, host)) => (
            serde_json::json!({
                "ok": true,
                "supervisorId": identity.supervisor_id,
                "ownerPrincipal": identity.owner_principal,
                "projectId": project.project_id,
                "projectName": project.name,
                "repoIdentity": project.repo_identity,
                "hostId": host.host_id,
                "gitCommonDir": host.git_common_dir,
            }),
            EXIT_OK,
        ),
        Err(error) => {
            let code = error.code();
            let (value, _) = failure(code, &error.to_string());
            (value, setup_exit_code(code))
        }
    }
}

/// Run `ferryx-cli supervisor setup ...`: print one JSON value on stdout and return the exit code.
fn run_setup_cli(args: &[String]) -> i32 {
    let (value, code) = match parse_setup_cli(args) {
        _ if refused_in_pane(std::env::var_os("FERRYX_SESSION_ID").as_deref()) => {
            let (value, _) = failure("FORBIDDEN", "supervisor setup is the owner's act and is refused inside a Ferryx pane (FERRYX_SESSION_ID is set)");
            (value, crate::agent_control::EXIT_REFUSED)
        }
        Err(UsageError(message)) => {
            let (value, _) = failure("INVALID_REQUEST", &message);
            (value, EXIT_INVALID)
        }
        Ok(setup) => match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(runtime) => runtime.block_on(run_setup(setup)),
            Err(error) => failure("INTERNAL", &error.to_string()),
        },
    };
    println!("{value}");
    code
}

/// How long one goal action may take: an approve waits for the pane to start the turn.
pub(crate) const GOAL_CLI_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(180);

fn take_values(args: &mut Vec<String>, name: &str) -> Result<Vec<String>, UsageError> {
    let mut values = Vec::new();
    while let Some(index) = args.iter().position(|arg| arg == name) {
        if index + 1 >= args.len() {
            return Err(UsageError(format!("{name} needs a value")));
        }
        values.push(args.remove(index + 1));
        args.remove(index);
    }
    Ok(values)
}

fn take_value(args: &mut Vec<String>, name: &str) -> Result<Option<String>, UsageError> {
    let mut values = take_values(args, name)?;
    if values.len() > 1 {
        return Err(UsageError(format!("{name} may be given once")));
    }
    Ok(values.pop())
}

fn lane_arg(rest: &[String]) -> Result<String, UsageError> {
    let lane = rest.first().ok_or_else(|| UsageError("a lane key is required".into()))?;
    if lane.trim().is_empty() || lane.starts_with("--") {
        return Err(UsageError(format!("not a lane key: {lane}")));
    }
    Ok(lane.clone())
}

fn id_arg(rest: &[String]) -> Result<i64, UsageError> {
    rest.first()
        .ok_or_else(|| UsageError("a revision id is required".into()))?
        .parse::<i64>()
        .map_err(|_| UsageError("the revision id must be a number".into()))
}

fn mode_arg(text: &str) -> Result<GoalMode, UsageError> {
    text.parse::<GoalMode>()
        .map_err(|_| UsageError("mode must be off, supervised or full".into()))
}

fn requirement_arg(text: &str) -> Result<RequirementSpec, UsageError> {
    let mut parts = text.splitn(3, '|');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(id), Some(requirement), Some(verifier))
            if !id.trim().is_empty() && !requirement.trim().is_empty() && !verifier.trim().is_empty() =>
        {
            Ok(RequirementSpec {
                id: id.trim().to_owned(),
                text: requirement.trim().to_owned(),
                verifier: verifier.trim().to_owned(),
            })
        }
        _ => Err(UsageError(format!("--requirement must be <id>|<text>|<verifier>: {text}"))),
    }
}

/// Parse `ferryx-cli supervisor goal ...` (`args[0]` is the binary, `args[1]` is `supervisor`).
pub fn parse_supervisor_cli(args: &[String]) -> Result<SupervisorGoalAction, UsageError> {
    if args.get(1).map(String::as_str) != Some("supervisor") || args.get(2).map(String::as_str) != Some("goal") {
        return Err(UsageError(SUPERVISOR_USAGE.into()));
    }
    let command = args.get(3).cloned().ok_or_else(|| UsageError(SUPERVISOR_USAGE.into()))?;
    // JSON is the only output format; --json is accepted so callers can state it explicitly.
    let mut rest: Vec<String> = args.iter().skip(4).filter(|arg| *arg != "--json").cloned().collect();
    let action = match command.as_str() {
        "set" => {
            let mut requirements = take_values(&mut rest, "--requirement")?
                .iter()
                .map(|text| requirement_arg(text))
                .collect::<Result<Vec<_>, _>>()?;
            if let Some(json) = take_value(&mut rest, "--requirements-json")? {
                let parsed: Vec<RequirementSpec> = serde_json::from_str(&json)
                    .map_err(|error| UsageError(format!("--requirements-json: {error}")))?;
                requirements.extend(parsed);
            }
            if requirements.is_empty() {
                return Err(UsageError("give at least one --requirement or --requirements-json".into()));
            }
            let mode = take_value(&mut rest, "--mode")?.map(|mode| mode_arg(&mode)).transpose()?;
            let turn_limit = take_value(&mut rest, "--turn-limit")?
                .map(|limit| match limit.parse::<u32>() {
                    Ok(limit) if limit > 0 => Ok(limit),
                    _ => Err(UsageError("--turn-limit must be a positive number".into())),
                })
                .transpose()?;
            SupervisorGoalAction::Set { lane_key: lane_arg(&rest)?, requirements, mode, turn_limit }
        }
        "show" => SupervisorGoalAction::Show { lane_key: lane_arg(&rest)? },
        "mode" => {
            let lane_key = lane_arg(&rest)?;
            let mode = rest.get(1).ok_or_else(|| UsageError("a mode is required".into()))?;
            SupervisorGoalAction::Mode { lane_key, mode: mode_arg(mode)? }
        }
        "approve" => SupervisorGoalAction::Approve { lane_key: lane_arg(&rest)? },
        "discard" => SupervisorGoalAction::Discard { lane_key: lane_arg(&rest)? },
        "accept-revision" => SupervisorGoalAction::AcceptRevision { id: id_arg(&rest)? },
        "reject-revision" => SupervisorGoalAction::RejectRevision { id: id_arg(&rest)? },
        other => return Err(UsageError(format!("unknown goal command {other}\n{SUPERVISOR_USAGE}"))),
    };
    Ok(action)
}

/// The JSON and exit code a daemon reply maps to.
pub fn outcome_of(response: DaemonResponse) -> (Value, i32) {
    match response {
        DaemonResponse::SupervisorGoalOk { snapshot } => {
            let mut value = serde_json::json!({ "ok": true });
            if let (Some(out), Value::Object(fields)) = (value.as_object_mut(), snapshot) {
                out.extend(fields);
            }
            (value, EXIT_OK)
        }
        DaemonResponse::Error { message, code, .. } => {
            let code = code.unwrap_or_default();
            failure(if code.is_empty() { "INTERNAL" } else { &code }, &message)
        }
        other => failure("INTERNAL", &format!("unexpected daemon response: {other:?}")),
    }
}

#[cfg(unix)]
pub(super) async fn connect_daemon() -> Result<tokio::net::UnixStream, (Value, i32)> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    let path = crate::daemon::server::get_socket_path();
    let not_running = || {
        let (value, _) = failure("NOT_FOUND", &format!("no Ferryx daemon is listening at {}", path.display()));
        (value, EXIT_NOT_FOUND)
    };
    let metadata = std::fs::symlink_metadata(&path).map_err(|_| not_running())?;
    // SAFETY: getuid takes no arguments, dereferences nothing and cannot fail.
    let uid = unsafe { libc::getuid() };
    if !metadata.file_type().is_socket() || metadata.uid() != uid {
        return Err(failure("FORBIDDEN", "the daemon socket is not a socket owned by this user"));
    }
    tokio::net::UnixStream::connect(&path).await.map_err(|_| not_running())
}

#[cfg(not(unix))]
pub(super) async fn connect_daemon() -> Result<tokio::net::TcpStream, (Value, i32)> {
    let path = crate::daemon::server::get_socket_path();
    let not_running = || {
        let (value, _) = failure("NOT_FOUND", "no Ferryx daemon is listening");
        (value, EXIT_NOT_FOUND)
    };
    let port: u16 = tokio::fs::read_to_string(&path)
        .await
        .ok()
        .and_then(|text| text.trim().parse().ok())
        .ok_or_else(not_running)?;
    tokio::net::TcpStream::connect(("127.0.0.1", port)).await.map_err(|_| not_running())
}

/// Send one goal action to the running daemon, gated on `supervisorGoalV1` so an older daemon
/// never receives a request it cannot parse. A lost answer to a mutation is
/// OPERATION_OUTCOME_UNKNOWN and is not sent again.
pub async fn request_goal(action: SupervisorGoalAction) -> (Value, i32) {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let stream = match connect_daemon().await {
        Ok(stream) => stream,
        Err(failed) => return failed,
    };
    let (read_half, mut write_half) = tokio::io::split(stream);
    let mut reader = BufReader::new(read_half);
    async fn exchange<R, W>(reader: &mut BufReader<R>, writer: &mut W, request: &DaemonRequest) -> Result<DaemonResponse, String>
    where
        R: tokio::io::AsyncRead + Unpin,
        W: tokio::io::AsyncWrite + Unpin,
    {
        let mut line = serde_json::to_string(request).map_err(|error| error.to_string())?;
        line.push('\n');
        writer.write_all(line.as_bytes()).await.map_err(|error| error.to_string())?;
        writer.flush().await.map_err(|error| error.to_string())?;
        let mut response = String::new();
        if reader.read_line(&mut response).await.map_err(|error| error.to_string())? == 0 {
            return Err("the daemon closed the connection".into());
        }
        serde_json::from_str(response.trim()).map_err(|error| format!("unreadable daemon response: {error}"))
    }
    let handshake = DaemonRequest::Handshake {
        version: DAEMON_PROTOCOL_VERSION,
        token: crate::daemon::client::read_transport_token(),
    };
    match exchange(&mut reader, &mut write_half, &handshake).await {
        Ok(DaemonResponse::HandshakeOk { .. }) => {}
        Ok(DaemonResponse::ProtocolMismatch { .. }) => return failure("UNSUPPORTED", "the daemon speaks another protocol version"),
        Ok(other) => return failure("UNSUPPORTED", &format!("unexpected handshake response: {other:?}")),
        Err(message) => return failure("INTERNAL", &message),
    }
    let required = crate::hub_runtime::SUPERVISOR_GOAL_CAPABILITY;
    match exchange(&mut reader, &mut write_half, &DaemonRequest::GetCapabilities).await {
        Ok(DaemonResponse::CapabilitiesOk { capabilities }) if capabilities.iter().any(|known| known == required) => {}
        Ok(DaemonResponse::CapabilitiesOk { .. }) => {
            return failure("UNSUPPORTED", &format!("this daemon does not advertise {required}"))
        }
        Ok(other) => return failure("INTERNAL", &format!("unexpected capabilities response: {other:?}")),
        Err(message) => return failure("INTERNAL", &message),
    }
    let mutates = action.mutates();
    match exchange(&mut reader, &mut write_half, &DaemonRequest::SupervisorGoal { request: action }).await {
        Ok(response) => outcome_of(response),
        Err(message) if mutates => failure("OPERATION_OUTCOME_UNKNOWN", &message),
        Err(message) => failure("INTERNAL", &message),
    }
}

/// Run `ferryx-cli supervisor ...`: print one JSON value on stdout and return the exit code.
pub fn run_supervisor_cli(args: &[String]) -> i32 {
    if args.get(2).map(String::as_str) == Some("setup") {
        return run_setup_cli(args);
    }
    if matches!(args.get(2).map(String::as_str), Some("submit" | "dispatch")) {
        return super::supervisor_dispatch::run_dispatch_cli(args);
    }
    if args.get(2).map(String::as_str) == Some("request") {
        return super::supervisor_request::run_request_cli(args);
    }
    let (value, code) = match parse_supervisor_cli(args) {
        Err(UsageError(message)) => {
            let (value, _) = failure("INVALID_REQUEST", &message);
            (value, EXIT_INVALID)
        }
        Ok(action) => {
            let mutates = action.mutates();
            match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(runtime) => runtime.block_on(async {
                    match tokio::time::timeout(GOAL_CLI_TIMEOUT, request_goal(action)).await {
                        Ok(result) => result,
                        Err(_) if mutates => failure(
                            "OPERATION_OUTCOME_UNKNOWN",
                            "the daemon did not answer before the wait budget elapsed; the outcome is unknown",
                        ),
                        Err(_) => failure("TIMEOUT", "the daemon did not answer before the wait budget elapsed"),
                    }
                }),
                Err(error) => failure("INTERNAL", &error.to_string()),
            }
        }
    };
    println!("{value}");
    code
}

/// One request to the running daemon after a handshake and a `capability` check, for the
/// `supervisor request` subcommand. A lost answer to a mutation is OPERATION_OUTCOME_UNKNOWN
/// and is never sent again.
pub(crate) async fn exchange_once(
    capability: &str,
    request: DaemonRequest,
    mutates: bool,
) -> Result<DaemonResponse, (Value, i32)> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let stream = connect_daemon().await?;
    let (read_half, mut write_half) = tokio::io::split(stream);
    let mut reader = BufReader::new(read_half);
    async fn send<R, W>(reader: &mut BufReader<R>, writer: &mut W, request: &DaemonRequest) -> Result<DaemonResponse, String>
    where
        R: tokio::io::AsyncRead + Unpin,
        W: tokio::io::AsyncWrite + Unpin,
    {
        let mut line = serde_json::to_string(request).map_err(|error| error.to_string())?;
        line.push('\n');
        writer.write_all(line.as_bytes()).await.map_err(|error| error.to_string())?;
        writer.flush().await.map_err(|error| error.to_string())?;
        let mut response = String::new();
        if reader.read_line(&mut response).await.map_err(|error| error.to_string())? == 0 {
            return Err("the daemon closed the connection".into());
        }
        serde_json::from_str(response.trim()).map_err(|error| format!("unreadable daemon response: {error}"))
    }
    let handshake = DaemonRequest::Handshake {
        version: DAEMON_PROTOCOL_VERSION,
        token: crate::daemon::client::read_transport_token(),
    };
    match send(&mut reader, &mut write_half, &handshake).await {
        Ok(DaemonResponse::HandshakeOk { .. }) => {}
        Ok(DaemonResponse::ProtocolMismatch { .. }) => {
            return Err(failure("UNSUPPORTED", "the daemon speaks another protocol version"))
        }
        Ok(other) => return Err(failure("UNSUPPORTED", &format!("unexpected handshake response: {other:?}"))),
        Err(message) => return Err(failure("INTERNAL", &message)),
    }
    match send(&mut reader, &mut write_half, &DaemonRequest::GetCapabilities).await {
        Ok(DaemonResponse::CapabilitiesOk { capabilities }) if capabilities.iter().any(|known| known == capability) => {}
        Ok(DaemonResponse::CapabilitiesOk { .. }) => {
            return Err(failure("UNSUPPORTED", &format!("this daemon does not advertise {capability}")))
        }
        Ok(other) => return Err(failure("INTERNAL", &format!("unexpected capabilities response: {other:?}"))),
        Err(message) => return Err(failure("INTERNAL", &message)),
    }
    match send(&mut reader, &mut write_half, &request).await {
        Ok(response) => Ok(response),
        Err(message) if mutates => Err(failure("OPERATION_OUTCOME_UNKNOWN", &message)),
        Err(message) => Err(failure("INTERNAL", &message)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::EXIT_REFUSED;

    fn argv(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| (*word).to_owned()).collect()
    }

    #[test]
    fn the_cli_parses_every_goal_command() {
        assert_eq!(
            parse_supervisor_cli(&argv(&[
                "ferryx-cli", "supervisor", "goal", "set", "ledger",
                "--requirement", "r1|tests pass|cargo test",
                "--requirement", "r2|docs updated|git diff docs",
                "--mode", "full", "--turn-limit", "5", "--json",
            ]))
            .unwrap(),
            SupervisorGoalAction::Set {
                lane_key: "ledger".into(),
                requirements: vec![
                    RequirementSpec { id: "r1".into(), text: "tests pass".into(), verifier: "cargo test".into() },
                    RequirementSpec { id: "r2".into(), text: "docs updated".into(), verifier: "git diff docs".into() },
                ],
                mode: Some(GoalMode::Full),
                turn_limit: Some(5),
            }
        );
        assert_eq!(
            parse_supervisor_cli(&argv(&[
                "ferryx-cli", "supervisor", "goal", "set", "ledger",
                "--requirements-json", r#"[{"id":"r1","text":"tests pass","verifier":"cargo test"}]"#,
            ]))
            .unwrap(),
            SupervisorGoalAction::Set {
                lane_key: "ledger".into(),
                requirements: vec![RequirementSpec { id: "r1".into(), text: "tests pass".into(), verifier: "cargo test".into() }],
                mode: None,
                turn_limit: None,
            }
        );
        assert_eq!(
            parse_supervisor_cli(&argv(&["ferryx-cli", "supervisor", "goal", "show", "ledger", "--json"])).unwrap(),
            SupervisorGoalAction::Show { lane_key: "ledger".into() }
        );
        assert_eq!(
            parse_supervisor_cli(&argv(&["ferryx-cli", "supervisor", "goal", "mode", "ledger", "supervised"])).unwrap(),
            SupervisorGoalAction::Mode { lane_key: "ledger".into(), mode: GoalMode::Supervised }
        );
        assert_eq!(
            parse_supervisor_cli(&argv(&["ferryx-cli", "supervisor", "goal", "approve", "ledger"])).unwrap(),
            SupervisorGoalAction::Approve { lane_key: "ledger".into() }
        );
        assert_eq!(
            parse_supervisor_cli(&argv(&["ferryx-cli", "supervisor", "goal", "discard", "ledger"])).unwrap(),
            SupervisorGoalAction::Discard { lane_key: "ledger".into() }
        );
        assert_eq!(
            parse_supervisor_cli(&argv(&["ferryx-cli", "supervisor", "goal", "accept-revision", "3"])).unwrap(),
            SupervisorGoalAction::AcceptRevision { id: 3 }
        );
        assert_eq!(
            parse_supervisor_cli(&argv(&["ferryx-cli", "supervisor", "goal", "reject-revision", "4", "--json"])).unwrap(),
            SupervisorGoalAction::RejectRevision { id: 4 }
        );
    }

    #[test]
    fn the_cli_refuses_bad_input_before_contacting_a_daemon() {
        for words in [
            &["ferryx-cli", "supervisor"][..],
            &["ferryx-cli", "supervisor", "goal"],
            &["ferryx-cli", "supervisor", "goal", "frobnicate", "ledger"],
            &["ferryx-cli", "supervisor", "goal", "show"],
            &["ferryx-cli", "supervisor", "goal", "set", "ledger"],
            &["ferryx-cli", "supervisor", "goal", "set", "ledger", "--requirement", "only-an-id"],
            &["ferryx-cli", "supervisor", "goal", "set", "ledger", "--requirement", "r1|t|v", "--turn-limit", "0"],
            &["ferryx-cli", "supervisor", "goal", "set", "ledger", "--requirement", "r1|t|v", "--mode", "turbo"],
            &["ferryx-cli", "supervisor", "goal", "mode", "ledger"],
            &["ferryx-cli", "supervisor", "goal", "mode", "ledger", "turbo"],
            &["ferryx-cli", "supervisor", "goal", "accept-revision", "abc"],
            &["ferryx-cli", "supervisor", "goal", "reject-revision"],
        ] {
            assert!(parse_supervisor_cli(&argv(words)).is_err(), "{words:?} should be refused");
        }
    }

    #[test]
    fn the_setup_cli_needs_every_flag_once_and_an_absolute_dir() {
        let full = [
            "ferryx-cli", "supervisor", "setup", "--owner", "qa-owner", "--project", "qa-supervisor",
            "--repo-identity", "qa:qa-supervisor", "--host", "local", "--git-common-dir", "/work/qa/.git", "--json",
        ];
        assert_eq!(
            parse_setup_cli(&argv(&full)).unwrap(),
            SetupArgs {
                owner: "qa-owner".into(),
                project: "qa-supervisor".into(),
                repo_identity: "qa:qa-supervisor".into(),
                host: "local".into(),
                git_common_dir: "/work/qa/.git".into(),
                remote: false,
            }
        );
        let without = |flag: &str| -> Vec<String> {
            let index = full.iter().position(|word| *word == flag).unwrap();
            full.iter().enumerate().filter(|(i, _)| *i != index && *i != index + 1).map(|(_, w)| (*w).to_owned()).collect()
        };
        for flag in ["--owner", "--project", "--repo-identity", "--host", "--git-common-dir"] {
            assert!(parse_setup_cli(&without(flag)).is_err(), "{flag} is required");
        }
        let mut twice = argv(&full);
        twice.extend(["--host".to_owned(), "other".to_owned()]);
        assert!(parse_setup_cli(&twice).is_err(), "a flag given twice is refused");
        let mut extra = argv(&full);
        extra.push("stray".into());
        assert!(parse_setup_cli(&extra).is_err(), "an unknown argument is refused");
        let relative: Vec<String> = full.iter().map(|w| if *w == "/work/qa/.git" { "work/.git".to_owned() } else { (*w).to_owned() }).collect();
        assert!(parse_setup_cli(&relative).is_err(), "a relative dir is refused");
        assert_eq!(setup_exit_code(ferryx_hub::supervisor::SETUP_CONFLICT), EXIT_REFUSED);
        assert_eq!(setup_exit_code("INVALID_REQUEST"), EXIT_INVALID);
        assert_eq!(setup_exit_code(NOT_A_GIT_DIR), EXIT_INVALID);
        assert_eq!(setup_exit_code(HOST_NOT_LOCAL), EXIT_INVALID);
        assert!(refused_in_pane(Some(std::ffi::OsStr::new("pane-1"))), "a pane worker never provisions");
        assert!(!refused_in_pane(Some(std::ffi::OsStr::new(""))));
        assert!(!refused_in_pane(None));
    }

    fn git_repo(root: &Path) -> PathBuf {
        let repo = root.join("r");
        std::fs::create_dir_all(&repo).unwrap();
        crate::worktree::git::run_git(&repo, &["init", "-q"]).unwrap();
        canonical(&repo).unwrap()
    }

    /// Every spelling of one checkout resolves to one canonical common dir, so a second project
    /// on another spelling is SETUP_CONFLICT, never a second mapping of the same checkout.
    #[tokio::test]
    async fn setup_spellings_of_one_checkout_are_one_mapping() {
        let temp = tempfile::TempDir::new().unwrap();
        let repo = git_repo(temp.path());
        let common = repo.join(".git").to_string_lossy().into_owned();
        let mut spellings = vec![
            common.clone(),
            format!("{common}{}", std::path::MAIN_SEPARATOR),
            repo.to_string_lossy().into_owned(),
        ];
        #[cfg(unix)]
        {
            let link = temp.path().join("link");
            std::os::unix::fs::symlink(&repo, &link).unwrap();
            spellings.push(link.join(".git").to_string_lossy().into_owned());
            spellings.push(link.to_string_lossy().into_owned());
        }
        for spelling in &spellings {
            assert_eq!(canonical_git_common_dir("local", spelling).unwrap(), common, "{spelling}");
        }

        let pool = ferryx_hub::storage::memory().await.unwrap();
        let first = canonical_git_common_dir("local", &spellings[0]).unwrap();
        let (_, project, _) =
            ferryx_hub::supervisor::provision_owner(&pool, "qa-owner", "a", "id-a", "local", &first).await.unwrap();
        for (index, spelling) in spellings.iter().enumerate().skip(1) {
            let dir = canonical_git_common_dir("local", spelling).unwrap();
            let refused = ferryx_hub::supervisor::provision_owner(
                &pool,
                "qa-owner",
                &format!("b{index}"),
                &format!("id-b{index}"),
                "local",
                &dir,
            )
            .await
            .unwrap_err();
            assert_eq!(refused.code(), ferryx_hub::supervisor::SETUP_CONFLICT, "{spelling}: {refused}");
        }
        let last = canonical_git_common_dir("local", spellings.last().unwrap()).unwrap();
        let again = ferryx_hub::supervisor::provision_owner(&pool, "qa-owner", "a", "id-a", "local", &last)
            .await
            .unwrap();
        assert_eq!(again.1, project, "the same project through another spelling is the same mapping");
    }

    /// `--remote` maps a paired host's `<repo>/.git` lexically: accepted for a paired host id,
    /// refused for local, relative, `..` and non-.git paths; a trailing separator is the same
    /// mapping, so a second project on it is SETUP_CONFLICT.
    #[tokio::test]
    async fn setup_remote_form_maps_a_paired_host_lexically() {
        let paired = "relay:http://127.0.0.1:8787#machine-1";
        let base = [
            "ferryx-cli", "supervisor", "setup", "--owner", "qa-owner", "--project", "qa-supervisor",
            "--repo-identity", "qa:qa-supervisor", "--git-common-dir", "/r/.git", "--json",
        ];
        let with = |host: &str, remote: bool| -> Vec<String> {
            let mut words = argv(&base);
            words.extend(["--host".to_owned(), host.to_owned()]);
            if remote {
                words.push("--remote".into());
            }
            words
        };
        let parsed = parse_setup_cli(&with(paired, true)).unwrap();
        assert!(parsed.remote && parsed.host == paired);
        assert!(!parse_setup_cli(&with(paired, false)).unwrap().remote, "without --remote the local path runs (HOST_NOT_LOCAL)");
        assert!(parse_setup_cli(&with("local", true)).is_err(), "--remote with host local is a usage error");
        let mut twice = with(paired, true);
        twice.push("--remote".into());
        assert!(parse_setup_cli(&twice).is_err());
        assert_eq!(canonical_git_common_dir(paired, "/r/.git").unwrap_err().0, HOST_NOT_LOCAL, "the local path still refuses a paired host");

        assert_eq!(remote_git_common_dir(paired, "/r/.git").unwrap(), "/r/.git");
        assert_eq!(remote_git_common_dir(paired, "/r/.git/").unwrap(), "/r/.git");
        assert_eq!(remote_git_common_dir(paired, "/r/.git//").unwrap(), "/r/.git");
        assert!(remote_git_common_dir("local", "/r/.git").is_err());
        for refused in ["r/.git", "/r/../r/.git", "/r/./.git", "/r//.git", "/r", "/r/", "/r/.git/objects", "/.git.bak", "/"] {
            assert_eq!(remote_git_common_dir(paired, refused).unwrap_err().0, NOT_A_GIT_DIR, "{refused}");
        }

        let pool = ferryx_hub::storage::memory().await.unwrap();
        let first = remote_git_common_dir(paired, "/r/.git").unwrap();
        let (_, project, host) =
            ferryx_hub::supervisor::provision_owner(&pool, "qa-owner", "a", "id-a", paired, &first).await.unwrap();
        assert_eq!((host.host_id.as_str(), host.git_common_dir.as_str()), (paired, "/r/.git"));
        let respelled = remote_git_common_dir(paired, "/r/.git/").unwrap();
        let again = ferryx_hub::supervisor::provision_owner(&pool, "qa-owner", "a", "id-a", paired, &respelled).await.unwrap();
        assert_eq!(again.1, project, "the trailing separator is the same mapping");
        let refused = ferryx_hub::supervisor::provision_owner(&pool, "qa-owner", "b", "id-b", paired, &respelled)
            .await
            .unwrap_err();
        assert_eq!(refused.code(), ferryx_hub::supervisor::SETUP_CONFLICT, "{refused}");
    }

    #[test]
    fn setup_refuses_missing_non_git_inner_and_remote_paths() {
        let temp = tempfile::TempDir::new().unwrap();
        let missing = temp.path().join("missing").to_string_lossy().into_owned();
        assert_eq!(canonical_git_common_dir("local", &missing).unwrap_err().0, NOT_A_GIT_DIR);
        let plain = temp.path().join("plain");
        std::fs::create_dir_all(&plain).unwrap();
        assert_eq!(canonical_git_common_dir("local", &plain.to_string_lossy()).unwrap_err().0, NOT_A_GIT_DIR);
        let repo = git_repo(temp.path());
        let inner = repo.join("src");
        std::fs::create_dir_all(&inner).unwrap();
        assert_eq!(canonical_git_common_dir("local", &inner.to_string_lossy()).unwrap_err().0, NOT_A_GIT_DIR);
        let objects = repo.join(".git").join("objects");
        assert_eq!(canonical_git_common_dir("local", &objects.to_string_lossy()).unwrap_err().0, NOT_A_GIT_DIR);
        let remote = canonical_git_common_dir("host-b", &repo.join(".git").to_string_lossy()).unwrap_err();
        assert_eq!(remote.0, HOST_NOT_LOCAL, "{}", remote.1);
    }

    #[test]
    fn a_request_conflict_exits_four_and_a_goal_reply_exits_zero() {
        assert_eq!(crate::agent_control::exit_code_for("REQUEST_CONFLICT"), EXIT_REFUSED);
        let (value, code) = outcome_of(DaemonResponse::Error {
            message: "lane ledger has no proposal awaiting approval".into(),
            code: Some("REQUEST_CONFLICT".into()),
            details: None,
        });
        assert_eq!(code, EXIT_REFUSED);
        assert_eq!(code, 4);
        assert_eq!(value["code"], "REQUEST_CONFLICT");
        assert_eq!(value["ok"], false);
        let (value, code) = outcome_of(DaemonResponse::SupervisorGoalOk {
            snapshot: serde_json::json!({"goal": {"laneKey": "ledger", "goalContract": {"requirements": []}}}),
        });
        assert_eq!(code, EXIT_OK);
        assert_eq!(value["ok"], true);
        assert_eq!(value["goal"]["laneKey"], "ledger");
        let (_, code) = outcome_of(DaemonResponse::Error { message: "x".into(), code: Some("NOT_FOUND".into()), details: None });
        assert_eq!(code, EXIT_NOT_FOUND);
    }
}
