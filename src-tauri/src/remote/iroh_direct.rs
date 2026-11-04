//! Todo 26 spike: direct remote attach over iroh with only the self-hosted relay.
//!
//! Compiled only with the `iroh-direct` cargo feature, which is off by default and never part
//! of default, headless or release builds. The gates this spike is measured against are frozen
//! in `docs/research/iroh-spike-gates.md`.
//!
//! The endpoint built here talks to exactly one relay, the Ferryx one: no n0 relays, no pkarr
//! publishing, no DNS address lookup and no public fallback nameservers. Peers are dialed by
//! endpoint id plus the self-hosted relay URL (and optionally a known direct address); iroh
//! upgrades to a direct UDP path when hole punching succeeds and otherwise stays on the relay.
//! The bytes on the stream are the existing attach handshake (`attach_crypto`), so the iroh
//! TLS layer is transport only and the same device authorization applies as on every other
//! attach path.

use std::time::{Duration, Instant};

use iroh::dns::{DnsResolver, FallbackMode};
use iroh::endpoint::{presets, Builder, Connection, RecvStream, SendStream};
use iroh::{Endpoint, EndpointAddr, EndpointId, RelayConfig, RelayMode, RelayUrl};
use tokio::io::{AsyncRead, AsyncWrite};

use super::attach_crypto::{AttachError, SecureStream};

/// The only relay the product build would use. The spike points at an isolated test hostname
/// instead (see the gates doc); changing the production relay needs owner approval.
pub const FERRYX_IROH_RELAY_URL: &str = "https://relay.ferryx.dev/relay";

/// ALPN of the direct attach stream.
pub const IROH_ATTACH_ALPN: &[u8] = b"ferryx/attach-direct/1";

/// Host suffixes of number 0's public infrastructure (relays, pkarr and DNS). The spike must
/// never configure any of them.
const N0_HOST_SUFFIXES: &[&str] = &["iroh.link", "iroh.network", "n0.computer"];

/// Parses `relay_url` and refuses anything that is not a self-hosted http(s) relay.
pub fn self_hosted_relay_url(relay_url: &str) -> Result<RelayUrl, String> {
    let url = relay_url
        .parse::<RelayUrl>()
        .map_err(|error| format!("invalid relay URL {relay_url:?}: {error}"))?;
    if !matches!(url.scheme(), "https" | "http") {
        return Err(format!("relay URL {relay_url:?} must be http(s)"));
    }
    let host = url
        .host_str()
        .ok_or_else(|| format!("relay URL {relay_url:?} has no host"))?
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if N0_HOST_SUFFIXES
        .iter()
        .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")))
    {
        return Err(format!(
            "relay URL {relay_url:?} is n0 infrastructure; only a self-hosted relay is allowed"
        ));
    }
    Ok(url)
}

/// The relay mode of every spike endpoint: a custom map holding only `relay_url`.
pub fn self_hosted_relay_mode(relay_url: &str) -> Result<RelayMode, String> {
    Ok(RelayMode::custom([self_hosted_relay_url(relay_url)?]))
}

/// The endpoint builder: `presets::Minimal` (crypto provider only), the self-hosted relay,
/// no address lookup, and a resolver that never falls back to public nameservers.
pub fn endpoint_builder(relay_url: &str) -> Result<Builder, String> {
    let dns = DnsResolver::builder()
        .with_system_defaults()
        .with_fallback_mode(FallbackMode::Never)
        .build();
    Ok(Endpoint::builder(presets::Minimal)
        .relay_mode(self_hosted_relay_mode(relay_url)?)
        .clear_address_lookup()
        .dns_resolver(dns)
        .alpns(vec![IROH_ATTACH_ALPN.to_vec()]))
}

/// Binds a spike endpoint; `bind_addr` restricts it to one local socket (tests use loopback).
pub async fn bind_endpoint(relay_url: &str, bind_addr: Option<&str>) -> Result<Endpoint, String> {
    let mut builder = endpoint_builder(relay_url)?;
    if let Some(addr) = bind_addr {
        builder = builder
            .clear_ip_transports()
            .bind_addr(addr)
            .map_err(|error| format!("invalid bind address {addr:?}: {error}"))?;
    }
    builder
        .bind()
        .await
        .map_err(|error| format!("iroh endpoint bind failed: {error}"))
}

