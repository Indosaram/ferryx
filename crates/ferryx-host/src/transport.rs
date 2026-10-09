use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fxsh::Uuid;
use tokio::io::{AsyncRead, AsyncWrite};

use crate::conn::{serve_connection, HostInfo};
use crate::host::HostShared;

pub fn uuid_hex(id: &Uuid) -> String {
    id.0.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn default_endpoint(instance: &Uuid) -> io::Result<String> {
    #[cfg(unix)]
    {
        let base = unix::runtime_dir()?;
        Ok(base.join(format!("host-{}.sock", uuid_hex(instance))).to_string_lossy().into_owned())
    }
    #[cfg(windows)]
    {
        Ok(format!(r"\\.\pipe\ferryx-host-{}-{}", windows::user_sid()?, uuid_hex(instance)))
    }
}

pub async fn serve(host: Arc<HostShared>, info: Arc<HostInfo>, endpoint: &str, ready: impl FnOnce()) -> io::Result<()> {
    #[cfg(unix)]
    {
        unix::serve(host, info, Path::new(endpoint), ready).await
    }
    #[cfg(windows)]
    {
        windows::serve(host, info, endpoint, ready).await
    }
}

pub async fn spawn_conn<R, W>(host: Arc<HostShared>, info: Arc<HostInfo>, r: R, w: W)
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    tokio::spawn(async move { serve_connection(host, info, r, w).await });
}

#[cfg(unix)]
pub mod unix {
    use super::*;
    use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
    use tokio::net::UnixListener;

    pub fn runtime_dir() -> io::Result<PathBuf> {
        let root = match std::env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty()) {
            Some(d) if cfg!(target_os = "linux") => PathBuf::from(d),
            _ => std::env::temp_dir(),
        };
        Ok(root.join("ferryx"))
    }

    pub fn prepare_dir(dir: &Path) -> io::Result<()> {
        match std::fs::symlink_metadata(dir) {
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                std::fs::create_dir_all(dir)?;
                std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
            }
            Err(e) => return Err(e),
        }
        let meta = std::fs::symlink_metadata(dir)?;
        let uid = unsafe { libc::geteuid() };
        if meta.file_type().is_symlink() || !meta.is_dir() || meta.uid() != uid || meta.mode() & 0o777 != 0o700 {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, format!("{} must be a real directory owned by the current user with mode 0700", dir.display())));
        }
        Ok(())
    }

    fn peer_is_current_user(stream: &tokio::net::UnixStream) -> bool {
        stream.peer_cred().map(|c| c.uid() == unsafe { libc::geteuid() }).unwrap_or(false)
    }

    pub async fn serve(host: Arc<HostShared>, info: Arc<HostInfo>, path: &Path, ready: impl FnOnce()) -> io::Result<()> {
        let dir = path.parent().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "endpoint has no directory"))?;
        prepare_dir(dir)?;
        if let Ok(meta) = std::fs::symlink_metadata(path) {
            if meta.file_type().is_socket() {
                std::fs::remove_file(path)?;
            } else {
                return Err(io::Error::new(io::ErrorKind::AlreadyExists, "endpoint path exists and is not a socket"));
            }
        }
        let listener = UnixListener::bind(path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        ready();
        loop {
            let (stream, _) = listener.accept().await?;
            if !peer_is_current_user(&stream) {
                continue;
            }
            let (r, w) = stream.into_split();
            spawn_conn(host.clone(), info.clone(), r, w).await;
        }
    }
}

#[cfg(windows)]
pub mod windows {
    use super::*;
    use std::ffi::c_void;
    use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, LocalFree, HANDLE};
    use windows_sys::Win32::Security::Authorization::{ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1};
    use windows_sys::Win32::Security::{GetTokenInformation, TokenUser, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER};
    use windows_sys::Win32::System::Pipes::GetNamedPipeClientProcessId;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION};

    fn sid_of_token(token: HANDLE) -> io::Result<(Vec<u8>, String)> {
        let mut len = 0u32;
        unsafe { GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut len) };
        let mut buf = vec![0u8; len as usize];
        if unsafe { GetTokenInformation(token, TokenUser, buf.as_mut_ptr() as *mut c_void, len, &mut len) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let user = unsafe { &*(buf.as_ptr() as *const TOKEN_USER) };
        let mut wide: *mut u16 = std::ptr::null_mut();
        if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut wide) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut n = 0;
        while unsafe { *wide.add(n) } != 0 {
            n += 1;
        }
        let s = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(wide, n) });
        unsafe { LocalFree(wide as *mut c_void) };
        Ok((buf, s))
    }

    pub fn user_sid() -> io::Result<String> {
        let mut token: HANDLE = std::ptr::null_mut();
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let r = sid_of_token(token).map(|(_, s)| s);
        unsafe { CloseHandle(token) };
        r
    }

    fn client_sid(pipe: &NamedPipeServer) -> Option<String> {
        use std::os::windows::io::AsRawHandle;
        let mut pid = 0u32;
        if unsafe { GetNamedPipeClientProcessId(pipe.as_raw_handle() as HANDLE, &mut pid) } == 0 {
            return None;
        }
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if process.is_null() {
            return None;
        }
        let mut token: HANDLE = std::ptr::null_mut();
        let ok = unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } != 0;
        unsafe { CloseHandle(process) };
        if !ok {
            return None;
        }
        let r = sid_of_token(token).ok().map(|(_, s)| s);
        unsafe { CloseHandle(token) };
        r
    }

    pub async fn serve(host: Arc<HostShared>, info: Arc<HostInfo>, name: &str, ready: impl FnOnce()) -> io::Result<()> {
        let sid = user_sid()?;
        let sddl: Vec<u16> = format!("D:P(A;;GA;;;{sid})\0").encode_utf16().collect();
        let mut descriptor: *mut c_void = std::ptr::null_mut();
        if unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(), SDDL_REVISION_1, &mut descriptor, std::ptr::null_mut()) } == 0 {
            return Err(io::Error::from_raw_os_error(unsafe { GetLastError() } as i32));
        }
        let mut attrs = SECURITY_ATTRIBUTES { nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: descriptor, bInheritHandle: 0 };
        let make = |first: bool| unsafe { ServerOptions::new().first_pipe_instance(first).reject_remote_clients(true).create_with_security_attributes_raw(name, &mut attrs as *mut _ as *mut c_void) };
        let mut server = make(true)?;
        ready();
        loop {
            server.connect().await?;
            let connected = std::mem::replace(&mut server, make(false)?);
            if client_sid(&connected).as_deref() != Some(sid.as_str()) {
                continue;
            }
            let (r, w) = tokio::io::split(connected);
            spawn_conn(host.clone(), info.clone(), r, w).await;
        }
    }
}
