//! Daemon side of the agent-neutral control surface (`crate::agent_control`).
//!
//! A child of `remote::server` on purpose: every operation reuses the reference-chat machinery
//! that already serves the phone client — the reconstructed screen, the per-agent prompt
//! detectors, the screen-validated answer and the ordered per-target input queue — so a
//! supervisor and a person on the phone share ONE queue per pane and cannot interleave keys.
//!
//! The caller is the machine owner: the request arrives on the daemon's owner-only control
//! socket, so there is no device token to re-check between writes. Nothing here launches,
//! signals or kills a process; Stop is the provider's interrupt key.

use super::*;

use crate::agent_control::events::{self, WaitHit};
use crate::agent_control::policy::{self, authorize_agent_control, AuditRecord};
use crate::agent_control::{
    derive_status, AgentControlOp, AgentControlState, AgentStateSource, AgentTier, ProviderReport,
};
use crate::scoped_contracts::{Epoch, ScopeError, ScopeErrorCode, TargetRef};
use futures_util::stream::{self, StreamExt as _};

/// Record that a person typed into `session_id` through the daemon input path.
pub(crate) fn note_human_keystrokes(session_id: &str) {
    let clock = reference_input::SystemReferenceClock;
    reference_chat_runtime()
        .input
        .note_raw_input(session_id, clock.now_ms());
}

fn refuse_if_human_typing(session_id: &str) -> Result<(), ScopeError> {
    let Some(typed_at) = reference_chat_runtime().input.last_raw_input_ms(session_id) else {
        return Ok(());
    };
    let now = reference_input::SystemReferenceClock.now_ms();
    if now.saturating_sub(typed_at) < policy::HUMAN_TYPING_WINDOW_MS {
        return Err(reference_chat_scope(
            ScopeErrorCode::ControlConflict,
            "a person typed in this pane; supervisor input is refused",
        ));
    }
    Ok(())
}

fn mutation_authorize(
    session_id: String,
    extra: Option<Arc<dyn Fn() -> Result<(), ScopeError> + Send + Sync>>,
) -> Box<dyn Fn() -> Result<(), ScopeError> + Send + Sync> {
    Box::new(move || {
        if let Some(extra) = &extra {
            extra()?;
        } else {
            // The owner socket has no device token. The uid check on connect is the authorization.
            owner_authorized()()?;
        }
        refuse_if_human_typing(&session_id)
    })
}

/// How many sessions a list reconstructs at once.
const LIST_CONCURRENCY: usize = 8;

fn owner_authorized() -> Box<dyn Fn() -> Result<(), ScopeError> + Send + Sync> {
    Box::new(|| Ok(()))
}

fn control_target(state: &RemoteGatewayState, session_id: &str) -> reference_types::ReferenceTargetRef {
    reference_types::ReferenceTargetRef::without_provider_session(TargetRef {
        host_id: reference_files::reference_host_id(),
        owner_id: state.reference_owner_id.clone(),
        epoch: Epoch(state.daemon_epoch.load(std::sync::atomic::Ordering::Acquire)),
        backend_session_id: session_id.to_string(),
    })
}

fn provider_report(state: &RemoteGatewayState, session_id: &str) -> Option<ProviderReport> {
    state.machine_services.as_ref()?.sessions.agent_control_report(session_id)
}

/// The wire error a refusal travels as on the daemon socket.
pub(crate) fn error_response(error: ScopeError) -> crate::daemon::protocol::DaemonResponse {
    crate::daemon::protocol::DaemonResponse::Error {
        message: error.message,
        code: Some(reference_chat_wire_code(error.code).to_string()),
        details: None,
    }
}

/// One session's status, and the screen it was read from.
struct Observed {
    entry: serde_json::Value,
    screen: Option<reference_types::ReferenceScreenSnapshot>,
}

async fn observe(state: &Arc<RemoteGatewayState>, session_id: &str) -> Result<Observed, ScopeError> {
    let details = state
        .session_backend
        .describe_session(session_id)
        .await
        .map_err(|message| reference_chat_scope(ScopeErrorCode::NotFound, message))?;
    let target = control_target(state, session_id);
    let screen = reference_chat_snapshot_screen(state, &target).await.ok();
    let provider = provider_report(state, session_id);
    let detection = screen.as_ref().and_then(|snapshot| {
        let input = crate::agent_detect::ScreenInput {
            rows: snapshot.text.lines().map(str::to_string).collect(),
            title: String::new(),
        };
        crate::agent_detect::default_engine().detect(&input, None)
    });
    let agent = provider
        .as_ref()
        .and_then(|report| report.agent.clone())
        .filter(|id| !id.trim().is_empty())
        .or_else(|| detection.as_ref().map(|found| found.manifest_id.clone()).filter(|id| !id.is_empty()));
    let readable = screen.as_ref().is_some_and(|snapshot| snapshot.is_answerable());
    let pending = match (&agent, &screen) {
        (Some(agent), Some(snapshot))
            if readable && reference_prompts::ReferencePromptFamily::of_agent(agent).is_some() =>
        {
            let identity = reference_chat_identity(state, &target, Some(agent)).await;
            let session = reference_chat_prompt_session(state, &identity).await;
            reference_prompts::detect_reference_prompt_in_session(agent, &snapshot.text, &session.borrow())
        }
        _ => None,
    };
    let status = derive_status(provider.as_ref(), detection.map(|found| found.state), pending.is_some());
    let entry = serde_json::json!({
        "sessionId": session_id,
        "running": details.running,
        "cols": details.cols,
        "rows": details.rows,
        "workspaceId": details.workspace_id,
        "cwd": details.worktree_path.map(|path| path.to_string_lossy().into_owned()),
        "agent": agent,
        "tier": status.tier,
        "state": status.state,
        "stateSource": status.source,
        "detail": provider.and_then(|report| report.detail),
        "pending": pending,
        "screenRevision": screen.as_ref().map(|snapshot| snapshot.revision.clone()),
        "screenReadable": readable,
    });
    Ok(Observed { entry, screen })
}

