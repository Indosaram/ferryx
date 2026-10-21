//! Unix owner-death supervision for spawned SSH bridge transports.
//!
//! The bridge needs a guarantee the kernel does not give a process about itself: when the owning
//! process disappears - normal exit, panic, `SIGKILL`, a crashed daemon - every local `ssh`
//! transport it spawned must still be terminated and reaped, while the remote PTY stays alive
//! for the next reconnect. `atexit` cannot express that (`SIGKILL` skips it) and macOS has no
//! `PR_SET_PDEATHSIG` equivalent, so supervision runs in a *separate process* started in a
//! hidden mode:
//!
//! ```text
//!   owner process                      supervisor process                 ssh process
//!   -------------                      ------------------                 -----------
//!   Owner { lease_write,   --spawn-->  run_supervisor_mode()   --spawn-->  ssh ...
//!           stdio, report }     \                                          |
//!                                \-- lease pipe (read end inherited) -------+
//!                                                                           \
//!                                            stdio passed as explicit fds (no pump)
//! ```
//!
//! * The supervisor is the real parent of `ssh`, so it ends and reaps the exact child through
//!   its own handle: never by pid, never by process group.
//! * The owner holds the write end of a lease pipe while the supervisor inherits the read end.
//!   When the owner dies for any reason the kernel closes the write end, the supervisor sees EOF
//!   and ends the transport. `Owner::shutdown` reaches the same path explicitly.
//! * The transport's stdio is handed over as explicit descriptor numbers, so the supervisor's own
//!   stdio never touches the bridge protocol and there is no forwarding pump to corrupt it.
//!
//! Production runs the *current executable* in supervisor mode, which the process entry point
//! must dispatch before daemon/GUI initialization:
//!
//! ```ignore
//! if let Some(code) = ferryx_lib::ssh::transport_unix::run_supervisor_mode() {
//!     std::process::exit(code);
//! }
//! ```
//!
//! That call also records that this executable understands the supervisor contract. Without it
//! the bridge keeps spawning unsupervised children rather than launching a process that would
//! misinterpret the supervisor argument.

use std::ffi::{OsStr, OsString};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command as StdCommand, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Hidden argv marker that puts a process into transport-supervisor mode.
pub const SUPERVISOR_FLAG: &str = "--ferryx-ssh-supervisor";

/// Test hook: the ignored test that hosts supervisor mode inside a test binary, whose harness
/// cannot take custom argv. Only referenced when [`ENV_SUPERVISOR_LIBTEST`] is set.
#[doc(hidden)]
pub const SUPERVISOR_ENTRY_TEST: &str = "ssh_bridge_transport_supervisor_entry";

#[cfg(test)]
const ENV_SUPERVISOR_PROGRAM: &str = "FERRYX_SSH_SUPERVISOR_PROGRAM";
#[cfg(test)]
const ENV_SUPERVISOR_LIBTEST: &str = "FERRYX_SSH_SUPERVISOR_LIBTEST";
/// Test hook: JSON supervisor spec delivered through the environment.
const ENV_SUPERVISOR_SPEC: &str = "FERRYX_SSH_SUPERVISOR_SPEC";

/// One startup failure line the supervisor may write to its own stderr, before the transport
/// owns that descriptor. It never writes afterwards.
const SUPERVISOR_FAILURE_PREFIX: &str = "FERRYX_SSH_SUPERVISOR_FAILED: ";

/// How long the supervisor waits between checks for its transport having exited.
const SUPERVISOR_TICK: Duration = Duration::from_millis(250);
/// How long the owner waits for the supervisor to report the transport pid.
const REPORT_DEADLINE: Duration = Duration::from_secs(2);

static SUPERVISOR_MODE_AVAILABLE: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SupervisorSpec {
    lease_fd: RawFd,
    report_fd: RawFd,
    stdin_fd: RawFd,
    stdout_fd: RawFd,
    stderr_fd: RawFd,
    program: String,
    args: Vec<String>,
}

