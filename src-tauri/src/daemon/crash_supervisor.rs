//! Abnormal-exit supervision for the headless daemon: the restart decision, its capped backoff,
//! the restart log, and the keeper gate that keeps supervision from ever running keeper-less.
//!
//! The daemon owns every PTY master descriptor, so a plain restart after a SIGSEGV only automates
//! session loss. Restart is therefore not a policy knob here: it is **enabled only while a live PTY
//! keeper is registered** ([`KeeperPresence`]). The keeper and its UDS protocol belong to Phase 3;
//! this module treats that protocol as an external contract and talks to it only through
//! [`KeeperSocketProbe`], which proves readiness with a bounded, **non-mutating** `HELLO` /
//! `HELLO_ACK` exchange: framing, request correlation, protocol major, the keeper's reported pid and
//! the socket's peer credentials must all agree. A socket that merely accepts a connection, a
//! foreign process, an incompatible generation and a dead or silent keeper all fail closed, and the
//! probe never attaches, spawns or fences anything.
//!
//! What a restart means here:
//!
//! * exit 0, a requested upgrade and a handover decision are **intentional**: never restart. Only a
//!   signal death or a non-zero exit is abnormal ([`classify_exit`]).
//! * abnormal exits restart at most [`RestartPolicy::max_restarts`] times per
//!   [`RestartPolicy::window`], with exponential backoff capped at [`RestartPolicy::max_backoff`];
//!   past the limit the supervisor writes a banner and stops instead of crash-looping.
//!   [`RestartPolicy::for_runtime`] keeps those numbers production-exact and lets only an isolated
//!   runtime shorten the *base* backoff ([`QA_BACKOFF_ENV`]), so a harness can reach the restart
//!   limit inside a bounded wait without measuring a relaxed budget.
//! * supervisor ownership belongs to the OS job (`daemon::launchd`), so an app replacement cannot
//!   start a second supervisor: the job runs this loop, and the loop spawns the daemon.
//!
//! # Exactly one supervisor per runtime
//!
//! Ownership of the OS job is not by itself enough. Two processes can still reach this loop - a job
//! relaunched while the previous one is still winding down, a hand-started supervisor beside the
//! job, an app-replacement path that starts one - and two supervisors on one runtime means two
//! daemons racing for one instance lock and two restart budgets for one crash. The loop therefore
//! takes an exclusive, non-blocking claim on `<runtime>/daemon-supervisor.lock`
//! ([`SupervisorLock`]) **before** its first decision, and a process that loses that claim writes
//! `event=refusal code=supervisor_already_running`, starts nothing, and exits 0. A refusal here is
//! deliberately silent in the user's UI: a healthy supervisor already serves this runtime, so a
//! banner saying the daemon was not started would be false.
//!
//! # What a refused gate does
//!
//! The gate is evaluated before supervision starts. A runtime it refuses still gets **exactly one**
//! daemon (the login has to be served by something), through [`SupervisedHost::spawn_unattended`]
//! - a different trait method from the one the restart loop uses - and this process then stops
//!   without ever entering the loop. The recorded shape is `event=gate supervised_restart=off`,
//!   then `event=refusal ... decision=serve_once`, then `event=start supervised=off
//!   decision=serve_once`, then `event=stop reason=no_live_keeper`: one start, zero restarts, and
//!   no code path in the loop through which the unattended start could be repeated. That is the
//!   tradeoff the plan's Phase 2 text names explicitly, and it is the only shape in which a
//!   keeper-less install still gets a daemon at login: `KeepAlive { SuccessfulExit = false }` would
//!   not relaunch this job after the clean exit that follows, so a hard refusal here would leave
//!   the app with no daemon at all until the user launched it by hand.
//!
//! # Handover
//!
//! A handover predecessor retires through `handover::HandoverManager::retirement_action()`, a bare
//! `exit(0)` that writes no retirement marker, so its exit would otherwise be indistinguishable
//! from an idle clean exit. The durable evidence is the decision record the commit writes beside
//! the legacy socket: `legacy-<pid>-<disambiguator>.transaction`, holding `predecessorPid` and
//! `state: "retired"`. [`ProcessHost`] reads that record for the pid that just exited and reports
//! [`DeliberateExit::HandoverDecision`], so a committed handover is on record as a handover and not
//! merely as "exit 0", which is what keeps it out of the restart path even when it exits non-zero.
//!
//! An intentional exit ends the loop, exactly like the daemon's own `exit(0)`; the successor that
//! a deliberate upgrade or handover starts is then unparented, as it already was when the job ran
//! `--daemon` directly and `KeepAlive { SuccessfulExit = false }` left an exit 0 alone.
//!
//! Every decision is written to `<data>/logs/daemon-crash.log`, which is the surface the Phase 2
//! acceptance harness asserts on.

use std::collections::VecDeque;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Restart log file name inside the daemon log directory (`~/.ferryx/logs`).
pub const RESTART_LOG_NAME: &str = "daemon-crash.log";

/// Name of the single-supervisor claim inside the runtime directory. Held for this process's whole
/// lifetime by the one supervisor that owns this runtime, and by nothing else: the daemon has its
/// own instance lock (`server::DaemonLockFile`) and never takes this one.
pub const SUPERVISOR_LOCK_NAME: &str = "daemon-supervisor.lock";

/// The command line the OS job runs: the supervisor, which spawns `--daemon` itself. Keeping the
/// argument on the supervisor is what makes the job the single owner of supervision.
pub const SUPERVISOR_ARG: &str = "--daemon-crash-supervisor";

/// Restart budget inside one window: five restarts per ten minutes, then stop.
pub const MAX_RESTARTS_PER_WINDOW: usize = 5;
pub const RESTART_WINDOW: Duration = Duration::from_secs(10 * 60);

/// First backoff after an abnormal exit; doubles per restart up to [`MAX_RESTART_BACKOFF`].
pub const RESTART_BACKOFF: Duration = Duration::from_secs(1);
pub const MAX_RESTART_BACKOFF: Duration = Duration::from_secs(60);

/// Name of the keeper's socket inside the runtime directory (`/tmp/ferryx-<uid>/keeper.sock`,
/// Phase 3). The daemon and the keeper both derive it from the runtime directory, never from
/// `$TMPDIR`, so an isolated runtime never reaches the user's keeper.
pub const KEEPER_SOCKET_NAME: &str = "keeper.sock";

/// One readiness probe's whole budget: connect plus `HELLO`/`HELLO_ACK`. The supervisor asks before
/// every restart decision, so no part of the probe may block past this; a keeper that cannot answer
/// inside it is not a guarantee a restart may lean on.
pub const KEEPER_PROBE_TIMEOUT: Duration = Duration::from_millis(500);

/// Whether a live PTY keeper is registered for this runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeeperStatus {
    /// A keeper completed this runtime's handshake on the keeper socket.
    Live { pid: Option<u32> },
    /// No keeper is registered. `detail` says why, and is safe to put in a log line.
    Absent { detail: String },
}

impl KeeperStatus {
    pub fn is_live(&self) -> bool {
        matches!(self, Self::Live { .. })
    }

    fn detail(&self) -> &str {
        match self {
            Self::Live { .. } => "keeper live",
            Self::Absent { detail } => detail,
        }
    }

    /// The pid the keeper reported in its `HELLO_ACK`, already corroborated against the kernel's
    /// peer pid where the platform exposes one. It is never inferred from liveness.
    pub fn pid(&self) -> Option<u32> {
        match self {
            Self::Live { pid } => *pid,
            Self::Absent { .. } => None,
        }
    }
}

/// The keeper-presence abstraction. Phase 3 registers its keeper behind this trait; nothing in the
/// supervisor can restart the daemon without asking it first.
pub trait KeeperPresence {
    fn live_keeper(&self) -> KeeperStatus;

    /// Whether this runtime routes new sessions to that keeper. Liveness proves reachability, not
    /// ownership; the default `false` keeps a gate closed that cannot answer.
    fn owns_new_sessions(&self) -> bool {
        false
    }
}

/// The production probe: this runtime's keeper socket must exist, be owned by this user, belong to a
/// peer of this user, and answer a `HELLO` with a `HELLO_ACK` that agrees with the wire contract.
///
/// `HELLO` is the keeper's own handshake verb and the only one this probe sends
/// (`server::dispatch`, `Kind::Hello`): the keeper answers it without reading or writing a session,
/// so a probe can neither claim a session nor fence a live daemon's epoch. Every refusal is an
/// [`KeeperStatus::Absent`] with a log-safe reason.
pub struct KeeperSocketProbe {
    runtime_dir: PathBuf,
    #[cfg_attr(not(unix), allow(dead_code))]
    ownership_table: Option<PathBuf>,
}

impl KeeperSocketProbe {
    pub fn new(runtime_dir: impl Into<PathBuf>) -> Self {
        Self {
            runtime_dir: runtime_dir.into(),
            ownership_table: None,
        }
    }

    pub fn with_ownership_table(mut self, path: impl Into<PathBuf>) -> Self {
        self.ownership_table = Some(path.into());
        self
    }

    #[cfg(unix)]
    fn ownership_table_path(&self) -> Option<PathBuf> {
        self.ownership_table.clone().or_else(|| {
            crate::daemon::keeper_session::OwnershipStore::for_runtime()
                .ok()
                .map(|store| store.path().to_path_buf())
        })
    }

    /// The probe for the running process's runtime directory.
    pub fn for_runtime() -> Self {
        Self::new(crate::daemon::server::get_runtime_dir())
    }

    pub fn socket_path(&self) -> PathBuf {
        self.runtime_dir.join(KEEPER_SOCKET_NAME)
    }

    /// Presence of the keeper socket. Only [`KeeperStatus`] escapes, never an error, because the
    /// only decision a caller makes is "may supervised restart be enabled".
    #[cfg(unix)]
    fn probe_socket(&self) -> KeeperStatus {
        use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _};

        let path = self.socket_path();
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return KeeperStatus::Absent {
                    detail: format!("no keeper socket at {}", path.display()),
                }
            }
            Err(error) => {
                return KeeperStatus::Absent {
                    detail: format!("keeper socket {} is unreadable: {error}", path.display()),
                }
            }
        };
        if !metadata.file_type().is_socket() {
            return KeeperStatus::Absent {
                detail: format!("{} is not a socket", path.display()),
            };
        }
        // SAFETY: `libc::getuid` takes no arguments, dereferences no pointers and has no side
        // effects; it is the current process's real uid.
        let own_uid = unsafe { libc::getuid() };
        if metadata.uid() != own_uid {
            return KeeperStatus::Absent {
                detail: format!(
                    "keeper socket {} is owned by uid {}, not {}",
                    path.display(),
                    metadata.uid(),
                    own_uid
                ),
            };
        }
        let deadline = Instant::now() + KEEPER_PROBE_TIMEOUT;
        let mut stream = match connect_bounded(&path, deadline) {
            Ok(stream) => stream,
            Err(detail) => return KeeperStatus::Absent { detail },
        };
        match verify_keeper(&path, &mut stream, deadline, own_uid) {
            Ok(pid) => KeeperStatus::Live { pid: Some(pid) },
            Err(detail) => KeeperStatus::Absent { detail },
        }
    }

    /// Off unix there is no keeper (Phase 3 introduces it on macOS first). Reporting `Absent` is
    /// what keeps supervised restart off on every other target by construction.
    #[cfg(not(unix))]
    fn probe_socket(&self) -> KeeperStatus {
        KeeperStatus::Absent {
            detail: "no PTY keeper exists on this target".to_string(),
        }
    }
}

impl KeeperPresence for KeeperSocketProbe {
    fn live_keeper(&self) -> KeeperStatus {
        self.probe_socket()
    }

    /// Read from the durable table, never from the handshake: answering `HELLO` says nothing about
    /// holding sessions. A broken table reads as `false`, so it can only close the gate.
    #[cfg(unix)]
    fn owns_new_sessions(&self) -> bool {
        let Some(path) = self.ownership_table_path() else {
            return false;
        };
        crate::daemon::keeper_session::OwnershipStore::at(path)
            .load()
            .is_ok_and(|table| {
                table.new_session_owner() == crate::daemon::keeper_session::SessionOwner::Keeper
            })
    }
}

