use crate::terminal::output_hub::{SessionHubSnapshot, TerminalOutputHub};
use crate::terminal::PtyError;
use parking_lot::{Mutex, RwLock};
use portable_pty::{Child, MasterPty, PtySize};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
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
        if res == 0 {
            return true;
        }
        let err = std::io::Error::last_os_error();
        err.raw_os_error() == Some(libc::EPERM)
    }
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
}

pub struct PtySession {
    #[cfg(windows)]
    input: windows_input::WindowsInput,
    #[cfg(unix)]
    input: tokio::io::unix::AsyncFd<std::os::fd::OwnedFd>,
    input_gate: tokio::sync::Mutex<()>,
    pub id: String,
    master: Arc<Mutex<Option<Box<dyn MasterPty + Send>>>>,
    writer: Arc<Mutex<Option<Box<dyn Write + Send>>>>,
    child: Arc<Mutex<Option<ProcessHandle>>>,
    reader_task: Arc<Mutex<Option<JoinHandle<()>>>>,
    output_tx: Arc<Mutex<Option<mpsc::Sender<Vec<u8>>>>>,
    worktree_path: Option<PathBuf>,
    reader_finished: Arc<AtomicBool>,
    reaped: Arc<AtomicBool>,
    /// Epoch millis of the last PTY output chunk read from the child (0 = none).
    last_output_at: Arc<AtomicU64>,
    state: Arc<Mutex<PtySessionState>>,
    cols: Arc<Mutex<u16>>,
    rows: Arc<Mutex<u16>>,
    output_hub: Arc<RwLock<Option<Arc<TerminalOutputHub>>>>,
    pause_requested: Arc<AtomicBool>,
    reader_paused: Arc<AtomicBool>,
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
        let reader_last_output_at = Arc::new(AtomicU64::new(0));
        let last_output_at = Arc::clone(&reader_last_output_at);
        let task_last_output_at = Arc::clone(&last_output_at);
        let pause_requested = Arc::new(AtomicBool::new(false));
        let reader_paused = Arc::new(AtomicBool::new(false));
        let pause_requested_task = Arc::clone(&pause_requested);
        let reader_paused_task = Arc::clone(&reader_paused);
        let reader_task = tokio::task::spawn_blocking(move || {
            let mut reader = reader;
            let mut buf = [0u8; 4096];
            loop {
                if pause_requested_task.load(Ordering::Acquire) {
                    reader_paused_task.store(true, Ordering::Release);
                    while pause_requested_task.load(Ordering::Acquire)
                        && !reader_finished_task.load(Ordering::Acquire)
                    {
                        std::thread::sleep(std::time::Duration::from_millis(20));
                    }
                    reader_paused_task.store(false, Ordering::Release);
                }

                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        record_output_millis(&task_last_output_at);
                        crate::terminal::metrics::record_pty_read(&metrics_session_id, n);
                        if reader_tx.blocking_send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
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
        });

        Self {
            input: config.input,
            input_gate: tokio::sync::Mutex::new(()),
            id: config.id,
            master: Arc::new(Mutex::new(Some(config.master))),
            writer: Arc::new(Mutex::new(Some(config.writer))),
            child: Arc::new(Mutex::new(Some(ProcessHandle::Spawned(config.child)))),
            reader_task: Arc::new(Mutex::new(Some(reader_task))),
            output_tx,
            worktree_path: config.worktree_path,
            reader_finished,
            last_output_at,
            reaped: Arc::new(AtomicBool::new(false)),
            state: Arc::new(Mutex::new(PtySessionState::Starting)),
            cols: Arc::new(Mutex::new(config.cols)),
            rows: Arc::new(Mutex::new(config.rows)),
            output_hub: Arc::new(RwLock::new(None)),
            pause_requested,
            reader_paused,
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

    pub fn pgid(&self) -> Option<u32> {
        self.child
            .lock()
            .as_ref()
            .and_then(|child| child.process_group())
            .or_else(|| {
                #[cfg(unix)]
                {
                    self.foreground_process_group().ok().flatten()
                }
                #[cfg(not(unix))]
                {
                    None
                }
            })
            .or_else(|| self.pid())
    }

    pub fn pause_reader(&self) {
        self.pause_requested.store(true, Ordering::Release);
    }

    pub fn resume_reader(&self) {
        self.pause_requested.store(false, Ordering::Release);
    }

    pub fn is_reader_paused(&self) -> bool {
        self.reader_paused.load(Ordering::Acquire)
    }

    pub fn stop_reader(&self) {
        if let Some(handle) = self.reader_task.lock().take() {
            handle.abort();
        }
        self.reader_finished.store(true, Ordering::Release);
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

    pub fn write_input(&self, data: &[u8]) -> Result<(), PtyError> {
        let mut writer = self.writer.lock();
        let writer = writer
            .as_mut()
            .ok_or_else(|| PtyError::IoError("PTY writer is closed".into()))?;
        let mut remaining = data;
        while !remaining.is_empty() {
            match writer.write(remaining) {
                Ok(0) => return Err(PtyError::IoError("PTY write returned zero".into())),
                Ok(n) => remaining = &remaining[n..],
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                #[cfg(unix)]
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    use std::os::fd::AsRawFd;
                    let mut poll = libc::pollfd {
                        fd: self.input.get_ref().as_raw_fd(),
                        events: libc::POLLOUT,
                        revents: 0,
                    };
                    if unsafe { libc::poll(&mut poll, 1, -1) } < 0
                        && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted
                    {
                        return Err(PtyError::IoError(
                            std::io::Error::last_os_error().to_string(),
                        ));
                    }
                }
                Err(e) => return Err(PtyError::IoError(format!("Write failed: {e}"))),
            }
        }
        writer
            .flush()
            .map_err(|e| PtyError::IoError(format!("Flush failed: {e}")))?;
        Ok(())
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
        // Addressing the negative pid signals the whole job-control process group rather
        // than only the shell process.
        let group = self.child.lock().as_ref().and_then(|h| h.process_group());
        let target = group.map(|g| -(g as i32)).unwrap_or(-(pid as i32));
        let result = unsafe { libc::kill(target, sig) };
        if result == 0 {
            return Ok(());
        }

        // Fallback to direct PID if process group signaling failed
        let result = unsafe { libc::kill(pid as i32, sig) };
        if result == 0 {
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

    pub(crate) fn close_output(&self) {
        self.output_tx.lock().take();
    }

    pub(crate) fn close_io(&self) {
        self.writer.lock().take();
        self.master.lock().take();
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
        let reader_last_output_at = Arc::new(AtomicU64::new(0));
        let last_output_at = Arc::clone(&reader_last_output_at);
        let task_last_output_at = Arc::clone(&last_output_at);
        let pause_requested = Arc::new(AtomicBool::new(false));
        let reader_paused = Arc::new(AtomicBool::new(false));
        let pause_requested_task = Arc::clone(&pause_requested);
        let reader_paused_task = Arc::clone(&reader_paused);

        let reader_task = tokio::task::spawn_blocking(move || {
            let mut reader = reader;
            let mut buf = [0u8; 4096];
            loop {
                if pause_requested_task.load(Ordering::Acquire) {
                    reader_paused_task.store(true, Ordering::Release);
                    while pause_requested_task.load(Ordering::Acquire)
                        && !reader_finished_task.load(Ordering::Acquire)
                    {
                        std::thread::sleep(std::time::Duration::from_millis(20));
                    }
                    reader_paused_task.store(false, Ordering::Release);
                }

                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        record_output_millis(&task_last_output_at);
                        crate::terminal::metrics::record_pty_read(&metrics_session_id, n);
                        if reader_tx.blocking_send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
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
            writer: Arc::new(Mutex::new(Some(writer))),
            child: Arc::new(Mutex::new(child_handle)),
            reader_task: Arc::new(Mutex::new(Some(reader_task))),
            output_tx,
            worktree_path: snapshot.worktree_path,
            reader_finished,
            last_output_at,
            reaped: Arc::new(AtomicBool::new(false)),
            state: Arc::new(Mutex::new(snapshot.state)),
            cols: Arc::new(Mutex::new(snapshot.cols)),
            rows: Arc::new(Mutex::new(snapshot.rows)),
            output_hub: Arc::new(RwLock::new(None)),
            pause_requested,
            reader_paused,
        };

        Ok((session, rx))
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        self.writer.lock().take();
        self.master.lock().take();
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
}
