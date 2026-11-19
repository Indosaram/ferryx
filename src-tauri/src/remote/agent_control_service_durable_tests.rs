struct GatewayControl(Arc<RemoteGatewayState>, &'static crate::remote::server::ReferenceChatRuntime);

#[async_trait::async_trait]
impl ferryx_hub::agent_control::AgentControl for GatewayControl {
    async fn execute(
        &self,
        _host_id: Option<&str>,
        op: ferryx_hub::agent_control::AgentOp,
    ) -> ferryx_hub::error::Result<serde_json::Value> {
        let op = crate::hub_runtime::to_agent_control_op(&op)
            .map_err(ferryx_hub::error::HubError::message)?;
        dispatch_with_runtime(&self.0, op, None, self.1)
            .await
            .map_err(|error| {
                ferryx_hub::error::HubError::message(format!(
                    "{} {}",
                    control_wire_code(&error),
                    error.message
                ))
            })
    }
}

#[tokio::test]
async fn a_retried_answer_after_a_restart_replays_under_a_new_runtime_and_a_revoked_retry_is_refused() {
    static FIRST: std::sync::OnceLock<crate::remote::server::ReferenceChatRuntime> = std::sync::OnceLock::new();
    static SECOND: std::sync::OnceLock<crate::remote::server::ReferenceChatRuntime> = std::sync::OnceLock::new();
    let first = FIRST.get_or_init(crate::remote::server::ReferenceChatRuntime::default);
    let second = SECOND.get_or_init(crate::remote::server::ReferenceChatRuntime::default);

    let root = tempfile::tempdir().expect("journal root");
    let history = tempfile::tempdir().expect("history home");
    let session_id = "agent-control-durable-replay-runtime";
    let request_id = "the-one-request-id";

    let (typed, state) = durable_pane_at(root.path(), session_id, CLAUDE_APPROVAL).await;
    *state.agent_history_home.write() = Some(history.path().to_path_buf());
    let read = dispatch_with_runtime(
        &state,
        AgentControlOp::Read { session_id: session_id.into() },
        None,
        first,
    )
    .await
    .expect("the approval screen is readable");
    let prompt_id = read["session"]["pending"]["id"].as_str().expect("a pending card").to_string();
    let revision = read["session"]["screenRevision"].as_str().expect("a screen revision").to_string();

    let accepted = dispatch_with_runtime(
        &state,
        answer(session_id, &prompt_id, &revision, Some(request_id)),
        None,
        first,
    )
    .await
    .expect("the card is accepted and typed");
    assert_eq!(accepted["receipt"]["stage"], "accepted", "{accepted}");
    assert_eq!(accepted["receipt"]["requestId"], request_id, "{accepted}");
    assert!(!typed.writes.lock().expect("writes").is_empty(), "the accepted answer typed its keys");
    assert!(
        recorded_card(&state, &prompt_id).resolution.is_some(),
        "the journal holds what the first delivery produced"
    );
    drop(state);

    let (restarted, next) = durable_pane_at(root.path(), session_id, CLAUDE_TURN).await;
    *next.agent_history_home.write() = Some(history.path().to_path_buf());
    let moved = dispatch_with_runtime(
        &next,
        AgentControlOp::Read { session_id: session_id.into() },
        None,
        second,
    )
    .await
    .expect("the restarted pane is readable");
    assert!(moved["session"]["pending"].is_null(), "the screen moved past the card: {moved}");

    let revoked: Arc<dyn Fn() -> Result<(), ScopeError> + Send + Sync> = Arc::new(|| {
        Err(reference_chat_scope(
            ScopeErrorCode::Unauthorized,
            "the caller's device is no longer authorized",
        ))
    });
    let refusal = dispatch_with_runtime(
        &next,
        answer(session_id, &prompt_id, &revision, Some(request_id)),
        Some(revoked),
        second,
    )
    .await
    .expect_err("a revoked caller is refused, not handed the durable receipt");
    assert_eq!(refusal.code, ScopeErrorCode::Unauthorized, "{}", refusal.message);

    let hub_pool = ferryx_hub::storage::memory().await.expect("the hub pool");
    let context = ferryx_hub::mcp::McpContext {
        control: Arc::new(GatewayControl(Arc::clone(&next), second)),
        launcher: Arc::new(ferryx_hub::agent_control::FixedLauncher::new(session_id)),
        pool: hub_pool,
        meta_session_id: "meta".into(),
        tracker_token: "tracker".into(),
        owner_route: None,
    };
    let retry = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "agents.answer",
            "arguments": {
                "sessionId": session_id,
                "promptId": prompt_id.clone(),
                "screenRevision": revision.clone(),
                "answer": {"optionIndex": 0},
                "requestId": request_id,
            },
        },
    });
    let reply = ferryx_hub::mcp::dispatch(&context, &retry).await;
    assert!(reply.get("error").is_none(), "the JSON-RPC handler accepted the tool call: {reply}");
    let replayed: serde_json::Value =
        serde_json::from_str(reply["result"]["content"][0]["text"].as_str().expect("a text result"))
            .expect("the tool result is the daemon's JSON");
    assert_eq!(
        replayed, accepted,
        "the production JSON-RPC handler and the journal replay the whole receipt; stdio framing is not under test"
    );

    let settled = recorded_card(&next, &prompt_id);
    assert!(
        settled.resolution.is_some_and(|resolution| resolution.receipt.is_some()),
        "the refusal left the durable receipt exactly where the first delivery put it"
    );
    assert_eq!(restarted.write_attempts.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(restarted.writes.lock().expect("writes").is_empty(), "a replay types nothing");
    assert!(restarted.signals.lock().expect("signals").is_empty(), "a replay signals nothing");
}

