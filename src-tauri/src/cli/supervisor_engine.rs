//! `ferryx-cli supervisor engine status|assess`: read the running daemon's retained mhc
//! supervisor engine, or ask it for one structured judgment. Connect-only like
//! `ferryx-cli supervisor goal`: never spawns, upgrades or retries, and prints one JSON value.
//! Neither command writes goals or dispatches, nor types into a pane; `actions` in the reply
//! lists the supervisor's recent pane input so a caller can check that.

use serde_json::Value;

use crate::agent_control::{failure, UsageError, EXIT_INVALID, EXIT_OK};
use crate::daemon::protocol::{DaemonRequest, DaemonResponse, SupervisorEngineAction, DAEMON_PROTOCOL_VERSION};

pub const SUPERVISOR_ENGINE_USAGE: &str = "usage: ferryx-cli supervisor engine status [--json]\n       ferryx-cli supervisor engine assess --prompt <text> [--json]";

/// How long one engine action may take: an assessment is one bounded engine turn (600s by
/// default) behind a possible engine launch.
const ENGINE_CLI_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(660);

/// Parse `ferryx-cli supervisor engine ...` (`args[0]` is the binary).
pub fn parse_supervisor_engine_cli(args: &[String]) -> Result<SupervisorEngineAction, UsageError> {
    if args.get(1).map(String::as_str) != Some("supervisor") || args.get(2).map(String::as_str) != Some("engine") {
        return Err(UsageError(SUPERVISOR_ENGINE_USAGE.into()));
    }
    let command = args.get(3).ok_or_else(|| UsageError(SUPERVISOR_ENGINE_USAGE.into()))?;
    // JSON is the only output format; --json is accepted so callers can state it explicitly.
    let rest: Vec<&String> = args.iter().skip(4).filter(|arg| *arg != "--json").collect();
    match command.as_str() {
        "status" if rest.is_empty() => Ok(SupervisorEngineAction::Status),
        "assess" => match rest.as_slice() {
            [flag, prompt] if flag.as_str() == "--prompt" && !prompt.trim().is_empty() => {
                Ok(SupervisorEngineAction::Assess { prompt: (*prompt).clone() })
            }
            _ => Err(UsageError(format!("assess needs exactly --prompt <text>\n{SUPERVISOR_ENGINE_USAGE}"))),
        },
        "status" => Err(UsageError(format!("status takes no arguments\n{SUPERVISOR_ENGINE_USAGE}"))),
        other => Err(UsageError(format!("unknown engine command {other}\n{SUPERVISOR_ENGINE_USAGE}"))),
    }
}

/// The JSON and exit code a daemon reply maps to. An engine fault keeps its `MHC_*` code.
pub fn engine_outcome_of(response: DaemonResponse) -> (Value, i32) {
    match response {
        DaemonResponse::SupervisorEngineOk { snapshot } => {
            let mut value = serde_json::json!({ "ok": true });
            if let (Some(out), Value::Object(fields)) = (value.as_object_mut(), snapshot) {
                out.extend(fields);
            }
            (value, EXIT_OK)
        }
        DaemonResponse::Error { message, code, .. } => {
            let code = code.unwrap_or_default();
            failure(if code.is_empty() { "INTERNAL" } else { &code }, &message)
        }
        other => failure("INTERNAL", &format!("unexpected daemon response: {other:?}")),
    }
}

