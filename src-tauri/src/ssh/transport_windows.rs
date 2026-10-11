//! Windows abrupt-owner-death cleanup for the local SSH transport.
//!
//! `ssh::bridge` ends a locally spawned `ssh` child through its owner guard plus a Unix `atexit`
//! hook. Neither covers Windows: there is no portable process-exit hook, and `Drop` is bypassed by
//! `std::process::exit`, by an abort, and by the OS tearing the process down.
//!
//! This module owns the Windows transport spawn for exactly that reason. The child is created with
//! `PROC_THREAD_ATTRIBUTE_JOB_LIST` (a `STARTUPINFOEX` attribute), so it is a member of the private
//! kill-on-close job **at creation**: there is no window in which a child exists outside the job,
//! which is what a `CreateProcess`-then-`AssignProcessToJobObject` sequence cannot promise - an
//! owner that dies inside that window would strand a child that nothing owns. The child is created
//! suspended, membership is verified through the kernel, and only then is it resumed, so a member
//! can never run unsupervised. `PROC_THREAD_ATTRIBUTE_JOB_LIST` needs Windows 10 / Server 2016 or
//! newer, the same floor this application already has.
//!
//! Ownership shape (the reason this module performs the spawn):
//! * [`TransportOwner::prepare`] creates the private, non-inheritable job limited to
//!   `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`;
//! * [`TransportOwner::spawn`] builds the creation attributes for that job and for the exact stdio
//!   handles the child may inherit, creates the child inside the job, verifies membership with
//!   `IsProcessInJob`, resumes its primary thread, and hands back a [`TransportChild`] whose stdio
//!   is already converted to the `tokio::process` stream types the bridge expects;
//! * the job handle is the only handle to the job, so the kernel terminates every member as soon as
//!   it closes: normal drop, `std::process::exit`, panic, or hard process death alike.
//!
//! No process is ever identified by pid for signalling: the child is only ever terminated through
//! the process handle returned by `CreateProcessW` or by closing the job. The module touches no
//! remote PTY, helper, or reconnect state, and the non-Windows transport path is untouched.
//!
//! Tests (Windows only): `cargo test --manifest-path src-tauri/Cargo.toml --lib transport_windows`.
//! `creation_places_the_child_in_the_job_before_it_runs` is the regression test for the creation
//! window: it asserts membership and the still-suspended primary thread before any resume, the
//! owner-death case included. See the protocol notes on `tests`.

#![cfg(windows)]

use std::collections::BTreeMap;
use std::ffi::{c_void, OsStr, OsString};
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle, IntoRawHandle, OwnedHandle, RawHandle};
use std::os::windows::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::{ChildStderr, ChildStdin, ChildStdout, ExitStatus};
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, ReadBuf};

use windows_sys::Win32::Foundation::HANDLE;

/// `JobObjectExtendedLimitInformation`: the info class used with [`ExtendedLimitInformation`].
const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS: i32 = 9;
/// `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`: all member processes are terminated when the last handle
/// to the job is closed.
const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;
/// `PROC_THREAD_ATTRIBUTE_JOB_LIST`: jobs the child is assigned to at creation, in order.
const PROC_THREAD_ATTRIBUTE_JOB_LIST: usize = 0x0002_000D;
/// `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`: the only handles the child may inherit, all of which must
/// be inheritable and none of which may be a pseudo handle. Documented to require
/// `bInheritHandles` to stay true.
const PROC_THREAD_ATTRIBUTE_HANDLE_LIST: usize = 0x0002_0002;
/// The suffix appended while searching for a bare program name, like `std` does.
const EXE_SUFFIX: &str = ".exe";
/// `HANDLE_FLAG_INHERIT`: the `SetHandleInformation` mask for handle inheritance.
const HANDLE_FLAG_INHERIT: u32 = 0x0000_0001;
/// `CREATE_SUSPENDED`: the primary thread never runs before creation is complete and verified.
const CREATE_SUSPENDED: u32 = 0x0000_0004;
/// `CREATE_UNICODE_ENVIRONMENT`: the explicit environment block is UTF-16.
const CREATE_UNICODE_ENVIRONMENT: u32 = 0x0000_0400;
/// `CREATE_NO_WINDOW`, matching `crate::util::CREATE_NO_WINDOW`.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// `EXTENDED_STARTUPINFO_PRESENT`: `STARTUPINFOW` is a `STARTUPINFOEXW` carrying the attributes.
const EXTENDED_STARTUPINFO_PRESENT: u32 = 0x0008_0000;
/// `STARTF_USESTDHANDLES`: the three stdio handles in `STARTUPINFOW` are the child's stdio.
const STARTF_USESTDHANDLES: u32 = 0x0000_0100;
/// `DUPLICATE_SAME_ACCESS`: duplicate with the source handle's access rights.
const DUPLICATE_SAME_ACCESS: u32 = 0x0000_0002;
/// `STILL_ACTIVE`: a process that has not exited reports this.
const STILL_ACTIVE: u32 = 259;
/// `WAIT_OBJECT_0`: the wait finished because the object was signalled.
const WAIT_OBJECT_0: u32 = 0;
/// `INFINITE`: wait without a deadline.
const INFINITE: u32 = 0xFFFF_FFFF;
/// `STD_INPUT_HANDLE` / `STD_OUTPUT_HANDLE` / `STD_ERROR_HANDLE` (`GetStdHandle` kinds).
const STD_INPUT_HANDLE: u32 = -10i32 as u32;
const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
const STD_ERROR_HANDLE: u32 = -12i32 as u32;
/// The bit bucket opened for [`ChildStdio::Null`].
const NUL_DEVICE: &str = r"\\.\NUL";
const GENERIC_WRITE: u32 = 0x4000_0000;
const OPEN_EXISTING: u32 = 3;
const INVALID_HANDLE_VALUE: isize = -1;

/// `JOBOBJECT_BASIC_LIMIT_INFORMATION` (winnt.h).
#[repr(C)]
#[derive(Default)]
struct BasicLimitInformation {
    per_process_user_time_limit: i64,
    per_job_user_time_limit: i64,
    limit_flags: u32,
    minimum_working_set_size: usize,
    maximum_working_set_size: usize,
    active_process_limit: u32,
    affinity: usize,
    priority_class: u32,
    scheduling_class: u32,
}

/// `JOBOBJECT_EXTENDED_LIMIT_INFORMATION` (winnt.h): basic limits, `IO_COUNTERS`, memory limits.
#[repr(C)]
#[derive(Default)]
struct ExtendedLimitInformation {
    basic_limit_information: BasicLimitInformation,
    io_counters: [u64; 6],
    process_memory_limit: usize,
    job_memory_limit: usize,
    peak_process_memory_used: usize,
    peak_job_memory_used: usize,
}

/// `STARTUPINFOW` (processthreadsapi.h), fields in declaration order.
#[repr(C)]
struct StartupInfoW {
    cb: u32,
    reserved: *mut u16,
    desktop: *mut u16,
    title: *mut u16,
    x: u32,
    y: u32,
    x_size: u32,
    y_size: u32,
    x_count_chars: u32,
    y_count_chars: u32,
    fill_attribute: u32,
    flags: u32,
    show_window: u16,
    reserved2: u16,
    reserved2_ptr: *mut u8,
    std_input: HANDLE,
    std_output: HANDLE,
    std_error: HANDLE,
}

/// `STARTUPINFOEXW` (processthreadsapi.h): the startup info plus the attribute list pointer.
#[repr(C)]
struct StartupInfoExW {
    startup_info: StartupInfoW,
    attribute_list: *mut c_void,
}

/// `PROCESS_INFORMATION` (processthreadsapi.h).
#[repr(C)]
struct ProcessInformation {
    process: HANDLE,
    thread: HANDLE,
    process_id: u32,
    thread_id: u32,
}

