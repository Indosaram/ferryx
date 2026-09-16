use crate::remote::auth::{write_private_json, AuthManager};
use crate::remote::backend::RemoteSessionBackend;
use crate::remote::protocol::{RemoteActiveDesktopSelection, RemoteEventMessage};
use crate::terminal::TerminalService;
use crate::worktree::WorkspaceRegistry;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
#[cfg(test)]
use std::sync::atomic::Ordering;
use std::sync::Arc;
#[cfg(test)]
use tokio::sync::Notify;
use tokio::sync::{broadcast, watch};

pub type DesktopEventSink = Arc<dyn Fn(&str, serde_json::Value) + Send + Sync>;
pub const REMOTE_ACTIVE_SELECTION_CHANGED_EVENT: &str = "remote_active_selection_changed";
pub const DEFAULT_RELAY_URL: &str = "https://relay.checka.cc";
#[cfg(not(test))]
pub const REMOTE_GATEWAY_PORT: u16 = 43821;
#[cfg(test)]
pub const REMOTE_GATEWAY_PORT: u16 = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RemoteNetworkMode {
    Off,
    LocalNetwork,
    Tailscale,
    Relay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum RemoteRestartPolicy {
    SessionOnly,
    #[default]
    RestoreListener,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteGatewayConfig {
    pub mode: RemoteNetworkMode,
    pub port: u16,
    pub allow_control: bool,
    #[serde(default)]
    pub relay_url: Option<String>,
}

impl Default for RemoteGatewayConfig {
    fn default() -> Self {
        Self {
            mode: RemoteNetworkMode::Off,
            port: REMOTE_GATEWAY_PORT,
            allow_control: true,
            relay_url: None,
        }
    }
}

impl RemoteGatewayConfig {
    pub fn restart_policy(&self) -> RemoteRestartPolicy {
        RemoteRestartPolicy::RestoreListener
    }

    fn persisted_snapshot(&self) -> PersistedRemoteGatewayConfig {
        PersistedRemoteGatewayConfig {
            mode: self.mode,
            port: self.port,
            allow_control: self.allow_control,
            restart_policy: self.restart_policy(),
            relay_url: self.relay_url.clone(),
        }
    }
}

/// Resolves the network interface address a `RemoteNetworkMode` should bind
/// to, beyond the always-on loopback listener.
///
/// Implementations must never return a wildcard (`0.0.0.0`) or loopback
/// address for [`RemoteNetworkMode::LocalNetwork`] or
/// [`RemoteNetworkMode::Tailscale`]: those modes exist specifically to expose
/// the gateway on a *specific* external interface, not on every interface.
pub trait InterfaceResolver: Send + Sync {
    /// Returns the primary non-loopback IPv4 address of this machine on the
    /// local network (e.g. a `192.168.x.x` or `10.x.x.x` address).
    fn local_network_address(&self) -> Result<std::net::Ipv4Addr, String>;

    /// Returns the Tailscale CGNAT IPv4 address (`100.64.0.0/10`) of this
    /// machine, or an error if no Tailscale interface is active.
    fn tailscale_address(&self) -> Result<std::net::Ipv4Addr, String>;

    /// Resolves the extra bind address (if any) required for `mode`, on top
    /// of the baseline loopback listener. Returns `Ok(None)` for modes that
    /// only need loopback (`Off`).
    fn resolve(&self, mode: RemoteNetworkMode) -> Result<Option<std::net::Ipv4Addr>, String> {
        match mode {
            RemoteNetworkMode::Off => Ok(None),
            RemoteNetworkMode::LocalNetwork => self.local_network_address().map(Some),
            RemoteNetworkMode::Tailscale => self.tailscale_address().map(Some),
            RemoteNetworkMode::Relay => Ok(None),
        }
    }
}

/// Returns `true` for addresses in the Tailscale/CGNAT range `100.64.0.0/10`
/// (i.e. `100.64.0.0` through `100.127.255.255`).
pub fn is_tailscale_cgnat_address(addr: &std::net::Ipv4Addr) -> bool {
    let octets = addr.octets();
    octets[0] == 100 && (octets[1] & 0b1100_0000) == 0b0100_0000
}

/// Enumerates active, non-loopback IPv4 interface addresses on this machine.
#[cfg(unix)]
fn enumerate_ipv4_interface_addresses() -> Result<Vec<std::net::Ipv4Addr>, String> {
    use std::net::Ipv4Addr;

    let mut addrs = Vec::new();
    unsafe {
        let mut head: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut head) != 0 {
            return Err("failed to enumerate network interfaces".into());
        }
        let mut cursor = head;
        while !cursor.is_null() {
            let iface = &*cursor;
            if !iface.ifa_addr.is_null() && (*iface.ifa_addr).sa_family as i32 == libc::AF_INET {
                let flags = iface.ifa_flags as i32;
                let up = flags & libc::IFF_UP != 0;
                let loopback = flags & libc::IFF_LOOPBACK != 0;
                if up && !loopback {
                    let sockaddr_in = iface.ifa_addr as *const libc::sockaddr_in;
                    let raw = (*sockaddr_in).sin_addr.s_addr;
                    addrs.push(Ipv4Addr::from(u32::from_be(raw)));
                }
            }
            cursor = iface.ifa_next;
        }
        libc::freeifaddrs(head);
    }
    Ok(addrs)
}

/// Ask the routing table for its source address. UDP connect does not send
/// packets, so this needs a route, not a response from the destination.
fn routed_ipv4_address(destination: &str) -> Result<std::net::Ipv4Addr, String> {
    let probe = || -> std::io::Result<std::net::SocketAddr> {
        let socket = std::net::UdpSocket::bind("0.0.0.0:0")?;
        socket.connect(destination)?;
        socket.local_addr()
    };
    match probe().map_err(|error| format!("route probe to {destination} failed: {error}"))? {
        std::net::SocketAddr::V4(addr)
            if !addr.ip().is_unspecified() && !addr.ip().is_loopback() =>
        {
            Ok(*addr.ip())
        }
        _ => Err(format!(
            "route probe to {destination} found no external IPv4 address"
        )),
    }
}

#[cfg(any(not(any(unix, windows)), test))]
fn portable_ipv4_interface_addresses() -> Result<Vec<std::net::Ipv4Addr>, String> {
    portable_ipv4_interface_addresses_with(routed_ipv4_address)
}

#[cfg(any(not(unix), test))]
fn portable_ipv4_interface_addresses_with(
    mut route: impl FnMut(&str) -> Result<std::net::Ipv4Addr, String>,
) -> Result<Vec<std::net::Ipv4Addr>, String> {
    let mut addresses = Vec::new();
    let mut errors = Vec::new();
    for destination in ["8.8.8.8:80", "100.100.100.100:80"] {
        match route(destination) {
            Ok(address) if !addresses.contains(&address) => addresses.push(address),
            Ok(_) => {}
            Err(error) => errors.push(error),
        }
    }
    if addresses.is_empty() {
        Err(errors.join("; "))
    } else {
        for error in errors {
            tracing::debug!(%error, "optional interface route probe unavailable");
        }
        Ok(addresses)
    }
}

#[cfg(not(any(unix, windows)))]
fn enumerate_ipv4_interface_addresses() -> Result<Vec<std::net::Ipv4Addr>, String> {
    portable_ipv4_interface_addresses()
}

#[cfg(windows)]
fn enumerate_ipv4_interface_addresses() -> Result<Vec<std::net::Ipv4Addr>, String> {
    non_unix_ipv4_interface_addresses_with(routed_ipv4_address, windows_adapters::enumerate)
}

#[cfg(any(windows, test))]
fn non_unix_ipv4_interface_addresses_with(
    route: impl FnMut(&str) -> Result<std::net::Ipv4Addr, String>,
    adapters: impl FnOnce() -> Result<Vec<std::net::Ipv4Addr>, String>,
) -> Result<Vec<std::net::Ipv4Addr>, String> {
    let routed = portable_ipv4_interface_addresses_with(route);
    let mut addresses = adapters()?;
    match routed {
        Ok(routed) => {
            for address in routed {
                if !addresses.contains(&address) {
                    addresses.push(address);
                }
            }
        }
        Err(error) => tracing::debug!(%error, "using adapter inventory without off-link routes"),
    }
    Ok(addresses)
}

#[cfg(windows)]
mod windows_adapters {
    use std::mem::{size_of, MaybeUninit};
    use std::net::Ipv4Addr;
    use windows_sys::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, ERROR_NO_DATA, NO_ERROR};
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER,
        GAA_FLAG_SKIP_MULTICAST, IF_TYPE_SOFTWARE_LOOPBACK, IP_ADAPTER_ADDRESSES_LH,
    };
    use windows_sys::Win32::NetworkManagement::Ndis::IfOperStatusUp;
    use windows_sys::Win32::Networking::WinSock::{IpDadStatePreferred, AF_INET, SOCKADDR_IN};

    pub(super) fn enumerate() -> Result<Vec<Ipv4Addr>, String> {
        // SAFETY: GetAdaptersAddresses honors the supplied capacity and on success
        // initializes the linked records within this allocation. No pointer escapes.
        unsafe {
            enumerate_with(|buffer, size| {
                GetAdaptersAddresses(
                    u32::from(AF_INET),
                    GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER,
                    std::ptr::null(),
                    buffer,
                    size,
                )
            })
        }
    }

    /// # Safety
    /// `query` must obey GetAdaptersAddresses' buffer contract: never write beyond
    /// the input size, and on success provide initialized, aligned, acyclic adapter
    /// and unicast records whose pointers remain valid until traversal completes.
    /// All sockaddr pointers must address at least their advertised length.
    unsafe fn enumerate_with(
        mut query: impl FnMut(*mut IP_ADAPTER_ADDRESSES_LH, &mut u32) -> u32,
    ) -> Result<Vec<Ipv4Addr>, String> {
        let mut size = 15 * 1024u32;
        for _ in 0..3 {
            // Typed backing storage guarantees adapter alignment (unlike Vec<u8>).
            // It is never resized after the API has installed interior pointers.
            let count = (size as usize).div_ceil(size_of::<IP_ADAPTER_ADDRESSES_LH>());
            let mut buffer = Vec::<MaybeUninit<IP_ADAPTER_ADDRESSES_LH>>::new();
            buffer.try_reserve_exact(count).map_err(|error| {
                format!("failed to allocate Windows adapter inventory: {error}")
            })?;
            buffer.resize_with(count, MaybeUninit::zeroed);
            let head = buffer.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
            match query(head, &mut size) {
                ERROR_BUFFER_OVERFLOW => continue,
                ERROR_NO_DATA => return Ok(Vec::new()),
                NO_ERROR => {
                    // SAFETY: query's success contract guarantees initialized lists
                    // and sockaddr storage. buffer stays alive/unmoved during this
                    // walk; only copied IPv4 values are returned before it is dropped.
                    return Ok(unsafe { collect(head) });
                }
                status => return Err(format!("GetAdaptersAddresses failed: {status}")),
            }
        }
        Err(format!(
            "GetAdaptersAddresses failed after 3 buffer attempts: {ERROR_BUFFER_OVERFLOW}"
        ))
    }

    /// # Safety
    /// `head` must satisfy enumerate_with's successful query list contract.
    unsafe fn collect(mut head: *const IP_ADAPTER_ADDRESSES_LH) -> Vec<Ipv4Addr> {
        let mut addresses = Vec::new();
        // SAFETY: caller guarantees valid initialized acyclic lists for this walk.
        // Null pointers terminate lists; sockaddr length/family are checked before
        // reading IPv4 layout, with read_unaligned avoiding stronger alignment needs.
        unsafe {
            while let Some(adapter) = head.as_ref() {
                if adapter.OperStatus == IfOperStatusUp
                    && adapter.IfType != IF_TYPE_SOFTWARE_LOOPBACK
                {
                    let mut cursor = adapter.FirstUnicastAddress;
                    while let Some(unicast) = cursor.as_ref() {
                        let socket = unicast.Address;
                        if unicast.DadState == IpDadStatePreferred
                            && !socket.lpSockaddr.is_null()
                            && socket.iSockaddrLength >= size_of::<SOCKADDR_IN>() as i32
                            && std::ptr::addr_of!((*socket.lpSockaddr).sa_family).read_unaligned()
                                == AF_INET
                        {
                            let ipv4 = socket.lpSockaddr.cast::<SOCKADDR_IN>().read_unaligned();
                            let address = Ipv4Addr::from(ipv4.sin_addr.S_un.S_addr.to_ne_bytes());
                            if !addresses.contains(&address) {
                                addresses.push(address);
                            }
                        }
                        cursor = unicast.Next;
                    }
                }
                head = adapter.Next;
            }
        }
        addresses
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use windows_sys::Win32::NetworkManagement::IpHelper::IP_ADAPTER_UNICAST_ADDRESS_LH;
        use windows_sys::Win32::Networking::WinSock::{IpDadStateTentative, AF_INET6};

        #[repr(C)]
        struct Inventory {
            adapters: [IP_ADAPTER_ADDRESSES_LH; 3],
            unicast: [IP_ADAPTER_UNICAST_ADDRESS_LH; 7],
            sockets: [SOCKADDR_IN; 7],
        }

        #[test]
        fn native_inventory_walk_filters_records_and_preserves_cgnat() {
            // Given: typed records stored inside the production-owned API buffer.
            // SAFETY: fixture fits the supplied size/alignment, zero is valid for
            // these C integer/union/raw-pointer structs. All linked pointers stay
            // within the buffer and are installed after its final allocation.
            let result = unsafe {
                enumerate_with(|buffer, size| {
                    assert!(size_of::<Inventory>() <= *size as usize);
                    assert!(
                        std::mem::align_of::<Inventory>()
                            <= std::mem::align_of::<IP_ADAPTER_ADDRESSES_LH>()
                    );
                    let fixture = buffer.cast::<Inventory>();
                    fixture.write(std::mem::zeroed());
                    let adapters = std::ptr::addr_of_mut!((*fixture).adapters)
                        .cast::<IP_ADAPTER_ADDRESSES_LH>();
                    let unicast = std::ptr::addr_of_mut!((*fixture).unicast)
                        .cast::<IP_ADAPTER_UNICAST_ADDRESS_LH>();
                    let sockets = std::ptr::addr_of_mut!((*fixture).sockets).cast::<SOCKADDR_IN>();
                    for index in 0..3 {
                        (*adapters.add(index)).OperStatus = IfOperStatusUp;
                        (*adapters.add(index)).FirstUnicastAddress = unicast;
                        if index < 2 {
                            (*adapters.add(index)).Next = adapters.add(index + 1);
                        }
                    }
                    (*adapters).OperStatus = 0; // down adapter
                    (*adapters.add(1)).IfType = IF_TYPE_SOFTWARE_LOOPBACK;
                    (*adapters.add(2)).FirstUnicastAddress = unicast.add(1);
                    for index in 0..7 {
                        (*unicast.add(index)).DadState = IpDadStatePreferred;
                        (*unicast.add(index)).Address.lpSockaddr = sockets.add(index).cast();
                        (*unicast.add(index)).Address.iSockaddrLength =
                            size_of::<SOCKADDR_IN>() as i32;
                        (*sockets.add(index)).sin_family = AF_INET;
                        (*sockets.add(index)).sin_addr.S_un.S_addr =
                            u32::from_ne_bytes([10, 0, 0, index as u8]);
                        if index < 6 {
                            (*unicast.add(index)).Next = unicast.add(index + 1);
                        }
                    }
                    (*sockets.add(1)).sin_family = AF_INET6;
                    (*unicast.add(2)).Address.iSockaddrLength = 1;
                    (*unicast.add(3)).Address.lpSockaddr = std::ptr::null_mut();
                    (*unicast.add(4)).DadState = IpDadStateTentative;
                    (*sockets.add(5)).sin_addr.S_un.S_addr = u32::from_ne_bytes([100, 88, 12, 4]);
                    (*sockets.add(6)).sin_addr.S_un.S_addr = u32::from_ne_bytes([192, 168, 50, 7]);
                    NO_ERROR
                })
            };
            // When / Then: production traversal copies network-order IPv4 values;
            // down/loopback adapters and unusable unicast records contribute none.
            let addresses = result.expect("native inventory");
            assert_eq!(
                addresses,
                [
                    Ipv4Addr::new(100, 88, 12, 4),
                    Ipv4Addr::new(192, 168, 50, 7)
                ]
            );
            assert_eq!(
                super::super::select_local_network_address(addresses, None),
                Some(Ipv4Addr::new(192, 168, 50, 7))
            );
        }

        #[test]
        fn native_buffer_overflow_retries_with_aligned_larger_storage() {
            let mut calls = 0;
            // SAFETY: first call writes no records and requests a larger buffer;
            // second initializes one terminal C adapter with no unicast records.
            let result = unsafe {
                enumerate_with(|buffer, size| {
                    calls += 1;
                    assert!(buffer.is_aligned());
                    if calls == 1 {
                        *size = 32 * 1024;
                        ERROR_BUFFER_OVERFLOW
                    } else {
                        assert_eq!(*size, 32 * 1024);
                        buffer.write(std::mem::zeroed());
                        NO_ERROR
                    }
                })
            };
            assert_eq!(calls, 2);
            assert_eq!(result, Ok(Vec::new()));
        }

        #[test]
        fn native_buffer_overflow_is_bounded() {
            let mut calls = 0;
            // SAFETY: error-only query never provides records to traverse.
            let result = unsafe {
                enumerate_with(|_, size| {
                    calls += 1;
                    *size += 1024;
                    ERROR_BUFFER_OVERFLOW
                })
            };
            assert_eq!(calls, 3);
            assert!(result.is_err());
        }

        #[test]
        fn native_api_no_data_is_empty_and_failure_is_not_success() {
            // SAFETY: neither return status permits buffer traversal.
            let empty = unsafe { enumerate_with(|_, _| ERROR_NO_DATA) };
            // SAFETY: error-only query does not initialize or expose records.
            let failure = unsafe { enumerate_with(|_, _| 5) };
            assert_eq!(empty, Ok(Vec::new()));
            assert!(failure.is_err());
        }
    }
}

