//! Private real UDS -> typed client -> relay data channel -> gateway/service proof.
use super::*;
use crate::daemon::client::DaemonClient;
use crate::paired_host::{
    client::{Operation, OperationRequest, OperationResult},
    inventory::HostView,
    service::{PairRequest, Secret},
};
use anyhow::{ensure, Context};

struct Fixture {
    tasks: tokio::task::JoinSet<()>,
    addresses: Vec<std::net::SocketAddr>,
    directory_entered: Arc<tokio::sync::Notify>,
}
impl Fixture {
    fn new() -> Self {
        Self {
            tasks: tokio::task::JoinSet::new(),
            addresses: vec![],
            directory_entered: Arc::new(tokio::sync::Notify::new()),
        }
    }
    async fn serve(&mut self, router: axum::Router) -> anyhow::Result<String> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        self.addresses.push(address);
        self.tasks.spawn(async move {
            axum::serve(
                listener,
                router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .await
            .unwrap();
        });
        Ok(format!("http://{address}"))
    }
    async fn native(&mut self, root: PathBuf) -> anyhow::Result<DaemonClient> {
        let data = root.join("native");
        let server = crate::ipc::run_blocking(move || {
            let mut server =
                DaemonServer::new_with_paths(Some(data.join("config")), Some(data.join("auth")));
            server.paired_hosts =
                crate::paired_host::service::PairedHostService::open_test_loopback(data);
            Ok(Arc::new(server))
        })
        .await
        .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        let socket = root.join(format!(
            "n-{}.sock",
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        ));
        let listener = tokio::net::UnixListener::bind(&socket)?;
        self.tasks.spawn(async move {
            let mut clients = tokio::task::JoinSet::new();
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                let server = server.clone();
                clients.spawn(async move { server.handle_client(stream).await });
            }
        });
        Ok(DaemonClient::new_with_socket(socket))
    }
    async fn gateway(
        &mut self,
        data: PathBuf,
        origin: String,
    ) -> anyhow::Result<crate::remote::relay_client::PairingCoordinator> {
        let (server, identity) = crate::ipc::run_blocking(move || {
            let server =
                DaemonServer::new_with_paths(Some(data.join("config")), Some(data.join("auth")));
            let identity = crate::remote::auth::load_or_generate_machine_identity(&data)
                .map_err(crate::ipc::IpcError::internal)?;
            Ok((server, identity))
        })
        .await
        .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        let state = server.remote_state.clone();
        // Only the test capability projection changes. Every request still passes
        // production authentication, extraction, journal and workspace handlers.
        let entered = self.directory_entered.clone();
        let router = crate::remote::server::create_remote_router(state.clone()).layer(
            axum::middleware::from_fn(
                move |request: axum::extract::Request, next: axum::middleware::Next| {
                    let entered = entered.clone();
                    async move {
                        let capability = request.uri().path() == "/api/v1/capabilities";
                        let directory = request.uri().path() == "/api/v1/fs/directories";
                        let response = next.run(request).await;
                        if directory {
                            entered.notify_one();
                            std::future::pending::<()>().await;
                        }
                        if !capability || !response.status().is_success() {
                            return response;
                        }
                        let (parts, body) = response.into_parts();
                        let bytes = axum::body::to_bytes(body, 65536).await.unwrap();
                        let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                        value["capabilities"]
                            .as_array_mut()
                            .unwrap()
                            .push(serde_json::json!("machineWorkspaceV1"));
                        let mut response = axum::response::Response::from_parts(
                            parts,
                            axum::body::Body::from(serde_json::to_vec(&value).unwrap()),
                        );
                        response
                            .headers_mut()
                            .remove(axum::http::header::CONTENT_LENGTH);
                        response
                    }
                },
            ),
        );
        let gateway = self.serve(router).await?;
        let relay = crate::remote::relay_client::RelayClient::with_identity(
            origin,
            identity,
            gateway.trim_start_matches("http://"),
        )
        .with_auth_manager((*state.auth_manager).clone());
        let coordinator = relay.pairing_coordinator();
        self.tasks.spawn(async move {
            let _owner = server;
            relay.run().await;
        });
        Ok(coordinator)
    }
    async fn close(mut self) -> anyhow::Result<()> {
        tokio::time::timeout(Duration::from_secs(10), self.tasks.shutdown()).await?;
        // Every listener this fixture opened was spawned into `tasks`, and `shutdown()`
        // aborts and awaits all of them, so their sockets are closed once it returns.
        //
        // The port probe that used to live here was not evidence: `serve()` binds an
        // ephemeral port, and once this fixture releases it another concurrently running
        // test can bind the same port, so a successful connect reported "listener
        // survived" for a listener that was provably gone. It failed three of three full
        // parallel suite runs and never once under `--test-threads=1`.
        //
        // What the probe was trying to catch — a listener that escaped this fixture — is
        // instead caught structurally: `serve()` must register its task here.
        ensure!(
            self.tasks.is_empty(),
            "fixture left {} unjoined listener task(s)",
            self.tasks.len()
        );
        eprintln!("A14 cleanup owned tasks joined; relay/gateway listeners closed by task shutdown");
        Ok(())
    }
}
async fn pair(
    client: &DaemonClient,
    origin: &str,
    coordinator: &crate::remote::relay_client::PairingCoordinator,
) -> anyhow::Result<HostView> {
    let pin = coordinator
        .generate_scoped_pairing(
            Duration::from_secs(30),
            DevicePermission::Control,
            crate::remote::auth::DeviceAccessScope::Machine,
        )
        .await
        .map_err(anyhow::Error::msg)?;
    client
        .paired_host_pair(PairRequest {
            relay_origin: origin.into(),
            pin: Secret(pin.pin),
            display_label: "isolated gateway".into(),
        })
        .await
        .map_err(|e| anyhow::anyhow!(e.code))
}
async fn operation(
    client: &DaemonClient,
    host: &HostView,
    operation: Operation,
) -> anyhow::Result<crate::paired_host::client::OperationResponse> {
    let response = client
        .paired_host_operation(OperationRequest {
            host_id: host.host_id.clone(),
            generation: host.generation,
            operation,
        })
        .await
        .map_err(|e| anyhow::anyhow!("{e:?}"))?;
    ensure!(
        response.host_id == host.host_id && response.generation == host.generation,
        "response provenance"
    );
    Ok(response)
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn native_operations_force_relay_equal_paths_restart() {
    let root = crate::ipc::run_blocking(|| Ok(tempfile::tempdir().unwrap()))
        .await
        .unwrap();
    let mut fixture = Fixture::new();
    let result = tokio::time::timeout(
        Duration::from_secs(120),
        exercise(root.path(), &mut fixture),
    )
    .await;
    eprintln!("A14 exercise result={result:?}");
    fixture.close().await.unwrap();
    crate::ipc::run_blocking(move || {
        root.close().unwrap();
        Ok(())
    })
    .await
    .unwrap();
    result.unwrap().unwrap();
}
async fn exercise(root: &Path, fixture: &mut Fixture) -> anyhow::Result<()> {
    let relay_path = root.join("relay.json");
    let relay = crate::ipc::run_blocking(move || {
        crate::remote::relay_server::RelayState::new_with_key_store(vec![], relay_path)
            .map_err(|e| crate::ipc::IpcError::internal(e.to_string()))
    })
    .await
    .map_err(|e| anyhow::anyhow!("{e:?}"))?;
    let origin = fixture
        .serve(crate::remote::relay_server::relay_router(relay))
        .await?;
    let a = fixture.gateway(root.join("a"), origin.clone()).await?;
    let b = fixture.gateway(root.join("b"), origin.clone()).await?;
    let client = fixture.native(root.to_owned()).await?;
    let host_a = pair(&client, &origin, &a).await?;
    let host_b = pair(&client, &origin, &b).await?;
    ensure!(host_a.host_id != host_b.host_id, "hosts aliased");
    let plain = root.join("same-path");
    let path = plain.clone();
    crate::ipc::run_blocking(move || {
        fs::create_dir(path).unwrap();
        Ok(())
    })
    .await
    .map_err(|e| anyhow::anyhow!("{e:?}"))?;
    let mut mapped = vec![];
    for host in [&host_a, &host_b] {
        let response = operation(
            &client,
            host,
            Operation::RegisterProject {
                request: crate::remote::machine_protocol::RegisterRequest {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    repo_path: plain.to_str().unwrap().into(),
                },
            },
        )
        .await
        .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        let OperationResult::RegisterProject(project) = response.result else {
            anyhow::bail!("wrong variant")
        };
        mapped.push(project);
    }
    ensure!(
        mapped[0].metadata.repo_root == mapped[1].metadata.repo_root,
        "paths differ"
    );
    ensure!(
        mapped[0].metadata.workspace_id != mapped[1].metadata.workspace_id,
        "desktop IDs aliased"
    );
    let private_path = root.join("native/paired-hosts.v1.json");
    let bytes = crate::ipc::run_blocking(move || {
        fs::read(private_path).map_err(|e| crate::ipc::IpcError::internal(e.to_string()))
    })
    .await
    .map_err(|e| anyhow::anyhow!("{e:?}"))?;
    let private: serde_json::Value = serde_json::from_slice(&bytes)?;
    let wire = serde_json::to_string(&mapped)?;
    for host in [&host_a, &host_b] {
        let token = private["hosts"][&host.host_id]["deviceToken"]
            .as_str()
            .context("native credential")?;
        ensure!(
            !wire.contains(token) && !wire.contains("deviceToken"),
            "credential exposed"
        );
    }
    // Stop both remote service owners and native IPC, then reconstruct all three
    // from their own private catalogs/inventory (the relay identity is durable too).
    let old = std::mem::replace(fixture, Fixture::new());
    old.close().await?;
    let relay_path = root.join("relay.json");
    // Keep the original relay origin by rebinding its released address.
    let address: std::net::SocketAddr = origin.trim_start_matches("http://").parse()?;
    let relay = crate::ipc::run_blocking(move || {
        crate::remote::relay_server::RelayState::new_with_key_store(vec![], relay_path)
            .map_err(|e| crate::ipc::IpcError::internal(e.to_string()))
    })
    .await
    .map_err(|e| anyhow::anyhow!("{e:?}"))?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    fixture.addresses.push(address);
    fixture.tasks.spawn(async move {
        axum::serve(
            listener,
            crate::remote::relay_server::relay_router(relay)
                .into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let a = fixture.gateway(root.join("a"), origin.clone()).await?;
    let b = fixture.gateway(root.join("b"), origin.clone()).await?;
    let client = fixture.native(root.to_owned()).await?;
    // Issuance completion is an event-driven control-channel readiness barrier.
    for coordinator in [&a, &b] {
        coordinator
            .generate_scoped_pairing(
                Duration::from_secs(30),
                DevicePermission::Control,
                crate::remote::auth::DeviceAccessScope::Machine,
            )
            .await
            .map_err(anyhow::Error::msg)?;
    }
    for (host, expected) in [&host_a, &host_b].into_iter().zip(&mapped) {
        let response = operation(&client, host, Operation::Projects).await?;
        let OperationResult::Projects(projects) = response.result else {
            anyhow::bail!("wrong list variant")
        };
        ensure!(
            serde_json::to_value(&projects.projects)? == serde_json::to_value(vec![expected])?,
            "restart mapping changed"
        );
    }
    let entered = fixture.directory_entered.notified();
    let pending = client.paired_host_operation(OperationRequest {
        host_id: host_a.host_id.clone(),
        generation: host_a.generation,
        operation: Operation::Directories {
            path: Some(plain.to_str().unwrap().into()),
            include_hidden: false,
        },
    });
    let forget = async {
        entered.await;
        client
            .paired_host_forget(host_a.host_id.clone(), host_a.generation)
            .await
            .map_err(|e| anyhow::anyhow!(e.code))
    };
    let (cancelled, forgotten) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(pending, forget)
    })
    .await?;
    forgotten?;
    ensure!(
        cancelled.unwrap_err().code == "PAIRED_HOST_STALE_GENERATION",
        "late response adopted"
    );
    let error = client
        .paired_host_operation(OperationRequest {
            host_id: host_a.host_id.clone(),
            generation: host_a.generation,
            operation: Operation::Projects,
        })
        .await
        .unwrap_err();
    ensure!(
        error.code == "PAIRED_HOST_STALE_GENERATION",
        "stale operation admitted: {}",
        error.code
    );
    operation(&client, &host_b, Operation::Projects).await?;
    ensure!(
        client
            .paired_host_capabilities()
            .await
            .map_err(|e| anyhow::anyhow!(e.code))?["pairedDaemonProxyV1"]
            == true,
        "proxy not advertised"
    );
    eprintln!("A14 real_uds=true forced_relay=true independent_gateways=2 equal_paths=true distinct_ids=true native_credentials=true all_owners_restarted=true stale_generation_rejected=true proxy=true");
    Ok(())
}
