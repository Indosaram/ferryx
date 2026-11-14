//! HTTP proxy egress for relay traffic, hand-rolled on HTTP `CONNECT` (no proxy crate).
//!
//! Every outbound connection to a relay - the daemon's control and data channels, a paired
//! client's attach and legacy sockets, and the paired clients' relay HTTP calls - asks
//! [`select`] whether it goes through a proxy. First match wins:
//!
//! Direct (non-relay) paths to a paired host - LAN, Tailscale, SSH forward, the direct QUIC
//! adapter - never reach this module: their callers dial without a proxy by path kind, however
//! the address looks. For relay destinations:
//!
//! 1. Loopback and local-network destinations are always dialled directly: `localhost`,
//!    `*.localhost`, `*.local`, Tailscale `*.ts.net` names, single-label names, and loopback,
//!    private, link-local, shared (100.64.0.0/10, Tailscale) and unique-local IP literals.
//! 2. `no_proxy` / `NO_PROXY`: comma or space separated entries. `*` matches everything; a
//!    name matches itself and every subdomain (`example.com`, `.example.com` and
//!    `*.example.com` all match `relay.example.com`); an IP literal matches that address
//!    (`[2001:db8::1]` or bare `2001:db8::1` for IPv6); `addr/prefix` matches a CIDR block
//!    (`[2001:db8::]/32` or bare `2001:db8::/32` for IPv6);
//!    a `:port` suffix (`relay.example.com:8443`, `[2001:db8::1]:443`) limits an entry to
//!    that port.
//! 3. The proxy: `FERRYX_RELAY_PROXY` first (`direct` or `none` turns proxying off), then
//!    `https_proxy` / `HTTPS_PROXY` for `wss`/`https` targets or `http_proxy` / `HTTP_PROXY`
//!    for `ws`/`http` targets, then `all_proxy` / `ALL_PROXY`. Empty values count as unset.
//!
//! Only `http://` proxies are supported (a value without a scheme is read as `http://`). An
//! `https://` proxy (TLS to the proxy itself) or a SOCKS proxy is refused with
//! [`RelayProxyError::UnsupportedScheme`] instead of being ignored, so a misconfigured proxy is
//! reported as such rather than as a relay outage; the relay's own TLS still runs end to end
//! inside the tunnel. `user:password@` in the proxy URL is sent as `Proxy-Authorization: Basic`
//! and is never logged or shown in an error. The proxy `CONNECT` step (TCP connect plus the
//! proxy's answer) is bounded by [`CONNECT_TIMEOUT`].

use base64::Engine;
use std::net::IpAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::{
    self, client::IntoClientRequest, error::UrlError, handshake::client::Response,
};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

/// Bound on reaching the proxy and receiving its answer to `CONNECT`.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Bound on the proxy's `CONNECT` response head.
const MAX_CONNECT_RESPONSE_HEAD: usize = 8 * 1024;

/// A configured `http://` proxy. Credentials never appear in `Display` or `Debug`.
#[derive(Clone)]
pub(crate) struct Proxy {
    var: &'static str,
    host: String,
    port: u16,
    authorization: Option<String>,
    /// The URL as configured, credentials included, for reqwest's own proxy support.
    url: url::Url,
}

impl std::fmt::Display for Proxy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "http://{} (from {})",
            authority(&self.host, self.port),
            self.var
        )
    }
}

impl std::fmt::Debug for Proxy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Proxy({self})")
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum RelayProxyError {
    #[error(
        "relay proxy {proxy} from {var} uses unsupported scheme {scheme}://; only http:// proxies are supported (set FERRYX_RELAY_PROXY=direct to bypass it)"
    )]
    UnsupportedScheme {
        var: &'static str,
        proxy: String,
        scheme: String,
    },
    #[error("relay proxy from {var} is not a valid proxy URL")]
    Invalid { var: &'static str },
    #[error("relay proxy {proxy} is unreachable while connecting to {target}: {error}")]
    Unreachable {
        proxy: String,
        target: String,
        error: std::io::Error,
    },
    #[error("relay proxy {proxy} refused CONNECT to {target}: HTTP {status} {reason}")]
    Refused {
        proxy: String,
        target: String,
        status: u16,
        reason: String,
    },
    #[error("relay proxy {proxy} sent no valid answer to CONNECT {target}")]
    BadResponse { proxy: String, target: String },
    #[error("relay proxy {proxy} did not complete CONNECT to {target} within {seconds:.1}s")]
    Timeout {
        proxy: String,
        target: String,
        seconds: f64,
    },
}

