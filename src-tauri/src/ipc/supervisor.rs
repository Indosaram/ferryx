//! The desktop's Supervisor view: a snapshot of what the hub supervisor drives and watches, and
//! the pause a person sets on one lane. Both go to the daemon, which owns the hub.

use super::error::{IpcError, IpcErrorCode};
use crate::daemon::client::DaemonClient;
use crate::daemon::protocol::{
    DaemonRequest, DaemonResponse, SupervisorDispatchAction, SupervisorGoalAction, SupervisorReadAction,
    SupervisorSetupAction,
};
use crate::agent_control::AgentControlOp;
use crate::remote::reference_chat::types::ReferencePromptAnswer;
use std::sync::Arc;

/// A daemon that knows these requests types every failure, so an untyped error is an older daemon.
fn failure(message: String, code: Option<String>) -> IpcError {
    match code {
        None => IpcError::new(
            IpcErrorCode::DaemonUpgradeRequired,
            "The running Ferryx daemon predates the Supervisor view; it appears after the daemon restarts",
        ),
        Some(code) => IpcError::new(IpcErrorCode::from_code_str(&code), message),
    }
}

fn snapshot_from(response: DaemonResponse) -> Result<serde_json::Value, IpcError> {
    match response {
        DaemonResponse::SupervisorSnapshotOk { snapshot } => Ok(snapshot),
        DaemonResponse::Error { message, code, .. } => Err(failure(message, code)),
        _ => Err(IpcError::internal("Unexpected daemon supervisor snapshot response")),
    }
}

fn paused_from(response: DaemonResponse) -> Result<bool, IpcError> {
    match response {
        DaemonResponse::SupervisorSetPausedOk { paused } => Ok(paused),
        DaemonResponse::Error { message, code, .. } => Err(failure(message, code)),
        _ => Err(IpcError::internal("Unexpected daemon supervisor pause response")),
    }
}

fn goal_from(response: DaemonResponse) -> Result<serde_json::Value, IpcError> {
    match response {
        DaemonResponse::SupervisorGoalOk { snapshot } => Ok(snapshot),
        DaemonResponse::Error { message, code, .. } => Err(failure(message, code)),
        _ => Err(IpcError::internal("Unexpected daemon supervisor goal response")),
    }
}

#[tauri::command]
pub async fn cmd_supervisor_snapshot(
    daemon_client: tauri::State<'_, Arc<DaemonClient>>,
) -> Result<serde_json::Value, IpcError> {
    snapshot_from(daemon_client.send_request(DaemonRequest::SupervisorSnapshot).await?)
}

#[tauri::command]
pub async fn cmd_supervisor_set_paused(
    daemon_client: tauri::State<'_, Arc<DaemonClient>>,
    lane_key: String,
    paused: bool,
) -> Result<bool, IpcError> {
    paused_from(
        daemon_client
            .send_request(DaemonRequest::SupervisorSetPaused { lane_key, paused })
            .await?,
    )
}

/// Show or steer one lane's goal contract; returns the supervisor snapshot afterwards.
#[tauri::command]
pub async fn cmd_supervisor_goal(
    daemon_client: tauri::State<'_, Arc<DaemonClient>>,
    request: SupervisorGoalAction,
) -> Result<serde_json::Value, IpcError> {
    goal_from(
        daemon_client
            .send_request(DaemonRequest::SupervisorGoal { request })
            .await?,
    )
}

fn receipt_from(response: DaemonResponse) -> Result<serde_json::Value, IpcError> {
    match response {
        DaemonResponse::SupervisorRequestV1Ok { receipt } => Ok(receipt),
        DaemonResponse::Error {
            message,
            code,
            details,
        } => {
            let mut error = failure(message, code);
            if details.is_some() {
                error.details = details;
            }
            Err(error)
        }
        _ => Err(IpcError::internal(
            "Unexpected daemon supervisor request response",
        )),
    }
}

