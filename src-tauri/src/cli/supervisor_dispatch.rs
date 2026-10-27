//! `ferryx-cli supervisor submit ...` and `ferryx-cli supervisor dispatch status ...`.
//!
//! `submit` admits one owner request through the running daemon's hub as the installation
//! owner (adapter `cli`). With `--session` the goal is bound at admission to that existing pane
//! on `--host` (a paired host's pane is then delivered through the paired path), so no pane is
//! launched for it. The pane choice exists only here, on the owner socket: the model's work
//! has no such field, a worker inside a Ferryx pane (FERRYX_SESSION_ID set) is refused, and a
//! pane another open goal holds is SESSION_ALREADY_BOUND (exit 4). A lost answer is
//! OPERATION_OUTCOME_UNKNOWN; re-running with the same message id replays the admission.
//!
//! `dispatch status` reads one goal's run state, binding and dispatches with their receipts.
//! It writes nothing.
//!
//! Both are connect-only like `ferryx-cli supervisor goal`, gated on `supervisorDispatchV1`,
//! and print one JSON value.

use serde_json::Value;

use crate::agent_control::{failure, UsageError, EXIT_INVALID, EXIT_OK, EXIT_REFUSED};
use crate::daemon::protocol::{DaemonRequest, DaemonResponse, SupervisorDispatchAction, DAEMON_PROTOCOL_VERSION};
use ferryx_hub::supervisor::admission::SubmitWork;

pub const DISPATCH_USAGE: &str = "usage: ferryx-cli supervisor submit --request-file <json> [--host <host-id>] [--session <session-id>] [--json]\n       ferryx-cli supervisor dispatch status --goal <goal-id> [--json]\n  request file: {\"messageId\": <dedupe id>, \"body\": <the owner's request>, \"work\": <submit_work arguments>}";

/// A submit may wait on the daemon's write lock; a status read is quick.
const DISPATCH_CLI_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// The owner's request file of `supervisor submit`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RequestFile {
    pub message_id: String,
    pub body: String,
    pub work: SubmitWork,
}

/// What `ferryx-cli supervisor submit|dispatch ...` asks for, before the request file is read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchCli {
    Submit { request_file: String, host_id: Option<String>, session_id: Option<String> },
    Status { goal_id: String },
}

fn take_value(rest: &mut Vec<String>, name: &str) -> Result<Option<String>, UsageError> {
    let Some(index) = rest.iter().position(|arg| arg == name) else {
        return Ok(None);
    };
    let value = rest.get(index + 1).cloned().filter(|value| !value.trim().is_empty() && !value.starts_with("--"));
    let Some(value) = value else {
        return Err(UsageError(format!("{name} needs a value\n{DISPATCH_USAGE}")));
    };
    rest.drain(index..=index + 1);
    if rest.iter().any(|arg| arg == name) {
        return Err(UsageError(format!("{name} may be given once\n{DISPATCH_USAGE}")));
    }
    Ok(Some(value))
}

/// Parse `ferryx-cli supervisor submit ...` or `ferryx-cli supervisor dispatch status ...`.
pub fn parse_dispatch_cli(args: &[String]) -> Result<DispatchCli, UsageError> {
    if args.get(1).map(String::as_str) != Some("supervisor") {
        return Err(UsageError(DISPATCH_USAGE.into()));
    }
    let (command, skip) = match (args.get(2).map(String::as_str), args.get(3).map(String::as_str)) {
        (Some("submit"), _) => ("submit", 3),
        (Some("dispatch"), Some("status")) => ("status", 4),
        _ => return Err(UsageError(DISPATCH_USAGE.into())),
    };
    let mut rest: Vec<String> = args.iter().skip(skip).filter(|arg| *arg != "--json").cloned().collect();
    let parsed = if command == "submit" {
        let request_file = take_value(&mut rest, "--request-file")?
            .ok_or_else(|| UsageError(format!("--request-file is required\n{DISPATCH_USAGE}")))?;
        let host_id = take_value(&mut rest, "--host")?;
        let session_id = take_value(&mut rest, "--session")?;
        if session_id.is_some() && host_id.is_none() {
            return Err(UsageError(format!("--session needs the --host the pane lives on\n{DISPATCH_USAGE}")));
        }
        DispatchCli::Submit { request_file, host_id, session_id }
    } else {
        let goal_id = take_value(&mut rest, "--goal")?
            .ok_or_else(|| UsageError(format!("--goal is required\n{DISPATCH_USAGE}")))?;
        DispatchCli::Status { goal_id }
    };
    if let Some(extra) = rest.first() {
        return Err(UsageError(format!("unexpected argument {extra}\n{DISPATCH_USAGE}")));
    }
    Ok(parsed)
}

