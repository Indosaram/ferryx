//! Paired-host file read: host-side range serving and desktop-side chunked fetch into the
//! remote-file cache. The peer already holds terminal control of the host, so this adds no
//! authority beyond what the terminal grants; admission matches paste-upload.
use crate::daemon::client::DaemonClient;
use crate::ipc::{run_blocking, IpcError, IpcErrorCode};
use crate::paired_host::client::{ClientError, Operation, OperationRequest, OperationResult};
use crate::remote::machine_protocol as m;
use base64::Engine as _;
use std::future::Future;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

pub const PAIRED_FILE_READ_CAPABILITY: &str = "pairedFileReadV1";
/// Advertised by a local daemon whose protocol carries [Operation::FileRead].
pub const PAIRED_FILE_READ_PROXY_CAPABILITY: &str = "pairedFileReadProxyV1";
const MAX_PATH_CHARS: usize = 4096;

/// Typed host-side refusal; `code()` is the machine error code put on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostReadError {
    InvalidRequest,
    InvalidPath,
    NotFound,
    NotRegularFile,
    TooLarge,
    Denied,
    Unavailable,
}

impl HostReadError {
    pub fn code(self) -> &'static str {
        match self {
            Self::InvalidRequest => "INVALID_REQUEST",
            Self::InvalidPath => "INVALID_PATH",
            Self::NotFound => "NOT_FOUND",
            Self::NotRegularFile => "UNSUPPORTED_PATH",
            Self::TooLarge => "PAYLOAD_TOO_LARGE",
            Self::Denied => "PERMISSION_DENIED",
            Self::Unavailable => "MACHINE_SERVICE_UNAVAILABLE",
        }
    }
    pub fn status(self) -> u16 {
        match self {
            Self::InvalidRequest | Self::InvalidPath | Self::NotRegularFile => 400,
            Self::NotFound => 404,
            Self::TooLarge => 413,
            Self::Denied => 403,
            Self::Unavailable => 503,
        }
    }
}

fn io_error(error: std::io::Error) -> HostReadError {
    match error.kind() {
        std::io::ErrorKind::NotFound => HostReadError::NotFound,
        std::io::ErrorKind::PermissionDenied => HostReadError::Denied,
        _ => HostReadError::Unavailable,
    }
}

/// True when `path` needs no base directory: `~`, `~/...`, or absolute.
pub fn is_home_or_absolute(path: &str) -> bool {
    path == "~" || path.starts_with("~/") || Path::new(path).is_absolute()
}

/// Lexically resolves a terminal path token on the host. `~` expands to `home`; a relative
/// path joins `cwd` (must be absolute) or else `session_base`; `..` may not climb above root.
pub fn resolve_host_path(
    input: &str,
    cwd: Option<&str>,
    home: Option<&Path>,
    session_base: Option<&Path>,
) -> Result<PathBuf, HostReadError> {
    if input.is_empty() || input.len() > MAX_PATH_CHARS || input.chars().any(char::is_control) {
        return Err(HostReadError::InvalidPath);
    }
    let joined = if input == "~" || input.starts_with("~/") {
        let home = home.ok_or(HostReadError::Unavailable)?;
        home.join(input[1..].trim_start_matches('/'))
    } else if Path::new(input).is_absolute() {
        PathBuf::from(input)
    } else {
        let base = match cwd {
            Some(c) => {
                let c = Path::new(c);
                if !c.is_absolute() || c.to_str().is_some_and(|s| s.chars().any(char::is_control)) {
                    return Err(HostReadError::InvalidPath);
                }
                c.to_path_buf()
            }
            None => session_base.ok_or(HostReadError::InvalidPath)?.to_path_buf(),
        };
        base.join(input)
    };
    let mut out = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => out.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    return Err(HostReadError::InvalidPath);
                }
                out.pop();
            }
            Component::Normal(part) => out.push(part),
        }
    }
    if !out.is_absolute() {
        return Err(HostReadError::InvalidPath);
    }
    Ok(out)
}

