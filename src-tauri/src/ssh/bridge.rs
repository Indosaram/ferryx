//! Async framed OpenSSH client for remote helper communication and daemon reconnection.

use crate::ipc::IpcError;
use crate::scoped_contracts::{Epoch, TargetRef};
use crate::ssh::direct;
use crate::ssh::helper_setup::{self, HelperLocation};
use crate::ssh::runtime::RemoteEnvironment;
use crate::ssh::SshHost;
use base64::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader, BufWriter};
use tokio::sync::Mutex;

pub const MAX_FRAME: usize = 1024 * 1024;
pub const PROTOCOL_VERSION: u32 = 1;
const DEFAULT_RPC_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_STDERR_BYTES: usize = 64 * 1024;

/// Remote process identifier on the SSH host.
///
/// Intentionally isolated: must NEVER be exposed or converted as a local POSIX
/// pid or signal target. Remote lifecycle must be controlled exclusively via
/// framed helper RPC (`pty.stop`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RemotePid(pub u32);

impl RemotePid {
    pub fn as_u32(&self) -> u32 {
        self.0
    }
}

impl std::fmt::Display for RemotePid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Canonical decimal u64 remote ring-buffer cursor.
///
/// Guarantees that remote cursors are validated and preserved as non-negative
/// integers, preventing unvalidated string propagation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RemoteCursor(pub u64);

