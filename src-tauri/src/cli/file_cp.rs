//! `ferryx file cp`: copy one file to or from a paired host.
//!
//! The bytes ride the paired-host operation the daemon already sends over direct QUIC,
//! falling back to the relay. This is not OpenSSH scp.
use crate::daemon::protocol::{DaemonRequest, DaemonResponse, DAEMON_PROTOCOL_VERSION};
use crate::paired_host::client::{Operation, OperationRequest, OperationResult};
use crate::paired_host::inventory::{AuthStatus, HostView};
use crate::remote::machine_protocol as m;
use base64::Engine as _;
use serde_json::{json, Value};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};

const EXIT_OK: i32 = 0;
const EXIT_INVALID: i32 = 2;
const EXIT_NOT_FOUND: i32 = 3;
const EXIT_REFUSED: i32 = 4;
const EXIT_FAILURE: i32 = 5;

pub const FILE_CP_USAGE: &str = "usage: ferryx-cli file cp <src> <dst> [--json]\n       one side is <host>:<path> (paired host id or label; path is absolute or ~/)";

#[derive(Debug, PartialEq, Eq)]
pub struct FileCp {
    pub host: String,
    pub remote_path: String,
    pub local_path: PathBuf,
    pub put: bool,
    pub json: bool,
}

pub fn parse_file_cp(args: &[String]) -> Result<FileCp, String> {
    if args.get(1).map(String::as_str) != Some("file") || args.get(2).map(String::as_str) != Some("cp") {
        return Err(FILE_CP_USAGE.into());
    }
    let mut json = false;
    let mut positionals = Vec::new();
    for arg in args.iter().skip(3) {
        if arg == "--json" {
            json = true;
        } else if arg.starts_with('-') {
            return Err(format!("unknown flag {arg}\n{FILE_CP_USAGE}"));
        } else {
            positionals.push(arg.clone());
        }
    }
    if positionals.len() != 2 {
        return Err(FILE_CP_USAGE.into());
    }
    let src = split_remote(&positionals[0]);
    let dst = split_remote(&positionals[1]);
    match (src, dst) {
        (None, Some((host, remote_path))) => Ok(FileCp {
            host,
            remote_path,
            local_path: PathBuf::from(&positionals[0]),
            put: true,
            json,
        }),
        (Some((host, remote_path)), None) => Ok(FileCp {
            host,
            remote_path,
            local_path: PathBuf::from(&positionals[1]),
            put: false,
            json,
        }),
        _ => Err(format!("exactly one of src or dst must be <host>:<path>\n{FILE_CP_USAGE}")),
    }
}

fn split_remote(spec: &str) -> Option<(String, String)> {
    let (host, path) = spec.split_once(':')?;
    if host.is_empty() || path.is_empty() || host.contains('/') || host.contains('\\') {
        return None;
    }
    if host.len() == 1 && host.as_bytes()[0].is_ascii_alphabetic() {
        return None;
    }
    if !remote_path_ok(path) {
        return None;
    }
    Some((host.to_string(), path.to_string()))
}

fn remote_path_ok(path: &str) -> bool {
    if path == "~" || path.starts_with("~/") || path.starts_with('/') {
        return true;
    }
    let bytes = path.as_bytes();
    bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && matches!(bytes[2], b'\\' | b'/')
}

pub fn run_file_cli(args: &[String]) -> i32 {
    let command = match parse_file_cp(args) {
        Ok(command) => command,
        Err(message) => {
            let (value, _) = failure("INVALID_REQUEST", &message);
            return finish(value, EXIT_INVALID, false);
        }
    };
    let json = command.json;
    match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => {
            let (value, code) = runtime.block_on(copy_file(command));
            finish(value, code, json)
        }
        Err(error) => finish(failure("INTERNAL", &error.to_string()).0, EXIT_FAILURE, json),
    }
}

fn finish(value: Value, code: i32, json: bool) -> i32 {
    if json || code != EXIT_OK {
        println!("{value}");
    } else if let Some(message) = value.get("message").and_then(Value::as_str) {
        println!("{message}");
    }
    code
}

