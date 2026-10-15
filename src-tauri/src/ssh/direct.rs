//! Direct OpenSSH transport. Remote paths never enter local filesystem APIs.
use super::{SshAuthMethod, SshHost};
use crate::ipc::{IpcError, IpcErrorCode};
use crate::terminal::shell::ShellCommandPlan;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;

#[cfg(windows)]
const WINDOWS_STDOUT_CHECK_INTERVAL: Duration = Duration::from_millis(25);

fn invalid(message: &str) -> IpcError {
    IpcError::new(IpcErrorCode::InvalidPath, message)
}

pub fn validate_host(host: &SshHost) -> Result<(), IpcError> {
    let token = |value: &str, extra: &str| {
        !value.is_empty()
            && !value.starts_with('-')
            && value.len() <= 1024
            && value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || extra.contains(c))
    };
    if !token(&host.hostname, "._-:[]")
        || host.username.as_deref().is_some_and(|u| !token(u, "._-"))
        || host.port == Some(0)
        || host
            .jump_host
            .as_deref()
            .is_some_and(|j| j.split(',').any(|part| !token(part, "._-@:[]")))
    {
        return Err(invalid("Invalid SSH hostname, username, port or jump host"));
    }
    if host.identity_file.as_deref().is_some_and(|p| {
        (!std::path::Path::new(p).is_absolute()
            && !p.starts_with("~/")
            && !p.starts_with(r"~\"))
            || p.len() > 4096
            || p.chars().any(char::is_control)
    }) || (host.auth_method == SshAuthMethod::Key && host.identity_file.is_none())
    {
        return Err(invalid("Invalid or missing SSH identity file"));
    }
    Ok(())
}

pub fn validate_remote_path(path: &str) -> Result<(), IpcError> {
    if !path.starts_with('/') || path.len() > 4096 || path.chars().any(char::is_control) {
        return Err(invalid(
            "Remote path must be an absolute POSIX path without control characters",
        ));
    }
    Ok(())
}

pub fn quote_posix(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub fn ssh_plan(
    host: &SshHost,
    command: String,
    interactive: bool,
) -> Result<ShellCommandPlan, IpcError> {
    validate_host(host)?;
    super::password::require(host)?;
    let program = super::exec::resolve_ssh_program()?;
    let mut args = super::exec::interactive_argv(host);
    args.remove(0);
    if !interactive {
        args[0] = "-T".into();
    }
    // These precede user config and prevent prompts, forwarding, or connection reuse.
    let mut options = vec![
        if host.auth_method == SshAuthMethod::Password {
            "BatchMode=no"
        } else {
            "BatchMode=yes"
        },
        "StrictHostKeyChecking=yes",
        "UpdateHostKeys=no",
        "ConnectTimeout=3",
        "ConnectionAttempts=1",
        "ServerAliveInterval=15",
        "ServerAliveCountMax=2",
        "PermitLocalCommand=no",
        "RemoteCommand=none",
        "ControlMaster=no",
        "ControlPath=none",
    ];
    if !interactive {
        options.push("ClearAllForwardings=yes");
    }
    if host.auth_method == SshAuthMethod::Password {
        options.extend([
            "PreferredAuthentications=password",
            "PasswordAuthentication=yes",
            "PubkeyAuthentication=no",
            "KbdInteractiveAuthentication=no",
            "NumberOfPasswordPrompts=1",
        ]);
    }
    for option in options {
        args.splice(0..0, ["-o".to_string(), option.to_string()]);
    }
    args.push(command);
    Ok(ShellCommandPlan {
        program,
        args,
    })
}

pub fn shell_plan_with_session(
    host: &SshHost,
    root: &str,
    session_id: Option<&str>,
    local_agent_socket: Option<&str>,
) -> Result<ShellCommandPlan, IpcError> {
    validate_remote_path(root)?;
    let remote_command = match (session_id, local_agent_socket) {
        (Some(sid), Some(_)) => {
            let remote_sock = format!("/tmp/ferryx-agent-{sid}.sock");
            format!(
                "export FERRYX_SESSION_ID={} FERRYX_AGENT_STATE_SOCKET={}; cd {} && exec \"${{SHELL:-/bin/sh}}\" -l",
                quote_posix(sid),
                quote_posix(&remote_sock),
                quote_posix(root)
            )
        }
        (Some(sid), None) => {
            format!(
                "export FERRYX_SESSION_ID={}; cd {} && exec \"${{SHELL:-/bin/sh}}\" -l",
                quote_posix(sid),
                quote_posix(root)
            )
        }
        _ => {
            format!(
                "cd {} && exec \"${{SHELL:-/bin/sh}}\" -l",
                quote_posix(root)
            )
        }
    };

    let mut plan = ssh_plan(host, remote_command, true)?;

    if let (Some(sid), Some(local_sock)) = (session_id, local_agent_socket) {
        let remote_sock = format!("/tmp/ferryx-agent-{sid}.sock");
        plan.args.splice(
            0..0,
            [
                "-o".to_string(),
                "StreamLocalBindUnlink=yes".to_string(),
                "-R".to_string(),
                format!("{remote_sock}:{local_sock}"),
            ],
        );
    }
    Ok(plan)
}

pub fn shell_plan(host: &SshHost, root: &str) -> Result<ShellCommandPlan, IpcError> {
    shell_plan_with_session(host, root, None, None)
}

pub async fn ensure_remote_extension_installed(host: &SshHost) -> Result<(), IpcError> {
    let environment = super::runtime::detect(host).await?;
    super::operations::prepare_integration(host, &environment).await
}

pub fn upload_temp_command(file_name: &str) -> Result<String, IpcError> {
    if file_name.is_empty()
        || file_name.len() > 128
        || file_name.starts_with('.')
        || !file_name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
    {
        return Err(invalid("Invalid remote attachment file name"));
    }
    // Pasted attachments outlive the paste only long enough for the agent to read them, so
    // each upload also prunes what an earlier day left behind.
    Ok(format!(
        "set -e; d=\"${{TMPDIR:-/tmp}}\"; d=\"${{d%/}}/ferryx-paste\"; mkdir -p \"$d\"; \
         chmod 700 \"$d\"; find \"$d\" -type f -mtime +1 -delete 2>/dev/null || true; \
         umask 077; cat > \"$d/{file_name}\"; printf '%s' \"$d/{file_name}\""
    ))
}

/// Streams `bytes` into a fresh file on `host` and returns the absolute remote path.
pub async fn upload_temp_file(
    host: &SshHost,
    file_name: &str,
    bytes: Vec<u8>,
) -> Result<String, IpcError> {
    let environment = super::runtime::detect(host).await?;
    super::operations::upload(host, &environment, file_name, bytes).await
}

pub fn probe_command(path: &str) -> Result<String, IpcError> {
    validate_remote_path(path)?;
    Ok(format!("cd {} && root=$(pwd -P) && printf 'FERRYX_REMOTE_V1\\0%s\\0' \"$root\" && if gitroot=$(git rev-parse --show-toplevel 2>/dev/null); then printf '%s\\0' \"$gitroot\"; else printf '\\0'; fi && if origin=$(git remote get-url origin 2>/dev/null); then printf '%s\\0' \"$origin\"; else printf '\\0'; fi", quote_posix(path)))
}

pub fn parse_probe(bytes: &[u8]) -> Result<(String, Option<String>, Option<String>), IpcError> {
    let parts = bytes.split(|b| *b == 0).collect::<Vec<_>>();
    if (parts.len() != 4 && parts.len() != 5)
        || parts[0] != b"FERRYX_REMOTE_V1"
        || !parts.last().unwrap().is_empty()
    {
        return Err(invalid("Invalid remote directory probe response"));
    }
    let root = std::str::from_utf8(parts[1]).map_err(|_| invalid("Remote path is not UTF-8"))?;
    validate_remote_path(root)?;
    let git = std::str::from_utf8(parts[2]).map_err(|_| invalid("Remote Git path is not UTF-8"))?;
    let git_root = if git.is_empty() {
        None
    } else {
        validate_remote_path(git)?;
        Some(git.to_string())
    };
    let git_remote = if parts.len() == 5 {
        let remote =
            std::str::from_utf8(parts[3]).map_err(|_| invalid("Remote origin URL is not UTF-8"))?;
        if remote.trim().is_empty() {
            None
        } else {
            Some(remote.trim().to_string())
        }
    } else {
        None
    };
    Ok((root.to_string(), git_root, git_remote))
}

pub async fn probe(
    host: &SshHost,
    path: &str,
) -> Result<(String, Option<String>, Option<String>), IpcError> {
    let environment = super::runtime::detect(host).await?;
    let probed = super::operations::probe(host, &environment, path).await?;
    Ok((probed.repo_root, probed.git_root, probed.git_remote))
}

fn windows_powershell_root_arg(root: &str) -> String {
    if (root.contains(' ') || root.contains('\t')) && root.ends_with('\\') {
        let mut escaped = root.to_string();
        let trailing = root.chars().rev().take_while(|&c| c == '\\').count();
        for _ in 0..trailing {
            escaped.push('\\');
        }
        super::runtime::powershell_data(&escaped)
    } else {
        super::runtime::powershell_data(root)
    }
}

pub fn bridge_command(
    env: &super::runtime::RemoteEnvironment,
    location: &super::helper_setup::HelperLocation,
) -> String {
    match env.platform {
        super::runtime::RemotePlatform::Posix => format!(
            "exec {} bridge --stdio --root {}",
            quote_posix(&location.executable),
            quote_posix(&location.root),
        ),
        super::runtime::RemotePlatform::Windows => {
            let root_arg = match env.executor {
                super::runtime::RemoteExecutor::Powershell => {
                    windows_powershell_root_arg(&location.root)
                }
                _ => super::runtime::powershell_data(&location.root),
            };
            format!(
                "& {} bridge --stdio --root {}",
                super::runtime::powershell_data(&location.executable),
                root_arg,
            )
        }
    }
}

fn quote_windows_cmd_arg(arg: &str) -> String {
    let mut quoted = String::from('"');
    let mut slashes = 0;
    for ch in arg.chars() {
        match ch {
            '\\' => {
                slashes += 1;
            }
            '"' => {
                quoted.extend(std::iter::repeat_n('\\', slashes * 2 + 1));
                quoted.push('"');
                slashes = 0;
            }
            _ => {
                quoted.extend(std::iter::repeat_n('\\', slashes));
                quoted.push(ch);
                slashes = 0;
            }
        }
    }
    quoted.extend(std::iter::repeat_n('\\', slashes * 2));
    quoted.push('"');
    quoted
}

pub fn windows_bridge_command(
    env: &super::runtime::RemoteEnvironment,
    location: &super::helper_setup::HelperLocation,
) -> Result<String, IpcError> {
    super::runtime::RemotePlatform::Windows.validate_path(&location.executable)?;
    super::runtime::RemotePlatform::Windows.validate_path(&location.root)?;
    // Start the binary without PowerShell's native-command pipeline: bridge frames
    // must inherit the SSH channel's raw stdio, not be decoded as shell text.
    let arguments = format!("bridge --stdio --root {}", quote_windows_cmd_arg(&location.root));
    let script = format!(
        "$psi = New-Object System.Diagnostics.ProcessStartInfo; \
         $psi.FileName = {}; $psi.Arguments = {}; $psi.UseShellExecute = $false; \
         $child = [System.Diagnostics.Process]::Start($psi); $child.WaitForExit(); exit $child.ExitCode",
        super::runtime::powershell_data(&location.executable),
        super::runtime::powershell_data(&arguments),
    );
    Ok(env.executor.command(&script))
}

pub fn bridge_plan(
    host: &SshHost,
    env: &super::runtime::RemoteEnvironment,
    location: &super::helper_setup::HelperLocation,
) -> Result<ShellCommandPlan, IpcError> {
    match env.platform {
        super::runtime::RemotePlatform::Posix => {
            let command = bridge_command(env, location);
            ssh_plan(host, env.executor.command(&command), false)
        }
        super::runtime::RemotePlatform::Windows => {
            let command = windows_bridge_command(env, location)?;
            ssh_plan(host, command, false)
        }
    }
}

pub(crate) fn spawn_child(
    plan: &ShellCommandPlan,
    stdin: Stdio,
) -> Result<tokio::process::Child, IpcError> {
    spawn_child_with_stdout(plan, stdin, Stdio::piped())
}

fn spawn_child_with_stdout(
    plan: &ShellCommandPlan,
    stdin: Stdio,
    stdout: Stdio,
) -> Result<tokio::process::Child, IpcError> {
    crate::util::no_window_tokio_command(&plan.program)
        .args(&plan.args)
        .envs(super::password::environment(&plan.args)?)
        .stdin(stdin)
        .stdout(stdout)
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| IpcError::new(IpcErrorCode::IoError, format!("Failed to start SSH: {e}")))
}

fn strip_ansi_colors(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for code in chars.by_ref() {
                if ('@'..='~').contains(&code) {
                    break;
                }
            }
        } else {
            plain.push(ch);
        }
    }
    plain
}