/// Runtime proof for gate (d): the bound endpoint has zero address lookup services, holds the
/// self-hosted relay, and holds none of n0's production or staging relays.
pub async fn assert_self_hosted_only(endpoint: &Endpoint, relay_url: &str) -> Result<(), String> {
    let lookups = endpoint
        .address_lookup()
        .map_err(|error| format!("endpoint closed: {error}"))?;
    if !lookups.is_empty() {
        return Err(format!(
            "endpoint has {} address lookup services, expected 0",
            lookups.len()
        ));
    }
    let own = self_hosted_relay_url(relay_url)?;
    // Re-inserting the same config is a no-op that reports whether the relay was present.
    if endpoint
        .insert_relay(own.clone(), std::sync::Arc::new(RelayConfig::from(own.clone())))
        .await
        .is_none()
    {
        return Err(format!("endpoint does not hold the self-hosted relay {own}"));
    }
    let n0_relays: Vec<RelayUrl> = iroh::defaults::prod::default_relay_map()
        .urls::<Vec<_>>()
        .into_iter()
        .chain(iroh::defaults::staging::default_relay_map().urls::<Vec<_>>())
        .collect();
    for n0 in n0_relays {
        // Removing a relay the map does not hold changes nothing and answers `None`.
        if endpoint.remove_relay(&n0).await.is_some() {
            return Err(format!("endpoint held the n0 relay {n0}"));
        }
    }
    Ok(())
}

/// The address a dialer needs: the peer's id, the self-hosted relay, and any known direct
/// addresses. No lookup service is consulted.
pub fn peer_addr(
    peer: EndpointId,
    relay_url: &str,
    direct: impl IntoIterator<Item = std::net::SocketAddr>,
) -> Result<EndpointAddr, String> {
    let mut addr = EndpointAddr::new(peer).with_relay_url(self_hosted_relay_url(relay_url)?);
    for socket in direct {
        addr = addr.with_ip_addr(socket);
    }
    Ok(addr)
}

/// One bidirectional QUIC stream as the byte stream the attach handshake runs over.
pub type IrohStream = tokio::io::Join<RecvStream, SendStream>;

/// Dials `peer` and opens the attach stream.
pub async fn dial(endpoint: &Endpoint, peer: EndpointAddr) -> Result<(Connection, IrohStream), String> {
    let connection = endpoint
        .connect(peer, IROH_ATTACH_ALPN)
        .await
        .map_err(|error| format!("iroh connect failed: {error}"))?;
    let (send, recv) = connection
        .open_bi()
        .await
        .map_err(|error| format!("iroh open_bi failed: {error}"))?;
    Ok((connection, tokio::io::join(recv, send)))
}

/// Accepts the next connection and its attach stream; `None` once the endpoint is closed.
pub async fn accept(endpoint: &Endpoint) -> Option<Result<(Connection, IrohStream), String>> {
    let incoming = endpoint.accept().await?;
    Some(
        async {
            let connection = incoming
                .await
                .map_err(|error| format!("iroh accept failed: {error}"))?;
            let (send, recv) = connection
                .accept_bi()
                .await
                .map_err(|error| format!("iroh accept_bi failed: {error}"))?;
            Ok((connection, tokio::io::join(recv, send)))
        }
        .await,
    )
}

/// Whether the connection currently sends over a direct IP path (`false` = via the relay).
pub fn uses_direct_path(connection: &Connection) -> bool {
    connection
        .paths()
        .iter()
        .any(|path| path.is_selected() && path.is_ip())
}

/// Echo side of the keystroke round-trip measurement: every decrypted frame goes straight
/// back. Returns when the peer finishes the session.
pub async fn serve_echo<S>(mut secure: SecureStream<S>) -> Result<u64, AttachError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut echoed = 0u64;
    loop {
        let frame = match secure.recv_frame().await {
            Ok(frame) => frame,
            Err(AttachError::StreamClosed) => return Ok(echoed),
            Err(error) => return Err(error),
        };
        secure.send_frame(&frame).await?;
        echoed += 1;
    }
}

