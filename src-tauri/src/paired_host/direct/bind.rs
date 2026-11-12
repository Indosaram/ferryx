//! Local UDP port selection for direct (P2P) negotiation.
//!
//! Direct negotiation needs a UDP socket whose *public* endpoint (as STUN reports it) is
//! reachable by the peer. A socket bound to an ephemeral port (`0.0.0.0:0`) can only be
//! reached through NAT hole punching, which fails on symmetric NAT and gives an operator
//! no port to forward. Binding a configured fixed port makes the endpoint stable and
//! therefore forwardable.
//!
//! A fixed port cannot serve every concurrent session, so the configured port is a
//! *preference*: it is claimed when free, and the caller falls back to an ephemeral bind
//! when it is already in use. That keeps `max_sessions` concurrency intact while making
//! the first/steady-state session forwardable.

use tokio::net::UdpSocket;

pub const DIRECT_LOCAL_PORT_ENV: &str = "FERRYX_DIRECT_LOCAL_PORT";

/// Reads the preferred local port from the environment.
///
/// Returns `None` when unset, empty, unparsable, or `0` - all of which mean "ephemeral".
/// A malformed value is treated as unset rather than an error: a typo in an optional
/// tuning knob must not make direct connectivity fail outright.
pub fn preferred_local_port() -> Option<u16> {
    parse_preferred_local_port(std::env::var(DIRECT_LOCAL_PORT_ENV).ok().as_deref())
}

/// Pure parser behind [`preferred_local_port`], so tests need not mutate the environment.
pub fn parse_preferred_local_port(raw: Option<&str>) -> Option<u16> {
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    match raw.parse::<u16>() {
        Ok(port) if port != 0 => Some(port),
        _ => None,
    }
}

/// Binds the direct-negotiation socket, preferring `preferred` but never failing because
/// of it.
///
/// When a preferred port is given, the IPv4 any-address bind is tried first. Any failure
/// (port busy, or a platform that refuses the bind) falls back to an ephemeral port, so
/// the returned socket is always usable and the caller never has to distinguish the two
/// cases. Callers that care which happened can compare the bound port against the
/// preferred one.
pub async fn bind_direct_socket(preferred: Option<u16>) -> std::io::Result<UdpSocket> {
    if let Some(port) = preferred {
        match UdpSocket::bind(("0.0.0.0", port)).await {
            Ok(socket) => return Ok(socket),
            Err(error) => {
                tracing::debug!(
                    port,
                    %error,
                    "direct: preferred local UDP port unavailable, using ephemeral port"
                );
            }
        }
    }
    UdpSocket::bind("0.0.0.0:0").await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_empty_zero_and_garbage_all_mean_ephemeral() {
        for raw in [None, Some(""), Some("   "), Some("0"), Some("abc"), Some("-1"), Some("70000")] {
            assert_eq!(parse_preferred_local_port(raw), None, "raw={raw:?}");
        }
    }

    #[test]
    fn valid_port_is_parsed() {
        assert_eq!(parse_preferred_local_port(Some("41641")), Some(41641));
        assert_eq!(parse_preferred_local_port(Some(" 41641 ")), Some(41641));
        assert_eq!(parse_preferred_local_port(Some("65535")), Some(65535));
        assert_eq!(parse_preferred_local_port(Some("1")), Some(1));
    }

    #[tokio::test]
    async fn binds_the_preferred_port_when_it_is_free() {
        let probe = UdpSocket::bind("0.0.0.0:0").await.expect("probe bind");
        let port = probe.local_addr().expect("probe addr").port();
        drop(probe);

        let socket = bind_direct_socket(Some(port)).await.expect("bind preferred");
        assert_eq!(socket.local_addr().expect("addr").port(), port);
    }

    #[tokio::test]
    async fn falls_back_to_an_ephemeral_port_when_the_preferred_one_is_taken() {
        let holder = UdpSocket::bind("0.0.0.0:0").await.expect("holder bind");
        let taken = holder.local_addr().expect("holder addr").port();

        let socket = bind_direct_socket(Some(taken))
            .await
            .expect("fallback bind must still succeed");
        let bound = socket.local_addr().expect("addr").port();
        assert_ne!(bound, taken, "a taken preferred port must not be reused");
        assert_ne!(bound, 0, "fallback must be a real bound port");
    }

    #[tokio::test]
    async fn no_preference_binds_an_ephemeral_port() {
        let socket = bind_direct_socket(None).await.expect("bind");
        assert_ne!(socket.local_addr().expect("addr").port(), 0);
    }
}
