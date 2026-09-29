//! Named-pipe transport for the session host (design rev3 sections 2 and 3). Windows only.
//!
//! - Server instances: byte mode, `PIPE_REJECT_REMOTE_CLIENTS`, at most 2 instances (active +
//!   standby), the first with `FILE_FLAG_FIRST_PIPE_INSTANCE`, and an explicit protected DACL
//!   granting GENERIC_ALL to the current-user SID and SYSTEM only. A NULL DACL is never used.
//! - Frame reading follows the [FrameDecoder] contract: drain `next_frame` until `None`, then
//!   read at most [MAX_DECODER_PUSH] bytes and `push` them, propagating every error.
//! - [secure_private_dir] gives the registry directory the same owner-only ACL, inheritable by
//!   files and subdirectories. Registry files are written by temp file + rename in that
//!   directory and so inherit it; the call must happen before any write.

use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::path::Path;
use std::ptr;
use std::time::Duration;

use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::windows::named_pipe::{
    ClientOptions, NamedPipeClient, NamedPipeServer, PipeMode, ServerOptions,
};
use tokio::time::Instant;
use windows_sys::Win32::Foundation::{
    LocalFree, ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY, HANDLE, HLOCAL,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
    SetNamedSecurityInfoW, SDDL_REVISION_1, SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::{
    GetSecurityDescriptorDacl, GetTokenInformation, TokenUser, ACL, DACL_SECURITY_INFORMATION,
    PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY,
    TOKEN_USER,
};
use windows_sys::Win32::System::Pipes::{GetNamedPipeClientProcessId, GetNamedPipeServerProcessId};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use super::protocol::{encode_frame, Frame, FrameDecoder, FrameError, MAX_DECODER_PUSH};
use super::registry::PipeProbe;

/// Active + standby (design section 2).
pub const MAX_PIPE_INSTANCES: usize = 2;
const PIPE_BUFFER_BYTES: u32 = 64 * 1024;
const OPEN_RETRY_INTERVAL: Duration = Duration::from_millis(25);

fn os_err(op: &'static str) -> io::Error {
    let error = io::Error::last_os_error();
    io::Error::new(error.kind(), format!("{op} failed: {error}"))
}

/// Frees a LocalAlloc'd buffer returned by an SDDL/SID/security-info call.
struct LocalBox(HLOCAL);

impl Drop for LocalBox {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the pointer came from a Win32 call documented to allocate with LocalAlloc
            // and is freed exactly once.
            unsafe { LocalFree(self.0) };
        }
    }
}

fn wide_z(text: &OsStr) -> io::Result<Vec<u16>> {
    let mut wide: Vec<u16> = text.encode_wide().collect();
    if wide.contains(&0) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL"));
    }
    wide.push(0);
    Ok(wide)
}

/// # Safety
/// `text` must point at a NUL-terminated UTF-16 string.
unsafe fn from_wide_z(text: *const u16) -> String {
    let mut len = 0usize;
    // SAFETY: per the contract the string is NUL-terminated.
    while unsafe { *text.add(len) } != 0 {
        len += 1;
    }
    // SAFETY: len elements were just read and are initialized.
    String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, len) })
}

