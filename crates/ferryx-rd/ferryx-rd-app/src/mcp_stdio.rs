//! Readiness-based stdio. No Tokio blocking stdin job survives cancellation.
#[cfg(unix)]
mod unix {
    use std::{
        io,
        os::fd::OwnedFd,
        pin::Pin,
        task::{Context, Poll},
    };
    use tokio::io::{unix::AsyncFd, AsyncRead, AsyncWrite, ReadBuf};

    pub struct Stdio(AsyncFd<OwnedFd>);
    impl Stdio {
        pub fn new(fd: impl std::os::fd::AsFd) -> io::Result<Self> {
            let fd = rustix::io::dup(fd)?;
            rustix::io::ioctl_fionbio(&fd, true)?;
            Ok(Self(AsyncFd::new(fd)?))
        }
    }
    impl AsyncRead for Stdio {
        fn poll_read(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            loop {
                let mut ready = std::task::ready!(self.0.poll_read_ready(cx))?;
                match ready.try_io(|fd| {
                    rustix::io::read(fd.get_ref(), buf.initialize_unfilled())
                        .map_err(io::Error::from)
                }) {
                    Ok(Ok(count)) => {
                        buf.advance(count);
                        return Poll::Ready(Ok(()));
                    }
                    Ok(Err(err)) => return Poll::Ready(Err(err)),
                    Err(_) => continue,
                }
            }
        }
    }
    impl AsyncWrite for Stdio {
        fn poll_write(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            loop {
                let mut ready = std::task::ready!(self.0.poll_write_ready(cx))?;
                match ready
                    .try_io(|fd| rustix::io::write(fd.get_ref(), buf).map_err(io::Error::from))
                {
                    Ok(result) => return Poll::Ready(result),
                    Err(_) => continue,
                }
            }
        }
        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
        fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }
}

#[cfg(windows)]
#[path = "mcp_windows.rs"]
mod windows;

#[cfg(all(test, unix))]
mod unix_tests {
    #[tokio::test]
    async fn unix_adapter_roundtrips_and_reads_eof() {
        use std::io::{Read, Write};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        // Given a real Unix stream using the unchanged readiness adapter.
        let (socket, mut peer) = std::os::unix::net::UnixStream::pair().unwrap();
        let mut adapter = super::unix::Stdio::new(socket).unwrap();
        peer.write_all(b"request").unwrap();
        peer.shutdown(std::net::Shutdown::Write).unwrap();
        // When reading through EOF and writing the response.
        let mut request = Vec::new();
        adapter.read_to_end(&mut request).await.unwrap();
        adapter.write_all(b"response").await.unwrap();
        let mut response = [0; 8];
        peer.read_exact(&mut response).unwrap();
        // Then both directions preserve bytes and read terminates at EOF.
        assert_eq!(request, b"request");
        assert_eq!(&response, b"response");
    }
}

pub async fn run(
    backend: std::sync::Arc<dyn crate::agent_server::AgentServerBackend>,
    stop: tokio::sync::watch::Receiver<bool>,
) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let stdin = unix::Stdio::new(std::io::stdin())?;
        let stdout = unix::Stdio::new(std::io::stdout())?;
        super::run_mcp_io_until(backend, (stdin, stdout), stop).await
    }
    #[cfg(windows)]
    {
        windows::run(backend, stop).await
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (backend, stop);
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "cancellable native MCP stdio is not implemented on this platform",
        ))
    }
}
