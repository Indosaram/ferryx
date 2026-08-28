use crate::browser::{
    BrowserAutomationRequest, BrowserAutomationSnapshot, BrowserError, BrowserManager,
    BrowserSessionSummary,
};
use crate::ipc::browser::{browser_automation_act, browser_automation_snapshot};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::AppHandle;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "camelCase")]
pub enum BrowserCliRequest {
    List,
    Snapshot { browser_id: String },
    Act { request: BrowserAutomationRequest },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum BrowserCliResponse {
    List {
        sessions: Vec<BrowserSessionSummary>,
    },
    Snapshot {
        snapshot: BrowserAutomationSnapshot,
    },
    Acted,
    Error {
        code: String,
        message: String,
    },
}

#[cfg(unix)]
pub fn browser_cli_socket_path() -> PathBuf {
    crate::daemon::server::get_runtime_dir().join("browser.sock")
}

#[cfg(not(unix))]
pub fn browser_cli_socket_path() -> PathBuf {
    crate::daemon::server::get_runtime_dir().join("browser.port")
}

pub fn write_port_file(path: &Path, port: u16) -> Result<(), BrowserError> {
    if let Some(parent) = path.parent() {
        let file_stem = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("browser_port");
        let temp_path = parent.join(format!(".{file_stem}.tmp.{}", std::process::id()));
        fs::write(&temp_path, port.to_string()).map_err(|error| {
            BrowserError::Internal(format!("Failed to write port file: {error}"))
        })?;

        match fs::symlink_metadata(path) {
            Ok(meta) => {
                if meta.file_type().is_symlink() || meta.is_dir() {
                    let _ = fs::remove_file(&temp_path);
                    return Err(BrowserError::Internal(format!(
                        "Path {} is a directory or symlink, refusing to overwrite",
                        path.display()
                    )));
                }
                if let Err(error) = fs::remove_file(path) {
                    let _ = fs::remove_file(&temp_path);
                    return Err(BrowserError::Internal(format!(
                        "Failed to replace existing port file {}: {error}",
                        path.display()
                    )));
                }
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => {
                let _ = fs::remove_file(&temp_path);
                return Err(BrowserError::Internal(format!(
                    "Failed to inspect existing port file {}: {error}",
                    path.display()
                )));
            }
        }

        if let Err(error) = fs::rename(&temp_path, path) {
            let _ = fs::remove_file(&temp_path);
            fs::write(path, port.to_string()).map_err(|write_err| {
                BrowserError::Internal(format!(
                    "Failed to persist port file: {write_err} (rename error: {error})"
                ))
            })?;
        }
    } else {
        fs::write(path, port.to_string()).map_err(|error| {
            BrowserError::Internal(format!("Failed to write port file: {error}"))
        })?;
    }
    Ok(())
}

pub fn read_port_from_file(path: &Path) -> Result<u16, BrowserError> {
    let content = fs::read_to_string(path).map_err(|error| {
        BrowserError::CliUnavailable(format!("Ferryx desktop app is not running: {error}"))
    })?;
    let port: u16 = content.trim().parse().map_err(|error| {
        BrowserError::CliUnavailable(format!(
            "Invalid browser CLI port in {}: {error}",
            path.display()
        ))
    })?;
    if port == 0 {
        return Err(BrowserError::CliUnavailable(format!(
            "Invalid browser CLI port in {}: port cannot be 0",
            path.display()
        )));
    }
    Ok(port)
}

#[cfg(unix)]
pub fn start_browser_cli_server<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: Arc<BrowserManager>,
) -> Result<(), BrowserError> {
    start_browser_cli_server_at_path(app, manager, &browser_cli_socket_path())
}

#[cfg(unix)]
fn start_browser_cli_server_at_path<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: Arc<BrowserManager>,
    socket_path: &Path,
) -> Result<(), BrowserError> {
    use std::os::unix::fs::{FileTypeExt, PermissionsExt};
    use std::os::unix::net::UnixListener;

    let runtime_dir = socket_path
        .parent()
        .ok_or_else(|| BrowserError::Internal("browser CLI socket path has no parent".into()))?;
    fs::create_dir_all(&runtime_dir).map_err(|error| BrowserError::Internal(error.to_string()))?;
    fs::set_permissions(&runtime_dir, fs::Permissions::from_mode(0o700))
        .map_err(|error| BrowserError::Internal(error.to_string()))?;
    match fs::symlink_metadata(socket_path) {
        Ok(metadata) if metadata.file_type().is_socket() => fs::remove_file(socket_path)
            .map_err(|error| BrowserError::Internal(error.to_string()))?,
        Ok(_) => {
            return Err(BrowserError::Internal(
                "browser CLI socket path is not a socket".into(),
            ));
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(BrowserError::Internal(error.to_string())),
    }
    let listener = UnixListener::bind(socket_path)
        .map_err(|error| BrowserError::Internal(error.to_string()))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| BrowserError::Internal(error.to_string()))?;
    fs::set_permissions(socket_path, fs::Permissions::from_mode(0o600))
        .map_err(|error| BrowserError::Internal(error.to_string()))?;

    tauri::async_runtime::spawn(async move {
        let listener = match tokio::net::UnixListener::from_std(listener) {
            Ok(listener) => listener,
            Err(error) => {
                tracing::error!("Failed to register browser CLI socket with Tokio: {error}");
                return;
            }
        };
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let app = app.clone();
            let manager = Arc::clone(&manager);
            tauri::async_runtime::spawn(async move {
                let _ = handle_connection(stream, app, manager).await;
            });
        }
    });
    Ok(())
}

