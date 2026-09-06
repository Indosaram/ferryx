//! Direct OpenSSH transport. Remote paths never enter local filesystem APIs.
use super::{SshAuthMethod, SshHost};
use crate::ipc::{IpcError, IpcErrorCode};
use crate::terminal::shell::ShellCommandPlan;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;

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
        (!std::path::Path::new(p).is_absolute() && !p.starts_with("~/"))
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
    let mut args = super::exec::interactive_argv(host);
    args.remove(0);
    if !interactive {
        args.remove(0);
    }
    // These precede user config and prevent prompts, forwarding, or connection reuse.
    for option in [
        "BatchMode=yes",
        "StrictHostKeyChecking=yes",
        "ConnectTimeout=5",
        "ConnectionAttempts=1",
        "ServerAliveInterval=15",
        "ServerAliveCountMax=2",
        "ClearAllForwardings=yes",
        "PermitLocalCommand=no",
        "RemoteCommand=none",
        "ControlMaster=no",
        "ControlPath=none",
    ] {
        args.splice(0..0, ["-o".to_string(), option.to_string()]);
    }
    args.push(command);
    Ok(ShellCommandPlan {
        program: "ssh".into(),
        args,
    })
}

pub fn shell_plan(host: &SshHost, root: &str) -> Result<ShellCommandPlan, IpcError> {
    validate_remote_path(root)?;
    ssh_plan(
        host,
        format!(
            "cd {} && exec \"${{SHELL:-/bin/sh}}\" -l",
            quote_posix(root)
        ),
        true,
    )
}

pub fn probe_command(path: &str) -> Result<String, IpcError> {
    validate_remote_path(path)?;
    Ok(format!("cd {} && root=$(pwd -P) && printf 'FERRYX_REMOTE_V1\\0%s\\0' \"$root\" && if gitroot=$(git rev-parse --show-toplevel 2>/dev/null); then printf '%s\\0' \"$gitroot\"; else printf '\\0'; fi", quote_posix(path)))
}

pub fn parse_probe(bytes: &[u8]) -> Result<(String, Option<String>), IpcError> {
    let parts = bytes.split(|b| *b == 0).collect::<Vec<_>>();
    if parts.len() != 4 || parts[0] != b"FERRYX_REMOTE_V1" || !parts[3].is_empty() {
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
    Ok((root.to_string(), git_root))
}

pub async fn probe(host: &SshHost, path: &str) -> Result<(String, Option<String>), IpcError> {
    let plan = ssh_plan(host, probe_command(path)?, false)?;
    let output = bounded_output(&plan, Duration::from_secs(8)).await?;
    parse_probe(&output)
}

pub(crate) async fn bounded_output(
    plan: &ShellCommandPlan,
    deadline: Duration,
) -> Result<Vec<u8>, IpcError> {
    let child = tokio::process::Command::new(&plan.program)
        .args(&plan.args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| IpcError::new(IpcErrorCode::IoError, format!("Failed to start SSH: {e}")))?;
    collect_output(child, deadline).await
}

async fn collect_output(
    mut child: tokio::process::Child,
    deadline: Duration,
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
        stream.take(16385).read_to_end(&mut bytes).await?;
        if bytes.len() > 16384 {
            return Err(std::io::Error::other("SSH output exceeds 16 KiB"));
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
                Err(invalid(&format!(
                    "SSH validation failed: {}",
                    String::from_utf8_lossy(&stderr).trim()
                )))
            };
        }
        Err(_) => invalid("SSH directory probe timed out after its bounded deadline"),
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

#[cfg(test)]
#[path = "direct_tests.rs"]
mod tests;
