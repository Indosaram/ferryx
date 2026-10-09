//! Isolated source-only acceptance fixture. Build/run only on the designated verifier.
//! Product routes are never replaced: failures in the composed product stay failures.
use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use ferryx_lib::account::offer_sink::apply_grant_offer;
use ferryx_lib::daemon::server::DaemonServer;
use ferryx_lib::remote::account_protocol::{AccountGrantOffer, AccountGrantOfferEnvelope, AccountGrantScope};
use ferryx_lib::remote::attach_identity::load_or_generate_attach_identity;
use ferryx_lib::remote::managed_chat_api::{
    register_managed_provider, unregister_managed_provider, CallbackKind, CallbackStatus,
    LiveCallbackEntry, ManagedChatProvider, LIVE_CALLBACKS,
};
use ferryx_lib::remote::server::create_remote_router;
use ferryx_lib::remote::sealed_offer::seal_offer;
use ferryx_lib::remote::{DeviceAccessScope, DevicePermission, RemoteActiveDesktopSelection, RemoteTerminalTabInfo};
use ferryx_lib::scoped_contracts::{DeliveryReceipt, DeliveryStage, TargetRef};
use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{io::Write, net::SocketAddr, sync::Arc, time::{Duration, Instant}};
use tokio::io::{AsyncBufReadExt, BufReader};

struct AcceptanceFixtureProvider {
    stage: Mutex<DeliveryStage>,
    reject_next: Mutex<bool>,
}

#[async_trait::async_trait]
impl ManagedChatProvider for AcceptanceFixtureProvider {
    async fn send_turn(&self, target: &TargetRef, input: Vec<Value>) -> Result<DeliveryReceipt, String> {
        let rejected = std::mem::take(&mut *self.reject_next.lock());
        output(json!({"event":"providerSend", "sessionId":target.backend_session_id,
            "inputCount":input.len(), "rejected":rejected})).map_err(|e| e.to_string())?;
        if rejected { return Err("fixture rejected this turn".into()); }
        // The production route must bind the caller requestId; providers do not receive it.
        Ok(DeliveryReceipt { request_id: uuid::Uuid::new_v4().to_string(),
            target: target.clone(), stage: *self.stage.lock() })
    }
    async fn reply_callback(&self, callback_id: Value, thread_id: &str, turn_id: &str, _result: Value) -> Result<(), String> {
        // Canonical LIVE_CALLBACKS validates freshness before invoking the provider.
        output(json!({"event":"providerReply", "callbackId":callback_id,
            "threadId":thread_id,"turnId":turn_id})).map_err(|e| e.to_string())
    }
    async fn stop_agent(&self, target: &TargetRef) -> Result<(), String> {
        output(json!({"event":"providerStop","sessionId":target.backend_session_id})).map_err(|e| e.to_string())
    }
}

#[derive(Deserialize)]
#[serde(tag="type", rename_all="camelCase", deny_unknown_fields)]
enum Command {
    Focus { index: usize },
    SetDeliveryStage { stage: DeliveryStage },
    RejectNext,
    UseFixture { index: usize },
    StartManaged { index: usize },
    RewriteHistory { index: usize, marker: String },
    PublishCallback { index: usize, id: String, #[serde(rename="threadId")] thread_id: String,
        #[serde(rename="turnId")] turn_id: String, text: String },
    Shutdown,
}

fn output(value: Value) -> Result<()> {
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "{value}")?;
    stdout.flush()?;
    Ok(())
}

async fn post(client: &reqwest::Client, base: &str, token: &str, path: &str, body: Value) -> Result<Value> {
    Ok(client.post(format!("{base}{path}")).bearer_auth(token).json(&body)
        .send().await?.error_for_status()?.json().await?)
}

async fn write_history(thread: String, marker: String) -> Result<()> {
    tokio::task::spawn_blocking(move || -> Result<()> {
        let directory = std::path::PathBuf::from(std::env::var("HOME")?)
            .join(".omo/agent/sessions/herdr-isolated");
        std::fs::create_dir_all(&directory)?;
        let records = [json!({"type":"session","id":thread}),
            json!({"type":"message","id":uuid::Uuid::new_v4(),"message":{
                "role":"assistant","content":[{"type":"text","text":marker}]}})];
        std::fs::write(directory.join(format!("{thread}.jsonl")),
            records.iter().map(Value::to_string).collect::<Vec<_>>().join("\n") + "\n")?;
        Ok(())
    }).await?
}

