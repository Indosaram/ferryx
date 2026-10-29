//! First-contact host-key trust. Ordinary sessions keep `StrictHostKeyChecking=yes`; only the
//! explicit scan below lets OpenSSH record an unknown key, and only into a private temporary file.
//! A key reaches the user's known_hosts after the user confirmed its SHA256 fingerprint and a
//! fresh scan still presents exactly that key.
use super::SshHost;
use crate::ipc::{IpcError, IpcErrorCode};
use base64::{
    engine::general_purpose::STANDARD, engine::general_purpose::STANDARD_NO_PAD, Engine as _,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

const SCAN_DEADLINE: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScannedHostKey {
    pub key_type: String,
    pub fingerprint_sha256: String,
    /// The exact known_hosts line OpenSSH wrote for this host.
    pub line: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostKeyScan {
    pub keys: Vec<ScannedHostKey>,
}

fn host_key_error(code: IpcErrorCode, message: &str) -> IpcError {
    super::runtime::error(code, "host_key", message)
}

/// OpenSSH's `SHA256:` fingerprint of a base64 public-key blob.
pub fn fingerprint_sha256(blob_base64: &str) -> Option<String> {
    let blob = STANDARD.decode(blob_base64).ok()?;
    Some(format!(
        "SHA256:{}",
        STANDARD_NO_PAD.encode(Sha256::digest(&blob))
    ))
}

/// Parses the known_hosts text OpenSSH wrote during a scan. Marker, hashed and comment lines are
/// skipped: a scan file only ever holds plain `hosts keytype base64` lines.
pub fn parse_scan(text: &str) -> Vec<ScannedHostKey> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with('@'))
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let hosts = fields.next()?;
            let key_type = fields.next()?;
            let blob = fields.next()?;
            if hosts.starts_with('|') {
                return None;
            }
            Some(ScannedHostKey {
                key_type: key_type.to_string(),
                fingerprint_sha256: fingerprint_sha256(blob)?,
                line: format!("{hosts} {key_type} {blob}"),
            })
        })
        .collect()
}

fn hashed_host_matches(entry: &str, name: &str) -> bool {
    use hmac::{Hmac, Mac};
    let Some(rest) = entry.strip_prefix("|1|") else {
        return false;
    };
    let Some((salt, hash)) = rest.split_once('|') else {
        return false;
    };
    let (Ok(salt), Ok(hash)) = (STANDARD.decode(salt), STANDARD.decode(hash)) else {
        return false;
    };
    let Ok(mut mac) = Hmac::<sha1::Sha1>::new_from_slice(&salt) else {
        return false;
    };
    mac.update(name.as_bytes());
    mac.verify_slice(&hash).is_ok()
}

/// Whether `known_hosts` already holds any entry (plain, hashed, `@revoked` or
/// `@cert-authority`) for one of `names`. Wildcard patterns are not expanded.
pub fn has_entry_for(known_hosts: &str, names: &[&str]) -> bool {
    known_hosts.lines().any(|line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return false;
        }
        let mut fields = line.split_whitespace();
        let mut hosts = fields.next().unwrap_or_default();
        if hosts.starts_with('@') {
            hosts = fields.next().unwrap_or_default();
        }
        hosts.split(',').any(|entry| {
            names
                .iter()
                .any(|name| entry.eq_ignore_ascii_case(name) || hashed_host_matches(entry, name))
        })
    })
}

/// Picks the line to pin: the freshly scanned key whose fingerprint the user confirmed, and only
/// when known_hosts has no entry for that host yet. Appending next to an existing (changed) entry
/// would silence OpenSSH's changed-key protection, so that case is refused.
pub fn line_to_trust(
    scan: &[ScannedHostKey],
    confirmed_fingerprint: &str,
    known_hosts: &str,
) -> Result<String, IpcError> {
    let key = scan
        .iter()
        .find(|key| key.fingerprint_sha256 == confirmed_fingerprint.trim())
        .ok_or_else(|| {
            host_key_error(
                IpcErrorCode::HostKeyChanged,
                "The server now presents a different host key than the one you confirmed. Nothing was trusted.",
            )
        })?;
    let names: Vec<&str> = key
        .line
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .split(',')
        .collect();
    if has_entry_for(known_hosts, &names) {
        return Err(host_key_error(
            IpcErrorCode::HostKeyChanged,
            "known_hosts already has an entry for this host. Remove the stale entry (ssh-keygen -R) before trusting a new key.",
        ));
    }
    Ok(key.line.clone())
}

/// OpenSSH's default user known_hosts file; used only when `ssh -G` cannot name the real one.
pub fn user_known_hosts() -> Result<PathBuf, IpcError> {
    super::config::dirs_fallback_ssh_dir()
        .map(|dir| dir.join("known_hosts"))
        .ok_or_else(|| {
            IpcError::new(
                IpcErrorCode::NotFound,
                "No home directory for ~/.ssh/known_hosts",
            )
        })
}

