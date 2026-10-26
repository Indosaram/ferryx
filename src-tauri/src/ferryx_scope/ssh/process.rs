//! Blocking portable stdio adapter. Call via spawn_blocking from async application code.
use super::helper::{read_frame, write_frame, Request, Runtime};
use serde_json::{json, Value};
#[cfg(any(target_os = "linux", test))]
use std::ffi::OsString;
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex, MutexGuard},
    time::{Duration, Instant},
};

pub const HELPER_VERSION: &str = "2026.1025.2";

/// Capability tokens this helper can advertise, in handshake order. The last one
/// is bound at runtime, so the handshake omits it when the listener is unavailable.
pub const HELPER_CAPABILITIES: [&str; 5] = [
    "sshHelperV1",
    "dagStreamingV1",
    "dagSubscribeV1",
    AGENT_HOOK_CAPABILITY,
    "agentStateV1",
];

/// Advertised by helpers that answer `agent-hook <agent> <event>`. A host whose staged helper
/// predates it (same version string, older build) never gets remote hooks that would fail.
pub const AGENT_HOOK_CAPABILITY: &str = "agentHookV1";

/// The single capability that depends on a runtime binding rather than the build.
pub const CONDITIONAL_AGENT_STATE: &str = "agentStateV1";

/// Machine-readable capability advertisement shared by both stdio adapters.
pub fn capabilities_report() -> String {
    json!({
        "protocol": 1,
        "helperVersion": HELPER_VERSION,
        "capabilities": HELPER_CAPABILITIES,
        "runtimeConditional": [CONDITIONAL_AGENT_STATE],
    })
    .to_string()
}

pub fn run_with_io(
    args: impl IntoIterator<Item = String>,
    mut out: impl Write,
) -> Result<(), String> {
    let args: Vec<_> = args.into_iter().collect();
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        writeln!(out, "{HELPER_VERSION}").map_err(|e| e.to_string())?;
        return Ok(());
    }
    if args.iter().any(|arg| arg == "--capabilities") {
        writeln!(out, "{}", capabilities_report()).map_err(|e| e.to_string())?;
        return Ok(());
    }
    run(args)
}

#[cfg(windows)]
mod process_windows;

#[derive(serde::Serialize, serde::Deserialize)]
struct Endpoint {
    address: String,
    token: String,
    #[serde(default)]
    pid: Option<u32>,
}

fn reply(runtime: &Runtime, value: Value, connection: u64) -> Value {
    match serde_json::from_value::<Request>(value)
        .map_err(|e| e.to_string())
        .and_then(|r| runtime.handle_on_connection(r, connection))
    {
        Ok(data) => json!({"ok":true,"data":data}),
        Err(error) => json!({"ok":false,"error":error}),
    }
}
fn serve(mut stream: impl Read + Write, runtime: Arc<Runtime>) -> Result<(), String> {
    // Non-zero, unique per accepted connection: DAG subscriptions are owned by
    // the connection that opened them and released on its EOF. PTY sessions are
    // deliberately NOT connection-scoped.
    static NEXT_CONNECTION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let connection = NEXT_CONNECTION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let result = (|| -> Result<(), String> {
        while let Some(request) = read_frame(&mut stream)? {
            write_frame(&mut stream, &reply(&runtime, request, connection))?;
        }
        Ok(()) // Bridge EOF releases only this connection; Runtime owns every PTY.
    })();
    runtime.release_connection(connection);
    result
}
fn validate_private(path: &Path) -> Result<std::fs::Metadata, String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if metadata.file_type().is_symlink() {
        return Err("FORBIDDEN: helper IPC cannot be a symlink".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // SAFETY: geteuid has no arguments, pointers, or caller preconditions.
        let uid = unsafe { libc::geteuid() };
        if metadata.uid() != uid || metadata.mode() & 0o077 != 0 {
            return Err(
                "FORBIDDEN: helper IPC must be owned by the current user and private".into(),
            );
        }
        if metadata.is_file() && metadata.nlink() != 1 {
            return Err("FORBIDDEN: helper IPC cannot have hard links".into());
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("FORBIDDEN: helper IPC cannot be a reparse point".into());
        }
        validate_windows_acl(path)?;
    }
    Ok(metadata)
}

