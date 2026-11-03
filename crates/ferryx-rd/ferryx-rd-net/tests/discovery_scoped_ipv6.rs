use ferryx_rd_net::discovery::{
    choose_preferred_endpoint, choose_preferred_ip_with_scope, decide_service_state_action,
    parse_service_metadata_scoped, DiscoveredEndpoint, ServiceStateAction,
};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs};

#[cfg(any(target_os = "macos", target_os = "ios"))]
const LOOPBACK_IFACE: &str = "lo0";
#[cfg(target_os = "linux")]
const LOOPBACK_IFACE: &str = "lo";

#[cfg(any(target_os = "macos", target_os = "ios"))]
use libc::if_nametoindex;

#[cfg(target_os = "linux")]
mod libc_compat {
    extern "C" {
        pub fn if_nametoindex(ifname: *const std::ffi::c_char) -> u32;
    }
}
#[cfg(target_os = "linux")]
use libc_compat::if_nametoindex;

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "ios"))]
fn get_platform_loopback_interface() -> (&'static str, u32) {
    let c_name =
        std::ffi::CString::new(LOOPBACK_IFACE).expect("valid C string for loopback interface");
    // SAFETY: `c_name` is an owned, null-terminated C string (`CString`) pointing to a
    // statically allocated ASCII interface name ("lo" on Linux, "lo0" on macOS/iOS).
    // The pointer is valid and non-null for the lifetime of `c_name`.
    // The POSIX `if_nametoindex` function reads the string up to the null terminator
    // (at most IFNAMSIZ characters) without mutating memory or retaining the pointer.
    let idx = unsafe { if_nametoindex(c_name.as_ptr()) };
    (LOOPBACK_IFACE, idx)
}

fn sample_valid_txt() -> Vec<(String, Vec<u8>)> {
    vec![
        ("protocol".into(), b"3".to_vec()),
        ("name".into(), b"desk-host".to_vec()),
        ("os".into(), b"macos".to_vec()),
        ("udp_port".into(), b"19731".to_vec()),
    ]
}

#[test]
fn test_scoped_ipv6_link_local_published() {
    let txt = sample_valid_txt();
    let addrs = vec![(
        IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1)),
        Some(5),
    )];

    let action = decide_service_state_action(
        "desk-host._maho-rd._tcp.local.",
        Some("desk-host.local."),
        19730,
        &txt,
        &addrs,
    );

    match action {
        ServiceStateAction::Publish(host) => {
            assert_eq!(host.id, "desk-host._maho-rd._tcp.local.");
            assert_eq!(host.name, "desk-host");
            assert_eq!(host.ip, "fe80::1%5");
            assert_eq!(host.os, "macos");
            assert_eq!(host.tcp_port, 19730);
            assert_eq!(host.udp_port, 19731);
        }
        ServiceStateAction::Retract => {
            panic!("expected Publish(host) for scoped link-local fe80::1%5, got Retract");
        }
    }
}

#[test]
fn test_unscoped_ipv6_link_local_retracted() {
    let txt = sample_valid_txt();
    let addrs = vec![(IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1)), None)];

    let action = decide_service_state_action(
        "desk-host._maho-rd._tcp.local.",
        Some("desk-host.local."),
        19730,
        &txt,
        &addrs,
    );

    assert_eq!(action, ServiceStateAction::Retract);
}

#[test]
fn test_scoped_ipv6_link_local_with_scope_zero_retracted() {
    let txt = sample_valid_txt();
    let addrs = vec![(
        IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1)),
        Some(0),
    )];

    let action = decide_service_state_action(
        "desk-host._maho-rd._tcp.local.",
        Some("desk-host.local."),
        19730,
        &txt,
        &addrs,
    );

    assert_eq!(action, ServiceStateAction::Retract);
}

#[test]
fn test_ipv4_preferred_over_scoped_ipv6_link_local() {
    let txt = sample_valid_txt();
    let addrs = vec![
        (
            IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1)),
            Some(5),
        ),
        (IpAddr::V4(Ipv4Addr::new(192, 168, 1, 50)), None),
    ];

    let action = decide_service_state_action(
        "desk-host._maho-rd._tcp.local.",
        Some("desk-host.local."),
        19730,
        &txt,
        &addrs,
    );

    match action {
        ServiceStateAction::Publish(host) => {
            assert_eq!(host.ip, "192.168.1.50");
            assert_eq!(host.tcp_port, 19730);
        }
        ServiceStateAction::Retract => {
            panic!("expected Publish with IPv4 192.168.1.50, got Retract");
        }
    }
}