pub(crate) async fn bounded_output(
    plan: &ShellCommandPlan,
    deadline: Duration,
) -> Result<Vec<u8>, IpcError> {
    collect_finite_output(plan, Stdio::null(), deadline, PROBE_OUTPUT_LIMIT).await
}

/// For callers that legitimately expect a large body - a transcript file, not a probe.
///
/// The bound still exists so a misbehaving remote cannot stream without limit, but it is orders of
/// magnitude above [`PROBE_OUTPUT_LIMIT`], which is sized for directory listings and probe reports.
/// Callers should also bound the remote side (`tail -c`) so the wire carries only what is needed.
pub(crate) async fn bounded_output_with_limit(
    plan: &ShellCommandPlan,
    deadline: Duration,
    max_bytes: usize,
) -> Result<Vec<u8>, IpcError> {
    collect_finite_output(plan, Stdio::null(), deadline, max_bytes).await
}

pub(crate) async fn bounded_output_with_stdin(
    plan: &ShellCommandPlan,
    deadline: Duration,
    input: Vec<u8>,
) -> Result<Vec<u8>, IpcError> {
    #[cfg(windows)]
    { bounded_output_with_stdin_limit(plan, deadline, input, PROBE_OUTPUT_LIMIT).await }
    #[cfg(not(windows))]
    {
        let mut child = spawn_child(plan, Stdio::piped())?;
        let mut stdin = child.stdin.take().ok_or_else(|| IpcError::internal("Missing SSH stdin"))?;
        let write = async move {
            use tokio::io::AsyncWriteExt;
            stdin.write_all(&input).await?;
            stdin.shutdown().await
        };
        let result = tokio::time::timeout(deadline, async {
            let output = collect_pipe_output(child, deadline, PROBE_OUTPUT_LIMIT);
            let write = async { write.await.map_err(|error| IpcError::new(IpcErrorCode::IoError, error.to_string())) };
            tokio::try_join!(write, output).map(|(_, bytes)| bytes)
        }).await;
        match result {
            Ok(Ok(bytes)) => Ok(bytes),
            Ok(Err(error)) => Err(error),
            Err(_) => Err(super::runtime::error(IpcErrorCode::IoError,"transport","SSH operation timed out")),
        }
    }
}

