//! Supervisor fence ratchet (GAP-4).
//!
//! Only the account's supervisor lease holder may type into agent panes. Every supervisor
//! agent-control request carries the holder's lease fence and the relay account it was issued
//! for (`supervisorFence` / `supervisorAccount` on the daemon wire, the paired send body and
//! `POST supervisor/fence`). The executing daemon keeps the highest fence it has seen per
//! account in `supervisor-fence.json` (0600, atomic) and refuses a lower one with
//! [`STALE_SUPERVISOR_FENCE`] before anything runs; a higher one raises the stored maximum
//! before the request executes. A request without a fence is user or manual control and is
//! never fenced.
//!
//! The sending side asks a [`SupervisorFenceSource`] (the daemon lease client) right before
//! every supervisor input: a machine that is not the holder now refuses the send instead of
//! stamping a fence.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};

use serde::{Deserialize, Serialize};

/// Wire code of a refused supervisor request: its fence is below the account's stored maximum,
/// or the sender is not the lease holder now. Nothing was executed or typed.
pub const STALE_SUPERVISOR_FENCE: &str = ferryx_hub::agent_control::STALE_SUPERVISOR_FENCE;

/// Wire code of a fenced request whose higher fence could not be stored; nothing ran.
pub const SUPERVISOR_FENCE_UNAVAILABLE: &str =
    ferryx_hub::agent_control::SUPERVISOR_FENCE_UNAVAILABLE;

/// The durable per-account maximum, beside the agent-control settings.
pub const SUPERVISOR_FENCE_FILE: &str = "supervisor-fence.json";

/// Advertised by a daemon that serves `POST supervisor/fence` and ratchets fenced requests.
pub const SUPERVISOR_FENCE_CAPABILITY: &str = "supervisorFenceV1";

/// One supervisor lease fence and the relay account user id it was issued for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupervisorFence {
    pub account: String,
    pub fence: u64,
}

impl SupervisorFence {
    /// The fence a request carries, from its two optional wire fields. Neither is a request
    /// without a fence (never fenced); a fence without its account is malformed.
    pub fn from_wire(
        fence: Option<u64>,
        account: Option<String>,
    ) -> Result<Option<Self>, FenceRefusal> {
        match (fence, account) {
            (None, None) => Ok(None),
            (Some(fence), Some(account)) if !account.trim().is_empty() => Ok(Some(Self {
                account: account.trim().to_owned(),
                fence,
            })),
            _ => Err(FenceRefusal::Invalid(
                "a supervisor fence needs both supervisorFence and supervisorAccount",
            )),
        }
    }
}

/// Why a fenced request was not executed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FenceRefusal {
    /// The request's fence is below the account's stored maximum.
    Stale { fence: u64, max: u64 },
    /// This machine is not the supervisor lease holder right now; nothing was sent.
    NotHolder,
    /// The fence fields are malformed.
    Invalid(&'static str),
    /// A higher fence could not be stored, so the request was not executed.
    NotPersisted(String),
}

impl FenceRefusal {
    pub fn code(&self) -> &'static str {
        match self {
            FenceRefusal::Stale { .. } | FenceRefusal::NotHolder => STALE_SUPERVISOR_FENCE,
            FenceRefusal::Invalid(_) => "INVALID_REQUEST",
            FenceRefusal::NotPersisted(_) => SUPERVISOR_FENCE_UNAVAILABLE,
        }
    }

    pub fn message(&self) -> String {
        match self {
            FenceRefusal::Stale { fence, max } => format!(
                "supervisor fence {fence} is older than {max}; another machine holds the supervisor lease"
            ),
            FenceRefusal::NotHolder => {
                "this machine does not hold the supervisor lease now; nothing was sent".to_owned()
            }
            FenceRefusal::Invalid(why) => (*why).to_owned(),
            FenceRefusal::NotPersisted(why) => {
                format!("the supervisor fence could not be stored; nothing was executed: {why}")
            }
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedFences {
    #[serde(default)]
    max_fence: BTreeMap<String, u64>,
}

fn read_fences(path: &Path) -> BTreeMap<String, u64> {
    match std::fs::read(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "supervisor fences are unreadable; the next holder's fence push raises them again");
            BTreeMap::new()
        }
        Ok(bytes) => match serde_json::from_slice::<PersistedFences>(&bytes) {
            Ok(saved) => saved.max_fence,
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "supervisor fences are unreadable; the next holder's fence push raises them again");
                BTreeMap::new()
            }
        },
    }
}

