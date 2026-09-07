//! Isolated HTTP/WebSocket QA fixture using the production router and a real PTY.
//! Does not open a desktop window, contact the daemon, or persist pairing data.
use anyhow::{anyhow, Context, Result};
use ferryx_lib::remote::{
    create_remote_router, DevicePermission, RemoteActiveDesktopSelection, RemoteGatewayState,
    RemoteTerminalTabInfo,
};
use ferryx_lib::terminal::{PtyManager, TerminalOutputHub, TerminalService};
use ferryx_lib::worktree::WorkspaceRegistry;
use portable_pty::CommandBuilder;
use serde::Deserialize;
use serde_json::json;
use std::io::Write;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
enum Command {
    Pair { permission: DevicePermission },
    Event { marker: String },
    Focus { active: bool },
    Shutdown,
}

fn output(value: serde_json::Value) -> Result<()> {
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "{value}")?;
    stdout.flush()?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let pty = Arc::new(PtyManager::new());
    let hub = Arc::new(TerminalOutputHub::default());
    let terminal = Arc::new(TerminalService::new(Arc::clone(&pty), Arc::clone(&hub)));
    let command = CommandBuilder::new(if cfg!(windows) { "cmd.exe" } else { "/bin/cat" });
    let spawn_pty = Arc::clone(&pty);
    let (session_id, mut pty_rx) =
        tokio::task::spawn_blocking(move || spawn_pty.spawn(command, 80, 24))
            .await
            .context("PTY spawn task")??;
    hub.register_session(&session_id);
    hub.record_initial_size(&session_id, 80, 24);
    let output_hub = Arc::clone(&hub);
    let output_session = session_id.clone();
    let pump = tokio::spawn(async move {
        while let Some(bytes) = pty_rx.recv().await {
            output_hub.publish(&output_session, bytes);
        }
    });
    let state = Arc::new(RemoteGatewayState::new(terminal, WorkspaceRegistry::new()));
    let selection = RemoteActiveDesktopSelection {
        workspace_id: Some("qa-workspace".into()),
        worktree_label: Some("main".into()),
        tab_id: Some("qa-tab::qa-pane".into()),
        session_id: Some(session_id.clone()),
        terminal_tabs: vec![RemoteTerminalTabInfo {
            id: "qa-tab::qa-pane".into(),
            label: "QA terminal".into(),
            session_id: Some(session_id.clone()),
            ..Default::default()
        }],
        ..Default::default()
    };
    state.set_active_selection(selection.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let router = create_remote_router(Arc::clone(&state));
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async {
            let _ = shutdown_rx.await;
        })
        .await
    });
    output(json!({ "event": "ready", "addr": addr, "sessionId": session_id }))?;

    let commands = async {
        let mut lines = BufReader::new(tokio::io::stdin()).lines();
        while let Some(line) = lines.next_line().await? {
            match serde_json::from_str::<Command>(&line).context("QA stdin command")? {
                Command::Pair { permission } => {
                    let code = state.auth_manager.create_pairing_code(permission);
                    output(json!({ "event": "pairingCode", "code": code }))?;
                }
                Command::Event { marker } => {
                    state.emit_event(json!({ "event": "qa", "payload": marker }).to_string());
                    output(json!({ "event": "published" }))?;
                }
                Command::Focus { active } => {
                    let mut next = selection.clone();
                    if !active {
                        next.session_id = None;
                    }
                    state.set_active_selection(next);
                    output(json!({ "event": "focusChanged" }))?;
                }
                Command::Shutdown => break,
            }
        }
        Ok::<(), anyhow::Error>(())
    }
    .await;

    let _ = shutdown_tx.send(());
    pty.close_session(&session_id).await?;
    pump.abort();
    match tokio::time::timeout(std::time::Duration::from_secs(5), server).await {
        Ok(result) => result.context("server task")??,
        Err(_) => return Err(anyhow!("QA clients remained connected during shutdown")),
    }
    commands?;
    output(json!({ "event": "stopped" }))
}
