pub mod tracker;
pub use tracker::DiscoveryTracker;

pub mod mac;
pub use mac::{normalize_mac, primary_mac};

pub mod endpoint;
pub use endpoint::{
    choose_and_format_address, choose_preferred_endpoint, choose_preferred_ip_with_scope,
    decide_service_state_action, is_usable_endpoint, is_usable_ipv6_with_scope,
    validate_resolved_service, DiscoveredEndpoint, ServiceStateAction,
};

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub mod apple;

#[cfg(all(feature = "mdns", any(target_os = "linux", target_os = "windows")))]
pub mod mdns;

#[cfg(not(any(
    target_os = "macos",
    target_os = "ios",
    all(feature = "mdns", any(target_os = "linux", target_os = "windows"))
)))]
pub mod stub;

use serde::{Deserialize, Serialize};
use std::net::{IpAddr, SocketAddr};
use thiserror::Error;

pub fn is_usable_ipv4(ip: &IpAddr) -> bool {
    if let IpAddr::V4(v4) = ip {
        !v4.is_unspecified() && !v4.is_loopback() && !v4.is_multicast() && !v4.is_broadcast()
    } else {
        false
    }
}

pub fn is_unscoped_ipv6_link_local(v6: &std::net::Ipv6Addr) -> bool {
    (v6.segments()[0] & 0xffc0) == 0xfe80
}

pub fn is_usable_ipv6(ip: &IpAddr) -> bool {
    if let IpAddr::V6(v6) = ip {
        !v6.is_unspecified()
            && !v6.is_loopback()
            && !v6.is_multicast()
            && !is_unscoped_ipv6_link_local(v6)
    } else {
        false
    }
}

pub fn is_usable_address(ip: &IpAddr) -> bool {
    is_usable_ipv4(ip) || is_usable_ipv6(ip)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveredHost {
    pub id: String,
    pub name: String,
    pub ip: String,
    pub os: String,
    pub tcp_port: u16,
    pub udp_port: u16,
    /// Primary NIC MAC advertised in the `mac` TXT entry, canonical lowercase.
    #[serde(default)]
    pub mac: Option<String>,
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum DiscoveryError {
    #[error("local network access denied by system policy")]
    PolicyDenied,
    #[error("discovery backend error: {0}")]
    Backend(String),
    #[error("discovery service unavailable")]
    Unavailable,
    #[error("invalid discovery payload: {0}")]
    InvalidPayload(String),
}

pub fn parse_service_metadata(
    fullname: &str,
    host_target: &str,
    srv_port: u16,
    txt: &[(String, Vec<u8>)],
    addresses: &[IpAddr],
) -> Result<DiscoveredHost, DiscoveryError> {
    if addresses.is_empty() {
        return Err(DiscoveryError::InvalidPayload("no addresses found".into()));
    }
    let addr_tuples: Vec<(IpAddr, Option<u32>)> = addresses.iter().map(|&ip| (ip, None)).collect();
    let endpoint = choose_preferred_endpoint(&addr_tuples)
        .ok_or_else(|| DiscoveryError::InvalidPayload("no usable unicast address found".into()))?;
    validate_resolved_service(fullname, host_target, srv_port, txt, &endpoint)
}

pub fn parse_service_metadata_scoped(
    fullname: &str,
    host_target: &str,
    srv_port: u16,
    txt: &[(String, Vec<u8>)],
    addresses: &[(IpAddr, Option<u32>)],
) -> Result<DiscoveredHost, DiscoveryError> {
    if addresses.is_empty() {
        return Err(DiscoveryError::InvalidPayload("no addresses found".into()));
    }
    let endpoint = choose_preferred_endpoint(addresses)
        .ok_or_else(|| DiscoveryError::InvalidPayload("no usable unicast address found".into()))?;
    validate_resolved_service(fullname, host_target, srv_port, txt, &endpoint)
}

/// Pairs resolved addresses with the scope ids they may need. Backends such as `mdns-sd` report
/// addresses without a scope id, and an IPv6 link-local address without a positive scope is
/// rejected as unusable, so every link-local address is paired with each candidate local interface
/// index. All other addresses are passed through unscoped.
pub fn attach_link_local_scopes(
    addresses: &[IpAddr],
    link_local_interfaces: &[u32],
) -> Vec<(IpAddr, Option<u32>)> {
    let mut scoped = Vec::with_capacity(addresses.len());
    for &ip in addresses {
        let is_link_local = matches!(ip, IpAddr::V6(v6) if is_unscoped_ipv6_link_local(&v6));
        if !is_link_local || link_local_interfaces.is_empty() {
            scoped.push((ip, None));
            continue;
        }
        for &if_index in link_local_interfaces {
            scoped.push((ip, Some(if_index)));
        }
    }
    scoped
}

/// Resolves `host:port` like `ToSocketAddrs`, but rejects an IPv6 zone that names no interface.
///
/// libc resolvers disagree on `fe80::1%nope`: Linux fails, while macOS silently drops the unknown
/// zone and yields a scope-0 link-local address that can never be routed. A numeric zone and a
/// zone-less host are passed through unchanged.
pub fn resolve_socket_addrs(host: &str, port: u16) -> std::io::Result<Vec<SocketAddr>> {
    use std::net::ToSocketAddrs;
    if let Some((address, zone)) = host.split_once('%') {
        if address.contains(':') && zone.parse::<u32>().is_err() && !interface_exists(zone) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("unknown IPv6 scope interface '{zone}'"),
            ));
        }
    }
    Ok((host, port).to_socket_addrs()?.collect())
}

