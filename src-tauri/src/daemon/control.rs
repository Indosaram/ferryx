//! Frozen, version-stable daemon control channel ("control-v1").
//!
//! The control channel is separate from the versioned data protocol (protocol.rs): it lets a
//! client of any revision ask a daemon of any other revision who it is, how many live sessions it
//! owns, and to upgrade, retire or shut down. Its schema is frozen at v1 and additive-only forever:
//! new optional fields and new ops may be added; nothing may be removed, renamed or retyped. An
//! unknown op answers { "ok": false, "code": "UNSUPPORTED_OP" } and unknown fields are ignored.
//! src-tauri/tests/fixtures/control-v1.json pins the canonical frames; the round-trip test below
//! fails the suite if a change breaks v1.
//!
//! Wire: one JSON object per line. Request { "control": 1, "op": <string>, ...args }; response
//! { "ok": true, ... } or { "ok": false, "code": <string> }.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Schema version carried in the "control" field of every request. Frozen.
pub const CONTROL_SCHEMA_VERSION: u64 = 1;
/// Capability advertised in HandshakeOk.capabilities by a daemon that serves this channel.
pub const CONTROL_CAPABILITY: &str = "control-v1";
/// Op name a daemon lists in its Hello when it speaks the frozen handover-v1 wire.
pub const HANDOVER_V1_OP: &str = "handover-v1";

/// Unix endpoint file name inside the runtime dir.
pub const CONTROL_SOCKET_FILE: &str = "control.sock";
/// Windows loopback rendezvous files inside the runtime dir.
pub const CONTROL_PORT_FILE: &str = "control.port";
pub const CONTROL_TOKEN_FILE: &str = "control.token";
/// Field of the first frame that carries the Windows bearer token.
pub const CONTROL_TOKEN_FIELD: &str = "token";

pub const OP_HELLO: &str = "Hello";
pub const OP_INVENTORY: &str = "Inventory";
pub const OP_REQUEST_UPGRADE: &str = "RequestUpgrade";
pub const OP_RETIRE_WHEN_IDLE: &str = "RetireWhenIdle";
pub const OP_SHUTDOWN: &str = "Shutdown";

/// Every op this revision serves; returned in Hello's ops.
pub const FROZEN_OPS: &[&str] = &[
    OP_HELLO,
    OP_INVENTORY,
    OP_REQUEST_UPGRADE,
    OP_RETIRE_WHEN_IDLE,
    OP_SHUTDOWN,
];

pub const CODE_UNSUPPORTED_OP: &str = "UNSUPPORTED_OP";
pub const CODE_BAD_REQUEST: &str = "BAD_REQUEST";
pub const CODE_UNSUPPORTED_CONTROL_VERSION: &str = "UNSUPPORTED_CONTROL_VERSION";
pub const CODE_UPGRADE_UNSUPPORTED: &str = "UPGRADE_UNSUPPORTED";
/// RequestUpgrade refused for now (the successor job is busy or could not be started, or a
/// handover is already in progress); the daemon keeps serving everything. Additive (todo 8).
pub const CODE_UPGRADE_REFUSED: &str = "UPGRADE_REFUSED";
pub const CODE_HANDOVER_WIRE_UNSUPPORTED: &str = "HANDOVER_WIRE_UNSUPPORTED";
pub const CODE_SPAWN_IN_FLIGHT: &str = "SPAWN_IN_FLIGHT";
pub const CODE_NOT_ACTIVE_OWNER: &str = "NOT_ACTIVE_OWNER";
pub const CODE_COUNT_CHANGED: &str = "COUNT_CHANGED";
pub const CODE_DAEMON_RETIRING: &str = "DAEMON_RETIRING";

/// One decoded control request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlRequest {
    Hello,
    Inventory,
    RequestUpgrade {
        staged_executable: String,
        generation_key: Option<String>,
    },
    RetireWhenIdle,
    Shutdown {
        confirm_live_sessions: u32,
    },
    /// An op this revision does not know; answered with UNSUPPORTED_OP.
    Unsupported { op: String },
}