#[cfg(windows)]
fn validate_windows_acl(path: &Path) -> Result<(), String> {
    use base64::Engine as _;
    use std::os::windows::process::CommandExt as _;
    let path_str = path
        .to_str()
        .ok_or_else(|| "FORBIDDEN: helper IPC path is invalid UTF-8".to_string())?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(path_str.as_bytes());
    let script = format!(
        r#"$ProgressPreference='SilentlyContinue';$ErrorActionPreference='Stop';$raw=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{encoded}'));if($raw.StartsWith('\\?\UNC\')){{$raw='\\'+$raw.Substring(8)}}elseif($raw.StartsWith('\\?\')){{$raw=$raw.Substring(4)}};$p=[System.IO.Path]::GetFullPath($raw);$item=Get-Item -LiteralPath $p -Force;$acl=$item.GetAccessControl();$u=[System.Security.Principal.WindowsIdentity]::GetCurrent();$allowed=@($u.User.Value,'S-1-5-18','S-1-5-32-544');$owner=try{{$acl.GetOwner([System.Security.Principal.SecurityIdentifier]).Value}}catch{{$null}};if(-not $owner){{try{{$owner=(New-Object System.Security.Principal.NTAccount($acl.Owner)).Translate([System.Security.Principal.SecurityIdentifier]).Value}}catch{{$owner=$acl.Owner}}}};if($allowed -notcontains $owner){{exit 1}};$rules=$acl.Access;if($null -eq $rules -or $rules.Count -eq 0){{exit 2}};$hasAllowed=$false;foreach($r in $rules){{if($r.AccessControlType.ToString() -eq 'Allow'){{$sid=try{{$r.IdentityReference.Translate([System.Security.Principal.SecurityIdentifier]).Value}}catch{{$r.IdentityReference.Value}};if($allowed -notcontains $sid){{exit 3}};$hasAllowed=$true}}}};if(-not $hasAllowed){{exit 4}};exit 0;"#
    );
    let output = std::process::Command::new("powershell.exe")
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ])
        .output();
    let output = output
        .map_err(|e| format!("FORBIDDEN: cannot execute PowerShell ACL validation: {e}"))?;

    if !output.status.success() {
        return Err("FORBIDDEN: helper IPC must be owned by the current user and private".into());
    }
    Ok(())
}

fn endpoint(root: &Path) -> Result<Endpoint, String> {
    if !validate_private(root)?.is_dir() {
        return Err("FORBIDDEN: helper root must be a private directory".into());
    }
    let path = root.join("endpoint.json");
    if !validate_private(&path)?.is_file() {
        return Err("FORBIDDEN: helper endpoint must be a regular file".into());
    }
    let value: Endpoint = serde_json::from_slice(
        &std::fs::read(&path).map_err(|e| format!("REMOTE_RUNTIME_MISSING: {e}"))?,
    )
    .map_err(|e| e.to_string())?;
    #[cfg(unix)]
    if Path::new(&value.address)
        != root
            .canonicalize()
            .map_err(|e| e.to_string())?
            .join("helper.sock")
    {
        return Err("FORBIDDEN: helper socket outside private runtime".into());
    }
    Ok(value)
}

#[cfg(unix)]
type Listener = std::os::unix::net::UnixListener;
#[cfg(not(unix))]
type Listener = std::net::TcpListener;

struct BoundRuntime {
    listener: Listener,
    runtime: Arc<Runtime>,
    _lock: std::fs::File,
}

