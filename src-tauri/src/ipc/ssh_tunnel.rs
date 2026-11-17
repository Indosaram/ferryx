//! GUI-side SSH tunnel commands: seven thin wrappers over the daemon client.
//!
//! Tunnel definitions live in `ssh_tunnels.json`, which only the daemon writes. This
//! module therefore opens no path and takes no store lock: every mutating verb is
//! forwarded (`SpecsSave`, `SpecsReset`, `InvalidateHost`, `ForgetHost`) and the
//! daemon's dispatch arm performs the single write inside the tunnel store module.
//! A write here would be the second writer the store design forbids, so the absence
//! of a filesystem import in this module is enforced by
//! `no_second_store_writer_from_the_gui`.
//!
//! Three refusals precede any forward, in this order: platform first (Windows never
//! advertises the capability, so a capability gate first would make the platform
//! refusal unreachable), then the `sshTunnelV1` capability, then the remote-gateway
//! route. The tunnel classification travels in `details.failureCode` (or
//! `details.reason`), never in `IpcError::code`, because `code` is always an
//! `IpcErrorCode`.
use crate::ipc::{IpcError, IpcErrorCode};
use crate::ssh::tunnel::{
    validate_saved_spec, validate_specs_for_host, SpecError, TunnelHostIdentity,
};
use crate::ssh::SshHost;
use std::collections::HashMap;
use std::sync::Mutex;

/// The one capability a daemon must advertise before any tunnel verb is forwarded.
pub use crate::daemon::protocol::SSH_TUNNEL_CAPABILITY;

/// Wire code carried in `details.failureCode` for the Windows refusal.
pub const UNSUPPORTED_PLATFORM: &str = "unsupported_platform";
/// `details.reason` for the remote-gateway route refusal.
pub const REMOTE_GATEWAY_ROUTE: &str = "remote_gateway_route";
/// `details.reason` for a malformed GUI-sent host id.
pub const INVALID_HOST_ID: &str = "invalid_host_id";
/// `details.failureCode` for a duplicate local port caught before forwarding.
pub const PORT_IN_USE: &str = "port_in_use";
/// `details.reason` the daemon uses when a reset targets a file that parses.
pub const STORE_NOT_CORRUPT: &str = "store_not_corrupt";

/// Whether the connected daemon is the local one or a remote-gateway route.
///
/// A remote-gateway connection has no tunnel registry behind it, so its requests are
/// refused up front instead of being relayed to a peer that cannot serve them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TunnelRoute {
    Local,
    RemoteGateway,
}

/// The target platform as this layer sees it. A parameter, not `cfg!` at the comparison
/// site, so the precedence rule is testable on every build target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TunnelPlatform {
    Unix,
    Windows,
}

impl TunnelPlatform {
    pub fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::Unix
        }
    }
}

/// A refusal decided before any tunnel request is forwarded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    UnsupportedPlatform,
    MissingCapability,
    RemoteGatewayRoute,
}

impl Refusal {
    pub fn into_error(self) -> IpcError {
        match self {
            // Serialised code is `UNSUPPORTED`; the classification lives in details.
            Self::UnsupportedPlatform => IpcError::new(
                IpcErrorCode::Unsupported,
                "SSH tunnels need a Unix control socket, which Windows does not provide",
            )
            .with_details(serde_json::json!({
                "failureCode": UNSUPPORTED_PLATFORM,
                "platform": "windows",
            })),
            Self::MissingCapability => IpcError::new(
                IpcErrorCode::UnsupportedCapability,
                "SSH tunnels need a newer Ferryx background service",
            )
            .with_details(serde_json::json!({ "capability": SSH_TUNNEL_CAPABILITY })),
            Self::RemoteGatewayRoute => IpcError::new(
                IpcErrorCode::Unsupported,
                "SSH tunnels are managed by the local background service, not the remote gateway",
            )
            .with_details(serde_json::json!({ "reason": REMOTE_GATEWAY_ROUTE })),
        }
    }
}

/// The platform half of the gate, on its own.
///
/// Take this verdict before asking the daemon anything: `gate` calls it first, so a
/// Windows build refuses without a capabilities round trip and the platform refusal
/// cannot be replaced by the capability one.
pub fn platform_refusal(platform: TunnelPlatform) -> Option<Refusal> {
    (platform == TunnelPlatform::Windows).then_some(Refusal::UnsupportedPlatform)
}

/// Platform precedence, as one pure predicate.
///
/// Windows answers `UnsupportedPlatform` whether or not the capability is present, so a
/// capability gate can never mask the platform refusal. On Unix a missing capability is
/// the only refusal this predicate owns.
pub fn refuse_platform_before_capability(
    platform: TunnelPlatform,
    capabilities: &[String],
) -> Option<Refusal> {
    match platform {
        TunnelPlatform::Windows => Some(Refusal::UnsupportedPlatform),
        TunnelPlatform::Unix => (!capabilities
            .iter()
            .any(|capability| capability.as_str() == SSH_TUNNEL_CAPABILITY))
        .then_some(Refusal::MissingCapability),
    }
}

/// A GUI-sent host id must be non-empty and must not start with `-`, so it can never be
/// mistaken for a flag by a downstream argv builder.
pub fn validate_host_id(host_id: &str) -> Result<(), IpcError> {
    if host_id.trim().is_empty() || host_id.starts_with('-') {
        return Err(
            IpcError::new(IpcErrorCode::InvalidArgument, "Invalid tunnel host id")
                .with_details(serde_json::json!({ "reason": INVALID_HOST_ID })),
        );
    }
    Ok(())
}

/// One semantic daemon operation. The impl owns protocol detail; the command layer stays
/// a wrapper.
#[derive(Debug, Clone)]
pub enum TunnelCall {
    Capabilities,
    List,
    Start(crate::daemon::protocol::TunnelStartRequest),
    Stop(crate::daemon::protocol::TunnelStopRequest),
    InvalidateHost(crate::daemon::protocol::TunnelInvalidateHostRequest),
    ForgetHost(crate::daemon::protocol::TunnelForgetHostRequest),
    SpecsSave(crate::daemon::protocol::TunnelSpecsSaveRequest),
    SpecsReset,
}

#[derive(Debug, Clone)]
pub enum TunnelReply {
    Capabilities {
        capabilities: Vec<String>,
        route: TunnelRoute,
    },
    List(crate::daemon::protocol::TunnelListResponse),
    Start(crate::daemon::protocol::TunnelStartResponse),
    Stop(crate::daemon::protocol::TunnelStopResponse),
    Invalidated(crate::daemon::protocol::TunnelInvalidateHostResponse),
    Forgotten(crate::daemon::protocol::TunnelForgetHostResponse),
    SpecsSaved(crate::daemon::protocol::TunnelSpecsSaveResponse),
    SpecsReset(crate::daemon::protocol::TunnelSpecsResetResponse),
}

