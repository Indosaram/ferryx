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
//! Windows fallback: the loopback TCP transport has no peer pid. A connection that presented
//! the transport token is trusted (`TokenAuthenticated`). That is weaker than the POSIX check:
//! a pane process that reads the token file itself is not told apart.
use std::collections::HashSet;

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
    /// The transport authenticated the peer another way (Windows token transport, documented
    /// above).
    TokenAuthenticated,
}

/// One process as the checks need it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcInfo {
    pub ppid: u32,
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
        let mut cred = libc::ucred { pid: 0, uid: 0, gid: 0 };
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        // SAFETY: cred and len are valid for the duration of the call.
        let ok = unsafe {
            libc::getsockopt(fd, libc::SOL_SOCKET, libc::SO_PEERCRED, (&mut cred as *mut libc::ucred).cast(), &mut len)
        } == 0;
        return (ok && cred.pid > 0).then_some(cred.pid as u32);
    }
    #[cfg(target_os = "macos")]
    {
        let mut pid: libc::pid_t = 0;
        let mut len = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
        // SAFETY: pid and len are valid for the duration of the call.
        let ok = unsafe {
            libc::getsockopt(fd, libc::SOL_LOCAL, libc::LOCAL_PEERPID, (&mut pid as *mut libc::pid_t).cast(), &mut len)
        } == 0;
        return (ok && pid > 0).then_some(pid as u32);
    }
    #[allow(unreachable_code)]
    {
        let _ = fd;
        None
    }
}

/// The peer identity to keep for a connection: its pid bound to its start time, or `Unknown`.
pub fn identify(pid: Option<u32>) -> PeerOrigin {
    match pid.map(|pid| (pid, proc_info(pid))) {
        Some((pid, Ok(info))) => PeerOrigin::Process(PeerProcess { pid, start: info.start }),
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
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Err(absent_or_hidden(pid)),
            Err(_) => return Err(ProcError::Unreadable),
        };
        // Fields after the command name: index n-3 is field n of proc(5).
        let (_, rest) = stat.rsplit_once(')').ok_or(ProcError::Unreadable)?;
        let fields: Vec<&str> = rest.split_whitespace().collect();
        let field = |index: usize| fields.get(index).and_then(|value| value.parse::<i64>().ok());
        let ppid = field(1).ok_or(ProcError::Unreadable)?;
        let session = field(3).ok_or(ProcError::Unreadable)?;
        let tty = field(4).ok_or(ProcError::Unreadable)?;
        let start = field(19).ok_or(ProcError::Unreadable)?;
        return Ok(ProcInfo {
            ppid: u32::try_from(ppid).unwrap_or(0),
            session: u32::try_from(session).ok().filter(|session| *session > 0),
            tty: u64::try_from(tty).ok().filter(|tty| *tty != 0),
            start: u64::try_from(start).unwrap_or(0),
        });
    }
    #[cfg(target_os = "macos")]
    {
        // SAFETY: a zeroed proc_bsdinfo is a valid out-buffer of exactly `size` bytes.
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of_val(&info) as i32;
        // SAFETY: info is a valid buffer of `size` bytes for the call.
        let read = unsafe {
            libc::proc_pidinfo(pid as i32, libc::PROC_PIDTBSDINFO, 0, (&mut info as *mut libc::proc_bsdinfo).cast(), size)
        };
        if read != size {
            let errno = std::io::Error::last_os_error().raw_os_error();
            return Err(if errno == Some(libc::ESRCH) { ProcError::Vanished } else { ProcError::Unreadable });
        }
        // SAFETY: getsid only reads the process table.
        let session = unsafe { libc::getsid(pid as libc::pid_t) };
        const NODEV: u32 = u32::MAX;
        return Ok(ProcInfo {
            ppid: info.pbi_ppid,
            session: (session > 0).then_some(session as u32),
            tty: (info.e_tdev != NODEV && info.e_tdev != 0).then_some(u64::from(info.e_tdev)),
            start: info.pbi_start_tvsec.saturating_mul(1_000_000).saturating_add(info.pbi_start_tvusec),
        });
    }
    #[allow(unreachable_code)]
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

/// The daemon's pane children and the sessions and terminals they own.
#[derive(Debug, Default, Clone)]
pub struct PaneSet {
    pub pids: HashSet<u32>,
    pub sessions: HashSet<u32>,
    pub ttys: HashSet<u64>,
}

impl PaneSet {
    /// The pane children `pids`, with their sessions (a pane child is its own session leader)
    /// and controlling terminals as `table` reports them.
    pub fn of(pids: impl IntoIterator<Item = u32>, table: &impl ProcTable) -> Self {
        let mut set = PaneSet::default();
        for pid in pids {
            set.pids.insert(pid);
            set.sessions.insert(pid);
            if let Ok(info) = table.info(pid) {
                set.sessions.extend(info.session);
                set.ttys.extend(info.tty);
            }
        }
        set
    }
}

