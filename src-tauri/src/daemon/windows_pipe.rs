//! Owner-only Windows transports. TCP remains available for one compatibility release.
use std::{ffi::c_void, io, os::windows::io::AsRawHandle};
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions};
use windows_sys::Win32::{
    Foundation::{CloseHandle, LocalFree, HANDLE},
    Security::{Authorization::{ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1}, GetTokenInformation, TokenUser, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER},
    System::{Pipes::GetNamedPipeClientProcessId, Threading::{GetCurrentProcess, OpenProcessToken}},
};

pub fn user_sid() -> io::Result<String> {
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: token is an out-pointer; the returned handle is closed below.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let result = (|| {
        let mut len = 0;
        unsafe { GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut len) };
        // usize storage supplies TOKEN_USER's alignment, unlike a byte vector.
        let mut buf = vec![0usize; (len as usize).div_ceil(std::mem::size_of::<usize>())];
        if unsafe { GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), len, &mut len) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let user = unsafe { &*buf.as_ptr().cast::<TOKEN_USER>() };
        let mut wide = std::ptr::null_mut();
        if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut wide) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut n = 0;
        while unsafe { *wide.add(n) } != 0 { n += 1; }
        let sid = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(wide, n) });
        unsafe { LocalFree(wide.cast()) };
        Ok(sid)
    })();
    unsafe { CloseHandle(token) };
    result
}

pub fn name(channel: &str) -> io::Result<String> {
    let suffix = if cfg!(debug_assertions) { "-dev" } else { "" };
    Ok(format!(r"\\.\pipe\ferryx-{}-{channel}{suffix}", user_sid()?))
}

pub fn connect(channel: &str) -> io::Result<NamedPipeClient> {
    ClientOptions::new().open(name(channel)?)
}

pub struct Listener {
    name: String,
    pending: NamedPipeServer,
}

fn create(name: &str, first: bool) -> io::Result<NamedPipeServer> {
    let sddl: Vec<u16> = format!("D:P(A;;GA;;;{})\0", user_sid()?).encode_utf16().collect();
    let mut descriptor = std::ptr::null_mut();
    if unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(), SDDL_REVISION_1, &mut descriptor, std::ptr::null_mut()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut attrs = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    // SAFETY: Tokio copies the descriptor during creation; it is freed after the call.
    let result = unsafe { ServerOptions::new().first_pipe_instance(first)
        .reject_remote_clients(true).create_with_security_attributes_raw(name, (&mut attrs as *mut SECURITY_ATTRIBUTES).cast::<c_void>()) };
    unsafe { LocalFree(descriptor) };
    result
}

impl Listener {
    pub fn bind(channel: &str) -> io::Result<Self> {
        let name = name(channel)?;
        let pending = create(&name, true)?;
        Ok(Self { name, pending })
    }

    pub async fn accept(&mut self) -> io::Result<NamedPipeServer> {
        self.pending.connect().await?;
        // Keep an instance alive while creating the successor: never reopen the name to squatters.
        let next = create(&self.name, false)?;
        Ok(std::mem::replace(&mut self.pending, next))
    }
}

pub async fn identity(pipe: &NamedPipeServer) -> (super::supervisor_peer::PeerOrigin, Option<super::supervisor_peer::Ancestry>) {
    let mut pid = 0;
    let ok = unsafe { GetNamedPipeClientProcessId(pipe.as_raw_handle().cast(), &mut pid) } != 0;
    tokio::task::spawn_blocking(move || {
        let origin = super::supervisor_peer::identify(ok.then_some(pid));
        let ancestry = super::supervisor_peer::Ancestry::at_accept(origin, &super::supervisor_peer::SystemProcs);
        (origin, ancestry)
    }).await.unwrap_or((super::supervisor_peer::PeerOrigin::Unknown, None))
}

