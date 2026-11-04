//! `ferryx-cli grants add|remove|list`: the owner's cross-pane grants (rex todo 5), and the
//! owner-action check that `up`, `pair` and `remote` run inside a Ferryx pane.
//!
//! The daemon decides who may change grants (only a peer outside every pane); this CLI only
//! shapes the request and prints one JSON value.
use serde_json::Value;

use crate::agent_control::{failure, UsageError, EXIT_INVALID, EXIT_OK};
use crate::daemon::pane_grants::{GrantOp, PaneGrant, PaneGrantAction, GRANT_HINT, OWNER_ACTION_REQUIRED, PANE_GRANTS_CAPABILITY};
use crate::daemon::protocol::{DaemonRequest, DaemonResponse};

pub const GRANTS_USAGE: &str = "usage: ferryx-cli grants add --from <session|workspace> --to <session|workspace|*> [--ops write,control,spawn,admin] | grants remove --from <id> --to <id> | grants list";

const GRANTS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|index| args.get(index + 1))
        .cloned()
}

fn required(args: &[String], name: &str) -> Result<String, UsageError> {
    flag(args, name)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| UsageError(format!("{name} is required\n{GRANTS_USAGE}")))
}

/// Parses `ferryx-cli grants ...` (`args[1] == "grants"`).
pub fn parse_grants_cli(args: &[String]) -> Result<PaneGrantAction, UsageError> {
    let rest = args.get(3..).unwrap_or_default();
    match args.get(2).map(String::as_str) {
        Some("list") => Ok(PaneGrantAction::List),
        Some("add") => {
            let ops = flag(rest, "--ops").unwrap_or_else(|| "write".into());
            let ops = ops
                .split(',')
                .map(|op| GrantOp::parse(op.trim()).ok_or_else(|| UsageError(format!("unknown op `{op}`\n{GRANTS_USAGE}"))))
                .collect::<Result<_, _>>()?;
            Ok(PaneGrantAction::Add {
                grant: PaneGrant { from: required(rest, "--from")?, to: required(rest, "--to")?, ops },
            })
        }
        Some("remove") => Ok(PaneGrantAction::Remove { from: required(rest, "--from")?, to: required(rest, "--to")? }),
        _ => Err(UsageError(GRANTS_USAGE.into())),
    }
}

/// The JSON and exit code a daemon reply maps to. An in-pane change is FORBIDDEN with the hint.
pub fn outcome_of(response: DaemonResponse) -> (Value, i32) {
    match response {
        DaemonResponse::PaneGrantsOk { grants, .. } => (serde_json::json!({ "ok": true, "grants": grants }), EXIT_OK),
        DaemonResponse::Error { message, code, .. } => {
            let code = code.unwrap_or_default();
            let (mut value, exit) = failure(if code.is_empty() { "INTERNAL" } else { &code }, &message);
            if code == "FORBIDDEN" {
                value["hint"] = GRANT_HINT.into();
            }
            (value, exit)
        }
        other => failure("INTERNAL", &format!("unexpected daemon response: {other:?}")),
    }
}

fn block_on<T>(future: impl std::future::Future<Output = T>) -> Result<T, (Value, i32)> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map(|runtime| runtime.block_on(future))
        .map_err(|error| failure("INTERNAL", &error.to_string()))
}

/// Runs `ferryx-cli grants ...`: prints one JSON value and returns the exit code.
pub fn run_grants_cli(args: &[String]) -> i32 {
    let (value, code) = match parse_grants_cli(args) {
        Err(UsageError(message)) => {
            let (value, _) = failure("INVALID_REQUEST", &message);
            (value, EXIT_INVALID)
        }
        Ok(action) => {
            let mutates = !matches!(action, PaneGrantAction::List);
            let request = DaemonRequest::PaneGrant { request: action };
            match block_on(tokio::time::timeout(
                GRANTS_TIMEOUT,
                super::supervisor_goal::exchange_once(PANE_GRANTS_CAPABILITY, request, mutates),
            )) {
                Ok(Ok(Ok(response))) => outcome_of(response),
                Ok(Ok(Err(failed))) | Err(failed) => failed,
                Ok(Err(_)) => failure("TIMEOUT", "the daemon did not answer before the wait budget elapsed"),
            }
        }
    };
    println!("{value}");
    code
}