/// One unified supervisor request envelope; the daemon derives the actor.
#[tauri::command]
pub async fn cmd_supervisor_request(
    daemon_client: tauri::State<'_, Arc<DaemonClient>>,
    envelope: serde_json::Value,
) -> Result<serde_json::Value, IpcError> {
    receipt_from(
        daemon_client
            .send_request(DaemonRequest::SupervisorRequestV1 {
                envelope,
                cli: false,
                mcp: false,
            })
            .await?,
    )
}

fn read_from(response: DaemonResponse) -> Result<serde_json::Value, IpcError> {
    match response {
        DaemonResponse::SupervisorReadV1Ok { value } => Ok(value),
        DaemonResponse::Error { message, code, details } => {
            let mut error = failure(message, code);
            if details.is_some() {
                error.details = details;
            }
            Err(error)
        }
        _ => Err(IpcError::internal("Unexpected daemon supervisor read response")),
    }
}

/// A read-only unified supervisor view: one project's conversation page or the memory
/// receipts. Refusals keep their wire code (`SUPERVISOR_UNAVAILABLE`, `MEMORY_*`, ...).
#[tauri::command]
pub async fn cmd_supervisor_read(
    daemon_client: tauri::State<'_, Arc<DaemonClient>>,
    request: SupervisorReadAction,
) -> Result<serde_json::Value, IpcError> {
    read_from(daemon_client.send_request(DaemonRequest::SupervisorReadV1 { request }).await?)
}

fn converse_from(response: DaemonResponse) -> Result<serde_json::Value, IpcError> {
    match response {
        DaemonResponse::SupervisorConverseV1Ok { value } => Ok(value),
        DaemonResponse::Error { message, code, details } => {
            let mut error = failure(message, code);
            if details.is_some() {
                error.details = details;
            }
            Err(error)
        }
        _ => Err(IpcError::internal("Unexpected daemon supervisor converse response")),
    }
}

/// Saves transcript text as a new file in the user's Downloads folder and returns its path. The
/// desktop webview has no download handler, so the panel's "Export transcript" lands here. The
/// name is a plain file name (no path parts), an existing file is never overwritten, and a write
/// that fails leaves no partial file behind.
fn write_download(
    downloads: &std::path::Path,
    file_name: &str,
    text: &str,
) -> Result<std::path::PathBuf, IpcError> {
    write_download_with(downloads, file_name, text, |file, bytes| {
        use std::io::Write;
        file.write_all(bytes)
    })
}

fn write_download_with(
    downloads: &std::path::Path,
    file_name: &str,
    text: &str,
    write: impl Fn(&mut std::fs::File, &[u8]) -> std::io::Result<()>,
) -> Result<std::path::PathBuf, IpcError> {
    let plain = !file_name.is_empty()
        && file_name.len() <= 128
        && file_name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !file_name.starts_with('.');
    if !plain {
        return Err(IpcError::new(IpcErrorCode::InvalidRequest, "the export file name must be a plain file name"));
    }
    let io = |error: std::io::Error| IpcError::new(IpcErrorCode::IoError, format!("the transcript was not saved: {error}"));
    std::fs::create_dir_all(downloads).map_err(io)?;
    let (stem, extension) = file_name.rsplit_once('.').map_or((file_name, String::new()), |(s, e)| (s, format!(".{e}")));
    for attempt in 0..100 {
        let name = if attempt == 0 { file_name.to_owned() } else { format!("{stem} ({attempt}){extension}") };
        let path = downloads.join(name);
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                if let Err(error) = write(&mut file, text.as_bytes()) {
                    drop(file);
                    let _ = std::fs::remove_file(&path);
                    return Err(io(error));
                }
                return Ok(path);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(io(error)),
        }
    }
    Err(IpcError::new(IpcErrorCode::IoError, "the transcript was not saved: too many files with that name"))
}