pub(crate) fn process_env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

/// `host:port`, bracketing an IPv6 literal.
fn authority(host: &str, port: u16) -> String {
    if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

fn normalize_host(host: &str) -> String {
    host.trim_start_matches('[')
        .trim_end_matches(']')
        .trim_end_matches('.')
        .to_ascii_lowercase()
}

/// Loopback and local-network destinations that are always dialled directly.
fn is_local_destination(host: &str) -> bool {
    fn local_v4(ip: std::net::Ipv4Addr) -> bool {
        let [a, b, ..] = ip.octets();
        ip.is_loopback()
            || ip.is_private()
            || ip.is_link_local()
            || ip.is_unspecified()
            || (a == 100 && (64..128).contains(&b))
    }
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => local_v4(ip),
        Ok(IpAddr::V6(ip)) => {
            let first = ip.segments()[0];
            ip.is_loopback()
                || ip.is_unspecified()
                || first & 0xfe00 == 0xfc00
                || first & 0xffc0 == 0xfe80
                || ip.to_ipv4_mapped().is_some_and(local_v4)
        }
        Err(_) => {
            host == "localhost"
                || host.ends_with(".localhost")
                || host.ends_with(".local")
                || host.ends_with(".ts.net")
                || !host.contains('.')
        }
    }
}

/// Splits a `NO_PROXY` entry into its host pattern and optional port.
fn split_entry(entry: &str) -> Option<(String, Option<u16>)> {
    if let Some(rest) = entry.strip_prefix('[') {
        let (host, tail) = rest.split_once(']')?;
        return match tail.strip_prefix(':') {
            Some(port) => Some((host.to_owned(), Some(port.parse().ok()?))),
            None if tail.is_empty() => Some((host.to_owned(), None)),
            // A bracketed IPv6 CIDR block: `[2001:db8::]/32`.
            None if tail.starts_with('/') => Some((format!("{host}{tail}"), None)),
            None => None,
        };
    }
    if entry.matches(':').count() > 1 {
        // A bare IPv6 literal (optionally with a CIDR prefix) carries no port.
        return Some((entry.to_owned(), None));
    }
    match entry.split_once(':') {
        Some((host, port)) => Some((host.to_owned(), Some(port.parse().ok()?))),
        None => Some((entry.to_owned(), None)),
    }
}

fn in_cidr(host: IpAddr, network: IpAddr, prefix: u32) -> bool {
    match (host, network) {
        (IpAddr::V4(host), IpAddr::V4(network)) if prefix <= 32 => {
            let mask = u32::MAX.checked_shl(32 - prefix).unwrap_or(0);
            u32::from(host) & mask == u32::from(network) & mask
        }
        (IpAddr::V6(host), IpAddr::V6(network)) if prefix <= 128 => {
            let mask = u128::MAX.checked_shl(128 - prefix).unwrap_or(0);
            u128::from(host) & mask == u128::from(network) & mask
        }
        _ => false,
    }
}

/// Whether `host:port` (normalized) is excluded by a `NO_PROXY` list.
fn no_proxy_matches(list: &str, host: &str, port: u16) -> bool {
    let host_ip = host.parse::<IpAddr>().ok();
    list.split(|c: char| c == ',' || c.is_whitespace())
        .filter(|entry| !entry.is_empty())
        .any(|entry| {
            if entry == "*" {
                return true;
            }
            let Some((pattern, entry_port)) = split_entry(entry) else {
                return false;
            };
            if entry_port.is_some_and(|entry_port| entry_port != port) {
                return false;
            }
            if let Some((network, prefix)) = pattern.split_once('/') {
                return match (host_ip, network.parse::<IpAddr>(), prefix.parse::<u32>()) {
                    (Some(host), Ok(network), Ok(prefix)) => in_cidr(host, network, prefix),
                    _ => false,
                };
            }
            if let Ok(pattern_ip) = pattern.parse::<IpAddr>() {
                return host_ip == Some(pattern_ip);
            }
            let domain = normalize_host(pattern.trim_start_matches('*').trim_start_matches('.'));
            !domain.is_empty()
                && (host == domain
                    || host
                        .strip_suffix(domain.as_str())
                        .is_some_and(|prefix| prefix.ends_with('.')))
        })
}

