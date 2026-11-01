//! Linux text clipboard synchronization.
//!
//! Wayland uses `wl-copy`/`wl-paste` from `wl-clipboard`; X11/XWayland falls
//! back to `xclip`. The Wayland path inspects advertised MIME types before
//! reading the text and suppresses the macOS concealed/transient marker types.
//! X11 has no portable equivalent for `org.nspasteboard.ConcealedType`, so the
//! fallback cannot export an entry-level concealed marker; password managers
//! should disable clipboard ownership or expose only non-sensitive selections.
//!
//! A CLI clipboard has no `NSPasteboard.changeCount`. The equivalent state is
//! the content hash plus a short remote-write suppression period. This prevents
//! a remote write from echoing while still allowing a later, genuinely changed
//! local value to be sent.

use std::{
    env,
    ffi::OsString,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use thiserror::Error;

pub const MAX_CLIPBOARD_BYTES: usize = 4 * 1024;
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(500);
pub const DEFAULT_ECHO_PERIOD: Duration = Duration::from_secs(2);
/// `wl-paste`/`xclip` block until the selection owner answers. A frozen owner
/// must not hold the host session thread, so every read is bounded.
const READ_COMMAND_TIMEOUT: Duration = Duration::from_millis(500);
/// `wl-copy` forks a daemon that serves the selection and inherits the stderr
/// pipe, so that pipe never reaches EOF. Waiting for it would park the session
/// thread forever, so a write is bounded the same way a read is.
const WRITE_COMMAND_TIMEOUT: Duration = Duration::from_millis(1000);
/// Longest single poll while draining a child's stderr.
const DRAIN_SLICE: Duration = Duration::from_millis(25);
/// Cap on retained stderr text, which only ever feeds an error message.
const MAX_COMMAND_STDERR_BYTES: usize = 4 * 1024;
const MAX_TYPE_LIST_BYTES: usize = 64 * 1024;
const CONCEALED_TYPES: [&str; 2] = [
    "org.nspasteboard.ConcealedType",
    "org.nspasteboard.TransientType",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardBackend {
    Wayland,
    X11,
}

impl ClipboardBackend {
    pub fn detect() -> Result<Self, ClipboardError> {
        if env::var_os("WAYLAND_DISPLAY").is_some()
            && executable_in_path("wl-copy").is_some()
            && executable_in_path("wl-paste").is_some()
        {
            return Ok(Self::Wayland);
        }
        if env::var_os("DISPLAY").is_some() && executable_in_path("xclip").is_some() {
            return Ok(Self::X11);
        }
        Err(ClipboardError::Unavailable)
    }
}

#[derive(Debug, Error)]
pub enum ClipboardError {
    #[error("install wl-clipboard for Wayland or xclip for X11 clipboard support")]
    Unavailable,
    #[error("clipboard command failed: {0}")]
    Command(String),
    #[error("clipboard text is not valid UTF-8")]
    InvalidUtf8,
    #[error("clipboard text exceeds the 4 KiB protocol limit")]
    TooLarge,
    #[error("clipboard command did not answer within the deadline")]
    Timeout,
    #[error("clipboard I/O failed: {0}")]
    Io(#[from] io::Error),
}

/// Stable FNV-1a content hash used by the polling state machine.
pub fn content_hash(text: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Pure echo/dedup state, separated from process I/O for deterministic tests.
#[derive(Debug, Clone)]
pub struct ClipboardEchoSuppressor {
    echo_period: Duration,
    last_observed_hash: Option<u64>,
    remote_write: Option<(u64, Instant)>,
}

impl ClipboardEchoSuppressor {
    pub fn new(echo_period: Duration) -> Self {
        Self {
            echo_period,
            last_observed_hash: None,
            remote_write: None,
        }
    }

    pub fn record_remote_write(&mut self, text: &str, now: Instant) {
        let hash = content_hash(text);
        self.last_observed_hash = Some(hash);
        self.remote_write = Some((hash, now));
    }

    /// Returns true only for a new local value that should cross the wire.
    pub fn observe_local(&mut self, text: &str, now: Instant) -> bool {
        let hash = content_hash(text);
        if let Some((remote_hash, written_at)) = self.remote_write {
            if remote_hash == hash && now.saturating_duration_since(written_at) <= self.echo_period
            {
                self.last_observed_hash = Some(hash);
                return false;
            }
            if now.saturating_duration_since(written_at) > self.echo_period {
                self.remote_write = None;
            }
        }
        if self.last_observed_hash == Some(hash) {
            return false;
        }
        self.last_observed_hash = Some(hash);
        true
    }
}

pub struct LinuxClipboard {
    image: Option<Vec<u8>>,
    backend: ClipboardBackend,
    suppressor: ClipboardEchoSuppressor,
}

impl LinuxClipboard {
    pub fn new() -> Result<Self, ClipboardError> {
        Self::with_backend(ClipboardBackend::detect())
    }

    pub fn with_backend(
        backend: Result<ClipboardBackend, ClipboardError>,
    ) -> Result<Self, ClipboardError> {
        Ok(Self {
            image: None,
            backend: backend?,
            suppressor: ClipboardEchoSuppressor::new(DEFAULT_ECHO_PERIOD),
        })
    }

    pub fn backend(&self) -> ClipboardBackend {
        self.backend
    }

    pub fn apply_remote_image(&mut self, bytes: &[u8]) -> Result<(), ClipboardError> {
        crate::clipboard_image::validate_png(bytes)?;
        if self.backend != ClipboardBackend::Wayland {
            return Err(ClipboardError::Unavailable);
        }
        write_command_bounded(
            Command::new("wl-copy").args(["--type", "image/png"]),
            bytes,
            WRITE_COMMAND_TIMEOUT,
        )?;
        self.image = Some(bytes.to_vec());
        Ok(())
    }

    pub fn poll_image(&mut self) -> Result<Option<Vec<u8>>, ClipboardError> {
        if self.backend != ClipboardBackend::Wayland || self.wayland_selection_is_concealed()? {
            return Ok(None);
        }
        let types = read_command_bounded(
            Command::new("wl-paste").arg("--list-types"),
            MAX_TYPE_LIST_BYTES,
        )?;
        if !String::from_utf8_lossy(&types)
            .lines()
            .any(|mime| mime == "image/png")
        {
            self.image = None;
            return Ok(None);
        }
        let bytes = read_command_bounded(
            Command::new("wl-paste").args(["--type", "image/png"]),
            crate::clipboard_image::LIMIT,
        )?;
        crate::clipboard_image::validate_png(&bytes)?;
        if self.image.as_ref() == Some(&bytes) {
            return Ok(None);
        }
        self.image = Some(bytes.clone());
        Ok(Some(bytes))
    }

    /// Poll once. The host session should call this every
    /// [`DEFAULT_POLL_INTERVAL`].
    pub fn poll(&mut self, now: Instant) -> Result<Option<String>, ClipboardError> {
        if self.backend == ClipboardBackend::Wayland && self.wayland_selection_is_concealed()? {
            return Ok(None);
        }

        let bytes = match self.backend {
            ClipboardBackend::Wayland => read_command_bounded(
                Command::new("wl-paste").args([
                    "--no-newline",
                    "--type",
                    "text/plain;charset=utf-8",
                ]),
                MAX_CLIPBOARD_BYTES,
            )
            .or_else(|error| {
                // Some producers advertise only `text/plain`.
                if matches!(error, ClipboardError::Command(_)) {
                    read_command_bounded(
                        Command::new("wl-paste").args(["--no-newline", "--type", "text/plain"]),
                        MAX_CLIPBOARD_BYTES,
                    )
                } else {
                    Err(error)
                }
            })?,
            ClipboardBackend::X11 => read_command_bounded(
                Command::new("xclip").args(["-selection", "clipboard", "-out"]),
                MAX_CLIPBOARD_BYTES,
            )?,
        };

        if bytes.is_empty() {
            return Ok(None);
        }
        let text = String::from_utf8(bytes).map_err(|_| ClipboardError::InvalidUtf8)?;
        Ok(self.suppressor.observe_local(&text, now).then_some(text))
    }

    pub fn apply_remote_text(&mut self, text: &str, now: Instant) -> Result<(), ClipboardError> {
        if text.len() > MAX_CLIPBOARD_BYTES {
            return Err(ClipboardError::TooLarge);
        }
        let mut command = match self.backend {
            ClipboardBackend::Wayland => {
                let mut command = Command::new("wl-copy");
                command.args(["--type", "text/plain;charset=utf-8"]);
                command
            }
            ClipboardBackend::X11 => {
                let mut command = Command::new("xclip");
                command.args(["-selection", "clipboard", "-in"]);
                command
            }
        };
        write_command_bounded(&mut command, text.as_bytes(), WRITE_COMMAND_TIMEOUT)?;
        self.suppressor.record_remote_write(text, now);
        Ok(())
    }

    fn wayland_selection_is_concealed(&self) -> Result<bool, ClipboardError> {
        let types = read_command_bounded(
            Command::new("wl-paste").arg("--list-types"),
            MAX_TYPE_LIST_BYTES,
        )?;
        let types = String::from_utf8(types).map_err(|_| ClipboardError::InvalidUtf8)?;
        Ok(types
            .lines()
            .any(|mime| CONCEALED_TYPES.contains(&mime.trim())))
    }
}

/// Writes `input` to a clipboard owner's stdin and reaps the child within `timeout`.
///
/// `wait_with_output()` is unusable here. On Wayland `wl-copy` forks a daemon
/// that keeps serving the selection, that daemon inherits the stderr pipe, and
/// the pipe therefore never reaches EOF. Waiting for it parked the session
/// thread in `read(2)` forever, which no watchdog can break: shutting down the
/// TCP socket does not interrupt an unrelated pipe read. The thread then never
/// unwound, so its media pipeline was never dropped and the capture and encode
/// workers kept burning the GPU with no client attached.
///
/// The child's own exit is the completion signal instead, and stderr is only
/// read while it is already available, so an inherited write end cannot stall
/// the caller.
fn write_command_bounded(
    command: &mut Command,
    input: &[u8],
    timeout: Duration,
) -> Result<(), ClipboardError> {
    use rustix::event::{poll, PollFd, PollFlags, Timespec};

    command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let mut stdin = match child.stdin.take() {
        Some(stdin) => stdin,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ClipboardError::Command(
                "clipboard stdin was unavailable".into(),
            ));
        }
    };
    let mut stderr = child.stderr.take();

    let outcome: Result<(std::process::ExitStatus, Vec<u8>), ClipboardError> = (|| {
        let flags = rustix::fs::fcntl_getfl(&stdin).map_err(io::Error::from)?;
        rustix::fs::fcntl_setfl(&stdin, flags | rustix::fs::OFlags::NONBLOCK)
            .map_err(io::Error::from)?;
        let deadline = Instant::now() + timeout;
        let mut offset = 0;
        let mut write_error = None;
        while offset < input.len() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(ClipboardError::Timeout);
            }
            let timeout = Timespec::try_from(remaining).map_err(io::Error::other)?;
            let mut fds = [PollFd::new(&stdin, PollFlags::OUT)];
            match poll(&mut fds, Some(&timeout)) {
                Ok(0) | Err(rustix::io::Errno::INTR) => continue,
                Ok(_) => {}
                Err(error) => return Err(io::Error::from(error).into()),
            }
            match stdin.write(&input[offset..]) {
                Ok(0) => return Err(io::Error::from(io::ErrorKind::WriteZero).into()),
                Ok(count) => offset += count,
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) =>
                {
                    continue
                }
                Err(error) if error.kind() == io::ErrorKind::BrokenPipe => {
                    // A rejected owner can close stdin before reporting why.
                    // Preserve its exit status and stderr under the same deadline.
                    write_error = Some(error);
                    break;
                }
                Err(error) => return Err(error.into()),
            }
        }
        // Closing stdin marks end-of-input; otherwise the owner waits for more.
        drop(stdin);

        let mut collected = Vec::new();
        loop {
            let exited = child.try_wait()?;
            let remaining = deadline.saturating_duration_since(Instant::now());
            if exited.is_none() && remaining.is_zero() {
                return Err(ClipboardError::Timeout);
            }
            // Read only what is already available. Once the child has exited an
            // immediate poll is enough to pick up any error text, and while it
            // runs the slice is short so the deadline stays responsive.
            let slice = if exited.is_some() {
                Duration::ZERO
            } else {
                remaining.min(DRAIN_SLICE)
            };
            let slice = Timespec::try_from(slice).expect("bounded duration");
            // An empty poll is a sleep: with the pipe at EOF there is nothing to
            // watch, and polling it would spin on HUP.
            let ready = {
                let mut fds: Vec<PollFd> = match stderr.as_ref() {
                    Some(pipe) => vec![PollFd::new(pipe, PollFlags::IN)],
                    None => Vec::new(),
                };
                match poll(&mut fds, Some(&slice)) {
                    Ok(count) => count > 0,
                    Err(rustix::io::Errno::INTR) => continue,
                    Err(error) => return Err(io::Error::from(error).into()),
                }
            };
            if ready {
                if let Some(pipe) = stderr.as_mut() {
                    let mut chunk = [0; 512];
                    match pipe.read(&mut chunk) {
                        Ok(0) => stderr = None,
                        Ok(count) => {
                            if collected.len() < MAX_COMMAND_STDERR_BYTES {
                                let count = count.min(MAX_COMMAND_STDERR_BYTES - collected.len());
                                collected.extend_from_slice(&chunk[..count]);
                            }
                        }
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                        Err(error) => return Err(error.into()),
                    }
                }
            }
            if let Some(status) = exited {
                if ready && remaining > Duration::ZERO {
                    continue;
                }
                if status.success() {
                    if let Some(error) = write_error {
                        return Err(error.into());
                    }
                }
                return Ok((status, collected));
            }
        }
    })();

    match outcome {
        Ok((status, _)) if status.success() => Ok(()),
        Ok((_, collected)) => Err(ClipboardError::Command(
            String::from_utf8_lossy(&collected).trim().to_owned(),
        )),
        Err(error) => {
            // Reap on the failure path too, or a killed child stays a zombie.
            let _ = child.kill();
            let _ = child.wait();
            Err(error)
        }
    }
}