/// The executing daemon's durable fence maximum per account.
#[derive(Debug)]
pub struct FenceRatchet {
    /// `None` keeps the maximum in memory only (legacy and test gateways).
    path: Option<PathBuf>,
    /// Held across the store write, so two fenced requests ratchet one after the other.
    max: tokio::sync::Mutex<BTreeMap<String, u64>>,
}

impl FenceRatchet {
    /// The maximum saved at `path`. A missing file is a fresh install.
    pub fn load(path: Option<PathBuf>) -> Self {
        let max = path.as_deref().map(read_fences).unwrap_or_default();
        Self {
            path,
            max: tokio::sync::Mutex::new(max),
        }
    }

    /// The stored maximum for `account` (0 when none was seen).
    pub async fn max_for(&self, account: &str) -> u64 {
        self.max.lock().await.get(account).copied().unwrap_or(0)
    }

    /// Admit one fenced request before it executes: a fence below the stored maximum is
    /// [`FenceRefusal::Stale`]; a higher one is stored (durably, when this ratchet has a path)
    /// before `Ok` is returned; an equal one passes unchanged.
    pub async fn admit(&self, fence: &SupervisorFence) -> Result<(), FenceRefusal> {
        let account = fence.account.trim();
        if account.is_empty() {
            return Err(FenceRefusal::Invalid("a supervisor fence names its account"));
        }
        let mut max = self.max.lock().await;
        let current = max.get(account).copied().unwrap_or(0);
        if fence.fence < current {
            return Err(FenceRefusal::Stale {
                fence: fence.fence,
                max: current,
            });
        }
        if fence.fence == current {
            return Ok(());
        }
        let mut next = max.clone();
        next.insert(account.to_owned(), fence.fence);
        if let Some(path) = self.path.clone() {
            let saved = PersistedFences {
                max_fence: next.clone(),
            };
            crate::ipc::run_blocking(move || {
                crate::remote::auth::write_private_json(&path, &saved)
                    .map_err(crate::ipc::error::IpcError::internal)
            })
            .await
            .map_err(|error| FenceRefusal::NotPersisted(error.to_string()))?;
        }
        *max = next;
        Ok(())
    }
}

/// What the sending side knows about its lease right now (the daemon lease client, plan
/// task 3, implements this).
pub trait SupervisorFenceSource: Send + Sync {
    /// The relay account user id the lease was issued for; `None` on an account-less or
    /// self-hosted install, whose requests stay unfenced (today's behavior).
    fn account(&self) -> Option<String>;
    /// Whether this machine holds the lease now, re-checked against both clocks.
    fn is_holder_now(&self) -> bool;
    /// The fence of the lease this machine holds.
    fn current_fence(&self) -> u64;
    /// An executing daemon refused this machine's fence: give the lease up now.
    fn stand_down(&self) {}
}

/// The fence one supervisor input is stamped with, asked right before the send. `Ok(None)`
/// without a source or account (unfenced, today's behavior); [`FenceRefusal::NotHolder`] when
/// the lease is not held now, so nothing is sent.
pub fn stamp(
    source: Option<&dyn SupervisorFenceSource>,
) -> Result<Option<SupervisorFence>, FenceRefusal> {
    let Some(source) = source else {
        return Ok(None);
    };
    let Some(account) = source.account().filter(|account| !account.trim().is_empty()) else {
        return Ok(None);
    };
    if !source.is_holder_now() {
        return Err(FenceRefusal::NotHolder);
    }
    Ok(Some(SupervisorFence {
        account,
        fence: source.current_fence(),
    }))
}

fn installed() -> &'static RwLock<Option<Arc<dyn SupervisorFenceSource>>> {
    static SOURCE: OnceLock<RwLock<Option<Arc<dyn SupervisorFenceSource>>>> = OnceLock::new();
    SOURCE.get_or_init(|| RwLock::new(None))
}

/// Installs this process's lease source (the daemon wires the lease client here once).
pub fn install_source(source: Option<Arc<dyn SupervisorFenceSource>>) {
    *installed()
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = source;
}

