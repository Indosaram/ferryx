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
//!
//! Windows keeps this module compiled and tested for the port, but every entry point above
//! `acquire` stays unreachable there, so its helpers are allowed to be unused on that target
//! rather than silently deleted.
#![cfg_attr(windows, allow(dead_code))]

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
    /// Operations currently holding the master. Idle expiry is gated on this reaching zero,
    /// never on elapsed time alone: an elapsed-time reaper could close a socket a live
    /// terminal session or a slow git probe is still using.
    active: std::sync::atomic::AtomicUsize,
}

impl MasterEntry {
    fn busy(&self) -> bool {
        self.active.load(std::sync::atomic::Ordering::SeqCst) > 0
    }

    fn socket(&self) -> &Path {
        &self.socket
    }
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

/// A borrowed master socket. The lease keeps the operation counted as active so the idle
/// reaper cannot close the master while a command is still running on it; dropping it is what
/// allows expiry, and only after every other holder has also dropped.
pub struct MasterLease {
    socket: PathBuf,
    key: ConnectionKey,
    entry: Arc<MasterEntry>,
}

impl MasterLease {
    pub fn socket(&self) -> &Path {
        &self.socket
    }
}

impl Drop for MasterLease {
    fn drop(&mut self) {
        let outstanding = self
            .entry
            .active
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst)
            .saturating_sub(1);
        // A lease can be dropped from a plain (non-runtime) thread, where spawning a reaper
        // would panic; the master then simply expires on the next acquire instead.
        if outstanding == 0 && tokio::runtime::Handle::try_current().is_ok() {
            reap_when_idle(self.key.clone(), Arc::clone(&self.entry), IDLE_GRACE);
        }
    }
}

/// How long a master may sit with no active operation before it is closed. Only the count of
/// active operations starts this clock; an operation that starts inside the grace window
/// cancels the expiry because the reaper re-checks before tearing anything down.
const IDLE_GRACE: std::time::Duration = std::time::Duration::from_secs(30);

fn reap_when_idle(key: ConnectionKey, entry: Arc<MasterEntry>, grace: std::time::Duration) {
    tokio::spawn(async move {
        tokio::time::sleep(grace).await;
        if entry.busy() {
            return;
        }
        let removed = {
            let mut masters = MASTERS
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            // Identity, not just the key: a master re-established in the meantime must survive.
            match masters.get(&key) {
                Some(current) if Arc::ptr_eq(current, &entry) => masters.remove(&key),
                _ => None,
            }
        };
        // Dropping the entry closes the supervisor's liveness pipe, whose EOF tears the whole
        // connection tree down. Nothing here sends a signal to a connection we do not own.
        drop(removed);
        let socket = entry.socket.clone();
        let _ = crate::ipc::run_blocking(move || {
            let _ = std::fs::remove_file(&socket);
            Ok::<(), IpcError>(())
        })
        .await;
    });
}

#[cfg(windows)]
pub async fn acquire(_host: &SshHost) -> Result<Option<MasterLease>, IpcError> {
    // Windows is gated off until the supervisor and control-socket path are ported; every caller
    // then uses cold SSH, exactly as before this module existed.
    Ok(None)
}

/// Socket-directory work stays off the runtime threads, per the crate's blocking-I/O rule.
async fn probe_socket(socket: &Path) -> bool {
    let socket = socket.to_path_buf();
    crate::ipc::run_blocking(move || Ok(socket.exists()))
        .await
        .unwrap_or(false)
}

async fn unlink_socket(socket: &Path) {
    let socket = socket.to_path_buf();
    let _ = crate::ipc::run_blocking(move || {
        let _ = std::fs::remove_file(&socket);
        Ok::<(), IpcError>(())
    })
    .await;
}

#[cfg(not(windows))]
pub async fn acquire(host: &SshHost) -> Result<Option<MasterLease>, IpcError> {
    if host.auth_method == SshAuthMethod::Password {
        return Ok(None);
    }
    let key = ConnectionKey::of(host);
    let socket = control_path(&key);
    if let Some(entry) = lookup(&key) {
        // A cached entry whose master has died (host rebooted, connection dropped) must never be
        // handed out: the client would fail with a socket nobody is listening on. Re-check and
        // fall through to establish a fresh one.
        let program = super::exec::resolve_ssh_program().unwrap_or_else(|_| "ssh".to_string());
        if master_is_live(&program, &entry.socket, &key.target, key.port).await {
            return Ok(Some(lease(key, entry)));
        }
        forget_entry(&key, &entry).await;
    }
    static CREATE_LOCK: LazyLock<tokio::sync::Mutex<()>> = LazyLock::new(Default::default);
    let _guard = CREATE_LOCK.lock().await;
    if let Some(entry) = lookup(&key) {
        return Ok(Some(lease(key, entry)));
    }
    let program = super::exec::resolve_ssh_program().unwrap_or_else(|_| "ssh".to_string());
    // A socket left behind by a daemon that died without reaping is unusable: `ssh -O check`
    // cannot answer for it, and a client that tried to use it would only fail. Unlinking it is
    // the prerequisite for a fresh master, so it happens before anything is spawned.
    let stale = probe_socket(&socket).await
        && !master_is_live(&program, &socket, &key.target, key.port).await;
    if stale {
        unlink_socket(&socket).await;
    }
    let dir_target = socket.clone();
    let dir_ready = crate::ipc::run_blocking(move || {
        ensure_socket_dir(&dir_target).map_err(|error| IpcError::new(
            IpcErrorCode::IoError,
            format!("SSH master socket directory is unusable: {error}"),
        ))
    })
    .await;
    dir_ready?;

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
        unlink_socket(&socket).await;
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
        active: std::sync::atomic::AtomicUsize::new(0),
    });
    let entry = {
        let mut masters = MASTERS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
        masters.insert(key.clone(), Arc::clone(&entry));
        entry
    };
    Ok(Some(lease(key, entry)))
}