#[cfg(not(unix))]
pub fn start_browser_cli_server<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: Arc<BrowserManager>,
) -> Result<(), BrowserError> {
    start_browser_cli_server_at_path(app, manager, &browser_cli_socket_path())
}

#[cfg(not(unix))]
fn start_browser_cli_server_at_path<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: Arc<BrowserManager>,
    port_path: &Path,
) -> Result<(), BrowserError> {
    use std::net::TcpListener;

    let runtime_dir = port_path
        .parent()
        .ok_or_else(|| BrowserError::Internal("browser CLI socket path has no parent".into()))?;
    fs::create_dir_all(&runtime_dir).map_err(|error| BrowserError::Internal(error.to_string()))?;

    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|error| BrowserError::Internal(format!("Failed to bind TCP listener: {error}")))?;
    let port = listener
        .local_addr()
        .map_err(|error| BrowserError::Internal(format!("Failed to get local port: {error}")))?
        .port();

    listener
        .set_nonblocking(true)
        .map_err(|error| BrowserError::Internal(error.to_string()))?;

    write_port_file(port_path, port)?;

    tauri::async_runtime::spawn(async move {
        let listener = match tokio::net::TcpListener::from_std(listener) {
            Ok(listener) => listener,
            Err(error) => {
                tracing::error!("Failed to register browser CLI TCP listener with Tokio: {error}");
                return;
            }
        };
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let app = app.clone();
            let manager = Arc::clone(&manager);
            tauri::async_runtime::spawn(async move {
                let _ = handle_connection(stream, app, manager).await;
            });
        }
    });
    Ok(())
}

async fn handle_connection<S, R: tauri::Runtime>(
    stream: S,
    app: AppHandle<R>,
    manager: Arc<BrowserManager>,
) -> Result<(), BrowserError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let (reader, mut writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    let response = match reader.read_line(&mut line).await {
        Ok(0) => return Ok(()),
        Ok(_) => match serde_json::from_str::<BrowserCliRequest>(line.trim()) {
            Ok(request) => execute_request(&app, &manager, request).await,
            Err(error) => BrowserCliResponse::Error {
                code: "BROWSER_CLI_REQUEST_INVALID".into(),
                message: error.to_string(),
            },
        },
        Err(error) => return Err(BrowserError::Internal(error.to_string())),
    };
    let mut response = serde_json::to_string(&response)
        .map_err(|error| BrowserError::Internal(error.to_string()))?;
    response.push('\n');
    writer
        .write_all(response.as_bytes())
        .await
        .map_err(|error| BrowserError::Internal(error.to_string()))?;
    writer
        .flush()
        .await
        .map_err(|error| BrowserError::Internal(error.to_string()))
}

async fn execute_request<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &Arc<BrowserManager>,
    request: BrowserCliRequest,
) -> BrowserCliResponse {
    match request {
        BrowserCliRequest::List => BrowserCliResponse::List {
            sessions: manager.list_sessions(),
        },
        BrowserCliRequest::Snapshot { browser_id } => {
            match browser_automation_snapshot(app.clone(), manager, browser_id).await {
                Ok(snapshot) => BrowserCliResponse::Snapshot { snapshot },
                Err(error) => BrowserCliResponse::Error {
                    code: format!("{:?}", error.code),
                    message: error.message,
                },
            }
        }
        BrowserCliRequest::Act { request } => {
            match browser_automation_act(app.clone(), manager, request).await {
                Ok(()) => BrowserCliResponse::Acted,
                Err(error) => BrowserCliResponse::Error {
                    code: format!("{:?}", error.code),
                    message: error.message,
                },
            }
        }
    }
}

