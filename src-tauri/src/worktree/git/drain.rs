//! Exit observations, not waitpid/reaping of nonchildren. All membership is
//! checked against the unreaped leader's PGID; signals remain group-only.
use std::{
    io,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    time::Instant,
};

#[cfg(target_os = "macos")]
pub(super) fn members(group: i32) -> io::Result<Vec<i32>> {
    let mut pids = vec![0i32; 65536];
    let bytes = unsafe {
        libc::proc_listpids(
            2,
            group as u32,
            pids.as_mut_ptr().cast(),
            (pids.len() * 4) as i32,
        )
    };
    if bytes < 0 {
        return Err(io::Error::last_os_error());
    }
    if bytes as usize >= pids.len() * 4 {
        return Err(io::Error::other("Git group inventory exceeded limit"));
    }
    let mut result = Vec::new();
    for pid in pids.into_iter().take(bytes as usize / 4).filter(|p| *p > 0) {
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of_val(&info) as i32;
        if unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTBSDINFO,
                0,
                (&mut info as *mut libc::proc_bsdinfo).cast(),
                size,
            )
        } != size
        {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ESRCH) {
                continue;
            }
            return Err(error);
        }
        if info.pbi_pgid == group as u32 && info.pbi_status != libc::SZOMB {
            result.push(pid);
        }
    }
    Ok(result)
}

#[cfg(target_os = "linux")]
pub(super) fn members(group: i32) -> io::Result<Vec<i32>> {
    let mut result = Vec::new();
    // Linux exposes no group-specific inventory syscall. Inspect stat only;
    // never traverse ancestry or signal any enumerated PID.
    for entry in std::fs::read_dir("/proc")? {
        let entry = entry?;
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<i32>().ok())
        else {
            continue;
        };
        let stat = match std::fs::read_to_string(entry.path().join("stat")) {
            Ok(stat) => stat,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        let fields: Vec<_> = stat
            .rsplit_once(") ")
            .ok_or_else(|| io::Error::other("invalid proc stat"))?
            .1
            .split_whitespace()
            .collect();
        if fields.get(2).and_then(|s| s.parse::<i32>().ok()) == Some(group)
            && !matches!(fields[0], "Z" | "X")
        {
            result.push(pid);
        }
    }
    Ok(result)
}

#[cfg(target_os = "macos")]
pub(super) fn subscribe(pid: i32) -> io::Result<Option<OwnedFd>> {
    let raw = unsafe { libc::kqueue() };
    if raw < 0 {
        return Err(io::Error::last_os_error());
    }
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    let event = libc::kevent {
        ident: pid as usize,
        filter: libc::EVFILT_PROC,
        flags: libc::EV_ADD | libc::EV_ONESHOT,
        fflags: libc::NOTE_EXIT,
        data: 0,
        udata: std::ptr::null_mut(),
    };
    if unsafe { libc::kevent(raw, &event, 1, std::ptr::null_mut(), 0, std::ptr::null()) } < 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            return Ok(None);
        }
        return Err(error);
    }
    Ok(Some(fd))
}

#[cfg(target_os = "linux")]
pub(super) fn subscribe(pid: i32) -> io::Result<Option<OwnedFd>> {
    let raw = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) } as i32;
    if raw < 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            return Ok(None);
        }
        return Err(error);
    }
    Ok(Some(unsafe { OwnedFd::from_raw_fd(raw) }))
}

pub(super) fn wait(fd: OwnedFd, deadline: Instant) -> io::Result<()> {
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::TimedOut, "Git descendant exit deadline")
            })?;
        #[cfg(target_os = "macos")]
        let count = {
            let mut event: libc::kevent = unsafe { std::mem::zeroed() };
            let timeout = libc::timespec {
                tv_sec: remaining.as_secs() as _,
                tv_nsec: remaining.subsec_nanos() as _,
            };
            unsafe { libc::kevent(fd.as_raw_fd(), std::ptr::null(), 0, &mut event, 1, &timeout) }
        };
        #[cfg(target_os = "linux")]
        let count = {
            let mut event = libc::pollfd {
                fd: fd.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            unsafe {
                libc::poll(
                    &mut event,
                    1,
                    remaining.as_millis().max(1).min(i32::MAX as u128) as i32,
                )
            }
        };
        if count > 0 {
            return Ok(());
        }
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Git descendant exit deadline",
            ));
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}