#[test]
fn test_scoped_ipv6_published_string_and_resolver_retain_scope() {
    let txt = sample_valid_txt();
    let addrs = vec![(
        IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1)),
        Some(5),
    )];

    let action = decide_service_state_action(
        "desk-host._maho-rd._tcp.local.",
        Some("desk-host.local."),
        19730,
        &txt,
        &addrs,
    );

    let host = match action {
        ServiceStateAction::Publish(h) => h,
        ServiceStateAction::Retract => panic!("expected Publish"),
    };

    assert_eq!(host.ip, "fe80::1%5");

    let resolved: Vec<_> = (host.ip.as_str(), host.tcp_port)
        .to_socket_addrs()
        .expect("resolver should succeed for scoped link-local numeric scope")
        .collect();

    assert!(
        !resolved.is_empty(),
        "resolver must return at least one socket address"
    );
    match resolved[0] {
        std::net::SocketAddr::V6(v6_addr) => {
            assert_eq!(*v6_addr.ip(), Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1));
            assert_eq!(v6_addr.port(), 19730);
            assert_eq!(
                v6_addr.scope_id(),
                5,
                "resolver SocketAddrV6 must retain scope_id == 5"
            );
            assert_eq!(v6_addr.to_string(), "[fe80::1%5]:19730");
        }
        std::net::SocketAddr::V4(_) => panic!("expected IPv6 socket address"),
    }
}

#[test]
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "ios"))]
fn test_resolver_resolves_interface_name_scope() {
    let (iface, expected_scope) = get_platform_loopback_interface();
    assert!(
        expected_scope > 0,
        "platform loopback interface '{iface}' must have positive index via if_nametoindex"
    );

    let host_str = format!("fe80::1%{iface}");
    let list = ferryx_rd_net::discovery::resolve_socket_addrs(&host_str, 19730)
        .expect("interface name resolver must succeed for platform loopback interface");

    assert!(
        !list.is_empty(),
        "interface name resolver must return at least one address"
    );

    match list[0] {
        std::net::SocketAddr::V6(v6_addr) => {
            assert_eq!(*v6_addr.ip(), Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1));
            assert_eq!(v6_addr.port(), 19730);
            assert!(
                v6_addr.scope_id() > 0,
                "interface name must resolve to positive scope_id"
            );
            assert_eq!(
                v6_addr.scope_id(),
                expected_scope,
                "resolver scope_id must match exact index of '{iface}' from if_nametoindex"
            );
        }
        std::net::SocketAddr::V4(_) => {
            panic!("expected IPv6 socket address for scoped link-local");
        }
    }
}

#[test]
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "ios"))]
fn test_resolver_rejects_invalid_interface_name_scope() {
    let result = ferryx_rd_net::discovery::resolve_socket_addrs("fe80::1%nonexistent_iface99", 19730);
    assert!(
        result.is_err(),
        "interface name resolver must return Err for nonexistent interface"
    );
}

#[test]
fn test_choose_preferred_endpoint_selects_scoped_ipv6_and_preserves_scope() {
    let addrs = vec![(
        IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1)),
        Some(5),
    )];
    let ep = choose_preferred_endpoint(&addrs).expect("must select scoped link-local");
    assert_eq!(
        ep.ip,
        IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1))
    );
    assert_eq!(ep.scope_id, Some(5));
    assert_eq!(ep.formatted, "fe80::1%5");

    let ip_with_scope = choose_preferred_ip_with_scope(&addrs).expect("alias must match");
    assert_eq!(ip_with_scope, ep);

    let from_tuple: DiscoveredEndpoint = (
        IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1)),
        Some(5),
    )
        .into();
    assert_eq!(from_tuple, ep);
}

#[test]
fn test_parse_service_metadata_scoped_accepts_scoped_ipv6() {
    let txt = sample_valid_txt();
    let addrs = vec![(
        IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1)),
        Some(5),
    )];
    let host = parse_service_metadata_scoped(
        "desk-host._maho-rd._tcp.local.",
        "desk-host.local.",
        19730,
        &txt,
        &addrs,
    )
    .expect("must parse successfully");
    assert_eq!(host.ip, "fe80::1%5");
}
