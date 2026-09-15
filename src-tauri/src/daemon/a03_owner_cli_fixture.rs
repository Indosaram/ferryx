//! In-crate owner CLI surface fixture. Wire under daemon::server with cfg(test, unix).
//! The outer test launches a dedicated test-binary process with private roots.
//! Build ferryx-cli before running; no canonical daemon discovery is permitted.
use super::*;
use anyhow::{ensure, Context};
use serde::Deserialize;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

#[path = "a13_issuer_repeat_fixture.rs"]
mod issuer_repeat;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Exchange {
    token: String,
    device: crate::remote::auth::DeviceInfo,
    machine_id: String,
}

// Deliberately no Debug: a failed assertion must not print issued credentials.
struct CliOutput {
    pin: String,
    authority: String,
}

// Owned by the outer process, so inner runtime failure cannot strand a CLI.
struct OwnedGroup(u32);
impl OwnedGroup {
    fn cleanup(&self) -> std::io::Result<()> {
        let group = format!("-{}", self.0);
        let alive = std::process::Command::new("/bin/kill")
            .args(["-0", "--", &group]).stdout(Stdio::null()).stderr(Stdio::null()).status()?;
        if alive.success() {
            let killed = std::process::Command::new("/bin/kill")
                .args(["-KILL", "--", &group]).status()?;
            if !killed.success() { return Err(std::io::Error::other("owned group cleanup failed")); }
        }
        Ok(())
    }
}
impl Drop for OwnedGroup {
    fn drop(&mut self) {
        if let Err(error) = self.cleanup() { eprintln!("A03 CLEANUP ERROR: {error}"); }
    }
}

async fn owner_cli(binary: &Path, root: &Path, machine: bool) -> anyhow::Result<CliOutput> {
    let mut command = tokio::process::Command::new(binary);
    command.arg("pair").arg("generate");
    if machine { command.args(["--access", "machine"]); }
    command.env("HOME", root.join("home"))
        .env("FERRYX_DATA_DIR", root.join("data"))
        .env("FERRYX_RUNTIME_DIR", root.join("runtime"))
        .env_remove("FERRYX_MACHINE_TOKEN")
        .env_remove("FERRYX_RELAY_URL")
        .stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    let child = command.spawn().context("launch private owner CLI")?;
    let pid = child.id().context("owned CLI PID")?;
    let output = tokio::time::timeout(Duration::from_secs(20), child.wait_with_output()).await;

    let output = output.context("bounded CLI completion")??;
    ensure!(output.status.success(), "owner CLI failed (output withheld)");
    let stdout = String::from_utf8(output.stdout).context("CLI stdout encoding")?;
    let authority = String::from_utf8(output.stderr).context("CLI stderr encoding")?;
    let pin = stdout.lines().next().context("CLI PIN missing")?.to_owned();
    ensure!(pin.len() == 6 && pin.bytes().all(|b| b.is_ascii_digit()), "invalid CLI PIN shape");
    println!("A03 CLI pid={pid} exited=0 reaped=true");
    Ok(CliOutput { pin, authority })
}

async fn handshake(socket: &Path) -> anyhow::Result<()> {
    let stream = tokio::net::UnixStream::connect(socket).await?;
    let (read, mut write) = stream.into_split();
    write.write_all(b"{\"type\":\"handshake\",\"version\":3}\n").await?;
    let mut reader = tokio::io::BufReader::new(read);
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut line)).await??;
    let response: DaemonResponse = serde_json::from_str(&line)?;
    ensure!(matches!(response, DaemonResponse::HandshakeOk { version: DAEMON_PROTOCOL_VERSION, pid, .. }
        if pid == std::process::id()), "fixture PID handshake mismatch");
    write.write_all(b"{\"type\":\"getCapabilities\"}\n").await?;
    line.clear();
    tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut line)).await??;
    let response: DaemonResponse = serde_json::from_str(&line)?;
    ensure!(matches!(response, DaemonResponse::CapabilitiesOk { capabilities }
        if capabilities == vec!["machinePairingV1", "pairedHostInventoryV1"]), "local capabilities mismatch");
    write.write_all(b"{\"type\":\"ping\"}\n").await?;
    line.clear();
    tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut line)).await??;
    let response: DaemonResponse = serde_json::from_str(&line)?;
    ensure!(matches!(response, DaemonResponse::Pong), "legacy ping refused");
    println!("A03 UDS protocol=3 exact_owner_pid={} capabilities=machinePairingV1,pairedHostInventoryV1 legacy_ping=pong", std::process::id());
    Ok(())
}

