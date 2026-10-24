//! End-to-end encrypted relay transport for paired hosts.
//!
//! A host paired with its machine attach key is reached through the relay as an opaque
//! session: the client allocates it (`POST /api/v1/attach/session?e2e=1`), dials
//! `/tunnel/opaque/{session}`, runs the existing Noise initiator (`paired_host::attach`),
//! and speaks plain HTTP/1.1 to the machine's loopback gateway inside the encrypted frames.
//! The relay only ever carries ciphertext. Nothing here touches the direct (LAN) path.
use super::{
    attach::{attach_relay_session, RelayAttachedStream},
    client::ClientError,
    inventory::HostAttach,
};
use crate::remote::attach_crypto::MAX_ATTACH_FRAME;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    pin::Pin,
    sync::{Arc, LazyLock, Mutex},
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, DuplexStream, ReadBuf},
    net::TcpStream,
};
use tokio_tungstenite::{
    tungstenite::{self, client::IntoClientRequest, protocol::WebSocketConfig},
    MaybeTlsStream, WebSocketStream,
};

/// Build string every updated Rust client sends on relay requests.
pub const CLIENT_BUILD: &str = concat!("ferryx/", env!("CARGO_PKG_VERSION"));
pub const CLIENT_BUILD_HEADER: &str = "x-ferryx-client-build";
/// Bound on one allocation + tunnel dial + handshake.
const ESTABLISH_TIMEOUT: Duration = Duration::from_secs(30);
/// Bound on a response status line plus headers.
const MAX_RESPONSE_HEAD: usize = 64 * 1024;
/// Bound on an allocation answer.
const MAX_ALLOCATION_BODY: usize = 64 * 1024;

/// The byte stream under a paired-host WebSocket: a network socket (direct or legacy
/// relay, TLS is layered by `MaybeTlsStream`) or the plaintext end of an encrypted tunnel.
pub enum PairedStream {
    Net(TcpStream),
    Tunnel(DuplexStream),
}
/// Every paired-host WebSocket, whichever path carries it.
pub type PairedSocket = WebSocketStream<MaybeTlsStream<PairedStream>>;

impl AsyncRead for PairedStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Net(stream) => Pin::new(stream).poll_read(cx, buf),
            Self::Tunnel(stream) => Pin::new(stream).poll_read(cx, buf),
        }
    }
}
impl AsyncWrite for PairedStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            Self::Net(stream) => Pin::new(stream).poll_write(cx, buf),
            Self::Tunnel(stream) => Pin::new(stream).poll_write(cx, buf),
        }
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Net(stream) => Pin::new(stream).poll_flush(cx),
            Self::Tunnel(stream) => Pin::new(stream).poll_flush(cx),
        }
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Net(stream) => Pin::new(stream).poll_shutdown(cx),
            Self::Tunnel(stream) => Pin::new(stream).poll_shutdown(cx),
        }
    }
}

/// Dials a `ws`/`wss` URL over TCP the way `connect_async_with_config(.., true)` does
/// (Nagle off, default rustls roots for `wss`), yielding the shared socket type.
pub async fn connect_net_websocket(
    request: tungstenite::handshake::client::Request,
    config: WebSocketConfig,
) -> Result<PairedSocket, tungstenite::Error> {
    use tungstenite::error::UrlError;
    let uri = request.uri();
    let host = uri
        .host()
        .ok_or(tungstenite::Error::Url(UrlError::NoHostName))?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_owned();
    let port = uri
        .port_u16()
        .or(match uri.scheme_str() {
            Some("wss") => Some(443),
            Some("ws") => Some(80),
            _ => None,
        })
        .ok_or(tungstenite::Error::Url(UrlError::UnsupportedUrlScheme))?;
    let tcp = TcpStream::connect((host.as_str(), port)).await?;
    tcp.set_nodelay(true)?;
    let (socket, _) = tokio_tungstenite::client_async_tls_with_config(
        request,
        PairedStream::Net(tcp),
        Some(config),
        None,
    )
    .await?;
    Ok(socket)
}

