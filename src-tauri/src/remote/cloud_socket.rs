//! Raw byte forwarding to a cloud host's own RPC socket, for hosts that are not reachable
//! over SSH at all (`T2` in the plan: every provider whose only channel is the relay).
//!
//! Task 1's spike selected `MODE=A` with `ENGINE=omo`
//! (`.omo/evidence/ferryx-cloud-host-sandbox/task-1-spike-r2.txt`): a forwarded socket carries
//! the RPC protocol intact and a turn finalizes through it, so the verbs run on this Mac
//! against a local socket and the box never executes them.
//!
//! This module is the machine half of that forward. It is deliberately dumb: the route
//! authenticates exactly as the DAG route does, resolves one **fixed** path
//! (`$HOME/.ferryx-cloud/rpc.sock`) and copies bytes both ways. It adds no RPC framing of its
//! own and never accepts a path, a command or an argv from the caller - the whole class of
//! "let the caller name a filesystem object" bugs is excluded by construction rather than
//! filtered.
//!
//! The relay needs no change and none was made: the plaintext ticket routes it serves keep
//! refusing this target (426 `E2E_REQUIRED`, then its own target allow-list), and the
//! end-to-end allocation carries only `{machine_id, session_kind}`, so the public relay never
//! learns this inner path.

use super::{
    auth::{DeviceAccessScope, DevicePermission},
    state::RemoteGatewayState,
};
use axum::{
    extract::{
        ws::{WebSocket, WebSocketUpgrade},
        Query, State,
    },
    http::{HeaderMap, StatusCode},
    response::Response,
};
use serde::Deserialize;
use std::sync::Arc;

/// Socket route a socket-forward ticket may be minted against.
pub const SOCKET_FORWARD_TARGET: &str = "/api/v1/cloud/socket";
/// Capability a host advertises once it can forward its RPC socket.
pub const SOCKET_FORWARD_CAPABILITY: &str = "socket_forward_v1";
/// The box-side socket, relative to the host's home directory.
///
/// Fixed on purpose: the path the spike proved is the path `omo host ensure` creates, and a
/// caller that could name a different one would turn this route into a general proxy for every
/// unix socket the host's user can open.
pub const RPC_SOCKET_RELATIVE: &str = ".ferryx-cloud/rpc.sock";

/// The one path this route forwards to. `None` when the host has no home directory.
pub fn rpc_socket_path() -> Option<std::path::PathBuf> {
    dirs::home_dir().map(|home| home.join(RPC_SOCKET_RELATIVE))
}

#[derive(Debug, Deserialize)]
pub(super) struct CloudSocketQuery {
    ticket: Option<String>,
}

/// Upgrades an authenticated machine-scope client to the host's RPC socket.
///
/// The admission check is the DAG route's, unchanged and for the same reason: a mirror or
/// view-only device is refused before anything is opened, so a grant without machine Control
/// can never reach a socket.
pub(super) async fn ws_socket_forward_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<CloudSocketQuery>,
    headers: HeaderMap,
    State(state): State<Arc<RemoteGatewayState>>,
) -> Result<Response, (StatusCode, String)> {
    let token = match query.ticket.as_deref() {
        Some(ticket) => {
            super::server::consume_socket_ticket(&state, ticket, SOCKET_FORWARD_TARGET)
        }
        None => super::server::extract_token(&headers),
    }
    .ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
    let device = state
        .auth_manager
        .validate_token(&token)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token".into()))?;
    if device.access_scope != DeviceAccessScope::Machine
        || device.permission != DevicePermission::Control
    {
        return Err((StatusCode::FORBIDDEN, "MACHINE_ACCESS_REQUIRED".into()));
    }
    let mut revocation = state
        .auth_manager
        .device_revocation(&device.id)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token".into()))?;
    let target = state.cloud_socket_target();
    Ok(ws.on_upgrade(move |socket| async move {
        tokio::select! {
            biased;
            _ = revocation.wait_for(|revoked| *revoked) => {},
            _ = serve(socket, target) => {},
        }
    }))
}

/// Copies bytes between one upgraded socket and the host's RPC socket.
///
/// Both directions end together: a closed RPC socket closes the websocket, and the reverse.
/// Nothing is buffered beyond the copy buffers, so a stalled reader on one side stops the
/// other rather than growing a queue.
async fn serve(socket: WebSocket, target: Option<std::path::PathBuf>) {
    let Some(target) = target else {
        let _ = send_close(socket).await;
        return;
    };
    #[cfg(unix)]
    {
        let Ok(stream) = tokio::net::UnixStream::connect(&target).await else {
            // A host whose daemon is not running is a normal state, not an error to log per
            // attempt: the client's own readiness probe reports it.
            let _ = send_close(socket).await;
            return;
        };
        let mut socket = super::attach_router::AxumWebSocketStream::new(socket);
        let mut stream = stream;
        let _ = tokio::io::copy_bidirectional(&mut socket, &mut stream).await;
    }
    // A unix socket exists only on unix, so every other target has nothing to forward to and
    // answers the same close a host with no daemon answers.
    #[cfg(not(unix))]
    {
        let _ = target;
        let _ = send_close(socket).await;
    }
}

async fn send_close(mut socket: WebSocket) {
    let _ = socket.send(axum::extract::ws::Message::Close(None)).await;
}
