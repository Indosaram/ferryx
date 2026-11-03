pub use super::{
    choose_and_format_address, choose_preferred_endpoint, choose_preferred_ip_with_scope,
    decide_service_state_action, ServiceStateAction,
};
use super::{DiscoveredHost, DiscoveryError, DiscoveryTracker};
use std::{
    collections::{HashMap, HashSet},
    ffi::{CStr, CString},
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    os::unix::{io::AsRawFd, net::UnixStream},
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

type DNSServiceRef = *mut libc::c_void;
type DNSServiceFlags = u32;
type DNSServiceErrorType = i32;

const K_DNS_SERVICE_ERR_NO_ERROR: DNSServiceErrorType = 0;
const K_DNS_SERVICE_ERR_POLICY_DENIED: DNSServiceErrorType = -65570;
const K_DNS_SERVICE_ERR_NAME_CONFLICT: DNSServiceErrorType = -65548;
const K_DNS_SERVICE_FLAGS_ADD: DNSServiceFlags = 0x2;
const K_DNS_SERVICE_INTERFACE_INDEX_ANY: u32 = 0;
const K_DNS_SERVICE_INTERFACE_INDEX_LOCAL_ONLY: u32 = !0u32;
/// Lowest reserved interface-index sentinel: mDNSResponder uses `-1` (LocalOnly), `-2` (Unicast),
/// `-3` (P2P) and `-4` (BLE); none of them names a real system interface.
const K_DNS_SERVICE_INTERFACE_INDEX_SENTINEL_MIN: u32 = !0u32 - 3;
const K_DNS_SERVICE_MAX_DOMAIN_NAME: usize = 1009;
/// A DNS-SD TXT attribute string is at most 255 bytes, and the advertised name entry is prefixed
/// with `"name="`, so the name itself must fit in the remaining bytes.
const MAX_ADVERTISED_NAME_LEN: usize = 255 - "name=".len();
const MAX_CONCURRENT_SERVICES: usize = 64;
/// A browsed service that never produces both a hosttarget and at least one address within this
/// window is torn down so unresponsive advertisers cannot occupy the bounded service table.
const UNRESOLVED_SERVICE_TTL: Duration = Duration::from_secs(30);

/// True when poll() reports the descriptor as hung up, errored or invalid; such a descriptor stays
/// permanently "ready" and must be torn down instead of being processed again.
fn revents_indicate_hangup(revents: libc::c_short) -> bool {
    (revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL)) != 0
}

/// Resolves the IPv6 scope id to attach to an address reported by mDNSResponder. The socket's own
/// scope wins; otherwise the callback's interface index is used, but only when it names a real
/// interface: index `0` means "any" and the reserved sentinels (LocalOnly and friends) would format
/// as an unconnectable zone such as `fe80::1%4294967295`.
fn effective_ipv6_scope(sin6_scope_id: u32, interface_index: u32) -> Option<u32> {
    if sin6_scope_id != 0 {
        return Some(sin6_scope_id);
    }
    if interface_index == K_DNS_SERVICE_INTERFACE_INDEX_ANY
        || interface_index >= K_DNS_SERVICE_INTERFACE_INDEX_SENTINEL_MIN
    {
        return None;
    }
    Some(interface_index)
}

/// True when a browsed service has not resolved to a usable hosttarget plus address within
/// `UNRESOLVED_SERVICE_TTL`.
fn unresolved_service_expired(
    hosttarget: Option<&str>,
    addresses: &[(IpAddr, Option<u32>)],
    age: Duration,
) -> bool {
    let resolved = hosttarget.is_some() && !addresses.is_empty();
    !resolved && age >= UNRESOLVED_SERVICE_TTL
}

const REGTYPE_C_STR: &CStr = c"_maho-rd._tcp";

extern "C" {
    fn DNSServiceBrowse(
        sdRef: *mut DNSServiceRef,
        flags: DNSServiceFlags,
        interfaceIndex: u32,
        regtype: *const libc::c_char,
        domain: *const libc::c_char,
        callBack: Option<
            unsafe extern "C" fn(
                DNSServiceRef,
                DNSServiceFlags,
                u32,
                DNSServiceErrorType,
                *const libc::c_char,
                *const libc::c_char,
                *const libc::c_char,
                *mut libc::c_void,
            ),
        >,
        context: *mut libc::c_void,
    ) -> DNSServiceErrorType;

    fn DNSServiceResolve(
        sdRef: *mut DNSServiceRef,
        flags: DNSServiceFlags,
        interfaceIndex: u32,
        serviceName: *const libc::c_char,
        regtype: *const libc::c_char,
        replyDomain: *const libc::c_char,
        callBack: Option<
            unsafe extern "C" fn(
                DNSServiceRef,
                DNSServiceFlags,
                u32,
                DNSServiceErrorType,
                *const libc::c_char,
                *const libc::c_char,
                u16,
                u16,
                *const u8,
                *mut libc::c_void,
            ),
        >,
        context: *mut libc::c_void,
    ) -> DNSServiceErrorType;

    fn DNSServiceGetAddrInfo(
        sdRef: *mut DNSServiceRef,
        flags: DNSServiceFlags,
        interfaceIndex: u32,
        protocol: DNSServiceFlags,
        hostname: *const libc::c_char,
        callBack: Option<
            unsafe extern "C" fn(
                DNSServiceRef,
                DNSServiceFlags,
                u32,
                DNSServiceErrorType,
                *const libc::c_char,
                *const libc::sockaddr,
                u32,
                *mut libc::c_void,
            ),
        >,
        context: *mut libc::c_void,
    ) -> DNSServiceErrorType;

    fn DNSServiceRegister(
        sdRef: *mut DNSServiceRef,
        flags: DNSServiceFlags,
        interfaceIndex: u32,
        name: *const libc::c_char,
        regtype: *const libc::c_char,
        domain: *const libc::c_char,
        host: *const libc::c_char,
        port: u16,
        txtLen: u16,
        txtRecord: *const libc::c_void,
        callBack: Option<
            unsafe extern "C" fn(
                DNSServiceRef,
                DNSServiceFlags,
                DNSServiceErrorType,
                *const libc::c_char,
                *const libc::c_char,
                *const libc::c_char,
                *mut libc::c_void,
            ),
        >,
        context: *mut libc::c_void,
    ) -> DNSServiceErrorType;

    fn DNSServiceConstructFullName(
        fullname: *mut libc::c_char,
        service: *const libc::c_char,
        regtype: *const libc::c_char,
        domain: *const libc::c_char,
    ) -> DNSServiceErrorType;

    fn DNSServiceProcessResult(sdRef: DNSServiceRef) -> DNSServiceErrorType;
    fn DNSServiceRefSockFD(sdRef: DNSServiceRef) -> libc::c_int;
    fn DNSServiceRefDeallocate(sdRef: DNSServiceRef);
}

pub fn construct_full_name(service: &str, regtype: &str, domain: &str) -> Option<String> {
    let c_service = CString::new(service).ok()?;
    let c_regtype = CString::new(regtype).ok()?;
    let c_domain = CString::new(domain).ok()?;
    let mut buf = vec![0 as libc::c_char; K_DNS_SERVICE_MAX_DOMAIN_NAME];

    // SAFETY: buf is allocated to K_DNS_SERVICE_MAX_DOMAIN_NAME bytes, and CString pointers are valid.
    let err = unsafe {
        DNSServiceConstructFullName(
            buf.as_mut_ptr(),
            c_service.as_ptr(),
            c_regtype.as_ptr(),
            c_domain.as_ptr(),
        )
    };

    if err == K_DNS_SERVICE_ERR_NO_ERROR {
        // SAFETY: DNSServiceConstructFullName writes a null-terminated C string on success.
        let c_str = unsafe { CStr::from_ptr(buf.as_ptr()) };
        Some(c_str.to_string_lossy().into_owned())
    } else {
        None
    }
}

