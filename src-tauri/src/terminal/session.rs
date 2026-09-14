use crate::terminal::PtyError;
use parking_lot::Mutex;
use portable_pty::{Child, MasterPty, PtySize};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
#[cfg(windows)]
#[path = "windows_input.rs"]
pub(crate) mod windows_input;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PtySessionState {
    Starting,
    Running,
    Closing,
    Exited { code: Option<i32> },
    Failed { reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TerminalSignal {
    Interrupt,
    Terminate,
    Kill,
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
    child: Arc<Mutex<Option<Box<dyn Child + Send + Sync>>>>,
    reader_task: Arc<Mutex<Option<JoinHandle<()>>>>,
    output_tx: Arc<Mutex<Option<mpsc::Sender<Vec<u8>>>>>,
    worktree_path: Option<PathBuf>,
    reader_finished: Arc<AtomicBool>,
    reaped: Arc<AtomicBool>,
    state: Arc<Mutex<PtySessionState>>,
    cols: Arc<Mutex<u16>>,
    rows: Arc<Mutex<u16>>,
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
            config.input.get_ref().as_fd().try_clone_to_owned().expect("duplicate PTY poll descriptor")
        };
        let metrics_session_id = config.id.clone();
        let reader_finished = Arc::new(AtomicBool::new(false));
        let reader_finished_task = Arc::clone(&reader_finished);
        let reader_task = tokio::task::spawn_blocking(move || {
            let mut reader = reader;
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        crate::terminal::metrics::record_pty_read(&metrics_session_id, n);
                        if reader_tx.blocking_send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    #[cfg(unix)]
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        use std::os::fd::AsRawFd;
                        let mut poll = libc::pollfd { fd: reader_poll.as_raw_fd(), events: libc::POLLIN, revents: 0 };
                        if unsafe { libc::poll(&mut poll, 1, -1) } < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted { break; }
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
            child: Arc::new(Mutex::new(Some(config.child))),
            reader_task: Arc::new(Mutex::new(Some(reader_task))),
            output_tx,
            worktree_path: config.worktree_path,
            reader_finished,
            reaped: Arc::new(AtomicBool::new(false)),
            state: Arc::new(Mutex::new(PtySessionState::Starting)),
            cols: Arc::new(Mutex::new(config.cols)),
            rows: Arc::new(Mutex::new(config.rows)),
        }
    }

    pub fn id(&self) -> &str {
        &self.id
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
            .and_then(|child| child.process_id())
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
                    let mut poll = libc::pollfd { fd: self.input.get_ref().as_raw_fd(), events: libc::POLLOUT, revents: 0 };
                    if unsafe { libc::poll(&mut poll, 1, -1) } < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
                        return Err(PtyError::IoError(std::io::Error::last_os_error().to_string()));
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
        if data.len() > 65536 { return Err(PtyError::IoError("PTY_INPUT_TOO_LARGE".into())); }
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            let _gate = self.input_gate.lock().await;
            let mut remaining = data;
            while !remaining.is_empty() {
                let mut ready = self.input.writable().await.map_err(|e| PtyError::IoError(e.to_string()))?;
                // Do not wait behind a synchronous legacy writer or closed I/O.
                let writer = self.writer.try_lock().ok_or_else(|| PtyError::IoError("PTY_INPUT_BUSY".into()))?;
                if writer.is_none() { return Err(PtyError::IoError("PTY_INPUT_CLOSED".into())); }
                let state=self.state.lock();
                if !matches!(*state,PtySessionState::Starting | PtySessionState::Running) { return Err(PtyError::IoError("PTY_INPUT_CLOSED".into())); }
                let result = ready.try_io(|fd| {
                    let n = unsafe { libc::write(fd.get_ref().as_raw_fd(), remaining.as_ptr().cast(), remaining.len()) };
                    if n < 0 { Err(std::io::Error::last_os_error()) } else { Ok(n as usize) }
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
        }).await.map_err(|_| PtyError::IoError("PTY_INPUT_TIMEOUT".into()))?
    }

    #[cfg(windows)]
    pub async fn write_input_cancellable(&self, data: &[u8]) -> Result<(), PtyError> {
        if data.len() > 65536 { return Err(PtyError::IoError("PTY_INPUT_TOO_LARGE".into())); }
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            let _gate = self.input_gate.lock().await;
            if self.writer.try_lock().is_none_or(|w| w.is_none()) { return Err(PtyError::IoError("PTY_INPUT_BUSY_OR_CLOSED".into())); }
            let mut bytes=data;
            while !bytes.is_empty() {
                let n=self.write_input_slice(bytes)?;
                if n==0 {tokio::time::sleep(std::time::Duration::from_millis(1)).await;} else {bytes=&bytes[n..];}
            }
            Ok(())
        }).await.map_err(|_| PtyError::IoError("PTY_INPUT_TIMEOUT".into()))?
    }

    /// One synchronous pump iteration for the Windows ConPTY input path. Every
    /// lock is acquired and released inside this plain frame, so no guard or
    /// raw-handle-bearing input value is ever live across the caller's await
    /// point: the ConPTY input type made the surrounding async block !Send and
    /// broke the axum WebSocket upgrade future.
    #[cfg(windows)]
    fn write_input_slice(&self, bytes: &[u8]) -> Result<usize, PtyError> {
        let writer=self.writer.try_lock().ok_or_else(|| PtyError::IoError("PTY_INPUT_BUSY".into()))?;
        let state=self.state.lock();
        if writer.is_none() || !matches!(*state,PtySessionState::Starting | PtySessionState::Running) { return Err(PtyError::IoError("PTY_INPUT_CLOSED".into())); }
        self.input.try_write(bytes).map_err(|e|PtyError::IoError(e.to_string()))
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
        child
            .kill()
            .map_err(|e| PtyError::KillError(format!("Kill failed: {e}")))
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
        };

        // portable-pty creates the child as the PTY session/process-group leader on Unix.
        // Addressing the negative pid signals the whole job-control process group rather
        // than only the shell process.
        let result = unsafe { libc::kill(-(pid as i32), sig) };
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
        }
    }

    pub(crate) fn poll_exit_code(&self) -> Result<Option<i32>, PtyError> {
        let mut child_slot = self.child.lock();
        let Some(child) = child_slot.as_mut() else {
            return Ok(match self.state() {
                PtySessionState::Exited { code } => code,
                _ => None,
            });
        };

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

    pub(crate) fn wait_and_reap(&self) -> Result<Option<i32>, PtyError> {
        let mut child_slot = self.child.lock();
        let Some(mut child) = child_slot.take() else {
            return Ok(match self.state() {
                PtySessionState::Exited { code } => code,
                _ => None,
            });
        };

        let result = child
            .wait()
            .map(|status| Some(status.exit_code() as i32))
            .map_err(|e| PtyError::Other(format!("wait failed: {e}")));
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