/// The panel's "Export transcript" on the desktop: the transcript text the panel read from the
/// app transcript is saved to the Downloads folder; the reply is the saved path.
#[tauri::command]
pub async fn cmd_supervisor_transcript_export(file_name: String, text: String) -> Result<String, IpcError> {
    // The platform's Downloads folder (Known Folder redirection on Windows, XDG on Linux); a
    // home-relative `Downloads` only when the platform names none.
    let downloads = dirs::download_dir()
        .or_else(|| ferryx_hub::hub_env::home().map(|home| home.join("Downloads")))
        .ok_or_else(|| IpcError::new(IpcErrorCode::IoError, "the transcript was not saved: this account has no Downloads folder"))?;
    let path = write_download(&downloads, &file_name, &text)?;
    Ok(path.to_string_lossy().into_owned())
}

/// One owner message from the panel composer: the supervisor answers on the `app` channel and
/// both are stored in the app transcript. Returns `{owner, reply, replayed}`.
#[tauri::command]
pub async fn cmd_supervisor_converse(
    daemon_client: tauri::State<'_, Arc<DaemonClient>>,
    text: String,
    mentions: Vec<String>,
    request_id: String,
) -> Result<serde_json::Value, IpcError> {
    converse_from(
        daemon_client
            .send_request(DaemonRequest::SupervisorConverseV1 { text, mentions, request_id })
            .await?,
    )
}

fn setup_from(response: DaemonResponse) -> Result<serde_json::Value, IpcError> {
    match response {
        DaemonResponse::SupervisorSetupOk { value } => Ok(value),
        DaemonResponse::Error { message, code, .. } => Err(failure(message, code)),
        _ => Err(IpcError::internal("Unexpected daemon supervisor setup response")),
    }
}

/// The shared supervisor SetupStatus.
#[tauri::command]
pub async fn cmd_supervisor_setup_status(
    daemon_client: tauri::State<'_, Arc<DaemonClient>>,
) -> Result<serde_json::Value, IpcError> {
    setup_from(
        daemon_client
            .send_request(DaemonRequest::SupervisorSetup { request: SupervisorSetupAction::Status })
            .await?,
    )
}

/// The Supervisor panel snapshot (`PanelSnapshot`, camelCase): sessions, handling, log, rules,
/// decisions, the pause flag and `setupRequired` when no hub runs.
#[tauri::command]
pub async fn cmd_supervisor_panel(
    daemon_client: tauri::State<'_, Arc<DaemonClient>>,
) -> Result<serde_json::Value, IpcError> {
    read_from(
        daemon_client
            .send_request(DaemonRequest::SupervisorReadV1 { request: SupervisorReadAction::Panel })
            .await?,
    )
}

/// Runs automatic supervisor setup in the daemon-hosted hub; returns the fresh SetupStatus.
#[tauri::command]
pub async fn cmd_supervisor_setup_run(
    daemon_client: tauri::State<'_, Arc<DaemonClient>>,
    import_memory: bool,
    projects: Vec<crate::hub_runtime::SetupProject>,
) -> Result<serde_json::Value, IpcError> {
    setup_from(
        daemon_client
            .send_request(DaemonRequest::SupervisorSetup {
                request: SupervisorSetupAction::Run { import_memory, projects },
            })
            .await?,
    )
}

/// Adopts every adoptable legacy lane as the owner: {adopted: [lane], refused: [{lane, code, message}]}.
#[tauri::command]
pub async fn cmd_supervisor_adopt_all(
    daemon_client: tauri::State<'_, Arc<DaemonClient>>,
) -> Result<serde_json::Value, IpcError> {
    setup_from(
        daemon_client
            .send_request(DaemonRequest::SupervisorSetup { request: SupervisorSetupAction::AdoptAll })
            .await?,
    )
}


/// A `<hostId>:<backendSessionId>` handle split for agent control: the host is `None` (this
/// daemon) when it names this machine, else the paired host that owns the pane.
fn agent_target(session_ref: &str, local_host_id: &str) -> Result<(Option<String>, String), IpcError> {
    let parsed = ferryx_hub::supervisor::panel_contract::SessionRef::parse(session_ref)
        .map_err(|error| IpcError::new(IpcErrorCode::InvalidArgument, error.to_string()))?;
    let host = parsed.host_id();
    let host_id = (host != ferryx_hub::agent_control::LOCAL_HOST_ID && host != local_host_id)
        .then(|| host.to_owned());
    Ok((host_id, parsed.backend_session_id().to_owned()))
}

