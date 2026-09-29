//! Windows process plumbing for the session host (design rev3 section 3).
//!
//! - Versioned host copy: hash, temp copy, re-hash, rename; re-verified on every use, so a host
//!   never runs from (or locks) `$INSTDIR\ferryx.exe`.
//! - Daemon side: `CreateProcessW` of the host copy with no inherited handles.
//! - Host side: the ConPTY shell is created suspended, assigned to the host's job, and only then
//!   resumed, so every descendant is in the job before the shell runs a single instruction.
//! - Process identity: handles opened with `SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION`;
//!   creation times are read through those handles (FILETIME ticks, 100 ns since 1601), which
//!   makes pid reuse detectable and, while the handle is held, impossible.
//!
//! Windows only; `session_host/mod.rs` declares this module under `cfg(windows)`.

use std::collections::BTreeMap;
use std::ffi::{c_void, OsStr};
use std::fs::{self, File};
use std::io;
use std::mem;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::{mpsc, Arc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use thiserror::Error;
use windows_sys::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_INSUFFICIENT_BUFFER, ERROR_INVALID_PARAMETER, FILETIME, HANDLE,
    INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::System::Console::{
    ClosePseudoConsole, CreatePseudoConsole, ResizePseudoConsole, COORD, HPCON,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob, TerminateJobObject,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetCurrentProcess, GetExitCodeProcess,
    GetProcessTimes, InitializeProcThreadAttributeList, OpenProcess, ResumeThread,
    TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject, CREATE_NO_WINDOW,
    CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, EXTENDED_STARTUPINFO_PRESENT, INFINITE,
    LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SYNCHRONIZE, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, STARTF_USESTDHANDLES,
    STARTUPINFOEXW, STARTUPINFOW,
};

use super::fence::{OpenedProcess, ProcessHandle};
use super::protocol::encode_hex;
use super::registry::ProcessProbe;

/// First argument that switches `ferryx.exe` into host mode (dispatched in main.rs).
pub const SESSION_HOST_FLAG: &str = "--session-host";
/// Followed by the spec path written by the daemon.
pub const SPEC_FLAG: &str = "--spec";
/// Overrides the versioned-copy root (QA isolation).
pub const HOST_DIR_ENV: &str = "FERRYX_SESSION_HOST_DIR";
pub const HOST_EXE_NAME: &str = "ferryx.exe";

const UNSTARTED_KILL_WAIT: Duration = Duration::from_secs(5);

fn with_op(op: &'static str, error: io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("{op} failed: {error}"))
}

/// Must be called immediately after the failing Win32 call, before anything can reset the
/// thread's last-error value.
fn os_err(op: &'static str) -> io::Error {
    with_op(op, io::Error::last_os_error())
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn raw(handle: &OwnedHandle) -> HANDLE {
    handle.as_raw_handle() as HANDLE
}

/// Takes ownership of a handle returned by a Win32 call that reports failure as null.
///
/// # Safety
/// `handle` must be null or a fresh handle that nothing else owns or closes.
unsafe fn owned_or_err(handle: HANDLE, op: &'static str) -> io::Result<OwnedHandle> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(os_err(op));
    }
    // SAFETY: per the function contract the handle is fresh and uniquely owned.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle as RawHandle) })
}

fn filetime_ticks(time: FILETIME) -> u64 {
    (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)
}

fn handle_creation_time(handle: HANDLE) -> io::Result<u64> {
    let zero = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut creation, mut exit, mut kernel, mut user) = (zero, zero, zero, zero);
    // SAFETY: handle is live and carries PROCESS_QUERY_LIMITED_INFORMATION (or is the
    // current-process pseudo handle); all four out pointers are valid for the call.
    let ok = unsafe { GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) };
    if ok == 0 {
        return Err(os_err("GetProcessTimes"));
    }
    Ok(filetime_ticks(creation))
}

/// Creation time of this process, for the host registry record and Welcome.
pub fn current_process_creation_time() -> io::Result<u64> {
    // SAFETY: GetCurrentProcess returns a pseudo handle that is never closed.
    handle_creation_time(unsafe { GetCurrentProcess() })
}

/// A process handle this process owns. Handles from [WinProcess::open] carry
/// `SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION` only; handles from `CreateProcessW` carry
/// full access, including `PROCESS_TERMINATE`.
#[derive(Debug)]
pub struct WinProcess(OwnedHandle);

