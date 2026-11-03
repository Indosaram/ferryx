use super::{
    attach_link_local_scopes, parse_service_metadata_scoped, DiscoveredHost, DiscoveryError,
    DiscoveryTracker,
};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

pub struct MdnsSdBrowser {
    tracker: Arc<Mutex<DiscoveryTracker>>,
    daemon: Option<mdns_sd::ServiceDaemon>,
    worker_handle: Option<thread::JoinHandle<()>>,
    last_error: Arc<Mutex<Option<DiscoveryError>>>,
    cancel_flag: Arc<AtomicBool>,
}

impl MdnsSdBrowser {
    pub fn new() -> Result<Self, DiscoveryError> {
        let daemon = mdns_sd::ServiceDaemon::new()
            .map_err(|e| DiscoveryError::Backend(format!("mdns-sd daemon error: {e}")))?;

        let monitor_rx = match daemon.monitor() {
            Ok(rx) => rx,
            Err(e) => {
                let _ = daemon.shutdown();
                return Err(DiscoveryError::Backend(format!(
                    "mdns-sd monitor error: {e}"
                )));
            }
        };

        let service_type = "_maho-rd._tcp.local.";
        let receiver = match daemon.browse(service_type) {
            Ok(rx) => rx,
            Err(e) => {
                let _ = daemon.shutdown();
                return Err(DiscoveryError::Backend(format!(
                    "mdns-sd browse error: {e}"
                )));
            }
        };

        let tracker = Arc::new(Mutex::new(DiscoveryTracker::new()));
        let last_error = Arc::new(Mutex::new(None));
        let cancel_flag = Arc::new(AtomicBool::new(false));

        let tracker_clone = Arc::clone(&tracker);
        let last_error_clone = Arc::clone(&last_error);
        let cancel_flag_clone = Arc::clone(&cancel_flag);
        let worker_handle = thread::Builder::new()
            .name("maho-mdns-browser".into())
            .spawn(move || {
                while !cancel_flag_clone.load(Ordering::Relaxed) {
                    while let Ok(daemon_event) = monitor_rx.try_recv() {
                        if let mdns_sd::DaemonEvent::Error(err) = daemon_event {
                            tracing::warn!("mDNS daemon monitor error: {err}");
                            if let Ok(mut err_guard) = last_error_clone.lock() {
                                *err_guard = Some(DiscoveryError::Backend(err.to_string()));
                            }
                        }
                    }

                    match receiver.recv_timeout(Duration::from_millis(200)) {
                        Ok(event) => match event {
                            mdns_sd::ServiceEvent::ServiceResolved(info) => {
                                let fullname = info.get_fullname().to_string();
                                let host_target = info.get_hostname().to_string();
                                let srv_port = info.get_port();

                                let mut txt_items = Vec::new();
                                for prop in info.get_properties().iter() {
                                    let key = prop.key().to_string();
                                    let val = prop.val().unwrap_or_default().to_vec();
                                    txt_items.push((key, val));
                                }

                                let addrs: Vec<IpAddr> =
                                    info.get_addresses().iter().copied().collect();
                                let scoped_addrs = scope_resolved_addresses(&addrs);

                                if let Ok(host) = parse_service_metadata_scoped(
                                    &fullname,
                                    &host_target,
                                    srv_port,
                                    &txt_items,
                                    &scoped_addrs,
                                ) {
                                    if let Ok(mut tr_guard) = tracker_clone.lock() {
                                        tr_guard.upsert(host);
                                    }
                                }
                            }
                            mdns_sd::ServiceEvent::ServiceRemoved(_, fullname) => {
                                if let Ok(mut tr_guard) = tracker_clone.lock() {
                                    tr_guard.remove(&fullname);
                                }
                            }
                            mdns_sd::ServiceEvent::SearchStopped(_) => {
                                break;
                            }
                            _ => {}
                        },
                        Err(flume::RecvTimeoutError::Timeout) => {}
                        Err(flume::RecvTimeoutError::Disconnected) => {
                            if !cancel_flag_clone.load(Ordering::Relaxed) {
                                tracing::warn!("mDNS browse channel disconnected unexpectedly");
                                if let Ok(mut err_guard) = last_error_clone.lock() {
                                    *err_guard = Some(DiscoveryError::Backend(
                                        "mDNS browse channel closed unexpectedly".into(),
                                    ));
                                }
                                if let Ok(mut tr_guard) = tracker_clone.lock() {
                                    tr_guard.clear();
                                }
                            }
                            break;
                        }
                    }
                }
            })
            .map_err(|e| {
                let _ = daemon.shutdown();
                DiscoveryError::Backend(format!("failed to spawn worker: {e}"))
            })?;

        Ok(Self {
            tracker,
            daemon: Some(daemon),
            worker_handle: Some(worker_handle),
            last_error,
            cancel_flag,
        })
    }