/// Owner actions (`up`, `pair`, `remote`) refuse to run inside a Ferryx pane unless the daemon
/// says this process may (outside every pane, or a pane with an `admin` grant). Outside a pane
/// (no `FERRYX_SESSION_ID`) nothing is asked. A daemon that cannot answer (none running, an
/// older daemon without `paneGrantsV1`) leaves the decision to the daemon-side gate.
pub fn owner_action_refusal(session_id: Option<&std::ffi::OsStr>) -> Option<String> {
    session_id.filter(|session| !session.is_empty())?;
    let check = DaemonRequest::PaneGrant { request: PaneGrantAction::CheckAdmin };
    let answer = block_on(tokio::time::timeout(
        GRANTS_TIMEOUT,
        super::supervisor_goal::exchange_once(PANE_GRANTS_CAPABILITY, check, false),
    ));
    match answer {
        Ok(Ok(Ok(DaemonResponse::PaneGrantsOk { admin: Some(false), .. }))) => Some(format!(
            "FORBIDDEN: {OWNER_ACTION_REQUIRED}: run from Settings > Pane access or outside a pane \
             (this shell runs inside a Ferryx pane; run it from a terminal outside Ferryx or grant admin first)"
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| (*arg).to_owned()).collect()
    }

    #[test]
    fn grants_cli_parses_add_remove_and_list() {
        assert_eq!(parse_grants_cli(&argv(&["ferryx-cli", "grants", "list"])), Ok(PaneGrantAction::List));
        match parse_grants_cli(&argv(&["ferryx-cli", "grants", "add", "--from", "a", "--to", "b", "--ops", "write,control"])) {
            Ok(PaneGrantAction::Add { grant }) => {
                assert_eq!((grant.from.as_str(), grant.to.as_str()), ("a", "b"));
                assert_eq!(grant.ops.into_iter().collect::<Vec<_>>(), vec![GrantOp::Write, GrantOp::Control]);
            }
            other => panic!("{other:?}"),
        }
        match parse_grants_cli(&argv(&["ferryx-cli", "grants", "add", "--from", "a", "--to", "b"])) {
            Ok(PaneGrantAction::Add { grant }) => assert_eq!(grant.ops.into_iter().collect::<Vec<_>>(), vec![GrantOp::Write]),
            other => panic!("{other:?}"),
        }
        assert_eq!(
            parse_grants_cli(&argv(&["ferryx-cli", "grants", "remove", "--from", "a", "--to", "b"])),
            Ok(PaneGrantAction::Remove { from: "a".into(), to: "b".into() })
        );
        for bad in [
            &["ferryx-cli", "grants"][..],
            &["ferryx-cli", "grants", "add", "--from", "a"],
            &["ferryx-cli", "grants", "add", "--from", "a", "--to", "b", "--ops", "root"],
            &["ferryx-cli", "grants", "remove", "--to", "b"],
            &["ferryx-cli", "grants", "add", "--from", "", "--to", "b"],
        ] {
            assert!(parse_grants_cli(&argv(bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_forbidden_reply_carries_the_settings_hint() {
        let (value, code) = outcome_of(DaemonResponse::Error {
            message: "only the owner may change pane grants".into(),
            code: Some("FORBIDDEN".into()),
            details: None,
        });
        assert_eq!(value["code"], "FORBIDDEN");
        assert_eq!(value["hint"], GRANT_HINT);
        assert_eq!(code, crate::agent_control::EXIT_REFUSED);
    }

    #[test]
    fn outside_a_pane_owner_actions_ask_nothing() {
        assert_eq!(owner_action_refusal(None), None);
        assert_eq!(owner_action_refusal(Some(std::ffi::OsStr::new(""))), None);
    }
}
