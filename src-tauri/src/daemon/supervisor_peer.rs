//! Refuses owner-mutating supervisor requests from processes running inside Ferryx worker
//! panes.
//!
//! The owner socket admits any same-uid peer, including a worker agent in one of this daemon's
//! panes, and the CLI's `FERRYX_SESSION_ID` guard is advisory (`env -u`, or writing to the
//! socket directly). So the daemon reads the peer's pid from the kernel at accept time and
//! walks its parent chain. A peer that is a pane child, or descends from one, is refused
//! with `FORBIDDEN` for every request that mutates supervisor state. Reads stay allowed.
//!
//! POSIX: Linux reads `SO_PEERCRED` and `/proc/<pid>/stat`; macOS reads `LOCAL_PEERPID` and
//! `proc_pidinfo(PROC_PIDTBSDINFO)`. A peer whose pid or own parent cannot be read (another
//! unix, or a pid that vanished) is refused for supervisor mutations rather than trusted. An
//! ancestor further up that cannot be read (a root-owned sshd or login process) ends the walk:
//! a pane and every process it starts run as the daemon's user and stay readable, so the
//! chain from a pane descendant reaches the pane before any unreadable process.
//!
//! Windows fallback: the loopback TCP transport has no peer pid. The daemon refuses supervisor
//! mutations there unless the connection presented the transport token, and panes spawned by
//! this daemon do not receive that token in their environment. That is weaker than ancestry:
//! a pane process that reads the token file itself is not told apart.
use std::collections::HashSet;

/// How deep a parent chain is followed before the walk gives up (and refuses).
const MAX_ANCESTRY: usize = 128;

/// The wire code a refused peer gets.
pub const FORBIDDEN: &str = "FORBIDDEN";

/// What the daemon knows about one connection's peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerOrigin {
    /// A process with this pid connected.
    Pid(u32),
    /// The transport carries no peer pid (in-process pairs in tests, or a platform without
    /// peer credentials). Supervisor mutations are refused.
    Unknown,
    /// The transport authenticated the peer another way and no pane can reach it (Windows
    /// token transport, documented above).
    TokenAuthenticated,
}

/// The pid of the peer on a connected unix socket.
#[cfg(unix)]
pub fn peer_pid(fd: std::os::unix::io::RawFd) -> Option<u32> {
    #[cfg(target_os = "linux")]
    {
        let mut cred = libc::ucred { pid: 0, uid: 0, gid: 0 };
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        // SAFETY: cred and len are valid for the duration of the call.
        let ok = unsafe {
            libc::getsockopt(fd, libc::SOL_SOCKET, libc::SO_PEERCRED, (&mut cred as *mut libc::ucred).cast(), &mut len)
        } == 0;
        return (ok && cred.pid > 0).then_some(cred.pid as u32);
    }
    #[cfg(target_os = "macos")]
    {
        let mut pid: libc::pid_t = 0;
        let mut len = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
        // SAFETY: pid and len are valid for the duration of the call.
        let ok = unsafe {
            libc::getsockopt(fd, libc::SOL_LOCAL, libc::LOCAL_PEERPID, (&mut pid as *mut libc::pid_t).cast(), &mut len)
        } == 0;
        return (ok && pid > 0).then_some(pid as u32);
    }
    #[allow(unreachable_code)]
    {
        let _ = fd;
        None
    }
}

/// The parent pid of `pid`, or `None` when it cannot be read.
pub fn parent_pid(pid: u32) -> Option<u32> {
    #[cfg(target_os = "linux")]
    {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let (_, rest) = stat.rsplit_once(')')?;
        return rest.split_whitespace().nth(1)?.parse().ok();
    }
    #[cfg(target_os = "macos")]
    {
        // SAFETY: a zeroed proc_bsdinfo is a valid out-buffer of exactly `size` bytes.
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of_val(&info) as i32;
        let read = unsafe {
            libc::proc_pidinfo(pid as i32, libc::PROC_PIDTBSDINFO, 0, (&mut info as *mut libc::proc_bsdinfo).cast(), size)
        };
        return (read == size).then_some(info.pbi_ppid);
    }
    #[allow(unreachable_code)]
    {
        let _ = pid;
        None
    }
}

