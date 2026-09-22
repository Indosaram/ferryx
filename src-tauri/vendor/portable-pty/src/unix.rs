//! Working with pseudo-terminals

use crate::{Child, CommandBuilder, MasterPty, PtyPair, PtySize, PtySystem, SlavePty};
use anyhow::{bail, Error};
use filedescriptor::FileDescriptor;
use libc::{self, winsize};
use std::cell::RefCell;
use std::ffi::OsStr;
use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::{AsRawFd, FromRawFd, IntoRawFd};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::{io, mem, ptr};

pub use std::os::unix::io::{OwnedFd, RawFd};

#[derive(Default)]
pub struct UnixPtySystem {}

fn openpty(size: PtySize) -> anyhow::Result<(UnixMasterPty, UnixSlavePty)> {
    let mut master: RawFd = -1;
    let mut slave: RawFd = -1;

    let mut size = winsize {
        ws_row: size.rows,
        ws_col: size.cols,
        ws_xpixel: size.pixel_width,
        ws_ypixel: size.pixel_height,
    };

    let result = unsafe {
        // BSDish systems may require mut pointers to some args
        #[allow(clippy::unnecessary_mut_passed)]
        libc::openpty(
            &mut master,
            &mut slave,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut size,
        )
    };

    if result != 0 {
        bail!("failed to openpty: {:?}", io::Error::last_os_error());
    }

    let tty_name = tty_name(slave);

    let master = UnixMasterPty {
        fd: PtyFd(unsafe { FileDescriptor::from_raw_fd(master) }),
        took_writer: RefCell::new(false),
        tty_name,
    };
    let slave = UnixSlavePty {
        fd: PtyFd(unsafe { FileDescriptor::from_raw_fd(slave) }),
    };

    // Ensure that these descriptors will get closed when we execute
    // the child process.  This is done after constructing the Pty
    // instances so that we ensure that the Ptys get drop()'d if
    // the cloexec() functions fail (unlikely!).
    cloexec(master.fd.as_raw_fd())?;
    cloexec(slave.fd.as_raw_fd())?;

    Ok((master, slave))
}

impl PtySystem for UnixPtySystem {
    fn openpty(&self, size: PtySize) -> anyhow::Result<PtyPair> {
        let (master, slave) = openpty(size)?;
        Ok(PtyPair {
            master: Box::new(master),
            slave: Box::new(slave),
        })
    }
}

struct PtyFd(pub FileDescriptor);
impl std::ops::Deref for PtyFd {
    type Target = FileDescriptor;
    fn deref(&self) -> &FileDescriptor {
        &self.0
    }
}
impl std::ops::DerefMut for PtyFd {
    fn deref_mut(&mut self) -> &mut FileDescriptor {
        &mut self.0
    }
}

impl Read for PtyFd {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, io::Error> {
        match self.0.read(buf) {
            Err(ref e) if e.raw_os_error() == Some(libc::EIO) => {
                // EIO indicates that the slave pty has been closed.
                // Treat this as EOF so that std::io::Read::read_to_string
                // and similar functions gracefully terminate when they
                // encounter this condition
                Ok(0)
            }
            x => x,
        }
    }
}

impl Write for PtyFd {
    fn write(&mut self, buf: &[u8]) -> Result<usize, io::Error> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> Result<(), io::Error> {
        self.0.flush()
    }
}

fn tty_name(fd: RawFd) -> Option<PathBuf> {
    let mut buf = vec![0 as std::ffi::c_char; 128];

    loop {
        let res = unsafe { libc::ttyname_r(fd, buf.as_mut_ptr(), buf.len()) };

        if res == libc::ERANGE {
            if buf.len() > 64 * 1024 {
                // on macOS, if the buf is "too big", ttyname_r can
                // return ERANGE, even though that is supposed to
                // indicate buf is "too small".
                return None;
            }
            buf.resize(buf.len() * 2, 0 as std::ffi::c_char);
            continue;
        }

        return if res == 0 {
            let cstr = unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) };
            let osstr = OsStr::from_bytes(cstr.to_bytes());
            Some(PathBuf::from(osstr))
        } else {
            None
        };
    }
}