fn select_local_network_address(
    addresses: impl IntoIterator<Item = std::net::Ipv4Addr>,
    routed: Option<std::net::Ipv4Addr>,
) -> Option<std::net::Ipv4Addr> {
    addresses
        .into_iter()
        .chain(routed)
        .filter(|addr| {
            !addr.is_unspecified()
                && !addr.is_loopback()
                && !addr.is_link_local()
                && !addr.is_multicast()
                && !addr.is_broadcast()
                && !is_tailscale_cgnat_address(addr)
        })
        .min_by_key(|addr| (Some(*addr) != routed, !addr.is_private()))
}

/// Default [`InterfaceResolver`] backed by routing probes and the operating
/// system's active interface list on Unix and Windows.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemInterfaceResolver;

impl InterfaceResolver for SystemInterfaceResolver {
    fn local_network_address(&self) -> Result<std::net::Ipv4Addr, String> {
        match routed_ipv4_address("8.8.8.8:80") {
            Ok(routed) => {
                if let Some(address) = select_local_network_address([], Some(routed)) {
                    return Ok(address);
                }
            }
            Err(error) => tracing::debug!(%error, "using interface list for LAN resolution"),
        }
        select_local_network_address(enumerate_ipv4_interface_addresses()?, None)
            .ok_or_else(|| "no active local network IPv4 interface found".into())
    }

