use super::*;
use crate::remote::backend::{RemoteSessionBackend, RemoteSessionDetails};
use crate::terminal::{SessionAttachment, TerminalOutputHub, TerminalSignal};
use futures_util::future::BoxFuture;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Notify;

#[derive(Default)]
struct Gate {
    entered: Notify,
    release: Notify,
    pending: AtomicUsize,
}

impl Gate {
    async fn wait(&self) {
        struct Pending<'a>(&'a AtomicUsize);
        impl Drop for Pending<'_> {
            fn drop(&mut self) {
                self.0.fetch_sub(1, Ordering::SeqCst);
            }
        }
        self.pending.fetch_add(1, Ordering::SeqCst);
        let _pending = Pending(&self.pending);
        self.entered.notify_one();
        self.release.notified().await;
    }
}

/// Only the PTY boundary is replaced: real auth, router, TCP/WebSocket framing,
/// output replay/broadcast hub and (feature-on) Ghostty mirror run unchanged.
struct SocketBackend {
    hub: TerminalOutputHub,
    attach_gate: Option<Arc<Gate>>,
    input_gate: Arc<Gate>,
    completed_inputs: AtomicUsize,
}

impl SocketBackend {
    fn new(attach_gate: Option<Arc<Gate>>) -> Arc<Self> {
        let hub = TerminalOutputHub::default();
        hub.register_session("session");
        hub.publish("session", b"READY".to_vec()).unwrap();
        Arc::new(Self {
            hub,
            attach_gate,
            input_gate: Arc::new(Gate::default()),
            completed_inputs: AtomicUsize::new(0),
        })
    }
}

impl RemoteSessionBackend for SocketBackend {
    fn list_sessions(&self) -> BoxFuture<'_, Vec<String>> {
        Box::pin(async { vec!["session".into()] })
    }
    fn describe_session<'a>(
        &'a self,
        id: &'a str,
    ) -> BoxFuture<'a, Result<RemoteSessionDetails, String>> {
        Box::pin(async move {
            Ok(RemoteSessionDetails {
                session_id: id.into(),
                workspace_id: None,
                worktree_label: None,
                worktree_path: None,
                running: true,
                cols: 80,
                rows: 24,
            })
        })
    }
    fn attach_with_sequence<'a>(
        &'a self,
        id: &'a str,
        sequence: Option<u64>,
    ) -> BoxFuture<'a, Result<SessionAttachment, String>> {
        Box::pin(async move {
            let attachment = self
                .hub
                .subscribe_with_sequence(id, sequence)
                .ok_or("unknown session")?;
            if let Some(gate) = &self.attach_gate {
                gate.wait().await;
            }
            Ok(attachment)
        })
    }
    fn write_input<'a>(&'a self, _: &'a str, _: &'a [u8]) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            self.input_gate.wait().await;
            self.completed_inputs.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }
    fn resize<'a>(&'a self, _: &'a str, _: u16, _: u16) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async { Ok(()) })
    }
    fn signal<'a>(&'a self, _: &'a str, _: TerminalSignal) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async { Ok(()) })
    }
}

fn socket_state(backend: Arc<SocketBackend>) -> Arc<RemoteGatewayState> {
    let state = Arc::new(RemoteGatewayState::new_with_backend(
        backend,
        crate::worktree::WorkspaceRegistry::new(),
    ));
    state.set_active_selection(RemoteActiveDesktopSelection {
        session_id: Some("session".into()),
        ..Default::default()
    });
    state
}

async fn frame(stream: &mut tokio::net::TcpStream) -> ServerWebSocketFrame {
    tokio::time::timeout(DEADLINE, read_server_ws_frame(stream))
        .await
        .expect("bounded socket frame/close")
}

