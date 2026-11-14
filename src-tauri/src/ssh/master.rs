//! Daemon-owned SSH connection master for one-shot operation reuse.
//!
//! Ownership invariants the code alone does not convey:
//! - No ControlPersist: the master never self-closes, so only the daemon ends it.
//! - ClearAllForwardings=yes on the master: without it the master inherits the user's port
//!   forwards and either fails noisily or competes for their ports (measured defect).
//! - Crash cleanup is a liveness pipe, not a shutdown path: the supervisor is a separate process
//!   holding the read end, so a SIGKILLed daemon closes the write end, the supervisor sees EOF,
//!   and it terminates the master's process group. An in-daemon supervisor cannot do this, since
//!   it dies with the daemon.
//! - ControlMaster=no on every client: a client can never take ownership from the daemon.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

use super::{SshAuthMethod, SshHost};
use crate::ipc::{IpcError, IpcErrorCode};

const ESTABLISH_BUDGET: std::time::Duration = std::time::Duration::from_secs(8);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ConnectionKey {
    target: String,
    port: Option<u16>,
    identity_file: Option<String>,
    jump_host: Option<String>,
}

impl ConnectionKey {
    fn of(host: &SshHost) -> Self {
        Self {
            target: host.target(),
            port: host.port,
            identity_file: host.identity_file.clone(),
            jump_host: host.jump_host.clone(),
        }
    }
}

struct MasterEntry {
    socket: PathBuf,
    _liveness: tokio::process::ChildStdin,
}

static MASTERS: LazyLock<Mutex<HashMap<ConnectionKey, Arc<MasterEntry>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[cfg(not(windows))]
fn control_path(key: &ConnectionKey) -> PathBuf {
    use std::hash::{Hash as _, Hasher as _};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hasher);
    crate::daemon::server::get_runtime_dir()
        .join("ssh")
        .join(format!("{:016x}.sock", hasher.finish()))
}

#[cfg(windows)]
fn control_path(_key: &ConnectionKey) -> PathBuf {
    PathBuf::new()
}