    fn tailscale_address(&self) -> Result<std::net::Ipv4Addr, String> {
        enumerate_ipv4_interface_addresses()?
            .into_iter()
            .find(is_tailscale_cgnat_address)
            .ok_or_else(|| "no active Tailscale IPv4 interface found".into())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedRemoteGatewayConfig {
    mode: RemoteNetworkMode,
    port: u16,
    allow_control: bool,
    #[serde(default)]
    restart_policy: RemoteRestartPolicy,
    #[serde(default)]
    relay_url: Option<String>,
}

#[derive(Debug, Clone)]
struct WorkspaceCacheEntry {
    snapshot: Arc<crate::remote::server::WorkspaceSnapshotCache>,
    revision: u64,
    created_at: std::time::Instant,
}

pub(crate) const WORKSPACE_SNAPSHOT_REFRESH_INTERVAL: std::time::Duration =
    std::time::Duration::from_secs(2);

/// A pairing coordinator published by one relay owner.
///
/// `epoch` identifies the publishing owner so a stopping relay clears only its
/// own entry and never a newer owner's.
pub struct PublishedPairing {
    pub coordinator: crate::remote::relay_client::PairingCoordinator,
    pub epoch: u64,
}

/// Monotonic source for PublishedPairing::epoch.
pub static RELAY_PAIRING_EPOCH: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);

pub struct RemoteGatewayState {
    pub config: RwLock<RemoteGatewayConfig>,
    pub auth_manager: Arc<AuthManager>,
    pub terminal_service: Arc<TerminalService>,
    /// Session-owning daemon epoch, supplied by the daemon at construction.
    pub daemon_epoch: AtomicU64,
    /// Explicit stores keep identity alongside auth, including private fixtures.
    pub(crate) identity_dir: Option<PathBuf>,
    #[cfg(test)]
    pub(crate) identity_probe: RwLock<Option<Arc<dyn Fn() + Send + Sync>>>,
    #[cfg(test)]
    pub(crate) browse_home: RwLock<Option<PathBuf>>,
    #[cfg(test)]
    pub(crate) browse_probe: RwLock<Option<Arc<dyn Fn() + Send + Sync>>>,
    pub session_backend: Arc<dyn RemoteSessionBackend>,
    /// Absent for legacy/test constructors. Presence enables no future API.
    pub machine_services: Option<Arc<crate::daemon::MachineServices>>,
    pub workspace_registry: WorkspaceRegistry,
    pub ssh_store_path: RwLock<Option<PathBuf>>,
    pub active_selection: RwLock<Option<RemoteActiveDesktopSelection>>,
    pub active_session_tx: watch::Sender<Option<String>>,
    pub event_tx: broadcast::Sender<String>,
    pub is_running: RwLock<bool>,
    pub bound_address: RwLock<Option<String>>,
    config_path: Option<PathBuf>,
    pub desktop_event_sink: RwLock<Option<DesktopEventSink>>,
    /// The single relay pairing authority owned by the running gateway.
    ///
    /// Populated when the relay client starts. A pairing code is only redeemable
    /// remotely if the PIN was registered with the relay, so GUI and CLI pairing must
    /// go through this one coordinator instead of each minting a local-only code or
    /// standing up a competing RelayClient for the same machine identity.
    pub relay_pairing: RwLock<Option<PublishedPairing>>,
    /// Single-use socket tickets minted for direct-gateway WebSocket upgrades.
    ///
    /// The browser `WebSocket` constructor cannot set an `Authorization` header, so a
    /// direct connection previously put the PERMANENT device token in the URL query,
    /// where it lands in gateway access logs and browser history. A ticket is minted
    /// from the bearer over HTTP, scoped to one target, expires in
    /// [`SOCKET_TICKET_TTL_SECS`], and is removed on first use.
    ///
    /// Maps ticket -> (device token, target, expiry unix seconds).
    pub socket_tickets: parking_lot::Mutex<std::collections::HashMap<String, (String, String, u64)>>,
    snapshot_cache: RwLock<Option<WorkspaceCacheEntry>>,
    snapshot_lock: tokio::sync::Mutex<()>,
    #[cfg(test)]
    snapshot_build_count: Arc<AtomicU64>,
    #[cfg(test)]
    snapshot_build_completed: Notify,
    #[cfg(test)]
    snapshot_post_build_hook: Arc<RwLock<Option<Arc<dyn Fn() + Send + Sync>>>>,
}

impl RemoteGatewayState {
    /// Derive both gateway handles from the authority before publication.
    pub(crate) fn with_machine_services(
        mut self,
        sessions: Arc<crate::daemon::session_service::DaemonSessionService>,
    ) -> Self {
        let workspaces = Arc::clone(&sessions.workspace_service);
        self.workspace_registry = workspaces.registry.clone();
        self.session_backend = sessions.clone();
        self.machine_services = Some(Arc::new(crate::daemon::MachineServices { sessions, workspaces }));
        self
    }