impl ControlRequest {
    pub fn op(&self) -> &str {
        match self {
            ControlRequest::Hello => OP_HELLO,
            ControlRequest::Inventory => OP_INVENTORY,
            ControlRequest::RequestUpgrade { .. } => OP_REQUEST_UPGRADE,
            ControlRequest::RetireWhenIdle => OP_RETIRE_WHEN_IDLE,
            ControlRequest::Shutdown { .. } => OP_SHUTDOWN,
            ControlRequest::Unsupported { op } => op.as_str(),
        }
    }
}

#[derive(Deserialize)]
struct RequestUpgradeArgs {
    staged_executable: String,
    #[serde(default)]
    generation_key: Option<String>,
}

#[derive(Deserialize)]
struct ShutdownArgs {
    confirm_live_sessions: u32,
}

/// Hello reply body. generation_key is serialized as null when absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloReply {
    pub daemon_version: String,
    pub protocol_version: u32,
    pub pid: u32,
    pub start_time: u64,
    pub generation_key: Option<String>,
    pub epoch: u64,
    pub ops: Vec<String>,
}

/// Inventory reply body: per-kind live counts from handover::live_session_counts, which applies
/// todo 11's live definition (reader_open for local, connected for ssh/paired); exited or
/// disconnected entries are not counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryReply {
    pub live_local: u32,
    pub live_remote: u32,
    pub live_paired: u32,
}

impl InventoryReply {
    pub fn total(&self) -> u32 {
        self.live_local + self.live_remote + self.live_paired
    }
}

/// A failed reply. live is present only for COUNT_CHANGED.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlFailure {
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live: Option<u32>,
}

impl ControlFailure {
    pub fn code(code: &str) -> Self {
        Self {
            code: code.to_string(),
            live: None,
        }
    }
}

/// One control reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlReply {
    Hello(HelloReply),
    Inventory(InventoryReply),
    /// RequestUpgrade accepted: { ok: true, accepted: true }.
    UpgradeAccepted,
    /// RetireWhenIdle / Shutdown accepted: { ok: true, live }.
    Live { live: u32 },
    Failed(ControlFailure),
}

/// Errors decoding a control frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlWireError {
    /// The frame is not a JSON object.
    NotAnObject,
    /// The request carries a schema version this revision cannot read.
    UnsupportedVersion,
    /// A required field is missing or has the wrong type.
    BadField(String),
}

impl std::fmt::Display for ControlWireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ControlWireError::NotAnObject => write!(f, "control frame is not a JSON object"),
            ControlWireError::UnsupportedVersion => write!(f, "unsupported control schema version"),
            ControlWireError::BadField(detail) => write!(f, "bad control field: {detail}"),
        }
    }
}

impl std::error::Error for ControlWireError {}

impl ControlWireError {
    /// The reply a server sends for a frame it could not decode.
    pub fn reply(&self) -> ControlReply {
        let code = match self {
            ControlWireError::UnsupportedVersion => CODE_UNSUPPORTED_CONTROL_VERSION,
            ControlWireError::NotAnObject | ControlWireError::BadField(_) => CODE_BAD_REQUEST,
        };
        ControlReply::Failed(ControlFailure::code(code))
    }
}

pub(crate) fn parse_object(line: &str) -> Result<Map<String, Value>, ControlWireError> {
    match serde_json::from_str::<Value>(line.trim_end()) {
        Ok(Value::Object(map)) => Ok(map),
        _ => Err(ControlWireError::NotAnObject),
    }
}

fn from_map<T: for<'de> Deserialize<'de>>(map: Map<String, Value>) -> Result<T, ControlWireError> {
    serde_json::from_value(Value::Object(map)).map_err(|e| ControlWireError::BadField(e.to_string()))
}

/// Encodes a request as one JSON object (no trailing newline).
pub fn encode_request(request: &ControlRequest) -> Value {
    let mut map = Map::new();
    map.insert("control".into(), Value::from(CONTROL_SCHEMA_VERSION));
    map.insert("op".into(), Value::from(request.op()));
    match request {
        ControlRequest::RequestUpgrade {
            staged_executable,
            generation_key,
        } => {
            map.insert("staged_executable".into(), Value::from(staged_executable.clone()));
            map.insert(
                "generation_key".into(),
                generation_key.clone().map(Value::from).unwrap_or(Value::Null),
            );
        }
        ControlRequest::Shutdown {
            confirm_live_sessions,
        } => {
            map.insert(
                "confirm_live_sessions".into(),
                Value::from(*confirm_live_sessions),
            );
        }
        ControlRequest::Hello
        | ControlRequest::Inventory
        | ControlRequest::RetireWhenIdle
        | ControlRequest::Unsupported { .. } => {}
    }
    Value::Object(map)
}