pub fn lookup_interface_index_for_ip(target: IpAddr) -> Option<u32> {
    let mut ifaddrs: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: libc::getifaddrs allocates a linked list of network interfaces.
    if unsafe { libc::getifaddrs(&mut ifaddrs) } != 0 || ifaddrs.is_null() {
        return None;
    }

    let mut current = ifaddrs;
    let mut found_index = None;

    while !current.is_null() {
        // SAFETY: current is non-null and traversed according to libc::ifaddrs linked list layout.
        unsafe {
            let ifa = &*current;
            if !ifa.ifa_addr.is_null() {
                let family = (*ifa.ifa_addr).sa_family as libc::c_int;
                let matches = match (family, target) {
                    (libc::AF_INET, IpAddr::V4(v4)) => {
                        let in_addr = ifa.ifa_addr as *const libc::sockaddr_in;
                        let ip = Ipv4Addr::from((*in_addr).sin_addr.s_addr.to_ne_bytes());
                        ip == v4
                    }
                    (libc::AF_INET6, IpAddr::V6(v6)) => {
                        let in6_addr = ifa.ifa_addr as *const libc::sockaddr_in6;
                        let ip = Ipv6Addr::from((*in6_addr).sin6_addr.s6_addr);
                        ip == v6
                    }
                    _ => false,
                };
                if matches && !ifa.ifa_name.is_null() {
                    let idx = libc::if_nametoindex(ifa.ifa_name);
                    if idx > 0 {
                        found_index = Some(idx);
                        break;
                    }
                }
            }
            current = ifa.ifa_next;
        }
    }

    // SAFETY: ifaddrs was allocated by getifaddrs and must be freed with freeifaddrs.
    unsafe { libc::freeifaddrs(ifaddrs) };
    found_index
}

pub fn enumerate_allowed_physical_interfaces() -> Result<Vec<u32>, DiscoveryError> {
    let mut ifaddrs: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: libc::getifaddrs allocates a linked list of network interfaces.
    if unsafe { libc::getifaddrs(&mut ifaddrs) } != 0 || ifaddrs.is_null() {
        return Err(DiscoveryError::Backend("getifaddrs failed".into()));
    }

    let mut current = ifaddrs;
    let mut indices = Vec::new();
    let mut seen = HashSet::new();

    while !current.is_null() {
        // SAFETY: current is non-null and traversed according to libc::ifaddrs linked list layout.
        unsafe {
            let ifa = &*current;
            let flags = ifa.ifa_flags as libc::c_int;
            let is_up = (flags & libc::IFF_UP) != 0;
            let is_running = (flags & libc::IFF_RUNNING) != 0;
            let is_loopback = (flags & libc::IFF_LOOPBACK) != 0;

            if is_up
                && is_running
                && !is_loopback
                && !ifa.ifa_name.is_null()
                && !ifa.ifa_addr.is_null()
            {
                let name = CStr::from_ptr(ifa.ifa_name).to_string_lossy();
                let is_vpn_or_virtual = name.starts_with("utun")
                    || name.starts_with("tun")
                    || name.starts_with("tap")
                    || name.starts_with("ppp")
                    || name.starts_with("ipsec")
                    || name.starts_with("bridge")
                    || name.starts_with("docker")
                    || name.starts_with("vboxnet");

                if !is_vpn_or_virtual {
                    let idx = libc::if_nametoindex(ifa.ifa_name);
                    if idx > 0 && seen.insert(idx) {
                        indices.push(idx);
                    }
                }
            }
            current = ifa.ifa_next;
        }
    }

    // SAFETY: ifaddrs was allocated by getifaddrs and must be freed with freeifaddrs.
    unsafe { libc::freeifaddrs(ifaddrs) };

    if indices.is_empty() {
        Err(DiscoveryError::Backend(
            "no active physical LAN interfaces found".into(),
        ))
    } else {
        Ok(indices)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ServiceKey {
    service_name: String,
    regtype: String,
    domain: String,
    interface_index: u32,
}

struct ServiceContext {
    fullname: String,
    hosttarget: Option<String>,
    srv_port: u16,
    txt_items: Vec<(String, Vec<u8>)>,
    addresses: Vec<(IpAddr, Option<u32>)>,
    state_dirty: bool,
    error: Option<DiscoveryError>,
}

struct ActiveService {
    resolve_ref: Option<DNSServiceRef>,
    addr_ref: Option<DNSServiceRef>,
    context: *mut ServiceContext,
    created_at: Instant,
}

struct BrowseContext {
    pending_ops: Vec<BrowseOp>,
    error: Option<DiscoveryError>,
}

enum BrowseOp {
    Added { key: ServiceKey, fullname: String },
    Removed { key: ServiceKey },
}

unsafe extern "C" fn browse_callback(
    _sd_ref: DNSServiceRef,
    flags: DNSServiceFlags,
    interface_index: u32,
    error_code: DNSServiceErrorType,
    service_name: *const libc::c_char,
    regtype: *const libc::c_char,
    domain: *const libc::c_char,
    context: *mut libc::c_void,
) {
    let catch_res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if context.is_null() {
            return;
        }
        // SAFETY: context was allocated as Box<BrowseContext> and pinned for the lifetime of browse_ref on this worker thread.
        let ctx = unsafe { &mut *(context as *mut BrowseContext) };

        if error_code == K_DNS_SERVICE_ERR_POLICY_DENIED {
            ctx.error = Some(DiscoveryError::PolicyDenied);
            return;
        }
        if error_code != K_DNS_SERVICE_ERR_NO_ERROR {
            ctx.error = Some(DiscoveryError::Backend(format!(
                "DNSServiceBrowse failed: {error_code}"
            )));
            return;
        }

        if service_name.is_null() || regtype.is_null() || domain.is_null() {
            return;
        }

        // SAFETY: Pointers are verified non-null and provided by the system mDNSResponder daemon callback.
        let s_name = unsafe { CStr::from_ptr(service_name) }
            .to_string_lossy()
            .into_owned();
        let r_type = unsafe { CStr::from_ptr(regtype) }
            .to_string_lossy()
            .into_owned();
        let d_name = unsafe { CStr::from_ptr(domain) }
            .to_string_lossy()
            .into_owned();

        let key = ServiceKey {
            service_name: s_name.clone(),
            regtype: r_type.clone(),
            domain: d_name.clone(),
            interface_index,
        };

        let is_add = (flags & K_DNS_SERVICE_FLAGS_ADD) != 0;
        if is_add {
            if let Some(fullname) = construct_full_name(&s_name, &r_type, &d_name) {
                ctx.pending_ops.push(BrowseOp::Added { key, fullname });
            }
        } else {
            ctx.pending_ops.push(BrowseOp::Removed { key });
        }
    }));

    if catch_res.is_err() && !context.is_null() {
        // SAFETY: context is verified non-null and points to BrowseContext on this worker thread.
        let ctx = unsafe { &mut *(context as *mut BrowseContext) };
        ctx.error = Some(DiscoveryError::Backend("browse callback panicked".into()));
    }
}

