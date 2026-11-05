//! Refuses owner-mutating supervisor requests from processes running inside Ferryx panes.
//!
//! The owner socket admits any same-uid peer, including a worker agent in one of this daemon's
//! panes, and the CLI's `FERRYX_SESSION_ID` guard is advisory (`env -u`, or writing to the
//! socket directly). So the daemon reads the peer's pid from the kernel at accept time, binds
//! it to the process start time, and on every gated request checks three signals against the
//! daemon's pane children:
//! 1. the peer's session id: a pane child calls `setsid`, so every process it starts shares
//!    that session unless it calls `setsid` itself; this survives reparenting (`(cli &)`,
//!    `nohup`, a double fork to pid 1 or a subreaper);
//! 2. the peer's controlling terminal: a pane's pty, kept across reparenting and by
//!    `sudo`-style children of another uid;
//! 3. the parent chain up to pid 1.
//!
//! The pane children live in a [`PaneRegistry`] the PTY manager keeps as panes spawn, are
//! adopted by handover and exit, so a gated request never rebuilds it. The parent chain is
//! walked once at accept and kept on the connection ([`Ancestry`], each ancestor bound to its
//! start time); a request re-reads only the peer (start time, session, terminal) and walks again
//! only when the kept chain ended unfinished. [`classify`] names the pane session a peer is in.
//!
//! Fail closed: a peer whose pid or start time cannot be read, whose start time changed since
//! accept (the pid was reused), or an ancestor that vanished mid-walk (the chain changed under
//! the walk) is refused. An ancestor that exists but cannot be read (another uid's process,
//! e.g. a root sshd, including one `hidepid` hides from /proc, told apart from an exited one
//! by `kill(pid, 0)`) ends the walk; pane descendants of another uid are still caught by the
//! session and terminal signals above.
//!
//! Not caught: a pane process that both calls `setsid` and gives up the pane's terminal and
//! is then reparented away from the pane. Such a process is a deliberate escape and the
//! limit of what a same-uid owner socket can tell; the request still needs the owner's
//! authority on the hub side.
//!
//! What is gated (see `server.rs`): `SupervisorRequestV1` (including every write
//! `ferryx-cli mcp` forwards: submit, adopt, a message with work, goal control, memory), a
//! mutating `SupervisorDispatch` and the supervisor-owned branch of `SupervisorSetPaused`. The pre-existing lane-goal (`SupervisorGoal`) and legacy lane pause
//! requests keep their semantics for the owner's own shell in a Ferryx pane; a supervisor
//! goal's lane already refuses them with `SUPERVISOR_OWNED`.
//!
//! POSIX: Linux reads `SO_PEERCRED` and `/proc/<pid>/stat`; macOS reads `LOCAL_PEERPID`,
//! `proc_pidinfo(PROC_PIDTBSDINFO)` and `getsid`. Every read is a blocking syscall and runs
//! on a blocking thread, never on a Tokio worker.
//!
//! Windows: the daemon listens on loopback TCP, which carries no peer pid, and the transport
//! token only proves the peer can read a file a pane process of the same user can read too.
//! The desktop app reaches the daemon over the same TCP and token, so neither it nor the CLI
//! or `ferryx-cli mcp` can be told apart from a worker pane. A `TokenAuthenticated`
//! connection is therefore refused every gated request with `FORBIDDEN`, like an `Unknown`
//! peer: on Windows the supervisor cannot be mutated over the daemon socket (not from the
//! desktop app either) until the transport can prove the owner's origin. Reads stay served.
use std::collections::HashMap;

/// How deep a parent chain is followed before the walk gives up (and refuses).
const MAX_ANCESTRY: usize = 128;

/// The wire code a refused peer gets.
pub const FORBIDDEN: &str = "FORBIDDEN";

/// A peer process as the kernel reported it at accept: its pid and that process's start time,
/// so a later check notices a reused pid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerProcess {
    pub pid: u32,
    pub start: u64,
}

/// What the daemon knows about one connection's peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerOrigin {
    /// A process connected; its pid and start time were read at accept.
    Process(PeerProcess),
    /// The transport carries no peer pid (in-process pairs in tests, a platform without peer
    /// credentials, or a peer that exited before accept finished). Gated requests are refused.
    Unknown,
    /// The peer presented the transport token but its process is unknown (Windows loopback
    /// TCP, documented above). Gated requests are refused.
    TokenAuthenticated,
}

/// One process as the checks need it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcInfo {
    pub ppid: u32,
    /// Process group; `None` when unknown.
    pub pgid: Option<u32>,
    pub session: Option<u32>,
    /// Controlling terminal device; `None` when the process has none.
    pub tty: Option<u64>,
    pub start: u64,
}

