use crate::daemon::{
    agent_state::AgentState,
    protocol::{AgentProviderSession, AgentProviderSessionKey, AgentStateOrigin},
    server::DaemonServer,
};
use crate::paired_host::{
    client::{MachineClient, Operation, OperationRequest, OperationResult},
    service::{PairRequest, PairedHostService, Secret},
};
use crate::remote::{
    auth::{DeviceAccessScope, DevicePermission},
    machine_protocol as m,
};
use crate::scoped_contracts::Epoch;
use crate::terminal::{
    output_hub::TerminalOutputHub,
    paired_daemon::{Descriptor, Proxy},
};
use std::{path::Path, sync::Arc, time::Duration};

struct TestSink {
    records: parking_lot::Mutex<Vec<AgentState>>,
}

impl TestSink {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            records: parking_lot::Mutex::new(Vec::new()),
        })
    }

    fn snapshots(&self) -> Vec<AgentState> {
        self.records.lock().clone()
    }
}

impl crate::terminal::remote::AgentStateSink for TestSink {
    fn accept(&self, state: AgentState) -> bool {
        self.records.lock().push(state);
        true
    }
}

struct Fixture {
    tasks: tokio::task::JoinSet<()>,
    host: Arc<DaemonServer>,
    host_state: Arc<crate::remote::state::RemoteGatewayState>,
    coordinator: crate::remote::relay_client::PairingCoordinator,
    origin: String,
    paired_service: PairedHostService,
}