/// On Big Sur, Cocoa leaks various file descriptors to child processes,
/// so we need to make a pass through the open descriptors beyond just the
/// stdio descriptors and close them all out.
/// This is approximately equivalent to the darwin `posix_spawnattr_setflags`
/// option POSIX_SPAWN_CLOEXEC_DEFAULT which is used as a bit of a cheat
/// on macOS.
/// On Linux, gnome/mutter leak shell extension fds to wezterm too, so we
/// also need to make an effort to clean up the mess.
///
/// This function enumerates the open filedescriptors in the current process
/// and then will forcibly call close(2) on each open fd that is numbered
/// 3 or higher, effectively closing all descriptors except for the stdio
/// streams.
///
/// The implementation of this function relies on `/dev/fd` being available
/// to provide the list of open fds.  Any errors in enumerating or closing
/// the fds are silently ignored.
pub fn close_random_fds() {
    // FreeBSD, macOS and presumably other BSDish systems have /dev/fd as
    // a directory listing the current fd numbers for the process.
    //
    // On Linux, /dev/fd is a symlink to /proc/self/fd
    if let Ok(dir) = std::fs::read_dir("/dev/fd") {
        let mut fds = vec![];
        for entry in dir {
            if let Some(num) = entry
                .ok()
                .map(|e| e.file_name())
                .and_then(|s| s.into_string().ok())
                .and_then(|n| n.parse::<libc::c_int>().ok())
            {
                if num > 2 {
                    fds.push(num);
                }
            }
        }
        for fd in fds {
            unsafe {
                libc::close(fd);
            }
        }
    }
}

impl PtyFd {
    fn resize(&self, size: PtySize) -> Result<(), Error> {
        let ws_size = winsize {
            ws_row: size.rows,
            ws_col: size.cols,
            ws_xpixel: size.pixel_width,
            ws_ypixel: size.pixel_height,
        };

        if unsafe {
            libc::ioctl(
                self.0.as_raw_fd(),
                libc::TIOCSWINSZ as _,
                &ws_size as *const _,
            )
        } != 0
        {
            bail!(
                "failed to ioctl(TIOCSWINSZ): {:?}",
                io::Error::last_os_error()
            );
        }

        Ok(())
    }

    fn get_size(&self) -> Result<PtySize, Error> {
        let mut size: winsize = unsafe { mem::zeroed() };
        if unsafe {
            libc::ioctl(
                self.0.as_raw_fd(),
                libc::TIOCGWINSZ as _,
                &mut size as *mut _,
            )
        } != 0
        {
            bail!(
                "failed to ioctl(TIOCGWINSZ): {:?}",
                io::Error::last_os_error()
            );
        }
        Ok(PtySize {
            rows: size.ws_row,
            cols: size.ws_col,
            pixel_width: size.ws_xpixel,
            pixel_height: size.ws_ypixel,
        })
    }

    fn spawn_command(&self, builder: CommandBuilder) -> anyhow::Result<std::process::Child> {
        let configured_umask = builder.umask;

        let mut cmd = builder.as_command()?;
        let controlling_tty = builder.get_controlling_tty();

        unsafe {
            cmd.stdin(self.as_stdio()?)
                .stdout(self.as_stdio()?)
                .stderr(self.as_stdio()?)
                .pre_exec(move || {
                    // Clean up a few things before we exec the program
                    // Clear out any potentially problematic signal
                    // dispositions that we might have inherited
                    for signo in &[
                        libc::SIGCHLD,
                        libc::SIGHUP,
                        libc::SIGINT,
                        libc::SIGQUIT,
                        libc::SIGTERM,
                        libc::SIGALRM,
                    ] {
                        libc::signal(*signo, libc::SIG_DFL);
                    }

                    let empty_set: libc::sigset_t = std::mem::zeroed();
                    libc::sigprocmask(libc::SIG_SETMASK, &empty_set, std::ptr::null_mut());

                    // Establish ourselves as a session leader.
                    if libc::setsid() == -1 {
                        return Err(io::Error::last_os_error());
                    }

                    // Clippy wants us to explicitly cast TIOCSCTTY using
                    // type::from(), but the size and potentially signedness
                    // are system dependent, which is why we're using `as _`.
                    // Suppress this lint for this section of code.
                    #[allow(clippy::cast_lossless)]
                    if controlling_tty {
                        // Set the pty as the controlling terminal.
                        // Failure to do this means that delivery of
                        // SIGWINCH won't happen when we resize the
                        // terminal, among other undesirable effects.
                        if libc::ioctl(0, libc::TIOCSCTTY as _, 0) == -1 {
                            return Err(io::Error::last_os_error());
                        }
                    }

                    close_random_fds();

                    if let Some(mask) = configured_umask {
                        libc::umask(mask);
                    }

                    Ok(())
                })
        };

        let mut child = cmd.spawn()?;

        // Ensure that we close out the slave fds that Child retains;
        // they are not what we need (we need the master side to reference
        // them) and won't work in the usual way anyway.
        // In practice these are None, but it seems best to be move them
        // out in case the behavior of Command changes in the future.
        child.stdin.take();
        child.stdout.take();
        child.stderr.take();

        Ok(child)
    }
}