#[cfg(unix)]
fn connect_bounded(
    path: &Path,
    deadline: Instant,
) -> Result<std::os::unix::net::UnixStream, String> {
    use std::os::fd::{AsRawFd as _, FromRawFd as _, IntoRawFd as _, OwnedFd};
    use std::os::unix::ffi::OsStrExt as _;
    use std::os::unix::net::UnixStream;

    let bytes = path.as_os_str().as_bytes();
    let offset = std::mem::offset_of!(libc::sockaddr_un, sun_path);
    if bytes.len() + 1 > std::mem::size_of::<libc::sockaddr_un>() - offset {
        return Err(format!("keeper socket path {} is too long", path.display()));
    }
    let length = (offset + bytes.len() + 1) as libc::socklen_t;
    // SAFETY: a zeroed `sockaddr_un` is a valid value; `sun_family`/`sun_len` and the path bytes
    // inside `length` are written before any call reads it.
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    // BSD and Darwin carry the address length in `sun_len`, and a connect fails without it.
    #[cfg(any(
        target_vendor = "apple",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "dragonfly",
    ))]
    {
        address.sun_len = length as u8;
    }
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (slot, byte) in address.sun_path.iter_mut().zip(bytes) {
        *slot = *byte as libc::c_char;
    }

    // SAFETY: the returned descriptor is checked for failure and owned from then on, so the error
    // paths below cannot leak it.
    let fd = unsafe {
        let raw = libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0);
        if raw < 0 {
            return Err(format!(
                "keeper socket {} could not be opened: {}",
                path.display(),
                std::io::Error::last_os_error()
            ));
        }
        OwnedFd::from_raw_fd(raw)
    };
    set_nonblocking(fd.as_raw_fd(), true)?;
    // SAFETY: `address` holds `length` initialised bytes and `fd` is an open socket owned here.
    let connected = unsafe {
        libc::connect(
            fd.as_raw_fd(),
            (&address as *const libc::sockaddr_un).cast(),
            length,
        )
    };
    if connected != 0 {
        let error = std::io::Error::last_os_error();
        let pending = matches!(error.raw_os_error(), Some(code) if code == libc::EINPROGRESS)
            || error.kind() == std::io::ErrorKind::WouldBlock;
        if !pending {
            return Err(format!(
                "keeper socket {} refused a connection: {error}",
                path.display()
            ));
        }
        wait_writable(fd.as_raw_fd(), deadline, path)?;
    }
    set_nonblocking(fd.as_raw_fd(), false)?;
    // No `SO_RCVTIMEO`/`SO_SNDTIMEO` is armed on this stream, on purpose: Darwin refuses both with
    // `EINVAL` once the peer has closed, even though the bytes that peer already sent are still
    // buffered and readable. The probe's deadline is enforced by [`wait_until_ready`] before every
    // read and write instead, which behaves identically on every unix and never depends on socket
    // options a hung-up peer can make unsettable.
    // SAFETY: `fd` is an open, connected socket owned here and released to the stream.
    Ok(unsafe { UnixStream::from_raw_fd(fd.into_raw_fd()) })
}

/// Waits until `events` is ready on `fd`, or the probe's deadline passes.
///
/// `Ok(true)` means the descriptor is ready, `Ok(false)` that the budget ran out first. This is the
/// only wait the probe performs, so no socket timeout option is involved at all. Exhaustion answers
/// `Ok(false)` rather than an error so each caller names the step it was waiting on.
#[cfg(unix)]
fn wait_until_ready(
    fd: std::os::fd::RawFd,
    events: libc::c_short,
    deadline: Instant,
) -> Result<bool, String> {
    let mut descriptor = libc::pollfd {
        fd,
        events,
        revents: 0,
    };
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Ok(false);
        }
        let remaining = deadline.saturating_duration_since(now);
        let timeout = i32::try_from(remaining.as_millis()).unwrap_or(i32::MAX);
        // SAFETY: `descriptor` is one valid pollfd and `timeout` is a bounded millisecond count.
        let ready = unsafe { libc::poll(&mut descriptor, 1, timeout) };
        if ready > 0 {
            return Ok(true);
        }
        if ready == 0 {
            return Ok(false);
        }
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::Interrupted {
            return Err(format!("keeper socket wait failed: {error}"));
        }
    }
}

#[cfg(unix)]
fn wait_writable(fd: std::os::fd::RawFd, deadline: Instant, path: &Path) -> Result<(), String> {
    let mut descriptor = libc::pollfd {
        fd,
        events: libc::POLLOUT,
        revents: 0,
    };
    loop {
        let remaining = remaining_budget(deadline)
            .map_err(|detail| format!("keeper socket {} {detail}", path.display()))?;
        let timeout = i32::try_from(remaining.as_millis()).unwrap_or(i32::MAX);
        // SAFETY: `descriptor` is one valid pollfd and `timeout` is a bounded millisecond count.
        let ready = unsafe { libc::poll(&mut descriptor, 1, timeout) };
        if ready > 0 {
            if descriptor.revents & libc::POLLNVAL != 0 {
                return Err(format!(
                    "keeper socket {} connect descriptor is invalid",
                    path.display()
                ));
            }
            // A completed non-blocking connect reports its verdict through SO_ERROR alone.
            let mut so_error: libc::c_int = 0;
            let mut length = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
            // SAFETY: `so_error` is a writable c_int of the declared length.
            let read = unsafe {
                libc::getsockopt(
                    fd,
                    libc::SOL_SOCKET,
                    libc::SO_ERROR,
                    (&mut so_error as *mut libc::c_int).cast(),
                    &mut length,
                )
            };
            if read != 0 {
                return Err(format!(
                    "keeper socket {} connect state is unreadable: {}",
                    path.display(),
                    std::io::Error::last_os_error()
                ));
            }
            if so_error != 0 {
                return Err(format!(
                    "keeper socket {} refused a connection: {}",
                    path.display(),
                    std::io::Error::from_raw_os_error(so_error)
                ));
            }
            if descriptor.revents & (libc::POLLERR | libc::POLLHUP) != 0 {
                return Err(format!(
                    "keeper socket {} did not accept the connection",
                    path.display()
                ));
            }
            return Ok(());
        }
        if ready == 0 {
            return Err(format!(
                "keeper socket {} did not accept the connection within {KEEPER_PROBE_TIMEOUT:?}",
                path.display()
            ));
        }
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::Interrupted {
            return Err(format!(
                "keeper socket {} connect wait failed: {error}",
                path.display()
            ));
        }
    }
}