pub type TunnelFuture<'a> =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<TunnelReply, IpcError>> + Send + 'a>>;

/// The daemon-facing surface all seven commands need.
///
/// A trait rather than a concrete `DaemonClient` so the command layer can be driven by a
/// counting fake: a forwarded call is then an observation, never a belief.
pub trait TunnelDaemon: Send + Sync {
    fn call(&self, call: TunnelCall) -> TunnelFuture<'_>;
}

#[cfg(feature = "desktop")]
impl TunnelDaemon for crate::daemon::client::DaemonClient {
    fn call(&self, call: TunnelCall) -> TunnelFuture<'_> {
        use crate::daemon::protocol::{DaemonRequest, DaemonResponse};
        Box::pin(async move {
            match call {
                TunnelCall::Capabilities => {
                    match self.send_request(DaemonRequest::GetCapabilities).await? {
                        // The GUI's control connection is the local daemon; a gateway
                        // connection supplies its own `TunnelDaemon` implementation.
                        DaemonResponse::CapabilitiesOk { capabilities } => Ok(
                            TunnelReply::Capabilities { capabilities, route: TunnelRoute::Local },
                        ),
                        _ => Err(IpcError::internal(
                            "Daemon answered a capabilities request unexpectedly",
                        )),
                    }
                }
                TunnelCall::List => Ok(TunnelReply::List(self.tunnel_list().await?)),
                TunnelCall::Start(request) => {
                    Ok(TunnelReply::Start(self.tunnel_start(request).await?))
                }
                TunnelCall::Stop(request) => Ok(TunnelReply::Stop(self.tunnel_stop(request).await?)),
                TunnelCall::InvalidateHost(request) => Ok(TunnelReply::Invalidated(
                    self.tunnel_invalidate_host(request).await?,
                )),
                TunnelCall::ForgetHost(request) => {
                    Ok(TunnelReply::Forgotten(self.tunnel_forget_host(request).await?))
                }
                TunnelCall::SpecsSave(request) => {
                    Ok(TunnelReply::SpecsSaved(self.tunnel_specs_save(request).await?))
                }
                TunnelCall::SpecsReset => {
                    Ok(TunnelReply::SpecsReset(self.tunnel_specs_reset().await?))
                }
            }
        })
    }
}

fn unexpected(verb: &str) -> IpcError {
    IpcError::internal(format!("Daemon answered {verb} unexpectedly"))
}

/// Reads the advertised capabilities and route. A failure to ask is a refusal, never a
/// silent proceed: forwarding to an unknown daemon is the case this gate exists for.
async fn capability_state<C: TunnelDaemon + ?Sized>(
    client: &C,
) -> Result<(Vec<String>, TunnelRoute), IpcError> {
    match client.call(TunnelCall::Capabilities).await? {
        TunnelReply::Capabilities { capabilities, route } => Ok((capabilities, route)),
        _ => Err(unexpected("a capabilities request")),
    }
}

/// The gate every command runs before forwarding.
pub async fn gate<C: TunnelDaemon + ?Sized>(client: &C) -> Result<(), IpcError> {
    let platform = TunnelPlatform::current();
    // Windows: refuse without asking anything, so no forward precedes it and an
    // unadvertised capability can never turn the platform refusal into something else.
    if let Some(refusal) = platform_refusal(platform) {
        return Err(refusal.into_error());
    }
    let (capabilities, route) = capability_state(client).await?;
    if let Some(refusal) = refuse_platform_before_capability(platform, &capabilities) {
        return Err(refusal.into_error());
    }
    if route == TunnelRoute::RemoteGateway {
        return Err(Refusal::RemoteGatewayRoute.into_error());
    }
    Ok(())
}

fn expect_list(
    reply: TunnelReply,
) -> Result<crate::daemon::protocol::TunnelListResponse, IpcError> {
    match reply {
        TunnelReply::List(response) => Ok(response),
        _ => Err(unexpected("list")),
    }
}

/// The revision the caller last saw for `host_id`; 0 when the host is unknown, which is
/// also what the daemon reads for a host it has never bumped.
pub async fn observed_revision<C: TunnelDaemon + ?Sized>(
    client: &C,
    host_id: &str,
) -> Result<u64, IpcError> {
    let list = expect_list(client.call(TunnelCall::List).await?)?;
    Ok(list
        .host_revisions
        .iter()
        .find(|entry| entry.host_id == host_id)
        .map(|entry| entry.revision)
        .unwrap_or(0))
}

// ---------------------------------------------------------------------------
// The seven commands' cores.
// ---------------------------------------------------------------------------

pub async fn list_with<C: TunnelDaemon + ?Sized>(
    client: &C,
) -> Result<crate::daemon::protocol::TunnelListResponse, IpcError> {
    gate(client).await?;
    expect_list(client.call(TunnelCall::List).await?)
}

pub async fn start_with<C: TunnelDaemon + ?Sized>(
    client: &C,
    request: crate::daemon::protocol::TunnelStartRequest,
) -> Result<crate::daemon::protocol::TunnelStartResponse, IpcError> {
    validate_host_id(&request.host_id)?;
    gate(client).await?;
    match client.call(TunnelCall::Start(request)).await? {
        TunnelReply::Start(response) => Ok(response),
        _ => Err(unexpected("start")),
    }
}

pub async fn stop_with<C: TunnelDaemon + ?Sized>(
    client: &C,
    request: crate::daemon::protocol::TunnelStopRequest,
) -> Result<crate::daemon::protocol::TunnelStopResponse, IpcError> {
    gate(client).await?;
    match client.call(TunnelCall::Stop(request)).await? {
        TunnelReply::Stop(response) => Ok(response),
        _ => Err(unexpected("stop")),
    }
}

pub async fn invalidate_host_with<C: TunnelDaemon + ?Sized>(
    client: &C,
    host_id: String,
    host_revision: u64,
    identity: Option<TunnelHostIdentity>,
) -> Result<crate::daemon::protocol::TunnelInvalidateHostResponse, IpcError> {
    validate_host_id(&host_id)?;
    gate(client).await?;
    let request = crate::daemon::protocol::TunnelInvalidateHostRequest {
        host_id,
        host_revision,
        identity,
    };
    match client.call(TunnelCall::InvalidateHost(request)).await? {
        TunnelReply::Invalidated(response) => Ok(response),
        _ => Err(unexpected("invalidate")),
    }
}

