//! Agent-neutral control surface over every live PTY session the daemon owns.
//!
//! One contract serves every agent: list / read / send / answer / stop. Each session reports
//! where its state came from (`stateSource`) and how faithful the control is (`tier`), so a
//! supervisor can apply policy without knowing which agent runs in the pane. The daemon side
//! lives in `remote::server::agent_control_service` because it reuses the reference-chat screen,
//! prompt and ordered-input transactions; this module owns the wire types, the pure status
//! derivation, and the connect-only `ferryx-cli agents` client.
//!
//! Exit codes match the omon-gateway session-control contract: 0 ok, 2 invalid request,
//! 3 not found / not running / no daemon, 4 refused, 5 other or unknown outcome.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::remote::reference_chat::types::ReferencePromptAnswer;

/// Capability string the daemon advertises in `GetCapabilities`.
pub const AGENT_CONTROL_CAPABILITY: &str = "agentControlV1";

pub const EXIT_OK: i32 = 0;
pub const EXIT_INVALID: i32 = 2;
pub const EXIT_NOT_FOUND: i32 = 3;
pub const EXIT_REFUSED: i32 = 4;
pub const EXIT_OTHER: i32 = 5;

/// One control operation, carried inside `DaemonRequest::AgentControl`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum AgentControlOp {
    List,
    Read {
        session_id: String,
    },
    Send {
        session_id: String,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    Answer {
        session_id: String,
        prompt_id: String,
        screen_revision: String,
        answer: ReferencePromptAnswer,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    Stop {
        session_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
}

/// How faithful control of a session is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentTier {
    /// The agent reports its own state through the Ferryx extension.
    Native,
    /// Only the reconstructed screen is known.
    Screen,
}

/// The session's activity, in one vocabulary for every agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentControlState {
    Working,
    Blocked,
    Idle,
    Unknown,
}

/// Where the state was read from, so policy can trust a provider report over a screen guess.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentStateSource {
    Provider,
    Screen,
    None,
}

/// What the agent extension last reported for a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderReport {
    pub agent: Option<String>,
    pub state: String,
    /// The frame came from the agent itself, not from a daemon release or a manual reset.
    pub self_reported: bool,
    pub detail: Option<String>,
}

/// The derived status of one session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DerivedStatus {
    pub tier: AgentTier,
    pub state: AgentControlState,
    pub source: AgentStateSource,
}

fn provider_state(word: &str) -> Option<AgentControlState> {
    match word.trim().to_ascii_lowercase().as_str() {
        "working" => Some(AgentControlState::Working),
        "blocked" | "waiting" => Some(AgentControlState::Blocked),
        "idle" | "done" => Some(AgentControlState::Idle),
        _ => None,
    }
}

/// Combine the provider report, the screen detection and a detected pending prompt.
///
/// A self-reported provider state wins; a released or reset report says nothing about the agent
/// and falls through to the screen. A prompt the screen holds is always `blocked`: the pane is
/// waiting for an answer whatever the last report said.
pub fn derive_status(
    provider: Option<&ProviderReport>,
    screen: Option<crate::agent_detect::AgentActivity>,
    pending_prompt: bool,
) -> DerivedStatus {
    use crate::agent_detect::AgentActivity;
    let native = provider.filter(|report| report.self_reported);
    if let Some(state) = native.and_then(|report| provider_state(&report.state)) {
        let state = if pending_prompt { AgentControlState::Blocked } else { state };
        return DerivedStatus { tier: AgentTier::Native, state, source: AgentStateSource::Provider };
    }
    let tier = if native.is_some() { AgentTier::Native } else { AgentTier::Screen };
    if pending_prompt {
        return DerivedStatus { tier, state: AgentControlState::Blocked, source: AgentStateSource::Screen };
    }
    match screen {
        Some(activity) => DerivedStatus {
            tier,
            state: match activity {
                AgentActivity::Working => AgentControlState::Working,
                AgentActivity::Blocked => AgentControlState::Blocked,
                AgentActivity::Idle => AgentControlState::Idle,
            },
            source: AgentStateSource::Screen,
        },
        None => DerivedStatus { tier, state: AgentControlState::Unknown, source: AgentStateSource::None },
    }
}