/// The agent-control request that stops one session: the provider's interrupt key, straight to
/// the daemon. No hub and no engine are on this path, so it works when either is down.
fn stop_request(session_ref: &str, local_host_id: &str, request_id: String) -> Result<DaemonRequest, IpcError> {
    let (host_id, session_id) = agent_target(session_ref, local_host_id)?;
    Ok(DaemonRequest::AgentControl {
        request: AgentControlOp::Stop { session_id, request_id: Some(request_id) },
        host_id,
    })
}

/// The agent-control request for an owner's tap on a decision card. The screen revision is the
/// stale guard: an answer for a screen that moved on types nothing.
fn answer_request(
    session_ref: &str,
    local_host_id: &str,
    prompt_id: String,
    screen_revision: String,
    option_index: u32,
    request_id: String,
) -> Result<DaemonRequest, IpcError> {
    let (host_id, session_id) = agent_target(session_ref, local_host_id)?;
    Ok(DaemonRequest::AgentControl {
        request: AgentControlOp::Answer {
            session_id,
            prompt_id,
            screen_revision,
            answer: ReferencePromptAnswer { option_index: Some(option_index), ..Default::default() },
            request_id: Some(request_id),
        },
        host_id,
    })
}

fn control_result(response: DaemonResponse) -> Result<serde_json::Value, IpcError> {
    match response {
        DaemonResponse::AgentControlOk { result } | DaemonResponse::SupervisorControlOk { result } => Ok(result),
        DaemonResponse::Error { message, code, details } => {
            let mut error = failure(message, code);
            if details.is_some() {
                error.details = details;
            }
            Err(error)
        }
        _ => Err(IpcError::internal("Unexpected daemon supervisor control response")),
    }
}

fn local_host_id() -> String {
    crate::remote::reference_chat::files::reference_host_id()
}

/// Stop one session. Straight to agent control (the provider's interrupt key), never through
/// the hub or the engine.
#[tauri::command]
pub async fn cmd_session_stop(
    daemon_client: tauri::State<'_, Arc<DaemonClient>>,
    session_ref: String,
) -> Result<serde_json::Value, IpcError> {
    let request = stop_request(&session_ref, &local_host_id(), uuid::Uuid::new_v4().to_string())?;
    control_result(daemon_client.send_request(request).await?)
}

/// The owner taps an option on a decision card. No answer gate: it is the owner's own tap.
#[tauri::command]
pub async fn cmd_supervisor_answer(
    daemon_client: tauri::State<'_, Arc<DaemonClient>>,
    session_ref: String,
    prompt_id: String,
    screen_revision: String,
    option_index: u32,
) -> Result<serde_json::Value, IpcError> {
    let request = answer_request(
        &session_ref,
        &local_host_id(),
        prompt_id,
        screen_revision,
        option_index,
        uuid::Uuid::new_v4().to_string(),
    )?;
    control_result(daemon_client.send_request(request).await?)
}

/// Pause (`true`) or resume (`false`) every supervisor action at once.
#[tauri::command]
pub async fn cmd_supervisor_pause(
    daemon_client: tauri::State<'_, Arc<DaemonClient>>,
    paused: bool,
) -> Result<serde_json::Value, IpcError> {
    control_result(daemon_client.send_request(DaemonRequest::SupervisorGlobalPause { paused }).await?)
}

/// The persisted supervisor settings; with `enabled` set, the owner turns the supervisor on or
/// off first. The reply's `enabled` is true only for the owner home that turned it on.
#[tauri::command]
pub async fn cmd_supervisor_settings(
    daemon_client: tauri::State<'_, Arc<DaemonClient>>,
    enabled: Option<bool>,
) -> Result<serde_json::Value, IpcError> {
    control_result(daemon_client.send_request(DaemonRequest::SupervisorSettings { enabled }).await?)
}