async fn scenario(binary: &Path, root: &Path, machine: bool) -> anyhow::Result<()> {
    use crate::remote::auth::DeviceAccessScope;
    let runtime = root.join("runtime");
    for path in [&runtime, &root.join("home"), &root.join("data/remote")] {
        std::fs::create_dir_all(path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    let socket = runtime.join("daemon.sock");
    let listener = tokio::net::UnixListener::bind(&socket)?;
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
    let server = Arc::new(DaemonServer::new_with_paths(
        Some(root.join("data/remote/config.json")),
        Some(root.join("data/remote/remote-auth.json")),
    ));
    let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel();
    let owner = Arc::clone(&server);
    let uds = tokio::spawn(async move {
        let mut clients = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                _ = &mut stop_rx => break,
                accepted = listener.accept() => {
                    let (stream, _) = accepted?;
                    let owner = Arc::clone(&owner);
                    clients.spawn(async move { owner.handle_client(stream).await });
                }
            }
        }
        clients.shutdown().await;
        Ok::<_, std::io::Error>(())
    });
    let relay_state = crate::remote::relay_server::RelayState::new_with_key_store(
        vec![], root.join("relay-keys.json"),
    ).map_err(anyhow::Error::msg)?;
    let relay_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let relay_url = format!("http://{}", relay_listener.local_addr()?);
    let (relay_stop, relay_shutdown) = tokio::sync::oneshot::channel();
    let relay = tokio::spawn(async move {
        axum::serve(relay_listener, crate::remote::relay_server::relay_router(relay_state)
            .into_make_service_with_connect_info::<std::net::SocketAddr>())
            .with_graceful_shutdown(async { let _ = relay_shutdown.await; }).await
    });
    let result = async {
        server.configure_gateway(RemoteGatewayConfig {
            mode: RemoteNetworkMode::Relay, port: 0,
            relay_url: Some(relay_url.clone()), ..Default::default()
        }).await.map_err(anyhow::Error::msg)?;
        handshake(&socket).await?;
        let issued = owner_cli(binary, root, machine).await?;
        let expected = if machine { DeviceAccessScope::Machine } else { DeviceAccessScope::Mirror };
        // Prose is captured for human review, not pinned by a wording assertion.
        ensure!(!issued.authority.contains(&issued.pin), "authority output leaks PIN");
        let authority = issued.authority.lines().find(|line| line.starts_with("Access:"))
            .context("CLI authority display missing")?;
        println!("A03 AUTHORITY {authority}");
        let client = reqwest::Client::builder().no_proxy()
            .redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(8)).build()?;
        let response = client.post(format!("{relay_url}/api/v1/pair/exchange"))
            .header("content-type", "application/json")
            .body(serde_json::to_vec(&serde_json::json!({"pin": issued.pin, "deviceName": "A03 owner CLI",
                "accessScope": "machine", "permission": "control", "clientType": "desktop"}))?)
            .send().await?;
        ensure!(response.status().is_success(), "real relay exchange failed");
        let exchange: Exchange = serde_json::from_slice(&response.bytes().await?)?;
        ensure!(exchange.device.access_scope == expected, "exchange scope upgraded");
        ensure!(exchange.device.permission == DevicePermission::Control, "permission mismatch");
        issuer_repeat::repeat_after_redemption(binary, root, &relay_url).await?;
        let address = server.remote_state.bound_address.read().clone().context("gateway bound address")?;
        let gateway = format!("http://{address}");
        let capability_url = format!("{gateway}/api/v1/capabilities");
        ensure!(client.get(&capability_url).send().await?.status() == 401, "anonymous capability admitted");
        let caps = client.get(&capability_url).bearer_auth(&exchange.token).send().await?;
        ensure!(caps.status() == 200, "authenticated capability refused");
        ensure!(caps.headers().get("cache-control").is_some_and(|v| v == "no-store"), "capability cacheable");
        let value: serde_json::Value = serde_json::from_slice(&caps.bytes().await?)?;
        let _: crate::remote::machine_protocol::Capabilities = serde_json::from_value(value.clone())?;
        ensure!(value["machineId"] == exchange.machine_id, "machine identity changed");
        ensure!(value["accessScope"] == serde_json::to_value(expected)?, "capability scope mismatch");
        let expected_capabilities = if machine { serde_json::json!(["directoryBrowseV1", "machineWorkspaceV1", "managedWorktreesV1", "terminalCreateV1"]) } else { serde_json::json!([]) };
        ensure!(value["capabilities"] == expected_capabilities, "machine capability contract mismatch");
        ensure!(!value.to_string().contains(&root.to_string_lossy().to_string()), "capability leaks private path");
        ensure!(client.get(&capability_url).bearer_auth("invalid-credential").send().await?.status() == 401,
            "malformed credential admitted");
        let browse_root = root.join("home/owner-browse");
        std::fs::create_dir_all(browse_root.join("visible"))?;
        std::fs::create_dir_all(browse_root.join(".hidden"))?;
        std::fs::write(browse_root.join("sentinel"), b"unchanged")?;
        let response = client.get(format!("{gateway}/api/v1/fs/directories"))
            .query(&[("path", browse_root.to_str().context("private path encoding")?)])
            .bearer_auth(&exchange.token).send().await?;
        ensure!(response.status().as_u16() == if machine { 200 } else { 403 }, "directory admission mismatch");
        ensure!(response.headers().get("cache-control").is_some_and(|v| v == "no-store"), "directory response cacheable");
        let bytes = response.bytes().await?;
        if machine {
            let listing: crate::remote::machine_protocol::Directories = serde_json::from_slice(&bytes)?;
            ensure!(Path::new(&listing.path) == browse_root.canonicalize()?, "directory root mismatch");
            ensure!(Path::new(&listing.home_path) == root.join("home").canonicalize()?, "directory home mismatch");
            ensure!(listing.parent_path.as_deref().map(Path::new) == Some(root.join("home").canonicalize()?.as_path()), "directory parent mismatch");
            ensure!(!listing.truncated && listing.entries.len() == 1, "directory projection incomplete");
            ensure!(listing.entries[0].name == "visible" && !listing.entries[0].hidden && Path::new(&listing.entries[0].path) == browse_root.join("visible").canonicalize()?, "directory entry mismatch");
        } else {
            let error: crate::remote::machine_protocol::ErrorEnvelope = serde_json::from_slice(&bytes)?;
            ensure!(error.error.code == "MACHINE_ACCESS_REQUIRED", "mirror directory scope refusal mismatch");
            ensure!(!String::from_utf8_lossy(&bytes).contains(browse_root.to_str().context("private path encoding")?), "mirror directory error leaks path");
        }
        ensure!(std::fs::read(browse_root.join("sentinel"))? == b"unchanged", "browse changed sentinel");
        let response = client.post(format!("{gateway}/api/v1/sessions"))
            .bearer_auth(&exchange.token).send().await?;
        ensure!(response.status().as_u16() == if machine { 400 } else { 403 }, "session admission mismatch");
        let error: crate::remote::machine_protocol::ErrorEnvelope = serde_json::from_slice(&response.bytes().await?)?;
        ensure!(error.error.code == if machine { "INVALID_REQUEST" } else { "MACHINE_ACCESS_REQUIRED" }, "session refusal mismatch");
        println!("A03 DIRECTORY scope={expected:?} status={} native_projection_verified={} sentinel_unchanged=true sessions_status={}", if machine {200} else {403}, machine, if machine {400} else {403});
        ensure!(server.remote_state.auth_manager.revoke_device(&exchange.device.id), "revoke failed");
        let probes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = Arc::clone(&probes);
        *server.remote_state.identity_probe.write() = Some(Arc::new(move || {
            observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }));
        ensure!(client.get(&capability_url).bearer_auth(&exchange.token).send().await?.status() == 401,
            "revoked capability admitted");
        ensure!(probes.load(std::sync::atomic::Ordering::SeqCst) == 0, "revoked request probed identity");
        handshake(&socket).await?;
        println!("A03 CAPABILITIES {value}");
        println!("A03 SURFACE scope={expected:?} permission=Control relay_exchange=200 capabilities=200 anonymous=401 revoked=401 machine_capabilities={expected_capabilities}");
        Ok::<_, anyhow::Error>(())
    }.await;
    let stopped = server.configure_gateway(RemoteGatewayConfig::default()).await;
    let uds_signalled = stop_tx.send(());
    let relay_signalled = relay_stop.send(());
    let uds_joined = tokio::time::timeout(Duration::from_secs(5), uds).await;
    let relay_joined = tokio::time::timeout(Duration::from_secs(5), relay).await;
    stopped.map_err(anyhow::Error::msg)?;
    ensure!(uds_signalled.is_ok() && relay_signalled.is_ok(), "listener shutdown channel failed");
    uds_joined???;
    relay_joined???;
    std::fs::remove_file(socket)?;
    result
}

