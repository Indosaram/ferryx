//! Remote helper installation and SSH-independent startup lifecycle.

use crate::ipc::{IpcError, IpcErrorCode};
use crate::ssh::direct;
use crate::ssh::runtime::{self, RemoteEnvironment, RemotePlatform};
use crate::ssh::SshHost;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HelperLocation {
    pub executable: String,
    pub root: String,
}

/// Honest Terminal Helper readiness from a bounded, non-installing remote check.
///
/// `Installed` means the helper binary was observed present (and executable on
/// POSIX) at the default location. `Missing` means the remote explicitly
/// reported the binary absent or not executable. `Unknown` covers every other
/// outcome (timeout, transport failure, unexpected output): no claim is made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HelperProbeState {
    Installed,
    Missing,
    Unknown,
}

/// Stdout marker printed by the probe script when the helper binary checks out.
const PROBE_READY_MARKER: &str = "FERRYX_HELPER_READY";

#[derive(Debug, Deserialize)]
struct HelperReadyEvent {
    event: String,
    protocol: u32,
}

pub(crate) fn host_slug(host_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(host_id.as_bytes());
    let digest = hasher.finalize();
    let hex_hash: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
    let safe_prefix: String = host_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(16)
        .collect();
    if safe_prefix.is_empty() {
        format!("host-{}", &hex_hash[..32])
    } else {
        format!("{}-{}", safe_prefix, &hex_hash[..32])
    }
}

pub fn default_location(
    host: &SshHost,
    env: &RemoteEnvironment,
) -> Result<HelperLocation, IpcError> {
    env.platform.validate_path(&env.home)?;
    let slug = host_slug(&host.id);
    let (executable, root) = match env.platform {
        RemotePlatform::Posix => {
            let home = env.home.trim_end_matches('/');
            (
                format!("{home}/.ferryx/bin/ferryx-remote-helper"),
                format!("{home}/.ferryx/helper/{slug}"),
            )
        }
        RemotePlatform::Windows => {
            let home = env.home.trim_end_matches(&['\\', '/'][..]);
            (
                format!("{home}\\.ferryx\\bin\\ferryx-remote-helper.exe"),
                format!("{home}\\.ferryx\\helper\\{slug}"),
            )
        }
    };
    env.platform.validate_path(&executable)?;
    env.platform.validate_path(&root)?;
    Ok(HelperLocation { executable, root })
}

pub async fn install(
    host: &SshHost,
    env: &RemoteEnvironment,
    location: &HelperLocation,
    local_binary: &Path,
) -> Result<(), IpcError> {
    env.platform.validate_path(&location.executable)?;
    env.platform.validate_path(&location.root)?;

    let local_path = local_binary.to_path_buf();
    let binary_data = tokio::task::spawn_blocking(move || {
        let metadata = std::fs::metadata(&local_path).map_err(|e| {
            IpcError::new(
                IpcErrorCode::IoError,
                format!("Local helper binary not accessible at {}: {}", local_path.display(), e),
            )
        })?;
        if !metadata.is_file() {
            return Err(IpcError::new(
                IpcErrorCode::InvalidPath,
                format!("Local helper binary path {} is not a regular file", local_path.display()),
            ));
        }
        let bytes = std::fs::read(&local_path).map_err(|e| {
            IpcError::new(
                IpcErrorCode::IoError,
                format!("Failed to read local helper binary from {}: {}", local_path.display(), e),
            )
        })?;
        if bytes.is_empty() {
            return Err(IpcError::new(
                IpcErrorCode::InvalidPath,
                format!("Local helper binary at {} is empty", local_path.display()),
            ));
        }
        Ok(bytes)
    })
    .await
    .map_err(|e| IpcError::new(IpcErrorCode::InternalError, format!("Join error in blocking offload: {e}")))?
    ?;

    let (cmd, input_bytes) = match env.platform {
        RemotePlatform::Posix => {
            let script = build_posix_upload_script(location, &binary_data);
            (env.executor.command("sh -s"), script.into_bytes())
        }
        RemotePlatform::Windows => {
            let script = build_windows_upload_script(location, &binary_data);
            (env.executor.command("-Command -"), script.into_bytes())
        }
    };

    let plan = direct::ssh_plan(host, cmd, false)?;
    direct::bounded_output_with_stdin(&plan, Duration::from_secs(60), input_bytes)
        .await
        .map_err(|mut err| {
            if let Some(details) = err.details.as_mut() {
                details["stage"] = "install".into();
                details["executable"] = location.executable.clone().into();
            }
            err
        })?;

    Ok(())
}

