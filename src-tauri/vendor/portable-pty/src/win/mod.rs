use crate::{Child, ChildKiller, ExitStatus};
use anyhow::Context as _;
use std::io::{Error as IoError, Result as IoResult};
use std::os::windows::io::{AsRawHandle, RawHandle};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use winapi::shared::minwindef::{DWORD, LPVOID};
use winapi::um::jobapi2::{CreateJobObjectW, SetInformationJobObject, TerminateJobObject};
use winapi::um::minwinbase::STILL_ACTIVE;
use winapi::um::processthreadsapi::*;
use winapi::um::synchapi::WaitForSingleObject;
use winapi::um::winbase::INFINITE;
use winapi::um::winnt::{
    JobObjectExtendedLimitInformation, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};

pub mod conpty;
mod procthreadattr;
mod psuedocon;

use filedescriptor::OwnedHandle;

/// Creates the private job a ConPTY child is placed in before it runs.
///
/// Unnamed (nothing can open it by name), non-inheritable (`CreateJobObjectW` with null security
/// attributes), and limited to `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` with no breakaway flag, so the
/// shell and everything it starts - including `Start-Process -NoNewWindow` grandchildren that are
/// not on the console - stay members and die with the job. The handles to the job live only in the
/// [`WinChild`] and its killers: terminating the job, or dropping the last of them, ends the tree.
pub(crate) fn kill_on_close_job() -> anyhow::Result<OwnedHandle> {
    use std::os::windows::io::FromRawHandle;

    let raw = unsafe { CreateJobObjectW(std::ptr::null_mut(), std::ptr::null()) };
    anyhow::ensure!(
        !raw.is_null(),
        "CreateJobObjectW failed: {}",
        IoError::last_os_error()
    );
    // Owned at once, so every early return below closes the job exactly once.
    let job = unsafe { OwnedHandle::from_raw_handle(raw as _) };
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let configured = unsafe {
        SetInformationJobObject(
            job.as_raw_handle() as _,
            JobObjectExtendedLimitInformation,
            &mut limits as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION as LPVOID,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as DWORD,
        )
    };
    anyhow::ensure!(
        configured != 0,
        "SetInformationJobObject(KILL_ON_JOB_CLOSE) failed: {}",
        IoError::last_os_error()
    );
    Ok(job)
}

/// Ends the child's whole process tree: the job first, which reaches every descendant, and the
/// child's own handle only when the job cannot be terminated.
///
/// Both Win32 calls return nonzero on success. The upstream check was inverted - it reported a
/// successful `TerminateProcess` as an error and a failed one as success - which is why callers
/// had learned to discard the result.
fn terminate_tree(job: &OwnedHandle, proc: &OwnedHandle) -> IoResult<()> {
    if unsafe { TerminateJobObject(job.as_raw_handle() as _, 1) } != 0 {
        return Ok(());
    }
    let job_error = IoError::last_os_error();
    if unsafe { TerminateProcess(proc.as_raw_handle() as _, 1) } != 0 {
        log::warn!("TerminateJobObject failed ({job_error}); terminated the shell process only");
        return Ok(());
    }
    Err(IoError::last_os_error())
}

#[derive(Debug)]
pub struct WinChild {
    proc: Mutex<OwnedHandle>,
    /// The kill-on-close job the child was created in. Shared with every killer, so the tree is
    /// terminated when the job is, and at the latest when the last holder is dropped.
    job: Arc<OwnedHandle>,
}

impl WinChild {
    fn is_complete(&mut self) -> IoResult<Option<ExitStatus>> {
        let mut status: DWORD = 0;
        let proc = self.proc.lock().unwrap().try_clone().unwrap();
        let res = unsafe { GetExitCodeProcess(proc.as_raw_handle() as _, &mut status) };
        if res != 0 {
            if status == STILL_ACTIVE {
                Ok(None)
            } else {
                Ok(Some(ExitStatus::with_exit_code(status)))
            }
        } else {
            Ok(None)
        }
    }

    fn do_kill(&mut self) -> IoResult<()> {
        let proc = self.proc.lock().unwrap().try_clone().unwrap();
        terminate_tree(&self.job, &proc)
    }
}

impl ChildKiller for WinChild {
    fn kill(&mut self) -> IoResult<()> {
        self.do_kill()
    }

    fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
        let proc = self.proc.lock().unwrap().try_clone().unwrap();
        Box::new(WinChildKiller {
            proc,
            job: Arc::clone(&self.job),
        })
    }
}

#[derive(Debug)]
pub struct WinChildKiller {
    proc: OwnedHandle,
    job: Arc<OwnedHandle>,
}

impl ChildKiller for WinChildKiller {
    fn kill(&mut self) -> IoResult<()> {
        terminate_tree(&self.job, &self.proc)
    }

    fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
        let proc = self.proc.try_clone().unwrap();
        Box::new(WinChildKiller {
            proc,
            job: Arc::clone(&self.job),
        })
    }
}

impl Child for WinChild {
    fn try_wait(&mut self) -> IoResult<Option<ExitStatus>> {
        self.is_complete()
    }

    fn wait(&mut self) -> IoResult<ExitStatus> {
        if let Ok(Some(status)) = self.try_wait() {
            return Ok(status);
        }
        let proc = self.proc.lock().unwrap().try_clone().unwrap();
        unsafe {
            WaitForSingleObject(proc.as_raw_handle() as _, INFINITE);
        }
        let mut status: DWORD = 0;
        let res = unsafe { GetExitCodeProcess(proc.as_raw_handle() as _, &mut status) };
        if res != 0 {
            Ok(ExitStatus::with_exit_code(status))
        } else {
            Err(IoError::last_os_error())
        }
    }

    fn process_id(&self) -> Option<u32> {
        let res = unsafe { GetProcessId(self.proc.lock().unwrap().as_raw_handle() as _) };
        if res == 0 {
            None
        } else {
            Some(res)
        }
    }

    fn as_raw_handle(&self) -> Option<std::os::windows::io::RawHandle> {
        let proc = self.proc.lock().unwrap();
        Some(proc.as_raw_handle())
    }
}

impl std::future::Future for WinChild {
    type Output = anyhow::Result<ExitStatus>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<anyhow::Result<ExitStatus>> {
        match self.is_complete() {
            Ok(Some(status)) => Poll::Ready(Ok(status)),
            Err(err) => Poll::Ready(Err(err).context("Failed to retrieve process exit status")),
            Ok(None) => {
                struct PassRawHandleToWaiterThread(pub RawHandle);
                unsafe impl Send for PassRawHandleToWaiterThread {}

                let proc = self.proc.lock().unwrap().try_clone()?;
                let handle = PassRawHandleToWaiterThread(proc.as_raw_handle());

                let waker = cx.waker().clone();
                std::thread::spawn(move || {
                    unsafe {
                        WaitForSingleObject(handle.0 as _, INFINITE);
                    }
                    waker.wake();
                });
                Poll::Pending
            }
        }
    }
}