async fn live(state: &Arc<RemoteGatewayState>, session_id: &str) -> Result<(), ScopeError> {
    reference_chat_ensure_live(state, session_id).await
}

fn outcome_result(outcome: reference_input::ReferenceInputOutcome) -> Result<serde_json::Value, ScopeError> {
    match outcome {
        reference_input::ReferenceInputOutcome::Accepted { receipt } => Ok(serde_json::json!({ "receipt": receipt })),
        reference_input::ReferenceInputOutcome::NotTyped { error } => Err(error),
        reference_input::ReferenceInputOutcome::OutcomeUnknown { message, .. } => {
            Err(reference_chat_scope(ScopeErrorCode::OperationOutcomeUnknown, message))
        }
    }
}

fn joined(result: Result<reference_input::ReferenceInputOutcome, tokio::task::JoinError>) -> Result<serde_json::Value, ScopeError> {
    match result {
        Ok(outcome) => outcome_result(outcome),
        Err(error) => Err(reference_chat_scope(
            ScopeErrorCode::OperationOutcomeUnknown,
            format!("the input transaction did not finish; its outcome is unknown: {error}"),
        )),
    }
}

fn request_id_or_new(request_id: Option<String>) -> String {
    request_id
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| format!("agent-control-{}", uuid::Uuid::new_v4()))
}

/// The agent the per-agent detectors and answer planners run for.
fn registry_agent(observed: &Observed) -> Option<String> {
    observed.entry.get("agent").and_then(serde_json::Value::as_str).map(str::to_string)
}

fn replay_value(replay: events::AgentReplay) -> serde_json::Value {
    match replay {
        events::AgentReplay::Events {
            events,
            after_sequence,
        } => serde_json::json!({
            "gap": false,
            "events": events.iter().map(|event| event.sequenced()).collect::<Vec<_>>(),
            "afterSequence": after_sequence,
        }),
        events::AgentReplay::Gap {
            snapshot,
            after_sequence,
        } => serde_json::json!({
            "gap": true,
            "snapshot": snapshot.iter().map(|event| event.sequenced()).collect::<Vec<_>>(),
            "afterSequence": after_sequence,
        }),
    }
}

fn wait_value(hit: WaitHit) -> serde_json::Value {
    match hit {
        WaitHit::Observation(observation) => serde_json::json!({
            "gap": false,
            "observation": observation.sequenced(),
            "afterSequence": observation.sequence,
        }),
        WaitHit::Gap {
            snapshot,
            after_sequence,
        } => serde_json::json!({
            "gap": true,
            "snapshot": snapshot.iter().map(|event| event.sequenced()).collect::<Vec<_>>(),
            "afterSequence": after_sequence,
        }),
    }
}

/// Run one operation for the machine owner. The owner socket stays authorized.
pub(crate) async fn handle(
    state: &Arc<RemoteGatewayState>,
    op: AgentControlOp,
) -> Result<serde_json::Value, ScopeError> {
    handle_as(state, op, "owner", None).await
}

/// Run one operation as `actor`, re-checking `authorize` inside every input transaction.
///
/// Every call, including a refusal, is appended to the agent-control audit log with the device id.
pub(crate) async fn handle_as(
    state: &Arc<RemoteGatewayState>,
    op: AgentControlOp,
    actor: &str,
    authorize: Option<Arc<dyn Fn() -> Result<(), ScopeError> + Send + Sync>>,
) -> Result<serde_json::Value, ScopeError> {
    let result = dispatch(state, op.clone(), authorize).await;
    state.agent_control_audit.record(AuditRecord {
        device_id: actor.to_string(),
        op: policy::audit_op_name(&op).to_string(),
        session_id: policy::audit_session(&op),
        request_id: policy::audit_request(&op),
        code: match &result {
            Ok(_) => "OK".to_string(),
            Err(error) => reference_chat_wire_code(error.code).to_string(),
        },
    });
    result
}

