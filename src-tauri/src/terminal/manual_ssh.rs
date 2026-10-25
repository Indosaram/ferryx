//! Detects an interactive `ssh` the user typed by hand inside a daemon-owned local PTY.
//!
//! Ownership comes only from this session's own terminal: the PTY foreground process group
//! (`tcgetpgrp`) and the true argv of the processes in it (`KERN_PROCARGS2` on macOS,
//! `/proc/<pid>/cmdline` on Linux). Flattened `ps` args lose quoting and spaces, so they are
//! never used. Agent detection (`foreground.rs`) is intentionally untouched.
use serde::{Deserialize, Serialize};

pub const CODE_AMBIGUOUS: &str = "MANUAL_SSH_AMBIGUOUS";
pub const CODE_UNSUPPORTED: &str = "MANUAL_SSH_UNSUPPORTED";
pub const CODE_CHANGED: &str = "MANUAL_SSH_CHANGED";
/// Inspection itself failed (syscall/snapshot error); distinct from an old daemon's uncoded error.
pub const CODE_IO: &str = "MANUAL_SSH_IO";

/// The detected foreground SSH client. Compared field-for-field before and after an upload,
/// so an ssh that exited or was replaced mid-transfer never yields a path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualSshProcess {
    pub pid: u32,
    pub foreground_group: u32,
    pub argv: Vec<String>,
    /// The binary the kernel actually executed; argv[0] may be a bare or relative name that
    /// the GUI's PATH/cwd would resolve to a different ssh.
    pub executable: Option<String>,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetectError {
    Ambiguous(String),
    Unsupported(String),
    Changed,
    Io(String),
}

impl DetectError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Ambiguous(_) => CODE_AMBIGUOUS,
            Self::Unsupported(_) => CODE_UNSUPPORTED,
            Self::Changed => CODE_CHANGED,
            Self::Io(_) => CODE_IO,
        }
    }
    pub fn message(&self) -> String {
        match self {
            Self::Ambiguous(m) | Self::Unsupported(m) | Self::Io(m) => m.clone(),
            Self::Changed => "Terminal foreground changed while inspecting SSH".into(),
        }
    }
}

/// One process in the PTY foreground group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupProcess {
    pub pid: u32,
    pub parent: u32,
    pub argv: Option<Vec<String>>,
    pub executable: Option<String>,
}

pub fn is_ssh_executable(arg0: &str) -> bool {
    let name = arg0.rsplit(['/', '\\']).next().unwrap_or(arg0);
    name.eq_ignore_ascii_case("ssh") || name.eq_ignore_ascii_case("ssh.exe")
}

/// Whether a process is ssh, decided by the kernel-reported image. argv[0] is chosen by the
/// caller (`exec -a`, wrappers, aliases), so it may only corroborate: a process named ssh that
/// runs another binary, or whose image is unreadable, is refused rather than trusted.
fn ssh_identity(process: &GroupProcess) -> Result<bool, DetectError> {
    let named_ssh = process.argv.as_ref().and_then(|a| a.first()).is_some_and(|a| is_ssh_executable(a));
    match process.executable.as_deref() {
        Some(exe) if is_ssh_executable(exe) => Ok(true),
        Some(exe) if named_ssh => Err(DetectError::Unsupported(format!("Process {} is named ssh but runs {exe}", process.pid))),
        None if named_ssh => Err(DetectError::Unsupported(format!("Executable of ssh process {} is unreadable", process.pid))),
        _ => Ok(false),
    }
}