impl WinProcess {
    /// Returns the raw OS error on failure so callers can classify it (see [probe_process]).
    pub fn open(pid: u32) -> io::Result<Self> {
        // SAFETY: OpenProcess takes no pointers; a non-null result is a fresh handle we own.
        let handle = unsafe {
            OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, 0, pid)
        };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: checked non-null above; the handle is uniquely owned.
        Ok(Self(unsafe { OwnedHandle::from_raw_handle(handle as RawHandle) }))
    }

    fn raw(&self) -> HANDLE {
        raw(&self.0)
    }

    pub fn creation_time(&self) -> io::Result<u64> {
        handle_creation_time(self.raw())
    }

    /// Event wait on the process handle; `Ok(true)` once the process has exited. Bounded:
    /// the timeout is clamped below INFINITE.
    pub fn wait_timeout(&self, timeout: Duration) -> io::Result<bool> {
        let millis = u32::try_from(timeout.as_millis())
            .unwrap_or(u32::MAX)
            .min(INFINITE - 1);
        // SAFETY: the handle is live and carries SYNCHRONIZE.
        match unsafe { WaitForSingleObject(self.raw(), millis) } {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            _ => Err(os_err("WaitForSingleObject")),
        }
    }

    /// Exit code; meaningful only after [WinProcess::wait_timeout] returned `Ok(true)`.
    pub fn exit_code(&self) -> io::Result<u32> {
        let mut code = 0u32;
        // SAFETY: the handle is live with query access; code is a valid out pointer.
        if unsafe { GetExitCodeProcess(self.raw(), &mut code) } == 0 {
            return Err(os_err("GetExitCodeProcess"));
        }
        Ok(code)
    }

    /// Requires `PROCESS_TERMINATE`, which only `CreateProcessW` handles carry here.
    pub fn terminate(&self, exit_code: u32) -> io::Result<()> {
        // SAFETY: the handle is live; TerminateProcess fails cleanly without terminate access.
        if unsafe { TerminateProcess(self.raw(), exit_code) } == 0 {
            return Err(os_err("TerminateProcess"));
        }
        Ok(())
    }
}

impl ProcessHandle for WinProcess {
    fn duplicate(&self) -> io::Result<Self> {
        self.0.try_clone().map(Self)
    }
}

/// Opens `pid` and reads its creation time through the same handle (fence `PeerFacts.process`,
/// `activate_commit`'s `successor_process`).
pub fn open_identity(pid: u32) -> io::Result<OpenedProcess<WinProcess>> {
    let handle = WinProcess::open(pid)?;
    let creation_time = handle.creation_time()?;
    Ok(OpenedProcess {
        creation_time,
        handle,
    })
}

/// The process half of `host_liveness`. Only an observed exit, a creation-time mismatch, or
/// `ERROR_INVALID_PARAMETER` can lead to Dead in `registry::classify_liveness`.
pub fn probe_process(pid: u32) -> ProcessProbe {
    let process = match WinProcess::open(pid) {
        Ok(process) => process,
        Err(error) => {
            let code = error.raw_os_error().unwrap_or(0);
            return if code == ERROR_INVALID_PARAMETER as i32 {
                ProcessProbe::NoSuchProcess
            } else if code == ERROR_ACCESS_DENIED as i32 {
                ProcessProbe::AccessDenied
            } else {
                ProcessProbe::OpenFailed(code)
            };
        }
    };
    let os_code = |error: io::Error| error.raw_os_error().unwrap_or(0);
    let creation_time = process.creation_time().map_err(os_code);
    match process.wait_timeout(Duration::ZERO) {
        Ok(exited) => ProcessProbe::Opened {
            exited,
            creation_time,
        },
        // A failed wait is not a confirmed exit: report it through the query-error slot so the
        // classifier yields Unknown, never Dead.
        Err(error) => ProcessProbe::Opened {
            exited: false,
            creation_time: Err(os_code(error)),
        },
    }
}

/// The host's job. Created without `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`: only an explicit Close
/// reaps the tree via [ShellJob::terminate].
#[derive(Debug)]
pub struct ShellJob(OwnedHandle);

impl ShellJob {
    pub fn create() -> io::Result<Self> {
        // SAFETY: null attributes and name create an anonymous, non-inheritable job; a non-null
        // result is a fresh handle we own.
        unsafe { owned_or_err(CreateJobObjectW(ptr::null(), ptr::null()), "CreateJobObjectW") }
            .map(Self)
    }

    /// TerminateJobObject: reaches the shell and every descendant.
    pub fn terminate(&self, exit_code: u32) -> io::Result<()> {
        // SAFETY: the job handle is live and has JOB_OBJECT_TERMINATE (creator's full access).
        if unsafe { TerminateJobObject(raw(&self.0), exit_code) } == 0 {
            return Err(os_err("TerminateJobObject"));
        }
        Ok(())
    }

    pub fn contains(&self, process: &WinProcess) -> io::Result<bool> {
        let mut in_job = 0;
        // SAFETY: both handles are live; in_job is a valid out pointer.
        if unsafe { IsProcessInJob(process.raw(), raw(&self.0), &mut in_job) } == 0 {
            return Err(os_err("IsProcessInJob"));
        }
        Ok(in_job != 0)
    }
}

fn console_size(cols: u16, rows: u16) -> io::Result<COORD> {
    let x = i16::try_from(cols.max(1)).map_err(|_| invalid("cols exceeds i16::MAX"))?;
    let y = i16::try_from(rows.max(1)).map_err(|_| invalid("rows exceeds i16::MAX"))?;
    Ok(COORD { X: x, Y: y })
}

