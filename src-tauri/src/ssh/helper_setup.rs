//! Remote helper installation and SSH-independent startup lifecycle.

use crate::ipc::{IpcError, IpcErrorCode};
use crate::ssh::direct;
use crate::ssh::runtime::{self, RemoteEnvironment, RemoteExecutor, RemotePlatform};
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

/// Stdout marker printed by the upload script when installation succeeded.
pub const INSTALL_READY_MARKER: &str = "FERRYX_INSTALL_OK";

/// Stdout marker prefix printed when helper binary replacement was deferred.
pub const INSTALL_DEFERRED_MARKER: &str = "FERRYX_INSTALL_DEFERRED:";

/// Stderr marker printed by the POSIX base64 upload script when the remote has neither a
/// `base64` decoder (`-d`/`-D`) nor `openssl enc -base64 -d`; `install` then retries with the
/// octal (`od`-format) payload that only needs the shell's `printf`.
pub const UPLOAD_DECODER_MISSING_MARKER: &str = "FERRYX_ERR_HELPER_DECODER_MISSING";

/// Stderr marker printed when the uploaded file does not hash to the local helper's sha256.
pub const UPLOAD_CHECKSUM_MARKER: &str = "FERRYX_ERR_HELPER_CHECKSUM";

/// Stderr marker printed when no sha256 tool exists and the uploaded byte count is wrong.
pub const UPLOAD_SIZE_MARKER: &str = "FERRYX_ERR_HELPER_SIZE";

/// Decoders the POSIX base64 upload looks for, named in the error when none exists.
const UPLOAD_DECODERS: [&str; 2] = ["base64", "openssl"];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeferredPayload {
    reason: String,
    #[serde(default)]
    pid: Option<u32>,
    #[serde(default)]
    running_version: Option<String>,
    #[serde(default)]
    installed_version: Option<String>,
}

/// Honest Terminal Helper readiness from a bounded, non-installing remote check.
///
/// `Installed` means the helper binary was observed present (and executable on
/// POSIX) and answered `--version`. `Missing` means the remote explicitly
/// reported the binary absent, not executable, or unrunnable. `Unknown` covers every other
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

/// Stderr marker printed by the probe script when the binary exists but `--version` fails.
const PROBE_UNRUNNABLE_MARKER: &str = "FERRYX_ERR_HELPER_UNRUNNABLE";

#[derive(Debug, Deserialize)]
struct HelperReadyEvent {
    event: String,
    protocol: u32,
}

/// 32 lowercase hex characters of the host-id digest: the collision-resistant
/// value [`host_slug`] appends, reused as the compact runtime-root component so
/// the helper's socket path stays inside the Unix socket limit (`SUN_LEN`).
pub(crate) fn host_digest(host_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(host_id.as_bytes());
    let digest = hasher.finalize();
    let hex_hash: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
    hex_hash[..32].to_string()
}