unsafe extern "C" fn resolve_callback(
    _sd_ref: DNSServiceRef,
    _flags: DNSServiceFlags,
    _if_index: u32,
    error_code: DNSServiceErrorType,
    _fullname: *const libc::c_char,
    hosttarget: *const libc::c_char,
    port: u16,
    txt_len: u16,
    txt_record: *const u8,
    context: *mut libc::c_void,
) {
    let catch_res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if context.is_null() {
            return;
        }
        // SAFETY: context points to heap-pinned ServiceContext owned by ActiveService on this worker thread.
        let ctx = unsafe { &mut *(context as *mut ServiceContext) };

        if error_code == K_DNS_SERVICE_ERR_POLICY_DENIED {
            ctx.error = Some(DiscoveryError::PolicyDenied);
            return;
        }
        if error_code != K_DNS_SERVICE_ERR_NO_ERROR {
            ctx.error = Some(DiscoveryError::Backend(format!(
                "DNSServiceResolve failed: {error_code}"
            )));
            return;
        }

        if !hosttarget.is_null() {
            // SAFETY: hosttarget is non-null and provided by the resolve callback.
            let target = unsafe { CStr::from_ptr(hosttarget) }
                .to_string_lossy()
                .into_owned();
            ctx.hosttarget = Some(target);
        }

        ctx.srv_port = u16::from_be(port);

        let mut txt_items = Vec::new();
        if !txt_record.is_null() && txt_len > 0 {
            // SAFETY: txt_record is verified non-null with valid txt_len bounds.
            let slice = unsafe { std::slice::from_raw_parts(txt_record, txt_len as usize) };
            let mut cursor = 0;
            while cursor < slice.len() {
                let len = slice[cursor] as usize;
                cursor += 1;
                if cursor + len > slice.len() {
                    break;
                }
                let item = &slice[cursor..cursor + len];
                cursor += len;
                if let Some(pos) = item.iter().position(|&b| b == b'=') {
                    if let Ok(key) = std::str::from_utf8(&item[..pos]) {
                        let val = item[pos + 1..].to_vec();
                        txt_items.push((key.to_string(), val));
                    }
                }
            }
        }
        ctx.txt_items = txt_items;
        ctx.state_dirty = true;
    }));

    if catch_res.is_err() && !context.is_null() {
        // SAFETY: context is verified non-null and points to ServiceContext on this worker thread.
        let ctx = unsafe { &mut *(context as *mut ServiceContext) };
        ctx.error = Some(DiscoveryError::Backend("resolve callback panicked".into()));
    }
}

unsafe extern "C" fn addr_callback(
    _sd_ref: DNSServiceRef,
    flags: DNSServiceFlags,
    interface_index: u32,
    error_code: DNSServiceErrorType,
    _hostname: *const libc::c_char,
    address: *const libc::sockaddr,
    _ttl: u32,
    context: *mut libc::c_void,
) {
    let catch_res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if context.is_null() {
            return;
        }
        // SAFETY: context points to heap-pinned ServiceContext owned by ActiveService on this worker thread.
        let ctx = unsafe { &mut *(context as *mut ServiceContext) };

        if error_code == K_DNS_SERVICE_ERR_POLICY_DENIED {
            ctx.error = Some(DiscoveryError::PolicyDenied);
            return;
        }
        if error_code != K_DNS_SERVICE_ERR_NO_ERROR || address.is_null() {
            return;
        }

        // SAFETY: address is non-null and valid sockaddr pointer passed by mDNSResponder.
        let (parsed_ip, scope_id) = unsafe {
            let family = (*address).sa_family as libc::c_int;
            if family == libc::AF_INET {
                let in_addr = address as *const libc::sockaddr_in;
                let ip = Ipv4Addr::from((*in_addr).sin_addr.s_addr.to_ne_bytes());
                (Some(IpAddr::V4(ip)), None)
            } else if family == libc::AF_INET6 {
                let in6_addr = address as *const libc::sockaddr_in6;
                let ip = Ipv6Addr::from((*in6_addr).sin6_addr.s6_addr);
                let scope = (*in6_addr).sin6_scope_id;
                (
                    Some(IpAddr::V6(ip)),
                    effective_ipv6_scope(scope, interface_index),
                )
            } else {
                (None, None)
            }
        };

        if let Some(ip) = parsed_ip {
            let is_add = (flags & K_DNS_SERVICE_FLAGS_ADD) != 0;
            if is_add {
                if !ctx.addresses.iter().any(|(existing, _)| *existing == ip) {
                    ctx.addresses.push((ip, scope_id));
                    ctx.state_dirty = true;
                }
            } else {
                let prev_len = ctx.addresses.len();
                ctx.addresses.retain(|(existing, _)| *existing != ip);
                if ctx.addresses.len() != prev_len {
                    ctx.state_dirty = true;
                }
            }
        }
    }));

    if catch_res.is_err() && !context.is_null() {
        // SAFETY: context is verified non-null and points to ServiceContext on this worker thread.
        let ctx = unsafe { &mut *(context as *mut ServiceContext) };
        ctx.error = Some(DiscoveryError::Backend("addr callback panicked".into()));
    }
}

pub struct AppleDnsServiceBrowser {
    tracker: Arc<Mutex<DiscoveryTracker>>,
    wake_tx: UnixStream,
    worker_handle: Option<thread::JoinHandle<()>>,
    last_error: Arc<Mutex<Option<DiscoveryError>>>,
}

