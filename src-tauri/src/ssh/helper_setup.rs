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

    let script = match env.platform {
        RemotePlatform::Posix => format!(
            "dest={}; parent=$(dirname \"$dest\"); mkdir -p \"$parent\" && chmod 700 \"$parent\"; \
             tmp=\"${{dest}}.tmp.$$\"; trap 'rm -f \"$tmp\"' EXIT HUP INT TERM; \
             cat > \"$tmp\" && chmod 700 \"$tmp\" && mv -f \"$tmp\" \"$dest\"",
            direct::quote_posix(&location.executable)
        ),
        RemotePlatform::Windows => format!(
            "$dest = {}; $dir = [System.IO.Path]::GetDirectoryName($dest); \
             if (-not [System.IO.Directory]::Exists($dir)) {{ [System.IO.Directory]::CreateDirectory($dir) | Out-Null }}; \
             $tmp = \"$dest.tmp.\" + [System.Guid]::NewGuid().ToString('N'); \
             $backup = $null; \
             try {{ \
                 $in = [System.Console]::OpenStandardInput(); \
                 $file = [System.IO.File]::Create($tmp); \
                 $in.CopyTo($file); \
                 $file.Close(); \
                 $aclRes = & icacls $tmp /inheritance:r /grant:r \"$($env:USERNAME):(F)\" 2>&1; \
                 if ($LASTEXITCODE -ne 0) {{ throw \"Failed to set private ACL on helper binary: $aclRes\" }}; \
                 if ([System.IO.File]::Exists($dest)) {{ \
                     $backup = \"$dest.bak.\" + [System.Guid]::NewGuid().ToString('N'); \
                     [System.IO.File]::Move($dest, $backup); \
                 }}; \
                 [System.IO.File]::Move($tmp, $dest); \
                 if ($backup -and [System.IO.File]::Exists($backup)) {{ [System.IO.File]::Delete($backup); }} \
             }} catch {{ \
                 if ($backup -and [System.IO.File]::Exists($backup)) {{ \
                     try {{ [System.IO.File]::Move($backup, $dest) }} catch {{}} \
                 }}; \
                 throw \
             }} finally {{ \
                 if ([System.IO.File]::Exists($tmp)) {{ [System.IO.File]::Delete($tmp) }} \
             }}",
            runtime::powershell_data(&location.executable)
        ),
    };

    let plan = direct::ssh_plan(host, env.executor.command(&script), false)?;
    direct::bounded_output_with_stdin(&plan, Duration::from_secs(60), binary_data)
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

    let is_missing = exit_code == Some(127)
        || stderr.contains("FERRYX_ERR_HELPER_MISSING")
        || err.message.contains("FERRYX_ERR_HELPER_MISSING");

    let is_permissions = exit_code == Some(126)
        || stderr.contains("FERRYX_ERR_HELPER_NOT_EXECUTABLE")
        || err.message.contains("FERRYX_ERR_HELPER_NOT_EXECUTABLE");

    if is_missing {
        IpcError::new(
            IpcErrorCode::CliExecutableNotFound,
            format!(
                "Remote helper binary is not installed at '{}'. Run install to set up the helper.",
                location.executable
            ),
        )
        .with_details(serde_json::json!({
            "stage": "helper_missing",
            "executable": location.executable,
            "root": location.root,
        }))
    } else if is_permissions {
        IpcError::new(
            IpcErrorCode::Unsupported,
            format!(
                "Remote helper binary at '{}' is not executable. Please verify permissions.",
                location.executable
            ),
        )
        .with_details(serde_json::json!({
            "stage": "helper_permissions",
            "executable": location.executable,
            "root": location.root,
        }))
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

#[cfg(test)]
#[path = "helper_setup_tests.rs"]
mod tests;