/// Represents the master end of a pty.
/// The file descriptor will be closed when the Pty is dropped.
pub struct UnixMasterPty {
    fd: PtyFd,
    took_writer: RefCell<bool>,
    tty_name: Option<PathBuf>,
}

impl UnixMasterPty {
    /// Constructs a `UnixMasterPty` directly from an owned file descriptor and optional slave `tty_name`.
    ///
    /// Takes ownership of the descriptor, validates PTY semantics via `libc::isatty`,
    /// sets the `FD_CLOEXEC` flag, and wraps the descriptor for master PTY operations.
    pub fn new(fd: OwnedFd, tty_name: Option<PathBuf>) -> Result<Self, Error> {
        let raw = fd.as_raw_fd();

        // Validate PTY semantics: must be an open terminal descriptor.
        if unsafe { libc::isatty(raw) } != 1 {
            bail!(
                "file descriptor {} is not a tty: {:?}",
                raw,
                io::Error::last_os_error()
            );
        }

        // Ensure close-on-exec is set.
        cloexec(raw)?;

        // Transfer descriptor ownership to FileDescriptor.
        let raw = fd.into_raw_fd();
        let file_desc = unsafe { FileDescriptor::from_raw_fd(raw) };

        Ok(Self {
            fd: PtyFd(file_desc),
            took_writer: RefCell::new(false),
            tty_name,
        })
    }

    /// Reconstructs a master PTY trait object from an owned file descriptor.
    ///
    /// Takes ownership, validates PTY semantics (`libc::isatty`), sets `FD_CLOEXEC`,
    /// and implements the existing `MasterPty` trait by reusing `UnixMasterPty` internals.
    pub fn from_owned_fd(fd: OwnedFd) -> Result<Box<dyn MasterPty + Send>, Error> {
        Self::from_owned_fd_with_tty_name(fd, None)
    }

    /// Reconstructs a master PTY trait object from an owned file descriptor and optional slave `tty_name`.
    ///
    /// Takes ownership, validates PTY semantics (`libc::isatty`), sets `FD_CLOEXEC`,
    /// and implements the existing `MasterPty` trait by reusing `UnixMasterPty` internals.
    pub fn from_owned_fd_with_tty_name(
        fd: OwnedFd,
        tty_name: Option<PathBuf>,
    ) -> Result<Box<dyn MasterPty + Send>, Error> {
        let master = Self::new(fd, tty_name)?;
        Ok(Box::new(master))
    }

    /// Reconstructs a master PTY trait object from a raw file descriptor.
    ///
    /// # Safety
    /// Caller must ensure `fd` is a valid, open descriptor and transfers ownership.
    pub unsafe fn from_raw_fd(fd: RawFd) -> Result<Box<dyn MasterPty + Send>, Error> {
        let owned = OwnedFd::from_raw_fd(fd);
        Self::from_owned_fd(owned)
    }
}

/// Convenience constructor to reconstruct a master PTY from an owned file descriptor
/// and optional slave `tty_name` path (design doc section 11 'PTY master adoption').
pub fn master_from_owned_fd(
    fd: OwnedFd,
    tty_name: Option<PathBuf>,
) -> Result<Box<dyn MasterPty + Send>, Error> {
    UnixMasterPty::from_owned_fd_with_tty_name(fd, tty_name)
}

/// Represents the slave end of a pty.
/// The file descriptor will be closed when the Pty is dropped.
pub struct UnixSlavePty {
    fd: PtyFd,
}

impl UnixSlavePty {
    /// Obtain a readable handle to the slave end.
    pub fn try_clone_reader(&self) -> Result<Box<dyn Read + Send>, Error> {
        let fd = PtyFd(self.fd.try_clone()?);
        Ok(Box::new(fd))
    }