/// The current process user's SID, as an SDDL string (S-1-5-21-...).
pub fn current_user_sid() -> io::Result<String> {
    let mut token: HANDLE = ptr::null_mut();
    // SAFETY: GetCurrentProcess is a pseudo handle; token is a valid out pointer.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(os_err("OpenProcessToken"));
    }
    // SAFETY: OpenProcessToken succeeded; the token handle is fresh and owned only here.
    let token = unsafe { OwnedHandle::from_raw_handle(token as RawHandle) };
    let mut needed = 0u32;
    // SAFETY: a null buffer of length 0 only reports the required size (the call fails with
    // ERROR_INSUFFICIENT_BUFFER, which is expected; `needed` is checked instead).
    unsafe {
        GetTokenInformation(
            token.as_raw_handle() as HANDLE,
            TokenUser,
            ptr::null_mut(),
            0,
            &mut needed,
        )
    };
    if needed == 0 {
        return Err(os_err("GetTokenInformation(size)"));
    }
    // u64 storage keeps TOKEN_USER (pointer-containing) aligned.
    let mut buffer = vec![0u64; (needed as usize).div_ceil(8)];
    // SAFETY: the buffer holds at least `needed` bytes; needed is its reported length.
    let ok = unsafe {
        GetTokenInformation(
            token.as_raw_handle() as HANDLE,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    };
    if ok == 0 {
        return Err(os_err("GetTokenInformation"));
    }
    // SAFETY: on success the buffer starts with a TOKEN_USER whose Sid points into the buffer,
    // which stays alive until the conversion below returns.
    let sid = unsafe { (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    let mut text: *mut u16 = ptr::null_mut();
    // SAFETY: sid is valid (see above); text is a valid out pointer.
    if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 {
        return Err(os_err("ConvertSidToStringSidW"));
    }
    let _owned = LocalBox(text.cast());
    // SAFETY: ConvertSidToStringSidW returns a NUL-terminated string.
    Ok(unsafe { from_wide_z(text) })
}

/// Protected DACL: GENERIC_ALL for `sid` and SYSTEM; nothing else and nothing inherited from
/// above. `inherit` adds OICI so files and subdirectories created inside inherit the same ACEs.
fn owner_only_sddl(sid: &str, inherit: bool) -> String {
    let flags = if inherit { "OICI" } else { "" };
    format!("D:P(A;{flags};GA;;;{sid})(A;{flags};GA;;;SY)")
}

/// A self-relative security descriptor parsed from SDDL (LocalFree'd on drop).
struct SecurityDescriptor(LocalBox);

impl SecurityDescriptor {
    fn from_sddl(sddl: &str) -> io::Result<Self> {
        let text = wide_z(OsStr::new(sddl))?;
        let mut sd: PSECURITY_DESCRIPTOR = ptr::null_mut();
        // SAFETY: text is NUL-terminated; sd is a valid out pointer; the size pointer may be null.
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                text.as_ptr(),
                SDDL_REVISION_1,
                &mut sd,
                ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(os_err("ConvertStringSecurityDescriptorToSecurityDescriptorW"));
        }
        Ok(Self(LocalBox(sd)))
    }

    fn as_ptr(&self) -> PSECURITY_DESCRIPTOR {
        self.0 .0
    }

    fn dacl(&self) -> io::Result<*mut ACL> {
        let (mut present, mut defaulted) = (0, 0);
        let mut dacl: *mut ACL = ptr::null_mut();
        // SAFETY: the descriptor is valid while self lives; all out pointers are valid.
        let ok = unsafe {
            GetSecurityDescriptorDacl(self.as_ptr(), &mut present, &mut dacl, &mut defaulted)
        };
        if ok == 0 {
            return Err(os_err("GetSecurityDescriptorDacl"));
        }
        if present == 0 || dacl.is_null() {
            return Err(io::Error::other("owner-only descriptor has no DACL"));
        }
        Ok(dacl)
    }
}

/// Creates `dir` (and parents) and replaces its DACL with a protected current-user + SYSTEM ACL
/// that files and subdirectories inherit. SetNamedSecurityInfoW also re-propagates the
/// inheritable ACEs to existing children. Call before writing any spec, record, manifest, epoch
/// or token file into it.
pub fn secure_private_dir(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let descriptor = SecurityDescriptor::from_sddl(&owner_only_sddl(&current_user_sid()?, true))?;
    let dacl = descriptor.dacl()?;
    let name = wide_z(dir.as_os_str())?;
    // SAFETY: name is NUL-terminated; dacl points into descriptor, alive for the call; owner,
    // group and SACL are not being set, so their pointers are null.
    let status = unsafe {
        SetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            dacl,
            ptr::null(),
        )
    };
    if status != 0 {
        let error = io::Error::from_raw_os_error(status as i32);
        return Err(io::Error::new(
            error.kind(),
            format!("SetNamedSecurityInfoW {}: {error}", dir.display()),
        ));
    }
    Ok(())
}

/// Creates one host pipe instance. `first` sets FILE_FLAG_FIRST_PIPE_INSTANCE: it fails if any
/// instance of the name already exists, which is how a squatted name is detected at startup.
/// Later instances (standby slot, re-listen after a disconnect) pass `first = false`.
pub fn create_server_instance(pipe_name: &str, first: bool) -> io::Result<NamedPipeServer> {
    let descriptor = SecurityDescriptor::from_sddl(&owner_only_sddl(&current_user_sid()?, false))?;
    let mut attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.as_ptr(),
        bInheritHandle: 0,
    };
    let mut options = ServerOptions::new();
    options
        .pipe_mode(PipeMode::Byte)
        .first_pipe_instance(first)
        .reject_remote_clients(true)
        .max_instances(MAX_PIPE_INSTANCES)
        .in_buffer_size(PIPE_BUFFER_BYTES)
        .out_buffer_size(PIPE_BUFFER_BYTES);
    // SAFETY: attributes is a valid SECURITY_ATTRIBUTES whose descriptor outlives the call
    // (CreateNamedPipeW copies the security into the kernel object).
    unsafe {
        options.create_with_security_attributes_raw(
            pipe_name,
            std::ptr::addr_of_mut!(attributes).cast(),
        )
    }
}