/// Decodes a request object. Unknown fields are ignored; an unknown op decodes to Unsupported.
pub fn decode_request_value(map: Map<String, Value>) -> Result<ControlRequest, ControlWireError> {
    match map.get("control").and_then(Value::as_u64) {
        Some(CONTROL_SCHEMA_VERSION) => {}
        Some(_) => return Err(ControlWireError::UnsupportedVersion),
        None => return Err(ControlWireError::BadField("control".into())),
    }
    let op = map
        .get("op")
        .and_then(Value::as_str)
        .ok_or_else(|| ControlWireError::BadField("op".into()))?
        .to_string();
    Ok(match op.as_str() {
        OP_HELLO => ControlRequest::Hello,
        OP_INVENTORY => ControlRequest::Inventory,
        OP_REQUEST_UPGRADE => {
            let args: RequestUpgradeArgs = from_map(map)?;
            ControlRequest::RequestUpgrade {
                staged_executable: args.staged_executable,
                generation_key: args.generation_key,
            }
        }
        OP_RETIRE_WHEN_IDLE => ControlRequest::RetireWhenIdle,
        OP_SHUTDOWN => {
            let args: ShutdownArgs = from_map(map)?;
            ControlRequest::Shutdown {
                confirm_live_sessions: args.confirm_live_sessions,
            }
        }
        _ => ControlRequest::Unsupported { op },
    })
}

/// Decodes one request line.
pub fn decode_request(line: &str) -> Result<ControlRequest, ControlWireError> {
    decode_request_value(parse_object(line)?)
}

fn with_ok<T: Serialize>(ok: bool, body: &T) -> Value {
    let mut map = match serde_json::to_value(body) {
        Ok(Value::Object(map)) => map,
        _ => Map::new(),
    };
    map.insert("ok".into(), Value::Bool(ok));
    Value::Object(map)
}

/// Encodes a reply as one JSON object.
pub fn encode_reply(reply: &ControlReply) -> Value {
    match reply {
        ControlReply::Hello(hello) => with_ok(true, hello),
        ControlReply::Inventory(inventory) => with_ok(true, inventory),
        ControlReply::UpgradeAccepted => serde_json::json!({ "ok": true, "accepted": true }),
        ControlReply::Live { live } => serde_json::json!({ "ok": true, "live": live }),
        ControlReply::Failed(failure) => with_ok(false, failure),
    }
}

#[derive(Deserialize)]
struct LiveBody {
    live: u32,
}

/// Decodes a reply to op. Replies carry no op, so the caller passes the op it sent.
pub fn decode_reply(op: &str, line: &str) -> Result<ControlReply, ControlWireError> {
    let map = parse_object(line)?;
    let ok = map
        .get("ok")
        .and_then(Value::as_bool)
        .ok_or_else(|| ControlWireError::BadField("ok".into()))?;
    if !ok {
        return Ok(ControlReply::Failed(from_map(map)?));
    }
    Ok(match op {
        OP_HELLO => ControlReply::Hello(from_map(map)?),
        OP_INVENTORY => ControlReply::Inventory(from_map(map)?),
        OP_REQUEST_UPGRADE => ControlReply::UpgradeAccepted,
        OP_RETIRE_WHEN_IDLE | OP_SHUTDOWN => {
            let body: LiveBody = from_map(map)?;
            ControlReply::Live { live: body.live }
        }
        other => return Err(ControlWireError::BadField(format!("reply to unknown op {other}"))),
    })
}

/// Encodes a frame as one newline-terminated line.
pub fn to_line(value: &Value) -> String {
    let mut line = value.to_string();
    line.push('\n');
    line
}

/// The owner status classes the control ops decide on (mirrors HandoverStatus).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerStatus {
    Active,
    Retiring,
    Draining,
    Prepared,
    Retired,
}

