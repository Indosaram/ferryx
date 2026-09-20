//! Production-path proof that a paired workspace's DAG comes from the authenticated
//! remote host, not from a local scan of the remote path.
//!
//! The fixture separates the two machines on purpose: the checkpoint exists ONLY
//! under the host's registered root, and the desktop-side path string points at a
//! directory that does not exist locally. A local scan therefore cannot produce the
//! run, so observing it proves the frame crossed the authenticated transport.
use crate::daemon::{
    client::DaemonClient,
    protocol::{DaemonStreamMessage, PairedDagBinding},
    server::DaemonServer,
};
use crate::paired_host::service::{PairRequest, PairedHostService, Secret};
use crate::remote::auth::{DeviceAccessScope, DevicePermission};
use std::{path::Path, sync::Arc, time::Duration};

const CHECKPOINT: &str =
    include_str!("../dag/testdata/dag_081e597f-0aa8-4a20-a826-4e3d045aacef.json");
const RUN_ID: &str = "dag_081e597f-0aa8-4a20-a826-4e3d045aacef";

struct Fixture {
    tasks: tokio::task::JoinSet<()>,
    desktop: Arc<DaemonServer>,
    client: DaemonClient,
    host_state: Arc<crate::remote::state::RemoteGatewayState>,
    coordinator: crate::remote::relay_client::PairingCoordinator,
    origin: String,
    socket: std::path::PathBuf,
}

impl Fixture {
    /// Builds a relay, a host gateway published through it, and a desktop daemon.
    /// Pairing and every DAG frame therefore cross the same authenticated relay
    /// transport the product uses.
    async fn new(root: &Path) -> anyhow::Result<Self> {
        let host_data = root.join("host");
        // Both private stores must exist before either authority writes: the paired
        // inventory fails closed rather than creating its own directory.
        std::fs::create_dir_all(&host_data)?;
        std::fs::create_dir_all(root.join("desktop"))?;

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

        let desktop_data = root.join("desktop");
        let desktop = crate::ipc::run_blocking({
            let desktop_data = desktop_data.clone();
            move || {
                let mut server = DaemonServer::new_with_paths(
                    Some(desktop_data.join("config")),
                    Some(desktop_data.join("auth")),
                );
                server
                    .set_paired_hosts_for_test(PairedHostService::open_test_loopback(desktop_data));
                Ok(Arc::new(server))
            }
        })
        .await
        .map_err(|e| anyhow::anyhow!("{e:?}"))?;

        let socket = root.join("desktop.sock");
        let uds = tokio::net::UnixListener::bind(&socket)?;
        let owner = Arc::clone(&desktop);
        tasks.spawn(async move {
            let mut clients = tokio::task::JoinSet::new();
            while let Ok((stream, _)) = uds.accept().await {
                let owner = Arc::clone(&owner);
                clients.spawn(async move { owner.handle_client(stream).await });
            }
        });

        let client = DaemonClient::new_with_socket(socket.clone());
        let _host_owner = host;
        tasks.spawn(async move {
            let _owner = _host_owner;
            std::future::pending::<()>().await;
        });
        Ok(Self {
            tasks,
            desktop,
            client,
            host_state,
            coordinator,
            origin,
            socket,
        })
    }

    /// Pairs the desktop with the host using a real machine-scope pairing code.
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
        self.client
            .paired_host_pair(PairRequest {
                relay_origin: self.origin.clone(),
                pin: Secret(issued.pin),
                display_label: "dag fixture host".into(),
            })
            .await
            .map_err(|e| anyhow::anyhow!(e.code))
    }

    async fn close(mut self) -> anyhow::Result<()> {
        self.tasks.shutdown().await;
        drop(self.client);
        drop(self.desktop);
        let _ = std::fs::remove_file(&self.socket);
        Ok(())
    }
}