/// Serves one chunk; call only on a blocking worker. Reads exactly the requested range.
pub fn serve_host_read(
    request: &m::FileReadRequest,
    home: Option<&Path>,
    session_base: Option<&Path>,
) -> Result<m::FileReadResult, HostReadError> {
    if request.length == 0 || request.length > m::FILE_READ_CHUNK_MAX_BYTES {
        return Err(HostReadError::InvalidRequest);
    }
    let path = resolve_host_path(&request.path, request.cwd.as_deref(), home, session_base)?;
    let resolved_path = path.to_str().ok_or(HostReadError::InvalidPath)?.to_string();
    // Refuse FIFOs, devices and directories before open(): opening a FIFO for read blocks
    // until a writer appears, which would pin this blocking worker forever.
    if !std::fs::metadata(&path).map_err(io_error)?.is_file() {
        return Err(HostReadError::NotRegularFile);
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        // Closes the stat/open race: a path swapped to a FIFO still opens without blocking,
        // and the fstat below refuses it. Regular files ignore O_NONBLOCK.
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let mut file = options.open(&path).map_err(io_error)?;
    let meta = file.metadata().map_err(io_error)?;
    if !meta.is_file() {
        return Err(HostReadError::NotRegularFile);
    }
    let total_bytes = meta.len();
    let version = content_version(&meta);
    if total_bytes > crate::ipc::remote_file::REMOTE_FILE_MAX_BYTES as u64 {
        return Err(HostReadError::TooLarge);
    }
    if request.offset > total_bytes {
        return Err(HostReadError::InvalidRequest);
    }
    file.seek(SeekFrom::Start(request.offset)).map_err(io_error)?;
    let want = (total_bytes - request.offset).min(u64::from(request.length));
    let mut data = Vec::with_capacity(want as usize);
    file.take(want).read_to_end(&mut data).map_err(io_error)?;
    let end = request.offset + data.len() as u64;
    Ok(m::FileReadResult {
        resolved_path,
        total_bytes,
        offset: request.offset,
        data: base64::engine::general_purpose::STANDARD.encode(&data),
        eof: end >= total_bytes,
        version,
    })
}

/// Stamp for one file's content identity: size, mtime nanos and (on unix) device/inode.
fn content_version(meta: &std::fs::Metadata) -> String {
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        format!("{}-{}-{}-{}", meta.len(), mtime, meta.dev(), meta.ino())
    }
    #[cfg(not(unix))]
    {
        format!("{}-{}", meta.len(), mtime)
    }
}

fn invalid_response(message: &str) -> IpcError {
    IpcError::new(IpcErrorCode::PairedHostInvalidResponse, message.to_string())
        .with_details(serde_json::json!({ "reason": "pairedFileReadInvalidResponse" }))
}

/// Keeps only a cwd the host can anchor on (absolute on either platform) and refuses a
/// relative token that has neither such a cwd nor a host session whose live cwd the host
/// can read: the host would otherwise answer with a misleading invalid-path refusal, and
/// the UI must be able to say that the terminal's directory is unknown.
pub(crate) fn admit_paired_cwd(
    path: &str,
    cwd: Option<String>,
    session_id: Option<&str>,
) -> Result<Option<String>, IpcError> {
    // The host may run another OS than this desktop, so absolute means absolute on either.
    let any_absolute = |p: &str| {
        let b = p.as_bytes();
        Path::new(p).is_absolute()
            || p.starts_with('/')
            || p.starts_with("\\\\")
            || (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && matches!(b[2], b'\\' | b'/'))
    };
    let cwd = cwd.filter(|c| any_absolute(c));
    if cwd.is_none() && session_id.is_none() && !is_home_or_absolute(path) && !any_absolute(path) {
        return Err(IpcError::new(
            IpcErrorCode::InvalidRequest,
            "The paired terminal's current directory is unknown, so a relative path cannot be resolved",
        )
        .with_details(serde_json::json!({ "reason": "remoteCwdUnknown" })));
    }
    Ok(cwd)
}

