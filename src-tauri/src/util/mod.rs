pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

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

#[cfg(not(unix))]
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
