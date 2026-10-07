use crate::cmdbuilder::CommandBuilder;
use crate::win::psuedocon::PsuedoCon;
use crate::{Child, MasterPty, PtyPair, PtySize, PtySystem, ReaderInterrupt, SlavePty};
use anyhow::Error;
use filedescriptor::{FileDescriptor, Pipe};
use std::io::Read;
use std::os::windows::io::{AsRawHandle, FromRawHandle, RawHandle};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use winapi::shared::minwindef::{DWORD, FALSE, TRUE};
use winapi::shared::winerror::{ERROR_IO_PENDING, ERROR_OPERATION_ABORTED};
use winapi::um::ioapiset::{CancelIoEx, GetOverlappedResult};
use winapi::um::minwinbase::OVERLAPPED;
use winapi::um::synchapi::{CreateEventW, SetEvent, WaitForMultipleObjects, WaitForSingleObject};
use winapi::um::winbase::{INFINITE, WAIT_FAILED, WAIT_OBJECT_0};
use winapi::um::wincon::COORD;

#[derive(Default)]
pub struct ConPtySystem {}

impl PtySystem for ConPtySystem {
    fn openpty(&self, size: PtySize) -> anyhow::Result<PtyPair> {
        let stdin = nonblocking_input_pipe()?;
        let stdout = overlapped_output_pipe()?;
        let cancel = ReaderCancel::new()?;

        let con = PsuedoCon::new(
            COORD {
                X: size.cols as i16,
                Y: size.rows as i16,
            },
            stdin.read,
            stdout.write,
        )?;

        let master = ConPtyMasterPty {
            inner: Arc::new(Mutex::new(Inner {
                con,
                readable: stdout.read,
                cancel,
                input_handle: stdin.write.try_clone()?,
                writable: Some(stdin.write),
                size,
            })),
        };

        let slave = ConPtySlavePty {
            inner: master.inner.clone(),
        };

        Ok(PtyPair {
            master: Box::new(master),
            slave: Box::new(slave),
        })
    }
}

struct Inner {
    input_handle: FileDescriptor,
    con: PsuedoCon,
    /// The overlapped read end of the output pipe. Readers clone this handle and share `cancel`.
    readable: FileDescriptor,
    /// This master's one cancellation event, shared with every reader cloned from it.
    cancel: Arc<ReaderCancel>,
    writable: Option<FileDescriptor>,
    size: PtySize,
}

impl Inner {
    pub fn resize(
        &mut self,
        num_rows: u16,
        num_cols: u16,
        pixel_width: u16,
        pixel_height: u16,
    ) -> Result<(), Error> {
        self.con.resize(COORD {
            X: num_cols as i16,
            Y: num_rows as i16,
        })?;
        self.size = PtySize {
            rows: num_rows,
            cols: num_cols,
            pixel_width,
            pixel_height,
        };
        Ok(())
    }
}

#[derive(Clone)]
pub struct ConPtyMasterPty {
    inner: Arc<Mutex<Inner>>,
}

pub struct ConPtySlavePty {
    inner: Arc<Mutex<Inner>>,
}

impl MasterPty for ConPtyMasterPty {
    fn resize(&self, size: PtySize) -> anyhow::Result<()> {
        let mut inner = self.inner.lock().unwrap();
        inner.resize(size.rows, size.cols, size.pixel_width, size.pixel_height)
    }

    fn get_size(&self) -> Result<PtySize, Error> {
        let inner = self.inner.lock().unwrap();
        Ok(inner.size.clone())
    }

    fn try_clone_reader(&self) -> anyhow::Result<Box<dyn std::io::Read + Send>> {
        let inner = self.inner.lock().unwrap();
        // Each reader duplicates the pipe handle but shares the ONE cancellation event of this
        // master, which is what keeps a cancellation scoped to this master's own readers.
        let handle = inner.readable.try_clone()?;
        Ok(Box::new(OverlappedOutputReader::new(
            handle,
            Arc::clone(&inner.cancel),
        )?))
    }