fn failure(code: &str, message: &str) -> (Value, i32) {
    (json!({ "ok": false, "code": code, "message": message }), EXIT_FAILURE)
}

async fn copy_file(command: FileCp) -> (Value, i32) {
    let stream = match connect_daemon().await {
        Ok(stream) => stream,
        Err(failed) => return failed,
    };
    let (read_half, mut write_half) = tokio::io::split(stream);
    let mut reader = tokio::io::BufReader::new(read_half);
    if let Err(message) = handshake(&mut reader, &mut write_half).await {
        return failure("INTERNAL", &message);
    }
    match exchange(&mut reader, &mut write_half, &DaemonRequest::GetCapabilities).await {
        Ok(DaemonResponse::CapabilitiesOk { capabilities }) => {
            if let Some(missing) = required_capabilities(&command, &capabilities) {
                return (json!({ "ok": false, "code": "UNSUPPORTED", "message": format!("this daemon does not advertise {missing}; restart it") }), EXIT_REFUSED);
            }
        }
        Ok(other) => return failure("INTERNAL", &format!("unexpected capabilities response: {other:?}")),
        Err(message) => return failure("INTERNAL", &message),
    }
    let hosts = match exchange(&mut reader, &mut write_half, &DaemonRequest::PairedHostList).await {
        Ok(DaemonResponse::PairedHostListOk { hosts }) => hosts,
        Ok(DaemonResponse::PairedHostError { error }) => return map_service(&error),
        Ok(other) => return failure("INTERNAL", &format!("unexpected host list: {other:?}")),
        Err(message) => return failure("INTERNAL", &message),
    };
    let Some(host) = resolve_host(&hosts, &command.host) else {
        return (json!({ "ok": false, "code": "NOT_FOUND", "message": format!("no paired host named {}", command.host) }), EXIT_NOT_FOUND);
    };
    if host.auth_status != AuthStatus::Paired {
        return (json!({ "ok": false, "code": "UNAUTHORIZED", "message": "machine authorization required for this paired host" }), EXIT_REFUSED);
    }
    let result = if command.put {
        if std::fs::symlink_metadata(&command.local_path).is_ok_and(|meta| meta.is_dir()) {
            put_tree(&mut reader, &mut write_half, &host, &command.remote_path, &command.local_path, 0).await
        } else {
            put_file(&mut reader, &mut write_half, &host, &command.local_path, &command.remote_path).await
        }
    } else {
        get_any(&mut reader, &mut write_half, &host, &command.local_path, &command.remote_path).await
    };
    result
}

fn required_capabilities(command: &FileCp, capabilities: &[String]) -> Option<&'static str> {
    let mut needed = vec![if command.put {
        crate::paired_host::file_write::PAIRED_FILE_WRITE_PROXY_CAPABILITY
    } else {
        crate::paired_host::file_read::PAIRED_FILE_READ_PROXY_CAPABILITY
    }];
    if command.put && std::fs::symlink_metadata(&command.local_path).is_ok_and(|meta| meta.is_dir()) {
        needed.push(crate::paired_host::file_tree::PAIRED_FILE_MKDIR_PROXY_CAPABILITY);
    }
    if !command.put {
        needed.push(crate::paired_host::file_tree::PAIRED_FILE_LIST_PROXY_CAPABILITY);
    }
    needed.into_iter().find(|need| !capabilities.iter().any(|cap| cap == *need))
}

fn resolve_host(hosts: &[HostView], name: &str) -> Option<HostView> {
    hosts.iter().find(|host| host.host_id == name || host.display_label == name).cloned()
}