pub(crate) fn parse_ready_output(bytes: &[u8]) -> Result<(), IpcError> {
    let text = std::str::from_utf8(bytes).map_err(|_| {
        IpcError::new(
            IpcErrorCode::IoError,
            "Remote helper startup output is not valid UTF-8",
        )
    })?;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(event) = serde_json::from_str::<HelperReadyEvent>(trimmed) {
            if event.event == "ready" && event.protocol == 1 {
                return Ok(());
            }
        }
    }
    Err(IpcError::new(
        IpcErrorCode::IoError,
        "Remote helper startup did not produce a valid ready event",
    )
    .with_details(serde_json::json!({
        "stage": "startup",
        "output": text.trim(),
    })))
}

pub(crate) fn map_ensure_started_error(err: IpcError, location: &HelperLocation) -> IpcError {
    let stage = err
        .details
        .as_ref()
        .and_then(|d| d.get("stage"))
        .and_then(|s| s.as_str());
    let exit_code = err
        .details
        .as_ref()
        .and_then(|d| d.get("exitCode"))
        .and_then(|c| c.as_i64());
    let stderr = err
        .details
        .as_ref()
        .and_then(|d| d.get("stderr"))
        .and_then(|s| s.as_str())
        .unwrap_or("");

    // Preserve transport/connection errors (exit 255 or transport stage)
    if exit_code == Some(255) || stage == Some("transport") {
        return err;
    }

    let has_missing_marker = stderr.contains("FERRYX_ERR_HELPER_MISSING")
        || err.message.contains("FERRYX_ERR_HELPER_MISSING");
    let has_not_executable_marker = stderr.contains("FERRYX_ERR_HELPER_NOT_EXECUTABLE")
        || err.message.contains("FERRYX_ERR_HELPER_NOT_EXECUTABLE");

    // Reject contradictory markers or exit codes
    if (has_missing_marker && has_not_executable_marker)
        || (exit_code == Some(126) && has_missing_marker)
        || (exit_code == Some(127) && has_not_executable_marker)
    {
        return err;
    }

    let is_missing = exit_code == Some(127) || (exit_code == Some(1) && has_missing_marker);
    let is_permissions =
        exit_code == Some(126) || (exit_code == Some(1) && has_not_executable_marker);

    let cause = err.details.clone();

    if is_missing {
        let mut details = serde_json::json!({
            "stage": "helper_missing",
            "executable": location.executable,
            "root": location.root,
        });
        if let Some(c) = cause {
            details["cause"] = c;
        }
        IpcError::new(
            IpcErrorCode::CliExecutableNotFound,
            format!(
                "Remote helper binary is not installed at '{}'. Run install to set up the helper.",
                location.executable
            ),
        )
        .with_details(details)
    } else if is_permissions {
        let mut details = serde_json::json!({
            "stage": "helper_permissions",
            "executable": location.executable,
            "root": location.root,
        });
        if let Some(c) = cause {
            details["cause"] = c;
        }
        IpcError::new(
            IpcErrorCode::Unsupported,
            format!(
                "Remote helper binary at '{}' is not executable. Please verify permissions.",
                location.executable
            ),
        )
        .with_details(details)
    } else {
        err
    }
}