#[cfg(not(windows))]
fn ensure_socket_dir(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn master_args(key: &ConnectionKey, socket: &Path) -> Vec<String> {
    let program = super::exec::resolve_ssh_program().unwrap_or_else(|_| "ssh".to_string());
    let mut args = vec![program, "-M".to_string(), "-N".to_string(), "-T".to_string()];
    for option in [
        "BatchMode=yes".to_string(),
        "StrictHostKeyChecking=yes".to_string(),
        "UpdateHostKeys=no".to_string(),
        "ConnectTimeout=3".to_string(),
        "ConnectionAttempts=1".to_string(),
        "ServerAliveInterval=15".to_string(),
        "ServerAliveCountMax=2".to_string(),
        "PermitLocalCommand=no".to_string(),
        "ClearAllForwardings=yes".to_string(),
        "ControlMaster=yes".to_string(),
        format!("ControlPath={}", socket.display()),
        "ControlPersist=no".to_string(),
    ] {
        args.push("-o".to_string());
        args.push(option);
    }
    if let Some(port) = key.port {
        args.push("-p".to_string());
        args.push(port.to_string());
    }
    if let Some(identity) = &key.identity_file {
        args.push("-i".to_string());
        args.push(identity.clone());
    }
    if let Some(jump) = &key.jump_host {
        args.push("-J".to_string());
        args.push(jump.clone());
    }
    args.push(key.target.clone());
    args
}

#[cfg(not(windows))]
fn supervisor_command(master: &[String]) -> std::process::Command {
    use std::os::unix::process::CommandExt as _;
    let quoted = master
        .iter()
        .map(|arg| format!("'{}'", arg.replace('\'', r#"'\''"#)))
        .collect::<Vec<_>>()
        .join(" ");
    // `$child` is signalled rather than `-$$` so the supervisor survives long enough to reap it;
    // the trailing group sweep then catches anything the master forked, without risking a zombie
    // from a pid that has already been reaped.
    let script = format!(
        "{quoted} </dev/null >/dev/null 2>&1 &\n\
         child=$!\n\
         cat >/dev/null\n\
         kill -TERM $child 2>/dev/null\n\
         wait $child 2>/dev/null\n\
         kill -KILL -- -$$ 2>/dev/null\n"
    );
    let mut command = std::process::Command::new("/bin/sh");
    command
        .arg("-c")
        .arg(script)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // SAFETY: `setsid` is async-signal-safe, takes no pointers, and returns a value instead of
    // unwinding, which is what `pre_exec` requires between fork and exec. Making the shell a
    // group leader is what lets it kill the master's whole group by targeting its own pid.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    command
}

async fn master_is_live(program: &str, socket: &Path, target: &str, port: Option<u16>) -> bool {
    let mut command = crate::util::no_window_tokio_command(program);
    command
        .arg("-O")
        .arg("check")
        .arg("-o")
        .arg(format!("ControlPath={}", socket.display()));
    if let Some(port) = port {
        command.arg("-p").arg(port.to_string());
    }
    command
        .arg(target)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    matches!(command.status().await, Ok(status) if status.success())
}

pub fn client_reuse_args(socket: &Path) -> Vec<String> {
    vec![
        "-o".to_string(),
        "ControlMaster=no".to_string(),
        "-o".to_string(),
        format!("ControlPath={}", socket.display()),
    ]
}

#[cfg(windows)]
pub async fn acquire(_host: &SshHost) -> Result<Option<PathBuf>, IpcError> {
    // Windows is gated off until the supervisor and control-socket path are ported; every caller
    // then uses cold SSH, exactly as before this module existed.
    Ok(None)
}

#[cfg(not(windows))]
pub async fn acquire(host: &SshHost) -> Result<Option<PathBuf>, IpcError> {
    if host.auth_method == SshAuthMethod::Password {
        return Ok(None);
    }
    let key = ConnectionKey::of(host);
    let socket = control_path(&key);
    if let Some(entry) = lookup(&key) {
        return Ok(Some(entry.socket.clone()));
    }
    static CREATE_LOCK: LazyLock<tokio::sync::Mutex<()>> = LazyLock::new(Default::default);
    let _guard = CREATE_LOCK.lock().await;
    if let Some(entry) = lookup(&key) {
        return Ok(Some(entry.socket.clone()));
    }
    let program = super::exec::resolve_ssh_program().unwrap_or_else(|_| "ssh".to_string());
    if socket.exists() && !master_is_live(&program, &socket, &key.target, key.port).await {
        let _ = std::fs::remove_file(&socket);
    }
    if let Err(error) = ensure_socket_dir(&socket) {
        return Err(IpcError::new(
            IpcErrorCode::IoError,
            format!("SSH master socket directory is unusable: {error}"),
        ));
    }

    let mut child = tokio::process::Command::from(supervisor_command(&master_args(&key, &socket)))
        .spawn()
        .map_err(|error| {
            IpcError::new(
                IpcErrorCode::IoError,
                format!("Failed to start SSH master supervisor: {error}"),
            )
        })?;
    let liveness = child
        .stdin
        .take()
        .ok_or_else(|| IpcError::internal("SSH master supervisor has no liveness pipe"))?;

    let deadline = tokio::time::Instant::now() + ESTABLISH_BUDGET;
    let mut established = false;
    while tokio::time::Instant::now() < deadline {
        if master_is_live(&program, &socket, &key.target, key.port).await {
            established = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    if !established {
        let _ = std::fs::remove_file(&socket);
        drop(liveness);
        let _ = child.kill().await;
        return Err(IpcError::new(
            IpcErrorCode::IoError,
            "SSH master did not become usable in time",
        ));
    }

    // `wait()` both reaps the supervisor and keeps this task as the only owner of its status;
    // dropping the child without waiting would leave a zombie for the daemon's whole lifetime.
    tokio::spawn(async move {
        let _ = child.wait().await;
    });

    let entry = Arc::new(MasterEntry {
        socket: socket.clone(),
        _liveness: liveness,
    });
    MASTERS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(key, entry);
    Ok(Some(socket))
}

fn lookup(key: &ConnectionKey) -> Option<Arc<MasterEntry>> {
    MASTERS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(key)
        .cloned()
}

#[cfg(not(windows))]
pub fn forget(host: &SshHost) {
    let removed = MASTERS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(&ConnectionKey::of(host));
    drop(removed);
}

#[cfg(not(windows))]
pub fn live_count() -> usize {
    MASTERS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .len()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn host(hostname: &str) -> SshHost {
        SshHost {
            id: "t".into(),
            label: "t".into(),
            hostname: hostname.into(),
            username: None,
            port: None,
            identity_file: None,
            jump_host: None,
            source: super::super::SshHostSource::Manual,
            auth_method: SshAuthMethod::Key,
            disabled: None,
        }
    }

    #[test]
    fn distinct_targets_get_distinct_sockets_and_one_target_is_stable() {
        let a = ConnectionKey::of(&host("a.example"));
        let b = ConnectionKey::of(&host("b.example"));
        assert_ne!(control_path(&a), control_path(&b));
        assert_eq!(
            control_path(&a),
            control_path(&ConnectionKey::of(&host("a.example")))
        );
    }

    #[test]
    fn master_arguments_never_enable_persistence_and_clear_forwardings() {
        let key = ConnectionKey::of(&host("a.example"));
        let args = master_args(&key, Path::new("/run/ferryx/x.sock"));
        assert!(args.iter().any(|a| a == "ControlMaster=yes"));
        assert!(args.iter().any(|a| a == "ControlPersist=no"));
        assert!(args.iter().any(|a| a == "ClearAllForwardings=yes"));
        assert!(args.iter().any(|a| a == "ControlPath=/run/ferryx/x.sock"));
        assert!(args.iter().any(|a| a == "-N"));
    }

    #[test]
    fn client_arguments_can_never_take_ownership() {
        let args = client_reuse_args(Path::new("/run/ferryx/x.sock"));
        assert!(args.iter().any(|a| a == "ControlMaster=no"));
        assert!(!args.iter().any(|a| a == "ControlMaster=yes"));
    }

    #[test]
    fn supervisor_kills_its_own_group_and_quotes_the_master_command() {
        let command = supervisor_command(&["ssh".into(), "-N".into(), "it's".into()]);
        let script = command.get_args().nth(1).unwrap().to_string_lossy().into_owned();
        assert!(script.contains("kill -TERM $child"));
        assert!(script.contains("kill -KILL -- -$$"));
        assert!(script.contains(r"'it'\''s'"));
    }
}