impl RemoteCursor {
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for RemoteCursor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::ops::Deref for RemoteCursor {
    type Target = u64;
    fn deref(&self) -> &u64 {
        &self.0
    }
}

impl From<u64> for RemoteCursor {
    fn from(v: u64) -> Self {
        Self(v)
    }
}

impl Serialize for RemoteCursor {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for RemoteCursor {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct CursorVisitor;
        impl<'de> serde::de::Visitor<'de> for CursorVisitor {
            type Value = RemoteCursor;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("canonical decimal u64 string or non-negative integer")
            }

            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                let n = v.parse::<u64>().map_err(E::custom)?;
                if n.to_string() != v {
                    return Err(E::custom("cursor string must be canonical decimal u64"));
                }
                Ok(RemoteCursor(n))
            }

            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(RemoteCursor(v))
            }

            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
                if v < 0 {
                    return Err(E::custom("cursor cannot be negative"));
                }
                Ok(RemoteCursor(v as u64))
            }
        }
        deserializer.deserialize_any(CursorVisitor)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandshakeResult {
    pub protocol: u32,
    pub capabilities: Vec<String>,
    pub host_id: String,
    pub owner_id: String,
    pub epoch: Epoch,
    pub os: String,
    pub arch: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SpawnParams {
    pub project_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cols: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub program: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_request_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpawnResult {
    pub target: TargetRef,
    pub pid: RemotePid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeResult {
    pub target: TargetRef,
    pub pid: RemotePid,
    pub cwd: PathBuf,
    pub cols: u16,
    pub rows: u16,
    pub cursor: RemoteCursor,
    pub exited: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadChunk {
    pub cursor: RemoteCursor,
    pub sequence: u64,
    #[serde(rename = "data")]
    pub data_base64: String,
    #[serde(default, skip_serializing)]
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadResult {
    pub target: TargetRef,
    pub pid: RemotePid,
    pub cwd: PathBuf,
    pub cursor: RemoteCursor,
    pub after_sequence: u64,
    pub gap: bool,
    pub exited: bool,
    pub chunks: Vec<ReadChunk>,
}

impl ReadResult {
    /// Returns the concatenated raw bytes of all chunk payloads.
    /// Bytes are decoded authoritatively from base64 data.
    pub fn decoded_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for chunk in &self.chunks {
            out.extend_from_slice(&chunk.bytes);
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteResult {
    pub accepted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResizeResult {
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StopResult {
    pub stopped: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("SSH plan error: {0}")]
    SshPlan(String),

    #[error("SSH setup error: {}", .0.message)]
    SshSetup(IpcError),

    #[error("Process spawn failed: {0}")]
    ProcessSpawn(String),

    #[error("Process exited unexpectedly with code {code:?}: {stderr}")]
    ProcessExited { code: Option<i32>, stderr: String },

    #[error("Frame length {0} exceeds maximum allowed frame size (1 MiB)")]
    FrameTooLarge(usize),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Remote error: {0}")]
    Remote(String),

    #[error("Operation timed out: {0}")]
    Timeout(String),

    #[error("Configured host ID mismatch: expected {expected}, remote helper reported {actual}")]
    HostMismatch { expected: String, actual: String },

    #[error("Target expired or mismatched: expected owner {expected_owner}, epoch {expected_epoch:?}; remote helper has owner {actual_owner}, epoch {actual_epoch:?}")]
    TargetExpired {
        expected_owner: String,
        expected_epoch: Epoch,
        actual_owner: String,
        actual_epoch: Epoch,
    },

    #[error("Remote helper rejected target as expired (TARGET_EXPIRED)")]
    RemoteTargetExpired,

    #[error("Target mismatch: expected {expected:?}, remote helper returned {actual:?}")]
    TargetMismatch {
        expected: TargetRef,
        actual: TargetRef,
    },

    #[error("Target not found on remote helper")]
    TargetNotFound,

    #[error("Connection closed or poisoned")]
    ConnectionClosed,
}

impl From<IpcError> for BridgeError {
    fn from(err: IpcError) -> Self {
        BridgeError::SshSetup(err)
    }
}

pub fn validate_target_handshake(
    handshake: &HandshakeResult,
    configured_host_id: &str,
    target: Option<&TargetRef>,
) -> Result<(), BridgeError> {
    if handshake.host_id != configured_host_id {
        return Err(BridgeError::HostMismatch {
            expected: configured_host_id.to_string(),
            actual: handshake.host_id.clone(),
        });
    }
    if let Some(t) = target {
        if t.host_id != configured_host_id
            || t.owner_id != handshake.owner_id
            || t.epoch != handshake.epoch
        {
            return Err(BridgeError::TargetExpired {
                expected_owner: t.owner_id.clone(),
                expected_epoch: t.epoch,
                actual_owner: handshake.owner_id.clone(),
                actual_epoch: handshake.epoch,
            });
        }
    }
    Ok(())
}

pub async fn write_frame_async<W: AsyncWrite + Unpin>(
    writer: &mut W,
    value: &Value,
    timeout_dur: Duration,
) -> Result<(), BridgeError> {
    let bytes = serde_json::to_vec(value)
        .map_err(|e| BridgeError::Protocol(format!("Failed to serialize request: {e}")))?;
    if bytes.len() > MAX_FRAME {
        return Err(BridgeError::FrameTooLarge(bytes.len()));
    }
    let len_header = (bytes.len() as u32).to_be_bytes();

    tokio::time::timeout(timeout_dur, async {
        writer.write_all(&len_header).await?;
        writer.write_all(&bytes).await?;
        writer.flush().await?;
        Ok::<(), std::io::Error>(())
    })
    .await
    .map_err(|_| BridgeError::Timeout("Frame write timed out".into()))?
    .map_err(BridgeError::Io)
}

pub async fn read_frame_async<R: AsyncRead + Unpin>(
    reader: &mut R,
    timeout_dur: Duration,
) -> Result<Option<Value>, BridgeError> {
    tokio::time::timeout(timeout_dur, async {
        let mut header = [0u8; 4];
        match reader.read(&mut header[..1]).await? {
            0 => return Ok(None),
            _ => (),
        }
        reader.read_exact(&mut header[1..4]).await?;
        let len = u32::from_be_bytes(header) as usize;
        if len > MAX_FRAME {
            return Err(BridgeError::FrameTooLarge(len));
        }
        let mut buf = vec![0u8; len];
        reader.read_exact(&mut buf).await?;
        let val: Value = serde_json::from_slice(&buf)
            .map_err(|e| BridgeError::Protocol(format!("Invalid response JSON: {e}")))?;
        Ok(Some(val))
    })
    .await
    .map_err(|_| BridgeError::Timeout("Frame read timed out".into()))?
}

/// A single framed SSH bridge connection managing an owned SSH child process.
pub struct BridgeConnection {
    writer: Option<BufWriter<tokio::process::ChildStdin>>,
    reader: Option<BufReader<tokio::process::ChildStdout>>,
    child: Option<tokio::process::Child>,
    stderr_capture: Arc<std::sync::Mutex<Vec<u8>>>,
    stderr_task: Option<tokio::task::JoinHandle<()>>,
    closed: bool,
    poisoned: bool,
}

impl BridgeConnection {
    /// Spawns a new framed OpenSSH bridge connection to `host`.
    pub async fn spawn(
        host: &SshHost,
        env: &RemoteEnvironment,
        location: &HelperLocation,
    ) -> Result<Self, BridgeError> {
        let plan = direct::bridge_plan(host, env, location)?;
        let child = direct::spawn_child(&plan, Stdio::piped())
            .map_err(|e| BridgeError::ProcessSpawn(e.message))?;
        Self::from_child(child)
    }

    /// Creates a framed bridge connection from any spawned `Child` with piped stdio.
    pub fn from_child(mut child: tokio::process::Child) -> Result<Self, BridgeError> {
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| BridgeError::ProcessSpawn("Child stdin not available".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| BridgeError::ProcessSpawn("Child stdout not available".into()))?;
        let mut stderr = child
            .stderr
            .take()
            .ok_or_else(|| BridgeError::ProcessSpawn("Child stderr not available".into()))?;

        let stderr_capture = Arc::new(std::sync::Mutex::new(Vec::new()));
        let stderr_sink = Arc::clone(&stderr_capture);

        let stderr_task = tokio::spawn(async move {
            let mut buf = [0u8; 4096];
            while let Ok(n) = stderr.read(&mut buf).await {
                if n == 0 {
                    break;
                }
                let mut lock = stderr_sink.lock().unwrap();
                let remaining = MAX_STDERR_BYTES.saturating_sub(lock.len());
                if remaining > 0 {
                    let to_copy = n.min(remaining);
                    lock.extend_from_slice(&buf[..to_copy]);
                }
            }
        });

        Ok(Self {
            writer: Some(BufWriter::new(stdin)),
            reader: Some(BufReader::new(stdout)),
            child: Some(child),
            stderr_capture,
            stderr_task: Some(stderr_task),
            closed: false,
            poisoned: false,
        })
    }

    /// Returns the OS process ID of the owned child process, if still active.
    pub fn child_id(&self) -> Option<u32> {
        self.child.as_ref().and_then(|c| c.id())
    }

    fn check_stderr(&self) -> String {
        let lock = self.stderr_capture.lock().unwrap();
        String::from_utf8_lossy(&lock).trim().to_string()
    }

    /// Dispatches a typed RPC operation over the framed bridge stream.
    ///
    /// Correctness guarantee: poisons the connection BEFORE awaiting any I/O.
    /// If write/read times out, errors, or is cancelled, the connection remains
    /// permanently poisoned, owned resources are cleaned up idempotently, and no
    /// subsequent request can perform a second write or consume a misassociated response.
    pub async fn request(
        &mut self,
        op: &str,
        params: Value,
        timeout_dur: Duration,
    ) -> Result<Value, BridgeError> {
        if self.closed || self.poisoned {
            let _ = self.close().await;
            return Err(BridgeError::ConnectionClosed);
        }

        // Poison connection BEFORE awaiting I/O.
        self.poisoned = true;

        let writer = match self.writer.as_mut() {
            Some(w) => w,
            None => {
                let _ = self.close().await;
                return Err(BridgeError::ConnectionClosed);
            }
        };

        let request = json!({
            "protocol": PROTOCOL_VERSION,
            "token": "",
            "op": op,
            "params": params,
        });

        if let Err(e) = write_frame_async(writer, &request, timeout_dur).await {
            let stderr = self.check_stderr();
            let _ = self.close().await;
            return Err(match e {
                BridgeError::Io(_) if !stderr.is_empty() => BridgeError::ProcessExited {
                    code: None,
                    stderr,
                },
                other => other,
            });
        }

        let reader = match self.reader.as_mut() {
            Some(r) => r,
            None => {
                let _ = self.close().await;
                return Err(BridgeError::ConnectionClosed);
            }
        };

        let response = match read_frame_async(reader, timeout_dur).await {
            Ok(Some(v)) => v,
            Ok(None) => {
                let stderr = self.check_stderr();
                let exit_code = if let Some(child) = self.child.as_mut() {
                    child.try_wait().ok().flatten().and_then(|s| s.code())
                } else {
                    None
                };
                let _ = self.close().await;
                return Err(BridgeError::ProcessExited {
                    code: exit_code,
                    stderr,
                });
            }
            Err(e) => {
                let stderr = self.check_stderr();
                let _ = self.close().await;
                return Err(match e {
                    BridgeError::Io(_) if !stderr.is_empty() => BridgeError::ProcessExited {
                        code: None,
                        stderr,
                    },
                    other => other,
                });
            }
        };

        let ok = response
            .get("ok")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if ok {
            // Restore usability ONLY after a complete, valid response frame is parsed
            self.poisoned = false;
            Ok(response.get("data").cloned().unwrap_or(Value::Null))
        } else {
            // A structured remote error was cleanly received; framing is intact
            self.poisoned = false;
            let error_msg = response
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("Unknown remote helper error")
                .to_string();

            if error_msg == "TARGET_EXPIRED" {
                Err(BridgeError::RemoteTargetExpired)
            } else if error_msg == "NOT_FOUND" {
                Err(BridgeError::TargetNotFound)
            } else {
                Err(BridgeError::Remote(error_msg))
            }
        }
    }

    /// Performs the helper protocol handshake.
    pub async fn handshake(&mut self) -> Result<HandshakeResult, BridgeError> {
        let val = self
            .request("handshake", json!({}), DEFAULT_RPC_TIMEOUT)
            .await?;
        serde_json::from_value(val)
            .map_err(|e| BridgeError::Protocol(format!("Invalid handshake response: {e}")))
    }

    /// Registers a project path on the remote helper.
    pub async fn project_register(&mut self, id: &str, path: &str) -> Result<String, BridgeError> {
        let val = self
            .request(
                "project.register",
                json!({ "id": id, "path": path }),
                DEFAULT_RPC_TIMEOUT,
            )
            .await?;
        val.get("projectId")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| BridgeError::Protocol("Missing projectId in response".into()))
    }

    /// Creates a worktree on the remote helper.
    pub async fn worktree_create(
        &mut self,
        project_id: &str,
        slug: &str,
    ) -> Result<String, BridgeError> {
        let val = self
            .request(
                "worktree.create",
                json!({ "projectId": project_id, "slug": slug }),
                DEFAULT_RPC_TIMEOUT,
            )
            .await?;
        val.get("worktree")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| BridgeError::Protocol("Missing worktree in response".into()))
    }

    /// Spawns a PTY process on the remote helper.
    pub async fn pty_spawn(&mut self, params: &SpawnParams) -> Result<SpawnResult, BridgeError> {
        let params_val = serde_json::to_value(params)
            .map_err(|e| BridgeError::Protocol(format!("Invalid spawn params: {e}")))?;
        let val = self
            .request("pty.spawn", params_val, DEFAULT_RPC_TIMEOUT)
            .await?;
        serde_json::from_value(val)
            .map_err(|e| BridgeError::Protocol(format!("Invalid spawn response: {e}")))
    }

    /// Queries the state of an existing PTY session on the remote helper.
    pub async fn pty_describe(&mut self, target: &TargetRef) -> Result<DescribeResult, BridgeError> {
        let val = self
            .request(
                "pty.describe",
                json!({ "target": target }),
                DEFAULT_RPC_TIMEOUT,
            )
            .await?;
        let describe: DescribeResult = serde_json::from_value(val)
            .map_err(|e| BridgeError::Protocol(format!("Invalid describe response: {e}")))?;
        if &describe.target != target {
            return Err(BridgeError::TargetMismatch {
                expected: target.clone(),
                actual: describe.target,
            });
        }
        Ok(describe)
    }

    /// Reads output chunks from an existing PTY session.
    ///
    /// Cursor is a canonical decimal u64 string representing remote ring-buffer chunk
    /// sequence, NOT local byte sequence. Chunks are authoritatively decoded from base64.
    pub async fn pty_read(
        &mut self,
        target: &TargetRef,
        cursor: impl Into<RemoteCursor>,
        wait_ms: u64,
    ) -> Result<ReadResult, BridgeError> {
        let cursor = cursor.into();
        let timeout = Duration::from_millis(wait_ms.saturating_add(5000));
        let val = self
            .request(
                "pty.read",
                json!({ "target": target, "cursor": cursor.to_string(), "waitMs": wait_ms }),
                timeout,
            )
            .await?;
        let mut read_res: ReadResult = serde_json::from_value(val)
            .map_err(|e| BridgeError::Protocol(format!("Invalid read response: {e}")))?;

        if &read_res.target != target {
            return Err(BridgeError::TargetMismatch {
                expected: target.clone(),
                actual: read_res.target,
            });
        }

        // Authoritatively decode base64 chunk payload
        for chunk in &mut read_res.chunks {
            chunk.bytes = BASE64_STANDARD
                .decode(&chunk.data_base64)
                .map_err(|e| BridgeError::Protocol(format!("Invalid base64 in chunk data: {e}")))?;
        }
        Ok(read_res)
    }

    /// Writes raw byte payload to the PTY.
    /// Single-attempt, never queued or replayed across reconnects.
    pub async fn pty_write(
        &mut self,
        target: &TargetRef,
        data: &[u8],
    ) -> Result<WriteResult, BridgeError> {
        let b64 = BASE64_STANDARD.encode(data);
        let val = self
            .request(
                "pty.write",
                json!({ "target": target, "data": b64 }),
                DEFAULT_RPC_TIMEOUT,
            )
            .await?;
        serde_json::from_value(val)
            .map_err(|e| BridgeError::Protocol(format!("Invalid write response: {e}")))
    }

    /// Resizes the PTY window dimensions.
    pub async fn pty_resize(
        &mut self,
        target: &TargetRef,
        cols: u16,
        rows: u16,
    ) -> Result<ResizeResult, BridgeError> {
        let val = self
            .request(
                "pty.resize",
                json!({ "target": target, "cols": cols, "rows": rows }),
                DEFAULT_RPC_TIMEOUT,
            )
            .await?;
        serde_json::from_value(val)
            .map_err(|e| BridgeError::Protocol(format!("Invalid resize response: {e}")))
    }

    /// Stops the PTY child process. Single-attempt.
    pub async fn pty_stop(&mut self, target: &TargetRef) -> Result<StopResult, BridgeError> {
        let val = self
            .request(
                "pty.stop",
                json!({ "target": target }),
                DEFAULT_RPC_TIMEOUT,
            )
            .await?;
        serde_json::from_value(val)
            .map_err(|e| BridgeError::Protocol(format!("Invalid stop response: {e}")))
    }

    /// Lists all active PTY sessions on the remote helper.
    pub async fn pty_list(&mut self) -> Result<Vec<DescribeResult>, BridgeError> {
        let val = self
            .request("pty.list", json!({}), DEFAULT_RPC_TIMEOUT)
            .await?;
        serde_json::from_value(val)
            .map_err(|e| BridgeError::Protocol(format!("Invalid pty.list response: {e}")))
    }

    /// Explicitly closes the connection and boundedly reaps all owned resources.
    /// Idempotency is governed by `Option::take()` on resources, never by early
    /// return on `closed`.
    pub async fn close(&mut self) -> Result<(), BridgeError> {
        self.closed = true;
        self.poisoned = true;

        drop(self.writer.take());
        drop(self.reader.take());

        if let Some(task) = self.stderr_task.take() {
            task.abort();
            let _ = tokio::time::timeout(Duration::from_millis(200), task).await;
        }

        if let Some(mut child) = self.child.take() {
            // First bounded wait for child to exit on stdin closure
            let first_wait = tokio::time::timeout(Duration::from_millis(500), child.wait()).await;
            if first_wait.is_err() {
                let _ = child.start_kill();
                // Bounded wait after kill signal; NEVER unbounded!
                let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
            }
        }

        Ok(())
    }
}

impl Drop for BridgeConnection {
    fn drop(&mut self) {
        self.closed = true;
        self.poisoned = true;

        drop(self.writer.take());
        drop(self.reader.take());

        if let Some(task) = self.stderr_task.take() {
            task.abort();
        }

        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
        }
    }
}

/// High-level async framed OpenSSH client managing dual independent connections:
/// - `control`: for handshake, spawn, describe, write, resize, stop, project registration
/// - `reader`: dedicated to long-poll `pty.read` frames so input is never blocked
#[derive(Clone)]
pub struct SshBridgeClient {
    control: Arc<Mutex<BridgeConnection>>,
    reader: Arc<Mutex<BridgeConnection>>,
    host_id: String,
    owner_id: String,
    epoch: Epoch,
    handshake: HandshakeResult,
}

impl SshBridgeClient {
    /// Connects to the remote helper over SSH, starting the daemon if necessary,
    /// establishing dual independent transport connections and validating identity.
    pub async fn connect(
        host: &SshHost,
        env: &RemoteEnvironment,
        location: &HelperLocation,
    ) -> Result<Self, BridgeError> {
        Self::connect_with_target(host, env, location, None).await
    }

    /// Connects to the remote helper over SSH, validating handshake identity against both
    /// the configured host and an optional restored target ref.
    pub async fn connect_with_target(
        host: &SshHost,
        env: &RemoteEnvironment,
        location: &HelperLocation,
        target: Option<&TargetRef>,
    ) -> Result<Self, BridgeError> {
        // Ensure remote helper daemon is started (idempotent, no silent build/trust modifications)
        helper_setup::ensure_started(host, env, location).await?;

        // Open control connection and execute initial handshake
        let mut control = BridgeConnection::spawn(host, env, location).await?;
        let handshake = control.handshake().await?;

        // Protocol & capability verification
        if handshake.protocol != PROTOCOL_VERSION {
            let _ = control.close().await;
            return Err(BridgeError::Protocol(format!(
                "Unsupported protocol version: expected {PROTOCOL_VERSION}, got {}",
                handshake.protocol
            )));
        }
        if !handshake.capabilities.iter().any(|c| c == "sshHelperV1") {
            let _ = control.close().await;
            return Err(BridgeError::Protocol(
                "Helper missing required capability 'sshHelperV1'".into(),
            ));
        }

        // Handshake must independently match configured host and restored target
        if let Err(e) = validate_target_handshake(&handshake, &host.id, target) {
            let _ = control.close().await;
            return Err(e);
        }

        // Open independent read connection and verify handshake parity
        let mut reader = BridgeConnection::spawn(host, env, location).await?;
        let reader_handshake = reader.handshake().await?;

        if reader_handshake.protocol != PROTOCOL_VERSION {
            let _ = control.close().await;
            let _ = reader.close().await;
            return Err(BridgeError::Protocol(format!(
                "Reader connection unsupported protocol version: expected {PROTOCOL_VERSION}, got {}",
                reader_handshake.protocol
            )));
        }
        if !reader_handshake.capabilities.iter().any(|c| c == "sshHelperV1") {
            let _ = control.close().await;
            let _ = reader.close().await;
            return Err(BridgeError::Protocol(
                "Reader helper missing required capability 'sshHelperV1'".into(),
            ));
        }

        if reader_handshake.host_id != handshake.host_id
            || reader_handshake.owner_id != handshake.owner_id
            || reader_handshake.epoch != handshake.epoch
        {
            let _ = control.close().await;
            let _ = reader.close().await;
            return Err(BridgeError::Protocol(
                "Reader connection handshake does not match control connection".into(),
            ));
        }

        Ok(Self {
            host_id: handshake.host_id.clone(),
            owner_id: handshake.owner_id.clone(),
            epoch: handshake.epoch,
            handshake,
            control: Arc::new(Mutex::new(control)),
            reader: Arc::new(Mutex::new(reader)),
        })
    }

    /// Constructs an `SshBridgeClient` from existing dual connections and expected host ID.
    /// Useful for local tests, mock transports, or in-process verification.
    pub async fn from_connections(
        mut control: BridgeConnection,
        mut reader: BridgeConnection,
        expected_host_id: &str,
    ) -> Result<Self, BridgeError> {
        let handshake = control.handshake().await?;
        if handshake.protocol != PROTOCOL_VERSION {
            let _ = control.close().await;
            let _ = reader.close().await;
            return Err(BridgeError::Protocol(format!(
                "Unsupported protocol version: expected {PROTOCOL_VERSION}, got {}",
                handshake.protocol
            )));
        }
        if !handshake.capabilities.iter().any(|c| c == "sshHelperV1") {
            let _ = control.close().await;
            let _ = reader.close().await;
            return Err(BridgeError::Protocol(
                "Helper missing required capability 'sshHelperV1'".into(),
            ));
        }
        if handshake.host_id != expected_host_id {
            let _ = control.close().await;
            let _ = reader.close().await;
            return Err(BridgeError::HostMismatch {
                expected: expected_host_id.to_string(),
                actual: handshake.host_id,
            });
        }

        let reader_handshake = reader.handshake().await?;
        if reader_handshake.protocol != PROTOCOL_VERSION {
            let _ = control.close().await;
            let _ = reader.close().await;
            return Err(BridgeError::Protocol(format!(
                "Reader connection unsupported protocol version: expected {PROTOCOL_VERSION}, got {}",
                reader_handshake.protocol
            )));
        }
        if !reader_handshake.capabilities.iter().any(|c| c == "sshHelperV1") {
            let _ = control.close().await;
            let _ = reader.close().await;
            return Err(BridgeError::Protocol(
                "Reader helper missing required capability 'sshHelperV1'".into(),
            ));
        }
        if reader_handshake.host_id != handshake.host_id
            || reader_handshake.owner_id != handshake.owner_id
            || reader_handshake.epoch != handshake.epoch
        {
            let _ = control.close().await;
            let _ = reader.close().await;
            return Err(BridgeError::Protocol(
                "Reader connection handshake does not match control connection".into(),
            ));
        }

        Ok(Self {
            host_id: handshake.host_id.clone(),
            owner_id: handshake.owner_id.clone(),
            epoch: handshake.epoch,
            handshake,
            control: Arc::new(Mutex::new(control)),
            reader: Arc::new(Mutex::new(reader)),
        })
    }

    pub fn host_id(&self) -> &str {
        &self.host_id
    }

    pub fn owner_id(&self) -> &str {
        &self.owner_id
    }

    pub fn epoch(&self) -> Epoch {
        self.epoch
    }

    pub fn handshake_info(&self) -> &HandshakeResult {
        &self.handshake
    }

    /// Validates that a restored `TargetRef` matches the live helper identity.
    pub fn validate_target(&self, target: &TargetRef) -> Result<(), BridgeError> {
        validate_target_handshake(&self.handshake, &self.host_id, Some(target))
    }

    pub async fn project_register(&self, id: &str, path: &str) -> Result<String, BridgeError> {
        let mut ctrl = self.control.lock().await;
        ctrl.project_register(id, path).await
    }

    pub async fn worktree_create(
        &self,
        project_id: &str,
        slug: &str,
    ) -> Result<String, BridgeError> {
        let mut ctrl = self.control.lock().await;
        ctrl.worktree_create(project_id, slug).await
    }

    pub async fn pty_spawn(&self, params: &SpawnParams) -> Result<SpawnResult, BridgeError> {
        let mut ctrl = self.control.lock().await;
        ctrl.pty_spawn(params).await
    }

    /// Spawns a PTY with a stable clientRequestId, validating retry contract.
    pub async fn pty_spawn_retry(
        &self,
        params: &SpawnParams,
        expected_client_request_id: &str,
    ) -> Result<SpawnResult, BridgeError> {
        if params.client_request_id.as_deref() != Some(expected_client_request_id) {
            return Err(BridgeError::Protocol(format!(
                "Spawn retry must preserve stable clientRequestId '{expected_client_request_id}'"
            )));
        }
        self.pty_spawn(params).await
    }

    pub async fn pty_describe(&self, target: &TargetRef) -> Result<DescribeResult, BridgeError> {
        self.validate_target(target)?;
        let mut ctrl = self.control.lock().await;
        ctrl.pty_describe(target).await
    }

    /// Reattaches to an existing target via `pty.describe`.
    ///
    /// Validates target host, owner, and epoch; missing or expired targets return
    /// explicit typed failure, NEVER fallback spawn.
    pub async fn reattach(&self, target: &TargetRef) -> Result<DescribeResult, BridgeError> {
        self.validate_target(target)?;
        let mut ctrl = self.control.lock().await;
        ctrl.pty_describe(target).await
    }

    /// Single-attempt write on the control connection.
    /// Never queued or replayed across reconnection.
    pub async fn pty_write(
        &self,
        target: &TargetRef,
        data: &[u8],
    ) -> Result<WriteResult, BridgeError> {
        self.validate_target(target)?;
        let mut ctrl = self.control.lock().await;
        ctrl.pty_write(target, data).await
    }

    pub async fn pty_resize(
        &self,
        target: &TargetRef,
        cols: u16,
        rows: u16,
    ) -> Result<ResizeResult, BridgeError> {
        self.validate_target(target)?;
        let mut ctrl = self.control.lock().await;
        ctrl.pty_resize(target, cols, rows).await
    }

    /// Single-attempt stop on the control connection.
    pub async fn pty_stop(&self, target: &TargetRef) -> Result<StopResult, BridgeError> {
        self.validate_target(target)?;
        let mut ctrl = self.control.lock().await;
        ctrl.pty_stop(target).await
    }

    pub async fn pty_list(&self) -> Result<Vec<DescribeResult>, BridgeError> {
        let mut ctrl = self.control.lock().await;
        ctrl.pty_list().await
    }

    /// Reads output on the dedicated reader connection.
    /// Running a long-poll read on `reader` never blocks `pty_write` on `control`.
    pub async fn pty_read(
        &self,
        target: &TargetRef,
        cursor: impl Into<RemoteCursor>,
        wait_ms: u64,
    ) -> Result<ReadResult, BridgeError> {
        self.validate_target(target)?;
        let mut rdr = self.reader.lock().await;
        rdr.pty_read(target, cursor, wait_ms).await
    }

    /// Closes both control and read connections and reaps their child processes.
    pub async fn close(&self) -> Result<(), BridgeError> {
        let mut ctrl = self.control.lock().await;
        let mut rdr = self.reader.lock().await;
        let _ = ctrl.close().await;
        let _ = rdr.close().await;
        Ok(())
    }

    /// Consumes the client and splits it into the two raw independent connections.
    pub fn into_split(self) -> Result<(BridgeConnection, BridgeConnection), BridgeError> {
        let ctrl = Arc::try_unwrap(self.control)
            .map_err(|_| BridgeError::Protocol("Control connection is still borrowed".into()))?
            .into_inner();
        let rdr = Arc::try_unwrap(self.reader)
            .map_err(|_| BridgeError::Protocol("Reader connection is still borrowed".into()))?
            .into_inner();
        Ok((ctrl, rdr))
    }
}

#[cfg(test)]
#[path = "bridge_tests.rs"]
mod tests;