#[cfg(windows)]
async fn bounded_output_with_stdin_limit(
    plan: &ShellCommandPlan,
    deadline: Duration,
    input: Vec<u8>,
    max_bytes: usize,
) -> Result<Vec<u8>, IpcError> {
    let (stdout_file, child_stdout) = crate::ipc::run_blocking(|| {
        let file = tempfile::tempfile().map_err(|error| IpcError::new(IpcErrorCode::IoError, error.to_string()))?;
        let child_file = file.try_clone().map_err(|error| IpcError::new(IpcErrorCode::IoError, error.to_string()))?;
        Ok((std::sync::Arc::new(file), child_file))
    }).await?;
    let mut child = spawn_child_with_stdout(plan, Stdio::piped(), Stdio::from(child_stdout))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| IpcError::internal("Missing SSH stdin"))?;
    let write = async move {
        use tokio::io::AsyncWriteExt;
        stdin.write_all(&input).await?;
        stdin.shutdown().await
    };
    collect_windows_output(child, stdout_file, deadline, max_bytes, Some(Box::pin(write))).await
}

/// Ceiling for probe-style commands (listings, reports, probes). Not a universal limit: see
/// [`bounded_output_with_limit`].
pub(crate) const PROBE_OUTPUT_LIMIT: usize = 16 * 1024;