// Declared locally for the same reason as `worktree::git::windows`: these process, job, handle,
// attribute, and wait entry points are not part of the `windows-sys` feature set this crate
// enables. kernel32 exports all of them.
#[link(name = "kernel32")]
extern "system" {
    fn CreateJobObjectW(attributes: *const c_void, name: *const u16) -> HANDLE;
    fn SetInformationJobObject(job: HANDLE, class: i32, info: *const c_void, length: u32) -> i32;
    fn SetHandleInformation(handle: HANDLE, mask: u32, flags: u32) -> i32;
    fn TerminateProcess(process: HANDLE, exit_code: u32) -> i32;
    fn InitializeProcThreadAttributeList(
        list: *mut c_void,
        count: u32,
        flags: u32,
        size: *mut usize,
    ) -> i32;
    fn UpdateProcThreadAttribute(
        list: *mut c_void,
        flags: u32,
        attribute: usize,
        value: *const c_void,
        size: usize,
        previous: *mut c_void,
        return_size: *mut usize,
    ) -> i32;
    fn DeleteProcThreadAttributeList(list: *mut c_void);
    fn CreatePipe(
        read: *mut HANDLE,
        write: *mut HANDLE,
        attributes: *const c_void,
        size: u32,
    ) -> i32;
    #[allow(clippy::too_many_arguments)]
    fn CreateFileW(
        file_name: *const u16,
        desired_access: u32,
        share_mode: u32,
        security_attributes: *const c_void,
        creation_disposition: u32,
        flags_and_attributes: u32,
        template_file: HANDLE,
    ) -> HANDLE;
    #[allow(clippy::too_many_arguments)]
    fn CreateProcessW(
        application: *const u16,
        command_line: *mut u16,
        process_attributes: *const c_void,
        thread_attributes: *const c_void,
        inherit_handles: i32,
        creation_flags: u32,
        environment: *const c_void,
        current_directory: *const u16,
        startup_info: *const StartupInfoW,
        process_information: *mut ProcessInformation,
    ) -> i32;
    fn ResumeThread(thread: HANDLE) -> u32;
    fn WaitForSingleObject(handle: HANDLE, milliseconds: u32) -> u32;
    fn GetExitCodeProcess(process: HANDLE, code: *mut u32) -> i32;
    fn GetProcessId(process: HANDLE) -> u32;
    fn IsProcessInJob(process: HANDLE, job: HANDLE, result: *mut i32) -> i32;
    fn GetStdHandle(kind: u32) -> HANDLE;
    fn GetSystemDirectoryW(buffer: *mut u16, size: u32) -> u32;
    fn GetWindowsDirectoryW(buffer: *mut u16, size: u32) -> u32;
    fn GetCurrentProcess() -> HANDLE;
    fn DuplicateHandle(
        source_process: HANDLE,
        source: HANDLE,
        target_process: HANDLE,
        target: *mut HANDLE,
        access: u32,
        inherit: i32,
        options: u32,
    ) -> i32;
}

/// Stdio the caller wants in the child, mirroring `std::process::Stdio` for the cases the transport
/// needs. `Handle` transfers ownership exactly like `Stdio::from(OwnedHandle)`: the handle is
/// closed once the child has its own inherited copy.
// Only the piped form is constructed by the transport path today; the other forms are part of the
// stdio contract and are exercised by the tests in this module.
#[cfg_attr(not(test), allow(dead_code))]
pub(super) enum ChildStdio {
    /// A pipe; the transport keeps the parent-side end.
    Piped,
    /// The NUL device.
    Null,
    /// A specific handle to hand to the child.
    Handle(OwnedHandle),
}

/// Everything [`TransportOwner::spawn`] needs to create the child. Program, arguments, and
/// environment are passed through unchanged, so the SSH plan, its askpass/token environment, and
/// its stdio shape stay the caller's decision.
pub(super) struct ChildSpec {
    pub program: OsString,
    pub args: Vec<OsString>,
    /// Environment additions on top of this process's environment, like `Command::envs`.
    pub envs: Vec<(OsString, OsString)>,
    pub stdin: ChildStdio,
    pub stdout: ChildStdio,
    pub stderr: ChildStdio,
}

impl ChildSpec {
    /// A spec with no arguments, no environment changes, and all three stdio slots piped.
    pub(super) fn new(program: impl Into<OsString>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            envs: Vec::new(),
            stdin: ChildStdio::Piped,
            stdout: ChildStdio::Piped,
            stderr: ChildStdio::Piped,
        }
    }
}

/// Owns the private job object and the attribute list that names it in `CreateProcessW`.
///
/// The job is unnamed (nothing can open it by name), non-inheritable (a member holding an inherited
/// copy could otherwise keep it alive past owner death), and limited to
/// `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` with no breakaway flag, so members cannot leave it.
///
/// The job handle is a std [`OwnedHandle`]: it closes with `CloseHandle` exactly once on drop and
/// is `Send + Sync`, so the owner can live inside the transport's async state with no hand-rolled
/// drop or `unsafe impl`. Retain it for as long as the transport owns the child.
pub(super) struct TransportOwner {
    job: OwnedHandle,
}