impl AppleDnsServiceBrowser {
    pub fn new() -> Result<Self, DiscoveryError> {
        let (wake_tx, wake_rx) = UnixStream::pair()
            .map_err(|e| DiscoveryError::Backend(format!("UnixStream::pair failed: {e}")))?;
        wake_tx
            .set_nonblocking(true)
            .map_err(|e| DiscoveryError::Backend(format!("set_nonblocking failed: {e}")))?;
        wake_rx
            .set_nonblocking(true)
            .map_err(|e| DiscoveryError::Backend(format!("set_nonblocking failed: {e}")))?;

        let tracker = Arc::new(Mutex::new(DiscoveryTracker::new()));
        let last_error = Arc::new(Mutex::new(None));

        let tracker_clone = Arc::clone(&tracker);
        let err_clone = Arc::clone(&last_error);

        let worker_handle = thread::Builder::new()
            .name("maho-dnssd-worker".into())
            .spawn(move || {
                let mut browse_ref: DNSServiceRef = std::ptr::null_mut();

                let browse_ctx_box = Box::new(BrowseContext {
                    pending_ops: Vec::new(),
                    error: None,
                });
                let browse_ctx_ptr = Box::into_raw(browse_ctx_box);

                // SAFETY: browse_ref is a valid mutable pointer, REGTYPE_C_STR is valid null-terminated, and browse_ctx_ptr is heap-pinned.
                let err = unsafe {
                    DNSServiceBrowse(
                        &mut browse_ref,
                        0,
                        0,
                        REGTYPE_C_STR.as_ptr(),
                        std::ptr::null(),
                        Some(browse_callback),
                        browse_ctx_ptr as *mut libc::c_void,
                    )
                };

                if err == K_DNS_SERVICE_ERR_POLICY_DENIED {
                    let mut guard = err_clone.lock().unwrap_or_else(|e| e.into_inner());
                    *guard = Some(DiscoveryError::PolicyDenied);
                    // SAFETY: browse_ctx_ptr was allocated via Box::into_raw and is reclaimed before thread exit.
                    unsafe { drop(Box::from_raw(browse_ctx_ptr)) };
                    return;
                }
                if err != K_DNS_SERVICE_ERR_NO_ERROR {
                    let mut guard = err_clone.lock().unwrap_or_else(|e| e.into_inner());
                    *guard = Some(DiscoveryError::Backend(format!(
                        "DNSServiceBrowse failed with error: {err}"
                    )));
                    // SAFETY: browse_ctx_ptr was allocated via Box::into_raw and is reclaimed before thread exit.
                    unsafe { drop(Box::from_raw(browse_ctx_ptr)) };
                    return;
                }

                // SAFETY: browse_ref is verified non-null after successful DNSServiceBrowse call.
                let browse_sock = unsafe { DNSServiceRefSockFD(browse_ref) };
                let wake_sock = wake_rx.as_raw_fd();

                let mut active_services: HashMap<ServiceKey, ActiveService> = HashMap::new();

                'event_loop: loop {
                    // SAFETY: browse_ctx_ptr remains pinned on this single worker thread.
                    let bctx = unsafe { &mut *browse_ctx_ptr };
                    if let Some(err) = bctx.error.take() {
                        let mut guard = err_clone.lock().unwrap_or_else(|e| e.into_inner());
                        *guard = Some(err);
                    }

                    let new_ops = std::mem::take(&mut bctx.pending_ops);
                    for op in new_ops {
                        match op {
                            BrowseOp::Removed { key } => {
                                if let Some(mut service) = active_services.remove(&key) {
                                    // SAFETY: service.context is pinned on this worker thread and valid.
                                    let removed_fullname =
                                        unsafe { &(*service.context).fullname }.clone();

                                    // SAFETY: Deallocate resolve and addr handles before freeing boxed context.
                                    if let Some(addr_ref) = service.addr_ref.take() {
                                        unsafe { DNSServiceRefDeallocate(addr_ref) };
                                    }
                                    if let Some(resolve_ref) = service.resolve_ref.take() {
                                        unsafe { DNSServiceRefDeallocate(resolve_ref) };
                                    }
                                    // SAFETY: context was allocated via Box::into_raw and handles referencing it are deallocated.
                                    unsafe { drop(Box::from_raw(service.context)) };

                                    let surviving_host =
                                        active_services.values().find_map(|other| {
                                            // SAFETY: other.context is valid and pinned on this worker thread.
                                            let other_ctx = unsafe { &*other.context };
                                            if other_ctx.fullname == removed_fullname {
                                                match decide_service_state_action(
                                                    &other_ctx.fullname,
                                                    other_ctx.hosttarget.as_deref(),
                                                    other_ctx.srv_port,
                                                    &other_ctx.txt_items,
                                                    &other_ctx.addresses,
                                                ) {
                                                    ServiceStateAction::Publish(host) => Some(host),
                                                    ServiceStateAction::Retract => None,
                                                }
                                            } else {
                                                None
                                            }
                                        });

                                    let mut tracker_guard =
                                        tracker_clone.lock().unwrap_or_else(|e| e.into_inner());
                                    if let Some(surviving) = surviving_host {
                                        tracker_guard.upsert(surviving);
                                    } else {
                                        tracker_guard.remove(&removed_fullname);
                                    }
                                }
                            }
                            BrowseOp::Added { key, fullname } => {
                                if active_services.len() >= MAX_CONCURRENT_SERVICES {
                                    continue;
                                }
                                if active_services.contains_key(&key) {
                                    continue;
                                }

                                let c_sname = match CString::new(key.service_name.as_str()) {
                                    Ok(s) => s,
                                    Err(_) => continue,
                                };
                                let c_rtype = match CString::new(key.regtype.as_str()) {
                                    Ok(s) => s,
                                    Err(_) => continue,
                                };
                                let c_domain = match CString::new(key.domain.as_str()) {
                                    Ok(s) => s,
                                    Err(_) => continue,
                                };

                                let s_ctx_box = Box::new(ServiceContext {
                                    fullname,
                                    hosttarget: None,
                                    srv_port: 0,
                                    txt_items: Vec::new(),
                                    addresses: Vec::new(),
                                    state_dirty: false,
                                    error: None,
                                });
                                let s_ctx_ptr = Box::into_raw(s_ctx_box);
                                let mut resolve_ref: DNSServiceRef = std::ptr::null_mut();

                                // SAFETY: resolve_ref is valid mutable pointer, c-strings are null-terminated, and s_ctx_ptr is heap-pinned.
                                let r_err = unsafe {
                                    DNSServiceResolve(
                                        &mut resolve_ref,
                                        0,
                                        key.interface_index,
                                        c_sname.as_ptr(),
                                        c_rtype.as_ptr(),
                                        c_domain.as_ptr(),
                                        Some(resolve_callback),
                                        s_ctx_ptr as *mut libc::c_void,
                                    )
                                };

                                if r_err == K_DNS_SERVICE_ERR_NO_ERROR && !resolve_ref.is_null() {
                                    active_services.insert(
                                        key,
                                        ActiveService {
                                            resolve_ref: Some(resolve_ref),
                                            addr_ref: None,
                                            context: s_ctx_ptr,
                                            created_at: Instant::now(),
                                        },
                                    );
                                } else {
                                    // SAFETY: resolve failed; clean up heap-pinned context immediately.
                                    unsafe { drop(Box::from_raw(s_ctx_ptr)) };
                                }
                            }
                        }
                    }

                    // Bounded lifetime for services that never resolve: tear them down so they cannot
                    // occupy slots in the MAX_CONCURRENT_SERVICES table forever.
                    let expired_keys: Vec<ServiceKey> = active_services
                        .iter()
                        .filter(|(_, service)| {
                            // SAFETY: service.context is valid and pinned on this worker thread.
                            let s_ctx = unsafe { &*service.context };
                            unresolved_service_expired(
                                s_ctx.hosttarget.as_deref(),
                                &s_ctx.addresses,
                                service.created_at.elapsed(),
                            )
                        })
                        .map(|(key, _)| key.clone())
                        .collect();
                    for key in expired_keys {
                        if let Some(mut service) = active_services.remove(&key) {
                            // SAFETY: Deallocate handles before freeing the heap-pinned context.
                            if let Some(addr_ref) = service.addr_ref.take() {
                                unsafe { DNSServiceRefDeallocate(addr_ref) };
                            }
                            if let Some(resolve_ref) = service.resolve_ref.take() {
                                unsafe { DNSServiceRefDeallocate(resolve_ref) };
                            }
                            // SAFETY: context was allocated via Box::into_raw and all handles are deallocated.
                            let s_ctx = unsafe { Box::from_raw(service.context) };
                            tracing::debug!(
                                "dropping unresolved mDNS service after timeout: {}",
                                s_ctx.fullname
                            );
                        }
                    }

                    let mut pending_tracker_ops: Vec<(String, Option<DiscoveredHost>)> = Vec::new();
                    for (key, service) in active_services.iter_mut() {
                        // SAFETY: service.context is valid and pinned on this worker thread.
                        let s_ctx = unsafe { &mut *service.context };
                        if let Some(err) = s_ctx.error.take() {
                            // Per-service resolve failures are local to one advertiser; logging and
                            // discarding keeps one broken third-party service from poisoning the
                            // browser-wide last_error for the process lifetime.
                            tracing::warn!(
                                "discarding per-service discovery error for {}: {err}",
                                s_ctx.fullname
                            );
                            pending_tracker_ops.push((s_ctx.fullname.clone(), None));
                        }

                        if service.addr_ref.is_none() {
                            if let Some(ref target) = s_ctx.hosttarget {
                                if let Ok(c_target) = CString::new(target.as_str()) {
                                    let mut addr_ref: DNSServiceRef = std::ptr::null_mut();
                                    // SAFETY: addr_ref is valid mutable pointer, c_target is valid, and s_ctx is heap-pinned.
                                    let a_err = unsafe {
                                        DNSServiceGetAddrInfo(
                                            &mut addr_ref,
                                            0,
                                            key.interface_index,
                                            0,
                                            c_target.as_ptr(),
                                            Some(addr_callback),
                                            service.context as *mut libc::c_void,
                                        )
                                    };
                                    if a_err == K_DNS_SERVICE_ERR_NO_ERROR && !addr_ref.is_null() {
                                        service.addr_ref = Some(addr_ref);
                                    }
                                }
                            }
                        }

                        if s_ctx.state_dirty {
                            s_ctx.state_dirty = false;
                            let action = decide_service_state_action(
                                &s_ctx.fullname,
                                s_ctx.hosttarget.as_deref(),
                                s_ctx.srv_port,
                                &s_ctx.txt_items,
                                &s_ctx.addresses,
                            );
                            match action {
                                ServiceStateAction::Publish(host) => {
                                    pending_tracker_ops.push((s_ctx.fullname.clone(), Some(host)));
                                }
                                ServiceStateAction::Retract => {
                                    pending_tracker_ops.push((s_ctx.fullname.clone(), None));
                                }
                            }
                        }
                    }

                    for (fullname, publish) in pending_tracker_ops {
                        let resolved = match publish {
                            Some(host) => Some(host),
                            None => {
                                // A retraction on one interface must not erase a host that is still
                                // live on another interface.
                                active_services.values().find_map(|other| {
                                    // SAFETY: other.context is valid and pinned on this worker thread.
                                    let other_ctx = unsafe { &*other.context };
                                    if other_ctx.fullname == fullname {
                                        match decide_service_state_action(
                                            &other_ctx.fullname,
                                            other_ctx.hosttarget.as_deref(),
                                            other_ctx.srv_port,
                                            &other_ctx.txt_items,
                                            &other_ctx.addresses,
                                        ) {
                                            ServiceStateAction::Publish(host) => Some(host),
                                            ServiceStateAction::Retract => None,
                                        }
                                    } else {
                                        None
                                    }
                                })
                            }
                        };
                        let mut tracker_guard =
                            tracker_clone.lock().unwrap_or_else(|e| e.into_inner());
                        match resolved {
                            Some(host) => tracker_guard.upsert(host),
                            None => tracker_guard.remove(&fullname),
                        }
                    }

                    let mut poll_fds = Vec::with_capacity(2 + active_services.len() * 2);
                    poll_fds.push(libc::pollfd {
                        fd: wake_sock,
                        events: libc::POLLIN,
                        revents: 0,
                    });
                    poll_fds.push(libc::pollfd {
                        fd: browse_sock,
                        events: libc::POLLIN,
                        revents: 0,
                    });

                    let mut dispatch_handles = Vec::with_capacity(active_services.len() * 2);
                    for service in active_services.values() {
                        if let Some(r_ref) = service.resolve_ref {
                            // SAFETY: r_ref is valid and owned by active_services on this worker thread.
                            let fd = unsafe { DNSServiceRefSockFD(r_ref) };
                            if fd >= 0 {
                                poll_fds.push(libc::pollfd {
                                    fd,
                                    events: libc::POLLIN,
                                    revents: 0,
                                });
                                dispatch_handles.push(r_ref);
                            }
                        }
                        if let Some(a_ref) = service.addr_ref {
                            // SAFETY: a_ref is valid and owned by active_services on this worker thread.
                            let fd = unsafe { DNSServiceRefSockFD(a_ref) };
                            if fd >= 0 {
                                poll_fds.push(libc::pollfd {
                                    fd,
                                    events: libc::POLLIN,
                                    revents: 0,
                                });
                                dispatch_handles.push(a_ref);
                            }
                        }
                    }

                    // SAFETY: poll_fds is a valid contiguous array of pollfd structs.
                    let res = unsafe {
                        libc::poll(poll_fds.as_mut_ptr(), poll_fds.len() as libc::nfds_t, 250)
                    };

                    if res < 0 {
                        let err_kind = io::Error::last_os_error();
                        if err_kind.kind() != io::ErrorKind::Interrupted {
                            let mut guard = err_clone.lock().unwrap_or_else(|e| e.into_inner());
                            *guard =
                                Some(DiscoveryError::Backend(format!("poll error: {err_kind}")));
                            break 'event_loop;
                        }
                        continue;
                    }

                    if poll_fds[0].revents
                        & (libc::POLLIN | libc::POLLHUP | libc::POLLERR | libc::POLLNVAL)
                        != 0
                    {
                        break 'event_loop;
                    }

                    if revents_indicate_hangup(poll_fds[1].revents) {
                        // The browse socket hung up: mDNSResponder restarted or closed the query.
                        // Reporting readiness forever would spin, so treat it as a fatal browser error.
                        let mut guard = err_clone.lock().unwrap_or_else(|e| e.into_inner());
                        *guard = Some(DiscoveryError::Backend("mDNS browse socket hung up".into()));
                        let mut tracker_guard =
                            tracker_clone.lock().unwrap_or_else(|e| e.into_inner());
                        for svc in active_services.values() {
                            // SAFETY: svc.context is valid on this worker thread.
                            let fname = unsafe { &(*svc.context).fullname };
                            tracker_guard.remove(fname);
                        }
                        break 'event_loop;
                    }

                    if poll_fds[1].revents & libc::POLLIN != 0 {
                        // SAFETY: browse_ref is valid and polled readable.
                        let proc_err = unsafe { DNSServiceProcessResult(browse_ref) };
                        if proc_err != K_DNS_SERVICE_ERR_NO_ERROR {
                            let mut guard = err_clone.lock().unwrap_or_else(|e| e.into_inner());
                            if proc_err == K_DNS_SERVICE_ERR_POLICY_DENIED {
                                *guard = Some(DiscoveryError::PolicyDenied);
                            } else {
                                *guard = Some(DiscoveryError::Backend(format!(
                                    "ProcessResult browse error: {proc_err}"
                                )));
                            }
                            let mut tracker_guard =
                                tracker_clone.lock().unwrap_or_else(|e| e.into_inner());
                            for svc in active_services.values() {
                                // SAFETY: svc.context is valid on this worker thread.
                                let fname = unsafe { &(*svc.context).fullname };
                                tracker_guard.remove(fname);
                            }
                            break 'event_loop;
                        }
                        // Browser-level recovery: a successful browse round with no callback error
                        // clears any previously recorded transient browser error. A policy denial is
                        // terminal for this process and is never cleared.
                        // SAFETY: browse_ctx_ptr remains pinned on this single worker thread.
                        if unsafe { (*browse_ctx_ptr).error.is_none() } {
                            let mut guard = err_clone.lock().unwrap_or_else(|e| e.into_inner());
                            if !matches!(*guard, Some(DiscoveryError::PolicyDenied)) {
                                *guard = None;
                            }
                        }
                    }

                    // Handles whose socket hung up or whose ProcessResult failed must be torn down;
                    // otherwise poll() keeps reporting them ready and the loop spins.
                    let mut dead_handles: Vec<DNSServiceRef> = Vec::new();
                    for (idx, &handle) in dispatch_handles.iter().enumerate() {
                        let pfd_idx = 2 + idx;
                        if pfd_idx >= poll_fds.len() {
                            continue;
                        }
                        let revents = poll_fds[pfd_idx].revents;
                        if revents_indicate_hangup(revents) {
                            dead_handles.push(handle);
                            continue;
                        }
                        if revents & libc::POLLIN != 0 {
                            // SAFETY: handle is verified non-null and polled readable.
                            let proc_err = unsafe { DNSServiceProcessResult(handle) };
                            if proc_err != K_DNS_SERVICE_ERR_NO_ERROR {
                                tracing::warn!(
                                    "DNSServiceProcessResult failed for service handle: {proc_err}"
                                );
                                dead_handles.push(handle);
                            }
                        }
                    }

                    for dead in dead_handles {
                        for service in active_services.values_mut() {
                            if service.resolve_ref == Some(dead) {
                                service.resolve_ref = None;
                            }
                            if service.addr_ref == Some(dead) {
                                service.addr_ref = None;
                            }
                        }
                        // SAFETY: dead is no longer referenced by any ActiveService and is deallocated once.
                        unsafe { DNSServiceRefDeallocate(dead) };
                    }
                }

                for mut service in active_services.into_values() {
                    // SAFETY: Deallocate active C handles before freeing the heap-pinned context.
                    if let Some(addr_ref) = service.addr_ref.take() {
                        unsafe { DNSServiceRefDeallocate(addr_ref) };
                    }
                    if let Some(resolve_ref) = service.resolve_ref.take() {
                        unsafe { DNSServiceRefDeallocate(resolve_ref) };
                    }
                    // SAFETY: context was allocated with Box::into_raw and is reclaimed once all handles are deallocated.
                    unsafe { drop(Box::from_raw(service.context)) };
                }

                // SAFETY: browse_ref is deallocated and browse_ctx_ptr reclaimed before thread exit.
                unsafe {
                    DNSServiceRefDeallocate(browse_ref);
                    drop(Box::from_raw(browse_ctx_ptr));
                }
            })
            .map_err(|e| DiscoveryError::Backend(format!("thread spawn failed: {e}")))?;