/// Why a peer may not mutate supervisor state, or `None` when it may. `pane_pids` are the
/// child pids of this daemon's panes; `parent` reads one parent pid.
pub fn refusal(
    origin: PeerOrigin,
    pane_pids: &HashSet<u32>,
    parent: impl Fn(u32) -> Option<u32>,
) -> Option<String> {
    let pid = match origin {
        PeerOrigin::TokenAuthenticated => return None,
        PeerOrigin::Unknown => {
            return Some("the daemon cannot identify this connection's process, so it may not change the supervisor".into())
        }
        PeerOrigin::Pid(pid) => pid,
    };
    let mut current = pid;
    for _ in 0..MAX_ANCESTRY {
        if pane_pids.contains(&current) {
            return Some(format!(
                "process {pid} runs inside a Ferryx worker pane (pane process {current}); a worker may not change the supervisor"
            ));
        }
        // pid 1 (init/launchd) ends the chain: not under a pane.
        if current <= 1 {
            return None;
        }
        match parent(current) {
            Some(next) if next <= 1 || next == current => return None,
            Some(next) => current = next,
            None if current != pid => return None,
            None => {
                return Some(format!(
                    "the parent of process {pid} could not be read; it may not change the supervisor"
                ))
            }
        }
    }
    Some(format!("the parent chain of process {pid} is deeper than {MAX_ANCESTRY}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn tree(edges: &[(u32, u32)]) -> impl Fn(u32) -> Option<u32> {
        let map: HashMap<u32, u32> = edges.iter().copied().collect();
        move |pid| map.get(&pid).copied()
    }

    #[test]
    fn a_pane_descendant_is_refused_and_an_outside_process_is_not() {
        // launchd(1) -> terminal(50) -> shell(60) -> cli(70); pane child 200 -> agent 210 -> cli 220.
        let parents = tree(&[(50, 1), (60, 50), (70, 60), (200, 100), (100, 1), (210, 200), (220, 210)]);
        let panes: HashSet<u32> = [200].into();
        assert!(refusal(PeerOrigin::Pid(220), &panes, &parents).is_some_and(|why| why.contains("pane process 200")));
        assert!(refusal(PeerOrigin::Pid(200), &panes, &parents).is_some());
        assert_eq!(refusal(PeerOrigin::Pid(70), &panes, &parents), None);
    }

    #[test]
    fn unknown_or_unreadable_peers_are_refused_and_token_peers_are_not() {
        let panes: HashSet<u32> = [200].into();
        assert!(refusal(PeerOrigin::Unknown, &panes, tree(&[])).is_some());
        // The peer's own parent cannot be read: refused rather than trusted.
        assert!(refusal(PeerOrigin::Pid(70), &panes, tree(&[])).is_some());
        // An unreadable ancestor above the peer (a root sshd) ends the walk outside the panes.
        assert_eq!(refusal(PeerOrigin::Pid(70), &panes, tree(&[(70, 60), (60, 50)])), None);
        // A pane below that ancestor is still found.
        assert!(refusal(PeerOrigin::Pid(220), &panes, tree(&[(220, 210), (210, 200), (200, 100)])).is_some());
        assert_eq!(refusal(PeerOrigin::TokenAuthenticated, &panes, tree(&[])), None);
    }

    #[cfg(unix)]
    #[test]
    fn the_real_process_tree_is_walked() {
        let me = std::process::id();
        let parent = parent_pid(me).expect("own parent pid");
        // This test process is not under a pane...
        assert_eq!(refusal(PeerOrigin::Pid(me), &HashSet::new(), parent_pid), None);
        // ...but it is when its parent is a pane child.
        let panes: HashSet<u32> = [parent].into();
        assert!(refusal(PeerOrigin::Pid(me), &panes, parent_pid).is_some());
    }

    #[cfg(unix)]
    #[test]
    fn the_peer_pid_of_a_socketpair_is_this_process() {
        use std::os::unix::io::AsRawFd;
        let (left, _right) = std::os::unix::net::UnixStream::pair().expect("pair");
        assert_eq!(peer_pid(left.as_raw_fd()), Some(std::process::id()));
    }
}