#[cfg(unix)]
fn interface_exists(name: &str) -> bool {
    std::ffi::CString::new(name)
        // SAFETY: `name` is an owned NUL-terminated string that outlives the call, and
        // `if_nametoindex` only reads it.
        .is_ok_and(|name| unsafe { libc::if_nametoindex(name.as_ptr()) } != 0)
}

/// Windows resolves named zones itself; without a portable lookup, defer to the resolver.
#[cfg(not(unix))]
fn interface_exists(_name: &str) -> bool {
    true
}

pub struct LanDiscovery {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    inner: apple::AppleDnsServiceBrowser,
    #[cfg(all(feature = "mdns", any(target_os = "linux", target_os = "windows")))]
    inner: mdns::MdnsSdBrowser,
    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        all(feature = "mdns", any(target_os = "linux", target_os = "windows"))
    )))]
    inner: stub::StubBrowser,
}

impl LanDiscovery {
    pub fn new() -> Result<Self, DiscoveryError> {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            let inner = apple::AppleDnsServiceBrowser::new()?;
            Ok(Self { inner })
        }
        #[cfg(all(feature = "mdns", any(target_os = "linux", target_os = "windows")))]
        {
            let inner = mdns::MdnsSdBrowser::new()?;
            Ok(Self { inner })
        }
        #[cfg(not(any(
            target_os = "macos",
            target_os = "ios",
            all(feature = "mdns", any(target_os = "linux", target_os = "windows"))
        )))]
        {
            let inner = stub::StubBrowser::new()?;
            Ok(Self { inner })
        }
    }

    pub fn snapshot(&self) -> Result<Vec<DiscoveredHost>, DiscoveryError> {
        self.inner.snapshot()
    }
}

pub struct ServiceAdvertiser {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    _inner: apple::AppleDnsServiceAdvertiser,
    #[cfg(all(feature = "mdns", any(target_os = "linux", target_os = "windows")))]
    _inner: mdns::MdnsSdAdvertiser,
    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        all(feature = "mdns", any(target_os = "linux", target_os = "windows"))
    )))]
    _inner: stub::StubAdvertiser,
}

impl ServiceAdvertiser {
    pub fn start(
        name: &str,
        tcp_port: u16,
        udp_port: u16,
        bind_addr: SocketAddr,
    ) -> Result<Self, DiscoveryError> {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            let _inner =
                apple::AppleDnsServiceAdvertiser::start(name, tcp_port, udp_port, bind_addr)?;
            Ok(Self { _inner })
        }
        #[cfg(all(feature = "mdns", any(target_os = "linux", target_os = "windows")))]
        {
            let _inner = mdns::MdnsSdAdvertiser::start(name, tcp_port, udp_port, bind_addr)?;
            Ok(Self { _inner })
        }
        #[cfg(not(any(
            target_os = "macos",
            target_os = "ios",
            all(feature = "mdns", any(target_os = "linux", target_os = "windows"))
        )))]
        {
            let _inner = stub::StubAdvertiser::start(name, tcp_port, udp_port, bind_addr)?;
            Ok(Self { _inner })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn attach_link_local_scopes_makes_link_local_only_hosts_usable() {
        let link_local = IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1));

        // Without a scope the only advertised address is rejected as unusable.
        assert!(choose_preferred_endpoint(&[(link_local, None)]).is_none());

        let scoped = attach_link_local_scopes(&[link_local], &[5, 9]);
        assert_eq!(scoped, vec![(link_local, Some(5)), (link_local, Some(9))]);
        let endpoint = choose_preferred_endpoint(&scoped)
            .expect("scoped link-local address must be selectable");
        assert_eq!(endpoint.formatted, "fe80::1%5");
    }

    #[test]
    fn attach_link_local_scopes_leaves_other_addresses_unscoped() {
        let v4 = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));
        let global_v6 = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1));
        let link_local = IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 2));

        assert_eq!(
            attach_link_local_scopes(&[v4, global_v6], &[5]),
            vec![(v4, None), (global_v6, None)]
        );
        // With no usable local interface the address is passed through rather than dropped.
        assert_eq!(
            attach_link_local_scopes(&[link_local], &[]),
            vec![(link_local, None)]
        );
    }

    #[test]
    fn parse_service_metadata_scoped_publishes_link_local_only_host() {
        let txt = vec![
            ("protocol".to_string(), b"3".to_vec()),
            ("name".to_string(), b"desk".to_vec()),
            ("os".to_string(), b"linux".to_vec()),
            ("udp_port".to_string(), b"19731".to_vec()),
        ];
        let addrs = [IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 3))];

        assert!(parse_service_metadata(
            "desk._maho-rd._tcp.local.",
            "desk.local.",
            19730,
            &txt,
            &addrs
        )
        .is_err());

        let host = parse_service_metadata_scoped(
            "desk._maho-rd._tcp.local.",
            "desk.local.",
            19730,
            &txt,
            &attach_link_local_scopes(&addrs, &[7]),
        )
        .expect("scoped link-local host must resolve");
        assert_eq!(host.ip, "fe80::3%7");
    }
}
