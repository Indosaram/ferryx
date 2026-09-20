//! A suspended Git is assigned before any hook can run. The private job does
//! not permit breakaway; closing its last handle terminates remaining members.
use std::{ffi::c_void, io, sync::Arc};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};

#[repr(C)]
#[derive(Default)]
struct BasicLimits {
    process_time: i64,
    job_time: i64,
    flags: u32,
    min_working_set: usize,
    max_working_set: usize,
    active_process_limit: u32,
    affinity: usize,
    priority: u32,
    scheduling: u32,
}
#[repr(C)]
#[derive(Default)]
struct Limits {
    basic: BasicLimits,
    io: [u64; 6],
    process_memory: usize,
    job_memory: usize,
    peak_process_memory: usize,
    peak_job_memory: usize,
}
#[link(name = "kernel32")]
extern "system" {
    fn CreateJobObjectW(attributes: *const c_void, name: *const u16) -> HANDLE;
    fn SetInformationJobObject(job: HANDLE, class: i32, info: *const c_void, length: u32) -> i32;
    fn AssignProcessToJobObject(job: HANDLE, process: HANDLE) -> i32;
    fn TerminateJobObject(job: HANDLE, code: u32) -> i32;
    fn WaitForSingleObject(handle: HANDLE, milliseconds: u32) -> u32;
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> HANDLE;
    fn CreateIoCompletionPort(file: HANDLE, existing: HANDLE, key: usize, threads: u32) -> HANDLE;
    fn GetQueuedCompletionStatus(
        port: HANDLE,
        bytes: *mut u32,
        key: *mut usize,
        overlapped: *mut *mut c_void,
        milliseconds: u32,
    ) -> i32;
}
#[link(name = "ntdll")]
extern "system" {
    fn NtResumeProcess(process: HANDLE) -> i32;
    fn RtlNtStatusToDosError(status: i32) -> u32;
}
struct Handle(HANDLE);
// Kernel handles may be waited/closed on another thread. Arc pins their lifetime.
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
pub(super) struct Owner {
    job: Handle,
    port: Handle,
    process: Option<Arc<Handle>>,
}
pub(super) struct Exit(Arc<Handle>);
impl Owner {
    pub fn prepare(command: &mut tokio::process::Command) -> io::Result<Self> {
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if job.is_null() {
            return Err(io::Error::last_os_error());
        }
        let job = Handle(job);
        let port = unsafe { CreateIoCompletionPort(-1isize as HANDLE, std::ptr::null_mut(), 0, 1) };
        if port.is_null() {
            return Err(io::Error::last_os_error());
        }
        let port = Handle(port);
        #[repr(C)]
        struct Association {
            key: usize,
            port: HANDLE,
        }
        let association = Association {
            key: 1,
            port: port.0,
        };
        if unsafe {
            SetInformationJobObject(
                job.0,
                7,
                &association as *const _ as _,
                std::mem::size_of_val(&association) as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut limits = Limits::default();
        limits.basic.flags = 0x2000; // JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        if unsafe {
            SetInformationJobObject(
                job.0,
                9,
                &limits as *const _ as _,
                std::mem::size_of::<Limits>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        command.creation_flags(0x08000000 | 0x00000004); // NO_WINDOW | SUSPENDED
        Ok(Self {
            job,
            port,
            process: None,
        })
    }
    pub fn attach(mut self, child: &tokio::process::Child) -> io::Result<Self> {
        let raw = child.raw_handle().expect("spawned Git handle") as HANDLE;
        if unsafe { AssignProcessToJobObject(self.job.0, raw) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let process = unsafe { OpenProcess(0x00100000, 0, child.id().expect("spawned Git PID")) }; // SYNCHRONIZE
        if process.is_null() {
            return Err(io::Error::last_os_error());
        }
        self.process = Some(Arc::new(Handle(process)));
        let status = unsafe { NtResumeProcess(raw) };
        if status < 0 {
            return Err(io::Error::from_raw_os_error(
                unsafe { RtlNtStatusToDosError(status) } as i32,
            ));
        }
        Ok(self)
    }
    pub fn exit_observer(&self) -> Exit {
        Exit(self.process.as_ref().unwrap().clone())
    }
    pub fn terminate(&self) -> io::Result<()> {
        if unsafe { TerminateJobObject(self.job.0, 1) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let remaining = deadline
                .checked_duration_since(std::time::Instant::now())
                .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "Git job drain deadline"))?;
            let (mut message, mut key, mut process) = (0, 0, std::ptr::null_mut());
            if unsafe {
                GetQueuedCompletionStatus(
                    self.port.0,
                    &mut message,
                    &mut key,
                    &mut process,
                    remaining.as_millis().max(1) as u32,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            if key == 1 && message == 4 {
                return Ok(());
            } // JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO
        }
    }
}
impl Exit {
    pub fn wait(self) -> io::Result<()> {
        match unsafe { WaitForSingleObject(self.0 .0, 40_000) } {
            0 => Ok(()),
            258 => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Git exit observer deadline",
            )),
            _ => Err(io::Error::last_os_error()),
        }
    }
}