/// Pulls pages from `fetch(offset)` until eof and returns (resolved path, bytes). Every page
/// must continue at the requested offset with a stable path and size within `max_bytes`.
pub async fn read_all_chunks<F, Fut>(
    mut fetch: F,
    max_bytes: usize,
) -> Result<(String, Vec<u8>), IpcError>
where
    F: FnMut(u64) -> Fut,
    Fut: Future<Output = Result<m::FileReadResult, IpcError>>,
{
    let mut bytes = Vec::new();
    let mut identity: Option<(String, u64, String)> = None;
    loop {
        let offset = bytes.len() as u64;
        let page = fetch(offset).await?;
        if page.offset != offset {
            return Err(invalid_response("paired host answered a different offset"));
        }
        match &identity {
            None => {
                crate::ipc::remote_file::ensure_within_limit(page.total_bytes as usize, max_bytes)?;
                identity = Some((page.resolved_path.clone(), page.total_bytes, page.version.clone()));
            }
            Some((path, total, version))
                if *path != page.resolved_path
                    || *total != page.total_bytes
                    || *version != page.version =>
            {
                return Err(invalid_response("paired file changed while it was being read"));
            }
            Some(_) => {}
        }
        let chunk = base64::engine::general_purpose::STANDARD
            .decode(&page.data)
            .map_err(|_| invalid_response("paired host sent invalid base64"))?;
        if chunk.is_empty() && !page.eof {
            return Err(invalid_response("paired host made no progress"));
        }
        bytes.extend_from_slice(&chunk);
        let total = identity.as_ref().map_or(0, |(_, t, _)| *t);
        if bytes.len() as u64 > total {
            return Err(invalid_response("paired host sent more bytes than the file holds"));
        }
        if page.eof {
            if bytes.len() as u64 != total {
                return Err(invalid_response("paired file ended before its stated size"));
            }
            let (path, _, _) = identity.expect("identity set on first page");
            return Ok((path, bytes));
        }
    }
}

fn client_error(error: ClientError) -> IpcError {
    // A bodyless 403 never came from the host's file-read handler (its refusals carry a
    // typed envelope); it is a relay or host that does not admit this route yet.
    let bodyless_403 = error.code == "PERMISSION_DENIED"
        && error.machine_error.as_ref().is_some_and(|e| {
            e.request_id.is_empty()
                && e.details.get("httpStatus").and_then(|v| v.as_u64()) == Some(403)
        });
    if bodyless_403 {
        return IpcError::new(
            IpcErrorCode::Unsupported,
            "The paired host or its relay must be updated before it can serve terminal file links",
        )
        .with_details(serde_json::json!({ "reason": "pairedHostNeedsUpdate", "httpStatus": 403 }));
    }
    let code = IpcErrorCode::from_code_str(&error.code);
    let reason = match error.code.as_str() {
        "NOT_FOUND" => "remoteFileMissing",
        "PAYLOAD_TOO_LARGE" => "remoteFileTooLarge",
        "UNSUPPORTED_PATH" => "remoteFileNotRegular",
        "INVALID_PATH" => "invalidRemotePath",
        _ => "pairedFileReadFailed",
    };
    IpcError::new(code, format!("Paired host file read failed: {}", error.code))
        .with_details(serde_json::json!({ "reason": reason, "pairedCode": error.code }))
}