/// GetNamedPipeClientProcessId on a connected server instance (fence `PeerFacts.client_pid`).
pub fn client_process_id(server: &NamedPipeServer) -> io::Result<u32> {
    let mut pid = 0u32;
    // SAFETY: the pipe handle is live for the borrow; pid is a valid out pointer.
    if unsafe { GetNamedPipeClientProcessId(server.as_raw_handle() as HANDLE, &mut pid) } == 0 {
        return Err(os_err("GetNamedPipeClientProcessId"));
    }
    Ok(pid)
}

/// GetNamedPipeServerProcessId on a connected client (spawn step 3 and reconnect checks).
pub fn server_process_id(client: &NamedPipeClient) -> io::Result<u32> {
    let mut pid = 0u32;
    // SAFETY: the pipe handle is live for the borrow; pid is a valid out pointer.
    if unsafe { GetNamedPipeServerProcessId(client.as_raw_handle() as HANDLE, &mut pid) } == 0 {
        return Err(os_err("GetNamedPipeServerProcessId"));
    }
    Ok(pid)
}

fn is_retryable_open(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error().map(|code| code as u32),
        Some(ERROR_PIPE_BUSY | ERROR_FILE_NOT_FOUND)
    )
}

/// Opens the host pipe, retrying only "not created yet" and "all instances busy" until
/// `deadline` (design section 3: 5 s). The OS offers no awaitable pipe-creation event, so this
/// is a bounded retry. Any other error returns at once. ClientOptions' default SQOS limits the
/// server to identification-level impersonation.
pub async fn open_client(pipe_name: &str, deadline: Instant) -> io::Result<NamedPipeClient> {
    loop {
        match ClientOptions::new().open(pipe_name) {
            Ok(client) => return Ok(client),
            Err(error) if is_retryable_open(&error) => {
                if Instant::now() + OPEN_RETRY_INTERVAL >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        format!("session-host pipe open timed out: {error}"),
                    ));
                }
                tokio::time::sleep(OPEN_RETRY_INTERVAL).await;
            }
            Err(error) => return Err(error),
        }
    }
}

/// One open attempt for `host_liveness` (the pipe half; consulted only when the pid is gone).
/// A successful open is dropped immediately; the host treats it as a connection that never
/// sent Hello.
pub fn probe_pipe(pipe_name: &str) -> PipeProbe {
    match ClientOptions::new().open(pipe_name) {
        Ok(client) => {
            drop(client);
            PipeProbe::Connected
        }
        Err(error) => match error.raw_os_error().map(|code| code as u32) {
            Some(ERROR_FILE_NOT_FOUND) => PipeProbe::NotFound,
            Some(ERROR_PIPE_BUSY) => PipeProbe::Busy,
            Some(code) => PipeProbe::Failed(code as i32),
            None => PipeProbe::Failed(0),
        },
    }
}

#[derive(Debug, Error)]
pub enum FrameReadError {
    #[error("pipe read: {0}")]
    Io(#[from] io::Error),
    #[error("frame: {0}")]
    Frame(#[from] FrameError),
    #[error("pipe closed in the middle of a frame")]
    Truncated,
}

/// Reads frames under the [FrameDecoder] contract. After any error the connection must be
/// dropped (the decoder stays failed). Cancel-safe: a cancelled `read_frame` loses no bytes,
/// because bytes move into the decoder only after a read completes.
pub struct FrameReader<R> {
    reader: R,
    decoder: FrameDecoder,
    chunk: Box<[u8]>,
}

impl<R: AsyncRead + Unpin> FrameReader<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            decoder: FrameDecoder::new(),
            chunk: vec![0u8; MAX_DECODER_PUSH].into_boxed_slice(),
        }
    }

    /// `Ok(None)` on a clean EOF at a frame boundary.
    pub async fn read_frame(&mut self) -> Result<Option<Frame>, FrameReadError> {
        loop {
            // Drain first: push is only legal once next_frame has returned None.
            if let Some(frame) = self.decoder.next_frame()? {
                return Ok(Some(frame));
            }
            let read = self.reader.read(&mut self.chunk).await?;
            if read == 0 {
                return if self.decoder.has_partial_frame() {
                    Err(FrameReadError::Truncated)
                } else {
                    Ok(None)
                };
            }
            self.decoder.push(&self.chunk[..read])?;
        }
    }

    pub fn get_ref(&self) -> &R {
        &self.reader
    }
}

