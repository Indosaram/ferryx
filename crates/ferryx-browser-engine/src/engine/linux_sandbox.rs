//! Which Chromium sandbox the engine can use on Linux.
//!
//! The engine is installed in the user's data dir, where `chrome-sandbox` can never be
//! setuid root, so the setuid sandbox is out and the bundle does not ship it. Chromium's
//! namespace sandbox needs unprivileged user namespaces. Where the kernel or its policy denies
//! them (`kernel.unprivileged_userns_clone=0`, `user.max_user_namespaces=0`, Ubuntu's
//! `kernel.apparmor_restrict_unprivileged_userns=1` without an AppArmor profile for the
//! engine), Chromium would refuse to start with "No usable sandbox"; only then the engine runs
//! without the sandbox, and says so on stderr.

use std::process::Command;

/// Whether this process may create a user namespace and map its own uid in it, which is what
/// Chromium's namespace sandbox does first. Tried in a short-lived child, so this process is
/// untouched.
pub fn user_namespaces_usable() -> bool {
    use std::os::unix::process::CommandExt;
    // Ubuntu 23.10+ lets the namespace be created but strips its capabilities unless an
    // AppArmor profile allows the program, and Chromium's sandbox then fails to start.
    let restricted = std::fs::read_to_string("/proc/sys/kernel/apparmor_restrict_unprivileged_userns")
        .is_ok_and(|value| value.trim() == "1");
    if restricted {
        return false;
    }
    let uid = unsafe { libc::getuid() };
    let map = format!("0 {uid} 1\n");
    let mut probe = Command::new("/bin/sh");
    probe.args(["-c", "exit 0"]);
    // SAFETY: runs in the forked child before exec and only makes raw syscalls on memory
    // allocated before the fork.
    unsafe {
        probe.pre_exec(move || {
            if libc::unshare(libc::CLONE_NEWUSER) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            let fd = libc::open(c"/proc/self/uid_map".as_ptr(), libc::O_WRONLY);
            if fd < 0 {
                return Err(std::io::Error::last_os_error());
            }
            let written = libc::write(fd, map.as_ptr().cast(), map.len());
            let error = std::io::Error::last_os_error();
            libc::close(fd);
            if written != map.len() as isize {
                return Err(error);
            }
            Ok(())
        });
    }
    probe.status().is_ok_and(|status| status.success())
}

/// The decision for this engine process: the sandbox stays on unless user namespaces are
/// unusable. `FERRYX_ENGINE_NO_SANDBOX=1` forces it off (diagnostics only).
pub fn sandbox_usable() -> bool {
    if std::env::var_os("FERRYX_ENGINE_NO_SANDBOX").is_some_and(|value| value == "1") {
        eprintln!("ferryx-engine: sandbox disabled by FERRYX_ENGINE_NO_SANDBOX=1");
        return false;
    }
    if user_namespaces_usable() {
        return true;
    }
    eprintln!(
        "ferryx-engine: unprivileged user namespaces are unavailable (kernel or AppArmor policy); \
         Chromium's namespace sandbox cannot run, so the engine runs without the sandbox"
    );
    false
}