pub async fn ensure_started(
    host: &SshHost,
    env: &RemoteEnvironment,
    location: &HelperLocation,
) -> Result<(), IpcError> {
    env.platform.validate_path(&location.executable)?;
    env.platform.validate_path(&location.root)?;

    let script = match env.platform {
        RemotePlatform::Posix => format!(
            "exe={}; root={}; host_id={}; \
             if [ ! -f \"$exe\" ]; then printf 'FERRYX_ERR_HELPER_MISSING\\n' >&2; exit 127; fi; \
             if [ ! -x \"$exe\" ]; then printf 'FERRYX_ERR_HELPER_NOT_EXECUTABLE\\n' >&2; exit 126; fi; \
             exec \"$exe\" start --root \"$root\" --host-id \"$host_id\"",
            direct::quote_posix(&location.executable),
            direct::quote_posix(&location.root),
            direct::quote_posix(&host.id)
        ),
        RemotePlatform::Windows => format!(
            "$exe = {}; $root = {}; $hostId = {}; \
             if (-not [System.IO.File]::Exists($exe)) {{ [Console]::Error.WriteLine('FERRYX_ERR_HELPER_MISSING'); exit 127; }}; \
             & $exe start --root $root --host-id $hostId",
            runtime::powershell_data(&location.executable),
            runtime::powershell_data(&location.root),
            runtime::powershell_data(&host.id)
        ),
    };

    let plan = direct::ssh_plan(host, env.executor.command(&script), false)?;
    let output = direct::bounded_output(&plan, Duration::from_secs(15)).await;

    match output {
        Ok(stdout_bytes) => parse_ready_output(&stdout_bytes),
        Err(err) => Err(map_ensure_started_error(err, location)),
    }
}

/// Pure classification of a probe outcome: no I/O, unit-testable.
///
/// `Ok(stdout)` with the success marker means the binary was observed ready.
/// Either explicit stderr sentinel (missing or not-executable) means the helper
/// is not usable. Timeouts, transport failures, and anything else stay
/// `Unknown` so the UI never claims readiness it did not observe.
pub(crate) fn classify_probe_result(result: Result<&[u8], &IpcError>) -> HelperProbeState {
    match result {
        Ok(stdout) => {
            let text = std::str::from_utf8(stdout).unwrap_or("");
            if text.contains(PROBE_READY_MARKER) {
                HelperProbeState::Installed
            } else {
                HelperProbeState::Unknown
            }
        }
        Err(err) => classify_probe_error(err),
    }
}

fn classify_probe_error(err: &IpcError) -> HelperProbeState {
    let details = err.details.as_ref();
    let stage = details
        .and_then(|d| d.get("stage"))
        .and_then(|s| s.as_str());
    let exit_code = details
        .and_then(|d| d.get("exitCode"))
        .and_then(|c| c.as_i64());
    let stderr = details
        .and_then(|d| d.get("stderr"))
        .and_then(|s| s.as_str())
        .unwrap_or("");

    // Transport and connection failures (exit 255, timeouts) say nothing about
    // the helper binary, so they must not read as Missing.
    if exit_code == Some(255) || stage == Some("transport") {
        return HelperProbeState::Unknown;
    }

    let has_missing_marker = stderr.contains("FERRYX_ERR_HELPER_MISSING")
        || err.message.contains("FERRYX_ERR_HELPER_MISSING");
    let has_not_executable_marker = stderr.contains("FERRYX_ERR_HELPER_NOT_EXECUTABLE")
        || err.message.contains("FERRYX_ERR_HELPER_NOT_EXECUTABLE");

    // Contradictory signals are untrustworthy: claim nothing.
    if (has_missing_marker && has_not_executable_marker)
        || (exit_code == Some(126) && has_missing_marker)
        || (exit_code == Some(127) && has_not_executable_marker)
    {
        return HelperProbeState::Unknown;
    }

    // Windows sshd.exe normalizes child exit codes to 1, so the explicit
    // stderr sentinels are the reliable signal there; POSIX adds 127/126.
    // Both sentinels mean the helper is not usable at its default location.
    if has_missing_marker || has_not_executable_marker || exit_code == Some(127) || exit_code == Some(126) {
        HelperProbeState::Missing
    } else {
        HelperProbeState::Unknown
    }
}

