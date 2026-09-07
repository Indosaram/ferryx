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
    let mut options = vec![
        "BatchMode=yes",
        "StrictHostKeyChecking=yes",
        "ConnectTimeout=5",
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
    for option in options {
        args.splice(0..0, ["-o".to_string(), option.to_string()]);
    }
    args.push(command);
    Ok(ShellCommandPlan {
        program: "ssh".into(),
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

pub fn install_remote_extension_script() -> String {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let source_b64 = STANDARD.encode(crate::daemon::agent_extension::EXTENSION_SOURCE);
    format!(
        "sh -c 'for b in \"$HOME/.omo\" \"$HOME/.pi\" \"$HOME/.omp\"; do \
            if [ -d \"$b\" ] || [ \"$b\" = \"$HOME/.omo\" ]; then \
                d=\"$b/agent/extensions\"; \
                mkdir -p \"$d\" 2>/dev/null; \
                t=\"$d/ferryx-agent-state.ts\"; \
                tmp=\"$d/.ferryx-agent-state.ts.tmp\"; \
                echo \"{source_b64}\" | (base64 -d 2>/dev/null || base64 -D 2>/dev/null || openssl enc -base64 -d 2>/dev/null) > \"$tmp\" 2>/dev/null; \
                if [ -s \"$tmp\" ]; then mv -f \"$tmp\" \"$t\" 2>/dev/null; else rm -f \"$tmp\" 2>/dev/null; fi; \
            fi; \
        done'"
    )
}

pub async fn ensure_remote_extension_installed(host: &SshHost) -> Result<(), IpcError> {
    let script = install_remote_extension_script();
    let plan = ssh_plan(host, script, false)?;
    let _ = bounded_output(&plan, Duration::from_secs(8)).await?;
    Ok(())
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
        let remote = std::str::from_utf8(parts[3]).map_err(|_| invalid("Remote origin URL is not UTF-8"))?;
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

pub async fn probe(host: &SshHost, path: &str) -> Result<(String, Option<String>, Option<String>), IpcError> {
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