/// Undo one action-log entry through its undo handle.
#[tauri::command]
pub async fn cmd_supervisor_action_undo(
    daemon_client: tauri::State<'_, Arc<DaemonClient>>,
    id: i64,
) -> Result<serde_json::Value, IpcError> {
    control_result(daemon_client.send_request(DaemonRequest::SupervisorActionUndo { id }).await?)
}

/// "Start a session": submit owner work to an explicit project. A request without a project
/// is refused here, before the daemon is asked: the panel never guesses one.
fn dispatch_request(
    project_id: &str,
    title: String,
    instruction: String,
    requirements: Vec<ferryx_hub::goals::contract::RequirementSpec>,
    host_id: Option<String>,
    session_id: Option<String>,
    message_id: String,
) -> Result<DaemonRequest, IpcError> {
    let invalid = |message: &str| IpcError::new(IpcErrorCode::InvalidArgument, message);
    if project_id.trim().is_empty() {
        return Err(invalid("a dispatch needs an explicit project"));
    }
    if title.trim().is_empty() || instruction.trim().is_empty() {
        return Err(invalid("a dispatch needs a title and an instruction"));
    }
    if requirements.is_empty() {
        return Err(invalid("a dispatch needs at least one requirement"));
    }
    if session_id.is_some() && host_id.is_none() {
        return Err(invalid("binding a session needs its host"));
    }
    let work = ferryx_hub::supervisor::admission::SubmitWork {
        project_id: Some(project_id.trim().to_owned()),
        host_id: host_id.clone(),
        kind: None,
        title,
        instruction: instruction.clone(),
        requirements,
        cwd: None,
        operations: Vec::new(),
        turn_limit: None,
    };
    Ok(DaemonRequest::SupervisorDispatch {
        request: SupervisorDispatchAction::Submit {
            message_id,
            body: instruction,
            host_id,
            session_id,
            work: Box::new(work),
        },
    })
}