async fn revocation_closes_device_sockets(path: &str) {
    // Given two open sockets for the victim and one for an unrelated device.
    let backend = SocketBackend::new(None);
    let state = socket_state(Arc::clone(&backend));
    let (token, device) = pair(&state, DevicePermission::Control);
    let (other_token, _) = pair(&state, DevicePermission::Control);
    let server = SecurityServer::start(Arc::clone(&state)).await;
    let mut victims = Vec::new();
    for _ in 0..2 {
        let mut stream =
            tokio::time::timeout(DEADLINE, open_ws_stream(server.addr, path, Some(&token)))
                .await
                .unwrap();
        assert!(!matches!(
            frame(&mut stream).await,
            ServerWebSocketFrame::Close
        ));
        victims.push(stream);
    }
    let mut other = open_ws_stream(server.addr, path, Some(&other_token)).await;
    assert!(!matches!(
        frame(&mut other).await,
        ServerWebSocketFrame::Close
    ));
    // When the management HTTP route revokes the victim and new output arrives.
    assert_eq!(
        server
            .request(
                "POST",
                &format!("/api/v1/devices/{}/revoke", device.id),
                Some(&other_token),
                None
            )
            .await
            .0,
        204
    );
    state.emit_event(r#"{"event":"after_revoke","payload":{}}"#.into());
    backend
        .hub
        .publish("session", b"AFTER_REVOKE".to_vec())
        .unwrap();
    // Then all victim sockets close without delivering that output, but the
    // unrelated socket still delivers an actual event/raw/grid frame.
    for victim in &mut victims {
        assert!(
            matches!(frame(victim).await, ServerWebSocketFrame::Close),
            "revoked socket delivered post-revocation output"
        );
    }
    assert!(!matches!(
        frame(&mut other).await,
        ServerWebSocketFrame::Close
    ));
    state
        .auth_manager
        .revoke_device(&state.auth_manager.validate_token(&other_token).unwrap().id);
    assert!(matches!(
        frame(&mut other).await,
        ServerWebSocketFrame::Close
    ));
    drop(victims);
    drop(other);
    server.stop().await;
}

#[tokio::test]
async fn revoke_closes_all_event_sockets_only_for_that_device() {
    revocation_closes_device_sockets("/api/v1/events").await;
}
#[tokio::test]
async fn revoke_closes_all_raw_sockets_only_for_that_device() {
    revocation_closes_device_sockets("/api/v1/terminal/session").await;
}
#[cfg(feature = "native-terminal")]
#[tokio::test]
async fn revoke_closes_all_grid_sockets_only_for_that_device() {
    revocation_closes_device_sockets("/api/v1/terminal/session?render=grid").await;
}

async fn revocation_cancels_pending_input(path: &str) {
    let backend = SocketBackend::new(None);
    let state = socket_state(Arc::clone(&backend));
    let (token, device) = pair(&state, DevicePermission::Control);
    let server = SecurityServer::start(Arc::clone(&state)).await;
    let mut socket = open_ws_stream(server.addr, path, Some(&token)).await;
    frame(&mut socket).await;
    // Exact backend entry acknowledges that input was accepted before revoke.
    let entered = backend.input_gate.entered.notified();
    write_client_ws_frame(&mut socket, 2, b"pending input").await;
    tokio::time::timeout(DEADLINE, entered).await.unwrap();
    assert_eq!(backend.input_gate.pending.load(Ordering::SeqCst), 1);
    assert!(state.auth_manager.revoke_device(&device.id));
    assert!(matches!(
        frame(&mut socket).await,
        ServerWebSocketFrame::Close
    ));
    // Closure is a lifecycle barrier, not a timed negative observation.
    assert_eq!(
        backend.input_gate.pending.load(Ordering::SeqCst),
        0,
        "input future was detached rather than dropped"
    );
    assert_eq!(backend.completed_inputs.load(Ordering::SeqCst), 0);
    server.stop().await;
}

#[tokio::test]
async fn revoke_cancels_pending_raw_input() {
    revocation_cancels_pending_input("/api/v1/terminal/session").await;
}
#[cfg(feature = "native-terminal")]
#[tokio::test]
async fn revoke_cancels_pending_grid_input() {
    revocation_cancels_pending_input("/api/v1/terminal/session?render=grid").await;
}

async fn revocation_during_attachment(path: &str) {
    let gate = Arc::new(Gate::default());
    let backend = SocketBackend::new(Some(Arc::clone(&gate)));
    let state = socket_state(backend);
    let (token, device) = pair(&state, DevicePermission::Control);
    let server = SecurityServer::start(Arc::clone(&state)).await;
    let entered = gate.entered.notified();
    let handshake = ws_handshake_status(server.addr, path, Some(&token));
    tokio::pin!(handshake);
    tokio::time::timeout(DEADLINE, async {
        tokio::select! {
            _ = entered => {},
            status = &mut handshake => panic!("attachment was not gated: {status}"),
        }
    })
    .await
    .unwrap();
    assert!(state.auth_manager.revoke_device(&device.id));
    gate.release.notify_one();
    let status = tokio::time::timeout(DEADLINE, handshake).await.unwrap();
    assert_eq!(
        status, 401,
        "revocation during attachment must prevent upgrade"
    );
    assert_eq!(gate.pending.load(Ordering::SeqCst), 0);
    server.stop().await;
}

#[tokio::test]
async fn revoke_during_raw_attachment_prevents_upgrade() {
    revocation_during_attachment("/api/v1/terminal/session").await;
}
#[cfg(feature = "native-terminal")]
#[tokio::test]
async fn revoke_during_grid_attachment_prevents_upgrade() {
    revocation_during_attachment("/api/v1/terminal/session?render=grid").await;
}

async fn revocation_before_upgrade_callback(path: &str) {
    let state = socket_state(SocketBackend::new(None));
    let (token, device) = pair(&state, DevicePermission::Control);
    let gate = Arc::new(Gate::default());
    let gate_for_layer = Arc::clone(&gate);
    // Delay the real 101 response after the handler has authorized/subscribed,
    // before Hyper can invoke its on_upgrade future. No production test hook.
    let router = create_remote_router(Arc::clone(&state)).layer(axum::middleware::from_fn(
        move |request: axum::extract::Request, next: axum::middleware::Next| {
            let gate = Arc::clone(&gate_for_layer);
            async move {
                let response = next.run(request).await;
                assert_eq!(response.status(), 101);
                gate.wait().await;
                response
            }
        },
    ));
    let server = SecurityServer::start_router(router).await;
    let entered = gate.entered.notified();
    let handshake = open_ws_stream(server.addr, path, Some(&token));
    tokio::pin!(handshake);
    tokio::time::timeout(DEADLINE, async {
        tokio::select! {
            _ = entered => {},
            _ = &mut handshake => panic!("upgrade response was not gated"),
        }
    })
    .await
    .unwrap();
    assert!(state.auth_manager.revoke_device(&device.id));
    gate.release.notify_one();
    let mut socket = tokio::time::timeout(DEADLINE, handshake).await.unwrap();
    assert!(
        matches!(frame(&mut socket).await, ServerWebSocketFrame::Close),
        "revoked upgrade emitted its initial snapshot"
    );
    server.stop().await;
}

#[tokio::test]
async fn revoke_before_event_upgrade_is_not_lost() {
    revocation_before_upgrade_callback("/api/v1/events").await;
}
#[tokio::test]
async fn revoke_before_raw_upgrade_is_not_lost() {
    revocation_before_upgrade_callback("/api/v1/terminal/session").await;
}
#[cfg(feature = "native-terminal")]
#[tokio::test]
async fn revoke_before_grid_upgrade_is_not_lost() {
    revocation_before_upgrade_callback("/api/v1/terminal/session?render=grid").await;
}
