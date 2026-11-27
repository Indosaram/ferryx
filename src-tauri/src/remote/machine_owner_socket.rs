//! Forward authorized machine sockets to the original owner's controller authority.
use super::{machine_socket_error, AuthQuery};
use crate::daemon::proxy::LegacyPeer;
use axum::{
    body::{to_bytes, Body},
    extract::ws::{Message, WebSocketUpgrade},
    http::{header, Request, StatusCode},
    response::{IntoResponse, Response},
};
use futures_util::{SinkExt, StreamExt};
use hyper_util::rt::TokioIo;
use std::{sync::Arc, time::Duration};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::{
    tungstenite::{client::IntoClientRequest, handshake::derive_accept_key, protocol::Role, Message as Wire},
    WebSocketStream,
};

// A WebSocket client's HTTP error contains only the bytes read with the headers.
// Drive the owner HTTP exchange to completion so its 409 label/capabilities survive
// a split body. Hyper's upgrade retains any frame bytes read alongside a 101.
async fn owner_handshake<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    stream: S,
    mut request: Request<()>,
    json_refusal: bool,
) -> Result<Result<WebSocketStream<TokioIo<hyper::upgrade::Upgraded>>, Response>, (StatusCode, String)> {
    let unavailable = || machine_socket_error("HOST_UNAVAILABLE");
    let accept = derive_accept_key(request.headers().get(header::SEC_WEBSOCKET_KEY).ok_or_else(unavailable)?.as_bytes());
    let path = request.uri().path_and_query().cloned().ok_or_else(unavailable)?;
    *request.uri_mut() = path.into(); // The IPC peer is the origin server, not an HTTP proxy.
    let (mut http, driver) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await.map_err(|_| unavailable())?;
    let exchange = async move {
        let mut reply = http.send_request(request.map(|()| Body::empty())).await.map_err(|_| unavailable())?;
        let status = reply.status();
        let headers = reply.headers();
        if status != StatusCode::SWITCHING_PROTOCOLS {
            let json = json_refusal && headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok())
                .and_then(|v| v.split(';').next()).is_some_and(|v| v.trim().eq_ignore_ascii_case("application/json"));
            let mime = if json { "application/json" } else { "text/plain; charset=utf-8" };
            let body = to_bytes(Body::new(reply.into_body()), 16 * 1024).await.map_err(|_| unavailable())?;
            return Ok(Err((status, [(header::CONTENT_TYPE, mime)], body).into_response()));
        }
        let websocket = headers.get(header::UPGRADE).is_some_and(|v| v.as_bytes().eq_ignore_ascii_case(b"websocket"));
        let upgrade = headers.get_all(header::CONNECTION).iter().filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(',')).any(|v| v.trim().eq_ignore_ascii_case("upgrade"));
        if !websocket || !upgrade || headers.get(header::SEC_WEBSOCKET_ACCEPT).is_none_or(|v| v.as_bytes() != accept.as_bytes())
            || headers.contains_key(header::SEC_WEBSOCKET_PROTOCOL) || headers.contains_key(header::SEC_WEBSOCKET_EXTENSIONS)
        {
            return Err(unavailable());
        }
        let socket = hyper::upgrade::on(&mut reply).await.map_err(|_| unavailable())?;
        Ok(Ok(WebSocketStream::from_raw_socket(TokioIo::new(socket), Role::Client, None).await))
    };
    // Keep the driver inside the caller's existing timeout and revocation boundary.
    // Once a non-101 refusal response has been read completely, a trailing socket reset from the
    // closing owner must not replace that negotiated refusal with 503 HOST_UNAVAILABLE.
    let (result, driver_outcome) = tokio::join!(exchange, driver.with_upgrades());
    match result {
        Ok(Err(refusal)) => Ok(Err(refusal)),
        Ok(Ok(socket)) => {
            driver_outcome.map_err(|_| unavailable())?;
            Ok(Ok(socket))
        }
        Err(err) => Err(err),
    }
}

pub(super) async fn upgrade(
    ws: WebSocketUpgrade,
    peer: Arc<LegacyPeer>,
    id: String,
    query: AuthQuery,
    token: String,
    mut revoked: tokio::sync::watch::Receiver<bool>,
    json_refusal: bool,
) -> Result<Response, (StatusCode, String)> {
    let epoch = query
        .daemon_epoch
        .ok_or_else(|| machine_socket_error("STALE_EPOCH"))?;
    // `render=grid` is what the account browser client always requests, so it must be
    // forwarded rather than refused; otherwise a phone can never attach through a
    // peer at all, takeover or not. Only values the local handler accepts are allowed.
    match query.render.as_deref() {
        None | Some("grid") => {}
        Some(_) => return Err(machine_socket_error("INVALID_REQUEST")),
    }
    if id.contains("::") || query.cols.is_some() || query.rows.is_some() {
        return Err(machine_socket_error("INVALID_REQUEST"));
    }
    let takeover = match query.takeover.as_deref() {
        None | Some("0") => false,
        Some("1") => true,
        Some(_) => return Err(machine_socket_error("INVALID_REQUEST")),
    };
    let dial = async {
        let stream = peer
            .machine_gateway()
            .await
            .map_err(|code| machine_socket_error(&code))?;
        let mut url = reqwest::Url::parse("ws://localhost/")
            .map_err(|_| machine_socket_error("INVALID_REQUEST"))?;
        url.path_segments_mut()
            .map_err(|_| machine_socket_error("INVALID_REQUEST"))?
            .extend(["api", "v1", "terminal", &id]);
        url.query_pairs_mut()
            .append_pair("daemonEpoch", &epoch.0.to_string());
        if let Some(render) = query.render.as_deref() {
            url.query_pairs_mut().append_pair("render", render);
        }
        if takeover {
            url.query_pairs_mut().append_pair("takeover", "1");
        }
        if let Some(after) = query.after_sequence {
            url.query_pairs_mut()
                .append_pair("afterSequence", &after.0.to_string());
        }
        let mut request = url
            .as_str()
            .into_client_request()
            .map_err(|_| machine_socket_error("INVALID_REQUEST"))?;
        request.headers_mut().insert(
            "authorization",
            format!("Bearer {token}")
                .parse()
                .map_err(|_| machine_socket_error("UNAUTHORIZED"))?,
        );
        if json_refusal {
            request.headers_mut().insert("x-ferryx-refusal-format", axum::http::HeaderValue::from_static("json"));
        }
        owner_handshake(stream, request, json_refusal).await
    };
    let handshake = tokio::select! {
        biased;
        _ = revoked.wait_for(|value| *value) => return Err(machine_socket_error("UNAUTHORIZED")),
        result = tokio::time::timeout(Duration::from_secs(10), dial) => result.map_err(|_| machine_socket_error("TIMEOUT"))??,
    };
    // A forwarder must preserve the original owner's negotiated refusal, not advertise
    // its own capabilities for a legacy owner.
    let owner = match handshake { Ok(socket) => socket, Err(response) => return Ok(response) };
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

#[cfg(test)]
#[path = "machine_owner_socket_tests.rs"]
mod tests;