async fn controlled_child() -> Result<()> {
    let thread = uuid::Uuid::new_v4().to_string();
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    while let Some(line) = lines.next_line().await? {
        let request: Value = serde_json::from_str(&line)?;
        match request["method"].as_str() {
            Some("initialize") => output(json!({"id":request["id"],"result":{"userAgent":"herdr-controlled-child"}}))?,
            Some("initialized") => {},
            Some("thread/start") => {
                write_history(thread.clone(), format!("HERDR_HISTORY_{thread}")).await?;
                output(json!({"id":request["id"],"result":{"thread":{"id":thread}}}))?;
            }
            Some("turn/start") => {
                let turn = uuid::Uuid::new_v4().to_string();
                output(json!({"id":request["id"],"result":{"turn":{"id":turn}}}))?;
                output(json!({"id":format!("approval-{turn}"),"method":"item/commandExecution/requestApproval",
                    "params":{"threadId":thread,"turnId":turn,"message":"HERDR_CONTROLLED_APPROVAL"}}))?;
            }
            None if request.get("result").is_some() => {},
            _ => return Err(anyhow!("Unexpected controlled child RPC")),
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    if std::env::var("FERRYX_QA_ISOLATED").as_deref() != Ok("1") {
        return Err(anyhow!("FERRYX_QA_ISOLATED=1 and isolated directory overrides are required"));
    }
    for name in ["FERRYX_DATA_DIR", "FERRYX_RUNTIME_DIR", "FERRYX_SESSION_DIR", "HOME"] {
        std::env::var(name).with_context(|| format!("missing isolation override {name}"))?;
    }
    if std::env::args().nth(1).as_deref() == Some("app-server") {
        return controlled_child().await;
    }
    let (root, owner) = tokio::task::spawn_blocking(|| -> Result<_> {
        let root = tempfile::tempdir()?;
        let status = std::process::Command::new("git").args(["init", "--quiet"]).arg(root.path()).status()?;
        if !status.success() { return Err(anyhow!("isolated git init failed")); }
        std::fs::write(root.path().join("result.txt"), b"HERDR_RESULT_FILE_ROUND_TRIP\n")?;
        let dag_runs = root.path().join(".omo/senpi-task/dag/runs");
        std::fs::create_dir_all(&dag_runs)?;
        std::fs::write(dag_runs.join("herdr-acceptance.json"), serde_json::to_vec(&json!({
            "schemaVersion": 1,
            "checkpointSeq": 1,
            "runId": "dag_herdr_acceptance",
            "runKey": "herdr-acceptance",
            "name": "Herdr mobile acceptance",
            "status": "completed",
            "nodes": [{
                "id": "step-1",
                "label": "Acceptance result",
                "state": "completed",
                "route": {"kind": "category", "category": "acceptance"},
                "resultArtifact": {"relativePath": "result.txt"}
            }],
            "edges": [],
            "waves": [],
            "criticalPath": [],
            "bottlenecks": []
        }))?)?;
        std::fs::write(root.path().join("remote_sessions.json"), b"{\"remoteSessions\":[]}")?;
        let owner = DaemonServer::new_with_paths(Some(root.path().join("config")), Some(root.path().join("auth")));
        Ok((root, owner))
    }).await??;
    let state = owner.remote_state().clone();
    let pairing_state = state.clone();
    let pairing_root = root.path().to_path_buf();
    let (token, device) = tokio::task::spawn_blocking(move || -> Result<_> {
        let attach = load_or_generate_attach_identity(&pairing_root).map_err(|e| anyhow!(e))?;
        let identity = ferryx_lib::remote::auth::load_or_generate_machine_identity(&pairing_root)
            .map_err(|e| anyhow!(e))?;
        let epoch = uuid::Uuid::new_v4().to_string();
        let capability = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
        let offer = AccountGrantOffer {
            grant_id: uuid::Uuid::new_v4().to_string(),
            machine_id: identity.machine_id.clone(), enrollment_epoch: epoch.clone(),
            pairing_token: capability.clone(), device_label: "herdr-isolated-controller".into(),
            installation_id: uuid::Uuid::new_v4().to_string(), grant_scope: AccountGrantScope::Machine,
            expires_at: now + 600, device_attach_public_key: attach.public_key.clone(),
        };
        let sealed = seal_offer(&attach.public_key, &identity.machine_id, &epoch, &serde_json::to_vec(&offer)?)
            .map_err(|e| anyhow!("seal fixture grant: {e:?}"))?;
        let envelope = AccountGrantOfferEnvelope {
            machine_id: identity.machine_id.clone(), enrollment_epoch: epoch.clone(), sealed: STANDARD.encode(sealed),
        };
        apply_grant_offer(&pairing_state.auth_manager, &attach, &identity.machine_id, &epoch, &envelope, now)
            .map_err(|e| anyhow!("apply fixture grant: {e}"))?;
        let pair = pairing_state.auth_manager.exchange_pairing_code_with_installation(
            &capability, &offer.device_label, Some(&offer.installation_id),
        ).map_err(|e| anyhow!("pairing exchange: {e}"))?;
        if pair.1.permission != DevicePermission::Control || pair.1.access_scope != DeviceAccessScope::Machine {
            return Err(anyhow!("fixture grant did not produce machine control authority"));
        }
        Ok(pair)
    }).await??;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", listener.local_addr()?);
    let router = create_remote_router(state.clone());
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let mut server = tokio::spawn(async move {
        axum::serve(listener, router.into_make_service_with_connect_info::<SocketAddr>())
            .with_graceful_shutdown(async { if shutdown_rx.await.is_err() { eprintln!("shutdown sender dropped"); } }).await
    });
    let client = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(20)).build()?;
    let provider = Arc::new(AcceptanceFixtureProvider { stage: Mutex::new(DeliveryStage::Accepted), reject_next: Mutex::new(false) });
    let mut sessions = Vec::new();
    let mut targets: Vec<TargetRef> = Vec::new();
    let run = async {
        let project = post(&client, &base, &token, "/api/v1/workspace/projects",
            json!({"requestId":uuid::Uuid::new_v4(),"repoPath":root.path()})).await?;
        let workspace = project["workspaceId"].as_str().context("project workspaceId")?;
        // Re-register through the DAEMON path so the workspace becomes mirror-exposed.
        // The remote `/api/v1/workspace/projects` route inserts its catalog row with
        // `mirror_exposed: false` (remote/workspace_api.rs), and `RemoteGatewayState::set_active_selection`
        // deliberately CLEARS any selection whose workspace is not mirror-exposed (remote/state.rs:1164).
        // So every selection this fixture set - including its own `Focus` command - was silently
        // discarded, the served workspace state carried no active selection, and `get_workspace_state`
        // synthesized one from a HashMap (arbitrary order). That is why the UI bound to sessions[1]
        // while the runner asserted targets[0]. Registering here flips the row to `mirror_exposed: true`,
        // which makes `focus` effective and the binding deterministic.
        // Fail loudly: a silently skipped registration would reproduce the arbitrary-order mystery.
        owner
            .handle_register_workspace(
                workspace,
                root.path().to_str().context("fixture repo root is not UTF-8")?,
            )
            .map_err(|error| anyhow!("mirror-exposed workspace registration failed: {error}"))?;
        let mut selections = Vec::new();
        let mut threads = Vec::new();
        for index in 0..2 {
            let session = post(&client, &base, &token, "/api/v1/sessions", json!({
                "requestId":uuid::Uuid::new_v4(),"workspaceId":workspace,"worktree":null,
                "inheritFromSessionId":null,"cwdRelative":null,"cols":80,"rows":24,"startup":{"kind":"shell"}
            })).await?;
            let id = session["target"]["sessionId"].as_str().context("created sessionId")?.to_owned();
            sessions.push(id.clone());
            let target: TargetRef = serde_json::from_value(json!({"hostId":session["target"]["machineId"],
                "ownerId":device.id,"epoch":session["target"]["daemonEpoch"],"backendSessionId":id}))?;
            targets.push(target.clone());
            // No session is pre-started. The runner's `production_launch` clicks the UI's Start button
            // for whichever session the product serves as active, and the real route correctly answers
            // 409 REQUEST_CONFLICT when that backendSessionId already holds a managed provider
            // (managed_chat_lifecycle.rs). Pre-starting any session would therefore turn a correct
            // product response into a spurious scenario failure. The runner fills this in from its own
            // launch response, and scenario 1 installs the fixture provider via `useFixture`.
            threads.push(String::new());
            selections.push(RemoteActiveDesktopSelection {
                workspace_id: Some(workspace.to_owned()), worktree_label: Some("main".into()),
                tab_id: Some(format!("qa-tab-{index}::qa-pane")), session_id: Some(id.clone()),
                terminal_tabs: vec![RemoteTerminalTabInfo { id: format!("qa-tab-{index}::qa-pane"),
                    label: format!("QA target {index}"), session_id: Some(id), ..Default::default() }],
                ..Default::default()
            });
        }
        state.set_active_selection(selections[0].clone());
        let capabilities: Value = client.get(format!("{base}/api/v1/capabilities"))
            .bearer_auth(&token).send().await?.error_for_status()?.json().await?;
        let workspace_state: Value = client.get(format!("{base}/api/v1/workspace/state"))
            .bearer_auth(&token).send().await?.error_for_status()?.json().await?;
        let session_inventory: Value = client.get(format!("{base}/api/v1/sessions"))
            .bearer_auth(&token).send().await?.error_for_status()?.json().await?;
        output(json!({"event":"ready","gatewayUrl":base,"token":token,"deviceId":device.id,
            "targets":targets,"threads":threads,"resultPath":"result.txt",
            "capabilities":capabilities,"workspaceState":workspace_state,"sessionInventory":session_inventory,
            "historySource":"controlled child transcript through production binding and reader"}))?;
        let mut lines = BufReader::new(tokio::io::stdin()).lines();
        while let Some(line) = lines.next_line().await? {
            let command: Command = serde_json::from_str(&line).context("fixture command")?;
            match command {
                Command::Focus { index } => {
                    state.set_active_selection(selections.get(index).context("unknown target index")?.clone());
                    output(json!({"event":"focusChanged","index":index}))?;
                }
                Command::SetDeliveryStage { stage } => {
                    *provider.stage.lock() = stage;
                    output(json!({"event":"deliveryStageConfigured"}))?;
                }
                Command::RejectNext => {
                    *provider.reject_next.lock() = true;
                    output(json!({"event":"rejectNextConfigured"}))?;
                }
                Command::UseFixture { index } => {
                    let target = targets.get(index).context("unknown target index")?;
                    let current = ferryx_lib::remote::managed_chat_api::MANAGED_PROVIDERS.lock()
                        .get(&target.backend_session_id).cloned();
                    if let Some(current) = current { current.stop_agent(target).await.map_err(|e| anyhow!(e))?; }
                    unregister_managed_provider(&target.backend_session_id);
                    register_managed_provider(&target.backend_session_id, provider.clone());
                    output(json!({"event":"fixtureProviderInstalled","index":index}))?;
                }
                Command::StartManaged { index } => {
                    let target = targets.get(index).context("unknown target index")?;
                    let current = ferryx_lib::remote::managed_chat_api::MANAGED_PROVIDERS.lock()
                        .get(&target.backend_session_id).cloned();
                    if let Some(current) = current { current.stop_agent(target).await.map_err(|e| anyhow!(e))?; }
                    unregister_managed_provider(&target.backend_session_id);
                    let started = post(&client, &base, &token, "/api/v1/chat/start", json!({
                        "requestId":uuid::Uuid::new_v4(),"target":target,"provider":"codex"
                    })).await?;
                    let thread = started["data"]["threadId"].as_str().context("managed threadId")?.to_owned();
                    threads[index] = thread.clone();
                    output(json!({"event":"managedStarted","index":index,"threadId":thread}))?;
                }
                Command::RewriteHistory { index, marker } => {
                    write_history(threads.get(index).context("unknown history index")?.clone(), marker).await?;
                    output(json!({"event":"historyRewritten","index":index}))?;
                }
                Command::PublishCallback { index, id, thread_id, turn_id, text } => {
                    let target = targets.get(index).context("unknown callback target")?.clone();
                    LIVE_CALLBACKS.lock().register(LiveCallbackEntry {
                        callback_id:id.clone(), thread_id:thread_id.clone(), turn_id:turn_id.clone(),
                        callback_incarnation:0,
                        target:target.clone(),kind:CallbackKind::Approval,text:Some(text.clone()),questions:None,
                        status:CallbackStatus::Pending,created_at:Instant::now(),
                    }).map_err(|e| anyhow!("callback registration: {e:?}"))?;
                    output(json!({"event":"callbackPublished","id":id}))?;
                }
                Command::Shutdown => break,
            }
        }
        Ok::<(), anyhow::Error>(())
    }.await;
    // These sessions and this server are the only resources this process owns.
    let mut cleanup_errors = Vec::new();
    for id in &sessions {
        let current = ferryx_lib::remote::managed_chat_api::MANAGED_PROVIDERS.lock().get(id).cloned();
        if let (Some(current), Some(target)) = (current, targets.iter().find(|t| &t.backend_session_id == id)) {
            if let Err(error) = current.stop_agent(target).await { cleanup_errors.push(error); }
        }
        unregister_managed_provider(id);
        if let Err(error) = owner.terminal_service().close_session(id).await { cleanup_errors.push(error.to_string()); }
    }
    if let Err(error) = state.auth_manager.revoke_device(&device.id) { cleanup_errors.push(error.to_string()); }
    if shutdown_tx.send(()).is_err() { cleanup_errors.push("server already stopped".into()); }
    match tokio::time::timeout(Duration::from_secs(5), &mut server).await {
        Ok(result) => { result.context("server join")??; }
        Err(_) => { server.abort(); cleanup_errors.push("graceful server shutdown timed out".into()); }
    }
    let remaining = owner.terminal_service().list_sessions();
    if sessions.iter().any(|id| remaining.contains(id)) { cleanup_errors.push("owned terminal remains registered".into()); }
    drop(owner);
    drop(state);
    if let Err(error) = tokio::task::spawn_blocking(move || root.close()).await? {
        cleanup_errors.push(error.to_string());
    }
    output(json!({"event":"stopped","ownedSessionCount":sessions.len(),"cleanupErrors":cleanup_errors}))?;
    run?;
    if !cleanup_errors.is_empty() { return Err(anyhow!("fixture cleanup failed")); }
    Ok(())
}
