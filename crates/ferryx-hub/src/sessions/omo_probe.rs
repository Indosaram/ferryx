//! omo/mhc session snapshots, read the way omo-gateway's `sessions overview` read them.
//!
//! Every omo or mhc process serves a JSON-lines RPC socket under `~/.omo/agent/rpc`:
//! - `tui/*.sock`: one interactive session each. A client first writes the bytes of
//!   `<socket>.secret`, then requests; `get_state` needs no session id.
//! - `rpc.sock` and `p-*.sock`: RPC hosts (the omo daemon, mhc host supervisors) that serve
//!   several sessions. No secret; `list_sessions` names them and every request carries a
//!   `sessionId`.
//!
//! Each session is read with `get_state` and `get_messages`; an `ask_user_question` tool call
//! without an answer is a pending question. On this machine the sockets are read in-process.
//! A remote host (`OMON_SESSION_HOSTS`, else `~/.omon/session-hosts.json`) is read over
//! `ssh <host> python3 -` with [`REMOTE_PROBE`] on stdin, which reads the same sockets the
//! same way there, as omo-gateway did.
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::watch::compact;

/// The remote half: run on a host by `python3 - snapshot <text_limit>`.
pub const REMOTE_PROBE: &str = include_str!("omo_probe.py");
/// Characters kept of the last user and assistant text.
pub const TEXT_LIMIT: usize = 1000;
const REMOTE_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestionItem {
    pub id: String,
    pub header: Option<String>,
    pub question: Option<String>,
    pub options: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingQuestion {
    pub request_id: String,
    pub questions: Vec<QuestionItem>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LiveSession {
    pub session_id: String,
    pub routing_id: String,
    pub endpoint_kind: String,
    pub name: Option<String>,
    pub cwd: Option<String>,
    pub streaming: bool,
    pub compacting: bool,
    pub blocking_question: bool,
    pub pending_messages: u64,
    pub message_count: u64,
    pub last_user: String,
    pub last_assistant: String,
    pub questions: Vec<PendingQuestion>,
    pub messages_error: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotError {
    pub endpoint: String,
    pub error: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineSnapshot {
    pub sessions: Vec<LiveSession>,
    pub errors: Vec<SnapshotError>,
}

/// `~/.omo/agent/rpc`.
pub fn default_rpc_dir() -> Option<PathBuf> {
    crate::hub_env::var_os("HOME").map(|home| PathBuf::from(home).join(".omo/agent/rpc"))
}

/// Host names reach `ssh` as an argument; reject empty, spaced, or option-like names.
pub fn valid_host(host: &str) -> bool {
    !host.is_empty() && !host.starts_with('-') && !host.chars().any(char::is_whitespace)
}

/// `OMON_SESSION_HOSTS` (comma separated) when set, else the seeded host file. `local` and
/// invalid names are dropped.
pub fn parse_hosts(configured: Option<&str>, seeded: Option<&str>) -> Vec<String> {
    let names: Vec<String> = match configured.filter(|value| !value.trim().is_empty()) {
        Some(value) => value.split(',').map(|host| host.trim().to_owned()).collect(),
        None => seeded
            .and_then(|text| serde_json::from_str::<Vec<String>>(text).ok())
            .unwrap_or_default(),
    };
    let mut hosts: Vec<String> = Vec::new();
    for host in names {
        if host == "local" || hosts.contains(&host) {
            continue;
        }
        if valid_host(&host) {
            hosts.push(host);
        } else {
            tracing::warn!(host = %compact(&host, 60), "omo session host is not a valid ssh host; skipped");
        }
    }
    hosts
}

pub async fn configured_hosts() -> Vec<String> {
    let configured = crate::hub_env::var("OMON_SESSION_HOSTS").ok();
    let seeded = match configured.as_deref().filter(|value| !value.trim().is_empty()) {
        Some(_) => None,
        None => match crate::hub_env::var_os("HOME") {
            Some(home) => {
                tokio::fs::read_to_string(PathBuf::from(home).join(".omon/session-hosts.json"))
                    .await
                    .ok()
            }
            None => None,
        },
    };
    parse_hosts(configured.as_deref(), seeded.as_deref())
}

/// `local` from the sockets in `rpc_dir`, then every host over ssh, read concurrently.
pub async fn snapshot_all(
    rpc_dir: Option<&Path>,
    hosts: &[String],
) -> Vec<(String, Result<MachineSnapshot, String>)> {
    let local = async {
        match rpc_dir {
            Some(dir) => Ok(snapshot_local(dir, TEXT_LIMIT).await),
            None => Err("HOME is not set".to_owned()),
        }
    };
    let remote = async {
        let mut set = tokio::task::JoinSet::new();
        for (index, host) in hosts.iter().enumerate() {
            let host = host.clone();
            set.spawn(async move { (index, snapshot_remote(&host, TEXT_LIMIT).await) });
        }
        let mut results: Vec<Option<Result<MachineSnapshot, String>>> = vec![None; hosts.len()];
        while let Some(joined) = set.join_next().await {
            if let Ok((index, result)) = joined {
                results[index] = Some(result);
            }
        }
        results
    };
    let (local, remote) = tokio::join!(local, remote);
    let mut snapshots = vec![("local".to_owned(), local)];
    for (host, result) in hosts.iter().zip(remote) {
        snapshots.push((
            host.clone(),
            result.unwrap_or_else(|| Err("probe task failed".to_owned())),
        ));
    }
    snapshots
}

/// The argv that runs [`REMOTE_PROBE`] on `host`; the script itself goes to stdin.
pub fn remote_argv(host: &str, text_limit: usize) -> Vec<String> {
    [
        "-o",
        "BatchMode=yes",
        "-o",
        "ConnectTimeout=5",
        "--",
        host,
        "python3",
        "-",
        "snapshot",
    ]
    .iter()
    .map(|word| (*word).to_owned())
    .chain([text_limit.to_string()])
    .collect()
}

pub async fn snapshot_remote(host: &str, text_limit: usize) -> Result<MachineSnapshot, String> {
    use tokio::io::AsyncWriteExt;
    if !valid_host(host) {
        return Err(format!("invalid host name {host:?}"));
    }
    let mut command = crate::child_env::command("ssh");
    command
        .args(remote_argv(host, text_limit))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let run = async {
        let mut child = command.spawn().map_err(|error| format!("ssh: {error}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            // A host that closes stdin early still answers with its exit status below.
            let _ = stdin.write_all(REMOTE_PROBE.as_bytes()).await;
        }
        child
            .wait_with_output()
            .await
            .map_err(|error| format!("ssh: {error}"))
    };
    let output = tokio::time::timeout(REMOTE_TIMEOUT, run)
        .await
        .map_err(|_| "probe timed out".to_owned())??;
    parse_remote(output.status.code(), &output.stdout, &output.stderr)
}

pub fn parse_remote(
    code: Option<i32>,
    stdout: &[u8],
    stderr: &[u8],
) -> Result<MachineSnapshot, String> {
    if code == Some(0) {
        return serde_json::from_slice(stdout)
            .map_err(|error| format!("invalid probe output: {error}"));
    }
    let detail = if stderr.iter().any(|byte| !byte.is_ascii_whitespace()) {
        stderr
    } else {
        stdout
    };
    Err(compact(
        &format!(
            "exit {}: {}",
            code.map_or_else(|| "signal".to_owned(), |code| code.to_string()),
            String::from_utf8_lossy(detail).trim()
        ),
        300,
    ))
}

fn text_of(message: &Value) -> String {
    match message.get("content") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
            .map(|block| block.get("text").and_then(Value::as_str).unwrap_or_default())
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    }
}

/// `ask_user_question` calls with neither a settled tool result nor an
/// `[Answer to question <id>]` user message.
pub fn pending_questions(messages: &[Value]) -> Vec<PendingQuestion> {
    let mut asked: Vec<(String, Value)> = Vec::new();
    let mut answered: Vec<String> = Vec::new();
    for message in messages {
        match message.get("role").and_then(Value::as_str) {
            Some("assistant") => {
                for block in message
                    .get("content")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let id = block.get("id").and_then(Value::as_str);
                    if block.get("type").and_then(Value::as_str) == Some("toolCall")
                        && block.get("name").and_then(Value::as_str) == Some("ask_user_question")
                    {
                        if let Some(id) = id {
                            let arguments = block.get("arguments").cloned().unwrap_or(Value::Null);
                            match asked.iter_mut().find(|(seen, _)| seen == id) {
                                Some(entry) => entry.1 = arguments,
                                None => asked.push((id.to_owned(), arguments)),
                            }
                        }
                    }
                }
            }
            Some("toolResult")
                if message.get("toolName").and_then(Value::as_str)
                    == Some("ask_user_question") =>
            {
                let status = message
                    .get("details")
                    .and_then(|details| details.get("status"))
                    .and_then(Value::as_str);
                if status != Some("pending") {
                    if let Some(id) = message.get("toolCallId").and_then(Value::as_str) {
                        answered.push(id.to_owned());
                    }
                }
            }
            Some("user") => {
                let text = text_of(message);
                if let Some(rest) = text.strip_prefix("[Answer to question ") {
                    answered.push(rest.split(']').next().unwrap_or_default().to_owned());
                }
            }
            _ => {}
        }
    }
    asked
        .into_iter()
        .filter(|(id, _)| !answered.contains(id))
        .map(|(request_id, arguments)| PendingQuestion {
            request_id,
            questions: arguments
                .get("questions")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
                .filter(|(_, question)| question.is_object())
                .map(|(index, question)| QuestionItem {
                    id: question
                        .get("id")
                        .and_then(Value::as_str)
                        .filter(|id| !id.is_empty())
                        .map_or_else(|| format!("q{}", index + 1), str::to_owned),
                    header: question
                        .get("header")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    question: question
                        .get("question")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    options: question
                        .get("options")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(|option| option.get("label").and_then(Value::as_str))
                        .map(str::to_owned)
                        .collect(),
                })
                .collect(),
        })
        .collect()
}

/// The named pipe mhc serves a logical socket path on under Windows: a sha256 of the normalized,
/// lowercased path followed by the socket secret (mhc `maho-rpc/src/socket_transport.rs`
/// `resolve_socket_transport_address`; the listener is `host_lifecycle.rs` `win32_pipe`).
#[cfg(any(windows, test))]
pub fn pipe_address(socket_path: &str, secret: &[u8]) -> Option<String> {
    use sha2::{Digest, Sha256};
    let bytes = socket_path.as_bytes();
    let drive = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/');
    let unc = socket_path.starts_with("\\\\") && !socket_path.starts_with("\\\\/");
    if !drive && !unc {
        return None;
    }
    let path = socket_path.replace('/', "\\");
    let (prefix, rest) = if drive {
        (path[..2].to_owned(), path[2..].to_owned())
    } else {
        let mut parts = path.trim_start_matches('\\').splitn(3, '\\');
        let server = parts.next().unwrap_or_default();
        let share = parts.next().unwrap_or_default();
        (
            format!("\\\\{server}\\{share}"),
            format!("\\{}", parts.next().unwrap_or_default()),
        )
    };
    let mut segments: Vec<&str> = Vec::new();
    for segment in rest.split('\\') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            other => segments.push(other),
        }
    }
    let joined = segments.join("\\");
    let normalized = if joined.is_empty() {
        format!("{prefix}\\")
    } else {
        format!("{prefix}\\{joined}")
    };
    let mut identity = normalized.to_lowercase().into_bytes();
    identity.extend_from_slice(secret);
    let digest: String = Sha256::digest(&identity)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Some(format!("\\\\.\\pipe\\senpi-rpc-{}", &digest[..32]))
}

#[cfg(not(any(unix, windows)))]
pub async fn snapshot_local(_rpc_dir: &Path, _text_limit: usize) -> MachineSnapshot {
    // omo and mhc serve RPC over unix sockets or Windows named pipes; neither exists here.
    MachineSnapshot::default()
}

#[cfg(any(unix, windows))]
pub use local::snapshot_local;

/// On unix every endpoint is a socket file. On Windows mhc serves a named pipe whose name is
/// derived from the logical `.sock` path and its secret, and no socket file exists: endpoints
/// are found by their `<logical path>.secret` files, and every connection starts with the
/// secret (mhc `multi_session_host.rs` requires it on Windows even for RPC hosts).
#[cfg(any(unix, windows))]
mod local {
    use super::*;
    use serde_json::json;
    use std::sync::Arc;
    use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt};

    #[cfg(unix)]
    const ENDPOINT_SUFFIX: &str = ".sock";
    #[cfg(windows)]
    const ENDPOINT_SUFFIX: &str = ".sock.secret";

    const STATE_TIMEOUT: Duration = Duration::from_secs(4);
    const MESSAGES_TIMEOUT: Duration = Duration::from_secs(8);
    const PARALLEL_READS: usize = 16;

    pub(super) fn tail(text: &str, limit: usize) -> String {
        let count = text.chars().count();
        text.chars().skip(count.saturating_sub(limit)).collect()
    }

    fn last_text(messages: &[Value], role: &str) -> String {
        messages
            .iter()
            .rev()
            .filter(|message| message.get("role").and_then(Value::as_str) == Some(role))
            .map(text_of)
            .find(|text| !text.trim().is_empty())
            .unwrap_or_default()
    }

    /// The durable session id an RPC host's session file carries: `<timestamp>_<id>.jsonl`.
    pub(super) fn durable_id(session_file: &str) -> Option<String> {
        let name = Path::new(session_file).file_name()?.to_str()?;
        let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
        stem.split_once('_').map(|(_, id)| id.to_owned())
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Kind {
        Tui,
        RpcHost,
    }

    /// A socket nobody serves (a stale file, a missing secret) is not an error: the session
    /// is gone.
    enum RpcError {
        Dead,
        Failed(String),
    }

    fn io_error(error: std::io::Error) -> RpcError {
        match error.kind() {
            std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound => RpcError::Dead,
            _ => RpcError::Failed(error.to_string()),
        }
    }

    pub(super) fn secret_path(socket: &Path) -> PathBuf {
        let mut path = socket.as_os_str().to_owned();
        path.push(".secret");
        PathBuf::from(path)
    }

    async fn rpc(
        socket: &Path,
        request: Value,
        timeout: Duration,
        secret: bool,
    ) -> Result<Value, RpcError> {
        let id = format!("ferryx-{}", uuid::Uuid::new_v4());
        let kind = request_type(&request);
        let call = async {
            let stream = connect(socket, secret).await?;
            exchange(stream, request, &id).await
        };
        tokio::time::timeout(timeout, call)
            .await
            .map_err(|_| RpcError::Failed(format!("{kind} timed out")))?
    }

    #[cfg(unix)]
    async fn connect(socket: &Path, secret: bool) -> Result<tokio::net::UnixStream, RpcError> {
        let mut stream = tokio::net::UnixStream::connect(socket)
            .await
            .map_err(io_error)?;
        if secret {
            let bytes = tokio::fs::read(secret_path(socket))
                .await
                .map_err(io_error)?;
            stream.write_all(&bytes).await.map_err(io_error)?;
        }
        Ok(stream)
    }

    #[cfg(windows)]
    async fn connect(
        socket: &Path,
        _secret: bool,
    ) -> Result<tokio::net::windows::named_pipe::NamedPipeClient, RpcError> {
        /// `ERROR_PIPE_BUSY`: every instance is momentarily in use; wait and retry.
        const ERROR_PIPE_BUSY: i32 = 231;
        let bytes = tokio::fs::read(secret_path(socket))
            .await
            .map_err(io_error)?;
        let logical = socket.to_string_lossy();
        let address = pipe_address(&logical, &bytes)
            .ok_or_else(|| RpcError::Failed(format!("not a Windows socket path: {logical}")))?;
        let mut stream = loop {
            match tokio::net::windows::named_pipe::ClientOptions::new().open(&address) {
                Ok(stream) => break stream,
                Err(error) if error.raw_os_error() == Some(ERROR_PIPE_BUSY) => {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                Err(error) => return Err(io_error(error)),
            }
        };
        stream.write_all(&bytes).await.map_err(io_error)?;
        Ok(stream)
    }

    async fn exchange(
        mut stream: impl AsyncRead + AsyncWrite + Unpin,
        mut request: Value,
        id: &str,
    ) -> Result<Value, RpcError> {
        request["id"] = Value::String(id.to_owned());
        let mut line = request.to_string();
        line.push('\n');
        stream.write_all(line.as_bytes()).await.map_err(io_error)?;
        let mut reader = tokio::io::BufReader::new(stream);
        loop {
            line.clear();
            if reader.read_line(&mut line).await.map_err(io_error)? == 0 {
                return Err(RpcError::Failed(
                    "connection closed before the response".into(),
                ));
            }
            if line.trim().is_empty() {
                continue;
            }
            let message: Value = serde_json::from_str(&line)
                .map_err(|error| RpcError::Failed(format!("invalid response: {error}")))?;
            if message.get("id").and_then(Value::as_str) == Some(id) {
                return Ok(message);
            }
        }
    }

    fn request_type(request: &Value) -> String {
        request
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("request")
            .to_owned()
    }

    fn data_of(response: Value) -> Result<Value, RpcError> {
        if response.get("success") == Some(&Value::Bool(false)) {
            let error = response.get("error").map_or_else(
                || "refused".to_owned(),
                |error| error.as_str().map_or_else(|| error.to_string(), str::to_owned),
            );
            return Err(RpcError::Failed(error));
        }
        Ok(response.get("data").cloned().unwrap_or(Value::Null))
    }

    /// Logical socket paths under `dir` whose names start with `prefix`.
    async fn sorted_sockets(dir: &Path, prefix: &str) -> Vec<PathBuf> {
        let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
            return Vec::new();
        };
        let mut sockets = Vec::new();
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if !name.starts_with(prefix) {
                continue;
            }
            if let Some(stem) = name.strip_suffix(ENDPOINT_SUFFIX) {
                sockets.push(dir.join(format!("{stem}.sock")));
            }
        }
        sockets.sort();
        sockets
    }

    /// Session ids an RPC host lists. A host that does not answer contributes none.
    async fn listed_ids(host: &Path) -> Vec<String> {
        let listed = match rpc(host, json!({"type": "list_sessions"}), STATE_TIMEOUT, false)
            .await
            .and_then(data_of)
        {
            Ok(listed) => listed,
            Err(RpcError::Dead) => return Vec::new(),
            Err(RpcError::Failed(error)) => {
                tracing::debug!(socket = %host.display(), %error, "omo rpc host did not list sessions");
                return Vec::new();
            }
        };
        listed
            .get("sessions")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|session| session.get("sessionId").and_then(Value::as_str))
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
            .collect()
    }

    async fn endpoints(rpc_dir: &Path) -> Vec<(PathBuf, Kind, Option<String>)> {
        let mut found: Vec<(PathBuf, Kind, Option<String>)> =
            sorted_sockets(&rpc_dir.join("tui"), "")
                .await
                .into_iter()
                .map(|socket| (socket, Kind::Tui, None))
                .collect();
        let mut hosts = vec![rpc_dir.join("rpc.sock")];
        hosts.extend(sorted_sockets(rpc_dir, "p-").await);
        let permits = Arc::new(tokio::sync::Semaphore::new(PARALLEL_READS));
        let mut set = tokio::task::JoinSet::new();
        for (index, host) in hosts.into_iter().enumerate() {
            let permits = Arc::clone(&permits);
            set.spawn(async move {
                let _permit = permits.acquire_owned().await;
                let ids = listed_ids(&host).await;
                (index, host, ids)
            });
        }
        let mut listed = Vec::new();
        while let Some(joined) = set.join_next().await {
            if let Ok(entry) = joined {
                listed.push(entry);
            }
        }
        listed.sort_by_key(|(index, _, _)| *index);
        for (_, host, ids) in listed {
            found.extend(ids.into_iter().map(|id| (host.clone(), Kind::RpcHost, Some(id))));
        }
        found
    }

    async fn updated_at(session_file: Option<&str>) -> Option<String> {
        let modified = tokio::fs::metadata(session_file?).await.ok()?.modified().ok()?;
        Some(
            chrono::DateTime::<chrono::Utc>::from(modified)
                .format("%Y-%m-%dT%H:%M:%SZ")
                .to_string(),
        )
    }

    async fn session_view(
        socket: &Path,
        kind: Kind,
        routing: Option<&str>,
        text_limit: usize,
    ) -> Result<LiveSession, RpcError> {
        let secret = kind == Kind::Tui;
        let mut request = json!({"type": "get_state"});
        if let Some(routing) = routing {
            request["sessionId"] = Value::String(routing.to_owned());
        }
        let state = data_of(rpc(socket, request, STATE_TIMEOUT, secret).await?)?;
        let str_of = |key: &str| state.get(key).and_then(Value::as_str).map(str::to_owned);
        let flag = |key: &str| state.get(key).and_then(Value::as_bool).unwrap_or(false);
        let session_id = str_of("sessionId")
            .filter(|id| !id.is_empty())
            .or_else(|| routing.map(str::to_owned))
            .unwrap_or_default();
        let address = routing.map_or_else(|| session_id.clone(), str::to_owned);
        let (messages, messages_error) = match rpc(
            socket,
            json!({"type": "get_messages", "sessionId": address}),
            MESSAGES_TIMEOUT,
            secret,
        )
        .await
        .and_then(data_of)
        {
            Ok(data) => (
                data.get("messages")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default(),
                None,
            ),
            Err(RpcError::Dead) => return Err(RpcError::Dead),
            Err(RpcError::Failed(error)) => (Vec::new(), Some(compact(&error, 120))),
        };
        let session_file = str_of("sessionFile");
        let durable = match kind {
            Kind::RpcHost => session_file
                .as_deref()
                .and_then(durable_id)
                .unwrap_or_else(|| session_id.clone()),
            Kind::Tui => session_id.clone(),
        };
        let count = state
            .get("messageCount")
            .and_then(Value::as_u64)
            .filter(|count| *count > 0)
            .unwrap_or(messages.len() as u64);
        Ok(LiveSession {
            session_id: durable,
            routing_id: address,
            endpoint_kind: match kind {
                Kind::Tui => "tui",
                Kind::RpcHost => "rpc_host",
            }
            .to_owned(),
            name: str_of("sessionName"),
            cwd: str_of("cwd"),
            streaming: flag("isStreaming"),
            compacting: flag("isCompacting") || flag("compacting"),
            blocking_question: flag("blocking_question"),
            pending_messages: state
                .get("pendingMessageCount")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            message_count: count,
            last_user: tail(&last_text(&messages, "user"), text_limit),
            last_assistant: tail(&last_text(&messages, "assistant"), text_limit),
            questions: pending_questions(&messages),
            messages_error,
            updated_at: updated_at(session_file.as_deref()).await,
        })
    }

    /// Every live session served under `rpc_dir`. A session served twice (a TUI also hosted
    /// by an RPC host) is reported once.
    pub async fn snapshot_local(rpc_dir: &Path, text_limit: usize) -> MachineSnapshot {
        let jobs = endpoints(rpc_dir).await;
        let permits = Arc::new(tokio::sync::Semaphore::new(PARALLEL_READS));
        let mut set = tokio::task::JoinSet::new();
        for (index, (socket, kind, routing)) in jobs.into_iter().enumerate() {
            let permits = Arc::clone(&permits);
            set.spawn(async move {
                let _permit = permits.acquire_owned().await;
                let view = session_view(&socket, kind, routing.as_deref(), text_limit).await;
                (index, socket, view)
            });
        }
        let mut views = Vec::new();
        while let Some(joined) = set.join_next().await {
            if let Ok(view) = joined {
                views.push(view);
            }
        }
        views.sort_by_key(|(index, _, _)| *index);
        let mut snapshot = MachineSnapshot::default();
        for (_, socket, view) in views {
            match view {
                Ok(session) => {
                    if !snapshot
                        .sessions
                        .iter()
                        .any(|seen| seen.session_id == session.session_id)
                    {
                        snapshot.sessions.push(session);
                    }
                }
                Err(RpcError::Dead) => {}
                Err(RpcError::Failed(error)) => snapshot.errors.push(SnapshotError {
                    endpoint: socket.display().to_string(),
                    error: compact(&error, 200),
                }),
            }
        }
        snapshot
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn hosts_come_from_the_env_first_then_the_seeded_file() {
        assert_eq!(
            parse_hosts(Some(" omarchy, local,box ,omarchy"), Some(r#"["seeded"]"#)),
            vec!["omarchy", "box"]
        );
        assert_eq!(parse_hosts(Some("  "), Some(r#"["seeded"]"#)), vec!["seeded"]);
        assert_eq!(
            parse_hosts(None, Some(r#"["-oProxyCommand=x","ok"]"#)),
            vec!["ok"]
        );
        assert!(parse_hosts(None, Some("not json")).is_empty());
        assert!(parse_hosts(None, None).is_empty());
    }

    #[test]
    fn remote_probe_runs_python_from_stdin_over_batch_ssh() {
        assert_eq!(
            remote_argv("omarchy", 1000),
            [
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=5",
                "--",
                "omarchy",
                "python3",
                "-",
                "snapshot",
                "1000"
            ]
        );
        assert!(REMOTE_PROBE.contains("def snapshot(limit)"));
    }

    #[test]
    fn remote_output_parses_or_reports_the_failure() {
        let ok = parse_remote(
            Some(0),
            br#"{"sessions":[{"session_id":"s1","routing_id":"s1","endpoint_kind":"tui","name":null,"cwd":"/w","streaming":true,"compacting":false,"blocking_question":false,"pending_messages":0,"message_count":3,"last_user":"u","last_assistant":"a","questions":[],"messages_error":null,"updated_at":null}],"errors":[]}"#,
            b"",
        )
        .unwrap();
        assert_eq!(ok.sessions[0].session_id, "s1");
        assert!(ok.sessions[0].streaming);
        assert_eq!(
            parse_remote(Some(255), b"", b"ssh: connect to host x port 22: refused\n").unwrap_err(),
            "exit 255: ssh: connect to host x port 22: refused"
        );
        assert_eq!(
            parse_remote(Some(5), b"{\"ok\":false}\n", b"").unwrap_err(),
            "exit 5: {\"ok\":false}"
        );
        assert!(parse_remote(Some(0), b"nope", b"").is_err());
    }

    #[test]
    fn windows_pipe_names_match_mhc() {
        // Expected digests computed independently (python hashlib) from mhc's rule: sha256 of
        // the normalized lowercase logical path, then the secret.
        let seven = [7u8; 32];
        let name = pipe_address("C:/Agent Dir/rpc.sock", &seven).unwrap();
        assert_eq!(name, "\\\\.\\pipe\\senpi-rpc-e28e5394702989fdf0a5e7f2e3a58ebf");
        assert_eq!(
            pipe_address("C:\\Agent Dir\\.\\..\\Agent Dir\\rpc.sock", &seven).unwrap(),
            name
        );
        assert_eq!(
            pipe_address("C:\\Users\\demo\\.omo\\rpc\\rpc.sock", &[]).unwrap(),
            "\\\\.\\pipe\\senpi-rpc-bc81003b5845d900bbd77b1dc7e57ba8"
        );
        assert_eq!(
            pipe_address("//srv/share/rpc.sock", b"k"),
            None,
            "forward-slash UNC is not a path mhc accepts"
        );
        assert_eq!(
            pipe_address("\\\\srv\\share\\rpc.sock", b"k").unwrap(),
            "\\\\.\\pipe\\senpi-rpc-3481c13fc8d373852cea410158c9ee1d"
        );
        assert_eq!(pipe_address("relative\\rpc.sock", &seven), None);
    }

    #[test]
    fn only_unanswered_questions_are_pending() {
        let ask = |id: &str| {
            json!({"role": "assistant", "content": [
                {"type": "text", "text": "need input"},
                {"type": "toolCall", "name": "ask_user_question", "id": id, "arguments": {"questions": [
                    {"header": "Pick", "question": "Ship?", "options": [{"label": "Yes"}, {"label": "No"}, {"bad": 1}]},
                    "not an object"
                ]}}
            ]})
        };
        let messages = vec![
            ask("call_done"),
            json!({"role": "toolResult", "toolName": "ask_user_question", "toolCallId": "call_done", "details": {"status": "answered"}}),
            ask("call_typed"),
            json!({"role": "user", "content": "[Answer to question call_typed] yes"}),
            ask("call_waiting"),
            json!({"role": "toolResult", "toolName": "ask_user_question", "toolCallId": "call_waiting", "details": {"status": "pending"}}),
        ];
        assert_eq!(
            pending_questions(&messages),
            vec![PendingQuestion {
                request_id: "call_waiting".into(),
                questions: vec![QuestionItem {
                    id: "q1".into(),
                    header: Some("Pick".into()),
                    question: Some("Ship?".into()),
                    options: vec!["Yes".into(), "No".into()],
                }],
            }]
        );
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn durable_ids_and_tails() {
        use super::local::{durable_id, tail};
        assert_eq!(
            durable_id("/a/--w--/2026-10-17T11-25-54-982Z_01a1499c-7da6.jsonl").as_deref(),
            Some("01a1499c-7da6")
        );
        assert_eq!(durable_id("/a/plain.jsonl"), None);
        assert_eq!(tail("한글abc", 4), "글abc");
        assert_eq!(tail("ab", 10), "ab");
    }

    #[cfg(unix)]
    mod sockets {
        use super::super::*;
        use serde_json::json;
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

        struct Scratch(PathBuf);
        impl Drop for Scratch {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        fn scratch() -> Scratch {
            // Short: unix socket paths are limited to about 100 bytes.
            let dir = std::env::temp_dir().join(format!(
                "fx-omo-{}",
                &uuid::Uuid::new_v4().simple().to_string()[..8]
            ));
            std::fs::create_dir_all(dir.join("tui")).unwrap();
            Scratch(dir)
        }

        /// Serves `reply(request)` on `path`. With `secret`, a request line must start with it.
        fn serve(
            path: &Path,
            secret: Option<&'static str>,
            reply: impl Fn(&Value) -> Value + Send + Sync + 'static,
        ) -> tokio::task::JoinHandle<()> {
            let listener = tokio::net::UnixListener::bind(path).unwrap();
            if let Some(secret) = secret {
                std::fs::write(super::super::local::secret_path(path), secret).unwrap();
            }
            let reply = std::sync::Arc::new(reply);
            tokio::spawn(async move {
                loop {
                    let Ok((stream, _)) = listener.accept().await else {
                        return;
                    };
                    let reply = std::sync::Arc::clone(&reply);
                    tokio::spawn(async move {
                        let (read, mut write) = stream.into_split();
                        let mut line = String::new();
                        if tokio::io::BufReader::new(read)
                            .read_line(&mut line)
                            .await
                            .is_err()
                        {
                            return;
                        }
                        let body = match secret {
                            Some(secret) => match line.strip_prefix(secret) {
                                Some(body) => body.to_owned(),
                                None => return,
                            },
                            None => line,
                        };
                        let Ok(request) = serde_json::from_str::<Value>(&body) else {
                            return;
                        };
                        let mut response = reply(&request);
                        response["id"] = request["id"].clone();
                        let out = format!(
                            "{}\n\n{}\n",
                            json!({"type": "agent_start", "id": "someone-else"}),
                            response
                        );
                        write.write_all(out.as_bytes()).await.unwrap();
                    });
                }
            })
        }

        fn ok(data: Value) -> Value {
            json!({"type": "response", "success": true, "data": data})
        }

        #[tokio::test]
        async fn tui_and_rpc_host_sessions_are_read_and_dead_sockets_skipped() {
            let dir = scratch();
            let root = dir.0.clone();
            let session_file = root.join("2026-10-29T00-00-00-000Z_durable-1.jsonl");
            std::fs::write(&session_file, "{}\n").unwrap();

            let _tui = serve(&root.join("tui/t-1.sock"), Some("s3cr3t"), |request| {
                match request["type"].as_str().unwrap() {
                    "get_state" => ok(json!({
                        "sessionId": "tui-1", "sessionName": "Release", "cwd": "/w",
                        "isStreaming": false, "messageCount": 0, "pendingMessageCount": 2
                    })),
                    "get_messages" => {
                        assert_eq!(request["sessionId"], "tui-1");
                        ok(json!({"messages": [
                            {"role": "user", "content": "deploy it"},
                            {"role": "assistant", "content": [
                                {"type": "text", "text": "Ready to ship."},
                                {"type": "toolCall", "name": "ask_user_question", "id": "call_1",
                                 "arguments": {"questions": [{"id": "go", "question": "Deploy now?", "options": [{"label": "Ship"}]}]}}
                            ]}
                        ]}))
                    }
                    other => panic!("unexpected {other}"),
                }
            });
            let file = session_file.display().to_string();
            let _host = serve(&root.join("p-0001.sock"), None, move |request| {
                match request["type"].as_str().unwrap() {
                    "list_sessions" => ok(json!({"sessions": [{"sessionId": "route-1"}, {"cwd": "/no-id"}]})),
                    "get_state" => {
                        assert_eq!(request["sessionId"], "route-1");
                        ok(json!({"sessionId": "route-1", "isStreaming": true, "isCompacting": false,
                                  "sessionFile": file, "messageCount": 7}))
                    }
                    "get_messages" => json!({"type": "response", "success": false, "error": "busy"}),
                    other => panic!("unexpected {other}"),
                }
            });
            // An empty mhc host, a stale socket file, a TUI whose secret is gone, and a TUI
            // that refuses: only the last one is an error.
            let _empty = serve(&root.join("p-0002.sock"), None, |_| ok(json!({"sessions": []})));
            drop(std::os::unix::net::UnixListener::bind(root.join("p-0003.sock")).unwrap());
            let _secretless = serve(&root.join("tui/t-2.sock"), None, |_| ok(json!({})));
            let _refusing = serve(&root.join("tui/t-3.sock"), Some("x"), |_| {
                json!({"type": "response", "success": false, "error": "not ready"})
            });

            let snapshot = snapshot_local(&root, 5).await;
            assert_eq!(snapshot.sessions.len(), 2, "{snapshot:?}");
            let tui = &snapshot.sessions[0];
            assert_eq!(tui.session_id, "tui-1");
            assert_eq!(tui.routing_id, "tui-1");
            assert_eq!(tui.endpoint_kind, "tui");
            assert_eq!(tui.name.as_deref(), Some("Release"));
            assert_eq!(tui.message_count, 2, "messageCount 0 falls back to the messages read");
            assert_eq!(tui.pending_messages, 2);
            assert_eq!(tui.last_user, "oy it", "text is cut to its last characters");
            assert_eq!(tui.last_assistant, "ship.");
            assert_eq!(tui.questions.len(), 1);
            assert_eq!(tui.questions[0].request_id, "call_1");
            assert_eq!(tui.questions[0].questions[0].id, "go");
            assert_eq!(tui.questions[0].questions[0].options, vec!["Ship"]);

            let hosted = &snapshot.sessions[1];
            assert_eq!(hosted.session_id, "durable-1");
            assert_eq!(hosted.routing_id, "route-1");
            assert_eq!(hosted.endpoint_kind, "rpc_host");
            assert!(hosted.streaming);
            assert_eq!(hosted.message_count, 7);
            assert_eq!(hosted.messages_error.as_deref(), Some("busy"));
            assert!(hosted.updated_at.as_deref().is_some_and(|at| at.ends_with('Z')));

            assert_eq!(snapshot.errors.len(), 1, "{:?}", snapshot.errors);
            assert!(snapshot.errors[0].endpoint.ends_with("tui/t-3.sock"));
            assert_eq!(snapshot.errors[0].error, "not ready");
        }

        #[tokio::test]
        async fn a_missing_rpc_dir_is_an_empty_machine() {
            let dir = scratch();
            let snapshot = snapshot_local(&dir.0.join("absent"), 10).await;
            assert_eq!(snapshot, MachineSnapshot::default());
        }
    }
}