impl TransportOwner {
    /// Creates the private kill-on-close job that [`TransportOwner::spawn`] names in the child's
    /// creation attributes.
    ///
    /// On failure every partially created kernel object is released before returning.
    pub(super) fn prepare() -> io::Result<Self> {
        // SAFETY: null security attributes and a null name are the documented private-job form.
        // The returned handle is checked for null and then owned by `OwnedHandle`, which closes it
        // exactly once on every path.
        let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if raw.is_null() {
            return Err(io::Error::last_os_error());
        }
        let job = unsafe { OwnedHandle::from_raw_handle(raw as RawHandle) };
        let handle = job.as_raw_handle() as HANDLE;
        // `CreateJobObjectW` already returns a non-inheritable handle; clear the flag explicitly so
        // termination-on-close cannot be defeated by a member holding an inherited copy.
        // SAFETY: `handle` is a live job handle owned by `job` for the rest of this call.
        if unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut limits = ExtendedLimitInformation::default();
        limits.basic_limit_information.limit_flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: `limits` is a correctly sized JOBOBJECT_EXTENDED_LIMIT_INFORMATION for class 9.
        let configured = unsafe {
            SetInformationJobObject(
                handle,
                JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS,
                &limits as *const ExtendedLimitInformation as *const c_void,
                std::mem::size_of::<ExtendedLimitInformation>() as u32,
            )
        };
        if configured == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { job })
    }

    /// Creates the child suspended and already inside the job, then verifies membership and resumes
    /// it.
    ///
    /// Fails closed: any failure after `CreateProcessW` terminates the child through its own
    /// handle, so a caller can never end up holding a live `ssh` child this process does not
    /// supervise.
    pub(super) fn spawn(&self, spec: ChildSpec) -> io::Result<TransportChild> {
        let mut child = self.create_suspended(spec)?;
        if let Err(error) = child.resume() {
            let _ = child.start_kill();
            return Err(error);
        }
        Ok(child)
    }

    /// Creates the child suspended inside the job and verifies that membership holds.
    ///
    /// Kept separate from [`TransportOwner::spawn`] so tests can observe the pre-resume state, which
    /// is the instant the owner-death window used to exist in.
    fn create_suspended(&self, spec: ChildSpec) -> io::Result<TransportChild> {
        // `CreateProcessW` never searches `PATH` for a non-null application name, so the executable
        // is resolved here exactly as `std::process::Command` resolves it; the command line keeps
        // the caller's spelling as argv[0].
        let resolved = resolve_program(&spec.program)?;
        let mut program: Vec<u16> = resolved.encode_wide().collect();
        program.push(0);
        let mut command_line = build_command_line(&spec.program, &spec.args)?;
        let environment = build_environment(&spec.envs)?;

        // The child's stdio handles are duplicated as inheritable; our copies are closed as soon as
        // the child has inherited its own, so each pipe side has exactly one holder.
        let mut inherited: Vec<OwnedHandle> = Vec::new();
        let (stdin_child, stdin_parent) =
            prepare_stdio(&spec.stdin, STD_INPUT_HANDLE, &mut inherited)?;
        let (stdout_child, stdout_parent) =
            prepare_stdout(&spec.stdout, &mut inherited)?;
        let (stderr_child, stderr_parent) =
            prepare_stdio(&spec.stderr, STD_ERROR_HANDLE, &mut inherited)?;

        // The attributes are built in this frame, so their storage outlives the call below by
        // construction: the job the child joins at creation, and the only handles it may inherit.
        let attributes =
            AttributeList::for_job_and_stdio(&self.job, [stdin_child, stdout_child, stderr_child])?;
        let startup = StartupInfoExW {
            startup_info: StartupInfoW {
                cb: std::mem::size_of::<StartupInfoExW>() as u32,
                reserved: std::ptr::null_mut(),
                desktop: std::ptr::null_mut(),
                title: std::ptr::null_mut(),
                x: 0,
                y: 0,
                x_size: 0,
                y_size: 0,
                x_count_chars: 0,
                y_count_chars: 0,
                fill_attribute: 0,
                flags: STARTF_USESTDHANDLES,
                show_window: 0,
                reserved2: 0,
                reserved2_ptr: std::ptr::null_mut(),
                std_input: stdin_child,
                std_output: stdout_child,
                std_error: stderr_child,
            },
            attribute_list: attributes.pointer(),
        };
        let mut process_information = ProcessInformation {
            process: std::ptr::null_mut(),
            thread: std::ptr::null_mut(),
            process_id: 0,
            thread_id: 0,
        };
        let mut creation_flags = CREATE_SUSPENDED | CREATE_NO_WINDOW | EXTENDED_STARTUPINFO_PRESENT;
        let environment_pointer = match environment.as_ref() {
            Some(block) => {
                creation_flags |= CREATE_UNICODE_ENVIRONMENT;
                block.as_ptr() as *const c_void
            }
            None => std::ptr::null(),
        };

        // SAFETY: every pointer is valid for the duration of the call - the UTF-16 program and
        // command line are NUL terminated, the environment block is double NUL terminated, the
        // startup info carries a live attribute list, and the child-side stdio handles are live,
        // inheritable, and owned by `inherited` until after the call returns.
        let created = unsafe {
            CreateProcessW(
                program.as_ptr(),
                command_line.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1,
                creation_flags,
                environment_pointer,
                std::ptr::null(),
                &startup.startup_info,
                &mut process_information,
            )
        };
        drop(attributes);
        drop(inherited);
        drop(spec);
        if created == 0 {
            return Err(io::Error::last_os_error());
        }

        let process_handle = process_information.process as HANDLE;
        let thread_handle = process_information.thread as HANDLE;
        // The whole point of the attribute list: membership must already hold. If it does not, the
        // child would be unsupervised, so it is terminated rather than returned.
        let mut in_job = 0i32;
        // SAFETY: both handles are live: the process handle is fresh from `CreateProcessW` and the
        // job is owned by the caller for the duration of this call.
        let queried = unsafe {
            IsProcessInJob(
                process_handle,
                self.job.as_raw_handle() as HANDLE,
                &mut in_job,
            )
        };
        if queried == 0 || in_job == 0 {
            // SAFETY: the process handle is live until it is closed just below.
            let _ = unsafe { TerminateProcess(process_handle, 1) };
            // SAFETY: both handles are fresh and owned here, so both are closed on this path.
            drop(unsafe { OwnedHandle::from_raw_handle(process_handle as RawHandle) });
            drop(unsafe { OwnedHandle::from_raw_handle(thread_handle as RawHandle) });
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "child was not created inside the transport job",
            ));
        }
        // SAFETY: `CreateProcessW` succeeded, so both handles are fresh and owned here.
        let (process, thread) = unsafe {
            (
                OwnedHandle::from_raw_handle(process_handle as RawHandle),
                OwnedHandle::from_raw_handle(thread_handle as RawHandle),
            )
        };
        let (stdin, stderr) = match adopt_stdio(stdin_parent, stderr_parent) {
            Ok(stdio) => stdio,
            Err(error) => {
                // SAFETY: the child is alive and its handle is owned by `process` in this scope.
                let _ = unsafe { TerminateProcess(process_handle, 1) };
                return Err(error);
            }
        };
        Ok(TransportChild {
            process,
            thread: Some(thread),
            stdin,
            stdout: stdout_parent,
            stderr,
        })
    }
}

/// Owns one `PROC_THREAD_ATTRIBUTE_LIST` buffer and the value arrays it points into: the job the
/// child joins at creation and the stdio handles it may inherit.
///
/// The list stores pointers to the values it was given, so the arrays must outlive the list;
/// `Drop` deletes the list before the fields (and therefore the arrays) are released. Everything
/// here lives in the same stack frame as the `CreateProcessW` call, so no kernel pointer can
/// outlive its value.
struct AttributeList {
    list: Box<[u8]>,
    job_handles: Box<[usize; 1]>,
    stdio_handles: Box<[usize; 3]>,
}