#[cfg(unix)]
fn set_nonblocking(fd: std::os::fd::RawFd, nonblocking: bool) -> Result<(), String> {
    // SAFETY: `fcntl` only reads flag bits from an open descriptor owned by the caller.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(format!(
            "keeper socket flags are unreadable: {}",
            std::io::Error::last_os_error()
        ));
    }
    let flags = if nonblocking {
        flags | libc::O_NONBLOCK
    } else {
        flags & !libc::O_NONBLOCK
    };
    // SAFETY: the same open descriptor, with a flag word read a moment ago.
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags) } < 0 {
        return Err(format!(
            "keeper socket flags could not be set: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn remaining_budget(deadline: Instant) -> Result<Duration, String> {
    let now = Instant::now();
    if now >= deadline {
        return Err(format!(
            "did not complete the keeper handshake within {KEEPER_PROBE_TIMEOUT:?}"
        ));
    }
    // `poll` reads a zero millisecond timeout as "do not wait", so the remainder is floored at 1 ms.
    Ok(deadline
        .saturating_duration_since(now)
        .max(Duration::from_millis(1)))
}

#[cfg(unix)]
fn read_frame_bounded(
    stream: &mut std::os::unix::net::UnixStream,
    deadline: Instant,
) -> Result<crate::daemon::keeper_client::KeeperFrame, String> {
    use crate::daemon::keeper_client::{
        KeeperFrame, KEEPER_HEADER_BYTES, KEEPER_MAX_PAYLOAD_BYTES,
    };

    let mut header = [0u8; KEEPER_HEADER_BYTES];
    read_exact_bounded(stream, &mut header, deadline, "header")?;
    let payload_len = u32::from_be_bytes([header[8], header[9], header[10], header[11]]) as usize;
    if payload_len > KEEPER_MAX_PAYLOAD_BYTES as usize {
        return Err(format!("keeper announced {payload_len} payload bytes"));
    }
    let mut bytes = vec![0u8; KEEPER_HEADER_BYTES + payload_len];
    bytes[..KEEPER_HEADER_BYTES].copy_from_slice(&header);
    read_exact_bounded(
        stream,
        &mut bytes[KEEPER_HEADER_BYTES..],
        deadline,
        "payload",
    )?;
    KeeperFrame::decode(&bytes).map_err(|error| error.to_string())
}

#[cfg(unix)]
fn read_exact_bounded(
    stream: &mut std::os::unix::net::UnixStream,
    buffer: &mut [u8],
    deadline: Instant,
    what: &str,
) -> Result<(), String> {
    use std::io::Read as _;
    use std::os::fd::AsRawFd as _;

    let fd = stream.as_raw_fd();
    let mut filled = 0;
    while filled < buffer.len() {
        // Readiness, not `SO_RCVTIMEO`: Darwin rejects a receive timeout on an AF_UNIX socket whose
        // peer has closed (`EINVAL`) while the peer's bytes are still buffered, so arming one would
        // turn a keeper that answered and hung up into an unreadable one. `poll` gives the same
        // bound on every unix, and a closed peer is simply readable and then reads EOF.
        if !wait_until_ready(fd, libc::POLLIN, deadline)? {
            return Err(format!(
                "keeper did not send its {what} within {KEEPER_PROBE_TIMEOUT:?}"
            ));
        }
        match stream.read(&mut buffer[filled..]) {
            Ok(0) => return Err(format!("keeper closed the connection before its {what}")),
            Ok(count) => filled += count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(format!("keeper {what} read failed: {error}")),
        }
    }
    Ok(())
}

/// Writes the whole handshake frame, bounded only by the probe's deadline.
///
/// `SO_SNDTIMEO` is unusable here for the same Darwin reason as the read side, so each write waits
/// for writability with `poll` first. A peer that has gone away reports `POLLHUP` and the write
/// fails, which is the "closed before the handshake" case - not a timeout that could not be armed.
#[cfg(unix)]
fn write_all_bounded(
    stream: &mut std::os::unix::net::UnixStream,
    bytes: &[u8],
    deadline: Instant,
    path: &Path,
) -> Result<(), String> {
    use std::io::Write as _;
    use std::os::fd::AsRawFd as _;

    let fd = stream.as_raw_fd();
    let mut written = 0;
    while written < bytes.len() {
        if !wait_until_ready(fd, libc::POLLOUT, deadline)? {
            return Err(format!(
                "keeper socket {} did not accept the handshake within {KEEPER_PROBE_TIMEOUT:?}",
                path.display()
            ));
        }
        match stream.write(&bytes[written..]) {
            Ok(0) => {
                return Err(format!(
                    "keeper socket {} closed before the handshake",
                    path.display()
                ))
            }
            Ok(count) => written += count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => {
                return Err(format!(
                    "keeper socket {} closed before the handshake: {error}",
                    path.display()
                ))
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn verify_keeper(
    path: &Path,
    stream: &mut std::os::unix::net::UnixStream,
    deadline: Instant,
    own_uid: u32,
) -> Result<u32, String> {
    use std::os::fd::AsRawFd as _;

    use crate::daemon::keeper_client::{KeeperFrame, KeeperKind, KEEPER_PROTOCOL_MAJOR};

    // The socket file's owner was checked by the caller; this is the process behind it, and it is
    // the last point at which a foreign listener can be told apart from this user's keeper.
    let credentials = crate::daemon::handover_wire::peer_credentials(stream.as_raw_fd())
        .map_err(|error| {
            format!(
                "keeper socket {} did not reveal its peer credentials: {error}",
                path.display()
            )
        })?;
    if credentials.uid != own_uid {
        return Err(format!(
            "keeper socket {} is served by uid {}, not {}",
            path.display(),
            credentials.uid,
            own_uid
        ));
    }
    let kernel_pid = crate::daemon::supervisor_peer::peer_pid(stream.as_raw_fd());

    let request_id = uuid::Uuid::new_v4().to_string();
    let hello = KeeperFrame::new(
        KeeperKind::Hello,
        request_id.clone(),
        0,
        serde_json::json!({
            "protocolMajor": KEEPER_PROTOCOL_MAJOR,
            "protocolMinor": 0,
            "keeperVersion": env!("CARGO_PKG_VERSION"),
        }),
    );
    write_all_bounded(stream, &hello.encode(), deadline, path)?;

    let reply = read_frame_bounded(stream, deadline)?;
    if reply.kind == KeeperKind::Error {
        return Err(format!(
            "keeper refused the handshake: {}",
            reply
                .payload
                .get("message")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("no reason given")
        ));
    }
    if reply.kind != KeeperKind::HelloAck {
        return Err(format!(
            "keeper answered {:?} instead of the handshake",
            reply.kind
        ));
    }
    if reply.request_id != request_id || reply.frame_sequence != hello.frame_sequence {
        return Err(format!(
            "keeper answered request {:?} sequence {} while {:?}:{} was outstanding",
            reply.request_id, reply.frame_sequence, request_id, hello.frame_sequence
        ));
    }
    let major = reply
        .payload
        .get("protocolMajor")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| "keeper handshake carries no protocolMajor".to_string())?;
    if major != u64::from(KEEPER_PROTOCOL_MAJOR) {
        return Err(format!(
            "keeper speaks protocol major {major}, this daemon speaks {KEEPER_PROTOCOL_MAJOR}"
        ));
    }
    // A newer minor stays compatible under the additive-only rule, so it only has to be present.
    if reply.payload.get("protocolMinor").and_then(serde_json::Value::as_u64).is_none() {
        return Err("keeper handshake carries no protocolMinor".to_string());
    }
    if reply
        .payload
        .get("keeperVersion")
        .and_then(serde_json::Value::as_str)
        .is_none_or(str::is_empty)
    {
        return Err("keeper handshake carries no keeperVersion".to_string());
    }
    let pid = reply
        .payload
        .get("pid")
        .and_then(serde_json::Value::as_u64)
        .and_then(|pid| u32::try_from(pid).ok())
        .filter(|pid| *pid > 0)
        .ok_or_else(|| "keeper handshake carries no usable pid".to_string())?;
    if let Some(observed) = kernel_pid {
        if observed != pid {
            return Err(format!(
                "keeper claimed pid {pid} but the kernel reports {observed} on {}",
                path.display()
            ));
        }
    }
    Ok(pid)
}

/// A keeper presence that is never live. A build with no keeper support reports this, so
/// supervised restart stays off there without any caller having to remember to check.
pub struct NoKeeper;

impl KeeperPresence for NoKeeper {
    fn live_keeper(&self) -> KeeperStatus {
        KeeperStatus::Absent {
            detail: "no PTY keeper is registered".to_string(),
        }
    }
}

/// Why supervised restart may not be enabled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestartRefusal {
    NoLiveKeeper { detail: String },
    NoKeeperOwnership { detail: String },
}

impl RestartRefusal {
    /// Why the gate closed, in a log-safe phrase: the absence the probe reported, or the ownership
    /// fact that closed the gate.
    pub fn detail(&self) -> &str {
        match self {
            Self::NoLiveKeeper { detail } | Self::NoKeeperOwnership { detail } => detail,
        }
    }

    /// The line the UI surfaces. Supervision never starts a restart after writing this.
    pub fn banner(&self) -> String {
        match self {
            Self::NoLiveKeeper { detail } => format!(
                "Automatic daemon restart is off: no live PTY keeper is registered ({detail}). \
                 Sessions would not survive a restart, so the daemon is not restarted automatically."
            ),
            Self::NoKeeperOwnership { detail } => format!(
                "Automatic daemon restart is off: the PTY keeper is live but does not own new \
                 sessions ({detail}). Sessions would not survive a restart, so the daemon is not \
                 restarted automatically."
            ),
        }
    }

    /// The `reason=` token the restart log carries.
    pub fn token(&self) -> &'static str {
        match self {
            Self::NoLiveKeeper { .. } => "no_live_keeper",
            Self::NoKeeperOwnership { .. } => "no_keeper_ownership",
        }
    }

    /// The [`StopReason`] this refusal stops supervision with.
    pub fn stop_reason(&self) -> StopReason {
        match self {
            Self::NoLiveKeeper { .. } => StopReason::NoLiveKeeper,
            Self::NoKeeperOwnership { .. } => StopReason::NoKeeperOwnership,
        }
    }
}

/// The hard rule from the release plan: supervised restart is enabled only when a live keeper also
/// owns the sessions, so the plan's "keeper live, everything still direct-owned" hazard state can
/// never be answered by a restart.
pub fn supervised_restart_gate(keeper: &dyn KeeperPresence) -> Result<KeeperStatus, RestartRefusal> {
    let status = keeper.live_keeper();
    if !status.is_live() {
        return Err(RestartRefusal::NoLiveKeeper {
            detail: status.detail().to_string(),
        });
    }
    if !keeper.owns_new_sessions() {
        let live = status.detail();
        return Err(RestartRefusal::NoKeeperOwnership {
            detail: format!("keeper {live}, but new sessions are routed to the daemon"),
        });
    }
    Ok(status)
}

/// Why this process may not supervise this runtime.
///
/// Typed, because the two causes need different answers: another supervisor already owns the
/// runtime (say nothing to the user; a healthy daemon is being supervised), or the claim itself
/// could not be taken (the runtime directory is unusable, which the caller must surface).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorClaimError {
    /// Another process holds [`SUPERVISOR_LOCK_NAME`] in this runtime directory.
    AlreadyRunning { path: PathBuf },
    /// The claim could not be created, opened or secured, and says why.
    Unavailable { detail: String },
}

impl SupervisorClaimError {
    /// The `code=` token the restart log carries.
    pub fn token(&self) -> &'static str {
        match self {
            Self::AlreadyRunning { .. } => "supervisor_already_running",
            Self::Unavailable { .. } => "supervisor_unavailable",
        }
    }

    /// A log-safe phrase naming the cause. Never contains a newline.
    pub fn detail(&self) -> String {
        match self {
            Self::AlreadyRunning { path } => format!(
                "another supervisor holds {}; this process starts no daemon and supervises nothing",
                path.display()
            ),
            Self::Unavailable { detail } => detail.clone(),
        }
    }

    /// The [`StopReason`] a refused claim stops with.
    pub fn stop_reason(&self) -> StopReason {
        match self {
            Self::AlreadyRunning { .. } => StopReason::AlreadySupervised,
            Self::Unavailable { detail } => StopReason::SpawnFailed(detail.clone()),
        }
    }
}

/// The single-supervisor claim on one runtime directory: an exclusive, non-blocking lock on
/// `<runtime>/daemon-supervisor.lock`, held for this process's whole life.
///
/// `launchd` ownership of the job is not by itself exclusivity. A job relaunched while its previous
/// instance is still winding down, a supervisor started by hand beside the job, and an app
/// replacement that starts one all reach [`run_supervision`], and two supervisors on one runtime
/// means two daemons racing for the instance lock and two independent restart budgets for one
/// crash. The lock is therefore taken **before the first decision**, and it is the OS that decides:
/// `flock` is released by the kernel when the descriptor closes for any reason, so a crashed
/// supervisor never leaves the runtime un-supervisable.
///
/// The file itself is opened through the daemon's own secure-lock helper, so it is created `0600`,
/// refuses a symlink and refuses a file owned by another user - the same rules as the daemon's
/// instance lock. Nothing here deletes it: an empty `0600` file is the lock's whole state.
#[derive(Debug)]
pub struct SupervisorLock {
    #[cfg(unix)]
    _lock: crate::daemon::server::DaemonLockFile,
    path: PathBuf,
}

impl SupervisorLock {
    /// Takes the claim for `runtime_dir`, or reports who else holds it.
    ///
    /// `runtime_dir` is created and re-verified `0700` first, because the lock file is only as
    /// trustworthy as the directory it lives in.
    #[cfg(unix)]
    pub fn acquire(runtime_dir: &Path) -> Result<Self, SupervisorClaimError> {
        crate::daemon::server::ensure_runtime_directory(runtime_dir).map_err(|detail| {
            SupervisorClaimError::Unavailable { detail }
        })?;
        let path = runtime_dir.join(SUPERVISOR_LOCK_NAME);
        let file = crate::daemon::server::open_secure_lock_file(&path)
            .map_err(|detail| SupervisorClaimError::Unavailable { detail })?;
        // The file is already open and secured, so the only way `try_lock` fails is that another
        // live descriptor holds the exclusive lock. That is the answer this claim exists to give.
        let lock = crate::daemon::server::DaemonLockFile::try_lock(file)
            .map_err(|_| SupervisorClaimError::AlreadyRunning { path: path.clone() })?;
        Ok(Self {
            _lock: lock,
            path,
        })
    }

    /// Non-unix fallback: this target has no `flock`, and it has no crash supervisor either.
    ///
    /// `cli::run_daemon_crash_supervisor` refuses [`SUPERVISOR_ARG`] outside unix, and the Windows
    /// watchdog (`daemon::supervisor_windows`) owns supervision there with its own single-owner
    /// rules. The claim therefore only has to succeed without changing behaviour, so this creates
    /// the runtime directory exactly as the unix path does and takes no lock.
    #[cfg(not(unix))]
    pub fn acquire(runtime_dir: &Path) -> Result<Self, SupervisorClaimError> {
        crate::daemon::server::ensure_runtime_directory(runtime_dir).map_err(|detail| {
            SupervisorClaimError::Unavailable { detail }
        })?;
        Ok(Self {
            path: runtime_dir.join(SUPERVISOR_LOCK_NAME),
        })
    }

    /// The lock file this process holds.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Restart budget and backoff. Injectable so the harness runs without wall-clock waits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartPolicy {
    pub max_restarts: usize,
    pub window: Duration,
    pub base_backoff: Duration,
    pub max_backoff: Duration,
}

impl Default for RestartPolicy {
    fn default() -> Self {
        Self {
            max_restarts: MAX_RESTARTS_PER_WINDOW,
            window: RESTART_WINDOW,
            base_backoff: RESTART_BACKOFF,
            max_backoff: MAX_RESTART_BACKOFF,
        }
    }
}

impl RestartPolicy {
    /// Backoff before restart number `attempt` (1-based): `base * 2^(attempt-1)`, capped.
    pub fn backoff(&self, attempt: usize) -> Duration {
        let doublings = attempt.saturating_sub(1).min(16) as u32;
        self.base_backoff
            .saturating_mul(1u32 << doublings)
            .min(self.max_backoff)
    }

    /// The policy this process supervises under: always [`RestartPolicy::default`] in production.
    ///
    /// Only an isolated runtime that also sets [`QA_BACKOFF_ENV`] may shorten `base_backoff`, and
    /// only `base_backoff`; the budget, the window and the cap stay production-exact so a harness
    /// measures the real limit instead of a relaxed one.
    pub fn for_runtime() -> Self {
        Self::resolve(&|key| std::env::var_os(key))
    }

    fn resolve(env: &dyn Fn(&str) -> Option<std::ffi::OsString>) -> Self {
        let policy = Self::default();
        let isolated = crate::daemon::supervisor::ISOLATED_RUNTIME_ENV
            .iter()
            .any(|key| env(key).is_some_and(|value| !value.is_empty()));
        if !isolated {
            return policy;
        }
        let Some(ms) = env(QA_BACKOFF_ENV)
            .and_then(|value| value.to_string_lossy().trim().parse::<u64>().ok())
            .filter(|ms| *ms > 0)
        else {
            return policy;
        };
        Self {
            base_backoff: Duration::from_millis(ms),
            ..policy
        }
    }
}

/// QA-only base-backoff override, ignored outside an isolated runtime ([`RestartPolicy::for_runtime`]).
pub const QA_BACKOFF_ENV: &str = "FERRYX_DAEMON_RESTART_BACKOFF_MS";

/// The intentional-exit signals the daemon records before it leaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliberateExit {
    /// Nothing recorded; the exit is judged on its status alone.
    None,
    /// The daemon exited 0 (duplicate-endpoint startup, idle retirement, ...).
    CleanExit,
    /// A binary upgrade was requested and the daemon handed over.
    RequestedUpgrade,
    /// A handover decision committed and the predecessor retired on purpose.
    HandoverDecision,
}

impl DeliberateExit {
    /// The `kind=` token the restart log carries.
    pub fn kind(self) -> &'static str {
        match self {
            Self::None => "unrecorded",
            Self::CleanExit => "clean_exit",
            Self::RequestedUpgrade => "requested_upgrade",
            Self::HandoverDecision => "handover",
        }
    }
}

/// A process exit as the OS reported it. `signal` is set only for a signal death.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExitObservation {
    pub code: Option<i32>,
    pub signal: Option<i32>,
}

impl ExitObservation {
    pub fn code(code: i32) -> Self {
        Self {
            code: Some(code),
            signal: None,
        }
    }

    pub fn signaled(signal: i32) -> Self {
        Self {
            code: None,
            signal: Some(signal),
        }
    }

    fn describe(&self) -> String {
        match (self.code, self.signal) {
            (Some(code), _) => format!("code={code}"),
            (None, Some(signal)) => format!("signal={signal}"),
            (None, None) => "code=unknown".to_string(),
        }
    }
}