fn hresult(op: &'static str, hr: i32) -> io::Result<()> {
    if hr < 0 {
        return Err(io::Error::other(format!("{op} failed: HRESULT {hr:#010x}")));
    }
    Ok(())
}

/// Sole owner of the HPCON. Dropping it calls `ClosePseudoConsole`; on Windows before 11 24H2
/// that blocks until the output pipe is drained. Arm it with [PseudoConsole::arm_close] right
/// after launch; the armed closer runs `ClosePseudoConsole` on a thread created at arm time.
#[derive(Debug)]
pub struct PseudoConsole(HPCON);

impl PseudoConsole {
    pub fn resize(&self, cols: u16, rows: u16) -> io::Result<()> {
        let size = console_size(cols, rows)?;
        // SAFETY: self.0 is a live HPCON owned by this value.
        hresult("ResizePseudoConsole", unsafe { ResizePseudoConsole(self.0, size) })
    }

    /// Creates the dedicated close thread now, so that design section 5 step 6 never creates a
    /// thread at Close time. Call it during startup, before serving. If the thread cannot be
    /// created, the console is handed back unarmed: std drops the rejected closure (holding
    /// only an `Arc` clone) on this thread, so no `ClosePseudoConsole` runs here. The caller then
    /// fails startup and must drop its output reader before the console.
    pub fn arm_close(self) -> Result<ArmedConsole, (io::Error, PseudoConsole)> {
        let console = Arc::new(self);
        let owned = Arc::clone(&console);
        let (trigger, trigger_rx) = mpsc::sync_channel::<()>(1);
        let (done_tx, done) = mpsc::sync_channel::<()>(1);
        let spawned = thread::Builder::new()
            .name("ferryx-conpty-close".into())
            .spawn(move || {
                // A trigger or a dropped ArmedConsole both end the wait.
                let _ = trigger_rx.recv();
                drop(owned);
                let _ = done_tx.send(());
            });
        match spawned {
            Ok(thread) => Ok(ArmedConsole {
                console,
                trigger,
                done,
                _thread: thread,
            }),
            Err(error) => Err((
                error,
                Arc::try_unwrap(console).expect("rejected spawn closure is dropped before return"),
            )),
        }
    }
}

impl Drop for PseudoConsole {
    fn drop(&mut self) {
        // SAFETY: self.0 came from a successful CreatePseudoConsole and is closed exactly once.
        unsafe { ClosePseudoConsole(self.0) };
    }
}

/// A console whose close thread already exists. Field order matters: `console` (our Arc) drops
/// before `trigger`, so the final `ClosePseudoConsole` always runs on the close thread, never
/// the caller, including on an implicit drop.
#[derive(Debug)]
pub struct ArmedConsole {
    console: Arc<PseudoConsole>,
    trigger: mpsc::SyncSender<()>,
    done: mpsc::Receiver<()>,
    _thread: JoinHandle<()>,
}

impl ArmedConsole {
    pub fn resize(&self, cols: u16, rows: u16) -> io::Result<()> {
        self.console.resize(cols, rows)
    }

    /// Section 5 step 6. Never blocks and never creates a thread. The reader keeps draining
    /// while `ClosePseudoConsole` runs on the armed thread.
    pub fn close(self) -> CloseCompletion {
        let ArmedConsole {
            console,
            trigger,
            done,
            _thread,
        } = self;
        drop(console);
        let _ = trigger.send(());
        CloseCompletion { done }
    }
}

/// Completion signal of an armed close.
#[derive(Debug)]
pub struct CloseCompletion {
    done: mpsc::Receiver<()>,
}

impl CloseCompletion {
    /// True once `ClosePseudoConsole` returned. False on timeout, or if the close thread died
    /// without finishing.
    pub fn wait(&self, timeout: Duration) -> bool {
        self.done.recv_timeout(timeout).is_ok()
    }
}

/// A running ConPTY shell. Field order is drop order: the pipe ends close before the console, so
/// an implicit drop never blocks in `ClosePseudoConsole`.
#[derive(Debug)]
pub struct ConptyShell {
    /// Write end of the console input pipe (the host's single input writer).
    pub input: File,
    /// Read end of the console output pipe; EOF arrives after the console closes.
    pub output: File,
    pub console: PseudoConsole,
    pub process: WinProcess,
    pub pid: u32,
}

/// Everything the shell launch needs (fields of `registry::HostSpec`). `env` is the complete
/// environment block; later entries override earlier ones, keys compared case-insensitively.
#[derive(Debug, Clone, Copy)]
pub struct ShellCommand<'a> {
    pub program: &'a str,
    pub args: &'a [String],
    pub cwd: &'a Path,
    pub env: &'a [(String, String)],
    pub cols: u16,
    pub rows: u16,
}