/// Path argument for an ssh option value, quoted so whitespace survives OpenSSH's splitting.
fn option_path(path: &Path) -> Result<String, IpcError> {
    let text = path
        .to_str()
        .ok_or_else(|| IpcError::new(IpcErrorCode::InvalidPath, "Temporary path is not UTF-8"))?;
    let text = if cfg!(windows) {
        text.replace('\\', "/")
    } else {
        text.to_string()
    };
    if text.contains(['"', '%', '\n', '\r']) {
        return Err(IpcError::new(
            IpcErrorCode::InvalidPath,
            "Temporary directory path cannot be passed to OpenSSH",
        ));
    }
    Ok(format!("\"{text}\""))
}

/// Keep a connection to a not-yet-trusted server from authenticating: no password, key or agent is
/// offered, nothing is forwarded, nothing prompts, and OpenSSH's host-key messages stay visible.
/// OpenSSH keeps the first value of an option, so these precede the plan's options and ssh_config.
/// It never touches a control master: reusing one would skip the host-key exchange entirely.
const UNAUTHENTICATED: [&str; 8] = [
    "PreferredAuthentications=none",
    "BatchMode=yes",
    "ForwardAgent=no",
    "ForwardX11=no",
    "ClearAllForwardings=yes",
    "LogLevel=ERROR",
    "ControlMaster=no",
    "ControlPath=none",
];

/// `ssh_plan`'s password-login options. Dropping them keeps `password::environment` from attaching
/// the askpass helper, so the stored password is never reachable from these connections.
const PASSWORD_ONLY: [&str; 6] = [
    "BatchMode=no",
    "PreferredAuthentications=password,keyboard-interactive",
    "PasswordAuthentication=yes",
    "PubkeyAuthentication=no",
    "KbdInteractiveAuthentication=yes",
    "NumberOfPasswordPrompts=1",
];

/// The regular strict ssh plan for `host`, made unable to authenticate (see [`UNAUTHENTICATED`]).
/// It still runs OpenSSH's own host-key check against every known_hosts file it would consult.
pub fn unauthenticated_plan(
    host: &SshHost,
) -> Result<crate::terminal::shell::ShellCommandPlan, IpcError> {
    let mut plan = super::direct::ssh_plan(host, "exit 0".into(), false)?;
    let original = std::mem::take(&mut plan.args);
    let mut args: Vec<String> = UNAUTHENTICATED
        .iter()
        .flat_map(|option| ["-o".to_string(), (*option).to_string()])
        .collect();
    let mut index = 0;
    while index < original.len() {
        if original[index] == "-o"
            && original
                .get(index + 1)
                .is_some_and(|value| PASSWORD_ONLY.contains(&value.as_str()))
        {
            index += 2;
            continue;
        }
        args.push(original[index].clone());
        index += 1;
    }
    plan.args = args;
    Ok(plan)
}

/// The first `userknownhostsfile` entry of `ssh -G` output: the file OpenSSH itself reads first,
/// with `~` and `%` tokens already expanded by OpenSSH. `ssh -G` joins entries with spaces without
/// quoting, so a path containing spaces (a Windows profile such as `C:\Users\Jane Doe`) is rebuilt
/// from a run of words that ends where the next entry starts (a word beginning an absolute path):
/// the shortest such run that names an existing file, else the shortest such run.
pub fn first_user_known_hosts(ssh_g: &str, is_file: impl Fn(&str) -> bool) -> Option<String> {
    let value = ssh_g
        .lines()
        .find_map(|line| line.strip_prefix("userknownhostsfile "))?
        .trim();
    if value.is_empty() || value.eq_ignore_ascii_case("none") {
        return None;
    }
    let words: Vec<&str> = value.split(' ').filter(|word| !word.is_empty()).collect();
    let starts_path = |word: &str| {
        let bytes = word.as_bytes();
        word.starts_with(['/', '\\', '~'])
            || (bytes.len() >= 3
                && bytes[0].is_ascii_alphabetic()
                && bytes[1] == b':'
                && matches!(bytes[2], b'/' | b'\\'))
    };
    let candidates: Vec<String> = (1..=words.len())
        .filter(|&count| count == words.len() || starts_path(words[count]))
        .map(|count| words[..count].join(" "))
        .collect();
    candidates
        .iter()
        .find(|path| is_file(path))
        .or(candidates.first())
        .cloned()
}

/// The known_hosts file trust writes to: the first user file `ssh -G` reports for this host (it
/// honours ssh_config `UserKnownHostsFile` and expands `~` from the account, not `$HOME`).
async fn known_hosts_target(host: &SshHost) -> Result<PathBuf, IpcError> {
    let mut plan = unauthenticated_plan(host)?;
    plan.args.insert(0, "-G".to_string());
    match super::direct::bounded_output(&plan, SCAN_DEADLINE).await {
        Ok(stdout) => {
            let text = String::from_utf8_lossy(&stdout);
            // A relative path (or an MSYS-style `/home/...` path on Windows) is not one this
            // process can open as OpenSSH would, so it falls back to the default.
            if let Some(path) = first_user_known_hosts(&text, |path| Path::new(path).is_file())
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
            {
                return Ok(path);
            }
            tracing::warn!(
                "ssh -G named no absolute user known_hosts file; using the default path"
            );
        }
        Err(error) => tracing::warn!("ssh -G failed ({error}); using the default known_hosts path"),
    }
    user_known_hosts()
}