/// Returns `Some(exit_code)` when this process was launched as a transport supervisor, and
/// `None` for every ordinary invocation.
///
/// Entry points must call this before daemon/GUI initialization, next to
/// `ssh::password::run_askpass`. The `None` path also records that this executable implements
/// supervisor mode, which is what allows bridge spawns to supervise their transports.
pub fn run_supervisor_mode() -> Option<i32> {
    match requested_spec() {
        Some(spec) => Some(run_supervisor(&spec)),
        None => {
            SUPERVISOR_MODE_AVAILABLE.store(true, Ordering::SeqCst);
            None
        }
    }
}

/// True when this executable dispatched [`run_supervisor_mode`] at startup, i.e. spawning it in
/// supervisor mode is known to be understood rather than guessed.
pub fn supervisor_mode_available() -> bool {
    SUPERVISOR_MODE_AVAILABLE.load(Ordering::SeqCst)
}

fn requested_spec() -> Option<String> {
    let mut args = std::env::args_os();
    let _program = args.next();
    if args.next().as_deref() == Some(OsStr::new(SUPERVISOR_FLAG)) {
        return args.next().map(|spec| spec.to_string_lossy().into_owned());
    }
    #[cfg(test)]
    {
        std::env::var(ENV_SUPERVISOR_SPEC).ok()
    }
    #[cfg(not(test))]
    {
        None
    }
}

/// Owner-side handle for one supervised transport.
///
/// The owner keeps the lease write end (its death ends the transport), the parent ends of the
/// transport's stdio, and the read end of the pid report pipe. Nothing here signals a pid.
pub struct Owner {
    lease_write: Option<OwnedFd>,
    report_read: Option<OwnedFd>,
    child_ends: Option<ChildEnds>,
    stdio: Option<OwnerStdio>,
    transport_pid: Option<u32>,
    supervisor_program: PathBuf,
}

struct ChildEnds {
    lease: OwnedFd,
    report: OwnedFd,
    stdin: OwnedFd,
    stdout: OwnedFd,
    stderr: OwnedFd,
}

impl ChildEnds {
    fn descriptors(&self) -> [RawFd; 5] {
        [
            self.lease.as_raw_fd(),
            self.report.as_raw_fd(),
            self.stdin.as_raw_fd(),
            self.stdout.as_raw_fd(),
            self.stderr.as_raw_fd(),
        ]
    }
}

struct OwnerStdio {
    stdin: OwnedFd,
    stdout: OwnedFd,
    stderr: OwnedFd,
}

impl Owner {
    /// Prepares `command` to run as a supervised transport.
    ///
    /// `command` carries the transport program, its arguments, the environment additions, and an
    /// optional working directory; the caller spawns it afterwards and calls [`Owner::attach`].
    /// The transport's stdio is created here and must be taken with [`Owner::take_stdio`] once
    /// the spawn succeeded, so the caller must not configure stdio on `command` itself.
    pub fn prepare(command: &mut tokio::process::Command) -> io::Result<Self> {
        Self::prepare_inner(command, None, None)
    }

    #[cfg(test)]
    pub(crate) fn prepare_for_test(
        command: &mut tokio::process::Command,
        libtest_entry: bool,
        supervisor_program_override: Option<&Path>,
    ) -> io::Result<Self> {
        Self::prepare_inner(command, Some(libtest_entry), supervisor_program_override)
    }