/// Registers `root` on the host and returns its host-side workspace id.
async fn register(
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

fn write_checkpoint(root: &Path) -> anyhow::Result<()> {
    let runs = root.join(".omo/senpi-task/dag/runs");
    std::fs::create_dir_all(&runs)?;
    std::fs::write(runs.join(format!("{RUN_ID}.json")), CHECKPOINT)?;
    Ok(())
}

/// Collects frames until an inventory arrives, so the assertion never depends on
/// arrival timing.
async fn next_inventory(
    rx: &mut tokio::sync::mpsc::Receiver<DaemonStreamMessage<'static>>,
) -> Option<(String, Vec<crate::dag::journal::DagRunSnapshot>)> {
    loop {
        match tokio::time::timeout(Duration::from_secs(20), rx.recv()).await {
            Ok(Some(DaemonStreamMessage::DagInventory {
                project_path, runs, ..
            })) => return Some((project_path.into_owned(), runs)),
            Ok(Some(_)) => continue,
            Ok(None) | Err(_) => return None,
        }
    }
}

/// Drains inventory frames until `expected` distinct runs have arrived, returning
/// the frame count. A byte-batched inventory spans several frames.
async fn drain_inventory(
    rx: &mut tokio::sync::mpsc::Receiver<DaemonStreamMessage<'static>>,
    expected: usize,
) -> (usize, std::collections::BTreeSet<String>) {
    let mut frames = 0usize;
    let mut seen = std::collections::BTreeSet::new();
    while seen.len() < expected {
        match tokio::time::timeout(Duration::from_secs(30), rx.recv()).await {
            Ok(Some(DaemonStreamMessage::DagInventory { runs, .. })) => {
                frames += 1;
                seen.extend(runs.into_iter().map(|run| run.run_id));
            }
            Ok(Some(_)) => continue,
            Ok(None) | Err(_) => break,
        }
    }
    (frames, seen)
}

/// Waits for a live update naming `run_id`.
async fn next_update(
    rx: &mut tokio::sync::mpsc::Receiver<DaemonStreamMessage<'static>>,
    run_id: &str,
) -> bool {
    loop {
        match tokio::time::timeout(Duration::from_secs(30), rx.recv()).await {
            Ok(Some(DaemonStreamMessage::DagRunUpdated { snapshot, .. })) => {
                if snapshot.run_id == run_id {
                    return true;
                }
            }
            Ok(Some(_)) => continue,
            Ok(None) | Err(_) => return false,
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn paired_dag_streams_from_authenticated_remote_host() {
    let root = tempfile::tempdir().unwrap();
    let host_root = root.path().join("remote-workspace");
    std::fs::create_dir_all(&host_root).unwrap();
    let host_root = std::fs::canonicalize(&host_root).unwrap();
    write_checkpoint(&host_root).unwrap();

    // The desktop-side path is deliberately absent locally: any run observed under
    // it can only have come from the remote host.
    let desktop_path = root.path().join("not-on-this-machine").join("workspace");
    assert!(!desktop_path.exists(), "local decoy must not exist");

    let fixture = Fixture::new(root.path()).await.unwrap();
    let outcome = async {
        let host = fixture.pair().await.map_err(|error| anyhow::anyhow!("pair fixture: {error:#}"))?;
        let remote_workspace_id = register(&fixture.host_state, &host_root).await
            .map_err(|error| anyhow::anyhow!("register workspace: {error:#}"))?;

        // Control: the pre-fix production path (no paired binding) scans the local
        // path and therefore cannot see the remote run.
        let mut local = fixture
            .client
            .subscribe_dag(
                "daemon:fixture",
                desktop_path.to_str().expect("utf-8 decoy path"),
            )
            .await
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        let (local_path, local_runs) = next_inventory(&mut local)
            .await
            .ok_or_else(|| anyhow::anyhow!("local subscription produced no inventory"))?;
        anyhow::ensure!(
            local_runs.is_empty(),
            "local scan of a remote path must not yield runs: {local_runs:?}"
        );
        anyhow::ensure!(local_path == desktop_path.to_string_lossy());
        drop(local);

        // Production path: bound to the authenticated host + its own workspace id.
        let binding = PairedDagBinding {
            host_id: host.host_id.clone(),
            generation: host.generation,
            remote_workspace_id: remote_workspace_id.clone(),
        };
        let mut remote = fixture
            .client
            .subscribe_dag_bound(
                "daemon:fixture",
                desktop_path.to_str().expect("utf-8 decoy path"),
                Some(binding.clone()),
            )
            .await
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        let (remote_path, remote_runs) = next_inventory(&mut remote)
            .await
            .ok_or_else(|| anyhow::anyhow!("remote subscription produced no inventory"))?;
        anyhow::ensure!(
            remote_runs.iter().any(|run| run.run_id == RUN_ID),
            "authenticated remote inventory missing the host-only run: {remote_runs:?}"
        );
        // The root is the one the HOST resolved, never the desktop's decoy.
        anyhow::ensure!(
            remote_path == host_root.to_string_lossy(),
            "project path must be the remote canonical root, got {remote_path}"
        );

        // A journal write on the host machine pushes a live update over the same
        // stream: delivery is push, not desktop polling.
        let second = format!("{RUN_ID}-live");
        let live = CHECKPOINT.replacen(RUN_ID, &second, 1);
        std::fs::write(
            host_root
                .join(".omo/senpi-task/dag/runs")
                .join(format!("{second}.json")),
            live,
        )?;
        anyhow::ensure!(
            next_update(&mut remote, &second).await,
            "host-side journal write did not reach the desktop as a live update"
        );
        drop(remote);

        // Reconnect must re-hydrate: the host watcher only emits snapshots it saw
        // change, so a fresh subscription that returned nothing would leave a
        // reconnecting desktop permanently blank.
        let mut reconnected = fixture
            .client
            .subscribe_dag_bound(
                "daemon:fixture",
                desktop_path.to_str().expect("utf-8 decoy path"),
                Some(binding.clone()),
            )
            .await
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        let (_, rehydrated) = next_inventory(&mut reconnected)
            .await
            .ok_or_else(|| anyhow::anyhow!("reconnect produced no inventory"))?;
        anyhow::ensure!(
            rehydrated.iter().any(|run| run.run_id == RUN_ID),
            "reconnect did not resend the existing inventory: {rehydrated:?}"
        );
        drop(reconnected);

        // A journal far larger than one frame must still hydrate: batching is by
        // bytes, so no single frame can exceed the transport ceiling.
        let bulk_runs = 60usize;
        let filler = "d".repeat(8 * 1024);
        for index in 0..bulk_runs {
            let id = format!("{RUN_ID}-bulk-{index}");
            let mut body: serde_json::Value = serde_json::from_str(CHECKPOINT)?;
            body["runId"] = serde_json::json!(id);
            body["name"] = serde_json::json!(filler);
            let body = serde_json::to_string(&body)?;
            let normalized = crate::dag::journal::parse_run_checkpoint(&body)?;
            anyhow::ensure!(normalized.name.len() == filler.len(), "bulk payload was discarded by parser");
            std::fs::write(
                host_root
                    .join(".omo/senpi-task/dag/runs")
                    .join(format!("{id}.json")),
                body,
            )?;
        }
        let mut bulk = fixture
            .client
            .subscribe_dag_bound(
                "daemon:fixture",
                desktop_path.to_str().expect("utf-8 decoy path"),
                Some(binding.clone()),
            )
            .await
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        // 2 originals + bulk runs, delivered across more than one frame.
        let expected_total = bulk_runs + 2;
        let (frames, seen) = drain_inventory(&mut bulk, expected_total).await;
        anyhow::ensure!(
            seen.len() == expected_total,
            "byte-batched inventory dropped runs: {} of {expected_total}",
            seen.len()
        );
        anyhow::ensure!(
            frames > 1,
            "oversized inventory must span multiple frames, got {frames}"
        );
        drop(bulk);

        // Unsubscribe: dropping the receiver must release the host-side watcher.
        let mut streams = crate::remote::dag_api::active_stream_count();
        tokio::time::timeout(Duration::from_secs(20), streams.wait_for(|n| *n == 0))
            .await
            .map_err(|_| anyhow::anyhow!("host watcher outlived the desktop subscription"))??;

        // Mismatched identity: a binding naming a workspace this host never granted
        // is refused, and no stream is opened.
        let foreign = PairedDagBinding {
            remote_workspace_id: "project-never-registered".into(),
            ..binding.clone()
        };
        let mut rejected = fixture
            .client
            .subscribe_dag_bound("daemon:fixture", "irrelevant", Some(foreign))
            .await
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        anyhow::ensure!(
            next_inventory(&mut rejected).await.is_none(),
            "unknown workspace must not stream any inventory"
        );
        anyhow::ensure!(
            *crate::remote::dag_api::active_stream_count().borrow() == 0,
            "refused subscription left a host stream running"
        );

        // A stale generation cannot stream: forgetting the host retires the grant.
        fixture
            .client
            .paired_host_forget(host.host_id.clone(), host.generation)
            .await
            .map_err(|e| anyhow::anyhow!(e.code))?;
        let mut stale = fixture
            .client
            .subscribe_dag_bound("daemon:fixture", "irrelevant", Some(binding))
            .await
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        anyhow::ensure!(
            next_inventory(&mut stale).await.is_none(),
            "retired credential must not stream"
        );

        eprintln!(
            "paired-dag remote_only_checkpoint=true remote_root={} live_push=true reconnect_rehydrated=true byte_batched_frames>1 unsubscribe_released=true unknown_workspace_rejected=true stale_generation_rejected=true",
            host_root.display()
        );
        Ok::<_, anyhow::Error>(())
    }
    .await;

    fixture.close().await.unwrap();
    root.close().unwrap();
    outcome.unwrap();
}