async fn send_over_stream<S>(
    stream: S,
    request: BrowserCliRequest,
) -> Result<BrowserCliResponse, BrowserError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let (reader, mut writer) = tokio::io::split(stream);
    let mut request_json = serde_json::to_string(&request)
        .map_err(|error| BrowserError::AutomationFailed(error.to_string()))?;
    request_json.push('\n');
    writer
        .write_all(request_json.as_bytes())
        .await
        .map_err(|error| BrowserError::AutomationFailed(error.to_string()))?;
    writer
        .flush()
        .await
        .map_err(|error| BrowserError::AutomationFailed(error.to_string()))?;
    let mut response = String::new();
    BufReader::new(reader)
        .read_line(&mut response)
        .await
        .map_err(|error| BrowserError::AutomationFailed(error.to_string()))?;
    serde_json::from_str(response.trim())
        .map_err(|error| BrowserError::AutomationFailed(error.to_string()))
}

#[cfg(unix)]
pub async fn send_browser_cli_request(
    request: BrowserCliRequest,
) -> Result<BrowserCliResponse, BrowserError> {
    send_browser_cli_request_at_path(request, &browser_cli_socket_path()).await
}

#[cfg(unix)]
async fn send_browser_cli_request_at_path(
    request: BrowserCliRequest,
    socket_path: &Path,
) -> Result<BrowserCliResponse, BrowserError> {
    use tokio::net::UnixStream;

    let stream = UnixStream::connect(socket_path).await.map_err(|error| {
        BrowserError::CliUnavailable(format!("Ferryx desktop app is not running: {error}"))
    })?;
    send_over_stream(stream, request).await
}

#[cfg(not(unix))]
pub async fn send_browser_cli_request(
    request: BrowserCliRequest,
) -> Result<BrowserCliResponse, BrowserError> {
    send_browser_cli_request_at_path(request, &browser_cli_socket_path()).await
}