pub async fn forget_host_with<C: TunnelDaemon + ?Sized>(
    client: &C,
    host_id: String,
) -> Result<crate::daemon::protocol::TunnelForgetHostResponse, IpcError> {
    validate_host_id(&host_id)?;
    gate(client).await?;
    let request = crate::daemon::protocol::TunnelForgetHostRequest { host_id };
    match client.call(TunnelCall::ForgetHost(request)).await? {
        TunnelReply::Forgotten(response) => Ok(response),
        _ => Err(unexpected("forget")),
    }
}

/// The definitions file is owned by the daemon, so `reset` only forwards: the arm takes
/// the sidecar lock, re-checks the file is still unparseable, quarantines it and answers
/// the path, which this command returns verbatim.
pub async fn specs_reset_with<C: TunnelDaemon + ?Sized>(
    client: &C,
) -> Result<crate::daemon::protocol::TunnelSpecsResetResponse, IpcError> {
    gate(client).await?;
    match client.call(TunnelCall::SpecsReset).await {
        Ok(TunnelReply::SpecsReset(response)) => Ok(response),
        Ok(_) => Err(unexpected("reset")),
        // Refusing to quarantine a healthy file is a conflict, not an internal error.
        Err(error) => Err(map_reset_error(error)),
    }
}

fn map_reset_error(error: IpcError) -> IpcError {
    let reason = error
        .details
        .as_ref()
        .and_then(|details| details.get("reason"))
        .and_then(serde_json::Value::as_str);
    match reason {
        Some(STORE_NOT_CORRUPT) => {
            IpcError::new(IpcErrorCode::RequestConflict, error.message)
                .with_details(serde_json::json!({ "reason": STORE_NOT_CORRUPT }))
        }
        _ => error,
    }
}

/// Duplicate local ports and malformed specs are refused here, before the daemon sees
/// them, so the caller gets the structured pair without a round trip and an old daemon
/// cannot accept a definition set the GUI already knows is invalid.
pub fn validate_save_request(
    request: &crate::daemon::protocol::TunnelSpecsSaveRequest,
) -> Result<(), IpcError> {
    validate_host_id(&request.host_id)?;
    for spec in &request.specs {
        validate_saved_spec(spec)?;
    }
    for error in validate_specs_for_host(&request.specs) {
        return Err(match error {
            // The only refusal in the plan that carries `port_in_use`; a start-time
            // `port_in_use` is an Ok + Failed view and is never an Err.
            SpecError::DuplicateLocalPort { local_port, .. } => IpcError::new(
                IpcErrorCode::InvalidArgument,
                "Two tunnels cannot share a local port",
            )
            .with_details(serde_json::json!({
                "failureCode": PORT_IN_USE,
                "localPort": local_port,
            })),
            SpecError::DuplicateId { id, .. } => IpcError::new(
                IpcErrorCode::InvalidArgument,
                "Two tunnels cannot share an id",
            )
            .with_details(serde_json::json!({
                "failureCode": "duplicate_spec_id",
                "specId": id.0,
            })),
        });
    }
    Ok(())
}

pub async fn specs_save_with<C: TunnelDaemon + ?Sized>(
    client: &C,
    request: crate::daemon::protocol::TunnelSpecsSaveRequest,
) -> Result<crate::daemon::protocol::TunnelSpecsSaveResponse, IpcError> {
    validate_save_request(&request)?;
    gate(client).await?;
    match client.call(TunnelCall::SpecsSave(request)).await? {
        TunnelReply::SpecsSaved(response) => Ok(response),
        _ => Err(unexpected("save")),
    }
}

// ---------------------------------------------------------------------------
// Host-mutation coordination.
//
// The daemon owns the invariant; this layer only sends the barrier. A mutation that
// changes a host's credential identity (or its disabled flag) sends `InvalidateHost`
// exactly once with the post-mutation tuple, which bumps the host revision and tears
// down that host's runtimes. A label or shell edit sends nothing. A delete sends
// `ForgetHost`, because a delete must not leave definitions behind to reattach to a
// later host that reuses the id.
//
// A tunnel service that is absent, unreachable or failing never blocks host
// management: every failure here is logged and swallowed, and the host store write
// still happens.
// ---------------------------------------------------------------------------

/// The identity tuple an invalidation records.
pub fn host_identity(host: &SshHost) -> TunnelHostIdentity {
    TunnelHostIdentity {
        hostname: host.hostname.clone(),
        username: host.username.clone(),
        port: host.port,
        identity_file: host.identity_file.clone(),
        jump_host: host.jump_host.clone(),
        disabled: host.disabled,
    }
}

/// The post-mutation tuple when it differs from the stored one. `None` covers an
/// unchanged identity (label/shell edit) and a host that did not exist before, where
/// there is nothing to tear down.
pub fn changed_identity(
    previous: Option<&SshHost>,
    next: &SshHost,
) -> Option<TunnelHostIdentity> {
    let previous = previous?;
    (host_identity(previous) != host_identity(next)).then(|| host_identity(next))
}

/// One entry per host present in both sets whose identity tuple changed, for the bulk
/// writers (config import, Tailscale sync) that rewrite the host store wholesale.
pub fn changed_identities(
    previous: &[SshHost],
    next: &[SshHost],
) -> Vec<(String, TunnelHostIdentity)> {
    next.iter()
        .filter_map(|host| {
            let before = previous.iter().find(|candidate| candidate.id == host.id)?;
            (host_identity(before) != host_identity(host))
                .then(|| (host.id.clone(), host_identity(host)))
        })
        .collect()
}

pub async fn invalidate_after_identity_change<C: TunnelDaemon + ?Sized>(
    client: &C,
    host_id: &str,
    identity: TunnelHostIdentity,
) -> Result<u64, IpcError> {
    let host_revision = observed_revision(client, host_id).await?;
    let request = crate::daemon::protocol::TunnelInvalidateHostRequest {
        host_id: host_id.to_string(),
        host_revision,
        identity: Some(identity),
    };
    match client.call(TunnelCall::InvalidateHost(request)).await? {
        TunnelReply::Invalidated(response) => Ok(response.revision),
        _ => Err(unexpected("invalidate")),
    }
}

