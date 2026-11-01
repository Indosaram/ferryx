use ferryx_rd_windows_stdio_contract::{agent_server::AgentServerBackend, mcp_server};
use std::{
    io,
    pin::Pin,
    process::Stdio,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader, ReadBuf};

struct Observed<T> {
    inner: T,
    armed: bool,
    written: usize,
}
impl<T: AsyncRead + Unpin> AsyncRead for Observed<T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let result = Pin::new(&mut self.inner).poll_read(cx, buf);
        if result.is_pending() && !self.armed {
            self.armed = true;
            eprintln!("ARMED_READ");
        }
        result
    }
}
impl<T: AsyncWrite + Unpin> AsyncWrite for Observed<T> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let result = Pin::new(&mut self.inner).poll_write(cx, bytes);
        if let Poll::Ready(Ok(count)) = &result {
            self.written += count;
        }
        if result.is_pending() && !self.armed && self.written >= 65536 {
            self.armed = true;
            eprintln!("ARMED_WRITE");
        }
        result
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}
pub async fn run_mcp_io_until(
    backend: Arc<dyn AgentServerBackend>,
    (reader, writer): (impl AsyncRead + Unpin, impl AsyncWrite + Unpin),
    stop: tokio::sync::watch::Receiver<bool>,
) -> io::Result<()> {
    mcp_server::run_mcp_io_until(
        backend,
        (
            Observed {
                inner: reader,
                armed: false,
                written: 0,
            },
            Observed {
                inner: writer,
                armed: false,
                written: 0,
            },
        ),
        stop,
    )
    .await
}

pub async fn cancellation_contract(direction: &str) -> io::Result<()> {
    // Given real inherited pipes whose peers remain owned until AFTER child exit.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let mut child = tokio::process::Command::new(std::env::current_exe()?)
        .args(["cancel-server", &listener.local_addr()?.to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let mut input = child.stdin.take().unwrap();
    let output = child.stdout.take().unwrap();
    let mut events = BufReader::new(child.stderr.take().unwrap()).lines();
    let result = tokio::time::timeout(Duration::from_secs(10), async {
        let (mut control, _) = listener.accept().await?;
        if direction == "WRITE" {
            // An oversized string id makes the real dispatcher write a response
            // larger than the native output pipe while its reader stays undrained.
            let request = serde_json::json!({"jsonrpc":"2.0","method":"initialize","id":"x".repeat(1024*1024)});
            input.write_all(format!("{request}\n").as_bytes()).await?;
        }
        // When the actual adapter returns Pending, not after a guessed delay.
        let armed = format!("ARMED_{direction}");
        loop {
            let event = events.next_line().await?.ok_or_else(|| io::Error::other("missing armed event"))?;
            if event == armed { break; }
        }
        control.write_u8(1).await?;
        assert!(child.wait().await?.success());
        // Relay descendants must close their inherited stderr before EOF.
        while let Some(event) = events.next_line().await? { println!("{event}"); }
        Ok::<_,io::Error>(())
    }).await;
    match result {
        Ok(result) => result?,
        Err(error) => {
            child.kill().await?;
            return Err(io::Error::other(error));
        }
    }
    drop((input, output));
    println!("CANCEL_{direction}_JOIN_GREEN");
    Ok(())
}
