use super::{is_usable_ipv4, is_usable_ipv6, DiscoveredHost, DiscoveryError};
use std::net::IpAddr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredEndpoint {
    pub ip: IpAddr,
    pub scope_id: Option<u32>,
    pub formatted: String,
}

impl DiscoveredEndpoint {
    pub fn new(ip: IpAddr, scope_id: Option<u32>) -> Self {
        let (scope_val, formatted) = match (ip, scope_id) {
            (IpAddr::V6(v6), Some(scope)) if (v6.segments()[0] & 0xffc0) == 0xfe80 && scope > 0 => {
                (Some(scope), format!("{v6}%{scope}"))
            }
            _ => (None, ip.to_string()),
        };
        Self {
            ip,
            scope_id: scope_val,
            formatted,
        }
    }
}

impl From<IpAddr> for DiscoveredEndpoint {
    fn from(ip: IpAddr) -> Self {
        Self::new(ip, None)
    }
}

impl From<(IpAddr, Option<u32>)> for DiscoveredEndpoint {
    fn from((ip, scope_id): (IpAddr, Option<u32>)) -> Self {
        Self::new(ip, scope_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceStateAction {
    Publish(DiscoveredHost),
    Retract,
}

pub fn is_usable_ipv6_with_scope(v6: &std::net::Ipv6Addr, scope_id: Option<u32>) -> bool {
    if v6.is_unspecified() || v6.is_loopback() || v6.is_multicast() {
        false
    } else if (v6.segments()[0] & 0xffc0) == 0xfe80 {
        scope_id.is_some_and(|s| s > 0)
    } else {
        true
    }
}

pub fn is_usable_endpoint(endpoint: &DiscoveredEndpoint) -> bool {
    match endpoint.ip {
        IpAddr::V4(_) => is_usable_ipv4(&endpoint.ip),
        IpAddr::V6(ref v6) => is_usable_ipv6_with_scope(v6, endpoint.scope_id),
    }
}

pub fn choose_preferred_endpoint(
    addresses: &[(IpAddr, Option<u32>)],
) -> Option<DiscoveredEndpoint> {
    // 1. IPv4 priority (usable unicast)
    if let Some(&(IpAddr::V4(v4), _)) = addresses
        .iter()
        .find(|(ip, _)| ip.is_ipv4() && is_usable_ipv4(ip))
    {
        return Some(DiscoveredEndpoint::new(IpAddr::V4(v4), None));
    }

    // 2. Global / ULA IPv6 priority (not link-local)
    if let Some(&(IpAddr::V6(v6), _)) = addresses
        .iter()
        .find(|(ip, _)| ip.is_ipv6() && is_usable_ipv6(ip))
    {
        return Some(DiscoveredEndpoint::new(IpAddr::V6(v6), None));
    }

    // 3. Link-local IPv6 with valid positive scope
    for &(ip, scope) in addresses {
        if let IpAddr::V6(v6) = ip {
            if (v6.segments()[0] & 0xffc0) == 0xfe80 {
                if let Some(scope_id) = scope.filter(|&s| s > 0) {
                    return Some(DiscoveredEndpoint::new(IpAddr::V6(v6), Some(scope_id)));
                }
            }
        }
    }

    None
}

pub fn choose_preferred_ip_with_scope(
    addresses: &[(IpAddr, Option<u32>)],
) -> Option<DiscoveredEndpoint> {
    choose_preferred_endpoint(addresses)
}

pub fn choose_and_format_address(addresses: &[(IpAddr, Option<u32>)]) -> Option<(IpAddr, String)> {
    choose_preferred_endpoint(addresses).map(|ep| (ep.ip, ep.formatted))
}

pub fn validate_resolved_service(
    fullname: &str,
    _host_target: &str,
    srv_port: u16,
    txt: &[(String, Vec<u8>)],
    endpoint: &DiscoveredEndpoint,
) -> Result<DiscoveredHost, DiscoveryError> {
    if fullname.is_empty() || fullname.len() > 255 {
        return Err(DiscoveryError::InvalidPayload(
            "fullname must be between 1 and 255 bytes".into(),
        ));
    }
    if !fullname.contains("._maho-rd._tcp.") {
        return Err(DiscoveryError::InvalidPayload(
            "fullname must contain service type ._maho-rd._tcp.".into(),
        ));
    }
    if srv_port == 0 {
        return Err(DiscoveryError::InvalidPayload(
            "SRV port must be nonzero".into(),
        ));
    }

    if !is_usable_endpoint(endpoint) {
        return Err(DiscoveryError::InvalidPayload(
            "no usable unicast address found".into(),
        ));
    }

    let mut total_len = 0;
    for (k, v) in txt {
        total_len += k.len() + v.len();
        if k.is_empty() || k.len() > 255 || v.len() > 255 {
            return Err(DiscoveryError::InvalidPayload(
                "TXT key or value length invalid".into(),
            ));
        }
        if !k.is_ascii() || k.contains('=') {
            return Err(DiscoveryError::InvalidPayload(
                "TXT key must be ASCII without '='".into(),
            ));
        }
    }
    if total_len > 1300 {
        return Err(DiscoveryError::InvalidPayload(
            "total TXT length exceeds bounded limit".into(),
        ));
    }

    let proto_entry = txt
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("protocol"))
        .ok_or_else(|| DiscoveryError::InvalidPayload("missing protocol in TXT".into()))?;

    if proto_entry.1.as_slice() != b"3" {
        return Err(DiscoveryError::InvalidPayload(
            "unsupported protocol version".into(),
        ));
    }

    let name_entry = txt
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("name"))
        .ok_or_else(|| DiscoveryError::InvalidPayload("missing name in TXT".into()))?;

    let name = std::str::from_utf8(&name_entry.1)
        .map_err(|_| DiscoveryError::InvalidPayload("name must be valid UTF-8".into()))?
        .to_string();
    if name.is_empty() || name.len() > 255 {
        return Err(DiscoveryError::InvalidPayload(
            "name must be between 1 and 255 bytes".into(),
        ));
    }

    let os_entry = txt
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("os"))
        .ok_or_else(|| DiscoveryError::InvalidPayload("missing os in TXT".into()))?;

