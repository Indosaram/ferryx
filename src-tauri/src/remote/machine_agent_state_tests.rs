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
        // The PIN-paired fixture host still uses the legacy plaintext relay proxy.
        relay.set_require_e2e(false);
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
        // The host's agent-stamp writer writes into `host/` after a batching window; it must be
        // finished before the caller removes the directory.
        let host_state = Arc::clone(&self.host_state);
        crate::ipc::run_blocking(move || {
            host_state.finish_agent_stamp_writer();
            Ok(())
        })
        .await
        .unwrap();
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
            outcome: None,
            kind: None,
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
            outcome: None,
            kind: None,
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
            outcome: None,
            kind: None,
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
            outcome: None,
            kind: None,
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

/// The HTTP gateway's stop for a paired computer's pane, through a real paired host over the
/// relay: the receipt names the paired host, the host typed the interrupt, and the gateway
/// side wrote exactly one audit line, under the calling device (not "owner").
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn gateway_stop_for_a_paired_session_ref_is_forwarded_and_audited_once_under_the_device() {
    let root = tempfile::tempdir().unwrap();
    let host_root = root.path().join("remote-workspace");
    std::fs::create_dir_all(&host_root).unwrap();
    let host_root = std::fs::canonicalize(&host_root).unwrap();

    let fixture = Fixture::new(root.path()).await.unwrap();
    let outcome = async {
        let host_view = fixture.pair().await?;
        let workspace_id = register_workspace(&fixture.host_state, &host_root).await?;
        let created = MachineClient::new()
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

        // The desktop's own gateway, sharing the paired-host inventory the fixture paired.
        let desktop_root = root.path().join("desktop-gateway");
        let mut desktop = DaemonServer::new_with_paths(
            Some(desktop_root.join("config")),
            Some(desktop_root.join("auth")),
        );
        desktop.set_paired_hosts_for_test(fixture.paired_service.clone());
        let state = Arc::clone(desktop.remote_state());
        state.agent_control_policy.set_enabled(true);
        let auth = &state.auth_manager;
        let pin = auth
            .create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine)
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        let (token, _) = auth
            .exchange_pairing_code(&pin, "phone")
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        let device = auth.validate_token(&token).map_err(|e| anyhow::anyhow!("{e:?}"))?;
        state.agent_control_policy.restrict_to([device.id.clone()]);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let router = crate::remote::server::create_remote_router(Arc::clone(&state));
        let server = tokio::spawn(async move { axum::serve(listener, router).await });

        let response = reqwest::Client::builder()
            .no_proxy()
            .build()?
            // A paired computer's id is a URL, so its ref is one percent-encoded path segment.
            .post(format!(
                "http://{addr}/api/v1/sessions/{}/stop",
                url::form_urlencoded::byte_serialize(
                    format!("{}:{}", host_view.host_id, session.target.session_id).as_bytes()
                )
                .collect::<String>()
            ))
            .bearer_auth(&token)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body("")
            .send()
            .await?;
        let status = response.status().as_u16();
        let body: serde_json::Value = response.json().await?;
        server.abort();
        anyhow::ensure!(status == 200, "{status}: {body}");
        assert_eq!(body["receipt"]["stage"], "accepted", "{body}");
        assert_eq!(body["receipt"]["target"]["hostId"], host_view.host_id, "the receipt names the paired host: {body}");

        let audit = state.agent_control_audit.recent();
        assert_eq!(audit.len(), 1, "exactly one gateway-side audit line: {audit:?}");
        assert_eq!((audit[0].op.as_str(), audit[0].device_id.as_str(), audit[0].code.as_str()), ("stop", device.id.as_str(), "OK"));
        assert!(audit[0].request_id.is_some(), "a request id was minted before the forward");
        let host_stops: Vec<_> = fixture
            .host_state
            .agent_control_audit
            .recent()
            .into_iter()
            .filter(|record| record.op == "stop")
            .collect();
        assert_eq!(host_stops.len(), 1, "the paired host executed exactly one stop: {host_stops:?}");
        assert_eq!(
            host_stops[0].request_id, audit[0].request_id,
            "the id minted at the gateway is the id the host executed"
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

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn paired_machine_input_is_fenced_by_the_admitted_host_controller() {
    use futures_util::FutureExt;

    async fn assert_host_size(
        sessions: &crate::daemon::session_service::DaemonSessionService,
        id: &str,
        generation: u64,
        hub: &TerminalOutputHub,
        host_id: &str,
        expected: &[u8],
    ) {
        let (_, mut output) = hub.subscribe(host_id).unwrap();
        sessions.write_machine_input(id, generation, b"stty size\n").await.unwrap();
        tokio::time::timeout(Duration::from_secs(10), async {
            let mut bytes = Vec::new();
            loop {
                bytes.extend_from_slice(&output.recv().await.unwrap());
                if bytes.windows(expected.len()).any(|window| window == expected) { break; }
            }
        }).await.expect("host PTY must report the expected geometry");
    }

    async fn output_barrier(
        sessions: &crate::daemon::session_service::DaemonSessionService,
        id: &str,
        generation: u64,
        hub: &TerminalOutputHub,
        host_id: &str,
    ) {
        let (_, mut output) = hub.subscribe(host_id).unwrap();
        let marker = uuid::Uuid::new_v4().simple().to_string();
        let (left, right) = marker.split_at(16);
        sessions.write_machine_input(id, generation,
            format!("printf '%s%s\\n' '{left}' '{right}'\n").as_bytes()).await.unwrap();
        tokio::time::timeout(Duration::from_secs(10), async {
            let mut bytes = Vec::new();
            loop {
                bytes.extend_from_slice(&output.recv().await.unwrap());
                if bytes.windows(marker.len()).any(|window| window == marker.as_bytes()) {
                    break;
                }
            }
        }).await.expect("host shell must execute the accepted input");
    }

    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("input-workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let fixture = Fixture::new(root.path()).await.unwrap();
    let desktop = crate::remote::server::machine_input_fixture::Fixture::new().await;
    let outcome = std::panic::AssertUnwindSafe(async {
        let mut shell = portable_pty::CommandBuilder::new("/bin/sh");
        shell.args(["-s"]);
        shell.env_remove("ENV");
        shell.env_remove("BASH_ENV");
        fixture.host.session_service().set_test_shell_command(shell);
        let host = fixture.pair().await.unwrap();
        let workspace_id = register_workspace(&fixture.host_state, &workspace).await.unwrap();
        let client = MachineClient::new();
        let response = client.execute(&fixture.paired_service, OperationRequest {
            host_id: host.host_id.clone(),
            generation: host.generation,
            operation: Operation::CreateSession { request: m::CreateSessionRequest {
                request_id: uuid::Uuid::new_v4().to_string(), workspace_id,
                worktree: None, cols: 80, rows: 24, inherit_from_session_id: None,
                cwd_relative: None, startup: m::Startup::Shell,
            } },
        }).await.unwrap();
        let OperationResult::CreateSession(session) = response.result else {
            panic!("expected created host session");
        };
        let descriptor = Descriptor {
            host_id: host.host_id, generation: host.generation,
            target: session.target.clone(), after_sequence: None,
        };
        let terminal = desktop.owner.terminal_service();
        let sessions = &desktop.state.machine_services.as_ref().unwrap().sessions;
        let mut proxy = Proxy::new(descriptor.clone(), terminal.output_hub().clone()).unwrap();
        let id = proxy.id().to_owned();
        assert_eq!(terminal.paired().install(proxy).unwrap(), id);
        let unattached = sessions.machine_input_generation(&id);
        assert_eq!(unattached, 0);
        assert_eq!(terminal.paired().recovery_outcome(&id).unwrap().generation, None);
        terminal.paired().detach(&id).await.unwrap();

        proxy = Proxy::new(descriptor.clone(), terminal.output_hub().clone()).unwrap();
        proxy.reattach(&client, &fixture.paired_service).await.unwrap();
        let first = proxy.controller().unwrap().0;
        terminal.paired().install(proxy).unwrap();
        assert_eq!(terminal.paired().recovery_outcome(&id).unwrap().generation, Some(first));
        assert_eq!(sessions.machine_input_generation(&id), first);
        assert_ne!(first, 0);
        let lease = {
            let mut controllers = sessions.machine_controllers.lock().await;
            crate::daemon::session_service::DaemonSessionService::acquire_machine_controller(
                &mut controllers, &id, "machine-socket",
            ).unwrap()
        };
        let host_terminal = fixture.host.terminal_service();
        output_barrier(sessions, &id, first, host_terminal.output_hub(), &session.target.session_id).await;
        terminal.paired().detach(&id).await.unwrap();
        let mut proxy = Proxy::new(descriptor.clone(), terminal.output_hub().clone()).unwrap();
        proxy.reattach(&client, &fixture.paired_service).await.unwrap();
        let second = proxy.controller().unwrap().0;
        assert!(second > first);
        terminal.paired().install(proxy).unwrap();
        assert_eq!(terminal.paired().recovery_outcome(&id).unwrap().generation, Some(second));
        assert_eq!(sessions.machine_input_generation(&id), second);
        assert_ne!(second, 0);

        let forbidden = workspace.join("stale-input-executed");
        let stale = format!("printf stale > '{}'\n", forbidden.display());
        for generation in [unattached, first] {
            let error = sessions.write_machine_input(&id, generation, stale.as_bytes()).await.unwrap_err();
            assert!(error.contains("STALE_GENERATION"), "{error}");
        }
        output_barrier(sessions, &id, second, host_terminal.output_hub(), &session.target.session_id).await;
        assert!(!forbidden.exists());
        let controllers = sessions.machine_controllers.lock().await;
        assert_eq!(controllers[&id].generation, lease.generation);
        assert_eq!(controllers[&id].device, "machine-socket");
        assert!(!*lease.cancelled.borrow());
        drop(controllers);

        let source = desktop.create().await;
        sessions.retain_paired_metadata_for_test(source["target"]["sessionId"].as_str().unwrap(), &id);
        sessions.record_desktop_geometry(&id, 80, 24);
        drop(lease);
        let mut paired = source.clone();
        paired["target"]["sessionId"] = id.clone().into();
        {
            let mut controllers = sessions.machine_controllers.lock().await;
            controllers.remove(&id);
        }
        let mut phone = desktop.attach(&paired).await;
        let generation = sessions.machine_controllers.lock().await[&id].generation;
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::Message;
        phone.send(Message::Text(serde_json::json!({
            "type":"resize", "generation":generation.to_string(), "cols":110, "rows":40,
        }).to_string().into())).await.unwrap();
        phone.send(Message::Text(r#"{"type":"ping"}"#.into())).await.unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let Message::Text(text) = phone.next().await.unwrap().unwrap() {
                    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
                    assert_ne!(value["type"], "error", "{value}");
                    if value["type"] == "pong" { break; }
                }
            }
        }).await.unwrap();
        assert_host_size(sessions, &id, second, host_terminal.output_hub(), &session.target.session_id, b"40 110").await;
        let (entered, release, finished) = crate::remote::server::machine_teardown_probe::register(&format!("restore:{id}"));
        phone.send(Message::Close(None)).await.unwrap();
        tokio::time::timeout(Duration::from_secs(3), entered).await.unwrap().unwrap();
        terminal.paired().detach(&id).await.unwrap();
        let mut recovering = Proxy::new(descriptor.clone(), terminal.output_hub().clone()).unwrap();
        recovering.set_recovery(fixture.paired_service.clone());
        recovering.recovery_failed(&crate::paired_host::client::ClientError::local("PERMISSION_DENIED"));
        terminal.paired().install(recovering).unwrap();
        assert_eq!(sessions.resize_machine_input(&id, 0, 120, 50).await.unwrap_err(), "SESSION_INPUT_UNAVAILABLE");
        release.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(3), finished).await.unwrap().unwrap();
        let (blocked_tx, blocked_rx) = tokio::sync::oneshot::channel();
        let (resume_tx, resume_rx) = tokio::sync::oneshot::channel();
        terminal.paired().block_actor(&id, blocked_tx, resume_rx);
        tokio::time::timeout(Duration::from_secs(3), blocked_rx).await.unwrap().unwrap();
        let (mut cancelled_restore, enqueued) = terminal.paired().resize_with_enqueue_receipt(&id, 0, 200, 60);
        tokio::time::timeout(Duration::from_secs(3), async {
            tokio::select! {
                result = &mut cancelled_restore => panic!("blocked actor completed resize: {result:?}"),
                receipt = enqueued => receipt.expect("resize entered actor queue"),
            }
        }).await.unwrap();
        drop(cancelled_restore);
        resume_tx.send(()).unwrap();
        let (recovery_entered, recovery_release) = tokio::time::timeout(
            Duration::from_secs(3), terminal.paired().hold_next_recovery(&id),
        ).await.unwrap();
        let mut recovery = terminal.paired().subscribe_recovery_outcome(&id).unwrap();
        terminal.paired().get_or_rearm(descriptor, terminal.output_hub().clone(), fixture.paired_service.clone()).unwrap();
        tokio::time::timeout(Duration::from_secs(3), recovery_entered).await.unwrap().unwrap();
        let (mut cancelled_recovery_resize, enqueued) = terminal.paired().resize_with_enqueue_receipt(&id, 0, 210, 65);
        tokio::time::timeout(Duration::from_secs(3), async {
            tokio::select! {
                result = &mut cancelled_recovery_resize => panic!("held recovery completed resize: {result:?}"),
                receipt = enqueued => receipt.expect("resize queued during recovery hold"),
            }
        }).await.unwrap();
        drop(cancelled_recovery_resize);
        recovery_release.send(()).unwrap();
        let recovered = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let outcome = recovery.borrow_and_update().clone();
                if outcome.state == "proxy_attached" { break outcome.generation.unwrap(); }
                recovery.changed().await.unwrap();
            }
        }).await.unwrap();
        assert_host_size(sessions, &id, recovered, host_terminal.output_hub(), &session.target.session_id, b"24 80").await;
        terminal.paired().detach(&id).await.unwrap();
    }).catch_unwind().await;
    desktop.cleanup().await;
    fixture.close().await;
    if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
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
