use crate::terminal::output_hub::TerminalOutputHub;
#[cfg(unix)]
use crate::terminal::session::PtySessionExport;
use crate::terminal::session::PtySessionSnapshot;
use crate::terminal::{
    session::PtySessionConfig, PtyError, PtySession, PtySessionState, TerminalSignal,
};
use crate::worktree::WorktreeManager;
use parking_lot::{Mutex, RwLock};
use portable_pty::{native_pty_system, CommandBuilder, PtySize, PtySystem};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use uuid::Uuid;

pub(crate) const LIFECYCLE_POLL_INTERVAL: Duration = Duration::from_millis(250);
const READER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
/// How many bounded reader waits one close can apply: stage 1, which lets the stream end the reader,
/// the stage-2 cancellation fallback, and the bounded join that follows them.
const READER_WAIT_STAGES: u32 = 3;
/// The poll a bounded REAP takes inside a close. `poll_reap_bounded` sleeps
/// `LIFECYCLE_POLL_INTERVAL.min(this)`, so a reap can return up to this long after the deadline it
/// was given - which is why `close_fence_for` sums it once per reap it covers.
const REAP_POLL_INTERVAL: Duration = Duration::from_millis(50);
/// The poll a bounded READER wait takes. `await_reader_finished` sleeps
/// `LIFECYCLE_POLL_INTERVAL.min(this)`, and it can return up to this long after its own deadline
/// for the same reason `REAP_POLL_INTERVAL` can, so `close_fence_for` sums it once per
/// reader wait it covers.
const READER_WAIT_POLL_INTERVAL: Duration = Duration::from_millis(20);
/// The bounded wait the close applies to the master release it hands to the blocking pool. The pool
/// is a thread budget rather than a duration bound, so the release needs a bound of its own to be
/// observable at all.
const MASTER_RELEASE_TIMEOUT: Duration = READER_SHUTDOWN_TIMEOUT;
const TERM_GRACE_TIMEOUT: Duration = Duration::from_secs(1);
const KILL_REAP_TIMEOUT: Duration = Duration::from_secs(1);

/// The worst case ONE close can spend before it leaves the registry, for the TERM grace that close
/// was invoked with: the grace phase itself, the KILL+reap phase, the observed master release, the
/// bounded reader waits, and the poll quantization those waits add to their own deadlines.
///
/// The concurrent-close fence must never be shorter than the close it waits on, or a second close
/// reports a timeout for a close that does happen. The grace phase is a CALLER PARAMETER and not a
/// module property - `close_session` passes `TERM_GRACE_TIMEOUT`, the machine close path passes 5 s -
/// so the bound is a function of the grace rather than a constant derived from the default one: a
/// constant would describe one path and be short by the difference on every other.
///
/// The phase deadlines alone are still not an upper bound. Every bounded wait tests its deadline only
/// AFTER taking a poll, so it can return up to one poll interval past the deadline it was given, and
/// one close applies four such waits: the two bounded reaps (`poll_reap_bounded`, the TERM grace
/// phase and the KILL escalation) at `REAP_POLL_INTERVAL`, and the two bounded reader waits
/// (`await_reader_finished`, the stream stage and the cancellation stage) at
/// `READER_WAIT_POLL_INTERVAL`. Those overshoots are summed here, one per wait the sum above
/// covers, because a fence that stops at the deadlines expires before a close that spends its whole
/// bound can leave the registry. The master release and the reader join are `timeout`s over a
/// handle, which wake on their own deadline, so they contribute no poll of their own.
///
/// What this cannot bound is work the CALLER owns: `authorize` runs on the close path but is not
/// one of the close's phases, so a caller whose closure is slow can still outlast the fence. That is
/// a property of the caller, not something this bound can derive.
fn close_fence_for(grace: Duration) -> Duration {
    grace
        + KILL_REAP_TIMEOUT
        + MASTER_RELEASE_TIMEOUT
        + READER_SHUTDOWN_TIMEOUT * READER_WAIT_STAGES
        + REAP_POLL_INTERVAL * 2
        + READER_WAIT_POLL_INTERVAL * 2
}