/// Refuses before paging when the running daemon predates the FileRead proxy operation.
async fn ensure_daemon_proxies_file_read(daemon: &DaemonClient) -> Result<(), IpcError> {
    let capabilities = daemon.paired_daemon_capabilities().await.map_err(|error| {
        IpcError::new(
            IpcErrorCode::from_code_str(&error.code),
            format!("Paired daemon capability query failed: {}", error.code),
        )
        .with_details(serde_json::json!({ "reason": "pairedFileReadFailed", "pairedCode": error.code }))
    })?;
    if capabilities.iter().any(|c| c == PAIRED_FILE_READ_PROXY_CAPABILITY) {
        return Ok(());
    }
    Err(IpcError::new(
        IpcErrorCode::Unsupported,
        "The background daemon must be restarted before it can fetch paired terminal file links",
    )
    .with_details(serde_json::json!({ "reason": "daemonNeedsRestart" })))
}


/// Fetches `path` from the paired host behind `workspace_id` into the remote-files cache.
/// `session_id` must already be the HOST-side session id. Returns (local, remote, length).
pub async fn fetch_paired_file(
    daemon: &Arc<DaemonClient>,
    data_dir: &Path,
    cache_root: &Path,
    workspace_id: &str,
    session_id: Option<String>,
    cwd: Option<String>,
    path: String,
) -> Result<(PathBuf, String, u64), IpcError> {
    let cwd = admit_paired_cwd(&path, cwd, session_id.as_deref())?;
    let host = crate::paired_host::upload::resolve_paired_host(daemon, data_dir, workspace_id).await?;
    ensure_daemon_proxies_file_read(daemon).await?;
    let caps = daemon
        .paired_host_operation(OperationRequest {
            host_id: host.host_id.clone(),
            generation: host.generation,
            operation: Operation::Capabilities,
        })
        .await
        .map_err(client_error)?;
    let supported = matches!(&caps.result, OperationResult::Capabilities(c)
        if c.capabilities.iter().any(|v| v == PAIRED_FILE_READ_CAPABILITY));
    if !supported {
        return Err(IpcError::new(
            IpcErrorCode::Unsupported,
            "The paired host must be updated before it can serve terminal file links",
        )
        .with_details(serde_json::json!({ "reason": "pairedHostNeedsUpdate" })));
    }
    let (remote_path, bytes) = read_all_chunks(
        |offset| {
            let request = m::FileReadRequest {
                request_id: uuid::Uuid::new_v4().to_string(),
                session_id: session_id.clone(),
                cwd: cwd.clone(),
                path: path.clone(),
                offset,
                length: m::FILE_READ_CHUNK_MAX_BYTES,
            };
            let operation = OperationRequest {
                host_id: host.host_id.clone(),
                generation: host.generation,
                operation: Operation::FileRead { request },
            };
            async move {
                match daemon.paired_host_operation(operation).await.map_err(client_error)?.result {
                    OperationResult::FileRead(page) => Ok(page),
                    _ => Err(invalid_response("paired host answered a different operation")),
                }
            }
        },
        crate::ipc::remote_file::REMOTE_FILE_MAX_BYTES,
    )
    .await?;
    let target = crate::ipc::remote_file::remote_cache_path(cache_root, workspace_id, &remote_path);
    let length = bytes.len() as u64;
    let local =
        run_blocking(move || crate::ipc::remote_file::write_cache_file(&target, &bytes)).await?;
    Ok((local, remote_path, length))
}



