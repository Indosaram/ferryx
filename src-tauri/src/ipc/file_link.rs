//! Local desktop terminal file links.
//!
//! Terminal panes surface tokens such as `src/app.rs:42:10`. This module turns
//! such a token into a launch plan for the local desktop: either the OS opener
//! or a specific editor (VS Code, Cursor, Zed) positioned at line/column.
//!
//! Design rules:
//! - Everything except the final process/URL launch is a pure function, so the
//!   contract is testable without touching the user's desktop applications.
//! - Arguments are always passed as argv (never a shell string), so spaces,
//!   `&`, `$(...)`, Windows drive letters and UNC prefixes survive verbatim.
//! - Paths that belong to a remote host (SSH workspace, paired-host relay
//!   session, `scheme://` or `user@host:` specs) are refused instead of being
//!   opened against an unrelated local file of the same name.
use crate::daemon::protocol::DaemonSessionDetails;
use crate::ipc::error::{IpcError, IpcErrorCode};
use serde_json::json;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// Editor selected by the caller. `system` delegates to the OS file opener.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorTarget {
    System,
    VsCode,
    Cursor,
    Zed,
}

impl EditorTarget {
    /// Parses the optional `editor` IPC argument. `None` means `system`.
    pub fn parse(raw: Option<&str>) -> Result<Self, IpcError> {
        let Some(raw) = raw else {
            return Ok(Self::System);
        };
        match raw.trim() {
            "" | "system" => Ok(Self::System),
            "vscode" => Ok(Self::VsCode),
            "cursor" => Ok(Self::Cursor),
            "zed" => Ok(Self::Zed),
            other => Err(IpcError::new(
                IpcErrorCode::InvalidArgument,
                format!("unknown editor '{other}'"),
            )
            .with_details(json!({
                "editor": other,
                "supported": ["system", "vscode", "cursor", "zed"],
            }))),
        }
    }

    /// Identifier echoed back in structured error details.
    pub fn id(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::VsCode => "vscode",
            Self::Cursor => "cursor",
            Self::Zed => "zed",
        }
    }

    /// Command-line launcher names, most specific first. Windows resolution
    /// appends `PATHEXT` entries (`code.cmd`, `cursor.cmd`, `zed.exe`).
    pub fn cli_names(self) -> &'static [&'static str] {
        match self {
            Self::System => &[],
            Self::VsCode => &["code"],
            Self::Cursor => &["cursor"],
            Self::Zed => &["zed"],
        }
    }

    /// URL scheme used when no CLI launcher is installed.
    /// Documented schemes: `vscode://file/<path>:<line>:<col>` (VS Code CLI
    /// docs), `cursor://file/...` (VS Code fork, same handler), and
    /// `zed://file/<path>:<line>:<col>` (Zed `OpenRequest::parse`).
    pub fn url_scheme(self) -> Option<&'static str> {
        match self {
            Self::System => None,
            Self::VsCode => Some("vscode"),
            Self::Cursor => Some("cursor"),
            Self::Zed => Some("zed"),
        }
    }

    /// macOS application bundles ship a CLI that is not always on `PATH` for
    /// GUI-launched processes.
    pub fn bundled_cli_paths(self) -> &'static [&'static str] {
        match self {
            Self::System => &[],
            Self::VsCode => &[
                "/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code",
            ],
            Self::Cursor => &["/Applications/Cursor.app/Contents/Resources/app/bin/cursor"],
            Self::Zed => &["/Applications/Zed.app/Contents/MacOS/cli"],
        }
    }
}

/// A launch that has been fully resolved but not yet executed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchPlan {
    /// Hand the file to the platform opener (`open`, `xdg-open`, `ShellExecuteW`).
    SystemOpen { target: PathBuf },
    /// Run an editor launcher with an explicit argument vector (no shell).
    Cli {
        program: PathBuf,
        args: Vec<OsString>,
    },
    /// Hand a documented editor URL to the platform opener.
    Url { url: String },
}

/// Line/column pair after validation. Columns require a line.
pub type Position = Option<(u32, Option<u32>)>;

fn invalid_argument(message: impl Into<String>) -> IpcError {
    IpcError::new(IpcErrorCode::InvalidArgument, message)
}