/// Why a process could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcError {
    /// It does not exist (any more).
    Vanished,
    /// It exists but this process may not read it.
    Unreadable,
}

/// Reads processes. [`SystemProcs`] is the kernel; tests use a table.
pub trait ProcTable {
    fn info(&self, pid: u32) -> Result<ProcInfo, ProcError>;
}

/// The kernel's process table.
pub struct SystemProcs;

impl ProcTable for SystemProcs {
    fn info(&self, pid: u32) -> Result<ProcInfo, ProcError> {
        proc_info(pid)
    }
}

/// The pid of the peer on a connected unix socket.
#[cfg(unix)]
pub fn peer_pid(fd: std::os::unix::io::RawFd) -> Option<u32> {
    #[cfg(target_os = "linux")]
    {
        let mut cred = libc::ucred {
            pid: 0,
            uid: 0,
            gid: 0,
        };
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        // SAFETY: cred and len are valid for the duration of the call.
        let ok = unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&mut cred as *mut libc::ucred).cast(),
                &mut len,
            )
        } == 0;
        (ok && cred.pid > 0).then_some(cred.pid as u32)
    }
    #[cfg(target_os = "macos")]
    {
        let mut pid: libc::pid_t = 0;
        let mut len = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
        // SAFETY: pid and len are valid for the duration of the call.
        let ok = unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_LOCAL,
                libc::LOCAL_PEERPID,
                (&mut pid as *mut libc::pid_t).cast(),
                &mut len,
            )
        } == 0;
        (ok && pid > 0).then_some(pid as u32)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = fd;
        None
    }
}

/// The peer identity to keep for a connection: its pid bound to its start time, or `Unknown`.
pub fn identify(pid: Option<u32>) -> PeerOrigin {
    match pid.map(|pid| (pid, proc_info(pid))) {
        Some((pid, Ok(info))) => PeerOrigin::Process(PeerProcess {
            pid,
            start: info.start,
        }),
        _ => PeerOrigin::Unknown,
    }
}

/// One process from the kernel.
pub fn proc_info(pid: u32) -> Result<ProcInfo, ProcError> {
    #[cfg(target_os = "linux")]
    {
        let stat = match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(stat) => stat,
            // Under `hidepid` another uid's process has no /proc entry for us, so ENOENT alone
            // does not mean it exited: `kill(pid, 0)` (not affected by hidepid) tells them apart.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(absent_or_hidden(pid))
            }
            Err(_) => return Err(ProcError::Unreadable),
        };
        // Fields after the command name: index n-3 is field n of proc(5).
        let (_, rest) = stat.rsplit_once(')').ok_or(ProcError::Unreadable)?;
        let fields: Vec<&str> = rest.split_whitespace().collect();
        let field = |index: usize| {
            fields
                .get(index)
                .and_then(|value| value.parse::<i64>().ok())
        };
        let ppid = field(1).ok_or(ProcError::Unreadable)?;
        let pgid = field(2).ok_or(ProcError::Unreadable)?;
        let session = field(3).ok_or(ProcError::Unreadable)?;
        let tty = field(4).ok_or(ProcError::Unreadable)?;
        let start = field(19).ok_or(ProcError::Unreadable)?;
        Ok(ProcInfo {
            ppid: u32::try_from(ppid).unwrap_or(0),
            pgid: u32::try_from(pgid).ok().filter(|pgid| *pgid > 0),
            session: u32::try_from(session).ok().filter(|session| *session > 0),
            tty: u64::try_from(tty).ok().filter(|tty| *tty != 0),
            start: u64::try_from(start).unwrap_or(0),
        })
    }
    #[cfg(target_os = "macos")]
    {
        // SAFETY: a zeroed proc_bsdinfo is a valid out-buffer of exactly `size` bytes.
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of_val(&info) as i32;
        // SAFETY: info is a valid buffer of `size` bytes for the call.
        let read = unsafe {
            libc::proc_pidinfo(
                pid as i32,
                libc::PROC_PIDTBSDINFO,
                0,
                (&mut info as *mut libc::proc_bsdinfo).cast(),
                size,
            )
        };
        if read != size {
            let errno = std::io::Error::last_os_error().raw_os_error();
            return Err(if errno == Some(libc::ESRCH) {
                ProcError::Vanished
            } else {
                ProcError::Unreadable
            });
        }
        // SAFETY: getsid only reads the process table.
        let session = unsafe { libc::getsid(pid as libc::pid_t) };
        const NODEV: u32 = u32::MAX;
        Ok(ProcInfo {
            ppid: info.pbi_ppid,
            pgid: (info.pbi_pgid > 0).then_some(info.pbi_pgid),
            session: (session > 0).then_some(session as u32),
            tty: (info.e_tdev != NODEV && info.e_tdev != 0).then_some(u64::from(info.e_tdev)),
            start: info
                .pbi_start_tvsec
                .saturating_mul(1_000_000)
                .saturating_add(info.pbi_start_tvusec),
        })
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::{
            Foundation::{CloseHandle, FILETIME, INVALID_HANDLE_VALUE},
            System::{
                Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS},
                Threading::{OpenProcess, GetProcessTimes, PROCESS_QUERY_LIMITED_INFORMATION},
            },
        };
        // Hold a process handle while taking the snapshot so pid reuse cannot substitute a
        // different process between the parent and creation-time reads.
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if process.is_null() {
            return Err(if std::io::Error::last_os_error().raw_os_error() == Some(87) {
                ProcError::Vanished
            } else { ProcError::Unreadable });
        }
        let result = (|| {
            let mut creation: FILETIME = unsafe { std::mem::zeroed() };
            let mut exit = creation;
            let mut kernel = creation;
            let mut user = creation;
            if unsafe { GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user) } == 0 {
                return Err(ProcError::Unreadable);
            }
            let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
            if snapshot == INVALID_HANDLE_VALUE { return Err(ProcError::Unreadable); }
            let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut found = None;
            let mut more = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
            while more {
                if entry.th32ProcessID == pid { found = Some(entry.th32ParentProcessID); break; }
                more = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
            }
            unsafe { CloseHandle(snapshot) };
            Ok(ProcInfo {
                ppid: found.ok_or(ProcError::Vanished)?, pgid: None, session: None, tty: None,
                start: (u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime),
            })
        })();
        unsafe { CloseHandle(process) };
        result
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = pid;
        Err(ProcError::Unreadable)
    }
}