/// The exit code a daemon error code maps to.
pub fn exit_code_for(code: &str) -> i32 {
    match code {
        "INVALID_REQUEST" | "PAYLOAD_TOO_LARGE" => EXIT_INVALID,
        "NOT_FOUND" | "TARGET_EXPIRED" => EXIT_NOT_FOUND,
        "UNAUTHORIZED" | "FORBIDDEN" | "CONTROL_CONFLICT" | "REQUEST_CONFLICT" | "PROVIDER_OWNED"
        | "UNSUPPORTED" | "CAPTURE_UNSUPPORTED" => EXIT_REFUSED,
        _ => EXIT_OTHER,
    }
}

pub const AGENTS_USAGE: &str = "usage: ferryx-cli agents list [--json]\n       ferryx-cli agents read <session> [--json]\n       ferryx-cli agents send <session> <text> [--request-id <id>] [--json]\n       ferryx-cli agents answer <session> --prompt <id> --revision <rev> (--option <n> | --options <n,m> | --text <t>) [--request-id <id>] [--json]\n       ferryx-cli agents stop <session> [--request-id <id>] [--json]";

/// A CLI usage error: always exit 2, never contacts the daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageError(pub String);

fn take_flag(args: &mut Vec<String>, name: &str) -> Result<Option<String>, UsageError> {
    let Some(index) = args.iter().position(|arg| arg == name) else {
        return Ok(None);
    };
    if index + 1 >= args.len() {
        return Err(UsageError(format!("{name} needs a value")));
    }
    let value = args.remove(index + 1);
    args.remove(index);
    Ok(Some(value))
}

fn session_arg(args: &[String]) -> Result<String, UsageError> {
    let session = args.first().ok_or_else(|| UsageError("a session id is required".into()))?;
    if !crate::agent_transcript::is_valid_session_id(session) {
        return Err(UsageError(format!("not a session id: {session}")));
    }
    Ok(session.clone())
}

/// Parse `ferryx-cli agents ...` (`args[0]` is the binary, `args[1]` is `agents`).
pub fn parse_agents_cli(args: &[String]) -> Result<AgentControlOp, UsageError> {
    if args.get(1).map(String::as_str) != Some("agents") {
        return Err(UsageError(AGENTS_USAGE.into()));
    }
    let command = args.get(2).cloned().ok_or_else(|| UsageError(AGENTS_USAGE.into()))?;
    // JSON is the only output format; --json is accepted so callers can state it explicitly.
    let mut rest: Vec<String> = args.iter().skip(3).filter(|arg| *arg != "--json").cloned().collect();
    let request_id = take_flag(&mut rest, "--request-id")?;
    let op = match command.as_str() {
        "list" => AgentControlOp::List,
        "read" => AgentControlOp::Read { session_id: session_arg(&rest)? },
        "send" => {
            let session_id = session_arg(&rest)?;
            let text = rest.get(1..).map(|words| words.join(" ")).unwrap_or_default();
            if text.trim().is_empty() || text.contains('\0') {
                return Err(UsageError("text must not be empty or contain NUL".into()));
            }
            AgentControlOp::Send { session_id, text, request_id: request_id.clone() }
        }
        "answer" => {
            let prompt_id = take_flag(&mut rest, "--prompt")?.ok_or_else(|| UsageError("--prompt is required".into()))?;
            let screen_revision = take_flag(&mut rest, "--revision")?.ok_or_else(|| UsageError("--revision is required".into()))?;
            let option = take_flag(&mut rest, "--option")?;
            let options = take_flag(&mut rest, "--options")?;
            let text = take_flag(&mut rest, "--text")?;
            let answer = match (option, options, text) {
                (Some(index), None, None) => ReferencePromptAnswer {
                    option_index: Some(index.parse().map_err(|_| UsageError("--option must be a number".into()))?),
                    ..Default::default()
                },
                (None, Some(list), None) => ReferencePromptAnswer {
                    option_indices: Some(
                        list.split(',')
                            .map(|item| item.trim().parse::<u32>())
                            .collect::<Result<Vec<_>, _>>()
                            .map_err(|_| UsageError("--options must be comma-separated numbers".into()))?,
                    ),
                    ..Default::default()
                },
                (None, None, Some(text)) if !text.trim().is_empty() => {
                    ReferencePromptAnswer { custom_text: Some(text), ..Default::default() }
                }
                _ => return Err(UsageError("give exactly one of --option, --options or a non-empty --text".into())),
            };
            AgentControlOp::Answer { session_id: session_arg(&rest)?, prompt_id, screen_revision, answer, request_id: request_id.clone() }
        }
        "stop" => AgentControlOp::Stop { session_id: session_arg(&rest)?, request_id: request_id.clone() },
        _ => return Err(UsageError(AGENTS_USAGE.into())),
    };
    if matches!(op, AgentControlOp::List | AgentControlOp::Read { .. }) && request_id.is_some() {
        return Err(UsageError("--request-id applies to send, answer and stop only".into()));
    }
    Ok(op)
}