        Ok(Self {
            tracker,
            wake_tx,
            worker_handle: Some(worker_handle),
            last_error,
        })
    }

    pub fn snapshot(&self) -> Result<Vec<DiscoveredHost>, DiscoveryError> {
        let guard = self.last_error.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(err) = guard.clone() {
            return Err(err);
        }
        let tracker_guard = self.tracker.lock().unwrap_or_else(|e| e.into_inner());
        Ok(tracker_guard.snapshot())
    }
}

impl Drop for AppleDnsServiceBrowser {
    fn drop(&mut self) {
        let fd = self.wake_tx.as_raw_fd();
        // SAFETY: fd is valid and owned by self.wake_tx; MSG_NOSIGNAL prevents SIGPIPE if worker already exited.
        unsafe {
            libc::send(
                fd,
                [1u8].as_ptr() as *const libc::c_void,
                1,
                libc::MSG_NOSIGNAL,
            );
        };
        if let Some(handle) = self.worker_handle.take() {
            let _ = handle.join();
        }
    }
}

struct RegisterContext {
    error_slot: Arc<Mutex<Option<DiscoveryError>>>,
}

unsafe extern "C" fn register_callback(
    _sd_ref: DNSServiceRef,
    _flags: DNSServiceFlags,
    error_code: DNSServiceErrorType,
    _name: *const libc::c_char,
    _regtype: *const libc::c_char,
    _domain: *const libc::c_char,
    context: *mut libc::c_void,
) {
    let catch_res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if context.is_null() {
            return;
        }
        // SAFETY: context is verified non-null and points to pinned RegisterContext on the advertiser worker thread.
        let ctx = unsafe { &mut *(context as *mut RegisterContext) };
        if error_code == K_DNS_SERVICE_ERR_POLICY_DENIED {
            let mut guard = ctx.error_slot.lock().unwrap_or_else(|e| e.into_inner());
            *guard = Some(DiscoveryError::PolicyDenied);
        } else if error_code == K_DNS_SERVICE_ERR_NAME_CONFLICT {
            let mut guard = ctx.error_slot.lock().unwrap_or_else(|e| e.into_inner());
            *guard = Some(DiscoveryError::Backend("service name conflict".into()));
        } else if error_code != K_DNS_SERVICE_ERR_NO_ERROR {
            let mut guard = ctx.error_slot.lock().unwrap_or_else(|e| e.into_inner());
            *guard = Some(DiscoveryError::Backend(format!(
                "DNSServiceRegister failed: {error_code}"
            )));
        }
    }));

    if catch_res.is_err() && !context.is_null() {
        // SAFETY: context is verified non-null and points to RegisterContext on this worker thread.
        let ctx = unsafe { &mut *(context as *mut RegisterContext) };
        let mut guard = ctx.error_slot.lock().unwrap_or_else(|e| e.into_inner());
        *guard = Some(DiscoveryError::Backend("register callback panicked".into()));
    }
}