/// Whether an exit restarts the daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitKind {
    Intentional(DeliberateExit),
    Abnormal,
}

/// Only signal deaths and non-zero exits are abnormal; a recorded deliberate exit wins over the
/// status, and exit 0 is never abnormal.
pub fn classify_exit(observed: ExitObservation, deliberate: DeliberateExit) -> ExitKind {
    if deliberate != DeliberateExit::None {
        return ExitKind::Intentional(deliberate);
    }
    match (observed.code, observed.signal) {
        (Some(0), _) => ExitKind::Intentional(DeliberateExit::CleanExit),
        _ => ExitKind::Abnormal,
    }
}

/// Why supervision stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    /// The daemon left on purpose; nothing restarts it.
    IntentionalExit(DeliberateExit),
    /// Restart budget exhausted inside the window: banner, no crash loop.
    RestartLimitReached,
    /// No live keeper: supervised restart is refused ([`supervised_restart_gate`]).
    NoLiveKeeper,
    /// A keeper is live but owns no new sessions, so a restart would still drop them.
    NoKeeperOwnership,
    /// Another supervisor already owns this runtime ([`SupervisorLock`]): nothing was started.
    AlreadySupervised,
    SpawnFailed(String),
    WaitFailed(String),
}

impl StopReason {
    /// The `reason=` token the restart log carries.
    pub fn token(&self) -> String {
        match self {
            Self::IntentionalExit(why) => format!("intentional:{}", why.kind()),
            Self::RestartLimitReached => "restart_limit".to_string(),
            Self::NoLiveKeeper => "no_live_keeper".to_string(),
            Self::NoKeeperOwnership => "no_keeper_ownership".to_string(),
            Self::AlreadySupervised => "already_supervised".to_string(),
            Self::SpawnFailed(_) => "spawn_failed".to_string(),
            Self::WaitFailed(_) => "wait_failed".to_string(),
        }
    }
}

/// Appends one line per supervision decision to `<data>/logs/daemon-crash.log`.
pub struct RestartLog {
    path: PathBuf,
}

impl RestartLog {
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The live restart log of this runtime, or `None` when no private data directory resolves.
    pub fn for_runtime() -> Option<Self> {
        crate::daemon::logging::logs_dir()
            .ok()
            .map(|dir| Self::at(dir.join(RESTART_LOG_NAME)))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The newest user-facing banner this log holds, or `None` when the log's newest decision
    /// supersedes every banner in it. A missing or unreadable log is `None`, never an error: no
    /// banner exists when the supervisor never ran.
    ///
    /// One rule decides staleness: a banner is only current while no later
    /// `event=gate supervised_restart=on` line exists. A refusal describes a state in which
    /// supervision is off, so the moment the gate opens the refusal is no longer true and the UI
    /// must stop showing it - otherwise "the daemon was not started" would outlive the login that
    /// wrote it and sit in front of a healthy supervised daemon. A refusal is deliberately *not*
    /// cleared by the single unattended start it allows, because that start is exactly the state
    /// the banner describes.
    pub fn latest_banner(&self) -> Option<String> {
        let text = std::fs::read_to_string(&self.path).ok()?;
        let mut banner: Option<String> = None;
        for line in text.lines() {
            if let Some(rest) = line.split(" event=banner ").nth(1) {
                if let Some(text) = rest.strip_prefix("text=") {
                    banner = Some(text.to_string());
                }
                continue;
            }
            if line.contains(" event=gate supervised_restart=on") {
                banner = None;
            }
        }
        banner
    }

    /// One `ts=<unix_ms> key=value ...` line. A failed append is reported, never fatal: the
    /// supervisor must still make its decision when the disk is full.
    fn write(&self, fields: &[(&str, String)]) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("restart log directory {}: {e}", parent.display()))?;
        }
        let mut line = format!("ts={} ", unix_ms());
        for (index, (key, value)) in fields.iter().enumerate() {
            if index != 0 {
                line.push(' ');
            }
            line.push_str(key);
            line.push('=');
            line.push_str(&one_line(value));
        }
        line.push('\n');
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| format!("restart log {}: {e}", self.path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let _ = file.set_permissions(std::fs::Permissions::from_mode(0o600));
        }
        file.write_all(line.as_bytes())
            .map_err(|e| format!("restart log {}: {e}", self.path.display()))
    }

    /// The startup gate decision, written once before any daemon lifetime begins.
    pub fn record_gate(&self, detail: &str, enabled: bool) -> Result<(), String> {
        self.write(&[
            ("event", "gate".to_string()),
            (
                "supervised_restart",
                if enabled { "on" } else { "off" }.to_string(),
            ),
            ("detail", detail.to_string()),
        ])
    }

    /// Why this process did not supervise: a closed gate it is about to serve around
    /// (`decision=serve_once`), or a claim refusal that starts nothing (`decision=stop`).
    pub fn record_refusal(
        &self,
        code: &str,
        decision: &str,
        detail: &str,
    ) -> Result<(), String> {
        self.write(&[
            ("event", "refusal".to_string()),
            ("code", code.to_string()),
            ("decision", decision.to_string()),
            ("detail", detail.to_string()),
        ])
    }

    /// The one unsupervised start a refused gate is allowed. Recorded as its own line so the shape
    /// "one start, zero restarts" is assertable without inferring it from process state.
    pub fn record_unattended_start(&self, pid: u32) -> Result<(), String> {
        self.write(&[
            ("event", "start".to_string()),
            ("pid", pid.to_string()),
            ("supervised", "off".to_string()),
            ("decision", "serve_once".to_string()),
        ])
    }

    /// The first supervised daemon lifetime. Recorded as its own line so the harness can count
    /// starts without inferring them from process state.
    pub fn record_start(&self, pid: u32) -> Result<(), String> {
        self.write(&[
            ("event", "start".to_string()),
            ("pid", pid.to_string()),
            ("supervised", "on".to_string()),
            ("decision", "supervise".to_string()),
        ])
    }

    /// The fell-over decision, before any restart is attempted.
    pub fn record_exit(
        &self,
        pid: u32,
        observed: ExitObservation,
        kind: ExitKind,
        restarts: usize,
    ) -> Result<(), String> {
        let kind_token = match kind {
            ExitKind::Intentional(why) => why.kind(),
            ExitKind::Abnormal => "abnormal",
        };
        self.write(&[
            ("event", "exit".to_string()),
            ("pid", pid.to_string()),
            ("exit", observed.describe()),
            ("kind", kind_token.to_string()),
            ("restarts", restarts.to_string()),
        ])
    }

    pub fn record_restart(&self, pid: u32, attempt: usize, after: Duration) -> Result<(), String> {
        self.write(&[
            ("event", "restart".to_string()),
            ("pid", pid.to_string()),
            ("attempt", attempt.to_string()),
            ("after_ms", after.as_millis().to_string()),
            ("decision", "restart".to_string()),
        ])
    }

    pub fn record_stop(&self, reason: &StopReason, restarts: usize) -> Result<(), String> {
        let mut fields = vec![
            ("event", "stop".to_string()),
            ("reason", reason.token()),
            ("restarts", restarts.to_string()),
            ("decision", "stop".to_string()),
        ];
        if let StopReason::SpawnFailed(detail) | StopReason::WaitFailed(detail) = reason {
            fields.push(("detail", detail.clone()));
        }
        self.write(&fields)
    }

    /// The user-visible banner. Written as its own line so a reader (and the harness) can find it
    /// without parsing a decision.
    pub fn record_banner(&self, banner: &str) -> Result<(), String> {
        self.write(&[("event", "banner".to_string()), ("text", banner.to_string())])
    }
}

fn one_line(value: &str) -> String {
    value
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect()
}

fn unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// The banner shown when the restart budget is spent.
pub fn restart_limit_banner(policy: &RestartPolicy) -> String {
    format!(
        "The Ferryx daemon crashed {} times in {} minutes and was not restarted again. \
         Your terminal sessions are preserved by the PTY keeper; open Ferryx to reattach.",
        policy.max_restarts,
        policy.window.as_secs() / 60
    )
}

/// The supervision loop's side effects, injected so every decision is testable with no real daemon.
pub trait SupervisedHost {
    /// Takes this runtime's single-supervisor claim, or reports who else holds it.
    ///
    /// Called once, before the first decision, so a second supervisor decides *before* it touches
    /// the runtime. The default succeeds, which keeps the loop's tests free of a real lock and
    /// leaves lock acquisition to the production host.
    fn exclusive_ownership(&mut self) -> Result<(), SupervisorClaimError> {
        Ok(())
    }

    /// Starts the daemon **for supervision** and returns its pid.
    fn spawn_daemon(&mut self) -> Result<u32, String>;

    /// Starts the daemon **once, unsupervised**, for a login the gate refused to supervise.
    ///
    /// Deliberately not `spawn_daemon`: the refusal path gets its own method so a restart loop can
    /// never reach it, and so a test can prove the refusal starts at most one daemon by counting
    /// calls to this method alone.
    fn spawn_unattended(&mut self) -> Result<u32, String>;

    /// Blocks until `pid` exits and reports how.
    fn wait(&mut self, pid: u32) -> Result<ExitObservation, String>;
    /// What the daemon recorded before leaving ([`DeliberateExit`]).
    fn deliberate_exit(&mut self, pid: u32) -> DeliberateExit;
    fn sleep(&mut self, duration: Duration);
    fn banner(&mut self, message: &str);
}

/// Everything the loop needs that is not a side effect.
pub struct Supervision<'a> {
    pub log: &'a RestartLog,
    pub policy: RestartPolicy,
    pub keeper: &'a dyn KeeperPresence,
}

/// One supervised daemon lifetime after another until the loop decides to stop.
///
/// Three rules, in this order:
///
/// 1. **One supervisor per runtime.** [`SupervisedHost::exclusive_ownership`] is asked first, and a
///    refusal starts nothing: another supervisor already serves this runtime, so starting a second
///    daemon would only race it for the instance lock. The refusal is recorded and the loop returns
///    without writing a banner, because "the daemon was not started" would be false.
/// 2. **The keeper gate is evaluated before supervision starts**, not only before a restart. A
///    runtime the gate refuses still gets exactly one daemon, through
///    [`SupervisedHost::spawn_unattended`], and this process then stops instead of supervising. There
///    is therefore no state in which supervised restart exists without a keeper that also owns the
///    sessions, and no code path in which a crash is answered by an unsupervised relaunch: the
///    unattended start is outside the loop and the loop's own start is `spawn_daemon`.
/// 3. **The gate is re-checked before every restart**, because a keeper can die - or stop owning -
///    while the daemon is running.
pub fn run_supervision(host: &mut dyn SupervisedHost, supervision: &Supervision<'_>) -> StopReason {
    let log = supervision.log;
    let policy = &supervision.policy;

    if let Err(refusal) = host.exclusive_ownership() {
        let reason = refusal.stop_reason();
        let _ = log.record_refusal(refusal.token(), "stop", &refusal.detail());
        let _ = log.record_stop(&reason, 0);
        return reason;
    }

    let keeper_status = match supervised_restart_gate(supervision.keeper) {
        Ok(status) => status,
        Err(refusal) => {
            let reason = refusal.stop_reason();
            let banner = refusal.banner();
            let _ = log.record_gate(refusal.detail(), false);
            let _ = log.record_refusal(refusal.token(), "serve_once", refusal.detail());
            let _ = log.record_banner(&banner);
            host.banner(&banner);
            // The daemon still has to serve this login; it is simply not supervised. This is the
            // refusal path's only start, and it is a different call from the loop's.
            return match host.spawn_unattended() {
                Ok(pid) => {
                    let _ = log.record_unattended_start(pid);
                    let _ = log.record_stop(&reason, 0);
                    reason
                }
                Err(detail) => {
                    let reason = StopReason::SpawnFailed(detail);
                    let _ = log.record_stop(&reason, 0);
                    reason
                }
            };
        }
    };
    let _ = log.record_gate(
        &format!(
            "supervised restart enabled: a live PTY keeper owns new sessions ({})",
            keeper_status.pid().map_or_else(
                || "keeper pid unavailable".to_string(),
                |pid| format!("pid {pid}"),
            )
        ),
        true,
    );

    let mut restarts: VecDeque<Instant> = VecDeque::new();

    let mut pid = match host.spawn_daemon() {
        Ok(pid) => pid,
        Err(detail) => {
            let reason = StopReason::SpawnFailed(detail);
            let _ = log.record_stop(&reason, 0);
            return reason;
        }
    };
    let _ = log.record_start(pid);

    loop {
        let observed = match host.wait(pid) {
            Ok(observed) => observed,
            Err(detail) => {
                let reason = StopReason::WaitFailed(detail);
                let _ = log.record_stop(&reason, restarts.len());
                return reason;
            }
        };
        let deliberate = host.deliberate_exit(pid);
        let kind = classify_exit(observed, deliberate);
        let _ = log.record_exit(pid, observed, kind, restarts.len());

        if let ExitKind::Intentional(why) = kind {
            let reason = StopReason::IntentionalExit(why);
            let _ = log.record_stop(&reason, restarts.len());
            return reason;
        }

        // HARD RULE again: a keeper that died while the daemon ran must not be restarted either.
        if let Err(refusal) = supervised_restart_gate(supervision.keeper) {
            let reason = refusal.stop_reason();
            let _ = log.record_stop(&reason, restarts.len());
            let _ = log.record_gate(refusal.detail(), false);
            let _ = log.record_refusal(refusal.token(), "stop", refusal.detail());
            let banner = refusal.banner();
            let _ = log.record_banner(&banner);
            host.banner(&banner);
            return reason;
        }

        let now = Instant::now();
        while restarts
            .front()
            .is_some_and(|at| now.saturating_duration_since(*at) >= policy.window)
        {
            restarts.pop_front();
        }
        if restarts.len() >= policy.max_restarts {
            let reason = StopReason::RestartLimitReached;
            let _ = log.record_stop(&reason, restarts.len());
            let banner = restart_limit_banner(policy);
            let _ = log.record_banner(&banner);
            host.banner(&banner);
            return reason;
        }

        let attempt = restarts.len() + 1;
        let after = policy.backoff(attempt);
        let _ = log.record_restart(pid, attempt, after);
        host.sleep(after);
        restarts.push_back(Instant::now());
        pid = match host.spawn_daemon() {
            Ok(pid) => pid,
            Err(detail) => {
                let reason = StopReason::SpawnFailed(detail);
                let _ = log.record_stop(&reason, restarts.len());
                return reason;
            }
        };
    }
}