async fn dispatch(
    state: &Arc<RemoteGatewayState>,
    op: AgentControlOp,
    authorize: Option<Arc<dyn Fn() -> Result<(), ScopeError> + Send + Sync>>,
) -> Result<serde_json::Value, ScopeError> {
    match op {
        AgentControlOp::List => {
            let ids = state.session_backend.list_sessions().await;
            let entries: Vec<serde_json::Value> = stream::iter(ids)
                .map(|id| async move {
                    match observe(state, &id).await {
                        Ok(observed) => observed.entry,
                        // A session that vanished between the list and its read is reported, not dropped.
                        Err(error) => serde_json::json!({
                            "sessionId": id,
                            "running": false,
                            "tier": AgentTier::Screen,
                            "state": AgentControlState::Unknown,
                            "stateSource": AgentStateSource::None,
                            "error": error.message,
                        }),
                    }
                })
                .buffered(LIST_CONCURRENCY)
                .collect()
                .await;
            Ok(serde_json::json!({ "sessions": entries }))
        }
        AgentControlOp::Read { session_id } => {
            live(state, &session_id).await?;
            let observed = observe(state, &session_id).await?;
            let mut entry = observed.entry;
            entry["screen"] = match observed.screen {
                Some(snapshot) => serde_json::json!({
                    "text": snapshot.text,
                    "revision": snapshot.revision,
                    "gap": snapshot.gap,
                    "truncated": snapshot.truncated,
                }),
                None => serde_json::Value::Null,
            };
            Ok(serde_json::json!({ "session": entry }))
        }
        AgentControlOp::Send { session_id, text, request_id } => {
            if text.trim().is_empty() || text.contains('\0') {
                return Err(reference_chat_scope(ScopeErrorCode::InvalidRequest, "text must not be empty or contain NUL"));
            }
            live(state, &session_id).await?;
            refuse_if_human_typing(&session_id)?;
            let hub = Arc::clone(state.terminal_service.output_hub());
            // A pane this hub does not hold has a paste mode this host cannot read; a multi-line
            // message would submit its first line on its own, so it is refused untyped.
            if hub.session_sequence_range(&session_id).is_none()
                && reference_input::reference_composer_body(&text).contains('\r')
            {
                return Err(reference_chat_scope(
                    ScopeErrorCode::Unsupported,
                    "this pane's paste mode cannot be read on this host, so a multi-line message cannot be typed as one turn; nothing was typed",
                ));
            }
            let observed = observe(state, &session_id).await?;
            let agent = registry_agent(&observed);
            let target = control_target(state, &session_id);
            let request_id = request_id_or_new(request_id);
            let authorize = mutation_authorize(session_id.clone(), authorize);
            let state = Arc::clone(state);
            let transaction = tokio::spawn(async move {
                let payload = reference_types::ReferenceSubmitPayload {
                    text,
                    attachment_ids: Vec::new(),
                    origin: reference_types::ReferenceSubmitOrigin::Chat,
                };
                let paste_session = session_id.clone();
                let clock = reference_input::SystemReferenceClock;
                let submit = reference_input::ReferenceSubmitRequest {
                    target: &target,
                    request_id: &request_id,
                    payload: &payload,
                    bracketed_paste: &move || hub.is_bracketed_paste_enabled(&paste_session),
                    blocked_prompt: None,
                    arrived_at_ms: clock.now_ms(),
                    last_typed_at_ms: None,
                    authorize: &*authorize,
                };
                let probe = || reference_chat_blocked_prompt(&state, &target, agent.as_deref());
                let writer = reference_input::ReferenceSessionWriter::new(state.session_backend.as_ref());
                reference_chat_runtime().input.submit_with_probe(&submit, probe, &writer, &clock).await
            });
            joined(transaction.await)
        }
        AgentControlOp::Answer { session_id, prompt_id, screen_revision, answer, request_id } => {
            live(state, &session_id).await?;
            refuse_if_human_typing(&session_id)?;
            let observed = observe(state, &session_id).await?;
            let Some(agent) = registry_agent(&observed) else {
                return Err(reference_chat_scope(
                    ScopeErrorCode::Unsupported,
                    "no prompt reader knows the agent in this pane, so a prompt cannot be answered",
                ));
            };
            let target = control_target(state, &session_id);
            let request_id = request_id_or_new(request_id);
            let authorize = mutation_authorize(session_id.clone(), authorize);
            let state = Arc::clone(state);
            let transaction = tokio::spawn(async move {
                let identity = reference_chat_identity(&state, &target, Some(&agent)).await;
                let prompt_session = reference_chat_prompt_session(&state, &identity).await;
                let payload = reference_types::ReferencePromptAnswerPayload {
                    prompt_id,
                    screen_revision,
                    answer,
                };
                let alive = || true;
                let request = reference_prompts::ReferenceAnswerRequest {
                    target: &target,
                    request_id: &request_id,
                    payload: &payload,
                    session: prompt_session.borrow(),
                    agent: agent.as_str(),
                    authorize: &*authorize,
                    alive: &alive,
                };
                let runtime = reference_chat_runtime();
                let step = reference_prompts::ReferenceQueueStep::new(&runtime.input);
                let screen = ReferenceGatewayScreenReader { backend: Arc::clone(&state.session_backend) };
                let writer = reference_input::ReferenceSessionWriter::new(state.session_backend.as_ref());
                let clock = reference_input::SystemReferenceClock;
                runtime.answers.answer(&request, &step, &screen, &writer, &clock).await
            });
            joined(transaction.await)
        }
        AgentControlOp::Stop { session_id, request_id } => {
            live(state, &session_id).await?;
            refuse_if_human_typing(&session_id)?;
            let target = control_target(state, &session_id);
            let request_id = request_id_or_new(request_id);
            let payload = reference_types::ReferenceStopPayload {
                capability: reference_types::ReferenceStopCapability::ProviderInterrupt,
            };
            let authorize = mutation_authorize(session_id.clone(), authorize);
            let alive = || true;
            let stop = reference_input::ReferenceStopRequest {
                target: &target,
                request_id: &request_id,
                payload: &payload,
                authorize: &*authorize,
                alive: &alive,
            };
            let writer = reference_input::ReferenceSessionWriter::new(state.session_backend.as_ref());
            outcome_result(reference_chat_runtime().input.stop(&stop, &writer).await)
        }
        AgentControlOp::Wait {
            session_id,
            until,
            after_revision,
            after,
            timeout_ms,
        } => {
            if !events::until_is_known(&until) {
                return Err(reference_chat_scope(
                    ScopeErrorCode::InvalidRequest,
                    "until must be working, blocked, idle, prompt or any",
                ));
            }
            let timeout_ms = policy::clamp_wait_timeout(timeout_ms)?;
            live(state, &session_id).await?;
            let hit = events::wait_for(
                &state.agent_events,
                &session_id,
                &until,
                after,
                after_revision.as_deref(),
                std::time::Duration::from_millis(timeout_ms),
            )
            .await;
            match hit {
                Some(hit) => Ok(wait_value(hit)),
                None => Err(reference_chat_scope(
                    ScopeErrorCode::Timeout,
                    "the pane did not reach that state before the timeout",
                )),
            }
        }
        AgentControlOp::Events { after } => Ok(replay_value(state.agent_events.replay(after))),
    }
}

