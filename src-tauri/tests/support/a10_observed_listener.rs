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

impl axum::serve::Listener for ObservedListener {
    type Io = ObservedIo;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        let (stream, addr) = self.listener.accept().await.expect("owned listener accept");
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
