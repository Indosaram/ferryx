//! Blocking portable stdio adapter. Call via spawn_blocking from async application code.
use super::helper::{read_frame, write_frame, Request, Runtime};
use serde_json::{json, Value};
use std::{io::{Read, Write}, path::{Path, PathBuf}, sync::Arc};

#[cfg(windows)]
mod process_windows;

#[derive(serde::Serialize, serde::Deserialize)]
struct Endpoint {
    address: String,
    token: String,
    #[serde(default)]
    pid: Option<u32>,
}

fn reply(runtime: &Runtime, value: Value) -> Value {
    match serde_json::from_value::<Request>(value).map_err(|e|e.to_string()).and_then(|r|runtime.handle(r)) {
        Ok(data) => json!({"ok":true,"data":data}), Err(error) => json!({"ok":false,"error":error}),
    }
}
fn serve(mut stream: impl Read + Write, runtime: Arc<Runtime>) -> Result<(),String> {
    while let Some(request) = read_frame(&mut stream)? { write_frame(&mut stream, &reply(&runtime,request))?; }
    Ok(()) // Bridge EOF releases only this connection; Runtime owns every PTY.
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
            return Err("FORBIDDEN: helper IPC must be owned by the current user and private".into());
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
    let path_str = path.to_str().ok_or_else(|| "FORBIDDEN: helper IPC path is invalid UTF-8".to_string())?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(path_str.as_bytes());
    let script = format!(
        r#"$ProgressPreference='SilentlyContinue';$ErrorActionPreference='Stop';$raw=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{encoded}'));if($raw.StartsWith('\\?\UNC\')){{$raw='\\'+$raw.Substring(8)}}elseif($raw.StartsWith('\\?\')){{$raw=$raw.Substring(4)}};$p=[System.IO.Path]::GetFullPath($raw);$item=Get-Item -LiteralPath $p -Force;$acl=$item.GetAccessControl();$u=[System.Security.Principal.WindowsIdentity]::GetCurrent();$allowed=@($u.User.Value,'S-1-5-18','S-1-5-32-544');$owner=try{{$acl.GetOwner([System.Security.Principal.SecurityIdentifier]).Value}}catch{{$null}};if(-not $owner){{try{{$owner=(New-Object System.Security.Principal.NTAccount($acl.Owner)).Translate([System.Security.Principal.SecurityIdentifier]).Value}}catch{{$owner=$acl.Owner}}}};if($allowed -notcontains $owner){{exit 1}};$rules=$acl.Access;if($null -eq $rules -or $rules.Count -eq 0){{exit 2}};$hasAllowed=$false;foreach($r in $rules){{if($r.AccessControlType.ToString() -eq 'Allow'){{$sid=try{{$r.IdentityReference.Translate([System.Security.Principal.SecurityIdentifier]).Value}}catch{{$r.IdentityReference.Value}};if($allowed -notcontains $sid){{exit 3}};$hasAllowed=$true}}}};if(-not $hasAllowed){{exit 4}};exit 0;"#
    );
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &script])
        .output()
        .map_err(|e| format!("FORBIDDEN: cannot execute PowerShell ACL validation: {e}"))?;

    if !output.status.success() {
        return Err("FORBIDDEN: helper IPC must be owned by the current user and private".into());
    }
    Ok(())
}