/// Why a peer may not mutate supervisor state, or `None` when it may.
pub fn refusal(origin: PeerOrigin, panes: &PaneSet, table: &impl ProcTable) -> Option<String> {
    let peer = match origin {
        PeerOrigin::TokenAuthenticated => return None,
        PeerOrigin::Unknown => {
            return Some("the daemon cannot identify this connection's process, so it may not change the supervisor".into())
        }
        PeerOrigin::Process(peer) => peer,
    };
    let pid = peer.pid;
    let info = match table.info(pid) {
        Ok(info) if info.start == peer.start => info,
        Ok(_) => return Some(format!("process {pid} is not the process that connected (its pid was reused); it may not change the supervisor")),
        Err(_) => return Some(format!("process {pid} could not be read; it may not change the supervisor")),
    };
    let inside = |why: String| Some(format!("process {pid} runs inside a Ferryx pane ({why}); a pane process may not change the supervisor"));
    if panes.pids.contains(&pid) {
        return inside(format!("pane process {pid}"));
    }
    if let Some(session) = info.session.filter(|session| panes.sessions.contains(session)) {
        return inside(format!("pane session {session}"));
    }
    if let Some(tty) = info.tty.filter(|tty| panes.ttys.contains(tty)) {
        return inside(format!("pane terminal {tty}"));
    }
    let mut current = info.ppid;
    for _ in 0..MAX_ANCESTRY {
        if current <= 1 {
            return None;
        }
        if panes.pids.contains(&current) {
            return inside(format!("pane process {current}"));
        }
        match table.info(current) {
            Ok(next) if next.ppid == current => return None,
            Ok(next) => current = next.ppid,
            // Another uid's process (a root sshd or login): pane descendants of another uid
            // were already caught by session and terminal.
            Err(ProcError::Unreadable) => return None,
            Err(ProcError::Vanished) => {
                return Some(format!(
                    "the parent chain of process {pid} changed while it was checked (process {current} exited); retry"
                ))
            }
        }
    }
    Some(format!("the parent chain of process {pid} is deeper than {MAX_ANCESTRY}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

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
                .map(|&(pid, ppid, session, tty)| (pid, ProcInfo { ppid, session: Some(session), tty, start: u64::from(pid) * 10 }))
                .collect(),
            unreadable: HashSet::new(),
        }
    }

    fn peer(pid: u32) -> PeerOrigin {
        PeerOrigin::Process(PeerProcess { pid, start: u64::from(pid) * 10 })
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
        let panes = PaneSet::of([200], &procs);
        assert!(refusal(peer(220), &panes, &procs).is_some_and(|why| why.contains("pane")));
        assert!(refusal(peer(200), &panes, &procs).is_some());
        assert_eq!(refusal(peer(70), &panes, &procs), None);
    }

    #[test]
    fn a_reparented_pane_process_is_still_refused_by_session_or_terminal() {
        let mut procs = world();
        // `(cli &)`: the cli's parent exited and it was reparented to pid 1.
        procs.procs.insert(230, ProcInfo { ppid: 1, session: Some(200), tty: Some(9), start: 2300 });
        let panes = PaneSet::of([200], &procs);
        assert!(refusal(peer(230), &panes, &procs).is_some_and(|why| why.contains("session 200")));
        // `setsid` kept the pane terminal: refused by terminal.
        procs.procs.insert(240, ProcInfo { ppid: 1, session: Some(240), tty: Some(9), start: 2400 });
        assert!(refusal(peer(240), &panes, &procs).is_some_and(|why| why.contains("terminal 9")));
    }

    #[test]
    fn a_reused_pid_and_an_ancestor_that_vanishes_mid_walk_are_refused() {
        let mut procs = world();
        let panes = PaneSet::of([200], &procs);
        // The pid that connected was 70 started at 700; now 70 is another process.
        let stale = PeerOrigin::Process(PeerProcess { pid: 70, start: 1 });
        assert!(refusal(stale, &panes, &procs).is_some_and(|why| why.contains("reused")));
        // The shell (60) exits between reading the cli and walking up: fail closed.
        procs.procs.remove(&60);
        assert!(refusal(peer(70), &panes, &procs).is_some_and(|why| why.contains("changed")));
    }

    #[test]
    fn unknown_or_unreadable_peers_are_refused_and_token_peers_are_not() {
        let mut procs = world();
        let panes = PaneSet::of([200], &procs);
        assert!(refusal(PeerOrigin::Unknown, &panes, &procs).is_some());
        assert!(refusal(peer(999), &panes, &procs).is_some(), "a peer that is gone is refused");
        procs.unreadable.insert(70);
        assert!(refusal(peer(70), &panes, &procs).is_some(), "an unreadable peer is refused");
        procs.unreadable.clear();
        // An unreadable ancestor (a root sshd) ends the walk outside the panes...
        procs.unreadable.insert(50);
        assert_eq!(refusal(peer(70), &panes, &procs), None);
        // ...and a pane process under a root `sudo` is still caught by its session.
        procs.procs.insert(250, ProcInfo { ppid: 245, session: Some(200), tty: Some(9), start: 2500 });
        procs.unreadable.insert(245);
        assert!(refusal(peer(250), &panes, &procs).is_some());
        assert_eq!(refusal(PeerOrigin::TokenAuthenticated, &panes, &procs), None);
    }

    /// hidepid: a live process without a /proc entry for us is unreadable (the walk ends
    /// outside), and only a reaped one is vanished (the walk fails closed).
    #[cfg(target_os = "linux")]
    #[test]
    fn a_hidden_live_process_is_unreadable_and_a_reaped_one_vanished() {
        assert_eq!(absent_or_hidden(1), ProcError::Unreadable, "pid 1 always exists");
        let mut child = std::process::Command::new("true").spawn().expect("spawn true");
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
        assert_eq!(refusal(origin, &PaneSet::default(), &SystemProcs), None);
        // ...but it is when its parent is a pane child.
        let panes = PaneSet::of([parent], &SystemProcs);
        assert!(refusal(origin, &panes, &SystemProcs).is_some());
    }

    #[cfg(unix)]
    #[test]
    fn the_peer_pid_of_a_socketpair_is_this_process() {
        use std::os::unix::io::AsRawFd;
        let (left, _right) = std::os::unix::net::UnixStream::pair().expect("pair");
        assert_eq!(peer_pid(left.as_raw_fd()), Some(std::process::id()));
    }
}
