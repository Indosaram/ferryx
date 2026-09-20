//! Included as a private daemon test module; never compiled into production.
use super::*;
use crate::daemon::client::DaemonClient;
use crate::paired_host::{
    inventory::{HostView, MigrationReceipt},
    service::{PairRequest, Secret},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn paired_host_process_child() {
    let Some(root) = std::env::var_os("A13_PROCESS_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let data = root.join("data");
    let mut server =
        DaemonServer::new_with_paths(Some(data.join("config")), Some(data.join("auth")));
    server.paired_hosts = crate::paired_host::service::PairedHostService::open_test_loopback(data);
    let server = Arc::new(server);
    let socket = root.join("daemon.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let mut control = tokio::net::UnixStream::connect(root.join("control.sock"))
        .await
        .unwrap();
    control.write_all(b"R").await.unwrap();
    let mut clients = tokio::task::JoinSet::new();
    let mut stop = [0];
    loop {
        tokio::select! {
            result = control.read_exact(&mut stop) => { result.unwrap(); break; }
            accepted = listener.accept() => {
                let (stream, _) = accepted.unwrap();
                let server = Arc::clone(&server);
                clients.spawn(async move { server.handle_client(stream).await });
            }
        }
    }
    clients.shutdown().await;
    drop(listener);
    drop(server);
    fs::remove_file(socket).unwrap();
}

async fn with_process<T>(
    root: &Path,
    action: impl AsyncFnOnce(DaemonClient) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    use anyhow::ensure;
    let control_path = root.join("control.sock");
    let listener = tokio::net::UnixListener::bind(&control_path)?;
    let mut command = tokio::process::Command::new(std::env::current_exe()?);
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin")
        .env("CARGO_HOME", "/Users/indo/.cargo")
        .env("RUSTUP_HOME", "/Users/indo/.rustup")
        .args([
            "--exact",
            "daemon::server::paired_host_process_tests::paired_host_process_child",
            "--nocapture",
        ])
        .env("A13_PROCESS_ROOT", root)
        .env("HOME", root.join("home"))
        .env("FERRYX_DATA_DIR", root.join("data"))
        .env("FERRYX_RUNTIME_DIR", root.join("runtime"))
        .env("FERRYX_SESSION_DIR", root.join("sessions"))
        .env("FERRYX_AGENT_STATE_SOCKET", root.join("runtime/agent.sock"))
        .env("XDG_CONFIG_HOME", root.join("xdg-config"))
        .env("XDG_CACHE_HOME", root.join("xdg-cache"))
        .env("XDG_DATA_HOME", root.join("xdg-data"))
        .env("XDG_RUNTIME_DIR", root.join("runtime"))
        .env("TMPDIR", root.join("tmp"))
        .env("TMP", root.join("tmp"))
        .env("TEMP", root.join("tmp"))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .kill_on_drop(true);
    // Cargo supplies the build-local dynamic library search path for test binaries.
    // Preserve only that loader seam after clearing inherited application settings.
    for key in [
        "DYLD_FALLBACK_LIBRARY_PATH",
        "DYLD_LIBRARY_PATH",
        "LD_LIBRARY_PATH",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    let mut child = command.spawn()?;
    let pid = child.id().unwrap();
    let result = tokio::time::timeout(Duration::from_secs(35), async {
        let (mut control, _) = tokio::select! {
            accepted = listener.accept() => accepted?,
            exited = child.wait() => { anyhow::bail!("child exited before readiness: {}", exited?); }
        };
        let mut ready = [0]; control.read_exact(&mut ready).await?;
        ensure!(ready == *b"R", "bad readiness signal");
        let result = action(DaemonClient::new_with_socket(root.join("daemon.sock"))).await;
        control.write_all(b"S").await?;
        Ok::<_, anyhow::Error>(result)
    }).await;
    // Reap on both normal completion and injected action failure, and on timeout.
    let status = match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
        Ok(status) => status?,
        Err(_) => {
            child.start_kill()?;
            child.wait().await?
        }
    };
    let refused = tokio::net::UnixStream::connect(root.join("daemon.sock"))
        .await
        .is_err();
    drop(listener);
    fs::remove_file(control_path)?;
    eprintln!(
        "A13 process pid={pid} reaped=true uds_refused={refused} success={}",
        status.success()
    );
    ensure!(refused, "child UDS survived reap");
    ensure!(status.success(), "child failed");
    result??
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn paired_host_separate_process_restart() {
    let root = tempfile::tempdir().unwrap();
    for dir in [
        "home",
        "data",
        "runtime",
        "sessions",
        "xdg-config",
        "xdg-cache",
        "xdg-data",
        "tmp",
    ] {
        fs::create_dir(root.path().join(dir)).unwrap();
    }
    let result = restart(root.path()).await;
    let path = root.path().to_owned();
    root.close().unwrap();
    eprintln!(
        "A13 process root={} removed={}",
        path.display(),
        !path.exists()
    );
    result.unwrap();
}
async fn restart(root: &Path) -> anyhow::Result<()> {
    use anyhow::{ensure, Context};
    let relay_state = crate::remote::relay_server::RelayState::new_with_key_store(
        vec![],
        root.join("relay-keys.json"),
    )
    .map_err(anyhow::Error::msg)?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let origin = format!("http://{address}");
    let mut tasks = tokio::task::JoinSet::new();
    tasks.spawn(async move {
        axum::serve(
            listener,
            crate::remote::relay_server::relay_router(relay_state)
                .into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
    });
    let gateway = Arc::new(DaemonServer::new_with_paths(
        Some(root.join("gateway/config")),
        Some(root.join("gateway/auth")),
    ));
    struct Stop(Arc<DaemonServer>);
    impl Drop for Stop {
        fn drop(&mut self) {
            if let Some(handle) = self.0.remote_server_handle.lock().take() {
                handle.stop();
            }
        }
    }
    let guard = Stop(Arc::clone(&gateway));
    let result = async {
        gateway.configure_gateway(RemoteGatewayConfig { mode: RemoteNetworkMode::Relay, port: 0, relay_url: Some(origin.clone()), ..Default::default() }).await.map_err(anyhow::Error::msg)?;
        let coordinator = gateway.remote_state.relay_pairing.read().as_ref().map(|p| p.coordinator.clone()).context("coordinator")?;
        let pin = coordinator.generate_scoped_pairing(Duration::from_secs(60), DevicePermission::Control, crate::remote::auth::DeviceAccessScope::Machine).await.map_err(anyhow::Error::msg)?;
        let host: HostView = with_process(root, async |client| {
            client.paired_host_pair(PairRequest { relay_origin: origin.clone(), pin: Secret(pin.pin), display_label: "Process host".into() }).await.map_err(|e| anyhow::anyhow!(e.code))
        }).await?;
        let bytes = fs::read(root.join("data/paired-hosts.v1.json"))?;
        let disk: serde_json::Value = serde_json::from_slice(&bytes)?;
        let token = disk["hosts"][&host.host_id]["deviceToken"].as_str().context("private stored credential")?.to_owned();
        with_process(root, async |client| {
            ensure!(client.paired_host_list().await.map_err(|e| anyhow::anyhow!(e.code))? == vec![host.clone()], "restart lost record");
            let read = client.paired_host_read(MigrationReceipt { host_id: host.host_id.clone(), generation: host.generation }).await.map_err(|e| anyhow::anyhow!(e.code))?;
            ensure!(read == host, "restart exact readback mismatch");
            let receipt = client.paired_host_migrate_legacy(crate::paired_host::service::MigrationRequest {
                relay_origin: origin.clone(), machine_id: host.machine_id.clone(), display_label: host.display_label.clone(), device_token: Secret(token.clone()),
            }).await.map_err(|e| anyhow::anyhow!(e.code))?;
            ensure!(receipt.host_id == host.host_id && receipt.generation == host.generation, "restart credential failed real relay authentication");
            ensure!(client.paired_host_capabilities().await.map_err(|e| anyhow::anyhow!(e.code))?["pairedDaemonProxyV1"] == true, "proxy not advertised");
            Ok(())
        }).await?;
        ensure!(fs::read(root.join("data/paired-hosts.v1.json"))? == bytes, "restart rewrote private authority");
        let injected: anyhow::Result<()> = with_process(root, async |_client| { anyhow::bail!("injected action failure") }).await;
        ensure!(injected.is_err(), "injected failure disappeared");
        eprintln!("A13 separate_process_restart=true real_relay_pair=true exact_generation={} private_bytes_preserved=true injected_failure_reaped=true", host.generation.0);
        Ok::<_, anyhow::Error>(())
    }.await;
    drop(guard);
    tasks.shutdown().await;
    let refused = tokio::net::TcpStream::connect(address).await.is_err();
    eprintln!("A13 relay listener_refused={refused}");
    ensure!(refused, "relay survived cleanup");
    result
}
