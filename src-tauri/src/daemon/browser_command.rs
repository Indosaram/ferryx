//! `DaemonRequest::Browser` over the local daemon socket.
//!
//! This is the client half of the seam `agents --host` already uses: the CLI opens the daemon
//! socket, hands the request over, and the daemon decides whether the command runs here or is
//! forwarded to a paired host. No relay client lives in the CLI.

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::daemon::protocol::{DaemonRequest, DaemonResponse, DAEMON_PROTOCOL_VERSION};

/// Runs one browser command through the daemon and returns its answer as printed JSON.
///
/// `host_id` absent runs it on this machine's browser; `Some` forwards it to that paired host.
pub async fn request_browser_daemon(
    browser_id: String,
    command: String,
    params: Option<serde_json::Value>,
    host_id: Option<String>,
) -> Result<String, String> {
    let stream = connect_daemon().await.map_err(|(value, _)| value.to_string())?;
    let (read_half, mut write_half) = tokio::io::split(stream);
    let mut reader = BufReader::new(read_half);

    exchange(
        &mut reader,
        &mut write_half,
        &DaemonRequest::Handshake {
            version: DAEMON_PROTOCOL_VERSION,
            token: crate::daemon::client::read_transport_token(),
        },
    )
    .await?;

    let response = exchange(
        &mut reader,
        &mut write_half,
        &DaemonRequest::Browser {
            browser_id,
            command,
            params,
            host_id,
            document_generation: None,
            browser_instance_id: None,
            desktop_epoch: None,
            lease_epoch: None,
            device_id: None,
            connection_id: None,
        },
    )
    .await?;

    match response {
        DaemonResponse::BrowserOk { result } => {
            let mut value = serde_json::json!({ "ok": true });
            if let (Some(out), serde_json::Value::Object(fields)) =
                (value.as_object_mut(), result)
            {
                out.extend(fields);
            }
            Ok(value.to_string())
        }
        DaemonResponse::Error {
            message,
            code,
            details,
        } => {
            let code = code
                .filter(|code| !code.is_empty())
                .unwrap_or_else(|| "INTERNAL".into());
            Err(serde_json::json!({
                "ok": false,
                "code": code,
                "message": message,
                "details": details,
            })
            .to_string())
        }
        DaemonResponse::ProtocolMismatch { .. } => {
            Err(r#"{"ok":false,"code":"UNSUPPORTED","message":"the daemon speaks another protocol version"}"#.to_string())
        }
        other => Err(serde_json::json!({
            "ok": false,
            "code": "INTERNAL",
            "message": format!("unexpected daemon response: {other:?}"),
        })
        .to_string()),
    }
}

#[cfg(unix)]
async fn connect_daemon() -> Result<tokio::net::UnixStream, (serde_json::Value, i32)> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    let path = crate::daemon::server::get_socket_path();
    let not_running = || {
        (
            serde_json::json!({
                "ok": false,
                "code": "NOT_FOUND",
                "message": format!("no Ferryx daemon is listening at {}", path.display()),
            }),
            3,
        )
    };
    let metadata = std::fs::symlink_metadata(&path).map_err(|_| not_running())?;
    // SAFETY: getuid takes no arguments, dereferences nothing and cannot fail.
    let uid = unsafe { libc::getuid() };
    if !metadata.file_type().is_socket() || metadata.uid() != uid {
        return Err((
            serde_json::json!({
                "ok": false,
                "code": "FORBIDDEN",
                "message": "the daemon socket is not a socket owned by this user",
            }),
            4,
        ));
    }
    tokio::net::UnixStream::connect(&path)
        .await
        .map_err(|_| not_running())
}

#[cfg(not(unix))]
async fn connect_daemon() -> Result<tokio::net::TcpStream, (serde_json::Value, i32)> {
    let path = crate::daemon::server::get_socket_path();
    let not_running = || {
        (
            serde_json::json!({
                "ok": false,
                "code": "NOT_FOUND",
                "message": "no Ferryx daemon is listening",
            }),
            3,
        )
    };
    let port: u16 = tokio::fs::read_to_string(&path)
        .await
        .ok()
        .and_then(|text| text.trim().parse().ok())
        .ok_or_else(not_running)?;
    tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .map_err(|_| not_running())
}

async fn exchange<R, W>(
    reader: &mut BufReader<R>,
    writer: &mut W,
    request: &DaemonRequest,
) -> Result<DaemonResponse, String>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut line = serde_json::to_string(request).map_err(|error| error.to_string())?;
    line.push('\n');
    writer
        .write_all(line.as_bytes())
        .await
        .map_err(|error| error.to_string())?;
    writer.flush().await.map_err(|error| error.to_string())?;
    let mut response = String::new();
    if reader
        .read_line(&mut response)
        .await
        .map_err(|error| error.to_string())?
        == 0
    {
        return Err("the daemon closed the connection".into());
    }
    serde_json::from_str(response.trim())
        .map_err(|error| format!("unreadable daemon response: {error}"))
}