impl AttributeList {
    /// Builds the two creation attributes that make the spawn atomic and closed: the job the child
    /// is a member of from creation, and the only handles it may inherit.
    fn for_job_and_stdio(job: &OwnedHandle, stdio: [HANDLE; 3]) -> io::Result<Self> {
        let job_handles = Box::new([job.as_raw_handle() as usize]);
        let stdio_handles = Box::new([stdio[0] as usize, stdio[1] as usize, stdio[2] as usize]);
        let count = 2u32;
        // The sizing call is documented to fail by design while reporting the required size.
        let mut size = 0usize;
        // SAFETY: a null list asks only for the buffer size.
        unsafe { InitializeProcThreadAttributeList(std::ptr::null_mut(), count, 0, &mut size) };
        if size == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut list = vec![0u8; size].into_boxed_slice();
        let pointer = list.as_mut_ptr().cast::<c_void>();
        // SAFETY: `pointer` addresses a buffer of exactly the size the previous call reported.
        if unsafe { InitializeProcThreadAttributeList(pointer, count, 0, &mut size) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let attributes = Self {
            list,
            job_handles,
            stdio_handles,
        };
        let job_value: *const [usize; 1] = &*attributes.job_handles;
        attributes.set(
            PROC_THREAD_ATTRIBUTE_JOB_LIST,
            job_value.cast::<c_void>(),
            std::mem::size_of::<[usize; 1]>(),
        )?;
        let stdio_value: *const [usize; 3] = &*attributes.stdio_handles;
        attributes.set(
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
            stdio_value.cast::<c_void>(),
            std::mem::size_of::<[usize; 3]>(),
        )?;
        Ok(attributes)
    }

    fn set(&self, attribute: usize, value: *const c_void, size: usize) -> io::Result<()> {
        // SAFETY: the list is initialized, the value points at an array owned by `self` for the
        // rest of this frame, and the size matches that array.
        let updated = unsafe {
            UpdateProcThreadAttribute(
                self.pointer(),
                0,
                attribute,
                value,
                size,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if updated == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn pointer(&self) -> *mut c_void {
        self.list.as_ptr() as *mut c_void
    }
}

impl Drop for AttributeList {
    fn drop(&mut self) {
        // SAFETY: `list` was initialized by `InitializeProcThreadAttributeList` and is still live
        // here; the fields are released only after this call.
        unsafe { DeleteProcThreadAttributeList(self.pointer()) };
    }
}

/// A child created through [`TransportOwner::spawn`].
///
/// Its stdio is already converted to the `tokio::process` stream types, and its lifetime is driven
/// through the real `CreateProcessW` handle: [`TransportChild::try_wait`],
/// [`TransportChild::wait`], and [`TransportChild::start_kill`] never address a pid.
pub(super) struct TransportChild {
    process: OwnedHandle,
    thread: Option<OwnedHandle>,
    stdin: Option<tokio::process::ChildStdin>,
    stdout: Option<TransportStdout>,
    stderr: Option<tokio::process::ChildStderr>,
}

impl TransportChild {
    pub(super) fn take_stdin(&mut self) -> Option<tokio::process::ChildStdin> {
        self.stdin.take()
    }

    pub(super) fn take_stdout(
        &mut self,
    ) -> Option<TransportStdout> {
        self.stdout.take()
    }

    pub(super) fn take_stderr(&mut self) -> Option<tokio::process::ChildStderr> {
        self.stderr.take()
    }

    /// Identity for diagnostics only: this value is never signalled.
    pub(super) fn id(&self) -> Option<u32> {
        // SAFETY: `self.process` is a live process handle.
        let pid = unsafe { GetProcessId(self.process.as_raw_handle() as HANDLE) };
        (pid != 0).then_some(pid)
    }

    /// Returns the exit status once the child has exited, without blocking.
    pub(super) fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        let mut code = 0u32;
        // SAFETY: `self.process` is a live process handle and `code` is a valid out pointer.
        if unsafe { GetExitCodeProcess(self.process.as_raw_handle() as HANDLE, &mut code) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok((code != STILL_ACTIVE).then(|| ExitStatus::from_raw(code)))
    }

    /// Waits for the child to exit, off the async reactor.
    pub(super) async fn wait(&mut self) -> io::Result<ExitStatus> {
        let process = self.process.try_clone()?;
        tokio::task::spawn_blocking(move || {
            let handle = process.as_raw_handle() as HANDLE;
            // SAFETY: `process` is a live process handle owned by this closure.
            if unsafe { WaitForSingleObject(handle, INFINITE) } != WAIT_OBJECT_0 {
                return Err(io::Error::last_os_error());
            }
            let mut code = 0u32;
            // SAFETY: as above; `code` is a valid out pointer.
            if unsafe { GetExitCodeProcess(handle, &mut code) } == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(ExitStatus::from_raw(code))
        })
        .await
        .map_err(io::Error::other)?
    }

    /// Terminates the child through its process handle.
    pub(super) fn start_kill(&mut self) -> io::Result<()> {
        let _ = self.thread.take();
        // SAFETY: `self.process` is a live process handle.
        if unsafe { TerminateProcess(self.process.as_raw_handle() as HANDLE, 1) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// The process handle, for tests that query kernel state directly.
    #[cfg(test)]
    fn handle(&self) -> HANDLE {
        self.process.as_raw_handle() as HANDLE
    }

    /// Resumes the primary thread created in the suspended state, returning the previous suspend
    /// count, which is one for a child that has not run yet.
    fn resume(&mut self) -> io::Result<u32> {
        let Some(thread) = self.thread.take() else {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "child primary thread is already resumed and released",
            ));
        };
        // SAFETY: `thread` is the live primary-thread handle from `CreateProcessW`.
        let previous = unsafe { ResumeThread(thread.as_raw_handle() as HANDLE) };
        if previous == u32::MAX {
            return Err(io::Error::last_os_error());
        }
        Ok(previous)
    }
}

/// Converts the parent-side pipe ends into the asynchronous stream types the bridge streams with.
///
/// `std::process::ChildStd*::from` adopts the handle, and `tokio::process::ChildStd*::from_std`
/// applies the same conversion tokio applies to the handles `std::process::Command` creates, so
/// the anonymous pipe handles this module creates are exactly what those streams accept.
fn adopt_stdio(
    stdin: Option<OwnedHandle>,
    stderr: Option<OwnedHandle>,
) -> io::Result<(
    Option<tokio::process::ChildStdin>,
    Option<tokio::process::ChildStderr>,
)> {
    Ok((
        stdin
            .map(ChildStdin::from)
            .map(tokio::process::ChildStdin::from_std)
            .transpose()?,
        stderr
            .map(ChildStderr::from)
            .map(tokio::process::ChildStderr::from_std)
            .transpose()?,
    ))
}

/// Asynchronous stdout reader stream for Windows transport child processes.
/// Automatically connects the named pipe server on the first `poll_read` before reading data.
pub struct TransportStdout {
    server: tokio::net::windows::named_pipe::NamedPipeServer,
    connected: bool,
}

impl TransportStdout {
    pub(super) fn new(server: tokio::net::windows::named_pipe::NamedPipeServer) -> Self {
        Self {
            server,
            connected: false,
        }
    }
}

impl AsyncRead for TransportStdout {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.as_mut().get_mut();
        if !this.connected {
            let mut connect = std::pin::pin!(this.server.connect());
            match std::future::Future::poll(connect.as_mut(), cx) {
                Poll::Ready(Ok(())) => {
                    this.connected = true;
                }
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Pending => return Poll::Pending,
            }
        }
        Pin::new(&mut this.server).poll_read(cx, buf)
    }
}

/// Prepares the child's stdout slot: creates an asynchronous overlapped named pipe server for the
/// parent, and an inheritable synchronous client handle for the child.
fn prepare_stdout(
    stdio: &ChildStdio,
    inherited: &mut Vec<OwnedHandle>,
) -> io::Result<(HANDLE, Option<TransportStdout>)> {
    match stdio {
        ChildStdio::Piped => {
            static PIPE_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
            let counter = PIPE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let pipe_name = format!(
                r"\\.\pipe\ferryx-bridge-stdout-{}-{}",
                std::process::id(),
                counter
            );

            // Server side: asynchronous (overlapped) named pipe server registered with Tokio IOCP.
            let server = tokio::net::windows::named_pipe::ServerOptions::new()
                .access_inbound(true)
                .access_outbound(false)
                .first_pipe_instance(true)
                .in_buffer_size(65536)
                .out_buffer_size(65536)
                .create(&pipe_name)?;

            // Client side: synchronous so ssh.exe can write stdout without an OVERLAPPED.
            // The parent's separate server handle remains asynchronous for Tokio IOCP.
            let mut pipe_name_wide: Vec<u16> = pipe_name.encode_utf16().collect();
            pipe_name_wide.push(0);

            let client_handle = unsafe {
                CreateFileW(
                    pipe_name_wide.as_ptr(),
                    GENERIC_WRITE,
                    0,
                    std::ptr::null(),
                    OPEN_EXISTING,
                    0,
                    std::ptr::null_mut(),
                )
            };
            if client_handle.is_null() || client_handle as isize == INVALID_HANDLE_VALUE {
                return Err(io::Error::last_os_error());
            }
            let client_owned = unsafe { OwnedHandle::from_raw_handle(client_handle as RawHandle) };
            let child = duplicate_inheritable(client_owned.as_raw_handle() as HANDLE, inherited)?;
            drop(client_owned);

            Ok((child, Some(TransportStdout::new(server))))
        }
        ChildStdio::Null => {
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(NUL_DEVICE)?;
            let handle = file.into_raw_handle();
            let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
            Ok((
                duplicate_inheritable(handle.as_raw_handle() as HANDLE, inherited)?,
                None,
            ))
        }
        ChildStdio::Handle(handle) => Ok((
            duplicate_inheritable(handle.as_raw_handle() as HANDLE, inherited)?,
            None,
        )),
    }
}

/// Prepares one stdio slot: returns the child-side inheritable handle plus the parent-side handle
/// the transport keeps for piped slots.
fn prepare_stdio(
    stdio: &ChildStdio,
    slot: u32,
    inherited: &mut Vec<OwnedHandle>,
) -> io::Result<(HANDLE, Option<OwnedHandle>)> {
    match stdio {
        ChildStdio::Piped => {
            let (mut read, mut write) = (std::ptr::null_mut(), std::ptr::null_mut());
            // SAFETY: null attributes ask for a non-inheritable pipe; the call fills both outputs.
            if unsafe { CreatePipe(&mut read, &mut write, std::ptr::null(), 0) } == 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: `CreatePipe` succeeded, so both handles are fresh and owned here.
            let (read, write) = unsafe {
                (
                    OwnedHandle::from_raw_handle(read as RawHandle),
                    OwnedHandle::from_raw_handle(write as RawHandle),
                )
            };
            let (child_end, parent_end) = if slot == STD_INPUT_HANDLE {
                (read, write)
            } else {
                (write, read)
            };
            let child = duplicate_inheritable(child_end.as_raw_handle() as HANDLE, inherited)?;
            Ok((child, Some(parent_end)))
        }
        ChildStdio::Null => {
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(NUL_DEVICE)?;
            let handle = file.into_raw_handle();
            // SAFETY: `into_raw_handle` transferred ownership of a live file handle.
            let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
            Ok((
                duplicate_inheritable(handle.as_raw_handle() as HANDLE, inherited)?,
                None,
            ))
        }
        ChildStdio::Handle(handle) => Ok((
            duplicate_inheritable(handle.as_raw_handle() as HANDLE, inherited)?,
            None,
        )),
    }
}

/// Duplicates a handle as inheritable for `CreateProcessW` and parks it in `inherited` so it is
/// closed as soon as the child has its own copy.
fn duplicate_inheritable(source: HANDLE, inherited: &mut Vec<OwnedHandle>) -> io::Result<HANDLE> {
    let mut duplicate: HANDLE = std::ptr::null_mut();
    // SAFETY: `source` is a live handle; both process handles are the current process.
    let duplicated = unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            source,
            GetCurrentProcess(),
            &mut duplicate,
            0,
            1,
            DUPLICATE_SAME_ACCESS,
        )
    };
    if duplicated == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `DuplicateHandle` succeeded, so `duplicate` is fresh and owned here.
    let duplicate = unsafe { OwnedHandle::from_raw_handle(duplicate as RawHandle) };
    let value = duplicate.as_raw_handle() as HANDLE;
    inherited.push(duplicate);
    Ok(value)
}

/// Resolves a program to the executable `CreateProcessW` must load, with the same search order
/// `std::process::Command` uses on Windows: a path is taken as given (with `.exe` appended when
/// that names an existing file), while a bare file name is searched in the application directory,
/// the system directory, the Windows directory, and then `PATH`. The current directory is
/// deliberately not searched.
fn resolve_program(program: &OsStr) -> io::Result<OsString> {
    let encoded = program.as_encoded_bytes();
    let usable = !encoded.is_empty()
        && !encoded.contains(&0)
        && !encoded.ends_with(b"\\")
        && !encoded.ends_with(b"/");
    if !usable {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "child program has no usable file name",
        ));
    }
    let has_exe_suffix = encoded.len() >= EXE_SUFFIX.len()
        && encoded[encoded.len() - EXE_SUFFIX.len()..].eq_ignore_ascii_case(EXE_SUFFIX.as_bytes());
    if encoded.iter().any(|c| *c == b'\\' || *c == b'/') {
        if has_exe_suffix {
            return Ok(program.to_os_string());
        }
        let mut appended = program.to_os_string();
        appended.push(EXE_SUFFIX);
        return Ok(if Path::new(&appended).is_file() {
            appended
        } else {
            program.to_os_string()
        });
    }
    let mut named = program.to_os_string();
    if !has_exe_suffix {
        named.push(EXE_SUFFIX);
    }
    for directory in search_directories() {
        let candidate = directory.join(&named);
        if candidate.is_file() {
            return Ok(candidate.into_os_string());
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!("program not found: {}", program.to_string_lossy()),
    ))
}

