use ferryx_rd_net::discovery::{parse_service_metadata, DiscoveredHost, DiscoveryTracker};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

fn valid_txt() -> Vec<(String, Vec<u8>)> {
    vec![
        ("protocol".into(), b"3".to_vec()),
        ("name".into(), b"host-workstation".to_vec()),
        ("os".into(), b"linux".to_vec()),
        ("udp_port".into(), b"19731".to_vec()),
    ]
}

#[test]
fn parse_service_metadata_accepts_valid_v3_records_and_prefers_ipv4() {
    let txt = valid_txt();
    let addresses = vec![
        IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1)),
        IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)),
    ];
    let host = parse_service_metadata(
        "host-workstation._maho-rd._tcp.local.",
        "host-workstation.local.",
        19730,
        &txt,
        &addresses,
    )
    .unwrap();

    assert_eq!(host.id, "host-workstation._maho-rd._tcp.local.");
    assert_eq!(host.name, "host-workstation");
    assert_eq!(host.os, "linux");
    assert_eq!(host.tcp_port, 19730);
    assert_eq!(host.udp_port, 19731);
    assert_eq!(host.ip, "192.168.1.100");
}

#[test]
fn parse_service_metadata_accepts_usable_ipv6_when_ipv4_absent() {
    let txt = valid_txt();
    let addresses = vec![IpAddr::V6(Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 2))];
    let host = parse_service_metadata(
        "host-ipv6._maho-rd._tcp.local.",
        "host-ipv6.local.",
        19730,
        &txt,
        &addresses,
    )
    .unwrap();

    assert_eq!(host.ip, "fd00::2");
}

#[test]
fn parse_service_metadata_rejects_missing_protocol() {
    let txt = vec![
        ("name".into(), b"host".to_vec()),
        ("os".into(), b"linux".to_vec()),
        ("udp_port".into(), b"19731".to_vec()),
    ];
    let addresses = vec![IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100))];
    let result = parse_service_metadata(
        "host._maho-rd._tcp.local.",
        "host.local.",
        19730,
        &txt,
        &addresses,
    );
    assert!(result.is_err());
}

#[test]
fn parse_service_metadata_rejects_unsupported_protocol() {
    for proto in [b"1".as_slice(), b"2", b"4", b"invalid"] {
        let txt = vec![
            ("protocol".into(), proto.to_vec()),
            ("name".into(), b"host".to_vec()),
            ("os".into(), b"linux".to_vec()),
            ("udp_port".into(), b"19731".to_vec()),
        ];
        let addresses = vec![IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100))];
        let result = parse_service_metadata(
            "host._maho-rd._tcp.local.",
            "host.local.",
            19730,
            &txt,
            &addresses,
        );
        assert!(result.is_err());
    }
}

#[test]
fn parse_service_metadata_rejects_zero_or_missing_ports() {
    let addresses = vec![IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100))];

    let txt_zero_udp = vec![
        ("protocol".into(), b"3".to_vec()),
        ("name".into(), b"host".to_vec()),
        ("os".into(), b"linux".to_vec()),
        ("udp_port".into(), b"0".to_vec()),
    ];
    assert!(parse_service_metadata(
        "h._maho-rd._tcp.local.",
        "h.local.",
        19730,
        &txt_zero_udp,
        &addresses
    )
    .is_err());

    let txt_valid = valid_txt();
    assert!(parse_service_metadata(
        "h._maho-rd._tcp.local.",
        "h.local.",
        0,
        &txt_valid,
        &addresses
    )
    .is_err());

    let txt_missing_udp = vec![
        ("protocol".into(), b"3".to_vec()),
        ("name".into(), b"host".to_vec()),
        ("os".into(), b"linux".to_vec()),
    ];
    assert!(parse_service_metadata(
        "h._maho-rd._tcp.local.",
        "h.local.",
        19730,
        &txt_missing_udp,
        &addresses
    )
    .is_err());
}