/// A pid without a readable /proc entry: `Vanished` only when the kernel says no such process
/// (`ESRCH`); a live process hidden by `hidepid` (`EPERM`, or signalable but unlisted) is
/// `Unreadable`, which ends an ancestor walk like any other uid's process instead of refusing
/// the owner for good.
#[cfg(target_os = "linux")]
fn absent_or_hidden(pid: u32) -> ProcError {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return ProcError::Vanished;
    };
    // SAFETY: signal 0 only checks that the process exists and may be signalled.
    let alive = unsafe { libc::kill(pid, 0) } == 0;
    match (alive, std::io::Error::last_os_error().raw_os_error()) {
        (true, _) => ProcError::Unreadable,
        (false, Some(libc::ESRCH)) => ProcError::Vanished,
        (false, _) => ProcError::Unreadable,
    }
}

/// One pane child as the daemon recorded it when the pane was spawned or adopted: its pid,
/// process group, session, controlling terminal and start time (`None` where the kernel did not
/// tell). A pane child is its own session leader, so its pid also names its session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaneEntry {
    pub pid: u32,
    pub pgid: Option<u32>,
    pub sid: Option<u32>,
    pub tty: Option<u64>,
    pub start: Option<u64>,
}

impl PaneEntry {
    /// The pane child `pid` as `table` reports it now.
    pub fn read(pid: u32, table: &impl ProcTable) -> Self {
        let info = table.info(pid).ok();
        PaneEntry {
            pid,
            pgid: info.and_then(|info| info.pgid),
            sid: info.and_then(|info| info.session),
            tty: info.and_then(|info| info.tty),
            start: info.map(|info| info.start),
        }
    }
}

/// The daemon's pane children by session id, kept as panes are spawned, adopted (handover
/// import) and exit, so a gated request never rebuilds the pane set from the process table.
#[derive(Debug, Default)]
pub struct PaneRegistry {
    panes: parking_lot::RwLock<HashMap<String, PaneEntry>>,
}

impl PaneRegistry {
    /// Records the pane child `pid` of `session_id`, read from `table`. Blocking when `table`
    /// is the kernel.
    pub fn insert(&self, session_id: &str, pid: u32, table: &impl ProcTable) {
        let entry = PaneEntry::read(pid, table);
        self.panes.write().insert(session_id.to_owned(), entry);
    }

    /// Forgets the pane of `session_id` (it exited, was closed or was handed over).
    pub fn remove(&self, session_id: &str) -> Option<PaneEntry> {
        self.panes.write().remove(session_id)
    }

    pub fn entry(&self, session_id: &str) -> Option<PaneEntry> {
        self.panes.read().get(session_id).copied()
    }

    pub fn len(&self) -> usize {
        self.panes.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.panes.read().is_empty()
    }

    fn find(&self, matches: impl Fn(&PaneEntry) -> bool) -> Option<(String, PaneEntry)> {
        self.panes
            .read()
            .iter()
            .find(|(_, entry)| matches(entry))
            .map(|(session_id, entry)| (session_id.clone(), *entry))
    }
}

