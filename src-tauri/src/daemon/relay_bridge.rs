use crate::daemon::server::get_socket_path;
use std::io;
use std::path::Path;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};

const RELAY_READY_SENTINEL: &str = "FERRYX-DAEMON RELAY v3 READY\n";

#[cfg(unix)]
type DaemonStream = tokio::net::UnixStream;
#[cfg(not(unix))]
type DaemonStream = tokio::net::TcpStream;

#[cfg(unix)]
async fn connect_endpoint(path: &Path) -> io::Result<DaemonStream> {
    DaemonStream::connect(path).await
}

#[cfg(not(unix))]
async fn connect_endpoint(path: &Path) -> io::Result<DaemonStream> {
    let port = tokio::fs::read_to_string(path).await?;
    let port = port
        .trim()
        .parse::<u16>()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    DaemonStream::connect(("127.0.0.1", port)).await
}

async fn relay_lines<R, W>(mut reader: R, mut writer: W) -> io::Result<()>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut frame = Vec::new();
    loop {
        frame.clear();
        let bytes_read = reader.read_until(b'\n', &mut frame).await?;
        if bytes_read == 0 {
            return Ok(());
        }
        writer.write_all(&frame).await?;
        writer.flush().await?;
    }
}

async fn run_with_io<R, W>(path: &Path, stdin: R, mut stdout: W) -> io::Result<()>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    // Readiness is deliberately emitted only after the endpoint connection succeeds.
    let stream = connect_endpoint(path).await?;
    stdout.write_all(RELAY_READY_SENTINEL.as_bytes()).await?;
    stdout.flush().await?;

    let (daemon_reader, daemon_writer) = stream.into_split();
    let to_daemon = relay_lines(stdin, daemon_writer);
    let from_daemon = relay_lines(BufReader::new(daemon_reader), stdout);

    tokio::select! {
        result = to_daemon => result,
        result = from_daemon => result,
    }
}

pub fn run_relay_bridge() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime
        .block_on(run_with_io(
            &get_socket_path(),
            BufReader::new(tokio::io::stdin()),
            tokio::io::stdout(),
        ))
        .map_err(Into::into)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::daemon::protocol::{
        decode_daemon_stream_frame, encode_daemon_stream_frame, DaemonStreamMessage,
    };
    use std::borrow::Cow;
    use tempfile::tempdir;
    use tokio::io::{duplex, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixListener;
    use tokio::time::{timeout, Duration};

    const TEST_TIMEOUT: Duration = Duration::from_secs(2);

    fn output_frame(sequence: u64, data: &'static [u8]) -> String {
        encode_daemon_stream_frame(&DaemonStreamMessage::Output {
            session_id: Cow::Borrowed("remote-session"),
            sequence,
            data: Cow::Borrowed(data),
            metrics_read_unix_micros: None,
        })
        .expect("encode daemon stream frame")
    }

    #[tokio::test]
    async fn bridge_relays_ndjson_frames_in_both_directions_after_one_sentinel() {
        let directory = tempdir().expect("create socket directory");
        let socket_path = directory.path().join("daemon.sock");
        let listener = UnixListener::bind(&socket_path).expect("bind fake daemon");
        let (stdin_bridge, mut stdin_driver) = duplex(4096);
        let (stdout_bridge, stdout_driver) = duplex(4096);
        let mut stdout_lines = BufReader::new(stdout_driver).lines();

        let bridge_path = socket_path.clone();
        let bridge = tokio::spawn(async move {
            run_with_io(&bridge_path, BufReader::new(stdin_bridge), stdout_bridge).await
        });
        let daemon = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept relay bridge");
            let (read_half, mut write_half) = stream.into_split();
            let mut lines = BufReader::new(read_half).lines();
            let inbound = lines
                .next_line()
                .await
                .expect("read inbound frame")
                .expect("inbound frame before EOF");
            let decoded = decode_daemon_stream_frame(&inbound).expect("decode inbound frame");
            assert!(matches!(
                decoded,
                DaemonStreamMessage::Output { sequence: 1, .. }
            ));
            write_half
                .write_all(output_frame(2, b"response").as_bytes())
                .await
                .expect("write reverse frame");
        });

        let sentinel = timeout(TEST_TIMEOUT, stdout_lines.next_line())
            .await
            .expect("sentinel timeout")
            .expect("read sentinel")
            .expect("sentinel before EOF");
        assert_eq!(sentinel, RELAY_READY_SENTINEL.trim_end());

        stdin_driver
            .write_all(output_frame(1, b"request").as_bytes())
            .await
            .expect("write stdin frame");
        let response = timeout(TEST_TIMEOUT, stdout_lines.next_line())
            .await
            .expect("response timeout")
            .expect("read response")
            .expect("response before EOF");
        let decoded = decode_daemon_stream_frame(&response).expect("decode response frame");
        assert!(matches!(
            decoded,
            DaemonStreamMessage::Output { sequence: 2, .. }
        ));

        drop(stdin_driver);
        timeout(TEST_TIMEOUT, daemon)
            .await
            .expect("fake daemon timeout")
            .expect("fake daemon task");
        timeout(TEST_TIMEOUT, bridge)
            .await
            .expect("bridge exit timeout")
            .expect("bridge task")
            .expect("bridge result");
        assert!(stdout_lines
            .next_line()
            .await
            .expect("read final EOF")
            .is_none());
    }

    #[tokio::test]
    async fn endpoint_connect_failure_returns_error_without_sentinel() {
        let directory = tempdir().expect("create socket directory");
        let socket_path = directory.path().join("missing.sock");
        let (stdin_bridge, _stdin_driver) = duplex(64);
        let (stdout_bridge, mut stdout_driver) = duplex(64);

        let result = run_with_io(&socket_path, BufReader::new(stdin_bridge), stdout_bridge).await;
        assert!(result.is_err());

        let mut output = Vec::new();
        stdout_driver
            .read_to_end(&mut output)
            .await
            .expect("read bridge stdout");
        assert!(
            output.is_empty(),
            "failure must not print readiness sentinel"
        );
    }

    #[tokio::test]
    async fn daemon_disconnect_makes_bridge_exit() {
        let directory = tempdir().expect("create socket directory");
        let socket_path = directory.path().join("daemon.sock");
        let listener = UnixListener::bind(&socket_path).expect("bind fake daemon");
        let (stdin_bridge, _stdin_driver) = duplex(64);
        let (stdout_bridge, mut stdout_driver) = duplex(256);

        let daemon = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept relay bridge");
            drop(stream);
        });
        let bridge_path = socket_path.clone();
        let bridge = tokio::spawn(async move {
            run_with_io(&bridge_path, BufReader::new(stdin_bridge), stdout_bridge).await
        });

        let mut sentinel = String::new();
        timeout(TEST_TIMEOUT, stdout_driver.read_to_string(&mut sentinel))
            .await
            .expect("stdout close timeout")
            .expect("read stdout");
        assert_eq!(sentinel, RELAY_READY_SENTINEL);
        timeout(TEST_TIMEOUT, daemon)
            .await
            .expect("fake daemon timeout")
            .expect("fake daemon task");
        timeout(TEST_TIMEOUT, bridge)
            .await
            .expect("bridge exit timeout")
            .expect("bridge task")
            .expect("bridge result");
    }
}