/// The directory holding this client's own attach identity. Deliberately separate from
/// the machine's `attach-identity.json`: a client key must never be the machine key.
pub fn client_attach_dir() -> Result<PathBuf, ClientError> {
    let unavailable = || ClientError::local("ATTACH_IDENTITY_UNAVAILABLE");
    let dir = crate::remote::auth::canonical_identity_dir()
        .map_err(|_| unavailable())?
        .join("paired-host-client");
    std::fs::create_dir_all(&dir).map_err(|_| unavailable())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| unavailable())?;
    }
    Ok(dir)
}

/// This client's attach public key (base64), generating the identity on first use.
/// The private half never leaves this process's disk store.
pub fn client_attach_public_key() -> Result<String, ClientError> {
    crate::remote::attach_client::load_or_generate_client_attach_identity(&client_attach_dir()?)
        .map(|identity| identity.public_key)
        .map_err(|_| ClientError::local("ATTACH_IDENTITY_UNAVAILABLE"))
}

fn session_id_valid(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// Maps a refused allocation to a typed code. Never carries relay prose.
fn allocation_error(status: u16, body: &[u8]) -> ClientError {
    let value = serde_json::from_slice::<serde_json::Value>(body).ok();
    let code = value.as_ref().and_then(|v| {
        v.get("code")
            .and_then(serde_json::Value::as_str)
            .or_else(|| v.pointer("/error/code").and_then(serde_json::Value::as_str))
    });
    if code == Some("E2E_REQUIRED") || status == 426 {
        return ClientError::local("REPAIR_REQUIRED");
    }
    ClientError::local(match (status, code) {
        // An account relay that only accepts account sessions answers the device token
        // with a bare or generic 401; a relay that knows `?e2e=1` names a specific code.
        (401, None) | (401, Some("UNAUTHORIZED")) => "E2E_UNSUPPORTED",
        (401, _) => "UNAUTHORIZED",
        (409, _) | (502, _) | (503, _) => "HOST_UNAVAILABLE",
        (402, _) => "REMOTE_SUSPENDED",
        (404, Some("MACHINE_NOT_FOUND")) => "PAIRED_HOST_WRONG_MACHINE",
        // A relay that predates `?e2e=1` device-token allocation.
        (404, _) | (405, _) | (400, _) => "E2E_UNSUPPORTED",
        (429, _) => "RATE_LIMITED",
        (504, _) => "TIMEOUT",
        _ => "PAIRED_HOST_REMOTE_ERROR",
    })
}

/// `POST {relay_origin}/api/v1/attach/session?e2e=1`; answers the opaque session id.
pub async fn allocate_e2e_session(
    http: &reqwest::Client,
    relay_origin: &str,
    bearer: &str,
    machine_id: &str,
) -> Result<String, ClientError> {
    let origin = relay_origin.trim_end_matches('/');
    let body = serde_json::to_vec(&serde_json::json!({ "machineId": machine_id }))
        .map_err(|_| ClientError::local("INVALID_REQUEST"))?;
    let mut response = http
        .post(format!("{origin}/api/v1/attach/session?e2e=1"))
        .bearer_auth(bearer)
        .header("content-type", "application/json")
        .header(CLIENT_BUILD_HEADER, CLIENT_BUILD)
        .body(body)
        .send()
        .await
        .map_err(|error| {
            ClientError::local(if error.is_timeout() { "TIMEOUT" } else { "HOST_UNAVAILABLE" })
        })?;
    let status = response.status();
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| ClientError::local("HOST_UNAVAILABLE"))?
    {
        if bytes.len() + chunk.len() > MAX_ALLOCATION_BODY {
            return Err(ClientError::local("PAIRED_HOST_INVALID_RESPONSE"));
        }
        bytes.extend_from_slice(&chunk);
    }
    if !status.is_success() {
        return Err(allocation_error(status.as_u16(), &bytes));
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Allocated {
        session_id: String,
        machine_id: String,
        #[serde(default)]
        opaque: bool,
    }
    let allocated: Allocated = serde_json::from_slice(&bytes)
        .map_err(|_| ClientError::local("PAIRED_HOST_INVALID_RESPONSE"))?;
    // A non-opaque session would be proxied by the relay in plaintext: refuse it.
    if !allocated.opaque
        || allocated.machine_id != machine_id
        || !session_id_valid(&allocated.session_id)
    {
        return Err(ClientError::local("PAIRED_HOST_INVALID_RESPONSE"));
    }
    Ok(allocated.session_id)
}

/// One complete HTTP/1.1 response read from the tunnel. Header names are lowercased.
#[derive(Debug)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}
impl HttpResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

/// Parses one response from the front of `buf`. `Ok(None)` means more bytes are needed;
/// `Ok(Some((response, consumed)))` reports how many bytes the response used. A body over
/// `limit` is refused as soon as it is known, before it is buffered.
pub(crate) fn parse_response(
    buf: &[u8],
    limit: usize,
) -> Result<Option<(HttpResponse, usize)>, ClientError> {
    let invalid = || ClientError::local("PAIRED_HOST_INVALID_RESPONSE");
    let Some(head_end) = find(buf, b"\r\n\r\n") else {
        if buf.len() > MAX_RESPONSE_HEAD {
            return Err(invalid());
        }
        return Ok(None);
    };
    if head_end > MAX_RESPONSE_HEAD {
        return Err(invalid());
    }
    let head = std::str::from_utf8(&buf[..head_end]).map_err(|_| invalid())?;
    let mut lines = head.split("\r\n");
    let status_line = lines.next().ok_or_else(invalid)?;
    let mut parts = status_line.splitn(3, ' ');
    if !parts.next().is_some_and(|version| version.starts_with("HTTP/1.")) {
        return Err(invalid());
    }
    let status: u16 = parts
        .next()
        .filter(|code| code.len() == 3)
        .and_then(|code| code.parse().ok())
        .ok_or_else(invalid)?;
    let mut headers = Vec::new();
    for line in lines {
        let (name, value) = line.split_once(':').ok_or_else(invalid)?;
        headers.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
    }
    let body_start = head_end + 4;
    let rest = &buf[body_start..];
    let response = |body: Vec<u8>| HttpResponse {
        status,
        headers: headers.clone(),
        body,
    };
    if status == 204 || status == 304 || (100..200).contains(&status) {
        return Ok(Some((response(Vec::new()), body_start)));
    }
    let chunked = headers.iter().any(|(name, value)| {
        name == "transfer-encoding" && value.to_ascii_lowercase().contains("chunked")
    });
    if chunked {
        let mut body = Vec::new();
        let mut at = 0usize;
        loop {
            let Some(line_end) = find(&rest[at..], b"\r\n") else {
                return Ok(None);
            };
            let line = std::str::from_utf8(&rest[at..at + line_end]).map_err(|_| invalid())?;
            let size_text = line.split(';').next().unwrap_or("").trim();
            let size = usize::from_str_radix(size_text, 16).map_err(|_| invalid())?;
            at += line_end + 2;
            if size == 0 {
                // Trailers (normally none) end with an empty line.
                let Some(trailer_end) = find(&rest[at..], b"\r\n") else {
                    return Ok(None);
                };
                if trailer_end == 0 {
                    return Ok(Some((response(body), body_start + at + 2)));
                }
                let Some(end) = find(&rest[at..], b"\r\n\r\n") else {
                    return Ok(None);
                };
                return Ok(Some((response(body), body_start + at + end + 4)));
            }
            if body.len().saturating_add(size) > limit {
                return Err(ClientError::local("PAYLOAD_TOO_LARGE"));
            }
            if rest.len() < at + size + 2 {
                return Ok(None);
            }
            if &rest[at + size..at + size + 2] != b"\r\n" {
                return Err(invalid());
            }
            body.extend_from_slice(&rest[at..at + size]);
            at += size + 2;
        }
    }
    let length = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .map(|(_, value)| value.parse::<usize>().map_err(|_| invalid()))
        .transpose()?
        // A kept-alive tunnel cannot delimit a body that names no length.
        .ok_or_else(invalid)?;
    if length > limit {
        return Err(ClientError::local("PAYLOAD_TOO_LARGE"));
    }
    if rest.len() < length {
        return Ok(None);
    }
    Ok(Some((response(rest[..length].to_vec()), body_start + length)))
}

/// Encodes one HTTP/1.1 request for the machine's loopback gateway.
fn encode_request(
    method: &str,
    path_and_query: &str,
    headers: &[(&str, String)],
    body: Option<&[u8]>,
) -> Result<Vec<u8>, ClientError> {
    let invalid = || ClientError::local("INVALID_REQUEST");
    let clean = |text: &str| !text.bytes().any(|b| b.is_ascii_control() || b == b' ');
    if !path_and_query.starts_with('/') || !clean(path_and_query) || !clean(method) {
        return Err(invalid());
    }
    let body = body.unwrap_or_default();
    let mut out = format!("{method} {path_and_query} HTTP/1.1\r\nHost: localhost\r\n");
    for (name, value) in headers {
        if !clean(*name) || value.bytes().any(|b| b == b'\r' || b == b'\n') {
            return Err(invalid());
        }
        out.push_str(&format!("{name}: {value}\r\n"));
    }
    out.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
    let mut bytes = out.into_bytes();
    bytes.extend_from_slice(body);
    Ok(bytes)
}

/// An HTTP/1.1 conversation with the machine's loopback gateway inside one opaque session.
pub struct SecureHttpSession {
    stream: RelayAttachedStream,
}

/// Allocates an opaque session with `allocation_bearer` (device token, or an account
/// session while redeeming a grant) and completes the attach handshake.
async fn establish_stream(
    http: &reqwest::Client,
    relay_origin: &str,
    allocation_bearer: &str,
    machine_id: &str,
    attach: &HostAttach,
) -> Result<RelayAttachedStream, ClientError> {
    let relay_origin = crate::remote::relay_client::live_relay_url(relay_origin).into_owned();
    let dir = client_attach_dir()?;
    tokio::time::timeout(ESTABLISH_TIMEOUT, async {
        let session_id =
            allocate_e2e_session(http, &relay_origin, allocation_bearer, machine_id).await?;
        let attached = attach_relay_session(
            &relay_origin,
            &session_id,
            machine_id,
            &attach.machine_attach_public_key,
            &attach.enrollment_epoch,
            &dir,
        )
        .await;
        // The origin record only matters while a session is being dialled.
        super::path_select::release_attach_origin(&session_id);
        attached.map_err(|error| {
            tracing::warn!("paired host e2e attach failed: {error}");
            ClientError::local("HOST_UNAVAILABLE")
        })
    })
    .await
    .map_err(|_| ClientError::local("TIMEOUT"))?
}

impl SecureHttpSession {
    pub async fn establish(
        http: &reqwest::Client,
        relay_origin: &str,
        allocation_bearer: &str,
        machine_id: &str,
        attach: &HostAttach,
    ) -> Result<Self, ClientError> {
        establish_stream(http, relay_origin, allocation_bearer, machine_id, attach)
            .await
            .map(|stream| Self { stream })
    }

    /// Sends one request and reads one complete response. On failure the second value says
    /// whether any request byte may have reached the machine. The bool in the answer says
    /// whether the session can carry another request.
    pub async fn request(
        &mut self,
        method: &str,
        path_and_query: &str,
        headers: &[(&str, String)],
        body: Option<&[u8]>,
        limit: usize,
    ) -> Result<(HttpResponse, bool), (ClientError, bool)> {
        let bytes =
            encode_request(method, path_and_query, headers, body).map_err(|error| (error, false))?;
        let mut sent_any = false;
        for chunk in bytes.chunks(MAX_ATTACH_FRAME) {
            if self.stream.send_frame(chunk).await.is_err() {
                return Err((ClientError::local("HOST_UNAVAILABLE"), sent_any));
            }
            sent_any = true;
        }
        let mut buf = Vec::new();
        loop {
            if let Some((response, consumed)) =
                parse_response(&buf, limit).map_err(|error| (error, true))?
            {
                let close = response
                    .header("connection")
                    .is_some_and(|value| value.eq_ignore_ascii_case("close"));
                return Ok((response, consumed == buf.len() && !close));
            }
            let frame = self
                .stream
                .recv_frame()
                .await
                .map_err(|_| (ClientError::local("HOST_UNAVAILABLE"), true))?;
            buf.extend_from_slice(&frame);
        }
    }
}

/// Everything needed to reach one paired host generation end-to-end encrypted.
pub struct SecureTarget<'a> {
    pub http: &'a reqwest::Client,
    pub host_id: &'a str,
    pub generation: u64,
    pub relay_origin: &'a str,
    pub machine_id: &'a str,
    pub attach: &'a HostAttach,
    pub bearer: &'a str,
}