    /// Obtain a writable handle to the slave end.
    pub fn try_clone_writer(&self) -> Result<Box<dyn Write + Send>, Error> {
        let fd = PtyFd(self.fd.try_clone()?);
        Ok(Box::new(fd))
    }

    /// Returns the raw file descriptor for the slave end.
    pub fn as_raw_fd(&self) -> RawFd {
        self.fd.0.as_raw_fd()
    }
}

impl Read for UnixSlavePty {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.fd.read(buf)
    }
}

impl Write for UnixSlavePty {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.fd.0.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.fd.0.flush()
    }
}

/// Helper function to set the close-on-exec flag for a raw descriptor
fn cloexec(fd: RawFd) -> Result<(), Error> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags == -1 {
        bail!(
            "fcntl to read flags failed: {:?}",
            io::Error::last_os_error()
        );
    }
    let result = unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) };
    if result == -1 {
        bail!(
            "fcntl to set CLOEXEC failed: {:?}",
            io::Error::last_os_error()
        );
    }
    Ok(())
}

impl SlavePty for UnixSlavePty {
    fn spawn_command(
        &self,
        builder: CommandBuilder,
    ) -> Result<Box<dyn Child + Send + Sync>, Error> {
        Ok(Box::new(self.fd.spawn_command(builder)?))
    }

    #[cfg(unix)]
    fn as_raw_fd(&self) -> Option<RawFd> {
        Some(self.fd.0.as_raw_fd())
    }
}

impl MasterPty for UnixMasterPty {
    fn resize(&self, size: PtySize) -> Result<(), Error> {
        self.fd.resize(size)
    }

    fn get_size(&self) -> Result<PtySize, Error> {
        self.fd.get_size()
    }

    fn try_clone_reader(&self) -> Result<Box<dyn Read + Send>, Error> {
        let fd = PtyFd(self.fd.try_clone()?);
        Ok(Box::new(fd))
    }

    fn take_writer(&self) -> Result<Box<dyn Write + Send>, Error> {
        if *self.took_writer.borrow() {
            anyhow::bail!("cannot take writer more than once");
        }
        *self.took_writer.borrow_mut() = true;
        let fd = PtyFd(self.fd.try_clone()?);
        Ok(Box::new(UnixMasterWriter { fd }))
    }

    fn as_raw_fd(&self) -> Option<RawFd> {
        Some(self.fd.0.as_raw_fd())
    }

    fn tty_name(&self) -> Option<PathBuf> {
        self.tty_name.clone()
    }

    fn process_group_leader(&self) -> Option<libc::pid_t> {
        match unsafe { libc::tcgetpgrp(self.fd.0.as_raw_fd()) } {
            pid if pid > 0 => Some(pid),
            _ => None,
        }
    }

    fn get_termios(&self) -> Option<nix::sys::termios::Termios> {
        nix::sys::termios::tcgetattr(self.fd.0.as_fd()).ok()
    }
}

/// Represents the master end of a pty.
/// EOT will be sent, and then the file descriptor will be closed when
/// the Pty is dropped.
struct UnixMasterWriter {
    fd: PtyFd,
}

impl Drop for UnixMasterWriter {
    fn drop(&mut self) {
        let mut t: libc::termios = unsafe { std::mem::MaybeUninit::zeroed().assume_init() };
        if unsafe { libc::tcgetattr(self.fd.0.as_raw_fd(), &mut t) } == 0 {
            // EOF is only interpreted after a newline, so if it is set,
            // we send a newline followed by EOF.
            let eot = t.c_cc[libc::VEOF];
            if eot != 0 {
                let _ = self.fd.0.write_all(&[b'\n', eot]);
            }
        }
    }
}

