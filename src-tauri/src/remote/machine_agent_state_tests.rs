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
        // Agent control is off on a fresh host; these round trips drive it, so the owner turns it on.
        host_state.agent_control_policy.set_enabled(true);
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
        let sessions = self.host.terminal_service().list_sessions();
        for session_id in sessions {
            match self.host.terminal_service().close_session(&session_id).await {
                Ok(()) | Err(crate::terminal::PtyError::SessionNotFound(_)) => {}
                Err(err) => panic!("fixture session close failed for {session_id}: {err:?}"),
            }
        }
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
                launcher: None,
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
                launcher: None,
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
                launcher: None,
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
                launcher: None,
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

        let close_req = m::CloseSessionRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            daemon_epoch: target.daemon_epoch,
        };
        let close_res = machine_client
            .execute(
                &fixture.paired_service,
                OperationRequest {
                    host_id: host_view.host_id.clone(),
                    generation: host_view.generation,
                    operation: Operation::CloseSession {
                        session_id: target.session_id.clone(),
                        request: close_req,
                    },
                },
            )
            .await
            .map_err(|e| anyhow::anyhow!("CloseSession error: {:?}", e))?;
        assert!(matches!(close_res.result, OperationResult::CloseSession(())));
        proxy.detach().await.map_err(|e| anyhow::anyhow!("detach error: {:?}", e))?;
        Ok::<_, anyhow::Error>(())
    }
    .await;

    fixture.close().await;
    root.close().unwrap();
    outcome.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn loopback_relay_round_trips_agent_control_list_send_and_read() {
    let root = tempfile::tempdir().unwrap();
    let host_root = root.path().join("remote-workspace");
    std::fs::create_dir_all(&host_root).unwrap();
    let host_root = std::fs::canonicalize(&host_root).unwrap();

    let fixture = Fixture::new(root.path()).await.unwrap();
    let outcome = async {
        let host_view = fixture.pair().await?;
        let workspace_id = register_workspace(&fixture.host_state, &host_root).await?;
        let machine_client = MachineClient::new();
        let created = machine_client
            .execute(
                &fixture.paired_service,
                OperationRequest {
                    host_id: host_view.host_id.clone(),
                    generation: host_view.generation,
                    operation: Operation::CreateSession {
                        request: m::CreateSessionRequest {
                            request_id: uuid::Uuid::new_v4().to_string(),
                            workspace_id,
                            worktree: None,
                            cols: 80,
                            rows: 24,
                            inherit_from_session_id: None,
                            cwd_relative: None,
                            startup: m::Startup::Shell,
                        },
                    },
                },
            )
            .await
            .map_err(|error| anyhow::anyhow!("create session: {error:?}"))?;
        let session = match created.result {
            OperationResult::CreateSession(session) => session,
            other => anyhow::bail!("expected a session, got {other:?}"),
        };

        let listed = machine_client
            .execute(
                &fixture.paired_service,
                OperationRequest {
                    host_id: host_view.host_id.clone(),
                    generation: host_view.generation,
                    operation: Operation::AgentControl {
                        op: crate::agent_control::AgentControlOp::List,
                        fence: None,
                    },
                },
            )
            .await
            .map_err(|error| anyhow::anyhow!("agent list: {error:?}"))?;
        let listed = match listed.result {
            OperationResult::AgentControl(value) => value,
            other => anyhow::bail!("expected agent list, got {other:?}"),
        };
        let sessions = listed["sessions"].as_array().expect("sessions");
        assert!(
            sessions
                .iter()
                .any(|entry| entry["sessionId"] == session.target.session_id),
            "the created pane is visible over agent control: {listed}"
        );

        let sent = machine_client
            .execute(
                &fixture.paired_service,
                OperationRequest {
                    host_id: host_view.host_id.clone(),
                    generation: host_view.generation,
                    operation: Operation::AgentControl {
                        op: crate::agent_control::AgentControlOp::Send {
                            session_id: session.target.session_id.clone(),
                            text: "echo ferryx-agent-control".into(),
                            request_id: Some("relay-agent-send-1".into()),
                        },
                        fence: None,
                    },
                },
            )
            .await
            .map_err(|error| anyhow::anyhow!("agent send: {error:?}"))?;
        match sent.result {
            OperationResult::AgentControl(value) => {
                assert_eq!(value["receipt"]["stage"], "accepted", "{value}");
            }
            other => anyhow::bail!("expected agent send, got {other:?}"),
        }

        let read = machine_client
            .execute(
                &fixture.paired_service,
                OperationRequest {
                    host_id: host_view.host_id.clone(),
                    generation: host_view.generation,
                    operation: Operation::AgentControl {
                        op: crate::agent_control::AgentControlOp::Read {
                            session_id: session.target.session_id.clone(),
                        },
                        fence: None,
                    },
                },
            )
            .await
            .map_err(|error| anyhow::anyhow!("agent read: {error:?}"))?;
        let read = match read.result {
            OperationResult::AgentControl(value) => value,
            other => anyhow::bail!("expected agent read, got {other:?}"),
        };
        assert_eq!(read["session"]["sessionId"], session.target.session_id);

        let audit = fixture.host_state.agent_control_audit.recent();
        assert!(
            audit
                .iter()
                .any(|record| record.op == "send" && record.device_id != "owner"),
            "the remote send is audited with the paired device id: {audit:?}"
        );
        Ok::<_, anyhow::Error>(())
    }
    .await;

    fixture.close().await;
    outcome.unwrap();
}