struct ForwardFailure {
    code: String,
    message: String,
}

fn forward_code(code: &str) -> String {
    match code {
        "PAIRED_HOST_CAPABILITY_UNAVAILABLE" => "UNSUPPORTED".into(),
        "SESSION_NOT_FOUND" | "SESSION_EXPIRED" => "NOT_FOUND".into(),
        other => other.to_string(),
    }
}

/// Forward one operation to a paired host. Executes once. An ambiguous transport error is
/// `OPERATION_OUTCOME_UNKNOWN` and is not issued again, even when the caller omitted a request id.
pub(crate) async fn forward_daemon(
    state: &Arc<RemoteGatewayState>,
    hosts: &crate::paired_host::service::PairedHostService,
    host_id: String,
    op: AgentControlOp,
) -> crate::daemon::protocol::DaemonResponse {
    let failure = match forward_once(hosts, &host_id, op.clone()).await {
        Ok(result) => {
            state.agent_control_audit.record(AuditRecord {
                device_id: "owner".into(),
                op: policy::audit_op_name(&op).to_string(),
                session_id: policy::audit_session(&op),
                request_id: policy::audit_request(&op),
                code: "OK".into(),
            });
            return crate::daemon::protocol::DaemonResponse::AgentControlOk { result };
        }
        Err(failure) => failure,
    };
    state.agent_control_audit.record(AuditRecord {
        device_id: "owner".into(),
        op: policy::audit_op_name(&op).to_string(),
        session_id: policy::audit_session(&op),
        request_id: policy::audit_request(&op),
        code: failure.code.clone(),
    });
    crate::daemon::protocol::DaemonResponse::Error {
        message: failure.message,
        code: Some(failure.code),
        details: None,
    }
}

