use crate::browser::{
    BrowserAutomationRequest, BrowserAutomationSnapshot, BrowserConsoleEntry, BrowserCookieEntry,
    BrowserError, BrowserManager, BrowserSessionCreatedPayload, BrowserSessionSummary,
    BrowserWaitCondition, CreateBrowserRequest,
};
use crate::ipc::browser::{
    browser_automation_act, browser_automation_snapshot, close_browser_session,
    create_browser_session, identify_browser_session, navigate_browser_session,
};
use crate::ipc::error::{IpcError, IpcErrorCode};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::io::BufReader;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "camelCase")]
pub enum BrowserCliRequest {
    List,
    Snapshot {
        browser_id: String,
    },
    Act {
        request: BrowserAutomationRequest,
    },
    #[serde(rename_all = "camelCase")]
    Open {
        url: String,
        #[serde(default, alias = "workspace_id")]
        workspace_id: Option<String>,
        #[serde(default, alias = "worktree_path")]
        worktree_path: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Navigate {
        #[serde(alias = "browser_id")]
        browser_id: String,
        url: String,
    },
    #[serde(rename_all = "camelCase")]
    Close {
        #[serde(alias = "browser_id")]
        browser_id: String,
    },
    Identify,
    #[serde(rename_all = "camelCase")]
    Eval {
        #[serde(alias = "browser_id")]
        browser_id: String,
        script: String,
    },
    #[serde(rename_all = "camelCase")]
    Wait {
        #[serde(alias = "browser_id")]
        browser_id: String,
        condition: BrowserWaitCondition,
    },
    #[serde(rename_all = "camelCase")]
    Console {
        #[serde(alias = "browser_id")]
        browser_id: String,
        #[serde(default, alias = "errors_only")]
        errors_only: Option<bool>,
        #[serde(default)]
        clear: Option<bool>,
    },
    #[serde(rename_all = "camelCase")]
    Focus {
        #[serde(alias = "browser_id")]
        browser_id: String,
    },
    #[serde(rename_all = "camelCase")]
    Screenshot {
        #[serde(alias = "browser_id")]
        browser_id: String,
        #[serde(alias = "out_path")]
        out_path: String,
    },
    #[serde(rename_all = "camelCase")]
    Cookies {
        #[serde(alias = "browser_id")]
        browser_id: String,
        action: String,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        value: Option<String>,
        #[serde(default)]
        domain: Option<String>,
        #[serde(default)]
        path: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Storage {
        #[serde(alias = "browser_id")]
        browser_id: String,
        kind: String,
        action: String,
        #[serde(default)]
        key: Option<String>,
        #[serde(default)]
        value: Option<String>,
    },
}

/// An authenticated request line: the capability token plus the command itself.
///
/// The control socket drives the user's logged-in browser, so possession of the
/// endpoint address alone must never be sufficient. On Windows the endpoint is a
/// loopback TCP port that every local process can reach, and on unix the socket
/// mode only narrows callers to the same uid. The token is what actually proves
/// the caller was allowed to read the capability file this process wrote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserCliEnvelope {
    pub token: String,
    #[serde(flatten)]
    pub request: BrowserCliRequest,
}

pub const BROWSER_CLI_UNAUTHORIZED: &str = "BROWSER_CLI_UNAUTHORIZED";

/// Length of the hex-encoded capability token (32 bytes of entropy).
const BROWSER_CLI_TOKEN_BYTES: usize = 32;

/// Capability file that carries the token for the current server instance. It
/// sits beside the socket/port file inside the 0700 runtime directory.
pub fn browser_cli_token_path() -> PathBuf {
    crate::daemon::server::get_runtime_dir().join("browser.token")
}

fn token_path_for(endpoint_path: &Path) -> PathBuf {
    endpoint_path.with_file_name(match endpoint_path.file_name().and_then(|n| n.to_str()) {
        Some(name) => format!("{name}.token"),
        None => "browser.token".to_string(),
    })
}

fn generate_browser_cli_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; BROWSER_CLI_TOKEN_BYTES];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Compares two tokens without leaking their matching prefix length through
/// timing. Length is public information here, so an early length check is safe.
fn tokens_match(expected: &str, provided: &str) -> bool {
    let expected = expected.as_bytes();
    let provided = provided.as_bytes();
    if expected.len() != provided.len() {
        return false;
    }
    let mut difference = 0u8;
    for (left, right) in expected.iter().zip(provided.iter()) {
        difference |= left ^ right;
    }
    difference == 0
}

fn write_token_file(path: &Path, token: &str) -> Result<(), BrowserError> {
    match fs::symlink_metadata(path) {
        Ok(_) => fs::remove_file(path).map_err(|error| {
            BrowserError::Internal(format!(
                "Failed to replace browser CLI token file {}: {error}",
                path.display()
            ))
        })?,
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => {
            return Err(BrowserError::Internal(format!(
                "Failed to inspect browser CLI token file {}: {error}",
                path.display()
            )))
        }
    }
    fs::write(path, token).map_err(|error| {
        BrowserError::Internal(format!("Failed to write browser CLI token file: {error}"))
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|error| {
            BrowserError::Internal(format!(
                "Failed to restrict browser CLI token file permissions: {error}"
            ))
        })?;
    }
    Ok(())
}