/// Sends `count` one-byte keystroke frames one at a time and records each echo round trip.
pub async fn measure_echo_rtt<S>(
    secure: &mut SecureStream<S>,
    count: usize,
) -> Result<Vec<Duration>, AttachError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut samples = Vec::with_capacity(count);
    for index in 0..count {
        let keystroke = [b'a' + (index % 26) as u8];
        let started = Instant::now();
        secure.send_frame(&keystroke).await?;
        let echo = secure.recv_frame().await?;
        samples.push(started.elapsed());
        if echo != keystroke {
            return Err(AttachError::DecryptFailed(format!(
                "echo {index} returned {echo:?}, sent {keystroke:?}"
            )));
        }
    }
    Ok(samples)
}

/// Nearest-rank percentile in milliseconds; `None` for no samples or `p` outside 0..=100.
pub fn percentile_ms(samples: &[Duration], p: f64) -> Option<f64> {
    if samples.is_empty() || !(0.0..=100.0).contains(&p) {
        return None;
    }
    let mut sorted: Vec<Duration> = samples.to_vec();
    sorted.sort_unstable();
    let rank = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
    let index = rank.clamp(1, sorted.len()) - 1;
    Some(sorted[index].as_secs_f64() * 1000.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::attach_crypto::AttachInitiator;
    use crate::remote::attach_identity::AttachIdentity;
    use crate::remote::session_transport::{establish_session, SessionTransport};
    use base64::{engine::general_purpose::STANDARD, Engine as _};

    // Nothing listens here, so tests never reach a relay; direct loopback paths carry them.
    const UNREACHABLE_RELAY: &str = "http://127.0.0.1:9/relay";

    fn attach_identity() -> AttachIdentity {
        let secret = x25519_dalek::StaticSecret::random_from_rng(rand::rngs::OsRng);
        AttachIdentity {
            public_key: STANDARD.encode(x25519_dalek::PublicKey::from(&secret).as_bytes()),
            private_key: STANDARD.encode(secret.to_bytes()),
        }
    }

    #[test]
    fn the_relay_mode_holds_only_the_self_hosted_relay() {
        let mode = self_hosted_relay_mode(FERRYX_IROH_RELAY_URL).expect("relay mode");
        let urls: Vec<String> = mode
            .relay_map()
            .urls::<Vec<_>>()
            .into_iter()
            .map(|url| url.to_string())
            .collect();
        assert_eq!(urls, vec![FERRYX_IROH_RELAY_URL.to_string()]);
    }

    #[test]
    fn n0_and_malformed_relay_urls_are_refused() {
        for bad in [
            "https://use1-1.relay.n0.iroh.link./",
            "https://euc1-1.staging-relay.n0.iroh.link",
            "https://dns.iroh.link/pkarr",
            "https://x.iroh.network",
            "https://n0.computer",
            "ftp://relay.ferryx.dev/relay",
            "relay.ferryx.dev",
            "",
            "https://",
        ] {
            assert!(self_hosted_relay_url(bad).is_err(), "{bad:?} must be refused");
        }
        assert!(self_hosted_relay_url("https://iroh-spike.example.test/relay").is_ok());
    }

    #[test]
    fn percentiles_use_nearest_rank_and_reject_bad_input() {
        let samples: Vec<Duration> = (1..=100).map(Duration::from_millis).collect();
        assert_eq!(percentile_ms(&samples, 95.0), Some(95.0));
        assert_eq!(percentile_ms(&samples, 50.0), Some(50.0));
        assert_eq!(percentile_ms(&samples, 100.0), Some(100.0));
        assert_eq!(percentile_ms(&samples, 0.0), Some(1.0));
        assert_eq!(percentile_ms(&[], 95.0), None);
        assert_eq!(percentile_ms(&samples, 101.0), None);
        assert_eq!(percentile_ms(&samples, f64::NAN), None);
    }

    #[tokio::test]
    async fn a_bound_endpoint_has_no_address_lookup_and_only_the_self_hosted_relay() {
        let endpoint = bind_endpoint(UNREACHABLE_RELAY, Some("127.0.0.1:0"))
            .await
            .expect("bind");
        assert_self_hosted_only(&endpoint, UNREACHABLE_RELAY)
            .await
            .expect("self-hosted only");
        endpoint.close().await;
    }

    #[tokio::test]
    async fn an_attach_over_iroh_authorizes_the_device_and_echoes_keystrokes() {
        let machine = attach_identity();
        let device_secret = x25519_dalek::StaticSecret::random_from_rng(rand::rngs::OsRng);
        let device_public = x25519_dalek::PublicKey::from(&device_secret).to_bytes();

        let server = bind_endpoint(UNREACHABLE_RELAY, Some("127.0.0.1:0"))
            .await
            .expect("server bind");
        let client = bind_endpoint(UNREACHABLE_RELAY, Some("127.0.0.1:0"))
            .await
            .expect("client bind");
        let target = peer_addr(server.id(), UNREACHABLE_RELAY, server.bound_sockets())
            .expect("peer addr");

        let serve = tokio::spawn({
            let server = server.clone();
            let machine = machine.clone();
            async move {
                let (_connection, stream) = accept(&server).await.expect("open")?;
                let transport = establish_session(
                    stream,
                    true,
                    Some(&machine),
                    "machine-1",
                    "iroh",
                    "1",
                    move |key: &[u8; 32]| *key == device_public,
                )
                .await
                .map_err(|error| error.to_string())?;
                let SessionTransport::Attached(secure) = transport else {
                    return Err("expected an attached transport".to_string());
                };
                serve_echo(secure).await.map_err(|error| error.to_string())
            }
        });

        let run = async {
            let (connection, stream) = dial(&client, target).await.expect("dial");
            let initiator = AttachInitiator::new(
                &device_secret.to_bytes(),
                &machine.public_key,
                "machine-1",
                "iroh",
                "1",
            )
            .expect("initiator");
            let (mut secure, _) = initiator.connect(stream).await.expect("handshake");
            let samples = measure_echo_rtt(&mut secure, 50).await.expect("echo");
            assert!(uses_direct_path(&connection), "loopback must use the direct path");
            drop(secure);
            connection.close(0u32.into(), b"done");
            samples
        };
        let samples = tokio::time::timeout(Duration::from_secs(20), run)
            .await
            .expect("attach over iroh timed out");
        assert_eq!(samples.len(), 50);
        assert!(percentile_ms(&samples, 95.0).is_some());

        let echoed = tokio::time::timeout(Duration::from_secs(10), serve)
            .await
            .expect("echo side timed out")
            .expect("join")
            .expect("echo side");
        assert_eq!(echoed, 50);
        client.close().await;
        server.close().await;
    }

    #[tokio::test]
    async fn an_attach_over_iroh_refuses_an_unknown_device() {
        let machine = attach_identity();
        let stranger = x25519_dalek::StaticSecret::random_from_rng(rand::rngs::OsRng);

        let server = bind_endpoint(UNREACHABLE_RELAY, Some("127.0.0.1:0"))
            .await
            .expect("server bind");
        let client = bind_endpoint(UNREACHABLE_RELAY, Some("127.0.0.1:0"))
            .await
            .expect("client bind");
        let target = peer_addr(server.id(), UNREACHABLE_RELAY, server.bound_sockets())
            .expect("peer addr");

        let serve = tokio::spawn({
            let server = server.clone();
            let machine = machine.clone();
            async move {
                let (_connection, stream) = accept(&server).await.expect("open")?;
                establish_session(stream, true, Some(&machine), "machine-1", "iroh", "1", |_| false)
                    .await
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            }
        });
        let outcome = tokio::time::timeout(Duration::from_secs(20), async {
            let (_connection, stream) = dial(&client, target).await.expect("dial");
            AttachInitiator::new(&stranger.to_bytes(), &machine.public_key, "machine-1", "iroh", "1")
                .expect("initiator")
                .connect(stream)
                .await
                .map(|_| ())
        })
        .await
        .expect("dial timed out");
        let responder = tokio::time::timeout(Duration::from_secs(10), serve)
            .await
            .expect("responder timed out")
            .expect("join");
        assert!(
            outcome.is_err() || responder.is_err(),
            "an unknown device must not attach over iroh"
        );
        client.close().await;
        server.close().await;
    }
}