#[test]
fn parse_service_metadata_rejects_oversized_metadata() {
    let long_name = vec![b'a'; 300];
    let txt = vec![
        ("protocol".into(), b"3".to_vec()),
        ("name".into(), long_name),
        ("os".into(), b"linux".to_vec()),
        ("udp_port".into(), b"19731".to_vec()),
    ];
    let addresses = vec![IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100))];
    let result = parse_service_metadata(
        "h._maho-rd._tcp.local.",
        "h.local.",
        19730,
        &txt,
        &addresses,
    );
    assert!(result.is_err());
}

#[test]
fn parse_service_metadata_rejects_empty_addresses() {
    let txt = valid_txt();
    let addresses: Vec<IpAddr> = Vec::new();
    let result = parse_service_metadata(
        "h._maho-rd._tcp.local.",
        "h.local.",
        19730,
        &txt,
        &addresses,
    );
    assert!(result.is_err());
}

#[test]
fn tracker_add_service_makes_host_visible_in_snapshot() {
    let mut tracker = DiscoveryTracker::new();
    assert!(tracker.snapshot().is_empty());

    let host = DiscoveredHost {
        id: "desk._maho-rd._tcp.local.".into(),
        name: "desk".into(),
        ip: "192.168.1.50".into(),
        os: "linux".into(),
        tcp_port: 19730,
        udp_port: 19731,
        mac: None,
    };
    tracker.upsert(host.clone());

    let snap = tracker.snapshot();
    assert_eq!(snap.len(), 1);
    assert_eq!(snap[0], host);
}

#[test]
fn tracker_remove_service_removes_host_from_snapshot() {
    let mut tracker = DiscoveryTracker::new();
    tracker.upsert(DiscoveredHost {
        id: "desk._maho-rd._tcp.local.".into(),
        name: "desk".into(),
        ip: "192.168.1.50".into(),
        os: "linux".into(),
        tcp_port: 19730,
        udp_port: 19731,
        mac: None,
    });
    assert_eq!(tracker.snapshot().len(), 1);

    tracker.remove("desk._maho-rd._tcp.local.");
    assert!(tracker.snapshot().is_empty());

    tracker.remove("nonexistent._maho-rd._tcp.local.");
    assert!(tracker.snapshot().is_empty());
}

#[test]
fn tracker_address_replacement_updates_existing_entry() {
    let mut tracker = DiscoveryTracker::new();
    tracker.upsert(DiscoveredHost {
        id: "desk._maho-rd._tcp.local.".into(),
        name: "desk".into(),
        ip: "192.168.1.50".into(),
        os: "linux".into(),
        tcp_port: 19730,
        udp_port: 19731,
        mac: None,
    });

    tracker.upsert(DiscoveredHost {
        id: "desk._maho-rd._tcp.local.".into(),
        name: "desk-renamed".into(),
        ip: "192.168.1.99".into(),
        os: "linux".into(),
        tcp_port: 19730,
        udp_port: 19731,
        mac: None,
    });

    let snap = tracker.snapshot();
    assert_eq!(snap.len(), 1);
    assert_eq!(snap[0].id, "desk._maho-rd._tcp.local.");
    assert_eq!(snap[0].name, "desk-renamed");
    assert_eq!(snap[0].ip, "192.168.1.99");
}

#[test]
fn parse_service_metadata_rejects_loopback_only_addresses() {
    let txt = valid_txt();
    for loopback in [
        IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
        IpAddr::V6(Ipv6Addr::LOCALHOST),
    ] {
        let res = parse_service_metadata(
            "host._maho-rd._tcp.local.",
            "host.local.",
            19730,
            &txt,
            &[loopback],
        );
        assert!(res.is_err(), "loopback address {loopback} must be rejected");
    }
}