/// The production host: spawns `<current exe> --daemon`, holds this runtime's supervisor claim, and
/// reads what the daemon recorded before leaving (a retirement marker, or a handover decision).
#[cfg(unix)]
pub struct ProcessHost {
    exe: PathBuf,
    runtime_dir: PathBuf,
    child: Option<std::process::Child>,
    /// The single-supervisor claim, taken by [`SupervisedHost::exclusive_ownership`] before the
    /// first decision and held until this host is dropped. Dropping it releases the lock, which is
    /// why a supervisor that returns for any reason leaves the runtime reclaimable.
    claim: Option<SupervisorLock>,
}

#[cfg(unix)]
impl ProcessHost {
    pub fn for_runtime() -> Result<Self, String> {
        let exe = std::env::current_exe()
            .map_err(|e| format!("Failed to get current exe path: {e}"))?;
        Ok(Self {
            exe,
            runtime_dir: crate::daemon::server::get_runtime_dir(),
            child: None,
            claim: None,
        })
    }

    /// Spawns `<current exe> --daemon`, recording it as the child [`SupervisedHost::wait`] reaps.
    /// Both starts go through here so they cannot drift apart in stdio handling, which is the only
    /// thing that distinguishes them.
    fn spawn(&mut self) -> Result<u32, String> {
        let mut command = std::process::Command::new(&self.exe);
        command
            .arg("--daemon")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let child = command
            .spawn()
            .map_err(|e| format!("Failed to spawn {} --daemon: {e}", self.exe.display()))?;
        let pid = child.id();
        self.child = Some(child);
        Ok(pid)
    }

    /// Reads the retirement marker the daemon writes before a deliberate exit
    /// (`control_server::finish_control_retirement`) and maps its reason onto
    /// [`DeliberateExit`]. A marker naming another pid belongs to an earlier daemon.
    fn read_deliberate_exit(runtime_dir: &Path, pid: u32) -> DeliberateExit {
        let key = crate::daemon::owner_admission::current_generation_key();
        for name in
            crate::daemon::supervisor_windows::retirement_marker_names(Some(&key), pid)
        {
            let Ok(body) = std::fs::read_to_string(runtime_dir.join(name)) else {
                continue;
            };
            if !crate::daemon::supervisor_windows::marker_names_pid(&body, pid) {
                continue;
            }
            let reason = serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|value| {
                    value
                        .get("reason")
                        .and_then(|reason| reason.as_str())
                        .map(str::to_lowercase)
                })
                .unwrap_or_default();
            if reason.contains("handover") {
                return DeliberateExit::HandoverDecision;
            }
            if reason.contains("upgrade") || reason.contains("swap") {
                return DeliberateExit::RequestedUpgrade;
            }
            return DeliberateExit::CleanExit;
        }
        DeliberateExit::None
    }

    /// Whether `pid` committed a handover and retired, judged from the durable decision record the
    /// commit wrote: `<runtime>/legacy-<pid>-<disambiguator>.transaction`.
    ///
    /// This is the only durable evidence a v5 handover leaves. `HandoverManager::retirement_action`
    /// is a bare `exit(0)` that writes no retirement marker, so without this read a committed
    /// handover is indistinguishable from an idle clean exit - and a predecessor that exited
    /// non-zero after committing would be treated as a crash and restarted while its successor owns
    /// the endpoint. `predecessorPid` is the committing process's own pid
    /// (`handover::record_decision` passes `std::process::id()`), and `state` is the snake_case
    /// `"retired"` the record serialises, so neither has to be inferred.
    fn handover_decision_for(runtime_dir: &Path, pid: u32) -> Option<DeliberateExit> {
        let prefix = format!("legacy-{pid}-");
        let entries = std::fs::read_dir(runtime_dir).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
            if !name.starts_with(&prefix) || !name.ends_with(".transaction") {
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                continue;
            };
            let predecessor = value.get("predecessorPid").and_then(|v| v.as_u64());
            let state = value.get("state").and_then(|v| v.as_str());
            if predecessor == Some(u64::from(pid)) && state == Some("retired") {
                return Some(DeliberateExit::HandoverDecision);
            }
        }
        None
    }
    /// Whether `runtime_dir` holds a valid, unexpired handover request naming this generation and
    /// its live predecessor. Read-only: leaves `handover-request.json` intact so the spawned
    /// `--daemon` child consumes it through `handover_request::take_startup_request`.
    fn has_authorized_handover_request(
        runtime_dir: &Path,
        own_generation_key: Option<&str>,
        now_unix_ms: u64,
        start_time_of: &dyn Fn(u32) -> Option<u64>,
    ) -> bool {
        use crate::daemon::handover_request::{
            request_path, startup_role, HandoverRequest, StartupRole,
        };
        use crate::daemon::owner_admission::read_record;

        let request = read_record::<HandoverRequest>(&request_path(runtime_dir));
        matches!(
            startup_role(request, own_generation_key, now_unix_ms, start_time_of),
            StartupRole::Candidate { .. }
        )
    }

    fn is_authorized_handover_successor(&self) -> bool {
        let own_key = crate::daemon::handover_request::own_generation_key();
        let now_ms = crate::daemon::owner_admission::unix_now_ms();
        let start_time_of = &crate::daemon::owner_admission::process_start_time;
        if Self::has_authorized_handover_request(
            &self.runtime_dir,
            own_key.as_deref(),
            now_ms,
            start_time_of,
        ) {
            return true;
        }
        if let Some(legacy_dir) = crate::daemon::server::get_legacy_runtime_dir()
            .filter(|legacy| *legacy != self.runtime_dir)
        {
            return Self::has_authorized_handover_request(
                &legacy_dir,
                own_key.as_deref(),
                now_ms,
                start_time_of,
            );
        }
        false
    }

    fn try_reclaim_supervisor_lock(&mut self) {
        if self.claim.is_none() {
            if let Ok(lock) = SupervisorLock::acquire(&self.runtime_dir) {
                self.claim = Some(lock);
            }
        }
    }
}

#[cfg(unix)]
impl SupervisedHost for ProcessHost {
    fn exclusive_ownership(&mut self) -> Result<(), SupervisorClaimError> {
        if self.claim.is_some() {
            return Ok(());
        }
        match SupervisorLock::acquire(&self.runtime_dir) {
            Ok(lock) => {
                self.claim = Some(lock);
                Ok(())
            }
            Err(SupervisorClaimError::AlreadyRunning { path }) => {
                // During a RequestUpgrade rolling handover, the predecessor generation's crash
                // supervisor holds `daemon-supervisor.lock` until the successor daemon commits
                // handover and the predecessor finishes retiring. Admit the authorized successor
                // generation without consuming `handover-request.json`, then claim the lock in
                // `wait()` once the predecessor releases it.
                if self.is_authorized_handover_successor() {
                    Ok(())
                } else {
                    Err(SupervisorClaimError::AlreadyRunning { path })
                }
            }
            Err(other) => Err(other),
        }
    }

    fn spawn_daemon(&mut self) -> Result<u32, String> {
        self.try_reclaim_supervisor_lock();
        self.spawn()
    }

    fn spawn_unattended(&mut self) -> Result<u32, String> {
        self.spawn()
    }

    fn wait(&mut self, _pid: u32) -> Result<ExitObservation, String> {
        use std::os::unix::process::ExitStatusExt as _;

        let Some(mut child) = self.child.take() else {
            return Err("supervised daemon was not spawned by this supervisor".to_string());
        };
        let status = loop {
            if self.claim.is_some() {
                break child
                    .wait()
                    .map_err(|e| format!("Failed to wait for the daemon: {e}"))?;
            }
            self.try_reclaim_supervisor_lock();
            if let Some(exited) = child
                .try_wait()
                .map_err(|e| format!("Failed to wait for the daemon: {e}"))?
            {
                break exited;
            }
            std::thread::sleep(Duration::from_millis(200));
        };
        Ok(match status.signal() {
            Some(signal) => ExitObservation::signaled(signal),
            None => ExitObservation::code(status.code().unwrap_or(-1)),
        })
    }

    fn deliberate_exit(&mut self, pid: u32) -> DeliberateExit {
        // A marker is written by a control-v1 retirement; a committed handover writes only its
        // decision record. The marker is checked first because it is the narrower statement: it
        // names this pid and this reason, while the record only proves a commit happened.
        match Self::read_deliberate_exit(&self.runtime_dir, pid) {
            DeliberateExit::None => {
                Self::handover_decision_for(&self.runtime_dir, pid).unwrap_or(DeliberateExit::None)
            }
            recorded => recorded,
        }
    }

    fn sleep(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }

    fn banner(&mut self, message: &str) {
        // stderr reaches the launchd job's StandardErrorPath; the restart log already has the line.
        tracing::warn!(banner = message, "daemon supervision stopped");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// No polling: the log is closed before `run_supervision` returns, and a fixed sleep would let a
    /// test pass on timing luck instead of on the recorded decision.
    fn read_log(path: &Path) -> String {
        std::fs::read_to_string(path).expect("the supervision run writes its restart log")
    }

    /// A host whose daemons exit with a scripted status, so the loop runs against real process
    /// exits without waiting on anything. `unattended` counts the refusal path's one start
    /// separately from the loop's, which is what makes "one start, zero restarts" assertable.
    struct ScriptedHost {
        exits: Vec<ExitObservation>,
        deliberate: DeliberateExit,
        spawns: Arc<AtomicUsize>,
        unattended: Arc<AtomicUsize>,
        claim: Result<(), SupervisorClaimError>,
        waited: usize,
        banners: Vec<String>,
    }

    impl ScriptedHost {
        fn new(exits: Vec<ExitObservation>, deliberate: DeliberateExit) -> Self {
            Self {
                exits,
                deliberate,
                spawns: Arc::new(AtomicUsize::new(0)),
                unattended: Arc::new(AtomicUsize::new(0)),
                claim: Ok(()),
                waited: 0,
                banners: Vec::new(),
            }
        }

        /// A host whose runtime another supervisor already owns.
        fn already_supervised(exits: Vec<ExitObservation>) -> Self {
            Self {
                claim: Err(SupervisorClaimError::AlreadyRunning {
                    path: PathBuf::from("/tmp/fx/daemon-supervisor.lock"),
                }),
                ..Self::new(exits, DeliberateExit::None)
            }
        }
    }

    impl SupervisedHost for ScriptedHost {
        fn exclusive_ownership(&mut self) -> Result<(), SupervisorClaimError> {
            self.claim.clone()
        }

        fn spawn_daemon(&mut self) -> Result<u32, String> {
            let spawn = self.spawns.fetch_add(1, Ordering::SeqCst) + 1;
            Ok(spawn as u32)
        }

        fn spawn_unattended(&mut self) -> Result<u32, String> {
            let spans = self.spawns.fetch_add(1, Ordering::SeqCst) + 1;
            self.unattended.fetch_add(1, Ordering::SeqCst);
            Ok(spans as u32)
        }

        fn wait(&mut self, _pid: u32) -> Result<ExitObservation, String> {
            let observed = self
                .exits
                .get(self.waited)
                .copied()
                .unwrap_or(ExitObservation::code(1));
            self.waited += 1;
            Ok(observed)
        }

        fn deliberate_exit(&mut self, _pid: u32) -> DeliberateExit {
            self.deliberate
        }

        fn sleep(&mut self, _duration: Duration) {}

        fn banner(&mut self, message: &str) {
            self.banners.push(message.to_string());
        }
    }

    struct LiveKeeper;
    impl KeeperPresence for LiveKeeper {
        fn live_keeper(&self) -> KeeperStatus {
            KeeperStatus::Live { pid: Some(4242) }
        }

        fn owns_new_sessions(&self) -> bool {
            true
        }
    }

    /// Live once, then absent: a keeper that exits while the daemon is still running.
    struct KeeperDiesAfterOneCheck(AtomicUsize);

    impl KeeperPresence for KeeperDiesAfterOneCheck {
        fn live_keeper(&self) -> KeeperStatus {
            if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
                KeeperStatus::Live { pid: Some(4242) }
            } else {
                KeeperStatus::Absent {
                    detail: "keeper exited while the daemon ran".to_string(),
                }
            }
        }

        fn owns_new_sessions(&self) -> bool {
            true
        }
    }