/// The daemon request a parsed command sends; `submit` reads and checks its request file.
pub fn dispatch_action(command: DispatchCli) -> Result<SupervisorDispatchAction, UsageError> {
    match command {
        DispatchCli::Status { goal_id } => Ok(SupervisorDispatchAction::Status { goal_id }),
        DispatchCli::Submit { request_file, host_id, session_id } => {
            let text = std::fs::read_to_string(&request_file)
                .map_err(|error| UsageError(format!("cannot read {request_file}: {error}")))?;
            let file: RequestFile = serde_json::from_str(&text)
                .map_err(|error| UsageError(format!("{request_file} is not a request file: {error}\n{DISPATCH_USAGE}")))?;
            if file.message_id.trim().is_empty() || file.body.trim().is_empty() {
                return Err(UsageError(format!("{request_file} needs a messageId and a body")));
            }
            Ok(SupervisorDispatchAction::Submit {
                message_id: file.message_id,
                body: file.body,
                host_id,
                session_id,
                work: file.work,
            })
        }
    }
}

/// The JSON and exit code a daemon reply maps to. SESSION_ALREADY_BOUND is a refusal (4).
pub fn dispatch_outcome_of(response: DaemonResponse) -> (Value, i32) {
    match response {
        DaemonResponse::SupervisorDispatchOk { result } => {
            let mut value = serde_json::json!({ "ok": true });
            if let (Some(out), Value::Object(fields)) = (value.as_object_mut(), result) {
                out.extend(fields);
            }
            (value, EXIT_OK)
        }
        DaemonResponse::Error { message, code, .. } => {
            let code = code.filter(|code| !code.is_empty()).unwrap_or_else(|| "INTERNAL".into());
            let (value, exit) = failure(&code, &message);
            let exit = if code == ferryx_hub::supervisor::SESSION_ALREADY_BOUND { EXIT_REFUSED } else { exit };
            (value, exit)
        }
        other => failure("INTERNAL", &format!("unexpected daemon response: {other:?}")),
    }
}