    fn interrupt_handle(&self) -> Option<Box<dyn ReaderInterrupt>> {
        // Cloned out of the master so it outlives it: a session gives the master up (a failed
        // handover export drops it) and must still be able to end its reader when it closes.
        Some(Box::new(MasterInterrupt(Arc::clone(
            &self.inner.lock().unwrap().cancel,
        ))))
    }

    fn take_writer(&self) -> anyhow::Result<Box<dyn std::io::Write + Send>> {
        Ok(Box::new(BlockingInput(
            self.inner
                .lock()
                .unwrap()
                .writable
                .take()
                .ok_or_else(|| anyhow::anyhow!("writer already taken"))?,
        )))
    }

    fn try_clone_input_handle(&self) -> anyhow::Result<std::os::windows::io::OwnedHandle> {
        use std::os::windows::io::{FromRawHandle, IntoRawHandle};
        let fd = self.inner.lock().unwrap().input_handle.try_clone()?;
        Ok(unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(fd.into_raw_handle()) })
    }
}

struct BlockingInput(FileDescriptor);
/// One master's reader-cancellation event.
///
/// Manual reset on purpose: once a teardown has asked for cancellation it stays asked, so repeated
/// requests are idempotent and a read issued after the request cannot slip through as a fresh
/// blocking wait. One event belongs to one master, so cancelling a master can only end the readers
/// cloned from that same master.
struct ReaderCancel {
    event: FileDescriptor,
    // Reads of this master that the kernel reported as pending, counted at the moment the kernel
    // said so. This is how a test waits for a read that is genuinely in flight instead of inferring
    // one from a delay; production never reads it.
    pending_reads: AtomicU64,
}

impl ReaderCancel {
    fn new() -> anyhow::Result<Arc<Self>> {
        // Manual reset, initially unsignalled, unnamed.
        let handle = unsafe { CreateEventW(std::ptr::null_mut(), TRUE, FALSE, std::ptr::null()) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(Arc::new(Self {
            event: unsafe { FileDescriptor::from_raw_handle(handle.cast()) },
            pending_reads: AtomicU64::new(0),
        }))
    }

    fn request(&self) {
        unsafe { SetEvent(self.event.as_raw_handle() as _) };
    }

    fn is_requested(&self) -> bool {
        unsafe { WaitForSingleObject(self.event.as_raw_handle() as _, 0) == WAIT_OBJECT_0 }
    }

    fn note_pending_read(&self) {
        self.pending_reads.fetch_add(1, Ordering::Release);
    }

    fn pending_read_count(&self) -> u64 {
        self.pending_reads.load(Ordering::Acquire)
    }
}

/// The master's cancellation event, handed out so it can outlive the master.
struct MasterInterrupt(Arc<ReaderCancel>);

impl ReaderInterrupt for MasterInterrupt {
    fn request(&self) {
        self.0.request();
    }

    fn pending_read_count(&self) -> u64 {
        self.0.pending_read_count()
    }
}

/// The result every cancelled read reports.
///
/// `Interrupted` is the kind the session's reader loop already tolerates, and it is deliberately
/// not EOF: a cancelled read must never be mistaken for the pane having closed.
fn cancelled_error() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Interrupted, "pty reader cancelled")
}

/// Reads the ConPTY output pipe through overlapped I/O, so a teardown can end a read that is
/// already in flight instead of leaking the thread that issued it.
///
/// Cancellation participates in the operation itself rather than in a flag checked around it: the
/// request is observed before a read is issued, and the event is one of the handles the wait blocks
/// on, so a request landing between those two points is still observed by that same wait instead of
/// by a later read that would block again.
struct OverlappedOutputReader {
    /// The read end, opened with FILE_FLAG_OVERLAPPED: reads on it must carry an OVERLAPPED.
    handle: FileDescriptor,
    /// This reader's own completion event (auto reset).
    completion: FileDescriptor,
    /// The master's cancellation event, shared with every reader cloned from that master.
    cancel: Arc<ReaderCancel>,
    /// Owned so its address stays stable for the kernel while an operation is in flight.
    overlapped: Box<OVERLAPPED>,
    /// True while the kernel still owns the buffer and OVERLAPPED of an issued operation.
    pending: bool,
}

