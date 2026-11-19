//! Exercise the bridge over its actual daemon socket and the real replay handler. The socket
//! peer substitutes only for the daemon accept loop, not for dispatch or epoch admission.
use super::*;
use crate::agent_control::events::{AgentEventBus, JournalBatch, ObservationDraft};
use crate::agent_control::{AgentControlState, AgentStateSource, AgentTier};
use crate::daemon::protocol::DAEMON_PROTOCOL_VERSION;
use crate::remote::machine_operation_journal::MachineOperationJournal;
use crate::remote::RemoteGatewayState;
use crate::terminal::{PtyManager, TerminalOutputHub, TerminalService};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

fn reading(revision: &str) -> ObservationDraft {
    ObservationDraft {
        session_id: "s1".into(),
        state: AgentControlState::Working,
        state_source: AgentStateSource::Screen,
        tier: AgentTier::Screen,
        prompt_id: None,
        kind: None,
        screen_revision: Some(revision.into()),
        workspace_id: None,
    }
}

#[tokio::test]
async fn execute_gaps_a_refused_durable_epoch_but_replays_the_retained_current_tail() {
    let dir = tempfile::tempdir().expect("temporary journal and socket");
    let path = dir.path().join("journal.json");
    let previous = AgentEventBus::new(8);
    let observations = (0..5)
        .map(|n| (previous.instance().to_owned(), previous.publish(reading(&format!("old-{n}")))))
        .collect();
    let journal = MachineOperationJournal::open(path.clone());
    journal.record_journal(JournalBatch { stamps: Default::default(), observations })
        .expect("persist foreign epoch");
    drop(journal);
    let history = MachineOperationJournal::open(path).observation_history()
        .expect("reopen foreign epoch");
    let foreign_head = history.head;
    let terminal = Arc::new(TerminalService::new(
        Arc::new(PtyManager::new()),
        Arc::new(TerminalOutputHub::default()),
    ));
    let state = Arc::new(RemoteGatewayState::new(
        terminal,
        crate::worktree::registry::WorkspaceRegistry::new(),
    ));
    // Warm before the stored epoch is discovered: stale cursor 1 is in range, so the old
    // out-of-range guard cannot make this test pass accidentally.
    let warm = state.agent_events.publish(reading("warm"));
    assert_eq!(warm.sequence, 1);
    assert!(!state.agent_events.adopt_history(history));
    let retained = state.agent_events.publish(reading("current-1"));
    let newest = state.agent_events.publish(reading("current-2"));
    assert!(retained.sequence > foreign_head);

    let socket = dir.path().join("replay.sock");
    let listener = tokio::net::UnixListener::bind(&socket).expect("bind daemon peer");
    let peer = async {
        let (stream, _) = listener.accept().await.expect("bridge connects");
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        let mut line = String::new();
        reader.read_line(&mut line).await.expect("handshake");
        assert!(matches!(serde_json::from_str::<DaemonRequest>(&line).expect("request"),
            DaemonRequest::Handshake { .. }));
        writer.write_all(format!(
            "{{\"type\":\"handshakeOk\",\"version\":{DAEMON_PROTOCOL_VERSION},\"pid\":1,\"epoch\":1}}\n"
        ).as_bytes()).await.expect("handshake reply");
        for _ in 0..2 {
            line.clear();
            reader.read_line(&mut line).await.expect("agent request");
            let DaemonRequest::AgentControl {
                request, host_id, supervisor_fence, supervisor_account,
            } = serde_json::from_str(&line).expect("agent-control wire request") else {
                panic!("bridge must dispatch agent control");
            };
            assert!(host_id.is_none());
            let fence = crate::agent_control::fence::SupervisorFence::from_wire(
                supervisor_fence, supervisor_account,
            ).expect("valid wire fence");
            let response = crate::remote::server::agent_control_service::handle_fenced(
                &state, request, fence,
            ).await;
            let mut reply = serde_json::to_vec(&response).expect("serialize actual handler reply");
            reply.push(b'\n');
            writer.write_all(&reply).await.expect("handler reply");
        }
    };
    let bridge = DaemonBridge { client: DaemonClient::new_with_socket(socket), gateway: None };
    let action = async {
        let stale = bridge.execute(None, AgentOp::Events { instance: Some("foreign-run".into()), after: warm.sequence })
            .await.expect("a refused epoch is a successful gap response");
        // Deleting start_above's warm-epoch boundary (or its adopt_history call) changes this
        // actual execute result to gap=false: the foreign cursor would silently read this run.
        assert_eq!(stale["gap"], true, "execute must gap a foreign in-range cursor");
        assert_eq!(stale["snapshot"][0]["sequence"], newest.sequence);
        let current = bridge.execute(None, AgentOp::Events { instance: Some(state.agent_events.instance().to_owned()), after: retained.sequence })
            .await.expect("retained current cursor");
        assert_eq!(current["gap"], false, "execute must not gap every cursor");
        assert_eq!(current["events"].as_array().expect("events").len(), 1);
        assert_eq!(current["events"][0]["sequence"], newest.sequence);
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(peer, action);
    }).await.expect("bounded socket exchange without sleeps or polling");
}

