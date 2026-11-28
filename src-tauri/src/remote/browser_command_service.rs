//! One browser command on a paired host, forwarded over the relay the remote UI already uses.
//!
//! The CLI reaches this through `DaemonRequest::Browser { host_id: Some(..) }`, exactly as
//! `agents --host` reaches `DaemonRequest::AgentControl`. The daemon does the branching, so no
//! second transport exists and a command is never re-issued after an ambiguous answer.

use std::sync::Arc;

use crate::daemon::protocol::DaemonResponse;
use crate::remote::browser_backend::BrowserCommandContext;
use crate::remote::server::BROWSER_REMOTE_CAPABILITY;
use crate::remote::state::RemoteGatewayState;

/// The wire error a browser refusal travels as on the daemon socket. The code keeps the
/// typed names the browser backend already uses, so a caller can tell a forbidden command
/// from a missing browser without parsing the message.
pub(crate) fn browser_error_response(
    error: crate::remote::browser_backend::RemoteBrowserError,
) -> DaemonResponse {
    use crate::remote::browser_backend::RemoteBrowserError as E;
    let code = match &error {
        E::Unavailable(_) => "BROWSER_UNAVAILABLE",
        E::NotFound(_) => "BROWSER_NOT_FOUND",
        E::Forbidden(_) => "BROWSER_FORBIDDEN",
        E::InvalidRequest(_) => "BROWSER_INVALID_REQUEST",
        E::WaitTimeout => "BROWSER_WAIT_TIMEOUT",
        E::TargetNotFound(_) => "BROWSER_TARGET_NOT_FOUND",
        E::StaleFrame(_) => "BROWSER_STALE_FRAME",
        E::ExecutionFailed(_) => "BROWSER_EXECUTION_FAILED",
        _ => "BROWSER_FAILED",
    };
    DaemonResponse::Error {
        message: error.to_string(),
        code: Some(code.into()),
        details: None,
    }
}

/// Forward one browser command to a paired host. Executes once: a lost answer is
/// `OPERATION_OUTCOME_UNKNOWN` and is not sent again, because a mutation may have applied.
pub(crate) async fn forward_browser(
    _state: &Arc<RemoteGatewayState>,
    hosts: &crate::paired_host::service::PairedHostService,
    host_id: String,
    ctx: BrowserCommandContext,
) -> DaemonResponse {
    let known = match hosts.list().await {
        Ok(known) => known,
        Err(error) => {
            return DaemonResponse::Error {
                code: Some("INVENTORY_INCOMPLETE".into()),
                message: format!(
                    "the paired-host inventory could not be read: {} ({})",
                    error.code, error.message
                ),
                details: None,
            };
        }
    };
    let Some(host) = known.into_iter().find(|host| host.host_id == host_id) else {
        return DaemonResponse::Error {
            code: Some("NOT_FOUND".into()),
            message: format!("no paired host is named {host_id}"),
            details: None,
        };
    };
    let request = crate::paired_host::client::OperationRequest {
        host_id: host.host_id,
        generation: host.generation,
        operation: crate::paired_host::client::Operation::Browser {
            browser_id: ctx.browser_id,
            command: ctx.command,
            params: ctx.params,
        },
    };
    match crate::paired_host::client::MachineClient::new()
        .execute(hosts, request)
        .await
    {
        Ok(response) => match response.result {
            crate::paired_host::client::OperationResult::Browser(value) => {
                DaemonResponse::BrowserOk { result: value }
            }
            other => DaemonResponse::Error {
                code: Some("INTERNAL".into()),
                message: format!("unexpected answer for a browser command: {other:?}"),
                details: None,
            },
        },
        // The command may have reached the host. Say so instead of inviting a retry.
        Err(error) if error.ambiguous || error.code == "OPERATION_OUTCOME_UNKNOWN" => {
            DaemonResponse::Error {
                code: Some("OPERATION_OUTCOME_UNKNOWN".into()),
                message: format!(
                    "the paired host may have applied the browser command; it was not sent again ({host_id})"
                ),
                details: None,
            }
        }
        Err(error) => {
            let code = error
                .machine_error
                .as_ref()
                .map(|machine| machine.code.as_str())
                .unwrap_or(error.code.as_str());
            DaemonResponse::Error {
                code: Some(crate::remote::agent_control_service::forward_code(code)),
                message: error
                    .machine_error
                    .as_ref()
                    .map(|machine| machine.message.clone())
                    .unwrap_or(error.message),
                details: None,
            }
        }
    }
}

/// The capability a peer must advertise before a browser command is written to it.
pub(crate) const REQUIRED_CAPABILITY: &str = BROWSER_REMOTE_CAPABILITY;

/// Reads one paired host's advertised capabilities.
///
/// A read is safe to repeat, so an ambiguous answer is reported as unknown rather than being
/// treated as a failure of the host.
pub(crate) async fn forward_capabilities(
    hosts: &crate::paired_host::service::PairedHostService,
    host_id: String,
) -> DaemonResponse {
    let known = match hosts.list().await {
        Ok(known) => known,
        Err(error) => {
            return DaemonResponse::Error {
                code: Some("INVENTORY_INCOMPLETE".into()),
                message: format!(
                    "the paired-host inventory could not be read: {} ({})",
                    error.code, error.message
                ),
                details: None,
            }
        }
    };
    let Some(host) = known.into_iter().find(|host| host.host_id == host_id) else {
        return DaemonResponse::Error {
            code: Some("NOT_FOUND".into()),
            message: format!("no paired host is named {host_id}"),
            details: None,
        };
    };
    let request = crate::paired_host::client::OperationRequest {
        host_id: host.host_id,
        generation: host.generation,
        operation: crate::paired_host::client::Operation::Capabilities,
    };
    match crate::paired_host::client::MachineClient::new()
        .execute(hosts, request)
        .await
    {
        Ok(response) => match response.result {
            crate::paired_host::client::OperationResult::Capabilities(capabilities) => {
                match serde_json::to_value(capabilities) {
                    Ok(capabilities) => DaemonResponse::HostCapabilitiesOk { capabilities },
                    Err(error) => DaemonResponse::Error {
                        code: Some("INTERNAL".into()),
                        message: format!("the host capabilities could not be encoded: {error}"),
                        details: None,
                    },
                }
            }
            other => DaemonResponse::Error {
                code: Some("INTERNAL".into()),
                message: format!("unexpected answer for a capabilities read: {other:?}"),
                details: None,
            },
        },
        Err(error) => DaemonResponse::Error {
            code: Some(crate::remote::agent_control_service::forward_code(
                error
                    .machine_error
                    .as_ref()
                    .map(|machine| machine.code.as_str())
                    .unwrap_or(error.code.as_str()),
            )),
            message: error
                .machine_error
                .as_ref()
                .map(|machine| machine.message.clone())
                .unwrap_or_else(|| error.code.clone()),
            details: None,
        },
    }
}