impl Fixture {
    async fn new(root: &Path) -> anyhow::Result<Self> {
        let host_data = root.join("host");
        std::fs::create_dir_all(&host_data)?;
        let desktop_data = root.join("desktop");
        std::fs::create_dir_all(&desktop_data)?;

        let mut tasks = tokio::task::JoinSet::new();
        let relay_path = root.join("relay-keys.json");
        let relay = crate::ipc::run_blocking(move || {
            crate::remote::relay_server::RelayState::new_with_key_store(vec![], relay_path)
                .map_err(|error| crate::ipc::IpcError::internal(error.to_string()))
        })
        .await
        .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        let relay_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let origin = format!("http://{}", relay_listener.local_addr()?);
        tasks.spawn(async move {
            axum::serve(
                relay_listener,
                crate::remote::relay_server::relay_router(relay)
                    .into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .await
            .unwrap();
        });

        let host = crate::ipc::run_blocking({
            let host_data = host_data.clone();
            move || {
                Ok(Arc::new(DaemonServer::new_with_paths(
                    Some(host_data.join("config")),
                    Some(host_data.join("auth")),
                )))
            }
        })
        .await
        .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        let host_state = host.remote_state().clone();
        host.configure_gateway(crate::remote::state::RemoteGatewayConfig {
            mode: crate::remote::state::RemoteNetworkMode::Relay,
            port: 0,
            relay_url: Some(origin.clone()),
            ..Default::default()
        })
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
        let coordinator = host_state
            .relay_pairing
            .read()
            .as_ref()
            .map(|pairing| pairing.coordinator.clone())
            .ok_or_else(|| anyhow::anyhow!("relay coordinator not published"))?;

        let paired_service = PairedHostService::open_test_loopback(desktop_data);

        Ok(Self {
            tasks,
            host,
            host_state,
            coordinator,
            origin,
            paired_service,
        })
    }

    async fn pair(&self) -> anyhow::Result<crate::paired_host::inventory::HostView> {
        let issued = self
            .coordinator
            .generate_scoped_pairing(
                Duration::from_secs(60),
                DevicePermission::Control,
                DeviceAccessScope::Machine,
            )
            .await
            .map_err(anyhow::Error::msg)?;
        self.paired_service
            .pair(PairRequest {
                relay_origin: self.origin.clone(),
                pin: Secret(issued.pin),
                display_label: "test host".into(),
            })
            .await
            .map_err(|e| anyhow::anyhow!(e.code))
    }

    async fn close(mut self) {
        self.tasks.shutdown().await;
    }
}

async fn register_workspace(
    state: &Arc<crate::remote::state::RemoteGatewayState>,
    root: &Path,
) -> anyhow::Result<String> {
    let services = state.machine_services.as_ref().expect("machine services");
    let workspaces = Arc::clone(&services.workspaces);
    let root = root.to_string_lossy().into_owned();
    crate::ipc::run_blocking(move || {
        workspaces
            .register_machine(&root)
            .map_err(crate::ipc::IpcError::internal)
    })
    .await
    .map_err(|e| anyhow::anyhow!("{e:?}"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn machine_terminal_websocket_publishes_authoritative_agent_state_and_ignores_unrelated() {
    let root = tempfile::tempdir().unwrap();
    let host_root = root.path().join("remote-workspace");
    std::fs::create_dir_all(&host_root).unwrap();
    let host_root = std::fs::canonicalize(&host_root).unwrap();

    let fixture = Fixture::new(root.path()).await.unwrap();
    let outcome = async {
        let host_view = fixture
            .pair()
            .await
            .map_err(|e| anyhow::anyhow!("pairing failed: {e}"))?;
        let workspace_id = register_workspace(&fixture.host_state, &host_root)
            .await
            .map_err(|e| anyhow::anyhow!("register workspace failed: {e}"))?;

        let machine_client = MachineClient::new();

        let session_req = m::CreateSessionRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            workspace_id: workspace_id.clone(),
            worktree: None,
            cols: 80,
            rows: 24,
            inherit_from_session_id: None,
            cwd_relative: None,
            startup: m::Startup::Shell,
        };

        let op_res = machine_client
            .execute(
                &fixture.paired_service,
                OperationRequest {
                    host_id: host_view.host_id.clone(),
                    generation: host_view.generation,
                    operation: Operation::CreateSession {
                        request: session_req,
                    },
                },
            )
            .await
            .map_err(|e| anyhow::anyhow!("CreateSession error: {:?}", e))?;

        let session = match op_res.result {
            OperationResult::CreateSession(s) => s,
            other => panic!("expected CreateSession result, got: {:?}", other),
        };
        let target = session.target.clone();

        let host_session_service = fixture.host.session_service();

        let initial_agent_state = AgentState {
            session_id: target.session_id.clone(),
            state: "working".into(),
            agent: Some("omo".into()),
            provider_session: Some(AgentProviderSession {
                key: AgentProviderSessionKey::SessionId,
                id: "initial-provider-id-42".into(),
                transcript_path: None,
            }),
            detail: Some("analyzing codebase".into()),
            origin: AgentStateOrigin::Agent,
        };
        host_session_service.publish_agent_state_for_test(initial_agent_state);

        let unrelated_session_id = uuid::Uuid::new_v4().to_string();
        host_session_service.publish_agent_state_for_test(AgentState {
            session_id: unrelated_session_id.clone(),
            state: "done".into(),
            agent: Some("unrelated-agent".into()),
            provider_session: Some(AgentProviderSession {
                key: AgentProviderSessionKey::ConversationId,
                id: "unrelated-conv-id".into(),
                transcript_path: None,
            }),
            detail: None,
            origin: AgentStateOrigin::Agent,
        });

        let descriptor = Descriptor {
            host_id: host_view.host_id.clone(),
            generation: host_view.generation,
            target: target.clone(),
            after_sequence: None,
        };
        let hub = Arc::new(TerminalOutputHub::new(1024));
        let mut proxy = Proxy::new(descriptor, hub.clone())
            .map_err(|e| anyhow::anyhow!("Proxy::new failed: {:?}", e))?;

        let sink = TestSink::new();
        proxy.set_agent_sink(sink.clone());

        proxy
            .reattach(&machine_client, &fixture.paired_service)
            .await
            .map_err(|e| anyhow::anyhow!("proxy reattach failed: {:?}", e))?;

        let receive_initial = async {
            loop {
                if let Some(state) = sink.snapshots().into_iter().find(|s| s.state == "working") {
                    return Ok::<AgentState, anyhow::Error>(state);
                }
                proxy
                    .receive()
                    .await
                    .map_err(|e| anyhow::anyhow!("proxy receive error: {:?}", e))?;
            }
        };

        let captured_initial: AgentState = tokio::time::timeout(Duration::from_secs(10), receive_initial)
            .await
            .map_err(|_| anyhow::anyhow!("timeout waiting for initial agent state via websocket"))??;

        assert_eq!(captured_initial.session_id, proxy.id());
        assert_eq!(captured_initial.state, "working");
        assert_eq!(
            captured_initial.provider_session.as_ref().unwrap().id,
            "initial-provider-id-42"
        );
        assert_eq!(
            captured_initial.detail.as_deref(),
            Some("analyzing codebase")
        );

        host_session_service.publish_agent_state_for_test(AgentState {
            session_id: "another-unrelated-session".into(),
            state: "working".into(),
            agent: Some("other".into()),
            provider_session: Some(AgentProviderSession {
                key: AgentProviderSessionKey::SessionId,
                id: "unrelated-barrier-id".into(),
                transcript_path: None,
            }),
            detail: None,
            origin: AgentStateOrigin::Agent,
        });

        let rotated_agent_state = AgentState {
            session_id: target.session_id.clone(),
            state: "waiting".into(),
            agent: Some("omo".into()),
            provider_session: Some(AgentProviderSession {
                key: AgentProviderSessionKey::SessionId,
                id: "rotated-provider-id-99".into(),
                transcript_path: None,
            }),
            detail: Some("blocked on user confirmation".into()),
            origin: AgentStateOrigin::Agent,
        };
        host_session_service.publish_agent_state_for_test(rotated_agent_state);

        let receive_rotation = async {
            loop {
                if let Some(state) = sink.snapshots().into_iter().find(|s| s.state == "waiting") {
                    return Ok::<AgentState, anyhow::Error>(state);
                }
                proxy
                    .receive()
                    .await
                    .map_err(|e| anyhow::anyhow!("proxy receive error: {:?}", e))?;
            }
        };

        let captured_rotated: AgentState = tokio::time::timeout(Duration::from_secs(10), receive_rotation)
            .await
            .map_err(|_| anyhow::anyhow!("timeout waiting for rotated agent state via websocket"))??;

        assert_eq!(captured_rotated.session_id, proxy.id());
        assert_eq!(captured_rotated.state, "waiting");
        assert_eq!(
            captured_rotated.provider_session.as_ref().unwrap().id,
            "rotated-provider-id-99"
        );
        assert_eq!(
            captured_rotated.detail.as_deref(),
            Some("blocked on user confirmation")
        );

        let all_recorded = sink.snapshots();
        assert_eq!(all_recorded.len(), 2, "sink must only receive the 2 own states, got: {:?}", all_recorded);
        assert_eq!(
            all_recorded[0].provider_session.as_ref().unwrap().id,
            "initial-provider-id-42"
        );
        assert_eq!(
            all_recorded[1].provider_session.as_ref().unwrap().id,
            "rotated-provider-id-99"
        );

        proxy.detach().await.map_err(|e| anyhow::anyhow!("detach error: {:?}", e))?;
        Ok::<_, anyhow::Error>(())
    }
    .await;

    fixture.close().await;
    root.close().unwrap();
    outcome.unwrap();
}
