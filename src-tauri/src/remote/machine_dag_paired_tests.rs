//! Portable (Windows-inclusive) proof that a paired workspace's DAG comes from the
//! authenticated remote host, including runs that live inside authoritative managed
//! worktrees.
//!
//! `dag_paired_tests` drives the same production path through the desktop daemon, which
//! needs a Unix socket, so its fixture cannot compile under `cfg(windows)`. This file
//! uses the transport the desktop daemon itself uses - `MachineClient::attach_dag`
//! (machine-scope capability probe, then a ticketed socket over the paired relay) - and
//! therefore runs on every target.
//!
//! The checkpoint exists ONLY under the host's registered root and the desktop never
//! supplies a filesystem path, so observing the run proves the frame crossed the
//! authenticated transport from the host's own worktree authority.
use crate::daemon::server::DaemonServer;
use crate::paired_host::{
    client::{MachineClient, Operation, OperationRequest, OperationResult, PairedDagStream},
    service::{PairRequest, PairedHostService, Secret},
};
use crate::remote::{
    auth::{DeviceAccessScope, DevicePermission},
    dag_api::DagFrame,
    machine_protocol as m,
};
use futures_util::StreamExt;
use std::{path::Path, sync::Arc, time::Duration};

const CHECKPOINT: &str =
    include_str!("../dag/testdata/dag_081e597f-0aa8-4a20-a826-4e3d045aacef.json");
const RUN_ID: &str = "dag_081e597f-0aa8-4a20-a826-4e3d045aacef";