    pub fn new(
        terminal_service: Arc<TerminalService>,
        workspace_registry: WorkspaceRegistry,
    ) -> Self {
        Self::new_with_paths_and_service(
            Arc::clone(&terminal_service) as Arc<dyn RemoteSessionBackend>,
            terminal_service,
            workspace_registry,
            None,
            None,
        )
    }

    pub fn new_with_backend(
        session_backend: Arc<dyn RemoteSessionBackend>,
        workspace_registry: WorkspaceRegistry,
    ) -> Self {
        Self::new_with_paths_backend(session_backend, workspace_registry, None, None)
    }

    pub fn new_persistent(
        terminal_service: Arc<TerminalService>,
        workspace_registry: WorkspaceRegistry,
    ) -> Self {
        Self::new_persistent_with_service(
            Arc::clone(&terminal_service) as Arc<dyn RemoteSessionBackend>,
            terminal_service,
            workspace_registry,
        )
    }

    pub fn new_persistent_with_backend(
        session_backend: Arc<dyn RemoteSessionBackend>,
        workspace_registry: WorkspaceRegistry,
    ) -> Self {
        Self::new_persistent_with_service(
            session_backend,
            Arc::new(TerminalService::default()),
            workspace_registry,
        )
    }

    pub fn new_persistent_with_service(
        session_backend: Arc<dyn RemoteSessionBackend>,
        terminal_service: Arc<TerminalService>,
        workspace_registry: WorkspaceRegistry,
    ) -> Self {
        let base = remote_data_dir();
        if base.is_none() {
            tracing::warn!(
                "no per-user data directory could be resolved; remote pairing state will be \
                 kept in memory only and lost on exit"
            );
        }
        Self::new_with_paths_and_service(
            session_backend,
            terminal_service,
            workspace_registry,
            base.as_ref().map(|base| base.join("remote-config.json")),
            base.as_ref().map(|base| base.join("remote-auth.json")),
        )
    }

    pub fn new_with_paths(
        terminal_service: Arc<TerminalService>,
        workspace_registry: WorkspaceRegistry,
        config_path: Option<PathBuf>,
        auth_path: Option<PathBuf>,
    ) -> Self {
        Self::new_with_paths_and_service(
            Arc::clone(&terminal_service) as Arc<dyn RemoteSessionBackend>,
            terminal_service,
            workspace_registry,
            config_path,
            auth_path,
        )
    }

    pub fn new_with_paths_backend(
        session_backend: Arc<dyn RemoteSessionBackend>,
        workspace_registry: WorkspaceRegistry,
        config_path: Option<PathBuf>,
        auth_path: Option<PathBuf>,
    ) -> Self {
        Self::new_with_paths_and_service(
            session_backend,
            Arc::new(TerminalService::default()),
            workspace_registry,
            config_path,
            auth_path,
        )
    }

    pub fn new_with_paths_and_service(
        session_backend: Arc<dyn RemoteSessionBackend>,
        terminal_service: Arc<TerminalService>,
        workspace_registry: WorkspaceRegistry,
        config_path: Option<PathBuf>,
        auth_path: Option<PathBuf>,
    ) -> Self {
        let (event_tx, _) = broadcast::channel(1024);
        let (active_session_tx, _) = watch::channel(None);
        let config = config_path
            .as_deref()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<PersistedRemoteGatewayConfig>(&bytes).ok())
            .map(|persisted| RemoteGatewayConfig {
                mode: persisted.mode,
                // Port is fixed; ignore persisted value so stale custom ports heal on load.
                port: REMOTE_GATEWAY_PORT,
                allow_control: persisted.allow_control,
                relay_url: persisted.relay_url,
            })
            .unwrap_or_default();
        Self {
            config: RwLock::new(config),
            identity_dir: auth_path.as_deref().and_then(|path| path.parent()).map(PathBuf::from),
            #[cfg(test)]
            identity_probe: RwLock::new(None),
            #[cfg(test)]
            browse_home: RwLock::new(None),
            #[cfg(test)]
            browse_probe: RwLock::new(None),
            auth_manager: Arc::new(AuthManager::with_persistence(auth_path)),
            terminal_service,
            daemon_epoch: AtomicU64::new(0),
            session_backend,
            workspace_registry,
            machine_services: None,
            active_selection: RwLock::new(None),
            ssh_store_path: RwLock::new(None),
            active_session_tx,
            event_tx,
            is_running: RwLock::new(false),
            bound_address: RwLock::new(None),
            config_path,
            desktop_event_sink: RwLock::new(None),
            relay_pairing: RwLock::new(None),
            socket_tickets: parking_lot::Mutex::new(std::collections::HashMap::new()),
            snapshot_cache: RwLock::new(None),
            snapshot_lock: tokio::sync::Mutex::new(()),
            #[cfg(test)]
            snapshot_build_count: Arc::new(AtomicU64::new(0)),
            #[cfg(test)]
            snapshot_build_completed: Notify::new(),
            #[cfg(test)]
            snapshot_post_build_hook: Arc::new(RwLock::new(None)),
        }
    }

    #[cfg(test)]
    pub fn snapshot_build_count(&self) -> u64 {
        self.snapshot_build_count.load(Ordering::Acquire)
    }

    pub fn invalidate_workspace_snapshot(&self) {
        *self.snapshot_cache.write() = None;
    }

    pub(crate) async fn workspace_snapshot(
        self: &Arc<Self>,
    ) -> Result<Arc<crate::remote::server::WorkspaceSnapshotCache>, String> {
        self.workspace_snapshot_at(std::time::Instant::now()).await
    }

    pub(crate) async fn workspace_snapshot_at(
        self: &Arc<Self>,
        now: std::time::Instant,
    ) -> Result<Arc<crate::remote::server::WorkspaceSnapshotCache>, String> {
        let current_rev = self.workspace_registry.revision();
        let mut observed_snapshot = None;

        {
            let cache = self.snapshot_cache.read();
            if let Some(entry) = cache.as_ref() {
                observed_snapshot = Some(Arc::clone(&entry.snapshot));
                // P16: Only serve cached snapshot when within refresh interval.
                // Once the refresh interval expires, await rebuild so explicit
                // list/refresh requests reflect external worktree changes immediately
                // without serving intentionally stale data.
                if entry.revision == current_rev
                    && now.saturating_duration_since(entry.created_at)
                        < WORKSPACE_SNAPSHOT_REFRESH_INTERVAL
                {
                    return Ok(Arc::clone(&entry.snapshot));
                }
            }
        }

        self.rebuild_workspace_snapshot(now, observed_snapshot)
            .await
    }