type Slot = Arc<tokio::sync::Mutex<Option<SecureHttpSession>>>;
/// One shared session per host generation; requests on it are serialized by its lock.
static SESSIONS: LazyLock<Mutex<HashMap<(String, u64), Slot>>> = LazyLock::new(Default::default);

fn session_slot(host_id: &str, generation: u64) -> Slot {
    let mut sessions = SESSIONS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    // A re-pair or forget retires every older generation's session.
    sessions.retain(|(id, current), _| id != host_id || *current == generation);
    Arc::clone(
        sessions
            .entry((host_id.to_owned(), generation))
            .or_default(),
    )
}

/// Sends one request over the host's shared encrypted session, establishing it when
/// absent. The session is taken out of its slot while in use, so a cancelled caller drops
/// it instead of leaving half a response behind. A reused session that turns out broken is
/// replaced once; the request is replayed only when it is idempotent or provably unsent.
pub async fn shared_request(
    target: &SecureTarget<'_>,
    method: &str,
    path_and_query: &str,
    headers: &[(&str, String)],
    body: Option<&[u8]>,
    limit: usize,
    idempotent: bool,
) -> Result<HttpResponse, ClientError> {
    let slot = session_slot(target.host_id, target.generation);
    let mut guard = slot.lock().await;
    let establish = || {
        SecureHttpSession::establish(
            target.http,
            target.relay_origin,
            target.bearer,
            target.machine_id,
            target.attach,
        )
    };
    let (mut session, reused) = match guard.take() {
        Some(session) => (session, true),
        None => (establish().await?, false),
    };
    match session
        .request(method, path_and_query, headers, body, limit)
        .await
    {
        Ok((response, reusable)) => {
            if reusable {
                *guard = Some(session);
            }
            Ok(response)
        }
        Err((error, reached))
            if reused && error.code == "HOST_UNAVAILABLE" && (idempotent || !reached) =>
        {
            drop(session);
            let mut fresh = establish().await?;
            let (response, reusable) = fresh
                .request(method, path_and_query, headers, body, limit)
                .await
                .map_err(|(error, _)| error)?;
            if reusable {
                *guard = Some(fresh);
            }
            Ok(response)
        }
        Err((error, _)) => Err(error),
    }
}