/// Decides a UTF-8 `LANG` override for PTY children whose inherited environment selects
/// no character-type locale at all (e.g. a launchd-spawned GUI daemon without LANG).
/// Returns `None` when the child already receives an explicit locale via `LC_ALL`,
/// `LC_CTYPE`, or a non-empty `LANG`; explicit settings are never overridden.
pub fn utf8_locale_override<E>(get_env: E) -> Option<(&'static str, &'static str)>
where
    E: Fn(&'static str) -> Option<std::ffi::OsString>,
{
    utf8_locale_override_for(get_env, cfg!(target_os = "windows"))
}

/// Windows console encoding is governed by the console code page, not by `LANG`/`LC_*`: ConPTY
/// shells (cmd.exe, PowerShell, pwsh) ignore these variables entirely, so an override written
/// into a Windows pane would be dead logic. The platform decision is a parameter so it stays
/// testable from every host.
fn utf8_locale_override_for<E>(
    get_env: E,
    is_windows: bool,
) -> Option<(&'static str, &'static str)>
where
    E: Fn(&'static str) -> Option<std::ffi::OsString>,
{
    if is_windows {
        return None;
    }
    for key in ["LC_ALL", "LC_CTYPE", "LANG"] {
        if get_env(key).map(|v| !v.is_empty()).unwrap_or(false) {
            return None;
        }
    }
    let locale = if cfg!(target_os = "macos") {
        "en_US.UTF-8"
    } else {
        "C.UTF-8"
    };
    Some(("LANG", locale))
}

pub fn apply_session_env(
    cmd: &mut CommandBuilder,
    session_id: &str,
    worktree_path: &str,
    workspace_id: Option<&str>,
) {
    cmd.env("FERRYX_SESSION_ID", session_id);
    // Windows canonicalization returns verbatim paths (`\\?\C:\repo\...`), and a consumer of
    // this variable reads it as literal path text rather than handing it back to the OS, so the
    // prefix is stripped here. On unix the normalizer is a no-op for an ordinary path.
    cmd.env(
        "FERRYX_WORKTREE_PATH",
        crate::daemon::session_service::normalize_process_cwd(Path::new(worktree_path)),
    );
    if let Some(ws) = workspace_id {
        let trimmed = ws.trim();
        if !trimmed.is_empty() {
            cmd.env("FERRYX_WORKSPACE_ID", trimmed);
        }
    }
}

/// Exports the loopback agent-state rendezvous the daemon publishes beside its agent-state
/// socket while it runs. Windows has no unix socket, so the bundled extension's TCP mode is the
/// only path a pane's state report can take. Both variables are written together or not at all:
/// the extension enables TCP only when it has a usable port *and* a token.
#[cfg(not(unix))]
fn apply_agent_state_tcp_env(cmd: &mut CommandBuilder) {
    let Ok(record) = std::fs::read_to_string(crate::daemon::get_agent_state_rendezvous_path()) else {
        return;
    };
    let Some((port, token)) = parse_agent_state_rendezvous(&record) else {
        return;
    };
    cmd.env("FERRYX_AGENT_STATE_PORT", port.to_string());
    cmd.env("FERRYX_AGENT_STATE_TOKEN", token);
}

/// Reads the daemon-published rendezvous record: the port on the first line, the token on the
/// second. The daemon renames the whole record into place, so a torn pair cannot be observed;
/// a record missing either half is still read as "not published yet" and never reaches a pane.
#[cfg(any(not(unix), test))]
fn parse_agent_state_rendezvous(record: &str) -> Option<(u16, String)> {
    let mut lines = record.lines();
    let port = lines
        .next()?
        .trim()
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0)?;
    let token = lines.next()?.trim();
    if token.is_empty() {
        return None;
    }
    Some((port, token.to_string()))
}

#[cfg(test)]
mod agent_state_rendezvous_tests {
    use super::parse_agent_state_rendezvous;

    #[test]
    fn parse_accepts_only_a_complete_pair() {
        assert_eq!(
            parse_agent_state_rendezvous("41234\ntoken-abc\n"),
            Some((41234, "token-abc".to_string()))
        );
        // Surrounding whitespace is tolerated: the record is a plain text file.
        assert_eq!(
            parse_agent_state_rendezvous(" 41234 \n token-abc \n"),
            Some((41234, "token-abc".to_string()))
        );

        // A port without a token is half a pair, whether the token line is absent, empty or
        // whitespace: exporting the port alone would enable nothing while looking configured.
        assert_eq!(parse_agent_state_rendezvous("41234\n"), None);
        assert_eq!(parse_agent_state_rendezvous("41234"), None);
        assert_eq!(parse_agent_state_rendezvous("41234\n\n"), None);
        assert_eq!(parse_agent_state_rendezvous("41234\n   \n"), None);

        // A token without a usable port is the other half.
        assert_eq!(parse_agent_state_rendezvous("\ntoken-abc\n"), None);
        assert_eq!(parse_agent_state_rendezvous("not-a-port\ntoken-abc\n"), None);
        assert_eq!(parse_agent_state_rendezvous("0\ntoken-abc\n"), None);
        assert_eq!(parse_agent_state_rendezvous("70000\ntoken-abc\n"), None);

        // Nothing published at all.
        assert_eq!(parse_agent_state_rendezvous(""), None);
    }
}

#[derive(Clone)]
pub struct PtyManager {
    sessions: Arc<RwLock<HashMap<String, Arc<PtySession>>>>,
    pty_system: Arc<Mutex<Box<dyn PtySystem + Send>>>,
    output_hub: Arc<RwLock<Option<Arc<TerminalOutputHub>>>>,
}

impl Default for PtyManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PtyManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            pty_system: Arc::new(Mutex::new(native_pty_system())),
            output_hub: Arc::new(RwLock::new(None)),
        }
    }

    pub fn with_output_hub(self, hub: Arc<TerminalOutputHub>) -> Self {
        *self.output_hub.write() = Some(hub);
        self
    }

    pub fn set_output_hub(&self, hub: Arc<TerminalOutputHub>) {
        *self.output_hub.write() = Some(hub);
    }

    pub fn output_hub(&self) -> Option<Arc<TerminalOutputHub>> {
        self.output_hub.read().clone()
    }

    pub fn spawn(
        &self,
        cmd: CommandBuilder,
        cols: u16,
        rows: u16,
    ) -> Result<(String, mpsc::Receiver<Vec<u8>>), PtyError> {
        let session_id = Uuid::new_v4().to_string();
        let rx = self.spawn_with_id_and_worktree(session_id.clone(), cmd, cols, rows, None)?;
        Ok((session_id, rx))
    }

    pub fn spawn_shell(
        &self,
        cols: u16,
        rows: u16,
    ) -> Result<(String, mpsc::Receiver<Vec<u8>>), PtyError> {
        let cmd = crate::terminal::shell::resolve_shell_command(None);
        self.spawn(cmd, cols, rows)
    }

    /// Spawn an interactive terminal owned by `worktree_path`.
    ///
    /// Interactive PTYs are deliberately multi-session.  The worktree writer lease is an
    /// exclusive mutation/agent guard and must not be used to serialize terminal panes:
    /// Orca-style split panes require two or more live PTYs in the same worktree.
    pub fn spawn_in_worktree(
        &self,
        cmd: CommandBuilder,
        cols: u16,
        rows: u16,
        worktree_manager: &WorktreeManager,
        worktree_path: &Path,
    ) -> Result<(String, mpsc::Receiver<Vec<u8>>), PtyError> {
        let session_id = Uuid::new_v4().to_string();
        self.spawn_in_worktree_with_id(session_id, cmd, cols, rows, worktree_manager, worktree_path)
    }

    pub(crate) fn spawn_in_worktree_with_id(
        &self,
        session_id: String,
        mut cmd: CommandBuilder,
        cols: u16,
        rows: u16,
        worktree_manager: &WorktreeManager,
        worktree_path: &Path,
    ) -> Result<(String, mpsc::Receiver<Vec<u8>>), PtyError> {
        let canonical_worktree = worktree_manager
            .canonical_allowed_path(worktree_path)
            .map_err(|error| PtyError::Other(error.to_string()))?;
        let workspace_id = worktree_manager
            .find_worktree(&canonical_worktree)
            .ok()
            .flatten()
            .and_then(|w| w.orca_info())
            .map(|info| info.ws_id);
        apply_session_env(
            &mut cmd,
            &session_id,
            &canonical_worktree.to_string_lossy(),
            workspace_id.as_deref(),
        );
        let rx = self.spawn_with_id_and_worktree(
            session_id.clone(),
            cmd,
            cols,
            rows,
            Some(canonical_worktree),
        )?;
        Ok((session_id, rx))
    }

    pub fn spawn_with_id(
        &self,
        session_id: impl Into<String>,
        cmd: CommandBuilder,
        cols: u16,
        rows: u16,
    ) -> Result<mpsc::Receiver<Vec<u8>>, PtyError> {
        self.spawn_with_id_and_worktree(session_id.into(), cmd, cols, rows, None)
    }

    fn spawn_with_id_and_worktree(
        &self,
        session_id: String,
        mut cmd: CommandBuilder,
        cols: u16,
        rows: u16,
        worktree_path: Option<std::path::PathBuf>,
    ) -> Result<mpsc::Receiver<Vec<u8>>, PtyError> {
        if self.has_session(&session_id) {
            return Err(PtyError::Other(format!(
                "PTY session '{session_id}' already exists"
            )));
        }

        // The agent extension reports state for the pane it runs in, so it needs the session
        // identity here: this is the first point where the id exists and the child is not yet
        // spawned.
        if let Some(ref path) = worktree_path {
            apply_session_env(&mut cmd, &session_id, &path.to_string_lossy(), None);
        } else {
            cmd.env("FERRYX_SESSION_ID", &session_id);
        }
        // Unix panes report agent state over the daemon's unix socket; every other platform has
        // no such socket and reaches the daemon's loopback TCP ingress instead.
        #[cfg(unix)]
        cmd.env(
            "FERRYX_AGENT_STATE_SOCKET",
            crate::daemon::agent_state_socket_path(),
        );
        #[cfg(not(unix))]
        apply_agent_state_tcp_env(&mut cmd);

        // A GUI-launched daemon inherits TERM=dumb, which agent TUIs read as a non-interactive
        // terminal: they drop to plain mode and stop reporting activity. A PTY is a real
        // terminal, so it must advertise one.
        if std::env::var("TERM").map(|t| t == "dumb").unwrap_or(true) {
            cmd.env("TERM", "xterm-256color");
        }

        // A Dock/launchd-launched daemon inherits no LANG/LC_* at all: PTY children then run
        // in the "C" locale and compute CJK widths per byte, which desyncs line editors
        // (zsh/bash) from the terminal grid while typing CJK text. Any UTF-8 locale fixes it.
        if let Some((key, value)) = utf8_locale_override(std::env::var_os) {
            cmd.env(key, value);
        }

        // GUI-launched daemons inherit minimal PATH (/usr/bin:/bin:/usr/sbin:/sbin) on macOS.
        // Direct child spawns (such as agent resumes) fail to find binaries in Homebrew, bun,
        // cargo, nvm unless PATH is augmented with the user's login shell search paths.
        if let Ok(augmented) = std::env::join_paths(crate::ipc::agents::search_paths()) {
            cmd.env("PATH", augmented);
        }

        // TERM=xterm-256color only claims 256 indexed colors. Truecolor-capable agent TUIs read
        // COLORTERM instead, and degrade to a reduced palette when it is missing.
        cmd.env("COLORTERM", "truecolor");

        // Pi/Senpi otherwise disables images for an unknown TERM_PROGRAM.
        #[cfg(feature = "native-terminal")]
        if cmd.get_env("PI_IMAGE_PROTOCOL").is_none() {
            cmd.env("PI_IMAGE_PROTOCOL", "kitty");
        }

        // A Pi client only uses Kitty unicode placeholders for a VT engine it
        // trusts, and that is the form Ferryx renders best: an image becomes
        // placeholder cells, so it scrolls with its text and survives in the
        // scrollback. Without it every inline image degrades to a cursor
        // anchored overlay pinned to one content row. Ferryx's engine is
        // libghostty-vt and its renderer resolves placeholder placements.
        #[cfg(feature = "native-terminal")]
        if cmd.get_env("TERM_PROGRAM").is_none() {
            cmd.env("TERM_PROGRAM", "ghostty");
        }

        let pty_size = PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };

        let pair = {
            let pty_system = self.pty_system.lock();
            pty_system
                .openpty(pty_size)
                .map_err(|e| PtyError::PtyCreationError(e.to_string()))?
        };

        // Input registration and session tasks require an entered runtime. Reject
        // synchronous callers before creating a child, rather than panicking in
        // AsyncFd and bypassing the fallible spawn API.
        #[cfg(unix)]
        let _runtime = tokio::runtime::Handle::try_current()
            .map_err(|error| PtyError::SpawnError(format!("Failed to spawn command: {error}")))?;

        #[cfg(unix)]
        let input = {
            use std::os::fd::{AsRawFd, FromRawFd};
            let raw = pair
                .master
                .as_raw_fd()
                .ok_or_else(|| PtyError::IoError("PTY descriptor unavailable".into()))?;
            let duplicate = unsafe { libc::fcntl(raw, libc::F_DUPFD_CLOEXEC, 0) };
            if duplicate < 0 {
                return Err(PtyError::IoError(
                    std::io::Error::last_os_error().to_string(),
                ));
            }
            let fd = unsafe { std::os::fd::OwnedFd::from_raw_fd(duplicate) };
            let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
            if flags < 0
                || unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) }
                    < 0
            {
                return Err(PtyError::IoError(
                    std::io::Error::last_os_error().to_string(),
                ));
            }
            tokio::io::unix::AsyncFd::new(fd).map_err(|e| PtyError::IoError(e.to_string()))?
        };

        #[cfg(windows)]
        let input = super::session::windows_input::WindowsInput(
            pair.master
                .try_clone_input_handle()
                .map_err(|e| PtyError::IoError(e.to_string()))?,
        );
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| PtyError::IoError(format!("Failed to clone reader: {e}")))?;

        let writer = pair
            .master
            .take_writer()
            .map_err(|e| PtyError::IoError(format!("Failed to take writer: {e}")))?;

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| PtyError::SpawnError(format!("Failed to spawn command: {e}")))?;

        drop(pair.slave);

        let (tx, rx) = mpsc::channel::<Vec<u8>>(1024);
        // Taken while the master is still in hand: the session keeps it for the whole of its life,
        // so a close can end the reader even after a failed export took the master away.
        let reader_interrupt = pair.master.interrupt_handle();
        let session = Arc::new(PtySession::new(PtySessionConfig {
            input,
            id: session_id.clone(),
            master: pair.master,
            reader_interrupt,
            child,
            writer,
            reader,
            cols,
            rows,
            tx,
            worktree_path,
            // Empty: the grace a concurrent close has to cover is recorded by the close that claims
            // the session, not by the session itself.
            close_grace: Arc::new(Mutex::new(None)),
        }));

        if let Some(hub) = self.output_hub.read().clone() {
            session.set_output_hub(hub);
        }

        self.sessions
            .write()
            .insert(session_id.clone(), Arc::clone(&session));
        session.mark_running();
        self.start_lifecycle_watcher(session_id);
        Ok(rx)
    }

    fn start_lifecycle_watcher(&self, session_id: String) {
        let manager = self.clone();
        tokio::spawn(async move {
            let Some(session) = manager.get_session(&session_id) else {
                return;
            };
            let mut reader_task = session.take_reader_task();

            loop {
                let Some(session) = manager.get_session(&session_id) else {
                    break;
                };

                match session.state() {
                    PtySessionState::Exited { .. } | PtySessionState::Failed { .. } => {
                        session.close_output();
                        manager.remove_from_registry(&session_id);
                        break;
                    }
                    PtySessionState::Closing => {
                        tokio::time::sleep(LIFECYCLE_POLL_INTERVAL).await;
                        continue;
                    }
                    PtySessionState::Starting | PtySessionState::Running => {}
                }

                match session.poll_exit_code() {
                    Ok(Some(code)) => {
                        manager.finalize_natural_exit(&session_id, code).await;
                        break;
                    }
                    Ok(None)
                        if session.output_receiver_closed() || session.is_reader_finished() =>
                    {
                        if let Err(error) = manager.close_session(&session_id).await {
                            tracing::debug!(
                                "PTY receiver-drop cleanup failed for {}: {}",
                                session_id,
                                error
                            );
                        }
                        break;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        session.mark_failed(error.to_string());
                        // Tearing the session down: give the master up so the console host can
                        // flush, then end the reader with the cancellation as the bounded fallback.
                        session.close_io();
                        let _ = Self::end_reader_after_close(&session).await;
                        session.close_output();
                        manager.remove_from_registry(&session_id);
                        break;
                    }
                }

                if let Some(ref mut handle) = reader_task {
                    tokio::select! {
                        _ = handle => {
                            reader_task = None;
                        }
                        _ = tokio::time::sleep(LIFECYCLE_POLL_INTERVAL) => {}
                    }
                } else {
                    tokio::time::sleep(LIFECYCLE_POLL_INTERVAL).await;
                }
            }
        });
    }

    async fn finalize_natural_exit(&self, session_id: &str, code: i32) {
        let Some(session) = self.get_session(session_id) else {
            return;
        };
        if !session.begin_closing() {
            return;
        }

        // The child has exited, so its stream is settling. Giving up the master lets the console
        // host flush whatever it still holds and exit, and the reader delivers that tail and ends on
        // the stream; the cancellation inside is the bounded fallback for a host that never exits,
        // never the first move.
        session.close_io();
        let _ = Self::end_reader_after_close(&session).await;
        session.mark_exited(Some(code));
        session.close_output();
        self.remove_from_registry(session_id);
    }

    async fn join_reader_bounded(session: &Arc<PtySession>) -> Result<(), PtyError> {
        let Some(mut reader_task) = session.take_reader_task() else {
            return Ok(());
        };

        match tokio::time::timeout(READER_SHUTDOWN_TIMEOUT, &mut reader_task).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(PtyError::Other(format!("PTY reader task failed: {error}"))),
            Err(_) => {
                reader_task.abort();
                Err(PtyError::Other(
                    "Timed out waiting for PTY reader shutdown".into(),
                ))
            }
        }
    }

    /// End the reader of a session whose I/O has already been closed, keeping the output the console
    /// host may still be holding.
    ///
    /// The order is the contract. `close_io` gives up the master first, and dropping the master is
    /// what runs `ClosePseudoConsole` - the operation that lets the host flush its tail and exit. The
    /// reader stays armed through that, delivers the tail, and ends on the stream, all within the
    /// bound below. The cancellation is only the fallback for the case this seam exists for, a host
    /// that never exits and a read the kernel owns forever; applying it first would disarm the reader
    /// before the one operation that can still produce output, which is how a tail gets truncated.
    /// Progress is read from the READER'S OWN FLAG, never from the join's result. A spawned session
    /// hands its reader handle to the lifecycle watcher at startup, so `join_reader_bounded` has
    /// nothing to take and returns `Ok` at once - reading that as "the reader ended" let a close
    /// report success while the reader was still parked, and a parked reader holds the last output
    /// sender, so the exit record could never be written.
    async fn end_reader_after_close(session: &Arc<PtySession>) -> Result<(), PtyError> {
        // The release `close_io` handed to the blocking pool is observed first, and bounded: it is
        // what runs `ClosePseudoConsole`, the operation that lets the console host flush its tail and
        // exit, so a release the pool never ran is both a failure of its own and the reason a reader
        // would have nothing left to end on. Observing it here is what makes that reported instead of
        // read as a completed teardown.
        let release_error = Self::await_master_release(session).await.err();

        // Stage 1: the stream ends the reader. The master is already given up, so the host can flush
        // its tail and exit, and the reader delivers that tail and finishes.
        let reader_error = if Self::await_reader_finished(session, READER_SHUTDOWN_TIMEOUT).await {
            let _ = Self::join_reader_bounded(session).await;
            None
        } else {
            // Stage 2: the host never exited and the read is one the kernel still owns. Cancel,
            // bounded - never the first move, because it would disarm the reader before the one
            // operation that can still produce output.
            session.request_reader_stop();
            let ended = Self::await_reader_finished(session, READER_SHUTDOWN_TIMEOUT).await;
            let _ = Self::join_reader_bounded(session).await;
            if ended {
                None
            } else {
                Some(PtyError::Other(
                    "Timed out waiting for PTY reader shutdown".into(),
                ))
            }
        };

        match (release_error, reader_error) {
            // The release is the earlier and the more fundamental of the two failures, and a pool
            // that never ran it is also what leaves a reader with nothing to end it, so it is the
            // one reported when both fail.
            (Some(release), _) => Err(release),
            (None, Some(reader)) => Err(reader),
            (None, None) => Ok(()),
        }
    }

    /// Wait, bounded, for the master release this close handed to the blocking pool, and report a
    /// release that never ran.
    ///
    /// `close_io` gives the master up on the runtime's blocking seam. That seam is a thread budget
    /// and not a duration bound: a pool whose blocking threads are all busy leaves the release queued
    /// with the master alive, and a pool that is shutting down never runs it at all. Neither outcome
    /// can be told from a completed release through a discarded handle, which is why the close
    /// observes the handle it kept and reports the failure instead.
    async fn await_master_release(session: &Arc<PtySession>) -> Result<(), PtyError> {
        match session.observe_master_release(MASTER_RELEASE_TIMEOUT).await {
            // Nothing was handed to the pool: no master was left to drop, or there was no runtime to
            // enter and the drop ran inline on the caller. Both are complete.
            None => Ok(()),
            Some(true) => Ok(()),
            Some(false) => Err(PtyError::Other(
                "The PTY master release did not complete: the blocking pool never ran it".into(),
            )),
        }
    }

    /// Observe the reader's finished flag, bounded. The reader sets it as its last act, so this
    /// reports the same event `join_reader_bounded` waits for, without needing the handle.
    async fn await_reader_finished(session: &Arc<PtySession>, timeout: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + timeout;
        while !session.is_reader_finished() {
            if tokio::time::Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(LIFECYCLE_POLL_INTERVAL.min(READER_WAIT_POLL_INTERVAL)).await;
        }
        true
    }

    async fn poll_reap_bounded(
        session: &Arc<PtySession>,
        timeout: Duration,
    ) -> Result<Option<i32>, PtyError> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            match session.poll_exit_code()? {
                Some(code) => return Ok(Some(code)),
                None if session.is_reaped() => return Ok(None),
                None => {}
            }
            if tokio::time::Instant::now() >= deadline {
                return Ok(None);
            }
            tokio::time::sleep(LIFECYCLE_POLL_INTERVAL.min(REAP_POLL_INTERVAL)).await;
        }
    }

    pub async fn close_session(&self, session_id: &str) -> Result<(), PtyError> {
        self.close_authorized(session_id, TERM_GRACE_TIMEOUT, Arc::new(|| Ok(())))
            .await
    }

    pub(crate) async fn close_authorized(
        &self,
        session_id: &str,
        grace: Duration,
        authorize: Arc<dyn Fn() -> Result<(), String> + Send + Sync>,
    ) -> Result<(), PtyError> {
        authorize().map_err(PtyError::Other)?;
        let Some(session) = self.get_session(session_id) else {
            return Ok(());
        };

        // Recorded BEFORE the session is claimed, so whichever close wins the race has already
        // published the grace it is about to run: the loser's fence reads the winner's grace and not
        // its own, and a loser's shorter grace can never shrink the bound the winner needs.
        session.note_close_grace(grace);

        if !session.begin_closing() {
            if matches!(
                session.state(),
                PtySessionState::Exited { .. } | PtySessionState::Failed { .. }
            ) {
                session.close_output();
                self.remove_from_registry(session_id);
                return Ok(());
            }

            // The close this waits on is the one that CLAIMS the session, and the claim can land after
            // this fence is armed - so the bound is re-derived on every poll from the grace the claim
            // recorded, always measured from the same start, which makes it grow or stay and never
            // shrink. A fence sized from the waiting call's own grace, or from the module default, is
            // short whenever the close in flight runs a longer one: a 1 s `close_session` waiting on a
            // 5 s machine close would report a timeout for a close that does happen.
            let started = tokio::time::Instant::now();
            let mut deadline = started + close_fence_for(grace);
            while tokio::time::Instant::now() < deadline {
                if !self.has_session(session_id) {
                    return Ok(());
                }
                let in_flight = session.close_grace().unwrap_or(grace);
                let bound = started + close_fence_for(in_flight);
                if bound > deadline {
                    deadline = bound;
                }
                tokio::time::sleep(LIFECYCLE_POLL_INTERVAL).await;
            }
            // The loop's last look at the registry is taken BEFORE its final poll, so it can precede
            // the deadline by up to one `LIFECYCLE_POLL_INTERVAL` - and a session that leaves the
            // registry inside that window would be reported as a timeout for a close that happened.
            // The registry is therefore read once more here, at the moment the deadline has actually
            // passed and the report is about to be made.
            if !self.has_session(session_id) {
                return Ok(());
            }
            return Err(PtyError::Other(format!(
                "Timed out waiting for concurrent close of session '{session_id}'"
            )));
        }

        let mut first_error: Option<PtyError> = None;
        let mut exit_code = match session.poll_exit_code() {
            Ok(code) => code,
            Err(error) => {
                first_error = Some(error);
                None
            }
        };

        if exit_code.is_none() && !session.is_reaped() {
            if let Err(signal_error) = session.signal(TerminalSignal::Terminate) {
                // TERM is a best-effort graceful phase. A process-group signal can become
                // unavailable after job-control transitions (for example, immediately after
                // VINTR/SIGINT), but Close still has a mandatory KILL+reap fallback below.
                // Do not report the graceful-phase error if escalation succeeds.
                tracing::debug!(
                    "PTY TERM signal failed for {}; escalating: {}",
                    session_id,
                    signal_error
                );
            } else {
                match Self::poll_reap_bounded(&session, grace).await {
                    Ok(Some(code)) => exit_code = Some(code),
                    Ok(None) => {}
                    Err(error) => {
                        if first_error.is_none() {
                            first_error = Some(error);
                        }
                    }
                }
            }
        }

        if exit_code.is_none() && !session.is_reaped() {
            if let Err(error) = authorize() {
                session.mark_running();
                return Err(PtyError::Other(error));
            }
            if let Err(signal_error) = session
                .signal(TerminalSignal::Kill)
                .or_else(|_| session.kill())
            {
                if first_error.is_none() {
                    first_error = Some(signal_error);
                }
            } else {
                match Self::poll_reap_bounded(&session, KILL_REAP_TIMEOUT).await {
                    Ok(Some(code)) => exit_code = Some(code),
                    Ok(None) if session.is_reaped() => {}
                    Ok(None) => {
                        if first_error.is_none() {
                            first_error = Some(PtyError::Other(format!(
                                "Timed out reaping killed PTY session '{session_id}'"
                            )));
                        }
                    }
                    Err(error) => {
                        if first_error.is_none() {
                            first_error = Some(error);
                        }
                    }
                }
            }
        }

        // The child has been reaped above (or was already gone). Giving up the master now lets the
        // console host flush what it still holds, and the reader delivers that tail and ends on the
        // stream; the cancellation inside is the bounded fallback, not the first move. Requesting it
        // here instead would disarm the reader before the only operation that can produce more
        // output.
        session.close_io();

        if let Err(reader_error) = Self::end_reader_after_close(&session).await {
            if first_error.is_none() {
                first_error = Some(reader_error);
            }
        }

        if let Some(error) = first_error {
            session.mark_failed(error.to_string());
            session.close_output();
            self.remove_from_registry(session_id);
            return Err(error);
        }

        session.mark_exited(exit_code);
        session.close_output();
        self.remove_from_registry(session_id);
        Ok(())
    }

    pub fn write_input(&self, session_id: &str, data: &[u8]) -> Result<(), PtyError> {
        let session = self
            .get_session(session_id)
            .ok_or_else(|| PtyError::SessionNotFound(session_id.to_string()))?;
        session.write_input(data)
    }

    pub fn resize(&self, session_id: &str, cols: u16, rows: u16) -> Result<(), PtyError> {
        let session = self
            .get_session(session_id)
            .ok_or_else(|| PtyError::SessionNotFound(session_id.to_string()))?;
        session.resize(cols, rows)
    }

    pub fn kill(&self, session_id: &str) -> Result<(), PtyError> {
        let session = self
            .get_session(session_id)
            .ok_or_else(|| PtyError::SessionNotFound(session_id.to_string()))?;
        session.kill()
    }

    pub fn signal(&self, session_id: &str, signal: TerminalSignal) -> Result<(), PtyError> {
        let session = self
            .get_session(session_id)
            .ok_or_else(|| PtyError::SessionNotFound(session_id.to_string()))?;
        session.signal(signal)
    }

    pub fn get_session(&self, session_id: &str) -> Option<Arc<PtySession>> {
        self.sessions.read().get(session_id).cloned()
    }

    pub fn has_session(&self, session_id: &str) -> bool {
        self.sessions.read().contains_key(session_id)
    }

    fn remove_from_registry(&self, session_id: &str) -> Option<Arc<PtySession>> {
        self.sessions.write().remove(session_id)
    }

    pub fn list_sessions(&self) -> Vec<String> {
        self.sessions.read().keys().cloned().collect()
    }

    pub fn session_count(&self) -> usize {
        self.sessions.read().len()
    }

    pub fn is_alive(&self, session_id: &str) -> Result<bool, PtyError> {
        let session = self
            .get_session(session_id)
            .ok_or_else(|| PtyError::SessionNotFound(session_id.to_string()))?;
        Ok(session.is_alive())
    }

    /// Exports the session identified by `session_id` for handover transfer (design doc sections 7.2, 9.2).
    #[cfg(unix)]
    pub fn export_session(&self, session_id: &str) -> Result<PtySessionExport, PtyError> {
        let hub = self.output_hub.read().clone();
        self.export_session_with_hub(session_id, hub.as_deref())
    }

    /// Exports the session identified by `session_id` with an explicit `TerminalOutputHub` reference.
    #[cfg(unix)]
    pub fn export_session_with_hub(
        &self,
        session_id: &str,
        hub: Option<&TerminalOutputHub>,
    ) -> Result<PtySessionExport, PtyError> {
        let session = self
            .get_session(session_id)
            .ok_or_else(|| PtyError::SessionNotFound(session_id.to_string()))?;
        // Pausing stops the predecessor from consuming output (the reader loop honours
        // `pause_requested` at the top of each iteration) WITHOUT setting `reader_finished`,
        // so the lifecycle watcher does not close a session the successor now owns.
        // Previously, `stop_reader` was called here, which only set `reader_finished = true`
        // without stopping the spawn_blocking reader task; the lifecycle watcher observed
        // `is_reader_finished()` and called `close_session`, sending SIGTERM/SIGKILL to the
        // transferred child process.
        // Only pause once the export has actually succeeded, so a failure leaves the session
        // untouched and the predecessor keeps serving it.
        let export = session.export_for_transfer_with_hub(hub)?;
        session.pause_reader();
        Ok(export)
    }

    /// Resumes readers for all registered sessions that were paused.
    ///
    /// Returns the number of sessions that had a pending pause and were resumed.
    /// This is how a predecessor takes back sessions it exported to a successor that then failed.
    pub fn resume_paused_readers(&self) -> usize {
        let sessions: Vec<Arc<PtySession>> = self.sessions.read().values().cloned().collect();
        let mut resumed = 0;
        for session in sessions {
            if session.resume_reader() {
                resumed += 1;
            }
        }
        resumed
    }

    /// Non-unix stub for `export_session`.
    #[cfg(not(unix))]
    pub fn export_session(&self, _session_id: &str) -> Result<(), PtyError> {
        Err(PtyError::Other("PTY export is only supported on Unix".into()))
    }

    /// Adopts an exported PTY session into this manager without spawning a command (design doc section 11 'PtyManager adoption').
    ///
    /// Rejects duplicate session IDs, restores output-hub state when an output hub is available,
    /// constructs the adopted session via `PtySession::adopt_from_transfer`, inserts it into the
    /// registry, marks it running, and starts lifecycle observation.
    #[cfg(unix)]
    pub fn adopt_transferred_session(
        &self,
        master: std::os::fd::OwnedFd,
        snapshot: PtySessionSnapshot,
    ) -> Result<mpsc::Receiver<Vec<u8>>, PtyError> {
        let hub = self.output_hub.read().clone();
        self.adopt_transferred_session_with_hub(
            master,
            snapshot,
            hub.as_deref(),
        )
    }

    /// Adopts a transferred session with an explicit `TerminalOutputHub` reference.
    #[cfg(unix)]
    pub fn adopt_transferred_session_with_hub(
        &self,
        master: std::os::fd::OwnedFd,
        snapshot: PtySessionSnapshot,
        output_hub: Option<&TerminalOutputHub>,
    ) -> Result<mpsc::Receiver<Vec<u8>>, PtyError> {
        let session_id = snapshot.session_id.clone();
        if self.has_session(&session_id) {
            return Err(PtyError::Other(format!(
                "PTY session '{session_id}' already exists"
            )));
        }

        if let Some(hub) = output_hub {
            if let Some(hub_snap) = snapshot.hub_snapshot.as_ref() {
                hub.import_session_state(&session_id, hub_snap.clone())
                    .map_err(|e| PtyError::Other(format!("Failed to import hub state: {e}")))?;
            } else {
                let _ = hub.register_session(&session_id);
                hub.record_initial_size(&session_id, snapshot.cols, snapshot.rows);
            }
        }

        let initial_state = snapshot.state.clone();
        let (session, rx) = PtySession::adopt_from_transfer(master, snapshot)?;
        let session = Arc::new(session);

        if let Some(hub) = output_hub
            .map(|h| Arc::new(h.clone()))
            .or_else(|| self.output_hub.read().clone())
        {
            session.set_output_hub(hub);
        }

        self.sessions
            .write()
            .insert(session_id.clone(), Arc::clone(&session));

        match initial_state {
            PtySessionState::Starting | PtySessionState::Running => {
                session.mark_running();
            }
            _ => {}
        }

        self.start_lifecycle_watcher(session_id);
        Ok(rx)
    }

    /// Adopts from a complete `PtySessionExport` struct by splitting it into descriptor and snapshot.
    #[cfg(unix)]
    pub fn adopt_transferred_export(
        &self,
        export: PtySessionExport,
    ) -> Result<mpsc::Receiver<Vec<u8>>, PtyError> {
        let (master, snapshot) = export.into_parts();
        self.adopt_transferred_session(master, snapshot)
    }
}

