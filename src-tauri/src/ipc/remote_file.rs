use crate::ipc::{run_blocking, IpcError, IpcErrorCode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;
#[cfg(feature = "desktop")]
use tauri::{AppHandle, Manager, Runtime};

pub const REMOTE_FILE_MAX_BYTES: usize = 8 * 1024 * 1024;

/// Printed by [`remote_read_command`] right before the file bytes, so output a
/// login shell's startup files print first never becomes part of the file.
const REMOTE_READ_MARKER: &[u8] = b"FERRYX-REMOTE-FILE:";
/// Startup-file output tolerated in front of [`REMOTE_READ_MARKER`].
const REMOTE_READ_NOISE_ALLOWANCE: usize = 64 * 1024;
/// One fetch may need a password prompt plus 8 MiB (11 MiB base64 on Windows)
/// over a slow link.
const REMOTE_FETCH_DEADLINE: Duration = Duration::from_secs(120);
/// Fetched copies older than this are pruned from the cache.
const REMOTE_CACHE_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);
/// Longest sanitized file stem kept in a cache file name.
const CACHE_STEM_MAX_CHARS: usize = 100;
/// Longest extension (dot excluded) kept in a cache file name.
const CACHE_EXTENSION_MAX_CHARS: usize = 16;

fn invalid_remote_path(message: impl Into<String>) -> IpcError {
    IpcError::new(IpcErrorCode::InvalidRequest, message.into())
        .with_details(serde_json::json!({ "reason": "invalidRemotePath" }))
}

/// True when `input` names its remote file without a base directory: home-relative
/// (`~`, `~/...`, and on Windows `~\...`) or absolute. On Windows the rooted-but-not-
/// absolute forms (`\foo`, `C:foo`) also count, so [`windows_remote_target_path`]
/// refuses them as invalid paths rather than as a missing cwd.
fn remote_input_needs_no_base(input: &str, windows: bool) -> bool {
    if input == "~" || input.starts_with("~/") {
        return true;
    }
    if !windows {
        return input.starts_with('/');
    }
    input.starts_with("~\\")
        || input.starts_with(['\\', '/'])
        || input.as_bytes().get(1) == Some(&b':')
}

/// Picks the directory a relative remote token resolves against: the terminal's live
/// cwd, never a guess. Absolute and home-relative inputs need no base (`""`). A relative
/// input without a valid cwd (POSIX: absolute; Windows: [`RemotePlatform::validate_path`])
/// is refused with `remoteCwdUnknown`, because falling back to the project root could
/// fetch a different file that happens to share the name.
///
/// [`RemotePlatform::validate_path`]: crate::ssh::runtime::RemotePlatform::validate_path
pub(crate) fn remote_fetch_base<'a>(
    input: &str,
    cwd: Option<&'a str>,
    windows: bool,
) -> Result<&'a str, IpcError> {
    if remote_input_needs_no_base(input, windows) {
        return Ok("");
    }
    let valid = |c: &str| {
        if windows {
            crate::ssh::runtime::RemotePlatform::Windows
                .validate_path(c)
                .is_ok()
        } else {
            c.starts_with('/')
        }
    };
    match cwd {
        Some(c) if valid(c) => Ok(c),
        _ => Err(IpcError::new(
            IpcErrorCode::InvalidRequest,
            "The remote terminal's current directory is unknown, so a relative path cannot be resolved",
        )
        .with_details(serde_json::json!({ "reason": "remoteCwdUnknown" }))),
    }
}

/// The read command reaches the remote LOGIN shell, which may be fish: there a
/// backslash escapes even inside single quotes, so a path holding one could end
/// the quoting [`crate::ssh::direct::quote_posix`] applies and run as code.
fn reject_backslash(path: &str) -> Result<(), IpcError> {
    if path.contains('\\') {
        return Err(invalid_remote_path(
            "Remote POSIX paths containing a backslash are not supported",
        ));
    }
    Ok(())
}

/// Normalizes a terminal path token into the remote file it names.
///
/// Absolute and cwd-relative inputs yield a normalized absolute POSIX path.
/// Home-relative inputs (`~` or `~/...`) yield `~` or `~/<normalized suffix>`,
/// which only [`remote_read_command`] expands, as `"$HOME"`, on the remote host;
/// the suffix may not climb above the home directory.
pub fn remote_target_path(input: &str, base: &str) -> Result<String, IpcError> {
    if input.is_empty() || input.chars().any(char::is_control) {
        return Err(invalid_remote_path(
            "Remote path cannot be empty or contain control characters",
        ));
    }

    if input == "~" || input.starts_with("~/") {
        let mut segments: Vec<&str> = Vec::new();
        for segment in input[1..].split('/') {
            match segment {
                "" | "." => continue,
                ".." => {
                    if segments.pop().is_none() {
                        return Err(invalid_remote_path(
                            "Remote path climbs above the home directory",
                        ));
                    }
                }
                s => segments.push(s),
            }
        }
        let suffix = segments.join("/");
        crate::ssh::direct::validate_remote_path(&format!("/{suffix}"))
            .map_err(|e| invalid_remote_path(e.message))?;
        reject_backslash(&suffix)?;
        return Ok(if suffix.is_empty() {
            "~".to_string()
        } else {
            format!("~/{suffix}")
        });
    }

    let raw = if input.starts_with('/') {
        input.to_string()
    } else {
        if base.is_empty() || !base.starts_with('/') || base.chars().any(char::is_control) {
            return Err(invalid_remote_path(
                "Base path must be an absolute POSIX path without control characters",
            ));
        }
        if base.ends_with('/') {
            format!("{base}{input}")
        } else {
            format!("{base}/{input}")
        }
    };

    let mut segments: Vec<&str> = Vec::new();
    for segment in raw.split('/') {
        match segment {
            "" | "." => continue,
            ".." => {
                if segments.pop().is_none() {
                    return Err(invalid_remote_path(
                        "Remote path climbs above root directory",
                    ));
                }
            }
            s => segments.push(s),
        }
    }

    let normalized = if segments.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", segments.join("/"))
    };

    crate::ssh::direct::validate_remote_path(&normalized)
        .map_err(|e| invalid_remote_path(e.message))?;
    reject_backslash(&normalized)?;

    Ok(normalized)
}