async fn forward_once(
    hosts: &crate::paired_host::service::PairedHostService,
    host_id: &str,
    op: AgentControlOp,
) -> Result<serde_json::Value, ForwardFailure> {
    let known = hosts.list().await.map_err(|error| ForwardFailure {
        code: forward_code(&error.code),
        message: "the paired-host inventory could not be read".into(),
    })?;
    let Some(host) = known.into_iter().find(|host| host.host_id == host_id) else {
        return Err(ForwardFailure {
            code: "NOT_FOUND".into(),
            message: format!("no paired host is named {host_id}"),
        });
    };
    let request = crate::paired_host::client::OperationRequest {
        host_id: host.host_id,
        generation: host.generation,
        operation: crate::paired_host::client::Operation::AgentControl { op },
    };
    // One round trip. MachineClient does not mint a new request id, and this function does not
    // call it again when the answer is ambiguous.
    match crate::paired_host::client::MachineClient::new()
        .execute(hosts, request)
        .await
    {
        Ok(response) => match response.result {
            crate::paired_host::client::OperationResult::AgentControl(value) => Ok(value),
            _ => Err(ForwardFailure {
                code: "INTERNAL".into(),
                message: "the paired host answered a different operation".into(),
            }),
        },
        Err(error) if error.ambiguous || error.code == "OPERATION_OUTCOME_UNKNOWN" => {
            Err(ForwardFailure {
                code: "OPERATION_OUTCOME_UNKNOWN".into(),
                message: "the paired host may have applied the operation; it was not sent again"
                    .into(),
            })
        }
        Err(error) => {
            let code = error
                .machine_error
                .as_ref()
                .map(|machine| machine.code.as_str())
                .unwrap_or(error.code.as_str());
            Err(ForwardFailure {
                code: forward_code(code),
                message: error
                    .machine_error
                    .as_ref()
                    .map(|machine| machine.message.clone())
                    .unwrap_or_else(|| "the paired host refused agent control".into()),
            })
        }
    }
}

fn control_json(value: serde_json::Value) -> Response {
    (
        StatusCode::OK,
        [(header::CACHE_CONTROL, "no-store")],
        Json(value),
    )
        .into_response()
}

fn control_finish(result: Result<serde_json::Value, ScopeError>) -> Response {
    match result {
        Ok(value) => control_json(value),
        Err(error) => reference_chat_refusal(&error),
    }
}

fn http_device(
    state: &RemoteGatewayState,
    headers: &HeaderMap,
    mutation: bool,
) -> Result<DeviceInfo, Response> {
    let token = extract_token(headers)
        .ok_or_else(|| reference_chat_error(ScopeErrorCode::Unauthorized, "missing auth token"))?;
    let device = state.auth_manager.validate_token(&token).map_err(|_| {
        reference_chat_error(ScopeErrorCode::Unauthorized, "invalid or revoked token")
    })?;
    authorize_agent_control(
        &device,
        mutation,
        state.agent_control_policy.permits(&device.id),
    )
    .map_err(|error| reference_chat_refusal(&error))?;
    Ok(device)
}