/// G1. One list across the loopback host, a host that fails, and a host that hangs.
/// The hung host returns the seeded cache and is marked stale. The failure stays an entry.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn loopback_fleet_lists_a_live_host_a_failure_and_a_stale_cache() {
    let root = tempfile::tempdir().unwrap();
    let host_root = root.path().join("remote-workspace");
    std::fs::create_dir_all(&host_root).unwrap();
    let host_root = std::fs::canonicalize(&host_root).unwrap();
    let fixture = Fixture::new(root.path()).await.unwrap();
    let outcome = async {
        let host_view = fixture.pair().await?;
        let workspace_id = register_workspace(&fixture.host_state, &host_root).await?;
        let machine_client = MachineClient::new();
        let created = machine_client
            .execute(
                &fixture.paired_service,
                OperationRequest {
                    host_id: host_view.host_id.clone(),
                    generation: host_view.generation,
                    operation: Operation::CreateSession {
                        request: m::CreateSessionRequest {
                            request_id: uuid::Uuid::new_v4().to_string(),
                            workspace_id,
                            worktree: None,
                            cols: 80,
                            rows: 24,
                            inherit_from_session_id: None,
                            cwd_relative: None,
                            startup: m::Startup::Shell,
                        },
                    },
                },
            )
            .await
            .map_err(|error| anyhow::anyhow!("create session: {error:?}"))?;
        let session = match created.result {
            OperationResult::CreateSession(session) => session,
            other => anyhow::bail!("expected a session, got {other:?}"),
        };
        let control = LiveAndFakeHosts {
            live_host: host_view.host_id.clone(),
            generation: host_view.generation,
            client: machine_client,
            service: &fixture.paired_service,
        };
        let mut cache = std::collections::HashMap::new();
        cache.insert("hung".into(), vec![serde_json::json!({"sessionId": "cached-before-hang"})]);
        let entries = ferryx_hub::fleet::list_hosts(
            &control,
            &[host_view.host_id.clone(), "missing-host".into(), "hung".into()],
            std::time::Duration::from_secs(5),
            &mut cache,
        )
        .await;
        assert!(
            entries[0].sessions.iter().any(|entry| entry["sessionId"] == session.target.session_id),
            "live host lists the pane: {entries:?}"
        );
        assert!(entries[0].error.is_none());
        assert!(!entries[0].stale);
        assert!(entries[1].error.as_ref().unwrap().contains("missing-host"));
        assert!(!entries[1].stale);
        assert!(entries[2].stale, "a hung host serves the cache");
        assert_eq!(entries[2].sessions[0]["sessionId"], "cached-before-hang");
        Ok::<_, anyhow::Error>(())
    }
    .await;
    fixture.close().await;
    outcome.unwrap();
}

struct LiveAndFakeHosts<'a> {
    live_host: String,
    generation: crate::scoped_contracts::Epoch,
    client: MachineClient,
    service: &'a crate::paired_host::service::PairedHostService,
}

#[async_trait::async_trait]
impl ferryx_hub::agent_control::AgentControl for LiveAndFakeHosts<'_> {
    async fn execute(
        &self,
        host_id: Option<&str>,
        op: ferryx_hub::agent_control::AgentOp,
    ) -> Result<serde_json::Value, ferryx_hub::HubError> {
        let host = host_id.unwrap_or("local");
        if host == "hung" {
            std::future::pending::<()>().await;
        }
        if host != self.live_host {
            return Err(ferryx_hub::HubError::message(format!("host {host} failed")));
        }
        let request = crate::hub_runtime::to_agent_control_op(&op).map_err(ferryx_hub::HubError::message)?;
        let response = self
            .client
            .execute(
                self.service,
                OperationRequest {
                    host_id: self.live_host.clone(),
                    generation: self.generation,
                    operation: Operation::AgentControl { op: request, fence: None },
                },
            )
            .await
            .map_err(|error| ferryx_hub::HubError::message(format!("{error:?}")))?;
        match response.result {
            OperationResult::AgentControl(value) => Ok(value),
            other => Err(ferryx_hub::HubError::message(format!("unexpected {other:?}"))),
        }
    }
}