fn anonymous_pipe() -> io::Result<(OwnedHandle, OwnedHandle)> {
    let mut read: HANDLE = ptr::null_mut();
    let mut write: HANDLE = ptr::null_mut();
    // SAFETY: both out pointers are valid; null attributes make both ends non-inheritable.
    if unsafe { CreatePipe(&mut read, &mut write, ptr::null(), 0) } == 0 {
        return Err(os_err("CreatePipe"));
    }
    // SAFETY: CreatePipe succeeded, so both are fresh handles owned only by us.
    Ok(unsafe {
        (
            OwnedHandle::from_raw_handle(read as RawHandle),
            OwnedHandle::from_raw_handle(write as RawHandle),
        )
    })
}

/// An initialized attribute list carrying `PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE`.
struct ProcThreadAttributeList {
    /// usize storage keeps the list pointer-aligned.
    storage: Vec<usize>,
}

impl ProcThreadAttributeList {
    fn with_pseudoconsole(console: &PseudoConsole) -> io::Result<Self> {
        let mut size = 0usize;
        // SAFETY: a null list with one attribute only reports the required size.
        if unsafe { InitializeProcThreadAttributeList(ptr::null_mut(), 1, 0, &mut size) } == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER as i32) {
                return Err(with_op("InitializeProcThreadAttributeList(size)", error));
            }
        }
        let mut storage = vec![0usize; size.div_ceil(mem::size_of::<usize>())];
        // SAFETY: storage holds at least `size` bytes and outlives the list.
        let ok = unsafe {
            InitializeProcThreadAttributeList(storage.as_mut_ptr().cast(), 1, 0, &mut size)
        };
        if ok == 0 {
            return Err(os_err("InitializeProcThreadAttributeList"));
        }
        // From here Drop runs DeleteProcThreadAttributeList, which needs an initialized list.
        let mut list = Self { storage };
        // SAFETY: the list is initialized. ConPTY takes the HPCON value itself (not a pointer to
        // it) with size_of::<HPCON>(); the caller keeps the console alive past CreateProcessW.
        let ok = unsafe {
            UpdateProcThreadAttribute(
                list.as_mut_ptr(),
                0,
                PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
                console.0 as *const c_void,
                mem::size_of::<HPCON>(),
                ptr::null_mut(),
                ptr::null(),
            )
        };
        if ok == 0 {
            return Err(os_err("UpdateProcThreadAttribute"));
        }
        Ok(list)
    }

    fn as_mut_ptr(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.storage.as_mut_ptr().cast()
    }
}

impl Drop for ProcThreadAttributeList {
    fn drop(&mut self) {
        // SAFETY: the list was initialized in with_pseudoconsole and is deleted exactly once.
        unsafe { DeleteProcThreadAttributeList(self.as_mut_ptr()) };
    }
}

/// Kills a shell that never ran user code (suspended, or resume failed), then waits for it.
fn kill_unstarted(process: &WinProcess) {
    if let Err(error) = process.terminate(1) {
        tracing::error!(%error, "failed to terminate unstarted session-host shell");
        return;
    }
    match process.wait_timeout(UNSTARTED_KILL_WAIT) {
        Ok(true) => {}
        Ok(false) => tracing::error!("unstarted session-host shell did not exit after terminate"),
        Err(error) => tracing::error!(%error, "waiting for terminated shell failed"),
    }
}

