pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// The child gets no console at all (it does not inherit the launcher's).
pub const DETACHED_PROCESS: u32 = 0x0000_0008;
/// The child leaves the launcher's job object, so closing that job (a Task Scheduler task ending,
/// a terminal tearing down its job) cannot take the daemon with it.
pub const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;

/// `JOB_OBJECT_LIMIT_BREAKAWAY_OK` | `JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK`.
const JOB_LIMITS_ALLOWING_BREAKAWAY: u32 = 0x0000_0800 | 0x0000_1000;

/// The creation flags a detached child gets. `CREATE_BREAKAWAY_FROM_JOB` makes `CreateProcess` fail
/// with access denied when the launcher sits in a job that forbids breakaway, so it is only asked for
/// when the launcher is in no job, or in one whose limits allow it (`job_limits` is `None` outside a
/// job and when the limits cannot be read is treated as forbidding it).
pub const fn detach_flags_for(in_job: bool, job_limits: Option<u32>) -> u32 {
    let breakaway = match (in_job, job_limits) {
        (false, _) => true,
        (true, Some(limits)) => limits & JOB_LIMITS_ALLOWING_BREAKAWAY != 0,
        (true, None) => false,
    };
    if breakaway {
        CREATE_BREAKAWAY_FROM_JOB | DETACHED_PROCESS
    } else {
        DETACHED_PROCESS
    }
}

/// [`detach_flags_for`] for this process.
#[cfg(windows)]
pub fn windows_detach_flags() -> u32 {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> *mut c_void;
        fn IsProcessInJob(process: *mut c_void, job: *mut c_void, result: *mut i32) -> i32;
        fn QueryInformationJobObject(
            job: *mut c_void,
            class: i32,
            info: *mut c_void,
            length: u32,
            returned: *mut u32,
        ) -> i32;
    }
    const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS: i32 = 9;
    // JOBOBJECT_EXTENDED_LIMIT_INFORMATION is 144 bytes on 64-bit Windows; `LimitFlags` is the
    // u32 at byte 16, after the two LARGE_INTEGER time limits.
    let mut info = [0u64; 18];
    let mut in_job = 0i32;
    // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no closing; IsProcessInJob reads
    // that handle and a null job (any job) and writes one i32 to a valid out pointer.
    let queried = unsafe { IsProcessInJob(GetCurrentProcess(), std::ptr::null_mut(), &mut in_job) };
    if queried == 0 {
        return detach_flags_for(true, None);
    }
    if in_job == 0 {
        return detach_flags_for(false, None);
    }
    // SAFETY: a null job handle queries the job of the calling process; `info` is a writable,
    // 8-byte-aligned buffer of exactly the length passed, and the returned-length pointer may be null.
    let read = unsafe {
        QueryInformationJobObject(
            std::ptr::null_mut(),
            JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS,
            info.as_mut_ptr().cast(),
            std::mem::size_of_val(&info) as u32,
            std::ptr::null_mut(),
        )
    };
    let limits = (read != 0).then(|| info[2] as u32);
    detach_flags_for(true, limits)
}

#[inline]
pub fn configure_no_window(cmd: &mut std::process::Command) -> &mut std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

#[inline]
pub fn configure_tokio_no_window(
    cmd: &mut tokio::process::Command,
) -> &mut tokio::process::Command {
    #[cfg(windows)]
    {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

pub fn no_window_command<S: AsRef<std::ffi::OsStr>>(program: S) -> std::process::Command {
    let mut c = std::process::Command::new(program);
    configure_no_window(&mut c);
    c
}

pub fn no_window_tokio_command<S: AsRef<std::ffi::OsStr>>(program: S) -> tokio::process::Command {
    let mut c = tokio::process::Command::new(program);
    configure_tokio_no_window(&mut c);
    c
}

/// Makes a spawned child outlive the process that launched it.
///
/// `setsid` in the child puts it in its own session and process group, so a signal aimed at the
/// launcher's group - a force quit, a logout, a session teardown - cannot reach it, and it has no
/// controlling terminal to hang up on. Without this the background daemon is just another member of
/// the GUI's group, which is how 54 of 74 terminal panes were lost on 2026-10-07.
#[cfg(unix)]
pub fn detach_launched_child(command: &mut std::process::Command) -> &mut std::process::Command {
    use std::os::unix::process::CommandExt as _;
    // SAFETY:
    // Category: Foreign Function Interface (FFI).
    // Invariant: `pre_exec` runs between fork and exec, where only async-signal-safe calls are
    // legal; `setsid` and `signal` both qualify - they take no pointers, allocate nothing, and
    // return a value instead of unwinding. A failure is returned as an `io::Error`, which the spawn
    // reports to its caller.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            libc::signal(libc::SIGHUP, libc::SIG_IGN);
            libc::signal(libc::SIGPIPE, libc::SIG_IGN);
            Ok(())
        });
    }
    command
}