/// One absolute bound per wait. Timing out frame-by-frame would let an unrelated but busy
/// stream extend a wait whose deadline has already passed.
const EVENT_WAIT: Duration = Duration::from_secs(30);

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
                display_label: "dag fixture host".into(),
            })
            .await
            .map_err(|e| anyhow::anyhow!(e.code))
    }

    async fn close(mut self) {
        // The daemon owns the host gateway, so it is released before the relay tasks join.
        drop(self.host);
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

/// Windows canonicalization yields verbatim (`\\?\C:\...`) paths and Windows compares
/// them case-insensitively, so both sides are reduced to one comparable form.
fn comparable_path(path: &str) -> String {
    crate::worktree::strip_verbatim_prefix(path)
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_ascii_lowercase()
}

fn write_run(worktree_root: &str, run_id: &str) -> anyhow::Result<()> {
    let runs = Path::new(worktree_root).join(".omo/senpi-task/dag/runs");
    std::fs::create_dir_all(&runs)?;
    std::fs::write(
        runs.join(format!("{run_id}.json")),
        CHECKPOINT.replacen(RUN_ID, run_id, 1),
    )?;
    Ok(())
}

/// Creates a managed worktree through the machine route the paired desktop uses, so the
/// committed revision that publishes the worktree change is produced exactly as the app
/// produces it.
async fn create_worktree_via_machine(
    client: &MachineClient,
    service: &PairedHostService,
    host_id: &str,
    generation: crate::scoped_contracts::Epoch,
    workspace_id: &str,
    slug: &str,
) -> anyhow::Result<m::Worktree> {
    let response = client
        .execute(
            service,
            OperationRequest {
                host_id: host_id.to_string(),
                generation,
                operation: Operation::CreateWorktree {
                    request: m::CreateWorktreeRequest {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        workspace_id: workspace_id.to_string(),
                        worktree: m::WorktreeIdentity {
                            ws_id: workspace_id.to_string(),
                            slug: slug.to_string(),
                        },
                        base_ref: None,
                    },
                },
            },
        )
        .await
        .map_err(|error| anyhow::anyhow!("CreateWorktree({slug}) rejected: {error:?}"))?;
    match response.result {
        OperationResult::CreateWorktree(worktree) => Ok(worktree),
        other => Err(anyhow::anyhow!(
            "expected CreateWorktree result for {slug}, got: {other:?}"
        )),
    }
}

/// Waits for `run_id` in either form the host may publish it: an inventory frame (the
/// reconcile scan observed it) or a live update (the watcher armed for that root fired
/// first). Which one wins is a race, so a regression keyed on one frame type only would
/// be timing-dependent.
async fn next_run_state(stream: &mut PairedDagStream, run_id: &str) -> anyhow::Result<bool> {
    let wait = async {
        loop {
            let frame = match stream.socket.next().await {
                Some(Ok(frame)) => frame,
                Some(Err(error)) => return Err(anyhow::anyhow!("dag socket error: {error}")),
                None => return Ok(false),
            };
            match frame {
                tokio_tungstenite::tungstenite::Message::Text(text) => {
                    match serde_json::from_str::<DagFrame>(&text) {
                        Ok(DagFrame::DagInventory { runs, .. })
                            if runs.iter().any(|run| run.run_id == run_id) =>
                        {
                            return Ok(true);
                        }
                        Ok(DagFrame::DagRunUpdated { snapshot, .. })
                            if snapshot.run_id == run_id =>
                        {
                            return Ok(true);
                        }
                        Ok(DagFrame::DagError { code }) => {
                            return Err(anyhow::anyhow!("host reported dag error: {code}"));
                        }
                        _ => continue,
                    }
                }
                tokio_tungstenite::tungstenite::Message::Close(_) => return Ok(false),
                _ => continue,
            }
        }
    };
    let bounded: Result<Result<bool, anyhow::Error>, tokio::time::error::Elapsed> =
        tokio::time::timeout(EVENT_WAIT, wait).await;
    bounded.unwrap_or(Ok(false))
}

/// Waits for the first inventory frame, so the assertion never depends on arrival timing.
async fn next_inventory(
    stream: &mut PairedDagStream,
) -> anyhow::Result<Option<(String, Vec<crate::dag::journal::DagRunSnapshot>)>> {
    let wait = async {
        loop {
            let frame = match stream.socket.next().await {
                Some(Ok(frame)) => frame,
                Some(Err(error)) => return Err(anyhow::anyhow!("dag socket error: {error}")),
                None => return Ok(None),
            };
            match frame {
                tokio_tungstenite::tungstenite::Message::Text(text) => {
                    match serde_json::from_str::<DagFrame>(&text) {
                        Ok(DagFrame::DagInventory {
                            project_path, runs, ..
                        }) => return Ok(Some((project_path, runs))),
                        Ok(DagFrame::DagError { code }) => {
                            return Err(anyhow::anyhow!("host reported dag error: {code}"));
                        }
                        _ => continue,
                    }
                }
                tokio_tungstenite::tungstenite::Message::Close(_) => return Ok(None),
                _ => continue,
            }
        }
    };
    let bounded: Result<
        Result<Option<(String, Vec<crate::dag::journal::DagRunSnapshot>)>, anyhow::Error>,
        tokio::time::error::Elapsed,
    > = tokio::time::timeout(EVENT_WAIT, wait).await;
    bounded.unwrap_or(Ok(None))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn paired_workspace_dag_scans_and_watches_managed_worktree_runs() {
    let root = tempfile::tempdir().unwrap();
    let host_root = root.path().join("remote-workspace");
    std::fs::create_dir_all(&host_root).unwrap();
    let host_root = std::fs::canonicalize(&host_root).unwrap();

    // Managed worktrees are git worktrees, so the registered root must be a real repository.
    crate::worktree::run_git(&host_root, &["init"]).expect("git init");
    crate::worktree::run_git(&host_root, &["config", "user.name", "DAG Fixture"])
        .expect("git config user.name");
    crate::worktree::run_git(&host_root, &["config", "user.email", "dag@example.invalid"])
        .expect("git config user.email");
    std::fs::write(host_root.join("README.md"), "initial commit\n").unwrap();
    crate::worktree::run_git(&host_root, &["add", "README.md"]).expect("git add");
    crate::worktree::run_git(&host_root, &["commit", "-m", "initial commit"]).expect("git commit");

    let fixture = Fixture::new(root.path()).await.unwrap();
    let outcome = async {
        let host_view = fixture
            .pair()
            .await
            .map_err(|error| anyhow::anyhow!("pair fixture: {error:#}"))?;
        let workspace_id = register_workspace(&fixture.host_state, &host_root)
            .await
            .map_err(|error| anyhow::anyhow!("register workspace: {error:#}"))?;

        let machine_client = MachineClient::new();
        let first = create_worktree_via_machine(
            &machine_client,
            &fixture.paired_service,
            &host_view.host_id,
            host_view.generation,
            &workspace_id,
            "portable-first",
        )
        .await?;
        write_run(&first.path, RUN_ID).map_err(|error| anyhow::anyhow!("write run: {error}"))?;

        let mut stream = machine_client
            .attach_dag(
                &fixture.paired_service,
                host_view.host_id.clone(),
                host_view.generation,
                &workspace_id,
            )
            .await
            .map_err(|error| anyhow::anyhow!("attach_dag: {error:?}"))?;

        // The desktop supplied no path: the frame carries the root the host resolved, and
        // the run is only reachable through the host's authoritative worktree enumeration.
        let (project_path, runs) = next_inventory(&mut stream)
            .await?
            .ok_or_else(|| anyhow::anyhow!("paired DAG stream published no inventory"))?;
        anyhow::ensure!(
            comparable_path(&project_path) == comparable_path(&host_root.to_string_lossy()),
            "project path must be the root the host resolved, got {project_path}"
        );
        anyhow::ensure!(
            runs.iter().any(|run| run.run_id == RUN_ID),
            "inventory is missing the run that exists only inside the managed worktree: {runs:?}"
        );

        // Delivery is push, not polling: a journal write inside the worktree arrives as an
        // update over the same stream.
        let live_run_id = format!("{RUN_ID}-live");
        write_run(&first.path, &live_run_id)
            .map_err(|error| anyhow::anyhow!("write live run: {error}"))?;
        anyhow::ensure!(
            next_run_state(&mut stream, &live_run_id).await?,
            "a journal write inside the managed worktree never reached the paired desktop"
        );

        // A managed worktree created AFTER the subscription is reconciled through the
        // committed worktree change: its watcher is armed and its runs are published.
        let late = create_worktree_via_machine(
            &machine_client,
            &fixture.paired_service,
            &host_view.host_id,
            host_view.generation,
            &workspace_id,
            "portable-late",
        )
        .await?;
        let late_run_id = format!("{RUN_ID}-late");
        write_run(&late.path, &late_run_id)
            .map_err(|error| anyhow::anyhow!("write late run: {error}"))?;
        anyhow::ensure!(
            next_run_state(&mut stream, &late_run_id).await?,
            "the host never reconciled the managed worktree created after the subscription"
        );

        // Closing the socket is what retires the host-side watchers; dropping alone would
        // leave the host streaming until its transfer timeout.
        let _ = stream.socket.close(None).await;
        Ok::<_, anyhow::Error>(())
    }
    .await;

    fixture.close().await;
    root.close().unwrap();
    outcome.unwrap();
}