/// Host startup step 5: ConPTY shell created suspended, joined to `job`, then resumed.
/// On any error after `CreateProcessW` the suspended shell is terminated before returning, so
/// the controller sees `SESSION_HOST_SPAWN_FAILED` and nothing escapes the job.
pub fn launch_shell_in_job(command: &ShellCommand<'_>, job: &ShellJob) -> io::Result<ConptyShell> {
    let size = console_size(command.cols, command.rows)?;
    let line = shell_command_line(command.program, command.args)?;
    let mut command_line = wide_z(OsStr::new(&line), "command line")?;
    let cwd = wide_z(command.cwd.as_os_str(), "cwd")?;
    let env = env_block(command.env)?;

    let (in_read, in_write) = anonymous_pipe()?;
    let (out_read, out_write) = anonymous_pipe()?;
    let mut hpc: HPCON = 0;
    // SAFETY: both pipe handles are live and owned by this function; hpc is a valid out pointer.
    hresult("CreatePseudoConsole", unsafe {
        CreatePseudoConsole(size, raw(&in_read), raw(&out_write), 0, &mut hpc)
    })?;
    let console = PseudoConsole(hpc);
    // The console duplicated its ends. Closing ours means the reader sees EOF once the console
    // closes, and a dead console makes input writes fail instead of hang.
    drop(in_read);
    drop(out_write);
    // Bound after `console`, so on an early return they drop first and ClosePseudoConsole
    // cannot block on an undrained output pipe.
    let input = File::from(in_write);
    let output = File::from(out_read);

    let mut attributes = ProcThreadAttributeList::with_pseudoconsole(&console)?;
    // SAFETY: plain C structs; all-zero is a valid initial state.
    let mut startup: STARTUPINFOEXW = unsafe { mem::zeroed() };
    startup.StartupInfo.cb = mem::size_of::<STARTUPINFOEXW>() as u32;
    // Explicit invalid std handles keep the shell on the pseudoconsole even if the host itself
    // has redirected stdio.
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = INVALID_HANDLE_VALUE;
    startup.StartupInfo.hStdOutput = INVALID_HANDLE_VALUE;
    startup.StartupInfo.hStdError = INVALID_HANDLE_VALUE;
    startup.lpAttributeList = attributes.as_mut_ptr();
    // SAFETY: plain C struct; all-zero is valid.
    let mut info: PROCESS_INFORMATION = unsafe { mem::zeroed() };
    // SAFETY: command_line is a mutable NUL-terminated buffer; env is a double-NUL-terminated
    // UTF-16 block matching CREATE_UNICODE_ENVIRONMENT; cwd is NUL-terminated; startup and its
    // attribute list (and the HPCON it names) stay alive for the call; no handles are inherited.
    let ok = unsafe {
        CreateProcessW(
            ptr::null(),
            command_line.as_mut_ptr(),
            ptr::null(),
            ptr::null(),
            0,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT,
            env.as_ptr().cast(),
            cwd.as_ptr(),
            ptr::addr_of!(startup).cast::<STARTUPINFOW>(),
            &mut info,
        )
    };
    if ok == 0 {
        return Err(os_err("CreateProcessW(shell)"));
    }
    drop(attributes);
    // SAFETY: CreateProcessW succeeded; both handles are fresh and owned only by us.
    let process = WinProcess(unsafe { OwnedHandle::from_raw_handle(info.hProcess as RawHandle) });
    // SAFETY: as above.
    let thread = unsafe { OwnedHandle::from_raw_handle(info.hThread as RawHandle) };

    // SAFETY: job and process handles are live; the process is suspended and has run nothing.
    if unsafe { AssignProcessToJobObject(raw(&job.0), process.raw()) } == 0 {
        let error = os_err("AssignProcessToJobObject");
        kill_unstarted(&process);
        return Err(error);
    }
    // SAFETY: thread is the primary thread handle returned by CreateProcessW.
    if unsafe { ResumeThread(raw(&thread)) } == u32::MAX {
        let error = os_err("ResumeThread");
        kill_unstarted(&process);
        return Err(error);
    }
    drop(thread);
    Ok(ConptyShell {
        input,
        output,
        console,
        process,
        pid: info.dwProcessId,
    })
}

/// The daemon's handle on a host it started (full access: it can terminate the host if the
/// pipe handshake fails).
#[derive(Debug)]
pub struct SpawnedHost {
    pub process: WinProcess,
    pub pid: u32,
    pub creation_time: u64,
}

/// Spawn step 2: `CreateProcessW(copy, "--session-host --spec <p>")` with
/// `CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT | extra_flags` and `bInheritHandles=FALSE`.
/// `extra_flags` is `HostCapability::host_creation_flags()`. A denied breakaway surfaces as an
/// error (typically PermissionDenied), which the caller maps to SESSION_HOST_SPAWN_FAILED.
pub fn spawn_host(exe: &Path, spec: &Path, extra_flags: u32) -> io::Result<SpawnedHost> {
    let exe_text = exe
        .to_str()
        .ok_or_else(|| invalid("session-host exe path is not valid UTF-8"))?;
    let spec_text = spec
        .to_str()
        .ok_or_else(|| invalid("session-host spec path is not valid UTF-8"))?;
    let args = [
        SESSION_HOST_FLAG.to_owned(),
        SPEC_FLAG.to_owned(),
        spec_text.to_owned(),
    ];
    let line = shell_command_line(exe_text, &args)?;
    let mut command_line = wide_z(OsStr::new(&line), "command line")?;
    let application = wide_z(exe.as_os_str(), "exe path")?;
    // Run from the copy's own directory so the host never pins the daemon's cwd or $INSTDIR.
    let cwd_path = exe
        .parent()
        .ok_or_else(|| invalid("session-host exe path has no parent directory"))?;
    let cwd = wide_z(cwd_path.as_os_str(), "exe directory")?;

    // SAFETY: plain C structs; all-zero is valid.
    let mut startup: STARTUPINFOW = unsafe { mem::zeroed() };
    startup.cb = mem::size_of::<STARTUPINFOW>() as u32;
    // SAFETY: as above.
    let mut info: PROCESS_INFORMATION = unsafe { mem::zeroed() };
    // SAFETY: application and cwd are NUL-terminated; command_line is a mutable NUL-terminated
    // buffer; a null environment inherits the daemon's; nothing is inherited.
    let ok = unsafe {
        CreateProcessW(
            application.as_ptr(),
            command_line.as_mut_ptr(),
            ptr::null(),
            ptr::null(),
            0,
            CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT | extra_flags,
            ptr::null(),
            cwd.as_ptr(),
            &startup,
            &mut info,
        )
    };
    if ok == 0 {
        return Err(os_err("CreateProcessW(session host)"));
    }
    // SAFETY: CreateProcessW succeeded; both handles are fresh and owned only by us.
    let process = WinProcess(unsafe { OwnedHandle::from_raw_handle(info.hProcess as RawHandle) });
    // SAFETY: as above. The host runs unsuspended; its thread handle is not needed.
    drop(unsafe { OwnedHandle::from_raw_handle(info.hThread as RawHandle) });
    match process.creation_time() {
        Ok(creation_time) => Ok(SpawnedHost {
            process,
            pid: info.dwProcessId,
            creation_time,
        }),
        Err(error) => {
            if let Err(kill) = process.terminate(1) {
                tracing::error!(error = %kill, "failed to terminate session host after identity read failed");
            }
            Err(error)
        }
    }
}