fn bind_runtime(root: &Path, host: String) -> Result<BoundRuntime, String> {
    if !root.try_exists().map_err(|e| e.to_string())? {
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(root).map_err(|e| e.to_string())?;
        super::private_file(root)?;
    }
    if !validate_private(root)?.is_dir() {
        return Err("FORBIDDEN: helper root must be a private directory".into());
    }
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let lock_path = root.join("runtime.lock");
    if lock_path.symlink_metadata().is_ok() && !validate_private(&lock_path)?.is_file() {
        return Err("FORBIDDEN: helper lock must be a regular file".into());
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let lock = options.open(&lock_path).map_err(|e| e.to_string())?;
    super::private_file(&lock_path)?;
    lock.try_lock()
        .map_err(|e| format!("REMOTE_RUNTIME_CONFLICT: {e}"))?;
    // The OS lock, not a failed connection probe, authorizes stale-file removal.
    let path = root.join("endpoint.json");
    if path.symlink_metadata().is_ok() {
        let previous = endpoint(&root)?;
        #[cfg(unix)]
        let live = std::os::unix::net::UnixStream::connect(&previous.address).is_ok();
        #[cfg(not(unix))]
        let live = {
            let address: std::net::SocketAddr = previous
                .address
                .parse()
                .map_err(|e: std::net::AddrParseError| e.to_string())?;
            if !address.ip().is_loopback() {
                return Err("FORBIDDEN: helper IPC must be loopback".into());
            }
            std::net::TcpStream::connect_timeout(&address, std::time::Duration::from_secs(1))
                .is_ok()
        };
        if live {
            return Err("REMOTE_RUNTIME_CONFLICT: existing helper is still listening".into());
        }
    }
    let token = format!("{}{}", uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    let runtime = Arc::new(Runtime::new(root.clone(), host, token.clone())?);
    #[cfg(unix)]
    let (listener, address) = {
        let socket = root.join("helper.sock");
        if socket.symlink_metadata().is_ok() {
            use std::os::unix::fs::FileTypeExt;
            if !validate_private(&socket)?.file_type().is_socket() {
                return Err("FORBIDDEN: helper socket path is not a socket".into());
            }
            if std::os::unix::net::UnixStream::connect(&socket).is_ok() {
                return Err("REMOTE_RUNTIME_CONFLICT: existing helper is still listening".into());
            }
            std::fs::remove_file(&socket).map_err(|e| e.to_string())?;
        }
        let listener =
            std::os::unix::net::UnixListener::bind(&socket).map_err(|e| e.to_string())?;
        super::private_file(&socket)?;
        (listener, socket.to_string_lossy().into_owned())
    };
    #[cfg(not(unix))]
    let (listener, address) = {
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .map_err(|e| e.to_string())?;
        let address = listener
            .local_addr()
            .map_err(|e| e.to_string())?
            .to_string();
        (listener, address)
    };
    let staged = root.join(format!("endpoint-{}.tmp", uuid::Uuid::new_v4()));
    let mut publish = std::fs::OpenOptions::new();
    publish.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        publish.mode(0o600);
    }
    let publication = (|| {
        let mut file = publish.open(&staged).map_err(|e| e.to_string())?;
        super::private_file(&staged)?;
        file.write_all(
            &serde_json::to_vec(&Endpoint {
                address,
                token,
                pid: Some(std::process::id()),
            })
            .map_err(|e| e.to_string())?,
        )
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
        drop(file);
        #[cfg(windows)]
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
        std::fs::rename(&staged, &path).map_err(|e| e.to_string())
    })();
    if publication.is_err() {
        let _ = std::fs::remove_file(&staged);
    }
    publication?;
    Ok(BoundRuntime {
        listener,
        runtime,
        _lock: lock,
    })
}

pub fn is_live(root: &Path, expected_host: &str) -> bool {
    let Ok(ep) = endpoint(root) else {
        return false;
    };
    #[cfg(unix)]
    let mut stream = {
        use std::os::unix::fs::FileTypeExt;
        if !validate_private(Path::new(&ep.address))
            .map(|m| m.file_type().is_socket())
            .unwrap_or(false)
        {
            return false;
        }
        let Ok(s) = std::os::unix::net::UnixStream::connect(&ep.address) else {
            return false;
        };
        let _ = s.set_read_timeout(Some(std::time::Duration::from_millis(500)));
        let _ = s.set_write_timeout(Some(std::time::Duration::from_millis(500)));
        s
    };
    #[cfg(not(unix))]
    let mut stream = {
        let Ok(address) = ep.address.parse::<std::net::SocketAddr>() else {
            return false;
        };
        if !address.ip().is_loopback() {
            return false;
        }
        let Ok(s) =
            std::net::TcpStream::connect_timeout(&address, std::time::Duration::from_millis(500))
        else {
            return false;
        };
        let _ = s.set_read_timeout(Some(std::time::Duration::from_millis(500)));
        let _ = s.set_write_timeout(Some(std::time::Duration::from_millis(500)));
        s
    };

    let handshake_req = json!({
        "protocol": 1,
        "token": ep.token,
        "op": "handshake",
        "params": {}
    });
    if write_frame(&mut stream, &handshake_req).is_err() {
        return false;
    }
    let Ok(Some(resp)) = read_frame(&mut stream) else {
        return false;
    };
    let is_ok = resp.get("ok").and_then(Value::as_bool) == Some(true);
    let proto_match = resp
        .get("data")
        .and_then(|d| d.get("protocol"))
        .and_then(Value::as_u64)
        == Some(1);
    let host_match = resp
        .get("data")
        .and_then(|d| d.get("hostId"))
        .and_then(Value::as_str)
        == Some(expected_host);
    is_ok && proto_match && host_match
}

pub fn daemon(root: PathBuf, host: String) -> Result<(), String> {
    let bound = bind_runtime(&root, host)?;
    println!("{}", json!({"event":"ready","protocol":1}));
    std::io::stdout().flush().map_err(|e| e.to_string())?;
    super::hygiene::start();
    for incoming in bound.listener.incoming() {
        let stream = incoming.map_err(|e| e.to_string())?;
        let runtime = bound.runtime.clone();
        std::thread::spawn(move || {
            if let Err(error) = serve(stream, runtime) {
                eprintln!("SSH_HELPER_CONNECTION: {error}");
            }
        });
    }
    Ok(())
}

pub fn start(root: PathBuf, host: String) -> Result<(), String> {
    if root.symlink_metadata().is_ok() {
        if !validate_private(&root)?.is_dir() {
            return Err("FORBIDDEN: helper root must be a private directory".into());
        }
    } else {
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&root).map_err(|e| e.to_string())?;
        super::private_file(&root)?;
    }

    let root = root.canonicalize().map_err(|e| e.to_string())?;

    if is_live(&root, &host) {
        println!("{}", json!({"event":"ready","protocol":1}));
        std::io::stdout().flush().map_err(|e| e.to_string())?;
        return Ok(());
    }

    #[cfg(windows)]
    {
        return process_windows::start(&root, &host);
    }

    #[cfg(not(windows))]
    {
        let current_exe =
            std::env::current_exe().map_err(|e| format!("REMOTE_RUNTIME_INVALID: {e}"))?;
        #[cfg(target_os = "linux")]
        {
            match try_systemd_unit(&current_exe, &root, &host) {
                Ok(()) => {
                    println!("{}", json!({"event":"ready","protocol":1}));
                    std::io::stdout().flush().map_err(|e| e.to_string())?;
                    return Ok(());
                }
                Err(UnitLaunchError::Unavailable(reason)) => {
                    eprintln!("SSH_HELPER_UNIT_FALLBACK: {reason}");
                }
                Err(UnitLaunchError::Failed(message)) => return Err(message),
            }
        }
        let mut command = std::process::Command::new(&current_exe);
        let root_str = root
            .to_str()
            .ok_or("REMOTE_RUNTIME_INVALID: root is not valid UTF-8")?;
        command.args(["daemon", "--root", root_str, "--host-id", &host]);
        command.stdin(std::process::Stdio::null());
        command.stdout(std::process::Stdio::null());

        let log_path = root.join(format!("startup-{}.tmp", uuid::Uuid::new_v4()));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let log_file = options.open(&log_path).map_err(|e| e.to_string())?;
        super::private_file(&log_path)?;
        command.stderr(log_file);

        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // SAFETY: libc::setsid is an async-signal-safe POSIX syscall with no pointers,
            // allocations, or preconditions. Calling it between fork and exec creates a new
            // session leader without a controlling terminal, cleanly decoupling the daemon from
            // the caller's SSH session while allowing PTY shells to manage their own signals.
            unsafe {
                command.pre_exec(|| {
                    if libc::setsid() == -1 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }

        let mut child = command.spawn().map_err(|e| {
            let _ = std::fs::remove_file(&log_path);
            format!("REMOTE_RUNTIME_SPAWN_FAILED: {e}")
        })?;

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            if is_live(&root, &host) {
                let _ = std::fs::remove_file(&log_path);
                println!("{}", json!({"event":"ready","protocol":1}));
                std::io::stdout().flush().map_err(|e| e.to_string())?;
                return Ok(());
            }
            if let Ok(Some(status)) = child.try_wait() {
                let detail = std::fs::read_to_string(&log_path).unwrap_or_default();
                let _ = std::fs::remove_file(&log_path);
                let detail = detail.trim();
                return if detail.is_empty() {
                    Err(format!("REMOTE_RUNTIME_EXITED: helper daemon exited prematurely with status {status}"))
                } else {
                    Err(format!(
                        "REMOTE_RUNTIME_EXITED: helper daemon exited ({status}): {detail}"
                    ))
                };
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_file(&log_path);
        Err("REMOTE_RUNTIME_TIMEOUT: helper daemon failed to become ready within deadline".into())
    }
}

#[cfg(any(target_os = "linux", test))]
/// Unit identity is (host, runtime root): one host with two qualified roots must never
/// share a unit - an active unit for another root would otherwise satisfy the
/// "already running" check while the requested root stays dead - and a repeated
/// start for the same root stays idempotent. Units from older roots keep serving
/// their own roots untouched.
///
/// [`root_unit_digest`] only disambiguates names (it is not a security boundary), so
/// no hashing dependency is added here.
fn helper_unit_name(host: &str, root: &Path) -> String {
    let mut sanitized: String = host
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
                ch
            } else {
                '-'
            }
        })
        .collect();
    if sanitized.is_empty() {
        sanitized.push_str("default");
    }
    sanitized.truncate(64);
    format!("ferryx-helper-{sanitized}-{}", root_unit_digest(root))
}