    let os = std::str::from_utf8(&os_entry.1)
        .map_err(|_| DiscoveryError::InvalidPayload("os must be valid UTF-8".into()))?
        .to_string();
    if os.is_empty() || os.len() > 64 {
        return Err(DiscoveryError::InvalidPayload(
            "os must be between 1 and 64 bytes".into(),
        ));
    }

    let udp_entry = txt
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("udp_port"))
        .ok_or_else(|| DiscoveryError::InvalidPayload("missing udp_port in TXT".into()))?;

    let udp_str = std::str::from_utf8(&udp_entry.1)
        .map_err(|_| DiscoveryError::InvalidPayload("udp_port must be valid ASCII".into()))?;

    let udp_port: u16 = udp_str
        .parse()
        .map_err(|_| DiscoveryError::InvalidPayload("udp_port is not a valid integer".into()))?;

    if udp_port == 0 {
        return Err(DiscoveryError::InvalidPayload(
            "udp_port must be nonzero".into(),
        ));
    }

    // The MAC is advisory (Wake-on-LAN only); a malformed value is ignored
    // rather than invalidating an otherwise usable service.
    let mac = txt
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("mac"))
        .and_then(|(_, v)| std::str::from_utf8(v).ok())
        .and_then(super::mac::normalize_mac);

    Ok(DiscoveredHost {
        id: fullname.to_string(),
        name,
        ip: endpoint.formatted.clone(),
        os,
        tcp_port: srv_port,
        udp_port,
        mac,
    })
}

pub fn decide_service_state_action(
    fullname: &str,
    hosttarget: Option<&str>,
    srv_port: u16,
    txt_items: &[(String, Vec<u8>)],
    addresses: &[(IpAddr, Option<u32>)],
) -> ServiceStateAction {
    let target = match hosttarget {
        Some(t) => t,
        None => return ServiceStateAction::Retract,
    };
    let endpoint = match choose_preferred_endpoint(addresses) {
        Some(res) => res,
        None => return ServiceStateAction::Retract,
    };
    match validate_resolved_service(fullname, target, srv_port, txt_items, &endpoint) {
        Ok(host) => ServiceStateAction::Publish(host),
        Err(_) => ServiceStateAction::Retract,
    }
}