    /// The plan's hazard state: a keeper answers, but every session is still direct-owned.
    struct LiveKeeperWithoutOwnership;

    impl KeeperPresence for LiveKeeperWithoutOwnership {
        fn live_keeper(&self) -> KeeperStatus {
            KeeperStatus::Live { pid: Some(4242) }
        }
    }

    /// Live and owning at startup, then still live but no longer owning.
    struct KeeperLosesOwnershipAfterOneCheck(AtomicUsize);

    impl KeeperPresence for KeeperLosesOwnershipAfterOneCheck {
        fn live_keeper(&self) -> KeeperStatus {
            KeeperStatus::Live { pid: Some(4242) }
        }

        fn owns_new_sessions(&self) -> bool {
            self.0.fetch_add(1, Ordering::SeqCst) == 0
        }
    }

    fn policy() -> RestartPolicy {
        RestartPolicy {
            max_restarts: MAX_RESTARTS_PER_WINDOW,
            window: RESTART_WINDOW,
            base_backoff: Duration::from_millis(1),
            max_backoff: Duration::from_millis(4),
        }
    }

    fn run(dir: &Path, host: &mut ScriptedHost, keeper: &dyn KeeperPresence) -> (StopReason, String) {
        let log = RestartLog::at(dir.join(RESTART_LOG_NAME));
        let supervision = Supervision {
            log: &log,
            policy: policy(),
            keeper,
        };
        let result = run_supervision(host, &supervision);
        let text = read_log(log.path());
        (result, text)
    }

    #[test]
    fn abnormal_exit_restarts_once_and_is_recorded() {
        let dir = tempfile::tempdir().unwrap();
        // One crash, then a daemon that stays down (waited again with no exit scripted) until the
        // harness stops reading: the second wait returns code 0, which ends supervision.
        let mut host = ScriptedHost::new(
            vec![ExitObservation::signaled(11), ExitObservation::code(0)],
            DeliberateExit::None,
        );
        let (result, log) = run(dir.path(), &mut host, &LiveKeeper);

        assert_eq!(
            result,
            StopReason::IntentionalExit(DeliberateExit::CleanExit),
            "an abnormal exit must be restarted exactly once, then the exit 0 stops the loop: {log}"
        );
        assert_eq!(
            host.spawns.load(Ordering::SeqCst),
            2,
            "one initial start plus one restart: {log}"
        );
        assert!(
            log.contains("exit=signal=11 kind=abnormal"),
            "the crash must be recorded with its signal: {log}"
        );
        assert_eq!(
            log.matches("event=restart").count(),
            1,
            "exactly one restart line: {log}"
        );
        assert!(log.contains("decision=restart"), "{log}");
        assert!(
            log.contains("event=gate supervised_restart=on"),
            "a live keeper must open the gate before supervision starts: {log}"
        );
    }

    #[test]
    fn exit_zero_does_not_restart() {
        let dir = tempfile::tempdir().unwrap();
        let mut host = ScriptedHost::new(vec![ExitObservation::code(0)], DeliberateExit::None);
        let (result, log) = run(dir.path(), &mut host, &LiveKeeper);

        assert_eq!(
            result,
            StopReason::IntentionalExit(DeliberateExit::CleanExit),
            "exit 0 is intentional: {log}"
        );
        assert_eq!(host.spawns.load(Ordering::SeqCst), 1, "no restart: {log}");
        assert!(
            !log.contains("event=restart"),
            "an exit 0 must never write a restart line: {log}"
        );
        assert!(log.contains("kind=clean_exit"), "{log}");
    }

    #[test]
    fn handover_decision_does_not_restart() {
        let dir = tempfile::tempdir().unwrap();
        // A handover predecessor retires on purpose; even a non-zero status must not restart it.
        let mut host = ScriptedHost::new(
            vec![ExitObservation::code(1)],
            DeliberateExit::HandoverDecision,
        );
        let (result, log) = run(dir.path(), &mut host, &LiveKeeper);

        assert_eq!(
            result,
            StopReason::IntentionalExit(DeliberateExit::HandoverDecision),
            "a handover decision is intentional: {log}"
        );
        assert_eq!(host.spawns.load(Ordering::SeqCst), 1, "no restart: {log}");
        assert_eq!(log.matches("event=restart").count(), 0, "{log}");
        assert!(log.contains("kind=handover"), "{log}");
    }

    #[test]
    fn six_rapid_failures_stop_at_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        let mut host = ScriptedHost::new(
            vec![
                ExitObservation::code(1),
                ExitObservation::code(1),
                ExitObservation::code(1),
                ExitObservation::code(1),
                ExitObservation::code(1),
                ExitObservation::code(1),
            ],
            DeliberateExit::None,
        );
        let (result, log) = run(dir.path(), &mut host, &LiveKeeper);