/// Directory under the app cache that holds every fetched remote file.
pub fn remote_files_root(cache_root: &Path) -> PathBuf {
    cache_root.join("remote-files")
}

/// Windows counterpart of [`remote_target_path`].
///
/// Absolute inputs (drive `C:\...` or UNC `\\server\share\...`, either separator)
/// and base-relative inputs yield a normalized absolute path joined with `\`; `..`
/// may not climb above the drive or UNC share root. Home-relative inputs (`~`,
/// `~\...`, `~/...`) yield `~` or `~\<normalized suffix>`, which only
/// [`windows_remote_read_script`] resolves, against `$HOME`, on the remote host.
/// Drive-relative (`C:foo`) and root-relative (`\foo`) forms are rejected: their
/// meaning depends on per-drive state the remote shell does not share with us.
pub fn windows_remote_target_path(input: &str, base: &str) -> Result<String, IpcError> {
    use crate::ssh::runtime::RemotePlatform;

    if input.is_empty() || input.chars().any(char::is_control) {
        return Err(invalid_remote_path(
            "Remote path cannot be empty or contain control characters",
        ));
    }

    let normalize = |rest: &str, floor: &str| -> Result<Vec<String>, IpcError> {
        let mut segments: Vec<String> = Vec::new();
        for segment in rest.split(['\\', '/']) {
            match segment {
                "" | "." => continue,
                ".." => {
                    if segments.pop().is_none() {
                        return Err(invalid_remote_path(format!(
                            "Remote path climbs above the {floor}"
                        )));
                    }
                }
                s => segments.push(s.to_string()),
            }
        }
        Ok(segments)
    };

    if input == "~" || input.starts_with("~\\") || input.starts_with("~/") {
        let segments = normalize(&input[1..], "home directory")?;
        return Ok(if segments.is_empty() {
            "~".to_string()
        } else {
            format!("~\\{}", segments.join("\\"))
        });
    }

    let is_drive_absolute = |p: &str| {
        let b = p.as_bytes();
        b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && matches!(b[2], b'\\' | b'/')
    };
    let is_unc = |p: &str| p.starts_with("\\\\") || p.starts_with("//");

    let raw = if is_drive_absolute(input) || is_unc(input) {
        input.to_string()
    } else if input.starts_with(['\\', '/']) || input.as_bytes().get(1) == Some(&b':') {
        return Err(invalid_remote_path(
            "Remote path must be absolute (drive or UNC) or relative to the working directory",
        ));
    } else {
        if RemotePlatform::Windows.validate_path(base).is_err() {
            return Err(invalid_remote_path(
                "Base path must be an absolute Windows path without control characters",
            ));
        }
        format!("{}\\{input}", base.trim_end_matches(['\\', '/']))
    };

    let normalized = if is_unc(&raw) {
        let mut parts = raw[2..].split(['\\', '/']).filter(|s| !s.is_empty());
        let (server, share) = match (parts.next(), parts.next()) {
            (Some(server), Some(share)) if server != "?" && server != "." => (server, share),
            _ => {
                return Err(invalid_remote_path(
                    "UNC path must name a server and a share",
                ))
            }
        };
        let rest: Vec<&str> = parts.collect();
        let segments = normalize(&rest.join("\\"), "UNC share root")?;
        let root = format!("\\\\{server}\\{share}");
        if segments.is_empty() {
            root
        } else {
            format!("{root}\\{}", segments.join("\\"))
        }
    } else {
        let drive = raw[..1].to_ascii_uppercase();
        let segments = normalize(&raw[3..], "drive root")?;
        format!("{drive}:\\{}", segments.join("\\"))
    };

    RemotePlatform::Windows
        .validate_path(&normalized)
        .map_err(|e| invalid_remote_path(e.message))?;
    Ok(normalized)
}

/// Byte length of the base64 text [`windows_remote_read_script`] prints for a
/// file read up to its `REMOTE_FILE_MAX_BYTES + 1` byte cap.
pub fn windows_remote_wire_limit() -> usize {
    (REMOTE_FILE_MAX_BYTES + 1).div_ceil(3) * 4
}

