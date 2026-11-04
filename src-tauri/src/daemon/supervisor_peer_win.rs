//! Windows peer identification for the loopback TCP daemon transport (see `supervisor_peer`).
//!
//! A TCP connection carries no peer credentials, so at accept the daemon looks the connection up
//! in the kernel's TCP table (`GetExtendedTcpTable`, `TCP_TABLE_OWNER_PID_CONNECTIONS`): the
//! client's row is the one whose local endpoint is the peer address `accept` reported and whose
//! remote endpoint is the daemon's listening address, exactly. Its owning pid is then bound to
//! that process's creation time, like `identify` on unix, and the table is read a second time
//! after the process handle is open, so the pid was the connection's owner while it was held.
//!
//! The pid is then classified by the unix rule (`supervisor_peer::refusal`): a pane child, or a
//! parent chain that reaches one, is refused. Windows does not reparent: an orphan keeps the pid
//! of its exited parent, which may be reused. So a parent counts only while its creation time is
//! not later than its child's ([`chain_parent`]), and a parent that is gone or unreadable ends
//! the chain. What the unix session id does for a reparented pane process, the pane Job Object
//! does here: every pane child is placed in one daemon-owned job right after it is created, and
//! every process it starts afterwards is in that job whatever happens to its parents
//! ([`classify`]).
//!
//! The job sets no limits, so it forbids `CREATE_BREAKAWAY_FROM_JOB`: a pane process cannot
//! leave it directly. A process started for it by a broker (explorer, Task Scheduler, WMI, an
//! out-of-process COM server) is outside both the job and the parent chain and passes: the
//! Windows counterpart of the unix `setsid` escape `supervisor_peer` accepts as a deliberate
//! one. A process a pane child starts in the instant between its creation and its job assignment (portable-pty cannot create it suspended)
//! is not in the job either; the parent chain still catches it while the pane child lives.
//!
//! Fail closed: no unique owning row, a pid that cannot be opened, a creation time that changed
//! (pid reused), a pane that could not be placed in the job, or a job that could not be made all
//! refuse, as `TokenAuthenticated` did before.
//!
//! The decoding and classification here are platform-neutral and tested everywhere; the FFI is
//! in the `cfg(windows)` `sys` module below.
use super::supervisor_peer::{PaneSet, PeerOrigin, PeerProcess, ProcTable};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

/// `MIB_TCP_STATE_ESTAB`.
pub const TCP_STATE_ESTABLISHED: u32 = 5;

/// One row of the kernel's TCP connection table, decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TcpRow {
    pub local: SocketAddr,
    pub remote: SocketAddr,
    pub state: u32,
    pub pid: u32,
}

/// A port as `MIB_TCPROW_OWNER_PID` stores it: network byte order in the low two bytes of the
/// DWORD's memory.
pub fn row_port(raw: u32) -> u16 {
    let bytes = raw.to_ne_bytes();
    u16::from_be_bytes([bytes[0], bytes[1]])
}

/// An IPv4 address as `MIB_TCPROW_OWNER_PID` stores it: an `in_addr` (network order) in a DWORD.
pub fn row_v4(raw: u32) -> Ipv4Addr {
    Ipv4Addr::from(raw.to_ne_bytes())
}

/// An IPv4 row (`MIB_TCPROW_OWNER_PID` fields, raw).
pub fn v4_row(state: u32, local: (u32, u32), remote: (u32, u32), pid: u32) -> TcpRow {
    TcpRow {
        local: SocketAddr::new(IpAddr::V4(row_v4(local.0)), row_port(local.1)),
        remote: SocketAddr::new(IpAddr::V4(row_v4(remote.0)), row_port(remote.1)),
        state,
        pid,
    }
}

/// An IPv6 row (`MIB_TCP6ROW_OWNER_PID` fields, raw: address bytes, scope id, port).
pub fn v6_row(state: u32, local: ([u8; 16], u32, u32), remote: ([u8; 16], u32, u32), pid: u32) -> TcpRow {
    let addr = |(ip, scope, port): ([u8; 16], u32, u32)| {
        SocketAddr::V6(std::net::SocketAddrV6::new(
            Ipv6Addr::from(ip),
            row_port(port),
            0,
            u32::from_be(scope),
        ))
    };
    TcpRow {
        local: addr(local),
        remote: addr(remote),
        state,
        pid,
    }
}