    pub fn snapshot(&self) -> Result<Vec<DiscoveredHost>, DiscoveryError> {
        if let Ok(err_guard) = self.last_error.lock() {
            if let Some(err) = err_guard.clone() {
                return Err(err);
            }
        }
        self.tracker
            .lock()
            .map(|t| t.snapshot())
            .map_err(|_| DiscoveryError::Backend("tracker lock poisoned".into()))
    }
}

impl Drop for MdnsSdBrowser {
    fn drop(&mut self) {
        self.cancel_flag.store(true, Ordering::SeqCst);
        if let Some(ref daemon) = self.daemon {
            if let Err(e) = daemon.stop_browse("_maho-rd._tcp.local.") {
                tracing::warn!("Failed to stop mDNS browse in Drop: {e}");
            }
        }
        if let Some(daemon) = self.daemon.take() {
            match daemon.shutdown() {
                Ok(rx) => {
                    let _ = rx.recv_timeout(Duration::from_secs(1));
                }
                Err(e) => {
                    tracing::warn!("Failed to shutdown mDNS browser daemon in Drop: {e}");
                }
            }
        }
        if let Some(handle) = self.worker_handle.take() {
            if let Err(e) = handle.join() {
                tracing::warn!("Failed to join mDNS browser worker thread: {e:?}");
            }
        }
    }
}

pub struct MdnsSdAdvertiser {
    daemon: Option<mdns_sd::ServiceDaemon>,
    fullname: String,
}

