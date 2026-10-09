#![cfg(unix)]

use std::{
    env,
    fs::{self, OpenOptions},
    io::Write,
    net::SocketAddr,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::{atomic::Ordering, Arc, Mutex},
    time::Duration,
};

use anyhow::{bail, Context, Result};
use axum::{extract::State, routing::get, Json, Router};
use ferryx_lib::{
    daemon::server::DaemonServer,
    remote::{auth::DevicePermission, protocol::{RemoteActiveDesktopSelection, RemoteTerminalTabInfo}, server::create_remote_router},
};
use rand::{distributions::Alphanumeric, Rng};
use serde_json::{json, Value};
use tokio::{sync::{broadcast, oneshot}, task::JoinHandle};

struct GatewayGuard {
    address: SocketAddr,
    stop: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

impl GatewayGuard {
    async fn start(state: Arc<ferryx_lib::remote::state::RemoteGatewayState>) -> Result<Self> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let router = create_remote_router(state);
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async { let _ = stopped.await; })
                .await
                .expect("fixture gateway failed");
        });
        Ok(Self { address, stop: Some(stop), task })
    }

    async fn shutdown(mut self) -> Result<()> {
        if let Some(stop) = self.stop.take() { let _ = stop.send(()); }
        tokio::time::timeout(Duration::from_secs(5), &mut self.task).await.context("gateway shutdown timed out")??;
        Ok(())
    }
}

struct PtyGuard {
    id: String,
    service: Arc<ferryx_lib::terminal::TerminalService>,
    pty: Arc<ferryx_lib::terminal::PtySession>,
    output_task: Option<JoinHandle<()>>,
    receipt: Arc<Mutex<PtyReceipt>>,
}

#[derive(Default)]
struct PtyReceipt {
    output: Vec<u8>,
    observer_lagged: bool,
}