async fn put_file<R, W>(reader: &mut tokio::io::BufReader<R>, writer: &mut W, host: &HostView, local: &Path, remote: &str) -> (Value, i32)
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut file = match std::fs::File::open(local) {
        Ok(file) => file,
        Err(error) => return failure("NOT_FOUND", &format!("cannot read {}: {error}", local.display())),
    };
    let total = match file.metadata() {
        Ok(meta) if meta.is_file() => meta.len(),
        Ok(_) => return failure("INVALID_REQUEST", "source is not a regular file"),
        Err(error) => return failure("INTERNAL", &error.to_string()),
    };
    let mut upload_id = uuid::Uuid::new_v4().to_string();
    let mut restarted = false;
    let mut offset = 0u64;
    let chunk_max = m::FILE_WRITE_CHUNK_MAX_BYTES as usize;
    loop {
        let mut buf = vec![0u8; chunk_max];
        let read = if total == 0 && offset == 0 {
            0
        } else {
            match file.read(&mut buf) {
                Ok(0) if offset != total => return failure("INTERNAL", "local file ended before its declared length"),
                Ok(n) => n,
                Err(error) => return failure("INTERNAL", &error.to_string()),
            }
        };
        let bytes = if total == 0 { &[][..] } else { &buf[..read] };
        let request = m::FileWriteRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            path: remote.to_string(),
            upload_id: upload_id.clone(),
            offset,
            total_bytes: total,
            data: base64::engine::general_purpose::STANDARD.encode(bytes),
        };
        match operation(reader, writer, host, Operation::FileWrite { request }).await {
            Ok(OperationResult::FileWrite(result)) if result.complete => {
                return ok_copy(total, &result.path, "put");
            }
            Ok(OperationResult::FileWrite(_)) if offset + bytes.len() as u64 == total => {
                return failure("INTERNAL", "the paired host did not publish the file");
            }
            Ok(OperationResult::FileWrite(_)) => {
                offset += bytes.len() as u64;
            }
            Err(error) if error_code(&error) == "STAGING_LOST" && !restarted => {
                restarted = true;
                upload_id = uuid::Uuid::new_v4().to_string();
                offset = 0;
                if file.seek(SeekFrom::Start(0)).is_err() {
                    return failure("INTERNAL", "could not restart the local read");
                }
                continue;
            }
            Err(message) => return failure("INTERNAL", &message),
            Ok(_) => return failure("INTERNAL", "paired host answered a different operation"),
        }
        if total == 0 {
            break;
        }
    }
    ok_copy(total, remote, "put")
}

async fn get_file<R, W>(reader: &mut tokio::io::BufReader<R>, writer: &mut W, host: &HostView, local: &Path, remote: &str) -> (Value, i32)
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let parent = local.parent().unwrap_or_else(|| Path::new("."));
    if !parent.as_os_str().is_empty() && !parent.exists() {
        return failure("NOT_FOUND", &format!("local directory {} does not exist", parent.display()));
    }
    let partial = parent.join(format!(".{}.ferryx-partial", uuid::Uuid::new_v4()));
    let mut out = match std::fs::File::create(&partial) {
        Ok(file) => file,
        Err(error) => return failure("INTERNAL", &error.to_string()),
    };
    let mut offset = 0u64;
    let mut version = String::new();
    loop {
        let request = m::FileReadRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            session_id: None,
            cwd: None,
            path: remote.to_string(),
            offset,
            length: m::FILE_READ_CHUNK_MAX_BYTES,
            copy: true,
        };
        let page = match operation(reader, writer, host, Operation::FileRead { request }).await {
            Ok(OperationResult::FileRead(page)) => page,
            Err(message) => {
                let _ = std::fs::remove_file(&partial);
                return failure("INTERNAL", &message);
            }
            Ok(_) => {
                let _ = std::fs::remove_file(&partial);
                return failure("INTERNAL", "paired host answered a different operation");
            }
        };
        if offset == 0 {
            version = page.version.clone();
        } else if !page.version.is_empty() && page.version != version {
            let _ = std::fs::remove_file(&partial);
            return failure("INTERNAL", "remote file changed during the copy");
        }
        let bytes = match base64::engine::general_purpose::STANDARD.decode(page.data) {
            Ok(bytes) => bytes,
            Err(_) => {
                let _ = std::fs::remove_file(&partial);
                return failure("INTERNAL", "remote file page was not base64");
            }
        };
        if out.write_all(&bytes).is_err() {
            let _ = std::fs::remove_file(&partial);
            return failure("INTERNAL", "failed to write the local copy");
        }
        offset += bytes.len() as u64;
        if page.eof || offset >= page.total_bytes {
            break;
        }
    }
    if out.sync_all().is_err() {
        let _ = std::fs::remove_file(&partial);
        return failure("INTERNAL", "failed to flush the local copy");
    }
    drop(out);
    // rename replaces an existing file in one step on every target (Windows: MoveFileExW
    // with MOVEFILE_REPLACE_EXISTING), so the destination is never missing.
    if std::fs::rename(&partial, local).is_err() {
        let _ = std::fs::remove_file(&partial);
        return failure("INTERNAL", "failed to publish the local copy");
    }
    ok_copy(offset, remote, "get")
}

