//! Inherited synchronous handles live only in killable relay processes.
//! Parent endpoints are Tokio overlapped pipes; teardown kills once and waits.
use std::{
    future::Future,
    io,
    pin::Pin,
    process::Stdio,
    task::{Context, Poll},
};
use tokio::{
    io::{AsyncReadExt, AsyncWrite, AsyncWriteExt},
    process::{Child, ChildStderr, ChildStdin, Command},
};

const CHUNK: usize = 8192;
type Completion =
    Pin<Box<dyn Future<Output = (io::Result<usize>, ChildStdin, ChildStderr)> + Send>>;
pub struct Output {
    pipes: Option<(ChildStdin, ChildStderr)>,
    pending: Option<Completion>,
}
impl AsyncWrite for Output {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if this.pending.is_none() {
            let (mut input, mut ack) = this
                .pipes
                .take()
                .ok_or_else(|| io::Error::other("relay pipes unavailable"))?;
            let bytes = bytes[..bytes.len().min(CHUNK)].to_vec();
            this.pending = Some(Box::pin(async move {
                let result = async {
                    input.write_all(&(bytes.len() as u32).to_le_bytes()).await?;
                    input.write_all(&bytes).await?;
                    let mut receipt = [0];
                    ack.read_exact(&mut receipt).await?;
                    if receipt != [1] {
                        return Err(io::Error::other("invalid relay receipt"));
                    }
                    Ok(bytes.len())
                }
                .await;
                (result, input, ack)
            }));
        }
        let pending = this
            .pending
            .as_mut()
            .ok_or_else(|| io::Error::other("relay completion unavailable"))?;
        let (result, input, ack) = std::task::ready!(pending.as_mut().poll(cx));
        this.pending = None;
        this.pipes = Some((input, ack));
        Poll::Ready(result)
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        // Each completed write has already been flushed by the relay.
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.poll_flush(cx)
    }
}

pub async fn run(
    backend: std::sync::Arc<dyn crate::agent_server::AgentServerBackend>,
    stop: tokio::sync::watch::Receiver<bool>,
) -> io::Result<()> {
    let executable = std::env::current_exe()?;
    let mut input = Command::new(&executable)
        .arg("--maho-mcp-relay-input")
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()?;
    let output = Command::new(&executable)
        .arg("--maho-mcp-relay-output")
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn();
    let mut output = match output {
        Ok(output) => output,
        Err(error) => {
            terminate(&mut input).await?;
            return Err(error);
        }
    };
    let result = async {
        let reader = input
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("missing input relay pipe"))?;
        let writer = Output {
            pipes: Some((
                output
                    .stdin
                    .take()
                    .ok_or_else(|| io::Error::other("missing output relay pipe"))?,
                output
                    .stderr
                    .take()
                    .ok_or_else(|| io::Error::other("missing output relay receipt"))?,
            )),
            pending: None,
        };
        super::super::run_mcp_io_until(backend, (reader, writer), stop).await
    }
    .await;
    let (input_cleanup, output_cleanup) =
        tokio::join!(terminate(&mut input), terminate(&mut output));
    result.and(input_cleanup).and(output_cleanup)
}

async fn terminate(child: &mut Child) -> io::Result<()> {
    child.kill().await // Tokio issues one termination then awaits/reaps the process.
}