    async fn rebuild_workspace_snapshot(
        &self,
        now: std::time::Instant,
        observed_snapshot: Option<Arc<crate::remote::server::WorkspaceSnapshotCache>>,
    ) -> Result<Arc<crate::remote::server::WorkspaceSnapshotCache>, String> {
        let _guard = self.snapshot_lock.lock().await;
        let current_rev = self.workspace_registry.revision();

        {
            let cache = self.snapshot_cache.read();
            if let Some(entry) = cache.as_ref() {
                if entry.revision == current_rev {
                    match observed_snapshot.as_ref() {
                        None => return Ok(Arc::clone(&entry.snapshot)),
                        Some(observed) if !Arc::ptr_eq(observed, &entry.snapshot) => {
                            return Ok(Arc::clone(&entry.snapshot));
                        }
                        Some(_) => {}
                    }
                }
            }
        }

        let registry = self.workspace_registry.clone();
        #[cfg(test)]
        let build_counter = Arc::clone(&self.snapshot_build_count);
        #[cfg(test)]
        let post_build_hook = Arc::clone(&self.snapshot_post_build_hook);
        let snapshot = tokio::task::spawn_blocking(move || {
            #[cfg(test)]
            build_counter.fetch_add(1, Ordering::AcqRel);
            let snapshot = crate::remote::server::WorkspaceSnapshotCache::build(&registry);
            #[cfg(test)]
            if let Some(hook) = post_build_hook.read().as_ref() {
                hook();
            }
            snapshot
        })
        .await
        .map_err(|error| format!("workspace snapshot task failed: {error}"))?;

        let snapshot_arc = Arc::new(snapshot);
        {
            let mut cache = self.snapshot_cache.write();
            *cache = Some(WorkspaceCacheEntry {
                snapshot: Arc::clone(&snapshot_arc),
                // Use the revision observed before discovery started. A managed
                // mutation that overlaps the blocking Git scan is therefore a
                // mismatch on the next request, never falsely marked as part of
                // this snapshot.
                revision: current_rev,
                created_at: now,
            });
        }
        #[cfg(test)]
        self.snapshot_build_completed.notify_waiters();
        Ok(snapshot_arc)
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn next_snapshot_build(&self) -> impl std::future::Future<Output = ()> + '_ {
        self.snapshot_build_completed.notified()
    }

    #[cfg(test)]
    pub(crate) fn set_snapshot_post_build_hook(&self, hook: Option<Arc<dyn Fn() + Send + Sync>>) {
        *self.snapshot_post_build_hook.write() = hook;
    }

    pub fn set_active_selection(&self, mut selection: RemoteActiveDesktopSelection) {
        // This broadcaster belongs to the legacy mirror. Machine ownership must
        // also fence selection snapshots/events, not just the session list/socket.
        if let Some(services) = &self.machine_services {
            let Ok(catalog) = services.workspaces.catalog() else {
                self.clear_active_selection();
                return;
            };
            let private_workspace = |id: &str| catalog.workspaces.get(id).is_some_and(|row| !row.mirror_exposed);
            if selection.session_id.as_deref().is_some_and(|id| services.sessions.machine_only(id))
                || selection.workspace_id.as_deref().is_some_and(private_workspace) {
                self.clear_active_selection();
                return;
            }
            selection.terminal_tabs.retain(|tab| !tab.session_id.as_deref().is_some_and(|id| services.sessions.machine_only(id)));
            selection.attention_inventory.retain(|entry| !private_workspace(&entry.workspace_id));
        }
        let session_id = selection.session_id.clone();
        let payload = serde_json::to_value(&selection).unwrap_or(serde_json::Value::Null);
        *self.active_selection.write() = Some(selection);
        // `send` fails and discards the value when no receiver is alive, which is the normal
        // state before any remote client attaches. `send_replace` stores it regardless so a
        // later subscriber observes the current selection instead of the initial `None`.
        self.active_session_tx.send_replace(session_id);
        self.emit_active_selection_changed(payload);
    }

    pub fn clear_active_selection(&self) {
        *self.active_selection.write() = None;
        self.active_session_tx.send_replace(None);
        self.emit_active_selection_changed(serde_json::Value::Null);
    }

    pub fn set_active_selection_opt(&self, selection: Option<RemoteActiveDesktopSelection>) {
        match selection {
            Some(sel) => self.set_active_selection(sel),
            None => self.clear_active_selection(),
        }
    }

    pub fn active_session_watch_rx(&self) -> watch::Receiver<Option<String>> {
        self.active_session_tx.subscribe()
    }

    pub fn active_selection(&self) -> Option<RemoteActiveDesktopSelection> {
        self.active_selection.read().clone()
    }

    pub fn set_desktop_event_sink(&self, sink: DesktopEventSink) {
        *self.desktop_event_sink.write() = Some(sink);
    }

    pub fn emit_desktop_event(&self, event: &str, payload: serde_json::Value) {
        if let Some(sink) = self.desktop_event_sink.read().as_ref() {
            sink(event, payload.clone());
        }
        let _ = self.event_tx.send(
            serde_json::to_string(&RemoteEventMessage {
                event: event.to_string(),
                payload,
            })
            .unwrap_or_default(),
        );
    }

    pub fn emit_event(&self, event_json: String) {
        let _ = self.event_tx.send(event_json);
    }

    fn emit_active_selection_changed(&self, payload: serde_json::Value) {
        if let Ok(event) = serde_json::to_string(&RemoteEventMessage {
            event: REMOTE_ACTIVE_SELECTION_CHANGED_EVENT.to_string(),
            payload,
        }) {
            let _ = self.event_tx.send(event);
        }
    }

    pub fn persist_config(&self) -> std::io::Result<()> {
        let Some(path) = self.config_path.as_deref() else {
            return Ok(());
        };
        write_private_json(path, &self.config.read().persisted_snapshot())
    }
}

/// Per-user directory sources consulted in order, as `(environment variable, subdirectory)`.
///
/// Windows does not define `HOME`; per-user application state belongs under `LOCALAPPDATA`,
/// whose default ACL excludes other standard users.
#[cfg(windows)]
const DATA_DIR_SOURCES: &[(&str, &str)] = &[("LOCALAPPDATA", "Ferryx"), ("USERPROFILE", ".ferryx")];
#[cfg(not(windows))]
const DATA_DIR_SOURCES: &[(&str, &str)] = &[("HOME", ".ferryx")];

/// Resolves the remote data directory from an arbitrary environment lookup.
///
/// Returns `None` when no per-user location can be determined. Callers then run without
/// persistence: this file holds plaintext device tokens, so falling back to a shared
/// scratch directory such as [`std::env::temp_dir`] would place credentials somewhere
/// other accounts may read and automated cleaners routinely purge.
fn resolve_remote_data_dir<F>(lookup: F) -> Option<PathBuf>
where
    F: Fn(&str) -> Option<OsString>,
{
    if let Some(path) = lookup("FERRYX_DATA_DIR") {
        return Some(PathBuf::from(path).join("remote"));
    }
    DATA_DIR_SOURCES
        .iter()
        .find_map(|(variable, subdirectory)| {
            lookup(variable).map(|base| PathBuf::from(base).join(subdirectory).join("remote"))
        })
}

fn remote_data_dir() -> Option<PathBuf> {
    resolve_remote_data_dir(|variable| std::env::var_os(variable))
}