impl Write for UnixMasterWriter {
    fn write(&mut self, buf: &[u8]) -> Result<usize, io::Error> {
        self.fd.write(buf)
    }
    fn flush(&mut self) -> Result<(), io::Error> {
        self.fd.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn test_master_from_owned_fd_roundtrip_and_resize() -> Result<(), Error> {
        let pty_system = UnixPtySystem::default();
        let initial_size = PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        };

        // 1. Open a real PTY via the crate's existing openpty path.
        let pair = pty_system.openpty(initial_size)?;
        let master_raw = pair
            .master
            .as_raw_fd()
            .expect("master raw fd should be available");
        let slave_raw = pair
            .slave
            .as_raw_fd()
            .expect("slave raw fd should be available");
        let tty_name = pair.master.tty_name();

        // 2. Put the slave in raw mode so that bytes roundtrip without echo or line editing.
        let mut termios: libc::termios = unsafe { mem::zeroed() };
        let rc = unsafe { libc::tcgetattr(slave_raw, &mut termios) };
        assert_eq!(rc, 0, "tcgetattr on slave failed");
        unsafe { libc::cfmakeraw(&mut termios) };
        let rc = unsafe { libc::tcsetattr(slave_raw, libc::TCSANOW, &termios) };
        assert_eq!(rc, 0, "tcsetattr on slave failed");

        // 3. dup() the master fd into an OwnedFd.
        let dup_fd = unsafe { libc::dup(master_raw) };
        assert!(
            dup_fd >= 0,
            "dup failed: {:?}",
            io::Error::last_os_error()
        );

        // Verify that plain dup() does not have FD_CLOEXEC set.
        let initial_flags = unsafe { libc::fcntl(dup_fd, libc::F_GETFD) };
        assert!(initial_flags >= 0);
        assert_eq!(
            initial_flags & libc::FD_CLOEXEC,
            0,
            "plain dup() should not have FD_CLOEXEC"
        );

        let owned_fd = unsafe { OwnedFd::from_raw_fd(dup_fd) };

        // Drop the original master to prove the adopted master operates independently.
        drop(pair.master);

        // 4. Construct a new master from the dup'd OwnedFd.
        let adopted_master =
            UnixMasterPty::from_owned_fd_with_tty_name(owned_fd, tty_name.clone())?;

        // Verify FD_CLOEXEC was set by the constructor.
        let adopted_fd = adopted_master
            .as_raw_fd()
            .expect("adopted master as_raw_fd");
        let adopted_flags = unsafe { libc::fcntl(adopted_fd, libc::F_GETFD) };
        assert!(adopted_flags >= 0);
        assert_ne!(
            adopted_flags & libc::FD_CLOEXEC,
            0,
            "adopted master descriptor must have FD_CLOEXEC set"
        );

        // Verify tty_name matches if passed.
        assert_eq!(adopted_master.tty_name(), tty_name);

        // 5. Assert write -> read roundtrip through the slave.
        // Step 5a: Write from adopted master -> read from slave.
        let test_payload_m2s = b"hello slave from adopted master";
        let mut master_writer = adopted_master.take_writer()?;
        master_writer.write_all(test_payload_m2s)?;
        master_writer.flush()?;

        let mut slave_buf = vec![0u8; test_payload_m2s.len()];
        let mut bytes_read = 0;
        while bytes_read < slave_buf.len() {
            let n = unsafe {
                libc::read(
                    slave_raw,
                    slave_buf[bytes_read..].as_mut_ptr() as *mut libc::c_void,
                    slave_buf.len() - bytes_read,
                )
            };
            assert!(n > 0, "read from slave failed or returned EOF");
            bytes_read += n as usize;
        }
        assert_eq!(&slave_buf, test_payload_m2s);

        // Drop master writer after slave has read the payload
        drop(master_writer);

        // Step 5b: Write from slave -> read from adopted master.
        let test_payload_s2m = b"hello adopted master from slave";
        let n_written = unsafe {
            libc::write(
                slave_raw,
                test_payload_s2m.as_ptr() as *const libc::c_void,
                test_payload_s2m.len(),
            )
        };
        assert_eq!(n_written as usize, test_payload_s2m.len());

        let mut master_reader = adopted_master.try_clone_reader()?;
        let mut master_buf = vec![0u8; test_payload_s2m.len()];
        master_reader.read_exact(&mut master_buf)?;
        assert_eq!(&master_buf, test_payload_s2m);

        // 6. Assert resize and get_size on the adopted master.
        let current_size = adopted_master.get_size()?;
        assert_eq!(current_size.rows, 24);
        assert_eq!(current_size.cols, 80);

        let new_size = PtySize {
            rows: 42,
            cols: 132,
            pixel_width: 0,
            pixel_height: 0,
        };
        adopted_master.resize(new_size)?;

        let updated_size = adopted_master.get_size()?;
        assert_eq!(updated_size.rows, 42);
        assert_eq!(updated_size.cols, 132);

        Ok(())
    }