impl MdnsSdAdvertiser {
    pub fn start(
        name: &str,
        tcp_port: u16,
        udp_port: u16,
        bind_addr: SocketAddr,
    ) -> Result<Self, DiscoveryError> {
        let daemon = mdns_sd::ServiceDaemon::new()
            .map_err(|e| DiscoveryError::Backend(format!("mdns-sd daemon error: {e}")))?;

        let ip_to_advertise = if bind_addr.ip().is_loopback() {
            if let Err(e) = daemon.disable_interface(mdns_sd::IfKind::All) {
                tracing::warn!("Failed to disable all interfaces for loopback bind: {e}");
            }
            if bind_addr.is_ipv4() {
                daemon
                    .enable_interface(mdns_sd::IfKind::LoopbackV4)
                    .map_err(|e| {
                        DiscoveryError::Backend(format!("failed to enable LoopbackV4: {e}"))
                    })?;
            } else {
                daemon
                    .enable_interface(mdns_sd::IfKind::LoopbackV6)
                    .map_err(|e| {
                        DiscoveryError::Backend(format!("failed to enable LoopbackV6: {e}"))
                    })?;
            }
            bind_addr.ip().to_string()
        } else if bind_addr.ip().is_unspecified() {
            let lan_interfaces = enumerate_lan_interfaces()?;
            if lan_interfaces.is_empty() {
                let _ = daemon.shutdown();
                return Err(DiscoveryError::Backend(
                    "no active physical LAN interface found for advertisement".into(),
                ));
            }

            let _ = daemon.disable_interface(mdns_sd::IfKind::LoopbackV4);
            let _ = daemon.disable_interface(mdns_sd::IfKind::LoopbackV6);

            let mut seen_names = std::collections::HashSet::new();
            let mut ips = Vec::new();
            for (if_name, ip) in lan_interfaces {
                if seen_names.insert(if_name.clone()) {
                    if let Err(e) = daemon.enable_interface(mdns_sd::IfKind::Name(if_name)) {
                        tracing::debug!("enable_interface failed: {e}");
                    }
                }
                ips.push(ip.to_string());
            }
            ips.dedup();
            ips.join(",")
        } else {
            if let Err(e) = daemon.disable_interface(mdns_sd::IfKind::All) {
                tracing::warn!("Failed to disable all interfaces for specific bind: {e}");
            }
            daemon
                .enable_interface(mdns_sd::IfKind::Addr(bind_addr.ip()))
                .map_err(|e| {
                    DiscoveryError::Backend(format!(
                        "failed to enable interface for {}: {e}",
                        bind_addr.ip()
                    ))
                })?;
            bind_addr.ip().to_string()
        };

        let mut properties = HashMap::new();
        properties.insert("protocol".to_string(), "3".to_string());
        properties.insert("name".to_string(), name.to_string());
        properties.insert("os".to_string(), std::env::consts::OS.to_string());
        properties.insert("udp_port".to_string(), udp_port.to_string());
        if let Some(mac) = super::mac::primary_mac(bind_addr.ip()) {
            properties.insert("mac".to_string(), mac);
        }

        let service_type = "_maho-rd._tcp.local.";
        let host_name = format!("{name}.local.");

        let service_info = mdns_sd::ServiceInfo::new(
            service_type,
            name,
            &host_name,
            ip_to_advertise.as_str(),
            tcp_port,
            properties,
        )
        .map_err(|e| DiscoveryError::InvalidPayload(format!("invalid service info: {e}")))?
        .enable_addr_auto();

        let fullname = service_info.get_fullname().to_string();
        if let Err(e) = daemon.register(service_info) {
            let _ = daemon.shutdown();
            return Err(DiscoveryError::Backend(format!(
                "mdns-sd register error: {e}"
            )));
        }

        Ok(Self {
            daemon: Some(daemon),
            fullname,
        })
    }
}

impl Drop for MdnsSdAdvertiser {
    fn drop(&mut self) {
        if let Some(daemon) = self.daemon.take() {
            if let Err(e) = daemon.unregister(&self.fullname) {
                tracing::warn!(
                    "Failed to unregister mDNS service '{}' in Drop: {e}",
                    self.fullname
                );
            }
            match daemon.shutdown() {
                Ok(rx) => {
                    let _ = rx.recv_timeout(Duration::from_secs(1));
                }
                Err(e) => {
                    tracing::warn!("Failed to shutdown mDNS advertiser daemon in Drop: {e}");
                }
            }
        }
    }
}

/// `mdns-sd` reports resolved addresses without a scope id, so an advertised IPv6 link-local
/// address would be discarded as unusable. Pair each such address with the local interface indices
/// that carry an IPv6 link-local address of their own; those are the only interfaces over which a
/// link-local peer is reachable.
fn scope_resolved_addresses(addresses: &[IpAddr]) -> Vec<(IpAddr, Option<u32>)> {
    let has_link_local = addresses
        .iter()
        .any(|ip| matches!(ip, IpAddr::V6(v6) if is_unscoped_link_local_ipv6(v6)));
    if !has_link_local {
        return addresses.iter().map(|&ip| (ip, None)).collect();
    }
    attach_link_local_scopes(addresses, &link_local_ipv6_interface_indices())
}