/// Whether the strict, unauthenticated probe failed with OpenSSH's "No <type> host key is known"
/// line. Any other outcome (a changed or wildcard/global/known_hosts2 entry, a session that
/// opened, a probe that could not run) means the host is not new.
pub fn probe_reports_new_host(probe: &Result<Vec<u8>, IpcError>) -> bool {
    let Err(error) = probe else {
        return false;
    };
    error
        .details
        .as_ref()
        .and_then(|details| details.get("stderr"))
        .and_then(|stderr| stderr.as_str())
        .is_some_and(super::runtime::host_key_is_new)
}

/// Builds the scan command: the unauthenticated plan with host-key acceptance redirected into
/// `scan_file` only. The user's own known_hosts is neither read nor written.
pub fn scan_plan(
    host: &SshHost,
    scan_file: &Path,
) -> Result<crate::terminal::shell::ShellCommandPlan, IpcError> {
    let mut plan = unauthenticated_plan(host)?;
    for arg in &mut plan.args {
        if arg == "StrictHostKeyChecking=yes" {
            *arg = "StrictHostKeyChecking=accept-new".into();
        }
    }
    let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
    // OpenSSH keeps the first value of an option, so these precede everything else.
    plan.args.splice(
        0..0,
        [
            "-o".to_string(),
            format!("UserKnownHostsFile={}", option_path(scan_file)?),
            "-o".to_string(),
            format!("GlobalKnownHostsFile={null}"),
            "-o".to_string(),
            "HashKnownHosts=no".to_string(),
        ],
    );
    Ok(plan)
}

/// Connects once with `accept-new` into a private file and reports the key the server presented.
/// The connection cannot authenticate (see [`UNAUTHENTICATED`]) and its failure does not matter:
/// OpenSSH records the host key before authentication.
pub async fn scan(host: &SshHost) -> Result<HostKeyScan, IpcError> {
    let dir = crate::ipc::run_blocking(|| {
        tempfile::Builder::new()
            .prefix("ferryx-hostkey")
            .tempdir()
            .map_err(|error| IpcError::new(IpcErrorCode::IoError, error.to_string()))
    })
    .await?;
    let file = dir.path().join("known_hosts");
    let plan = scan_plan(host, &file)?;
    let outcome = super::direct::bounded_output(&plan, SCAN_DEADLINE).await;
    let text = match std::fs::read_to_string(&file) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(IpcError::new(IpcErrorCode::IoError, error.to_string())),
    };
    let keys = parse_scan(&text);
    if keys.is_empty() {
        return Err(outcome.err().unwrap_or_else(|| {
            host_key_error(
                IpcErrorCode::IoError,
                "The server did not present a host key",
            )
        }));
    }
    Ok(HostKeyScan { keys })
}

fn append_line(path: &Path, line: &str) -> Result<(), IpcError> {
    let io = |error: std::io::Error| IpcError::new(IpcErrorCode::IoError, error.to_string());
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent).map_err(io)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
                    .map_err(io)?;
            }
        }
    }
    let existing = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(io(error)),
    };
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(io)?;
    let separator = if existing.is_empty() || existing.ends_with(b"\n") {
        ""
    } else {
        "\n"
    };
    file.write_all(format!("{separator}{line}\n").as_bytes())
        .map_err(io)?;
    file.sync_all().map_err(io)
}

/// Pins the host key whose fingerprint the user confirmed into `known_hosts`.
pub fn trust_into(
    known_hosts: &Path,
    scan: &[ScannedHostKey],
    confirmed_fingerprint: &str,
) -> Result<String, IpcError> {
    let existing = match std::fs::read_to_string(known_hosts) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(IpcError::new(IpcErrorCode::IoError, error.to_string())),
    };
    let line = line_to_trust(scan, confirmed_fingerprint, &existing)?;
    append_line(known_hosts, &line)?;
    Ok(line)
}

/// Pins the server's key only when OpenSSH itself reports the host as new (its own resolution of
/// every known_hosts file, hashed names and wildcards) and a fresh scan still presents the key whose
/// fingerprint the user confirmed.
pub async fn trust(host: &SshHost, confirmed_fingerprint: String) -> Result<(), IpcError> {
    let probe = super::direct::bounded_output(&unauthenticated_plan(host)?, SCAN_DEADLINE).await;
    if !probe_reports_new_host(&probe) {
        return Err(host_key_error(
            IpcErrorCode::HostKeyChanged,
            "OpenSSH does not report this host as new: a known_hosts entry already covers it, or the check could not run. Nothing was trusted.",
        ));
    }
    let scanned = scan(host).await?;
    let path = known_hosts_target(host).await?;
    crate::ipc::run_blocking(move || {
        trust_into(&path, &scanned.keys, &confirmed_fingerprint).map(|_| ())
    })
    .await
}

#[cfg(test)]
#[path = "host_key_tests.rs"]
mod tests;