pub(crate) fn host_slug(host_id: &str) -> String {
    let hex_hash = host_digest(host_id);
    let safe_prefix: String = host_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(16)
        .collect();
    if safe_prefix.is_empty() {
        format!("host-{hex_hash}")
    } else {
        format!("{safe_prefix}-{hex_hash}")
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

pub fn parse_install_output(stdout: &[u8], location: &HelperLocation) -> Result<(), IpcError> {
    let text = std::str::from_utf8(stdout).map_err(|_| {
        IpcError::new(
            IpcErrorCode::IoError,
            "Remote helper installation output is not valid UTF-8",
        )
    })?;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix(INSTALL_DEFERRED_MARKER) {
            let rest = rest.trim();
            let payload: DeferredPayload = serde_json::from_str(rest).map_err(|e| {
                IpcError::new(
                    IpcErrorCode::ParseError,
                    format!("Malformed helper deferral payload: {e}"),
                )
                .with_details(serde_json::json!({
                    "stage": "install",
                    "output": trimmed,
                }))
            })?;

            if payload.reason != "live_daemon" {
                return Err(IpcError::new(
                    IpcErrorCode::InternalError,
                    format!("Unrecognized helper deferral reason: {}", payload.reason),
                )
                .with_details(serde_json::json!({
                    "stage": "install",
                    "reason": payload.reason,
                    "output": trimmed,
                })));
            }

            let pid_str = payload
                .pid
                .map(|p| format!(" (PID {p})"))
                .unwrap_or_default();
            let running = payload.running_version.as_deref().unwrap_or("unknown");
            let installed = payload.installed_version.as_deref().unwrap_or("unknown");

            return Err(IpcError::new(
                IpcErrorCode::Unsupported,
                format!(
                    "Remote helper upgrade deferred: helper daemon is already running{pid_str} (running version: {running}, installed version: {installed}). Existing sessions remain active and usable without interruption.",
                ),
            )
            .with_details(serde_json::json!({
                "stage": "helper_upgrade_deferred",
                "reason": "live_daemon",
                "pid": payload.pid,
                "runningVersion": payload.running_version,
                "installedVersion": payload.installed_version,
                "executable": location.executable,
                "root": location.root,
            })));
        }
        if trimmed == INSTALL_READY_MARKER {
            return Ok(());
        }
    }

    Err(IpcError::new(
        IpcErrorCode::IoError,
        "Remote helper install script did not produce a valid installation marker",
    )
    .with_details(serde_json::json!({
        "stage": "install",
        "output": text.trim(),
    })))
}

/// Raw stdin-mode PowerShell invocation for Windows helper uploads.
///
/// [`RemoteExecutor::command`] wraps its argument as an `-EncodedCommand`
/// script, so passing "-Command -" through it makes the remote shell decode
/// and *execute a program named* `-Command` (exit 1: the term is not
/// recognized). The upload script itself arrives on stdin, which is exactly
/// what the POSIX arm's `sh -c 'sh -s'` achieves.
fn windows_stdin_command(executor: RemoteExecutor) -> String {
    format!(
        "{} -NoLogo -NoProfile -NonInteractive -Command -",
        executor.program()
    )
}

/// Pure pairing of the upload transport command with the payload bytes for
/// one platform, so tests can bind `install` to a stdin-mode command without
/// touching SSH.
fn install_invocation(
    env: &RemoteEnvironment,
    location: &HelperLocation,
    binary_data: &[u8],
) -> (String, Vec<u8>) {
    match env.platform {
        RemotePlatform::Posix => {
            let script = build_posix_upload_script(location, binary_data);
            (env.executor.command("sh -s"), script.into_bytes())
        }
        RemotePlatform::Windows => {
            let script = build_windows_upload_script(location, binary_data);
            (windows_stdin_command(env.executor), script.into_bytes())
        }
    }
}

fn error_stderr(err: &IpcError) -> &str {
    err.details
        .as_ref()
        .and_then(|d| d.get("stderr"))
        .and_then(|s| s.as_str())
        .unwrap_or("")
}

/// True when the upload failed only because the remote has no base64 decoder.
pub(crate) fn upload_decoder_missing(err: &IpcError) -> bool {
    error_stderr(err).contains(UPLOAD_DECODER_MISSING_MARKER)
        || err.message.contains(UPLOAD_DECODER_MISSING_MARKER)
}

/// Tags an upload failure with its stage and, when the remote verification rejected the
/// uploaded bytes, the machine-readable reason.
fn tag_install_error(mut err: IpcError, location: &HelperLocation) -> IpcError {
    let text = format!("{} {}", err.message, error_stderr(&err));
    let reason = if text.contains(UPLOAD_CHECKSUM_MARKER) {
        Some("checksum_mismatch")
    } else if text.contains(UPLOAD_SIZE_MARKER) {
        Some("size_mismatch")
    } else {
        None
    };
    let details = err.details.get_or_insert_with(|| serde_json::json!({}));
    details["stage"] = "install".into();
    details["executable"] = location.executable.clone().into();
    if let Some(reason) = reason {
        details["reason"] = reason.into();
    }
    err
}

/// Uploads `binary_data` through `run(command, stdin, deadline)`, with a deadline scaled to the
/// payload size. A POSIX remote without any base64 decoder answers the first attempt with
/// [`UPLOAD_DECODER_MISSING_MARKER`]; the upload is then repeated once with the octal payload.
pub(super) async fn install_with<F, Fut>(
    env: &RemoteEnvironment,
    location: &HelperLocation,
    binary_data: &[u8],
    mut run: F,
) -> Result<(), IpcError>
where
    F: FnMut(String, Vec<u8>, Duration) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<u8>, IpcError>>,
{
    let (cmd, input_bytes) = install_invocation(env, location, binary_data);
    let deadline = super::operations::upload_deadline_for_bytes(input_bytes.len());
    let output = match run(cmd, input_bytes, deadline).await {
        Ok(output) => output,
        Err(err) if env.platform == RemotePlatform::Posix && upload_decoder_missing(&err) => {
            tracing::warn!(
                executable = %location.executable,
                "Remote host has no base64 decoder (tried {}); uploading the helper as octal through printf",
                UPLOAD_DECODERS.join(", ")
            );
            let script = build_posix_octal_upload_script(location, binary_data);
            let input_bytes = script.into_bytes();
            let deadline = super::operations::upload_deadline_for_bytes(input_bytes.len());
            run(env.executor.command("sh -s"), input_bytes, deadline)
                .await
                .map_err(|mut err| {
                    err.message = format!(
                        "Remote host has no base64 decoder ({} missing) and the printf fallback upload failed: {}",
                        UPLOAD_DECODERS.join(" and "),
                        err.message
                    );
                    let details = err.details.get_or_insert_with(|| serde_json::json!({}));
                    details["decodersMissing"] = serde_json::json!(UPLOAD_DECODERS);
                    tag_install_error(err, location)
                })?
        }
        Err(err) => return Err(tag_install_error(err, location)),
    };

    parse_install_output(&output, location)
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
                format!(
                    "Local helper binary not accessible at {}: {}",
                    local_path.display(),
                    e
                ),
            )
        })?;
        if !metadata.is_file() {
            return Err(IpcError::new(
                IpcErrorCode::InvalidPath,
                format!(
                    "Local helper binary path {} is not a regular file",
                    local_path.display()
                ),
            ));
        }
        let bytes = std::fs::read(&local_path).map_err(|e| {
            IpcError::new(
                IpcErrorCode::IoError,
                format!(
                    "Failed to read local helper binary from {}: {}",
                    local_path.display(),
                    e
                ),
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
    .map_err(|e| {
        IpcError::new(
            IpcErrorCode::InternalError,
            format!("Join error in blocking offload: {e}"),
        )
    })??;

    install_with(
        env,
        location,
        &binary_data,
        |cmd, input, deadline| async move {
            let plan = direct::ssh_plan(host, cmd, false)?;
            direct::bounded_output_with_stdin(&plan, deadline, input).await
        },
    )
    .await
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
        "operation": "helper_startup_readiness",
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
    if (has_missing_marker && (has_not_executable_marker || exit_code == Some(126)))
        || (exit_code == Some(127) && has_not_executable_marker)
    {
        return err;
    }

    // Exit 127 alone is not proof of absence: a shell also returns 127 when the binary exists
    // but its ELF interpreter does not (a glibc helper on musl/Alpine). Only the explicit marker
    // printed after the existence check means "not installed".
    let is_missing = has_missing_marker && matches!(exit_code, Some(127) | Some(1));
    let is_permissions =
        exit_code == Some(126) || (exit_code == Some(1) && has_not_executable_marker);
    let is_exec_failure = exit_code == Some(127) && !has_missing_marker;

    let cause = err.details.clone();

    if is_exec_failure {
        let mut details = serde_json::json!({
            "stage": "helper_exec_failed",
            "operation": "helper_startup_readiness",
            "executable": location.executable,
            "root": location.root,
            "exitCode": 127,
            "stderr": stderr,
        });
        if let Some(c) = cause {
            details["cause"] = c;
        }
        IpcError::new(
            IpcErrorCode::Unsupported,
            format!(
                "Remote helper at '{}' exists but could not be executed on this host: {}",
                location.executable,
                stderr.trim()
            ),
        )
        .with_details(details)
    } else if is_missing {
        let mut details = serde_json::json!({
            "stage": "helper_missing",
            "operation": "helper_startup_readiness",
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
            "operation": "helper_startup_readiness",
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

    let script = ensure_started_script(host, env, location);
    let plan = direct::ssh_plan(host, env.executor.command(&script), false)?;
    let output = direct::bounded_output(&plan, Duration::from_secs(15))
        .await
        .map_err(|mut err| {
            let details = err.details.get_or_insert_with(|| serde_json::json!({}));
            details["operation"] = "helper_startup_readiness".into();
            err
        });

    match output {
        Ok(stdout_bytes) => parse_ready_output(&stdout_bytes).map_err(|mut err| {
            let details = err.details.get_or_insert_with(|| serde_json::json!({}));
            details["operation"] = "helper_startup_readiness".into();
            err
        }),
        Err(err) => Err(map_ensure_started_error(err, location)),
    }
}

/// The remote start script. For a version-qualified location it also prunes, after a successful
/// start only, the helper versions older than the newest two (see [`posix_prune_versions_script`]).
pub(crate) fn ensure_started_script(
    host: &SshHost,
    env: &RemoteEnvironment,
    location: &HelperLocation,
) -> String {
    let qualified = qualified_version_parts(env.platform, location);
    match env.platform {
        RemotePlatform::Posix => {
            let start = match &qualified {
                Some((versions, tag)) => format!(
                    "\"$exe\" start --root \"$root\" --host-id \"$host_id\"; rc=$?; \
                     if [ \"$rc\" -eq 0 ]; then ( {} ) >/dev/null 2>&1 </dev/null; fi; exit \"$rc\"",
                    posix_prune_versions_script(versions, tag)
                ),
                None => "exec \"$exe\" start --root \"$root\" --host-id \"$host_id\"".to_string(),
            };
            format!(
                "exe={}; root={}; host_id={}; \
                 if [ ! -f \"$exe\" ]; then printf 'FERRYX_ERR_HELPER_MISSING\\n' >&2; exit 127; fi; \
                 if [ ! -x \"$exe\" ]; then printf 'FERRYX_ERR_HELPER_NOT_EXECUTABLE\\n' >&2; exit 126; fi; \
                 {start}",
                direct::quote_posix(&location.executable),
                direct::quote_posix(&location.root),
                direct::quote_posix(&host.id)
            )
        }
        RemotePlatform::Windows => {
            let prune = match &qualified {
                Some((versions, tag)) => format!(
                    "if ($rc -eq 0) {{ try {{ {} }} catch {{}} }}; ",
                    windows_prune_versions_script(versions, tag)
                ),
                None => String::new(),
            };
            format!(
                "$exe = {}; $root = {}; $hostId = {}; \
                 if (-not [System.IO.File]::Exists($exe)) {{ [Console]::Error.WriteLine('FERRYX_ERR_HELPER_MISSING'); exit 127; }}; \
                 & $exe start --root $root --host-id $hostId 2>&1 | ForEach-Object {{ \
                     if ($_ -is [System.Management.Automation.ErrorRecord]) {{ [Console]::Error.WriteLine($_.ToString()) }} \
                     else {{ [Console]::Out.WriteLine($_.ToString()) }} \
                 }}; $rc = $LASTEXITCODE; {prune}exit $rc",
                runtime::powershell_data(&location.executable),
                runtime::powershell_data(&location.root),
                runtime::powershell_data(&host.id)
            )
        }
    }
}

/// `(versions dir, version tag)` when `location` is a [`qualified_location`]
/// (`<home>/.ferryx/versions/<calver>/bin/ferryx-remote-helper[.exe]`), else `None`.
pub(crate) fn qualified_version_parts(
    platform: RemotePlatform,
    location: &HelperLocation,
) -> Option<(String, String)> {
    let (sep, suffix, marker) = match platform {
        RemotePlatform::Posix => ('/', "/bin/ferryx-remote-helper", "/.ferryx/versions"),
        RemotePlatform::Windows => (
            '\\',
            "\\bin\\ferryx-remote-helper.exe",
            "\\.ferryx\\versions",
        ),
    };
    let version_dir = location.executable.strip_suffix(suffix)?;
    let (versions, tag) = version_dir.rsplit_once(sep)?;
    if !versions.ends_with(marker) || parse_calver_version(tag).is_none() {
        return None;
    }
    if tag.split('.').count() != 3 {
        return None;
    }
    Some((versions.to_string(), tag.to_string()))
}

/// Removes `<versions>/<tag>` directories older than the newest two calver tags, never the
/// `current` one, and never one that is still running: a process whose executable (`/proc/*/exe`)
/// or command line lies inside it, or a live pid in one of that version's runtime roots
/// (`<home>/.ferryx/r/<tag>/*/endpoint.json`), keeps the version on disk. Best effort: every
/// failure leaves the directory in place.
pub fn posix_prune_versions_script(versions: &str, current: &str) -> String {
    format!(
        "vb={}; cur={}; fx=\"${{vb%/*}}\"; \
         tags=$(ls \"$vb\" 2>/dev/null | grep -E '^[0-9]+\\.[0-9]+\\.[0-9]+$' | sort -t. -k1,1n -k2,2n -k3,3n); \
         n=0; for t in $tags; do n=$((n+1)); done; \
         procs=$(ps -A -o args= 2>/dev/null || ps -o args 2>/dev/null); \
         i=0; \
         for t in $tags; do \
             i=$((i+1)); [ \"$i\" -le $((n-2)) ] || break; \
             [ \"$t\" = \"$cur\" ] && continue; \
             d=\"$vb/$t\"; if [ ! -d \"$d\" ] || [ -L \"$d\" ]; then continue; fi; \
             live=0; \
             for p in /proc/[0-9]*; do l=$(readlink \"$p/exe\" 2>/dev/null) || continue; case \"$l\" in \"$d\"/*) live=1; break;; esac; done; \
             case \"$procs\" in *\"$d/\"*) live=1;; esac; \
             for ep in \"$fx/r/$t\"/*/endpoint.json; do \
                 [ -f \"$ep\" ] || continue; \
                 pid=$(tr -d ' \\t\\r\\n' < \"$ep\" 2>/dev/null | sed -n 's/.*\"pid\":\\([0-9]\\{{1,\\}}\\).*/\\1/p'); \
                 if [ -n \"$pid\" ] && kill -0 \"$pid\" 2>/dev/null; then live=1; fi; \
             done; \
             [ \"$live\" -eq 0 ] && rm -rf \"$d\"; \
         done; :",
        direct::quote_posix(versions),
        direct::quote_posix(current)
    )
}

/// Windows counterpart of [`posix_prune_versions_script`]: a version whose directory holds the
/// image of a running `ferryx-remote-helper` process is kept (Windows also refuses to delete a
/// running executable, so a failed removal leaves the directory in place).
pub(crate) fn windows_prune_versions_script(versions: &str, current: &str) -> String {
    format!(
        "$vb = {}; $cur = {}; \
         $tags = @(Get-ChildItem -LiteralPath $vb -Directory -ErrorAction SilentlyContinue | \
             Where-Object {{ $_.Name -match '^\\d+\\.\\d+\\.\\d+$' -and -not ($_.Attributes -band [System.IO.FileAttributes]::ReparsePoint) }} | \
             Sort-Object {{ [version]$_.Name }}); \
         $procs = @(Get-Process -Name 'ferryx-remote-helper' -ErrorAction SilentlyContinue | ForEach-Object {{ try {{ $_.Path }} catch {{ $null }} }} | Where-Object {{ $_ }}); \
         for ($i = 0; $i -lt ($tags.Count - 2); $i++) {{ \
             $d = $tags[$i]; \
             if ($d.Name -eq $cur) {{ continue }}; \
             $prefix = $d.FullName.TrimEnd('\\') + '\\'; \
             if (@($procs | Where-Object {{ $_.StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase) }}).Count -gt 0) {{ continue }}; \
             Remove-Item -LiteralPath $d.FullName -Recurse -Force -ErrorAction SilentlyContinue \
         }}",
        runtime::powershell_data(versions),
        runtime::powershell_data(current)
    )
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
    let has_unrunnable_marker =
        stderr.contains(PROBE_UNRUNNABLE_MARKER) || err.message.contains(PROBE_UNRUNNABLE_MARKER);

    // Contradictory signals are untrustworthy: claim nothing.
    if (has_missing_marker && (has_not_executable_marker || exit_code == Some(126)))
        || (exit_code == Some(127) && has_not_executable_marker)
    {
        return HelperProbeState::Unknown;
    }

    // Windows sshd.exe normalizes child exit codes to 1, so the explicit
    // stderr sentinels are the reliable signal there; POSIX adds 127/126.
    // Every sentinel means the helper is not usable at its default location;
    // an unrunnable binary must be replaced just like a missing one.
    if has_missing_marker
        || has_not_executable_marker
        || has_unrunnable_marker
        || exit_code == Some(127)
        || exit_code == Some(126)
    {
        HelperProbeState::Missing
    } else {
        HelperProbeState::Unknown
    }
}

pub fn probe_locations(
    host: &SshHost,
    environment: &RemoteEnvironment,
) -> Result<Vec<HelperLocation>, IpcError> {
    let mut locations = Vec::with_capacity(2);
    if let Ok(qualified) = qualified_location(
        host,
        environment,
        crate::ssh::helper_runtime::process::HELPER_VERSION,
    ) {
        locations.push(qualified);
    }
    locations.push(default_location(host, environment)?);
    Ok(locations)
}

/// Bounded (<=15s), non-installing readiness check for the helper binary.
///
/// Tests existence (plus executability on POSIX) at candidate locations:
/// first checks `qualified_location` for the current bundled helper version,
/// falling back to unversioned `default_location` for legacy installs.
/// It never writes or installs. In the same round trip it runs `--version`,
/// which exits immediately and spawns nothing persistent, because a present
/// binary can still be unrunnable (a glibc build on a musl host exits 127):
/// that case prints `FERRYX_ERR_HELPER_UNRUNNABLE` and reads as Missing so the
/// caller reinstalls. Remote platform dispatch and the bounded exec helper
/// mirror `ensure_started`.
fn probe_ready_script(environment: &RemoteEnvironment, location: &HelperLocation) -> String {
    match environment.platform {
        RemotePlatform::Posix => format!(
            "exe={}; \
             if [ ! -f \"$exe\" ]; then printf 'FERRYX_ERR_HELPER_MISSING\\n' >&2; exit 127; fi; \
             if [ ! -x \"$exe\" ]; then printf 'FERRYX_ERR_HELPER_NOT_EXECUTABLE\\n' >&2; exit 126; fi; \
             out=$(\"$exe\" --version 2>&1); rc=$?; \
             if [ \"$rc\" -ne 0 ]; then printf 'FERRYX_ERR_HELPER_UNRUNNABLE exit=%s %s\\n' \"$rc\" \"$(printf '%s\\n' \"$out\" | head -n 1)\" >&2; exit 1; fi; \
             printf 'FERRYX_HELPER_READY\\n'",
            direct::quote_posix(&location.executable)
        ),
        RemotePlatform::Windows => format!(
            "$exe = {}; \
             if (-not [System.IO.File]::Exists($exe)) {{ [Console]::Error.WriteLine('FERRYX_ERR_HELPER_MISSING'); exit 127; }}; \
             try {{ $out = (& $exe --version 2>&1 | Out-String).Trim(); $rc = $LASTEXITCODE }} catch {{ $out = $_.Exception.Message; $rc = -1 }}; \
             if ($rc -ne 0) {{ [Console]::Error.WriteLine('FERRYX_ERR_HELPER_UNRUNNABLE exit=' + $rc + ' ' + ($out -split '\\r?\\n')[0]); exit 1; }}; \
             [Console]::WriteLine('FERRYX_HELPER_READY')",
            runtime::powershell_data(&location.executable)
        ),
    }
}

/// User-facing reason a present helper failed its probe `--version` run, taken from the
/// `FERRYX_ERR_HELPER_UNRUNNABLE exit=<code> <first output line>` sentinel. `None` when the
/// probe did not observe a present-but-unrunnable binary (absent, not executable, transport).
pub(crate) fn probe_exec_failure(err: &IpcError, location: &HelperLocation) -> Option<String> {
    if classify_probe_error(err) != HelperProbeState::Missing {
        return None;
    }
    let stderr = err
        .details
        .as_ref()
        .and_then(|d| d.get("stderr"))
        .and_then(|s| s.as_str())
        .unwrap_or("");
    let detail = stderr.lines().chain(err.message.lines()).find_map(|line| {
        line.split_once(PROBE_UNRUNNABLE_MARKER)
            .map(|(_, rest)| rest.trim().to_owned())
    })?;
    Some(format!(
        "Remote helper at '{}' exists but could not be executed on this host ({detail}); reinstall it.",
        location.executable
    ))
}

async fn probe_ready_at_detailed_with<F, Fut>(
    environment: &RemoteEnvironment,
    location: &HelperLocation,
    mut run: F,
) -> (HelperProbeState, Option<String>)
where
    F: FnMut(RemoteExecutor, String, Duration) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<u8>, IpcError>>,
{
    let script = probe_ready_script(environment, location);
    match run(environment.executor, script, Duration::from_secs(10)).await {
        Ok(stdout) => (classify_probe_result(Ok(&stdout)), None),
        Err(err) => (
            classify_probe_result(Err(&err)),
            probe_exec_failure(&err, location),
        ),
    }
}

pub(super) async fn probe_ready_at_with<F, Fut>(
    environment: &RemoteEnvironment,
    location: &HelperLocation,
    run: F,
) -> HelperProbeState
where
    F: FnMut(RemoteExecutor, String, Duration) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<u8>, IpcError>>,
{
    probe_ready_at_detailed_with(environment, location, run)
        .await
        .0
}

pub(super) async fn probe_ready_detailed_with<F, Fut>(
    host: &SshHost,
    environment: &RemoteEnvironment,
    mut run: F,
) -> (HelperProbeState, Option<String>)
where
    F: FnMut(RemoteExecutor, String, Duration) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<u8>, IpcError>>,
{
    let locations = match probe_locations(host, environment) {
        Ok(locs) => locs,
        Err(_) => return (HelperProbeState::Unknown, None),
    };
    let mut last_state = HelperProbeState::Missing;
    let mut exec_failure = None;
    for location in &locations {
        let (state, failure) = probe_ready_at_detailed_with(environment, location, &mut run).await;
        if state == HelperProbeState::Installed {
            return (HelperProbeState::Installed, None);
        }
        last_state = state;
        exec_failure = exec_failure.or(failure);
    }
    (last_state, exec_failure)
}

pub async fn probe_ready(host: &SshHost, environment: &RemoteEnvironment) -> HelperProbeState {
    probe_ready_detailed(host, environment).await.0
}

/// `probe_ready` plus the exec-failure reason when a present helper binary could not run
/// (see [`probe_exec_failure`]); same single round trip per location.
pub async fn probe_ready_detailed(
    host: &SshHost,
    environment: &RemoteEnvironment,
) -> (HelperProbeState, Option<String>) {
    probe_ready_detailed_with(host, environment, |executor, script, timeout| async move {
        let plan = direct::ssh_plan(host, executor.command(&script), false)?;
        direct::bounded_output(&plan, timeout)
            .await
            .map_err(|mut err| {
                let details = err.details.get_or_insert_with(|| serde_json::json!({}));
                details["operation"] = "helper_readiness_probe".into();
                details["executor"] = executor.program().into();
                err
            })
    })
    .await
}

pub async fn probe_ready_at(
    host: &SshHost,
    environment: &RemoteEnvironment,
    location: &HelperLocation,
) -> HelperProbeState {
    probe_ready_at_with(
        environment,
        location,
        |executor, script, timeout| async move {
            let plan = direct::ssh_plan(host, executor.command(&script), false)?;
            direct::bounded_output(&plan, timeout)
                .await
                .map_err(|mut err| {
                    let details = err.details.get_or_insert_with(|| serde_json::json!({}));
                    details["operation"] = "helper_readiness_probe".into();
                    details["executor"] = executor.program().into();
                    err
                })
        },
    )
    .await
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
            && token.chars().next().is_some_and(|c| c.is_ascii_digit())
            && token
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '+')
        {
            return Some(token.to_string());
        }
    }
    None
}

fn installed_version_script(env: &RemoteEnvironment, location: &HelperLocation) -> String {
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
    script
}

pub(super) async fn installed_version_at_with<F, Fut>(
    env: &RemoteEnvironment,
    location: &HelperLocation,
    mut run: F,
) -> Option<String>
where
    F: FnMut(RemoteExecutor, String, Duration) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<u8>, IpcError>>,
{
    let script = installed_version_script(env, location);
    match run(env.executor, script, Duration::from_secs(10)).await {
        Ok(stdout) => parse_installed_version_output(&stdout),
        Err(_) => None,
    }
}

pub(super) async fn installed_version_with<F, Fut>(
    host: &SshHost,
    env: &RemoteEnvironment,
    mut run: F,
) -> Option<String>
where
    F: FnMut(RemoteExecutor, String, Duration) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<u8>, IpcError>>,
{
    let locations = probe_locations(host, env).ok()?;
    for location in &locations {
        if let Some(version) = installed_version_at_with(env, location, &mut run).await {
            return Some(version);
        }
    }
    None
}

pub async fn installed_version_at(
    host: &SshHost,
    env: &RemoteEnvironment,
    location: &HelperLocation,
) -> Option<String> {
    installed_version_at_with(env, location, |executor, script, timeout| async move {
        let plan = direct::ssh_plan(host, executor.command(&script), false)?;
        direct::bounded_output(&plan, timeout).await
    })
    .await
}

pub async fn installed_version(host: &SshHost, env: &RemoteEnvironment) -> Option<String> {
    installed_version_with(host, env, |executor, script, timeout| async move {
        let plan = direct::ssh_plan(host, executor.command(&script), false)?;
        direct::bounded_output(&plan, timeout).await
    })
    .await
}

/// Parses a calver-style version like "2026.917.1" into comparable numeric parts.
/// Returns None for anything that is not at least three all-numeric segments.
pub fn parse_calver_version(version: &str) -> Option<(u64, u64, u64)> {
    let parts: Vec<u64> = version
        .split('.')
        .map(|part| part.parse::<u64>().ok())
        .collect::<Option<Vec<_>>>()?;
    if parts.len() < 3 {
        return None;
    }
    Some((parts[0], parts[1], parts[2]))
}

pub fn decide_helper_upgrade(
    installed: bool,
    remote_version: Option<&str>,
    bundled_version: &str,
) -> HelperUpgradeDecision {
    if !installed {
        return HelperUpgradeDecision::Install;
    }
    match remote_version {
        Some(remote) if remote == bundled_version => HelperUpgradeDecision::NoOp,
        Some(remote) => {
            // Calver ordering: a remote already newer than (or equal to) the
            // local bundle must never be offered a downgrade.
            match (
                parse_calver_version(remote),
                parse_calver_version(bundled_version),
            ) {
                (Some(remote_cal), Some(bundled_cal)) if remote_cal >= bundled_cal => {
                    HelperUpgradeDecision::NoOp
                }
                _ => HelperUpgradeDecision::Upgrade,
            }
        }
        None => HelperUpgradeDecision::Upgrade,
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

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Base64 upload: the decoder is chosen by decoding a probe value first, so a remote without
/// any decoder still consumes the payload and then reports [`UPLOAD_DECODER_MISSING_MARKER`]
/// (exit 3) instead of leaving a partial file.
pub fn build_posix_upload_script(location: &HelperLocation, binary_bytes: &[u8]) -> String {
    let chunked = chunk_base64(binary_bytes, 76);
    let decode = format!(
        "dec=''; \
         if [ \"$(printf 'QQ==\\n' | base64 -d 2>/dev/null)\" = A ]; then dec='base64 -d'; \
         elif [ \"$(printf 'QQ==\\n' | base64 -D 2>/dev/null)\" = A ]; then dec='base64 -D'; \
         elif [ \"$(printf 'QQ==\\n' | openssl enc -base64 -d 2>/dev/null)\" = A ]; then dec='openssl enc -base64 -d'; fi; \
         {{ if [ -n \"$dec\" ]; then $dec; else :; fi; }} << 'FERRYX_HELPER_PAYLOAD_EOF' > \"$tmp\"\n\
{chunked}\
FERRYX_HELPER_PAYLOAD_EOF\n\
         decode_ok=$?; \
         if [ -z \"$dec\" ]; then rm -f \"$tmp\"; \
             printf '{UPLOAD_DECODER_MISSING_MARKER}: no base64 decoder on this host (tried base64 -d, base64 -D, openssl enc -base64 -d)\\n' >&2; exit 3; fi; "
    );
    posix_upload_script(location, binary_bytes, &decode)
}

/// Payload bytes per `printf` line of the octal upload.
const OCTAL_UPLOAD_LINE_BYTES: usize = 1024;

/// Pure-shell fallback for remotes without base64 or openssl: every byte travels as a
/// three-digit octal escape (the `od -b` format) inside `printf` format strings, which every
/// POSIX shell's `printf` decodes. Each line is parsed and executed on its own, so the shell
/// never holds the whole payload. Four bytes on the wire per payload byte.
pub fn build_posix_octal_upload_script(location: &HelperLocation, binary_bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut decode = String::with_capacity(binary_bytes.len() * 4 + binary_bytes.len() / 32 + 64);
    decode.push_str("decode_ok=0; exec 3> \"$tmp\"\n");
    for chunk in binary_bytes.chunks(OCTAL_UPLOAD_LINE_BYTES) {
        decode.push_str("printf '");
        for byte in chunk {
            let _ = write!(decode, "\\{byte:03o}");
        }
        decode.push_str("' >&3 || decode_ok=1\n");
    }
    decode.push_str("exec 3>&-; ");
    posix_upload_script(location, binary_bytes, &decode)
}

/// The upload body shared by both payload encodings. `decode` writes the payload to `$tmp` and
/// sets `decode_ok`; the result is then checked against the local sha256 (or, on a remote with
/// no sha256 tool, the byte count) before anything is moved into place.
fn posix_upload_script(location: &HelperLocation, binary_bytes: &[u8], decode: &str) -> String {
    format!(
        "dest={}; root={}; want={}; want_len={}; \
         parent=$(dirname \"$dest\"); mkdir -p \"$parent\" && chmod 700 \"$parent\"; \
         tmp=\"${{dest}}.tmp.$$\"; trap 'rm -f \"$tmp\"' EXIT HUP INT TERM; \
         {decode}\
         if [ \"$decode_ok\" -ne 0 ] || [ ! -s \"$tmp\" ]; then rm -f \"$tmp\"; exit 1; fi; \
         got=''; \
         if command -v sha256sum >/dev/null 2>&1; then set -- $(sha256sum < \"$tmp\"); got=\"${{1:-}}\"; \
         elif command -v shasum >/dev/null 2>&1; then set -- $(shasum -a 256 < \"$tmp\"); got=\"${{1:-}}\"; \
         elif command -v openssl >/dev/null 2>&1; then for w in $(openssl dgst -sha256 < \"$tmp\"); do got=\"$w\"; done; fi; \
         if [ -n \"$got\" ]; then \
             if [ \"$got\" != \"$want\" ]; then rm -f \"$tmp\"; \
                 printf '{UPLOAD_CHECKSUM_MARKER} expected=%s actual=%s\\n' \"$want\" \"$got\" >&2; exit 1; fi; \
         else \
             set -- $(wc -c < \"$tmp\"); \
             if [ \"${{1:-}}\" != \"$want_len\" ]; then rm -f \"$tmp\"; \
                 printf '{UPLOAD_SIZE_MARKER} expected=%s actual=%s\\n' \"$want_len\" \"${{1:-}}\" >&2; exit 1; fi; \
         fi; \
         chmod 700 \"$tmp\" && \
         daemon_pid=\"\" && \
         if [ -f \"$root/endpoint.json\" ]; then \
             pid=$(tr -d ' \\t\\r\\n' < \"$root/endpoint.json\" 2>/dev/null | sed -n 's/.*\"pid\":\\([0-9]\\{{1,\\}}\\).*/\\1/p'); \
             if [ -n \"$pid\" ] && [ \"$pid\" -gt 0 ] 2>/dev/null; then \
                 if kill -0 \"$pid\" 2>/dev/null && ps -p \"$pid\" -o comm= 2>/dev/null | grep -Eq '(^|/)ferryx-remote-h(elper)?$'; then \
                     daemon_pid=\"$pid\"; \
                 fi; \
             fi; \
         fi; \
         if [ -n \"$daemon_pid\" ]; then \
             rm -f \"$tmp\"; \
             printf 'FERRYX_INSTALL_DEFERRED:{{\"reason\":\"live_daemon\",\"pid\":%d}}\\n' \"$daemon_pid\"; \
             exit 0; \
         fi; \
         mv -f \"$tmp\" \"$dest\" && printf 'FERRYX_INSTALL_OK\\n'",
        direct::quote_posix(&location.executable),
        direct::quote_posix(&location.root),
        sha256_hex(binary_bytes),
        binary_bytes.len(),
    )
}

pub fn build_windows_upload_script(location: &HelperLocation, binary_bytes: &[u8]) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    // Delivered on stdin to `<shell> -Command -`: that mode silently discards
    // multi-line here-strings on live Windows hosts (pwsh 7.6.6 and
    // powershell.exe both exit 0 with no output), so the payload is assigned as
    // one newline-free single-quoted literal instead.
    let encoded = STANDARD.encode(binary_bytes);
    format!(
        "$ProgressPreference='SilentlyContinue'; $ErrorActionPreference='Stop'; \
         [Console]::OutputEncoding = New-Object Text.UTF8Encoding($false); \
         try {{ \
             $dest = {}; \
             $root = {}; \
             $dir = [System.IO.Path]::GetDirectoryName($dest); \
             if (-not [System.IO.Directory]::Exists($dir)) {{ [System.IO.Directory]::CreateDirectory($dir) | Out-Null }}; \
             $tmp = \"$dest.tmp.\" + [System.Guid]::NewGuid().ToString('N'); \
             $b64 = '{}'; \
             [System.IO.File]::WriteAllBytes($tmp, [System.Convert]::FromBase64String($b64.Trim())); \
             $sha = [System.Security.Cryptography.SHA256]::Create(); \
             try {{ $got = -join ($sha.ComputeHash([System.IO.File]::ReadAllBytes($tmp)) | ForEach-Object {{ $_.ToString('x2') }}) }} finally {{ $sha.Dispose() }}; \
             if ($got -ne '{want}') {{ [System.IO.File]::Delete($tmp); throw \"{UPLOAD_CHECKSUM_MARKER} expected={want} actual=$got\" }}; \
             $identity = [System.Security.Principal.WindowsIdentity]::GetCurrent(); \
             $userAcc = if ($identity.User) {{ '*' + $identity.User.Value }} else {{ \"$($env:USERDOMAIN)\\$($env:USERNAME)\" }}; \
             $aclRes = & icacls $tmp /inheritance:r /grant:r \"$($userAcc):(F)\" 2>&1; \
             if ($LASTEXITCODE -ne 0) {{ $aclRes = & icacls $tmp /inheritance:r /grant:r \"$($env:USERNAME):(F)\" 2>&1 }}; \
             if ($LASTEXITCODE -ne 0) {{ throw \"Failed to set private ACL on helper binary: $aclRes\" }}; \
             $epPath = Join-Path $root 'endpoint.json'; \
             $daemonProc = $null; \
             if ([System.IO.File]::Exists($epPath)) {{ \
                 try {{ \
                     $ep = Get-Content -Raw $epPath | ConvertFrom-Json; \
                     if ($ep.pid) {{ \
                         $proc = Get-Process -Id $ep.pid -ErrorAction SilentlyContinue; \
                         if ($proc -and $proc.ProcessName -eq 'ferryx-remote-helper') {{ \
                             $daemonProc = $proc; \
                         }} \
                     }} \
                 }} catch {{}} \
             }}; \
             if ($daemonProc) {{ \
                 if ([System.IO.File]::Exists($tmp)) {{ try {{ [System.IO.File]::Delete($tmp); }} catch {{}} }}; \
                 $json = @{{ reason = 'live_daemon'; pid = [int]$daemonProc.Id }} | ConvertTo-Json -Compress; \
                 [Console]::WriteLine(\"FERRYX_INSTALL_DEFERRED:$json\"); \
                 exit 0; \
             }}; \
             $old = \"$dest.old\"; \
             if ([System.IO.File]::Exists($old)) {{ [System.IO.File]::Delete($old); }}; \
             if ([System.IO.File]::Exists($dest)) {{ [System.IO.File]::Move($dest, $old); }}; \
             [System.IO.File]::Move($tmp, $dest); \
             if ([System.IO.File]::Exists($old)) {{ try {{ [System.IO.File]::Delete($old); }} catch {{}} }}; \
             [Console]::WriteLine('FERRYX_INSTALL_OK'); \
         }} catch {{ \
             if ($old -and -not [System.IO.File]::Exists($dest) -and [System.IO.File]::Exists($old)) {{ \
                 try {{ [System.IO.File]::Move($old, $dest) | Out-Null; }} catch {{}} \
             }}; \
             [Console]::Error.Write($_.Exception.Message); \
             exit 1; \
         }}",
        runtime::powershell_data(&location.executable),
        runtime::powershell_data(&location.root),
        encoded,
        want = sha256_hex(binary_bytes),
    )
}

pub fn qualified_location(
    host: &SshHost,
    env: &RemoteEnvironment,
    version_tag: &str,
) -> Result<HelperLocation, IpcError> {
    env.platform.validate_path(&env.home)?;
    // The executable stays under the version-qualified `versions/<tag>` tree; the
    // runtime root is version-qualified and host-digest scoped but deliberately
    // compact, because the helper binds `<root>/helper.sock` and that socket path
    // must stay inside the Unix socket limit (103 bytes on macOS) for typical
    // remote homes - up to roughly 37 UTF-8 bytes of home path. No length
    // preflight is enforced here; existing roots are never migrated, because
    // sessions keep the location persisted in their descriptor.
    let digest = host_digest(&host.id);

    let (executable, root) = match env.platform {
        RemotePlatform::Posix => {
            let home = env.home.trim_end_matches('/');
            (
                format!("{home}/.ferryx/versions/{version_tag}/bin/ferryx-remote-helper"),
                format!("{home}/.ferryx/r/{version_tag}/{digest}"),
            )
        }
        RemotePlatform::Windows => {
            let home = env.home.trim_end_matches(&['\\', '/'][..]);
            (
                format!("{home}\\.ferryx\\versions\\{version_tag}\\bin\\ferryx-remote-helper.exe"),
                format!("{home}\\.ferryx\\r\\{version_tag}\\{digest}"),
            )
        }
    };
    env.platform.validate_path(&executable)?;
    env.platform.validate_path(&root)?;
    Ok(HelperLocation { executable, root })
}

pub async fn provision_qualified(
    host: &SshHost,
    env: &RemoteEnvironment,
    local_binary: &Path,
    version_or_tag: &str,
) -> Result<HelperLocation, IpcError> {
    let location = qualified_location(host, env, version_or_tag)?;
    install(host, env, &location, local_binary).await?;
    ensure_started(host, env, &location).await?;
    Ok(location)
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