/// How the accept-time parent walk ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WalkEnd {
    /// Reached pid 1, a self-parented process or another uid's process.
    Complete,
    /// The ancestor `pid` exited mid-walk.
    Changed { pid: u32 },
    /// Deeper than [`MAX_ANCESTRY`].
    TooDeep,
}

/// A connection peer's parent chain, walked once at accept and kept on the connection: every
/// ancestor's pid with its start time (`None` for the last one when it could not be read).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ancestry {
    peer: PeerProcess,
    chain: Vec<(u32, Option<u64>)>,
    end: WalkEnd,
}

impl Ancestry {
    /// Walks the parent chain of `peer`, whose own record is `info`.
    fn walk(peer: PeerProcess, info: &ProcInfo, table: &impl ProcTable) -> Self {
        let mut chain = Vec::new();
        let mut current = info.ppid;
        let mut child_start = info.start;
        for _ in 0..MAX_ANCESTRY {
            if current <= 1 {
                return Ancestry { peer, chain, end: WalkEnd::Complete };
            }
            match table.info(current) {
                Ok(next) => {
                    if next.start > child_start {
                        chain.push((current, None));
                        return Ancestry { peer, chain, end: WalkEnd::Changed { pid: current } };
                    }
                    child_start = next.start;
                    chain.push((current, Some(next.start)));
                    if next.ppid == current {
                        return Ancestry { peer, chain, end: WalkEnd::Complete };
                    }
                    current = next.ppid;
                }
                // Another uid's process (a root sshd or login): pane descendants of another uid
                // are caught by session and terminal.
                Err(ProcError::Unreadable) => {
                    chain.push((current, None));
                    return Ancestry { peer, chain, end: WalkEnd::Complete };
                }
                Err(ProcError::Vanished) => {
                    chain.push((current, None));
                    return Ancestry { peer, chain, end: WalkEnd::Changed { pid: current } };
                }
            }
        }
        Ancestry { peer, chain, end: WalkEnd::TooDeep }
    }

    /// The parent chain of the connection's peer, read at accept; `None` for a peer that is not
    /// a readable process (the request-time check refuses it anyway).
    pub fn at_accept(origin: PeerOrigin, table: &impl ProcTable) -> Option<Self> {
        let PeerOrigin::Process(peer) = origin else {
            return None;
        };
        let info = table.info(peer.pid).ok().filter(|info| info.start == peer.start)?;
        Some(Self::walk(peer, &info, table))
    }
}

/// Who a connection's peer is, as far as owner decisions go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerClass {
    /// A process outside every pane: the owner's.
    Owner,
    /// A process inside the pane of `session_id`; `why` names the signal that matched.
    Pane { session_id: String, why: String },
    /// See [`PeerOrigin::Unknown`].
    Unknown,
    /// See [`PeerOrigin::TokenAuthenticated`].
    TokenAuthenticated,
}

/// Classifies a connection's peer against the pane registry. The peer's start time is re-read
/// on every call (a reused pid is refused); its session and terminal come from that same read;
/// the parent chain comes from `ancestry`, walked at accept, and is walked again only when it is
/// missing, belongs to another process or ended unfinished (an ancestor exited mid-walk), so a
/// transient refusal is retried like before. `Err` is a fail-closed refusal reason.
pub fn classify(
    origin: PeerOrigin,
    ancestry: &mut Option<Ancestry>,
    panes: &PaneRegistry,
    table: &impl ProcTable,
) -> Result<PeerClass, String> {
    classify_with_cutoff(origin, ancestry, panes, table, None)
}