#[cfg(test)]
mod tests {
    /// A pairing requested as View must not redeem into a Control device. The relay
    /// capability carries the permission that `exchange_pairing_code` copies onto the
    /// issued device, so dropping it silently escalates the recipient.
    #[tokio::test]
    async fn relay_pairing_preserves_the_requested_permission() {
        use crate::remote::auth::{AuthManager, DevicePermission};
        use crate::remote::relay_client::PairingCoordinator;

        for permission in [DevicePermission::View, DevicePermission::Control] {
            let (tx, mut rx) = tokio::sync::mpsc::channel(1);
            let auth = AuthManager::with_persistence(None);
            let coordinator =
                PairingCoordinator::new_with_auth("perm-machine", tx, auth.clone());

            let relay = tokio::spawn(async move {
                let request = rx.recv().await.expect("registration reaches the relay");
                let ack = crate::remote::protocol::RegisterPairingPinAck {
                    generation: request.registration.generation,
                    pin: request.registration.pin.clone(),
                    machine_id: request.registration.machine_id.clone(),
                    status: "ready".into(),
                };
                let token = request.registration.pairing_token.clone();
                let _ = request.ack.send(Ok(ack));
                token
            });

            coordinator
                .generate_pairing_with_permission(std::time::Duration::from_secs(60), permission)
                .await
                .expect("pairing generation succeeds once the relay ACKs");
            let token = relay.await.unwrap();

            let (_, device) = auth
                .exchange_pairing_code(&token, "paired device")
                .expect("the relay capability must be redeemable");
            assert_eq!(
                device.permission, permission,
                "the issued device must carry the permission the pairing requested"
            );
        }
    }

    /// A stopped relay must not leave a dead coordinator selected: pairing would fail
    /// with "Relay registration channel closed" instead of falling back to local.
    #[test]
    fn stopping_a_relay_clears_only_its_own_published_coordinator() {
        use crate::remote::relay_client::PairingCoordinator;
        use std::sync::atomic::Ordering;

        let published = |epoch: u64| {
            let (tx, _rx) = tokio::sync::mpsc::channel(1);
            PublishedPairing {
                coordinator: PairingCoordinator::new("m", tx),
                epoch,
            }
        };
        let slot = parking_lot::RwLock::new(Some(published(1)));

        // A newer owner replaced the publication; the older handle's cleanup must be
        // a no-op rather than clearing the live coordinator.
        *slot.write() = Some(published(2));
        let stale_epoch = 1;
        {
            let mut guard = slot.write();
            if guard.as_ref().is_some_and(|c| c.epoch == stale_epoch) {
                *guard = None;
            }
        }
        assert!(
            slot.read().is_some(),
            "an older handle must not clear a newer owner's coordinator"
        );

        // The owning handle clears its own publication.
        let owning_epoch = slot.read().as_ref().unwrap().epoch;
        {
            let mut guard = slot.write();
            if guard.as_ref().is_some_and(|c| c.epoch == owning_epoch) {
                *guard = None;
            }
        }
        assert!(
            slot.read().is_none(),
            "a stopping relay must clear the coordinator it published"
        );

        assert!(RELAY_PAIRING_EPOCH.fetch_add(1, Ordering::Relaxed) >= 1);
    }

    /// The pairing authority the daemon serves GUI/CLI requests from must be the one
    /// registered with the relay. A code minted only in the local AuthManager is not
    /// redeemable remotely, because the relay never learns its PIN.
    #[tokio::test]
    async fn relay_pairing_coordinator_registers_the_pin_with_the_relay() {
        use crate::remote::relay_client::PairingCoordinator;

        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let coordinator = PairingCoordinator::new("daemon-machine", tx);

        // Answer the registration the way a connected relay control channel would.
        let relay = tokio::spawn(async move {
            let request = rx.recv().await.expect("pairing must be registered with the relay");
            let pin = request.registration.pin.clone();
            let machine = request.registration.machine_id.clone();
            let _ = request.ack.send(Ok(
                crate::remote::protocol::RegisterPairingPinAck {
                    generation: request.registration.generation,
                    pin: pin.clone(),
                    machine_id: machine.clone(),
                    status: "ready".into(),
                },
            ));
            (pin, machine)
        });

        let info = coordinator
            .generate_pairing(std::time::Duration::from_secs(60))
            .await
            .expect("pairing generation must succeed once the relay ACKs");

        let (registered_pin, machine) = relay.await.unwrap();
        assert_eq!(
            registered_pin, info.pin,
            "the PIN handed to the user must be the PIN registered with the relay"
        );
        assert_eq!(machine, "daemon-machine");
        assert_eq!(info.pin.len(), 6);
    }

    use super::*;
    use crate::remote::auth::DevicePermission;
    use crate::terminal::{PtyManager, TerminalOutputHub};

    fn test_state(
        config_path: PathBuf,
        auth_path: PathBuf,
    ) -> (RemoteGatewayState, WorkspaceRegistry, Arc<TerminalService>) {
        let pty = Arc::new(PtyManager::new());
        let hub = Arc::new(TerminalOutputHub::default());
        let terminal = Arc::new(TerminalService::new(pty, hub));
        let registry = WorkspaceRegistry::new();
        let state = RemoteGatewayState::new_with_paths(
            Arc::clone(&terminal),
            registry.clone(),
            Some(config_path),
            Some(auth_path),
        );
        (state, registry, terminal)
    }

    /// Test-only [`InterfaceResolver`] returning fixed, injected addresses so
    /// resolution logic can be exercised without depending on the host's
    /// actual network configuration.
    struct MockInterfaceResolver {
        local_network: Result<std::net::Ipv4Addr, String>,
        tailscale: Result<std::net::Ipv4Addr, String>,
    }

    impl InterfaceResolver for MockInterfaceResolver {
        fn local_network_address(&self) -> Result<std::net::Ipv4Addr, String> {
            self.local_network.clone()
        }

        fn tailscale_address(&self) -> Result<std::net::Ipv4Addr, String> {
            self.tailscale.clone()
        }
    }