/// Address, port and (IPv6) scope equal; flow info is not part of a connection's identity.
fn same_endpoint(a: SocketAddr, b: SocketAddr) -> bool {
    match (a, b) {
        (SocketAddr::V4(a), SocketAddr::V4(b)) => a == b,
        (SocketAddr::V6(a), SocketAddr::V6(b)) => {
            a.ip() == b.ip() && a.port() == b.port() && a.scope_id() == b.scope_id()
        }
        _ => false,
    }
}

/// The pid owning the client end of the connection the daemon accepted from `peer` on
/// `listening`: the one established row whose local endpoint is `peer` and whose remote endpoint
/// is `listening`. `None` unless exactly one row matches and its pid is a real process (not the
/// idle process 0 or System 4).
pub fn owning_pid(rows: &[TcpRow], peer: SocketAddr, listening: SocketAddr) -> Option<u32> {
    let mut owners = rows.iter().filter(|row| {
        row.state == TCP_STATE_ESTABLISHED
            && same_endpoint(row.local, peer)
            && same_endpoint(row.remote, listening)
    });
    let row = owners.next()?;
    if owners.next().is_some() {
        return None;
    }
    (row.pid > 4).then_some(row.pid)
}

/// The parent a chain walk may follow: `ppid` only while that process exists and was created no
/// later than its child, so an exited parent whose pid was reused is not mistaken for it. `0`
/// ends the walk.
pub fn chain_parent(ppid: u32, child_start: u64, parent_start: Option<u64>) -> u32 {
    match parent_start {
        Some(start) if ppid > 0 && start <= child_start => ppid,
        _ => 0,
    }
}

/// The process table the Windows rule needs on top of [`ProcTable`]: pane job membership.
pub trait WinProcTable: ProcTable {
    /// Whether `peer` (still the process created at `peer.start`) is in the pane job. `Err` when
    /// that cannot be told, which refuses.
    fn in_pane_job(&self, peer: PeerProcess) -> Result<bool, String>;
}

/// Why a Windows peer may not mutate supervisor state, or `None` when it may: the unix rule
/// (pane process, parent chain), then pane job membership.
pub fn classify(origin: PeerOrigin, panes: &PaneSet, table: &impl WinProcTable) -> Option<String> {
    if let Some(why) = super::supervisor_peer::refusal(origin, panes, table) {
        return Some(why);
    }
    let PeerOrigin::Process(peer) = origin else {
        return Some("the daemon cannot identify this connection's process, so it may not change the supervisor".into());
    };
    match table.in_pane_job(peer) {
        Ok(false) => None,
        Ok(true) => Some(format!(
            "process {} runs inside a Ferryx pane (pane job); a pane process may not change the supervisor",
            peer.pid
        )),
        Err(why) => Some(format!(
            "process {} could not be checked against the Ferryx panes ({why}); it may not change the supervisor",
            peer.pid
        )),
    }
}

