use crate::terminal::output_hub::TerminalOutputHub;
#[cfg(unix)]
use crate::terminal::session::PtySessionExport;
#[cfg(unix)]
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
const TERM_GRACE_TIMEOUT: Duration = Duration::from_secs(1);
const KILL_REAP_TIMEOUT: Duration = Duration::from_secs(1);

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

/// Authority resolved by the daemon under its worktree fence, not rediscovered here.
#[derive(Debug, Clone)]
pub(crate) struct ResolvedSpawnContext {
    pub root: std::path::PathBuf,
    pub cwd: std::path::PathBuf,
    pub managed_workspace_id: Option<String>,
}

impl ResolvedSpawnContext {
    fn apply(&self, command: &mut CommandBuilder, session_id: &str) {
        command.cwd(crate::daemon::session_service::normalize_process_cwd(&self.cwd));
        // A plain-root shell must not inherit the daemon's own managed identity.
        command.env_remove("FERRYX_WORKSPACE_ID");
        apply_session_env(
            command,
            session_id,
            &self.root.to_string_lossy(),
            self.managed_workspace_id.as_deref(),
        );
    }
}

#[cfg(test)]
mod preparation_context_tests {
    use super::*;

    #[test]
    fn preparation_resolved_context_keeps_root_cwd_and_managed_identity_distinct() {
        let root = tempfile::tempdir().unwrap();
        let cwd = root.path().join("subdirectory");
        let context = ResolvedSpawnContext {
            root: root.path().to_owned(),
            cwd: cwd.clone(),
            managed_workspace_id: Some("managed-ws".into()),
        };
        let mut command = CommandBuilder::new("shell");
        command.arg("custom-argument");
        command.env("PATH", "inherited-path");
        context.apply(&mut command, "backend-id");
        assert_eq!(
            command.get_cwd().unwrap().to_str().unwrap(),
            crate::daemon::session_service::normalize_process_cwd(&cwd).to_str().unwrap()
        );
        assert_eq!(
            command.get_env("FERRYX_WORKTREE_PATH").unwrap().to_str().unwrap(),
            crate::daemon::session_service::normalize_process_cwd(root.path()).to_str().unwrap()
        );
        assert_eq!(command.get_env("FERRYX_WORKSPACE_ID").unwrap(), "managed-ws");
        assert_eq!(command.get_env("FERRYX_SESSION_ID").unwrap(), "backend-id");
        assert_eq!(command.get_env("PATH").unwrap(), "inherited-path");
        assert_eq!(command.get_argv(), &["shell", "custom-argument"]);
        ResolvedSpawnContext {
            managed_workspace_id: None,
            ..context
        }
        .apply(&mut command, "root-id");
        assert!(command.get_env("FERRYX_WORKSPACE_ID").is_none());
    }
}

#[derive(Clone, Copy)]
enum SpawnPathPolicy {
    LegacyDiscovery,
    Inherited,
}

#[derive(Clone)]
pub struct PtyManager {
    sessions: Arc<RwLock<HashMap<String, Arc<PtySession>>>>,
    cwd_probe_permits: Arc<parking_lot::Mutex<HashMap<String, Arc<tokio::sync::Semaphore>>>>,
    pty_system: Arc<Mutex<Box<dyn PtySystem + Send>>>,
    output_hub: Arc<RwLock<Option<Arc<TerminalOutputHub>>>>,
    #[cfg(unix)]
    transfer_owners: Arc<Mutex<HashMap<String, PtySessionSnapshot>>>,
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
            cwd_probe_permits: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            pty_system: Arc::new(Mutex::new(native_pty_system())),
            output_hub: Arc::new(RwLock::new(None)),
            #[cfg(unix)]
            transfer_owners: Arc::new(Mutex::new(HashMap::new())),
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
        cmd.env("FERRYX_PTY_INCARNATION", Uuid::new_v4().to_string());
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

    pub(crate) fn spawn_resolved_with_id(
        &self,
        session_id: String,
        mut cmd: CommandBuilder,
        cols: u16,
        rows: u16,
        context: ResolvedSpawnContext,
    ) -> Result<(String, mpsc::Receiver<Vec<u8>>), PtyError> {
        context.apply(&mut cmd, &session_id);
        let rx = self.spawn_with_path_policy(
            session_id.clone(),
            cmd,
            cols,
            rows,
            Some(context.root),
            SpawnPathPolicy::Inherited,
        )?;
        Ok((session_id, rx))
    }