pub struct AppleDnsServiceAdvertiser {
    wake_tx: UnixStream,
    worker_handle: Option<thread::JoinHandle<()>>,
    async_error: Arc<Mutex<Option<DiscoveryError>>>,
}

impl AppleDnsServiceAdvertiser {
    pub fn start(
        name: &str,
        tcp_port: u16,
        udp_port: u16,
        bind_addr: SocketAddr,
    ) -> Result<Self, DiscoveryError> {
        // A name longer than this cannot be carried in the "name=" TXT attribute, and silently
        // dropping it would advertise a service no peer can validate.
        if name.len() > MAX_ADVERTISED_NAME_LEN {
            return Err(DiscoveryError::InvalidPayload(format!(
                "name must be at most {MAX_ADVERTISED_NAME_LEN} bytes to fit the TXT record"
            )));
        }

        let (wake_tx, wake_rx) = UnixStream::pair()
            .map_err(|e| DiscoveryError::Backend(format!("UnixStream::pair failed: {e}")))?;
        wake_tx
            .set_nonblocking(true)
            .map_err(|e| DiscoveryError::Backend(format!("set_nonblocking failed: {e}")))?;
        wake_rx
            .set_nonblocking(true)
            .map_err(|e| DiscoveryError::Backend(format!("set_nonblocking failed: {e}")))?;

        let interfaces_to_register = if bind_addr.ip().is_loopback() {
            vec![K_DNS_SERVICE_INTERFACE_INDEX_LOCAL_ONLY]
        } else if bind_addr.ip().is_unspecified() {
            // One registration on "any" interface: registering the same instance name once per
            // physical interface makes mDNSResponder treat the extra registrations as name
            // conflicts and auto-rename them to "Host (2)", "Host (3)", ...
            vec![K_DNS_SERVICE_INTERFACE_INDEX_ANY]
        } else {
            let idx = lookup_interface_index_for_ip(bind_addr.ip()).ok_or_else(|| {
                DiscoveryError::Backend(format!(
                    "no local network interface matches bind address {}",
                    bind_addr.ip()
                ))
            })?;
            vec![idx]
        };

        let (init_tx, init_rx) = mpsc::channel();
        let name_owned = name.to_string();
        let async_error = Arc::new(Mutex::new(None));
        let async_error_clone = Arc::clone(&async_error);

        let worker_handle = thread::Builder::new()
            .name("maho-dnssd-adv".into())
            .spawn(move || {
                let c_name = match CString::new(name_owned.as_str()) {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = init_tx.send(Err(DiscoveryError::InvalidPayload(e.to_string())));
                        return;
                    }
                };

                let os_name = std::env::consts::OS;
                let mut txt =
                    format!("protocol=3\0name={name_owned}\0os={os_name}\0udp_port={udp_port}");
                if let Some(mac) = super::mac::primary_mac(bind_addr.ip()) {
                    txt.push_str(&format!("\0mac={mac}"));
                }
                let mut txt_bytes = Vec::new();
                for part in txt.split('\0') {
                    if !part.is_empty() && part.len() <= 255 {
                        txt_bytes.push(part.len() as u8);
                        txt_bytes.extend_from_slice(part.as_bytes());
                    }
                }

                let reg_ctx_box = Box::new(RegisterContext {
                    error_slot: Arc::clone(&async_error_clone),
                });
                let reg_ctx_ptr = Box::into_raw(reg_ctx_box);
                let port_be = tcp_port.to_be();
                let mut registered_handles = Vec::with_capacity(interfaces_to_register.len());

                for &if_index in &interfaces_to_register {
                    let mut reg_ref: DNSServiceRef = std::ptr::null_mut();
                    // SAFETY: reg_ref is valid mutable pointer, c-strings and txt_bytes are valid, reg_ctx_ptr is heap-pinned.
                    let err = unsafe {
                        DNSServiceRegister(
                            &mut reg_ref,
                            0,
                            if_index,
                            c_name.as_ptr(),
                            REGTYPE_C_STR.as_ptr(),
                            std::ptr::null(),
                            std::ptr::null(),
                            port_be,
                            txt_bytes.len() as u16,
                            txt_bytes.as_ptr() as *const libc::c_void,
                            Some(register_callback),
                            reg_ctx_ptr as *mut libc::c_void,
                        )
                    };

                    if err != K_DNS_SERVICE_ERR_NO_ERROR {
                        for h in registered_handles {
                            // SAFETY: Deallocate previously registered handles on partial failure.
                            unsafe { DNSServiceRefDeallocate(h) };
                        }
                        // SAFETY: Registration failed; reclaim pinned context before thread exit.
                        unsafe { drop(Box::from_raw(reg_ctx_ptr)) };
                        let mapped_err = if err == K_DNS_SERVICE_ERR_POLICY_DENIED {
                            DiscoveryError::PolicyDenied
                        } else {
                            DiscoveryError::Backend(format!("DNSServiceRegister error code: {err}"))
                        };
                        let _ = init_tx.send(Err(mapped_err));
                        return;
                    }
                    registered_handles.push(reg_ref);
                }

                let _ = init_tx.send(Ok(()));
                let wake_sock = wake_rx.as_raw_fd();

                loop {
                    let mut poll_fds = Vec::with_capacity(1 + registered_handles.len());
                    poll_fds.push(libc::pollfd {
                        fd: wake_sock,
                        events: libc::POLLIN,
                        revents: 0,
                    });

                    // Handle and its pollfd slot are stored as one pair so a skipped handle
                    // (fd < 0) cannot shift the index mapping and misdispatch events.
                    let mut polled: Vec<(DNSServiceRef, usize)> =
                        Vec::with_capacity(registered_handles.len());
                    for &h in &registered_handles {
                        // SAFETY: h is valid registered DNSServiceRef handle.
                        let fd = unsafe { DNSServiceRefSockFD(h) };
                        if fd >= 0 {
                            polled.push((h, poll_fds.len()));
                            poll_fds.push(libc::pollfd {
                                fd,
                                events: libc::POLLIN,
                                revents: 0,
                            });
                        }
                    }

                    // SAFETY: poll_fds is valid contiguous slice of pollfd structs.
                    let res = unsafe { libc::poll(poll_fds.as_mut_ptr(), poll_fds.len() as libc::nfds_t, -1) };
                    if res <= 0 {
                        let err_kind = io::Error::last_os_error();
                        if err_kind.kind() != io::ErrorKind::Interrupted {
                            break;
                        }
                        continue;
                    }

                    if poll_fds[0].revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 {
                        break;
                    }

                    // A hung-up or failing registration socket is reported ready by poll() forever,
                    // so tear it down instead of spinning on it.
                    let mut dead_handles: Vec<DNSServiceRef> = Vec::new();
                    for &(h, pfd_idx) in &polled {
                        let revents = poll_fds[pfd_idx].revents;
                        if revents_indicate_hangup(revents) {
                            tracing::warn!("mDNS registration socket hung up; dropping handle");
                            dead_handles.push(h);
                            continue;
                        }
                        if revents & libc::POLLIN != 0 {
                            // SAFETY: h is valid and polled readable.
                            let proc_err = unsafe { DNSServiceProcessResult(h) };
                            if proc_err != K_DNS_SERVICE_ERR_NO_ERROR {
                                tracing::warn!(
                                    "DNSServiceProcessResult failed for registration handle: {proc_err}"
                                );
                                dead_handles.push(h);
                            }
                        }
                    }

                    if !dead_handles.is_empty() {
                        {
                            let mut guard =
                                async_error_clone.lock().unwrap_or_else(|e| e.into_inner());
                            if guard.is_none() {
                                *guard = Some(DiscoveryError::Backend(
                                    "mDNS registration handle closed unexpectedly".into(),
                                ));
                            }
                        }
                        registered_handles.retain(|h| !dead_handles.contains(h));
                        for dead in dead_handles {
                            // SAFETY: dead was removed from registered_handles and is deallocated once.
                            unsafe { DNSServiceRefDeallocate(dead) };
                        }
                    }
                }

                for h in registered_handles {
                    // SAFETY: Deallocate reg_ref handles before freeing reg_ctx_ptr.
                    unsafe { DNSServiceRefDeallocate(h) };
                }
                // SAFETY: reg_ctx_ptr was allocated with Box::into_raw and is reclaimed once all handles are deallocated.
                unsafe { drop(Box::from_raw(reg_ctx_ptr)) };
            })
            .map_err(|e| DiscoveryError::Backend(format!("thread spawn failed: {e}")))?;