async fn collect_finite_output(
    plan: &ShellCommandPlan,
    stdin: Stdio,
    deadline: Duration,
    max_bytes: usize,
) -> Result<Vec<u8>, IpcError> {
    #[cfg(windows)]
    {
        let (stdout_file, child_stdout) = crate::ipc::run_blocking(|| {
            let file = tempfile::tempfile().map_err(|error| IpcError::new(IpcErrorCode::IoError, error.to_string()))?;
            let child_stdout = file.try_clone().map_err(|error| IpcError::new(IpcErrorCode::IoError, error.to_string()))?;
            Ok((std::sync::Arc::new(file), child_stdout))
        }).await?;
        let child = spawn_child_with_stdout(plan, stdin, Stdio::from(child_stdout))?;
        collect_windows_output(child, stdout_file, deadline, max_bytes, None).await
    }
    #[cfg(not(windows))]
    {
        let child = spawn_child(plan, stdin)?;
        collect_pipe_output(child, deadline, max_bytes).await
    }
}

async fn collect_output(
    child: tokio::process::Child,
    deadline: Duration,
    max_bytes: usize,
) -> Result<Vec<u8>, IpcError> {
    collect_pipe_output(child, deadline, max_bytes).await
}

async fn collect_pipe_output(
    mut child: tokio::process::Child,
    deadline: Duration,
    max_bytes: usize,
) -> Result<Vec<u8>, IpcError> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| IpcError::internal("Missing SSH stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| IpcError::internal("Missing SSH stderr"))?;
    let read = async |stream: Box<dyn tokio::io::AsyncRead + Unpin + Send>| {
        let mut bytes = Vec::new();
        stream.take(max_bytes as u64 + 1).read_to_end(&mut bytes).await?;
        if bytes.len() > max_bytes {
            return Err(std::io::Error::other(format!(
                "SSH output exceeds {max_bytes} bytes"
            )));
        }
        Ok::<_, std::io::Error>(bytes)
    };
    let result = tokio::time::timeout(deadline, async {
        tokio::try_join!(child.wait(), read(Box::new(stdout)), read(Box::new(stderr)))
    })
    .await;
    let error = match result {
        Ok(Ok((status, stdout, stderr))) => {
            return if status.success() {
                Ok(stdout)
            } else {
                use base64::{engine::general_purpose::STANDARD, Engine as _};
                let text = std::str::from_utf8(&stderr).ok();
                let diagnostic = if stderr.is_empty() {
                    "Remote stderr was empty; raw bytes are available in details".to_string()
                } else {
                    match text.map(str::trim) {
                        Some(s) if !s.is_empty() => strip_ansi_colors(s),
                        Some(_) => "Remote stderr was whitespace only; raw bytes are available in details".to_string(),
                        None => "Remote stderr contained non-UTF-8 data; raw bytes are available in details".to_string(),
                    }
                };
                Err(IpcError::new(
                    IpcErrorCode::IoError,
                    format!(
                        "SSH command failed (exit {}): {}",
                        status
                            .code()
                            .map_or_else(|| "signal".into(), |code| code.to_string()),
                        diagnostic
                    ),
                )
                .with_details(serde_json::json!({
                    "stage": "execution",
                    "exitCode": status.code(),
                    "stderr": text,
                    "stderrBase64": STANDARD.encode(&stderr),
                    "encoding": if text.is_some() { "utf-8" } else { "unknown" }
                })))
            };
        }
        Err(_) => super::runtime::error(
            IpcErrorCode::IoError,
            "transport",
            "SSH operation timed out",
        ),
        Ok(Err(error)) => IpcError::new(
            IpcErrorCode::IoError,
            format!("SSH probe output failed: {error}"),
        ),
    };
    // kill() also waits/reaps. Cancellation drops a kill_on_drop-owned child.
    child.kill().await.map_err(|e| {
        IpcError::new(
            IpcErrorCode::IoError,
            format!("Failed to terminate SSH probe: {e}"),
        )
    })?;
    Err(error)
}