impl PtyGuard {
    async fn spawn(state: Arc<ferryx_lib::remote::state::RemoteGatewayState>) -> Result<Self> {
        let service = Arc::clone(&state.terminal_service);
        let (id, _lifecycle_watch) = tokio::task::spawn_blocking(move || service.spawn_shell(80, 24))
            .await.context("join PTY spawn task")?.context("spawn PTY through blocking pool")?;
        let pty = Arc::clone(&state.terminal_service).get_session(&id).context("get spawned PTY")?;
        let receipt = Arc::new(Mutex::new(PtyReceipt::default()));
        let (history, mut receiver) = state.terminal_service.output_hub().subscribe(&id).context("subscribe PTY output observer")?;
        receipt.lock().unwrap().output = history;
        let receipt_for_task = Arc::clone(&receipt);
        let output_task = tokio::spawn(async move {
            loop {
                match receiver.recv().await {
                    Ok(chunk) => receipt_for_task.lock().unwrap().output.extend_from_slice(&chunk),
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        receipt_for_task.lock().unwrap().observer_lagged = true;
                        break;
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
        Ok(Self { id, service: Arc::clone(&state.terminal_service), pty, output_task: Some(output_task), receipt })
    }

    async fn close(mut self) -> Value {
        let output_observer_stopped = if let Some(output_task) = self.output_task.take() {
            output_task.abort();
            matches!(tokio::time::timeout(Duration::from_secs(5), output_task).await, Ok(Err(error)) if error.is_cancelled())
        } else { true };
        let close = tokio::time::timeout(Duration::from_secs(10), self.service.close_session(&self.id)).await;
        let closed = matches!(close, Ok(Ok(())));
        let (output_observer_lagged, observed_output_bytes) = {
            let receipt = self.receipt.lock().unwrap();
            (receipt.observer_lagged, receipt.output.len())
        };
        json!({
            "sessionId": self.id,
            "closed": closed,
            "sessionRemoved": !self.service.list_sessions().iter().any(|id| id == &self.id),
            "reaped": self.pty.is_reaped(),
            "readerFinished": self.pty.is_reader_finished(),
            "outputObserverStopped": output_observer_stopped,
            "outputObserverLagged": output_observer_lagged,
            "observedOutputBytes": observed_output_bytes,
            "closeError": match close { Ok(Err(error)) => Some(error.to_string()), Err(_) => Some("close timeout".into()), _ => None }
        })
    }
}

#[derive(Clone)]
struct ObserverState {
    nonce: String,
    session_id: String,
    receipt: Arc<Mutex<PtyReceipt>>,
    session_closed: Arc<std::sync::atomic::AtomicBool>,
    pty: Arc<ferryx_lib::terminal::PtySession>,
    terminal: Arc<ferryx_lib::terminal::TerminalService>,
    gateway_address: SocketAddr,
    host_write_bytes: Arc<std::sync::atomic::AtomicU64>,
}

async fn observe(
    State(state): State<ObserverState>,
    axum::extract::Query(query): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, axum::http::StatusCode> {
    if query.get("nonce").map(String::as_str) != Some(state.nonce.as_str())
        || query.get("sessionId").map(String::as_str) != Some(state.session_id.as_str())
    { return Err(axum::http::StatusCode::FORBIDDEN); }
    let action = query.get("action").map(String::as_str).unwrap_or("snapshot");
    if action == "host-write" {
        let marker = query.get("marker").ok_or(axum::http::StatusCode::BAD_REQUEST)?;
        if !marker.starts_with("SHR03_HOST_") || !marker.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_') {
            return Err(axum::http::StatusCode::BAD_REQUEST);
        }
        let command = format!("printf '%s\\n' '{marker}'\r");
        state.terminal.write_input(&state.session_id, command.as_bytes()).map_err(|_| axum::http::StatusCode::CONFLICT)?;
        state.host_write_bytes.fetch_add(command.len() as u64, Ordering::AcqRel);
        return Ok(Json(json!({ "accepted": true, "hostWriteBytes": state.host_write_bytes.load(Ordering::Acquire), "marker": marker })));
    }
    if action != "snapshot" { return Err(axum::http::StatusCode::BAD_REQUEST); }
    let (output, observer_lagged) = {
        let receipt = state.receipt.lock().unwrap();
        (receipt.output.clone(), receipt.observer_lagged)
    };
    let attach_token = query.get("attachToken").map(String::as_str).unwrap_or_default();
    let attach_status = if attach_token.is_empty() {
        None
    } else {
        let client = reqwest::Client::builder().no_proxy().build().map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
        let target = format!("/api/v1/terminal/{}", state.session_id);
        let response = client.post(format!("http://{}/api/v1/socket-ticket", state.gateway_address))
            .bearer_auth(attach_token)
            .json(&json!({"target":target}))
            .send().await.map_err(|_| axum::http::StatusCode::BAD_GATEWAY)?;
        Some(response.status().as_u16())
    };
    Ok(Json(json!({
        "sessionId": state.session_id,
        "outputBytes": output.len(),
        "output": String::from_utf8_lossy(&output),
        "outputObserverLagged": observer_lagged,
        "sessionClosed": state.session_closed.load(Ordering::Acquire),
        "ptyReaped": state.pty.is_reaped(),
        "readerFinished": state.pty.is_reader_finished(),
        "sessionPresent": state.terminal.list_sessions().contains(&state.session_id),
        "hostWriteBytes": state.host_write_bytes.load(Ordering::Acquire),
        "attachStatus": attach_status
    })))
}

fn write_private(path: &Path, value: &Value) -> Result<()> {
    if let Some(parent) = path.parent() { fs::create_dir_all(parent)?; }
    let mut file = OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)?;
    serde_json::to_writer(&mut file, value)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

fn required(name: &str) -> Result<String> {
    env::var(name).with_context(|| format!("missing {name}"))
}

#[tokio::main]
async fn main() -> Result<()> {
    let root = PathBuf::from(required("FERRYX_QA_FIXTURE_ROOT")?);
    let fixture_path = PathBuf::from(required("FERRYX_QA_FIXTURE_FILE")?);
    fs::create_dir_all(&root)?;
    let root = root.canonicalize()?;
    if root == Path::new("/") || root.parent().is_none() { bail!("unsafe fixture root"); }
    let data = root.join("data");
    fs::create_dir_all(&data)?;
    fs::create_dir_all(root.join("runtime"))?;
    let daemon = Arc::new(DaemonServer::new_with_paths(Some(data.join("config.json")), Some(data.join("auth.json"))));
    let state = Arc::clone(daemon.remote_state());
    let session = PtyGuard::spawn(Arc::clone(&state)).await?;
    let pin = state.auth_manager.create_pairing_code(DevicePermission::Control);
    let (host_token, host_device) = state.auth_manager.exchange_pairing_code(&pin, "private-share-fixture-host")?;
    state.set_active_selection(RemoteActiveDesktopSelection {
        workspace_id: Some("default".into()),
        session_id: Some(session.id.clone()),
        tab_id: Some(session.id.clone()),
        terminal_tabs: vec![RemoteTerminalTabInfo {
            id: session.id.clone(),
            label: "Share fixture".into(),
            session_id: Some(session.id.clone()),
            ..Default::default()
        }],
        ..Default::default()
    });
    let gateway = GatewayGuard::start(state).await?;

    let session_closed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let terminal = Arc::clone(&daemon.remote_state().terminal_service);
    let output_hub = Arc::clone(terminal.output_hub());
    let observer_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let observer_address = observer_listener.local_addr()?;
    let nonce: String = rand::thread_rng().sample_iter(&Alphanumeric).take(48).map(char::from).collect();
    let observer_state = ObserverState {
        nonce: nonce.clone(),
        session_id: session.id.clone(),
        receipt: Arc::clone(&session.receipt),
        session_closed: Arc::clone(&session_closed),
        pty: Arc::clone(&session.pty),
        terminal: Arc::clone(&session.service),
        gateway_address: gateway.address,
        host_write_bytes: Arc::new(std::sync::atomic::AtomicU64::new(0)),
    };
    let (observer_stop, observer_stopped) = oneshot::channel();
    let observer_task = tokio::spawn(async move {
        let app = Router::new().route("/observe", get(observe).with_state(observer_state));
        axum::serve(observer_listener, app).with_graceful_shutdown(async { let _ = observer_stopped.await; }).await
            .expect("fixture observer failed");
    });
    let fixture = json!({
        "storageKey": "ferryx_remote_token",
        "hostToken": host_token,
        "sessionId": session.id,
        "primaryCommand": "printf 'SHARE_FIXTURE_INPUT\\n'",
        "alternateCommand": "printf '\\033[?1049hSHARE_FIXTURE_ALT\\033[?1049l'",
        "modeCommands": {"sgr":"printf '\\033[31mX\\033[0m'","x10":"printf '\\033[?1000hX\\033[?1000l'","csi":"printf '\\033[1;1H'","ss3":"printf '\\033O1;1H'"},
        "hostDeviceId": host_device.id,
        "selectors": {"primary":"[data-testid='remote-terminal']","alternate":"[data-testid='remote-terminal']","viewportShift":"[data-testid='remote-terminal']","touchCell":"[data-testid='remote-terminal']","shareButton":"[data-testid='share-session-button']","shareCreate":"button","shareReadonly":"button","presenceBadge":"[data-testid='session-presence-badge']","readonlyBadge":"[data-testid='remote-terminal-readonly']"},
        "presenceEvent":"session_presence_changed",
        "fixtureControl":{"url":format!("http://{observer_address}/observe"),"nonce":nonce},
        "telemetry":{"outputUrl":format!("http://{observer_address}/observe?nonce={}&sessionId={}",nonce,session.id)}
    });
    write_private(&fixture_path, &fixture)?;
    println!("FERRYX_QA_FIXTURE_READY http://{}", gateway.address);
    tokio::signal::ctrl_c().await?;

    session_closed.store(true, Ordering::Release);
    let _ = observer_stop.send(());
    let observer_stopped = matches!(tokio::time::timeout(Duration::from_secs(5), observer_task).await, Ok(Ok(())));
    let session_receipt = session.close().await;
    let output_observer_stopped = session_receipt["outputObserverStopped"].as_bool().unwrap_or(false);
    let gateway_stopped = gateway.shutdown().await.is_ok();
    let receipt = json!({"gatewayStopped":gateway_stopped,"observerStopped":observer_stopped,"outputObserverStopped":output_observer_stopped,"session":session_receipt,"fixtureRoot":root,"rootRemoval":"parent owns removal after receipt validation"});
    fs::write(root.join("cleanup-receipts.json"), serde_json::to_vec_pretty(&receipt)?)?;
    if !gateway_stopped || !observer_stopped || !output_observer_stopped || !receipt["session"]["closed"].as_bool().unwrap_or(false)
        || !receipt["session"]["outputObserverStopped"].as_bool().unwrap_or(false)
        || !receipt["session"]["sessionRemoved"].as_bool().unwrap_or(false)
        || !receipt["session"]["reaped"].as_bool().unwrap_or(false)
        || !receipt["session"]["readerFinished"].as_bool().unwrap_or(false)
    { bail!("fixture teardown receipt did not prove clean shutdown: {receipt}"); }
    Ok(())
}