fn join_remote(parent: &str, name: &str) -> Result<String, String> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\\') || name.chars().any(char::is_control) {
        return Err(format!("refusing child name {name}"));
    }
    if parent == "~" {
        return Ok(format!("~/{name}"));
    }
    let sep = if parent.contains('\\') && !parent.contains('/') { '\\' } else { '/' };
    if parent.ends_with(sep) { Ok(format!("{parent}{name}")) } else { Ok(format!("{parent}{sep}{name}")) }
}

/// Joins a remote entry name under the local root. The name must be one plain component
/// here too: on Windows a name such as C:x carries a drive prefix that would replace the root.
fn local_child(local: &Path, name: &str) -> Result<PathBuf, String> {
    let mut components = Path::new(name).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Ok(local.join(name)),
        _ => Err(format!("refusing child name {name}")),
    }
}

/// The machine error code an operation failure carries (its first word).
fn error_code(message: &str) -> &str {
    message.split(' ').next().unwrap_or("")
}

const TREE_DEPTH_MAX: u32 = 64;
struct Copied { bytes: u64, files: u64, dirs: u64 }

fn ok_tree(copied: Copied, path: &str, direction: &str) -> (Value, i32) {
    (json!({ "ok": true, "direction": direction, "bytes": copied.bytes, "files": copied.files, "directories": copied.dirs, "path": path, "message": format!("copied {} files ({} bytes) {direction} {path}", copied.files, copied.bytes) }), EXIT_OK)
}

fn ensure_local_dir(path: &Path) -> Result<(), (Value, i32)> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(failure("UNSUPPORTED_PATH", "refusing to copy onto a symlink")),
        Ok(meta) if meta.is_dir() => Ok(()),
        Ok(_) => Err(failure("INVALID_REQUEST", "local destination is not a directory")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = path.parent().unwrap_or_else(|| Path::new("."));
            if !parent.as_os_str().is_empty() && !parent.exists() {
                return Err(failure("NOT_FOUND", &format!("local directory {} does not exist", parent.display())));
            }
            std::fs::create_dir(path).map_err(|error| failure("INTERNAL", &error.to_string()))
        }
        Err(error) => Err(failure("INTERNAL", &error.to_string())),
    }
}

async fn mkdir_remote<R, W>(reader: &mut tokio::io::BufReader<R>, writer: &mut W, host: &HostView, path: &str) -> Result<(), String>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let request = m::FileMkdirRequest { request_id: uuid::Uuid::new_v4().to_string(), path: path.to_string() };
    match operation(reader, writer, host, Operation::FileMkdir { request }).await {
        Ok(OperationResult::FileMkdir(_)) => Ok(()),
        Ok(_) => Err("paired host answered a different operation".into()),
        Err(message) => Err(message),
    }
}

async fn list_all<R, W>(reader: &mut tokio::io::BufReader<R>, writer: &mut W, host: &HostView, path: &str) -> Result<Vec<m::FileListEntry>, String>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut cursor = None;
    let mut entries = Vec::new();
    loop {
        let request = m::FileListRequest { request_id: uuid::Uuid::new_v4().to_string(), path: path.to_string(), cursor: cursor.clone() };
        match operation(reader, writer, host, Operation::FileList { request }).await {
            Ok(OperationResult::FileList(page)) => {
                let next = page.entries.last().map(|entry| entry.name.clone());
                let truncated = page.truncated;
                entries.extend(page.entries);
                if !truncated { return Ok(entries); }
                let Some(next) = next else { return Err("directory page was truncated with no entries".into()); };
                cursor = Some(next);
            }
            Ok(_) => return Err("paired host answered a different operation".into()),
            Err(message) => return Err(message),
        }
    }
}