fn percent_decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let escaped = if bytes[index] == b'%' {
            raw.get(index + 1..index + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        } else {
            None
        };
        match escaped {
            Some(byte) => {
                decoded.push(byte);
                index += 3;
            }
            None => {
                decoded.push(bytes[index]);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn parse_proxy(var: &'static str, value: &str) -> Result<Proxy, RelayProxyError> {
    let candidate = if value.contains("://") {
        value.to_owned()
    } else {
        format!("http://{value}")
    };
    let url = url::Url::parse(&candidate).map_err(|_| RelayProxyError::Invalid { var })?;
    let host = url
        .host_str()
        .map(normalize_host)
        .filter(|host| !host.is_empty())
        .ok_or(RelayProxyError::Invalid { var })?;
    if url.scheme() != "http" {
        let port = url
            .port()
            .map(|port| format!(":{port}"))
            .unwrap_or_default();
        return Err(RelayProxyError::UnsupportedScheme {
            var,
            proxy: format!(
                "{}://{}{port}",
                url.scheme(),
                url.host_str().unwrap_or_default()
            ),
            scheme: url.scheme().to_owned(),
        });
    }
    let port = url.port_or_known_default().unwrap_or(80);
    let authorization = (!url.username().is_empty() || url.password().is_some()).then(|| {
        let credentials = format!(
            "{}:{}",
            percent_decode(url.username()),
            percent_decode(url.password().unwrap_or_default())
        );
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(credentials)
        )
    });
    Ok(Proxy {
        var,
        host,
        port,
        authorization,
        url,
    })
}

/// The proxy for one relay destination, or `None` to dial it directly. `secure` is true for
/// `wss`/`https` targets. `env` reads one environment variable.
pub(crate) fn select(
    secure: bool,
    host: &str,
    port: u16,
    env: &(dyn Fn(&str) -> Option<String> + Sync),
) -> Result<Option<Proxy>, RelayProxyError> {
    let host = normalize_host(host);
    if is_local_destination(&host) {
        return Ok(None);
    }
    let read = |name: &'static str| {
        env(name)
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .map(|value| (name, value))
    };
    if let Some((_, list)) = read("no_proxy").or_else(|| read("NO_PROXY")) {
        if no_proxy_matches(&list, &host, port) {
            return Ok(None);
        }
    }
    let scheme_vars: [&'static str; 2] = if secure {
        ["https_proxy", "HTTPS_PROXY"]
    } else {
        ["http_proxy", "HTTP_PROXY"]
    };
    let configured = std::iter::once("FERRYX_RELAY_PROXY")
        .chain(scheme_vars)
        .chain(["all_proxy", "ALL_PROXY"])
        .find_map(read);
    match configured {
        None => Ok(None),
        Some(("FERRYX_RELAY_PROXY", value))
            if value.eq_ignore_ascii_case("direct") || value.eq_ignore_ascii_case("none") =>
        {
            Ok(None)
        }
        Some((var, value)) => parse_proxy(var, &value).map(Some),
    }
}

/// Reads the proxy's answer to `CONNECT` one byte at a time, so no byte of the tunnelled
/// stream is consumed, and returns the status line.
async fn read_connect_status(stream: &mut TcpStream) -> std::io::Result<Option<String>> {
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        if head.len() >= MAX_CONNECT_RESPONSE_HEAD {
            return Ok(None);
        }
        let mut byte = [0u8; 1];
        if stream.read(&mut byte).await? == 0 {
            return Ok(None);
        }
        head.push(byte[0]);
    }
    Ok(String::from_utf8_lossy(&head)
        .lines()
        .next()
        .map(str::to_owned))
}