fn read_token_file(path: &Path) -> Result<String, BrowserError> {
    let token = fs::read_to_string(path).map_err(|error| {
        BrowserError::CliUnavailable(format!("Ferryx desktop app is not running: {error}"))
    })?;
    let token = token.trim().to_string();
    if token.is_empty() {
        return Err(BrowserError::CliUnavailable(format!(
            "Invalid browser CLI token in {}: token is empty",
            path.display()
        )));
    }
    Ok(token)
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
    Opened {
        browser: BrowserSessionSummary,
    },
    Navigated,
    Closed,
    Identified {
        browser: Option<BrowserSessionSummary>,
    },
    Evaluated {
        result: Option<String>,
        truncated: bool,
    },
    Waited,
    Focused,
    ScreenshotSaved {
        path: String,
    },
    ConsoleEntries {
        entries: Vec<BrowserConsoleEntry>,
    },
    CookieEntries {
        cookies: Vec<BrowserCookieEntry>,
    },
    StorageValue {
        value: Option<String>,
    },
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
    let token = Arc::new(generate_browser_cli_token());
    write_token_file(&token_path_for(socket_path), token.as_str())?;

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
            let token = Arc::clone(&token);
            tauri::async_runtime::spawn(async move {
                let _ = handle_connection(stream, app, manager, token).await;
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
    let token = Arc::new(generate_browser_cli_token());
    write_token_file(&token_path_for(port_path), token.as_str())?;

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
            let token = Arc::clone(&token);
            tauri::async_runtime::spawn(async move {
                let _ = handle_connection(stream, app, manager, token).await;
            });
        }
    });
    Ok(())
}

/// Upper bound for a single CLI request line (1 MiB). A connection whose line
/// exceeds this is rejected with `BROWSER_CLI_REQUEST_TOO_LARGE` instead of
/// being buffered without bound.
const MAX_REQUEST_BYTES: usize = 1024 * 1024;

enum RequestLine {
    Eof,
    Line(String),
    TooLarge,
}

async fn read_limited_line<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
    max_bytes: usize,
) -> Result<RequestLine, std::io::Error> {
    use tokio::io::AsyncBufReadExt;

    let mut line = Vec::new();
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return Ok(if line.is_empty() {
                RequestLine::Eof
            } else {
                RequestLine::Line(String::from_utf8_lossy(&line).into_owned())
            });
        }
        match available.iter().position(|&byte| byte == b'\n') {
            Some(newline_index) => {
                if line.len() + newline_index + 1 > max_bytes {
                    return Ok(RequestLine::TooLarge);
                }
                line.extend_from_slice(&available[..=newline_index]);
                reader.consume(newline_index + 1);
                return Ok(RequestLine::Line(
                    String::from_utf8_lossy(&line).into_owned(),
                ));
            }
            None => {
                if line.len() + available.len() > max_bytes {
                    return Ok(RequestLine::TooLarge);
                }
                let chunk_len = available.len();
                line.extend_from_slice(available);
                reader.consume(chunk_len);
            }
        }
    }
}

fn ipc_error_code_string(code: IpcErrorCode) -> String {
    serde_json::to_value(&code)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| format!("{code:?}"))
}

