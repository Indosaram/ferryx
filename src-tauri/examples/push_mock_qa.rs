//! Headless phone-push QA: the production gateway router and push dispatcher against a local
//! mock push service. The mock holds the browser half of the subscription keys, so it decrypts
//! what the daemon sends. Nothing persists outside a temp dir; no daemon or desktop is touched.
//!
//! Run with `FERRYX_PUSH_ALLOW_LOOPBACK_ENDPOINT=1` in a debug build so the mock's
//! `http://127.0.0.1` endpoint is accepted.
use anyhow::{anyhow, Context, Result};
use axum::{extract::State, http::HeaderMap, routing::post, Router};
use base64::Engine;
use ferryx_lib::agent_control::events::ObservationDraft;
use ferryx_lib::agent_control::{AgentControlState, AgentStateSource, AgentTier};
use ferryx_lib::remote::{create_remote_router, DevicePermission, RemoteGatewayState};
use ferryx_lib::terminal::{PtyManager, TerminalOutputHub, TerminalService};
use ferryx_lib::worktree::WorkspaceRegistry;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

type Received = (Option<String>, Vec<u8>);

#[tokio::main]
async fn main() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let terminal = Arc::new(TerminalService::new(
        Arc::new(PtyManager::new()),
        Arc::new(TerminalOutputHub::default()),
    ));
    let state = Arc::new(RemoteGatewayState::new_with_paths(
        terminal,
        WorkspaceRegistry::new(),
        Some(dir.path().join("remote-config.json")),
        Some(dir.path().join("remote-auth.json")),
    ));
    state.start_push_dispatch();

    let gateway = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let gateway_addr = gateway.local_addr()?;
    let router = create_remote_router(Arc::clone(&state));
    tokio::spawn(async move { axum::serve(gateway, router).await });

    let (tx, mut rx) = mpsc::unbounded_channel::<Received>();
    let mock = Router::new()
        .route(
            "/push/{id}",
            post(
                |State(tx): State<mpsc::UnboundedSender<Received>>,
                 headers: HeaderMap,
                 body: axum::body::Bytes| async move {
                    let encoding = headers
                        .get("content-encoding")
                        .and_then(|v| v.to_str().ok())
                        .map(str::to_string);
                    let _ = tx.send((encoding, body.to_vec()));
                    axum::http::StatusCode::CREATED
                },
            ),
        )
        .with_state(tx);
    let mock_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let mock_addr = mock_listener.local_addr()?;
    tokio::spawn(async move { axum::serve(mock_listener, mock).await });

    let code = state.auth_manager.create_pairing_code(DevicePermission::Control);
    let (token, device) = state
        .auth_manager
        .exchange_pairing_code(&code, "QA phone")
        .map_err(|e| anyhow!("pair: {e}"))?;
    println!("paired device={} gateway=http://{gateway_addr} mock=http://{mock_addr}", device.id);

    let http = reqwest::Client::builder().no_proxy().build()?;
    let vapid: serde_json::Value = http
        .get(format!("http://{gateway_addr}/api/push/vapid-public-key"))
        .bearer_auth(&token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    println!("GET /api/push/vapid-public-key -> {vapid}");

    let (keypair, auth) = ece::generate_keypair_and_auth_secret().map_err(|e| anyhow!("{e}"))?;
    let components = keypair.raw_components().map_err(|e| anyhow!("{e}"))?;
    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let subscribe = http
        .post(format!("http://{gateway_addr}/api/push/subscribe"))
        .bearer_auth(&token)
        .json(&json!({
            "endpoint": format!("http://{mock_addr}/push/qa"),
            "keys": { "p256dh": b64.encode(components.public_key()), "auth": b64.encode(auth) },
            "phoneNeedsInput": true,
            "phoneFinishedMode": "longTurns",
        }))
        .send()
        .await?;
    println!("POST /api/push/subscribe -> {}", subscribe.status());
    subscribe.error_for_status()?;

    let publish = |s: AgentControlState| {
        state.agent_events.publish(ObservationDraft {
            session_id: "qa-session".into(),
            state: s,
            state_source: AgentStateSource::Provider,
            tier: AgentTier::Native,
            prompt_id: None,
            kind: None,
            screen_revision: None,
            workspace_id: None,
        });
    };

    let scenario = std::env::args().nth(1).unwrap_or_else(|| "happy".into());
    match scenario.as_str() {
        "happy" => {
            publish(AgentControlState::Working);
            publish(AgentControlState::Blocked);
            println!("published agent transition working -> blocked (desktop focus not reported)");
            let (encoding, body) = tokio::time::timeout(Duration::from_secs(10), rx.recv())
                .await
                .context("no push reached the mock endpoint within 10s")?
                .context("mock closed")?;
            let plain = ece::decrypt(&components, &auth, &body).map_err(|e| anyhow!("decrypt: {e}"))?;
            println!("mock endpoint received POST content-encoding={encoding:?} ciphertext_bytes={}", body.len());
            println!("decrypted payload: {}", String::from_utf8_lossy(&plain));
            let payload: serde_json::Value = serde_json::from_slice(&plain)?;
            if payload["state"] != "waiting_input" {
                return Err(anyhow!("unexpected state {}", payload["state"]));
            }
            let test = http
                .post(format!("http://{gateway_addr}/api/v1/push/test"))
                .bearer_auth(&token)
                .send()
                .await?;
            println!("POST /api/v1/push/test -> {} {}", test.status(), test.text().await?);
            let (_, body) = tokio::time::timeout(Duration::from_secs(10), rx.recv())
                .await
                .context("test push did not arrive")?
                .context("mock closed")?;
            let plain = ece::decrypt(&components, &auth, &body).map_err(|e| anyhow!("decrypt: {e}"))?;
            println!("test push payload: {}", String::from_utf8_lossy(&plain));
            println!("RESULT PASS happy: payload state waiting_input");
        }
        "suppressed" => {
            state.set_desktop_window_focused(true);
            println!("desktop window focus reported: true");
            publish(AgentControlState::Working);
            publish(AgentControlState::Blocked);
            println!("published agent transition working -> blocked; waiting 5s for any request");
            match tokio::time::timeout(Duration::from_secs(5), rx.recv()).await {
                Err(_) => println!("RESULT PASS suppressed: no request reached the mock endpoint within 5s"),
                Ok(received) => return Err(anyhow!("a push arrived while focused: {:?}", received.map(|r| r.1.len()))),
            }
        }
        other => return Err(anyhow!("unknown scenario {other}")),
    }
    Ok(())
}