/// Indices of non-loopback, non-excluded interfaces that have an IPv6 link-local address, sorted
/// ascending so scope selection is deterministic.
fn link_local_ipv6_interface_indices() -> Vec<u32> {
    let interfaces = match if_addrs::get_if_addrs() {
        Ok(interfaces) => interfaces,
        Err(e) => {
            tracing::warn!("failed to enumerate interfaces for IPv6 scope resolution: {e}");
            return Vec::new();
        }
    };

    let mut indices: Vec<u32> = interfaces
        .into_iter()
        .filter(|iface| {
            !iface.is_loopback()
                && !is_excluded_interface_name(&iface.name.to_ascii_lowercase())
                && matches!(iface.ip(), IpAddr::V6(v6) if is_unscoped_link_local_ipv6(&v6))
        })
        .filter_map(|iface| iface.index.filter(|&idx| idx > 0))
        .collect();
    indices.sort_unstable();
    indices.dedup();
    indices
}

pub fn enumerate_lan_interfaces() -> Result<Vec<(String, IpAddr)>, DiscoveryError> {
    let addrs = if_addrs::get_if_addrs()
        .map_err(|e| DiscoveryError::Backend(format!("failed to enumerate interfaces: {e}")))?;

    let mut result = Vec::new();
    for iface in addrs {
        if iface.is_loopback() {
            continue;
        }
        let name_lower = iface.name.to_ascii_lowercase();
        if is_excluded_interface_name(&name_lower) {
            continue;
        }
        let ip = iface.addr.ip();
        if is_excluded_ip(&ip) {
            continue;
        }
        result.push((iface.name, ip));
    }
    Ok(result)
}

pub fn is_excluded_interface_name(name: &str) -> bool {
    let excluded_prefixes = [
        "tailscale",
        "tun",
        "tap",
        "wg",
        "wireguard",
        "docker",
        "veth",
        "br-",
        "cni",
        "flannel",
        "dummy",
        "virbr",
        "vmnet",
    ];
    excluded_prefixes
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

pub fn is_excluded_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_unspecified()
                || v4.is_multicast()
                || v4.is_broadcast()
                || is_tailscale_ipv4(v4)
                || is_docker_ipv4(v4)
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || is_unscoped_link_local_ipv6(v6)
                || is_tailscale_ipv6(v6)
        }
    }
}

fn is_tailscale_ipv4(v4: &std::net::Ipv4Addr) -> bool {
    let octets = v4.octets();
    octets[0] == 100 && (octets[1] >= 64 && octets[1] <= 127)
}

fn is_tailscale_ipv6(v6: &std::net::Ipv6Addr) -> bool {
    let segs = v6.segments();
    segs[0] == 0xfd7a && segs[1] == 0x115c && segs[2] == 0xa1e0
}

fn is_docker_ipv4(v4: &std::net::Ipv4Addr) -> bool {
    let octets = v4.octets();
    octets[0] == 172 && octets[1] == 17
}

fn is_unscoped_link_local_ipv6(v6: &std::net::Ipv6Addr) -> bool {
    (v6.segments()[0] & 0xffc0) == 0xfe80
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn scope_resolved_addresses_passes_through_non_link_local() {
        let v4 = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));
        let global_v6 = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1));
        assert_eq!(
            scope_resolved_addresses(&[v4, global_v6]),
            vec![(v4, None), (global_v6, None)]
        );
    }

    #[test]
    fn scope_resolved_addresses_scopes_link_local_with_local_interfaces() {
        let link_local = IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1));
        let scoped = scope_resolved_addresses(&[link_local]);
        assert!(!scoped.is_empty());
        // Every entry must be the same address; a host with IPv6 link-local interfaces yields at
        // least one positive scope, which is what makes such a peer selectable at all.
        assert!(scoped.iter().all(|&(ip, _)| ip == link_local));
        let expected_scopes = link_local_ipv6_interface_indices();
        if expected_scopes.is_empty() {
            assert_eq!(scoped, vec![(link_local, None)]);
        } else {
            assert_eq!(
                scoped,
                expected_scopes
                    .iter()
                    .map(|&idx| (link_local, Some(idx)))
                    .collect::<Vec<_>>()
            );
            assert!(super::super::choose_preferred_endpoint(&scoped).is_some());
        }
    }
}
