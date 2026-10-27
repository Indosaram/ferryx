//! `ferryx-cli supervisor request --json <file>`: sends one unified supervisor request envelope
//! (`{requestId, projectId?, goalId?, expectedRevision?, action, payload}`) to the running
//! daemon's shared supervisor service and prints the receipt or the typed refusal as one JSON
//! value. `-` reads the envelope from stdin. The daemon derives the actor; a process inside a
//! worker pane is refused FORBIDDEN by the daemon whatever its environment says.
//! Exit codes follow `agent_control::exit_code_for`; a 409 conflict is EXIT_REFUSED.

use serde_json::Value;

use crate::agent_control::{failure, UsageError, EXIT_INVALID, EXIT_OK, EXIT_REFUSED};
use crate::daemon::protocol::{DaemonRequest, DaemonResponse};

pub const REQUEST_USAGE: &str = "usage: ferryx-cli supervisor request --json <file|->";

/// The envelope file named by `ferryx-cli supervisor request --json <file>`.
pub fn parse_request_cli(args: &[String]) -> Result<String, UsageError> {
    match args.get(3..).unwrap_or_default() {
        [flag, path] if flag == "--json" && !path.trim().is_empty() => Ok(path.clone()),
        _ => Err(UsageError(REQUEST_USAGE.into())),
    }
}

/// Reads and parses the envelope. Its schema is the daemon's to check.
pub fn read_envelope(path: &str) -> Result<Value, UsageError> {
    let text = if path == "-" {
        let mut text = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut text)
            .map_err(|error| UsageError(format!("stdin: {error}")))?;
        text
    } else {
        std::fs::read_to_string(path).map_err(|error| UsageError(format!("{path}: {error}")))?
    };
    serde_json::from_str(&text).map_err(|error| UsageError(format!("{path}: {error}")))
}

/// The JSON and exit code a daemon reply maps to. A refusal keeps its HTTP-equivalent status.
pub fn outcome_of(response: DaemonResponse) -> (Value, i32) {
    match response {
        DaemonResponse::SupervisorRequestV1Ok { receipt } => {
            (serde_json::json!({ "ok": true, "receipt": receipt }), EXIT_OK)
        }
        DaemonResponse::Error { message, code, details } => {
            let code = code.filter(|code| !code.is_empty()).unwrap_or_else(|| "INTERNAL".into());
            let (mut value, mut exit) = failure(&code, &message);
            let status = details.as_ref().and_then(|details| details.get("status")).and_then(Value::as_u64);
            if let (Some(fields), Some(status)) = (value.as_object_mut(), status) {
                fields.insert("status".into(), status.into());
                if status == 409 {
                    exit = EXIT_REFUSED;
                }
            }
            (value, exit)
        }
        other => failure("INTERNAL", &format!("unexpected daemon response: {other:?}")),
    }
}

/// Run `ferryx-cli supervisor request ...`.
pub fn run_request_cli(args: &[String]) -> i32 {
    let envelope = parse_request_cli(args).and_then(|path| read_envelope(&path));
    let (value, code) = match envelope {
        Err(UsageError(message)) => {
            let (value, _) = failure("INVALID_REQUEST", &message);
            (value, EXIT_INVALID)
        }
        Ok(envelope) => match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(runtime) => runtime.block_on(async {
                let request = DaemonRequest::SupervisorRequestV1 { envelope, cli: true, mcp: false };
                match tokio::time::timeout(
                    super::supervisor_goal::GOAL_CLI_TIMEOUT,
                    super::supervisor_goal::exchange_once(
                        crate::hub_runtime::SUPERVISOR_REQUEST_CAPABILITY,
                        request,
                        true,
                    ),
                )
                .await
                {
                    Ok(Ok(response)) => outcome_of(response),
                    Ok(Err(failed)) => failed,
                    Err(_) => failure(
                        "OPERATION_OUTCOME_UNKNOWN",
                        "the daemon did not answer before the wait budget elapsed; the outcome is unknown",
                    ),
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

    fn argv(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| (*word).to_owned()).collect()
    }

    #[test]
    fn the_cli_takes_exactly_one_json_file() {
        assert_eq!(parse_request_cli(&argv(&["ferryx-cli", "supervisor", "request", "--json", "e.json"])).unwrap(), "e.json");
        assert_eq!(parse_request_cli(&argv(&["ferryx-cli", "supervisor", "request", "--json", "-"])).unwrap(), "-");
        for bad in [
            vec!["ferryx-cli", "supervisor", "request"],
            vec!["ferryx-cli", "supervisor", "request", "--json"],
            vec!["ferryx-cli", "supervisor", "request", "e.json"],
            vec!["ferryx-cli", "supervisor", "request", "--json", "e.json", "--actor", "x"],
        ] {
            assert!(parse_request_cli(&argv(&bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_receipt_and_typed_refusals_map_to_json_and_exit_codes() {
        let (value, code) = outcome_of(DaemonResponse::SupervisorRequestV1Ok { receipt: serde_json::json!({"revision": 2}) });
        assert_eq!((value["ok"].clone(), value["receipt"]["revision"].clone(), code), (true.into(), 2.into(), EXIT_OK));
        let stale = DaemonResponse::Error {
            message: "stale".into(),
            code: Some("REVISION_CONFLICT".into()),
            details: Some(serde_json::json!({"status": 409})),
        };
        let (value, code) = outcome_of(stale);
        assert_eq!((value["code"].clone(), value["status"].clone(), code), ("REVISION_CONFLICT".into(), 409.into(), EXIT_REFUSED));
        let forbidden = DaemonResponse::Error { message: "pane".into(), code: Some("FORBIDDEN".into()), details: None };
        assert_eq!(outcome_of(forbidden).1, EXIT_REFUSED);
        let invalid = DaemonResponse::Error { message: "bad".into(), code: Some("INVALID_REQUEST".into()), details: None };
        assert_eq!(outcome_of(invalid).1, EXIT_INVALID);
    }
}