    #[test]
    fn test_relay_mode_configuration() {
        // RemoteNetworkMode::Relay serializes to the expected camelCase wire value.
        let mode_json = serde_json::to_string(&RemoteNetworkMode::Relay).expect("serialize mode");
        assert_eq!(mode_json, "\"relay\"");
        let mode_back: RemoteNetworkMode =
            serde_json::from_str(&mode_json).expect("deserialize mode");
        assert_eq!(mode_back, RemoteNetworkMode::Relay);

        // RemoteGatewayConfig with a relay_url round-trips through JSON.
        let config = RemoteGatewayConfig {
            mode: RemoteNetworkMode::Relay,
            port: REMOTE_GATEWAY_PORT,
            allow_control: true,
            relay_url: Some("https://relay.example.com".to_string()),
        };
        let config_json = serde_json::to_string(&config).expect("serialize config");
        assert!(config_json.contains(r#""mode":"relay""#));
        assert!(config_json.contains(r#""relayUrl":"https://relay.example.com""#));
        let config_back: RemoteGatewayConfig =
            serde_json::from_str(&config_json).expect("deserialize config");
        assert_eq!(config_back.mode, RemoteNetworkMode::Relay);
        assert_eq!(
            config_back.relay_url,
            Some("https://relay.example.com".to_string())
        );

        // The relay URL persists to disk and is restored on reopen.
        let dir = tempfile::TempDir::new().expect("tempdir");
        let config_path = dir.path().join("config.json");
        let auth_path = dir.path().join("auth.json");
        let (state, registry, terminal) = test_state(config_path.clone(), auth_path.clone());

        {
            let mut config = state.config.write();
            config.mode = RemoteNetworkMode::Relay;
            config.relay_url = Some("https://relay.example.com".to_string());
        }
        state.persist_config().expect("persist");

        let on_disk: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&config_path).expect("read persisted config"))
                .expect("parse persisted config");
        assert_eq!(on_disk["mode"], "relay");
        assert_eq!(on_disk["relayUrl"], "https://relay.example.com");

        let reopened = RemoteGatewayState::new_with_paths(
            terminal,
            registry,
            Some(config_path),
            Some(auth_path),
        );
        let reopened_config = reopened.config.read().clone();
        assert_eq!(reopened_config.mode, RemoteNetworkMode::Relay);
        assert_eq!(
            reopened_config.relay_url,
            Some("https://relay.example.com".to_string())
        );
    }

    #[test]
    fn test_interface_resolver_portable() {
        // This smoke test requires an active IPv4 network, but no remote server
        // or Internet response. Exercise the Windows probe path on Unix too.
        let addresses = portable_ipv4_interface_addresses().expect("active IPv4 route");
        assert!(!addresses.is_empty());
        let address = SystemInterfaceResolver
            .local_network_address()
            .expect("active local network IPv4 interface");
        assert!(!address.is_unspecified());
        assert!(!address.is_loopback());
        assert!(!is_tailscale_cgnat_address(&address));
        std::net::UdpSocket::bind((address, 0)).expect("resolved address is locally bindable");
    }

    #[test]
    fn p15_offline_lan_survives_both_route_failures() {
        use std::net::Ipv4Addr;

        // Given: only on-link LAN is usable; neither off-link route exists.
        let lan = Ipv4Addr::new(192, 168, 50, 7);
        let cgnat = Ipv4Addr::new(100, 88, 12, 4);
        let mut probes = Vec::new();
        let inventory_read = std::cell::Cell::new(false);

        // When: execute the production non-Unix fallback and LAN selector.
        let result = non_unix_ipv4_interface_addresses_with(
            |destination| {
                probes.push(destination.to_owned());
                Err("fixture: no off-link route".to_owned())
            },
            || {
                inventory_read.set(true);
                Ok(vec![cgnat, lan])
            },
        )
        .and_then(|addresses| {
            select_local_network_address(addresses, None)
                .ok_or_else(|| "fixture: no eligible LAN address".to_owned())
        });

        // Then: route failures cannot hide a usable adapter or select CGNAT.
        assert_eq!(probes, ["8.8.8.8:80", "100.100.100.100:80"]);
        assert_eq!(result, Ok(lan));
        assert!(inventory_read.get());
    }

    #[test]
    fn p15_offline_lan_rejects_cgnat_only_inventory() {
        use std::net::Ipv4Addr;

        // Given: a Tailscale-only inventory and no usable preferred route.
        let inventory = [
            Ipv4Addr::new(100, 64, 0, 0),
            Ipv4Addr::new(100, 127, 255, 255),
        ];
        // When / Then: LocalNetwork must not expose the Tailscale adapter.
        assert_eq!(select_local_network_address(inventory, None), None);
    }

    #[test]
    fn test_interface_resolver_multihoming() {
        use std::net::Ipv4Addr;

        let bridge = Ipv4Addr::new(172, 17, 0, 1);
        let lan = Ipv4Addr::new(192, 168, 1, 42);
        let public = Ipv4Addr::new(203, 0, 113, 5);
        let tailscale = Ipv4Addr::new(100, 88, 12, 4);
        assert_eq!(
            select_local_network_address([bridge, lan, public], Some(lan)),
            Some(lan)
        );
        assert_eq!(
            select_local_network_address([bridge, public], Some(public)),
            Some(public)
        );
        assert_eq!(select_local_network_address([public, lan], None), Some(lan));
        assert_eq!(
            select_local_network_address([tailscale, lan], Some(tailscale)),
            Some(lan)
        );
        assert_eq!(
            select_local_network_address(
                [Ipv4Addr::UNSPECIFIED, Ipv4Addr::LOCALHOST, tailscale],
                None
            ),
            None
        );
    }

    #[test]
    fn test_interface_resolver_address_selection() {
        use std::net::Ipv4Addr;

        let resolver = MockInterfaceResolver {
            local_network: Ok(Ipv4Addr::new(192, 168, 1, 42)),
            tailscale: Ok(Ipv4Addr::new(100, 88, 12, 4)),
        };

        // Off never needs an extra bind address.
        assert_eq!(resolver.resolve(RemoteNetworkMode::Off), Ok(None));

        // LocalNetwork resolves to the detected LAN address.
        assert_eq!(
            resolver.resolve(RemoteNetworkMode::LocalNetwork),
            Ok(Some(Ipv4Addr::new(192, 168, 1, 42)))
        );

        // Tailscale resolves to the detected CGNAT address.
        assert_eq!(
            resolver.resolve(RemoteNetworkMode::Tailscale),
            Ok(Some(Ipv4Addr::new(100, 88, 12, 4)))
        );

        // Tailscale resolution surfaces an error when no interface is found,
        // it must never silently fall back to a wildcard bind.
        let no_tailscale = MockInterfaceResolver {
            local_network: Ok(Ipv4Addr::new(10, 0, 0, 5)),
            tailscale: Err("no active Tailscale IPv4 interface found".into()),
        };
        assert!(no_tailscale.resolve(RemoteNetworkMode::Tailscale).is_err());

        // CGNAT range classification: 100.64.0.0/10 only.
        assert!(is_tailscale_cgnat_address(&Ipv4Addr::new(100, 64, 0, 0)));
        assert!(is_tailscale_cgnat_address(&Ipv4Addr::new(100, 100, 1, 1)));
        assert!(is_tailscale_cgnat_address(&Ipv4Addr::new(
            100, 127, 255, 255
        )));
        assert!(!is_tailscale_cgnat_address(&Ipv4Addr::new(100, 63, 0, 0)));
        assert!(!is_tailscale_cgnat_address(&Ipv4Addr::new(100, 128, 0, 0)));
        assert!(!is_tailscale_cgnat_address(&Ipv4Addr::new(192, 168, 1, 1)));
        assert!(!is_tailscale_cgnat_address(&Ipv4Addr::new(127, 0, 0, 1)));
    }

    #[test]
    fn enabled_gateway_persists_config_and_restores_on_reopen() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let config_path = dir.path().join("config.json");
        let auth_path = dir.path().join("auth.json");
        let (state, registry, terminal) = test_state(config_path.clone(), auth_path.clone());

        {
            let mut config = state.config.write();
            config.mode = RemoteNetworkMode::LocalNetwork;
            config.port = 45678;
            config.allow_control = false;
        }
        *state.is_running.write() = true;
        *state.bound_address.write() = Some("0.0.0.0:45678".into());
        let code = state
            .auth_manager
            .create_pairing_code(DevicePermission::Control);
        let (token, device) = state
            .auth_manager
            .exchange_pairing_code(&code, "Phone")
            .expect("pair");
        state.persist_config().expect("persist");

        let on_disk: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&config_path).expect("read persisted config"))
                .expect("parse persisted config");
        assert_eq!(on_disk["mode"], "localNetwork");
        assert_eq!(on_disk["port"], 45678);
        assert_eq!(on_disk["allowControl"], false);
        assert_eq!(on_disk["restartPolicy"], "restoreListener");

        let reopened = RemoteGatewayState::new_with_paths(
            terminal,
            registry,
            Some(config_path),
            Some(auth_path),
        );
        let config = reopened.config.read().clone();
        assert_eq!(config.mode, RemoteNetworkMode::LocalNetwork);
        assert_eq!(config.port, REMOTE_GATEWAY_PORT);
        assert!(!config.allow_control);
        assert_eq!(
            config.restart_policy(),
            RemoteRestartPolicy::RestoreListener
        );
        assert!(!*reopened.is_running.read());
        assert!(reopened.bound_address.read().is_none());