    fn prepare_inner(
        command: &mut tokio::process::Command,
        test_entry: Option<bool>,
        supervisor_program_override: Option<&Path>,
    ) -> io::Result<Self> {
        if !supervisor_mode_available() && !libtest_supervision(test_entry) {
            return Err(io::Error::other(
                "transport supervision is unavailable: the process entry point did not dispatch \
                 ssh::transport_unix::run_supervisor_mode()",
            ));
        }

        let std_command = command.as_std_mut();
        let program = std_command.get_program().to_string_lossy().into_owned();
        let args: Vec<String> = std_command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        let envs: Vec<(OsString, Option<OsString>)> = std_command
            .get_envs()
            .map(|(key, value)| (key.to_os_string(), value.map(OsString::from)))
            .collect();
        let cwd = std_command.get_current_dir().map(Path::to_path_buf);

        let lease = pipe_cloexec()?;
        let report = pipe_cloexec()?;
        let stdin_pipe = pipe_cloexec()?;
        let stdout_pipe = pipe_cloexec()?;
        let stderr_pipe = pipe_cloexec()?;

        let spec = SupervisorSpec {
            lease_fd: lease.0.as_raw_fd(),
            report_fd: report.1.as_raw_fd(),
            stdin_fd: stdin_pipe.0.as_raw_fd(),
            stdout_fd: stdout_pipe.1.as_raw_fd(),
            stderr_fd: stderr_pipe.1.as_raw_fd(),
            program,
            args,
        };
        let spec = serde_json::to_string(&spec)
            .map_err(|e| io::Error::other(format!("invalid supervisor spec: {e}")))?;

        let supervisor_program = supervisor_program(supervisor_program_override)?;
        let mut supervised =
            supervisor_command(&supervisor_program, &spec, libtest_supervision(test_entry));
        for (key, value) in envs {
            match value {
                Some(value) => {
                    supervised.env(key, value);
                }
                None => {
                    supervised.env_remove(key);
                }
            }
        }
        if let Some(cwd) = cwd {
            supervised.current_dir(cwd);
        }

        let child_ends = ChildEnds {
            lease: lease.0,
            report: report.1,
            stdin: stdin_pipe.0,
            stdout: stdout_pipe.1,
            stderr: stderr_pipe.1,
        };
        // SAFETY: the closure runs after `fork` in the supervisor child and calls only
        // async-signal-safe `fcntl` on descriptors this function owns; the owner keeps them alive
        // until after the spawn, so the numbers cannot be recycled before the fork.
        let inherited = child_ends.descriptors();
        unsafe {
            supervised.pre_exec(move || {
                for fd in inherited {
                    clear_cloexec(fd)?;
                }
                Ok(())
            });
        }
        *command.as_std_mut() = supervised;

        Ok(Self {
            lease_write: Some(lease.1),
            report_read: Some(report.0),
            child_ends: Some(child_ends),
            stdio: Some(OwnerStdio {
                stdin: stdin_pipe.1,
                stdout: stdout_pipe.0,
                stderr: stderr_pipe.0,
            }),
            transport_pid: None,
            supervisor_program,
        })
    }

    /// Binds this owner to the supervisor child that `prepare` set up.
    ///
    /// Closes the inherited child ends and learns the transport pid the supervisor reports. The
    /// child argument is the supervisor itself; Unix supervision does not need it beyond the
    /// liveness check (the Windows owner needs it to assign the process to its job object).
    pub async fn attach(mut self, supervisor_child: &tokio::process::Child) -> io::Result<Self> {
        if supervisor_child.id().is_none() {
            return Err(io::Error::other("transport supervisor is not running"));
        }
        // Close every inherited child end before waiting for the report: while this process held
        // a copy of the report write end, a supervisor that failed to start could never produce
        // the EOF that ends the wait, and the stdio ends would stay wrongly open too.
        self.child_ends = None;
        let report = self
            .report_read
            .take()
            .ok_or_else(|| io::Error::other("supervisor report pipe was already consumed"))?;
        // The bounded report read blocks, so it runs off the reactor. The lease stays with this
        // owner: a caller cancelled while the task runs still drops it, and that drop is what ends
        // any transport the supervisor already started.
        let pid = tokio::task::spawn_blocking(move || {
            let pid = read_reported_pid(report.as_raw_fd(), REPORT_DEADLINE);
            drop(report);
            pid
        })
        .await
        .map_err(|error| io::Error::other(format!("supervisor report task failed: {error}")))??;
        self.transport_pid = Some(pid);
        Ok(self)
    }

