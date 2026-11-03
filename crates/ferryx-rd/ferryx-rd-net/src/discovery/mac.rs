//! Primary network-interface MAC discovery for Wake-on-LAN advertisement.

use std::net::IpAddr;

/// Formats six octets as lowercase colon-separated hex.
pub fn format_mac(octets: &[u8; 6]) -> String {
    octets
        .iter()
        .map(|octet| format!("{octet:02x}"))
        .collect::<Vec<_>>()
        .join(":")
}

/// Accepts exactly six colon- or hyphen-separated hex pairs and returns the
/// canonical lowercase colon form. All-zero and multicast addresses are not
/// wakeable unicast NIC addresses and are rejected.
pub fn normalize_mac(text: &str) -> Option<String> {
    let parts: Vec<&str> = text.split([':', '-']).collect();
    if parts.len() != 6 {
        return None;
    }
    let mut octets = [0_u8; 6];
    for (slot, part) in octets.iter_mut().zip(parts) {
        if part.len() != 2 {
            return None;
        }
        *slot = u8::from_str_radix(part, 16).ok()?;
    }
    if octets == [0; 6] || octets[0] & 1 == 1 {
        return None;
    }
    Some(format_mac(&octets))
}

/// One network interface as seen by a platform walker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub ips: Vec<IpAddr>,
    pub mac: Option<[u8; 6]>,
    pub loopback: bool,
    pub tunnel: bool,
    pub up: bool,
}

/// Picks the first usable interface that actually has a wakeable link
/// address. With a specific bind address only the interface owning it
/// qualifies; an unspecified bind accepts the first IPv4-bearing interface.
/// Tunnels (Tailscale utun, VPN adapters) and interfaces without a 6-byte
/// MAC are skipped instead of ending the search.
pub fn choose_mac(candidates: &[Candidate], bind: IpAddr) -> Option<String> {
    candidates
        .iter()
        .filter(|candidate| candidate.up && !candidate.loopback && !candidate.tunnel)
        .filter(|candidate| {
            if bind.is_unspecified() {
                candidate.ips.iter().any(IpAddr::is_ipv4)
            } else {
                candidate.ips.contains(&bind)
            }
        })
        .find_map(|candidate| {
            candidate
                .mac
                .and_then(|mac| normalize_mac(&format_mac(&mac)))
        })
}

/// MAC of the interface that owns `bind` (or, for an unspecified bind, the
/// first suitable IPv4 interface). `None` when it cannot be determined.
pub fn primary_mac(bind: IpAddr) -> Option<String> {
    choose_mac(&platform::candidates(), bind)
}

#[cfg(target_os = "linux")]
mod platform {
    use super::Candidate;

    pub fn candidates() -> Vec<Candidate> {
        let Ok(interfaces) = if_addrs::get_if_addrs() else {
            return Vec::new();
        };
        let mut names: Vec<String> = Vec::new();
        for interface in &interfaces {
            if !names.contains(&interface.name) {
                names.push(interface.name.clone());
            }
        }
        names
            .into_iter()
            .map(|name| {
                let ips = interfaces
                    .iter()
                    .filter(|interface| interface.name == name)
                    .map(if_addrs::Interface::ip)
                    .collect();
                let loopback = interfaces
                    .iter()
                    .any(|interface| interface.name == name && interface.is_loopback());
                let mac = std::fs::read_to_string(format!("/sys/class/net/{name}/address"))
                    .ok()
                    .and_then(|text| super::normalize_mac(text.trim()))
                    .and_then(|text| parse_octets(&text));
                Candidate {
                    ips,
                    mac,
                    loopback,
                    tunnel: false,
                    up: true,
                }
            })
            .collect()
    }