/// Opens a WebSocket to the machine's gateway inside its own fresh encrypted session.
/// The bearer travels inside the tunnel, so no socket ticket is minted.
pub async fn open_secure_websocket(
    target: &SecureTarget<'_>,
    path_and_query: &str,
    config: WebSocketConfig,
) -> Result<PairedSocket, ClientError> {
    let invalid = || ClientError::local("INVALID_REQUEST");
    let secure = establish_stream(
        target.http,
        target.relay_origin,
        target.bearer,
        target.machine_id,
        target.attach,
    )
    .await?;
    let (local, gateway) = tokio::io::duplex(256 * 1024);
    tokio::spawn(async move {
        // Ends when either side closes; dropping the socket closes `local`.
        let _ = crate::remote::session_transport::proxy_secure_to_gateway(secure, gateway).await;
    });
    let mut request = format!("ws://localhost{path_and_query}")
        .into_client_request()
        .map_err(|_| invalid())?;
    let headers = request.headers_mut();
    headers.insert(
        "authorization",
        format!("Bearer {}", target.bearer)
            .parse()
            .map_err(|_| invalid())?,
    );
    headers.insert(CLIENT_BUILD_HEADER, CLIENT_BUILD.parse().map_err(|_| invalid())?);
    let (socket, _) = tokio::time::timeout(
        ESTABLISH_TIMEOUT,
        tokio_tungstenite::client_async_with_config(
            request,
            MaybeTlsStream::Plain(PairedStream::Tunnel(local)),
            Some(config),
        ),
    )
    .await
    .map_err(|_| ClientError::local("TIMEOUT"))?
    .map_err(|error| {
        tracing::warn!("paired host e2e websocket upgrade failed: {error}");
        ClientError::local("HOST_UNAVAILABLE")
    })?;
    Ok(socket)
}