// SAFETY: the kernel only touches `overlapped` and the caller's buffer while an operation is
// pending, and `read` always reaps that operation before returning, so the value is never moved or
// dropped with an operation outstanding. `read` takes `&mut self`, so two threads can never be
// inside it at once.
unsafe impl Send for OverlappedOutputReader {}

impl OverlappedOutputReader {
    fn new(handle: FileDescriptor, cancel: Arc<ReaderCancel>) -> std::io::Result<Self> {
        let completion =
            unsafe { CreateEventW(std::ptr::null_mut(), FALSE, FALSE, std::ptr::null()) };
        if completion.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Self {
            handle,
            completion: unsafe { FileDescriptor::from_raw_handle(completion.cast()) },
            cancel,
            overlapped: Box::new(unsafe { std::mem::zeroed() }),
            pending: false,
        })
    }

    /// Deliver bytes the pipe is already holding, without waiting for more.
    ///
    /// `PeekNamedPipe` answers what is buffered right now, so this never waits and never
    /// guesses: it either returns bytes that exist or reports that none do. That is what lets a
    /// cancellation end the read without discarding the child's last output.
    fn drain_available(&mut self, buf: &mut [u8]) -> Option<usize> {
        // Only ever called with no operation outstanding: issuing a second ReadFile onto an
        // OVERLAPPED the kernel still owns would be a use-after-issue, not a drain.
        if self.pending {
            return None;
        }
        let mut available: DWORD = 0;
        let peeked = unsafe {
            winapi::um::namedpipeapi::PeekNamedPipe(
                self.handle.as_raw_handle() as _,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut available,
                std::ptr::null_mut(),
            )
        };
        if peeked == 0 || available == 0 {
            return None;
        }
        let want = (available as usize).min(buf.len()) as DWORD;
        self.overlapped.hEvent = self.completion.as_raw_handle() as _;
        let mut transferred: DWORD = 0;
        self.pending = true;
        let issued = unsafe {
            winapi::um::fileapi::ReadFile(
                self.handle.as_raw_handle() as _,
                buf.as_mut_ptr().cast(),
                want,
                &mut transferred,
                &mut *self.overlapped,
            )
        };
        if issued != 0 {
            self.pending = false;
            return Some(transferred as usize);
        }
        // The bytes were there, so the completion is already on its way: reap it as usual.
        self.drain_pending()
    }

    /// Cancel and reap an operation the kernel still owns, so its OVERLAPPED and the caller's
    /// buffer are ours again. `GetOverlappedResult` with `bWait` set is what makes the reap
    /// synchronous: a cancelled operation still delivers a completion, and only after that may the
    /// memory be reused or freed. Returns the transferred count when the operation turned out to
    /// have completed anyway, so bytes that did arrive are never dropped.
    fn drain_pending(&mut self) -> Option<usize> {
        if !self.pending {
            return None;
        }
        let mut transferred: DWORD = 0;
        let collected = unsafe {
            CancelIoEx(self.handle.as_raw_handle() as _, &mut *self.overlapped);
            GetOverlappedResult(
                self.handle.as_raw_handle() as _,
                &mut *self.overlapped,
                &mut transferred,
                TRUE,
            )
        };
        self.pending = false;
        if collected != 0 {
            Some(transferred as usize)
        } else {
            None
        }
    }
}

impl Drop for OverlappedOutputReader {
    fn drop(&mut self) {
        let _ = self.drain_pending();
    }
}