fn endpoint(root: &Path) -> Result<Endpoint,String> {
    if !validate_private(root)?.is_dir() {
        return Err("FORBIDDEN: helper root must be a private directory".into());
    }
    let path = root.join("endpoint.json");
    if !validate_private(&path)?.is_file() {
        return Err("FORBIDDEN: helper endpoint must be a regular file".into());
    }
    let value: Endpoint = serde_json::from_slice(&std::fs::read(&path)
        .map_err(|e| format!("REMOTE_RUNTIME_MISSING: {e}"))?).map_err(|e|e.to_string())?;
    #[cfg(unix)]
    if Path::new(&value.address) != root.canonicalize().map_err(|e| e.to_string())?.join("helper.sock") {
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
        #[cfg(unix)] {
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
    #[cfg(unix)] {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let lock = options.open(&lock_path).map_err(|e| e.to_string())?;
    super::private_file(&lock_path)?;
    lock.try_lock().map_err(|e| format!("REMOTE_RUNTIME_CONFLICT: {e}"))?;
    // The OS lock, not a failed connection probe, authorizes stale-file removal.
    let path = root.join("endpoint.json");
    if path.symlink_metadata().is_ok() {
        let previous = endpoint(&root)?;
        #[cfg(unix)]
        let live = std::os::unix::net::UnixStream::connect(&previous.address).is_ok();
        #[cfg(not(unix))]
        let live = {
            let address: std::net::SocketAddr = previous.address.parse()
                .map_err(|e: std::net::AddrParseError| e.to_string())?;
            if !address.ip().is_loopback() {
                return Err("FORBIDDEN: helper IPC must be loopback".into());
            }
            std::net::TcpStream::connect_timeout(&address, std::time::Duration::from_secs(1)).is_ok()
        };
        if live {
            return Err("REMOTE_RUNTIME_CONFLICT: existing helper is still listening".into());
        }
    }
    let token = format!("{}{}",uuid::Uuid::new_v4(),uuid::Uuid::new_v4());
    let runtime = Arc::new(Runtime::new(root.clone(),host,token.clone())?);
    #[cfg(unix)] let (listener, address) = {
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
        let listener = std::os::unix::net::UnixListener::bind(&socket).map_err(|e|e.to_string())?;
        super::private_file(&socket)?; (listener,socket.to_string_lossy().into_owned())
    };
    #[cfg(not(unix))] let (listener, address) = {
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST,0)).map_err(|e|e.to_string())?;
        let address = listener.local_addr().map_err(|e|e.to_string())?.to_string(); (listener,address)
    };
    let staged = root.join(format!("endpoint-{}.tmp", uuid::Uuid::new_v4()));
    let mut publish = std::fs::OpenOptions::new();
    publish.write(true).create_new(true);
    #[cfg(unix)] {
        use std::os::unix::fs::OpenOptionsExt;
        publish.mode(0o600);
    }
    let publication = (|| {
        let mut file = publish.open(&staged).map_err(|e| e.to_string())?;
        super::private_file(&staged)?;
        file.write_all(&serde_json::to_vec(&Endpoint { address, token, pid: Some(std::process::id()) }).map_err(|e|e.to_string())?)
            .and_then(|_|file.sync_all()).map_err(|e|e.to_string())?;
        drop(file);
        #[cfg(windows)]
        if path.exists() { std::fs::remove_file(&path).map_err(|e| e.to_string())?; }
        std::fs::rename(&staged, &path).map_err(|e| e.to_string())
    })();
    if publication.is_err() {
        let _ = std::fs::remove_file(&staged);
    }
    publication?;
    Ok(BoundRuntime { listener, runtime, _lock: lock })
}

