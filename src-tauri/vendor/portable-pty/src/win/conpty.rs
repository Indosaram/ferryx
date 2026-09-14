use crate::cmdbuilder::CommandBuilder;
use crate::win::psuedocon::PsuedoCon;
use crate::{Child, MasterPty, PtyPair, PtySize, PtySystem, SlavePty};
use anyhow::Error;
use filedescriptor::{FileDescriptor, Pipe};
use std::sync::{Arc, Mutex};
use winapi::um::wincon::COORD;

#[derive(Default)]
pub struct ConPtySystem {}

impl PtySystem for ConPtySystem {
    fn openpty(&self, size: PtySize) -> anyhow::Result<PtyPair> {
        let stdin = nonblocking_input_pipe()?;
        let stdout = Pipe::new()?;

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
    readable: FileDescriptor,
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
        Ok(Box::new(self.inner.lock().unwrap().readable.try_clone()?))
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

impl SlavePty for ConPtySlavePty {
    fn spawn_command(&self, cmd: CommandBuilder) -> anyhow::Result<Box<dyn Child + Send + Sync>> {
        let inner = self.inner.lock().unwrap();
        let child = inner.con.spawn_command(cmd)?;
        Ok(Box::new(child))
    }
}