impl Read for OverlappedOutputReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        // A request that is already pending must not be turned into a new read - but ending the
        // read must not discard output the pipe already holds either.
        if self.cancel.is_requested() {
            if let Some(transferred) = self.drain_available(buf) {
                return Ok(transferred);
            }
            return Err(cancelled_error());
        }
        self.overlapped.hEvent = self.completion.as_raw_handle() as _;
        let mut transferred: DWORD = 0;
        self.pending = true;
        let issued = unsafe {
            winapi::um::fileapi::ReadFile(
                self.handle.as_raw_handle() as _,
                buf.as_mut_ptr().cast(),
                buf.len() as DWORD,
                &mut transferred,
                &mut *self.overlapped,
            )
        };
        if issued != 0 {
            // Completed inline: nothing is pending and the bytes belong to the caller.
            self.pending = false;
            return Ok(transferred as usize);
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_IO_PENDING as i32) {
            self.pending = false;
            return Err(error);
        }
        // The wait carries the cancellation event, so a request that arrived before or during this
        // operation is observed here rather than missed.
        self.cancel.note_pending_read();
        let handles: [RawHandle; 2] = [
            self.completion.as_raw_handle(),
            self.cancel.event.as_raw_handle(),
        ];
        let waited = unsafe { WaitForMultipleObjects(2, handles.as_ptr(), FALSE, INFINITE) };
        if waited == WAIT_FAILED {
            let error = std::io::Error::last_os_error();
            let _ = self.drain_pending();
            return Err(error);
        }
        if waited == WAIT_OBJECT_0 + 1 {
            // Cancellation won. Bytes that completed in the same instant are still returned, so
            // output is never dropped; the next read sees the still-set event and stops.
            if let Some(transferred) = self.drain_pending() {
                return Ok(transferred);
            }
            // Output the child already wrote and the pipe already holds is delivered rather than
            // discarded: ending a read must not truncate the pane's last lines.
            if let Some(transferred) = self.drain_available(buf) {
                return Ok(transferred);
            }
            return Err(cancelled_error());
        }
        let collected = unsafe {
            GetOverlappedResult(
                self.handle.as_raw_handle() as _,
                &mut *self.overlapped,
                &mut transferred,
                TRUE,
            )
        };
        self.pending = false;
        if collected != 0 {
            return Ok(transferred as usize);
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(ERROR_OPERATION_ABORTED as i32) {
            return Err(cancelled_error());
        }
        Err(error)
    }
}
impl std::io::Write for BlockingInput {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        loop {
            match self.0.write(bytes) {
                Ok(0) if !bytes.is_empty() => std::thread::sleep(std::time::Duration::from_millis(1)),
                result => return result,
            }
        }
    }
    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}

fn nonblocking_input_pipe() -> anyhow::Result<Pipe> {
    use std::os::windows::io::FromRawHandle;
    use winapi::um::{fileapi::CreateFileW, namedpipeapi::CreateNamedPipeW, handleapi::INVALID_HANDLE_VALUE};
    use winapi::um::winbase::{PIPE_ACCESS_OUTBOUND, PIPE_TYPE_BYTE, PIPE_READMODE_BYTE, PIPE_NOWAIT};
    use winapi::um::winnt::GENERIC_READ;
    use winapi::um::fileapi::OPEN_EXISTING;
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let name: Vec<u16> = format!("\\\\.\\pipe\\ferryx-pty-{}-{}", std::process::id(), NEXT.fetch_add(1,std::sync::atomic::Ordering::Relaxed)).encode_utf16().chain(Some(0)).collect();
    let write = unsafe { CreateNamedPipeW(name.as_ptr(), PIPE_ACCESS_OUTBOUND | 0x00080000,
        PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_NOWAIT, 1, 4096, 4096, 0, std::ptr::null_mut()) };
    if write == INVALID_HANDLE_VALUE { return Err(std::io::Error::last_os_error().into()); }
    let write = unsafe { FileDescriptor::from_raw_handle(write.cast()) };
    let read = unsafe { CreateFileW(name.as_ptr(), GENERIC_READ, 0, std::ptr::null_mut(), OPEN_EXISTING, 0, std::ptr::null_mut()) };
    if read == INVALID_HANDLE_VALUE { return Err(std::io::Error::last_os_error().into()); }
    Ok(Pipe { read: unsafe { FileDescriptor::from_raw_handle(read.cast()) }, write })
}