/// The directories a bare program name is searched in, in the documented order.
fn search_directories() -> Vec<PathBuf> {
    let mut directories = Vec::new();
    if let Some(parent) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        directories.push(parent);
    }
    if let Some(directory) = system_directory(GetSystemDirectoryW) {
        directories.push(directory);
    }
    if let Some(directory) = system_directory(GetWindowsDirectoryW) {
        directories.push(directory);
    }
    if let Some(path) = std::env::var_os("PATH") {
        directories
            .extend(std::env::split_paths(&path).filter(|entry| !entry.as_os_str().is_empty()));
    }
    directories
}

/// Reads a system directory through the two-call buffer pattern of the Windows directory APIs.
fn system_directory(get: unsafe extern "system" fn(*mut u16, u32) -> u32) -> Option<PathBuf> {
    let mut buffer = vec![0u16; 260];
    loop {
        // SAFETY: `buffer` is a live UTF-16 buffer and its length is passed with it.
        let written = unsafe { get(buffer.as_mut_ptr(), buffer.len() as u32) };
        if written == 0 {
            return None;
        }
        if (written as usize) < buffer.len() {
            buffer.truncate(written as usize);
            return Some(PathBuf::from(OsString::from_wide(&buffer)));
        }
        buffer.resize(written as usize + 1, 0);
    }
}

/// Builds a `CreateProcessW` command line from the program and its arguments, quoting exactly like
/// `std::process::Command` does (the MSVCRT rules in
/// <https://learn.microsoft.com/en-us/cpp/c-language/parsing-c-command-line-arguments>).
fn build_command_line(program: &OsStr, args: &[OsString]) -> io::Result<Vec<u16>> {
    if program.encode_wide().any(|unit| unit == 0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "NUL in child program",
        ));
    }
    // The program token is always quoted, exactly like `std::process::Command` builds it; file
    // paths cannot contain quotes, so it needs no escaping.
    let mut command_line = vec![b'"' as u16];
    command_line.extend(program.encode_wide());
    command_line.push(b'"' as u16);
    for arg in args {
        command_line.push(b' ' as u16);
        append_argument(&mut command_line, arg)?;
    }
    command_line.push(0);
    Ok(command_line)
}

fn append_argument(command_line: &mut Vec<u16>, argument: &OsStr) -> io::Result<()> {
    let encoded = argument.as_encoded_bytes();
    if encoded.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "NUL in child argument",
        ));
    }
    let quote = encoded.is_empty() || encoded.iter().any(|c| *c == b' ' || *c == b'\t');
    if quote {
        command_line.push(b'"' as u16);
    }
    let mut backslashes = 0usize;
    for unit in argument.encode_wide() {
        if unit == b'\\' as u16 {
            backslashes += 1;
        } else {
            if unit == b'"' as u16 {
                command_line.extend(std::iter::repeat_n(b'\\' as u16, backslashes + 1));
            }
            backslashes = 0;
        }
        command_line.push(unit);
    }
    if quote {
        command_line.extend(std::iter::repeat_n(b'\\' as u16, backslashes));
        command_line.push(b'"' as u16);
    }
    Ok(())
}

/// Builds the child's environment block from this process's environment plus `additions`, or `None`
/// to let `CreateProcessW` pass the parent environment through unchanged, like `Command` does when
/// no environment changes are requested.
fn build_environment(additions: &[(OsString, OsString)]) -> io::Result<Option<Vec<u16>>> {
    if additions.is_empty() {
        return Ok(None);
    }
    let mut merged: BTreeMap<OsString, OsString> = std::env::vars_os().collect();
    for (key, value) in additions {
        // Windows environment names are case-insensitive, so an addition replaces any spelling.
        let name = key.to_string_lossy().into_owned();
        merged.retain(|existing, _| {
            !existing
                .to_string_lossy()
                .eq_ignore_ascii_case(name.as_str())
        });
        merged.insert(key.clone(), value.clone());
    }
    let mut block = Vec::new();
    for (key, value) in merged {
        let invalid = key.is_empty()
            || key.as_encoded_bytes().contains(&0)
            || value.as_encoded_bytes().contains(&0);
        if invalid {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "NUL or empty name in child environment",
            ));
        }
        block.extend(key.encode_wide());
        block.push(b'=' as u16);
        block.extend(value.encode_wide());
        block.push(0);
    }
    block.push(0);
    Ok(Some(block))
}