async fn handle_connection<S, R: tauri::Runtime>(
    stream: S,
    app: AppHandle<R>,
    manager: Arc<BrowserManager>,
    expected_token: Arc<String>,
) -> Result<(), BrowserError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::AsyncWriteExt;

    let unauthorized = || BrowserCliResponse::Error {
        code: BROWSER_CLI_UNAUTHORIZED.into(),
        message: "browser CLI requires the capability token of the running Ferryx app".into(),
    };

    let (reader, mut writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(reader);
    let response = match read_limited_line(&mut reader, MAX_REQUEST_BYTES).await {
        Ok(RequestLine::Eof) => return Ok(()),
        Ok(RequestLine::TooLarge) => BrowserCliResponse::Error {
            code: "BROWSER_CLI_REQUEST_TOO_LARGE".into(),
            message: format!(
                "request exceeds the maximum of {MAX_REQUEST_BYTES} bytes per connection"
            ),
        },
        Ok(RequestLine::Line(line)) => {
            // Authorization is decided before the command is interpreted, so an
            // unauthorized peer learns nothing about which commands exist or
            // whether its arguments named a real browser session.
            match serde_json::from_str::<BrowserCliEnvelope>(line.trim()) {
                Ok(envelope) if tokens_match(expected_token.as_str(), &envelope.token) => {
                    execute_request(&app, &manager, envelope.request).await
                }
                Ok(_) => unauthorized(),
                Err(error) => {
                    if serde_json::from_str::<BrowserCliRequest>(line.trim()).is_ok() {
                        unauthorized()
                    } else {
                        BrowserCliResponse::Error {
                            code: "BROWSER_CLI_REQUEST_INVALID".into(),
                            message: error.to_string(),
                        }
                    }
                }
            }
        }
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
                    code: ipc_error_code_string(error.code),
                    message: error.message,
                },
            }
        }
        BrowserCliRequest::Act { request } => {
            match browser_automation_act(app.clone(), manager, request).await {
                Ok(()) => BrowserCliResponse::Acted,
                Err(error) => BrowserCliResponse::Error {
                    code: ipc_error_code_string(error.code),
                    message: error.message,
                },
            }
        }
        BrowserCliRequest::Open {
            url,
            workspace_id,
            worktree_path,
        } => {
            if let Err(browser_error) = crate::browser::validate_url(&url) {
                let ipc_error = IpcError::from(browser_error);
                return BrowserCliResponse::Error {
                    code: ipc_error_code_string(ipc_error.code),
                    message: ipc_error.message,
                };
            }
            let create_req = CreateBrowserRequest {
                browser_id: None,
                workspace_id: workspace_id.clone(),
                worktree_path,
                url,
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            };
            match create_browser_session(app, manager, create_req).await {
                Ok(state) => {
                    let payload = BrowserSessionCreatedPayload {
                        browser: state.clone(),
                        workspace_id,
                    };
                    let _ = app.emit(crate::browser::guest::BROWSER_SESSION_CREATED_EVENT, payload);
                    BrowserCliResponse::Opened {
                        browser: BrowserSessionSummary::from(state),
                    }
                }
                Err(error) => BrowserCliResponse::Error {
                    code: ipc_error_code_string(error.code),
                    message: error.message,
                },
            }
        }
        BrowserCliRequest::Navigate { browser_id, url } => {
            if let Err(browser_error) = crate::browser::validate_url(&url) {
                let ipc_error = IpcError::from(browser_error);
                return BrowserCliResponse::Error {
                    code: ipc_error_code_string(ipc_error.code),
                    message: ipc_error.message,
                };
            }
            match navigate_browser_session(app, manager, &browser_id, &url).await {
                Ok(()) => BrowserCliResponse::Navigated,
                Err(error) => BrowserCliResponse::Error {
                    code: ipc_error_code_string(error.code),
                    message: error.message,
                },
            }
        }
        BrowserCliRequest::Close { browser_id } => {
            match close_browser_session(app, manager, &browser_id).await {
                Ok(()) => BrowserCliResponse::Closed,
                Err(error) => BrowserCliResponse::Error {
                    code: ipc_error_code_string(error.code),
                    message: error.message,
                },
            }
        }
        BrowserCliRequest::Identify => {
            BrowserCliResponse::Identified {
                browser: identify_browser_session(manager),
            }
        }
        BrowserCliRequest::Eval { browser_id, script } => {
            match crate::ipc::browser::eval_browser_session(app, manager, &browser_id, &script).await {
                Ok((result, truncated)) => BrowserCliResponse::Evaluated { result, truncated },
                Err(error) => BrowserCliResponse::Error {
                    code: ipc_error_code_string(error.code),
                    message: error.message,
                },
            }
        }
        BrowserCliRequest::Wait {
            browser_id,
            condition,
        } => {
            match crate::ipc::browser::wait_browser_session(app, manager, &browser_id, condition).await {
                Ok(()) => BrowserCliResponse::Waited,
                Err(error) => BrowserCliResponse::Error {
                    code: ipc_error_code_string(error.code),
                    message: error.message,
                },
            }
        }
        BrowserCliRequest::Console {
            browser_id,
            errors_only,
            clear,
        } => {
            match crate::ipc::browser::console_browser_session(
                app,
                manager,
                &browser_id,
                errors_only.unwrap_or(false),
                clear.unwrap_or(false),
            )
            .await
            {
                Ok(entries) => BrowserCliResponse::ConsoleEntries { entries },
                Err(error) => BrowserCliResponse::Error {
                    code: ipc_error_code_string(error.code),
                    message: error.message,
                },
            }
        }
        BrowserCliRequest::Focus { browser_id } => {
            match crate::ipc::browser::focus_browser_session(app, manager, &browser_id) {
                Ok(()) => BrowserCliResponse::Focused,
                Err(error) => BrowserCliResponse::Error {
                    code: ipc_error_code_string(error.code),
                    message: error.message,
                },
            }
        }
        BrowserCliRequest::Screenshot {
            browser_id,
            out_path,
        } => {
            match crate::ipc::browser::screenshot_browser_session(
                app,
                manager,
                &browser_id,
                &out_path,
            )
            .await
            {
                Ok(path) => BrowserCliResponse::ScreenshotSaved { path },
                Err(error) => BrowserCliResponse::Error {
                    code: ipc_error_code_string(error.code),
                    message: error.message,
                },
            }
        }
        BrowserCliRequest::Cookies {
            browser_id,
            action,
            name,
            value,
            domain,
            path,
        } => {
            match crate::ipc::browser::cookies_browser_session(
                app,
                manager,
                &browser_id,
                &action,
                name.as_deref(),
                value.as_deref(),
                domain.as_deref(),
                path.as_deref(),
            )
            .await
            {
                Ok(cookies) => BrowserCliResponse::CookieEntries { cookies },
                Err(error) => BrowserCliResponse::Error {
                    code: ipc_error_code_string(error.code),
                    message: error.message,
                },
            }
        }
        BrowserCliRequest::Storage {
            browser_id,
            kind,
            action,
            key,
            value,
        } => {
            match crate::ipc::browser::storage_browser_session(
                app,
                manager,
                &browser_id,
                &kind,
                &action,
                key.as_deref(),
                value.as_deref(),
            )
            .await
            {
                Ok(value) => BrowserCliResponse::StorageValue { value },
                Err(error) => BrowserCliResponse::Error {
                    code: ipc_error_code_string(error.code),
                    message: error.message,
                },
            }
        }
    }
}