fn lease(key: ConnectionKey, entry: Arc<MasterEntry>) -> MasterLease {
    entry
        .active
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    MasterLease {
        socket: entry.socket.clone(),
        key,
        entry,
    }
}

fn lookup(key: &ConnectionKey) -> Option<Arc<MasterEntry>> {
    MASTERS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(key)
        .cloned()
}

/// Removes one specific entry, and only if it is still the registered one: a master that was
/// re-established in the meantime must survive a stale holder's cleanup.
async fn forget_entry(key: &ConnectionKey, entry: &Arc<MasterEntry>) {
    let removed = {
        let mut masters = MASTERS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match masters.get(key) {
            Some(current) if Arc::ptr_eq(current, entry) => masters.remove(key),
            _ => None,
        }
    };
    if let Some(removed) = removed {
        drop(removed);
        unlink_socket(entry.socket()).await;
    }
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

static REUSED_TOTAL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static COLD_TOTAL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Records which path a one-shot operation actually took, so the reuse win is measured
/// rather than assumed, the way the plan requires.
///
/// Compiled on every platform because the call sites are shared: Windows never has a master
/// (acquire yields none there), so its counters honestly record cold spawns instead of the
/// symbol going missing and breaking that build.
pub fn note_path(operation: &str, reused: bool) {
    use std::sync::atomic::Ordering;
    let (counter, path) = if reused {
        (&REUSED_TOTAL, "master_reused")
    } else {
        (&COLD_TOTAL, "cold_spawn")
    };
    let count = counter.fetch_add(1, Ordering::Relaxed) + 1;
    tracing::debug!(operation, path, count, "ssh one-shot transport");
}

/// `(reused, cold)` totals since the daemon started, so a later run reads the win instead of
/// trusting that reuse happened.
pub fn path_totals() -> (usize, usize) {
    use std::sync::atomic::Ordering;
    (
        REUSED_TOTAL.load(Ordering::Relaxed),
        COLD_TOTAL.load(Ordering::Relaxed),
    )
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// The plan requires the reuse win to be measured rather than assumed, so the counters must
    /// actually move: a path that never increments would make every later reading a lie.
    #[test]
    fn path_counters_move_independently() {
        let (reused_before, cold_before) = path_totals();
        note_path("test_reused", true);
        let (reused_after, cold_after) = path_totals();
        assert_eq!(reused_after, reused_before + 1);
        assert_eq!(cold_after, cold_before);

        note_path("test_cold", false);
        let (reused_final, cold_final) = path_totals();
        assert_eq!(reused_final, reused_after);
        assert_eq!(cold_final, cold_after + 1);
    }

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
            shell: None,
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

    /// A lease is the only thing that counts as activity; the placeholder supervisor stands in
    /// for the `ssh -M` process so the counting logic runs without a network or a real host.
    async fn entry_for(key: &ConnectionKey) -> Arc<MasterEntry> {
        let mut child = tokio::process::Command::new("sh")
            .arg("-c")
            .arg("sleep 30")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .expect("placeholder supervisor spawns");
        let liveness = child.stdin.take().expect("placeholder supervisor has a stdin pipe");
        tokio::spawn(async move {
            let _ = child.wait().await;
        });
        Arc::new(MasterEntry {
            socket: control_path(key),
            _liveness: liveness,
            active: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    #[tokio::test]
    async fn a_held_lease_keeps_the_master_past_its_idle_grace() {
        let key = ConnectionKey::of(&host("held.example"));
        let entry = entry_for(&key).await;
        MASTERS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(key.clone(), Arc::clone(&entry));
        let held = lease(key.clone(), Arc::clone(&entry));
        reap_when_idle(key.clone(), Arc::clone(&entry), std::time::Duration::from_millis(20));
        tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        assert!(
            MASTERS
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .contains_key(&key),
            "an operation still holding the master must survive the grace window"
        );
        assert_eq!(entry.active.load(std::sync::atomic::Ordering::SeqCst), 1);
        drop(held);
        assert_eq!(entry.active.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn an_idle_master_is_reaped_and_its_socket_removed() {
        let key = ConnectionKey::of(&host("idle.example"));
        let entry = entry_for(&key).await;
        let socket = entry.socket.clone();
        if let Some(parent) = socket.parent() {
            std::fs::create_dir_all(parent).expect("socket dir is creatable");
        }
        std::fs::write(&socket, b"stale").expect("placeholder socket is writable");
        MASTERS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(key.clone(), Arc::clone(&entry));
        reap_when_idle(key.clone(), Arc::clone(&entry), std::time::Duration::from_millis(20));
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        assert!(
            !MASTERS
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .contains_key(&key),
            "an idle master must leave the table so the next call re-establishes it"
        );
        assert!(!socket.exists(), "the control socket must not outlive its master");
    }

    /// Password auth cannot be handed to a multiplexed master: there is no agent socket for the
    /// background connection to authenticate with, so a master would either prompt on a pty that
    /// does not exist or hang. The gate must answer "cold path" instead of failing the caller.
    #[tokio::test]
    async fn a_password_host_never_enters_the_multiplexed_path() {
        let mut password_host = host("password.example");
        password_host.auth_method = SshAuthMethod::Password;
        let key = ConnectionKey::of(&password_host);
        let acquired = acquire(&password_host)
            .await
            .expect("a password host is a cold path, not an error");
        assert!(
            acquired.is_none(),
            "no lease may be handed out for password auth"
        );
        assert!(
            !MASTERS
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .contains_key(&key),
            "a password host must not leave a table entry behind"
        );
    }
}