async fn put_tree<R, W>(reader: &mut tokio::io::BufReader<R>, writer: &mut W, host: &HostView, remote: &str, local: &Path, depth: u32) -> (Value, i32)
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    match put_tree_counted(reader, writer, host, remote, local, depth).await {
        Ok(copied) => ok_tree(copied, remote, "put"),
        Err(failed) => failed,
    }
}

async fn put_tree_counted<R, W>(reader: &mut tokio::io::BufReader<R>, writer: &mut W, host: &HostView, remote: &str, local: &Path, depth: u32) -> Result<Copied, (Value, i32)>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    if depth > TREE_DEPTH_MAX { return Err(failure("INVALID_REQUEST", "directory is too deep")); }
    let meta = std::fs::symlink_metadata(local).map_err(|error| failure("NOT_FOUND", &error.to_string()))?;
    if meta.file_type().is_symlink() { return Err(failure("UNSUPPORTED_PATH", &format!("refusing to copy symlink {}", local.display()))); }
    if meta.is_file() {
        let (value, code) = put_file(reader, writer, host, local, remote).await;
        if code != EXIT_OK { return Err((value, code)); }
        return Ok(Copied { bytes: value.get("bytes").and_then(Value::as_u64).unwrap_or(0), files: 1, dirs: 0 });
    }
    if !meta.is_dir() { return Err(failure("UNSUPPORTED_PATH", &format!("refusing to copy {}", local.display()))); }
    mkdir_remote(reader, writer, host, remote).await.map_err(|message| failure("INTERNAL", &message))?;
    let mut copied = Copied { bytes: 0, files: 0, dirs: 1 };
    for child in std::fs::read_dir(local).map_err(|error| failure("INTERNAL", &error.to_string()))? {
        let child = child.map_err(|error| failure("INTERNAL", &error.to_string()))?;
        let name = child.file_name().into_string().map_err(|_| failure("INVALID_PATH", "a child name is not utf-8"))?;
        let child_remote = join_remote(remote, &name).map_err(|message| failure("INVALID_PATH", &message))?;
        let nested = Box::pin(put_tree_counted(reader, writer, host, &child_remote, &local.join(name), depth + 1)).await?;
        copied.bytes += nested.bytes;
        copied.files += nested.files;
        copied.dirs += nested.dirs;
    }
    Ok(copied)
}

async fn get_any<R, W>(reader: &mut tokio::io::BufReader<R>, writer: &mut W, host: &HostView, local: &Path, remote: &str) -> (Value, i32)
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    match list_all(reader, writer, host, remote).await {
        Ok(_) => match get_tree_counted(reader, writer, host, remote, local, 0).await {
            Ok(copied) => ok_tree(copied, remote, "get"),
            Err(failed) => failed,
        },
        Err(message) if error_code(&message) == "NOT_DIRECTORY" => get_file(reader, writer, host, local, remote).await,
        Err(message) => failure("INTERNAL", &message),
    }
}