fn read_command_bounded(command: &mut Command, limit: usize) -> Result<Vec<u8>, ClipboardError> {
    read_command_bounded_within(command, limit, READ_COMMAND_TIMEOUT)
}

fn read_command_bounded_within(
    command: &mut Command,
    limit: usize,
    timeout: Duration,
) -> Result<Vec<u8>, ClipboardError> {
    use rustix::event::{poll, PollFd, PollFlags, Timespec};

    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let deadline = Instant::now() + timeout;
    let mut stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ClipboardError::Command(
                "clipboard stdout was unavailable".into(),
            ));
        }
    };

    let collected = (|| {
        let mut bytes = Vec::with_capacity(limit.min(1024));
        let mut eof = false;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(ClipboardError::Timeout);
            }
            if eof {
                if let Some(status) = child.try_wait()? {
                    return Ok((bytes, status));
                }
            }
            let slice = Timespec::try_from(remaining.min(Duration::from_millis(25)))
                .expect("bounded duration");
            let mut fds = [PollFd::new(&stdout, PollFlags::IN)];
            // EOF may precede child exit. An empty poll avoids spinning on HUP.
            let ready = match poll(if eof { &mut [] } else { &mut fds }, Some(&slice)) {
                Ok(count) => count > 0,
                Err(rustix::io::Errno::INTR) => continue,
                Err(error) => return Err(io::Error::from(error).into()),
            };
            if ready {
                let mut chunk = [0; 1024];
                let count = stdout.read(&mut chunk)?;
                eof = count == 0;
                bytes.extend_from_slice(&chunk[..count]);
                if bytes.len() > limit {
                    return Err(ClipboardError::TooLarge);
                }
            }
        }
    })();

    let (bytes, status) = match collected {
        Ok(collected) => collected,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    if !status.success() {
        let mut stderr = Vec::new();
        if let Some(pipe) = child.stderr.take() {
            let _ = pipe.take(4096).read_to_end(&mut stderr);
        }
        return Err(ClipboardError::Command(
            String::from_utf8_lossy(&stderr).trim().to_owned(),
        ));
    }
    Ok(bytes)
}