/// The kernel side. Every `unsafe` block is a single documented FFI call; handles are owned by
/// `OwnedHandle` and closed on drop.
#[cfg(windows)]
pub mod sys {
    use super::{chain_parent, owning_pid, v4_row, v6_row, TcpRow, WinProcTable};
    use crate::daemon::supervisor_peer::{PeerOrigin, PeerProcess, ProcError, ProcInfo, ProcTable};
    use std::collections::HashMap;
    use std::net::SocketAddr;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::OnceLock;
    use windows_sys::Win32::Foundation::{
        GetLastError, ERROR_INSUFFICIENT_BUFFER, ERROR_INVALID_PARAMETER, FILETIME, HANDLE,
        INVALID_HANDLE_VALUE, NO_ERROR, STILL_ACTIVE,
    };
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCP6TABLE_OWNER_PID, MIB_TCPROW_OWNER_PID,
        MIB_TCPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_CONNECTIONS,
    };
    use windows_sys::Win32::Networking::WinSock::{AF_INET, AF_INET6};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::JobObjects::{AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    /// The kernel's TCP connections of one address family, decoded. `None` when the table cannot
    /// be read.
    fn tcp_rows(v6: bool) -> Option<Vec<TcpRow>> {
        let family = u32::from(if v6 { AF_INET6 } else { AF_INET });
        let mut wanted: u32 = 4096;
        // The table can grow between the size probe and the read; a few retries settle it.
        for _ in 0..8 {
            // u64 storage: 8-byte aligned, more than the 4 the row types need.
            let mut buf = vec![0u64; (wanted as usize).div_ceil(8)];
            let mut len = u32::try_from(buf.len() * 8).ok()?;
            // SAFETY: `buf` is writable for `len` bytes and outlives the call; `len` is a valid
            // in/out pointer; the class/family pair selects MIB_TCP(6)TABLE_OWNER_PID, which the
            // kernel writes within `len` bytes or answers ERROR_INSUFFICIENT_BUFFER.
            let status = unsafe {
                GetExtendedTcpTable(buf.as_mut_ptr().cast(), &mut len, 0, family, TCP_TABLE_OWNER_PID_CONNECTIONS, 0)
            };
            if status == ERROR_INSUFFICIENT_BUFFER {
                wanted = len.max(wanted.saturating_mul(2));
                continue;
            }
            if status != NO_ERROR {
                return None;
            }
            let filled = (len as usize).min(buf.len() * 8);
            // SAFETY: the first `filled` bytes of `buf` are initialised (zeroed, then written).
            let bytes = unsafe { std::slice::from_raw_parts(buf.as_ptr().cast::<u8>(), filled) };
            return if v6 {
                rows::<MIB_TCP6ROW_OWNER_PID>(bytes, std::mem::offset_of!(MIB_TCP6TABLE_OWNER_PID, table), |r| {
                    v6_row(
                        r.dwState,
                        (r.ucLocalAddr, r.dwLocalScopeId, r.dwLocalPort),
                        (r.ucRemoteAddr, r.dwRemoteScopeId, r.dwRemotePort),
                        r.dwOwningPid,
                    )
                })
            } else {
                rows::<MIB_TCPROW_OWNER_PID>(bytes, std::mem::offset_of!(MIB_TCPTABLE_OWNER_PID, table), |r| {
                    v4_row(r.dwState, (r.dwLocalAddr, r.dwLocalPort), (r.dwRemoteAddr, r.dwRemotePort), r.dwOwningPid)
                })
            };
        }
        None
    }

    /// The `dwNumEntries` rows after the header, bounds-checked against the bytes written.
    fn rows<R: Copy>(bytes: &[u8], first: usize, decode: impl Fn(R) -> TcpRow) -> Option<Vec<TcpRow>> {
        let count = u32::from_ne_bytes(bytes.get(..4)?.try_into().ok()?) as usize;
        let size = std::mem::size_of::<R>();
        let end = count.checked_mul(size)?.checked_add(first)?;
        if end > bytes.len() {
            return None;
        }
        Some(
            (0..count)
                .map(|i| {
                    // SAFETY: `first + i * size + size <= end <= bytes.len()`, and R is a plain
                    // C struct of integers and byte arrays, valid for any bit pattern; the read
                    // is unaligned-safe.
                    decode(unsafe { std::ptr::read_unaligned(bytes.as_ptr().add(first + i * size).cast::<R>()) })
                })
                .collect(),
        )
    }

    /// `pid` opened for limited query. While the handle is open the pid cannot be reused.
    fn open(pid: u32) -> Result<OwnedHandle, ProcError> {
        // SAFETY: OpenProcess takes plain values and returns a new handle or null.
        let handle: HANDLE = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            // SAFETY: reads this thread's last-error value.
            return Err(if unsafe { GetLastError() } == ERROR_INVALID_PARAMETER {
                ProcError::Vanished
            } else {
                ProcError::Unreadable
            });
        }
        // SAFETY: `handle` is a fresh, valid process handle owned by nobody else.
        Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
    }

    /// The process's creation time (FILETIME ticks).
    fn creation_time(process: &OwnedHandle) -> Option<u64> {
        let zero = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
        // SAFETY: `process` is a valid handle with query access; the four out pointers are valid.
        let ok = unsafe {
            GetProcessTimes(process.as_raw_handle(), &mut created, &mut exited, &mut kernel, &mut user)
        };
        if ok == 0 {
            return None;
        }
        Some((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
    }

    fn still_running(process: RawHandle) -> bool {
        let mut code = 0u32;
        // SAFETY: `process` is a valid process handle the caller holds; `code` is a valid out pointer.
        let ok = unsafe { GetExitCodeProcess(process, &mut code) };
        ok == 0 || code == STILL_ACTIVE as u32
    }

    /// The peer of an accepted loopback connection: `peer` is `accept`'s peer address and
    /// `listening` the accepted socket's local address. `TokenAuthenticated` (refused) unless
    /// the owner is found and bound to its creation time.
    pub fn identify(peer: SocketAddr, listening: SocketAddr) -> PeerOrigin {
        identified(peer, listening).map_or(PeerOrigin::TokenAuthenticated, PeerOrigin::Process)
    }

    fn identified(peer: SocketAddr, listening: SocketAddr) -> Option<PeerProcess> {
        let v6 = peer.is_ipv6();
        let pid = owning_pid(&tcp_rows(v6)?, peer, listening)?;
        let process = open(pid).ok()?;
        let start = creation_time(&process)?;
        // The pid was read before the handle pinned it: read the table again now that it cannot
        // be reused, so `start` belongs to the process that owns the connection.
        (owning_pid(&tcp_rows(v6)?, peer, listening)? == pid).then_some(PeerProcess { pid, start })
    }

    /// The job every pane child joins (no breakaway, so a pane's descendants stay in it).
    static PANE_JOB: OnceLock<Option<OwnedHandle>> = OnceLock::new();
    /// Set once a live pane could not be placed in the job: from then on no peer can be told
    /// apart from a pane descendant, so every gated request is refused.
    static ADOPTION_FAILED: AtomicBool = AtomicBool::new(false);

    /// Places a newly spawned pane child in the pane job. A child that already exited is skipped.
    pub fn adopt_pane(child: &dyn portable_pty::Child) {
        // The child owns its process handle and keeps it open while it is borrowed here.
        let Some(process) = child.as_raw_handle() else {
            ADOPTION_FAILED.store(true, Ordering::SeqCst);
            return;
        };
        let job = PANE_JOB.get_or_init(|| {
            // SAFETY: null attributes and name create an unnamed job with default security; the
            // returned handle is null on failure and owned here otherwise.
            let job: HANDLE = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if job.is_null() {
                return None;
            }
            // SAFETY: `job` is a fresh, valid job handle owned by nobody else. No limits are set,
            // so the job forbids breakaway: a pane's descendants cannot leave it.
            Some(unsafe { OwnedHandle::from_raw_handle(job) })
        });
        let assigned = job.as_ref().is_some_and(|job| {
            // SAFETY: both handles are valid for the call; the caller keeps `process` open.
            unsafe { AssignProcessToJobObject(job.as_raw_handle(), process) != 0 }
        });
        if !assigned && still_running(process) {
            tracing::warn!("a pane could not be placed in the pane job; supervisor changes are refused from now on");
            ADOPTION_FAILED.store(true, Ordering::SeqCst);
        }
    }

    /// The live process table: parents from one Toolhelp snapshot, each process bound to its
    /// creation time when read.
    pub struct SystemWinProcs {
        parents: HashMap<u32, u32>,
    }

    impl SystemWinProcs {
        pub fn snapshot() -> Option<Self> {
            // SAFETY: plain values; returns INVALID_HANDLE_VALUE on failure.
            let snapshot: HANDLE = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
            if snapshot == INVALID_HANDLE_VALUE {
                return None;
            }
            // SAFETY: a fresh, valid snapshot handle owned by nobody else.
            let snapshot = unsafe { OwnedHandle::from_raw_handle(snapshot) };
            // SAFETY: PROCESSENTRY32W is plain data; all-zero is valid.
            let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut parents = HashMap::new();
            // SAFETY: valid snapshot handle and an entry whose dwSize is set.
            let mut more = unsafe { Process32FirstW(snapshot.as_raw_handle(), &mut entry) } != 0;
            while more {
                parents.insert(entry.th32ProcessID, entry.th32ParentProcessID);
                // SAFETY: as above.
                more = unsafe { Process32NextW(snapshot.as_raw_handle(), &mut entry) } != 0;
            }
            Some(Self { parents })
        }
    }

    impl ProcTable for SystemWinProcs {
        fn info(&self, pid: u32) -> Result<ProcInfo, ProcError> {
            let process = open(pid)?;
            let start = creation_time(&process).ok_or(ProcError::Unreadable)?;
            let ppid = *self.parents.get(&pid).ok_or(ProcError::Unreadable)?;
            let parent_start = open(ppid).ok().and_then(|parent| creation_time(&parent));
            Ok(ProcInfo {
                ppid: chain_parent(ppid, start, parent_start),
                session: None,
                tty: None,
                start,
            })
        }
    }

    impl WinProcTable for SystemWinProcs {
        fn in_pane_job(&self, peer: PeerProcess) -> Result<bool, String> {
            if ADOPTION_FAILED.load(Ordering::SeqCst) {
                return Err("a pane could not be placed in the pane job".into());
            }
            let Some(job) = PANE_JOB.get() else {
                return Ok(false);
            };
            let job = job.as_ref().ok_or("the pane job could not be created")?;
            let process = open(peer.pid).map_err(|_| "the process could not be opened".to_string())?;
            if creation_time(&process) != Some(peer.start) {
                return Err("its pid was reused".into());
            }
            let mut inside = 0;
            // SAFETY: both handles are valid for the call; `inside` is a valid out pointer.
            let ok = unsafe {
                IsProcessInJob(process.as_raw_handle(), job.as_raw_handle(), &mut inside)
            };
            if ok == 0 {
                return Err("its job membership could not be read".into());
            }
            Ok(inside != 0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::supervisor_peer::{ProcError, ProcInfo};
    use std::collections::{HashMap, HashSet};

    /// A raw row DWORD the way the kernel lays out network-order bytes in memory.
    fn raw(bytes: [u8; 4]) -> u32 {
        u32::from_ne_bytes(bytes)
    }
    fn raw_port(port: u16) -> u32 {
        let [hi, lo] = port.to_be_bytes();
        raw([hi, lo, 0, 0])
    }
    fn addr(text: &str) -> SocketAddr {
        text.parse().unwrap()
    }
    fn v4(state: u32, local: &str, remote: &str, pid: u32) -> TcpRow {
        let (l, r) = (addr(local), addr(remote));
        let ip = |a: SocketAddr| match a.ip() {
            IpAddr::V4(ip) => raw(ip.octets()),
            IpAddr::V6(_) => unreachable!(),
        };
        v4_row(state, (ip(l), raw_port(l.port())), (ip(r), raw_port(r.port())), pid)
    }

    #[test]
    fn a_kernel_row_decodes_to_the_endpoints_it_names() {
        let row = v4(TCP_STATE_ESTABLISHED, "127.0.0.1:50123", "127.0.0.1:4100", 77);
        assert_eq!(row.local, addr("127.0.0.1:50123"));
        assert_eq!(row.remote, addr("127.0.0.1:4100"));
        let ip6 = "::1".parse::<Ipv6Addr>().unwrap().octets();
        let row = v6_row(TCP_STATE_ESTABLISHED, (ip6, 0, raw_port(50124)), (ip6, 0, raw_port(4100)), 78);
        assert_eq!(row.local, addr("[::1]:50124"));
        assert_eq!(row.remote, addr("[::1]:4100"));
        let scoped = v6_row(5, (ip6, 3u32.to_be(), raw_port(1)), (ip6, 0, raw_port(2)), 1);
        let SocketAddr::V6(local) = scoped.local else { unreachable!() };
        assert_eq!(local.scope_id(), 3, "scope ids are network order in the row");
    }

    /// The client row is the one whose local end is the accepted peer and whose remote end is
    /// the listener; the daemon's own (reversed) row, other connections, non-established rows
    /// and ambiguous or kernel-owned rows never name the owner.
    #[test]
    fn only_the_exact_client_row_names_the_owner() {
        let peer = addr("127.0.0.1:50123");
        let listening = addr("127.0.0.1:4100");
        let daemon_side = v4(TCP_STATE_ESTABLISHED, "127.0.0.1:4100", "127.0.0.1:50123", 10);
        let client = v4(TCP_STATE_ESTABLISHED, "127.0.0.1:50123", "127.0.0.1:4100", 77);
        let other = v4(TCP_STATE_ESTABLISHED, "127.0.0.1:50124", "127.0.0.1:4100", 88);
        let other_listener = v4(TCP_STATE_ESTABLISHED, "127.0.0.1:50123", "127.0.0.1:4101", 99);
        let other_ip = v4(TCP_STATE_ESTABLISHED, "127.0.0.2:50123", "127.0.0.1:4100", 99);
        assert_eq!(owning_pid(&[daemon_side, other, other_listener, other_ip, client], peer, listening), Some(77));
        assert_eq!(owning_pid(&[daemon_side, other, other_listener, other_ip], peer, listening), None);
        let closing = TcpRow { state: 8, ..client };
        assert_eq!(owning_pid(&[closing], peer, listening), None);
        let twin = TcpRow { pid: 78, ..client };
        assert_eq!(owning_pid(&[client, twin], peer, listening), None, "ambiguous");
        for pid in [0, 4] {
            assert_eq!(owning_pid(&[TcpRow { pid, ..client }], peer, listening), None);
        }
        let ip6 = "::1".parse::<Ipv6Addr>().unwrap().octets();
        let client6 = v6_row(TCP_STATE_ESTABLISHED, (ip6, 0, raw_port(50123)), (ip6, 0, raw_port(4100)), 66);
        assert_eq!(owning_pid(&[client, client6], addr("[::1]:50123"), addr("[::1]:4100")), Some(66));
        assert_eq!(owning_pid(&[client6], peer, listening), None, "families never mix");
        let scoped6 = v6_row(TCP_STATE_ESTABLISHED, (ip6, 2u32.to_be(), raw_port(50123)), (ip6, 0, raw_port(4100)), 65);
        assert_eq!(owning_pid(&[scoped6], addr("[::1]:50123"), addr("[::1]:4100")), None, "scope differs");
    }

    #[test]
    fn a_parent_created_after_its_child_is_a_reused_pid_and_ends_the_chain() {
        assert_eq!(chain_parent(40, 1000, Some(900)), 40);
        assert_eq!(chain_parent(40, 1000, Some(1000)), 40);
        assert_eq!(chain_parent(40, 1000, Some(1001)), 0, "pid reused after the parent exited");
        assert_eq!(chain_parent(40, 1000, None), 0, "parent gone or unreadable");
        assert_eq!(chain_parent(0, 1000, Some(1)), 0);
    }

    struct Table {
        procs: HashMap<u32, ProcInfo>,
        job: HashSet<u32>,
        job_unreadable: bool,
    }
    impl ProcTable for Table {
        fn info(&self, pid: u32) -> Result<ProcInfo, ProcError> {
            self.procs.get(&pid).copied().ok_or(ProcError::Vanished)
        }
    }
    impl WinProcTable for Table {
        fn in_pane_job(&self, peer: PeerProcess) -> Result<bool, String> {
            if self.job_unreadable {
                return Err("unreadable".into());
            }
            Ok(self.job.contains(&peer.pid))
        }
    }
    fn proc(ppid: u32, start: u64) -> ProcInfo {
        ProcInfo { ppid, session: None, tty: None, start }
    }
    fn peer(pid: u32, table: &Table) -> PeerOrigin {
        PeerOrigin::Process(PeerProcess { pid, start: table.procs[&pid].start })
    }
    /// explorer(50) -> desktop app(60); daemon(100) -> pane child(200) -> agent(210) -> cli(220);
    /// orphan(230): its pane parent exited, so its chain ends at 0 but it is still in the job.
    fn world() -> Table {
        let procs = [
            (50, proc(0, 1)),
            (60, proc(50, 2)),
            (100, proc(0, 3)),
            (200, proc(100, 4)),
            (210, proc(200, 5)),
            (220, proc(210, 6)),
            (230, proc(0, 7)),
        ];
        Table { procs: procs.into_iter().collect(), job: [200, 210, 220, 230].into(), job_unreadable: false }
    }

    #[test]
    fn the_desktop_app_may_change_the_supervisor_and_pane_processes_may_not() {
        let table = world();
        let panes = PaneSet::of([200], &table);
        assert_eq!(classify(peer(60, &table), &panes, &table), None);
        for pid in [200, 220, 230] {
            assert!(classify(peer(pid, &table), &panes, &table).is_some_and(|why| why.contains("Ferryx pane")), "{pid}");
        }
        // The orphan is caught by the job alone; the descendant by the parent chain alone.
        let mut no_job = world();
        no_job.job.clear();
        assert!(classify(peer(220, &no_job), &panes, &no_job).is_some());
        assert_eq!(classify(peer(230, &no_job), &panes, &no_job), None);
    }

    #[test]
    fn an_unresolved_or_unreadable_peer_is_refused() {
        let mut table = world();
        let panes = PaneSet::of([200], &table);
        assert!(classify(PeerOrigin::TokenAuthenticated, &panes, &table).is_some());
        assert!(classify(PeerOrigin::Unknown, &panes, &table).is_some());
        let reused = PeerOrigin::Process(PeerProcess { pid: 60, start: 99 });
        assert!(classify(reused, &panes, &table).is_some_and(|why| why.contains("reused")));
        table.job_unreadable = true;
        assert!(classify(peer(60, &table), &panes, &table).is_some());
    }
}