/// Start work in a project from the panel's "+" or composer. Admitted as the owner through the
/// existing dispatch path; the work narrows to what the owner chose, never widens.
#[tauri::command]
pub async fn cmd_supervisor_dispatch(
    daemon_client: tauri::State<'_, Arc<DaemonClient>>,
    project_id: String,
    title: String,
    instruction: String,
    requirements: Vec<ferryx_hub::goals::contract::RequirementSpec>,
    host_id: Option<String>,
    session_id: Option<String>,
) -> Result<serde_json::Value, IpcError> {
    let request = dispatch_request(
        &project_id,
        title,
        instruction,
        requirements,
        host_id,
        session_id,
        uuid::Uuid::new_v4().to_string(),
    )?;
    match daemon_client.send_request(request).await? {
        DaemonResponse::SupervisorDispatchOk { result } => Ok(result),
        DaemonResponse::Error { message, code, details } => {
            let mut error = failure(message, code);
            if details.is_some() {
                error.details = details;
            }
            Err(error)
        }
        _ => Err(IpcError::internal("Unexpected daemon supervisor dispatch response")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_setup_answer_is_its_value_and_an_untyped_error_is_an_older_daemon() {
        let value = serde_json::json!({"nextAction": "map_projects"});
        assert_eq!(setup_from(DaemonResponse::SupervisorSetupOk { value: value.clone() }).unwrap(), value);
        let older = setup_from(DaemonResponse::Error { message: "unknown".into(), code: None, details: None }).unwrap_err();
        assert_eq!(older.code, IpcErrorCode::DaemonUpgradeRequired);
    }

    #[test]
    fn a_local_session_ref_stops_on_this_daemon_and_a_paired_one_forwards_to_its_host() {
        let local = stop_request("local:pane-1", "local", "r1".into()).unwrap();
        assert!(matches!(
            local,
            DaemonRequest::AgentControl { host_id: None, request: AgentControlOp::Stop { ref session_id, request_id: Some(ref id) } }
                if session_id == "pane-1" && id == "r1"
        ));
        // A host id that is this machine's own configured id is local too.
        let own = stop_request("studio:pane-1", "studio", "r1".into()).unwrap();
        assert!(matches!(own, DaemonRequest::AgentControl { host_id: None, .. }));
        let remote = stop_request("omarchy:pane:7", "studio", "r2".into()).unwrap();
        assert!(matches!(
            remote,
            DaemonRequest::AgentControl { host_id: Some(ref host), request: AgentControlOp::Stop { ref session_id, .. } }
                if host == "omarchy" && session_id == "pane:7"
        ));
    }

    #[test]
    fn a_malformed_session_ref_is_refused_before_the_daemon_is_asked() {
        for bad in ["nohost", ":pane", "host:", ""] {
            assert_eq!(
                stop_request(bad, "local", "r".into()).unwrap_err().code,
                IpcErrorCode::InvalidArgument,
                "{bad}"
            );
        }
    }

    #[test]
    fn an_owner_tap_carries_the_stale_guard_and_the_chosen_option_only() {
        let request = answer_request("local:pane-1", "local", "p1".into(), "rev-9".into(), 2, "r".into()).unwrap();
        let DaemonRequest::AgentControl { request: AgentControlOp::Answer { prompt_id, screen_revision, answer, session_id, .. }, host_id } =
            request
        else {
            panic!("an answer is an agent-control answer");
        };
        assert_eq!((prompt_id.as_str(), screen_revision.as_str(), session_id.as_str()), ("p1", "rev-9", "pane-1"));
        assert_eq!(host_id, None);
        assert_eq!(answer.option_index, Some(2));
        assert!(answer.is_single_choice(), "exactly one answer shape");
    }

    fn requirement() -> ferryx_hub::goals::contract::RequirementSpec {
        ferryx_hub::goals::contract::RequirementSpec {
            id: "r1".into(),
            text: "result.txt contains READY".into(),
            verifier: "verify.sh exits 0".into(),
        }
    }

    #[test]
    fn a_dispatch_names_its_project_and_is_refused_without_one() {
        let request = dispatch_request(
            " proj-1 ",
            "Fix relay".into(),
            "Fix the reconnect".into(),
            vec![requirement()],
            Some("omarchy".into()),
            None,
            "m1".into(),
        )
        .unwrap();
        let DaemonRequest::SupervisorDispatch { request: SupervisorDispatchAction::Submit { message_id, host_id, session_id, work, .. } } =
            request
        else {
            panic!("a dispatch is a submit");
        };
        assert_eq!(message_id, "m1");
        assert_eq!(host_id.as_deref(), Some("omarchy"));
        assert_eq!(session_id, None);
        assert_eq!(work.project_id.as_deref(), Some("proj-1"), "the explicit project, trimmed");
        assert!(work.operations.is_empty(), "a dispatch asks for no extra operations");

        let refused = |project: &str| {
            dispatch_request(project, "t".into(), "i".into(), vec![requirement()], None, None, "m".into())
                .unwrap_err()
                .code
        };
        assert_eq!(refused(""), IpcErrorCode::InvalidArgument);
        assert_eq!(refused("   "), IpcErrorCode::InvalidArgument);
    }

    #[test]
    fn a_dispatch_needs_a_title_an_instruction_a_requirement_and_a_host_for_a_bound_session() {
        let build = |title: &str, instruction: &str, requirements: Vec<_>, host: Option<&str>, session: Option<&str>| {
            dispatch_request(
                "p",
                title.into(),
                instruction.into(),
                requirements,
                host.map(str::to_owned),
                session.map(str::to_owned),
                "m".into(),
            )
        };
        assert!(build("", "i", vec![requirement()], None, None).is_err());
        assert!(build("t", " ", vec![requirement()], None, None).is_err());
        assert!(build("t", "i", Vec::new(), None, None).is_err());
        assert!(build("t", "i", vec![requirement()], None, Some("pane-1")).is_err());
        assert!(build("t", "i", vec![requirement()], Some("h"), Some("pane-1")).is_ok());
    }

    #[test]
    fn control_replies_carry_their_result_and_a_refusal_keeps_its_code_and_details() {
        let ok = DaemonResponse::SupervisorControlOk { result: serde_json::json!({"paused": true}) };
        assert_eq!(control_result(ok).unwrap()["paused"], true);
        let agent = DaemonResponse::AgentControlOk { result: serde_json::json!({"receipt": {"stage": "accepted"}}) };
        assert_eq!(control_result(agent).unwrap()["receipt"]["stage"], "accepted");
        let stale = DaemonResponse::Error {
            message: "the screen moved on".into(),
            code: Some("REQUEST_CONFLICT".into()),
            details: Some(serde_json::json!({"status": 409})),
        };
        let error = control_result(stale).unwrap_err();
        assert_eq!(error.code, IpcErrorCode::RequestConflict);
        assert_eq!(error.details.unwrap()["status"], 409);
        let old = DaemonResponse::Error { message: "unknown variant".into(), code: None, details: None };
        assert_eq!(control_result(old).unwrap_err().code, IpcErrorCode::DaemonUpgradeRequired);
    }

    #[test]
    fn an_untyped_error_means_the_daemon_is_older() {
        let old = DaemonResponse::Error {
            message: "unknown variant".into(),
            code: None,
            details: None,
        };
        assert_eq!(
            snapshot_from(old).unwrap_err().code,
            IpcErrorCode::DaemonUpgradeRequired
        );
        let typed = DaemonResponse::Error {
            message: "the supervisor hub is not open in this daemon".into(),
            code: Some("SUPERVISOR_UNAVAILABLE".into()),
            details: None,
        };
        assert_ne!(
            paused_from(typed).unwrap_err().code,
            IpcErrorCode::DaemonUpgradeRequired
        );
        assert!(paused_from(DaemonResponse::SupervisorSetPausedOk { paused: true }).unwrap());
    }

    #[test]
    fn a_goal_reply_carries_the_snapshot_and_a_conflict_stays_typed() {
        let ok = DaemonResponse::SupervisorGoalOk {
            snapshot: serde_json::json!({"lanes": []}),
        };
        assert_eq!(goal_from(ok).unwrap(), serde_json::json!({"lanes": []}));
        let conflict = DaemonResponse::Error {
            message: "lane ledger has no proposal awaiting approval".into(),
            code: Some("REQUEST_CONFLICT".into()),
            details: None,
        };
        assert_eq!(
            goal_from(conflict).unwrap_err().code,
            IpcErrorCode::RequestConflict
        );
        let request: SupervisorGoalAction =
            serde_json::from_value(serde_json::json!({"action": "approve", "laneKey": "ledger"}))
                .unwrap();
        assert_eq!(
            request,
            SupervisorGoalAction::Approve {
                lane_key: "ledger".into()
            }
        );
    }

    #[test]
    fn a_supervisor_request_reply_carries_the_receipt_and_a_refusal_keeps_its_details() {
        let ok = DaemonResponse::SupervisorRequestV1Ok {
            receipt: serde_json::json!({"goalId": "g", "revision": 2}),
        };
        assert_eq!(receipt_from(ok).unwrap()["revision"], 2);
        let stale = DaemonResponse::Error {
            message: "goal g revision changed".into(),
            code: Some("REVISION_CONFLICT".into()),
            details: Some(serde_json::json!({"code": "REVISION_CONFLICT", "status": 409})),
        };
        let error = receipt_from(stale).unwrap_err();
        assert_ne!(error.code, IpcErrorCode::DaemonUpgradeRequired);
        assert_eq!(error.details.unwrap()["status"], 409);
    }

    #[test]
    fn a_supervisor_read_reply_carries_the_view_and_a_refusal_keeps_its_code() {
        let ok = DaemonResponse::SupervisorReadV1Ok { value: serde_json::json!({"next": 3, "messages": []}) };
        assert_eq!(read_from(ok).unwrap()["next"], 3);
        let memory = DaemonResponse::Error {
            message: "the hub has no native memory receipt listing yet".into(),
            code: Some("MEMORY_RECEIPTS_UNAVAILABLE".into()),
            details: Some(serde_json::json!({"code": "MEMORY_RECEIPTS_UNAVAILABLE", "status": 503})),
        };
        let error = read_from(memory).unwrap_err();
        assert_eq!(error.code, IpcErrorCode::Custom("MEMORY_RECEIPTS_UNAVAILABLE".into()));
        assert_eq!(error.details.unwrap()["status"], 503);
        let old = DaemonResponse::Error { message: "unknown variant".into(), code: None, details: None };
        assert_eq!(read_from(old).unwrap_err().code, IpcErrorCode::DaemonUpgradeRequired);
        let request: SupervisorReadAction =
            serde_json::from_value(serde_json::json!({"view": "conversation", "projectId": "p", "after": 2})).unwrap();
        assert_eq!(request, SupervisorReadAction::Conversation { project_id: "p".into(), after: Some(2) });
    }

    #[test]
    fn a_converse_reply_carries_the_rows_and_a_refusal_keeps_its_code() {
        let ok = DaemonResponse::SupervisorConverseV1Ok { value: serde_json::json!({"replayed": false}) };
        assert_eq!(converse_from(ok).unwrap()["replayed"], false);
        let refused = DaemonResponse::Error {
            message: "no mhc supervisor engine is installed in this daemon".into(),
            code: Some("MHC_NOT_RUNNING".into()),
            details: Some(serde_json::json!({"code": "MHC_NOT_RUNNING", "status": 503})),
        };
        let error = converse_from(refused).unwrap_err();
        assert_eq!(error.code, IpcErrorCode::Custom("MHC_NOT_RUNNING".into()));
        assert_eq!(error.details.unwrap()["status"], 503);
        let old = DaemonResponse::Error { message: "unknown variant".into(), code: None, details: None };
        assert_eq!(converse_from(old).unwrap_err().code, IpcErrorCode::DaemonUpgradeRequired);
    }

    #[test]
    fn an_export_is_a_new_plain_named_file_that_never_overwrites() {
        let dir = tempfile::tempdir().expect("tempdir");
        let downloads = dir.path().join("Downloads");
        let first = write_download(&downloads, "supervisor-transcript-1.txt", "Owner prompt\nSupervisor reply\n").unwrap();
        assert_eq!(std::fs::read_to_string(&first).unwrap(), "Owner prompt\nSupervisor reply\n");
        let second = write_download(&downloads, "supervisor-transcript-1.txt", "later").unwrap();
        assert_ne!(first, second);
        assert_eq!(std::fs::read_to_string(&first).unwrap(), "Owner prompt\nSupervisor reply\n");
        for bad in ["", "../escape.txt", "a/b.txt", ".hidden", "a b.txt"] {
            let refused = write_download(&downloads, bad, "x").unwrap_err();
            assert_eq!(refused.code, IpcErrorCode::InvalidRequest, "{bad:?}");
        }
    }

    #[test]
    fn a_failed_write_removes_the_partial_file_and_a_retry_can_reuse_the_name() {
        let dir = tempfile::tempdir().expect("tempdir");
        let failing = write_download_with(dir.path(), "t.txt", "text", |file, _| {
            use std::io::Write;
            file.write_all(b"par")?;
            Err(std::io::Error::other("disk full"))
        })
        .unwrap_err();
        assert_eq!(failing.code, IpcErrorCode::IoError);
        assert!(failing.message.contains("disk full"));
        assert!(!dir.path().join("t.txt").exists(), "no partial file is left");
        let again = write_download(dir.path(), "t.txt", "text").unwrap();
        assert_eq!(again, dir.path().join("t.txt"));
    }
}