async fn get_tree_counted<R, W>(reader: &mut tokio::io::BufReader<R>, writer: &mut W, host: &HostView, remote: &str, local: &Path, depth: u32) -> Result<Copied, (Value, i32)>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    if depth > TREE_DEPTH_MAX { return Err(failure("INVALID_REQUEST", "directory is too deep")); }
    ensure_local_dir(local)?;
    let entries = list_all(reader, writer, host, remote).await.map_err(|message| failure("INTERNAL", &message))?;
    let mut copied = Copied { bytes: 0, files: 0, dirs: 1 };
    for entry in entries {
        let child_remote = join_remote(remote, &entry.name).map_err(|message| failure("INVALID_PATH", &message))?;
        let child_local = local_child(local, &entry.name).map_err(|message| failure("INVALID_PATH", &message))?;
        match entry.kind {
            m::FileKind::Dir => {
                let nested = Box::pin(get_tree_counted(reader, writer, host, &child_remote, &child_local, depth + 1)).await?;
                copied.bytes += nested.bytes; copied.files += nested.files; copied.dirs += nested.dirs;
            }
            m::FileKind::File => {
                let (value, code) = get_file(reader, writer, host, &child_local, &child_remote).await;
                if code != EXIT_OK { return Err((value, code)); }
                copied.bytes += value.get("bytes").and_then(Value::as_u64).unwrap_or(0);
                copied.files += 1;
            }
            m::FileKind::Symlink | m::FileKind::Other => return Err(failure("UNSUPPORTED_PATH", &format!("refusing to copy {}", entry.name))),
        }
    }
    Ok(copied)
}

fn ok_copy(bytes: u64, path: &str, direction: &str) -> (Value, i32) {
    (json!({
        "ok": true,
        "direction": direction,
        "bytes": bytes,
        "path": path,
        "message": format!("copied {bytes} bytes {direction} {path}"),
    }), EXIT_OK)
}

async fn operation<R, W>(
    reader: &mut tokio::io::BufReader<R>,
    writer: &mut W,
    host: &HostView,
    operation: Operation,
) -> Result<OperationResult, String>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let response = exchange(reader, writer, &DaemonRequest::PairedHostOperation {
        request: OperationRequest {
            host_id: host.host_id.clone(),
            generation: host.generation,
            operation,
        },
    }).await?;
    match response {
        DaemonResponse::PairedHostOperationOk { response } => Ok(response.result),
        DaemonResponse::PairedHostOperationError { error } => {
            let machine = error.machine_error.as_ref().map(|item| item.code.as_str()).unwrap_or("");
            Err(format!("{} {machine}", error.code))
        }
        other => Err(format!("unexpected operation response: {other:?}")),
    }
}

fn map_service(error: &crate::paired_host::service::ServiceError) -> (Value, i32) {
    (json!({ "ok": false, "code": error.code, "message": error.message }), EXIT_REFUSED)
}

async fn handshake<R, W>(reader: &mut tokio::io::BufReader<R>, writer: &mut W) -> Result<(), String>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    match exchange(reader, writer, &DaemonRequest::Handshake {
        version: DAEMON_PROTOCOL_VERSION,
        token: crate::daemon::client::read_transport_token(),
    }).await? {
        DaemonResponse::HandshakeOk { .. } => Ok(()),
        DaemonResponse::ProtocolMismatch { .. } => Err("the daemon speaks another protocol version".into()),
        other => Err(format!("unexpected handshake response: {other:?}")),
    }
}

async fn exchange<R, W>(reader: &mut tokio::io::BufReader<R>, writer: &mut W, request: &DaemonRequest) -> Result<DaemonResponse, String>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let mut line = serde_json::to_string(request).map_err(|error| error.to_string())?;
    line.push('\n');
    writer.write_all(line.as_bytes()).await.map_err(|error| error.to_string())?;
    writer.flush().await.map_err(|error| error.to_string())?;
    let mut response = String::new();
    if reader.read_line(&mut response).await.map_err(|error| error.to_string())? == 0 {
        return Err("the daemon closed the connection".into());
    }
    serde_json::from_str(response.trim()).map_err(|error| format!("unreadable daemon response: {error}"))
}

#[cfg(unix)]
async fn connect_daemon() -> Result<tokio::net::UnixStream, (Value, i32)> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    let path = crate::daemon::server::get_socket_path();
    let not_running = || (json!({ "ok": false, "code": "NOT_FOUND", "message": format!("no Ferryx daemon is listening at {}", path.display()) }), EXIT_NOT_FOUND);
    let metadata = std::fs::symlink_metadata(&path).map_err(|_| not_running())?;
    // SAFETY: getuid takes no arguments, dereferences nothing and cannot fail.
    let uid = unsafe { libc::getuid() };
    if !metadata.file_type().is_socket() || metadata.uid() != uid {
        return Err((json!({ "ok": false, "code": "FORBIDDEN", "message": "the daemon socket is not a socket owned by this user" }), EXIT_REFUSED));
    }
    tokio::net::UnixStream::connect(&path).await.map_err(|_| not_running())
}