        assert_eq!(
            result,
            StopReason::RestartLimitReached,
            "six failures inside the window must stop at the fifth restart: {log}"
        );
        assert_eq!(
            host.spawns.load(Ordering::SeqCst),
            MAX_RESTARTS_PER_WINDOW + 1,
            "one initial start plus five restarts: {log}"
        );
        assert_eq!(
            log.matches("event=restart").count(),
            MAX_RESTARTS_PER_WINDOW,
            "the cap is five restarts per window: {log}"
        );
        assert!(log.contains("reason=restart_limit"), "{log}");
        assert!(log.contains("event=banner"), "the stop must surface a banner: {log}");
        assert!(log.contains("crashed 5 times in 10 minutes"), "{log}");
    }

    /// A live keeper that owns nothing is the plan's hazard state: restarting would drop every
    /// session while the gate read "safe". It must refuse exactly as an absent keeper does, and
    /// the refusal must name ownership so the two are distinguishable in the log.
    #[test]
    fn a_live_keeper_without_ownership_refuses_the_restart() {
        let dir = tempfile::tempdir().unwrap();
        let mut host = ScriptedHost::new(vec![ExitObservation::signaled(9)], DeliberateExit::None);
        let (result, log) = run(dir.path(), &mut host, &LiveKeeperWithoutOwnership);

        assert_eq!(
            result,
            StopReason::NoKeeperOwnership,
            "a live keeper that owns no sessions must never enable a restart: {log}"
        );
        assert_eq!(
            host.spawns.load(Ordering::SeqCst),
            1,
            "the initial start is allowed, the restart is not: {log}"
        );
        assert_eq!(log.matches("event=restart").count(), 0, "{log}");
        assert!(log.contains("reason=no_keeper_ownership"), "{log}");
        assert!(
            !log.contains("reason=no_live_keeper"),
            "the refusal must distinguish missing liveness from missing ownership: {log}"
        );
        assert!(
            log.contains("event=gate supervised_restart=off"),
            "the gate decision itself must be on record: {log}"
        );
        assert!(
            log.contains("does not own new sessions"),
            "the banner must say why restart is off: {log}"
        );
        assert_eq!(host.banners.len(), 1, "{:?}", host.banners);
    }

    /// The same refusal must be reachable *after* a crash, not only at startup: ownership can be
    /// turned off while the daemon runs, and the re-check must then answer the same way.
    #[test]
    fn ownership_lost_mid_run_refuses_the_restart() {
        let dir = tempfile::tempdir().unwrap();
        let mut host = ScriptedHost::new(vec![ExitObservation::signaled(11)], DeliberateExit::None);
        let keeper = KeeperLosesOwnershipAfterOneCheck(AtomicUsize::new(0));
        let (result, log) = run(dir.path(), &mut host, &keeper);

        assert_eq!(
            result,
            StopReason::NoKeeperOwnership,
            "a crash with a keeper that stopped owning must not be restarted: {log}"
        );
        assert_eq!(log.matches("event=restart").count(), 0, "{log}");
        assert!(
            log.contains("event=gate supervised_restart=on"),
            "the startup gate saw a keeper that owned new sessions: {log}"
        );
        assert!(
            log.contains("event=gate supervised_restart=off"),
            "the mid-run loss of ownership must close the gate on record: {log}"
        );
    }

    #[test]
    fn no_live_keeper_keeps_supervised_restart_off() {
        let dir = tempfile::tempdir().unwrap();
        let mut host = ScriptedHost::new(vec![ExitObservation::signaled(9)], DeliberateExit::None);
        let (result, log) = run(dir.path(), &mut host, &NoKeeper);

        assert_eq!(
            result,
            StopReason::NoLiveKeeper,
            "without a live keeper the daemon must never be restarted: {log}"
        );
        assert_eq!(
            host.spawns.load(Ordering::SeqCst),
            1,
            "exactly one start is allowed, and no restart: {log}"
        );
        assert_eq!(
            host.unattended.load(Ordering::SeqCst),
            1,
            "the one start must come from the refusal path, not the loop: {log}"
        );
        assert_eq!(log.matches("event=restart").count(), 0, "{log}");
        assert!(log.contains("reason=no_live_keeper"), "{log}");
        assert!(
            log.contains("event=gate supervised_restart=off"),
            "the gate decision itself must be on record: {log}"
        );
        assert!(
            log.contains("event=refusal code=no_live_keeper decision=serve_once"),
            "the refusal must be a typed decision, not only a banner: {log}"
        );
        assert!(
            log.contains("event=start pid=1 supervised=off decision=serve_once"),
            "the one allowed start must be on record as unsupervised: {log}"
        );
        assert!(
            log.contains("event=banner") && log.contains("no live PTY keeper"),
            "the refusal must be surfaced, not silent: {log}"
        );
        assert_eq!(host.banners.len(), 1, "{:?}", host.banners);
    }

    /// A runtime another supervisor already owns gets nothing from this process: no daemon, no
    /// banner, no claim. A healthy supervisor already serves it, so a second start would race the
    /// instance lock and a banner would be a false statement about a working daemon.
    #[test]
    fn a_second_supervisor_starts_nothing_and_stays_silent() {
        let dir = tempfile::tempdir().unwrap();
        let mut host = ScriptedHost::already_supervised(vec![ExitObservation::signaled(9)]);
        let (result, log) = run(dir.path(), &mut host, &LiveKeeper);

        assert_eq!(
            result,
            StopReason::AlreadySupervised,
            "a supervisor that lost the claim must stop: {log}"
        );
        assert_eq!(
            host.spawns.load(Ordering::SeqCst),
            0,
            "a second supervisor must start no daemon at all: {log}"
        );
        assert_eq!(
            host.unattended.load(Ordering::SeqCst),
            0,
            "the refusal path must not run either: {log}"
        );
        assert_eq!(
            log.matches("event=gate").count(),
            0,
            "no gate was decided, because nothing was supervised: {log}"
        );
        assert!(
            log.contains("event=refusal code=supervisor_already_running decision=stop"),
            "the claim refusal is on record: {log}"
        );
        assert!(
            log.contains("reason=already_supervised"),
            "the stop reason names the cause: {log}"
        );
        assert!(
            host.banners.is_empty(),
            "nothing may be shown to the user while another supervisor serves the runtime: {:?}",
            host.banners
        );
        assert!(
            !log.contains("decision=restart"),
            "a refused claim can never restart anything: {log}"
        );
    }

    /// The real claim, twice, on one runtime directory: the second acquisition fails with
    /// `AlreadyRunning` and the first keeps ownership until it is dropped.
    #[cfg(unix)]
    #[test]
    fn the_supervisor_claim_is_exclusive_and_released_by_drop() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = dir.path().join("runtime");

        let first = SupervisorLock::acquire(&runtime).expect("the first claim must succeed");
        assert_eq!(first.path(), runtime.join(SUPERVISOR_LOCK_NAME));
        assert!(
            first.path().exists(),
            "the claim is a file, so a reader can see which runtime is supervised"
        );
        match SupervisorLock::acquire(&runtime) {
            Err(SupervisorClaimError::AlreadyRunning { path }) => {
                assert_eq!(path, runtime.join(SUPERVISOR_LOCK_NAME))
            }
            other => panic!("the second claim must be refused, got {other:?}"),
        }

        drop(first);
        SupervisorLock::acquire(&runtime)
            .expect("the claim must be reclaimable once its holder is gone");
    }

    /// A handover predecessor's durable decision record is what makes its exit a handover. The
    /// record is matched on the committing pid and on the committed state, so a record left by an
    /// earlier daemon - or by an aborted handover - never classifies a later exit.
    #[cfg(unix)]
    #[test]
    fn a_committed_handover_record_classifies_the_exit_it_belongs_to() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = dir.path();
        let predecessor = 4242u32;

        assert_eq!(
            ProcessHost::handover_decision_for(runtime, predecessor),
            None,
            "no record yet"
        );

        let record = |predecessor_pid: u32, state: &str| {
            serde_json::json!({
                "transferId": format!("/tmp/x/legacy-{predecessor_pid}-1.sock"),
                "clientRequestId": "r",
                "predecessorPid": predecessor_pid,
                "predecessorEpoch": 7,
                "successorPid": 0,
                "successorEpoch": 0,
                "state": state,
                "fingerprint": "fp",
            })
        };
        let write = |name: &str, value: &serde_json::Value| {
            std::fs::write(runtime.join(name), serde_json::to_vec(value).unwrap()).unwrap();
        };

        // An aborted handover names this pid but never committed.
        write(&format!("legacy-{predecessor}-1.transaction"), &record(predecessor, "active"));
        assert_eq!(
            ProcessHost::handover_decision_for(runtime, predecessor),
            None,
            "an abort decision is not a handover retirement"
        );

        // A committed record of an earlier daemon names another pid.
        write("legacy-99-1.transaction", &record(99, "retired"));
        assert_eq!(
            ProcessHost::handover_decision_for(runtime, predecessor),
            None,
            "another daemon's record must not classify this exit"
        );

        write(&format!("legacy-{predecessor}-2.transaction"), &record(predecessor, "retired"));
        assert_eq!(
            ProcessHost::handover_decision_for(runtime, predecessor),
            Some(DeliberateExit::HandoverDecision),
            "a committed record for this pid is the durable handover evidence"
        );
        assert_eq!(
            ProcessHost::handover_decision_for(runtime, 7),
            None,
            "the same directory says nothing about a pid it has no record for"
        );
    }

    /// The banner must not outlive the state it describes: a later gate that opened supersedes a
    /// refusal, while the single unattended start a refusal allows does not.
    #[test]
    fn a_banner_is_dropped_once_the_gate_opens_again() {
        let dir = tempfile::tempdir().unwrap();
        let log = RestartLog::at(dir.path().join(RESTART_LOG_NAME));
        let banner = RestartRefusal::NoLiveKeeper {
            detail: "no keeper socket".to_string(),
        }
        .banner();

        log.record_gate("no keeper socket", false).unwrap();
        log.record_refusal("no_live_keeper", "serve_once", "no keeper socket")
            .unwrap();
        log.record_banner(&banner).unwrap();
        log.record_unattended_start(11).unwrap();
        assert_eq!(
            log.latest_banner(),
            Some(banner.clone()),
            "the unsupervised daemon the banner describes is still the current state"
        );

        log.record_gate("supervised restart enabled", true).unwrap();
        assert_eq!(
            log.latest_banner(),
            None,
            "an open gate makes the old refusal false, so it must not be shown"
        );
    }

    /// A keeper that dies mid-run must close the gate before any restart, not only at startup.
    #[test]
    fn a_keeper_dying_mid_run_stops_the_restart() {
        let dir = tempfile::tempdir().unwrap();
        let mut host = ScriptedHost::new(
            vec![ExitObservation::signaled(11)],
            DeliberateExit::None,
        );
        let keeper = KeeperDiesAfterOneCheck(AtomicUsize::new(0));
        let (result, log) = run(dir.path(), &mut host, &keeper);

        assert_eq!(
            result,
            StopReason::NoLiveKeeper,
            "a crash with no keeper left must not be restarted: {log}"
        );
        assert_eq!(
            host.spawns.load(Ordering::SeqCst),
            1,
            "the initial start is allowed, the restart is not: {log}"
        );
        assert_eq!(log.matches("event=restart").count(), 0, "{log}");
        assert!(
            log.contains("event=gate supervised_restart=on"),
            "the startup gate saw a live keeper: {log}"
        );
        assert!(
            log.contains("event=gate supervised_restart=off"),
            "the mid-run death must close the gate on record: {log}"
        );
        assert_eq!(host.banners.len(), 1, "{:?}", host.banners);
    }

    /// The gate itself: a keeper socket that nothing listens on is not a live keeper, so a stale
    /// socket file left by a dead keeper never turns supervised restart on.
    #[cfg(unix)]
    #[test]
    fn a_stale_keeper_socket_is_not_a_live_keeper() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join(KEEPER_SOCKET_NAME);
        std::fs::write(&socket, b"stale").unwrap();
        let probe = KeeperSocketProbe::new(dir.path());

        assert_eq!(
            probe.probe_socket(),
            KeeperStatus::Absent {
                detail: format!("{} is not a socket", socket.display())
            }
        );
        assert!(supervised_restart_gate(&probe).is_err());
        assert!(supervised_restart_gate(&NoKeeper).is_err());
    }

    #[cfg(unix)]
    fn hello_ack(
        request: &crate::daemon::keeper_client::KeeperFrame,
        payload: serde_json::Value,
    ) -> Vec<u8> {
        use crate::daemon::keeper_client::{KeeperFrame, KeeperKind};

        KeeperFrame::new(
            KeeperKind::HelloAck,
            request.request_id.clone(),
            request.frame_sequence,
            payload,
        )
        .encode()
    }

    /// A fake keeper: binds `socket`, answers `handshakes` connections with what `reply` returns, and
    /// yields every request frame it read so a test can assert what the probe actually sent.
    #[cfg(unix)]
    fn serve_handshakes(
        socket: &Path,
        handshakes: usize,
        reply: impl Fn(&crate::daemon::keeper_client::KeeperFrame) -> Vec<u8> + Send + 'static,
    ) -> std::thread::JoinHandle<Vec<crate::daemon::keeper_client::KeeperFrame>> {
        use std::io::{Read as _, Write as _};

        use crate::daemon::keeper_client::{KeeperFrame, KEEPER_HEADER_BYTES};

        // Bound here, not in the thread: the probe must never race the bind.
        let listener = std::os::unix::net::UnixListener::bind(socket).unwrap();
        std::thread::spawn(move || {
            let mut requests = Vec::with_capacity(handshakes);
            for _ in 0..handshakes {
                let (mut stream, _) = listener.accept().expect("probe connects");
                let mut header = [0u8; KEEPER_HEADER_BYTES];
                stream.read_exact(&mut header).expect("handshake header");
                let payload_len =
                    u32::from_be_bytes([header[8], header[9], header[10], header[11]]) as usize;
                let mut bytes = vec![0u8; KEEPER_HEADER_BYTES + payload_len];
                bytes[..KEEPER_HEADER_BYTES].copy_from_slice(&header);
                stream
                    .read_exact(&mut bytes[KEEPER_HEADER_BYTES..])
                    .expect("handshake payload");
                let request = KeeperFrame::decode(&bytes).expect("the probe sends one valid frame");
                let answer = reply(&request);
                if !answer.is_empty() {
                    stream.write_all(&answer).expect("handshake answer");
                }
                requests.push(request);
            }
            requests
        })
    }

    #[cfg(unix)]
    fn absent_detail(status: KeeperStatus) -> String {
        match status {
            KeeperStatus::Absent { detail } => detail,
            KeeperStatus::Live { pid } => panic!("expected Absent, got Live {{ pid: {pid:?} }}"),
        }
    }

    #[cfg(unix)]
    fn ack_payload() -> serde_json::Value {
        serde_json::json!({
            "protocolMajor": crate::daemon::keeper_client::KEEPER_PROTOCOL_MAJOR,
            "protocolMinor": 0,
            "keeperVersion": "1.2.3",
            "pid": std::process::id(),
        })
    }

    /// A keeper that completes the handshake is the only state in which the gate opens, and its
    /// reported pid is what the supervisor records.
    #[cfg(unix)]
    #[test]
    fn a_keeper_that_answers_the_handshake_opens_the_gate() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join(KEEPER_SOCKET_NAME);
        let server = serve_handshakes(&socket, 2, |request| hello_ack(request, ack_payload()));
        // The handshake alone must not open the gate; a keeper that owns new sessions is what does.
        let table = dir.path().join(crate::daemon::keeper_session::OWNERSHIP_FILE_NAME);
        crate::daemon::keeper_session::OwnershipStore::at(&table)
            .set_keeper_enabled(true)
            .unwrap();
        let probe = KeeperSocketProbe::new(dir.path()).with_ownership_table(&table);

        assert_eq!(
            probe.probe_socket(),
            KeeperStatus::Live {
                pid: Some(std::process::id())
            }
        );
        assert!(supervised_restart_gate(&probe).is_ok());
        let requests = server.join().expect("fake keeper");
        assert_eq!(requests.len(), 2, "one HELLO per probe, and the gate probes too");
        for request in requests {
            assert_eq!(
                request.kind,
                crate::daemon::keeper_client::KeeperKind::Hello,
                "readiness must be asked with HELLO, the one verb with no session side effect"
            );
            assert_eq!(
                request
                    .payload
                    .get("protocolMajor")
                    .and_then(serde_json::Value::as_u64),
                Some(u64::from(crate::daemon::keeper_client::KEEPER_PROTOCOL_MAJOR))
            );
            assert!(
                request.payload.get("sessionId").is_none()
                    && request.payload.get("epoch").is_none()
                    && request.payload.get("requestId").is_none(),
                "the probe must never name a session or fence an epoch: {}",
                request.payload
            );
        }
    }

    /// The seam F1 was: a real, complete, kernel-corroborated handshake with a table that does not
    /// route new sessions to the keeper. Presence is proven and the gate must still refuse, because
    /// a restart would drop the direct-owned sessions the keeper never held.
    #[cfg(unix)]
    #[test]
    fn a_responding_keeper_that_owns_no_sessions_keeps_the_gate_closed() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join(KEEPER_SOCKET_NAME);
        let server = serve_handshakes(&socket, 2, |request| hello_ack(request, ack_payload()));
        let table = dir.path().join(crate::daemon::keeper_session::OWNERSHIP_FILE_NAME);
        crate::daemon::keeper_session::OwnershipStore::at(&table)
            .set_keeper_enabled(false)
            .unwrap();
        let probe = KeeperSocketProbe::new(dir.path()).with_ownership_table(&table);

        assert!(
            probe.probe_socket().is_live(),
            "the keeper must be genuinely reachable for this case to mean anything"
        );
        let refusal = supervised_restart_gate(&probe).expect_err("ownership is absent");
        assert_eq!(refusal.token(), "no_keeper_ownership");
        assert!(
            refusal.banner().contains("does not own new sessions"),
            "the banner must name ownership: {}",
            refusal.banner()
        );
        let _ = server.join().expect("fake keeper");
    }

    /// An absent table is the common case for a user who never enabled the keeper, and it must read
    /// as "not owned" rather than as an error that opens the gate.
    #[cfg(unix)]
    #[test]
    fn a_missing_ownership_table_is_not_ownership() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join(KEEPER_SOCKET_NAME);
        let server = serve_handshakes(&socket, 2, |request| hello_ack(request, ack_payload()));
        let probe = KeeperSocketProbe::new(dir.path())
            .with_ownership_table(dir.path().join("absent-pty-ownership.json"));

        assert!(probe.probe_socket().is_live());
        assert!(supervised_restart_gate(&probe).is_err());
        let _ = server.join().expect("fake keeper");
    }

    /// A same-uid socket that accepts the connection and then says nothing is exactly what the old
    /// connect-only probe mistook for a keeper. It must never open the gate, and must not hang.
    #[cfg(unix)]
    #[test]
    fn a_silent_same_uid_listener_never_opens_the_gate() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join(KEEPER_SOCKET_NAME);
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let (release, released) = std::sync::mpsc::channel::<()>();
        let holder = std::thread::spawn(move || {
            let (stream, _) = listener.accept().expect("probe connects");
            // Held until the probe gives up, so the refusal is a deadline and not an early EOF.
            let _ = released.recv();
            drop(stream);
        });
        let probe = KeeperSocketProbe::new(dir.path());
        let started = Instant::now();
        let status = probe.probe_socket();
        let elapsed = started.elapsed();
        let _ = release.send(());
        holder.join().expect("silent listener");

        let detail = absent_detail(status);
        assert!(
            detail.contains("did not send its header"),
            "a listener that never answers must be refused by the deadline: {detail}"
        );
        assert!(
            elapsed < KEEPER_PROBE_TIMEOUT * 8,
            "the probe must be bounded, took {elapsed:?}"
        );
        assert!(supervised_restart_gate(&probe).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_keeper_of_another_protocol_major_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join(KEEPER_SOCKET_NAME);
        let server = serve_handshakes(&socket, 1, |request| {
            let mut payload = ack_payload();
            payload["protocolMajor"] = serde_json::json!(2);
            hello_ack(request, payload)
        });
        let probe = KeeperSocketProbe::new(dir.path());

        let detail = absent_detail(probe.probe_socket());
        server.join().expect("fake keeper");
        assert!(
            detail.contains("protocol major 2"),
            "an incompatible generation must be named, not trusted: {detail}"
        );
        assert!(supervised_restart_gate(&probe).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn an_answer_correlated_to_another_request_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join(KEEPER_SOCKET_NAME);
        let server = serve_handshakes(&socket, 1, |request| {
            use crate::daemon::keeper_client::{KeeperFrame, KeeperKind};

            KeeperFrame::new(
                KeeperKind::HelloAck,
                "11111111-1111-4111-8111-111111111111",
                request.frame_sequence + 1,
                ack_payload(),
            )
            .encode()
        });
        let probe = KeeperSocketProbe::new(dir.path());

        let detail = absent_detail(probe.probe_socket());
        server.join().expect("fake keeper");
        assert!(
            detail.contains("outstanding"),
            "an uncorrelated answer is not this keeper's: {detail}"
        );
        assert!(supervised_restart_gate(&probe).is_err());
    }

    /// The keeper names its pid, so the kernel's peer pid can contradict a process that merely
    /// pretends to be the keeper.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn a_keeper_claiming_another_pid_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join(KEEPER_SOCKET_NAME);
        let server = serve_handshakes(&socket, 1, |request| {
            let mut payload = ack_payload();
            payload["pid"] = serde_json::json!(std::process::id() + 1);
            hello_ack(request, payload)
        });
        let probe = KeeperSocketProbe::new(dir.path());

        let detail = absent_detail(probe.probe_socket());
        server.join().expect("fake keeper");
        assert!(
            detail.contains("kernel reports"),
            "the reported pid must agree with the kernel's peer pid: {detail}"
        );
        assert!(supervised_restart_gate(&probe).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_handshake_answer_without_a_pid_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join(KEEPER_SOCKET_NAME);
        let server = serve_handshakes(&socket, 1, |request| {
            let mut payload = ack_payload();
            payload.as_object_mut().unwrap().remove("pid");
            hello_ack(request, payload)
        });
        let probe = KeeperSocketProbe::new(dir.path());

        let detail = absent_detail(probe.probe_socket());
        server.join().expect("fake keeper");
        assert!(
            detail.contains("no usable pid"),
            "a keeper that cannot be identified is not live: {detail}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_refused_handshake_keeps_the_gate_closed() {
        use crate::daemon::keeper_client::{KeeperFrame, KeeperKind};

        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join(KEEPER_SOCKET_NAME);
        let server = serve_handshakes(&socket, 1, |request| {
            KeeperFrame::new(
                KeeperKind::Error,
                request.request_id.clone(),
                request.frame_sequence,
                serde_json::json!({
                    "code": "BAD_FRAME",
                    "message": "protocol major 7 is not supported",
                }),
            )
            .encode()
        });
        let probe = KeeperSocketProbe::new(dir.path());

        let detail = absent_detail(probe.probe_socket());
        server.join().expect("fake keeper");
        assert!(
            detail.contains("protocol major 7"),
            "the keeper's own refusal must reach the log verbatim: {detail}"
        );
        assert!(supervised_restart_gate(&probe).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_socket_that_answers_garbage_is_not_a_live_keeper() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join(KEEPER_SOCKET_NAME);
        let server = serve_handshakes(&socket, 1, |request| {
            let mut bytes = hello_ack(request, ack_payload());
            bytes[0..4].copy_from_slice(b"XXXX");
            bytes
        });
        let probe = KeeperSocketProbe::new(dir.path());

        let detail = absent_detail(probe.probe_socket());
        server.join().expect("fake keeper");
        assert!(
            detail.contains("magic"),
            "a non-keeper must be refused by framing, not trusted: {detail}"
        );
    }

    #[test]
    fn an_unrecorded_signal_death_is_abnormal_and_exit_zero_is_not() {
        for signal in [11, 9, 15] {
            assert_eq!(
                classify_exit(ExitObservation::signaled(signal), DeliberateExit::None),
                ExitKind::Abnormal,
                "signal {signal} is a crash"
            );
        }
        assert_eq!(
            classify_exit(ExitObservation::code(0), DeliberateExit::None),
            ExitKind::Intentional(DeliberateExit::CleanExit)
        );
        assert_eq!(
            classify_exit(ExitObservation::code(2), DeliberateExit::None),
            ExitKind::Abnormal
        );
        assert_eq!(
            classify_exit(
                ExitObservation::code(0),
                DeliberateExit::RequestedUpgrade
            ),
            ExitKind::Intentional(DeliberateExit::RequestedUpgrade)
        );
    }

    #[test]
    fn backoff_doubles_and_is_capped() {
        let policy = RestartPolicy {
            base_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(10),
            ..RestartPolicy::default()
        };
        assert_eq!(policy.backoff(1), Duration::from_secs(1));
        assert_eq!(policy.backoff(2), Duration::from_secs(2));
        assert_eq!(policy.backoff(3), Duration::from_secs(4));
        assert_eq!(policy.backoff(4), Duration::from_secs(8));
        assert_eq!(policy.backoff(5), Duration::from_secs(10));
        assert_eq!(policy.backoff(64), Duration::from_secs(10));
    }

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<std::ffi::OsString> {
        let map: std::collections::HashMap<String, std::ffi::OsString> = pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), std::ffi::OsString::from(*value)))
            .collect();
        move |key| map.get(key).cloned()
    }

    /// The QA knob shortens only `base_backoff`, and only inside an isolated runtime.
    #[test]
    fn only_an_isolated_runtime_may_shorten_the_base_backoff() {
        let production = RestartPolicy::default();

        let qa = RestartPolicy::resolve(&env_of(&[
            ("FERRYX_RUNTIME_DIR", "/tmp/ferryx-qa-fixture"),
            (QA_BACKOFF_ENV, "5"),
        ]));
        assert_eq!(qa.base_backoff, Duration::from_millis(5), "the harness knob applies");
        assert_eq!(qa.max_restarts, production.max_restarts, "the budget is never tunable");
        assert_eq!(qa.window, production.window, "the window is never tunable");
        assert_eq!(qa.max_backoff, production.max_backoff, "the cap is never tunable");
        assert_eq!(
            (1..=6).map(|attempt| qa.backoff(attempt)).collect::<Vec<_>>(),
            vec![
                Duration::from_millis(5),
                Duration::from_millis(10),
                Duration::from_millis(20),
                Duration::from_millis(40),
                Duration::from_millis(80),
                Duration::from_millis(160),
            ],
            "the harness still measures a real doubling sequence"
        );

        for (label, env) in [
            ("no isolated runtime", env_of(&[(QA_BACKOFF_ENV, "5")])),
            ("an empty override", env_of(&[("FERRYX_RUNTIME_DIR", "")])),
            (
                "a zero",
                env_of(&[("FERRYX_DATA_DIR", "/tmp/fixture"), (QA_BACKOFF_ENV, "0")]),
            ),
            (
                "nonsense",
                env_of(&[("FERRYX_SESSION_DIR", "/tmp/fixture"), (QA_BACKOFF_ENV, "fast")]),
            ),
            ("an absent knob", env_of(&[("FERRYX_RUNTIME_DIR", "/tmp/fixture")])),
        ] {
            assert_eq!(
                RestartPolicy::resolve(&env).base_backoff,
                production.base_backoff,
                "{label} must fall back to the production backoff"
            );
        }
    }

    /// The refusal banner is retrievable verbatim through `latest_banner`.
    #[test]
    fn the_recorded_banner_is_read_back_verbatim() {
        let dir = tempfile::tempdir().unwrap();
        let log = RestartLog::at(dir.path().join(RESTART_LOG_NAME));
        assert_eq!(log.latest_banner(), None, "an absent log carries no banner");

        let banner = RestartRefusal::NoLiveKeeper {
            detail: "no keeper socket at /tmp/x/keeper.sock".to_string(),
        }
        .banner();
        log.record_gate(&banner, false).unwrap();
        log.record_stop(&StopReason::NoLiveKeeper, 0).unwrap();
        log.record_banner(&banner).unwrap();
        log.record_stop(&StopReason::RestartLimitReached, 5).unwrap();

        assert_eq!(
            log.latest_banner(),
            Some(banner),
            "the newest banner line wins and is not reworded by the reader"
        );
    }

    #[cfg(unix)]
    #[test]
    fn handover_successor_is_admitted_without_consuming_request_and_rejects_foreign_or_expired() {
        use crate::daemon::handover_request::{request_path, write_request, HandoverRequest};

        let dir = tempfile::tempdir().unwrap();
        let runtime = dir.path();
        let _predecessor_lock = SupervisorLock::acquire(runtime).unwrap();

        let key = "2026.1127.2-deadbeef";
        let req = HandoverRequest::new(
            runtime.join("legacy-4242.sock"),
            4242,
            1_000,
            7,
            key.to_string(),
            runtime.join("ferryx"),
            5_000,
        );
        write_request(runtime, &req).unwrap();

        let alive = |pid: u32| (pid == 4242).then_some(1_000);
        let dead = |_pid: u32| None;

        // Valid request for this generation admits the handover successor and leaves the file
        // intact for the child `--daemon` to consume via `take_startup_request`.
        assert!(ProcessHost::has_authorized_handover_request(
            runtime,
            Some(key),
            6_000,
            &alive,
        ));
        assert!(
            request_path(runtime).exists(),
            "supervisor must not consume handover-request.json before spawning --daemon"
        );

        // Wrong generation, expired request, or dead predecessor must still be refused.
        assert!(!ProcessHost::has_authorized_handover_request(
            runtime,
            Some("2026.1127.2-other"),
            6_000,
            &alive,
        ));
        assert!(!ProcessHost::has_authorized_handover_request(
            runtime,
            Some(key),
            req.expires_at_unix_ms,
            &alive,
        ));
        assert!(!ProcessHost::has_authorized_handover_request(
            runtime,
            Some(key),
            6_000,
            &dead,
        ));
    }

    #[cfg(unix)]
    #[test]
    fn handover_successor_reclaims_supervisor_lock_after_predecessor_retires() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = dir.path();
        let predecessor_lock = SupervisorLock::acquire(runtime).unwrap();

        let mut successor = ProcessHost {
            exe: runtime.join("ferryx"),
            runtime_dir: runtime.to_path_buf(),
            child: None,
            claim: None,
        };
        // While predecessor holds the lock, try_reclaim_supervisor_lock stays None.
        successor.try_reclaim_supervisor_lock();
        assert!(successor.claim.is_none());

        // Once predecessor supervisor retires and drops its lock, successor claims it and blocks
        // any third supervisor from taking ownership.
        drop(predecessor_lock);
        successor.try_reclaim_supervisor_lock();
        assert!(successor.claim.is_some());
        assert!(matches!(
            SupervisorLock::acquire(runtime),
            Err(SupervisorClaimError::AlreadyRunning { .. })
        ));
    }
}