        init_rx
            .recv_timeout(Duration::from_millis(500))
            .map_err(|_| DiscoveryError::Backend("advertiser registration timed out".into()))??;

        Ok(Self {
            wake_tx,
            worker_handle: Some(worker_handle),
            async_error,
        })
    }

    pub fn check_error(&self) -> Result<(), DiscoveryError> {
        let guard = self.async_error.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(err) = guard.clone() {
            Err(err)
        } else {
            Ok(())
        }
    }
}

impl Drop for AppleDnsServiceAdvertiser {
    fn drop(&mut self) {
        let fd = self.wake_tx.as_raw_fd();
        // SAFETY: fd is valid; MSG_NOSIGNAL prevents SIGPIPE if worker thread already exited.
        unsafe {
            libc::send(
                fd,
                [1u8].as_ptr() as *const libc::c_void,
                1,
                libc::MSG_NOSIGNAL,
            );
        };
        if let Some(handle) = self.worker_handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_ipv6_scope_rejects_sentinel_interface_indices() {
        // Socket scope wins whenever it is set.
        assert_eq!(effective_ipv6_scope(7, 0), Some(7));
        assert_eq!(
            effective_ipv6_scope(7, K_DNS_SERVICE_INTERFACE_INDEX_LOCAL_ONLY),
            Some(7)
        );
        // A real interface index is adopted when the socket carries no scope.
        assert_eq!(effective_ipv6_scope(0, 4), Some(4));
        // "Any" and the reserved sentinels must not become a zone id.
        assert_eq!(
            effective_ipv6_scope(0, K_DNS_SERVICE_INTERFACE_INDEX_ANY),
            None
        );
        assert_eq!(
            effective_ipv6_scope(0, K_DNS_SERVICE_INTERFACE_INDEX_LOCAL_ONLY),
            None
        );
        assert_eq!(
            effective_ipv6_scope(0, K_DNS_SERVICE_INTERFACE_INDEX_SENTINEL_MIN),
            None
        );
        assert_eq!(
            effective_ipv6_scope(0, K_DNS_SERVICE_INTERFACE_INDEX_SENTINEL_MIN - 1),
            Some(K_DNS_SERVICE_INTERFACE_INDEX_SENTINEL_MIN - 1)
        );

        // A sentinel index must never reach the formatted endpoint as "%4294967295".
        let link_local = IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1));
        let scope = effective_ipv6_scope(0, K_DNS_SERVICE_INTERFACE_INDEX_LOCAL_ONLY);
        assert!(choose_and_format_address(&[(link_local, scope)]).is_none());
    }

    #[test]
    fn advertiser_rejects_name_too_long_for_txt_attribute() {
        assert_eq!(MAX_ADVERTISED_NAME_LEN, 250);
        let too_long = "n".repeat(MAX_ADVERTISED_NAME_LEN + 1);
        let bind = SocketAddr::from((Ipv4Addr::LOCALHOST, 19730));
        let err = AppleDnsServiceAdvertiser::start(&too_long, 19730, 19731, bind)
            .err()
            .expect("over-long name must be rejected instead of silently dropping the TXT entry");
        assert!(
            matches!(err, DiscoveryError::InvalidPayload(ref m) if m.contains("250")),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn revents_indicate_hangup_detects_closed_and_invalid_fds() {
        assert!(revents_indicate_hangup(libc::POLLHUP));
        assert!(revents_indicate_hangup(libc::POLLERR));
        assert!(revents_indicate_hangup(libc::POLLNVAL));
        assert!(revents_indicate_hangup(libc::POLLIN | libc::POLLHUP));
        assert!(!revents_indicate_hangup(libc::POLLIN));
        assert!(!revents_indicate_hangup(0));
    }

    #[test]
    fn unresolved_service_expires_only_when_still_unresolved() {
        let addrs = vec![(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 50)), None)];

        assert!(unresolved_service_expired(
            None,
            &[],
            UNRESOLVED_SERVICE_TTL + Duration::from_secs(1)
        ));
        assert!(unresolved_service_expired(
            Some("desk.local."),
            &[],
            UNRESOLVED_SERVICE_TTL
        ));
        assert!(!unresolved_service_expired(
            None,
            &[],
            UNRESOLVED_SERVICE_TTL - Duration::from_secs(1)
        ));
        assert!(!unresolved_service_expired(
            Some("desk.local."),
            &addrs,
            UNRESOLVED_SERVICE_TTL * 10
        ));
    }

    #[test]
    fn construct_full_name_produces_valid_escaped_fullname() {
        let fullname = construct_full_name("My Host", "_maho-rd._tcp.", "local.").unwrap();
        // DNS-SD escapes a space in the instance label as \032.
        assert_eq!(fullname, "My\\032Host._maho-rd._tcp.local.");

        let dot_name = construct_full_name("host.1", "_maho-rd._tcp", "local").unwrap();
        assert!(dot_name.ends_with("._maho-rd._tcp.local."));
    }

    #[test]
    fn choose_and_format_address_prioritizes_ipv4_over_scoped_ipv6() {
        let addrs = vec![
            (
                IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1)),
                Some(4),
            ),
            (IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)), None),
        ];
        let (ip, formatted) = choose_and_format_address(&addrs).unwrap();
        assert_eq!(ip, IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)));
        assert_eq!(formatted, "192.168.1.10");
    }

    #[test]
    fn choose_and_format_address_formats_scoped_link_local_when_ipv4_absent() {
        let addrs = vec![(
            IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1)),
            Some(5),
        )];
        let (ip, formatted) = choose_and_format_address(&addrs).unwrap();
        assert_eq!(ip, IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1)));
        assert_eq!(formatted, "fe80::1%5");

        let unscoped = vec![
            (IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1)), None),
            (
                IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1)),
                Some(0),
            ),
        ];
        assert!(choose_and_format_address(&unscoped).is_none());
    }

    #[test]
    fn decide_service_state_action_publishes_valid_and_retracts_invalid() {
        let valid_txt = vec![
            ("protocol".into(), b"3".to_vec()),
            ("name".into(), b"desk".to_vec()),
            ("os".into(), b"macos".to_vec()),
            ("udp_port".into(), b"19731".to_vec()),
        ];
        let addrs = vec![(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 50)), None)];

        let action = decide_service_state_action(
            "desk._maho-rd._tcp.local.",
            Some("desk.local."),
            19730,
            &valid_txt,
            &addrs,
        );
        match action {
            ServiceStateAction::Publish(host) => {
                assert_eq!(host.id, "desk._maho-rd._tcp.local.");
                assert_eq!(host.ip, "192.168.1.50");
                assert_eq!(host.tcp_port, 19730);
                assert_eq!(host.udp_port, 19731);
            }
            ServiceStateAction::Retract => panic!("expected publish"),
        }

        let invalid_proto_txt = vec![
            ("protocol".into(), b"2".to_vec()),
            ("name".into(), b"desk".to_vec()),
            ("os".into(), b"macos".to_vec()),
            ("udp_port".into(), b"19731".to_vec()),
        ];
        let retract_action = decide_service_state_action(
            "desk._maho-rd._tcp.local.",
            Some("desk.local."),
            19730,
            &invalid_proto_txt,
            &addrs,
        );
        assert_eq!(retract_action, ServiceStateAction::Retract);

        let zero_port_action = decide_service_state_action(
            "desk._maho-rd._tcp.local.",
            Some("desk.local."),
            0,
            &valid_txt,
            &addrs,
        );
        assert_eq!(zero_port_action, ServiceStateAction::Retract);

        let no_addr_action = decide_service_state_action(
            "desk._maho-rd._tcp.local.",
            Some("desk.local."),
            19730,
            &valid_txt,
            &[],
        );
        assert_eq!(no_addr_action, ServiceStateAction::Retract);

        let scoped_v6_addrs = vec![(
            IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1)),
            Some(5),
        )];
        let scoped_action = decide_service_state_action(
            "desk._maho-rd._tcp.local.",
            Some("desk.local."),
            19730,
            &valid_txt,
            &scoped_v6_addrs,
        );
        match scoped_action {
            ServiceStateAction::Publish(host) => {
                assert_eq!(host.id, "desk._maho-rd._tcp.local.");
                assert_eq!(host.ip, "fe80::1%5");
                assert_eq!(host.tcp_port, 19730);
                assert_eq!(host.udp_port, 19731);
            }
            ServiceStateAction::Retract => panic!("expected publish for fe80::1%5"),
        }
    }
}