/// The JSON a failed command prints, and its exit code.
pub fn failure(code: &str, message: &str) -> (Value, i32) {
    (serde_json::json!({ "ok": false, "code": code, "message": message }), exit_code_for(code))
}

#[cfg(unix)]
async fn connect_daemon() -> Result<tokio::net::UnixStream, (Value, i32)> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    let path = crate::daemon::server::get_socket_path();
    let not_running = || {
        let (value, _) = failure("NOT_FOUND", &format!("no Ferryx daemon is listening at {}", path.display()));
        (value, EXIT_NOT_FOUND)
    };
    let metadata = std::fs::symlink_metadata(&path).map_err(|_| not_running())?;
    // SAFETY: getuid takes no arguments, dereferences nothing and cannot fail.
    let uid = unsafe { libc::getuid() };
    if !metadata.file_type().is_socket() || metadata.uid() != uid {
        return Err(failure("FORBIDDEN", "the daemon socket is not a socket owned by this user"));
    }
    tokio::net::UnixStream::connect(&path).await.map_err(|_| not_running())
}

#[cfg(not(unix))]
async fn connect_daemon() -> Result<tokio::net::TcpStream, (Value, i32)> {
    let path = crate::daemon::server::get_socket_path();
    let not_running = || {
        let (value, _) = failure("NOT_FOUND", "no Ferryx daemon is listening");
        (value, EXIT_NOT_FOUND)
    };
    let port: u16 = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| text.trim().parse().ok())
        .ok_or_else(not_running)?;
    tokio::net::TcpStream::connect(("127.0.0.1", port)).await.map_err(|_| not_running())
}