#[cfg(windows)]
async fn collect_windows_output(
    mut child: tokio::process::Child,
    stdout_file: std::sync::Arc<std::fs::File>,
    deadline: Duration,
    max_bytes: usize,
    stdin_write: Option<
        std::pin::Pin<Box<dyn std::future::Future<Output = std::io::Result<()>> + Send>>,
    >,
) -> Result<Vec<u8>, IpcError> {
    use tokio::io::AsyncReadExt;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| IpcError::internal("Missing SSH stderr"))?;
    let stderr_prefix = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let stderr_prefix_reader = stderr_prefix.clone();
    let stderr_read = async move {
        let mut bytes = Vec::new();
        let mut stderr = stderr.take(max_bytes as u64 + 1);
        let mut chunk = [0u8; 1024];
        loop {
            let count = stderr.read(&mut chunk).await?;
            if count == 0 {
                break;
            }
            bytes.extend_from_slice(&chunk[..count]);
            let captured_len = bytes.len().min(max_bytes);
            *stderr_prefix_reader.lock().unwrap() = bytes[..captured_len].to_vec();
        }
        if bytes.len() > max_bytes {
            return Err(std::io::Error::other(format!("SSH stderr exceeds {max_bytes} bytes")));
        }
        Ok::<_, std::io::Error>(bytes)
    };
    tokio::pin!(stderr_read);
    let mut stdin_write = stdin_write;
    let mut stderr_bytes = None;
    let mut stdin_done = stdin_write.is_none();
    let result = tokio::time::timeout(deadline, async {
        let mut interval = tokio::time::interval(WINDOWS_STDOUT_CHECK_INTERVAL);
        let mut status = None;
        loop {
            if status.is_some() && stderr_bytes.is_some() && stdin_done { break; }
            tokio::select! {
                result = child.wait(), if status.is_none() => {
                    status = Some(result?);
                }
                result = &mut stderr_read, if stderr_bytes.is_none() => {
                    stderr_bytes = Some(result?);
                }
                result = async { stdin_write.as_mut().expect("stdin writer present").as_mut().await }, if !stdin_done => {
                    stdin_done = true;
                    result?;
                }
                _ = interval.tick() => {
                    let file = stdout_file.clone();
                    let len = crate::ipc::run_blocking(move || {
                        let metadata_file = file.try_clone().map_err(|error| IpcError::new(IpcErrorCode::IoError, error.to_string()))?;
                        metadata_file.metadata().map(|metadata| metadata.len())
                            .map_err(|error| IpcError::new(IpcErrorCode::IoError, error.to_string()))
                    }).await.map_err(|error| std::io::Error::other(error.to_string()))?;
                    if len > max_bytes as u64 {
                        return Err(std::io::Error::other(format!("SSH output exceeds {max_bytes} bytes")));
                    }
                }
            }
        }
        let status = status.expect("child wait completed");
        let stderr = stderr_bytes.expect("stderr read completed");
        use std::io::{Read, Seek, SeekFrom};
        let file = stdout_file.clone();
        let stdout = crate::ipc::run_blocking(move || {
            let mut file = file.try_clone().map_err(|error| IpcError::new(IpcErrorCode::IoError, error.to_string()))?;
            file.seek(SeekFrom::Start(0)).map_err(|error| IpcError::new(IpcErrorCode::IoError, error.to_string()))?;
            let mut bytes = Vec::new();
            file.take(max_bytes as u64 + 1).read_to_end(&mut bytes)
                .map_err(|error| IpcError::new(IpcErrorCode::IoError, error.to_string()))?;
            Ok(bytes)
        }).await.map_err(|error| std::io::Error::other(error.to_string()))?;
        if stdout.len() > max_bytes { return Err(std::io::Error::other(format!("SSH output exceeds {max_bytes} bytes"))); }
        Ok((status, stderr, stdout))
    }).await;
    match result {
        Ok(Ok((status, stderr, stdout))) => {
            if status.success() { return Ok(stdout); }
            return Err(ssh_status_error(status, &stderr));
        }
        Err(_) => {
            child.kill().await.map_err(|error| IpcError::new(IpcErrorCode::IoError, format!("Failed to terminate SSH probe: {error}")))?;
            let stderr = stderr_prefix.lock().unwrap().clone();
            Err(timeout_error_with_stderr(&stderr))
        }
        Ok(Err(error)) => {
            child.kill().await.map_err(|kill_error| IpcError::new(IpcErrorCode::IoError, format!("Failed to terminate SSH probe: {kill_error}")))?;
            Err(IpcError::new(IpcErrorCode::IoError, format!("SSH probe output failed: {error}")))
        }
    }
}