/// Picks the single user-facing ssh from a foreground group. An ssh whose ancestor in the same
/// group is also ssh is a ProxyCommand/ProxyJump helper and is excluded.
pub fn select_candidate(processes: &[GroupProcess]) -> Result<Option<&GroupProcess>, DetectError> {
    let ssh = processes.iter().map(ssh_identity).collect::<Result<Vec<bool>, _>>()?;
    let index_of = |pid: u32| processes.iter().position(|p| p.pid == pid);
    let mut top = Vec::new();
    for process in processes.iter().enumerate().filter(|(i, _)| ssh[*i]).map(|(_, p)| p) {
        let mut cursor = index_of(process.parent);
        let mut nested = false;
        let mut hops = 0;
        while let Some(parent) = cursor {
            if hops > processes.len() {
                break;
            }
            if ssh[parent] {
                nested = true;
                break;
            }
            cursor = index_of(processes[parent].parent);
            hops += 1;
        }
        if !nested {
            top.push(process);
        }
    }
    // An unreadable argv inside the foreground group may itself be an ssh (or the real owner),
    // so any unreadable live member makes the answer unknown. Zombies are skipped by callers.
    if processes.iter().any(|p| p.argv.is_none()) {
        return Err(DetectError::Unsupported("Foreground process arguments are unreadable".into()));
    }
    match top.as_slice() {
        [] => Ok(None),
        [one] => Ok(Some(one)),
        _ => Err(DetectError::Ambiguous("Multiple SSH clients own this terminal".into())),
    }
}

/// Parses `KERN_PROCARGS2`: `argc` (i32 native-endian), exec path, NUL padding, then argv.
/// Returns `(exec_path, argv)`.
pub fn parse_procargs2(buffer: &[u8]) -> Option<(String, Vec<String>)> {
    let argc = i32::from_ne_bytes(buffer.get(..4)?.try_into().ok()?);
    if argc <= 0 {
        return None;
    }
    let mut rest = &buffer[4..];
    let end = rest.iter().position(|b| *b == 0)?;
    let exec = String::from_utf8(rest[..end].to_vec()).ok()?;
    rest = &rest[end..];
    let start = rest.iter().position(|b| *b != 0)?;
    rest = &rest[start..];
    let mut argv = Vec::with_capacity(argc as usize);
    for part in rest.split(|b| *b == 0).take(argc as usize) {
        argv.push(String::from_utf8(part.to_vec()).ok()?);
    }
    (argv.len() == argc as usize).then_some((exec, argv))
}

/// `proc_listpgrppids` returns a pid COUNT (libproc divides the byte count by `sizeof(int)`).
/// A full buffer may be truncated, so it is refused rather than read as the whole group.
pub fn listed_pids(buffer: &[i32], count: usize) -> Result<Vec<u32>, DetectError> {
    if count >= buffer.len() {
        return Err(DetectError::Unsupported("Terminal process group is too large to inspect".into()));
    }
    Ok(buffer[..count].iter().filter(|pid| **pid > 0).map(|pid| *pid as u32).collect())
}

/// Linux `/proc/<pid>/cmdline`: NUL-terminated arguments.
pub fn parse_cmdline(buffer: &[u8]) -> Option<Vec<String>> {
    let trimmed = buffer.strip_suffix(&[0]).unwrap_or(buffer);
    if trimmed.is_empty() {
        return None;
    }
    trimmed.split(|b| *b == 0).map(|p| String::from_utf8(p.to_vec()).ok()).collect()
}