#[test]
fn a03_private_owner_cli_surface() -> anyhow::Result<()> {
    // This entry must be explicitly invoked in an isolated process, not skipped
    // into a falsely green normal suite when fixture prerequisites are absent.
    let Some(root) = std::env::var_os("A03_PRIVATE_ROOT") else {
        let root = tempfile::Builder::new().prefix("a03-cli-").tempdir_in("/tmp")?;
        let private_root = root.path().canonicalize()?;
        for directory in ["home", "sessions", "runtime", "data", "xdg-config", "xdg-cache", "xdg-data", "tmp"] {
            std::fs::create_dir(private_root.join(directory))?;
        }
        let exe = std::env::current_exe()?;
        let binary = exe.parent().and_then(Path::parent).context("target debug directory")?.join("ferryx-cli");
        ensure!(binary.is_file(), "build ferryx-cli before the owner fixture");
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        let result = runtime.block_on(async {
            let mut command = tokio::process::Command::new(exe);
            command.args(["daemon::server::a03_owner_cli_fixture::a03_private_owner_cli_surface", "--exact", "--nocapture"])
                .env_clear().env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
                .env("A03_PRIVATE_ROOT", &private_root).env("A03_CLI_BINARY", binary)
                .env("FERRYX_SESSION_DIR", private_root.join("sessions"))
                .env("FERRYX_AGENT_STATE_SOCKET", private_root.join("runtime/agent.sock"))
                .env("XDG_CONFIG_HOME", private_root.join("xdg-config"))
                .env("XDG_CACHE_HOME", private_root.join("xdg-cache"))
                .env("XDG_DATA_HOME", private_root.join("xdg-data"))
                .env("XDG_RUNTIME_DIR", private_root.join("runtime"))
                .env("TMPDIR", private_root.join("tmp"))
                .env("TMP", private_root.join("tmp"))
                .env("TEMP", private_root.join("tmp"))
                .env("HOME", private_root.join("home"))
                .env("FERRYX_DATA_DIR", private_root.join("data"))
                .env("FERRYX_RUNTIME_DIR", private_root.join("runtime"))
                .env_remove("FERRYX_MACHINE_TOKEN").env_remove("FERRYX_RELAY_URL")
                .kill_on_drop(true);
            if let Some(libraries) = std::env::var_os("DYLD_FALLBACK_LIBRARY_PATH") {
                command.env("DYLD_FALLBACK_LIBRARY_PATH", libraries);
            }
            command.as_std_mut().process_group(0);
            let mut child = command.spawn()?;
            let pid = child.id().context("fixture child PID")?;
            let group = OwnedGroup(pid);
            let status = match tokio::time::timeout(Duration::from_secs(90), child.wait()).await {
                Ok(status) => status?,
                Err(error) => { group.cleanup()?; child.wait().await?; return Err(error.into()); }
            };
            group.cleanup()?;
            println!("A03 CLEANUP exact_fixture_pid={pid} reaped=true exit={status}");
            ensure!(status.success(), "isolated owner CLI scenario failed");
            Ok::<_, anyhow::Error>(())
        });
        root.close()?;
        println!("A03 CLEANUP outer_private_root_removed=true");
        return result;
    };
    let root = PathBuf::from(root).canonicalize()?;
    for key in ["HOME", "FERRYX_DATA_DIR", "FERRYX_RUNTIME_DIR"] {
        let value = PathBuf::from(std::env::var_os(key).with_context(|| format!("missing {key}"))?);
        ensure!(value.starts_with(&root) && value != root, "ambient fixture path refused");
    }
    ensure!(std::env::var_os("FERRYX_MACHINE_TOKEN").is_none(), "ambient relay credential refused");
    let binary = PathBuf::from(std::env::var_os("A03_CLI_BINARY").context("fresh CLI binary required")?);
    for machine in [false, true] {
        let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build()?;
        let result = runtime.block_on(scenario(&binary, &root, machine));
        // Drops all otherwise detached gateway tasks before removing private data.
        runtime.shutdown_timeout(Duration::from_secs(5));
        result?;
    }
    println!("A03 CLEANUP fixture_pid={} runtimes_stopped=true sockets_removed=true", std::process::id());
    Ok(())
}