pub fn is_live(root: &Path, expected_host: &str) -> bool {
    let Ok(ep) = endpoint(root) else { return false; };
    #[cfg(unix)]
    let mut stream = {
        use std::os::unix::fs::FileTypeExt;
        if !validate_private(Path::new(&ep.address)).map(|m| m.file_type().is_socket()).unwrap_or(false) {
            return false;
        }
        let Ok(s) = std::os::unix::net::UnixStream::connect(&ep.address) else { return false; };
        let _ = s.set_read_timeout(Some(std::time::Duration::from_millis(500)));
        let _ = s.set_write_timeout(Some(std::time::Duration::from_millis(500)));
        s
    };
    #[cfg(not(unix))]
    let mut stream = {
        let Ok(address) = ep.address.parse::<std::net::SocketAddr>() else { return false; };
        if !address.ip().is_loopback() { return false; }
        let Ok(s) = std::net::TcpStream::connect_timeout(&address, std::time::Duration::from_millis(500)) else { return false; };
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
    let proto_match = resp.get("data")
        .and_then(|d| d.get("protocol"))
        .and_then(Value::as_u64) == Some(1);
    let host_match = resp.get("data")
        .and_then(|d| d.get("hostId"))
        .and_then(Value::as_str) == Some(expected_host);
    is_ok && proto_match && host_match
}

pub fn daemon(root: PathBuf, host: String) -> Result<(),String> {
    let bound = bind_runtime(&root, host)?;
    println!("{}",json!({"event":"ready","protocol":1})); std::io::stdout().flush().map_err(|e|e.to_string())?;
    for incoming in bound.listener.incoming() { let stream = incoming.map_err(|e|e.to_string())?; let runtime = bound.runtime.clone();
        std::thread::spawn(move || { if let Err(error) = serve(stream,runtime) { eprintln!("SSH_HELPER_CONNECTION: {error}"); } });
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
        #[cfg(unix)] {
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
    return process_windows::start(&root, &host);

    #[cfg(not(windows))]
    {
    let current_exe = std::env::current_exe().map_err(|e| format!("REMOTE_RUNTIME_INVALID: {e}"))?;
    let mut command = std::process::Command::new(current_exe);
    let root_str = root.to_str().ok_or("REMOTE_RUNTIME_INVALID: root is not valid UTF-8")?;
    command.args(["daemon", "--root", root_str, "--host-id", &host]);
    command.stdin(std::process::Stdio::null());
    command.stdout(std::process::Stdio::null());

    let log_path = root.join(format!("startup-{}.tmp", uuid::Uuid::new_v4()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)] {
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
                Err(format!("REMOTE_RUNTIME_EXITED: helper daemon exited ({status}): {detail}"))
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

pub fn bridge(root: &Path, mut input: impl Read, mut output: impl Write) -> Result<(),String> {
    let endpoint = endpoint(root)?;
    #[cfg(unix)] let mut stream = {
        use std::os::unix::fs::FileTypeExt;
        if !validate_private(Path::new(&endpoint.address))?.file_type().is_socket() {
            return Err("FORBIDDEN: helper IPC must be a socket".into());
        }
        std::os::unix::net::UnixStream::connect(endpoint.address).map_err(|e|e.to_string())?
    };
    #[cfg(not(unix))] let mut stream = {
        let address: std::net::SocketAddr = endpoint.address.parse().map_err(|e: std::net::AddrParseError|e.to_string())?;
        if !address.ip().is_loopback() { return Err("FORBIDDEN: helper IPC must be loopback".into()); }
        std::net::TcpStream::connect(address).map_err(|e|e.to_string())?
    };
    while let Some(mut request) = read_frame(&mut input)? {
        let op = request.get("op").and_then(Value::as_str).unwrap_or("");
        if !matches!(op,"handshake"|"project.register"|"project.list"|"worktree.create"|"pty.spawn"|"pty.list"|"pty.describe"|"pty.read"|"pty.write"|"pty.resize"|"pty.stop") {
            write_frame(&mut output,&json!({"ok":false,"error":"UNSUPPORTED: operation not allowlisted"}))?; continue;
        }
        request["token"] = json!(endpoint.token);
        write_frame(&mut stream,&request)?;
        let response = read_frame(&mut stream)?.ok_or("REMOTE_UNAVAILABLE: daemon EOF")?;
        write_frame(&mut output,&response)?;
    }
    Ok(())
}

/// Binary entry: integrator calls this instead of any Tauri initialization.
pub fn run(args: impl IntoIterator<Item=String>) -> Result<(),String> {
    let args: Vec<_> = args.into_iter().collect();
    let flag = |name: &str| args.windows(2).find(|pair|pair[0]==name).map(|pair|pair[1].clone());
    let root = flag("--root").or_else(||std::env::var("FERRYX_REMOTE_ROOT").ok()).map(PathBuf::from).ok_or("REMOTE_RUNTIME_MISSING: configure FERRYX_REMOTE_ROOT or --root")?;
    match args.first().map(String::as_str) {
        Some("start") => start(root,flag("--host-id").ok_or("INVALID_REQUEST: --host-id required")?),
        Some("daemon") => daemon(root,flag("--host-id").ok_or("INVALID_REQUEST: --host-id required")?),
        Some("bridge") if args.iter().any(|arg|arg=="--stdio") => bridge(&root,std::io::stdin().lock(),std::io::stdout().lock()),
        _ => Err("Usage: ferryx-remote-helper start|daemon --root <private-root> --host-id <id> | bridge --stdio [--root <private-root>]".into()),
    }
}

#[cfg(test)]
#[path = "helper_service_tests.rs"]
mod helper_service_tests;