#[cfg(not(unix))]
async fn send_browser_cli_request_at_path(
    request: BrowserCliRequest,
    port_path: &Path,
) -> Result<BrowserCliResponse, BrowserError> {
    use tokio::net::TcpStream;

    let port = read_port_from_file(port_path)?;
    let stream = TcpStream::connect(format!("127.0.0.1:{port}"))
        .await
        .map_err(|error| {
            BrowserError::CliUnavailable(format!("Ferryx desktop app is not running: {error}"))
        })?;
    send_over_stream(stream, request).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::{BrowserProfileId, BrowserSessionSummary, CreateBrowserRequest};

    #[test]
    fn test_browser_cli_list_request_serialization() {
        let req = BrowserCliRequest::List;
        let json = serde_json::to_string(&req).unwrap();
        assert_eq!(json, r#"{"command":"list"}"#);
        let parsed: BrowserCliRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, BrowserCliRequest::List);
    }

    #[test]
    fn test_browser_cli_list_response_serialization() {
        let resp = BrowserCliResponse::List {
            sessions: vec![BrowserSessionSummary {
                browser_id: "test-id".to_string(),
                webview_label: "test-label".to_string(),
                workspace_id: Some("ws-1".to_string()),
                profile_id: BrowserProfileId::Default,
                url: "https://example.com/".to_string(),
                title: Some("Example".to_string()),
                visible: true,
            }],
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: BrowserCliResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, resp);
    }

    #[test]
    fn test_read_and_write_port_file_round_trip() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let port_path = temp_dir.path().join("browser.port");
        write_port_file(&port_path, 43210).expect("write port file");
        let read_port = read_port_from_file(&port_path).expect("read port file");
        assert_eq!(read_port, 43210);
    }

    #[test]
    fn test_write_port_file_replaces_existing_file() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let port_path = temp_dir.path().join("browser.port");
        write_port_file(&port_path, 11111).expect("write initial port file");
        assert_eq!(read_port_from_file(&port_path).expect("read port"), 11111);

        write_port_file(&port_path, 22222).expect("overwrite port file");
        assert_eq!(
            read_port_from_file(&port_path).expect("read updated port"),
            22222
        );
    }

    #[test]
    fn test_read_port_from_file_missing_returns_unavailable() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let port_path = temp_dir.path().join("nonexistent.port");
        let result = read_port_from_file(&port_path);
        assert!(matches!(result, Err(BrowserError::CliUnavailable(_))));
    }

    #[test]
    fn test_read_port_from_file_malformed_returns_unavailable() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let port_path = temp_dir.path().join("invalid.port");

        fs::write(&port_path, "not-a-port\n").expect("write malformed");
        assert!(matches!(
            read_port_from_file(&port_path),
            Err(BrowserError::CliUnavailable(_))
        ));

        fs::write(&port_path, "0\n").expect("write port 0");
        assert!(matches!(
            read_port_from_file(&port_path),
            Err(BrowserError::CliUnavailable(_))
        ));

        fs::write(&port_path, "70000\n").expect("write out-of-range port");
        assert!(matches!(
            read_port_from_file(&port_path),
            Err(BrowserError::CliUnavailable(_))
        ));

        fs::write(&port_path, "   \n").expect("write empty/whitespace");
        assert!(matches!(
            read_port_from_file(&port_path),
            Err(BrowserError::CliUnavailable(_))
        ));
    }

    #[tokio::test]
    async fn test_browser_cli_send_over_stream_round_trip() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");

        let manager = Arc::new(BrowserManager::new());
        let registered_session = manager
            .register_session(CreateBrowserRequest {
                browser_id: None,
                workspace_id: Some("workspace-duplex".to_string()),
                worktree_path: Some("/worktree/alpha".to_string()),
                url: "https://ferryx.dev".to_string(),
                profile: Some(BrowserProfileId::Default),
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .expect("register session");

        let (client_stream, server_stream) = tokio::io::duplex(4096);
        let app_handle = app.handle().clone();
        let manager_clone = Arc::clone(&manager);
        let server_task = tokio::spawn(async move {
            handle_connection(server_stream, app_handle, manager_clone).await
        });

        let response = send_over_stream(client_stream, BrowserCliRequest::List)
            .await
            .expect("send request over stream");

        let server_result = server_task.await.expect("server task completed");
        assert!(server_result.is_ok());

        let expected_summary = BrowserSessionSummary {
            browser_id: registered_session.browser_id,
            webview_label: registered_session.webview_label,
            workspace_id: Some("workspace-duplex".to_string()),
            profile_id: BrowserProfileId::Default,
            url: registered_session.url,
            title: None,
            visible: true,
        };

        assert_eq!(
            response,
            BrowserCliResponse::List {
                sessions: vec![expected_summary],
            }
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_browser_cli_list_round_trip_unix_stream() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");

        let manager = Arc::new(BrowserManager::new());
        let registered_session = manager
            .register_session(CreateBrowserRequest {
                browser_id: None,
                workspace_id: Some("workspace-regression".to_string()),
                worktree_path: Some("/worktree/alpha".to_string()),
                url: "https://ferryx.dev".to_string(),
                profile: Some(BrowserProfileId::Default),
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .expect("register session");

        let (mut client_stream, server_stream) =
            tokio::net::UnixStream::pair().expect("unix stream pair");

        let app_handle = app.handle().clone();
        let manager_clone = Arc::clone(&manager);
        let handler_task = tokio::spawn(async move {
            handle_connection(server_stream, app_handle, manager_clone).await
        });

        client_stream
            .write_all(b"{\"command\":\"list\"}\n")
            .await
            .expect("write request line");
        client_stream.flush().await.expect("flush request");

        let (client_reader, _) = client_stream.into_split();
        let mut response_line = String::new();
        BufReader::new(client_reader)
            .read_line(&mut response_line)
            .await
            .expect("read response line");

        let handler_result = handler_task.await.expect("handler task completed");
        assert!(handler_result.is_ok());

        let response: BrowserCliResponse =
            serde_json::from_str(&response_line).expect("deserialize response");

        let expected_summary = BrowserSessionSummary {
            browser_id: registered_session.browser_id,
            webview_label: registered_session.webview_label,
            workspace_id: Some("workspace-regression".to_string()),
            profile_id: BrowserProfileId::Default,
            url: registered_session.url,
            title: None,
            visible: true,
        };

        assert_eq!(
            response,
            BrowserCliResponse::List {
                sessions: vec![expected_summary],
            }
        );
    }

    #[cfg(unix)]
    #[test]
    fn browser_cli_server_starts_without_tokio_reactor() {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::fs::PermissionsExt;
        use std::os::unix::net::UnixStream;
        use std::time::Duration;

        assert!(tokio::runtime::Handle::try_current().is_err());
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let socket_path = temp_dir.path().join("browser.sock");
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");

        start_browser_cli_server_at_path(
            app.handle().clone(),
            Arc::new(BrowserManager::new()),
            &socket_path,
        )
        .expect("browser CLI startup succeeds without Tokio reactor");
        assert_eq!(
            fs::metadata(&socket_path)
                .expect("socket metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600,
        );

        let mut client = UnixStream::connect(&socket_path).expect("connect to browser CLI socket");
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("set response timeout");
        client
            .write_all(b"{\"command\":\"list\"}\n")
            .expect("write list request");
        let mut response = String::new();
        BufReader::new(client)
            .read_line(&mut response)
            .expect("read browser CLI list response");
        assert_eq!(
            serde_json::from_str::<BrowserCliResponse>(response.trim())
                .expect("parse browser CLI list response"),
            BrowserCliResponse::List {
                sessions: Vec::new()
            },
        );
    }

    #[cfg(not(unix))]
    #[test]
    fn browser_cli_server_starts_without_tokio_reactor() {
        use std::io::{BufRead, BufReader, Write};
        use std::net::TcpStream;
        use std::time::Duration;

        assert!(tokio::runtime::Handle::try_current().is_err());
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let port_path = temp_dir.path().join("browser.port");
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");

        start_browser_cli_server_at_path(
            app.handle().clone(),
            Arc::new(BrowserManager::new()),
            &port_path,
        )
        .expect("browser CLI startup succeeds without Tokio reactor");

        assert!(port_path.exists());
        let port = read_port_from_file(&port_path).expect("read port from file");
        assert!(port > 0);

        let mut client =
            TcpStream::connect(format!("127.0.0.1:{port}")).expect("connect to browser CLI port");
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("set response timeout");
        client
            .write_all(b"{\"command\":\"list\"}\n")
            .expect("write list request");
        let mut response = String::new();
        BufReader::new(client)
            .read_line(&mut response)
            .expect("read browser CLI list response");
        assert_eq!(
            serde_json::from_str::<BrowserCliResponse>(response.trim())
                .expect("parse browser CLI list response"),
            BrowserCliResponse::List {
                sessions: Vec::new()
            },
        );
    }

    #[cfg(not(unix))]
    #[tokio::test]
    async fn test_browser_cli_list_round_trip_tcp() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");

        let manager = Arc::new(BrowserManager::new());
        let registered_session = manager
            .register_session(CreateBrowserRequest {
                browser_id: None,
                workspace_id: Some("workspace-tcp".to_string()),
                worktree_path: Some("/worktree/alpha".to_string()),
                url: "https://ferryx.dev".to_string(),
                profile: Some(BrowserProfileId::Default),
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .expect("register session");

        let temp_dir = tempfile::tempdir().expect("tempdir");
        let port_path = temp_dir.path().join("browser.port");

        start_browser_cli_server_at_path(app.handle().clone(), Arc::clone(&manager), &port_path)
            .expect("start browser CLI server");

        let response = send_browser_cli_request_at_path(BrowserCliRequest::List, &port_path)
            .await
            .expect("send browser CLI list request");

        let expected_summary = BrowserSessionSummary {
            browser_id: registered_session.browser_id,
            webview_label: registered_session.webview_label,
            workspace_id: Some("workspace-tcp".to_string()),
            profile_id: BrowserProfileId::Default,
            url: registered_session.url,
            title: None,
            visible: true,
        };

        assert_eq!(
            response,
            BrowserCliResponse::List {
                sessions: vec![expected_summary],
            }
        );
    }

    #[cfg(not(unix))]
    #[tokio::test]
    async fn test_browser_cli_send_request_stale_port_fails() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let port_path = temp_dir.path().join("stale.port");

        // Bind to get an unused port and immediately close it
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let closed_port = listener.local_addr().expect("local addr").port();
        drop(listener);

        write_port_file(&port_path, closed_port).expect("write stale port file");

        let result = send_browser_cli_request_at_path(BrowserCliRequest::List, &port_path).await;
        assert!(matches!(result, Err(BrowserError::CliUnavailable(_))));
    }
}
