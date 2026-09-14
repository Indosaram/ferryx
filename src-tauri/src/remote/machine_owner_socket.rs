//! Forward authorized machine sockets to the original owner's controller authority.
use super::{machine_socket_error, AuthQuery};
use crate::daemon::proxy::LegacyPeer;
use axum::{extract::ws::{WebSocketUpgrade, Message}, http::StatusCode, response::Response};
use futures_util::{SinkExt, StreamExt};
use std::{sync::Arc, time::Duration};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message as Wire};

pub(super) async fn upgrade(ws: WebSocketUpgrade, peer: Arc<LegacyPeer>, id: String, query: AuthQuery,
    token: String, mut revoked: tokio::sync::watch::Receiver<bool>) -> Result<Response, (StatusCode, String)> {
    let epoch = query.daemon_epoch.ok_or_else(|| machine_socket_error("STALE_EPOCH"))?;
    if id.contains("::") || query.render.is_some() || query.cols.is_some() || query.rows.is_some() {
        return Err(machine_socket_error("INVALID_REQUEST"));
    }
    let dial = async {
        let stream = peer.machine_gateway().await.map_err(|code| machine_socket_error(&code))?;
        let mut url = reqwest::Url::parse("ws://localhost/").map_err(|_| machine_socket_error("INVALID_REQUEST"))?;
        url.path_segments_mut().map_err(|_| machine_socket_error("INVALID_REQUEST"))?.extend(["api", "v1", "terminal", &id]);
        url.query_pairs_mut().append_pair("daemonEpoch", &epoch.0.to_string());
        if let Some(after) = query.after_sequence { url.query_pairs_mut().append_pair("afterSequence", &after.0.to_string()); }
        let mut request = url.as_str().into_client_request().map_err(|_| machine_socket_error("INVALID_REQUEST"))?;
        request.headers_mut().insert("authorization", format!("Bearer {token}").parse().map_err(|_| machine_socket_error("UNAUTHORIZED"))?);
        tokio_tungstenite::client_async(request, stream).await.map_err(|error| match error {
            tokio_tungstenite::tungstenite::Error::Http(response) => (response.status(), String::from_utf8_lossy(response.body().as_deref().unwrap_or_default()).into_owned()),
            _ => machine_socket_error("HOST_UNAVAILABLE"),
        })
    };
    let (owner, _) = tokio::select! {
        biased;
        _ = revoked.wait_for(|value| *value) => return Err(machine_socket_error("UNAUTHORIZED")),
        result = tokio::time::timeout(Duration::from_secs(10), dial) => result.map_err(|_| machine_socket_error("TIMEOUT"))??,
    };
    Ok(ws.max_message_size(64 * 1024).max_frame_size(64 * 1024).on_upgrade(move |socket| async move {
        let (mut downstream, mut input) = socket.split();
        let (mut upstream, mut output) = owner.split();
        let incoming = async {
            while let Some(Ok(message)) = input.next().await {
                let message = match message {
                    Message::Binary(bytes) => Wire::Binary(bytes), Message::Text(text) => Wire::Text(text.as_str().into()),
                    Message::Ping(bytes) => Wire::Ping(bytes), Message::Pong(bytes) => Wire::Pong(bytes), Message::Close(_) => break,
                };
                if !matches!(tokio::time::timeout(Duration::from_secs(10), upstream.send(message)).await, Ok(Ok(()))) { break; }
            }
        };
        let outgoing = async {
            while let Some(Ok(message)) = output.next().await {
                let message = match message {
                    Wire::Binary(bytes) => Message::Binary(bytes), Wire::Text(text) => Message::Text(text.as_str().into()),
                    Wire::Ping(bytes) => Message::Ping(bytes), Wire::Pong(bytes) => Message::Pong(bytes), Wire::Close(_) => break,
                    Wire::Frame(_) => continue,
                };
                if !matches!(tokio::time::timeout(Duration::from_secs(10), downstream.send(message)).await, Ok(Ok(()))) { break; }
            }
        };
        tokio::select! { biased; _ = revoked.wait_for(|value| *value) => {}, _ = incoming => {}, _ = outgoing => {} }
    }))
}
