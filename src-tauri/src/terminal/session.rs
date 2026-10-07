use crate::terminal::output_hub::{SessionHubSnapshot, TerminalOutputHub};
use crate::terminal::PtyError;
use parking_lot::{Mutex, RwLock};
use portable_pty::{Child, MasterPty, PtySize, ReaderInterrupt};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
#[cfg(windows)]
#[path = "windows_input.rs"]
pub(crate) mod windows_input;

#[cfg(windows)]
mod windows_suspend {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};

    const PROCESS_SUSPEND_RESUME: u32 = 0x0800;
    const PROCESS_SET_QUOTA: u32 = 0x0100;
    const PROCESS_QUERY_INFORMATION: u32 = 0x0400;

    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> HANDLE;
        fn K32EmptyWorkingSet(process: HANDLE) -> i32;
    }

    #[link(name = "ntdll")]
    extern "system" {
        fn NtSuspendProcess(process: HANDLE) -> i32;
        fn NtResumeProcess(process: HANDLE) -> i32;
    }

    pub fn suspend_process(pid: u32) -> Result<(), String> {
        let handle = unsafe {
            OpenProcess(
                PROCESS_SUSPEND_RESUME | PROCESS_SET_QUOTA | PROCESS_QUERY_INFORMATION,
                0,
                pid,
            )
        };
        if handle.is_null() {
            return Err(format!(
                "OpenProcess failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        let status = unsafe { NtSuspendProcess(handle) };
        if status < 0 {
            unsafe {
                CloseHandle(handle);
            }
            return Err(format!("NtSuspendProcess failed with status {status:#x}"));
        }
        unsafe {
            K32EmptyWorkingSet(handle);
        }
        unsafe {
            CloseHandle(handle);
        }
        Ok(())
    }

    pub fn resume_process(pid: u32) -> Result<(), String> {
        let handle = unsafe { OpenProcess(PROCESS_SUSPEND_RESUME, 0, pid) };
        if handle.is_null() {
            return Err(format!(
                "OpenProcess failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        let status = unsafe { NtResumeProcess(handle) };
        unsafe {
            CloseHandle(handle);
        }
        if status < 0 {
            return Err(format!("NtResumeProcess failed with status {status:#x}"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PtySessionState {
    Starting,
    Running,
    Closing,
    Exited { code: Option<i32> },
    Failed { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AdoptedProcess {
    pub pid: u32,
    pub process_group: Option<u32>,
}

#[cfg(unix)]
impl AdoptedProcess {
    pub fn is_alive(&self) -> bool {
        let res = unsafe { libc::kill(self.pid as i32, 0) };
        if res != 0 {
            let err = std::io::Error::last_os_error();
            return err.raw_os_error() == Some(libc::EPERM);
        }
        // A successor daemon is not the adopted shell's parent, so it can never waitpid it.
        // An exited-but-unreaped shell still answers kill(pid, 0) until its real parent reaps
        // it; treating that zombie as alive made Close time out after a successful SIGKILL.
        !process_is_zombie(self.pid)
    }
}

/// The process group the kernel currently reports for `pid`.
#[cfg(unix)]
fn live_process_group(pid: u32) -> Option<u32> {
    let group = unsafe { libc::getpgid(pid as libc::pid_t) };
    (group > 0).then_some(group as u32)
}

#[cfg(target_os = "macos")]
fn process_is_zombie(pid: u32) -> bool {
    kinfo_proc_stat(pid) == Some(libc::SZOMB)
}

/// Whether the kernel reports `pid` as job-control stopped (SIGSTOP/SIGTSTP). This is the
/// ground truth for "suspended": it survives GUI restarts and daemon handovers, which the
/// in-memory lifecycle registry and the frontend's sleeping set do not.
#[cfg(target_os = "macos")]
fn process_is_stopped(pid: u32) -> bool {
    kinfo_proc_stat(pid) == Some(libc::SSTOP)
}

#[cfg(target_os = "macos")]
fn kinfo_proc_stat(pid: u32) -> Option<u32> {
    // proc_pidinfo answers ESRCH for a zombie, so only the kinfo_proc sysctl can see one.
    // libc does not bind kinfo_proc on Apple targets; read extern_proc.p_stat by offset
    // (identical on arm64 and x86_64: 648-byte struct, p_stat at 36).
    const KINFO_PROC_SIZE: usize = 648;
    const P_STAT_OFFSET: usize = 36;
    let mut mib = [libc::CTL_KERN, libc::KERN_PROC, libc::KERN_PROC_PID, pid as i32];
    let mut info = [0u8; KINFO_PROC_SIZE];
    let mut len = info.len();
    let read = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as u32,
            info.as_mut_ptr().cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    (read == 0 && len == KINFO_PROC_SIZE).then(|| u32::from(info[P_STAT_OFFSET]))
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn process_is_zombie(pid: u32) -> bool {
    proc_stat_state(pid) == Some('Z')
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn process_is_stopped(pid: u32) -> bool {
    // 'T' is a job-control stop; 't' is a ptrace stop, which is not a Ferryx suspension.
    proc_stat_state(pid) == Some('T')
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn proc_stat_state(pid: u32) -> Option<char> {
    // The state field follows the parenthesised command name, which may itself contain ')'.
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|stat| {
            stat.rsplit_once(')')
                .and_then(|(_, rest)| rest.trim_start().chars().next())
        })
}

#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "linux", target_os = "android"))
))]
fn process_is_zombie(_pid: u32) -> bool {
    false
}

#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "linux", target_os = "android"))
))]
fn process_is_stopped(_pid: u32) -> bool {
    false
}

pub(crate) enum ProcessHandle {
    Spawned(Box<dyn Child + Send + Sync>),
    Adopted(AdoptedProcess),
}

impl ProcessHandle {
    pub fn pid(&self) -> Option<u32> {
        match self {
            Self::Spawned(child) => child.process_id(),
            Self::Adopted(adopted) => Some(adopted.pid),
        }
    }

    pub fn process_group(&self) -> Option<u32> {
        match self {
            Self::Spawned(_) => None,
            Self::Adopted(adopted) => adopted.process_group,
        }
    }

    pub fn kill(&mut self) -> Result<(), PtyError> {
        match self {
            Self::Spawned(child) => child
                .kill()
                .map_err(|e| PtyError::KillError(format!("Kill failed: {e}"))),
            #[cfg(unix)]
            Self::Adopted(adopted) => {
                let pid = adopted.pid as i32;
                if pid <= 1 {
                    return Err(PtyError::KillError(format!("Invalid PID for kill: {pid}")));
                }
                if let Some(pg) = adopted.process_group {
                    if pg > 1 {
                        let target = -(pg as i32);
                        let res = unsafe { libc::kill(target, libc::SIGKILL) };
                        if res == 0 {
                            return Ok(());
                        }
                    }
                }
                let res = unsafe { libc::kill(pid, libc::SIGKILL) };
                if res == 0 {
                    return Ok(());
                }
                let err = std::io::Error::last_os_error();
                if err.raw_os_error() == Some(libc::ESRCH) {
                    Ok(())
                } else {
                    Err(PtyError::KillError(format!(
                        "Failed to kill adopted process {pid}: {err}"
                    )))
                }
            }
            #[cfg(not(unix))]
            Self::Adopted(_) => Err(PtyError::Other("Adopted process not supported on this platform".into())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PtySessionSnapshot {
    pub session_id: String,
    pub pid: Option<u32>,
    pub pgid: Option<u32>,
    pub cols: u16,
    pub rows: u16,
    pub worktree_path: Option<PathBuf>,
    pub state: PtySessionState,
    pub hub_snapshot: Option<SessionHubSnapshot>,
}

pub type PtyAdoptSnapshot = PtySessionSnapshot;

#[cfg(unix)]
#[derive(Debug)]
pub struct PtySessionExport {
    pub session_id: String,
    pub pid: Option<u32>,
    pub pgid: Option<u32>,
    pub cols: u16,
    pub rows: u16,
    pub worktree_path: Option<PathBuf>,
    pub state: PtySessionState,
    pub hub_snapshot: Option<SessionHubSnapshot>,
    pub master_raw_fd: std::os::fd::RawFd,
    pub master_fd: std::os::fd::OwnedFd,
}

#[cfg(unix)]
pub type ExportedPtySession = PtySessionExport;

#[cfg(unix)]
impl PtySessionExport {
    pub fn snapshot(&self) -> PtySessionSnapshot {
        PtySessionSnapshot {
            session_id: self.session_id.clone(),
            pid: self.pid,
            pgid: self.pgid,
            cols: self.cols,
            rows: self.rows,
            worktree_path: self.worktree_path.clone(),
            state: self.state.clone(),
            hub_snapshot: self.hub_snapshot.clone(),
        }
    }

    pub fn into_parts(self) -> (std::os::fd::OwnedFd, PtySessionSnapshot) {
        let snapshot = self.snapshot();
        (self.master_fd, snapshot)
    }

    pub fn from_parts(snapshot: PtySessionSnapshot, master_fd: std::os::fd::OwnedFd) -> Self {
        use std::os::fd::AsRawFd;
        let master_raw_fd = master_fd.as_raw_fd();
        Self {
            session_id: snapshot.session_id,
            pid: snapshot.pid,
            pgid: snapshot.pgid,
            cols: snapshot.cols,
            rows: snapshot.rows,
            worktree_path: snapshot.worktree_path,
            state: snapshot.state,
            hub_snapshot: snapshot.hub_snapshot,
            master_raw_fd,
            master_fd,
        }
    }
}

#[cfg(unix)]
impl From<PtySessionExport> for PtySessionSnapshot {
    fn from(export: PtySessionExport) -> Self {
        export.snapshot()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TerminalSignal {
    Interrupt,
    Terminate,
    Kill,
    Stop,
    Continue,
}

pub(crate) struct PtySessionConfig {
    #[cfg(windows)]
    pub input: windows_input::WindowsInput,
    #[cfg(unix)]
    pub input: tokio::io::unix::AsyncFd<std::os::fd::OwnedFd>,
    pub id: String,
    pub master: Box<dyn MasterPty + Send>,
    /// The same master's reader-interruption handle, taken before the master was boxed, so the
    /// session keeps it for the whole of its life rather than only while the master is present.
    pub reader_interrupt: Option<Arc<dyn ReaderInterrupt>>,
    pub child: Box<dyn Child + Send + Sync>,
    pub writer: Box<dyn Write + Send>,
    pub reader: Box<dyn Read + Send>,
    pub cols: u16,
    pub rows: u16,
    pub tx: mpsc::Sender<Vec<u8>>,
    /// Canonical worktree/repository root that owns this interactive terminal.
    ///
    /// Interactive terminals are intentionally *not* exclusive worktree writers: Orca
    /// allows several PTYs in the same worktree.  Keeping the ownership path directly on
    /// the PTY session preserves session listing and CWD validation without abusing the
    /// exclusive writer lease used by agent/destructive-operation safety.
    pub worktree_path: Option<PathBuf>,
    /// The TERM grace the close that claims this session will run, shared with the session so a
    /// concurrent close can size its fence for the close actually in flight rather than for its own
    /// grace. Both constructors start it empty: a session that is not closing has no close to cover.
    pub close_grace: Arc<Mutex<Option<Duration>>>,
}

/// Where a PTY reader thread currently is. Diagnostic only: written by the reader loop and read
/// by tests and failure messages; it changes no behavior. A parked thread reports the phase it was
/// entering when it stopped, which is what locates a stall the boolean `reader_finished` cannot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PtyReaderPhase {
    Starting,
    BeforeRead,
    AfterRead,
    BeforeSend,
    AfterSend,
    Finished,
}

impl PtyReaderPhase {
    pub(crate) fn as_raw(self) -> u64 {
        match self {
            PtyReaderPhase::Starting => 0,
            PtyReaderPhase::BeforeRead => 1,
            PtyReaderPhase::AfterRead => 2,
            PtyReaderPhase::BeforeSend => 3,
            PtyReaderPhase::AfterSend => 4,
            PtyReaderPhase::Finished => 5,
        }
    }

    pub(crate) fn from_raw(raw: u64) -> Self {
        match raw {
            1 => PtyReaderPhase::BeforeRead,
            2 => PtyReaderPhase::AfterRead,
            3 => PtyReaderPhase::BeforeSend,
            4 => PtyReaderPhase::AfterSend,
            5 => PtyReaderPhase::Finished,
            _ => PtyReaderPhase::Starting,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            PtyReaderPhase::Starting => "starting",
            PtyReaderPhase::BeforeRead => "before-read",
            PtyReaderPhase::AfterRead => "after-read",
            PtyReaderPhase::BeforeSend => "before-send",
            PtyReaderPhase::AfterSend => "after-send",
            PtyReaderPhase::Finished => "finished",
        }
    }
}

pub struct PtySession {
    #[cfg(windows)]
    input: windows_input::WindowsInput,
    #[cfg(unix)]
    input: tokio::io::unix::AsyncFd<std::os::fd::OwnedFd>,
    input_gate: tokio::sync::Mutex<()>,
    pub id: String,
    master: Arc<Mutex<Option<Box<dyn MasterPty + Send>>>>,
    /// The reader-interruption handle, taken from the master while it was still present.
    ///
    /// Retained separately from `master` on purpose: a session gives its master up - after a failed
    /// handover export the master is already gone - and must still be able to end its reader when it
    /// finally closes. A platform whose reader needs no handle yields `None` and behaves as before.
    reader_interrupt: Arc<Mutex<Option<Arc<dyn ReaderInterrupt>>>>,
    /// The master release this session handed to the runtime's blocking pool, if the release went
    /// off-thread. Kept so the close can OBSERVE that release and bound it: the pool is a thread
    /// budget and not a duration bound, so an unobserved handle cannot tell a release that ran from
    /// one that is still queued - or from one the pool never ran at all.
    master_release: Arc<Mutex<Option<JoinHandle<()>>>>,
    writer: Arc<Mutex<Option<Box<dyn Write + Send>>>>,
    child: Arc<Mutex<Option<ProcessHandle>>>,
    reader_task: Arc<Mutex<Option<JoinHandle<()>>>>,
    output_tx: Arc<Mutex<Option<mpsc::Sender<Vec<u8>>>>>,
    worktree_path: Option<PathBuf>,
    reader_finished: Arc<AtomicBool>,
    /// Set when this session begins closing, so its reader stops instead of issuing another
    /// blocking read. Deliberately separate from `reader_finished`, which means "the reader thread
    /// has returned" and is consumed by the lifecycle watcher.
    reader_stop_requested: Arc<AtomicBool>,
    /// The reader's own phase, diagnostics only (see `PtyReaderPhase`).
    reader_phase: Arc<AtomicU64>,
    reaped: Arc<AtomicBool>,
    /// Epoch millis of the last PTY output chunk read from the child (0 = none).
    last_output_at: Arc<AtomicU64>,
    state: Arc<Mutex<PtySessionState>>,
    cols: Arc<Mutex<u16>>,
    rows: Arc<Mutex<u16>>,
    output_hub: Arc<RwLock<Option<Arc<TerminalOutputHub>>>>,
    pause_requested: Arc<AtomicBool>,
    reader_paused: Arc<AtomicBool>,
    /// Set when a teardown releases a reader parked by `pause_reader`.
    ///
    /// Deliberately NOT `reader_finished`: releasing a parked reader is not the reader's end, and the
    /// close path reads `reader_finished` as proof that the reader thread returned, so writing the
    /// release into that flag let a close pass stage 1 with the reader still parked.
    pause_released: Arc<AtomicBool>,
    /// The TERM grace the close that claimed this session is running, kept at the MAXIMUM over every
    /// close that claimed it.
    ///
    /// The concurrent-close fence has to cover the close it waits on, and that close's grace phase is
    /// a caller parameter rather than a module constant: a fence sized from the waiting call's own
    /// grace is still short whenever the two callers pass different ones, and a 1 s `close_session`
    /// waiting on a 5 s machine close is exactly that case. Empty until a close claims the session.
    close_grace: Arc<Mutex<Option<Duration>>>,
}

fn record_output_millis(target: &AtomicU64) {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    target.store(millis, Ordering::Relaxed);
}

fn last_output_age_from(last_output_millis: u64) -> Option<u64> {
    if last_output_millis == 0 {
        return None;
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(last_output_millis);
    Some(now.saturating_sub(last_output_millis))
}

impl PtySession {
    pub(crate) fn new(config: PtySessionConfig) -> Self {
        let reader_interrupt = Arc::new(Mutex::new(config.reader_interrupt));
        let output_tx = Arc::new(Mutex::new(Some(config.tx)));
        let reader_tx = output_tx
            .lock()
            .as_ref()
            .expect("output sender must exist while starting reader")
            .clone();
        let reader = config.reader;
        #[cfg(unix)]
        let reader_poll = {
            use std::os::fd::AsFd;
            config
                .input
                .get_ref()
                .as_fd()
                .try_clone_to_owned()
                .expect("duplicate PTY poll descriptor")
        };
        let metrics_session_id = config.id.clone();
        let reader_finished = Arc::new(AtomicBool::new(false));
        let reader_finished_task = Arc::clone(&reader_finished);
        let reader_stop_requested = Arc::new(AtomicBool::new(false));
        let reader_stop_requested_task = Arc::clone(&reader_stop_requested);
        let reader_last_output_at = Arc::new(AtomicU64::new(0));
        let last_output_at = Arc::clone(&reader_last_output_at);
        let task_last_output_at = Arc::clone(&last_output_at);
        let pause_requested = Arc::new(AtomicBool::new(false));
        let reader_paused = Arc::new(AtomicBool::new(false));
        let pause_requested_task = Arc::clone(&pause_requested);
        let reader_paused_task = Arc::clone(&reader_paused);
        let pause_released = Arc::new(AtomicBool::new(false));
        let pause_released_task = Arc::clone(&pause_released);
        let reader_phase = Arc::new(AtomicU64::new(PtyReaderPhase::Starting.as_raw()));
        let reader_phase_task = Arc::clone(&reader_phase);
        let reader_task = tokio::task::spawn_blocking(move || {
            let mut reader = reader;
            let mut buf = [0u8; 4096];
            loop {
                // A requested stop ends the loop once the pipe has nothing left to deliver. It is a
                // variable rather than an immediate `break` because one read carries at most one
                // buffer: the read below keeps draining while bytes remain, and the loop ends on the
                // read that reports the cancellation with nothing buffered. Breaking here instead
                // would drop a tail larger than one buffer that the pipe is still holding.
                let stopping = reader_stop_requested_task.load(Ordering::Acquire);
                if !stopping && pause_requested_task.load(Ordering::Acquire) {
                    reader_paused_task.store(true, Ordering::Release);
                    while pause_requested_task.load(Ordering::Acquire)
                        && !pause_released_task.load(Ordering::Acquire)
                    {
                        std::thread::sleep(std::time::Duration::from_millis(20));
                    }
                    reader_paused_task.store(false, Ordering::Release);
                    // Released by teardown (`release_paused_reader`), not by a resume: the
                    // descriptor may already belong to a successor daemon, so never read again. The
                    // release is its own signal, so it is not mistaken for the reader's own end,
                    // which is the flag this reader sets when it really returns.
                    if pause_released_task.load(Ordering::Acquire) {
                        break;
                    }
                }

                reader_phase_task.store(PtyReaderPhase::BeforeRead.as_raw(), Ordering::Release);
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        reader_phase_task.store(PtyReaderPhase::AfterRead.as_raw(), Ordering::Release);
                        record_output_millis(&task_last_output_at);
                        crate::terminal::metrics::record_pty_read(&metrics_session_id, n);
                        reader_phase_task.store(PtyReaderPhase::BeforeSend.as_raw(), Ordering::Release);
                        if reader_tx.blocking_send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                        reader_phase_task.store(PtyReaderPhase::AfterSend.as_raw(), Ordering::Release);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                        // Re-read the flag instead of the snapshot taken before this read: a stop
                        // requested DURING the read is precisely what made it return, so the snapshot
                        // is stale here and trusting it would issue one more read. The drain is
                        // unaffected - a read that still has buffered bytes returns Ok, not this - and
                        // a genuine EINTR with no stop still retries.
                        if reader_stop_requested_task.load(Ordering::Acquire) {
                            break;
                        }
                        continue;
                    }
                    #[cfg(unix)]
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        use std::os::fd::AsRawFd;
                        let mut poll = libc::pollfd {
                            fd: reader_poll.as_raw_fd(),
                            events: libc::POLLIN,
                            revents: 0,
                        };
                        if unsafe { libc::poll(&mut poll, 1, -1) } < 0
                            && std::io::Error::last_os_error().kind()
                                != std::io::ErrorKind::Interrupted
                        {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            reader_finished_task.store(true, Ordering::Release);
            reader_phase_task.store(PtyReaderPhase::Finished.as_raw(), Ordering::Release);
        });

        Self {
            input: config.input,
            input_gate: tokio::sync::Mutex::new(()),
            id: config.id,
            master: Arc::new(Mutex::new(Some(config.master))),
            reader_interrupt,
            master_release: Arc::new(Mutex::new(None)),
            writer: Arc::new(Mutex::new(Some(config.writer))),
            child: Arc::new(Mutex::new(Some(ProcessHandle::Spawned(config.child)))),
            reader_task: Arc::new(Mutex::new(Some(reader_task))),
            output_tx,
            worktree_path: config.worktree_path,
            reader_finished,
            reader_stop_requested,
            reader_phase,
            last_output_at,
            reaped: Arc::new(AtomicBool::new(false)),
            state: Arc::new(Mutex::new(PtySessionState::Starting)),
            cols: Arc::new(Mutex::new(config.cols)),
            rows: Arc::new(Mutex::new(config.rows)),
            output_hub: Arc::new(RwLock::new(None)),
            pause_requested,
            reader_paused,
            pause_released,
            close_grace: Arc::new(Mutex::new(None)),
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    /// Milliseconds since the PTY child last produced output, or `None` when no
    /// output has been read yet. Ground truth for auto-suspend decisions: a
    /// working agent keeps writing (spinners, redraws), so recent output must
    /// veto suspension even when a screen classifier mislabels the session idle.
    pub fn last_output_age_ms(&self) -> Option<u64> {
        last_output_age_from(self.last_output_at.load(Ordering::Relaxed))
    }

    pub fn state(&self) -> PtySessionState {
        self.state.lock().clone()
    }

    /// Kernel-observed job-control stop of the session's process. Windows has no queryable
    /// equivalent for `NtSuspendProcess`; callers combine this with the lifecycle registry.
    pub fn process_stopped(&self) -> bool {
        #[cfg(unix)]
        {
            self.pid().is_some_and(process_is_stopped)
        }
        #[cfg(not(unix))]
        {
            false
        }
    }

    #[cfg(unix)]
    pub fn raw_master_fd(&self) -> Option<std::os::unix::io::RawFd> {
        self.master.lock().as_ref().and_then(|m| m.as_raw_fd())
    }

    #[cfg(unix)]
    pub(crate) fn foreground_process_group(&self) -> std::io::Result<Option<u32>> {
        let master = self.master.lock();
        let Some(fd) = master.as_ref().and_then(|m| m.as_raw_fd()) else {
            return Ok(None);
        };
        // Hold the master lock across the syscall so teardown cannot recycle the fd.
        let group = unsafe { libc::tcgetpgrp(fd) };
        if group < 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(Some(group as u32))
        }
    }

    pub(crate) fn mark_running(&self) {
        let mut state = self.state.lock();
        if matches!(*state, PtySessionState::Starting) {
            *state = PtySessionState::Running;
        }
    }

    /// Marks the session closing. Deliberately does NOT stop the reader: closing is entered before
    /// the child is signalled, so stopping here would truncate output the process has not written
    /// yet. The stop is requested by the close paths once the process that produced the output is
    /// dead, and by `Drop` when the session is simply let go.
    pub(crate) fn begin_closing(&self) -> bool {
        let mut state = self.state.lock();
        match *state {
            PtySessionState::Starting | PtySessionState::Running => {
                *state = PtySessionState::Closing;
                true
            }
            PtySessionState::Closing
            | PtySessionState::Exited { .. }
            | PtySessionState::Failed { .. } => false,
        }
    }

    pub(crate) fn mark_exited(&self, code: Option<i32>) {
        *self.state.lock() = PtySessionState::Exited { code };
    }

    pub(crate) fn mark_failed(&self, reason: impl Into<String>) {
        *self.state.lock() = PtySessionState::Failed {
            reason: reason.into(),
        };
    }

    pub fn pid(&self) -> Option<u32> {
        self.child
            .lock()
            .as_ref()
            .and_then(|child| child.pid())
    }

    /// The shell's own process group. This is what handover records and what lifecycle
    /// signals target. It must never be the terminal's foreground job group
    /// (`tcgetpgrp`): a record holding a job's group makes Close kill only that job and
    /// leave the shell running. The kernel's live answer wins over a recorded group so a
    /// record written by an older daemon (which stored the job group) cannot misdirect.
    pub fn pgid(&self) -> Option<u32> {
        let pid = self.pid();
        #[cfg(unix)]
        {
            if let Some(group) = pid.and_then(live_process_group) {
                return Some(group);
            }
        }
        self.child
            .lock()
            .as_ref()
            .and_then(|child| child.process_group())
            .or(pid)
    }

    pub fn pause_reader(&self) {
        self.pause_requested.store(true, Ordering::Release);
    }

    pub fn resume_reader(&self) -> bool {
        self.pause_requested.swap(false, Ordering::AcqRel)
    }

    pub fn is_reader_paused(&self) -> bool {
        self.reader_paused.load(Ordering::Acquire)
    }

    /// Record the TERM grace a close is about to run on this session.
    ///
    /// Called before the close claims the session, so the value is published whichever close wins the
    /// race: a concurrent close reads it to size its fence, and keeping the maximum means a later
    /// close's shorter grace can never shrink the bound an earlier, longer one needs. The closing
    /// phase is claimed at most once (`begin_closing`), so the maximum is the grace of that close.
    pub(crate) fn note_close_grace(&self, grace: Duration) {
        let mut recorded = self.close_grace.lock();
        *recorded = Some(match *recorded {
            Some(existing) => existing.max(grace),
            None => grace,
        });
    }

    /// The TERM grace the close that claimed this session is running, if one has claimed it.
    ///
    /// `None` until a close records one; a caller that gets `None` still holds the session open and is
    /// about to run a grace phase of its own, so it has nothing to wait on.
    pub(crate) fn close_grace(&self) -> Option<Duration> {
        *self.close_grace.lock()
    }

    /// A reader parked by `pause_reader` never reads again on its own, so tearing the session
    /// down must release it, or its blocking thread outlives the session and stalls runtime
    /// shutdown. Only a paused reader is touched, and it exits without reading.
    ///
    /// The release is its own signal and NOT `reader_finished`: the close path reads that flag as
    /// proof the reader thread returned, so writing the release into it let a close that arrived
    /// while the reader was still parked pass stage 1 as if the reader had ended. The parked reader
    /// observes this flag, leaves its park without reading again, and sets `reader_finished` itself
    /// on the way out.
    fn release_paused_reader(&self) {
        if self.pause_requested.load(Ordering::Acquire) {
            self.pause_released.store(true, Ordering::Release);
        }
    }

    /// Stop this session's reader for good.
    ///
    /// Both halves are required, and the park is the half that is easy to miss: a reader parked by
    /// `pause_reader` waits on `pause_released` and no longer observes `reader_finished`, so setting
    /// the flag alone left the thread parked forever while `is_reader_finished()` already reported
    /// true - a close would pass its first stage on that flag without the thread ever ending, and a
    /// parked reader holds the last output sender. Releasing the park here is what makes "stopped"
    /// mean the thread really ends.
    ///
    /// The other half is the same lesson where the reader is NOT parked. `abort()` cannot end a
    /// `spawn_blocking` closure that has already started, so a reader blocked in `read()` needs the
    /// stop request - the flag, and on Windows the retained interrupt that makes the read return - or
    /// the abort ends nothing and only this function's own word said the reader had stopped. That is
    /// also why this no longer sets `reader_finished` itself: the flag means the reader thread
    /// returned, it is set by the reader as its own last act, and `Drop` - the other path that
    /// stops a reader without waiting for a tail - does not set it either.
    pub fn stop_reader(&self) {
        self.request_reader_stop();
        if let Some(handle) = self.reader_task.lock().take() {
            handle.abort();
        }
        self.pause_released.store(true, Ordering::Release);
    }

    pub fn set_output_hub(&self, hub: Arc<TerminalOutputHub>) {
        *self.output_hub.write() = Some(hub);
    }

    pub fn output_hub(&self) -> Option<Arc<TerminalOutputHub>> {
        self.output_hub.read().clone()
    }

    pub fn worktree_path(&self) -> Option<PathBuf> {
        self.worktree_path.clone()
    }

    pub const DEFAULT_PTY_WRITE_TIMEOUT: Duration = Duration::from_secs(10);

    pub fn write_input(&self, data: &[u8]) -> Result<(), PtyError> {
        self.write_input_with_deadline(data, Self::DEFAULT_PTY_WRITE_TIMEOUT)
    }

    #[cfg(unix)]
    fn poll_writable_deadline(
        &self,
        start: std::time::Instant,
        deadline: Duration,
    ) -> Result<(), PtyError> {
        use std::os::fd::AsRawFd;
        let elapsed = start.elapsed();
        if elapsed >= deadline {
            return Err(PtyError::IoError("PTY_INPUT_TIMEOUT".into()));
        }
        let remaining_duration = deadline - elapsed;
        let timeout_ms = i32::try_from(remaining_duration.as_millis())
            .unwrap_or(i32::MAX)
            .max(1);

        let mut poll = libc::pollfd {
            fd: self.input.get_ref().as_raw_fd(),
            events: libc::POLLOUT,
            revents: 0,
        };
        let res = unsafe { libc::poll(&mut poll, 1, timeout_ms) };
        if res == 0 {
            return Err(PtyError::IoError("PTY_INPUT_TIMEOUT".into()));
        }
        if res < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                if start.elapsed() >= deadline {
                    return Err(PtyError::IoError("PTY_INPUT_TIMEOUT".into()));
                }
                return Ok(());
            }
            return Err(PtyError::IoError(err.to_string()));
        }
        Ok(())
    }

    pub fn write_input_with_deadline(
        &self,
        data: &[u8],
        deadline: Duration,
    ) -> Result<(), PtyError> {
        let start = std::time::Instant::now();

        let mut writer = self.writer.lock();
        let writer = writer
            .as_mut()
            .ok_or_else(|| PtyError::IoError("PTY writer is closed".into()))?;
        let mut remaining = data;
        while !remaining.is_empty() {
            if start.elapsed() >= deadline {
                return Err(PtyError::IoError("PTY_INPUT_TIMEOUT".into()));
            }
            match writer.write(remaining) {
                Ok(0) => return Err(PtyError::IoError("PTY write returned zero".into())),
                Ok(n) => remaining = &remaining[n..],
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                    if start.elapsed() >= deadline {
                        return Err(PtyError::IoError("PTY_INPUT_TIMEOUT".into()));
                    }
                    continue;
                }
                #[cfg(unix)]
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    self.poll_writable_deadline(start, deadline)?;
                }
                Err(e) => return Err(PtyError::IoError(format!("Write failed: {e}"))),
            }
        }
        loop {
            if start.elapsed() >= deadline {
                return Err(PtyError::IoError("PTY_INPUT_TIMEOUT".into()));
            }
            match writer.flush() {
                Ok(()) => break,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                    if start.elapsed() >= deadline {
                        return Err(PtyError::IoError("PTY_INPUT_TIMEOUT".into()));
                    }
                    continue;
                }
                #[cfg(unix)]
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    self.poll_writable_deadline(start, deadline)?;
                }
                Err(e) => return Err(PtyError::IoError(format!("Flush failed: {e}"))),
            }
        }
        Ok(())
    }

    /// Test seam: replace the PTY writer so the WouldBlock stall path can be
    /// reproduced without depending on kernel input-queue limits.
    #[cfg(test)]
    pub(crate) fn set_writer_for_test(&self, writer: Box<dyn Write + Send>) {
        *self.writer.lock() = Some(writer);
    }

    /// Cancellation drops the pending readiness future: no worker owns bytes.
    /// Bytes accepted before cancellation cannot be withdrawn from the kernel.
    /// A partial failure must never be retried as a complete frame.
    #[cfg(unix)]
    pub async fn write_input_cancellable(&self, data: &[u8]) -> Result<(), PtyError> {
        use std::os::fd::AsRawFd;
        if data.len() > 65536 {
            return Err(PtyError::IoError("PTY_INPUT_TOO_LARGE".into()));
        }
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            let _gate = self.input_gate.lock().await;
            let mut remaining = data;
            while !remaining.is_empty() {
                let mut ready = self
                    .input
                    .writable()
                    .await
                    .map_err(|e| PtyError::IoError(e.to_string()))?;
                // Do not wait behind a synchronous legacy writer or closed I/O.
                let writer = self
                    .writer
                    .try_lock()
                    .ok_or_else(|| PtyError::IoError("PTY_INPUT_BUSY".into()))?;
                if writer.is_none() {
                    return Err(PtyError::IoError("PTY_INPUT_CLOSED".into()));
                }
                let state = self.state.lock();
                if !matches!(*state, PtySessionState::Starting | PtySessionState::Running) {
                    return Err(PtyError::IoError("PTY_INPUT_CLOSED".into()));
                }
                let result = ready.try_io(|fd| {
                    let n = unsafe {
                        libc::write(
                            fd.get_ref().as_raw_fd(),
                            remaining.as_ptr().cast(),
                            remaining.len(),
                        )
                    };
                    if n < 0 {
                        Err(std::io::Error::last_os_error())
                    } else {
                        Ok(n as usize)
                    }
                });
                drop(state);
                drop(writer);
                match result {
                    Ok(Ok(0)) => return Err(PtyError::IoError("PTY write returned zero".into())),
                    Ok(Ok(n)) => remaining = &remaining[n..],
                    Ok(Err(e)) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Ok(Err(e)) => return Err(PtyError::IoError(e.to_string())),
                    Err(_) => continue,
                }
            }
            Ok(())
        })
        .await
        .map_err(|_| PtyError::IoError("PTY_INPUT_TIMEOUT".into()))?
    }

    #[cfg(windows)]
    pub async fn write_input_cancellable(&self, data: &[u8]) -> Result<(), PtyError> {
        if data.len() > 65536 {
            return Err(PtyError::IoError("PTY_INPUT_TOO_LARGE".into()));
        }
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            let _gate = self.input_gate.lock().await;
            if self.writer.try_lock().is_none_or(|w| w.is_none()) {
                return Err(PtyError::IoError("PTY_INPUT_BUSY_OR_CLOSED".into()));
            }
            let mut bytes = data;
            while !bytes.is_empty() {
                let n = self.write_input_slice(bytes)?;
                if n == 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                } else {
                    bytes = &bytes[n..];
                }
            }
            Ok(())
        })
        .await
        .map_err(|_| PtyError::IoError("PTY_INPUT_TIMEOUT".into()))?
    }

    /// One synchronous pump iteration for the Windows ConPTY input path. Every
    /// lock is acquired and released inside this plain frame, so no guard or
    /// raw-handle-bearing input value is ever live across the caller's await
    /// point: the ConPTY input type made the surrounding async block !Send and
    /// broke the axum WebSocket upgrade future.
    #[cfg(windows)]
    fn write_input_slice(&self, bytes: &[u8]) -> Result<usize, PtyError> {
        let writer = self
            .writer
            .try_lock()
            .ok_or_else(|| PtyError::IoError("PTY_INPUT_BUSY".into()))?;
        let state = self.state.lock();
        if writer.is_none()
            || !matches!(*state, PtySessionState::Starting | PtySessionState::Running)
        {
            return Err(PtyError::IoError("PTY_INPUT_CLOSED".into()));
        }
        self.input
            .try_write(bytes)
            .map_err(|e| PtyError::IoError(e.to_string()))
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), PtyError> {
        let size = PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };
        let master = self.master.lock();
        let master = master
            .as_ref()
            .ok_or_else(|| PtyError::ResizeError("PTY master is closed".into()))?;
        master
            .resize(size)
            .map_err(|e| PtyError::ResizeError(format!("Resize failed: {e}")))?;
        *self.cols.lock() = cols;
        *self.rows.lock() = rows;
        Ok(())
    }

    pub fn get_size(&self) -> (u16, u16) {
        (*self.cols.lock(), *self.rows.lock())
    }

    pub fn kill(&self) -> Result<(), PtyError> {
        let mut child = self.child.lock();
        let Some(child) = child.as_mut() else {
            return Ok(());
        };
        child.kill()
    }

    #[cfg(unix)]
    pub fn signal(&self, signal: TerminalSignal) -> Result<(), PtyError> {
        if signal == TerminalSignal::Interrupt {
            // A literal VINTR byte goes through the PTY line discipline, which directs
            // SIGINT to the terminal's current foreground process group.
            return self.write_input(&[0x03]);
        }

        let pid = self
            .pid()
            .ok_or_else(|| PtyError::KillError("PID not available for signal".into()))?;
        let sig = match signal {
            TerminalSignal::Interrupt => libc::SIGINT,
            TerminalSignal::Terminate => libc::SIGTERM,
            TerminalSignal::Kill => libc::SIGKILL,
            TerminalSignal::Stop => libc::SIGSTOP,
            TerminalSignal::Continue => libc::SIGCONT,
        };

        // portable-pty creates the child as the PTY session/process-group leader on Unix.
        // Addressing the negative group signals the shell's whole process group rather
        // than only the shell process.
        let shell_group = self.pgid().unwrap_or(pid);
        // Termination must also reach a job that currently owns the terminal from its own
        // group (job control puts every foreground job in a separate group). Read it before
        // signalling, while the shell still holds the terminal.
        let foreground_group = match signal {
            TerminalSignal::Terminate | TerminalSignal::Kill => self
                .foreground_process_group()
                .ok()
                .flatten()
                .filter(|group| *group > 1 && *group != shell_group),
            _ => None,
        };

        let mut delivered = unsafe { libc::kill(-(shell_group as i32), sig) } == 0
            // Fallback to direct PID if process group signaling failed
            || unsafe { libc::kill(pid as i32, sig) } == 0;
        if let Some(group) = foreground_group {
            delivered |= unsafe { libc::kill(-(group as i32), sig) } == 0;
        }
        if delivered {
            return Ok(());
        }

        Err(PtyError::KillError(format!(
            "Failed to send signal {signal:?} to process group {pid}: {}",
            std::io::Error::last_os_error()
        )))
    }

    #[cfg(not(unix))]
    pub fn signal(&self, signal: TerminalSignal) -> Result<(), PtyError> {
        match signal {
            TerminalSignal::Interrupt => self.write_input(&[0x03]),
            TerminalSignal::Terminate | TerminalSignal::Kill => self.kill(),
            #[cfg(windows)]
            TerminalSignal::Stop => {
                let pid = self
                    .pid()
                    .ok_or_else(|| PtyError::KillError("PID not available for signal".into()))?;
                windows_suspend::suspend_process(pid).map_err(PtyError::Other)
            }
            #[cfg(windows)]
            TerminalSignal::Continue => {
                let pid = self
                    .pid()
                    .ok_or_else(|| PtyError::KillError("PID not available for signal".into()))?;
                windows_suspend::resume_process(pid).map_err(PtyError::Other)
            }
            #[cfg(not(windows))]
            TerminalSignal::Stop | TerminalSignal::Continue => Err(PtyError::Other(
                "Process suspend/resume is not supported on this platform".into(),
            )),
        }
    }

    pub(crate) fn poll_exit_code(&self) -> Result<Option<i32>, PtyError> {
        let mut child_slot = self.child.lock();
        let Some(process) = child_slot.as_mut() else {
            return Ok(match self.state() {
                PtySessionState::Exited { code } => code,
                _ => None,
            });
        };

        match process {
            ProcessHandle::Spawned(child) => {
                let status = child
                    .try_wait()
                    .map_err(|e| PtyError::Other(format!("try_wait failed: {e}")))?;
                let Some(status) = status else {
                    return Ok(None);
                };

                let code = status.exit_code() as i32;
                child_slot.take();
                self.reaped.store(true, Ordering::Release);
                Ok(Some(code))
            }
            #[cfg(unix)]
            ProcessHandle::Adopted(adopted) => {
                if adopted.is_alive() {
                    Ok(None)
                } else {
                    child_slot.take();
                    self.reaped.store(true, Ordering::Release);
                    let code = match self.state() {
                        PtySessionState::Exited { code } => code,
                        _ => Some(0),
                    };
                    Ok(code)
                }
            }
            #[cfg(not(unix))]
            ProcessHandle::Adopted(_) => Ok(None),
        }
    }

    pub(crate) fn wait_and_reap(&self) -> Result<Option<i32>, PtyError> {
        let mut child_slot = self.child.lock();
        let Some(mut process) = child_slot.take() else {
            return Ok(match self.state() {
                PtySessionState::Exited { code } => code,
                _ => None,
            });
        };

        let result = match &mut process {
            ProcessHandle::Spawned(child) => child
                .wait()
                .map(|status| Some(status.exit_code() as i32))
                .map_err(|e| PtyError::Other(format!("wait failed: {e}"))),
            #[cfg(unix)]
            ProcessHandle::Adopted(adopted) => {
                let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
                while adopted.is_alive() && std::time::Instant::now() < deadline {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                let code = match self.state() {
                    PtySessionState::Exited { code } => code,
                    _ => Some(0),
                };
                Ok(code)
            }
            #[cfg(not(unix))]
            ProcessHandle::Adopted(_) => Ok(None),
        };
        self.reaped.store(true, Ordering::Release);
        result
    }

    pub fn is_alive(&self) -> bool {
        matches!(self.poll_exit_code(), Ok(None)) && !self.is_reaped()
    }

    pub fn is_reaped(&self) -> bool {
        self.reaped.load(Ordering::Acquire)
    }

    pub fn is_reader_finished(&self) -> bool {
        self.reader_finished.load(Ordering::Acquire)
    }

    pub(crate) fn output_receiver_closed(&self) -> bool {
        self.output_tx
            .lock()
            .as_ref()
            .map(mpsc::Sender::is_closed)
            .unwrap_or(true)
    }

    /// Ask this session's reader to stop.
    ///
    /// Two halves, both required: the flag ends the loop once the read in flight returns, and on
    /// Windows the master's cancellation event is what makes that read return. The flag alone
    /// cannot end a read the kernel still owns, and the event alone would leave the loop waiting
    /// for a read to answer it again.
    pub(crate) fn request_reader_stop(&self) {
        self.reader_stop_requested.store(true, Ordering::Release);
        // The retained handle, not the master: this must work on a session that has already given
        // its master up, which is exactly what a failed handover export leaves behind. Cloned out of
        // the slot first so no lock is held across the request.
        let interrupt = self.reader_interrupt.lock().clone();
        if let Some(interrupt) = interrupt {
            interrupt.request();
        }
    }

    /// Arm a subscription to this session's outstanding-read gauge and wait on it, bounded.
    ///
    /// Test-only. It blocks, so the tests drive it on a blocking thread; production never waits on
    /// this gauge.
    #[cfg(all(test, windows))]
    pub(crate) fn await_outstanding_reader_for_test(&self, timeout: Duration) -> bool {
        let interrupt = self.reader_interrupt.lock().clone();
        match interrupt {
            Some(interrupt) => interrupt.await_outstanding_read(timeout),
            None => false,
        }
    }

    pub(crate) fn close_output(&self) {
        self.output_tx.lock().take();
    }

    /// The reader's current phase, for diagnostics only.
    pub(crate) fn reader_phase(&self) -> &'static str {
        PtyReaderPhase::from_raw(self.reader_phase.load(Ordering::Acquire)).as_str()
    }

    pub(crate) fn close_io(&self) {
        self.writer.lock().take();
        self.release_master();
        self.release_paused_reader();
    }

    /// Give up the master, handing the drop to the runtime's blocking pool.
    ///
    /// Dropping the master runs `ClosePseudoConsole`, which waits for the console host to exit and
    /// flushes the output the host still holds - the operation that can still produce the last bytes
    /// of a pane. It can also wait on a client that has stopped reading, so the thread that drops it
    /// must be one nothing else depends on: the close path has to stay free to apply the bounded
    /// cancellation, and the reader is the thread that lets the host finish. The slot is emptied
    /// synchronously, so a caller that checks for a live master (export, resize) sees it gone at
    /// once - only the drop itself is off-thread.
    ///
    /// The handle the pool returns is KEPT, because the pool is a thread budget and not a duration
    /// bound. A pool whose blocking threads are all busy leaves the drop queued with the master alive,
    /// and a pool that is shutting down never runs it at all - in that case the pool has already
    /// dropped the closure on the thread that called `spawn_blocking`, which is the very reactor this
    /// seam exists to keep free. Neither outcome is visible through a discarded handle, so the close
    /// observes the handle it kept (`observe_master_release`) and reports a release that did not
    /// happen instead of reading it as a completed one. The cancelled case RESOLVES that handle rather
    /// than leaving it pending, which is why the observation reads the closure's own result: a handle
    /// that finished is not a closure that ran.
    fn release_master(&self) {
        let master = self.master.lock().take();
        let Some(master) = master else {
            return;
        };
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let release = handle.spawn_blocking(move || drop(master));
            *self.master_release.lock() = Some(release);
            return;
        }
        // Outside a runtime this thread is not a reactor, so dropping it here cannot block one: the
        // caller waits and the release is complete when this returns, which is why nothing is left
        // for `observe_master_release` to observe.
        tracing::debug!("releasing PTY master on the calling thread (no runtime is entered)");
        drop(master);
    }

    /// Wait, bounded, for the master release this session handed to the blocking pool.
    ///
    /// `None` when nothing was handed over: no master was left to drop, or there was no runtime to
    /// enter and the drop already ran inline on the caller. `Some(true)` only when the pool RAN the
    /// drop within `timeout`. `Some(false)` when it did not, which is two conditions rather than one:
    /// a pool whose blocking threads are all busy leaves the release queued with the master alive, and
    /// a pool that is already shutting down cancels the task and drops the closure - and the master
    /// with it - inline on the thread that called `spawn_blocking`, which is the very reactor this
    /// seam exists to keep free. The cancelled handle still RESOLVES, so the wait must read its result
    /// and not merely whether it finished, or that inline drop would be reported as a release that
    /// happened on the pool.
    ///
    /// The handle is taken out of its slot, so two callers cannot wait on the same release and a
    /// release that was already observed is not waited on twice.
    pub(crate) async fn observe_master_release(&self, timeout: Duration) -> Option<bool> {
        let release = self.master_release.lock().take()?;
        match tokio::time::timeout(timeout, release).await {
            // The closure ran: the master was dropped on the pool's own thread.
            Ok(Ok(())) => Some(true),
            // The wait finished without the closure running. `spawn_blocking` returns its handle
            // unchanged when the pool is already shutting down, but `spawn_task` has shutdown()-ed the
            // task by then, so the handle resolves with a cancellation instead of a run: the drop
            // happened inline on the caller. A pool that is merely saturated never resolves the handle
            // at all and lands in the timeout arm below. Neither is a release.
            Ok(Err(_)) => Some(false),
            Err(_) => Some(false),
        }
    }

    pub(crate) fn take_reader_task(&self) -> Option<JoinHandle<()>> {
        self.reader_task.lock().take()
    }

    /// Exports session state, metadata, output-hub snapshot, and duplicated master raw fd
    /// for ownership transfer to another daemon process (design doc sections 7.2, 9.2, 11).
    #[cfg(unix)]
    pub fn export_for_transfer(&self) -> Result<PtySessionExport, PtyError> {
        let hub = self.output_hub.read().clone();
        self.export_for_transfer_with_hub(hub.as_deref())
    }

    /// Variant of `export_for_transfer` allowing an explicit `TerminalOutputHub` reference.
    #[cfg(unix)]
    pub fn export_for_transfer_with_hub(
        &self,
        hub: Option<&TerminalOutputHub>,
    ) -> Result<PtySessionExport, PtyError> {
        use std::os::fd::{AsRawFd, FromRawFd};

        let master_raw_fd = {
            let master_lock = self.master.lock();
            let master = master_lock
                .as_ref()
                .ok_or_else(|| PtyError::IoError("PTY master is closed".into()))?;
            let raw = master
                .as_raw_fd()
                .ok_or_else(|| PtyError::IoError("PTY descriptor unavailable".into()))?;
            let dup_fd = unsafe { libc::fcntl(raw, libc::F_DUPFD_CLOEXEC, 0) };
            if dup_fd < 0 {
                return Err(PtyError::IoError(format!(
                    "Failed to duplicate master PTY fd: {}",
                    std::io::Error::last_os_error()
                )));
            }
            dup_fd
        };

        let master_fd = unsafe { std::os::fd::OwnedFd::from_raw_fd(master_raw_fd) };

        let pid = self.pid();
        let pgid = self.pgid();
        let (cols, rows) = self.get_size();
        let worktree_path = self.worktree_path();
        let state = self.state();

        let hub_snapshot = hub.and_then(|h| h.export_session_state(&self.id));

        Ok(PtySessionExport {
            session_id: self.id.clone(),
            pid,
            pgid,
            cols,
            rows,
            worktree_path,
            state,
            hub_snapshot,
            master_raw_fd,
            master_fd,
        })
    }

    /// Adoption constructor that rebuilds a `PtySession` from an adopted master PTY descriptor
    /// and snapshot WITHOUT spawning a command (design doc section 11 'Adopted process lifecycle').
    /// Starts the reader task fresh from the transferred master and represents the child as an
    /// `AdoptedProcess` whose liveness is observed via `libc::kill(pid, 0)`.
    #[cfg(unix)]
    pub fn adopt_from_transfer(
        master: std::os::fd::OwnedFd,
        snapshot: PtySessionSnapshot,
    ) -> Result<(Self, mpsc::Receiver<Vec<u8>>), PtyError> {
        use std::os::fd::{AsFd, AsRawFd, FromRawFd};

        let _runtime = tokio::runtime::Handle::try_current().map_err(|error| {
            PtyError::IoError(format!("Tokio runtime required for adoption: {error}"))
        })?;

        // Reconstruct UnixMasterPty from the transferred master OwnedFd
        let mut master_pty = portable_pty::master_from_owned_fd(master, None)
            .map_err(|e| PtyError::PtyCreationError(format!("Failed to adopt master PTY fd: {e}")))?;

        // Mirror the existing nonblocking-input pattern in PtyManager::spawn_with_id_and_worktree
        let raw = master_pty
            .as_raw_fd()
            .ok_or_else(|| PtyError::IoError("PTY descriptor unavailable".into()))?;
        let duplicate = unsafe { libc::fcntl(raw, libc::F_DUPFD_CLOEXEC, 0) };
        if duplicate < 0 {
            return Err(PtyError::IoError(std::io::Error::last_os_error().to_string()));
        }
        let fd = unsafe { std::os::fd::OwnedFd::from_raw_fd(duplicate) };
        let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
        if flags < 0
            || unsafe {
                libc::fcntl(
                    fd.as_raw_fd(),
                    libc::F_SETFL,
                    flags | libc::O_NONBLOCK,
                )
            } < 0
        {
            return Err(PtyError::IoError(std::io::Error::last_os_error().to_string()));
        }
        let input = tokio::io::unix::AsyncFd::new(fd)
            .map_err(|e| PtyError::IoError(e.to_string()))?;

        let reader = master_pty
            .try_clone_reader()
            .map_err(|e| PtyError::IoError(format!("Failed to clone reader: {e}")))?;
        // Taken before the master moves into the session, so an adopted session can end its reader
        // just as a spawned one can.
        let reader_interrupt = master_pty.interrupt_handle();

        let writer = master_pty
            .take_writer()
            .map_err(|e| PtyError::IoError(format!("Failed to take writer: {e}")))?;

        if snapshot.cols > 0 && snapshot.rows > 0 {
            let _ = master_pty.resize(PtySize {
                rows: snapshot.rows,
                cols: snapshot.cols,
                pixel_width: 0,
                pixel_height: 0,
            });
        }

        let (tx, rx) = mpsc::channel::<Vec<u8>>(1024);
        let output_tx = Arc::new(Mutex::new(Some(tx)));
        let reader_tx = output_tx
            .lock()
            .as_ref()
            .expect("output sender must exist while starting reader")
            .clone();

        let reader_poll = input
            .get_ref()
            .as_fd()
            .try_clone_to_owned()
            .expect("duplicate PTY poll descriptor");

        let metrics_session_id = snapshot.session_id.clone();
        let reader_finished = Arc::new(AtomicBool::new(false));
        let reader_finished_task = Arc::clone(&reader_finished);
        let reader_stop_requested = Arc::new(AtomicBool::new(false));
        let reader_stop_requested_task = Arc::clone(&reader_stop_requested);
        let reader_last_output_at = Arc::new(AtomicU64::new(0));
        let last_output_at = Arc::clone(&reader_last_output_at);
        let task_last_output_at = Arc::clone(&last_output_at);
        let pause_requested = Arc::new(AtomicBool::new(false));
        let reader_paused = Arc::new(AtomicBool::new(false));
        let pause_requested_task = Arc::clone(&pause_requested);
        let reader_paused_task = Arc::clone(&reader_paused);
        let pause_released = Arc::new(AtomicBool::new(false));
        let pause_released_task = Arc::clone(&pause_released);
        let reader_phase = Arc::new(AtomicU64::new(PtyReaderPhase::Starting.as_raw()));
        let reader_phase_task = Arc::clone(&reader_phase);

        let reader_task = tokio::task::spawn_blocking(move || {
            let mut reader = reader;
            let mut buf = [0u8; 4096];
            loop {
                // A requested stop ends the loop once the pipe has nothing left to deliver. It is a
                // variable rather than an immediate `break` because one read carries at most one
                // buffer: the read below keeps draining while bytes remain, and the loop ends on the
                // read that reports the cancellation with nothing buffered. Breaking here instead
                // would drop a tail larger than one buffer that the pipe is still holding.
                let stopping = reader_stop_requested_task.load(Ordering::Acquire);
                if !stopping && pause_requested_task.load(Ordering::Acquire) {
                    reader_paused_task.store(true, Ordering::Release);
                    while pause_requested_task.load(Ordering::Acquire)
                        && !pause_released_task.load(Ordering::Acquire)
                    {
                        std::thread::sleep(std::time::Duration::from_millis(20));
                    }
                    reader_paused_task.store(false, Ordering::Release);
                    // Released by teardown (`release_paused_reader`), not by a resume: the
                    // descriptor may already belong to a successor daemon, so never read again. The
                    // release is its own signal, so it is not mistaken for the reader's own end,
                    // which is the flag this reader sets when it really returns.
                    if pause_released_task.load(Ordering::Acquire) {
                        break;
                    }
                }

                reader_phase_task.store(PtyReaderPhase::BeforeRead.as_raw(), Ordering::Release);
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        reader_phase_task.store(PtyReaderPhase::AfterRead.as_raw(), Ordering::Release);
                        record_output_millis(&task_last_output_at);
                        crate::terminal::metrics::record_pty_read(&metrics_session_id, n);
                        reader_phase_task.store(PtyReaderPhase::BeforeSend.as_raw(), Ordering::Release);
                        if reader_tx.blocking_send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                        reader_phase_task.store(PtyReaderPhase::AfterSend.as_raw(), Ordering::Release);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                        // Re-read the flag rather than the snapshot taken before this read: a stop
                        // requested during the read is what made it return.
                        if reader_stop_requested_task.load(Ordering::Acquire) {
                            break;
                        }
                        continue;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        let mut poll = libc::pollfd {
                            fd: reader_poll.as_raw_fd(),
                            events: libc::POLLIN,
                            revents: 0,
                        };
                        if unsafe { libc::poll(&mut poll, 1, -1) } < 0
                            && std::io::Error::last_os_error().kind()
                                != std::io::ErrorKind::Interrupted
                        {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            reader_finished_task.store(true, Ordering::Release);
            reader_phase_task.store(PtyReaderPhase::Finished.as_raw(), Ordering::Release);
        });

        let child_handle = snapshot.pid.map(|pid| {
            ProcessHandle::Adopted(AdoptedProcess {
                pid,
                process_group: snapshot.pgid,
            })
        });

        let session = Self {
            input,
            input_gate: tokio::sync::Mutex::new(()),
            id: snapshot.session_id,
            master: Arc::new(Mutex::new(Some(master_pty))),
            reader_interrupt: Arc::new(Mutex::new(reader_interrupt)),
            master_release: Arc::new(Mutex::new(None)),
            writer: Arc::new(Mutex::new(Some(writer))),
            child: Arc::new(Mutex::new(child_handle)),
            reader_task: Arc::new(Mutex::new(Some(reader_task))),
            output_tx,
            worktree_path: snapshot.worktree_path,
            reader_finished,
            reader_stop_requested,
            reader_phase,
            last_output_at,
            reaped: Arc::new(AtomicBool::new(false)),
            state: Arc::new(Mutex::new(snapshot.state)),
            cols: Arc::new(Mutex::new(snapshot.cols)),
            rows: Arc::new(Mutex::new(snapshot.rows)),
            output_hub: Arc::new(RwLock::new(None)),
            pause_requested,
            reader_paused,
            pause_released,
            close_grace: Arc::new(Mutex::new(None)),
        };

        Ok((session, rx))
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        self.writer.lock().take();
        // A session dropped without an explicit close must still not leave its reader parked on the
        // pipe: the blocking thread would outlive the session and hold runtime shutdown open. Drop
        // cannot wait for a tail and it closes the output channel a few lines below, so there is no
        // consumer left for one: the cancellation is the only thing that can end the read here, and
        // it is applied directly instead of after a drain.
        self.request_reader_stop();
        self.release_master();
        self.release_paused_reader();
        self.output_tx.lock().take();
        if let Some(handle) = self.reader_task.lock().take() {
            handle.abort();
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn output_age_is_none_before_first_output() {
        assert_eq!(last_output_age_from(0), None);
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn process_stop_state_tracks_sigstop_and_sigcont() {
        let mut child = std::process::Command::new("sleep").arg("30").spawn().unwrap();
        let pid = child.id();
        let wait_for = |want: bool| {
            // Signal delivery is asynchronous; the kernel state settles within milliseconds.
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while process_is_stopped(pid) != want {
                assert!(std::time::Instant::now() < deadline, "stop state never became {want}");
                std::thread::yield_now();
            }
        };
        assert!(!process_is_stopped(pid));
        unsafe { libc::kill(pid as i32, libc::SIGSTOP) };
        wait_for(true);
        unsafe { libc::kill(pid as i32, libc::SIGCONT) };
        wait_for(false);
        child.kill().unwrap();
        child.wait().unwrap();
    }

    #[test]
    fn output_age_measures_elapsed_since_last_chunk() {
        let now_millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let age = last_output_age_from(now_millis - 5_000).expect("age must be set for a stamped output");
        assert!(age >= 5_000, "age must count from the stamped output, got {age}");
    }

    #[test]
    fn record_output_millis_stamps_a_fresh_timestamp() {
        let target = AtomicU64::new(0);
        record_output_millis(&target);
        assert_ne!(target.load(Ordering::Relaxed), 0);
    }

    fn test_adopted_process_liveness_observation() {
        // Real process (current test process) must be observed as alive
        let my_pid = std::process::id();
        let alive_proc = AdoptedProcess {
            pid: my_pid,
            process_group: None,
        };
        assert!(alive_proc.is_alive(), "current process must be alive");

        // Non-existent PID must be observed as dead (ESRCH)
        let dead_proc = AdoptedProcess {
            pid: 999_999_999,
            process_group: None,
        };
        assert!(!dead_proc.is_alive(), "non-existent process must not be alive");
    }

    #[test]
    fn test_session_snapshot_serialization_roundtrip() {
        let snapshot = PtySessionSnapshot {
            session_id: "test-roundtrip-id".to_string(),
            pid: Some(12345),
            pgid: Some(12340),
            cols: 120,
            rows: 40,
            worktree_path: Some(PathBuf::from("/tmp/wt")),
            state: PtySessionState::Running,
            hub_snapshot: None,
        };

        let json = serde_json::to_string(&snapshot).expect("serialize snapshot");
        let decoded: PtySessionSnapshot = serde_json::from_str(&json).expect("deserialize snapshot");
        assert_eq!(decoded, snapshot);
    }

    #[test]
    fn test_write_input_with_deadline_times_out_when_writer_stalls() {
        use portable_pty::CommandBuilder;

        struct WouldBlockWriter;
        impl std::io::Write for WouldBlockWriter {
            fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::from(std::io::ErrorKind::WouldBlock))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let rt = tokio::runtime::Runtime::new().expect("create tokio runtime");
        let _guard = rt.enter();

        let manager = crate::terminal::PtyManager::new();
        let mut cmd = CommandBuilder::new("/bin/sh");
        cmd.arg("-c");
        cmd.arg("sleep 30");
        let (session_id, _rx) = manager.spawn(cmd, 80, 24).expect("spawn pty session");
        let session = manager.get_session(&session_id).expect("session exists");

        let (tx, rx) = std::sync::mpsc::channel();
        let session_clone = Arc::clone(&session);
        let handle = std::thread::spawn(move || {
            // A real slave input queue does not stall the master writer on this
            // platform, so install a writer that always reports WouldBlock to
            // reproduce the stall the deadline exists for.
            session_clone.set_writer_for_test(Box::new(WouldBlockWriter));
            let chunk = vec![b'A'; 4096];
            let deadline = Duration::from_millis(300);
            let call_start = std::time::Instant::now();
            let res = match session_clone.write_input_with_deadline(&chunk, deadline) {
                Ok(()) => panic!("bounded write succeeded while the writer reported WouldBlock"),
                Err(err) => (err, call_start.elapsed()),
            };
            let _ = tx.send(res);
        });

        let (err, call_elapsed) = rx
            .recv_timeout(Duration::from_secs(3))
            .expect("worker thread must complete within 3 seconds");
        let join_result = handle.join();
        assert!(join_result.is_ok(), "worker thread join succeeded");
        let total_wall_time = call_elapsed;

        let err_msg = err.to_string();
        assert!(
            err_msg.contains("PTY_INPUT_TIMEOUT"),
            "expected error containing PTY_INPUT_TIMEOUT, got: {err_msg}"
        );
        assert!(
            total_wall_time < Duration::from_secs(3),
            "total wall time must stay under 3 seconds, got {total_wall_time:?}"
        );
        assert!(
            call_elapsed < Duration::from_secs(3),
            "call elapsed time must stay under 3 seconds, got {call_elapsed:?}"
        );

        let _ = session.kill();
    }

    #[test]
    fn test_write_input_with_deadline_times_out_when_writer_returns_interrupted() {
        use portable_pty::CommandBuilder;

        struct InterruptedWriter;
        impl std::io::Write for InterruptedWriter {
            fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::from(std::io::ErrorKind::Interrupted))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let rt = tokio::runtime::Runtime::new().expect("create tokio runtime");
        let _guard = rt.enter();

        let manager = crate::terminal::PtyManager::new();
        let mut cmd = CommandBuilder::new("/bin/sh");
        cmd.arg("-c");
        cmd.arg("sleep 30");
        let (session_id, _rx) = manager.spawn(cmd, 80, 24).expect("spawn pty session");
        let session = manager.get_session(&session_id).expect("session exists");

        let (tx, rx) = std::sync::mpsc::channel();
        let session_clone = Arc::clone(&session);
        let handle = std::thread::spawn(move || {
            session_clone.set_writer_for_test(Box::new(InterruptedWriter));
            let chunk = vec![b'A'; 4096];
            let deadline = Duration::from_millis(300);
            let call_start = std::time::Instant::now();
            let res = match session_clone.write_input_with_deadline(&chunk, deadline) {
                Ok(()) => panic!("bounded write succeeded while the writer reported Interrupted"),
                Err(err) => (err, call_start.elapsed()),
            };
            let _ = tx.send(res);
        });

        let (err, call_elapsed) = rx
            .recv_timeout(Duration::from_secs(3))
            .expect("worker thread must complete within 3 seconds");
        let join_result = handle.join();
        assert!(join_result.is_ok(), "worker thread join succeeded");

        let err_msg = err.to_string();
        assert!(
            err_msg.contains("PTY_INPUT_TIMEOUT"),
            "expected error containing PTY_INPUT_TIMEOUT, got: {err_msg}"
        );
        assert!(
            call_elapsed < Duration::from_secs(3),
            "call elapsed time must stay under 3 seconds, got {call_elapsed:?}"
        );

        let _ = session.kill();
    }
}

/// The cancellable Windows reader, exercised through the same public API the daemon uses: a real
/// ConPTY pair, its real output pipe, and the master's cancellation event.
///
/// Every read that could block is driven on its own thread and observed through a bounded channel
/// wait, so a reader that fails to return FAILS the test instead of hanging the suite.
#[cfg(all(test, windows))]
mod windows_reader_tests {
    use super::*;
    use portable_pty::{native_pty_system, CommandBuilder, PtySystem};

    fn size() -> PtySize {
        PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        }
    }

    /// A read issued after cancellation must return, and must not be read as EOF. The handle is
    /// what is retained, so the request goes through it rather than through the master.
    #[test]
    fn a_reader_cancelled_before_its_read_returns_instead_of_reading() {
        let system = native_pty_system();
        let pair = system.openpty(size()).expect("a ConPTY pair");
        let mut reader = pair.master.try_clone_reader().expect("a reader");
        let handle = pair
            .master
            .interrupt_handle()
            .expect("a cancellable master offers a handle");

        handle.request();
        let mut buf = [0u8; 64];
        let error = reader
            .read(&mut buf)
            .expect_err("a cancelled reader must not block on a read");
        assert_eq!(error.kind(), std::io::ErrorKind::Interrupted);

        // Idempotent: asking twice is not an error, and the reader stays cancelled.
        handle.request();
        let mut buf = [0u8; 64];
        assert!(
            reader.read(&mut buf).is_err(),
            "a second read after cancellation must return an error rather than block"
        );
    }

    /// The hole this seam exists for: a session that has already given its master up - exactly what
    /// a failed handover export leaves behind - must still be able to end its reader when it closes.
    /// The master is gone, so only the retained handle can reach that reader, and here the stream is
    /// never going to end on its own, so the cancellation is what returns the parked read.
    ///
    /// The "closing must not stop the reader" half of the contract is asserted on unix, where it is
    /// deterministic (`a_failed_export_leaves_the_session_readable_by_the_predecessor`). Asserting it
    /// here would race the console host exiting and closing the pipe by itself, which is a correct
    /// way for this reader to end - a legitimate EOF, not a failure of the seam.
    #[tokio::test]
    async fn a_session_whose_master_is_gone_can_still_end_its_reader() {
        let manager = crate::terminal::PtyManager::new();
        let (session_id, mut rx) = manager
            .spawn(CommandBuilder::new("cmd.exe"), 80, 24)
            .expect("spawn a ConPTY session");
        let session = manager.get_session(&session_id).expect("session registered");

        // ARMED BEFORE THE CLOSE: the gauge is subscribed to on a blocking thread, which waits on
        // the state itself - a read the kernel owns, right now - instead of polling for it.
        assert!(
            await_session_outstanding_read(&session).await,
            "the reader never parked in a read the kernel owns"
        );

        // The master goes away the way a failed export takes it, leaving the reader parked on a
        // pipe whose write end the console host still holds. Whether the host then exits by itself
        // or never does, this close must end the reader, and the assertions below are what prove it.
        session.close_io();

        session.request_reader_stop();

        // The reader holds the last output sender, so this channel closing is the exact moment the
        // reader thread exited - an event to await, not a delay to wait out.
        let ended = tokio::time::timeout(Duration::from_secs(10), async {
            while rx.recv().await.is_some() {}
        })
        .await;
        assert!(
            ended.is_ok(),
            "a close after a failed export left the reader running"
        );
        assert!(
            session.is_reader_finished(),
            "the reader must report finished once its read was ended"
        );
    }

    /// The close regression itself, on the platform it was found on: closing must end the reader
    /// rather than wait out a shutdown timeout, and the session must leave the registry.
    #[tokio::test]
    async fn closing_a_session_ends_its_reader_and_reports_no_timeout() {
        let manager = crate::terminal::PtyManager::new();
        let (session_id, mut rx) = manager
            .spawn(CommandBuilder::new("cmd.exe"), 80, 24)
            .expect("spawn a ConPTY session");
        let session = manager.get_session(&session_id).expect("session registered");
        // ARMED BEFORE THE CLOSE, and driven off the runtime because the subscription blocks.
        assert!(
            await_session_outstanding_read(&session).await,
            "the reader never parked in a read the kernel owns"
        );

        tokio::time::timeout(Duration::from_secs(20), manager.close_session(&session_id))
            .await
            .expect("close must be bounded")
            .expect("close must not report a reader that would not stop");

        assert!(!manager.has_session(&session_id), "a closed session leaves the registry");
        let ended = tokio::time::timeout(Duration::from_secs(10), async {
            while rx.recv().await.is_some() {}
        })
        .await;
        assert!(ended.is_ok(), "close left the reader running");
        assert!(session.is_reader_finished(), "close must end the session's reader");
    }
}

/// Subscribe to a session's outstanding-read gauge and wait for it, bounded.
///
/// The wait blocks, so it runs on a blocking thread: the test thread is a runtime thread, and the
/// subscription is on the state itself rather than a poll loop, which is what keeps this a
/// subscription and not a spin.
#[cfg(all(test, windows))]
async fn await_session_outstanding_read(session: &Arc<PtySession>) -> bool {
    let session = Arc::clone(session);
    tokio::task::spawn_blocking(move || session.await_outstanding_reader_for_test(Duration::from_secs(10)))
        .await
        .expect("the subscription thread ends")
}