/// Send one dispatch action to the running daemon, gated on `supervisorDispatchV1`. A lost
/// answer to a submit is OPERATION_OUTCOME_UNKNOWN and is not sent again.
pub async fn request_dispatch(action: SupervisorDispatchAction) -> (Value, i32) {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let stream = match super::supervisor_goal::connect_daemon().await {
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
    let required = crate::hub_runtime::SUPERVISOR_DISPATCH_CAPABILITY;
    match exchange(&mut reader, &mut write_half, &DaemonRequest::GetCapabilities).await {
        Ok(DaemonResponse::CapabilitiesOk { capabilities }) if capabilities.iter().any(|known| known == required) => {}
        Ok(DaemonResponse::CapabilitiesOk { .. }) => {
            return failure("UNSUPPORTED", &format!("this daemon does not advertise {required}"))
        }
        Ok(other) => return failure("INTERNAL", &format!("unexpected capabilities response: {other:?}")),
        Err(message) => return failure("INTERNAL", &message),
    }
    let mutates = action.mutates();
    match exchange(&mut reader, &mut write_half, &DaemonRequest::SupervisorDispatch { request: action }).await {
        Ok(response) => dispatch_outcome_of(response),
        Err(message) if mutates => failure("OPERATION_OUTCOME_UNKNOWN", &message),
        Err(message) => failure("INTERNAL", &message),
    }
}

/// Run `ferryx-cli supervisor submit|dispatch ...`: print one JSON value and return the exit code.
pub fn run_dispatch_cli(args: &[String]) -> i32 {
    let in_pane = std::env::var_os("FERRYX_SESSION_ID").is_some_and(|session| !session.is_empty());
    let parsed = parse_dispatch_cli(args);
    let (value, code) = match parsed.and_then(|command| {
        let submits = matches!(command, DispatchCli::Submit { .. });
        dispatch_action(command).map(|action| (action, submits))
    }) {
        Ok((_, true)) if in_pane => {
            let (value, _) = failure("FORBIDDEN", "supervisor submit is the owner's act and is refused inside a Ferryx pane (FERRYX_SESSION_ID is set)");
            (value, EXIT_REFUSED)
        }
        Err(UsageError(message)) => {
            let (value, _) = failure("INVALID_REQUEST", &message);
            (value, EXIT_INVALID)
        }
        Ok((action, _)) => {
            let mutates = action.mutates();
            match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(runtime) => runtime.block_on(async {
                    match tokio::time::timeout(DISPATCH_CLI_TIMEOUT, request_dispatch(action)).await {
                        Ok(result) => result,
                        Err(_) if mutates => failure(
                            "OPERATION_OUTCOME_UNKNOWN",
                            "the daemon did not answer before the wait budget elapsed; re-run with the same messageId to learn the outcome",
                        ),
                        Err(_) => failure("TIMEOUT", "the daemon did not answer before the wait budget elapsed"),
                    }
                }),
                Err(error) => failure("INTERNAL", &error.to_string()),
            }
        }
    };
    println!("{value}");
    code
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::EXIT_NOT_FOUND;

    fn argv(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| (*word).to_owned()).collect()
    }

    #[test]
    fn submit_and_status_parse_their_flags() {
        assert_eq!(
            parse_dispatch_cli(&argv(&["ferryx-cli", "supervisor", "submit", "--host", "h", "--session", "s", "--request-file", "/r.json", "--json"])),
            Ok(DispatchCli::Submit { request_file: "/r.json".into(), host_id: Some("h".into()), session_id: Some("s".into()) })
        );
        assert_eq!(
            parse_dispatch_cli(&argv(&["ferryx-cli", "supervisor", "dispatch", "status", "--goal", "g-1", "--json"])),
            Ok(DispatchCli::Status { goal_id: "g-1".into() })
        );
        for refused in [
            &["ferryx-cli", "supervisor", "submit", "--session", "s", "--request-file", "/r.json"][..],
            &["ferryx-cli", "supervisor", "submit", "--host", "h"][..],
            &["ferryx-cli", "supervisor", "submit", "--request-file", "/r.json", "--request-file", "/s.json"][..],
            &["ferryx-cli", "supervisor", "dispatch", "status"][..],
            &["ferryx-cli", "supervisor", "dispatch", "status", "--goal", "g", "extra"][..],
            &["ferryx-cli", "supervisor", "dispatch", "show", "--goal", "g"][..],
        ] {
            assert!(parse_dispatch_cli(&argv(refused)).is_err(), "{refused:?}");
        }
    }

    #[test]
    fn a_request_file_carries_the_owner_body_and_never_a_model_pane() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("request.json");
        let work = serde_json::json!({"title": "t", "instruction": "i", "requirements": []});
        std::fs::write(&path, serde_json::json!({"messageId": "m-1", "body": "Do it.", "work": work}).to_string()).unwrap();
        let command = DispatchCli::Submit {
            request_file: path.display().to_string(),
            host_id: Some("h".into()),
            session_id: Some("s".into()),
        };
        match dispatch_action(command.clone()).unwrap() {
            SupervisorDispatchAction::Submit { message_id, body, host_id, session_id, .. } => {
                assert_eq!((message_id.as_str(), body.as_str()), ("m-1", "Do it."));
                assert_eq!((host_id.as_deref(), session_id.as_deref()), (Some("h"), Some("s")));
            }
            other => panic!("{other:?}"),
        }
        let smuggled = serde_json::json!({"title": "t", "instruction": "i", "requirements": [], "sessionId": "s"});
        std::fs::write(&path, serde_json::json!({"messageId": "m-1", "body": "b", "work": smuggled}).to_string()).unwrap();
        assert!(dispatch_action(command).is_err(), "a pane inside the model's work is refused");
    }

    #[test]
    fn dispatch_replies_map_to_exit_codes() {
        let (value, code) = dispatch_outcome_of(DaemonResponse::SupervisorDispatchOk {
            result: serde_json::json!({"outcome": "admitted"}),
        });
        assert_eq!((value["ok"].as_bool(), value["outcome"].as_str(), code), (Some(true), Some("admitted"), EXIT_OK));
        let refusal = |code: &str| DaemonResponse::Error { message: "m".into(), code: Some(code.into()), details: None };
        assert_eq!(dispatch_outcome_of(refusal(ferryx_hub::supervisor::SESSION_ALREADY_BOUND)).1, EXIT_REFUSED);
        assert_eq!(dispatch_outcome_of(refusal("UNAUTHORIZED")).1, EXIT_REFUSED);
        assert_eq!(dispatch_outcome_of(refusal("NOT_FOUND")).1, EXIT_NOT_FOUND);
        assert_eq!(dispatch_outcome_of(refusal("INVALID_REQUEST")).1, EXIT_INVALID);
    }
}