fn remote_refusal(reason: &str, details: serde_json::Value) -> IpcError {
    IpcError::new(
        IpcErrorCode::Unsupported,
        format!("refusing to open a remote path on this machine ({reason})"),
    )
    .with_details(details)
}

/// Trims whitespace and one layer of wrapping quotes/backticks from a token.
pub fn sanitize_path_token(raw: &str) -> Result<&str, IpcError> {
    let trimmed = raw
        .trim()
        .trim_matches(|c| c == '\'' || c == '"' || c == '`')
        .trim();
    if trimmed.is_empty() {
        return Err(invalid_argument("file path is empty"));
    }
    if trimmed.contains('\0') {
        return Err(invalid_argument("file path contains a NUL byte"));
    }
    Ok(trimmed)
}

/// Rejects tokens that name a host other than this machine.
pub fn reject_remote_path_spec(token: &str) -> Result<(), IpcError> {
    if let Some(scheme_end) = token.find("://") {
        let scheme = &token[..scheme_end];
        return Err(remote_refusal(
            "path is a URL",
            json!({ "path": token, "scheme": scheme }),
        ));
    }
    if let Some(at) = token.find('@') {
        let (user, rest) = token.split_at(at);
        let rest = &rest[1..];
        let host_end = rest.find(':');
        let user_is_plain = !user.is_empty()
            && user
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
        if let Some(host_end) = host_end {
            let host = &rest[..host_end];
            let host_is_plain = !host.is_empty()
                && host
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
            if user_is_plain && host_is_plain {
                return Err(remote_refusal(
                    "path is an scp-style host spec",
                    json!({ "path": token, "host": host }),
                ));
            }
        }
    }
    Ok(())
}

/// True for paths that are absolute on *their* platform, including Windows
/// drive (`C:\`, `C:/`) and UNC (`\\server\share`) forms evaluated on any host.
pub fn is_absolute_token(token: &str) -> bool {
    if Path::new(token).is_absolute() {
        return true;
    }
    if token.starts_with("\\\\") || token.starts_with("//") {
        return true;
    }
    let bytes = token.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
}

/// Expands `~`, keeps absolute paths verbatim, and joins relative paths onto
/// the terminal's working directory.
pub fn resolve_file_link(token: &str, cwd: Option<&str>, home: Option<PathBuf>) -> PathBuf {
    if token == "~" {
        return home.unwrap_or_else(|| PathBuf::from(token));
    }
    if let Some(suffix) = token.strip_prefix("~/").or_else(|| token.strip_prefix("~\\")) {
        return match home {
            Some(home) => {
                let mut path = home;
                for component in suffix.split(['/', '\\']) {
                    if !component.is_empty() {
                        path.push(component);
                    }
                }
                path
            }
            None => PathBuf::from(token),
        };
    }
    if is_absolute_token(token) {
        return PathBuf::from(token);
    }
    match cwd {
        Some(cwd) if !cwd.trim().is_empty() => Path::new(cwd).join(token),
        _ => PathBuf::from(token),
    }
}

/// Validates the optional 1-based line/column pair.
pub fn validate_position(line: Option<u32>, col: Option<u32>) -> Result<Position, IpcError> {
    match (line, col) {
        (None, None) => Ok(None),
        (None, Some(col)) => Err(invalid_argument(format!(
            "column {col} was provided without a line"
        ))),
        (Some(0), _) => Err(invalid_argument("line numbers are 1-based, got 0")),
        (Some(_), Some(0)) => Err(invalid_argument("column numbers are 1-based, got 0")),
        (Some(line), col) => Ok(Some((line, col))),
    }
}

/// Resolves the working directory for a backend session and refuses sessions
/// whose filesystem lives on another host.
///
/// `details` is the daemon's own description of the session; `None` means the
/// daemon could not describe it, and `cause` carries the transport reason.
pub fn session_cwd_guard(
    session_id: &str,
    details: Option<&DaemonSessionDetails>,
    cause: Option<&str>,
) -> Result<Option<PathBuf>, IpcError> {
    if crate::terminal::paired_runtime::Runtime::owns(session_id) {
        return Err(remote_refusal(
            "session is relayed from a paired host",
            json!({ "sessionId": session_id, "kind": "pairedHost" }),
        ));
    }
    let Some(details) = details else {
        return Err(
            IpcError::new(
                IpcErrorCode::SessionNotFound,
                format!("terminal session '{session_id}' is not available"),
            )
            .with_details(json!({ "sessionId": session_id, "cause": cause })),
        );
    };
    if let Some(workspace_id) = details.workspace_id.as_deref() {
        if crate::ssh::projects::is_remote(workspace_id) {
            return Err(remote_refusal(
                "session belongs to an SSH workspace",
                json!({ "sessionId": session_id, "workspaceId": workspace_id, "kind": "ssh" }),
            ));
        }
    }
    Ok(details
        .cwd
        .as_deref()
        .filter(|cwd| !cwd.trim().is_empty())
        .map(PathBuf::from))
}