impl OwnerStatus {
    fn retires_by_itself(self) -> bool {
        matches!(
            self,
            OwnerStatus::Draining | OwnerStatus::Prepared | OwnerStatus::Retired
        )
    }
}

/// Outcome of Shutdown { confirm_live_sessions }, decided under the status write lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownDecision {
    /// Refuse with this code and change nothing.
    Refuse(&'static str),
    /// The count moved: restore the prior status and answer COUNT_CHANGED with live.
    CountChanged { restore: OwnerStatus, live: u32 },
    /// Stay Retiring, write the retirement marker, shut down gracefully, exit 0.
    Proceed { live: u32 },
}

/// Decides Shutdown from the status seen under the status write lock. On CountChanged the caller
/// restores restore; on Proceed it keeps Retiring.
pub fn decide_shutdown(
    status: OwnerStatus,
    spawn_owners: usize,
    live: u32,
    confirm_live_sessions: u32,
) -> ShutdownDecision {
    if status.retires_by_itself() {
        return ShutdownDecision::Refuse(CODE_NOT_ACTIVE_OWNER);
    }
    if spawn_owners != 0 {
        return ShutdownDecision::Refuse(CODE_SPAWN_IN_FLIGHT);
    }
    if live != confirm_live_sessions {
        return ShutdownDecision::CountChanged {
            restore: status,
            live,
        };
    }
    ShutdownDecision::Proceed { live }
}

/// Outcome of RetireWhenIdle, decided under the status write lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetireDecision {
    /// Already retiring or retiring by itself: answer { ok, live } with no change.
    NoChange,
    /// Set status Retiring now, then re-check the retirement predicate.
    EnterRetiring,
}

pub fn decide_retire_when_idle(status: OwnerStatus) -> RetireDecision {
    match status {
        OwnerStatus::Active => RetireDecision::EnterRetiring,
        OwnerStatus::Retiring
        | OwnerStatus::Draining
        | OwnerStatus::Prepared
        | OwnerStatus::Retired => RetireDecision::NoChange,
    }
}

/// The retirement predicate for Retiring (and Draining): nothing live, nothing being spawned,
/// no request in flight.
pub fn retirement_ready(status: OwnerStatus, live: usize, spawn_owners: usize, in_flight: usize) -> bool {
    matches!(status, OwnerStatus::Retiring | OwnerStatus::Draining)
        && live == 0
        && spawn_owners == 0
        && in_flight == 0
}

/// Windows: the first frame of every connection must carry this boot's control token.
pub fn first_frame_token_matches(frame: &Map<String, Value>, expected: &str) -> bool {
    match frame.get(CONTROL_TOKEN_FIELD).and_then(Value::as_str) {
        Some(token) => {
            token.len() == expected.len()
                && token
                    .bytes()
                    .zip(expected.bytes())
                    .fold(0u8, |acc, (a, b)| acc | (a ^ b))
                    == 0
        }
        None => false,
    }
}
/// Client side. control::connect never falls back to the data protocol: no endpoint means a
/// pre-plan daemon (Missing).
pub enum ControlConnection {
    /// No control endpoint published: a daemon that predates control-v1, or none at all.
    Missing,
    Connected(ControlClient),
}

pub struct ControlClient {
    reader: tokio::io::BufReader<Box<dyn tokio::io::AsyncRead + Send + Unpin>>,
    writer: Box<dyn tokio::io::AsyncWrite + Send + Unpin>,
    /// Windows: this boot's control token, sent on the first frame only.
    token: Option<String>,
}

impl std::fmt::Debug for ControlClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ControlClient").finish_non_exhaustive()
    }
}