    /// The transport (`ssh`) pid the supervisor reported, once `attach` succeeded.
    pub fn transport_pid(&self) -> Option<u32> {
        self.transport_pid
    }

    /// Takes the parent ends of the transport stdio: stdin write, stdout read, stderr read.
    ///
    /// Must be called after `attach`; the caller owns the descriptors from then on.
    pub fn take_stdio(&mut self) -> Option<(OwnedFd, OwnedFd, OwnedFd)> {
        let stdio = self.stdio.take()?;
        Some((stdio.stdin, stdio.stdout, stdio.stderr))
    }

    /// Ends the transport: the supervisor terminates and reaps its child, then exits.
    pub fn shutdown(&mut self) {
        drop(self.lease_write.take());
    }

    /// Gives up supervision without ending the transport.
    ///
    /// The lease stays open (deliberately leaked, like the child handle of the legacy in-process
    /// handover), so the transport outlives this connection's drop yet still dies with this
    /// process: the kernel closes the leaked lease when the process ends. Passing the lease to a
    /// successor process would require real descriptor transfer, which production does not do
    /// because remote sessions reconnect from a persisted descriptor.
    pub fn release(&mut self) {
        std::mem::forget(self.lease_write.take());
    }

    /// The program the supervisor runs from.
    pub fn supervisor_program(&self) -> &Path {
        &self.supervisor_program
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn libtest_supervision(test_entry: Option<bool>) -> bool {
    match test_entry {
        Some(entry) => entry,
        #[cfg(test)]
        None => std::env::var_os(ENV_SUPERVISOR_LIBTEST).is_some(),
        #[cfg(not(test))]
        None => false,
    }
}

fn supervisor_program(program_override: Option<&Path>) -> io::Result<PathBuf> {
    if let Some(program) = program_override {
        return Ok(program.to_path_buf());
    }
    #[cfg(test)]
    if let Some(program) = std::env::var_os(ENV_SUPERVISOR_PROGRAM) {
        return Ok(PathBuf::from(program));
    }
    own_executable()
}

/// The program that re-runs this process's own image. Linux: `/proc/self/exe`, which the kernel
/// resolves to the running image even after the file at its path was replaced or deleted (an app
/// update or bundle replacement while the daemon keeps running). `current_exe()` reads the same
/// link as text there and then returns "<path> (deleted)", a path that does not exist, so every
/// later supervised spawn failed with ENOENT (T12 Q08, run 20261021t123034z). Elsewhere:
/// `current_exe()`.
fn own_executable() -> io::Result<PathBuf> {
    if cfg!(target_os = "linux") {
        Ok(PathBuf::from("/proc/self/exe"))
    } else {
        std::env::current_exe()
    }
}

fn supervisor_command(program: &Path, spec: &str, libtest_entry: bool) -> StdCommand {
    let mut command = StdCommand::new(program);
    if libtest_entry {
        command
            .args(["--ignored", "--nocapture", SUPERVISOR_ENTRY_TEST])
            .env(ENV_SUPERVISOR_SPEC, spec);
    } else {
        command.arg(SUPERVISOR_FLAG).arg(spec);
    }
    command
}

fn run_supervisor(spec_json: &str) -> i32 {
    let spec: SupervisorSpec = match serde_json::from_str(spec_json) {
        Ok(spec) => spec,
        Err(e) => return supervisor_failure(&format!("invalid spec: {e}")),
    };
    for (name, fd) in [
        ("lease", spec.lease_fd),
        ("report", spec.report_fd),
        ("stdin", spec.stdin_fd),
        ("stdout", spec.stdout_fd),
        ("stderr", spec.stderr_fd),
    ] {
        if let Err(e) = validate_pipe_fd(fd) {
            return supervisor_failure(&format!("invalid {name} descriptor {fd}: {e}"));
        }
    }

    // The transport must not inherit the lease read end or the report write end: the lease is the
    // owner's death note and the report fd is consumed here. The stdio descriptors are duplicated
    // to 0, 1, 2 by Command so their original descriptors must also be closed on exec.
    for (name, fd) in [
        ("lease", spec.lease_fd),
        ("report", spec.report_fd),
        ("stdin", spec.stdin_fd),
        ("stdout", spec.stdout_fd),
        ("stderr", spec.stderr_fd),
    ] {
        if let Err(e) = set_cloexec(fd) {
            return supervisor_failure(&format!("cannot protect {name} descriptor {fd}: {e}"));
        }
    }

    let mut command = StdCommand::new(&spec.program);
    command
        .args(&spec.args)
        .stdin(Stdio::from(unsafe { OwnedFd::from_raw_fd(spec.stdin_fd) }))
        .stdout(Stdio::from(unsafe { OwnedFd::from_raw_fd(spec.stdout_fd) }))
        .stderr(Stdio::from(unsafe { OwnedFd::from_raw_fd(spec.stderr_fd) }));
    #[cfg(target_os = "linux")]
    // SAFETY: the closure runs after `fork` and calls only `prctl`, which is async-signal-safe.
    unsafe {
        command.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
            Ok(())
        });
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => return supervisor_failure(&format!("spawn {} failed: {e}", spec.program)),
    };
    // Command retains its configured stdio handles after spawn. Only the transport may
    // hold these pipe ends while the supervisor watches the lease.
    drop(command);
    report_pid(spec.report_fd, child.id());