#[derive(Debug, Error)]
pub enum CopyError {
    #[error("session-host root is unknown: neither {HOST_DIR_ENV} nor LOCALAPPDATA is set")]
    NoHostRoot,
    #[error("crate version {0:?} is not usable as a directory name")]
    BadVersion(String),
    #[error("{op} {}: {source}", path.display())]
    Io {
        op: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    #[error("session-host copy {} does not match the source hash", path.display())]
    HashMismatch { path: PathBuf },
}

fn io_at<'a>(op: &'static str, path: &'a Path) -> impl FnOnce(io::Error) -> CopyError + 'a {
    move |source| CopyError::Io {
        op,
        path: path.to_path_buf(),
        source,
    }
}

/// `FERRYX_SESSION_HOST_DIR`, else `%LOCALAPPDATA%\Ferryx\session-host`.
pub fn default_host_root() -> Result<PathBuf, CopyError> {
    if let Some(dir) = std::env::var_os(HOST_DIR_ENV).filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    std::env::var_os("LOCALAPPDATA")
        .filter(|value| !value.is_empty())
        .map(|dir| PathBuf::from(dir).join("Ferryx").join("session-host"))
        .ok_or(CopyError::NoHostRoot)
}

pub fn sha256_file(path: &Path) -> Result<[u8; 32], CopyError> {
    let mut file = File::open(path).map_err(io_at("open", path))?;
    let mut hasher = Sha256::new();
    io::copy(&mut file, &mut hasher).map_err(io_at("hash", path))?;
    Ok(hasher.finalize().into())
}

/// `<version>-<first 16 hex chars of sha256(exe)>`.
pub fn host_copy_dir_name(version: &str, digest: &[u8; 32]) -> String {
    format!("{version}-{}", encode_hex(&digest[..8]))
}

/// Returns the verified `<hostRoot>\<version>-<hash16>\ferryx.exe`, creating it if needed.
/// An existing copy is re-hashed on every call; a mismatch is quarantined as `*.bad-<ms>` and
/// recopied once. A second failure is returned; the caller classifies it as
/// `CopyCheck::Failed` (capability `copy-failed`).
pub fn ensure_versioned_copy(
    host_root: &Path,
    version: &str,
    source: &Path,
) -> Result<PathBuf, CopyError> {
    if version.is_empty() || version.contains(['/', '\\', ':']) || version.contains("..") {
        return Err(CopyError::BadVersion(version.to_owned()));
    }
    let digest = sha256_file(source)?;
    let dir = host_root.join(host_copy_dir_name(version, &digest));
    let target = dir.join(HOST_EXE_NAME);
    if let Err(first) = verify_or_install(&dir, &target, source, &digest) {
        tracing::warn!(dir = %dir.display(), error = %first, "session-host copy rejected; quarantining and recopying");
        quarantine(&dir)?;
        verify_or_install(&dir, &target, source, &digest)?;
    }
    Ok(target)
}

fn verify_or_install(
    dir: &Path,
    target: &Path,
    source: &Path,
    digest: &[u8; 32],
) -> Result<(), CopyError> {
    match fs::metadata(target) {
        Ok(_) if sha256_file(target)? == *digest => return Ok(()),
        Ok(_) => {
            return Err(CopyError::HashMismatch {
                path: target.to_path_buf(),
            })
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_at("stat", target)(error)),
    }
    fs::create_dir_all(dir).map_err(io_at("create", dir))?;
    let temp = dir.join(format!("{HOST_EXE_NAME}.tmp-{}", std::process::id()));
    let result = install_from_temp(&temp, target, source, digest);
    if result.is_err() {
        if let Err(error) = fs::remove_file(&temp) {
            if error.kind() != io::ErrorKind::NotFound {
                tracing::warn!(path = %temp.display(), %error, "failed to remove session-host temp copy");
            }
        }
    }
    result
}

fn install_from_temp(
    temp: &Path,
    target: &Path,
    source: &Path,
    digest: &[u8; 32],
) -> Result<(), CopyError> {
    fs::copy(source, temp).map_err(io_at("copy", temp))?;
    File::options()
        .write(true)
        .open(temp)
        .and_then(|file| file.sync_all())
        .map_err(io_at("fsync", temp))?;
    if sha256_file(temp)? != *digest {
        return Err(CopyError::HashMismatch {
            path: temp.to_path_buf(),
        });
    }
    fs::rename(temp, target).map_err(io_at("rename", target))
}

fn quarantine(dir: &Path) -> Result<(), CopyError> {
    if !dir.exists() {
        return Ok(());
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis());
    let mut name = dir.as_os_str().to_owned();
    name.push(format!(".bad-{stamp}"));
    fs::rename(dir, PathBuf::from(name)).map_err(io_at("quarantine", dir))
}

fn wide_z(text: &OsStr, what: &'static str) -> io::Result<Vec<u16>> {
    let mut wide: Vec<u16> = text.encode_wide().collect();
    if wide.contains(&0) {
        return Err(invalid(format!("{what} contains a NUL character")));
    }
    wide.push(0);
    Ok(wide)
}

/// MSVCRT/CommandLineToArgvW quoting for one argument.
fn quote_arg(arg: &str, out: &mut String) {
    if !arg.is_empty() && !arg.contains([' ', '\t', '\n', '\u{b}', '"']) {
        out.push_str(arg);
        return;
    }
    out.push('"');
    let mut backslashes = 0usize;
    for ch in arg.chars() {
        match ch {
            '\\' => backslashes += 1,
            '"' => {
                out.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                out.extend(std::iter::repeat_n('\\', backslashes));
                out.push(ch);
                backslashes = 0;
            }
        }
    }
    out.extend(std::iter::repeat_n('\\', backslashes * 2));
    out.push('"');
}

fn shell_command_line(program: &str, args: &[String]) -> io::Result<String> {
    // argv[0] is parsed without escape rules: quote it whole; it cannot contain a quote.
    if program.is_empty() || program.contains('"') {
        return Err(invalid("program is empty or contains a double quote"));
    }
    let mut line = String::with_capacity(program.len() + 2);
    line.push('"');
    line.push_str(program);
    line.push('"');
    for arg in args {
        line.push(' ');
        quote_arg(arg, &mut line);
    }
    Ok(line)
}

/// Double-NUL-terminated UTF-16 block, sorted case-insensitively as CreateProcessW expects.
/// Later duplicates (case-insensitive) override earlier ones. Keys may start with '=' (drive cwd
/// entries such as `=C:`) but may not contain '=' elsewhere.
fn env_block(env: &[(String, String)]) -> io::Result<Vec<u16>> {
    let mut merged: BTreeMap<String, (&str, &str)> = BTreeMap::new();
    for (key, value) in env {
        let bad_key = key.is_empty() || key.chars().skip(1).any(|c| c == '=');
        if bad_key || key.contains('\0') || value.contains('\0') {
            return Err(invalid(format!("invalid environment entry {key:?}")));
        }
        merged.insert(key.to_uppercase(), (key.as_str(), value.as_str()));
    }
    let mut block = Vec::new();
    for (key, value) in merged.values() {
        block.extend(key.encode_utf16());
        block.push(u16::from(b'='));
        block.extend(value.encode_utf16());
        block.push(0);
    }
    if block.is_empty() {
        block.push(0);
    }
    block.push(0);
    Ok(block)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quoted(arg: &str) -> String {
        let mut out = String::new();
        quote_arg(arg, &mut out);
        out
    }

    #[test]
    fn quote_arg_follows_argv_rules() {
        assert_eq!(quoted("plain"), "plain");
        assert_eq!(quoted(""), "\"\"");
        assert_eq!(quoted("a b"), "\"a b\"");
        assert_eq!(quoted("a\"b"), "\"a\\\"b\"");
        assert_eq!(quoted("C:\\dir x\\"), "\"C:\\dir x\\\\\"");
        assert_eq!(quoted("a\\\\\"b"), "\"a\\\\\\\\\\\"b\"");
    }

    #[test]
    fn command_line_rejects_quoted_program() {
        assert!(shell_command_line("a\"b", &[]).is_err());
        assert_eq!(
            shell_command_line("C:\\x y\\pwsh.exe", &["-NoLogo".into()]).unwrap(),
            "\"C:\\x y\\pwsh.exe\" -NoLogo"
        );
    }

    #[test]
    fn env_block_is_sorted_last_wins_and_double_nul_terminated() {
        let env = vec![
            ("path".to_owned(), "a".to_owned()),
            ("A".to_owned(), "1".to_owned()),
            ("PATH".to_owned(), "b".to_owned()),
        ];
        let block = String::from_utf16(&env_block(&env).unwrap()).unwrap();
        assert_eq!(block, "A=1\0PATH=b\0\0");
        assert_eq!(env_block(&[]).unwrap(), vec![0, 0]);
        assert!(env_block(&[("A=B".into(), "x".into())]).is_err());
        assert!(env_block(&[("=C:".into(), "C:\\".into())]).is_ok());
        assert!(env_block(&[("\u{e9}X".into(), "x".into())]).is_ok());
    }

    fn cmd_exe() -> String {
        let root = std::env::var("SystemRoot").expect("SystemRoot is set on Windows");
        format!("{root}\\System32\\cmd.exe")
    }

    fn launch(args: &[String], job: &ShellJob) -> ConptyShell {
        let env: Vec<(String, String)> = std::env::vars().collect();
        let program = cmd_exe();
        let cwd = std::env::temp_dir();
        let command = ShellCommand {
            program: &program,
            args,
            cwd: &cwd,
            env: &env,
            cols: 80,
            rows: 24,
        };
        launch_shell_in_job(&command, job).unwrap()
    }

    const BOUND: Duration = Duration::from_secs(10);

    /// Owns a launched shell for one test. A drain thread reads output to EOF and reports the
    /// byte count, and the close thread is armed up front. Drop terminates the job, closes the
    /// console and waits with bounds, so an assertion failure never leaks the shell or blocks
    /// the test thread.
    struct ShellFixture {
        job: ShellJob,
        process: WinProcess,
        input: Option<File>,
        console: Option<ArmedConsole>,
        drained: mpsc::Receiver<io::Result<usize>>,
    }

    impl ShellFixture {
        fn launch(args: &[String]) -> Self {
            let job = ShellJob::create().unwrap();
            let ConptyShell {
                input,
                mut output,
                console,
                process,
                ..
            } = launch(args, &job);
            let (tx, drained) = mpsc::sync_channel(1);
            thread::spawn(move || {
                let mut buffer = [0u8; 8192];
                let mut total = 0usize;
                let result = loop {
                    match io::Read::read(&mut output, &mut buffer) {
                        Ok(0) => break Ok(total),
                        Ok(n) => total += n,
                        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => break Ok(total),
                        Err(e) => break Err(e),
                    }
                };
                let _ = tx.send(result);
            });
            let console = match console.arm_close() {
                Ok(armed) => armed,
                Err((error, _console)) => {
                    let _ = job.terminate(1);
                    panic!("arm_close: {error}");
                }
            };
            Self {
                job,
                process,
                input: Some(input),
                console: Some(console),
                drained,
            }
        }

        /// Section 5 order after exit: drop input, close the console, then read drain EOF.
        /// Returns the drained byte count.
        fn close(mut self) -> usize {
            self.input.take();
            let completion = self.console.take().unwrap().close();
            assert!(completion.wait(BOUND), "ClosePseudoConsole did not complete");
            self.drained
                .recv_timeout(BOUND)
                .expect("output drain reached no EOF")
                .unwrap()
        }
    }

    impl Drop for ShellFixture {
        fn drop(&mut self) {
            let Some(console) = self.console.take() else {
                return;
            };
            let _ = self.job.terminate(1);
            self.input.take();
            let _ = self.process.wait_timeout(BOUND);
            // The drain thread keeps reading, so this close cannot stall on a full pipe.
            let _ = console.close().wait(BOUND);
        }
    }

    #[test]
    fn shell_runs_inside_job_and_reports_exit_code() {
        // About 150 KiB of output: well past the ConPTY pipe buffer, so the shell can only
        // reach `exit 7` while the drain thread keeps reading.
        let script = "(for /l %i in (1,1,3000) do @echo 0123456789012345678901234567890123456789) & exit 7";
        let shell = ShellFixture::launch(&["/d".into(), "/c".into(), script.into()]);
        assert!(shell.job.contains(&shell.process).unwrap());
        assert!(shell.process.wait_timeout(BOUND).unwrap());
        assert_eq!(shell.process.exit_code().unwrap(), 7);
        let drained = shell.close();
        assert!(drained >= 3000 * 40, "drained only {drained} bytes");
    }

    #[test]
    fn terminate_job_reaps_waiting_shell() {
        let shell = ShellFixture::launch(&["/d".into(), "/k".into()]);
        assert!(!shell.process.wait_timeout(Duration::ZERO).unwrap());
        shell.job.terminate(9).unwrap();
        assert!(shell.process.wait_timeout(BOUND).unwrap());
        assert_eq!(shell.process.exit_code().unwrap(), 9);
        shell.close();
    }

    #[test]
    fn probe_of_own_pid_is_opened_and_running() {
        match probe_process(std::process::id()) {
            ProcessProbe::Opened {
                exited,
                creation_time,
            } => {
                assert!(!exited);
                assert_eq!(
                    creation_time.unwrap(),
                    current_process_creation_time().unwrap()
                );
            }
            other => panic!("unexpected probe {other:?}"),
        }
    }

    #[test]
    fn duplicated_handle_reports_same_identity() {
        let opened = open_identity(std::process::id()).unwrap();
        let copy = opened.handle.duplicate().unwrap();
        assert_eq!(copy.creation_time().unwrap(), opened.creation_time);
    }
}