/// Transport-neutral owned halves for the existing client connection pools.
pub struct Stream(Box<dyn Transport>);
trait Transport: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> Transport for T {}
impl From<tokio::net::TcpStream> for Stream {
    fn from(stream: tokio::net::TcpStream) -> Self { Self(Box::new(stream)) }
}
impl Stream {
    pub async fn connect(address: impl tokio::net::ToSocketAddrs) -> io::Result<Self> {
        tokio::net::TcpStream::connect(address).await.map(Self::from)
    }
    pub fn pipe(channel: &str) -> io::Result<Self> {
        connect(channel).map(|pipe| Self(Box::new(pipe)))
    }
    pub fn into_split(self) -> (tokio::io::ReadHalf<Self>, tokio::io::WriteHalf<Self>) {
        tokio::io::split(self)
    }
}
impl tokio::io::AsyncRead for Stream {
    fn poll_read(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>, buf: &mut tokio::io::ReadBuf<'_>) -> std::task::Poll<io::Result<()>> {
        std::pin::Pin::new(&mut *self.0).poll_read(cx, buf)
    }
}
impl tokio::io::AsyncWrite for Stream {
    fn poll_write(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>, buf: &[u8]) -> std::task::Poll<io::Result<usize>> {
        std::pin::Pin::new(&mut *self.0).poll_write(cx, buf)
    }
    fn poll_flush(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<io::Result<()>> {
        std::pin::Pin::new(&mut *self.0).poll_flush(cx)
    }
    fn poll_shutdown(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<io::Result<()>> {
        std::pin::Pin::new(&mut *self.0).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn isolated_name() -> String {
        format!(r"\\.\pipe\ferryx-test-{}-{}", std::process::id(), uuid::Uuid::new_v4())
    }

    #[tokio::test]
    async fn pipe_identity_and_cross_pane_write_refusal() {
        let name = isolated_name();
        let mut listener = Listener { pending: create(&name, true).unwrap(), name: name.clone() };
        assert!(create(&name, true).is_err(), "first instance cannot be squatted");
        let mut client = ClientOptions::new().open(&name).unwrap();
        let mut server = listener.accept().await.unwrap();
        let (origin, mut ancestry) = identity(&server).await;
        let pid = std::process::id();
        assert!(matches!(origin, super::super::supervisor_peer::PeerOrigin::Process(peer) if peer.pid == pid));
        let panes = super::super::supervisor_peer::PaneRegistry::default();
        panes.insert("pane-a", pid, &super::super::supervisor_peer::SystemProcs);
        let class = super::super::supervisor_peer::classify(origin, &mut ancestry, &panes,
            &super::super::supervisor_peer::SystemProcs).unwrap();
        let grants = super::super::pane_grants::PaneGrants::load(None);
        let gate = super::super::pane_grants::gate_for(&super::super::protocol::DaemonRequest::Write {
            session_id: "pane-b".into(), data: "x".into(),
        }).unwrap();
        assert_eq!(super::super::pane_grants::decide(&class, &gate, &grants, |_| None)
            .unwrap_err().reason, super::super::pane_grants::PANE_GRANT_REQUIRED);
        assert!(!grants.admin_allowed(&class, |_| None), "control lifetime operations refuse panes");
        client.write_all(b"x").await.unwrap();
        let mut byte = [0];
        tokio::time::timeout(std::time::Duration::from_secs(5), server.read_exact(&mut byte)).await.unwrap().unwrap();
        assert_eq!(byte, [b'x']);
    }

    #[tokio::test]
    async fn tcp_compatibility_stream_still_works() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let connect = Stream::connect(listener.local_addr().unwrap());
        let (client, accepted) = tokio::join!(connect, listener.accept());
        let mut client = client.unwrap();
        let mut server = accepted.unwrap().0;
        client.write_all(b"legacy").await.unwrap();
        let mut bytes = [0; 6];
        tokio::time::timeout(std::time::Duration::from_secs(5), server.read_exact(&mut bytes)).await.unwrap().unwrap();
        assert_eq!(&bytes, b"legacy");
    }

    #[tokio::test]
    async fn pipe_rejects_owner_sid_marked_deny_only() {
        use windows_sys::Win32::Security::{CreateRestrictedToken, GetTokenInformation,
            ImpersonateLoggedOnUser, RevertToSelf, SID_AND_ATTRIBUTES, TOKEN_DUPLICATE, TOKEN_IMPERSONATE};
        let name = isolated_name();
        let _server = create(&name, true).unwrap();
        std::thread::spawn(move || {
            let mut token = std::ptr::null_mut();
            assert_ne!(unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_IMPERSONATE, &mut token) }, 0);
            let mut len = 0;
            unsafe { GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut len) };
            let mut buf = vec![0usize; (len as usize).div_ceil(std::mem::size_of::<usize>())];
            assert_ne!(unsafe { GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), len, &mut len) }, 0);
            let user = unsafe { &*buf.as_ptr().cast::<TOKEN_USER>() };
            let deny = SID_AND_ATTRIBUTES { Sid: user.User.Sid, Attributes: 0 };
            let mut restricted = std::ptr::null_mut();
            assert_ne!(unsafe { CreateRestrictedToken(token, 0, 1, &deny, 0, std::ptr::null(), 0, std::ptr::null(), &mut restricted) }, 0);
            unsafe { CloseHandle(token) };
            assert_ne!(unsafe { ImpersonateLoggedOnUser(restricted) }, 0);
            let result = std::fs::OpenOptions::new().read(true).write(true).open(&name);
            let reverted = unsafe { RevertToSelf() };
            unsafe { CloseHandle(restricted) };
            assert_ne!(reverted, 0);
            assert_eq!(result.unwrap_err().raw_os_error(), Some(5), "ERROR_ACCESS_DENIED");
        }).join().unwrap();
    }
}
