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

pub fn browser_cli_socket_path() -> PathBuf {
    crate::daemon::server::get_runtime_dir().join("browser.sock")
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
    _app: AppHandle<R>,
    _manager: Arc<BrowserManager>,
) -> Result<(), BrowserError> {
    Err(BrowserError::PlatformUnsupported(
        "browser CLI transport requires a platform-specific local socket".into(),
    ))
}

#[cfg(unix)]
async fn handle_connection<R: tauri::Runtime>(
    stream: tokio::net::UnixStream,
    app: AppHandle<R>,
    manager: Arc<BrowserManager>,
) -> Result<(), BrowserError> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let (reader, mut writer) = stream.into_split();
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

#[cfg(unix)]
pub async fn send_browser_cli_request(
    request: BrowserCliRequest,
) -> Result<BrowserCliResponse, BrowserError> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;

    let stream = UnixStream::connect(browser_cli_socket_path())
        .await
        .map_err(|error| {
            BrowserError::CliUnavailable(format!("Ferryx desktop app is not running: {error}"))
        })?;
    let (reader, mut writer) = stream.into_split();
    let mut request = serde_json::to_string(&request)
        .map_err(|error| BrowserError::AutomationFailed(error.to_string()))?;
    request.push('\n');
    writer
        .write_all(request.as_bytes())
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

#[cfg(not(unix))]
pub async fn send_browser_cli_request(
    _request: BrowserCliRequest,
) -> Result<BrowserCliResponse, BrowserError> {
    Err(BrowserError::PlatformUnsupported(
        "browser CLI transport requires a platform-specific local socket".into(),
    ))
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
}
