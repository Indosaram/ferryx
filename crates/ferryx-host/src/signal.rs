use fxsh::types::Signal;
use portable_pty::ChildKiller;

#[cfg(unix)]
pub fn deliver(pid: u32, _killer: &mut (dyn ChildKiller + Send + Sync), signal: Signal) -> bool {
    let sig = match signal {
        Signal::Hangup => libc::SIGHUP,
        Signal::Interrupt => libc::SIGINT,
        Signal::Terminate => libc::SIGTERM,
        Signal::Kill => libc::SIGKILL,
    };
    pid != 0 && unsafe { libc::killpg(pid as i32, sig) } == 0
}

#[cfg(windows)]
pub fn deliver(_pid: u32, killer: &mut (dyn ChildKiller + Send + Sync), signal: Signal) -> bool {
    match signal {
        Signal::Hangup | Signal::Terminate | Signal::Kill => killer.kill().is_ok(),
        Signal::Interrupt => false,
    }
}