#[cfg(all(test, unix))]
mod tests {
    /// Wall-clock bounds in this module are anti-hang devices, never a grade of how fast a loaded host
    /// is: each one waits on a state that either arrives or never does, so load can stretch the wait
    /// while a regression never arrives at all. Generous enough that a host at load average 100 still
    /// passes, and still finite, so a regression reports instead of hanging the suite.
    const LOAD_TOLERANT_BOUND: Duration = Duration::from_secs(60);

    /// The report a close makes when the KILL reap it runs after a TERM phase the child survives misses
    /// its own 1 s budget. That budget is a production wall-clock bound and no test may widen it, and a
    /// loaded host can miss it without any behaviour changing: the child was still killed and reaped.
    /// Tests that pin a phase of the close accept this report too, and assert the outcome that close
    /// exists to produce - the child gone, the reader ended, the registry clean - themselves, which is
    /// what keeps the tolerance from hiding a regression.
    const REAP_DEADLINE_REPORT: &str = "Timed out reaping killed PTY session";

    /// A close a test expects to complete: `Ok`, or the KILL reap deadline a loaded host can miss. Any
    /// other failure still fails the calling test.
    fn assert_close_completed(result: Result<(), PtyError>, context: &str) {
        if let Err(error) = result {
            let message = error.to_string();
            assert!(message.contains(REAP_DEADLINE_REPORT), "{context}: {message}");
        }
    }