/// Maps the desktop's session id to the host-side session the read anchors on.
///
/// A relayed proxy id (`daemon-session:`) must resolve: dropping it would send a
/// session-less request and skip the host's check that the shell runs on that host.
/// Any other id is forwarded unchanged, and no id stays no id.
pub async fn resolve_host_session_id<F, Fut>(
    session_id: Option<String>,
    lookup: F,
) -> Result<Option<String>, IpcError>
where
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = Result<Option<String>, ClientError>>,
{
    match session_id {
        Some(id) if crate::terminal::paired_runtime::Runtime::owns(&id) => match lookup(id).await {
            Ok(Some(host_id)) => Ok(Some(host_id)),
            _ => Err(IpcError::new(
                IpcErrorCode::SessionNotFound,
                "The paired terminal session is no longer available",
            )
            .with_details(serde_json::json!({ "reason": "pairedSessionUnavailable" }))),
        },
        other => Ok(other),
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn relayed_session_ids_fail_closed_instead_of_dropping_the_session() {
        let ok = |host: Option<&'static str>| move |_id: String| async move { Ok(host.map(str::to_owned)) };
        assert_eq!(
            resolve_host_session_id(Some("daemon-session:abc".into()), ok(Some("host-s1"))).await.unwrap(),
            Some("host-s1".to_string())
        );
        // Lookup found nothing or failed: refuse, never forward None.
        assert!(resolve_host_session_id(Some("daemon-session:abc".into()), ok(None)).await.is_err());
        let failed = |_id: String| async move {
            Err::<Option<String>, _>(ClientError::local("HOST_UNAVAILABLE"))
        };
        assert!(resolve_host_session_id(Some("daemon-session:abc".into()), failed).await.is_err());
        // Non-relayed ids and no id pass through unchanged.
        assert_eq!(
            resolve_host_session_id(Some("local-1".into()), ok(None)).await.unwrap(),
            Some("local-1".to_string())
        );
        assert_eq!(resolve_host_session_id(None, ok(None)).await.unwrap(), None);
    }

    fn bodyless(status: u16) -> ClientError {
        let mut details = serde_json::Map::new();
        details.insert("status".into(), serde_json::json!(status));
        details.insert("httpStatus".into(), serde_json::json!(status));
        details.insert("contentTypeClass".into(), serde_json::json!("none"));
        ClientError {
            code: "PERMISSION_DENIED".into(),
            machine_error: Some(Box::new(m::MachineError {
                code: "PERMISSION_DENIED".into(),
                message: "denied".into(),
                retryable: false,
                request_id: String::new(),
                details,
            })),
            request_id: None,
            ambiguous: false,
        }
    }

    #[test]
    fn bodyless_403_maps_to_needs_update_not_permission_denied() {
        let error = client_error(bodyless(403));
        assert_eq!(error.code, IpcErrorCode::Unsupported);
        assert_eq!(error.details.as_ref().unwrap()["reason"], "pairedHostNeedsUpdate");
        // A typed host refusal (the file itself is unreadable) stays a permission error.
        let mut typed = bodyless(403);
        typed.machine_error.as_mut().unwrap().details.clear();
        typed.machine_error.as_mut().unwrap().request_id = "3941b9de-b16d-4d9a-ae0a-118f90fd91f4".into();
        assert_eq!(client_error(typed).code, IpcErrorCode::PermissionDenied);
    }

    #[cfg(unix)]
    async fn fake_daemon(capabilities: Vec<&'static str>) -> (tempfile::TempDir, DaemonClient, tokio::task::JoinHandle<()>) {
        use crate::daemon::protocol::{DaemonRequest, DAEMON_PROTOCOL_VERSION};
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        let root = tempfile::tempdir().unwrap();
        let socket = root.path().join("d.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        let task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let caps = capabilities.clone();
                tokio::spawn(async move {
                    let (reader, mut writer) = stream.into_split();
                    let mut lines = BufReader::new(reader).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        let reply = match serde_json::from_str::<DaemonRequest>(&line) {
                            Ok(DaemonRequest::Handshake { .. }) => serde_json::json!({
                                "type": "handshakeOk", "version": DAEMON_PROTOCOL_VERSION,
                                "pid": 1, "epoch": 1
                            }),
                            Ok(DaemonRequest::GetCapabilities) => serde_json::json!({
                                "type": "capabilitiesOk", "capabilities": caps
                            }),
                            _ => serde_json::json!({"type": "error", "message": "unknown request"}),
                        };
                        let mut bytes = serde_json::to_vec(&reply).unwrap();
                        bytes.push(b'\n');
                        if writer.write_all(&bytes).await.is_err() {
                            return;
                        }
                    }
                });
            }
        });
        let client = DaemonClient::new_with_socket(socket);
        (root, client, task)
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn old_daemon_without_file_read_proxy_needs_restart() {
        let (_root, client, task) = fake_daemon(vec!["pairedHostInventoryV1"]).await;
        let error = ensure_daemon_proxies_file_read(&client).await.unwrap_err();
        assert_eq!(error.code, IpcErrorCode::Unsupported);
        assert_eq!(error.details.as_ref().unwrap()["reason"], "daemonNeedsRestart");
        task.abort();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn daemon_advertising_file_read_proxy_is_admitted() {
        let (_root, client, task) =
            fake_daemon(vec!["pairedHostInventoryV1", PAIRED_FILE_READ_PROXY_CAPABILITY]).await;
        ensure_daemon_proxies_file_read(&client).await.expect("capable daemon admitted");
        task.abort();
    }

    #[test]
    fn daemon_capability_constant_is_stable() {
        assert_eq!(PAIRED_FILE_READ_PROXY_CAPABILITY, "pairedFileReadProxyV1");
    }

    fn req(path: &str, cwd: Option<&str>, offset: u64, length: u32) -> m::FileReadRequest {
        m::FileReadRequest {
            request_id: "3941b9de-b16d-4d9a-ae0a-118f90fd91f4".into(),
            session_id: None,
            cwd: cwd.map(str::to_owned),
            path: path.into(),
            offset,
            length,
        }
    }

    #[test]
    fn paired_relative_token_without_cwd_or_session_is_refused_as_cwd_unknown() {
        for cwd in [None, Some("relative/dir".to_string())] {
            let err = admit_paired_cwd("src/main.rs", cwd.clone(), None).unwrap_err();
            assert_eq!(err.code, IpcErrorCode::InvalidRequest, "{cwd:?}");
            assert_eq!(err.details.as_ref().unwrap()["reason"], "remoteCwdUnknown", "{cwd:?}");
        }
        // A host session lets the host anchor on its live cwd or worktree.
        assert_eq!(admit_paired_cwd("src/main.rs", None, Some("host-s1")).unwrap(), None);
        // An invalid cwd is dropped, never forwarded.
        assert_eq!(admit_paired_cwd("src/main.rs", Some("rel".into()), Some("host-s1")).unwrap(), None);
        assert_eq!(
            admit_paired_cwd("src/main.rs", Some("/repo".into()), None).unwrap(),
            Some("/repo".to_string())
        );
        for path in ["/etc/hosts", "~", "~/a.png"] {
            assert_eq!(admit_paired_cwd(path, None, None).unwrap(), None, "{path}");
        }
        // A Windows host's absolute paths need no cwd either, whatever the desktop OS.
        for path in [r"C:\logs\run.txt", r"\\srv\share\a.txt"] {
            assert_eq!(admit_paired_cwd(path, None, None).unwrap(), None, "{path}");
        }
        assert_eq!(
            admit_paired_cwd("a.txt", Some(r"C:\repo".into()), None).unwrap(),
            Some(r"C:\repo".to_string())
        );
    }

    #[test]
    fn resolve_host_path_expands_home() {
        let home = Path::new("/home/u");
        assert_eq!(resolve_host_path("~", None, Some(home), None).unwrap(), PathBuf::from("/home/u"));
        assert_eq!(
            resolve_host_path("~/shots/a.png", None, Some(home), None).unwrap(),
            PathBuf::from("/home/u/shots/a.png")
        );
        assert_eq!(
            resolve_host_path("~/x", None, None, None).unwrap_err(),
            HostReadError::Unavailable
        );
    }

    #[test]
    fn resolve_host_path_joins_relative_onto_cwd_then_session() {
        assert_eq!(
            resolve_host_path("shot.png", Some("/repo"), None, Some(Path::new("/other"))).unwrap(),
            PathBuf::from("/repo/shot.png")
        );
        assert_eq!(
            resolve_host_path("shot.png", None, None, Some(Path::new("/wt"))).unwrap(),
            PathBuf::from("/wt/shot.png")
        );
        assert_eq!(resolve_host_path("shot.png", None, None, None).unwrap_err(), HostReadError::InvalidPath);
        assert_eq!(
            resolve_host_path("shot.png", Some("rel"), None, None).unwrap_err(),
            HostReadError::InvalidPath
        );
    }

    #[test]
    fn resolve_host_path_keeps_absolute_and_normalizes_dot_dot() {
        assert_eq!(resolve_host_path("/etc/hosts", Some("/repo"), None, None).unwrap(), PathBuf::from("/etc/hosts"));
        assert_eq!(
            resolve_host_path("src/../docs/./a.md", Some("/repo"), None, None).unwrap(),
            PathBuf::from("/repo/docs/a.md")
        );
        assert_eq!(resolve_host_path("/../etc", None, None, None).unwrap_err(), HostReadError::InvalidPath);
        assert_eq!(resolve_host_path("a\nb", Some("/r"), None, None).unwrap_err(), HostReadError::InvalidPath);
    }

    #[test]
    fn serve_host_read_ranges_and_refusals() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_str().unwrap();
        std::fs::write(dir.path().join("f.bin"), b"0123456789").unwrap();
        std::fs::create_dir(dir.path().join("d")).unwrap();

        let first = serve_host_read(&req("f.bin", Some(root), 0, 4), None, None).unwrap();
        assert_eq!((first.total_bytes, first.offset, first.eof), (10, 0, false));
        assert_eq!(base64::engine::general_purpose::STANDARD.decode(first.data).unwrap(), b"0123");
        let last = serve_host_read(&req("f.bin", Some(root), 8, 4), None, None).unwrap();
        assert!(last.eof);
        assert_eq!(base64::engine::general_purpose::STANDARD.decode(last.data).unwrap(), b"89");

        assert_eq!(serve_host_read(&req("nope", Some(root), 0, 4), None, None).unwrap_err(), HostReadError::NotFound);
        assert_eq!(serve_host_read(&req("d", Some(root), 0, 4), None, None).unwrap_err(), HostReadError::NotRegularFile);
        assert_eq!(serve_host_read(&req("f.bin", Some(root), 11, 4), None, None).unwrap_err(), HostReadError::InvalidRequest);
        assert_eq!(
            serve_host_read(&req("f.bin", Some(root), 0, m::FILE_READ_CHUNK_MAX_BYTES + 1), None, None).unwrap_err(),
            HostReadError::InvalidRequest
        );

        let big = std::fs::File::create(dir.path().join("big")).unwrap();
        big.set_len(crate::ipc::remote_file::REMOTE_FILE_MAX_BYTES as u64 + 1).unwrap();
        let err = serve_host_read(&req("big", Some(root), 0, 4), None, None).unwrap_err();
        assert_eq!((err, err.code(), err.status()), (HostReadError::TooLarge, "PAYLOAD_TOO_LARGE", 413));
    }

    #[cfg(unix)]
    #[test]
    fn serve_host_read_refuses_fifo_without_blocking() {
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("pipe");
        let c_path = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0, "mkfifo failed");
        let root = dir.path().to_str().unwrap().to_string();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(serve_host_read(&req("pipe", Some(&root), 0, 4), None, None));
        });
        let outcome = rx.recv_timeout(std::time::Duration::from_secs(5));
        if outcome.is_err() {
            // Release a reader stuck in open() so the leaked thread can finish.
            use std::os::unix::fs::OpenOptionsExt;
            let _ = std::fs::OpenOptions::new()
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&fifo);
        }
        let result = outcome.expect("serve_host_read blocked on a FIFO instead of refusing it");
        assert_eq!(result.unwrap_err(), HostReadError::NotRegularFile);
    }

    #[test]
    fn serve_host_read_version_tracks_content_identity() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_str().unwrap().to_string();
        let file = dir.path().join("v.txt");
        std::fs::write(&file, b"aaaa").unwrap();
        let first = serve_host_read(&req("v.txt", Some(&root), 0, 4), None, None).unwrap();
        assert!(!first.version.is_empty(), "host must stamp a version");
        let again = serve_host_read(&req("v.txt", Some(&root), 0, 4), None, None).unwrap();
        assert_eq!(first.version, again.version, "unchanged file keeps its version");
        // Same-size atomic replacement: a new inode and mtime must change the stamp.
        let staged = dir.path().join("v.tmp");
        std::fs::write(&staged, b"bbbb").unwrap();
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        std::fs::File::options().write(true).open(&staged).unwrap().set_modified(later).unwrap();
        std::fs::rename(&staged, &file).unwrap();
        let after = serve_host_read(&req("v.txt", Some(&root), 0, 4), None, None).unwrap();
        assert_eq!(after.total_bytes, first.total_bytes);
        assert_ne!(after.version, first.version, "same-size rewrite must change the version");
    }

    fn page(path: &str, total: u64, offset: u64, data: &[u8], eof: bool) -> m::FileReadResult {
        m::FileReadResult {
            resolved_path: path.into(),
            total_bytes: total,
            offset,
            data: base64::engine::general_purpose::STANDARD.encode(data),
            eof,
            version: "v1".into(),
        }
    }

    async fn assemble(file: &'static [u8], chunk: usize, max: usize) -> Result<(String, Vec<u8>), IpcError> {
        read_all_chunks(
            |offset| async move {
                let start = offset as usize;
                let end = (start + chunk).min(file.len());
                Ok(page("/h/f", file.len() as u64, offset, &file[start..end], end == file.len()))
            },
            max,
        )
        .await
    }

    #[tokio::test]
    async fn read_all_chunks_assembles_pages_in_order() {
        let (path, bytes) = assemble(b"abcdefghij", 3, 100).await.unwrap();
        assert_eq!((path.as_str(), bytes.as_slice()), ("/h/f", b"abcdefghij".as_slice()));
        let (_, empty) = assemble(b"", 3, 100).await.unwrap();
        assert!(empty.is_empty());
    }

    #[tokio::test]
    async fn read_all_chunks_refuses_oversize_short_and_shifting_pages() {
        let err = assemble(b"abcdefghij", 3, 5).await.unwrap_err();
        assert_eq!(err.code, IpcErrorCode::PayloadTooLarge);

        let short = read_all_chunks(|o| async move { Ok(page("/h/f", 10, o, b"abc", true)) }, 100).await;
        assert_eq!(short.unwrap_err().code, IpcErrorCode::PairedHostInvalidResponse);

        let shifted = read_all_chunks(
            |o| async move {
                let total = if o == 0 { 6 } else { 7 };
                Ok(page("/h/f", total, o, b"abc", false))
            },
            100,
        )
        .await;
        assert_eq!(shifted.unwrap_err().code, IpcErrorCode::PairedHostInvalidResponse);

        let wrong_offset = read_all_chunks(|_| async move { Ok(page("/h/f", 6, 3, b"abc", false)) }, 100).await;
        assert_eq!(wrong_offset.unwrap_err().code, IpcErrorCode::PairedHostInvalidResponse);
    }

    #[tokio::test]
    async fn read_all_chunks_refuses_same_size_rewrite_between_pages() {
        let rewritten = read_all_chunks(
            |o| async move {
                let mut p = page("/h/f", 6, o, b"abc", o != 0);
                if o != 0 {
                    p.version = "v2".into();
                }
                Ok(p)
            },
            100,
        )
        .await;
        let err = rewritten.unwrap_err();
        assert_eq!(err.code, IpcErrorCode::PairedHostInvalidResponse);
        assert!(err.message.contains("changed while it was being read"), "{}", err.message);
    }
}