/// Sends the barrier for one host, swallowing refusals and failures.
///
/// Returns the bumped revision when the barrier landed, `None` when the tunnel service
/// is absent (old daemon, Windows), routed elsewhere, or failing. The caller keeps
/// saving the host either way.
pub async fn coordinate_identity_change<C: TunnelDaemon + ?Sized>(
    client: &C,
    host_id: &str,
    identity: TunnelHostIdentity,
) -> Option<u64> {
    if let Err(refusal) = gate(client).await {
        tracing::debug!(
            host_id = %host_id,
            code = ?refusal.code,
            "Skipping tunnel invalidation for a host mutation"
        );
        return None;
    }
    match invalidate_after_identity_change(client, host_id, identity).await {
        Ok(revision) => Some(revision),
        Err(error) => {
            tracing::warn!(
                host_id = %host_id,
                reason = ?error.details,
                "Tunnel invalidation failed; the host mutation still succeeds"
            );
            None
        }
    }
}

/// Same contract for the delete path: the definitions key goes away with the host.
pub async fn coordinate_host_forget<C: TunnelDaemon + ?Sized>(
    client: &C,
    host_id: &str,
) -> Option<u32> {
    if let Err(refusal) = gate(client).await {
        tracing::debug!(host_id = %host_id, code = ?refusal.code, "Skipping tunnel forget for a host delete");
        return None;
    }
    let request = crate::daemon::protocol::TunnelForgetHostRequest { host_id: host_id.to_string() };
    match client.call(TunnelCall::ForgetHost(request)).await {
        Ok(TunnelReply::Forgotten(response)) => Some(response.removed),
        Ok(_) => None,
        Err(error) => {
            tracing::warn!(
                host_id = %host_id,
                reason = ?error.details,
                "Tunnel forget failed; the host delete still succeeds"
            );
            None
        }
    }
}

/// Compensating barrier after a failed host-store write: the daemon recorded the
/// post-mutation tuple, but the file still holds the old one, so every later start at
/// the new revision would refuse forever. Sending the tuple that is actually on disk
/// restores agreement. A failed compensation is remembered, never swallowed.
pub async fn compensate_failed_write<C: TunnelDaemon + ?Sized>(
    client: &C,
    host_id: &str,
    on_disk: TunnelHostIdentity,
    pending: &Mutex<HashMap<String, TunnelHostIdentity>>,
) -> Result<u64, IpcError> {
    match invalidate_after_identity_change(client, host_id, on_disk.clone()).await {
        Ok(revision) => Ok(revision),
        Err(error) => {
            pending
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .insert(host_id.to_string(), on_disk);
            Err(error)
        }
    }
}

/// Retries compensations that previously failed. Runs before the next host save's own
/// barrier, so a transient failure heals on the next edit instead of stranding a host.
pub async fn retry_pending_compensations<C: TunnelDaemon + ?Sized>(
    client: &C,
    pending: &Mutex<HashMap<String, TunnelHostIdentity>>,
) -> usize {
    let outstanding: Vec<(String, TunnelHostIdentity)> = {
        let held = pending.lock().unwrap_or_else(|poison| poison.into_inner());
        held.iter()
            .map(|(host_id, identity)| (host_id.clone(), identity.clone()))
            .collect()
    };
    let mut healed = 0;
    for (host_id, identity) in outstanding {
        if invalidate_after_identity_change(client, &host_id, identity).await.is_ok() {
            pending
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .remove(&host_id);
            healed += 1;
        }
    }
    healed
}

/// Compensations that could not be delivered yet.
///
/// GUI state rather than disk state: it is retried before the next host save's barrier
/// and lost with the process, which is the same lifetime as the revision pairs it repairs.
pub fn pending_compensations() -> &'static Mutex<HashMap<String, TunnelHostIdentity>> {
    static PENDING: std::sync::OnceLock<Mutex<HashMap<String, TunnelHostIdentity>>> =
        std::sync::OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Records a compensation that could not be delivered, for retry on the next host write.
pub fn remember_pending_compensation(host_id: &str, identity: TunnelHostIdentity) {
    pending_compensations()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .insert(host_id.to_string(), identity);
}

/// The pending compensation for one host, if any.
pub fn pending_compensation(host_id: &str) -> Option<TunnelHostIdentity> {
    pending_compensations()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .get(host_id)
        .cloned()
}

/// Compensating barrier for a real store-write failure, recorded in process state when
/// it also fails so the next host write can retry it.
pub async fn compensate_failed_store_write<C: TunnelDaemon + ?Sized>(
    client: &C,
    host_id: &str,
    on_disk: TunnelHostIdentity,
) -> Result<u64, IpcError> {
    match invalidate_after_identity_change(client, host_id, on_disk.clone()).await {
        Ok(revision) => Ok(revision),
        Err(error) => {
            remember_pending_compensation(host_id, on_disk);
            Err(error)
        }
    }
}

/// Retries the process-wide pending compensations; returns how many healed.
pub async fn retry_pending_compensations_now<C: TunnelDaemon + ?Sized>(client: &C) -> usize {
    let outstanding: Vec<(String, TunnelHostIdentity)> = pending_compensations()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .iter()
        .map(|(host_id, identity)| (host_id.clone(), identity.clone()))
        .collect();
    let mut healed = 0;
    for (host_id, identity) in outstanding {
        if invalidate_after_identity_change(client, &host_id, identity).await.is_ok() {
            pending_compensations()
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .remove(&host_id);
            healed += 1;
        }
    }
    healed
}