/// [`stamp`] against the installed source.
pub fn stamp_now() -> Result<Option<SupervisorFence>, FenceRefusal> {
    let source = installed()
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    stamp(source.as_deref())
}

/// The fence a supervisor agent-control request is stamped with: only input (send, deliver,
/// answer, stop) is fenced; reads go out unstamped.
pub fn stamp_for(
    op: &crate::agent_control::AgentControlOp,
) -> Result<Option<SupervisorFence>, FenceRefusal> {
    if op.is_input_mutation() {
        stamp_now()
    } else {
        Ok(None)
    }
}

/// The two optional daemon-wire fields of a fence (`supervisorFence`, `supervisorAccount`).
pub fn wire(fence: Option<SupervisorFence>) -> (Option<u64>, Option<String>) {
    match fence {
        Some(fence) => (Some(fence.fence), Some(fence.account)),
        None => (None, None),
    }
}

/// Stamp a supervisor input and admit it on this machine's own daemon (the in-process
/// gateway), right before it is handed to the pane.
pub async fn admit_stamped(
    gateway: &crate::remote::state::RemoteGatewayState,
) -> Result<(), FenceRefusal> {
    match stamp_now()? {
        Some(fence) => gateway.supervisor_fence.admit(&fence).await,
        None => Ok(()),
    }
}

/// Pushes the holder's fence to this machine's daemon (in process) and to paired hosts
/// (`POST supervisor/fence` over the paired channel), so a machine reached later cannot accept
/// the previous holder's lower fence.
pub struct DaemonFencePush {
    client: crate::daemon::client::DaemonClient,
    gateway: Option<Arc<crate::remote::state::RemoteGatewayState>>,
}

#[async_trait::async_trait]
impl ferryx_hub::supervisor::fence_gate::FencePush for DaemonFencePush {
    async fn hosts(&self) -> Vec<String> {
        use crate::paired_host::inventory::{AuthStatus, GrantScope};
        // Unfenced (account-less, self-hosted, no lease client yet): nothing to push.
        if !matches!(stamp_now(), Ok(Some(_))) {
            return Vec::new();
        }
        match self.client.paired_host_list().await {
            Ok(hosts) => hosts
                .into_iter()
                .filter(|host| {
                    host.grant_scope == GrantScope::Machine && host.auth_status == AuthStatus::Paired
                })
                .map(|host| host.host_id)
                .collect(),
            Err(error) => {
                tracing::warn!(code = %error.code, "paired hosts were not listed for the supervisor fence push; each is pushed before its first dispatch");
                Vec::new()
            }
        }
    }

    fn stand_down(&self) {
        let source = installed()
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if let Some(source) = source {
            source.stand_down();
        }
    }

    async fn push_fence(&self, host_id: Option<&str>) -> Result<(), String> {
        let fence = match stamp_now() {
            Ok(Some(fence)) => fence,
            // Account-less or self-hosted: nothing is fenced, so nothing is pushed.
            Ok(None) => return Ok(()),
            Err(refusal) => return Err(format!("{} {}", refusal.code(), refusal.message())),
        };
        let Some(host_id) = host_id else {
            let Some(gateway) = self.gateway.as_ref() else {
                return Err("no in-process daemon to fence".to_owned());
            };
            return gateway
                .supervisor_fence
                .admit(&fence)
                .await
                .map_err(|refusal| format!("{} {}", refusal.code(), refusal.message()));
        };
        let hosts = self
            .client
            .paired_host_list()
            .await
            .map_err(|error| format!("{} {}", error.code, error.message))?;
        let host = hosts
            .into_iter()
            .find(|host| host.host_id == host_id)
            .ok_or_else(|| format!("NOT_FOUND no paired host is named {host_id}"))?;
        let request = crate::paired_host::client::OperationRequest {
            host_id: host.host_id,
            generation: host.generation,
            operation: crate::paired_host::client::Operation::SupervisorFence {
                account: fence.account,
                fence: fence.fence,
            },
        };
        self.client
            .paired_host_operation(request)
            .await
            .map(|_| ())
            .map_err(|error| {
                error
                    .machine_error
                    .as_ref()
                    .map(|machine| format!("{} {}", machine.code, machine.message))
                    .unwrap_or(error.code)
            })
    }
}