/// `PATH` lookup with Windows `PATHEXT` expansion. `exists` reports whether a
/// candidate is an existing executable file, which keeps the search testable.
pub fn which_in_path(
    program: &str,
    path_var: Option<&OsStr>,
    pathext: Option<&OsStr>,
    exists: &dyn Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let path_var = path_var?;
    let extensions: Vec<String> = match pathext {
        Some(pathext) => std::iter::once(String::new())
            .chain(
                pathext
                    .to_string_lossy()
                    .split(';')
                    .filter(|ext| !ext.trim().is_empty())
                    .map(|ext| ext.trim().to_ascii_lowercase()),
            )
            .collect(),
        None => vec![String::new()],
    };
    for dir in std::env::split_paths(path_var) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        for extension in &extensions {
            let mut file_name = OsString::from(program);
            file_name.push(extension);
            let candidate = dir.join(&file_name);
            if exists(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

/// Production launcher discovery: `PATH` first, then known app bundle CLIs.
pub fn discover_editor_cli(editor: EditorTarget) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH");
    let pathext = if cfg!(windows) {
        Some(std::env::var_os("PATHEXT").unwrap_or_else(|| OsString::from(".COM;.EXE;.BAT;.CMD")))
    } else {
        None
    };
    let exists = |candidate: &Path| candidate.is_file();
    for name in editor.cli_names() {
        if let Some(found) = which_in_path(
            name,
            path_var.as_deref(),
            pathext.as_deref(),
            &exists,
        ) {
            #[cfg(windows)]
            if found.extension().and_then(OsStr::to_str).is_some_and(|ext|
                ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat")) {
                let binary = match editor {
                    EditorTarget::VsCode => "Code.exe",
                    EditorTarget::Cursor => "Cursor.exe",
                    EditorTarget::Zed => "zed.exe",
                    EditorTarget::System => continue,
                };
                if let Some(root) = found.parent().and_then(Path::parent) {
                    let executable = root.join(binary);
                    if executable.is_file() { return Some(executable); }
                }
                continue;
            }
            return Some(found);
        }
    }
    editor
        .bundled_cli_paths()
        .iter()
        .map(PathBuf::from)
        .find(|candidate| candidate.is_file())
}

fn position_suffix(position: Position) -> String {
    match position {
        None => String::new(),
        Some((line, None)) => format!(":{line}"),
        Some((line, Some(col))) => format!(":{line}:{col}"),
    }
}

/// Percent-encodes a URL path segment sequence, preserving `/` and `:`.
pub fn percent_encode_url_path(raw: &str) -> String {
    let mut encoded = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' | b':' => {
                encoded.push(byte as char)
            }
            other => encoded.push_str(&format!("%{other:02X}")),
        }
    }
    encoded
}

/// Builds a documented editor URL, e.g. `vscode://file/c:/proj/main.rs:5:10`.
pub fn editor_url(scheme: &str, path: &Path, position: Position) -> Result<String, IpcError> {
    let raw = path.to_str().ok_or_else(|| {
        IpcError::new(
            IpcErrorCode::InvalidPath,
            "file path is not valid UTF-8 and cannot be encoded as an editor URL",
        )
    })?;
    if raw.starts_with("\\\\") || raw.starts_with("//") {
        return Err(IpcError::new(
            IpcErrorCode::Unsupported,
            "UNC paths cannot be handed to an editor URL handler; install the editor command line launcher",
        )
        .with_details(json!({ "path": raw })));
    }
    let slashed = raw.replace('\\', "/");
    let rooted = if slashed.starts_with('/') {
        slashed
    } else {
        format!("/{slashed}")
    };
    Ok(format!(
        "{scheme}://file{}{}",
        percent_encode_url_path(&rooted),
        position_suffix(position)
    ))
}