// ---------------------------------------------------------------------------
// Tauri commands: each one a thin wrapper over its core.
// ---------------------------------------------------------------------------

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_ssh_tunnel_list(
    daemon_client: tauri::State<'_, std::sync::Arc<crate::daemon::client::DaemonClient>>,
) -> Result<crate::daemon::protocol::TunnelListResponse, IpcError> {
    list_with(daemon_client.inner().as_ref()).await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_ssh_tunnel_start(
    daemon_client: tauri::State<'_, std::sync::Arc<crate::daemon::client::DaemonClient>>,
    request: crate::daemon::protocol::TunnelStartRequest,
) -> Result<crate::daemon::protocol::TunnelStartResponse, IpcError> {
    start_with(daemon_client.inner().as_ref(), request).await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_ssh_tunnel_stop(
    daemon_client: tauri::State<'_, std::sync::Arc<crate::daemon::client::DaemonClient>>,
    request: crate::daemon::protocol::TunnelStopRequest,
) -> Result<crate::daemon::protocol::TunnelStopResponse, IpcError> {
    stop_with(daemon_client.inner().as_ref(), request).await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_ssh_tunnel_invalidate_host(
    daemon_client: tauri::State<'_, std::sync::Arc<crate::daemon::client::DaemonClient>>,
    host_id: String,
    host_revision: u64,
    identity: Option<TunnelHostIdentity>,
) -> Result<crate::daemon::protocol::TunnelInvalidateHostResponse, IpcError> {
    invalidate_host_with(daemon_client.inner().as_ref(), host_id, host_revision, identity).await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_ssh_tunnel_specs_reset(
    daemon_client: tauri::State<'_, std::sync::Arc<crate::daemon::client::DaemonClient>>,
) -> Result<crate::daemon::protocol::TunnelSpecsResetResponse, IpcError> {
    specs_reset_with(daemon_client.inner().as_ref()).await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_ssh_tunnel_forget_host(
    daemon_client: tauri::State<'_, std::sync::Arc<crate::daemon::client::DaemonClient>>,
    host_id: String,
) -> Result<crate::daemon::protocol::TunnelForgetHostResponse, IpcError> {
    forget_host_with(daemon_client.inner().as_ref(), host_id).await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_ssh_tunnel_specs_save(
    daemon_client: tauri::State<'_, std::sync::Arc<crate::daemon::client::DaemonClient>>,
    request: crate::daemon::protocol::TunnelSpecsSaveRequest,
) -> Result<crate::daemon::protocol::TunnelSpecsSaveResponse, IpcError> {
    specs_save_with(daemon_client.inner().as_ref(), request).await
}

// ---------------------------------------------------------------------------
// Tests: `--lib ipc::ssh_tunnel`.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::protocol::{
        HostRevision, StoreStatus, StoreStatusKind, TunnelForgetHostRequest,
        TunnelForgetHostResponse, TunnelInvalidateHostRequest, TunnelInvalidateHostResponse,
        TunnelListResponse, TunnelSpecsResetResponse, TunnelSpecsSaveRequest,
        TunnelSpecsSaveResponse, TunnelStartRequest, TunnelStartResponse, TunnelStopRequest,
        TunnelStopResponse,
    };
    use crate::ssh::tunnel::{TunnelRuntimeId, TunnelSpec, TunnelSpecId, TunnelState, TunnelView};
    use crate::ssh::{SshAuthMethod, SshHostSource};
    use std::sync::atomic::{AtomicBool, Ordering};

    #[derive(Default)]
    struct FakeDaemon {
        capabilities: Vec<String>,
        route: Option<TunnelRoute>,
        calls: Mutex<Vec<TunnelCall>>,
        revisions: Mutex<HashMap<String, u64>>,
        invalidate_error: Mutex<Option<IpcError>>,
        forget_error: Mutex<Option<IpcError>>,
        reset: Mutex<Option<Result<TunnelSpecsResetResponse, IpcError>>>,
        fail_next_invalidate: AtomicBool,
    }

    impl FakeDaemon {
        fn advertised() -> Self {
            Self {
                capabilities: vec![SSH_TUNNEL_CAPABILITY.to_string()],
                route: Some(TunnelRoute::Local),
                ..Self::default()
            }
        }

        fn without_capability() -> Self {
            Self { capabilities: Vec::new(), route: Some(TunnelRoute::Local), ..Self::default() }
        }

        fn calls(&self) -> Vec<TunnelCall> {
            self.calls.lock().unwrap().clone()
        }

        fn invalidations(&self) -> Vec<TunnelInvalidateHostRequest> {
            self.calls()
                .into_iter()
                .filter_map(|call| match call {
                    TunnelCall::InvalidateHost(request) => Some(request),
                    _ => None,
                })
                .collect()
        }

        fn forgets(&self) -> Vec<TunnelForgetHostRequest> {
            self.calls()
                .into_iter()
                .filter_map(|call| match call {
                    TunnelCall::ForgetHost(request) => Some(request),
                    _ => None,
                })
                .collect()
        }

        fn list_response(&self) -> TunnelListResponse {
            let host_revisions = self
                .revisions
                .lock()
                .unwrap()
                .iter()
                .map(|(host_id, revision)| HostRevision {
                    host_id: host_id.clone(),
                    revision: *revision,
                })
                .collect();
            TunnelListResponse {
                views: Vec::new(),
                definitions: Vec::new(),
                host_revisions,
                definitions_revision: 1,
                store_status: StoreStatus {
                    kind: StoreStatusKind::Ok,
                    path: String::new(),
                    invalid_hosts: Vec::new(),
                    quarantined_path: None,
                },
            }
        }
    }

    impl TunnelDaemon for FakeDaemon {
        fn call(&self, call: TunnelCall) -> TunnelFuture<'_> {
            Box::pin(async move {
                self.calls.lock().unwrap().push(call.clone());
                match call {
                    TunnelCall::Capabilities => Ok(TunnelReply::Capabilities {
                        capabilities: self.capabilities.clone(),
                        route: self.route.unwrap_or(TunnelRoute::Local),
                    }),
                    TunnelCall::List => Ok(TunnelReply::List(self.list_response())),
                    TunnelCall::Start(request) => Ok(TunnelReply::Start(TunnelStartResponse {
                        view: fake_view(&request),
                        accepted_revision: 1,
                    })),
                    TunnelCall::Stop(request) => Ok(TunnelReply::Stop(TunnelStopResponse {
                        stopped: !request.runtime_id.is_empty(),
                    })),
                    TunnelCall::InvalidateHost(request) => {
                        if self.fail_next_invalidate.swap(false, Ordering::SeqCst) {
                            return Err(IpcError::new(IpcErrorCode::IoError, "daemon unreachable")
                                .with_details(serde_json::json!({ "reason": "transport" })));
                        }
                        if let Some(error) = self.invalidate_error.lock().unwrap().clone() {
                            return Err(error);
                        }
                        let bumped = {
                            let mut revisions = self.revisions.lock().unwrap();
                            let next = revisions
                                .get(&request.host_id)
                                .copied()
                                .unwrap_or(0)
                                .max(request.host_revision)
                                + 1;
                            revisions.insert(request.host_id.clone(), next);
                            next
                        };
                        Ok(TunnelReply::Invalidated(TunnelInvalidateHostResponse { revision: bumped }))
                    }
                    TunnelCall::ForgetHost(request) => {
                        if let Some(error) = self.forget_error.lock().unwrap().clone() {
                            return Err(error);
                        }
                        let removed = self.revisions.lock().unwrap().remove(&request.host_id);
                        Ok(TunnelReply::Forgotten(TunnelForgetHostResponse {
                            removed: if removed.is_some() { 1 } else { 0 },
                        }))
                    }
                    TunnelCall::SpecsSave(request) => Ok(TunnelReply::SpecsSaved(
                        TunnelSpecsSaveResponse { definitions_revision: request.specs.len() as u64 },
                    )),
                    TunnelCall::SpecsReset => match self.reset.lock().unwrap().take() {
                        Some(outcome) => outcome.map(TunnelReply::SpecsReset),
                        None => Ok(TunnelReply::SpecsReset(TunnelSpecsResetResponse {
                            quarantined_path: None,
                        })),
                    },
                }
            })
        }
    }

    fn fake_view(request: &TunnelStartRequest) -> TunnelView {
        let spec = request.spec.clone().expect("the fake start needs an inline spec");
        TunnelView {
            spec_id: spec.id.clone(),
            runtime_id: Some(TunnelRuntimeId("runtime-1".into())),
            label: spec.label.clone(),
            host_id: request.host_id.clone(),
            local_port: spec.local_port.unwrap_or(1),
            remote_host: spec.remote_host.clone(),
            remote_port: spec.remote_port,
            state: TunnelState::Listening,
            since: Some(0),
            attempt: 1,
            failure: None,
            failure_piece: None,
        }
    }

    fn spec(id: &str, local_port: u16) -> TunnelSpec {
        TunnelSpec {
            id: TunnelSpecId(id.to_string()),
            label: format!("tunnel {id}"),
            local_port: Some(local_port),
            remote_host: "db.internal".to_string(),
            remote_port: 5432,
            enabled: true,
        }
    }

    fn host(id: &str) -> SshHost {
        SshHost {
            id: id.to_string(),
            label: "db".to_string(),
            hostname: "db.internal".to_string(),
            username: Some("alice".to_string()),
            port: Some(2222),
            identity_file: Some("~/.ssh/id_test".to_string()),
            jump_host: Some("bastion".to_string()),
            source: SshHostSource::Manual,
            auth_method: SshAuthMethod::Key,
            disabled: None,
            shell: None,
        }
    }

    fn save_request(host_id: &str, specs: Vec<TunnelSpec>) -> TunnelSpecsSaveRequest {
        TunnelSpecsSaveRequest {
            host_id: host_id.to_string(),
            specs,
            expected_revision: Some(1),
            force: false,
            discard_invalid: false,
        }
    }

    // (a) The command pre-validates, so the fake records ZERO forwarded calls and the
    // caller still gets the structured pair Todo 10's bridge fixture pins.
    #[tokio::test]
    async fn specs_save_prevalidates_duplicate_port() {
        let fake = FakeDaemon::advertised();
        let error = specs_save_with(
            &fake,
            save_request("host-a", vec![spec("one", 5433), spec("two", 5433)]),
        )
        .await
        .expect_err("a duplicate local port must be refused before forwarding");
        assert_eq!(error.code, IpcErrorCode::InvalidArgument);
        let details = error.details.expect("structured refusal");
        assert_eq!(details["failureCode"], PORT_IN_USE);
        assert_eq!(details["localPort"], serde_json::json!(5433));
        assert!(fake.calls().is_empty(), "pre-validation must not forward anything");
    }

    // (b) Every host-mutation row: counted calls, not beliefs.
    #[tokio::test]
    async fn host_mutation_invalidation_is_counted() {
        let fake = FakeDaemon::advertised();
        fake.revisions.lock().unwrap().insert("host-a".into(), 4);

        // b1: a label or shell edit invalidates nothing.
        let mut relabelled = host("host-a");
        relabelled.label = "renamed".into();
        relabelled.shell = Some("/bin/zsh".into());
        assert!(changed_identity(Some(&host("host-a")), &relabelled).is_none());
        assert_eq!(fake.invalidations().len(), 0);

        // b2: a disable, and each identity field, invalidates exactly once for that host
        // carrying the revision the command observed.
        let mutations: [fn(&mut SshHost); 6] = [
            |h| h.disabled = Some(true),
            |h| h.hostname = "other.internal".into(),
            |h| h.username = Some("bob".into()),
            |h| h.port = Some(2200),
            |h| h.identity_file = Some("~/.ssh/id_other".into()),
            |h| h.jump_host = Some("other-bastion".into()),
        ];
        let mut expected_revision = 5;
        for mutate in mutations {
            let mut next = host("host-a");
            mutate(&mut next);
            let changed = changed_identity(Some(&host("host-a")), &next)
                .expect("an identity change must be detected");
            let before = fake.invalidations().len();
            let revision = coordinate_identity_change(&fake, "host-a", changed.clone())
                .await
                .expect("the barrier lands");
            let invalidations = fake.invalidations();
            assert_eq!(invalidations.len(), before + 1, "exactly one invalidation");
            let forwarded = invalidations.last().unwrap();
            assert_eq!(forwarded.host_id, "host-a");
            assert_eq!(forwarded.host_revision, expected_revision - 1);
            assert_eq!(forwarded.identity.as_ref(), Some(&changed));
            assert_eq!(revision, expected_revision);
            expected_revision += 1;
        }

        // b3: a delete forgets the host's definitions exactly once.
        let removed = coordinate_host_forget(&fake, "host-a").await.expect("the forget lands");
        assert_eq!(removed, 1);
        assert_eq!(fake.forgets().len(), 1);
        assert_eq!(fake.forgets()[0].host_id, "host-a");

        // b4: a bulk write that changes one hostname invalidates exactly that host once.
        let mut imported = host("host-a");
        imported.hostname = "imported.internal".into();
        let changed = changed_identities(&[host("host-a"), host("host-b")], &[imported, host("host-b")]);
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].0, "host-a");
        let before = fake.invalidations().len();
        for (host_id, identity) in changed {
            let _ = coordinate_identity_change(&fake, &host_id, identity).await;
        }
        assert_eq!(fake.invalidations().len(), before + 1);

        // b5: a bulk write with an unchanged tuple invalidates nothing.
        let before = fake.invalidations().len();
        assert!(changed_identities(&[host("host-b")], &[host("host-b")]).is_empty());
        assert_eq!(fake.invalidations().len(), before);

        // b6: a store write that fails after a successful barrier sends a compensating
        // invalidation carrying the tuple actually on disk.
        let pending = Mutex::new(HashMap::new());
        let on_disk = host_identity(&relabelled);
        let before = fake.invalidations().len();
        let revision = compensate_failed_write(&fake, "host-a", on_disk.clone(), &pending)
            .await
            .expect("the compensation lands");
        let invalidations = fake.invalidations();
        assert_eq!(invalidations.len(), before + 1);
        assert_eq!(invalidations.last().unwrap().identity.as_ref(), Some(&on_disk));
        assert!(revision > 0);
        assert!(pending.lock().unwrap().is_empty());

        // b7: when the compensation itself fails it is remembered, and the next save's
        // barrier is preceded by that retry.
        fake.fail_next_invalidate.store(true, Ordering::SeqCst);
        let stranded = host_identity(&host("host-a"));
        assert!(
            compensate_failed_write(&fake, "host-a", stranded.clone(), &pending).await.is_err()
        );
        assert_eq!(pending.lock().unwrap().get("host-a"), Some(&stranded));
        fake.revisions.lock().unwrap().insert("host-a".into(), 9);
        assert_eq!(retry_pending_compensations(&fake, &pending).await, 1);
        assert!(pending.lock().unwrap().is_empty());
        let invalidations = fake.invalidations();
        assert_eq!(
            invalidations.last().unwrap().identity.as_ref(),
            Some(&stranded),
            "the retry carries the stranded on-disk tuple"
        );
    }

    // (c) The GUI start command's own Windows refusal: platform before capability.
    #[cfg(windows)]
    #[tokio::test]
    async fn windows_start_command_refuses_unsupported_platform() {
        let fake = FakeDaemon::advertised();
        let request = TunnelStartRequest {
            host_id: "host-a".into(),
            spec: Some(spec("one", 5433)),
            spec_id: None,
            host_revision: Some(1),
        };
        let error = start_with(&fake, request).await.expect_err("Windows has no tunnels");
        assert_eq!(error.code, IpcErrorCode::Unsupported);
        let details = error.details.expect("structured refusal");
        assert_eq!(details["failureCode"], UNSUPPORTED_PLATFORM);
        assert_eq!(details["platform"], "windows");
        assert!(fake.calls().is_empty(), "the platform refusal precedes any daemon query");
    }

    // (d) The target-independent precedence oracle: swapping platform and capability
    // order goes red here even on a build that cannot reach the Windows arm.
    #[test]
    fn refuse_platform_before_capability() {
        let advertising = vec![SSH_TUNNEL_CAPABILITY.to_string()];
        // Qualified: this fn's own name shadows the predicate inside the module.
        assert_eq!(
            super::refuse_platform_before_capability(TunnelPlatform::Windows, &advertising),
            Some(Refusal::UnsupportedPlatform),
            "a Windows build must not be masked by an advertised capability"
        );
        assert_eq!(
            super::refuse_platform_before_capability(TunnelPlatform::Windows, &[]),
            Some(Refusal::UnsupportedPlatform),
            "nor by an absent one"
        );
        assert_eq!(
            super::refuse_platform_before_capability(TunnelPlatform::Unix, &advertising),
            None
        );
        assert_eq!(platform_refusal(TunnelPlatform::Windows), Some(Refusal::UnsupportedPlatform));
        assert_eq!(platform_refusal(TunnelPlatform::Unix), None);
        assert_eq!(
            super::refuse_platform_before_capability(TunnelPlatform::Unix, &[]),
            Some(Refusal::MissingCapability)
        );
        let error = Refusal::UnsupportedPlatform.into_error();
        assert_eq!(error.code, IpcErrorCode::Unsupported);
        assert_eq!(error.details.unwrap()["failureCode"], UNSUPPORTED_PLATFORM);
        let error = Refusal::MissingCapability.into_error();
        assert_eq!(error.code, IpcErrorCode::UnsupportedCapability);
        let error = Refusal::RemoteGatewayRoute.into_error();
        assert_eq!(error.details.unwrap()["reason"], REMOTE_GATEWAY_ROUTE);
    }

    // (e) One table-driven fn over the seven commands: payload in, payload out, plus the
    // capability gate refusing all seven when the daemon does not advertise.
    #[tokio::test]
    async fn all_seven_commands_map_payloads() {
        if cfg!(windows) {
            // On Windows every command refuses before forwarding; the rows below are the
            // non-Windows behaviour and are asserted there.
            return;
        }
        let fake = FakeDaemon::advertised();
        fake.revisions.lock().unwrap().insert("host-a".into(), 2);

        let list = list_with(&fake).await.expect("list maps");
        assert_eq!(list.definitions_revision, 1);
        assert_eq!(list.host_revisions[0].revision, 2);
        assert_eq!(list.host_revisions[0].host_id, "host-a");

        let start = start_with(
            &fake,
            TunnelStartRequest {
                host_id: "host-a".into(),
                spec: Some(spec("one", 5433)),
                spec_id: None,
                host_revision: Some(2),
            },
        )
        .await
        .expect("start maps");
        assert_eq!(start.view.host_id, "host-a");
        assert_eq!(start.view.local_port, 5433);
        assert_eq!(start.accepted_revision, 1);

        let stop = stop_with(&fake, TunnelStopRequest { runtime_id: "runtime-1".into() })
            .await
            .expect("stop maps");
        assert!(stop.stopped);

        let invalidated = invalidate_host_with(
            &fake,
            "host-a".to_string(),
            2,
            Some(host_identity(&host("host-a"))),
        )
        .await
        .expect("invalidate maps");
        assert_eq!(invalidated.revision, 3);

        let saved = specs_save_with(&fake, save_request("host-a", vec![spec("one", 5433)]))
            .await
            .expect("save maps");
        assert_eq!(saved.definitions_revision, 1);

        let forgotten = forget_host_with(&fake, "host-a".to_string()).await.expect("forget maps");
        assert_eq!(forgotten.removed, 1);

        *fake.reset.lock().unwrap() = Some(Ok(TunnelSpecsResetResponse {
            quarantined_path: Some("/data/ssh_tunnels.corrupt-1.json".into()),
        }));
        let reset = specs_reset_with(&fake).await.expect("reset maps");
        assert_eq!(reset.quarantined_path.as_deref(), Some("/data/ssh_tunnels.corrupt-1.json"));

        // All seven refuse on an old daemon, and no verb is forwarded: gating only
        // start would leave list/save writing a file that daemon never reads.
        let old = FakeDaemon::without_capability();
        let list_error = list_with(&old).await.expect_err("no capability, no tunnels");
        assert_eq!(list_error.code, IpcErrorCode::UnsupportedCapability);
        assert_eq!(
            start_with(
                &old,
                TunnelStartRequest {
                    host_id: "host-a".into(),
                    spec: Some(spec("one", 5433)),
                    spec_id: None,
                    host_revision: None,
                },
            )
            .await
            .expect_err("start refuses")
            .code,
            IpcErrorCode::UnsupportedCapability
        );
        assert_eq!(
            stop_with(&old, TunnelStopRequest { runtime_id: "r".into() })
                .await
                .unwrap_err()
                .code,
            IpcErrorCode::UnsupportedCapability
        );
        assert_eq!(
            invalidate_host_with(&old, "host-a".to_string(), 1, None).await.unwrap_err().code,
            IpcErrorCode::UnsupportedCapability
        );
        assert_eq!(
            forget_host_with(&old, "host-a".to_string()).await.unwrap_err().code,
            IpcErrorCode::UnsupportedCapability
        );
        assert_eq!(
            specs_save_with(&old, save_request("host-a", vec![spec("one", 5433)]))
                .await
                .unwrap_err()
                .code,
            IpcErrorCode::UnsupportedCapability
        );
        assert_eq!(
            specs_reset_with(&old).await.unwrap_err().code,
            IpcErrorCode::UnsupportedCapability
        );
        assert!(
            old.calls().iter().all(|call| matches!(call, TunnelCall::Capabilities)),
            "a refused command forwards no verb"
        );
    }

    // (f) f1/f2/f3: a host mutation is never hostage to the tunnel service.
    #[tokio::test]
    async fn host_mutation_tears_down_and_marks_host_changed() {
        let identity = host_identity(&host("host-a"));

        // f1: capability absent (old daemon, Windows) - coordination is skipped and the
        // mutation succeeds.
        let old = FakeDaemon::without_capability();
        assert_eq!(coordinate_identity_change(&old, "host-a", identity.clone()).await, None);
        assert!(old.invalidations().is_empty());
        assert_eq!(coordinate_host_forget(&old, "host-a").await, None);
        assert!(old.forgets().is_empty());

        // f2: advertised but failing - the reason is logged, no failure is raised, and the
        // caller's store write is unaffected.
        let failing = FakeDaemon::advertised();
        *failing.invalidate_error.lock().unwrap() = Some(
            IpcError::new(IpcErrorCode::IoError, "daemon busy")
                .with_details(serde_json::json!({ "reason": "transport" })),
        );
        *failing.forget_error.lock().unwrap() =
            Some(IpcError::new(IpcErrorCode::IoError, "daemon busy"));
        assert_eq!(coordinate_identity_change(&failing, "host-a", identity.clone()).await, None);
        assert_eq!(failing.invalidations().len(), 1, "one attempt, then give up");
        assert_eq!(coordinate_host_forget(&failing, "host-a").await, None);
        assert_eq!(failing.forgets().len(), 1);

        // f3: advertised and reachable - the barrier lands and the new revision is
        // returned, so the tunnels of that host are torn down.
        let working = FakeDaemon::advertised();
        working.revisions.lock().unwrap().insert("host-a".into(), 7);
        assert_eq!(coordinate_identity_change(&working, "host-a", identity.clone()).await, Some(8));
        assert_eq!(working.invalidations()[0].host_revision, 7);
        assert_eq!(coordinate_host_forget(&working, "host-a").await, Some(1));

        // A remote-gateway route refuses instead of tearing anything down, and the
        // mutation still succeeds because that refusal is swallowed here.
        let gateway = FakeDaemon {
            capabilities: vec![SSH_TUNNEL_CAPABILITY.to_string()],
            route: Some(TunnelRoute::RemoteGateway),
            ..FakeDaemon::default()
        };
        assert_eq!(coordinate_identity_change(&gateway, "host-a", identity).await, None);
        assert!(gateway.invalidations().is_empty());
        assert_eq!(
            list_with(&gateway).await.unwrap_err().details.unwrap()["reason"],
            REMOTE_GATEWAY_ROUTE
        );
    }

    // (r) reset forwards, returns the daemon's path verbatim, and maps a healthy-file
    // refusal to `store_not_corrupt` rather than quarantining good data. The rename is the
    // daemon arm's work and is not claimed here.
    #[tokio::test]
    async fn reset_forwards_and_maps_quarantined_path() {
        let fake = FakeDaemon::advertised();
        *fake.reset.lock().unwrap() = Some(Ok(TunnelSpecsResetResponse {
            quarantined_path: Some("/data/ssh_tunnels.corrupt-2026.json".into()),
        }));
        let response = specs_reset_with(&fake).await.expect("reset forwards");
        assert_eq!(
            response.quarantined_path.as_deref(),
            Some("/data/ssh_tunnels.corrupt-2026.json")
        );
        assert_eq!(
            fake.calls()
                .iter()
                .filter(|call| matches!(call, TunnelCall::SpecsReset))
                .count(),
            1
        );

        let refusing = FakeDaemon::advertised();
        *refusing.reset.lock().unwrap() = Some(Err(IpcError::new(
            IpcErrorCode::RequestConflict,
            "tunnel store parses",
        )
        .with_details(serde_json::json!({ "reason": STORE_NOT_CORRUPT }))));
        let error = specs_reset_with(&refusing).await.expect_err("a healthy file is not reset");
        assert_eq!(error.code, IpcErrorCode::RequestConflict);
        assert_eq!(error.details.unwrap()["reason"], STORE_NOT_CORRUPT);
    }

    // The no-GUI-write rule, as a source-level assertion: a second writer appearing in
    // this module goes red without needing a daemon to observe it. Needles are split so
    // this test's own text cannot satisfy them.
    #[test]
    fn no_second_store_writer_from_the_gui() {
        let source = include_str!("ssh_tunnel.rs");
        for needle in [
            concat!("std", "::fs"),
            concat!("f", "s::"),
            concat!("Open", "Options"),
            concat!("save", "_store"),
            concat!("File", "::open"),
            concat!("write", "_all"),
        ] {
            assert!(
                !source.contains(needle),
                "ipc::ssh_tunnel must not touch the definitions file; found {needle:?}"
            );
        }
        // Every mutating verb is forwarded to the daemon instead.
        for forwarded in ["SpecsSave", "SpecsReset", "ForgetHost", "InvalidateHost"] {
            assert!(source.contains(forwarded), "missing forward for {forwarded}");
        }
    }

    // The GUI-side host id shape rule: an empty id and a flag-shaped id are refused before
    // any forward, which is the shape check this layer owns.
    #[tokio::test]
    async fn save_with_unknown_host_id_is_counted_prevalidation() {
        for host_id in ["", "   ", "-oProxyCommand=touch /tmp/pwn"] {
            let fake = FakeDaemon::advertised();
            let error = specs_save_with(&fake, save_request(host_id, vec![spec("one", 5433)]))
                .await
                .expect_err("a malformed host id is refused");
            assert_eq!(error.code, IpcErrorCode::InvalidArgument);
            assert_eq!(error.details.unwrap()["reason"], INVALID_HOST_ID);
            assert!(
                fake.calls().is_empty(),
                "an invalid host id must not reach the daemon ({host_id:?})"
            );
        }
    }
}