    loop {
        match child.try_wait() {
            Ok(Some(status)) => return exit_like(status),
            Ok(None) => {}
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return supervisor_failure(&format!("wait failed: {e}"));
            }
        }
        if lease_ended(spec.lease_fd, SUPERVISOR_TICK) {
            // The owner is gone: end and reap the exact child, then leave. Only the local
            // transport dies here; the remote helper keeps its PTY for the next reconnect.
            let _ = child.kill();
            let _ = child.wait();
            return 0;
        }
    }
}

fn supervisor_failure(message: &str) -> i32 {
    eprintln!("{SUPERVISOR_FAILURE_PREFIX}{message}");
    1
}

fn report_pid(fd: RawFd, pid: u32) {
    let line = format!("{pid}\n");
    // A short line into a fresh pipe cannot block or split; the error is irrelevant here.
    let _ = unsafe { libc::write(fd, line.as_ptr().cast(), line.len()) };
    let _ = unsafe { libc::close(fd) };
}

fn exit_like(status: std::process::ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    if let Some(code) = status.code() {
        return code;
    }
    if let Some(signal) = status.signal() {
        // Mirror the transport's death so the owner observes the same wait status it would have
        // observed without supervision.
        unsafe {
            libc::signal(signal, libc::SIG_DFL);
            libc::raise(signal);
        }
    }
    1
}

/// True when the lease reports its owner gone: nobody ever writes to it, so any readiness is EOF
/// or a hang-up.
fn lease_ended(fd: RawFd, timeout: Duration) -> bool {
    let mut poll_fd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    let millis = timeout.as_millis().min(i32::MAX as u128) as i32;
    let result = unsafe { libc::poll(&mut poll_fd, 1, millis) };
    if result > 0 {
        return true;
    }
    if result == 0 {
        return false;
    }
    // An interrupted poll is not a death report; any other error is treated as one because the
    // transport is disposable and the supervisor must not outlive its owner silently.
    io::Error::last_os_error().kind() != io::ErrorKind::Interrupted
}