/// Send one operation to the running daemon. Connect-only: never spawns, upgrades or retries.
pub async fn request_daemon(op: AgentControlOp) -> (Value, i32) {
    use crate::daemon::protocol::{DaemonRequest, DaemonResponse, DAEMON_PROTOCOL_VERSION};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let stream = match connect_daemon().await {
        Ok(stream) => stream,
        Err(failed) => return failed,
    };
    let (read_half, mut write_half) = tokio::io::split(stream);
    let mut reader = BufReader::new(read_half);
    async fn exchange<R, W>(reader: &mut BufReader<R>, writer: &mut W, request: &DaemonRequest) -> Result<DaemonResponse, String>
    where
        R: tokio::io::AsyncRead + Unpin,
        W: tokio::io::AsyncWrite + Unpin,
    {
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
    let handshake = DaemonRequest::Handshake {
        version: DAEMON_PROTOCOL_VERSION,
        token: crate::daemon::client::read_transport_token(),
    };
    match exchange(&mut reader, &mut write_half, &handshake).await {
        Ok(DaemonResponse::HandshakeOk { .. }) => {}
        Ok(DaemonResponse::ProtocolMismatch { .. }) => return failure("UNSUPPORTED", "the daemon speaks another protocol version"),
        Ok(other) => return failure("UNSUPPORTED", &format!("unexpected handshake response: {other:?}")),
        Err(message) => return failure("INTERNAL", &message),
    }
    match exchange(&mut reader, &mut write_half, &DaemonRequest::AgentControl { request: op }).await {
        Ok(DaemonResponse::AgentControlOk { result }) => {
            let mut value = serde_json::json!({ "ok": true });
            if let (Some(out), Value::Object(fields)) = (value.as_object_mut(), result) {
                out.extend(fields);
            }
            (value, EXIT_OK)
        }
        Ok(DaemonResponse::Error { message, code, .. }) => {
            let code = code.unwrap_or_default();
            // An older daemon answers an unknown request type as malformed: that is a refusal.
            if code.is_empty() && message.starts_with("Malformed request") {
                return failure("UNSUPPORTED", "this daemon does not support agent control (agentControlV1)");
            }
            failure(if code.is_empty() { "INTERNAL" } else { &code }, &message)
        }
        Ok(other) => failure("INTERNAL", &format!("unexpected daemon response: {other:?}")),
        Err(message) => failure("OPERATION_OUTCOME_UNKNOWN", &message),
    }
}

/// Run `ferryx-cli agents ...`: print one JSON value on stdout and return the exit code.
pub fn run_agents_cli(args: &[String]) -> i32 {
    let (value, code) = match parse_agents_cli(args) {
        Err(UsageError(message)) => {
            let (value, _) = failure("INVALID_REQUEST", &message);
            (value, EXIT_INVALID)
        }
        Ok(op) => match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(runtime) => runtime.block_on(async {
                match tokio::time::timeout(std::time::Duration::from_secs(60), request_daemon(op)).await {
                    Ok(result) => result,
                    Err(_) => failure("TIMEOUT", "the daemon did not answer within 60s; the outcome is unknown"),
                }
            }),
            Err(error) => failure("INTERNAL", &error.to_string()),
        },
    };
    println!("{value}");
    code
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_detect::AgentActivity;

    fn argv(words: &[&str]) -> Vec<String> {
        std::iter::once("ferryx-cli").chain(words.iter().copied()).map(String::from).collect()
    }

    fn report(state: &str, self_reported: bool) -> ProviderReport {
        ProviderReport { agent: Some("omo".into()), state: state.into(), self_reported, detail: None }
    }

    #[test]
    fn the_request_round_trips_on_the_daemon_wire() {
        let request = crate::daemon::protocol::DaemonRequest::AgentControl {
            request: AgentControlOp::Send { session_id: "s-1".into(), text: "hi".into(), request_id: Some("r".into()) },
        };
        let json = serde_json::to_string(&request).unwrap();
        assert_eq!(json, r#"{"type":"agentControl","request":{"op":"send","sessionId":"s-1","text":"hi","requestId":"r"}}"#);
        let back: crate::daemon::protocol::DaemonRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, crate::daemon::protocol::DaemonRequest::AgentControl { request } if request == AgentControlOp::Send { session_id: "s-1".into(), text: "hi".into(), request_id: Some("r".into()) }));
    }

    #[test]
    fn error_codes_map_to_the_gateway_exit_codes() {
        assert_eq!(exit_code_for("INVALID_REQUEST"), 2);
        assert_eq!(exit_code_for("PAYLOAD_TOO_LARGE"), 2);
        assert_eq!(exit_code_for("NOT_FOUND"), 3);
        assert_eq!(exit_code_for("TARGET_EXPIRED"), 3);
        assert_eq!(exit_code_for("REQUEST_CONFLICT"), 4);
        assert_eq!(exit_code_for("UNSUPPORTED"), 4);
        assert_eq!(exit_code_for("OPERATION_OUTCOME_UNKNOWN"), 5);
        assert_eq!(exit_code_for("TIMEOUT"), 5);
    }

    #[test]
    fn the_cli_refuses_bad_input_before_contacting_a_daemon() {
        assert!(parse_agents_cli(&argv(&["agents", "send", "s-1", "  "])).is_err());
        assert!(parse_agents_cli(&argv(&["agents", "send", "s-1"])).is_err());
        assert!(parse_agents_cli(&argv(&["agents", "read"])).is_err());
        assert!(parse_agents_cli(&argv(&["agents", "read", "../x"])).is_err());
        assert!(parse_agents_cli(&argv(&["agents", "frobnicate"])).is_err());
        assert!(parse_agents_cli(&argv(&["agents", "list", "--request-id", "x"])).is_err());
        assert!(parse_agents_cli(&argv(&["agents", "answer", "s-1", "--prompt", "p", "--revision", "r"])).is_err());
        assert!(parse_agents_cli(&argv(&["agents", "answer", "s-1", "--prompt", "p", "--revision", "r", "--option", "1", "--text", "x"])).is_err());
    }

    #[test]
    fn the_cli_parses_every_operation() {
        assert_eq!(parse_agents_cli(&argv(&["agents", "list", "--json"])).unwrap(), AgentControlOp::List);
        assert_eq!(
            parse_agents_cli(&argv(&["agents", "send", "s-1", "echo", "hi", "--request-id", "r1"])).unwrap(),
            AgentControlOp::Send { session_id: "s-1".into(), text: "echo hi".into(), request_id: Some("r1".into()) }
        );
        assert_eq!(
            parse_agents_cli(&argv(&["agents", "answer", "s-1", "--prompt", "p", "--revision", "r", "--options", "0,2"])).unwrap(),
            AgentControlOp::Answer {
                session_id: "s-1".into(),
                prompt_id: "p".into(),
                screen_revision: "r".into(),
                answer: ReferencePromptAnswer { option_indices: Some(vec![0, 2]), ..Default::default() },
                request_id: None,
            }
        );
        assert_eq!(parse_agents_cli(&argv(&["agents", "stop", "s-1"])).unwrap(), AgentControlOp::Stop { session_id: "s-1".into(), request_id: None });
    }

    #[test]
    fn a_self_reported_provider_state_wins_over_the_screen() {
        let status = derive_status(Some(&report("working", true)), Some(AgentActivity::Idle), false);
        assert_eq!(status, DerivedStatus { tier: AgentTier::Native, state: AgentControlState::Working, source: AgentStateSource::Provider });
        let waiting = derive_status(Some(&report("waiting", true)), None, false);
        assert_eq!(waiting.state, AgentControlState::Blocked);
    }

    #[test]
    fn a_released_report_falls_back_to_the_screen() {
        let status = derive_status(Some(&report("idle", false)), Some(AgentActivity::Working), false);
        assert_eq!(status, DerivedStatus { tier: AgentTier::Screen, state: AgentControlState::Working, source: AgentStateSource::Screen });
    }

    #[test]
    fn a_pending_prompt_is_always_blocked() {
        assert_eq!(derive_status(Some(&report("working", true)), None, true).state, AgentControlState::Blocked);
        let screen_only = derive_status(None, Some(AgentActivity::Idle), true);
        assert_eq!((screen_only.state, screen_only.source), (AgentControlState::Blocked, AgentStateSource::Screen));
    }

    #[test]
    fn nothing_known_is_unknown_not_idle() {
        assert_eq!(
            derive_status(None, None, false),
            DerivedStatus { tier: AgentTier::Screen, state: AgentControlState::Unknown, source: AgentStateSource::None }
        );
    }
}