/// PowerShell script reading one remote file on a Windows host. Exits 3 when the
/// target is missing or a directory (matching [`remote_read_command`]); any other
/// open failure, such as access denied, surfaces as an error. Reads at most
/// `REMOTE_FILE_MAX_BYTES + 1` bytes, and prints them base64-encoded so the
/// PowerShell text pipeline cannot transcode binary content. The path is passed as
/// base64 data, never interpolated as PowerShell source.
pub fn windows_remote_read_script(remote_path: &str) -> String {
    use crate::ssh::runtime::powershell_data;

    let resolve = if remote_path == "~" {
        "$p=$HOME".to_string()
    } else if let Some(suffix) = remote_path.strip_prefix("~\\") {
        format!("$p=Join-Path $HOME {}", powershell_data(suffix))
    } else {
        format!("$p={}", powershell_data(remote_path))
    };
    format!(
        "{resolve}; \
         if ([IO.Directory]::Exists($p)) {{ exit 3 }}; \
         $n={}; $buf=New-Object byte[] $n; $t=0; \
         try {{ $f=[IO.File]::Open($p,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::ReadWrite) }} \
         catch [IO.FileNotFoundException],[IO.DirectoryNotFoundException] {{ exit 3 }}; \
         try {{ while ($t -lt $n) {{ $r=$f.Read($buf,$t,$n-$t); if ($r -le 0) {{ break }}; $t+=$r }} }} \
         finally {{ $f.Dispose() }}; \
         [Console]::Out.Write([Convert]::ToBase64String($buf,0,$t))",
        REMOTE_FILE_MAX_BYTES + 1
    )
}

fn decode_windows_payload(bytes: &[u8]) -> Result<Vec<u8>, IpcError> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};

    let text: Vec<u8> = bytes
        .iter()
        .copied()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    STANDARD.decode(text).map_err(|e| {
        IpcError::new(
            IpcErrorCode::ParseError,
            format!("Remote file response is not valid base64: {e}"),
        )
    })
}

/// Marks a fetched cache file as downloaded from another machine so the OS
/// treats it like any download: `com.apple.quarantine` on macOS, a
/// `Zone.Identifier` (Mark of the Web) stream on Windows, nothing elsewhere.
/// Failures are logged, never fatal: the fetch itself already succeeded.
pub fn mark_downloaded(path: &Path) {
    if let Err(error) = quarantine::mark(path) {
        tracing::warn!(%error, path = %path.display(), "Failed to mark fetched remote file as downloaded");
    }
}

#[cfg(target_os = "macos")]
mod quarantine {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    pub fn mark(path: &Path) -> std::io::Result<()> {
        let seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        // 0081: downloaded, user has not yet approved it.
        let value = format!("0081;{seconds:x};Ferryx;");
        let c_path = CString::new(path.as_os_str().as_bytes())?;
        let name = CString::new("com.apple.quarantine")?;
        // SAFETY: both strings are NUL-terminated and outlive the call; the value
        // pointer/length describe an initialized buffer.
        let result = unsafe {
            libc::setxattr(
                c_path.as_ptr(),
                name.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
                0,
            )
        };
        if result == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
}

#[cfg(target_os = "windows")]
mod quarantine {
    use std::path::Path;