/// Inspects one local daemon PTY. `Ok(None)` means a normal local foreground.
pub fn detect(session: &super::PtySession) -> Result<Option<ManualSshProcess>, DetectError> {
    platform::detect(session)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
mod platform {
    use super::*;

    pub(super) fn detect(session: &crate::terminal::PtySession) -> Result<Option<ManualSshProcess>, DetectError> {
        let io = |e: std::io::Error| DetectError::Io(e.to_string());
        let Some(group) = session.foreground_process_group().map_err(io)? else {
            return Ok(None);
        };
        let processes = group_processes(group)?;
        let found = select_candidate(&processes)?.cloned();
        // The group may be replaced while it is being read; a mixed view is not evidence.
        if session.foreground_process_group().map_err(io)? != Some(group) {
            return Err(DetectError::Changed);
        }
        Ok(found.map(|p| ManualSshProcess {
            pid: p.pid,
            foreground_group: group,
            argv: p.argv.unwrap_or_default(),
            executable: p.executable,
            cwd: crate::ipc::terminal::process_cwd(p.pid).map(|c| c.to_string_lossy().into_owned()),
        }))
    }

    #[cfg(target_os = "linux")]
    fn group_processes(group: u32) -> Result<Vec<GroupProcess>, DetectError> {
        let io = |e: std::io::Error| DetectError::Io(e.to_string());
        let mut out = Vec::new();
        for entry in std::fs::read_dir("/proc").map_err(io)? {
            let Some(pid) = entry.map_err(io)?.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else {
                continue;
            };
            let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
                continue;
            };
            // Fields after the parenthesised comm: state ppid pgrp ...
            let Some((_, rest)) = stat.rsplit_once(')') else { continue };
            let fields: Vec<&str> = rest.split_whitespace().collect();
            let (Some(parent), Some(pgrp)) = (fields.get(1).and_then(|v| v.parse().ok()), fields.get(2).and_then(|v| v.parse::<u32>().ok())) else {
                continue;
            };
            // A zombie/dead member no longer owns the terminal and has an empty cmdline.
            if pgrp != group || matches!(fields.first(), Some(&"Z" | &"X")) {
                continue;
            }
            let argv = std::fs::read(format!("/proc/{pid}/cmdline")).ok().and_then(|b| parse_cmdline(&b));
            let executable = std::fs::read_link(format!("/proc/{pid}/exe")).ok().map(|p| p.to_string_lossy().into_owned());
            out.push(GroupProcess { pid, parent, argv, executable });
        }
        Ok(out)
    }

    #[cfg(target_os = "macos")]
    fn group_processes(group: u32) -> Result<Vec<GroupProcess>, DetectError> {
        let mut pids = vec![0 as libc::pid_t; 512];
        let count = unsafe {
            libc::proc_listpgrppids(group as libc::pid_t, pids.as_mut_ptr().cast(), (pids.len() * std::mem::size_of::<libc::pid_t>()) as libc::c_int)
        };
        let count = usize::try_from(count).map_err(|_| DetectError::Io(std::io::Error::last_os_error().to_string()))?;
        let mut out = Vec::new();
        for pid in listed_pids(&pids, count)? {
            let pid = pid as libc::pid_t;
            let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
            let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
            let read = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, (&mut info as *mut libc::proc_bsdinfo).cast(), size) };
            // Exited between listing and inspection, left the group, or a zombie: not an owner.
            if read != size || info.pbi_pgid != group || info.pbi_status == libc::SZOMB {
                continue;
            }
            let argv = procargs(pid).map(|(_, argv)| argv);
            // KERN_PROCARGS2's exec path is the string handed to execve (possibly relative to
            // the exec-time cwd); proc_pidpath is the kernel-resolved image that identifies ssh.
            out.push(GroupProcess { pid: pid as u32, parent: info.pbi_ppid, argv, executable: pid_path(pid) });
        }
        Ok(out)
    }

    #[cfg(target_os = "macos")]
    fn pid_path(pid: libc::pid_t) -> Option<String> {
        // PROC_PIDPATHINFO_MAXSIZE (4 * MAXPATHLEN).
        let mut buffer = vec![0u8; 4 * 1024];
        let len = unsafe { libc::proc_pidpath(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
        if len <= 0 {
            return None;
        }
        buffer.truncate(len as usize);
        String::from_utf8(buffer).ok()
    }

    #[cfg(target_os = "macos")]
    fn procargs(pid: libc::pid_t) -> Option<(String, Vec<String>)> {
        let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
        let mut len: libc::size_t = 0;
        let ok = unsafe { libc::sysctl(mib.as_mut_ptr(), 3, std::ptr::null_mut(), &mut len, std::ptr::null_mut(), 0) };
        if ok != 0 || len == 0 {
            return None;
        }
        let mut buffer = vec![0u8; len];
        let ok = unsafe { libc::sysctl(mib.as_mut_ptr(), 3, buffer.as_mut_ptr().cast(), &mut len, std::ptr::null_mut(), 0) };
        if ok != 0 {
            return None;
        }
        buffer.truncate(len);
        parse_procargs2(&buffer)
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    /// ConPTY exposes no foreground group and Win32 CommandLine is a re-quoted string, so a
    /// descendant ssh.exe cannot be proven to own the pane: refuse it rather than guess argv.
    pub(super) fn detect(session: &crate::terminal::PtySession) -> Result<Option<ManualSshProcess>, DetectError> {
        let Some(shell) = session.pid() else { return Ok(None) };
        let output = crate::util::no_window_command("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", "Get-CimInstance Win32_Process -ErrorAction Stop | ForEach-Object { \"$($_.ProcessId) $($_.ParentProcessId) $($_.Name)\" }"])
            .output()
            .map_err(|e| DetectError::Io(e.to_string()))?;
        if !output.status.success() {
            return Err(DetectError::Io("Process snapshot failed".into()));
        }
        let rows: Vec<(u32, u32, String)> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|l| {
                let mut f = l.splitn(3, ' ');
                Some((f.next()?.parse().ok()?, f.next()?.parse().ok()?, f.next()?.trim().to_string()))
            })
            .collect();
        let mut descendants = vec![shell];
        loop {
            let before = descendants.len();
            for (pid, parent, _) in &rows {
                if descendants.contains(parent) && !descendants.contains(pid) {
                    descendants.push(*pid);
                }
            }
            if descendants.len() == before {
                break;
            }
        }
        if rows.iter().any(|(pid, _, name)| *pid != shell && descendants.contains(pid) && is_ssh_executable(name)) {
            return Err(DetectError::Unsupported("Manual SSH image paste is not supported in Windows terminals yet".into()));
        }
        Ok(None)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
mod platform {
    use super::*;
    pub(super) fn detect(_: &crate::terminal::PtySession) -> Result<Option<ManualSshProcess>, DetectError> {
        Err(DetectError::Unsupported("Manual SSH detection is unavailable on this platform".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[tokio::test]
    async fn detects_real_ssh_owned_by_the_foreground_pty() {
        use tokio::time::{timeout, Duration};
        let manager = crate::terminal::PtyManager::new();
        let (id, mut output) = manager.spawn(portable_pty::CommandBuilder::new("/bin/sh"), 80, 24).unwrap();
        let session = manager.get_session(&id).unwrap();
        session.write_input(b"stty -echo; exec /usr/bin/ssh -F /dev/null -o 'ProxyCommand=sh -c \"printf FERRYX_SSH_ >&2; printf READY >&2; while IFS= read -r line; do :; done\"' clipboard.invalid\n").unwrap();
        let ready = timeout(Duration::from_secs(10), async {
            let mut bytes = Vec::new();
            loop {
                let Some(chunk) = output.recv().await else {
                    return Err(String::from_utf8_lossy(&bytes).into_owned());
                };
                bytes.extend(chunk);
                if bytes.windows(b"FERRYX_SSH_READY".len()).any(|w| w == b"FERRYX_SSH_READY") {
                    return Ok(());
                }
            }
        }).await;
        let detected = if matches!(ready, Ok(Ok(()))) { Some(detect(&session)) } else { None };
        manager.close_session(&id).await.unwrap();
        ready.expect("SSH proxy startup event").expect("SSH exited before readiness");
        let detected = detected.unwrap().unwrap().expect("foreground SSH client");
        assert!(detected.argv.iter().any(|arg| arg == "clipboard.invalid"));
        assert!(is_ssh_executable(detected.executable.as_deref().unwrap()));
    }

    /// A process whose kernel image matches its argv[0], as for an ordinary exec.
    fn proc(pid: u32, parent: u32, argv: &[&str]) -> GroupProcess {
        let name = argv[0].rsplit('/').next().unwrap();
        GroupProcess { pid, parent, argv: Some(argv.iter().map(|s| s.to_string()).collect()), executable: Some(format!("/usr/bin/{name}")) }
    }

    #[test]
    fn ssh_identity_comes_from_the_kernel_image_not_argv0() {
        // `exec -a ssh python ...`: named ssh, runs something else.
        let spoofed = GroupProcess { executable: Some("/usr/bin/python3".into()), ..proc(20, 10, &["ssh", "box"]) };
        assert!(matches!(select_candidate(&[spoofed]), Err(DetectError::Unsupported(_))));
        // Linux reports an ssh binary replaced by an upgrade as "<path> (deleted)".
        let deleted = GroupProcess { executable: Some("/usr/bin/ssh (deleted)".into()), ..proc(20, 10, &["ssh", "box"]) };
        assert!(matches!(select_candidate(&[deleted]), Err(DetectError::Unsupported(_))));
        let unknown = GroupProcess { executable: None, ..proc(20, 10, &["ssh", "box"]) };
        assert!(matches!(select_candidate(&[unknown]), Err(DetectError::Unsupported(_))));
        // A real ssh image under another argv[0] is still the user's ssh.
        let renamed = GroupProcess { executable: Some("/opt/homebrew/Cellar/openssh/9.9/bin/ssh".into()), ..proc(20, 10, &["slogin", "box"]) };
        assert_eq!(select_candidate(&[renamed]).unwrap().unwrap().pid, 20);
        // A non-ssh binary without a readable image is not guessed to be ssh.
        let other = GroupProcess { executable: None, ..proc(11, 10, &["vim", "x"]) };
        assert_eq!(select_candidate(&[other]).unwrap(), None);
    }

    #[test]
    fn procargs2_keeps_spaces_inside_arguments() {
        let mut buf = 3i32.to_ne_bytes().to_vec();
        buf.extend_from_slice(b"/usr/bin/ssh\0\0\0ssh\0-i\0/Users/a b/key\0env=x\0");
        let (exec, argv) = parse_procargs2(&buf).unwrap();
        assert_eq!(exec, "/usr/bin/ssh");
        assert_eq!(argv, vec!["ssh", "-i", "/Users/a b/key"]);
        assert_eq!(parse_procargs2(&0i32.to_ne_bytes()), None);
        assert_eq!(parse_procargs2(&[1, 0]), None);
    }

    #[test]
    fn cmdline_splits_on_nul_only() {
        assert_eq!(parse_cmdline(b"ssh\0-o\0User=a b\0").unwrap(), vec!["ssh", "-o", "User=a b"]);
        assert_eq!(parse_cmdline(b""), None);
    }

    #[test]
    fn no_ssh_in_foreground_is_local() {
        assert_eq!(select_candidate(&[proc(10, 1, &["zsh"]), proc(11, 10, &["vim", "x"])]).unwrap(), None);
    }

    #[test]
    fn proxy_ssh_descendant_is_excluded() {
        let group = [proc(20, 10, &["ssh", "-J", "jump", "box"]), proc(21, 20, &["ssh", "-W", "[box]:22", "jump"])];
        assert_eq!(select_candidate(&group).unwrap().unwrap().pid, 20);
    }

    #[test]
    fn two_top_level_ssh_clients_are_ambiguous() {
        let group = [proc(20, 10, &["ssh", "a"]), proc(30, 10, &["/usr/bin/ssh", "b"])];
        assert!(matches!(select_candidate(&group), Err(DetectError::Ambiguous(_))));
    }

    #[test]
    fn unreadable_argv_is_never_guessed() {
        let unreadable = GroupProcess { pid: 21, parent: 10, argv: None, executable: None };
        assert!(matches!(select_candidate(std::slice::from_ref(&unreadable)), Err(DetectError::Unsupported(_))));
        // Next to a readable ssh the unreadable member could be a second client.
        let group = [proc(20, 10, &["ssh", "a"]), unreadable];
        assert!(matches!(select_candidate(&group), Err(DetectError::Unsupported(_))));
    }

    #[test]
    fn listed_pids_uses_count_not_bytes_and_refuses_full_buffer() {
        let buffer = [101, 102, 103, 0, 0, 0, 0, 0];
        assert_eq!(listed_pids(&buffer, 3).unwrap(), vec![101, 102, 103]);
        assert!(matches!(listed_pids(&buffer, 8), Err(DetectError::Unsupported(_))));
        assert_eq!(listed_pids(&buffer, 0).unwrap(), Vec::<u32>::new());
    }

    #[test]
    fn detect_errors_carry_structured_codes() {
        assert_eq!(DetectError::Changed.code(), CODE_CHANGED);
        assert_eq!(DetectError::Io("x".into()).code(), CODE_IO);
    }
}