#[tokio::test]
async fn execute_replays_persisted_observations_after_restart_unfenced() {
    let dir = tempfile::tempdir().expect("isolated gateway stores");
    let auth_path = dir.path().join("remote-auth.json");
    let journal_path = dir.path().join("machine-operations.v1.json");
    let terminal = Arc::new(TerminalService::new(
        Arc::new(PtyManager::new()),
        Arc::new(TerminalOutputHub::default()),
    ));
    let gateway = Arc::new(RemoteGatewayState::new_with_paths(
        terminal,
        crate::worktree::registry::WorkspaceRegistry::new(),
        None,
        Some(auth_path.clone()),
    ));
    let current = crate::agent_control::fence::SupervisorFence {
        account: "account-1".into(),
        fence: 12,
    };
    gateway.supervisor_fence.admit(&current).await.expect("persist current fence");
    let previous = AgentEventBus::new(8);
    let prior = previous.publish(reading("before-restart"));
    MachineOperationJournal::open(journal_path.clone())
        .record_journal(JournalBatch {
            stamps: Default::default(),
            observations: vec![(previous.instance().to_owned(), prior.clone())],
        })
        .expect("persist pre-restart observation");
    drop(gateway);

    // Reopening with the same auth store models a daemon restart: the non-empty fence must
    // come from disk, not an in-process ratchet shared with the first gateway.
    let restarted = Arc::new(RemoteGatewayState::new_with_paths(
        Arc::new(TerminalService::new(
            Arc::new(PtyManager::new()),
            Arc::new(TerminalOutputHub::default()),
        )),
        crate::worktree::registry::WorkspaceRegistry::new(),
        None,
        Some(auth_path),
    ));
    assert_eq!(restarted.supervisor_fence.max_for("account-1").await, 12);
    let history = MachineOperationJournal::open(journal_path)
        .observation_history()
        .expect("load persisted observation history");
    assert!(restarted.agent_events.adopt_history(history), "restart adopts prior epoch");

    let socket = dir.path().join("replay-after-restart.sock");
    let listener = tokio::net::UnixListener::bind(&socket).expect("bind daemon peer");
    let peer = async {
        let (stream, _) = listener.accept().await.expect("bridge connects");
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        let mut line = String::new();
        reader.read_line(&mut line).await.expect("handshake");
        assert!(matches!(serde_json::from_str::<DaemonRequest>(&line).expect("request"),
            DaemonRequest::Handshake { .. }));
        writer.write_all(format!(
            "{{\"type\":\"handshakeOk\",\"version\":{DAEMON_PROTOCOL_VERSION},\"pid\":1,\"epoch\":1}}\n"
        ).as_bytes()).await.expect("handshake reply");
        for request_index in 0..2 {
            line.clear();
            reader.read_line(&mut line).await.expect("replay request");
            let DaemonRequest::AgentControl { request, supervisor_fence, supervisor_account, .. } =
                serde_json::from_str(&line).expect("agent-control request") else {
                    panic!("bridge must dispatch agent control");
                };
            if request_index == 0 {
                assert!(matches!(request, AgentControlOp::Events { instance: None, after } if after == prior.sequence - 1));
            } else {
                assert!(matches!(request, AgentControlOp::Events { instance: Some(ref instance), after } if instance == previous.instance() && after == prior.sequence - 1));
            }
            assert_eq!(supervisor_fence, None, "read-only replay is intentionally unfenced");
            assert_eq!(supervisor_account, None, "read-only replay is intentionally unfenced");
            let response = crate::remote::server::agent_control_service::handle_fenced(
                &restarted, request, None,
            ).await;
            assert!(matches!(response, DaemonResponse::AgentControlOk { .. }));
            let mut reply = serde_json::to_vec(&response).expect("serialize actual handler reply");
            reply.push(b'\n');
            writer.write_all(&reply).await.expect("replay response");
        }
    };
    let bridge = DaemonBridge { client: DaemonClient::new_with_socket(socket), gateway: None };
    let action = async {
        let missing_epoch = bridge
            .execute(None, AgentOp::Events { instance: None, after: prior.sequence - 1 })
            .await
            .expect("an epoch-less replay returns a recovery gap");
        assert_eq!(missing_epoch["gap"], true, "a numeric cursor alone is never comparable");
        let result = bridge.execute(None, AgentOp::Events { instance: Some(previous.instance().to_owned()), after: prior.sequence - 1 })
            .await.expect("the old cursor returns a recovery gap");
        assert_eq!(result["gap"], true, "a pre-restart cursor cannot replay after restart");
        assert_eq!(result["snapshot"].as_array().expect("retained snapshot").len(), 1);
        assert_eq!(result["snapshot"][0]["sequence"], prior.sequence);
        assert_eq!(result["snapshot"][0]["screenRevision"], "before-restart");
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(peer, action);
    }).await.expect("bounded socket exchange without sleeps or polling");
    assert_eq!(restarted.supervisor_fence.max_for("account-1").await, 12);
}