/// Bounded (<=15s), non-installing readiness check for the helper binary.
///
/// Only tests existence (plus executability on POSIX) at `default_location`:
/// it never writes, never installs, and never executes or spawns the helper,
/// so no persistent remote process can linger. Remote platform dispatch and
/// the bounded exec helper mirror `ensure_started`.
pub async fn probe_ready(host: &SshHost, environment: &RemoteEnvironment) -> HelperProbeState {
    let location = match default_location(host, environment) {
        Ok(location) => location,
        Err(_) => return HelperProbeState::Unknown,
    };
    let script = match environment.platform {
        RemotePlatform::Posix => format!(
            "exe={}; \
             if [ ! -f \"$exe\" ]; then printf 'FERRYX_ERR_HELPER_MISSING\\n' >&2; exit 127; fi; \
             if [ ! -x \"$exe\" ]; then printf 'FERRYX_ERR_HELPER_NOT_EXECUTABLE\\n' >&2; exit 126; fi; \
             printf 'FERRYX_HELPER_READY\\n'",
            direct::quote_posix(&location.executable)
        ),
        RemotePlatform::Windows => format!(
            "$exe = {}; \
             if (-not [System.IO.File]::Exists($exe)) {{ [Console]::Error.WriteLine('FERRYX_ERR_HELPER_MISSING'); exit 127; }}; \
             [Console]::WriteLine('FERRYX_HELPER_READY')",
            runtime::powershell_data(&location.executable)
        ),
    };
    let plan = match direct::ssh_plan(host, environment.executor.command(&script), false) {
        Ok(plan) => plan,
        Err(_) => return HelperProbeState::Unknown,
    };
    match direct::bounded_output(&plan, Duration::from_secs(10)).await {
        Ok(stdout) => classify_probe_result(Ok(&stdout)),
        Err(err) => classify_probe_result(Err(&err)),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HelperUpgradeDecision {
    Install,
    NoOp,
    Upgrade,
}

pub fn parse_installed_version_output(stdout: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(stdout).ok()?;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let token = trimmed.split_whitespace().last()?;
        if token.contains('.')
            && token.chars().next().map_or(false, |c| c.is_ascii_digit())
            && token.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '+')
        {
            return Some(token.to_string());
        }
    }
    None
}

pub async fn installed_version(
    host: &SshHost,
    env: &RemoteEnvironment,
) -> Option<String> {
    let location = default_location(host, env).ok()?;
    let script = match env.platform {
        RemotePlatform::Posix => format!(
            "exe={}; \
             if [ ! -f \"$exe\" ] || [ ! -x \"$exe\" ]; then exit 127; fi; \
             exec \"$exe\" --version",
            direct::quote_posix(&location.executable)
        ),
        RemotePlatform::Windows => format!(
            "$exe = {}; \
             if (-not [System.IO.File]::Exists($exe)) {{ exit 127; }}; \
             & $exe --version",
            runtime::powershell_data(&location.executable)
        ),
    };
    let plan = direct::ssh_plan(host, env.executor.command(&script), false).ok()?;
    match direct::bounded_output(&plan, Duration::from_secs(10)).await {
        Ok(stdout) => parse_installed_version_output(&stdout),
        Err(_) => None,
    }
}

pub fn decide_helper_upgrade(
    installed: bool,
    remote_version: Option<&str>,
    bundled_version: &str,
) -> HelperUpgradeDecision {
    if !installed {
        HelperUpgradeDecision::Install
    } else {
        match remote_version {
            Some(remote) if remote == bundled_version => HelperUpgradeDecision::NoOp,
            _ => HelperUpgradeDecision::Upgrade,
        }
    }
}

pub fn chunk_base64(bytes: &[u8], line_length: usize) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let encoded = STANDARD.encode(bytes);
    let mut chunked = String::with_capacity(encoded.len() + (encoded.len() / line_length) * 2);
    for chunk in encoded.as_bytes().chunks(line_length) {
        chunked.push_str(std::str::from_utf8(chunk).unwrap_or(""));
        chunked.push('\n');
    }
    chunked
}