fn timeout_error_with_stderr(stderr: &[u8]) -> IpcError {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let text = std::str::from_utf8(stderr).ok();
    super::runtime::error(IpcErrorCode::IoError, "transport", "SSH operation timed out")
        .with_details(serde_json::json!({
            "stage": "transport",
            "stderr": text,
            "stderrBase64": STANDARD.encode(stderr),
            "encoding": if text.is_some() { "utf-8" } else { "unknown" },
        }))
}

fn ssh_status_error(status: std::process::ExitStatus, stderr: &[u8]) -> IpcError {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let text = std::str::from_utf8(stderr).ok();
    let diagnostic = if stderr.is_empty() {
        "Remote stderr was empty; raw bytes are available in details".to_string()
    } else {
        match text.map(str::trim) {
            Some(s) if !s.is_empty() => strip_ansi_colors(s),
            Some(_) => "Remote stderr was whitespace only; raw bytes are available in details".to_string(),
            None => "Remote stderr contained non-UTF-8 data; raw bytes are available in details".to_string(),
        }
    };
    IpcError::new(
        IpcErrorCode::IoError,
        format!("SSH command failed (exit {}): {diagnostic}", status.code().map_or_else(|| "signal".into(), |code| code.to_string())),
    ).with_details(serde_json::json!({
        "stage":"execution", "exitCode":status.code(), "stderr":text,
        "stderrBase64":STANDARD.encode(stderr), "encoding":if text.is_some(){"utf-8"}else{"unknown"}
    }))
}

#[cfg(test)]
#[path = "direct_tests.rs"]
mod tests;