/// Standard-library hash of the runtime root path, rendered as 16 lowercase hex
/// characters. Only a name disambiguator: an algorithm change across Rust releases
/// would merely mint a new unit name for the same root, and a same-root conflict is
/// still caught by the socket/endpoint liveness check.
#[cfg(any(target_os = "linux", test))]
fn root_unit_digest(root: &Path) -> String {
    use std::hash::{Hash as _, Hasher as _};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    root.to_string_lossy().as_ref().hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

#[cfg(any(target_os = "linux", test))]
fn valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' => {}
        _ => return false,
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

#[cfg(any(target_os = "linux", test))]
fn forwarded_env(env: &[(OsString, OsString)]) -> Vec<(String, String)> {
    const DENY: [&str; 5] = [
        "SSH_CONNECTION",
        "SSH_CLIENT",
        "SSH_TTY",
        "SSH_SESSION_ID",
        "SSH_ASKPASS",
    ];
    env.iter()
        .filter_map(|(key, value)| {
            let key = key.to_str()?;
            if DENY.contains(&key) || !valid_env_name(key) {
                return None;
            }
            let value = value.to_str()?;
            if value.is_empty() {
                return None;
            }
            Some((key.to_string(), value.to_string()))
        })
        .collect()
}

#[cfg(any(target_os = "linux", test))]
pub(crate) fn unit_launch_args(
    exe: &Path,
    root: &Path,
    host: &str,
    env: &[(OsString, OsString)],
) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec![
        "--user".into(),
        format!("--unit={}", helper_unit_name(host, root)).into(),
        "--collect".into(),
        "--quiet".into(),
    ];
    for (key, value) in forwarded_env(env) {
        args.push(format!("--setenv={key}={value}").into());
    }
    args.push("--".into());
    args.push(exe.as_os_str().to_os_string());
    args.push("daemon".into());
    args.push("--root".into());
    args.push(root.as_os_str().to_os_string());
    args.push("--host-id".into());
    args.push(host.into());
    args
}