#[derive(Debug, Error)]
pub enum FrameWriteError {
    #[error("pipe write: {0}")]
    Io(#[from] io::Error),
    #[error("frame: {0}")]
    Frame(#[from] FrameError),
}

/// Encodes (enforcing per-kind caps) and writes one whole frame. Not cancel-safe: a cancelled
/// write can leave a partial frame, after which the connection must be dropped.
pub async fn write_frame<W: AsyncWrite + Unpin>(
    writer: &mut W,
    frame: &Frame,
) -> Result<(), FrameWriteError> {
    let bytes = encode_frame(frame)?;
    writer.write_all(&bytes).await?;
    writer.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::session_host::protocol::Epoch;
    use std::future::Future;
    use windows_sys::Win32::Security::Authorization::{
        ConvertSecurityDescriptorToStringSecurityDescriptorW, GetNamedSecurityInfoW,
    };

    const WAIT: Duration = Duration::from_secs(10);

    async fn bounded<T>(what: &str, future: impl Future<Output = T>) -> T {
        tokio::time::timeout(WAIT, future)
            .await
            .unwrap_or_else(|_| panic!("{what} did not finish within {WAIT:?}"))
    }

    fn unique_pipe(tag: &str) -> String {
        format!(
            r"\\.\pipe\ferryx-sh-test-{tag}-{}-{:x}",
            std::process::id(),
            rand::random::<u64>()
        )
    }

    #[tokio::test]
    async fn reader_splits_frames_larger_than_one_push() {
        let (mut tx, rx) = tokio::io::duplex(8 * 1024);
        let frames: Vec<Frame> = (0..3u64)
            .map(|i| Frame::Output {
                sequence: i,
                bytes: vec![i as u8; 100 * 1024],
            })
            .chain(std::iter::once(Frame::Input {
                controller_epoch: Epoch(4),
                bytes: b"ls\r".to_vec(),
            }))
            .collect();
        let expected = frames.clone();
        let writer = tokio::spawn(async move {
            for frame in &frames {
                write_frame(&mut tx, frame).await.unwrap();
            }
        });
        let mut reader = FrameReader::new(rx);
        for want in expected {
            let got = bounded("read_frame", reader.read_frame()).await.unwrap();
            assert_eq!(got, Some(want));
        }
        bounded("writer join", writer).await.unwrap();
        assert!(bounded("read eof", reader.read_frame()).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn reader_reports_truncated_frame() {
        let (mut tx, rx) = tokio::io::duplex(1024);
        let bytes = encode_frame(&Frame::Control(b"{}".to_vec())).unwrap();
        bounded("write", tx.write_all(&bytes[..bytes.len() - 1]))
            .await
            .unwrap();
        drop(tx);
        let mut reader = FrameReader::new(rx);
        assert!(matches!(
            bounded("read_frame", reader.read_frame()).await,
            Err(FrameReadError::Truncated)
        ));
    }

    #[tokio::test]
    async fn first_instance_detects_squatted_name() {
        let name = unique_pipe("squat");
        let _squatter = create_server_instance(&name, true).unwrap();
        assert!(create_server_instance(&name, true).is_err());
        // A non-first second instance is still allowed (standby slot).
        let _standby = create_server_instance(&name, false).unwrap();
    }

    #[tokio::test]
    async fn pipe_peers_report_each_others_pid_and_carry_frames() {
        let name = unique_pipe("pid");
        let server = create_server_instance(&name, true).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut client = bounded("open_client", open_client(&name, deadline))
            .await
            .unwrap();
        bounded("connect", server.connect()).await.unwrap();
        assert_eq!(client_process_id(&server).unwrap(), std::process::id());
        assert_eq!(server_process_id(&client).unwrap(), std::process::id());
        let frame = Frame::Control(br#"{"t":"ping"}"#.to_vec());
        bounded("write_frame", write_frame(&mut client, &frame))
            .await
            .unwrap();
        let mut reader = FrameReader::new(server);
        let got = bounded("read_frame", reader.read_frame()).await.unwrap();
        assert_eq!(got, Some(frame));
    }

    #[test]
    fn probe_of_missing_pipe_is_not_found() {
        assert_eq!(probe_pipe(&unique_pipe("missing")), PipeProbe::NotFound);
    }

    fn dacl_sddl(path: &Path) -> String {
        let name = wide_z(path.as_os_str()).unwrap();
        let mut sd: PSECURITY_DESCRIPTOR = ptr::null_mut();
        // SAFETY: name is NUL-terminated; only the descriptor out pointer is requested.
        let status = unsafe {
            GetNamedSecurityInfoW(
                name.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                &mut sd,
            )
        };
        assert_eq!(status, 0, "GetNamedSecurityInfoW {}", path.display());
        let _sd = LocalBox(sd);
        let mut text: *mut u16 = ptr::null_mut();
        // SAFETY: sd is valid; text is a valid out pointer; the length pointer may be null.
        let ok = unsafe {
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                sd,
                SDDL_REVISION_1,
                DACL_SECURITY_INFORMATION,
                &mut text,
                ptr::null_mut(),
            )
        };
        assert_ne!(ok, 0, "ConvertSecurityDescriptorToStringSecurityDescriptorW");
        let _text = LocalBox(text.cast());
        // SAFETY: the conversion returns a NUL-terminated string.
        unsafe { from_wide_z(text) }
    }

    /// (ace flags, trustee sid) for each ACE of an SDDL DACL; every ACE must be an allow ACE.
    fn aces(sddl: &str) -> Vec<(String, String)> {
        let body = sddl.strip_prefix("D:").expect("DACL SDDL");
        let list = &body[body.find('(').unwrap_or(body.len())..];
        list.trim_start_matches('(')
            .trim_end_matches(')')
            .split(")(")
            .filter(|ace| !ace.is_empty())
            .map(|ace| {
                let parts: Vec<&str> = ace.split(';').collect();
                assert_eq!(parts.len(), 6, "malformed ACE {ace:?} in {sddl}");
                assert_eq!(parts[0], "A", "non-allow ACE {ace:?} in {sddl}");
                (parts[1].to_owned(), parts[5].to_owned())
            })
            .collect()
    }

    /// Only the current user and SYSTEM appear (Windows may split an inheritable ACE into
    /// effective + inherit-only entries); each trustee's ACEs together carry all of
    /// `required_flags`.
    fn assert_owner_only(path: &Path, sid: &str, required_flags: &[&str]) -> String {
        let sddl = dacl_sddl(path);
        let aces = aces(&sddl);
        let mut trustees: Vec<&str> = aces.iter().map(|(_, who)| who.as_str()).collect();
        trustees.sort_unstable();
        trustees.dedup();
        let mut want = vec![sid, "SY"];
        want.sort_unstable();
        assert_eq!(trustees, want, "{}: {sddl}", path.display());
        for who in [sid, "SY"] {
            let flags: String = aces
                .iter()
                .filter(|(_, trustee)| trustee == who)
                .map(|(flags, _)| flags.as_str())
                .collect();
            for flag in required_flags {
                assert!(
                    flags.contains(flag),
                    "{}: ACE for {who} lacks {flag}: {sddl}",
                    path.display()
                );
            }
        }
        sddl
    }

    #[test]
    fn private_dir_and_every_child_are_user_and_system_only() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("session-hosts");
        secure_private_dir(&dir).unwrap();
        let sid = current_user_sid().unwrap();

        let dir_sddl = assert_owner_only(&dir, &sid, &["OI", "CI"]);
        assert!(dir_sddl.starts_with("D:P"), "directory DACL not protected: {dir_sddl}");

        // Plain file created inside.
        let file = dir.join("record.json");
        std::fs::write(&file, b"{}").unwrap();
        assert_owner_only(&file, &sid, &["ID"]);

        // Temp + rename within the directory, as registry::write_json_atomic does.
        let temp = dir.join("spec.json.tmp");
        std::fs::write(&temp, b"{\"token\":\"x\"}").unwrap();
        let spec = dir.join("spec.json");
        std::fs::rename(&temp, &spec).unwrap();
        assert_owner_only(&spec, &sid, &["ID"]);

        // Subdirectory and a file inside it inherit too.
        let sub = dir.join("manifests");
        std::fs::create_dir(&sub).unwrap();
        assert_owner_only(&sub, &sid, &["ID", "OI", "CI"]);
        let nested = sub.join("m.json");
        std::fs::write(&nested, b"{}").unwrap();
        assert_owner_only(&nested, &sid, &["ID"]);

        // Re-securing an existing tree keeps children owner-only.
        secure_private_dir(&dir).unwrap();
        assert_owner_only(&file, &sid, &["ID"]);
    }
}
