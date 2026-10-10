use std::{io, net::SocketAddr, pin::Pin, task::{Context, Poll}};
use tokio::{io::{AsyncRead, AsyncWrite, ReadBuf}, net::{TcpListener, TcpStream}, sync::{mpsc, watch}};

#[derive(Clone, Copy, Debug, Default)]
pub struct Progress {
    pub pending: bool,
    pub dropped: bool,
}

pub struct ObservedListener {
    pub listener: TcpListener,
    pub accepted: mpsc::Sender<(SocketAddr, watch::Receiver<Progress>)>,
}

pub struct ObservedIo {
    stream: TcpStream,
    progress: watch::Sender<Progress>,
}

pub fn set_small_buffers(stream: &TcpStream) {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        let fd = stream.as_raw_fd();
        let size: libc::c_int = 4096;
        unsafe {
            let result = libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_SNDBUF,
                &size as *const _ as *const libc::c_void,
                std::mem::size_of_val(&size) as libc::socklen_t,
            );
            assert_eq!(result, 0, "SO_SNDBUF: {}", io::Error::last_os_error());
            let result = libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_RCVBUF,
                &size as *const _ as *const libc::c_void,
                std::mem::size_of_val(&size) as libc::socklen_t,
            );
            assert_eq!(result, 0, "SO_RCVBUF: {}", io::Error::last_os_error());
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawSocket;
        let sock = stream.as_raw_socket();
        let size: i32 = 4096;
        unsafe {
            let result = windows_sys::Win32::Networking::WinSock::setsockopt(
                sock as _,
                windows_sys::Win32::Networking::WinSock::SOL_SOCKET,
                windows_sys::Win32::Networking::WinSock::SO_SNDBUF,
                &size as *const _ as *const _,
                std::mem::size_of_val(&size) as _,
            );
            assert_eq!(result, 0, "SO_SNDBUF: WSA error {}", windows_sys::Win32::Networking::WinSock::WSAGetLastError());
            let result = windows_sys::Win32::Networking::WinSock::setsockopt(
                sock as _,
                windows_sys::Win32::Networking::WinSock::SOL_SOCKET,
                windows_sys::Win32::Networking::WinSock::SO_RCVBUF,
                &size as *const _ as *const _,
                std::mem::size_of_val(&size) as _,
            );
            assert_eq!(result, 0, "SO_RCVBUF: WSA error {}", windows_sys::Win32::Networking::WinSock::WSAGetLastError());
        }
    }
}

impl axum::serve::Listener for ObservedListener {
    type Io = ObservedIo;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        let (stream, addr) = self.listener.accept().await.expect("owned listener accept");
        stream.set_nodelay(true).expect("set accepted TCP_NODELAY");
        set_small_buffers(&stream);
        let (progress, receiver) = watch::channel(Progress::default());
        self.accepted.try_send((addr, receiver)).expect("bounded accepted observations");
        (ObservedIo { stream, progress }, addr)
    }

    fn local_addr(&self) -> io::Result<SocketAddr> { self.listener.local_addr() }
}

impl AsyncRead for ObservedIo {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_read(cx, buf)
    }
}

impl AsyncWrite for ObservedIo {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, bytes: &[u8]) -> Poll<io::Result<usize>> {
        let result = Pin::new(&mut self.stream).poll_write(cx, bytes);
        if result.is_pending() {
            self.progress.send_if_modified(|state| {
                if state.pending { false } else { state.pending = true; true }
            });
        }
        result
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}

impl Drop for ObservedIo {
    fn drop(&mut self) { self.progress.send_modify(|state| state.dropped = true); }
}