/// Owner-lifetime and creation-window tests.
///
/// `creation_places_the_child_in_the_job_before_it_runs` and the flag checks run entirely in this
/// process; the two `owner_*_terminates_the_job_member` cases additionally re-exec this test binary
/// as an owner subprocess (`owner_lifetime_helper`, selected with `--ignored` and
/// `FERRYX_TRANSPORT_OWNER_TEST_MODE`) so that the owner's death is a real process event:
///
/// * the parent passes the member's stdin pipe read end to the owner and keeps the write end open
///   and unwritten, so the member's blocking read can never end on its own;
/// * the parent gives the owner a `Stdio::piped` stderr channel, which the owner re-passes to the
///   member as its stdout, so that channel closes only when both writer processes are gone;
/// * the owner announces readiness only after asserting the member is running and inside the job;
/// * the parent then either terminates the owner's process handle with a code of its own, or lets
///   the owner leave through `std::process::exit`, and asserts the owner's exit code pins that
///   cause before awaiting channel EOF as the member's death event.
///
/// The member's exit status is deliberately not evidence at either level: a member terminated
/// because the last handle to a kill-on-close job was closed exits with `STATUS_SUCCESS` (0), the
/// status the kernel's close path passes to its job termination, while `TerminateJobObject` is the
/// API that carries a caller-chosen code. Both levels therefore assert membership, liveness, and
/// the exclusion of every self-driven exit path instead of an exit code.
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    use std::os::windows::io::BorrowedHandle;
    use std::process::Stdio;
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

    /// Upper bound only: every await here is driven by a process event (normal exit, job kill,
    /// readiness line, or pipe EOF). Nothing waits a fixed time, and the blocked member can only
    /// end through the job, so this value only fires on a hang.
    const AWAIT_CEILING: Duration = Duration::from_secs(30);

    /// Readiness marker the owner subprocess prints on the channel only after it has proven the
    /// member is alive and inside the job.
    const OWNER_READY: &str = "FERRYX_TRANSPORT_OWNER_READY";
    /// Readiness marker the in-process member prints for itself before it blocks on its input.
    const MEMBER_READY: &str = "FERRYX_TRANSPORT_MEMBER_READY";
    /// Selects how the owner subprocess leaves after readiness.
    const OWNER_MODE_ENV: &str = "FERRYX_TRANSPORT_OWNER_TEST_MODE";
    const OWNER_MODE_KILLED: &str = "killed";
    const OWNER_MODE_EXIT: &str = "exit";
    /// Substring filter that makes the re-executed test binary run only the owner helper.
    const OWNER_HELPER_FILTER: &str = "transport_windows::tests::owner_lifetime_helper";
    /// Exit code the parent terminates the owner with, so the owner's exit status cannot be
    /// mistaken for a normal exit.
    const OWNER_KILL_CODE: u32 = 0x5A17;
    /// The probe child reports through these markers whether an inheritable handle crossed into it.
    const PROBE_HANDLE_ENV: &str = "FERRYX_TRANSPORT_PROBE_HANDLE";
    const PROBE_HELPER_FILTER: &str = "transport_windows::tests::handle_leak_probe_helper";
    const PROBE_SEPARATED: &str = "FERRYX_HANDLE_SEPARATED";
    const PROBE_INHERITED: &str = "FERRYX_HANDLE_INHERITED";
    /// The echo child gates on this variable and reports through these markers.
    const ECHO_CHILD_ENV: &str = "FERRYX_TRANSPORT_ECHO_CHILD";
    const ECHO_HELPER_FILTER: &str = "transport_windows::tests::stdio_echo_helper";
    const ECHO_READY: &str = "FERRYX_STDIO_ECHO_READY";
    const ECHO_LINE: &str = "FERRYX_STDIO_ROUNDTRIP";

    // Query-only declarations: the crate's enabled `windows-sys` features do not include them.
    #[link(name = "kernel32")]
    extern "system" {
        fn QueryInformationJobObject(
            job: HANDLE,
            class: i32,
            info: *mut c_void,
            length: u32,
            returned: *mut u32,
        ) -> i32;
        fn GetHandleInformation(handle: HANDLE, flags: *mut u32) -> i32;
        fn PeekNamedPipe(
            handle: HANDLE,
            buffer: *mut c_void,
            size: u32,
            read: *mut u32,
            available: *mut u32,
            left: *mut u32,
        ) -> i32;
    }

    fn member_program() -> OsString {
        std::env::var_os("COMSPEC").unwrap_or_else(|| OsString::from("cmd.exe"))
    }

    fn member_spec(args: &[&str]) -> ChildSpec {
        let mut spec = ChildSpec::new(member_program());
        spec.args = args.iter().map(OsString::from).collect();
        spec
    }

    /// Creates the member's stdin pipe: the caller keeps the write end open and never writes it, so
    /// a member reading the returned read end blocks until it is killed.
    fn blocking_input_pipe() -> io::Result<(OwnedHandle, OwnedHandle)> {
        let (mut read, mut write) = (std::ptr::null_mut(), std::ptr::null_mut());
        // SAFETY: null attributes ask for a non-inheritable pipe, and the call fills both outputs.
        if unsafe { CreatePipe(&mut read, &mut write, std::ptr::null(), 0) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `CreatePipe` succeeded, so both handles are fresh and owned here.
        unsafe {
            Ok((
                OwnedHandle::from_raw_handle(read as RawHandle),
                OwnedHandle::from_raw_handle(write as RawHandle),
            ))
        }
    }

    /// Clones a stdio handle inherited from the spawning parent.
    fn inherited_stdio(kind: u32) -> OwnedHandle {
        // SAFETY: the parent populated this stdio slot before spawning; the clone is owned here.
        let raw = unsafe { GetStdHandle(kind) };
        assert!(!raw.is_null(), "no inherited stdio handle in slot {kind}");
        // SAFETY: `raw` is a live inherited handle; the clone is non-inheritable by construction.
        let borrowed = unsafe { BorrowedHandle::borrow_raw(raw as RawHandle) };
        borrowed
            .try_clone_to_owned()
            .expect("clone inherited stdio handle")
    }

    /// Reads the channel line by line until `marker` appears, failing on early EOF.
    async fn await_marker<R>(channel: &mut BufReader<R>, marker: &str, who: &str)
    where
        R: tokio::io::AsyncRead + Unpin,
    {
        let mut seen: Vec<String> = Vec::new();
        loop {
            let mut line = String::new();
            let read = tokio::time::timeout(AWAIT_CEILING, channel.read_line(&mut line))
                .await
                .unwrap_or_else(|_| panic!("{who} never announced readiness"))
                .unwrap_or_else(|error| panic!("read {who} readiness channel: {error}"));
            if read == 0 {
                panic!("{who} channel closed before readiness; saw {seen:?}");
            }
            if line.trim_end() == marker {
                return;
            }
            if seen.len() == 8 {
                seen.remove(0);
            }
            seen.push(line.trim_end().to_string());
        }
    }

    /// Awaits channel EOF under a bound: the member's only exit path is the job.
    async fn await_member_death(channel: &mut BufReader<tokio::process::ChildStderr>) {
        let mut sink = [0u8; 256];
        loop {
            let read = tokio::time::timeout(AWAIT_CEILING, channel.read(&mut sink))
                .await
                .expect("job member outlived its owner: readiness channel never closed")
                .expect("read owner readiness channel");
            if read == 0 {
                return;
            }
        }
    }

    /// `prepare` is checked against the kernel, not against its own inputs: the job must report
    /// kill-on-close and its handle must not be inheritable.
    #[test]
    fn prepare_configures_kill_on_close_with_a_private_handle() {
        let owner = TransportOwner::prepare().expect("prepare");
        let job = owner.job.as_raw_handle() as HANDLE;

        let mut limits = ExtendedLimitInformation::default();
        let queried = unsafe {
            QueryInformationJobObject(
                job,
                JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS,
                &mut limits as *mut ExtendedLimitInformation as *mut c_void,
                std::mem::size_of::<ExtendedLimitInformation>() as u32,
                std::ptr::null_mut(),
            )
        };
        assert_ne!(
            queried,
            0,
            "QueryInformationJobObject failed: {}",
            io::Error::last_os_error()
        );
        assert_ne!(
            limits.basic_limit_information.limit_flags & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            0,
            "job is missing JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE"
        );

        let mut handle_flags = 0u32;
        let inspected = unsafe { GetHandleInformation(job, &mut handle_flags) };
        assert_ne!(
            inspected,
            0,
            "GetHandleInformation failed: {}",
            io::Error::last_os_error()
        );
        assert_eq!(
            handle_flags & HANDLE_FLAG_INHERIT,
            0,
            "job handle is inheritable"
        );
    }

    /// The creation-window regression: the member must be a job member and still suspended before
    /// anything resumes or assigns it, and an owner death at that instant must end it anyway.
    ///
    /// A spawn that creates the child first and assigns it afterwards fails here: membership would
    /// not hold yet, and an owner death would strand the member, so the final await would time out.
    #[tokio::test]
    async fn creation_places_the_child_in_the_job_before_it_runs() {
        let (member_stdin, blocking_write_end) =
            blocking_input_pipe().expect("blocking input pipe");
        let owner = TransportOwner::prepare().expect("prepare");
        let mut spec = member_spec(&["/d", "/q"]);
        spec.stdin = ChildStdio::Handle(member_stdin);
        spec.stdout = ChildStdio::Null;
        spec.stderr = ChildStdio::Null;
        let mut child = owner.create_suspended(spec).expect("create suspended");

        let mut in_job = 0i32;
        let job = owner.job.as_raw_handle() as HANDLE;
        let queried = unsafe { IsProcessInJob(child.handle(), job, &mut in_job) };
        assert_ne!(
            queried,
            0,
            "IsProcessInJob failed: {}",
            io::Error::last_os_error()
        );
        assert_ne!(in_job, 0, "member is not inside the job at creation");

        assert_eq!(
            child.resume().expect("resume member"),
            1,
            "member primary thread should not have run before resume"
        );

        // The owner dies with the member already inside the job, so nothing can be stranded.
        drop(owner);
        tokio::time::timeout(AWAIT_CEILING, child.wait())
            .await
            .expect("member survived its owner: a stranded or unsupervised child would hang here")
            .expect("wait for member");
        drop(blocking_write_end);
    }

    /// `spawn` resumes the member: `wait` can only observe the exit if the primary thread ran, so a
    /// suspended orphan would hang instead of passing.
    #[tokio::test]
    async fn spawn_resumes_the_child_to_its_normal_exit() {
        let owner = TransportOwner::prepare().expect("prepare");
        let mut spec = member_spec(&["/c", "exit", "0"]);
        spec.stdin = ChildStdio::Null;
        spec.stdout = ChildStdio::Null;
        spec.stderr = ChildStdio::Null;
        let mut child = owner.spawn(spec).expect("spawn");

        let status = tokio::time::timeout(AWAIT_CEILING, child.wait())
            .await
            .expect("child never exited: spawn did not resume it")
            .expect("wait for child");
        assert_eq!(status.code(), Some(0));
        drop(owner);
    }

    /// Closing the last job handle is what an owner process death does to its handles, because the
    /// job is private and non-inheritable: the member - which announces itself and then blocks on
    /// input this test never supplies - can only be ended by the job.
    #[tokio::test]
    async fn dropping_the_owner_terminates_a_blocked_member() {
        let (member_stdin, blocking_write_end) =
            blocking_input_pipe().expect("blocking input pipe");
        // The writer is the member's only self-driven exit path, so its non-inheritability is
        // proven, not assumed: no copy may exist that the member could close on its own.
        let mut writer_flags = 0u32;
        let writer_inspected = unsafe {
            GetHandleInformation(
                blocking_write_end.as_raw_handle() as HANDLE,
                &mut writer_flags,
            )
        };
        assert_ne!(
            writer_inspected,
            0,
            "GetHandleInformation(input writer) failed: {}",
            io::Error::last_os_error()
        );
        assert_eq!(
            writer_flags & HANDLE_FLAG_INHERIT,
            0,
            "input writer must not be inheritable"
        );

        let owner = TransportOwner::prepare().expect("prepare");
        let script = format!("echo {MEMBER_READY} & set /p ferryx_block=");
        let mut spec = member_spec(&["/d", "/q", "/c", script.as_str()]);
        spec.stdin = ChildStdio::Handle(member_stdin);
        spec.stdout = ChildStdio::Piped;
        spec.stderr = ChildStdio::Null;
        let mut child = owner.spawn(spec).expect("spawn");

        // Positive readiness from the member itself: it prints this before its blocking read, so
        // seeing the marker proves the member ran up to the read that parks it.
        let mut stdout = BufReader::new(child.take_stdout().expect("member stdout is piped"));
        await_marker(&mut stdout, MEMBER_READY, "member").await;
        assert!(
            child.try_wait().expect("try_wait").is_none(),
            "member exited instead of blocking on its input"
        );

        drop(owner);

        // The exit status is not the evidence: a member terminated because the last handle to a
        // kill-on-close job was closed exits with STATUS_SUCCESS (0), and only TerminateJobObject
        // carries a caller-chosen code. The causal chain is: alive at the blocking read, its only
        // self-driven exit path (a closed, unwritten input writer) still shut and non-inheritable,
        // and the exit observed only after the job closed. Without kill-on-close nothing can end the
        // member here, so this await hits the ceiling instead.
        tokio::time::timeout(AWAIT_CEILING, child.wait())
            .await
            .expect("member survived the job close")
            .expect("wait for member");
        // Only after the member's death is the writer released.
        drop(blocking_write_end);
    }

    /// Hard owner death: the parent terminates the owner's process handle while the member is
    /// blocked, so the member's survival is impossible on its own.
    #[tokio::test]
    async fn owner_kill_terminates_the_job_member() {
        assert_member_dies_with_owner(OWNER_MODE_KILLED).await;
    }

    /// Normal owner death without `Drop`: the owner leaves through `std::process::exit` and the OS
    /// closes its job handle, which must terminate the blocked member just the same.
    #[tokio::test]
    async fn owner_process_exit_terminates_the_job_member() {
        assert_member_dies_with_owner(OWNER_MODE_EXIT).await;
    }

    /// Shared proof chain for both owner death causes.
    async fn assert_member_dies_with_owner(mode: &str) {
        let (member_stdin, blocking_write_end) =
            blocking_input_pipe().expect("blocking input pipe");
        let mut owner = spawn_owner_subprocess(mode, member_stdin).expect("spawn owner subprocess");
        let mut channel = BufReader::new(owner.stderr.take().expect("owner stderr is piped"));

        await_marker(&mut channel, OWNER_READY, "owner subprocess").await;

        if mode == OWNER_MODE_KILLED {
            let handle = owner.raw_handle().expect("owner process handle");
            assert_ne!(
                unsafe { TerminateProcess(handle as HANDLE, OWNER_KILL_CODE) },
                0,
                "TerminateProcess(owner) failed: {}",
                io::Error::last_os_error()
            );
        }

        let status = tokio::time::timeout(AWAIT_CEILING, owner.wait())
            .await
            .expect("owner subprocess never exited")
            .expect("wait for owner subprocess");
        let expected = if mode == OWNER_MODE_KILLED {
            OWNER_KILL_CODE as i32
        } else {
            0
        };
        assert_eq!(
            status.code(),
            Some(expected),
            "owner ({mode}) exit status must show how it died"
        );

        // The member cannot have exited on its own, so channel EOF is its death: the channel keeps
        // a writer from the owner (now gone) and one from the member's inherited stdout.
        await_member_death(&mut channel).await;
        drop(blocking_write_end);
    }

    fn spawn_owner_subprocess(
        mode: &str,
        member_stdin: OwnedHandle,
    ) -> io::Result<tokio::process::Child> {
        let mut command = tokio::process::Command::new(std::env::current_exe()?);
        command
            .args(["--ignored", "--nocapture", OWNER_HELPER_FILTER])
            .env(OWNER_MODE_ENV, mode)
            .stdin(Stdio::from(member_stdin))
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        command.spawn()
    }

    /// Owner subprocess entry point: creates a job-backed member blocked on its inherited stdin,
    /// proves the member is running and inside the job, announces readiness, then parks for the
    /// parent's kill or leaves through `std::process::exit`.
    ///
    /// Inert when run without `FERRYX_TRANSPORT_OWNER_TEST_MODE`, so an ordinary `--ignored` sweep
    /// passes without spawning anything.
    #[tokio::test]
    #[ignore = "re-executed as the owner subprocess by the owner lifetime tests"]
    async fn owner_lifetime_helper() {
        let Ok(mode) = std::env::var(OWNER_MODE_ENV) else {
            return;
        };
        let member_stdin = inherited_stdio(STD_INPUT_HANDLE);
        let readiness_channel = inherited_stdio(STD_ERROR_HANDLE);

        let owner = TransportOwner::prepare().expect("prepare member job");
        let mut spec = member_spec(&["/d", "/q"]);
        spec.stdin = ChildStdio::Handle(member_stdin.try_clone().expect("clone member stdin"));
        spec.stdout = ChildStdio::Handle(
            readiness_channel
                .try_clone()
                .expect("clone readiness channel"),
        );
        spec.stderr = ChildStdio::Null;
        let mut member = owner.spawn(spec).expect("spawn member");

        let mut in_job = 0i32;
        let job = owner.job.as_raw_handle() as HANDLE;
        let queried = unsafe { IsProcessInJob(member.handle(), job, &mut in_job) };
        assert_ne!(
            queried,
            0,
            "IsProcessInJob failed: {}",
            io::Error::last_os_error()
        );
        assert_ne!(in_job, 0, "member is not inside the job at readiness");
        assert!(
            member.try_wait().expect("try_wait").is_none(),
            "member is not running at readiness"
        );

        eprintln!("{OWNER_READY}");
        match mode.as_str() {
            OWNER_MODE_KILLED => std::future::pending::<()>().await,
            OWNER_MODE_EXIT => std::process::exit(0),
            other => panic!("unknown owner mode {other:?}"),
        }
    }

    /// A bare program name (no directory, no extension) must be resolved through the documented
    /// search order: `CreateProcessW` never searches `PATH` for a non-null application name, so a
    /// spawn that skipped resolution would fail with `ERROR_FILE_NOT_FOUND`.
    #[tokio::test]
    async fn bare_program_name_is_resolved_and_runs() {
        let owner = TransportOwner::prepare().expect("prepare");
        let mut spec = ChildSpec::new("cmd");
        spec.args = vec![
            OsString::from("/c"),
            OsString::from("exit"),
            OsString::from("0"),
        ];
        spec.stdin = ChildStdio::Null;
        spec.stdout = ChildStdio::Null;
        spec.stderr = ChildStdio::Null;
        let mut child = owner.spawn(spec).expect("spawn bare program");

        let status = tokio::time::timeout(AWAIT_CEILING, child.wait())
            .await
            .expect("bare program never exited")
            .expect("wait for child");
        assert_eq!(status.code(), Some(0));
        drop(owner);
    }

    /// Child stdio must work as real asynchronous streams. The fixture is this test binary
    /// re-executed as an echo child, not an external filter: console tools buffer their stdout when
    /// it is a pipe, which would measure their buffering instead of the transport.
    ///
    /// The echo child announces itself before it reads anything, so the read direction is proven
    /// first and a failure of the second await can only be the write direction.
    #[tokio::test]
    async fn stdio_roundtrips_through_tokio_streams() {
        let owner = TransportOwner::prepare().expect("prepare");
        let mut spec = ChildSpec::new(std::env::current_exe().expect("current exe"));
        spec.args = ["--ignored", "--nocapture", ECHO_HELPER_FILTER]
            .into_iter()
            .map(OsString::from)
            .collect();
        spec.envs = vec![(OsString::from(ECHO_CHILD_ENV), OsString::from("1"))];
        spec.stdin = ChildStdio::Piped;
        spec.stdout = ChildStdio::Piped;
        spec.stderr = ChildStdio::Null;
        let mut child = owner.spawn(spec).expect("spawn echo child");
        let mut stdin = child.take_stdin().expect("child stdin is piped");
        let mut stdout = BufReader::new(child.take_stdout().expect("child stdout is piped"));

        // Read direction: the echo child writes this before it blocks on its input.
        await_marker(&mut stdout, ECHO_READY, "echo child").await;
        stdin
            .write_all(format!("{ECHO_LINE}\r\n").as_bytes())
            .await
            .expect("write child stdin");
        stdin.flush().await.expect("flush child stdin");
        // Write direction: the echo can only appear if the child received what the test wrote.
        await_marker(&mut stdout, ECHO_LINE, "echo child").await;

        drop(stdin);
        drop(owner);
        tokio::time::timeout(AWAIT_CEILING, child.wait())
            .await
            .expect("member survived the job close")
            .expect("wait for member");
    }

    /// An inheritable handle that is not part of the child's stdio must not cross into the child:
    /// the handle list constrains inheritance to exactly the stdio handles, so an inheritable
    /// handle held elsewhere in this process cannot leak into a transport child.
    #[tokio::test]
    async fn unrelated_inheritable_handles_do_not_leak_into_the_child() {
        let (mut read, mut write) = (std::ptr::null_mut(), std::ptr::null_mut());
        // SAFETY: null attributes ask for a non-inheritable pipe, and the call fills both outputs.
        assert_ne!(
            unsafe { CreatePipe(&mut read, &mut write, std::ptr::null(), 0) },
            0,
            "CreatePipe failed: {}",
            io::Error::last_os_error()
        );
        // SAFETY: the pipe was just created, so both handles are fresh and owned here.
        let (read, write) = unsafe {
            (
                OwnedHandle::from_raw_handle(read as RawHandle),
                OwnedHandle::from_raw_handle(write as RawHandle),
            )
        };
        // The inheritable handle the child must not receive, primed with a byte so that a leaked
        // copy would be observable in the probe child.
        let mut inheritable = Vec::new();
        let leaked = duplicate_inheritable(read.as_raw_handle() as HANDLE, &mut inheritable)
            .expect("duplicate the inheritable read end");
        let mut writer = std::fs::File::from(write);
        std::io::Write::write_all(&mut writer, b".").expect("prime the pipe");

        let owner = TransportOwner::prepare().expect("prepare");
        let mut spec = ChildSpec::new(std::env::current_exe().expect("current exe"));
        spec.args = ["--ignored", "--nocapture", PROBE_HELPER_FILTER]
            .into_iter()
            .map(OsString::from)
            .collect();
        spec.envs = vec![(
            OsString::from(PROBE_HANDLE_ENV),
            OsString::from((leaked as usize).to_string()),
        )];
        spec.stdin = ChildStdio::Null;
        spec.stdout = ChildStdio::Piped;
        spec.stderr = ChildStdio::Null;
        let mut child = owner.spawn(spec).expect("spawn probe child");
        let mut stdout = BufReader::new(child.take_stdout().expect("probe stdout is piped"));

        await_marker(&mut stdout, PROBE_SEPARATED, "probe child").await;
        drop(owner);
        tokio::time::timeout(AWAIT_CEILING, child.wait())
            .await
            .expect("probe child survived the job close")
            .expect("wait for probe child");
        drop(inheritable);
    }

    /// Probe child: reports whether the handle value from the environment is present *and* still
    /// refers to the parent's primed pipe.
    ///
    /// Inert when run without `FERRYX_TRANSPORT_PROBE_HANDLE`, so an ordinary `--ignored` sweep
    /// passes without inspecting anything.
    #[test]
    #[ignore = "re-executed as the probe child by the inheritance test"]
    fn handle_leak_probe_helper() {
        let Ok(value) = std::env::var(PROBE_HANDLE_ENV) else {
            return;
        };
        let Ok(value) = value.parse::<usize>() else {
            return;
        };
        let mut available = 0u32;
        // SAFETY: `PeekNamedPipe` only inspects a pipe handle; an absent or unrelated handle makes
        // the call fail, and nothing is read from or written to the pipe.
        let inherited = unsafe {
            PeekNamedPipe(
                value as HANDLE,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut available,
                std::ptr::null_mut(),
            )
        } != 0
            && available > 0;
        println!(
            "{}",
            if inherited {
                PROBE_INHERITED
            } else {
                PROBE_SEPARATED
            }
        );
    }

    /// Echo child: announces readiness, then echoes every line it reads until its input ends.
    ///
    /// Inert when run without the gate variable, so an ordinary `--ignored` sweep passes without
    /// reading anything. Both markers are flushed explicitly, so a failure cannot be attributed to
    /// this process's own buffering.
    #[test]
    #[ignore = "re-executed as the echo child by the stdio roundtrip test"]
    fn stdio_echo_helper() {
        if std::env::var_os(ECHO_CHILD_ENV).is_none() {
            return;
        }
        let mut stdout = std::io::stdout();
        println!("{ECHO_READY}");
        std::io::Write::flush(&mut stdout).expect("flush readiness");
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            let Ok(line) = line else {
                break;
            };
            println!("{line}");
            std::io::Write::flush(&mut stdout).expect("flush echo");
        }
    }

    #[tokio::test]
    async fn named_pipe_stdout_streams_bytes_until_eof() {
        use tokio::io::AsyncReadExt;
        let owner = TransportOwner::prepare().expect("prepare");
        let mut spec = ChildSpec::new("cmd.exe");
        spec.args = vec![
            OsString::from("/c"),
            OsString::from("echo hello-named-pipe"),
        ];
        spec.stdin = ChildStdio::Null;
        spec.stdout = ChildStdio::Piped;
        spec.stderr = ChildStdio::Null;
        let mut child = owner.spawn(spec).expect("spawn cmd child");
        let mut stdout = child.take_stdout().expect("named pipe stdout");
        let mut bytes = Vec::new();
        tokio::time::timeout(AWAIT_CEILING, stdout.read_to_end(&mut bytes))
            .await
            .expect("read_to_end timeout")
            .expect("read_to_end ok");
        assert!(bytes.starts_with(b"hello-named-pipe"));
        let status = tokio::time::timeout(AWAIT_CEILING, child.wait())
            .await
            .expect("wait timeout")
            .expect("wait ok");
        assert_eq!(status.code(), Some(0));
        drop(owner);
    }
}