#[test]
fn parse_service_metadata_rejects_unspecified_only_addresses() {
    let txt = valid_txt();
    for unspec in [
        IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        IpAddr::V6(Ipv6Addr::UNSPECIFIED),
    ] {
        let res = parse_service_metadata(
            "host._maho-rd._tcp.local.",
            "host.local.",
            19730,
            &txt,
            &[unspec],
        );
        assert!(
            res.is_err(),
            "unspecified address {unspec} must be rejected"
        );
    }
}

#[test]
fn parse_service_metadata_rejects_multicast_only_addresses() {
    let txt = valid_txt();
    for mcast in [
        IpAddr::V4(Ipv4Addr::new(224, 0, 0, 251)),
        IpAddr::V6(Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 0xfb)),
    ] {
        let res = parse_service_metadata(
            "host._maho-rd._tcp.local.",
            "host.local.",
            19730,
            &txt,
            &[mcast],
        );
        assert!(res.is_err(), "multicast address {mcast} must be rejected");
    }
}

#[test]
fn parse_service_metadata_rejects_unscoped_ipv6_link_local_only() {
    let txt = valid_txt();
    let link_local = vec![IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1))];
    let res = parse_service_metadata(
        "host._maho-rd._tcp.local.",
        "host.local.",
        19730,
        &txt,
        &link_local,
    );
    assert!(
        res.is_err(),
        "unscoped link-local IPv6 address must be rejected"
    );
}

#[test]
fn parse_service_metadata_accepts_publicly_numbered_ethernet_ip() {
    let txt = valid_txt();
    let addresses = vec![IpAddr::V4(Ipv4Addr::new(1, 231, 34, 236))];
    let host = parse_service_metadata(
        "omarchy._maho-rd._tcp.local.",
        "omarchy.local.",
        19730,
        &txt,
        &addresses,
    )
    .unwrap();
    assert_eq!(host.ip, "1.231.34.236");
}

#[test]
fn parse_service_metadata_rejects_invalid_fullname() {
    let txt = valid_txt();
    let addresses = vec![IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10))];

    assert!(parse_service_metadata("", "host.local.", 19730, &txt, &addresses).is_err());

    assert!(parse_service_metadata(
        "host._other._tcp.local.",
        "host.local.",
        19730,
        &txt,
        &addresses
    )
    .is_err());

    let long_fn = format!("{}.{}", "a".repeat(250), "_maho-rd._tcp.local.");
    assert!(parse_service_metadata(&long_fn, "host.local.", 19730, &txt, &addresses).is_err());
}

#[test]
fn parse_service_metadata_rejects_out_of_range_udp_port() {
    let addresses = vec![IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10))];
    for bad_port in ["0", "65536", "70000", "-1", "abc", "12.34"] {
        let txt = vec![
            ("protocol".into(), b"3".to_vec()),
            ("name".into(), b"host".to_vec()),
            ("os".into(), b"linux".to_vec()),
            ("udp_port".into(), bad_port.as_bytes().to_vec()),
        ];
        assert!(
            parse_service_metadata(
                "host._maho-rd._tcp.local.",
                "h.local.",
                19730,
                &txt,
                &addresses
            )
            .is_err(),
            "udp_port {bad_port} must be rejected"
        );
    }
}

#[test]
fn tracker_clear_resets_snapshot_to_empty() {
    let mut tracker = DiscoveryTracker::new();
    tracker.upsert(DiscoveredHost {
        id: "h1._maho-rd._tcp.local.".into(),
        name: "h1".into(),
        ip: "192.168.1.1".into(),
        os: "linux".into(),
        tcp_port: 19730,
        udp_port: 19731,
        mac: None,
    });
    tracker.upsert(DiscoveredHost {
        id: "h2._maho-rd._tcp.local.".into(),
        name: "h2".into(),
        ip: "192.168.1.2".into(),
        os: "linux".into(),
        tcp_port: 19730,
        udp_port: 19731,
        mac: None,
    });
    assert_eq!(tracker.snapshot().len(), 2);

    tracker.clear();
    assert!(tracker.snapshot().is_empty());
}