    /// A close a test expects to report a bounded failure naming `phase`: that report, or the KILL reap
    /// deadline that can legitimately preempt it on a loaded host. Reporting SUCCESS is the regression
    /// this guards, and it still fails here.
    fn assert_close_reported_phase(result: Result<(), PtyError>, phase: &str, context: &str) {
        let error = result.expect_err(context);
        let message = error.to_string();
        assert!(
            message.contains(phase) || message.contains(REAP_DEADLINE_REPORT),
            "{context}: expected a report naming '{phase}', got: {message}"
        );
    }

    /// Await a session's own end, bounded: the KILL escalation a close runs exists to produce it, and a
    /// close that reports the reap deadline still produces it a moment later, so the outcome is awaited
    /// rather than read at the instant the close returned. `is_alive` polls the child, which is what
    /// lets a reap nobody else is watching be observed at all.
    fn wait_until_session_ended(session: &PtySession) -> bool {
        let deadline = std::time::Instant::now() + LOAD_TOLERANT_BOUND;
        while session.is_alive() {
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        true
    }

    /// Whether the process this test spawned is still RUNNING.
    ///
    /// `kill(pid, 0)` answers "does this pid exist", which is a different question: a child the close
    /// killed but whose exit the host has not finished completing - and one nothing has reaped - keeps
    /// answering it for as long as the machine takes, so a wait built on it measures the host rather
    /// than the close. The kernel's own process state answers the question the tests are asking; `ps`
    /// is the portable door to it from here (production reads the same field through `kinfo_proc` and
    /// `procfs`, which are private to the session module).
    fn child_is_running(pid: u32) -> bool {
        if unsafe { libc::kill(pid as libc::pid_t, 0) } != 0 {
            return false;
        }
        let state = std::process::Command::new("/bin/ps")
            .args(["-p", &pid.to_string(), "-o", "state="])
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
            .unwrap_or_default();
        // `Z` is a zombie and `E` is a process trying to exit; neither is running. An empty answer
        // means `ps` no longer sees the pid at all.
        !(state.is_empty() || state.starts_with('Z') || state.starts_with('E'))
    }

    /// Wait, bounded, for a child this test spawned to stop RUNNING, reaping it when the kernel has it
    /// ready for reaping - the test is its parent, so nothing else will.
    ///
    /// This is what a test asserts when it says the close left no shell behind. A shell that really
    /// survived the close is running, not a zombie or an exiting process, so that regression still
    /// fails here; what no longer fails is a host that is slow to finish a kill it did deliver.
    fn wait_until_child_stopped(pid: u32) -> bool {
        let deadline = std::time::Instant::now() + LOAD_TOLERANT_BOUND;
        loop {
            let mut status: libc::c_int = 0;
            let reaped = unsafe {
                libc::waitpid(pid as libc::pid_t, &mut status, libc::WNOHANG)
            };
            if reaped == pid as libc::pid_t {
                return true;
            }
            if !child_is_running(pid) {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// A handover on 2026-09-26 moved 18 of 20 sessions; the two casualties left no trace.
    /// Part of why a failed export is unrecoverable was this ordering: the reader was stopped
    /// BEFORE the export was attempted, so a session whose export failed stayed registered with a
    /// dead reader - alive to `list_sessions`, silent to the user, and unrecoverable by the
    /// predecessor that was still supposed to be serving it.
    #[tokio::test]
    async fn a_failed_export_leaves_the_session_readable_by_the_predecessor() {
        let manager = PtyManager::new();
        let cmd = CommandBuilder::new("/bin/sh");
        let (session_id, _rx) = manager.spawn(cmd, 80, 24).expect("spawn PTY session");

        let session = manager.get_session(&session_id).expect("session registered");
        assert!(
            !session.is_reader_finished(),
            "a freshly spawned session must have a live reader"
        );

        // Force the export to fail the way it fails in the field: the master descriptor is gone,
        // so there is nothing to hand the successor.
        session.close_io();
        let failed = manager.export_session(&session_id);
        assert!(failed.is_err(), "exporting a session with no master must fail");
        assert!(
            !session.is_reader_finished(),
            "a FAILED export must leave the reader running: this session is staying with the \
             predecessor, and stopping its reader silently strands it"
        );

        let _ = manager.close_session(&session_id);
    }

    /// The other half of the same lifecycle: a session whose export FAILED is still this daemon's
    /// to close, and that close must end its reader instead of leaving one parked behind.
    ///
    /// On Windows this is the case the retained interrupt handle exists for: the master is already
    /// gone, so nothing left on the session can reach the reader without it.
    #[tokio::test]
    async fn a_session_whose_export_failed_still_closes_and_ends_its_reader() {
        let manager = PtyManager::new();
        let (session_id, mut rx) = manager
            .spawn(CommandBuilder::new("/bin/sh"), 80, 24)
            .expect("spawn PTY session");
        let session = manager.get_session(&session_id).expect("session registered");

        // Force the export to fail the way it fails in the field: the master descriptor is gone.
        session.close_io();
        let failed = manager.export_session(&session_id);
        assert!(failed.is_err(), "exporting a session with no master must fail");
        assert!(
            !session.is_reader_finished(),
            "a failed export must leave the reader running"
        );

        let close = tokio::time::timeout(LOAD_TOLERANT_BOUND, manager.close_session(&session_id))
            .await
            .expect("close must be bounded");
        assert_close_completed(close, "a session whose export failed must still close");
        assert!(!manager.has_session(&session_id), "a closed session leaves the registry");

        // The reader holds the last output sender, so this channel closing is the exact moment the
        // reader thread exited - an event to await, not a delay to wait out.
        let ended = tokio::time::timeout(LOAD_TOLERANT_BOUND, async {
            while rx.recv().await.is_some() {}
        })
        .await;
        assert!(ended.is_ok(), "close left the reader running");
        assert!(session.is_reader_finished(), "close must end the session's reader");
    }

    /// Regression: a close must end the reader even when it does not own the reader's join handle.
    ///
    /// The lifecycle watcher takes that handle at startup for every spawned session, so the close
    /// path's join has nothing to take and returns immediately. Reading that as "the reader ended"
    /// let a close report success while the reader was still parked - and a parked reader holds the
    /// last output sender, so the exit record was never written and the machine lifecycle timed out.
    #[tokio::test]
    async fn a_close_ends_the_reader_even_when_the_join_handle_was_taken() {
        let manager = PtyManager::new();
        let (session_id, mut rx) = manager
            .spawn(CommandBuilder::new("/bin/sh"), 80, 24)
            .expect("spawn PTY session");
        let session = manager.get_session(&session_id).expect("session registered");

        // Stand in for the lifecycle watcher: take the handle before the close, exactly as
        // `start_lifecycle_watcher` does, and keep it out of the close path's reach. Dropping a
        // JoinHandle detaches, so the reader keeps running - which is the point.
        let taken = session.take_reader_task();
        assert!(taken.is_some(), "a spawned session's reader handle is takeable");

        let close = tokio::time::timeout(LOAD_TOLERANT_BOUND, manager.close_session(&session_id))
            .await
            .expect("close must be bounded");
        assert_close_completed(close, "close must not report a reader that would not stop");
        assert!(!manager.has_session(&session_id), "a closed session leaves the registry");
        assert!(
            session.is_reader_finished(),
            "close must end the reader even when the join handle was taken from it"
        );
        let ended = tokio::time::timeout(LOAD_TOLERANT_BOUND, async {
            while rx.recv().await.is_some() {}
        })
        .await;
        assert!(ended.is_ok(), "close left the reader holding the output sender");
        drop(taken);
    }

    /// A SUCCESSFUL export pauses the reader so the successor can own the stream, WITHOUT
    /// setting reader_finished so the lifecycle watcher does not kill the child process.
    #[tokio::test]
    async fn a_successful_export_pauses_the_reader_without_finishing_it() {
        let manager = PtyManager::new();
        let cmd = CommandBuilder::new("/bin/sh");
        let (session_id, _rx) = manager.spawn(cmd, 80, 24).expect("spawn PTY session");
        let session = manager.get_session(&session_id).expect("session registered");

        let export = manager.export_session(&session_id).expect("export succeeds");

        // Crucial invariant: reader must NOT be finished, because that is the exact predicate
        // the lifecycle watcher uses to close and kill the child process.
        assert!(
            !session.is_reader_finished(),
            "export must pause the reader without setting reader_finished"
        );

        // Write input to trigger reader activity so it observes pause_requested
        manager.write_input(&session_id, b"\n").expect("write input to session");

        // Await the reader's own act - it parks only once the export and the input have reached it -
        // under a bound that reports a regression instead of hanging on one, not one that grades a
        // loaded host.
        let deadline = tokio::time::Instant::now() + LOAD_TOLERANT_BOUND;
        let mut paused = false;
        while tokio::time::Instant::now() < deadline {
            if session.is_reader_paused() {
                paused = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(paused, "the reader never parked after the export and the input");

        drop(export);
        let _ = manager.close_session(&session_id).await;
    }

    #[tokio::test]
    async fn the_predecessor_does_not_close_a_session_it_exported() {
        let manager = PtyManager::new();
        let mut cmd = CommandBuilder::new("/bin/sh");
        cmd.args(["-c", "sleep 30"]);
        let (session_id, _rx) = manager.spawn(cmd, 80, 24).expect("spawn PTY session");
        let session = manager.get_session(&session_id).expect("session registered");
        let pid = session.pid().expect("session pid must exist");

        let export = manager.export_session(&session_id).expect("export succeeds");

        // The observation window is legitimate here because the behaviour under test is a periodic
        // watcher running on LIFECYCLE_POLL_INTERVAL. If the watcher erroneously considered the
        // exported session closed, it would close_session and SIGTERM the child process.
        let observation_duration = LIFECYCLE_POLL_INTERVAL * 4;
        let start = tokio::time::Instant::now();
        while start.elapsed() < observation_duration {
            assert!(
                manager.has_session(&session_id),
                "predecessor manager must retain exported session during handover"
            );
            assert_eq!(
                unsafe { libc::kill(pid as i32, 0) },
                0,
                "exported child process must remain alive"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        // Clean up with SIGKILL to the pid and drop(export)
        unsafe {
            libc::kill(pid as i32, libc::SIGKILL);
        }
        drop(export);
        let _ = manager.close_session(&session_id).await;
    }

    #[tokio::test]
    async fn resume_paused_readers_hands_an_exported_session_back() {
        let manager = PtyManager::new();
        let cmd = CommandBuilder::new("/bin/sh");
        let (session_id, mut rx) = manager.spawn(cmd, 80, 24).expect("spawn PTY session");

        let export = manager.export_session(&session_id).expect("export succeeds");

        assert_eq!(
            manager.resume_paused_readers(),
            1,
            "resume_paused_readers must resume the single exported session"
        );

        // Write a command whose OUTPUT marker is split so the echoed input never contains it
        manager
            .write_input(&session_id, b"printf 'RESUMED_%s\\n' 'MARKER'\n")
            .expect("write command");

        let mut accumulated = Vec::new();
        let mut marker_found = false;
        let deadline = tokio::time::Instant::now() + LOAD_TOLERANT_BOUND;
        while tokio::time::Instant::now() < deadline {
            match tokio::time::timeout(Duration::from_millis(100), rx.recv()).await {
                Ok(Some(chunk)) => {
                    accumulated.extend_from_slice(&chunk);
                    let text = String::from_utf8_lossy(&accumulated);
                    if text.contains("RESUMED_MARKER") {
                        marker_found = true;
                        break;
                    }
                }
                Ok(None) => break,
                Err(_) => continue,
            }
        }
        assert!(
            marker_found,
            "assembled RESUMED_MARKER must appear in output, proving reader resumed: {}",
            String::from_utf8_lossy(&accumulated)
        );

        assert_eq!(
            manager.resume_paused_readers(),
            0,
            "second resume_paused_readers must return 0"
        );

        drop(export);
        let _ = manager.close_session(&session_id).await;
    }

    /// Runs `body` on a private runtime whose shutdown is bounded, so a reader thread that outlives its
    /// session reports instead of hanging the test. The shutdown's own duration is deliberately NOT
    /// asserted: it measures how fast a loaded host is, not whether the reader was released, and the
    /// caller asserts the release itself.
    fn with_bounded_runtime<T, F: std::future::Future<Output = T>>(body: F) -> T {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build test runtime");
        let outcome = rt.block_on(body);
        rt.shutdown_background();
        outcome
    }

    #[test]
    fn closing_an_exported_session_releases_its_paused_reader() {
        // The event this test exists to observe is the reader's own end: the close releases the park the
        // reader is waiting on, the reader returns from it and finishes, and the output channel - held
        // by the reader alone once the close gives its own sender up - closes. Both are asserted, so a
        // close that leaves the park unreleased fails here instead of merely taking longer.
        let (released, reader_finished) = with_bounded_runtime(async {
            let manager = PtyManager::new();
            let (session_id, mut rx) = manager
                .spawn(CommandBuilder::new("/bin/sh"), 80, 24)
                .expect("spawn PTY session");
            let session = manager.get_session(&session_id).expect("session registered");
            let _export = manager.export_session(&session_id).expect("export succeeds");
            let close = manager.close_session(&session_id).await;
            assert_close_completed(close, "close exported session");
            let released = tokio::time::timeout(LOAD_TOLERANT_BOUND, async {
                while rx.recv().await.is_some() {}
            })
            .await
            .is_ok();
            (released, session.is_reader_finished())
        });
        assert!(reader_finished, "close left the paused reader running");
        assert!(
            released,
            "a paused reader outlived its closed session: its output channel never closed"
        );
    }

    #[test]
    fn dropping_an_exported_session_releases_its_paused_reader() {
        // Once the session is gone the reader holds the last sender, so the output channel
        // closes exactly when the reader thread exits. A reader still parked keeps it open, and
        // the bounded wait turns that into a failure instead of a hang.
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build test runtime");
        let (released, child_pid) = rt.block_on(async {
            let manager = PtyManager::new();
            let mut cmd = CommandBuilder::new("/bin/sh");
            cmd.args(["-c", "sleep 30"]);
            let (session_id, mut rx) = manager.spawn(cmd, 80, 24).expect("spawn PTY session");
            let session = manager.get_session(&session_id).expect("session registered");
            let child_pid = session.pid();
            let export = manager.export_session(&session_id).expect("export succeeds");
            // The tty echo wakes the reader so it reaches the pause point and parks. A parked
            // reader raises no event, so its flag is the only thing to wait on.
            manager.write_input(&session_id, b"\n").expect("write input");
            let deadline = tokio::time::Instant::now() + LOAD_TOLERANT_BOUND;
            while !session.is_reader_paused() {
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "reader never parked after export"
                );
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            // Let go of the session the way the lifecycle watcher's exit arms do: out of the
            // registry with no close_io, so only Drop can release the parked reader.
            manager.remove_from_registry(&session_id);
            drop(session);
            drop(export);
            let released = tokio::time::timeout(LOAD_TOLERANT_BOUND, async {
                while rx.recv().await.is_some() {}
            })
            .await
            .is_ok();
            (released, child_pid)
        });
        if let Some(pid) = child_pid {
            unsafe {
                libc::kill(pid as i32, libc::SIGKILL);
            }
        }
        // Never wait on a reader that may still be parked: a failing run must report, not hang.
        rt.shutdown_background();
        assert!(
            released,
            "a paused reader outlived its dropped session: its output channel never closed"
        );
    }

    use super::*;

    #[tokio::test]
    async fn close_escalates_term_ignoring_process_group_and_reaps_bounded_without_touching_sibling(
    ) {
        let manager = PtyManager::new();

        let mut stubborn_cmd = CommandBuilder::new("/bin/sh");
        stubborn_cmd.arg("-c");
        stubborn_cmd.arg("trap '' TERM; sleep 30");
        let (stubborn_id, _stubborn_rx) = manager
            .spawn(stubborn_cmd, 80, 24)
            .expect("spawn TERM-resistant session");
        let stubborn_session = manager
            .get_session(&stubborn_id)
            .expect("stubborn session registered");

        let mut sibling_cmd = CommandBuilder::new("/bin/sh");
        sibling_cmd.arg("-c");
        sibling_cmd.arg("sleep 30");
        let (sibling_id, _sibling_rx) = manager
            .spawn(sibling_cmd, 80, 24)
            .expect("spawn sibling session");

        tokio::time::sleep(Duration::from_millis(100)).await;
        let close_result =
            tokio::time::timeout(LOAD_TOLERANT_BOUND, manager.close_session(&stubborn_id))
                .await
                .expect("close must be bounded");
        // The escalation this test guards is proven by the child's own state below, not by the
        // close's return: the KILL reap runs on a 1 s production budget a loaded host can miss, and
        // the close reports that deadline rather than `Ok` when it does. The enclosing bound is what
        // keeps the close from hanging, so its duration is not measured again here.
        assert_close_completed(close_result, "TERM-resistant close should escalate and succeed");
        assert!(
            wait_until_session_ended(&stubborn_session),
            "closed child must be reaped"
        );
        assert!(!manager.has_session(&stubborn_id));
        assert!(manager.has_session(&sibling_id));
        assert!(manager.is_alive(&sibling_id).expect("sibling state"));

        manager
            .close_session(&sibling_id)
            .await
            .expect("cleanup sibling session");
    }

    #[tokio::test]
    async fn close_after_interrupt_still_succeeds_via_escalation_and_reap() {
        let manager = PtyManager::new();
        let (session_id, _rx) = manager.spawn_shell(80, 24).expect("spawn shell");
        let session = manager
            .get_session(&session_id)
            .expect("shell session registered");

        manager
            .signal(&session_id, TerminalSignal::Interrupt)
            .expect("send interrupt through PTY");
        tokio::time::sleep(Duration::from_millis(100)).await;

        let close = tokio::time::timeout(LOAD_TOLERANT_BOUND, manager.close_session(&session_id))
            .await
            .expect("close after interrupt must be bounded");
        assert_close_completed(close, "close after interrupt must succeed after escalation");

        assert!(
            wait_until_session_ended(&session),
            "closed shell must be reaped"
        );
        assert!(!manager.has_session(&session_id));
    }

    #[test]
    fn utf8_locale_override_injected_when_environment_selects_no_locale() {
        let empty: fn(&str) -> Option<std::ffi::OsString> = |_| None;
        let (key, value) = utf8_locale_override(empty).expect("override must be produced");
        assert_eq!(key, "LANG");
        assert!(value.ends_with("UTF-8"), "locale must be UTF-8: {value}");
        if cfg!(target_os = "macos") {
            assert_eq!(value, "en_US.UTF-8");
        }
    }

    #[test]
    fn utf8_locale_override_respects_explicit_locale_variables() {
        let cases: [(&str, &str); 3] = [
            ("LANG", "ko_KR.UTF-8"),
            ("LC_ALL", "en_US.UTF-8"),
            ("LC_CTYPE", "UTF-8"),
        ];
        for (set_key, set_value) in cases {
            let lookup = move |k: &str| {
                if k == set_key {
                    Some(std::ffi::OsString::from(set_value))
                } else {
                    None
                }
            };
            assert!(
                utf8_locale_override(lookup).is_none(),
                "explicit {set_key} must suppress the override"
            );
        }
    }

    #[test]
    fn utf8_locale_override_treats_empty_locale_variables_as_unset() {
        let lookup = |k: &str| {
            if k == "LANG" {
                Some(std::ffi::OsString::new())
            } else {
                None
            }
        };
        assert!(
            utf8_locale_override(lookup).is_some(),
            "empty LANG must not suppress the override"
        );
    }

    #[test]
    fn utf8_locale_override_is_absent_on_windows() {
        // ConPTY shells read the console code page, not LANG, so a Windows pane must receive no
        // override at all. The decision is asserted on every host, not only on Windows.
        let empty: fn(&str) -> Option<std::ffi::OsString> = |_| None;
        assert_eq!(utf8_locale_override_for(empty, true), None);

        let with_lang = |k: &str| (k == "LANG").then(|| std::ffi::OsString::from("ko_KR.UTF-8"));
        assert_eq!(utf8_locale_override_for(with_lang, true), None);

        // The unix branch is untouched: with no locale in the environment the override lands.
        assert!(utf8_locale_override_for(empty, false).is_some());
    }

    #[test]
    fn apply_session_env_sets_worktree_and_session_variables() {
        let mut cmd = CommandBuilder::new("/bin/sh");
        apply_session_env(
            &mut cmd,
            "session-test-42",
            "/path/to/worktree",
            Some("ws-42"),
        );
        assert_eq!(
            cmd.get_env("FERRYX_SESSION_ID").and_then(|s| s.to_str()),
            Some("session-test-42")
        );
        assert_eq!(
            cmd.get_env("FERRYX_WORKTREE_PATH").and_then(|s| s.to_str()),
            Some("/path/to/worktree")
        );
        assert_eq!(
            cmd.get_env("FERRYX_WORKSPACE_ID").and_then(|s| s.to_str()),
            Some("ws-42")
        );

        let mut cmd_no_ws = CommandBuilder::new("/bin/sh");
        apply_session_env(&mut cmd_no_ws, "session-test-42", "/path/to/worktree", None);
        assert_eq!(cmd_no_ws.get_env("FERRYX_WORKSPACE_ID"), None);

        let mut cmd_empty_ws = CommandBuilder::new("/bin/sh");
        apply_session_env(
            &mut cmd_empty_ws,
            "session-test-42",
            "/path/to/worktree",
            Some(""),
        );
        assert_eq!(cmd_empty_ws.get_env("FERRYX_WORKSPACE_ID"), None);
    }

    #[test]
    fn apply_session_env_normalizes_a_verbatim_windows_worktree_path() {
        // Windows canonicalization yields verbatim paths, and a pane's consumer reads this
        // variable as literal path text, so the prefix must never reach the environment.
        let mut cmd = CommandBuilder::new("/bin/sh");
        apply_session_env(
            &mut cmd,
            "session-test-42",
            r"\\?\C:\repo\.orca-worktrees\wt-slug",
            None,
        );
        assert_eq!(
            cmd.get_env("FERRYX_WORKTREE_PATH").and_then(|s| s.to_str()),
            Some(r"C:\repo\.orca-worktrees\wt-slug")
        );

        // A verbatim UNC path keeps its share and loses the prefix.
        let mut unc = CommandBuilder::new("/bin/sh");
        apply_session_env(&mut unc, "session-test-42", r"\\?\UNC\server\share\wt", None);
        assert_eq!(
            unc.get_env("FERRYX_WORKTREE_PATH").and_then(|s| s.to_str()),
            Some(r"\\server\share\wt")
        );

        // A plain path stays byte-identical, so unix panes cannot change.
        let mut plain = CommandBuilder::new("/bin/sh");
        apply_session_env(&mut plain, "session-test-42", "/path/to/worktree", None);
        assert_eq!(
            plain.get_env("FERRYX_WORKTREE_PATH").and_then(|s| s.to_str()),
            Some("/path/to/worktree")
        );
    }

    #[tokio::test]
    async fn test_pty_export_adopt_roundtrip_continuity_resize_liveness() {
        use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

        let predecessor_hub = Arc::new(TerminalOutputHub::new(4096));
        let predecessor_manager = PtyManager::new().with_output_hub(Arc::clone(&predecessor_hub));

        // 1. Spawn a real PTY session through the predecessor manager
        let cmd = CommandBuilder::new("/bin/sh");
        let (session_id, mut pred_rx) = predecessor_manager
            .spawn(cmd, 80, 24)
            .expect("spawn PTY session");

        predecessor_hub.register_session(&session_id);
        predecessor_hub.record_initial_size(&session_id, 80, 24);

        // Predecessor output pump to hub
        let p_hub = Arc::clone(&predecessor_hub);
        let p_id = session_id.clone();
        let pred_pump = tokio::spawn(async move {
            while let Some(chunk) = pred_rx.recv().await {
                p_hub.publish(&p_id, chunk);
            }
        });

        // 2. Write input before handover and wait for output to be recorded in hub
        predecessor_manager
            .write_input(&session_id, b"echo pre_handover_marker_12345\n")
            .expect("write input to predecessor");

        let mut pre_handover_found = false;
        let deadline = tokio::time::Instant::now() + LOAD_TOLERANT_BOUND;
        while tokio::time::Instant::now() < deadline {
            if let Some(att) = predecessor_hub.subscribe(&session_id) {
                let text = String::from_utf8_lossy(&att.0);
                if text.contains("pre_handover_marker_12345") {
                    pre_handover_found = true;
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(pre_handover_found, "predecessor hub must capture pre-handover output");

        // 3. Export session for transfer
        let export = predecessor_manager
            .export_session(&session_id)
            .expect("export session");

        assert_eq!(export.session_id, session_id);
        assert_eq!(export.cols, 80);
        assert_eq!(export.rows, 24);
        assert!(export.pid.is_some(), "exported session must have child PID");
        assert!(export.hub_snapshot.is_some(), "exported session must have hub snapshot");
        assert!(export.master_raw_fd >= 0, "duplicated master fd must be valid");

        // Export now pauses the predecessor's reader, so calling stop_reader is no longer needed -
        // and calling it would stop that reader for good (a stop request plus an abort), which is
        // what re-arms the watcher that closes the session and kills the transferred child.
        pred_pump.abort();

        // 4. Setup successor manager with its own TerminalOutputHub
        let successor_hub = Arc::new(TerminalOutputHub::new(4096));
        let successor_manager = PtyManager::new().with_output_hub(Arc::clone(&successor_hub));

        // 5. Test duplicate-id adoption rejection
        // Duplicate the master fd to test duplicate adoption attempt
        let dup_raw = unsafe { libc::fcntl(export.master_fd.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 0) };
        assert!(dup_raw >= 0);
        let dup_fd = unsafe { OwnedFd::from_raw_fd(dup_raw) };

        let (master_fd, snapshot) = export.into_parts();
        let snapshot_clone = snapshot.clone();

        // First adoption succeeds
        let mut succ_rx = successor_manager
            .adopt_transferred_session(master_fd, snapshot)
            .expect("adopt transferred session into successor");

        // Second adoption with the same session id MUST be rejected
        let dup_result = successor_manager.adopt_transferred_session(dup_fd, snapshot_clone);
        assert!(
            dup_result.is_err(),
            "adoption with duplicate session_id must be rejected"
        );

        // Successor output pump to successor hub
        let s_hub = Arc::clone(&successor_hub);
        let s_id = session_id.clone();
        let succ_pump = tokio::spawn(async move {
            while let Some(chunk) = succ_rx.recv().await {
                s_hub.publish(&s_id, chunk);
            }
        });

        // 6. Assert input write -> output continuity through the hub
        // Successor hub already imported historical chunks from predecessor!
        let initial_attachment = successor_hub
            .subscribe(&session_id)
            .expect("successor hub must have imported session");
        let initial_text = String::from_utf8_lossy(&initial_attachment.0);
        assert!(
            initial_text.contains("pre_handover_marker_12345"),
            "successor hub must retain pre-handover output continuity"
        );

        // Now write new input through the adopted session
        successor_manager
            .write_input(&session_id, b"echo post_handover_continuity_67890\n")
            .expect("write input to adopted session");

        let mut post_handover_found = false;
        let deadline = tokio::time::Instant::now() + LOAD_TOLERANT_BOUND;
        while tokio::time::Instant::now() < deadline {
            if let Some(att) = successor_hub.subscribe(&session_id) {
                let text = String::from_utf8_lossy(&att.0);
                if text.contains("post_handover_continuity_67890") {
                    post_handover_found = true;
                    // Verify continuity: both markers exist in the same continuous hub history
                    assert!(
                        text.contains("pre_handover_marker_12345"),
                        "hub history must contain pre-handover marker alongside post-handover marker"
                    );
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(
            post_handover_found,
            "successor hub must receive post-handover output from adopted PTY"
        );

        // 7. Assert resize works on adopted session
        successor_manager
            .resize(&session_id, 120, 35)
            .expect("resize adopted session");
        let adopted_session = successor_manager
            .get_session(&session_id)
            .expect("get adopted session");
        assert_eq!(adopted_session.get_size(), (120, 35));

        // 8. Assert adopted session liveness is observable
        assert!(
            successor_manager.is_alive(&session_id).expect("liveness check"),
            "adopted session must be reported alive while shell is running"
        );
        assert!(
            adopted_session.is_alive(),
            "adopted session object must report alive"
        );

        // Terminate adopted process and observe liveness transition to false
        successor_manager
            .signal(&session_id, TerminalSignal::Kill)
            .expect("send kill signal to adopted process");

        let mut observed_exit = false;
        let deadline = tokio::time::Instant::now() + LOAD_TOLERANT_BOUND;
        while tokio::time::Instant::now() < deadline {
            if !adopted_session.is_alive() {
                observed_exit = true;
                break;
            }
            match successor_manager.is_alive(&session_id) {
                Ok(false) => {
                    observed_exit = true;
                    break;
                }
                Err(PtyError::SessionNotFound(_)) => {
                    observed_exit = true;
                    break;
                }
                _ => tokio::time::sleep(Duration::from_millis(50)).await,
            }
        }
        assert!(observed_exit, "adopted session exit must be observable");

        succ_pump.abort();
    }

    /// Regression: closing an adopted session whose shell is running a foreground job.
    ///
    /// Job control puts the foreground job in its own process group. Handover used to record
    /// that job's group (`tcgetpgrp`) as the session's group, so Close sent TERM/KILL to the
    /// job only, the shell survived, and Close failed with "Timed out reaping killed PTY
    /// session". The shell and its job ignore TERM and HUP, so only a KILL that actually
    /// reaches each group lets Close succeed and leaves nothing behind.
    #[tokio::test]
    async fn close_adopted_session_reaps_shell_running_a_foreground_job() {
        let predecessor = PtyManager::new();
        let mut cmd = CommandBuilder::new("/bin/sh");
        cmd.arg("-c");
        cmd.arg("trap '' TERM HUP; set -m; sleep 300; sleep 300");
        let (session_id, _pred_rx) = predecessor.spawn(cmd, 80, 24).expect("spawn shell");
        let session = predecessor.get_session(&session_id).expect("session registered");
        let shell_pid = session.pid().expect("shell pid");

        // Wait until the job owns the terminal from a group other than the shell's.
        let deadline = tokio::time::Instant::now() + LOAD_TOLERANT_BOUND;
        let job_group = loop {
            if let Ok(Some(group)) = session.foreground_process_group() {
                if group != shell_pid {
                    break group;
                }
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "foreground job never took the terminal"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        };

        let export = predecessor.export_session(&session_id).expect("export session");
        assert_eq!(
            export.pgid,
            Some(shell_pid),
            "handover must record the shell's group, not the foreground job's"
        );

        // Adopt a record as older daemons wrote it: the job's group in place of the shell's.
        let (master_fd, mut snapshot) = export.into_parts();
        snapshot.pgid = Some(job_group);
        let successor = PtyManager::new();
        let _succ_rx = successor
            .adopt_transferred_session(master_fd, snapshot)
            .expect("adopt session");

        let close = tokio::time::timeout(LOAD_TOLERANT_BOUND, successor.close_session(&session_id))
            .await
            .expect("close must be bounded");
        assert_close_completed(close, "close must reap the adopted shell");

        assert!(!successor.has_session(&session_id));
        // The shell and its job are proven gone by their own process state, under a bound that reports
        // a regression instead of grading a loaded host: a close that reported the reap deadline still
        // killed them, and a close that signalled only one group leaves the other alive indefinitely.
        let shell = crate::terminal::session::AdoptedProcess { pid: shell_pid, process_group: None };
        let deadline = std::time::Instant::now() + LOAD_TOLERANT_BOUND;
        while shell.is_alive() {
            if std::time::Instant::now() >= deadline {
                panic!("the adopted shell must be gone after close");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        // The job ignores the SIGHUP a dying session leader sends, so it only goes away if
        // Close signalled the foreground group as well.
        let deadline = std::time::Instant::now() + LOAD_TOLERANT_BOUND;
        let job = crate::terminal::session::AdoptedProcess {
            pid: job_group,
            process_group: None,
        };
        while unsafe { libc::kill(-(job_group as i32), 0) } == 0 && job.is_alive() {
            if std::time::Instant::now() >= deadline {
                unsafe { libc::kill(-(job_group as i32), libc::SIGKILL) };
                panic!("close left the foreground job running");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = session.kill();
    }

    /// Regression: a successor daemon is never the adopted shell's parent, so it cannot reap
    /// it. An exited-but-unreaped process still answers `kill(pid, 0)`; liveness must report
    /// it dead instead of letting Close wait out its reap timeout.
    #[test]
    fn adopted_process_is_not_alive_once_it_is_a_zombie() {
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("300")
            .spawn()
            .expect("spawn child");
        let adopted = crate::terminal::session::AdoptedProcess {
            pid: child.id(),
            process_group: None,
        };
        assert!(adopted.is_alive(), "running child must be alive");

        child.kill().expect("kill child");
        // Deliberately not reaped: the child stays a zombie of this process.
        let deadline = std::time::Instant::now() + LOAD_TOLERANT_BOUND;
        while adopted.is_alive() {
            assert!(
                std::time::Instant::now() < deadline,
                "a killed, unreaped child must stop reporting alive"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        child.wait().expect("reap child");
    }

    /// The master release is OBSERVED and BOUNDED, and a release the blocking pool never runs is
    /// REPORTED rather than read as a completed close.
    ///
    /// The pool is a thread budget, not a duration bound: with every blocking thread busy the release
    /// sits in the queue and the master stays alive with its ConPTY handle open. The release seam used
    /// to discard the handle it created, so that condition was indistinguishable from a release that
    /// had run - which is exactly what the observation added here removes.
    #[test]
    fn a_master_release_the_blocking_pool_never_runs_is_reported() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .max_blocking_threads(1)
            .enable_all()
            .build()
            .expect("build the test runtime");
        // Occupy the single blocking thread for the whole test, so every blocking task the close
        // hands to the pool - the master release among them - queues instead of running. The occupier
        // also reports when it lets the thread go, so the test can prove the pool was free before the
        // runtime was dropped: a runtime dropped while the thread is still held shutdown-drops the
        // closures it queued, which is the very path this test exists to exercise.
        let (occupied_tx, occupied_rx) = std::sync::mpsc::channel();
        let (free_tx, free_rx) = std::sync::mpsc::channel();
        let (released_tx, released_rx) = std::sync::mpsc::channel();
        rt.spawn_blocking(move || {
            let _ = occupied_tx.send(());
            let _ = free_rx.recv();
            let _ = released_tx.send(());
        });
        occupied_rx.recv().expect("the only blocking thread is occupied");

        let (result, session, child_pid) = rt.block_on(async {
            let manager = PtyManager::new();
            let (session_id, _rx) = manager
                .spawn(CommandBuilder::new("/bin/sh"), 80, 24)
                .expect("spawn PTY session");
            let session = manager.get_session(&session_id).expect("session registered");
            let child_pid = session.pid().expect("a spawned session records its child pid");
            (manager.close_session(&session_id).await, session, child_pid)
        });
        free_tx.send(()).expect("free the blocking pool");
        released_rx
            .recv_timeout(LOAD_TOLERANT_BOUND)
            .expect("the blocking thread must be released before the runtime is dropped");

        assert_close_reported_phase(
            result,
            "master release",
            "a close whose master release the pool never ran must report it, not report success",
        );

        // The pool runs the closures it was holding as soon as the thread is free, and the session
        // reader is one of them: prove it ended rather than assume it, so the runtime is not dropped
        // under a reader that never ran.
        let reader_deadline = std::time::Instant::now() + LOAD_TOLERANT_BOUND;
        while !session.is_reader_finished() {
            assert!(
                std::time::Instant::now() < reader_deadline,
                "the reader the pool was holding never ran"
            );
            std::thread::sleep(Duration::from_millis(20));
        }

        // The close killed and reaped the child itself; prove it is gone, by the pid this test
        // recorded at spawn - never by a pattern.
        assert!(
            wait_until_child_stopped(child_pid),
            "the close left the spawned shell running"
        );
    }

    /// The concurrent-close fence must cover the WHOLE close it waits on, not only the reader stages.
    ///
    /// A second close waits for the first to leave the registry, and that happens only after the TERM
    /// grace phase, the KILL+reap phase, the observed master release and the reader stages. A fence
    /// derived from the reader stages alone is shorter than the close it observes, so a second close
    /// reports a timeout against a close that is still legitimately in progress.
    #[tokio::test]
    async fn a_concurrent_close_waits_out_the_whole_reap_and_reader_path() {
        let manager = PtyManager::new();
        let mut cmd = CommandBuilder::new("/bin/sh");
        cmd.arg("-c");
        cmd.arg("trap '' TERM; echo TRAP_READY; while true; do sleep 1; done");
        let (session_id, mut rx) = manager
            .spawn(cmd, 80, 24)
            .expect("spawn TERM-resistant session");
        let child_pid = manager
            .get_session(&session_id)
            .expect("session registered")
            .pid()
            .expect("a spawned session records its child pid");

        // Await the marker the child prints AFTER installing its TERM trap: an event, not a delay.
        // Without the trap the child would die on the first signal and the close would never reach
        // the reap phases this fence has to cover.
        let announced = tokio::time::timeout(LOAD_TOLERANT_BOUND, async {
            let mut seen = String::new();
            while let Some(chunk) = rx.recv().await {
                seen.push_str(&String::from_utf8_lossy(&chunk));
                if seen.contains("TRAP_READY") {
                    return true;
                }
            }
            false
        })
        .await
        .expect("the child must announce its trap within the bound");
        assert!(announced, "the child never announced its installed TERM trap");

        // The first close spends a whole grace phase refusing to die; the second arrives while it is
        // still in that phase and must wait for it rather than time out against it.
        let authorize: Arc<dyn Fn() -> Result<(), String> + Send + Sync> = Arc::new(|| Ok(()));
        let started = tokio::time::Instant::now();
        let (first, second) = tokio::join!(
            manager.close_authorized(&session_id, Duration::from_secs(6), authorize),
            manager.close_session(&session_id),
        );
        let elapsed = started.elapsed();

        assert!(
            elapsed >= Duration::from_secs(6),
            "the first close must spend its whole TERM grace phase, not {elapsed:?}"
        );
        // `Ok`, or the KILL reap deadline a loaded host can miss: the child's own state below is what
        // proves the escalation, and the second close below is what proves the fence.
        assert_close_completed(first, "the first close must escalate and succeed");
        second.expect("a second close must not time out against a close still in progress");
        assert!(!manager.has_session(&session_id), "a closed session leaves the registry");

        // The close killed and reaped the child itself; prove it is gone rather than leave it behind,
        // by the pid this test recorded at spawn - never by a pattern.
        assert!(
            wait_until_child_stopped(child_pid),
            "the close left the spawned shell running"
        );
    }

    /// F-1: a concurrent close is fenced for the close ACTUALLY in flight, not for its own grace and
    /// not for the module default.
    ///
    /// The grace phase is a caller parameter, so a bound derived from the default one is short by the
    /// difference on every path that passes a longer one - here the production machine-close grace of
    /// 5 s, whose close cannot leave the registry before it has spent that whole phase. The waiting
    /// close is a plain `close_session` (1 s grace), so this is also the case a fence sized from the
    /// grace of the caller that arms it gets wrong.
    ///
    /// The bound itself is pinned by `the_close_fence_tracks_the_grace_the_close_runs_with`; this test
    /// drives the concurrent path end to end, and it is not by itself the pre-fix regression, because
    /// a fast reader lets the pre-fix 10 s constant outlast a 5 s grace.
    #[tokio::test]
    async fn a_concurrent_close_covers_the_grace_of_the_close_in_flight() {
        let manager = PtyManager::new();
        let mut cmd = CommandBuilder::new("/bin/sh");
        cmd.arg("-c");
        cmd.arg("trap '' TERM; echo TRAP_READY; while true; do sleep 1; done");
        let (session_id, mut rx) = manager
            .spawn(cmd, 80, 24)
            .expect("spawn a TERM-resistant session");

        // Await the marker the child prints AFTER installing its TERM trap: an event, not a delay.
        // Without the trap the child would die on the first signal and the close would never reach the
        // grace phase this fence has to cover.
        let announced = tokio::time::timeout(LOAD_TOLERANT_BOUND, async {
            let mut seen = String::new();
            while let Some(chunk) = rx.recv().await {
                seen.push_str(&String::from_utf8_lossy(&chunk));
                if seen.contains("TRAP_READY") {
                    return true;
                }
            }
            false
        })
        .await
        .expect("the child must announce its trap within the bound");
        assert!(announced, "the child never announced its installed TERM trap");

        // The production machine-close grace, so this exercises the same phase length the fence has to
        // cover on that path: a bound derived from the DEFAULT grace is 5 s short here.
        let grace = Duration::from_secs(5);

        let authorize: Arc<dyn Fn() -> Result<(), String> + Send + Sync> = Arc::new(|| Ok(()));
        let started = tokio::time::Instant::now();
        let (first, second) = tokio::time::timeout(LOAD_TOLERANT_BOUND, async {
            tokio::join!(
                manager.close_authorized(&session_id, grace, authorize),
                manager.close_session(&session_id),
            )
        })
        .await
        .expect("both closes must be bounded");
        let elapsed = started.elapsed();

        assert!(
            elapsed >= grace,
            "the first close must spend its whole TERM grace phase, not {elapsed:?}"
        );
        // `Ok`, or the KILL reap deadline a loaded host can miss, exactly as in the whole-reap test:
        // the second close below is what carries this fence.
        assert_close_completed(first, "the first close must escalate and succeed");
        second.expect(
            "a second close must cover the grace of the close in flight, not its own or the default",
        );
        assert!(!manager.has_session(&session_id), "a closed session leaves the registry");
    }
    /// A concurrent close must cover a close that spends its WHOLE bound, not only the phase deadlines
    /// that bound is nominally made of.
    ///
    /// The second close waits for the first to leave the registry, and the first leaves only after its
    /// TERM grace phase, the observed master release and both bounded reader stages. This test pins
    /// every one of those to its own bound, so the close in flight outlives the pre-v3 fence constant
    /// (10 s) and the second close can only return `Ok` if the fence is derived from the grace that
    /// close actually runs - recorded by the close itself - rather than from the waiting call's own 1 s
    /// or from a constant. On the pre-v3 constant, and on a fence sized from the waiting call's grace,
    /// the second close reports `Timed out waiting for concurrent close` instead.
    ///
    /// This is `#[test]` rather than `#[tokio::test]` because it builds and enters its own runtime:
    /// the close's master release and its reader are handed to the blocking pool, and occupying that
    /// pool's only thread is what pins those phases instead of letting them finish in milliseconds.
    ///
    /// The child cannot announce its installed TERM trap through the pane - the reader that would
    /// deliver the marker is queued on the pool this test occupies - so it announces through a file,
    /// and the wait for that file is a bounded poll of the child's own act, never a fixed delay.
    #[test]
    fn a_concurrent_close_covers_a_close_that_spends_its_whole_fence() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .max_blocking_threads(1)
            .enable_all()
            .build()
            .expect("build the test runtime");
        // Occupy the pool's only blocking thread BEFORE the session exists, so the reader this spawn
        // queues is one the pool never runs: a reader that never reads cannot end, and that is what
        // forces both reader stages of the close to their bound. The occupier reports when it lets the
        // thread go, so the test can prove the pool was free before the runtime was dropped.
        let (occupied_tx, occupied_rx) = std::sync::mpsc::channel();
        let (free_tx, free_rx) = std::sync::mpsc::channel();
        let (released_tx, released_rx) = std::sync::mpsc::channel();
        rt.spawn_blocking(move || {
            let _ = occupied_tx.send(());
            let _ = free_rx.recv();
            let _ = released_tx.send(());
        });
        occupied_rx.recv().expect("the only blocking thread is occupied");

        // The readiness marker: the trap is installed before the child creates it, so its appearance
        // is the event that says a TERM will be survived. Removed first so a stale file cannot pass
        // for this child's own act.
        let marker = std::env::temp_dir().join(format!(
            "ferryx-close-fence-{}-{:?}.ready",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_file(&marker);
        let marker_arg = marker.to_string_lossy().into_owned();

        let (manager, session_id, session, child_pid, first, second, elapsed) = rt.block_on(async {
            let manager = PtyManager::new();
            let mut cmd = CommandBuilder::new("/bin/sh");
            cmd.arg("-c");
            cmd.arg("trap '' TERM; touch \"$1\"; while true; do sleep 1; done");
            cmd.arg("ferryx-close-fence");
            cmd.arg(&marker_arg);
            let (session_id, _rx) = manager
                .spawn(cmd, 80, 24)
                .expect("spawn a TERM-resistant session");
            let session = manager.get_session(&session_id).expect("session registered");
            let child_pid = session.pid().expect("a spawned session records its child pid");

            // Subscribed to BEFORE the closes and bounded: the child's own act, never a delay.
            let ready_deadline = std::time::Instant::now() + LOAD_TOLERANT_BOUND;
            while !marker.exists() {
                assert!(
                    std::time::Instant::now() < ready_deadline,
                    "the child never announced its installed TERM trap"
                );
                tokio::time::sleep(Duration::from_millis(20)).await;
            }

            // A 6 s grace, not the 5 s machine one: the close has to outlive the PRE-V3 constant
            // (10 s) on its pinned phases alone, and 6 s of grace plus the 2 s observed master release
            // and the two 2 s reader stages is what does that.
            let grace = Duration::from_secs(6);
            let authorize: Arc<dyn Fn() -> Result<(), String> + Send + Sync> = Arc::new(|| Ok(()));
            let started = tokio::time::Instant::now();
            let (first, second) = tokio::time::timeout(LOAD_TOLERANT_BOUND, async {
                tokio::join!(
                    manager.close_authorized(&session_id, grace, authorize),
                    manager.close_session(&session_id),
                )
            })
            .await
            .expect("both closes must be bounded");
            let elapsed = started.elapsed();
            (manager, session_id, session, child_pid, first, second, elapsed)
        });

        // Free the blocking thread, then prove the pool released it before the runtime is dropped: a
        // runtime dropped while its only blocking thread is held shutdown-drops the closures it
        // queued, which is not the path this test measures.
        free_tx.send(()).expect("free the blocking pool");
        released_rx
            .recv_timeout(LOAD_TOLERANT_BOUND)
            .expect("the blocking thread must be released before the runtime is dropped");

        // The close in flight cannot leave the registry before its pinned phases elapse: the TERM
        // grace it was given, the observed master release and the two bounded reader stages. This
        // lower bound is what makes the second close's `Ok` a statement about the fence rather than
        // about how fast the host happens to be.
        assert!(
            elapsed >= Duration::from_secs(12),
            "the close in flight must spend its whole bound, not {elapsed:?}"
        );
        // The saturated pool also leaves the release this close queued unrun, so that close reports
        // it: asserted rather than discarded, so a change to that report fails here instead of
        // passing silently.
        assert_close_reported_phase(
            first,
            "master release",
            "a close whose master release the pool never ran must report it",
        );
        second.expect(
            "a second close must cover a close that spends its whole bound, not time out against it",
        );
        assert!(!manager.has_session(&session_id), "a closed session leaves the registry");

        // The pool runs what it was holding as soon as the thread is free, the queued reader among
        // it: prove it ended rather than assume it, so the runtime is not dropped under a reader that
        // never ran.
        let reader_deadline = std::time::Instant::now() + LOAD_TOLERANT_BOUND;
        while !session.is_reader_finished() {
            assert!(
                std::time::Instant::now() < reader_deadline,
                "the reader the pool was holding never ran"
            );
            std::thread::sleep(Duration::from_millis(20));
        }

        // The close killed and reaped the child itself; prove it is gone by the pid this test
        // recorded at spawn - never by a pattern.
        assert!(
            wait_until_child_stopped(child_pid),
            "the close left the spawned shell running"
        );

        // The readiness marker is this test's own artifact: remove it rather than leave it behind.
        let _ = std::fs::remove_file(&marker);
    }

    /// The fence is a function of the grace the close runs with, not a constant: a constant derived
    /// from the default grace is short by exactly that difference on every other path, which is how a
    /// second close came to report a timeout for a machine close that does happen.
    ///
    /// This is the regression that FAILS on the pre-fix code. There the bound was the module constant
    /// - `TERM_GRACE_TIMEOUT + KILL_REAP_TIMEOUT + MASTER_RELEASE_TIMEOUT + READER_SHUTDOWN_TIMEOUT *
    /// READER_WAIT_STAGES` = 10 s - with no function of the grace at all, so the second assertion
    /// below, which requires 14.14 s for the 5 s machine grace, cannot be satisfied by it.
    ///
    /// The poll quantization of the bounded waits is part of that sum: dropping those terms from
    /// `close_fence_for` (14 s here) fails the second assertion too, which is what keeps the
    /// allowance from being removed without a test noticing.
    #[test]
    fn the_close_fence_tracks_the_grace_the_close_runs_with() {
        let default_fence = close_fence_for(TERM_GRACE_TIMEOUT);
        let machine_fence = close_fence_for(Duration::from_secs(5));
        assert_eq!(
            machine_fence - default_fence,
            Duration::from_secs(5) - TERM_GRACE_TIMEOUT,
            "the fence must move with the grace it is derived from"
        );
        assert_eq!(
            machine_fence,
            Duration::from_secs(5)
                + KILL_REAP_TIMEOUT
                + MASTER_RELEASE_TIMEOUT
                + READER_SHUTDOWN_TIMEOUT * READER_WAIT_STAGES
                + REAP_POLL_INTERVAL * 2
                + READER_WAIT_POLL_INTERVAL * 2,
            "the fence must cover every phase the close runs after its grace phase"
        );
    }

    /// F-2: a master release the pool CANCELS is not a release.
    ///
    /// In the pinned tokio, `spawn_blocking` returns its handle unchanged when the pool is already
    /// shutting down, but `spawn_task` has already shutdown()-ed the task: the closure - and with it
    /// the master - is dropped inline on the thread that called it, and the handle resolves with a
    /// cancellation. Reading `is_ok()` off that wait reported a release that never ran as an observed
    /// one, on the very thread the seam exists to keep free.
    #[tokio::test]
    async fn a_master_release_the_pool_cancels_is_not_reported_as_a_release() {
        let manager = PtyManager::new();
        let (session_id, _rx) = manager
            .spawn(CommandBuilder::new("/bin/sh"), 80, 24)
            .expect("spawn PTY session");
        let session = manager.get_session(&session_id).expect("session registered");
        let child_pid = session.pid().expect("a spawned session records its child pid");

        // A pool that is ALREADY shutting down. Entering its handle makes it the current runtime for
        // the release below, so the release is handed to a pool that can only cancel it.
        let cancelled_pool = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .max_blocking_threads(1)
            .enable_all()
            .build()
            .expect("build the shutting-down runtime");
        let handle = cancelled_pool.handle().clone();
        cancelled_pool.shutdown_background();

        {
            let _entered = handle.enter();
            session.close_io();
        }

        // Observed from a live runtime: the cancelled handle resolves at once, and the report must not
        // read that cancellation as the pool having run the release.
        let observed = tokio::time::timeout(
            LOAD_TOLERANT_BOUND,
            session.observe_master_release(MASTER_RELEASE_TIMEOUT),
        )
        .await
        .expect("the observation must be bounded");
        assert_eq!(
            observed,
            Some(false),
            "a master release the pool cancelled must not be reported as a release that happened"
        );

        // The child is this session's own and the master is already gone, so the close is what ends
        // it. The result is ASSERTED rather than discarded, and the child is proven gone by the pid
        // this test recorded at spawn - never by a pattern - so a failing close fails this test
        // instead of being swallowed with a shell left behind.
        let closed = tokio::time::timeout(LOAD_TOLERANT_BOUND, manager.close_session(&session_id))
            .await
            .expect("the close must be bounded");
        assert_close_completed(
            closed,
            "a session whose master release the pool cancelled must still close",
        );
        assert!(!manager.has_session(&session_id), "a closed session leaves the registry");

        assert!(
            wait_until_child_stopped(child_pid),
            "the close left the spawned shell running"
        );
    }

    /// F-4: `stop_reader` must wake a reader parked by an export, not leave a thread nothing can wake.
    ///
    /// The park waits on `pause_released` and no longer observes `reader_finished`, so a stop that only
    /// set that flag left the parked reader parked forever while `is_reader_finished()` already
    /// reported true - a close would pass its first stage on the flag without ever ending the thread,
    /// and a parked reader holds the last output sender, which is the chain the pause split exists to
    /// break.
    #[tokio::test]
    async fn stopping_a_paused_reader_wakes_it_instead_of_leaving_it_parked() {
        let manager = PtyManager::new();
        let (session_id, mut rx) = manager
            .spawn(CommandBuilder::new("/bin/sh"), 80, 24)
            .expect("spawn PTY session");
        let session = manager.get_session(&session_id).expect("session registered");

        // A successful export pauses the reader, and the echo of this input is what returns its read so
        // it reaches the pause point and parks there.
        let _export = manager.export_session(&session_id).expect("export succeeds");
        session.write_input(b"\n").expect("wake the reader");

        let parked_deadline = tokio::time::Instant::now() + LOAD_TOLERANT_BOUND;
        while !session.is_reader_paused() {
            assert!(
                tokio::time::Instant::now() < parked_deadline,
                "the reader never parked at its pause point"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        // Out of the registry first: the lifecycle watcher closes a session whose reader flag is set,
        // and that close releases the park itself, which would answer the question this test asks
        // instead of letting the stop do it. The output sender this session holds is given up here too,
        // so the channel below closes on the READER sender alone, which is the reader thread.
        manager.remove_from_registry(&session_id);
        session.close_output();
        session.stop_reader();

        // The child is signalled through the session itself, so a failing run cannot leave a shell
        // behind; the parked reader is not reading, so this does not touch the park under test.
        session.kill().expect("kill the child");
        // Deliberately NOT waiting here for the child to be observed as exited. That wait measures the
        // HOST, not this test: the signal has already been sent above, and a child the kernel has not
        // finished tearing down - one parked in an uninterruptible syscall on a host at load average
        // 100 - keeps `poll_exit_code()` at `Ok(None)` for as long as that takes, which is a property of
        // the machine and not of `stop_reader`. The observable this test exists for is the reader's own
        // end, asserted below, and that does not depend on the child: `stop_reader` releases the park,
        // the reader returns from it and finishes, and the last output sender - the session gave up its
        // own above - drops with it.

        // The reader holds the last output sender, so this channel closing is the exact moment its
        // thread exited - an event to await, not a delay to wait out.
        let ended = tokio::time::timeout(LOAD_TOLERANT_BOUND, async {
            while rx.recv().await.is_some() {}
        })
        .await;
        assert!(
            ended.is_ok(),
            "stop_reader left a parked reader that nothing will wake"
        );
    }

    /// C1: a pause release must not be readable as the reader's end.
    ///
    /// `close_io` releases a reader parked by an export, and that release used to set
    /// `reader_finished` - the very flag `end_reader_after_close` stage 1 reads as proof the reader
    /// thread ended - so a session closed while paused could pass stage 1 on the release alone, with
    /// the reader still running. The release is now its own signal, and only the reader's own end sets
    /// that flag.
    #[tokio::test]
    async fn a_pause_release_is_not_readable_as_the_readers_end() {
        let manager = PtyManager::new();
        let mut cmd = CommandBuilder::new("/bin/sh");
        cmd.args(["-c", "sleep 30"]);
        let (session_id, _rx) = manager.spawn(cmd, 80, 24).expect("spawn PTY session");
        let session = manager.get_session(&session_id).expect("session registered");

        // The reader issues its first read before anything pauses it. `sleep 30` writes nothing, so
        // that read stays blocked and the reader can reach neither its pause point nor its own end:
        // the flag asserted below can only have been set by the release.
        let deadline = tokio::time::Instant::now() + LOAD_TOLERANT_BOUND;
        while session.reader_phase() != "before-read" {
            assert!(
                tokio::time::Instant::now() < deadline,
                "the reader never issued its first read"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        // A successful export pauses the reader so a successor can own the stream.
        let export = manager.export_session(&session_id).expect("export succeeds");

        // The release runs inside `close_io`, exactly where the close path applies it.
        session.close_io();
        assert!(
            !session.is_reader_finished(),
            "the pause release was reported as the reader's end while the reader was still running"
        );

        // The session still closes, and the reader really ends there: the release did not strand it.
        drop(export);
        let close = tokio::time::timeout(LOAD_TOLERANT_BOUND, manager.close_session(&session_id))
            .await
            .expect("close must be bounded");
        assert_close_completed(close, "a session released while paused must still close");
        assert!(!manager.has_session(&session_id), "a closed session leaves the registry");
        assert!(
            session.is_reader_finished(),
            "the close must end the reader the release had unblocked"
        );
    }
}