pub fn build_posix_upload_script(location: &HelperLocation, binary_bytes: &[u8]) -> String {
    let chunked = chunk_base64(binary_bytes, 76);
    format!(
        "dest={}; root={}; \
         parent=$(dirname \"$dest\"); mkdir -p \"$parent\" && chmod 700 \"$parent\"; \
         tmp=\"${{dest}}.tmp.$$\"; trap 'rm -f \"$tmp\"' EXIT HUP INT TERM; \
         (base64 -d 2>/dev/null || base64 -D 2>/dev/null || openssl enc -base64 -d 2>/dev/null) << 'FERRYX_HELPER_PAYLOAD_EOF' > \"$tmp\"\n\
{}\
FERRYX_HELPER_PAYLOAD_EOF\n\
         chmod 700 \"$tmp\" && \
         if [ -f \"$root/endpoint.json\" ]; then \
             pid=$(tr -d ' \\t\\r\\n' < \"$root/endpoint.json\" 2>/dev/null | sed -n 's/.*\"pid\":\\([0-9]\\{{1,\\}}\\).*/\\1/p'); \
             if [ -n \"$pid\" ] && [ \"$pid\" -gt 0 ] 2>/dev/null; then kill \"$pid\" 2>/dev/null || true; sleep 0.2; fi; \
         fi; \
         mv -f \"$tmp\" \"$dest\" && printf 'FERRYX_INSTALL_OK\\n'",
        direct::quote_posix(&location.executable),
        direct::quote_posix(&location.root),
        chunked
    )
}

pub fn build_windows_upload_script(location: &HelperLocation, binary_bytes: &[u8]) -> String {
    let chunked = chunk_base64(binary_bytes, 76);
    format!(
        "$ProgressPreference='SilentlyContinue'; $ErrorActionPreference='Stop'; \
         [Console]::OutputEncoding = New-Object Text.UTF8Encoding($false); \
         try {{ \
             $dest = {}; \
             $root = {}; \
             $dir = [System.IO.Path]::GetDirectoryName($dest); \
             if (-not [System.IO.Directory]::Exists($dir)) {{ [System.IO.Directory]::CreateDirectory($dir) | Out-Null }}; \
             $tmp = \"$dest.tmp.\" + [System.Guid]::NewGuid().ToString('N'); \
             $b64 = @'\n\
{}\
'@;\n\
             [System.IO.File]::WriteAllBytes($tmp, [System.Convert]::FromBase64String($b64.Trim())); \
             $aclRes = & icacls $tmp /inheritance:r /grant:r \"$($env:USERNAME):(F)\" 2>&1; \
             if ($LASTEXITCODE -ne 0) {{ throw \"Failed to set private ACL on helper binary: $aclRes\" }}; \
             $epPath = Join-Path $root 'endpoint.json'; \
             if ([System.IO.File]::Exists($epPath)) {{ \
                 try {{ \
                     $ep = Get-Content -Raw $epPath | ConvertFrom-Json; \
                     if ($ep.pid) {{ taskkill /PID $ep.pid /F /T 2>&1 | Out-Null; Start-Sleep -Milliseconds 200; }} \
                 }} catch {{}} \
             }}; \
             $old = \"$dest.old\"; \
             if ([System.IO.File]::Exists($old)) {{ [System.IO.File]::Delete($old); }}; \
             if ([System.IO.File]::Exists($dest)) {{ [System.IO.File]::Move($dest, $old); }}; \
             [System.IO.File]::Move($tmp, $dest); \
             if ([System.IO.File]::Exists($old)) {{ try {{ [System.IO.File]::Delete($old); }} catch {{}} }}; \
             [Console]::WriteLine('FERRYX_INSTALL_OK'); \
         }} catch {{ \
             [Console]::Error.Write($_.Exception.Message); \
             exit 1; \
         }}",
        runtime::powershell_data(&location.executable),
        runtime::powershell_data(&location.root),
        chunked
    )
}

pub async fn provision(
    host: &SshHost,
    env: &RemoteEnvironment,
    local_binary: &Path,
) -> Result<HelperLocation, IpcError> {
    let location = default_location(host, env)?;
    install(host, env, &location, local_binary).await?;
    ensure_started(host, env, &location).await?;
    Ok(location)
}

#[cfg(test)]
#[path = "helper_setup_tests.rs"]
mod tests;
