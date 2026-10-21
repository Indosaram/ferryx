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
use crate::agent_control::{derive_status, AgentControlOp, AgentControlState, AgentStateSource, AgentTier, ProviderReport};
use crate::scoped_contracts::{Epoch, ScopeError, ScopeErrorCode, TargetRef};
use futures_util::stream::{self, StreamExt as _};

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

/// Run one operation for the machine owner.
pub(crate) async fn handle(state: &Arc<RemoteGatewayState>, op: AgentControlOp) -> Result<serde_json::Value, ScopeError> {
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
            let state = Arc::clone(state);
            let transaction = tokio::spawn(async move {
                let payload = reference_types::ReferenceSubmitPayload {
                    text,
                    attachment_ids: Vec::new(),
                    origin: reference_types::ReferenceSubmitOrigin::Chat,
                };
                let paste_session = session_id.clone();
                let authorize = owner_authorized();
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
            let observed = observe(state, &session_id).await?;
            let Some(agent) = registry_agent(&observed) else {
                return Err(reference_chat_scope(
                    ScopeErrorCode::Unsupported,
                    "no prompt reader knows the agent in this pane, so a prompt cannot be answered",
                ));
            };
            let target = control_target(state, &session_id);
            let request_id = request_id_or_new(request_id);
            let state = Arc::clone(state);
            let transaction = tokio::spawn(async move {
                let identity = reference_chat_identity(&state, &target, Some(&agent)).await;
                let prompt_session = reference_chat_prompt_session(&state, &identity).await;
                let payload = reference_types::ReferencePromptAnswerPayload { prompt_id, screen_revision, answer };
                let authorize = owner_authorized();
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
            let target = control_target(state, &session_id);
            let request_id = request_id_or_new(request_id);
            let payload = reference_types::ReferenceStopPayload {
                capability: reference_types::ReferenceStopCapability::ProviderInterrupt,
            };
            let authorize = owner_authorized();
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
    }
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
        fn write_input<'a>(&'a self, _session_id: &'a str, data: &'a [u8]) -> futures_util::future::BoxFuture<'a, Result<(), String>> {
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
}