/// Send one engine action to the running daemon, gated on `supervisorEngineV1` so an older
/// daemon never receives a request it cannot parse.
pub async fn request_engine(action: SupervisorEngineAction) -> (Value, i32) {
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
    let required = crate::hub_runtime::SUPERVISOR_ENGINE_CAPABILITY;
    match exchange(&mut reader, &mut write_half, &DaemonRequest::GetCapabilities).await {
        Ok(DaemonResponse::CapabilitiesOk { capabilities }) if capabilities.iter().any(|known| known == required) => {}
        Ok(DaemonResponse::CapabilitiesOk { .. }) => {
            return failure("UNSUPPORTED", &format!("this daemon does not advertise {required}"))
        }
        Ok(other) => return failure("INTERNAL", &format!("unexpected capabilities response: {other:?}")),
        Err(message) => return failure("INTERNAL", &message),
    }
    match exchange(&mut reader, &mut write_half, &DaemonRequest::SupervisorEngine { request: action }).await {
        Ok(response) => engine_outcome_of(response),
        Err(message) => failure("INTERNAL", &message),
    }
}

/// Run `ferryx-cli supervisor engine ...`: print one JSON value on stdout and return the exit code.
pub fn run_supervisor_engine_cli(args: &[String]) -> i32 {
    let (value, code) = match parse_supervisor_engine_cli(args) {
        Err(UsageError(message)) => {
            let (value, _) = failure("INVALID_REQUEST", &message);
            (value, EXIT_INVALID)
        }
        Ok(action) => match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(runtime) => runtime.block_on(async {
                match tokio::time::timeout(ENGINE_CLI_TIMEOUT, request_engine(action)).await {
                    Ok(result) => result,
                    Err(_) => failure("TIMEOUT", "the daemon did not answer before the wait budget elapsed"),
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
    use crate::agent_control::{EXIT_NOT_FOUND, EXIT_OTHER};

    fn argv(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| (*word).to_owned()).collect()
    }

    #[test]
    fn the_cli_parses_every_engine_command() {
        assert_eq!(
            parse_supervisor_engine_cli(&argv(&["ferryx-cli", "supervisor", "engine", "status", "--json"])).unwrap(),
            SupervisorEngineAction::Status
        );
        assert_eq!(
            parse_supervisor_engine_cli(&argv(&["ferryx-cli", "supervisor", "engine", "assess", "--prompt", "grade it", "--json"]))
                .unwrap(),
            SupervisorEngineAction::Assess { prompt: "grade it".into() }
        );
    }

    #[test]
    fn the_engine_cli_refuses_bad_input_before_contacting_a_daemon() {
        for words in [
            &["ferryx-cli", "supervisor", "engine"][..],
            &["ferryx-cli", "supervisor", "engine", "restart"],
            &["ferryx-cli", "supervisor", "engine", "status", "extra"],
            &["ferryx-cli", "supervisor", "engine", "assess"],
            &["ferryx-cli", "supervisor", "engine", "assess", "--prompt"],
            &["ferryx-cli", "supervisor", "engine", "assess", "--prompt", "  "],
            &["ferryx-cli", "supervisor", "goal", "show", "ledger"],
        ] {
            assert!(parse_supervisor_engine_cli(&argv(words)).is_err(), "{words:?} should be refused");
        }
    }

    #[test]
    fn an_engine_reply_exits_zero_and_a_typed_fault_keeps_its_code() {
        let (value, code) = engine_outcome_of(DaemonResponse::SupervisorEngineOk {
            snapshot: serde_json::json!({"engine": {"state": "ready"}, "actions": []}),
        });
        assert_eq!(code, EXIT_OK);
        assert_eq!(value["ok"], true);
        assert_eq!(value["engine"]["state"], "ready");
        let (value, code) = engine_outcome_of(DaemonResponse::Error {
            message: "MHC_NOT_RUNNING: no mhc supervisor engine is installed in this daemon".into(),
            code: Some("MHC_NOT_RUNNING".into()),
            details: None,
        });
        assert_eq!(code, EXIT_OTHER);
        assert_eq!(value["code"], "MHC_NOT_RUNNING");
        assert_eq!(value["ok"], false);
        let (_, code) = engine_outcome_of(DaemonResponse::Error { message: "x".into(), code: Some("NOT_FOUND".into()), details: None });
        assert_eq!(code, EXIT_NOT_FOUND);
    }
}