/// Windows: no console, and out of the launcher's job object whenever that job allows it
/// ([`detach_flags_for`]), so the end of a Task Scheduler task or a terminal's job cannot take the
/// daemon with it.
#[cfg(windows)]
pub fn detach_launched_child(command: &mut std::process::Command) -> &mut std::process::Command {
    use std::os::windows::process::CommandExt as _;
    command.creation_flags(windows_detach_flags())
}

#[cfg(not(any(unix, windows)))]
pub fn detach_launched_child(command: &mut std::process::Command) -> &mut std::process::Command {
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn test_no_window_command_spawn_succeeds() {
        // std exposes no getter for creation flags, so the assertion here is
        // behavioral: the helper-configured command must spawn and report
        // success. The flag application itself is a single cfg(windows) call
        // site in configure_no_window, verified by review.
        assert_eq!(CREATE_NO_WINDOW, 0x0800_0000);
        let status = no_window_command("cmd")
            .args(["/C", "exit", "0"])
            .status()
            .expect("cmd spawn with CREATE_NO_WINDOW must succeed");
        assert!(status.success());
    }

    #[cfg(not(windows))]
    #[test]
    fn test_configure_no_window_passthrough() {
        let mut cmd = std::process::Command::new("echo");
        let returned = configure_no_window(&mut cmd);
        assert_eq!(returned.get_program(), "echo");

        let mut tokio_cmd = tokio::process::Command::new("echo");
        let returned_tokio = configure_tokio_no_window(&mut tokio_cmd);
        assert_eq!(returned_tokio.as_std().get_program(), "echo");
    }

    /// A detached daemon must leave the launcher's job when it can, and must still start (inside the
    /// job) when the job forbids breakaway, because asking for it there fails the whole spawn.
    #[test]
    fn detach_flags_break_away_only_where_the_job_allows_it() {
        let both = CREATE_BREAKAWAY_FROM_JOB | DETACHED_PROCESS;
        assert_eq!(detach_flags_for(false, None), both);
        assert_eq!(detach_flags_for(true, Some(0x0800)), both, "BREAKAWAY_OK");
        assert_eq!(detach_flags_for(true, Some(0x1000)), both, "SILENT_BREAKAWAY_OK");
        assert_eq!(detach_flags_for(true, Some(0x2000)), DETACHED_PROCESS, "KILL_ON_JOB_CLOSE only");
        assert_eq!(detach_flags_for(true, None), DETACHED_PROCESS, "unreadable limits");
        assert_eq!(detach_flags_for(false, None) & CREATE_NO_WINDOW, 0);
    }

    /// The daemon must not be a member of the launcher's session or process group: that membership
    /// is what let a GUI teardown take every PTY with it on 2026-10-07.
    #[cfg(unix)]
    #[test]
    fn a_detached_child_leads_its_own_session() {
        use std::io::BufRead as _;

        let mut command = std::process::Command::new("/bin/sh");
        command
            .args(["-c", "echo ready; exec sleep 30"])
            .stdout(std::process::Stdio::piped());
        detach_launched_child(&mut command);
        let mut child = command.spawn().expect("spawn detached child");

        // Reading "ready" proves the child already reached `exec`, which happens only after the
        // `pre_exec` detach ran, so the session/group ids below are the settled ones rather than a
        // pre-detach snapshot.
        let stdout = child.stdout.take().expect("child stdout");
        let mut line = String::new();
        std::io::BufReader::new(stdout)
            .read_line(&mut line)
            .expect("read child readiness line");
        assert_eq!(line.trim(), "ready");

        let pid = child.id() as libc::pid_t;
        // SAFETY:
        // Category: Foreign Function Interface (FFI).
        // Invariant: `getsid`/`getpgid` take a pid (0 meaning "this process") and return a value;
        // they dereference no pointers and have no side effects.
        let (child_session, child_group, own_session) = unsafe {
            (libc::getsid(pid), libc::getpgid(pid), libc::getsid(0))
        };
        let _ = child.kill();
        let _ = child.wait();

        assert_eq!(child_session, pid, "the child must lead its own session");
        assert_eq!(child_group, pid, "the child must lead its own process group");
        assert_ne!(child_session, own_session, "the child must not share our session");
    }
}