#[cfg(target_os = "linux")]
enum UnitLaunchError {
    Unavailable(String),
    Failed(String),
}

#[cfg(target_os = "linux")]
fn unit_active_state(unit: &str) -> Result<String, UnitLaunchError> {
    let output = std::process::Command::new("systemctl")
        .args(["--user", "show", unit, "-p", "ActiveState", "--value"])
        .output()
        .map_err(|e| UnitLaunchError::Unavailable(format!("SYSTEMD_UNAVAILABLE: {e}")))?;
    if !output.status.success() {
        return Err(UnitLaunchError::Unavailable(format!(
            "SYSTEMD_UNAVAILABLE: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(target_os = "linux")]
fn try_systemd_unit(exe: &Path, root: &Path, host: &str) -> Result<(), UnitLaunchError> {
    let unit = helper_unit_name(host, root);
    let active = matches!(
        unit_active_state(&unit)?.as_str(),
        "active" | "activating" | "reloading"
    );
    if !active {
        let env: Vec<(OsString, OsString)> = std::env::vars_os().collect();
        let args = unit_launch_args(exe, root, host, &env);
        let output = std::process::Command::new("systemd-run")
            .args(&args)
            .output()
            .map_err(|e| {
                UnitLaunchError::Unavailable(format!("REMOTE_RUNTIME_SPAWN_FAILED: systemd-run: {e}"))
            })?;
        if !output.status.success() {
            let recheck = matches!(
                unit_active_state(&unit).unwrap_or_default().as_str(),
                "active" | "activating" | "reloading"
            );
            if !recheck {
                return Err(UnitLaunchError::Unavailable(format!(
                    "REMOTE_RUNTIME_SPAWN_FAILED: systemd-run: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                )));
            }
        }
    }

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if is_live(root, host) {
            return Ok(());
        }
        match unit_active_state(&unit)?.as_str() {
            "failed" | "inactive" => {
                return Err(UnitLaunchError::Failed(format!(
                    "REMOTE_RUNTIME_EXITED: systemd unit {unit} is not running"
                )))
            }
            _ => std::thread::sleep(std::time::Duration::from_millis(50)),
        }
    }
    Err(UnitLaunchError::Failed(format!(
        "REMOTE_RUNTIME_TIMEOUT: helper daemon did not become ready via systemd unit {unit}"
    )))
}

/// How long `bridge --stdio` waits for the next client request before it exits.
///
/// An SSH server cannot be relied on to end a session whose client vanished: Tailscale SSH and
/// OpenSSH with the default `ClientAliveInterval 0` keep an idle channel open indefinitely, so
/// a bridge blocked on stdin would outlive its client for as long as the host stays up. Desktop
/// clients send a request on every bridge connection at least once per
/// `ssh::bridge::KEEPALIVE_INTERVAL`, well inside this bound.
pub const BRIDGE_IDLE_TIMEOUT: Duration = Duration::from_secs(180);

const BRIDGE_OPS: [&str; 16] = [
    "handshake",
    "project.register",
    "project.list",
    "worktree.create",
    "pty.spawn",
    "pty.list",
    "pty.describe",
    "pty.read",
    "pty.write",
    "pty.resize",
    "pty.stop",
    "dag.inventory",
    "dag.poll",
    "dag.subscribe",
    "dag.next",
    "dag.unsubscribe",
];

#[cfg(unix)]
type HelperIpc = std::os::unix::net::UnixStream;
#[cfg(not(unix))]
type HelperIpc = std::net::TcpStream;

fn connect_helper_ipc(address: &str) -> Result<HelperIpc, String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        if !validate_private(Path::new(address))?
            .file_type()
            .is_socket()
        {
            return Err("FORBIDDEN: helper IPC must be a socket".into());
        }
        std::os::unix::net::UnixStream::connect(address).map_err(|e| e.to_string())
    }
    #[cfg(not(unix))]
    {
        let address: std::net::SocketAddr = address
            .parse()
            .map_err(|e: std::net::AddrParseError| e.to_string())?;
        if !address.ip().is_loopback() {
            return Err("FORBIDDEN: helper IPC must be loopback".into());
        }
        std::net::TcpStream::connect(address).map_err(|e| e.to_string())
    }
}

struct BridgeState {
    /// Requests forwarded to the daemon whose response has not been written back yet.
    pending: usize,
    last_request: Instant,
    end: Option<Result<(), String>>,
}

/// `output` has its own lock so a client that stops reading can stall only the writer,
/// never the thread that enforces the idle bound.
struct BridgeShared<O> {
    state: Mutex<BridgeState>,
    output: Mutex<O>,
    changed: Condvar,
}

impl<O> BridgeShared<O> {
    fn lock(&self) -> MutexGuard<'_, BridgeState> {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn output(&self) -> MutexGuard<'_, O> {
        self.output.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Waits until every forwarded request has been answered or the bridge has ended.
    fn settled<'a>(&'a self, state: MutexGuard<'a, BridgeState>) -> MutexGuard<'a, BridgeState> {
        self.changed
            .wait_while(state, |s| s.pending != 0 && s.end.is_none())
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Records the first reason the bridge ends; later reasons are consequences of it.
    fn finish(&self, result: Result<(), String>) {
        let mut state = self.lock();
        if state.end.is_none() {
            state.end = Some(result);
        }
        self.changed.notify_all();
    }
}

fn forward_requests<O: Write>(
    input: &mut impl Read,
    daemon: &mut impl Write,
    token: &str,
    shared: &BridgeShared<O>,
) -> Result<(), String> {
    while let Some(mut request) = read_frame(input)? {
        let op = request.get("op").and_then(Value::as_str).unwrap_or("");
        let allowed = BRIDGE_OPS.contains(&op);
        let mut state = shared.lock();
        state.last_request = Instant::now();
        if !allowed {
            // Responses must stay in request order, so earlier forwarded requests go first.
            if shared.settled(state).end.is_some() {
                return Ok(());
            }
            write_frame(
                &mut *shared.output(),
                &json!({"ok":false,"error":"UNSUPPORTED: operation not allowlisted"}),
            )?;
            continue;
        }
        state.pending += 1;
        drop(state);
        request["token"] = json!(token);
        write_frame(daemon, &request).map_err(|e| format!("REMOTE_UNAVAILABLE: {e}"))?;
    }
    // Client EOF: every forwarded request still receives its response before the bridge ends.
    drop(shared.settled(shared.lock()));
    Ok(())
}

fn forward_responses<O: Write>(
    daemon: &mut impl Read,
    shared: &BridgeShared<O>,
) -> Result<(), String> {
    loop {
        let response = read_frame(daemon)
            .map_err(|e| format!("REMOTE_UNAVAILABLE: {e}"))?
            .ok_or_else(|| "REMOTE_UNAVAILABLE: daemon EOF".to_string())?;
        if shared.lock().pending == 0 {
            return Err("PROTOCOL_ERROR: helper daemon sent an unsolicited frame".into());
        }
        // Only this thread decrements `pending`, so the check above still holds here.
        write_frame(&mut *shared.output(), &response)?;
        let mut state = shared.lock();
        state.pending -= 1;
        shared.changed.notify_all();
    }
}

/// Relays framed requests from `input` to the helper daemon and responses back to `output`.
///
/// The bridge ends on whichever comes first: client EOF (after every forwarded request is
/// answered), the daemon side closing or failing, or `BRIDGE_IDLE_TIMEOUT` without a client
/// request while nothing is in flight. The daemon side is read continuously, so a daemon that
/// dies or is replaced ends its bridges immediately instead of when the next request arrives.
///
/// A blocking stdin read cannot be interrupted portably, so the request thread is never joined:
/// the binary exits when this returns, which ends the thread and closes the SSH channel.
pub fn bridge<I, O>(root: &Path, input: I, output: O) -> Result<(), String>
where
    I: Read + Send + 'static,
    O: Write + Send + 'static,
{
    bridge_with_idle_timeout(root, input, output, BRIDGE_IDLE_TIMEOUT)
}

fn bridge_with_idle_timeout<I, O>(
    root: &Path,
    mut input: I,
    output: O,
    idle_timeout: Duration,
) -> Result<(), String>
where
    I: Read + Send + 'static,
    O: Write + Send + 'static,
{
    let endpoint = endpoint(root)?;
    let stream = connect_helper_ipc(&endpoint.address)?;
    let mut to_daemon = stream.try_clone().map_err(|e| e.to_string())?;
    let mut from_daemon = stream.try_clone().map_err(|e| e.to_string())?;
    let shared = Arc::new(BridgeShared {
        state: Mutex::new(BridgeState {
            pending: 0,
            last_request: Instant::now(),
            end: None,
        }),
        output: Mutex::new(output),
        changed: Condvar::new(),
    });

    let requests = Arc::clone(&shared);
    let token = endpoint.token;
    std::thread::spawn(move || {
        let result = forward_requests(&mut input, &mut to_daemon, &token, &requests);
        requests.finish(result);
        // Lets the daemon release this connection's subscriptions.
        let _ = to_daemon.shutdown(std::net::Shutdown::Write);
    });
    let responses = Arc::clone(&shared);
    std::thread::spawn(move || {
        let result = forward_responses(&mut from_daemon, &responses);
        responses.finish(result);
    });

    let mut state = shared.lock();
    let result = loop {
        if let Some(result) = state.end.take() {
            break result;
        }
        let idle_for = state.last_request.elapsed();
        if state.pending == 0 && idle_for >= idle_timeout {
            break Err(format!(
                "REMOTE_IDLE_TIMEOUT: no client request for {}s",
                idle_timeout.as_secs()
            ));
        }
        let wait = if state.pending == 0 {
            idle_timeout - idle_for
        } else {
            idle_timeout
        };
        state = shared
            .changed
            .wait_timeout(state, wait)
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .0;
    };
    drop(state);
    let _ = stream.shutdown(std::net::Shutdown::Write);
    result
}

/// Binary entry: integrator calls this instead of any Tauri initialization.
pub fn run(args: impl IntoIterator<Item = String>) -> Result<(), String> {
    let args: Vec<_> = args.into_iter().collect();
    // `agent-hook <agent> <event>` is run by an agent's hook on an SSH host, inside a pane that has
    // no helper root. It is the desktop's own reporter (`agent_hook_core.rs`), always exits 0 and
    // never blocks the agent, so it is answered before the root requirement below.
    if args.first().map(String::as_str) == Some(super::agent_hook_core::AGENT_HOOK_SUBCOMMAND) {
        super::agent_hook_core::run(args[1..].iter().map(std::ffi::OsString::from));
        return Ok(());
    }
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("{HELPER_VERSION}");
        return Ok(());
    }
    if args.iter().any(|arg| arg == "--capabilities") {
        println!("{}", capabilities_report());
        return Ok(());
    }
    let flag = |name: &str| {
        args.windows(2)
            .find(|pair| pair[0] == name)
            .map(|pair| pair[1].clone())
    };
    let root = flag("--root")
        .or_else(|| std::env::var("FERRYX_REMOTE_ROOT").ok())
        .map(PathBuf::from)
        .ok_or("REMOTE_RUNTIME_MISSING: configure FERRYX_REMOTE_ROOT or --root")?;
    match args.first().map(String::as_str) {
        Some("start") => start(root,flag("--host-id").ok_or("INVALID_REQUEST: --host-id required")?),
        Some("daemon") => daemon(root,flag("--host-id").ok_or("INVALID_REQUEST: --host-id required")?),
        Some("bridge") if args.iter().any(|arg|arg=="--stdio") => bridge(&root,std::io::stdin(),std::io::stdout()),
        _ => Err("Usage: ferryx-remote-helper start|daemon --root <private-root> --host-id <id> | bridge --stdio [--root <private-root>]".into()),
    }
}

#[cfg(test)]
#[path = "helper_service_tests.rs"]
mod helper_service_tests;
