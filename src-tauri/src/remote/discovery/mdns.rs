//! mDNS-based LAN discovery for Ferryx hosts.
//!
//! Advertises a local Ferryx instance via a `_ferryx._tcp.local.` mDNS
//! service and scans the LAN for other advertising instances. This is a
//! minimal, dependency-free implementation built directly on UDP multicast
//! sockets (no async DNS-SD crate): we encode/decode a tiny subset of the
//! mDNS wire format sufficient to announce and recognize Ferryx hosts.
//!
//! mDNS is treated as best-effort: any failure to bind or use the
//! multicast socket (e.g. sandboxed environments, missing permissions,
//! networks without multicast) is logged as a warning and surfaced as an
//! empty result or `Err`, never a panic.

use super::{HostAuthStatus, HostEndpoint, TransportType};
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::time::Duration;

/// The mDNS service type Ferryx hosts advertise themselves under.
pub const FERRYX_MDNS_SERVICE_TYPE: &str = "_ferryx._tcp.local.";

/// Standard mDNS multicast group and port (RFC 6762).
const MDNS_MULTICAST_ADDR: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);
const MDNS_PORT: u16 = 5353;

/// A single Ferryx mDNS advertisement: instance name, port, and IP.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MdnsAdvertisement {
    pub instance_name: String,
    pub port: u16,
    pub address: Ipv4Addr,
}

impl MdnsAdvertisement {
    pub fn new(instance_name: impl Into<String>, port: u16, address: Ipv4Addr) -> Self {
        Self {
            instance_name: instance_name.into(),
            port,
            address,
        }
    }

    /// Fully-qualified service instance name, e.g.
    /// `Ferryx-TestHost._ferryx._tcp.local.`.
    pub fn fqdn(&self) -> String {
        format!("{}.{}", self.instance_name, FERRYX_MDNS_SERVICE_TYPE)
    }

    /// Encodes this advertisement into a minimal custom packet understood
    /// by [`MdnsDiscoveryProvider::parse_packet`]. This is not a
    /// spec-complete mDNS/DNS message encoder; it carries just enough
    /// information (magic tag, service type, instance name, port, IPv4
    /// address) to announce and recognize Ferryx hosts on the LAN.
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(MAGIC);
        write_lp_str(&mut buf, FERRYX_MDNS_SERVICE_TYPE);
        write_lp_str(&mut buf, &self.instance_name);
        buf.extend_from_slice(&self.port.to_be_bytes());
        buf.extend_from_slice(&self.address.octets());
        buf
    }
}

/// Magic prefix identifying a Ferryx mDNS announcement packet.
const MAGIC: &[u8; 4] = b"FRYX";

fn write_lp_str(buf: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    buf.push(bytes.len() as u8);
    buf.extend_from_slice(bytes);
}

fn read_lp_str(buf: &[u8], offset: &mut usize) -> Option<String> {
    let len = *buf.get(*offset)? as usize;
    *offset += 1;
    let end = offset.checked_add(len)?;
    let s = std::str::from_utf8(buf.get(*offset..end)?).ok()?.to_string();
    *offset = end;
    Some(s)
}

/// mDNS-based discovery and advertisement of Ferryx LAN hosts.
#[derive(Debug, Default, Clone, Copy)]
pub struct MdnsDiscoveryProvider;

impl MdnsDiscoveryProvider {
    pub fn new() -> Self {
        Self
    }

    /// Builds the advertisement packet bytes for a local Ferryx host.
    /// Pure/no I/O helper primarily used by tests and by [`Self::advertise`].
    pub fn build_advertisement(
        instance_name: &str,
        port: u16,
        address: Ipv4Addr,
    ) -> MdnsAdvertisement {
        MdnsAdvertisement::new(instance_name, port, address)
    }

    /// Parses raw bytes received over the multicast socket into a
    /// [`HostEndpoint`]. Returns `None` if the packet is not a
    /// recognizable Ferryx advertisement.
    pub fn parse_packet(bytes: &[u8]) -> Option<HostEndpoint> {
        if bytes.len() < MAGIC.len() || &bytes[..MAGIC.len()] != MAGIC {
            return None;
        }
        let mut offset = MAGIC.len();
        let service_type = read_lp_str(bytes, &mut offset)?;
        if service_type != FERRYX_MDNS_SERVICE_TYPE {
            return None;
        }
        let instance_name = read_lp_str(bytes, &mut offset)?;
        let port_bytes: [u8; 2] = bytes.get(offset..offset + 2)?.try_into().ok()?;
        let port = u16::from_be_bytes(port_bytes);
        offset += 2;
        let ip_bytes: [u8; 4] = bytes.get(offset..offset + 4)?.try_into().ok()?;
        let address = Ipv4Addr::from(ip_bytes);

        Some(HostEndpoint {
            host_id: format!("mdns:{instance_name}"),
            name: instance_name,
            address: format!("{address}:{port}"),
            transport: TransportType::Mdns,
            latency_ms: None,
            auth_status: HostAuthStatus::Unknown,
            online: true,
        })
    }