    pub fn mark(path: &Path) -> std::io::Result<()> {
        let mut stream = path.as_os_str().to_os_string();
        stream.push(":Zone.Identifier");
        std::fs::write(stream, b"[ZoneTransfer]\r\nZoneId=3\r\n")
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod quarantine {
    pub fn mark(_path: &std::path::Path) -> std::io::Result<()> {
        Ok(())
    }
}

pub fn remote_cache_path(cache_root: &Path, workspace_id: &str, remote_path: &str) -> PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(workspace_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(remote_path.as_bytes());
    let hex_digest = format!("{:x}", hasher.finalize());
    let dir_name = &hex_digest[..32];

    let raw_basename = remote_path.rsplit(['/', '\\']).next().unwrap_or("");
    let sanitized: String = raw_basename
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();

    let file_name = if sanitized.is_empty() || sanitized == "." || sanitized == ".." {
        "file".to_string()
    } else {
        portable_cache_name(&sanitized)
    };

    remote_files_root(cache_root).join(dir_name).join(file_name)
}

/// Fits an ASCII-sanitized basename to every local filesystem: the stem is
/// capped (a remote name can exceed the local 255-byte limit) and a Windows
/// device name (`CON`, `nul.txt`, `COM1.log`) gains a `_` prefix.
fn portable_cache_name(sanitized: &str) -> String {
    let (stem, extension) = match sanitized.rsplit_once('.') {
        Some((stem, ext))
            if !stem.is_empty() && !ext.is_empty() && ext.len() <= CACHE_EXTENSION_MAX_CHARS =>
        {
            (stem, Some(ext))
        }
        _ => (sanitized, None),
    };
    let mut stem: String = stem.chars().take(CACHE_STEM_MAX_CHARS).collect();
    let device = stem.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(device.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (device.len() == 4
            && (device.starts_with("COM") || device.starts_with("LPT"))
            && device.as_bytes()[3].is_ascii_digit());
    if reserved {
        stem.insert(0, '_');
    }
    match extension {
        Some(ext) => format!("{stem}.{ext}"),
        None => stem,
    }
}

/// Publishes an immutable quarantined copy so concurrent fetches cannot overwrite
/// bytes owned by another preview. Old copies are pruned by the cache age policy.
pub(crate) fn write_cache_file(target: &Path, bytes: &[u8]) -> Result<PathBuf, IpcError> {
    let io = |message: String| IpcError::new(IpcErrorCode::IoError, message);
    let parent = target
        .parent()
        .ok_or_else(|| io("Missing parent directory for cache file".into()))?;
    let name = target
        .file_name()
        .ok_or_else(|| io("Missing cache file name".into()))?;
    if let Some(root) = parent.parent() {
        prune_remote_cache(root, parent, REMOTE_CACHE_MAX_AGE, std::time::SystemTime::now());
    }
    std::fs::create_dir_all(parent)
        .map_err(|e| io(format!("Failed to create cache directory: {e}")))?;

    let temp = parent.join(format!(".{}.tmp", uuid::Uuid::new_v4().simple()));
    let written = (|| {
        use std::io::Write;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()
    })();
    if let Err(error) = written {
        let _ = std::fs::remove_file(&temp);
        return Err(io(format!("Failed to write cached remote file: {error}")));
    }
    mark_downloaded(&temp);

    let fresh_dir = parent.join(uuid::Uuid::new_v4().simple().to_string());
    let fresh = fresh_dir.join(name);
    match std::fs::create_dir_all(&fresh_dir).and_then(|()| std::fs::rename(&temp, &fresh)) {
        Ok(()) => Ok(fresh),
        Err(error) => {
            let _ = std::fs::remove_file(&temp);
            Err(io(format!(
                "Failed to write cached remote file: {error}"
            )))
        }
    }
}

/// Removes cache files older than `max_age` (fetched secrets must not linger
/// on local disk) and the directories they leave empty, except `keep`, the
/// directory the caller is about to write into. Best effort: a file a viewer
/// still holds open on Windows simply stays until the next prune.
pub(crate) fn prune_remote_cache(
    root: &Path,
    keep: &Path,
    max_age: Duration,
    now: std::time::SystemTime,
) {
    fn walk(dir: &Path, depth: usize, keep: &Path, max_age: Duration, now: std::time::SystemTime) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.is_dir() {
                if depth < 2 {
                    walk(&path, depth + 1, keep, max_age, now);
                    if path != keep {
                        let _ = std::fs::remove_dir(&path);
                    }
                }
                continue;
            }
            let stale = meta
                .modified()
                .ok()
                .and_then(|modified| now.duration_since(modified).ok())
                .is_some_and(|age| age > max_age);
            if stale {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    walk(root, 0, keep, max_age, now);
}

/// Drops whatever the remote login shell printed before [`REMOTE_READ_MARKER`].
fn strip_read_marker(bytes: &[u8]) -> Result<Vec<u8>, IpcError> {
    bytes
        .windows(REMOTE_READ_MARKER.len())
        .position(|window| window == REMOTE_READ_MARKER)
        .map(|at| bytes[at + REMOTE_READ_MARKER.len()..].to_vec())
        .ok_or_else(|| {
            IpcError::new(
                IpcErrorCode::ParseError,
                "Remote file response is missing its start marker",
            )
        })
}

/// Shell command reading one remote file. Only the home-relative form produced
/// by [`remote_target_path`] expands anything: `"$HOME"`, never a quoted suffix.
pub fn remote_read_command(remote_path: &str) -> String {
    let q = if remote_path == "~" {
        "\"$HOME\"".to_string()
    } else if let Some(suffix) = remote_path.strip_prefix("~/") {
        format!("\"$HOME\"/{}", crate::ssh::direct::quote_posix(suffix))
    } else {
        crate::ssh::direct::quote_posix(remote_path)
    };
    format!(
        "test -f {q} || exit 3; printf {}; head -c {} -- {q}",
        String::from_utf8_lossy(REMOTE_READ_MARKER),
        REMOTE_FILE_MAX_BYTES + 1
    )
}

pub(crate) fn ensure_within_limit(len: usize, max: usize) -> Result<(), IpcError> {
    if len > max {
        return Err(IpcError::new(
            IpcErrorCode::PayloadTooLarge,
            format!("Remote file exceeds maximum preview size of {} bytes", max),
        )
        .with_details(serde_json::json!({
            "reason": "remoteFileTooLarge",
            "maxBytes": max,
        })));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteFileFetched {
    pub local_path: String,
    pub remote_path: String,
    pub byte_length: u64,
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_remote_file_fetch<R: Runtime>(
    app: AppHandle<R>,
    workspace_id: String,
    path: String,
    cwd: Option<String>,
) -> Result<RemoteFileFetched, IpcError> {
    let host_store = crate::ipc::ssh::get_ssh_store_path(&app)?;
    let ws_lookup = workspace_id.clone();
    let (project, host) = run_blocking(move || {
        crate::ssh::projects::resolve(&host_store, &ws_lookup)
    })
    .await?;

    let windows = project.platform == Some(crate::ssh::runtime::RemotePlatform::Windows);
    let base = remote_fetch_base(&path, cwd.as_deref(), windows)?;

    let (remote_path, command, max_wire_bytes) = if windows {
        let remote_path = windows_remote_target_path(&path, base)?;
        // powershell.exe ships with every supported Windows release and runs
        // regardless of the host's OpenSSH DefaultShell (cmd, powershell or pwsh).
        let command = crate::ssh::runtime::RemoteExecutor::Powershell
            .command(&windows_remote_read_script(&remote_path));
        (remote_path, command, windows_remote_wire_limit())
    } else {
        let remote_path = remote_target_path(&path, base)?;
        let command = remote_read_command(&remote_path);
        (
            remote_path,
            command,
            REMOTE_FILE_MAX_BYTES + 1 + REMOTE_READ_MARKER.len() + REMOTE_READ_NOISE_ALLOWANCE,
        )
    };
    let plan = crate::ssh::direct::ssh_plan(&host, command, false)?;

    let bytes_res =
        crate::ssh::direct::bounded_output_with_limit(&plan, REMOTE_FETCH_DEADLINE, max_wire_bytes)
            .await;

    let bytes = match bytes_res {
        Ok(b) => b,
        Err(err) => {
            let is_missing = err
                .details
                .as_ref()
                .and_then(|d| d.get("exitCode"))
                .and_then(|c| c.as_i64())
                == Some(3);
            if is_missing {
                return Err(IpcError::new(
                    IpcErrorCode::NotFound,
                    format!("Remote file not found or not a regular file: {remote_path}"),
                )
                .with_details(serde_json::json!({ "reason": "remoteFileMissing" })));
            }
            return Err(err);
        }
    };
    let bytes = if windows {
        decode_windows_payload(&bytes)?
    } else {
        strip_read_marker(&bytes)?
    };

    ensure_within_limit(bytes.len(), REMOTE_FILE_MAX_BYTES)?;

    let cache_root = app.path().app_cache_dir().map_err(|e| {
        IpcError::new(
            IpcErrorCode::IoError,
            format!("Failed to resolve app cache directory: {e}"),
        )
    })?;

    let target = remote_cache_path(&cache_root, &workspace_id, &remote_path);
    let byte_length = bytes.len() as u64;
    let local_target = run_blocking(move || write_cache_file(&target, &bytes)).await?;

    Ok(RemoteFileFetched {
        local_path: local_target.to_string_lossy().to_string(),
        remote_path,
        byte_length,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_remote_tokens_without_a_live_cwd_are_refused_not_anchored_on_the_repo() {
        let reason = |e: IpcError| e.details.unwrap()["reason"].as_str().unwrap().to_string();
        // POSIX: a relative token needs an absolute cwd.
        for cwd in [None, Some(""), Some("relative/dir")] {
            let err = remote_fetch_base("src/lib.rs", cwd, false).unwrap_err();
            assert_eq!(err.code, IpcErrorCode::InvalidRequest, "{cwd:?}");
            assert_eq!(reason(err), "remoteCwdUnknown", "{cwd:?}");
        }
        assert_eq!(remote_fetch_base("src/lib.rs", Some("/srv/app"), false).unwrap(), "/srv/app");
        // Absolute and home-relative inputs never need a cwd.
        for input in ["/etc/hosts", "~", "~/notes.md"] {
            assert_eq!(remote_fetch_base(input, None, false).unwrap(), "", "{input}");
        }

        // Windows: the cwd must be a valid absolute Windows path.
        for cwd in [None, Some("/home/u"), Some("repo")] {
            let err = remote_fetch_base(r"src\lib.rs", cwd, true).unwrap_err();
            assert_eq!(reason(err), "remoteCwdUnknown", "{cwd:?}");
        }
        assert_eq!(
            remote_fetch_base(r"src\lib.rs", Some(r"C:\repo"), true).unwrap(),
            r"C:\repo"
        );
        for input in [r"C:\x.txt", r"\\srv\share\x", "~", r"~\x", "~/x"] {
            assert_eq!(remote_fetch_base(input, None, true).unwrap(), "", "{input}");
        }
        // Rooted-but-relative Windows forms reach the target parser, which refuses them
        // as invalid paths, not as a missing cwd.
        for input in [r"\rooted", "C:drive-relative"] {
            let base = remote_fetch_base(input, None, true).unwrap();
            let err = windows_remote_target_path(input, base).unwrap_err();
            assert_eq!(reason(err), "invalidRemotePath", "{input}");
        }
    }

    #[test]
    fn test_remote_target_path_absolute_passthrough() {
        assert_eq!(
            remote_target_path("/var/log/syslog", "/home/user").unwrap(),
            "/var/log/syslog"
        );
        assert_eq!(
            remote_target_path("/etc/nginx/nginx.conf", "/home/user").unwrap(),
            "/etc/nginx/nginx.conf"
        );
    }

    #[test]
    fn test_remote_target_path_relative_join() {
        assert_eq!(
            remote_target_path("src/lib.rs", "/home/user/project").unwrap(),
            "/home/user/project/src/lib.rs"
        );
        assert_eq!(
            remote_target_path("config.json", "/home/user/project/").unwrap(),
            "/home/user/project/config.json"
        );
        assert_eq!(
            remote_target_path("./file.txt", "/home/user/project").unwrap(),
            "/home/user/project/file.txt"
        );
    }

    #[test]
    fn test_remote_target_path_dot_dot_normalization_inside_root() {
        assert_eq!(
            remote_target_path("src/../docs/readme.md", "/home/user/project").unwrap(),
            "/home/user/project/docs/readme.md"
        );
        assert_eq!(
            remote_target_path("/a/b/../c/./d", "/base").unwrap(),
            "/a/c/d"
        );
        assert_eq!(remote_target_path("..", "/a/b").unwrap(), "/a");
        assert_eq!(remote_target_path("../c", "/a/b").unwrap(), "/a/c");
        assert_eq!(
            remote_target_path("sub/../../a/b", "/home/user").unwrap(),
            "/home/a/b"
        );
    }

    #[test]
    fn test_remote_target_path_climbing_above_root_rejected() {
        let err1 = remote_target_path("/..", "/base").unwrap_err();
        assert_eq!(err1.code, IpcErrorCode::InvalidRequest);
        assert_eq!(
            err1.details.as_ref().unwrap().get("reason").unwrap(),
            "invalidRemotePath"
        );

        let err2 = remote_target_path("/a/../..", "/base").unwrap_err();
        assert_eq!(err2.code, IpcErrorCode::InvalidRequest);
        assert_eq!(
            err2.details.as_ref().unwrap().get("reason").unwrap(),
            "invalidRemotePath"
        );

        let err3 = remote_target_path("../../etc/passwd", "/home").unwrap_err();
        assert_eq!(err3.code, IpcErrorCode::InvalidRequest);
        assert_eq!(
            err3.details.as_ref().unwrap().get("reason").unwrap(),
            "invalidRemotePath"
        );

        let err4 = remote_target_path("..", "/").unwrap_err();
        assert_eq!(err4.code, IpcErrorCode::InvalidRequest);
        assert_eq!(
            err4.details.as_ref().unwrap().get("reason").unwrap(),
            "invalidRemotePath"
        );
    }

    #[test]
    fn test_remote_target_path_control_char_rejected() {
        let err_nul = remote_target_path("/path/with\0null", "/base").unwrap_err();
        assert_eq!(err_nul.code, IpcErrorCode::InvalidRequest);
        assert_eq!(
            err_nul.details.as_ref().unwrap().get("reason").unwrap(),
            "invalidRemotePath"
        );

        let err_nl = remote_target_path("/path/with\nnewline", "/base").unwrap_err();
        assert_eq!(err_nl.code, IpcErrorCode::InvalidRequest);
        assert_eq!(
            err_nl.details.as_ref().unwrap().get("reason").unwrap(),
            "invalidRemotePath"
        );

        let err_cr = remote_target_path("/path/with\rreturn", "/base").unwrap_err();
        assert_eq!(err_cr.code, IpcErrorCode::InvalidRequest);
        assert_eq!(
            err_cr.details.as_ref().unwrap().get("reason").unwrap(),
            "invalidRemotePath"
        );

        let err_tab = remote_target_path("some\tfile.txt", "/base").unwrap_err();
        assert_eq!(err_tab.code, IpcErrorCode::InvalidRequest);
        assert_eq!(
            err_tab.details.as_ref().unwrap().get("reason").unwrap(),
            "invalidRemotePath"
        );
    }

    #[test]
    fn test_remote_target_path_empty_rejected() {
        let err_empty = remote_target_path("", "/base").unwrap_err();
        assert_eq!(err_empty.code, IpcErrorCode::InvalidRequest);
        assert_eq!(
            err_empty.details.as_ref().unwrap().get("reason").unwrap(),
            "invalidRemotePath"
        );
    }

    #[test]
    fn test_remote_target_path_home_relative_stays_home_relative() {
        assert_eq!(
            remote_target_path("~/logs/run.png", "/home/u/proj").unwrap(),
            "~/logs/run.png"
        );
        assert_eq!(remote_target_path("~", "/home/u/proj").unwrap(), "~");
        assert_eq!(remote_target_path("~/", "/home/u/proj").unwrap(), "~");
        assert_eq!(
            remote_target_path("~/a/./b/../c.txt", "/home/u/proj").unwrap(),
            "~/a/c.txt"
        );
    }

    #[test]
    fn test_remote_target_path_home_relative_cannot_climb_above_home() {
        for input in ["~/..", "~/../etc/passwd", "~/a/../../b"] {
            let err = remote_target_path(input, "/home/u/proj").unwrap_err();
            assert_eq!(err.code, IpcErrorCode::InvalidRequest, "{input}");
            assert_eq!(
                err.details.as_ref().unwrap().get("reason").unwrap(),
                "invalidRemotePath"
            );
        }
    }

    #[test]
    fn test_remote_read_command_expands_only_home() {
        let cmd = remote_read_command("~/logs/run $(id)'x.png");
        let target = "\"$HOME\"/'logs/run $(id)'\\''x.png'";
        assert_eq!(
            cmd,
            format!(
                "test -f {target} || exit 3; printf FERRYX-REMOTE-FILE:; head -c {} -- {target}",
                REMOTE_FILE_MAX_BYTES + 1
            )
        );
        assert!(remote_read_command("~").starts_with("test -f \"$HOME\" || exit 3;"));
        // A "~" that is not the leading home marker stays inside single quotes.
        assert!(remote_read_command("/srv/~/x").starts_with("test -f '/srv/~/x' "));
    }

    #[test]
    fn posix_paths_with_backslash_are_rejected_before_reaching_the_login_shell() {
        // fish treats \' inside single quotes as an escaped quote, so this would
        // close the quoting and run "touch /tmp/pwn" on the remote host.
        let hostile = r"/x\'; touch /tmp/pwn; echo \'";
        for (input, base) in [
            (hostile, "/home/u"),
            (r"a\b.txt", "/home/u"),
            (r"~/x\y", "/home/u"),
            ("rel.txt", r"/home/u\'"),
        ] {
            let err = remote_target_path(input, base).unwrap_err();
            assert_eq!(err.code, IpcErrorCode::InvalidRequest, "{input:?}");
            assert_eq!(err.details.as_ref().unwrap()["reason"], "invalidRemotePath");
        }
    }

    #[test]
    fn read_marker_drops_login_shell_noise_and_keeps_binary_bytes() {
        let mut wire = b"Welcome to host\nmotd FERRYX\n".to_vec();
        wire.extend_from_slice(REMOTE_READ_MARKER);
        let payload: Vec<u8> = (0..=255u8).chain(REMOTE_READ_MARKER.iter().copied()).collect();
        wire.extend_from_slice(&payload);
        assert_eq!(strip_read_marker(&wire).unwrap(), payload);
        assert_eq!(
            strip_read_marker(b"no marker").unwrap_err().code,
            IpcErrorCode::ParseError
        );
    }

    #[test]
    fn cache_names_fit_local_filesystems() {
        let cache_root = Path::new("/var/cache/ferryx");
        let long = format!("/srv/{}.log", "a".repeat(300));
        let name = remote_cache_path(cache_root, "ssh:ws", &long)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert_eq!(name, format!("{}.log", "a".repeat(CACHE_STEM_MAX_CHARS)));
        for (remote, expected) in [
            ("/srv/CON", "_CON"),
            ("/srv/nul.txt", "_nul.txt"),
            ("/srv/com1.log", "_com1.log"),
            ("/srv/LPT9.tar.gz", "_LPT9.tar.gz"),
            ("/srv/console.txt", "console.txt"),
            ("/srv/com10.txt", "com10.txt"),
        ] {
            let path = remote_cache_path(cache_root, "ssh:ws", remote);
            assert_eq!(path.file_name().unwrap().to_str().unwrap(), expected, "{remote}");
        }
    }

    #[test]
    fn cache_write_preserves_each_published_copy() {
        let dir = tempfile::tempdir().unwrap();
        let target = remote_cache_path(dir.path(), "ssh:ws", "/srv/run.png");
        let first = write_cache_file(&target, b"one").unwrap();
        let second = write_cache_file(&target, b"two").unwrap();
        assert_ne!(first, second);
        assert_eq!(std::fs::read(&first).unwrap(), b"one");
        assert_eq!(std::fs::read(&second).unwrap(), b"two");

        // A rename onto a non-empty directory fails on every platform, standing
        // in for Windows refusing to replace a file a viewer holds open.
        let blocked = remote_cache_path(dir.path(), "ssh:ws", "/srv/held.pdf");
        std::fs::create_dir_all(blocked.join("x")).unwrap();
        let landed = write_cache_file(&blocked, b"three").unwrap();
        assert_ne!(landed, blocked);
        assert_eq!(landed.file_name(), blocked.file_name());
        assert!(landed.starts_with(blocked.parent().unwrap()));
        assert_eq!(std::fs::read(&landed).unwrap(), b"three");
        let leftovers: Vec<_> = std::fs::read_dir(blocked.parent().unwrap())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn cache_prune_removes_stale_copies_and_orphaned_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = remote_files_root(dir.path());
        let old = remote_cache_path(dir.path(), "ssh:ws", "/srv/.env");
        let old = write_cache_file(&old, b"SECRET=1").unwrap();
        let orphan = old.parent().unwrap().join(".abc.tmp");
        std::fs::write(&orphan, b"partial").unwrap();
        let keep = remote_cache_path(dir.path(), "ssh:ws", "/srv/new.txt");
        std::fs::create_dir_all(keep.parent().unwrap()).unwrap();

        let now = std::time::SystemTime::now();
        prune_remote_cache(&root, keep.parent().unwrap(), REMOTE_CACHE_MAX_AGE, now);
        assert!(old.exists(), "fresh copies stay");

        let later = now + REMOTE_CACHE_MAX_AGE + Duration::from_secs(60);
        prune_remote_cache(&root, keep.parent().unwrap(), REMOTE_CACHE_MAX_AGE, later);
        assert!(!old.exists());
        assert!(!orphan.exists());
        assert!(!old.parent().unwrap().exists(), "emptied directories are removed");
        assert!(keep.parent().unwrap().exists(), "the directory being written survives");
    }

    #[test]
    fn test_remote_cache_path_deterministic() {
        let cache_root = Path::new("/var/cache/ferryx");
        let p1 = remote_cache_path(cache_root, "ssh:ws-1", "/home/user/project/file.txt");
        let p2 = remote_cache_path(cache_root, "ssh:ws-1", "/home/user/project/file.txt");
        assert_eq!(p1, p2);
    }

    #[test]
    fn test_remote_cache_path_differs_per_workspace() {
        let cache_root = Path::new("/var/cache/ferryx");
        let p1 = remote_cache_path(cache_root, "ssh:ws-1", "/home/user/project/file.txt");
        let p2 = remote_cache_path(cache_root, "ssh:ws-2", "/home/user/project/file.txt");
        assert_ne!(p1, p2);
    }

    #[test]
    fn test_remote_cache_path_sanitizes_basename() {
        let cache_root = Path::new("/var/cache/ferryx");
        let path = remote_cache_path(cache_root, "ssh:ws-1", "/home/user/a b;.txt");
        assert_eq!(path.file_name().unwrap().to_str().unwrap(), "a_b_.txt");

        let path_empty_slash = remote_cache_path(cache_root, "ssh:ws-1", "/");
        assert_eq!(
            path_empty_slash.file_name().unwrap().to_str().unwrap(),
            "file"
        );

        let path_trailing_slash = remote_cache_path(cache_root, "ssh:ws-1", "/dir/");
        assert_eq!(
            path_trailing_slash.file_name().unwrap().to_str().unwrap(),
            "file"
        );
    }

    #[test]
    fn test_remote_cache_path_stays_under_cache_root() {
        let cache_root = Path::new("/var/cache/ferryx");
        let p1 = remote_cache_path(cache_root, "ssh:ws-1", "/home/user/a b;.txt");
        assert!(p1.starts_with(cache_root));

        let p2 = remote_cache_path(cache_root, "ssh:ws-1", "/..");
        assert!(p2.starts_with(cache_root));

        let p3 = remote_cache_path(cache_root, "ssh:ws-1", "/");
        assert!(p3.starts_with(cache_root));

        let p4 = remote_cache_path(cache_root, "ssh:ws-1", "..");
        assert!(p4.starts_with(cache_root));
    }

    #[test]
    fn test_remote_read_command_quotes_path_with_single_quote() {
        let cmd = remote_read_command("/path/with'quote/file.txt");
        assert!(cmd.contains("'/path/with'\\''quote/file.txt'"));
        assert_eq!(
            cmd,
            format!(
                "test -f '/path/with'\\''quote/file.txt' || exit 3; printf FERRYX-REMOTE-FILE:; head -c {} -- '/path/with'\\''quote/file.txt'",
                REMOTE_FILE_MAX_BYTES + 1
            )
        );
    }

    #[test]
    fn windows_target_path_normalizes_absolute_and_relative_inputs() {
        let base = r"C:\Users\sook\repo";
        assert_eq!(
            windows_remote_target_path(r"src\lib.rs", base).unwrap(),
            r"C:\Users\sook\repo\src\lib.rs"
        );
        assert_eq!(
            windows_remote_target_path("src/../docs/a.md", r"C:\Users\sook\repo\").unwrap(),
            r"C:\Users\sook\repo\docs\a.md"
        );
        assert_eq!(
            windows_remote_target_path("d:/logs/./run.txt", base).unwrap(),
            r"D:\logs\run.txt"
        );
        assert_eq!(
            windows_remote_target_path(r"\\srv\share\a\..\b.txt", base).unwrap(),
            r"\\srv\share\b.txt"
        );
        assert_eq!(windows_remote_target_path("~", base).unwrap(), "~");
        assert_eq!(
            windows_remote_target_path("~/a/./b/../c.txt", base).unwrap(),
            r"~\a\c.txt"
        );
    }

    #[test]
    fn windows_target_path_rejects_escapes_and_ambiguous_forms() {
        let base = r"C:\repo";
        for input in [
            r"C:\..",
            r"..\..\x",
            r"\\srv\share\..\other",
            r"~\..",
            r"\rooted",
            "C:drive-relative",
            r"\\?\C:\x",
            r"\\srv",
            "",
            "a\nb",
        ] {
            let err = windows_remote_target_path(input, base).unwrap_err();
            assert_eq!(err.code, IpcErrorCode::InvalidRequest, "{input:?}");
            assert_eq!(
                err.details.as_ref().unwrap().get("reason").unwrap(),
                "invalidRemotePath",
                "{input:?}"
            );
        }
        assert!(windows_remote_target_path("rel.txt", "/home/u").is_err());
    }

    #[test]
    fn windows_read_script_passes_path_as_data_and_caps_read() {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        let hostile = r"C:\x\a'; Remove-Item -Recurse C:\ ;'.txt";
        let script = windows_remote_read_script(hostile);
        assert!(!script.contains("Remove-Item"), "{script}");
        assert!(script.contains(&STANDARD.encode(hostile)));
        assert!(script.contains("exit 3"));
        assert!(script.contains(&format!("$n={};", REMOTE_FILE_MAX_BYTES + 1)));

        let home = windows_remote_read_script(r"~\logs\run.png");
        assert!(home.starts_with("$p=Join-Path $HOME "), "{home}");
        assert!(home.contains(&STANDARD.encode(r"logs\run.png")));
        assert!(windows_remote_read_script("~").starts_with("$p=$HOME;"));
    }

    #[test]
    fn windows_payload_round_trips_binary_and_bounds_wire() {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        let data: Vec<u8> = (0..=255u8).collect();
        let wire = format!("{}\r\n", STANDARD.encode(&data));
        assert_eq!(decode_windows_payload(wire.as_bytes()).unwrap(), data);
        assert_eq!(
            decode_windows_payload(b"not base64!").unwrap_err().code,
            IpcErrorCode::ParseError
        );
        let max = vec![0u8; REMOTE_FILE_MAX_BYTES + 1];
        assert_eq!(STANDARD.encode(max).len(), windows_remote_wire_limit());
    }

    #[test]
    fn cache_path_uses_windows_basename() {
        let cache_root = Path::new("/var/cache/ferryx");
        let path = remote_cache_path(cache_root, "ssh:ws-1", r"C:\Users\sook\run.png");
        assert_eq!(path.file_name().unwrap().to_str().unwrap(), "run.png");
    }


    #[cfg(target_os = "macos")]
    #[test]
    fn mark_downloaded_sets_the_quarantine_xattr() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("run.command");
        std::fs::write(&file, b"echo hi").unwrap();
        mark_downloaded(&file);
        let c_path = CString::new(file.as_os_str().as_bytes()).unwrap();
        let name = CString::new("com.apple.quarantine").unwrap();
        let mut buf = vec![0u8; 256];
        let len = unsafe {
            libc::getxattr(
                c_path.as_ptr(),
                name.as_ptr(),
                buf.as_mut_ptr().cast(),
                buf.len(),
                0,
                0,
            )
        };
        assert!(len > 0, "com.apple.quarantine must be set on a fetched cache file");
        let value = String::from_utf8_lossy(&buf[..len as usize]).into_owned();
        assert!(value.starts_with("0081;"), "{value}");
        assert!(value.contains(";Ferryx;"), "{value}");
    }

    #[test]
    fn test_ensure_within_limit() {
        let max = REMOTE_FILE_MAX_BYTES;
        assert!(ensure_within_limit(0, max).is_ok());
        assert!(ensure_within_limit(max, max).is_ok());

        let err = ensure_within_limit(max + 1, max).unwrap_err();
        assert_eq!(err.code, IpcErrorCode::PayloadTooLarge);
        let details = err.details.as_ref().expect("details must be present");
        assert_eq!(
            details.get("reason").unwrap(),
            "remoteFileTooLarge"
        );
        assert_eq!(
            details.get("maxBytes").and_then(|v| v.as_u64()),
            Some(max as u64)
        );
        assert_eq!(
            details.get("maxBytes").unwrap(),
            &serde_json::json!(max)
        );
    }
}
