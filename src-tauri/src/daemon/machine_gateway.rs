//! One authenticated daemon connection carries the original owner's HTTP gateway.
use std::{
    io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
#[cfg(test)]
#[path = "machine_gateway_tests.rs"]
mod tests;

struct Connection<S> {
    stream: S,
    closed: tokio::sync::watch::Sender<bool>,
    read_deadline: Option<Pin<Box<tokio::time::Sleep>>>,
    write_deadline: Option<Pin<Box<tokio::time::Sleep>>>,
}
fn progress<T>(
    result: Poll<io::Result<T>>,
    timer: &mut Option<Pin<Box<tokio::time::Sleep>>>,
    cx: &mut Context<'_>,
) -> Poll<io::Result<T>> {
    use std::future::Future;
    match result {
        Poll::Ready(result) => {
            *timer = None;
            Poll::Ready(result)
        }
        Poll::Pending => {
            let deadline = timer.get_or_insert_with(|| {
                Box::pin(tokio::time::sleep(std::time::Duration::from_secs(10)))
            });
            match deadline.as_mut().poll(cx) {
                Poll::Ready(()) => Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "machine gateway I/O deadline",
                ))),
                Poll::Pending => Poll::Pending,
            }
        }
    }
}
impl<S> Drop for Connection<S> {
    fn drop(&mut self) {
        self.closed.send_replace(true);
    }
}
impl<S: AsyncRead + Unpin> AsyncRead for Connection<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let result = Pin::new(&mut self.stream).poll_read(cx, buf);
        progress(result, &mut self.read_deadline, cx)
    }
}
impl<S: AsyncWrite + Unpin> AsyncWrite for Connection<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let result = Pin::new(&mut self.stream).poll_write(cx, bytes);
        progress(result, &mut self.write_deadline, cx)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let result = Pin::new(&mut self.stream).poll_flush(cx);
        progress(result, &mut self.write_deadline, cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let result = Pin::new(&mut self.stream).poll_shutdown(cx);
        progress(result, &mut self.write_deadline, cx)
    }
}
struct Listener<S> {
    connection: Option<Connection<S>>,
}
impl<S: AsyncRead + AsyncWrite + Unpin + Send + 'static> axum::serve::Listener for Listener<S> {
    type Io = Connection<S>;
    type Addr = ();
    async fn accept(&mut self) -> (Self::Io, ()) {
        match self.connection.take() {
            Some(stream) => (stream, ()),
            None => std::future::pending().await,
        }
    }
    fn local_addr(&self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) async fn serve<S>(stream: S, state: Arc<crate::remote::state::RemoteGatewayState>)
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let _retirement = match state
        .machine_services
        .as_ref()
        .map(|services| services.sessions.retain_machine_request())
        .transpose()
    {
        Ok(guard) => guard,
        Err(error) => {
            tracing::warn!(%error, "Retiring machine gateway rejected connection");
            return;
        }
    };
    let (closed, mut receiver) = tokio::sync::watch::channel(false);
    let listener = Listener {
        connection: Some(Connection {
            stream,
            closed,
            read_deadline: None,
            write_deadline: None,
        }),
    };
    let router = crate::remote::server::create_remote_router(state);
    if let Err(error) = axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            drop(receiver.wait_for(|closed| *closed).await);
        })
        .await
    {
        tracing::warn!(%error, "Legacy machine gateway connection failed");
    }
}