/// Opens a `CONNECT` tunnel to `host:port` through `proxy`, bounded by `limit`.
async fn tunnel(
    proxy: &Proxy,
    host: &str,
    port: u16,
    limit: Duration,
) -> Result<TcpStream, RelayProxyError> {
    let target = authority(host, port);
    let unreachable = |error| RelayProxyError::Unreachable {
        proxy: proxy.to_string(),
        target: target.clone(),
        error,
    };
    let exchange = async {
        let mut stream = TcpStream::connect((proxy.host.as_str(), proxy.port))
            .await
            .map_err(unreachable)?;
        let mut request = format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n");
        if let Some(authorization) = &proxy.authorization {
            request.push_str(&format!("Proxy-Authorization: {authorization}\r\n"));
        }
        request.push_str("\r\n");
        stream
            .write_all(request.as_bytes())
            .await
            .map_err(unreachable)?;
        let bad_response = || RelayProxyError::BadResponse {
            proxy: proxy.to_string(),
            target: target.clone(),
        };
        let status_line = read_connect_status(&mut stream)
            .await
            .map_err(unreachable)?
            .ok_or_else(bad_response)?;
        let mut parts = status_line.splitn(3, ' ');
        let status = match (parts.next(), parts.next()) {
            (Some(version), Some(code)) if version.starts_with("HTTP/") && code.len() == 3 => {
                code.parse::<u16>().map_err(|_| bad_response())?
            }
            _ => return Err(bad_response()),
        };
        if !(200..300).contains(&status) {
            let reason: String = parts
                .next()
                .unwrap_or_default()
                .chars()
                .filter(|c| c.is_ascii_graphic() || *c == ' ')
                .take(80)
                .collect();
            return Err(RelayProxyError::Refused {
                proxy: proxy.to_string(),
                target: target.clone(),
                status,
                reason,
            });
        }
        Ok(stream)
    };
    tokio::time::timeout(limit, exchange)
        .await
        .map_err(|_| RelayProxyError::Timeout {
            proxy: proxy.to_string(),
            target: target.clone(),
            seconds: limit.as_secs_f64(),
        })?
}

/// A TCP stream to a relay endpoint, tunnelled through the proxy `env` configures when one
/// applies. A proxy failure is an `io::Error` wrapping [`RelayProxyError`], whose message
/// names the proxy. Only relay transport dials through here; direct paths never do.
pub(crate) async fn dial_with(
    secure: bool,
    host: &str,
    port: u16,
    env: &(dyn Fn(&str) -> Option<String> + Sync),
) -> std::io::Result<TcpStream> {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    match select(secure, host, port, env).map_err(std::io::Error::other)? {
        None => TcpStream::connect((host, port)).await,
        Some(proxy) => {
            tracing::debug!(
                "relay connection to {} via proxy {proxy}",
                authority(host, port)
            );
            tunnel(&proxy, host, port, CONNECT_TIMEOUT)
                .await
                .map_err(std::io::Error::other)
        }
    }
}

async fn connect_websocket_with<R: IntoClientRequest>(
    request: R,
    env: &(dyn Fn(&str) -> Option<String> + Sync),
) -> Result<(WebSocketStream<MaybeTlsStream<TcpStream>>, Response), tungstenite::Error> {
    let request = request.into_client_request()?;
    let uri = request.uri();
    let secure = match uri.scheme_str() {
        Some("wss") => true,
        Some("ws") => false,
        _ => return Err(tungstenite::Error::Url(UrlError::UnsupportedUrlScheme)),
    };
    let host = uri
        .host()
        .ok_or(tungstenite::Error::Url(UrlError::NoHostName))?
        .to_owned();
    let port = uri.port_u16().unwrap_or(if secure { 443 } else { 80 });
    let stream = dial_with(secure, &host, port, env).await?;
    tokio_tungstenite::client_async_tls_with_config(request, stream, None, None).await
}

/// `tokio_tungstenite::connect_async` for relay endpoints, through the configured proxy.
pub(crate) async fn connect_websocket<R: IntoClientRequest>(
    request: R,
) -> Result<(WebSocketStream<MaybeTlsStream<TcpStream>>, Response), tungstenite::Error> {
    connect_websocket_with(request, &process_env).await
}