        let devices = reopened.auth_manager.list_devices();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].id, device.id);
        assert!(reopened.auth_manager.validate_token(&token).is_ok());
    }

    #[test]
    fn daemon_default_state_is_off_and_uses_supplied_terminal_service() {
        let pty = Arc::new(PtyManager::new());
        let hub = Arc::new(TerminalOutputHub::default());
        let terminal = Arc::new(TerminalService::new(pty, hub));
        let state = RemoteGatewayState::new_with_backend(
            Arc::clone(&terminal) as Arc<dyn RemoteSessionBackend>,
            WorkspaceRegistry::new(),
        );

        assert_eq!(state.config.read().mode, RemoteNetworkMode::Off);
        assert!(!*state.is_running.read());
    }

    #[test]
    fn persisted_enabled_config_file_loads_enabled_mode_with_unbound_initial_state() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let config_path = dir.path().join("config.json");
        let auth_path = dir.path().join("auth.json");
        std::fs::write(
            &config_path,
            r#"{"mode":"localNetwork","port":41234,"allowControl":true}"#,
        )
        .expect("write enabled config");

        let (state, _, _) = test_state(config_path, auth_path);
        let config = state.config.read().clone();
        assert_eq!(config.mode, RemoteNetworkMode::LocalNetwork);
        assert_eq!(config.port, REMOTE_GATEWAY_PORT);
        assert!(config.allow_control);
        assert_eq!(
            config.restart_policy(),
            RemoteRestartPolicy::RestoreListener
        );
        assert!(!*state.is_running.read());
        assert!(state.bound_address.read().is_none());
    }

    /// Builds a lookup over an explicit variable set so the platform-specific resolution order
    /// can be exercised on any host without mutating process environment.
    fn lookup_from<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        |variable: &str| {
            pairs
                .iter()
                .find(|(key, _)| *key == variable)
                .map(|(_, value)| OsString::from(*value))
        }
    }

    #[test]
    fn explicit_data_dir_override_wins_over_platform_home() {
        let resolved = resolve_remote_data_dir(lookup_from(&[
            ("FERRYX_DATA_DIR", "/explicit/base"),
            ("HOME", "/home/user"),
            ("LOCALAPPDATA", r"C:\Users\user\AppData\Local"),
        ]))
        .expect("override must resolve");

        assert_eq!(resolved, PathBuf::from("/explicit/base").join("remote"));
    }

    #[test]
    fn credentials_are_never_placed_in_a_shared_scratch_directory() {
        // Windows leaves HOME unset, which previously fell through to `std::env::temp_dir()` and
        // wrote plaintext device tokens into the shared TEMP directory.
        assert!(
            resolve_remote_data_dir(lookup_from(&[("TMP", r"C:\Windows\Temp")])).is_none(),
            "with no per-user variable set, persistence must be declined rather than redirected"
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_prefers_localappdata_and_accepts_userprofile() {
        let localappdata = resolve_remote_data_dir(lookup_from(&[
            ("LOCALAPPDATA", r"C:\Users\user\AppData\Local"),
            ("USERPROFILE", r"C:\Users\user"),
        ]))
        .expect("LOCALAPPDATA must resolve");
        assert_eq!(
            localappdata,
            PathBuf::from(r"C:\Users\user\AppData\Local")
                .join("Ferryx")
                .join("remote")
        );

        let userprofile =
            resolve_remote_data_dir(lookup_from(&[("USERPROFILE", r"C:\Users\user")]))
                .expect("USERPROFILE must resolve when LOCALAPPDATA is absent");
        assert_eq!(
            userprofile,
            PathBuf::from(r"C:\Users\user")
                .join(".ferryx")
                .join("remote")
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn unix_resolves_under_home_and_ignores_windows_variables() {
        let resolved = resolve_remote_data_dir(lookup_from(&[("HOME", "/home/user")]))
            .expect("HOME must resolve");
        assert_eq!(
            resolved,
            PathBuf::from("/home/user").join(".ferryx").join("remote")
        );

        assert!(
            resolve_remote_data_dir(lookup_from(&[(
                "LOCALAPPDATA",
                r"C:\Users\user\AppData\Local"
            )]))
            .is_none(),
            "Windows-only variables must not be honored on Unix"
        );
    }

    #[tokio::test]
    async fn test_workspace_snapshot_expired_cache_awaits_rebuild_and_reflects_external_changes() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let repo_root = dir.path().join("repo");
        std::fs::create_dir_all(&repo_root).expect("create repo");
        crate::worktree::run_git(&repo_root, &["init"]).expect("git init");
        crate::worktree::run_git(
            &repo_root,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "--allow-empty",
                "-m",
                "initial",
            ],
        )
        .expect("git commit");

        let registry = crate::worktree::WorkspaceRegistry::new();
        let workspace_id = "ws-external-refresh-p16";
        registry
            .register(workspace_id, &repo_root)
            .expect("register");
        let state = Arc::new(RemoteGatewayState::new(
            Arc::new(TerminalService::default()),
            registry.clone(),
        ));
        let initial_time = std::time::Instant::now();

        // 1. Initial snapshot: cache populated, no external worktree
        let initial = state
            .workspace_snapshot_at(initial_time)
            .await
            .expect("initial snapshot");
        assert!(!initial
            .worktrees_for(workspace_id, None)
            .iter()
            .any(|worktree| worktree.worktree_label.as_deref() == Some("external-change")));
        assert_eq!(state.snapshot_build_count(), 1);

        // 2. External git worktree created via CLI (does not bump registry revision)
        let manager = registry.manager(workspace_id).expect("manager");
        let identity = crate::worktree::WorktreeIdentity {
            ws_id: workspace_id.to_string(),
            slug: "external-change".to_string(),
        };
        let external_path = manager
            .worktree_path_for(&identity.ws_id, &identity.slug)
            .expect("external path");
        let external_path_text = external_path.to_string_lossy().into_owned();
        crate::worktree::run_git(
            &repo_root,
            &[
                "worktree",
                "add",
                "-b",
                "external-change",
                &external_path_text,
            ],
        )
        .expect("external git worktree add");

        // Within refresh interval: served from cache (still without external-change)
        let before_interval = state
            .workspace_snapshot_at(
                initial_time + WORKSPACE_SNAPSHOT_REFRESH_INTERVAL / 2,
            )
            .await
            .expect("cached snapshot before expiry");
        assert!(!before_interval
            .worktrees_for(workspace_id, None)
            .iter()
            .any(|worktree| worktree.worktree_label.as_deref() == Some("external-change")));
        assert_eq!(state.snapshot_build_count(), 1);

        // 3. After refresh interval has expired:
        // P16 remediation: an explicit list request must immediately reflect the external change
        // rather than returning an intentionally stale snapshot and postponing freshness to the next request.
        let after_interval = state
            .workspace_snapshot_at(
                initial_time + WORKSPACE_SNAPSHOT_REFRESH_INTERVAL,
            )
            .await
            .expect("snapshot after refresh interval");
        assert!(
            after_interval
                .worktrees_for(workspace_id, None)
                .iter()
                .any(|worktree| worktree.worktree_label.as_deref() == Some("external-change")),
            "P16: snapshot request after refresh interval expired must reflect external worktree changes immediately"
        );
        assert_eq!(state.snapshot_build_count(), 2);

        // 4. Now simulate external removal via CLI (again does not bump registry revision)
        crate::worktree::run_git(
            &repo_root,
            &["worktree", "remove", "--force", &external_path_text],
        )
        .expect("external git worktree remove");

        // After another refresh interval, removal must be immediately reflected
        let after_removal = state
            .workspace_snapshot_at(
                initial_time + WORKSPACE_SNAPSHOT_REFRESH_INTERVAL * 2,
            )
            .await
            .expect("snapshot after removal interval");
        assert!(
            !after_removal
                .worktrees_for(workspace_id, None)
                .iter()
                .any(|worktree| worktree.worktree_label.as_deref() == Some("external-change")),
            "P16: snapshot request after removal and refresh interval expired must reflect removal immediately"
        );
        assert_eq!(state.snapshot_build_count(), 3);
    }
}