/// Connects to the control endpoint inside runtime_dir.
pub async fn connect(runtime_dir: &std::path::Path) -> std::io::Result<ControlConnection> {
    #[cfg(unix)]
    {
        let path = runtime_dir.join(CONTROL_SOCKET_FILE);
        match tokio::net::UnixStream::connect(&path).await {
            Ok(stream) => {
                let (read_half, write_half) = stream.into_split();
                let read_half: Box<dyn tokio::io::AsyncRead + Send + Unpin> = Box::new(read_half);
                Ok(ControlConnection::Connected(ControlClient {
                    reader: tokio::io::BufReader::new(read_half),
                    writer: Box::new(write_half),
                    token: None,
                }))
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
                ) =>
            {
                Ok(ControlConnection::Missing)
            }
            Err(error) => Err(error),
        }
    }
    #[cfg(not(unix))]
    {
        let port_path = runtime_dir.join(CONTROL_PORT_FILE);
        let token_path = runtime_dir.join(CONTROL_TOKEN_FILE);
        let (port, token) = match (
            tokio::fs::read_to_string(&port_path).await,
            tokio::fs::read_to_string(&token_path).await,
        ) {
            (Ok(port), Ok(token)) => (port, token),
            _ => return Ok(ControlConnection::Missing),
        };
        let Ok(port) = port.trim().parse::<u16>() else {
            return Ok(ControlConnection::Missing);
        };
        match tokio::net::TcpStream::connect(("127.0.0.1", port)).await {
            Ok(stream) => {
                let (read_half, write_half) = stream.into_split();
                let read_half: Box<dyn tokio::io::AsyncRead + Send + Unpin> = Box::new(read_half);
                Ok(ControlConnection::Connected(ControlClient {
                    reader: tokio::io::BufReader::new(read_half),
                    writer: Box::new(write_half),
                    token: Some(token.trim().to_string()),
                }))
            }
            Err(error) if error.kind() == std::io::ErrorKind::ConnectionRefused => {
                Ok(ControlConnection::Missing)
            }
            Err(error) => Err(error),
        }
    }
}