/// The dispatcher's fence gate for one holder term (one hub generation). The hub runtime
/// passes it to `Dispatcher::with_fence_gate` when it spawns the dispatcher.
pub fn dispatcher_fence_gate(
    client: crate::daemon::client::DaemonClient,
    gateway: Option<Arc<crate::remote::state::RemoteGatewayState>>,
) -> Arc<ferryx_hub::supervisor::fence_gate::FenceGate> {
    Arc::new(ferryx_hub::supervisor::fence_gate::FenceGate::new(
        Arc::new(DaemonFencePush { client, gateway }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fence(account: &str, fence: u64) -> SupervisorFence {
        SupervisorFence {
            account: account.into(),
            fence,
        }
    }

    #[tokio::test]
    async fn a_lower_fence_is_refused_and_an_equal_or_higher_one_passes() {
        let ratchet = FenceRatchet::load(None);
        ratchet.admit(&fence("user-1", 2)).await.unwrap();
        assert_eq!(
            ratchet.admit(&fence("user-1", 1)).await,
            Err(FenceRefusal::Stale { fence: 1, max: 2 })
        );
        ratchet.admit(&fence("user-1", 2)).await.unwrap();
        ratchet.admit(&fence("user-1", 3)).await.unwrap();
        assert_eq!(ratchet.max_for("user-1").await, 3);
        // Another account has its own maximum.
        ratchet.admit(&fence("user-2", 1)).await.unwrap();
    }

    #[tokio::test]
    async fn the_maximum_survives_a_reload_and_the_file_is_private() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SUPERVISOR_FENCE_FILE);
        FenceRatchet::load(Some(path.clone()))
            .admit(&fence("user-1", 7))
            .await
            .unwrap();
        let reloaded = FenceRatchet::load(Some(path.clone()));
        assert_eq!(reloaded.max_for("user-1").await, 7);
        assert_eq!(
            reloaded.admit(&fence("user-1", 6)).await,
            Err(FenceRefusal::Stale { fence: 6, max: 7 })
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[test]
    fn wire_fields_need_each_other() {
        assert_eq!(SupervisorFence::from_wire(None, None), Ok(None));
        assert_eq!(
            SupervisorFence::from_wire(Some(4), Some("user-1".into())),
            Ok(Some(fence("user-1", 4)))
        );
        assert!(SupervisorFence::from_wire(Some(4), None).is_err());
        assert!(SupervisorFence::from_wire(None, Some("user-1".into())).is_err());
        assert!(SupervisorFence::from_wire(Some(4), Some(" ".into())).is_err());
    }

    /// A lease whose deadline is measured on a wall clock that already passed it, with no tick
    /// since: the source answers from the clocks at the call, not from its last tick.
    struct PastDeadline {
        deadline: std::time::SystemTime,
    }

    impl SupervisorFenceSource for PastDeadline {
        fn account(&self) -> Option<String> {
            Some("user-1".into())
        }
        fn is_holder_now(&self) -> bool {
            std::time::SystemTime::now() < self.deadline
        }
        fn current_fence(&self) -> u64 {
            5
        }
    }

    struct Unleased;

    impl SupervisorFenceSource for Unleased {
        fn account(&self) -> Option<String> {
            None
        }
        fn is_holder_now(&self) -> bool {
            true
        }
        fn current_fence(&self) -> u64 {
            0
        }
    }

    #[test]
    fn a_send_is_refused_unless_the_lease_is_held_now() {
        let expired = PastDeadline {
            deadline: std::time::SystemTime::now() - std::time::Duration::from_secs(1),
        };
        assert_eq!(
            stamp(Some(&expired as &dyn SupervisorFenceSource)),
            Err(FenceRefusal::NotHolder)
        );
        let held = PastDeadline {
            deadline: std::time::SystemTime::now() + std::time::Duration::from_secs(3600),
        };
        assert_eq!(
            stamp(Some(&held as &dyn SupervisorFenceSource)),
            Ok(Some(fence("user-1", 5)))
        );
        assert_eq!(stamp(Some(&Unleased as &dyn SupervisorFenceSource)), Ok(None));
        assert_eq!(stamp(None), Ok(None));
    }
}