/// The proxy policy above for reqwest clients that reach relays only; a direct-path client
/// is built with `.no_proxy()` instead. An unusable proxy setting is logged and the request
/// goes direct; reqwest reads the credentials from the returned URL itself.
pub(crate) fn reqwest_proxy() -> reqwest::Proxy {
    reqwest_proxy_with(process_env)
}

/// [`reqwest_proxy`] reading the environment through `env`.
pub(crate) fn reqwest_proxy_with(
    env: impl Fn(&str) -> Option<String> + Send + Sync + 'static,
) -> reqwest::Proxy {
    reqwest::Proxy::custom(move |url: &url::Url| {
        let host = url.host_str()?;
        let port = url.port_or_known_default()?;
        match select(url.scheme() == "https", host, port, &env) {
            Ok(proxy) => proxy.map(|proxy| proxy.url),
            Err(error) => {
                tracing::warn!("{error}; connecting directly");
                None
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::{SinkExt, StreamExt};
    use std::collections::HashMap;
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::Message;

    const LIMIT: Duration = Duration::from_secs(5);

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + Sync {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        move |name| map.get(name).cloned()
    }

    fn chosen(
        secure: bool,
        host: &str,
        port: u16,
        pairs: &[(&str, &str)],
    ) -> Option<(&'static str, String, u16)> {
        select(secure, host, port, &env(pairs))
            .expect("valid proxy settings")
            .map(|proxy| (proxy.var, proxy.host, proxy.port))
    }

    #[test]
    fn selection_follows_the_override_then_scheme_then_all_proxy_order() {
        let all = [
            ("FERRYX_RELAY_PROXY", "http://override:1"),
            ("https_proxy", "http://lower-https:2"),
            ("HTTPS_PROXY", "http://upper-https:3"),
            ("http_proxy", "http://lower-http:4"),
            ("ALL_PROXY", "http://all:5"),
        ];
        let relay = "relay.example.com";
        assert_eq!(
            chosen(true, relay, 443, &all),
            Some(("FERRYX_RELAY_PROXY", "override".into(), 1))
        );
        assert_eq!(
            chosen(true, relay, 443, &all[1..]),
            Some(("https_proxy", "lower-https".into(), 2))
        );
        assert_eq!(
            chosen(true, relay, 443, &all[2..]),
            Some(("HTTPS_PROXY", "upper-https".into(), 3))
        );
        assert_eq!(
            chosen(true, relay, 443, &all[3..]),
            Some(("ALL_PROXY", "all".into(), 5))
        );
        assert_eq!(
            chosen(false, relay, 80, &all[1..]),
            Some(("http_proxy", "lower-http".into(), 4))
        );
        assert_eq!(chosen(true, relay, 443, &[]), None);
        assert_eq!(
            chosen(true, relay, 443, &[("HTTPS_PROXY", "  ")]),
            None,
            "empty counts as unset"
        );
        assert_eq!(
            chosen(true, relay, 443, &[("HTTPS_PROXY", "proxy.corp:3128")]),
            Some(("HTTPS_PROXY", "proxy.corp".into(), 3128)),
            "a bare host:port is an http proxy"
        );
        assert_eq!(
            chosen(true, relay, 443, &[("HTTPS_PROXY", "http://proxy.corp")]),
            Some(("HTTPS_PROXY", "proxy.corp".into(), 80))
        );
        for off in ["direct", "NONE"] {
            assert_eq!(
                chosen(
                    true,
                    relay,
                    443,
                    &[("FERRYX_RELAY_PROXY", off), ("HTTPS_PROXY", "http://p:1")]
                ),
                None,
                "FERRYX_RELAY_PROXY={off} disables proxying"
            );
        }
    }

    #[test]
    fn loopback_and_local_network_destinations_bypass_the_proxy() {
        let proxied = [
            ("FERRYX_RELAY_PROXY", "http://proxy:3128"),
            ("ALL_PROXY", "http://proxy:3128"),
        ];
        for host in [
            "localhost",
            "relay.localhost",
            "127.0.0.1",
            "127.3.2.1",
            "[::1]",
            "::1",
            "0.0.0.0",
            "10.1.2.3",
            "172.16.0.9",
            "192.168.1.10",
            "169.254.1.1",
            "100.100.1.1",
            "fd7a:115c:a1e0::1",
            "[fe80::1]",
            "::ffff:192.168.1.2",
            "nas.local",
            "mac-mini.tailnet-1234.ts.net.",
            "devbox",
        ] {
            assert_eq!(
                chosen(true, host, 443, &proxied),
                None,
                "{host} must be dialled directly"
            );
        }
        for host in [
            "relay.example.com",
            "8.8.8.8",
            "100.128.0.1",
            "2001:db8::1",
            "[2606:4700::1]",
        ] {
            assert!(
                chosen(true, host, 443, &proxied).is_some(),
                "{host} must use the proxy"
            );
        }
    }

    #[test]
    fn no_proxy_excludes_suffixes_wildcards_ports_and_ip_literals() {
        let with = |list: &str| {
            [
                ("no_proxy", list.to_owned()),
                ("HTTPS_PROXY", "http://proxy:3128".to_owned()),
            ]
        };
        let proxied = |list: &str, host: &str, port: u16| {
            let pairs = with(list);
            let pairs: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
            chosen(true, host, port, &pairs).is_some()
        };
        for list in [
            "example.com",
            ".example.com",
            "*.example.com",
            "EXAMPLE.COM.",
            "other.org, example.com",
        ] {
            assert!(
                !proxied(list, "relay.example.com", 443),
                "{list} excludes a subdomain"
            );
            assert!(
                !proxied(list, "example.com", 443),
                "{list} excludes the domain itself"
            );
            assert!(
                proxied(list, "badexample.com", 443),
                "{list} is a suffix on a label boundary"
            );
        }
        assert!(!proxied("*", "relay.example.com", 443));
        assert!(!proxied(
            "relay.example.com:8443",
            "relay.example.com",
            8443
        ));
        assert!(
            proxied("relay.example.com:8443", "relay.example.com", 443),
            "the port limits the entry"
        );
        assert!(!proxied("[2001:db8::1]", "[2001:db8::1]", 443));
        assert!(
            !proxied("2001:db8::1", "2001:db8:0:0::1", 443),
            "IPv6 compares by address"
        );
        assert!(!proxied("[2001:db8::1]:443", "2001:db8::1", 443));
        assert!(proxied("[2001:db8::1]:8443", "2001:db8::1", 443));
        assert!(!proxied("203.0.113.0/24", "203.0.113.77", 443));
        assert!(proxied("203.0.113.0/24", "203.0.114.1", 443));
        assert!(!proxied("2001:db8::/32", "2001:db8:ffff::1", 443));
        assert!(
            !proxied("[2001:db8::]/32", "2001:db8::5", 443),
            "a bracketed IPv6 CIDR block excludes"
        );
        assert!(proxied("[2001:db8::]/32", "2001:db9::5", 443));
        assert!(proxied("203.0.113.9", "203.0.113.10", 443));
        // The uppercase list is read when the lowercase one is absent.
        assert_eq!(
            chosen(
                true,
                "relay.example.com",
                443,
                &[("NO_PROXY", "example.com"), ("HTTPS_PROXY", "http://p:1")]
            ),
            None
        );
    }

    #[test]
    fn credentials_become_basic_auth_and_never_reach_display_or_errors() {
        let proxy = select(
            true,
            "relay.example.com",
            443,
            &env(&[("HTTPS_PROXY", "http://al%40ice:s3cr%3At@proxy.corp:3128")]),
        )
        .unwrap()
        .unwrap();
        let expected = base64::engine::general_purpose::STANDARD.encode("al@ice:s3cr:t");
        assert_eq!(
            proxy.authorization.as_deref(),
            Some(format!("Basic {expected}").as_str())
        );
        for shown in [proxy.to_string(), format!("{proxy:?}")] {
            assert!(!shown.contains("s3cr"), "{shown}");
            assert!(shown.contains("http://proxy.corp:3128"), "{shown}");
            assert!(shown.contains("HTTPS_PROXY"), "{shown}");
        }

        for (value, scheme) in [
            ("https://user:hunter2@tls-proxy.corp:443", "https"),
            ("socks5://user:hunter2@socks.corp:1080", "socks5"),
        ] {
            let error = select(
                true,
                "relay.example.com",
                443,
                &env(&[("ALL_PROXY", value)]),
            )
            .unwrap_err();
            assert!(
                matches!(&error, RelayProxyError::UnsupportedScheme { var: "ALL_PROXY", scheme: s, .. } if s == scheme),
                "{error:?}"
            );
            let message = error.to_string();
            assert!(
                !message.contains("hunter2") && !message.contains("user:"),
                "{message}"
            );
            assert!(message.contains("ALL_PROXY"), "{message}");
        }
        let error = select(
            true,
            "relay.example.com",
            443,
            &env(&[("HTTPS_PROXY", "http://:pw@/")]),
        )
        .unwrap_err();
        assert!(
            matches!(error, RelayProxyError::Invalid { var: "HTTPS_PROXY" }),
            "{error:?}"
        );
        assert_eq!(authority("2001:db8::1", 443), "[2001:db8::1]:443");
    }

    /// A one-connection CONNECT proxy: records the request head, answers `status_line`, and on
    /// a 2xx pipes the tunnel to `upstream`.
    async fn spawn_connect_proxy(
        status_line: &'static str,
        upstream: std::net::SocketAddr,
    ) -> (std::net::SocketAddr, tokio::task::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (mut client, _) = listener.accept().await.unwrap();
            let mut head = Vec::new();
            while !head.ends_with(b"\r\n\r\n") {
                let mut byte = [0u8; 1];
                assert_eq!(client.read(&mut byte).await.unwrap(), 1);
                head.push(byte[0]);
            }
            client
                .write_all(format!("{status_line}\r\nVia: test-proxy\r\n\r\n").as_bytes())
                .await
                .unwrap();
            if status_line
                .split(' ')
                .nth(1)
                .is_some_and(|code| code.starts_with('2'))
            {
                let mut server = TcpStream::connect(upstream).await.unwrap();
                let _ = tokio::io::copy_bidirectional(&mut client, &mut server).await;
            }
            String::from_utf8(head).unwrap()
        });
        (address, task)
    }

    async fn spawn_echo_websocket() -> std::net::SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
            while let Some(Ok(message)) = socket.next().await {
                if message.is_text() && socket.send(message).await.is_err() {
                    break;
                }
            }
        });
        address
    }

    fn proxy_env(proxy: std::net::SocketAddr) -> impl Fn(&str) -> Option<String> + Sync {
        let value = format!("http://relay-user:relay-pass@{proxy}");
        move |name| (name == "FERRYX_RELAY_PROXY").then(|| value.clone())
    }

    #[tokio::test]
    async fn a_relay_websocket_is_tunnelled_through_the_proxy_with_basic_auth() {
        let upstream = spawn_echo_websocket().await;
        let (proxy, recorded) =
            spawn_connect_proxy("HTTP/1.1 200 Connection established", upstream).await;
        let url = format!("ws://relay.qa.invalid:{}/tunnel/control", upstream.port());
        let (mut socket, _) =
            tokio::time::timeout(LIMIT, connect_websocket_with(url, &proxy_env(proxy)))
                .await
                .expect("bounded connect")
                .expect("tunnelled handshake");
        socket.send(Message::Text("hello".into())).await.unwrap();
        let echoed = tokio::time::timeout(LIMIT, socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(echoed, Message::Text("hello".into()));
        drop(socket);
        let head = tokio::time::timeout(LIMIT, recorded)
            .await
            .unwrap()
            .unwrap();
        let target = format!("relay.qa.invalid:{}", upstream.port());
        assert!(
            head.starts_with(&format!("CONNECT {target} HTTP/1.1\r\n")),
            "{head}"
        );
        assert!(head.contains(&format!("\r\nHost: {target}\r\n")), "{head}");
        let credentials = base64::engine::general_purpose::STANDARD.encode("relay-user:relay-pass");
        assert!(
            head.contains(&format!("\r\nProxy-Authorization: Basic {credentials}\r\n")),
            "{head}"
        );
    }

    #[tokio::test]
    async fn a_refused_connect_is_an_error_naming_the_proxy() {
        let upstream = spawn_echo_websocket().await;
        let (proxy, recorded) =
            spawn_connect_proxy("HTTP/1.1 407 Proxy Authentication Required", upstream).await;
        let error =
            connect_websocket_with("wss://relay.example.com/tunnel/control", &proxy_env(proxy))
                .await
                .unwrap_err();
        let tungstenite::Error::Io(io) = &error else {
            panic!("expected an io error, got {error:?}");
        };
        let proxy_error = io
            .get_ref()
            .and_then(|e| e.downcast_ref::<RelayProxyError>());
        assert!(
            matches!(proxy_error, Some(RelayProxyError::Refused { status: 407, target, .. }) if target == "relay.example.com:443"),
            "{error:?}"
        );
        let message = error.to_string();
        assert!(message.contains(&format!("http://{proxy}")), "{message}");
        assert!(
            message.contains("407") && !message.contains("relay-pass"),
            "{message}"
        );
        recorded.await.unwrap();
    }

    #[tokio::test]
    async fn an_unreachable_proxy_is_an_error_naming_the_proxy() {
        let closed = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = closed.local_addr().unwrap();
        drop(closed);
        let error = dial_with(true, "relay.example.com", 443, &proxy_env(address))
            .await
            .unwrap_err();
        assert!(
            matches!(
                error
                    .get_ref()
                    .and_then(|e| e.downcast_ref::<RelayProxyError>()),
                Some(RelayProxyError::Unreachable { .. })
            ),
            "{error:?}"
        );
        let message = error.to_string();
        assert!(
            message.contains(&format!(
                "relay proxy http://{address} (from FERRYX_RELAY_PROXY) is unreachable"
            )),
            "{message}"
        );
    }

    #[tokio::test]
    async fn a_silent_proxy_times_out_and_a_garbled_one_is_a_bad_response() {
        let silent = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = silent.local_addr().unwrap();
        let held = tokio::spawn(async move {
            // Keep the connection open without answering until the client gives up.
            let (mut stream, _) = silent.accept().await.unwrap();
            let mut sink = Vec::new();
            let _ = stream.read_to_end(&mut sink).await;
        });
        let proxy = parse_proxy("HTTPS_PROXY", &format!("http://{address}")).unwrap();
        let error = tunnel(&proxy, "relay.example.com", 443, Duration::from_millis(200))
            .await
            .unwrap_err();
        assert!(
            matches!(error, RelayProxyError::Timeout { .. }),
            "{error:?}"
        );
        assert!(error.to_string().contains(&format!("http://{address}")));
        held.await.unwrap();

        let upstream = spawn_echo_websocket().await;
        let (garbled, recorded) = spawn_connect_proxy("SSH-2.0-OpenSSH", upstream).await;
        let proxy = parse_proxy("HTTPS_PROXY", &format!("http://{garbled}")).unwrap();
        let error = tunnel(&proxy, "relay.example.com", 443, LIMIT)
            .await
            .unwrap_err();
        assert!(
            matches!(error, RelayProxyError::BadResponse { .. }),
            "{error:?}"
        );
        recorded.await.unwrap();
    }

    #[tokio::test]
    async fn a_loopback_relay_is_dialled_directly_even_with_a_proxy_configured() {
        let upstream = spawn_echo_websocket().await;
        let unused = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy = unused.local_addr().unwrap();
        drop(unused);
        let (mut socket, _) = tokio::time::timeout(
            LIMIT,
            connect_websocket_with(format!("ws://{upstream}/tunnel/control"), &proxy_env(proxy)),
        )
        .await
        .unwrap()
        .expect("direct loopback connection despite a dead proxy");
        socket.send(Message::Text("direct".into())).await.unwrap();
        let echoed = tokio::time::timeout(LIMIT, socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(echoed, Message::Text("direct".into()));
    }
}