    fn spawn_with_id_and_worktree(
        &self,
        session_id: String,
        cmd: CommandBuilder,
        cols: u16,
        rows: u16,
        worktree_path: Option<std::path::PathBuf>,
    ) -> Result<mpsc::Receiver<Vec<u8>>, PtyError> {
        self.spawn_with_path_policy(
            session_id,
            cmd,
            cols,
            rows,
            worktree_path,
            SpawnPathPolicy::LegacyDiscovery,
        )
    }

    fn spawn_with_path_policy(
        &self,
        session_id: String,
        mut cmd: CommandBuilder,
        cols: u16,
        rows: u16,
        worktree_path: Option<std::path::PathBuf>,
        path_policy: SpawnPathPolicy,
    ) -> Result<mpsc::Receiver<Vec<u8>>, PtyError> {
        if self.has_session(&session_id) {
            return Err(PtyError::Other(format!(
                "PTY session '{session_id}' already exists"
            )));
        }
        let incarnation = Uuid::new_v4().to_string();
        cmd.env("FERRYX_PTY_INCARNATION", &incarnation);

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
        match path_policy {
            SpawnPathPolicy::LegacyDiscovery => {
                if let Ok(augmented) = std::env::join_paths(super::shell::legacy_search_paths()) {
                    cmd.env("PATH", augmented);
                }
            }
            SpawnPathPolicy::Inherited => {}
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
        let session = Arc::new(PtySession::new(PtySessionConfig {
            input,
            id: session_id.clone(),
            incarnation: Some(incarnation),
            master: pair.master,
            child,
            writer,
            reader,
            cols,
            rows,
            tx,
            worktree_path,
        }));

        if let Some(hub) = self.output_hub.read().clone() {
            session.set_output_hub(hub);
        }

        self.sessions
            .write()
            .insert(session_id.clone(), Arc::clone(&session));
        session.mark_running();
        #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
        super::qa_liveness::record_owned_pty_creation(&session_id);
        self.start_lifecycle_watcher(session_id);
        Ok(rx)
    }

    fn start_lifecycle_watcher(&self, session_id: String) {
        let manager = self.clone();
        tokio::spawn(async move {
            let Some(session) = manager.get_session(&session_id) else {
                return;
            };

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
                        session.close_io();
                        session.close_output();
                        manager.remove_from_registry(&session_id);
                        break;
                    }
                }

                tokio::time::sleep(LIFECYCLE_POLL_INTERVAL).await;
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

        session.close_io();
        let _ = Self::join_reader_bounded(&session).await;
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
            tokio::time::sleep(LIFECYCLE_POLL_INTERVAL.min(Duration::from_millis(50))).await;
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

        if !session.begin_closing() {
            if matches!(
                session.state(),
                PtySessionState::Exited { .. } | PtySessionState::Failed { .. }
            ) {
                session.close_output();
                self.remove_from_registry(session_id);
                return Ok(());
            }

            let deadline = tokio::time::Instant::now() + READER_SHUTDOWN_TIMEOUT;
            while tokio::time::Instant::now() < deadline {
                if !self.has_session(session_id) {
                    return Ok(());
                }
                tokio::time::sleep(LIFECYCLE_POLL_INTERVAL).await;
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

        session.close_io();

        if let Err(reader_error) = Self::join_reader_bounded(&session).await {
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

    pub(crate) fn try_acquire_cwd_probe(
        &self,
        session_id: &str,
    ) -> Option<tokio::sync::OwnedSemaphorePermit> {
        let sessions = self.sessions.read();
        sessions.get(session_id)?;
        let semaphore = self
            .cwd_probe_permits
            .lock()
            .entry(session_id.to_owned())
            .or_insert_with(|| Arc::new(tokio::sync::Semaphore::new(1)))
            .clone();
        semaphore.try_acquire_owned().ok()
    }

    fn remove_from_registry(&self, session_id: &str) -> Option<Arc<PtySession>> {
        let mut sessions = self.sessions.write();
        self.cwd_probe_permits.lock().remove(session_id);
        sessions.remove(session_id)
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
        let mut owners = self.transfer_owners.lock();
        let expected = owners.get(&session_id).ok_or_else(|| PtyError::Other(
            format!("No authoritative predecessor identity for '{session_id}'; source retained")))?;
        validate_transfer_identity(expected, &snapshot)?;
        let mut registry = self.sessions.write();
        if snapshot.incarnation.as_deref().map(str::is_empty).unwrap_or(true) {
            return Err(PtyError::Other(format!(
                "PTY session '{session_id}' has no verifiable incarnation; source ownership retained"
            )));
        }
        if registry.contains_key(&session_id) {
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

        registry.insert(session_id.clone(), Arc::clone(&session));
        owners.remove(&session_id);
        drop(registry);
        drop(owners);

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

    #[cfg(unix)]
    pub fn expect_transferred_owner(&self, expected: PtySessionSnapshot) -> Result<(), PtyError> {
        if expected.incarnation.as_ref().is_none_or(|value| value.is_empty()) {
            return Err(PtyError::Other("Predecessor identity lacks an incarnation".into()));
        }
        self.transfer_owners.lock().insert(expected.session_id.clone(), expected);
        Ok(())
    }

    pub async fn relinquish_transferred_session(&self, session_id: &str) -> Result<(), PtyError> {
        let session = self.get_session(session_id)
            .ok_or_else(|| PtyError::SessionNotFound(session_id.into()))?;
        if !session.begin_closing() {
            return Err(PtyError::Other("Successor relinquish conflicts with session teardown".into()));
        }
        session.release_paused_reader();
        session.close_output();
        if let Some(reader) = session.take_reader_task() {
            tokio::time::timeout(READER_SHUTDOWN_TIMEOUT, reader).await
                .map_err(|_| PtyError::Other("Successor reader relinquish was not acknowledged".into()))?
                .map_err(|error| PtyError::Other(error.to_string()))?;
        }
        session.close_io();
        self.remove_from_registry(session_id);
        Ok(())
    }
}

#[cfg(unix)]
fn validate_transfer_identity(expected: &PtySessionSnapshot, actual: &PtySessionSnapshot) -> Result<(), PtyError> {
    if expected.session_id != actual.session_id || expected.incarnation.is_none()
        || expected.incarnation != actual.incarnation || expected.pid != actual.pid
        || expected.pgid != actual.pgid || expected.worktree_path != actual.worktree_path
    {
        return Err(PtyError::Other("Transferred identity domain differs from authoritative owner; source retained".into()));
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    #[tokio::test]
    async fn pane_liveness_reader_rollback_rejected_identity_keeps_source_usable() {
        let source = super::PtyManager::new();
        let successor = super::PtyManager::new();
        let command = if cfg!(windows) {
            let mut command = portable_pty::CommandBuilder::new("cmd.exe");
            command.args(["/D", "/Q", "/K"]);
            command
        } else {
            portable_pty::CommandBuilder::new("/bin/sh")
        };
        let (id, mut output) = source.spawn(command, 80, 24).unwrap();
        #[cfg(unix)]
        {
        let export = source.export_session(&id).unwrap();
        let mut expected = export.snapshot();
        expected.incarnation = Some("wrong-owner".into());
        successor.expect_transferred_owner(expected).unwrap();
        assert!(successor.adopt_transferred_export(export).is_err());
        assert_eq!(source.resume_paused_readers(), 1);
        }
        #[cfg(not(unix))]
        assert!(source.export_session(&id).is_err());
        let observed = async {
            let mut bytes = Vec::new();
            while let Some(chunk) = output.recv().await {
                bytes.extend_from_slice(&chunk);
                if bytes.windows(b"REJECTED_SOURCE_USABLE".len())
                    .any(|window| window == b"REJECTED_SOURCE_USABLE") { return; }
            }
            panic!("source output ended before marker");
        };
        let trigger = async {
            let input: &[u8] = if cfg!(windows) {
                b"echo REJECTED_SOURCE_USABLE\r\n"
            } else {
                b"printf 'REJECTED_SOURCE_%s\\n' 'USABLE'\n"
            };
            source.write_input(&id, input).unwrap();
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            tokio::join!(observed, trigger);
        }).await.unwrap();
        source.close_session(&id).await.unwrap();
    }

    #[test]
    fn pane_liveness_identity_rejects_wrong_incarnation_and_domain() {
        let expected = crate::terminal::session::PtySessionSnapshot {
            session_id: "session".into(), incarnation: Some("owner-a".into()),
            pid: Some(1), pgid: Some(1), cols: 80, rows: 24,
            worktree_path: Some(std::path::PathBuf::from("workspace")),
            state: crate::terminal::PtySessionState::Running, hub_snapshot: None,
            suspension_receipt: None,
        };
        let mut export = expected.clone();
        export.incarnation = Some("owner-b".into());
        assert!(super::validate_transfer_identity(&expected, &export).is_err());
        export = expected.clone();
        export.worktree_path = Some(std::path::PathBuf::from("other-workspace"));
        assert!(super::validate_transfer_identity(&expected, &export).is_err());
        assert!(super::validate_transfer_identity(&expected, &expected).is_ok());
    }

    #[tokio::test]
    async fn pane_liveness_reader_rollback_missing_successor_cannot_acknowledge() {
        let successor = super::PtyManager::new();
        assert!(successor.relinquish_transferred_session("not-owned").await.is_err());
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

        // Bounded 5s wait polling every 20ms for session.is_reader_paused()
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        let mut paused = false;
        while tokio::time::Instant::now() < deadline {
            if session.is_reader_paused() {
                paused = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(paused, "reader should pause within 5s after export and input");

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
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
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

    /// Runs `body` on a private runtime and reports how long that runtime took to shut down.
    /// An ordinary runtime drop waits for every blocking task without a bound, so a reader
    /// thread that outlives its session would hang the test instead of failing it.
    fn shutdown_elapsed_after<F: std::future::Future<Output = ()>>(body: F) -> Duration {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build test runtime");
        rt.block_on(body);
        let start = std::time::Instant::now();
        rt.shutdown_timeout(Duration::from_secs(3));
        start.elapsed()
    }

    #[test]
    fn closing_an_exported_session_releases_its_paused_reader() {
        let elapsed = shutdown_elapsed_after(async {
            let manager = PtyManager::new();
            let (session_id, _rx) = manager
                .spawn(CommandBuilder::new("/bin/sh"), 80, 24)
                .expect("spawn PTY session");
            let _export = manager.export_session(&session_id).expect("export succeeds");
            manager
                .close_session(&session_id)
                .await
                .expect("close exported session");
        });
        assert!(
            elapsed < Duration::from_secs(2),
            "a paused reader outlived its closed session: runtime shutdown took {elapsed:?}"
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
            let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
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
            let released = tokio::time::timeout(Duration::from_secs(5), async {
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
        let started = tokio::time::Instant::now();
        let close_result =
            tokio::time::timeout(Duration::from_secs(6), manager.close_session(&stubborn_id))
                .await
                .expect("close must be bounded");
        close_result.expect("TERM-resistant close should escalate and succeed");

        assert!(started.elapsed() < Duration::from_secs(6));
        assert!(stubborn_session.is_reaped(), "closed child must be reaped");
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

        tokio::time::timeout(Duration::from_secs(6), manager.close_session(&session_id))
            .await
            .expect("close after interrupt must be bounded")
            .expect("close after interrupt must succeed after escalation");

        assert!(session.is_reaped(), "closed shell must be reaped");
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
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
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

        // Export now pauses the predecessor's reader, so calling stop_reader is no longer needed
        // (and calling stop_reader would set reader_finished, re-arming the watcher that kills
        // the transferred child ~1.25s later).
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
        successor_manager.expect_transferred_owner(snapshot.clone()).expect("predecessor identity");

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
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
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
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
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
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
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
        successor.expect_transferred_owner(snapshot.clone()).expect("authoritative fixture owner");
        let _succ_rx = successor
            .adopt_transferred_session(master_fd, snapshot)
            .expect("adopt session");

        tokio::time::timeout(Duration::from_secs(8), successor.close_session(&session_id))
            .await
            .expect("close must be bounded")
            .expect("close must reap the adopted shell");

        assert!(!successor.has_session(&session_id));
        assert!(
            !crate::terminal::session::AdoptedProcess { pid: shell_pid, process_group: None }
                .is_alive(),
            "the adopted shell must be gone after close"
        );
        // The job ignores the SIGHUP a dying session leader sends, so it only goes away if
        // Close signalled the foreground group as well.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
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
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while adopted.is_alive() {
            assert!(
                std::time::Instant::now() < deadline,
                "a killed, unreaped child must stop reporting alive"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        child.wait().expect("reap child");
    }
}
