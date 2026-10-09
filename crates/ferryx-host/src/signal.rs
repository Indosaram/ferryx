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
pub fn deliver(pid: u32, _killer: &mut (dyn ChildKiller + Send + Sync), signal: Signal) -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
    match signal {
        Signal::Hangup | Signal::Terminate | Signal::Kill if pid != 0 => {
            let process = unsafe { OpenProcess(PROCESS_TERMINATE, 0, pid) };
            if process.is_null() {
                return false;
            }
            let ok = unsafe { TerminateProcess(process, 1) } != 0;
            unsafe { CloseHandle(process) };
            ok
        }
        _ => false,
    }
}