/// rs-input-durable-r1: dropping the generic dispatch caller at the first navigation key must not
/// cancel the rest of the plan, and the accepted receipt must survive a reopen of the journal.
#[tokio::test(start_paused = true)]
async fn a_dropped_generic_answer_caller_still_finishes_its_keys_and_keeps_the_receipt() {
    use notify::{RecursiveMode, Watcher};

    let session_id = "agent-control-durable-caller-left";
    let root = tempfile::tempdir().expect("temporary daemon root");
    let home = tempfile::tempdir().expect("isolated transcript root");
    // The daemon derives this journal from `config`'s parent, the same file `durable_pane_at`
    // reopens. The path is canonicalized because the macOS change notifier reports canonical
    // paths, and `/var` is a symlink to `/private/var` under a temporary directory.
    let journal_root = root.path().canonicalize().expect("journal root");
    let journal_path = journal_root.join("machine-operations.v1.json");
    let services = {
        let root = root.path().to_path_buf();
        crate::ipc::run_blocking(move || {
            let server = crate::daemon::server::DaemonServer::new_with_paths(
                Some(root.join("config")),
                Some(root.join("auth")),
            );
            Ok(server.remote_state().machine_services.clone().expect("daemon services"))
        })
        .await
        .expect("daemon journal")
    };
    let (backend, mut written) = pane_backend(session_id, CLAUDE_APPROVAL);
    let state = gateway_over(&backend, Some(services));
    *state.agent_history_home.write() = Some(home.path().to_path_buf());
    let (prompt_id, revision) = pending_prompt(&state, session_id).await;
    let runtime: &'static crate::remote::server::ReferenceChatRuntime =
        Box::leak(Box::new(crate::remote::server::ReferenceChatRuntime::default()));

    let op = AgentControlOp::Answer {
        session_id: session_id.to_string(),
        prompt_id: prompt_id.clone(),
        screen_revision: revision,
        answer: ReferencePromptAnswer { option_index: Some(3), ..Default::default() },
        request_id: Some("durable-detached-answer".to_string()),
    };

    let (landed, mut writes_landed) = tokio::sync::mpsc::unbounded_channel();
    let watched = journal_path.clone();
    let mut watcher = notify::recommended_watcher(move |event: Result<notify::Event, notify::Error>| {
        let Ok(event) = event else { return };
        let written = matches!(
            event.kind,
            notify::EventKind::Create(_) | notify::EventKind::Modify(_)
        );
        if written && event.paths.iter().any(|path| path == &watched) {
            let _ = landed.send(());
        }
    })
    .expect("journal watcher");
    watcher
        .watch(&journal_root, RecursiveMode::NonRecursive)
        .expect("watch the journal root");

    let mut dispatch = Box::pin(dispatch_with_runtime(&state, op, None, runtime));
    let first = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::select! {
            biased;
            result = &mut dispatch => panic!("the answer finished before its caller could leave: {result:?}"),
            key = written.recv() => key.expect("the first answer key"),
        }
    })
    .await
    .expect("the first answer key arrived");
    assert_eq!(first, b"\x1b[B", "the pane signalled its first navigation key");
    assert_eq!(
        backend.writes.lock().expect("writes").len(),
        1,
        "the caller left on the first key, before the rest of the plan was typed"
    );
    drop(dispatch);
    // The drop is behind us, so the remaining keys and the journal's real I/O run on real time;
    // leaving the clock paused would let bounded waits expire against the frozen clock instead.
    tokio::time::resume();

    let remaining = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let mut keys = Vec::new();
        for _ in 0..3 {
            keys.push(written.recv().await.expect("a remaining answer key"));
        }
        keys
    })
    .await
    .expect("the keys the card still needed arrive after the caller leaves");
    assert_eq!(
        remaining,
        vec![b"\x1b[B".to_vec(), b"\x1b[B".to_vec(), b"\r".to_vec()],
        "the remaining down keys and the Enter are delivered after the caller left"
    );
    assert_eq!(
        backend.writes.lock().expect("writes").clone(),
        vec![b"\x1b[B".to_vec(), b"\x1b[B".to_vec(), b"\x1b[B".to_vec(), b"\r".to_vec()],
        "the answer is typed exactly once, in order"
    );
    assert_eq!(
        backend.write_attempts.load(std::sync::atomic::Ordering::SeqCst),
        4,
        "no key was attempted twice"
    );

    // Each matching event is a write or rename of the journal itself, so its content is already
    // atomic and complete. The claim persisted before the first key, so an event whose reopened row
    // is still open is skipped; the loop stops on the write that settles this session's card.
    let mut settled = None;
    for _ in 0..8 {
        tokio::time::timeout(std::time::Duration::from_secs(10), writes_landed.recv())
            .await
            .expect("the receipt's journal write reached the disk")
            .expect("the journal write channel is open");
        let path = journal_path.clone();
        let card = prompt_id.clone();
        let read = tokio::task::spawn_blocking(move || {
            let journal =
                crate::remote::machine_operation_journal::MachineOperationJournal::open(path);
            crate::agent_control::pending_input::get(&journal, &card)
        })
        .await
        .expect("the journal read finished")
        .expect("the reopened journal is readable");
        settled = read.filter(|stored| !stored.is_open());
        if settled.is_some() {
            break;
        }
    }
    let stored = settled.expect("the accepted receipt is on disk after the reopen");
    assert_eq!(
        stored.attempt.as_ref().and_then(|attempt| attempt.outcome.as_deref()),
        Some("accepted"),
        "the attempt that typed the keys is the one the journal settled"
    );
    let resolution = stored.resolution.as_ref().expect("a settled card keeps its resolution");
    assert_eq!(resolution.outcome, "accepted");
    let receipt = resolution.receipt.as_ref().expect("the accepted resolution carries its receipt");
    assert_eq!(
        receipt.request_id, "durable-detached-answer",
        "the receipt certifies the request the dropped caller began"
    );
    assert_eq!(receipt.target.backend_session_id, session_id);
    assert_eq!(receipt.stage, crate::scoped_contracts::DeliveryStage::Accepted);
}