    #[test]
    fn test_from_owned_fd_roundtrip_with_unix_slave_pty() -> Result<(), Error> {
        let (orig_master, slave) = openpty(PtySize {
            rows: 25,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        let master_raw = orig_master.as_raw_fd().unwrap();

        // Put slave in raw mode
        let slave_raw = slave.as_raw_fd();
        let mut termios: libc::termios = unsafe { mem::zeroed() };
        unsafe {
            assert_eq!(libc::tcgetattr(slave_raw, &mut termios), 0);
            libc::cfmakeraw(&mut termios);
            assert_eq!(libc::tcsetattr(slave_raw, libc::TCSANOW, &termios), 0);
        }

        let dup_fd = unsafe { libc::dup(master_raw) };
        assert!(dup_fd >= 0);
        let owned_fd = unsafe { OwnedFd::from_raw_fd(dup_fd) };
        drop(orig_master);

        let adopted = UnixMasterPty::from_owned_fd(owned_fd)?;

        // Write from adopted master, read via slave's try_clone_reader
        let mut master_writer = adopted.take_writer()?;
        master_writer.write_all(b"roundtrip_direct")?;
        master_writer.flush()?;

        let mut slave_reader = slave.try_clone_reader()?;
        let mut buf = [0u8; 16];
        slave_reader.read_exact(&mut buf)?;
        assert_eq!(&buf, b"roundtrip_direct");

        drop(master_writer);

        // Write via slave's try_clone_writer, read from adopted master
        let mut slave_writer = slave.try_clone_writer()?;
        slave_writer.write_all(b"roundtrip_reply!")?;
        slave_writer.flush()?;

        let mut master_reader = adopted.try_clone_reader()?;
        let mut reply_buf = [0u8; 16];
        master_reader.read_exact(&mut reply_buf)?;
        assert_eq!(&reply_buf, b"roundtrip_reply!");

        // Resize and get_size check
        let resize_target = PtySize {
            rows: 35,
            cols: 95,
            pixel_width: 0,
            pixel_height: 0,
        };
        adopted.resize(resize_target)?;
        let got_size = adopted.get_size()?;
        assert_eq!(got_size.rows, 35);
        assert_eq!(got_size.cols, 95);

        Ok(())
    }

    #[test]
    fn test_master_from_owned_fd_function() -> Result<(), Error> {
        let (orig_master, orig_slave) = openpty(PtySize::default())?;
        let master_raw = orig_master.as_raw_fd().unwrap();
        let tty_name = orig_master.tty_name();

        let dup_fd = unsafe { libc::dup(master_raw) };
        assert!(dup_fd >= 0);
        let owned_fd = unsafe { OwnedFd::from_raw_fd(dup_fd) };
        drop(orig_master);

        let adopted = master_from_owned_fd(owned_fd, tty_name.clone())?;
        assert_eq!(adopted.tty_name(), tty_name);
        assert_eq!(adopted.get_size()?, PtySize::default());

        drop(orig_slave);
        Ok(())
    }

    #[test]
    fn test_from_owned_fd_rejects_non_tty() {
        let mut pipe_fds = [-1; 2];
        let rc = unsafe { libc::pipe(pipe_fds.as_mut_ptr()) };
        assert_eq!(rc, 0);

        let pipe_read = unsafe { OwnedFd::from_raw_fd(pipe_fds[0]) };
        let pipe_write = unsafe { OwnedFd::from_raw_fd(pipe_fds[1]) };

        let result = UnixMasterPty::from_owned_fd(pipe_read);
        assert!(
            result.is_err(),
            "pipe descriptor must not be accepted as a PTY master"
        );

        drop(pipe_write);
    }

    #[test]
    fn test_from_raw_fd_unsafe() -> Result<(), Error> {
        let (orig_master, orig_slave) = openpty(PtySize::default())?;
        let master_raw = orig_master.as_raw_fd().unwrap();

        let dup_fd = unsafe { libc::dup(master_raw) };
        assert!(dup_fd >= 0);
        drop(orig_master);

        let adopted = unsafe { UnixMasterPty::from_raw_fd(dup_fd)? };
        assert_eq!(adopted.get_size()?, PtySize::default());

        drop(orig_slave);
        Ok(())
    }
}