/// Buffer sizes for the output pipe. The inbound side (console to us) is the one that carries pane
/// output, so it is the large one; the outbound side is unused in this direction.
const OUT_PIPE_BUFFER_BYTES: DWORD = 4 * 1024;
const IN_PIPE_BUFFER_BYTES: DWORD = 64 * 1024;

/// The ConPTY output pipe, with an overlapped read end so a teardown can cancel a read in flight.
///
/// `filedescriptor::Pipe::new` cannot be used here: `CreatePipe` makes an anonymous pipe, which
/// supports neither overlapped I/O nor a cancellation handle, and that is exactly what left the
/// reader parked forever. The shape mirrors `nonblocking_input_pipe` (a named pipe, one instance)
/// with the roles swapped: the server end is ours to read, and the client end is what the
/// pseudoconsole writes into.
fn overlapped_output_pipe() -> anyhow::Result<Pipe> {
    use winapi::um::fileapi::{CreateFileW, OPEN_EXISTING};
    use winapi::um::handleapi::INVALID_HANDLE_VALUE;
    use winapi::um::namedpipeapi::CreateNamedPipeW;
    use winapi::um::winbase::{
        FILE_FLAG_OVERLAPPED, PIPE_ACCESS_INBOUND, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_WAIT,
    };
    use winapi::um::winnt::GENERIC_WRITE;
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let name: Vec<u16> = format!(
        r"\\.\pipe\ferryx-pty-out-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )
    .encode_utf16()
    .chain(Some(0))
    .collect();
    // PIPE_WAIT, not PIPE_NOWAIT: the pseudoconsole's writes must block when the buffer is full,
    // while our side reads asynchronously through the overlapped handle.
    let read = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            PIPE_ACCESS_INBOUND | FILE_FLAG_OVERLAPPED | 0x00080000, // FILE_FLAG_FIRST_PIPE_INSTANCE
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
            1,
            OUT_PIPE_BUFFER_BYTES,
            IN_PIPE_BUFFER_BYTES,
            0,
            std::ptr::null_mut(),
        )
    };
    if read == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error().into());
    }
    let read = unsafe { FileDescriptor::from_raw_handle(read.cast()) };
    let write = unsafe {
        CreateFileW(
            name.as_ptr(),
            GENERIC_WRITE,
            0,
            std::ptr::null_mut(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    };
    if write == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(Pipe {
        read,
        write: unsafe { FileDescriptor::from_raw_handle(write.cast()) },
    })
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::io::Write;

    /// Deterministic by construction: the test owns BOTH ends of the pipe, so it knows exactly
    /// what is buffered at the moment it cancels. No delay, no inference.
    #[test]
    fn a_cancelled_read_delivers_bytes_the_pipe_already_holds() {
        let mut pipe = overlapped_output_pipe().expect("an overlapped output pipe");
        let cancel = ReaderCancel::new().expect("a cancellation event");
        let mut reader = OverlappedOutputReader::new(
            pipe.read.try_clone().expect("a duplicate read end"),
            Arc::clone(&cancel),
        )
        .expect("a reader");

        pipe.write_all(b"tail").expect("write into the pipe");
        cancel.request();

        let mut buf = [0u8; 16];
        let read = reader
            .read(&mut buf)
            .expect("a cancelled read must still deliver buffered bytes");
        assert_eq!(
            &buf[..read],
            b"tail",
            "cancellation discarded output the pipe already held"
        );

        // With the pipe empty the same reader reports the cancellation instead of blocking.
        let error = reader
            .read(&mut buf)
            .expect_err("an empty pipe has nothing left to deliver");
        assert_eq!(error.kind(), std::io::ErrorKind::Interrupted);
    }

    /// The subscription a regression uses in place of a delay: the count rises at the moment the
    /// kernel reports the read pending, and only a cancellation ends that read.
    #[test]
    fn a_pending_read_is_counted_when_the_kernel_reports_it() {
        let pipe = overlapped_output_pipe().expect("an overlapped output pipe");
        let cancel = ReaderCancel::new().expect("a cancellation event");
        let mut reader = OverlappedOutputReader::new(
            pipe.read.try_clone().expect("a duplicate read end"),
            Arc::clone(&cancel),
        )
        .expect("a reader");
        assert_eq!(cancel.pending_read_count(), 0, "nothing was issued yet");

        let (issued_tx, issued_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let mut buf = [0u8; 16];
            let _ = issued_tx.send(());
            let _ = done_tx.send(reader.read(&mut buf));
        });
        issued_rx.recv().expect("the worker reports before it reads");

        // Wait for the kernel's own report that the read is in flight; fail rather than hang.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while cancel.pending_read_count() == 0 {
            assert!(
                std::time::Instant::now() < deadline,
                "the kernel never reported the read as pending"
            );
            std::thread::yield_now();
        }

        cancel.request();
        let outcome = done_rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("a cancelled pending read must return");
        let error = outcome.expect_err("the read was cancelled, not completed");
        assert_eq!(error.kind(), std::io::ErrorKind::Interrupted);
        worker.join().expect("the reader thread ends");
    }

    /// Cancellation is scoped to one pipe: ending one reader must leave another reader waiting for
    /// its own pipe.
    #[test]
    fn cancelling_one_pipe_does_not_end_another_pipes_read() {
        let first = overlapped_output_pipe().expect("a first pipe");
        let second = overlapped_output_pipe().expect("a second pipe");
        let first_cancel = ReaderCancel::new().expect("a first event");
        let second_cancel = ReaderCancel::new().expect("a second event");
        let mut first_reader = OverlappedOutputReader::new(
            first.read.try_clone().expect("a duplicate read end"),
            Arc::clone(&first_cancel),
        )
        .expect("a reader");
        let mut second_reader = OverlappedOutputReader::new(
            second.read.try_clone().expect("a duplicate read end"),
            Arc::clone(&second_cancel),
        )
        .expect("a reader");

        first_cancel.request();
        let mut buf = [0u8; 8];
        assert!(
            first_reader.read(&mut buf).is_err(),
            "the cancelled reader must report the cancellation"
        );

        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let mut buf = [0u8; 8];
            let _ = done_tx.send(second_reader.read(&mut buf));
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while second_cancel.pending_read_count() == 0 {
            assert!(
                std::time::Instant::now() < deadline,
                "the untouched reader never issued its read"
            );
            std::thread::yield_now();
        }
        // Its pipe holds nothing and it was never cancelled, so it can only still be waiting.
        assert!(
            done_rx.try_recv().is_err(),
            "cancelling one pipe ended another pipe's read"
        );

        second_cancel.request();
        let outcome = done_rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("its own cancellation must end its read");
        assert!(
            outcome.is_err(),
            "the second reader reports its own cancellation as an error"
        );
        worker.join().expect("the reader thread ends");
    }
}

impl SlavePty for ConPtySlavePty {
    fn spawn_command(&self, cmd: CommandBuilder) -> anyhow::Result<Box<dyn Child + Send + Sync>> {
        let inner = self.inner.lock().unwrap();
        let child = inner.con.spawn_command(cmd)?;
        Ok(Box::new(child))
    }
}