    fn parse_octets(text: &str) -> Option<[u8; 6]> {
        let mut octets = [0_u8; 6];
        let mut parts = text.split(':');
        for slot in &mut octets {
            *slot = u8::from_str_radix(parts.next()?, 16).ok()?;
        }
        Some(octets)
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::Candidate;
    use std::collections::BTreeMap;
    use std::ffi::CStr;
    use std::net::IpAddr;

    pub fn candidates() -> Vec<Candidate> {
        let mut head: *mut libc::ifaddrs = std::ptr::null_mut();
        // SAFETY: getifaddrs fills `head` with a list released by freeifaddrs below.
        if unsafe { libc::getifaddrs(&mut head) } != 0 {
            return Vec::new();
        }
        let mut by_name: BTreeMap<String, Candidate> = BTreeMap::new();
        let mut order: Vec<String> = Vec::new();
        let mut cursor = head;
        while !cursor.is_null() {
            // SAFETY: `cursor` walks the list returned by getifaddrs.
            let entry = unsafe { &*cursor };
            cursor = entry.ifa_next;
            if entry.ifa_addr.is_null() || entry.ifa_name.is_null() {
                continue;
            }
            // SAFETY: ifa_name is a NUL-terminated string owned by the list.
            let name = unsafe { CStr::from_ptr(entry.ifa_name) }
                .to_string_lossy()
                .into_owned();
            if !by_name.contains_key(&name) {
                order.push(name.clone());
            }
            let candidate = by_name.entry(name).or_insert(Candidate {
                ips: Vec::new(),
                mac: None,
                loopback: entry.ifa_flags & (libc::IFF_LOOPBACK as u32) != 0,
                tunnel: false,
                up: entry.ifa_flags & (libc::IFF_UP as u32) != 0,
            });
            // SAFETY: ifa_addr was checked non-null; sa_family selects the layout.
            let family = i32::from(unsafe { (*entry.ifa_addr).sa_family });
            if family == libc::AF_INET {
                // SAFETY: AF_INET addresses are sockaddr_in.
                let sin = unsafe { &*(entry.ifa_addr as *const libc::sockaddr_in) };
                candidate
                    .ips
                    .push(IpAddr::from(sin.sin_addr.s_addr.to_ne_bytes()));
            } else if family == libc::AF_LINK {
                // SAFETY: AF_LINK addresses are sockaddr_dl; LLADDR follows the name bytes.
                let link = unsafe { &*(entry.ifa_addr as *const libc::sockaddr_dl) };
                if link.sdl_alen == 6 {
                    let start = usize::from(link.sdl_nlen);
                    let data = link.sdl_data.as_ptr().cast::<u8>();
                    let mut octets = [0_u8; 6];
                    for (index, slot) in octets.iter_mut().enumerate() {
                        // SAFETY: sdl_alen == 6 bytes follow sdl_nlen name bytes.
                        *slot = unsafe { *data.add(start + index) };
                    }
                    candidate.mac = Some(octets);
                }
            }
        }
        // SAFETY: `head` came from a successful getifaddrs call and is freed once.
        unsafe { libc::freeifaddrs(head) };
        order
            .into_iter()
            .filter_map(|name| by_name.remove(&name))
            .collect()
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::Candidate;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    use windows::Win32::Foundation::ERROR_BUFFER_OVERFLOW;
    use windows::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER,
        GAA_FLAG_SKIP_MULTICAST, IF_TYPE_SOFTWARE_LOOPBACK, IF_TYPE_TUNNEL,
        IP_ADAPTER_ADDRESSES_LH,
    };
    use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
    use windows::Win32::Networking::WinSock::{
        AF_INET, AF_INET6, AF_UNSPEC, SOCKADDR_IN, SOCKADDR_IN6,
    };

    pub fn candidates() -> Vec<Candidate> {
        let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
        // The adapter list can grow between the size query and the fill call,
        // so retry a bounded number of times on ERROR_BUFFER_OVERFLOW.
        let mut size: u32 = 15 * 1024;
        for _ in 0..4 {
            let mut buffer: Vec<u64> = vec![0_u64; (size as usize).div_ceil(8)];
            let head = buffer.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
            // SAFETY: `head` points to a zeroed, 8-byte-aligned buffer of at
            // least `size` bytes, and `size` is a valid in/out length.
            let status = unsafe {
                GetAdaptersAddresses(u32::from(AF_UNSPEC.0), flags, None, Some(head), &mut size)
            };
            if status == 0 {
                return walk(head);
            }
            if status != ERROR_BUFFER_OVERFLOW.0 {
                return Vec::new();
            }
        }
        Vec::new()
    }

    fn walk(head: *const IP_ADAPTER_ADDRESSES_LH) -> Vec<Candidate> {
        let mut out = Vec::new();
        let mut adapter = head;
        while !adapter.is_null() {
            // SAFETY: the list was filled by GetAdaptersAddresses into a live buffer.
            let entry = unsafe { &*adapter };
            adapter = entry.Next;
            let mut ips = Vec::new();
            let mut unicast = entry.FirstUnicastAddress;
            while !unicast.is_null() {
                // SAFETY: unicast nodes live in the same buffer as the adapter.
                let node = unsafe { &*unicast };
                unicast = node.Next;
                let sockaddr = node.Address.lpSockaddr;
                if sockaddr.is_null() {
                    continue;
                }
                // SAFETY: lpSockaddr is non-null and sa_family selects the layout.
                let family = unsafe { (*sockaddr).sa_family };
                if family == AF_INET {
                    // SAFETY: AF_INET addresses are SOCKADDR_IN.
                    let sin = unsafe { &*sockaddr.cast::<SOCKADDR_IN>() };
                    // SAFETY: reading the S_addr view of the address union.
                    let raw = unsafe { sin.sin_addr.S_un.S_addr };
                    ips.push(IpAddr::V4(Ipv4Addr::from(raw.to_ne_bytes())));
                } else if family == AF_INET6 {
                    // SAFETY: AF_INET6 addresses are SOCKADDR_IN6.
                    let sin6 = unsafe { &*sockaddr.cast::<SOCKADDR_IN6>() };
                    // SAFETY: reading the byte view of the address union.
                    let bytes = unsafe { sin6.sin6_addr.u.Byte };
                    ips.push(IpAddr::V6(Ipv6Addr::from(bytes)));
                }
            }
            let mac = (entry.PhysicalAddressLength == 6).then(|| {
                let mut octets = [0_u8; 6];
                octets.copy_from_slice(&entry.PhysicalAddress[..6]);
                octets
            });
            out.push(Candidate {
                ips,
                mac,
                loopback: entry.IfType == IF_TYPE_SOFTWARE_LOOPBACK,
                tunnel: entry.IfType == IF_TYPE_TUNNEL,
                up: entry.OperStatus == IfOperStatusUp,
            });
        }
        out
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod platform {
    use super::Candidate;

    pub fn candidates() -> Vec<Candidate> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_separators_and_case() {
        assert_eq!(
            normalize_mac("AA-bb-0C-01-02-03").as_deref(),
            Some("aa:bb:0c:01:02:03")
        );
    }

    #[test]
    fn rejects_malformed_zero_and_multicast() {
        for bad in [
            "",
            "aa:bb:cc:dd:ee",
            "aa:bb:cc:dd:ee:ff:00",
            "aa:bb:cc:dd:ee:g0",
            "a:bb:cc:dd:ee:ff",
            "00:00:00:00:00:00",
            "01:00:5e:00:00:01",
        ] {
            assert_eq!(normalize_mac(bad), None, "{bad}");
        }
    }

    fn candidate(ips: &[&str], mac: Option<[u8; 6]>) -> Candidate {
        Candidate {
            ips: ips.iter().map(|ip| ip.parse().unwrap()).collect(),
            mac,
            loopback: false,
            tunnel: false,
            up: true,
        }
    }

    const NIC: [u8; 6] = [0x3c, 0x22, 0xfb, 0x01, 0x02, 0x03];

    #[test]
    fn skips_tunnel_without_link_address_and_uses_next_interface() {
        // utun (Tailscale) comes first and has no MAC; en0 must still win.
        let candidates = [
            candidate(&["100.64.0.2"], None),
            candidate(&["192.168.1.5"], Some(NIC)),
        ];
        let unspecified = "0.0.0.0".parse().unwrap();
        assert_eq!(
            choose_mac(&candidates, unspecified).as_deref(),
            Some("3c:22:fb:01:02:03")
        );
    }

    #[test]
    fn specific_bind_only_matches_the_owning_interface() {
        let other = [0x3c, 0x22, 0xfb, 0x0a, 0x0b, 0x0c];
        let candidates = [
            candidate(&["192.168.1.5"], Some(NIC)),
            candidate(&["10.0.0.9"], Some(other)),
        ];
        assert_eq!(
            choose_mac(&candidates, "10.0.0.9".parse().unwrap()).as_deref(),
            Some("3c:22:fb:0a:0b:0c")
        );
        assert_eq!(choose_mac(&candidates, "172.16.0.1".parse().unwrap()), None);
    }

    #[test]
    fn ignores_loopback_tunnel_down_and_ipv6_only_interfaces() {
        let mut loopback = candidate(&["127.0.0.1"], Some(NIC));
        loopback.loopback = true;
        let mut tunnel = candidate(&["10.8.0.2"], Some(NIC));
        tunnel.tunnel = true;
        let mut down = candidate(&["192.168.9.9"], Some(NIC));
        down.up = false;
        let v6_only = candidate(&["fe80::1"], Some(NIC));
        let unspecified = "0.0.0.0".parse().unwrap();
        assert_eq!(
            choose_mac(&[loopback, tunnel, down, v6_only], unspecified),
            None
        );
    }
}