fn classify_with_cutoff(
    origin: PeerOrigin,
    ancestry: &mut Option<Ancestry>,
    panes: &PaneRegistry,
    table: &impl ProcTable,
    owner_cutoff: Option<u64>,
) -> Result<PeerClass, String> {
    let peer = match origin {
        PeerOrigin::TokenAuthenticated => return Ok(PeerClass::TokenAuthenticated),
        PeerOrigin::Unknown => return Ok(PeerClass::Unknown),
        PeerOrigin::Process(peer) => peer,
    };
    let pid = peer.pid;
    let info = match table.info(pid) {
        Ok(info) if info.start == peer.start => info,
        Ok(_) => return Err(format!("process {pid} is not the process that connected (its pid was reused); it may not change the supervisor")),
        Err(_) => return Err(format!("process {pid} could not be read; it may not change the supervisor")),
    };
    let pane = |session_id: String, why: String| Ok(PeerClass::Pane { session_id, why });
    if let Some((session_id, _)) = panes.find(|entry| entry.pid == pid) {
        return pane(session_id, format!("pane process {pid}"));
    }
    if let Some(session) = info.session {
        if let Some((session_id, _)) =
            panes.find(|entry| entry.pid == session || entry.sid == Some(session))
        {
            return pane(session_id, format!("pane session {session}"));
        }
    }
    if let Some(tty) = info.tty {
        if let Some((session_id, _)) = panes.find(|entry| entry.tty == Some(tty)) {
            return pane(session_id, format!("pane terminal {tty}"));
        }
    }
    let fresh = match ancestry.as_ref() {
        Some(cached) => cached.peer != peer || cached.end != WalkEnd::Complete,
        None => true,
    };
    if fresh {
        *ancestry = Some(Ancestry::walk(peer, &info, table));
    }
    let walked = ancestry.as_ref().expect("walked above");
    for &(ancestor, start) in &walked.chain {
        // A pane whose start time differs is a later process that reused an ancestor's pid.
        let descends = |entry: &PaneEntry| {
            entry.pid == ancestor
                && match (entry.start, start) {
                    (Some(recorded), Some(seen)) => recorded == seen,
                    _ => true,
                }
        };
        if let Some((session_id, _)) = panes.find(descends) {
            return pane(session_id, format!("pane process {ancestor}"));
        }
    }
    match walked.end {
        WalkEnd::Complete => Ok(PeerClass::Owner),
        WalkEnd::Changed { pid: current } => {
            {
                #[cfg(windows)]
                let owner_cutoff = owner_cutoff.or_else(|| table.info(std::process::id()).ok().map(|info| info.start));
                let panes = panes.panes.read();
                if let Some(cutoff) = owner_cutoff.filter(|_| panes.values().all(|entry| entry.start.is_some())) {
                    let cutoff = panes.values().filter_map(|entry| entry.start).fold(cutoff, u64::min);
                    if predates_restricted_roots(peer, &walked.chain, cutoff) {
                        return Ok(PeerClass::Owner);
                    }
                }
            }
            Err(format!(
                "the parent chain of process {pid} changed while it was checked (process {current} exited); retry"
            ))
        },
        WalkEnd::TooDeep => Err(format!(
            "the parent chain of process {pid} is deeper than {MAX_ANCESTRY}"
        )),
    }
}

fn predates_restricted_roots(peer: PeerProcess, chain: &[(u32, Option<u64>)], cutoff: u64) -> bool {
    peer.start < cutoff || chain.iter().any(|(_, start)| start.is_some_and(|start| start < cutoff))
}