fn read_reported_pid(fd: RawFd, deadline: Duration) -> io::Result<u32> {
    let started = Instant::now();
    let mut received: Vec<u8> = Vec::new();
    loop {
        let remaining = deadline.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "supervisor did not report a transport pid",
            ));
        }
        let mut poll_fd = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let millis = remaining.as_millis().min(i32::MAX as u128) as i32;
        let result = unsafe { libc::poll(&mut poll_fd, 1, millis) };
        if result == 0 {
            continue;
        }
        if result < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        let mut chunk = [0u8; 16];
        let read = unsafe { libc::read(fd, chunk.as_mut_ptr().cast(), chunk.len()) };
        if read == 0 {
            return Err(io::Error::other(
                "supervisor exited before reporting a transport pid",
            ));
        }
        if read < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        received.extend_from_slice(&chunk[..read as usize]);
        if received.len() > 16 {
            return Err(io::Error::other(
                "supervisor reported an oversized pid line",
            ));
        }
        if let Some(newline) = received.iter().position(|byte| *byte == b'\n') {
            let text = std::str::from_utf8(&received[..newline])
                .map_err(|_| io::Error::other("supervisor reported a non UTF-8 pid"))?;
            return text.trim().parse::<u32>().map_err(|_| {
                io::Error::other(format!("supervisor reported a malformed pid: {text:?}"))
            });
        }
    }
}

fn pipe_cloexec() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0 as RawFd; 2];
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `pipe` returned two fresh descriptors owned by this process.
    let read = unsafe { OwnedFd::from_raw_fd(fds[0]) };
    let write = unsafe { OwnedFd::from_raw_fd(fds[1]) };
    set_cloexec(read.as_raw_fd())?;
    set_cloexec(write.as_raw_fd())?;
    Ok((read, write))
}

fn set_cloexec(fd: RawFd) -> io::Result<()> {
    if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn clear_cloexec(fd: RawFd) -> io::Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn validate_pipe_fd(fd: RawFd) -> io::Result<()> {
    if fd < 0 {
        return Err(io::Error::other("negative descriptor"));
    }
    if unsafe { libc::fcntl(fd, libc::F_GETFD) } < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `fstat` only writes into the provided zeroed `stat`.
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat(fd, &mut stat) } != 0 {
        return Err(io::Error::last_os_error());
    }
    if (stat.st_mode & libc::S_IFMT) != libc::S_IFIFO {
        return Err(io::Error::other("not a pipe"));
    }
    Ok(())
}

#[cfg(test)]
mod own_executable_tests {
    use super::*;

    /// T12 Q08: a daemon whose executable was replaced on disk must still start supervisors.
    /// Linux names the image through `/proc/self/exe`; a copy of a program is started, its file is
    /// replaced the way `cp -f` replaces a busy binary (unlink, then a new file at the same path),
    /// and the text path the old `current_exe()` returned no longer exists while the image is
    /// still runnable through its `/proc/<pid>/exe` link.
    #[test]
    fn supervisor_program_survives_replacement_of_the_running_executable() {
        if !cfg!(target_os = "linux") {
            assert_eq!(own_executable().unwrap(), std::env::current_exe().unwrap());
            return;
        }
        assert_eq!(own_executable().unwrap(), PathBuf::from("/proc/self/exe"));
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("ferryx");
        std::fs::copy("/bin/sleep", &program).unwrap();
        let mut running = StdCommand::new(&program).arg("30").spawn().unwrap();
        let link = PathBuf::from(format!("/proc/{}/exe", running.id()));
        std::fs::remove_file(&program).unwrap();
        std::fs::write(&program, b"replacement bytes").unwrap();
        let text = std::fs::read_link(&link).unwrap();
        let text = text.to_string_lossy().into_owned();
        assert!(text.ends_with(" (deleted)"), "{text}");
        assert!(!Path::new(&text).exists(), "{text}");
        let status = StdCommand::new(&link).arg("0").status().unwrap();
        assert!(status.success(), "{status:?}");
        running.kill().unwrap();
        running.wait().unwrap();
    }
}