static REPAIR_WARNED: LazyLock<Mutex<HashSet<(String, u64)>>> = LazyLock::new(Default::default);
static UNSUPPORTED_WARNED: LazyLock<Mutex<HashSet<(String, u64)>>> =
    LazyLock::new(Default::default);

/// The relay predates `?e2e=1` device-token allocation, so this host generation falls back
/// to the legacy plaintext relay path; say so once per host generation.
pub fn warn_e2e_unsupported(host_id: &str, generation: u64) {
    let first = UNSUPPORTED_WARNED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert((host_id.to_owned(), generation));
    if first {
        tracing::warn!(
            target: "ferryx::paired_host::e2e",
            code = "E2E_UNSUPPORTED",
            host = %host_id,
            generation,
            "relay does not support end-to-end device-token allocation; falling back to the plaintext relay path"
        );
    }
}

/// A relay-connected host this client cannot encrypt to keeps working in plaintext; say so
/// once per host generation so the re-pair prompt has a trail.
pub fn warn_repair_required(host_id: &str, generation: u64) {
    let first = REPAIR_WARNED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert((host_id.to_owned(), generation));
    if first {
        tracing::warn!(
            target: "ferryx::paired_host::e2e",
            code = "REPAIR_REQUIRED",
            host = %host_id,
            generation,
            "paired host has no attach key; relay traffic stays plaintext until it is re-paired through the account"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_content_length_response_completes_only_when_the_body_has_arrived() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\n\r\n{\"ok\":true}";
        assert!(parse_response(&raw[..raw.len() - 3], 1024).unwrap().is_none());
        let (response, consumed) = parse_response(raw, 1024).unwrap().unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.header("content-type"), Some("application/json"));
        assert_eq!(response.body, b"{\"ok\":true}");
        assert_eq!(consumed, raw.len());
    }

    #[test]
    fn a_chunked_response_is_reassembled() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\n{\"a\"\r\n3\r\n:1}\r\n0\r\n\r\n";
        assert!(parse_response(&raw[..raw.len() - 2], 1024).unwrap().is_none());
        let (response, consumed) = parse_response(raw, 1024).unwrap().unwrap();
        assert_eq!(response.body, b"{\"a\":1}");
        assert_eq!(consumed, raw.len());
    }

    #[test]
    fn an_over_limit_body_is_refused_before_it_is_buffered() {
        let declared = b"HTTP/1.1 200 OK\r\nContent-Length: 4096\r\n\r\n";
        assert_eq!(
            parse_response(declared, 1024).unwrap_err().code,
            "PAYLOAD_TOO_LARGE"
        );
        let chunked = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n800\r\n";
        assert_eq!(
            parse_response(chunked, 1024).unwrap_err().code,
            "PAYLOAD_TOO_LARGE"
        );
    }

    #[test]
    fn a_bodyless_status_needs_no_length() {
        let raw = b"HTTP/1.1 204 No Content\r\n\r\n";
        let (response, consumed) = parse_response(raw, 16).unwrap().unwrap();
        assert_eq!((response.status, response.body.len(), consumed), (204, 0, raw.len()));
    }

    #[test]
    fn an_e2e_required_refusal_is_a_repair_prompt() {
        let body = br#"{"error":{"code":"E2E_REQUIRED","message":"x","retryable":false,"requestId":"","details":{}}}"#;
        assert_eq!(allocation_error(426, body).code, "REPAIR_REQUIRED");
        assert_eq!(allocation_error(401, b"{}").code, "E2E_UNSUPPORTED");
        assert_eq!(allocation_error(409, br#"{"code":"MACHINE_OFFLINE"}"#).code, "HOST_UNAVAILABLE");
    }

    #[test]
    fn an_old_relay_refusal_is_e2e_unsupported() {
        assert_eq!(allocation_error(404, b"").code, "E2E_UNSUPPORTED");
        assert_eq!(allocation_error(405, b"").code, "E2E_UNSUPPORTED");
        assert_eq!(allocation_error(401, br#"{"code":"UNAUTHORIZED"}"#).code, "E2E_UNSUPPORTED");
        assert_eq!(allocation_error(401, br#"{"code":"TOKEN_REVOKED"}"#).code, "UNAUTHORIZED");
        assert_eq!(allocation_error(404, br#"{"code":"MACHINE_NOT_FOUND"}"#).code, "PAIRED_HOST_WRONG_MACHINE");
        assert_eq!(allocation_error(426, b"").code, "REPAIR_REQUIRED");
        assert_eq!(allocation_error(409, b"").code, "HOST_UNAVAILABLE");
    }
}