async fn send_over_stream<S>(
    stream: S,
    request: BrowserCliRequest,
    token: String,
) -> Result<BrowserCliResponse, BrowserError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let (reader, mut writer) = tokio::io::split(stream);
    let mut request_json = serde_json::to_string(&BrowserCliEnvelope { token, request })
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

    let token = read_token_file(&token_path_for(socket_path))?;
    let stream = UnixStream::connect(socket_path).await.map_err(|error| {
        BrowserError::CliUnavailable(format!("Ferryx desktop app is not running: {error}"))
    })?;
    send_over_stream(stream, request, token).await
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
    let token = read_token_file(&token_path_for(port_path))?;
    let stream = TcpStream::connect(format!("127.0.0.1:{port}"))
        .await
        .map_err(|error| {
            BrowserError::CliUnavailable(format!("Ferryx desktop app is not running: {error}"))
        })?;
    send_over_stream(stream, request, token).await
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

    // This fixture uses the production connection handler over an actual owned
    // TCP socket on every platform. No desktop, daemon or global runtime path.
    async fn p12_raw_tcp_request(request: serde_json::Value) -> BrowserCliResponse {

        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());
        manager
            .register_session(CreateBrowserRequest {
                browser_id: Some("p12-owned-browser".into()),
                workspace_id: Some("p12-owned-workspace".into()),
                worktree_path: None,
                url: "https://example.test/p12-private".into(),
                profile: Some(BrowserProfileId::Default),
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .expect("register owned browser");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind owned listener");
        let address = listener.local_addr().expect("listener address");
        let server = async {
            let (stream, _) = listener.accept().await.expect("accept independent peer");
            handle_connection(
                stream,
                app.handle().clone(),
                manager,
                Arc::new("p12-owned-capability-token".to_string()),
            )
            .await
            .expect("handle actual TCP connection");
        };
        let client = async {
            let mut stream = tokio::net::TcpStream::connect(address)
                .await
                .expect("connect independent peer");
            let mut bytes = serde_json::to_vec(&request).expect("serialize raw request");
            bytes.push(b'\n');
            stream.write_all(&bytes).await.expect("write raw request");
            let mut response = String::new();
            BufReader::new(stream)
                .read_line(&mut response)
                .await
                .expect("read authorization result");
            serde_json::from_str(&response).expect("parse authorization result")
        };
        // Joining scoped futures, rather than spawning, guarantees that timeout
        // and panic drop the listener and both streams; there is no orphan task.
        let (_, response) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            tokio::join!(server, client)
        })
        .await
        .expect("bounded socket exchange");
        response
    }

    #[tokio::test]
    async fn p12_tcp_rejects_unauthenticated_commands() {
        // Given an actual listener and a registered private browser, when a peer
        // sends each supported command without credentials, then dispatch is denied.
        for request in [
            serde_json::json!({"command": "list"}),
            serde_json::json!({"command": "snapshot", "browser_id": "missing-browser"}),
            serde_json::json!({"command": "act", "request": {
                "browserId": "missing-browser", "generation": 1,
                "action": {"type": "click", "reference": "e1"}
            }}),
        ] {
            let response = p12_raw_tcp_request(request.clone()).await;
            assert!(
                matches!(&response, BrowserCliResponse::Error { code, .. }
                    if code == "BROWSER_CLI_UNAUTHORIZED"),
                "unauthenticated {request} reached dispatch: {response:?}"
            );
        }
    }

    #[tokio::test]
    async fn p12_tcp_rejects_forged_credential() {
        // Given a peer without the capability, when it supplies a forged token,
        // then the actual socket must not disclose the registered browser URL.
        let response = p12_raw_tcp_request(serde_json::json!({
            "command": "list", "token": "p12-forged-not-a-capability"
        }))
        .await;
        assert!(
            matches!(&response, BrowserCliResponse::Error { code, .. }
                if code == "BROWSER_CLI_UNAUTHORIZED"),
            "forged credential reached dispatch: {response:?}"
        );
    }

    #[tokio::test]
    async fn p12_tcp_accepts_the_capability_token() {
        // Given the capability token this server instance minted, when a peer
        // presents it, then the command is dispatched normally. This is what
        // proves the rejection tests above fail on authorization, not on the
        // envelope shape.
        let response = p12_raw_tcp_request(serde_json::json!({
            "command": "list", "token": "p12-owned-capability-token"
        }))
        .await;
        let BrowserCliResponse::List { sessions } = response else {
            panic!("authorized list must dispatch: {response:?}");
        };
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].browser_id, "p12-owned-browser");
    }

    #[test]
    fn browser_cli_tokens_match_only_on_exact_equality() {
        let token = generate_browser_cli_token();
        assert_eq!(token.len(), BROWSER_CLI_TOKEN_BYTES * 2);
        assert!(tokens_match(&token, &token.clone()));
        assert!(!tokens_match(&token, &token[..token.len() - 1]));
        assert!(!tokens_match(&token, ""));
        // Two separately minted tokens must not collide.
        assert_ne!(token, generate_browser_cli_token());
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
            handle_connection(
                server_stream,
                app_handle,
                manager_clone,
                Arc::new("duplex-capability-token".to_string()),
            )
            .await
        });

        let response = send_over_stream(
            client_stream,
            BrowserCliRequest::List,
            "duplex-capability-token".to_string(),
        )
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

    #[tokio::test]
    async fn test_read_limited_line_rejects_oversized_request_line() {
        let mut oversized = vec![b'a'; MAX_REQUEST_BYTES];
        oversized.push(b'!');
        oversized.push(b'\n');
        let mut reader = BufReader::new(&oversized[..]);
        let result = read_limited_line(&mut reader, MAX_REQUEST_BYTES).await;
        assert!(matches!(result, Ok(RequestLine::TooLarge)));

        // A line exactly at the cap is still accepted.
        let mut exact = vec![b'a'; MAX_REQUEST_BYTES - 1];
        exact.push(b'\n');
        let mut reader = BufReader::new(&exact[..]);
        let result = read_limited_line(&mut reader, MAX_REQUEST_BYTES).await;
        assert!(matches!(result, Ok(RequestLine::Line(_))));
    }

    #[tokio::test]
    async fn test_browser_cli_oversized_request_gets_too_large_error() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());

        let (client_stream, server_stream) = tokio::io::duplex(64 * 1024);
        let app_handle = app.handle().clone();
        let server_task = tokio::spawn(async move {
            handle_connection(
                server_stream,
                app_handle,
                manager,
                Arc::new("oversize-capability-token".to_string()),
            )
            .await
        });

        let (client_reader, mut client_writer) = tokio::io::split(client_stream);
        let mut oversized_line = vec![b'a'; MAX_REQUEST_BYTES + 16];
        oversized_line.push(b'\n');
        // The server stops reading once the cap is exceeded and closes its half,
        // so the tail of this write may surface as a broken pipe; either way the
        // oversized line must never be accepted.
        let _ = client_writer.write_all(&oversized_line).await;
        let _ = client_writer.flush().await;

        let mut response_line = String::new();
        BufReader::new(client_reader)
            .read_line(&mut response_line)
            .await
            .expect("read too-large response line");
        let response: BrowserCliResponse =
            serde_json::from_str(response_line.trim()).expect("deserialize response");
        assert_eq!(
            response,
            BrowserCliResponse::Error {
                code: "BROWSER_CLI_REQUEST_TOO_LARGE".into(),
                message: format!(
                    "request exceeds the maximum of {MAX_REQUEST_BYTES} bytes per connection"
                ),
            }
        );

        let server_result = server_task.await.expect("server task completed");
        assert!(server_result.is_ok());
    }

    #[tokio::test]
    async fn test_browser_cli_error_code_matches_ipc_wire_format() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());

        let response = execute_request(
            app.handle(),
            &manager,
            BrowserCliRequest::Snapshot {
                browser_id: "missing-browser".into(),
            },
        )
        .await;

        let wire_code =
            serde_json::to_value(IpcErrorCode::BrowserNotFound).expect("serialize IPC error code");
        let wire_code = wire_code.as_str().expect("IPC code serializes to string");
        assert_eq!(wire_code, "BROWSER_NOT_FOUND");

        match response {
            BrowserCliResponse::Error { code, message } => {
                assert_eq!(code, wire_code);
                assert_ne!(code, "BrowserNotFound");
                assert!(!message.is_empty());
            }
            other => panic!("expected error response, got {other:?}"),
        }
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
            handle_connection(
                server_stream,
                app_handle,
                manager_clone,
                Arc::new("unix-pair-capability-token".to_string()),
            )
            .await
        });

        client_stream
            .write_all(b"{\"command\":\"list\",\"token\":\"unix-pair-capability-token\"}\n")
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
        // The server minted this token at startup; reading it back is exactly what
        // an authorized caller does, and the file must be owner-only.
        let token_path = token_path_for(&socket_path);
        assert_eq!(
            fs::metadata(&token_path)
                .expect("token metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600,
        );
        let token = read_token_file(&token_path).expect("read capability token");
        client
            .write_all(format!("{{\"command\":\"list\",\"token\":\"{token}\"}}\n").as_bytes())
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
        let token = read_token_file(&token_path_for(&port_path)).expect("read capability token");
        client
            .write_all(format!("{{\"command\":\"list\",\"token\":\"{token}\"}}\n").as_bytes())
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

    async fn send_raw_line<R: tauri::Runtime>(
        app_handle: AppHandle<R>,
        manager: Arc<BrowserManager>,
        token: &str,
        raw_json: &str,
    ) -> serde_json::Value {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let (client_stream, server_stream) = tokio::io::duplex(8192);
        let token_clone = Arc::new(token.to_string());
        let handler_task = tokio::spawn(async move {
            handle_connection(server_stream, app_handle, manager, token_clone).await
        });

        let (client_reader, mut client_writer) = tokio::io::split(client_stream);
        let mut line = raw_json.as_bytes().to_vec();
        line.push(b'\n');
        client_writer.write_all(&line).await.expect("write raw json line");
        client_writer.flush().await.expect("flush raw json line");

        let mut response_line = String::new();
        BufReader::new(client_reader)
            .read_line(&mut response_line)
            .await
            .expect("read response line");

        let handler_result = handler_task.await.expect("handler task completed");
        assert!(handler_result.is_ok());

        serde_json::from_str(response_line.trim()).expect("deserialize response JSON")
    }

    #[tokio::test]
    async fn test_browser_cli_open_round_trip() {
        use tauri::Listener;

        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());
        let token = "test-token";

        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<serde_json::Value>();
        app.listen("browser_session_created", move |event| {
            if let Ok(payload) = serde_json::from_str::<serde_json::Value>(event.payload()) {
                let _ = event_tx.send(payload);
            }
        });

        let raw_req = format!(
            "{{\"command\":\"open\",\"url\":\"https://example.com\",\"workspaceId\":\"ws-a\",\"token\":\"{token}\"}}"
        );
        let resp = send_raw_line(app.handle().clone(), Arc::clone(&manager), token, &raw_req).await;

        assert_eq!(resp["type"], "opened", "unexpected response: {resp:?}");
        assert!(
            resp["browser"]["url"] == "https://example.com"
                || resp["browser"]["url"] == "https://example.com/",
            "unexpected browser url: {:?}",
            resp["browser"]["url"]
        );
        assert_eq!(resp["browser"]["workspaceId"], "ws-a");

        let opened_id = resp["browser"]["browserId"].as_str().expect("browserId");

        let list_req = format!("{{\"command\":\"list\",\"token\":\"{token}\"}}");
        let list_resp = send_raw_line(app.handle().clone(), Arc::clone(&manager), token, &list_req).await;
        assert_eq!(list_resp["type"], "list");
        let sessions = list_resp["sessions"].as_array().expect("sessions array");
        assert!(sessions.iter().any(|s| s["browserId"] == opened_id && (s["url"] == "https://example.com" || s["url"] == "https://example.com/")));

        let event_payload = event_rx.try_recv().expect("received browser_session_created event");
        assert_eq!(event_payload["browser"]["browserId"], opened_id);
        assert_eq!(event_payload["workspaceId"], "ws-a");
    }

    #[tokio::test]
    async fn test_browser_cli_open_rejects_file_scheme() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());
        let token = "test-token";

        let raw_req = format!(
            "{{\"command\":\"open\",\"url\":\"file:///etc/passwd\",\"token\":\"{token}\"}}"
        );
        let resp = send_raw_line(app.handle().clone(), Arc::clone(&manager), token, &raw_req).await;

        assert_eq!(resp["type"], "error", "unexpected response: {resp:?}");
        assert_eq!(resp["code"], "BROWSER_URL_SCHEME_DENIED", "unexpected response: {resp:?}");
    }

    #[tokio::test]
    async fn test_browser_cli_open_rejects_javascript_scheme() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());
        let token = "test-token";

        let raw_req = format!(
            "{{\"command\":\"open\",\"url\":\"javascript:alert(1)\",\"token\":\"{token}\"}}"
        );
        let resp = send_raw_line(app.handle().clone(), Arc::clone(&manager), token, &raw_req).await;

        assert_eq!(resp["type"], "error", "unexpected response: {resp:?}");
        assert_eq!(resp["code"], "BROWSER_URL_SCHEME_DENIED", "unexpected response: {resp:?}");
    }

    #[tokio::test]
    async fn test_browser_cli_navigate_round_trip() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());
        let token = "test-token";

        // Navigate to unknown browserId -> Error BROWSER_NOT_FOUND
        let raw_req_unknown = format!(
            "{{\"command\":\"navigate\",\"browserId\":\"unknown-browser-id\",\"url\":\"https://example.com\",\"token\":\"{token}\"}}"
        );
        let resp_unknown = send_raw_line(
            app.handle().clone(),
            Arc::clone(&manager),
            token,
            &raw_req_unknown,
        )
        .await;
        assert_eq!(resp_unknown["type"], "error", "unexpected response: {resp_unknown:?}");
        assert_eq!(resp_unknown["code"], "BROWSER_NOT_FOUND", "unexpected response: {resp_unknown:?}");

        // Navigate existing (register one via manager.register_session) -> Navigated and url updates in list
        let registered = manager
            .register_session(CreateBrowserRequest {
                browser_id: None,
                workspace_id: None,
                worktree_path: None,
                url: "https://initial.example.com".to_string(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .expect("register session");

        let raw_req_existing = format!(
            "{{\"command\":\"navigate\",\"browserId\":\"{}\",\"url\":\"https://updated.example.com\",\"token\":\"{token}\"}}",
            registered.browser_id
        );
        let resp_existing = send_raw_line(
            app.handle().clone(),
            Arc::clone(&manager),
            token,
            &raw_req_existing,
        )
        .await;
        assert_eq!(resp_existing["type"], "navigated", "unexpected response: {resp_existing:?}");

        // List confirms updated url
        let list_req = format!("{{\"command\":\"list\",\"token\":\"{token}\"}}");
        let list_resp = send_raw_line(app.handle().clone(), Arc::clone(&manager), token, &list_req).await;
        let sessions = list_resp["sessions"].as_array().expect("sessions array");
        let found = sessions.iter().find(|s| s["browserId"] == registered.browser_id);
        assert!(found.is_some(), "session not found in list");
        assert!(
            found.unwrap()["url"] == "https://updated.example.com"
                || found.unwrap()["url"] == "https://updated.example.com/",
            "unexpected updated url: {:?}",
            found.unwrap()["url"]
        );
    }

    #[tokio::test]
    async fn test_browser_cli_close_round_trip() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());
        let token = "test-token";

        let registered = manager
            .register_session(CreateBrowserRequest {
                browser_id: None,
                workspace_id: None,
                worktree_path: None,
                url: "https://example.com".to_string(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .expect("register session");

        // Close existing -> Closed then list is empty
        let raw_req_close = format!(
            "{{\"command\":\"close\",\"browserId\":\"{}\",\"token\":\"{token}\"}}",
            registered.browser_id
        );
        let resp_close = send_raw_line(
            app.handle().clone(),
            Arc::clone(&manager),
            token,
            &raw_req_close,
        )
        .await;
        assert_eq!(resp_close["type"], "closed", "unexpected response: {resp_close:?}");

        let list_req = format!("{{\"command\":\"list\",\"token\":\"{token}\"}}");
        let list_resp = send_raw_line(app.handle().clone(), Arc::clone(&manager), token, &list_req).await;
        let sessions = list_resp["sessions"].as_array().expect("sessions array");
        assert!(sessions.is_empty(), "expected empty list after close, got: {sessions:?}");

        // Close again -> Error BROWSER_NOT_FOUND
        let resp_close_again = send_raw_line(
            app.handle().clone(),
            Arc::clone(&manager),
            token,
            &raw_req_close,
        )
        .await;
        assert_eq!(resp_close_again["type"], "error", "unexpected response: {resp_close_again:?}");
        assert_eq!(resp_close_again["code"], "BROWSER_NOT_FOUND", "unexpected response: {resp_close_again:?}");
    }

    #[tokio::test]
    async fn test_browser_cli_identify_round_trip() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());
        let token = "test-token";

        // Identify with empty manager -> Identified with None (browser == null)
        let raw_req_identify = format!("{{\"command\":\"identify\",\"token\":\"{token}\"}}");
        let resp_empty = send_raw_line(
            app.handle().clone(),
            Arc::clone(&manager),
            token,
            &raw_req_identify,
        )
        .await;
        assert_eq!(resp_empty["type"], "identified", "unexpected response: {resp_empty:?}");
        assert!(resp_empty["browser"].is_null(), "expected null browser for empty manager");

        // Identify with a registered visible session -> Identified with Some
        let registered = manager
            .register_session(CreateBrowserRequest {
                browser_id: None,
                workspace_id: None,
                worktree_path: None,
                url: "https://example.com".to_string(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .expect("register session");

        let resp_visible = send_raw_line(
            app.handle().clone(),
            Arc::clone(&manager),
            token,
            &raw_req_identify,
        )
        .await;
        assert_eq!(resp_visible["type"], "identified", "unexpected response: {resp_visible:?}");
        assert!(!resp_visible["browser"].is_null(), "expected Some browser for visible session");
        assert_eq!(resp_visible["browser"]["browserId"], registered.browser_id);
    }

    #[test]
    fn test_browser_cli_phase3_wire_serialization() {
        use crate::browser::model::{BrowserConsoleEntry, BrowserCookieEntry, BrowserWaitCondition};

        // Eval
        let req = BrowserCliRequest::Eval {
            browser_id: "b1".into(),
            script: "1 + 1".into(),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""command":"eval""#));
        assert!(json.contains(r#""browserId":"b1""#));
        let parsed: BrowserCliRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, req);

        // Wait
        let req = BrowserCliRequest::Wait {
            browser_id: "b1".into(),
            condition: BrowserWaitCondition::Selector {
                selector: "#main".into(),
            },
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""command":"wait""#));
        assert!(json.contains(r##""selector":"#main""##));
        let parsed: BrowserCliRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, req);

        // Console
        let req = BrowserCliRequest::Console {
            browser_id: "b1".into(),
            errors_only: Some(true),
            clear: Some(false),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""command":"console""#));
        let parsed: BrowserCliRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, req);

        // Focus
        let req = BrowserCliRequest::Focus {
            browser_id: "b1".into(),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""command":"focus""#));
        let parsed: BrowserCliRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, req);

        // Screenshot
        let req = BrowserCliRequest::Screenshot {
            browser_id: "b1".into(),
            out_path: "/tmp/shot.png".into(),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""command":"screenshot""#));
        assert!(json.contains(r#""outPath":"/tmp/shot.png""#));
        let parsed: BrowserCliRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, req);

        // Cookies
        let req = BrowserCliRequest::Cookies {
            browser_id: "b1".into(),
            action: "get".into(),
            name: Some("foo".into()),
            value: None,
            domain: None,
            path: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""command":"cookies""#));
        let parsed: BrowserCliRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, req);

        // Storage
        let req = BrowserCliRequest::Storage {
            browser_id: "b1".into(),
            kind: "local".into(),
            action: "get".into(),
            key: Some("theme".into()),
            value: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""command":"storage""#));
        let parsed: BrowserCliRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, req);

        // Responses
        let resp = BrowserCliResponse::Evaluated {
            result: Some("hello".into()),
            truncated: false,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains(r#""type":"evaluated""#));

        let resp = BrowserCliResponse::Waited;
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains(r#""type":"waited""#));

        let resp = BrowserCliResponse::Focused;
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains(r#""type":"focused""#));

        let resp = BrowserCliResponse::ScreenshotSaved {
            path: "/tmp/shot.png".into(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains(r#""type":"screenshotSaved""#));

        let resp = BrowserCliResponse::ConsoleEntries {
            entries: vec![BrowserConsoleEntry {
                level: "warn".into(),
                text: "careful".into(),
                at_ms: 1234,
            }],
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains(r#""type":"consoleEntries""#));

        let resp = BrowserCliResponse::CookieEntries {
            cookies: vec![BrowserCookieEntry {
                name: "c1".into(),
                value: "v1".into(),
            }],
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains(r#""type":"cookieEntries""#));

        let resp = BrowserCliResponse::StorageValue {
            value: Some("dark".into()),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains(r#""type":"storageValue""#));
    }

    #[tokio::test]
    async fn test_browser_cli_phase3_registered_missing_webview_round_trip() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let manager = Arc::new(BrowserManager::new());
        let token = "test-token";

        let registered = manager
            .register_session(CreateBrowserRequest {
                browser_id: None,
                workspace_id: None,
                worktree_path: None,
                url: "https://example.com".to_string(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .expect("register session");

        let b_id = &registered.browser_id;

        // Eval on registered session without webview -> WEBVIEW_NOT_FOUND
        let raw = format!(
            "{{\"command\":\"eval\",\"browserId\":\"{b_id}\",\"script\":\"1+1\",\"token\":\"{token}\"}}"
        );
        let resp = send_raw_line(app.handle().clone(), Arc::clone(&manager), token, &raw).await;
        assert_eq!(resp["type"], "error");
        assert!(resp["code"] == "WEBVIEW_NOT_FOUND" || resp["code"] == "BROWSER_WEBVIEW_NOT_FOUND");

        // Wait on registered session without webview -> WEBVIEW_NOT_FOUND
        let raw = format!(
            "{{\"command\":\"wait\",\"browserId\":\"{b_id}\",\"condition\":{{\"condition\":\"selector\",\"selector\":\"#none\"}},\"token\":\"{token}\"}}"
        );
        let resp = send_raw_line(app.handle().clone(), Arc::clone(&manager), token, &raw).await;
        assert_eq!(resp["type"], "error");
        assert!(resp["code"] == "WEBVIEW_NOT_FOUND" || resp["code"] == "BROWSER_WEBVIEW_NOT_FOUND");

        // Console on registered session without webview -> WEBVIEW_NOT_FOUND
        let raw = format!(
            "{{\"command\":\"console\",\"browserId\":\"{b_id}\",\"token\":\"{token}\"}}"
        );
        let resp = send_raw_line(app.handle().clone(), Arc::clone(&manager), token, &raw).await;
        assert_eq!(resp["type"], "error");
        assert!(resp["code"] == "WEBVIEW_NOT_FOUND" || resp["code"] == "BROWSER_WEBVIEW_NOT_FOUND");

        // Focus on registered session without webview -> WEBVIEW_NOT_FOUND
        let raw = format!(
            "{{\"command\":\"focus\",\"browserId\":\"{b_id}\",\"token\":\"{token}\"}}"
        );
        let resp = send_raw_line(app.handle().clone(), Arc::clone(&manager), token, &raw).await;
        assert_eq!(resp["type"], "error");
        assert!(resp["code"] == "WEBVIEW_NOT_FOUND" || resp["code"] == "BROWSER_WEBVIEW_NOT_FOUND");

        // Screenshot on registered session without webview -> WEBVIEW_NOT_FOUND
        let raw = format!(
            "{{\"command\":\"screenshot\",\"browserId\":\"{b_id}\",\"outPath\":\"/tmp/test.png\",\"token\":\"{token}\"}}"
        );
        let resp = send_raw_line(app.handle().clone(), Arc::clone(&manager), token, &raw).await;
        assert_eq!(resp["type"], "error");
        assert!(resp["code"] == "WEBVIEW_NOT_FOUND" || resp["code"] == "BROWSER_WEBVIEW_NOT_FOUND");

        // Cookies on registered session without webview -> WEBVIEW_NOT_FOUND
        let raw = format!(
            "{{\"command\":\"cookies\",\"browserId\":\"{b_id}\",\"action\":\"get\",\"token\":\"{token}\"}}"
        );
        let resp = send_raw_line(app.handle().clone(), Arc::clone(&manager), token, &raw).await;
        assert_eq!(resp["type"], "error");
        assert!(resp["code"] == "WEBVIEW_NOT_FOUND" || resp["code"] == "BROWSER_WEBVIEW_NOT_FOUND");

        // Storage on registered session without webview -> WEBVIEW_NOT_FOUND
        let raw = format!(
            "{{\"command\":\"storage\",\"browserId\":\"{b_id}\",\"kind\":\"local\",\"action\":\"get\",\"key\":\"k\",\"token\":\"{token}\"}}"
        );
        let resp = send_raw_line(app.handle().clone(), Arc::clone(&manager), token, &raw).await;
        assert_eq!(resp["type"], "error");
        assert!(resp["code"] == "WEBVIEW_NOT_FOUND" || resp["code"] == "BROWSER_WEBVIEW_NOT_FOUND");
    }
}