#[cfg(not(unix))]
async fn connect_daemon() -> Result<tokio::net::TcpStream, (Value, i32)> {
    let path = crate::daemon::server::get_socket_path();
    let not_running = || (json!({ "ok": false, "code": "NOT_FOUND", "message": "no Ferryx daemon is listening" }), EXIT_NOT_FOUND);
    let port: u16 = tokio::fs::read_to_string(&path).await.ok().and_then(|text| text.trim().parse().ok()).ok_or_else(not_running)?;
    tokio::net::TcpStream::connect(("127.0.0.1", port)).await.map_err(|_| not_running())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_put_and_get_and_rejects_two_remotes() {
        let put = parse_file_cp(&["ferryx".into(), "file".into(), "cp".into(), "./note.txt".into(), "omarchy:~/note.txt".into()]).unwrap();
        assert!(put.put);
        assert_eq!(put.host, "omarchy");
        assert_eq!(put.remote_path, "~/note.txt");
        let get = parse_file_cp(&["ferryx".into(), "file".into(), "cp".into(), "box:/tmp/a.txt".into(), "C:/local/a.txt".into(), "--json".into()]).unwrap();
        assert!(!get.put);
        assert!(get.json);
        assert_eq!(get.local_path, PathBuf::from("C:/local/a.txt"));
        assert!(parse_file_cp(&["ferryx".into(), "file".into(), "cp".into(), "a:~/x".into(), "b:~/y".into()]).is_err());
        assert!(parse_file_cp(&["ferryx".into(), "file".into(), "cp".into(), "note.txt".into(), "other.txt".into()]).is_err());
    }

    #[test]
    fn join_remote_uses_the_caller_path_and_refuses_a_climb() {
        assert_eq!(join_remote("~/notes", "a.txt").unwrap(), "~/notes/a.txt");
        assert_eq!(join_remote("~", "a.txt").unwrap(), "~/a.txt");
        assert_eq!(join_remote("C:\\notes", "a.txt").unwrap(), "C:\\notes\\a.txt");
        assert!(join_remote("/tmp", "..").is_err());
        assert!(join_remote("/tmp", "a/b").is_err());
    }

    #[test]
    fn local_child_stays_under_the_root() {
        let root = std::env::temp_dir();
        assert_eq!(local_child(&root, "a.txt").unwrap(), root.join("a.txt"));
        assert!(local_child(&root, "..").is_err());
        assert!(local_child(&root, "a/b").is_err());
        assert!(local_child(&root, "/etc").is_err());
        #[cfg(windows)]
        {
            assert!(local_child(&root, "C:x").is_err());
            assert!(local_child(&root, "C:\\x").is_err());
        }
    }

    fn host() -> HostView {
        HostView {
            host_id: "h".into(),
            relay_origin: "https://relay.ferryx.dev".into(),
            machine_id: "m".into(),
            display_label: "box".into(),
            grant_scope: crate::paired_host::inventory::GrantScope::Machine,
            generation: crate::scoped_contracts::Epoch(1),
            auth_status: AuthStatus::Paired,
            online: true,
            ssh_host_id: None,
            device_id: None,
            unavailable_reason: None,
            published_origins: Vec::new(),
        }
    }

    /// The host's real refusal, carried through the client's error projection and the daemon
    /// wire, exactly as a live copy sees it.
    fn host_refusal(code: &str) -> DaemonResponse {
        let body = serde_json::to_vec(&m::ErrorEnvelope {
            error: m::MachineError {
                code: code.into(),
                message: code.into(),
                retryable: false,
                request_id: "3941b9de-b16d-4d9a-ae0a-118f90fd91f4".into(),
                details: serde_json::Map::new(),
            },
        })
        .unwrap();
        DaemonResponse::PairedHostOperationError {
            error: crate::paired_host::client::map_operation_http_error(reqwest::StatusCode::BAD_REQUEST, None, None, &body),
        }
    }

    fn ok(result: OperationResult) -> DaemonResponse {
        DaemonResponse::PairedHostOperationOk {
            response: crate::paired_host::client::OperationResponse {
                host_id: "h".into(),
                generation: crate::scoped_contracts::Epoch(1),
                result,
            },
        }
    }

    type Reader = tokio::io::BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>;
    type Writer = tokio::io::WriteHalf<tokio::io::DuplexStream>;

    /// Answers every operation the copy sends with answer, as the daemon would.
    fn fake_daemon<F>(answer: F) -> (Reader, Writer, tokio::task::JoinHandle<()>)
    where
        F: Fn(Operation) -> DaemonResponse + Send + 'static,
    {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
        let (client, server) = tokio::io::duplex(256 * 1024);
        let daemon = tokio::spawn(async move {
            let (read_half, mut write_half) = tokio::io::split(server);
            let mut lines = tokio::io::BufReader::new(read_half).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let response = match serde_json::from_str::<DaemonRequest>(&line).unwrap() {
                    DaemonRequest::PairedHostOperation { request } => answer(request.operation),
                    other => panic!("unexpected request {other:?}"),
                };
                let mut text = serde_json::to_string(&response).unwrap();
                text.push('\n');
                write_half.write_all(text.as_bytes()).await.unwrap();
            }
        });
        let (read_half, write_half) = tokio::io::split(client);
        (tokio::io::BufReader::new(read_half), write_half, daemon)
    }

    #[tokio::test]
    async fn get_of_a_plain_file_falls_back_from_the_host_not_directory_refusal() {
        let dir = tempfile::tempdir().unwrap();
        let local = dir.path().join("a.txt");
        let (mut reader, mut writer, daemon) = fake_daemon(|operation| match operation {
            Operation::FileList { .. } => host_refusal("NOT_DIRECTORY"),
            Operation::FileRead { request } => {
                assert!(request.copy, "a copy read must refuse symlinks and skip the viewer cap");
                ok(OperationResult::FileRead(m::FileReadResult {
                    resolved_path: "/h/a.txt".into(),
                    total_bytes: 5,
                    offset: 0,
                    data: base64::engine::general_purpose::STANDARD.encode(b"hello"),
                    eof: true,
                    version: "v1".into(),
                }))
            }
            other => panic!("unexpected operation {other:?}"),
        });
        let (value, code) = get_any(&mut reader, &mut writer, &host(), &local, "~/a.txt").await;
        daemon.abort();
        assert_eq!(code, EXIT_OK, "{value}");
        assert_eq!(std::fs::read(&local).unwrap(), b"hello");
    }

    #[tokio::test]
    async fn put_restarts_once_when_the_host_reports_staging_lost() {
        let dir = tempfile::tempdir().unwrap();
        let local = dir.path().join("a.txt");
        std::fs::write(&local, b"hello").unwrap();
        let uploads = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = uploads.clone();
        let (mut reader, mut writer, daemon) = fake_daemon(move |operation| match operation {
            Operation::FileWrite { request } => {
                let mut seen = seen.lock().unwrap();
                seen.push(request.upload_id.clone());
                if seen.len() == 1 {
                    host_refusal("STAGING_LOST")
                } else {
                    ok(OperationResult::FileWrite(m::FileWriteResult { path: "/h/a.txt".into(), offset: 0, complete: true }))
                }
            }
            other => panic!("unexpected operation {other:?}"),
        });
        let (value, code) = put_file(&mut reader, &mut writer, &host(), &local, "~/a.txt").await;
        daemon.abort();
        assert_eq!(code, EXIT_OK, "{value}");
        let uploads = uploads.lock().unwrap();
        assert_eq!(uploads.len(), 2);
        assert_ne!(uploads[0], uploads[1], "the restart uses a fresh upload id");
    }
}