/// Why a peer may not mutate supervisor state, or `None` when it may (see [`classify`]).
pub fn refusal(
    origin: PeerOrigin,
    ancestry: &mut Option<Ancestry>,
    panes: &PaneRegistry,
    table: &impl ProcTable,
) -> Option<String> {
    match classify(origin, ancestry, panes, table) {
        Ok(PeerClass::Owner) => None,
        Ok(PeerClass::TokenAuthenticated) => Some("this connection proved only the transport token, which a Ferryx pane process can read too, and the daemon cannot identify its process, so it may not change the supervisor".into()),
        Ok(PeerClass::Unknown) => Some("the daemon cannot identify this connection's process, so it may not change the supervisor".into()),
        Ok(PeerClass::Pane { why, .. }) => {
            let pid = match origin {
                PeerOrigin::Process(peer) => peer.pid,
                _ => 0,
            };
            Some(format!("process {pid} runs inside a Ferryx pane ({why}); a pane process may not change the supervisor"))
        }
        Err(reason) => Some(reason),
    }
}
/// A spawned pane is in the registry while it runs and leaves it when it exits.
#[cfg(unix)]
#[cfg(test)]
#[tokio::test]
async fn registry_drops_exited_pane() {
    let manager = crate::terminal::PtyManager::new();
    let mut cmd = portable_pty::CommandBuilder::new("/bin/sh");
    cmd.arg("-c");
    cmd.arg("read line; exit 0");
    let (session_id, _rx) = manager.spawn(cmd, 80, 24).expect("spawn shell");
    let pid = manager.get_session(&session_id).and_then(|session| session.pid());
    let entry = manager.pane_registry().entry(&session_id).expect("registered on spawn");
    assert_eq!(Some(entry.pid), pid);
    assert_eq!(entry.sid, pid, "a pane child leads its own session");
    assert!(entry.tty.is_some() && entry.start.is_some(), "{entry:?}");

    let removal = manager.removal_notify();
    manager.write_input(&session_id, b"go\n").expect("release the shell");
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        while manager.has_session(&session_id) {
            let notified = removal.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if !manager.has_session(&session_id) {
                break;
            }
            notified.await;
        }
    })
    .await
    .expect("the shell's exit is observed");
    assert_eq!(manager.pane_registry().entry(&session_id), None, "dropped on exit");
    assert!(manager.pane_registry().is_empty());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    #[test]
    fn exited_windows_ssh_ancestor_needs_positive_pre_pane_identity() {
        assert!(predates_restricted_roots(PeerProcess { pid: 50, start: 100 }, &[], 200));
        assert!(predates_restricted_roots(PeerProcess { pid: 60, start: 300 }, &[(50, Some(100)), (40, None)], 200));
        assert!(!predates_restricted_roots(PeerProcess { pid: 60, start: 300 }, &[(50, Some(250)), (40, None)], 200));
        assert!(!predates_restricted_roots(PeerProcess { pid: 60, start: 300 }, &[(40, None)], 200));
        assert!(!predates_restricted_roots(PeerProcess { pid: 60, start: 200 }, &[], 200));
    }

    #[test]
    fn verify_win_peer_11476_exited_ssh_ancestor_932_owner_and_pane_cases() {
        // verify-win/pipe-final-qa.log records these pids and the vanished ancestor, not
        // creation times. Times below are explicit scenarios, not reconstructed evidence.
        let mut procs = Table { procs: HashMap::new(), unreadable: HashSet::new() };
        let info = |ppid, start| ProcInfo { ppid, pgid: None, session: None, tty: None, start };
        procs.procs.insert(11476, info(932, 100));
        procs.procs.insert(22324, info(932, 200));
        procs.procs.insert(30000, info(22324, 300));
        let panes = PaneRegistry::default();
        panes.insert("pane-a", 30000, &procs);
        let owner = PeerOrigin::Process(PeerProcess { pid: 11476, start: 100 });
        let mut ancestry = Ancestry::at_accept(owner, &procs);
        assert!(matches!(ancestry.as_ref().unwrap().end, WalkEnd::Changed { pid: 932 }));
        assert_eq!(classify_with_cutoff(owner, &mut ancestry, &panes, &procs, Some(200)), Ok(PeerClass::Owner));

        procs.procs.insert(31000, info(30000, 400));
        let descendant = PeerOrigin::Process(PeerProcess { pid: 31000, start: 400 });
        assert!(matches!(classify_with_cutoff(descendant, &mut None, &panes, &procs, Some(200)),
            Ok(PeerClass::Pane { session_id, .. }) if session_id == "pane-a"));
        procs.procs.remove(&30000);
        assert!(classify_with_cutoff(descendant, &mut None, &panes, &procs, Some(200)).is_err());

        procs.procs.get_mut(&11476).unwrap().start = 500;
        let later_owner = PeerOrigin::Process(PeerProcess { pid: 11476, start: 500 });
        assert!(classify_with_cutoff(later_owner, &mut None, &panes, &procs, Some(200)).is_err());
        procs.procs.insert(932, info(1, 600));
        assert!(classify_with_cutoff(later_owner, &mut None, &panes, &procs, Some(200)).is_err());
        assert!(classify_with_cutoff(owner, &mut ancestry, &panes, &procs, Some(200)).is_err());
    }

    #[test]
    fn exited_ancestor_owner_recovery_uses_real_classifier_without_promoting_panes() {
        let mut procs = world();
        procs.procs.remove(&50);
        let panes = panes_of(200, &procs);
        assert_eq!(classify_with_cutoff(peer(70), &mut None, &panes, &procs, Some(1000)), Ok(PeerClass::Owner));
        procs.procs.get_mut(&220).unwrap().session = None;
        procs.procs.get_mut(&220).unwrap().tty = None;
        assert!(matches!(classify_with_cutoff(peer(220), &mut None, &panes, &procs, Some(1000)), Ok(PeerClass::Pane { .. })));
        procs.procs.remove(&210);
        assert!(classify_with_cutoff(peer(220), &mut None, &panes, &procs, Some(1000)).is_err());
    }

    /// A fake process table: pid -> (ppid, session, tty); missing pids are `Vanished`, pids in
    /// `unreadable` exist but cannot be read.
    struct Table {
        procs: HashMap<u32, ProcInfo>,
        unreadable: HashSet<u32>,
    }

    impl ProcTable for Table {
        fn info(&self, pid: u32) -> Result<ProcInfo, ProcError> {
            if self.unreadable.contains(&pid) {
                return Err(ProcError::Unreadable);
            }
            self.procs.get(&pid).copied().ok_or(ProcError::Vanished)
        }
    }

    fn table(rows: &[(u32, u32, u32, Option<u64>)]) -> Table {
        Table {
            procs: rows
                .iter()
                .map(|&(pid, ppid, session, tty)| {
                    (
                        pid,
                        ProcInfo {
                            ppid,
                            pgid: Some(pid),
                            session: Some(session),
                            tty,
                            start: u64::from(pid) * 10,
                        },
                    )
                })
                .collect(),
            unreadable: HashSet::new(),
        }
    }

    /// A registry holding the pane child `pid` as session `pane-<pid>`.
    fn panes_of(pid: u32, table: &impl ProcTable) -> PaneRegistry {
        let panes = PaneRegistry::default();
        panes.insert(&format!("pane-{pid}"), pid, table);
        panes
    }

    fn peer(pid: u32) -> PeerOrigin {
        PeerOrigin::Process(PeerProcess {
            pid,
            start: u64::from(pid) * 10,
        })
    }

    /// launchd(1) -> terminal(50, session 50, tty 7) -> shell(60) -> cli(70);
    /// daemon(100) -> pane child 200 (session 200, tty 9) -> agent 210 -> cli 220.
    fn world() -> Table {
        table(&[
            (50, 1, 50, Some(7)),
            (60, 50, 50, Some(7)),
            (70, 60, 50, Some(7)),
            (100, 1, 100, None),
            (200, 100, 200, Some(9)),
            (210, 200, 200, Some(9)),
            (220, 210, 200, Some(9)),
        ])
    }

    #[test]
    fn a_pane_descendant_is_refused_and_an_outside_process_is_not() {
        let procs = world();
        let panes = panes_of(200, &procs);
        assert!(refusal(peer(220), &mut None, &panes, &procs).is_some_and(|why| why.contains("pane")));
        assert!(refusal(peer(200), &mut None, &panes, &procs).is_some());
        assert_eq!(refusal(peer(70), &mut None, &panes, &procs), None);
    }

    #[test]
    fn classify_returns_the_session_the_peer_descends_from() {
        let mut procs = world();
        let panes = panes_of(200, &procs);
        panes.insert("pane-50", 50, &procs);
        assert!(panes.remove("pane-50").is_some());
        assert_eq!(panes.len(), 1);
        // A descendant that left the pane's session and terminal is still found by its chain.
        procs.procs.insert(
            260,
            ProcInfo { ppid: 210, pgid: Some(260), session: Some(260), tty: None, start: 2600 },
        );
        for pid in [200, 220, 260] {
            match classify(peer(pid), &mut None, &panes, &procs) {
                Ok(PeerClass::Pane { session_id, .. }) => assert_eq!(session_id, "pane-200"),
                other => panic!("{pid}: {other:?}"),
            }
        }
        assert_eq!(classify(peer(70), &mut None, &panes, &procs), Ok(PeerClass::Owner));
        assert_eq!(classify(PeerOrigin::Unknown, &mut None, &panes, &procs), Ok(PeerClass::Unknown));
        assert_eq!(
            classify(PeerOrigin::TokenAuthenticated, &mut None, &panes, &procs),
            Ok(PeerClass::TokenAuthenticated)
        );
    }

    /// The chain is walked at accept and reused: an ancestor exiting later does not refuse the
    /// peer, a pane closing later lets it through, but the peer itself is re-read every time.
    #[test]
    fn the_accept_time_chain_is_reused_and_the_peer_re_read() {
        let mut procs = world();
        procs.procs.insert(
            260,
            ProcInfo { ppid: 210, pgid: Some(260), session: Some(260), tty: None, start: 2600 },
        );
        let panes = panes_of(200, &procs);
        let mut ancestry = Ancestry::at_accept(peer(260), &procs);
        assert!(ancestry.is_some());
        procs.procs.remove(&210);
        assert!(matches!(
            classify(peer(260), &mut ancestry, &panes, &procs),
            Ok(PeerClass::Pane { ref session_id, .. }) if session_id == "pane-200"
        ));
        assert!(refusal(peer(260), &mut None, &panes, &procs).is_some_and(|why| why.contains("changed")));
        panes.remove("pane-200");
        assert_eq!(classify(peer(260), &mut ancestry, &panes, &procs), Ok(PeerClass::Owner));
        // Pid reuse: 260 is now another process; the cached chain does not vouch for it.
        procs.procs.get_mut(&260).expect("peer").start = 1;
        assert!(refusal(peer(260), &mut ancestry, &panes, &procs).is_some_and(|why| why.contains("reused")));
    }

    /// A pane child whose pid an ancestor of the peer had before (the ancestor exited and the pid
    /// was reused by a new pane) is not the peer's pane.
    #[test]
    fn a_reused_ancestor_pid_is_not_a_pane() {
        let mut procs = world();
        let mut ancestry = Ancestry::at_accept(peer(70), &procs);
        // The shell 60 exits and a new pane child gets pid 60 with another start time.
        procs.procs.insert(60, ProcInfo { ppid: 100, pgid: Some(60), session: Some(60), tty: Some(11), start: 9999 });
        let panes = panes_of(60, &procs);
        assert_eq!(classify(peer(70), &mut ancestry, &panes, &procs), Ok(PeerClass::Owner));
    }

    #[test]
    fn a_reparented_pane_process_is_still_refused_by_session_or_terminal() {
        let mut procs = world();
        // `(cli &)`: the cli's parent exited and it was reparented to pid 1.
        procs.procs.insert(
            230,
            ProcInfo {
                ppid: 1,
                pgid: None,
                session: Some(200),
                tty: Some(9),
                start: 2300,
            },
        );
        let panes = panes_of(200, &procs);
        assert!(refusal(peer(230), &mut None, &panes, &procs).is_some_and(|why| why.contains("session 200")));
        // `setsid` kept the pane terminal: refused by terminal.
        procs.procs.insert(
            240,
            ProcInfo {
                ppid: 1,
                pgid: None,
                session: Some(240),
                tty: Some(9),
                start: 2400,
            },
        );
        assert!(refusal(peer(240), &mut None, &panes, &procs).is_some_and(|why| why.contains("terminal 9")));
    }

    #[test]
    fn a_reused_pid_and_an_ancestor_that_vanishes_mid_walk_are_refused() {
        let mut procs = world();
        let panes = panes_of(200, &procs);
        // The pid that connected was 70 started at 700; now 70 is another process.
        let stale = PeerOrigin::Process(PeerProcess { pid: 70, start: 1 });
        assert!(refusal(stale, &mut None, &panes, &procs).is_some_and(|why| why.contains("reused")));
        // The shell (60) exits between reading the cli and walking up: fail closed.
        procs.procs.remove(&60);
        assert!(refusal(peer(70), &mut None, &panes, &procs).is_some_and(|why| why.contains("changed")));
    }

    #[test]
    fn unknown_or_unreadable_peers_are_refused() {
        let mut procs = world();
        let panes = panes_of(200, &procs);
        assert!(refusal(PeerOrigin::Unknown, &mut None, &panes, &procs).is_some());
        assert!(
            refusal(peer(999), &mut None, &panes, &procs).is_some(),
            "a peer that is gone is refused"
        );
        procs.unreadable.insert(70);
        assert!(
            refusal(peer(70), &mut None, &panes, &procs).is_some(),
            "an unreadable peer is refused"
        );
        procs.unreadable.clear();
        // An unreadable ancestor (a root sshd) ends the walk outside the panes...
        procs.unreadable.insert(50);
        assert_eq!(refusal(peer(70), &mut None, &panes, &procs), None);
        // ...and a pane process under a root `sudo` is still caught by its session.
        procs.procs.insert(
            250,
            ProcInfo {
                ppid: 245,
                pgid: None,
                session: Some(200),
                tty: Some(9),
                start: 2500,
            },
        );
        procs.unreadable.insert(245);
        assert!(refusal(peer(250), &mut None, &panes, &procs).is_some());
    }

    /// A token alone (Windows loopback TCP) does not tell the owner from a pane process that
    /// read the token file: refused whatever the pane set, even with no panes at all.
    #[test]
    fn a_token_authenticated_peer_is_refused() {
        let procs = world();
        for panes in [PaneRegistry::default(), panes_of(200, &procs)] {
            assert!(refusal(PeerOrigin::TokenAuthenticated, &mut None, &panes, &procs)
                .is_some_and(|why| why.contains("transport token")));
        }
    }

    /// hidepid: a live process without a /proc entry for us is unreadable (the walk ends
    /// outside), and only a reaped one is vanished (the walk fails closed).
    #[cfg(target_os = "linux")]
    #[test]
    fn a_hidden_live_process_is_unreadable_and_a_reaped_one_vanished() {
        assert_eq!(
            absent_or_hidden(1),
            ProcError::Unreadable,
            "pid 1 always exists"
        );
        let mut child = std::process::Command::new("true")
            .spawn()
            .expect("spawn true");
        let pid = child.id();
        child.wait().expect("reap");
        assert_eq!(absent_or_hidden(pid), ProcError::Vanished);
    }

    #[cfg(unix)]
    #[test]
    fn the_real_process_table_is_read() {
        let me = std::process::id();
        let origin = identify(Some(me));
        assert!(matches!(origin, PeerOrigin::Process(PeerProcess { pid, .. }) if pid == me));
        let parent = proc_info(me).expect("own process").ppid;
        // This test process is not under a pane...
        assert_eq!(refusal(origin, &mut None, &PaneRegistry::default(), &SystemProcs), None);
        // ...but it is when its parent is a pane child.
        let panes = panes_of(parent, &SystemProcs);
        assert!(refusal(origin, &mut None, &panes, &SystemProcs).is_some());
    }

    #[cfg(unix)]
    #[test]
    fn the_peer_pid_of_a_socketpair_is_this_process() {
        use std::os::unix::io::AsRawFd;
        let (left, _right) = std::os::unix::net::UnixStream::pair().expect("pair");
        assert_eq!(peer_pid(left.as_raw_fd()), Some(std::process::id()));
    }
}