impl ControlClient {
    /// Sends one request and reads its one-line reply.
    pub async fn call(&mut self, request: &ControlRequest) -> std::io::Result<ControlReply> {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
        let mut frame = encode_request(request);
        if let (Some(token), Value::Object(map)) = (self.token.take(), &mut frame) {
            map.insert(CONTROL_TOKEN_FIELD.into(), Value::from(token));
        }
        self.writer.write_all(to_line(&frame).as_bytes()).await?;
        self.writer.flush().await?;
        let mut line = String::new();
        if self.reader.read_line(&mut line).await? == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "control connection closed before a reply",
            ));
        }
        decode_reply(request.op(), &line)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../../tests/fixtures/control-v1.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("control-v1 fixture is valid JSON")
    }

    #[test]
    fn control_v1_fixture_round_trips_value_for_value() {
        let fixture = fixture();
        let cases = fixture["cases"].as_array().expect("cases array");
        let mut ops_seen = Vec::new();
        for case in cases {
            let op = case["op"].as_str().expect("op");
            let request_value = case["request"].clone();
            let request = decode_request(&request_value.to_string()).expect("decode request");
            assert_eq!(request.op(), op);
            assert_eq!(encode_request(&request), request_value, "request {op}");
            for response_value in case["responses"].as_array().expect("responses") {
                let reply = decode_reply(op, &response_value.to_string()).expect("decode reply");
                assert_eq!(&encode_reply(&reply), response_value, "reply {op}");
            }
            ops_seen.push(op.to_string());
        }
        for op in FROZEN_OPS {
            assert!(ops_seen.iter().any(|seen| seen == op), "fixture covers {op}");
        }
    }

    #[test]
    fn control_v1_unknown_op_is_unsupported() {
        let fixture = fixture();
        let case = &fixture["unknown_op"];
        let request = decode_request(&case["request"].to_string()).expect("decode");
        assert_eq!(
            request,
            ControlRequest::Unsupported {
                op: "FutureOp".into()
            }
        );
        let reply = ControlReply::Failed(ControlFailure::code(CODE_UNSUPPORTED_OP));
        assert_eq!(encode_reply(&reply), case["response"]);
    }

    #[test]
    fn control_v1_unknown_fields_are_ignored() {
        let request = decode_request(
            r#"{"control":1,"op":"Shutdown","confirm_live_sessions":3,"added_later":true}"#,
        )
        .expect("decode");
        assert_eq!(
            request,
            ControlRequest::Shutdown {
                confirm_live_sessions: 3
            }
        );
        let reply = decode_reply(
            OP_INVENTORY,
            r#"{"ok":true,"live_local":1,"live_remote":2,"live_paired":3,"live_future":9}"#,
        )
        .expect("decode");
        assert_eq!(
            reply,
            ControlReply::Inventory(InventoryReply {
                live_local: 1,
                live_remote: 2,
                live_paired: 3
            })
        );
    }

    #[test]
    fn control_v1_rejects_other_schema_versions_and_garbage() {
        assert_eq!(
            decode_request(r#"{"control":2,"op":"Hello"}"#),
            Err(ControlWireError::UnsupportedVersion)
        );
        assert!(matches!(
            decode_request(r#"{"op":"Hello"}"#),
            Err(ControlWireError::BadField(_))
        ));
        assert_eq!(decode_request("[1]"), Err(ControlWireError::NotAnObject));
        assert_eq!(decode_request("not json"), Err(ControlWireError::NotAnObject));
        assert!(matches!(
            decode_request(r#"{"control":1,"op":"Shutdown"}"#),
            Err(ControlWireError::BadField(_))
        ));
    }

    #[test]
    fn shutdown_wrong_count_restores_prior_status() {
        assert_eq!(
            decide_shutdown(OwnerStatus::Active, 0, 2, 3),
            ShutdownDecision::CountChanged {
                restore: OwnerStatus::Active,
                live: 2
            }
        );
    }

    #[test]
    fn shutdown_stale_count_on_retiring_daemon_stays_retiring() {
        assert_eq!(
            decide_shutdown(OwnerStatus::Retiring, 0, 1, 0),
            ShutdownDecision::CountChanged {
                restore: OwnerStatus::Retiring,
                live: 1
            }
        );
    }

    #[test]
    fn shutdown_refuses_while_a_spawn_is_in_flight() {
        assert_eq!(
            decide_shutdown(OwnerStatus::Active, 1, 0, 0),
            ShutdownDecision::Refuse(CODE_SPAWN_IN_FLIGHT)
        );
    }

    #[test]
    fn shutdown_and_retire_on_draining_daemon_change_nothing() {
        for status in [OwnerStatus::Draining, OwnerStatus::Prepared, OwnerStatus::Retired] {
            assert_eq!(
                decide_shutdown(status, 0, 0, 0),
                ShutdownDecision::Refuse(CODE_NOT_ACTIVE_OWNER)
            );
            assert_eq!(decide_retire_when_idle(status), RetireDecision::NoChange);
        }
    }

    #[test]
    fn shutdown_with_matching_count_proceeds() {
        assert_eq!(
            decide_shutdown(OwnerStatus::Active, 0, 4, 4),
            ShutdownDecision::Proceed { live: 4 }
        );
    }

    #[test]
    fn retire_when_idle_enters_retiring_once() {
        assert_eq!(
            decide_retire_when_idle(OwnerStatus::Active),
            RetireDecision::EnterRetiring
        );
        assert_eq!(
            decide_retire_when_idle(OwnerStatus::Retiring),
            RetireDecision::NoChange
        );
    }

    #[test]
    fn retirement_waits_for_every_counter() {
        assert!(retirement_ready(OwnerStatus::Retiring, 0, 0, 0));
        assert!(retirement_ready(OwnerStatus::Draining, 0, 0, 0));
        assert!(!retirement_ready(OwnerStatus::Retiring, 1, 0, 0));
        assert!(!retirement_ready(OwnerStatus::Retiring, 0, 1, 0));
        assert!(!retirement_ready(OwnerStatus::Retiring, 0, 0, 1));
        assert!(!retirement_ready(OwnerStatus::Active, 0, 0, 0));
    }

    #[test]
    fn windows_wrong_token_is_rejected() {
        let good = parse_object(r#"{"control":1,"op":"Hello","token":"abc123"}"#).unwrap();
        let bad = parse_object(r#"{"control":1,"op":"Hello","token":"abc124"}"#).unwrap();
        let short = parse_object(r#"{"control":1,"op":"Hello","token":"abc"}"#).unwrap();
        let missing = parse_object(r#"{"control":1,"op":"Hello"}"#).unwrap();
        assert!(first_frame_token_matches(&good, "abc123"));
        assert!(!first_frame_token_matches(&bad, "abc123"));
        assert!(!first_frame_token_matches(&short, "abc123"));
        assert!(!first_frame_token_matches(&missing, "abc123"));
    }
}