fn http_reauthorize(
    state: &Arc<RemoteGatewayState>,
    headers: &HeaderMap,
) -> Arc<dyn Fn() -> Result<(), ScopeError> + Send + Sync> {
    let token = extract_token(headers);
    let auth = Arc::clone(&state.auth_manager);
    let policy = Arc::clone(&state.agent_control_policy);
    Arc::new(move || {
        let token = token.clone().ok_or_else(|| {
            reference_chat_scope(ScopeErrorCode::Unauthorized, "the caller's token is gone")
        })?;
        let device = auth.validate_token(&token).map_err(|_| {
            reference_chat_scope(
                ScopeErrorCode::Unauthorized,
                "the caller's device is no longer authorized",
            )
        })?;
        authorize_agent_control(&device, true, policy.permits(&device.id))
    })
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EventsQuery {
    #[serde(default)]
    after: u64,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct WaitQuery {
    until: String,
    #[serde(default)]
    after: u64,
    timeout_ms: u64,
    #[serde(default)]
    revision: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SendBody {
    text: String,
    #[serde(default)]
    request_id: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AnswerBody {
    prompt_id: String,
    screen_revision: String,
    answer: crate::remote::reference_chat::types::ReferencePromptAnswer,
    #[serde(default)]
    request_id: Option<String>,
}

#[derive(serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StopBody {
    #[serde(default)]
    request_id: Option<String>,
}

fn query_error(rejection: QueryRejection) -> Response {
    reference_chat_error(
        ScopeErrorCode::InvalidRequest,
        format!("the query is not readable: {rejection}"),
    )
}

pub(crate) async fn http_sessions(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = http_device(&state, &headers, false) {
        return response;
    }
    control_finish(handle(&state, AgentControlOp::List).await)
}

pub(crate) async fn http_session(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
) -> Response {
    if let Err(response) = http_device(&state, &headers, false) {
        return response;
    }
    control_finish(handle(&state, AgentControlOp::Read { session_id }).await)
}

pub(crate) async fn http_events(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    query: Result<Query<EventsQuery>, QueryRejection>,
) -> Response {
    if let Err(response) = http_device(&state, &headers, false) {
        return response;
    }
    let query = match query {
        Ok(Query(query)) => query,
        Err(rejection) => return query_error(rejection),
    };
    control_finish(handle(&state, AgentControlOp::Events { after: query.after }).await)
}

pub(crate) async fn http_wait(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
    query: Result<Query<WaitQuery>, QueryRejection>,
) -> Response {
    if let Err(response) = http_device(&state, &headers, false) {
        return response;
    }
    let query = match query {
        Ok(Query(query)) => query,
        Err(rejection) => return query_error(rejection),
    };
    control_finish(
        handle(
            &state,
            AgentControlOp::Wait {
                session_id,
                until: query.until,
                after_revision: query.revision,
                after: query.after,
                timeout_ms: query.timeout_ms,
            },
        )
        .await,
    )
}

pub(crate) async fn http_send(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
    body: Result<Json<SendBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let device = match http_device(&state, &headers, true) {
        Ok(device) => device,
        Err(response) => return response,
    };
    let Json(body) = match body {
        Ok(body) => body,
        Err(_) => {
            return reference_chat_error(
                ScopeErrorCode::InvalidRequest,
                "the send body is not readable",
            )
        }
    };
    let authorize = http_reauthorize(&state, &headers);
    control_finish(
        handle_as(
            &state,
            AgentControlOp::Send {
                session_id,
                text: body.text,
                request_id: body.request_id,
            },
            &device.id,
            Some(authorize),
        )
        .await,
    )
}

pub(crate) async fn http_answer(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
    body: Result<Json<AnswerBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let device = match http_device(&state, &headers, true) {
        Ok(device) => device,
        Err(response) => return response,
    };
    let Json(body) = match body {
        Ok(body) => body,
        Err(_) => {
            return reference_chat_error(
                ScopeErrorCode::InvalidRequest,
                "the answer body is not readable",
            )
        }
    };
    let authorize = http_reauthorize(&state, &headers);
    control_finish(
        handle_as(
            &state,
            AgentControlOp::Answer {
                session_id,
                prompt_id: body.prompt_id,
                screen_revision: body.screen_revision,
                answer: body.answer,
                request_id: body.request_id,
            },
            &device.id,
            Some(authorize),
        )
        .await,
    )
}

pub(crate) async fn http_stop(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
    body: Result<Json<StopBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let device = match http_device(&state, &headers, true) {
        Ok(device) => device,
        Err(response) => return response,
    };
    let body = match body {
        Ok(Json(body)) => body,
        Err(_) => StopBody::default(),
    };
    let authorize = http_reauthorize(&state, &headers);
    control_finish(
        handle_as(
            &state,
            AgentControlOp::Stop {
                session_id,
                request_id: body.request_id,
            },
            &device.id,
            Some(authorize),
        )
        .await,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::backend::RemoteSessionBackend as _;
    use crate::remote::reference_chat::types::ReferencePromptAnswer;

    const CLAUDE_APPROVAL: &str = include_str!("reference_chat/fixtures/claude/prompt/approval-command.screen");

    /// One pane showing a fixed screen; every write and every signal it is handed is recorded.
    struct FixedPane {
        session_id: String,
        screen: Vec<u8>,
        writes: std::sync::Mutex<Vec<Vec<u8>>>,
        signals: std::sync::Mutex<Vec<crate::terminal::TerminalSignal>>,
        fail_writes: std::sync::atomic::AtomicBool,
        write_attempts: std::sync::atomic::AtomicUsize,
    }

    impl crate::remote::backend::RemoteSessionBackend for FixedPane {
        fn list_sessions(&self) -> futures_util::future::BoxFuture<'_, Vec<String>> {
            Box::pin(async move { vec![self.session_id.clone()] })
        }
        fn describe_session<'a>(
            &'a self,
            session_id: &'a str,
        ) -> futures_util::future::BoxFuture<'a, Result<RemoteSessionDetails, String>> {
            Box::pin(async move {
                if session_id != self.session_id {
                    return Err("unknown session".to_string());
                }
                Ok(RemoteSessionDetails {
                    session_id: self.session_id.clone(),
                    workspace_id: None,
                    worktree_label: None,
                    worktree_path: None,
                    running: true,
                    cols: 100,
                    rows: 30,
                })
            })
        }
        fn attach_with_sequence<'a>(
            &'a self,
            _session_id: &'a str,
            _after_sequence: Option<u64>,
        ) -> futures_util::future::BoxFuture<'a, Result<crate::terminal::SessionAttachment, String>> {
            Box::pin(async move {
                let (sender, receiver) = tokio::sync::broadcast::channel(1);
                std::mem::forget(sender);
                Ok(crate::terminal::SessionAttachment {
                    snapshot: crate::terminal::AttachmentSnapshot {
                        session_id: self.session_id.clone(),
                        history_start_sequence: Some(1),
                        history_end_sequence: Some(1),
                        history: self.screen.clone(),
                        history_segments: vec![crate::terminal::HistorySegment {
                            cols: Some(100),
                            rows: Some(30),
                            bytes: self.screen.clone(),
                        }],
                        gap: None,
                        history_incomplete: false,
                    },
                    receiver,
                })
            })
        }
        fn write_input<'a>(
            &'a self,
            _session_id: &'a str,
            data: &'a [u8],
        ) -> futures_util::future::BoxFuture<'a, Result<(), String>> {
            self.write_attempts
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.fail_writes.load(std::sync::atomic::Ordering::SeqCst) {
                return Box::pin(async { Err("the write failed".to_string()) });
            }
            self.writes.lock().expect("writes").push(data.to_vec());
            Box::pin(async { Ok(()) })
        }
        fn resize<'a>(&'a self, _session_id: &'a str, _cols: u16, _rows: u16) -> futures_util::future::BoxFuture<'a, Result<(), String>> {
            Box::pin(async { Err("this pane never resizes".to_string()) })
        }
        fn signal<'a>(
            &'a self,
            _session_id: &'a str,
            signal: crate::terminal::TerminalSignal,
        ) -> futures_util::future::BoxFuture<'a, Result<(), String>> {
            self.signals.lock().expect("signals").push(signal);
            Box::pin(async { Ok(()) })
        }
    }

    fn pane(session_id: &str, screen: &str) -> (Arc<FixedPane>, Arc<RemoteGatewayState>) {
        let backend = Arc::new(FixedPane {
            session_id: session_id.to_string(),
            screen: screen.trim_end().replace('\n', "\r\n").into_bytes(),
            writes: std::sync::Mutex::new(Vec::new()),
            signals: std::sync::Mutex::new(Vec::new()),
            fail_writes: std::sync::atomic::AtomicBool::new(false),
            write_attempts: std::sync::atomic::AtomicUsize::new(0),
        });
        let state = Arc::new(RemoteGatewayState::new_with_backend(
            Arc::clone(&backend) as Arc<dyn crate::remote::backend::RemoteSessionBackend>,
            crate::worktree::registry::WorkspaceRegistry::new(),
        ));
        (backend, state)
    }

    /// The pending prompt a read reports, and the revision it was read from.
    async fn pending_prompt(state: &Arc<RemoteGatewayState>, session_id: &str) -> (String, String) {
        let read = handle(state, AgentControlOp::Read { session_id: session_id.to_string() }).await.expect("read");
        let session = &read["session"];
        assert_eq!(session["agent"], "claude", "{read}");
        assert_eq!(session["state"], "blocked", "{read}");
        assert_eq!(session["stateSource"], "screen", "{read}");
        let prompt_id = session["pending"]["id"].as_str().expect("a pending prompt").to_string();
        let revision = session["screenRevision"].as_str().expect("a screen revision").to_string();
        (prompt_id, revision)
    }

    fn answer(session_id: &str, prompt_id: &str, revision: &str, request_id: Option<&str>) -> AgentControlOp {
        AgentControlOp::Answer {
            session_id: session_id.to_string(),
            prompt_id: prompt_id.to_string(),
            screen_revision: revision.to_string(),
            answer: ReferencePromptAnswer { option_index: Some(0), ..Default::default() },
            request_id: request_id.map(str::to_string),
        }
    }

    #[tokio::test]
    async fn an_answer_to_the_prompt_on_the_screen_is_typed() {
        let session_id = "agent-control-answer-ok";
        let (backend, state) = pane(session_id, CLAUDE_APPROVAL);
        let (prompt_id, revision) = pending_prompt(&state, session_id).await;

        let result = handle(&state, answer(session_id, &prompt_id, &revision, None)).await.expect("the answer is accepted");

        assert_eq!(result["receipt"]["stage"], "accepted", "{result}");
        assert!(!backend.writes.lock().expect("writes").is_empty(), "an accepted answer typed its keys");
        let replay = handle(&state, answer(session_id, &prompt_id, &revision, Some("agent-control-answer-replay")))
            .await
            .expect_err("a card is answered once");
        assert_eq!(replay.code, ScopeErrorCode::RequestConflict, "{}", replay.message);
    }

    #[tokio::test]
    async fn an_answer_for_a_screen_that_moved_on_types_nothing() {
        let session_id = "agent-control-answer-stale";
        let (backend, state) = pane(session_id, CLAUDE_APPROVAL);
        let (prompt_id, _) = pending_prompt(&state, session_id).await;

        let error = handle(&state, answer(session_id, &prompt_id, "not-the-current-revision", None))
            .await
            .expect_err("a stale revision is refused");

        assert_eq!(error.code, ScopeErrorCode::RequestConflict, "{}", error.message);
        assert!(backend.writes.lock().expect("writes").is_empty(), "a refused answer typed nothing");
    }

    #[tokio::test]
    async fn a_pane_no_prompt_reader_knows_cannot_be_answered() {
        let session_id = "agent-control-answer-shell";
        let (backend, state) = pane(session_id, "$ ");

        let error = handle(&state, answer(session_id, "any", "any", None)).await.expect_err("no reader knows a shell");

        assert_eq!(error.code, ScopeErrorCode::Unsupported, "{}", error.message);
        assert!(backend.writes.lock().expect("writes").is_empty(), "nothing was typed");
    }

    #[tokio::test]
    async fn stop_presses_the_interrupt_key_and_never_signals() {
        let session_id = "agent-control-stop";
        let (backend, state) = pane(session_id, CLAUDE_APPROVAL);

        handle(&state, AgentControlOp::Stop { session_id: session_id.to_string(), request_id: None })
            .await
            .expect("stop is accepted");

        assert_eq!(backend.writes.lock().expect("writes").clone(), vec![b"\x1b".to_vec()]);
        assert!(backend.signals.lock().expect("signals").is_empty(), "stop must never signal the process");
        assert!(backend.list_sessions().await.contains(&session_id.to_string()));
    }

    #[tokio::test]
    async fn a_person_who_just_typed_blocks_supervisor_input() {
        let session_id = "agent-control-human-typing";
        let (backend, state) = pane(session_id, "$ ");
        reference_chat_runtime()
            .input
            .note_raw_input(session_id, reference_input::SystemReferenceClock.now_ms());

        let error = handle(
            &state,
            AgentControlOp::Send {
                session_id: session_id.to_string(),
                text: "echo hi".into(),
                request_id: Some("human-1".into()),
            },
        )
        .await
        .expect_err("a recent keystroke is a conflict");

        assert_eq!(
            error.code,
            ScopeErrorCode::ControlConflict,
            "{}",
            error.message
        );
        assert_eq!(
            backend
                .write_attempts
                .load(std::sync::atomic::Ordering::SeqCst),
            0
        );
        assert!(backend.writes.lock().expect("writes").is_empty());
    }

    #[tokio::test]
    async fn an_outcome_unknown_send_is_not_retried() {
        let session_id = "agent-control-outcome-unknown";
        let (backend, state) = pane(session_id, "$ ");
        backend
            .fail_writes
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let op = AgentControlOp::Send {
            session_id: session_id.to_string(),
            text: "echo hi".into(),
            request_id: Some("unknown-1".into()),
        };

        let first = handle(&state, op.clone())
            .await
            .expect_err("the write failed");
        assert_eq!(
            first.code,
            ScopeErrorCode::OperationOutcomeUnknown,
            "{}",
            first.message
        );
        let attempts = backend
            .write_attempts
            .load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(attempts, 1);

        let second = handle(&state, op)
            .await
            .expect_err("the same id is not typed again");
        assert_eq!(
            second.code,
            ScopeErrorCode::OperationOutcomeUnknown,
            "{}",
            second.message
        );
        assert_eq!(
            backend
                .write_attempts
                .load(std::sync::atomic::Ordering::SeqCst),
            attempts,
            "outcome-unknown is held and never replayed"
        );
    }

    #[tokio::test]
    async fn a_revoked_authorizer_types_nothing() {
        let session_id = "agent-control-revoked-midway";
        let (backend, state) = pane(session_id, "$ ");
        let authorize = Arc::new(|| {
            Err(reference_chat_scope(
                ScopeErrorCode::Unauthorized,
                "the caller's device is no longer authorized",
            ))
        });

        let error = handle_as(
            &state,
            AgentControlOp::Send {
                session_id: session_id.to_string(),
                text: "echo hi".into(),
                request_id: Some("revoked-1".into()),
            },
            "dev-revoked",
            Some(authorize),
        )
        .await
        .expect_err("a revoke before the first byte types nothing");

        assert_eq!(
            error.code,
            ScopeErrorCode::Unauthorized,
            "{}",
            error.message
        );
        assert!(backend.writes.lock().expect("writes").is_empty());
        let audit = state.agent_control_audit.recent();
        let record = audit.last().expect("the refusal is audited");
        assert_eq!(record.device_id, "dev-revoked");
        assert_eq!(record.op, "send");
        assert_eq!(record.code, "UNAUTHORIZED");
    }

    #[tokio::test]
    async fn events_and_wait_replay_a_gap_and_a_matching_reading() {
        let session_id = "agent-control-gap";
        let (_backend, state) = pane(session_id, "$ ");
        for index in 0..=events::AGENT_EVENT_RETAIN {
            state.agent_events.publish(events::ObservationDraft {
                session_id: session_id.to_string(),
                state: if index % 2 == 0 {
                    AgentControlState::Working
                } else {
                    AgentControlState::Idle
                },
                state_source: AgentStateSource::Screen,
                tier: AgentTier::Screen,
                prompt_id: None,
                kind: None,
                screen_revision: Some(format!("rev-{index}")),
                workspace_id: None,
            });
        }

        let gapped = handle(&state, AgentControlOp::Events { after: 0 })
            .await
            .expect("a fallen cursor is still a successful replay");
        assert_eq!(gapped["gap"], true, "{gapped}");
        assert!(gapped["snapshot"].as_array().unwrap().len() >= 1);

        let waited = handle(
            &state,
            AgentControlOp::Wait {
                session_id: session_id.to_string(),
                until: "any".into(),
                after_revision: None,
                after: 0,
                timeout_ms: 1_000,
            },
        )
        .await
        .expect("wait reports the gap instead of blocking");
        assert_eq!(waited["gap"], true, "{waited}");

        let latest = state.agent_events.latest(session_id).unwrap();
        let tail = handle(
            &state,
            AgentControlOp::Events {
                after: latest.sequence.saturating_sub(1),
            },
        )
        .await
        .expect("a cursor inside the tail replays");
        assert_eq!(tail["gap"], false, "{tail}");
        assert!(!tail["events"].as_array().unwrap().is_empty());
    }
}