fn executable_in_path(name: &str) -> Option<PathBuf> {
    let path: OsString = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|directory| directory.join(name))
        .find(|candidate| is_executable(candidate))
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_equal_content_equally() {
        assert_eq!(content_hash("hello"), content_hash("hello"));
        assert_ne!(content_hash("hello"), content_hash("hello!"));
    }

    #[test]
    fn suppresses_remote_echo_and_unchanged_local_content() {
        let start = Instant::now();
        let mut state = ClipboardEchoSuppressor::new(Duration::from_secs(2));
        state.record_remote_write("remote", start);
        assert!(!state.observe_local("remote", start + Duration::from_millis(500)));
        assert!(!state.observe_local("remote", start + Duration::from_secs(3)));
        assert!(state.observe_local("local", start + Duration::from_secs(3)));
        assert!(!state.observe_local("local", start + Duration::from_secs(4)));
    }

    #[test]
    fn four_kibibyte_limit_is_utf8_bytes() {
        assert_eq!("é".repeat(2048).len(), MAX_CLIPBOARD_BYTES);
        assert!("é".repeat(2049).len() > MAX_CLIPBOARD_BYTES);
    }

    #[test]
    fn bounded_read_returns_short_output_and_times_out_on_a_frozen_owner() {
        let mut fast = Command::new("sh");
        fast.args(["-c", "printf clipboard"]);
        assert_eq!(
            read_command_bounded_within(&mut fast, MAX_CLIPBOARD_BYTES, Duration::from_secs(5))
                .unwrap(),
            b"clipboard".to_vec()
        );

        // A selection owner that never answers keeps stdout open forever.
        let mut frozen = Command::new("sh");
        frozen.args(["-c", "sleep 300"]);
        assert!(matches!(
            read_command_bounded_within(
                &mut frozen,
                MAX_CLIPBOARD_BYTES,
                Duration::from_millis(50)
            ),
            Err(ClipboardError::Timeout)
        ));
    }

    #[test]
    fn bounded_read_rejects_oversized_output() {
        let mut command = Command::new("sh");
        command.args(["-c", "yes x"]);
        assert!(matches!(
            read_command_bounded_within(&mut command, 16, Duration::from_secs(5)),
            Err(ClipboardError::TooLarge)
        ));
    }

    #[test]
    fn a_forked_grandchild_keeps_the_stderr_pipe_open() {
        // The shape `wait_with_output()` used to hang on: the clipboard owner
        // forks a daemon, that daemon inherits the stderr pipe, and the pipe
        // never reaches EOF even though the owner itself has already exited.
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 5 & exit 0"]);
        command.stdout(Stdio::null()).stderr(Stdio::piped());
        let mut child = command.spawn().unwrap();
        let stderr = child.stderr.take().unwrap();

        use rustix::event::{poll, PollFd, PollFlags, Timespec};
        let slice = Timespec::try_from(Duration::from_millis(300)).unwrap();
        let mut fds = [PollFd::new(&stderr, PollFlags::IN)];
        assert_eq!(
            poll(&mut fds, Some(&slice)).unwrap(),
            0,
            "an inherited write end must leave the pipe open and unreadable"
        );

        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn write_returns_once_the_owner_exits_even_if_a_grandchild_holds_the_pipe() {
        // Regression: this is the exact clipboard shape that wedged the host
        // session thread, leaking the capture and encode workers for 13 hours.
        let mut command = Command::new("sh");
        // The real owner consumes stdin before forking. Exiting before that
        // races the writer and tests BrokenPipe instead of inherited stderr.
        command.args(["-c", "cat >/dev/null; sleep 5 & exit 0"]);
        let started = Instant::now();
        write_command_bounded(&mut command, b"payload", Duration::from_secs(10))
            .expect("a forked grandchild must not stall the write");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "the write waited on the inherited pipe instead of the child's exit"
        );
    }

    #[test]
    fn write_times_out_and_reaps_a_hung_owner() {
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 5"]);
        let timeout = Duration::from_millis(150);
        let started = Instant::now();
        assert!(matches!(
            write_command_bounded(&mut command, b"payload", timeout),
            Err(ClipboardError::Timeout)
        ));
        assert!(
            started.elapsed() >= timeout,
            "a hung owner must not be reported as a failure before the deadline"
        );
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "the deadline must bound the write"
        );
    }

    #[test]
    fn write_reports_stderr_when_owner_closes_stdin_before_delivery() {
        // More than a pipe can hold forces an unfinished write regardless of
        // whether the parent or child runs first. Closing stdin forces EPIPE;
        // the owner still gets to publish its error and exit status.
        let mut command = Command::new("sh");
        command.args([
            "-c",
            "exec 0<&-; printf clipboard-refused >&2; printf '%5000s' '' | tr ' ' x >&2; exit 3",
        ]);
        match write_command_bounded(
            &mut command,
            &vec![b'x'; 1024 * 1024],
            Duration::from_secs(5),
        ) {
            Err(ClipboardError::Command(message)) => {
                assert!(message.starts_with("clipboard-refused"));
                assert_eq!(message.len(), MAX_COMMAND_STDERR_BYTES);
            }
            other => panic!("expected the owner's complete stderr, got {other:?}"),
        }
    }

    #[test]
    fn write_rejects_successful_owner_that_did_not_accept_payload() {
        let mut command = Command::new("sh");
        command.args(["-c", "exec 0<&-; exit 0"]);
        assert!(matches!(
            write_command_bounded(
                &mut command,
                &vec![b'x'; 1024 * 1024],
                Duration::from_secs(5)
            ),
            Err(ClipboardError::Io(error)) if error.kind() == io::ErrorKind::BrokenPipe
        ));
    }

    #[test]
    fn write_delivers_payload_larger_than_pipe_capacity() {
        let mut command = Command::new("sh");
        command.args(["-c", "[ \"$(wc -c)\" -eq 1048576 ]"]);
        write_command_bounded(
            &mut command,
            &vec![b'x'; 1024 * 1024],
            Duration::from_secs(5),
        )
        .expect("the owner must receive every byte before EOF");
    }

    #[test]
    fn write_times_out_when_owner_never_reads_full_pipe() {
        let mut command = Command::new("sh");
        command.args(["-c", "exec sleep 300"]);
        assert!(matches!(
            write_command_bounded(
                &mut command,
                &vec![b'x'; 1024 * 1024],
                Duration::from_millis(150)
            ),
            Err(ClipboardError::Timeout)
        ));
    }

    #[test]
    fn write_delivers_stdin_and_reports_the_owner_status() {
        let mut matching = Command::new("sh");
        matching.args(["-c", "read value; [ \"$value\" = clipboard-probe ]"]);
        assert!(
            write_command_bounded(&mut matching, b"clipboard-probe\n", Duration::from_secs(5))
                .is_ok(),
            "the owner must receive the payload on stdin"
        );

        let mut failing = Command::new("sh");
        failing.args(["-c", "echo clipboard refused >&2; exit 3"]);
        match write_command_bounded(&mut failing, b"payload", Duration::from_secs(5)) {
            Err(ClipboardError::Command(message)) => {
                assert!(message.contains("clipboard refused"));
            }
            other => panic!("expected the owner's stderr, got {other:?}"),
        }
    }
}