/// Chooses how to open `path`, given an already-discovered CLI launcher.
pub fn plan_launch(
    editor: EditorTarget,
    path: &Path,
    position: Position,
    cli: Option<PathBuf>,
) -> Result<LaunchPlan, IpcError> {
    if editor == EditorTarget::System {
        return Ok(LaunchPlan::SystemOpen {
            target: path.to_path_buf(),
        });
    }
    let mut target = OsString::from(path.as_os_str());
    target.push(position_suffix(position));
    if let Some(program) = cli {
        let args = match editor {
            // `code`/`cursor` accept `--goto <file>:<line>:<col>`.
            EditorTarget::VsCode | EditorTarget::Cursor => {
                vec![OsString::from("--goto"), target]
            }
            // `zed <file>:<line>:<col>` (Zed CLI `paths_with_position`).
            EditorTarget::Zed => vec![target],
            EditorTarget::System => unreachable!("handled above"),
        };
        return Ok(LaunchPlan::Cli { program, args });
    }
    let scheme = editor
        .url_scheme()
        .expect("non-system editors define a URL scheme");
    Ok(LaunchPlan::Url {
        url: editor_url(scheme, path, position)?,
    })
}

/// Full resolution: validate the request, resolve the path against the
/// terminal cwd, require the file to exist, and choose a launch plan.
pub fn prepare_launch(
    path: &str,
    cwd: Option<&str>,
    line: Option<u32>,
    col: Option<u32>,
    editor: EditorTarget,
    home: Option<PathBuf>,
    cli_lookup: &dyn Fn(EditorTarget) -> Option<PathBuf>,
) -> Result<LaunchPlan, IpcError> {
    let token = sanitize_path_token(path)?;
    reject_remote_path_spec(token)?;
    let position = validate_position(line, col)?;
    let candidate = resolve_file_link(token, cwd, home);
    if !candidate.exists() {
        return Err(IpcError::new(
            IpcErrorCode::InvalidPath,
            format!("no such file or directory: {}", candidate.display()),
        )
        .with_details(json!({
            "path": candidate.to_string_lossy(),
            "requested": token,
            "cwd": cwd,
            "reason": "missing",
        })));
    }
    plan_launch(editor, &candidate, position, cli_lookup(editor))
}

/// Executes a plan. This is the only function with desktop side effects.
pub fn execute(plan: LaunchPlan) -> Result<(), IpcError> {
    match plan {
        LaunchPlan::SystemOpen { target } => {
            crate::ipc::browser::open_system_target(target.as_os_str())
        }
        LaunchPlan::Url { url } => {
            crate::ipc::browser::open_system_target(std::ffi::OsStr::new(&url))
        }
        LaunchPlan::Cli { program, args } => {
            // argv only: no shell, so metacharacters in the path are literal.
            std::process::Command::new(&program)
                .args(&args)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map_err(|error| {
                    IpcError::new(
                        IpcErrorCode::IoError,
                        format!("failed to launch {}: {error}", program.display()),
                    )
                    .with_details(json!({
                        "program": program.to_string_lossy(),
                        "kind": format!("{:?}", error.kind()),
                    }))
                })
                .and_then(|status| {
                    if status.success() {
                        Ok(())
                    } else {
                        Err(IpcError::new(IpcErrorCode::IoError,
                            format!("editor launcher exited with {status}")))
                    }
                })
        }
    }
}

/// Blocking core of `cmd_open_file_path`: resolve, then launch.
pub fn open_file_link_blocking(
    path: &str,
    cwd: Option<&str>,
    line: Option<u32>,
    col: Option<u32>,
    editor: EditorTarget,
) -> Result<bool, IpcError> {
    let plan = prepare_launch(
        path,
        cwd,
        line,
        col,
        editor,
        home_dir(),
        &discover_editor_cli,
    )?;
    execute(plan)?;
    Ok(true)
}

/// Home directory used for `~` expansion. `HOME` is not set for GUI-launched
/// processes on Windows, where `USERPROFILE` is authoritative.
pub fn home_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        if let Some(profile) = std::env::var_os("USERPROFILE") {
            if !profile.is_empty() {
                return Some(PathBuf::from(profile));
            }
        }
    }
    std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
}

#[cfg(test)]
#[path = "file_link_tests.rs"]
mod file_link_tests;