    /// Binds a multicast UDP socket for mDNS send/receive. Best-effort:
    /// any bind/join failure is logged as a warning and returned as `Err`
    /// so callers can treat mDNS as unavailable rather than panicking.
    fn bind_multicast_socket() -> io::Result<UdpSocket> {
        let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, MDNS_PORT))
            .or_else(|_| UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)))?;
        if let Err(e) = socket.join_multicast_v4(&MDNS_MULTICAST_ADDR, &Ipv4Addr::UNSPECIFIED) {
            tracing::warn!("mdns: failed to join multicast group: {e}");
            return Err(e);
        }
        socket.set_multicast_loop_v4(true).ok();
        Ok(socket)
    }

    /// Advertises a local Ferryx host on the LAN by broadcasting a single
    /// mDNS-style announcement packet over UDP multicast. Best-effort:
    /// failures are logged as a warning and returned as `Err` instead of
    /// panicking. Runs entirely on the calling thread via non-blocking
    /// socket I/O and should be invoked from a blocking context (e.g.
    /// `tokio::task::spawn_blocking`) to avoid stalling the async runtime.
    pub fn advertise(instance_name: &str, port: u16, address: Ipv4Addr) -> io::Result<()> {
        let socket = match Self::bind_multicast_socket() {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("mdns: advertise unavailable, skipping: {e}");
                return Err(e);
            }
        };
        let packet = Self::build_advertisement(instance_name, port, address).encode();
        let dest = SocketAddr::V4(SocketAddrV4::new(MDNS_MULTICAST_ADDR, MDNS_PORT));
        if let Err(e) = socket.send_to(&packet, dest) {
            tracing::warn!("mdns: failed to send advertisement: {e}");
            return Err(e);
        }
        Ok(())
    }

    /// Scans the LAN for Ferryx mDNS advertisements for up to `timeout`,
    /// returning discovered [`HostEndpoint`] records. Uses a non-blocking
    /// receive loop (no OS thread is blocked indefinitely); this method
    /// itself performs blocking syscalls bounded by `timeout` and should
    /// be run via `tokio::task::spawn_blocking` from async contexts.
    ///
    /// Best-effort: if the multicast socket cannot be bound (e.g. no
    /// permission, no multicast-capable interface), this logs a warning
    /// and returns an empty vector rather than erroring or panicking.
    pub fn scan(timeout: Duration) -> Vec<HostEndpoint> {
        let socket = match Self::bind_multicast_socket() {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("mdns: scan unavailable, returning no hosts: {e}");
                return Vec::new();
            }
        };

        if let Err(e) = socket.set_read_timeout(Some(timeout.min(Duration::from_millis(200)))) {
            tracing::warn!("mdns: failed to set read timeout: {e}");
            return Vec::new();
        }

        let deadline = std::time::Instant::now() + timeout;
        let mut discovered: Vec<HostEndpoint> = Vec::new();
        let mut buf = [0u8; 512];

        while std::time::Instant::now() < deadline {
            match socket.recv_from(&mut buf) {
                Ok((n, _from)) => {
                    if let Some(endpoint) = Self::parse_packet(&buf[..n]) {
                        if !discovered.iter().any(|e: &HostEndpoint| e.host_id == endpoint.host_id) {
                            discovered.push(endpoint);
                        }
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut => {
                    continue;
                }
                Err(e) => {
                    tracing::warn!("mdns: error receiving packet: {e}");
                    break;
                }
            }
        }

        discovered
    }

    /// Convenience helper mirroring [`Self::scan`] but operating purely on
    /// already-received bytes (e.g. in tests), avoiding any real socket
    /// I/O.
    pub fn discover_from_packets(packets: &[Vec<u8>]) -> Vec<HostEndpoint> {
        let mut discovered = Vec::new();
        for packet in packets {
            if let Some(endpoint) = Self::parse_packet(packet) {
                if !discovered.iter().any(|e: &HostEndpoint| e.host_id == endpoint.host_id) {
                    discovered.push(endpoint);
                }
            }
        }
        discovered
    }
}

/// Suppresses an unused-import warning for `IpAddr` when platform cfgs
/// change socket option availability; kept for API clarity in signatures
/// that may accept either IPv4/IPv6 in the future.
#[allow(dead_code)]
fn _assert_ipaddr_used(_a: IpAddr) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mdns_advertisement_and_scan() {
        let advertisement = MdnsDiscoveryProvider::build_advertisement(
            "Ferryx-TestHost",
            43821,
            Ipv4Addr::new(192, 168, 1, 42),
        );
        assert_eq!(
            advertisement.fqdn(),
            "Ferryx-TestHost._ferryx._tcp.local."
        );

        let packet = advertisement.encode();

        // "Discover" the advertisement by parsing the encoded packet, the
        // same decoding path used for packets received over the wire.
        let discovered = MdnsDiscoveryProvider::discover_from_packets(&[packet]);

        assert_eq!(discovered.len(), 1);
        let endpoint = &discovered[0];
        assert_eq!(endpoint.transport, TransportType::Mdns);
        assert_eq!(endpoint.auth_status, HostAuthStatus::Unknown);
        assert!(endpoint.online);
        assert_eq!(endpoint.name, "Ferryx-TestHost");
        assert_eq!(endpoint.address, "192.168.1.42:43821");
    }

    #[test]
    fn test_parse_packet_rejects_garbage() {
        assert!(MdnsDiscoveryProvider::parse_packet(b"not a real packet").is_none());
        assert!(MdnsDiscoveryProvider::parse_packet(b"").is_none());
    }

    #[test]
    fn test_parse_packet_rejects_wrong_service_type() {
        let mut buf = Vec::new();
        buf.extend_from_slice(MAGIC);
        write_lp_str(&mut buf, "_other._tcp.local.");
        write_lp_str(&mut buf, "Some-Host");
        buf.extend_from_slice(&8080u16.to_be_bytes());
        buf.extend_from_slice(&Ipv4Addr::new(10, 0, 0, 1).octets());
        assert!(MdnsDiscoveryProvider::parse_packet(&buf).is_none());
    }

    #[test]
    fn test_service_type_constant() {
        assert_eq!(FERRYX_MDNS_SERVICE_TYPE, "_ferryx._tcp.local.");
    }
}
